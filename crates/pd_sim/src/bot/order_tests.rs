//! The personalities' rules and a human's orders, on the spike's pillar arena
//! (it has no pickups, so the main loop is never sent for one): each rule on
//! staged chrs, with the brains off unless a test walks the simulant.
//!
//! Chr indexes are not the setup's order (the allocation shuffles the
//! simulants' slots): [`sims`] maps setup simulant `k` to its chr.

use glam::Vec3;
use pd_core::ids::*;
use pd_core::mp::MatchSetup;

use super::{bot_calculate_max_speed, MyAction};
use crate::chr::Act;
use crate::harness;
use crate::testutil::res;
use crate::world::World;

/// The arena with `players` humans and one simulant per entry of `types`
/// (`BOTTYPE_*`, NormalSims), everyone unarmed, after the first frames.
fn arena(players: usize, types: &[u8], edit: impl FnOnce(&mut MatchSetup)) -> World {
    let mut setup = harness::setup(players, types.len(), BOTDIFF_NORMAL);
    for (s, &t) in setup.simulants.iter_mut().zip(types) {
        s.bottype = t;
    }
    edit(&mut setup);
    let mut w = harness::arena(res(), setup, harness::SPIKE_SEED).unwrap();
    w.bot_brains = false;
    for _ in 0..150 {
        harness::step_idle(&mut w);
    }
    w
}

/// The chr index of each setup simulant, in setup order.
fn sims(w: &World) -> Vec<usize> {
    (0..w.setup.simulants.len()).map(|k| w.chrs.iter().position(|c| c.aibot.as_ref().is_some_and(|a| a.aibotnum == k)).unwrap()).collect()
}

/// Stand chr `i` at `(x, z)` on the arena floor facing +z.
fn stand(w: &mut World, i: usize, x: f32, z: f32) {
    if let Some(p) = w.chrs[i].player {
        w.players[p].place(Vec3::new(x, 0.0, z), 0.0);
        w.sync_player_chr(p);
    } else {
        harness::place(w, i, Vec3::new(x, 0.0, z), 0.0);
    }
}

/// Poll everyone's sight and distance for simulant `i` (one chr per call).
fn look_around(w: &mut World, i: usize) {
    for _ in 0..w.chrs.len() * 2 {
        w.bot_choose_general_target(i);
    }
}

/// The main loop once, from scratch.
fn main_loop(w: &mut World, i: usize) {
    w.ab_mut(i).myaction = MyAction::MainLoop;
    w.bot_tick_mainloop(i);
}

#[test]
fn a_turtlesim_crawls_and_a_speedsim_runs() {
    // One body for all three: the speed scales with the body's height.
    let w = arena(0, &[BOTTYPE_GENERAL, BOTTYPE_TURTLE, BOTTYPE_SPEED], |s| s.simulants.iter_mut().for_each(|b| b.chr.mpbodynum = 0));
    let s = sims(&w);
    let base = bot_calculate_max_speed(&w.chrs[s[0]]);
    let mult = |i: usize| bot_calculate_max_speed(&w.chrs[i]) / base * 7.6;
    let (turtle, speed) = (mult(s[1]), mult(s[2]));
    assert!((turtle - 3.5).abs() < 1e-4 && (speed - 14.0).abs() < 1e-4, "turtle {turtle}, speed {speed}");
}

#[test]
fn a_peacesim_leaves_the_unarmed_alone() {
    let mut w = arena(0, &[BOTTYPE_PEACE, BOTTYPE_GENERAL], |_| {});
    let s = sims(&w);
    stand(&mut w, s[0], 0.0, -300.0);
    stand(&mut w, s[1], 0.0, 300.0);
    look_around(&mut w, s[0]);
    assert!(w.ab(s[0]).chrsinsight[s[1]], "the two can see each other");
    assert_eq!(w.chrs[s[0]].target, None, "an unarmed simulant is no target for a PeaceSim");
    w.ab_mut(s[1]).weaponnum = WEAPON_FALCON2;
    look_around(&mut w, s[0]);
    assert_eq!(w.chrs[s[0]].target, Some(s[1]), "an armed one is");
    // A NormalSim takes the unarmed.
    let mut w = arena(0, &[BOTTYPE_GENERAL, BOTTYPE_GENERAL], |_| {});
    let s = sims(&w);
    stand(&mut w, s[0], 0.0, -300.0);
    stand(&mut w, s[1], 0.0, 300.0);
    look_around(&mut w, s[0]);
    assert_eq!(w.chrs[s[0]].target, Some(s[1]));
}

