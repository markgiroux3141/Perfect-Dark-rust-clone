//! Rooms of very unequal sizes dropped at seeded random places, before anything joins them.
//! A port of `ge_pd_tools/scatter.py`: the same seed gives the same lots.
//!
//! Lots may overlap in two ways, both kept: a small lot that lands on a tall "host" lot is
//! lifted to one of its upper storeys and becomes a pod (a room that pokes into the hall), and
//! lots a storey apart may sit over one another. Units: x and y in 3 m cells, heights in game
//! units.

use crate::pyrand::PyRandom;
use std::collections::{BTreeMap, VecDeque};

/// Floor to floor.
pub const LEVEL: i32 = 276;
/// Floor to ceiling.
pub const STOREY: i32 = 236;
/// The slab between a ceiling and the floor above.
pub const SLAB: i32 = LEVEL - STOREY;
/// The floor levels a pod may be lifted to (scatter.py's own list, shorter than the router's).
pub const POD_LEVELS: [i32; 5] = [-276, 0, 276, 551, 827];

/// (x0, y0, x1, y1) in cells.
pub type Rect = (i32, i32, i32, i32);

/// One kind of room to scatter: how many, how big, and on which floors.
#[derive(Clone, Debug, PartialEq)]
pub struct Kind {
    pub name: &'static str,
    pub count: usize,
    /// (least, most) cells, the longer side.
    pub long: (i32, i32),
    /// (least, most) cells, the shorter side.
    pub short: (i32, i32),
    /// Floor levels to pick from (one listed twice is twice as likely).
    pub floors: &'static [i32],
    /// Floor-to-ceiling heights to pick from.
    pub heights: &'static [i32],
    /// Tall enough to take pods.
    pub host: bool,
    /// May land in a host, and is lifted into it.
    pub pod: bool,
}

impl Kind {
    pub const fn new(name: &'static str, count: usize, long: (i32, i32), short: (i32, i32), floors: &'static [i32], heights: &'static [i32]) -> Self {
        Kind { name, count, long, short, floors, heights, host: false, pod: false }
    }

    pub const fn host(mut self) -> Self {
        self.host = true;
        self
    }

    pub const fn pod(mut self) -> Self {
        self.pod = true;
        self
    }
}

/// A scattered room: a box on the cell grid with a floor and a top.
#[derive(Clone, Debug, PartialEq)]
pub struct Lot {
    pub key: String,
    pub kind: &'static str,
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub floor: i32,
    pub top: i32,
    /// For a pod: the lot it pokes into. Empty otherwise.
    pub host: String,
}

impl Lot {
    pub fn rect(&self) -> Rect {
        (self.x0, self.y0, self.x1, self.y1)
    }

    pub fn area(&self) -> i32 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }

    pub fn centre(&self) -> (f64, f64) {
        ((self.x0 + self.x1) as f64 / 2.0, (self.y0 + self.y1) as f64 / 2.0)
    }
}

