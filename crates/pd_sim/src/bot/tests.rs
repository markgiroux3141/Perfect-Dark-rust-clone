//! Simulants on Complex, headless: the spike's bot tests (`pd_spike/tests.rs`)
//! and the Complex fight's (`pd_complex/tests.rs`) on a [`World`], the A/B
//! baseline, and a seeded match run twice.
//!
//! `cargo test --release -p pd_sim bot::tests`; the probes:
//! `cargo test --release -p pd_sim bot::tests::probe -- --ignored --nocapture`.

use glam::{Vec2, Vec3};
use pd_core::ids::*;

use crate::chr::Act;
use crate::harness::{self, abtest, NavChoice, SPIKE_MIX, SPIKE_SEED};
use crate::player::PlayerInput;
use crate::stage::{Stage, TileLevel};
use crate::testutil::{complex, complex_arc, res};
use crate::world::World;

/// A match on Complex with `players` humans and `bots` simulants of
/// `difficulty`, carrying the spike's mix, routing on PD's graph.
pub(crate) fn complex_world(players: usize, bots: usize, difficulty: u8, seed: u64) -> World {
    let (stage, level) = complex_arc();
    harness::world(stage, level, res(), harness::setup(players, bots, difficulty), NavChoice::Pd, seed, true).unwrap()
}

/// The floor under `p` (a 30 cm cylinder).
fn ground(level: &TileLevel, p: Vec3) -> Vec3 {
    Vec3::new(p.x, level.cd_find_ground_at_cyl(p, 30.0).0, p.z)
}

/// `vv_theta` (degrees; forward = (−sin, 0, cos)) that faces from `a` to `b`.
pub(crate) fn theta_towards(a: Vec3, b: Vec3) -> f32 {
    let t = (-(b.x - a.x)).atan2(b.z - a.z).to_degrees();
    if t < 0.0 {
        t + 360.0
    } else {
        t
    }
}

/// A spawn pad with `ahead` cm of level floor in front of it, clear to see
/// across at 60 and 150 cm: (the pad's floor, the floor ahead).
pub(crate) fn open_spot(level: &TileLevel, stage: &Stage, ahead: f32) -> (Vec3, Vec3) {
    for &p in &stage.spawn_pads {
        let pad = &stage.pads[p];
        let a = ground(level, pad.pos);
        let d = Vec3::new(pad.look.x, 0.0, pad.look.z).normalize_or_zero();
        let b = ground(level, a + d * ahead + Vec3::Y * 60.0);
        if (b.y - a.y).abs() < 1.0 && level.los(a + Vec3::Y * 150.0, b + Vec3::Y * 150.0) && level.los(a + Vec3::Y * 60.0, b + Vec3::Y * 60.0) {
            return (a, b);
        }
    }
    panic!("no open spawn with {ahead} cm of floor ahead");
}

/// One player and one simulant (its brain off, standing at `b`), after the
/// first frames: the player at `a` facing it.
pub(crate) fn duel(weapon: Option<u8>) -> (World, Vec3, Vec3) {
    let (stage, level) = complex();
    let mut w = complex_world(1, 1, BOTDIFF_NORMAL, SPIKE_SEED);
    w.bot_loadout = vec![weapon.map(|wn| (wn, false))];
    let (a, b) = open_spot(level, stage, 400.0);
    w.bot_brains = false;
    harness::step_idle(&mut w);
    let angle = pd_core::math::atan2f(a.x - b.x, a.z - b.z);
    harness::place(&mut w, 1, b, pd_core::math::wrap_pos(angle));
    w.players[0].place(a, theta_towards(a, b));
    w.sync_player_chr(0);
    (w, a, b)
}

pub(crate) fn step(w: &mut World, input: &PlayerInput) {
    w.step(4, std::slice::from_ref(input));
}

// ─── the spike's ─────────────────────────────────────────────────────────────

/// Four NormalSims on Complex routing on PD's own graph fight (kills), climb
/// (time above the ground floor), and never stand in a go-to for 3 s. The
/// spike's stage-3 baseline.
#[test]
fn bots_fight_across_complex_on_pds_graph_without_stalling() {
    let w = complex_world(0, 4, BOTDIFF_NORMAL, harness::M10_SEED);
    let m = abtest::run_match(w, 120);
    let up = (m.band_frames[2] + m.band_frames[3]) as f32 / m.alive_frames as f32;
    println!("kills {}, upstairs {:.0}% of alive time, stalls {}, rounds {} hits {}", m.kills, up * 100.0, m.stalls, m.shots, m.hits);
    assert!(m.kills >= 5, "only {} kills in 2 minutes", m.kills);
    assert!(up > 0.05, "the simulants spent only {:.1}% of their time above the ground floor", up * 100.0);
    assert_eq!(m.stalls, 0);
}

