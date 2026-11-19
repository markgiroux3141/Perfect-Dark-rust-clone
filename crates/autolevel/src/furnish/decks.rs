//! The walls as a loop, the decks laid along them, bridges between galleries, and a basin's
//! bridge and tanks. From `furnish.py` (`slot` to `walk`, and `lay_pool`).
//!
//! A perimeter position ("slot") counts half cells anticlockwise from the south-west corner:
//! along the south wall, up the east, back along the north, down the west.

use super::{components, meets, runs, touching, Interior, Pad, RampIn, GALLERIES_MIN, MEZZANINE_MIN, RING_MIN};
use crate::cells::{rect_cells, Cell, Cells};
use crate::py;
use crate::space::door_pad;
use std::collections::BTreeSet;

impl Interior {
    /// Perimeter positions of a cell next to a wall (a corner cell has two).
    pub fn slot(&self, c: Cell) -> Vec<i32> {
        let (x, y) = c;
        let mut out = Vec::new();
        if y == self.y0 {
            out.push(x - self.x0);
        }
        if x == self.x1 - 1 {
            out.push(self.w + y - self.y0);
        }
        if y == self.y1 - 1 {
            out.push(self.w + self.d + self.x1 - 1 - x);
        }
        if x == self.x0 {
            out.push(2 * self.w + self.d + self.y1 - 1 - y);
        }
        out
    }

    /// Deck cells for one perimeter position: `band` deep from the wall.
    pub fn slot_cells(&self, t: i32) -> Cells {
        let (w, d) = (self.w, self.d);
        let t = t.rem_euclid(2 * (w + d));
        let k = 0..self.band;
        if t < w {
            return k.map(|k| (self.x0 + t, self.y0 + k)).collect();
        }
        if t < w + d {
            return k.map(|k| (self.x1 - 1 - k, self.y0 + t - w)).collect();
        }
        if t < 2 * w + d {
            return k.map(|k| (self.x1 - 1 - (t - w - d), self.y1 - 1 - k)).collect();
        }
        k.map(|k| (self.x0 + k, self.y1 - 1 - (t - 2 * w - d))).collect()
    }

    /// The perimeter position each wall starts at: south, east, north, west.
    pub fn corners(&self) -> [i32; 4] {
        [0, self.w, self.w + self.d, 2 * self.w + self.d]
    }

    /// The perimeter positions along one wall.
    fn wall(&self, side: usize) -> std::ops::Range<i32> {
        let c0 = self.corners()[side];
        c0..c0 + if side.is_multiple_of(2) { self.w } else { self.d }
    }

    /// Perimeter positions to deck: every needed one, joined the short way round (the longest
    /// gap between them stays open, and so does any gap a pod blocks). `whole`: the deck must be
    /// one piece (a towering hall's high storey, which no ramp joins to anything): where a pod
    /// blocks a gap, the longest is decked instead, if nothing blocks that.
    pub fn arc(&self, need: &BTreeSet<i32>, blocked: &BTreeSet<i32>, ring: bool, whole: bool) -> BTreeSet<i32> {
        let p = 2 * (self.w + self.d);
        if ring {
            return (0..p).filter(|t| !blocked.contains(t)).collect();
        }
        let need: Vec<i32> = need.iter().copied().collect();
        if need.is_empty() {
            return BTreeSet::new();
        }
        let mut out: BTreeSet<i32> = need.iter().copied().collect();
        let mut gaps = Vec::new();
        for (i, &a) in need.iter().enumerate() {
            let b = if i + 1 < need.len() { need[i + 1] } else { need[0] + p };
            if b - a > 1 {
                gaps.push((b - a, a, b));
            }
        }
        if let Some(longest) = py::first_max(gaps.iter().copied(), |g| *g) {
            let span = |g: &(i32, i32, i32)| -> BTreeSet<i32> { (g.1 + 1..g.2).map(|t| t.rem_euclid(p)).collect() };
            let mut cut = false;
            for g in &gaps {
                let span = span(g);
                if *g == longest {
                    continue;
                }
                if !span.is_disjoint(blocked) {
                    cut = true;
                    continue;
                }
                out.extend(span);
            }
            if whole && cut && span(&longest).is_disjoint(blocked) {
                out.extend(span(&longest));
            }
        }
        // an arc that stops just short of a corner runs on to it
        let corners = self.corners();
        for run in runs(out.iter().copied(), Some(p)) {
            for (t, step) in [(run[run.len() - 1], 1), (run[0], -1)] {
                for k in 1..7 {
                    let q = (t + step * k).rem_euclid(p);
                    if blocked.contains(&q) || out.contains(&q) {
                        break;
                    }
                    if corners.contains(&if step > 0 { (q + 1).rem_euclid(p) } else { q }) {
                        out.extend((1..=k).map(|j| (t + step * j).rem_euclid(p)));
                        break;
                    }
                }
            }
        }
        out.retain(|t| !blocked.contains(t));
        out
    }

