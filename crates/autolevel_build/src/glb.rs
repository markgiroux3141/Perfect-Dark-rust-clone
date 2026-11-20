//! The GLB writer: the level as one `.glb` (binary glTF 2.0) with its textures embedded, as
//! `gltf_export.export_glb` writes it from Blender.
//!
//! What the file holds, as the Python's does:
//! - one scene, named for the level; a node and a mesh for each room, in the order of the rooms'
//!   numbers, named `Room_NN`, the node with `extras: {ge_room, ge_plan_key}`;
//! - a mesh is one primitive for each material the room uses, in the order they are first used:
//!   POSITION (GE's (x, z, -y) of the Blender-space mesh; glTF is Y-up as GE is), NORMAL (the
//!   face's own: the level is flat-shaded), TEXCOORD_0 (u, 1 - v: glTF's v runs down), COLOR_0
//!   (the baked lighting, RGBA as normalised u16) and indices. A vertex is a different (position,
//!   normal, uv, colour) corner, in the order first met;
//! - COLOR_0 is linear, as glTF has vertex colours: the stored value is `srgb_to_linear(byte /
//!   255)`, so a reader gets the raw 0-255 lighting back by converting to display space;
//! - materials are `KHR_materials_unlit` (the lighting is baked), named `cx_<role>`, with
//!   `extras: {ge_flags, ge_preset}`; CullBoth is `doubleSided`, Transparent is `alphaMode: BLEND`
//!   (and counted in `asset.extras.transparent_materials`); the sampler clamps only when both
//!   ClampS and ClampT are set, mirrors on either Mirror flag, and repeats otherwise (the
//!   Blender image node the Python went through wraps both axes one way);
//! - a PNG for each texture preset, shared by the materials that use it.
//!
//! Textures are ROM data: none is in this crate. They are read from a folder the caller names
//! (`tempImgEd<preset>.bmp`, as the Setup Editor dumps them); a missing one is a warning and its
//! materials are written with no texture. A 32-bit BMP's fourth byte is its alpha (the `image`
//! crate drops it, so those are read here, as pd-walk reads them) and makes an RGBA PNG; a 24-bit
//! one makes an RGB PNG. That is what Blender made of them.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use image::ImageEncoder as _;
use serde_json::{json, Map, Value};

use crate::kit::{Kit, Paint};
use crate::mesh::{Face, Room};
use crate::obj::texture_file;

// glTF's numbers: component types, buffer view targets, sampler wraps and filters.
const FLOAT: u32 = 5126;
const U16: u32 = 5123;
const U32: u32 = 5125;
const ARRAY: u32 = 34962;
const ELEMENTS: u32 = 34963;
const REPEAT: u32 = 10497;
const CLAMP: u32 = 33071;
const MIRROR: u32 = 33648;
const LINEAR: u32 = 9729;
const TRILINEAR: u32 = 9987;
// A GLB's: the file's magic, and its two chunk types.
const MAGIC: u32 = 0x4654_6C67;
const JSON: u32 = 0x4E4F_534A;
const BIN: u32 = 0x004E_4942;

/// What was written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub rooms: usize,
    pub materials: usize,
    pub bytes: usize,
    pub warnings: Vec<String>,
}

/// A texture's pixels, rows top to bottom: RGBA, or RGB when its file has no alpha.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
    pub data: Vec<u8>,
}

/// A material's name in the file, as the Python's kit names its Blender materials (Complex's:
/// `cx_floor`).
pub fn material_name(kit: &Kit, paint: &Paint) -> String {
    format!("{}_{}", kit.tag, paint.role)
}

/// The sampler wrap (both axes) for a material's flags.
pub fn wrap(flags: &[&str]) -> u32 {
    if flags.contains(&"ClampS") && flags.contains(&"ClampT") {
        CLAMP
    } else if flags.contains(&"MirrorS") || flags.contains(&"MirrorT") {
        MIRROR
    } else {
        REPEAT
    }
}

/// A raw 0-255 colour byte as glTF stores it: linear, as a normalised u16 (`_srgb_to_linear`).
pub fn linear16(byte: u8) -> u16 {
    let c = byte as f64 / 255.0;
    let lin = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    (lin * 65535.0).round() as u16
}

/// A texture file's pixels. An uncompressed 24- or 32-bit BMP is read here (a 32-bit one's
/// fourth byte is alpha, unless every one is 0: then it was never used, and the image is
/// opaque); anything else goes through `image`.
pub fn decode(data: &[u8]) -> Result<Pixels, String> {
    if data.len() >= 54 && &data[..2] == b"BM" {
        let u32_at = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let (off, w, h) = (u32_at(10) as usize, u32_at(18) as i32, u32_at(22) as i32);
        let (bpp, comp) = (u16::from_le_bytes([data[28], data[29]]), u32_at(30));
        if (bpp == 24 || bpp == 32) && comp == 0 && w > 0 && h != 0 {
            let (wu, hu, n) = (w as usize, h.unsigned_abs() as usize, bpp as usize / 8);
            let stride = (wu * n).div_ceil(4) * 4;
            if off + stride * (hu - 1) + wu * n > data.len() {
                return Err("a truncated BMP".into());
            }
            let mut out = Vec::with_capacity(wu * hu * n);
            for row in 0..hu {
                // bottom-up unless the height is negative
                let from = off + if h > 0 { hu - 1 - row } else { row } * stride;
                for p in data[from..from + wu * n].chunks_exact(n) {
                    out.extend([p[2], p[1], p[0]]);
                    if n == 4 {
                        out.push(p[3]);
                    }
                }
            }
            if n == 4 && out.chunks_exact(4).all(|p| p[3] == 0) {
                out.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            }
            return Ok(Pixels { width: wu as u32, height: hu as u32, alpha: n == 4, data: out });
        }
    }
    let img = image::load_from_memory(data).map_err(|e| e.to_string())?;
    let (width, height, alpha) = (img.width(), img.height(), img.color().has_alpha());
    Ok(Pixels { width, height, alpha, data: if alpha { img.into_rgba8().into_raw() } else { img.into_rgb8().into_raw() } })
}

