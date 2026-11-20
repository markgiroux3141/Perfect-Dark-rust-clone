//! The rescue: a room no set of ramps can join, on any attempt, furnished once more with ladders
//! and bridges. New in the Rust (the Python gives up there, and so did the port: "no set of ramps
//! joins its storeys"), and only ever tried after every attempt without it has failed
//! (`Build::furnish`), so a room that could be furnished before is furnished as before.
//!
//! What it does (`Setup::rescue`):
//!
//! - **a door onto a pod's roof** that no deck touches: that roof is made a floor (`land_roofs`);
//! - **an island pod's pier** through where a door is stepped through is left out (the pod
//!   floats: `Interior::build`);
//! - **a piece of deck no ramp can arrive at** (a pool or pods under it, no wall to run a gangway
//!   on to, or the tries run out): the ramps are laid past it (`ramps::solve`, `strand`), though
//!   never so that a door is left nowhere to stand;
//! - **then the pieces are stitched** (`stitch`): while some door (or pod's door) does not reach
//!   the others of its band, the cheapest join from the piece the first door is on: a **chute**,
//!   a ladder a cell wide up the edge of a deck from a storey below (`Chute`), or a **bridge**, deck
//!   a cell wide across the air of one storey, straight where it can be and round what stands in
//!   the way (`Way`). Then every piece of deck upstairs no door reaches is joined the same way if
//!   it can be (else it stays as it is). A high storey of a towering hall is a band of its own: it
//!   is bridged, and only if no bridge can join it (a pod as wide as the storey cuts its catwalk in
//!   two) is a chute put up to it from the hall: there, its climb from outside is not the only way.
//!
//! A chute stands clear of ramps and of where a door is stepped through on every storey it passes,
//! with a second block of floor straight out from its foot to step off onto. Its foot is kept clear
//! (`Interior::keep`: no cover, no platform or angled catwalk in its column), nobody walks through
//! the ladder where it stands (`nav::stand`), a flooded floor's walkways reach it (`flood`), its
//! top's rail is left open (`rails`), and the nav joins its two ends both ways (`nav`). The level
//! builds it on the deck's edge (`geometry::chute_ladder`), and walks it up and down
//! (`chute_<room>_<n>`, `chutedown_<room>_<n>`; every other walk down it stops at its lip first).

use super::{components, Interior, Node};
use crate::cells::{rect_cells, Cell, Cells};
use crate::space::DIRS;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, VecDeque};

/// The longest bridge, steps of a half cell (thirty-two cells), and what a turn in it costs, steps.
const BRIDGE_MAX: i32 = 64;
const TURN: i32 = 6;
/// What a chute costs, for each storey it climbs, against a bridge's new half cells.
const CHUTE_COST: usize = 24;
/// Joins tried before the room is given up.
const ROUNDS: usize = 40;

/// A ladder up the edge of a deck from a storey below, put up by the rescue (`stitch`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chute {
    /// The block it is climbed from, on the storey below, and the block on the deck it comes up
    /// onto (nav nodes: the foot is the top's neighbour, two half cells out past the deck's edge).
    pub foot: Node,
    pub top: Node,
    /// The way from the top to the foot: out of the deck (an index into `DIRS`).
    pub side: usize,
}

impl Chute {
    /// The ladder's two ends along the deck's edge, in cells (a cell wide: the edge of the top's
    /// block), and the way it faces (out of the deck).
    pub fn line(&self) -> ((f64, f64), (f64, f64), (i32, i32)) {
        let (dx, dy) = DIRS[self.side];
        let (x, y) = (self.top.0, self.top.1);
        let h = |v: i32| v as f64 / 2.0;
        let (ex, ey) = (if dx > 0 { x + 2 } else { x }, if dy > 0 { y + 2 } else { y });
        if dx != 0 {
            ((h(ex), h(y)), (h(ex), h(y + 2)), (dx, 0))
        } else {
            ((h(x), h(ey)), (h(x + 2), h(ey)), (0, dy))
        }
    }

    /// Where it is climbed from and where it ends: the middles of its foot's block and its top's,
    /// cells.
    pub fn ends(&self) -> ((f64, f64, i32), (f64, f64, i32)) {
        let m = |n: Node| ((n.0 + 1) as f64 / 2.0, (n.1 + 1) as f64 / 2.0, n.2);
        (m(self.foot), m(self.top))
    }

    /// The deck's edge at its top: each half cell there, with the way out to the ladder (a rail
    /// is not laid across it).
    pub fn lip(&self) -> Vec<(Cell, usize)> {
        let (dx, dy) = DIRS[self.side];
        let foot = block(self.foot);
        block(self.top).into_iter().filter(|c| foot.contains(&(c.0 + dx, c.1 + dy))).map(|c| (c, self.side)).collect()
    }
}

