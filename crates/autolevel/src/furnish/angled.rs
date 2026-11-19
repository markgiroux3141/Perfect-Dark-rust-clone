//! Angled catwalks: straight catwalks at an angle across a hall's air, from one deck edge to
//! another on the same storey. New in the Rust (docs/AUTOLEVEL.md, "Angled catwalks"): the Python
//! has none. Two things are made of them:
//!
//! - **an island**: a square of deck in the hall's emptiest air (the most clear air round it on its
//!   storey, with nothing built over or under it within two storeys), a clear gap (`CLEAR`) from
//!   every other deck, reached only by three or four angled catwalks ("spokes") from the catwalks
//!   and platforms round it, spread round it (`SPREAD`);
//! - **a shortcut**: one across the air between two decks of a storey (a catwalk's two walls, a
//!   catwalk and a platform, two platforms, an island), where it saves the most walking (at least
//!   `SAVES`).
//!
//! The grid knows nothing of angles, so an angled catwalk is an overlay on it:
//!
//! - **It is not deck.** It is a slab a catwalk wide, its ends cut along the two deck edges it
//!   joins (`End`), so its top is flush with both decks and nothing overlaps. Its `footprint` (the
//!   half cells it crosses, and a half cell round) is kept clear of everything else: what is built
//!   near its storey when it is laid, and what comes after (`spanned`: pits land on nothing there,
//!   no pillar stands under it, no drop falls through it). An island is ordinary deck.
//! - **Its way**: a nav edge joins the nav block just inside each end (`End::node`), so the room's
//!   paths, the dead-end graph and the walks cross it (`waypoints` steps along its middle line).
//! - **Rails**: the deck's own rail along each edge it meets is cut where it crosses (`cut_rails`);
//!   its two sides are railed by the polygons (`geometry::angled`).
//!
//! Laid after the platforms (so it goes where they did not, and may reach them) and before the
//! pits, the cover and the rails. With the chance `Setup::angled` a hall gets an island, and each
//! of its catwalk storeys a shortcut, all drawn first from a stream of its own
//! ("{seed}/{key}/pits/angled"): nothing else in the room moves, and none unless asked for.
//! Where it all goes is searched, with no more random numbers.

use super::platforms::Sums;
use super::{grow, Interior, Node, Rail};
use crate::cells::{rect_cells, Cells, Rect};
use crate::plan::cells as units;
use crate::py;
use crate::pyrand::PyRandom;
use crate::space::DIRS;
use std::collections::{HashMap, VecDeque};

/// A room's shorter side is at least this, h, for angled catwalks.
const NARROWEST: i32 = 20;
/// An island is laid only with this many spokes or more, and gets no more than
const FEWEST: usize = 3;
const MOST: usize = 4;
/// The least angle between two of an island's spokes, degrees.
const SPREAD: f64 = 60.0;
/// Clear air round an island, h, to any other deck of its storey (three cells: it reads as an
/// island, its spokes as bridges, and nobody steps across).
const CLEAR: i32 = 6;
/// How much clear air round an island counts, h: the most is the emptiest spot.
const REACH: i32 = 24;
/// Island sizes, in catwalk widths: the bigger first (a spoke at an angle needs a long edge to
/// leave from: its width over the sine of its angle to the edge, and a margin each side).
const ISLANDS: [(i32, i32); 7] = [(6, 6), (6, 4), (4, 6), (5, 5), (4, 4), (5, 3), (3, 5)];
/// Islands tried, the emptiest first, before the room is given none.
const TRIES: usize = 40;
/// A shortcut saves at least this many cells' walk (one between decks no walk joins counts as
/// `JOINS`).
const SAVES: f64 = 10.0;
const JOINS: f64 = 40.0;
/// An angled catwalk's length, h, along its middle: at least, and at most.
const SHORTEST: f64 = 4.0;
const LONGEST: f64 = 48.0;
/// Air kept either side of one, h, and past each end of its joint along the deck's edge.
const MARGIN: f64 = 1.0;
/// The angles it may leave a deck's edge at, degrees off straight out (straight out, or near it,
/// is a catwalk the grid could lay).
const ANGLES: [f64; 8] = [-60.0, -45.0, -30.0, -20.0, 20.0, 30.0, 45.0, 60.0];
/// Steps along its middle while looking for the deck it reaches, h.
const STEP: f64 = 0.25;
/// Its slab's depth under its top, units (a deck's: `plans`), and the room over it a pier keeps clear.
const SLAB: i32 = 40;
const HEADROOM: i32 = crate::plan::HEADROOM;

