//! `World`: one `Lv`, one `Rng`, the stage, the human players (simulants join
//! them in one chr list in M6, as PD has), the effects, and the event queue.
//! [`World::step`] runs one PD frame in PD's order for all players at once; it is
//! sized for up to four humans, and split-screen is a renderer concern.
//!
//! The frame, as `main.c:1043` runs it:
//! 1. `lv_tick`: the timing, then `casings_tick`, `sparks_tick`, and `props_tick`
//!    (each player's tracers, the explosions, the smoke);
//! 2. each player's `player_tick`: `bmove_tick` (the controls, the hands' state
//!    machines in `bgun_tick_gameplay`, the walk) and the camera;
//! 3. `lv_render`, per player: `lights_tick`, `hands_tick_attack` (the shots,
//!    which set `hitpos`), then `player_render_hud` → `bgun_tick_gameplay2` (the
//!    gun's pose; its tracer runs from the muzzle to `hitpos`) and the HUD's
//!    timers.
//!
//! Chrs and simulants (M6), props (M5/M8) and the match rules (M7) slot in
//! where PD ticks them.
//!
//! Source: `pd_complex/fight.rs` `Fight::frame` and `pd_guns/sim.rs` `Sim::frame`,
//! which glued the two spike sims.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use pd_core::anim::AnimBank;
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::ids::{HAND_RIGHT, WEAPONFLAG_AIMTRACK};
use pd_core::lv::{Lv, LvTickIn};
use pd_core::model::ModelStore;
use pd_core::mp::MatchSetup;
use pd_core::rng::Rng;

use crate::fx::Fx;
use crate::gun::Gset;
use crate::player::{player_choose_spawn_location, Player, PlayerInput, SpawnOther, WalkEnv};
use crate::props::explosions::{ExpOut, ExpWorld, Explosions, Victim, VictimId};
use crate::propsnd::{self, AudioConfigs, Listener};
use crate::stage::{PerimCyl, Stage, TileLevel};

/// What every world loads once and shares: the animation bank, the weapon
/// table, the model files and the sounds' audio configs.
pub struct WorldRes {
    pub bank: Arc<AnimBank>,
    pub gset: Arc<Gset>,
    pub models: Arc<ModelStore>,
    pub audio: Arc<AudioConfigs>,
}

impl WorldRes {
    pub fn load(assets: &AssetDir) -> Result<WorldRes, String> {
        Ok(WorldRes {
            bank: Arc::new(AnimBank::load(assets)?),
            gset: Arc::new(Gset::load(assets)?),
            models: Arc::new(ModelStore::load(assets)?),
            audio: Arc::new(AudioConfigs::load(assets)?),
        })
    }
}

/// `// SUBST:` PD lights each room from its light data
/// (`br_settled_regional`, `dlights.c`) / every room is settled at 230 until
/// M9's room lighting; it is what the gun spike's range defaulted to.
pub const ROOM_BRIGHTNESS: f32 = 230.0;

/// A room's lighting: `struct room`'s `br_flash` over `br_settled_regional`.
#[derive(Clone, Copy, Debug)]
pub struct RoomLight {
    pub br_settled_regional: f32,
    pub br_flash: i32,
}

impl RoomLight {
    /// `room_get_final_brightness_for_player` (`dlights.c:106`).
    pub fn final_brightness(&self) -> f32 {
        (self.br_flash as f32 + self.br_settled_regional).clamp(0.0, 255.0)
    }

    /// `room_flash_lighting(room, start, limit)` (`dlights.c:1567`) for the room
    /// itself (its light transfer to itself is full, so the step is `start`),
    /// then `room_flash_local_lighting` (`:1599`).
    pub fn flash(&mut self, start: f32, limit: i32) {
        let v = start * 5.0;
        let increment = if start > 0.0 { v.min(start) } else { v.max(start) } as i32;
        if increment > 0 {
            if self.br_flash < limit {
                self.br_flash = (self.br_flash + increment).min(limit);
            }
        } else if self.br_flash > limit {
            self.br_flash = (self.br_flash + increment).max(limit);
        }
    }

