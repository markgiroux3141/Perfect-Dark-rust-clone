//! The sculpt planner's rooms put together: placed near each other (some overlapping), stamped
//! into one grid with the later room cut back a wall's thickness from the earlier, then joined.
//! Rooms that share a thin wall get a doorway through it; others a corridor, routed through the
//! solid with ramps where the doorways are at different heights.
//!
//! ```text
//! let placed = place(&mut rng, rooms);         // where each sculpted room goes
//! let mut v = stamp(&placed, &mut zones);      // one grid, rooms cut to fit
//! let pairs = graph(&v, n, 0.35);              // a spanning tree and some loops
//! doorway(&mut v, a, b) / corridor(...)        // each pair joined
//! ```

use super::nav::{Sheet, STAND};
use super::shape::HEAD;
use super::vox::{add, up, Ix, Vox};
use crate::pyrand::PyRandom;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Solid round the rooms for corridors to be routed through, cells across and up.
pub const MARGIN: i32 = 14;
pub const MARGIN_UP: i32 = 10;
/// A corridor's air: cells across, cells up.
pub const WIDE: i32 = 2;
pub const TALL: i32 = 4;
/// States a corridor's search may open before it is given up.
const SEARCH: usize = 60_000;

const ACROSS: [Ix; 4] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0)];

fn int(rng: &mut PyRandom, a: i32, b: i32) -> i32 {
    rng.randint(a as i64, b as i64) as i32
}

fn perp(d: Ix) -> Ix {
    if d.0 != 0 {
        (0, 1, 0)
    } else {
        (1, 0, 0)
    }
}

/// A sculpted room and where its grid's corner goes (in the level's unshifted cells).
#[derive(Clone, Debug)]
pub struct Placed {
    pub vox: Vox,
    pub at: Ix,
}

impl Placed {
    fn hi(&self) -> Ix {
        add(self.at, (self.vox.nx, self.vox.ny, self.vox.nz))
    }
}

fn overlap(a: (Ix, Ix), b: (Ix, Ix)) -> i64 {
    let d = |l0: i32, h0: i32, l1: i32, h1: i32| (h0.min(h1) - l0.max(l1)).max(0) as i64;
    d(a.0 .0, a.1 .0, b.0 .0, b.1 .0) * d(a.0 .1, a.1 .1, b.0 .1, b.1 .1) * d(a.0 .2, a.1 .2, b.0 .2, b.1 .2)
}

/// Each room put beside one already placed, on one of its four sides: a gap of up to 9 cells or
/// an overlap of up to 3, slid along that side, a few cells up or down. A place that overlaps any
/// room by more than a fifth of the smaller is drawn again.
pub fn place(rng: &mut PyRandom, rooms: Vec<Vox>) -> Vec<Placed> {
    let mut out: Vec<Placed> = Vec::new();
    for vox in rooms {
        if out.is_empty() {
            out.push(Placed { vox, at: (0, 0, 0) });
            continue;
        }
        let (sx, sy, sz) = (vox.nx, vox.ny, vox.nz);
        for _ in 0..80 {
            let a = &out[rng.below(out.len() as u64) as usize];
            let (alo, ahi) = (a.at, a.hi());
            let gap = int(rng, -3, 9);
            let side = rng.below(4);
            let slide = |rng: &mut PyRandom, lo: i32, hi: i32, size: i32| int(rng, lo - size / 2, hi - size / 2);
            let x = match side {
                0 => ahi.0 + gap,
                1 => alo.0 - gap - sx,
                _ => slide(rng, alo.0, ahi.0, sx),
            };
            let y = match side {
                2 => ahi.1 + gap,
                3 => alo.1 - gap - sy,
                _ => slide(rng, alo.1, ahi.1, sy),
            };
            let z = alo.2 + int(rng, -6, 6);
            let at = (x, y, z);
            let me = (at, add(at, (sx, sy, sz)));
            let vol = |p: &Placed| (p.vox.nx * p.vox.ny * p.vox.nz) as i64;
            let mine = (sx * sy * sz) as i64;
            if out.iter().all(|p| overlap(me, (p.at, p.hi())) * 5 <= vol(p).min(mine)) {
                out.push(Placed { vox, at });
                break;
            }
        }
    }
    out
}

