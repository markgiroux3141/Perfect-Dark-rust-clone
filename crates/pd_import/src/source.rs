//! The level source: what a game-specific importer ([`crate::oot`]) hands the
//! rest of the pipeline, already in PD's terms. Every position is PD world
//! space (centimetres, Y up); collision is PD tiles' flags; the lighting is
//! baked into the vertex colours, as a PD BG's is.
//!
//! From here on nothing knows which game a level came from: the rooms
//! ([`crate::rooms`]), the stage files ([`crate::write`]) and the gameplay
//! data ([`crate::place`]) read only this.

use glam::{Vec2, Vec3};

/// How a material's pixels meet the framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    /// Opaque with the texture's alpha as a cut-out (`G_RM_AA_ZB_TEX_EDGE`).
    Cutout,
    /// Blended by the texture's alpha (`G_RM_AA_ZB_XLU_SURF`).
    Translucent,
}

/// A texture axis's addressing, as PD's `TXMODE_*` numbers it (0 wrap, 1
/// clamp, 2 mirror).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Repeat = 0,
    Clamp = 1,
    Mirror = 2,
}

/// An encoded PNG and its size.
#[derive(Clone, Debug)]
pub struct Texture {
    pub png: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug)]
pub struct Material {
    /// Into [`LevelSource::textures`].
    pub texture: Option<usize>,
    pub wrap: [Wrap; 2],
    pub blend: Blend,
    /// Back faces culled (else drawn both sides).
    pub cull_back: bool,
    pub zwrite: bool,
    /// `ZMODE_DEC`: drawn over coplanar geometry.
    pub decal: bool,
    /// What a shot hitting it sounds and looks like: PD's `SURFACETYPE_*`
    /// (`g_Textures[].surfacetype` and `soundsurfacetype`, `tex.c:160`).
    pub surface: u8,
    /// One of PD's own materials in place of everything above (`texture` is
    /// `None` then): a level drawn on the game's texture pool.
    pub pd: Option<PdMaterial>,
}

/// A material of one of PD's arenas, on a texture of the game's pool
/// (`assets/textures/<nnnn>.png`): its rows of the arena's `bg.json`, written
/// into the level's as they are. The level then carries no image of its own
/// for it, is drawn through PD's own combiner for that texture (mip-mapped,
/// the alpha where PD takes it from), wraps and clamps as the arena does, and
/// a shot or a footstep on it sounds as on the arena (the pool's surface
/// types).
#[derive(Clone, Debug)]
pub struct PdMaterial {
    /// The pool texture's number (`0x073e`).
    pub texture: u32,
    /// Its size in texels: a vertex's `uv` (0..1 across it, v up the image as
    /// the Setup Editor shows it, which is down the pool's rows) times this
    /// is PD's s, t.
    pub size: (u32, u32),
    /// `bg.json` `materials[k]`.
    pub row: serde_json::Value,
    /// `bg.json` `textures[<number>]`.
    pub texture_row: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
    pub pos: Vec3,
    /// 0..1 across the texture.
    pub uv: Vec2,
    /// The baked colour (and alpha).
    pub col: [u8; 4],
}

impl Vert {
    /// The vertex `t` of the way from `self` to `b`.
    pub fn lerp(&self, b: &Vert, t: f32) -> Vert {
        let c = |i: usize| (self.col[i] as f32 + (b.col[i] as f32 - self.col[i] as f32) * t).round() as u8;
        Vert { pos: self.pos.lerp(b.pos, t), uv: self.uv.lerp(b.uv, t), col: [c(0), c(1), c(2), c(3)] }
    }
}

/// A drawn triangle.
#[derive(Clone, Debug)]
pub struct Tri {
    pub v: [Vert; 3],
    pub material: usize,
    /// Drawn in the translucent pass (the source's own draw buffer).
    pub xlu: bool,
    /// Its room, from 1, in a level that brings its own rooms
    /// ([`LevelSource::rooms`]); 0 otherwise (the split decides).
    pub room: u16,
}

