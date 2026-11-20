//! What stands on a room's floor, and the room's own outline. From `furnish.py` (`lay_cover`,
//! `_colonnade`, `_shape`).
//!
//! By the room's mode: "pillars" (a colonnade, each pillar up to the ceiling or to the underside
//! of the deck over it), "crates" (low housings), "lanes" (only the pillars that break a long
//! lane between doors) or "none". Nothing in front of a door, nothing tall within three cells
//! of one, and nothing that cuts the floor in two.

use super::{bounds, grow, meets, overlap, Cover, Interior, CLEAR};
use crate::cells::{rect_cells, Cells, Rect};
use crate::py;
use crate::pyrand::PyRandom;
use crate::space::door_pad;
use crate::storeys::storey;

/// A ladder up the side of a pillar to its top: a pillar in a room of more than one storey stops
/// at the first storey's ceiling (`reach`), and where nothing is over it its top is a place to
/// stand, climbed to by this. New in the Rust (the user's call, 2026-11-19).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ladder {
    /// The pillar: an index into `Interior::cover`.
    pub pillar: usize,
    /// The side of it the ladder is on (0 south, 1 east, 2 north, 3 west).
    pub side: usize,
}

/// The least headroom over a pillar's top for a ladder to it, units (a player is 1.6 m tall).
const HEADROOM: i32 = 200;

/// A line between two doors on the floor: its length, and the middle of each door's pad, h.
type Lane = (f64, (f64, f64), (f64, f64));

/// The floor as cover sees it (what `lay_cover`'s inner functions close over in the Python).
struct Site {
    /// The floor's height.
    z: i32,
    /// Nothing at all here.
    clear: Cells,
    /// Nothing tall here.
    low_only: Cells,
    floor: Cells,
    /// What has been stood on it so far.
    placed: Vec<Cover>,
    /// Floor to ceiling.
    full: i32,
    /// Each upper storey's deck and pods' roofs.
    above: Vec<(i32, Cells)>,
    /// What a pillar may not run up through.
    no_pillar: Cells,
}

impl Interior {
    /// How tall a pillar there stands: to the ceiling, or to the underside of a deck that
    /// covers it. 0 where it would clip a deck's edge, a ramp or a pod.
    fn reach(&self, st: &Site, x: i32, y: i32, w: i32, d: i32) -> i32 {
        let cells = rect_cells((x, y, x + w, y + d));
        let grown = rect_cells((x - 2, y - 2, x + w + 2, y + d + 2));
        if meets(&grown, &st.no_pillar) {
            return 0;
        }
        // A pillar stops at its storey's ceiling, under the first storey's deck or where it would
        // be (the Rust's own, the user's call, 2026-11-19: to the hall's ceiling, as the Python
        // stood them, a tall hall was a forest of columns), so only that deck stands in its way.
        let first = storey(st.z).1 - st.z;
        for (zz, deck) in &st.above {
            if zz - 40 - st.z > first {
                break;
            }
            if meets(&grown, deck) {
                return if cells.is_subset(deck) { (zz - 40 - st.z).min(first) } else { 0 };
            }
        }
        st.full.min(first)
    }

    fn fits(&self, st: &Site, x: i32, y: i32, w: i32, d: i32, tall: bool) -> bool {
        let cells = rect_cells((x, y, x + w, y + d));
        if !cells.is_subset(&st.floor) || meets(&cells, &st.clear) || (tall && meets(&cells, &st.low_only)) {
            return false;
        }
        if [x - self.x0, self.x1 - x - w, y - self.y0, self.y1 - y - d].iter().any(|&g| 0 < g && g < 2) {
            return false; // flush to a wall, or a cell clear of it
        }
        let grown = (x - 2, y - 2, x + w + 2, y + d + 2);
        !st.placed.iter().any(|c| overlap(grown, (c.0, c.1, c.2, c.3)))
    }

    /// Stand something there, unless it cuts a door off from another.
    fn place(&mut self, st: &mut Site, x: i32, y: i32, (w, d): (i32, i32), h: i32, tag: &'static str) -> bool {
        let item = (x, y, x + w, y + d, h, tag);
        self.cover.push(item);
        if self.connected() {
            st.placed.push(item);
            return true;
        }
        self.cover.pop();
        false
    }

