//! Level plans: rooms as boxes on the grid, before any geometry exists. A port of
//! `ge_pd_tools/plan.py` (`MODULE`, `cells`, `box`, `subtract`, `Ramp`, `Room`, `Link`,
//! `Contact`, `box_contact`, and `Plan` with `room`, `link`, `contacts`, `check`, `graph`).
//!
//! A plan lists rooms (boxes of air, with the solids and ramps inside them) and the links
//! between rooms. Air in different rooms joins with no wall when it is built, so every face two
//! rooms' boxes share is an opening: `contacts` finds them all, and `check` holds them against
//! the links declared, which catches a corridor that opens along its whole side by accident.
//! `graph` counts loops, dead ends and links between floors.
//!
//! A cell is 118.11 units (3 m). Everything a plan holds is in whole units (x east, y north,
//! z up). Not ported: `Spot` (nothing here places any), `footprint`, `to_carver`,
//! `write_portals` and `write_svg`, which belong to the stages that build and draw a plan.

use crate::py;
use std::collections::BTreeSet;

/// One cell of the grid, in game units.
pub const MODULE: f64 = 118.11;

/// The least clear height between a walkable top and an open ramp over or under it, units: under
/// any storey's own (235 or 236: floors 275 or 276 apart, less a deck's slab), and over the 172
/// that `pd_import` makes a wall of (its `low_ceilings` rule). Not the Python's (`Plan::headroom`).
pub const HEADROOM: i32 = 200;

/// (x, y, z) in units.
pub type Point3 = (i32, i32, i32);
/// An axis-aligned box: its low corner and its high corner, in units.
pub type Box3 = (Point3, Point3);

/// Cells (whole or half) as units: Python's `int(round(n * MODULE))`.
pub fn cells(n: f64) -> i32 {
    py::round(n * MODULE) as i32
}

/// An axis-aligned box: x and y in cells, z in units (`plan.box`; `box` is a word Rust keeps).
pub fn box3(x0: f64, y0: f64, x1: f64, y1: f64, z0: i32, z1: i32) -> Box3 {
    ((cells(x0), cells(y0), z0), (cells(x1), cells(y1), z1))
}

/// Box `b` with the `holes` cut out of it, as boxes that don't overlap.
///
/// For a room that pokes into a hall (a lookout pod): the hall's air is its box minus the pod's
/// shell, so the shell stays solid and the pod's own air is carved inside it.
pub fn subtract(b: Box3, holes: &[Box3]) -> Vec<Box3> {
    let mut out = vec![b];
    for h in holes {
        let mut nxt = Vec::new();
        for (lo, hi) in out {
            let ilo = (lo.0.max(h.0 .0), lo.1.max(h.0 .1), lo.2.max(h.0 .2));
            let ihi = (hi.0.min(h.1 .0), hi.1.min(h.1 .1), hi.2.min(h.1 .2));
            if ihi.0 <= ilo.0 || ihi.1 <= ilo.1 || ihi.2 <= ilo.2 {
                nxt.push((lo, hi));
                continue;
            }
            if lo.2 < ilo.2 {
                nxt.push((lo, (hi.0, hi.1, ilo.2)));
            }
            if ihi.2 < hi.2 {
                nxt.push(((lo.0, lo.1, ihi.2), hi));
            }
            let (z0, z1) = (ilo.2, ihi.2);
            if lo.1 < ilo.1 {
                nxt.push(((lo.0, lo.1, z0), (hi.0, ilo.1, z1)));
            }
            if ihi.1 < hi.1 {
                nxt.push(((lo.0, ihi.1, z0), (hi.0, hi.1, z1)));
            }
            if lo.0 < ilo.0 {
                nxt.push(((lo.0, ilo.1, z0), (ilo.0, ihi.1, z1)));
            }
            if ihi.0 < hi.0 {
                nxt.push(((ihi.0, ilo.1, z0), (hi.0, ihi.1, z1)));
            }
        }
        out = nxt;
    }
    out
}

/// A ramp in a plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Ramp {
    /// Centre-line points in units, sharing x or y: the foot, then the top.
    pub start: Point3,
    pub end: Point3,
    pub width: i32,
    /// "open" grate sheet, "enclosed" ramp corridor, "plate" floor ramp, or "duct" (a vent's
    /// sloped run between two of its flat boxes).
    pub kind: &'static str,
    /// Which sides get a hanging edge strip: -1 the low side, 1 the high side.
    pub open_sides: Vec<i32>,
}

