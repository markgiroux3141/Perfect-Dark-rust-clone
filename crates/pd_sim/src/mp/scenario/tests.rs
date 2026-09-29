//! The scenarios on Complex, headless: each one's props at the start, the
//! player's side of its rules (with PD's messages), and simulants playing it
//! to a score.

use std::collections::BTreeSet;
use std::sync::Arc;

use glam::Vec3;
use pd_core::ids::*;
use pd_core::mp::MatchSetup;

use super::PropRef;
use crate::harness::{self, NavChoice, SPIKE_SEED};
use crate::lights::LIGHTOP_HIGHLIGHT;
use crate::player::PlayerInput;
use crate::stage::{Stage, TileLevel};
use crate::testutil::{complex_arc, res};
use crate::world::World;

/// `scenario` on `stage` with `players` humans and `bots` NormalSims, on
/// teams `teams` (chr slot order) when given; no limits.
fn scenario_world_on(stage: Arc<Stage>, level: Arc<TileLevel>, scenario: u8, players: usize, bots: usize, teams: Option<&[u8]>, seed: u64) -> World {
    let mut setup = MatchSetup { stagenum: stage.stagenum, scenario, ..harness::setup(players, bots, BOTDIFF_NORMAL) };
    setup = harness::with_weapons(setup, &harness::DEFAULT_SET);
    if let Some(t) = teams {
        setup.options |= MPOPTION_TEAMSENABLED;
        for (k, p) in setup.players.iter_mut().enumerate() {
            p.chr.team = t[k];
        }
        for (k, s) in setup.simulants.iter_mut().enumerate() {
            s.chr.team = t[players + k];
        }
    }
    harness::world(stage, level, res(), setup, NavChoice::Pd, seed, false).unwrap()
}

fn scenario_world(scenario: u8, players: usize, bots: usize, teams: Option<&[u8]>) -> World {
    let (stage, level) = complex_arc();
    scenario_world_on(stage, level, scenario, players, bots, teams, SPIKE_SEED)
}

/// Every HUD message player `pi` has had, as the frames go by.
#[derive(Default)]
struct Seen(BTreeSet<String>);

impl Seen {
    fn look(&mut self, w: &World, pi: usize) {
        for m in w.mp.hudmsgs.msgs.iter().filter(|m| m.playernum == pi && m.state != HUDMSGSTATE_FREE) {
            self.0.insert(m.text.clone());
        }
    }

    fn has(&self, s: &str) -> bool {
        self.0.iter().any(|t| t.contains(s))
    }
}

fn step_n(w: &mut World, input: &PlayerInput, n: usize, seen: &mut Seen) {
    for _ in 0..n {
        w.step(4, std::slice::from_ref(input));
        seen.look(w, 0);
    }
}

/// The floor under `p`.
fn floor(w: &World, p: Vec3) -> Vec3 {
    Vec3::new(p.x, w.level.cd_find_ground_at_cyl(p + Vec3::Y * 20.0, 30.0).0, p.z)
}

