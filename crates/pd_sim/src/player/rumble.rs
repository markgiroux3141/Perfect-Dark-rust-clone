//! The Rumble Pak (`pak.c:4920` `pak_rumble`, `joy.c:1080` `joys_tick_rumble`):
//! each player's controller has a motor, run for a while (`rumblettl`, 60 ×
//! seconds, only ever raised) steadily or in pulses (`rumblepulsestopat` on
//! of every `rumblepulselen`); ticked once a frame from the main loop. Firing
//! pulses it for a fifth of a second (`bgun_rumble`), taking a hit runs it for
//! a quarter (`chr_damage`); a player's death and the pause stop it, a new
//! life and the unpause let it run again.
//!
//! The game drives the pad's motor from [`World::rumble_motor`]; a pad with
//! no force feedback (or the keyboard) has none.
//!
//! `// SUBST:` PD rumbles only a controller with a Rumble Pak in it and
//! checks the pak each time (`PAKSTATE_READY`, `PAKTYPE_RUMBLE`) / every
//! player's controller counts as having one; one controller per player
//! (control styles 1.x; the two-controller 2.x styles aren't offered).

use crate::world::World;

/// `RUMBLESTATE_*` (`constants.h:3629`).
pub const RUMBLESTATE_1: u8 = 1;
pub const RUMBLESTATE_ENABLED_STOPPED: u8 = 2;
pub const RUMBLESTATE_ENABLED_STARTING: u8 = 3;
pub const RUMBLESTATE_ENABLED_RUMBLING: u8 = 4;
pub const RUMBLESTATE_ENABLED_STOPPING: u8 = 5;
pub const RUMBLESTATE_DISABLED_STOPPING: u8 = 6;
pub const RUMBLESTATE_DISABLED_STOPPED: u8 = 7;
pub const RUMBLESTATE_ENABLING: u8 = 8;

/// A controller's rumble fields of `g_Paks[]`, and its motor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rumble {
    pub rumblestate: u8,
    pub rumblettl: i32,
    pub rumblepulsestopat: i32,
    pub rumblepulselen: i32,
    pub rumblepulsetimer: i32,
    /// `osMotorStart` / `osMotorStop`.
    pub motor: bool,
}

impl Default for Rumble {
    /// A pak just found (`pak.c:2874`): `RUMBLESTATE_1`, no time left.
    fn default() -> Self {
        Rumble { rumblestate: RUMBLESTATE_1, rumblettl: -1, rumblepulsestopat: 0, rumblepulselen: 0, rumblepulsetimer: 0, motor: false }
    }
}

impl Rumble {
    /// `pak_rumble` (`pak.c:4920`): rumble for `numsecs` unless it is off or
    /// already rumbling longer; `onduration` ticks on of every
    /// `onduration + offduration` (-1: steadily).
    pub fn pak_rumble(&mut self, numsecs: f32, onduration: i32, offduration: i32) {
        if self.rumblestate != RUMBLESTATE_DISABLED_STOPPING && self.rumblestate != RUMBLESTATE_DISABLED_STOPPED && (self.rumblettl as f32) < 60.0 * numsecs {
            self.rumblestate = RUMBLESTATE_ENABLED_STARTING;
            self.rumblettl = (60.0 * numsecs) as i32;
            self.rumblepulsestopat = onduration;
            self.rumblepulselen = onduration + offduration;
            self.rumblepulsetimer = 0;
        }
    }

    /// `pak_disable_rumble_for_player`: stop now (`joy_stop_rumble`), and
    /// no more until re-enabled.
    pub fn disable(&mut self) {
        self.rumblestate = RUMBLESTATE_DISABLED_STOPPING;
        self.joy_stop_rumble();
    }

    /// `pak_enable_rumble_for_player`: a stopped-off motor may run again.
    pub fn enable(&mut self) {
        if self.rumblestate == RUMBLESTATE_DISABLED_STOPPED {
            self.rumblestate = RUMBLESTATE_ENABLING;
        }
    }