/// One end of an angled catwalk: where it meets a deck, cut along that deck's edge.
#[derive(Clone, Debug, PartialEq)]
pub struct End {
    /// The edge: a line of x (axis 0) or of y (axis 1), at `line` h, and the way out of the deck
    /// over the catwalk (an index into `DIRS`).
    pub axis: usize,
    pub line: i32,
    pub out: usize,
    /// Where its middle crosses the edge, h.
    pub at: (f64, f64),
    /// Its two corners on the edge, units: its left side's (looking from its first end) first.
    pub corners: [(i32, i32); 2],
    /// The nav block just inside the deck there.
    pub node: Node,
}

/// An angled catwalk on storey `z`: its top at `z`, its slab as thick as a deck's.
#[derive(Clone, Debug, PartialEq)]
pub struct Angled {
    pub z: i32,
    /// "spoke" (to an island: its first end is the island's) or "shortcut".
    pub kind: &'static str,
    pub ends: [End; 2],
    /// Half cells it crosses on its storey, and a half cell round.
    pub footprint: Cells,
}

impl Angled {
    /// Its length along its middle, in cells.
    pub fn cells(&self) -> f64 {
        let (a, b) = (self.ends[0].at, self.ends[1].at);
        (b.0 - a.0).hypot(b.1 - a.1) / 2.0
    }

    /// The way it runs from its first end, a unit vector.
    pub fn way(&self) -> (f64, f64) {
        let (a, b) = (self.ends[0].at, self.ends[1].at);
        let n = (b.0 - a.0).hypot(b.1 - a.1);
        ((b.0 - a.0) / n, (b.1 - a.1) / n)
    }

    /// The way it runs, degrees anticlockwise from +x.
    fn bearing(&self) -> f64 {
        let (x, y) = self.way();
        y.atan2(x).to_degrees()
    }
}

/// A point on the line through `p` along `u`, offset `s` to its left, where it meets the line of
/// `axis` at `line`.
fn meet(p: (f64, f64), u: (f64, f64), s: f64, axis: usize, line: f64) -> (f64, f64) {
    let q = (p.0 - u.1 * s, p.1 + u.0 * s);
    let t = if axis == 0 { (line - q.0) / u.0 } else { (line - q.1) / u.1 };
    (q.0 + t * u.0, q.1 + t * u.1)
}

/// Does a convex polygon (h) overlap the inside of half cell `c`, more than a sliver?
fn overlaps(poly: &[(f64, f64)], c: (i32, i32)) -> bool {
    const EPS: f64 = 1e-6;
    let sq = [(c.0 as f64, c.1 as f64), (c.0 as f64 + 1.0, c.1 as f64), (c.0 as f64 + 1.0, c.1 as f64 + 1.0), (c.0 as f64, c.1 as f64 + 1.0)];
    let mut axes = vec![(1.0, 0.0), (0.0, 1.0)];
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        axes.push((b.1 - a.1, a.0 - b.0));
    }
    axes.into_iter().all(|(ax, ay)| {
        let span = |pts: &[(f64, f64)]| pts.iter().map(|p| p.0 * ax + p.1 * ay).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let ((a0, a1), (b0, b1)) = (span(poly), span(&sq));
        a1.min(b1) - a0.max(b0) > EPS * ax.hypot(ay).max(1e-9)
    })
}

/// The half cells a convex polygon (h) overlaps.
fn cover(poly: &[(f64, f64)]) -> Cells {
    let lo = |f: fn(&(f64, f64)) -> f64| poly.iter().map(f).fold(f64::INFINITY, f64::min).floor() as i32;
    let hi = |f: fn(&(f64, f64)) -> f64| poly.iter().map(f).fold(f64::NEG_INFINITY, f64::max).ceil() as i32;
    let (x0, y0, x1, y1) = (lo(|p| p.0), lo(|p| p.1), hi(|p| p.0), hi(|p| p.1));
    (x0..x1).flat_map(|x| (y0..y1).map(move |y| (x, y))).filter(|&c| overlaps(poly, c)).collect()
}

