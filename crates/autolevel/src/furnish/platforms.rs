//! A hall's empty air, and platforms to fill it. New in the Rust (docs/AUTOLEVEL.md,
//! "Platforms"): the Python has neither.
//!
//! **Empty air** (`Interior::void`): the half cells of a tall room over which nothing is built on
//! any storey above its floor (no catwalk, pod, pod roof, ramp, landing or pier, and nothing a
//! drop or a grate's beam falls through). A column of it is air from the floor to the ceiling.
//!
//! **Platforms** (`Interior::lay_platforms`): a large square of deck pushed out from a catwalk
//! into the emptiest of that air, as the landings at a catwalk's turns are but several cells each
//! way. One is laid where it stands over empty air only, a cell clear of anything hard (pods,
//! ramps, piers) and of the walls, along a good length of catwalk on its own storey, and with
//! mostly empty air round it; the best by its size and how empty its neighbourhood is, then the
//! next, each kept clear of the last and counting for less the fewer storeys it is from another
//! (`SPREAD`). No random numbers: a room's other choices are as they were.
//! Laid after the ramps and before the cover and the rails, so the rails run round a platform's
//! open edges and a pillar under one stops at its underside, as under any catwalk.

use super::{grow, Interior};
use crate::cells::{rect_cells, Cell, Cells, Rect};
use crate::storeys::{index, LEVELS};

/// The least cells of empty air a platform is laid for, h (100 cells: one in a hall that has
/// that much, more in a bigger one, by `Setup::platforms`).
const PER: f64 = 400.0;
/// A room's shorter side is at least this, h, for a platform.
const NARROWEST: i32 = 20;
/// Platform sizes, h: (along the catwalk, out from it), the biggest first.
const SIZES: [(i32, i32); 6] = [(16, 12), (14, 10), (12, 12), (12, 8), (10, 8), (8, 8)];
/// How far round a platform its neighbourhood reaches, h, and how much of it must be empty air.
const AROUND: i32 = 6;
const EMPTY: f64 = 0.6;
/// A platform's least edge on its catwalk, h.
const CONTACT: i32 = 4;
/// Storeys between two platforms of a room before the second counts in full.
const SPREAD: f64 = 3.0;
/// What is built this many storeys over or under a platform's is in its way (a column of a tall
/// hall may hold a platform low down and another high up).
const WINDOW: i64 = 2;

/// The room's half cells as a grid, with how many of any rectangle's are set, at once.
pub(super) struct Sums {
    x0: i32,
    y0: i32,
    w: i32,
    d: i32,
    /// (w + 1) x (d + 1): the set cells below and left of each corner.
    s: Vec<i32>,
}

impl Sums {
    pub(super) fn new(it: &Interior, set: &Cells) -> Sums {
        let (w, d) = (it.w, it.d);
        let mut s = vec![0; ((w + 1) * (d + 1)) as usize];
        for y in 0..d {
            for x in 0..w {
                let on = set.contains(&(it.x0 + x, it.y0 + y)) as i32;
                let at = |x: i32, y: i32| (y * (w + 1) + x) as usize;
                s[at(x + 1, y + 1)] = on + s[at(x, y + 1)] + s[at(x + 1, y)] - s[at(x, y)];
            }
        }
        Sums { x0: it.x0, y0: it.y0, w, d, s }
    }

    /// How many of a rectangle's cells are set (the part of it inside the room).
    pub(super) fn count(&self, r: Rect) -> i32 {
        let x0 = (r.0 - self.x0).clamp(0, self.w);
        let y0 = (r.1 - self.y0).clamp(0, self.d);
        let x1 = (r.2 - self.x0).clamp(0, self.w);
        let y1 = (r.3 - self.y0).clamp(0, self.d);
        if x1 <= x0 || y1 <= y0 {
            return 0;
        }
        let at = |x: i32, y: i32| self.s[(y * (self.w + 1) + x) as usize];
        at(x1, y1) - at(x0, y1) - at(x1, y0) + at(x0, y0)
    }