/// In the spike's arena: an automatic (the AR34, a DarkSim) fires every 4 ticks (`3600 / maxrpm`)
/// and holds the trigger past a burst.
#[test]
fn an_automatic_bot_fires_every_fourth_tick_with_no_burst_pause() {
    let mut w = harness::arena(res(), harness::setup(0, 2, BOTDIFF_DARK), SPIKE_SEED).unwrap();
    w.bot_loadout = vec![Some((WEAPON_AR34, false)); 2];
    let mut last: Option<i32> = None;
    let mut gaps = Vec::new();
    let mut prev_ammo = None;
    for _ in 0..60 * 20 {
        harness::step_idle(&mut w);
        let Some(a) = w.chrs[0].aibot.as_ref() else { continue };
        let ammo = a.loadedammo[HAND_RIGHT];
        if prev_ammo.is_some_and(|p| ammo == p - 1) {
            if let Some(l) = last {
                gaps.push(w.lv.lvframe60 - l);
            }
            last = Some(w.lv.lvframe60);
        }
        prev_ammo = Some(ammo);
    }
    println!("gaps {gaps:?}");
    let min = *gaps.iter().min().expect("simulant 0 fired");
    let longest_run = gaps.split(|&g| g != 4).map(|r| r.len()).max().unwrap();
    println!("gaps min {min}, longest 4-tick run {longest_run}");
    assert_eq!(min, 4);
    assert!(longest_run >= 5, "an automatic should hold the trigger past 3 rounds");
}

/// In the spike's arena: `forcezerominspeed` keeps a floor under the zeroing speed even when fully
/// zeroed: a NormalSim's aim never settles below ±5° of residual error.
#[test]
fn a_zeroed_normalsim_still_carries_up_to_five_degrees_of_error() {
    let mut w = harness::arena(res(), harness::setup(0, 2, BOTDIFF_NORMAL), SPIKE_SEED).unwrap();
    w.bot_loadout = vec![Some((SPIKE_MIX[0], false)), Some((SPIKE_MIX[1], false))];
    let (mut worst, mut zeroed_frames) = (0.0f32, 0);
    let (mut maxtimer, mut insight) = (0.0f32, 0);
    for _ in 0..60 * 120 {
        harness::step_idle(&mut w);
        for c in &w.chrs {
            let a = c.aibot.as_ref().unwrap();
            maxtimer = maxtimer.max(a.curzerotimer60);
            insight += a.targetinsight as i32;
            if a.targetinsight && a.curzerotimer60 >= 180.0 {
                worst = worst.max(a.zeroangle.abs());
                zeroed_frames += 1;
            }
        }
    }
    println!("worst residual zeroangle {:.2} deg over {zeroed_frames} fully-zeroed frames; max timer {maxtimer}, insight frames {insight}", worst.to_degrees());
    assert!(zeroed_frames > 0, "no simulant ever stayed zeroed for 180 ticks");
    assert!(worst > 0.0 && worst.to_degrees() <= 5.01);
}

/// A seeded match is the same match every time: two worlds from one seed
/// agree on every chr, bit for bit, and on the event stream.
#[test]
fn a_seeded_match_is_reproducible_bit_for_bit() {
    let run = || {
        let mut w = complex_world(1, 4, BOTDIFF_HARD, 0x5eed);
        let mut trace: Vec<u32> = Vec::new();
        let walk = PlayerInput { walk_y: 100, mouse_dx: 3.0, fire: true, ..PlayerInput::default() };
        for f in 0..60 * 40 {
            let input = if f % 90 < 60 { walk.clone() } else { PlayerInput { a_held: true, ..PlayerInput::default() } };
            step(&mut w, &input);
            for (i, c) in w.chrs.iter().enumerate() {
                trace.extend([c.pos.x.to_bits(), c.pos.y.to_bits(), c.pos.z.to_bits(), c.damage.to_bits(), w.mp_chr_kills(i), w.mp_chr_deaths(i), c.anim.frame.to_bits()]);
            }
            trace.push(w.players[0].bondhealth.to_bits());
            trace.push(w.take_events().len() as u32);
        }
        (trace, w.rng.random())
    };
    let (a, ra) = run();
    let (b, rb) = run();
    assert_eq!(ra, rb, "the random streams diverged");
    let first = a.iter().zip(&b).position(|(x, y)| x != y);
    assert!(first.is_none(), "diverged at word {first:?}");
    assert_eq!(a.len(), b.len());
}

