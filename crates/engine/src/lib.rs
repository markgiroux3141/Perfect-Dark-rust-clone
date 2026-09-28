//! The engine: everything a game needs from the machine, and nothing about any game.
//!
//! The engine owns the winit event loop ([`app`]) and calls into a game at a fixed
//! tick rate plus once per rendered frame. It provides GPU plumbing ([`gpu`]) but no
//! renderer: pipelines, materials and draw order belong to the game. That is the
//! lesson of the old engine, whose 4,400-line `Renderer` god object the spikes had
//! to route around with a pass hook.
//!
//! Rules:
//! - no game or console vocabulary in this crate (`tools/check_boundaries.py`
//!   checks the source);
//! - no depending on other workspace crates;
//! - no compile-time asset paths (`env!("CARGO_MANIFEST_DIR")`): assets are found
//!   at run time through [`assets`].

pub mod app;
pub mod assets;
pub mod audio;
pub mod clock;
pub mod debug_ui;
pub mod gpu;
pub mod input;

// The GPU and UI crates the game builds on, at the versions the engine uses.
pub use egui;
pub use wgpu;
