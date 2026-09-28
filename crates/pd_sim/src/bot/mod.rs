//! Simulants (`bot.c`, `botact.c`): the `aibot` state, `bot_tick` (facing,
//! aim, movement, then `chr_tick`), `bot_tick_unpaused` (reloads, the main
//! loop's attack, the round-robin route to the target, the per-hand trigger),
//! target choice (`bot_choose_general_target`, round-robin sight polling),
//! zeroing (`bot_update_zero_angle`, `g_BotDifficulties`), cloak awareness,
//! and spawning (`bot_reset`, `bot_spawn`). How an attacking simulant closes in
//! or backs off is [`botcmd`].
//!
//! Call graph per frame, PD's (`bot_tick`, `bot.c:909`), on the tick that
//! fully ticks the chr (the first player's pass):
//! 1. `bot_tick_unpaused`;
//! 2. facing: `theta` turns towards the target (plus the zeroing error) or the
//!    travel heading, at most 3.53°/tick, which sets `speedtheta`;
//! 3. `chr_calculate_aimend` (vertical only: a simulant's horizontal aim is its
//!    facing) and the speed multipliers;
//! 4. `bot_apply_movement` → `player_choose_third_person_animation`, and the
//!    model turned to `theta − angleoffset`;
//! 5. `chr_tick`: the action, the animation, the position, the pose, the shots.
//!
//! Nothing moves before `lvframe60 >= 145` (2.4 s into the match), as in PD.
//!
//! A simulant spawns unarmed and goes for the arena's pickups
//! (`bot_find_default_pickup`, [`botinv`]), choosing what to hold as it goes
//! (`botinv_tick`). Its personality (`BOTTYPE_*`) is the general sim's (M11).
//!
//! Sources: the old repo's `pd_spike/bot.rs` and `botcmd.rs`, checked against
//! `reference/pd_bot_port_sheet.md`, which added the per-weapon punch timers,
//! the Reaper's spin, the Cyclone's discharge, the Mauler's charge, the reload
//! sound, the dizzy wobble and the model's turn in `bot_apply_movement`.

pub mod botact;
pub mod botcmd;
pub mod botinv;

use glam::{Vec2, Vec3};
use pd_core::ids::*;
use pd_core::math::{atan2f, baddtor, baddtor2, dtor, turn, wrap_pos};

use crate::chr::thirdperson::{self, bot_guess_crouch_pos, AttackAnimConfig, WieldMode};
use crate::chr::{Act, Chr, HAND_LEFT, HAND_RIGHT};
use crate::gun::Gset;
use crate::nav::chrnavseed;
use crate::world::World;
use botcmd::DistMode;

/// `struct botdifficulty` (`bot.c:45`); the angles in degrees, `BADDTOR2` at use.
#[derive(Clone, Copy, Debug)]
pub struct BotDifficulty {
    pub shootdelay60: i32,
    pub minzerospeed: f32,
    pub maxzerospeed: f32,
    pub zerotime60: f32,
    pub turnunzeromult: f32,
    pub zerocloakspeed: f32,
    pub forcezerominspeed: f32,
    pub dizzyamount: i32,
}

const fn bd(shootdelay60: i32, minz: f32, maxz: f32, zerotime60: f32, turnunzeromult: f32, zerocloak: f32, forcez: f32, dizzy: i32) -> BotDifficulty {
    BotDifficulty { shootdelay60, minzerospeed: minz, maxzerospeed: maxz, zerotime60, turnunzeromult, zerocloakspeed: zerocloak, forcezerominspeed: forcez, dizzyamount: dizzy }
}

/// `g_BotDifficulties` (`bot.c:98`), meat to dark.
pub const G_BOT_DIFFICULTIES: [BotDifficulty; 6] = [
    bd(90, 15.0, 30.0, 600.0, 10.0, 40.0, 20.0, 1000),
    bd(60, 7.0, 14.0, 360.0, 10.0, 28.5, 8.0, 1000),
    bd(30, 4.0, 8.0, 180.0, 4.0, 20.0, 5.0, 1500),
    bd(15, 1.5, 4.0, 90.0, 2.0, 14.0, 2.0, 2500),
    bd(0, 0.0, 2.0, 45.0, 1.0, 10.0, 0.0, 4000),
    bd(0, 0.0, 0.0, 0.0, 0.0, 8.0, 0.0, 4000),
];

/// `struct mpbotconfig` as the brain reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BotConfig {
    /// `BOTDIFF_*`.
    pub difficulty: u8,
    /// `BOTTYPE_*`.
    pub bottype: u8,
}

impl BotConfig {
    pub fn tuning(&self) -> &'static BotDifficulty {
        &G_BOT_DIFFICULTIES[(self.difficulty as usize).min(5)]
    }
}

/// `chr->myaction` values a free-for-all general sim uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MyAction {
    MainLoop,
    Attack,
    /// `MA_AIBOTGETITEM`: going to `aibot->gotoprop`.
    GetItem,
}

/// `struct aibot` (the Combat fields).
#[derive(Clone, Debug)]
pub struct Aibot {
    pub config: BotConfig,
    /// `aibotnum`: the simulant's setup slot.
    pub aibotnum: usize,
    pub myaction: MyAction,
    pub weaponnum: u8,
    pub gunfunc: usize,
    pub ismeleeweapon: bool,
    pub loadedammo: [i32; 2],
    /// `ammoheld[]`: the reserve by ammo type.
    pub ammoheld: [i32; 33],
    /// `aibot->items`: the inventory (`botinv_init(chr, 10)`).
    pub items: [Option<botinv::BotInvItem>; botinv::MAX_BOTINV_ITEMS],
    /// `BOTFLAG_*`.
    pub flags: u32,
    /// `aibot->gotoprop`: the pickup it is going for (an object id).
    pub gotoprop: Option<u32>,
    pub throwtimer60: i32,
    /// What it has learnt of the set's weapons, by slot and function
    /// (`botinv_score_weapon`'s `learn`).
    pub equipdurations60: [[i32; 2]; 6],
    pub killsbygunfunc: [[f32; 2]; 6],
    pub suicidesbygunfunc: [[f32; 2]; 6],
    pub equipextrascores: [i32; 6],
    pub equipextrascorestimer60: i32,
    pub dampensuicidesttl60: i32,
    pub random1ttl60: i32,
    /// `aibot->cloakdeviceenabled` (the cloaking device: M11).
    pub cloakdeviceenabled: bool,
    /// `aibot->skrocket`: a Slayer rocket it is flying (an object id).
    pub skrocket: Option<u32>,
    pub timeuntilreload60: [i32; 2],
    pub nextbullettimer60: [i32; 2],
    pub burstsdone: [u8; 2],
    pub punchtimer60: [i32; 2],
    pub changeguntimer60: i32,
    pub reaperspeed: [i32; 2],
    pub maulercharge: [f32; 2],
    pub cyclonedischarging: [bool; 2],
    pub attackanimconfig: Option<&'static AttackAnimConfig>,
    pub speedmultforwards: f32,
    pub speedmultsideways: f32,
    pub speedtheta: f32,
    pub angleoffset: f32,
    pub lookangle: f32,
    pub roty: f32,
    pub moveratex: f32,
    pub moveratey: f32,
    pub distmode: Option<DistMode>,
    pub distmodettl60: i32,
    pub distoverrideprop: Option<usize>,
    pub distoverridetimer60: i32,
    pub attackingplayernum: Option<usize>,
    pub abortattacktimer60: i32,
    pub forcemainloop: bool,
    pub shotspeed: Vec3,
    pub shootdelaytimer60: i32,
    pub targetlastseen60: i32,
    pub lastseenanytarget60: i32,
    pub targetinsight: bool,
    pub queryplayernum: usize,
    pub chrnumsbydistanceasc: Vec<i32>,
    pub chrdistances: Vec<f32>,
    pub chrsinsight: Vec<bool>,
    pub chrslastseen60: Vec<i32>,
    /// `chrrooms[]`: the room each chr was in when last polled for sight.
    pub chrrooms: Vec<Option<u16>>,
    pub zeroangle: f32,
    pub zerospeed: f32,
    pub zeroinc: f32,
    pub random1: u32,
    pub random2ttl60: i32,
    pub random2: u32,
    pub randomfrac: f32,
    pub random3ttl60: i32,
    pub random3: u32,
    pub curzerotimer60: f32,
    pub realignangleframe: i32,
    /// PD's count (terminator included) of the route from the target back
    /// to this simulant, refreshed round-robin.
    pub numwaystepstotarget: i32,
    /// Spawn fade-in: drawn at `(120 − fadeintimer60) / 120` (`chr.c:3393`).
    pub fadeintimer60: i32,
    pub canseecloaked: bool,
    pub targetcloaktimer60: i32,
    /// The last distance `botcmd_tick_dist_mode` measured (debug).
    pub last_dist: f32,
    /// The last animation choice (debug).
    pub last_choice: Option<thirdperson::Choice>,
    /// `lastkilledbyplayernum`: the chr index of whoever last killed this
    /// simulant (`mpstats_record_death`), -1 none.
    pub lastkilledbyplayernum: i32,
}

