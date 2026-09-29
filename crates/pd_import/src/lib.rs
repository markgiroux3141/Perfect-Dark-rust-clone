//! **The custom level importer.** A level from another game becomes a Combat
//! Simulator arena: the same four stage files PD's arenas have
//! (`tools/pd-assets/pd_stage.py` writes theirs), so nothing at run time knows
//! it is not PD's. Everything a converted level lacks is made here, ahead of
//! time: its lighting baked into vertex colours, its rooms and portals, its
//! collision flagged as PD's tiles are, its waypoints, spawns, weapons,
//! hills, bases and cover.
//!
//! ```text
//! recipe (levels/<code>.json)
//!   ├─ source importer (oot) ──► LevelSource: triangles + materials + textures,
//!   │                             collision polygons, markers, environment
//!   │    └─ rooms ──► boxes over the floor area, every polygon cut to them, portals
//!   │         └─ write ──► bg.json + bg.bin + tex/, tiles.json   (the geometry)
//!   ├─ ge (GoldenEye) ──► tools/ge-extract: the level already in PD's formats
//!   │                     (its rooms, portals, tiles; its doors, their pads
//!   │                     and models), and the spots its setups mark
//!   └─ Stage::load, TileLevel::for_stage          (as the game loads it)
//!        └─ place ──► waypoints (nav::gen), spawns, weapons + ammo,
//!                     hills, bases, cover (after the source's own pads and props)
//!             └─ write ──► pads.json, setup.json, custom/levels.json
//!                  └─ check: the stage loads, a simulant match plays
//! ```
//!
//! Output goes to `custom/` (gitignored: a converted level is its game's data),
//! laid over `assets/` at run time (`pd_core::assets::AssetDir::with_custom_levels`);
//! the arena menu lists it under "Custom".
//!
//! Importers: [`oot`] (Ocarina of Time scenes, from the OoT Clone repo's
//! extractor), [`ge`] (GoldenEye 007 levels, from the ROM by
//! `tools/ge-extract`).

pub mod ge;
pub mod glb;
pub mod oot;
pub mod place;
pub mod recipe;
pub mod rooms;
pub mod source;
pub mod write;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pd_core::assets::AssetDir;
use pd_core::ids::BOTDIFF_NORMAL;
use pd_sim::stage::{Stage, TileLevel};

use recipe::{Recipe, Source};
use source::{LevelSource, Marker};

/// Where to read and write.
pub struct Paths {
    /// The repo's `assets/` (the world's animations, models and sounds).
    pub assets: PathBuf,
    /// The custom tree written into (`custom/`).
    pub custom: PathBuf,
    /// A source directory in place of the recipe's.
    pub src: Option<PathBuf>,
}

/// Convert `r` into `custom/stages/<code>/` and list it. Returns the report.
pub fn import(r: &Recipe, paths: &Paths) -> Result<Vec<String>, String> {
    match &r.source {
        Source::Oot(o) => {
            let dir = paths.src.clone().unwrap_or_else(|| PathBuf::from(&o.scene_dir));
            build(r, oot::load(r, o, &dir)?, paths)
        }
        Source::Ge(g) => {
            let mut report = vec![title(r)];
            let dir = fresh_stage_dir(paths, r)?;
            let (fixed, markers, lines) = ge::extract(r, g, paths, &dir)?;
            report.extend(lines);
            finish(r, paths, &dir, report, fixed, &markers)
        }
    }
}

fn title(r: &Recipe) -> String {
    format!("{} ({}, stage {:#x}), scale {}", r.name, r.code, r.stagenum, r.scale)
}