    /// The flash's decay in `lights_tick` (`dlights.c:1411`).
    pub fn tick(&mut self, lv: &Lv) {
        if self.br_flash != 0 {
            let mut increment = lv.lvupdate240 * 2;
            if self.br_flash > 0 {
                increment = increment.min(self.br_flash);
                self.br_flash -= increment;
            } else {
                // PD's @bug branch, kept: br_flash is <= 0 here.
                if increment < self.br_flash {
                    increment = self.br_flash;
                }
                self.br_flash += increment;
            }
        }
    }
}

/// Every room's light, created settled on first use.
#[derive(Clone, Debug, Default)]
pub struct Lights {
    pub rooms: HashMap<u16, RoomLight>,
}

impl Lights {
    pub fn room(&mut self, room: u16) -> &mut RoomLight {
        self.rooms.entry(room).or_insert(RoomLight { br_settled_regional: ROOM_BRIGHTNESS, br_flash: 0 })
    }

    pub fn brightness(&self, room: Option<u16>) -> f32 {
        room.and_then(|r| self.rooms.get(&r)).map_or(ROOM_BRIGHTNESS, RoomLight::final_brightness)
    }

    pub fn tick(&mut self, lv: &Lv) {
        for r in self.rooms.values_mut() {
            r.tick(lv);
        }
    }
}

/// `vi_shake` / `vi_handle_retrace` (`vi.c:453`, `:252`): the whole picture
/// jumps up and down by `intensity` half-lines, alternating every retrace.
#[derive(Clone, Copy, Debug)]
pub struct ViShake {
    pub intensity: f32,
    pub timer: i32,
    pub direction: i32,
    /// This frame's vertical offset, half-lines.
    pub offset: f32,
}

impl Default for ViShake {
    fn default() -> Self {
        ViShake { intensity: 0.0, timer: 0, direction: 1, offset: 0.0 }
    }
}

impl ViShake {
    pub fn shake(&mut self, intensity: f32) {
        self.intensity = intensity.clamp(0.0, 14.0);
        self.timer = 10;
    }

    /// One 60 Hz retrace.
    pub fn retrace(&mut self) {
        if self.timer != 0 {
            self.timer -= 1;
            if self.timer == 0 {
                self.intensity = 0.0;
            }
        }
        self.offset = self.direction as f32 * self.intensity;
        self.direction = -self.direction;
    }
}

/// A target board: the firing range's props, a thin box that counts its hits.
/// They stand in for the objects a shot can hit until props arrive (M8).
#[derive(Clone, Debug)]
pub struct Board {
    pub min: Vec3,
    pub max: Vec3,
    /// The face's centre and half-extents in its plane, for drawing.
    pub face: Vec3,
    pub half: Vec2,
    pub hits: u32,
    pub damage: f32,
    /// 1 on a hit, fading over 20 ticks.
    pub flash: f32,
}

pub struct World {
    pub setup: MatchSetup,
    /// The frame timing, `g_Vars`' `lv*` fields: one for the whole world.
    pub lv: Lv,
    /// `random()`'s state: one stream for the whole world.
    pub rng: Rng,
    pub stage: Arc<Stage>,
    pub level: Arc<TileLevel>,
    pub res: Arc<WorldRes>,
    /// One per human in `setup.players`, in that order.
    pub players: Vec<Player>,
    /// How many times each player has (re)spawned.
    pub spawns: Vec<u32>,
    pub fx: Fx,
    pub explosions: Explosions,
    /// The target boards (the firing range's); none on an arena.
    pub boards: Vec<Board>,
    pub lights: Lights,
    pub vi: ViShake,
    /// Explosion damage each player has taken. M6 applies it (`chr_damage`).
    pub player_damage: Vec<f32>,
    /// Shots each player has fired (`mpstats_increment_player_shotcount`).
    pub shots_fired: Vec<u32>,
    /// `player->lookingatprop.prop`: the board under each player's crosshair
    /// (the sight turns red on it). M6: chrs.
    pub lookingatprop: Vec<Option<usize>>,
    /// `g_20SecIntervalFrac`: the HUD's wave shimmer.
    pub frac20: f32,
    events: Vec<Event>,
}

/// The stage as the explosions see it.
struct StageExp<'a> {
    level: &'a TileLevel,
}

