//! The level geometry every consumer reads: collision ([`super::TileLevel`]),
//! the waypoint generator (M6, `pd_sim::nav`) and the debug viewer. A polygon
//! carries its outline and the flags the chr code asks about: walkable floor,
//! wall, blocks sight, blocks shots, ladder, crouch/duck zone, floor type and
//! room. PD's collision tiles are one source ([`super::Stage::load`]); the test
//! fixtures ([`super::fixtures`]) are another, which is what keeps the waypoint
//! generator usable on geometry PD has no graph for (ARCHITECTURE.md D8).
//!
//! Units are PD world units (centimetres), Y up.
//!
//! Source: the old repo's `pd_spike/level_geom.rs`.

use glam::Vec3;

/// A floor polygon is "flat" when it tilts less than this (5°).
const FLAT_COS: f32 = 0.996_2;
/// ...and "vertical" (a floor-flagged riser, not something to stand on) past 60°.
const VERTICAL_COS: f32 = 0.5;

/// One planar, convex polygon of the level.
#[derive(Clone, Debug)]
pub struct GeomPoly {
    pub verts: Vec<Vec3>,
    /// Unit normal from the outline (Newell's method). Winding is **not** reliable
    /// in source data — 19 of Complex's floor tiles are wound downwards — so use
    /// [`GeomPoly::tilt_cos`] (which ignores the sign) to decide orientation.
    pub normal: Vec3,
    /// Something a chr can stand on.
    pub floor: bool,
    /// Something a chr collides with sideways.
    pub wall: bool,
    pub blocks_sight: bool,
    pub blocks_shot: bool,
    /// The room this polygon belongs to, when the source has rooms.
    pub room: Option<u16>,
    /// A climbable wall: a chr on a go-to that touches it climbs instead of
    /// walking. Our editor's ladders are the same thing.
    pub ladder: bool,
    /// Floor where a chr must crouch to 90 cm (a crawl space, a vent).
    pub crouch: bool,
    /// Floor where a chr must duck to 135 cm.
    pub duck: bool,
    /// What the floor is made of, for footsteps: PD's `FLOORTYPE_*`
    /// (`constants.h:961`: 0 default/none, 1 wood, 2 stone, 3 carpet, 4 metal,
    /// 5 mud, 6 water, 7 dirt, 8 snow). An editor level would set it per material.
    pub floortype: u8,
}

/// How a floor polygon should be treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloorKind {
    Flat,
    Ramp,
    /// Floor-flagged but steeper than 60°: step risers and the like.
    Vertical,
}

impl GeomPoly {
    pub fn new(verts: Vec<Vec3>, floor: bool, wall: bool, blocks_sight: bool, blocks_shot: bool, room: Option<u16>) -> Self {
        let normal = newell_normal(&verts);
        GeomPoly { verts, normal, floor, wall, blocks_sight, blocks_shot, room, ladder: false, crouch: false, duck: false, floortype: 0 }
    }

    /// |cos| of the angle between the polygon's plane normal and +Y: 1 = level.
    pub fn tilt_cos(&self) -> f32 {
        self.normal.y.abs()
    }

    pub fn floor_kind(&self) -> Option<FloorKind> {
        if !self.floor {
            return None;
        }
        let c = self.tilt_cos();
        Some(if c >= FLAT_COS {
            FloorKind::Flat
        } else if c >= VERTICAL_COS {
            FloorKind::Ramp
        } else {
            FloorKind::Vertical
        })
    }

    pub fn min_y(&self) -> f32 {
        self.verts.iter().map(|v| v.y).fold(f32::INFINITY, f32::min)
    }

