//! Open ramps between a room's storeys. From `furnish.py` (`place_ramps`, `_solve`, `_landing`,
//! `_gangway`, `_static`, `_ramp_options`).
//!
//! Every separate piece of floor upstairs gets a ramp down, top storey first (a ramp's foot may
//! add a landing to the storey below, which then needs its own). A choice that leaves no place
//! for a ramp further down is taken back and the next best tried.

use super::{bounds, common, components, grow, meets, overlap, runs, touching, Interior, RampIn, LAND, RAMP_H};
use crate::cells::{rect_cells, Cells, Rect};
use crate::py;
use crate::space::DIRS;
use crate::storeys::index;
use std::collections::BTreeSet;

/// A place a ramp could go: (its score, the lower the better; the way it climbs; its cells; the
/// landing past its top; the landing past its foot).
type Found = (f64, usize, Rect, Cells, Cells);

impl Interior {
    /// Ramps for every piece of floor upstairs, trying at most `budget` places. Err if no set of
    /// ramps joins the storeys.
    pub(super) fn place_ramps(&mut self, budget: i32) -> Result<(), String> {
        let mut left = budget;
        if !self.solve(self.levels.len() - 1, &mut left) {
            return Err(format!("{}: no set of ramps joins its storeys", self.s.key));
        }
        Ok(())
    }

    fn solve(&mut self, hi_i: usize, left: &mut i32) -> bool {
        if hi_i == 0 {
            // a ramp laid over a catwalk can cut it in two. (The rescue joins what is left apart
            // afterwards, `stitch`, but a ramp still may not stand where a door is stepped through.)
            return if self.rescue { self.door_nodes().iter().all(Option::is_some) } else { self.connected() };
        }
        let (hi, lo) = (self.levels[hi_i], self.levels[hi_i - 1]);
        if self.band_of(hi) != self.band_of(lo) {
            return self.solve(hi_i - 1, left); // an atrium's upper band is not ramped down
        }
        let mut served = Cells::new();
        for r in self.ramps.iter().filter(|r| r.hi == hi) {
            served.extend(r.top.iter().copied());
        }
        let claimed = self.claim.contains(&hi);
        let todo: Vec<Cells> = components(&self.walk(hi))
            .into_iter()
            .filter(|c| !meets(c, &served) && (meets(c, &self.deck[&hi]) || claimed) && !self.stranded.iter().any(|s| meets(c, s)))
            .collect();
        // (pieces come in no order: ties are broken by place)
        let Some(comp) = py::first_max(todo, |c| (c.len(), *c.first().expect("a piece has cells"))) else {
            return self.solve(hi_i - 1, left);
        };
        let mut options = self.ramp_options(&comp, lo, hi, false, 6);
        if options.is_empty() {
            options = self.ramp_options(&comp, lo, hi, true, 6);
        }
        if options.is_empty() {
            // nowhere for a ramp to arrive: run a gangway on to the nearest wall
            let added = if !self.extended.contains(&comp) && self.extended.len() < 8 { self.gangway(&comp, hi, 12) } else { Cells::new() };
            if added.is_empty() {
                // (the rescue lets it be: a ladder or a bridge joins it once the ramps are down)
                return self.rescue && self.strand(comp, hi_i, left);
            }
            self.extended.insert(comp.clone());
            self.deck.get_mut(&hi).expect("a storey of the room").extend(added.iter().copied());
            if self.solve(hi_i, left) {
                return true;
            }
            self.deck.get_mut(&hi).expect("a storey of the room").retain(|c| !added.contains(c));
            self.extended.remove(&comp);
            return false;
        }
        for r in options {
            if *left <= 0 {
                // (out of tries: the rescue lets it be, as a piece no ramp can reach)
                return self.rescue && self.strand(comp.clone(), hi_i, left);
            }
            *left -= 1;
            let saved = (self.deck.clone(), self.ramps.len());
            let top: Cells = r.top.difference(&self.roof[&hi]).copied().collect();
            self.deck.get_mut(&hi).expect("a storey of the room").extend(top);
            if lo != self.levels[0] {
                let foot: Cells = r.foot.difference(&self.roof[&lo]).copied().collect();
                self.deck.get_mut(&lo).expect("a storey of the room").extend(foot);
            }
            self.ramps.push(r);
            if self.solve(hi_i, left) {
                return true;
            }
            self.deck = saved.0;
            self.ramps.truncate(saved.1);
        }
        false
    }

