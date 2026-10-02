//! The stage files, in exactly the formats `tools/pd-assets/pd_stage.py`
//! writes for PD's arenas (its docstring is the reference): `bg.json` +
//! `bg.bin` (the one model format, `pd_core::model`), `tiles.json`,
//! `pads.json`, `setup.json`; the level's textures beside them; and its entry
//! in `custom/levels.json`.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;

use glam::Vec3;
use pd_core::ids::*;
use serde_json::{json, Value};

use crate::recipe::Recipe;
use crate::rooms::Partition;
use crate::source::{Blend, LevelSource, Vert};

pub const EXPORTER: &str = "crates/pd_import";

/// The id a stage's own texture takes: `0x10000 | n` (`pd_core::model`: a
/// texture stored in the file rather than the pool). One per material, so
/// each carries its material's shot surface (the file may be shared).
fn texture_id(material: usize) -> u32 {
    0x10000 | material as u32
}

/// fast3d's combiner mux ids (`gbi.h:364-396`, as `pd_bg.py` numbers them).
const CC_TEXEL0: u8 = 1;
const CC_SHADE: u8 = 4;
/// "0" in the colour muxes a/b (4 bits) and d (3 bits).
const CC_ZERO_A: u8 = 15;
const CC_ZERO_D: u8 = 7;
const AC_TEXEL0: u8 = 1;
const AC_ONE: u8 = 6;
const AC_ZERO: u8 = 7;

pub fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let mut s = serde_json::to_string(v).map_err(|e| e.to_string())?;
    s.push('\n');
    std::fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
}

fn v3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// A material as the one model format holds it: `(TEXEL0 − 0) × SHADE + 0`,
/// alpha 1 or the texture's; `G_RM_FOG_SHADE_A` on a fog stage.
fn material_json(src: &LevelSource, m: usize, fog: bool) -> Value {
    let mat = &src.materials[m];
    let alpha = if mat.blend == Blend::Opaque { [AC_ZERO, AC_ZERO, AC_ZERO, AC_ONE] } else { [AC_ZERO, AC_ZERO, AC_ZERO, AC_TEXEL0] };
    let cyc = [CC_TEXEL0, CC_ZERO_A, CC_SHADE, CC_ZERO_D, alpha[0], alpha[1], alpha[2], alpha[3]];
    let combine: Vec<u8> = cyc.iter().chain(cyc.iter()).copied().collect();
    let texture = mat.texture.map(|_| {
        json!({"id": texture_id(m), "cms": mat.wrap[0] as u8, "cmt": mat.wrap[1] as u8, "shifts": 0, "shiftt": 0,
               "uls": 0.0, "ult": 0.0, "mipmap": false, "linear": true})
    });
    json!({
        "two_cycle": false, "combine": combine, "cull": if mat.cull_back { "back" } else { "none" },
        "lighting": false, "texgen": false, "texgen_linear": false, "texture": texture,
        "prim": [255, 255, 255, 255], "env": null, "fog": null,
        "blend": if mat.blend == Blend::Translucent { "alpha" } else { "opaque" },
        "ztest": true, "zwrite": mat.zwrite && mat.blend != Blend::Translucent, "decal": mat.decal,
        "alpha_test": if mat.blend == Blend::Cutout { "edge" } else { "none" },
        "fog_tint": false, "fog_shade": fog,
    })
}

/// The environment row: `g_FogEnvironments`' shape with fog, else
/// `g_NoFogEnvironments`' (`struct fogenvironment`, `nofogenvironment`).
fn env_json(r: &Recipe, src: &LevelSource) -> Value {
    let e = &src.env;
    let mut row = json!({
        "stage": format!("STAGE_CUSTOM_{}", r.code.to_uppercase()), "near": e.near, "far": e.far,
        "opaperc": 0, "xluperc": 0, "refdist": 0,
        "sky_r": e.sky[0], "sky_g": e.sky[1], "sky_b": e.sky[2], "numsuns": 0, "suns": null,
        "clouds_enabled": 0, "clouds_r": 255, "clouds_g": 255, "clouds_b": 255, "clouds_scale": 5000, "clouds_type": "TEX_ENV_00",
        "water_enabled": 0, "water_r": 0, "water_g": 0, "water_b": 0, "water_scale": 0, "water_type": "TEX_ENV_00",
        "clouds_height": 0,
    });
    match e.fog {
        Some((min, max)) => {
            row["fogmin"] = json!(min);
            row["fogmax"] = json!(max);
            json!({"fog": true, "transparency": false, "provenance": "pd_import: the source's fog", "replaced_commands": 0, "fogenvironment": row})
        }
        None => {
            row["transparency"] = json!(0);
            json!({"fog": false, "transparency": false, "provenance": "pd_import", "replaced_commands": 0, "nofogenvironment": row})
        }
    }
}