/// A convex, planar collision polygon, flagged as a PD tile is.
#[derive(Clone, Debug)]
pub struct ColPoly {
    pub verts: Vec<Vec3>,
    /// `GEOFLAG_*`.
    pub flags: u32,
    /// `FLOORTYPE_*`.
    pub floortype: u8,
    /// A wall the source lets you climb over when it is a ledge (see
    /// [`LevelSource::mark_ledges`]).
    pub grab: bool,
    /// A floor's colour, `r << 8 | g << 4 | b` (`geotilei.floorcol`, four
    /// bits each): an object's fog starts from its tint (`floor_tint`).
    pub floorcol: u32,
    /// Its room, as [`Tri::room`].
    pub room: u16,
}

/// A triangle facing up at least this much is a floor (45°).
pub const FLOOR_NY: f32 = 0.7;
/// A triangle facing down at least this much is a ceiling (no tile).
pub const CEILING_NY: f32 = -0.5;

/// A drawn triangle as a tile, by PD's arenas' own rules (Complex's tiles
/// show them): a translucent upright one (a railing, a grille) is a wall seen
/// and shot through (`GEOFLAG_WALL` alone); a translucent flat one (a water
/// surface) and a ceiling are none; an opaque one facing up is a floor, its
/// `floorcol` the triangle's colour; the rest are walls. `(flags, floorcol)`.
pub fn tile_of_triangle(pos: [Vec3; 3], col: [[u8; 4]; 3], xlu: bool) -> Option<(u32, u32)> {
    use pd_core::ids::*;
    let n = (pos[1] - pos[0]).cross(pos[2] - pos[0]);
    if n.length_squared() < 1e-6 {
        return None;
    }
    let ny = n.normalize().y;
    if xlu {
        return (ny.abs() <= 0.5).then_some((GEOFLAG_WALL, 0));
    }
    if ny >= FLOOR_NY {
        let avg = |c: usize| (col.iter().map(|v| v[c] as u32).sum::<u32>() / 3) >> 4;
        Some((GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2 | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT, avg(0) << 8 | avg(1) << 4 | avg(2)))
    } else if ny <= CEILING_NY {
        None
    } else {
        Some((GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT, 0))
    }
}

/// A ceiling lower than this over the floor beneath it is a wall tile
/// ([`LevelSource::wall_low_ceilings`]): a standing head, Joanna's 159 cm eye
/// and the 13 above it (`player.c:1495`).
pub const LOW_CEILING: f32 = 172.0;

/// A triangle's unit normal's y; `None` for one without area.
fn facing(pos: [Vec3; 3]) -> Option<f32> {
    let n = (pos[1] - pos[0]).cross(pos[2] - pos[0]);
    (n.length_squared() >= 1e-6).then(|| n.normalize().y)
}

/// A translucent triangle facing up as a tile: a grate, a mesh catwalk. Stood
/// on, seen and shot through (`GEOFLAG_FLOOR1 | FLOOR2` alone), as PD's own
/// clipping makes its grated walkways (G5's, Pipes'). [`tile_of_triangle`]
/// makes it none, which is right for another game's water and wrong for a
/// level whose translucent floors are walked on. `(flags, floorcol)`.
pub fn grate_of_triangle(pos: [Vec3; 3], col: [[u8; 4]; 3]) -> Option<(u32, u32)> {
    use pd_core::ids::*;
    let avg = |c: usize| (col.iter().map(|v| v[c] as u32).sum::<u32>() / 3) >> 4;
    (facing(pos)? >= FLOOR_NY).then(|| (GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2, avg(0) << 8 | avg(1) << 4 | avg(2)))
}

/// Does the triangle face down enough to be a ceiling ([`tile_of_triangle`]
/// makes no tile of one)?
pub fn is_ceiling(pos: [Vec3; 3]) -> bool {
    facing(pos).is_some_and(|ny| ny <= CEILING_NY)
}

