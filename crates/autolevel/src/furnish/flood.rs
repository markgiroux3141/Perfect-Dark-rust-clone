//! A flooded room: its floor is water (`Space::flood`, `level::floods`), crossed by railed
//! walkways at floor level that join its doors. New in the Rust (docs/AUTOLEVEL.md, "Flooded
//! rooms"): the Python has none.
//!
//! The room is furnished dry first (its decks, ramps and landings are as they would be), then
//! flooded (`lay_flood`, before the platforms):
//!
//! - **walkways**: a catwalk wide, from every floor target (a door's pad, a pod's way in, the foot
//!   of a ramp from the floor) to the walkway already laid, each the shortest way there that turns
//!   least (`walkway`), so they run straight and branch;
//! - **rafts**: platforms in the water at floor level, each off a walkway along one side and
//!   jutting into the water where it is widest (`lay_rafts`);
//! - **angled catwalks** over the water (`angled`): an island with the chance `ISLE`, and the
//!   shortcut that saves the most walking, both drawn from the room's own stream
//!   ("{seed}/{key}/pits/flood");
//! - **tanks** standing in the water (`lay_tanks`), as a basin's do, after the angled catwalks.
//!
//! The water is the rest of the floor (`pool`: the room less the walkways, rafts, islands and the
//! pods' blocks), `level::FLOOD_DEPTH` under it: the rails run round every walkway's edge over it,
//! and nobody walks on it. The walkways are the basin's bridge (`bridge`): floor slabs over the
//! water's pit (`plans`).

use super::{grow, Interior};
use crate::cells::{rect_cells, Cell, Cells, Rect};
use crate::pyrand::PyRandom;
use crate::space::DIRS;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// What a turn costs a walkway, in half cells of its length: the higher, the straighter.
const TURN: u32 = 6;
/// The chance a flooded room has an island over its water.
const ISLE: f64 = 0.5;
/// One raft for this many cells of water, and no more than
const RAFT_PER: f64 = 150.0;
const RAFTS: usize = 3;
/// Raft sizes, h, the biggest first.
const RAFT_SIZES: [(i32, i32); 6] = [(12, 8), (8, 12), (10, 8), (8, 10), (8, 8), (6, 6)];
/// A raft's least edge on its walkway, h; how far round it its water is measured, h.
const CONTACT: i32 = 4;
const AROUND: i32 = 6;
/// One tank for this many cells of water, and no more than
const TANK_PER: f64 = 60.0;
const TANKS: usize = 6;
/// A tank stands this high over the floor, units (a basin's).
const TANK_TOP: i32 = 118;

impl Interior {
    /// The floor of storey `z` that angled catwalks join: its deck, or on a flooded room's floor
    /// its walkways.
    pub(crate) fn decked(&self, z: i32) -> &Cells {
        if z == self.levels[0] {
            &self.bridge
        } else {
            &self.deck[&z]
        }
    }

    pub(super) fn decked_mut(&mut self, z: i32) -> &mut Cells {
        if z == self.levels[0] {
            &mut self.bridge
        } else {
            self.deck.get_mut(&z).expect("a storey of the room")
        }
    }

    /// A flooded room's water: the floor less its walkways and its pods' blocks.
    pub(super) fn reflood(&mut self) {
        if self.flood.is_none() {
            return;
        }
        let z0 = self.levels[0];
        self.pool = self.room.iter().filter(|c| !self.bridge.contains(c) && !self.shell[&z0].contains(c)).copied().collect();
    }

    /// What a walkway on the floor may not run through: the pods' blocks, the ramps from the floor,
    /// the piers.
    fn wet_blocked(&self) -> Cells {
        let z0 = self.levels[0];
        let mut out = self.shell[&z0].clone();
        for r in self.ramps.iter().filter(|r| r.lo == z0) {
            out.extend(r.cells());
        }
        for q in self.piers.iter().filter(|q| q.4 <= z0 && z0 < q.5) {
            out.extend(rect_cells((q.0, q.1, q.2, q.3)));
        }
        out
    }