/// A railing: its two ends in units, and the way it faces.
pub type Rail = (Point3, Point3, (i32, i32, i32));
/// A box filled back in: its low corner, its high corner and the tag it is built with.
pub type Solid = (Point3, Point3, &'static str);

/// One room of a plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    pub key: String,
    pub name: String,
    /// Its main floor level, used for "links between floor levels".
    pub floor: i32,
    /// Its air.
    pub boxes: Vec<Box3>,
    /// hub, room, corridor, ramp, vent or perch.
    pub kind: &'static str,
    /// The wall scheme it is built in.
    pub scheme: &'static str,
    /// Boxes filled back in, in order.
    pub solids: Vec<Solid>,
    pub ramps: Vec<Ramp>,
    /// Mesh rails.
    pub rails: Vec<Rail>,
    /// Solid tags whose tops players stand on.
    pub walkable: Vec<&'static str>,
    /// What a hub is, in words.
    pub hub: String,
    pub tint: &'static str,
    pub note: String,
    /// Its `Room_NN` number, given by `Plan::room` when 0.
    pub room_id: usize,
}

impl Room {
    /// A room with the Python's defaults: kind "room", scheme "standard", nothing in it.
    pub fn new(key: &str, name: &str, floor: i32, boxes: Vec<Box3>) -> Room {
        Room {
            key: key.to_string(),
            name: name.to_string(),
            floor,
            boxes,
            kind: "room",
            scheme: "standard",
            solids: Vec::new(),
            ramps: Vec::new(),
            rails: Vec::new(),
            walkable: vec!["trim", "grate", "core"],
            hub: String::new(),
            tint: "",
            note: String::new(),
            room_id: 0,
        }
    }

    /// The low corner of the box round all its air (it must have some).
    pub fn lo(&self) -> Point3 {
        let least = |f: fn(&Box3) -> i32| self.boxes.iter().map(f).min().expect("a room has a box");
        (least(|b| b.0 .0), least(|b| b.0 .1), least(|b| b.0 .2))
    }

    /// And its high corner.
    pub fn hi(&self) -> Point3 {
        let most = |f: fn(&Box3) -> i32| self.boxes.iter().map(f).max().expect("a room has a box");
        (most(|b| b.1 .0), most(|b| b.1 .1), most(|b| b.1 .2))
    }

    /// The name of its mesh in a level: `Room_NN`, the number in hex.
    pub fn object_name(&self) -> String {
        format!("Room_{:02X}", self.room_id)
    }
}

/// Two rooms declared to open onto each other.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub a: String,
    pub b: String,
    /// door, ramp, vent, drop, overlook or ladder.
    pub kind: &'static str,
    pub note: String,
}

/// A face two rooms' air shares.
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub a: String,
    pub b: String,
    /// The axis the face is square to: 0 x, 1 y, 2 z.
    pub axis: usize,
    /// The shared rectangle (flat along `axis`).
    pub lo: Point3,
    pub hi: Point3,
}

impl Contact {
    /// The rectangle's two sides.
    pub fn size(&self) -> (i32, i32) {
        let side = |i: usize| at(self.hi, i) - at(self.lo, i);
        match self.axis {
            0 => (side(1), side(2)),
            1 => (side(0), side(2)),
            _ => (side(0), side(1)),
        }
    }
}

/// A volume two rooms' air overlaps in: (the rooms, its low corner, its high corner).
pub type Overlap = (String, String, Point3, Point3);

/// How two boxes meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Touch {
    /// They share a face: (the axis it is square to, the rectangle's low and high corners).
    Face(usize, Point3, Point3),
    /// They overlap in volume: the box they share.
    Overlap(Point3, Point3),
}

/// One coordinate of a point.
pub fn at(p: Point3, i: usize) -> i32 {
    [p.0, p.1, p.2][i]
}

/// A point as Python writes a tuple: `(1, 2, 3)`.
fn tuple(p: Point3) -> String {
    format!("({}, {}, {})", p.0, p.1, p.2)
}

