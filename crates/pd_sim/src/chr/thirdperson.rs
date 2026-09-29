//! `player_choose_third_person_animation` (`player.c:5472`), the one function
//! that animates a simulant's body (and a player's, seen by another player),
//! and its table `var80070ba4` (`player.c:4155`).
//!
//! Simulants never enter PD's guard `ACT_ATTACK` and never play an attack
//! clip. Standing, walking, running and shooting are all this: seven rows per
//! wield mode (standing still / soft turn / hard turn, ducking and squatting
//! still / moving), picked from the chr's crouch position and how fast it moves
//! relative to where it faces, with the legs twisted towards the travel
//! direction (±60°, ±30° squatting) and the clip played **backwards** when the
//! chr moves away from where it faces. The rows' `attackanimconfig`s matter for
//! the idle clip and the vertical aim limits (`chr_calculate_aimend_vertical`).
//!
//! Source: the old repo's `pd_spike/thirdperson.rs`, on `pd_core::anim`.

use pd_core::anim::{Anim, AnimCtx};
use pd_core::ids::{CROUCHPOS_DUCK, CROUCHPOS_SQUAT};
use pd_core::math::baddtor;

/// The animations this chooser and the death branch can pick, by PD number.
pub const ANIM_0002: u16 = 0x0002;
pub const ANIM_0030: u16 = 0x0030;
pub const ANIM_0031: u16 = 0x0031;
pub const ANIM_0041: u16 = 0x0041;
pub const ANIM_0052: u16 = 0x0052;
pub const ANIM_0055: u16 = 0x0055;
pub const ANIM_RUNNING_ONEHANDGUN: u16 = 0x0059;
pub const ANIM_006A: u16 = 0x006a;
pub const ANIM_006B: u16 = 0x006b;
pub const ANIM_006C: u16 = 0x006c;
pub const ANIM_006E: u16 = 0x006e;
pub const ANIM_007A: u16 = 0x007a;
pub const ANIM_0280: u16 = 0x0280;
pub const ANIM_0281: u16 = 0x0281;
pub const ANIM_0282: u16 = 0x0282;
pub const ANIM_0283: u16 = 0x0283;
pub const ANIM_0284: u16 = 0x0284;
pub const ANIM_0285: u16 = 0x0285;
pub const ANIM_0286: u16 = 0x0286;
pub const ANIM_0287: u16 = 0x0287;

/// `g_DeathAnimations` (`player.c:187`).
pub const DEATH_ANIMS: [u16; 8] = [0x001a, 0x001c, 0x0020, 0x0021, 0x0022, 0x0023, 0x0024, 0x0025];

/// The fields of `struct attackanimconfig` the chooser and the aim read.
#[derive(Clone, Copy, Debug)]
pub struct AttackAnimConfig {
    pub animnum: u16,
    /// Degrees here; `BADDTOR` at use.
    pub maxup: f32,
    pub maxdown: f32,
    pub maxleft: f32,
    pub maxright: f32,
    pub freearmfracup: f32,
    pub freearmfracdown: f32,
}

const fn cfg(animnum: u16, up: f32, down: f32, left: f32, right: f32, fu: f32, fd: f32) -> AttackAnimConfig {
    AttackAnimConfig { animnum, maxup: up, maxdown: down, maxleft: left, maxright: right, freearmfracup: fu, freearmfracdown: fd }
}

