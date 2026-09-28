//! PD models: the one format every model file is exported in, the walker that
//! poses it (`model_set_matrices_with_anim`), the part-box hit test
//! (`model_test_for_hit`) and a CPU draw through [`n64::rdp`].
//!
//! One format and one walker for guns, hands, casings, held guns, props, chr
//! bodies and heads, and the menu hudpiece. The spikes had two walkers (the gun
//! spike's, which skipped the elbow/knee helper matrices every chr body uses,
//! and the menu's) over two formats, plus glTF bodies for the simulants.
//!
//! # The format (`tools/pd-assets/pd_models.py`)
//!
//! `assets/models/<stem>.json` holds the `modeldef`: `name`, `stem`, PD's
//! `filenum`/`file` (FILE_*), `source` (the decomp file), `skel`,
//! `nummatrices`, and
//!
//! * `nodes[]`: the node tree in PD's depth-first order (a parent before its
//!   children, a subtree contiguous), each `{type, parent, partnum?, ...}`:
//!   `chrinfo {animpart, mtx}`, `position {pos, animpart, mtx[3], flags}` (the
//!   `MODELNODETYPE_0100`/`0200` bits of the node type in `flags`, their helper
//!   slots in `mtx[1]`/`mtx[2]`), `positionheld {pos, mtx}`, `toggle {target}`,
//!   `distance {near, far, target}`, `headspot`, `bbox {hitpart, bbox[6]}`,
//!   `chrgunfire {pos, dim, texture, texture_size}`, `stargunfire {quads,
//!   batches}`, `gundl`/`dl {rendermode, cull_exit?}`, and the rest by name;
//! * `parts`: `{MODELPART_*: node index}` (`model_get_part`);
//! * `materials[]`: the N64 draw state each batch was drawn with, interpreted
//!   from the display lists ([`Material`]); a texture is named by its pool
//!   number, or `0x10000 | texconfig index` when it is stored in the model;
//! * `textures`: `{id: {file, w, h, cfg_w, cfg_h, levels, source}}`, `file`
//!   relative to the asset root;
//! * `batches[]`: `{node, material, nverts, nidx}` in draw order.
//!
//! `assets/models/<stem>.bin` holds the batches back to back, little-endian:
//! `nverts` vertices of 28 bytes (`f32 x,y,z; u16 mtx; f32 u,v; u8 c[4]; u8
//! flags; u8 pad`: `mtx` is the model matrix slot the display list loaded, u/v
//! are texels (the texgen scale when texgen), `c` is a colour or, when lit, a
//! normal; flags 1 = lit, 2 = texgen), then `nidx` u16 indices.
//!
//! Units are the model's own (a gun is millimetres, scaled into world
//! centimetres by its root; a chr body is scaled by `g_HeadsAndBodies[].scale ×
//! 0.1`, `body.c:170`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::Vec3;
use serde::Deserialize;

use crate::assets::AssetDir;

pub mod draw;
pub mod hit;
pub mod pose;

#[cfg(test)]
mod oracle;
#[cfg(test)]
mod tests;

pub use n64::rdp::Cull;
pub use pose::{JointFn, Model, PoseParams};

/// `modeldef.skel` values that matter to callers (`g_Skel*`, skeletons.c).
pub const SKEL_CHR: i32 = 0x09;
pub const SKEL_HEAD: i32 = 0x0d;
pub const SKEL_HUDPIECE: i32 = 0x2a;

/// The node-type flag bits that give a POSITION node its helper matrices.
pub const MODELNODETYPE_0100: u32 = 0x0100;
pub const MODELNODETYPE_0200: u32 = 0x0200;

/// `MODELPART_CHR_HEADSPOT`.
pub const MODELPART_CHR_HEADSPOT: i32 = 4;

