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
    /// The arena menu's group ("Jedi Academy"); else "Custom".
    #[serde(default)]
    pub group: Option<String>,
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
    /// locations with two ammo crates each, 4-7 hills). A PD arena rebuilt
    /// keeps its own.
    #[serde(default)]
    pub spawns: usize,
    #[serde(default)]
    pub weapons: usize,
    #[serde(default)]
    pub hills: usize,
    /// At most this share of the spawns and of the weapons stand on the
    /// source's own spots (its player starts, its pickups); the rest spread
    /// over the whole level. A source whose spots cover only part of it takes
    /// less (Facility's MP setup marks only the half GoldenEye's arena uses).
    #[serde(default = "one")]
    pub marker_share: f32,
    /// Which of the waypoint graph's parts placement uses: the largest, or
    /// the one most of the source's player starts are in (a level whose
    /// outside, its roofs, is the biggest walkable part: Jedi Academy's).
    #[serde(default)]
    pub play_area: PlayArea,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayArea {
    #[default]
    Largest,
    Starts,
}

fn one() -> f32 {
    1.0
}

/// Which importer reads the source, with its own settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "game", rename_all = "lowercase")]
pub enum Source {
    Oot(OotSource),
    Gltf(GltfSource),
    Ge(GeSource),
    Pd(PdSource),
    Jka(JkaSource),
}

/// A Jedi Academy map ([`crate::jka`]): a compiled `.bsp`, with the game's
/// own directory for its textures and shader scripts.
#[derive(Clone, Debug, Deserialize)]
pub struct JkaSource {
    /// The game's `base` directory (`GameData/base`: `assets0.pk3` ...).
    pub base: String,
    /// The map: a `.bsp` file, or its path in the game's packages
    /// (`maps/mp/ffa1.bsp`); `--src` overrides it.
    pub bsp: String,
    /// The clear colour where nothing is drawn (else the sky box's average,
    /// else black).
    #[serde(default)]
    pub sky: Option<[u8; 3]>,
    /// The lightmaps' colours are scaled by this into the vertex colours.
    #[serde(default = "one")]
    pub light_scale: f32,
    /// Textures bigger than this (pixels, either side) are halved until they
    /// fit.
    #[serde(default = "max_texture")]
    pub max_texture: u32,
    /// Walls up to shoulder height between two floors become climbable
    /// ([`crate::source::LevelSource::mark_ledges`]): PD has no jump, and
    /// Jedi Academy's levels are built round one.
    #[serde(default = "yes")]
    pub climb_ledges: bool,
    /// Shader names drawn as others: a texture the map shipped in its own
    /// package that the game lacks, as the nearest one it has
    /// (`"textures/yavin/temple_vines": "textures/yavin/temple_vines2"`).
    #[serde(default)]
    pub substitute: std::collections::HashMap<String, String>,
    /// The lighting's detail: an edge is split where its middle is lit this
    /// many steps (of 255) off its ends, while it is longer than
    /// `light_min_edge` units.
    #[serde(default = "light_tolerance")]
    pub light_tolerance: f32,
    #[serde(default = "light_min_edge")]
    pub light_min_edge: f32,
}

fn light_tolerance() -> f32 {
    20.0
}

fn light_min_edge() -> f32 {
    32.0
}

fn yes() -> bool {
    true
}

/// Any glTF 2.0 level, `.glb` or `.gltf` ([`crate::gltf`]).
#[derive(Clone, Debug, Deserialize)]
pub struct GltfSource {
    /// The file; `--src` overrides it.
    pub path: String,
    /// The clear colour where no geometry is (an outdoor level's sky).
    #[serde(default)]
    pub sky: Option<[u8; 3]>,
    /// Toward the sun baked into meshes without vertex colours (default
    /// high, from +x +z).
    #[serde(default)]
    pub sun: Option<[f32; 3]>,
    /// Walls up to shoulder height between two floors become climbable, as
    /// Kokiri's ledges ([`crate::source::LevelSource::mark_ledges`]); PD's
    /// arenas have none.
    #[serde(default)]
    pub climb_ledges: bool,
    /// Textures bigger than this (pixels, either side) are halved until they
    /// fit.
    #[serde(default = "max_texture")]
    pub max_texture: u32,
}

fn max_texture() -> u32 {
    256
}

/// One of PD's arenas rebuilt in another tool: a glTF of its rooms, changed
/// and added to (the Blender MCP repo's levels: `complex_plus.py` makes Very
/// Complex from Complex), laid over the arena it came from ([`crate::pd`]);
/// or several PD stages fused into one (`felicity_ci.py`: CI Training and
/// Felicity joined by a hall), laid over them all.
#[derive(Clone, Debug, Deserialize)]
pub struct PdSource {
    /// One arena rebuilt: its stage code (`ref`: Complex), in `assets/stages/`.
    /// Its setup is kept whole, on its own pads.
    #[serde(default)]
    pub base: Option<String>,
    /// Several stages fused, each moved to where the glTF has it. The gameplay
    /// data is generated (from the recipe's counts), preferring the spots the
    /// bases' MP setups mark.
    #[serde(default)]
    pub bases: Vec<PdBase>,
    /// The `.glb`: a node per room (`extras.ge_room`, and with `bases` the
    /// room's stage and number there, `ge_level` and `ge_room_src`), each
    /// material naming its PD texture (`extras.ge_preset`); `--src` overrides it.
    pub glb: String,
}

/// One stage of a fused level.
#[derive(Clone, Debug, Deserialize)]
pub struct PdBase {
    /// Its stage code: an arena in `assets/stages/`, or (with `stage`) any
    /// other of PD's stages.
    pub code: String,
    /// Not an arena: its `STAGE_*` (`STAGE_CITRAINING`), exported from the
    /// decomp by `tools/pd-assets/pd_stage.py --base` into `custom/cache/pd/`
    /// on the first import.
    #[serde(default)]
    pub stage: Option<String>,
    /// The glTF rooms' `extras.ge_level` for this stage (`CI`, `FEL`).
    pub tag: String,
    /// The fused level's number for this stage's room 1 (its rooms follow in
    /// order).
    pub first_room: u16,
    /// Turned about the vertical by this many degrees (Blender's +Z, PD's +Y:
    /// x' = x cos + z sin, z' = z cos - x sin), then moved by `offset` (PD's
    /// axes, cm: Blender's (x, y, z) is PD's (x, -z, y)).
    #[serde(default)]
    pub turn: f64,
    #[serde(default)]
    pub offset: [f64; 3],
}

impl PdSource {
    /// The stages the level is laid over: `bases`, or the one `base` in place.
    pub fn parts(&self) -> Result<Vec<PdBase>, String> {
        match (&self.base, self.bases.is_empty()) {
            (Some(code), true) => Ok(vec![PdBase { code: code.clone(), stage: None, tag: String::new(), first_room: 1, turn: 0.0, offset: [0.0; 3] }]),
            (None, false) => Ok(self.bases.clone()),
            _ => Err("a pd source names its arena (`base`) or the stages it fuses (`bases`), one of the two".into()),
        }
    }
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
