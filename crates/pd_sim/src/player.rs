//! The human player: `bondmove` (look/turn/aim, including the PC port's mouse aim),
//! `bondwalk` (collide-and-slide, stepping, ramps, falls, landing dip, ladders,
//! crouch under ceilings), `bondhead` (bob), lean, zoom, health and damage feedback,
//! death and respawn. Input is a per-player `PlayerInput`: the N64 controller plus the
//! PC port's mouse extension. PD's control styles are applied here, as PD does.
//!
//! Sources: `pd_guns/player.rs`, `pd_complex/health.rs` (state half), `fight.rs`.
