//! Felicity-style levels from a seed: a building cut into rooms, with a big tall hall beside it.
//! Not a port of anything; the numbers are Felicity's own (`assets/stages/mp11`, measured
//! 2026-11-17: docs/AUTOLEVEL.md, "Felicity").
//!
//! ```text
//! let lv = felicity::generate(7, &Options { style: "felicity", ..Options::default() })?;
//! lv.plan                 // rooms (air boxes, steps as solids) and the links between them
//! lv.doorways             // every doorway, and the door in it if it has one
//! lv.walks, lv.views      // routes for the walker, places to look from
//! ```
//!
//! What Felicity is, and so what this makes:
//!
//! - **Rooms off a hallway, and rooms off rooms**, not halls joined by corridors (Felicity has
//!   one corridor in 20 spaces; Complex ten in 33). The building is a rectangle on a 106 grid
//!   (the door's width), a hallway four cells wide runs its length, and the band either side of
//!   it is cut into rooms: a front row on the hallway and, where the band is deep, a back row
//!   reached through the front rooms.
//! - **A tall hall** beside the building (Felicity's three-bay hall, 7.8 m tall), joined to the
//!   hallway's end by a double doorway, with a platform 70 up along its far wall, three steps
//!   up to it, and pillars.
//! - **An upper storey** over one band's front row, 283 up (the storey; rooms are 259 tall,
//!   the slab 24), reached by **stairs** (twelve risers of 23.6, 40 deep) in a stair room at
//!   each end of the row: two ways up, so the upper rooms are a loop, not a dead end.
//! - **Doorways 106 wide and 212 tall** (212 for a double), with a lintel, in walls 40 thick.
//!   **Doors** where Felicity puts them: on 9 of 11 doorways from the hallway or the hall into a
//!   room, 4 of 12 between two rooms, none into a small room; double doorways take two leaves;
//!   swinging to sliding about 2:1.
//!
//! The layout and the doors draw from two random streams (`<seed>/felicity` and
//! `<seed>/felicity/doors`), so a change to how doors are chosen never moves a room. Units are
//! the plan's (x east, y north, z up; 1 unit = 1 cm at the importer's scale 1).

use crate::cells::Cells;
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
/// The hallway's width, in cells.
pub const SPINE: i32 = 4;
/// A stair room's length along its row, in cells: a floor to land on from the hallway (two
/// cells), the flight (480) and the landing at the top.
pub const STAIR: i32 = 8;
/// A room this small (cells of floor) has no door (Felicity's tiny rooms have none).
pub const SMALL: i32 = 12;
/// How far inside a room a walk starts and ends from a doorway's middle plane.
const INSIDE: i32 = 100;

/// What a lot is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Spine,
    Room,
    Hall,
    /// A stair room (its stairs: `stairs::fit`).
    Stair,
}

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

    /// The hallway and the hall are where everyone passes: their doorways into rooms take doors.
    fn hub(&self) -> bool {
        matches!(self.kind, Kind::Spine | Kind::Hall)
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

/// A room's type (its `scheme`: `autolevel_build::felicity::TYPES`) for a draw in [0, 1): offices
/// mostly, then the marble rooms, the industrial ones and the washrooms. (Placeholder shares: a
/// type should follow where a room is and how big.)
fn room_type(roll: f64) -> &'static str {
    match roll {
        r if r < 0.55 => "office",
        r if r < 0.70 => "marble",
        r if r < 0.85 => "industrial",
        _ => "washroom",
    }
}

/// Widths in [lo, hi] cells summing to `len`, at random (`len` at least `lo`).
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

/// Which lots the doorways join to `from` (by place in the lot list).
fn reach(gaps: &[Gap], from: usize, n: usize) -> Vec<bool> {
    let mut seen = vec![false; n];
    seen[from] = true;
    let mut grew = true;
    while grew {
        grew = false;
        for g in gaps {
            if seen[g.a] != seen[g.b] {
                (seen[g.a], seen[g.b]) = (true, true);
                grew = true;
            }
        }
    }
    seen
}

