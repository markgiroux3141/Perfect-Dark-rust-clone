//! A Felicity level's dressing: the detail that stands in its rooms once they are planned, on
//! its own random streams (`<seed>/felicity/dress/...`) so that dressing never moves a room.
//!
//! **Ceiling braces** (measured 2026-11-17 from Felicity's rooms 4 and 17, its two biggest
//! blue-floor rooms, about 8 x 6 cells): a cross of beams under the ceiling, each 40 wide and 24
//! deep (the underside 235 up a 259 room), on a pilaster 13 deep and 40 wide at each end, floor to
//! ceiling. The beams' sides are the plain panel 024B, the pilasters the tile 0256, and the
//! undersides 0262: a strip of four tiles, the third a light panel, about two repeats a span from
//! each wall in, baked full white (254), with a bright pool on the floor under the cross. Here:
//! every one-storey office or marble room 5 cells or more each way, `BRACED` of them, gets beams
//! both ways, as many across each span as it is long (`EVERY`), and a lamp under each light panel
//! in place of the room's lamp grid. A pilaster keeps `MARGIN` off every doorway's opening in its
//! wall (a beam slides along `SLIDE` at most to clear one, or is left out).

use crate::felicity::{ROOM_H, WALL};
use crate::level::Doorway;
use crate::plan::{at, Plan};
use crate::pyrand::PyRandom;

/// How wide a beam (and its pilasters) is.
pub const BEAM_W: i32 = 40;
/// How far a beam drops from the ceiling.
pub const BEAM_D: i32 = 24;
/// How far a pilaster stands out from its wall.
pub const PILASTER: i32 = 13;
/// About how long one repeat of the underside's strip is (one light panel each), whole repeats
/// to an arm: Felicity's arms (278 to 396 long) hold two each, 139 to 198 a repeat.
pub const PANEL: f64 = 180.0;
/// Where in a repeat of the strip its light panel is: the third of its four tiles.
pub const LIGHT: (f64, f64) = (0.5, 0.75);
/// How much of a room's span each beam across it takes: one beam to every `EVERY` past the first.
pub const EVERY: f64 = 424.0;
/// The least span (inside the walls) both ways for a room to be braced: 5 cells.
pub const LEAST: i32 = 5 * 106 - WALL;
/// How far apart a corridor's ribs are (3 cells): not Felicity's (its hallway has none), CI
/// Training's corridors' rhythm, in Felicity's brace.
pub const RIB: f64 = 318.0;
/// The share of the rooms that could be braced that are.
pub const BRACED: f64 = 0.7;
/// How far a pilaster keeps off a doorway's opening in its wall (the frame's 13 and more).
pub const MARGIN: i32 = 40;
/// How far a beam may slide from its even place to clear the doorways, in steps of 20.
pub const SLIDE: i32 = 160;

/// A beam under a room's ceiling, wall to wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Brace {
    pub room: String,
    /// The axis it runs along (0: x, 1: y).
    pub axis: usize,
    /// Its middle across that.
    pub at: i32,
    /// Wall to wall along it (the room's air).
    pub t: (i32, i32),
    pub floor: i32,
    /// The ceiling it hangs from.
    pub top: i32,
}

/// A stretch of a beam between its walls and the beams crossing it.
#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    pub room: String,
    pub axis: usize,
    pub at: i32,
    /// From where to where along the axis: a wall's face, or a crossing beam's side.
    pub t: (i32, i32),
    /// Whether each end is at a wall (a pilaster there).
    pub walls: (bool, bool),
    pub floor: i32,
    pub top: i32,
}

impl Arm {
    /// The end its strip starts from (u 0): a wall's (the low one if both or neither).
    pub fn from_high(&self) -> bool {
        self.walls.1 && !self.walls.0
    }

    /// How many repeats of the strip it holds: whole ones, about `PANEL` long.
    pub fn repeats(&self) -> usize {
        (((self.t.1 - self.t.0) as f64 / PANEL).round() as usize).max(1)
    }

    /// Its light panels' middles along the axis.
    pub fn panels(&self) -> Vec<f64> {
        let (n, len) = (self.repeats(), (self.t.1 - self.t.0) as f64);
        let each = len / n as f64;
        let mid = (LIGHT.0 + LIGHT.1) / 2.0;
        (0..n).map(|k| (k as f64 + mid) * each).map(|d| if self.from_high() { self.t.1 as f64 - d } else { self.t.0 as f64 + d }).collect()
    }
}

/// Where two beams cross: the square under both (its middle).
#[derive(Clone, Debug, PartialEq)]
pub struct Crossing {
    pub room: String,
    pub x: i32,
    pub y: i32,
    pub top: i32,
}

/// Each room's beams cut at the beams crossing them: the arms, and the squares where they cross.
pub fn arms(braces: &[Brace]) -> (Vec<Arm>, Vec<Crossing>) {
    let (mut arms, mut crossings) = (Vec::new(), Vec::new());
    for b in braces {
        let h = BEAM_W / 2;
        let mut cuts: Vec<i32> = braces.iter().filter(|c| c.room == b.room && c.axis != b.axis).map(|c| c.at).collect();
        cuts.sort();
        let mut from = (b.t.0, true);
        for &c in &cuts {
            arms.push(Arm { room: b.room.clone(), axis: b.axis, at: b.at, t: (from.0, c - h), walls: (from.1, false), floor: b.floor, top: b.top });
            from = (c + h, false);
            if b.axis == 0 {
                crossings.push(Crossing { room: b.room.clone(), x: c, y: b.at, top: b.top });
            }
        }
        arms.push(Arm { room: b.room.clone(), axis: b.axis, at: b.at, t: (from.0, b.t.1), walls: (from.1, true), floor: b.floor, top: b.top });
    }
    (arms, crossings)
}

/// A lamp under each light panel: (room, the point), just under the beam.
pub fn lamps(arms: &[Arm]) -> Vec<(String, (i32, i32, i32))> {
    let mut out = Vec::new();
    for a in arms {
        for t in a.panels() {
            let t = t.round() as i32;
            let z = a.top - BEAM_D - 10;
            out.push((a.room.clone(), if a.axis == 0 { (t, a.at, z) } else { (a.at, t, z) }));
        }
    }
    out
}

