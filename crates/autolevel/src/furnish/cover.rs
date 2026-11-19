//! What stands on a room's floor, and the room's own outline. From `furnish.py` (`lay_cover`,
//! `_colonnade`, `_shape`).
//!
//! By the room's mode: "pillars" (a colonnade, each pillar up to the ceiling or to the underside
//! of the deck over it), "crates" (low housings), "lanes" (only the pillars that break a long
//! lane between doors) or "none". Nothing in front of a door, nothing tall within three cells
//! of one, and nothing that cuts the floor in two.

use super::{bounds, grow, meets, overlap, Cover, Interior, CLEAR};
use crate::cells::{rect_cells, Cells};
use crate::py;
use crate::space::door_pad;

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
        for (zz, deck) in &st.above {
            if meets(&grown, deck) {
                return if cells.is_subset(deck) { zz - 40 - st.z } else { 0 };
            }
        }
        st.full
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
        for p in &self.piers {
            clear.extend(rect_cells(grow((p.0, p.1, p.2, p.3), 2)));
        }
        let floor: Cells = self.room.iter().filter(|c| !self.pool.contains(c) && !self.hole.contains(c)).copied().collect();
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

    /// Rows of pillars down the room's long axis: one row for each seven cells of width, a
    /// pillar every three to six cells (by `clutter`), two cells square in the widest halls.
    /// Each stands to the ceiling, or to the deck over it.
    fn colonnade(&mut self, st: &mut Site) {
        let long_x = self.w >= self.d;
        let (lh, sh) = if long_x { (self.w, self.d) } else { (self.d, self.w) };
        let size = if sh >= 28 { 4 } else { 2 };
        let rows = 1.max(py::round(sh as f64 / 14.0)) as i32;
        let step = 2 * py::round(2.0 + 1.0 / self.clutter.max(0.2)) as i32 + if size == 4 { 4 } else { 0 };
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
