//! Devices to N64 controllers and to the match's per-player controls.
//!
//! **Menus** ([`Controls::read`]): the keyboard (the menu spike's key table)
//! and USB N64 pads by raw code (`n64::pad::USB_ADAPTER_RAW`), with the D-pad as
//! buttons or a hat and the stick through `n64::pad::stick_from_unit`.
//!
//! **A match** ([`Controls::read_match`]): gamepad `k` drives player `k` with
//! PD's control style 1.1 (applied in `pd_sim::player`, as PD does), and the
//! keyboard and mouse drive player [`Controls::kb_player`] the PC port's way
//! (the gun spike's key table):
//! WASD move · mouse look · RMB (hold) aim · LMB fire · E / MMB use ·
//! R reload · Q next gun · 1-0 pick a gun (by inventory slot) · Ctrl or C crouch down ·
//! Space crouch up · ↑/↓ zoom · Enter START (the pause menu) · Esc frees the mouse
//! (a click takes it back). The pause menu reads the controllers as the menus do.
//!
//! Keyboard (menus; it drives controller [`Controls::kb_player`]): arrows / WASD
//! D-pad · Enter A · Esc B · Space START · Z Z · Q / E L / R · Backspace the name
//! keyboard's delete. F2-F4 press START on controllers 2-4, so a second player
//! can join without a second pad.
//!
//! Source: the old repo's `pd_menu/app.rs` `poll_input` and `pd_guns/app.rs`
//! `input` / `merge_pad`.

use engine::input::{Input, KeyCode, MouseButton, PadAxis, PadButton, PadView};
use n64::pad::*;
use pd_sim::player::PlayerInput;

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

/// A USB N64 pad by raw code, plus a D-pad from buttons or a hat.
fn pad_reading(p: &PadView) -> Reading {
    let mut r = Reading { connected: true, stick: stick_from_unit(p.axis(PadAxis::LeftStickX), p.axis(PadAxis::LeftStickY)), buttons: 0 };
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
    r
}

/// An N64 pad as control style 1.1 reads it (`bondmove.c:1166`): the stick
/// walks and turns, Z fires, R aims, B is use, A cycles, the C-buttons strafe
/// and look.
fn pad_input(inp: &mut PlayerInput, r: Reading) {
    let b = |bit: u16| r.buttons & bit != 0;
    inp.pad = true;
    inp.aim |= b(R_TRIG);
    inp.fire |= b(Z_TRIG);
    inp.use_held |= b(B_BUTTON);
    inp.a_held = b(A_BUTTON);
    inp.look_x = r.stick.0 as i32;
    inp.look_y = r.stick.1 as i32;
    inp.c_up = b(U_CBUTTONS);
    inp.c_down = b(D_CBUTTONS);
    inp.c_left = b(L_CBUTTONS);
    inp.c_right = b(R_CBUTTONS);
    inp.start = b(START_BUTTON);
}

pub struct Controls {
    /// Which controller the keyboard drives (0-3).
    pub kb_player: usize,
    /// START taps waiting for the next tick (F2-F4, or the debug panel).
    pub start_taps: [bool; MAX_PADS],
    /// The mouse is captured for mouse look.
    pub captured: bool,
}

impl Controls {
    pub fn new() -> Controls {
        Controls { kb_player: 0, start_taps: [false; MAX_PADS], captured: false }
    }

    /// This tick's four controllers and the keyboard delete, for the menus.
    /// `joined` marks controllers whose player is in the game, which stay connected.
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
                *r = pad_reading(&p);
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

    /// This tick's controls for `n` players in a match. START is held (the
    /// sim takes its presses, `bmove_process_input`): a pad's, or Enter on the
    /// keyboard.
    pub fn read_match(&mut self, input: &Input, n: usize) -> Vec<PlayerInput> {
        let mut out = vec![PlayerInput::default(); n];
        for (i, inp) in out.iter_mut().enumerate() {
            let kb = i == self.kb_player;
            if let Some(p) = input.pads.as_ref().and_then(|p| p.pad(i)) {
                let r = pad_reading(&p);
                // The keyboard's player plays the PC way until its pad is touched.
                if !kb || r.buttons != 0 || r.stick != (0, 0) {
                    pad_input(inp, r);
                }
            }
            if kb {
                self.keyboard_input(input, inp);
                inp.start |= input.key_down(KeyCode::Enter) || input.key_down(KeyCode::NumpadEnter);
            }
        }
        out
    }

    /// The keyboard and mouse the PC port's way (`CONTROLMODE_PC`), merged into
    /// what a pad on the same controller set.
    fn keyboard_input(&self, input: &Input, inp: &mut PlayerInput) {
        let k = |c: KeyCode| input.key_down(c);
        inp.walk_y = (k(KeyCode::KeyW) as i32 - k(KeyCode::KeyS) as i32) * 127;
        inp.walk_x = (k(KeyCode::KeyD) as i32 - k(KeyCode::KeyA) as i32) * 127;
        if self.captured {
            let (dx, dy) = input.mouse_delta();
            inp.mouse_dx = dx as f32;
            inp.mouse_dy = dy as f32;
            inp.fire |= input.mouse_down(MouseButton::Left);
            inp.aim |= input.mouse_down(MouseButton::Right);
        }
        inp.use_held |= k(KeyCode::KeyE) || input.mouse_down(MouseButton::Middle);
        inp.reload = input.key_pressed(KeyCode::KeyR);
        inp.cycle_next = input.key_pressed(KeyCode::KeyQ);
        inp.crouch_down = input.key_pressed(KeyCode::ControlLeft) || input.key_pressed(KeyCode::ControlRight) || input.key_pressed(KeyCode::KeyC);
        inp.crouch_up = input.key_pressed(KeyCode::Space);
        inp.zoom_in = k(KeyCode::ArrowUp);
        inp.zoom_out = k(KeyCode::ArrowDown);
    }

    /// The inventory slot a number key picks this tick (1 is the first, 0 the
    /// tenth); the caller looks the weapon up in the player's inventory.
    pub fn slot_pressed(&self, input: &Input) -> Option<usize> {
        const DIGITS: [KeyCode; 10] = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
            KeyCode::Digit0,
        ];
        DIGITS.iter().position(|&k| input.key_pressed(k))
    }
}