impl Aibot {
    /// `botmgr_allocate_bot`'s aibot (`botmgr.c:112`) for `nchrs` MP chrs.
    pub fn new(config: BotConfig, aibotnum: usize, nchrs: usize, random1: u32) -> Aibot {
        Aibot {
            config,
            aibotnum,
            myaction: MyAction::MainLoop,
            weaponnum: WEAPON_UNARMED,
            gunfunc: FUNC_PRIMARY,
            ismeleeweapon: true,
            loadedammo: [0; 2],
            ammoheld: [0; 33],
            items: [None; botinv::MAX_BOTINV_ITEMS],
            flags: 0,
            gotoprop: None,
            throwtimer60: 0,
            equipdurations60: [[0; 2]; 6],
            killsbygunfunc: [[0.0; 2]; 6],
            suicidesbygunfunc: [[0.0; 2]; 6],
            equipextrascores: [0; 6],
            equipextrascorestimer60: 0,
            dampensuicidesttl60: 0,
            random1ttl60: 0,
            cloakdeviceenabled: false,
            skrocket: None,
            timeuntilreload60: [0; 2],
            nextbullettimer60: [0; 2],
            burstsdone: [0; 2],
            punchtimer60: [0, -1],
            changeguntimer60: 0,
            reaperspeed: [0; 2],
            maulercharge: [0.0; 2],
            cyclonedischarging: [false; 2],
            attackanimconfig: None,
            speedmultforwards: 0.0,
            speedmultsideways: 0.0,
            speedtheta: 0.0,
            angleoffset: 0.0,
            lookangle: 0.0,
            roty: 0.0,
            moveratex: 0.0,
            moveratey: 0.0,
            distmode: None,
            distmodettl60: 0,
            distoverrideprop: None,
            distoverridetimer60: 0,
            attackingplayernum: None,
            abortattacktimer60: -1,
            forcemainloop: false,
            shotspeed: Vec3::ZERO,
            shootdelaytimer60: 0,
            targetlastseen60: -1,
            lastseenanytarget60: -1,
            targetinsight: false,
            queryplayernum: 0,
            chrnumsbydistanceasc: vec![-1; nchrs],
            chrdistances: vec![u32::MAX as f32; nchrs],
            chrsinsight: vec![false; nchrs],
            chrslastseen60: vec![-1; nchrs],
            chrrooms: vec![None; nchrs],
            zeroangle: 0.0,
            zerospeed: 0.0,
            zeroinc: 0.0,
            random1,
            random2ttl60: 0,
            random2: 0,
            randomfrac: 0.0,
            random3ttl60: -1,
            random3: 0,
            curzerotimer60: 0.0,
            realignangleframe: -1,
            numwaystepstotarget: 0,
            fadeintimer60: 0,
            canseecloaked: false,
            targetcloaktimer60: 0,
            last_dist: 0.0,
            last_choice: None,
            lastkilledbyplayernum: -1,
        }
    }
}

/// `weapon_get_num_ticks_per_shot` (`gset.c:563`): `3600 / maxrpm` truncated,
/// for an automatic; 0 for a single-shot gun.
pub fn weapon_get_num_ticks_per_shot(gset: &Gset, weaponnum: u8, func: usize) -> i32 {
    match gset.func(weaponnum, func) {
        Some(f) if f.ftype == INVENTORYFUNCTYPE_SHOOT_AUTOMATIC => f.shoot.as_ref().map_or(0, |s| (3600.0 / s.maxrpm) as i32),
        _ => 0,
    }
}

/// `botact_get_shoot_interval60` (`botact.c:403`): a shooting function's
/// `unk24 + unk25`, 60 for a melee weapon but the Reaper, else 1.
pub fn botact_get_shoot_interval60(gset: &Gset, weaponnum: u8, func: usize) -> i32 {
    match gset.func(weaponnum, func) {
        Some(f) if matches!(f.ftype, INVENTORYFUNCTYPE_SHOOT_SINGLE | INVENTORYFUNCTYPE_SHOOT_AUTOMATIC | INVENTORYFUNCTYPE_SHOOT_PROJECTILE) => {
            f.shoot.as_ref().map_or(1, |s| s.unk24 + s.unk25)
        }
        Some(f) if f.ftype == INVENTORYFUNCTYPE_MELEE && weaponnum != WEAPON_REAPER => 60,
        _ => 1,
    }
}

/// `botact_get_clip_capacity_by_function` (`botact.c:37`).
pub fn botact_get_clip_capacity_by_function(gset: &Gset, weaponnum: u8, func: usize) -> i32 {
    if !(WEAPON_FALCON2..=WEAPON_SUICIDEPILL).contains(&weaponnum) {
        return 0;
    }
    let Some(w) = gset.weapon(weaponnum) else { return 0 };
    let ammoindex = w.functions[func.min(1)].as_ref().map_or(-1, |f| f.ammoindex);
    if ammoindex < 0 {
        return 0;
    }
    w.ammos.get(ammoindex as usize).and_then(|a| a.as_ref()).map_or(0, |a| a.clipsize)
}

/// `botact_is_weapon_throwable` (`botact.c:288`).
pub fn botact_is_weapon_throwable(weaponnum: u8, is_secondary: bool) -> bool {
    match weaponnum {
        WEAPON_LAPTOPGUN | WEAPON_DRAGON | WEAPON_COMBATKNIFE => is_secondary,
        WEAPON_GRENADE | WEAPON_NBOMB | WEAPON_TIMEDMINE | WEAPON_PROXIMITYMINE | WEAPON_REMOTEMINE => true,
        _ => false,
    }
}

/// `botinv_get_dist_config` (`botinv.c`): the function's `BOTDISTCFG_*`.
pub fn botinv_get_dist_config(gset: &Gset, weaponnum: u8, func: usize) -> usize {
    let pref = gset.weapon(weaponnum).and_then(|w| w.bot);
    match pref {
        Some(p) if func != FUNC_PRIMARY => p.secdistconfig,
        Some(p) => p.pridistconfig,
        None => botcmd::BOTDISTCFG_CLOSE,
    }
}