/// PD's graph and ours on the same seeds (the spike's D1-D4): first contact,
/// kills per minute, sight, go-tos, re-paths, stalls, falls, coverage.
/// `AB_SEEDS` (default 10), `AB_SECONDS` (default 180).
#[test]
#[ignore]
fn probe_ab() {
    let seeds: u64 = std::env::var("AB_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    let secs: u32 = std::env::var("AB_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(180);
    let (stage, level) = complex_arc();
    let cells = abtest::floor_cells(&level);
    let mut pooled = [abtest::Pooled::default(), abtest::Pooled::default()];
    for (k, nav) in [NavChoice::Pd, NavChoice::Ours].into_iter().enumerate() {
        for seed in 1..=seeds {
            let w = harness::world(stage.clone(), level.clone(), res(), harness::setup(0, 4, BOTDIFF_NORMAL), nav, seed, true).unwrap();
            let m = abtest::run_match(w, secs);
            pooled[k].add(&m, &cells);
        }
    }
    let [pd, ours] = &pooled;
    let row = |name: &str, a: f32, b: f32| println!("{name:<34} {a:>9.3} {b:>9.3}   ratio {:.2}", b / a);
    println!("\n{seeds} seeds x {secs} s, 4 simulants              PD      ours");
    row("D1 first contact, median (s)", pd.median_first_contact(), ours.median_first_contact());
    row("D2 kills per minute", pd.kills_per_min(), ours.kills_per_min());
    row("D2 target in sight (share)", pd.insight_share(), ours.insight_share());
    row("D2 in a go-to (share)", pd.gopos_share(), ours.gopos_share());
    row("D3 re-paths per bot-minute", pd.per_bot_minute(pd.repaths), ours.per_bot_minute(ours.repaths));
    row("D3 3-s stalls per bot-minute", pd.per_bot_minute(pd.stalls), ours.per_bot_minute(ours.stalls));
    row("D3 ledge falls per bot-minute", pd.per_bot_minute(pd.ledge_falls), ours.per_bot_minute(ours.ledge_falls));
    println!("go-to calls / no start / no end / no route: PD {:?}, ours {:?}", pd.gotos, ours.gotos);
    println!("kills per match: PD {:?}, ours {:?}", pd.kills_each, ours.kills_each);
    row("shots per minute", pd.shots as f32 / pd.minutes, ours.shots as f32 / ours.minutes);
    row("hit rate", pd.hits as f32 / pd.shots.max(1) as f32, ours.hits as f32 / ours.shots.max(1) as f32);
    for (b, &(_, _, name)) in abtest::BANDS.iter().enumerate() {
        row(&format!("D4 coverage, {name} ({} cells)", cells[b].len()), pd.coverage(b), ours.coverage(b));
    }
}

/// A readable timeline of one match on Complex (PD's graph).
#[test]
#[ignore]
fn probe_complex_match() {
    let mut w = complex_world(0, 4, BOTDIFF_NORMAL, SPIKE_SEED);
    for f in 0..60 * 180 {
        harness::step_idle(&mut w);
        if f % 600 == 0 {
            let line: Vec<String> = w
                .chrs
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let a = c.aibot.as_ref().unwrap();
                    format!("{:>6} y{:>4.0} {:?} {:?} K{}D{} t{:?}", c.name, c.manground, c.actiontype, a.distmode.map(|d| d.label()), w.mp_chr_kills(i), w.mp_chr_deaths(i), c.target)
                })
                .collect();
            println!("t={:>3}s | {}", f / 60, line.join(" | "));
        }
    }
    let kills: u32 = (0..w.chrs.len()).map(|i| w.mp_chr_kills(i)).sum();
    println!("kills {kills} in 3 min; gotos {} (no start {}, no end {}, no route {}), repaths {}", w.navstats.gotos, w.navstats.goto_no_start, w.navstats.goto_no_end, w.navstats.goto_no_route, w.navstats.repaths);
}

// ─── the Complex fight's ─────────────────────────────────────────────────────

/// The player's Falcon kills a simulant standing 4 m ahead: the hits reach
/// `chr_damage`, the simulant dies at `maxdamage`, the kill is the player's,
/// and it fades out and respawns.
#[test]
fn the_players_falcon_kills_a_simulant() {
    let (mut w, _, _) = duel(None);
    let mut died_at = None;
    for t in 0..60 * 12 {
        // Tap the trigger (a semi-automatic): 6 ticks down, 6 up.
        let fire = t > 60 && (t / 6) % 2 == 0;
        step(&mut w, &PlayerInput { fire, ..PlayerInput::default() });
        if w.chr_is_dead(1) {
            died_at = Some(t);
            break;
        }
    }
    assert!(died_at.is_some(), "the simulant survived: damage {:.2}", w.chrs[1].damage);
    assert_eq!(w.mp_chr_kills(0), 1);
    assert_eq!(w.mp_chr_deaths(1), 1);
    // chr_tick_die, then the 90-tick fade and bot_spawn.
    let mut faded = false;
    for _ in 0..60 * 10 {
        step(&mut w, &PlayerInput::default());
        faded |= w.chrs[1].actiontype == Act::Dead && w.chrs[1].fadealpha < 128.0 && w.chrs[1].fadealpha >= 0.0;
        if faded && !w.chr_is_dead(1) {
            break;
        }
    }
    assert!(faded, "the corpse never faded");
    assert!(!w.chr_is_dead(1) && w.chrs[1].damage == 0.0, "no respawn");
    assert!(w.chrs[1].aibot.as_ref().unwrap().fadeintimer60 > 0, "it fades in");
}