/// Each scenario's props as `setup_create_props` leaves them: the case in a
/// crate's place, the terminal and the uplink, the victims' order, the hill
/// lit, a case on each playing team's home pad with its base lit.
#[test]
fn each_scenario_places_its_props() {
    let w = scenario_world(MPSCENARIO_HOLDTHEBRIEFCASE, 1, 3, None);
    let Some(PropRef::Obj(id)) = w.mp.scenariodata.htb.token else { panic!("no case: {:?}", w.mp.scenariodata.htb.token) };
    let case = w.props.get(id).unwrap();
    assert_eq!((case.ty, case.weaponnum, case.hidden2 & OBJH2FLAG_CANREGEN), (OBJTYPE_WEAPON, WEAPON_BRIEFCASE2, 0));
    let crate_id = w.mp.scenariodata.replacedcrate.expect("a crate replaced");
    let crate_ = w.props.get(crate_id).unwrap();
    assert_eq!((crate_.pad, crate_.is_deleting()), (case.pad, true));

    let w = scenario_world(MPSCENARIO_HACKERCENTRAL, 1, 3, None);
    let d = &w.mp.scenariodata.htm;
    let term = w.props.get(d.terminals[0].prop.expect("a terminal")).unwrap();
    assert_eq!((term.modelnum, term.flags3 & OBJFLAG3_HTMTERMINAL != 0), (MODEL_GOODPC, true));
    assert_eq!(d.numterminals, 2, "PD draws a pad for the uplink too");
    let Some(PropRef::Obj(up)) = d.uplink else { panic!("no uplink") };
    assert_eq!(w.props.get(up).unwrap().weaponnum, WEAPON_DATAUPLINK);

    let w = scenario_world(MPSCENARIO_POPACAP, 1, 3, None);
    let mut v: Vec<i16> = w.mp.scenariodata.pac.victims[..4].to_vec();
    v.sort();
    assert_eq!((v, w.mp.scenariodata.pac.victimindex), (vec![0, 1, 2, 3], -1));

    let w = scenario_world(MPSCENARIO_KINGOFTHEHILL, 1, 3, Some(&[0, 1, 0, 1]));
    let hill = w.mp.scenariodata.koh.hillroom.expect("a hill");
    assert_eq!(w.lights.rooms[hill as usize].lightop, LIGHTOP_HIGHLIGHT);
    let k = &w.mp.scenariodata.koh;
    assert_eq!(k.hillcount, 5, "Complex has five hills");
    assert_eq!(w.stage.pads[k.hillpads[k.hillindex as usize] as usize].room, Some(hill));

    let w = scenario_world(MPSCENARIO_CAPTURETHECASE, 1, 3, Some(&[0, 1, 0, 1]));
    let d = &w.mp.scenariodata.ctc;
    assert_eq!(d.playercountsperteam, [2, 2, 0, 0]);
    for t in 0..2 {
        let Some(PropRef::Obj(id)) = d.tokens[t] else { panic!("team {t} has no case") };
        let o = w.props.get(id).unwrap();
        let base = d.spawnpadsperteam[d.teamindexes[t] as usize];
        assert_eq!((o.team as usize, o.pad), (t, base.homepad as i32));
        let room = d.baserooms[t].expect("a base room");
        assert_eq!(w.lights.rooms[room as usize].lightop, LIGHTOP_HIGHLIGHT);
    }
    assert_eq!((d.tokens[2], d.teamindexes[2]), (None, -1));
}

/// Hold the Briefcase: walking onto the case takes it ("Picked up the
/// Briefcase."), shows the 0:30 countdown, refuses shields, scores "1 Point!"
/// after 30 s; the carrier's death drops the case where it fell.
#[test]
fn holding_the_briefcase_scores_a_point_every_30_seconds() {
    let mut w = scenario_world(MPSCENARIO_HOLDTHEBRIEFCASE, 1, 1, None);
    w.bot_brains = false;
    let mut seen = Seen::default();
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    let Some(PropRef::Obj(id)) = w.mp.scenariodata.htb.token else { panic!() };
    let at = floor(&w, w.props.get(id).unwrap().pos);
    harness::place_player(&mut w, 0, at, 0.0);
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    assert_eq!(w.mp.scenariodata.htb.token, Some(PropRef::Chr(0)));
    assert!(w.inv_has_briefcase(0));
    assert!(seen.has("Briefcase"), "{:?}", seen.0);
    assert_eq!(w.scenario_hud(0), super::ScenarioHud::Countdown { text: "0:30".into() });
    let start = w.mp.chrs[0].numpoints;
    step_n(&mut w, &PlayerInput::default(), 30 * 60, &mut seen);
    assert_eq!(w.mp.chrs[0].numpoints, start + 1);
    assert!(seen.has("1 Point!"), "{:?}", seen.0);
    let s = w.scenario_scores();
    assert_eq!(w.mp_scoring(&s).scenario_calculate_player_score(0).0, 1);

    // A shield is refused while carrying.
    let shield = crate::props::Obj { ty: OBJTYPE_SHIELD, shieldamount: 1.0, pos: w.players[0].pos, ..w.props.get(w.props.objs[0].id).unwrap().clone() };
    let sid = w.props.alloc_id();
    w.props.objs.push(crate::props::Obj { id: sid, ..shield });
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    assert!(w.props.get(sid).is_some_and(|o| !o.is_gone()), "the carrier took a shield");

    // Dying drops it.
    w.player_die_by_shooter(0, Some(1));
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    let Some(PropRef::Obj(dropped)) = w.mp.scenariodata.htb.token else { panic!("{:?}", w.mp.scenariodata.htb.token) };
    assert_eq!(w.props.get(dropped).unwrap().weaponnum, WEAPON_BRIEFCASE2);
}

