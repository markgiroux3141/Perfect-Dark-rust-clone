//! Drops inside a room: holes in its platforms (`platforms`) down to what stands under them, as
//! the drops between rooms (`build::openings`) go through a floor into the room below. New in the
//! Rust (docs/AUTOLEVEL.md, "Drops"): the Python has none.
//!
//! Two kinds, each in the middle of a platform with a catwalk's depth of deck round it:
//!
//! - **a hole**: two cells square (two by three in a big platform), railed on three sides and
//!   open on the side towards the catwalk the platform hangs off, as a drop between rooms is;
//! - **a span**: the platform's middle open, a catwalk's depth from its edges, crossed by one or
//!   two catwalks a cell wide with no rails: walk across, or step off.
//!
//! A platform gets one with the chance `Setup::holes`, from a stream of its own (so nothing else in
//! the room moves), where what is under the whole opening, within `MAX_FALL` storeys, is a floor a
//! player can stand on all over (the hall's floor, a catwalk, another platform), with nothing in
//! between. Nothing stands where it lands (`keep`). Laid after the platforms, before the cover
//! and the rails, which leave the open side and the spans' sides unrailed (`Interior::unrailed`).

use super::{Interior, Node};
use crate::cells::{rect_cells, Cell, Cells, Rect};
use crate::pyrand::PyRandom;
use crate::storeys::index;

/// The most storeys a drop inside a room falls (about 11 m).
pub const MAX_FALL: usize = 4;
/// Deck left round an opening, h (a catwalk's depth at walkway 1).
const RIM: i32 = 2;

/// A drop inside a room.
#[derive(Clone, Debug, PartialEq)]
pub struct Pit {
    /// The storey of the deck it is cut in, and the storey it lands on.
    pub z: i32,
    pub land: i32,
    /// The opening, h (its bounds: the planks are inside it).
    pub hole: Rect,
    /// A span's catwalks across it, h. Empty: a hole.
    pub planks: Vec<Rect>,
    /// A hole's open side (0 south, 1 east, 2 north, 3 west).
    pub open: Option<usize>,
    /// Where a player steps off (a nav node on the deck), and a block it lands on.
    pub from: Node,
    pub to: Node,
}

impl Pit {
    /// The cells of the opening a player falls through (the planks are deck).
    pub fn cells(&self) -> Cells {
        let mut out = rect_cells(self.hole);
        for p in &self.planks {
            for c in rect_cells(*p) {
                out.remove(&c);
            }
        }
        out
    }
}

/// The nav node a player steps into an opening from, on one of its sides (0 south, 1 east,
/// 2 north, 3 west), on storey `z`: the block just outside it, at its middle.
pub fn approach(hole: Rect, side: usize, z: i32) -> Node {
    let (x0, y0, x1, y1) = hole;
    let (mx, my) = ((x0 + x1).div_euclid(2) - 1, (y0 + y1).div_euclid(2) - 1);
    let (x, y) = [(mx, y0 - 2), (x1, my), (mx, y1), (x0 - 2, my)][side];
    (x, y, z)
}

/// The block at a rectangle's middle (its low corner), on storey `z`.
fn middle(r: Rect, z: i32) -> Node {
    ((r.0 + r.2).div_euclid(2) - 1, (r.1 + r.3).div_euclid(2) - 1, z)
}

impl Interior {
    /// The storey an opening over `cells` on storey `z` lands on: the first below with anything
    /// built under them, if a player can stand on all of it there and it is no more than
    /// `MAX_FALL` storeys down. None if it would land on something else, or fall too far.
    fn lands_on(&self, z: i32, cells: &Cells) -> Option<i32> {
        let below: Vec<i32> = self.levels.iter().copied().filter(|&s| s < z).rev().collect();
        for s in below {
            if index(z) - index(s) > MAX_FALL {
                return None;
            }
            let floor = s == self.levels[0];
            let mut built: Cells = self.walk(s);
            if !floor {
                built.extend(self.shell[&s].iter().copied());
                built.extend(self.roof[&s].iter().copied());
            }
            for r in self.ramps.iter().filter(|r| r.lo == s) {
                built.extend(r.cells());
            }
            for q in self.piers.iter().filter(|q| q.4 <= s && s < q.5) {
                built.extend(rect_cells((q.0, q.1, q.2, q.3)));
            }
            built.extend(self.spanned(s)); // an angled catwalk: no standing room, no falling past
            if !floor && cells.is_disjoint(&built) {
                continue; // air: it falls on
            }
            // (no landing on another opening)
            let open = self.pits.iter().filter(|p| p.z == s).any(|p| !p.cells().is_disjoint(cells));
            return (cells.is_subset(&self.stand(s)) && !open).then_some(s);
        }
        None
    }

