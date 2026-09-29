//! A level's recipe (`crates/pd_import/levels/<code>.json`): where its source
//! is, how it is scaled and cropped, and how much of each kind of gameplay
//! data to generate. Committed; the source data and the output are not.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Recipe {
    pub format: String,
    /// The stage code: `custom/stages/<code>/`.
    pub code: String,
    /// The arena menu's name.
    pub name: String,
    /// Its `STAGE_*` number, from `pd_core::assets::CUSTOM_STAGENUMS`.
    pub stagenum: u8,
    /// Source units to PD centimetres.
    pub scale: f32,
    pub source: Source,
    /// How many rooms to split the level into (PD's arenas have 25-118): a
    /// source without rooms of its own. GoldenEye's levels bring theirs.
    #[serde(default)]
    pub rooms: usize,
    /// How many of each to place (PD's arenas: 10-21 spawns, 10 weapon
    /// locations with two ammo crates each, 4-7 hills).
    pub spawns: usize,
    pub weapons: usize,
    pub hills: usize,
    /// At most this share of the spawns and of the weapons stand on the
    /// source's own spots (its player starts, its pickups); the rest spread
    /// over the whole level. A source whose spots cover only part of it takes
    /// less (Facility's MP setup marks only the half GoldenEye's arena uses).
    #[serde(default = "one")]
    pub marker_share: f32,
}

fn one() -> f32 {
    1.0
}

/// Which importer reads the source, with its own settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "game", rename_all = "lowercase")]
pub enum Source {
    Oot(OotSource),
    Ge(GeSource),
}

/// A GoldenEye 007 level, from the NTSC ROM by `tools/ge-extract` (the BG, the
/// clipping, the setup's doors and their models, already in PD's formats).
#[derive(Clone, Debug, Deserialize)]
pub struct GeSource {
    /// The ROM (`ge007.u`, any byte order); `--src` overrides it.
    pub rom: String,
    /// The GoldenEye decomp (n64decomp/007), relative to the repo root.
    pub decomp: String,
    /// Its `levelinfotable` row (`bg.c:184`), e.g. `LEVELID_FACILITY`.
    pub level: String,
    /// The setup whose doors are kept (the solo one for the whole level).
    pub setup: String,
    /// Other setups whose spots placement prefers (the MP one).
    #[serde(default)]
    pub markers: Vec<String>,
}

/// An Ocarina of Time scene as the OoT Clone repo's extractor writes it
/// (`extracted/scenes/<category>/<scene>/`: `<scene>.glb`, `collision.json`,
/// `scene.json`).
#[derive(Clone, Debug, Deserialize)]
pub struct OotSource {
    /// The scene's directory; `--src` overrides it.
    pub scene_dir: String,
    pub scene: String,
    /// The scene header (layer) whose actors mark spots (0: child, day).
    pub layer: usize,
    /// The `LIGHT_SETTINGS_LIST` entry to bake and fog with.
    pub light_setting: usize,
    /// The sun's direction at the chosen time of day (`LIGHT_MODE_TIME`
    /// scenes move the first light with the sun), OoT's s8 units.
    pub sun_dir: Option<[f32; 3]>,
    /// The rooms whose meshes are drawn (all when absent). OoT draws only the
    /// room the player is in (and its neighbour through a doorway), so one
    /// room's meshes are what a player standing in it sees: the clean crop.
    #[serde(default)]
    pub draw_rooms: Option<Vec<usize>>,
    /// Source-unit boxes whose triangles and collision are dropped (a crop
    /// where the scene has no room boundary; a triangle goes by its centre).
    #[serde(default)]
    pub remove: Vec<Box3>,
    /// Invisible walls to close a crop, in source units.
    #[serde(default)]
    pub walls: Vec<Wall>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Box3 {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Box3 {
    pub fn contains(&self, p: glam::Vec3) -> bool {
        (0..3).all(|k| p[k] >= self.min[k] && p[k] <= self.max[k])
    }
}

/// A vertical wall from `from` to `to` (x, z) between heights `y`.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Wall {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub y: [f32; 2],
}

impl Recipe {
    pub fn load(path: &std::path::Path) -> Result<Recipe, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let r: Recipe = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if r.format != "pd-import-recipe/1" {
            return Err(format!("{}: format {:?}, expected pd-import-recipe/1", path.display(), r.format));
        }
        if !pd_core::assets::CUSTOM_STAGENUMS.contains(&r.stagenum) {
            return Err(format!("{}: stage number {:#x} is outside the custom range", path.display(), r.stagenum));
        }
        if pd_core::ids::STAGE_CODES.iter().any(|(_, c)| *c == r.code) {
            return Err(format!("{}: {:?} is one of PD's stage codes", path.display(), r.code));
        }
        Ok(r)
    }
}