/// Hacker Central: the player takes the uplink, holds it (from the pause
/// menu's inventory), faces the terminal and uses it ("Starting download."),
/// and 20 s later has two points ("Download successful."). Walking away
/// breaks the link.
#[test]
fn downloading_at_the_terminal_scores_two_points() {
    let mut w = scenario_world(MPSCENARIO_HACKERCENTRAL, 1, 1, None);
    w.bot_brains = false;
    let mut seen = Seen::default();
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    let Some(PropRef::Obj(up)) = w.mp.scenariodata.htm.uplink else { panic!() };
    let at = floor(&w, w.props.get(up).unwrap().pos);
    harness::place_player(&mut w, 0, at, 0.0);
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    assert_eq!(w.mp.scenariodata.htm.uplink, Some(PropRef::Chr(0)));
    step_n(&mut w, &PlayerInput { select: Some((WEAPON_DATAUPLINK, false)), ..Default::default() }, 1, &mut seen);
    step_n(&mut w, &PlayerInput::default(), 90, &mut seen);
    assert_eq!(w.players[0].gun.bgun_get_weapon_num(HAND_RIGHT), WEAPON_DATAUPLINK);
    // Stand 1 m from the terminal facing it.
    let tp = w.props.get(w.mp.scenariodata.htm.terminals[0].prop.unwrap()).unwrap().pos;
    let spot = w.stage.pads.iter().map(|p| floor(&w, p.pos)).filter(|p| (p.y - tp.y).abs() < 150.0 && p.distance(Vec3::new(tp.x, p.y, tp.z)) > 60.0).min_by(|a, b| a.distance(tp).total_cmp(&b.distance(tp))).unwrap();
    let theta = crate::bot::tests::theta_towards(spot, tp);
    harness::place_player(&mut w, 0, spot, theta);
    step_n(&mut w, &PlayerInput { use_held: true, ..Default::default() }, 3, &mut seen);
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    assert!(seen.has("Starting download."), "{:?} at {spot} for {tp}", seen.0);
    assert!(matches!(w.scenario_hud(0), super::ScenarioHud::DownloadBar { .. }));
    step_n(&mut w, &PlayerInput::default(), 21 * 60, &mut seen);
    assert!(seen.has("Download successful."), "{:?}", seen.0);
    assert_eq!(w.mp.scenariodata.htm.numpoints[0], 1);
    let s = w.scenario_scores();
    assert_eq!(w.mp_scoring(&s).scenario_calculate_player_score(0).0, 2);
}

/// Pop a Cap: the first victim is named on the first frame; a living victim
/// scores a point a minute; killing the victim scores the killer two and names
/// the next.
#[test]
fn popping_the_victims_cap_scores_two() {
    let mut w = scenario_world(MPSCENARIO_POPACAP, 1, 1, None);
    w.bot_brains = false;
    let mut seen = Seen::default();
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    let v = w.pac_victim().expect("a victim");
    assert!(seen.has(if v == 0 { "You are the victim!" } else { "Get Sim 1" }), "{:?}", seen.0);
    step_n(&mut w, &PlayerInput::default(), 61 * 60, &mut seen);
    assert_eq!(w.mp.scenariodata.pac.survivalcounts[v], 1);
    let killer = 1 - v;
    w.mpstats_record_death(killer as i32, v as i32);
    assert_eq!(w.mp.scenariodata.pac.killcounts[killer], 1);
    assert_eq!(w.pac_victim(), Some(killer));
    let s = w.scenario_scores();
    let killerslot = w.chrs[killer].mpslot;
    assert_eq!(w.mp_scoring(&s).scenario_calculate_player_score(killerslot).0, 2 + 1);
}

