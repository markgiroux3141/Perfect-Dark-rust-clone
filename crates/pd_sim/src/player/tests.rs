//! The player's walk on Complex: every link of PD's waypoint graph walked with
//! the ported `bondwalk.c`, ledge drops, the head bob, footsteps, spawning.
//! The walk harness comes from the old repo's `pd_complex/tests.rs`.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use pd_core::anim::AnimBank;
use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_core::lv::{Lv, LvTickIn};
use pd_core::rng::Rng;

use super::*;
use crate::chr::chr_adjust_pos_for_spawn;
use crate::stage::{FloorKind, GeomPoly, Stage, TileLevel};

fn assets() -> AssetDir {
    AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))
}

fn complex() -> (Stage, TileLevel) {
    let stage = Stage::load(&assets(), "ref").expect("assets/stages/ref");
    let level = TileLevel::new(stage.geom.clone());
    (stage, level)
}

/// The player alone on a stage, stepped at 60 Hz.
struct Walker {
    p: Player,
    lv: Lv,
    rng: Rng,
    events: Vec<Event>,
}

impl Walker {
    fn new() -> Walker {
        let bank = Arc::new(AnimBank::load(&assets()).expect("assets/anims"));
        let mut rng = Rng::new(0);
        let p = Player::new(bank, Vec3::ZERO, 0.0, 1, &mut rng).unwrap();
        Walker { p, lv: Lv::new(), rng, events: Vec::new() }
    }

    fn frame(&mut self, level: &TileLevel, input: &PlayerInput) {
        self.lv.frame(4, LvTickIn::default());
        let env = WalkEnv { level, cyls: &[] };
        self.p.tick(input, &self.lv, &env, &mut self.rng, &mut self.events);
    }
}

/// `vv_theta` (degrees; forward = (−sin, 0, cos)) that faces from `a` to `b`.
fn theta_towards(a: Vec3, b: Vec3) -> f32 {
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let t = (-dx).atan2(dz).to_degrees();
    if t < 0.0 {
        t + 360.0
    } else {
        t
    }
}

/// The floor a PD pad belongs to. PD places pads 52–121 cm above their floor
/// (measured over Complex's waypoint and spawn pads), and some sit on a railing
/// line or a walkway edge, just off their tile. So: the highest floor polygon
/// within 60 cm (XZ) of the pad whose height there is 40–130 cm below it.
fn pd_pad_floor(level: &TileLevel, pad: Vec3) -> Option<f32> {
    let mut best: Option<f32> = None;
    for p in &level.geom.polys {
        if !matches!(p.floor_kind(), Some(FloorKind::Flat | FloorKind::Ramp)) {
            continue;
        }
        let q = nearest_point_xz(p, Vec2::new(pad.x, pad.z));
        if q.distance(Vec2::new(pad.x, pad.z)) > 60.0 {
            continue;
        }
        let y = p.find_y(q.x, q.y);
        if (40.0..=130.0).contains(&(pad.y - y)) && best.is_none_or(|by| y > by) {
            best = Some(y);
        }
    }
    best
}

