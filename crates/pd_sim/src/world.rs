//! `World`: one `Lv`, one `Rng`, the stage, the human players, every chr
//! (the players' and the simulants', in PD's `g_MpAllChrPtrs` order), the
//! navigation graph, the effects, and the event queue.
//! [`World::step`] runs one PD frame in PD's order for all players at once; it is
//! sized for up to four humans, and split-screen is a renderer concern.
//!
//! The frame, as `main.c:1043` runs it:
//! 1. `lv_tick`: the timing (a Combat Boost caps it), `bgun_tick_boost`,
//!    `casings_tick`, `sparks_tick`, `nbombs_tick`, `lv_update_misc_sfx`,
//!    `lighting_tick`, and
//!    `props_tick` (each player's tracers, the sentries' tracers, the explosions,
//!    the smoke);
//! 2. each player's `player_tick`: `bmove_tick` (the controls, the hands' state
//!    machines in `bgun_tick_gameplay`, the walk) and the camera, or riding a
//!    Slayer rocket; the player's chr follows its player;
//! 3. `lv_render`, per player: `bg_tick` (the x-ray's eraser, the rooms on
//!    screen through the portals), `lights_tick`,
//!    `props_tick_player` (the objects: projectiles in flight, fuses, mines, the
//!    sentries; the simulants, `bot_tick`; the player's cloak), after the last
//!    player `alarm_tick`'s
//!    proximity triggers, then `hands_tick_attack` (the shots, which set
//!    `hitpos`; throws and launches), `player_render_hud` → `bgun_tick_gameplay2`
//!    (the vision mode, the gun's pose, the RC-P120's cloak drain) and the HUD's
//!    timers and the death sequence, the framebuffer effects (`lv_render`'s
//!    `bview_*`), and a new life for a player who asked (`lv.c:1652`).
//!
//! The pickups respawn in `props_tick` (`obj_tick`) and are collected in each
//! player's `lv_render` pass (`props_test_for_pickup`, `lv.c:1304`) and in each
//! simulant's `bot_tick` (`bot_check_pickups`).
//!
//! Source: `pd_complex/fight.rs` `Fight::frame` and `pd_guns/sim.rs` `Sim::frame`,
//! which glued the two spike sims.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use pd_core::anim::AnimBank;
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::ids::{CAMERAMODE_DEFAULT, CAMERAMODE_THIRDPERSON, HAND_RIGHT, VISIONMODE_SLAYERROCKET, VISIONMODE_SLAYERROCKETSTATIC, WEAPONFLAG_AIMTRACK};
use pd_core::lang::Lang;
use pd_core::lv::{Lv, LvTickIn};
use pd_core::text::Fonts;
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
use crate::stage::{PropGeo, Stage, TileLevel};

/// What every world loads once and shares: the animation bank, the weapon
/// table, the model files, the bodies and heads, and the sounds' audio configs.
pub struct WorldRes {
    pub bank: Arc<AnimBank>,
    pub gset: Arc<Gset>,
    pub models: Arc<ModelStore>,
    pub bodies: Arc<Bodies>,
    pub audio: Arc<AudioConfigs>,
    /// The English text (the HUD messages).
    pub lang: Arc<Lang>,
    /// The fonts, for the HUD messages' measures.
    pub fonts: Arc<Fonts>,
}

