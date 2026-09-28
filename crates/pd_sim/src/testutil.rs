//! The guns' test harness, shared by the gun and prop tests: the firing range
//! with one player (`stage::fixtures::firing_range`), stepping it, and the
//! spike's input helpers.

use std::sync::{Arc, OnceLock};

use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::mp::{MatchPlayer, MatchSetup};

use crate::player::PlayerInput;
use crate::stage::{fixtures, Stage, TileLevel};
use crate::world::{World, WorldRes};

pub fn assets() -> AssetDir {
    AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))
}

pub fn res() -> Arc<WorldRes> {
    static RES: OnceLock<Arc<WorldRes>> = OnceLock::new();
    RES.get_or_init(|| Arc::new(WorldRes::load(&assets()).expect("assets/"))).clone()
}

/// One player in the firing range, with its boards, as the spike's `Sim::new`.
pub fn range() -> World {
    range_with(fixtures::firing_range())
}

pub fn range_with(geom: crate::stage::LevelGeom) -> World {
    let stage = Stage::fixture("range", geom, &[fixtures::FIRING_RANGE_SPAWN]);
    let level = TileLevel::new(stage.geom.clone());
    let setup = MatchSetup { players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
    let mut w = World::new(setup, Arc::new(stage), Arc::new(level), res(), 0x1234_5678).unwrap();
    w.boards = fixtures::firing_range_boards();
    w
}

pub fn run(w: &mut World, input: &PlayerInput, n: usize) {
    for _ in 0..n {
        w.step(4, std::slice::from_ref(input));
    }
}

pub fn idle(w: &mut World, n: usize) {
    run(w, &PlayerInput::default(), n);
}

/// Equip `weapon` and let it come up (the spike's `settled_sim`).
pub fn settled(weapon: u8) -> World {
    let mut w = range();
    run(&mut w, &PlayerInput { select: Some((weapon, false)), ..Default::default() }, 1);
    idle(&mut w, 200);
    w
}

pub fn fire_once(w: &mut World) {
    run(w, &PlayerInput { fire: true, ..Default::default() }, 1);
}

/// Hold B until the gun function toggles, then let it settle.
pub fn secondary(w: &mut World) {
    run(w, &PlayerInput { use_held: true, ..Default::default() }, 30);
    idle(w, 80);
}

pub fn sounds(events: &[Event]) -> Vec<u16> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Sound { sound, .. } | Event::HandleSound { sound, .. } => Some(*sound),
            _ => None,
        })
        .collect()
}


/// `n` players in the firing range, the rest spread along the hall.
pub fn range_players(n: usize) -> World {
    let stage = Stage::fixture("range", fixtures::firing_range(), &[fixtures::FIRING_RANGE_SPAWN]);
    let level = TileLevel::new(stage.geom.clone());
    let players = (0..n).map(|i| MatchPlayer { slot: i as u8, handicap: 128, ..Default::default() }).collect();
    let setup = MatchSetup { players, ..Default::default() };
    let mut w = World::new(setup, Arc::new(stage), Arc::new(level), res(), 0x1234_5678).unwrap();
    w.boards = fixtures::firing_range_boards();
    w
}

/// Step with a different input per player.
pub fn run_all(w: &mut World, inputs: &[PlayerInput], n: usize) {
    for _ in 0..n {
        w.step(4, inputs);
    }
}

/// Hold the trigger three frames (a throw's wind-up and release).
pub fn throw_once(w: &mut World) {
    run(w, &PlayerInput { fire: true, ..Default::default() }, 3);
}