/// A node's block: the 2 x 2 half cells from its low corner.
fn block(n: Node) -> Cells {
    rect_cells((n.0, n.1, n.0 + 2, n.1 + 2))
}

/// A way to join two pieces of the room.
#[derive(Clone, Debug)]
enum Join {
    /// New deck on storey z (on the floor: over the pool).
    Bridge(i32, Cells),
    Chute(Chute),
}

impl Interior {
    /// A door (or a pod's) that opens onto a pod's roof no deck touches: that roof is made a floor
    /// (`landed`), or there is nowhere to stand inside the door.
    pub(super) fn land_roofs(&mut self) {
        for (pad, z) in self.targets() {
            if z == self.levels[0] || !self.roof.contains_key(&z) || pad.is_subset(&self.walk(z)) {
                continue;
            }
            let roof = &self.roof[&z];
            if let Some(piece) = components(roof).into_iter().find(|p| !p.is_disjoint(&pad)) {
                self.landed.entry(z).or_default().extend(piece);
            }
        }
    }

    /// Joins the room's pieces with chutes and bridges until every door reaches every other of its
    /// band (Err if they can't be), then every piece of deck left upstairs that can be.
    pub(super) fn stitch(&mut self) -> Result<(), String> {
        let key = self.s.key.clone();
        let refused = || format!("{key}: no set of ramps joins its storeys");
        // the doors first
        for round in 0.. {
            let blocks = self.all_blocks();
            let g = self.nav_over(&blocks);
            let (piece, pieces) = pieces(&g);
            let doors = self.door_nodes_over(&blocks).into_iter().collect::<Option<Vec<Node>>>().ok_or_else(refused)?;
            // each band's doors' pieces, the first door's first
            let mut bands: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
            for n in &doors {
                let ps = bands.entry(self.band_of(n.2)).or_default();
                if !ps.contains(&piece[n]) {
                    ps.push(piece[n]);
                }
            }
            let Some((&band, ps)) = bands.iter().find(|(_, ps)| ps.len() > 1) else { break };
            if round == ROUNDS {
                return Err(refused());
            }
            let (from, want) = (ps[0], ps[1..].to_vec());
            // straight to a door's piece if it can be, else to any other piece of the band, a step
            // on; and last, the same by a chute to or from another band (a pod as wide as a high
            // storey cuts its catwalk in two: each end is then climbed to from the hall)
            let others = |band: Option<i32>| -> Vec<usize> {
                (0..pieces.len()).filter(|&i| i != from && band.is_none_or(|b| self.band_of(pieces[i][0].2) == b)).collect()
            };
            let join = [Some(band), None].into_iter().find_map(|b| {
                self.best_join(&blocks, &piece, &pieces[from], &want, b).or_else(|| self.best_join(&blocks, &piece, &pieces[from], &others(b), b))
            });
            self.join(join.ok_or_else(refused)?);
        }
        // then each piece of deck upstairs no door reaches (in a band with doors), if it can be
        let mut tried: BTreeSet<Node> = BTreeSet::new(); // (a piece, by its least node)
        for _ in 0..ROUNDS {
            let blocks = self.all_blocks();
            let g = self.nav_over(&blocks);
            let (piece, pieces) = pieces(&g);
            let doors: Vec<Node> = self.door_nodes_over(&blocks).into_iter().flatten().collect();
            let reached: BTreeSet<usize> = doors.iter().map(|n| piece[n]).collect();
            let lone = pieces.iter().enumerate().find(|(i, p)| {
                !reached.contains(i) && p[0].2 != self.levels[0] && !tried.contains(&p[0]) && p.iter().any(|n| self.deck[&n.2].contains(&(n.0, n.1)))
            });
            let Some((i, p)) = lone else { break };
            tried.insert(p[0]);
            let band = self.band_of(p[0].2);
            let Some(from) = doors.iter().find(|n| self.band_of(n.2) == band).map(|n| piece[n]) else { continue };
            if let Some(j) = self.best_join(&blocks, &piece, &pieces[from], &[i], Some(band)) {
                self.join(j);
            }
        }
        Ok(())
    }

    fn join(&mut self, j: Join) {
        if let Join::Bridge(z, cells) = &j {
            self.crossings.push((*z, cells.clone()));
        }
        match j {
            Join::Bridge(z, cells) if z == self.levels[0] => self.bridge.extend(cells),
            Join::Bridge(z, cells) => self.deck.get_mut(&z).expect("a storey of the room").extend(cells),
            Join::Chute(c) => {
                self.keep.push((c.foot.0, c.foot.1, c.foot.0 + 2, c.foot.1 + 2));
                self.chutes.push(c);
            }
        }
    }

