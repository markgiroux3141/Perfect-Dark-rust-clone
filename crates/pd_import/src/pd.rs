//! One of PD's own arenas, rebuilt in another tool. The Blender MCP repo
//! imports an arena's rooms into Blender (one object per room), changes and
//! adds to them, and writes a glTF back: a node per room (`extras.ge_room`),
//! each material naming the PD texture it draws (`extras.ge_preset`), the
//! baked light in `COLOR_0`. The export is the drawn geometry only: no
//! clipping, no portals, no setup.
//!
//! So the rebuilt level is laid over the arena it came from
//! (`assets/stages/<base>/`), room by room:
//!
//! - a room whose triangles are the arena's (by position) keeps the arena's
//!   own data: its batches, byte for byte, its tiles, its portals;
//! - a room the tool changed is drawn from the glTF, and keeps the arena's
//!   tiles but those resting on geometry it took away; its new triangles
//!   become tiles ([`tile_for`]);
//! - a room the tool added is drawn from the glTF, every triangle a tile;
//! - wherever a changed or added room's open edges run along another room's,
//!   the opening between them is a portal: the convex hull of those
//!   stretches in their plane ([`openings`]). On the arena's own rooms this
//!   finds every one of its portals (`tests.rs`).
//!
//! Every material is the arena's own (the one drawing the same texture with
//! the same culling in the same layer), so a new room draws as an old one
//! does. The arena's setup is kept whole, on its own pads (its spawns,
//! weapons, ammo, hills, bases, cover); only the waypoints are made anew, over
//! the whole level ([`crate::place::route`]).

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_core::model::ModelDef;
use serde_json::{json, Value};

use crate::glb::Glb;
use crate::recipe::{PdBase, PdSource, Recipe};
use crate::source::{srgb8, Marker, MarkerKind};
use crate::write::{write_json, Gameplay, PadRow, EXPORTER};
use crate::{Paths, SourceHow};


/// A drawn triangle of the rebuilt level, in the arena's terms.
#[derive(Clone, Copy, Debug)]
struct Tri {
    pos: [Vec3; 3],
    /// Texels.
    uv: [[f32; 2]; 3],
    col: [[u8; 4]; 3],
    /// Into the arena's materials.
    material: usize,
    xlu: bool,
}

/// A triangle by its corners, rounded to the centimetre, in order.
type Key = [[i32; 3]; 3];

/// A portal found: its two rooms (lower first; as `room1`, `room2` once
/// [`orient`]ed) and its corners.
type Opening = ([u16; 2], Vec<Vec3>);

/// A rebuilt room's layer: its triangles by material.
type Layer = Vec<(usize, Vec<Tri>)>;

fn round(v: Vec3) -> [i32; 3] {
    [v.x.round() as i32, v.y.round() as i32, v.z.round() as i32]
}

fn key(p: &[Vec3; 3]) -> Key {
    let mut k = p.map(round);
    k.sort();
    k
}

fn v3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn vec3(v: &Value) -> Vec3 {
    Vec3::new(v[0].as_f64().unwrap_or(0.0) as f32, v[1].as_f64().unwrap_or(0.0) as f32, v[2].as_f64().unwrap_or(0.0) as f32)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// What became of a room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Same,
    Changed,
    Added,
}

/// Where a stage sits in the fused level: turned about the vertical, then
/// moved ([`PdBase`]). In f64, so an unturned stage lands exactly.
#[derive(Clone, Copy, Debug)]
struct Xf {
    cos: f64,
    sin: f64,
    offset: [f64; 3],
}

impl Xf {
    fn of(b: &PdBase) -> Xf {
        let t = b.turn.to_radians();
        Xf { cos: t.cos(), sin: t.sin(), offset: b.offset }
    }

    fn turned(&self) -> bool {
        self.sin != 0.0 || self.cos != 1.0
    }

    fn turn(&self, v: Vec3) -> [f64; 3] {
        let (x, y, z) = (v.x as f64, v.y as f64, v.z as f64);
        [x * self.cos + z * self.sin, y, z * self.cos - x * self.sin]
    }

    fn dir(&self, v: Vec3) -> Vec3 {
        let d = self.turn(v);
        Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32)
    }

    fn point(&self, v: Vec3) -> Vec3 {
        let d = self.turn(v);
        Vec3::new((d[0] + self.offset[0]) as f32, (d[1] + self.offset[1]) as f32, (d[2] + self.offset[2]) as f32)
    }

    /// A box moved: the box round its corners (the same box unturned; its
    /// centre is the moved centre either way).
    fn aabb(&self, lo: Vec3, hi: Vec3) -> (Vec3, Vec3) {
        (0..8)
            .map(|i| self.point(Vec3::new([lo.x, hi.x][i & 1], [lo.y, hi.y][i >> 1 & 1], [lo.z, hi.z][i >> 2 & 1])))
            .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(p), b.max(p)))
    }
}

/// The arena as `pd_stage.py` exported it; or several stages, moved to where
/// the fused level has them, as one ([`Base::merge`]).
struct Base {
    bg: Value,
    def: ModelDef,
    tiles: Vec<Value>,
    pads: Value,
    setup: Value,
    /// Room -> its batches (indices into `def.batches`), in file order.
    batches: HashMap<u16, Vec<usize>>,
    /// Batch -> its room and layer (xlu).
    batch_room: Vec<(u16, bool)>,
    /// Each stage's rooms (first, last) in the level's numbering, and each
    /// material's stage: a rebuilt room draws on its own stage's materials.
    part_rooms: Vec<(u16, u16)>,
    mat_part: Vec<usize>,
    /// The asset root each texture is under (`assets/`, or the export cache).
    tex_root: HashMap<u32, PathBuf>,
    /// The spots the stages' MP setups mark (spawns, weapons), moved.
    markers: Vec<Marker>,
}

impl Base {
    fn load(root: &Path, code: &str) -> Result<Base, String> {
        let dir = root.join("stages").join(code);
        let bg = read_json(&dir.join("bg.json"))?;
        let def = ModelDef::load_file(&AssetDir::new(root), &dir.join("bg.json"), &dir.join("bg.bin"))?;
        let nodes = bg["nodes"].as_array().ok_or("bg.json: no nodes")?;
        let mut batches: HashMap<u16, Vec<usize>> = HashMap::new();
        let mut batch_room = Vec::new();
        for (i, b) in def.batches.iter().enumerate() {
            let n = &nodes[b.node];
            let room = n["room"].as_u64().ok_or_else(|| format!("{code}: batch {i} is not on a room's node"))? as u16;
            batches.entry(room).or_default().push(i);
            batch_room.push((room, n["layer"] == "xlu"));
        }
        let tiles = read_json(&dir.join("tiles.json"))?["tiles"].as_array().cloned().unwrap_or_default();
        // A stage exported only to build on has no pads or setup.
        let optional = |f: &str| if dir.join(f).exists() { read_json(&dir.join(f)) } else { Ok(Value::Null) };
        let nrooms = bg["rooms"].as_array().map_or(0, |a| a.len()) as u16;
        let nmats = def.materials.len();
        let tex_root = def.textures.keys().map(|&id| (id, root.to_path_buf())).collect();
        Ok(Base {
            pads: optional("pads.json")?,
            setup: optional("setup.json")?,
            bg,
            def,
            tiles,
            batches,
            batch_room,
            part_rooms: vec![(1, nrooms)],
            mat_part: vec![0; nmats],
            tex_root,
            markers: Vec::new(),
        })
    }

