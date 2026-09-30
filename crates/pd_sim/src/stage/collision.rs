//! A [`LevelGeom`] as PD's collision system sees it: the `lib/collision.c`
//! primitives the chr and player movement code calls, ported onto the polygons.
//!
//! * wall tests: `cd_volume_collect` + `cd_volume_collect_tilei` (a cylinder against
//!   `wall` polygons, by y overlap then XZ edge distance), `cd_test_volume_simple`,
//!   `cd_test_volume_closestedge`, `cd_test_volume_fromdir`, and the swept tests
//!   `cd_test_cylmove_*` via `cd_is_cylpath_intersecting_tilei` (the centre line
//!   against tile edges);
//! * ground: `cd_find_ground_at_cyl_ctfril` + `cd_find_ground_finalise`, over the
//!   tiles and the props' floors ([`PropFloor`]);
//! * ladders and flagged tiles: `cd_find_ladder`, `is_cyl_touching_tile_with_flags`;
//! * other chrs as `GEOTYPE_CYL` perimeters (`chr_get_geometry`), passed per call.
//! * The ground search is `cd_find_ground_finalise` in full (`GEOFLAG_STEP`,
//!   `SLOPE`, `DIE`, the props' `GEOTYPE_TILE_F` floors and lifts), and ramp walls
//!   are tested (`cd_test_ramp_wall`, with `enableslopes`).
//! * Sight and shot lines against the props' blocks, cylinders and floors:
//!   [`TileLevel::cd_los_props`].
//!
//! Substitutions:
//! * `// SUBST:` PD only tests the tiles of the rooms a chr is in or passes
//!   through. Here every polygon is a candidate, rejected by bounding box first,
//!   which is a superset.
//! * `// SUBST:` the `oobfail` checks ("did the move leave every room?") become
//!   [`TileLevel::in_bounds`]: is there any floor within the cylinder below the point.
//! * A PD stage's rooms neighbour across its portals ([`TileLevel::for_stage`]).
//!   `// SUBST:` a test fixture has no portals: its rooms neighbour when their
//!   polygon edges run along each other ([`infer_room_neighbours`]).
//! * `// SUBST:` sight and shot rays test the fan triangles of each polygon (what
//!   `cd_is_line_intersecting_tilei` does) with a standard segment/triangle test in
//!   place of `func0002f490`.
//!
//! Units: PD world units (cm). `ymax`/`ymin` arguments are **relative to the
//! position** they're tested at, exactly as PD passes `ymax - prop->pos.y`.
//!
//! Source: the old repo's `pd_spike/tile_level.rs`.

use glam::{Vec2, Vec3};
use pd_core::ids::{FLOORTYPE_WATER, GEOFLAG_DIE, GEOFLAG_FLOOR1, GEOFLAG_FLOOR2, GEOFLAG_LIFTFLOOR, GEOFLAG_RAMPWALL, GEOFLAG_SLOPE, GEOFLAG_STEP, GEOFLAG_UNDERWATER};

thread_local! {
    /// `g_Vars.enableslopes`: false while a player standing on a
    /// `GEOFLAG_SLOPE` floor tests its own moves (`bwalk_try_move_upwards`,
    /// `bwalk_try_delta_nopush`), which lets it through the ramp walls
    /// (`cd_test_ramp_wall`). `// SUBST:` PD's global / one per thread, so
    /// worlds on other threads (the tests) don't share it; PD starts a stage
    /// with it false until the first player move test, here true.
    static ENABLESLOPES: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Sets `g_Vars.enableslopes` for a player's move test; dropping it sets it
/// back to true, as each of those functions does on its way out.
pub struct EnableSlopes;

impl EnableSlopes {
    pub fn set(enable: bool) -> EnableSlopes {
        ENABLESLOPES.with(|e| e.set(enable));
        EnableSlopes
    }
}

impl Drop for EnableSlopes {
    fn drop(&mut self) {
        ENABLESLOPES.with(|e| e.set(true));
    }
}

use super::geom::{GeomPoly, LevelGeom};

/// `CDRESULT_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdResult {
    NoCollision,
    Collision,
    Error,
}

/// A prop's collision geometry (`prop_get_geometry`): a chr's perimeter
/// (`GEOTYPE_CYL`, `chr_get_geometry`, `chr.c:4949`: a vertical cylinder from
/// `manground` to `manground + height`) or an object's block (`GEOTYPE_BLOCK`,
/// `obj_update_core_geo`: a convex footprint, from `ymin` to `ymax`). A block
/// meets any wall, sight or shot query (`cd_volume_collect_from_bytes`,
/// `collision.c:1246`); nothing stands on either.
#[derive(Clone, Copy, Debug, Default)]
pub struct PropGeo {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
    pub ymin: f32,
    pub ymax: f32,
    /// `GEOTYPE_BLOCK`: the footprint is the first `numvertices` of
    /// `vertices` (x, z); 0 for a cylinder.
    pub numvertices: u8,
    pub vertices: [Vec2; 8],
}

impl PropGeo {
    /// A `GEOTYPE_CYL`.
    pub fn cyl(x: f32, z: f32, radius: f32, ymin: f32, ymax: f32) -> PropGeo {
        PropGeo { x, z, radius, ymin, ymax, ..Default::default() }
    }

    /// A `GEOTYPE_BLOCK` (at most 8 vertices, as `struct geoblock`).
    pub fn block(verts: &[Vec2], ymin: f32, ymax: f32) -> PropGeo {
        let mut g = PropGeo { ymin, ymax, numvertices: verts.len().min(8) as u8, ..Default::default() };
        let n = g.numvertices as usize;
        g.vertices[..n].copy_from_slice(&verts[..n]);
        g
    }

    pub fn is_block(&self) -> bool {
        self.numvertices > 0
    }

    pub fn verts(&self) -> &[Vec2] {
        &self.vertices[..self.numvertices as usize]
    }

    /// Edge `i -> i + 1` of a block at height `y`.
    fn edge(&self, i: usize, y: f32) -> Edge {
        let v = self.verts();
        let (a, b) = (v[i], v[(i + 1) % v.len()]);
        (Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y))
    }

    /// The vertical overlap test every prop geometry gets.
    fn overlaps(&self, y: f32, checkvertical: bool, ymax: f32, ymin: f32) -> bool {
        !checkvertical || (y + ymax >= self.ymin && y + ymin <= self.ymax)
    }

    /// `cd_block_collides_with_cyl_laterally` (`collision.c:1118`) or
    /// `cd_cyl_collides_with_cyl_laterally` (`:1163`): the edge index hit.
    fn collides_laterally(&self, x: f32, z: f32, radius: f32) -> Option<usize> {
        if !self.is_block() {
            let (sx, sz, w) = (x - self.x, z - self.z, self.radius + radius);
            return (sx * sx + sz * sz <= w * w).then_some(0);
        }
        if cd_is_xz_in_block(self.verts(), x, z) {
            return Some(0);
        }
        let v = self.verts();
        for i in 0..v.len() {
            let next = (i + 1) % v.len();
            let value = cd_pos_get_dist_to_line(v[i].x, v[i].y, v[next].x, v[next].y, x, z).abs();
            if value <= radius
                && (cd_pos_get_dist_to_vtx(v[i].x, v[i].y, x, z) <= radius
                    || cd_pos_get_dist_to_vtx(v[next].x, v[next].y, x, z) <= radius
                    || cd_pos_get_side(v[i].x, v[i].y, v[next].x, v[next].y, x, z))
            {
                return Some(i);
            }
        }
        None
    }
}

/// `cd_is_xz_in_block` (`collision.c:759`): on one side of every edge.
pub fn cd_is_xz_in_block(v: &[Vec2], x: f32, z: f32) -> bool {
    let mut result: i32 = -1;
    for i in 0..v.len() {
        let next = (i + 1) % v.len();
        let value = (v[next].y - v[i].y) * (x - v[i].x) - (v[next].x - v[i].x) * (z - v[i].y);
        if value != 0.0 {
            if i == 0 || result < 0 {
                result = (value > 0.0) as i32;
            } else {
                if result != 0 && value < 0.0 {
                    return false;
                }
                if result == 0 && value > 0.0 {
                    return false;
                }
            }
        }
    }
    result >= 0
}

/// A prop's floor (`GEOTYPE_TILE_F`): a lift's floor or an object's floor
/// quad, in world space, with its `GEOFLAG_*`.
#[derive(Clone, Copy, Debug)]
pub struct PropFloor {
    pub verts: [Vec3; 4],
    pub flags: u32,
    /// The object's id (`collision->prop`).
    pub prop: u32,
    /// The room the prop is in (`collision->room`).
    pub room: Option<u16>,
}

impl PropFloor {
    /// The tile's box (`tile->min[]`, `tile->max[]`).
    fn bounds(&self) -> (Vec3, Vec3) {
        let lo = self.verts.iter().copied().fold(Vec3::splat(f32::MAX), Vec3::min);
        let hi = self.verts.iter().copied().fold(Vec3::splat(f32::MIN), Vec3::max);
        (lo, hi)
    }

    fn in_range(&self, pos: Vec3, radius: f32) -> bool {
        let (lo, hi) = self.bounds();
        pos.x >= lo.x - radius && pos.x <= hi.x + radius && pos.z >= lo.z - radius && pos.z <= hi.z + radius
    }

    /// `cd_is_xz_in_tilef` (`collision.c:725`).
    pub fn xz_in(&self, x: f32, z: f32) -> bool {
        let v: Vec<Vec2> = self.verts.iter().map(|p| Vec2::new(p.x, p.z)).collect();
        cd_is_xz_in_block(&v, x, z)
    }

    /// `cd_volume_collect_tilef` (`collision.c:1081`).
    fn collect(&self, x: f32, z: f32, radius: f32) -> bool {
        if self.xz_in(x, z) {
            return true;
        }
        let v = &self.verts;
        (0..4).any(|i| {
            let next = (i + 1) % 4;
            let value = cd_pos_get_dist_to_line(v[i].x, v[i].z, v[next].x, v[next].z, x, z).abs();
            value <= radius
                && (cd_pos_get_dist_to_vtx(v[i].x, v[i].z, x, z) <= radius
                    || cd_pos_get_dist_to_vtx(v[next].x, v[next].z, x, z) <= radius
                    || cd_pos_get_side(v[i].x, v[i].z, v[next].x, v[next].z, x, z))
        })
    }

    /// `cd_find_y_tilef` (`collision.c:652`): the plane of the first three
    /// vertices at (x, z), held within the tile's height range.
    pub fn find_y(&self, x: f32, z: f32) -> f32 {
        let v = &self.verts;
        let a = v[1] - v[0];
        let b = v[2] - v[0];
        let n = Vec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
        let tmp = n.x * v[0].x + n.y * v[0].y + n.z * v[0].z;
        let (lo, hi) = self.bounds();
        if n.y == 0.0 {
            return hi.y;
        }
        let ground = ((tmp as f64 - x as f64 * n.x as f64 - z as f64 * n.z as f64) / n.y as f64) as f32;
        ground.clamp(lo.y, hi.y)
    }
}