    /// The cheapest join from the piece `from` to any of the pieces `to`, in band `band` (None: a
    /// chute between any): a bridge (its new half cells) or a chute (`CHUTE_COST` a storey), ties
    /// broken by place. A chute stands clear of where a door is stepped through on every storey it
    /// passes, and has room to step off at its foot.
    fn best_join(&self, blocks: &BTreeMap<i32, Cells>, piece: &HashMap<Node, usize>, from: &[Node], to: &[usize], band: Option<i32>) -> Option<Join> {
        let within = |z: i32| band.is_none_or(|b| self.band_of(z) == b);
        let pads = self.targets();
        let to: BTreeSet<usize> = to.iter().copied().collect();
        let mine = piece[&from[0]];
        let walk: BTreeMap<i32, Cells> = self.levels.iter().map(|&z| (z, self.walk(z))).collect();
        let open = self.open_air(&walk);
        let mut best: Option<((usize, Node, Node), Join)> = None;
        let mut offer = |key: (usize, Node, Node), j: Join| {
            if best.as_ref().is_none_or(|(k, _)| key < *k) {
                best = Some((key, j));
            }
        };
        // bridges: the shortest way, a block wide, over the air of a storey from this piece to one
        // of those
        let storeys: BTreeSet<i32> = from.iter().map(|n| n.2).collect();
        for z in storeys {
            let stand = self.stand(z);
            let way = Way { z, here: &blocks[&z], piece, walk: &walk[&z], stand: &stand, open: &open[&z] };
            if let Some((cells, a, b)) = way.shortest(from, mine, &to).filter(|(cells, _, _)| !cells.is_empty()) {
                offer((cells.len(), a, b), Join::Bridge(z, cells));
            }
        }
        // chutes: from a block on a storey below, up past the edge of a deck to a block on it
        let ramps: Vec<(i32, i32, Cells)> = self.ramps.iter().map(|r| (r.lo, r.hi, r.cells())).collect();
        let mut tops: Vec<Node> =
            piece.iter().filter(|(n, &i)| (i == mine || to.contains(&i)) && n.2 != self.levels[0] && within(n.2)).map(|(n, _)| *n).collect();
        tops.sort();
        for t in tops {
            let ti = piece[&t];
            for (d, (dx, dy)) in DIRS.iter().enumerate() {
                let (fx, fy) = (t.0 + 2 * dx, t.1 + 2 * dy);
                let column = block((fx, fy, 0));
                if !column.is_subset(&open[&t.2]) {
                    continue; // (no edge here, or something beside it)
                }
                let lip: Cells = Chute { foot: (fx, fy, 0), top: t, side: d }.lip().into_iter().map(|(c, _)| c).collect();
                for &lo in self.levels.iter().filter(|&&z| z < t.2 && within(z)).rev() {
                    let f = (fx, fy, lo);
                    if blocks[&lo].contains(&(fx, fy)) {
                        let i = piece[&f];
                        let joins = (ti == mine && to.contains(&i)) || (i == mine && to.contains(&ti));
                        let ramped = ramps.iter().any(|(rl, rh, rc)| *rl < t.2 && *rh > lo && !rc.is_disjoint(&column));
                        // room to step off its foot: a second block straight on out from it
                        let off = blocks[&lo].contains(&(fx + 2 * dx, fy + 2 * dy));
                        let doorway = pads.iter().any(|(pad, z)| lo <= *z && *z < t.2 && !pad.is_disjoint(&lip));
                        if joins && !ramped && off && !doorway {
                            let storeys = self.levels.iter().filter(|&&z| lo < z && z <= t.2).count();
                            offer((CHUTE_COST * storeys, f, t), Join::Chute(Chute { foot: f, top: t, side: d }));
                        }
                        break; // (a floor under it: no lower storey is reached past it)
                    }
                    if !column.is_subset(&open[&lo]) {
                        break;
                    }
                }
            }
        }
        best.map(|(_, j)| j)
    }

    /// By storey, the half cells of open air a bridge may be laid in and a chute climbs through:
    /// in the room, not floor already, and clear of pods, roofs, piers, ramps, angled catwalks,
    /// the hole and what is kept clear. On the floor, only the pool (a bridge goes over it).
    fn open_air(&self, walk: &BTreeMap<i32, Cells>) -> BTreeMap<i32, Cells> {
        let mut kept: Cells = self.hole.clone();
        for &k in &self.keep {
            kept.extend(rect_cells(k));
        }
        let mut out = BTreeMap::new();
        for &z in &self.levels {
            let mut open: Cells = if z == self.levels[0] { self.pool.intersection(&self.room).copied().collect() } else { self.room.clone() };
            let mut shut: Cells = kept.union(&walk[&z]).copied().collect();
            if z != self.levels[0] {
                shut.extend(self.shell[&z].iter().copied());
                shut.extend(self.roof[&z].iter().copied());
                shut.extend(self.spanned(z));
                for p in self.piers.iter().filter(|p| p.4 <= z && z < p.5) {
                    shut.extend(rect_cells((p.0, p.1, p.2, p.3)));
                }
                for r in self.ramps.iter().filter(|r| r.lo <= z && z <= r.hi) {
                    shut.extend(r.cells());
                }
            }
            open.retain(|c| !shut.contains(c));
            out.insert(z, open);
        }
        out
    }
}

