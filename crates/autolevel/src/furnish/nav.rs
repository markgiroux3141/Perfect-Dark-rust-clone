//! Where a player can stand in a room and how its doors reach one another. From `furnish.py`
//! (`stand`, `blocks`, `nav`, `targets`, `door_nodes`, `connected`, `path`, `waypoints`).
//!
//! A player's middle can be wherever a 2 x 2 h block (a one-cell square, 118 across) is all
//! standing room. Those positions, with the ramps joining storeys, are the room's nav graph.

use super::{mid_block, Interior};
use crate::cells::{rect_cells, Cells};
use crate::py;
use crate::space::{door_pad, DIRS};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// A nav node: the (x, y) of a 2 x 2 h block's low corner, and its storey.
pub type Node = (i32, i32, i32);
/// A room's nav graph: each node's neighbours, in the order they are tried.
pub type Nav = HashMap<Node, Vec<Node>>;

impl Interior {
    /// Cells of storey `z` a player can be on: floor there, nothing on it, headroom.
    pub fn stand(&self, z: i32) -> Cells {
        let mut out = self.walk(z);
        for r in self.ramps.iter().filter(|r| r.lo == z) {
            for c in r.cells() {
                out.remove(&c);
            }
        }
        if z == self.levels[0] {
            for &(x0, y0, x1, y1, _, _) in &self.cover {
                for c in rect_cells((x0, y0, x1, y1)) {
                    out.remove(&c);
                }
            }
        }
        for &(x0, y0, x1, y1, a, b) in &self.piers {
            // a pod's pier stands through every deck below the pod
            if a <= z && z < b {
                for c in rect_cells((x0, y0, x1, y1)) {
                    out.remove(&c);
                }
            }
        }
        out
    }

    /// Positions (x, y) whose 2 x 2 h block is all standing room: where a player's middle can be.
    pub fn blocks(&self, z: i32) -> Cells {
        let st = self.stand(z);
        st.iter().filter(|c| st.contains(&(c.0 + 1, c.1)) && st.contains(&(c.0, c.1 + 1)) && st.contains(&(c.0 + 1, c.1 + 1))).copied().collect()
    }

    /// Every storey's blocks.
    fn all_blocks(&self) -> BTreeMap<i32, Cells> {
        self.levels.iter().map(|&z| (z, self.blocks(z))).collect()
    }

    /// Each node's neighbours over (x, y, z) block positions, with ramps joining storeys.
    pub fn nav(&self) -> Nav {
        self.nav_over(&self.all_blocks())
    }

    /// `nav` over blocks already worked out.
    fn nav_over(&self, blocks: &BTreeMap<i32, Cells>) -> Nav {
        let mut g = Nav::new();
        for (&z, bs) in blocks {
            for c in bs {
                g.insert((c.0, c.1, z), DIRS.iter().map(|(dx, dy)| (c.0 + dx, c.1 + dy)).filter(|n| bs.contains(n)).map(|n| (n.0, n.1, z)).collect());
            }
        }
        for r in &self.ramps {
            let (t, f) = (r.node(true), r.node(false));
            let (a, b) = ((t.0, t.1, r.hi), (f.0, f.1, r.lo));
            if g.contains_key(&a) && g.contains_key(&b) {
                g.get_mut(&a).expect("just seen").push(b);
                g.get_mut(&b).expect("just seen").push(a);
            }
        }
        g
    }

    /// (cells, storey): the floor just inside every door, and outside every pod's door.
    pub fn targets(&self) -> Vec<(Cells, i32)> {
        let mut out: Vec<(Cells, i32)> = self.doors.iter().map(|d| (door_pad(&self.s, d, self.band, 0), d.level)).collect();
        for p in &self.pods {
            if let Some(acc) = self.access_of(&p.key) {
                out.push((acc.pad.clone(), p.floor));
            }
        }
        out
    }

    /// One nav node at each target (None where there is no standing room).
    pub fn door_nodes(&self) -> Vec<Option<Node>> {
        self.door_nodes_over(&self.all_blocks())
    }

