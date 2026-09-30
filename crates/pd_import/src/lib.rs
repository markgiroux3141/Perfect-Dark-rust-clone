//! **The custom level importer.** A level from another game, or any glTF,
//! becomes a Combat Simulator arena: the same four stage files PD's arenas
//! have (`tools/pd-assets/pd_stage.py` writes theirs), so nothing at run time
//! knows it is not PD's. Everything a converted level lacks is made here,
//! ahead of time: its lighting baked into vertex colours, its rooms and
//! portals, its collision flagged as PD's tiles are, its waypoints, spawns,
//! weapons, hills, bases and cover.
//!
//! ```text
//! recipe (levels/<code>.json)
//!   ├─ source importer (oot, gltf) ──► LevelSource: triangles + materials +
//!   │                             textures, collision polygons, markers, environment
//!   │    └─ rooms ──► boxes over the floor area, every polygon cut to them, portals
//!   │         └─ write ──► bg.json + bg.bin + tex/, tiles.json   (the geometry)
//!   ├─ ge (GoldenEye) ──► tools/ge-extract: the level already in PD's formats
//!   │                     (its rooms, portals, tiles; its doors, their pads
//!   │                     and models), and the spots its setups mark
//!   ├─ pd (a PD arena rebuilt, or PD stages fused) ──► the stages' own
//!   │                     data where unchanged
//!   └─ source.json: the source's own gameplay data and markers (for --place)
//!        └─ Stage::load, TileLevel::for_stage          (as the game loads it)
//!             └─ waypoints (nav::gen; cached in custom/cache/nav/ for --place)
//!                  └─ place ──► the layout (levels/<code>.layout.json) as it
//!                       is, the rest generated: spawns, weapons + ammo, hills,
//!                       bases, cover (after the source's own pads and props)
//!                       └─ write ──► pads.json, setup.json, custom/levels.json
//!                            └─ check: the stage loads, a simulant match plays
//! ```
//!
//! Output goes to `custom/` (gitignored: a converted level is its game's data),
//! laid over `assets/` at run time (`pd_core::assets::AssetDir::with_custom_levels`);
//! the arena menu lists it under "Custom". The hand-placed layout is
//! committed beside the recipe ([`layout`]); `pd_edit` writes it.
//!
//! Importers: [`oot`] (Ocarina of Time scenes, from the OoT Clone repo's
//! extractor), [`gltf`] (any glTF 2.0 file), [`ge`] (GoldenEye 007 levels,
//! from the ROM by `tools/ge-extract`), [`pd`] (PD's own arenas, rebuilt in
//! Blender).

pub mod cache;
pub mod ge;
pub mod glb;
pub mod gltf;
pub mod layout;
pub mod oot;
pub mod pd;
pub mod place;
pub mod recipe;
pub mod rooms;
pub mod source;
pub mod waypoints;
pub mod write;

#[cfg(test)]
mod gltf_tests;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::Vec3;
use pd_core::assets::AssetDir;
use pd_core::ids::BOTDIFF_NORMAL;
use pd_sim::stage::{Stage, TileLevel};
use serde_json::{json, Value};

use layout::Layout;
use recipe::{Recipe, Source};
use source::{LevelSource, Marker, MarkerKind};

/// Where to read and write.
pub struct Paths {
    /// The repo's `assets/` (the world's animations, models and sounds).
    pub assets: PathBuf,
    /// The custom tree written into (`custom/`).
    pub custom: PathBuf,
    /// A source directory in place of the recipe's.
    pub src: Option<PathBuf>,
}

impl Paths {
    /// `custom/stages/<code>/`.
    pub fn stage_dir(&self, code: &str) -> PathBuf {
        self.custom.join("stages").join(code)
    }
}

/// What a source importer hands on besides the geometry it wrote: its own
/// gameplay data (a GoldenEye level's doors, a rebuilt arena's setup) and
/// what placement makes of the rest. Kept in the stage directory as
/// `source.json`, so [`replace`] can place the level again without importing it.
pub struct SourceData {
    pub fixed: write::Gameplay,
    pub how: SourceHow,
    /// The report so far (the source and its geometry).
    pub report: Vec<String>,
}

/// [`place::How`], owned.
pub enum SourceHow {
    /// Generate what the layout leaves out, preferring these spots.
    Generate(Vec<Marker>),
    /// Keep the source's own rows; add cover in these rooms.
    Keep(Vec<u16>),
}