/// `bg.json` + `bg.bin` and the textures: every triangle cut into its rooms,
/// one DL node per room and layer (every opaque node first, as PD's order
/// is), a batch per material, a colour table entry per vertex. Returns the
/// triangle count.
pub fn write_bg(dir: &Path, r: &Recipe, src: &LevelSource, part: &Partition) -> Result<usize, String> {
    let fog = src.env.fog.is_some();
    // Cut every triangle into its rooms: (room, xlu, material) -> triangles.
    let mut groups: HashMap<(u16, bool, usize), Vec<[Vert; 3]>> = HashMap::new();
    for t in &src.tris {
        for (room, piece) in part.cut(t.v.to_vec()) {
            let g = groups.entry((room, t.xlu, t.material)).or_default();
            for k in 1..piece.len() - 1 {
                g.push([piece[0], piece[k], piece[k + 1]]);
            }
        }
    }
    let nrooms = part.rooms() as u16;
    let mut nodes = vec![json!({"type": "position", "parent": -1, "pos": [0, 0, 0], "animpart": 0, "mtx": [0, -1, -1], "flags": 0})];
    let mut node_of: HashMap<(u16, bool), usize> = HashMap::new();
    for xlu in [false, true] {
        for room in 1..=nrooms {
            if groups.keys().any(|&(rm, x, _)| rm == room && x == xlu) {
                node_of.insert((room, xlu), nodes.len());
                nodes.push(json!({"type": "dl", "parent": 0, "room": room, "layer": if xlu { "xlu" } else { "opa" }, "rendermode": 0}));
            }
        }
    }
    let mut keys: Vec<_> = groups.keys().copied().collect();
    keys.sort_by_key(|&(room, xlu, m)| (node_of[&(room, xlu)], m));
    let mut blob: Vec<u8> = Vec::new();
    let mut batches = Vec::new();
    let mut colours: HashMap<u16, usize> = HashMap::new();
    let mut room_box: HashMap<u16, (Vec3, Vec3)> = HashMap::new();
    let mut ntris = 0;
    for key in keys {
        let (room, xlu, m) = key;
        let tris = &groups[&key];
        let tex = src.materials[m].texture.map(|t| &src.textures[t]);
        let (tw, th) = tex.map_or((1.0, 1.0), |t| (t.w as f32, t.h as f32));
        // Vertices shared within the batch.
        let mut verts: Vec<Vert> = Vec::new();
        let mut index: HashMap<[u32; 8], u16> = HashMap::new();
        let mut idx: Vec<u16> = Vec::new();
        for t in tris {
            for v in t {
                let k = [v.pos.x.to_bits(), v.pos.y.to_bits(), v.pos.z.to_bits(), v.uv.x.to_bits(), v.uv.y.to_bits(), u32::from_le_bytes(v.col), 0, 0];
                let i = *index.entry(k).or_insert_with(|| {
                    verts.push(*v);
                    (verts.len() - 1) as u16
                });
                idx.push(i);
            }
        }
        if verts.len() > 0xffff {
            return Err(format!("room {room}: a batch of {} vertices (u16 indices)", verts.len()));
        }
        ntris += idx.len() / 3;
        let first = *colours.get(&room).unwrap_or(&0);
        let cidx: Vec<usize> = (first..first + verts.len()).collect();
        colours.insert(room, first + verts.len());
        for v in &verts {
            blob.extend_from_slice(&v.pos.x.to_le_bytes());
            blob.extend_from_slice(&v.pos.y.to_le_bytes());
            blob.extend_from_slice(&v.pos.z.to_le_bytes());
            blob.extend_from_slice(&0u16.to_le_bytes());
            blob.extend_from_slice(&(v.uv.x * tw).to_le_bytes());
            blob.extend_from_slice(&(v.uv.y * th).to_le_bytes());
            blob.extend_from_slice(&v.col);
            blob.extend_from_slice(&[0, 0]);
            let e = room_box.entry(room).or_insert((v.pos, v.pos));
            *e = (e.0.min(v.pos), e.1.max(v.pos));
        }
        for i in &idx {
            blob.extend_from_slice(&i.to_le_bytes());
        }
        batches.push(json!({"node": node_of[&(room, xlu)], "material": m, "nverts": verts.len(), "nidx": idx.len(), "leaf": 0, "cidx": cidx}));
    }

    // The textures, one file each; one id per material.
    let texdir = dir.join("tex");
    std::fs::create_dir_all(&texdir).map_err(|e| format!("{}: {e}", texdir.display()))?;
    for (i, t) in src.textures.iter().enumerate() {
        std::fs::write(texdir.join(format!("{i:03}.png")), &t.png).map_err(|e| format!("{}: {e}", texdir.display()))?;
    }
    let mut textures = serde_json::Map::new();
    for (m, mat) in src.materials.iter().enumerate() {
        if let Some(t) = mat.texture {
            let tx = &src.textures[t];
            textures.insert(
                texture_id(m).to_string(),
                json!({"file": format!("stages/{}/tex/{t:03}.png", r.code), "w": tx.w, "h": tx.h, "cfg_w": tx.w, "cfg_h": tx.h, "levels": 1,
                       "source": "import", "surfacetype": mat.surface, "soundsurfacetype": mat.surface}),
            );
        }
    }
    let materials: Vec<Value> = (0..src.materials.len()).map(|m| material_json(src, m, fog)).collect();

    let rooms: Vec<Value> = (1..=nrooms)
        .map(|room| {
            let (lo, hi) = part.bbox(room);
            let (glo, ghi) = room_box.get(&room).copied().map_or((Value::Null, Value::Null), |(a, b)| (v3(a), v3(b)));
            let n = *colours.get(&room).unwrap_or(&0);
            json!({"room": room, "pos": v3((lo + hi) * 0.5), "bbmin": v3(lo), "bbmax": v3(hi), "gfx_bbmin": glo, "gfx_bbmax": ghi,
                   "br_light_min": 128, "br_light_max": 255, "numlights": 0, "lightindex": -1,
                   "opa_node": node_of.get(&(room, false)), "xlu_node": node_of.get(&(room, true)),
                   "numvertices": n, "numcolours": n, "colour_alpha_only": [], "bsp_parents": 0})
        })
        .collect();
    let portals: Vec<Value> = part.portals().iter().map(|p| json!({"rooms": p.rooms, "flags": 0, "verts": p.verts.iter().map(|&v| v3(v)).collect::<Vec<_>>()})).collect();
    let head = json!({
        "format": "pd-model/1", "name": format!("bg_{}", r.code), "stem": "bg", "filenum": 0, "file": "custom",
        "source": format!("{}: pd_import", r.code), "exporter": EXPORTER, "skel": null, "nummatrices": 1, "scale": 1.0,
        "nodes": nodes, "parts": {}, "materials": materials, "textures": textures, "batches": batches,
        "vertex": {"size": 28, "fields": ["x:f32", "y:f32", "z:f32", "mtx:u16", "u:f32", "v:f32", "c0:u8", "c1:u8", "c2:u8", "c3:u8", "flags:u8", "pad:u8"], "flags": {"lit": 1, "texgen": 2}},
        "stage": format!("STAGE_CUSTOM_{}", r.code.to_uppercase()), "units": "cm (world)",
        "env": env_json(r, src), "rooms": rooms, "portals": portals, "bgcmds": [[0, 1, 0]], "lights": [], "section2_textures": [],
    });
    write_json(&dir.join("bg.json"), &head)?;
    let mut f = std::fs::File::create(dir.join("bg.bin")).map_err(|e| format!("bg.bin: {e}"))?;
    f.write_all(&blob).map_err(|e| format!("bg.bin: {e}"))?;
    Ok(ntris)
}