/// One grid of every room, room `i` zone `i + 1`, the solid margin round them. A room's air
/// within a cell of an earlier room's is left solid (the wall between them), and of what is left
/// only its largest part is kept. Returns the grid and the offset added to a room's place.
pub fn stamp(placed: &[Placed]) -> (Vox, Ix) {
    let lo = placed.iter().fold((i32::MAX, i32::MAX, i32::MAX), |m, p| (m.0.min(p.at.0), m.1.min(p.at.1), m.2.min(p.at.2)));
    let hi = placed.iter().fold((i32::MIN, i32::MIN, i32::MIN), |m, p| {
        let h = p.hi();
        (m.0.max(h.0), m.1.max(h.1), m.2.max(h.2))
    });
    let shift = (MARGIN - lo.0, MARGIN - lo.1, 2 - lo.2);
    let mut v = Vox::new(hi.0 - lo.0 + 2 * MARGIN, hi.1 - lo.1 + 2 * MARGIN, hi.2 - lo.2 + 2 + MARGIN_UP);
    for (i, p) in placed.iter().enumerate() {
        let zone = i as u16 + 1;
        for c in p.vox.each().filter(|&c| p.vox.air(c)).collect::<Vec<_>>() {
            let g = add(add(c, p.at), shift);
            let near = (-1..=1).any(|dx| {
                (-1..=1).any(|dy| {
                    (-1..=1).any(|dz| {
                        let z = v.get(add(g, (dx, dy, dz)));
                        z != 0 && z != zone
                    })
                })
            });
            if !near {
                v.set(g, zone);
            }
        }
        v.keep_largest(zone);
    }
    (v, shift)
}

/// The rooms' boxes in the grid (low, high exclusive), by zone - 1 (None: no air left).
pub fn boxes(v: &Vox, n: usize) -> Vec<Option<(Ix, Ix)>> {
    let mut out: Vec<Option<(Ix, Ix)>> = vec![None; n];
    for p in v.each() {
        let z = v.get(p) as usize;
        if z == 0 || z > n {
            continue;
        }
        let b = out[z - 1].get_or_insert((p, add(p, (1, 1, 1))));
        b.0 = (b.0 .0.min(p.0), b.0 .1.min(p.1), b.0 .2.min(p.2));
        b.1 = (b.1 .0.max(p.0 + 1), b.1 .1.max(p.1 + 1), b.1 .2.max(p.2 + 1));
    }
    out
}

/// Pairs of rooms (zones) to join: a spanning tree over how far apart their boxes are, then the
/// nearest other pairs, `loops` of the number of rooms of them.
pub fn graph(bx: &[Option<(Ix, Ix)>], loops: f64) -> Vec<(u16, u16, bool)> {
    let gap = |a: (Ix, Ix), b: (Ix, Ix)| {
        let g = |l0: i32, h0: i32, l1: i32, h1: i32| (l0 - h1).max(l1 - h0).max(0);
        g(a.0 .0, a.1 .0, b.0 .0, b.1 .0) + g(a.0 .1, a.1 .1, b.0 .1, b.1 .1) + g(a.0 .2, a.1 .2, b.0 .2, b.1 .2) / 2
    };
    let live: Vec<usize> = (0..bx.len()).filter(|&i| bx[i].is_some()).collect();
    let mut pairs: Vec<(i32, usize, usize)> = Vec::new();
    for (i, &a) in live.iter().enumerate() {
        for &b in &live[i + 1..] {
            pairs.push((gap(bx[a].unwrap(), bx[b].unwrap()), a, b));
        }
    }
    pairs.sort();
    let mut set: Vec<usize> = (0..bx.len()).collect();
    fn root(s: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while s[r] != r {
            r = s[r];
        }
        s[i] = r;
        r
    }
    let mut out = Vec::new();
    let mut rest = Vec::new();
    for &(_, a, b) in &pairs {
        let (ra, rb) = (root(&mut set, a), root(&mut set, b));
        if ra != rb {
            set[ra] = rb;
            out.push((a as u16 + 1, b as u16 + 1, true));
        } else {
            rest.push((a as u16 + 1, b as u16 + 1, false));
        }
    }
    let extra = ((live.len() as f64) * loops).round() as usize;
    out.extend(rest.into_iter().take(extra));
    out
}

/// A cell a player stands in with a doorway's height of their zone's air over it.
fn floor_under_door(v: &Vox, p: Ix, zone: u16) -> bool {
    v.get(p) == zone && !v.air(up(p, -1)) && (1..HEAD).all(|k| v.get(up(p, k)) == zone)
}