    /// How many of a rectangle's cells are inside the room.
    pub(super) fn inside(&self, r: Rect) -> i32 {
        let span = |a: i32, b: i32, lo: i32, n: i32| ((b - lo).clamp(0, n) - (a - lo).clamp(0, n)).max(0);
        span(r.0, r.2, self.x0, self.w) * span(r.1, r.3, self.y0, self.d)
    }
}

/// A room's empty air (`Interior::void`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Void {
    /// Cells of empty air, and of the room.
    pub empty: f64,
    pub room: f64,
    /// The biggest rectangle of it: (cells, its rectangle in half cells).
    pub largest: f64,
    pub rect: Option<Rect>,
}

/// The biggest rectangle of set cells in a grid of `w` x `d` (row-major), as (x0, y0, x1, y1).
fn largest(grid: &[bool], w: i32, d: i32) -> Option<Rect> {
    let mut heights = vec![0i32; w as usize];
    let mut best: Option<(i32, Rect)> = None;
    for y in 0..d {
        for x in 0..w {
            heights[x as usize] = if grid[(y * w + x) as usize] { heights[x as usize] + 1 } else { 0 };
        }
        // the biggest rectangle under the skyline of column heights ending on row y
        let mut stack: Vec<(i32, i32)> = Vec::new(); // (start x, height)
        for x in 0..=w {
            let h = if x < w { heights[x as usize] } else { 0 };
            let mut start = x;
            while let Some(&(sx, sh)) = stack.last() {
                if sh <= h {
                    break;
                }
                stack.pop();
                let area = sh * (x - sx);
                if best.is_none_or(|(a, _)| area > a) {
                    best = Some((area, (sx, y + 1 - sh, x, y + 1)));
                }
                start = sx;
            }
            if stack.last().is_none_or(|&(_, sh)| sh < h) {
                stack.push((start, h));
            }
        }
    }
    best.filter(|(a, _)| *a > 0).map(|(_, r)| r)
}

impl Interior {
    /// What stands in the air of the storeys above the floor (all of them, or with `near` those
    /// within `WINDOW` of that one): (soft, hard). Soft is deck, which a platform may stand
    /// beside; hard is everything a platform keeps a cell clear of (pods' blocks and roofs, ramps
    /// and their landings, piers, angled catwalks, and, on every storey, where a drop or a beam
    /// falls).
    pub(super) fn built(&self, near: Option<i32>) -> (Cells, Cells) {
        let at = |z: i32| index(z) as i64;
        let (lo, hi) = near.map_or((i64::MIN, i64::MAX), |z| (at(z) - WINDOW, at(z) + WINDOW));
        let within = |a: i64, b: i64| a <= hi && lo <= b;
        let mut soft = Cells::new();
        let mut hard = Cells::new();
        for &z in self.upper.iter().filter(|&&z| within(at(z), at(z))) {
            soft.extend(self.deck[&z].iter().copied());
            hard.extend(self.shell[&z].iter().copied());
            hard.extend(self.roof[&z].iter().copied());
            hard.extend(self.spanned(z)); // under an angled catwalk
        }
        for r in self.ramps.iter().filter(|r| within(at(r.lo), at(r.hi))) {
            hard.extend(r.cells());
            hard.extend(r.top.iter().copied());
            hard.extend(r.foot.iter().copied());
        }
        // (a pier runs from one height to another: the storeys it stands in)
        let holding = |h: i32| LEVELS.iter().rposition(|&l| l <= h).unwrap_or(0) as i64;
        for q in self.piers.iter().filter(|q| within(holding(q.4), holding(q.5 - 1))) {
            hard.extend(rect_cells((q.0, q.1, q.2, q.3)));
        }
        for k in &self.keep {
            hard.extend(rect_cells(*k));
        }
        hard.extend(self.hole.iter().copied());
        hard.extend(self.cut.iter().copied()); // (a shaped room's: solid)
        (soft, hard)
    }