/// One aimed Falcon round at the head, chest and shin of a simulant 4 m away:
/// PD's hit parts (head ×4, torso ×2, limbs ×1 of the Falcon's damage).
#[test]
fn a_falcon_round_does_head_torso_and_leg_damage() {
    let (mut w, a, b) = duel(None);
    w.players[0].gun.select_weapon(WEAPON_FALCON2, false);
    for _ in 0..120 {
        step(&mut w, &PlayerInput::default());
    }
    let falcon = w.res.gset.func(WEAPON_FALCON2, FUNC_PRIMARY).and_then(|f| f.shoot.as_ref()).unwrap().damage;
    let angle = pd_core::math::wrap_pos(pd_core::math::atan2f(a.x - b.x, a.z - b.z));
    // The shin: the left one's joint (the stance puts the legs either side of
    // the centre line), `HITPART_LSHIN`'s box.
    let lshin = {
        let c = &w.chrs[1];
        let def = &c.model.def;
        let n = def.nodes.iter().position(|n| matches!(n.kind, pd_core::model::NodeKind::BBox { hitpart: HITPART_LSHIN, .. })).unwrap();
        c.model.matrices[def.find_node_mtx_index(n, 0).unwrap()].w_axis.truncate()
    };
    let shot_at = |w: &mut World, height: f32| -> f32 {
        harness::place(w, 1, b, angle);
        w.chrs[1].damage = 0.0;
        w.chrs[1].flinchcnt = -1;
        let at = if height < 80.0 { Vec3::new(lshin.x, b.y, lshin.z) } else { b };
        w.players[0].place(a, theta_towards(a, at));
        let d = Vec2::new(at.x - a.x, at.z - a.z).length();
        let target = b.y + height;
        let aim = |w: &mut World| w.players[0].verta = ((target - w.players[0].pos.y) / d).atan().to_degrees();
        for _ in 0..40 {
            aim(w);
            step(w, &PlayerInput { aim: true, ..PlayerInput::default() });
        }
        let before = w.chrs[1].damage;
        for t in 0..8 {
            aim(w);
            step(w, &PlayerInput { aim: true, fire: t < 2, ..PlayerInput::default() });
        }
        (w.chrs[1].damage - before) / falcon
    };
    let rounds = |w: &mut World, h: f32| (0..4).map(|_| shot_at(w, h)).collect::<Vec<f32>>();
    let head = rounds(&mut w, 165.0);
    let chest = rounds(&mut w, 128.0);
    let shin = rounds(&mut w, 45.0);
    let has = |v: &[f32], x: f32| v.iter().any(|d| (d - x).abs() < 1e-3);
    println!("head {head:?} chest {chest:?} shin {shin:?}");
    assert!(has(&head, 4.0), "head rounds did {head:?}");
    assert!(has(&chest, 2.0), "chest rounds did {chest:?}");
    assert!(has(&shin, 1.0), "shin rounds did {shin:?}");
    assert!(head.iter().chain(&chest).chain(&shin).all(|d| [0.0, 1.0, 2.0, 4.0].iter().any(|x| (d - x).abs() < 1e-3)), "a round did an odd amount");
}

/// A simulant hunts down a player who stands still, hurts them (the health
/// bar, the red flash), kills them; the player dies (the head falls, the
/// screen fades) and a press of Z starts a new life at full health.
#[test]
fn a_simulant_kills_the_player_who_respawns() {
    let mut w = complex_world(1, 1, BOTDIFF_HARD, harness::M10_SEED);
    w.bot_loadout = vec![Some((WEAPON_AR34, false))];
    let (mut targeted, mut hurt, mut flashed, mut died, mut respawned) = (false, false, false, false, false);
    for _ in 0..60 * 120 {
        let press = w.players[0].isdead && w.players[0].health.player_is_fade_complete();
        step(&mut w, &PlayerInput { fire: press, ..PlayerInput::default() });
        let p = &w.players[0];
        targeted |= w.chrs[1].target == Some(0);
        hurt |= p.bondhealth < 1.0 && p.health.player_is_health_visible();
        flashed |= p.health.colourscreen == [0x96, 0, 0] && p.health.colourscreenfrac > 0.2 && !p.isdead;
        if p.isdead {
            died = true;
        } else if died {
            respawned = true;
            break;
        }
    }
    assert!(targeted, "the simulant never targeted the player");
    assert!(hurt, "the player was never hurt");
    assert!(flashed, "no red flash");
    assert!(died, "the player never died");
    assert!(respawned && w.spawns[0] >= 2, "no respawn (spawns {})", w.spawns[0]);
    assert_eq!(w.players[0].bondhealth, 1.0);
    assert_eq!(w.mp_chr_deaths(0), 1);
    assert_eq!(w.mp_chr_kills(1), 1);
}