fn nearest_point_xz(p: &GeomPoly, pt: Vec2) -> Vec2 {
    if p.xz_in_convex(pt.x, pt.y) {
        return pt;
    }
    let v = &p.verts;
    let mut best = Vec2::new(v[0].x, v[0].z);
    for i in 0..v.len() {
        let (a, b) = (Vec2::new(v[i].x, v[i].z), Vec2::new(v[(i + 1) % v.len()].x, v[(i + 1) % v.len()].z));
        let ab = b - a;
        let t = if ab.length_squared() > 0.0 { ((pt - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        let c = a + ab * t;
        if c.distance(pt) < best.distance(pt) {
            best = c;
        }
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Arrived,
    WrongFloor,
    Stuck,
    Timeout,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct PlayerWalk {
    outcome: Outcome,
    ticks: i32,
    end: Vec3,
    /// Largest `manground − ground` seen: past 69 it went off a ledge.
    max_drop: f32,
    landed: bool,
}

/// TEST HARNESS (not PD): drive the player from the floor under pad `from` to
/// pad `to` by holding forward and turning the view straight at the pad each
/// tick (the turn is set directly, as a perfect mouse would), crouched if the
/// goal is a crouch pad. Everything else is the ported walk.
fn walk_player(w: &mut Walker, level: &TileLevel, from: Vec3, to: Vec3, to_floor: Option<f32>, crouch: bool) -> PlayerWalk {
    // Placed as PD spawns a player: a clear 30 cm cylinder at or around the pad
    // (`chr_adjust_pos_for_spawn`), stood on its floor.
    let spot = chr_adjust_pos_for_spawn(level, 30.0, from, 0.0, &[]).unwrap_or(from);
    let start = level.drop_to_ground(spot);
    w.p.place(start, theta_towards(start, to));
    w.p.crouchpos = if crouch { CROUCHPOS_SQUAT } else { CROUCHPOS_STAND };
    // A start inside the crawl space is already squatting (standing up in there
    // is refused by the ceiling, and so is crouching while the head is in it).
    if crouch {
        w.p.set_crouch_offset(-90.0);
    }
    // PD's pad heights lie for some pads (the pit's sit 118 cm above the rim), so
    // either the pad's own floor or the floor under it counts, as for the bots.
    let under = level.drop_to_ground(to).y;
    let goal_floor = to_floor.unwrap_or(under);
    let dist = Vec2::new(to.x - start.x, to.z - start.z).length();
    // The player runs ~8 cm a tick standing, 0.35× that squatting.
    let speed = if crouch { 2.5 } else { 6.0 };
    let limit = (dist / speed) as i32 * 2 + 120;
    let mut still = 0;
    let mut max_drop = 0.0f32;
    let mut landed = false;
    let input = PlayerInput { walk_y: 127, ..PlayerInput::default() };
    for t in 0..limit {
        let p = w.p.pos;
        w.p.theta = theta_towards(p, to);
        let before = w.p.pos;
        w.frame(level, &input);
        max_drop = max_drop.max(w.p.manground - w.p.ground);
        landed |= w.p.landed.is_some();
        let p = w.p.pos;
        let xz = Vec2::new(to.x - p.x, to.z - p.z).length();
        // By the floor under the cylinder: going up a ramp, `manground` trails it
        // by ~24 cm (the step low-pass).
        let floor = w.p.ground;
        let on_floor = (floor - goal_floor).abs() <= 40.0 || (floor - under).abs() <= 40.0;
        // Mid-fall over the goal is not arriving: wait for the landing.
        let falling = w.p.isfalling || w.p.manground > w.p.ground + 1.0;
        if xz <= 40.0 && !falling {
            let outcome = if on_floor || w.p.onladder { Outcome::Arrived } else { Outcome::WrongFloor };
            return PlayerWalk { outcome, ticks: t, end: p, max_drop, landed };
        }
        if Vec2::new(p.x - before.x, p.z - before.z).length() < 0.05 && !w.p.onladder {
            still += 1;
            if still > 90 {
                return PlayerWalk { outcome: Outcome::Stuck, ticks: t, end: p, max_drop, landed };
            }
        } else {
            still = 0;
        }
    }
    PlayerWalk { outcome: Outcome::Timeout, ticks: limit, end: w.p.pos, max_drop, landed }
}

fn crouch_link(stage: &Stage, a: usize, b: usize) -> bool {
    [a, b].iter().any(|&w| stage.pads[stage.waypoints[w].padnum].has(PADFLAG_AICROUCH))
}

/// The player walks PD's own graph: stairs, ramps, ledge drops, the crawl space
/// (crouched) and the ladder, with the ported `bondwalk.c`. The only links it
/// fails are the ones the simulant spike found bad for bots too, for the same
/// reason: `0x01 -> 0x03` and `0x88 -> 0x8a` climb a sheer 276 cm pit wall
/// (PD's data marks them two-way); `0x0f -> 0x0e` starts at a walkway-edge pad,
/// so a start dropped to the floor under it lands on the floor below.
#[test]
fn the_player_walks_every_link_of_pds_complex_graph_but_three_known_ones() {
    let (stage, level) = complex();
    let mut w = Walker::new();
    let mut failed = Vec::new();
    let links = stage.waypoint_links();
    assert_eq!(links.len(), 401);
    for &(a, b, _) in &links {
        let (pa, pb) = (stage.waypoint_pos(a), stage.waypoint_pos(b));
        let r = walk_player(&mut w, &level, pa, pb, pd_pad_floor(&level, pb), crouch_link(&stage, a, b));
        if r.outcome != Outcome::Arrived {
            failed.push((a, b, r.outcome));
        }
    }
    let known = [(0x01, 0x03), (0x0f, 0x0e), (0x88, 0x8a)];
    let got: Vec<(usize, usize)> = failed.iter().map(|&(a, b, _)| (a, b)).collect();
    assert_eq!(got, known, "{failed:x?}");
}

/// Off a ledge the player falls with PD's gravity, lands on the floor below
/// (not through it, not hovering) and dips on landing.
#[test]
fn walking_off_a_ledge_falls_and_lands_on_the_floor_below() {
    let (stage, level) = complex();
    let mut w = Walker::new();
    let mut drops = 0;
    // PD's one-way links are its ledge drops.
    for (a, b, one_way) in stage.waypoint_links() {
        if !one_way {
            continue;
        }
        let (pa, pb) = (stage.waypoint_pos(a), stage.waypoint_pos(b));
        // Where the walk really starts and ends (pad heights lie for some pads).
        let fa = level.drop_to_ground(chr_adjust_pos_for_spawn(&level, 30.0, pa, 0.0, &[]).unwrap_or(pa)).y;
        let fb = pd_pad_floor(&level, pb).unwrap_or_else(|| level.drop_to_ground(pb).y);
        if fa - fb < 100.0 {
            continue;
        }
        let r = walk_player(&mut w, &level, pa, pb, Some(fb), false);
        assert_eq!(r.outcome, Outcome::Arrived, "drop {a:#x} -> {b:#x}: {r:?}");
        assert!(r.max_drop > 69.0, "{a:#x} -> {b:#x} should fall, not step: {r:?}");
        assert!(r.landed, "{a:#x} -> {b:#x}: a {:.0} cm fall should land hard enough to dip", fa - fb);
        assert!((w.p.manground - w.p.ground).abs() < 1.0, "stands on the floor after landing");
        drops += 1;
    }
    assert!(drops >= 3, "only {drops} drops of 1 m+ in PD's graph");
}

/// Held forward, the head-bob model eases the player in from standing to PD's
/// run (~8 cm a tick with the 3 s speed boost still off), and letting go eases
/// them out: there is no acceleration constant, it is the clips' root motion.
#[test]
fn the_walk_eases_in_to_a_run_and_out_to_a_stop() {
    let level = TileLevel::new(crate::stage::fixtures::firing_range());
    let mut w = Walker::new();
    w.p.place(Vec3::new(0.0, 0.0, 0.0), 0.0);
    let fwd = PlayerInput { walk_y: 127, ..PlayerInput::default() };
    let mut speeds = Vec::new();
    for _ in 0..90 {
        let z = w.p.pos.z;
        w.frame(&level, &fwd);
        speeds.push(w.p.pos.z - z);
    }
    assert!(speeds[0] < 1.0, "the first tick barely moves: {:.2}", speeds[0]);
    let to6 = speeds.iter().position(|&s| s >= 6.0).unwrap();
    assert!((8..16).contains(&to6), "reaches 6 cm/tick after {to6} ticks");
    let top = speeds[60..].iter().copied().fold(0.0, f32::max);
    assert!((6.0..10.0).contains(&top), "running speed {top:.2} cm/tick");
    assert!(speeds.iter().all(|&s| s >= 0.0), "never backwards");
    assert_eq!(w.p.headanim, HEADANIM_MOVING, "running uses the run clip");
    // Let go: the head's damped root motion decays by 0.982 a quarter-tick
    // (about 0.93 a frame), so the stop is an exponential ease, not a brake.
    let mut stop = 0;
    let mut last = f32::MAX;
    while last > 1.0 && stop < 120 {
        let z = w.p.pos.z;
        w.frame(&level, &PlayerInput::default());
        last = w.p.pos.z - z;
        stop += 1;
    }
    assert!((20..40).contains(&stop), "slows below 1 cm/tick after {stop} ticks");
    // The eye rides at the eye height over flat floor, bobbing a few cm.
    assert!((w.p.pos.y - 159.0).abs() < 8.0, "eye at {:.1}", w.p.pos.y);
}

/// Complex's floors are metal: walking makes PD's metal footsteps, one every
/// 150 cm, and none while standing still.
#[test]
fn walking_on_complex_makes_metal_footsteps_every_150cm() {
    let (stage, level) = complex();
    let mut w = Walker::new();
    // Spawned as PD spawns, facing along the pad into the room.
    let pad = &stage.pads[stage.spawn_pads[0]];
    w.p.start_new_life(&level, pad.pos, pad.look_angle());
    for _ in 0..60 {
        w.frame(&level, &PlayerInput::default());
    }
    assert!(w.events.is_empty(), "standing still is silent");
    let metal = &crate::chr::FOOTSTEP_SOUNDS[FLOORTYPE_METAL as usize * 8..FLOORTYPE_METAL as usize * 8 + 8];
    let start = w.p.pos;
    let mut walked = 0.0;
    for _ in 0..60 {
        let before = w.p.pos;
        w.frame(&level, &PlayerInput { walk_y: 127, ..PlayerInput::default() });
        walked += (w.p.pos - before).length();
    }
    let steps: Vec<u16> = w.events.iter().map(|e| match e {
        Event::Sound { sound, .. } => *sound,
    }).collect();
    assert!(walked > 300.0, "walked only {walked:.0} cm from {start}");
    assert!(steps.iter().all(|s| metal.contains(s)), "{steps:x?}");
    // `footstepdist` restarts from 0 at each step, dropping the overshoot (under
    // a tick's travel), so the count is just under one per 150 cm.
    let n = steps.len();
    assert!((walked / 160.0) as usize <= n && n <= (walked / 150.0) as usize, "{n} steps over {walked:.0} cm");
}

/// Crouching lowers the eye by PD's offsets (−90 squat, −45 duck) and slows the
/// walk; standing up under a low ceiling is refused.
#[test]
fn crouching_lowers_the_eye_and_a_ceiling_keeps_you_down() {
    let level = TileLevel::new(crate::stage::fixtures::firing_range());
    let mut w = Walker::new();
    w.p.place(Vec3::ZERO, 0.0);
    let down = PlayerInput { crouch_down: true, ..PlayerInput::default() };
    w.frame(&level, &down);
    w.frame(&level, &down);
    for _ in 0..60 {
        w.frame(&level, &PlayerInput::default());
    }
    assert_eq!(w.p.crouchpos, CROUCHPOS_SQUAT);
    assert_eq!(w.p.crouchoffset, -90.0);
    assert!((w.p.pos.y - (159.0 - 90.0)).abs() < 8.0, "squatting eye at {:.1}", w.p.pos.y);
    // A 100 cm crawl space: a slab from 100 cm up over the player.
    let mut geom = crate::stage::fixtures::firing_range();
    let slab = [Vec3::new(-100.0, 100.0, -100.0), Vec3::new(100.0, 100.0, -100.0), Vec3::new(100.0, 100.0, 100.0), Vec3::new(-100.0, 100.0, 100.0)];
    geom.polys.push(GeomPoly::new(slab.to_vec(), false, true, true, true, Some(0)));
    let low = TileLevel::new(geom);
    let up = PlayerInput { crouch_up: true, ..PlayerInput::default() };
    w.frame(&low, &up);
    w.frame(&low, &up);
    for _ in 0..60 {
        w.frame(&low, &PlayerInput::default());
    }
    assert!(w.p.crouchoffset < -45.0, "stood up into the ceiling: offset {}", w.p.crouchoffset);
}

/// A match on Complex spawns each player on one of the stage's spawn spots,
/// standing on its floor and facing along the pad.
#[test]
fn players_spawn_on_complexs_spawn_pads() {
    use pd_core::mp::{MatchPlayer, MatchSetup};
    let (stage, level) = complex();
    let bank = Arc::new(AnimBank::load(&assets()).unwrap());
    let setup = MatchSetup {
        stagenum: STAGE_MP_COMPLEX,
        players: (0..4).map(|slot| MatchPlayer { slot, handicap: 128, ..Default::default() }).collect(),
        ..Default::default()
    };
    let world = crate::world::World::new(setup, Arc::new(stage.clone()), Arc::new(level), bank, 1).unwrap();
    assert_eq!(world.players.len(), 4);
    for p in &world.players {
        let pad = stage
            .spawn_pads
            .iter()
            .map(|&i| &stage.pads[i])
            .find(|pad| Vec2::new(pad.pos.x - p.pos.x, pad.pos.z - p.pos.z).length() < 61.0)
            .unwrap_or_else(|| panic!("player at {} is not at a spawn pad", p.pos));
        assert!((p.pos.y - p.manground - p.eyeheight).abs() < 0.01);
        assert!((pad.pos.y - p.manground - 55.0).abs() < 10.0, "feet {:.0} under pad {:.0}", p.manground, pad.pos.y);
        let facing = p.theta_vec();
        assert!(facing.dot(Vec3::new(pad.look.x, 0.0, pad.look.z).normalize()) > 0.99, "faces along the pad's look");
    }
    // Four different spots, well apart (no one spawns on anyone).
    for i in 0..4 {
        for j in i + 1..4 {
            assert!(world.players[i].pos.distance(world.players[j].pos) > 100.0);
        }
    }
}

/// The same seed and inputs give the same match, bit for bit.
#[test]
fn a_seeded_walk_is_reproducible() {
    use pd_core::mp::{MatchPlayer, MatchSetup};
    let run = || {
        let (stage, level) = complex();
        let bank = Arc::new(AnimBank::load(&assets()).unwrap());
        let setup = MatchSetup { stagenum: STAGE_MP_COMPLEX, players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
        let mut world = crate::world::World::new(setup, Arc::new(stage), Arc::new(level), bank, 7).unwrap();
        for t in 0..600 {
            let input = PlayerInput { walk_y: 127, mouse_dx: if (t / 90) % 2 == 0 { 3.0 } else { -2.0 }, ..PlayerInput::default() };
            world.step(4, &[input]);
        }
        (world.players[0].pos, world.players[0].theta, world.rng.seed)
    };
    assert_eq!(run(), run());
}

/// Where each spawn pad's floor is, by PD's placement and by the spike's
/// harness drop. `cargo test -p pd_sim --release probe_spawn_floors -- --ignored --nocapture`
#[test]
#[ignore]
fn probe_spawn_floors() {
    let (stage, level) = complex();
    for &p in &stage.spawn_pads {
        let pos = stage.pads[p].pos;
        let pd = level.cd_find_ground_at_cyl(pos, 30.0).0;
        let spike = level.drop_to_ground(pos).y;
        println!("pad {p:#06x} at {pos:.0?}: PD ground {pd:.1}, spike drop {spike:.1}");
    }
}
