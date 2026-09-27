//! Perfect Dark's foundations, shared by the simulation, the menus and the renderer.
//!
//! One of everything: one RNG, one frame-timing struct, one `struct anim`, one model
//! walker, one `text.c`. The spikes each carried their own copy of most of these;
//! this crate is where they are merged.

pub mod anim;
pub mod assets;
pub mod ids;
pub mod lang;
pub mod lv;
pub mod math;
pub mod model;
pub mod mp;
pub mod rng;
pub mod text;
