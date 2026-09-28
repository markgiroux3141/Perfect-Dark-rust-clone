//! `World`: one `Lv`, one `Rng`, the stage, the human players, every chr
//! (the players' and the simulants', in PD's `g_MpAllChrPtrs` order), the
//! navigation graph, the effects, and the event queue.
//! [`World::step`] runs one PD frame in PD's order for all players at once; it is
//! sized for up to four humans, and split-screen is a renderer concern.
//!
//! The frame, as `main.c:1043` runs it:
//! 1. `lv_tick`: the timing (a Combat Boost caps it), `bgun_tick_boost`,
//!    `casings_tick`, `sparks_tick`, `nbombs_tick`, `lv_update_misc_sfx`, and
//!    `props_tick` (each player's tracers, the sentries' tracers, the explosions,
//!    the smoke);
//! 2. each player's `player_tick`: `bmove_tick` (the controls, the hands' state
//!    machines in `bgun_tick_gameplay`, the walk) and the camera, or riding a
//!    Slayer rocket; the player's chr follows its player;
//! 3. `lv_render`, per player: the x-ray's eraser (`bg_tick`), `lights_tick`,
//!    `props_tick_player` (the objects: projectiles in flight, fuses, mines, the
//!    sentries; the simulants, `bot_tick`; the player's cloak), after the last
//!    player `alarm_tick`'s
//!    proximity triggers, then `hands_tick_attack` (the shots, which set
//!    `hitpos`; throws and launches), `player_render_hud` → `bgun_tick_gameplay2`
//!    (the vision mode, the gun's pose, the RC-P120's cloak drain) and the HUD's
//!    timers and the death sequence, the framebuffer effects (`lv_render`'s
//!    `bview_*`), and a new life for a player who asked (`lv.c:1652`).
//!
//! Pickups (M8) and the match rules (M7) slot in where PD ticks them.
//!
//! Source: `pd_complex/fight.rs` `Fight::frame` and `pd_guns/sim.rs` `Sim::frame`,
//! which glued the two spike sims.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use pd_core::anim::AnimBank;
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::ids::{CAMERAMODE_DEFAULT, CAMERAMODE_THIRDPERSON, HAND_RIGHT, VISIONMODE_SLAYERROCKET, VISIONMODE_SLAYERROCKETSTATIC, WEAPONFLAG_AIMTRACK};
use pd_core::lv::{Lv, LvTickIn};
use pd_core::model::{Bodies, ModelStore};
use pd_core::mp::MatchSetup;
use pd_core::rng::Rng;

use crate::chr::{Chr, GruntNext, NavStats};
use crate::fx::Fx;
use crate::gun::boost::SpeedPill;
use crate::gun::shot::AimedAt;
use crate::gun::Gset;
use crate::nav::NavGraph;
use crate::player::{player_choose_spawn_location, Player, PlayerInput, SpawnOther, WalkEnv};
use crate::props::explosions::{ExpOut, ExpWorld, Explosions, Victim, VictimId};
use crate::props::Props;
use crate::propsnd::{self, AudioConfigs, Listener};
use crate::stage::{PerimCyl, Stage, TileLevel};

/// What every world loads once and shares: the animation bank, the weapon
/// table, the model files, the bodies and heads, and the sounds' audio configs.
pub struct WorldRes {
    pub bank: Arc<AnimBank>,
    pub gset: Arc<Gset>,
    pub models: Arc<ModelStore>,
    pub bodies: Arc<Bodies>,
    pub audio: Arc<AudioConfigs>,
}