/// The way `DIRS[out]` turned `deg` degrees anticlockwise.
fn turn(out: usize, deg: f64) -> (f64, f64) {
    let (x, y) = (DIRS[out].0 as f64, DIRS[out].1 as f64);
    let (s, c) = deg.to_radians().sin_cos();
    (x * c - y * s, x * s + y * c)
}

/// A unit vector's part along `axis`.
fn along_axis(u: (f64, f64), axis: usize) -> f64 {
    if axis == 0 {
        u.0
    } else {
        u.1
    }
}

/// Nav steps from `a` to every node it reaches.
fn steps(g: &super::Nav, a: Node) -> HashMap<Node, u32> {
    let mut d = HashMap::from([(a, 0)]);
    let mut todo = VecDeque::from([a]);
    while let Some(n) = todo.pop_front() {
        let k = d[&n];
        for &m in g.get(&n).map_or(&[][..], |v| v.as_slice()) {
            d.entry(m).or_insert_with(|| {
                todo.push_back(m);
                k + 1
            });
        }
    }
    d
}

/// A point along a deck's edge an angled catwalk may leave from: the edge (axis, line, the way out
/// of the deck) and the point, h.
type Edge = (usize, i32, usize, (f64, f64));

impl Interior {
    /// The half cells the angled catwalks on storey `z` keep clear.
    pub fn spanned(&self, z: i32) -> Cells {
        self.angled.iter().filter(|s| s.z == z).flat_map(|s| s.footprint.iter().copied()).collect()
    }

    /// The deck round the ends of the angled catwalks on storey `z`: a pit keeps out of it.
    pub(super) fn joints(&self, z: i32) -> Cells {
        let ends = self.angled.iter().filter(|s| s.z == z).flat_map(|s| s.ends.iter());
        ends.flat_map(|e| rect_cells((e.node.0 - 1, e.node.1 - 1, e.node.0 + 3, e.node.1 + 3))).collect()
    }

    /// What an angled catwalk on storey `z` keeps clear of (besides deck): what is at its height
    /// (pods' blocks and roofs on the storey, ramps from or to it and their landings, piers through
    /// it, the angled catwalks already there) and every column a drop or a beam falls down. (What is
    /// a storey or more over or under it has the headroom a deck over a deck has.)
    fn blocked(&self, z: i32) -> Cells {
        let mut out: Cells = self.shell[&z].union(&self.roof[&z]).copied().collect();
        for r in self.ramps.iter().filter(|r| r.lo == z || r.hi == z) {
            out.extend(r.cells());
            out.extend(r.top.iter().copied());
            out.extend(r.foot.iter().copied());
        }
        for q in self.piers.iter().filter(|q| q.4 < z + HEADROOM && z - SLAB < q.5) {
            out.extend(rect_cells((q.0, q.1, q.2, q.3)));
        }
        for k in &self.keep {
            out.extend(rect_cells(*k));
        }
        out.extend(self.hole.iter().copied());
        out.extend(self.spanned(z));
        out
    }

    /// An end of an angled catwalk whose middle runs along `u` and crosses the edge (`axis`,
    /// `line`) at `at`, out of `deck` the way `out`: None unless the deck runs the whole joint and
    /// a `MARGIN` past it, with a nav block just inside.
    #[allow(clippy::too_many_arguments)]
    fn end(&self, at: (f64, f64), u: (f64, f64), axis: usize, line: i32, out: usize, deck: &Cells, blocks: &Cells) -> Option<End> {
        let hw = self.band as f64 / 2.0;
        let across = along_axis(u, axis).abs();
        if across < 0.5 - 1e-9 {
            return None; // (shallower than 30 degrees to the edge: a long, thin joint)
        }
        let along = if axis == 0 { at.1 } else { at.0 };
        let reach = (hw + MARGIN) / across;
        let inward = if axis == 0 { -DIRS[out].0 } else { -DIRS[out].1 }; // the deck's side of the line
        let row = if inward > 0 { line } else { line - 1 };
        let cell = |k: i32| if axis == 0 { (row, k) } else { (k, row) };
        if !((along - reach).floor() as i32..(along + reach).ceil() as i32).all(|k| deck.contains(&cell(k))) {
            return None;
        }
        let k = along.round() as i32 - 1;
        let low = if inward > 0 { line } else { line - 2 };
        let node = if axis == 0 { (low, k) } else { (k, low) };
        if !blocks.contains(&node) {
            return None;
        }
        let corner = |s: f64| {
            let p = meet(at, u, s, axis, line as f64);
            (units(p.0 / 2.0), units(p.1 / 2.0))
        };
        Some(End { axis, line, out, at, corners: [corner(hw), corner(-hw)], node: (node.0, node.1, 0) })
    }