    /// Walkways, rafts, angled catwalks and the water (see the module). Err if a floor target can't
    /// be joined to the others.
    pub(super) fn lay_flood(&mut self) -> Result<(), String> {
        if self.flood.is_none() {
            return Ok(());
        }
        let z0 = self.levels[0];
        let bad = self.wet_blocked();
        let mut targets: Vec<Cells> = self.targets().into_iter().filter(|(c, z)| *z == z0 && !c.is_empty()).map(|(c, _)| c).collect();
        for r in self.ramps.iter().filter(|r| r.lo == z0) {
            targets.push(r.foot.clone());
        }
        let mut walk = Cells::new();
        if !targets.is_empty() {
            walk.extend(targets.remove(0));
        }
        while !targets.is_empty() {
            let (path, k) = self.walkway(&walk, &targets, &bad).ok_or_else(|| format!("{}: a door can't be walked to over the water", self.s.key))?;
            walk.extend(path);
            walk.extend(targets.remove(k));
        }
        // (never under a ramp from the floor: another ramp's landing may reach under one, where
        // nobody stands, and a walkway's slab there would have no headroom)
        self.bridge = walk.difference(&bad).copied().collect();
        self.reflood();
        self.lay_rafts(&bad);
        let mut rng = PyRandom::named(&format!("{}/flood", self.stream));
        if rng.random() < ISLE {
            self.lay_island(&[z0]);
        }
        self.lay_shortcut(z0);
        Ok(())
    }

    /// The cells of the shortest walkway (a catwalk wide, turning least) from `from` to one of
    /// `to`, and which. None if none is reached.
    fn walkway(&self, from: &Cells, to: &[Cells], bad: &Cells) -> Option<(Cells, usize)> {
        let b = self.band;
        let square = |p: Cell| rect_cells((p.0, p.1, p.0 + b, p.1 + b));
        let fits = |p: Cell| square(p).iter().all(|c| self.inside(*c) && !bad.contains(c));
        let spots: Vec<Cell> = (self.y0..=self.y1 - b).flat_map(|y| (self.x0..=self.x1 - b).map(move |x| (x, y))).collect();
        let mut goal: HashMap<Cell, usize> = HashMap::new();
        for (k, t) in to.iter().enumerate() {
            for &p in spots.iter().filter(|p| square(**p).is_subset(t)) {
                goal.entry(p).or_insert(k);
            }
        }
        // (cost, spot, way it came: 4 for none), cheapest first
        let mut todo: BinaryHeap<Reverse<(u32, Cell, usize)>> = BinaryHeap::new();
        let mut best: HashMap<(Cell, usize), u32> = HashMap::new();
        let mut came: HashMap<(Cell, usize), (Cell, usize)> = HashMap::new();
        for &p in spots.iter().filter(|p| square(**p).is_subset(from)) {
            best.insert((p, 4), 0);
            todo.push(Reverse((0, p, 4)));
        }
        while let Some(Reverse((cost, p, d))) = todo.pop() {
            if best.get(&(p, d)).is_some_and(|&c| c < cost) {
                continue;
            }
            if let Some(&k) = goal.get(&p) {
                let mut cells = square(p);
                let mut at = (p, d);
                while let Some(&back) = came.get(&at) {
                    cells.extend(square(back.0));
                    at = back;
                }
                return Some((cells, k));
            }
            for (e, &(dx, dy)) in DIRS.iter().enumerate() {
                let q = (p.0 + dx, p.1 + dy);
                if !fits(q) {
                    continue;
                }
                let c = cost + 1 + if d != 4 && d != e { TURN } else { 0 };
                if best.get(&(q, e)).is_none_or(|&o| c < o) {
                    best.insert((q, e), c);
                    came.insert((q, e), (p, d));
                    todo.push(Reverse((c, q, e)));
                }
            }
        }
        None
    }