/// `bot_calculate_max_speed` (`bot.c:1096`), cm per 60 Hz tick.
pub fn bot_calculate_max_speed(c: &Chr) -> f32 {
    let Some(a) = &c.aibot else { return 0.0 };
    let mut speed = c.bodyheight * (1.0 / 159.0);
    speed = speed * 0.002_830_188_954_249 + 1.0;
    speed *= match a.config.difficulty {
        BOTDIFF_MEAT => 5.0,
        BOTDIFF_EASY => 6.2,
        BOTDIFF_NORMAL => 7.6,
        BOTDIFF_HARD => 9.4,
        _ => 11.2,
    };
    // Crouched: 0.35× squatting, 0.5× ducking; else the last leg of a go-to (no
    // waypoints left) within 2 m: half speed.
    let crouchpos = bot_guess_crouch_pos(c.height);
    if crouchpos == CROUCHPOS_SQUAT {
        speed *= 0.35;
    } else if crouchpos == CROUCHPOS_DUCK {
        speed *= 0.5;
    } else if c.actiontype == Act::GoPos
        && c.act_gopos.curindex >= c.act_gopos.waypoints.len()
        && Vec2::new(c.pos.x - c.act_gopos.endpos.x, c.pos.z - c.act_gopos.endpos.z).length() < 200.0
    {
        speed *= 0.5;
    }
    speed
}

/// `bot_update_lateral` (`bot.c:1152`): the smoothed displacement this frame.
pub fn bot_update_lateral(c: &mut Chr, lvframe60: i32, numupdates: i32, lvupdate60freal: f32) -> Vec2 {
    if lvframe60 < 145 {
        return Vec2::ZERO;
    }
    let speed = bot_calculate_max_speed(c);
    let roty = c.roty();
    let Some(a) = c.aibot.as_mut() else { return Vec2::ZERO };
    let speedsideways = a.speedmultsideways * speed;
    let speedforwards = a.speedmultforwards * speed;
    let (sine, cosine) = roty.sin_cos();
    let sp30 = Vec2::new(speedsideways * cosine + speedforwards * sine, -speedsideways * sine + speedforwards * cosine);
    let mut mv = Vec2::ZERO;
    let tmp = 0.055_000_007_152_557 * lvupdate60freal / numupdates as f32;
    for _ in 0..numupdates {
        a.moveratex = 0.945 * a.moveratex + sp30.x;
        a.moveratey = 0.945 * a.moveratey + sp30.y;
        mv.x += a.moveratex * tmp;
        mv.y += a.moveratey * tmp;
    }
    mv
}

/// The wield mode `player_choose_third_person_animation` derives from the hands.
pub fn wieldmode(gset: &Gset, c: &Chr) -> WieldMode {
    let (l, r) = (&c.held[HAND_LEFT], &c.held[HAND_RIGHT]);
    match (l, r) {
        (Some(_), Some(_)) => WieldMode::DualGuns,
        (None, None) => WieldMode::Unarmed,
        _ => {
            if l.as_ref().is_some_and(|g| !gset.has_flag(g.weaponnum, WEAPONFLAG_AICANUSE)) || r.as_ref().is_some_and(|g| !gset.has_flag(g.weaponnum, WEAPONFLAG_AICANUSE)) {
                WieldMode::Unarmed
            } else if l.as_ref().is_some_and(|g| gset.has_flag(g.weaponnum, WEAPONFLAG_ONEHANDED)) || r.as_ref().is_some_and(|g| gset.has_flag(g.weaponnum, WEAPONFLAG_ONEHANDED)) {
                WieldMode::Pistol
            } else {
                WieldMode::Heavy
            }
        }
    }
}

impl World {
    /// The simulant's aibot (panics on a player's chr: the callers are the bot's).
    pub(crate) fn ab(&self, i: usize) -> &Aibot {
        self.chrs[i].aibot.as_ref().expect("a simulant")
    }

    pub(crate) fn ab_mut(&mut self, i: usize) -> &mut Aibot {
        self.chrs[i].aibot.as_mut().expect("a simulant")
    }

    /// `bot_tick` (`bot.c:909`) for simulant `i`; `fulltick` is PD's
    /// `PROPFLAG_NOTYETTICKED` (this frame's first player pass).
    pub(crate) fn bot_tick(&mut self, i: usize, fulltick: bool) {
        let updateable = fulltick && self.lv.lvupdate240 > 0;
        if updateable && self.lv.lvframe60 >= 145 {
            // Not PD: a harness can switch the brains off (`World::bot_brains`).
            if self.bot_brains {
                self.bot_tick_unpaused(i);
            }
            // "cheap" (every room on screen until M9) has no reader in Combat.
            // Dampen blur.
            let lv60 = self.lv.lvupdate60;
            {
                let c = &mut self.chrs[i];
                if c.blurdrugamount > 0 {
                    c.blurdrugamount = c.blurdrugamount.min(5000);
                    c.blurdrugamount -= lv60 * (c.blurnumtimesdied + 1);
                    if c.blurdrugamount <= 0 {
                        c.blurdrugamount = 0;
                        c.blurnumtimesdied = 0;
                    }
                }
            }
            // Calculate the target angle.
            let about = self.bot_is_about_to_attack(i, false);
            let dead = self.chr_is_dead(i);
            let target_pos = self.chrs[i].target.map(|t| self.chrs[t].pos);
            let lvframe60 = self.lv.lvframe60;
            let freal = self.lv.lvupdate60freal;
            let c = &mut self.chrs[i];
            let oldangle = c.theta();
            let mut targetangle = if dead {
                c.theta()
            } else if about {
                let a = c.chr_get_angle_to_pos(target_pos.unwrap());
                oldangle + a + c.aibot.as_ref().unwrap().zeroangle
            } else {
                c.roty()
            };
            let t = turn();
            while targetangle >= t {
                targetangle -= t;
            }
            while targetangle < 0.0 {
                targetangle += t;
            }
            if c.blurdrugamount > 0 && !dead {
                targetangle += c.blurdrugamount as f32 * 0.000_314_109_260_216_36 * ((lvframe60 % 120) as f32 * 0.052_351_541_817_188).sin();
                if targetangle >= t {
                    targetangle -= t;
                }
                targetangle += t;
            }
            let tweenangle = freal * 0.061_590_049_415_827;
            let mut diffangle = targetangle - oldangle;
            if diffangle < dtor(-180.0) {
                diffangle += t;
            } else if diffangle >= dtor(180.0) {
                diffangle -= t;
            }
            let newangle = if diffangle >= 0.0 {
                if diffangle <= tweenangle {
                    targetangle
                } else {
                    let mut n = oldangle + tweenangle;
                    if n >= t {
                        n -= t;
                    }
                    n
                }
            } else if diffangle >= -tweenangle {
                targetangle
            } else {
                let mut n = oldangle - tweenangle;
                if n < 0.0 {
                    n += t;
                }
                n
            };
            let mut st = newangle - oldangle;
            if st < 0.0 {
                st += t;
            }
            if st >= dtor(180.0) {
                st -= t;
            }
            st /= freal;
            st *= 16.236_389_160_156;
            let a = c.aibot.as_mut().unwrap();
            a.speedtheta = st;
            // chr_set_theta: a simulant's lookangle.
            a.lookangle = wrap_pos(newangle);
        }
        if updateable && self.lv.lvframe60 >= 145 {
            let has_target = self.chrs[i].target.is_some() && !self.ab(i).ismeleeweapon;
            if has_target {
                let t = self.chrs[i].target.unwrap();
                let tpos = self.chrs[t].pos;
                // A player target: its eye height, and the RANDOMFRAC
                // chr_calculate_aimend draws for it (`chraction.c:9123`).
                let player = self.chrs[t].player.map(|_| (self.chrs[t].eyeheight, self.rng.randomfrac()));
                let cfg = self.ab(i).attackanimconfig;
                self.chrs[i].chr_calculate_aimend(tpos, cfg, player);
            } else {
                self.chrs[i].chr_reset_aimend();
            }
            let lvframe60 = self.lv.lvframe60;
            let c = &mut self.chrs[i];
            let act = c.actiontype;
            let a = c.aibot.as_mut().unwrap();
            if matches!(act, Act::Die | Act::Dead) {
                a.speedmultforwards = 0.0;
                a.speedmultsideways = 0.0;
            } else if act == Act::GoPos {
                // (GOPOSFLAG_WAITING is lift logic: no lifts in the arenas yet.)
                a.speedmultforwards = 1.0;
                a.speedmultsideways = 0.0;
            } else {
                a.speedmultforwards = 0.0;
                a.speedmultsideways = 0.0;
                a.realignangleframe = lvframe60;
            }
        }
        self.bot_apply_movement(i);
        self.chr_tick(i, fulltick);
        // bot.c:1085: (scenario_tick_chr, M10) then what it walked over.
        if self.lv.lvframe60 >= 145 && updateable && !self.chr_is_dead(i) {
            self.bot_check_pickups(i);
        }
    }

