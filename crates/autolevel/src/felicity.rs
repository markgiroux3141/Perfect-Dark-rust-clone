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

use crate::balcony::{self, Balcony};
use crate::cells::Cells;
use crate::circulation::{self, TARGETS};
use crate::level::{Arch, DoorKind, Doorway, Level, Options, Step, View};
use crate::network::Network;
use crate::plan::{Box3, Plan, Room, Solid, MODULE};
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
/// The hall's screens: how thick, the arches beside its side walls (how wide, where they spring:
/// high enough for a walkway to pass under), and the piers between the arches.
const SCREEN: i32 = 80;
const SIDE_ARCH: i32 = 400;
const SIDE_SPRING: i32 = 400;
const PIER: i32 = CELL;
/// How far inside a room a walk starts and ends from a doorway's middle plane.
const INSIDE: i32 = 100;
/// How many layouts a seed may draw before one meets the targets.
const TRIES: usize = 40;
/// The share of layouts that sprawl (`sprawl`); the rest are the two-corridor block (`block`).
const SPRAWL: f64 = 0.7;
/// The most lots a sprawling layout makes (the targets want 34 at most).
const MOST: usize = 32;
/// The chance a sprawling layout with a gallery takes a second, over another long corridor.
const SECOND: f64 = 0.6;
/// The chance a tall room takes a landing off its walkway, and the most a layout has.
const LANDING: f64 = 0.7;
const LANDINGS: usize = 2;
/// The chance a block layout's hall takes a landing (its only tall room).
const BLOCK_LANDING: f64 = 0.9;

/// What a lot is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Corridor,
    Room,
    Hall,
    /// A big open room people pass through (circulation, a "hub"): one to three storeys tall.
    Open,
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

