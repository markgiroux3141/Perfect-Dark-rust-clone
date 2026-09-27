//! `perfect_dark`: the game.
//!
//! A small state machine over the engine runner: Boot -> Menus -> Match -> (pause
//! menu, end-of-match scores) -> Menus. It owns the glue and nothing else:
//! - `controls`: keyboard, mouse and gamepads to N64 controllers, plus the PC
//!   port's mouse aim;
//! - `audio`: `pd_sim`/`pd_menu` sound events to engine voices, with PD's pitch, and
//!   the optional TV-speaker chain;
//! - `states`: the menu, match and results states.

mod audio;
mod controls;
mod states;

fn main() {
    env_logger::init();
    log::info!("perfect_dark: skeleton (M0). See docs/MILESTONES.md.");
}