/// The face two boxes share, or the volume they overlap in. None if they are apart, or meet
/// only along an edge or at a corner.
pub fn box_contact(a: Box3, b: Box3) -> Option<Touch> {
    let ((alo, ahi), (blo, bhi)) = (a, b);
    let ov: [i32; 3] = std::array::from_fn(|i| at(ahi, i).min(at(bhi, i)) - at(alo, i).max(at(blo, i)));
    let lo: [i32; 3] = std::array::from_fn(|i| at(alo, i).max(at(blo, i)));
    let hi: [i32; 3] = std::array::from_fn(|i| at(ahi, i).min(at(bhi, i)));
    if ov.iter().all(|&o| o > 0) {
        return Some(Touch::Overlap((lo[0], lo[1], lo[2]), (hi[0], hi[1], hi[2])));
    }
    for axis in 0..3 {
        if ov[axis] != 0 || (0..3).any(|i| i != axis && ov[i] <= 0) {
            continue;
        }
        let v = if at(ahi, axis) == at(blo, axis) { at(ahi, axis) } else { at(alo, axis) };
        let (mut lo, mut hi) = (lo, hi);
        lo[axis] = v;
        hi[axis] = v;
        return Some(Touch::Face(axis, (lo[0], lo[1], lo[2]), (hi[0], hi[1], hi[2])));
    }
    None
}

/// Two rooms' keys, whichever way round they were given (the Python's `frozenset((a, b))`).
fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// What `Plan::graph` counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub rooms: usize,
    /// Pairs of rooms linked (a pair linked twice counts once).
    pub links: usize,
    /// Pieces the plan is in (1: every room reaches every other).
    pub components: usize,
    pub independent_loops: i64,
    /// (links, rooms that have that many), fewest links first.
    pub degree_histogram: Vec<(usize, usize)>,
    /// Rooms with one link, and with none, sorted.
    pub dead_ends: Vec<String>,
    pub isolated: Vec<String>,
    /// Links between rooms whose floors are 100 or more apart, and their share of all links.
    pub links_between_floor_levels: usize,
    pub vertical_share: f64,
    /// (kind, links of that kind), in the order the kinds are first met.
    pub link_kinds: Vec<(&'static str, usize)>,
    /// (floor level, rooms on it), in the order the levels are first met.
    pub floors: Vec<(i32, usize)>,
    /// Every room's links, in the rooms' order.
    pub degree: Vec<(String, usize)>,
}

/// A level's rooms and the links between them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Plan {
    pub name: String,
    /// The rooms, in the order they were added (the Python's dict).
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
}

impl Plan {
    pub fn new(name: &str) -> Plan {
        Plan { name: name.to_string(), rooms: Vec::new(), links: Vec::new() }
    }