/// `tiles.json`: every collision polygon cut into its rooms, rooms ascending.
/// Returns the tile count.
pub fn write_tiles(dir: &Path, r: &Recipe, src: &LevelSource, part: &Partition) -> Result<usize, String> {
    let mut tiles: Vec<(u16, Value)> = Vec::new();
    for p in &src.collision {
        for (room, piece) in part.cut(p.verts.clone()) {
            tiles.push((room, json!({"room": room, "flags": p.flags, "floortype": p.floortype, "floorcol": p.floorcol, "verts": piece.iter().map(|&v| v3(v)).collect::<Vec<_>>()})));
        }
    }
    tiles.sort_by_key(|t| t.0);
    let n = tiles.len();
    write_json(
        &dir.join("tiles.json"),
        &json!({"format": "pd-tiles/1", "source": format!("{}: pd_import", r.code), "exporter": EXPORTER,
                "rooms": (1..=part.rooms()).collect::<Vec<_>>(), "tiles": tiles.into_iter().map(|t| t.1).collect::<Vec<_>>()}),
    )?;
    Ok(n)
}

/// One `pads.json` pad.
#[derive(Clone, Debug)]
pub struct PadRow {
    pub pos: Vec3,
    pub look: Vec3,
    pub up: Vec3,
    pub flags: u32,
    /// `[xmin, xmax, ymin, ymax, zmin, zmax]`, read with `PADFLAG_HASBBOXDATA`.
    pub bbox: [f32; 6],
}