    /// `bot_is_about_to_attack(chr, forcloak)` (`bot.c:831`): should the
    /// simulant face its target rather than where it's going? Yes if the target
    /// is in sight; from Easy up, also if seen in the last 4 s or in the same
    /// room; from Normal up, also if the rooms neighbour (or the target is in,
    /// or next to, the room it was last polled in), or the route between them
    /// is 1–3 waypoints (1–4 from Hard). Meat and Easy also need the target
    /// within 25° / 90° of travel.
    pub fn bot_is_about_to_attack(&self, i: usize, forcloak: bool) -> bool {
        let c = &self.chrs[i];
        let Some(t) = c.target else { return false };
        let a = self.ab(i);
        let tc = &self.chrs[t];
        let mut result = a.chrsinsight[t];
        let diff = a.config.difficulty;
        if diff >= BOTDIFF_EASY {
            if a.chrslastseen60[t] >= self.lv.lvframe60 - 240 || c.rooms.iter().any(|r| tc.rooms.contains(r)) {
                result = true;
            }
            if diff >= BOTDIFF_NORMAL {
                let neighbours = |x: Option<u16>, y: Option<u16>| match (x, y) {
                    (Some(x), Some(y)) => self.level.rooms_are_neighbours(x, y),
                    _ => false,
                };
                let (r0, t0) = (c.rooms.first().copied(), tc.rooms.first().copied());
                let last = a.chrrooms[t];
                if neighbours(r0, t0) || (last.is_some() && last == t0) || neighbours(last, t0) {
                    result = true;
                }
                let steps = a.numwaystepstotarget;
                let max = if diff == BOTDIFF_NORMAL { 3 } else { 4 };
                if (1..=max).contains(&steps) {
                    result = true;
                }
            }
        }
        if !forcloak && (diff == BOTDIFF_MEAT || diff == BOTDIFF_EASY) {
            let mut angletotarget = atan2f(tc.pos.x - c.pos.x, tc.pos.z - c.pos.z) - c.roty();
            if angletotarget < 0.0 {
                angletotarget += turn();
            }
            if angletotarget > dtor(180.0) {
                angletotarget = turn() - angletotarget;
            }
            if diff == BOTDIFF_MEAT && angletotarget > baddtor(25.0) {
                result = false;
            } else if diff == BOTDIFF_EASY && angletotarget > baddtor2(90.0) {
                result = false;
            }
        }
        result
    }

    /// `bot_apply_movement` (`bot.c:763`): the speeds in the facing's frame for
    /// the animation chooser, then the model turned to `theta − angleoffset`.
    fn bot_apply_movement(&mut self, i: usize) {
        let freal = self.lv.lvupdate60freal;
        // `random() % g_NumDeathAnimations` is only drawn on the dead branch.
        let dead = self.chr_is_dead(i);
        let death_pick = if dead { self.rng.random() } else { 0 };
        let res = self.res.clone();
        let wm = wieldmode(&res.gset, &self.chrs[i]);
        let c = &mut self.chrs[i];
        let mut angle = c.theta() - c.roty();
        if angle < 0.0 {
            angle += turn();
        }
        let a = c.aibot.as_ref().unwrap();
        let speedforwards = a.speedmultforwards * angle.cos() - angle.sin() * a.speedmultsideways;
        let speedsideways = a.speedmultforwards * angle.sin() + angle.cos() * a.speedmultsideways;
        let speedtheta = a.speedtheta;
        let crouchpos = bot_guess_crouch_pos(c.height);
        let mut ctx = c.model.anim_ctx(&res.bank);
        if dead {
            thirdperson::choose_death(&mut c.anim, &mut ctx, death_pick);
            c.aibot.as_mut().unwrap().attackanimconfig = None;
        } else {
            let mut off = c.aibot.as_ref().unwrap().angleoffset;
            let (cfg, choice) = thirdperson::choose(&mut c.anim, &mut ctx, crouchpos, wm, speedsideways, speedforwards, speedtheta, &mut off, freal, false);
            let a = c.aibot.as_mut().unwrap();
            a.angleoffset = off;
            a.attackanimconfig = cfg;
            a.last_choice = Some(choice);
        }
        let mut angle2 = c.theta() - c.angleoffset();
        if angle2 < 0.0 {
            angle2 += turn();
        }
        if angle2 >= turn() {
            angle2 -= turn();
        }
        c.model.chrinfo.set_chr_rot_y(angle2);
    }