/// A deployed Laptop sentry shoots a simulant standing in front of it.
#[test]
fn a_deployed_laptop_sentry_shoots_a_simulant() {
    let (mut w, _, b) = duel(None);
    step(&mut w, &PlayerInput { select: Some((WEAPON_LAPTOPGUN, false)), ..PlayerInput::default() });
    for _ in 0..150 {
        step(&mut w, &PlayerInput::default());
    }
    w.players[0].verta = -30.0;
    for _ in 0..30 {
        step(&mut w, &PlayerInput { use_held: true, ..PlayerInput::default() });
    }
    for _ in 0..4 {
        step(&mut w, &PlayerInput { use_held: true, fire: true, ..PlayerInput::default() });
    }
    let angle = w.chrs[1].theta();
    let mut hurt = 0.0f32;
    for _ in 0..600 {
        let before = w.chrs[1].damage;
        step(&mut w, &PlayerInput::default());
        hurt += (w.chrs[1].damage - before).max(0.0);
        if w.chr_is_dead(1) {
            break;
        }
        if w.chrs[1].pos.distance(b) > 100.0 {
            harness::place(&mut w, 1, b, angle);
        }
    }
    assert!(w.props.objs.iter().any(|o| o.ty == OBJTYPE_AUTOGUN), "no sentry deployed");
    assert!(hurt > 0.0 || w.chr_is_dead(1), "the sentry never hurt the simulant");
}

/// Footsteps on Complex's metal floors: running simulants make them from their
/// animation's footfall frames (`footstep_check_default`), the walking player
/// every 150 cm (`bmove_tick`), all from `g_FootstepSounds`' metal row.
#[test]
fn simulants_and_the_player_make_metal_footsteps() {
    let metal = &crate::chr::FOOTSTEP_SOUNDS[FLOORTYPE_METAL as usize * 8..FLOORTYPE_METAL as usize * 8 + 8];
    let mut w = complex_world(1, 2, BOTDIFF_NORMAL, SPIKE_SEED);
    w.bot_loadout = vec![Some((WEAPON_FALCON2, false)); 2];
    // The player walks from the spawn: its own steps are centred, at full volume.
    let (mut footfalls, mut bot_steps, mut my_steps) = (0, 0, 0);
    for _ in 0..60 * 5 {
        step(&mut w, &PlayerInput { walk_y: 127, ..PlayerInput::default() });
        for e in w.take_events() {
            if let pd_core::events::Event::Sound { sound, volume, pan, .. } = e {
                if metal.contains(&sound) && pan == 0.0 && volume == 1.0 {
                    my_steps += 1;
                }
            }
        }
    }
    // Then it stands beside a simulant: their footfalls, and the sounds it hears.
    let c = &w.chrs[1];
    let feet = (0..8)
        .find_map(|k| {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let p = Vec3::new(c.pos.x + 150.0 * a.sin(), c.manground + 40.0, c.pos.z + 150.0 * a.cos());
            let (y, poly) = w.level.cd_find_ground_at_cyl(p, 30.0);
            (poly.is_some() && (y - c.manground).abs() < 30.0).then_some(Vec3::new(p.x, y, p.z))
        })
        .expect("floor beside the simulant");
    harness::place_player(&mut w, 0, feet, 0.0);
    for _ in 0..60 * 10 {
        step(&mut w, &PlayerInput::default());
        footfalls += w.chrs.iter().skip(1).filter(|c| c.footstep != 0 && c.floortype == FLOORTYPE_METAL).count();
        for e in w.take_events() {
            if let pd_core::events::Event::Sound { sound, .. } = e {
                if metal.contains(&sound) {
                    bot_steps += 1;
                }
            }
        }
    }
    assert!(my_steps > 3, "player footsteps: {my_steps}");
    assert!(footfalls > 10, "simulant footfalls on metal: {footfalls}");
    assert!(bot_steps > 0, "none heard from the simulants");
}

