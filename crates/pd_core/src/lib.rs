//! Perfect Dark's foundations, shared by the simulation, the menus and the renderer.
//!
//! One of everything: one RNG, one frame-timing struct, one `struct anim`, one model
//! walker, one `text.c`. The spikes each carried their own copy of most of these;
//! this crate is where they are merged.

// PD's constants are kept digit for digit (M_BADPI, guAlignF's pi, the
// quaternion thresholds), and ported functions keep the C's shape (index loops,
// argument lists).
#![allow(clippy::excessive_precision, clippy::approx_constant, clippy::too_many_arguments, clippy::needless_range_loop, clippy::collapsible_if)]

pub mod anim;
pub mod events;
pub mod assets;
pub mod ids;
pub mod lang;
pub mod lv;
pub mod math;
pub mod menugfx;
pub mod model;
pub mod mp;
pub mod mpweapons;
pub mod music;
pub mod pak;
pub mod rng;
pub mod savebuffer;
pub mod text;
