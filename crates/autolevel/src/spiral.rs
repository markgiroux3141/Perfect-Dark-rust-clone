//! A spiral tower: ramps that climb the walls, a storey a wall, with a deck between each.
//! A port of `autolevel/spiral.py`.
//!
//! The player circles up: a ramp along one wall to the corner, a deck round the corner and
//! along the next wall to the foot of the next ramp, and so on to the top, where the deck is a
//! ring. Each ramp has a landing two cells square at its top, in the corner, and at its foot:
//! room to turn. It is all fixed by the room's size, so it is worked out before any corridor is
//! routed, and corridors may only arrive where there is deck to arrive on.
//!
//! Everything is in half cells ("h", absolute).

use crate::cells::{rect_cells, Cells, Rect};
use std::collections::BTreeMap;

/// A ramp's run: four cells for a storey.
pub const RUN: i32 = 8;
/// A landing is at least this square.
pub const LAND: i32 = 4;

/// One ramp of a spiral.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ramp {
    /// The storeys it joins.
    pub lo: i32,
    pub hi: i32,
    pub rect: Rect,
    /// The way it climbs: an index into the router's directions (E, N, W, S).
    pub dir: usize,
    /// The landing past its top, and the one before its foot.
    pub top: Rect,
    pub foot: Rect,
}

/// A spiral: its ramps from the floor up, the deck on each upper storey, and the cells of the
/// floor under and before the first ramp, where no door may open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub ramps: Vec<Ramp>,
    pub decks: BTreeMap<i32, Cells>,
    pub clear: Cells,
}

/// The strip `depth` deep along wall `side` from u0 to u1, measured the anticlockwise way.
fn strip(r: Rect, side: usize, u0: i32, u1: i32, depth: i32) -> Rect {
    let (x0, y0, x1, y1) = r;
    match side {
        0 => (x0 + u0, y0, x0 + u1, y0 + depth),
        1 => (x1 - depth, y0 + u0, x1, y0 + u1),
        2 => (x1 - u1, y1 - depth, x1 - u0, y1),
        _ => (x0, y1 - u1, x0 + depth, y1 - u0),
    }
}

/// The spiral for a room with rectangle `r` and the given storeys (floor first).
///
/// `start` is the wall the first ramp climbs (0 south, 1 east, 2 north, 3 west), `turn` the way
/// round (1 anticlockwise seen from above, -1 clockwise), `band` the depth of ramps and decks
/// from the wall (2 for catwalks one cell wide). None for a room too small for a ramp and two
/// landings along each wall, or with one storey.
pub fn layout(r: Rect, storeys: &[i32], start: usize, turn: i32, band: i32) -> Option<Layout> {
    let (x0, y0, x1, y1) = r;
    let (w, d) = (x1 - x0, y1 - y0);
    let land = LAND.max(band);
    if w.min(d) < RUN + 2 * land || storeys.len() < 2 {
        return None;
    }
    if turn < 0 {
        // clockwise: lay it out mirrored, then mirror back
        let out = layout(r, storeys, [0, 3, 2, 1][start % 4], 1, band)?;
        let flip = |q: Rect| (x0 + x1 - q.2, q.1, x0 + x1 - q.0, q.3);
        let mirror = |cs: &Cells| cs.iter().map(|&(x, y)| (x0 + x1 - 1 - x, y)).collect::<Cells>();
        let ramps =
            out.ramps.iter().map(|p| Ramp { lo: p.lo, hi: p.hi, rect: flip(p.rect), dir: [2, 1, 0, 3][p.dir], top: flip(p.top), foot: flip(p.foot) }).collect();
        let decks = out.decks.iter().map(|(&z, cs)| (z, mirror(cs))).collect();
        return Some(Layout { ramps, decks, clear: mirror(&out.clear) });
    }
    let n = storeys.len() - 1;
    let mut ramps: Vec<Ramp> = Vec::new();
    let mut decks = BTreeMap::new();
    for k in 1..=n {
        let side = (start + k - 1) % 4;
        let len = if side.is_multiple_of(2) { w } else { d };
        let a = len - RUN - land; // the ramp ends a landing short of the far corner
        let top = strip(r, side, a + RUN, len, land);
        let foot = strip(r, side, a - land, a, land);
        let rect = strip(r, side, a, a + RUN, band);
        ramps.push(Ramp { lo: storeys[k - 1], hi: storeys[k], rect, dir: side, top, foot });
        let mut deck = rect_cells(top);
        if k < n {
            // round the corner and along the next wall to the next foot
            let next = (side + 1) % 4;
            let a2 = (if next.is_multiple_of(2) { w } else { d }) - RUN - land;
            deck.extend(rect_cells(strip(r, next, 0, a2, band)));
            deck.extend(rect_cells(strip(r, next, a2 - land, a2, land)));
        } else {
            // the top: a ring, broken only where the last ramp arrives
            for x in x0..x1 {
                for y in y0..y1 {
                    if x - x0 < band || x1 - x <= band || y - y0 < band || y1 - y <= band {
                        deck.insert((x, y));
                    }
                }
            }
            for c in rect_cells(rect) {
                deck.remove(&c);
            }
        }
        decks.insert(storeys[k], deck);
    }
    let mut clear = rect_cells(ramps[0].rect);
    clear.extend(rect_cells(ramps[0].foot));
    Some(Layout { ramps, decks, clear })
}