/// A place the source game marks: where players start, where it put its
/// pickups. Placement prefers these over arbitrary points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    /// A player start.
    Spawn,
    /// Somewhere people stand (an NPC).
    Person,
    /// A pickup.
    Item,
    /// A treasure: a spot for a prize weapon.
    Prize,
}

impl MarkerKind {
    pub fn name(self) -> &'static str {
        match self {
            MarkerKind::Spawn => "spawn",
            MarkerKind::Person => "person",
            MarkerKind::Item => "item",
            MarkerKind::Prize => "prize",
        }
    }

    pub fn from_name(s: &str) -> Option<MarkerKind> {
        [MarkerKind::Spawn, MarkerKind::Person, MarkerKind::Item, MarkerKind::Prize].into_iter().find(|k| k.name() == s)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub kind: MarkerKind,
    /// On (or just above) the floor.
    pub pos: Vec3,
    /// The facing, as PD's `atan2f(look.x, look.z)`.
    pub facing: f32,
}

/// The environment row the stage gets (`struct fogenvironment` /
/// `nofogenvironment`).
#[derive(Clone, Copy, Debug)]
pub struct Env {
    pub sky: [u8; 3],
    pub near: f32,
    pub far: f32,
    /// `fogmin`, `fogmax` (thousandths of the depth range): fog to the sky
    /// colour, as PD's fog stages do.
    pub fog: Option<(i32, i32)>,
}

pub struct LevelSource {
    pub textures: Vec<Texture>,
    pub materials: Vec<Material>,
    pub tris: Vec<Tri>,
    pub collision: Vec<ColPoly>,
    pub markers: Vec<Marker>,
    pub env: Env,
    /// The level's own rooms and portals, for a source that has them (a
    /// generated level's: [`crate::autolevel`]); `None`: its footprint is
    /// split into rooms ([`crate::rooms`]).
    pub rooms: Option<OwnRooms>,
}

/// Rooms and portals a source brings: every [`Tri`] and [`ColPoly`] names its
/// room, nothing is cut, and the portals are the level's own openings.
#[derive(Clone, Debug)]
pub struct OwnRooms {
    /// Room `i + 1`'s box (`g_Rooms[].bbmin`, `bbmax`): what holds its air
    /// and everything drawn in it.
    pub boxes: Vec<(Vec3, Vec3)>,
    /// Every opening between two rooms, each wound so PD takes its rooms the
    /// right way round ([`crate::rooms::portal_between`]).
    pub portals: Vec<crate::rooms::Portal>,
}

/// The linear channel `c` (0..1) as the sRGB byte. glTF's vertex colours are
/// linear (Blender's exporter writes them so), a PD BG's are display-space
/// (measured: Complex's 120 comes back from Blender as 0.1878).
pub fn srgb8(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// A wall this short between two floors is a step (cm): the chrs' 69 cm
/// ground probe climbs it, as PD's.
pub const MAX_RISER: f32 = 60.0;

/// A wall between two floors up to this tall is a ledge a chr climbs (cm):
/// shoulder height.
pub const MAX_LEDGE: f32 = 160.0;

/// The height of the floor polygon `p`'s plane at (x, z), if that is over it
/// (inside every edge in XZ, give or take `tol`).
fn floor_at(p: &[Vec3], x: f32, z: f32, tol: f32) -> Option<f32> {
    let mut n = Vec3::ZERO;
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    if n.y.abs() < 1e-3 {
        return None;
    }
    // Inside every edge (in XZ, the winding's way), give or take `tol`.
    let s = n.y.signum();
    let inside = (0..p.len()).all(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        let e = glam::Vec2::new(b.x - a.x, b.z - a.z);
        let len = e.length().max(1e-6);
        -s * (e.x * (z - a.z) - e.y * (x - a.x)) / len >= -tol
    });
    inside.then(|| p[0].y - (n.x * (x - p[0].x) + n.z * (z - p[0].z)) / n.y)
}

/// Is `v` on the floor polygon `p` (over it within `tol` in XZ, on its plane
/// within `tol`)?
fn on_floor(v: Vec3, p: &[Vec3], tol: f32) -> bool {
    floor_at(p, v.x, v.z, tol).is_some_and(|y| (y - v.y).abs() <= tol)
}