/// The storey a doorway from an upper lot onto a tall room's walkway is on: the upper lot's
/// floor, a storey over the tall room's (a hall or an open room tall enough). None for any other
/// pair. Such doorways are only ever planned (a landing's), never found (`could` leaves them out).
fn upper(a: &Lot, b: &Lot) -> Option<i32> {
    let tall = |l: &Lot| matches!(l.kind, Kind::Open | Kind::Hall);
    let (t, u) = if tall(a) { (a, b) } else { (b, a) };
    (tall(t) && !tall(u) && u.kind != Kind::Stair && u.floor == t.floor + STOREY && t.top >= u.floor + ROOM_H).then_some(u.floor)
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
    let z = storey(&lots[i], &lots[j]).or_else(|| upper(&lots[i], &lots[j]))?;
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

/// Where stairs `st` in stair room `s` may take their doorways: (lot, cell along the shared wall)
/// onto a corridor on the stair room's floor at the foot (or onto `hub`, the tall room a landing's
/// stairs stand beside; never where the doorway's mouth meets the hall's platform, its far wall
/// `far`), and at the top onto an upper corridor (`top`, if given, else any).
#[allow(clippy::type_complexity)]
fn stair_ways(lots: &[Lot], s: usize, st: &Stairs, top: Option<usize>, hub: Option<usize>, far: char) -> (Vec<(usize, i32)>, Vec<(usize, i32)>) {
    let l = &lots[s];
    let (mut foot, mut up) = (Vec::new(), Vec::new());
    let off_platform = |n: usize, k: i32| {
        let c = &lots[n];
        if c.kind != Kind::Hall {
            return true;
        }
        let (a, b) = if shared(l, c).is_some() { (s, n) } else { (n, s) };
        let Some((axis, line, _, _)) = shared(&lots[a], &lots[b]) else { return true };
        let (lo, hi) = gap_box(&Gap { a, b, axis, line, k, wide: 1, z: c.floor });
        let m = if axis == 0 { (lo.0 - MOUTH, lo.1 - 60, hi.0 + MOUTH, hi.1 + 60) } else { (lo.0 - 60, lo.1 - MOUTH, hi.0 + 60, hi.1 + MOUTH) };
        !fixtures(c, far).iter().any(|(p, q, _)| m.0 < q.0 && p.0 < m.2 && m.1 < q.1 && p.1 < m.3)
    };
    for (n, c) in lots.iter().enumerate().filter(|&(n, c)| c.kind == Kind::Corridor || hub == Some(n)) {
        let Some((wall, s0, s1)) = side(l, c) else { continue };
        let (spans, ways) = if c.floor == l.floor {
            (&st.foot, &mut foot)
        } else if top.is_none_or(|t| t == n) {
            (&st.top, &mut up)
        } else {
            continue;
        };
        ways.extend(
            (s0 + 1..=s1 - 2).filter(|&k| spans.iter().any(|p| p.side == wall && p.holds(k * CELL, (k + 1) * CELL)) && off_platform(n, k)).map(|k| (n, k)),
        );
    }
    (foot, up)
}

/// Whether some way round, stairs of `shape` fit stair room `s` with a doorway at the foot (onto a
/// corridor, or `hub`) and one at the top onto `top`. Drawn up the draft's way round (the hall's
/// far wall east).
fn stair_fits(lots: &[Lot], s: usize, shape: Shape, top: usize, hub: Option<usize>) -> bool {
    let ((x0, y0, _), (x1, y1, _)) = lots[s].air();
    Frame::ALL.iter().any(|&f| {
        stair::fit(shape, (x0, y0), (x1, y1), lots[s].floor, STOREY, f).is_ok_and(|st| {
            let (foot, up) = stair_ways(lots, s, &st, Some(top), hub, 'E');
            !foot.is_empty() && !up.is_empty()
        })
    })
}

/// A layout being drawn up: its lots, the doorways it wants, and what the later stages are told.
#[derive(Default)]
struct Draft {
    lots: Vec<Lot>,
    /// Doorways to make, in order: the two lots and how wide (cells).
    doors: Vec<(usize, usize, i32)>,
    /// The stair rooms, the stairs each is sized for, the upper corridor its top must open on
    /// (None: any), and the tall room its foot may open on instead of a corridor (a landing's).
    stairs: Vec<(usize, Shape, Option<usize>, Option<usize>)>,
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

    /// A big open room, `top` its ceiling.
    fn open(&mut self, n: usize, r: Rect, top: i32, scheme: &'static str) -> usize {
        let at = self.lot(&format!("open{n}"), r, 0, Kind::Open, scheme);
        self.lots[at].top = top;
        at
    }

    /// Whether nothing stands in `r` between `floor` and `top`.
    fn clear(&self, r: Rect, floor: i32, top: i32) -> bool {
        !self.lots.iter().any(|l| l.floor < top && floor < l.top && overlaps(r, (l.x0, l.y0, l.x1, l.y1)))
    }
}

/// A layout one way round (the hall's far wall to the east): most sprawl, the rest are the
/// two-corridor block.
fn draft(rng: &mut PyRandom, sprawling: bool) -> Result<Draft, String> {
    if sprawling {
        sprawl(rng)
    } else {
        block(rng)
    }
}

/// The four ways out of a lot: 0 east, 1 north, 2 west, 3 south.
fn wall_len(l: &Lot, dir: usize) -> i32 {
    if dir % 2 == 0 {
        l.y1 - l.y0
    } else {
        l.x1 - l.x0
    }
}

/// The cells out from lot `l`'s wall `dir`: from `at` cells along the wall (from its low end)
/// for `width`, and `len` out from it.
fn beside(l: &Lot, dir: usize, at: i32, width: i32, len: i32) -> Rect {
    match dir {
        0 => (l.x1, l.y0 + at, l.x1 + len, l.y0 + at + width),
        1 => (l.x0 + at, l.y1, l.x0 + at + width, l.y1 + len),
        2 => (l.x0 - len, l.y0 + at, l.x0, l.y0 + at + width),
        _ => (l.x0 + at, l.y0 - len, l.x0 + at + width, l.y0),
    }
}

/// Whether a lot runs along x (its long way).
fn along_x(l: &Lot) -> bool {
    l.x1 - l.x0 >= l.y1 - l.y0
}

/// A sprawling layout, one way round (the hall's far wall to the east; nothing is built past it
/// or beside its platform). It grows from the hall: piece by piece, a corridor (3 to 5 cells
/// wide, 6 to 18 long) or a big open room (7 to 14 by 7 to 12, one to three storeys tall) off a
/// wall of a piece already there, at its end (a straight run or a bend) or its side (a branch),
/// the newest pieces the likeliest, so arms reach out. Pieces that end up wall to wall are
/// joined (loops). Then a gallery upstairs over a long corridor with a stair room near each end,
/// and rooms of every size along every wall with stretches left empty, so the outline is
/// ragged: some rooms two storeys tall, a washroom cluster, a few rooms behind rooms.
fn sprawl(rng: &mut PyRandom) -> Result<Draft, String> {
    let mut d = Draft::default();
    let double = |rng: &mut PyRandom| if rng.random() < 0.2 { 2 } else { 1 };
    let (hw, hd) = (rng.randint(12, 16) as i32, rng.randint(14, 19) as i32);
    let hall = d.lot("hall", (0, 0, hw, hd), 0, Kind::Hall, "hall");
    // past the far wall, and beside the platform (its four cells and one), nothing
    let blocked = [(hw, -60, hw + 60, hd + 60), (hw - 5, hd, hw + 60, hd + 60), (hw - 5, -60, hw + 60, 0)];
    let ok = |d: &Draft, r: Rect, floor: i32, top: i32| d.clear(r, floor, top) && !blocked.iter().any(|b| overlaps(*b, r));
    let tall = |rng: &mut PyRandom| ROOM_H + STOREY * *rng.choice(&[0, 0, 0, 1, 1, 2]);
    let corridor_scheme = |rng: &mut PyRandom| *rng.choice(&["office", "office", "marble", "industrial"]);

    // ---- the circulation: two corridors off the hall, then pieces off pieces
    let mut anchors: Vec<(usize, usize)> = vec![(hall, 2), (hall, if rng.random() < 0.5 { 1 } else { 3 })];
    let want = rng.randint(6, 10) as usize;
    let (mut corridors, mut opens, mut pieces) = (0, 0, 0);
    let mut tries = 0;
    while pieces < want && tries < 300 && !anchors.is_empty() {
        tries += 1;
        // the newest anchors the likeliest (an arm keeps growing), now and then any
        // (the first two pieces off the hall's own two walls: two ways into it)
        let i = if pieces < 2 {
            match anchors.iter().position(|a| a.0 == hall) {
                Some(i) => i,
                None => break,
            }
        } else if rng.random() < 0.65 {
            anchors.len() - 1 - rng.randint(0, (anchors.len() - 1).min(2) as i64) as usize
        } else {
            rng.randint(0, (anchors.len() - 1) as i64) as usize
        };
        let (from, dir) = anchors[i];
        let l = d.lots[from].clone();
        let wl = wall_len(&l, dir);
        let corridor = pieces < 2 || rng.random() < 0.55;
        let (r, top) = if corridor {
            let w = (*rng.choice(&[3, 3, 4, 4, 5])).min(wl);
            if w < 3 {
                anchors.remove(i);
                continue;
            }
            let len = rng.randint(6, 18) as i32;
            (beside(&l, dir, rng.randint(0, (wl - w) as i64) as i32, w, len), ROOM_H)
        } else {
            let (w, len) = (rng.randint(7, 14) as i32, rng.randint(7, 12) as i32);
            if wl < 3 {
                anchors.remove(i);
                continue;
            }
            // over the wall's end if it likes, three cells shared at least
            let at = rng.randint(-(w - 3) as i64, (wl - 3) as i64) as i32;
            (beside(&l, dir, at, w, len), tall(rng))
        };
        if !ok(&d, r, 0, top) {
            if rng.random() < 0.4 {
                anchors.remove(i);
            }
            continue;
        }
        let n = if corridor {
            corridors += 1;
            let scheme = corridor_scheme(rng);
            d.lot(&format!("corridor{corridors}"), r, 0, Kind::Corridor, scheme)
        } else {
            opens += 1;
            let scheme = *rng.choice(&["marble", "industrial", "hall", "office"]);
            d.open(opens, r, top, scheme)
        };
        // a corridor's end opens all but a cell each side; a hall or open room by two cells
        let span = if d.lots[from].kind == Kind::Corridor && wl <= 5 { wl - 2 } else { 2 };
        d.doors.push((from, n, span.clamp(1, 3)));
        pieces += 1;
        // its walls to grow from: a corridor's far end and its sides, an open room's three
        let back = (dir + 2) % 4;
        for k in 0..4 {
            if k != back {
                anchors.push((n, k));
            }
        }
        // the far end of a corridor last: it is the likeliest to be taken next (a long arm)
        if corridor {
            let end = anchors.iter().position(|a| *a == (n, dir)).expect("its end");
            let e = anchors.remove(end);
            anchors.push(e);
        }
    }
    if corridors < 2 || opens == 0 {
        return Err("the sprawl grew too few corridors, or no open room".into());
    }
    // pieces wall to wall are joined (a loop through the circulation)
    let circ: Vec<usize> = (0..d.lots.len()).collect();
    for &a in &circ {
        for &b in &circ {
            if a < b && !d.doors.iter().any(|&(i, j, _)| (i, j) == (a, b) || (i, j) == (b, a)) && could(&d.lots, a, b) && rng.random() < 0.8 {
                let wide = side(&d.lots[a], &d.lots[b]).map_or(1, |(_, s0, s1)| (s1 - s0 - 2).clamp(1, 2));
                d.doors.push((a, b, wide));
            }
        }
    }
    // one corridor is the industrial one
    let cs: Vec<usize> = (0..d.lots.len()).filter(|&n| d.lots[n].kind == Kind::Corridor).collect();
    if !cs.iter().any(|&n| d.lots[n].scheme == "industrial") {
        let n = *rng.choice(&cs);
        d.lots[n].scheme = "industrial";
    }

    // ---- the gallery over a long corridor, and a stair room near each end of it, beside it
    let mut long: Vec<usize> = cs
        .iter()
        .copied()
        .filter(|&n| {
            let l = &d.lots[n];
            let (len, w) = if along_x(l) { (l.x1 - l.x0, l.y1 - l.y0) } else { (l.y1 - l.y0, l.x1 - l.x0) };
            len >= 12 && w <= 4
        })
        .collect();
    rng.shuffle(&mut long);
    let mut shapes = Shape::ALL;
    rng.shuffle(&mut shapes);
    // a second gallery, now and then, over another long corridor
    let (mut galleries, mut want) = (0, 1);
    // the galleries' rooms are hung last of the upper storey: the landings and the rooms along the
    // tall rooms come first (the lots run out)
    let mut gallery_rooms: Vec<(usize, [usize; 2])> = Vec::new();
    for &c in &long {
        if galleries >= want {
            break;
        }
        let l = d.lots[c].clone();
        let ax = along_x(&l);
        let (a0, a1) = if ax { (l.x0, l.x1) } else { (l.y0, l.y1) };
        let lu = (rng.randint(12, 20) as i32).min(a1 - a0);
        let u0 = a0 + rng.randint(0, (a1 - a0 - lu) as i64) as i32;
        let g = if ax { (u0, l.y0, u0 + lu, l.y1) } else { (l.x0, u0, l.x1, u0 + lu) };
        if !ok(&d, g, STOREY, STOREY + ROOM_H) {
            continue;
        }
        // the stairs: two shapes, one near each end, on either side of the corridor
        let mut placed: Vec<(Rect, Shape)> = Vec::new();
        for (k, end) in [true, false].into_iter().enumerate() {
            let mut found = None;
            'shape: for shape in [shapes[k], shapes[(k + 2) % 3]] {
                let Some((len, depth)) = stair_size(rng, shape, true, 0) else { continue };
                let off = rng.randint(0, 1) as i32;
                let at = if end { u0 - a0 + off } else { u0 - a0 + lu - off - len };
                let mut sides: [usize; 2] = if ax { [1, 3] } else { [0, 2] };
                rng.shuffle(&mut sides);
                for dir in sides {
                    let r = beside(&l, dir, at, len, depth);
                    let inside_gallery = if ax { u0 <= r.0 && r.2 <= u0 + lu } else { u0 <= r.1 && r.3 <= u0 + lu };
                    if inside_gallery && ok(&d, r, 0, STOREY + ROOM_H) && !placed.iter().any(|p| overlaps(p.0, r)) {
                        found = Some((r, shape));
                        break 'shape;
                    }
                }
            }
            if let Some(f) = found {
                placed.push(f);
            }
        }
        if placed.len() < 2 {
            continue;
        }
        galleries += 1;
        let key = if galleries == 1 { "gallery".to_string() } else { format!("gallery{galleries}") };
        let gallery = d.lot(&key, g, STOREY, Kind::Corridor, "office");
        for (r, shape) in &placed {
            let s = d.lot(&format!("stairs{}", d.stairs.len() + 1), *r, 0, Kind::Stair, "office");
            d.stairs.push((s, *shape, Some(gallery), None));
        }
        gallery_rooms.push((gallery, if ax { [1, 3] } else { [0, 2] }));
        if galleries == 1 && d.lots.len() + 4 <= MOST && rng.random() < SECOND {
            want = 2;
        }
    }
    if galleries == 0 {
        return Err("no room for the stairs".into());
    }

    // ---- landings: an upper corridor along a wall of a tall room a storey up, opening onto the
    // walkway that wall will take (`balconies`), with a stair room off its far side or an end down
    // to a corridor on the ground (its second way), and rooms off it
    let mut talls: Vec<usize> = (0..d.lots.len()).filter(|&n| matches!(d.lots[n].kind, Kind::Open | Kind::Hall) && d.lots[n].top >= ROOM_H + STOREY).collect();
    rng.shuffle(&mut talls);
    let mut landings = 0;
    for t in talls {
        if landings >= LANDINGS || d.lots.len() + 3 > MOST || rng.random() >= LANDING {
            continue;
        }
        // the hall's walkway runs along a side wall (its screens stand across the others')
        let mut dirs: Vec<usize> = if d.lots[t].kind == Kind::Hall { vec![1, 3] } else { vec![0, 1, 2, 3] };
        rng.shuffle(&mut dirs);
        if landing(&mut d, rng, t, &dirs, landings + 1, i32::MAX, &ok) {
            landings += 1;
        }
    }

    // ---- upper rooms along the tall rooms' walls, off an upper corridor beside them (each is then
    // likely to open onto the tall room's walkway too: `onto_walkways`); the hall's side walls only,
    // and a tall room with a landing on the landing's wall
    let talls: Vec<usize> = (0..d.lots.len()).filter(|&n| levels(&d.lots[n]) >= 1).collect();
    for t in talls {
        if d.lots.len() >= MOST || rng.random() >= ALONG {
            continue;
        }
        let tl = d.lots[t].clone();
        let landing = d.doors.iter().find_map(|&(i, j, _)| (i == t && d.lots[j].floor == STOREY).then(|| side(&tl, &d.lots[j]).map(|s| s.0)).flatten());
        let uppers: Vec<Lot> = d.lots.iter().filter(|l| l.floor == STOREY && l.kind == Kind::Corridor).cloned().collect();
        let mut can: Vec<(Rect, usize)> = Vec::new();
        for dir in if tl.kind == Kind::Hall { vec![1, 3] } else { vec![0, 1, 2, 3] } {
            if landing.is_some_and(|w| w != ['E', 'N', 'W', 'S'][dir]) {
                continue;
            }
            let wl = wall_len(&tl, dir);
            for at in -3..wl - 3 {
                for (w, depth) in (3..=6).flat_map(|w| (3..=5).map(move |dd| (w, dd))) {
                    let r = beside(&tl, dir, at, w, depth);
                    let room =
                        Lot { key: String::new(), x0: r.0, y0: r.1, x1: r.2, y1: r.3, floor: STOREY, top: STOREY + ROOM_H, kind: Kind::Room, scheme: "office" };
                    let along = (at + w).min(wl) - at.max(0);
                    let off = uppers.iter().position(|u| side(u, &room).is_some_and(|(_, s0, s1)| s1 - s0 >= 3));
                    if let (true, Some(u)) = (along >= 3 && ok(&d, r, STOREY, STOREY + ROOM_H), off) {
                        let key = &uppers[u].key;
                        can.push((r, d.lots.iter().position(|l| l.key == *key).expect("an upper corridor")));
                    }
                }
            }
        }
        if !can.is_empty() {
            let (r, u) = *rng.choice(&can);
            let k = d.room(r, STOREY);
            d.doors.push((u, k, 1));
        }
    }

    for (gallery, dirs) in gallery_rooms {
        upper_rooms(&mut d, rng, gallery, &dirs, &ok);
    }

    // ---- rooms along every wall of the circulation on the ground, every size, gaps between
    let mut order: Vec<usize> =
        (0..d.lots.len()).filter(|&n| d.lots[n].floor == 0 && matches!(d.lots[n].kind, Kind::Corridor | Kind::Open | Kind::Hall)).collect();
    rng.shuffle(&mut order);
    let mut offs: Vec<(usize, usize, usize)> = Vec::new(); // (room, the lot it is off, the wall's way out)
    let mut wash_done = false;
    for &c in &order {
        let l = d.lots[c].clone();
        for dir in 0..4 {
            let wl = wall_len(&l, dir);
            let mut at = 0;
            while wl - at >= 3 && d.lots.len() < MOST {
                if rng.random() < 0.18 {
                    at += rng.randint(1, 3) as i32; // a stretch of bare wall
                    continue;
                }
                let big = rng.random() < 0.15;
                let w = if big { rng.randint(9, 13) } else { rng.randint(3, 8) } as i32;
                let depth = if big { rng.randint(7, 11) } else { rng.randint(3, 8) } as i32;
                // the washrooms: two or three small rooms together, once
                if !wash_done && !big && w >= 6 && wl - at >= 6 && l.kind == Kind::Corridor && rng.random() < 0.35 {
                    let widths: &[i32] = match w {
                        6 => &[3, 3],
                        7 => &[3, 4],
                        8 => &[4, 4],
                        _ => &[3, 3, 3],
                    };
                    let rs: Vec<Rect> = widths
                        .iter()
                        .scan(at, |x, &ww| {
                            let r = beside(&l, dir, *x, ww, 3);
                            *x += ww;
                            Some(r)
                        })
                        .collect();
                    if rs.iter().all(|r| ok(&d, *r, 0, ROOM_H)) {
                        for r in rs {
                            let k = d.room(r, 0);
                            d.doors.push((c, k, 1));
                            d.wash.push(k);
                        }
                        wash_done = true;
                        at += widths.iter().sum::<i32>();
                        continue;
                    }
                }
                let top = if w * depth >= 30 && rng.random() < 0.15 { ROOM_H + STOREY } else { ROOM_H };
                let r = beside(&l, dir, at, w, depth);
                let r3 = beside(&l, dir, at, w, 3);
                let pick = if ok(&d, r, 0, top) {
                    Some((r, top))
                } else if ok(&d, r3, 0, ROOM_H) {
                    Some((r3, ROOM_H))
                } else {
                    None
                };
                match pick {
                    Some((r, top)) => {
                        let k = d.room(r, 0);
                        d.lots[k].top = top;
                        d.doors.push((c, k, double(rng)));
                        offs.push((k, c, dir));
                        at += w;
                    }
                    None => at += 1,
                }
            }
        }
    }
    // ---- a few rooms behind rooms: two doorways deep
    let n = rng.randint(0, 3) as usize;
    rng.shuffle(&mut offs);
    for (front, on, dir) in offs.into_iter().filter(|&(_, on, _)| on != hall).take(n) {
        let _ = on;
        let f = d.lots[front].clone();
        if d.lots.len() >= MOST || f.top != ROOM_H {
            continue;
        }
        let wl = wall_len(&f, dir);
        let w = wl.min(rng.randint(3, 6) as i32);
        if w < 3 {
            continue;
        }
        let at = if rng.random() < 0.5 { 0 } else { wl - w };
        let r = beside(&f, dir, at, w, rng.randint(3, 4) as i32);
        if ok(&d, r, 0, ROOM_H) {
            let k = d.room(r, 0);
            d.doors.push((front, k, 1));
        }
    }
    Ok(d)
}