/// The overlap of two rectangles, if they overlap.
pub fn rect_overlap(a: Rect, b: Rect) -> Option<Rect> {
    let (x0, y0, x1, y1) = (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

/// (dx, dy): clear cells between two rectangles along each axis (0 where they overlap).
pub fn rect_gap(a: Rect, b: Rect) -> (i32, i32) {
    (0.max(a.0 - b.2).max(b.0 - a.2), 0.max(a.1 - b.3).max(b.1 - a.3))
}

/// Are two lots clear of each other in height, with a slab between?
pub fn apart(a: &Lot, b: &Lot) -> bool {
    a.floor >= b.top + SLAB || b.floor >= a.top + SLAB
}

/// Are two lots clear of each other in plan: `gap` cells between them along x, or along y?
pub fn beside(a: &Lot, b: &Lot, gap: i32) -> bool {
    let (dx, dy) = rect_gap(a.rect(), b.rect());
    dx >= gap || dy >= gap
}

/// May two lots that are not a hall and its pod stand as they are? They must be clear in plan,
/// or clear in height with a slab between (one over the other, or beside it on another storey).
/// The rule the scatter places by, and that lots changed by hand are held to (`place::Placed`).
pub fn clear(a: &Lot, b: &Lot, gap: i32) -> bool {
    beside(a, b, gap) || apart(a, b)
}

/// Is this one of the kinds tall enough to take pods?
pub fn hosts(kinds: &[Kind], name: &str) -> bool {
    kinds.iter().any(|k| k.host && k.name == name)
}

/// The storeys of a hall a pod may be on: its first two upper ones.
pub fn pod_levels(host: &Lot) -> Vec<i32> {
    let mut out: Vec<i32> = POD_LEVELS.iter().copied().filter(|&z| host.floor < z && z + STOREY <= host.top).take(2).collect();
    if crate::space::towers(host.kind, crate::storeys::snap_top(host.top)) {
        // and a towering hall's high storeys, over the ones ramped inside it (`Space::high`)
        let first = crate::storeys::index(host.floor) + crate::space::ramped(host.kind, host.area());
        let high = crate::storeys::LEVELS.iter().copied().skip(first).filter(|&z| z + STOREY <= host.top);
        out.extend(high.filter(|z| !out.contains(z)).collect::<Vec<_>>());
    }
    out
}

/// Why a pod may not poke into its hall where it overlaps it (`ov`), if it may not: it reaches
/// under `pod_depth` cells in, or takes more than a fifth of the hall's floor.
pub fn pod_misfit(ov: Rect, host: &Lot, pod_depth: i32) -> Option<Misfit> {
    let (ow, od) = (ov.2 - ov.0, ov.3 - ov.1);
    if ow.min(od) < pod_depth {
        return Some(Misfit::Shallow(ow.min(od)));
    }
    ((ow * od) as f64 > 0.2 * host.area() as f64).then_some(Misfit::Big(ow * od))
}

/// Why a pod does not fit its hall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Misfit {
    /// It reaches only this many cells in.
    Shallow(i32),
    /// It takes this many cells of the hall's floor, more than a fifth.
    Big(i32),
}

/// Drop every kind's lots at random places in the site (cells), the kinds in order.
///
/// A lot is kept where it is at least `gap` cells clear of every lot it shares a height with.
/// A pod kind that lands on a host is lifted to one of the host's upper storeys instead, if it
/// reaches at least `pod_depth` cells in and leaves most of the host's floor open. Err names
/// the lot that found no place in `tries` goes.
pub fn scatter(seed: u64, site: (i32, i32), kinds: &[Kind], gap: i32, tries: usize, pod_depth: i32) -> Result<Vec<Lot>, String> {
    let mut rng = PyRandom::from_int(seed);
    let mut lots: Vec<Lot> = Vec::new();
    let (site_w, site_d) = site;
    for k in kinds {
        for i in 0..k.count {
            let mut placed = false;
            for _ in 0..tries {
                let a = rng.randint(k.long.0 as i64, k.long.1 as i64) as i32;
                let b = rng.randint(k.short.0 as i64, k.short.1 as i64) as i32;
                let (w, d) = if rng.random() < 0.5 { (a, b) } else { (b, a) };
                if w > site_w || d > site_d {
                    continue;
                }
                let x = rng.randint(0, (site_w - w) as i64) as i32;
                let y = rng.randint(0, (site_d - d) as i64) as i32;
                let floor = *rng.choice(k.floors);
                let top = floor + *rng.choice(k.heights);
                let key = if k.count > 1 { format!("{}{}", k.name, i + 1) } else { k.name.to_string() };
                let mut lot = Lot { key, kind: k.name, x0: x, y0: y, x1: x + w, y1: y + d, floor, top, host: String::new() };
                if place(&mut lot, &lots, k, gap, pod_depth, &mut rng, kinds) {
                    lots.push(lot);
                    placed = true;
                    break;
                }
            }
            if !placed {
                return Err(format!("no room for {}{} (seed {seed})", k.name, i + 1));
            }
        }
    }
    Ok(lots)
}

/// May this lot stay where it fell? A pod kind on a host is lifted into it here.
fn place(lot: &mut Lot, lots: &[Lot], kind: &Kind, gap: i32, pod_depth: i32, rng: &mut PyRandom, kinds: &[Kind]) -> bool {
    let mut host: Option<(usize, Rect)> = None;
    for (n, o) in lots.iter().enumerate() {
        if beside(lot, o, gap) {
            continue; // clear in plan
        }
        let ov = rect_overlap(lot.rect(), o.rect());
        if let Some(ov) = ov {
            if kind.pod && hosts(kinds, o.kind) && host.is_none() && o.host.is_empty() {
                host = Some((n, ov));
                continue;
            }
        }
        if apart(lot, o) {
            continue; // stacked, or beside it on another storey
        }
        return false;
    }
    let Some((n, ov)) = host else { return true };
    let o = &lots[n];
    if pod_misfit(ov, o, pod_depth).is_some() {
        return false;
    }
    // the host's upper storeys: the first two
    let levels = pod_levels(o);
    if levels.is_empty() {
        return false;
    }
    lot.floor = *rng.choice(&levels);
    lot.top = lot.floor + STOREY;
    lot.host = o.key.clone();
    for (m, q) in lots.iter().enumerate() {
        if m == n {
            continue;
        }
        if !clear(lot, q, gap) {
            return false; // the lifted lot must still clear everything else
        }
    }
    true
}

/// How many doors away from `start` everything it reaches is, over (a, b) edges, in the order
/// a breadth-first walk finds them (`start` itself first, at 0).
pub fn hops<K: Ord + Clone>(edges: &[(K, K)], start: &K) -> Vec<(K, usize)> {
    let mut nb: BTreeMap<&K, Vec<&K>> = BTreeMap::new();
    for (a, b) in edges {
        nb.entry(a).or_default().push(b);
        nb.entry(b).or_default().push(a);
    }
    let mut seen: BTreeMap<&K, usize> = BTreeMap::from([(start, 0)]);
    let mut order: Vec<(K, usize)> = vec![(start.clone(), 0)];
    let mut todo: VecDeque<&K> = VecDeque::from([start]);
    while let Some(k) = todo.pop_front() {
        let far = seen[k] + 1;
        for &n in nb.get(k).map_or(&[][..], |v| v.as_slice()) {
            if !seen.contains_key(n) {
                seen.insert(n, far);
                order.push((n.clone(), far));
                todo.push_back(n);
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hops_count_doors() {
        let edges = [("a", "b"), ("b", "c"), ("c", "d"), ("a", "d"), ("x", "y")];
        assert_eq!(hops(&edges, &"a"), [("a", 0), ("b", 1), ("d", 1), ("c", 2)]);
        assert_eq!(hops(&edges, &"z"), [("z", 0)]);
    }

    #[test]
    fn gaps_and_overlaps() {
        assert_eq!(rect_gap((0, 0, 4, 4), (6, 1, 9, 3)), (2, 0));
        assert_eq!(rect_gap((6, 9, 9, 12), (0, 0, 4, 4)), (2, 5));
        assert_eq!(rect_overlap((0, 0, 4, 4), (2, 3, 9, 9)), Some((2, 3, 4, 4)));
        assert_eq!(rect_overlap((0, 0, 4, 4), (4, 0, 9, 9)), None);
    }
}
