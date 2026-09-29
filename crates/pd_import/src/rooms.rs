//! Rooms and portals for a level that has none PD could use. PD's game runs on
//! rooms: the portal pass draws only the rooms a player can see, the lighting
//! and the King of the Hill hill are per room, a chr's rooms decide which
//! spawn pads are safe and whether a simulant is near its target. An outdoor
//! level from another game has a handful of huge ones, if any.
//!
//! So the level's footprint is split into boxes: a k-d tree over its floor
//! area, each cut at the median of the floor on either side, until every box
//! holds about `area / rooms` of floor. Each box is a room from below the
//! lowest point to above the highest; every polygon (drawn or collision) is
//! cut where it crosses a box's side, so each piece lies in one room; and
//! wherever two boxes touch, the shared rectangle is a portal between them.
//! Because the boxes tile the footprint and their portals cover every shared
//! side, a line of sight from one room to another always crosses portals, and
//! the portal pass finds every room a player can see.

use glam::{Vec2, Vec3};

use crate::source::{LevelSource, Vert};

/// A room's footprint: `lo`..`hi` in (x, z).
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub lo: Vec2,
    pub hi: Vec2,
}

impl Rect {
    pub fn centre(&self) -> Vec2 {
        (self.lo + self.hi) * 0.5
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.lo.x && p.x <= self.hi.x && p.y >= self.lo.y && p.y <= self.hi.y
    }
}

enum Node {
    Leaf(usize),
    /// Cut at `at` on `axis` (0: x, 1: z); `lo` holds what is below it.
    Split { axis: usize, at: f32, lo: Box<Node>, hi: Box<Node> },
}

/// A portal: the rectangle two rooms share.
#[derive(Clone, Debug)]
pub struct Portal {
    pub rooms: [u16; 2],
    pub verts: [Vec3; 4],
}

pub struct Partition {
    /// Room `i + 1`'s footprint.
    pub rects: Vec<Rect>,
    /// Every room's height range.
    pub ylo: f32,
    pub yhi: f32,
    tree: Node,
}

/// Something a polygon is made of that can be cut: a position, lerped.
pub trait Cut: Copy {
    fn pos(&self) -> Vec3;
    fn with_pos(&self, p: Vec3) -> Self;
    fn lerp_to(&self, b: &Self, t: f32) -> Self;
}

impl Cut for Vec3 {
    fn pos(&self) -> Vec3 {
        *self
    }
    fn with_pos(&self, p: Vec3) -> Self {
        p
    }
    fn lerp_to(&self, b: &Self, t: f32) -> Self {
        self.lerp(*b, t)
    }
}

impl Cut for Vert {
    fn pos(&self) -> Vec3 {
        self.pos
    }
    fn with_pos(&self, p: Vec3) -> Self {
        Vert { pos: p, ..*self }
    }
    fn lerp_to(&self, b: &Self, t: f32) -> Self {
        self.lerp(b, t)
    }
}

/// Coordinates this close to a cut count as on it (cm).
const EPS: f32 = 0.5;

/// A room's footprint is never cut narrower than this (cm).
const MIN_SIDE: f32 = 600.0;

fn xz(p: Vec3, axis: usize) -> f32 {
    if axis == 0 {
        p.x
    } else {
        p.z
    }
}

impl Partition {
    /// Split `src`'s footprint into about `rooms` rooms by its floor area.
    pub fn build(src: &LevelSource, rooms: usize) -> Partition {
        let (lo, hi) = src.bounds();
        let floors: Vec<(Vec2, f32)> = src
            .collision
            .iter()
            .filter(|p| p.flags & (pd_core::ids::GEOFLAG_FLOOR1 | pd_core::ids::GEOFLAG_FLOOR2) != 0)
            .filter_map(|p| {
                let v = &p.verts;
                let mut area = 0.0;
                for i in 1..v.len() - 1 {
                    let (a, b) = (v[i] - v[0], v[i + 1] - v[0]);
                    area += (a.x * b.z - a.z * b.x).abs() * 0.5;
                }
                let c = v.iter().copied().sum::<Vec3>() / v.len() as f32;
                (area > 0.0).then_some((Vec2::new(c.x, c.z), area))
            })
            .collect();
        let total: f32 = floors.iter().map(|f| f.1).sum();
        let target = total / rooms.max(1) as f32;
        let mut rects = Vec::new();
        let tree = split(Rect { lo: Vec2::new(lo.x, lo.z), hi: Vec2::new(hi.x, hi.z) }, floors, target, &mut rects);
        Partition { rects, ylo: lo.y - 200.0, yhi: hi.y + 1000.0, tree }
    }

    pub fn rooms(&self) -> usize {
        self.rects.len()
    }

    /// Room `room`'s box.
    pub fn bbox(&self, room: u16) -> (Vec3, Vec3) {
        let r = &self.rects[room as usize - 1];
        (Vec3::new(r.lo.x, self.ylo, r.lo.y), Vec3::new(r.hi.x, self.yhi, r.hi.y))
    }

    /// The room whose footprint holds `p`.
    pub fn room_at(&self, p: Vec3) -> u16 {
        let mut n = &self.tree;
        loop {
            match n {
                Node::Leaf(i) => return *i as u16 + 1,
                Node::Split { axis, at, lo, hi } => n = if xz(p, *axis) <= *at { lo } else { hi },
            }
        }
    }