    /// Rafts: platforms in the water off the walkways, where it is widest (see the module).
    fn lay_rafts(&mut self, bad: &Cells) {
        let water: Cells = self.pool.difference(bad).copied().collect();
        let want = ((water.len() as f64 / 4.0 / RAFT_PER).round() as usize).min(RAFTS);
        for _ in 0..want {
            let mut best: Option<(f64, Rect)> = None;
            for &(a, b) in &RAFT_SIZES {
                for y in (self.y0..=self.y1 - b).filter(|y| y.rem_euclid(2) == 0) {
                    for x in (self.x0..=self.x1 - a).filter(|x| x.rem_euclid(2) == 0) {
                        let r = (x, y, x + a, y + b);
                        let cells = rect_cells(r);
                        if !cells.is_subset(&water) || rect_cells(grow(r, 2)).iter().any(|c| bad.contains(c)) {
                            continue;
                        }
                        if self.rafts.iter().any(|&o| rect_cells(grow(o, 4)).iter().any(|c| cells.contains(c))) {
                            continue;
                        }
                        // on a walkway along one side, water a cell wide round the others (it juts out)
                        let sides = [(x - 1, y, x, y + b), (x + a, y, x + a + 1, y + b), (x, y - 1, x + a, y), (x, y + b, x + a, y + b + 1)];
                        let on = |s: Rect| rect_cells(s).iter().filter(|c| self.bridge.contains(c)).count();
                        let Some(k) = (0..4).max_by_key(|&k| (on(sides[k]), std::cmp::Reverse(k))) else { continue };
                        if (on(sides[k]) as i32) < CONTACT {
                            continue;
                        }
                        let beyond = [
                            (x - 2, y - 2, x, y + b + 2),
                            (x + a, y - 2, x + a + 2, y + b + 2),
                            (x - 2, y - 2, x + a + 2, y),
                            (x - 2, y + b, x + a + 2, y + b + 2),
                        ];
                        let side = rect_cells(beyond[k]);
                        if rect_cells(grow(r, 2)).iter().any(|c| self.bridge.contains(c) && !side.contains(c)) {
                            continue;
                        }
                        let round: Cells = rect_cells(grow(r, AROUND)).into_iter().filter(|c| self.inside(*c)).collect();
                        let wet = round.iter().filter(|c| water.contains(c)).count() as f64 / round.len().max(1) as f64;
                        let score = (a * b) as f64 * wet;
                        if best.is_none_or(|(s, _)| score > s) {
                            best = Some((score, r));
                        }
                    }
                }
            }
            let Some((_, r)) = best else { break };
            self.bridge.extend(rect_cells(r));
            self.rafts.push(r);
            self.reflood();
        }
    }