    pub fn max_y(&self) -> f32 {
        self.verts.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max)
    }

    /// Is `(x, z)` inside the polygon's footprint (its XZ projection)?
    pub fn contains_xz(&self, x: f32, z: f32) -> bool {
        let v = &self.verts;
        let mut inside = false;
        let mut j = v.len() - 1;
        for i in 0..v.len() {
            let (a, b) = (v[i], v[j]);
            if (a.z > z) != (b.z > z) {
                let xi = a.x + (z - a.z) * (b.x - a.x) / (b.z - a.z);
                if x < xi {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }

    /// `cd_is_xz_in_tilei` (`collision.c:691`): inside the convex outline's XZ
    /// projection, by every edge's cross product having one sign. A vertical
    /// polygon projects to a line and never contains a point.
    pub fn xz_in_convex(&self, x: f32, z: f32) -> bool {
        let v = &self.verts;
        let mut result: i32 = -1;
        for i in 0..v.len() {
            let next = (i + 1) % v.len();
            let value = (v[next].z - v[i].z) * (x - v[i].x) - (v[next].x - v[i].x) * (z - v[i].z);
            if value != 0.0 {
                if i == 0 || result < 0 {
                    result = (value > 0.0) as i32;
                } else if (result != 0 && value < 0.0) || (result == 0 && value > 0.0) {
                    return false;
                }
            }
        }
        result >= 0
    }

    /// `cd_find_y_tilei` (`collision.c:619`): PD evaluates a tile's height on the
    /// fan triangle `(0, i, i+1)` that holds the point, so a slightly non-planar
    /// quad is followed exactly, then clamps to the tile's own y range.
    pub fn find_y(&self, x: f32, z: f32) -> f32 {
        let v = &self.verts;
        let mut i = 1;
        let mut ival: i32 = -1;
        if v.len() >= 4 {
            while i < v.len() {
                let (tmpx, tmpz) = (v[i].x, v[i].z);
                let fval = (v[0].z - tmpz) * (x - tmpx) - (v[0].x - tmpx) * (z - tmpz);
                if fval != 0.0 {
                    if ival < 0 {
                        ival = (fval > 0.0) as i32;
                    } else if (ival != 0 && fval < 0.0) || (ival == 0 && fval > 0.0) {
                        i -= 1;
                        break;
                    }
                }
                i += 1;
            }
        }
        self.find_y_vtx(x, z, i)
    }

    /// `cd_find_y_tilei_vtx` (`collision.c:560`): height at `(x, z)` on the plane
    /// of vertices `0`, `vertexindex`, `vertexindex + 1`, clamped to the y range.
    pub fn find_y_vtx(&self, x: f32, z: f32, vertexindex: usize) -> f32 {
        let v = &self.verts;
        let n = v.len();
        let vi = if vertexindex == 0 { 1 } else { vertexindex.min(n - 1) };
        let mut next = (vi + 1) % n;
        if next == 0 {
            next = 1;
        }
        let a = v[vi] - v[0];
        let b = v[next] - v[0];
        let (sp58, sp60, sp68) = (
            (a.y * b.z - a.z * b.y) as f64,
            (a.z * b.x - a.x * b.z) as f64,
            (a.x * b.y - a.y * b.x) as f64,
        );
        let (ymin, ymax) = (self.min_y(), self.max_y());
        if sp60 == 0.0 {
            return ymax;
        }
        let tmp = sp58 * v[0].x as f64 + sp60 * v[0].y as f64 + sp68 * v[0].z as f64;
        let ground = ((tmp - x as f64 * sp58 - z as f64 * sp68) / sp60) as f32;
        ground.clamp(ymin, ymax)
    }

    /// Height of the polygon's plane at `(x, z)`, or `None` for a vertical polygon.
    pub fn y_at(&self, x: f32, z: f32) -> Option<f32> {
        let n = self.normal;
        if n.y.abs() < 1e-4 {
            return None;
        }
        let a = self.verts[0];
        Some(a.y - (n.x * (x - a.x) + n.z * (z - a.z)) / n.y)
    }
}

/// A whole level: a soup of polygons, optionally grouped into rooms.
#[derive(Clone, Debug, Default)]
pub struct LevelGeom {
    pub polys: Vec<GeomPoly>,
    /// Every room number the source names (some have no tiles), ascending.
    pub rooms: Vec<u16>,
}

impl LevelGeom {
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.polys {
            for v in &p.verts {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        (lo, hi)
    }

    /// The highest non-vertical floor surface under `(x, z)` whose height is at most
    /// `max_y`, as `(height, polygon index)`. Brute force; fine at ~1,200 polygons.
    pub fn floor_below(&self, x: f32, z: f32, max_y: f32) -> Option<(f32, usize)> {
        let mut best: Option<(f32, usize)> = None;
        for (i, p) in self.polys.iter().enumerate() {
            if !matches!(p.floor_kind(), Some(FloorKind::Flat | FloorKind::Ramp)) || !p.contains_xz(x, z) {
                continue;
            }
            let Some(y) = p.y_at(x, z) else { continue };
            if y <= max_y && best.is_none_or(|(by, _)| y > by) {
                best = Some((y, i));
            }
        }
        best
    }
}

fn newell_normal(v: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for i in 0..v.len() {
        let (a, b) = (v[i], v[(i + 1) % v.len()]);
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n.normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(y0: f32, y1: f32) -> GeomPoly {
        // A 2 m square rising from y0 at z = 0 to y1 at z = 200.
        let verts = vec![
            Vec3::new(0.0, y0, 0.0),
            Vec3::new(200.0, y0, 0.0),
            Vec3::new(200.0, y1, 200.0),
            Vec3::new(0.0, y1, 200.0),
        ];
        GeomPoly::new(verts, true, false, true, true, None)
    }

    #[test]
    fn floors_classify_by_tilt_whatever_their_winding() {
        let flat = quad(0.0, 0.0);
        assert_eq!(flat.floor_kind(), Some(FloorKind::Flat));
        let mut down = flat.clone();
        down.verts.reverse();
        let down = GeomPoly::new(down.verts, true, false, true, true, None);
        assert!(down.normal.y * flat.normal.y < 0.0, "reversing the winding flips the normal");
        assert_eq!(down.floor_kind(), Some(FloorKind::Flat));
        assert_eq!(quad(0.0, 100.0).floor_kind(), Some(FloorKind::Ramp));
        assert_eq!(quad(0.0, 1000.0).floor_kind(), Some(FloorKind::Vertical));
    }

    #[test]
    fn a_ramp_reports_its_height_under_a_point() {
        let ramp = quad(0.0, 100.0);
        assert!(ramp.contains_xz(100.0, 100.0));
        assert!(!ramp.contains_xz(-1.0, 100.0));
        assert!((ramp.y_at(100.0, 100.0).unwrap() - 50.0).abs() < 1e-3);
        let level = LevelGeom { polys: vec![quad(0.0, 0.0), quad(280.0, 280.0)], rooms: Vec::new() };
        assert_eq!(level.floor_below(50.0, 50.0, 500.0).unwrap().1, 1);
        assert_eq!(level.floor_below(50.0, 50.0, 100.0).unwrap().1, 0);
    }
}