    /// The rescue's way past a piece of deck the ramps can't join (`stitch` joins it after): the
    /// rest of the storeys solved without it, or false (and it is a piece again).
    fn strand(&mut self, comp: Cells, hi_i: usize, left: &mut i32) -> bool {
        self.stranded.push(comp);
        if self.solve(hi_i, left) {
            return true;
        }
        self.stranded.pop();
        false
    }

    /// Where the landing at one end of a ramp may go: `LAND` square (or the ramp's width, if
    /// wider), in line with it and reaching past one of its sides or the other.
    fn landing_spans(&self, rect: Rect, d: usize, top: bool) -> Vec<Rect> {
        let (x0, y0, x1, y1) = rect;
        let (dx, dy) = DIRS[d];
        let size = LAND.max(self.band);
        let across = if dx != 0 { y1 - y0 } else { x1 - x0 };
        let mut offsets = vec![0, size - across];
        offsets.sort();
        offsets.dedup();
        if dx != 0 {
            let a = if (dx > 0) == top { x1 } else { x0 - size };
            offsets.into_iter().map(|e| (a, y0 - e, a + size, y0 - e + size)).collect()
        } else {
            let a = if (dy > 0) == top { y1 } else { y0 - size };
            offsets.into_iter().map(|e| (x0 - e, a, x0 - e + size, a + size)).collect()
        }
    }

    /// The landing at one end of a ramp: of the places it may go, the one that lies more on the
    /// floor already there (`have`). None if neither fits: outside the room, or on `bad` cells
    /// (for a top landing, only its cells that aren't floor yet count).
    fn landing(&self, rect: Rect, d: usize, top: bool, have: &Cells, bad: &Cells, new_only: bool) -> Option<Cells> {
        let mut best: Option<(usize, Cells)> = None;
        for span in self.landing_spans(rect, d, top) {
            if !self.holds(span) {
                continue;
            }
            let cells = rect_cells(span);
            let on_bad = if new_only { cells.iter().any(|c| !have.contains(c) && bad.contains(c)) } else { meets(&cells, bad) };
            if on_bad {
                continue;
            }
            let on = common(&cells, have);
            if best.as_ref().is_none_or(|(most, _)| on > *most) {
                best = Some((on, cells));
            }
        }
        best.map(|(_, cells)| cells)
    }

    /// More deck for a piece no ramp can arrive at: the shortest straight run on to a wall it
    /// doesn't reach (up to `reach` h), or else two cells further along the wall it is on, at
    /// each end that is free. Empty if neither. A one-cell pad boxed in between a pod and a door
    /// below has no room for a ramp until the catwalk runs on.
    fn gangway(&self, comp: &Cells, hi: i32, reach: i32) -> Cells {
        let (x0, y0, x1, y1) = bounds(comp);
        let w = self.band;
        let mut taken: Cells = self.shell[&hi].union(&self.roof[&hi]).copied().collect();
        for r in self.ramps.iter().filter(|r| r.lo == hi || r.hi == hi) {
            taken.extend(r.cells());
        }
        let deck = &self.deck[&hi];
        let mut best = Cells::new();
        let strips = [
            (x0, y1, x0 + w, self.y1),
            (x1 - w, y1, x1, self.y1),
            (x0, self.y0, x0 + w, y0),
            (x1 - w, self.y0, x1, y0),
            (x1, y0, self.x1, y0 + w),
            (x1, y1 - w, self.x1, y1),
            (self.x0, y0, x0, y0 + w),
            (self.x0, y1 - w, x0, y1),
        ];
        for strip in strips.map(rect_cells) {
            let new: Cells = strip.difference(deck).copied().collect();
            if !new.is_empty()
                && strip.len() as i32 <= reach * w
                && strip.iter().all(|c| self.inside(*c))
                && !meets(&strip, &taken)
                && (best.is_empty() || new.len() < best.len())
            {
                best = new;
            }
        }
        if !best.is_empty() {
            return best;
        }
        let p = 2 * (self.w + self.d);
        let (mut slots, mut blocked) = (BTreeSet::new(), BTreeSet::new());
        for &c in comp {
            slots.extend(self.slot(c));
        }
        for &c in &self.shell[&hi] {
            blocked.extend(self.slot(c));
        }
        for run in runs(slots, Some(p)) {
            for (t, step) in [(run[run.len() - 1], 1), (run[0], -1)] {
                for k in 1..5 {
                    let cells = self.slot_cells(t + step * k);
                    if blocked.contains(&(t + step * k).rem_euclid(p)) || meets(&cells, &taken) {
                        break;
                    }
                    best.extend(cells);
                }
            }
        }
        best.retain(|c| !deck.contains(c));
        best
    }