impl ExpWorld for StageExp<'_> {
    /// The floor's room's bbox, from its tiles.
    fn room_bbox(&self, pos: Vec3) -> (Vec3, Vec3) {
        if let Some(room) = self.level.floor_room(pos, 1.0) {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for p in self.level.geom.polys.iter().filter(|p| p.room == Some(room)) {
                for v in &p.verts {
                    lo = lo.min(*v);
                    hi = hi.max(*v);
                }
            }
            if lo.x <= hi.x {
                return (lo, hi);
            }
        }
        let (lo, hi) = self.level.geom.bounds();
        (lo - Vec3::splat(100.0), hi + Vec3::splat(100.0))
    }

    /// `cd_find_room_at_pos_ycnp` over the floor tiles.
    fn floor_below(&self, pos: Vec3) -> Option<(f32, Vec3, bool)> {
        let (y, poly) = self.level.cd_find_ground_at_cyl(pos, 0.1);
        let n = self.level.geom.polys[poly?].normal;
        Some((y, if n.y < 0.0 { -n } else { n }, false))
    }
}

impl World {
    /// Start a match: every player spawns in turn, each choosing a pad away
    /// from those already placed (`player_choose_spawn_location`), with the M4
    /// loadout (see [`World::give_loadout`]).
    pub fn new(setup: MatchSetup, stage: Arc<Stage>, level: Arc<TileLevel>, res: Arc<WorldRes>, seed: u64) -> Result<World, String> {
        if stage.spawn_pads.is_empty() {
            return Err(format!("stage {} has no spawn pads", stage.code));
        }
        let mut rng = Rng::new(seed);
        let n = setup.players.len();
        let mut players = Vec::with_capacity(n);
        for _ in 0..n {
            players.push(Player::new(&res, Vec3::ZERO, 0.0, n, &mut rng)?);
        }
        let mut w = World {
            setup,
            lv: Lv::new(),
            rng,
            stage,
            level,
            res,
            players,
            spawns: vec![0; n],
            fx: Fx::default(),
            explosions: Explosions::default(),
            boards: Vec::new(),
            lights: Lights::default(),
            vi: ViShake::default(),
            player_damage: vec![0.0; n],
            shots_fired: vec![0; n],
            lookingatprop: vec![None; n],
            frac20: 0.0,
            events: Vec::new(),
        };
        for i in 0..n {
            let before: Vec<usize> = (0..i).collect();
            w.spawn_player(i, &before);
            w.give_loadout(i);
        }
        Ok(w)
    }

    /// `// SUBST:` a Combat Simulator player starts unarmed and picks weapons
    /// up from the arena's pads (M8) / until then every player carries every
    /// weapon in the set, twice where it dual-wields, with unlimited ammo, and
    /// starts with the Falcon 2.
    pub fn give_loadout(&mut self, i: usize) {
        let gset = self.res.gset.clone();
        let gun = &mut self.players[i].gun;
        for &wn in &gset.order {
            let dual = gset.has_flag(wn, pd_core::ids::WEAPONFLAG_DUALWIELD);
            gun.give_weapon(&gset, wn, dual);
        }
        gun.p.unlimited_ammo = true;
        gun.bgun_equip_weapon(pd_core::ids::WEAPON_FALCON2);
    }

    /// The perimeters of every player but `except`.
    fn perims_except(&self, except: usize) -> Vec<PerimCyl> {
        self.players.iter().enumerate().filter(|&(j, _)| j != except).map(|(_, p)| p.perim()).collect()
    }

    /// Spawn player `i`, judging the pads against the players in `others`
    /// (at the match start, the ones already spawned; later, everyone else).
    fn spawn_player(&mut self, i: usize, others: &[usize]) {
        let judged: Vec<SpawnOther> = others.iter().map(|&j| &self.players[j]).map(|p| SpawnOther { pos: p.pos, rooms: p.floorroom.into_iter().collect() }).collect();
        let cyls: Vec<PerimCyl> = others.iter().map(|&j| self.players[j].perim()).collect();
        let (pos, angle) = player_choose_spawn_location(&self.level, &self.stage, 30.0, &judged, &cyls, &mut self.rng);
        self.players[i].start_new_life(&self.level, pos, angle);
        self.spawns[i] += 1;
    }

