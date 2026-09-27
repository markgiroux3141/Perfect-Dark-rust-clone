//! Perfect Dark's menus: the dialog engine, the menu items, every Combat Simulator
//! dialog and handler, and MP state (configs, presets, locks, challenges).
//!
//! The API is small. Tick with up to four N64 controllers, render into an RGBA
//! framebuffer (320x220 PD pixels, with alpha, so the pause menu can go over the game
//! view), and receive an outcome such as "start this `MatchSetup`" instead of a text
//! summary. Sound requests use the same event type the world uses.
//!
//! Source: `pd_menu/` in the old repo. Its `Pd` god object splits into menu state,
//! MP state and render state here.

pub mod dialogs;
pub mod gfx;
pub mod handlers;
pub mod item;
pub mod menu;
pub mod model;
pub mod mpstate;