    /// `joy_stop_rumble` (`joy.c:1028`).
    fn joy_stop_rumble(&mut self) {
        self.motor = false;
        if self.rumblestate != RUMBLESTATE_DISABLED_STOPPING && self.rumblestate != RUMBLESTATE_DISABLED_STOPPED {
            self.rumblestate = RUMBLESTATE_ENABLED_STOPPING;
        }
        self.rumblettl = -1;
    }

    /// `joys_tick_rumble` (`joy.c:1080`) for this controller.
    pub fn tick(&mut self) {
        match self.rumblestate {
            RUMBLESTATE_ENABLED_STARTING => {
                self.rumblestate = RUMBLESTATE_ENABLED_RUMBLING;
                self.motor = true;
            }
            RUMBLESTATE_ENABLED_RUMBLING => {
                if self.rumblepulsestopat != -1 {
                    if self.rumblepulsetimer == 0 {
                        self.motor = true;
                    } else if self.rumblepulsestopat == self.rumblepulsetimer {
                        self.motor = false;
                    }
                    self.rumblepulsetimer += 1;
                    if self.rumblepulselen == self.rumblepulsetimer {
                        self.rumblepulsetimer = 0;
                    }
                }
                self.rumblettl -= 1;
                if self.rumblettl < 0 {
                    self.rumblestate = RUMBLESTATE_ENABLED_STOPPING;
                }
            }
            RUMBLESTATE_ENABLED_STOPPING => {
                self.rumblestate = RUMBLESTATE_ENABLED_STOPPED;
                self.motor = false;
            }
            RUMBLESTATE_DISABLED_STOPPING => {
                self.motor = false;
                self.rumblestate = RUMBLESTATE_DISABLED_STOPPED;
            }
            RUMBLESTATE_ENABLING => {
                self.rumblestate = RUMBLESTATE_ENABLED_STOPPED;
                self.rumblettl = -1;
            }
            _ => {}
        }
    }
}

impl World {
    /// Whether player `pi`'s controller's motor runs this frame.
    pub fn rumble_motor(&self, pi: usize) -> bool {
        self.players.get(pi).is_some_and(|p| p.rumble.motor)
    }

    /// `lv_tick`'s rumble on a change of pause (`lv.c:2425`): every
    /// controller stops while paused and may run again after.
    pub(crate) fn lv_tick_rumble_pause(&mut self, paused: bool) {
        if paused != self.rumble_paused {
            self.rumble_paused = paused;
            for p in self.players.iter_mut() {
                if paused {
                    p.rumble.disable();
                } else {
                    p.rumble.enable();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shot's pulse: 2 ticks on, 4 off, for 12 ticks, then stopped; a
    /// shorter rumble doesn't cut a longer one short; disabled, none starts.
    #[test]
    fn the_motor_pulses_then_stops() {
        let mut r = Rumble::default();
        r.pak_rumble(0.2, 2, 4);
        let mut on = Vec::new();
        for _ in 0..16 {
            r.tick();
            on.push(r.motor);
        }
        // Starting, then pulses of 2 on / 4 off, then the stop.
        assert_eq!(on[..14], [true, true, true, false, false, false, false, true, true, false, false, false, false, true]);
        assert!(!on[15] && r.rumblestate == RUMBLESTATE_ENABLED_STOPPED);
        // A hit's steady quarter second, then a shot's fifth: still steady.
        r.pak_rumble(0.25, -1, -1);
        r.tick();
        r.pak_rumble(0.2, 2, 4);
        assert_eq!(r.rumblepulsestopat, -1, "the longer rumble stays");
        r.disable();
        r.tick();
        assert!(!r.motor && r.rumblestate == RUMBLESTATE_DISABLED_STOPPED);
        r.pak_rumble(0.2, 2, 4);
        assert_eq!(r.rumblestate, RUMBLESTATE_DISABLED_STOPPED, "no rumble while disabled");
        r.enable();
        r.tick();
        r.pak_rumble(0.2, 2, 4);
        r.tick();
        assert!(r.motor, "enabled again");
    }
}