    /// `bot_tick_unpaused` (`bot.c:2445`), the free-for-all general-sim path.
    fn bot_tick_unpaused(&mut self, i: usize) {
        if self.chr_is_dead(i) {
            return;
        }
        let lv60 = self.lv.lvupdate60;
        let lvframe60 = self.lv.lvframe60;
        let gset = self.res.gset.clone();

        // Consider updating random values.
        self.ab_mut(i).random2ttl60 -= lv60;
        if self.ab(i).random2ttl60 < 0 {
            let x = self.rng.random();
            let y = self.rng.random();
            let f = self.rng.randomfrac();
            let a = self.ab_mut(i);
            a.random2ttl60 = 1800 + (x % (60 * 240)) as i32;
            a.random2 = y;
            a.randomfrac = f;
        }

        // Consider reloading.
        for h in 0..2 {
            let a = self.ab(i);
            if a.timeuntilreload60[h] > 0 {
                self.ab_mut(i).timeuntilreload60[h] -= lv60;
                if self.ab(i).timeuntilreload60[h] <= 0 {
                    self.botact_reload(i, h, true);
                }
            } else if !botact_is_weapon_throwable(a.weaponnum, a.gunfunc != FUNC_PRIMARY) {
                let loadedammo = a.loadedammo[h];
                let clipsize = botact_get_clip_capacity_by_function(&gset, a.weaponnum, a.gunfunc);
                if loadedammo <= 0 && clipsize > 0 {
                    self.bot_schedule_reload(i, h);
                } else if loadedammo < clipsize / 2 && a.lastseenanytarget60 < lvframe60 - 120 {
                    self.bot_schedule_reload(i, h);
                }
            }
        }
        // Switching weapons (`bot.c:2491`).
        self.bot_tick_changegun(i);
        // (bot.c:2522-2680: the laser's ammo, cloaks, KazeSim, the scenarios'
        // commands: M10/M11.)

        // The main loop (`bot.c:2681`): pickups it needs, else attack the
        // target, else anything to pick up. (Commands and personalities: M11;
        // following a teammate with no target, `bot_find_teammate_to_follow`: M11.)
        if self.ab(i).myaction == MyAction::MainLoop || self.ab(i).forcemainloop {
            {
                let a = self.ab_mut(i);
                a.forcemainloop = false;
                a.attackingplayernum = None;
            }
            let mut newaction = None;
            let gotoprop = self.bot_find_pickup(i, botinv::PICKUPCRITERIA_DEFAULT);
            self.ab_mut(i).gotoprop = gotoprop;
            if gotoprop.is_some() {
                newaction = Some(MyAction::GetItem);
            }
            if newaction.is_none() && self.chrs[i].target.is_some() {
                newaction = Some(MyAction::Attack);
                self.ab_mut(i).abortattacktimer60 = -1;
            }
            if newaction.is_none() {
                let gotoprop = self.bot_find_pickup(i, botinv::PICKUPCRITERIA_ANY);
                self.ab_mut(i).gotoprop = gotoprop;
                if gotoprop.is_some() {
                    newaction = Some(MyAction::GetItem);
                }
            }
            match newaction {
                Some(MyAction::GetItem) => {
                    // chr_go_to_prop(chr, gotoprop, GOPOSFLAG_RUN).
                    if let Some(pos) = self.ab(i).gotoprop.and_then(|id| self.props.get(id)).map(|o| o.pos) {
                        self.chr_go_to_room_pos(i, pos);
                        self.ab_mut(i).myaction = MyAction::GetItem;
                    }
                }
                Some(MyAction::Attack) => {
                    let a = self.ab_mut(i);
                    if a.myaction != MyAction::Attack {
                        a.myaction = MyAction::Attack;
                        a.distmode = None;
                    }
                }
                _ => {}
            }
        }
        // The action is no longer valid: back to the main loop (`bot.c:3225`).
        if self.ab(i).myaction == MyAction::GetItem {
            let gone = self.ab(i).gotoprop.and_then(|id| self.props.get(id)).is_none_or(|o| o.timetoregen != 0 || o.is_gone());
            if self.chrs[i].actiontype != crate::chr::Act::GoPos || gone {
                self.ab_mut(i).myaction = MyAction::MainLoop;
            }
        } else if self.ab(i).myaction == MyAction::Attack {
            let invalid = match self.ab(i).attackingplayernum {
                Some(p) => self.chr_is_dead(p),
                None => self.chrs[i].target.is_none_or(|t| self.chr_is_dead(t)),
            };
            if invalid {
                self.ab_mut(i).myaction = MyAction::MainLoop;
            } else {
                self.botcmd_tick_dist_mode(i);
                if self.bot_find_pickup(i, botinv::PICKUPCRITERIA_CRITICAL).is_some() {
                    // bot_can_do_critical_pickup.
                    self.ab_mut(i).myaction = MyAction::MainLoop;
                } else {
                    let a = self.ab(i);
                    if a.abortattacktimer60 >= 0 && a.targetlastseen60 < lvframe60 - a.abortattacktimer60 {
                        self.ab_mut(i).myaction = MyAction::MainLoop;
                    }
                }
            }
        }

        self.bot_choose_general_target(i);

        // Keep a route to the target, even if it won't be followed (`bot.c:3372`):
        // one chr per frame, round-robin. Only its length is used, by
        // `bot_is_about_to_attack`.
        if (self.lv.lvframenum.rem_euclid(self.chrs.len() as i32)) as usize == i {
            if let Some(t) = self.chrs[i].target {
                let first = self.nav.waypoint_find_closest_to_pos(&self.level, self.chrs[i].pos, &self.chrs[i].rooms);
                let last = self.nav.waypoint_find_closest_to_pos(&self.level, self.chrs[t].pos, &self.chrs[t].rooms);
                if let (Some(first), Some(last)) = (first, last) {
                    let seed = chrnavseed(lvframe60, self.chrs[i].chrnum);
                    let (_, n) = self.nav.nav_find_route(last, first, crate::nav::MAX_CHRWAYPOINTS, seed, &mut self.rng);
                    self.ab_mut(i).numwaystepstotarget = n;
                }
            }
        }
        // Tick the inventory: it may switch weapons.
        self.botinv_tick(i);

        self.bot_tick_triggers(i);
    }