    /// The angled catwalk on storey `z` that leaves the edge `e` along `u`, to the first deck it
    /// meets: None if it meets something else first, or nothing near enough, or it can't be cut
    /// cleanly at either end. `deck` is all the storey's.
    #[allow(clippy::too_many_arguments)]
    fn cast(&self, z: i32, e: Edge, u: (f64, f64), deck: &Cells, blocked: &Cells, blocks: &Cells, kind: &'static str) -> Option<Angled> {
        let (axis, line, out, at) = e;
        let start = self.end(at, u, axis, line, out, deck, blocks)?;
        // along its middle to the first cell of deck
        let mut t = STEP / 4.0;
        let mut was = ((at.0 + u.0 * t).floor() as i32, (at.1 + u.1 * t).floor() as i32);
        let (cell, prev, t) = loop {
            t += STEP;
            if t > LONGEST {
                return None;
            }
            let c = ((at.0 + u.0 * t).floor() as i32, (at.1 + u.1 * t).floor() as i32);
            if c != was {
                if !self.inside(c) || blocked.contains(&c) {
                    return None;
                }
                if deck.contains(&c) {
                    break (c, was, t);
                }
                was = c;
            }
        };
        if t < SHORTEST || (cell.0 != prev.0 && cell.1 != prev.1) {
            return None; // (too short, or through a corner)
        }
        let axis1 = if cell.0 != prev.0 { 0 } else { 1 };
        let line1 = if axis1 == 0 { cell.0.max(prev.0) } else { cell.1.max(prev.1) };
        let step = if axis1 == 0 { (prev.0 - cell.0, 0) } else { (0, prev.1 - cell.1) };
        let out1 = DIRS.iter().position(|&d| d == step).expect("a step between neighbours");
        let far = self.end(meet(at, u, 0.0, axis1, line1 as f64), u, axis1, line1, out1, deck, blocks)?;
        // the slab and a margin either side, between the two edges
        let w = self.band as f64 / 2.0 + MARGIN;
        let quad =
            [meet(at, u, w, axis, line as f64), meet(at, u, -w, axis, line as f64), meet(at, u, -w, axis1, line1 as f64), meet(at, u, w, axis1, line1 as f64)];
        let footprint = cover(&quad);
        if footprint.iter().any(|c| !self.inside(*c) || deck.contains(c) || blocked.contains(c)) {
            return None;
        }
        let on = |mut e: End| {
            e.node.2 = z;
            e
        };
        Some(Angled { z, kind, ends: [on(start), on(far)], footprint })
    }

    /// The points along the edges of `from` (some of the storey's `deck`) that face the room's open
    /// air, one at the middle of each half cell's side.
    fn edges(&self, from: &Cells, deck: &Cells, blocked: &Cells) -> Vec<Edge> {
        let mut out = Vec::new();
        for &c in from {
            for (d, &(dx, dy)) in DIRS.iter().enumerate() {
                let n = (c.0 + dx, c.1 + dy);
                if !self.inside(n) || deck.contains(&n) || blocked.contains(&n) {
                    continue;
                }
                if dx != 0 {
                    let line = c.0 + (dx > 0) as i32;
                    out.push((0, line, d, (line as f64, c.1 as f64 + 0.5)));
                } else {
                    let line = c.1 + (dy > 0) as i32;
                    out.push((1, line, d, (c.0 as f64 + 0.5, line as f64)));
                }
            }
        }
        out
    }

    /// Every angled catwalk on storey `z` from the edges of `from`, at every angle of `ANGLES`.
    fn casts(&self, z: i32, from: &Cells, kind: &'static str) -> Vec<Angled> {
        let (deck, blocked, blocks) = (self.decked(z), self.blocked(z), self.blocks(z));
        let mut out = Vec::new();
        for e in self.edges(from, deck, &blocked) {
            for deg in ANGLES {
                out.extend(self.cast(z, e, turn(e.2, deg), deck, &blocked, &blocks, kind));
            }
        }
        out
    }