const SOURCE_FORMAT: &str = "pd-import-source/1";

impl SourceData {
    fn save(&self, dir: &Path) -> Result<(), String> {
        let how = match &self.how {
            SourceHow::Generate(markers) => json!({"generate": markers.iter().map(|m| json!({"kind": m.kind.name(), "pos": [m.pos.x, m.pos.y, m.pos.z], "facing": m.facing})).collect::<Vec<_>>()}),
            SourceHow::Keep(rooms) => json!({"keep": rooms}),
        };
        write::write_json(&dir.join("source.json"), &json!({"format": SOURCE_FORMAT, "fixed": self.fixed.to_json(), "how": how, "report": self.report}))
    }

    fn load(dir: &Path) -> Result<SourceData, String> {
        let path = dir.join("source.json");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e} (import the level first)", path.display()))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if v["format"] != SOURCE_FORMAT {
            return Err(format!("{}: format {}, expected {SOURCE_FORMAT} (import the level again)", path.display(), v["format"]));
        }
        let how = if let Some(ms) = v["how"]["generate"].as_array() {
            SourceHow::Generate(
                ms.iter()
                    .filter_map(|m| {
                        let p = &m["pos"];
                        Some(Marker { kind: MarkerKind::from_name(m["kind"].as_str()?)?, pos: Vec3::new(p[0].as_f64()? as f32, p[1].as_f64()? as f32, p[2].as_f64()? as f32), facing: m["facing"].as_f64().unwrap_or(0.0) as f32 })
                    })
                    .collect(),
            )
        } else {
            SourceHow::Keep(v["how"]["keep"].as_array().into_iter().flatten().filter_map(|r| r.as_u64()).map(|r| r as u16).collect())
        };
        let report = v["report"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_owned)).collect();
        Ok(SourceData { fixed: write::Gameplay::from_json(&v["fixed"])?, how, report })
    }
}

/// How [`finish`] ends.
#[derive(Clone, Copy, Debug)]
pub struct Finish {
    /// Play the check match (a minute of simulants: a few seconds).
    pub check: bool,
    /// Generate the waypoints even if the cache has them for this geometry.
    pub fresh_graph: bool,
}

/// Convert `r` into `custom/stages/<code>/`, place it with `layout`, list it
/// and check it. Returns the report.
pub fn import(r: &Recipe, paths: &Paths, layout: &Layout) -> Result<Vec<String>, String> {
    let (dir, data) = geometry(r, paths)?;
    finish(r, paths, &dir, data, layout, Finish { check: true, fresh_graph: true })
}

/// Place a level whose geometry is already written again (its `source.json`
/// read back, the waypoints from the cache when the geometry is the same),
/// with `layout`: what the editor does on a save. Returns the report.
pub fn replace(r: &Recipe, paths: &Paths, layout: &Layout, how: Finish) -> Result<Vec<String>, String> {
    let dir = paths.stage_dir(&r.code);
    let data = SourceData::load(&dir)?;
    finish(r, paths, &dir, data, layout, how)
}

/// The source importer: the stage's geometry written into a fresh
/// `custom/stages/<code>/`, and what the source brings besides (saved there).
pub fn geometry(r: &Recipe, paths: &Paths) -> Result<(PathBuf, SourceData), String> {
    let (dir, data) = match &r.source {
        Source::Oot(o) => {
            let dir = paths.src.clone().unwrap_or_else(|| PathBuf::from(&o.scene_dir));
            write_geometry(r, oot::load(r, o, &dir)?, paths, vec![title(r)])?
        }
        Source::Gltf(g) => {
            let mut report = vec![title(r)];
            let path = paths.src.clone().unwrap_or_else(|| PathBuf::from(&g.path));
            let src = gltf::load(r, g, &path, &mut report)?;
            write_geometry(r, src, paths, report)?
        }
        Source::Ge(g) => {
            let mut report = vec![title(r)];
            let dir = fresh_stage_dir(paths, r)?;
            let (fixed, markers, lines) = ge::extract(r, g, paths, &dir)?;
            report.extend(lines);
            (dir, SourceData { fixed, how: SourceHow::Generate(markers), report })
        }
        Source::Pd(p) => {
            let mut report = vec![title(r)];
            let dir = fresh_stage_dir(paths, r)?;
            let (fixed, how, lines) = pd::rebuild(r, p, paths, &dir)?;
            report.extend(lines);
            (dir, SourceData { fixed, how, report })
        }
    };
    data.save(&dir)?;
    Ok((dir, data))
}