    /// The per-hand trigger at the end of `bot_tick_unpaused` (`bot.c:3390`).
    fn bot_tick_triggers(&mut self, i: usize) {
        let gset = self.res.gset.clone();
        let lv60 = self.lv.lvupdate60;
        let lv60freal = self.lv.lvupdate60freal;
        let mut firingright = false;
        for h in 0..2 {
            let mut firing = false;
            let target = self.chrs[i].target;
            let target_dead = target.is_none_or(|t| self.chr_is_dead(t));
            let target_pos = target.map(|t| self.chrs[t].pos);
            let dizzy = self.bot_is_dizzy(i);
            let shootdelay60 = self.ab(i).config.tuning().shootdelay60;
            {
                let a = self.ab_mut(i);
                if a.nextbullettimer60[h] > 0 {
                    a.nextbullettimer60[h] -= lv60;
                }
                // Don't shoot the left hand on the same frame as the right.
                if h == HAND_LEFT && firingright {
                    a.nextbullettimer60[h] = 1;
                }
            }
            if self.ab(i).skrocket.is_none() && self.ab(i).changeguntimer60 <= 0 {
                let (weaponnum, gunfunc) = (self.ab(i).weaponnum, self.ab(i).gunfunc);
                if self.ab(i).ismeleeweapon {
                    // Punching (and pistol whips): punchtimer60 is 0 idle,
                    // positive cooling down, negative "punch now".
                    let a = self.ab(i);
                    let minclip = crate::gun::bgun_get_min_clip_qty(&gset, WEAPON_TRANQUILIZER, FUNC_SECONDARY);
                    if a.punchtimer60[h] >= 0 && a.timeuntilreload60[h] <= 0 && weaponnum == WEAPON_TRANQUILIZER && a.loadedammo[h] < minclip {
                        self.ab_mut(i).punchtimer60[h] = 0;
                        self.bot_schedule_reload(i, h);
                    } else if a.punchtimer60[h] >= 0 && a.timeuntilreload60[h] <= 0 {
                        let range = 210.0;
                        self.ab_mut(i).punchtimer60[h] -= lv60;
                        let a = self.ab(i);
                        if target.is_some() && a.targetinsight && a.shootdelaytimer60 >= shootdelay60 {
                            if !dizzy {
                                let tp = target_pos.unwrap();
                                let c = &self.chrs[i];
                                let out = if weaponnum == WEAPON_TRANQUILIZER { !c.chr_is_pos_in_fov(tp, 30) || c.pos.distance(tp) > range } else { !c.chr_is_pos_in_fov(tp, 40) || c.pos.distance(tp) > range + 150.0 };
                                if out {
                                    self.ab_mut(i).punchtimer60[h] = 0;
                                }
                            }
                        } else {
                            self.ab_mut(i).punchtimer60[h] = 0;
                        }
                        if self.ab(i).punchtimer60[h] < 0 {
                            self.chr_uncloak_temporarily_chr(i);
                            self.chr_punch_inflict_damage(i, 2.0, range);
                            if h == HAND_RIGHT {
                                self.bot_punch_cooldown(i, weaponnum);
                            }
                        }
                    }
                } else if weaponnum == WEAPON_SLAYER && gunfunc != FUNC_PRIMARY && target.is_some() {
                    // botact_create_slayer_rocket: never reached, the fly-by-wire
                    // scores 0 (see botinv_score_weapon's SUBST).
                } else if botact_is_weapon_throwable(weaponnum, gunfunc != FUNC_PRIMARY) {
                    // A throw from the right hand (`bot.c:3547`).
                    if h == HAND_RIGHT {
                        if self.ab(i).throwtimer60 > 0 {
                            self.ab_mut(i).throwtimer60 -= lv60;
                        }
                        if self.ab(i).throwtimer60 <= 0 && (self.ab(i).botact_get_ammo_quantity_by_weapon(&gset, weaponnum, gunfunc, false) > 0 || weaponnum == WEAPON_LAPTOPGUN || weaponnum == WEAPON_DRAGON) {
                            let a = self.ab(i);
                            let infov = target_pos.is_some_and(|tp| self.chrs[i].chr_is_pos_in_fov(tp, 45));
                            if target.is_some() && a.targetinsight && a.shootdelaytimer60 >= shootdelay60 && (dizzy || infov) {
                                self.chr_uncloak_temporarily_chr(i);
                                self.ab_mut(i).botact_try_remove_ammo_from_reserve(&gset, weaponnum, gunfunc, 1);
                                self.botact_throw(i);
                                if gset.func(weaponnum, gunfunc).is_some_and(|f| f.flags & FUNCFLAG_DISCARDWEAPON != 0) {
                                    self.ab_mut(i).botinv_remove_item(weaponnum);
                                    self.botinv_switch_to_weapon(i, WEAPON_UNARMED, FUNC_PRIMARY);
                                }
                                self.ab_mut(i).throwtimer60 = botact::botact_get_projectile_throw_interval(weaponnum);
                            }
                        }
                    }
                } else if self.chrs[i].held[h].is_some() && self.ab(i).loadedammo[h] > 0 {
                    let tps = weapon_get_num_ticks_per_shot(&gset, weaponnum, gunfunc);
                    let mut canshoot = false;
                    if tps <= 0 {
                        let a = self.ab_mut(i);
                        if weaponnum == WEAPON_MAULER && gunfunc == FUNC_SECONDARY && a.loadedammo[h] >= 2 {
                            let old = a.maulercharge[h] as i32;
                            a.maulercharge[h] = (a.maulercharge[h] + lv60freal * 0.05).min(5.0);
                            if a.maulercharge[h] as i32 != old {
                                a.loadedammo[h] -= 1;
                            }
                        }
                        if a.nextbullettimer60[h] <= 0 {
                            canshoot = true;
                        }
                    } else {
                        canshoot = true;
                    }
                    if canshoot {
                        let infov = target_pos.is_some_and(|tp| self.chrs[i].chr_is_pos_in_fov(tp, 45));
                        let a = self.ab_mut(i);
                        if a.cyclonedischarging[h] || a.burstsdone[h] > 0 {
                            firing = true;
                        } else if target.is_some() && a.targetinsight && a.shootdelaytimer60 >= shootdelay60 && (dizzy || infov) && !target_dead {
                            firing = true;
                            if weaponnum == WEAPON_CYCLONE && gunfunc == FUNC_SECONDARY {
                                a.cyclonedischarging[h] = true;
                            } else if weaponnum == WEAPON_REAPER {
                                a.reaperspeed[h] = (a.reaperspeed[h] + lv60).min(90);
                            }
                        }
                        // The Reaper keeps shooting a moment after the trigger goes.
                        if !firing && a.reaperspeed[h] > 0 {
                            firing = true;
                            a.reaperspeed[h] = (a.reaperspeed[h] - lv60).max(0);
                        }
                    }
                    if tps <= 0 && firing {
                        let interval = botact_get_shoot_interval60(&gset, weaponnum, gunfunc);
                        let flags = gset.func(weaponnum, gunfunc).map_or(0, |f| f.flags);
                        let a = self.ab_mut(i);
                        a.nextbullettimer60[h] = interval;
                        if flags & (FUNCFLAG_BURST3 | FUNCFLAG_BURST2) != 0 && a.loadedammo[h] >= 2 {
                            let burstqty = if flags & FUNCFLAG_BURST2 != 0 { 2 } else { 3 };
                            a.burstsdone[h] = (a.burstsdone[h] + 1) % burstqty;
                            if a.burstsdone[h] != 0 {
                                a.nextbullettimer60[h] = 5;
                            }
                        }
                    }
                } else {
                    let a = self.ab_mut(i);
                    a.cyclonedischarging[h] = false;
                    a.burstsdone[h] = 0;
                    a.reaperspeed[h] = 0;
                }
            }
            if firing {
                self.chr_uncloak_temporarily_chr(i);
                if h == HAND_RIGHT {
                    firingright = true;
                }
            }
            self.chrs[i].chr_set_hand_firing(h, firing);
        }
    }

    /// The punch cooldown per weapon (`bot.c:3450`).
    fn bot_punch_cooldown(&mut self, i: usize, weaponnum: u8) {
        let diff = self.ab(i).config.difficulty;
        match weaponnum {
            WEAPON_FALCON2 | WEAPON_FALCON2_SILENCER | WEAPON_FALCON2_SCOPE | WEAPON_DY357MAGNUM | WEAPON_DY357LX | WEAPON_COMBATKNIFE => {
                let t = match diff {
                    BOTDIFF_MEAT => 120,
                    BOTDIFF_EASY => 90,
                    _ => 60,
                };
                let left = self.chrs[i].held[HAND_LEFT].is_some();
                let a = self.ab_mut(i);
                a.punchtimer60[0] = t;
                if left {
                    a.punchtimer60[1] = t - 40;
                }
            }
            WEAPON_TRANQUILIZER => {
                let q = crate::gun::bgun_get_min_clip_qty(&self.res.gset, WEAPON_TRANQUILIZER, FUNC_SECONDARY);
                let a = self.ab_mut(i);
                a.punchtimer60[0] = 60;
                a.loadedammo[0] -= q;
            }
            WEAPON_REAPER => self.ab_mut(i).punchtimer60[0] = 0,
            _ => {
                let t = match diff {
                    BOTDIFF_MEAT => 120,
                    BOTDIFF_EASY => 60,
                    _ => 30,
                };
                let r = self.rng.random();
                let a = self.ab_mut(i);
                a.punchtimer60[0] = t;
                if r.is_multiple_of(3) {
                    a.punchtimer60[1] = t - 20;
                }
            }
        }
    }

    /// `bot_is_dizzy` (`bot.c:103`).
    pub fn bot_is_dizzy(&self, i: usize) -> bool {
        self.chrs[i].blurdrugamount >= self.ab(i).config.tuning().dizzyamount
    }

    /// `bot_schedule_reload` (`bot.c:1832`).
    fn bot_schedule_reload(&mut self, i: usize, hand: usize) {
        let gset = self.res.gset.clone();
        let (weaponnum, gunfunc, loaded) = (self.ab(i).weaponnum, self.ab(i).gunfunc, self.ab(i).loadedammo[hand]);
        let pref = gset.weapon(weaponnum).and_then(|w| w.bot).unwrap_or_default();
        let mut t = pref.reloaddelay * 60;
        if pref.allowpartialreloaddelay != 0 {
            let capacity = botact_get_clip_capacity_by_function(&gset, weaponnum, gunfunc);
            if capacity > 0 {
                t *= capacity - loaded;
                t /= capacity;
            }
        }
        self.ab_mut(i).timeuntilreload60[hand] = t;
    }