/// `var80065be0[0]` (`chraction.c:980`): pistol stand.
pub const CFG_STAND_PISTOL: AttackAnimConfig = cfg(ANIM_0041, 50.0, -40.0, 40.0, -40.0, 0.0, 0.0);
/// `var800656c0[0]` (`chraction.c:912`): heavy stand.
pub const CFG_STAND_HEAVY: AttackAnimConfig = cfg(ANIM_0002, 50.0, -30.0, 60.0, -20.0, 1.6, 1.8);
/// `var800663d8[0]` (`chraction.c:1063`): dual stand.
pub const CFG_STAND_DUAL: AttackAnimConfig = cfg(ANIM_007A, 50.0, -40.0, 40.0, -40.0, 0.0, 0.0);
/// `g_WalkAttackAnims[0..6]` (`chraction.c:1299`).
pub const CFG_WALK_HEAVY: AttackAnimConfig = cfg(ANIM_0030, 50.0, -30.0, 30.0, -30.0, 1.4, 1.3);
pub const CFG_RUN_HEAVY: AttackAnimConfig = cfg(ANIM_0031, 50.0, -30.0, 30.0, -30.0, 1.1, 1.2);
pub const CFG_WALK_PISTOL: AttackAnimConfig = cfg(ANIM_0052, 50.0, -30.0, 30.0, -30.0, 0.0, 0.0);
pub const CFG_RUN_PISTOL: AttackAnimConfig = cfg(ANIM_0055, 50.0, -30.0, 30.0, -30.0, 0.0, 0.0);
pub const CFG_WALK_DUAL: AttackAnimConfig = cfg(ANIM_006C, 50.0, -30.0, 30.0, -30.0, 0.0, 0.0);
pub const CFG_RUN_DUAL: AttackAnimConfig = cfg(ANIM_006E, 50.0, -30.0, 30.0, -30.0, 0.0, 0.0);
/// The crouch rows' configs (`player.c:4140-4145`).
pub const CFG_DUCK_PISTOL: AttackAnimConfig = cfg(ANIM_0281, 20.0, -90.0, 90.0, -90.0, 0.0, 0.0);
pub const CFG_SQUAT_PISTOL: AttackAnimConfig = cfg(ANIM_0285, 20.0, -90.0, 90.0, -90.0, 0.0, 0.0);
pub const CFG_DUCK_HEAVY: AttackAnimConfig = cfg(ANIM_0282, 20.0, -90.0, 90.0, -90.0, 1.6, 1.6);
pub const CFG_SQUAT_HEAVY: AttackAnimConfig = cfg(ANIM_0286, 10.0, -90.0, 90.0, -90.0, 1.6, 1.6);
pub const CFG_DUCK_DUAL: AttackAnimConfig = cfg(ANIM_0283, 20.0, -90.0, 90.0, -90.0, 0.0, 0.0);
pub const CFG_SQUAT_DUAL: AttackAnimConfig = cfg(ANIM_0287, 10.0, -90.0, 90.0, -90.0, 0.0, 0.0);

impl AttackAnimConfig {
    pub fn maxup_rad(&self) -> f32 {
        baddtor(self.maxup)
    }
    pub fn maxdown_rad(&self) -> f32 {
        baddtor(self.maxdown)
    }
}

/// `WIELDMODE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WieldMode {
    Pistol = 0,
    Heavy = 1,
    Unarmed = 2,
    DualGuns = 3,
}

/// `TURNMODE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnMode {
    StandNoTurn = 0,
    StandSoftTurn = 1,
    StandHardTurn = 2,
    DuckNoTurn = 3,
    DuckTurn = 4,
    SquatNoTurn = 5,
    SquatTurn = 6,
}

/// `bot_guess_crouch_pos` (`bot.c:748`): `CROUCHPOS_*` from the chr's height.
pub fn bot_guess_crouch_pos(height: f32) -> i32 {
    if height <= 90.0 {
        CROUCHPOS_SQUAT
    } else if height <= 135.0 {
        CROUCHPOS_DUCK
    } else {
        pd_core::ids::CROUCHPOS_STAND
    }
}

/// One `struct var80070ba4` row.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub animcfg: Option<&'static AttackAnimConfig>,
    pub animnum: u16,
    pub speed: f32,
    pub startframe: f32,
    pub endframe: f32,
    /// `unk14`: the leg-twist limit, degrees here.
    pub unk14: f32,
}

const fn row(animcfg: Option<&'static AttackAnimConfig>, animnum: u16, speed: f32, startframe: f32, endframe: f32, unk14: f32) -> Row {
    Row { animcfg, animnum, speed, startframe, endframe, unk14 }
}