    /// The room with this key.
    pub fn get(&self, key: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.key == key)
    }

    // ------------------------------------------------------------ authoring

    /// Add a room. One with no number gets the next. Err if its key is taken.
    pub fn room(&mut self, mut r: Room) -> Result<&Room, String> {
        if self.get(&r.key).is_some() {
            return Err(format!("duplicate room {}", r.key));
        }
        if r.room_id == 0 {
            r.room_id = self.rooms.len() + 1;
        }
        self.rooms.push(r);
        Ok(&self.rooms[self.rooms.len() - 1])
    }

    /// Declare that two rooms open onto each other. Err if either is not in the plan.
    pub fn link(&mut self, a: &str, b: &str, kind: &'static str, note: &str) -> Result<(), String> {
        for k in [a, b] {
            if self.get(k).is_none() {
                return Err(format!("no room {k} to link"));
            }
        }
        self.links.push(Link { a: a.to_string(), b: b.to_string(), kind, note: note.to_string() });
        Ok(())
    }

    // ------------------------------------------------------------ analysis

    /// Every face shared by two rooms' air (an opening, once built), and every volume two
    /// different rooms overlap in (always an error): (rooms a and b, the box they share).
    pub fn contacts(&self) -> (Vec<Contact>, Vec<Overlap>) {
        let (mut out, mut overlaps) = (Vec::new(), Vec::new());
        for (i, ra) in self.rooms.iter().enumerate() {
            for rb in &self.rooms[i + 1..] {
                for a in &ra.boxes {
                    for b in &rb.boxes {
                        match box_contact(*a, *b) {
                            None => {}
                            Some(Touch::Overlap(lo, hi)) => overlaps.push((ra.key.clone(), rb.key.clone(), lo, hi)),
                            Some(Touch::Face(axis, lo, hi)) => out.push(Contact { a: ra.key.clone(), b: rb.key.clone(), axis, lo, hi }),
                        }
                    }
                }
            }
        }
        (out, overlaps)
    }

    /// Problems, in words: overlapping rooms, links declared twice, contacts with no link
    /// declared (accidental openings), links with no contact (ladders aside), and holes between
    /// a floor and a ceiling that no link accounts for.
    pub fn check(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let (contacts, overlaps) = self.contacts();
        for (a, b, lo, hi) in &overlaps {
            problems.push(format!("overlap {a}/{b} at {}..{}", tuple(*lo), tuple(*hi)));
        }
        // each pair of rooms that touch, in the order first met, with its first contact
        let mut touching: Vec<((String, String), &Contact)> = Vec::new();
        let mut touch: BTreeSet<(String, String)> = BTreeSet::new();
        for c in &contacts {
            let p = pair(&c.a, &c.b);
            if touch.insert(p.clone()) {
                touching.push((p, c));
            }
        }
        // and each pair declared, in the order first declared, with how many times
        let mut declared: Vec<((String, String), usize)> = Vec::new();
        for ln in &self.links {
            let p = pair(&ln.a, &ln.b);
            match declared.iter_mut().find(|(q, _)| *q == p) {
                Some((_, n)) => *n += 1,
                None => declared.push((p, 1)),
            }
        }
        let linked: BTreeSet<&(String, String)> = declared.iter().map(|(p, _)| p).collect();
        for (p, n) in &declared {
            if *n > 1 {
                let both = if p.0 == p.1 { vec![&p.0] } else { vec![&p.0, &p.1] };
                problems.push(format!("link declared {n} times: {}", py::repr_list(&both)));
            }
        }
        for (p, c) in &touching {
            if !linked.contains(p) {
                problems.push(format!("undeclared opening {}/{} axis {} at {}..{}", c.a, c.b, c.axis, tuple(c.lo), tuple(c.hi)));
            }
        }
        for ln in &self.links {
            if ln.kind != "ladder" && !touch.contains(&pair(&ln.a, &ln.b)) {
                problems.push(format!("link {}/{} ({}) has no shared face", ln.a, ln.b, ln.kind));
            }
        }
        for c in &contacts {
            if c.axis == 2 && !linked.contains(&pair(&c.a, &c.b)) {
                problems.push(format!("floor/ceiling hole between {} and {}", c.a, c.b));
            }
        }
        problems.extend(self.headroom());
        problems
    }

    /// Not the Python's (a rule of the Rust's own, 2026-11-18): every walkable top (a solid
    /// whose tag is in its room's `walkable`) that an open or floor ramp passes over or under in
    /// plan, with less than `HEADROOM` between the two. A catwalk running on under a ramp's side
    /// was one (`Interior::clear_under_ramps`); this keeps any other from reaching a level.
    fn headroom(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for room in &self.rooms {
            for (i, rp) in room.ramps.iter().enumerate() {
                if rp.kind != "open" && rp.kind != "plate" {
                    continue; // an enclosed ramp or a duct is a room of its own, with its own ceiling
                }
                let (s, e, h) = (rp.start, rp.end, rp.width / 2);
                let (x0, x1, y0, y1) = (s.0.min(e.0), s.0.max(e.0), s.1.min(e.1), s.1.max(e.1));
                let foot = if s.0 == e.0 { (x0 - h, y0, x1 + h, y1) } else { (x0, y0 - h, x1, y1 + h) };
                // the ramp's surface over a point of its run, which goes along x or along y
                let z_at = |x: i32, y: i32| {
                    let t = if s.0 != e.0 {
                        (x - s.0) as f64 / (e.0 - s.0) as f64
                    } else if s.1 != e.1 {
                        (y - s.1) as f64 / (e.1 - s.1) as f64
                    } else {
                        0.0
                    };
                    s.2 as f64 + t.clamp(0.0, 1.0) * (e.2 - s.2) as f64
                };
                for other in &self.rooms {
                    for &(lo, hi, tag) in &other.solids {
                        if !other.walkable.contains(&tag) {
                            continue;
                        }
                        let (ox0, oy0, ox1, oy1) = (foot.0.max(lo.0), foot.1.max(lo.1), foot.2.min(hi.0), foot.3.min(hi.1));
                        if ox0 >= ox1 || oy0 >= oy1 {
                            continue;
                        }
                        let (za, zb) = (z_at(ox0, oy0), z_at(ox1, oy1));
                        let (low, high) = (za.min(zb), za.max(zb));
                        if low >= (hi.2 + HEADROOM) as f64 || high + HEADROOM as f64 <= lo.2 as f64 {
                            continue;
                        }
                        problems.push(format!(
                            "no headroom: {}'s ramp {i} ({:.0}..{:.0}) and {}'s {tag} ({}..{}) at {}..{}",
                            room.key,
                            low,
                            high,
                            other.key,
                            lo.2,
                            hi.2,
                            tuple((ox0, oy0, lo.2)),
                            tuple((ox1, oy1, hi.2))
                        ));
                    }
                }
            }
        }
        problems
    }

    /// The plan as a graph of rooms: how many, in how many pieces, with how many loops.
    pub fn graph(&self) -> Graph {
        // one link for each pair of rooms: the last declared, in the order first declared
        let mut edges: Vec<((String, String), &Link)> = Vec::new();
        for ln in &self.links {
            let p = pair(&ln.a, &ln.b);
            match edges.iter_mut().find(|(q, _)| *q == p) {
                Some((_, last)) => *last = ln,
                None => edges.push((p, ln)),
            }
        }
        let number = |key: &str| self.rooms.iter().position(|r| r.key == key).unwrap_or_else(|| panic!("no room {key}"));
        let n = self.rooms.len();
        let mut deg = vec![0usize; n];
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        for ((a, b), _) in &edges {
            let (a, b) = (number(a), number(b));
            deg[a] += 1;
            if b != a {
                deg[b] += 1;
            }
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            parent[ra] = rb;
        }
        let roots: BTreeSet<usize> = (0..n).map(|k| find(&mut parent, k)).collect();
        let floor = |key: &str| self.rooms[number(key)].floor;
        let vertical = edges.iter().filter(|(_, ln)| (floor(&ln.a) - floor(&ln.b)).abs() >= 100).count();
        let mut histogram: Vec<(usize, usize)> = Vec::new();
        for &d in &deg {
            match histogram.iter_mut().find(|(k, _)| *k == d) {
                Some((_, rooms)) => *rooms += 1,
                None => histogram.push((d, 1)),
            }
        }
        histogram.sort();
        let mut link_kinds: Vec<(&'static str, usize)> = Vec::new();
        for (_, ln) in &edges {
            match link_kinds.iter_mut().find(|(k, _)| *k == ln.kind) {
                Some((_, links)) => *links += 1,
                None => link_kinds.push((ln.kind, 1)),
            }
        }
        let mut floors: Vec<(i32, usize)> = Vec::new();
        for r in &self.rooms {
            match floors.iter_mut().find(|(z, _)| *z == r.floor) {
                Some((_, rooms)) => *rooms += 1,
                None => floors.push((r.floor, 1)),
            }
        }
        let with = |links: usize| {
            let mut keys: Vec<String> = self.rooms.iter().zip(&deg).filter(|(_, d)| **d == links).map(|(r, _)| r.key.clone()).collect();
            keys.sort();
            keys
        };
        Graph {
            rooms: n,
            links: edges.len(),
            components: roots.len(),
            independent_loops: edges.len() as i64 - n as i64 + roots.len() as i64,
            degree_histogram: histogram,
            dead_ends: with(1),
            isolated: with(0),
            links_between_floor_levels: vertical,
            vertical_share: py::round_to(vertical as f64 / edges.len().max(1) as f64, 3),
            link_kinds,
            floors,
            degree: self.rooms.iter().zip(&deg).map(|(r, d)| (r.key.clone(), *d)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(b: &Box3) -> i64 {
        (b.1 .0 - b.0 .0) as i64 * (b.1 .1 - b.0 .1) as i64 * (b.1 .2 - b.0 .2) as i64
    }

    #[test]
    fn cells_are_units_rounded_as_python_rounds() {
        assert_eq!([0.0, 0.5, 1.0, 2.5, 10.0, 61.5].map(cells), [0, 59, 118, 295, 1181, 7264]);
        assert_eq!(box3(1.0, 2.0, 3.5, 4.0, -40, 236), ((118, 236, -40), (413, 472, 236)));
    }

    #[test]
    fn a_hole_leaves_boxes_round_it_that_do_not_overlap() {
        let hall = ((0, 0, 0), (1000, 800, 787));
        let shell = ((300, -50, 236), (600, 300, 551));
        let left = subtract(hall, &[shell]);
        // under it, over it, the far side in y, and each side in x
        assert_eq!(left.len(), 5);
        assert_eq!(left.iter().map(volume).sum::<i64>(), volume(&hall) - 300 * 300 * 315);
        for (i, a) in left.iter().enumerate() {
            for b in &left[i + 1..] {
                let apart = (0..3).any(|k| {
                    let (a0, a1, b0, b1) = match k {
                        0 => (a.0 .0, a.1 .0, b.0 .0, b.1 .0),
                        1 => (a.0 .1, a.1 .1, b.0 .1, b.1 .1),
                        _ => (a.0 .2, a.1 .2, b.0 .2, b.1 .2),
                    };
                    a1 <= b0 || b1 <= a0
                });
                assert!(apart, "{a:?} and {b:?} overlap");
            }
        }
        // a hole that misses leaves the box as it was
        assert_eq!(subtract(hall, &[((2000, 0, 0), (2100, 100, 100))]), [hall]);
    }

    #[test]
    fn a_walkable_top_under_an_open_ramp_needs_headroom() {
        // an open ramp along y in a hall, from 276 at y 472 up to 551 at y 0
        let ramp = Ramp { start: (59, 472, 276), end: (59, 0, 551), width: 118, kind: "open", open_sides: vec![-1, 1] };
        let with = |solid: Solid| {
            let mut hall = Room::new("h", "H", 0, vec![((0, 0, 0), (1000, 1000, 1063))]);
            hall.ramps.push(ramp.clone());
            hall.solids.push(solid);
            let mut p = Plan::new("t");
            p.room(hall).unwrap();
            p.check()
        };
        // a catwalk at 276 running in under the ramp's side: 90 to 158 over it
        let problems = with(((0, 200, 236), (1000, 318, 276), "trim"));
        assert_eq!(problems, ["no headroom: h's ramp 0 (366..434) and h's trim (236..276) at (0, 200, 236)..(118, 318, 276)"]);
        // the same ramp over a floor a storey below the catwalk, or over something nobody stands on
        assert!(with(((0, 200, -40), (1000, 318, 0), "trim")).is_empty());
        assert!(with(((0, 200, 236), (1000, 318, 276), "column")).is_empty());
        // and a deck beside it, not under it
        assert!(with(((118, 200, 236), (1000, 318, 276), "trim")).is_empty());
    }

    /// Two rooms side by side on the ground: `b` against `a`'s east wall.
    fn two_rooms() -> Plan {
        let mut p = Plan::new("t");
        p.room(Room::new("a", "A", 0, vec![((0, 0, 0), (500, 500, 236))])).unwrap();
        p.room(Room::new("b", "B", 0, vec![((500, 100, 0), (900, 300, 236))])).unwrap();
        p
    }

    // (what each of these plans gives is what plan.py gives for the same plan)

    #[test]
    fn boxes_meet_at_a_face_or_overlap_or_neither() {
        let a = ((0, 0, 0), (10, 10, 10));
        assert_eq!(box_contact(a, ((10, 10, 0), (20, 20, 10))), None); // only an edge
        assert_eq!(box_contact(a, ((11, 0, 0), (20, 10, 10))), None);
        assert_eq!(box_contact(a, ((10, 2, 3), (20, 20, 10))), Some(Touch::Face(0, (10, 2, 3), (10, 10, 10))));
        assert_eq!(box_contact(a, ((2, 3, -10), (8, 20, 0))), Some(Touch::Face(2, (2, 3, 0), (8, 10, 0))));
        assert_eq!(box_contact(a, ((5, 5, 5), (20, 20, 20))), Some(Touch::Overlap((5, 5, 5), (10, 10, 10))));
        let r = Room::new("x", "X", 0, vec![((0, 5, 0), (10, 10, 10)), ((-3, 6, 2), (4, 20, 8))]);
        assert_eq!((r.lo(), r.hi()), ((-3, 5, 0), (10, 20, 10)));
        assert_eq!(Contact { a: "a".into(), b: "b".into(), axis: 1, lo: (0, 5, 2), hi: (10, 5, 9) }.size(), (10, 7));
    }

    #[test]
    fn a_contact_nobody_declared_is_reported_and_a_declared_one_passes() {
        let mut p = two_rooms();
        assert_eq!(p.check(), ["undeclared opening a/b axis 0 at (500, 100, 0)..(500, 300, 236)"]);
        p.link("a", "b", "door", "").unwrap();
        assert!(p.check().is_empty());
        assert_eq!((p.get("b").unwrap().room_id, p.get("b").unwrap().object_name()), (2, "Room_02".to_string()));
        // the same pair again, the other way round, is the same link twice
        p.link("b", "a", "ramp", "").unwrap();
        assert_eq!(p.check(), ["link declared 2 times: ['a', 'b']"]);
        assert_eq!(p.link("a", "nowhere", "door", "").unwrap_err(), "no room nowhere to link");
        assert_eq!(p.room(Room::new("a", "A", 0, Vec::new())).unwrap_err(), "duplicate room a");
    }

    #[test]
    fn rooms_that_overlap_are_reported() {
        let mut p = two_rooms();
        p.room(Room::new("c", "C", 0, vec![((400, 400, 100), (700, 700, 300))])).unwrap();
        assert_eq!(p.check(), ["overlap a/c at (400, 400, 100)..(500, 500, 236)", "undeclared opening a/b axis 0 at (500, 100, 0)..(500, 300, 236)"]);
    }

    #[test]
    fn a_link_needs_a_shared_face_unless_it_is_a_ladder() {
        let mut p = two_rooms();
        p.link("a", "b", "door", "").unwrap();
        p.room(Room::new("d", "D", 276, vec![((2000, 0, 276), (2400, 400, 512))])).unwrap();
        p.link("a", "d", "door", "").unwrap();
        p.link("b", "d", "ladder", "").unwrap();
        assert_eq!(p.check(), ["link a/d (door) has no shared face"]);
    }

    #[test]
    fn a_hole_in_a_floor_is_reported_until_it_is_linked_and_the_graph_counts() {
        let mut p = two_rooms();
        p.link("a", "b", "door", "").unwrap();
        p.room(Room::new("e", "E", 276, vec![((100, 100, 236), (300, 300, 600)), ((600, 100, 236), (700, 200, 400))])).unwrap();
        assert_eq!(
            p.check(),
            [
                "undeclared opening a/e axis 2 at (100, 100, 236)..(300, 300, 236)",
                "undeclared opening b/e axis 2 at (600, 100, 236)..(700, 200, 236)",
                "floor/ceiling hole between a and e",
                "floor/ceiling hole between b and e"
            ]
        );
        p.link("e", "a", "drop", "").unwrap();
        assert_eq!(p.check(), ["undeclared opening b/e axis 2 at (600, 100, 236)..(700, 200, 236)", "floor/ceiling hole between b and e"]);
        let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let degree = |v: &[(&str, usize)]| v.iter().map(|(k, d)| (k.to_string(), *d)).collect::<Vec<_>>();
        let g = p.graph();
        assert_eq!((g.rooms, g.links, g.components, g.independent_loops), (3, 2, 1, 0));
        assert_eq!((g.degree_histogram, g.dead_ends, g.isolated), (vec![(1, 2), (2, 1)], own(&["b", "e"]), own(&[])));
        assert_eq!((g.links_between_floor_levels, g.vertical_share), (1, 0.5));
        assert_eq!((g.link_kinds, g.floors, g.degree), (vec![("door", 1), ("drop", 1)], vec![(0, 2), (276, 1)], degree(&[("a", 2), ("b", 1), ("e", 1)])));
        // a room on its own, and a pair linked twice: the pair counts once, as its last link
        p.room(Room::new("f", "F", -276, vec![((5000, 0, -276), (5100, 100, -40))])).unwrap();
        p.link("e", "b", "overlook", "").unwrap();
        p.link("b", "e", "drop", "").unwrap();
        let g = p.graph();
        assert_eq!((g.rooms, g.links, g.components, g.independent_loops), (4, 3, 2, 1));
        assert_eq!((g.degree_histogram, g.dead_ends, g.isolated), (vec![(0, 1), (2, 3)], own(&[]), own(&["f"])));
        assert_eq!((g.links_between_floor_levels, g.vertical_share), (2, 0.667));
        assert_eq!((g.link_kinds, g.floors), (vec![("door", 1), ("drop", 2)], vec![(0, 2), (276, 1), (-276, 1)]));
        assert_eq!(g.degree, degree(&[("a", 2), ("b", 2), ("e", 2), ("f", 0)]));
    }
}
