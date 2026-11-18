//! Where a player can be in the sculpt planner's grid, and where they can get to.
//!
//! A cell is **walkable** if it is air with solid under it and four cells of air over it (236, a
//! corridor's height: with three, 177, the game's player stops short of a 172 player's room),
//! and no ramp sheet passes over it. Walkable cells of one zone at one
//! height that touch are a **region** (a floor, a platform, a ledge). From a region a player
//! walks into a touching one of another zone at the same height (a doorway), **drops** off an
//! edge into whatever is below (one way: a player can't jump), or goes up or down a **sheet** (a
//! ramp: both ways).
//!
//! ```text
//! let nav = Nav::build(&vox, &sheets, None);
//! let scc = nav.components();      // each region's strongly connected component
//! nav.path(a, b)                   // cells from a to b, with how each was reached
//! ```

use super::vox::{add, up, Ix, Vox};
use std::collections::{BTreeSet, BinaryHeap, HashMap, VecDeque};

/// Air a player needs over a floor, cells.
pub const STAND: i32 = 4;

const ACROSS: [Ix; 4] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0)];

/// A ramp: a sloped sheet from one face of a run of cells to another, two cells wide.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub zone: u16,
    /// The two lanes' cells just off its low end, at the low floor, and just off its high end.
    pub foot: [Ix; 2],
    pub top: [Ix; 2],
    /// The columns it passes over, with the highest cell it passes through there: a cell of the
    /// column at or under that is not walkable.
    pub cover: Vec<(i32, i32, i32)>,
    /// The sheet for the plan: its centre line, low end then high, as grid lines (x and y: the
    /// faces between cells; z: the cell whose floor it is level with).
    pub start: (i32, i32, i32),
    pub end: (i32, i32, i32),
    /// Whether it stands in a room (open, with rails) or a corridor (between walls).
    pub open: bool,
}

/// How a cell of a path was reached from the one before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Start,
    Walk,
    Drop,
    Ramp,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub zone: u16,
    pub k: i32,
    pub cells: Vec<Ix>,
}

impl Region {
    /// The cell nearest the region's middle.
    pub fn middle(&self) -> Ix {
        let n = self.cells.len() as f64;
        let (mx, my) = (self.cells.iter().map(|c| c.0 as f64).sum::<f64>() / n, self.cells.iter().map(|c| c.1 as f64).sum::<f64>() / n);
        *self
            .cells
            .iter()
            .min_by(|a, b| {
                let d = |c: &&Ix| (c.0 as f64 - mx).powi(2) + (c.1 as f64 - my).powi(2);
                d(a).total_cmp(&d(b))
            })
            .expect("a region has a cell")
    }
}

/// The walkable cells, their regions and the ways between them.
#[derive(Clone, Debug)]
pub struct Nav {
    pub region_of: HashMap<Ix, usize>,
    pub regions: Vec<Region>,
    /// Region -> the regions one move takes a player to.
    pub next: Vec<BTreeSet<usize>>,
    /// Cell -> (cell, how) one move takes a player to.
    links: HashMap<Ix, Vec<(Ix, Move, u32)>>,
}

/// Whether a cell is walkable, sheets aside.
pub fn standable(v: &Vox, p: Ix) -> bool {
    v.air(p) && !v.air(up(p, -1)) && (1..STAND).all(|k| v.air(up(p, k)))
}