/// The RC-P120's cloak hides the player from a simulant that isn't already
/// tracking them (`bot_is_target_invisible`); uncloaked, it sees them again.
#[test]
fn the_rcp120_cloak_hides_the_player_from_a_simulant() {
    let (mut w, _, b) = duel(Some(WEAPON_FALCON2));
    w.bot_brains = true;
    let angle = w.chrs[1].theta();
    step(&mut w, &PlayerInput { select: Some((WEAPON_RCP120, false)), ..PlayerInput::default() });
    for _ in 0..150 {
        step(&mut w, &PlayerInput::default());
        harness::place(&mut w, 1, b, angle);
    }
    w.players[0].devicesactive |= DEVICE_CLOAKRCP120;
    for _ in 0..120 {
        step(&mut w, &PlayerInput::default());
        harness::place(&mut w, 1, b, angle);
    }
    assert!(w.players[0].cloak.cloaked && w.chrs[0].cloak.cloaked, "cloak on");
    // Forget the player, then look for them for five seconds.
    let health = w.players[0].bondhealth;
    {
        w.chrs[1].target = None;
        let a = w.chrs[1].aibot.as_mut().unwrap();
        a.targetcloaktimer60 = 0;
        a.targetinsight = false;
        a.chrsinsight[0] = false;
    }
    let mut seen = 0;
    for _ in 0..300 {
        step(&mut w, &PlayerInput::default());
        harness::place(&mut w, 1, b, angle);
        seen += w.chrs[1].aibot.as_ref().unwrap().chrsinsight[0] as i32;
    }
    assert_eq!(seen, 0, "a cloaked player was seen");
    assert_eq!(w.players[0].bondhealth, health, "and not shot");
    w.players[0].devicesactive &= !DEVICE_CLOAKRCP120;
    let mut seen = 0;
    for _ in 0..300 {
        step(&mut w, &PlayerInput::default());
        harness::place(&mut w, 1, b, angle);
        seen += w.chrs[1].aibot.as_ref().unwrap().chrsinsight[0] as i32;
    }
    assert!(seen > 0, "uncloaked, the simulant should see the player");
}

#[test]
#[ignore]
fn probe_duel() {
    let (mut w, a, b) = duel(Some(WEAPON_CMP150));
    for _ in 0..30 {
        step(&mut w, &PlayerInput::default());
    }
    for h in w.chrs[1].held.iter().flatten() {
        let m = &h.model;
        println!("held {} stem {} scale {} mats {} first {:?} vis {}", h.weaponnum, m.def.stem, m.scale, m.matrices.len(), m.matrices.first().map(|x| x.w_axis), m.vis.iter().filter(|v| **v).count());
    }
    println!("chr scale {} def.scale {} body root {:?}", w.chrs[1].model.scale, w.chrs[1].model.def.scale, w.chrs[1].model.matrices[0].w_axis);
    {
        let c = &w.chrs[1];
        let def = &c.model.def;
        let rh = def.get_part(MODELPART_CHR_RIGHTHAND).and_then(|n| def.find_node_mtx_index(n, 0)).unwrap();
        println!("right hand slot {rh} at {:?}; nmats {}; anim {} frame {}", c.model.matrices[rh], c.model.matrices.len(), c.anim.animnum, c.anim.frame);
        let g = &c.held[0].as_ref().unwrap().model;
        println!("gun root node {:?}", g.def.nodes[0].kind);
    }
    let (_, level) = complex();
    println!("ground at b {:?} {:?}; chr man {} ground {} fall {} force {} pos {} act {:?}", level.cd_find_ground_at_cyl(Vec3::new(b.x, 69.0, b.z), 20.0), level.cd_find_ground_at_cyl(b + Vec3::Y * 60.0, 30.0), w.chrs[1].manground, w.chrs[1].ground, w.chrs[1].fallspeed, w.chrs[1].forcetoground, w.chrs[1].pos, w.chrs[1].actiontype);
    step(&mut w, &PlayerInput::default());
    println!("after 1: man {} ground {} fall {} pos {} ci {}", w.chrs[1].manground, w.chrs[1].ground, w.chrs[1].fallspeed, w.chrs[1].pos, w.chrs[1].model.chrinfo.pos);
    for t in 0..200 {
        let fire = t > 60 && (t / 6) % 2 == 0;
        step(&mut w, &PlayerInput { fire, ..PlayerInput::default() });
        if t % 20 == 0 {
            let c = &w.chrs[1];
            let p = &w.players[0];
            let on = crate::chr::body::pos_is_onscreen(&p.cam, c.pos, c.effective_scale());
            let root = c.model.matrices.first().map(|m| m.w_axis.truncate());
            println!("t{t} a{a} b{b} chr{} root{root:?} on{on} anyscreen{} p{} theta{} shots{} wn{} dmg{} hitpos{:?}", c.pos, c.onanyscreen, p.pos, p.theta, w.shots_fired(0), p.gun.bgun_get_weapon_num(0), c.damage, p.gun.hands[0].hitpos);
        }
    }
}

#[test]
#[ignore]
fn probe_hit_boxes() {
    let (mut w, a, b) = duel(None);
    for _ in 0..60 {
        step(&mut w, &PlayerInput::default());
    }
    let c = &w.chrs[1];
    let eye = a + Vec3::Y * 159.0;
    for h in [10.0, 30.0, 45.0, 60.0, 80.0, 100.0, 128.0, 165.0] {
        let t = b + Vec3::Y * h;
        let dir = (t - eye).normalize();
        let hit = c.chr_test_hit(eye, dir, true);
        let root = c.model.matrices[0].w_axis.truncate();
        let raw = c.model.test_for_hit(eye, dir, None, pd_core::model::hit::HitPad(0.0));
        println!("   radius {} facing {} raw {raw:?}", c.chr_get_hit_radius(), crate::chr::body::pos_is_facing_pos(eye, dir, root, c.chr_get_hit_radius()));
        let bg = w.stage.bghit.bg_test_hit(eye, eye + dir * 1000.0).map(|b| b.pos);
        println!("h{h}: {:?} bg {bg:?}", hit.map(|x| (x.hitpart, x.pos)));
    }
    for (n, node) in c.model.def.nodes.iter().enumerate() {
        if let pd_core::model::NodeKind::BBox { hitpart, bbox } = node.kind {
            let m = c.model.def.find_node_mtx_index(n, 0).map(|i| c.model.matrices[i]);
            println!("box {n} part {hitpart} {bbox:?} at {:?}", m.map(|m| m.w_axis.truncate()));
        }
    }
}


