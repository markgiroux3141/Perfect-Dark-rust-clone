//! Walkways up high in a tall room: a balcony along one wall a storey up, railed, reached by a
//! flight of stairs up the wall beside it; in a room three storeys tall, a second along the
//! opposite wall two storeys up, reached by a flight up from the first. Not a port; the
//! storeys and stairs are Felicity's (`stairs`).
//!
//! ```text
//! let b = balcony::fit((x0, y0), (x1, y1), floor, STOREY, 2, frame, &solids)?;   // Err: it doesn't fit
//! b.solids                // the flights' steps ("step") and the walkways ("walkway"), filled back in
//! b.rails, b.sloped       // railings along every edge that drops into the room
//! b.paths                 // the way up each flight and along each walkway (units)
//! b.upper                 // the walls behind the walkways, where a doorway may open onto one
//! b.ground                // what stands on the floor (to keep doorways' mouths clear of)
//! ```
//!
//! Drawn in a local frame (`stairs::Frame`, as the stairs are): a along the first walkway's wall
//! (b = 0), b out from it into the room, the room's air [0, A] x [0, B].
//!
//! - **The first walkway**: a slab `SLAB` thick, its top a storey up, `DEPTH` out from the b = 0
//!   wall, from the a = 0 wall along to `A` (or to where something standing in the room stops
//!   it: a pillar, the hall's platform).
//! - **Its flight**: against the a = 0 wall, `stairs::WIDTH` wide, twelve risers rising towards
//!   the b = 0 wall and arriving at the walkway's edge; solid down to the floor (as Felicity's
//!   stairs are), with `stairs::FOOT` of floor before its first step.
//! - **The second walkway** (a room three storeys tall): along the b = B wall two storeys up, from
//!   wall to wall, reached by a flight against the a = A wall from the first walkway's far end.
//!   That flight stands on nothing: each step is a block `SLAB` below the one before it, so its
//!   underside is stepped. Where the flight's top falls short of the second walkway, a landing
//!   along the a = A wall joins them.
//!
//! Every open edge is railed: the first flight's side, the walkways' edges (but where a flight
//! arrives or leaves), a walkway's end that is not a wall, the second flight's side and its
//! landing's.

use crate::plan::{Point3, Rail, Solid};
use crate::stairs::{riser, Frame, Span, FOOT, GOING, RISERS, WIDTH};

/// How far a walkway stands out from its wall.
pub const DEPTH: i32 = 160;
/// A walkway's thickness (Felicity's slab: storey 283 less room 259).
pub const SLAB: i32 = 24;
/// The least a walkway runs past the flight up to it.
const LEAST: i32 = 300;
/// What a walkway stops short of, and what a flight keeps off, round anything in the room.
const CLEAR: i32 = 40;

/// A tall room's walkways and the flights up to them, in world units.
#[derive(Clone, Debug, PartialEq)]
pub struct Balcony {
    pub frame: Frame,
    /// How many walkways (1 or 2).
    pub levels: i32,
    pub solids: Vec<Solid>,
    pub rails: Vec<Rail>,
    pub sloped: Vec<(Point3, Point3, (i32, i32, i32))>,
    /// The way up: from the first flight's foot along the first walkway to its far end, and
    /// (with a second walkway) from there up the second flight and along it.
    pub paths: Vec<Vec<Point3>>,
    /// The walls behind the walkways (a doorway in one opens onto it).
    pub upper: Vec<Span>,
    /// The first flight and the floor before its foot, as (x0, y0, x1, y1).
    pub ground: (i32, i32, i32, i32),
}

/// Whether two rectangles (x0, y0, x1, y1) overlap.
fn overlap(p: (i32, i32, i32, i32), q: (i32, i32, i32, i32)) -> bool {
    p.0 < q.2 && q.0 < p.2 && p.1 < q.3 && q.1 < p.3
}

