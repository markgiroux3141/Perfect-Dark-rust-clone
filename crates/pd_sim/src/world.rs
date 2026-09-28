//! `World`: one `Lv`, one `Rng`, the stage, the human players (simulants join
//! them in one chr list in M6, as PD has), and the event queue. [`World::step`]
//! runs one PD frame in PD's order for all players at once; it is sized for up
//! to four humans, and split-screen is a renderer concern.
//!
//! M3 has the frame's first stage: `lv_tick`, then each player's `bmove_tick`
//! (walk, look, footsteps). The guns (`bgun_tick_gameplay`, `hands_tick_attack`,
//! M4), chrs and simulants (M6), props (M5/M8) and match rules (M7) slot in
//! after it, in the order `docs/ARCHITECTURE.md` § A frame gives.
//!
//! Source: `pd_complex/fight.rs` `Fight::frame`, which glued the two spike sims.

use std::sync::Arc;

use pd_core::anim::AnimBank;
use pd_core::events::Event;
use pd_core::lv::{Lv, LvTickIn};
use pd_core::mp::MatchSetup;
use pd_core::rng::Rng;

use crate::player::{player_choose_spawn_location, Player, PlayerInput, SpawnOther, WalkEnv};
use crate::stage::{PerimCyl, Stage, TileLevel};

pub struct World {
    pub setup: MatchSetup,
    /// The frame timing, `g_Vars`' `lv*` fields: one for the whole world.
    pub lv: Lv,
    /// `random()`'s state: one stream for the whole world.
    pub rng: Rng,
    pub stage: Arc<Stage>,
    pub level: Arc<TileLevel>,
    /// One per human in `setup.players`, in that order.
    pub players: Vec<Player>,
    /// How many times each player has (re)spawned.
    pub spawns: Vec<u32>,
    events: Vec<Event>,
}

impl World {
    /// Start a match: every player spawns in turn, each choosing a pad away
    /// from those already placed (`player_choose_spawn_location`).
    pub fn new(setup: MatchSetup, stage: Arc<Stage>, level: Arc<TileLevel>, bank: Arc<AnimBank>, seed: u64) -> Result<World, String> {
        if stage.spawn_pads.is_empty() {
            return Err(format!("stage {} has no spawn pads", stage.code));
        }
        let mut rng = Rng::new(seed);
        let n = setup.players.len();
        let mut players = Vec::with_capacity(n);
        for _ in 0..n {
            players.push(Player::new(bank.clone(), glam::Vec3::ZERO, 0.0, n, &mut rng)?);
        }
        let mut w = World { setup, lv: Lv::new(), rng, stage, level, players, spawns: vec![0; n], events: Vec::new() };
        for i in 0..n {
            let before: Vec<usize> = (0..i).collect();
            w.spawn_player(i, &before);
        }
        Ok(w)
    }

    /// The perimeters of every player but `except`.
    fn perims_except(&self, except: usize) -> Vec<PerimCyl> {
        self.players.iter().enumerate().filter(|&(j, _)| j != except).map(|(_, p)| p.perim()).collect()
    }

    /// Spawn player `i`, judging the pads against the players in `others`
    /// (at the match start, the ones already spawned; later, everyone else).
    fn spawn_player(&mut self, i: usize, others: &[usize]) {
        let judged: Vec<SpawnOther> =
            others.iter().map(|&j| &self.players[j]).map(|p| SpawnOther { pos: p.pos, rooms: p.floorroom.into_iter().collect() }).collect();
        let cyls: Vec<PerimCyl> = others.iter().map(|&j| self.players[j].perim()).collect();
        let (pos, angle) = player_choose_spawn_location(&self.level, &self.stage, 30.0, &judged, &cyls, &mut self.rng);
        self.players[i].start_new_life(&self.level, pos, angle);
        self.spawns[i] += 1;
    }

    /// One PD frame `diffframe240` quarter-ticks long (4 at 60 Hz, 8 at 30,
    /// 12 at 20), with each player's controls (missing inputs are idle).
    pub fn step(&mut self, diffframe240: i32, inputs: &[PlayerInput]) {
        // lv_tick. M7: the slow-motion option and Combat Boost (M5) feed LvTickIn.
        self.lv.frame(diffframe240, LvTickIn::default());
        let idle = PlayerInput::default();
        for i in 0..self.players.len() {
            let cyls = self.perims_except(i);
            let env = WalkEnv { level: &self.level, cyls: &cyls };
            let input = inputs.get(i).unwrap_or(&idle);
            self.players[i].tick(input, &self.lv, &env, &mut self.rng, &mut self.events);
            if self.players[i].die_request {
                // SUBST: PD kills the player (fell for 4 s or out of the world)
                // and runs the death sequence before the respawn / no deaths
                // until M6-M7, so the player respawns at once.
                let others: Vec<usize> = (0..self.players.len()).filter(|&j| j != i).collect();
                self.spawn_player(i, &others);
            }
        }
    }

    /// What happened since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}