/// What a sight or shot line met among the props ([`TileLevel::cd_los_props`]):
/// an index into the geometry or the floors passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LosProp {
    Geo(usize),
    Floor(usize),
}

/// Where the ground search found the ground.
#[derive(Clone, Copy, Debug)]
enum GroundSrc {
    Tile(usize),
    Floor(usize),
}

/// The ground under a cylinder (`cd_find_ground_at_cyl_ctfril`'s outputs).
#[derive(Clone, Copy, Debug)]
pub struct Ground {
    pub y: f32,
    /// A BG tile's polygon.
    pub poly: Option<usize>,
    /// The index into the prop floors passed, and that floor's prop.
    pub floor: Option<usize>,
    pub prop: Option<u32>,
    /// The floor's `GEOFLAG_*` (`floorflags`; 0 when nothing was found, where
    /// PD leaves the caller's value).
    pub flags: u32,
    /// `floorroom`: the tile's room, or the prop's.
    pub room: Option<u16>,
    /// `floortype`: the tile's (water under `GEOFLAG_UNDERWATER`); a prop's
    /// floor is `FLOORTYPE_DEFAULT`; 0xff with no floor.
    pub floortype: u8,
}

impl Default for Ground {
    fn default() -> Ground {
        Ground { y: NO_GROUND, poly: None, floor: None, prop: None, flags: 0, room: None, floortype: 0xff }
    }
}

impl Ground {
    /// `inlift`, `lift`: the ground is a lift's floor (`GEOTYPE_TILE_F` with
    /// `GEOFLAG_LIFTFLOOR`, `collision.c:2255`), and that lift.
    pub fn lift(&self) -> Option<u32> {
        self.prop.filter(|_| self.flags & GEOFLAG_LIFTFLOOR != 0)
    }
}

/// What a sight/shot ray met first.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub dist: f32,
    pub point: Vec3,
    pub poly: usize,
}

/// The obstacle edge a failed move reports (`cd_get_edge`).
pub type Edge = (Vec3, Vec3);

/// What PD's collision globals hold after a failed test: the obstacle's edge
/// (`cd_get_edge`), the fraction of the move that fits (`cd_get_distance`, when
/// `cd_has_distance`), and which perimeter it was (`cd_get_obstacle_prop`; `None`
/// for the background).
#[derive(Clone, Copy, Debug)]
pub struct CdObstacle {
    pub edge: Edge,
    pub dist: Option<f32>,
    pub cyl: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
enum Hit {
    Tile { poly: usize, vertexindex: usize },
    Prop { k: usize, vertexindex: usize },
}

/// "No ground" as PD returns it from `cd_find_ground_finalise`.
pub const NO_GROUND: f32 = -4_294_967_296.0;

pub struct TileLevel {
    pub geom: LevelGeom,
    bbox: Vec<(Vec3, Vec3)>,
    walls: Vec<usize>,
    floors: Vec<usize>,
    sight: Vec<usize>,
    shot: Vec<usize>,
    any_blocker: Vec<usize>,
    ladders: Vec<usize>,
    crouch: Vec<usize>,
    duck: Vec<usize>,
    /// Rooms that share a polygon edge (`bg_room_get_neighbours`, see [`Self::new`]).
    room_neighbours: std::collections::BTreeMap<u16, Vec<u16>>,
    /// Each room's box over its tiles (`g_Rooms[].bbmin/bbmax`, see [`Self::room_bbox`]).
    room_bboxes: std::collections::BTreeMap<u16, (Vec3, Vec3)>,
}

/// Which flagged tiles [`TileLevel::is_cyl_touching_tile_with_flags`] looks at.
#[derive(Clone, Copy, Debug)]
pub enum TileFlag {
    Crouch,
    Duck,
}

impl TileLevel {
    /// Collision over `geom`, the rooms' neighbours inferred from where their
    /// tiles' edges meet ([`infer_room_neighbours`]): a level without portals
    /// (a fixture, a test level). A PD stage's come from its portals
    /// ([`TileLevel::for_stage`]).
    pub fn new(geom: LevelGeom) -> Self {
        let mut l = TileLevel::without_room_neighbours(geom);
        l.room_neighbours = infer_room_neighbours(&l.geom);
        l
    }

    /// Collision over `geom` with no rooms' neighbours, for a caller that
    /// supplies them or never asks for them. Inferring them compares every
    /// tile edge with every other, the one cost here that grows with the
    /// square of the tiles (over a second on GoldenEye's Facility).
    pub(crate) fn without_room_neighbours(geom: LevelGeom) -> Self {
        let bbox = geom
            .polys
            .iter()
            .map(|p| {
                let lo = p.verts.iter().copied().fold(Vec3::splat(f32::INFINITY), Vec3::min);
                let hi = p.verts.iter().copied().fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
                (lo, hi)
            })
            .collect();
        let pick = |f: &dyn Fn(&GeomPoly) -> bool| {
            geom.polys.iter().enumerate().filter(|(_, p)| f(p)).map(|(i, _)| i).collect::<Vec<_>>()
        };
        let walls = pick(&|p| p.wall);
        let floors = pick(&|p| p.floor);
        let sight = pick(&|p| p.blocks_sight);
        let shot = pick(&|p| p.blocks_shot);
        let any_blocker = pick(&|p| p.wall || p.blocks_sight || p.blocks_shot);
        let ladders = pick(&|p| p.ladder_flags() != 0);
        let crouch = pick(&|p| p.crouch);
        let duck = pick(&|p| p.duck);
        let room_neighbours = Default::default();
        let mut room_bboxes: std::collections::BTreeMap<u16, (Vec3, Vec3)> = Default::default();
        for (p, &(lo, hi)) in geom.polys.iter().zip(&bbox) {
            if let Some(r) = p.room {
                let e = room_bboxes.entry(r).or_insert((lo, hi));
                *e = (e.0.min(lo), e.1.max(hi));
            }
        }
        TileLevel { geom, bbox, walls, floors, sight, shot, any_blocker, ladders, crouch, duck, room_neighbours, room_bboxes }
    }

    /// A PD stage's collision: its tiles, with the rooms' neighbours from the
    /// BG's portals (`bg_room_get_neighbours`, `bg.c:5869`) and their boxes
    /// from `g_Rooms[].bbmin/bbmax`, in place of what [`TileLevel::new`] infers
    /// from the tiles.
    pub fn for_stage(stage: &super::Stage) -> Self {
        let rooms = &stage.rooms;
        if rooms.portals.is_empty() {
            return TileLevel::new(stage.geom.clone()); // a fixture
        }
        let mut l = TileLevel::without_room_neighbours(stage.geom.clone());
        l.room_neighbours = (1..rooms.roomcount()).map(|r| (r as u16, rooms.bg_room_get_neighbours(r, usize::MAX))).collect();
        l.room_bboxes = (1..rooms.roomcount()).map(|r| (r as u16, (rooms.rooms[r].bbmin, rooms.rooms[r].bbmax))).collect();
        l
    }

    // ─── Volume tests ────────────────────────────────────────────────────────

    /// `cd_volume_collect_from_bytes` (`collision.c:1186`) for tiles: bounding box
    /// grown by the radius, then the y overlap when `checkvertical`.
    fn tile_in_range(&self, poly: usize, pos: Vec3, radius: f32, checkvertical: bool, ymax: f32, ymin: f32) -> bool {
        let (lo, hi) = self.bbox[poly];
        pos.x >= lo.x - radius
            && pos.x <= hi.x + radius
            && pos.z >= lo.z - radius
            && pos.z <= hi.z + radius
            && (!checkvertical || (pos.y + ymax >= lo.y && pos.y + ymin <= hi.y))
    }

    /// `cd_volume_collect_tilei` (`collision.c:1044`): the cylinder's circle meets
    /// the tile's XZ projection — the centre inside it, or within `radius` of an
    /// edge (near an end, or with the perpendicular foot inside the edge).
    fn volume_collect_tile(&self, poly: usize, x: f32, z: f32, radius: f32) -> Option<usize> {
        let p = &self.geom.polys[poly];
        if p.xz_in_convex(x, z) {
            return Some(0);
        }
        let v = &p.verts;
        for i in 0..v.len() {
            let next = (i + 1) % v.len();
            let value = cd_pos_get_dist_to_line(v[i].x, v[i].z, v[next].x, v[next].z, x, z).abs();
            if value <= radius
                && (cd_pos_get_dist_to_vtx(v[i].x, v[i].z, x, z) <= radius
                    || cd_pos_get_dist_to_vtx(v[next].x, v[next].z, x, z) <= radius
                    || cd_pos_get_side(v[i].x, v[i].z, v[next].x, v[next].z, x, z))
            {
                return Some(i);
            }
        }
        None
    }

