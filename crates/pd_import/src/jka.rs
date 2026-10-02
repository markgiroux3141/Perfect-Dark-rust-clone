//! A Jedi Academy map: a compiled `.bsp` (Raven's `RBSP` version 1, the
//! Quake 3 engine's format with four lightmaps a vertex), read with the
//! game's own files (`GameData/base`'s packages, [`crate::pk3`]) for its
//! textures and shader scripts ([`crate::q3shader`]).
//!
//! What the conversion does:
//! - **Units and axes**: Quake's Z up becomes PD's Y up, `(x, y, z)` →
//!   `(x, z, -y)` (a rotation: windings keep their sense), × the recipe's
//!   scale (a Jedi's eye is 60 units over his feet, Joanna's 159 cm: 2.65).
//! - **Geometry**: the world's draw surfaces (and the static brush entities':
//!   `func_static`, `func_breakable`, `func_glass`, `func_wall`): planar faces
//!   and triangle soups as they are, curved patches tessellated (each 3 × 3
//!   piece into as many steps as its bend needs). Quake winds front faces
//!   clockwise; every triangle is turned to face along its vertex normals.
//! - **Lighting**: PD lights by vertex colour, Quake by lightmap. A
//!   lightmapped surface's triangles are split where the lightmap varies
//!   (an edge whose middle the lightmap lights more than a step off the
//!   average of its ends), and every vertex takes the lightmap's colour
//!   there × the recipe's `light_scale` (Jedi Academy draws lightmaps
//!   unscaled: `r_mapOverBrightBits` 0). An edge's split is decided for the
//!   whole level, so neighbouring triangles split alike (no cracks).
//!   Vertex-lit surfaces keep their vertex colours; unlit ones are white.
//! - **Materials**: a surface's shader script names its picture (the first
//!   stage that is neither the lightmap nor a reflection), else its name is
//!   the image's; an alpha test is a cut-out; a stage blending over nothing
//!   is translucent (additive ones by their brightness as alpha); `cull none`
//!   draws both sides. A missing image draws a grey checker (listed in the
//!   report). `q3map_material` gives the shot surface and the footsteps.
//! - **Collision**: what Quake collides with: the drawn faces of solid
//!   shaders (`CONTENTS_SOLID`, `CONTENTS_PLAYERCLIP`), patches included,
//!   by PD's arenas' normal rule ([`crate::source::tile_of_triangle`]), and
//!   every face of a solid brush with nothing drawn (clip brushes, invisible
//!   walls). Triangle soups (models) don't collide, as in Quake.
//! - **Markers**: the player starts (deathmatch, duel, CTF), the weapons,
//!   the NPCs; facing by their `angle`.
//! - **Environment**: no fog (Quake's fog volumes are listed, not
//!   converted); the clear colour the recipe's, else the sky box's average.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use glam::{Vec2, Vec3};
use pd_core::ids::*;
use pd_sim::stage::bghit::*;

use crate::pk3::Vfs;
use crate::q3shader::{self, Shader};
use crate::recipe::{JkaSource, Recipe};
use crate::source::*;

const SURF_SKY: u32 = 0x2000;
const SURF_NODRAW: u32 = 0x20_0000;
const CONTENTS_SOLID: u32 = 0x1;
const CONTENTS_PLAYERCLIP: u32 = 0x10;
const MST_PLANAR: i32 = 1;
const MST_PATCH: i32 = 2;
const MST_TRIANGLE_SOUP: i32 = 3;
/// `lightmapNum` of a surface lit by its vertex colours.
const LIGHTMAP_BY_VERTEX: i32 = -3;
/// A lightmap page's side (`LIGHTMAP_SIZE`).
const LM: usize = 128;

/// The splitting's passes at most (an edge's middle is tested against
/// `JkaSource::light_tolerance` down to `light_min_edge`).
const LIGHT_PASSES: usize = 8;
/// A patch piece is cut until its chords stray at most this far from the
/// curve (units).
const PATCH_TOLERANCE: f32 = 1.5;

/// The brush entities drawn and collided with as the world (static ones).
const STATIC_ENTITIES: [&str; 4] = ["func_static", "func_breakable", "func_glass", "func_wall"];

// ── The file ───────────────────────────────────────────────────────────────

struct Lumps<'a> {
    d: &'a [u8],
    dir: Vec<(usize, usize)>,
}

impl<'a> Lumps<'a> {
    fn new(d: &'a [u8]) -> Result<Lumps<'a>, String> {
        if d.len() < 8 + 18 * 8 {
            return Err("too short for a BSP".into());
        }
        match (&d[0..4], i32::from_le_bytes([d[4], d[5], d[6], d[7]])) {
            (b"RBSP", 1) => {}
            (b"IBSP", v) => return Err(format!("a Quake 3 IBSP (version {v}): only Raven's RBSP (Jedi Academy, Jedi Outcast) is read")),
            (m, v) => return Err(format!("not an RBSP: {:?} version {v}", String::from_utf8_lossy(m))),
        }
        let dir = (0..18)
            .map(|i| {
                let o = 8 + i * 8;
                (u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as usize, u32::from_le_bytes(d[o + 4..o + 8].try_into().unwrap()) as usize)
            })
            .collect::<Vec<_>>();
        if let Some(i) = dir.iter().position(|&(o, l)| o + l > d.len()) {
            return Err(format!("lump {i} runs past the end of the file"));
        }
        Ok(Lumps { d, dir })
    }

    /// Lump `i` as records of `size` bytes.
    fn records(&self, i: usize, size: usize) -> impl Iterator<Item = &'a [u8]> {
        let (o, l) = self.dir[i];
        self.d[o..o + l].chunks_exact(size)
    }

    fn bytes(&self, i: usize) -> &'a [u8] {
        let (o, l) = self.dir[i];
        &self.d[o..o + l]
    }
}

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn vec3_at(b: &[u8], o: usize) -> Vec3 {
    Vec3::new(f32_at(b, o), f32_at(b, o + 4), f32_at(b, o + 8))
}

struct BspShader {
    name: String,
    flags: u32,
    contents: u32,
}

