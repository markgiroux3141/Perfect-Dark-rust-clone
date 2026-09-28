//! Raw input, snapshotted once per tick: keys held and pressed this tick, mouse
//! buttons and accumulated motion, and gilrs gamepads (buttons, axes, raw codes for
//! USB adapters, hot-plug). No action mapping here: turning a device into a
//! game's controller or control style is the game's job.
//!
//! The runner feeds window events in between ticks; a tick reads [`Input`] and the
//! runner then calls [`Input::end_tick`], so "pressed" means "went down since the
//! last tick" even when several frames pass without one.
//!
//! Source: the old engine's `platform/input.rs` (held keys, mouse delta) and
//! `platform/gamepad.rs` (`Gamepads`, raw codes), which read one active pad; this
//! keeps every connected pad, in connection order, so several players can join.
//!
//! ## Windows and USB adapters
//! gilrs on Windows is XInput/WGI-backed. An adapter may expose its buttons under
//! gilrs's semantic names or only as raw codes. Set `GAMEPAD_DEBUG=1` to log every
//! button and axis with its gilrs name and native code, then encode the layout in
//! the game's binding table.

use std::collections::{HashMap, HashSet};

use gilrs::{EventType, GamepadId, Gilrs};

pub use gilrs::{Axis as PadAxis, Button as PadButton};
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// Keyboard and mouse, plus the pads.
#[derive(Default)]
pub struct Input {
    keys: HashSet<KeyCode>,
    pressed: HashSet<KeyCode>,
    mouse: HashSet<MouseButton>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_delta: (f64, f64),
    /// Gamepads, if the backend started.
    pub pads: Option<Gamepads>,
}

impl Input {
    pub fn new() -> Input {
        Input { pads: Gamepads::new(), ..Default::default() }
    }

    pub fn key_event(&mut self, key: KeyCode, down: bool) {
        if down {
            if self.keys.insert(key) {
                self.pressed.insert(key);
            }
        } else {
            self.keys.remove(&key);
        }
    }

    pub fn mouse_button_event(&mut self, b: MouseButton, down: bool) {
        if down {
            if self.mouse.insert(b) {
                self.mouse_pressed.insert(b);
            }
        } else {
            self.mouse.remove(&b);
        }
    }

    pub fn mouse_motion(&mut self, dx: f64, dy: f64) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    /// Focus lost: nothing is held any more.
    pub fn clear(&mut self) {
        self.keys.clear();
        self.mouse.clear();
    }

    pub fn key_down(&self, key: KeyCode) -> bool {
        self.keys.contains(&key)
    }

    /// Went down since the last tick.
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.pressed.contains(&key)
    }

    pub fn mouse_down(&self, b: MouseButton) -> bool {
        self.mouse.contains(&b)
    }

    pub fn mouse_pressed(&self, b: MouseButton) -> bool {
        self.mouse_pressed.contains(&b)
    }

    /// Mouse motion since the last tick, in device units.
    pub fn mouse_delta(&self) -> (f64, f64) {
        self.mouse_delta
    }

    /// Poll the pads (once per tick, before the game reads them).
    pub fn poll_pads(&mut self) {
        if let Some(p) = self.pads.as_mut() {
            p.poll();
        }
    }

    /// Forget this tick's edges and motion.
    pub fn end_tick(&mut self) {
        self.pressed.clear();
        self.mouse_pressed.clear();
        self.mouse_delta = (0.0, 0.0);
    }
}

struct PadState {
    id: GamepadId,
    /// Held state of every button by its raw native code.
    raw: HashMap<u32, bool>,
}

/// Every connected gamepad, in the order they connected.
pub struct Gamepads {
    gilrs: Gilrs,
    pads: Vec<PadState>,
    debug: bool,
}

/// One pad's state, as read after [`Gamepads::poll`].
pub struct PadView<'a> {
    gilrs: &'a Gilrs,
    state: &'a PadState,
}

impl PadView<'_> {
    pub fn name(&self) -> String {
        self.gilrs.gamepad(self.state.id).name().to_string()
    }

    /// Roughly −1..1 (gilrs applies a small dead zone of its own).
    pub fn axis(&self, axis: PadAxis) -> f32 {
        self.gilrs.gamepad(self.state.id).value(axis)
    }

    pub fn pressed(&self, button: PadButton) -> bool {
        self.gilrs.gamepad(self.state.id).is_pressed(button)
    }

    /// A button by its raw native code (for buttons gilrs cannot name).
    pub fn pressed_raw(&self, code: u32) -> bool {
        *self.state.raw.get(&code).unwrap_or(&false)
    }
}

impl Gamepads {
    /// `None` if the gamepad backend cannot start; the game then runs on the keyboard.
    pub fn new() -> Option<Gamepads> {
        let gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                log::warn!("gamepad: gilrs init failed ({e:?}); pads disabled");
                return None;
            }
        };
        let mut pads = Vec::new();
        for (id, gp) in gilrs.gamepads() {
            log::info!("gamepad: found \"{}\" (id {id})", gp.name());
            pads.push(PadState { id, raw: HashMap::new() });
        }
        Some(Gamepads { gilrs, pads, debug: std::env::var_os("GAMEPAD_DEBUG").is_some() })
    }

    /// Drain the event queue: hot-plug, raw button state, debug logging.
    pub fn poll(&mut self) {
        while let Some(ev) = self.gilrs.next_event() {
            let pos = self.pads.iter().position(|p| p.id == ev.id);
            match ev.event {
                EventType::Connected => {
                    log::info!("gamepad: connected \"{}\" (id {})", self.gilrs.gamepad(ev.id).name(), ev.id);
                    if pos.is_none() {
                        self.pads.push(PadState { id: ev.id, raw: HashMap::new() });
                    }
                }
                EventType::Disconnected => {
                    log::info!("gamepad: disconnected (id {})", ev.id);
                    if let Some(i) = pos {
                        self.pads.remove(i);
                    }
                }
                EventType::ButtonPressed(btn, code) => {
                    if self.debug {
                        log::info!("gamepad {}: BUTTON {btn:?} (code {})", ev.id, code.into_u32());
                    }
                    if let Some(i) = pos {
                        self.pads[i].raw.insert(code.into_u32(), true);
                    }
                }
                EventType::ButtonReleased(_, code) => {
                    if let Some(i) = pos {
                        self.pads[i].raw.insert(code.into_u32(), false);
                    }
                }
                EventType::ButtonChanged(_, val, code) => {
                    if let Some(i) = pos {
                        self.pads[i].raw.insert(code.into_u32(), val > 0.5);
                    }
                }
                EventType::AxisChanged(axis, val, code) if self.debug && val.abs() > 0.5 => {
                    log::info!("gamepad {}: AXIS {axis:?} = {val:+.2} (code {})", ev.id, code.into_u32());
                }
                _ => {}
            }
        }
    }

    pub fn count(&self) -> usize {
        self.pads.len()
    }

    /// The `i`th connected pad.
    pub fn pad(&self, i: usize) -> Option<PadView<'_>> {
        let state = self.pads.get(i)?;
        self.gilrs.gamepad(state.id).is_connected().then_some(PadView { gilrs: &self.gilrs, state })
    }
}
