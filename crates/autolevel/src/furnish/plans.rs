//! What a furnished room gives the level's plan: its air, its solids and its ramps, in units.
//! From `furnish.py` (`boxes`, `solids`, `plan_ramps`).

use super::{rectangles, shell_z, Interior};
use crate::cells::Rect;
use crate::plan::{box3, cells as units, subtract, Box3, Point3, Ramp};
use crate::space::{shell_rect, DIRS};

/// A rectangle of half cells between two heights, as a box in units.
fn box_h(r: Rect, z0: i32, z1: i32) -> Box3 {
    box3(r.0 as f64 / 2.0, r.1 as f64 / 2.0, r.2 as f64 / 2.0, r.3 as f64 / 2.0, z0, z1)
}

impl Interior {
    /// The room's air: its box with the pods' blocks cut out, and the pit under its water.
    pub fn boxes(&self) -> Vec<Box3> {
        let s = &self.s;
        let main = box3(s.x0 as f64, s.y0 as f64, s.x1 as f64, s.y1 as f64, s.floor, s.top);
        let shells: Vec<Box3> = self
            .pods
            .iter()
            .map(|p| {
                let (lo, hi) = shell_z(p, s);
                box_h(shell_rect(p, s), lo, hi)
            })
            .collect();
        let mut out = if shells.is_empty() { vec![main] } else { subtract(main, &shells) };
        if let Some(p) = s.pool {
            out.push(box3(p.0 as f64, p.1 as f64, p.2 as f64, p.3 as f64, p.4, s.floor));
        }
        if let Some(water) = self.flood {
            // a flooded room's pit, all of it but under its pods (which stand on its ground)
            let pit = box3(s.x0 as f64, s.y0 as f64, s.x1 as f64, s.y1 as f64, water, s.floor);
            let under: Vec<Box3> = self.pods.iter().filter(|p| p.floor == s.floor).map(|p| box_h(shell_rect(p, s), water, s.floor)).collect();
            out.extend(if under.is_empty() { vec![pit] } else { subtract(pit, &under) });
        }
        out
    }

    /// Its solids: (low corner, high corner, tag). Decks and the bridge ("trim"), pods' piers
    /// ("column"), tanks, and the cover.
    pub fn solids(&self) -> Vec<(Point3, Point3, &'static str)> {
        let mut out = Vec::new();
        let z0 = self.levels[0];
        let slab = |z: i32| z - if z == 0 { 39 } else { 40 };
        let mut add = |b: Box3, tag: &'static str| out.push((b.0, b.1, tag));
        for &z in &self.upper {
            for r in rectangles(&self.deck[&z]) {
                add(box_h(r, slab(z), z), "trim");
            }
        }
        for r in rectangles(&self.bridge) {
            add(box_h(r, slab(z0), z0), "trim");
        }
        for &(x0, y0, x1, y1, a, b) in &self.piers {
            add(box_h((x0, y0, x1, y1), a, b), "column");
        }
        for &(x0, y0, x1, y1, a, b, tag) in &self.extra_solids {
            add(box_h((x0, y0, x1, y1), a, b), tag);
        }
        for &(x0, y0, x1, y1, h, tag) in &self.cover {
            add(box_h((x0, y0, x1, y1), z0, z0 + h), tag);
        }
        out
    }

    /// Its ramps as the plan holds them: open, with a hanging edge strip down each side that is
    /// not against a wall.
    pub fn plan_ramps(&self) -> Vec<Ramp> {
        let mut out = Vec::new();
        for r in &self.ramps {
            let ((fx, fy), (tx, ty)) = r.line();
            let (x0, y0, x1, y1) = r.rect;
            let (dx, _) = DIRS[r.d];
            // the cells along its low side and its high side
            let (lows, highs) = if dx != 0 { ((x0, y0 - 1, x1, y0), (x0, y1, x1, y1 + 1)) } else { ((x0 - 1, y0, x0, y1), (x1, y0, x1 + 1, y1)) };
            let open_sides = [(-1, lows), (1, highs)].into_iter().filter(|(_, cs)| self.holds(*cs)).map(|(sg, _)| sg).collect();
            out.push(Ramp {
                start: (units(fx), units(fy), r.lo),
                end: (units(tx), units(ty), r.hi),
                width: units(self.band as f64 / 2.0),
                kind: "open",
                open_sides,
            });
        }
        out
    }
}