/// The pixels as a PNG file's bytes: 8-bit RGBA or RGB.
pub fn png(p: &Pixels) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let kind = if p.alpha { image::ExtendedColorType::Rgba8 } else { image::ExtendedColorType::Rgb8 };
    image::codecs::png::PngEncoder::new(&mut out).write_image(&p.data, p.width, p.height, kind).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The binary chunk as it grows, with the buffer views and accessors that describe it.
#[derive(Default)]
struct Buffer {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Buffer {
    /// Add bytes as a buffer view, on a 4-byte boundary. Its number.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.data.resize(self.data.len().div_ceil(4) * 4, 0);
        let mut v = json!({"buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }

    /// Add an accessor over a new view of `bytes`. Its number.
    fn accessor(&mut self, bytes: &[u8], target: u32, component: u32, kind: &str, count: usize) -> usize {
        let view = self.view(bytes, Some(target));
        self.accessors.push(json!({"bufferView": view, "componentType": component, "count": count, "type": kind}));
        self.accessors.len() - 1
    }
}

/// What makes a vertex: its position (a place in the room's list), and the bits of its normal
/// and uv, and its colour.
type Corner = (u32, [u32; 3], [u32; 2], [u8; 3]);

/// One primitive as it is gathered: a room's triangles of one material.
#[derive(Default)]
struct Prim {
    pos: Vec<[i32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[u8; 3]>,
    idx: Vec<u32>,
    seen: HashMap<Corner, u32>,
}

/// Blender's (x, y, z) as GE's, and glTF's, (x, z, -y).
fn ge(p: [i32; 3]) -> [i32; 3] {
    [p[0], p[2], -p[1]]
}

/// A face's unit normal in GE's axes: from its corners (Newell), which are whole numbers, or the
/// normal it carries if rounding left it no area.
fn normal(room: &Room, f: &Face) -> [f32; 3] {
    let pts: Vec<[i64; 3]> = f.verts.iter().map(|&i| ge(room.verts[i as usize]).map(i64::from)).collect();
    let mut n = [0i64; 3];
    for (k, a) in pts.iter().enumerate() {
        let b = pts[(k + 1) % pts.len()];
        n = [n[0] + (a[1] - b[1]) * (a[2] + b[2]), n[1] + (a[2] - b[2]) * (a[0] + b[0]), n[2] + (a[0] - b[0]) * (a[1] + b[1])];
    }
    let mut n = n.map(|c| c as f64);
    if n == [0.0; 3] {
        n = [f.normal[0], f.normal[2], -f.normal[1]];
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len == 0.0 {
        return [0.0, 1.0, 0.0];
    }
    // + 0.0: no minus zeros
    n.map(|c| (c / len) as f32 + 0.0)
}

/// What a face must be for the writer: an error names the room.
fn sound(room: &Room, f: &Face, kit: &Kit) -> Result<(), String> {
    let n = f.verts.len();
    let ok = f.material < kit.palette.len()
        && f.uvs.len() == n
        && f.colors.len() == n
        && f.verts.iter().all(|&v| (v as usize) < room.verts.len())
        && f.tris.iter().flatten().all(|&c| (c as usize) < n);
    if ok {
        Ok(())
    } else {
        Err(format!("{}: a face whose corners, uvs, colours, triangles or material do not agree", room.name))
    }
}

fn floats(v: impl IntoIterator<Item = f32>) -> Vec<u8> {
    v.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// A room's triangles as primitives, one a material, in the order the materials are first used.
fn primitives(room: &Room, kit: &Kit) -> Result<Vec<(usize, Prim)>, String> {
    let mut prims: Vec<(usize, Prim)> = Vec::new();
    for f in &room.faces {
        sound(room, f, kit)?;
        if f.tris.is_empty() {
            continue;
        }
        let at = match prims.iter().position(|(m, _)| *m == f.material) {
            Some(at) => at,
            None => {
                prims.push((f.material, Prim::default()));
                prims.len() - 1
            }
        };
        let p = &mut prims[at].1;
        let nrm = normal(room, f);
        for c in f.tris.iter().flatten().map(|&c| c as usize) {
            let uv = [f.uvs[c].0 as f32 + 0.0, (1.0 - f.uvs[c].1) as f32 + 0.0];
            let key: Corner = (f.verts[c], nrm.map(f32::to_bits), uv.map(f32::to_bits), f.colors[c]);
            let next = p.pos.len() as u32;
            let i = *p.seen.entry(key).or_insert(next);
            if i == next {
                p.pos.push(ge(room.verts[f.verts[c] as usize]));
                p.nrm.push(nrm);
                p.uv.push(uv);
                p.col.push(f.colors[c]);
            }
            p.idx.push(i);
        }
    }
    Ok(prims)
}

/// A primitive's arrays into the buffer; its JSON.
fn primitive(buf: &mut Buffer, p: &Prim, material: usize, shade: &[u16; 256]) -> Value {
    let count = p.pos.len();
    let position = buf.accessor(&floats(p.pos.iter().flatten().map(|&c| c as f32)), ARRAY, FLOAT, "VEC3", count);
    let (mut lo, mut hi) = (p.pos[0], p.pos[0]);
    for v in &p.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    buf.accessors[position]["min"] = json!(lo);
    buf.accessors[position]["max"] = json!(hi);
    let normal = buf.accessor(&floats(p.nrm.iter().flatten().copied()), ARRAY, FLOAT, "VEC3", count);
    let uv = buf.accessor(&floats(p.uv.iter().flatten().copied()), ARRAY, FLOAT, "VEC2", count);
    let colors: Vec<u8> =
        p.col.iter().flat_map(|c| [shade[c[0] as usize], shade[c[1] as usize], shade[c[2] as usize], u16::MAX]).flat_map(u16::to_le_bytes).collect();
    let color = buf.accessor(&colors, ARRAY, U16, "VEC4", count);
    buf.accessors[color]["normalized"] = json!(true);
    // 65535 is not an index a u16 may hold (it restarts a strip), so that many vertices take u32
    let indices = if count < 65536 {
        buf.accessor(&p.idx.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect::<Vec<u8>>(), ELEMENTS, U16, "SCALAR", p.idx.len())
    } else {
        buf.accessor(&p.idx.iter().flat_map(|&i| i.to_le_bytes()).collect::<Vec<u8>>(), ELEMENTS, U32, "SCALAR", p.idx.len())
    };
    json!({"attributes": {"POSITION": position, "NORMAL": normal, "TEXCOORD_0": uv, "COLOR_0": color}, "indices": indices, "material": material})
}

/// The images, samplers and textures as the materials ask for them.
#[derive(Default)]
struct Textures {
    /// A preset's image, once it has been looked for (None: there is no such file).
    images: Vec<(&'static str, Option<usize>)>,
    image_json: Vec<Value>,
    /// The samplers' wraps.
    samplers: Vec<u32>,
    /// (image, sampler) for each texture.
    textures: Vec<(usize, usize)>,
}

impl Textures {
    /// The texture for a material: its preset's image under its flags' wrap. None, with a
    /// warning the first time, if the image's file is missing or cannot be read.
    fn of(&mut self, paint: &Paint, dir: &Path, buf: &mut Buffer, warnings: &mut Vec<String>) -> Option<usize> {
        let image = match self.images.iter().find(|(p, _)| *p == paint.preset) {
            Some((_, image)) => *image,
            None => {
                let path = dir.join(texture_file(paint.preset));
                let made = if path.is_file() {
                    fs::read(&path).map_err(|e| e.to_string()).and_then(|d| decode(&d)).and_then(|p| png(&p))
                } else {
                    Err("no texture".into())
                };
                let image = match made {
                    Ok(bytes) => {
                        let view = buf.view(&bytes, None);
                        self.image_json.push(json!({"bufferView": view, "mimeType": "image/png", "name": format!("tempImgEd{}", paint.preset)}));
                        Some(self.image_json.len() - 1)
                    }
                    Err(why) if why == "no texture" => {
                        warnings.push(format!("no texture {}", path.display()));
                        None
                    }
                    Err(why) => {
                        warnings.push(format!("texture {}: {why}", path.display()));
                        None
                    }
                };
                self.images.push((paint.preset, image));
                image
            }
        }?;
        let mode = wrap(paint.flags);
        let sampler = self.samplers.iter().position(|&w| w == mode).unwrap_or_else(|| {
            self.samplers.push(mode);
            self.samplers.len() - 1
        });
        Some(self.textures.iter().position(|&t| t == (image, sampler)).unwrap_or_else(|| {
            self.textures.push((image, sampler));
            self.textures.len() - 1
        }))
    }
}

/// The GLB's bytes and what it holds. `rooms` in any order; they are written by number. Their
/// materials are places in `kit`'s palette.
pub fn bytes(rooms: &[Room], kit: &Kit, name: &str, textures: Option<&Path>) -> Result<(Vec<u8>, Report), String> {
    let mut order: Vec<&Room> = rooms.iter().collect();
    order.sort_by_key(|r| r.number);
    let mut report = Report { rooms: order.len(), ..Report::default() };
    let shade: [u16; 256] = std::array::from_fn(|b| linear16(b as u8));
    let mut buf = Buffer::default();
    // the materials, in the order they are first used: a place in the palette, for each
    let mut used: Vec<usize> = Vec::new();
    let (mut nodes, mut meshes) = (Vec::new(), Vec::new());
    for room in order {
        let mut node = json!({"name": room.name, "extras": {"ge_room": room.number, "ge_plan_key": room.key}});
        let mut prims = Vec::new();
        for (m, p) in primitives(room, kit)? {
            let material = used.iter().position(|u| *u == m).unwrap_or_else(|| {
                used.push(m);
                used.len() - 1
            });
            prims.push(primitive(&mut buf, &p, material, &shade));
        }
        // a mesh must have a primitive: a room with no triangles is a node alone
        if !prims.is_empty() {
            node["mesh"] = json!(meshes.len());
            meshes.push(json!({"name": room.name, "primitives": prims}));
        }
        nodes.push(node);
    }
    let mut tex = Textures::default();
    let (mut materials, mut clear) = (Vec::new(), 0);
    for &m in &used {
        let paint = &kit.palette[m];
        let mut pbr = json!({"metallicFactor": 0.0, "roughnessFactor": 0.9});
        if let Some(t) = textures.and_then(|dir| tex.of(paint, dir, &mut buf, &mut report.warnings)) {
            pbr["baseColorTexture"] = json!({"index": t});
        }
        let mut mat = json!({
            "name": material_name(kit, paint),
            "extensions": {"KHR_materials_unlit": {}},
            "extras": {"ge_flags": paint.flags.join(" "), "ge_preset": paint.preset},
            "pbrMetallicRoughness": pbr,
        });
        if paint.flags.contains(&"CullBoth") {
            mat["doubleSided"] = json!(true);
        }
        if paint.role == "ladder" {
            mat["extras"]["pd"] = json!("ladder"); // (climbed: `pd_import::gltf`'s tags)
        }
        if paint.flags.contains(&"Transparent") {
            mat["alphaMode"] = json!("BLEND");
            clear += 1;
        }
        materials.push(mat);
    }
    report.materials = materials.len();
    buf.data.resize(buf.data.len().div_ceil(4) * 4, 0);

    let mut root = Map::new();
    let generator = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));
    root.insert("asset".into(), json!({"generator": generator, "version": "2.0", "extras": {"transparent_materials": clear}}));
    root.insert("scene".into(), json!(0));
    let mut scene = json!({"name": name});
    if !nodes.is_empty() {
        scene["nodes"] = json!((0..nodes.len()).collect::<Vec<usize>>());
    }
    root.insert("scenes".into(), json!([scene]));
    if !materials.is_empty() {
        root.insert("extensionsUsed".into(), json!(["KHR_materials_unlit"]));
    }
    let samplers: Vec<Value> = tex.samplers.iter().map(|w| json!({"magFilter": LINEAR, "minFilter": TRILINEAR, "wrapS": w, "wrapT": w})).collect();
    let texs: Vec<Value> = tex.textures.iter().map(|(image, sampler)| json!({"sampler": sampler, "source": image})).collect();
    // an empty list is not valid glTF: each is written only if it holds something
    let lists = [("nodes", nodes), ("meshes", meshes), ("materials", materials), ("textures", texs), ("images", tex.image_json), ("samplers", samplers)];
    for (key, list) in lists.into_iter().chain([("accessors", buf.accessors), ("bufferViews", buf.views)]) {
        if !list.is_empty() {
            root.insert(key.into(), Value::Array(list));
        }
    }
    if !buf.data.is_empty() {
        root.insert("buffers".into(), json!([{"byteLength": buf.data.len()}]));
    }
    let out = pack(&Value::Object(root), &buf.data)?;
    report.bytes = out.len();
    Ok((out, report))
}

/// A GLB: the 12-byte header, the JSON chunk (padded with spaces to 4 bytes) and, if there is
/// any, the binary chunk (padded with zeros).
pub fn pack(gltf: &Value, bin: &[u8]) -> Result<Vec<u8>, String> {
    let mut text = serde_json::to_vec(gltf).map_err(|e| e.to_string())?;
    text.resize(text.len().div_ceil(4) * 4, b' ');
    let padded = bin.len().div_ceil(4) * 4;
    let total = 12 + 8 + text.len() + if bin.is_empty() { 0 } else { 8 + padded };
    let small = |n: usize| u32::try_from(n).map_err(|_| "the level is too big for one GLB (4 GB)".to_string());
    let mut out = Vec::with_capacity(total);
    for word in [MAGIC, 2, small(total)?, small(text.len())?, JSON] {
        out.extend(word.to_le_bytes());
    }
    out.extend(&text);
    if !bin.is_empty() {
        out.extend(small(padded)?.to_le_bytes());
        out.extend(BIN.to_le_bytes());
        out.extend(bin);
        out.resize(total, 0);
    }
    Ok(out)
}

/// Write the rooms to `path` as a GLB named `name`, with the textures found in `textures`
/// (none at all without a folder).
pub fn write(path: &Path, rooms: &[Room], kit: &Kit, name: &str, textures: Option<&Path>) -> Result<Report, String> {
    let (out, report) = bytes(rooms, kit, name, textures)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, &out).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complex::KIT;
    use std::path::PathBuf;

    fn role(name: &str) -> usize {
        KIT.index(name).unwrap()
    }

    /// One square floor, cut on its diagonal.
    fn room(number: usize, material: usize) -> Room {
        let verts = vec![[10, 20, 30], [128, 20, 30], [128, 138, 30], [10, 138, 30]];
        let face = Face {
            verts: vec![0, 1, 2, 3],
            uvs: vec![(0.0, 0.0), (3.0, 0.0), (3.0, 0.25), (0.0, 0.25)],
            normal: [0.0, 0.0, 1.0],
            material,
            lit: None,
            colors: vec![[167, 167, 167], [255, 255, 255], [78, 0, 1], [90, 90, 90]],
            tris: vec![[0, 1, 2], [0, 2, 3]],
        };
        Room { key: format!("r{number}"), name: format!("Room_{number:02X}"), number, verts, faces: vec![face] }
    }

    /// Take a GLB apart, holding it to the container's rules on the way: (its JSON, its binary chunk).
    fn parse(glb: &[u8]) -> (Value, Vec<u8>) {
        let word = |o: usize| u32::from_le_bytes(glb[o..o + 4].try_into().unwrap()) as usize;
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(word(4), 2, "the container's version");
        assert_eq!(word(8), glb.len(), "the header's length is the file's");
        assert_eq!(glb.len() % 4, 0);
        let jlen = word(12);
        assert_eq!(word(16), 0x4E4F534A, "the first chunk is JSON");
        assert_eq!(jlen % 4, 0, "the JSON chunk ends on a 4-byte boundary");
        let gltf: Value = serde_json::from_slice(&glb[20..20 + jlen]).expect("the JSON parses");
        assert!(glb[20..20 + jlen].iter().rev().take_while(|b| !b"}".contains(b)).all(|&b| b == b' '), "JSON is padded with spaces");
        if 20 + jlen == glb.len() {
            assert!(gltf.get("buffers").is_none());
            return (gltf, Vec::new());
        }
        let blen = word(20 + jlen);
        assert_eq!(word(24 + jlen), 0x004E4942, "the second chunk is BIN");
        assert_eq!(blen % 4, 0, "the BIN chunk ends on a 4-byte boundary");
        assert_eq!(28 + jlen + blen, glb.len(), "the two chunks are the whole file");
        let bin = glb[28 + jlen..].to_vec();
        let declared = gltf["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
        assert!(declared <= blen && blen - declared < 4, "the buffer's length is the chunk's");
        assert_eq!(gltf["buffers"].as_array().unwrap().len(), 1);
        (gltf, bin)
    }

    fn list<'a>(gltf: &'a Value, key: &str) -> &'a [Value] {
        gltf[key].as_array().map_or(&[], |v| v.as_slice())
    }

    /// An accessor's numbers, read out of the buffer as its JSON says (normalised ones as stored).
    fn read(gltf: &Value, bin: &[u8], accessor: &Value) -> Vec<Vec<f64>> {
        let a = &gltf["accessors"][accessor.as_u64().unwrap() as usize];
        let v = &gltf["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let n = match a["type"].as_str().unwrap() {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            t => panic!("type {t}"),
        };
        let size = match a["componentType"].as_u64().unwrap() {
            5126 | 5125 => 4,
            5123 => 2,
            t => panic!("component {t}"),
        };
        let (off, len) = (v["byteOffset"].as_u64().unwrap() as usize, v["byteLength"].as_u64().unwrap() as usize);
        let count = a["count"].as_u64().unwrap() as usize;
        assert_eq!(off % 4, 0, "a view starts on a 4-byte boundary");
        assert_eq!(len, count * n * size, "a view is as long as its accessor");
        assert!(off + len <= bin.len(), "a view is inside the buffer");
        let one = |o: usize| match a["componentType"].as_u64().unwrap() {
            5126 => f32::from_le_bytes(bin[o..o + 4].try_into().unwrap()) as f64,
            5125 => u32::from_le_bytes(bin[o..o + 4].try_into().unwrap()) as f64,
            _ => u16::from_le_bytes(bin[o..o + 2].try_into().unwrap()) as f64,
        };
        (0..count).map(|i| (0..n).map(|k| one(off + (i * n + k) * size)).collect()).collect()
    }

    /// The structure a reader relies on, for every mesh: the attributes agree in count, the
    /// indices are in range, POSITION's min and max are the data's, normals are unit. The
    /// number of triangles.
    fn check(gltf: &Value, bin: &[u8]) -> usize {
        let mut tris = 0;
        for mesh in list(gltf, "meshes") {
            assert!(!list(mesh, "primitives").is_empty());
            for p in list(mesh, "primitives") {
                let at = &p["attributes"];
                let pos = read(gltf, bin, &at["POSITION"]);
                let a = &gltf["accessors"][at["POSITION"].as_u64().unwrap() as usize];
                for k in 0..3 {
                    let col = pos.iter().map(|v| v[k]);
                    assert_eq!(a["min"][k].as_f64().unwrap(), col.clone().fold(f64::INFINITY, f64::min));
                    assert_eq!(a["max"][k].as_f64().unwrap(), col.fold(f64::NEG_INFINITY, f64::max));
                }
                let nrm = read(gltf, bin, &at["NORMAL"]);
                assert!(nrm.iter().all(|n| ((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-5), "unit normals");
                let col = read(gltf, bin, &at["COLOR_0"]);
                assert_eq!(gltf["accessors"][at["COLOR_0"].as_u64().unwrap() as usize]["normalized"], json!(true));
                assert!(col.iter().all(|c| c[3] == 65535.0));
                assert_eq!((nrm.len(), read(gltf, bin, &at["TEXCOORD_0"]).len(), col.len()), (pos.len(), pos.len(), pos.len()));
                let idx = read(gltf, bin, &p["indices"]);
                assert_eq!(idx.len() % 3, 0);
                assert!(idx.iter().all(|i| (i[0] as usize) < pos.len()));
                assert!((p["material"].as_u64().unwrap() as usize) < list(gltf, "materials").len());
                tris += idx.len() / 3;
            }
        }
        for key in ["nodes", "meshes", "materials", "textures", "images", "samplers", "accessors", "bufferViews", "buffers"] {
            assert!(gltf.get(key).is_none_or(|v| !v.as_array().unwrap().is_empty()), "an empty {key}");
        }
        tris
    }

    #[test]
    fn a_room_comes_back_as_it_went_in() {
        let (glb, report) = bytes(&[room(2, role("floor"))], &KIT, "a level", None).unwrap();
        assert_eq!(report, Report { rooms: 1, materials: 1, bytes: glb.len(), warnings: vec![] });
        let (gltf, bin) = parse(&glb);
        assert_eq!(check(&gltf, &bin), 2);
        assert_eq!(gltf["asset"]["version"], "2.0");
        assert_eq!(gltf["asset"]["generator"], "autolevel_build 0.1.0");
        assert_eq!(gltf["asset"]["extras"]["transparent_materials"], 0);
        assert_eq!(gltf["extensionsUsed"], json!(["KHR_materials_unlit"]));
        assert_eq!(gltf["scene"], 0);
        assert_eq!(gltf["scenes"], json!([{"name": "a level", "nodes": [0]}]));
        assert_eq!(gltf["nodes"], json!([{"name": "Room_02", "mesh": 0, "extras": {"ge_room": 2, "ge_plan_key": "r2"}}]));
        assert_eq!(gltf["meshes"][0]["name"], "Room_02");
        let p = &gltf["meshes"][0]["primitives"][0];
        // four corners, none alike, and two triangles
        let a = |k: &str| &gltf["accessors"][p["attributes"][k].as_u64().unwrap() as usize];
        assert_eq!(a("POSITION"), &json!({"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [10, 30, -138], "max": [128, 30, -20]}));
        assert_eq!(a("NORMAL"), &json!({"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3"}));
        assert_eq!(a("TEXCOORD_0"), &json!({"bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2"}));
        assert_eq!(a("COLOR_0"), &json!({"bufferView": 3, "componentType": 5123, "count": 4, "type": "VEC4", "normalized": true}));
        assert_eq!(gltf["accessors"][4], json!({"bufferView": 4, "componentType": 5123, "count": 6, "type": "SCALAR"}));
        assert_eq!(gltf["bufferViews"][0], json!({"buffer": 0, "byteOffset": 0, "byteLength": 48, "target": 34962}));
        assert_eq!(gltf["bufferViews"][4], json!({"buffer": 0, "byteOffset": 160, "byteLength": 12, "target": 34963}));
        assert_eq!(bin.len(), 172);
        // Blender's (x, y, z) is (x, z, -y); v is turned over; the face looks up
        let at = &p["attributes"];
        assert_eq!(read(&gltf, &bin, &at["POSITION"]), [[10.0, 30.0, -20.0], [128.0, 30.0, -20.0], [128.0, 30.0, -138.0], [10.0, 30.0, -138.0]]);
        assert_eq!(read(&gltf, &bin, &at["NORMAL"]), vec![[0.0, 1.0, 0.0]; 4]);
        assert_eq!(read(&gltf, &bin, &at["TEXCOORD_0"]), [[0.0, 1.0], [3.0, 1.0], [3.0, 0.75], [0.0, 0.75]]);
        // 78 is 4993 in the Blender export of seed 40, too
        let lit = read(&gltf, &bin, &at["COLOR_0"]);
        assert_eq!(lit[2], [4993.0, 0.0, 20.0, 65535.0]);
        assert_eq!(lit[1], [65535.0; 4]);
        assert_eq!(read(&gltf, &bin, &p["indices"]).concat(), [0.0, 1.0, 2.0, 0.0, 2.0, 3.0]);
        // no folder of textures: no image, and a material with none
        assert_eq!(
            gltf["materials"],
            json!([{"name": "cx_floor", "extensions": {"KHR_materials_unlit": {}}, "extras": {"ge_flags": "", "ge_preset": "073E"}, "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 0.9}}])
        );
        assert!(gltf.get("images").is_none() && gltf.get("textures").is_none() && gltf.get("samplers").is_none());
    }

    #[test]
    fn the_lighting_reads_back_as_the_bytes_it_was() {
        // as the clone's importer reads COLOR_0: normalised, linear, to display space, to a byte
        for b in 0..=255u8 {
            let x = linear16(b) as f32 / 65535.0;
            let srgb = if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
            assert_eq!((srgb * 255.0).round() as u8, b);
        }
        assert_eq!((linear16(0), linear16(255)), (0, 65535));
    }

    #[test]
    fn corners_are_shared_only_when_they_are_alike() {
        // two squares side by side on one edge: alike there in everything, so 6 vertices; then one lit differently, 8
        let mut r = room(1, role("floor"));
        r.verts.extend([[246, 20, 30], [246, 138, 30]]);
        let mut next = r.faces[0].clone();
        next.verts = vec![1, 4, 5, 2];
        next.uvs = vec![(3.0, 0.0), (6.0, 0.0), (6.0, 0.25), (3.0, 0.25)];
        next.colors = vec![[255, 255, 255], [9, 9, 9], [9, 9, 9], [78, 0, 1]];
        r.faces.push(next);
        let count = |r: &Room| {
            let (gltf, bin) = parse(&bytes(std::slice::from_ref(r), &KIT, "x", None).unwrap().0);
            assert_eq!(check(&gltf, &bin), 4);
            gltf["accessors"][0]["count"].as_u64().unwrap()
        };
        assert_eq!(count(&r), 6);
        r.faces[1].colors[0] = [254, 255, 255];
        r.faces[1].colors[3] = [78, 0, 2];
        assert_eq!(count(&r), 8);
    }

    #[test]
    fn rooms_go_by_number_and_materials_as_first_used() {
        let mut both = room(1, role("window"));
        both.faces.push(room(1, role("floor")).faces.remove(0));
        both.faces.push(room(1, role("window")).faces.remove(0));
        let rooms = [room(0x1A, role("grate")), room(3, role("floor")), both, room(0x0B, role("water_surface"))];
        let (glb, report) = bytes(&rooms, &KIT, "x", None).unwrap();
        assert_eq!((report.rooms, report.materials), (4, 4));
        let (gltf, bin) = parse(&glb);
        assert_eq!(check(&gltf, &bin), 12);
        let names: Vec<&str> = list(&gltf, "nodes").iter().map(|n| n["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Room_01", "Room_03", "Room_0B", "Room_1A"]);
        assert_eq!(gltf["nodes"][3]["extras"], json!({"ge_room": 26, "ge_plan_key": "r26"}));
        // room 1: the glass is one primitive of four triangles, then the floor
        let prims = list(&gltf["meshes"][0], "primitives");
        assert_eq!(prims.len(), 2);
        assert_eq!((prims[0]["material"].as_u64(), prims[1]["material"].as_u64()), (Some(0), Some(1)));
        assert_eq!(gltf["accessors"][prims[0]["indices"].as_u64().unwrap() as usize]["count"], 12);
        let mats = list(&gltf, "materials");
        let names: Vec<&str> = mats.iter().map(|m| m["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["cx_window", "cx_floor", "cx_water_surface", "cx_grate"]);
        assert_eq!(mats[0]["extras"], json!({"ge_flags": "Transparent CullBoth ClampT", "ge_preset": "0190"}));
        let looks: Vec<(Option<bool>, Option<&str>)> = mats.iter().map(|m| (m["doubleSided"].as_bool(), m["alphaMode"].as_str())).collect();
        assert_eq!(looks, [(Some(true), Some("BLEND")), (None, None), (None, Some("BLEND")), (Some(true), None)]);
        assert_eq!(gltf["asset"]["extras"]["transparent_materials"], 2);
    }

    #[test]
    fn a_level_with_nothing_in_it_is_still_a_file() {
        let (glb, report) = bytes(&[], &KIT, "empty", None).unwrap();
        let (gltf, bin) = parse(&glb);
        assert_eq!((report.rooms, report.materials, bin.len()), (0, 0, 0));
        assert_eq!(check(&gltf, &bin), 0);
        assert_eq!(gltf["scenes"], json!([{"name": "empty"}]));
        // a room with no triangles is a node with no mesh
        let mut bare = room(5, role("floor"));
        bare.faces.clear();
        let (gltf, bin) = parse(&bytes(&[bare], &KIT, "bare", None).unwrap().0);
        assert_eq!(check(&gltf, &bin), 0);
        assert_eq!(gltf["nodes"], json!([{"name": "Room_05", "extras": {"ge_room": 5, "ge_plan_key": "r5"}}]));
        // and a face that does not agree with itself is an error, not a panic
        let mut bad = room(6, role("floor"));
        bad.faces[0].uvs.pop();
        assert!(bytes(&[bad], &KIT, "bad", None).unwrap_err().starts_with("Room_06"));
    }

    #[test]
    fn wraps_are_the_image_nodes() {
        assert_eq!(wrap(&[]), 10497);
        assert_eq!(wrap(&["ClampT"]), 10497);
        assert_eq!(wrap(&["ClampS", "ClampT"]), 33071);
        assert_eq!(wrap(&["ClampT", "MirrorS"]), 33648);
    }

    /// A made-up BMP as the Setup Editor dumps them: a 40-byte header, rows bottom-up, BGR(A).
    fn bmp(w: u32, h: u32, depth: u16, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let n = depth as usize / 8;
        let stride = (w as usize * n).div_ceil(4) * 4;
        let mut out = b"BM".to_vec();
        for word in [54 + (stride * h as usize) as u32, 0, 54, 40, w, h] {
            out.extend(word.to_le_bytes());
        }
        out.extend(1u16.to_le_bytes());
        out.extend(depth.to_le_bytes());
        out.extend([0u8; 24]);
        for y in (0..h).rev() {
            let mut row = Vec::new();
            for x in 0..w {
                let [r, g, b, a] = pixel(x, y);
                row.extend(&[b, g, r, a][..n]);
            }
            row.resize(stride, 0);
            out.extend(row);
        }
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("autolevel_glb_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An image of the file, taken out of its buffer and decoded: (name, pixels).
    fn image(gltf: &Value, bin: &[u8], index: usize) -> (String, image::DynamicImage) {
        let im = &gltf["images"][index];
        assert_eq!(im["mimeType"], "image/png");
        let v = &gltf["bufferViews"][im["bufferView"].as_u64().unwrap() as usize];
        assert!(v.get("target").is_none(), "an image's view is no vertex data");
        let (off, len) = (v["byteOffset"].as_u64().unwrap() as usize, v["byteLength"].as_u64().unwrap() as usize);
        assert_eq!(off % 4, 0);
        (im["name"].as_str().unwrap().to_string(), image::load_from_memory_with_format(&bin[off..off + len], image::ImageFormat::Png).unwrap())
    }

    #[test]
    fn textures_are_embedded_as_pngs_with_their_alpha() {
        let dir = scratch("textures");
        // the window's glass: 32-bit with alpha; the floor's (and the grate's): 24-bit, 3 wide so rows are padded
        let glass = |x: u32, y: u32| [x as u8 * 40, y as u8 * 50, 7, if (x + y).is_multiple_of(2) { 0 } else { 255 }];
        let tile = |x: u32, y: u32| [200 + x as u8, 100 + y as u8, 9, 255];
        fs::write(dir.join("tempImgEd0190.bmp"), bmp(4, 2, 32, glass)).unwrap();
        fs::write(dir.join("tempImgEd073E.bmp"), bmp(3, 2, 24, tile)).unwrap();
        // the water's: 32-bit, its alpha never used
        fs::write(dir.join("tempImgEd090F.bmp"), bmp(2, 2, 32, |x, y| [x as u8, y as u8, 3, 0])).unwrap();
        fs::write(dir.join("tempImgEd0741.bmp"), b"BM, but no bitmap").unwrap();
        let rooms =
            [room(1, role("window")), room(2, role("floor")), room(3, role("grate")), room(4, role("water")), room(5, role("plain")), room(6, role("panel"))];
        let path = dir.join("out").join("level.glb");
        let report = write(&path, &rooms, &KIT, "textured", Some(&dir)).unwrap();
        let glb = fs::read(&path).unwrap();
        assert_eq!((report.rooms, report.materials, report.bytes), (6, 6, glb.len()));
        // the plain wall's texture is not there, the panel's is no image: two warnings, and the level is written
        assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
        assert!(report.warnings[0].starts_with("no texture ") && report.warnings[0].ends_with("tempImgEd0746.bmp"), "{:?}", report.warnings);
        assert!(report.warnings[1].starts_with("texture ") && report.warnings[1].contains("tempImgEd0741.bmp"), "{:?}", report.warnings);
        let (gltf, bin) = parse(&glb);
        assert_eq!(check(&gltf, &bin), 12);
        // three images, in the order they are first asked for; the floor and the grate share one, and its texture
        assert_eq!(list(&gltf, "images").len(), 3);
        assert_eq!(gltf["textures"], json!([{"sampler": 0, "source": 0}, {"sampler": 0, "source": 1}, {"sampler": 0, "source": 2}]));
        assert_eq!(gltf["samplers"], json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]));
        let index: Vec<Option<u64>> = list(&gltf, "materials").iter().map(|m| m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64()).collect();
        assert_eq!(index, [Some(0), Some(1), Some(1), Some(2), None, None]);
        let (name, got) = image(&gltf, &bin, 0);
        assert_eq!((name.as_str(), got.width(), got.height(), got.color()), ("tempImgEd0190", 4, 2, image::ColorType::Rgba8));
        let got = got.into_rgba8();
        for (x, y, p) in got.enumerate_pixels() {
            assert_eq!(p.0, glass(x, y), "the glass at {x}, {y}");
        }
        let (name, got) = image(&gltf, &bin, 1);
        assert_eq!((name.as_str(), got.width(), got.height(), got.color()), ("tempImgEd073E", 3, 2, image::ColorType::Rgb8));
        for (x, y, p) in got.into_rgba8().enumerate_pixels() {
            assert_eq!(p.0, tile(x, y), "the tile at {x}, {y}");
        }
        let (_, got) = image(&gltf, &bin, 2);
        assert!(got.into_rgba8().enumerate_pixels().all(|(x, y, p)| p.0 == [x as u8, y as u8, 3, 255]), "alpha that was never used is opaque");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_whole_level_is_written() {
        use autolevel::level::{generate, Options};
        let lv = generate(40, &Options::python()).unwrap();
        let (polys, _) = crate::geometry::polys(&lv, &KIT).unwrap();
        let rooms = crate::mesh::emit(&lv, &polys, &KIT).unwrap();
        // the textures are the ROM's, and not this crate's: the game's pool, dumped into a folder as a
        // build dumps them (`textures::dumps`); without the pool, none
        let pool = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
        let presets = KIT.presets(rooms.iter().flat_map(|r| r.faces.iter().map(|f| f.material)));
        let dir = std::env::temp_dir().join(format!("autolevel_glb_level_{}", std::process::id()));
        let textures = crate::textures::is_pool(&pool, &presets).then(|| crate::textures::dumps(&pool, &dir, &presets).unwrap());
        let (glb, report) = bytes(&rooms, &KIT, "autolevel_40", textures.as_deref()).unwrap();
        let (gltf, bin) = parse(&glb);
        // as the Blender build of seed 40: 61 rooms, 15,303 triangles; 18 materials, three of them
        // see-through (the Blender build's 17 and two: its railings and floor grates were one, the
        // mesh, and the railings are now Complex's metal railing, the glass strips' texture)
        assert_eq!(check(&gltf, &bin), 15085); // (15,303 before the Rust's pillars stopped at the first storey; 14,995 before their ladders)
        assert_eq!((report.rooms, report.materials, list(&gltf, "nodes").len(), list(&gltf, "meshes").len()), (61, 19, 61, 61)); // (19: the pillars' ladders)
        assert_eq!(gltf["asset"]["extras"]["transparent_materials"], 3);
        assert_eq!(gltf["nodes"][0]["name"], "Room_01");
        let numbers: Vec<u64> = list(&gltf, "nodes").iter().map(|n| n["extras"]["ge_room"].as_u64().unwrap()).collect();
        assert!(numbers.windows(2).all(|w| w[0] < w[1]));
        if textures.is_some() {
            assert_eq!(report.warnings, Vec::<String>::new());
            assert_eq!((list(&gltf, "images").len(), list(&gltf, "textures").len(), list(&gltf, "samplers").len()), (16, 16, 1));
            // the see-through textures keep their cut-out alpha
            for m in list(&gltf, "materials").iter().filter(|m| m["alphaMode"] == "BLEND") {
                let t = &gltf["textures"][m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64().unwrap() as usize];
                let (name, got) = image(&gltf, &bin, t["source"].as_u64().unwrap() as usize);
                let clear = got.into_rgba8().pixels().filter(|p| p.0[3] == 0).count();
                assert!(clear > 0, "{name} has no alpha");
            }
        } else {
            assert!(gltf.get("images").is_none());
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