    /// Move this stage to where the fused level has it: turned and moved by
    /// `xf`, its rooms numbered from `first`, its materials and lights after
    /// `mats` and `lights` others'. Its MP setup's spots become markers.
    fn place(&mut self, xf: Xf, first: u16, mats: usize, lights: usize) {
        let dr = first - 1;
        for b in &mut self.def.batches {
            b.material += mats;
            for v in &mut b.verts {
                v.pos = xf.point(v.pos);
                // A lit vertex's colour is its normal.
                if v.flags & 1 != 0 && xf.turned() {
                    let n = xf.dir(Vec3::new(v.c[0] as i8 as f32, v.c[1] as i8 as f32, v.c[2] as i8 as f32));
                    for k in 0..3 {
                        v.c[k] = n[k].round().clamp(-128.0, 127.0) as i8 as u8;
                    }
                }
            }
        }
        for row in self.bg["batches"].as_array_mut().into_iter().flatten() {
            row["material"] = json!(row["material"].as_u64().unwrap_or(0) as usize + mats);
        }
        for r in &mut self.batch_room {
            r.0 += dr;
        }
        self.batches = std::mem::take(&mut self.batches).into_iter().map(|(r, v)| (r + dr, v)).collect();
        let boxed = |row: &mut Value, lo: &str, hi: &str| {
            if row[lo].is_array() {
                let (a, b) = xf.aabb(vec3(&row[lo]), vec3(&row[hi]));
                row[lo] = v3(a);
                row[hi] = v3(b);
            }
        };
        for row in self.bg["rooms"].as_array_mut().into_iter().flatten() {
            row["room"] = json!(row["room"].as_u64().unwrap_or(0) as u16 + dr);
            row["pos"] = v3(xf.point(vec3(&row["pos"])));
            boxed(row, "bbmin", "bbmax");
            boxed(row, "gfx_bbmin", "gfx_bbmax");
            if let Some(i) = row["lightindex"].as_i64().filter(|&i| i >= 0) {
                row["lightindex"] = json!(i as usize + lights);
            }
        }
        for p in self.bg["portals"].as_array_mut().into_iter().flatten() {
            p["rooms"] = json!([p["rooms"][0].as_u64().unwrap_or(0) as u16 + dr, p["rooms"][1].as_u64().unwrap_or(0) as u16 + dr]);
            p["verts"] = json!(p["verts"].as_array().into_iter().flatten().map(|v| v3(xf.point(vec3(v)))).collect::<Vec<_>>());
        }
        for l in self.bg["lights"].as_array_mut().into_iter().flatten() {
            l["roomnum"] = json!(l["roomnum"].as_u64().unwrap_or(0) as u16 + dr);
            // A light's box is relative to its room's `pos` (`lights.rs`): only turned.
            if xf.turned() {
                let int = |v: Vec3| json!([v.x.round() as i32, v.y.round() as i32, v.z.round() as i32]);
                l["bbox"] = json!(l["bbox"].as_array().into_iter().flatten().map(|v| int(xf.dir(vec3(v)))).collect::<Vec<_>>());
                l["dir"] = int(xf.dir(vec3(&l["dir"])).clamp(Vec3::splat(-128.0), Vec3::splat(127.0)));
            }
        }
        for t in &mut self.tiles {
            t["room"] = json!(t["room"].as_u64().unwrap_or(0) as u16 + dr);
            t["verts"] = json!(t["verts"].as_array().into_iter().flatten().map(|v| v3(xf.point(vec3(v)))).collect::<Vec<_>>());
        }
        let pads: Vec<Value> = self.pads["pads"].as_array().cloned().unwrap_or_default();
        let mut markers = Vec::new();
        for (kind, list, want) in [(MarkerKind::Spawn, "intro", "spawn"), (MarkerKind::Item, "props", "weapon")] {
            for row in self.setup[list].as_array().into_iter().flatten().filter(|r| r["type"] == want) {
                // An intro row's pad is `pad`; a prop's, `chr` (`props.h`'s name).
                if let Some(p) = ["pad", "chr"].iter().find_map(|k| row[*k].as_u64().and_then(|n| pads.get(n as usize))) {
                    let look = xf.dir(vec3(&p["look"]));
                    markers.push(Marker { kind, pos: xf.point(vec3(&p["pos"])), facing: look.x.atan2(look.z) });
                }
            }
        }
        self.markers = markers;
        for p in self.pads["pads"].as_array_mut().into_iter().flatten() {
            p["pos"] = v3(xf.point(vec3(&p["pos"])));
            p["look"] = v3(xf.dir(vec3(&p["look"])));
            p["up"] = v3(xf.dir(vec3(&p["up"])));
        }
        for c in self.pads["cover"].as_array_mut().into_iter().flatten() {
            c["pos"] = v3(xf.point(vec3(&c["pos"])));
            c["look"] = v3(xf.dir(vec3(&c["look"])));
        }
        self.part_rooms = vec![(first, first + self.part_rooms[0].1 - 1)];
    }

    /// Stages already [`Base::place`]d, as one: the first's header (its
    /// environment, its root node), everyone's materials, textures, batches,
    /// rooms, portals, lights, tiles and markers. Only the first's pads and
    /// setup (a fused level's gameplay data is generated).
    fn merge(mut parts: Vec<Base>) -> Result<Base, String> {
        let mut out = parts.remove(0);
        for (k, p) in parts.into_iter().enumerate() {
            let nb = out.def.batches.len();
            out.def.materials.extend(p.def.materials);
            out.mat_part.extend(p.mat_part.iter().map(|_| k + 1));
            out.def.batches.extend(p.def.batches);
            out.batch_room.extend(p.batch_room);
            for (room, bs) in p.batches {
                out.batches.entry(room).or_default().extend(bs.into_iter().map(|b| b + nb));
            }
            for (id, t) in p.def.textures {
                out.def.textures.entry(id).or_insert(t);
            }
            for (id, r) in p.tex_root {
                out.tex_root.entry(id).or_insert(r);
            }
            for (id, t) in p.bg["textures"].as_object().into_iter().flatten() {
                if out.bg["textures"].get(id).is_none() {
                    out.bg["textures"][id] = t.clone();
                }
            }
            for key in ["materials", "batches", "rooms", "portals", "lights", "section2_textures"] {
                let more = p.bg[key].as_array().cloned().unwrap_or_default();
                let into = out.bg[key].as_array_mut().ok_or_else(|| format!("bg.json: no {key}"))?;
                for v in more {
                    if key != "section2_textures" || !into.contains(&v) {
                        into.push(v);
                    }
                }
            }
            out.tiles.extend(p.tiles);
            out.part_rooms.extend(p.part_rooms);
            out.markers.extend(p.markers);
        }
        for (i, row) in out.bg["rooms"].as_array().into_iter().flatten().enumerate() {
            if row["room"].as_u64() != Some(i as u64 + 1) {
                return Err(format!("the stages' rooms don't run on from each other: room {} is at {} (the bases' first_room)", row["room"], i + 1));
            }
        }
        Ok(out)
    }