    // ------------------------------------------------------------ decks

    pub(super) fn lay_decks(&mut self) {
        let band = self.band;
        let upper = self.upper.clone();
        if let Some(decks) = self.s.decks.clone() {
            // a spiral: decks and ramps were fixed before routing
            for &z in &upper {
                self.deck.insert(z, decks.get(&z).cloned().unwrap_or_default());
                for d in self.doors.iter().filter(|d| d.level == z) {
                    self.pads.push(Pad { cells: door_pad(&self.s, d, band, 0), level: z, door: Some(d.clone()) });
                }
            }
            for p in &self.s.preset {
                self.ramps.push(RampIn { lo: p.lo, hi: p.hi, rect: p.rect, d: p.dir, top: rect_cells(p.top), foot: rect_cells(p.foot) });
            }
            return;
        }
        let (w, d) = (self.w, self.d);
        let long_walls = if d >= w { [1, 3] } else { [0, 2] };
        let ring_kind = self.s.kind == "tower" && self.tower == "ring";
        let ends = self.s.kind == "tower" && self.tower == "ends";
        let short = if d >= w { [0, 2] } else { [1, 3] }; // the walls at the ends of its long axis
        for (n, &z) in upper.iter().enumerate() {
            let mut need: BTreeSet<i32> = BTreeSet::new();
            let mut cells = Cells::new();
            for dr in self.doors.iter().filter(|dr| dr.level == z) {
                let pad = door_pad(&self.s, dr, band, 0);
                cells.extend(pad.iter().copied());
                self.pads.push(Pad { cells: pad, level: z, door: Some(dr.clone()) });
            }
            for p in self.pods.iter().filter(|p| p.floor == z) {
                if let Some(acc) = self.access_of(&p.key) {
                    cells.extend(acc.deck.iter().copied());
                }
            }
            let mut blocked: BTreeSet<i32> = BTreeSet::new();
            for &c in &self.shell[&z] {
                blocked.extend(self.slot(c));
            }
            for &c in &cells {
                need.extend(self.slot(c));
            }
            let own = self.top.is_none_or(|top| z <= top); // may this storey have deck nothing arrives on?
            let mut ring = ring_kind && w.min(d) >= 12 && own;
            let lay = if n == 0 && own { self.layout } else { None };
            if lay == Some("ring") && w.min(d) >= RING_MIN {
                ring = true;
            } else if lay == Some("galleries") && w.min(d) >= GALLERIES_MIN {
                for side in long_walls {
                    need.extend(self.wall(side).filter(|t| !blocked.contains(t)));
                }
            } else if lay == Some("mezzanine") && w.max(d) >= MEZZANINE_MIN {
                let side = short[self.rng.below(2) as usize];
                let dm = 20.min(8.max(2 * py::round(0.15 * w.max(d) as f64) as i32));
                let (x0, y0, x1, y1) = (self.x0, self.y0, self.x1, self.y1);
                let mz = rect_cells([(x0, y0, x1, y0 + dm), (x1 - dm, y0, x1, y1), (x0, y1 - dm, x1, y1), (x0, y0, x0 + dm, y1)][side]);
                for &c in &mz {
                    need.extend(self.slot(c));
                }
                cells.extend(mz);
            }
            if ends && own {
                // a deck across one end, the other end next storey up
                let end: BTreeSet<i32> = self.wall(short[n % 2]).filter(|t| !blocked.contains(t)).collect();
                if end.len() >= 4 {
                    need.extend(end);
                }
            }
            if need.is_empty() && !ring {
                match self.balcony {
                    Some(side) if z == upper[0] && own => need = self.wall(side).collect(),
                    _ => continue,
                }
            } else if !need.is_empty() && !ring && matches!(self.s.kind, "vast" | "basin") && self.rng.random() < 0.3 && w.min(d) >= 16 {
                // (not on a first upper storey laid out by hand: that is as the person chose)
                ring = own && !(self.fixed && n == 0);
            }
            let slots = self.arc(&need, &blocked, ring, self.s.high.contains(&z));
            let wanted = cells.clone(); // doors' pads and pods' gangways
            for t in slots {
                cells.extend(self.slot_cells(t));
            }
            cells.retain(|c| self.inside(*c) && !self.shell[&z].contains(c));
            if !cells.is_empty() && cells.is_subset(&self.roof[&z]) {
                // the deck would be a pod's roof: climb to that
                self.claim.insert(z);
            }
            let mut deck: Cells = cells.difference(&self.roof[&z]).copied().collect();
            // a scrap of catwalk between two pods that nothing arrives on is left out
            for comp in components(&deck) {
                if !meets(&comp, &wanted) && (comp.len() as i32) < 12 * band {
                    deck.retain(|c| !comp.contains(c));
                }
            }
            self.deck.insert(z, deck);
            if ring || lay == Some("galleries") {
                self.lay_bridges(z, if w.max(d) >= 44 { 2 } else { 1 });
            }
        }
    }