/// One storey, as a bridge's way across it sees it (`Interior::best_join`).
struct Way<'a> {
    z: i32,
    /// The storey's blocks, each node's piece, its floor, where on it a player can stand, and its
    /// open air (`open_air`).
    here: &'a Cells,
    piece: &'a HashMap<Node, usize>,
    walk: &'a Cells,
    stand: &'a Cells,
    open: &'a Cells,
}

impl Way<'_> {
    /// The best bridge from a block of `from` (piece `mine`) to a block of one of the pieces `to`:
    /// blocks over standing room or open air, one step to the next, at most `BRIDGE_MAX` of them, never
    /// through another piece, the fewest steps with `TURN` more for each turn (so it runs straight,
    /// and round what stands in the way in as few legs as it can). (Its new half cells, the block
    /// it leaves, the block it reaches.)
    fn shortest(&self, from: &[Node], mine: usize, to: &BTreeSet<usize>) -> Option<(Cells, Node, Node)> {
        type State = ((i32, i32), usize); // (block, the way it was stepped into: 4 for none)
        type Todo = Reverse<(i32, i32, (i32, i32), usize)>; // (cost, steps, block, way), cheapest first
        let z = self.z;
        let mut came: HashMap<State, Option<State>> = HashMap::new();
        let mut cost: HashMap<State, i32> = HashMap::new();
        let mut todo: BinaryHeap<Todo> = BinaryHeap::new();
        for n in from.iter().filter(|n| n.2 == z) {
            let st = ((n.0, n.1), 4);
            came.insert(st, None);
            cost.insert(st, 0);
            todo.push(Reverse((0, 0, (n.0, n.1), 4)));
        }
        while let Some(Reverse((c, steps, p, way))) = todo.pop() {
            if cost.get(&(p, way)).is_some_and(|&best| c > best) {
                continue;
            }
            if self.here.contains(&p) && steps > 0 {
                let i = self.piece[&(p.0, p.1, z)];
                if i != mine && to.contains(&i) {
                    // back along the way to where it left this piece
                    let mut cells = Cells::new();
                    let mut at = came[&(p, way)].expect("a step was taken");
                    while let Some(prev) = came[&at] {
                        cells.extend(block((at.0 .0, at.0 .1, z)).into_iter().filter(|c| !self.walk.contains(c)));
                        at = prev;
                    }
                    return Some((cells, (at.0 .0, at.0 .1, z), (p.0, p.1, z)));
                }
            }
            if steps >= BRIDGE_MAX {
                continue;
            }
            for (d, (dx, dy)) in DIRS.iter().enumerate() {
                let q = (p.0 + dx, p.1 + dy);
                if self.here.contains(&q) {
                    let i = self.piece[&(q.0, q.1, z)];
                    if i == mine || !to.contains(&i) {
                        continue; // (back into its own piece, or through another)
                    }
                } else if !block((q.0, q.1, z)).iter().all(|c| self.stand.contains(c) || self.open.contains(c)) {
                    continue;
                }
                let qc = c + 1 + if way != 4 && way != d { TURN } else { 0 };
                if cost.get(&(q, d)).is_none_or(|&best| qc < best) {
                    cost.insert((q, d), qc);
                    came.insert((q, d), Some((p, way)));
                    todo.push(Reverse((qc, steps + 1, q, d)));
                }
            }
        }
        None
    }
}

/// The nav graph's pieces: each node's, and each piece's nodes (least first), numbered in the
/// order of their least node.
fn pieces(g: &HashMap<Node, Vec<Node>>) -> (HashMap<Node, usize>, Vec<Vec<Node>>) {
    let mut order: Vec<Node> = g.keys().copied().collect();
    order.sort();
    let mut piece: HashMap<Node, usize> = HashMap::new();
    let mut out: Vec<Vec<Node>> = Vec::new();
    for s in order {
        if piece.contains_key(&s) {
            continue;
        }
        let i = out.len();
        let mut nodes = vec![s];
        piece.insert(s, i);
        let mut todo = VecDeque::from([s]);
        while let Some(at) = todo.pop_front() {
            for &n in &g[&at] {
                if let std::collections::hash_map::Entry::Vacant(e) = piece.entry(n) {
                    e.insert(i);
                    nodes.push(n);
                    todo.push_back(n);
                }
            }
        }
        nodes.sort();
        out.push(nodes);
    }
    (piece, out)
}