// ─── the JSON header ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Header {
    format: String,
    name: String,
    stem: String,
    filenum: u32,
    #[serde(default)]
    skel: Option<i64>,
    nummatrices: usize,
    nodes: Vec<RawNode>,
    parts: HashMap<String, usize>,
    materials: Vec<Material>,
    textures: HashMap<String, TexInfo>,
    batches: Vec<BatchHead>,
}

#[derive(Deserialize)]
struct BatchHead {
    node: usize,
    material: usize,
    nverts: usize,
    nidx: usize,
}

#[derive(Deserialize, Debug, Clone)]
struct RawNode {
    #[serde(rename = "type")]
    kind: String,
    parent: i32,
    partnum: Option<i32>,
    pos: Option<[f32; 3]>,
    animpart: Option<u16>,
    mtx: Option<[i16; 3]>,
    flags: Option<u32>,
    near: Option<f32>,
    far: Option<f32>,
    rendermode: Option<i32>,
    cull_exit: Option<String>,
    quads: Option<Vec<[[i16; 3]; 4]>>,
    hitpart: Option<i32>,
    bbox: Option<[f32; 6]>,
    dim: Option<[f32; 3]>,
    texture: Option<u32>,
    texture_size: Option<[f32; 2]>,
}

/// How a batch's triangles are culled.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatCull {
    /// The node never set cull bits: the state left by the previous node drawn.
    Inherit,
    None,
    Back,
    Front,
    Both,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatBlend {
    Opaque,
    /// The blender's `M = CLR_MEM, B = 1MA`.
    Alpha,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatAlphaTest {
    None,
    Edge,
    Threshold,
}

/// One interpreted N64 draw state (`pd_fpgun.py` `Interp.material_key`).
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Material {
    pub two_cycle: bool,
    /// fast3d mux order: `[a0,b0,c0,d0, Aa0,Ab0,Ac0,Ad0, a1,b1,c1,d1, Aa1,Ab1,Ac1,Ad1]`.
    pub combine: [u8; 16],
    pub cull: MatCull,
    pub lighting: bool,
    pub texgen: bool,
    pub texgen_linear: bool,
    pub texture: Option<MatTexture>,
    pub prim: [u8; 4],
    /// `None`: the render context's env colour.
    pub env: Option<[u8; 4]>,
    /// `None`: the render context's fog colour.
    pub fog: Option<[u8; 4]>,
    pub blend: MatBlend,
    pub ztest: bool,
    pub zwrite: bool,
    pub decal: bool,
    pub alpha_test: MatAlphaTest,
    /// Cycle 1 of a two-cycle blender is `FOG_PRIM_A` (the gun's shade tint).
    /// The menu context turns fog off, so the menus ignore it.
    pub fog_tint: bool,
}

/// Tile 0 of a material, as the texture load left it.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct MatTexture {
    pub id: u32,
    /// 0 wrap, 1 clamp, 2 mirror (PD's TXMODE_*).
    pub cms: u8,
    pub cmt: u8,
    pub shifts: u8,
    pub shiftt: u8,
    pub uls: f32,
    pub ult: f32,
    pub mipmap: bool,
    pub linear: bool,
}

/// Where a texture's pixels are, and its texconfig.
#[derive(Deserialize, Debug, Clone)]
pub struct TexInfo {
    /// Relative to the asset root.
    pub file: String,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub cfg_w: u32,
    #[serde(default)]
    pub cfg_h: u32,
    #[serde(default)]
    pub levels: Option<u32>,
}

// ─── the definition ──────────────────────────────────────────────────────────

/// One vertex of a batch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
    pub pos: Vec3,
    /// The model matrix slot the display list loaded for this vertex.
    pub mtx: u16,
    pub uv: [f32; 2],
    /// A colour, or when lit a signed normal and alpha.
    pub c: [u8; 4],
    /// 1 = lit, 2 = texgen.
    pub flags: u8,
}