/// `custom/stages/<code>/`, emptied: the importer writes a stage from scratch.
fn fresh_stage_dir(paths: &Paths, r: &Recipe) -> Result<PathBuf, String> {
    let dir = paths.custom.join("stages").join(&r.code);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Everything after the source importer: `src` into the stage files, placed,
/// listed and checked.
pub fn build(r: &Recipe, mut src: LevelSource, paths: &Paths) -> Result<Vec<String>, String> {
    let mut report = vec![title(r)];
    let risers = src.drop_step_risers();
    let ledges = src.mark_ledges();
    let (lo, hi) = src.bounds();
    report.push(format!(
        "source: {} triangles, {} materials, {} textures, {} collision polygons ({} step risers dropped, {} ledges made climbable), {} markers; {:.0} x {:.0} x {:.0} m",
        src.tris.len(),
        src.materials.len(),
        src.textures.len(),
        src.collision.len(),
        risers,
        ledges,
        src.markers.len(),
        (hi.x - lo.x) / 100.0,
        (hi.z - lo.z) / 100.0,
        (hi.y - lo.y) / 100.0
    ));
    let part = rooms::Partition::build(&src, r.rooms);
    let dir = fresh_stage_dir(paths, r)?;
    let ntris = write::write_bg(&dir, r, &src, &part)?;
    let ntiles = write::write_tiles(&dir, r, &src, &part)?;
    report.push(format!("geometry: {} rooms, {} portals, {} BG triangles, {} tiles", part.rooms(), part.portals().len(), ntris, ntiles));
    finish(r, paths, &dir, report, write::Gameplay::default(), &src.markers)
}

/// The stage's geometry is written: load it as the game does, place the
/// gameplay data after `fixed` (what the source brings), write the pads and
/// the setup, list the level and check it.
fn finish(r: &Recipe, paths: &Paths, dir: &Path, mut report: Vec<String>, fixed: write::Gameplay, markers: &[Marker]) -> Result<Vec<String>, String> {
    // The stage as the game loads it, with only the source's gameplay data.
    write::write_pads(dir, r, &fixed)?;
    write::write_setup(dir, r, &fixed)?;
    let assets = AssetDir::new(&paths.assets).with_custom_dir(&paths.custom);
    let stage = Stage::load(&assets, &r.code)?;
    let level = TileLevel::for_stage(&stage);
    let placed = place::place(r, markers, &stage, &level, fixed)?;
    report.extend(placed.report);
    write::write_pads(dir, r, &placed.gameplay)?;
    write::write_setup(dir, r, &placed.gameplay)?;
    write::register(&paths.custom, r)?;

    report.extend(check(&assets, &r.code)?);
    let text = report.join("\n") + "\n";
    std::fs::write(dir.join("report.txt"), &text).map_err(|e| format!("report.txt: {e}"))?;
    Ok(report)
}

/// The finished stage loads as an arena does, every spawn stands over a floor,
/// and a minute of six simulants plays on it.
pub fn check(assets: &AssetDir, code: &str) -> Result<Vec<String>, String> {
    let stage = Arc::new(Stage::load(assets, code)?);
    let level = Arc::new(TileLevel::for_stage(&stage));
    let mut report = Vec::new();
    for &p in &stage.spawn_pads {
        let pad = &stage.pads[p];
        let (y, poly) = level.cd_find_ground_at_cyl(pad.pos, 30.0);
        if poly.is_none() || pad.pos.y - y > 150.0 || pad.pos.y < y {
            return Err(format!("spawn pad {p} at {} has no floor under it (ground {y})", pad.pos));
        }
    }
    // The rooms' lighting as PD settles it with every room on screen: the BG's
    // colours are scaled down to it (`room_highlight`), so a room much dimmer
    // than its neighbours shows a seam.
    let mut lights = pd_sim::lights::Lights::new(&stage.rooms);
    let flags = vec![pd_sim::stage::portals::ROOMFLAG_ONSCREEN; stage.rooms.roomcount()];
    let mut rng = pd_core::rng::Rng::new(1);
    for _ in 0..4 {
        lights.lighting_tick(&flags, 4, &mut rng, &[]);
    }
    let mut br: Vec<u8> = (1..stage.rooms.roomcount()).map(|r| lights.room_get_settled_regional_brightness_for_player(r)).collect();
    br.sort();
    if let (Some(lo), Some(hi)) = (br.first(), br.last()) {
        report.push(format!("lighting: room brightness {lo}..{hi}, median {}", br[br.len() / 2]));
    }
    let res = Arc::new(pd_sim::world::WorldRes::load(assets)?);
    let setup = pd_sim::harness::with_weapons(pd_sim::harness::setup(0, 6, BOTDIFF_NORMAL), &pd_sim::harness::DEFAULT_SET);
    let setup = pd_core::mp::MatchSetup { stagenum: stage.stagenum, ..setup };
    let w = pd_sim::world::World::new(setup, stage.clone(), level.clone(), res, 1)?;
    let m = pd_sim::harness::abtest::run_match(w, 60);
    report.push(format!(
        "check: a minute of six Normal simulants: {} kills, {} shots ({} hits), {} stalls, {} ledge falls",
        m.kills, m.shots, m.hits, m.stalls, m.ledge_falls
    ));
    Ok(report)
}