    /// The stage a room of the level came from.
    fn part_of(&self, room: u16) -> Option<usize> {
        self.part_rooms.iter().position(|&(a, b)| (a..=b).contains(&room))
    }

    fn tris(&self, room: u16) -> impl Iterator<Item = ([Vec3; 3], usize)> + '_ {
        self.batches.get(&room).into_iter().flatten().flat_map(move |&bi| {
            let b = &self.def.batches[bi];
            b.idx.chunks_exact(3).map(move |t| ([0, 1, 2].map(|k| b.verts[t[k] as usize].pos), bi))
        })
    }

    fn texture_size(&self, id: u32) -> Option<(f32, f32)> {
        let t = &self.bg["textures"][id.to_string()];
        Some((t["w"].as_f64()? as f32, t["h"].as_f64()? as f32))
    }
}

/// Where a stage's export is: `assets/` for an arena; for another of PD's
/// stages, `custom/cache/pd/`, exported from the decomp the first time
/// (`tools/pd-assets/pd_stage.py --base`, a few seconds).
fn stage_root(paths: &Paths, b: &PdBase) -> Result<PathBuf, String> {
    let Some(stage) = &b.stage else { return Ok(paths.assets.clone()) };
    let root = paths.custom.join("cache").join("pd");
    if root.join("stages").join(&b.code).join("bg.json").exists() {
        return Ok(root);
    }
    let python = std::env::var_os("PYTHON").unwrap_or_else(|| "python".into());
    let out = std::process::Command::new(&python)
        .arg(crate::ge::repo().join("tools").join("pd-assets").join("pd_stage.py"))
        .args(["--base", &b.code, stage])
        .env("PD_ASSETS_OUT", &root)
        .output()
        .map_err(|e| format!("{}: {e} (set PYTHON)", python.to_string_lossy()))?;
    if !out.status.success() {
        return Err(format!("pd_stage.py --base {} {stage}: {}", b.code, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(root)
}

/// The level's stages, loaded, placed and merged.
fn load_bases(paths: &Paths, parts: &[PdBase]) -> Result<Base, String> {
    let (mut mats, mut lights) = (0, 0);
    let mut loaded = Vec::new();
    for b in parts {
        let mut base = Base::load(&stage_root(paths, b)?, &b.code)?;
        base.place(Xf::of(b), b.first_room, mats, lights);
        mats += base.def.materials.len();
        lights += base.bg["lights"].as_array().map_or(0, |a| a.len());
        loaded.push(base);
    }
    Base::merge(loaded)
}

/// The glTF's rooms, every triangle on the arena's material for it (its own
/// stage's, in a fused level). A node naming its stage and its room there
/// (`extras.ge_level`, `ge_room_src`) must be where the recipe's `bases` put
/// that room.
fn read_glb(path: &Path, base: &Base, parts: &[PdBase]) -> Result<HashMap<u16, Vec<Tri>>, String> {
    let glb = Glb::load(path)?;
    let layers: HashSet<(usize, bool)> = base.def.batches.iter().zip(&base.batch_room).map(|(b, &(_, xlu))| (b.material, xlu)).collect();
    let gmats = glb.json["materials"].as_array().cloned().unwrap_or_default();
    // (glTF material, the room's stage) -> (the arena's material, xlu, texture size).
    type MatOf = (usize, bool, f32, f32);
    let mut mats: HashMap<(usize, Option<usize>), MatOf> = HashMap::new();
    let mut material = |i: usize, part: Option<usize>| -> Result<(usize, bool, f32, f32), String> {
        if let Some(&m) = mats.get(&(i, part)) {
            return Ok(m);
        }
        let m = gmats.get(i).ok_or("a glTF primitive's material is out of range")?;
        let preset = m["extras"]["ge_preset"].as_str().and_then(|s| u32::from_str_radix(s, 16).ok()).ok_or_else(|| format!("glTF material {i} names no texture (extras.ge_preset)"))?;
        let both = m["doubleSided"].as_bool().unwrap_or(false);
        let xlu = m["alphaMode"] == "BLEND";
        // The same texture and culling; then the room's own stage, the same
        // layer, and a material that isn't lit or texgen (the glTF's colours
        // are colours, not normals).
        let pick = base
            .def
            .materials
            .iter()
            .enumerate()
            .filter(|(_, bm)| bm.texture.as_ref().is_some_and(|t| t.id == preset) && (bm.cull == pd_core::model::MatCull::None) == both)
            .min_by_key(|&(k, bm)| (part.is_some_and(|p| base.mat_part[k] != p), !layers.contains(&(k, xlu)), bm.lighting || bm.texgen, k))
            .map(|(k, _)| k);
        let k = pick.ok_or_else(|| format!("glTF material {i} ({}): the arena has none drawing texture {preset:#06x}", m["name"]))?;
        // The UVs are the glTF image's, whose rows may be padded (a 54-texel
        // wide texture in a 56-pixel image: the Blender repo's export), which
        // PD's own texels follow; the texture's size without an image.
        let image = m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64().and_then(|t| glb.json["textures"][t as usize]["source"].as_u64());
        let (w, h) = match image {
            Some(im) => {
                let bytes = glb.image_bytes(im as usize)?;
                let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| format!("glTF image {im}: {e}"))?.into_dimensions().map_err(|e| format!("glTF image {im}: {e}"))?;
                (w as f32, h as f32)
            }
            None => base.texture_size(preset).ok_or_else(|| format!("texture {preset:#06x} is not in the arena's bg.json"))?,
        };
        mats.insert((i, part), (k, xlu, w, h));
        Ok((k, xlu, w, h))
    };
    let mut rooms: HashMap<u16, Vec<Tri>> = HashMap::new();
    for node in glb.nodes() {
        let Some(room) = node["extras"]["ge_room"].as_u64() else { continue };
        let room = room as u16;
        if ["translation", "rotation", "scale", "matrix"].iter().any(|k| !node[*k].is_null()) {
            return Err(format!("glTF node {}: a transform (rooms are in world space)", node["name"]));
        }
        let part = base.part_of(room);
        if let Some(tag) = node["extras"]["ge_level"].as_str() {
            let src = node["extras"]["ge_room_src"].as_u64();
            let want = parts.iter().find(|b| b.tag == tag).map(|b| (b.first_room, src.map(|s| s as u16 + b.first_room - 1)));
            match (want, part.map(|p| &parts[p])) {
                (Some((_, Some(r))), Some(b)) if r == room && b.tag == tag => {}
                _ => return Err(format!("glTF node {}: room {room:#x} of {tag} (its room {src:?} there), which the recipe's bases put elsewhere", node["name"])),
            }
        }
        let Some(mesh) = node["mesh"].as_u64() else { continue };
        for prim in glb.json["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten() {
            let at = &prim["attributes"];
            let get = |name: &str| at[name].as_u64().ok_or_else(|| format!("glTF node {}: no {name}", node["name"])).and_then(|a| glb.floats(a as usize));
            let (pos, _) = get("POSITION")?;
            let (uv, _) = get("TEXCOORD_0")?;
            let (col, cc) = get("COLOR_0")?;
            let n = pos.len() / 3;
            let mi = prim["material"].as_u64().ok_or("a glTF primitive without a material")? as usize;
            let (material, xlu, w, h) = material(mi, part)?;
            let vert = |i: usize| {
                let c = |k: usize| if k < cc { col[i * cc + k] } else { 1.0 };
                // The alpha is linear; the colour sRGB (`srgb8`). V runs up in
                // the glTF, down the texture in PD (measured on Complex).
                (Vec3::new(pos[3 * i], pos[3 * i + 1], pos[3 * i + 2]), [uv[2 * i] * w, (1.0 - uv[2 * i + 1]) * h], [srgb8(c(0)), srgb8(c(1)), srgb8(c(2)), (c(3) * 255.0).round() as u8])
            };
            for t in glb.triangle_indices(prim, n)?.chunks_exact(3) {
                let v = [vert(t[0]), vert(t[1]), vert(t[2])];
                rooms.entry(room).or_default().push(Tri { pos: v.map(|v| v.0), uv: v.map(|v| v.1), col: v.map(|v| v.2), material, xlu });
            }
        }
    }
    Ok(rooms)
}

/// How far the glTF's unchanged rooms are from the arena's own vertices. A
/// check that the conversion's conventions (texels, V, sRGB) are the arena's.
/// A decal drawn over a surface has the same corners as it (CI's), so a glTF
/// triangle is compared with the closest of the arena's at its corners.
struct ConversionError {
    /// The worst texel and colour step over the triangles within tolerance.
    duv: f32,
    dc: i32,
    /// The triangles compared, and those past 2 texels or 3 colour steps, by
    /// room (an export's own slips: Felicity's room 0x0c has 4 triangles'
    /// alpha 0 in the Blender repo's glTF, 255 in PD).
    tris: usize,
    off: Vec<(u16, usize)>,
}

fn conversion_error(base: &Base, glb: &HashMap<u16, Vec<Tri>>, same: &[u16]) -> ConversionError {
    let mut e = ConversionError { duv: 0.0, dc: 0, tris: 0, off: Vec::new() };
    for &room in same {
        let mut off_n = 0;
        let mut by_key: HashMap<Key, Vec<(usize, [u16; 3])>> = HashMap::new();
        for bi in base.batches.get(&room).into_iter().flatten() {
            let b = &base.def.batches[*bi];
            for t in b.idx.chunks_exact(3) {
                by_key.entry(key(&[0, 1, 2].map(|k| b.verts[t[k] as usize].pos))).or_default().push((*bi, [t[0], t[1], t[2]]));
            }
        }
        for t in &glb[&room] {
            let off = |&(bi, idx): &(usize, [u16; 3])| {
                let b = &base.def.batches[bi];
                let (mut du, mut dcol) = (0.0f32, 0);
                for k in 0..3 {
                    // A lit vertex's colour is its normal (CI's texgen surfaces).
                    let Some(v) = idx.iter().map(|&i| &b.verts[i as usize]).find(|v| round(v.pos) == round(t.pos[k]) && v.flags & 1 == 0) else { continue };
                    du = du.max((v.uv[0] - t.uv[k][0]).abs()).max((v.uv[1] - t.uv[k][1]).abs());
                    dcol = dcol.max((0..4).map(|c| (v.c[c] as i32 - t.col[k][c] as i32).abs()).max().unwrap_or(0));
                }
                (du, dcol)
            };
            if let Some((du, dcol)) = by_key.get(&key(&t.pos)).into_iter().flatten().map(off).min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1))) {
                e.tris += 1;
                if du > 2.0 || dcol > 3 {
                    off_n += 1;
                } else {
                    e.duv = e.duv.max(du);
                    e.dc = e.dc.max(dcol);
                }
            }
        }
        if off_n > 0 {
            e.off.push((room, off_n));
        }
    }
    e
}

