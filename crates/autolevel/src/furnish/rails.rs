//! Rails on every edge that drops, the side of a hole that stays open, and the sloped rails down
//! ramps. From `furnish.py` (`lay_rails`, `hole_approach`, `_hole_side`, `ramp_rails`).

use super::{bounds, common, runs, Interior, Node};
use crate::cells::{Cell, Cells};
use crate::plan::{cells as units, Point3};
use crate::py;
use crate::space::DIRS;
use std::collections::{BTreeMap, BTreeSet};

/// A rail: its two ends in units, and the way it faces (out over the drop).
pub type Rail = (Point3, Point3, (i32, i32, i32));

impl Interior {
    /// Every edge with a drop past it, in units. It also settles which side of the room's hole
    /// stays open (`hole_open`).
    pub fn lay_rails(&mut self) -> Vec<Rail> {
        // (storey, direction, the line it is on) -> positions along it
        let mut edges: BTreeMap<(i32, usize, i32), BTreeSet<i32>> = BTreeMap::new();
        let pits: Cells = self.pool.union(&self.hole).copied().collect();
        self.hole_open = self.hole_side();
        let enter = self.hole_open.map(|side| (side + 1) % 4); // the way a player steps into the hole
        for &z in &self.levels {
            let walk = self.walk(z);
            let floor = z == self.levels[0];
            if floor && pits.is_empty() {
                continue;
            }
            let mut arrive: BTreeSet<(Cell, usize)> = BTreeSet::new(); // where a ramp joins this storey
            for r in &self.ramps {
                let rc = r.cells();
                if r.hi == z {
                    let back = (r.d + 2) % 4;
                    for &c in &r.top {
                        if rc.contains(&(c.0 + DIRS[back].0, c.1 + DIRS[back].1)) {
                            arrive.insert((c, back));
                        }
                    }
                }
                if r.lo == z {
                    for &c in &r.foot {
                        if rc.contains(&(c.0 + DIRS[r.d].0, c.1 + DIRS[r.d].1)) {
                            arrive.insert((c, r.d));
                        }
                    }
                }
            }
            let open = self.unrailed(z); // a pit's open side and its planks' sides
            let mut under = Cells::new();
            if !floor {
                for r in self.ramps.iter().filter(|r| r.lo == z) {
                    under.extend(r.cells());
                }
            }
            for &c in &walk {
                if floor && !DIRS.iter().any(|(dx, dy)| pits.contains(&(c.0 + dx, c.1 + dy))) {
                    continue;
                }
                if under.contains(&c) {
                    // deck that a ramp rises over: its edge there would be a rail across the ramp
                    // (the ramp's own side rails keep players out)
                    continue;
                }
                for (d, &(dx, dy)) in DIRS.iter().enumerate() {
                    let n = (c.0 + dx, c.1 + dy);
                    if !self.inside(n) || walk.contains(&n) || self.shell[&z].contains(&n) || arrive.contains(&(c, d)) || open.contains(&(c, d)) {
                        continue;
                    }
                    if floor && (!pits.contains(&n) || (self.hole.contains(&n) && Some(d) == enter)) {
                        continue;
                    }
                    if dx != 0 {
                        edges.entry((z, d, c.0 + if dx > 0 { 1 } else { 0 })).or_default().insert(c.1);
                    } else {
                        edges.entry((z, d, c.1 + if dy > 0 { 1 } else { 0 })).or_default().insert(c.0);
                    }
                }
            }
        }
        let mut rails = Vec::new();
        for ((z, d, line), pos) in edges {
            let (dx, dy) = DIRS[d];
            let line = units(line as f64 / 2.0);
            for run in runs(pos, None) {
                let (a, b) = (units(run[0] as f64 / 2.0), units((run[run.len() - 1] + 1) as f64 / 2.0));
                if dx != 0 {
                    rails.push(((line, a, z), (line, b, z), (dx, 0, 0)));
                } else {
                    rails.push(((a, line, z), (b, line, z), (0, dy, 0)));
                }
            }
        }
        rails
    }

    /// The nav node a player steps into the hole from, on one of its sides (0 south, 1 east,
    /// 2 north, 3 west). The room must have a hole.
    pub fn hole_approach(&self, side: usize) -> Node {
        let (x0, y0, x1, y1) = bounds(&self.hole);
        let (mx, my) = ((x0 + x1).div_euclid(2) - 1, (y0 + y1).div_euclid(2) - 1);
        let (x, y) = [(mx, y0 - 2), (x1, my), (mx, y1), (x0 - 2, my)][side];
        (x, y, self.levels[0])
    }

    /// Which side of the hole stays open: one a door can be walked to from, the nearest to the
    /// middle of the room first. None (railed all round) if there is none.
    fn hole_side(&self) -> Option<usize> {
        if self.hole.is_empty() {
            return None;
        }
        let first = self.door_nodes().into_iter().flatten().next()?;
        let (cx, cy) = ((self.x0 + self.x1) as f64 / 2.0, (self.y0 + self.y1) as f64 / 2.0);
        let off = |s: &usize| {
            let node = self.hole_approach(*s);
            (node.0 as f64 - cx).abs() + (node.1 as f64 - cy).abs()
        };
        let mut sides = [0, 1, 2, 3];
        sides.sort_by(|a, b| py::cmp_f64(off(a), off(b)));
        let g = self.nav();
        sides.into_iter().find(|&s| {
            let node = self.hole_approach(s);
            g.contains_key(&node) && Self::path_in(&g, first, node).is_some()
        })
    }

    /// A sloped rail, in units, down each side of a ramp that is open (not against a wall, and
    /// not alongside the deck it climbs to, which has its own rail there).
    pub fn ramp_rails(&self) -> Vec<Rail> {
        let mut out = Vec::new();
        for r in &self.ramps {
            let (x0, y0, x1, y1) = r.rect;
            let (dx, _) = DIRS[r.d];
            let ((fx, fy), (tx, ty)) = r.line();
            let deck: Cells = self.deck[&r.hi].union(&self.roof[&r.hi]).copied().collect();
            for sg in [-1, 1] {
                let cs: Cells = if dx != 0 {
                    (x0..x1).map(|x| (x, if sg < 0 { y0 - 1 } else { y1 })).collect()
                } else {
                    (y0..y1).map(|y| (if sg < 0 { x0 - 1 } else { x1 }, y)).collect()
                };
                if !cs.iter().all(|c| self.inside(*c)) || 2 * common(&cs, &deck) >= cs.len() {
                    continue;
                }
                if dx != 0 {
                    let edge = units((if sg < 0 { y0 } else { y1 }) as f64 / 2.0);
                    out.push(((units(fx), edge, r.lo), (units(tx), edge, r.hi), (0, sg, 0)));
                } else {
                    let edge = units((if sg < 0 { x0 } else { x1 }) as f64 / 2.0);
                    out.push(((edge, units(fy), r.lo), (edge, units(ty), r.hi), (sg, 0, 0)));
                }
            }
        }
        out
    }
}
