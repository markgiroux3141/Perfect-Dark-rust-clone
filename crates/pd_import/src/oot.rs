//! Ocarina of Time scenes, as the OoT Clone repo's extractor writes them
//! (`extracted/scenes/<category>/<scene>/`): `<scene>.glb` (every room's
//! display lists as meshes, the N64 draw state in each material's extras, the
//! actors as nodes), `collision.json` (the scene's `CollisionHeader`) and
//! `scene.json` (the scene headers: light settings, spawns). Everything there
//! is 1 unit = 1 OoT unit, Y up, world space.
//!
//! What the conversion does, per source:
//! - **Geometry**: every room's triangles, scaled to centimetres; a triangle
//!   whose centre is in a recipe `remove` box is dropped (the crop).
//! - **Lighting**: OoT lights most room geometry on the RSP (normals, the
//!   scene's ambient and two directional lights, `G_LIGHTING`); a PD BG has
//!   its lighting baked into vertex colours. The chosen light setting is baked
//!   the way the RSP lights a vertex: ambient + Σ colour × max(0, n·l),
//!   clamped. Unlit geometry keeps its vertex colours.
//! - **Materials**: the extractor baked each combiner's prim/env colours into
//!   its texture (`n64_combiner_baked_into_texture`), so a material is its
//!   texture × shade; the blend (opaque, cut-out, translucent), cull, z write
//!   and decal come from its render mode.
//! - **Collision**: every polygon that entities collide with (camera-only ones
//!   are dropped), classed as OoT classes them (floor `ny > 0.5`, ceiling
//!   `ny < -0.8`, else wall) and flagged as PD's tiles are; ladders and vines
//!   are ladders; the floor sound picks the floor type and, voted over the
//!   triangles near each material, the shot surface. An exit (a doorway into
//!   another scene) becomes an invisible block, the recipe's `walls` close the
//!   crop.
//! - **Markers**: the player spawns, the Kokiri (people), the rupees and
//!   wonder items (pickups), the chests (prizes).
//! - **Environment**: OoT fogs to its fog colour over its z range (near 10,
//!   far `zFar`) from `fogNear`/1000 of the depth; so does a PD fog stage
//!   (`g_FogEnvironments`), with the sky colour as the fog colour. The same
//!   near/far ratio keeps the fog where OoT has it.

use std::collections::HashMap;
use std::path::Path;

use glam::{Vec2, Vec3};
use pd_core::ids::*;
use serde_json::Value;

use crate::glb::Glb;
use crate::recipe::{OotSource, Recipe};
use crate::source::*;

/// OoT's z near (`View_SetPerspective(view, fovy, 10, zFar)`, `z_play.c`).
const OOT_ZNEAR: f32 = 10.0;

/// How high the invisible block over an exit reaches (source units).
const EXIT_BLOCK_HEIGHT: f32 = 120.0;

/// How deep a chr wades in a water box (cm): PD's chrs don't swim, so the
/// floor under OoT's water is raised to this far under its surface, and the
/// banks become steps out of it.
const WADE_DEPTH: f32 = 30.0;

/// A `WaterBox` (`collision.json` `water_boxes`), source units.
struct WaterBox {
    x: [f32; 2],
    z: [f32; 2],
    surface: f32,
}

impl WaterBox {
    fn holds(&self, p: Vec3) -> bool {
        p.x >= self.x[0] && p.x <= self.x[1] && p.z >= self.z[0] && p.z <= self.z[1] && p.y < self.surface
    }
}

/// PD's `SURFACETYPE_*` (`g_SurfaceTypes`, `tex.c:160`).
const SURFACETYPE_STONE: u8 = 1;
const SURFACETYPE_WOOD: u8 = 2;
const SURFACETYPE_SNOW: u8 = 6;
const SURFACETYPE_DIRT: u8 = 7;
const SURFACETYPE_MUD: u8 = 8;

/// A collision polygon's sound (`sfx`, `NA_SE_PL_WALK_*`) as PD's floor type
/// (footsteps) and shot surface.
fn floor_sound(sfx: &str) -> (u8, u8) {
    let s = sfx.trim_start_matches("NA_SE_PL_WALK_");
    match s {
        "GROUND" | "SAND" | "DIRT" | "GRASS" => (FLOORTYPE_DIRT, SURFACETYPE_DIRT),
        "CONCRETE" | "MAGMA" => (FLOORTYPE_STONE, SURFACETYPE_STONE),
        _ if s.starts_with("WATER") => (FLOORTYPE_WATER, pd_sim::stage::bghit::SURFACETYPE_SHALLOWWATER),
        // OoT's wooden floors sound NA_SE_PL_WALK_LADDER.
        "LADDER" | "GLASS" => (FLOORTYPE_WOOD, SURFACETYPE_WOOD),
        "ICE" => (FLOORTYPE_SNOW, SURFACETYPE_SNOW),
        "CARPET" => (FLOORTYPE_CARPET, pd_sim::stage::bghit::SURFACETYPE_DEFAULT),
        "JABU" => (FLOORTYPE_MUD, SURFACETYPE_MUD),
        _ => (FLOORTYPE_DEFAULT, pd_sim::stage::bghit::SURFACETYPE_DEFAULT),
    }
}

