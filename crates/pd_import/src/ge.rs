//! GoldenEye 007 levels. GoldenEye is Perfect Dark's ancestor in every part
//! PD's game runs on: rooms with portals, floor tiles, pads, doors (PD's
//! `struct doorobj` and `setup_create_door` are GE's `DoorRecord` and
//! `setupDoor`), the display-list dialect, the image format. So a GE level
//! needs none of the made-up structure another game's does (no k-d rooms, no
//! baked light, no step or ledge rewriting): `tools/ge-extract` converts it
//! from the ROM straight into PD's stage formats, through PD's own exporters
//! (`tools/pd-assets`), and writes into the stage directory:
//!
//! - `bg.json` + `bg.bin`: GE's rooms one for one, its portals, its display
//!   lists as PD's, its fog row; the images under `custom/ge/tex/`;
//! - `tiles.json`: GE's floor tiles, and the walls GE implies at every edge
//!   without a neighbour;
//! - the door models into `custom/models/` (`MODEL_*` 0x1000 + GE's prop
//!   number, `pd_core::assets::CUSTOM_MODELNUMS`);
//! - `ge.json`: what this reads back: the doors' pads and `setup.json` rows,
//!   the spots the setups mark (for [`crate::place`]), and its report.
//!
//! The level's missions (guards, objectives, keys, cameras, glass) are left
//! behind: the Combat Simulator takes its architecture.

use std::path::{Path, PathBuf};
use std::process::Command;

use glam::Vec3;
use serde::Deserialize;
use serde_json::Value;

use crate::recipe::{GeSource, Recipe};
use crate::source::{Marker, MarkerKind};
use crate::write::{Gameplay, PadRow};
use crate::Paths;

#[derive(Deserialize)]
struct Extract {
    format: String,
    pads: Vec<ExtractPad>,
    props: Vec<Value>,
    markers: Vec<ExtractMarker>,
    report: Vec<String>,
}

#[derive(Deserialize)]
struct ExtractPad {
    pos: [f32; 3],
    look: [f32; 3],
    up: [f32; 3],
    flags: u32,
    bbox: [f32; 6],
}

#[derive(Deserialize)]
struct ExtractMarker {
    kind: String,
    pos: [f32; 3],
    facing: f32,
}

/// The repository root: the extractor and the decomp are found from it.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Run `tools/ge-extract` into `dir` (`custom/stages/<code>/`) and read back
/// its hand-off: the source's own gameplay data (the doors and their pads),
/// the spots it marks, and its report.
pub fn extract(r: &Recipe, g: &GeSource, paths: &Paths, dir: &Path) -> Result<(Gameplay, Vec<Marker>, Vec<String>), String> {
    let repo = repo();
    let rom = paths.src.clone().unwrap_or_else(|| PathBuf::from(&g.rom));
    let python = std::env::var_os("PYTHON").unwrap_or_else(|| "python".into());
    let mut cmd = Command::new(&python);
    cmd.arg(repo.join("tools").join("ge-extract").join("ge_extract.py"))
        .arg("--rom")
        .arg(&rom)
        .arg("--decomp")
        .arg(repo.join(&g.decomp))
        .args(["--level", &g.level, "--setup", &g.setup, "--code", &r.code])
        .arg("--scale")
        .arg(r.scale.to_string())
        .arg("--custom")
        .arg(&paths.custom);
    if !g.markers.is_empty() {
        cmd.arg("--markers").args(&g.markers);
    }
    let out = cmd.output().map_err(|e| format!("{}: {e} (the GoldenEye extractor needs Python; set PYTHON)", python.to_string_lossy()))?;
    if !out.status.success() {
        return Err(format!("tools/ge-extract failed:\n{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    }
    let path = dir.join("ge.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let x: Extract = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if x.format != "pd-ge-extract/1" {
        return Err(format!("{}: format {:?}, expected pd-ge-extract/1", path.display(), x.format));
    }
    let fixed = Gameplay {
        pads: x.pads.iter().map(|p| PadRow { pos: p.pos.into(), look: p.look.into(), up: p.up.into(), flags: p.flags, bbox: p.bbox }).collect(),
        props: x.props,
        ..Default::default()
    };
    let markers = x
        .markers
        .iter()
        .filter_map(|m| {
            let kind = match m.kind.as_str() {
                "spawn" => MarkerKind::Spawn,
                "person" => MarkerKind::Person,
                "item" => MarkerKind::Item,
                "prize" => MarkerKind::Prize,
                _ => return None,
            };
            Some(Marker { kind, pos: Vec3::from(m.pos), facing: m.facing })
        })
        .collect();
    Ok((fixed, markers, x.report))
}