/// Rebuild `r`'s arena from its glTF into `dir`. Returns the gameplay data
/// placement starts from and what it makes of the rest: one arena's setup (on
/// its pads, for [`crate::place::route`]) and the rooms the glTF changed or
/// added; for fused stages, nothing, and the spots their MP setups mark. And
/// the report.
pub fn rebuild(r: &Recipe, s: &PdSource, paths: &Paths, dir: &Path) -> Result<(Gameplay, SourceHow, Vec<String>), String> {
    let mut report = Vec::new();
    let parts = s.parts()?;
    let base = load_bases(paths, &parts)?;
    let glb_path = paths.src.clone().unwrap_or_else(|| PathBuf::from(&s.glb));
    let glb = read_glb(&glb_path, &base, &parts)?;
    let nbase = base.bg["rooms"].as_array().map_or(0, |a| a.len()) as u16;
    let nrooms = glb.keys().copied().max().unwrap_or(0);
    if let Some(missing) = (1..=nrooms.max(nbase)).find(|rm| !glb.contains_key(rm)) {
        return Err(format!("the glTF has no room {missing:#x} (rooms 1..={nrooms:#x}, the arena's 1..={nbase:#x})"));
    }
    let kinds: Vec<Kind> = (0..=nrooms)
        .map(|room| {
            if room == 0 || room > nbase {
                return Kind::Added;
            }
            let mut a: Vec<Key> = base.tris(room).map(|(p, _)| key(&p)).collect();
            let mut b: Vec<Key> = glb[&room].iter().map(|t| key(&t.pos)).collect();
            a.sort();
            b.sort();
            if a == b {
                Kind::Same
            } else {
                Kind::Changed
            }
        })
        .collect();
    let of = |k: Kind| (1..=nrooms).filter(|&rm| kinds[rm as usize] == k).collect::<Vec<u16>>();
    let (same, changed, added) = (of(Kind::Same), of(Kind::Changed), of(Kind::Added));
    let names = parts.iter().map(|b| b.code.as_str()).collect::<Vec<_>>().join(" + ");
    if parts.len() > 1 {
        report.push(format!(
            "stages: {}",
            parts.iter().zip(&base.part_rooms).map(|(b, (lo, hi))| format!("{} as rooms {lo:#x}..={hi:#x}, turned {}°, moved {:?}", b.code, b.turn, b.offset)).collect::<Vec<_>>().join("; ")
        ));
    }
    report.push(format!(
        "base: {} ({}), {} rooms; the glTF: {} rooms, {} triangles: {} the arena's, changed {:x?}, added {:x?}",
        base.bg["stage"].as_str().unwrap_or("?"),
        names,
        nbase,
        nrooms,
        glb.values().map(|t| t.len()).sum::<usize>(),
        same.len(),
        changed,
        added
    ));
    let e = conversion_error(&base, &glb, &same);
    let noff: usize = e.off.iter().map(|o| o.1).sum();
    report.push(format!(
        "conversion: the arena's rooms from the glTF are within {:.2} texels and {} colour steps of its own on {} of {} triangles{}",
        e.duv,
        e.dc,
        e.tris - noff,
        e.tris,
        if noff > 0 { format!(" (off: {})", e.off.iter().map(|(r, n)| format!("room {r:#x} {n}")).collect::<Vec<_>>().join(", ")) } else { String::new() }
    ));
    if noff * 100 > e.tris {
        return Err(format!("the glTF's unchanged rooms don't convert to the arena's vertices ({noff} of {} triangles past 2 texels or 3 colour steps): the texel, V or colour convention is off", e.tris));
    }

    // ── The BG: the arena's batches for its rooms, the glTF's for the rest ─
    let rebuilt = |room: u16| kinds[room as usize] != Kind::Same;
    // (room, xlu) -> (material -> triangles), for the rebuilt rooms.
    let mut groups: HashMap<(u16, bool), Layer> = HashMap::new();
    for room in (1..=nrooms).filter(|&rm| rebuilt(rm)) {
        for t in &glb[&room] {
            let g = groups.entry((room, t.xlu)).or_default();
            match g.iter_mut().find(|(m, _)| *m == t.material) {
                Some((_, v)) => v.push(*t),
                None => g.push((t.material, vec![*t])),
            }
        }
    }
    let has = |room: u16, xlu: bool| if rebuilt(room) { groups.contains_key(&(room, xlu)) } else { base.batches.get(&room).is_some_and(|bs| bs.iter().any(|&b| base.batch_room[b].1 == xlu)) };
    let mut nodes = vec![base.bg["nodes"][0].clone()];
    let mut node_of: HashMap<(u16, bool), usize> = HashMap::new();
    for xlu in [false, true] {
        for room in (1..=nrooms).filter(|&rm| has(rm, xlu)) {
            node_of.insert((room, xlu), nodes.len());
            nodes.push(json!({"type": "dl", "parent": 0, "room": room, "layer": if xlu { "xlu" } else { "opa" }, "rendermode": 0}));
        }
    }
    let mut order: Vec<(u16, bool)> = node_of.keys().copied().collect();
    order.sort_by_key(|k| node_of[k]);
    let mut blob: Vec<u8> = Vec::new();
    let mut batches = Vec::new();
    let mut ncolours: HashMap<u16, usize> = HashMap::new();
    let mut gfx: HashMap<u16, (Vec3, Vec3)> = HashMap::new();
    let put_vert = |blob: &mut Vec<u8>, pos: Vec3, mtx: u16, uv: [f32; 2], c: [u8; 4], flags: u8| {
        for f in [pos.x, pos.y, pos.z] {
            blob.extend_from_slice(&f.to_le_bytes());
        }
        blob.extend_from_slice(&mtx.to_le_bytes());
        blob.extend_from_slice(&uv[0].to_le_bytes());
        blob.extend_from_slice(&uv[1].to_le_bytes());
        blob.extend_from_slice(&c);
        blob.extend_from_slice(&[flags, 0]);
    };
    for (room, xlu) in order {
        let node = node_of[&(room, xlu)];
        if !rebuilt(room) {
            for &bi in base.batches[&room].iter().filter(|&&b| base.batch_room[b].1 == xlu) {
                let b = &base.def.batches[bi];
                for v in &b.verts {
                    put_vert(&mut blob, v.pos, v.mtx, v.uv, v.c, v.flags);
                }
                for i in &b.idx {
                    blob.extend_from_slice(&i.to_le_bytes());
                }
                let mut row = base.bg["batches"][bi].clone();
                row["node"] = json!(node);
                batches.push(row);
            }
            continue;
        }
        for (m, tris) in &groups[&(room, xlu)] {
            // Vertices shared within a batch, at most u16's worth each.
            for chunk in tris.chunks(0x5000) {
                let mut verts: Vec<(Vec3, [f32; 2], [u8; 4])> = Vec::new();
                let mut index: HashMap<[u32; 6], u16> = HashMap::new();
                let mut idx: Vec<u16> = Vec::new();
                for t in chunk {
                    for k in 0..3 {
                        let (p, uv, c) = (t.pos[k], t.uv[k], t.col[k]);
                        let kk = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), uv[0].to_bits(), uv[1].to_bits(), u32::from_le_bytes(c)];
                        idx.push(*index.entry(kk).or_insert_with(|| {
                            verts.push((p, uv, c));
                            (verts.len() - 1) as u16
                        }));
                    }
                }
                let first = *ncolours.get(&room).unwrap_or(&0);
                ncolours.insert(room, first + verts.len());
                for &(p, uv, c) in &verts {
                    put_vert(&mut blob, p, 0, uv, c, 0);
                    let e = gfx.entry(room).or_insert((p, p));
                    *e = (e.0.min(p), e.1.max(p));
                }
                for i in &idx {
                    blob.extend_from_slice(&i.to_le_bytes());
                }
                batches.push(json!({"node": node, "material": m, "nverts": verts.len(), "nidx": idx.len(), "leaf": 0, "cidx": (first..first + verts.len()).collect::<Vec<_>>()}));
            }
        }
    }
    let rooms: Vec<Value> = (1..=nrooms)
        .map(|room| {
            let mut row = if room <= nbase { base.bg["rooms"][room as usize - 1].clone() } else { Value::Null };
            if rebuilt(room) {
                let (lo, hi) = gfx.get(&room).copied().unwrap_or((Vec3::ZERO, Vec3::ZERO));
                let n = *ncolours.get(&room).unwrap_or(&0);
                if row.is_null() {
                    row = json!({"room": room, "pos": v3(((lo + hi) * 0.5).round()), "br_light_min": 128, "br_light_max": 255, "numlights": 0, "lightindex": -1, "colour_alpha_only": [], "bsp_parents": 0});
                }
                row["bbmin"] = v3(lo.floor());
                row["bbmax"] = v3(hi.ceil());
                row["gfx_bbmin"] = v3(lo);
                row["gfx_bbmax"] = v3(hi);
                row["numvertices"] = json!(n);
                row["numcolours"] = json!(n);
            }
            row["opa_node"] = json!(node_of.get(&(room, false)));
            row["xlu_node"] = json!(node_of.get(&(room, true)));
            row
        })
        .collect();

    // ── Portals: the arena's, and the openings to the rebuilt rooms ────────
    let mut portals: Vec<Value> = base.bg["portals"].as_array().cloned().unwrap_or_default();
    let joined: HashSet<[u16; 2]> = portals
        .iter()
        .map(|p| {
            let mut rs = [p["rooms"][0].as_u64().unwrap_or(0) as u16, p["rooms"][1].as_u64().unwrap_or(0) as u16];
            rs.sort();
            rs
        })
        .collect();
    // A translucent triangle (a grille, a railing) closes no opening.
    let room_tris: HashMap<u16, Vec<[Vec3; 3]>> = glb.iter().map(|(&rm, ts)| (rm, ts.iter().filter(|t| !t.xlu).map(|t| t.pos).collect())).collect();
    let centre = |room: u16| {
        let row = &rooms[room as usize - 1];
        (vec3(&row["bbmin"]) + vec3(&row["bbmax"])) * 0.5
    };
    let found = find_portals(&room_tris, |a, b| (rebuilt(a) || rebuilt(b)) && !joined.contains(&[a, b]))
        .into_iter()
        .map(|(rs, hull)| orient(rs, hull, &room_tris, centre))
        .collect::<Result<Vec<_>, _>>()?;
    report.push(format!(
        "portals: {} the arena's, {} found to the rebuilt rooms: {}",
        portals.len(),
        found.len(),
        found.iter().map(|(rs, v)| format!("{:x}->{:x} ({} verts)", rs[0], rs[1], v.len())).collect::<Vec<_>>().join(", ")
    ));
    for (rs, verts) in &found {
        portals.push(json!({"rooms": rs, "flags": 0, "verts": verts.iter().map(|&v| v3(v)).collect::<Vec<_>>()}));
    }

    let code = format!("STAGE_CUSTOM_{}", r.code.to_uppercase());
    let mut head = base.bg.clone();
    for (k, v) in [
        ("name", json!(format!("bg_{}", r.code))),
        ("file", json!("custom")),
        ("source", json!(format!("{}: pd_import, {} rebuilt from {}", r.code, names, glb_path.file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned())))),
        ("exporter", json!(EXPORTER)),
        ("stage", json!(code)),
        ("nodes", json!(nodes)),
        ("batches", json!(batches)),
        ("rooms", json!(rooms)),
        ("portals", json!(portals)),
    ] {
        head[k] = v;
    }
    for row in ["fogenvironment", "nofogenvironment"] {
        if head["env"][row].is_object() {
            head["env"][row]["stage"] = json!(code);
        }
    }
    report.push(copy_textures(&base, &mut head, paths)?);
    write_json(&dir.join("bg.json"), &head)?;
    std::fs::File::create(dir.join("bg.bin")).and_then(|mut f| f.write_all(&blob)).map_err(|e| format!("bg.bin: {e}"))?;

    // ── Tiles ──────────────────────────────────────────────────────────────
    let floortype = {
        let mut n: HashMap<u64, usize> = HashMap::new();
        for t in base.tiles.iter().filter(|t| t["flags"].as_u64().unwrap_or(0) as u32 & GEOFLAG_FLOOR1 != 0) {
            *n.entry(t["floortype"].as_u64().unwrap_or(0)).or_default() += 1;
        }
        n.into_iter().max_by_key(|e| e.1).map_or(FLOORTYPE_DEFAULT, |e| e.0 as u8)
    };
    let mut tiles: Vec<Value> = Vec::new();
    let (mut kept, mut dropped, mut made) = (0, 0, 0);
    for room in 1..=nrooms {
        let own: Vec<&Value> = base.tiles.iter().filter(|t| t["room"].as_u64() == Some(room as u64)).collect();
        match kinds[room as usize] {
            Kind::Same => {
                kept += own.len();
                tiles.extend(own.into_iter().cloned());
            }
            kind => {
                let now = &glb[&room];
                let old: HashSet<Key> = base.tris(room).map(|(p, _)| key(&p)).collect();
                let new: HashSet<Key> = now.iter().map(|t| key(&t.pos)).collect();
                let removed: Vec<[Vec3; 3]> = base.tris(room).map(|(p, _)| p).filter(|p| !new.contains(&key(p))).collect();
                let mut gone: Vec<Vec<Vec3>> = Vec::new();
                for t in own {
                    let verts: Vec<Vec3> = t["verts"].as_array().into_iter().flatten().map(vec3).collect();
                    // Resting on geometry the tool took away, where nothing is now.
                    let lost = samples(&verts).into_iter().any(|p| removed.iter().any(|r| on_poly(p, r, 1.0)) && !now.iter().any(|n| on_poly(p, &n.pos, 1.0)));
                    if lost {
                        dropped += 1;
                        gone.push(verts);
                    } else {
                        kept += 1;
                        tiles.push(t.clone());
                    }
                }
                for t in now {
                    // A changed room: its new triangles, and those the dropped tiles covered.
                    let fresh = kind == Kind::Added
                        || !old.contains(&key(&t.pos))
                        || gone.iter().any(|g| on_poly((t.pos[0] + t.pos[1] + t.pos[2]) / 3.0, g, 1.0) || samples(g).into_iter().any(|p| on_poly(p, &t.pos, 1.0)));
                    if let Some(tile) = fresh.then(|| tile_for(t, room, floortype)).flatten() {
                        made += 1;
                        tiles.push(tile);
                    }
                }
            }
        }
    }
    report.push(format!("tiles: {kept} the arena's, {dropped} dropped (on geometry taken away), {made} made from the rebuilt rooms' triangles"));
    write_json(
        &dir.join("tiles.json"),
        &json!({"format": "pd-tiles/1", "source": format!("{}: pd_import", r.code), "exporter": EXPORTER, "rooms": (1..=nrooms).collect::<Vec<_>>(), "tiles": tiles}),
    )?;

    let mut touched = changed;
    touched.extend(added);
    if parts.len() > 1 {
        let n = |k: MarkerKind| base.markers.iter().filter(|m| m.kind == k).count();
        report.push(format!("setup: generated; the stages' MP setups mark {} spawns and {} weapons", n(MarkerKind::Spawn), n(MarkerKind::Item)));
        return Ok((Gameplay::default(), SourceHow::Generate(base.markers), report));
    }

    // ── The arena's setup, on its pads ─────────────────────────────────────
    let mut fixed = Gameplay::default();
    for p in base.pads["pads"].as_array().into_iter().flatten() {
        if p["liftnum"].as_u64().unwrap_or(0) != 0 {
            return Err("the arena has a lift pad (pads.json liftnum), which the writer doesn't carry".into());
        }
        let b: Vec<f32> = p["bbox"].as_array().into_iter().flatten().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
        let bbox: [f32; 6] = b.try_into().map_err(|_| "a pad's bbox is not six numbers")?;
        fixed.pads.push(PadRow { pos: vec3(&p["pos"]), look: vec3(&p["look"]), up: vec3(&p["up"]), flags: p["flags"].as_u64().unwrap_or(0) as u32, bbox });
    }
    for c in base.pads["cover"].as_array().into_iter().flatten() {
        if c["special"].as_u64().unwrap_or(0) != 0 {
            return Err("the arena has special cover, which the writer doesn't carry".into());
        }
        fixed.cover.push((vec3(&c["pos"]), vec3(&c["look"])));
    }
    fixed.intro = base.setup["intro"].as_array().cloned().unwrap_or_default();
    fixed.props = base.setup["props"].as_array().cloned().unwrap_or_default();
    report.push(format!("setup: the arena's, {} intro rows and {} props on its {} pads, {} cover spots", fixed.intro.len(), fixed.props.len(), fixed.pads.len(), fixed.cover.len()));
    Ok((fixed, SourceHow::Keep(touched), report))
}