#[test]
fn a_vengesim_goes_after_its_last_killer() {
    let mut w = arena(0, &[BOTTYPE_VENGE, BOTTYPE_GENERAL, BOTTYPE_GENERAL], |_| {});
    let s = sims(&w);
    main_loop(&mut w, s[0]);
    assert_ne!(w.ab(s[0]).attackingplayernum, Some(s[2]));
    w.ab_mut(s[0]).lastkilledbyplayernum = s[2] as i32;
    main_loop(&mut w, s[0]);
    assert_eq!((w.ab(s[0]).myaction, w.ab(s[0]).attackingplayernum), (MyAction::Attack, Some(s[2])));
}

#[test]
fn a_feudsim_keeps_its_first_grudge() {
    let mut w = arena(0, &[BOTTYPE_FEUD, BOTTYPE_GENERAL, BOTTYPE_GENERAL], |_| {});
    let s = sims(&w);
    w.ab_mut(s[0]).lastkilledbyplayernum = s[1] as i32;
    main_loop(&mut w, s[0]);
    assert_eq!((w.ab(s[0]).feudplayernum, w.ab(s[0]).attackingplayernum), (Some(s[1]), Some(s[1])));
    // Killed by another since: still after the first.
    w.ab_mut(s[0]).lastkilledbyplayernum = s[2] as i32;
    main_loop(&mut w, s[0]);
    assert_eq!((w.ab(s[0]).feudplayernum, w.ab(s[0]).attackingplayernum), (Some(s[1]), Some(s[1])));
}

#[test]
fn a_feudsim_holds_no_grudge_against_a_teammate() {
    let mut w = arena(0, &[BOTTYPE_FEUD, BOTTYPE_GENERAL, BOTTYPE_GENERAL], |s| {
        s.options |= MPOPTION_TEAMSENABLED;
        s.simulants[0].chr.team = 0;
        s.simulants[1].chr.team = 0;
        s.simulants[2].chr.team = 1;
    });
    let s = sims(&w);
    w.ab_mut(s[0]).lastkilledbyplayernum = s[1] as i32;
    main_loop(&mut w, s[0]);
    assert_eq!(w.ab(s[0]).feudplayernum, None);
    w.ab_mut(s[0]).lastkilledbyplayernum = s[2] as i32;
    main_loop(&mut w, s[0]);
    assert_eq!(w.ab(s[0]).feudplayernum, Some(s[2]));
}

/// PD's JudgeSim loop has no break: it ends on the worst placed chr it may
/// attack, not the leader.
#[test]
fn a_judgesim_picks_the_last_ranked() {
    let mut w = arena(0, &[BOTTYPE_JUDGE, BOTTYPE_GENERAL, BOTTYPE_GENERAL, BOTTYPE_GENERAL], |_| {});
    let s = sims(&w);
    let slot = |i: usize| w.chrs[i].mpslot;
    let (judge, s1, s2, s3) = (slot(s[0]), slot(s[1]), slot(s[2]), slot(s[3]));
    // Sim 2 leads with three kills, sim 3 has two, the judge one, sim 1 none.
    w.mp.chrs[s2].killcounts[s1] = 3;
    w.mp.chrs[s3].killcounts[s1] = 2;
    w.mp.chrs[judge].killcounts[s1] = 1;
    main_loop(&mut w, s[0]);
    assert_eq!(w.ab(s[0]).attackingplayernum, Some(s[1]));
    // The last placed dead: the next up that isn't the judge itself.
    w.chrs[s[1]].actiontype = Act::Dead;
    main_loop(&mut w, s[0]);
    assert_eq!(w.ab(s[0]).attackingplayernum, Some(s[3]));
}

#[test]
fn a_preysim_picks_the_weakest() {
    let mut w = arena(1, &[BOTTYPE_PREY, BOTTYPE_GENERAL, BOTTYPE_GENERAL], |_| {});
    let s = sims(&w);
    // Simulants: 8 health less damage; the player: bondhealth × 8.
    w.chrs[s[1]].damage = 5.0;
    w.chrs[s[2]].damage = 1.0;
    w.players[0].bondhealth = 0.5;
    main_loop(&mut w, s[0]);
    assert_eq!(w.ab(s[0]).attackingplayernum, Some(s[1]));
    w.players[0].bondhealth = 0.25;
    main_loop(&mut w, s[0]);
    assert_eq!(w.ab(s[0]).attackingplayernum, Some(0));
}

