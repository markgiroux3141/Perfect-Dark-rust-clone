//! A sunken floor: a rectangle of a room's floor `level::SUNK_DEPTH` lower (`Space::sunk`, chosen
//! by `level::sinks`), open all round (no railing), and a ramp down into it from the middle of one
//! of its sides, a floor ramp with a lip down each side and no railing either (the user's calls,
//! 2026-11-19). The side is
//! drawn from the room's own stream ("{seed}/{key}/sunken") among those with floor a cell deep in
//! front of the ramp's top. New in the Rust (docs/AUTOLEVEL.md, "Sunken floors").
//!
//! The pit is not the floor's: nothing stands in it or a cell round it, no ramp's foot is there,
//! no drop lands in it, and the room's nav goes round it. Its own walk goes down the ramp and back.

use super::{grow, Interior};
use crate::cells::{rect_cells, Cells, Rect};
use crate::level::SUNK_DEPTH;
use crate::pyrand::PyRandom;

/// A sunken floor's ramp's run, h: two cells for a cell's drop (26.6 degrees, a little gentler
/// than a storey's ramps).
pub const RUN: i32 = 4;

/// A sunken floor, as laid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sunk {
    /// Its rectangle, h.
    pub rect: Rect,
    /// Its floor's height.
    pub z: i32,
    /// The side its ramp comes down from (0 south, 1 east, 2 north, 3 west), and the ramp: in the
    /// pit, its top at the middle of that side.
    pub side: usize,
    pub ramp: Rect,
}

impl Sunk {
    /// The sunken floor of `rect` on a floor at `z`, its ramp `band` wide down from `side`.
    pub fn new(rect: Rect, z: i32, side: usize, band: i32) -> Sunk {
        let (x0, y0, x1, y1) = rect;
        let (mx, my) = ((x0 + x1).div_euclid(2) - band / 2, (y0 + y1).div_euclid(2) - band / 2);
        let ramp = match side {
            0 => (mx, y0, mx + band, y0 + RUN),
            1 => (x1 - RUN, my, x1, my + band),
            2 => (mx, y1 - RUN, mx + band, y1),
            _ => (x0, my, x0 + RUN, my + band),
        };
        Sunk { rect, z: z - SUNK_DEPTH, side, ramp }
    }

    /// The floor in front of the ramp's top: a cell deep, its width, h.
    pub fn approach(&self) -> Rect {
        let (x0, y0, x1, y1) = self.ramp;
        match self.side {
            0 => (x0, y0 - 2, x1, y0),
            1 => (x1, y0, x1 + 2, y1),
            2 => (x0, y1, x1, y1 + 2),
            _ => (x0 - 2, y0, x0, y1),
        }
    }

    /// The ramp's centre line, cells: its top (on the pit's edge), then its foot.
    pub fn line(&self) -> ((f64, f64), (f64, f64)) {
        let (x0, y0, x1, y1) = self.ramp;
        let (mx, my) = ((x0 + x1) as f64 / 4.0, (y0 + y1) as f64 / 4.0);
        let c = |v: i32| v as f64 / 2.0;
        match self.side {
            0 => ((mx, c(y0)), (mx, c(y1))),
            1 => ((c(x1), my), (c(x0), my)),
            2 => ((mx, c(y1)), (mx, c(y0))),
            _ => ((c(x0), my), (c(x1), my)),
        }
    }

    /// A point in the pit across from the ramp's foot, a cell in from the far side, cells; and the
    /// floor a cell past that side, which the pit's wall keeps a player from.
    pub fn far(&self) -> ((f64, f64), (f64, f64)) {
        let (x0, y0, x1, y1) = self.rect;
        let ((tx, ty), _) = self.line();
        let h = |v: i32| v as f64 / 2.0;
        match self.side {
            0 => ((tx, h(y1) - 1.0), (tx, h(y1) + 1.0)),
            1 => ((h(x0) + 1.0, ty), (h(x0) - 1.0, ty)),
            2 => ((tx, h(y0) + 1.0), (tx, h(y0) - 1.0)),
            _ => ((h(x1) - 1.0, ty), (h(x1) + 1.0, ty)),
        }
    }
}

impl Interior {
    /// What keeps clear of the sunken floor: the pit and a cell round it, and its ramp's way in
    /// with a half cell each side. Empty if there is none.
    pub(super) fn sunk_clear(&self) -> Cells {
        let Some(sk) = &self.sunk else { return Cells::new() };
        let mut out = rect_cells(grow(sk.rect, 2));
        out.extend(rect_cells(grow(sk.approach(), 1)));
        out
    }