impl PadRow {
    /// A pad standing at `pos` facing `look`, with the default box.
    pub fn at(pos: Vec3, look: Vec3, flags: u32) -> PadRow {
        PadRow { pos, look, up: Vec3::Y, flags, bbox: [-100.0, 100.0, -100.0, 100.0, -100.0, 100.0] }
    }
}

/// The gameplay half: pads (waypoints' and items'), the waypoint graph, cover,
/// and the setup's `intro[]` and `props[]`. A source that brings its own
/// (GoldenEye's doors) hands them to placement first, which adds to them.
#[derive(Default)]
pub struct Gameplay {
    pub pads: Vec<PadRow>,
    /// `(pad, group, neighbours)`.
    pub waypoints: Vec<(usize, usize, Vec<i32>)>,
    pub waygroups: Vec<Vec<i32>>,
    /// `(pos, look)`.
    pub cover: Vec<(Vec3, Vec3)>,
    pub intro: Vec<Value>,
    pub props: Vec<Value>,
}

impl Gameplay {
    /// As `pads.json`'s `pads`, `waypoints`, `waygroups` and `cover`, and
    /// `setup.json`'s `intro` and `props`, in one object.
    pub fn to_json(&self) -> Value {
        let pads: Vec<Value> = self.pads.iter().map(|p| json!({"pos": v3(p.pos), "look": v3(p.look), "up": v3(p.up), "flags": p.flags, "bbox": p.bbox, "liftnum": 0})).collect();
        let waypoints: Vec<Value> = self.waypoints.iter().map(|(pad, group, n)| json!({"pad": pad, "group": group, "neighbours": n})).collect();
        let waygroups: Vec<Value> = self.waygroups.iter().map(|n| json!({"neighbours": n})).collect();
        let cover: Vec<Value> = self.cover.iter().map(|(p, l)| json!({"pos": v3(*p), "look": v3(*l), "special": 0})).collect();
        json!({"pads": pads, "waypoints": waypoints, "waygroups": waygroups, "cover": cover, "intro": self.intro, "props": self.props})
    }