impl WorldRes {
    pub fn load(assets: &AssetDir) -> Result<WorldRes, String> {
        Ok(WorldRes {
            bank: Arc::new(AnimBank::load(assets)?),
            gset: Arc::new(Gset::load(assets)?),
            models: Arc::new(ModelStore::load(assets)?),
            bodies: Arc::new(Bodies::load(assets)?),
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
    /// Every chr (`g_MpAllChrPtrs`): the players' first (chr `i` is player
    /// `i`'s), then the simulants in allocation order.
    pub chrs: Vec<Chr>,
    /// The graph the simulants route on (PD's own, unless a harness swaps it).
    pub nav: Arc<NavGraph>,
    /// Go-to bookkeeping for the A/B harness; `record_gotos` logs each request.
    pub navstats: NavStats,
    pub record_gotos: bool,
    /// The weapons the match hands out (`g_MpSetup.weapons` resolved to
    /// `WEAPON_*`, by `pd_game` from the menus' table).
    pub weaponset: Vec<u8>,
    /// Each simulant's loadout by setup slot (see [`World::bot_give_loadout`]).
    pub bot_loadout: Vec<Option<(u8, bool)>>,
    /// `chr_grunt`'s round-robin indexes.
    pub grunt_next: GruntNext,
    /// `bot_spawn_all` has run (the simulants' AI list, on the first frame).
    pub bots_spawned: bool,
    /// Not PD: `false` stops the simulants thinking (`bot_tick_unpaused`), for
    /// tests that stand one somewhere.
    pub bot_brains: bool,
    /// How many times each player has (re)spawned.
    pub spawns: Vec<u32>,
    pub fx: Fx,
    pub explosions: Explosions,
    /// The guns' objects in the world, the N-Bomb storms, the detonators.
    pub props: Props,
    /// The target boards (the firing range's); none on an arena.
    pub boards: Vec<Board>,
    pub lights: Lights,
    pub vi: ViShake,
    /// The Combat Boost, `g_Vars.speedpill*`: one for the whole world.
    pub speedpill: SpeedPill,
    /// `g_MiscSfxActiveTypes`: which misc loops play, by `MISCSFX_*`.
    pub misc_sfx: [bool; 3],
    /// Shots each player has fired (`mpstats_increment_player_shotcount`).
    pub shots_fired: Vec<u32>,
    /// `player->lookingatprop.prop`: the chr or board under each player's
    /// crosshair (the sight turns red on it).
    pub lookingatprop: Vec<Option<AimedAt>>,
    /// `g_20SecIntervalFrac`: the HUD's wave shimmer.
    pub frac20: f32,
    events: Vec<Event>,
}

/// The stage as the explosions see it.
pub(crate) struct StageExp<'a> {
    pub level: &'a TileLevel,
}

impl ExpWorld for StageExp<'_> {
    /// The floor's room's bbox, from its tiles.
    fn room_bbox(&self, pos: Vec3) -> (Vec3, Vec3) {
        if let Some(bb) = self.level.floor_room(pos, 1.0).and_then(|room| self.level.room_bbox(room)) {
            return bb;
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
    /// Start a match: the simulants are allocated (`setup.c:1961`), every
    /// player spawns in turn, each choosing a pad away from those already
    /// placed (`player_choose_spawn_location`), with the loadout (see
    /// [`World::give_loadout`]); the simulants spawn on the first frame
    /// (`bot_spawn_all`). The weapon set is every weapon until
    /// [`World::set_weapon_set`].
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
        let mut chrs = Vec::with_capacity(n + setup.simulants.len());
        for (i, p) in setup.players.iter().enumerate() {
            chrs.push(crate::chr::player_chr(&res.models, &res.bodies, i, &p.chr)?);
        }
        let nchrs = n + setup.simulants.len();
        for (_, mut c) in crate::chr::botmgr_allocate_bots(&res.models, &res.bodies, &setup, nchrs, &mut rng)? {
            c.chrnum = chrs.len() - n;
            chrs.push(c);
        }
        let nav = Arc::new(NavGraph::from_stage(&stage, &level));
        let weaponset = res.gset.order.clone();
        let mut w = World {
            setup,
            lv: Lv::new(),
            rng,
            stage,
            level,
            res,
            players,
            chrs,
            nav,
            navstats: NavStats::default(),
            record_gotos: false,
            weaponset,
            bot_loadout: Vec::new(),
            grunt_next: GruntNext::default(),
            bots_spawned: false,
            bot_brains: true,
            spawns: vec![0; n],
            fx: Fx::default(),
            explosions: Explosions::default(),
            props: Props::default(),
            boards: Vec::new(),
            lights: Lights::default(),
            vi: ViShake::default(),
            speedpill: SpeedPill::default(),
            misc_sfx: [false; 3],
            shots_fired: vec![0; n],
            lookingatprop: vec![None; n],
            frac20: 0.0,
            events: Vec::new(),
        };
        w.bot_loadout = w.bot_loadouts();
        for i in 0..n {
            let before: Vec<usize> = (0..i).collect();
            w.spawn_player(i, &before);
            w.give_loadout(i);
        }
        Ok(w)
    }

    /// The match's weapon set (`WEAPON_*`, the menu's six slots resolved;
    /// `WEAPON_NONE` and repeats are fine): the players' loadouts are given
    /// again and the simulants' chosen from it.
    pub fn set_weapon_set(&mut self, weapons: &[u8]) {
        let gset = self.res.gset.clone();
        let mut set: Vec<u8> = Vec::new();
        for &w in weapons {
            if gset.weapon(w).is_some() && w != pd_core::ids::WEAPON_NONE && !set.contains(&w) {
                set.push(w);
            }
        }
        self.weaponset = set;
        self.bot_loadout = self.bot_loadouts();
        for i in 0..self.players.len() {
            self.players[i].gun.p.inventory.clear();
            self.give_loadout(i);
        }
        if self.bots_spawned {
            for i in self.players.len()..self.chrs.len() {
                self.chrs[i].held = [None, None];
                self.bot_give_loadout(i);
            }
        }
    }

    /// `// SUBST:` a simulant picks its weapons up from the arena's pads and
    /// uses the one it rates highest (`botinv`, M8) / the weapons of the set a
    /// simulant can fire in M6 (`WEAPONFLAG_AICANUSE`, a third-person model,
    /// a hitscan primary: no launchers or throwables until M8) are dealt out
    /// in turn by setup slot, held twice where the weapon dual-wields and
    /// `g_AibotWeaponPreferences` rates two above one (`dualscore1 > score1`).
    /// With none in the set, the simulants punch.
    fn bot_loadouts(&self) -> Vec<Option<(u8, bool)>> {
        let gset = &self.res.gset;
        let usable: Vec<(u8, bool)> = self
            .weaponset
            .iter()
            .filter_map(|&w| {
                let def = gset.weapon(w)?;
                let pref = def.bot?;
                def.tp_model.as_ref()?;
                let f = gset.func(w, pd_core::ids::FUNC_PRIMARY)?;
                let hitscan = f.ftype == pd_core::ids::INVENTORYFUNCTYPE_SHOOT_SINGLE || f.ftype == pd_core::ids::INVENTORYFUNCTYPE_SHOOT_AUTOMATIC;
                (gset.has_flag(w, pd_core::ids::WEAPONFLAG_AICANUSE) && hitscan).then(|| (w, gset.has_flag(w, pd_core::ids::WEAPONFLAG_DUALWIELD) && pref.dualscore1 > pref.score1))
            })
            .collect();
        (0..self.setup.simulants.len()).map(|k| (!usable.is_empty()).then(|| usable[k % usable.len()])).collect()
    }

    /// `// SUBST:` a Combat Simulator player starts unarmed and picks weapons
    /// up from the arena's pads (M8) / until then every player carries every
    /// weapon in the set, twice where it dual-wields, with unlimited ammo, and
    /// starts with the first (the Falcon 2 if the set has it).
    pub fn give_loadout(&mut self, i: usize) {
        let gset = self.res.gset.clone();
        let set = self.weaponset.clone();
        let gun = &mut self.players[i].gun;
        for &wn in &set {
            let dual = gset.has_flag(wn, pd_core::ids::WEAPONFLAG_DUALWIELD);
            gun.give_weapon(&gset, wn, dual);
        }
        gun.p.unlimited_ammo = true;
        let first = if set.contains(&pd_core::ids::WEAPON_FALCON2) { pd_core::ids::WEAPON_FALCON2 } else { set.first().copied().unwrap_or(pd_core::ids::WEAPON_UNARMED) };
        gun.bgun_equip_weapon(first);
    }

    /// The perimeters of every chr but player `except`'s.
    fn perims_except(&self, except: usize) -> Vec<PerimCyl> {
        self.chr_perims_except(except)
    }

    /// Spawn player `i`, judging the pads against the chrs in `others` (at the
    /// match start, the players already spawned; later, every other chr).
    fn spawn_player(&mut self, i: usize, others: &[usize]) {
        let judged: Vec<SpawnOther> = others.iter().map(|&j| SpawnOther { pos: self.chrs[j].pos, rooms: self.chrs[j].rooms.clone() }).collect();
        let cyls: Vec<PerimCyl> = others.iter().filter_map(|&j| self.chrs[j].perim()).collect();
        let (pos, angle) = player_choose_spawn_location(&self.level, &self.stage, 30.0, &judged, &cyls, &mut self.rng);
        self.players[i].start_new_life(&self.level, pos, angle);
        self.spawns[i] += 1;
        self.sync_player_chr(i);
    }

    /// Player `i`'s chr (`prop->chr`) kept in step with its player: where it
    /// is, its perimeter, its facing and eye, its cloak and fade.
    pub(crate) fn sync_player_chr(&mut self, i: usize) {
        let p = &self.players[i];
        let perim = p.perim();
        let c = &mut self.chrs[i];
        c.pos = p.pos;
        c.manground = p.manground;
        c.ground = p.ground;
        c.radius = perim.radius;
        c.height = perim.ymax - perim.ymin;
        // chr_get_theta: BADDTOR2(360 - vv_theta).
        c.playertheta = pd_core::math::wrap_pos(pd_core::math::baddtor2(360.0 - p.theta));
        c.eyeheight = p.eyeheight;
        c.cloaked = p.cloak.cloaked;
        c.floorroom = p.floorroom;
        c.floortype = p.floortype;
        c.rooms = p.floorroom.into_iter().collect();
        c.fadealpha = p.chrfadefrac * 255.0;
    }

    /// `player_die` (`player.c:4793`): killed by the chr that last shot it
    /// (M7: `lastshooter`), else by its own hand.
    pub(crate) fn player_die(&mut self, pi: usize) {
        self.player_die_by_shooter(pi, Some(pi));
    }

    /// `player_die_by_shooter` (`player.c:4807`): the death scored, the cloak
    /// off, the guns thrown (`bgun_handle_player_dead`). `// M8:`
    /// `current_player_drop_all_items`; `// M7:` the hud messages and the
    /// shortest-life stat.
    pub(crate) fn player_die_by_shooter(&mut self, pi: usize, shooter: Option<usize>) {
        if self.players[pi].isdead {
            return;
        }
        self.mpstats_record_death(shooter, pi);
        self.chr_uncloak_chr(pi, true);
        self.players[pi].player_set_dead();
        let res = self.res.clone();
        let p = &mut self.players[pi];
        let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
        g.bgun_handle_player_dead();
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
        // lv_tick: a boost caps the frame. M7: smart slow motion's "an enemy is
        // on screen" (`bg_room_is_on_player_screen`).
        let tickin = LvTickIn { speedpillon: self.speedpill.on, slowmo: self.setup.slowmotion(), ..LvTickIn::default() };
        self.lv.frame(diffframe240, tickin);
        self.bgun_tick_boost();
        // menu_tick_timers (`game_006900.c:42`), from lv_tick's menu_tick.
        self.frac20 += self.lv.diffframe240f / 4800.0;
        if self.frac20 > 1.0 {
            self.frac20 -= 1.0;
        }
        self.tick_casings();
        self.fx.sparks.tick(&self.lv);
        if self.props.nbombs.active {
            self.nbombs_tick();
        }
        self.lv_update_misc_sfx();
        self.fx.boltbeams.tick(self.lv.lvupdate60freal);
        self.props_tick();

        // lv_tick_player: each player's player_tick.
        let idle = PlayerInput::default();
        for i in 0..self.players.len() {
            let cyls = self.perims_except(i);
            let env = WalkEnv { level: &self.level, cyls: &cyls };
            let input = inputs.get(i).unwrap_or(&idle);
            // player.c:3302: a rocket that is gone loses its signal.
            if self.players[i].visionmode == VISIONMODE_SLAYERROCKET && self.players[i].slayerrocket.is_none() {
                self.players[i].visionmode = VISIONMODE_SLAYERROCKETSTATIC;
            }
            if self.players[i].visionmode == VISIONMODE_SLAYERROCKET {
                // bmove_tick(0, 0, 0, 1): Jo stands still while the rocket flies.
                self.players[i].tick(&idle, &self.lv, &env, &self.res, &mut self.rng, &mut self.events);
                self.player_tick_slayer(i, input);
            } else {
                self.players[i].tick(input, &self.lv, &env, &self.res, &mut self.rng, &mut self.events);
                self.players[i].cameramode = CAMERAMODE_DEFAULT;
            }
            if self.players[i].die_request {
                // player_die(true): fell for 4 s, or out of the world.
                self.player_die(i);
            }
            self.sync_player_chr(i);
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
        let n = self.players.len();
        for i in 0..n {
            let motion_blur = self.lv_render_blur(i);
            // SUBST: bgun_render draws a fired rocket at the muzzle once more and
            // then lets go of it (`bondgun.c:8334`, and at once in x-ray) / the
            // renderer can't write the world, so the hand lets go here, a frame on.
            for hand in self.players[i].gun.hands.iter_mut() {
                if hand.firedrocket {
                    hand.rocket = None;
                }
            }
            self.bg_tick_eraser(i);
            self.props_tick_player(i);
            if i == 0 {
                self.chrs_tick();
            }
            if i == 0 {
                // SUBST: PD clears g_PlayersDetonatingMines in alarm_tick after the
                // last player's props, which with two players loses the first
                // player's press for the mines only its pass ticks / cleared
                // once the first pass has ticked the mines (the same with one).
                self.props.detonating = 0;
            }
            // player_tick_third_person: the player's chr (M6: its body).
            self.chr_update_cloak(i);
            if i + 1 == n {
                // alarm_tick (`propobj.c:20051`).
                self.chrs_trigger_proxies();
            }
            self.hands_tick_attack(i);
            // lookingatprop (`lv.c:1200`). The boards stand for the training
            // targets PD lets the sight react to (`MODEL_TARGET` in CI training).
            let aimtrack = self.res.gset.has_flag(self.players[i].gun.bgun_get_weapon_num(HAND_RIGHT), WEAPONFLAG_AIMTRACK) && self.players[i].insightaimmode;
            self.lookingatprop[i] = if self.players.len() == 1 || aimtrack { self.prop_find_aiming_at(i, HAND_RIGHT, false, false) } else { None };
            // player_render_hud (`player.c`): in the third person (riding a
            // Slayer rocket) no gun, no HUD, and bgun_tick_gameplay2 doesn't run.
            if self.players[i].cameramode != CAMERAMODE_THIRDPERSON {
                self.bgun_tick_vision(i);
                {
                    let res = self.res.clone();
                    let p = &mut self.players[i];
                    let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
                    g.bgun_tick_gameplay2();
                }
                // bgun_tick_gameplay2's RC-P120 cloak (`bondgun.c:8050`). PD drains
                // it before the hands are posed; the pose doesn't read the clip.
                self.rcp120_cloak_tick(i);
                {
                    let res = self.res.clone();
                    let p = &mut self.players[i];
                    let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
                    g.bgun_tick_hud();
                }
            }
            self.process_gun_events(i);
            // player_render_hud's death sequence (`player.c:4546`).
            let input = inputs.get(i).unwrap_or(&idle);
            self.players[i].player_tick_death(input);
            self.lv_render_fx(i, motion_blur);
            if self.players[i].dostartnewlife {
                // player_start_new_life (`lv.c:1652`).
                let others: Vec<usize> = (0..self.chrs.len()).filter(|&j| j != i).collect();
                self.spawn_player(i, &others);
                self.players[i].gun.p.inventory.clear();
                self.give_loadout(i);
            }
        }
        if n == 0 {
            // A headless match (the harness): no player pass to tick them in.
            self.chrs_tick();
        }
        for b in self.boards.iter_mut() {
            b.flash = (b.flash - self.lv.lvupdate60freal / 20.0).max(0.0);
        }
    }

    /// The simulants' part of the first player's `props_tick_player`: on the
    /// first frame `bot_spawn_all` (their AI list's first command), then each
    /// one's `bot_tick`, newest prop first.
    ///
    /// `// SUBST:` PD ticks a chr again on each later player's pass (the
    /// pose in that player's eye space) / once: the pose is world-space and
    /// its on-screen tests cover every player.
    fn chrs_tick(&mut self) {
        let first = self.players.len();
        if !self.bots_spawned {
            self.bots_spawned = true;
            for b in first..self.chrs.len() {
                self.bot_spawn(b, false);
            }
        }
        for b in (first..self.chrs.len()).rev() {
            self.bot_tick(b, true);
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

    /// `props_tick` (`proptick.c:48`): the players' tracers (`player_tick_beams`),
    /// the sentries' (`obj_tick`), the explosions, the smoke.
    fn props_tick(&mut self) {
        for p in self.players.iter_mut() {
            for h in 0..2 {
                p.gun.hands[h].beam.tick(&mut self.rng, &self.lv);
            }
        }
        // chr_tick_beams (`chr.c:2343`), newest prop first.
        let lv60 = self.lv.lvupdate60;
        for c in self.chrs.iter_mut().rev().filter(|c| c.player.is_none()) {
            for slot in c.fireslots.iter_mut() {
                slot.beam.tick(&mut self.rng, &self.lv);
            }
            if let Some(a) = c.aibot.as_mut() {
                if a.fadeintimer60 > 0 {
                    a.fadeintimer60 = if a.fadeintimer60 > lv60 { a.fadeintimer60 - lv60 } else { 0 };
                }
            }
        }
        for o in self.props.objs.iter_mut() {
            if let Some(a) = o.autogun.as_mut() {
                a.beam.tick(&mut self.rng, &self.lv);
            }
        }
        let victims: Vec<Victim> = self
            .chrs
            .iter()
            .enumerate()
            .filter(|(_, c)| c.actiontype != crate::chr::Act::Dead)
            .map(|(i, c)| {
                // prop_get_bbox: a player's player_get_bbox, a chr's chr_get_bbox.
                let chrbox = match c.player {
                    Some(p) => self.players[p].player_get_bbox(),
                    None => (c.radius, c.manground + c.height, c.manground + 20.0),
                };
                Victim { id: VictimId::Chr(i), pos: c.pos, chrbox: Some(chrbox) }
            })
            .chain(self.boards.iter().enumerate().map(|(i, b)| Victim { id: VictimId::Board(i), pos: (b.min + b.max) * 0.5, chrbox: None }))
            // The guns' objects: a blast sets off the explosives it reaches.
            .chain(self.props.objs.iter().filter(|o| !o.is_deleting() && o.flags & pd_core::ids::OBJFLAG_HELDROCKET == 0).map(|o| Victim { id: VictimId::Prop(o.id), pos: o.pos, chrbox: None }))
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
                VictimId::Chr(i) => {
                    let owner = (_owner >= 0 && (_owner as usize) < self.chrs.len()).then_some(_owner as usize);
                    self.chr_damage_by_explosion(i, dmg, _dir, owner);
                }
                VictimId::Board(i) => {
                    if let Some(b) = self.boards.get_mut(i) {
                        b.damage += dmg;
                        if dmg > 0.0 {
                            b.flash = 1.0;
                        }
                    }
                }
                // obj_damage_by_explosion(prop, damage, pos, WEAPON_REMOTEMINE, owner).
                VictimId::Prop(id) => {
                    if self.obj_damage(id, dmg, pd_core::ids::WEAPON_REMOTEMINE, _owner) {
                        if let Some(pos) = self.props.get(id).map(|o| o.pos) {
                            self.autogun_destroyed(pos, _owner, 0);
                        }
                    }
                }
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

    /// `explosion_create_simple` owned by chr `i` (a simulant's Phoenix), seen
    /// from the current player's (the first's) camera.
    pub(crate) fn explosion_create_by_chr(&mut self, i: usize, pos: Vec3, ty: usize) -> bool {
        let Some(p) = self.players.first() else { return false };
        let cam = crate::props::explosions::ExpCam { pos: p.cam.pos(), lodscalez: p.cam.c_lodscalez };
        let mut out = ExpOut::default();
        let level = self.level.clone();
        let ok = self.explosions.create_simple(&mut self.rng, &mut self.fx.smokes, &StageExp { level: &level }, None, pos, ty, i as i32, cam, &mut out);
        self.apply_explosion_out(out);
        ok
    }

    /// What happened since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}
