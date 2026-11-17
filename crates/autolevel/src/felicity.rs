//! Felicity-style levels from a seed: corridors first, with rooms hung off them and a big tall
//! hall where the corridors end. Not a port of anything; the numbers are Felicity's own
//! (`assets/stages/mp11`, measured 2026-11-17: docs/AUTOLEVEL.md, "Felicity").
//!
//! ```text
//! let lv = felicity::generate(7, &Options { style: "felicity", ..Options::default() })?;
//! lv.plan                 // rooms (air boxes, steps as solids) and the links between them
//! lv.doorways             // every doorway, and the door in it if it has one
//! lv.walks, lv.views      // routes for the walker, places to look from
//! ```
//!
//! The first planner cut a building into rooms off one hallway and off each other, and it played
//! as a maze (rooms up to five doorways deep). This one lays the **circulation** first, the
//! corridors, the hall and the stair rooms, all reaching each other without passing through a
//! room, and then hangs the rooms off it (`circulation::TARGETS`, which every level it returns
//! meets):
//!
//! - **Two long corridors** side by side, 3 or 4 cells wide (a cell is 106, a door's width), both
//!   ending on **the hall** (Felicity's hall, 7.8 m tall: a platform 70 up along its far wall,
//!   three steps up to it, pillars). **Cross corridors** join the two: one at their far ends (a
//!   ring), one a little way along (an H; one of the long corridors may then start at it, a
//!   bend), or both (a ladder).
//! - **A gallery** upstairs (283 up: rooms are 259 tall, the slab 24) over a stretch of one long
//!   corridor, with a **stair room** towards each end, so the upper storey is a way through, not
//!   a dead end. A stair room is sized for its stairs (`stairs::fit`: a straight flight, an L or
//!   a U, twelve risers) and stands beside the gallery, across the band between the two
//!   corridors (a flight from one corridor to the gallery over the other), or in line where the
//!   corridor under the gallery stops.
//! - **Rooms** in a row along each side of every long corridor and of the gallery, each with a
//!   doorway off its corridor; the rooms between the two corridors often open on both. A few
//!   rooms off the hall, a cluster of small washrooms, and a few rooms behind rooms (two doorways
//!   deep, never more). Rooms with one way in get a doorway onto another corridor or into a
//!   neighbour until at most about a third have only one.
//! - **Room types** by where a room is: `hallside` beside the hall, `industrial` for one cross
//!   corridor and the rooms beside it, `washroom` for the cluster, `marble` for the big rooms and
//!   one long corridor, `office` for the rest.
//! - **Doorways 106 wide and 212 tall** (212 wide for a double), with a lintel, in walls 40
//!   thick. **Doors** where Felicity puts them: on 9 of 11 doorways from circulation into a
//!   room, 4 of 12 between two rooms, none into a small room, none between corridors or onto
//!   stairs, on half of the hall's own; double doorways take two leaves; swinging to sliding
//!   about 2:1.
//!
//! The level is drawn up one way round (the hall to the east, the gallery over the northern
//! corridor) and then turned and mirrored by the seed. The layout and the doors draw from two
//! random streams (`<seed>/felicity` and `<seed>/felicity/doors`), so a change to how doors are
//! chosen never moves a room. Units are the plan's (x east, y north, z up; 1 unit = 1 cm at the
//! importer's scale 1).

use crate::cells::Cells;
use crate::circulation::{self, TARGETS};
use crate::level::{DoorKind, Doorway, Level, Options, Step, View};
use crate::network::Network;
use crate::plan::{Box3, Plan, Room, MODULE};
use crate::pyrand::PyRandom;
use crate::report::Report;
use crate::route::Grid;
use crate::space::Space;
use crate::stairs::{self as stair, Frame, Shape, Stairs};

/// The grid: a door's width.
pub const CELL: i32 = 106;
/// Walls' thickness, centred on the grid lines.
pub const WALL: i32 = 40;
/// A room's floor to its ceiling.
pub const ROOM_H: i32 = 259;
/// Floor to floor.
pub const STOREY: i32 = 283;
/// A doorway's height (a door's).
pub const DOOR_H: i32 = 212;
/// The stairs: risers in a storey, and each tread's depth.
pub const RISERS: i32 = 12;
pub const GOING: i32 = 40;
/// The big hall's height, and its platform's.
pub const HALL_H: i32 = 780;
pub const PLATFORM: i32 = 70;
/// A room this small (cells of floor) has no door (Felicity's tiny rooms have none).
pub const SMALL: i32 = 12;
/// A room this big (cells of floor) is a marble room.
pub const BIG: i32 = 40;
/// How long a stair room in line with its corridor is, in cells (a U: `stairs::fit`).
const END: i32 = 7;
/// How far inside a room a walk starts and ends from a doorway's middle plane.
const INSIDE: i32 = 100;
/// How many layouts a seed may draw before one meets the targets.
const TRIES: usize = 40;

/// What a lot is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Corridor,
    Room,
    Hall,
    /// A stair room (its stairs: `stairs::fit`).
    Stair,
}

/// Cells [x0, x1) x [y0, y1).
type Rect = (i32, i32, i32, i32);

/// A room on the grid: cells [x0, x1) x [y0, y1), its floor and its ceiling (units).
#[derive(Clone, Debug)]
struct Lot {
    key: String,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    floor: i32,
    top: i32,
    kind: Kind,
    scheme: &'static str,
}

impl Lot {
    fn area(&self) -> i32 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }

    /// Its air, inside its walls.
    fn air(&self) -> Box3 {
        let h = WALL / 2;
        ((self.x0 * CELL + h, self.y0 * CELL + h, self.floor), (self.x1 * CELL - h, self.y1 * CELL - h, self.top))
    }

    fn small(&self) -> bool {
        self.kind == Kind::Room && self.area() < SMALL
    }
}

/// A doorway planned on the grid: in the wall on grid line `line` (square to `axis`), from cell
/// `k` for `wide` cells along it, on the storey at `z`; `a` on the low side, `b` the high.
#[derive(Clone, Debug)]
struct Gap {
    a: usize,
    b: usize,
    axis: usize,
    line: i32,
    k: i32,
    wide: i32,
    z: i32,
}

/// A room to be, off the side of a corridor (or of the hall): cells [x0, x1) along it, from its
/// wall at `y` outwards (`out`: 1 north, -1 south) for `depth` cells.
#[derive(Clone, Copy, Debug)]
struct Front {
    x0: i32,
    x1: i32,
    y: i32,
    out: i32,
    depth: i32,
    floor: i32,
    /// The lot it opens off.
    on: usize,
    wash: bool,
}

impl Front {
    fn rect(&self) -> Rect {
        band(self.x0, self.x1, self.y, self.out, self.depth)
    }
}

