//! The N64 controller as PD reads it: a 16-bit button mask and a signed 8-bit
//! stick per controller, up to four, with PD's pressed/held edges
//! (`joy_get_buttons_pressed_this_frame` is `buttons & !prev`).
//!
//! Also the one raw-code map for the USB N64 adapter the user plays with, which
//! the spikes had copied into `pd_guns/app.rs` and `pd_menu/app.rs`. The game's
//! device layer (`pd_game::controls`) turns keyboard, mouse and gilrs input into
//! a [`Pad`]; nothing below it sees a device.

/// Button bits (`include/PR/os_cont.h`).
pub const A_BUTTON: u16 = 0x8000;
pub const B_BUTTON: u16 = 0x4000;
pub const Z_TRIG: u16 = 0x2000;
pub const START_BUTTON: u16 = 0x1000;
pub const U_JPAD: u16 = 0x0800;
pub const D_JPAD: u16 = 0x0400;
pub const L_JPAD: u16 = 0x0200;
pub const R_JPAD: u16 = 0x0100;
pub const L_TRIG: u16 = 0x0020;
pub const R_TRIG: u16 = 0x0010;
pub const U_CBUTTONS: u16 = 0x0008;
pub const D_CBUTTONS: u16 = 0x0004;
pub const L_CBUTTONS: u16 = 0x0002;
pub const R_CBUTTONS: u16 = 0x0001;

pub const MAX_PADS: usize = 4;

/// One controller's state this frame and last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u16,
    pub prev: u16,
    pub stick_x: i8,
    pub stick_y: i8,
    /// Whether a controller is plugged into this port.
    pub connected: bool,
}

impl Pad {
    /// Start a new frame with this frame's reading: last frame's buttons become `prev`.
    pub fn next_frame(&mut self, buttons: u16, stick_x: i8, stick_y: i8) {
        self.prev = self.buttons;
        self.buttons = buttons;
        self.stick_x = stick_x;
        self.stick_y = stick_y;
    }

    /// `joy_get_buttons`: held now, masked.
    pub fn held(&self, mask: u16) -> u16 {
        self.buttons & mask
    }

    /// `joy_get_buttons_pressed_this_frame`: down now, up last frame.
    pub fn pressed(&self, mask: u16) -> u16 {
        self.buttons & !self.prev & mask
    }

    /// Up now, down last frame.
    pub fn released(&self, mask: u16) -> u16 {
        !self.buttons & self.prev & mask
    }
}

/// The stick's reach PD sees from a real controller (about ±80 of the ±127 the
/// format allows).
pub const STICK_RANGE: f32 = 80.0;
/// The radial dead zone applied to an analogue stick before scaling.
pub const STICK_DEADZONE: f32 = 0.15;

/// A unit-square analogue stick (±1, y up) as N64 stick counts: a radial dead
/// zone, rescaled so the edge of the zone is 0, then ±[`STICK_RANGE`].
pub fn stick_from_unit(x: f32, y: f32) -> (i8, i8) {
    let mag = (x * x + y * y).sqrt();
    if mag < STICK_DEADZONE {
        return (0, 0);
    }
    let s = ((mag - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).min(1.0) / mag;
    ((x * s * STICK_RANGE).round() as i8, (y * s * STICK_RANGE).round() as i8)
}

/// The USB N64 adapter's raw button codes, as verified on the user's pad: gilrs's
/// semantic names mis-map this adapter, so its buttons are read by raw code. The
/// D-pad comes through as buttons or a hat, depending on the adapter.
pub const USB_ADAPTER_RAW: [(u32, u16); 10] = [
    (2, A_BUTTON),
    (1, B_BUTTON),
    (6, Z_TRIG),
    (12, START_BUTTON),
    (4, L_TRIG),
    (5, R_TRIG),
    (9, U_CBUTTONS),
    (3, D_CBUTTONS),
    (0, L_CBUTTONS),
    (8, R_CBUTTONS),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_is_the_rising_edge_only() {
        let mut p = Pad::default();
        p.next_frame(A_BUTTON, 0, 0);
        assert_eq!(p.pressed(A_BUTTON), A_BUTTON);
        p.next_frame(A_BUTTON | B_BUTTON, 0, 0);
        assert_eq!(p.pressed(0xffff), B_BUTTON);
        assert_eq!(p.held(0xffff), A_BUTTON | B_BUTTON);
        p.next_frame(B_BUTTON, 0, 0);
        assert_eq!(p.released(0xffff), A_BUTTON);
    }

    #[test]
    fn the_stick_has_a_dead_zone_and_reaches_eighty() {
        assert_eq!(stick_from_unit(0.1, 0.0), (0, 0));
        assert_eq!(stick_from_unit(1.0, 0.0), (80, 0));
        assert_eq!(stick_from_unit(0.0, -1.0), (0, -80));
    }

    #[test]
    fn every_button_has_one_raw_code() {
        let mut seen = 0u16;
        for (_, bit) in USB_ADAPTER_RAW {
            assert_eq!(seen & bit, 0);
            seen |= bit;
        }
        assert_eq!(seen.count_ones(), 10);
    }
}