/// Whether a family lets a lot stand on cells `r` from `floor` to `top` (nothing in the way, and
/// the family's own bounds).
type Fits<'a> = &'a dyn Fn(&Draft, Rect, i32, i32) -> bool;

/// Rooms along walls `dirs` of upper corridor `from`, upstairs, until the lots run out (`MOST`).
fn upper_rooms(d: &mut Draft, rng: &mut PyRandom, from: usize, dirs: &[usize], ok: Fits) {
    let gl = d.lots[from].clone();
    for &dir in dirs {
        let wl = wall_len(&gl, dir);
        let mut at = 0;
        while at < wl - 2 && d.lots.len() < MOST {
            let (w, depth) = (rng.randint(4, 9) as i32, rng.randint(4, 7) as i32);
            // as deep as it was drawn, or shortened to stop at what is in the way (often a tall
            // room's wall, which it may then open onto: `onto_walkways`)
            let r = (3..=depth).rev().map(|dd| beside(&gl, dir, at, w.min(wl - at), dd)).find(|r| ok(d, *r, STOREY, STOREY + ROOM_H));
            if let Some(r) = r.filter(|_| wl - at >= 3) {
                let k = d.room(r, STOREY);
                d.doors.push((from, k, 1));
                at += w;
            } else {
                at += 1;
            }
        }
    }
}