/// King of the Hill: the player alone in the hill takes it ("We have the
/// Hill!", the hill sound) and after the hill time (10 + 10 s) scores "King of
/// the Hill!"; with Mobile Hill the hill then moves.
#[test]
fn holding_the_hill_scores_and_the_hill_moves() {
    let mut w = scenario_world(MPSCENARIO_KINGOFTHEHILL, 1, 1, Some(&[0, 1]));
    w.bot_brains = false;
    let mut seen = Seen::default();
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    let hill = w.mp.scenariodata.koh.hillroom.unwrap();
    let pos = w.mp.scenariodata.koh.hillpos;
    harness::place_player(&mut w, 0, pos, 0.0);
    step_n(&mut w, &PlayerInput::default(), 3, &mut seen);
    assert_eq!(w.chrs[0].rooms.first().copied(), Some(hill), "not in the hill room");
    assert_eq!(w.mp.scenariodata.koh.occupiedteam, 0);
    assert!(seen.has("We have\nthe Hill!"), "{:?}", seen.0);
    assert_eq!(w.scenario_hud(0), super::ScenarioHud::Countdown { text: "20".into() });
    step_n(&mut w, &PlayerInput::default(), 20 * 60 + 5, &mut seen);
    assert!(seen.has("King of\nthe Hill!"), "{:?}", seen.0);
    assert_eq!(w.mp.chrs[0].numpoints, 1);
    assert!(w.mp.scenariodata.koh.movehill);
    // The old hill fades to white, then another is chosen and lit.
    step_n(&mut w, &PlayerInput::default(), 120, &mut seen);
    let newhill = w.mp.scenariodata.koh.hillroom.unwrap();
    assert!(!w.mp.scenariodata.koh.movehill);
    assert_ne!(w.mp.scenariodata.koh.hillindex, -1);
    assert_eq!(w.lights.rooms[newhill as usize].lightop, LIGHTOP_HIGHLIGHT);
    if newhill != hill {
        assert_ne!(w.lights.rooms[hill as usize].lightop, LIGHTOP_HIGHLIGHT);
    }
}

/// Capture the Case: the player takes the enemy case ("Got the ... "), and
/// touching its own case scores ("You captured ..."), the enemy case going
/// home.
#[test]
fn capturing_the_case_scores_three() {
    let mut w = scenario_world(MPSCENARIO_CAPTURETHECASE, 1, 1, Some(&[0, 1]));
    w.bot_brains = false;
    let mut seen = Seen::default();
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    let Some(PropRef::Obj(enemy)) = w.mp.scenariodata.ctc.tokens[1] else { panic!() };
    let Some(PropRef::Obj(own)) = w.mp.scenariodata.ctc.tokens[0] else { panic!() };
    let enemyhome = w.props.get(enemy).unwrap().pos;
    let at = floor(&w, enemyhome);
    harness::place_player(&mut w, 0, at, 0.0);
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    assert_eq!(w.mp.scenariodata.ctc.tokens[1], Some(PropRef::Chr(0)));
    assert!(seen.has("Got the"), "{:?}", seen.0);
    let at = floor(&w, w.props.get(own).unwrap().pos);
    harness::place_player(&mut w, 0, at, 0.0);
    step_n(&mut w, &PlayerInput::default(), 2, &mut seen);
    assert!(seen.has("You captured"), "{:?}", seen.0);
    assert_eq!(w.mp.chrs[0].numpoints, 1);
    let Some(PropRef::Obj(back)) = w.mp.scenariodata.ctc.tokens[1] else { panic!("{:?}", w.mp.scenariodata.ctc.tokens) };
    assert!(w.props.get(back).unwrap().pos.distance(enemyhome) < 30.0, "the case isn't home");
    let s = w.scenario_scores();
    assert_eq!(w.mp_scoring(&s).scenario_calculate_player_score(0).0, 3);
}

