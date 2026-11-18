//! Not the Python's (rules of the Rust's own, 2026-11-18): the deck an open ramp rises over is
//! cut away under it, and a catwalk the cut leaves ending at the ramp is run on round it, or
//! taken back if it is a short spur, so the cut leaves no dead end. See AUTOLEVEL.md, "Rules of
//! the Rust's own".
//!
//! The plan always knew nobody could stand under a ramp (`stand` takes a ramp's cells out of its
//! own storey), but the Python built the deck on underneath: a catwalk that met a ramp's side ran
//! on into its underside, and one under a ramp's whole run left a wedge of deck below its high
//! end, both with no headroom (the ramp's side rails are up on the ramp, above the catwalk). All
//! of this is done once the ramps are placed, so where they go and every draw of the generator
//! before it are as they were.

use super::{components, Interior, RampIn, RAMP_H};
use crate::cells::{rect_cells, Cell, Cells};
use crate::space::{door_pad, DIRS};
use std::collections::{BTreeMap, VecDeque};

/// The furthest a cut catwalk is run round a ramp, in half-cell steps: twice a ramp's run.
const DETOUR: usize = 2 * RAMP_H as usize;
/// The longest spur taken back where no way round fits, half cells: twice a ramp's run.
const SPUR: i32 = 2 * RAMP_H;

/// One step of `d` from `c`, `k` times.
fn step(c: Cell, d: usize, k: i32) -> Cell {
    (c.0 + k * DIRS[d].0, c.1 + k * DIRS[d].1)
}

impl Interior {
    /// Cut the deck from under every open ramp that rises over it, and mend what each cut left.
    pub(super) fn clear_under_ramps(&mut self) {
        let ramps: Vec<RampIn> = self.ramps.iter().filter(|r| r.lo != self.levels[0]).cloned().collect();
        for r in &ramps {
            let deck = self.deck.get_mut(&r.lo).expect("a storey of the room");
            let cut: Cells = r.cells().into_iter().filter(|c| deck.remove(c)).collect();
            if !cut.is_empty() {
                self.mend(r, &cut);
            }
        }
    }

    /// Each catwalk the cut under `r` left ending at the ramp: run on round it, else taken back.
    fn mend(&mut self, r: &RampIn, cut: &Cells) {
        let z = r.lo;
        // the deck beside the cut (its foot landing aside), with the way from it into the cut
        let mut into: BTreeMap<Cell, usize> = BTreeMap::new();
        for &c in cut {
            for d in 0..4 {
                let n = step(c, d, -1);
                if self.deck[&z].contains(&n) && !r.foot.contains(&n) {
                    into.entry(n).or_insert(d);
                }
            }
        }
        let ends: Cells = into.keys().copied().collect();
        for tip in components(&ends) {
            let d = into[tip.first().expect("a piece has cells")];
            if tip.iter().any(|c| into[c] != d) || !self.dead_end(z, &tip, d) {
                continue; // a corner, a wider deck or one that runs on past the cut: its rail will do
            }
            match self.way_round(z, &tip, d) {
                Some(way) => self.deck.get_mut(&z).expect("a storey of the room").extend(way),
                None => self.take_back(z, &tip, d),
            }
        }
    }

    /// The straight run of catwalk that ends at `tip` (going `d` into the cut): its cells, back
    /// to where other deck meets its side or it reaches a cell of `stop` (no more than `limit`
    /// half cells), and whether it gets there (it may end, turn or run on past `limit` first).
    fn spur(&self, z: i32, tip: &Cells, d: usize, limit: i32, stop: &Cells) -> (Cells, bool) {
        let walk = self.walk(z);
        let across = (d + 1) % 4;
        let mut cells = Cells::new();
        for k in 0..limit {
            let section: Cells = tip.iter().map(|&c| step(c, d, -k)).collect();
            if section.iter().any(|c| stop.contains(c)) {
                return (cells, true);
            }
            if !section.iter().all(|c| walk.contains(c)) {
                return (cells, false);
            }
            if section.iter().flat_map(|&c| [step(c, across, 1), step(c, across, -1)]).any(|n| !section.contains(&n) && walk.contains(&n)) {
                return (cells, true);
            }
            cells.extend(section);
        }
        (cells, false)
    }

    /// Is `tip` (deck cells that each meet the cut going `d`) the end of a catwalk: a strip
    /// as wide as the catwalks, with nothing walkable either side of it?
    pub(super) fn dead_end(&self, z: i32, tip: &Cells, d: usize) -> bool {
        let walk = self.walk(z);
        let across = (d + 1) % 4;
        let flanks = tip.iter().flat_map(|&c| [step(c, across, 1), step(c, across, -1)]).filter(|n| !tip.contains(n));
        tip.len() as i32 == self.band && !flanks.into_iter().any(|n| walk.contains(&n))
    }

