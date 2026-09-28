//! What a shot hits in the BG: PD tests the rooms' *display list* triangles
//! (`bg_test_hit_in_room` → `bg_test_hit_in_vtx_batch`, `bg.c:4137`), not the
//! collision tiles, and the texture of the triangle it hits picks the bullet
//! hole, the hit sound and the sparks through `g_Textures[].surfacetype` and
//! `soundsurfacetype` (`tex.c:160`).
//!
//! A stage's mesh is its textured BG (`bg.json`, one batch per room, layer and
//! material); a box fixture's is its polygons, all of `g_SurfaceTypeDefault`.
//!
//! `// SUBST:` PD tests only the rooms the shot passes through
//! (`portal_find_rooms`) plus the forced-onscreen ones / every room's
//! triangles, keeping the nearest hit, until M9's portals.

use std::collections::HashMap;

use glam::Vec3;
use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_core::math::func0002f560;
use pd_core::model::ModelDef;
use serde::Deserialize;

use super::geom::LevelGeom;

/// `SURFACETYPE_DEFAULT`.
pub const SURFACETYPE_DEFAULT: u8 = 0;
pub const SURFACETYPE_SHALLOWWATER: u8 = 5;
pub const SURFACETYPE_DEEPWATER: u8 = 14;

/// `struct surfacetype` (`tex.c`): the hit sounds and the bullet-hole textures.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceType {
    pub sounds: &'static [u16],
    pub wallhittexes: &'static [usize],
}

/// `g_SurfaceTypes[]` (`tex.c:160`), by `SURFACETYPE_*`.
pub const SURFACE_TYPES: [SurfaceType; 15] = [
    /* 0 default */ SurfaceType { sounds: &[0x8087, 0x8088], wallhittexes: &[WALLHITTEX_BULLET2] },
    /* 1 stone */ SurfaceType { sounds: &[0x8087, 0x8088], wallhittexes: &[WALLHITTEX_BULLET1] },
    /* 2 wood */ SurfaceType { sounds: &[0x807e, 0x807f], wallhittexes: &[WALLHITTEX_WOOD] },
    /* 3 metal */ SurfaceType { sounds: &[0x8079, 0x807b], wallhittexes: &[WALLHITTEX_METAL] },
    /* 4 glass */ SurfaceType { sounds: &[0x8077], wallhittexes: &[WALLHITTEX_GLASS1, WALLHITTEX_GLASS2, WALLHITTEX_GLASS3] },
    /* 5 shallow water */ SurfaceType { sounds: &[0x8080], wallhittexes: &[WALLHITTEX_WATER] },
    /* 6 snow */ SurfaceType { sounds: &[0x807d], wallhittexes: &[WALLHITTEX_BULLET1] },
    /* 7 dirt */ SurfaceType { sounds: &[0x8084, 0x8085], wallhittexes: &[WALLHITTEX_SOFT] },
    /* 8 mud */ SurfaceType { sounds: &[0x8081, 0x8082, 0x8083], wallhittexes: &[WALLHITTEX_SOFT] },
    /* 9 tile */ SurfaceType { sounds: &[0x8086], wallhittexes: &[WALLHITTEX_BULLET1] },
    /*10 metal obj */ SurfaceType { sounds: &[0x8089, 0x808a], wallhittexes: &[WALLHITTEX_BULLET1, WALLHITTEX_BULLET2] },
    /*11 chr */ SurfaceType { sounds: &[0x8076], wallhittexes: &[WALLHITTEX_SOFT] },
    /*12 glass xlu */ SurfaceType { sounds: &[0x8077], wallhittexes: &[WALLHITTEX_GLASS1, WALLHITTEX_GLASS2, WALLHITTEX_GLASS3] },
    /*13 none */ SurfaceType { sounds: &[], wallhittexes: &[] },
    /*14 deep water */ SurfaceType { sounds: &[0x8080], wallhittexes: &[WALLHITTEX_WATER] },
];

pub fn surface_type(t: u8) -> &'static SurfaceType {
    &SURFACE_TYPES[(t as usize).min(SURFACE_TYPES.len() - 1)]
}