    /// An opening in each platform, with the chance `Setup::holes` (see the module).
    pub(super) fn lay_pits(&mut self) {
        if self.hole_share <= 0.0 {
            return;
        }
        for (z, r) in self.platforms.clone() {
            let mut rng = PyRandom::named(&format!("{}/pit/{z}/{}/{}", self.stream, r.0, r.1));
            if rng.random() >= self.hole_share {
                continue;
            }
            let spanned = rng.random() < 0.5;
            let pit = if spanned { self.span(z, r).or_else(|| self.hole_in(z, r)) } else { self.hole_in(z, r).or_else(|| self.span(z, r)) };
            let Some(pit) = pit else { continue };
            if !self.angled.is_empty() && !pit.cells().is_disjoint(&self.joints(z)) {
                continue; // (an angled catwalk ends on the deck it would cut)
            }
            let open = pit.cells();
            let deck = self.deck.get_mut(&z).expect("a storey of the room");
            deck.retain(|c| !open.contains(c));
            if pit.land == self.levels[0] {
                self.keep.push(pit.hole); // nothing stands where it lands
            }
            self.pits.push(pit);
        }
    }

    /// The side of a platform that is on its catwalk (the one with the most deck beside it).
    fn joined_side(&self, z: i32, r: Rect) -> usize {
        let (x0, y0, x1, y1) = r;
        let deck = &self.deck[&z];
        let beside = |cs: Cells| cs.iter().filter(|c| deck.contains(c)).count();
        let sides = [rect_cells((x0, y0 - 1, x1, y0)), rect_cells((x1, y0, x1 + 1, y1)), rect_cells((x0, y1, x1, y1 + 1)), rect_cells((x0 - 1, y0, x0, y1))];
        let counts: Vec<usize> = sides.into_iter().map(beside).collect();
        (0..4).max_by_key(|&s| (counts[s], std::cmp::Reverse(s))).expect("four sides")
    }

    /// A hole two cells square (two by three where the platform has room) in the platform's
    /// middle, open towards its catwalk. None if it has no room or nothing to land on.
    fn hole_in(&self, z: i32, r: Rect) -> Option<Pit> {
        let (w, d) = (r.2 - r.0, r.3 - r.1);
        let (hw, hd) = (if w >= 6 + 2 * RIM + 2 { 6 } else { 4 }, if d >= 6 + 2 * RIM + 2 && w < 6 + 2 * RIM + 2 { 6 } else { 4 });
        if w < hw + 2 * RIM || d < hd + 2 * RIM {
            return None;
        }
        let (x0, y0) = (r.0 + (w - hw) / 2, r.1 + (d - hd) / 2);
        let hole = (x0, y0, x0 + hw, y0 + hd);
        let land = self.lands_on(z, &rect_cells(hole))?;
        let side = self.joined_side(z, r);
        Some(Pit { z, land, hole, planks: Vec::new(), open: Some(side), from: approach(hole, side, z), to: middle(hole, land) })
    }

    /// The platform's middle open, crossed by one or two unrailed catwalks a cell wide along its
    /// longer side. None if it has no room for gaps a cell wide either side, or nothing to land on.
    fn span(&self, z: i32, r: Rect) -> Option<Pit> {
        let hole = (r.0 + RIM, r.1 + RIM, r.2 - RIM, r.3 - RIM);
        let (w, d) = (hole.2 - hole.0, hole.3 - hole.1);
        let along_x = w >= d;
        let (long, across) = if along_x { (w, d) } else { (d, w) };
        let n = if across >= 2 * 2 + 3 * 2 { 2 } else { 1 };
        if long < 4 || across < 2 * n + 2 * (n + 1) {
            return None;
        }
        // planks evenly between the gaps
        let gap = (across - 2 * n) as f64 / (n + 1) as f64;
        let planks: Vec<Rect> = (0..n)
            .map(|k| {
                let a = (gap * (k + 1) as f64 + 2.0 * k as f64).round() as i32;
                if along_x {
                    (hole.0, hole.1 + a, hole.2, hole.1 + a + 2)
                } else {
                    (hole.0 + a, hole.1, hole.0 + a + 2, hole.3)
                }
            })
            .collect();
        let pit = Pit { z, land: 0, hole, planks: planks.clone(), open: None, from: (0, 0, 0), to: (0, 0, 0) };
        let cells = pit.cells();
        let land = self.lands_on(z, &cells)?;
        // step off the first plank's middle into the gap before it
        let p = planks[0];
        let from = middle(p, z);
        let to = if along_x {
            ((p.0 + p.2).div_euclid(2) - 1, hole.1 + (p.1 - hole.1 - 2).div_euclid(2).max(0), land)
        } else {
            (hole.0 + (p.0 - hole.0 - 2).div_euclid(2).max(0), (p.1 + p.3).div_euclid(2) - 1, land)
        };
        Some(Pit { land, from, to, ..pit })
    }