    /// Every player's camera, as the sound code hears from it.
    pub(crate) fn listeners(&self) -> Vec<Listener> {
        self.players.iter().map(|p| Listener { pos: p.cam.pos(), theta: p.theta }).collect()
    }

    /// A sound at a world position, as loud and panned as it is for the players
    /// (`ps_create`, `ps_apply_vol_pan`), with the caller's distances unless the
    /// sound has an audio config.
    pub(crate) fn sound_at(&mut self, sound: u16, pitch: f32, pos: Vec3, dists: [f32; 3]) {
        let (volume, pan) = propsnd::vol_pan(&self.res.audio, sound, pos, dists, &self.listeners());
        if volume > 0.0 {
            self.events.push(Event::Sound { sound, pitch, volume, pan });
        }
    }

    /// A sound with no position (`snd_start`): full volume, centred.
    pub(crate) fn sound(&mut self, sound: u16, pitch: f32) {
        self.events.push(Event::Sound { sound, pitch, volume: 1.0, pan: 0.0 });
    }

    pub(crate) fn push_event(&mut self, e: Event) {
        self.events.push(e);
    }

    /// One PD frame `diffframe240` quarter-ticks long (4 at 60 Hz, 8 at 30,
    /// 12 at 20), with each player's controls (missing inputs are idle).
    pub fn step(&mut self, diffframe240: i32, inputs: &[PlayerInput]) {
        // lv_tick. M7: the slow-motion option and Combat Boost (M5) feed LvTickIn.
        self.lv.frame(diffframe240, LvTickIn::default());
        // menu_tick_timers (`game_006900.c:42`), from lv_tick's menu_tick.
        self.frac20 += self.lv.diffframe240f / 4800.0;
        if self.frac20 > 1.0 {
            self.frac20 -= 1.0;
        }
        self.tick_casings();
        self.fx.sparks.tick(&self.lv);
        self.props_tick();

        // lv_tick_player: each player's player_tick.
        let idle = PlayerInput::default();
        for i in 0..self.players.len() {
            let cyls = self.perims_except(i);
            let env = WalkEnv { level: &self.level, cyls: &cyls };
            let input = inputs.get(i).unwrap_or(&idle);
            self.players[i].tick(input, &self.lv, &env, &self.res, &mut self.rng, &mut self.events);
            if self.players[i].die_request {
                // SUBST: PD kills the player (fell for 4 s or out of the world)
                // and runs the death sequence before the respawn / no deaths
                // until M6-M7, so the player respawns at once.
                let others: Vec<usize> = (0..self.players.len()).filter(|&j| j != i).collect();
                self.spawn_player(i, &others);
            }
        }

        // player_update_shake (`player.c:3012`), then the retraces this frame spans.
        if let Some(p) = self.players.first() {
            let intensity = self.explosions.update_shake(p.pos);
            self.vi.shake(intensity);
        }
        for _ in 0..self.lv.lvupdate60.max(1) {
            self.vi.retrace();
        }

        // lv_render, per player.
        self.lights.tick(&self.lv);
        for i in 0..self.players.len() {
            self.hands_tick_attack(i);
            // lookingatprop (`lv.c:1200`). The boards stand for the training
            // targets PD lets the sight react to (`MODEL_TARGET` in CI training).
            let aimtrack = self.res.gset.has_flag(self.players[i].gun.bgun_get_weapon_num(HAND_RIGHT), WEAPONFLAG_AIMTRACK) && self.players[i].insightaimmode;
            self.lookingatprop[i] = if self.players.len() == 1 || aimtrack { self.prop_find_aiming_at(i, HAND_RIGHT, false, false) } else { None };
            {
                let res = self.res.clone();
                let p = &mut self.players[i];
                let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
                g.bgun_tick_gameplay2();
                g.bgun_tick_hud();
            }
            self.process_gun_events(i);
        }
        for b in self.boards.iter_mut() {
            b.flash = (b.flash - self.lv.lvupdate60freal / 20.0).max(0.0);
        }
    }