    pub(super) fn lay_cover(&mut self) {
        let z = self.levels[0];
        let area = self.s.area();
        if !self.s.host.is_empty() || area < 9 {
            return;
        }
        let mut clear = self.apron(z, CLEAR);
        let low_only = self.apron(z, 6);
        for r in self.ramps.iter().filter(|r| r.lo == z) {
            clear.extend(rect_cells(grow(r.rect, 2)));
            clear.extend(r.foot.iter().copied());
            let foot = bounds(&r.foot);
            clear.extend(rect_cells((foot.0 - 2, foot.1 - 2, foot.2 + 2, foot.3 + 2)));
        }
        if !self.pool.is_empty() {
            clear.extend(rect_cells(grow(bounds(&self.pool), 2)));
        }
        if !self.hole.is_empty() {
            clear.extend(rect_cells(grow(bounds(&self.hole), 2)));
        }
        for &k in &self.keep {
            clear.extend(rect_cells(grow(k, 2)));
        }
        clear.extend(self.sunk_clear());
        for p in &self.piers {
            clear.extend(rect_cells(grow((p.0, p.1, p.2, p.3), 2)));
        }
        let floor: Cells = self.room.iter().filter(|c| !self.pool.contains(c) && !self.hole.contains(c) && !self.sunk_cells.contains(c)).copied().collect();
        let above: Vec<(i32, Cells)> = self.upper.iter().map(|&zz| (zz, self.deck[&zz].union(&self.roof[&zz]).copied().collect())).collect();
        let mut no_pillar = Cells::new();
        for zz in &self.upper {
            no_pillar.extend(self.shell[zz].iter().copied());
            no_pillar.extend(self.spanned(*zz)); // (a pillar would stop under an angled catwalk)
        }
        for r in self.ramps.iter().filter(|r| r.lo != z) {
            no_pillar.extend(r.cells());
            no_pillar.extend(r.top.iter().copied());
            no_pillar.extend(r.foot.iter().copied());
        }
        let mut st = Site { z, clear, low_only, floor, placed: Vec::new(), full: self.s.top - z, above, no_pillar };

        if self.shape.is_some() && self.levels.len() == 1 && self.pool.is_empty() {
            self.shape_room(&mut st);
        }
        let mode = self.cover_mode;
        if mode == "none" || self.clutter <= 0.0 {
            return;
        }
        // first, a pillar on the line between each pair of doors that face down a long lane
        let mids: Vec<(f64, f64)> = self
            .doors
            .iter()
            .filter(|dr| dr.level == z)
            .map(|dr| bounds(&door_pad(&self.s, dr, self.band, 0)))
            .map(|p| ((p.0 + p.2) as f64 / 2.0, (p.1 + p.3) as f64 / 2.0))
            .collect();
        let mut lanes: Vec<Lane> = Vec::new();
        for (i, &a) in mids.iter().enumerate() {
            for &b in &mids[i + 1..] {
                lanes.push(((a.0 - b.0).abs() + (a.1 - b.1).abs(), a, b));
            }
        }
        lanes.sort_by(|p, q| q.partial_cmp(p).expect("no lane is NaN")); // the longest first
        let sizes: &[(i32, i32)] = if self.w.min(self.d) < 12 { &[(2, 2)] } else { &[(4, 4), (4, 2), (2, 4), (2, 2)] };
        let mut broken = 0;
        for (length, a, b) in lanes {
            if length < 16.0 || broken >= 2 {
                continue; // under eight cells: not a lane
            }
            'spots: for t in [0.5, 0.4, 0.6, 0.3, 0.7] {
                let (mx, my) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                for &(w, d) in sizes {
                    let x = py::round((mx - w as f64 / 2.0) / 2.0) as i32 * 2;
                    let y = py::round((my - d as f64 / 2.0) / 2.0) as i32 * 2;
                    let h = self.reach(&st, x, y, w, d);
                    if h != 0 && self.fits(&st, x, y, w, d, true) && self.place(&mut st, x, y, (w, d), h, self.pillar) {
                        broken += 1;
                        break 'spots;
                    }
                }
            }
        }
        if mode == "pillars" {
            self.colonnade(&mut st);
            return;
        }
        if mode == "lanes" {
            return; // the lane-breaking pillars and nothing else
        }
        let want = 6.min((if area >= 12 { 1 } else { 0 }).max(py::round(self.clutter * area as f64 / 15.0)));
        let mut tries = 0;
        while (st.placed.iter().filter(|c| c.5 == "housing").count() as i64) < want && tries < 200 {
            tries += 1;
            let (w, d) = *self.rng.choice(&[(2, 2), (2, 2), (4, 2), (2, 4), (4, 4)]);
            if self.w.min(self.d) < 10 && (w, d) == (4, 4) {
                continue;
            }
            let x = self.rng.randrange(self.x0 as i64, (self.x1 - w + 1) as i64, 2) as i32;
            let y = self.rng.randrange(self.y0 as i64, (self.y1 - d + 1) as i64, 2) as i32;
            if self.fits(&st, x, y, w, d, false) {
                self.place(&mut st, x, y, (w, d), 118, "housing");
            }
        }
    }

    /// A ladder up each pillar that stops at the first storey's ceiling with headroom over it, on a
    /// side chosen at random (the room's own stream, "{seed}/{key}/ladders") from those whose floor
    /// in front is standing room. In a room of one storey a pillar runs to the ceiling: none.
    pub(super) fn lay_ladders(&mut self) {
        let z = self.levels[0];
        if self.upper.is_empty() {
            return;
        }
        let first = storey(z).1 - z;
        let stand = self.stand(z);
        let mut rng = PyRandom::named(&format!("{}/ladders", self.stream));
        let mut out = Vec::new();
        for (n, &(x0, y0, x1, y1, h, tag)) in self.cover.iter().enumerate() {
            if tag != self.pillar || h != first || z + h + HEADROOM > self.s.top {
                continue;
            }
            // nothing over its top (the first storey's deck is the only one `reach` keeps it from)
            let top = rect_cells((x0, y0, x1, y1));
            let over = self.upper.iter().any(|zz| {
                zz - 40 < z + h + HEADROOM && [&self.deck[zz], &self.roof[zz], &self.shell[zz], &self.spanned(*zz)].iter().any(|c| !c.is_disjoint(&top))
            });
            if over {
                continue;
            }
            let mut sides = [0, 1, 2, 3];
            rng.shuffle(&mut sides);
            if let Some(&side) = sides.iter().find(|&&side| rect_cells(front((x0, y0, x1, y1), side)).is_subset(&stand)) {
                out.push(Ladder { pillar: n, side });
            }
        }
        self.ladders = out;
    }

    /// A ladder's two ends on its pillar's side, cells, one cell wide in its middle (as a cellar's:
    /// `Cellar::ladder`), and the way it faces (out of the pillar).
    pub fn ladder_line(&self, l: &Ladder) -> ((f64, f64), (f64, f64), (i32, i32)) {
        let (x0, y0, x1, y1, _, _) = self.cover[l.pillar];
        let (x0, y0, x1, y1) = (x0 as f64 / 2.0, y0 as f64 / 2.0, x1 as f64 / 2.0, y1 as f64 / 2.0);
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        match l.side {
            0 => ((mx - 0.5, y0), (mx + 0.5, y0), (0, -1)),
            1 => ((x1, my - 0.5), (x1, my + 0.5), (1, 0)),
            2 => ((mx - 0.5, y1), (mx + 0.5, y1), (0, 1)),
            _ => ((x0, my - 0.5), (x0, my + 0.5), (-1, 0)),
        }
    }

    /// Where a ladder is climbed from (the middle of the cell in front of it, on the floor) and
    /// where it ends (the middle of its pillar's top), cells.
    pub fn ladder_ends(&self, l: &Ladder) -> ((f64, f64, i32), (f64, f64, i32)) {
        let (x0, y0, x1, y1, h, _) = self.cover[l.pillar];
        let f = front((x0, y0, x1, y1), l.side);
        let z = self.levels[0];
        let mid = |r: Rect| ((r.0 + r.2) as f64 / 4.0, (r.1 + r.3) as f64 / 4.0);
        let ((fx, fy), (tx, ty)) = (mid(f), mid((x0, y0, x1, y1)));
        ((fx, fy, z), (tx, ty, z + h))
    }

    /// Rows of pillars down the room's long axis: one row for each seven cells of width, a
    /// pillar every three to six cells (by `clutter`), two cells square in the widest halls; in a
    /// room of more than one storey, one row for each fourteen cells and half as far again apart.
    /// Each stands to its storey's ceiling, or to the deck over it (`reach`).
    fn colonnade(&mut self, st: &mut Site) {
        let long_x = self.w >= self.d;
        let (lh, sh) = if long_x { (self.w, self.d) } else { (self.d, self.w) };
        let size = if sh >= 28 { 4 } else { 2 };
        // (in a room of more than one storey, half the rows and half as far again between pillars:
        // the Rust's own, the user's call, 2026-11-19)
        let tall = !self.upper.is_empty();
        let rows = 1.max(py::round(sh as f64 / if tall { 28.0 } else { 14.0 })) as i32;
        let mut step = 2 * py::round(2.0 + 1.0 / self.clutter.max(0.2)) as i32 + if size == 4 { 4 } else { 0 };
        if tall {
            step = 2 * py::round(step as f64 * 0.75) as i32;
        }
        let n = 1.max((lh - 8 - size).div_euclid(step) + 1);
        let mut start = (lh - ((n - 1) * step + size)).div_euclid(2);
        start -= start.rem_euclid(2);
        // where its place is taken (a door's apron, the edge of a deck) a pillar moves a cell:
        // across the room first, which puts it under a gallery's edge or clear of it
        let shifts = [(0, 0), (0, -2), (0, 2), (-2, 0), (2, 0), (0, -4), (0, 4)];
        for i in 0..rows {
            let c = py::round((sh as f64 * (i as f64 + 0.5) / rows as f64 - size as f64 / 2.0) / 2.0) as i32 * 2;
            for j in 0..n {
                let u = start + j * step;
                for (du, dc) in shifts {
                    let (x, y) = if long_x { (self.x0 + u + du, self.y0 + c + dc) } else { (self.x0 + c + dc, self.y0 + u + du) };
                    let h = self.reach(st, x, y, size, size);
                    if h != 0 && self.fits(st, x, y, size, size, true) && self.place(st, x, y, (size, size), h, self.pillar) {
                        break;
                    }
                }
            }
        }
    }

    /// The room's own outline: a corner filled in with wall (an L), or a wall across it with a
    /// three-cell gap at one end (two bays). `_shape` in the Python.
    fn shape_room(&mut self, st: &mut Site) {
        let (x0, y0, x1, y1) = (self.x0, self.y0, self.x1, self.y1);
        let full = st.full;
        if self.shape == Some("notch") {
            let mut corners = [(0, 0), (1, 0), (0, 1), (1, 1)];
            self.rng.shuffle(&mut corners);
            for (cx, cy) in corners {
                let w = 2 * self.rng.randint(2, 2.max(4.min(self.w / 5)) as i64) as i32;
                let d = 2 * self.rng.randint(2, 2.max(4.min(self.d / 5)) as i64) as i32;
                let (x, y) = (if cx == 0 { x0 } else { x1 - w }, if cy == 0 { y0 } else { y1 - d });
                if self.fits(st, x, y, w, d, true) && self.place(st, x, y, (w, d), full, "wall") {
                    self.shaped = "notch";
                    return;
                }
            }
        } else if self.shape == Some("partition") {
            let long_x = self.w >= self.d;
            let (lh, sh) = if long_x { (self.w, self.d) } else { (self.d, self.w) };
            if lh < 16 || sh < 10 {
                return;
            }
            for _ in 0..8 {
                let u = 2 * py::round(lh as f64 * self.rng.uniform(0.35, 0.65) / 2.0) as i32;
                let length = sh - 6;
                let low = self.rng.random() < 0.5;
                let (x, y, w, d) =
                    if long_x { (x0 + u, if low { y0 } else { y1 - length }, 2, length) } else { (if low { x0 } else { x1 - length }, y0 + u, length, 2) };
                if self.fits(st, x, y, w, d, true) && self.place(st, x, y, (w, d), full, "wall") {
                    self.shaped = "partition";
                    return;
                }
            }
        }
    }
}

