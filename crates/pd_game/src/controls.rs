//! Devices to N64 controllers: the keyboard (the menu spike's key table), USB N64
//! pads by raw code (`n64::pad::USB_ADAPTER_RAW`) with the D-pad as buttons or a
//! hat, and the stick through `n64::pad::stick_from_unit`. PD control styles
//! (1.1-1.4, 2.1-2.4) are applied in the sim, as PD does; this module only builds
//! controller state. Mouse aim arrives with the match (M3).
//!
//! Keyboard (it drives controller [`Controls::kb_player`]): arrows / WASD D-pad ·
//! Enter A · Esc B · Space START · Z Z · Q / E L / R · Backspace the name
//! keyboard's delete. F2-F4 press START on controllers 2-4, so a second player
//! can join without a second pad.
//!
//! Gamepad `k` (in connection order) is controller `k`.
//!
//! Source: the old repo's `pd_menu/app.rs` `poll_input`.

use engine::input::{Input, KeyCode, PadAxis, PadButton};
use n64::pad::*;

/// One controller as read this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub buttons: u16,
    pub stick: (i8, i8),
    pub connected: bool,
}

const KEYS: [(&[KeyCode], u16); 10] = [
    (&[KeyCode::ArrowUp, KeyCode::KeyW], U_JPAD),
    (&[KeyCode::ArrowDown, KeyCode::KeyS], D_JPAD),
    (&[KeyCode::ArrowLeft, KeyCode::KeyA], L_JPAD),
    (&[KeyCode::ArrowRight, KeyCode::KeyD], R_JPAD),
    (&[KeyCode::Enter, KeyCode::NumpadEnter], A_BUTTON),
    (&[KeyCode::Escape], B_BUTTON),
    (&[KeyCode::Space], START_BUTTON),
    (&[KeyCode::KeyZ], Z_TRIG),
    (&[KeyCode::KeyQ], L_TRIG),
    (&[KeyCode::KeyE], R_TRIG),
];

pub struct Controls {
    /// Which controller the keyboard drives (0-3).
    pub kb_player: usize,
    /// START taps waiting for the next tick (F2-F4, or the debug panel).
    pub start_taps: [bool; MAX_PADS],
}

impl Controls {
    pub fn new() -> Controls {
        Controls { kb_player: 0, start_taps: [false; MAX_PADS] }
    }

    /// This tick's four controllers and the keyboard delete. `joined` marks
    /// controllers whose player is in the game, which stay connected.
    pub fn read(&mut self, input: &Input, joined: [bool; MAX_PADS]) -> ([Reading; MAX_PADS], [bool; MAX_PADS]) {
        for (key, pad) in [(KeyCode::F2, 1), (KeyCode::F3, 2), (KeyCode::F4, 3)] {
            if input.key_pressed(key) {
                self.start_taps[pad] = true;
            }
        }
        let mut kb = 0u16;
        for (keys, bit) in KEYS {
            if keys.iter().any(|&k| input.key_down(k)) {
                kb |= bit;
            }
        }
        let mut out = [Reading::default(); MAX_PADS];
        for (i, r) in out.iter_mut().enumerate() {
            if let Some(p) = input.pads.as_ref().and_then(|p| p.pad(i)) {
                r.connected = true;
                r.stick = stick_from_unit(p.axis(PadAxis::LeftStickX), p.axis(PadAxis::LeftStickY));
                for (code, bit) in USB_ADAPTER_RAW {
                    if p.pressed_raw(code) {
                        r.buttons |= bit;
                    }
                }
                for (b, bit) in [(PadButton::DPadUp, U_JPAD), (PadButton::DPadDown, D_JPAD), (PadButton::DPadLeft, L_JPAD), (PadButton::DPadRight, R_JPAD)] {
                    if p.pressed(b) {
                        r.buttons |= bit;
                    }
                }
                let (dx, dy) = (p.axis(PadAxis::DPadX), p.axis(PadAxis::DPadY));
                for (on, bit) in [(dy > 0.5, U_JPAD), (dy < -0.5, D_JPAD), (dx < -0.5, L_JPAD), (dx > 0.5, R_JPAD)] {
                    if on {
                        r.buttons |= bit;
                    }
                }
            }
            if i == self.kb_player {
                r.buttons |= kb;
            }
            if std::mem::take(&mut self.start_taps[i]) {
                r.buttons |= START_BUTTON;
            }
            if r.buttons != 0 || i == 0 || i == self.kb_player || joined[i] {
                r.connected = true;
            }
        }
        let mut back2 = [false; MAX_PADS];
        back2[self.kb_player] = input.key_pressed(KeyCode::Backspace);
        (out, back2)
    }
}