/// The scenario's own points scored by every chr slot, summed: the
/// mpchrconfigs' points, the downloads, the caps popped and minutes survived.
fn scenario_points(w: &World) -> i32 {
    let d = &w.mp.scenariodata;
    let points: i32 = w.mp.chrs.iter().map(|c| c.numpoints as i32).sum();
    points + d.htm.numpoints.iter().sum::<i32>() + d.pac.killcounts.iter().map(|&k| k as i32).sum::<i32>() + d.pac.survivalcounts.iter().map(|&k| k as i32).sum::<i32>()
}

/// NormalSims play `scenario` on `stage` for `seconds`, with `players` idle
/// humans, on `teams` (chr slot order) if given: the points scored.
#[allow(clippy::too_many_arguments)]
fn simulants_score(stage: Arc<Stage>, level: Arc<TileLevel>, scenario: u8, players: usize, bots: usize, teams: Option<&[u8]>, seconds: usize, seed: u64) -> i32 {
    let mut w = scenario_world_on(stage, level, scenario, players, bots, teams, seed);
    let idle = vec![PlayerInput::default(); players];
    for _ in 0..seconds * 60 {
        w.step(4, &idle);
    }
    scenario_points(&w)
}

/// Simulants play every scenario on Complex and score: four of them fetch
/// and hold the briefcase, pop the victim's cap and (two teams of two) hold
/// the hill; a pair on one team fetches the uplink and downloads at the
/// terminal (four at odds break every download off: the downloader turns on
/// whoever shoots it); a pair steals an idle human's team's case and captures
/// it (two teams of simulants tend to stand off, each carrier at home waiting
/// for its own case back). Hacker Central runs on seed 1: the spike's seed
/// puts the terminal on pad 13, a case pad in Complex's pit (room 42, 2.8 m
/// under the floor), where no simulant's route leads. Hold the Briefcase runs
/// on [`harness::M12_SEED`].
#[test]
fn simulants_play_every_scenario() {
    let runs: [(u8, usize, usize, Option<&[u8]>, usize, u64); 5] = [
        (MPSCENARIO_HOLDTHEBRIEFCASE, 0, 4, None, 120, harness::M12_SEED),
        (MPSCENARIO_HACKERCENTRAL, 0, 2, Some(&[0, 0]), 180, harness::M10_SEED),
        (MPSCENARIO_POPACAP, 0, 4, None, 120, SPIKE_SEED),
        (MPSCENARIO_KINGOFTHEHILL, 0, 4, Some(&[0, 1, 0, 1]), 120, SPIKE_SEED),
        (MPSCENARIO_CAPTURETHECASE, 1, 2, Some(&[0, 1, 1]), 180, SPIKE_SEED),
    ];
    let mut failed = Vec::new();
    for (scenario, players, bots, teams, seconds, seed) in runs {
        let (stage, level) = complex_arc();
        let score = simulants_score(stage, level, scenario, players, bots, teams, seconds, seed);
        println!("scenario {scenario}: {score} points in {seconds} s");
        if score == 0 {
            failed.push(scenario);
        }
    }
    assert!(failed.is_empty(), "no points in scenarios {failed:?}");
}

/// A long probe: every scenario on every arena, four NormalSims, the points
/// each scores in three minutes.
#[test]
#[ignore]
fn probe_every_arena_every_scenario() {
    for code in crate::stage::ARENAS {
        let stage = Arc::new(Stage::load(&crate::testutil::assets(), code).unwrap());
        let level = Arc::new(TileLevel::for_stage(&stage));
        let teams = |s: u8| matches!(s, MPSCENARIO_KINGOFTHEHILL | MPSCENARIO_CAPTURETHECASE).then_some(&[0u8, 1, 0, 1][..]);
        let scores: Vec<i32> = [MPSCENARIO_HOLDTHEBRIEFCASE, MPSCENARIO_HACKERCENTRAL, MPSCENARIO_POPACAP, MPSCENARIO_KINGOFTHEHILL, MPSCENARIO_CAPTURETHECASE].iter().map(|&s| simulants_score(stage.clone(), level.clone(), s, 0, 4, teams(s), 180, SPIKE_SEED)).collect();
        println!("{code:5} htb {:3} htm {:3} pac {:3} koh {:3} ctc {:3}", scores[0], scores[1], scores[2], scores[3], scores[4]);
    }
}

