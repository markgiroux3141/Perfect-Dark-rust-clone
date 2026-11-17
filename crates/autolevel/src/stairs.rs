//! Stairs in a stair room: a storey's flights (12 risers), their landings and railings, and
//! where the room's doorways may open at the foot and at the top. Not a port; Felicity's
//! stairs are the model (`assets/stages/mp11`: a straight flight 132 wide in a room 331 deep, a
//! turning stair 330 square).
//!
//! ```text
//! let s = stairs::fit(Shape::U, air, floor, STOREY, frame)?;   // Err: it doesn't fit this room
//! s.solids                // steps ("step") and landings ("landing"), filled back into the room
//! s.foot, s.top           // stretches of wall a doorway may open in, on the floor and a storey up
//! s.path                  // the way up, foot to top landing (units)
//! s.rails, s.sloped       // railings along every edge that drops into the stairwell
//! ```
//!
//! Each shape is drawn once in a local frame (a along the first flight, b across it, the room's
//! air [0, A] x [0, B]) and turned into the room by one of eight frames (`Frame::ALL`). The shapes:
//!
//! - **Straight**: one flight against the b = 0 wall, `WIDTH` wide, after `FOOT` of floor; a
//!   landing across the room's whole width at the top (a gallery). Floor beside the flight, under
//!   nothing, is railed off from the flight and the gallery.
//! - **L**: a half flight against the b = 0 wall, a landing along it to the far wall, a half flight
//!   up the far wall, and a gallery across the top. The floor in the crook is railed off.
//! - **U**: a switchback filling the room's width (each flight half of it): a half flight up, a
//!   landing across the far end, a half flight back, and the top landing over the foot's side.
//!   No well, so no rails.
//!
//! Heights are a storey's 12 risers, each rounded to a whole unit (24, 47, ... 283); a step is
//! `GOING` deep.

use crate::level::SlopedRail;
use crate::plan::{Point3, Rail, Solid};

/// A riser count per storey, and each step's depth.
pub const RISERS: i32 = 12;
pub const GOING: i32 = 40;
/// A straight or L flight's width (Felicity's straight flight: 132).
pub const WIDTH: i32 = 132;
/// Floor before the first step, and the least a landing at the top is deep. A U's foot is
/// longer: its doorway on the floor is beside the first flight (or in the end wall), and the
/// top landing is over the other half of the same end.
pub const FOOT: i32 = 150;
pub const FOOT_U: i32 = 250;
pub const LANDING: i32 = 130;

/// Which stairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Straight,
    L,
    U,
}

impl Shape {
    pub const ALL: [Shape; 3] = [Shape::Straight, Shape::L, Shape::U];

    pub fn name(self) -> &'static str {
        match self {
            Shape::Straight => "straight",
            Shape::L => "L",
            Shape::U => "U",
        }
    }
}

/// How the local frame lies in the room: the first flight's way (`a`) along world x or y, and
/// either axis turned round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub along_x: bool,
    pub flip_a: bool,
    pub flip_b: bool,
}

const fn frame(along_x: bool, flip_a: bool, flip_b: bool) -> Frame {
    Frame { along_x, flip_a, flip_b }
}

impl Frame {
    /// The eight ways a shape may lie.
    pub const ALL: [Frame; 8] = [
        frame(true, false, false),
        frame(true, false, true),
        frame(true, true, false),
        frame(true, true, true),
        frame(false, false, false),
        frame(false, false, true),
        frame(false, true, false),
        frame(false, true, true),
    ];
}

/// A stretch of a room's wall a doorway may open in: the wall (E, N, W, S: the way it faces out),
/// the stretch along it (world x for an N or S wall, world y for an E or W one) and the floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub side: char,
    pub lo: i32,
    pub hi: i32,
    pub z: i32,
}

impl Span {
    /// Whether a doorway from `lo` to `hi` along the wall fits in it.
    pub fn holds(&self, lo: i32, hi: i32) -> bool {
        self.lo <= lo && hi <= self.hi
    }
}