/// A duel with the simulant's brain on, 8 m apart, until `until` holds or 20 s.
fn armed_duel(weapon: u8, until: &mut dyn FnMut(&World) -> bool) -> World {
    let (mut w, _, _) = duel(Some(weapon));
    w.bot_brains = true;
    for _ in 0..60 * 20 {
        step(&mut w, &PlayerInput::default());
        if until(&w) {
            break;
        }
    }
    w
}

/// A simulant fires what it holds: rockets (here the launcher's homing ones,
/// at its target) that blow up by the player.
#[test]
fn a_simulant_fires_rockets_that_explode_by_the_player() {
    let mut rocket_seen = false;
    let w = armed_duel(WEAPON_ROCKETLAUNCHER, &mut |w| {
        rocket_seen |= w.props.objs.iter().any(|o| matches!(o.weaponnum, WEAPON_ROCKET | WEAPON_HOMINGROCKET) && o.projectile.is_some() && o.owner() == 1);
        w.players[0].bondhealth < 1.0 || w.players[0].isdead
    });
    assert!(rocket_seen, "a rocket in flight from the simulant");
    assert!(w.players[0].bondhealth < 1.0 || w.players[0].isdead, "the player was hurt");
}

/// A simulant throws its grenades (`botact_throw`) with the throw's woosh.
#[test]
fn a_simulant_throws_grenades() {
    let mut thrown = false;
    let w = armed_duel(WEAPON_GRENADE, &mut |w| {
        thrown |= w.props.objs.iter().any(|o| o.weaponnum == WEAPON_GRENADE && o.projectile.is_some() && o.owner() == 1);
        thrown
    });
    assert!(thrown, "a grenade in the air");
    assert!(w.ab(1).throwtimer60 > 0, "the next throw waits");
}

/// A player's thrown knife meets a simulant (`projectile_0f06c28c`): the
/// throw's damage, and the knife falls to the floor.
#[test]
fn a_thrown_knife_hurts_a_simulant() {
    let (mut w, a, b) = duel(None);
    w.harness_give_loadout(vec![WEAPON_COMBATKNIFE]);
    step(&mut w, &PlayerInput { select: Some((WEAPON_COMBATKNIFE, false)), ..Default::default() });
    for _ in 0..120 {
        step(&mut w, &PlayerInput::default());
    }
    // The secondary (throw), aimed at the chest.
    step(&mut w, &PlayerInput { use_held: true, ..Default::default() });
    for _ in 0..40 {
        step(&mut w, &PlayerInput { use_held: true, ..Default::default() });
    }
    for _ in 0..80 {
        step(&mut w, &PlayerInput::default());
    }
    let d = b + Vec3::Y * 110.0 - w.players[0].pos;
    w.players[0].verta = d.y.atan2((d.x * d.x + d.z * d.z).sqrt()).to_degrees();
    let _ = a;
    let before = w.chrs[1].damage;
    for _ in 0..3 {
        step(&mut w, &PlayerInput { fire: true, ..Default::default() });
    }
    for _ in 0..60 {
        step(&mut w, &PlayerInput::default());
    }
    assert!(w.chrs[1].damage > before, "the knife hurt it: {} -> {}", before, w.chrs[1].damage);
}

/// Every arena hosts a Combat match: four NormalSims, unarmed at the start as
/// in PD, arm themselves and kill each other within two minutes, routing on
/// PD's graph through the doors and lifts. Prints each arena's figures.
#[test]
fn every_arena_hosts_a_simulant_match() {
    let mut failed = Vec::new();
    for code in crate::stage::ARENAS {
        let stage = std::sync::Arc::new(Stage::load(&crate::testutil::assets(), code).unwrap());
        let level = std::sync::Arc::new(TileLevel::for_stage(&stage));
        let setup = pd_core::mp::MatchSetup { stagenum: stage.stagenum, ..harness::setup(0, 4, BOTDIFF_NORMAL) };
        let w = harness::world(stage, level, res(), setup, NavChoice::Pd, SPIKE_SEED, false).unwrap();
        let m = abtest::run_match(w, 120);
        println!("{code:5} kills {:3} stalls {:3} gotos {:5} failed (no start, end, route) {:?}", m.kills, m.stalls, m.gotos[0], &m.gotos[1..]);
        // (Villa: two unarmed simulants meeting in the tunnel under the pool,
        // rooms 59 and 60, find no waypoint from there, by PD's own lookup,
        // and stand retrying.)
        if m.kills < 3 {
            failed.push(code);
        }
    }
    assert!(failed.is_empty(), "no match on {failed:?}");
}