impl Nav {
    /// The nav of the grid, or of one zone's air only, within a box (low corner, high corner
    /// exclusive).
    pub fn build(v: &Vox, sheets: &[Sheet], scope: Option<(u16, Ix, Ix)>) -> Nav {
        let zone = scope.map(|s| s.0);
        let covered: HashMap<(i32, i32), i32> = {
            let mut m = HashMap::new();
            for s in sheets {
                for &(x, y, k) in &s.cover {
                    let e = m.entry((x, y)).or_insert(i32::MIN);
                    *e = (*e).max(k);
                }
            }
            m
        };
        let ok_zone = |p: Ix| zone.is_none_or(|z| v.get(p) == z);
        let walkable = |p: Ix| ok_zone(p) && standable(v, p) && covered.get(&(p.0, p.1)).is_none_or(|&k| p.2 > k);
        let cells: Vec<Ix> = match scope {
            Some((z, lo, hi)) => {
                (lo.2..hi.2).flat_map(|k| (lo.1..hi.1).flat_map(move |y| (lo.0..hi.0).map(move |x| (x, y, k)))).filter(|&p| v.get(p) == z).collect()
            }
            None => v.each().filter(|&p| v.air(p)).collect(),
        };
        let mut region_of: HashMap<Ix, usize> = HashMap::new();
        let mut regions: Vec<Region> = Vec::new();
        for &c in &cells {
            if region_of.contains_key(&c) || !walkable(c) {
                continue;
            }
            let id = regions.len();
            let z = v.get(c);
            let mut part = Vec::new();
            let mut q = VecDeque::from([c]);
            region_of.insert(c, id);
            while let Some(p) = q.pop_front() {
                part.push(p);
                for d in ACROSS {
                    let n = add(p, d);
                    if v.get(n) == z && !region_of.contains_key(&n) && walkable(n) {
                        region_of.insert(n, id);
                        q.push_back(n);
                    }
                }
            }
            regions.push(Region { zone: z, k: c.2, cells: part });
        }
        let mut links: HashMap<Ix, Vec<(Ix, Move, u32)>> = HashMap::new();
        for &c in region_of.keys() {
            let mut out = Vec::new();
            for d in ACROSS {
                let n = add(c, d);
                if region_of.contains_key(&n) {
                    out.push((n, Move::Walk, 2));
                    continue;
                }
                // off an edge: room to move into the cell, and nothing under it
                if !(0..STAND).all(|k| v.air(up(n, k))) || !v.air(up(n, -1)) || !ok_zone(n) {
                    continue;
                }
                let mut m = up(n, -1);
                while v.air(up(m, -1)) {
                    m = up(m, -1);
                }
                // a sheet in the way is landed on, not fallen past
                let onto_sheet = covered.get(&(n.0, n.1)).is_some_and(|&k| k >= m.2);
                if region_of.contains_key(&m) && !onto_sheet {
                    out.push((m, Move::Drop, 3));
                }
            }
            links.insert(c, out);
        }
        for s in sheets {
            for lane in 0..2 {
                let (f, t) = (s.foot[lane], s.top[lane]);
                if region_of.contains_key(&f) && region_of.contains_key(&t) {
                    let len = ((f.0 - t.0).abs() + (f.1 - t.1).abs()) as u32 * 2;
                    links.entry(f).or_default().push((t, Move::Ramp, len));
                    links.entry(t).or_default().push((f, Move::Ramp, len));
                }
            }
        }
        let mut next = vec![BTreeSet::new(); regions.len()];
        for (c, out) in &links {
            let a = region_of[c];
            for (n, _, _) in out {
                let b = region_of[n];
                if a != b {
                    next[a].insert(b);
                }
            }
        }
        Nav { region_of, regions, next, links }
    }

    /// Each region's strongly connected component (Tarjan's), numbered from 0.
    pub fn components(&self) -> Vec<usize> {
        let n = self.regions.len();
        let (mut index, mut low, mut on) = (vec![usize::MAX; n], vec![0; n], vec![false; n]);
        let mut comp = vec![usize::MAX; n];
        let (mut stack, mut counter, mut comps) = (Vec::new(), 0, 0);
        let next: Vec<Vec<usize>> = self.next.iter().map(|s| s.iter().copied().collect()).collect();
        for root in 0..n {
            if index[root] != usize::MAX {
                continue;
            }
            let mut work = vec![(root, 0usize)];
            index[root] = counter;
            low[root] = counter;
            counter += 1;
            stack.push(root);
            on[root] = true;
            while let Some(&mut (v, ref mut i)) = work.last_mut() {
                if *i < next[v].len() {
                    let w = next[v][*i];
                    *i += 1;
                    if index[w] == usize::MAX {
                        index[w] = counter;
                        low[w] = counter;
                        counter += 1;
                        stack.push(w);
                        on[w] = true;
                        work.push((w, 0));
                    } else if on[w] {
                        low[v] = low[v].min(index[w]);
                    }
                } else {
                    work.pop();
                    if let Some(&(u, _)) = work.last() {
                        low[u] = low[u].min(low[v]);
                    }
                    if low[v] == index[v] {
                        loop {
                            let w = stack.pop().expect("on the stack");
                            on[w] = false;
                            comp[w] = comps;
                            if w == v {
                                break;
                            }
                        }
                        comps += 1;
                    }
                }
            }
        }
        comp
    }