fn title(r: &Recipe) -> String {
    format!("{} ({}, stage {:#x}), scale {}", r.name, r.code, r.stagenum, r.scale)
}

/// `custom/stages/<code>/`, emptied: the importer writes a stage from scratch.
fn fresh_stage_dir(paths: &Paths, r: &Recipe) -> Result<PathBuf, String> {
    let dir = paths.stage_dir(&r.code);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// A level source's rooms when the recipe doesn't say: one per ~250 m² of
/// floor (PD's arenas have 25-118).
fn auto_rooms(src: &LevelSource) -> usize {
    let floor = pd_core::ids::GEOFLAG_FLOOR1 | pd_core::ids::GEOFLAG_FLOOR2;
    let area: f32 = src
        .collision
        .iter()
        .filter(|p| p.flags & floor != 0)
        .map(|p| (1..p.verts.len() - 1).map(|i| (p.verts[i] - p.verts[0]).cross(p.verts[i + 1] - p.verts[0]).y.abs() * 0.5).sum::<f32>())
        .sum();
    ((area / 2_500_000.0).round() as usize).clamp(8, 100)
}

/// Everything after a [`LevelSource`] importer: its triangles and collision
/// into the stage's geometry files.
fn write_geometry(r: &Recipe, mut src: LevelSource, paths: &Paths, mut report: Vec<String>) -> Result<(PathBuf, SourceData), String> {
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
    let rooms = if r.rooms > 0 { r.rooms } else { auto_rooms(&src) };
    let part = rooms::Partition::build(&src, rooms);
    let dir = fresh_stage_dir(paths, r)?;
    let ntris = write::write_bg(&dir, r, &src, &part)?;
    let ntiles = write::write_tiles(&dir, r, &src, &part)?;
    report.push(format!("geometry: {} rooms, {} portals, {} BG triangles, {} tiles", part.rooms(), part.portals().len(), ntris, ntiles));
    Ok((dir, SourceData { fixed: write::Gameplay::default(), how: SourceHow::Generate(src.markers), report }))
}

/// Everything after a [`LevelSource`] importer, placed without a layout.
pub fn build(r: &Recipe, src: LevelSource, paths: &Paths) -> Result<Vec<String>, String> {
    let (dir, data) = write_geometry(r, src, paths, vec![title(r)])?;
    data.save(&dir)?;
    finish(r, paths, &dir, data, &Layout::new(), Finish { check: true, fresh_graph: true })
}

/// The stage's geometry is written: load it as the game does, place the
/// gameplay data after the source's own with `layout`, write the pads and the
/// setup, list the level and (if asked) check it.
fn finish(r: &Recipe, paths: &Paths, dir: &Path, data: SourceData, layout: &Layout, how: Finish) -> Result<Vec<String>, String> {
    let mut report = data.report;
    // The stage as the game loads it, with only the source's gameplay data.
    write::write_pads(dir, r, &data.fixed)?;
    write::write_setup(dir, r, &data.fixed)?;
    let assets = AssetDir::new(&paths.assets).with_custom_dir(&paths.custom);
    let stage = Stage::load(&assets, &r.code)?;
    let level = TileLevel::for_stage(&stage);
    let (mut graph, line) = cache::graph(paths, &r.code, dir, &level, how.fresh_graph)?;
    report.push(line);
    if let Some(edits) = layout.waypoints.as_ref().filter(|e| !e.is_empty()) {
        let (edited, line) = waypoints::apply(&graph, edits, &level);
        graph = edited;
        report.push(line);
    }
    let placement = match &data.how {
        SourceHow::Generate(m) => place::How::Generate(m),
        SourceHow::Keep(rooms) => place::How::Keep(rooms),
    };
    let placed = place::place(r, &stage, &level, &graph, data.fixed, layout, &placement)?;
    report.extend(placed.report);
    write::write_pads(dir, r, &placed.gameplay)?;
    write::write_setup(dir, r, &placed.gameplay)?;
    write::register(&paths.custom, r)?;

    if how.check {
        report.extend(check(&assets, &r.code)?);
    }
    let text = report.join("\n") + "\n";
    std::fs::write(dir.join("report.txt"), &text).map_err(|e| format!("report.txt: {e}"))?;
    Ok(report)
}

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