/// A doorway two cells wide through a one-cell wall between rooms `a` and `b`, where both have
/// floor at one height either side; the wall's cells become `a`'s air. The cells either side
/// (a's, b's), or None if the wall has no such place.
pub fn doorway(v: &mut Vox, rng: &mut PyRandom, a: u16, b: u16) -> Option<(Ix, Ix)> {
    let mut spots = Vec::new();
    for w in v.each() {
        if v.air(w) {
            continue;
        }
        for d in ACROSS {
            let t = perp(d);
            let ok = [w, add(w, t)].into_iter().all(|q| {
                let (pa, pb) = ((q.0 - d.0, q.1 - d.1, q.2), add(q, d));
                floor_under_door(v, pa, a) && floor_under_door(v, pb, b) && !v.air(up(q, -1)) && (0..=HEAD).all(|k| !v.air(up(q, k)))
            });
            if ok {
                spots.push((w, d));
            }
        }
    }
    if spots.is_empty() {
        return None;
    }
    let (w, d) = spots[rng.below(spots.len() as u64) as usize];
    let t = perp(d);
    for q in [w, add(w, t)] {
        for k in 0..HEAD {
            v.set(up(q, k), a);
        }
    }
    Some(((w.0 - d.0, w.1 - d.1, w.2), add(w, d)))
}

/// Where a corridor may leave a room: two cells of floor side by side against a wall two cells
/// thick, `d` the way out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub zone: u16,
    /// The room's two cells at the doorway.
    pub cells: [Ix; 2],
    pub d: Ix,
    /// The corridor's first place: its low corner (the doorway through the wall).
    pub at: Ix,
}

/// Places a corridor may leave room `zone` from, facing `toward` (a cell), the likeliest first,
/// at least three cells apart.
pub fn spots(v: &Vox, rng: &mut PyRandom, zone: u16, within: (Ix, Ix), toward: (f64, f64)) -> Vec<Spot> {
    let (lo, hi) = within;
    let mut all: Vec<(f64, Spot)> = Vec::new();
    for k in lo.2..hi.2 {
        for y in lo.1..hi.1 {
            for x in lo.0..hi.0 {
                let c = (x, y, k);
                if !floor_under_door(v, c, zone) {
                    continue;
                }
                for d in ACROSS {
                    let t = perp(d);
                    let c2 = add(c, t);
                    if !floor_under_door(v, c2, zone) {
                        continue;
                    }
                    let wall = [c, c2].into_iter().all(|q| (1..=WIDE).all(|j| (0..=TALL).all(|kk| !v.air((q.0 + d.0 * j, q.1 + d.1 * j, q.2 + kk)))));
                    if !wall {
                        continue;
                    }
                    let (fx, fy) = (toward.0 - x as f64, toward.1 - y as f64);
                    let len = (fx * fx + fy * fy).sqrt().max(1.0);
                    let facing = (fx * d.0 as f64 + fy * d.1 as f64) / len;
                    if facing < 0.2 {
                        continue;
                    }
                    let cells = [add(c, d), add(c2, d), (c.0 + 2 * d.0, c.1 + 2 * d.1, k), (c2.0 + 2 * d.0, c2.1 + 2 * d.1, k)];
                    let at = (cells.iter().map(|q| q.0).min().unwrap(), cells.iter().map(|q| q.1).min().unwrap(), k);
                    let score = facing * 3.0 + rng.random() * 2.0 - len / 60.0;
                    all.push((score, Spot { zone, cells: [c, c2], d, at }));
                }
            }
        }
    }
    all.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out: Vec<Spot> = Vec::new();
    for (_, s) in all {
        let near = out.iter().any(|o| (o.at.0 - s.at.0).abs() + (o.at.1 - s.at.1).abs() + (o.at.2 - s.at.2).abs() < 3);
        if !near {
            out.push(s);
        }
        if out.len() >= 10 {
            break;
        }
    }
    out
}

/// A place of a corridor: its low corner, the air's span up (from, to exclusive), the way it was
/// entered and the rise of that move (0: flat; else a ramp).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub at: Ix,
    pub span: (i32, i32),
    pub dir: usize,
    pub dk: i32,
}