struct Model {
    surfaces: std::ops::Range<usize>,
    brushes: std::ops::Range<usize>,
}

struct Brush {
    sides: std::ops::Range<usize>,
    shader: usize,
}

struct Side {
    plane: usize,
    shader: usize,
}

#[derive(Clone, Copy)]
struct Surface {
    shader: usize,
    kind: i32,
    verts: (usize, usize),
    indexes: (usize, usize),
    lm: i32,
    /// The surface's rectangle in its lightmap page (x, y, w, h).
    lm_rect: [i32; 4],
    normal: Vec3,
    patch: (usize, usize),
}

/// A vertex being worked on: Quake units and coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct V {
    p: Vec3,
    st: Vec2,
    lm: Vec2,
    n: Vec3,
    col: [f32; 4],
}

impl V {
    fn sum(vs: &[(&V, f32)]) -> V {
        let mut o = V { p: Vec3::ZERO, st: Vec2::ZERO, lm: Vec2::ZERO, n: Vec3::ZERO, col: [0.0; 4] };
        for (v, w) in vs {
            o.p += v.p * *w;
            o.st += v.st * *w;
            o.lm += v.lm * *w;
            o.n += v.n * *w;
            for k in 0..4 {
                o.col[k] += v.col[k] * *w;
            }
        }
        o
    }

    fn mid(a: &V, b: &V) -> V {
        V::sum(&[(a, 0.5), (b, 0.5)])
    }
}

struct Bsp {
    shaders: Vec<BspShader>,
    planes: Vec<(Vec3, f32)>,
    models: Vec<Model>,
    brushes: Vec<Brush>,
    sides: Vec<Side>,
    verts: Vec<V>,
    indexes: Vec<usize>,
    surfaces: Vec<Surface>,
    lightmaps: Vec<u8>,
    fogs: usize,
    entities: Vec<HashMap<String, String>>,
}

fn range(first: i32, n: i32) -> std::ops::Range<usize> {
    first.max(0) as usize..(first.max(0) + n.max(0)) as usize
}

impl Bsp {
    fn parse(d: &[u8]) -> Result<Bsp, String> {
        let l = Lumps::new(d)?;
        let shaders = l
            .records(1, 72)
            .map(|r| BspShader { name: String::from_utf8_lossy(r[..64].split(|&c| c == 0).next().unwrap()).replace('\\', "/").to_ascii_lowercase(), flags: i32_at(r, 64) as u32, contents: i32_at(r, 68) as u32 })
            .collect();
        let planes = l.records(2, 16).map(|r| (vec3_at(r, 0), f32_at(r, 12))).collect();
        let models = l.records(7, 40).map(|r| Model { surfaces: range(i32_at(r, 24), i32_at(r, 28)), brushes: range(i32_at(r, 32), i32_at(r, 36)) }).collect();
        let brushes = l.records(8, 12).map(|r| Brush { sides: range(i32_at(r, 0), i32_at(r, 4)), shader: i32_at(r, 8).max(0) as usize }).collect();
        let sides = l.records(9, 12).map(|r| Side { plane: i32_at(r, 0).max(0) as usize, shader: i32_at(r, 4).max(0) as usize }).collect();
        // mapVert_t: xyz, st, lightmap[4][2], normal, color[4][4].
        let verts = l
            .records(10, 80)
            .map(|r| V { p: vec3_at(r, 0), st: Vec2::new(f32_at(r, 12), f32_at(r, 16)), lm: Vec2::new(f32_at(r, 20), f32_at(r, 24)), n: vec3_at(r, 52), col: std::array::from_fn(|k| r[64 + k] as f32) })
            .collect();
        let indexes = l.records(11, 4).map(|r| i32_at(r, 0).max(0) as usize).collect();
        // dsurface_t: shader, fog, type, firstVert, numVerts, firstIndex,
        // numIndexes, styles[4] x 2, lightmapNum[4], lightmapX[4], lightmapY[4],
        // lightmapWidth, lightmapHeight, lightmapOrigin, lightmapVecs[3],
        // patchWidth, patchHeight.
        let surfaces = l
            .records(13, 148)
            .map(|r| Surface {
                shader: i32_at(r, 0).max(0) as usize,
                kind: i32_at(r, 8),
                verts: (i32_at(r, 12).max(0) as usize, i32_at(r, 16).max(0) as usize),
                indexes: (i32_at(r, 20).max(0) as usize, i32_at(r, 24).max(0) as usize),
                lm: i32_at(r, 36),
                lm_rect: [i32_at(r, 52), i32_at(r, 68), i32_at(r, 84), i32_at(r, 88)],
                normal: vec3_at(r, 128),
                patch: (i32_at(r, 140).max(0) as usize, i32_at(r, 144).max(0) as usize),
            })
            .collect();
        let entities = parse_entities(&String::from_utf8_lossy(l.bytes(0)));
        Ok(Bsp { shaders, planes, models, brushes, sides, verts, indexes, surfaces, lightmaps: l.bytes(14).to_vec(), fogs: l.bytes(12).len() / 72, entities })
    }

    /// The lightmap's colour (0..255 a channel) at `lm` in page `page`, held
    /// inside the surface's rectangle (its neighbours' texels are others');
    /// a map that leaves the rectangle empty (q3map2's often do) is held
    /// inside the page.
    fn light(&self, page: usize, rect: [i32; 4], lm: Vec2) -> Option<Vec3> {
        let base = page * LM * LM * 3;
        if base + LM * LM * 3 > self.lightmaps.len() {
            return None;
        }
        let rect = if rect[2] > 0 && rect[3] > 0 { rect } else { [0, 0, LM as i32, LM as i32] };
        let (x0, y0) = (rect[0].clamp(0, LM as i32 - 1) as f32, rect[1].clamp(0, LM as i32 - 1) as f32);
        let (x1, y1) = (((rect[0] + rect[2] - 1).max(rect[0])).clamp(0, LM as i32 - 1) as f32, ((rect[1] + rect[3] - 1).max(rect[1])).clamp(0, LM as i32 - 1) as f32);
        let x = (lm.x * LM as f32 - 0.5).clamp(x0, x1);
        let y = (lm.y * LM as f32 - 0.5).clamp(y0, y1);
        let (ix, iy) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let px = |x: usize, y: usize| {
            let o = base + (y.min(LM - 1) * LM + x.min(LM - 1)) * 3;
            Vec3::new(self.lightmaps[o] as f32, self.lightmaps[o + 1] as f32, self.lightmaps[o + 2] as f32)
        };
        let top = px(ix, iy).lerp(px(ix + 1, iy), fx);
        let bottom = px(ix, iy + 1).lerp(px(ix + 1, iy + 1), fx);
        Some(top.lerp(bottom, fy))
    }
}