/// `var80070ba4[wieldmode][turnmode]` (`player.c:4155`).
pub const ROWS: [[Row; 7]; 4] = [
    [
        row(Some(&CFG_STAND_PISTOL), 0, 0.1, 79.0, 87.0, 60.0),
        row(Some(&CFG_WALK_PISTOL), 0, 0.5, -1.0, -1.0, 60.0),
        row(Some(&CFG_RUN_PISTOL), 0, 0.5, -1.0, -1.0, 60.0),
        row(Some(&CFG_DUCK_PISTOL), 0, 0.001, 0.0, 0.1, 60.0),
        row(Some(&CFG_DUCK_PISTOL), 0, 0.503, -1.0, -1.0, 60.0),
        row(Some(&CFG_SQUAT_PISTOL), 0, 0.001, 0.0, 0.1, 30.0),
        row(Some(&CFG_SQUAT_PISTOL), 0, 0.45, -1.0, -1.0, 30.0),
    ],
    [
        row(Some(&CFG_STAND_HEAVY), 0, 0.05, 35.0, 40.0, 60.0),
        row(Some(&CFG_WALK_HEAVY), 0, 0.5, -1.0, -1.0, 60.0),
        row(Some(&CFG_RUN_HEAVY), 0, 0.5, -1.0, -1.0, 60.0),
        row(Some(&CFG_DUCK_HEAVY), 0, 0.001, 0.0, 0.1, 60.0),
        row(Some(&CFG_DUCK_HEAVY), 0, 0.503, -1.0, -1.0, 60.0),
        row(Some(&CFG_SQUAT_HEAVY), 0, 0.001, 0.0, 0.1, 30.0),
        row(Some(&CFG_SQUAT_HEAVY), 0, 0.45, -1.0, -1.0, 30.0),
    ],
    [
        row(None, ANIM_006A, 0.25, 0.0, -1.0, 60.0),
        row(None, ANIM_006B, 0.5, -1.0, -1.0, 60.0),
        row(None, ANIM_RUNNING_ONEHANDGUN, 0.5, -1.0, -1.0, 60.0),
        row(None, ANIM_0280, 0.001, 0.0, 0.1, 60.0),
        row(None, ANIM_0280, 0.503, -1.0, -1.0, 60.0),
        row(None, ANIM_0284, 0.001, 0.0, 0.1, 30.0),
        row(None, ANIM_0284, 0.45, -1.0, -1.0, 30.0),
    ],
    [
        row(Some(&CFG_STAND_DUAL), 0, 0.1, 32.0, 42.0, 60.0),
        row(Some(&CFG_WALK_DUAL), 0, 0.5, -1.0, -1.0, 60.0),
        row(Some(&CFG_RUN_DUAL), 0, 0.5, -1.0, -1.0, 60.0),
        row(Some(&CFG_DUCK_DUAL), 0, 0.001, 0.0, 0.1, 60.0),
        row(Some(&CFG_DUCK_DUAL), 0, 0.503, -1.0, -1.0, 60.0),
        row(Some(&CFG_SQUAT_DUAL), 0, 0.001, 0.0, 0.1, 30.0),
        row(Some(&CFG_SQUAT_DUAL), 0, 0.45, -1.0, -1.0, 30.0),
    ],
];

/// What the chooser decided (the debug views).
#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub wieldmode: WieldMode,
    pub turnmode: TurnMode,
    /// The leg angle before slewing (radians).
    pub angle: f32,
    pub speed: f32,
    pub reconfigured: bool,
}