    /// `botact_reload` (`botact.c:51`): the clip topped up from the reserve at
    /// once, with the reload sound at the simulant.
    pub(crate) fn botact_reload(&mut self, i: usize, hand: usize, withsound: bool) {
        let gset = self.res.gset.clone();
        {
            let a = self.ab_mut(i);
            a.timeuntilreload60[hand] = 0;
            a.maulercharge[hand] = 0.0;
        }
        let (weaponnum, gunfunc) = (self.ab(i).weaponnum, self.ab(i).gunfunc);
        if self.chrs[i].held[hand].is_none() || botact_is_weapon_throwable(weaponnum, gunfunc != FUNC_PRIMARY) {
            return;
        }
        let capacity = botact_get_clip_capacity_by_function(&gset, weaponnum, gunfunc);
        if capacity > 0 {
            let a = self.ab_mut(i);
            let tryamount = capacity - a.loadedammo[hand];
            let actual = a.botact_try_remove_ammo_from_reserve(&gset, weaponnum, gunfunc, tryamount);
            a.loadedammo[hand] += actual;
            if actual > 0 && withsound {
                let sound = if weaponnum == WEAPON_FARSIGHT { 0x0433 } else { 0x804f };
                let pos = self.chrs[i].pos;
                self.sound_at(sound, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
            }
        }
    }

    // ─── targeting ───────────────────────────────────────────────────────────

    /// `bot_is_target_invisible` (`bot.c:1405`): a cloaked chr can't be seen
    /// unless it is this simulant's target and was in sight within
    /// `targetcloaktimer60`, or the rare `canseecloaked` roll is on while it
    /// looks at it (±45/256 of a turn).
    pub fn bot_is_target_invisible(&self, bot: usize, other: usize) -> bool {
        let o = &self.chrs[other];
        if !o.cloaked {
            return false;
        }
        let b = &self.chrs[bot];
        let a = self.ab(bot);
        if b.target == Some(other) && a.targetcloaktimer60 > 0 {
            return false;
        }
        if a.canseecloaked && b.chr_is_pos_in_fov(o.pos, 32) {
            return false;
        }
        true
    }

    /// `bot_set_target` (`bot.c:1358`).
    fn bot_set_target(&mut self, i: usize, target: Option<usize>) {
        let (lvupdate240, diffframe60, lv60) = (self.lv.lvupdate240, self.lv.diffframe60, self.lv.lvupdate60);
        let other_cloaked = target.is_some_and(|t| self.chrs[t].cloaked);
        let old = self.chrs[i].target;
        let a = self.chrs[i].aibot.as_mut().unwrap();
        if let Some(t) = target {
            a.targetinsight = a.chrsinsight[t];
            a.targetlastseen60 = a.chrslastseen60[t];
        } else {
            a.targetinsight = false;
            a.targetlastseen60 = -1;
        }
        if a.targetlastseen60 > a.lastseenanytarget60 {
            a.lastseenanytarget60 = a.targetlastseen60;
        }
        if old != target {
            a.shootdelaytimer60 = 0;
        } else if a.targetinsight {
            if lvupdate240 > 0 {
                a.shootdelaytimer60 += diffframe60;
            }
        } else {
            if lvupdate240 > 0 {
                a.shootdelaytimer60 -= diffframe60;
            }
            if a.shootdelaytimer60 < 0 {
                a.shootdelaytimer60 = 0;
            }
        }
        // `bot.c:1392`: an uncloaked target in sight keeps two seconds of memory.
        if a.targetinsight && target.is_some() {
            if !other_cloaked {
                a.targetcloaktimer60 = 120;
            } else if a.targetcloaktimer60 > 0 {
                a.targetcloaktimer60 -= lv60;
            }
        } else {
            a.targetcloaktimer60 = 0;
        }
        self.chrs[i].target = target;
    }

    /// `bot_update_zero_angle` (`bot.c:1461`).
    fn bot_update_zero_angle(&mut self, i: usize) {
        let (lv60, lv240, diffframe60, lv60f) = (self.lv.lvupdate60, self.lv.lvupdate240, self.lv.diffframe60, self.lv.lvupdate60f);
        let needs_roll = self.ab(i).random3ttl60 - lv60 <= 0;
        let target_cloaked = self.chrs[i].target.is_some_and(|t| self.chrs[t].cloaked);
        let (r1, r2) = if needs_roll { (self.rng.random(), self.rng.random()) } else { (0, 0) };
        let a = self.ab_mut(i);
        let d = a.config.tuning();
        a.random3ttl60 -= lv60;
        if a.random3ttl60 <= 0 {
            a.random3 = r1;
            a.random3ttl60 = 20 + (r2 % 20) as i32;
        }
        if lv240 > 0 {
            if a.targetinsight {
                a.curzerotimer60 += diffframe60 as f32;
            } else {
                a.curzerotimer60 -= diffframe60 as f32;
            }
            a.curzerotimer60 -= (d.turnunzeromult * (a.speedtheta * lv60f)).abs();
        }
        if a.curzerotimer60 > a.shootdelaytimer60 as f32 {
            a.curzerotimer60 = a.shootdelaytimer60 as f32;
        }
        if a.curzerotimer60 < 0.0 {
            a.curzerotimer60 = 0.0;
        }
        let (minspeed, mut maxspeed);
        if a.curzerotimer60 >= d.zerotime60 {
            a.curzerotimer60 = d.zerotime60;
            minspeed = 0.0;
            maxspeed = 0.0;
        } else {
            let frac = (d.zerotime60 - a.curzerotimer60) / d.zerotime60;
            minspeed = baddtor2(d.minzerospeed) * frac;
            maxspeed = baddtor2(d.maxzerospeed) * frac;
        }
        // A cloaked target: at least `zerocloakspeed` (`bot.c:1505`).
        if target_cloaked && maxspeed < baddtor2(d.zerocloakspeed) {
            maxspeed = baddtor2(d.zerocloakspeed);
        }
        if maxspeed < baddtor2(d.forcezerominspeed) {
            maxspeed = baddtor2(d.forcezerominspeed);
        }
        a.zeroinc = (maxspeed - minspeed) * (a.random3 % 0x10000) as f32 * (1.0 / 65535.0) + minspeed;
        if a.random3 & 0x10000 != 0 {
            a.zeroinc = -a.zeroinc;
        }
        for _ in 0..lv240 {
            a.zerospeed = a.zerospeed * 0.975_000_023_841_86 + a.zeroinc;
        }
        a.zeroangle = a.zerospeed * 0.024_999_976_158_142;
    }

    /// `bot_choose_general_target` (`bot.c:1024`): poll one chr's sight per
    /// frame, round-robin, then keep a target in sight, else take the nearest in
    /// sight, else the nearest.
    fn bot_choose_general_target(&mut self, i: usize) {
        let n = self.chrs.len();
        let (lv60, lvframe60) = (self.lv.lvupdate60, self.lv.lvframe60);
        let q = (self.ab(i).queryplayernum + 1) % n;
        self.ab_mut(i).queryplayernum = q;
        if q != i {
            // Once every 4 minutes per chr on average (`bot.c:1610`).
            if self.rng.random() % (4 * 60 * 60) < (n as u32) * lv60.max(0) as u32 {
                self.ab_mut(i).canseecloaked = true;
            }
            let dist = self.chrs[i].pos.distance(self.chrs[q].pos);
            let insight = self.chr_has_los_to_chr(i, q);
            self.ab_mut(i).canseecloaked = false;
            // `// SUBST:` `chr_has_los_to_chr` hands back the sight ray's final
            // room, found through the portals / without portals (M9), the
            // polled chr's own room.
            let room = self.chrs[q].rooms.first().copied();
            let a = self.ab_mut(i);
            a.chrdistances[q] = dist;
            a.chrsinsight[q] = insight;
            a.chrrooms[q] = room;
        }
        {
            let a = self.ab_mut(i);
            for k in 0..n {
                if a.chrsinsight[k] {
                    a.chrslastseen60[k] = lvframe60;
                }
            }
            // chrnumsbydistanceasc: a selection sort by distance.
            let mut done = vec![false; n];
            for slot in 0..n {
                let mut closest: Option<(usize, f32)> = None;
                for j in 0..n {
                    if !done[j] && closest.is_none_or(|(_, d)| a.chrdistances[j] < d) {
                        closest = Some((j, a.chrdistances[j]));
                    }
                }
                if let Some((j, _)) = closest {
                    a.chrnumsbydistanceasc[slot] = j as i32;
                    done[j] = true;
                }
            }
        }
        self.bot_update_zero_angle(i);

        // Drop a dead target, one out of sight and invisible (cloaked), or a
        // teammate (bot.c:1679). `// M11:` the peace and coward checks.
        if let Some(t) = self.chrs[i].target {
            if self.chr_is_dead(t) || (!self.ab(i).targetinsight && self.bot_is_target_invisible(i, t)) || self.chr_compare_teams(i, t, crate::mp::Compare::Friends) {
                self.chrs[i].target = None;
            }
        }
        let order = self.ab(i).chrnumsbydistanceasc.clone();
        if self.chrs[i].target.is_none() {
            let diff = self.ab(i).config.difficulty;
            let mut closestavailable: Option<usize> = None;
            for &k in &order {
                if k < 0 {
                    continue;
                }
                let k = k as usize;
                if k != i && !self.chr_is_dead(k) && self.chr_compare_teams(i, k, crate::mp::Compare::Enemies) {
                    if self.ab(i).chrsinsight[k] {
                        self.bot_set_target(i, Some(k));
                        return;
                    }
                    if !self.bot_is_target_invisible(i, k) && (diff == BOTDIFF_MEAT || diff == BOTDIFF_EASY) {
                        self.bot_set_target(i, Some(k));
                        return;
                    }
                    if !self.bot_is_target_invisible(i, k) && closestavailable.is_none() {
                        closestavailable = Some(k);
                    }
                }
            }
            self.bot_set_target(i, closestavailable);
            return;
        }
        // An existing target still in sight: keep it.
        let t = self.chrs[i].target.unwrap();
        if self.ab(i).chrsinsight[t] {
            self.bot_set_target(i, Some(t));
            return;
        }
        // Otherwise the nearest chr in sight, if any.
        for &k in &order {
            if k < 0 {
                continue;
            }
            let k = k as usize;
            if self.ab(i).chrsinsight[k] && k != i && !self.chr_is_dead(k) && self.chr_compare_teams(i, k, crate::mp::Compare::Enemies) {
                self.bot_set_target(i, Some(k));
                return;
            }
        }
        self.bot_set_target(i, Some(t));
    }

    // ─── spawning ────────────────────────────────────────────────────────────

    /// `bot_reset(chr, respawning)` (`bot.c:108`). The first spawn's (PD's
    /// `bot_spawn_all` passes false) keeps the aibot `botmgr_allocate_bot` made.
    fn bot_reset(&mut self, i: usize, respawning: bool) {
        let n = self.chrs.len();
        let c = &mut self.chrs[i];
        c.fadealpha = -1.0;
        c.cloaked = false;
        c.aibot.as_mut().expect("a simulant").myaction = MyAction::MainLoop;
        if respawning {
            let r1 = self.rng.random();
            let r2 = self.rng.random();
            let rf = self.rng.randomfrac();
            let c = &mut self.chrs[i];
            let old = c.aibot.take().expect("a simulant");
            c.damage = 0.0;
            c.target = None;
            c.unk32c_12 = 0;
            c.fireslots = Default::default();
            c.firecount = [0; 2];
            c.held = [None, None];
            c.height = 185.0;
            // The fresh aibot is bot_reset's list: no ammo, an empty inventory
            // (botinv_clear), the fists, nothing to fetch.
            let mut a = Aibot::new(old.config, old.aibotnum, n, r1);
            // What it has learnt of the set is kept (only botmgr_allocate_bot
            // clears it).
            a.killsbygunfunc = old.killsbygunfunc;
            a.suicidesbygunfunc = old.suicidesbygunfunc;
            a.equipdurations60 = old.equipdurations60;
            a.equipextrascores = old.equipextrascores;
            a.equipextrascorestimer60 = old.equipextrascorestimer60;
            a.dampensuicidesttl60 = old.dampensuicidesttl60;
            a.lastkilledbyplayernum = old.lastkilledbyplayernum;
            // Kept over a respawn: the facing (reset from the model in
            // bot_spawn), the moverates and the zeroing memory PD doesn't clear.
            a.zeroangle = old.zeroangle;
            a.zerospeed = old.zerospeed;
            a.zeroinc = old.zeroinc;
            a.random3 = old.random3;
            a.random3ttl60 = old.random3ttl60;
            a.curzerotimer60 = old.curzerotimer60;
            a.realignangleframe = old.realignangleframe;
            a.attackingplayernum = old.attackingplayernum;
            a.abortattacktimer60 = old.abortattacktimer60;
            a.random2 = r2;
            a.randomfrac = rf;
            a.random2ttl60 = 0;
            c.aibot = Some(Box::new(a));
            self.chr_set_shield(i, 0.0);
        }
        self.bot_reset_shield(i);
        self.ab_mut(i).fadeintimer60 = 120;
    }

    /// `bot_spawn(chr, respawning)` (`bot.c:262`): reset, then placed by
    /// `player_choose_spawn_location` and `chr_move_to_pos` (`chraction.c`).
    /// PD then sets `roty` and `lookangle` from the model's yaw, not the pad's
    /// (`chr_set_theta` only set `lookangle`): a respawned simulant starts
    /// facing the way its old body did.
    pub(crate) fn bot_spawn(&mut self, i: usize, respawning: bool) {
        self.bot_reset(i, respawning);
        let (pos, angle) = self.chr_choose_spawn_location(i);
        self.chr_move_to_pos(i, pos, angle);
        let yaw = self.chrs[i].model.chrinfo.yrot;
        let a = self.ab_mut(i);
        a.roty = yaw;
        a.angleoffset = 0.0;
        a.speedtheta = 0.0;
        a.lookangle = yaw;
        a.moveratex = 0.0;
        a.moveratey = 0.0;
        let lvframe60 = self.lv.lvframe60;
        let c = &mut self.chrs[i];
        c.actiontype = Act::Stand;
        c.lastmoveok60 = lvframe60;
        if !self.bot_loadout.is_empty() {
            self.bot_give_loadout(i);
        }
    }

    /// Not PD: the harness's loadout ([`World::bot_loadout`], the A/B probes
    /// and the tests that stand a simulant somewhere armed): the weapon in the
    /// inventory (a pair if asked and it dual-wields), a full reserve and PD's
    /// unlimited-ammo flag (`BOTFLAG_UNLIMITEDAMMO`), and in hand at once, loaded.
    pub(crate) fn bot_give_loadout(&mut self, i: usize) {
        let Some((weaponnum, dual)) = self.bot_loadout.get(self.ab(i).aibotnum).copied().flatten() else { return };
        let dual = dual && self.res.gset.has_flag(weaponnum, WEAPONFLAG_DUALWIELD);
        let gset = self.res.gset.clone();
        {
            let a = self.ab_mut(i);
            // A full reserve: PD's reload reads the reserve before the flag
            // (`botact_try_remove_ammo_from_reserve`).
            for f in [FUNC_PRIMARY, FUNC_SECONDARY] {
                let t = botinv::botact_get_ammo_type_by_function(&gset, weaponnum, f);
                if t > 0 {
                    a.ammoheld[t as usize] = crate::gun::Bgun::bgun_get_capacity_by_ammotype(t);
                }
            }
            a.botinv_give_single_weapon(weaponnum);
            if dual {
                a.botinv_give_dual_weapon(weaponnum);
            }
            a.flags |= BOTFLAG_UNLIMITEDAMMO;
            a.weaponnum = weaponnum;
            a.gunfunc = FUNC_PRIMARY;
            a.ismeleeweapon = false;
            a.changeguntimer60 = 0;
        }
        if self.chr_give_weapon(i, weaponnum, HAND_RIGHT) {
            self.botact_reload(i, HAND_RIGHT, false);
        }
        if dual && self.chr_give_weapon(i, weaponnum, HAND_LEFT) {
            self.botact_reload(i, HAND_LEFT, false);
        }
    }
}
#[cfg(test)]
pub(crate) mod tests;