/// The floor in front of a pillar's side, h: a cell deep, its width.
fn front((x0, y0, x1, y1): Rect, side: usize) -> Rect {
    match side {
        0 => (x0, y0 - 2, x1, y0),
        1 => (x1, y0, x1 + 2, y1),
        2 => (x0, y1, x1, y1 + 2),
        _ => (x0 - 2, y0, x0, y1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{generate, Options};
    use crate::place::Scale;

    #[test]
    fn a_pillar_that_stops_at_the_first_storey_has_a_ladder_up_to_its_clear_top() {
        let (mut ladders, mut sides) = (0, [0; 4]);
        for (scale, seeds) in [(Scale::Grand, 1..=6), (Scale::Towering, 1..=3)] {
            for seed in seeds {
                let mut o = Options::python();
                o.siting.scale = scale;
                let lv = generate(seed, &o).unwrap();
                for (key, it) in &lv.interiors {
                    let z = it.levels[0];
                    if it.upper.is_empty() {
                        assert!(it.ladders.is_empty(), "seed {seed}: {key}: a pillar to the ceiling");
                        continue;
                    }
                    let first = storey(z).1 - z;
                    let stand = it.stand(z);
                    for l in &it.ladders {
                        let at = format!("seed {seed}: {key}: pillar {}", l.pillar);
                        let (x0, y0, x1, y1, h, tag) = it.cover[l.pillar];
                        assert_eq!((h, tag), (first, it.pillar), "{at}");
                        // the floor in front is standing room, and nothing is over the top
                        assert!(rect_cells(front((x0, y0, x1, y1), l.side)).is_subset(&stand), "{at}");
                        let top = rect_cells((x0, y0, x1, y1));
                        for zz in &it.upper {
                            if zz - 40 < z + h + HEADROOM {
                                assert!(it.deck[zz].is_disjoint(&top) && it.roof[zz].is_disjoint(&top) && it.shell[zz].is_disjoint(&top), "{at}");
                            }
                        }
                        let ((_, _, fz), (tx, ty, tz)) = it.ladder_ends(l);
                        assert_eq!((fz, tz), (z, z + first), "{at}");
                        assert!(tx > x0 as f64 / 2.0 && tx < x1 as f64 / 2.0 && ty > y0 as f64 / 2.0 && ty < y1 as f64 / 2.0, "{at}");
                        assert!(lv.walks.iter().any(|(name, _)| name.starts_with(&format!("pillar_{key}_"))), "{at}: no walk up it");
                        ladders += 1;
                        sides[l.side] += 1;
                    }
                    // every pillar with a clear top has one, unless no side of it has floor in front
                    let laddered: Vec<usize> = it.ladders.iter().map(|l| l.pillar).collect();
                    for (n, c) in it.cover.iter().enumerate() {
                        if c.5 == it.pillar && c.4 == first && !laddered.contains(&n) {
                            let open = (0..4).any(|side| rect_cells(front((c.0, c.1, c.2, c.3), side)).is_subset(&stand));
                            let top = rect_cells((c.0, c.1, c.2, c.3));
                            let over = it.upper.iter().any(|zz| {
                                zz - 40 < z + c.4 + HEADROOM
                                    && [&it.deck[zz], &it.roof[zz], &it.shell[zz], &it.spanned(*zz)].iter().any(|o| !o.is_disjoint(&top))
                            });
                            assert!(!open || over || z + c.4 + HEADROOM > it.s.top, "seed {seed}: {key}: pillar {n} has no ladder");
                        }
                    }
                }
            }
        }
        // the sides are the stream's: every one is drawn
        assert!(ladders >= 40 && sides.iter().all(|&n| n >= 5), "{ladders} ladders, by side {sides:?}");
    }
}