/// `player_choose_third_person_animation` for a living chr on foot. Sets the
/// animation and `angleoffset`, and returns the row's `attackanimconfig`
/// (`*animcfgptr`). `soft` is the player's `headanim == HEADANIM_RESTING`
/// (always false for a simulant).
#[allow(clippy::too_many_arguments)]
pub fn choose(
    anim: &mut Anim,
    ctx: &mut AnimCtx,
    crouchpos: i32,
    wieldmode: WieldMode,
    speedsideways: f32,
    speedforwards: f32,
    speedtheta: f32,
    angleoffset: &mut f32,
    lvupdate60freal: f32,
    soft: bool,
) -> (Option<&'static AttackAnimConfig>, Choice) {
    let prevanimnum = anim.animnum;
    let mut turnspeed = (speedsideways * speedsideways + speedforwards * speedforwards).sqrt();
    let speedtheta = speedtheta.abs();
    if turnspeed < speedtheta {
        turnspeed = speedtheta;
    }
    let (row, mut speed, angle, turnmode);
    if turnspeed < 0.05 {
        turnmode = if crouchpos == CROUCHPOS_SQUAT {
            TurnMode::SquatNoTurn
        } else if crouchpos == CROUCHPOS_DUCK {
            TurnMode::DuckNoTurn
        } else {
            TurnMode::StandNoTurn
        };
        row = &ROWS[wieldmode as usize][turnmode as usize];
        speed = 1.0;
        angle = 0.0f32;
    } else {
        let mut a = pd_core::math::atan2f(speedsideways, speedforwards);
        if a >= baddtor(180.0) {
            a -= baddtor(360.0);
        }
        if crouchpos == CROUCHPOS_SQUAT {
            turnmode = TurnMode::SquatTurn;
            speed = (turnspeed * 2.857_142_925_262_5).min(1.2);
        } else if crouchpos == CROUCHPOS_DUCK {
            turnmode = TurnMode::DuckTurn;
            speed = (turnspeed * 2.0).min(1.2);
        } else if turnspeed < 0.4 || soft {
            turnmode = TurnMode::StandSoftTurn;
            speed = (2.0 * turnspeed).min(1.2);
        } else {
            turnmode = TurnMode::StandHardTurn;
            speed = turnspeed.min(1.2);
        }
        // Moving more than ~93.6° away from the facing: play the clip backwards
        // and fold the leg angle into the front half.
        if a < -1.633_368_015_289_3 {
            a += baddtor(180.0);
            speed = -speed;
        } else if a > 1.633_368_015_289_3 {
            a -= baddtor(180.0);
            speed = -speed;
        }
        row = &ROWS[wieldmode as usize][turnmode as usize];
        let limit = baddtor(row.unk14);
        angle = a.clamp(-limit, limit);
    }

    let limit = lvupdate60freal * (baddtor(360.0) / 60.0);
    if angle - *angleoffset > limit {
        *angleoffset += limit;
    } else if angle - *angleoffset < -limit {
        *angleoffset -= limit;
    } else {
        *angleoffset = angle;
    }

    let animcfg = row.animcfg;
    let animnum = if row.animnum != 0 { row.animnum } else { animcfg.map_or(0, |c| c.animnum) };
    speed *= row.speed;
    let (startframe, endframe) = (row.startframe, row.endframe);

    let mut reconfigure = animnum != prevanimnum;
    if startframe >= 0.0 && (!anim.looping || startframe != anim.loopframe) {
        reconfigure = true;
    }
    if startframe < 0.0 && anim.looping {
        reconfigure = true;
    }
    if reconfigure {
        // A merge in flight blocks a new animation (`animnum2 == 0`): the new
        // row takes effect once the current 16-tick merge has finished.
        if anim.animnum2 == 0 {
            anim.set_animation(ctx, animnum, false, if startframe >= 0.0 { startframe } else { 0.0 }, speed, 16.0);
            if startframe >= 0.0 {
                anim.set_looping(startframe, 16.0);
            }
            if endframe >= 0.0 {
                anim.set_end_frame(ctx.bank, endframe);
            }
        }
    } else if speed != anim.speed {
        anim.set_speed(speed, 1.0);
    }
    (animcfg, Choice { wieldmode, turnmode, angle, speed, reconfigured: reconfigure })
}

