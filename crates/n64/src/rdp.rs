//! The RDP in software: colour combiner (one- and two-cycle), blender (including
//! `XLU` and fog), texture formats and TLUTs, tile shift/clamp/mirror, the
//! top-left fill rule, the 3-point bilinear filter and TRILERP between mip levels,
//! and 16-bit RGBA5551 framebuffer storage with dither.
//!
//! Sources, to be merged into one: `pd_menu/gfx.rs` (2D RDP), `pd_menu/pdmodel.rs`
//! (`raster`, `combine`: the CPU copy of `pdgun.wgsl`), `pd_guns/font.rs` `Canvas`
//! and `pd_guns/app.rs` `shade_tri`.