/// The triangles one node drew in one draw state.
#[derive(Clone, Debug)]
pub struct Batch {
    pub node: usize,
    pub material: usize,
    pub verts: Vec<Vert>,
    pub idx: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// `MODELNODETYPE_CHRINFO`: the chr root (`modelrodata_chrinfo`).
    ChrInfo { animpart: u16, mtx: i16 },
    /// `MODELNODETYPE_POSITION`: a joint. `flags` holds the node type's
    /// `0x0100`/`0x0200` helper bits; their slots are `mtx[1]`/`mtx[2]`.
    Position { pos: Vec3, animpart: u16, mtx: [i16; 3], flags: u32 },
    PositionHeld { pos: Vec3, mtx: i16 },
    /// Children drawn only while visible (`rwdata->toggle.visible`).
    Toggle,
    /// Children drawn only between `near` and `far` (× model scale).
    Distance { near: f32, far: f32 },
    /// Where a head model is attached (`model_attach_head`).
    HeadSpot,
    /// A part box (`modelrodata_bbox`): `[xmin, xmax, ymin, ymax, zmin, zmax]`.
    BBox { hitpart: i32, bbox: [f32; 6] },
    /// A third-person muzzle flash billboard (`modelrodata_chrgunfire`).
    ChrGunfire { pos: Vec3, dim: Vec3, texture: Option<u32>, texture_size: [f32; 2] },
    /// The first-person muzzle star (`model_render_node_star_gunfire`).
    StarGunfire { quads: Vec<[[i16; 3]; 4]> },
    GunDl { rendermode: i32 },
    Dl { rendermode: i32 },
    /// Any other node type, by its exporter name (`reorder`, `type11`, ...).
    Other(String),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<usize>,
    /// The `MODELPART_*` that names this node, if any.
    pub partnum: Option<i32>,
    /// The cull state this node's display list leaves behind, if it changed it.
    pub cull_exit: Option<Cull>,
    /// Batches this node draws, in display-list order.
    pub batches: Vec<usize>,
    /// One past the last node of this node's subtree (the tree is preorder).
    pub subtree_end: usize,
}

/// An immutable model file (PD's `modeldef`).
#[derive(Clone, Debug)]
pub struct ModelDef {
    pub name: String,
    pub stem: String,
    pub filenum: u32,
    pub skel: i32,
    pub nummatrices: usize,
    pub nodes: Vec<Node>,
    pub parts: HashMap<i32, usize>,
    pub materials: Vec<Material>,
    pub textures: HashMap<u32, TexInfo>,
    pub batches: Vec<Batch>,
    /// `modeldef_find_bbox_rodata`: the first BBOX node's box.
    pub bbox: Option<[f32; 6]>,
}

fn parse_cull(s: &str) -> Option<Cull> {
    match s {
        "none" => Some(Cull::None),
        "back" => Some(Cull::Back),
        "front" => Some(Cull::Front),
        "both" => Some(Cull::Both),
        _ => None,
    }
}

impl ModelDef {
    /// Load `models/<stem>.json` + `.bin`.
    pub fn load(assets: &AssetDir, stem: &str) -> Result<ModelDef, String> {
        let head: Header = assets.read_json(&assets.model_json(stem))?;
        if head.format != "pd-model/1" {
            return Err(format!("{stem}: format {:?}, expected pd-model/1", head.format));
        }
        let bin_path = assets.model_bin(stem);
        let data = assets.read(&bin_path)?;
        let err = |what: &str| format!("{}: {what}", bin_path.display());
        let mut off = 0usize;
        let f32_at = |o: usize| f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let mut batches = Vec::with_capacity(head.batches.len());
        for b in &head.batches {
            if off + b.nverts * 28 + b.nidx * 2 > data.len() {
                return Err(err("truncated"));
            }
            let verts = (0..b.nverts)
                .map(|i| {
                    let o = off + i * 28;
                    Vert {
                        pos: Vec3::new(f32_at(o), f32_at(o + 4), f32_at(o + 8)),
                        mtx: u16::from_le_bytes([data[o + 12], data[o + 13]]),
                        uv: [f32_at(o + 14), f32_at(o + 18)],
                        c: [data[o + 22], data[o + 23], data[o + 24], data[o + 25]],
                        flags: data[o + 26],
                    }
                })
                .collect();
            off += b.nverts * 28;
            let idx = (0..b.nidx).map(|i| u16::from_le_bytes([data[off + 2 * i], data[off + 2 * i + 1]])).collect();
            off += b.nidx * 2;
            batches.push(Batch { node: b.node, material: b.material, verts, idx });
        }
        if off != data.len() {
            return Err(err("trailing bytes"));
        }
        Ok(Self::from_parts(head, batches))
    }

