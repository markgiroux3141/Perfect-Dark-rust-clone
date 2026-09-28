//! How the Nintendo 64 draws, sounds and reads its controllers, as far as Perfect
//! Dark depends on it.
//!
//! The CPU rasteriser in [`rdp`] is the reference implementation. PD's menus render
//! with it, and the snapshot tests use it as the oracle. The `gpu` feature ports
//! the same combiner, blender and filtering to WGSL for the 3D game view, and tests
//! pin the two against each other.

// The rasteriser keeps the reference implementation's shape (per-channel index loops).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod audio;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod pad;
pub mod rdp;
pub mod rsp;
