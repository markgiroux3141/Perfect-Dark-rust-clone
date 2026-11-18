//! The sculpt planner's grid: cells a plan module across (118.11) and `Z` (59) high, each either
//! solid (0) or the air of one zone (a room or a corridor: 1, 2, ...).
//!
//! ```text
//! let mut v = Vox::new(40, 30, 20);
//! v.set((3, 4, 5), 1);          // zone 1's air
//! v.air((3, 4, 5))              // true
//! v.boxes(1)                    // zone 1's air as disjoint boxes, columns first
//! ```

use std::collections::VecDeque;

/// A cell's height in units: a quarter of a 236 storey, half a module.
pub const Z: i32 = 59;

/// A cell: (x, y, z) indices.
pub type Ix = (i32, i32, i32);

/// The four ways across, then down and up.
pub const DIRS: [Ix; 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, -1), (0, 0, 1)];

pub fn add(a: Ix, b: Ix) -> Ix {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

pub fn up(a: Ix, k: i32) -> Ix {
    (a.0, a.1, a.2 + k)
}

/// A grid of cells. Outside it everything is solid.
#[derive(Clone, Debug)]
pub struct Vox {
    pub nx: i32,
    pub ny: i32,
    pub nz: i32,
    cells: Vec<u16>,
}

impl Vox {
    pub fn new(nx: i32, ny: i32, nz: i32) -> Vox {
        Vox { nx, ny, nz, cells: vec![0; (nx * ny * nz) as usize] }
    }

    pub fn inside(&self, p: Ix) -> bool {
        p.0 >= 0 && p.1 >= 0 && p.2 >= 0 && p.0 < self.nx && p.1 < self.ny && p.2 < self.nz
    }

    fn at(&self, p: Ix) -> usize {
        ((p.2 * self.ny + p.1) * self.nx + p.0) as usize
    }

    /// The zone whose air this is (0: solid, or outside).
    pub fn get(&self, p: Ix) -> u16 {
        if self.inside(p) {
            self.cells[self.at(p)]
        } else {
            0
        }
    }

    /// Set a cell; one outside is left alone.
    pub fn set(&mut self, p: Ix, zone: u16) {
        if self.inside(p) {
            let i = self.at(p);
            self.cells[i] = zone;
        }
    }

    pub fn air(&self, p: Ix) -> bool {
        self.get(p) != 0
    }

    /// Every cell, z then y then x.
    pub fn each(&self) -> impl Iterator<Item = Ix> + '_ {
        let (nx, ny, nz) = (self.nx, self.ny, self.nz);
        (0..nz).flat_map(move |z| (0..ny).flat_map(move |y| (0..nx).map(move |x| (x, y, z))))
    }

    /// The cells of a zone.
    pub fn cells_of(&self, zone: u16) -> Vec<Ix> {
        self.each().filter(|&p| self.get(p) == zone).collect()
    }

    /// The low and high corner (exclusive) of a zone's air, or None if it has none.
    pub fn bounds(&self, zone: u16) -> Option<(Ix, Ix)> {
        let mut out: Option<(Ix, Ix)> = None;
        for p in self.each().filter(|&p| self.get(p) == zone) {
            let (lo, hi) = out.get_or_insert((p, add(p, (1, 1, 1))));
            *lo = (lo.0.min(p.0), lo.1.min(p.1), lo.2.min(p.2));
            *hi = (hi.0.max(p.0 + 1), hi.1.max(p.1 + 1), hi.2.max(p.2 + 1));
        }
        out
    }

    /// The face-connected parts of a zone's air, the largest first.
    pub fn parts(&self, zone: u16) -> Vec<Vec<Ix>> {
        let mut seen = vec![false; self.cells.len()];
        let mut out = Vec::new();
        for p in self.each() {
            if self.get(p) != zone || seen[self.at(p)] {
                continue;
            }
            let mut part = Vec::new();
            let mut q = VecDeque::from([p]);
            seen[self.at(p)] = true;
            while let Some(c) = q.pop_front() {
                part.push(c);
                for d in DIRS {
                    let n = add(c, d);
                    if self.get(n) == zone && !seen[self.at(n)] {
                        seen[self.at(n)] = true;
                        q.push_back(n);
                    }
                }
            }
            out.push(part);
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.len()));
        out
    }

    /// Keep only the largest part of a zone's air; the rest is made solid.
    pub fn keep_largest(&mut self, zone: u16) {
        for part in self.parts(zone).into_iter().skip(1) {
            for p in part {
                self.set(p, 0);
            }
        }
    }

    /// A zone's air as disjoint boxes (low corner, high corner exclusive), grown up first (so a
    /// box reaches its ceiling), then along x, then along y.
    pub fn boxes(&self, zone: u16) -> Vec<(Ix, Ix)> {
        let mut used = vec![false; self.cells.len()];
        let free = |used: &Vec<bool>, p: Ix| self.get(p) == zone && !used[self.at(p)];
        let mut out = Vec::new();
        for p in self.each() {
            if !free(&used, p) {
                continue;
            }
            let mut h = 1;
            while free(&used, up(p, h)) {
                h += 1;
            }
            let column = |used: &Vec<bool>, x: i32, y: i32| (0..h).all(|k| free(used, (x, y, p.2 + k)));
            let mut w = 1;
            while column(&used, p.0 + w, p.1) {
                w += 1;
            }
            let mut d = 1;
            while (0..w).all(|i| column(&used, p.0 + i, p.1 + d)) {
                d += 1;
            }
            for x in 0..w {
                for y in 0..d {
                    for z in 0..h {
                        let i = self.at((p.0 + x, p.1 + y, p.2 + z));
                        used[i] = true;
                    }
                }
            }
            out.push((p, (p.0 + w, p.1 + d, p.2 + h)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_cover_a_zone_once() {
        let mut v = Vox::new(6, 5, 4);
        for p in v.each().collect::<Vec<_>>() {
            if p.0 < 4 && p.2 < 3 || (p.0 >= 4 && p.1 < 2) {
                v.set(p, 1);
            }
        }
        let boxes = v.boxes(1);
        let volume: i32 = boxes.iter().map(|(a, b)| (b.0 - a.0) * (b.1 - a.1) * (b.2 - a.2)).sum();
        assert_eq!(volume as usize, v.cells_of(1).len());
        assert!(boxes.len() <= 3, "{boxes:?}");
    }

    #[test]
    fn the_largest_part_is_kept() {
        let mut v = Vox::new(8, 1, 1);
        for x in [0, 1, 2, 5, 6] {
            v.set((x, 0, 0), 1);
        }
        v.keep_largest(1);
        assert_eq!(v.cells_of(1).len(), 3);
    }
}
