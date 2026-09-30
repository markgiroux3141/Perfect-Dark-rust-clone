//! Any glTF 2.0 level (`.glb` or `.gltf`), from any tool: the drawn geometry,
//! from which everything else a PD arena carries is made, as for another
//! game's level ([`crate::source`]).
//!
//! What the conversion does:
//! - **Geometry**: every mesh primitive of the default scene that draws
//!   triangles (lists, strips, fans), through its node's world matrix (a
//!   mirrored node's winding flipped back), × the recipe's scale (glTF's
//!   units are metres: 100 makes centimetres).
//! - **Materials**: the base colour texture (PNG or JPEG) through its
//!   `texCoord` set and sampler wraps, `baseColorFactor` multiplied into it
//!   (or, untextured, into the vertex colours), `alphaMode` as the blend
//!   (`OPAQUE`, `MASK` a cut-out, `BLEND` translucent, drawn in the
//!   translucent pass), `doubleSided` as no culling. Textures over the
//!   recipe's `max_texture` are halved until they fit.
//! - **Lighting**: a PD BG's is baked into its vertex colours. `COLOR_0` is
//!   taken as that bake (linear, so converted to display space: `srgb8`);
//!   without it a sun and an ambient term are baked from the normals.
//! - **Collision**: every drawn triangle is a tile by PD's arenas' rules
//!   ([`crate::source::tile_of_triangle`]). A node (and everything under it)
//!   named with `-nocol` draws without colliding, `-colonly` collides without
//!   drawing (invisible walls, simple collision), `-ladder` makes its upright
//!   triangles a ladder (`GEOFLAG_LADDER`); `extras.pd` may say the same
//!   (`"nocol"`, `"colonly"`, `"ladder"`).
//! - **Markers**: nodes named `pd_spawn*`, `pd_weapon*` (or `pd_item*`) and
//!   `pd_prize*` mark spawns, weapon spots and the prize weapon's spot, facing
//!   along their +Z.
//! - **Environment**: no fog; the recipe's sky colour; PD's arenas' z near
//!   (15 cm) and a far plane past the level's diagonal.

use std::collections::HashMap;
use std::path::Path;

use glam::{Mat3, Vec2, Vec3};
use pd_core::ids::*;
use serde_json::Value;

use crate::glb::Glb;
use crate::recipe::{GltfSource, Recipe};
use crate::source::*;

/// The z near of PD's arenas (`g_NoFogEnvironments`).
const ZNEAR: f32 = 15.0;
/// Their usual z far; a bigger level gets its diagonal and a margin.
const ZFAR: f32 = 10000.0;

/// What a node's name or extras ask of its geometry (inherited by children).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Tags {
    nocol: bool,
    colonly: bool,
    ladder: bool,
}

impl Tags {
    fn of(node: &Value, parent: Tags) -> Tags {
        let name = node["name"].as_str().unwrap_or("").to_ascii_lowercase();
        let extra = node["extras"]["pd"].as_str().unwrap_or("").to_ascii_lowercase();
        let has = |t: &str| name.contains(&format!("-{t}")) || extra == t;
        Tags { nocol: parent.nocol || has("nocol"), colonly: parent.colonly || has("colonly"), ladder: parent.ladder || has("ladder") }
    }
}

/// A glTF material, converted.
struct Mat {
    index: usize,
    /// `baseColorFactor` in display space (applied to the vertex colours when
    /// there is no texture).
    factor: [f32; 4],
    textured: bool,
    texcoord: usize,
    xlu: bool,
}

fn wrap(mode: u64) -> Wrap {
    match mode {
        33071 => Wrap::Clamp,
        33648 => Wrap::Mirror,
        _ => Wrap::Repeat,
    }
}

/// The display-space value of a linear factor.
fn display(c: f32) -> f32 {
    srgb8(c) as f32 / 255.0
}