    /// The sunken floor's ramp (see the module), or none: the pit is the floor's again where no
    /// side has floor in front of it (its rectangle keeps from the walls, so every side does).
    pub(super) fn lay_sunk(&mut self) {
        let Some(r) = self.s.sunk else { return };
        let z0 = self.levels[0];
        let rect = (2 * r.0, 2 * r.1, 2 * r.2, 2 * r.3);
        let floor = self.walk(z0);
        let mut rng = PyRandom::named(&format!("{}/sunken", self.stream));
        let mut sides = [0, 1, 2, 3];
        rng.shuffle(&mut sides);
        let fits = |sk: &Sunk| {
            // (the half cell round its way in that is not the pit's)
            let way: Cells = rect_cells(grow(sk.approach(), 1)).difference(&self.sunk_cells).copied().collect();
            way.is_subset(&floor) && way.is_disjoint(&self.shell[&z0])
        };
        match sides.into_iter().map(|side| Sunk::new(rect, z0, side, self.band)).find(fits) {
            Some(sk) => self.sunk = Some(sk),
            None => self.sunk_cells.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{generate, Options, SUNK_INSET};
    use crate::place::Scale;

    #[test]
    fn a_sunken_floor_is_a_pit_a_cell_deep_with_a_ramp_down_and_nothing_round_it() {
        let (mut levels, mut sunk, mut tall, mut sides) = (0, 0, 0, [0; 4]);
        for (scale, seeds) in [(Scale::Grand, 1..=10), (Scale::Towering, 1..=4)] {
            for seed in seeds {
                let mut o = Options::python();
                o.siting.scale = scale;
                let plain = generate(seed, &o);
                o.sunken = 1.0;
                let lv = generate(seed, &o);
                assert_eq!(plain.is_ok(), lv.is_ok(), "seed {seed}: a sunken floor changes whether a level is made");
                let Ok(lv) = lv else { continue };
                levels += 1;
                for (key, it) in &lv.interiors {
                    let Some(sk) = &it.sunk else {
                        assert!(it.sunk_cells.is_empty(), "seed {seed}: {key}");
                        continue;
                    };
                    let at = format!("seed {seed}: {key}");
                    let s = &it.s;
                    let z0 = it.levels[0];
                    sunk += 1;
                    tall += usize::from(!it.upper.is_empty());
                    sides[sk.side] += 1;
                    // a cell under the floor, the room's own (`Space::sunk`), clear of its walls
                    let r = s.sunk.unwrap();
                    assert_eq!((sk.rect, sk.z), ((2 * r.0, 2 * r.1, 2 * r.2, 2 * r.3), z0 - SUNK_DEPTH), "{at}");
                    assert!(r.0 - s.x0 >= SUNK_INSET && s.x1 - r.2 >= SUNK_INSET && r.1 - s.y0 >= SUNK_INSET && s.y1 - r.3 >= SUNK_INSET, "{at}");
                    // the ramp is in it, against the side it comes down from, its way in floor
                    let pit = rect_cells(sk.rect);
                    assert!(rect_cells(sk.ramp).is_subset(&pit), "{at}");
                    assert!(rect_cells(sk.approach()).is_subset(&it.stand(z0)), "{at}: its way in");
                    // nothing of the floor's in it or a cell round it: not standing room, no cover,
                    // no ramp's foot, no pier
                    assert!(pit.is_disjoint(&it.stand(z0)), "{at}");
                    let round = rect_cells(grow(sk.rect, 2));
                    for c in &it.cover {
                        assert!(rect_cells((c.0, c.1, c.2, c.3)).is_disjoint(&round), "{at}: {c:?}");
                    }
                    for rp in &it.ramps {
                        assert!(rp.cells().is_disjoint(&round) && rp.foot.is_disjoint(&round), "{at}: a ramp at it");
                    }
                    // its walks: down and back, and no way out over its far side
                    for name in [format!("sunk_{key}"), format!("!sunk_{key}")] {
                        assert!(lv.walks.iter().any(|(n, _)| *n == name), "{at}: no {name}");
                    }
                }
            }
        }
        assert!(
            levels >= 12 && sunk >= 15 && tall >= 5 && sides.iter().all(|&n| n >= 2), // (a room of one storey is seldom ten cells across)
            "{levels} levels, {sunk} sunken floors ({tall} in halls), by side {sides:?}"
        );
        // none when told none
        assert!(generate(40, &Options::python()).unwrap().interiors.iter().all(|(_, it)| it.sunk.is_none()));
    }
}
