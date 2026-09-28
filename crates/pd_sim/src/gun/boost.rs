//! The Combat Boost (`bondgun.c:10325`-`:10380`) and the world's looping
//! "misc" sounds (`lv.c:175`, `:205`): the boost's heartbeat and the Slayer
//! rocket's hum and beep.
//!
//! A pill buys ten seconds (slow motion on, it takes twenty away instead);
//! while `speedpillon` the whole world's frame is capped at 4 quarter-ticks
//! (`lv_tick`, [`pd_core::lv`]), which slows everything at 30 or 20 Hz and
//! nothing at 60, as in PD. The wipe in and out is `lv_render`'s
//! ([`crate::player::vision`]).
//!
//! Source: the old repo's `pd_guns/sim.rs` (`bgun_add_boost` .. `lv_update_misc_sfx`),
//! where the boost slowed only the player.

use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::lv::SlowMotion;

use crate::world::World;

/// `g_MiscSfxSounds` (`lv.c:145`), by `MISCSFX_*`.
pub const MISC_SFX_SOUNDS: [u16; 3] = [0x05c8, 0x8068, 0x01c8];
/// The sound handles the misc loops play on (`g_MiscSfxAudioHandles`).
pub const MISC_SFX_HANDLE: u32 = 0x200;

/// `g_Vars.speedpilltime` / `speedpillwant` / `speedpillon` / `speedpillchange`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpeedPill {
    /// Ticks of boost left.
    pub time: i32,
    pub want: bool,
    pub on: bool,
    /// The wipe's step, 0..30; `on` above 15.
    pub change: i32,
}

impl World {
    /// `bgun_add_boost` (`bondgun.c:10325`): at most five minutes; Jo speaks if
    /// the boost wasn't already wanted.
    pub fn bgun_add_boost(&mut self, amount: i32) {
        let sp = &mut self.speedpill;
        sp.time = (sp.time + amount).min(5 * 60 * 60);
        let first = !sp.want;
        sp.want = true;
        if first {
            let sound = if self.setup.slowmotion() != SlowMotion::Off { 0x02ad } else { 0x05c9 };
            self.sound(sound, 1.0);
        }
    }

    /// `bgun_subtract_boost` (`bondgun.c:10342`).
    pub fn bgun_subtract_boost(&mut self, amount: i32) {
        let sp = &mut self.speedpill;
        sp.time -= amount;
        if sp.time <= 0 {
            sp.time = 0;
            sp.want = false;
        }
    }

    /// `bgun_apply_boost` (`bondgun.c:10352`).
    pub(crate) fn bgun_apply_boost(&mut self) {
        if self.setup.slowmotion() != SlowMotion::Off {
            self.bgun_subtract_boost(1200);
        } else {
            self.bgun_add_boost(600);
        }
    }

    /// `bgun_revert_boost` (`bondgun.c:10361`).
    pub(crate) fn bgun_revert_boost(&mut self) {
        if self.setup.slowmotion() != SlowMotion::Off {
            self.bgun_add_boost(1200);
        } else {
            self.bgun_subtract_boost(600);
        }
    }

    /// `bgun_tick_boost` (`bondgun.c:10370`), from `lv_tick`.
    pub(crate) fn bgun_tick_boost(&mut self) {
        let lv60 = self.lv.lvupdate60;
        let sp = &mut self.speedpill;
        if sp.on && sp.time > 0 {
            sp.time -= lv60;
            if sp.time <= 0 {
                sp.time = 0;
                sp.want = false;
            }
        }
    }

    /// `lv_update_misc_sfx` (`lv.c:205`): the heartbeat while boosted with slow
    /// motion off, the rocket's hum and beep while anyone rides one; paused,
    /// all three stop.
    pub(crate) fn lv_update_misc_sfx(&mut self) {
        if self.lv.lvupdate240 == 0 {
            for ty in 0..3 {
                self.lv_set_misc_sfx_state(ty, false);
            }
            return;
        }
        let usingboost = self.speedpill.on && self.setup.slowmotion() == SlowMotion::Off;
        self.lv_set_misc_sfx_state(MISCSFX_BOOSTHEARTBEAT, usingboost);
        let usingrocket = self.players.iter().any(|p| p.visionmode == VISIONMODE_SLAYERROCKET);
        self.lv_set_misc_sfx_state(MISCSFX_SLAYERROCKETHUM, usingrocket);
        self.lv_set_misc_sfx_state(MISCSFX_SLAYERROCKETBEEP, usingrocket);
    }

    /// `lv_set_misc_sfx_state` (`lv.c:175`): start the loop once, stop it once.
    fn lv_set_misc_sfx_state(&mut self, ty: usize, play: bool) {
        if play == self.misc_sfx[ty] {
            return;
        }
        self.misc_sfx[ty] = play;
        let handle = MISC_SFX_HANDLE + ty as u32;
        if play {
            self.push_event(Event::HandleSound { handle, sound: MISC_SFX_SOUNDS[ty], pitch: 1.0, volume: 1.0, pan: 0.0 });
        } else {
            self.push_event(Event::StopSound { handle });
        }
    }
}