    /// Cells a ramp from `lo` to `hi` can't cross whatever else is built: pods, the pool, piers,
    /// and the floor in front of doors (`_static`: `static` is a word Rust keeps).
    fn fixed(&self, lo: i32, hi: i32, relax: bool) -> Cells {
        let floor = lo == self.levels[0];
        let mut out: Cells = self.shell[&hi].clone();
        out.extend(self.shell[&lo].iter().copied());
        out.extend(self.roof[&hi].iter().copied());
        // on the floor a ramp keeps three cells clear of a door; upstairs, just off the door's own deck
        out.extend(self.apron(lo, if floor && !relax { 6 } else { self.band }));
        if floor {
            out.extend(self.bridge.iter().copied());
            if !self.hole.is_empty() {
                out.extend(rect_cells(grow(bounds(&self.hole), 2)));
            }
            if !self.pool.is_empty() {
                // a cell of walkway between a ramp and the water, so stepping off its side lands on floor
                out.extend(rect_cells(grow(bounds(&self.pool), 2)));
            }
            for q in &self.piers {
                out.extend(rect_cells((q.0, q.1, q.2, q.3)));
            }
        } else if self.s.towers() {
            // a towering hall's high pods stand on piers through its ramped storeys: no ramp
            // between two of them goes through one (in the Python's levels a pier stops under
            // the first upper storeys, and only a ramp from the floor is kept clear of it)
            for q in self.piers.iter().filter(|q| q.4 < hi && lo < q.5) {
                out.extend(rect_cells((q.0, q.1, q.2, q.3)));
            }
        }
        out
    }