/// The textures the level draws that `assets/` lacks (a stage exported only
/// to build on: CI's), copied into the custom tree at the same path, and
/// their surface types (footsteps, shot effects: the pool index's, which the
/// game reads only from `assets/`) written into their `textures` entries.
fn copy_textures(base: &Base, head: &mut Value, paths: &Paths) -> Result<String, String> {
    let mut indexes: HashMap<PathBuf, Value> = HashMap::new();
    let mut copied = 0;
    for (id, t) in head["textures"].as_object_mut().into_iter().flatten() {
        let Some(file) = t["file"].as_str().map(str::to_owned) else { continue };
        let Some(root) = id.parse::<u32>().ok().and_then(|n| base.tex_root.get(&n)) else { continue };
        if paths.assets.join(&file).exists() {
            continue;
        }
        let to = paths.custom.join(&file);
        std::fs::create_dir_all(to.parent().unwrap_or(&paths.custom)).map_err(|e| format!("{}: {e}", to.display()))?;
        std::fs::copy(root.join(&file), &to).map_err(|e| format!("{}: {e}", root.join(&file).display()))?;
        copied += 1;
        if !indexes.contains_key(root) {
            indexes.insert(root.clone(), read_json(&root.join("textures").join("index.json"))?);
        }
        let e = &indexes[root][format!("{:04x}", id.parse::<u32>().unwrap_or(0))];
        for k in ["soundsurfacetype", "surfacetype"] {
            t[k] = e[k].clone();
        }
    }
    Ok(format!("textures: {} drawn, {copied} of them not in assets/ (copied into the custom tree)", head["textures"].as_object().map_or(0, |m| m.len())))
}