    /// `cd_test_ramp_wall` (`collision.c:1382`): for a triangular wall tile under
    /// a ramp (`GEOFLAG_RAMPWALL`), does the height `y1..y2` at `pos` actually
    /// meet it? Each edge's height where it passes `pos` (along x or z,
    /// whichever the edge runs further in) counts when it lies within the range,
    /// or when edges above and below it have both been seen. A slope's ramp
    /// walls count for nothing while `enableslopes` is off. Non-zero: a hit.
    pub fn cd_test_ramp_wall(&self, poly: usize, pos: Vec3, y1: f32, y2: f32) -> i32 {
        let p = &self.geom.polys[poly];
        if !ENABLESLOPES.with(|e| e.get()) && p.geoflags & GEOFLAG_SLOPE != 0 {
            return 0;
        }
        let v = &p.verts;
        let (mut count, mut y1count, mut y2count) = (0, 0, 0);
        for i in 0..v.len() {
            let next = (i + 1) % v.len();
            let xdiff = (v[next].x - v[i].x).abs();
            let zdiff = (v[next].z - v[i].z).abs();
            if xdiff != 0.0 || zdiff != 0.0 {
                let (this0, next0, posval) = if zdiff < xdiff { (v[i].x, v[next].x, pos.x) } else { (v[i].z, v[next].z, pos.z) };
                let somefloat = (posval - this0) / (next0 - this0);
                if (0.0..=1.0).contains(&somefloat) {
                    let somefloat2 = v[i].y + (v[next].y - v[i].y) * somefloat;
                    if somefloat2 >= y2 - 1.0 {
                        y2count += 1;
                        if y1count != 0 {
                            count += 1;
                        }
                    } else if somefloat2 <= y1 + 1.0 {
                        y1count += 1;
                        if y2count != 0 {
                            count += 1;
                        }
                    } else {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    /// A wall tile passes `cd_test_ramp_wall` if it isn't a ramp wall.
    fn ramp_wall_ok(&self, poly: usize, pos: Vec3, y1: f32, y2: f32) -> bool {
        self.geom.polys[poly].geoflags & GEOFLAG_RAMPWALL == 0 || self.cd_test_ramp_wall(poly, pos, y1, y2) != 0
    }

    /// `cd_volume_collect(..., GEOFLAG_WALL, ..., maxcollisions = 1)`: the first
    /// wall tile, else the first chr perimeter, the cylinder at `pos` touches. PD
    /// checks the background before props, so the order is the same.
    fn volume_collect_wall(&self, pos: Vec3, radius: f32, checkvertical: bool, ymax: f32, ymin: f32, cyls: &[PropGeo]) -> Option<Hit> {
        for &poly in &self.walls {
            if self.tile_in_range(poly, pos, radius, checkvertical, ymax, ymin) && self.ramp_wall_ok(poly, pos, pos.y + ymin, pos.y + ymax) {
                if let Some(vertexindex) = self.volume_collect_tile(poly, pos.x, pos.z, radius) {
                    return Some(Hit::Tile { poly, vertexindex });
                }
            }
        }
        for (k, c) in cyls.iter().enumerate() {
            if c.overlaps(pos.y, checkvertical, ymax, ymin) {
                if let Some(vertexindex) = c.collides_laterally(pos.x, pos.z, radius) {
                    return Some(Hit::Prop { k, vertexindex });
                }
            }
        }
        None
    }

    /// Diagnostic: every wall polygon a cylinder at `pos` touches, with the edge
    /// (`vertexindex`) it touches — what `volume_collect_wall` stops at the first of.
    pub fn walls_touching(&self, pos: Vec3, radius: f32, ymax: f32, ymin: f32) -> Vec<(usize, usize)> {
        self.walls
            .iter()
            .filter(|&&p| self.tile_in_range(p, pos, radius, true, ymax, ymin))
            .filter_map(|&p| self.volume_collect_tile(p, pos.x, pos.z, radius).map(|v| (p, v)))
            .collect()
    }

    /// The first tile of `list` a cylinder at `pos` touches (`cd_volume_collect`
    /// over the background with `CHECKVERTICAL_YES` and one slot).
    fn first_touching(&self, list: &[usize], pos: Vec3, radius: f32, ymax: f32, ymin: f32) -> Option<usize> {
        list.iter()
            .copied()
            .find(|&p| self.tile_in_range(p, pos, radius, true, ymax, ymin) && self.volume_collect_tile(p, pos.x, pos.z, radius).is_some())
    }

    /// `is_cyl_touching_tile_with_flags` (`collision.c:2191`).
    pub fn is_cyl_touching_tile_with_flags(&self, flag: TileFlag, pos: Vec3, radius: f32, ymax: f32, ymin: f32) -> bool {
        let list = match flag {
            TileFlag::Crouch => &self.crouch,
            TileFlag::Duck => &self.duck,
        };
        self.first_touching(list, pos, radius, ymax, ymin).is_some()
    }

    /// `cd_find_ladder` (`collision.c:2163`): the first tile with any of
    /// `geoflags` (a chr asks for `GEOFLAG_LADDER`, `chr.c:622`; a player also
    /// for `GEOFLAG_LADDER_PLAYERONLY`, `bondwalk.c:792`) a cylinder of `width`
    /// at `pos` touches, and its normal turned to face `pos`.
    pub fn cd_find_ladder(&self, pos: Vec3, width: f32, ymax: f32, ymin: f32, geoflags: u32) -> Option<Vec3> {
        let p = self
            .ladders
            .iter()
            .copied()
            .filter(|&p| self.geom.polys[p].ladder_flags() & geoflags != 0)
            .find(|&p| self.tile_in_range(p, pos, width, true, ymax, ymin) && self.volume_collect_tile(p, pos.x, pos.z, width).is_some())?;
        let poly = &self.geom.polys[p];
        let n = poly.normal;
        Some(if (pos - poly.verts[0]).dot(n) < 0.0 { -n } else { n })
    }

    /// `cd_test_volume_simple` (`collision.c:2428`).
    pub fn cd_test_volume_simple(&self, pos: Vec3, radius: f32, checkvertical: bool, ymax: f32, ymin: f32, cyls: &[PropGeo]) -> CdResult {
        match self.volume_collect_wall(pos, radius, checkvertical, ymax, ymin, cyls) {
            Some(_) => CdResult::Collision,
            None => CdResult::NoCollision,
        }
    }

    /// `cd_test_volume_simple` with `CDTYPE_ALL & ~CDTYPE_BG`: the other chrs'
    /// perimeters only (`chr_adjust_pos_for_spawn` with `force`).
    pub fn cd_test_volume_props(&self, pos: Vec3, radius: f32, checkvertical: bool, ymax: f32, ymin: f32, cyls: &[PropGeo]) -> CdResult {
        for c in cyls {
            if c.overlaps(pos.y, checkvertical, ymax, ymin) && c.collides_laterally(pos.x, pos.z, radius).is_some() {
                return CdResult::Collision;
            }
        }
        CdResult::NoCollision
    }

    /// `cd_test_volume_closestedge` (`collision.c:2459`): the volume test at `topos`,
    /// returning the edge that was hit (a tile's `vertexindex → next`, or for a
    /// cylinder the tangent edge facing `frompos`).
    pub fn cd_test_volume_closestedge(
        &self,
        frompos: Vec3,
        topos: Vec3,
        radius: f32,
        ymax: f32,
        ymin: f32,
        cyls: &[PropGeo],
    ) -> (CdResult, Option<Edge>) {
        match self.volume_collect_wall(topos, radius, true, ymax, ymin, cyls) {
            None => (CdResult::NoCollision, None),
            Some(Hit::Tile { poly, vertexindex }) => {
                let v = &self.geom.polys[poly].verts;
                (CdResult::Collision, Some((v[vertexindex], v[(vertexindex + 1) % v.len()])))
            }
            Some(Hit::Prop { k, vertexindex }) => {
                let c = &cyls[k];
                if c.is_block() {
                    return (CdResult::Collision, Some(c.edge(vertexindex, frompos.y)));
                }
                let (a, b) = cd_pos_get_cyl_edge(c.x, c.z, c.radius, frompos.x, frompos.z);
                (CdResult::Collision, Some((Vec3::new(a.x, frompos.y, a.y), Vec3::new(b.x, frompos.y, b.y))))
            }
        }
    }

    /// `cd_test_atobany` / `cd_test_atobclosest` with `ATOBTYPE_LOS` over props
    /// (`collision.c:3020`, `CHECKVERTICAL_YES` with no height offsets): how far
    /// along `from → to` (0..1) the nearest prop geometry is, and which: a block
    /// or cylinder in `geos` (any sight/shot flag meets a block), or one of the
    /// `floors` carrying one of `geoflags` (`cd_is_line_intersecting_tilef`).
    /// `// SUBST:` the floor quad is tested as its two triangles with a standard
    /// segment/triangle test, as the BG's tiles are.
    pub fn cd_los_props(from: Vec3, to: Vec3, geos: &[PropGeo], floors: &[PropFloor], geoflags: u32) -> Option<(f32, LosProp)> {
        let len = (to - from).length();
        let mut best: Option<(f32, LosProp)> = None;
        let mut take = |t: f32, what: LosProp| {
            if best.is_none_or(|(b, _)| t < b) {
                best = Some((t, what));
            }
        };
        for (k, g) in geos.iter().enumerate() {
            let hit = if g.is_block() { Self::cylpath_block(g, from, to, true, 0.0, 0.0) } else { Self::cylpath_cyl(g, from, to, true, 0.0, 0.0) };
            if let Some((end, _)) = hit {
                take(if len > 0.0 { (end - from).length() / len } else { 0.0 }, LosProp::Geo(k));
            }
        }
        for (k, f) in floors.iter().enumerate() {
            if f.flags & geoflags == 0 {
                continue;
            }
            let v = &f.verts;
            let d = to - from;
            if let Some(t) = segment_triangle(from, d, v[0], v[1], v[2]).or_else(|| segment_triangle(from, d, v[0], v[2], v[3])) {
                take(t, LosProp::Floor(k));
            }
        }
        best
    }

    // ─── Swept tests ─────────────────────────────────────────────────────────

    /// `cd_is_cylpath_intersecting_tilei` (`collision.c:2589`): does the centre line
    /// `frompos → topos` cross one of the tile's XZ edges while the swept cylinder
    /// overlaps the tile in y? Returns the crossing point and the edge.
    fn cylpath_tile(&self, poly: usize, frompos: Vec3, topos: Vec3, checkvertical: bool, ymax: f32, ymin: f32) -> Option<(Vec3, Edge)> {
        let p = &self.geom.polys[poly];
        let v = &p.verts;
        let (tileymin, tileymax) = (self.bbox[poly].0.y, self.bbox[poly].1.y);
        let vertical_ok = !checkvertical
            || (frompos.y + ymax >= tileymin && topos.y + ymin <= tileymax)
            || (frompos.y + ymin <= tileymax && topos.y + ymax >= tileymin);
        if !vertical_ok {
            return None;
        }
        let mut first = true;
        let mut best: Option<(f32, usize)> = None;
        let mut bestdistfrac = 1.0f32;
        for i in 0..v.len() {
            let next = (i + 1) % v.len();
            if cd_000254d8(frompos, topos, v[i].x, v[i].z, v[next].x, v[next].z, &mut first) {
                let distfrac = func0f1577f0(
                    Vec2::new(frompos.x, frompos.z),
                    Vec2::new(topos.x, topos.z),
                    Vec2::new(v[i].x, v[i].z),
                    Vec2::new(v[next].x, v[next].z),
                );
                if distfrac < bestdistfrac {
                    let y1 = frompos.y + (topos.y - frompos.y) * distfrac;
                    let (y2, y1) = (y1 + ymax, y1 + ymin);
                    if !checkvertical || !(y1 >= tileymax || y2 <= tileymin) {
                        bestdistfrac = distfrac;
                        best = Some((distfrac, i));
                    }
                }
            }
        }
        if let Some((frac, i)) = best {
            let end = frompos + (topos - frompos) * frac;
            let a = Vec3::new(v[i].x, end.y, v[i].z);
            let b = Vec3::new(v[(i + 1) % v.len()].x, end.y, v[(i + 1) % v.len()].z);
            return Some((end, (a, b)));
        }
        if first {
            // Started inside the tile's outline.
            return Some((frompos, (frompos, frompos)));
        }
        None
    }

    /// `cd_is_cylpath_intersecting_block` (`collision.c:2769`): the centre line
    /// crossing one of the block's edges while the cylinder overlaps it in y, or
    /// starting inside it.
    fn cylpath_block(c: &PropGeo, frompos: Vec3, topos: Vec3, checkvertical: bool, ymax: f32, ymin: f32) -> Option<(Vec3, Edge)> {
        if !(!checkvertical || (frompos.y + ymax >= c.ymin && topos.y + ymin <= c.ymax) || (frompos.y + ymin <= c.ymax && topos.y + ymax >= c.ymin)) {
            return None;
        }
        let v = c.verts();
        let mut spb8 = true;
        let mut best: Option<(f32, usize)> = None;
        let mut bestdistfrac = 1.0f32;
        for i in 0..v.len() {
            let next = (i + 1) % v.len();
            if cd_000254d8(frompos, topos, v[i].x, v[i].y, v[next].x, v[next].y, &mut spb8) {
                let distfrac = func0f1577f0(Vec2::new(frompos.x, frompos.z), Vec2::new(topos.x, topos.z), v[i], v[next]);
                if distfrac < bestdistfrac {
                    let y1 = (topos.y - frompos.y) * distfrac + frompos.y;
                    let (y2, y1) = (y1 + ymax, y1 + ymin);
                    if !checkvertical || (!(y1 >= c.ymax) && !(y2 <= c.ymin)) {
                        bestdistfrac = distfrac;
                        best = Some((distfrac, i));
                    }
                }
            }
        }
        if let Some((frac, i)) = best {
            let end = frompos + (topos - frompos) * frac;
            return Some((end, c.edge(i, end.y)));
        }
        if spb8 {
            return Some((frompos, (frompos, frompos)));
        }
        None
    }

    /// `cd_is_cylpath_intersecting_cyl` (`collision.c:2857`).
    fn cylpath_cyl(c: &PropGeo, frompos: Vec3, topos: Vec3, checkvertical: bool, ymax: f32, ymin: f32) -> Option<(Vec3, Edge)> {
        if c.is_block() {
            return Self::cylpath_block(c, frompos, topos, checkvertical, ymax, ymin);
        }
        let vertical_ok = !checkvertical
            || (frompos.y + ymax >= c.ymin && topos.y + ymin <= c.ymax)
            || (frompos.y + ymin <= c.ymax && topos.y + ymax >= c.ymin);
        if !vertical_ok {
            return None;
        }
        let sp74 = cd_pos_get_dist_to_line(frompos.x, frompos.z, topos.x, topos.z, c.x, c.z).abs();
        if !(sp74 < c.radius
            && (cd_pos_get_dist_to_vtx(frompos.x, frompos.z, c.x, c.z) < c.radius
                || cd_pos_get_dist_to_vtx(topos.x, topos.z, c.x, c.z) < c.radius
                || cd_pos_get_side(frompos.x, frompos.z, topos.x, topos.z, c.x, c.z)))
        {
            return None;
        }
        let len = Vec2::new(topos.x - frompos.x, topos.z - frompos.z).length();
        let mult = if len > 0.0 {
            let sq = Vec2::new(c.x - frompos.x, c.z - frompos.z).length_squared();
            let distance =
                if sp74 * sp74 <= sq { (sq - sp74 * sp74).sqrt() - (c.radius * c.radius - sp74 * sp74).sqrt() } else { 0.0 };
            distance / len
        } else {
            0.0
        };
        if mult >= 1.0 {
            return None;
        }
        let y = (topos.y - frompos.y) * mult + frompos.y;
        if checkvertical && (y + ymin >= c.ymax || y + ymax <= c.ymin) {
            return None;
        }
        let end = frompos + (topos - frompos) * mult;
        let (a, b) = cd_pos_get_cyl_edge(c.x, c.z, c.radius, frompos.x, frompos.z);
        Some((end, (Vec3::new(a.x, end.y, a.y), Vec3::new(b.x, end.y, b.y))))
    }

    /// `cd_test_atobany` / `cd_test_atobclosest` with `ATOBTYPE_CYL` and
    /// `GEOFLAG_WALL`: the swept centre line against wall tiles then perimeters.
    /// `closest` keeps the nearest crossing (the `findclosest` variants) instead of
    /// the first; either way the edge of the reported crossing comes back.
    fn atob_cyl(&self, frompos: Vec3, topos: Vec3, ymax: f32, ymin: f32, cyls: &[PropGeo], closest: bool) -> Option<Edge> {
        self.atob_cyl_obstacle(frompos, topos, true, ymax, ymin, cyls, closest).map(|(e, _)| e)
    }

    /// [`Self::atob_cyl`], also naming the perimeter that was hit (PD's
    /// `cd_get_obstacle_prop`) - `None` for a wall tile.
    #[allow(clippy::too_many_arguments)]
    fn atob_cyl_obstacle(
        &self,
        frompos: Vec3,
        topos: Vec3,
        checkvertical: bool,
        ymax: f32,
        ymin: f32,
        cyls: &[PropGeo],
        closest: bool,
    ) -> Option<(Edge, Option<usize>)> {
        let mut best: Option<(f32, Edge, Option<usize>)> = None;
        let mut consider = |end: Vec3, edge: Edge, cyl: Option<usize>| -> bool {
            let sq = (end - frompos).length_squared();
            if best.is_none_or(|(b, _, _)| sq < b) {
                best = Some((sq, edge, cyl));
            }
            !closest
        };
        for &poly in &self.walls {
            // cd_test_atobclosest_from_bytes (collision.c:3178) tests a ramp wall
            // at `frompos` first (cd_test_atobany_from_bytes doesn't).
            if closest && !self.ramp_wall_ok(poly, frompos, frompos.y + ymin, frompos.y + ymax) {
                continue;
            }
            // `cd_test_atobclosest_from_bytes`: skip tiles both ends lie beyond.
            let (lo, hi) = self.bbox[poly];
            if (frompos.x < lo.x && topos.x < lo.x)
                || (frompos.x > hi.x && topos.x > hi.x)
                || (frompos.z < lo.z && topos.z < lo.z)
                || (frompos.z > hi.z && topos.z > hi.z)
            {
                continue;
            }
            if let Some((end, edge)) = self.cylpath_tile(poly, frompos, topos, checkvertical, ymax, ymin) {
                if consider(end, edge, None) {
                    return best.map(|b| (b.1, b.2));
                }
            }
        }
        for (k, c) in cyls.iter().enumerate() {
            if let Some((end, edge)) = Self::cylpath_cyl(c, frompos, topos, checkvertical, ymax, ymin) {
                if consider(end, edge, Some(k)) {
                    return best.map(|b| (b.1, b.2));
                }
            }
        }
        best.map(|b| (b.1, b.2))
    }

    /// `cd_test_cylmove_oobok_findclosest` with the obstacle kept
    /// (`cd_get_obstacle_prop`): the index in `cyls` of the prop the swept line
    /// meets first, `None` when that is the BG or nothing.
    pub fn cd_test_cylmove_oobok_findclosest_obstacle(&self, frompos: Vec3, topos: Vec3, checkvertical: bool, ymax: f32, ymin: f32, cyls: &[PropGeo]) -> Option<usize> {
        self.atob_cyl_obstacle(frompos, topos, checkvertical, ymax, ymin, cyls, true).and_then(|(_, k)| k)
    }

    /// `cd_test_cylmove_oobok` (`collision.c:3576`): any wall on the swept line.
    pub fn cd_test_cylmove_oobok(&self, frompos: Vec3, topos: Vec3, ymax: f32, ymin: f32, cyls: &[PropGeo]) -> CdResult {
        match self.atob_cyl(frompos, topos, ymax, ymin, cyls, false) {
            Some(_) => CdResult::Collision,
            None => CdResult::NoCollision,
        }
    }

    /// `cd_test_cylmove_oobfail` (`collision.c:3586`): as `oobok`, but a
    /// destination outside the level counts as a collision.
    /// SUBST: "outside every room" → [`Self::in_bounds`].
    pub fn cd_test_cylmove_oobfail(
        &self,
        frompos: Vec3,
        topos: Vec3,
        radius: f32,
        ymax: f32,
        ymin: f32,
        cyls: &[PropGeo],
    ) -> CdResult {
        if !self.in_bounds(topos, radius) {
            return CdResult::Collision;
        }
        self.cd_test_cylmove_oobok(frompos, topos, ymax, ymin, cyls)
    }

    /// `cd_test_cylmove_oobfail_findclosest` (`collision.c:3622`): the nearest wall
    /// crossing and its edge; a clear path to a point outside the level is
    /// `CDRESULT_ERROR`. SUBST: "outside every room" → [`Self::in_bounds`].
    pub fn cd_test_cylmove_oobfail_findclosest(
        &self,
        frompos: Vec3,
        topos: Vec3,
        radius: f32,
        ymax: f32,
        ymin: f32,
        cyls: &[PropGeo],
    ) -> (CdResult, Option<Edge>) {
        match self.atob_cyl(frompos, topos, ymax, ymin, cyls, true) {
            Some(edge) => (CdResult::Collision, Some(edge)),
            None if !self.in_bounds(topos, radius) => (CdResult::Error, None),
            None => (CdResult::NoCollision, None),
        }
    }

    /// `cd_test_cylmove_oobfail_findclosest_finddist` (`collision.c:3640`): as
    /// [`Self::cd_test_cylmove_oobfail_findclosest`], and on a collision also the
    /// fraction of the move that fits before the cylinder meets the edge
    /// (`cd_set_obstacle_distance`, `collision.c:180`).
    pub fn cd_test_cylmove_oobfail_findclosest_finddist(
        &self,
        frompos: Vec3,
        topos: Vec3,
        radius: f32,
        ymax: f32,
        ymin: f32,
        cyls: &[PropGeo],
    ) -> (CdResult, Option<CdObstacle>) {
        match self.atob_cyl_obstacle(frompos, topos, true, ymax, ymin, cyls, true) {
            Some((edge, cyl)) => {
                let diff = Vec2::new(topos.x - frompos.x, topos.z - frompos.z);
                let dist = func0f1579cc(
                    Vec2::new(frompos.x, frompos.z),
                    radius,
                    Vec2::new(edge.0.x, edge.0.z),
                    Vec2::new(edge.1.x, edge.1.z),
                    diff,
                );
                (CdResult::Collision, Some(CdObstacle { edge, dist: Some(dist), cyl }))
            }
            None if !self.in_bounds(topos, radius) => (CdResult::Error, None),
            None => (CdResult::NoCollision, None),
        }
    }

    /// `cd_test_cylmove_oobok_findclosest` (`collision.c:3604`): the nearest wall
    /// or perimeter the centre line crosses, and its edge. Leaving the level is fine.
    pub fn cd_test_cylmove_oobok_findclosest(&self, frompos: Vec3, topos: Vec3, checkvertical: bool, ymax: f32, ymin: f32, cyls: &[PropGeo]) -> (CdResult, Option<Edge>) {
        match self.atob_cyl_obstacle(frompos, topos, checkvertical, ymax, ymin, cyls, true) {
            Some((edge, _)) => (CdResult::Collision, Some(edge)),
            None => (CdResult::NoCollision, None),
        }
    }

    /// `cd_test_cylmove_oobok_findclosest_getfinalroom_finddist` (`collision.c:3658`):
    /// as [`Self::cd_test_cylmove_oobok_findclosest`], with the fraction of the
    /// move a cylinder of `radius` makes before it meets the edge.
    #[allow(clippy::too_many_arguments)]
    pub fn cd_test_cylmove_oobok_findclosest_finddist(
        &self,
        frompos: Vec3,
        topos: Vec3,
        radius: f32,
        checkvertical: bool,
        ymax: f32,
        ymin: f32,
        cyls: &[PropGeo],
    ) -> (CdResult, Option<CdObstacle>) {
        match self.atob_cyl_obstacle(frompos, topos, checkvertical, ymax, ymin, cyls, true) {
            Some((edge, cyl)) => {
                let diff = Vec2::new(topos.x - frompos.x, topos.z - frompos.z);
                let dist = func0f1579cc(Vec2::new(frompos.x, frompos.z), radius, Vec2::new(edge.0.x, edge.0.z), Vec2::new(edge.1.x, edge.1.z), diff);
                (CdResult::Collision, Some(CdObstacle { edge, dist: Some(dist), cyl }))
            }
            None => (CdResult::NoCollision, None),
        }
    }

    /// `cd_test_volume_fromdir` (`collision.c:2536`): the volume test at `topos`
    /// collecting up to 20 touched wall edges and perimeters
    /// (`cd_volumefromdir_collect`, `:1614`), then the one the move from `frompos`
    /// meets first (`cd_volumefromdir_finalise`, `:1659`), with that fraction.
    ///
    /// Unlike `cd_volume_collect_tilei`, the fromdir collector only takes edges
    /// within `radius` (`cd_volumefromdir_collect_tilei`, `:1340`): a centre inside
    /// a wall tile's outline with no edge in reach touches nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn cd_test_volume_fromdir(
        &self,
        frompos: Vec3,
        topos: Vec3,
        radius: f32,
        checkvertical: bool,
        ymax: f32,
        ymin: f32,
        cyls: &[PropGeo],
    ) -> (CdResult, Option<CdObstacle>) {
        const MAX: usize = 20;
        // (edge, perimeter index, perimeter circle) per collision, in collection order.
        let mut collisions: Vec<(Edge, Option<usize>, Option<(f32, f32, f32)>)> = Vec::new();
        'bg: for &poly in &self.walls {
            if !self.tile_in_range(poly, topos, radius, checkvertical, ymax, ymin) || !self.ramp_wall_ok(poly, topos, topos.y + ymin, topos.y + ymax) {
                continue;
            }
            let v = &self.geom.polys[poly].verts;
            for i in 0..v.len() {
                let next = (i + 1) % v.len();
                if v[i].x == v[next].x && v[i].z == v[next].z {
                    continue;
                }
                let dist = cd_pos_get_dist_to_line(v[i].x, v[i].z, v[next].x, v[next].z, topos.x, topos.z).abs();
                if dist <= radius
                    && (cd_pos_get_dist_to_vtx(v[i].x, v[i].z, topos.x, topos.z) <= radius
                        || cd_pos_get_dist_to_vtx(v[next].x, v[next].z, topos.x, topos.z) <= radius
                        || cd_pos_get_side(v[i].x, v[i].z, v[next].x, v[next].z, topos.x, topos.z))
                {
                    if collisions.len() < MAX {
                        collisions.push(((v[i], v[next]), None, None));
                    } else {
                        break 'bg;
                    }
                }
            }
        }
        'props: for (k, c) in cyls.iter().enumerate() {
            if !c.overlaps(topos.y, checkvertical, ymax, ymin) {
                continue;
            }
            if c.is_block() {
                // cd_volumefromdir_collect_block (`collision.c:1498`): every edge in reach.
                let v = c.verts();
                for i in 0..v.len() {
                    let next = (i + 1) % v.len();
                    if v[i] == v[next] {
                        continue;
                    }
                    let dist = cd_pos_get_dist_to_line(v[i].x, v[i].y, v[next].x, v[next].y, topos.x, topos.z).abs();
                    if dist <= radius
                        && (cd_pos_get_dist_to_vtx(v[i].x, v[i].y, topos.x, topos.z) <= radius
                            || cd_pos_get_dist_to_vtx(v[next].x, v[next].y, topos.x, topos.z) <= radius
                            || cd_pos_get_side(v[i].x, v[i].y, v[next].x, v[next].y, topos.x, topos.z))
                    {
                        if collisions.len() < MAX {
                            collisions.push((c.edge(i, frompos.y), Some(k), None));
                        } else {
                            break 'props;
                        }
                    }
                }
                continue;
            }
            let (xd, zd, f16) = (topos.x - c.x, topos.z - c.z, radius + c.radius);
            if xd * xd + zd * zd <= f16 * f16 && collisions.len() < MAX {
                collisions.push(((Vec3::ZERO, Vec3::ZERO), Some(k), Some((c.x, c.z, c.radius))));
            }
        }
        if collisions.is_empty() {
            return (CdResult::NoCollision, None);
        }
        let from2 = Vec2::new(frompos.x, frompos.z);
        let diff = Vec2::new(topos.x - frompos.x, topos.z - frompos.z);
        let mut best: Option<(f32, usize)> = None;
        for (i, (edge, _, cyl)) in collisions.iter().enumerate() {
            let value = match cyl {
                // A perimeter: the swept radius grows by its radius, the "edge" is its centre.
                Some((x, z, r)) => func0f1579cc(from2, r + radius, Vec2::new(*x, *z), Vec2::new(*x, *z), diff),
                None => func0f1579cc(from2, radius, Vec2::new(edge.0.x, edge.0.z), Vec2::new(edge.1.x, edge.1.z), diff),
            };
            if best.is_none_or(|(b, _)| value < b) {
                best = Some((value, i));
            }
        }
        let (dist, i) = best.unwrap();
        let (edge, k, cyl) = collisions[i];
        let edge = match cyl {
            Some((x, z, r)) => {
                let (a, b) = cd_pos_get_cyl_edge(x, z, r, frompos.x, frompos.z);
                (Vec3::new(a.x, frompos.y, a.y), Vec3::new(b.x, frompos.y, b.y))
            }
            None => edge,
        };
        (CdResult::Collision, Some(CdObstacle { edge, dist: Some(dist), cyl: k }))
    }

    /// SUBST: PD asks whether a point is in any room's volume; here a point is inside the level when a
    /// floor lies within its cylinder somewhere below it. Rooms are volumes around
    /// their floors, so over a drop (a walkway edge) this is still "in bounds" and
    /// the chr falls, as in PD.
    pub fn in_bounds(&self, p: Vec3, radius: f32) -> bool {
        self.cd_find_ground_at_cyl(Vec3::new(p.x, p.y + 69.0, p.z), radius).0 > -100_000.0
    }

    /// Where a chr would stand over `p`: the floor PD's ground probe finds under
    /// a 20 cm cylinder from 10 cm above `p` (pads sit 50–120 cm above their
    /// floor), or `p` itself if there is none. A harness and debug helper; PD's
    /// spawns find their ground themselves (`player_start_new_life`).
    pub fn drop_to_ground(&self, p: Vec3) -> Vec3 {
        let (g, _) = self.cd_find_ground_at_cyl(Vec3::new(p.x, p.y + 10.0, p.z), 20.0);
        Vec3::new(p.x, if g > -100_000.0 { g } else { p.y }, p.z)
    }

    // ─── Ground ──────────────────────────────────────────────────────────────

    /// `cd_find_ground_at_cyl_ctfril` (`collision.c:2219`) over the tiles alone:
    /// `(ground, polygon)`, with [`NO_GROUND`] when nothing was found.
    pub fn cd_find_ground_at_cyl(&self, pos: Vec3, radius: f32) -> (f32, Option<usize>) {
        let g = self.cd_find_ground_at_cyl_ctfril(pos, radius, &[]);
        (g.y, g.poly)
    }

    /// `cd_find_ground_at_cyl_ctfril` (`collision.c:2219`) + `cd_find_ground_finalise`
    /// (`collision.c:1826`, NTSC 1.0+): the floor tiles and the props' floors the
    /// cylinder's circle touches (up to 20, no vertical test). If the centre is
    /// over any of them, the highest height there that is below `pos.y` (a
    /// `GEOFLAG_STEP` tile's even if higher, once nothing else is); a
    /// `GEOFLAG_DIE` tile is never under the centre. Otherwise (or when a slope
    /// is among them) the height at the nearest point on the nearest edge of a
    /// touched tile, a die tile's only while nothing else is found.
    pub fn cd_find_ground_at_cyl_ctfril(&self, pos: Vec3, radius: f32, floors: &[PropFloor]) -> Ground {
        // (tile or prop floor, intile)
        let mut collisions: Vec<(GroundSrc, bool)> = Vec::new();
        for &poly in &self.floors {
            if collisions.len() >= 20 {
                break;
            }
            if self.tile_in_range(poly, pos, radius, false, 0.0, 0.0) && self.volume_collect_tile(poly, pos.x, pos.z, radius).is_some() {
                collisions.push((GroundSrc::Tile(poly), false));
            }
        }
        for (k, f) in floors.iter().enumerate() {
            if collisions.len() >= 20 {
                break;
            }
            if f.flags & (GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2) != 0 && f.in_range(pos, radius) && f.collect(pos.x, pos.z, radius) {
                collisions.push((GroundSrc::Floor(k), false));
            }
        }
        let mut curground = NO_GROUND;
        let mut found: Option<GroundSrc> = None;
        let mut anyintile = false;
        let mut hasflag0100 = false;
        for c in &mut collisions {
            match c.0 {
                GroundSrc::Tile(poly) => {
                    let p = &self.geom.polys[poly];
                    if p.geoflags & GEOFLAG_DIE != 0 {
                        c.1 = false;
                    } else {
                        if p.geoflags & GEOFLAG_SLOPE != 0 {
                            hasflag0100 = true;
                        }
                        c.1 = p.xz_in_convex(pos.x, pos.z);
                    }
                }
                GroundSrc::Floor(k) => c.1 = floors[k].xz_in(pos.x, pos.z),
            }
            anyintile |= c.1;
        }
        let mut hasground = false;
        let mut hasdie = false;
        let step = |src: GroundSrc| matches!(src, GroundSrc::Tile(p) if self.geom.polys[p].geoflags & GEOFLAG_STEP != 0);
        if anyintile {
            for &(src, intile) in &collisions {
                if intile && !step(src) {
                    let ground = match src {
                        GroundSrc::Tile(p) => self.geom.polys[p].find_y(pos.x, pos.z),
                        GroundSrc::Floor(k) => floors[k].find_y(pos.x, pos.z),
                    };
                    if ground >= curground && ground < pos.y {
                        curground = ground;
                        found = Some(src);
                        hasground = true;
                    }
                }
            }
            for &(src, intile) in &collisions {
                if intile && step(src) {
                    let GroundSrc::Tile(p) = src else { continue };
                    let ground = self.geom.polys[p].find_y(pos.x, pos.z);
                    if ground >= curground && (ground < pos.y || !hasground) {
                        curground = ground;
                        found = Some(src);
                        hasground = true;
                    }
                }
            }
        }
        let hasgroundfromearlier = hasground;
        if !hasground || hasflag0100 {
            let mut spe4 = 4_294_967_296.0f32;
            for &(src, intile) in &collisions {
                if intile {
                    continue;
                }
                let slope = matches!(src, GroundSrc::Tile(p) if self.geom.polys[p].geoflags & GEOFLAG_SLOPE != 0);
                if hasgroundfromearlier && !slope {
                    continue;
                }
                let (verts, isdie, isstep): (Vec<Vec3>, bool, bool) = match src {
                    GroundSrc::Tile(p) => {
                        let poly = &self.geom.polys[p];
                        (poly.verts.clone(), poly.geoflags & GEOFLAG_DIE != 0, poly.geoflags & GEOFLAG_STEP != 0)
                    }
                    GroundSrc::Floor(k) => (floors[k].verts.to_vec(), false, false),
                };
                if isdie && hasground {
                    continue;
                }
                let tilei = matches!(src, GroundSrc::Tile(_));
                let height = |x: f32, z: f32, i: usize| match src {
                    GroundSrc::Tile(p) => self.geom.polys[p].find_y_vtx(x, z, i),
                    GroundSrc::Floor(k) => floors[k].find_y(x, z),
                };
                for i in 0..verts.len() {
                    let next = (i + 1) % verts.len();
                    let (thisx, thisz, nextx, nextz) = (verts[i].x, verts[i].z, verts[next].x, verts[next].z);
                    let spd4 = cd_pos_get_dist_to_line(thisx, thisz, nextx, nextz, pos.x, pos.z);
                    let f30 = spd4.abs();
                    // A die tile taken so far gives way to any other edge.
                    let closer = |d: f32, spe4: f32, hasdie: bool| d < spe4 || (tilei && hasdie);
                    let mut take = |x: f32, z: f32, d: f32, spe4: &mut f32, hasdie: &mut bool, hasground: &mut bool| {
                        let ground = height(x, z, i);
                        if ground < pos.y || isstep {
                            curground = ground;
                            found = Some(src);
                            *spe4 = d;
                            *hasground = true;
                            *hasdie = isdie;
                        }
                    };
                    if closer(f30, spe4, hasdie) {
                        if cd_pos_get_side(thisx, thisz, nextx, nextz, pos.x, pos.z) {
                            let (spb8, spb4) = (nextx - thisx, nextz - thisz);
                            let f14 = spd4 / (spb8 * spb8 + spb4 * spb4).sqrt();
                            take(pos.x + f14 * -spb4, pos.z + f14 * spb8, f30, &mut spe4, &mut hasdie, &mut hasground);
                        } else {
                            let thisvalue = cd_pos_get_dist_to_vtx(thisx, thisz, pos.x, pos.z);
                            let nextvalue = cd_pos_get_dist_to_vtx(nextx, nextz, pos.x, pos.z);
                            if thisvalue < nextvalue {
                                if closer(thisvalue, spe4, hasdie) {
                                    take(thisx, thisz, thisvalue, &mut spe4, &mut hasdie, &mut hasground);
                                }
                            } else if closer(nextvalue, spe4, hasdie) {
                                take(nextx, nextz, nextvalue, &mut spe4, &mut hasdie, &mut hasground);
                            }
                        }
                    }
                }
            }
        }
        let mut g = Ground { y: curground, ..Default::default() };
        match found {
            // cd_get_floor_type (collision.c:520): an underwater tile is water.
            Some(GroundSrc::Tile(p)) => {
                let poly = &self.geom.polys[p];
                g.poly = Some(p);
                g.flags = poly.geoflags;
                g.room = poly.room;
                g.floortype = if poly.geoflags & GEOFLAG_UNDERWATER != 0 { FLOORTYPE_WATER } else { poly.floortype };
            }
            Some(GroundSrc::Floor(k)) => {
                g.floor = Some(k);
                g.prop = Some(floors[k].prop);
                g.flags = floors[k].flags;
                g.room = floors[k].room;
            }
            None => {}
        }
        g
    }

    /// `cd_find_y` over the floor tiles (`collision.c:985`, `GEOFLAG_FLOOR1 |
    /// GEOFLAG_FLOOR2`): of the floors whose outline holds `pos`, the highest at or
    /// below it (`ceiling` false), or the lowest at or above it (`ceiling` true).
    fn cd_find_y(&self, pos: Vec3, ceiling: bool) -> Option<(f32, usize)> {
        self.cd_find_y_in(pos, ceiling, None)
    }

    /// [`Self::cd_find_y`] over the tiles of `rooms` only (`None`: every tile),
    /// room by room in the list's order, as PD walks them: on a tie the first
    /// room's floor wins.
    fn cd_find_y_in(&self, pos: Vec3, ceiling: bool, rooms: Option<&[u16]>) -> Option<(f32, usize)> {
        let order: Vec<usize> = match rooms {
            None => self.floors.clone(),
            Some(rooms) => rooms.iter().flat_map(|&r| self.floors.iter().copied().filter(move |&p| self.geom.polys[p].room == Some(r))).collect(),
        };
        let mut best: Option<(f32, usize)> = None;
        for poly in order {
            let (lo, hi) = self.bbox[poly];
            if pos.x < lo.x || pos.x > hi.x || pos.z < lo.z || pos.z > hi.z {
                continue;
            }
            if (!ceiling && pos.y < lo.y) || (ceiling && pos.y > hi.y) {
                continue;
            }
            let p = &self.geom.polys[poly];
            if !p.xz_in_convex(pos.x, pos.z) {
                continue;
            }
            let y = p.find_y(pos.x, pos.z);
            let better = match best {
                None => true,
                Some((b, _)) => (!ceiling && y > b) || (ceiling && y < b),
            };
            if better && ((!ceiling && y <= pos.y) || (ceiling && y >= pos.y)) {
                best = Some((y, poly));
            }
        }
        best
    }

    /// `cd_find_room_at_pos` (`collision.c:2312`): the room of the floor under
    /// `pos` among the tiles of `nearrooms`.
    pub fn cd_find_room_at_pos(&self, pos: Vec3, nearrooms: &[u16]) -> Option<u16> {
        self.cd_find_y_in(pos, false, Some(nearrooms)).and_then(|(_, p)| self.geom.polys[p].room)
    }

    /// `cd_find_ground_at_pos_ct` (`collision.c:2288`): the height of the floor
    /// under `pos` among the tiles of `rooms`, −2³² for none.
    pub fn cd_find_ground_at_pos_ct(&self, pos: Vec3, rooms: &[u16]) -> f32 {
        self.cd_find_y_in(pos, false, Some(rooms)).map_or(-4_294_967_296.0, |(y, _)| y)
    }

    /// `cd_find_room_at_pos_ycnp` (`collision.c:2381`): the floor under `pos`,
    /// its height and polygon; `None` where PD finds no room.
    pub fn cd_find_room_at_pos_ycnp(&self, pos: Vec3) -> Option<(f32, usize)> {
        self.cd_find_y(pos, false)
    }

    /// `cd_find_ceiling_room_at_pos_ycfn` (`collision.c:2401`): the nearest floor
    /// at or above `pos` (what a falling object's bottom has sunk through).
    pub fn cd_find_ceiling_room_at_pos_ycfn(&self, pos: Vec3) -> Option<(f32, usize)> {
        self.cd_find_y(pos, true)
    }

    /// A room's bounding box: `g_Rooms[].bbmin/bbmax` ([`TileLevel::for_stage`];
    /// a fixture's is the box over its tiles).
    pub fn room_bbox(&self, room: u16) -> Option<(Vec3, Vec3)> {
        self.room_bboxes.get(&room).copied()
    }

    /// `bg_room_get_neighbours`.
    pub fn room_neighbours(&self, room: u16) -> Vec<u16> {
        self.room_neighbours.get(&room).cloned().unwrap_or_default()
    }

    /// `bg_rooms_are_neighbours`.
    pub fn rooms_are_neighbours(&self, a: u16, b: u16) -> bool {
        self.room_neighbours.get(&a).is_some_and(|l| l.contains(&b))
    }

    /// The room of the floor a chr at `pos` stands on (`chr->floorroom`).
    pub fn floor_room(&self, pos: Vec3, radius: f32) -> Option<u16> {
        self.cd_find_ground_at_cyl(Vec3::new(pos.x, pos.y + 69.0, pos.z), radius).1.and_then(|p| self.geom.polys[p].room)
    }

    // ─── Sight and shots ─────────────────────────────────────────────────────

    fn first_hit(&self, list: &[usize], from: Vec3, to: Vec3) -> Option<RayHit> {
        let d = to - from;
        let (lo_s, hi_s) = (from.min(to), from.max(to));
        let mut best: Option<RayHit> = None;
        for &poly in list {
            let (lo, hi) = self.bbox[poly];
            if hi.x < lo_s.x || lo.x > hi_s.x || hi.y < lo_s.y || lo.y > hi_s.y || hi.z < lo_s.z || lo.z > hi_s.z {
                continue;
            }
            let v = &self.geom.polys[poly].verts;
            for k in 2..v.len() {
                if let Some(t) = segment_triangle(from, d, v[0], v[k - 1], v[k]) {
                    if best.is_none_or(|b| t < b.dist) {
                        best = Some(RayHit { dist: t, point: from + d * t, poly });
                    }
                }
            }
        }
        best.map(|mut b| {
            b.dist *= d.length();
            b
        })
    }

    /// The first floor or wall tile the segment `from` → `to` meets, of any
    /// kind (see-through ones too): the level editor's picking. Not PD.
    pub fn first_tile_hit(&self, from: Vec3, to: Vec3) -> Option<RayHit> {
        match (self.first_hit(&self.floors, from, to), self.first_hit(&self.walls, from, to)) {
            (Some(f), Some(w)) => Some(if f.dist <= w.dist { f } else { w }),
            (f, w) => f.or(w),
        }
    }

    /// Clear line of sight (`GEOFLAG_BLOCK_SIGHT` polygons only; chrs never block).
    pub fn los(&self, from: Vec3, to: Vec3) -> bool {
        self.first_hit(&self.sight, from, to).is_none()
    }

    /// `cd_test_los_oobfail(..., GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2)`: no floor in the way
    /// (what `waypoint_find_closest_to_pos` asks first — "is the pad on my floor").
    pub fn los_floors(&self, from: Vec3, to: Vec3) -> bool {
        self.first_hit(&self.floors, from, to).is_none()
    }

    /// `cd_test_los_*_autoflags` against the background: blocked by any polygon that
    /// is a wall, blocks sight or blocks shots.
    pub fn los_autoflags(&self, from: Vec3, to: Vec3) -> bool {
        self.first_hit(&self.any_blocker, from, to).is_none()
    }

    /// The first floor polygon along `origin + dir·t`, t ≤ `max_dist` (viewer picking).
    pub fn raycast_floor(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        self.first_hit(&self.floors, origin, origin + dir * max_dist)
    }

    /// The first `GEOFLAG_WALL` polygon along `origin + dir·t`, t ≤ `max_dist`
    /// (`cd_test_los_oobok_findclosest(..., GEOFLAG_WALL)`, `chr_run_from_pos`).
    pub fn raycast_walls(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        self.first_hit(&self.walls, origin, origin + dir * max_dist)
    }

    /// The first `GEOFLAG_BLOCK_SHOOT` polygon along `origin + dir·t`, t ≤ `max_dist`.
    pub fn raycast_shoot(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        self.first_hit(&self.shot, origin, origin + dir * max_dist)
    }
}

/// SUBST: PD reads a room's neighbours from its portals (`bg_room_get_neighbours`): two rooms are
/// neighbours when an edge of one room's polygon runs along an edge of the
/// other's — collinear and overlapping by more than a centimetre, in 3D or seen
/// from above. "Along", not "equal": where two rooms meet, one room's edge is often
/// split by the other's vertices (a T-junction). "Seen from above" joins rooms
/// stacked over open air, like a walkway and the floor under it. Checked against
/// PD's own graph, whose linked waypoints must share a room or neighbour rooms
/// (`padhalllv.c:67`).
fn infer_room_neighbours(geom: &LevelGeom) -> std::collections::BTreeMap<u16, Vec<u16>> {
    struct E {
        room: u16,
        a: Vec3,
        b: Vec3,
    }
    let mut edges = Vec::new();
    for p in &geom.polys {
        let Some(room) = p.room else { continue };
        for i in 0..p.verts.len() {
            let (a, b) = (p.verts[i], p.verts[(i + 1) % p.verts.len()]);
            if a.distance(b) > 1.0 {
                edges.push(E { room, a, b });
            }
        }
    }
    // Overlap of segment q onto segment p, both collinear within 1 cm.
    fn along(pa: Vec3, pb: Vec3, qa: Vec3, qb: Vec3) -> bool {
        let d = pb - pa;
        let len = d.length();
        if len < 1.0 {
            return false;
        }
        let u = d / len;
        let off = |q: Vec3| (q - pa) - u * (q - pa).dot(u);
        if off(qa).length() > 1.0 || off(qb).length() > 1.0 {
            return false;
        }
        let (t0, t1) = ((qa - pa).dot(u), (qb - pa).dot(u));
        let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(len));
        hi - lo > 1.0
    }
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let mut out: std::collections::BTreeMap<u16, Vec<u16>> = Default::default();
    for (i, e) in edges.iter().enumerate() {
        for f in &edges[i + 1..] {
            if e.room == f.room || out.get(&e.room).is_some_and(|l| l.contains(&f.room)) {
                continue;
            }
            let lo = e.a.min(e.b) - Vec3::ONE;
            let hi = e.a.max(e.b) + Vec3::ONE;
            let (flo, fhi) = (f.a.min(f.b), f.a.max(f.b));
            if flo.x > hi.x || fhi.x < lo.x || flo.z > hi.z || fhi.z < lo.z {
                continue;
            }
            let joined = along(e.a, e.b, f.a, f.b) || along(flat(e.a), flat(e.b), flat(f.a), flat(f.b));
            if joined {
                out.entry(e.room).or_default().push(f.room);
                out.entry(f.room).or_default().push(e.room);
            }
        }
    }
    for list in out.values_mut() {
        list.sort();
        list.dedup();
    }
    out
}

/// Segment `o + d·t` (t in 0..=1) against a triangle, both faces: returns `t`.
fn segment_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let w = d.dot(q) * inv;
    if w < 0.0 || u + w > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (0.0..=1.0).contains(&t).then_some(t)
}

/// `cd_pos_get_dist_to_line` (`collision.c:370`): signed perpendicular distance.
pub fn cd_pos_get_dist_to_line(x1: f32, z1: f32, x2: f32, z2: f32, posx: f32, posz: f32) -> f32 {
    let length = ((x2 - x1) * (x2 - x1) + (z2 - z1) * (z2 - z1)).sqrt();
    if length == 0.0 {
        return ((posx - x2) * (posx - x2) + (posz - z2) * (posz - z2)).sqrt();
    }
    ((posx - x1) * (z2 - z1) + -(x2 - x1) * (posz - z1)) / length
}

/// `cd_pos_get_dist_to_vtx` (`collision.c:384`).
pub fn cd_pos_get_dist_to_vtx(x1: f32, z1: f32, posx: f32, posz: f32) -> f32 {
    ((posx - x1) * (posx - x1) + (posz - z1) * (posz - z1)).sqrt()
}

/// `cd_pos_get_side` (`collision.c:396`). Despite the name, this is "does the
/// perpendicular from the point land strictly inside the segment": the first of
/// PD's two clauses (`f18 < f16 && f16 < 0`) can never hold, since `f18 >= 0`.
pub fn cd_pos_get_side(x1: f32, z1: f32, x2: f32, z2: f32, posx: f32, posz: f32) -> bool {
    let (px, pz) = (posx - x1, posz - z1);
    let (x2_2, z2_2) = (x2 - x1, z2 - z1);
    let f16 = px * x2_2 + pz * z2_2;
    let f18 = x2_2 * x2_2 + z2_2 * z2_2;
    (f18 < f16 && f16 < 0.0) || (f16 > 0.0 && f18 > f16)
}

/// `cd_pos_get_cyl_edge` (`collision.c:420`): a "wall" tangent to the cylinder,
/// facing `pos`.
pub fn cd_pos_get_cyl_edge(cylx: f32, cylz: f32, cylradius: f32, posx: f32, posz: f32) -> (Vec2, Vec2) {
    let (mut px, mut pz) = (posx - cylx, posz - cylz);
    if px != 0.0 || pz != 0.0 {
        let dist = (px * px + pz * pz).sqrt();
        if dist > 0.0 {
            let k = cylradius / dist;
            px *= k;
            pz *= k;
        }
    }
    (Vec2::new(cylx + px + pz, cylz + pz - px), Vec2::new(cylx + px - pz, cylz + pz + px))
}

/// `cd_00025410` (`collision.c:301`): which side of `(x1, z1)` `(x2, z2)` turns to,
/// with PD's collinear tie-breaks.
fn cd_00025410(x1: f32, z1: f32, x2: f32, z2: f32) -> i32 {
    let f0 = x1 * z2;
    let f2 = z1 * x2;
    if f2 < f0 {
        return 1;
    }
    if f2 > f0 {
        return -1;
    }
    if x1 * x2 < 0.0 || z1 * z2 < 0.0 {
        return -1;
    }
    if x1 * x1 + z1 * z1 < x2 * x2 + z2 * z2 {
        return 1;
    }
    0
}

/// `cd_000254d8` (`collision.c:325`): does segment `frompos → topos` cross edge
/// `(x1, z1) → (x2, z2)` in XZ? Also clears `first` once the start point is seen
/// to be outside this edge (so `first` survives only if it starts inside).
fn cd_000254d8(frompos: Vec3, topos: Vec3, x1: f32, z1: f32, x2: f32, z2: f32, first: &mut bool) -> bool {
    let sp54 = frompos.x - x1;
    let sp50 = frompos.z - z1;
    let sp3c = cd_00025410(x2 - x1, z2 - z1, sp54, sp50);
    let sp44 = cd_00025410(x2 - x1, z2 - z1, topos.x - x1, topos.z - z1);
    let mut result = false;
    if sp3c * sp44 <= 0 {
        let sp4c = topos.x - frompos.x;
        let sp48 = topos.z - frompos.z;
        let sp34 = cd_00025410(sp4c, sp48, -sp54, -sp50);
        let sp40 = cd_00025410(sp4c, sp48, x2 - frompos.x, z2 - frompos.z);
        if sp34 * sp40 <= 0 {
            result = true;
        }
    }
    if *first && (result || sp3c <= 0) {
        *first = false;
    }
    result
}

/// `func0f1577f0` (`collisionutils.c:15`): fraction along `a0 → a1` where it meets
/// the line through `a2, a3`; 1 when parallel or outside 0..1.
fn func0f1577f0(a0: Vec2, a1: Vec2, a2: Vec2, a3: Vec2) -> f32 {
    let mult1 = a2.y - a3.y;
    let mult2 = a3.x - a2.x;
    let a = (a2.y - a0.y) * mult2 + (a2.x - a0.x) * mult1;
    let b = (a1.y - a0.y) * mult2 + (a1.x - a0.x) * mult1;
    if b == 0.0 {
        return 1.0;
    }
    let a = a / b;
    if !(0.0..=1.0).contains(&a) {
        return 1.0;
    }
    a
}

/// `func0f1578c8` (`collisionutils.c:34`): how far along the unit direction `dir`
/// a circle of `radius` at `centre` travels before it touches the point `vtx`;
/// `f32::MAX` if it never does.
fn func0f1578c8(centre: Vec2, radius: f32, dir: Vec2, vtx: Vec2) -> f32 {
    let mult1 = vtx.x - centre.x;
    let mult2 = vtx.y - centre.y;
    let value1 = mult2 * dir.x - mult1 * dir.y;
    let mut value2 = mult1 * dir.x + mult2 * dir.y;
    let sp24 = (radius - value1) * (radius + value1);
    if sp24 < 0.0 {
        return f32::MAX;
    }
    value2 -= sp24.sqrt();
    if value2 < 0.0 {
        if value2 * value2 + value1 * value1 <= radius * radius {
            return 0.0;
        }
        return f32::MAX;
    }
    value2
}

/// `func0f1579cc` (`collisionutils.c:68`): the fraction (0..1) of the move `diff`
/// a circle of `radius` at `centre` makes before it meets the edge `v1 -> v2`
/// (its line pushed out by the radius on the circle's side, or an end point);
/// 1 when it never does.
pub fn func0f1579cc(centre: Vec2, radius: f32, v1: Vec2, v2: Vec2, diff: Vec2) -> f32 {
    let spac = (diff.x * diff.x + diff.y * diff.y).sqrt();
    if spac == 0.0 {
        return 1.0;
    }
    let spa0 = diff * (1.0 / spac);
    let (mut v1, mut v2) = (v1, v2);
    let sp98 = v2.x - v1.x;
    let sp9c = v2.y - v1.y;
    let sp94 = (sp98 * sp98 + sp9c * sp9c).sqrt();
    let sp60;
    if sp94 == 0.0 {
        // `goto handlezero`: a zero-length edge is its end point.
        sp60 = func0f1578c8(centre, radius, spa0, v2);
    } else {
        let sp90 = 1.0 / sp94;
        let mut sp88 = sp9c * sp90;
        let mut sp8c = -sp98 * sp90;
        let mut sp84 = radius * sp88;
        let mut sp80 = radius * sp8c;
        if sp84 * (centre.x - v1.x) + sp80 * (centre.y - v1.y) < 0.0 {
            sp84 = -sp84;
            sp80 = -sp80;
        }
        let sp78 = v1.x + sp84;
        let sp7c = v1.y + sp80;
        let sp70 = v2.x + sp84;
        let sp74 = v2.y + sp80;
        let mut sp68 = diff.y * sp78 - sp7c * diff.x;
        let sp6c = centre.x * diff.y - centre.y * diff.x;
        let mut sp64 = diff.y * sp70 - sp74 * diff.x;
        if sp64 < sp68 {
            std::mem::swap(&mut sp64, &mut sp68);
            std::mem::swap(&mut v1, &mut v2);
            sp88 = -sp88;
            sp8c = -sp8c;
        }
        if sp64 == sp68 {
            let a = func0f1578c8(centre, radius, spa0, v1);
            let b = func0f1578c8(centre, radius, spa0, v2);
            sp60 = a.min(b);
        } else if sp64 < sp6c {
            sp60 = func0f1578c8(centre, radius, spa0, v2);
        } else if sp6c < sp68 {
            sp60 = func0f1578c8(centre, radius, spa0, v1);
        } else {
            let sp58 = sp88 * (centre.x - v1.x) + sp8c * (centre.y - v1.y);
            let sp54 = sp88 * (centre.x + diff.x - v1.x) + sp8c * (centre.y + diff.y - v1.y);
            if sp58 == sp54 {
                return 1.0;
            }
            sp60 = (sp58 - radius) * spac / (sp58 - sp54);
        }
    }
    if spac < sp60 {
        return 1.0;
    }
    if sp60 < 0.0 {
        return 0.0;
    }
    sp60 * (1.0 / spac)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::fixtures::{self, ARENA_PILLARS, ARENA_PILLAR_HALF};

    fn arena() -> TileLevel {
        TileLevel::new(fixtures::arena())
    }

    /// `cd_find_ladder` matches the geoflags asked for: a
    /// `GEOFLAG_LADDER_PLAYERONLY` tile is a ladder to a player
    /// (`bondwalk.c:792` asks for both bits) and not to a chr (`chr.c:622`).
    #[test]
    fn a_player_only_ladder_is_climbed_by_players_alone() {
        use pd_core::ids::{GEOFLAG_LADDER, GEOFLAG_LADDER_PLAYERONLY, GEOFLAG_WALL};
        let wall = |x: f32, flags: u32| {
            let v = vec![Vec3::new(x, 0.0, -50.0), Vec3::new(x, 0.0, 50.0), Vec3::new(x, 150.0, 50.0), Vec3::new(x, 150.0, -50.0)];
            GeomPoly::from_tile(1, GEOFLAG_WALL | flags, 0, v)
        };
        let l = TileLevel::new(LevelGeom { polys: vec![wall(0.0, GEOFLAG_LADDER_PLAYERONLY), wall(500.0, GEOFLAG_LADDER)], rooms: vec![1] });
        let near = |x: f32| Vec3::new(x - 25.0, 50.0, 0.0);
        let player = GEOFLAG_LADDER | GEOFLAG_LADDER_PLAYERONLY;
        assert!(l.cd_find_ladder(near(0.0), 30.0, 130.0, -49.0, player).is_some());
        assert!(l.cd_find_ladder(near(0.0), 30.0, 130.0, -49.0, GEOFLAG_LADDER).is_none());
        assert!(l.cd_find_ladder(near(500.0), 30.0, 130.0, -49.0, GEOFLAG_LADDER).is_some());
        assert!(l.cd_find_ladder(near(500.0), 30.0, 130.0, -49.0, player).is_some());
    }

    #[test]
    fn the_arena_as_geometry_has_ground_walls_and_sight() {
        let l = arena();
        let (g, poly) = l.cd_find_ground_at_cyl(Vec3::new(0.0, 69.0, 0.0), 20.0);
        assert_eq!(g, 0.0);
        assert!(poly.is_some());
        // Walls are surfaces, as in PD: a cylinder deep inside a pillar (75 cm from
        // every face) meets nothing; one straddling a face does.
        let c = ARENA_PILLARS[0];
        let p = (c - Vec2::splat(ARENA_PILLAR_HALF), c + Vec2::splat(ARENA_PILLAR_HALF));
        assert_eq!(l.cd_test_volume_simple(Vec3::new(c.x, 50.0, c.y), 20.0, true, 135.0, -30.0, &[]), CdResult::NoCollision);
        assert_eq!(l.cd_test_volume_simple(Vec3::new(p.0.x + 10.0, 50.0, c.y), 20.0, true, 135.0, -30.0, &[]), CdResult::Collision);
        assert_eq!(l.cd_test_volume_simple(Vec3::new(0.0, 50.0, 0.0), 20.0, true, 135.0, -30.0, &[]), CdResult::NoCollision);
        // A cylinder just touching a pillar face collides; 1 cm further out it doesn't.
        let face = Vec3::new(p.0.x - 20.0, 50.0, c.y);
        assert_eq!(l.cd_test_volume_simple(face, 20.0, true, 135.0, -30.0, &[]), CdResult::Collision);
        assert_eq!(l.cd_test_volume_simple(face - Vec3::X, 20.0, true, 135.0, -30.0, &[]), CdResult::NoCollision);
        // Sight: blocked through the pillar, clear beside it.
        assert!(!l.los(Vec3::new(c.x - 300.0, 150.0, c.y), Vec3::new(c.x + 300.0, 150.0, c.y)));
        assert!(l.los(Vec3::new(c.x - 300.0, 150.0, c.y + 200.0), Vec3::new(c.x + 300.0, 150.0, c.y + 200.0)));
        let hit = l.raycast_shoot(Vec3::new(0.0, 100.0, 0.0), Vec3::X, 5000.0).unwrap();
        assert!((hit.dist - 800.0).abs() < 0.5, "{}", hit.dist);
    }

    #[test]
    fn a_swept_move_through_a_wall_reports_that_walls_edge() {
        let l = arena();
        let from = Vec3::new(700.0, 50.0, 0.0);
        let (r, edge) = l.cd_test_cylmove_oobfail_findclosest(from, Vec3::new(900.0, 50.0, 0.0), 20.0, 135.0, -30.0, &[]);
        assert_eq!(r, CdResult::Collision);
        let (a, b) = edge.unwrap();
        assert!((a.x - 800.0).abs() < 1e-3 && (b.x - 800.0).abs() < 1e-3, "{a} {b}");
        assert_eq!(l.cd_test_cylmove_oobok(from, Vec3::new(700.0, 50.0, 300.0), 135.0, -30.0, &[]), CdResult::NoCollision);
    }

    #[test]
    fn past_a_ledge_the_floor_under_the_centre_wins_else_the_nearest_edge() {
        // A 2 m platform at 280 cm. Over a void, a centre 10 cm past its edge still
        // stands on it (the nearest-edge branch of `cd_find_ground_finalise`). Over
        // a lower floor that branch never runs: the floor under the centre wins and
        // the chr drops, as in PD.
        let quad = |y: f32, x0: f32, x1: f32| {
            GeomPoly::new(
                vec![Vec3::new(x0, y, 0.0), Vec3::new(x1, y, 0.0), Vec3::new(x1, y, 200.0), Vec3::new(x0, y, 200.0)],
                true,
                false,
                true,
                true,
                None,
            )
        };
        let void = TileLevel::new(LevelGeom { polys: vec![quad(280.0, 0.0, 200.0)], rooms: vec![] });
        assert_eq!(void.cd_find_ground_at_cyl(Vec3::new(210.0, 349.0, 100.0), 20.0).0, 280.0);
        assert_eq!(void.cd_find_ground_at_cyl(Vec3::new(230.0, 349.0, 100.0), 20.0).0, NO_GROUND);
        let over = TileLevel::new(LevelGeom { polys: vec![quad(280.0, 0.0, 200.0), quad(0.0, -500.0, 700.0)], rooms: vec![] });
        assert_eq!(over.cd_find_ground_at_cyl(Vec3::new(190.0, 349.0, 100.0), 20.0).0, 280.0);
        assert_eq!(over.cd_find_ground_at_cyl(Vec3::new(210.0, 349.0, 100.0), 20.0).0, 0.0);
    }

    /// A ramp's triangular side wall (`GEOFLAG_RAMPWALL`) meets a cylinder
    /// whose height range crosses the ramp's edge above it, not one standing
    /// on the ramp above the wall; a slope's ramp wall meets nothing while
    /// `enableslopes` is off.
    #[test]
    fn a_ramp_wall_only_meets_what_is_beside_it() {
        let mut wall = GeomPoly::new(vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 0.0), Vec3::new(100.0, 0.0, 0.0)], false, true, true, true, Some(1));
        wall.geoflags = GEOFLAG_RAMPWALL | pd_core::ids::GEOFLAG_WALL | GEOFLAG_SLOPE;
        let l = TileLevel::new(LevelGeom { polys: vec![wall], rooms: vec![1] });
        // At x = 50 the ramp's edge is 50 cm up.
        let at = Vec3::new(50.0, 0.0, 0.0);
        assert_ne!(l.cd_test_ramp_wall(0, at, 0.0, 100.0), 0);
        assert_eq!(l.cd_test_ramp_wall(0, at, 60.0, 160.0), 0);
        assert_eq!(l.cd_test_volume_simple(Vec3::new(50.0, 110.0, 5.0), 20.0, true, 50.0, -50.0, &[]), CdResult::NoCollision);
        assert_eq!(l.cd_test_volume_simple(Vec3::new(50.0, 40.0, 5.0), 20.0, true, 50.0, -50.0, &[]), CdResult::Collision);
        let off = EnableSlopes::set(false);
        assert_eq!(l.cd_test_ramp_wall(0, at, 0.0, 100.0), 0);
        drop(off);
        assert_ne!(l.cd_test_ramp_wall(0, at, 0.0, 100.0), 0);
    }
}