    /// Edges the rails leave open on storey `z`: (deck cell, the way out of it). A hole's open side
    /// and a span's planks' sides.
    pub(super) fn unrailed(&self, z: i32) -> std::collections::BTreeSet<(Cell, usize)> {
        let mut out = std::collections::BTreeSet::new();
        for p in self.pits.iter().filter(|p| p.z == z) {
            let open = p.cells();
            let (x0, y0, x1, y1) = p.hole;
            match p.open {
                Some(side) => {
                    // the deck along the open side, stepping in
                    let (cells, d): (Vec<Cell>, usize) = match side {
                        0 => ((x0..x1).map(|x| (x, y0 - 1)).collect(), 1),
                        1 => ((y0..y1).map(|y| (x1, y)).collect(), 2),
                        2 => ((x0..x1).map(|x| (x, y1)).collect(), 3),
                        _ => ((y0..y1).map(|y| (x0 - 1, y)).collect(), 0),
                    };
                    out.extend(cells.into_iter().map(|c| (c, d)));
                }
                None => {
                    for plank in &p.planks {
                        for c in rect_cells(*plank) {
                            for (d, (dx, dy)) in crate::space::DIRS.iter().enumerate() {
                                if open.contains(&(c.0 + dx, c.1 + dy)) {
                                    out.insert((c, d));
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{generate, Options};
    use crate::place::Scale;

    #[test]
    fn a_pit_drops_through_a_platform_onto_what_is_under_it() {
        let (mut levels, mut pits, mut spans) = (0, 0, 0);
        for seed in 1..=6 {
            let mut o = Options { platforms: 1.0, galleries: 2, ..Options::default() };
            o.siting.scale = Scale::Towering;
            let plain = generate(seed, &o);
            o.holes = 1.0;
            let lv = generate(seed, &o);
            assert_eq!(plain.is_ok(), lv.is_ok(), "seed {seed}: holes change whether a level is made");
            let Ok(lv) = lv else { continue };
            levels += 1;
            for (key, it) in &lv.interiors {
                for (n, p) in it.pits.iter().enumerate() {
                    pits += 1;
                    spans += usize::from(!p.planks.is_empty());
                    let open = p.cells();
                    assert!(open.is_disjoint(&it.deck[&p.z]), "seed {seed}: {key}@{} pit {n} is still deck", p.z);
                    assert!(p.planks.iter().all(|q| rect_cells(*q).is_subset(&it.deck[&p.z])), "seed {seed}: {key}@{} a plank is no deck", p.z);
                    assert!(p.land < p.z && index(p.z) - index(p.land) <= MAX_FALL);
                    assert!(open.is_subset(&it.stand(p.land)), "seed {seed}: {key}@{} lands on {} where nobody stands", p.z, p.land);
                    assert!(
                        it.platforms.iter().any(|&(z, r)| z == p.z && rect_cells(p.hole).is_subset(&rect_cells(r))),
                        "seed {seed}: {key}@{} not in a platform",
                        p.z
                    );
                    assert!(lv.walks.iter().any(|(name, _)| name.starts_with(&format!("pit_{key}_{}", p.z))), "seed {seed}: {key}@{} has no walk", p.z);
                }
            }
        }
        assert!(levels >= 5 && pits >= 10 && spans >= 2, "{levels} levels, {pits} pits, {spans} spans");
        // none unless asked for
        let mut o = Options { platforms: 1.0, ..Options::default() };
        o.siting.scale = Scale::Towering;
        assert!(generate(9, &o).unwrap().interiors.iter().all(|(_, it)| it.pits.is_empty()));
    }
}