    /// `door_nodes` over blocks already worked out.
    fn door_nodes_over(&self, blocks: &BTreeMap<i32, Cells>) -> Vec<Option<Node>> {
        let none = Cells::new();
        self.targets()
            .into_iter()
            .map(|(pad, z)| {
                let mid = mid_block(&pad);
                let here = blocks.get(&z).unwrap_or(&none);
                py::first_min(pad.iter().filter(|c| here.contains(c)).map(|c| (c.0, c.1, z)), |c| ((c.0 - mid.0).abs() + (c.1 - mid.1).abs(), *c))
            })
            .collect()
    }

    /// Does every door (and every pod's door) reach every other on foot?
    pub fn connected(&self) -> bool {
        let blocks = self.all_blocks();
        let nodes: Vec<Node> = match self.door_nodes_over(&blocks).into_iter().collect::<Option<Vec<Node>>>() {
            Some(nodes) => nodes,
            None => return false,
        };
        if nodes.len() < 2 {
            return true;
        }
        let g = self.nav_over(&blocks);
        let mut seen: HashSet<Node> = HashSet::from([nodes[0]]);
        let mut todo = VecDeque::from([nodes[0]]);
        while let Some(at) = todo.pop_front() {
            for &n in &g[&at] {
                if seen.insert(n) {
                    todo.push_back(n);
                }
            }
        }
        nodes.iter().all(|n| seen.contains(n))
    }

    /// Nav nodes from a to b, the fewest there can be, or None.
    pub fn path(&self, a: Node, b: Node) -> Option<Vec<Node>> {
        Self::path_in(&self.nav(), a, b)
    }

    /// `path` over a nav graph already worked out.
    pub fn path_in(g: &Nav, a: Node, b: Node) -> Option<Vec<Node>> {
        let mut came: HashMap<Node, Option<Node>> = HashMap::from([(a, None)]);
        let mut todo = VecDeque::from([a]);
        while let Some(n) = todo.pop_front() {
            if n == b {
                let mut out = vec![n];
                let mut at = n;
                while let Some(before) = came[&at] {
                    out.push(before);
                    at = before;
                }
                out.reverse();
                return Some(out);
            }
            for &m in g.get(&n).map_or(&[][..], |v| v.as_slice()) {
                if let std::collections::hash_map::Entry::Vacant(e) = came.entry(m) {
                    e.insert(Some(n));
                    todo.push_back(m);
                }
            }
        }
        None
    }

    /// Nav nodes as points to walk, (x cells, y cells, z): corners only, cut straight where the
    /// way between two points is clear.
    pub fn waypoints(&self, nodes: &[Node]) -> Vec<(f64, f64, i32)> {
        if nodes.is_empty() {
            return Vec::new();
        }
        let blocks = self.all_blocks();
        let clear = |p: Node, q: Node| {
            if p.2 != q.2 {
                return false;
            }
            let here = &blocks[&p.2];
            let n = 1.max(2 * (q.0 - p.0).abs().max((q.1 - p.1).abs()));
            (0..=n).all(|i| {
                let x = p.0 as f64 + ((q.0 - p.0) * i) as f64 / n as f64;
                let y = p.1 as f64 + ((q.1 - p.1) * i) as f64 / n as f64;
                [x.floor(), x.ceil()].iter().all(|cx| [y.floor(), y.ceil()].iter().all(|cy| here.contains(&(*cx as i32, *cy as i32))))
            })
        };
        let mut keep = vec![nodes[0]];
        let mut i = 0;
        while i < nodes.len() - 1 {
            let mut j = nodes.len() - 1;
            while j > i + 1 && !clear(nodes[i], nodes[j]) {
                j -= 1;
            }
            keep.push(nodes[j]);
            i = j;
        }
        keep.iter().map(|n| ((n.0 + 1) as f64 / 2.0, (n.1 + 1) as f64 / 2.0, n.2)).collect()
    }
}