    /// Catwalks across the hall between the decks on its two long walls (`_bridges`).
    fn lay_bridges(&mut self, z: i32, count: usize) {
        let long_x = self.w >= self.d;
        let (lh, sh) = if long_x { (self.w, self.d) } else { (self.d, self.w) };
        if lh < 28 || sh < 16 || self.s.kind == "tower" {
            return;
        }
        let spots = if count == 1 {
            vec![self.rng.uniform(0.35, 0.65)]
        } else {
            let first = self.rng.uniform(0.25, 0.38);
            vec![first, self.rng.uniform(0.62, 0.75)]
        };
        let (band, wide) = (self.band, self.band);
        let (x0, y0, x1, y1) = (self.x0, self.y0, self.x1, self.y1);
        for t in spots {
            let u = 2 * py::round(lh as f64 * t / 2.0) as i32 - 2;
            let (strip, ends) = if long_x {
                (
                    rect_cells((x0 + u, y0, x0 + u + wide, y1)),
                    [rect_cells((x0 + u, y0, x0 + u + wide, y0 + band)), rect_cells((x0 + u, y1 - band, x0 + u + wide, y1))],
                )
            } else {
                (
                    rect_cells((x0, y0 + u, x1, y0 + u + wide)),
                    [rect_cells((x0, y0 + u, x0 + band, y0 + u + wide)), rect_cells((x1 - band, y0 + u, x1, y0 + u + wide))],
                )
            };
            if meets(&strip, &self.shell[&z]) || meets(&strip, &self.roof[&z]) {
                continue;
            }
            let deck = self.deck.get_mut(&z).expect("a storey of the room");
            if !ends.iter().all(|e| e.is_subset(deck)) {
                continue;
            }
            deck.extend(strip);
            self.bridges += 1;
        }
    }

    /// Cells with a floor on storey `z`.
    pub fn walk(&self, z: i32) -> Cells {
        if z == self.levels[0] {
            let mut out: Cells = self.room.iter().filter(|c| !self.pool.contains(c) && !self.hole.contains(c)).copied().collect();
            out.extend(self.bridge.iter().copied());
            return out;
        }
        let mut out = self.deck[&z].clone();
        let roof = &self.roof[&z];
        if !roof.is_empty() && (self.claim.contains(&z) || touching(roof, &out, 2)) {
            out.extend(roof.iter().copied());
        }
        out
    }

    // ------------------------------------------------------------ pool

    /// A basin's bridge, the short way across the water, and tanks standing in it. Err if the
    /// pool is too narrow for a bridge (the Python has no answer there: it stops with a
    /// TypeError).
    pub(super) fn lay_pool(&mut self) -> Result<(), String> {
        let Some(pool) = self.s.pool else { return Ok(()) };
        if self.pool.is_empty() {
            return Ok(());
        }
        let (px0, py0, px1, py1) = (2 * pool.0, 2 * pool.1, 2 * pool.2, 2 * pool.3);
        let wide = self.band;
        // one of lo, lo + 2, ... below hi, or None if there is none (no number is drawn then)
        fn pick(rng: &mut crate::pyrand::PyRandom, lo: i32, hi: i32) -> Option<i32> {
            let n = if hi > lo { (hi - lo + 1) / 2 } else { 0 };
            (n > 0).then(|| lo + 2 * rng.below(n as u64) as i32)
        }
        let narrow = || format!("{}: its pool is too narrow for a bridge", self.s.key);
        if px1 - px0 <= py1 - py0 {
            // the bridge crosses the short way
            let y = pick(&mut self.rng, py0 + 2, py1 - wide - 1).ok_or_else(narrow)?;
            self.bridge = rect_cells((px0, y, px1, y + wide));
        } else {
            let x = pick(&mut self.rng, px0 + 2, px1 - wide - 1).ok_or_else(narrow)?;
            self.bridge = rect_cells((x, py0, x + wide, py1));
        }
        let water: Cells = self.pool.difference(&self.bridge).copied().collect();
        for _ in 0..2 {
            // tanks standing in the water
            for _ in 0..30 {
                let (w, d) = *self.rng.choice(&[(2, 2), (4, 2), (2, 4)]);
                let x = pick(&mut self.rng, px0 + 2, px1 - w - 1);
                let y = pick(&mut self.rng, py0 + 2, py1 - d - 1);
                let (Some(x), Some(y)) = (x, y) else { continue };
                let t = rect_cells((x, y, x + w, y + d));
                let grown = rect_cells((x - 2, y - 2, x + w + 2, y + d + 2));
                if t.is_subset(&water) && !meets(&grown, &self.bridge) && !self.extra_solids.iter().any(|c| meets(&grown, &rect_cells((c.0, c.1, c.2, c.3)))) {
                    self.extra_solids.push((x, y, x + w, y + d, pool.4, self.s.floor + 118, "tank"));
                    break;
                }
            }
        }
        Ok(())
    }
}