    /// Back from [`Gameplay::to_json`] (or `pads.json` and `setup.json`
    /// merged).
    pub fn from_json(v: &Value) -> Result<Gameplay, String> {
        let f3 = |x: &Value| -> Vec3 { Vec3::new(x[0].as_f64().unwrap_or(0.0) as f32, x[1].as_f64().unwrap_or(0.0) as f32, x[2].as_f64().unwrap_or(0.0) as f32) };
        let arr = |k: &str| v[k].as_array().cloned().unwrap_or_default();
        let ints = |x: &Value| -> Vec<i32> { x.as_array().into_iter().flatten().filter_map(|n| n.as_i64()).map(|n| n as i32).collect() };
        let pads = arr("pads")
            .iter()
            .map(|p| PadRow { pos: f3(&p["pos"]), look: f3(&p["look"]), up: f3(&p["up"]), flags: p["flags"].as_u64().unwrap_or(0) as u32, bbox: std::array::from_fn(|k| p["bbox"][k].as_f64().unwrap_or(0.0) as f32) })
            .collect();
        let waypoints = arr("waypoints").iter().map(|w| (w["pad"].as_u64().unwrap_or(0) as usize, w["group"].as_u64().unwrap_or(0) as usize, ints(&w["neighbours"]))).collect();
        let waygroups = arr("waygroups").iter().map(|g| ints(&g["neighbours"])).collect();
        let cover = arr("cover").iter().map(|c| (f3(&c["pos"]), f3(&c["look"]))).collect();
        Ok(Gameplay { pads, waypoints, waygroups, cover, intro: arr("intro"), props: arr("props") })
    }
}

pub fn write_pads(dir: &Path, r: &Recipe, g: &Gameplay) -> Result<(), String> {
    let v = g.to_json();
    write_json(
        &dir.join("pads.json"),
        &json!({"format": "pd-pads/1", "source": format!("{}: pd_import", r.code), "exporter": EXPORTER, "pads": v["pads"], "waypoints": v["waypoints"], "waygroups": v["waygroups"], "cover": v["cover"]}),
    )
}

/// `setup.json`. The stage table row carries what the world reads
/// (`eraserpropdist`, `unk30`: the arenas' 400, 0); the background AI list is
/// the arenas' one (every MP setup's simulant set-up, `mp_setupref.c`).
pub fn write_setup(dir: &Path, r: &Recipe, g: &Gameplay) -> Result<(), String> {
    let name = format!("STAGE_CUSTOM_{}", r.code.to_uppercase());
    let bgai = json!([{"id": 0x1000, "cmds": [{"type": "mp_init_simulants"}, {"type": "rebuild_teams"}, {"type": "rebuild_squadrons"}, {"type": "set_ailist", "chr": 253, "ailist": 0}]}]);
    write_json(
        &dir.join("setup.json"),
        &json!({"format": "pd-setup/1", "source": format!("{}: pd_import", r.code), "exporter": EXPORTER,
                "stage": {"name": name, "num": r.stagenum, "code": r.code, "table": {"id": name, "eraserpropdist": 400, "unk30": 0}},
                "intro": g.intro, "props": g.props, "bgai": bgai}),
    )
}

/// Add (or replace) the level in `custom/levels.json`.
pub fn register(root: &Path, r: &Recipe) -> Result<(), String> {
    let path = root.join("levels.json");
    let mut levels: Vec<Value> = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str::<Value>(&t).map_err(|e| format!("{}: {e}", path.display()))?["levels"].as_array().cloned().unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    levels.retain(|l| l["code"] != r.code.as_str() && l["stagenum"] != r.stagenum);
    let mut row = json!({"code": r.code, "stagenum": r.stagenum, "name": r.name});
    if let Some(g) = r.group.as_ref().filter(|g| !g.is_empty()) {
        row["group"] = json!(g);
    }
    levels.push(row);
    levels.sort_by_key(|l| l["stagenum"].as_u64());
    write_json(&path, &json!({"format": "pd-custom-levels/1", "levels": levels}))
}

/// `PADFLAG_*` bits of a generated waypoint's flags.
pub fn pad_flags(f: &pd_sim::nav::PadFlags) -> u32 {
    (f.walkdirect as u32 * PADFLAG_AIWALKDIRECT) | (f.crouch as u32 * PADFLAG_AICROUCH) | (f.duck as u32 * PADFLAG_AIDUCK) | (f.ignorey as u32 * PADFLAG_AIIGNOREY)
}