/// A stair room's stairs, in world units.
#[derive(Clone, Debug, PartialEq)]
pub struct Stairs {
    pub shape: Shape,
    pub frame: Frame,
    pub solids: Vec<Solid>,
    pub foot: Vec<Span>,
    pub top: Vec<Span>,
    /// From just before the first step to the middle of the top landing.
    pub path: Vec<Point3>,
    /// Flat railings (landing edges) and sloped ones (flights' open sides).
    pub rails: Vec<Rail>,
    pub sloped: Vec<(Point3, Point3, (i32, i32, i32))>,
}

impl Stairs {
    /// The sloped railings as the level holds them, in room `key`.
    pub fn sloped_rails(&self, key: &str) -> Vec<SlopedRail> {
        self.sloped.iter().map(|(a, b, f)| (key.to_string(), *a, *b, *f)).collect()
    }
}

/// The top of riser `j` (0-based) above the floor.
pub fn riser(j: i32, rise: i32) -> i32 {
    (((j + 1) * rise) as f64 / RISERS as f64).round() as i32
}

/// Stairs of `shape` in a room whose air is `lo`..`hi` (x, y), floor `floor`, rising `rise`,
/// laid by `frame`. Err if they don't fit.
pub fn fit(shape: Shape, lo: (i32, i32), hi: (i32, i32), floor: i32, rise: i32, frame: Frame) -> Result<Stairs, String> {
    let (w, d) = (hi.0 - lo.0, hi.1 - lo.1);
    let (a_len, b_len) = if frame.along_x { (w, d) } else { (d, w) };
    let half = RISERS / 2;
    let run = |n: i32| n * GOING;
    // the shape in local (a, b, z) boxes, spans, path and rails
    let mut steps: Vec<((i32, i32), (i32, i32), i32, &'static str)> = Vec::new(); // (a0, a1), (b0, b1), top, tag
    let mut foot: Vec<(u8, i32, i32)> = Vec::new(); // (side: 0 a=0, 1 a=A, 2 b=0, 3 b=B), range along it
    let mut top: Vec<(u8, i32, i32)> = Vec::new();
    let mut path: Vec<(i32, i32, i32)> = Vec::new();
    let mut flat: Vec<((i32, i32), (i32, i32), i32, (i32, i32))> = Vec::new(); // ends (a, b), z, facing (a, b)
    let mut slope: Vec<((i32, i32, i32), (i32, i32, i32), (i32, i32))> = Vec::new();
    let flight = |steps: &mut Vec<_>, from: i32, dir: i32, b: (i32, i32), j0: i32, n: i32, along_b: bool| {
        // n steps from `from` in `dir` (+1/-1) along a (or along b), risers j0.., across `b`
        for i in 0..n {
            let (p, q) = (from + dir * run(i), from + dir * run(i + 1));
            let r = (p.min(q), p.max(q));
            let z = riser(j0 + i, rise);
            if along_b {
                steps.push((b, r, z, "step"));
            } else {
                steps.push((r, b, z, "step"));
            }
        }
    };
    match shape {
        Shape::Straight => {
            let wd = WIDTH.min(b_len);
            let end = FOOT + run(RISERS);
            if a_len < end + LANDING || b_len < wd {
                return Err(format!("straight stairs need {} x {wd}; the room is {a_len} x {b_len}", end + LANDING));
            }
            flight(&mut steps, FOOT, 1, (0, wd), 0, RISERS, false);
            steps.push(((end, a_len), (0, b_len), rise, "landing"));
            foot.extend([(0, 0, b_len), (2, 0, FOOT)]);
            if b_len > wd {
                foot.push((3, 0, end));
                flat.push(((end, wd), (end, b_len), rise, (-1, 0)));
                slope.push(((FOOT, wd, 0), (end, wd, rise), (0, 1)));
            } else {
                foot.push((3, 0, FOOT));
            }
            top.extend([(1, 0, b_len), (2, end, a_len), (3, end, a_len)]);
            let m = wd / 2;
            path.extend([(FOOT - 60, m, 0), (end + 60, m, rise), ((end + a_len) / 2, b_len / 2, rise)]);
        }
        Shape::L => {
            let wd = WIDTH;
            let (f1, f2) = (FOOT + run(half), wd + run(half));
            if a_len < f1 + wd || b_len < f2 + LANDING {
                return Err(format!("L stairs need {} x {}; the room is {a_len} x {b_len}", f1 + wd, f2 + LANDING));
            }
            let mid = riser(half - 1, rise);
            flight(&mut steps, FOOT, 1, (0, wd), 0, half, false);
            steps.push(((f1, a_len), (0, wd), mid, "landing"));
            flight(&mut steps, wd, 1, (a_len - wd, a_len), half, half, true);
            steps.push(((0, a_len), (f2, b_len), rise, "landing"));
            foot.extend([(0, 0, f2), (2, 0, FOOT)]);
            top.extend([(3, 0, a_len), (0, f2, b_len), (1, f2, b_len)]);
            // the crook's floor railed off: the first flight's side, the landing's, the second
            // flight's, the gallery's
            slope.push(((FOOT, wd, 0), (f1, wd, mid), (0, 1)));
            if a_len - wd > f1 {
                flat.push(((f1, wd), (a_len - wd, wd), mid, (0, 1)));
            }
            slope.push(((a_len - wd, wd, mid), (a_len - wd, f2, rise), (-1, 0)));
            flat.push(((0, f2), (a_len - wd, f2), rise, (0, -1)));
            let m = wd / 2;
            path.extend([(FOOT - 60, m, 0), (a_len - m, m, mid), (a_len - m, f2 + 60, rise), (a_len / 2, (f2 + b_len) / 2, rise)]);
        }
        Shape::U => {
            let wd = b_len / 2;
            let foot_u = FOOT_U;
            let f1 = foot_u + run(half);
            if !(120..=200).contains(&wd) || a_len < f1 + wd {
                return Err(format!("U stairs need a room 240 to 400 wide and {} long; it is {a_len} x {b_len}", f1 + 120));
            }
            let mid = riser(half - 1, rise);
            flight(&mut steps, foot_u, 1, (0, wd), 0, half, false);
            steps.push(((f1, a_len), (0, b_len), mid, "landing"));
            flight(&mut steps, f1, -1, (wd, b_len), half, half, false);
            steps.push(((0, foot_u), (wd, b_len), rise, "landing"));
            foot.extend([(0, 0, wd), (2, 0, foot_u)]);
            top.extend([(0, wd, b_len), (3, 0, foot_u)]);
            let (m1, m2) = (wd / 2, wd + (b_len - wd) / 2);
            path.extend([(foot_u - 60, m1, 0), ((f1 + a_len) / 2, m1, mid), ((f1 + a_len) / 2, m2, mid), (foot_u / 2, m2, rise)]);
        }
    }

    // ---- local to world
    let x = |a: i32, b: i32| -> (i32, i32) {
        let (a, b) = (if frame.flip_a { a_len - a } else { a }, if frame.flip_b { b_len - b } else { b });
        if frame.along_x {
            (lo.0 + a, lo.1 + b)
        } else {
            (lo.0 + b, lo.1 + a)
        }
    };
    let dir = |da: i32, db: i32| -> (i32, i32, i32) {
        let (da, db) = (if frame.flip_a { -da } else { da }, if frame.flip_b { -db } else { db });
        if frame.along_x {
            (da, db, 0)
        } else {
            (db, da, 0)
        }
    };
    let solids = steps
        .iter()
        .map(|&((a0, a1), (b0, b1), z, tag)| {
            let (p, q) = (x(a0, b0), x(a1, b1));
            ((p.0.min(q.0), p.1.min(q.1), floor), (p.0.max(q.0), p.1.max(q.1), floor + z), tag)
        })
        .collect();
    // a local side's world wall, and the stretch along it
    let span = |&(side, s0, s1): &(u8, i32, i32), z: i32| -> Span {
        let (p, q) = match side {
            0 => (x(0, s0), x(0, s1)),
            1 => (x(a_len, s0), x(a_len, s1)),
            2 => (x(s0, 0), x(s1, 0)),
            _ => (x(s0, b_len), x(s1, b_len)),
        };
        // the wall faces out of the room: away from the middle
        let (cx, cy) = (lo.0 + w / 2, lo.1 + d / 2);
        let (wall, along) = if p.0 == q.0 {
            (if p.0 > cx { 'E' } else { 'W' }, (p.1.min(q.1), p.1.max(q.1)))
        } else {
            (if p.1 > cy { 'N' } else { 'S' }, (p.0.min(q.0), p.0.max(q.0)))
        };
        Span { side: wall, lo: along.0, hi: along.1, z: floor + z }
    };
    let world = |(a, b, z): (i32, i32, i32)| {
        let (px, py) = x(a, b);
        (px, py, floor + z)
    };
    Ok(Stairs {
        shape,
        frame,
        solids,
        foot: foot.iter().map(|s| span(s, 0)).collect(),
        top: top.iter().map(|s| span(s, rise)).collect(),
        path: path.into_iter().map(world).collect(),
        rails: flat
            .iter()
            .map(|&((a0, b0), (a1, b1), z, (fa, fb))| {
                let (p, q) = (x(a0, b0), x(a1, b1));
                ((p.0, p.1, floor + z), (q.0, q.1, floor + z), dir(fa, fb))
            })
            .collect(),
        sloped: slope.iter().map(|&(p, q, (fa, fb))| (world(p), world(q), dir(fa, fb))).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The solids' tops along the path never rise more than a riser at a time.
    fn climbs(s: &Stairs, rise: i32) {
        let steps: Vec<i32> = s.solids.iter().filter(|b| b.2 == "step").map(|b| b.1 .2).collect();
        assert_eq!(steps.len(), RISERS as usize, "{:?}", s.shape);
        let mut tops = steps.clone();
        tops.sort();
        assert_eq!((tops[0], tops[11]), (24, rise));
        assert!(tops.windows(2).all(|w| (23..=24).contains(&(w[1] - w[0]))));
        assert_eq!(s.path.last().unwrap().2, rise);
    }

    #[test]
    fn every_shape_fits_a_room_its_size_and_says_where_its_doors_go() {
        let rise = 283;
        // a long room: straight and L; a square one: U
        for (shape, size) in [(Shape::Straight, (808, 384)), (Shape::L, (808, 596)), (Shape::U, (720, 330))] {
            for frame in Frame::ALL {
                let (w, d) = if frame.along_x { size } else { (size.1, size.0) };
                let s = fit(shape, (100, 200), (100 + w, 200 + d), 0, rise, frame).unwrap_or_else(|e| panic!("{shape:?} {frame:?}: {e}"));
                climbs(&s, rise);
                assert!(!s.foot.is_empty() && !s.top.is_empty());
                assert!(s.foot.iter().all(|p| p.z == 0) && s.top.iter().all(|p| p.z == rise));
                // every solid is inside the room
                assert!(s.solids.iter().all(|(lo, hi, _)| lo.0 >= 100 && lo.1 >= 200 && hi.0 <= 100 + w && hi.1 <= 200 + d));
                // a U has no well; the others rail theirs
                assert_eq!(s.rails.is_empty() && s.sloped.is_empty(), shape == Shape::U, "{shape:?}");
            }
        }
        assert!(fit(Shape::U, (0, 0), (808, 808), 0, rise, Frame::ALL[0]).is_err());
        assert!(fit(Shape::Straight, (0, 0), (500, 300), 0, rise, Frame::ALL[0]).is_err());
    }

    #[test]
    fn a_frame_turns_the_shape_and_its_spans_with_it() {
        let s = fit(Shape::Straight, (0, 0), (808, 384), 0, 283, Frame { along_x: true, flip_a: false, flip_b: false }).unwrap();
        // the flight against the south wall rising east: the gallery's wall is the east one
        assert!(s.top.iter().any(|p| p.side == 'E' && p.lo == 0 && p.hi == 384));
        assert!(s.foot.iter().any(|p| p.side == 'W'));
        let t = fit(Shape::Straight, (0, 0), (808, 384), 0, 283, Frame { along_x: true, flip_a: true, flip_b: false }).unwrap();
        assert!(t.top.iter().any(|p| p.side == 'W') && t.foot.iter().any(|p| p.side == 'E'));
    }
}
