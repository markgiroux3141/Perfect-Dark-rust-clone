//! (feature `gpu`) WGSL ports: the N64 combiner/blender material pipeline
//! (`pdgun.wgsl`, `pdfx.wgsl`), the framebuffer effects (`pdpost.wgsl`) and the
//! VI + CRT chain (`n64video.wgsl`: RGBA5551 + Bayer store, VI dither filter,
//! divot, composite/S-Video encode, beam, mask, TV-set frames).
//!
//! Source: `pd_guns/render.rs`, `pd_guns/n64video.rs` and their `.wgsl` files.
