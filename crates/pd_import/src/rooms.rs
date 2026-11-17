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
//!
//! A level that has rooms PD can use brings them
//! ([`crate::source::OwnRooms`]: a generated level's rooms and doorways), and
//! then nothing is split or cut: [`Partition::of`] is those rooms, each
//! polygon in the room it names, the portals the level's own openings
//! ([`portal_between`]).

use glam::{Vec2, Vec3};

use crate::source::{LevelSource, OwnRooms, Vert};

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
    /// The source's own rooms: no tree, a polygon says its room.
    Own,
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
    /// A source's own rooms, in place of the split.
    own: Option<OwnRooms>,
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
    /// The level's rooms: its own if it brings them, else its footprint split
    /// into about `rooms`.
    pub fn of(src: &LevelSource, rooms: usize) -> Partition {
        match &src.rooms {
            Some(own) => {
                let rects = own.boxes.iter().map(|(lo, hi)| Rect { lo: Vec2::new(lo.x, lo.z), hi: Vec2::new(hi.x, hi.z) }).collect();
                let (ylo, yhi) = own.boxes.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), (lo, hi)| (a.min(lo.y), b.max(hi.y)));
                Partition { rects, ylo, yhi, tree: Node::Own, own: Some(own.clone()) }
            }
            None => Partition::build(src, rooms),
        }
    }

    /// Are these the source's own rooms (nothing cut)?
    pub fn is_own(&self) -> bool {
        self.own.is_some()
    }

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
        Partition { rects, ylo: lo.y - 200.0, yhi: hi.y + 1000.0, tree, own: None }
    }

    pub fn rooms(&self) -> usize {
        self.rects.len()
    }

    /// Room `room`'s box.
    pub fn bbox(&self, room: u16) -> (Vec3, Vec3) {
        if let Some(own) = &self.own {
            return own.boxes[room as usize - 1];
        }
        let r = &self.rects[room as usize - 1];
        (Vec3::new(r.lo.x, self.ylo, r.lo.y), Vec3::new(r.hi.x, self.yhi, r.hi.y))
    }

    /// The room whose footprint holds `p` (of a source's own rooms, the
    /// first whose box does; 1 if none).
    pub fn room_at(&self, p: Vec3) -> u16 {
        let mut n = &self.tree;
        loop {
            match n {
                Node::Own => {
                    let boxes = self.own.as_ref().map_or(&[][..], |o| &o.boxes[..]);
                    return boxes.iter().position(|(lo, hi)| p.cmpge(*lo).all() && p.cmple(*hi).all()).map_or(1, |i| i as u16 + 1);
                }
                Node::Leaf(i) => return *i as u16 + 1,
                Node::Split { axis, at, lo, hi } => n = if xz(p, *axis) <= *at { lo } else { hi },
            }
        }
    }

    /// A convex polygon cut by every room side it crosses: each piece and its
    /// room. Slivers (under a square centimetre) are dropped. With a source's
    /// own rooms the polygon is whole, in `room` (the one it names).
    pub fn cut<V: Cut>(&self, room: u16, poly: Vec<V>) -> Vec<(u16, Vec<V>)> {
        if self.own.is_some() {
            return if (1..=self.rects.len()).contains(&(room as usize)) { vec![(room, poly)] } else { Vec::new() };
        }
        let mut out = Vec::new();
        cut_node(&self.tree, poly, &mut out);
        out.retain(|(_, p)| p.len() >= 3 && area(p) >= 1.0);
        out
    }

    /// Every rectangle two rooms share, as a portal from `ylo` to `yhi`; a
    /// source's own portals.
    pub fn portals(&self) -> Vec<Portal> {
        if let Some(own) = &self.own {
            return own.portals.clone();
        }
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
        Node::Own => {}
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

/// The opening `rect` (its corners in order round it, flat) between two
/// rooms, as a portal PD's `bg_init_portal` (`bg.c:6020`) takes the right way
/// round: the corners are counter-clockwise seen from `front`'s side, and
/// `behind` is the room on the other. PD's portal faces `room2` along the winding's
/// normal negated (`g_PortalMetrics`, `bg.c:1818`), and swaps the file's two
/// rooms only when the first's centre (its box's middle) is in front of the
/// portal and the second's is not: so the rooms are written `[behind,
/// front]`, wound to face `front`, which PD keeps unless both rooms' boxes
/// have their middles on the wrong sides of their own opening. Err then,
/// naming the rooms: nothing written can make PD take such a portal right
/// (its rooms' boxes must change).
pub fn portal_between(behind: u16, front: u16, rect: [Vec3; 4], boxes: &[(Vec3, Vec3)]) -> Result<Portal, String> {
    // Newell's normal: towards the side the corners are counter-clockwise from.
    let mut n = Vec3::ZERO;
    for i in 0..4 {
        let (a, b) = (rect[i], rect[(i + 1) % 4]);
        n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    // PD's is that negated: it faces where the corners are clockwise from,
    // which must be `front`, so the corners are written the other way round.
    let mut verts = rect;
    verts.reverse();
    let normal = n.normalize_or_zero();
    if normal == Vec3::ZERO {
        return Err(format!("the opening between rooms {behind:#x} and {front:#x} at {} has no area", rect[0]));
    }
    let centre = |room: u16| boxes.get(room as usize - 1).map(|(lo, hi)| (*lo + *hi) * 0.5);
    let (Some(cb), Some(cf)) = (centre(behind), centre(front)) else {
        return Err(format!("the opening at {} names a room the level has not ({behind:#x}, {front:#x} of {})", rect[0], boxes.len()));
    };
    let d = normal.dot(rect[0]);
    // bg_init_portal: swapped when room1's centre is in front and room2's is not.
    if normal.dot(cb) > d && normal.dot(cf) < d {
        return Err(format!("the opening between rooms {behind:#x} and {front:#x} at {}: both rooms' boxes have their middles on the far side of it, and PD would take the portal the wrong way round", rect[0]));
    }
    Ok(Portal { rooms: [behind, front], verts })
}

/// A polygon's area (cm²).
fn area<V: Cut>(p: &[V]) -> f32 {
    let mut n = Vec3::ZERO;
    for i in 1..p.len() - 1 {
        n += (p[i].pos() - p[0].pos()).cross(p[i + 1].pos() - p[0].pos());
    }
    n.length() * 0.5
}