    /// `casings_tick` (`casingtick.c:81`): each lands with `SFXMAP_8051` at
    /// 0.98..1.23, at most one sound per 20 quarter-ticks.
    fn tick_casings(&mut self) {
        let lv = self.lv.clone();
        if self.fx.casing_cooldown240 > 0 {
            self.fx.casing_cooldown240 = (self.fx.casing_cooldown240 - lv.lvupdate240).max(0);
        }
        let mut landed = Vec::new();
        self.fx.casings.retain_mut(|c| {
            if c.tick(&lv) {
                landed.push(c.pos);
                false
            } else {
                true
            }
        });
        for pos in landed {
            if self.fx.casing_cooldown240 == 0 && lv.lvupdate240 > 0 {
                self.fx.casing_cooldown240 = 20;
                let speed = self.rng.randomfrac() * 0.25 + 0.98;
                self.sound_at(0x8051, speed, pos, propsnd::DEFAULT_DISTS);
            }
        }
    }

    /// `props_tick` for what M4 has: the players' tracers
    /// (`player_tick_beams`), the explosions, the smoke.
    fn props_tick(&mut self) {
        for p in self.players.iter_mut() {
            for h in 0..2 {
                p.gun.hands[h].beam.tick(&mut self.rng, &self.lv);
            }
        }
        let victims: Vec<Victim> = self
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let (r, ymax, ymin) = p.player_get_bbox();
                Victim { id: VictimId::Player(i), pos: p.pos, chrbox: Some((r, ymax, ymin)) }
            })
            .chain(self.boards.iter().enumerate().map(|(i, b)| Victim { id: VictimId::Board(i), pos: (b.min + b.max) * 0.5, chrbox: None }))
            .collect();
        let mut out = ExpOut::default();
        // The scorches are coloured by the room at the camera, like the gun.
        let brightness = self.lights.brightness(self.players.first().and_then(|p| p.floorroom));
        self.explosions.tick(&mut self.rng, &self.lv, &mut self.fx.smokes, &victims, brightness, &|_| None, &mut out);
        self.apply_explosion_out(out);
        // smoke_tick reads g_Vars.currentplayer's muzzles: during lv_tick that is
        // the player the last frame's render loop ended on, the last one.
        let muzzles = self.players.last().map_or([Vec3::ZERO; 2], |p| [p.gun.hands[0].muzzlepos, p.gun.hands[1].muzzlepos]);
        self.fx.smokes.tick(&mut self.rng, &self.lv, muzzles, &|_| None);
    }

    /// Apply an explosion tick's (or creation's) side effects.
    pub(crate) fn apply_explosion_out(&mut self, out: ExpOut) {
        for (id, pos) in out.sounds {
            if id != 0 {
                self.sound_at(id, 1.0, pos, propsnd::DEFAULT_DISTS);
            }
        }
        for wh in out.wallhits {
            self.fx.push_wallhit(wh);
        }
        for (victim, dmg, _dir, _first, _owner) in out.damage {
            match victim {
                // M6: chr_damage_by_explosion.
                VictimId::Player(i) => self.player_damage[i] += dmg,
                VictimId::Board(i) => {
                    if let Some(b) = self.boards.get_mut(i) {
                        b.damage += dmg;
                        if dmg > 0.0 {
                            b.flash = 1.0;
                        }
                    }
                }
                VictimId::Prop(_) => {}
            }
        }
        for (pos, start) in out.flashes {
            if let Some(room) = self.level.floor_room(pos, 1.0) {
                self.lights.room(room).flash(start, 255);
            }
        }
    }

    /// `explosion_create_simple` from the gun code (`owner` a player), with its
    /// room flash applied.
    pub fn explosion_create_simple(&mut self, owner: usize, pos: Vec3, ty: usize) -> bool {
        let cam = crate::props::explosions::ExpCam { pos: self.players[owner].cam.pos(), lodscalez: self.players[owner].cam.c_lodscalez };
        let mut out = ExpOut::default();
        let level = self.level.clone();
        let ok = self.explosions.create_simple(&mut self.rng, &mut self.fx.smokes, &StageExp { level: &level }, None, pos, ty, owner as i32, cam, &mut out);
        self.apply_explosion_out(out);
        ok
    }

    /// What happened since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}