impl WorldRes {
    pub fn load(assets: &AssetDir) -> Result<WorldRes, String> {
        Ok(WorldRes {
            bank: Arc::new(AnimBank::load(assets)?),
            gset: Arc::new(Gset::load(assets)?),
            models: Arc::new(ModelStore::load(assets)?),
            bodies: Arc::new(Bodies::load(assets)?),
            audio: Arc::new(AudioConfigs::load(assets)?),
            lang: Arc::new(Lang::load(assets, "en")?),
            fonts: Arc::new(Fonts::load(assets)?),
        })
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

/// A target board: the firing range's props, a thin box that counts its hits
/// (standing in for the training range's `MODEL_TARGET`s).
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
    /// Not PD: the test harness's loadout for the players (the gun tests): every
    /// weapon listed, with unlimited ammo, on every life. `None` in a match.
    pub harness_loadout: Option<Vec<u8>>,
    /// Not PD: the harness's loadout for the simulants by setup slot (the A/B
    /// probes; see [`World::bot_give_loadout`]). Empty in a match.
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
    /// Room lighting (`dlights.c`).
    pub lights: crate::lights::Lights,
    pub vi: ViShake,
    /// The Combat Boost, `g_Vars.speedpill*`: one for the whole world.
    pub speedpill: SpeedPill,
    /// `g_MiscSfxActiveTypes`: which misc loops play, by `MISCSFX_*`.
    pub misc_sfx: [bool; 3],
    /// The match: pause, limits, counters, statistics, HUD messages.
    pub mp: crate::mp::MpMatch,
    /// `player->lookingatprop.prop`: the chr or board under each player's
    /// crosshair (the sight turns red on it).
    pub lookingatprop: Vec<Option<AimedAt>>,
    /// `g_20SecIntervalFrac`: the HUD's wave shimmer.
    pub frac20: f32,
    /// `g_Lv80SecIntervalFrac` (`game_006900.c:25`): 0 to 1 over 80 s of match
    /// time, which the animated textures run on.
    pub frac80: f32,
    /// `g_SkyCloudOffset` (`sky.c:43`): how far the clouds have drifted (in
    /// their texture's `t`), and `g_SkyWindSpeed`, per tick.
    pub sky_cloud_offset: f32,
    pub sky_wind_speed: f32,
    /// `g_BgPortals[].flags`: closed by doors and glass, forced open when those
    /// are gone.
    pub portalflags: crate::stage::rooms::PortalFlags,
    /// `g_MpRoomVisibility`: per room, bit `i` on player `i`'s screen, bit
    /// `4 + i` on its standby (`bg_choose_rooms_to_load`).
    pub mp_room_visibility: Vec<u8>,
    /// `g_Rooms[].flags`' ONSCREEN/STANDBY bits as the latest `bg_tick` left
    /// them (the last player's pass, until the next pass redoes them).
    pub roomflags: Vec<u16>,
    /// `var80084010` (`lv.c:2425`): the pause the rumble last saw.
    pub(crate) rumble_paused: bool,
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
    /// Start a match, in `lv_reset`'s order (`lv.c:345`): the simulants are
    /// allocated (`setup.c:1961`); the scenario's stage state is read from the
    /// setup (`scenario_reset`); the setup's props are placed, the pickups from
    /// the weapon set, and then the scenario's (`setup_create_props`,
    /// `scenario_init_props`); every player spawns in turn, each choosing a pad
    /// away from those already placed (`player_choose_spawn_location`),
    /// unarmed; the simulants learn whether their team has a human
    /// (`mp_calculate_team_is_only_ai`) and spawn on the first frame
    /// (`bot_spawn_all`).
    pub fn new(mut setup: MatchSetup, stage: Arc<Stage>, level: Arc<TileLevel>, res: Arc<WorldRes>, seed: u64) -> Result<World, String> {
        if stage.spawn_pads.is_empty() {
            return Err(format!("stage {} has no spawn pads", stage.code));
        }
        crate::mp::scenario::scenario_init_setup(&mut setup);
        let mut rng = Rng::new(seed);
        let n = setup.players.len();
        let mut players = Vec::with_capacity(n);
        for i in 0..n {
            let mut p = Player::new(&res, Vec3::ZERO, 0.0, n, &mut rng)?;
            p.set_viewport(i, n, setup.screensplit);
            players.push(p);
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
        // Each chr's slot (mp_chrindex_to_chrslot): the players', then the
        // simulants' by their setup row.
        for (i, p) in setup.players.iter().enumerate() {
            chrs[i].mpslot = p.slot as usize;
        }
        for c in chrs.iter_mut().skip(n) {
            if let Some(s) = c.aibot.as_ref().and_then(|a| setup.simulants.get(a.aibotnum)) {
                c.mpslot = s.slot as usize;
            }
        }
        let stage_portalflags = stage.rooms.initial_portal_flags();
        let lights = crate::lights::Lights::new(&stage.rooms);
        let nrooms = stage.rooms.roomcount();
        let mut w = World {
            mp: crate::mp::MpMatch::new(&setup),
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
            harness_loadout: None,
            bot_loadout: Vec::new(),
            grunt_next: GruntNext::default(),
            bots_spawned: false,
            bot_brains: true,
            spawns: vec![0; n],
            fx: Fx { shards: crate::fx::shards::Shards::new(n), ..Fx::default() },
            explosions: Explosions::default(),
            props: Props::default(),
            boards: Vec::new(),
            lights,
            vi: ViShake::default(),
            speedpill: SpeedPill::default(),
            misc_sfx: [false; 3],
            lookingatprop: vec![None; n],
            frac20: 0.0,
            frac80: 0.0,
            sky_cloud_offset: 0.0,
            sky_wind_speed: 1.0,
            portalflags: stage_portalflags,
            mp_room_visibility: vec![0; nrooms],
            roomflags: vec![0; nrooms],
            rumble_paused: false,
            events: Vec::new(),
        };
        // mp_reset's active menu orders (mplayer.c:286).
        w.am_init_bot_commands();
        w.scenario_reset();
        w.setup_create_props();
        for i in 0..n {
            // lv_reset's per-player resets (lv.c:421): am_reset, ..., the spawn,
            // then with teams on the simulant teammates.
            w.am_reset(i);
            let before: Vec<usize> = (0..i).collect();
            w.spawn_player(i, &before);
            w.player_spawn_inventory(i);
            // The end of the first player_spawn (`player.c:1103`): the body is
            // made (player_tick_chr_body → chr_place → chr_allocate).
            w.chrs[i].cmcount = (w.rng.random() % 300) as u16;
            if w.setup.teams_enabled() {
                w.playermgr_calculate_ai_buddy_nums(i);
            }
        }
        w.mp_calculate_team_is_only_ai();
        Ok(w)
    }

    /// A new life's inventory (`player_start_new_life`, `player.c:590`, and
    /// `player_spawn`, `:934`): nothing but the fists, no ammo, no shield, and
    /// no gun in hand (`bgun_equip_weapon2(g_DefaultWeapons)`: a Combat
    /// Simulator setup's intro gives no weapon, so both are `WEAPON_NONE`).
    pub(crate) fn player_spawn_inventory(&mut self, i: usize) {
        let gun = &mut self.players[i].gun;
        gun.p.inventory.inv_clear();
        gun.p.ammoheldarr = [0; 40];
        gun.p.inventory.inv_give_single_weapon(pd_core::ids::WEAPON_UNARMED);
        gun.ctrl.dualwielding = false;
        gun.bgun_equip_weapon(pd_core::ids::WEAPON_NONE);
        self.player_set_shield_frac(i, 0.0);
        if self.harness_loadout.is_some() {
            self.give_loadout(i);
        }
    }

    /// Not PD: the test harness arms the players with `weapons` (each twice
    /// where it dual-wields) and unlimited ammo, now and on every new life.
    pub fn harness_give_loadout(&mut self, weapons: Vec<u8>) {
        self.harness_loadout = Some(weapons);
        for i in 0..self.players.len() {
            self.give_loadout(i);
        }
    }

    /// Not PD: [`World::harness_give_loadout`]'s weapons for player `i`, the
    /// first (the Falcon 2 if listed) in hand.
    pub fn give_loadout(&mut self, i: usize) {
        let gset = self.res.gset.clone();
        let set = self.harness_loadout.clone().unwrap_or_else(|| gset.order.clone());
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
    pub(crate) fn perims_except(&self, except: usize) -> Vec<PropGeo> {
        self.chr_perims_except(except)
    }

    /// The rest of player `i`'s [`WalkEnv`]: fast movement, its shield, its
    /// menu, and whether it carries a case (`bondwalk.c:1472`: Hold the
    /// Briefcase and Capture the Case slow the carrier).
    pub(crate) fn walk_opts(&self, i: usize) -> (bool, f32, bool, bool) {
        let fastmovement = self.setup.options & pd_core::ids::MPOPTION_FASTMOVEMENT != 0;
        let s = self.setup.scenario;
        let briefcase = self.inv_has_briefcase(i) && (s == pd_core::ids::MPSCENARIO_HOLDTHEBRIEFCASE || s == pd_core::ids::MPSCENARIO_CAPTURETHECASE);
        (fastmovement, self.player_get_shield_frac(i), self.mp.menuopen.get(i).copied().unwrap_or(false), briefcase)
    }

    /// Spawn player `i`, judging the pads against the enemies among the chrs
    /// in `others` (at the match start, the players already spawned; later,
    /// every other chr).
    fn spawn_player(&mut self, i: usize, others: &[usize]) {
        // player_start_new_life's splat_reset_chr (`player.c:518`).
        self.chrs[i].splat = Default::default();
        let judged: Vec<SpawnOther> = others
            .iter()
            .filter(|&&j| self.chr_compare_teams(i, j, crate::mp::Compare::Enemies))
            .map(|&j| SpawnOther { pos: self.chrs[j].pos, rooms: self.chrs[j].rooms.clone(), player: self.chrs[j].player })
            .collect();
        let cyls: Vec<PropGeo> = others.iter().filter_map(|&j| self.chrs[j].perim()).collect();
        let pads = self.scenario_spawn_pads(i);
        let (pos, angle) = player_choose_spawn_location(&self.level, &self.stage, &pads, 30.0, &judged, &cyls, &self.mp_room_visibility, &mut self.rng);
        let floors = self.prop_floors();
        self.players[i].start_new_life(&self.level, &floors, pos, angle);
        self.mp.players[i].killsthislife = 0;
        self.mp.players[i].lifestarttime60 = self.player_get_mission_time(i);
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
        c.sumground = p.manground * 9.999_998;
        c.radius = perim.radius;
        c.height = perim.ymax - perim.ymin;
        // chr_get_theta: BADDTOR2(360 - vv_theta).
        c.playertheta = pd_core::math::wrap_pos(pd_core::math::baddtor2(360.0 - p.theta));
        c.eyeheight = p.eyeheight;
        c.cloak = p.cloak;
        c.floorroom = p.floorroom;
        c.floortype = p.floortype;
        // The player's chr is the player's prop: its rooms.
        c.rooms = if p.rooms.is_empty() { p.floorroom.into_iter().collect() } else { p.rooms.clone() };
        c.fadealpha = p.chrfadefrac * 255.0;
    }

    /// `player_die` (`player.c:4793`): killed by the chr that last shot it,
    /// else by its own hand. PD never sets `lastshooter` (it stays -1 from
    /// `chr_init`), so a fall is always a suicide.
    pub(crate) fn player_die(&mut self, pi: usize) {
        let c = &self.chrs[pi];
        let shooter = if c.lastshooter.is_some() && c.timeshooter > 0 { c.lastshooter } else { Some(pi) };
        self.player_die_by_shooter(pi, shooter);
    }

    /// `player_die_by_shooter` (`player.c:4807`): the player's menu closed,
    /// its HUD messages that need it alive gone, the death scored, the cloak
    /// off, every weapon dropped (`current_player_drop_all_items`), the guns
    /// thrown from the view (`bgun_handle_player_dead`), the shortest life.
    pub(crate) fn player_die_by_shooter(&mut self, pi: usize, shooter: Option<usize>) {
        if self.players[pi].isdead {
            return;
        }
        self.push_event(Event::MpCloseMenus { player: pi as u8 });
        self.hudmsgs_remove_for_dead_player(pi);
        self.mpstats_record_death(shooter.map_or(-1, |s| s as i32), pi as i32);
        self.chr_uncloak_chr(pi, true);
        self.current_player_drop_all_items(pi);
        self.players[pi].player_set_dead();
        let res = self.res.clone();
        let p = &mut self.players[pi];
        let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
        g.bgun_handle_player_dead();
        let now = self.player_get_mission_time(pi);
        let life = now - self.mp.players[pi].lifestarttime60;
        if life < self.mp.playerstats[pi].shortestlife {
            self.mp.playerstats[pi].shortestlife = life;
        }
        self.mp.players[pi].lifestarttime60 = now;
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
        // The main loop's joy_debug_joy → joys_tick_rumble, before the frame.
        for p in self.players.iter_mut() {
            p.rumble.tick();
        }
        // lv_tick: paused, the match's slow motion, a boost's cap.
        let paused = self.mp_is_paused();
        self.lv_tick_rumble_pause(paused);
        let tickin = LvTickIn { paused, speedpillon: self.speedpill.on, slowmo: self.setup.slowmotion(), enemy_on_screen: self.lv_smart_slowmo_enemy_on_screen() };
        self.lv.frame(diffframe240, tickin);
        if paused {
            // Every button but START waits to be released (lv.c:2045).
            for v in self.mp.players.iter_mut() {
                v.joybutinhibit = 0xefff_efff;
            }
        }
        self.bgun_tick_boost();
        self.hudmsgs_tick();
        self.lv_tick_mp();
        // menu_tick_timers (`game_006900.c:42`), from lv_tick's menu_tick.
        self.frac20 += self.lv.diffframe240f / 4800.0;
        if self.frac20 > 1.0 {
            self.frac20 -= 1.0;
        }
        self.frac80 += self.lv.lvupdate60freal / 4800.0;
        if self.frac80 > 1.0 {
            self.frac80 -= 1.0;
        }
        // sky_tick (`skytick.c:8`).
        self.sky_cloud_offset += self.lv.lvupdate60freal * self.sky_wind_speed;
        if self.sky_cloud_offset > 4096.0 {
            self.sky_cloud_offset -= 4096.0;
        }
        self.tick_casings();
        self.fx.shards.shards_tick(self.lv.lvupdate60);
        self.fx.sparks.tick(&self.lv);
        // wallhits_tick (lv.c:2302): the splats grow in and fade.
        self.wallhits_tick();
        if self.props.nbombs.active {
            self.nbombs_tick();
        }
        self.lv_update_misc_sfx();
        // lighting_tick (lv.c:2316).
        let highlight = self.scenario_highlighted_rooms();
        self.lights.lighting_tick(&self.roomflags, self.lv.lvupdate240, &mut self.rng, &highlight);
        self.fx.boltbeams.tick(self.lv.lvupdate60freal);
        // am_tick (lv.c:2319). SUBST: PD ticks the active menu before
        // menu_tick / after it: the session runs the menus before the world.
        self.am_tick(inputs);
        self.scenario_tick();
        if !self.mp.endscreen {
            self.props_tick();
        }

        // lv_tick_player: each player's player_tick.
        let idle = PlayerInput::default();
        for i in 0..self.players.len() {
            // player_tick's mission clock (player.c:3212).
            if !self.mp.endscreen {
                self.mp.players[i].bondviewlevtime60 += self.lv.lvupdate60;
            }
            let input = self.bmove_process_input_mp(i, inputs.get(i).unwrap_or(&idle));
            let input = &input;
            let cyls = self.perims_except(i);
            let floors = self.prop_floors();
            let (fastmovement, shieldfrac, menuopen, briefcase) = self.walk_opts(i);
            let env = WalkEnv { level: &self.level, cyls: &cyls, floors: &floors, fastmovement, shieldfrac, menuopen, briefcase };
            // player.c:3302: a rocket that is gone loses its signal.
            if self.players[i].visionmode == VISIONMODE_SLAYERROCKET && self.players[i].slayerrocket.is_none() {
                self.players[i].visionmode = VISIONMODE_SLAYERROCKETSTATIC;
            }
            if self.players[i].visionmode == VISIONMODE_SLAYERROCKET {
                // bmove_tick(0, 0, 0, 1): Jo stands still while the rocket flies.
                self.players[i].tick(&idle, &self.lv, &env, &self.res, &mut self.rng, &mut self.events);
                self.player_update_rooms(i);
                self.player_tick_slayer(i, input);
            } else {
                self.players[i].tick(input, &self.lv, &env, &self.res, &mut self.rng, &mut self.events);
                // bmove's am_open (bondmove.c:1256), after the tick here.
                if std::mem::take(&mut self.players[i].am_open_request) {
                    self.am_open(i);
                }
                self.players[i].cameramode = CAMERAMODE_DEFAULT;
                self.player_update_rooms(i);
                // The end of bwalk_tick (bondwalk.c:1834).
                if !self.players[i].isdead {
                    self.doors_check_automatic(i);
                }
            }
            if self.players[i].die_request {
                // player_die(true): fell for 4 s, or out of the world.
                self.player_die(i);
            }
            self.sync_player_chr(i);
            // lv_tick_player (lv.c:2371): the distance walked.
            let (pos, prev) = (self.players[i].pos, self.players[i].prevpos);
            self.mp.playerstats[i].distance += ((pos.x - prev.x).powi(2) + (pos.z - prev.z).powi(2)).sqrt();
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
        let n = self.players.len();
        let mut bodies_ticked = vec![false; n];
        for i in 0..n {
            // player_update_shoot_rot (lv.c:1187).
            self.players[i].player_update_shoot_rot();
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
            self.bg_tick(i);
            self.lights_tick(i);
            self.props_tick_player(i);
            if i == 0 {
                self.chrs_tick();
            }
            // player_tick_third_person: the other players' bodies.
            self.players_tick_bodies(i, &mut bodies_ticked);
            // scenario_tick_chr(NULL) (`lv.c:1195`).
            self.scenario_tick_chr(None, i);
            if i == 0 {
                // SUBST: PD clears g_PlayersDetonatingMines in alarm_tick after the
                // last player's props, which with two players loses the first
                // player's press for the mines only its pass ticks / cleared
                // once the first pass has ticked the mines (the same with one).
                self.props.detonating = 0;
            }
            // player_tick_third_person's own-pass branch: the player's cloak.
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
            // A cloaked chr isn't looked at (no IR scanner in a match).
            if let Some(crate::gun::shot::AimedAt::Chr(j)) = self.lookingatprop[i] {
                if self.chrs[j].cloak.cloaked {
                    self.lookingatprop[i] = None;
                }
            }
            // The Threat Detector and the aim-tracking weapons' boxes (lv.c:1238).
            self.lv_tick_tracked_props(i);
            // Opening doors and reloading (`lv.c:1293`).
            if self.players[i].bondactivateorreload && self.current_player_interact(i) {
                let res = self.res.clone();
                self.players[i].gun.bgun_reload_if_possible(&res.gset, HAND_RIGHT);
                self.players[i].gun.bgun_reload_if_possible(&res.gset, pd_core::ids::HAND_LEFT);
            }
            // props_test_for_pickup (`lv.c:1304`).
            self.props_test_for_pickup(i);
            // player_render_hud (`player.c`): in the third person (riding a
            // Slayer rocket) no gun, no HUD, and bgun_tick_gameplay2 doesn't run.
            // player_render_hud's player_render_shield (`player.c:4482`), after
            // the gun: the view's shield flash, moved on a frame.
            self.players[i].shieldflash = None;
            if self.players[i].cameramode != CAMERAMODE_THIRDPERSON {
                let c = &self.players[i].cam;
                let view = [c.c_screenleft, c.c_screentop, c.c_screenwidth, c.c_screenheight];
                let shield = self.player_get_shield_frac(i) * 8.0;
                let freal = self.lv.lvupdate60freal;
                self.players[i].shieldflash = self.players[i].shieldshow.player_render_shield(view, shield, freal);
            }
            if self.players[i].cameramode != CAMERAMODE_THIRDPERSON {
                // bgun_draw_sight → sight_draw → sight_tick (not while the
                // active menu is open): aiming, no damage flash, no menu.
                if self.players[i].activemenumode == crate::player::activemenu::AMMODE_CLOSED {
                    let menuopen = self.mp.menuopen.get(i).copied().unwrap_or(false);
                    let sighton = self.players[i].insightaimmode && !self.players[i].health.sightoff_damage && !menuopen;
                    self.sight_tick(i, sighton);
                }
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
                // The end of bgun_tick_gameplay2 (bondgun.c:9229).
                self.inv_increment_held_time(i);
            }
            self.process_gun_events(i);
            // am_render's writes (lv.c:1639).
            self.am_render_sim(i);
            // player_render_hud's death sequence (`player.c:4546`).
            let input = if self.mp.players[i].withcontrol { inputs.get(i).unwrap_or(&idle) } else { &idle };
            let canrestart = !self.mp_is_paused() && self.mp.numreasonstoend == 0;
            self.players[i].player_tick_death(input, canrestart);
            self.lv_render_fx(i, motion_blur);
            // SUBST: chr_render counts each chr it draws in the player's view
            // (chr.c:3570) / the chrs the renderer draws, those on screen.
            self.mp.playerstats[i].drawplayercount += self.chrs.iter().filter(|c| c.player.is_none() && c.onanyscreen).count() as i32;
            if self.players[i].dostartnewlife {
                // player_start_new_life (`lv.c:1652`).
                let others: Vec<usize> = (0..self.chrs.len()).filter(|&j| j != i).collect();
                self.spawn_player(i, &others);
                self.player_spawn_inventory(i);
            }
        }
        // chr_render_shield's bookkeeping for every chr drawn this frame.
        for ci in 0..self.chrs.len() {
            if self.chrs[ci].onanyscreen {
                self.chr_shield_crawl_tick(ci);
            }
        }
        if n == 0 {
            // A headless match (the harness, not PD): no player pass to tick
            // the doors, lifts and chrs in, or to free the objects taken.
            self.props_tick_machines();
            self.props_free_deleting();
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
            // props_tick_player: splat_tick_chr, then the chr's own tick.
            self.splat_tick_chr(b);
            self.bot_tick(b, true);
        }
        // The players' props: splat_tick_chr before player_tick_third_person.
        for p in 0..first {
            self.splat_tick_chr(p);
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
        // shieldhits_tick (`proptick.c:62`) before the props.
        self.shieldhits_tick();
        // player_tick_beams (`player.c:5255`): the hands' tracers, and in a
        // match the body's (the ones the other players see).
        for (pi, p) in self.players.iter_mut().enumerate() {
            for h in 0..2 {
                p.gun.hands[h].beam.tick(&mut self.rng, &self.lv);
            }
            for slot in self.chrs[pi].fireslots.iter_mut() {
                slot.beam.tick(&mut self.rng, &self.lv);
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
        // obj_tick (`propobj.c:10942`): a taken pickup's respawn, a sentry's beam.
        self.pickups_tick_regen();
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
            // The objects: a blast sets off the explosives it reaches; a
            // respawning pickup isn't there (`explosions.c:761`).
            .chain(self.props.objs.iter().filter(|o| !o.is_deleting() && o.timetoregen == 0 && o.flags & pd_core::ids::OBJFLAG_HELDROCKET == 0).map(|o| Victim { id: VictimId::Prop(o.id), pos: o.pos, chrbox: None }))
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
                self.room_flash_lighting(room, start as i32, 255);
            }
        }
    }

    /// `room_flash_lighting` (`dlights.c:1567`) with the rooms on screen as the
    /// latest portal tick left them.
    pub(crate) fn room_flash_lighting(&mut self, room: u16, start: i32, limit: i32) {
        self.lights.room_flash_lighting(&self.roomflags, room as usize, start, limit);
    }

    /// `lights_tick` (`dlights.c:1545`): a hand's muzzle flash lights the
    /// player's room.
    fn lights_tick(&mut self, pi: usize) {
        let p = &self.players[pi];
        if p.gun.hands.iter().any(|h| h.flashon) {
            if let Some(&room) = p.rooms.first() {
                self.room_flash_lighting(room, 64, 80);
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