    /// The cheapest way from cell `a` to cell `b`: each cell with how it was reached. None if
    /// there is none (or either is not walkable).
    pub fn path(&self, a: Ix, b: Ix) -> Option<Vec<(Ix, Move)>> {
        if !self.region_of.contains_key(&a) || !self.region_of.contains_key(&b) {
            return None;
        }
        let mut best: HashMap<Ix, (u32, Ix, Move)> = HashMap::from([(a, (0, a, Move::Start))]);
        let mut heap = BinaryHeap::from([(std::cmp::Reverse(0u32), a)]);
        while let Some((std::cmp::Reverse(g), c)) = heap.pop() {
            if c == b {
                break;
            }
            if best.get(&c).is_some_and(|e| e.0 < g) {
                continue;
            }
            for &(n, how, cost) in self.links.get(&c).map(|v| &v[..]).unwrap_or(&[]) {
                let ng = g + cost;
                if best.get(&n).is_none_or(|e| ng < e.0) {
                    best.insert(n, (ng, c, how));
                    heap.push((std::cmp::Reverse(ng), n));
                }
            }
        }
        best.get(&b)?;
        let mut out = vec![];
        let mut c = b;
        loop {
            let (_, from, how) = best[&c];
            out.push((c, how));
            if how == Move::Start {
                break;
            }
            c = from;
        }
        out.reverse();
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room 10 x 4, 8 tall, with a platform 4 up over its east half.
    fn room() -> Vox {
        let mut v = Vox::new(12, 6, 10);
        for p in v.each().collect::<Vec<_>>() {
            let inside = (1..11).contains(&p.0) && (1..5).contains(&p.1) && (1..9).contains(&p.2);
            let platform = p.0 >= 6 && p.2 < 5;
            if inside && !platform {
                v.set(p, 1);
            }
        }
        v
    }

    #[test]
    fn a_platform_is_dropped_from_but_not_climbed() {
        let v = room();
        let nav = Nav::build(&v, &[], None);
        assert_eq!(nav.regions.len(), 2);
        let (floor, deck) = (nav.region_of[&(2, 2, 1)], nav.region_of[&(8, 2, 5)]);
        assert!(nav.next[deck].contains(&floor) && !nav.next[floor].contains(&deck));
        let comp = nav.components();
        assert_ne!(comp[floor], comp[deck]);
        assert!(nav.path((2, 2, 1), (8, 2, 5)).is_none());
        assert!(nav.path((8, 2, 5), (2, 2, 1)).is_some());
    }

    #[test]
    fn a_sheet_joins_them_both_ways() {
        let v = room();
        // a ramp up the west side of the platform's edge: from x 1 to 5, in lanes y 1 and 2
        let sheet = Sheet {
            zone: 1,
            foot: [(1, 1, 1), (1, 2, 1)],
            top: [(6, 1, 5), (6, 2, 5)],
            cover: (2..6).flat_map(|x| [(x, 1, 5), (x, 2, 5)]).collect(),
            start: (0, 0, 0),
            end: (0, 0, 0),
            open: true,
        };
        let nav = Nav::build(&v, &[sheet], None);
        let comp = nav.components();
        assert_eq!(comp[nav.region_of[&(2, 3, 1)]], comp[nav.region_of[&(8, 2, 5)]]);
        let path = nav.path((2, 4, 1), (9, 4, 5)).unwrap();
        assert!(path.iter().any(|(_, m)| *m == Move::Ramp));
    }
}