    /// An island, and a shortcut on each catwalk storey, each with the chance `Setup::angled`
    /// (see the module).
    pub(super) fn lay_angled(&mut self) {
        if self.angled_share <= 0.0 || self.upper.is_empty() || self.s.decks.is_some() || self.w.min(self.d) < NARROWEST {
            return;
        }
        let mut rng = PyRandom::named(&format!("{}/angled", self.stream));
        let isle = rng.random() < self.angled_share;
        let short: Vec<(i32, bool)> = self.upper.iter().map(|&z| (z, rng.random() < self.angled_share)).collect();
        if isle {
            let up = self.upper.clone();
            self.lay_island(&up);
        }
        for (z, go) in short {
            if go && !self.decked(z).is_empty() {
                self.lay_shortcut(z);
            }
        }
    }

    /// An island on one of `storeys` at the emptiest spot that takes `FEWEST` spokes or more (see
    /// the module).
    pub(super) fn lay_island(&mut self, storeys: &[i32]) {
        let band = self.band;
        // (clear air round it on its storey, emptiness, storey, rectangle), the emptiest first
        let mut isles: Vec<(i32, f64, i32, Rect)> = Vec::new();
        for &z in storeys {
            if self.decked(z).is_empty() {
                continue;
            }
            // nothing built over or under it within two storeys (it floats), nothing at its
            // height within `CLEAR` (nobody steps onto it), and the more air round it the better
            let (soft, hard) = self.built(Some(z));
            let near = Sums::new(self, &soft.union(&hard).copied().collect());
            let level: Cells = self.decked(z).union(&self.blocked(z)).copied().collect();
            let t = Sums::new(self, &level);
            for (a, b) in ISLANDS.map(|(a, b)| (a * band, b * band)) {
                let (lo_x, lo_y, hi_x, hi_y) = (self.x0 + band + CLEAR, self.y0 + band + CLEAR, self.x1 - band - CLEAR - a, self.y1 - band - CLEAR - b);
                for y in (lo_y..=hi_y).filter(|y| y.rem_euclid(2) == 0) {
                    for x in (lo_x..=hi_x).filter(|x| x.rem_euclid(2) == 0) {
                        let r = (x, y, x + a, y + b);
                        if near.count(grow(r, 1)) > 0 || t.count(grow(r, CLEAR)) > 0 {
                            continue;
                        }
                        let mut g = CLEAR;
                        while g < REACH && t.count(grow(r, g + 1)) == 0 {
                            g += 1;
                        }
                        let round = grow(r, REACH);
                        isles.push((g, 1.0 - near.count(round) as f64 / near.inside(round).max(1) as f64, z, r));
                    }
                }
            }
        }
        // (stable: ties keep the order found)
        isles.sort_by(|p, q| q.0.cmp(&p.0).then_with(|| py::cmp_f64(q.1, p.1)));
        let mut tried: Vec<(i32, Rect)> = Vec::new();
        for &(_, _, z, r) in &isles {
            // (one of each spot: its neighbours a cell over do much the same)
            if tried.iter().any(|&(tz, tr)| tz == z && super::overlap(tr, r)) {
                continue;
            }
            if tried.len() == TRIES {
                break;
            }
            tried.push((z, r));
            let isle = rect_cells(r);
            self.decked_mut(z).extend(isle.iter().copied());
            self.reflood();
            let spokes = self.spokes(z, &isle);
            if spokes.len() >= FEWEST {
                self.islands.push((z, r));
                self.angled.extend(spokes);
                return;
            }
            self.decked_mut(z).retain(|c| !isle.contains(c));
            self.reflood();
        }
    }