/// One light setting (`EnvLightSettings`).
struct Lights {
    ambient: Vec3,
    dirs: [(Vec3, Vec3); 2],
    fog_colour: [u8; 3],
    fog_near: i32,
    fog_far: f32,
}

fn vec3(v: &Value) -> Vec3 {
    Vec3::new(v[0].as_f64().unwrap_or(0.0) as f32, v[1].as_f64().unwrap_or(0.0) as f32, v[2].as_f64().unwrap_or(0.0) as f32)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The scene header of `layer`'s commands.
fn header(scene: &Value, layer: usize) -> Result<&Vec<Value>, String> {
    scene["headers"]
        .as_array()
        .and_then(|h| h.iter().find(|h| h["layer"].as_u64() == Some(layer as u64)))
        .and_then(|h| h["commands"].as_array())
        .ok_or_else(|| format!("scene.json: no header for layer {layer}"))
}

fn command<'a>(cmds: &'a [Value], name: &str) -> Option<&'a Value> {
    cmds.iter().find(|c| c["command"] == name)
}

fn lights(scene: &Value, src: &OotSource) -> Result<Lights, String> {
    let cmds = header(scene, 0)?;
    let list = command(cmds, "LIGHT_SETTINGS_LIST").and_then(|c| c["light_settings"].as_array()).ok_or("scene.json: no light settings")?;
    let l = list.get(src.light_setting).ok_or_else(|| format!("scene.json: no light setting {}", src.light_setting))?;
    let col = |k: &str| vec3(&l[k]);
    let mut d1 = vec3(&l["light1_direction"]);
    let mut d2 = vec3(&l["light2_direction"]);
    if let Some(s) = src.sun_dir {
        // LIGHT_MODE_TIME: light 1 is the sun, light 2 opposite it.
        d1 = Vec3::from(s);
        d2 = -d1;
    }
    let fog = &l["fog_color"];
    Ok(Lights {
        ambient: col("ambient_color"),
        dirs: [(d1.normalize_or_zero(), col("light1_color")), (d2.normalize_or_zero(), col("light2_color"))],
        fog_colour: [fog[0].as_u64().unwrap_or(0) as u8, fog[1].as_u64().unwrap_or(0) as u8, fog[2].as_u64().unwrap_or(0) as u8],
        fog_near: l["fog_near"].as_i64().unwrap_or(996) as i32,
        fog_far: l["fog_far"].as_f64().unwrap_or(12800.0) as f32,
    })
}

/// The RSP's vertex lighting: ambient + each light by max(0, n·l), clamped
/// (`gSPLight`s, F3DEX2).
fn light_vertex(l: &Lights, n: Vec3) -> [u8; 3] {
    let mut c = l.ambient;
    for (dir, col) in &l.dirs {
        c += *col * n.dot(*dir).max(0.0);
    }
    let c = c.min(Vec3::splat(255.0));
    [c.x as u8, c.y as u8, c.z as u8]
}

fn wrap(mode: u64) -> Wrap {
    match mode {
        33071 => Wrap::Clamp,
        33648 => Wrap::Mirror,
        _ => Wrap::Repeat,
    }
}

fn hex_u32(v: &Value) -> u32 {
    v.as_str().and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0)
}