/// A hit triangle's texture as the shot code reads it: `g_Textures[texturenum]`'s
/// two surface types. `None` is PD's `texturenum == -1` (no texture loaded).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TexSurface {
    pub soundsurfacetype: u8,
    pub surfacetype: u8,
}

#[derive(Clone, Copy, Debug)]
struct BgTri {
    p: [Vec3; 3],
}

#[derive(Clone, Debug)]
struct BgBatch {
    bbmin: Vec3,
    bbmax: Vec3,
    tris: std::ops::Range<usize>,
    room: u16,
    /// `VTXBATCHTYPE_XLU`.
    xlu: bool,
    surface: Option<TexSurface>,
}

/// `struct hitthing`, the fields the shot reads.
#[derive(Clone, Copy, Debug)]
pub struct BgHit {
    pub pos: Vec3,
    /// `unk0c`: the triangle's normal, not normalised.
    pub normal: Vec3,
    pub room: u16,
    /// `unk2c == VTXBATCHTYPE_XLU`.
    pub xlu: bool,
    pub surface: Option<TexSurface>,
}

/// Every BG triangle a shot can hit, in batches with their boxes.
#[derive(Clone, Debug, Default)]
pub struct BgHitMesh {
    tris: Vec<BgTri>,
    batches: Vec<BgBatch>,
}

#[derive(Deserialize)]
struct RawNode {
    room: Option<u16>,
    layer: Option<String>,
}

#[derive(Deserialize)]
struct RawBg {
    nodes: Vec<RawNode>,
}

#[derive(Deserialize)]
struct PoolEntry {
    #[serde(default)]
    soundsurfacetype: u8,
    #[serde(default)]
    surfacetype: u8,
}

impl BgHitMesh {
    /// A stage's textured BG, `stages/<code>/bg.json` + `bg.bin`.
    pub fn load(assets: &AssetDir, code: &str) -> Result<BgHitMesh, String> {
        let dir = assets.stage(code);
        let def = ModelDef::load_file(assets, &dir.join("bg.json"), &dir.join("bg.bin"))?;
        let raw: RawBg = assets.read_json(&dir.join("bg.json"))?;
        let pool: HashMap<String, PoolEntry> = assets.read_json(&assets.texture_index())?;
        let mut mesh = BgHitMesh::default();
        for b in &def.batches {
            let node = raw.nodes.get(b.node).ok_or("bg.json: a batch's node is missing")?;
            let Some(room) = node.room else { continue };
            let m = &def.materials[b.material];
            // A pool texture's number; a texture stored in the file has none.
            let surface = m.texture.as_ref().filter(|t| t.id < 0x10000).map(|t| {
                let e = pool.get(&format!("{:04x}", t.id));
                TexSurface { soundsurfacetype: e.map_or(0, |e| e.soundsurfacetype), surfacetype: e.map_or(0, |e| e.surfacetype) }
            });
            let start = mesh.tris.len();
            let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for t in b.idx.chunks_exact(3) {
                let p = [b.verts[t[0] as usize].pos, b.verts[t[1] as usize].pos, b.verts[t[2] as usize].pos];
                for v in p {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
                mesh.tris.push(BgTri { p });
            }
            mesh.batches.push(BgBatch { bbmin: lo, bbmax: hi, tris: start..mesh.tris.len(), room, xlu: node.layer.as_deref() == Some("xlu"), surface });
        }
        Ok(mesh)
    }

    /// A box fixture's shot-blocking polygons as triangle fans, every one of
    /// `g_SurfaceTypeDefault`.
    pub fn from_geom(geom: &LevelGeom) -> BgHitMesh {
        let mut mesh = BgHitMesh::default();
        let surface = Some(TexSurface { soundsurfacetype: SURFACETYPE_DEFAULT, surfacetype: SURFACETYPE_DEFAULT });
        for poly in geom.polys.iter().filter(|p| p.blocks_shot) {
            let start = mesh.tris.len();
            let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for v in &poly.verts {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            for i in 1..poly.verts.len().saturating_sub(1) {
                mesh.tris.push(BgTri { p: [poly.verts[0], poly.verts[i], poly.verts[i + 1]] });
            }
            mesh.batches.push(BgBatch { bbmin: lo, bbmax: hi, tris: start..mesh.tris.len(), room: poly.room.unwrap_or(0), xlu: false, surface });
        }
        mesh
    }

    pub fn num_tris(&self) -> usize {
        self.tris.len()
    }

    /// Every triangle whose batch's box meets `lo..hi` (the x-ray draws them).
    pub fn tris_in(&self, lo: Vec3, hi: Vec3) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.batches.iter().filter(move |b| !(b.bbmax.cmplt(lo).any() || b.bbmin.cmpgt(hi).any())).flat_map(move |b| self.tris[b.tris.clone()].iter().map(|t| t.p))
    }

    /// `bg_test_hit_in_room` over every room: the nearest triangle the segment
    /// `from..to` crosses. A translucent batch whose texture is of the default
    /// surface type lets the shot through (`bg.c:4309`).
    pub fn bg_test_hit(&self, from: Vec3, to: Vec3) -> Option<BgHit> {
        let dir = to - from;
        let (seglo, seghi) = (from.min(to), from.max(to));
        let mut best: Option<(f32, BgHit)> = None;
        for b in &self.batches {
            if b.bbmax.cmplt(seglo).any() || b.bbmin.cmpgt(seghi).any() || !segment_hits_box(from, dir, b.bbmin, b.bbmax) {
                continue;
            }
            if b.xlu && b.surface.is_some_and(|s| s.surfacetype == SURFACETYPE_DEFAULT) {
                continue;
            }
            for t in &self.tris[b.tris.clone()] {
                if let Some((pos, normal)) = func0002f560(t.p[0], t.p[1], t.p[2], from, to, dir) {
                    let d = (pos - from).length_squared();
                    if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, BgHit { pos, normal, room: b.room, xlu: b.xlu, surface: b.surface }));
                    }
                }
            }
        }
        best.map(|(_, h)| h)
    }
}