    /// The spokes from an island (already deck): the shortest first, each `SPREAD` from the
    /// others round it and clear of them, up to `MOST`.
    fn spokes(&self, z: i32, isle: &Cells) -> Vec<Angled> {
        let mut all: Vec<Angled> = self.casts(z, isle, "spoke").into_iter().filter(|s| !isle.contains(&(s.ends[1].node.0, s.ends[1].node.1))).collect();
        all.sort_by(|a, b| py::cmp_f64(a.cells(), b.cells()));
        let mut out: Vec<Angled> = Vec::new();
        for s in all {
            let apart = |o: &Angled| {
                let d = (s.bearing() - o.bearing()).rem_euclid(360.0);
                d.min(360.0 - d) >= SPREAD
            };
            if out.len() < MOST && out.iter().all(|o| apart(o) && o.footprint.is_disjoint(&s.footprint)) {
                out.push(s);
            }
        }
        out
    }

    /// The angled catwalk on storey `z` that saves the most walking, if it saves `SAVES` or more.
    pub(super) fn lay_shortcut(&mut self, z: i32) {
        let g = self.nav();
        let mut from: HashMap<Node, HashMap<Node, u32>> = HashMap::new();
        let mut best: Option<(f64, Angled)> = None;
        for s in self.casts(z, &self.decked(z).clone(), "shortcut") {
            let (a, b) = (s.ends[0].node, s.ends[1].node);
            let d = from.entry(a).or_insert_with(|| steps(&g, a));
            let saved = d.get(&b).map_or(JOINS, |&k| k as f64 / 2.0 - s.cells());
            if saved >= SAVES && best.as_ref().is_none_or(|(sv, o)| saved > *sv || (saved == *sv && s.cells() < o.cells())) {
                best = Some((saved, s));
            }
        }
        if let Some((_, s)) = best {
            self.angled.push(s);
        }
    }