/// The tile a rebuilt room's triangle is, by the arena's own rules
/// ([`crate::source::tile_of_triangle`]), a floor of the arena's usual floor
/// type.
fn tile_for(t: &Tri, room: u16, floortype: u8) -> Option<Value> {
    let (flags, floorcol) = crate::source::tile_of_triangle(t.pos, t.col, t.xlu)?;
    let ft = if flags & GEOFLAG_FLOOR1 != 0 { floortype } else { 0 };
    Some(json!({"room": room, "flags": flags, "floortype": ft, "floorcol": floorcol, "verts": t.pos.iter().map(|&v| v3(v)).collect::<Vec<_>>()}))
}

/// A convex polygon's normal (Newell's), unnormalised.
fn newell(poly: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    n
}

/// Is `p` on the convex polygon `poly`: within `tol` of its plane, inside its
/// edges give or take `tol`?
fn on_poly(p: Vec3, poly: &[Vec3], tol: f32) -> bool {
    let n = newell(poly);
    if n.length_squared() < 1e-6 {
        return false;
    }
    let n = n.normalize();
    if (p - poly[0]).dot(n).abs() > tol {
        return false;
    }
    (0..poly.len()).all(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        (b - a).cross(p - a).dot(n) >= -tol * (b - a).length()
    })
}