/// Teams on: the player and simulant 0 on team 0, simulant 1 (if any) on 1.
fn team_arena(bots: usize) -> (World, Vec<usize>) {
    let w = arena(1, &vec![BOTTYPE_GENERAL; bots], |s| {
        s.options |= MPOPTION_TEAMSENABLED;
        s.players[0].chr.team = 0;
        s.simulants[0].chr.team = 0;
        if bots > 1 {
            s.simulants[1].chr.team = 1;
        }
    });
    let s = sims(&w);
    (w, s)
}

#[test]
fn follow_and_protect_keep_near_the_player() {
    let (mut w, s) = team_arena(1);
    assert!(!w.botcmd_apply(s[0], AIBOTCMD_FOLLOW, 0));
    w.bot_tick_mainloop(s[0]);
    let a = w.ab(s[0]);
    assert_eq!((a.command, a.myaction, a.followingplayernum, a.canbreakfollow), (AIBOTCMD_FOLLOW, MyAction::Follow, Some(0), true));
    w.botcmd_apply(s[0], AIBOTCMD_PROTECT, 0);
    w.bot_tick_mainloop(s[0]);
    let a = w.ab(s[0]);
    assert_eq!((a.command, a.myaction, a.followingplayernum, a.canbreakfollow), (AIBOTCMD_PROTECT, MyAction::Follow, Some(0), false));
}

#[test]
fn attack_goes_after_the_chosen_chr() {
    let (mut w, s) = team_arena(2);
    assert!(w.botcmd_apply(s[0], AIBOTCMD_ATTACK, 0), "Attack picks its target first");
    w.bot_apply_attack(s[0], s[1]);
    w.bot_tick_mainloop(s[0]);
    let a = w.ab(s[0]);
    assert_eq!((a.command, a.myaction, a.attackingplayernum), (AIBOTCMD_ATTACK, MyAction::Attack, Some(s[1])));
}

/// Hold: the simulant runs to where the player stood (the player has moved
/// on) and faces the way the player faced.
#[test]
fn hold_stands_where_the_player_stood_facing_its_way() {
    let (mut w, s) = team_arena(1);
    stand(&mut w, s[0], -400.0, -400.0);
    w.players[0].place(Vec3::new(400.0, 0.0, 0.0), 90.0);
    w.sync_player_chr(0);
    let (spot, rot) = (w.chrs[0].pos, w.chrs[0].theta());
    w.botcmd_apply(s[0], AIBOTCMD_HOLD, 0);
    stand(&mut w, 0, -400.0, 600.0);
    w.bot_brains = true;
    for _ in 0..60 * 8 {
        harness::step_idle(&mut w);
    }
    let (a, c) = (w.ab(s[0]), &w.chrs[s[0]]);
    let d = c.pos - spot;
    let turn = pd_core::math::turn();
    let off = ((c.theta() - rot).rem_euclid(turn)).min((rot - c.theta()).rem_euclid(turn));
    println!("{:?} {:?}, off the spot {:.0} {:.0}, facing off {:.2} deg", a.myaction, c.actiontype, d.x, d.z, off.to_degrees());
    assert_eq!(a.myaction, MyAction::Defend);
    assert!(d.x.abs() <= 40.0 && d.z.abs() <= 40.0, "on the spot");
    assert!(off.to_degrees() < 1.0, "facing the player's way");
}

/// With an enemy in sight of the spot, Hold stays put and Defend breaks off
/// to attack.
#[test]
fn defend_breaks_off_for_an_enemy_and_hold_does_not() {
    for hold in [true, false] {
        let (mut w, s) = team_arena(2);
        stand(&mut w, s[0], -400.0, -400.0);
        stand(&mut w, 0, 400.0, 0.0);
        w.botcmd_apply(s[0], if hold { AIBOTCMD_HOLD } else { AIBOTCMD_DEFEND }, 0);
        stand(&mut w, 0, -400.0, 600.0);
        w.bot_brains = true;
        let mut attacked = false;
        for _ in 0..60 * 8 {
            harness::step_idle(&mut w);
            // The enemy is held in the open, in sight of the spot.
            stand(&mut w, s[1], 600.0, -600.0);
            attacked |= w.ab(s[0]).myaction == MyAction::Attack;
        }
        println!("hold {hold}: {:?}, attacked {attacked}", w.ab(s[0]).myaction);
        assert_eq!(attacked, !hold);
    }
}