/// Where along a wall (from `lo` to `hi` on `axis`) a pilaster `BEAM_W` wide may stand: clear of
/// the room's corners and of every doorway's opening in the walls it stands against.
fn clear(c: i32, lo: i32, hi: i32, openings: &[(i32, i32)]) -> bool {
    let (a, b) = (c - BEAM_W / 2, c + BEAM_W / 2);
    a >= lo + 3 * BEAM_W / 2 && b <= hi - 3 * BEAM_W / 2 && openings.iter().all(|&(s0, s1)| b <= s0 - MARGIN || a >= s1 + MARGIN)
}

/// The braces of a Felicity level: in the rooms of `plan` that may have them (`BRACED` of them,
/// drawn in the plan's order), beams both ways, and in every one-storey office or marble corridor
/// ribs across it every `RIB`, each where it clears its walls' doorways.
pub fn braces(seed: u64, plan: &Plan, doorways: &[Doorway]) -> Vec<Brace> {
    let mut rng = PyRandom::named(&format!("{seed}/felicity/dress/braces"));
    let mut out = Vec::new();
    for r in &plan.rooms {
        let Some(&(lo, hi)) = r.boxes.first() else { continue };
        let (lo, hi) = ([lo.0, lo.1, lo.2], [hi.0, hi.1, hi.2]);
        let blue = matches!(r.scheme, "office" | "marble") && hi[2] - lo[2] == ROOM_H;
        // a corridor's ribs: across its short span only, every `RIB` along it, none drawn
        let ribbed = r.kind == "corridor" && blue;
        let may = r.kind == "room" && blue && (0..2).all(|k| hi[k] - lo[k] >= LEAST);
        if !ribbed && (!may || rng.random() >= BRACED) {
            continue;
        }
        let long = if hi[0] - lo[0] >= hi[1] - lo[1] { 0 } else { 1 };
        for k in (0..2).filter(|&k| !ribbed || k == long) {
            // beams placed along `k`, each running along the other axis, wall to wall
            let run = 1 - k;
            let span = hi[k] - lo[k];
            let every = if ribbed { RIB } else { EVERY };
            let n = (((span as f64 / every).round() as i32) - 1).max(1);
            // the openings in the two walls the beams meet
            let openings: Vec<(i32, i32)> = doorways
                .iter()
                .filter(|d| d.axis == run && (at(d.lo, run) == hi[run] || at(d.hi, run) == lo[run]) && d.lo.2 < hi[2] && d.hi.2 > lo[2])
                .filter(|d| d.a == r.key || d.b == r.key)
                .map(|d| (at(d.lo, k), at(d.hi, k)))
                .collect();
            for i in 0..n {
                let even = lo[k] + span * (i + 1) / (n + 1);
                let slides = (0..=SLIDE / 20).flat_map(|s| [s * 20, -s * 20]);
                if let Some(c) = slides.map(|s| even + s).find(|&c| clear(c, lo[k], hi[k], &openings)) {
                    out.push(Brace { room: r.key.clone(), axis: run, at: c, t: (lo[run], hi[run]), floor: lo[2], top: hi[2] });
                }
            }
        }
    }
    out
}

/// How far a hazard surround stands out of its wall (Felicity's room 29: its stripes run 39 deep).
pub const SURROUND: i32 = 39;
/// How far its stripes flare beside the opening, and above it, over that depth (14 and 12).
pub const FLARE: (i32, i32) = (14, 12);
/// How far beside the opening, and above it, its splay meets the wall again (the wall's tile).
pub const SPLAY: (i32, i32) = (54, 35);
/// How far a surround keeps off the room's corners and any other doorway's frame in its wall.
pub const SURROUND_GAP: i32 = 6;

/// A hazard surround (Felicity's grey bays, its rooms 29 and 32): on one face of a doorway, its
/// opening flares in yellow and black stripes `SURROUND` out of the wall, `FLARE` wider and taller,
/// and the wall's tile splays back from there to the wall `SPLAY` beside and above the opening.
#[derive(Clone, Debug, PartialEq)]
pub struct Surround {
    /// The doorway (its two rooms) and the room whose face it is on.
    pub a: String,
    pub b: String,
    pub room: String,
}

impl Surround {
    /// Whether it stands on the doorway's low face (room `a`'s).
    pub fn low(&self) -> bool {
        self.room == self.a
    }
}

/// How far a doorway's frame reaches beside its opening on `room`'s face: the surround's splay,
/// or the chamfer's 13.
pub fn frame_width(surrounds: &[Surround], d: &Doorway, room: &str) -> i32 {
    if surrounds.iter().any(|s| s.a == d.a && s.b == d.b && s.room == room) {
        SPLAY.0
    } else {
        13
    }
}

/// The hazard surrounds of a Felicity level: on every face of a doorway with no arch that is in a
/// one-storey industrial room or corridor (Felicity's grey bays), where it fits on the wall: clear
/// of the room's corners and of the frames of the other doorways in that wall. No draw.
pub fn surrounds(plan: &Plan, doorways: &[Doorway], arched: &dyn Fn(&Doorway) -> bool) -> Vec<Surround> {
    let mut out = Vec::new();
    for d in doorways.iter().filter(|d| !arched(d)) {
        for room in [&d.a, &d.b] {
            let Some(r) = plan.get(room) else { continue };
            let Some(&(lo, hi)) = r.boxes.first() else { continue };
            let one = hi.2 - lo.2 == ROOM_H && d.lo.2 == lo.2;
            if !(matches!(r.kind, "room" | "corridor") && r.scheme == "industrial" && one) {
                continue;
            }
            // along the wall: the room's span, and every other doorway's opening in the same wall
            let along = 1 - d.axis;
            let (lo, hi) = ([lo.0, lo.1], [hi.0, hi.1]);
            let (s0, s1) = (at(d.lo, along), at(d.hi, along));
            let mut fits = s0 - SPLAY.0 - SURROUND_GAP >= lo[along] && s1 + SPLAY.0 + SURROUND_GAP <= hi[along];
            for e in doorways.iter().filter(|e| *e != d && e.axis == d.axis && at(e.lo, d.axis) == at(d.lo, d.axis) && (e.a == *room || e.b == *room)) {
                let (e0, e1) = (at(e.lo, along), at(e.hi, along));
                // the other's frame may be a surround too: room for both
                let gap = SPLAY.0 * 2 + SURROUND_GAP;
                fits &= e1 + gap <= s0 || e0 >= s1 + gap;
            }
            if fits {
                out.push(Surround { a: d.a.clone(), b: d.b.clone(), room: room.clone() });
            }
        }
    }
    out
}