/// Every floor of a level's collision, bucketed by the 2 m cells its box
/// (grown by 1 cm) covers: the floors that can be under or beside a point.
struct Floors<'a> {
    collision: &'a [ColPoly],
    cells: std::collections::HashMap<(i32, i32), Vec<usize>>,
}

impl<'a> Floors<'a> {
    const CELL: f32 = 200.0;

    fn cell(x: f32) -> i32 {
        (x / Self::CELL).floor() as i32
    }

    fn new(collision: &'a [ColPoly]) -> Floors<'a> {
        let floor = pd_core::ids::GEOFLAG_FLOOR1 | pd_core::ids::GEOFLAG_FLOOR2;
        let mut cells: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
        for (i, p) in collision.iter().enumerate().filter(|(_, p)| p.flags & floor != 0) {
            let (lo, hi) = p.verts.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), v| (a.min(*v), b.max(*v)));
            for cx in Self::cell(lo.x - 1.0)..=Self::cell(hi.x + 1.0) {
                for cz in Self::cell(lo.z - 1.0)..=Self::cell(hi.z + 1.0) {
                    cells.entry((cx, cz)).or_default().push(i);
                }
            }
        }
        Floors { collision, cells }
    }

    /// The floors whose boxes come within 1 cm of `v` in XZ.
    fn near(&self, v: Vec3) -> impl Iterator<Item = &'a [Vec3]> + '_ {
        let collision = self.collision;
        self.cells.get(&(Self::cell(v.x), Self::cell(v.z))).into_iter().flatten().map(move |&f| collision[f].verts.as_slice())
    }

    /// Does `v` rest on a floor (on its plane and over it, within 1 cm)?
    fn rests(&self, v: Vec3) -> bool {
        self.near(v).any(|f| on_floor(v, f, 1.0))
    }

    /// The highest floor directly under `p`, at or below its height.
    fn under(&self, p: Vec3) -> Option<f32> {
        self.near(p).filter_map(|f| floor_at(f, p.x, p.z, 0.01)).filter(|&y| y <= p.y + 1.0).fold(None, |best: Option<f32>, y| Some(best.map_or(y, |b| b.max(y))))
    }

    /// Does `p` lie on a floor's edge (within half a centimetre)?
    fn on_edge(&self, p: Vec3) -> bool {
        self.near(p).any(|f| {
            (0..f.len()).any(|i| {
                let (a, b) = (f[i], f[(i + 1) % f.len()]);
                let ab = b - a;
                let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                (a + ab * t).distance(p) < 0.5
            })
        })
    }
}

/// The walls between two floors (every corner resting on a floor: at its
/// edge, or on it where the floor runs on under the wall) whose height is in
/// `range` (cm).
fn walls_between_floors(collision: &[ColPoly], range: std::ops::RangeInclusive<f32>) -> Vec<bool> {
    let floor = pd_core::ids::GEOFLAG_FLOOR1 | pd_core::ids::GEOFLAG_FLOOR2;
    let floors = Floors::new(collision);
    let rests = |v: Vec3| floors.rests(v);
    collision
        .iter()
        .map(|p| {
            let wall = p.flags & pd_core::ids::GEOFLAG_WALL != 0 && p.flags & (floor | pd_core::ids::GEOFLAG_LADDER) == 0;
            let (lo, hi) = p.verts.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v.y), b.max(v.y)));
            wall && range.contains(&(hi - lo)) && p.verts.iter().all(|&v| rests(v))
        })
        .collect()
}

/// The width of the strip of a ledge a simulant climbs it by (cm).
pub const LEDGE_STRIP: f32 = 100.0;