/// Read the glTF at `path` and convert it.
pub fn load(r: &Recipe, src: &GltfSource, path: &Path, report: &mut Vec<String>) -> Result<LevelSource, String> {
    let s = r.scale;
    let glb = Glb::load(path)?;
    let sun = Vec3::from(src.sun.unwrap_or([0.4, 1.0, 0.3])).normalize_or_zero();

    // ── Materials and textures ─────────────────────────────────────────────
    let mut textures: Vec<Texture> = Vec::new();
    let mut materials: Vec<Material> = Vec::new();
    let mut mats: Vec<Mat> = Vec::new();
    let mut tex_by_key: HashMap<(usize, [u32; 4]), usize> = HashMap::new();
    let mut resized = 0;
    let gmats = glb.json["materials"].as_array().cloned().unwrap_or_default();
    // The default material (a primitive without one): white, opaque, culled.
    let nmats = gmats.len() + 1;
    for gi in 0..nmats {
        let gm = gmats.get(gi).cloned().unwrap_or(Value::Null);
        let pbr = &gm["pbrMetallicRoughness"];
        let factor: [f32; 4] = std::array::from_fn(|k| {
            let f = pbr["baseColorFactor"][k].as_f64().unwrap_or(1.0) as f32;
            if k < 3 {
                display(f)
            } else {
                f
            }
        });
        let alpha = gm["alphaMode"].as_str().unwrap_or("OPAQUE");
        let blend = match alpha {
            "MASK" => Blend::Cutout,
            "BLEND" => Blend::Translucent,
            _ => Blend::Opaque,
        };
        let mut texture = None;
        let mut wraps = [Wrap::Repeat; 2];
        let bct = &pbr["baseColorTexture"];
        if let Some(ti) = bct["index"].as_u64() {
            let t = &glb.json["textures"][ti as usize];
            if let Some(image) = t["source"].as_u64().map(|i| i as usize) {
                if let Some(si) = t["sampler"].as_u64() {
                    let smp = &glb.json["samplers"][si as usize];
                    wraps = [wrap(smp["wrapS"].as_u64().unwrap_or(10497)), wrap(smp["wrapT"].as_u64().unwrap_or(10497))];
                }
                let key = (image, factor.map(f32::to_bits));
                let idx = match tex_by_key.get(&key) {
                    Some(&i) => i,
                    None => {
                        let bytes = glb.image_bytes(image)?;
                        let img = image::load_from_memory(&bytes).map_err(|e| format!("image {image}: {e}"))?;
                        let mut px = img.to_rgba8();
                        let (mut w, mut h) = (px.width(), px.height());
                        if w.max(h) > src.max_texture.max(8) {
                            while w.max(h) > src.max_texture.max(8) {
                                (w, h) = ((w / 2).max(1), (h / 2).max(1));
                            }
                            px = image::imageops::resize(&px, w, h, image::imageops::FilterType::Triangle);
                            resized += 1;
                        }
                        if factor != [1.0; 4] {
                            for p in px.pixels_mut() {
                                for (c, f) in p.0.iter_mut().zip(factor) {
                                    *c = (*c as f32 * f).round() as u8;
                                }
                            }
                        }
                        let mut out = std::io::Cursor::new(Vec::new());
                        px.write_to(&mut out, image::ImageFormat::Png).map_err(|e| format!("image {image}: {e}"))?;
                        textures.push(Texture { png: out.into_inner(), w, h });
                        tex_by_key.insert(key, textures.len() - 1);
                        textures.len() - 1
                    }
                };
                texture = Some(idx);
            }
        }
        materials.push(Material {
            texture,
            wrap: wraps,
            blend,
            cull_back: !gm["doubleSided"].as_bool().unwrap_or(false),
            zwrite: blend != Blend::Translucent,
            decal: false,
            surface: pd_sim::stage::bghit::SURFACETYPE_DEFAULT,
        });
        mats.push(Mat { index: materials.len() - 1, factor, textured: texture.is_some(), texcoord: bct["texCoord"].as_u64().unwrap_or(0) as usize, xlu: blend == Blend::Translucent });
    }

    // ── Every node's triangles ─────────────────────────────────────────────
    let mut tris: Vec<Tri> = Vec::new();
    let mut collision: Vec<ColPoly> = Vec::new();
    let mut markers: Vec<Marker> = Vec::new();
    let mut tags: Vec<Tags> = vec![Tags::default(); glb.nodes().len()];
    let mut parent: Vec<Option<usize>> = vec![None; glb.nodes().len()];
    for (i, n) in glb.nodes().iter().enumerate() {
        for c in n["children"].as_array().into_iter().flatten().filter_map(|c| c.as_u64()) {
            if let Some(p) = parent.get_mut(c as usize) {
                *p = Some(i);
            }
        }
    }
    let (mut unlit, mut skipped_prims) = (0usize, 0usize);
    for (ni, world) in glb.scene_nodes() {
        let node = &glb.nodes()[ni];
        tags[ni] = Tags::of(node, parent[ni].map(|p| tags[p]).unwrap_or_default());
        let t = tags[ni];
        let name = node["name"].as_str().unwrap_or("").to_ascii_lowercase();
        let marker = if name.starts_with("pd_spawn") {
            Some(MarkerKind::Spawn)
        } else if name.starts_with("pd_weapon") || name.starts_with("pd_item") {
            Some(MarkerKind::Item)
        } else if name.starts_with("pd_prize") {
            Some(MarkerKind::Prize)
        } else {
            None
        };
        if let Some(kind) = marker {
            let pos = world.transform_point3(Vec3::ZERO) * s;
            let fwd = world.transform_vector3(Vec3::Z);
            markers.push(Marker { kind, pos, facing: fwd.x.atan2(fwd.z) });
            continue;
        }
        let Some(mesh) = node["mesh"].as_u64() else { continue };
        let normal_mtx = Mat3::from_mat4(world).inverse().transpose();
        let mirrored = world.determinant() < 0.0;
        for prim in glb.json["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten() {
            let at = &prim["attributes"];
            let Some(pa) = at["POSITION"].as_u64() else {
                skipped_prims += 1;
                continue;
            };
            let (pos, _) = glb.floats(pa as usize)?;
            let n = pos.len() / 3;
            let Some(idx) = glb.triangles(prim, n)? else {
                skipped_prims += 1;
                continue;
            };
            let m = &mats[prim["material"].as_u64().map_or(nmats - 1, |m| (m as usize).min(nmats - 1))];
            let uv = match at[format!("TEXCOORD_{}", m.texcoord)].as_u64() {
                Some(a) => glb.floats(a as usize)?.0,
                None => vec![0.0; n * 2],
            };
            let normals = at["NORMAL"].as_u64().map(|a| glb.floats(a as usize)).transpose()?.map(|v| v.0);
            let colours = at["COLOR_0"].as_u64().map(|a| glb.floats(a as usize)).transpose()?;
            if colours.is_none() {
                unlit += 1;
            }
            let wpos: Vec<Vec3> = (0..n).map(|i| world.transform_point3(Vec3::new(pos[3 * i], pos[3 * i + 1], pos[3 * i + 2])) * s).collect();
            for tri in idx.chunks_exact(3) {
                let tri = if mirrored { [tri[0], tri[2], tri[1]] } else { [tri[0], tri[1], tri[2]] };
                let p = tri.map(|i| wpos[i]);
                let face = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
                let v: [Vert; 3] = tri.map(|i| {
                    let mut c = [255u8; 4];
                    match &colours {
                        Some((cv, k)) => {
                            for (j, ch) in c.iter_mut().enumerate().take((*k).min(4)) {
                                let x = cv[k * i + j];
                                *ch = if j < 3 { srgb8(x) } else { (x * 255.0).round().clamp(0.0, 255.0) as u8 };
                            }
                        }
                        None => {
                            let nrm = normals.as_ref().map_or(face, |nv| (normal_mtx * Vec3::new(nv[3 * i], nv[3 * i + 1], nv[3 * i + 2])).normalize_or_zero());
                            let l = (96.0 + 159.0 * nrm.dot(sun).max(0.0)).round() as u8;
                            c = [l, l, l, 255];
                        }
                    }
                    if !m.textured {
                        for (ch, f) in c.iter_mut().zip(m.factor) {
                            *ch = (*ch as f32 * f).round() as u8;
                        }
                    }
                    Vert { pos: wpos[i], uv: Vec2::new(uv[2 * i], uv[2 * i + 1]), col: c }
                });
                if !t.colonly {
                    tris.push(Tri { v, material: m.index, xlu: m.xlu });
                }
                if t.nocol {
                    continue;
                }
                // Invisible collision is opaque to sight and shots.
                let Some((mut flags, floorcol)) = tile_of_triangle(p, v.map(|x| x.col), m.xlu && !t.colonly) else { continue };
                if t.ladder && flags & GEOFLAG_WALL != 0 {
                    flags |= GEOFLAG_LADDER;
                }
                collision.push(ColPoly { verts: p.to_vec(), flags, floortype: FLOORTYPE_DEFAULT, grab: src.climb_ledges, floorcol });
            }
        }
    }
    if tris.is_empty() {
        return Err(format!("{}: no triangles in the default scene", path.display()));
    }
    let (lo, hi) = tris.iter().flat_map(|t| t.v.iter().map(|v| v.pos)).fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), p| (a.min(p), b.max(p)));
    report.push(format!(
        "glTF: {} nodes, {} materials, {} textures ({} resized to {} or less), {} primitives without vertex colours (lit by a sun), {} primitives skipped (points, lines, no positions), {} markers",
        glb.nodes().len(),
        gmats.len(),
        textures.len(),
        resized,
        src.max_texture,
        unlit,
        skipped_prims,
        markers.len()
    ));
    Ok(LevelSource {
        textures,
        materials,
        tris,
        collision,
        markers,
        env: Env { sky: src.sky.unwrap_or([120, 140, 170]), near: ZNEAR, far: ZFAR.max((hi - lo).length() * 1.2), fog: None },
    })
}