/// A landing (`landingN`) along one of walls `dirs` of tall room `t`, a storey up: an upper
/// corridor 5 to 12 cells along the wall and 3 or 4 out, opening onto the walkway that wall will
/// take (`balconies`), with a stair room off its far side or an end down to a corridor on the
/// ground or into `t` itself (its second way), and rooms off it, all within `reach` cells of the
/// wall's low end. Whether one was made.
fn landing(d: &mut Draft, rng: &mut PyRandom, t: usize, dirs: &[usize], n: usize, reach: i32, ok: Fits) -> bool {
    let tl = d.lots[t].clone();
    for &dir in dirs {
        // a walkway along this wall needs the room 8 cells deep away from it
        let (wl, deep) = (wall_len(&tl, dir).min(reach), wall_len(&tl, (dir + 1) % 4));
        if wl < 5 || deep < 8 {
            continue;
        }
        for _ in 0..8 {
            // short enough to leave three cells of the wall at an end where it can (a room there
            // stands along the tall room too, and may open onto its walkway)
            let lu = (rng.randint(5, 12) as i32).min(wl).min(if wl >= 8 { wl - 3 } else { wl });
            let mut at = rng.randint(0, (wl - lu) as i64) as i32;
            if wl - lu >= 3 && at < 3 && wl - lu - at < 3 {
                at = if rng.random() < 0.5 { 0 } else { wl - lu };
            }
            let u = beside(&tl, dir, at, lu, rng.randint(3, 4) as i32);
            if !ok(d, u, STOREY, STOREY + ROOM_H) {
                continue;
            }
            let un = d.lot(&format!("landing{n}"), u, STOREY, Kind::Corridor, "office");
            let ul = d.lots[un].clone();
            let mut ends = [dir, (dir + 1) % 4, (dir + 3) % 4];
            rng.shuffle(&mut ends);
            let mut shapes = Shape::ALL;
            rng.shuffle(&mut shapes);
            for d2 in ends {
                for shape in shapes {
                    let Some((len, depth)) = stair_size(rng, shape, true, 0) else { continue };
                    let ulen = wall_len(&ul, d2);
                    let pos = rng.randint(-(len - 3).min(ulen - 3) as i64, (ulen - 3) as i64) as i32;
                    // where drawn, and off an end flush with the tall room's wall (where it may
                    // open into the tall room)
                    // (the tall room is on the landing's low side off its east or north wall)
                    let flush = if dir <= 1 { 0 } else { ulen - len };
                    let mut tries = vec![pos];
                    if d2 != dir && pos != flush {
                        tries.push(flush);
                    }
                    for pos in tries {
                        let r = beside(&ul, d2, pos, len, depth);
                        if !ok(d, r, 0, STOREY + ROOM_H) {
                            continue;
                        }
                        let sn = d.lot(&format!("stairs{}", d.stairs.len() + 1), r, 0, Kind::Stair, "office");
                        let sl = &d.lots[sn];
                        // its foot onto a corridor on the ground, or into the tall room it overlooks
                        let beside_ground = d
                            .lots
                            .iter()
                            .enumerate()
                            .any(|(c, cl)| (cl.kind == Kind::Corridor || c == t) && cl.floor == 0 && side(sl, cl).is_some_and(|(_, a, b)| b - a >= 3));
                        if beside_ground && could(&d.lots, sn, un) && stair_fits(&d.lots, sn, shape, un, Some(t)) {
                            d.stairs.push((sn, shape, Some(un), Some(t)));
                            d.doors.push((t, un, 1));
                            let back = (dir + 2) % 4;
                            let walls: Vec<usize> = (0..4).filter(|&k| k != back).collect();
                            upper_rooms(d, rng, un, &walls, ok);
                            return true;
                        }
                        d.lots.pop();
                    }
                }
            }
            d.lots.pop();
        }
    }
    false
}