    /// Could a catwalk be laid on half cell `c` of storey `z`: inside the walls, nothing there yet,
    /// not under or over a ramp of this storey, not in a pod, its pier, its roof or a pit, and not
    /// over where a drop lands or a grate's light falls (`keep`)?
    fn free_for_deck(&self, z: i32, c: Cell) -> bool {
        let within = |(x0, y0, x1, y1): (i32, i32, i32, i32)| x0 <= c.0 && c.0 < x1 && y0 <= c.1 && c.1 < y1;
        self.inside(c)
            && !self.keep.iter().any(|&k| within(k))
            && !self.deck[&z].contains(&c)
            && !self.roof[&z].contains(&c)
            && !self.shell[&z].contains(&c)
            && !self.pool.contains(&c)
            && !self.hole.contains(&c)
            && !self.ramps.iter().any(|q| (q.lo == z || q.hi == z) && q.cells().contains(&c))
            && !self.piers.iter().any(|&(x0, y0, x1, y1, a, b)| a <= z && z < b && within((x0, y0, x1, y1)))
    }

    /// The fewest new cells of deck, a catwalk wide all the way, that join the end of the catwalk
    /// at `tip` to other deck (the ramp's foot landing, the deck on its far side, a catwalk beside
    /// it), going round the ramp. None if no way fits within `DETOUR` steps, or if the only one
    /// needs no new deck (the catwalk is a stub on other deck: `take_back` takes it).
    fn way_round(&self, z: i32, tip: &Cells, d: usize) -> Option<Cells> {
        let band = self.band;
        // anything walkable but the catwalk itself, back to where it branches
        let (own, _) = self.spur(z, tip, d, 4 * (self.w + self.d), &Cells::new());
        let goal: Cells = self.walk(z).difference(&own).copied().collect();
        let goal = &goal;
        // the catwalk's last square: the tip and the cells behind it, away from the cut
        let end: Cells = tip.iter().flat_map(|&c| (0..band).map(move |k| step(c, d, -k))).filter(|c| self.deck[&z].contains(c)).collect();
        let square = |p: Cell| rect_cells((p.0, p.1, p.0 + band, p.1 + band));
        let open = |p: Cell| square(p).iter().all(|&c| end.contains(&c) || goal.contains(&c) || self.free_for_deck(z, c));
        let (x0, y0, _, _) = super::bounds(&end);
        let start = (x0, y0);
        if !square(start).iter().all(|c| end.contains(c)) {
            return None;
        }
        let mut came: BTreeMap<Cell, Option<Cell>> = BTreeMap::from([(start, None)]);
        let mut todo = VecDeque::from([(start, 0)]);
        while let Some((p, n)) = todo.pop_front() {
            if p != start && square(p).iter().any(|c| goal.contains(c)) {
                let mut way = Cells::new();
                let mut at = Some(p);
                while let Some(q) = at {
                    way.extend(square(q).into_iter().filter(|c| !self.deck[&z].contains(c)));
                    at = came[&q];
                }
                return (!way.is_empty()).then_some(way);
            }
            if n == DETOUR {
                continue;
            }
            for e in 0..4 {
                let q = step(p, e, 1);
                if !came.contains_key(&q) && open(q) {
                    came.insert(q, Some(p));
                    todo.push_back((q, n + 1));
                }
            }
        }
        None
    }

    /// Take back the straight spur that ends at `tip` (going `d` into the cut), as far as where it
    /// joins other deck or something needs it (a door's or a pod's way in, a ramp's landing, a
    /// pod's roof), if that is no more than `SPUR`. Otherwise leave it: its end gets a rail.
    fn take_back(&mut self, z: i32, tip: &Cells, d: usize) {
        let mut needed: Cells = self.targets().into_iter().filter(|(_, level)| *level == z).flat_map(|(cells, _)| cells).collect();
        for door in self.doors.iter().filter(|door| door.level == z) {
            needed.extend(door_pad(&self.s, door, self.band, 0));
        }
        for q in &self.ramps {
            if q.lo == z {
                needed.extend(q.foot.iter().copied());
            }
            if q.hi == z {
                needed.extend(q.top.iter().copied());
            }
        }
        needed.extend(self.roof[&z].iter().copied());
        let (spur, joins) = self.spur(z, tip, d, SPUR, &needed);
        if !joins || spur.is_empty() {
            return; // it ends, turns or runs on too far: not a short spur off other deck
        }
        let deck = self.deck.get_mut(&z).expect("a storey of the room");
        deck.retain(|c| !spur.contains(c));
        if !self.connected() {
            self.deck.get_mut(&z).expect("a storey of the room").extend(spur);
        }
    }
}
