//! Shared plumbing for the headless tools: finding the assets, writing PNGs, and
//! the snapshot targets `pd_snapshot` dispatches to. Later milestones add the
//! offscreen GPU setup and the scripted-input format (`wN`, button taps,
//! `shot:name`) the menu and match snapshots use.

use std::path::Path;

use pd_core::assets::AssetDir;

pub mod lab;
pub mod snapshot;

/// The asset root: `PD_ASSETS` if set, else the repo's `assets/`; with the
/// custom levels (`PD_CUSTOM`, else `custom/` beside it) when there are any.
pub fn assets() -> AssetDir {
    let a = match std::env::var_os("PD_ASSETS") {
        Some(p) => AssetDir::new(p),
        None => AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR")),
    };
    a.with_custom_levels()
}

/// Write tightly packed RGBA8 as a PNG, creating the directory.
pub fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    image::save_buffer(path, rgba, w as u32, h as u32, image::ColorType::Rgba8).map_err(|e| format!("{}: {e}", path.display()))
}