    /// Tanks standing in the water, clear of the walkways and the angled catwalks (see the module).
    pub(super) fn lay_tanks(&mut self) {
        let Some(water) = self.flood else { return };
        let z0 = self.levels[0];
        let mut wet: Cells = self.pool.difference(&self.wet_blocked()).copied().collect();
        let want = ((wet.len() as f64 / 4.0 / TANK_PER).round() as usize).min(TANKS);
        let mut keep_off: Cells = self.bridge.clone();
        keep_off.extend(self.spanned(z0));
        let mut rng = PyRandom::named(&format!("{}/flood/tanks", self.stream));
        for _ in 0..want {
            for _ in 0..30 {
                let (w, d) = *rng.choice(&[(2, 2), (4, 2), (2, 4)]);
                let x = self.x0 + rng.below((self.w - w).max(1) as u64) as i32;
                let y = self.y0 + rng.below((self.d - d).max(1) as u64) as i32;
                let t = rect_cells((x, y, x + w, y + d));
                if t.is_subset(&wet) && rect_cells((x - 2, y - 2, x + w + 2, y + d + 2)).is_disjoint(&keep_off) {
                    self.extra_solids.push((x, y, x + w, y + d, water, z0 + TANK_TOP, "tank"));
                    wet.retain(|c| !rect_cells((x - 2, y - 2, x + w + 2, y + d + 2)).contains(c));
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{generate, Options};
    use crate::place::Scale;

    #[test]
    fn a_flooded_floor_is_water_under_railed_walkways_that_join_its_doors() {
        let (mut levels, mut floods, mut rafts, mut tanks) = (0, 0, 0, 0);
        for (scale, seeds) in [(Scale::Grand, 1..=6), (Scale::Towering, 1..=6)] {
            for seed in seeds {
                let mut o = Options { angled: 1.0, ..Options::default() };
                o.siting.scale = scale;
                let dry = generate(seed, &o);
                o.flooded = 1.0;
                let lv = generate(seed, &o);
                assert_eq!(dry.is_ok(), lv.is_ok(), "seed {seed}: floods change whether a level is made");
                let Ok(lv) = lv else { continue };
                levels += 1;
                for (key, it) in &lv.interiors {
                    let Some(water) = it.flood else { continue };
                    floods += 1;
                    let at = format!("seed {seed}: {key}");
                    let z0 = it.levels[0];
                    assert_eq!(lv.space(key).flood, Some(water), "{at}: the room and its inside differ");
                    assert_eq!(water, lv.space(key).floor - crate::level::FLOOD_DEPTH);
                    // the floor is walkway or water (or a pod's block), never both
                    assert!(it.pool.is_disjoint(&it.bridge), "{at}: water under a walkway");
                    let all: Cells = it.pool.union(&it.bridge).chain(it.shell[&z0].iter()).copied().collect();
                    assert_eq!(all, it.room, "{at}: floor neither walkway nor water");
                    assert!(it.stand(z0).is_disjoint(&it.pool), "{at}: water stood on");
                    // every door and ramp foot on the floor is on the walkways, and they all join
                    for (pad, z) in it.targets().into_iter().filter(|t| t.1 == z0) {
                        assert!(pad.is_subset(&it.bridge), "{at}: a door's pad at {z} off the walkways");
                    }
                    let up: Vec<&super::super::RampIn> = it.ramps.iter().filter(|r| r.lo == z0).collect();
                    assert!(up.iter().all(|r| r.foot.difference(&it.wet_blocked()).all(|c| it.bridge.contains(c))), "{at}: a ramp's foot in the water");
                    assert!(up.iter().all(|r| r.cells().is_disjoint(&it.bridge)), "{at}: a walkway under a ramp");
                    // a ramp's every open side over the water is railed from its foot (even
                    // alongside the deck it climbs to: the water is under that deck)
                    let sloped = it.ramp_rails();
                    for r in &up {
                        let (x0, y0, x1, y1) = r.rect;
                        let along_x = DIRS[r.d].0 != 0;
                        for sg in [-1, 1] {
                            let cs: Cells = if along_x {
                                (x0..x1).map(|x| (x, if sg < 0 { y0 - 1 } else { y1 })).collect()
                            } else {
                                (y0..y1).map(|y| (if sg < 0 { x0 - 1 } else { x1 }, y)).collect()
                            };
                            if !cs.iter().all(|c| it.inside(*c)) || cs.is_disjoint(&it.pool) {
                                continue;
                            }
                            let line = crate::plan::cells(
                                (if along_x {
                                    if sg < 0 {
                                        y0
                                    } else {
                                        y1
                                    }
                                } else if sg < 0 {
                                    x0
                                } else {
                                    x1
                                }) as f64
                                    / 2.0,
                            );
                            let railed =
                                sloped.iter().any(|(p, _, f)| p.2 == r.lo && if along_x { f.1 == sg && p.1 == line } else { f.0 == sg && p.0 == line });
                            assert!(railed, "{at}: a ramp {:?} open over the water on its side {sg}", r.rect);
                        }
                    }
                    assert!(it.connected(), "{at}: doors don't join");
                    // railed wherever a walkway meets the water
                    if !it.pool.is_empty() && !it.bridge.is_empty() {
                        assert!(it.rails.iter().any(|r| r.0 .2 == z0), "{at}: no rail on the floor");
                    }
                    for &r in &it.rafts {
                        rafts += 1;
                        assert!(rect_cells(r).is_subset(&it.bridge), "{at}: a raft {r:?} is no walkway");
                    }
                    for t in it.extra_solids.iter().filter(|t| t.6 == "tank") {
                        tanks += 1;
                        assert!(rect_cells((t.0, t.1, t.2, t.3)).is_subset(&it.pool) && t.4 == water, "{at}: a tank out of the water");
                    }
                    assert!(it.piers.iter().all(|q| q.4 == water), "{at}: a pier on no ground");
                    assert!(lv.pools.iter().any(|(k, z)| k == key && *z == water), "{at}: its water is not drawn");
                    assert!(!lv.drops.iter().any(|d| d.0 == *key || d.1 == *key), "{at}: a drop into or out of it");
                }
            }
        }
        assert!(levels >= 10 && floods >= 40 && rafts >= 5 && tanks >= 10, "{levels} levels, {floods} floods, {rafts} rafts, {tanks} tanks");
        // none when told none
        assert!(generate(40, &Options { flooded: 0.0, ..Options::default() }).unwrap().interiors.iter().all(|(_, it)| it.flood.is_none()));
    }
}