/// Points over a convex polygon's inside (a barycentric grid on its fan, in
/// eighths, off the edges).
fn samples(poly: &[Vec3]) -> Vec<Vec3> {
    let mut out = Vec::new();
    for k in 1..poly.len().saturating_sub(1) {
        let (a, b, c) = (poly[0], poly[k], poly[k + 1]);
        for i in 1..8 {
            for j in 1..8 - i {
                let (u, v) = (i as f32 / 8.0, j as f32 / 8.0);
                out.push(a + (b - a) * u + (c - a) * v);
            }
        }
    }
    out
}

/// A room's open edges: those only one of its triangles has (by position,
/// rounded to the centimetre).
fn open_edges(tris: &[[Vec3; 3]]) -> Vec<(Vec3, Vec3)> {
    let mut count: HashMap<([i32; 3], [i32; 3]), usize> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            let (a, b) = (round(t[k]), round(t[(k + 1) % 3]));
            if a != b {
                *count.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1;
            }
        }
    }
    let v = |p: [i32; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    // In order, so the openings made from them come out the same every run.
    let mut open: Vec<([i32; 3], [i32; 3])> = count.into_iter().filter(|e| e.1 == 1).map(|e| e.0).collect();
    open.sort();
    open.into_iter().map(|(a, b)| (v(a), v(b))).collect()
}

/// Where two rooms' open edges run along each other (within 1.5 cm, for over
/// a centimetre): each stretch's two ends.
fn shared_stretches(a: &[(Vec3, Vec3)], b: &[(Vec3, Vec3)]) -> Vec<Vec3> {
    const TOL: f32 = 1.5;
    let mut out = Vec::new();
    for &(p, q) in a {
        let len = p.distance(q);
        if len < 1.0 {
            continue;
        }
        let u = (q - p) / len;
        let off = |x: Vec3| (x - p) - u * (x - p).dot(u);
        for &(r, s) in b {
            if off(r).length() > TOL || off(s).length() > TOL {
                continue;
            }
            let (t0, t1) = {
                let (x, y) = ((r - p).dot(u), (s - p).dot(u));
                (x.min(y).max(0.0), x.max(y).min(len))
            };
            if t1 - t0 > 1.0 {
                out.push(p + u * t0);
                out.push(p + u * t1);
            }
        }
    }
    out
}

/// The openings `points` outline: for each plane they span, their convex hull
/// in it (two windows in one wall make one portal over both, which only draws
/// more). Points on a line open nothing.
fn openings(points: &[Vec3]) -> Vec<Vec<Vec3>> {
    let mut pts: Vec<Vec3> = Vec::new();
    for &p in points {
        if pts.iter().all(|q| q.distance(p) > 0.5) {
            pts.push(p);
        }
    }
    let mut out = Vec::new();
    while pts.len() >= 3 {
        // The plane of the widest triangle the points make.
        let mut best = (0.0f32, Vec3::ZERO, Vec3::ZERO);
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                for k in j + 1..pts.len() {
                    let n = (pts[j] - pts[i]).cross(pts[k] - pts[i]);
                    if n.length() > best.0 {
                        best = (n.length(), n / n.length(), pts[i]);
                    }
                }
            }
        }
        if best.0 < 100.0 {
            break;
        }
        let (n, o) = (best.1, best.2);
        let (on, off): (Vec<Vec3>, Vec<Vec3>) = pts.iter().partition(|p| (**p - o).dot(n).abs() <= 2.0);
        let hull = hull_in_plane(&on, n);
        if hull.len() >= 3 && newell(&hull).length() * 0.5 >= 100.0 {
            out.push(hull);
        }
        if off.len() == pts.len() {
            break;
        }
        pts = off;
    }
    out
}