/// The `chr_is_dead` branch of `player_choose_third_person_animation`
/// (`player.c:5503`): keep a death animation already playing, else one of
/// `g_DeathAnimations` by `random() % g_NumDeathAnimations` (drawn only
/// then); speed 0.5, merge 16, not looping.
pub fn choose_death(anim: &mut Anim, ctx: &mut AnimCtx, rng: &mut pd_core::rng::Rng) {
    let prev = anim.animnum;
    let animnum = if DEATH_ANIMS.contains(&prev) { prev } else { DEATH_ANIMS[(rng.random() % DEATH_ANIMS.len() as u32) as usize] };
    let speed = 0.5;
    let mut reconfigure = animnum != prev;
    // startframe = -1 and the anim loops: reconfigure.
    if anim.looping {
        reconfigure = true;
    }
    if reconfigure {
        if anim.animnum2 == 0 {
            anim.set_animation(ctx, animnum, false, 0.0, speed, 16.0);
        }
    } else if speed != anim.speed {
        anim.set_speed(speed, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::res;

    fn ctx_for(bank: &pd_core::anim::AnimBank) -> AnimCtx<'_> {
        AnimCtx { bank, skel: pd_core::model::SKEL_CHR, scale: 0.1, chrinfo: None, merging_enabled: true }
    }

    #[test]
    fn a_bot_running_forwards_plays_the_run_row_at_half_speed() {
        let r = res();
        let mut ctx = ctx_for(&r.bank);
        let mut a = Anim::default();
        let mut off = 0.0;
        let (_, c) = choose(&mut a, &mut ctx, pd_core::ids::CROUCHPOS_STAND, WieldMode::Heavy, 0.0, 1.0, 0.0, &mut off, 1.0, false);
        assert_eq!(c.turnmode, TurnMode::StandHardTurn);
        assert_eq!(a.animnum, ANIM_0031);
        assert!((a.speed - 0.5).abs() < 1e-6);
    }

    #[test]
    fn backing_away_plays_the_run_backwards() {
        let r = res();
        let mut ctx = ctx_for(&r.bank);
        let mut a = Anim::default();
        let mut off = 0.0;
        let (_, c) = choose(&mut a, &mut ctx, pd_core::ids::CROUCHPOS_STAND, WieldMode::Pistol, 0.0, -1.0, 0.0, &mut off, 1.0, false);
        assert!(c.speed < 0.0);
        assert_eq!(a.animnum, ANIM_0055);
        assert!(a.speed < 0.0);
    }

    #[test]
    fn strafing_twists_the_legs_at_six_degrees_a_tick_up_to_sixty() {
        let r = res();
        let mut ctx = ctx_for(&r.bank);
        let mut a = Anim::default();
        let mut off = 0.0;
        for i in 1..=15 {
            choose(&mut a, &mut ctx, pd_core::ids::CROUCHPOS_STAND, WieldMode::Heavy, 1.0, 0.0, 0.0, &mut off, 1.0, false);
            let expect = (baddtor(6.0) * i as f32).min(baddtor(60.0));
            assert!((off - expect).abs() < 1e-4, "tick {i}: {off} vs {expect}");
        }
    }

    #[test]
    fn a_squatting_bot_walks_the_squat_row_and_holds_frame_zero_when_still() {
        let r = res();
        let mut ctx = ctx_for(&r.bank);
        let mut a = Anim::default();
        let mut off = 0.0;
        let (cfg, c) = choose(&mut a, &mut ctx, CROUCHPOS_SQUAT, WieldMode::Heavy, 0.0, 1.0, 0.0, &mut off, 1.0, false);
        assert_eq!(c.turnmode, TurnMode::SquatTurn);
        assert_eq!(a.animnum, ANIM_0286);
        // turnspeed 1 × 2.857, capped at 1.2, × the row's 0.45.
        assert!((a.speed - 0.54).abs() < 1e-5, "{}", a.speed);
        assert_eq!(cfg.unwrap().maxup, 10.0);
        for _ in 0..20 {
            choose(&mut a, &mut ctx, CROUCHPOS_SQUAT, WieldMode::Heavy, 1.0, 0.0, 0.0, &mut off, 1.0, false);
        }
        assert!((off - baddtor(30.0)).abs() < 1e-4, "{off}");
        let mut still = Anim::default();
        let mut off = 0.0;
        choose(&mut still, &mut ctx, CROUCHPOS_DUCK, WieldMode::Unarmed, 0.0, 0.0, 0.0, &mut off, 1.0, false);
        assert_eq!(still.animnum, ANIM_0280);
        assert!(still.looping && still.loopframe == 0.0);
        assert_eq!(bot_guess_crouch_pos(90.0), CROUCHPOS_SQUAT);
        assert_eq!(bot_guess_crouch_pos(135.0), CROUCHPOS_DUCK);
        assert_eq!(bot_guess_crouch_pos(185.0), pd_core::ids::CROUCHPOS_STAND);
    }

    #[test]
    fn standing_still_loops_the_idle_window() {
        let r = res();
        let mut ctx = ctx_for(&r.bank);
        let mut a = Anim::default();
        let mut off = 0.0;
        choose(&mut a, &mut ctx, pd_core::ids::CROUCHPOS_STAND, WieldMode::Heavy, 0.0, 0.0, 0.0, &mut off, 1.0, false);
        assert_eq!(a.animnum, ANIM_0002);
        assert!(a.looping);
        assert_eq!(a.loopframe, 35.0);
        assert_eq!(a.endframe, 40.0);
        assert!((a.speed - 0.05).abs() < 1e-6);
        // Loops inside its window for 20 s at 60 Hz.
        for _ in 0..60 * 20 {
            a.tick_quarter(&mut ctx, 4, true);
            assert!(a.frame >= 35.0 - 1e-3 && a.frame <= 40.0 + 1e-3, "{}", a.frame);
        }
    }
}