/// Walkways (`levels`, 1 or 2) in a room whose air is `lo`..`hi` (x, y), floor `floor`, a storey
/// `rise`, laid by `frame`, the first flight `at` along the first walkway (0: against the wall at
/// its end), round the `standing` solids already in it (pillars, a platform). Err if they don't
/// fit.
pub fn fit((lo, hi): ((i32, i32), (i32, i32)), floor: i32, rise: i32, levels: i32, frame: Frame, at: i32, standing: &[Solid]) -> Result<Balcony, String> {
    let len = frame.sizes(hi.0 - lo.0, hi.1 - lo.1);
    let (a_len, b_len) = len;
    let run = RISERS * GOING;
    // local rectangles as the plan's
    let rect = |a0: i32, b0: i32, a1: i32, b1: i32| {
        let (p, q) = (frame.point(lo, len, a0, b0), frame.point(lo, len, a1, b1));
        (p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1))
    };
    let hits = |r: (i32, i32, i32, i32)| standing.iter().any(|(s, t, _)| overlap(r, (s.0 - CLEAR, s.1 - CLEAR, t.0 + CLEAR, t.1 + CLEAR)));
    if b_len < DEPTH + run + FOOT {
        return Err(format!("a walkway and its flight need {} out from the wall; the room is {b_len}", DEPTH + run + FOOT));
    }
    // the first walkway, as far along as nothing stands in its way
    let mut a1 = a_len;
    for (s, t, _) in standing {
        let (p, q) = ((s.0 - CLEAR, s.1 - CLEAR), (t.0 + CLEAR, t.1 + CLEAR));
        // the solid's stretch along a, where it is within the walkway's depth
        let local = [frame.local(lo, len, p.0, p.1), frame.local(lo, len, q.0, q.1)];
        let (sa0, sa1) = (local[0].0.min(local[1].0), local[0].0.max(local[1].0));
        let (sb0, sb1) = (local[0].1.min(local[1].1), local[0].1.max(local[1].1));
        if sb0 < DEPTH && 0 < sb1 && sa1 > 0 {
            a1 = a1.min(sa0.max(0));
        }
    }
    let two = levels >= 2 && a1 == a_len;
    if a1 < WIDTH + LEAST || (two && a_len < 2 * WIDTH + LEAST) {
        return Err(format!("a walkway {a1} long is too short"));
    }
    if two && b_len < 2 * DEPTH + run {
        return Err(format!("two walkways and the flight between need {} out from the wall; the room is {b_len}", 2 * DEPTH + run));
    }
    if at < 0 || at + WIDTH > a1 || (two && at + WIDTH > a_len - WIDTH - CLEAR) {
        return Err(format!("no flight {at} along a walkway {a1} long"));
    }
    // the first flight, and the floor before it
    let ground = rect(at, DEPTH, at + WIDTH, DEPTH + run + FOOT);
    if hits(ground) {
        return Err("something stands where the flight would".into());
    }

    let mut b = Balcony {
        frame,
        levels: if two { 2 } else { 1 },
        solids: Vec::new(),
        rails: Vec::new(),
        sloped: Vec::new(),
        paths: Vec::new(),
        upper: Vec::new(),
        ground,
    };
    let solid = |b: &mut Balcony, a0: i32, b0: i32, a1: i32, b1: i32, z0: i32, z1: i32, tag: &'static str| {
        let r = rect(a0, b0, a1, b1);
        b.solids.push(((r.0, r.1, floor + z0), (r.2, r.3, floor + z1), tag));
    };
    let world = |(a, bb, z): (i32, i32, i32)| {
        let p = frame.point(lo, len, a, bb);
        (p.0, p.1, floor + z)
    };
    let flat = |b: &mut Balcony, p: (i32, i32), q: (i32, i32), z: i32, facing: (i32, i32)| {
        if p.0 < q.0 || p.1 < q.1 {
            b.rails.push((world((p.0, p.1, z)), world((q.0, q.1, z)), frame.way(facing.0, facing.1)));
        }
    };
    // ---- the first flight: risers towards the wall, its top step at the walkway's edge; railed
    // on each side that is not against a wall
    let (f0, f1) = (at, at + WIDTH);
    for j in 0..RISERS {
        let b0 = DEPTH + run - (j + 1) * GOING;
        solid(&mut b, f0, b0, f1, b0 + GOING, 0, riser(j, rise), "step");
    }
    b.sloped.push((world((f1, DEPTH + run, 0)), world((f1, DEPTH, rise)), frame.way(1, 0)));
    if f0 > 0 {
        b.sloped.push((world((f0, DEPTH + run, 0)), world((f0, DEPTH, rise)), frame.way(-1, 0)));
    }
    // ---- the first walkway, railed along its edge but where the flights meet it
    solid(&mut b, 0, 0, a1, DEPTH, rise - SLAB, rise, "walkway");
    flat(&mut b, (0, DEPTH), (f0, DEPTH), rise, (0, 1));
    flat(&mut b, (f1, DEPTH), (if two { a_len - WIDTH } else { a1 }, DEPTH), rise, (0, 1));
    if a1 < a_len {
        flat(&mut b, (a1, 0), (a1, DEPTH), rise, (1, 0));
    }
    // the way up: the flight, then to the walkway's near end (if the flight is not at it) and its far one
    let m = WIDTH / 2;
    let mut path = vec![(f0 + m, DEPTH + run + 60, 0), (f0 + m, DEPTH / 2, rise)];
    if f0 >= 100 {
        path.push((60, DEPTH / 2, rise));
    }
    path.push((a1 - 60, DEPTH / 2, rise));
    b.paths.push(path.into_iter().map(world).collect());
    b.upper.push(frame.span(lo, len, (2, 0, a1), floor + rise));
    if two {
        // ---- the second flight, up the far wall from the first walkway's end: each step a
        // block on nothing, its underside a slab below its tread
        let top = 2 * rise;
        for j in 0..RISERS {
            let b0 = DEPTH + j * GOING;
            let z = rise + riser(j, rise);
            solid(&mut b, a_len - WIDTH, b0, a_len, b0 + GOING, z - 2 * SLAB, z, "step");
        }
        let (landing, far) = (DEPTH + run, b_len - DEPTH);
        b.sloped.push((world((a_len - WIDTH, DEPTH, rise)), world((a_len - WIDTH, landing, top)), frame.way(-1, 0)));
        // its landing along the far wall to the second walkway, and the walkway
        if landing < far {
            solid(&mut b, a_len - WIDTH, landing, a_len, far, top - SLAB, top, "walkway");
            flat(&mut b, (a_len - WIDTH, landing), (a_len - WIDTH, far), top, (-1, 0));
        }
        solid(&mut b, 0, far, a_len, b_len, top - SLAB, top, "walkway");
        flat(&mut b, (0, far), (a_len - WIDTH, far), top, (0, -1));
        let m2 = a_len - m;
        b.paths.push(
            vec![(a1 - 60, DEPTH / 2, rise), (m2, DEPTH / 2, rise), (m2, landing + 60, top), (m2, (far + b_len) / 2, top), (60, (far + b_len) / 2, top)]
                .into_iter()
                .map(world)
                .collect(),
        );
        b.upper.push(frame.span(lo, len, (3, 0, a_len), floor + top));
        if hits(rect(a_len - WIDTH, DEPTH, a_len, b_len)) || hits(rect(0, far, a_len, b_len)) {
            return Err("something stands where the second walkway would".into());
        }
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stairs::Frame;

    const RISE: i32 = 283;

    /// Every step of a flight is a riser above the one before it.
    fn steps(b: &Balcony) -> Vec<i32> {
        let mut tops: Vec<i32> = b.solids.iter().filter(|s| s.2 == "step").map(|s| s.1 .2).collect();
        tops.sort();
        tops
    }

    #[test]
    fn a_walkway_fits_a_tall_room_every_way_round() {
        for frame in Frame::ALL {
            let b = fit(((0, 0), (1000, 1100)), 0, RISE, 1, frame, 0, &[]).unwrap_or_else(|e| panic!("{frame:?}: {e}"));
            assert_eq!(b.levels, 1);
            let tops = steps(&b);
            assert_eq!((tops.len(), tops[0], tops[11]), (12, 24, RISE));
            // everything inside the room; one walkway, its top a storey up
            assert!(b.solids.iter().all(|(lo, hi, _)| lo.0 >= 0 && lo.1 >= 0 && hi.0 <= 1000 && hi.1 <= 1100));
            let decks: Vec<&Solid> = b.solids.iter().filter(|s| s.2 == "walkway").collect();
            assert_eq!(decks.len(), 1);
            assert_eq!((decks[0].0 .2, decks[0].1 .2), (RISE - SLAB, RISE));
            // the path climbs to the walkway, and its wall takes a doorway
            assert_eq!(b.paths[0].last().unwrap().2, RISE);
            assert_eq!(b.upper.len(), 1);
            assert_eq!(b.upper[0].z, RISE);
            // a sloped rail up the flight, a flat one along the walkway
            assert_eq!((b.sloped.len(), b.rails.len()), (1, 1));
        }
    }

    #[test]
    fn a_room_three_storeys_tall_takes_two() {
        for frame in Frame::ALL {
            let b = fit(((0, 0), (1100, 1100)), 0, RISE, 2, frame, 0, &[]).unwrap_or_else(|e| panic!("{frame:?}: {e}"));
            assert_eq!(b.levels, 2);
            let tops = steps(&b);
            assert_eq!((tops.len(), tops[0], tops[23]), (24, 24, 2 * RISE));
            assert!(tops.windows(2).all(|w| (23..=24).contains(&(w[1] - w[0]))));
            // the second flight stands on nothing: its lowest block starts under the walkway
            let lowest = b.solids.iter().filter(|s| s.2 == "step" && s.0 .2 > 0).map(|s| s.0 .2).min().unwrap();
            assert_eq!(lowest, RISE - SLAB);
            assert_eq!(b.paths.len(), 2);
            assert_eq!(b.paths[1].last().unwrap().2, 2 * RISE);
        }
    }

    #[test]
    fn it_keeps_clear_of_what_stands_in_the_room() {
        let frame = Frame { along_x: true, flip_a: false, flip_b: false };
        // a pillar near the walkway's wall stops it short, and its end is railed
        let pillar: Solid = ((600, 60, 0), (700, 160, 780), "pillar");
        let b = fit(((0, 0), (1200, 1100)), 0, RISE, 1, frame, 0, &[pillar]).unwrap();
        let deck = b.solids.iter().find(|s| s.2 == "walkway").unwrap();
        assert_eq!(deck.1 .0, 600 - CLEAR);
        assert_eq!(b.rails.len(), 2);
        // one where the flight goes refuses
        let block: Solid = ((0, 400, 0), (100, 500, 70), "platform");
        assert!(fit(((0, 0), (1200, 1100)), 0, RISE, 1, frame, 0, &[block]).is_err());
        // a flight out in the room is railed both sides, and the walkway either side of it
        let b = fit(((0, 0), (1200, 1100)), 0, RISE, 1, frame, 400, &[]).unwrap();
        assert_eq!((b.sloped.len(), b.rails.len()), (2, 2));
        assert_eq!(b.ground, (400, DEPTH, 400 + WIDTH, DEPTH + RISERS * GOING + FOOT));
        assert!(fit(((0, 0), (1200, 1100)), 0, RISE, 1, frame, 1100, &[]).is_err());
        // too shallow a room refuses
        assert!(fit(((0, 0), (1200, 700)), 0, RISE, 1, frame, 0, &[]).is_err());
    }
}