/// A long probe (not PD): every 10 s of a four-simulant match on `ARENA`
/// (default Fortress), each chr's place, action, target, guns and lift state,
/// and the nearest door's mode and fraction.
#[test]
#[ignore]
fn probe_arena_bots() {
    let code = std::env::var("ARENA").unwrap_or("mp12".into());
    let stage = std::sync::Arc::new(Stage::load(&crate::testutil::assets(), &code).unwrap());
    let level = std::sync::Arc::new(TileLevel::for_stage(&stage));
    let setup = pd_core::mp::MatchSetup { stagenum: stage.stagenum, ..harness::setup(0, 4, BOTDIFF_NORMAL) };
    let mut w = harness::world(stage, level, res(), setup, NavChoice::Pd, SPIKE_SEED, false).unwrap();
    for f in 0..60 * 120 {
        harness::step_idle(&mut w);
        if f % 600 == 0 {
            for (i, c) in w.chrs.iter().enumerate() {
                let a = c.aibot.as_ref().unwrap();
                let door = w.props.objs.iter().filter_map(|o| o.door.as_ref().map(|d| ((o.pos - c.pos).length(), o.id, d.mode, d.frac))).min_by(|a, b| a.0.total_cmp(&b.0));
                println!(
                    "t {:4} chr {i} at {:.0} rooms {:?} act {:?} my {:?} target {:?} held {:?} liftaction {} door {door:?}",
                    f / 60,
                    c.pos,
                    c.rooms,
                    c.actiontype,
                    a.myaction,
                    c.target,
                    c.held.iter().map(|h| h.as_ref().map(|g| g.weaponnum)).collect::<Vec<_>>(),
                    c.liftaction
                );
            }
        }
    }
}

/// `botact_create_slayer_rocket` and `rocket_tick_fbw`: a simulant's Slayer
/// rocket, launched at a target out of sight across Complex, flies the
/// waypoints pad by pad and is let go of when it blows up. Turning slowly, it
/// cuts corners and sometimes clips a doorway (PD's: it moves on to the next
/// pad within 1 m of the last); of the pad pairs tried, some reach their
/// target and blow up within 2.5 m of it.
#[test]
fn a_simulants_slayer_rocket_flies_the_waypoints_to_an_unseen_target() {
    let (stage, level) = complex();
    let floor = |p: usize| ground(level, stage.pads[p].pos);
    let pairs: Vec<(usize, usize)> = stage
        .spawn_pads
        .iter()
        .flat_map(|&p| stage.spawn_pads.iter().map(move |&q| (p, q)))
        .filter(|&(p, q)| {
            let (fa, fb) = (floor(p), floor(q));
            fa.distance(fb) > 800.0 && !level.los(fa + Vec3::Y * 150.0, fb + Vec3::Y * 150.0)
        })
        .take(8)
        .collect();
    assert!(pairs.len() >= 4);
    let mut reached = 0;
    for &(a, b) in &pairs {
        // A player (idle) for the objects' pass that flies projectiles.
        let mut w = complex_world(1, 2, BOTDIFF_NORMAL, SPIKE_SEED);
        w.bot_brains = false;
        harness::step_idle(&mut w);
        harness::place(&mut w, 1, floor(a), 0.0);
        harness::place(&mut w, 2, floor(b), 0.0);
        w.chrs[1].target = Some(2);
        w.botact_create_slayer_rocket(1);
        let Some(id) = w.chrs[1].aibot.as_ref().unwrap().skrocket else { continue };
        let (mut closest, mut steps) = (f32::MAX, 0);
        for _ in 0..60 * 20 {
            harness::step_idle(&mut w);
            if let Some(o) = w.props.get(id) {
                closest = closest.min(o.pos.distance(w.chrs[2].pos));
                steps = steps.max(o.projectile.as_ref().map_or(0, |p| p.step));
            }
            if w.chrs[1].aibot.as_ref().unwrap().skrocket.is_none() {
                break;
            }
        }
        let hurt = w.chrs[2].damage > 0.0 || w.chr_is_dead(2);
        println!("pads {a:3} -> {b:3}: {} pads flown, closest {closest:.0} cm, hurt {hurt}", steps);
        assert!(w.chrs[1].aibot.as_ref().unwrap().skrocket.is_none(), "the rocket is gone");
        assert!(steps >= 1, "it flew its route");
        if closest < 300.0 && hurt {
            reached += 1;
        }
    }
    println!("{reached} of {} reached their target", pairs.len());
    assert!(reached >= 2, "only {reached} rockets reached their target");
}