    fn from_parts(head: Header, batches: Vec<Batch>) -> ModelDef {
        let n = head.nodes.len();
        let mut nodes: Vec<Node> = head
            .nodes
            .iter()
            .map(|r| {
                let pos = Vec3::from(r.pos.unwrap_or([0.0; 3]));
                let kind = match r.kind.as_str() {
                    "chrinfo" => NodeKind::ChrInfo { animpart: r.animpart.unwrap_or(0), mtx: r.mtx.map_or(-1, |m| m[0]) },
                    "position" => NodeKind::Position { pos, animpart: r.animpart.unwrap_or(0), mtx: r.mtx.unwrap_or([0, -1, -1]), flags: r.flags.unwrap_or(0) },
                    "positionheld" => NodeKind::PositionHeld { pos, mtx: r.mtx.map_or(0, |m| m[0]) },
                    "toggle" => NodeKind::Toggle,
                    "distance" => NodeKind::Distance { near: r.near.unwrap_or(0.0), far: r.far.unwrap_or(0.0) },
                    "headspot" => NodeKind::HeadSpot,
                    "bbox" => NodeKind::BBox { hitpart: r.hitpart.unwrap_or(0), bbox: r.bbox.unwrap_or([0.0; 6]) },
                    "chrgunfire" => NodeKind::ChrGunfire {
                        pos,
                        dim: Vec3::from(r.dim.unwrap_or([0.0; 3])),
                        texture: r.texture,
                        texture_size: r.texture_size.unwrap_or([0.0; 2]),
                    },
                    "stargunfire" => NodeKind::StarGunfire { quads: r.quads.clone().unwrap_or_default() },
                    "gundl" => NodeKind::GunDl { rendermode: r.rendermode.unwrap_or(0) },
                    "dl" => NodeKind::Dl { rendermode: r.rendermode.unwrap_or(0) },
                    other => NodeKind::Other(other.to_owned()),
                };
                Node {
                    kind,
                    parent: (r.parent >= 0).then_some(r.parent as usize),
                    partnum: r.partnum,
                    cull_exit: r.cull_exit.as_deref().and_then(parse_cull),
                    batches: Vec::new(),
                    subtree_end: n,
                }
            })
            .collect();
        for (bi, b) in batches.iter().enumerate() {
            if let Some(node) = nodes.get_mut(b.node) {
                node.batches.push(bi);
            }
        }
        // Preorder: a subtree ends at the first later node that is not below it.
        for i in 0..n {
            let mut end = i + 1;
            while end < n && Self::is_below(&nodes, end, i) {
                end += 1;
            }
            nodes[i].subtree_end = end;
        }
        let bbox = nodes.iter().find_map(|n| match n.kind {
            NodeKind::BBox { bbox, .. } => Some(bbox),
            _ => None,
        });
        ModelDef {
            name: head.name,
            stem: head.stem,
            filenum: head.filenum,
            skel: head.skel.unwrap_or(0) as i32,
            nummatrices: head.nummatrices.max(1),
            parts: head.parts.iter().filter_map(|(k, v)| k.parse::<i32>().ok().map(|p| (p, *v))).collect(),
            materials: head.materials,
            textures: head.textures.iter().filter_map(|(k, v)| k.parse::<u32>().ok().map(|id| (id, v.clone()))).collect(),
            nodes,
            batches,
            bbox,
        }
    }

