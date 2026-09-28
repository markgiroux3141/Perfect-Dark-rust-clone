//! Headless matches for tests, probes and `pd_lab`: a setup with `n`
//! simulants, the spike's mixed loadout, a world on a stage with PD's graph or
//! ours, and the A/B measurements ([`abtest`]).
//!
//! Source: the old repo's `pd_spike/sim.rs` (`SimConfig`) and `abtest.rs`.

pub mod abtest;

use std::sync::Arc;

use pd_core::ids::*;
use pd_core::mp::{MatchChr, MatchPlayer, MatchSetup, MatchSimulant};

use crate::nav::gen::{generate, GenParams};
use crate::nav::NavGraph;
use crate::stage::{Stage, TileLevel};
use crate::world::{World, WorldRes};

/// The spike's per-simulant weapons (`SimConfig::from_env`'s mix), by slot.
pub const SPIKE_MIX: [u8; 6] = [WEAPON_AR34, WEAPON_CMP150, WEAPON_FALCON2, WEAPON_DRAGON, WEAPON_K7AVENGER, WEAPON_DY357MAGNUM];

/// The spike's default seed (`PD_SEED`).
pub const SPIKE_SEED: u64 = 0xab8d_9f77_8128_0783;

/// Which route graph the simulants use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavChoice {
    /// The stage's hand-placed pads, waypoints and waygroups.
    Pd,
    /// Ours, generated from the geometry (`nav::gen`).
    Ours,
}

/// A match setup: `players` humans in slots 0.., `bots` simulants of
/// `difficulty` in slots 4.., bodies in turn from the MP list, and no time or
/// score limits (the harness measures behaviour, and a match that ends stops
/// everything).
pub fn setup(players: usize, bots: usize, difficulty: u8) -> MatchSetup {
    let players = (0..players).map(|i| MatchPlayer { slot: i as u8, handicap: 128, chr: MatchChr { name: format!("Player {}", i + 1), mpbodynum: MPBODY_DARK_COMBAT, ..Default::default() }, ..Default::default() }).collect();
    let simulants = (0..bots)
        .map(|k| MatchSimulant {
            slot: 4 + k as u8,
            chr: MatchChr { name: format!("Sim {}", k + 1), mpbodynum: (k as u8 * 3) % 60, mpheadnum: (k as u8 * 5) % 40, team: 0, displayoptions: 0 },
            bottype: BOTTYPE_GENERAL,
            difficulty,
        })
        .collect();
    MatchSetup { players, simulants, timelimit: 60, scorelimit: 100, teamscorelimit: 400, ..MatchSetup::default() }
}

/// The Combat Simulator weapon set the tools start with when not handing out
/// the spike's mix: the Falcon 2, CMP150, shotgun, grenades, rocket launcher
/// and shield (`WEAPON_*`).
pub const DEFAULT_SET: [u8; 6] = [WEAPON_FALCON2, WEAPON_CMP150, WEAPON_SHOTGUN, WEAPON_GRENADE, WEAPON_ROCKETLAUNCHER, WEAPON_MPSHIELD];

/// `setup` with the weapon slots holding `weapons` (`WEAPON_*`, as the menus'
/// `MPWEAPON_*` indexes).
pub fn with_weapons(mut setup: MatchSetup, weapons: &[u8; 6]) -> MatchSetup {
    for (s, &w) in weapons.iter().enumerate() {
        setup.weapons[s] = pd_core::mp::mpweapon_index(w).unwrap_or(0);
    }
    setup
}

/// A world on `stage` from `setup`, routing on `nav`. With `mix` the harness
/// arms everyone (not PD, which starts them unarmed): the simulants with
/// [`SPIKE_MIX`] (single-wielded), the players with every weapon.
pub fn world(stage: Arc<Stage>, level: Arc<TileLevel>, res: Arc<WorldRes>, setup: MatchSetup, nav: NavChoice, seed: u64, mix: bool) -> Result<World, String> {
    let order = res.gset.order.clone();
    let mut w = World::new(setup, stage, level.clone(), res, seed)?;
    if nav == NavChoice::Ours {
        w.nav = Arc::new(generate(&level, &GenParams::default()).graph);
    }
    if mix {
        w.bot_loadout = (0..w.setup.simulants.len()).map(|k| Some((SPIKE_MIX[k % SPIKE_MIX.len()], false))).collect();
        if !w.players.is_empty() {
            w.harness_give_loadout(order);
        }
    }
    Ok(w)
}

/// The simulant spike's arena (`stage::fixtures::arena`: a 16 m box with four
/// pillars) as a stage: eight pads 120 cm in from the walls (corners and
/// middles, the spike's `spawn_pads`) facing the centre, routed on a 250 cm
/// grid (`NavGraph::grid`).
pub fn arena(res: Arc<WorldRes>, setup: MatchSetup, seed: u64) -> Result<World, String> {
    use crate::stage::fixtures::{arena, ARENA_HALF};
    let e = ARENA_HALF - 120.0;
    let spots = [(-e, -e), (0.0, -e), (e, -e), (e, 0.0), (e, e), (0.0, e), (-e, e), (-e, 0.0)];
    let spawns: Vec<(glam::Vec3, glam::Vec3)> = spots.iter().map(|&(x, z)| (glam::Vec3::new(x, 50.0, z), glam::Vec3::new(-x, 0.0, -z).normalize_or_zero())).collect();
    let stage = Stage::fixture("arena", arena(), &spawns);
    let level = Arc::new(TileLevel::new(stage.geom.clone()));
    let mut w = World::new(setup, Arc::new(stage), level.clone(), res, seed)?;
    w.nav = Arc::new(NavGraph::grid(&level, 250.0));
    Ok(w)
}

/// One frame at 60 Hz with every human idle.
pub fn step_idle(w: &mut World) {
    w.step(4, &[]);
}

/// A generated graph, for callers that want to keep it.
pub fn generated_graph(level: &TileLevel) -> NavGraph {
    generate(level, &GenParams::default()).graph
}

/// Not PD: stand chr `i` (a simulant) on the floor at `feet` facing `angle`,
/// as a respawn puts it (`chr_move_to_pos`, from a pad's height), idle.
pub fn place(w: &mut World, i: usize, feet: glam::Vec3, angle: f32) {
    w.chr_move_to_pos(i, feet + glam::Vec3::Y * 50.0, angle);
    let lvframe60 = w.lv.lvframe60;
    let c = &mut w.chrs[i];
    c.actiontype = crate::chr::Act::Stand;
    c.lastmoveok60 = lvframe60;
    if let Some(a) = c.aibot.as_mut() {
        a.moveratex = 0.0;
        a.moveratey = 0.0;
        a.shotspeed = glam::Vec3::ZERO;
        a.roty = angle;
    }
}

/// Not PD: stand player `i` on the floor at `feet` facing `theta` degrees,
/// its chr following.
pub fn place_player(w: &mut World, i: usize, feet: glam::Vec3, theta: f32) {
    w.players[i].place(feet, theta);
    w.sync_player_chr(i);
}