/// A layout one way round: corridor 1 along y = 0 and corridor 2 north of it, both running
/// east to the hall; the gallery over corridor 2; now and then a landing off the hall's walkway.
fn block(rng: &mut PyRandom) -> Result<Draft, String> {
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
        d.stairs.push((s, *shape, None, None));
    }

    // ---- a landing off the hall's walkway along a side wall (`landing`), all of it beside the hall
    // so nothing of the corridors' rows meets it. Its stairs (a U, mostly: foot and top at one end)
    // stand off its far end and open at the foot into the hall beside it; the landing stops three
    // cells short of the platform's stretch so that doorway is clear of the platform, and the
    // stair room may run on to the far wall
    let hx = (w, w + hw - 5);
    let beside_hall = |d: &Draft, r: Rect, floor: i32, top: i32| d.clear(r, floor, top) && w <= r.0 && r.2 <= w + hw;
    if rng.random() < BLOCK_LANDING {
        let mut dirs = vec![1, 3];
        rng.shuffle(&mut dirs);
        landing(&mut d, rng, hall, &dirs, 1, hx.1 - hx.0 - 3, &beside_hall);
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
    // off the hall's two side walls, clear of its platform (the far four cells) and of a landing's
    // stair room
    for (y, out) in [(hy0 + hd, 1), (hy0, -1)] {
        if rng.random() < 0.6 {
            let depth = rng.randint(4, 6) as i32;
            let stairs: Vec<(i32, i32)> = d
                .lots
                .iter()
                .filter(|l| l.kind == Kind::Stair && l.floor == 0 && (if out > 0 { l.y0 == y } else { l.y1 == y }))
                .map(|l| (l.x0, l.x1))
                .collect();
            let mut rooms = rooms_along(rng, &stretches(hx.0, hx.1, &stairs, &[]), 4, 7);
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

/// A layout: the lots, every doorway, each stair room's stairs, the walkways in the tall rooms,
/// and the hall's far wall.
struct Layout {
    lots: Vec<Lot>,
    gaps: Vec<Gap>,
    flights: Vec<(usize, Stairs)>,
    balconies: Vec<(usize, Balcony)>,
    hall: Hall,
    far: char,
}

/// The hall's screens (`screens`): their solids, arches and footprints.
#[derive(Default)]
struct Hall {
    solids: Vec<Solid>,
    arches: Vec<Arch>,
    feet: Vec<(i32, i32, i32, i32)>,
}

/// A doorway's box: through its wall, `wide` cells along it, a door's height up from its storey.
fn gap_box(g: &Gap) -> Box3 {
    let h = WALL / 2;
    let (along0, along1) = (g.k * CELL, (g.k + g.wide) * CELL);
    match g.axis {
        0 => ((g.line * CELL - h, along0, g.z), (g.line * CELL + h, along1, g.z + DOOR_H)),
        _ => ((along0, g.line * CELL - h, g.z), (along1, g.line * CELL + h, g.z + DOOR_H)),
    }
}

/// How far into a room a doorway's mouth is kept clear (of a flight standing on the floor).
const MOUTH: i32 = 120;

/// What stands in a hall or an open room: the hall's platform along its far wall with three
/// steps up its middle third (its screens are `screens`'); an open room 9 cells or more each way
/// four pillars, a third of the way in each way. Nothing in any other lot.
fn fixtures(l: &Lot, far: char) -> Vec<Solid> {
    let mut out: Vec<Solid> = Vec::new();
    match l.kind {
        Kind::Hall => {
            let frame = HallFrame { air: l.air(), far };
            let (deep, across) = frame.size();
            let pu = deep - 4 * CELL;
            let (lo, hi) = frame.solid((pu, deep), (0, across), PLATFORM);
            out.push((lo, hi, "platform"));
            let third = across / 3;
            for j in 0..3 {
                let top = (((j + 1) * PLATFORM) as f64 / 3.0).round() as i32;
                let u0 = pu - (3 - j) * GOING;
                let (lo, hi) = frame.solid((u0, u0 + GOING), (third, across - third), top);
                out.push((lo, hi, "step"));
            }
        }
        Kind::Open => {
            let ((x0, y0, _), (x1, y1, _)) = l.air();
            if l.x1 - l.x0 >= 9 && l.y1 - l.y0 >= 9 {
                let h = CELL / 2;
                for (i, j) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
                    let (x, y) = (x0 + (x1 - x0) * i / 3, y0 + (y1 - y0) * j / 3);
                    out.push(((x - h, y - h, l.floor), (x + h, y + h, l.top), "pillar"));
                }
            }
        }
        _ => {}
    }
    out
}

/// The hall's screens: two walls across it, `SCREEN` thick, that split it into three bays, the
/// last the platform's (Felicity's hall is three bays behind arched walls). Each is pierced by
/// three round-headed arches: one beside each side wall, `SIDE_ARCH` wide, springing at
/// `SIDE_SPRING` (a walkway along the side wall passes under it), and a wide one between, on a
/// semicircle as high as the hall allows; piers between them, a beam over each. The second stands
/// just short of the platform's steps, the first halfway to it; each moves (20 at a time, 200 at
/// most) to keep clear of any doorway a storey up in a side wall, or is left out. Gives the solids
/// (tags `pier`, `beam`), the arches, and each screen's footprint (x0, y0, x1, y1).
#[allow(clippy::type_complexity)]
fn screens(l: &Lot, far: char, gaps: &[Gap], n: usize) -> (Vec<Solid>, Vec<Arch>, Vec<(i32, i32, i32, i32)>) {
    let frame = HallFrame { air: l.air(), far };
    let (deep, across) = frame.size();
    let pu = deep - 4 * CELL;
    // where a doorway a storey up stands along the hall's depth
    let ups: Vec<(i32, i32)> = gaps
        .iter()
        .filter(|g| (g.a == n || g.b == n) && g.z == l.floor + STOREY)
        .map(|g| {
            let (lo, hi) = gap_box(g);
            let (u0, u1) = (frame.depth(lo.0, lo.1), frame.depth(hi.0, hi.1));
            (u0.min(u1), u0.max(u1))
        })
        .collect();
    let free = |u: i32| ups.iter().all(|&(g0, g1)| u + SCREEN + CLEAR_UP <= g0 || g1 + CLEAR_UP <= u);
    let second = pu - 3 * GOING - SCREEN - 40;
    let mut at: Vec<i32> = Vec::new();
    for want in [(second - SCREEN) / 2, second] {
        let lo = at.last().map_or(200, |&u| u + SCREEN + 200);
        let found = (0..=10).flat_map(|k| [want + 20 * k, want - 20 * k]).find(|&u| u >= lo && u <= second && free(u));
        at.extend(found);
    }
    let (mut solids, mut arches, mut feet) = (Vec::new(), Vec::new(), Vec::new());
    let wc = across - 2 * (SIDE_ARCH + PIER);
    let rc = wc / 2;
    let central = (SIDE_SPRING.min(HALL_H - 60 - rc), SIDE_SPRING.min(HALL_H - 60 - rc) + rc);
    // the openings across: (v0, v1, spring, crown)
    let openings = [
        (0, SIDE_ARCH, SIDE_SPRING, SIDE_SPRING + SIDE_ARCH / 2),
        (SIDE_ARCH + PIER, across - SIDE_ARCH - PIER, central.0, central.1),
        (across - SIDE_ARCH, across, SIDE_SPRING, SIDE_SPRING + SIDE_ARCH / 2),
    ];
    let axis = if matches!(far, 'E' | 'W') { 0 } else { 1 };
    let base = if axis == 0 { frame.air.0 .1 } else { frame.air.0 .0 };
    for u in at {
        let u1 = u + SCREEN;
        let (lo, hi) = frame.solid((u, u1), (0, across), HALL_H);
        feet.push((lo.0, lo.1, hi.0, hi.1));
        for v in [SIDE_ARCH, across - SIDE_ARCH - PIER] {
            let (lo, hi) = frame.solid((u, u1), (v, v + PIER), HALL_H);
            solids.push((lo, hi, "pier"));
        }
        let (p, q) = (frame.point(u, 0), frame.point(u1, 0));
        let t = if axis == 0 { (p.0.min(q.0), p.0.max(q.0)) } else { (p.1.min(q.1), p.1.max(q.1)) };
        for (v0, v1, spring, crown) in openings {
            let (mut lo, hi) = frame.solid((u, u1), (v0, v1), HALL_H);
            lo.2 = crown;
            solids.push((lo, hi, "beam"));
            arches.push(Arch {
                a: l.key.clone(),
                b: l.key.clone(),
                axis,
                t,
                mid: (t.0 + t.1) / 2,
                s: (base + v0, base + v1),
                floor: l.floor,
                spring,
                top: crown,
                kind: "screen",
            });
        }
    }
    (solids, arches, feet)
}

/// How far a screen keeps from a doorway a storey up in the hall's side wall.
const CLEAR_UP: i32 = 40;

/// Walkways up high in each tall room (an open room two or three storeys tall, the hall): the
/// first of the eight ways round, in an order drawn from `brng`, whose flight keeps clear of the
/// room's doorways on the floor and of what stands in it (`balcony::fit`), and whose walkway's
/// wall holds every doorway onto it a storey up (a landing's). A room three storeys tall takes a
/// second walkway two storeys up. None where nothing fits; Err if a room with a doorway onto its
/// walkway gets none.
fn balconies(brng: &mut PyRandom, lots: &[Lot], gaps: &[Gap], far: char, hall: &Hall) -> Result<Vec<(usize, Balcony)>, String> {
    let mut out = Vec::new();
    for (n, l) in lots.iter().enumerate() {
        if levels(l) < 1 {
            continue;
        }
        let mut frames = Frame::ALL;
        brng.shuffle(&mut frames);
        let ups = gaps.iter().any(|g| (g.a == n || g.b == n) && g.z == l.floor + STOREY);
        match walkway(lots, gaps, n, far, hall, &frames, &mut |steps| brng.randint(0, steps)) {
            Some(b) => out.push((n, b)),
            None if ups => return Err(format!("no walkway in {} for the doorway onto it", l.key)),
            None => {}
        }
    }
    Ok(out)
}

/// How many walkways a tall room takes: one a storey up in the hall, in an open room two
/// storeys tall and in a tall plain room, two in an open room three storeys tall; none elsewhere.
fn levels(l: &Lot) -> i32 {
    match l.kind {
        Kind::Hall => 1,
        Kind::Open | Kind::Room => (l.top - l.floor - ROOM_H) / STOREY,
        _ => 0,
    }
}

/// How far a flight on a plain room's floor keeps from the straight line between two of its
/// doorways (its walks go door to door in a line): a chr's width and some.
const LINE: f64 = 60.0;

/// Whether the segment `p`-`q` passes within `pad` of the rectangle `r` (x0, y0, x1, y1).
fn crosses(p: (f64, f64), q: (f64, f64), r: (i32, i32, i32, i32), pad: f64) -> bool {
    let (x0, y0, x1, y1) = (r.0 as f64 - pad, r.1 as f64 - pad, r.2 as f64 + pad, r.3 as f64 + pad);
    // the segment clipped to the rectangle, one axis at a time (Liang-Barsky)
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (from, d, lo, hi) in [(p.0, q.0 - p.0, x0, x1), (p.1, q.1 - p.1, y0, y1)] {
        if d == 0.0 {
            if from < lo || from > hi {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo - from) / d, (hi - from) / d);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    t0 <= t1
}

/// The walkways of tall room `n`: the first of `frames` whose flight, against the wall at the
/// walkway's end, keeps clear of the room's doorways on the floor and of what stands in it
/// (`balcony::fit`), and whose walkway's wall holds every doorway onto it a storey up; else the
/// first with its flight out in the room, half a cell at a time along the walkway from the place
/// `first(steps)` gives (it is asked only then). None if nothing fits.
fn walkway(lots: &[Lot], gaps: &[Gap], n: usize, far: char, hall: &Hall, frames: &[Frame], first: &mut dyn FnMut(i64) -> i64) -> Option<Balcony> {
    let l = &lots[n];
    let levels = levels(l);
    let mut standing = fixtures(l, far);
    // the hall's screens: their piers and beams stand in it, and no flight crosses one
    let mut keep: Vec<(i32, i32, i32, i32)> = Vec::new();
    if l.kind == Kind::Hall {
        standing.extend(hall.solids.iter().copied());
        keep.extend(hall.feet.iter().copied());
    }
    let ((x0, y0, _), (x1, y1, _)) = l.air();
    // each doorway on the floor, and its mouth either side
    let mut mouths: Vec<(i32, i32, i32, i32)> = gaps
        .iter()
        .filter(|g| (g.a == n || g.b == n) && g.z == l.floor)
        .map(|g| {
            let (lo, hi) = gap_box(g);
            if g.axis == 0 {
                (lo.0 - MOUTH, lo.1, hi.0 + MOUTH, hi.1)
            } else {
                (lo.0, lo.1 - MOUTH, hi.0, hi.1 + MOUTH)
            }
        })
        .collect();
    // each doorway onto the walkway: its wall (E, N, W, S) and its stretch along it
    let ups: Vec<(char, i32, i32)> = gaps
        .iter()
        .filter(|g| (g.a == n || g.b == n) && g.z == l.floor + STOREY)
        .map(|g| {
            let wall = match (g.axis, g.a == n) {
                (0, true) => 'E',
                (0, false) => 'W',
                (_, true) => 'N',
                _ => 'S',
            };
            (wall, g.k * CELL, (g.k + g.wide) * CELL)
        })
        .collect();
    mouths.extend(keep);
    // a plain room's walks go door to door in straight lines (from `INSIDE` in from each doorway's
    // middle), which its flight keeps off
    let ends: Vec<(f64, f64)> = gaps
        .iter()
        .filter(|g| l.kind == Kind::Room && (g.a == n || g.b == n) && g.z == l.floor)
        .map(|g| {
            let (lo, hi) = gap_box(g);
            let into = if g.a == n { -INSIDE } else { INSIDE };
            let (mid, across) = if g.axis == 0 { ((lo.0 + hi.0) / 2 + into, (lo.1 + hi.1) / 2) } else { ((lo.1 + hi.1) / 2 + into, (lo.0 + hi.0) / 2) };
            if g.axis == 0 {
                (mid as f64, across as f64)
            } else {
                (across as f64, mid as f64)
            }
        })
        .collect();
    let lines: Vec<((f64, f64), (f64, f64))> =
        (0..ends.len()).flat_map(|i| (i + 1..ends.len()).map(move |j| (i, j))).map(|(i, j)| (ends[i], ends[j])).collect();
    let fits = |frame: Frame, at: i32| {
        balcony::fit(((x0, y0), (x1, y1)), l.floor, STOREY, levels, frame, at, &standing).ok().filter(|b| {
            !mouths.iter().any(|m| m.0 < b.ground.2 && b.ground.0 < m.2 && m.1 < b.ground.3 && b.ground.1 < m.3)
                && !lines.iter().any(|&(p, q)| crosses(p, q, b.ground, LINE))
                && ups.iter().all(|&(wall, lo, hi)| b.upper[0].side == wall && b.upper[0].holds(lo, hi))
        })
    };
    let found = frames.iter().find_map(|&f| fits(f, 0));
    if found.is_some() {
        return found;
    }
    let span = (x1 - x0).max(y1 - y0);
    let steps = (span / (CELL / 2)) as i64;
    let first = first(steps);
    frames.iter().find_map(|&f| (0..=steps).find_map(|i| fits(f, ((first + i) % (steps + 1)) as i32 * (CELL / 2))))
}

/// Draw a layout: the draft turned, its doorways placed, the stairs fitted, second ways into
/// rooms, and every room's type.
fn layout(rng: &mut PyRandom, brng: &mut PyRandom, sprawling: bool) -> Result<Layout, String> {
    let d = draft(rng, sprawling)?;
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
    for &(s, shape, up, hub) in &d.stairs {
        let l = &lots[s];
        let ((ax0, ay0, _), (ax1, ay1, _)) = l.air();
        let mut frames = Frame::ALL;
        rng.shuffle(&mut frames);
        let mut found = None;
        for frame in frames {
            let Ok(st) = stair::fit(shape, (ax0, ay0), (ax1, ay1), l.floor, STOREY, frame) else { continue };
            let (foot, top) = stair_ways(&lots, s, &st, up, hub, far);
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
                Kind::Corridor | Kind::Open => 0.6,
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
    onto_walkways(rng, &lots, &mut gaps, far);

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
    let n = lots.iter().position(|l| l.kind == Kind::Hall).expect("a hall");
    let (solids, arches, feet) = screens(&lots[n], far, &gaps, n);
    let hall = Hall { solids, arches, feet };
    let balconies = balconies(brng, &lots, &gaps, far, &hall)?;
    Ok(Layout { lots, gaps, flights, balconies, hall, far })
}

/// The chance an upper room beside a tall room opens onto its walkway.
const ONTO: f64 = 0.8;
/// The chance a tall room takes an upper room planned along its wall, off an upper corridor.
const ALONG: f64 = 0.8;

/// Upper rooms beside a tall room open onto its walkway (`ONTO`), a second way in: all of a tall
/// room's on one wall, the walkway's (a landing's wall if it has one, else one drawn; the hall's
/// side walls only, its screens stand across the others' flights), and each kept only if a walkway
/// still fits holding every doorway onto it (`walkway`, tried every way round, drawing nothing).
fn onto_walkways(rng: &mut PyRandom, lots: &[Lot], gaps: &mut Vec<Gap>, far: char) {
    let wall_of = |g: &Gap, n: usize| match (g.axis, g.a == n) {
        (0, true) => 'E',
        (0, false) => 'W',
        (_, true) => 'N',
        _ => 'S',
    };
    for t in (0..lots.len()).filter(|&t| levels(&lots[t]) >= 1) {
        let tl = &lots[t];
        let side_walls: &[char] = if matches!(far, 'E' | 'W') { &['N', 'S'] } else { &['E', 'W'] };
        let mut by_wall: Vec<(char, Vec<usize>)> = Vec::new();
        for (r, rl) in lots.iter().enumerate() {
            if rl.kind != Kind::Room || linked(gaps, r, t) || upper(tl, rl).is_none() {
                continue;
            }
            let Some((wall, s0, s1)) = side(tl, rl) else { continue };
            if s1 - s0 < 3 || (tl.kind == Kind::Hall && !side_walls.contains(&wall)) {
                continue;
            }
            match by_wall.iter_mut().find(|w| w.0 == wall) {
                Some(w) => w.1.push(r),
                None => by_wall.push((wall, vec![r])),
            }
        }
        if by_wall.is_empty() {
            continue;
        }
        let held: Vec<char> = gaps.iter().filter(|g| (g.a == t || g.b == t) && g.z == tl.floor + STOREY).map(|g| wall_of(g, t)).collect();
        let wall = match held.first() {
            Some(w) => *w,
            None => rng.choice(&by_wall).0,
        };
        let Some((_, rooms)) = by_wall.iter().find(|w| w.0 == wall) else { continue };
        for &r in rooms {
            if rng.random() >= ONTO {
                continue;
            }
            let Some(g) = gap(rng, lots, r, t, 1, None) else { continue };
            gaps.push(g);
            let hall = if tl.kind == Kind::Hall {
                let (solids, arches, feet) = screens(tl, far, gaps, t);
                Hall { solids, arches, feet }
            } else {
                Hall::default()
            };
            if walkway(lots, gaps, t, far, &hall, &Frame::ALL, &mut |_| 0).is_none() {
                gaps.pop();
            }
        }
    }
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

    /// Where the plan's (x, y) is along u.
    fn depth(&self, x: i32, y: i32) -> i32 {
        let ((x0, y0, _), (x1, y1, _)) = self.air;
        match self.far {
            'E' => x - x0,
            'W' => x1 - x,
            'N' => y - y0,
            _ => y1 - y,
        }
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
    // the family once a seed: a seed that sprawls keeps sprawling through its redraws
    let sprawling = rng.random() < SPRAWL;
    let mut why = String::new();
    for _ in 0..TRIES {
        // the walkways draw from their own stream, so they never move a room
        let mut brng = PyRandom::named(&format!("{seed}/felicity/balconies"));
        let lv = match layout(&mut rng, &mut brng, sprawling) {
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
    let Layout { lots, gaps, flights, balconies, hall: screened, far } = layout;
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
                (Kind::Hall | Kind::Open, _) | (_, Kind::Hall | Kind::Open) => 0.5,
                _ => 0.0,
            }
        };
        let roll = drng.random();
        let swing = drng.random() < 0.63;
        // a doorway wider than two leaves is an open archway
        let door = (roll < p && g.wide <= 2).then_some(DoorKind { swing, leaves: g.wide as u8 });
        let (lo, hi) = gap_box(g);
        doorways.push(Doorway { a: a.key.clone(), b: b.key.clone(), lo, hi, axis: g.axis, door });
    }

    // ---- arches: over every doorless doorway between corridors, halls, open rooms and stair
    // rooms (a semicircle on one cell, a segment rising a quarter of the width on more, in the
    // doorway's own height), and the hall's screens'
    let circulation = |l: &Lot| l.kind != Kind::Room;
    let mut arches: Vec<Arch> = Vec::new();
    for (g, d) in gaps.iter().zip(&doorways) {
        if d.door.is_some() || !circulation(&lots[g.a]) || !circulation(&lots[g.b]) {
            continue;
        }
        let w = g.wide * CELL;
        let rise = if g.wide == 1 { w / 2 } else { w / 4 };
        let (t0, t1) = (crate::plan::at(d.lo, g.axis), crate::plan::at(d.hi, g.axis));
        let across = 1 - g.axis;
        arches.push(Arch {
            a: d.a.clone(),
            b: d.b.clone(),
            axis: g.axis,
            t: (t0, t1),
            mid: d.plane(),
            s: (crate::plan::at(d.lo, across), crate::plan::at(d.hi, across)),
            floor: g.z,
            spring: g.z + DOOR_H - rise,
            top: g.z + DOOR_H,
            kind: "doorway",
        });
    }
    arches.extend(screened.arches.iter().cloned());

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
            Kind::Hall | Kind::Open => "hub",
            Kind::Stair => "stair",
            Kind::Room => "room",
        };
        room.scheme = l.scheme;
        room.walkable = vec!["step", "landing", "platform", "walkway"];
        room.solids.extend(fixtures(l, far));
        if l.kind == Kind::Hall {
            room.solids.extend(screened.solids.iter().copied());
        }
        if let Some((_, b)) = balconies.iter().find(|b| b.0 == n) {
            room.solids.extend(b.solids.iter().copied());
            room.rails.extend(b.rails.iter().copied());
            ramp_rails.extend(b.sloped.iter().map(|(p, q, f)| (l.key.clone(), *p, *q, *f)));
            room.note = if b.levels == 2 { "two walkways" } else { "a walkway" }.to_string();
        }
        match l.kind {
            Kind::Stair => {
                let st = flight_of(n).expect("a stair room's stairs");
                room.solids.extend(st.solids.iter().copied());
                room.rails.extend(st.rails.iter().copied());
                ramp_rails.extend(st.sloped_rails(&l.key));
                room.note = format!("{} stairs", st.shape.name());
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
    // each tall room's walkways: from the foot of the flight up to the first, along it to its
    // far end, and on up to the second and along it
    for (n, b) in &balconies {
        let pts: Vec<(i32, i32, i32)> = b.paths.iter().flatten().copied().collect();
        walks.push((format!("walkway_{}", lots[*n].key), route(&pts)));
    }
    // the hall's platform, from a corridor's doorway: along the near wall to the middle, then
    // through the screens' middle arches and up the steps
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
        arches,
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
                // a doorway at its foot and one off its landing, both onto corridors (a landing's
                // stairs may stand on the tall room it overlooks instead)
                let ways: Vec<&Doorway> = lv.doorways.iter().filter(|d| d.a == r.key || d.b == r.key).collect();
                let mut floors: Vec<i32> = ways.iter().map(|d| d.lo.2).collect();
                floors.sort();
                assert_eq!(floors, [0, STOREY], "seed {seed} {}", r.key);
                let other = |d: &Doorway| lv.plan.get(if d.a == r.key { &d.b } else { &d.a }).unwrap();
                let landing = ways.iter().any(|d| other(d).key.starts_with("landing"));
                for d in &ways {
                    let o = other(d);
                    let foot_on_hub = landing && o.kind == "hub" && d.lo.2 == o.floor;
                    assert!((o.kind == "corridor" || foot_on_hub) && o.floor == d.lo.2, "seed {seed} {}: {} {}", r.key, o.kind, o.key);
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
    fn every_upstairs_area_is_reached_two_ways() {
        let (mut seconds, mut landings, mut blocks, mut block_landings) = (0, 0, 0, 0);
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            // the block family: two long corridors joined by cross corridors
            let block = lv.plan.get("cross1").is_some();
            blocks += block as usize;
            block_landings += (block && lv.plan.get("landing1").is_some()) as usize;
            let other = |d: &Doorway, key: &str| lv.plan.get(if d.a == key { &d.b } else { &d.a }).unwrap();
            for u in lv.plan.rooms.iter().filter(|r| r.kind == "corridor" && r.floor == STOREY) {
                let ways: Vec<&Doorway> = lv.doorways.iter().filter(|d| d.a == u.key || d.b == u.key).collect();
                let stairs = ways.iter().filter(|d| other(d, &u.key).kind == "stair").count();
                let walkways: Vec<&&Doorway> = ways.iter().filter(|d| other(d, &u.key).kind == "hub").collect();
                assert!(stairs + walkways.len() >= 2, "seed {seed} {}: {stairs} stairs, {} walkways", u.key, walkways.len());
                seconds += (u.key == "gallery2") as usize;
                if u.key.starts_with("landing") {
                    landings += 1;
                    assert_eq!((stairs, walkways.len()), (1, 1), "seed {seed} {}", u.key);
                }
                // a doorway onto a tall room lands on a walkway along that wall
                for d in walkways {
                    let t = other(d, &u.key);
                    assert!(on_walkway(t, d), "seed {seed} {} into {}: no walkway at the doorway", u.key, t.key);
                }
            }
        }
        println!("{seconds} second galleries, {landings} landings on seeds 1-40; {block_landings} of {blocks} block layouts have a landing");
        assert!(seconds >= 4 && landings >= 6, "{seconds} second galleries, {landings} landings");
        // the block's second upstairs area: a landing off the hall's walkway, on most
        assert!(block_landings * 2 >= blocks, "{block_landings} of {blocks} block layouts have a landing");
    }

    /// Whether doorway `d` into tall room `t` a storey up opens onto a walkway along its wall.
    fn on_walkway(t: &Room, d: &Doorway) -> bool {
        let face = if t.boxes[0].0 .0 == d.hi.0 || t.boxes[0].0 .1 == d.hi.1 { d.hi } else { d.lo };
        let along = 1 - d.axis;
        t.solids.iter().any(|(lo, hi, tag)| {
            *tag == "walkway"
                && hi.2 == d.lo.2
                && (crate::plan::at(*lo, d.axis) == crate::plan::at(face, d.axis) || crate::plan::at(*hi, d.axis) == crate::plan::at(face, d.axis))
                && crate::plan::at(*lo, along) <= crate::plan::at(d.lo, along)
                && crate::plan::at(d.hi, along) <= crate::plan::at(*hi, along)
        })
    }

    #[test]
    fn upper_rooms_beside_a_tall_room_open_onto_its_walkway() {
        let (mut rooms, mut halls) = (0, 0);
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            for d in &lv.doorways {
                let (a, b) = (lv.plan.get(&d.a).unwrap(), lv.plan.get(&d.b).unwrap());
                let (t, u) = if a.kind == "hub" { (a, b) } else { (b, a) };
                if t.kind != "hub" || u.kind != "room" || d.lo.2 == t.floor {
                    continue;
                }
                // a storey up, onto the walkway along that wall, and the room has another way in
                assert_eq!(d.lo.2, t.floor + STOREY, "seed {seed} {}", u.key);
                assert!(on_walkway(t, d), "seed {seed} {} into {}: no walkway at the doorway", u.key, t.key);
                assert!(lv.doorways.iter().filter(|e| e.a == u.key || e.b == u.key).count() >= 2, "seed {seed} {}", u.key);
                rooms += 1;
                halls += (t.key == "hall") as usize;
            }
        }
        println!("{rooms} upper rooms open onto a walkway on seeds 1-40, {halls} onto the hall's");
        assert!(rooms >= 8, "{rooms} upper rooms onto walkways");
    }

    #[test]
    fn circulation_doorways_are_arched_and_the_hall_stands_in_bays() {
        let (mut doorways, mut screens) = (0, 0);
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            let kind = |key: &str| lv.plan.get(key).unwrap().kind;
            // every doorless doorway between corridors, halls and stair rooms is arched, in the
            // doorway's own box: a semicircle on a cell, a segment rising a quarter of the width on more
            for d in &lv.doorways {
                let arched = lv.arches.iter().find(|a| a.kind == "doorway" && a.a == d.a && a.b == d.b);
                let want = d.door.is_none() && kind(&d.a) != "room" && kind(&d.b) != "room";
                assert_eq!(arched.is_some(), want, "seed {seed} {} {}", d.a, d.b);
                if let Some(a) = arched {
                    let (w, h) = d.size();
                    assert_eq!((a.floor, a.top, a.top - a.spring), (d.lo.2, d.lo.2 + h, if w == CELL { w / 2 } else { w / 4 }), "seed {seed}");
                    doorways += 1;
                }
            }
            // the hall: its screens' piers, two each, and three arches a screen
            let hall = lv.plan.get("hall").unwrap();
            let piers = hall.solids.iter().filter(|s| s.2 == "pier").count();
            let arches = lv.arches.iter().filter(|a| a.kind == "screen").count();
            assert!(piers % 2 == 0 && arches == piers / 2 * 3 && piers <= 4, "seed {seed}: {piers} piers, {arches} arches");
            assert!(!hall.solids.iter().any(|s| s.2 == "pillar"), "seed {seed}");
            screens += piers / 2;
        }
        println!("{doorways} arched doorways, {screens} screens in 40 halls");
        // nearly every hall is three bays
        assert!(screens >= 75, "{screens} screens");
    }

    #[test]
    fn tall_rooms_have_walkways_up_high() {
        let (mut tall, mut with, mut two) = (0, 0, 0);
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            for r in lv.plan.rooms.iter().filter(|r| r.kind == "hub") {
                let storeys = (r.hi().2 - r.floor - ROOM_H) / STOREY;
                if storeys < 1 {
                    continue;
                }
                tall += 1;
                let decks: Vec<&Solid> = r.solids.iter().filter(|s| s.2 == "walkway").collect();
                if decks.is_empty() {
                    continue;
                }
                with += 1;
                // a walkway's top is a storey line, its underside a slab below, all in the room's air
                let (lo, hi) = r.boxes[0];
                for (a, b, _) in &decks {
                    assert!([STOREY, 2 * STOREY].contains(&b.2) && b.2 - a.2 == balcony::SLAB, "seed {seed} {}", r.key);
                    assert!(a.0 >= lo.0 && a.1 >= lo.1 && b.0 <= hi.0 && b.1 <= hi.1, "seed {seed} {}", r.key);
                }
                // a room three storeys tall may take two; the hall one
                let top = decks.iter().map(|d| d.1 .2).max().unwrap();
                assert!(top <= STOREY * storeys.min(if r.key == "hall" { 1 } else { 2 }), "seed {seed} {}", r.key);
                two += (top == 2 * STOREY) as usize;
                // twelve steps a storey up to each, railed, and a walk up and along them
                let steps = r.solids.iter().filter(|s| s.2 == "step").count() - if r.key == "hall" { 3 } else { 0 };
                assert_eq!(steps as i32, RISERS * top / STOREY, "seed {seed} {}", r.key);
                assert!(!r.rails.is_empty() && lv.ramp_rails.iter().any(|rr| rr.0 == r.key), "seed {seed} {}", r.key);
                let walk = lv.walks.iter().find(|(name, _)| *name == format!("walkway_{}", r.key)).unwrap_or_else(|| panic!("seed {seed} {}: no walk", r.key));
                assert_eq!(walk.1.last().unwrap().z, top, "seed {seed} {}", r.key);
            }
        }
        println!("{with} of {tall} tall rooms have walkways, {two} of them two");
        // most tall rooms take one (a short hall's pillars leave no room for the flight)
        assert!(with * 4 >= tall * 3 && two >= 3, "{with} of {tall}, {two} with two");
    }

    #[test]
    fn tall_plain_rooms_have_walkways_clear_of_their_walks() {
        let (mut tall, mut with) = (0, 0);
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            for r in lv.plan.rooms.iter().filter(|r| r.kind == "room" && r.hi().2 - r.floor >= ROOM_H + STOREY) {
                tall += 1;
                let decks: Vec<&Solid> = r.solids.iter().filter(|s| s.2 == "walkway").collect();
                if decks.is_empty() {
                    continue;
                }
                with += 1;
                // one walkway a storey up, twelve steps to it, and a walk up and along it (the
                // door-to-door walks, straight lines, are walked with the rest)
                assert!(decks.iter().all(|d| d.1 .2 == r.floor + STOREY), "seed {seed} {}", r.key);
                assert_eq!(r.solids.iter().filter(|s| s.2 == "step").count() as i32, RISERS, "seed {seed} {}", r.key);
                assert!(lv.walks.iter().any(|(name, _)| *name == format!("walkway_{}", r.key)), "seed {seed} {}", r.key);
            }
        }
        println!("{with} of {tall} tall plain rooms have a walkway");
        assert!(tall > 0 && with * 3 >= tall, "{with} of {tall}");
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
