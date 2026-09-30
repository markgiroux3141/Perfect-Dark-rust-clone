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

/// Is `v` on the floor polygon `p` (over it within `tol` in XZ, on its plane
/// within `tol`)?
fn on_floor(v: Vec3, p: &[Vec3], tol: f32) -> bool {
    let mut n = Vec3::ZERO;
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    if n.y.abs() < 1e-3 {
        return false;
    }
    let y = p[0].y - (n.x * (v.x - p[0].x) + n.z * (v.z - p[0].z)) / n.y;
    if (y - v.y).abs() > tol {
        return false;
    }
    // Inside every edge (in XZ, the winding's way), give or take `tol`.
    let s = n.y.signum();
    (0..p.len()).all(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        let e = glam::Vec2::new(b.x - a.x, b.z - a.z);
        let len = e.length().max(1e-6);
        -s * (e.x * (v.z - a.z) - e.y * (v.x - a.x)) / len >= -tol
    })
}

/// The walls between two floors (every corner resting on a floor: at its
/// edge, or on it where the floor runs on under the wall) whose height is in
/// `range` (cm).
fn walls_between_floors(collision: &[ColPoly], range: std::ops::RangeInclusive<f32>) -> Vec<bool> {
    let floor = pd_core::ids::GEOFLAG_FLOOR1 | pd_core::ids::GEOFLAG_FLOOR2;
    // Every floor, bucketed by the 2 m cells its box covers.
    const CELL: f32 = 200.0;
    let cell = |x: f32| (x / CELL).floor() as i32;
    let mut floors: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
    for (i, p) in collision.iter().enumerate().filter(|(_, p)| p.flags & floor != 0) {
        let (lo, hi) = p.verts.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), v| (a.min(*v), b.max(*v)));
        for cx in cell(lo.x - 1.0)..=cell(hi.x + 1.0) {
            for cz in cell(lo.z - 1.0)..=cell(hi.z + 1.0) {
                floors.entry((cx, cz)).or_default().push(i);
            }
        }
    }
    let rests = |v: Vec3| floors.get(&(cell(v.x), cell(v.z))).is_some_and(|fs| fs.iter().any(|&f| on_floor(v, &collision[f].verts, 1.0)));
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

    /// The box around everything drawn and collided with.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let pts = self.tris.iter().flat_map(|t| t.v.iter().map(|v| v.pos)).chain(self.collision.iter().flat_map(|p| p.verts.iter().copied()));
        pts.fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(p), hi.max(p)))
    }
}
