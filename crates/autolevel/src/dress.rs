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
    fn a_pilaster_keeps_clear_of_doorways_and_corners() {
        assert!(clear(400, 0, 800, &[]));
        assert!(!clear(40, 0, 800, &[]));
        // an opening 300..406: a pilaster at 400 meets it, one 60 clear of its edge doesn't
        assert!(!clear(400, 0, 800, &[(300, 406)]));
        assert!(clear(406 + MARGIN + 20, 0, 800, &[(300, 406)]));
    }
}