    /// The room's empty air: columns from its floor to its ceiling that nothing is built in.
    /// A room of one storey has none.
    pub fn void(&self) -> Void {
        if self.upper.is_empty() {
            return Void::default();
        }
        let (soft, hard) = self.built(None);
        let grid: Vec<bool> = (0..self.d)
            .flat_map(|y| (0..self.w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let c = (self.x0 + x, self.y0 + y);
                !soft.contains(&c) && !hard.contains(&c)
            })
            .collect();
        let empty = grid.iter().filter(|&&e| e).count() as f64 / 4.0;
        let rect = largest(&grid, self.w, self.d).map(|(x0, y0, x1, y1)| (self.x0 + x0, self.y0 + y0, self.x0 + x1, self.y0 + y1));
        let largest = rect.map_or(0.0, |r| ((r.2 - r.0) * (r.3 - r.1)) as f64 / 4.0);
        Void { empty, room: (self.w * self.d) as f64 / 4.0, largest, rect }
    }

    /// Platforms off the catwalks into the emptiest air (see the module), as many as
    /// `Setup::platforms` asks for the room's empty air. A spiral's decks are left as they are.
    pub(super) fn lay_platforms(&mut self) {
        if self.platform_share <= 0.0 || self.upper.is_empty() || self.s.decks.is_some() || self.w.min(self.d) < NARROWEST {
            return;
        }
        let air = |(soft, hard): (Cells, Cells)| self.room.iter().filter(|c| !soft.contains(c) && !hard.contains(c)).count();
        // the empty air each storey sees (a tall hall has more of it than its columns: a storey
        // sees only those near it), shared among the storeys that see the same
        let seen: usize = self.upper.iter().map(|&z| air(self.built(Some(z)))).sum();
        let span = self.upper.len().min(2 * WINDOW as usize + 1) as f64;
        let want = (self.platform_share * seen as f64 / span / PER).round() as usize;
        let band = self.band;
        // a platform stands a catwalk's depth in from every wall
        let (lo_x, lo_y, hi_x, hi_y) = (self.x0 + band, self.y0 + band, self.x1 - band, self.y1 - band);
        for _ in 0..want {
            let mut best: Option<(f64, i32, Rect)> = None;
            for &z in &self.upper {
                let deck = &self.deck[&z];
                if deck.is_empty() {
                    continue;
                }
                let (soft, mut hard) = self.built(Some(z));
                // each platform near this storey keeps the next its distance
                for &(_, pr) in self.platforms.iter().filter(|(pz, _)| index(*pz).abs_diff(index(z)) as i64 <= WINDOW) {
                    hard.extend(rect_cells(grow(pr, 4)).into_iter().filter(|c: &Cell| self.inside(*c)));
                }
                let (s, h) = (Sums::new(self, &soft), Sums::new(self, &hard));
                let free: Cells = self.room.iter().filter(|c| !soft.contains(c) && !hard.contains(c)).copied().collect();
                let f = Sums::new(self, &free);
                let on = Sums::new(self, deck);
                // spread up the hall: a storey near another platform's counts for less
                let near = self.platforms.iter().map(|(pz, _)| crate::storeys::index(*pz).abs_diff(crate::storeys::index(z))).min();
                let apart = near.map_or(1.0, |n| (n as f64 / SPREAD).clamp(0.2, 1.0));
                for (a, b) in SIZES.iter().flat_map(|&(a, b)| [(a, b), (b, a)]) {
                    // a platform reaches no more than a third of the way across
                    if a > (hi_x - lo_x) || b > (hi_y - lo_y) || a.min(b) > self.w.min(self.d) / 3 + 2 {
                        continue;
                    }
                    for y in (lo_y..=hi_y - b).filter(|y| y.rem_euclid(2) == 0) {
                        for x in (lo_x..=hi_x - a).filter(|x| x.rem_euclid(2) == 0) {
                            let r = (x, y, x + a, y + b);
                            if s.count(r) > 0 || h.count(grow(r, 2)) > 0 {
                                continue;
                            }
                            // its edge on the catwalk: the best of its four sides
                            let contact = [(x - 1, y, x, y + b), (x + a, y, x + a + 1, y + b), (x, y - 1, x + a, y), (x, y + b, x + a, y + b + 1)]
                                .into_iter()
                                .map(|side| on.count(side))
                                .max()
                                .unwrap_or(0);
                            if contact < CONTACT {
                                continue;
                            }
                            let round = grow(r, AROUND);
                            let empty = f.count(round) as f64 / f.inside(round).max(1) as f64;
                            if empty < EMPTY {
                                continue;
                            }
                            let score = (a * b) as f64 * empty * apart;
                            if best.is_none_or(|(sc, _, _)| score > sc) {
                                best = Some((score, z, r));
                            }
                        }
                    }
                }
            }
            let Some((_, z, r)) = best else { break };
            self.deck.get_mut(&z).expect("a storey of the room").extend(rect_cells(r));
            self.platforms.push((z, r));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{generate, Options};
    use crate::place::Scale;

    #[test]
    fn a_platform_is_deck_off_a_catwalk_over_empty_air() {
        let (mut levels, mut laid) = (0, 0);
        for (scale, seeds) in [(Scale::Towering, 1..=6), (Scale::Grand, 1..=4)] {
            for seed in seeds {
                let mut o = Options::default();
                o.siting.scale = scale;
                let plain = generate(seed, &o);
                o.platforms = 1.0;
                let lv = generate(seed, &o);
                assert_eq!(plain.is_ok(), lv.is_ok(), "seed {seed}: platforms change whether a level is made");
                let Ok(lv) = lv else { continue };
                levels += 1;
                for (key, it) in &lv.interiors {
                    for &(z, r) in &it.platforms {
                        laid += 1;
                        let cells = rect_cells(r);
                        assert!(cells.is_subset(&it.deck[&z]), "seed {seed}: {key}@{z} {r:?} is not deck");
                        // joined to the rest of its storey's deck along a good edge
                        let rest: Cells = it.deck[&z].difference(&cells).copied().collect();
                        let edge = rect_cells(grow(r, 1)).into_iter().filter(|c| rest.contains(c)).count();
                        assert!(edge as i32 >= CONTACT, "seed {seed}: {key}@{z} {r:?} touches its catwalk along {edge}");
                        // nothing else over or under it, within `WINDOW` storeys
                        for &zz in it.upper.iter().filter(|&&zz| zz != z && index(zz).abs_diff(index(z)) as i64 <= WINDOW) {
                            assert!(
                                cells.is_disjoint(&it.deck[&zz]) && cells.is_disjoint(&it.shell[&zz]) && cells.is_disjoint(&it.roof[&zz]),
                                "seed {seed}: {key}@{z} {r:?} is over or under {zz}"
                            );
                        }
                        let near = |a: i32, b: i32| index(a) as i64 - WINDOW <= index(z) as i64 && index(z) as i64 <= index(b) as i64 + WINDOW;
                        assert!(
                            it.ramps.iter().filter(|rp| near(rp.lo, rp.hi)).all(|rp| cells.is_disjoint(&rp.cells())),
                            "seed {seed}: {key}@{z} {r:?} meets a ramp"
                        );
                        assert!(lv.views.iter().any(|(name, _)| name.starts_with(&format!("platform_{key}_{z}"))));
                    }
                }
                assert!(lv.report.platforms > 0 || lv.report.empty_air < 0.3, "seed {seed}: no platform in {} empty air", lv.report.empty_air);
            }
        }
        assert!(levels >= 8 && laid >= 20, "{levels} levels, {laid} platforms");
        // none unless asked for: every seed's level is as it was
        assert!(generate(40, &Options::default()).unwrap().interiors.iter().all(|(_, it)| it.platforms.is_empty()));
    }

    #[test]
    fn the_largest_rectangle_of_a_grid() {
        // . . # .
        // # # # #
        // # # # .
        let rows = ["..#.", "####", "###."];
        let grid: Vec<bool> = rows.iter().flat_map(|r| r.chars().map(|c| c == '#')).collect();
        assert_eq!(largest(&grid, 4, 3), Some((0, 1, 3, 3)));
        assert_eq!(largest(&[false; 4], 2, 2), None);
    }
}
