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
/// and corridors, broken at every doorway.
pub fn pipes(seed: u64, plan: &Plan, doorways: &[Doorway]) -> Vec<Pipe> {
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
            let (mut a, mut b) = (starts[wall] + a - PIPE_GAP - 13, starts[wall] + b + PIPE_GAP + 13);
            // a doorway near a corner: the pipe on the other wall stops short of its mouth
            if a - starts[wall] < PIPE + PIPE_GAP {
                a = starts[wall] - PIPE_MOUTH;
            }
            if starts[wall] + lengths[wall] - b < PIPE + PIPE_GAP {
                b = starts[wall] + lengths[wall] + PIPE_MOUTH;
            }
            blocked.push((a, b));
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
            assert_eq!(lamps(&arms(&lv.braces).0), lv.lamps);
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
                    let (s0, s1) = (at(d.lo, o) - 13 - PIPE_GAP + 1, at(d.hi, o) + 13 + PIPE_GAP - 1);
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
    fn a_pilaster_keeps_clear_of_doorways_and_corners() {
        assert!(clear(400, 0, 800, &[]));
        assert!(!clear(40, 0, 800, &[]));
        // an opening 300..406: a pilaster at 400 meets it, one 60 clear of its edge doesn't
        assert!(!clear(400, 0, 800, &[(300, 406)]));
        assert!(clear(406 + MARGIN + 20, 0, 800, &[(300, 406)]));
    }
}