    fn is_below(nodes: &[Node], node: usize, ancestor: usize) -> bool {
        let mut cur = nodes[node].parent;
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            cur = nodes[p].parent;
        }
        false
    }

    /// `model_get_part` (`model.c:327`).
    pub fn get_part(&self, partnum: i32) -> Option<usize> {
        self.parts.get(&partnum).copied()
    }

    /// `model_find_node_mtx_index(node, which)` (`model.c:123`): the nearest
    /// node at or above `node` that owns a matrix; `which` picks a POSITION
    /// node's helper slot (`0x100`, `0x200`). `None` for PD's -1.
    pub fn find_node_mtx_index(&self, node: usize, which: u32) -> Option<usize> {
        let mut cur = Some(node);
        while let Some(i) = cur {
            let m = match &self.nodes[i].kind {
                NodeKind::ChrInfo { mtx, .. } => Some(*mtx),
                NodeKind::Position { mtx, .. } => Some(mtx[if which == MODELNODETYPE_0200 { 2 } else if which == MODELNODETYPE_0100 { 1 } else { 0 }]),
                NodeKind::PositionHeld { mtx, .. } => Some(*mtx),
                _ => None,
            };
            if let Some(m) = m {
                return (m >= 0).then_some(m as usize);
            }
            cur = self.nodes[i].parent;
        }
        None
    }

    /// `body_calculate_head_offset`'s vertex pass (`body.c`): every
    /// `MODELNODETYPE_DL` vertex and the bbox move up by `offset`.
    pub fn with_head_offset(&self, offset: f32) -> ModelDef {
        let mut d = self.clone();
        for b in &mut d.batches {
            if matches!(d.nodes[b.node].kind, NodeKind::Dl { .. }) {
                for v in &mut b.verts {
                    v.pos.y += offset;
                }
            }
        }
        if let Some(bb) = &mut d.bbox {
            bb[2] += offset;
            bb[3] += offset;
        }
        d
    }
}

// ─── the store ───────────────────────────────────────────────────────────────

/// One `models/index.json` entry.
#[derive(Deserialize, Debug, Clone)]
pub struct ModelIndexEntry {
    pub filenum: u32,
    /// `FILE_*`.
    pub file: String,
    /// gun, hand, casing, held, prop, chr, hud.
    pub kind: String,
    pub source: String,
    pub tris: usize,
}

/// Every exported model file, loaded on first use and shared.
pub struct ModelStore {
    pub assets: AssetDir,
    pub index: HashMap<String, ModelIndexEntry>,
    by_filenum: HashMap<u32, String>,
    defs: Mutex<HashMap<String, Arc<ModelDef>>>,
}

impl ModelStore {
    pub fn load(assets: &AssetDir) -> Result<ModelStore, String> {
        let index: HashMap<String, ModelIndexEntry> = assets.read_json(&assets.model_index())?;
        let by_filenum = index.iter().map(|(stem, e)| (e.filenum, stem.clone())).collect();
        Ok(ModelStore { assets: assets.clone(), index, by_filenum, defs: Mutex::new(HashMap::new()) })
    }

    /// A model by file stem (e.g. `"falcon2"`, `"dark_combat"`).
    pub fn get(&self, stem: &str) -> Result<Arc<ModelDef>, String> {
        if let Some(d) = self.defs.lock().unwrap().get(stem) {
            return Ok(d.clone());
        }
        if !self.index.contains_key(stem) {
            return Err(format!("no model {stem:?} in models/index.json"));
        }
        let d = Arc::new(ModelDef::load(&self.assets, stem)?);
        self.defs.lock().unwrap().insert(stem.to_owned(), d.clone());
        Ok(d)
    }

    /// `modeldef_load(filenum)`: a model by PD's `FILE_*` number.
    pub fn by_filenum(&self, filenum: u32) -> Result<Arc<ModelDef>, String> {
        let stem = self.by_filenum.get(&filenum).ok_or_else(|| format!("no model for FILE {filenum:#x}"))?.clone();
        self.get(&stem)
    }