/// The convex hull of `pts` (on a plane of normal `n`), by the monotone chain
/// in the plane's own axes.
fn hull_in_plane(pts: &[Vec3], n: Vec3) -> Vec<Vec3> {
    let u = n.any_orthonormal_vector();
    let w = n.cross(u);
    let mut p: Vec<(Vec2, Vec3)> = pts.iter().map(|&x| (Vec2::new(x.dot(u), x.dot(w)), x)).collect();
    p.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y)));
    if p.len() < 3 {
        return p.into_iter().map(|x| x.1).collect();
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<(Vec2, Vec3)> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &(Vec2, Vec3)>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2].0, hull[hull.len() - 1].0, q.0) <= 0.01 {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull.into_iter().map(|x| x.1).collect()
}

/// Which way along `n` room `tris` leaves the opening `hull`: its triangles
/// with an edge on the opening (in its plane, over its outline: the floor,
/// the jambs and the lintel running through it), each by how far it reaches
/// out of the plane (up to [`NEAR`]). Only the room's geometry at the opening
/// says which side it opens from: a ramp room can wrap round behind its own
/// doorway.
fn side(tris: &[[Vec3; 3]], hull: &[Vec3], n: Vec3) -> f32 {
    let (lo, hi) = hull.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let off = |v: Vec3| (v - hull[0]).dot(n);
    let over = |p: Vec3| p.clamp(lo, hi).distance(p) <= 2.0;
    let mut s = 0.0;
    for t in tris {
        for k in 0..3 {
            let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            if off(a).abs() <= 2.0 && off(b).abs() <= 2.0 && (0..=8).any(|i| over(a.lerp(b, i as f32 / 8.0))) {
                s += off(c).clamp(-NEAR, NEAR);
            }
        }
    }
    s
}

/// How far a room's triangle at an opening counts towards its side (cm).
const NEAR: f32 = 100.0;

/// The opening `hull` between rooms `rs`, as a portal PD's `bg_init_portal`
/// (`bg.c:6020`) takes the right way round. PD finds a portal's front from its
/// winding and its rooms' centres (their boxes' middles): it faces `room2`,
/// unless `room1`'s centre is in front, when it swaps, and then swaps the
/// rooms back (not the normal) if the other centre was behind. PD's own rooms
/// keep their centres on their sides; a rebuilt one needn't (an L-shaped
/// corridor's centre can be past its own doorway), and PD would then look
/// through the portal only from the wrong side. So each room's side is taken
/// from its geometry near the opening ([`side`]), `room1` is a room whose
/// centre is on its own side (when neither is, PD's single swap comes out
/// right), and the vertices wind so the normal faces `room2`.
fn orient(rs: [u16; 2], mut hull: Vec<Vec3>, tris: &HashMap<u16, Vec<[Vec3; 3]>>, centre: impl Fn(u16) -> Vec3) -> Result<Opening, String> {
    let [a, b] = rs;
    let h = newell(&hull).normalize();
    let (sa, sb) = (side(&tris[&a], &hull, h), side(&tris[&b], &hull, h));
    // One room's say is enough: a window in a flat wall has its reveal on one
    // side only.
    if sa * sb > 0.0 || sa == sb {
        return Err(format!("the opening between rooms {a:#x} and {b:#x} at {}: can't tell their sides (their geometry at it reaches {sa}, {sb} along {h})", hull[0]));
    }
    let n = if sb > sa { h } else { -h };
    // `n` faces b.
    let d = n.dot(hull[0]);
    let (pair, front) = if n.dot(centre(a)) <= d || n.dot(centre(b)) < d { ([a, b], n) } else { ([b, a], -n) };
    // `portal_metric`'s normal is the winding's (Newell's), negated.
    if newell(&hull).dot(front) > 0.0 {
        hull.reverse();
    }
    Ok((pair, hull))
}

/// Portals between the rooms of `tris` (room -> its triangles) for each pair
/// `want` takes (lower room first): an opening per plane where their open
/// edges run along each other.
fn find_portals(tris: &HashMap<u16, Vec<[Vec3; 3]>>, want: impl Fn(u16, u16) -> bool) -> Vec<Opening> {
    let mut rooms: Vec<u16> = tris.keys().copied().collect();
    rooms.sort();
    let edges: HashMap<u16, Vec<(Vec3, Vec3)>> = rooms.iter().map(|&rm| (rm, open_edges(&tris[&rm]))).collect();
    let bounds: HashMap<u16, (Vec3, Vec3)> = rooms
        .iter()
        .map(|&rm| (rm, tris[&rm].iter().flatten().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| (lo.min(*v), hi.max(*v)))))
        .collect();
    let mut out = Vec::new();
    for (i, &a) in rooms.iter().enumerate() {
        for &b in &rooms[i + 1..] {
            let ((alo, ahi), (blo, bhi)) = (bounds[&a], bounds[&b]);
            if !want(a, b) || (alo - 2.0).cmpgt(bhi).any() || (blo - 2.0).cmpgt(ahi).any() {
                continue;
            }
            for hull in openings(&shared_stretches(&edges[&a], &edges[&b])) {
                out.push(([a, b], hull));
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod test_access {
    //! What `tests.rs` checks of this module.
    use super::*;

    /// The arena `code`'s rooms (its own opaque triangles, as the importer
    /// sees a rebuilt level's) and its portals' room pairs.
    pub fn arena_rooms(code: &str) -> (HashMap<u16, Vec<[Vec3; 3]>>, Vec<Opening>) {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets");
        let base = Base::load(&assets, code).unwrap();
        let mut rooms: HashMap<u16, Vec<[Vec3; 3]>> = HashMap::new();
        for &room in base.batches.keys() {
            rooms.insert(room, base.tris(room).filter(|&(_, bi)| !base.batch_room[bi].1).map(|(p, _)| p).collect());
        }
        let portals = base.bg["portals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let mut rs = [p["rooms"][0].as_u64().unwrap() as u16, p["rooms"][1].as_u64().unwrap() as u16];
                rs.sort();
                (rs, p["verts"].as_array().unwrap().iter().map(vec3).collect())
            })
            .collect();
        (rooms, portals)
    }

    pub fn portals_of(tris: &HashMap<u16, Vec<[Vec3; 3]>>) -> Vec<Opening> {
        find_portals(tris, |_, _| true)
    }

    pub fn on(p: Vec3, poly: &[Vec3]) -> bool {
        on_poly(p, poly, 1.0)
    }
}