/// Read the scene and convert it.
pub fn load(r: &Recipe, src: &OotSource, dir: &Path) -> Result<LevelSource, String> {
    let s = r.scale;
    let scene = read_json(&dir.join("scene.json"))?;
    let col = read_json(&dir.join("collision.json"))?;
    let glb = Glb::load(&dir.join(format!("{}.glb", src.scene)))?;
    let lt = lights(&scene, src)?;
    let removed = |p: Vec3| src.remove.iter().any(|b| b.contains(p));

    // ── Textures and materials, as the rooms use them ──────────────────────
    let mut textures: Vec<Texture> = Vec::new();
    let mut tex_by_image: HashMap<(usize, [u32; 4]), usize> = HashMap::new();
    let mut materials: Vec<Material> = Vec::new();
    let mut mat_by_glb: HashMap<usize, usize> = HashMap::new();
    let mut material = |gi: usize| -> Result<usize, String> {
        if let Some(&m) = mat_by_glb.get(&gi) {
            return Ok(m);
        }
        let gm = &glb.json["materials"][gi];
        let ex = &gm["extras"];
        let mut texture = None;
        let mut wraps = [Wrap::Repeat; 2];
        if let Some(ti) = gm["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64() {
            let t = &glb.json["textures"][ti as usize];
            let image = t["source"].as_u64().ok_or("a texture without an image")? as usize;
            if let Some(si) = t["sampler"].as_u64() {
                let smp = &glb.json["samplers"][si as usize];
                wraps = [wrap(smp["wrapS"].as_u64().unwrap_or(10497)), wrap(smp["wrapT"].as_u64().unwrap_or(10497))];
            }
            // glTF's base colour is the texture times the factor: the
            // extractor puts the prim colour there when it couldn't bake the
            // combiner into the texture (the two-texture materials).
            let factor: [f32; 4] = std::array::from_fn(|k| gm["pbrMetallicRoughness"]["baseColorFactor"][k].as_f64().unwrap_or(1.0) as f32);
            let fkey = factor.map(f32::to_bits);
            let idx = match tex_by_image.get(&(image, fkey)) {
                Some(&i) => i,
                None => {
                    let mut png = glb.image_png(image)?.to_vec();
                    let img = image::load_from_memory(&png).map_err(|e| format!("image {image}: {e}"))?;
                    let (w, h) = (img.width(), img.height());
                    if factor != [1.0; 4] {
                        let mut px = img.to_rgba8();
                        for p in px.pixels_mut() {
                            for (c, f) in p.0.iter_mut().zip(factor) {
                                *c = (*c as f32 * f).round() as u8;
                            }
                        }
                        let mut out = std::io::Cursor::new(Vec::new());
                        px.write_to(&mut out, image::ImageFormat::Png).map_err(|e| format!("image {image}: {e}"))?;
                        png = out.into_inner();
                    }
                    textures.push(Texture { png, w, h });
                    tex_by_image.insert((image, fkey), textures.len() - 1);
                    textures.len() - 1
                }
            };
            texture = Some(idx);
        }
        let blend = match ex["n64_blend"].as_str().unwrap_or("Opaque") {
            b if b.starts_with("Cutout") => Blend::Cutout,
            "Translucent" => Blend::Translucent,
            _ => Blend::Opaque,
        };
        let oml = hex_u32(&ex["n64_othermode_l"]);
        materials.push(Material {
            texture,
            wrap: wraps,
            blend,
            cull_back: ex["n64_cull"].as_str() != Some("None") && !gm["doubleSided"].as_bool().unwrap_or(false),
            // Z_UPD (0x20) and ZMODE_DEC (bits 10-11 = 3) of the render mode.
            zwrite: oml & 0x20 != 0 || oml == 0,
            decal: (oml >> 10) & 3 == 3 || ex["n64_decal"].as_bool().unwrap_or(false),
            surface: pd_sim::stage::bghit::SURFACETYPE_DEFAULT,
        });
        mat_by_glb.insert(gi, materials.len() - 1);
        Ok(materials.len() - 1)
    };

    // ── Every room's triangles ─────────────────────────────────────────────
    let mut tris: Vec<Tri> = Vec::new();
    let root = glb.node(&src.scene).ok_or_else(|| format!("{}.glb: no node {:?}", src.scene, src.scene))?;
    let drawn = |n: &Value| match (&src.draw_rooms, n["extras"]["room"].as_u64()) {
        (Some(keep), Some(r)) => keep.contains(&(r as usize)),
        _ => true,
    };
    for room in glb.children(root).filter(|n| n["name"].as_str().is_some_and(|s| s.starts_with("room_")) && drawn(n)) {
        for buf in glb.children(room) {
            let xlu = buf["name"].as_str().is_some_and(|s| s.ends_with("_xlu"));
            let Some(mi) = buf["mesh"].as_u64() else { continue };
            for prim in glb.json["meshes"][mi as usize]["primitives"].as_array().into_iter().flatten() {
                let gi = prim["material"].as_u64().ok_or("a primitive without a material")? as usize;
                let m = material(gi)?;
                let lit = glb.json["materials"][gi]["extras"]["n64_lit"].as_bool().unwrap_or(false);
                let at = &prim["attributes"];
                let (pos, _) = glb.floats(at["POSITION"].as_u64().ok_or("no POSITION")? as usize)?;
                let n = pos.len() / 3;
                let uv = match at["TEXCOORD_0"].as_u64() {
                    Some(a) => glb.floats(a as usize)?.0,
                    None => vec![0.0; n * 2],
                };
                let normals = at["NORMAL"].as_u64().map(|a| glb.floats(a as usize)).transpose()?.map(|v| v.0);
                let colours = at["COLOR_0"].as_u64().map(|a| glb.floats(a as usize)).transpose()?;
                let vert = |i: usize| -> Vert {
                    let p = Vec3::new(pos[3 * i], pos[3 * i + 1], pos[3 * i + 2]);
                    let mut c = [255u8; 4];
                    if let Some((cv, k)) = &colours {
                        for j in 0..(*k).min(4) {
                            c[j] = (cv[k * i + j] * 255.0).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                    if let (true, Some(nv)) = (lit, &normals) {
                        let nn = Vec3::new(nv[3 * i], nv[3 * i + 1], nv[3 * i + 2]).normalize_or_zero();
                        let l = light_vertex(&lt, nn);
                        c = [l[0], l[1], l[2], c[3]];
                    }
                    Vert { pos: p, uv: Vec2::new(uv[2 * i], uv[2 * i + 1]), col: c }
                };
                let idx = glb.triangle_indices(prim, n)?;
                for t in idx.chunks_exact(3) {
                    let v = [vert(t[0]), vert(t[1]), vert(t[2])];
                    let centre = (v[0].pos + v[1].pos + v[2].pos) / 3.0;
                    if removed(centre) {
                        continue;
                    }
                    tris.push(Tri { v: v.map(|mut x| {
                        x.pos *= s;
                        x
                    }), material: m, xlu });
                }
            }
        }
    }

    // ── Collision ──────────────────────────────────────────────────────────
    let verts: Vec<Vec3> = col["vertices"].as_array().ok_or("collision.json: no vertices")?.iter().map(vec3).collect();
    let surfaces = col["surface_types"].as_array().ok_or("collision.json: no surface types")?;
    let water: Vec<WaterBox> = col["water_boxes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|w| {
            let f = |k: &str| w[k].as_f64().unwrap_or(0.0) as f32;
            WaterBox { x: [f("x_min"), f("x_min") + f("x_length")], z: [f("z_min"), f("z_min") + f("z_length")], surface: f("y_surface") }
        })
        .collect();
    let mut collision: Vec<ColPoly> = Vec::new();
    // For the materials' surfaces: every collision polygon's centre, normal and surface.
    let mut surface_pts: Vec<(Vec3, Vec3, u8)> = Vec::new();
    for p in col["polys"].as_array().ok_or("collision.json: no polys")? {
        if p["ignore_entities"].as_bool().unwrap_or(false) {
            continue;
        }
        let vi: Vec<usize> = p["vertices"].as_array().into_iter().flatten().filter_map(|v| v.as_u64().map(|v| v as usize)).collect();
        if vi.len() != 3 || vi.iter().any(|&i| i >= verts.len()) {
            return Err("collision.json: a polygon that is not a triangle".into());
        }
        let v = [verts[vi[0]], verts[vi[1]], verts[vi[2]]];
        let centre = (v[0] + v[1] + v[2]) / 3.0;
        if removed(centre) {
            continue;
        }
        let normal = vec3(&p["normal"]);
        let st = &surfaces[p["surface_type"].as_u64().unwrap_or(0) as usize];
        let (floortype, surface) = floor_sound(st["sfx"].as_str().unwrap_or(""));
        surface_pts.push((centre, normal, surface));
        let wall_type = st["wall_type"].as_u64().unwrap_or(0);
        let flags = if normal.y > 0.5 {
            GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2 | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT
        } else if (2..=4).contains(&wall_type) && normal.y > -0.8 {
            // WALL_TYPE_LADDER, LADDER_TOP, CLIMBABLE (vines): PD's ladder tiles.
            GEOFLAG_WALL | GEOFLAG_LADDER
        } else if normal.y < -0.8 {
            GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT
        } else if normal.y.abs() >= 0.1 {
            // A sloped wall: PD's ramp-wall test measures it where the chr stands.
            GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT | GEOFLAG_RAMPWALL
        } else {
            GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT
        };
        let mut verts: Vec<Vec3> = v.to_vec();
        if let Some(w) = water.iter().find(|w| w.holds(centre)) {
            // Wading, not swimming: the bed raised to WADE_DEPTH under the
            // surface, the channel's walls cut off below it, and what is left
            // of a bank no taller than a step dropped (a step out).
            let bed = w.surface - WADE_DEPTH / s;
            if flags & GEOFLAG_FLOOR1 != 0 {
                for p in verts.iter_mut() {
                    p.y = p.y.max(bed);
                }
            } else {
                verts = crate::rooms::clip_plane(&verts, -Vec3::Y, -bed);
                let top = verts.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
                if verts.len() < 3 || (top - bed) * s <= crate::source::MAX_RISER {
                    continue;
                }
            }
        }
        // WALL_TYPE_NO_LEDGE_GRAB (1): Link can't climb it.
        collision.push(ColPoly { verts: verts.iter().map(|&x| x * s).collect(), flags, floortype, grab: wall_type != 1 });
        // An exit: the doorway leads to another scene, so it is closed with an
        // invisible block over the trigger.
        if st["exit_index"].as_u64().unwrap_or(0) != 0 && normal.y > 0.5 {
            for k in 0..3 {
                let (a, b) = (v[k], v[(k + 1) % 3]);
                let up = Vec3::Y * EXIT_BLOCK_HEIGHT;
                collision.push(ColPoly { verts: [a, b, b + up, a + up].iter().map(|&x| x * s).collect(), flags: GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT, floortype: 0, grab: false });
            }
        }
    }
    for w in &src.walls {
        let (a, b) = (Vec3::new(w.from[0], 0.0, w.from[1]), Vec3::new(w.to[0], 0.0, w.to[1]));
        let (y0, y1) = (Vec3::Y * w.y[0], Vec3::Y * w.y[1]);
        collision.push(ColPoly { verts: [a + y0, b + y0, b + y1, a + y1].iter().map(|&x| x * s).collect(), flags: GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT, floortype: 0, grab: false });
    }

    // ── Each material's shot surface: the polygons at its triangles vote ───
    let mut votes: Vec<HashMap<u8, f32>> = vec![HashMap::new(); materials.len()];
    for t in &tris {
        let (a, b, c) = (t.v[0].pos / s, t.v[1].pos / s, t.v[2].pos / s);
        let n = (b - a).cross(c - a);
        let area = n.length() * 0.5;
        let n = n.normalize_or_zero();
        let centre = (a + b + c) / 3.0;
        let best = surface_pts
            .iter()
            .filter(|(p, pn, _)| pn.dot(n).abs() > 0.9 && (*p - centre).dot(n).abs() < 10.0)
            .min_by(|x, y| x.0.distance_squared(centre).total_cmp(&y.0.distance_squared(centre)));
        if let Some(&(p, _, surface)) = best {
            if p.distance(centre) < 200.0 {
                *votes[t.material].entry(surface).or_default() += area;
            }
        }
    }
    for (m, v) in materials.iter_mut().zip(&votes) {
        if let Some((&surface, _)) = v.iter().max_by(|a, b| a.1.total_cmp(b.1)) {
            m.surface = surface;
        }
    }

    // ── Markers ────────────────────────────────────────────────────────────
    let mut markers = Vec::new();
    let angle = |raw: f64| (raw as f32) * std::f32::consts::TAU / 65536.0;
    if let Some(spawns) = command(header(&scene, src.layer)?, "SPAWN_LIST").and_then(|c| c["spawns"].as_array()) {
        for sp in spawns {
            let p = vec3(&sp["pos"]);
            if !removed(p) {
                markers.push(Marker { kind: MarkerKind::Spawn, pos: p * s, facing: angle(sp["rot"][1].as_f64().unwrap_or(0.0)) });
            }
        }
    }
    let prefix = format!("layer_{}_room_", src.layer);
    for group in glb.nodes().iter().filter(|n| n["name"].as_str().is_some_and(|s| s.starts_with(&prefix) && s.ends_with("_actors"))) {
        for a in glb.children(group) {
            let kind = match a["extras"]["actor"].as_str().unwrap_or("") {
                "ACTOR_EN_KO" | "ACTOR_EN_SA" | "ACTOR_EN_MD" => MarkerKind::Person,
                "ACTOR_EN_ITEM00" | "ACTOR_EN_WONDER_ITEM" => MarkerKind::Item,
                "ACTOR_EN_BOX" => MarkerKind::Prize,
                _ => continue,
            };
            let p = vec3(&a["translation"]);
            if !removed(p) {
                let yaw = a["extras"]["rot_raw"][1].as_f64().unwrap_or(0.0);
                markers.push(Marker { kind, pos: p * s, facing: angle(yaw) });
            }
        }
    }

    Ok(LevelSource {
        textures,
        materials,
        tris,
        collision,
        markers,
        env: Env {
            sky: lt.fog_colour,
            near: OOT_ZNEAR * s,
            far: lt.fog_far * s,
            fog: Some((lt.fog_near, 1000)),
        },
    })
}