    pub fn stem_of(&self, filenum: u32) -> Option<&str> {
        self.by_filenum.get(&filenum).map(String::as_str)
    }
}

// ─── g_HeadsAndBodies ────────────────────────────────────────────────────────

/// One `g_HeadsAndBodies` row (`modeldata/robot.c:64`), `data/bodies.json`.
#[derive(Deserialize, Debug, Clone)]
pub struct HeadOrBody {
    /// `BODY_*` / `HEAD_*`.
    pub num: usize,
    pub ismale: bool,
    pub unk00_01: bool,
    pub canvaryheight: bool,
    /// `HEADBODYTYPE_*`.
    #[serde(rename = "type")]
    pub ty: i32,
    /// cm.
    pub height: i32,
    pub filenum: u32,
    pub file: String,
    pub stem: Option<String>,
    pub scale: f32,
    pub animscale: f32,
    pub handfilenum: u32,
    pub hand: Option<String>,
}

impl HeadOrBody {
    /// `body_instantiate_model_to_addr` (`body.c:170`): the model scale, without
    /// cheats or the MP height variation.
    pub fn model_scale(&self) -> f32 {
        self.scale * 0.100_000_01
    }
}

/// One `g_MpBodies` row (`mplayer.c:1835`): a body the menus offer.
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct MpBody {
    /// `BODY_*`.
    pub bodynum: i32,
    /// `HEAD_*`, or 1000: a random head from `g_MpMaleHeads` /
    /// `g_MpFemaleHeads` by the body's sex (`mp_get_mpheadnum_by_mpbodynum`,
    /// `mplayer.c:2560`).
    pub headnum: i32,
    pub requirefeature: i32,
}

/// One `g_MpHeads` row (`mplayer.c:1674`).
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct MpHead {
    /// `HEAD_*`.
    pub headnum: i32,
    pub requirefeature: i32,
}

/// `data/bodies.json`: `g_HeadsAndBodies` (indexed by `BODY_*` / `HEAD_*`),
/// `g_MpBodies`, `g_MpHeads`, `g_MpMaleHeads` and `g_MpFemaleHeads`.
#[derive(Deserialize, Debug, Clone)]
pub struct Bodies {
    pub rows: Vec<HeadOrBody>,
    pub mpbodies: Vec<MpBody>,
    pub mpheads: Vec<MpHead>,
    pub maleheads: Vec<i32>,
    pub femaleheads: Vec<i32>,
}

impl Bodies {
    pub fn load(assets: &AssetDir) -> Result<Bodies, String> {
        assets.read_json(&assets.data("bodies.json"))
    }

    /// The `g_HeadsAndBodies` row whose model is `stem`.
    pub fn by_stem(&self, stem: &str) -> Option<&HeadOrBody> {
        self.rows.iter().find(|r| r.stem.as_deref() == Some(stem))
    }

    /// The head an MP body wears (`mp_get_mpheadnum_by_mpbodynum`): its own
    /// `g_MpBodies` head, or for 1000 the `pick`th of the male or female heads
    /// (PD picks with `random()`). `None` if `bodynum` is not an MP body.
    pub fn default_head(&self, bodynum: usize, pick: usize) -> Option<&HeadOrBody> {
        let mp = self.mpbodies.iter().find(|b| b.bodynum == bodynum as i32)?;
        let headnum = if mp.headnum == 1000 {
            let list = if self.rows.get(bodynum)?.ismale { &self.maleheads } else { &self.femaleheads };
            *list.get(pick % list.len().max(1))?
        } else {
            mp.headnum
        };
        self.rows.get(usize::try_from(headnum).ok()?)
    }
}

/// `g_HeadsAndBodies`, indexed by `BODY_*` / `HEAD_*`.
pub fn load_heads_and_bodies(assets: &AssetDir) -> Result<Vec<HeadOrBody>, String> {
    Ok(Bodies::load(assets)?.rows)
}