impl LevelSource {
    /// Make the ledges climbable: a wall between two floors, taller than a
    /// step and no taller than [`MAX_LEDGE`], that the source lets you grab.
    /// PD has no ledge climbing, but its ladders are the same move, and it
    /// has a ladder only players climb (`GEOFLAG_LADDER_PLAYERONLY`,
    /// `bondwalk.c:792`): the whole ledge becomes that, so a player climbs it
    /// anywhere, as Link does. A simulant climbs only `GEOFLAG_LADDER`
    /// (`chr.c:622`) and never climbs down one, so a strip
    /// [`LEDGE_STRIP`] wide in the middle of the ledge is a ladder proper: the
    /// waypoint generator links its foot to its top, and the rest of the
    /// ledge stays a drop. Another game's levels are built round climbing up
    /// ledges (Ocarina of Time's are); without this its terraces are one-way.
    /// Returns how many ledges.
    pub fn mark_ledges(&mut self) -> usize {
        use pd_core::ids::{GEOFLAG_LADDER, GEOFLAG_LADDER_PLAYERONLY};
        let ledge = walls_between_floors(&self.collision, (MAX_RISER + 0.01)..=MAX_LEDGE);
        let mut out = Vec::with_capacity(self.collision.len());
        let mut n = 0;
        for (mut p, is) in std::mem::take(&mut self.collision).into_iter().zip(ledge) {
            if !(is && p.grab) {
                out.push(p);
                continue;
            }
            n += 1;
            // Along the ledge: its widest horizontal extent.
            let (mut a, mut b) = (p.verts[0], p.verts[0]);
            for &v in &p.verts {
                for &w in &p.verts {
                    if (v - w).with_y(0.0).length() > (a - b).with_y(0.0).length() {
                        (a, b) = (v, w);
                    }
                }
            }
            let u = (b - a).with_y(0.0).normalize_or_zero();
            let (u0, u1) = p.verts.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v.dot(u)), hi.max(v.dot(u))));
            let mid = (u0 + u1) * 0.5;
            if u1 - u0 <= 2.0 * LEDGE_STRIP || u == Vec3::ZERO {
                p.flags |= GEOFLAG_LADDER;
                out.push(p);
                continue;
            }
            let (s0, s1) = (mid - LEDGE_STRIP * 0.5, mid + LEDGE_STRIP * 0.5);
            let strip = crate::rooms::clip_plane(&crate::rooms::clip_plane(&p.verts, u, s1), -u, -s0);
            for (verts, flag) in [(crate::rooms::clip_plane(&p.verts, u, s0), GEOFLAG_LADDER_PLAYERONLY), (strip, GEOFLAG_LADDER), (crate::rooms::clip_plane(&p.verts, -u, -s1), GEOFLAG_LADDER_PLAYERONLY)] {
                if verts.len() >= 3 {
                    out.push(ColPoly { verts, flags: p.flags | flag, ..p.clone() });
                }
            }
        }
        self.collision = out;
        n
    }

    /// Drop the walls of steps: a wall no taller than [`MAX_RISER`] whose
    /// every corner is a floor's corner. PD's tiles have no risers (a chr or
    /// player climbs a step because its ground probe finds the floor above;
    /// the cylinder ignores what is under its knees, 20-30 cm), but another
    /// game's collision walls every step in, and a riser taller than the
    /// cylinder's bottom would block it. Returns how many were dropped.
    pub fn drop_step_risers(&mut self) -> usize {
        let riser = walls_between_floors(&self.collision, 0.01..=MAX_RISER);
        let before = self.collision.len();
        let mut it = riser.into_iter();
        self.collision.retain(|_| !it.next().unwrap_or(false));
        before - self.collision.len()
    }

    /// Make walls of the low ceilings among `ceilings` (drawn triangles facing
    /// down, which are no tiles): one with some part less than a standing
    /// head ([`LOW_CEILING`]) above the floor beneath it, sampled at its
    /// middle and at its corners pulled a fifth of the way in. PD's designers
    /// flag the ceilings that matter this way and leave the rest out:
    /// Complex's crawl space has its 118 cm ceiling as wall tiles, which is
    /// what refuses standing up in it (`bwalk_update_crouch_offset`) and keeps
    /// a squat a squat. A level made of drawn triangles alone has vents with
    /// nothing over them otherwise. Each ceiling comes with its room (0 where
    /// the level has none of its own). Returns how many.
    pub fn wall_low_ceilings(&mut self, ceilings: &[(u16, [Vec3; 3])]) -> usize {
        use pd_core::ids::{GEOFLAG_BLOCK_SHOOT, GEOFLAG_BLOCK_SIGHT, GEOFLAG_WALL};
        let floors = Floors::new(&self.collision);
        let low: Vec<&(u16, [Vec3; 3])> = ceilings
            .iter()
            .filter(|(_, c)| {
                let mid = (c[0] + c[1] + c[2]) / 3.0;
                std::iter::once(mid).chain(c.iter().map(|&v| v + (mid - v) * 0.2)).any(|p| floors.under(p).is_some_and(|f| p.y - f < LOW_CEILING))
            })
            .collect();
        let n = low.len();
        let walls: Vec<ColPoly> = low
            .into_iter()
            .map(|(room, c)| ColPoly { verts: c.to_vec(), flags: GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT, floortype: pd_core::ids::FLOORTYPE_DEFAULT, grab: false, floorcol: 0, room: *room })
            .collect();
        self.collision.extend(walls);
        n
    }

    /// Flag the ramp walls (`GEOFLAG_RAMPWALL`): PD's designers flag the
    /// triangular walls under a ramp so, and such a wall is tested at its real
    /// height where the player is (`cd_test_ramp_wall`), not by its box, so a
    /// player walks beside a ramp's low end and under an open one. Here an
    /// upright wall triangle is one when its top or bottom slopes: it has a
    /// sloped edge and either no level edge (the strip under a ramp's edge, a
    /// duct's side) or a sloped edge along a floor's edge (the side of a solid
    /// ramp). A plain wall's diagonal (a rectangle split in two) is neither,
    /// so plain walls keep PD's box test. Returns how many.
    pub fn mark_ramp_walls(&mut self) -> usize {
        use pd_core::ids::{GEOFLAG_FLOOR1, GEOFLAG_FLOOR2, GEOFLAG_RAMPWALL, GEOFLAG_WALL};
        let floors = Floors::new(&self.collision);
        let flat = |a: Vec3, b: Vec3| glam::Vec2::new(a.x - b.x, a.z - b.z).length() >= 0.5;
        let level = |a: Vec3, b: Vec3| (a.y - b.y).abs() < 0.5 && flat(a, b);
        let sloped = |a: Vec3, b: Vec3| (a.y - b.y).abs() >= 0.5 && flat(a, b);
        let ramp: Vec<bool> = self
            .collision
            .iter()
            .map(|p| {
                let v = &p.verts;
                if p.flags & GEOFLAG_WALL == 0 || p.flags & (GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2) != 0 || v.len() < 3 {
                    return false;
                }
                let n = (1..v.len() - 1).map(|i| (v[i] - v[0]).cross(v[i + 1] - v[0])).sum::<Vec3>().normalize_or_zero();
                if n == Vec3::ZERO || n.y.abs() > 0.1 {
                    return false;
                }
                let edges = (0..v.len()).map(|i| (v[i], v[(i + 1) % v.len()]));
                if !edges.clone().any(|(a, b)| sloped(a, b)) {
                    return false;
                }
                if !edges.clone().any(|(a, b)| level(a, b)) {
                    return true;
                }
                edges.filter(|&(a, b)| sloped(a, b)).any(|(a, b)| floors.on_edge(a) && floors.on_edge(b) && floors.on_edge((a + b) * 0.5))
            })
            .collect();
        let n = ramp.iter().filter(|r| **r).count();
        for (p, r) in self.collision.iter_mut().zip(ramp) {
            if r {
                p.flags |= GEOFLAG_RAMPWALL;
            }
        }
        n
    }

    /// The box around everything drawn and collided with.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let pts = self.tris.iter().flat_map(|t| t.v.iter().map(|v| v.pos)).chain(self.collision.iter().flat_map(|p| p.verts.iter().copied()));
        pts.fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(p), hi.max(p)))
    }
}