/// The level for a seed (`o` is kept as the level's; Felicity reads none of it yet). Err only if
/// the planner made something its own plan check refuses (a fault here, not the seed's).
pub fn generate(seed: u64, o: &Options) -> Result<Level, String> {
    let mut rng = PyRandom::named(&format!("{seed}/felicity"));
    let mut lots: Vec<Lot> = Vec::new();
    let add = |lots: &mut Vec<Lot>, key: String, r: (i32, i32, i32, i32), floor: i32, top: i32, kind: Kind, scheme| {
        lots.push(Lot { key, x0: r.0, y0: r.1, x1: r.2, y1: r.3, floor, top, kind, scheme });
        lots.len() - 1
    };

    // ---- the building and its hallway
    let (bw, bd) = (rng.randint(26, 32) as i32, rng.randint(19, 24) as i32);
    let sy = rng.randint(6, (bd - SPINE - 6) as i64) as i32;
    let spine = add(&mut lots, "hallway".into(), (0, sy, bw, sy + SPINE), 0, ROOM_H, Kind::Spine, "office");
    // the band upstairs: north of the hallway, or south
    let up_north = rng.random() < 0.5;
    let mut rooms = 0;
    let mut name = |prefix: &str| {
        rooms += 1;
        format!("{prefix}{rooms}")
    };
    let mut front: Vec<Vec<usize>> = Vec::new(); // each band's front row, west to east
    let mut back: Vec<Vec<usize>> = Vec::new();
    let mut upstairs: Vec<usize> = Vec::new();
    let mut stairs: Vec<usize> = Vec::new();
    for north in [false, true] {
        let (r0, r1) = if north { (sy + SPINE, bd) } else { (0, sy) };
        let depth = r1 - r0;
        let df = if depth >= 9 { rng.randint(4, (depth - 4) as i64) as i32 } else { depth };
        // the front row's rows, and the back row's
        let (f0, f1, b0, b1) = if north { (r0, r0 + df, r0 + df, r1) } else { (r1 - df, r1, r0, r1 - df) };
        let up = north == up_north;
        let mut row = Vec::new();
        let mut x = 0;
        let widths = if up {
            let mut w = vec![STAIR];
            w.extend(cuts(&mut rng, bw - 2 * STAIR, 3, 8));
            w.push(STAIR);
            w
        } else {
            cuts(&mut rng, bw, 3, 8)
        };
        let n = widths.len();
        for (i, w) in widths.into_iter().enumerate() {
            let stair = up && (i == 0 || i == n - 1);
            let scheme = room_type(rng.random());
            let (key, top, kind) = if stair { (name("stairs"), STOREY + ROOM_H, Kind::Stair) } else { (name("room"), ROOM_H, Kind::Room) };
            let at = add(&mut lots, key, (x, f0, x + w, f1), 0, top, kind, scheme);
            if stair {
                stairs.push(at);
            }
            row.push(at);
            x += w;
        }
        front.push(row);
        let mut brow = Vec::new();
        if b1 > b0 {
            let mut x = 0;
            for w in cuts(&mut rng, bw, 3, 8) {
                let scheme = room_type(rng.random());
                brow.push(add(&mut lots, name("room"), (x, b0, x + w, b1), 0, ROOM_H, Kind::Room, scheme));
                x += w;
            }
        }
        back.push(brow);
        if up {
            // the upper rooms over the front row, between the stairs
            let mut x = STAIR;
            for w in cuts(&mut rng, bw - 2 * STAIR, 3, 8) {
                let scheme = room_type(rng.random());
                upstairs.push(add(&mut lots, name("upper"), (x, f0, x + w, f1), STOREY, STOREY + ROOM_H, Kind::Room, scheme));
                x += w;
            }
        }
    }

    // ---- the hall, off the hallway's east end
    let (hw, hd) = (rng.randint(14, 18) as i32, rng.randint(14, 18) as i32);
    let hy0 = (sy + SPINE / 2 - hd / 2 + rng.randint(-2, 2) as i32).clamp(sy + SPINE + 1 - hd, sy - 1);
    let hall = add(&mut lots, "hall".into(), (bw, hy0, bw + hw, hy0 + hd), 0, HALL_H, Kind::Hall, "hall");

    // ---- doorways
    let mut gaps: Vec<Gap> = Vec::new();
    // one in the wall i and j share, `wide` cells (or one if that won't fit), on storey z,
    // at k if given; None if no doorway fits
    let gap = |rng: &mut PyRandom, lots: &[Lot], i: usize, j: usize, wide: i32, z: i32, k: Option<i32>| -> Option<Gap> {
        let (lo, hi) = if shared(&lots[i], &lots[j]).is_some() { (i, j) } else { (j, i) };
        let (axis, line, s0, s1) = shared(&lots[lo], &lots[hi])?;
        // a cell of wall at each end
        let fits = |w: i32| s1 - 1 - w >= s0 + 1;
        let wide = if fits(wide) {
            wide
        } else if fits(1) {
            1
        } else {
            return None;
        };
        let k = k.unwrap_or_else(|| rng.randint((s0 + 1) as i64, (s1 - 1 - wide) as i64) as i32);
        Some(Gap { a: lo, b: hi, axis, line, k, wide, z })
    };
    let pick_wide = |rng: &mut PyRandom| if rng.random() < 0.25 { 2 } else { 1 };
    // each stair room's stairs: the first shape and frame (in a seeded order) that leave its
    // foot a doorway on the hallway's wall and its top one on the upper room's
    let mut flights: Vec<(usize, Stairs, i32, i32)> = Vec::new(); // (lot, its stairs, the foot's cell, the top's)
    for &s in &stairs {
        let l = &lots[s];
        let up = if l.x0 == 0 { upstairs[0] } else { upstairs[upstairs.len() - 1] };
        let hall_side = if lots[spine].y1 == l.y0 { 'S' } else { 'N' };
        let up_side = if lots[up].x0 == l.x1 { 'E' } else { 'W' };
        let ((ax0, ay0, _), (ax1, ay1, _)) = l.air();
        let mut shapes = Shape::ALL;
        rng.shuffle(&mut shapes);
        let mut frames = Frame::ALL;
        rng.shuffle(&mut frames);
        let cells = |k: i32| (k * CELL, (k + 1) * CELL);
        let mut found = None;
        'shapes: for shape in shapes {
            for frame in frames {
                let Ok(st) = stair::fit(shape, (ax0, ay0), (ax1, ay1), l.floor, STOREY, frame) else { continue };
                let opens = |spans: &[stair::Span], side: char, k: i32| spans.iter().any(|p| p.side == side && p.holds(cells(k).0, cells(k).1));
                let foot: Vec<i32> = (l.x0 + 1..=l.x1 - 2).filter(|&k| opens(&st.foot, hall_side, k)).collect();
                let (s0, s1) = (l.y0.max(lots[up].y0), l.y1.min(lots[up].y1));
                let top: Vec<i32> = (s0 + 1..=s1 - 2).filter(|&k| opens(&st.top, up_side, k)).collect();
                if !foot.is_empty() && !top.is_empty() {
                    let (f, t) = (*rng.choice(&foot), *rng.choice(&top));
                    found = Some((st, f, t));
                    break 'shapes;
                }
            }
        }
        let (st, f, t) = found.ok_or_else(|| format!("felicity seed {seed}: no stairs fit {}", l.key))?;
        flights.push((s, st, f, t));
    }
    let flight_of = |s: usize| flights.iter().find(|f| f.0 == s);
    // every front room off the hallway (a stair room at its foot)
    for row in &front {
        for &r in row {
            let k = flight_of(r).map(|f| f.2);
            let wide = if k.is_some() { 1 } else { pick_wide(&mut rng) };
            gaps.extend(gap(&mut rng, &lots, spine, r, wide, 0, k));
        }
    }
    // every back room off the front room it shares most wall with
    for (row, brow) in front.iter().zip(&back) {
        for &r in brow {
            let best = row
                .iter()
                .filter(|&&f| !matches!(lots[f].kind, Kind::Stair))
                .filter_map(|&f| shared(&lots[f], &lots[r]).or_else(|| shared(&lots[r], &lots[f])).map(|(_, _, s0, s1)| (s1 - s0, f)))
                .filter(|(n, _)| *n >= 3)
                .max_by_key(|(n, f)| (*n, std::cmp::Reverse(*f)));
            match best {
                Some((_, f)) => {
                    let w = pick_wide(&mut rng);
                    gaps.extend(gap(&mut rng, &lots, f, r, w, 0, None));
                }
                // under a stair room: off its neighbour along the back row instead (below)
                None => {}
            }
        }
    }
    // a back room nothing reaches yet (one behind a stair room): off a neighbour along its row
    // that is reached, until every one is
    loop {
        let reached = reach(&gaps, spine, lots.len());
        let unreached: Vec<(usize, usize)> =
            back.iter().flat_map(|brow| brow.iter().enumerate().filter(|(_, &r)| !reached[r]).map(move |(n, &r)| (n, r))).collect();
        if unreached.is_empty() {
            break;
        }
        let mut joined = false;
        'rooms: for brow in &back {
            for (n, &r) in brow.iter().enumerate().filter(|(_, &r)| !reached[r]) {
                for o in [n.checked_sub(1), Some(n + 1)].into_iter().flatten().filter_map(|m| brow.get(m).copied()) {
                    if reached[o] {
                        if let Some(g) = gap(&mut rng, &lots, o, r, 1, 0, None) {
                            gaps.push(g);
                            joined = true;
                            break 'rooms;
                        }
                    }
                }
            }
        }
        if !joined {
            return Err(format!("felicity seed {seed}: no way into {}", lots[unreached[0].1].key));
        }
    }
    // side doors along a row: the upper rooms in a chain, the others now and then (loops)
    for (row, always) in front.iter().chain(&back).map(|r| (r, false)).chain(std::iter::once((&upstairs, true))) {
        for pair in row.windows(2) {
            let (i, j) = (pair[0], pair[1]);
            if matches!(lots[i].kind, Kind::Stair) || matches!(lots[j].kind, Kind::Stair) {
                continue;
            }
            if always || rng.random() < 0.3 {
                if let Some(g) = gap(&mut rng, &lots, i, j, 1, lots[i].floor, None) {
                    if !gaps.iter().any(|h| (h.a, h.b) == (g.a, g.b)) {
                        gaps.push(g);
                    }
                }
            }
        }
    }
    // the stairs' landings onto the upper rooms
    for &s in &stairs {
        let up = if lots[s].x0 == 0 { upstairs[0] } else { upstairs[upstairs.len() - 1] };
        gaps.extend(gap(&mut rng, &lots, s, up, 1, STOREY, flight_of(s).map(|f| f.3)));
    }
    // the hall: off the hallway's end (double), and from a room at the building's east end
    gaps.extend(gap(&mut rng, &lots, spine, hall, 2, 0, Some(sy + 1)));
    let east: Vec<usize> = front
        .iter()
        .chain(&back)
        .flat_map(|row| row.last().copied())
        .filter(|&r| lots[r].kind == Kind::Room && shared(&lots[r], &lots[hall]).is_some_and(|(_, _, s0, s1)| s1 - s0 >= 3))
        .collect();
    if !east.is_empty() && rng.random() < 0.8 {
        let r = *rng.choice(&east);
        gaps.extend(gap(&mut rng, &lots, r, hall, 1, 0, None));
    }

    // ---- doors: their own stream
    let mut drng = PyRandom::named(&format!("{seed}/felicity/doors"));
    let mut doorways = Vec::new();
    for g in &gaps {
        let (a, b) = (&lots[g.a], &lots[g.b]);
        let hub = a.hub() || b.hub();
        let p = if a.small() || b.small() {
            0.0
        } else if hub {
            0.8
        } else {
            0.33
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
            Kind::Spine => "corridor",
            Kind::Hall => "hub",
            Kind::Stair => "stair",
            _ => "room",
        };
        room.scheme = l.scheme;
        room.walkable = vec!["step", "landing", "platform"];
        let ((ax0, ay0, _), (ax1, ay1, _)) = l.air();
        match l.kind {
            Kind::Stair => {
                let (_, st, _, _) = flight_of(n).expect("a stair room's stairs");
                room.solids.extend(st.solids.iter().copied());
                room.rails.extend(st.rails.iter().copied());
                ramp_rails.extend(st.sloped_rails(&l.key));
                room.note = format!("{} stairs", st.shape.name());
            }
            Kind::Hall => {
                // the platform along the east wall, three steps up to its middle third
                let px = ax1 - 4 * CELL;
                room.solids.push(((px, ay0, 0), (ax1, ay1, PLATFORM), "platform"));
                let third = (ay1 - ay0) / 3;
                for j in 0..3 {
                    let top = (((j + 1) * PLATFORM) as f64 / 3.0).round() as i32;
                    let x0 = px - (3 - j) * GOING;
                    room.solids.push(((x0, ay0 + third, 0), (x0 + GOING, ay1 - third, top), "step"));
                }
                // pillars, floor to ceiling, in two rows clear of the middle
                let cols = 3;
                for c in 0..cols {
                    for yrow in [1, 3] {
                        let x = ax0 + (px - ax0) * (c + 1) / (cols + 1);
                        let y = ay0 + (ay1 - ay0) * yrow / 4;
                        let h = CELL / 2;
                        room.solids.push(((x - h, y - h, 0), (x + h, y + h, HALL_H), "pillar"));
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
    let inside = |d: &Doorway, low: bool| -> (f64, f64, i32) {
        let mid = d.plane();
        let off = if low { -INSIDE } else { INSIDE };
        let across = 1 - d.axis;
        let centre = (crate::plan::at(d.lo, across) + crate::plan::at(d.hi, across)) / 2;
        match d.axis {
            0 => (c(mid + off), c(centre), d.lo.2),
            _ => (c(centre), c(mid + off), d.lo.2),
        }
    };
    for d in &doorways {
        let (a, b) = (inside(d, true), inside(d, false));
        let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, d.lo.2);
        walks.push((format!("door_{}_{}", d.a, d.b), vec![Step::at(a), Step::by(m, 1), Step::at(b)]));
    }
    // each plain room door to door (its floor is bare)
    for l in lots.iter().filter(|l| matches!(l.kind, Kind::Room | Kind::Spine)) {
        let ends: Vec<(f64, f64, i32)> =
            doorways.iter().filter_map(|d| (d.a == l.key).then(|| inside(d, true)).or_else(|| (d.b == l.key).then(|| inside(d, false)))).collect();
        for i in 0..ends.len() {
            for j in i + 1..ends.len() {
                walks.push((format!("room_{}_{i}_{j}", l.key), vec![Step::at(ends[i]), Step::at(ends[j])]));
            }
        }
    }
    // up each flight, from the hallway's doorway to the landing's
    for (s, st, _, _) in &flights {
        let l = &lots[*s];
        let below = doorways.iter().find(|d| (d.a == l.key || d.b == l.key) && d.lo.2 == l.floor);
        let above = doorways.iter().find(|d| (d.a == l.key || d.b == l.key) && d.lo.2 == l.floor + STOREY);
        if let (Some(below), Some(above)) = (below, above) {
            let start = inside(below, below.a == l.key);
            let top = inside(above, above.a == l.key);
            let mut pts = vec![Step::at(start)];
            pts.extend(st.path.iter().map(|p| Step::by((c(p.0), c(p.1), p.2), 1)));
            pts.push(Step::at(top));
            walks.push((format!("stairs_{}", l.key), pts));
        }
    }
    // the hall's platform, from the hallway's doorway up its steps
    {
        let l = &lots[hall];
        let ((_, ay0, _), (ax1, ay1, _)) = l.air();
        let px = ax1 - 4 * CELL;
        let ym = c((ay0 + ay1) / 2);
        if let Some(d) = doorways.iter().find(|d| d.a == "hallway" && d.b == "hall") {
            // along the west wall to the middle (the pillars stand a quarter in), then east
            let start = inside(d, false);
            let pts =
                vec![Step::at(start), Step::by((start.0, ym, 0), 1), Step::by((c(px - 3 * GOING - 80), ym, 0), 1), Step::at((c(px + 2 * CELL), ym, PLATFORM))];
            walks.push(("platform".to_string(), pts));
        }
    }
    let mut views: Vec<(String, View)> = Vec::new();
    for (n, l) in lots.iter().enumerate() {
        let ((x0, y0, _), (x1, y1, _)) = l.air();
        // a stair room from the foot of its stairs, looking at the top landing (its corner may
        // be inside a landing)
        let view = match flight_of(n).map(|f| (f.1.path.first().copied(), f.1.path.last().copied())) {
            Some((Some(foot), Some(top))) => (c(foot.0), c(foot.1), l.floor, c(top.0), c(top.1)),
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

    fn generate(seed: u64) -> Result<Level, String> {
        super::generate(seed, &Options { style: "felicity", ..Options::default() })
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
        let (mut hub, mut hub_doors, mut other, mut other_doors, mut leaves, mut swing) = (0, 0, 0, 0, 0, 0);
        for seed in 1..=40 {
            let lv = generate(seed).unwrap();
            for d in &lv.doorways {
                let is_hub = ["hallway", "hall"].contains(&d.a.as_str()) || ["hallway", "hall"].contains(&d.b.as_str());
                let (n, with) = if is_hub { (&mut hub, &mut hub_doors) } else { (&mut other, &mut other_doors) };
                *n += 1;
                if let Some(k) = d.door {
                    *with += 1;
                    leaves += k.leaves as usize;
                    swing += k.swing as usize;
                    // a door's doorway is a door's size: 106 a leaf, 212 tall
                    assert_eq!(d.size(), (CELL * k.leaves as i32, DOOR_H));
                }
            }
        }
        let share = |a: usize, b: usize| a as f64 / b as f64;
        assert!((0.6..0.9).contains(&share(hub_doors, hub)), "{hub_doors} of {hub}");
        assert!((0.15..0.4).contains(&share(other_doors, other)), "{other_doors} of {other}");
        assert!(leaves > hub_doors + other_doors && swing > 0);
    }

    /// The level of corridors with rooms as offshoots the user asked for (`circulation::TARGETS`),
    /// on seeds 1-60. Ignored until a planner meets it: `cargo test -p autolevel --release
    /// felicity_meets_the_circulation_targets -- --ignored --nocapture` prints every seed's
    /// measures and what each misses.
    #[test]
    #[ignore]
    fn felicity_meets_the_circulation_targets() {
        use crate::circulation::{of, TARGETS};
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
        let mut seen = std::collections::BTreeMap::new();
        for seed in 1..=40 {
            for r in generate(seed).unwrap().plan.rooms.iter().filter(|r| r.key.starts_with("stairs")) {
                *seen.entry(r.note.clone()).or_insert(0) += 1;
                if r.note.starts_with('U') || seed < 4 {
                    println!("seed {seed}: {} {}", r.key, r.note);
                }
            }
        }
        println!("{seen:?}");
        assert_eq!(seen.len(), 3, "{seen:?}");
    }

    #[test]
    fn the_stairs_rise_a_storey_in_twelve_steps() {
        let lv = generate(3).unwrap();
        let stairs: Vec<&Room> = lv.plan.rooms.iter().filter(|r| r.key.starts_with("stairs")).collect();
        assert_eq!(stairs.len(), 2);
        for r in stairs {
            let steps: Vec<i32> = r.solids.iter().filter(|s| s.2 == "step").map(|s| s.1 .2).collect();
            assert_eq!(steps.len(), 12);
            assert_eq!((steps[0], steps[11]), (24, 283));
            assert!(steps.windows(2).all(|w| (23..=24).contains(&(w[1] - w[0]))));
            // a doorway at its foot and one off its landing
            let ways: Vec<i32> = lv.doorways.iter().filter(|d| d.a == r.key || d.b == r.key).map(|d| d.lo.2).collect();
            assert_eq!(ways.len(), 2, "{}", r.key);
            assert!(ways.contains(&0) && ways.contains(&STOREY));
        }
    }
}