    /// The best places for an open ramp from the piece of floor `comp` on `hi` down to `lo`:
    /// every position and direction is tried. Ramps between the same storeys never overlap in
    /// plan. Ramps on the storeys above or below may only sit exactly over one another, climbing
    /// the same way (a stack, as in a narrow shaft), so a place that the ramps below could also
    /// use scores better. Returns the best in each direction, then the next best that are
    /// somewhere else (up to `keep`), best first.
    fn ramp_options(&mut self, comp: &Cells, lo: i32, hi: i32, relax: bool, keep: usize) -> Vec<RampIn> {
        let floor = lo == self.levels[0];
        let wide = self.band;
        let (deck_hi, deck_lo) = (self.deck[&hi].clone(), self.deck[&lo].clone());
        let (walk_hi, walk_lo) = (self.walk(hi), self.walk(lo));
        let li = index(lo) as i32;
        let mut same_cells = Cells::new();
        let mut near_cells = Cells::new();
        let mut below_cells = Cells::new();
        let mut near: Vec<(Rect, usize, Cells)> = Vec::new();
        for r in &self.ramps {
            if r.lo == lo {
                same_cells.extend(r.cells());
            } else if (index(r.lo) as i32 - li).abs() == 1 {
                near_cells.extend(r.cells());
                near.push((r.rect, r.d, r.cells()));
            }
            if r.hi == lo {
                below_cells.extend(r.cells());
            }
        }
        let fixed = self.fixed(lo, hi, relax);
        let mut no_ramp = deck_hi.clone();
        no_ramp.extend(same_cells.iter().copied());
        no_ramp.extend(fixed.iter().copied());
        let mut no_top: Cells = self.shell[&hi].union(&same_cells).copied().collect();
        let mut no_foot: Cells = self.shell[&lo].union(&same_cells).copied().collect();
        no_foot.extend(below_cells.iter().copied());
        // (a shaped room's cut is solid)
        no_ramp.extend(self.cut.iter().copied());
        no_top.extend(self.cut.iter().copied());
        no_foot.extend(self.cut.iter().copied());
        // (nor in a sunken floor, or by it)
        let sunk = self.sunk_clear();
        no_ramp.extend(sunk.iter().copied());
        no_foot.extend(sunk.iter().copied());
        if floor {
            no_foot.extend(self.pool.iter().copied());
            no_foot.extend(self.bridge.iter().copied());
            let apron = self.apron(lo, 6);
            no_foot.extend(fixed.difference(&apron).copied());
        }
        let hi_i = self.levels.iter().position(|&z| z == hi).expect("a storey of the room");
        let lower: Vec<Cells> = (1..hi_i)
            .map(|i| {
                let mut q = self.fixed(self.levels[i - 1], self.levels[i], false);
                q.extend(self.deck[&self.levels[i]].iter().copied());
                q
            })
            .collect();
        // (a landing off the piece's edge by more than a cell can't touch it: not worth the sets)
        let reach = grow(bounds(comp), 1);
        let mut found: Vec<Found> = Vec::new();
        for (d, &(dx, _)) in DIRS.iter().enumerate() {
            let (lx, ly) = if dx != 0 { (RAMP_H, wide) } else { (wide, RAMP_H) };
            for x0 in self.x0..=self.x1 - lx {
                for y0 in self.y0..=self.y1 - ly {
                    let rect = (x0, y0, x0 + lx, y0 + ly);
                    if !self.landing_spans(rect, d, true).into_iter().any(|span| overlap(span, reach)) {
                        continue;
                    }
                    let r = rect_cells(rect);
                    if meets(&r, &no_ramp) {
                        continue;
                    }
                    let mut stacked = false;
                    if meets(&r, &near_cells) {
                        if !near.iter().filter(|q| meets(&q.2, &r)).all(|q| q.0 == rect && q.1 == d) {
                            continue;
                        }
                        stacked = true;
                    }
                    let Some(t) = self.landing(rect, d, true, &walk_hi, &no_top, true) else { continue };
                    let Some(f) = self.landing(rect, d, false, &walk_lo, &no_foot, false) else { continue };
                    let in_comp = common(&t, comp);
                    if in_comp == 0 && !touching(&t, comp, 2) {
                        continue;
                    }
                    let mut score = (t.len() - in_comp) as f64 * 0.25 - if stacked { 1.0 } else { 0.0 };
                    if !floor {
                        let new = f.iter().filter(|c| !walk_lo.contains(c)).count();
                        if new > 0 {
                            if !f.iter().any(|&c| !self.slot(c).is_empty()) {
                                continue; // a landing hangs on a wall
                            }
                            if !(walk_lo.is_empty() || meets(&f, &walk_lo) || touching(&f, &walk_lo, 2)) {
                                score += 2.0; // a landing on its own: it will need a ramp too
                            }
                        }
                        score += new as f64 * 0.25 + common(&r, &deck_lo) as f64 * 0.4;
                    }
                    score += 3.0 * lower.iter().filter(|q| meets(&r, q)).count() as f64; // nothing could stack under it
                                                                                         // along a wall, or along the deck it climbs to, beats standing free
                    let sides: [Cells; 2] = if dx != 0 {
                        [(x0..x0 + lx).map(|x| (x, y0 - 1)).collect(), (x0..x0 + lx).map(|x| (x, y0 + ly)).collect()]
                    } else {
                        [(y0..y0 + ly).map(|y| (x0 - 1, y)).collect(), (y0..y0 + ly).map(|y| (x0 + lx, y)).collect()]
                    };
                    let wall = sides.iter().any(|sd| !sd.iter().all(|c| self.inside(*c)));
                    let along = sides.iter().map(|sd| common(sd, &deck_hi)).max().unwrap_or(0);
                    score += if wall {
                        -3.0
                    } else if along as i32 >= RAMP_H / 2 {
                        -2.0
                    } else {
                        2.0
                    };
                    score += 0.6 * ((x0 - self.x0).rem_euclid(2) + (y0 - self.y0).rem_euclid(2)) as f64;
                    score += self.rng.random() * 1.5;
                    found.push((score, d, rect, t, f));
                }
            }
        }
        found.sort_by(|a, b| py::cmp_f64(a.0, b.0).then_with(|| (a.1, a.2).cmp(&(b.1, b.2))));
        let mut out: Vec<usize> = Vec::new();
        let mut dirs = BTreeSet::new();
        for (i, f) in found.iter().enumerate() {
            // the best in each direction
            if dirs.insert(f.1) {
                out.push(i);
            }
        }
        for (i, f) in found.iter().enumerate() {
            // then others, a cell or more away from those
            if out.len() >= keep {
                break;
            }
            if out.iter().all(|&o| (f.2 .0 - found[o].2 .0).abs() + (f.2 .1 - found[o].2 .1).abs() >= 2) {
                out.push(i);
            }
        }
        out.sort(); // (as sorting them by score again: no two have the same place and direction)
        let mut found: Vec<Option<Found>> = found.into_iter().map(Some).collect();
        out.into_iter().map(|i| found[i].take().expect("each once")).map(|(_, d, rect, top, foot)| RampIn { lo, hi, rect, d, top, foot }).collect()
    }
}