/// The entity lump: `{ "key" "value" ... }` blocks.
fn parse_entities(text: &str) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    let mut cur: Option<HashMap<String, String>> = None;
    let mut key: Option<String> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => cur = Some(HashMap::new()),
            '}' => out.extend(cur.take()),
            '"' => {
                let s: String = chars.by_ref().take_while(|&c| c != '"').collect();
                match key.take() {
                    None => key = Some(s.to_ascii_lowercase()),
                    Some(k) => {
                        if let Some(e) = cur.as_mut() {
                            e.insert(k, s);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// ── Patches ────────────────────────────────────────────────────────────────

/// How many steps a quadratic piece `a b c` needs.
fn steps(a: Vec3, b: Vec3, c: Vec3) -> usize {
    // The curve's middle strays (2b - a - c) / 4 from the chord's; n steps
    // leave a quarter of that over n².
    let dev = (b * 2.0 - a - c).length() / 4.0;
    ((dev / PATCH_TOLERANCE).sqrt().ceil() as usize).clamp(1, 16)
}

/// A patch's control grid (`w` × `h`, odd) as a grid of points: each 3 × 3
/// piece in the steps its bend needs, the same along a whole row or column of
/// pieces (so they meet). Returns the grid and its width.
fn tessellate(ctrl: &[V], w: usize, h: usize) -> (Vec<V>, usize) {
    if w < 3 || h < 3 || w.is_multiple_of(2) || h.is_multiple_of(2) || ctrl.len() < w * h {
        return (Vec::new(), 0);
    }
    let at = |i: usize, j: usize| &ctrl[j * w + i];
    let (pu, pv) = ((w - 1) / 2, (h - 1) / 2);
    let su: Vec<usize> = (0..pu).map(|u| (0..h).map(|j| steps(at(2 * u, j).p, at(2 * u + 1, j).p, at(2 * u + 2, j).p)).max().unwrap()).collect();
    let sv: Vec<usize> = (0..pv).map(|v| (0..w).map(|i| steps(at(i, 2 * v).p, at(i, 2 * v + 1).p, at(i, 2 * v + 2).p)).max().unwrap()).collect();
    // The parameters along each axis: (piece, t).
    let params = |s: &[usize]| -> Vec<(usize, f32)> {
        let mut out: Vec<(usize, f32)> = s.iter().enumerate().flat_map(|(p, &n)| (0..n).map(move |k| (p, k as f32 / n as f32))).collect();
        out.push((s.len() - 1, 1.0));
        out
    };
    let quad = |a: &V, b: &V, c: &V, t: f32| V::sum(&[(a, (1.0 - t) * (1.0 - t)), (b, 2.0 * t * (1.0 - t)), (c, t * t)]);
    let (us, vs) = (params(&su), params(&sv));
    let mut grid = Vec::with_capacity(us.len() * vs.len());
    for &(v, tv) in &vs {
        for &(u, tu) in &us {
            let rows: Vec<V> = (0..3).map(|k| quad(at(2 * u, 2 * v + k), at(2 * u + 1, 2 * v + k), at(2 * u + 2, 2 * v + k), tu)).collect();
            grid.push(quad(&rows[0], &rows[1], &rows[2], tv));
        }
    }
    (grid, us.len())
}

// ── Triangles and their lighting ───────────────────────────────────────────

/// A triangle from surface `surf`.
#[derive(Clone, Copy)]
struct T {
    v: [V; 3],
    surf: usize,
}

/// `[a, b, c]` turned, if need be, to wind counter-clockwise seen from the
/// side `hint` points to; with no hint, Quake's clockwise is turned.
fn orient(mut v: [V; 3], hint: Vec3) -> [V; 3] {
    let n = (v[1].p - v[0].p).cross(v[2].p - v[0].p);
    let flip = if hint.length_squared() > 1e-6 { n.dot(hint) < 0.0 } else { true };
    if flip {
        v.swap(1, 2);
    }
    v
}

fn key(p: Vec3) -> [i32; 3] {
    (p * 16.0).round().as_ivec3().to_array()
}

fn edge_key(a: Vec3, b: Vec3) -> ([i32; 3], [i32; 3]) {
    let (ka, kb) = (key(a), key(b));
    if ka <= kb {
        (ka, kb)
    } else {
        (kb, ka)
    }
}

/// Split `t` along its marked edges (edge `k` runs from vertex `k` to `k + 1`).
fn split(t: &T, marked: [bool; 3], out: &mut Vec<T>) {
    let n = marked.iter().filter(|&&m| m).count();
    if n == 0 {
        out.push(*t);
        return;
    }
    // Turn so the marked edges start at edge 0 (one or two marked) .
    let r = match (n, marked) {
        (3, _) => 0,
        (1, _) => marked.iter().position(|&m| m).unwrap(),
        // Two: the unmarked one last.
        (_, _) => (marked.iter().position(|&m| !m).unwrap() + 1) % 3,
    };
    let [a, b, c] = [t.v[r], t.v[(r + 1) % 3], t.v[(r + 2) % 3]];
    let mk = |v: [V; 3]| T { v, surf: t.surf };
    let (ab, bc, ca) = (V::mid(&a, &b), V::mid(&b, &c), V::mid(&c, &a));
    match n {
        1 => out.extend([mk([a, ab, c]), mk([ab, b, c])]),
        2 => out.extend([mk([a, ab, bc]), mk([ab, b, bc]), mk([a, bc, c])]),
        _ => out.extend([mk([a, ab, ca]), mk([ab, b, bc]), mk([ca, bc, c]), mk([ab, bc, ca])]),
    }
}

/// How a surface's vertices are coloured.
#[derive(Clone, Copy, PartialEq)]
enum Lit {
    Map(usize, [i32; 4]),
    Vertex,
    Full,
}

/// Split the triangles where their lightmaps vary (if `refine`), then light
/// every vertex.
fn bake(bsp: &Bsp, lit: &[Lit], mut tris: Vec<T>, src: &JkaSource, refine: bool) -> (Vec<T>, usize) {
    let (scale, tolerance, min_edge) = (src.light_scale, src.light_tolerance, src.light_min_edge);
    let light = |t: &T, v: &V| match lit[t.surf] {
        Lit::Map(page, rect) => bsp.light(page, rect, v.lm),
        _ => None,
    };
    let before = tris.len();
    for _ in 0..if refine { LIGHT_PASSES } else { 0 } {
        let mut marks: HashSet<([i32; 3], [i32; 3])> = HashSet::new();
        for t in &tris {
            if !matches!(lit[t.surf], Lit::Map(..)) {
                continue;
            }
            let l: Vec<Vec3> = t.v.iter().filter_map(|v| light(t, v)).collect();
            if l.len() < 3 {
                continue;
            }
            let off = |got: Option<Vec3>, want: Vec3| got.is_some_and(|g| (g - want).abs().max_element() > tolerance);
            let mut longest = (0, 0.0);
            for k in 0..3 {
                let (a, b) = (&t.v[k], &t.v[(k + 1) % 3]);
                let len = a.p.distance(b.p);
                if len > longest.1 {
                    longest = (k, len);
                }
                if len > min_edge && off(light(t, &V::mid(a, b)), (l[k] + l[(k + 1) % 3]) * 0.5) {
                    marks.insert(edge_key(a.p, b.p));
                }
            }
            // A pool of light inside the triangle: its longest edge goes.
            let c = V::sum(&[(&t.v[0], 1.0 / 3.0), (&t.v[1], 1.0 / 3.0), (&t.v[2], 1.0 / 3.0)]);
            if longest.1 > min_edge && off(light(t, &c), (l[0] + l[1] + l[2]) / 3.0) {
                let k = longest.0;
                marks.insert(edge_key(t.v[k].p, t.v[(k + 1) % 3].p));
            }
        }
        if marks.is_empty() {
            break;
        }
        let mut next = Vec::with_capacity(tris.len() * 2);
        for t in &tris {
            let m: [bool; 3] = std::array::from_fn(|k| marks.contains(&edge_key(t.v[k].p, t.v[(k + 1) % 3].p)));
            split(t, m, &mut next);
        }
        tris = next;
    }
    for t in &mut tris {
        for k in 0..3 {
            let v = t.v[k];
            let c = match lit[t.surf] {
                Lit::Map(..) => light(t, &v).map(|l| [l.x * scale, l.y * scale, l.z * scale, 255.0]).unwrap_or(v.col),
                Lit::Vertex => v.col,
                Lit::Full => [255.0; 4],
            };
            t.v[k].col = c.map(|x| x.clamp(0.0, 255.0));
        }
    }
    let added = tris.len() - before;
    (tris, added)
}

// ── Materials ──────────────────────────────────────────────────────────────

/// A BSP shader, converted.
struct Mat {
    material: usize,
    draw: bool,
    collide: bool,
    xlu: bool,
    vertex_colour: bool,
    floortype: u8,
}

/// `q3map_material` as PD's shot surface and footsteps.
fn surface_of(material: Option<&str>) -> (u8, u8) {
    let m = material.unwrap_or("").to_ascii_lowercase();
    match m.as_str() {
        "rock" | "concrete" | "marble" | "plaster" | "stone" => (SURFACETYPE_STONE, FLOORTYPE_STONE),
        "tiles" => (SURFACETYPE_TILE, FLOORTYPE_STONE),
        "solidwood" | "hollowwood" => (SURFACETYPE_WOOD, FLOORTYPE_WOOD),
        "solidmetal" | "hollowmetal" | "armor" | "computer" => (SURFACETYPE_METAL, FLOORTYPE_METAL),
        "shortgrass" | "longgrass" | "dirt" | "sand" | "gravel" | "dryleaves" | "greenleaves" => (SURFACETYPE_DIRT, FLOORTYPE_DIRT),
        "mud" => (SURFACETYPE_MUD, FLOORTYPE_MUD),
        "snow" | "ice" => (SURFACETYPE_SNOW, FLOORTYPE_SNOW),
        "water" => (SURFACETYPE_SHALLOWWATER, FLOORTYPE_WATER),
        "glass" | "bpglass" | "shatterglass" => (SURFACETYPE_GLASS, FLOORTYPE_DEFAULT),
        "carpet" | "fabric" | "canvas" => (SURFACETYPE_DEFAULT, FLOORTYPE_CARPET),
        _ => (SURFACETYPE_DEFAULT, FLOORTYPE_DEFAULT),
    }
}

/// The image `name` (with or without its extension: `.tga`, `.png`, `.jpg`
/// tried, as the engine does).
fn image(vfs: &Vfs, name: &str) -> Result<Option<image::RgbaImage>, String> {
    let stem = match name.rsplit_once('.') {
        Some((s, "tga" | "jpg" | "jpeg" | "png")) => s,
        _ => name,
    };
    for (ext, fmt) in [("tga", image::ImageFormat::Tga), ("png", image::ImageFormat::Png), ("jpg", image::ImageFormat::Jpeg), ("jpeg", image::ImageFormat::Jpeg)] {
        if let Some(bytes) = vfs.read(&format!("{stem}.{ext}"))? {
            let img = image::load_from_memory_with_format(&bytes, fmt).map_err(|e| format!("{stem}.{ext}: {e}"))?;
            return Ok(Some(img.to_rgba8()));
        }
    }
    Ok(None)
}

fn png(px: &image::RgbaImage) -> Result<Texture, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    px.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(Texture { png: out.into_inner(), w: px.width(), h: px.height() })
}

/// The grey checker a missing image draws, tinted by its name.
fn checker(name: &str) -> image::RgbaImage {
    let h = name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let tint = [(h & 0xff) as f32, ((h >> 8) & 0xff) as f32, ((h >> 16) & 0xff) as f32].map(|c| 0.8 + 0.2 * c / 255.0);
    image::RgbaImage::from_fn(32, 32, |x, y| {
        let l = if (x / 8 + y / 8) % 2 == 0 { 150.0 } else { 105.0 };
        image::Rgba([(l * tint[0]) as u8, (l * tint[1]) as u8, (l * tint[2]) as u8, 255])
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Look {
    Opaque,
    Cutout,
    Blend,
    Add,
}

struct Materials<'a> {
    vfs: &'a Vfs,
    scripts: &'a HashMap<String, Shader>,
    max_texture: u32,
    textures: Vec<Texture>,
    materials: Vec<Material>,
    by_image: HashMap<(String, Look, bool, bool, u8), usize>,
    missing: Vec<String>,
    resized: usize,
}

impl Materials<'_> {
    /// The BSP shader `s`, looked up as `name` (the recipe's substitute, or
    /// its own); its texture made only if a drawn surface uses it (`used`).
    fn convert(&mut self, s: &BspShader, name: &str, used: bool) -> Result<Mat, String> {
        let none = Shader::default();
        let script = self.scripts.get(name);
        let sh = script.unwrap_or(&none);
        let sky = s.flags & SURF_SKY != 0 || sh.has("sky");
        let draw = s.flags & SURF_NODRAW == 0 && !sh.has("nodraw") && !sky;
        let collide = s.contents & (CONTENTS_SOLID | CONTENTS_PLAYERCLIP) != 0;
        let main = sh.main_stage();
        let look = match main {
            Some((_, st)) if st.alpha_func.is_some() => Look::Cutout,
            // A stage blending over nothing drawn before it.
            Some((0, st)) => match st.blend.as_ref().map(|(a, b)| (a.as_str(), b.as_str())) {
                None | Some(("GL_DST_COLOR", "GL_ZERO")) | Some(("GL_ZERO", "GL_SRC_COLOR")) | Some(("GL_ONE", "GL_ZERO")) => Look::Opaque,
                Some(("GL_ONE", "GL_ONE")) => Look::Add,
                Some(_) => Look::Blend,
            },
            _ => Look::Opaque,
        };
        let image_name = main.map(|(_, st)| st.map.clone()).unwrap_or_else(|| name.to_owned());
        let clamp = main.is_some_and(|(_, st)| st.clamp);
        let (surface, floortype) = surface_of(sh.material.as_deref());
        let k = (image_name.clone(), look, clamp, sh.two_sided, surface);
        let material = match self.by_image.get(&k) {
            _ if !draw || !used => usize::MAX,
            Some(&m) => m,
            None => {
                let px = match image(self.vfs, &image_name)? {
                    Some(mut px) => {
                        let (mut w, mut h) = (px.width(), px.height());
                        if w.max(h) > self.max_texture.max(8) {
                            while w.max(h) > self.max_texture.max(8) {
                                (w, h) = ((w / 2).max(1), (h / 2).max(1));
                            }
                            px = image::imageops::resize(&px, w, h, image::imageops::FilterType::Triangle);
                            self.resized += 1;
                        }
                        for p in px.pixels_mut() {
                            match look {
                                Look::Opaque => p.0[3] = 255,
                                // Added light as blended light: as bright,
                                // as opaque as it is bright.
                                Look::Add => {
                                    let a = p.0[0].max(p.0[1]).max(p.0[2]);
                                    for c in 0..3 {
                                        p.0[c] = if a == 0 { 0 } else { (p.0[c] as u32 * 255 / a as u32) as u8 };
                                    }
                                    p.0[3] = a;
                                }
                                _ => {}
                            }
                        }
                        px
                    }
                    None => {
                        self.missing.push(image_name.clone());
                        checker(&image_name)
                    }
                };
                self.textures.push(png(&px)?);
                let blend = match look {
                    Look::Opaque => Blend::Opaque,
                    Look::Cutout => Blend::Cutout,
                    Look::Blend | Look::Add => Blend::Translucent,
                };
                let wrap = if clamp { Wrap::Clamp } else { Wrap::Repeat };
                self.materials.push(Material { texture: Some(self.textures.len() - 1), wrap: [wrap; 2], blend, cull_back: !sh.two_sided, zwrite: blend != Blend::Translucent, decal: false, surface });
                self.by_image.insert(k, self.materials.len() - 1);
                self.materials.len() - 1
            }
        };
        let vertex_colour = main.is_some_and(|(_, st)| st.vertex_colour);
        Ok(Mat { material, draw, collide, xlu: matches!(look, Look::Blend | Look::Add), vertex_colour, floortype })
    }
}

// ── The level ──────────────────────────────────────────────────────────────

/// The player start classes (deathmatch, duel, CTF) and the NPCs.
fn marker_kind(class: &str) -> Option<MarkerKind> {
    let c = class.to_ascii_lowercase();
    if (c.starts_with("info_player_") && c != "info_player_intermission") || (c.starts_with("team_ctf_") && (c.ends_with("player") || c.ends_with("spawn"))) {
        Some(MarkerKind::Spawn)
    } else if c.starts_with("weapon_") {
        Some(MarkerKind::Item)
    } else if c.starts_with("npc_") {
        Some(MarkerKind::Person)
    } else {
        None
    }
}

/// Read the map `bsp` (its bytes) with the game directory `vfs`.
pub fn load(r: &Recipe, src: &JkaSource, bsp_bytes: &[u8], vfs: &Vfs, report: &mut Vec<String>) -> Result<LevelSource, String> {
    let s = r.scale;
    let pd = |p: Vec3| Vec3::new(p.x, p.z, -p.y) * s;
    let bsp = Bsp::parse(bsp_bytes)?;
    if bsp.models.is_empty() {
        return Err("no world model".into());
    }

    // Every shader script, by name.
    let mut scripts = HashMap::new();
    let mut files: Vec<&str> = vfs.names().filter(|n| n.starts_with("shaders/") && n.ends_with(".shader")).collect();
    files.sort();
    for f in &files {
        if let Some(text) = vfs.read(f)? {
            q3shader::parse(&String::from_utf8_lossy(&text), &mut scripts);
        }
    }
    // The world and the static brush entities; the rest listed.
    let mut models = vec![0usize];
    let mut skipped: HashMap<String, usize> = HashMap::new();
    for e in &bsp.entities {
        let Some(m) = e.get("model").and_then(|m| m.strip_prefix('*')).and_then(|m| m.parse::<usize>().ok()) else { continue };
        let class = e.get("classname").map(|c| c.to_ascii_lowercase()).unwrap_or_default();
        if STATIC_ENTITIES.contains(&class.as_str()) && m < bsp.models.len() {
            models.push(m);
        } else {
            *skipped.entry(class).or_default() += 1;
        }
    }

    // The shaders, each under the recipe's substitute if it names one (a
    // texture the map shipped with and the game lacks).
    let used: HashSet<usize> = models.iter().flat_map(|&m| bsp.models[m].surfaces.clone()).filter_map(|i| bsp.surfaces.get(i)).map(|sf| sf.shader).collect();
    let substitute: HashMap<String, String> = src.substitute.iter().map(|(a, b)| (a.to_ascii_lowercase(), b.to_ascii_lowercase())).collect();
    let mut substituted: Vec<String> = Vec::new();
    let mut mats = Materials { vfs, scripts: &scripts, max_texture: src.max_texture, textures: Vec::new(), materials: Vec::new(), by_image: HashMap::new(), missing: Vec::new(), resized: 0 };
    let mut shader_mats: Vec<Mat> = Vec::with_capacity(bsp.shaders.len());
    for (i, sh) in bsp.shaders.iter().enumerate() {
        let name = substitute.get(&sh.name).map_or(sh.name.as_str(), String::as_str);
        if name != sh.name && used.contains(&i) {
            substituted.push(format!("{} as {name}", sh.name));
        }
        shader_mats.push(mats.convert(sh, name, used.contains(&i))?);
    }

    // ── Triangles ──────────────────────────────────────────────────────────
    let mut lit: Vec<Lit> = vec![Lit::Full; bsp.surfaces.len()];
    let mut tris: Vec<T> = Vec::new();
    let mut col_tris: Vec<T> = Vec::new();
    let (mut patches, mut sky) = (0, 0);
    for &m in &models {
        for si in bsp.models[m].surfaces.clone() {
            let Some(sf) = bsp.surfaces.get(si) else { continue };
            let Some(mat) = shader_mats.get(sf.shader) else { continue };
            let sh = &bsp.shaders[sf.shader];
            if sh.flags & SURF_SKY != 0 {
                sky += 1;
            }
            lit[si] = if sf.lm >= 0 {
                Lit::Map(sf.lm as usize, sf.lm_rect)
            } else if sf.lm == LIGHTMAP_BY_VERTEX || mat.vertex_colour {
                Lit::Vertex
            } else {
                Lit::Full
            };
            let Some(verts) = bsp.verts.get(sf.verts.0..sf.verts.0 + sf.verts.1) else { continue };
            let mut surf_tris = Vec::new();
            match sf.kind {
                MST_PLANAR | MST_TRIANGLE_SOUP => {
                    let Some(idx) = bsp.indexes.get(sf.indexes.0..sf.indexes.0 + sf.indexes.1) else { continue };
                    for t in idx.chunks_exact(3) {
                        if t.iter().any(|&i| i >= verts.len()) {
                            continue;
                        }
                        let v = [verts[t[0]], verts[t[1]], verts[t[2]]];
                        let hint = if sf.kind == MST_PLANAR && sf.normal.length_squared() > 0.5 { sf.normal } else { v[0].n + v[1].n + v[2].n };
                        surf_tris.push(T { v: orient(v, hint), surf: si });
                    }
                }
                MST_PATCH => {
                    patches += 1;
                    let (grid, w) = tessellate(verts, sf.patch.0, sf.patch.1);
                    if w < 2 {
                        continue;
                    }
                    let h = grid.len() / w;
                    for j in 0..h - 1 {
                        for i in 0..w - 1 {
                            let q = [grid[j * w + i], grid[j * w + i + 1], grid[(j + 1) * w + i + 1], grid[(j + 1) * w + i]];
                            for v in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                                if (v[1].p - v[0].p).cross(v[2].p - v[0].p).length_squared() > 1e-8 {
                                    surf_tris.push(T { v: orient(v, v[0].n + v[1].n + v[2].n), surf: si });
                                }
                            }
                        }
                    }
                }
                _ => continue,
            }
            if mat.collide && sf.kind != MST_TRIANGLE_SOUP {
                col_tris.extend(surf_tris.iter().copied());
            }
            if mat.draw {
                tris.extend(surf_tris);
            }
        }
    }
    let quake_tris = tris.len();
    let (tris, added) = bake(&bsp, &lit, tris, src, true);
    // The collision's floor colours from its lighting too (unsplit).
    let (col_tris, _) = bake(&bsp, &lit, col_tris, src, false);

    let byte = |c: [f32; 4]| c.map(|x| x.round() as u8);
    let out_tris: Vec<Tri> = tris
        .iter()
        .map(|t| {
            let m = &shader_mats[bsp.surfaces[t.surf].shader];
            Tri { v: t.v.map(|v| Vert { pos: pd(v.p), uv: v.st, col: byte(v.col) }), material: m.material, xlu: m.xlu }
        })
        .collect();

    // ── Collision ──────────────────────────────────────────────────────────
    let mut collision: Vec<ColPoly> = Vec::new();
    for t in &col_tris {
        let m = &shader_mats[bsp.surfaces[t.surf].shader];
        let p = t.v.map(|v| pd(v.p));
        if let Some((flags, floorcol)) = tile_of_triangle(p, t.v.map(|v| byte(v.col)), m.xlu) {
            collision.push(ColPoly { verts: p.to_vec(), flags, floortype: m.floortype, grab: src.climb_ledges, floorcol });
        }
    }
    // Solid brushes with nothing drawn: every face.
    let mut hidden_brushes = 0;
    for &m in &models {
        for b in bsp.models[m].brushes.clone() {
            let Some(brush) = bsp.brushes.get(b) else { continue };
            let contents = bsp.shaders.get(brush.shader).map_or(0, |s| s.contents);
            let drawn = brush.sides.clone().filter_map(|i| bsp.sides.get(i)).any(|sd| shader_mats.get(sd.shader).is_some_and(|m| m.draw));
            if drawn || contents & (CONTENTS_SOLID | CONTENTS_PLAYERCLIP) == 0 {
                continue;
            }
            hidden_brushes += 1;
            let planes: Vec<(Vec3, f32)> = brush.sides.clone().filter_map(|i| bsp.sides.get(i)).filter_map(|sd| bsp.planes.get(sd.plane).copied()).collect();
            for (i, &(n, d)) in planes.iter().enumerate() {
                // A big square on the plane, wound about n, cut by the others.
                let u = if n.z.abs() < 0.9 { Vec3::Z.cross(n).normalize() } else { Vec3::X.cross(n).normalize() };
                let v = n.cross(u);
                let c = n * d;
                const R: f32 = 65536.0;
                let mut poly = vec![c - u * R - v * R, c + u * R - v * R, c + u * R + v * R, c - u * R + v * R];
                for (j, &(n2, d2)) in planes.iter().enumerate() {
                    if j != i && poly.len() >= 3 {
                        poly = crate::rooms::clip_plane(&poly, n2, d2);
                    }
                }
                let poly: Vec<Vec3> = poly.into_iter().map(pd).collect();
                for k in 1..poly.len().saturating_sub(1) {
                    let tri = [poly[0], poly[k], poly[k + 1]];
                    if let Some((flags, _)) = tile_of_triangle(tri, [[128, 128, 128, 255]; 3], false) {
                        collision.push(ColPoly { verts: tri.to_vec(), flags, floortype: FLOORTYPE_DEFAULT, grab: src.climb_ledges, floorcol: 0 });
                    }
                }
            }
        }
    }

    // ── Markers ────────────────────────────────────────────────────────────
    let mut markers = Vec::new();
    let mut flags = 0;
    for e in &bsp.entities {
        let class = e.get("classname").map(String::as_str).unwrap_or("");
        if class.eq_ignore_ascii_case("team_ctf_redflag") || class.eq_ignore_ascii_case("team_ctf_blueflag") {
            flags += 1;
        }
        let Some(kind) = marker_kind(class) else { continue };
        let Some(o) = e.get("origin").map(|o| o.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>()).filter(|o| o.len() == 3) else { continue };
        // The origin stands 24 units over the feet (`DEFAULT_MINS_2`).
        let pos = pd(Vec3::new(o[0], o[1], o[2] - 24.0));
        let yaw = e.get("angle").and_then(|a| a.parse::<f32>().ok()).or_else(|| e.get("angles").and_then(|a| a.split_whitespace().nth(1)?.parse().ok())).unwrap_or(0.0).to_radians();
        // Quake's yaw looks along (cos, sin, 0): PD's (cos, 0, -sin).
        markers.push(Marker { kind, pos, facing: yaw.cos().atan2(-yaw.sin()) });
    }

    // ── Environment ────────────────────────────────────────────────────────
    let sky_colour = match src.sky {
        Some(c) => c,
        None => sky_average(&bsp, &scripts, vfs)?.unwrap_or([0, 0, 0]),
    };
    let (lo, hi) = out_tris.iter().flat_map(|t| t.v.iter().map(|v| v.pos)).fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), p| (a.min(p), b.max(p)));
    if out_tris.is_empty() {
        return Err("no drawn triangles".into());
    }

    let mut skipped: Vec<(String, usize)> = skipped.into_iter().collect();
    skipped.sort();
    mats.missing.sort();
    mats.missing.dedup();
    report.push(format!(
        "jka: {} surfaces ({} patches, {} sky), {} shaders ({} with scripts of {} read from {} packages), {} materials, {} textures ({} resized to {} or less); {} Quake triangles, {} added where the lightmaps vary; {} hidden solid brushes collide; {} markers, {} CTF flags; {} fog volumes not converted",
        bsp.surfaces.len(),
        patches,
        sky,
        bsp.shaders.len(),
        bsp.shaders.iter().filter(|s| scripts.contains_key(&s.name)).count(),
        scripts.len(),
        vfs.pk3_count(),
        mats.materials.len(),
        mats.textures.len(),
        mats.resized,
        src.max_texture,
        quake_tris,
        added,
        hidden_brushes,
        markers.len(),
        flags,
        bsp.fogs
    ));
    if !substituted.is_empty() {
        report.push(format!("jka: the recipe's substitutes: {}", substituted.join(", ")));
    }
    if !mats.missing.is_empty() {
        report.push(format!("jka: images not found (a grey checker drawn): {}", mats.missing.join(", ")));
    }
    if !skipped.is_empty() {
        report.push(format!("jka: brush entities not converted: {}", skipped.iter().map(|(c, n)| format!("{c} x{n}")).collect::<Vec<_>>().join(", ")));
    }
    Ok(LevelSource {
        textures: mats.textures,
        materials: mats.materials,
        tris: out_tris,
        collision,
        markers,
        env: Env { sky: sky_colour, near: 15.0, far: 10000f32.max((hi - lo).length() * 1.2), fog: None },
    })
}

/// The average colour of the map's sky box (its `skyParms` images), if it
/// has a sky.
fn sky_average(bsp: &Bsp, scripts: &HashMap<String, Shader>, vfs: &Vfs) -> Result<Option<[u8; 3]>, String> {
    let Some(sky) = bsp.shaders.iter().filter(|s| s.flags & SURF_SKY != 0).find_map(|s| scripts.get(&s.name)) else { return Ok(None) };
    let mut names: Vec<String> = Vec::new();
    if let Some(b) = &sky.sky_box {
        names.extend(["ft", "bk", "lf", "rt", "up"].iter().map(|f| format!("{b}_{f}")));
    }
    names.extend(sky.main_stage().map(|(_, st)| st.map.clone()));
    let (mut sum, mut n) = (Vec3::ZERO, 0.0);
    for name in names {
        if let Some(px) = image(vfs, &name)? {
            for p in px.pixels() {
                sum += Vec3::new(p.0[0] as f32, p.0[1] as f32, p.0[2] as f32);
                n += 1.0;
            }
        }
    }
    Ok((n > 0.0).then(|| (sum / n).to_array().map(|c| c.round() as u8)))
}

/// The map's name: its worldspawn's `message`.
pub fn map_title(bsp: &[u8]) -> Option<String> {
    let l = Lumps::new(bsp).ok()?;
    let ents = parse_entities(&String::from_utf8_lossy(l.bytes(0)));
    ents.into_iter().find(|e| e.get("classname").is_some_and(|c| c.eq_ignore_ascii_case("worldspawn")))?.remove("message")
}

/// The map's bytes: a file, or a path inside the game's packages
/// (`maps/mp/ffa1.bsp`).
pub fn read_map(bsp: &str, vfs: &Vfs) -> Result<Vec<u8>, String> {
    let path = Path::new(bsp);
    if path.is_file() {
        return std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    }
    vfs.read(bsp)?.ok_or_else(|| format!("{bsp}: neither a file nor in the game's packages"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(p: Vec3) -> V {
        V { p, st: Vec2::new(p.x, p.y) / 64.0, lm: Vec2::ZERO, n: Vec3::Z, col: [255.0; 4] }
    }

    #[test]
    fn patches_tessellate_by_their_bend() {
        // A flat 3 x 3 patch is one quad; a half-pipe bends one way only.
        let flat: Vec<V> = (0..9).map(|k| v(Vec3::new((k % 3) as f32 * 64.0, (k / 3) as f32 * 64.0, 0.0))).collect();
        let (g, w) = tessellate(&flat, 3, 3);
        assert_eq!((g.len(), w), (4, 2));
        let pipe: Vec<V> = (0..9).map(|k| v(Vec3::new((k % 3) as f32 * 64.0, (k / 3) as f32 * 64.0, if k % 3 == 1 { 128.0 } else { 0.0 }))).collect();
        let (g, w) = tessellate(&pipe, 3, 3);
        assert!(w > 4, "bent along x: {w} columns");
        assert_eq!(g.len() / w, 2, "straight along y");
        // The curve tops out at half the control's height; the chords come
        // within the tolerance of it.
        let top = g.iter().map(|p| p.p.z).fold(0.0, f32::max);
        assert!(top <= 64.0 + 1e-3 && top > 64.0 - PATCH_TOLERANCE, "{top}");
        // The corners are the control points.
        assert_eq!(g[0].p, pipe[0].p);
        assert_eq!(g[g.len() - 1].p, pipe[8].p);
    }

    #[test]
    fn triangles_face_their_normals() {
        let a = [v(Vec3::ZERO), v(Vec3::X * 10.0), v(Vec3::Y * 10.0)];
        let up = orient(a, Vec3::Z);
        assert!((up[1].p - up[0].p).cross(up[2].p - up[0].p).z > 0.0);
        let down = orient(a, -Vec3::Z);
        assert!((down[1].p - down[0].p).cross(down[2].p - down[0].p).z < 0.0);
        // Without a hint, Quake's clockwise winding is turned.
        assert_eq!(orient(a, Vec3::ZERO)[1].p, a[2].p);
    }

    #[test]
    fn splits_keep_neighbours_meeting() {
        // Two triangles sharing an edge; only the first is marked to split it
        // (and its other two edges): both get the shared edge's middle.
        let (p0, p1, p2, p3) = (Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0), Vec3::new(0.0, 100.0, 0.0), Vec3::new(100.0, 100.0, 0.0));
        let a = T { v: [v(p0), v(p1), v(p2)], surf: 0 };
        let b = T { v: [v(p1), v(p3), v(p2)], surf: 1 };
        let marks: HashSet<_> = [edge_key(p1, p2), edge_key(p0, p1)].into_iter().collect();
        let mut out = Vec::new();
        for t in [&a, &b] {
            let m: [bool; 3] = std::array::from_fn(|k| marks.contains(&edge_key(t.v[k].p, t.v[(k + 1) % 3].p)));
            split(t, m, &mut out);
        }
        assert_eq!(out.len(), 3 + 2);
        let mid = (p1 + p2) * 0.5;
        assert_eq!(out.iter().filter(|t| t.surf == 0 && t.v.iter().any(|x| x.p == mid)).count(), 3, "two edges split: the shared middle is in all three");
        assert_eq!(out.iter().filter(|t| t.surf == 1 && t.v.iter().any(|x| x.p == mid)).count(), 2);
        // Every piece keeps the winding and the area adds up.
        let area = |t: &T| (t.v[1].p - t.v[0].p).cross(t.v[2].p - t.v[0].p).z * 0.5;
        assert!(out.iter().all(|t| area(t) > 0.0));
        assert!((out.iter().map(area).sum::<f32>() - 10000.0).abs() < 0.01);
    }

    #[test]
    fn entities_parse() {
        let e = parse_entities("{\n\"classname\" \"worldspawn\"\n}\n{\n\"origin\" \"1 2 3\"\n\"ClassName\" \"info_player_deathmatch\"\n\"angle\" \"90\"\n}\n");
        assert_eq!(e.len(), 2);
        assert_eq!(e[1]["classname"], "info_player_deathmatch");
        assert_eq!(e[1]["origin"], "1 2 3");
        assert_eq!(marker_kind("info_player_deathmatch"), Some(MarkerKind::Spawn));
        assert_eq!(marker_kind("info_player_intermission"), None);
        assert_eq!(marker_kind("team_CTF_bluespawn"), Some(MarkerKind::Spawn));
        assert_eq!(marker_kind("weapon_repeater"), Some(MarkerKind::Item));
        assert_eq!(marker_kind("ammo_powercell"), None);
    }
}