/// A portal across an industrial corridor (Felicity's grey bays, rooms 29-32: the openings between
/// its bays). How deep it is along the corridor: Felicity's passage 79 and a bevel 13 each side.
pub const PORTAL_D: i32 = 105;
pub const BEVEL: i32 = 13;
/// Its opening: `POST` in from each wall at the floor, straight up `JAMB`, then leaning in `LEAN`
/// up to its lintel at `LINTEL` (Felicity's: 47 straight, 53 in over 188, the lintel at 235, in a
/// bay 529 wide). Narrower here, so that a walker (radius 30, 185 tall) on a door-to-door line 80
/// off the wall clears it: 42 in at 185.
pub const POST: i32 = 13;
pub const JAMB: i32 = 47;
pub const LEAN: i32 = 40;
pub const LINTEL: i32 = 235;
/// About how far apart along a corridor (Felicity's bays: 475; here 4 cells).
pub const BAY: f64 = 424.0;
/// How far a portal keeps off the corridor's ends, and off the frame of any doorway in its walls.
pub const PORTAL_END: i32 = 106;
pub const PORTAL_MARGIN: i32 = 40;

/// A portal across a corridor: a frame `PORTAL_D` deep with a leaning opening under a glowing
/// lintel.
#[derive(Clone, Debug, PartialEq)]
pub struct Portal {
    pub room: String,
    /// The axis the corridor runs along, and the portal's middle along it.
    pub axis: usize,
    pub at: i32,
    /// Wall to wall across the corridor.
    pub across: (i32, i32),
    pub floor: i32,
    pub top: i32,
}

impl Portal {
    /// The light under its lintel: (room, the point).
    pub fn lamp(&self) -> (String, (i32, i32, i32)) {
        let (m, z) = ((self.across.0 + self.across.1) / 2, self.floor + LINTEL - 10);
        (self.room.clone(), if self.axis == 0 { (self.at, m, z) } else { (m, self.at, z) })
    }
}

/// The portals of a Felicity level: across every one-storey industrial corridor, one every `BAY`
/// or so along it (`round(length / BAY) - 1`, evenly), each slid up to `SLIDE` to keep
/// `PORTAL_MARGIN` off the frames of the doorways in its walls and `PORTAL_END` off its ends, or
/// left out. No draw.
pub fn portals(plan: &Plan, doorways: &[Doorway], surrounds: &[Surround]) -> Vec<Portal> {
    let mut out = Vec::new();
    for r in plan.rooms.iter().filter(|r| r.kind == "corridor" && r.scheme == "industrial") {
        let Some(&(lo, hi)) = r.boxes.first() else { continue };
        if hi.2 - lo.2 != ROOM_H {
            continue;
        }
        let (lo, hi) = ([lo.0, lo.1, lo.2], [hi.0, hi.1, hi.2]);
        let k = if hi[0] - lo[0] >= hi[1] - lo[1] { 0 } else { 1 };
        let o = 1 - k;
        // the doorways in its side walls, with their frames, along it
        let openings: Vec<(i32, i32)> = doorways
            .iter()
            .filter(|d| d.axis == o && (at(d.lo, o) == hi[o] || at(d.hi, o) == lo[o]) && (d.a == r.key || d.b == r.key))
            .map(|d| {
                let w = frame_width(surrounds, d, &r.key);
                (at(d.lo, k) - w, at(d.hi, k) + w)
            })
            .collect();
        let half = PORTAL_D / 2;
        let clear = |c: i32| {
            c - half >= lo[k] + PORTAL_END
                && c + half <= hi[k] - PORTAL_END
                && openings.iter().all(|&(s0, s1)| c + half <= s0 - PORTAL_MARGIN || c - half >= s1 + PORTAL_MARGIN)
        };
        let span = hi[k] - lo[k];
        let n = ((span as f64 / BAY).round() as i32) - 1;
        for i in 0..n {
            let even = lo[k] + span * (i + 1) / (n + 1);
            let slides = (0..=SLIDE / 20).flat_map(|s| [s * 20, -s * 20]);
            if let Some(c) = slides.map(|s| even + s).find(|&c| clear(c)) {
                out.push(Portal { room: r.key.clone(), axis: k, at: c, across: (lo[o], hi[o]), floor: lo[2], top: hi[2] });
            }
        }
    }
    out
}

/// The washrooms' fittings, measured from Felicity's washroom (its room 12): stalls 105 wide
/// between partitions `PARTITION` thick and 212 tall (`STALL_H`), 238 deep (`STALL_D` at most;
/// less in a shallow room); in each a toilet, its rim 77 deep and 71 wide (`TOILET`), `TOILET_GAP`
/// off the wall; basins 122 wide standing `BASIN_D` out of the wall.
pub const PARTITION: i32 = 14;
pub const STALL: i32 = 105;
pub const STALL_H: i32 = 212;
pub const STALL_D: i32 = 238;
/// The least a stall may be deep (a toilet, 91 from the wall, and a little more), and the steps
/// it is made shallower by to fit.
pub const STALL_LEAST: i32 = 110;
pub const STALL_STEP: i32 = 20;
pub const TOILET: (i32, i32) = (71, 77);
pub const TOILET_GAP: i32 = 14;
pub const BASIN: i32 = 122;
pub const BASIN_D: i32 = 53;
/// How far apart basins are along their wall, middle to middle, at least.
pub const BASIN_EVERY: i32 = 150;
/// How far a fitting keeps off a walk's line (a walker's radius, 30, and 10 more).
pub const FITTING_PAD: f64 = 40.0;