    /// A convex polygon cut by every room side it crosses: each piece and its
    /// room. Slivers (under a square centimetre) are dropped.
    pub fn cut<V: Cut>(&self, poly: Vec<V>) -> Vec<(u16, Vec<V>)> {
        let mut out = Vec::new();
        cut_node(&self.tree, poly, &mut out);
        out.retain(|(_, p)| p.len() >= 3 && area(p) >= 1.0);
        out
    }

    /// Every rectangle two rooms share, as a portal from `ylo` to `yhi`.
    pub fn portals(&self) -> Vec<Portal> {
        let mut out = Vec::new();
        let n = self.rects.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&self.rects[i], &self.rects[j]);
                for axis in 0..2 {
                    let other = 1 - axis;
                    let at = if (a.hi[axis] - b.lo[axis]).abs() < 0.01 {
                        a.hi[axis]
                    } else if (b.hi[axis] - a.lo[axis]).abs() < 0.01 {
                        b.hi[axis]
                    } else {
                        continue;
                    };
                    let (s0, s1) = (a.lo[other].max(b.lo[other]), a.hi[other].min(b.hi[other]));
                    if s1 - s0 < 1.0 {
                        continue;
                    }
                    let pt = |s: f32, y: f32| if axis == 0 { Vec3::new(at, y, s) } else { Vec3::new(s, y, at) };
                    out.push(Portal { rooms: [i as u16 + 1, j as u16 + 1], verts: [pt(s0, self.ylo), pt(s1, self.ylo), pt(s1, self.yhi), pt(s0, self.yhi)] });
                }
            }
        }
        out
    }
}

fn split(rect: Rect, floors: Vec<(Vec2, f32)>, target: f32, rects: &mut Vec<Rect>) -> Node {
    let area: f32 = floors.iter().map(|f| f.1).sum();
    let size = rect.hi - rect.lo;
    let can = |axis: usize| size[axis] >= 2.0 * MIN_SIDE;
    if area <= target * 1.4 || (!can(0) && !can(1)) {
        rects.push(rect);
        return Node::Leaf(rects.len() - 1);
    }
    let axis = if (size.x >= size.y && can(0)) || !can(1) { 0 } else { 1 };
    let mut sorted = floors;
    sorted.sort_by(|a, b| a.0[axis].total_cmp(&b.0[axis]));
    let mut acc = 0.0;
    let mut at = rect.centre()[axis];
    for f in &sorted {
        acc += f.1;
        if acc >= area * 0.5 {
            at = f.0[axis];
            break;
        }
    }
    let at = at.clamp(rect.lo[axis] + MIN_SIDE, rect.hi[axis] - MIN_SIDE).round();
    let (lof, hif): (Vec<_>, Vec<_>) = sorted.into_iter().partition(|f| f.0[axis] <= at);
    let mut lo_rect = rect;
    lo_rect.hi[axis] = at;
    let mut hi_rect = rect;
    hi_rect.lo[axis] = at;
    let lo = split(lo_rect, lof, target, rects);
    let hi = split(hi_rect, hif, target, rects);
    Node::Split { axis, at, lo: Box::new(lo), hi: Box::new(hi) }
}

fn cut_node<V: Cut>(n: &Node, poly: Vec<V>, out: &mut Vec<(u16, Vec<V>)>) {
    match n {
        Node::Leaf(i) => out.push((*i as u16 + 1, poly)),
        Node::Split { axis, at, lo, hi } => {
            let (min, max) = poly.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(xz(v.pos(), *axis)), b.max(xz(v.pos(), *axis))));
            if max <= at + EPS {
                cut_node(lo, poly, out);
            } else if min >= at - EPS {
                cut_node(hi, poly, out);
            } else {
                let n = if *axis == 0 { Vec3::X } else { Vec3::Z };
                let below = clip_plane(&poly, n, *at);
                let above = clip_plane(&poly, -n, -*at);
                if below.len() >= 3 {
                    cut_node(lo, below, out);
                }
                if above.len() >= 3 {
                    cut_node(hi, above, out);
                }
            }
        }
    }
}

/// Sutherland–Hodgman against one plane: the part of a convex polygon with
/// `n · p <= d`. A new vertex is put exactly on the plane (exactly on an
/// axis-aligned one, whatever the rounding).
pub fn clip_plane<V: Cut>(poly: &[V], n: Vec3, d: f32) -> Vec<V> {
    let side = |v: &V| n.dot(v.pos()) - d;
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (a, b) = (&poly[i], &poly[(i + 1) % poly.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa <= 0.0 {
            out.push(*a);
        }
        if (sa <= 0.0) != (sb <= 0.0) {
            let v = a.lerp_to(b, sa / (sa - sb));
            let mut p = v.pos() - n * (n.dot(v.pos()) - d);
            for k in 0..3 {
                if n[k].abs() == 1.0 {
                    p[k] = d * n[k];
                }
            }
            out.push(v.with_pos(p));
        }
    }
    out
}

/// A polygon's area (cm²).
fn area<V: Cut>(p: &[V]) -> f32 {
    let mut n = Vec3::ZERO;
    for i in 1..p.len() - 1 {
        n += (p[i].pos() - p[0].pos()).cross(p[i + 1].pos() - p[0].pos());
    }
    n.length() * 0.5
}