    /// The rails, with each deck edge an angled catwalk meets left open where it crosses.
    pub(super) fn cut_rails(&self, rails: Vec<Rail>) -> Vec<Rail> {
        if self.angled.is_empty() {
            return rails;
        }
        let mut out = Vec::new();
        for rail in rails {
            let mut pieces = vec![rail];
            for s in &self.angled {
                for e in &s.ends {
                    let (dx, dy) = DIRS[e.out];
                    let at = units(e.line as f64 / 2.0);
                    let k = 1 - e.axis; // the coordinate along the edge
                    let (a, b) = (e.corners[0], e.corners[1]);
                    let (cut0, cut1) = if k == 0 { (a.0.min(b.0), a.0.max(b.0)) } else { (a.1.min(b.1), a.1.max(b.1)) };
                    pieces = pieces
                        .into_iter()
                        .flat_map(|(p, q, f)| {
                            let on = p.2 == s.z && f == (dx, dy, 0) && if e.axis == 0 { p.0 == at && q.0 == at } else { p.1 == at && q.1 == at };
                            if !on {
                                return vec![(p, q, f)];
                            }
                            let get = |v: (i32, i32, i32)| if k == 0 { v.0 } else { v.1 };
                            let put = |v: (i32, i32, i32), c: i32| if k == 0 { (c, v.1, v.2) } else { (v.0, c, v.2) };
                            let (lo, hi) = (get(p).min(get(q)), get(p).max(get(q)));
                            let mut keep = Vec::new();
                            if lo < cut0.min(hi) {
                                keep.push((put(p, lo), put(p, cut0.min(hi)), f));
                            }
                            if cut1.max(lo) < hi {
                                keep.push((put(p, cut1.max(lo)), put(p, hi), f));
                            }
                            keep
                        })
                        .collect();
                }
            }
            out.extend(pieces);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deadends::{self, NavCache};
    use crate::level::{generate, Options};
    use crate::place::Scale;
    use std::collections::HashSet;

    /// The nav nodes reached from `from` over `g`.
    fn reach(g: &super::super::Nav, from: impl IntoIterator<Item = Node>) -> HashSet<Node> {
        let mut seen: HashSet<Node> = HashSet::new();
        let mut todo: VecDeque<Node> = VecDeque::new();
        for n in from {
            if seen.insert(n) {
                todo.push_back(n);
            }
        }
        while let Some(n) = todo.pop_front() {
            for &m in g.get(&n).into_iter().flatten() {
                if seen.insert(m) {
                    todo.push_back(m);
                }
            }
        }
        seen
    }

    /// A rail on storey `z` across the joint of end `e`, if there is one.
    fn railed_across(it: &Interior, z: i32, e: &End) -> Option<String> {
        let k = 1 - e.axis;
        let get = |c: (i32, i32)| if k == 0 { c.0 } else { c.1 };
        let (lo, hi) = (get(e.corners[0]).min(get(e.corners[1])), get(e.corners[0]).max(get(e.corners[1])));
        let line = units(e.line as f64 / 2.0);
        let (dx, dy) = DIRS[e.out];
        for (p, q, _) in it.rails.iter().filter(|(p, _, f)| p.2 == z && *f == (dx, dy, 0)) {
            let (pl, ql) = if e.axis == 0 { (p.0, q.0) } else { (p.1, q.1) };
            let (a, b) = if k == 0 { (p.0.min(q.0), p.0.max(q.0)) } else { (p.1.min(q.1), p.1.max(q.1)) };
            if pl == line && ql == line && a < hi && lo < b {
                return Some(format!("a rail {p:?}..{q:?} across its joint {lo}..{hi}"));
            }
        }
        None
    }

    /// What stands in an angled catwalk's footprint, if anything: deck, a pod, a ramp, a pier, a
    /// pillar, another angled catwalk, a pit's landing.
    fn in_the_way(it: &Interior, s: &Angled) -> Option<&'static str> {
        let (z, f) = (s.z, &s.footprint);
        if !(f.is_disjoint(it.decked(z)) && f.is_disjoint(&it.shell[&z]) && f.is_disjoint(&it.roof[&z])) {
            return Some("deck or a pod");
        }
        if it.ramps.iter().filter(|rp| rp.lo == z || rp.hi == z).any(|rp| !f.is_disjoint(&rp.cells()) || !f.is_disjoint(&rp.top) || !f.is_disjoint(&rp.foot)) {
            return Some("a ramp");
        }
        if it.piers.iter().filter(|q| q.4 <= z && z - 40 < q.5).any(|q| !f.is_disjoint(&rect_cells((q.0, q.1, q.2, q.3)))) {
            return Some("a pier");
        }
        if it.cover.iter().filter(|c| it.levels[0] + c.4 > z - 40).any(|c| !f.is_disjoint(&rect_cells((c.0, c.1, c.2, c.3)))) {
            return Some("a pillar");
        }
        if it.angled.iter().filter(|o| o.z == z && !std::ptr::eq(*o, s)).any(|o| !f.is_disjoint(&o.footprint)) {
            return Some("another angled catwalk");
        }
        if it.pits.iter().any(|p| p.land == z && !p.cells().is_disjoint(f)) {
            return Some("a pit's landing");
        }
        if it.pits.iter().any(|p| p.z == z && !p.cells().is_disjoint(&it.joints(z))) {
            return Some("a pit at its end");
        }
        None
    }

    #[test]
    fn angled_catwalks_join_decks_and_an_island_is_reached_only_by_its_spokes() {
        let (mut levels, mut isles, mut spokes, mut shortcuts) = (0, 0, 0, 0);
        // (grand bare: islands; towering with platforms and their pits: shortcuts to them)
        for (scale, seeds, platforms) in [(Scale::Grand, 1..=10, 0.0), (Scale::Towering, 1..=10, 1.0)] {
            for seed in seeds {
                let mut o = Options { platforms, holes: platforms / 2.0, ..Options::default() };
                o.siting.scale = scale;
                let plain = generate(seed, &o);
                o.angled = 1.0;
                let lv = generate(seed, &o);
                assert_eq!(plain.is_ok(), lv.is_ok(), "seed {seed}: angled catwalks change whether a level is made");
                let Ok(lv) = lv else { continue };
                levels += 1;
                let g = deadends::graph(lv.net.spaces(), &lv.net, &lv.interiors, &lv.drops, &mut NavCache::new());
                for (key, it) in &lv.interiors {
                    let blocks: HashMap<i32, Cells> = it.angled.iter().map(|s| (s.z, it.blocks(s.z))).collect();
                    for (n, s) in it.angled.iter().enumerate() {
                        let at = format!("seed {seed}: {key}@{} {} {n}", s.z, s.kind);
                        spokes += usize::from(s.kind == "spoke");
                        shortcuts += usize::from(s.kind == "shortcut");
                        for e in &s.ends {
                            assert!(blocks[&s.z].contains(&(e.node.0, e.node.1)), "{at}: an end's block is no standing room");
                            let line = units(e.line as f64 / 2.0);
                            assert!(e.corners.iter().all(|c| if e.axis == 0 { c.0 == line } else { c.1 == line }), "{at}: a corner off its edge");
                            assert_eq!(railed_across(it, s.z, e), None, "{at}");
                        }
                        assert_eq!(in_the_way(it, s), None, "{at}: in its way");
                        assert!(
                            g.ways.iter().any(|w| w.kind == "angled" && [w.pa, w.pb] == [s.ends[1].node, s.ends[0].node]),
                            "{at}: not in the dead-end graph"
                        );
                        let name = format!("angled_{key}_{}_{}", s.z, it.angled[..n].iter().filter(|q| q.z == s.z).count() + 1);
                        assert!(lv.walks.iter().any(|(w, _)| *w == name) && lv.views.iter().any(|(v, _)| *v == name), "{at}: no walk or view {name}");
                    }
                    for &(z, r) in &it.islands {
                        isles += 1;
                        let isle = rect_cells(r);
                        let here: Vec<&Angled> = it.angled.iter().filter(|s| s.z == z && s.kind == "spoke").collect();
                        let at = format!("seed {seed}: {key}@{z} {r:?}");
                        assert!((FEWEST..=MOST).contains(&here.len()), "{at}: {} spokes", here.len());
                        assert!(here.iter().all(|s| isle.contains(&(s.ends[0].node.0, s.ends[0].node.1))), "{at}: a spoke's first end is off the island");
                        assert!(isle.is_subset(it.decked(z)), "{at}: the island is not deck");
                        // no other deck of its storey near enough to step onto
                        let rest: Cells = it.decked(z).difference(&isle).copied().collect();
                        assert!(rect_cells(grow(r, CLEAR)).is_disjoint(&rest), "{at}: deck within a step of the island");
                        // on the grid alone the island is a piece of its own; with its spokes a door reaches it
                        let mut grid = it.nav();
                        for s in &it.angled {
                            for (p, q) in [(s.ends[0].node, s.ends[1].node), (s.ends[1].node, s.ends[0].node)] {
                                grid.get_mut(&p).expect("an end's block").retain(|m| *m != q);
                            }
                        }
                        let mine = reach(&grid, [here[0].ends[0].node]);
                        assert!(mine.iter().all(|n| n.2 == z && isle.contains(&(n.0, n.1))), "{at}: the island is reached by the grid");
                        let doors = reach(&it.nav(), it.door_nodes().into_iter().flatten());
                        assert!(doors.contains(&here[0].ends[0].node), "{at}: no door reaches the island");
                    }
                }
            }
        }
        assert!(levels >= 16 && isles >= 6 && spokes >= 18 && shortcuts >= 10, "{levels} levels, {isles} islands, {spokes} spokes, {shortcuts} shortcuts");
        // none unless asked for: every seed's level is as it was
        assert!(generate(40, &Options::default()).unwrap().interiors.iter().all(|(_, it)| it.islands.is_empty() && it.angled.is_empty()));
    }

    #[test]
    fn a_diagonal_slabs_cells() {
        // a strip a cell wide at 45 degrees from (0, 0) to (4, 4), its ends cut on x = 0 and x = 4
        let u = (0.5f64.sqrt(), 0.5f64.sqrt());
        let quad = [meet((0.0, 0.0), u, 1.0, 0, 0.0), meet((0.0, 0.0), u, -1.0, 0, 0.0), meet((0.0, 0.0), u, -1.0, 0, 4.0), meet((0.0, 0.0), u, 1.0, 0, 4.0)];
        let cells = cover(&quad);
        // all within x 0..4, the diagonal cells in, the far corners out
        assert!(cells.iter().all(|c| (0..4).contains(&c.0)));
        assert!((0..4).all(|i| cells.contains(&(i, i))));
        assert!(!cells.contains(&(0, 3)) && !cells.contains(&(3, 0)));
        // its half width along x = 0 is 1 / sin 45
        let (l, r) = (quad[0], quad[1]);
        assert!((l.1 - r.1 - 2.0 * 2f64.sqrt()).abs() < 1e-9 && l.0 == 0.0 && r.0 == 0.0);
        // a turn of the way east
        let (x, y) = turn(0, 90.0);
        assert!(x.abs() < 1e-9 && (y - 1.0).abs() < 1e-9);
    }
}