/// A fitting standing against a wall: its middle on the wall's face (`at`, plan x, y), facing
/// `out` into the room (a unit along x or y), `width` along the wall and `depth` out of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitting {
    pub room: String,
    /// "partition", "toilet" or "basin".
    pub kind: &'static str,
    pub at: (i32, i32),
    pub out: (i32, i32),
    pub width: i32,
    pub depth: i32,
    pub floor: i32,
}

impl Fitting {
    /// Its footprint on the plan: (x0, y0, x1, y1).
    pub fn footprint(&self) -> (i32, i32, i32, i32) {
        let ((x, y), (dx, dy), h) = (self.at, self.out, self.width / 2);
        let (ax, ay) = (x + dx * self.depth, y + dy * self.depth);
        if dx != 0 {
            (x.min(ax), y - h, x.max(ax), y + self.width - h)
        } else {
            (x - h, y.min(ay), x + self.width - h, y.max(ay))
        }
    }
}

/// The fittings of a Felicity level's washrooms (one-storey rooms of type `washroom`): along the
/// longest wall with no doorway that holds two stalls or more, a row of open-fronted stalls (as
/// many as the wall holds, else two or more from a corner), a toilet in each; along the longest
/// other wall with no doorway, as many basins as fit. Each keeps off the doorways' mouths and the
/// room's walks (every doorway to every other, in straight lines). No draw.
pub fn washrooms(plan: &Plan, doorways: &[Doorway]) -> Vec<Fitting> {
    use crate::felicity::{crosses, INSIDE, MOUTH};
    let mut out = Vec::new();
    for r in plan.rooms.iter().filter(|r| r.scheme == "washroom" && r.kind == "room") {
        let Some(&(lo, hi)) = r.boxes.first() else { continue };
        if hi.2 - lo.2 != ROOM_H {
            continue;
        }
        let doors: Vec<&Doorway> = doorways.iter().filter(|d| (d.a == r.key || d.b == r.key) && d.lo.2 == lo.2).collect();
        // the walks' points (`INSIDE` in from each doorway's middle) and their lines
        let ends: Vec<(f64, f64)> = doors
            .iter()
            .map(|d| {
                let off = if d.a == r.key { -INSIDE } else { INSIDE };
                let across = 1 - d.axis;
                let centre = (at(d.lo, across) + at(d.hi, across)) as f64 / 2.0;
                let mid = (d.plane() + off) as f64;
                if d.axis == 0 {
                    (mid, centre)
                } else {
                    (centre, mid)
                }
            })
            .collect();
        let lines: Vec<((f64, f64), (f64, f64))> =
            (0..ends.len()).flat_map(|i| (i + 1..ends.len()).map(move |j| (i, j))).map(|(i, j)| (ends[i], ends[j])).collect();
        // each doorway's mouth: its opening and frame, `MOUTH` into the room
        let mouths: Vec<(i32, i32, i32, i32)> = doors
            .iter()
            .map(|d| {
                let across = 1 - d.axis;
                let (s0, s1) = (at(d.lo, across) - 13, at(d.hi, across) + 13);
                let (t0, t1) = if d.a == r.key { (at(d.lo, d.axis) - MOUTH, at(d.lo, d.axis)) } else { (at(d.hi, d.axis), at(d.hi, d.axis) + MOUTH) };
                if d.axis == 0 {
                    (t0, s0, t1, s1)
                } else {
                    (s0, t0, s1, t1)
                }
            })
            .collect();
        let meets = |a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)| a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3;
        let clear = |f: (i32, i32, i32, i32)| !mouths.iter().any(|&m| meets(f, m)) && !lines.iter().any(|&(p, q)| crosses(p, q, f, FITTING_PAD));
        // the walls: (axis of its normal, its face, out into the room, its span along it)
        let walls = [(1, lo.1, 1, (lo.0, hi.0)), (0, hi.0, -1, (lo.1, hi.1)), (1, hi.1, -1, (lo.0, hi.0)), (0, lo.0, 1, (lo.1, hi.1))];
        let free: Vec<usize> = (0..4)
            .filter(|&w| {
                let (n, face, _, _) = walls[w];
                !doors.iter().any(|d| d.axis == n && (at(d.lo, n) == face || at(d.hi, n) == face))
            })
            .collect();
        let mut by_length = free.clone();
        by_length.sort_by_key(|&w| -(walls[w].3 .1 - walls[w].3 .0));
        // a fitting on wall `w`, its middle `s` along it
        let fitting = |w: usize, kind: &'static str, s: i32, width: i32, depth: i32| {
            let (n, face, o, _) = walls[w];
            let (at, out) = if n == 1 { ((s, face), (0, o)) } else { ((face, s), (o, 0)) };
            Fitting { room: r.key.clone(), kind, at, out, width, depth, floor: lo.2 }
        };
        let room_depth = |w: usize| if walls[w].0 == 1 { hi.1 - lo.1 } else { hi.0 - lo.0 };
        // the stalls
        let mut stall_wall = None;
        let mut row = Vec::new();
        for &w in &by_length {
            let deepest = (STALL_D.min(room_depth(w) - MOUTH) / 10) * 10;
            if deepest < STALL_LEAST {
                continue;
            }
            let (a, b) = walls[w].3;
            let most = (b - a + PARTITION) / (STALL + PARTITION);
            // (from, to, stalls, a partition at the low end, at the high end)
            let mut tries = vec![(a, b, most, false, false)];
            for n in (1..most).rev() {
                let span = n * (STALL + PARTITION);
                tries.push((a, a + span, n, false, true));
                tries.push((b - span, b, n, true, false));
            }
            // the most stalls first, each as deep as fits
            let depths: Vec<i32> = (0..).map(|k| deepest - k * STALL_STEP).take_while(|&d| d >= STALL_LEAST).collect();
            for ((s0, s1, n, low, high), deep) in tries.into_iter().filter(|t| t.2 >= 1).flat_map(|t| depths.iter().map(move |&d| (t, d))) {
                let mut pieces = Vec::new();
                // the partitions between stalls (and at an end that is no corner), the stalls' width
                let inner = (s1 - s0) - PARTITION * (n - 1 + low as i32 + high as i32);
                let mut s = s0 as f64 + if low { PARTITION as f64 } else { 0.0 };
                if low {
                    pieces.push(fitting(w, "partition", s0 + PARTITION / 2, PARTITION, deep));
                }
                for k in 0..n {
                    let width = inner as f64 / n as f64;
                    pieces.push(fitting(w, "toilet", (s + width / 2.0).round() as i32, TOILET.0, TOILET_GAP + TOILET.1));
                    s += width;
                    if k + 1 < n || high {
                        pieces.push(fitting(w, "partition", (s + PARTITION as f64 / 2.0).round() as i32, PARTITION, deep));
                        s += PARTITION as f64;
                    }
                }
                let foot = {
                    let f = fitting(w, "row", (s0 + s1) / 2, s1 - s0, deep);
                    f.footprint()
                };
                if clear(foot) {
                    row = pieces;
                    stall_wall = Some((w, foot));
                    break;
                }
            }
            if stall_wall.is_some() {
                break;
            }
        }
        out.extend(row);
        // the basins: on the longest other free wall where any fit
        for &w in by_length.iter().filter(|&&w| stall_wall.map(|s| s.0) != Some(w)) {
            let (a, b) = walls[w].3;
            let most = (b - a) / BASIN_EVERY;
            let placed = (1..=most).rev().find_map(|k| {
                let basins: Vec<Fitting> = (0..k).map(|i| fitting(w, "basin", a + (b - a) * (2 * i + 1) / (2 * k), BASIN, BASIN_D)).collect();
                let ok = basins.iter().all(|f| {
                    let foot = f.footprint();
                    clear(foot) && stall_wall.is_none_or(|(_, s)| !meets((foot.0 - 10, foot.1 - 10, foot.2 + 10, foot.3 + 10), s))
                });
                ok.then_some(basins)
            });
            if let Some(basins) = placed {
                out.extend(basins);
                break;
            }
        }
    }
    out
}