/// Whether the segment `from + dir·t`, t in 0..1, meets the box (slabs).
fn segment_hits_box(from: Vec3, dir: Vec3, lo: Vec3, hi: Vec3) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for a in 0..3 {
        if dir[a].abs() < 1e-12 {
            if from[a] < lo[a] || from[a] > hi[a] {
                return false;
            }
            continue;
        }
        let (mut ta, mut tb) = ((lo[a] - from[a]) / dir[a], (hi[a] - from[a]) / dir[a]);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::fixtures;

    /// The firing range: a level shot down the hall stops on the far wall; one
    /// at the first crate stops on its face.
    #[test]
    fn a_shot_stops_on_the_first_bg_triangle() {
        let m = BgHitMesh::from_geom(&fixtures::firing_range());
        let far = m.bg_test_hit(Vec3::new(0.0, 150.0, 0.0), Vec3::new(0.0, 150.0, 65536.0)).unwrap();
        assert!((far.pos.z - 3300.0).abs() < 0.01, "{:?}", far.pos);
        let crate_ = m.bg_test_hit(Vec3::new(-250.0, 50.0, 0.0), Vec3::new(-250.0, 50.0, 65536.0)).unwrap();
        assert!((crate_.pos.z - 300.0).abs() < 0.01, "{:?}", crate_.pos);
        assert_eq!(crate_.surface.unwrap().surfacetype, SURFACETYPE_DEFAULT);
    }

    /// Complex's BG: every triangle loads, and a shot at the floor from a spawn
    /// pad lands on it with the floor texture's surface.
    #[test]
    fn complex_has_textured_bg_hits() {
        let a = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
        let m = BgHitMesh::load(&a, "ref").unwrap();
        assert_eq!(m.num_tris(), 2559);
        let stage = crate::stage::Stage::load(&a, "ref").unwrap();
        let pad = &stage.pads[stage.spawn_pads[0]];
        let hit = m.bg_test_hit(pad.pos, pad.pos - Vec3::Y * 1000.0).expect("the floor under a spawn pad");
        assert!(hit.pos.y < pad.pos.y && pad.pos.y - hit.pos.y < 100.0, "{:?} under {:?}", hit.pos, pad.pos);
        assert!(hit.surface.is_some(), "the floor is textured");
    }
}