/// Cells [x0, x1) from the line `y` outwards for `depth`.
fn band(x0: i32, x1: i32, y: i32, out: i32, depth: i32) -> Rect {
    if out > 0 {
        (x0, y, x1, y + depth)
    } else {
        (x0, y - depth, x1, y)
    }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// Widths in [lo, hi] cells summing to `len`, at random (`len` at least `lo`, `hi` at least
/// twice `lo` less one).
fn cuts(rng: &mut PyRandom, len: i32, lo: i32, hi: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let mut left = len;
    while left > hi {
        let w = rng.randint(lo as i64, hi.min(left - lo) as i64) as i32;
        out.push(w);
        left -= w;
    }
    out.push(left);
    out
}

/// What is left of [x0, x1) once the `taken` stretches are out of it, broken at each of `breaks`.
fn stretches(x0: i32, x1: i32, taken: &[(i32, i32)], breaks: &[i32]) -> Vec<(i32, i32)> {
    let mut taken = taken.to_vec();
    taken.sort();
    let mut free = Vec::new();
    let mut x = x0;
    for (t0, t1) in taken {
        let (t0, t1) = (t0.max(x0), t1.min(x1));
        if t0 >= t1 {
            continue;
        }
        if t0 > x {
            free.push((x, t0));
        }
        x = x.max(t1);
    }
    if x < x1 {
        free.push((x, x1));
    }
    let mut breaks = breaks.to_vec();
    breaks.sort();
    let mut out = Vec::new();
    for (f0, f1) in free {
        let mut s = f0;
        for &b in breaks.iter().filter(|&&b| f0 < b && b < f1) {
            if b > s {
                out.push((s, b));
                s = b;
            }
        }
        out.push((s, f1));
    }
    out
}

/// The rooms along each stretch, as (x0, x1): `lo` to `hi` cells wide; a stretch narrower than
/// `lo` is one room, and one under three cells is left solid.
fn rooms_along(rng: &mut PyRandom, free: &[(i32, i32)], lo: i32, hi: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for &(s0, s1) in free {
        let len = s1 - s0;
        if len < 3 {
            continue;
        }
        let mut x = s0;
        for w in if len < lo { vec![len] } else { cuts(rng, len, lo, hi) } {
            out.push((x, x + w));
            x += w;
        }
    }
    out
}

/// Where two lots' walls meet: (axis, the grid line, the span along it in cells), `i` on the
/// low side. None if they don't share a wall.
fn shared(i: &Lot, j: &Lot) -> Option<(usize, i32, i32, i32)> {
    let span = |a0: i32, a1: i32, b0: i32, b1: i32| (a0.max(b0), a1.min(b1));
    if i.x1 == j.x0 {
        let (s0, s1) = span(i.y0, i.y1, j.y0, j.y1);
        return (s1 > s0).then_some((0, i.x1, s0, s1));
    }
    if i.y1 == j.y0 {
        let (s0, s1) = span(i.x0, i.x1, j.x0, j.x1);
        return (s1 > s0).then_some((1, i.y1, s0, s1));
    }
    None
}

/// The wall of `l` that `c` is behind (E, N, W, S) and the span they share along it, in cells.
fn side(l: &Lot, c: &Lot) -> Option<(char, i32, i32)> {
    if let Some((axis, _, s0, s1)) = shared(l, c) {
        return Some((if axis == 0 { 'E' } else { 'N' }, s0, s1));
    }
    shared(c, l).map(|(axis, _, s0, s1)| (if axis == 0 { 'W' } else { 'S' }, s0, s1))
}

/// The storey a doorway between two lots is on: their floor if they share one, the other's
/// floor for a stair room (it stands on both). None if they can't be joined.
fn storey(a: &Lot, b: &Lot) -> Option<i32> {
    match (a.kind, b.kind) {
        (Kind::Stair, Kind::Stair) => None,
        (Kind::Stair, _) => Some(b.floor),
        (_, Kind::Stair) => Some(a.floor),
        _ => (a.floor == b.floor).then_some(a.floor),
    }
}

/// Whether a doorway fits between two lots: a storey they share, and three cells of wall.
fn could(lots: &[Lot], i: usize, j: usize) -> bool {
    storey(&lots[i], &lots[j]).is_some() && side(&lots[i], &lots[j]).is_some_and(|(_, s0, s1)| s1 - s0 >= 3)
}

fn linked(gaps: &[Gap], i: usize, j: usize) -> bool {
    gaps.iter().any(|g| (g.a, g.b) == (i, j) || (g.a, g.b) == (j, i))
}

/// A doorway in the wall `i` and `j` share, `wide` cells (or one if that won't fit) with a cell
/// of wall at each end, at cell `k` if given; None if no doorway fits.
fn gap(rng: &mut PyRandom, lots: &[Lot], i: usize, j: usize, wide: i32, k: Option<i32>) -> Option<Gap> {
    let z = storey(&lots[i], &lots[j])?;
    let (lo, hi) = if shared(&lots[i], &lots[j]).is_some() { (i, j) } else { (j, i) };
    let (axis, line, s0, s1) = shared(&lots[lo], &lots[hi])?;
    let fits = |w: i32| s1 - 1 - w > s0;
    let wide = if fits(wide) {
        wide
    } else if fits(1) {
        1
    } else {
        return None;
    };
    let k = k.unwrap_or_else(|| rng.randint((s0 + 1) as i64, (s1 - 1 - wide) as i64) as i32);
    Some(Gap { a: lo, b: hi, axis, line, k, wide, z })
}

/// A stair room's size for its shape, in cells (along its corridor, away from it): on the far
/// side of the gallery's corridor (`north`), or in the band between the two long corridors,
/// `m` deep. None if the shape doesn't fit there.
fn stair_size(rng: &mut PyRandom, shape: Shape, north: bool, m: i32) -> Option<(i32, i32)> {
    let mut draw = |a: i32, b: i32| rng.randint(a as i64, b as i64) as i32;
    Some(match (shape, north) {
        (Shape::Straight, true) => (draw(9, 10), draw(3, 4)),
        (Shape::L, true) => (draw(6, 7), draw(6, 7)),
        (Shape::U, true) => (4, draw(7, 8)),
        (Shape::Straight, false) => {
            if m >= 8 && draw(0, 1) == 0 {
                // a flight straight across, from one corridor up to the gallery over the other
                (draw(3, 4), m)
            } else {
                // a flight along the gallery, and a room behind it off the other corridor
                (draw(9, 10), if m >= 7 { draw(3, 4) } else { 3 })
            }
        }
        (Shape::L, false) => (draw(6, 7), m),
        (Shape::U, false) if m >= 7 => (4, m),
        (Shape::U, false) => return None,
    })
}

/// A layout being drawn up: its lots, the doorways it wants, and what the later stages are told.
#[derive(Default)]
struct Draft {
    lots: Vec<Lot>,
    /// Doorways to make, in order: the two lots and how wide (cells).
    doors: Vec<(usize, usize, i32)>,
    /// The stair rooms and the stairs each is sized for.
    stairs: Vec<(usize, Shape)>,
    /// The washrooms.
    wash: Vec<usize>,
    rooms: usize,
    uppers: usize,
}

impl Draft {
    fn lot(&mut self, key: &str, r: Rect, floor: i32, kind: Kind, scheme: &'static str) -> usize {
        let top = match kind {
            Kind::Hall => HALL_H,
            Kind::Stair => floor + STOREY + ROOM_H,
            _ => floor + ROOM_H,
        };
        self.lots.push(Lot { key: key.to_string(), x0: r.0, y0: r.1, x1: r.2, y1: r.3, floor, top, kind, scheme });
        self.lots.len() - 1
    }

    /// A room: `roomN` on the ground, `upperN` upstairs.
    fn room(&mut self, r: Rect, floor: i32) -> usize {
        let key = if floor == 0 {
            self.rooms += 1;
            format!("room{}", self.rooms)
        } else {
            self.uppers += 1;
            format!("upper{}", self.uppers)
        };
        self.lot(&key, r, floor, Kind::Room, "office")
    }

    /// Whether nothing stands in `r` between `floor` and `top`.
    fn clear(&self, r: Rect, floor: i32, top: i32) -> bool {
        !self.lots.iter().any(|l| l.floor < top && floor < l.top && overlaps(r, (l.x0, l.y0, l.x1, l.y1)))
    }
}

/// A layout one way round: corridor 1 along y = 0 and corridor 2 north of it, both running
/// east to the hall; the gallery over corridor 2.
fn draft(rng: &mut PyRandom) -> Result<Draft, String> {
    let mut d = Draft::default();
    let wide = |rng: &mut PyRandom| if rng.random() < 0.6 { 4 } else { 3 };
    let double = |rng: &mut PyRandom| if rng.random() < 0.2 { 2 } else { 1 };
    let w = rng.randint(24, 30) as i32;
    let (wa, wb) = (wide(rng), wide(rng));
    // the band between the corridors, and the rows of rooms outside them
    let m = rng.randint(6, 9) as i32;
    let (ds, dn) = (rng.randint(5, 7) as i32, rng.randint(5, 7) as i32);
    let (ym0, ym1, yn0) = (wa, wa + m, wa + m + wb);

    // ---- the cross corridors, and where the long corridors start: a ring, an H (one corridor
    // may start at its cross corridor, a bend) or a ladder
    let (mut a0, mut b0) = (0, 0);
    let mut crosses: Vec<(i32, i32)> = Vec::new();
    let pattern = rng.randint(0, 2);
    match pattern {
        0 => crosses.push((0, wide(rng))),
        1 => {
            let c = rng.randint(5, 9) as i32;
            crosses.push((c, c + wide(rng)));
            match rng.randint(0, 2) {
                1 => a0 = c,
                2 => b0 = c,
                _ => {}
            }
        }
        _ => {
            crosses.push((0, wide(rng)));
            let c = w / 2 + rng.randint(-3, 2) as i32;
            crosses.push((c, c + wide(rng)));
        }
    }
    // stairs in line with corridor 2, where it stops short of the west end (an H's)
    let end = pattern == 1 && wb == 4 && crosses[0].0 >= END && rng.random() < 0.5;
    if end && b0 == 0 {
        b0 = END;
    }

    // ---- the gallery over corridor 2, and a stair room towards each end of it
    let lu = (rng.randint(14, 20) as i32).min(w - b0);
    let u0 = if end { b0 } else { b0 + rng.randint(0, (w - b0 - lu) as i64) as i32 };
    let u1 = u0 + lu;
    let mut shapes = Shape::ALL;
    rng.shuffle(&mut shapes);
    let mut placed: Vec<(Rect, Shape)> = Vec::new();
    let wanted: Vec<(bool, Shape)> = if end {
        placed.push(((b0 - END, ym1, b0, yn0), Shape::U));
        vec![(false, *shapes.iter().find(|s| **s != Shape::U).expect("three shapes"))]
    } else {
        vec![(true, shapes[0]), (false, shapes[1])]
    };
    for (west, shape) in wanted {
        let mut sides = [true, false];
        rng.shuffle(&mut sides);
        let mut found = None;
        for north in sides {
            let Some((len, depth)) = stair_size(rng, shape, north, m) else { continue };
            let off = rng.randint(0, 1) as i32;
            let x0 = if west { u0 + off } else { u1 - off - len };
            let r = if north { (x0, yn0, x0 + len, yn0 + depth) } else { (x0, ym1 - depth, x0 + len, ym1) };
            let hits = placed.iter().any(|p| overlaps(p.0, r)) || (!north && crosses.iter().any(|c| c.0 < r.2 && r.0 < c.1));
            if !hits && u0 <= x0 && x0 + len <= u1 {
                found = Some((r, shape));
                break;
            }
        }
        placed.push(found.ok_or("no room for the stairs")?);
    }
    let north: Vec<(i32, i32)> = placed.iter().filter(|p| p.0 .1 == yn0).map(|p| (p.0 .0, p.0 .2)).collect();
    let middle: Vec<(i32, i32)> = placed.iter().filter(|p| p.0 .3 == ym1).map(|p| (p.0 .0, p.0 .2)).collect();

    // ---- the circulation's lots, and the doorways between them
    let a = d.lot("corridor1", (a0, 0, w, wa), 0, Kind::Corridor, "office");
    let b = d.lot("corridor2", (b0, ym1, w, yn0), 0, Kind::Corridor, "marble");
    let span = yn0;
    let hd = span.max(14) + rng.randint(0, 2) as i32;
    let hy0 = -(rng.randint(0, (hd - span) as i64) as i32);
    let hw = rng.randint(12, 16) as i32;
    let hall = d.lot("hall", (w, hy0, w + hw, hy0 + hd), 0, Kind::Hall, "hall");
    d.doors.extend([(a, hall, 2), (b, hall, 2)]);
    for (n, c) in crosses.iter().enumerate() {
        // the last cross corridor is the industrial one
        let scheme = if n + 1 == crosses.len() { "industrial" } else { "office" };
        let x = d.lot(&format!("cross{}", n + 1), (c.0, ym0, c.1, ym1), 0, Kind::Corridor, scheme);
        d.doors.extend([(x, a, 2), (x, b, 2)]);
    }
    let gallery = d.lot("gallery", (u0, ym1, u1, yn0), STOREY, Kind::Corridor, "office");
    for (n, (r, shape)) in placed.iter().enumerate() {
        let s = d.lot(&format!("stairs{}", n + 1), *r, 0, Kind::Stair, "office");
        d.stairs.push((s, *shape));
    }

    // ---- the rows of rooms off the corridors' outer sides, upstairs and down, and off the hall
    let mut fronts: Vec<Front> = Vec::new();
    let row = |fronts: &mut Vec<Front>, rooms: Vec<(i32, i32)>, y: i32, out: i32, depth: i32, floor: i32, on: usize| {
        fronts.extend(rooms.into_iter().map(|(x0, x1)| Front { x0, x1, y, out, depth, floor, on, wash: false }));
    };
    let south = rooms_along(rng, &[(a0, w)], 4, 8);
    row(&mut fronts, south, 0, -1, ds, 0, a);
    let rooms = rooms_along(rng, &stretches(b0, w, &north, &[]), 4, 8);
    row(&mut fronts, rooms, yn0, 1, dn, 0, b);
    let (dnu, dsu) = (rng.randint(5, 7) as i32, m.min(rng.randint(5, 7) as i32));
    let rooms = rooms_along(rng, &stretches(u0, u1, &north, &[]), 5, 9);
    row(&mut fronts, rooms, yn0, 1, dnu, STOREY, gallery);
    let rooms = rooms_along(rng, &stretches(u0, u1, &middle, &[]), 5, 9);
    row(&mut fronts, rooms, ym1, -1, dsu, STOREY, gallery);
    // one room of these becomes the washrooms: two or three small rooms together
    let roomy: Vec<usize> = (0..fronts.len()).filter(|&n| fronts[n].x1 - fronts[n].x0 >= 6).collect();
    if !roomy.is_empty() {
        let n = *rng.choice(&roomy);
        let f = fronts[n];
        let widths: &[i32] = match f.x1 - f.x0 {
            6 => &[3, 3],
            7 => &[3, 4],
            8 => &[4, 4],
            _ => &[3, 3, 3],
        };
        let mut x = f.x0;
        let wash: Vec<Front> = widths
            .iter()
            .map(|&wd| {
                x += wd;
                Front { x0: x - wd, x1: x, depth: 3, wash: true, ..f }
            })
            .collect();
        fronts.splice(n..=n, wash);
    }
    // off the hall's two side walls, clear of its platform (the far four cells)
    for (y, out) in [(hy0 + hd, 1), (hy0, -1)] {
        if rng.random() < 0.6 {
            let depth = rng.randint(4, 6) as i32;
            let mut rooms = rooms_along(rng, &[(w, w + hw - 5)], 4, 7);
            rooms.truncate(rng.randint(1, 2) as usize);
            row(&mut fronts, rooms, y, out, depth, 0, hall);
        }
    }
    let mut made: Vec<(usize, Front)> = Vec::new();
    for f in &fronts {
        let r = d.room(f.rect(), f.floor);
        d.doors.push((f.on, r, double(rng)));
        if f.wash {
            d.wash.push(r);
        }
        made.push((r, *f));
    }

    // ---- the rooms between the two corridors, each off one of them (the other is tried later)
    let mut taken = crosses.clone();
    taken.extend(&middle);
    for (x0, x1) in rooms_along(rng, &stretches(0, w, &taken, &[a0, b0]), 4, 8) {
        let (on_a, on_b) = (x0 >= a0, x0 >= b0);
        let first = match (on_a, on_b) {
            (true, true) => *rng.choice(&[a, b]),
            (true, false) => a,
            (false, true) => b,
            _ => continue,
        };
        let r = d.room((x0, ym0, x1, ym1), 0);
        d.doors.push((first, r, double(rng)));
    }
    // and behind a stair room that doesn't reach corridor 1
    for (r, _) in placed.iter().filter(|p| p.0 .3 == ym1 && p.0 .1 > ym0) {
        for (x0, x1) in rooms_along(rng, &[(r.0.max(a0), r.2)], 4, 8) {
            let k = d.room((x0, ym0, x1, r.1), 0);
            d.doors.push((a, k, double(rng)));
        }
    }

    // ---- a few rooms behind rooms: two doorways deep
    let mut behind: Vec<(usize, Front)> = made.into_iter().filter(|(_, f)| !f.wash && f.on != hall).collect();
    let n = rng.randint(0, 3) as usize;
    rng.shuffle(&mut behind);
    for (front, f) in behind.into_iter().take(n) {
        let depth = rng.randint(3, 4) as i32;
        let width = (f.x1 - f.x0).min(rng.randint(3, 6) as i32);
        let x0 = if rng.random() < 0.5 { f.x0 } else { f.x1 - width };
        let r = band(x0, x0 + width, f.y + f.out * f.depth, f.out, depth);
        if d.clear(r, f.floor, f.floor + ROOM_H) {
            let k = d.room(r, f.floor);
            d.doors.push((front, k, 1));
        }
    }
    Ok(d)
}

/// Turn the draft's lots: mirrored in x, in y, and x swapped with y, then moved so the least
/// cell is (0, 0). Gives the hall's far wall (E, N, W or S).
fn turn(lots: &mut [Lot], flip_x: bool, flip_y: bool, swap: bool) -> char {
    let xs = lots.iter().map(|l| l.x0).min().unwrap_or(0) + lots.iter().map(|l| l.x1).max().unwrap_or(0);
    let ys = lots.iter().map(|l| l.y0).min().unwrap_or(0) + lots.iter().map(|l| l.y1).max().unwrap_or(0);
    for l in lots.iter_mut() {
        if flip_x {
            (l.x0, l.x1) = (xs - l.x1, xs - l.x0);
        }
        if flip_y {
            (l.y0, l.y1) = (ys - l.y1, ys - l.y0);
        }
        if swap {
            (l.x0, l.y0, l.x1, l.y1) = (l.y0, l.x0, l.y1, l.x1);
        }
    }
    let (mx, my) = (lots.iter().map(|l| l.x0).min().unwrap_or(0), lots.iter().map(|l| l.y0).min().unwrap_or(0));
    for l in lots.iter_mut() {
        (l.x0, l.y0, l.x1, l.y1) = (l.x0 - mx, l.y0 - my, l.x1 - mx, l.y1 - my);
    }
    match (flip_x, swap) {
        (false, false) => 'E',
        (true, false) => 'W',
        (false, true) => 'N',
        (true, true) => 'S',
    }
}

/// A layout: the lots, every doorway, each stair room's stairs, and the hall's far wall.
struct Layout {
    lots: Vec<Lot>,
    gaps: Vec<Gap>,
    flights: Vec<(usize, Stairs)>,
    far: char,
}

/// Draw a layout: the draft turned, its doorways placed, the stairs fitted, second ways into
/// rooms, and every room's type.
fn layout(rng: &mut PyRandom) -> Result<Layout, String> {
    let d = draft(rng)?;
    let mut lots = d.lots;
    let (flip_x, flip_y, swap) = (rng.random() < 0.5, rng.random() < 0.5, rng.random() < 0.5);
    let far = turn(&mut lots, flip_x, flip_y, swap);
    let mut gaps: Vec<Gap> = Vec::new();
    for &(i, j, wide) in &d.doors {
        let g = gap(rng, &lots, i, j, wide, None).ok_or_else(|| format!("no doorway fits between {} and {}", lots[i].key, lots[j].key))?;
        gaps.push(g);
    }

    // ---- each stair room's stairs: the first frame (in a seeded order) that leaves the foot a
    // doorway onto a corridor on the ground and the top one onto the gallery
    let mut flights: Vec<(usize, Stairs)> = Vec::new();
    for &(s, shape) in &d.stairs {
        let l = &lots[s];
        let ((ax0, ay0, _), (ax1, ay1, _)) = l.air();
        let mut frames = Frame::ALL;
        rng.shuffle(&mut frames);
        let mut found = None;
        for frame in frames {
            let Ok(st) = stair::fit(shape, (ax0, ay0), (ax1, ay1), l.floor, STOREY, frame) else { continue };
            let (mut foot, mut top) = (Vec::new(), Vec::new());
            for (n, c) in lots.iter().enumerate().filter(|(_, c)| c.kind == Kind::Corridor) {
                let Some((wall, s0, s1)) = side(l, c) else { continue };
                let (spans, ways) = if c.floor == l.floor { (&st.foot, &mut foot) } else { (&st.top, &mut top) };
                ways.extend((s0 + 1..=s1 - 2).filter(|&k| spans.iter().any(|p| p.side == wall && p.holds(k * CELL, (k + 1) * CELL))).map(|k| (n, k)));
            }
            if !foot.is_empty() && !top.is_empty() {
                found = Some((st, *rng.choice(&foot), *rng.choice(&top)));
                break;
            }
        }
        let (st, foot, top) = found.ok_or_else(|| format!("no {} stairs fit {}", shape.name(), l.key))?;
        for (n, k) in [foot, top] {
            gaps.extend(gap(rng, &lots, s, n, 1, Some(k)));
        }
        lots[s].scheme = lots[foot.0].scheme;
        flights.push((s, st));
    }

    // ---- second ways in. First onto circulation: a room beside another corridor or the hall
    // often opens on that too (a short cut through the room)
    let rooms: Vec<usize> = (0..lots.len()).filter(|&i| lots[i].kind == Kind::Room).collect();
    let ways = |gaps: &[Gap], i: usize| gaps.iter().filter(|g| g.a == i || g.b == i).count();
    for &r in &rooms {
        for n in 0..lots.len() {
            let chance = match lots[n].kind {
                Kind::Hall => 0.7,
                Kind::Corridor => 0.6,
                _ => continue,
            };
            if !linked(&gaps, r, n) && could(&lots, r, n) && ways(&gaps, r) < 3 && rng.random() < chance {
                gaps.extend(gap(rng, &lots, r, n, 1, None));
            }
        }
    }
    // then until few enough rooms have one way in and the level has its loops: a doorway from
    // such a room onto circulation or into a neighbour (two at once where both have one way in)
    let few = rng.uniform(0.2, 0.36);
    let need = (TARGETS.loops * lots.len() as f64).ceil() as i64;
    loop {
        let alone: Vec<usize> = rooms.iter().copied().filter(|&r| ways(&gaps, r) == 1).collect();
        let short = (gaps.len() as i64 - lots.len() as i64 + 1) < need;
        if alone.len() as f64 <= few * rooms.len() as f64 && !short {
            break;
        }
        let from: Vec<usize> = if alone.is_empty() { rooms.iter().copied().filter(|&r| ways(&gaps, r) == 2).collect() } else { alone.clone() };
        let mut can: Vec<(usize, usize)> = Vec::new();
        for &r in &from {
            for n in 0..lots.len() {
                let fine = match lots[n].kind {
                    Kind::Stair => false,
                    // washrooms open on each other, not into an office
                    Kind::Room => n != r && ways(&gaps, n) < 3 && d.wash.contains(&n) == d.wash.contains(&r),
                    _ => true,
                };
                if fine && !linked(&gaps, r, n) && could(&lots, r, n) {
                    can.push((r, n));
                }
            }
        }
        let both: Vec<(usize, usize)> = can.iter().copied().filter(|(_, n)| alone.contains(n)).collect();
        let Some(&(r, n)) = (if both.is_empty() { can.first().map(|_| rng.choice(&can)) } else { Some(rng.choice(&both)) }) else { break };
        gaps.extend(gap(rng, &lots, r, n, 1, None));
    }

    // ---- room types, by where a room is and how big
    let hall = lots.iter().position(|l| l.kind == Kind::Hall).expect("a hall");
    for &r in &rooms {
        let beside = |scheme: &str| (0..lots.len()).any(|n| lots[n].kind == Kind::Corridor && lots[n].scheme == scheme && could(&lots, r, n));
        lots[r].scheme = if d.wash.contains(&r) {
            "washroom"
        } else if could(&lots, r, hall) {
            "hallside"
        } else if beside("industrial") {
            "industrial"
        } else if lots[r].area() >= BIG {
            "marble"
        } else {
            "office"
        };
    }
    // a room behind a room is its front room's type (a washroom's aside)
    for g in &gaps {
        for (back, front) in [(g.a, g.b), (g.b, g.a)] {
            if lots[back].kind == Kind::Room && lots[front].kind == Kind::Room && ways(&gaps, back) == 1 && lots[front].scheme != "washroom" {
                lots[back].scheme = lots[front].scheme;
            }
        }
    }
    Ok(Layout { lots, gaps, flights, far })
}

/// The hall's own frame: `u` from the wall the building is on to the far wall, `v` along them.
struct HallFrame {
    air: Box3,
    far: char,
}

impl HallFrame {
    /// How deep (along u) and how wide (along v) the hall's air is.
    fn size(&self) -> (i32, i32) {
        let ((x0, y0, _), (x1, y1, _)) = self.air;
        if matches!(self.far, 'E' | 'W') {
            (x1 - x0, y1 - y0)
        } else {
            (y1 - y0, x1 - x0)
        }
    }

    /// (u, v) as the plan's (x, y).
    fn point(&self, u: i32, v: i32) -> (i32, i32) {
        let ((x0, y0, _), (x1, y1, _)) = self.air;
        match self.far {
            'E' => (x0 + u, y0 + v),
            'W' => (x1 - u, y0 + v),
            'N' => (x0 + v, y0 + u),
            _ => (x0 + v, y1 - u),
        }
    }

    /// A box [u0, u1] x [v0, v1] from the floor up to `top`, as a solid's corners.
    fn solid(&self, u: (i32, i32), v: (i32, i32), top: i32) -> ((i32, i32, i32), (i32, i32, i32)) {
        let (p, q) = (self.point(u.0, v.0), self.point(u.1, v.1));
        ((p.0.min(q.0), p.1.min(q.1), 0), (p.0.max(q.0), p.1.max(q.1), top))
    }

    /// Where the plan's (x, y) is along v.
    fn along(&self, x: i32, y: i32) -> i32 {
        if matches!(self.far, 'E' | 'W') {
            y - self.air.0 .1
        } else {
            x - self.air.0 .0
        }
    }
}

/// The level for a seed (`o` is kept as the level's; Felicity reads none of it yet). A layout
/// that misses the circulation targets is drawn again from the same stream. Err if none of a
/// seed's layouts meets them, or if the planner made something its own plan check refuses (a
/// fault here, not the seed's).
pub fn generate(seed: u64, o: &Options) -> Result<Level, String> {
    let mut rng = PyRandom::named(&format!("{seed}/felicity"));
    let mut why = String::new();
    for _ in 0..TRIES {
        let lv = match layout(&mut rng) {
            Ok(l) => level(seed, o, l)?,
            Err(e) => {
                why = e;
                continue;
            }
        };
        let misses = circulation::of(&lv.plan).misses(&TARGETS);
        if misses.is_empty() {
            return Ok(lv);
        }
        why = misses.join("; ");
    }
    Err(format!("felicity seed {seed}: no layout in {TRIES} tries ({why})"))
}

/// The level of a layout: its doors, plan, walks and views.
fn level(seed: u64, o: &Options, layout: Layout) -> Result<Level, String> {
    let Layout { lots, gaps, flights, far } = layout;
    let flight_of = |s: usize| flights.iter().find(|f| f.0 == s).map(|f| &f.1);

    // ---- doors: their own stream
    let mut drng = PyRandom::named(&format!("{seed}/felicity/doors"));
    let mut doorways = Vec::new();
    for g in &gaps {
        let (a, b) = (&lots[g.a], &lots[g.b]);
        let p = if a.small() || b.small() {
            0.0
        } else {
            match (a.kind, b.kind) {
                (Kind::Room, Kind::Room) => 0.33,
                (Kind::Room, _) | (_, Kind::Room) => 0.8,
                (Kind::Hall, _) | (_, Kind::Hall) => 0.5,
                _ => 0.0,
            }
        };
        let roll = drng.random();
        let swing = drng.random() < 0.63;
        let door = (roll < p).then_some(DoorKind { swing, leaves: g.wide as u8 });
        let h = WALL / 2;
        let (along0, along1) = (g.k * CELL, (g.k + g.wide) * CELL);
        let (lo, hi) = match g.axis {
            0 => ((g.line * CELL - h, along0, g.z), (g.line * CELL + h, along1, g.z + DOOR_H)),
            _ => ((along0, g.line * CELL - h, g.z), (along1, g.line * CELL + h, g.z + DOOR_H)),
        };
        doorways.push(Doorway { a: a.key.clone(), b: b.key.clone(), lo, hi, axis: g.axis, door });
    }

    // ---- the plan: each lot's air, its half of each doorway, the steps
    let mut plan = Plan::new(&format!("Felicity {seed}"));
    let mut ramp_rails = Vec::new();
    let hall = lots.iter().position(|l| l.kind == Kind::Hall).expect("a hall");
    let frame = HallFrame { air: lots[hall].air(), far };
    let (deep, across) = frame.size();
    // the platform along the far wall
    let pu = deep - 4 * CELL;
    for (n, l) in lots.iter().enumerate() {
        let mut boxes = vec![l.air()];
        for (g, d) in gaps.iter().zip(&doorways) {
            let mid = d.plane();
            let half = |lo: (i32, i32, i32), hi: (i32, i32, i32), low: bool| -> Box3 {
                let (mut lo, mut hi) = (lo, hi);
                match (g.axis, low) {
                    (0, true) => hi.0 = mid,
                    (0, false) => lo.0 = mid,
                    (_, true) => hi.1 = mid,
                    (_, false) => lo.1 = mid,
                }
                (lo, hi)
            };
            if g.a == n {
                boxes.push(half(d.lo, d.hi, true));
            } else if g.b == n {
                boxes.push(half(d.lo, d.hi, false));
            }
        }
        let mut room = Room::new(&l.key, &l.key, l.floor, boxes);
        room.kind = match l.kind {
            Kind::Corridor => "corridor",
            Kind::Hall => "hub",
            Kind::Stair => "stair",
            Kind::Room => "room",
        };
        room.scheme = l.scheme;
        room.walkable = vec!["step", "landing", "platform"];
        match l.kind {
            Kind::Stair => {
                let st = flight_of(n).expect("a stair room's stairs");
                room.solids.extend(st.solids.iter().copied());
                room.rails.extend(st.rails.iter().copied());
                ramp_rails.extend(st.sloped_rails(&l.key));
                room.note = format!("{} stairs", st.shape.name());
            }
            Kind::Hall => {
                // the platform, three steps up to its middle third
                let (lo, hi) = frame.solid((pu, deep), (0, across), PLATFORM);
                room.solids.push((lo, hi, "platform"));
                let third = across / 3;
                for j in 0..3 {
                    let top = (((j + 1) * PLATFORM) as f64 / 3.0).round() as i32;
                    let u0 = pu - (3 - j) * GOING;
                    let (lo, hi) = frame.solid((u0, u0 + GOING), (third, across - third), top);
                    room.solids.push((lo, hi, "step"));
                }
                // pillars, floor to ceiling, in two rows clear of the middle
                let cols = 3;
                for c in 0..cols {
                    for row in [1, 3] {
                        let (u, v) = (pu * (c + 1) / (cols + 1), across * row / 4);
                        let h = CELL / 2;
                        let (lo, hi) = frame.solid((u - h, u + h), (v - h, v + h), HALL_H);
                        room.solids.push((lo, hi, "pillar"));
                    }
                }
            }
            _ => {}
        }
        plan.room(room)?;
    }
    for d in &doorways {
        plan.link(&d.a, &d.b, "door", if d.door.is_some() { "with a door" } else { "" })?;
    }
    let problems = plan.check();
    if !problems.is_empty() {
        return Err(format!("felicity seed {seed}: plan check: {}", problems[..problems.len().min(3)].join("; ")));
    }

    // ---- walks and views (cells of the plan's MODULE, as every walk is written)
    let c = |u: i32| u as f64 / MODULE;
    let mut walks: Vec<(String, Vec<Step>)> = Vec::new();
    // a point on the floor `inside` a room from a doorway's middle (`low`: on its a side)
    let inside = |d: &Doorway, low: bool| -> (i32, i32, i32) {
        let mid = d.plane();
        let off = if low { -INSIDE } else { INSIDE };
        let across = 1 - d.axis;
        let centre = (crate::plan::at(d.lo, across) + crate::plan::at(d.hi, across)) / 2;
        match d.axis {
            0 => (mid + off, centre, d.lo.2),
            _ => (centre, mid + off, d.lo.2),
        }
    };
    let cells = |p: (i32, i32, i32)| (c(p.0), c(p.1), p.2);
    // the points of a walk: stops at its ends, the rest passed through, none twice running
    let route = |pts: &[(i32, i32, i32)]| -> Vec<Step> {
        let mut pts = pts.to_vec();
        pts.dedup();
        let last = pts.len() - 1;
        pts.iter().enumerate().map(|(n, &p)| if n == 0 || n == last { Step::at(cells(p)) } else { Step::by(cells(p), 1) }).collect()
    };
    for d in &doorways {
        let (a, b) = (inside(d, true), inside(d, false));
        let m = ((a.0 + b.0) / 2, (a.1 + b.1) / 2, d.lo.2);
        walks.push((format!("door_{}_{}", d.a, d.b), route(&[a, m, b])));
    }
    // each plain room and corridor door to door (its floor is bare)
    for l in lots.iter().filter(|l| matches!(l.kind, Kind::Room | Kind::Corridor)) {
        let ends: Vec<(i32, i32, i32)> =
            doorways.iter().filter_map(|d| (d.a == l.key).then(|| inside(d, true)).or_else(|| (d.b == l.key).then(|| inside(d, false)))).collect();
        for i in 0..ends.len() {
            for j in i + 1..ends.len() {
                walks.push((format!("room_{}_{i}_{j}", l.key), route(&[ends[i], ends[j]])));
            }
        }
    }
    // up each flight, from the doorway at its foot to the one off its top landing: along the
    // doorway's wall until level with the first step, up the stairs' path, and along the landing
    // to the doorway
    for (s, st) in &flights {
        let l = &lots[*s];
        let of = |z: i32| doorways.iter().find(|d| (d.a == l.key || d.b == l.key) && d.lo.2 == z);
        let (Some(below), Some(above)) = (of(l.floor), of(l.floor + STOREY)) else { continue };
        let (Some(&first), Some(&last)) = (st.path.first(), st.path.last()) else { continue };
        let level_with = |p: (i32, i32, i32), d: &Doorway, q: (i32, i32, i32)| if d.axis == 0 { (p.0, q.1, p.2) } else { (q.0, p.1, p.2) };
        let (start, end) = (inside(below, below.a == l.key), inside(above, above.a == l.key));
        let mut pts = vec![start, level_with(start, below, first)];
        pts.extend(st.path.iter().copied());
        pts.extend([level_with(end, above, last), end]);
        walks.push((format!("stairs_{}", l.key), route(&pts)));
    }
    // the hall's platform, from a corridor's doorway: along the near wall to the middle (the
    // pillars stand a quarter in), then up the steps
    if let Some(d) = doorways.iter().find(|d| [&d.a, &d.b].contains(&&lots[hall].key) && [&d.a, &d.b].iter().any(|k| k.starts_with("corridor"))) {
        let start = inside(d, d.a == lots[hall].key);
        let (u, v) = (INSIDE - WALL / 2, frame.along(start.0, start.1));
        let at = |u: i32, v: i32, z: i32| {
            let p = frame.point(u, v);
            (p.0, p.1, z)
        };
        let mid = across / 2;
        walks.push(("platform".to_string(), route(&[at(u, v, 0), at(u, mid, 0), at(pu - 3 * GOING - 80, mid, 0), at(pu + 2 * CELL, mid, PLATFORM)])));
    }
    let mut views: Vec<(String, View)> = Vec::new();
    for (n, l) in lots.iter().enumerate() {
        let ((x0, y0, _), (x1, y1, _)) = l.air();
        // a stair room from the doorway at its foot, looking up its first flight (its corner
        // may be inside a landing); the hall from a corner by the building, looking at its
        // platform (the far corners are on it)
        let foot = doorways.iter().find(|d| (d.a == l.key || d.b == l.key) && d.lo.2 == l.floor);
        let view = match (flight_of(n).and_then(|st| st.path.get(1)), foot) {
            (Some(up), Some(d)) => {
                let from = inside(d, d.a == l.key);
                (c(from.0), c(from.1), l.floor, c(up.0), c(up.1))
            }
            _ if n == hall => {
                let (from, to) = (frame.point(60, 60), frame.point(deep, across));
                (c(from.0), c(from.1), l.floor, c(to.0), c(to.1))
            }
            _ => (c(x0 + 60), c(y0 + 60), l.floor, c(x1), c(y1)),
        };
        views.push((l.key.clone(), view));
    }

    // ---- what the rest of the generator reads: the rooms as spaces, a report
    let spaces: Vec<Space> = lots
        .iter()
        .map(|l| {
            let ((x0, y0, _), (x1, y1, _)) = l.air();
            Space {
                key: l.key.clone(),
                kind: "room",
                x0: (x0 as f64 / MODULE).floor() as i32,
                y0: (y0 as f64 / MODULE).floor() as i32,
                x1: (x1 as f64 / MODULE).ceil() as i32,
                y1: (y1 as f64 / MODULE).ceil() as i32,
                floor: 0,
                top: 236,
                host: String::new(),
                pool: None,
                decks: None,
                clear: Cells::default(),
                preset: Vec::new(),
            }
        })
        .collect();
    let links = doorways.len();
    let report =
        Report { seed, scale: "standard", rooms: lots.len(), plan_rooms: lots.len(), links, loops: links as i64 - lots.len() as i64 + 1, ..Report::default() };
    Ok(Level {
        seed,
        options: o.clone(),
        net: Network { grid: Grid::new(spaces, 2), pods: Vec::new(), failed: Vec::new() },
        interiors: Vec::new(),
        pods: Vec::new(),
        plan,
        glass: Vec::new(),
        pools: Vec::new(),
        walks,
        views,
        ramp_rails,
        moods: Vec::new(),
        ceilings: Vec::new(),
        glows: Vec::new(),
        drops: Vec::new(),
        grates: Vec::new(),
        windows: Vec::new(),
        beams: Vec::new(),
        dropped: Vec::new(),
        dead_ends: Vec::new(),
        fixes: Vec::new(),
        doorways,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn generate(seed: u64) -> Result<Level, String> {
        super::generate(seed, &Options { style: "felicity", ..Options::default() })
    }

    /// A plan room's floor in cells (its first box is its lot's air).
    fn floor_cells(r: &Room) -> i32 {
        let (lo, hi) = r.boxes[0];
        ((hi.0 - lo.0 + WALL) / CELL) * ((hi.1 - lo.1 + WALL) / CELL)
    }

    #[test]
    fn a_seed_makes_the_same_level_and_its_plan_checks() {
        for seed in 1..=40 {
            let lv = generate(seed).unwrap_or_else(|e| panic!("{e}"));
            assert!(lv.plan.check().is_empty(), "seed {seed}");
            let again = generate(seed).unwrap();
            assert_eq!(lv.plan, again.plan, "seed {seed}");
            assert_eq!(lv.doorways, again.doorways, "seed {seed}");
            // every room is reached: one piece
            assert_eq!(lv.plan.graph().components, 1, "seed {seed}");
        }
    }

    #[test]
    fn doors_go_where_felicity_puts_them() {
        // doorways and doors: from circulation into a room, between two rooms, into a small
        // room, between two rooms of the circulation that are not the hall, and the hall's own
        let mut n: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        let (mut leaves, mut swing, mut doors) = (0, 0, 0);
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            for d in &lv.doorways {
                let (a, b) = (lv.plan.get(&d.a).unwrap(), lv.plan.get(&d.b).unwrap());
                let small = |r: &Room| r.kind == "room" && floor_cells(r) < SMALL;
                let class = if small(a) || small(b) {
                    "small"
                } else {
                    match (a.kind == "room", b.kind == "room") {
                        (true, true) => "rooms",
                        (false, false) if a.kind == "hub" || b.kind == "hub" => "hall",
                        (false, false) => "circulation",
                        _ => "off",
                    }
                };
                let e = n.entry(class).or_default();
                e.0 += 1;
                if let Some(k) = d.door {
                    e.1 += 1;
                    doors += 1;
                    leaves += k.leaves as usize;
                    swing += k.swing as usize;
                    // a door's doorway is a door's size: 106 a leaf, 212 tall
                    assert_eq!(d.size(), (CELL * k.leaves as i32, DOOR_H));
                }
            }
        }
        println!("{n:?}");
        let share = |class: &str| n[class].1 as f64 / n[class].0 as f64;
        assert!((0.72..0.88).contains(&share("off")), "{:?}", n["off"]);
        assert!((0.25..0.42).contains(&share("rooms")), "{:?}", n["rooms"]);
        assert!((0.3..0.7).contains(&share("hall")), "{:?}", n["hall"]);
        assert_eq!((n["small"].1, n["circulation"].1), (0, 0));
        assert!(n["small"].0 > 0 && leaves > doors && swing > 0 && swing < doors);
    }

    /// The level of corridors with rooms as offshoots the user asked for (`circulation::TARGETS`),
    /// on seeds 1-60: `cargo test -p autolevel --release felicity_meets_the_circulation_targets --
    /// --nocapture` prints every seed's measures and what each misses.
    #[test]
    fn felicity_meets_the_circulation_targets() {
        use crate::circulation::of;
        let mut failed = 0;
        for seed in 1..=60 {
            let c = of(&generate(seed).unwrap().plan);
            let misses = c.misses(&TARGETS);
            println!(
                "seed {seed:2}: {}{}",
                c.row(),
                if misses.is_empty() {
                    String::new()
                } else {
                    format!(
                        "
         MISSES: {}",
                        misses.join("; ")
                    )
                }
            );
            failed += !misses.is_empty() as usize;
        }
        assert_eq!(failed, 0, "{failed} of 60 seeds miss the targets");
    }

    #[test]
    fn stair_rooms_take_every_shape() {
        let mut seen = BTreeMap::new();
        for seed in 1..=40 {
            for r in generate(seed).unwrap().plan.rooms.iter().filter(|r| r.kind == "stair") {
                *seen.entry(r.note.clone()).or_insert(0) += 1;
            }
        }
        println!("{seen:?}");
        assert_eq!(seen.len(), 3, "{seen:?}");
        assert!(seen.values().all(|&n| n >= 10), "{seen:?}");
    }

    #[test]
    fn the_stairs_rise_a_storey_in_twelve_steps() {
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            let stairs: Vec<&Room> = lv.plan.rooms.iter().filter(|r| r.kind == "stair").collect();
            assert!(stairs.len() >= 2, "seed {seed}");
            for r in stairs {
                let steps: Vec<i32> = r.solids.iter().filter(|s| s.2 == "step").map(|s| s.1 .2).collect();
                assert_eq!(steps.len(), 12);
                assert_eq!((steps[0], steps[11]), (24, 283));
                assert!(steps.windows(2).all(|w| (23..=24).contains(&(w[1] - w[0]))));
                // a doorway at its foot and one off its landing, both onto corridors
                let ways: Vec<&Doorway> = lv.doorways.iter().filter(|d| d.a == r.key || d.b == r.key).collect();
                let mut floors: Vec<i32> = ways.iter().map(|d| d.lo.2).collect();
                floors.sort();
                assert_eq!(floors, [0, STOREY], "seed {seed} {}", r.key);
                for d in ways {
                    let other = lv.plan.get(if d.a == r.key { &d.b } else { &d.a }).unwrap();
                    assert_eq!((other.kind, other.floor), ("corridor", d.lo.2), "seed {seed} {}", r.key);
                }
                // and a walk up it
                assert!(lv.walks.iter().any(|(name, _)| *name == format!("stairs_{}", r.key)), "seed {seed} {}", r.key);
            }
        }
    }

    #[test]
    fn the_hall_and_the_upper_storey_are_ways_through() {
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            let ways = |key: &str, kind: &str| {
                lv.doorways
                    .iter()
                    .filter(|d| (d.a == key && lv.plan.get(&d.b).unwrap().kind == kind) || (d.b == key && lv.plan.get(&d.a).unwrap().kind == kind))
                    .count()
            };
            // two corridors end on the hall, and two stair rooms reach the gallery
            assert!(ways("hall", "corridor") >= 2, "seed {seed}");
            assert!(ways("gallery", "stair") >= 2, "seed {seed}");
            // every upper room is off the gallery, or behind a room that is
            let gallery = lv.plan.get("gallery").unwrap();
            assert_eq!((gallery.kind, gallery.floor), ("corridor", STOREY));
            // walks: every doorway crossed, and the platform climbed
            assert!(lv.doorways.iter().all(|d| lv.walks.iter().any(|(name, _)| *name == format!("door_{}_{}", d.a, d.b))), "seed {seed}");
            assert!(lv.walks.iter().any(|(name, _)| name == "platform"), "seed {seed}");
        }
    }

    #[test]
    fn rooms_are_typed_by_where_they_are() {
        let mut types: BTreeMap<&str, usize> = BTreeMap::new();
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            let hall = lv.plan.get("hall").unwrap();
            assert_eq!((hall.kind, hall.scheme), ("hub", "hall"));
            for r in lv.plan.rooms.iter().filter(|r| r.kind == "room") {
                *types.entry(r.scheme).or_default() += 1;
                // a washroom is a small room
                if r.scheme == "washroom" {
                    assert!(floor_cells(r) <= 12, "seed {seed} {}", r.key);
                }
            }
            // an industrial corridor in every level, and washrooms come two or three together
            assert!(lv.plan.rooms.iter().any(|r| r.kind == "corridor" && r.scheme == "industrial"), "seed {seed}");
            let wash = lv.plan.rooms.iter().filter(|r| r.scheme == "washroom").count();
            assert!(wash == 0 || (2..=3).contains(&wash), "seed {seed}: {wash} washrooms");
        }
        println!("{types:?}");
        // every type is used; offices are the most, and with the marble rooms most of the building
        assert_eq!(types.len(), 5, "{types:?}");
        let rooms: usize = types.values().sum();
        assert!(types.values().all(|&n| n <= types["office"]) && (types["office"] + types["marble"]) * 5 > rooms * 3, "{types:?}");
    }
}