/// How big a pipe is: Felicity's (room 39), a hexagon 53 across with tips `PIPE_TIP` deep, here
/// lying on the floor against a wall (flat top and bottom, its tips to the wall and the room).
pub const PIPE: i32 = 53;
pub const PIPE_TIP: i32 = 13;
/// The share of the industrial rooms and corridors with pipes round their walls.
pub const PIPED: f64 = 0.6;
/// How far a pipe's end keeps from a doorway's opening along its wall (the frame's 13 and more).
pub const PIPE_GAP: i32 = 40;
/// How far from a corner a pipe on the next wall stops, when a doorway is within a pipe's depth
/// of that corner (the doorway's mouth: `felicity::MOUTH`).
pub const PIPE_MOUTH: i32 = 120;

/// A pipe along the base of a room's walls: the points it runs through along the walls' faces
/// (plan x, y), the room's inside on its left, round the room's corners.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipe {
    pub room: String,
    pub path: Vec<(i32, i32)>,
    pub floor: i32,
}

/// The pipes of a Felicity level: round the walls of `PIPED` of its one-storey industrial rooms
/// and corridors, broken at every doorway (beyond its frame: a hazard surround's splay, if any) and
/// at every portal.
pub fn pipes(seed: u64, plan: &Plan, doorways: &[Doorway], surrounds: &[Surround], portals: &[Portal]) -> Vec<Pipe> {
    let mut rng = PyRandom::named(&format!("{seed}/felicity/dress/pipes"));
    let mut out = Vec::new();
    for r in &plan.rooms {
        let Some(&(lo, hi)) = r.boxes.first() else { continue };
        let may = matches!(r.kind, "room" | "corridor") && r.scheme == "industrial" && hi.2 - lo.2 == ROOM_H;
        if !may || rng.random() >= PIPED {
            continue;
        }
        // the walls anticlockwise from the low corner, the inside on the left
        let corners = [(lo.0, lo.1), (hi.0, lo.1), (hi.0, hi.1), (lo.0, hi.1)];
        let lengths: Vec<i32> = (0..4).map(|i| (corners[(i + 1) % 4].0 - corners[i].0).abs() + (corners[(i + 1) % 4].1 - corners[i].1).abs()).collect();
        let starts: Vec<i32> = (0..4).map(|i| lengths[..i].iter().sum()).collect();
        let round: i32 = lengths.iter().sum();
        // each doorway of the room: the stretch of the way round it blocks
        let mut blocked: Vec<(i32, i32)> = Vec::new();
        for d in doorways.iter().filter(|d| (d.a == r.key || d.b == r.key) && d.lo.2 < hi.2 && d.hi.2 > lo.2) {
            let along = 1 - d.axis;
            let (s0, s1) = (at(d.lo, along), at(d.hi, along));
            // which wall: the one whose line the doorway's box meets
            let wall = match d.axis {
                1 if d.hi.1 == lo.1 => 0,
                0 if d.lo.0 == hi.0 => 1,
                1 if d.lo.1 == hi.1 => 2,
                0 if d.hi.0 == lo.0 => 3,
                _ => continue,
            };
            // along the way round: walls 0 and 1 run up their axis, 2 and 3 down it
            let (a, b) = match wall {
                0 => (s0 - lo.0, s1 - lo.0),
                1 => (s0 - lo.1, s1 - lo.1),
                2 => (hi.0 - s1, hi.0 - s0),
                _ => (hi.1 - s1, hi.1 - s0),
            };
            let frame = frame_width(surrounds, d, &r.key);
            let (mut a, mut b) = (starts[wall] + a - PIPE_GAP - frame, starts[wall] + b + PIPE_GAP + frame);
            // a doorway near a corner: the pipe on the other wall stops short of its mouth
            if a - starts[wall] < PIPE + PIPE_GAP {
                a = starts[wall] - PIPE_MOUTH;
            }
            if starts[wall] + lengths[wall] - b < PIPE + PIPE_GAP {
                b = starts[wall] + lengths[wall] + PIPE_MOUTH;
            }
            blocked.push((a, b));
        }
        // each portal: both side walls, along its depth
        for p in portals.iter().filter(|p| p.room == r.key) {
            let (s0, s1) = (p.at - PORTAL_D / 2 - PIPE_GAP, p.at + PORTAL_D / 2 + PIPE_GAP);
            let sides =
                if p.axis == 0 { [(0, s0 - lo.0, s1 - lo.0), (2, hi.0 - s1, hi.0 - s0)] } else { [(1, s0 - lo.1, s1 - lo.1), (3, hi.1 - s1, hi.1 - s0)] };
            for (wall, a, b) in sides {
                blocked.push((starts[wall] + a, starts[wall] + b));
            }
        }
        if blocked.is_empty() {
            continue;
        }
        // the free stretches of the way round, between the blocked ones (it wraps)
        let mut marks = vec![false; round as usize];
        for &(a, b) in &blocked {
            for s in a..b {
                marks[s.rem_euclid(round) as usize] = true;
            }
        }
        let Some(first) = (0..round).find(|&s| marks[s as usize]) else { continue };
        let point = |s: i32| -> (i32, i32) {
            let s = s.rem_euclid(round);
            let w = (0..4).rev().find(|&w| starts[w] <= s).unwrap_or(0);
            let (c, t) = (corners[w], s - starts[w]);
            match w {
                0 => (c.0 + t, c.1),
                1 => (c.0, c.1 + t),
                2 => (c.0 - t, c.1),
                _ => (c.0, c.1 - t),
            }
        };
        let mut s = first;
        while s < first + round {
            if marks[s.rem_euclid(round) as usize] {
                s += 1;
                continue;
            }
            let start = s;
            while s < first + round && !marks[s.rem_euclid(round) as usize] {
                s += 1;
            }
            if s - start < 2 * PIPE {
                continue;
            }
            let mut path = vec![point(start)];
            // the corners passed on the way (the way round counted twice, as the run may wrap)
            for k in 0..8 {
                let c = starts[k % 4] + (k / 4) as i32 * round;
                if start < c && c < s {
                    path.push(point(c));
                }
            }
            path.push(point(s));
            out.push(Pipe { room: r.key.clone(), path, floor: lo.2 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brace(axis: usize, at: i32, t: (i32, i32)) -> Brace {
        Brace { room: "r".into(), axis, at, t, floor: 0, top: 259 }
    }

    #[test]
    fn a_cross_is_four_arms_from_the_walls_and_a_crossing() {
        // Felicity's room 4, as the plan has it: 845 x 621, the cross in the middle
        let (arms, crossings) = arms(&[brace(1, 422, (0, 621)), brace(0, 310, (0, 845))]);
        assert_eq!(arms.len(), 4);
        assert_eq!(crossings, vec![Crossing { room: "r".into(), x: 422, y: 310, top: 259 }]);
        for a in &arms {
            // each arm from a wall to the crossing's side
            assert!(a.walls.0 != a.walls.1, "{a:?}");
            assert!(a.t.1 - a.t.0 > 250);
            // two light panels, as Felicity's arms have, the first about 85-128 from the wall
            assert_eq!(a.panels().len(), 2, "{a:?}");
            let first = a.panels()[0];
            let wall = if a.from_high() { a.t.1 } else { a.t.0 } as f64;
            assert!((first - wall).abs() < 140.0, "{a:?}: {first}");
        }
        assert_eq!(lamps(&arms).len(), 8);
    }

    #[test]
    fn braced_rooms_keep_their_pilasters_clear_of_every_doorway() {
        use crate::level::{generate, Options};
        let (mut braced, mut could, mut beams, mut corridors, mut ribs) = (0, 0, 0, 0, 0);
        for seed in 1..=40 {
            let lv = generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap();
            for r in &lv.plan.rooms {
                let (lo, hi) = r.boxes[0];
                let blue = matches!(r.scheme, "office" | "marble") && hi.2 - lo.2 == ROOM_H;
                let may = r.kind == "room" && blue && hi.0 - lo.0 >= LEAST && hi.1 - lo.1 >= LEAST;
                let ribbed = r.kind == "corridor" && blue;
                could += may as usize;
                let mine: Vec<&Brace> = lv.braces.iter().filter(|b| b.room == r.key).collect();
                assert!(may || ribbed || mine.is_empty(), "seed {seed}: {} braced", r.key);
                if ribbed {
                    corridors += 1;
                    ribs += mine.len();
                    // across the corridor only
                    let long = if hi.0 - lo.0 >= hi.1 - lo.1 { 0 } else { 1 };
                    assert!(mine.iter().all(|b| b.axis == 1 - long), "seed {seed}: {} ribbed along", r.key);
                } else {
                    braced += !mine.is_empty() as usize;
                    beams += mine.len();
                }
                for b in mine {
                    let across = 1 - b.axis;
                    for d in lv.doorways.iter().filter(|d| (d.a == r.key || d.b == r.key) && d.axis == b.axis) {
                        let (s0, s1) = (at(d.lo, across), at(d.hi, across));
                        assert!(
                            b.at + BEAM_W / 2 <= s0 - MARGIN || b.at - BEAM_W / 2 >= s1 + MARGIN,
                            "seed {seed}: {} at {} meets a doorway {s0}..{s1}",
                            r.key,
                            b.at
                        );
                    }
                }
            }
            let mut want = lamps(&arms(&lv.braces).0);
            want.extend(lv.portals.iter().map(|p| p.lamp()));
            assert_eq!(want, lv.lamps);
        }
        println!("seeds 1-40: {braced} of {could} rooms that could be are braced, {beams} beams; {ribs} ribs in {corridors} corridors");
        assert!(braced * 2 > could);
    }

    #[test]
    fn pipes_run_round_industrial_rooms_clear_of_every_doorways_mouth() {
        use crate::level::{generate, Options};
        let (mut rooms, mut runs) = (std::collections::BTreeSet::new(), 0);
        for seed in 1..=40 {
            let lv = generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap();
            for p in &lv.pipes {
                let r = lv.plan.get(&p.room).unwrap();
                assert_eq!(r.scheme, "industrial");
                let (lo, hi) = r.boxes[0];
                rooms.insert((seed, p.room.clone()));
                runs += 1;
                // every point on the room's walls, every leg along one
                assert!(p
                    .path
                    .iter()
                    .all(|&(x, y)| (x == lo.0 || x == hi.0 || y == lo.1 || y == hi.1) && (lo.0..=hi.0).contains(&x) && (lo.1..=hi.1).contains(&y)));
                assert!(p.path.windows(2).all(|w| w[0].0 == w[1].0 || w[0].1 == w[1].1));
                // nothing of it in a doorway's mouth: 120 in front of the opening, frame and gap
                for d in lv.doorways.iter().filter(|d| d.a == p.room || d.b == p.room) {
                    let o = 1 - d.axis;
                    let frame = frame_width(&lv.surrounds, d, &p.room);
                    let (s0, s1) = (at(d.lo, o) - frame - PIPE_GAP + 1, at(d.hi, o) + frame + PIPE_GAP - 1);
                    let (m0, m1) = (at(d.lo, d.axis) - 120, at(d.hi, d.axis) + 120);
                    for w in p.path.windows(2) {
                        // the leg's footprint: PIPE out from its wall
                        let (x0, x1) = (w[0].0.min(w[1].0), w[0].0.max(w[1].0));
                        let (y0, y1) = (w[0].1.min(w[1].1), w[0].1.max(w[1].1));
                        let (fx0, fx1, fy0, fy1) = if y0 == y1 {
                            (x0, x1, if y0 == lo.1 { y0 } else { y0 - PIPE }, if y0 == lo.1 { y0 + PIPE } else { y0 })
                        } else {
                            (if x0 == lo.0 { x0 } else { x0 - PIPE }, if x0 == lo.0 { x0 + PIPE } else { x0 }, y0, y1)
                        };
                        let (a0, a1, b0, b1) = if d.axis == 0 { (fx0, fx1, fy0, fy1) } else { (fy0, fy1, fx0, fx1) };
                        assert!(a1 <= m0 || a0 >= m1 || b1 <= s0 || b0 >= s1, "seed {seed}: a pipe in {} meets the mouth of {} {}", p.room, d.a, d.b);
                    }
                }
            }
        }
        println!("seeds 1-40: {runs} pipe runs in {} industrial rooms and corridors", rooms.len());
        assert!(runs > 20);
    }

    #[test]
    fn hazard_surrounds_stand_on_industrial_faces_clear_of_corners_and_other_frames() {
        use crate::level::{generate, Options};
        let (mut faces, mut could) = (0, 0);
        for seed in 1..=40 {
            let lv = generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap();
            let arched = |d: &Doorway| lv.arches.iter().any(|a| a.kind == "doorway" && a.a == d.a && a.b == d.b);
            for d in lv.doorways.iter().filter(|d| !arched(d)) {
                for room in [&d.a, &d.b] {
                    let r = lv.plan.get(room).unwrap();
                    let (lo, hi) = r.boxes[0];
                    could += (r.scheme == "industrial" && matches!(r.kind, "room" | "corridor") && hi.2 - lo.2 == ROOM_H) as usize;
                }
            }
            for s in &lv.surrounds {
                faces += 1;
                let d = lv.doorways.iter().find(|d| d.a == s.a && d.b == s.b).unwrap();
                assert!(!arched(d), "seed {seed}: a surround on an arched doorway");
                let r = lv.plan.get(&s.room).unwrap();
                assert_eq!(r.scheme, "industrial");
                let (lo, hi) = r.boxes[0];
                assert_eq!(hi.2 - lo.2, ROOM_H);
                // on the room's wall, its splay inside the wall's span and clear of every other frame
                let o = 1 - d.axis;
                let (s0, s1) = (at(d.lo, o) - SPLAY.0, at(d.hi, o) + SPLAY.0);
                let (w0, w1) = if o == 0 { (lo.0, hi.0) } else { (lo.1, hi.1) };
                assert!(s0 >= w0 + SURROUND_GAP && s1 <= w1 - SURROUND_GAP, "seed {seed}: {s:?} past its wall's end");
                for e in lv.doorways.iter().filter(|e| *e != d && e.axis == d.axis && at(e.lo, d.axis) == at(d.lo, d.axis) && (e.a == s.room || e.b == s.room))
                {
                    let w = frame_width(&lv.surrounds, e, &s.room);
                    assert!(at(e.hi, o) + w <= s0 || at(e.lo, o) - w >= s1, "seed {seed}: {s:?} meets {} {}'s frame", e.a, e.b);
                }
            }
        }
        println!("seeds 1-40: {faces} hazard surrounds of {could} industrial doorway faces");
        assert!(faces * 2 > could, "{faces} of {could}");
    }

    #[test]
    fn portals_stand_across_industrial_corridors_clear_of_their_doorways_and_pipes() {
        use crate::level::{generate, Options};
        let (mut portals, mut corridors) = (0, std::collections::BTreeSet::new());
        for seed in 1..=40 {
            let lv = generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap();
            for p in &lv.portals {
                portals += 1;
                corridors.insert((seed, p.room.clone()));
                let r = lv.plan.get(&p.room).unwrap();
                assert!(r.kind == "corridor" && r.scheme == "industrial", "seed {seed}: a portal in {}", p.room);
                let (lo, hi) = r.boxes[0];
                let (lo, hi) = ([lo.0, lo.1, lo.2], [hi.0, hi.1, hi.2]);
                let (k, o) = (p.axis, 1 - p.axis);
                // across the corridor's short span, inside its ends
                assert!(hi[k] - lo[k] >= hi[o] - lo[o]);
                assert_eq!(p.across, (lo[o], hi[o]));
                let (t0, t1) = (p.at - PORTAL_D / 2, p.at + PORTAL_D / 2);
                assert!(t0 >= lo[k] + PORTAL_END && t1 <= hi[k] - PORTAL_END, "seed {seed}: {p:?}");
                // its opening wide enough to walk through under the lintel
                assert!(hi[o] - lo[o] - 2 * (POST + LEAN) >= 100, "seed {seed}: {p:?}");
                // clear of every doorway's frame in its walls (every doorway of the corridor)
                for d in lv.doorways.iter().filter(|d| (d.a == p.room || d.b == p.room) && d.axis == o) {
                    let w = frame_width(&lv.surrounds, d, &p.room);
                    let (s0, s1) = (at(d.lo, k) - w, at(d.hi, k) + w);
                    assert!(t1 <= s0 - PORTAL_MARGIN || t0 >= s1 + PORTAL_MARGIN, "seed {seed}: {p:?} meets {} {}", d.a, d.b);
                }
                // no pipe under it
                for pipe in lv.pipes.iter().filter(|q| q.room == p.room) {
                    for w in pipe.path.windows(2) {
                        let (a0, a1) = if k == 0 { (w[0].0.min(w[1].0), w[0].0.max(w[1].0)) } else { (w[0].1.min(w[1].1), w[0].1.max(w[1].1)) };
                        let along = if k == 0 { w[0].1 == w[1].1 } else { w[0].0 == w[1].0 };
                        assert!(!along || a1 <= t0 || a0 >= t1, "seed {seed}: a pipe through {p:?}");
                    }
                }
            }
        }
        // a walker (radius 30, 185 tall) on a door-to-door line, 80 off the wall's face, clears the posts
        let reach = POST as f64 + LEAN as f64 * (185 - JAMB) as f64 / (LINTEL - JAMB) as f64;
        assert!(reach + 30.0 <= 80.0, "{reach}");
        println!("seeds 1-40: {portals} portals in {} industrial corridors", corridors.len());
        assert!(portals > 20);
    }

    #[test]
    fn washrooms_have_stalls_and_basins_clear_of_their_doorways_and_walks() {
        use crate::felicity::{crosses, INSIDE, MOUTH};
        use crate::level::{generate, Options};
        let (mut rooms, mut fitted, mut stalls, mut basins) = (0, 0, 0, 0);
        for seed in 1..=40 {
            let o = Options { style: "felicity", ..Options::default() };
            let lv = generate(seed, &o).unwrap();
            // the option off: no fittings and nothing else changed
            let bare = generate(seed, &Options { washroom: false, ..o.clone() }).unwrap();
            assert!(bare.fittings.is_empty());
            assert_eq!((&bare.doorways, &bare.braces, &bare.pipes, &bare.portals, &bare.lamps), (&lv.doorways, &lv.braces, &lv.pipes, &lv.portals, &lv.lamps));
            assert_eq!(
                bare.plan.rooms.iter().map(|r| (&r.key, r.boxes.clone())).collect::<Vec<_>>(),
                lv.plan.rooms.iter().map(|r| (&r.key, r.boxes.clone())).collect::<Vec<_>>()
            );
            for r in lv.plan.rooms.iter().filter(|r| r.scheme == "washroom") {
                rooms += 1;
                let mine: Vec<&Fitting> = lv.fittings.iter().filter(|f| f.room == r.key).collect();
                fitted += !mine.is_empty() as usize;
                stalls += mine.iter().filter(|f| f.kind == "toilet").count();
                basins += mine.iter().filter(|f| f.kind == "basin").count();
                let (lo, hi) = r.boxes[0];
                let doors: Vec<&Doorway> = lv.doorways.iter().filter(|d| (d.a == r.key || d.b == r.key) && d.lo.2 == lo.2).collect();
                for f in &mine {
                    let (x0, y0, x1, y1) = f.footprint();
                    assert!(lo.0 <= x0 && x1 <= hi.0 && lo.1 <= y0 && y1 <= hi.1, "seed {seed}: {f:?} outside {}", r.key);
                    for d in &doors {
                        // not in a doorway's mouth
                        let across = 1 - d.axis;
                        let (s0, s1) = (at(d.lo, across) - 13, at(d.hi, across) + 13);
                        let (t0, t1) = if d.a == r.key { (at(d.lo, d.axis) - MOUTH, at(d.lo, d.axis)) } else { (at(d.hi, d.axis), at(d.hi, d.axis) + MOUTH) };
                        let m = if d.axis == 0 { (t0, s0, t1, s1) } else { (s0, t0, s1, t1) };
                        assert!(!(x0 < m.2 && m.0 < x1 && y0 < m.3 && m.1 < y1), "seed {seed}: {f:?} in the mouth of {} {}", d.a, d.b);
                    }
                    // off every walk between two of its doorways
                    let end = |d: &Doorway| {
                        let off = if d.a == r.key { -INSIDE } else { INSIDE };
                        let c = (at(d.lo, 1 - d.axis) + at(d.hi, 1 - d.axis)) as f64 / 2.0;
                        let m = (d.plane() + off) as f64;
                        if d.axis == 0 {
                            (m, c)
                        } else {
                            (c, m)
                        }
                    };
                    for i in 0..doors.len() {
                        for j in i + 1..doors.len() {
                            assert!(!crosses(end(doors[i]), end(doors[j]), (x0, y0, x1, y1), FITTING_PAD), "seed {seed}: {f:?} on a walk");
                        }
                    }
                }
            }
        }
        println!("seeds 1-40: {fitted} of {rooms} washrooms fitted, {stalls} stalls, {basins} basins");
        assert!(fitted * 2 > rooms, "{fitted} of {rooms}");
    }

    #[test]
    fn a_pilaster_keeps_clear_of_doorways_and_corners() {
        assert!(clear(400, 0, 800, &[]));
        assert!(!clear(40, 0, 800, &[]));
        // an opening 300..406: a pilaster at 400 meets it, one 60 clear of its edge doesn't
        assert!(!clear(400, 0, 800, &[(300, 406)]));
        assert!(clear(406 + MARGIN + 20, 0, 800, &[(300, 406)]));
    }
}