/// Whether a corridor's air can go here: every cell solid, and nothing round it the air of any
/// zone but `allow`.
fn fits(v: &Vox, at: Ix, span: (i32, i32), allow: u16) -> bool {
    let inside = |q: Ix| q.0 >= 1 && q.1 >= 1 && q.2 >= 1 && q.0 < v.nx - 1 && q.1 < v.ny - 1 && q.2 < v.nz - 1;
    let ours = |q: Ix| q.0 >= at.0 && q.0 < at.0 + WIDE && q.1 >= at.1 && q.1 < at.1 + WIDE && q.2 >= span.0 && q.2 < span.1;
    for i in 0..WIDE {
        for j in 0..WIDE {
            for k in span.0..span.1 {
                let q = (at.0 + i, at.1 + j, k);
                if !inside(q) || v.air(q) {
                    return false;
                }
                for d in super::vox::DIRS {
                    let n = add(q, d);
                    if ours(n) {
                        continue;
                    }
                    let z = v.get(n);
                    if z != 0 && z != allow {
                        return false;
                    }
                }
            }
        }
    }
    true
}

fn dir_index(d: Ix) -> usize {
    ACROSS.iter().position(|&a| a == d).expect("a way across")
}

/// A corridor from spot `a` to spot `b`: its places in order, or None if no way was found.
/// Moves are one or two cells flat, or two cells and one or two cells up or down (a ramp) in
/// the way it was already going; a ramp is straight and ends flat before it turns.
pub fn corridor(v: &Vox, a: &Spot, b: &Spot) -> Option<Vec<Place>> {
    let start = Place { at: a.at, span: (a.at.2, a.at.2 + TALL), dir: dir_index(a.d), dk: 0 };
    if !fits(v, a.at, start.span, a.zone) || !fits(v, b.at, (b.at.2, b.at.2 + TALL), b.zone) {
        return None;
    }
    let into_b = dir_index((-b.d.0, -b.d.1, 0));
    type Key = (Ix, usize, i32);
    let key = |p: &Place| (p.at, p.dir, p.dk);
    let h = |p: Ix| ((p.0 - b.at.0).abs() + (p.1 - b.at.1).abs()).max((p.2 - b.at.2).abs());
    let mut best: HashMap<Key, (u32, Option<Key>, Place)> = HashMap::new();
    best.insert(key(&start), (0, None, start));
    // weighted (the estimate counts half again): a corridor a little longer than the shortest,
    // found in a fraction of the search
    let mut heap = BinaryHeap::from([(Reverse(h(start.at) as u32 * 15), 0u32, key(&start))]);
    let mut opened = 0;
    let mut goal = None;
    while let Some((_, g, k)) = heap.pop() {
        let (gk, _, p) = best[&k];
        if gk < g {
            continue;
        }
        if p.at == b.at && p.dk == 0 && p.dir == into_b {
            goal = Some(k);
            break;
        }
        opened += 1;
        if opened > SEARCH {
            return None;
        }
        for (nd, d) in ACROSS.iter().enumerate() {
            if nd ^ 1 == p.dir {
                continue; // no turning back
            }
            if p.dk != 0 && nd != p.dir {
                continue; // a ramp ends flat before a turn
            }
            let mut moves: Vec<(i32, i32)> = vec![(1, 0), (2, 0)];
            if nd == p.dir {
                moves.extend([(2, 1), (2, -1), (2, 2), (2, -2)]);
            }
            for (len, dk) in moves {
                if p.dk != 0 && dk != 0 && dk != p.dk {
                    continue; // one slope a ramp
                }
                let at = (p.at.0 + d.0 * len, p.at.1 + d.1 * len, p.at.2 + dk);
                let span = if dk == 0 { (at.2, at.2 + TALL) } else { (p.at.2.min(at.2), p.at.2.max(at.2) + TALL) };
                let arriving = at == b.at && dk == 0;
                if !fits(v, at, span, if arriving { b.zone } else { 0 }) {
                    continue;
                }
                let cost = if dk == 0 { len as u32 * 10 } else { 28 } + if nd != p.dir { 20 } else { 0 };
                let np = Place { at, span, dir: nd, dk };
                let nk = key(&np);
                let ng = g + cost;
                if best.get(&nk).is_none_or(|e| ng < e.0) {
                    best.insert(nk, (ng, Some(k), np));
                    heap.push((Reverse(ng + h(at) as u32 * 15), ng, nk));
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut k = goal?;
    loop {
        let (_, from, p) = best[&k];
        out.push(p);
        match from {
            Some(f) => k = f,
            None => break,
        }
    }
    out.reverse();
    Some(out)
}

/// Carve a corridor's places as `zone`, and the sheets of its ramps.
pub fn carve(v: &mut Vox, zone: u16, places: &[Place]) -> Vec<Sheet> {
    for p in places {
        for i in 0..WIDE {
            for j in 0..WIDE {
                for k in p.span.0..p.span.1 {
                    v.set((p.at.0 + i, p.at.1 + j, k), zone);
                }
            }
        }
    }
    let mut sheets = Vec::new();
    let mut i = 1;
    while i < places.len() {
        if places[i].dk == 0 {
            i += 1;
            continue;
        }
        let first = i;
        while i + 1 < places.len() && places[i + 1].dk == places[first].dk {
            i += 1;
        }
        let (before, last) = (places[first - 1], places[i]);
        let d = ACROSS[places[first].dir];
        let along = |p: Ix| if d.0 != 0 { p.0 } else { p.1 };
        let across = if d.0 != 0 { places[first].at.1 } else { places[first].at.0 };
        // the faces it starts and ends on, along the way it runs
        let positive = d.0 + d.1 > 0;
        let f0 = if positive { along(places[first].at) } else { along(places[first].at) + WIDE };
        let f1 = if positive { along(last.at) + WIDE } else { along(last.at) };
        let (k0, k1) = (before.at.2, last.at.2);
        let before_face = |f: i32| if positive { f - 1 } else { f };
        let after_face = |f: i32| if positive { f } else { f - 1 };
        let cell = |a: i32, lane: i32, k: i32| if d.0 != 0 { (a, across + lane, k) } else { (across + lane, a, k) };
        let low_end = [cell(before_face(f0), 0, k0), cell(before_face(f0), 1, k0)];
        let high_end = [cell(after_face(f1), 0, k1), cell(after_face(f1), 1, k1)];
        let top = k0.max(k1);
        let mut cover = Vec::new();
        for p in &places[first..=i] {
            for a in 0..WIDE {
                for b in 0..WIDE {
                    cover.push((p.at.0 + a, p.at.1 + b, top));
                }
            }
        }
        let mid = across + 1;
        let pt = |f: i32, k: i32| if d.0 != 0 { (f, mid, k) } else { (mid, f, k) };
        let (foot, topc, start, end) = if k0 < k1 { (low_end, high_end, pt(f0, k0), pt(f1, k1)) } else { (high_end, low_end, pt(f1, k1), pt(f0, k0)) };
        sheets.push(Sheet { zone, foot, top: topc, cover, start, end, open: false });
        i += 1;
    }
    let _ = STAND;
    sheets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sculpt::nav::Nav;

    /// Two rooms 6 x 6 side by side 10 cells apart, the second 4 cells higher.
    fn two() -> Vox {
        let mut v = Vox::new(40, 20, 20);
        for p in v.each().collect::<Vec<_>>() {
            let (x, y, z) = p;
            if (4..10).contains(&x) && (6..12).contains(&y) && (3..9).contains(&z) {
                v.set(p, 1);
            }
            if (20..26).contains(&x) && (6..12).contains(&y) && (7..13).contains(&z) {
                v.set(p, 2);
            }
        }
        v
    }

    #[test]
    fn a_corridor_joins_rooms_at_two_heights() {
        let mut v = two();
        let mut rng = PyRandom::from_int(1);
        let a = spots(&v, &mut rng, 1, ((0, 0, 0), (40, 20, 20)), (23.0, 9.0));
        let b = spots(&v, &mut rng, 2, ((0, 0, 0), (40, 20, 20)), (7.0, 9.0));
        let route = a.iter().flat_map(|sa| b.iter().map(move |sb| (sa, sb))).find_map(|(sa, sb)| corridor(&v, sa, sb).map(|r| (*sa, *sb, r)));
        let (sa, sb, places) = route.expect("a corridor");
        let sheets = carve(&mut v, 3, &places);
        assert!(!sheets.is_empty(), "a ramp for the 4 cells' climb");
        let nav = Nav::build(&v, &sheets, None);
        assert!(nav.path(sa.cells[0], sb.cells[0]).is_some());
        assert!(nav.path(sb.cells[1], sa.cells[1]).is_some());
    }
}
