//! Dead ends: the parts of a level that hang off the rest by a single way in. A port of
//! `autolevel/deadends.py`.
//!
//! The user's note on the first big levels: a room off a main area that goes nowhere is fine (a
//! camping spot), but a long corridor, or a spiral of ramps up to the top of a very tall room,
//! that ends in nothing is not. This finds them, so the level can join them up or cut them short
//! (`Build::resolve` does), and lists them for anyone who wants to do that by hand.
//!
//! A **place** is somewhere to stand: a room's floor, each separate piece of deck on its upper
//! storeys, a pod. A **way** joins two places: a corridor (with its ramps), a stub through a
//! wall, a vent, an open ramp inside a room, a pod's door, and a drop through a floor, which is
//! one way. A way is **blind** going in if, past it, the only way back is the way you came.
//! Everything past a blind way is a dead end, unless it holds the looped middle of the level, or
//! is a district of its own (a loop and `district` rooms or more). Each is measured from the
//! place it hangs off (its root) to its farthest place (its tip): cells walked, how many of
//! those are corridor, and the storeys climbed (or descended) to reach decks with no room at or
//! past them. It is allowed while
//!
//! - its corridor is no longer than `corridor` cells (8: a room off a main area),
//! - the whole walk to its tip is no longer than `walk` cells (20: a pod two ramps up a hall is
//!   a lookout; a loft at the top of six storeys of spiral is not), and
//! - no climb to nothing is `climb` storeys or more (2: a balcony is fine).
//!
//! `joins` lists where a dead end could be joined to the rest, and `DeadEnd::leaf` and
//! `DeadEnd::cut` say what to take away if it can't be: the build tries the first and falls back
//! on the second. The same lists would do for a person choosing the joins.
//!
//! The Python knows places by their keys throughout. Here `find` numbers them once and works on
//! the numbers; wherever an order is by key, it still is. One thing is not as the Python:
//! `measure` goes along the ways it took in the order it took them, where the Python (frozen)
//! fails on a drop that costs nothing to reach. It is written up there.

use crate::build::Sided;
use crate::cells::{Cell, Cells};
use crate::furnish::{Interior, Node};
use crate::network::Network;
use crate::py;
use crate::route::Cost;
use crate::scatter::rect_gap;
use crate::space::{find as room, Door, Space, DIRS};
use crate::storeys::{index, LEVELS};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// An open ramp's run, cells.
pub const RAMP_CELLS: f64 = 4.0;
/// The kinds of way that count as corridor.
pub const CORRIDOR: [&str; 3] = ["corridor", "stub", "vent"];
/// `joins`: how far off (cells) a dead end looks, by default.
pub const REACH: i32 = 10;
/// `joins`: the most storeys between a place and the door it is joined to.
pub const STOREYS: i32 = 4;
/// `joins`: the most rooms listed for one place.
pub const EACH: usize = 3;

/// Somewhere to stand.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// "vast@0", "spire1@827", "spire1@827.2" (a second piece of deck there), or a pod's key.
    pub key: String,
    pub room: String,
    pub level: i32,
    /// floor, deck or pod.
    pub kind: &'static str,
}

/// What joins two places.
#[derive(Clone, Debug, PartialEq)]
pub struct Way {
    /// Place keys.
    pub a: String,
    pub b: String,
    /// corridor, stub, vent, ramp, pod or drop.
    pub kind: &'static str,
    /// Cells walked along it.
    pub length: f64,
    /// Where it meets a and b: nav nodes (x, y, z) in half cells.
    pub pa: Node,
    pub pb: Node,
    /// a to b only.
    pub oneway: bool,
    /// Its place in the network's routes, for corridors, stubs and vents.
    pub route: Option<usize>,
}

/// The places a player stands, and the ways between them.
#[derive(Clone, Debug)]
pub struct Graph {
    /// In the order they were made.
    places: Vec<Place>,
    pub ways: Vec<Way>,
    /// Room key -> its standing blocks.
    pub navs: HashMap<String, Arc<Nav>>,
    /// Place key -> its number in `places`.
    at: HashMap<String, usize>,
}

impl Graph {
    pub fn new(places: Vec<Place>, ways: Vec<Way>, navs: HashMap<String, Arc<Nav>>) -> Graph {
        let at = places.iter().enumerate().map(|(n, p)| (p.key.clone(), n)).collect();
        Graph { places, ways, navs, at }
    }

    /// Every place, in the order they were made.
    pub fn places(&self) -> &[Place] {
        &self.places
    }

    /// A place's number in `places`.
    pub fn number(&self, key: &str) -> usize {
        *self.at.get(key).unwrap_or_else(|| panic!("no place {key}"))
    }

    /// The place with this key.
    pub fn place(&self, key: &str) -> &Place {
        &self.places[self.number(key)]
    }

    /// Cells walked inside a place from one of its ports to another.
    pub fn inner(&self, place: &str, p: Node, q: Node) -> f64 {
        if p == q {
            return 0.0;
        }
        let here = self.place(place);
        match self.navs.get(&here.room) {
            Some(nav) if here.kind != "pod" => nav.dist(p, q),
            _ => ((p.0 - q.0).abs() + (p.1 - q.1).abs()) as f64 / 2.0,
        }
    }

    /// Where a way meets one of its two places.
    pub fn port(&self, way: &Way, place: &str) -> Node {
        if way.a == place {
            way.pa
        } else {
            way.pb
        }
    }
}

/// A dead end, as plain data for the report.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub root: String,
    pub tip: String,
    pub rooms: Vec<String>,
    pub length: f64,
    pub corridor: f64,
    pub storeys: i32,
    pub climb: i32,
    pub allowed: bool,
    pub why: Vec<String>,
}

/// A part of the level that hangs off the rest by one way in.
#[derive(Clone, Debug, PartialEq)]
pub struct DeadEnd {
    /// The place it hangs off.
    pub root: String,
    /// The one way in.
    pub way: Way,
    /// Its places, nearest first.
    pub places: Vec<String>,
    /// The farthest of them.
    pub tip: String,
    /// Cells from the root to the tip.
    pub length: f64,
    /// Of which corridor (stubs and vents too).
    pub corridor: f64,
    /// The most storeys from a room (or the root) to a deck with no room at or past it.
    pub climb: i32,
    /// Storeys between the root and its highest (or lowest) place.
    pub storeys: i32,
    /// Rooms that are wholly in it.
    pub rooms: Vec<String>,
    /// The places in the network's routes of the corridors in it.
    pub routes: Vec<usize>,
    /// It has a loop of its own (the Python's `loop`).
    pub looped: bool,
    /// Its places with no room at or past them, nearest first.
    pub bare: Vec<String>,
    /// (room, the highest storey its own decks may stay on) to end the climb, in the order the
    /// rooms were met. None: no storey of it may.
    pub cut: Vec<(String, Option<i32>)>,
    /// The farthest of its rooms that none of the others is reached through ("" if none).
    pub leaf: String,
    pub allowed: bool,
    pub why: Vec<String>,
}

impl DeadEnd {
    pub fn describe(&self) -> String {
        let what = if self.rooms.is_empty() { "decks".to_string() } else { self.rooms.join(", ") };
        let why = if self.allowed { String::new() } else { format!("  [{}]", self.why.join("; ")) };
        let (length, corridor, s) = (py::round(self.length), py::round(self.corridor), if self.storeys != 1 { "s" } else { "" });
        format!("{} -> {} ({what}): {length} cells, {corridor} of corridor, {} storey{s}{why}", self.root, self.tip, self.storeys)
    }

    /// As plain data, for the report.
    pub fn row(&self) -> Row {
        Row {
            root: self.root.clone(),
            tip: self.tip.clone(),
            rooms: self.rooms.clone(),
            length: py::round1(self.length),
            corridor: py::round1(self.corridor),
            storeys: self.storeys,
            climb: self.climb,
            allowed: self.allowed,
            why: self.why.clone(),
        }
    }
}

/// A room's standing blocks, storey by storey, in connected pieces (the Python's `_Nav`).
#[derive(Debug)]
pub struct Nav {
    pub blocks: BTreeMap<i32, Cells>,
    /// (x, y, z) -> piece number on its storey, from 1.
    pub piece: HashMap<Node, usize>,
    /// Pieces on each storey.
    pub count: BTreeMap<i32, usize>,
    /// Steps from a block to every block it reaches, for the blocks asked about so far.
    steps: Mutex<HashMap<Node, HashMap<Cell, i32>>>,
}

impl Nav {
    pub fn new(it: &Interior) -> Nav {
        let blocks: BTreeMap<i32, Cells> = it.levels.iter().map(|&z| (z, it.blocks(z))).collect();
        let mut piece: HashMap<Node, usize> = HashMap::new();
        let mut count = BTreeMap::new();
        for (&z, bs) in &blocks {
            let mut n = 0;
            for &c in bs {
                if piece.contains_key(&(c.0, c.1, z)) {
                    continue;
                }
                n += 1;
                let mut todo = vec![c];
                piece.insert((c.0, c.1, z), n);
                while let Some((x, y)) = todo.pop() {
                    for (dx, dy) in DIRS {
                        let q = (x + dx, y + dy);
                        if bs.contains(&q) && !piece.contains_key(&(q.0, q.1, z)) {
                            piece.insert((q.0, q.1, z), n);
                            todo.push(q);
                        }
                    }
                }
            }
            count.insert(z, n);
        }
        Nav { blocks, piece, count, steps: Mutex::new(HashMap::new()) }
    }

    /// The blocks of storey `z`, which the room must have.
    fn storey(&self, z: i32) -> &Cells {
        self.blocks.get(&z).unwrap_or_else(|| panic!("the room has no storey at {z}"))
    }

    /// The standing block nearest (x, y) on storey z.
    pub fn nearest(&self, x: i32, y: i32, z: i32) -> Option<Node> {
        py::first_min(self.storey(z).iter().map(|c| (c.0, c.1, z)), |c| ((c.0 - x).abs() + (c.1 - y).abs(), *c))
    }

    /// Cells walked from block p to block q on one storey (a straight-line guess if they don't join).
    pub fn dist(&self, p: Node, q: Node) -> f64 {
        let mut steps = self.steps.lock().unwrap_or_else(|e| e.into_inner());
        let seen = steps.entry(p).or_insert_with(|| {
            let bs = self.storey(p.2);
            let mut seen: HashMap<Cell, i32> = HashMap::from([((p.0, p.1), 0)]);
            let mut todo = VecDeque::from([(p.0, p.1)]);
            while let Some(c) = todo.pop_front() {
                let far = seen[&c];
                for (dx, dy) in DIRS {
                    let n = (c.0 + dx, c.1 + dy);
                    if bs.contains(&n) && !seen.contains_key(&n) {
                        seen.insert(n, far + 1);
                        todo.push_back(n);
                    }
                }
            }
            seen
        });
        match seen.get(&(q.0, q.1)) {
            None => ((p.0 - q.0).abs() + (p.1 - q.1).abs()) as f64 / 2.0,
            Some(&steps) => steps as f64 / 2.0,
        }
    }
}

/// Each inside's standing blocks, kept between calls of `graph`. The Python keys them by the
/// inside's `id`; here it is where the shared inside lives, which whoever keeps the cache must
/// keep alive (the build does: it keeps every inside it ever made).
pub type NavCache = HashMap<usize, Arc<Nav>>;

/// The places as `graph` makes them: a Python dict, in the order the keys were first set.
#[derive(Default)]
struct Places {
    list: Vec<Place>,
    at: HashMap<String, usize>,
}

impl Places {
    fn set(&mut self, p: Place) {
        match self.at.get(&p.key) {
            Some(&n) => self.list[n] = p,
            None => {
                self.at.insert(p.key.clone(), self.list.len());
                self.list.push(p);
            }
        }
    }

    /// The place of a room that a nav node is in (made on first use).
    fn of(&mut self, sp: &[Space], nav: &Nav, key: &str, node: Node) -> String {
        let z = node.2;
        let n = *nav.piece.get(&node).unwrap_or_else(|| panic!("{key}: nobody can stand at {node:?}"));
        let name = if n == 1 { format!("{key}@{z}") } else { format!("{key}@{z}.{n}") };
        if !self.at.contains_key(&name) {
            let kind = if z == room(sp, key).floor { "floor" } else { "deck" };
            self.set(Place { key: name.clone(), room: key.to_string(), level: z, kind });
        }
        name
    }
}

/// The places of a furnished level and the ways between them.
///
/// `sp`: the rooms; `net`: what joins them; `interiors`: every room's inside, built; `drops`:
/// (upper room, lower room, cells, open side or None); `cache`: each inside's standing blocks,
/// kept between calls.
pub fn graph(sp: &[Space], net: &Network, interiors: &[(String, Arc<Interior>)], drops: &[Sided], cache: &mut NavCache) -> Graph {
    let mut places = Places::default();
    let mut ways: Vec<Way> = Vec::new();
    let mut navs: HashMap<String, Arc<Nav>> = HashMap::new();
    // (room, side, a, level) -> (place key, node)
    let mut doors: HashMap<(&str, char, i32, i32), (String, Node)> = HashMap::new();
    for (key, it) in interiors {
        let nav = cache.entry(Arc::as_ptr(it) as usize).or_insert_with(|| Arc::new(Nav::new(it))).clone();
        navs.insert(key.clone(), nav.clone());
        let targets = it.door_nodes();
        for (d, node) in it.doors.iter().zip(&targets) {
            if let Some(node) = *node {
                doors.insert((key.as_str(), d.side, d.a, d.level), (places.of(sp, &nav, key, node), node));
            }
        }
        for r in &it.ramps {
            let (t, f) = (r.node(true), r.node(false));
            let (a, b) = ((t.0, t.1, r.hi), (f.0, f.1, r.lo));
            if nav.piece.contains_key(&a) && nav.piece.contains_key(&b) {
                let (foot, top) = (places.of(sp, &nav, key, b), places.of(sp, &nav, key, a));
                ways.push(Way { a: foot, b: top, kind: "ramp", length: RAMP_CELLS, pa: b, pb: a, oneway: false, route: None });
            }
        }
        let pods = it.pods.iter().filter(|p| it.access_of(&p.key).is_some());
        for (p, node) in pods.zip(&targets[it.doors.len()..]) {
            let mid = (p.x0 + p.x1 - 1, p.y0 + p.y1 - 1, p.floor);
            places.set(Place { key: p.key.clone(), room: p.key.clone(), level: p.floor, kind: "pod" });
            if let Some(node) = *node {
                ways.push(Way { a: places.of(sp, &nav, key, node), b: p.key.clone(), kind: "pod", length: 1.0, pa: node, pb: mid, oneway: false, route: None });
            }
        }
    }
    // a pod left without a way in still has its corridors
    for s in sp {
        if !s.host.is_empty() && !places.at.contains_key(&s.key) {
            places.set(Place { key: s.key.clone(), room: s.key.clone(), level: s.floor, kind: "pod" });
        }
    }
    for (i, r) in net.routes().iter().enumerate() {
        let end = |d: &Door| -> Option<(String, Node)> {
            let s = room(sp, &d.room);
            if !s.host.is_empty() {
                return Some((d.room.clone(), (s.x0 + s.x1 - 1, s.y0 + s.y1 - 1, s.floor)));
            }
            doors.get(&(d.room.as_str(), d.side, d.a, d.level)).cloned()
        };
        let (Some((a, pa)), Some((b, pb))) = (end(&r.a), end(&r.b)) else { continue };
        let kind = match r.pieces[0].kind {
            k @ ("stub" | "vent") => k,
            _ => "corridor",
        };
        let length = if kind == "stub" { r.cost } else { r.length().max(1.0) };
        ways.push(Way { a, b, kind, length, pa, pb, oneway: false, route: Some(i) });
    }
    for (up, low, (x0, y0, x1, y1), side) in drops {
        let (Some(side), Some((_, above)), Some(under)) = (*side, interiors.iter().find(|(k, _)| k == up), navs.get(low)) else { continue };
        let top = above.hole_approach(side);
        let land = under.nearest(x0 + x1 - 1, y0 + y1 - 1, room(sp, low).floor);
        let over = &navs[up];
        if let (true, Some(land)) = (over.piece.contains_key(&top), land) {
            let (a, b) = (places.of(sp, over, up, top), places.of(sp, under, low, land));
            ways.push(Way { a, b, kind: "drop", length: 0.0, pa: top, pb: land, oneway: true, route: None });
        }
    }
    Graph { places: places.list, ways, navs, at: places.at }
}

/// Some of a graph's places: whether each is among them, by its number, and how many are.
struct Reached {
    has: Vec<bool>,
    len: usize,
}

/// Places reached from `start` along the arcs in `out`, never by way number `skip` or through
/// the places in `block`. None as soon as `stop` is reached.
fn reach(out: &[Vec<(usize, usize)>], start: usize, skip: Option<usize>, stop: Option<usize>, block: Option<&[bool]>) -> Option<Reached> {
    let mut seen = Reached { has: vec![false; out.len()], len: 1 };
    seen.has[start] = true;
    let mut todo = vec![start];
    while let Some(k) = todo.pop() {
        for &(w, n) in &out[k] {
            if Some(w) != skip && !seen.has[n] && !block.is_some_and(|b| b[n]) {
                if Some(n) == stop {
                    return None;
                }
                seen.has[n] = true;
                seen.len += 1;
                todo.push(n);
            }
        }
    }
    Some(seen)
}

/// What `find` allows a dead end (its keyword arguments, with the Python's defaults).
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// The most cells of corridor an allowed one may have.
    pub corridor: i32,
    /// The most cells to its tip.
    pub walk: i32,
    /// The storeys at which a climb to nothing is too much.
    pub climb: i32,
    /// A part with a loop of its own and this many rooms is somewhere, not a dead end (what
    /// hangs off it is still looked at).
    pub district: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { corridor: 8, walk: 20, climb: 2, district: 4 }
    }
}

/// Every dead end of a graph: those not allowed first, the longest first. `sizes` is each
/// room's floor area, to say which looped part is the middle when two are as big (empty: none).
pub fn find(g: &Graph, lim: &Limits, sizes: &HashMap<String, i32>) -> Vec<DeadEnd> {
    let n = g.places.len();
    let key = |k: usize| g.places[k].key.as_str();
    // each way's two places, by number
    let ends: Vec<(usize, usize)> = g.ways.iter().map(|w| (g.number(&w.a), g.number(&w.b))).collect();
    let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for (i, (w, &(a, b))) in g.ways.iter().zip(&ends).enumerate() {
        out[a].push((i, b));
        if !w.oneway {
            out[b].push((i, a));
        }
    }
    // (way number, from, into, the places past it); and the ways with a way round
    let mut blind: Vec<(usize, usize, usize, Reached)> = Vec::new();
    let mut both: Vec<usize> = Vec::new();
    for (i, w) in g.ways.iter().enumerate() {
        if w.oneway {
            continue;
        }
        let mut round = 0;
        for (a, b) in [ends[i], (ends[i].1, ends[i].0)] {
            match reach(&out, b, Some(i), Some(a), None) {
                None => round += 1,
                Some(past) => blind.push((i, a, b, past)),
            }
        }
        if round == 2 {
            both.push(i);
        }
    }
    // the middle: the biggest part held together by ways that can be gone round
    let mut parent: Vec<usize> = (0..n).collect();
    fn top(parent: &mut [usize], mut k: usize) -> usize {
        while parent[k] != k {
            parent[k] = parent[parent[k]];
            k = parent[k];
        }
        k
    }
    for &i in &both {
        let onto = top(&mut parent, ends[i].1);
        let from = top(&mut parent, ends[i].0);
        parent[from] = onto;
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_of: HashMap<usize, usize> = HashMap::new();
    for k in 0..n {
        let at = *group_of.entry(top(&mut parent, k)).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[at].push(k);
    }
    let weight = |ks: &Vec<usize>| {
        let rooms: BTreeSet<&str> = ks.iter().map(|&k| g.places[k].room.as_str()).collect();
        let area: i64 = rooms.iter().map(|r| sizes.get(*r).copied().unwrap_or(0) as i64).sum();
        (rooms.len(), area, ks.len(), ks.iter().map(|&k| key(k)).min().expect("a group has a place"))
    };
    let middle: Option<usize> = py::first_max(groups.iter(), |ks| weight(ks)).map(|ks| ks[0]);
    blind.sort_by(|p, q| q.3.len.cmp(&p.3.len).then(p.0.cmp(&q.0)).then_with(|| key(p.2).cmp(key(q.2))));
    let mut found: Vec<DeadEnd> = Vec::new();
    let mut covered = vec![false; n];
    for (i, a, b, past) in &blind {
        if middle.is_some_and(|m| past.has[m]) || covered[*b] {
            continue;
        }
        let looped = both.iter().any(|&j| past.has[ends[j].0] && past.has[ends[j].1]);
        let rooms = rooms_in(g, past);
        if looped && rooms.len() >= lim.district {
            continue; // a district: only what hangs off it is a dead end
        }
        let mut e = measure(g, &out, *i, *a, *b, past, rooms, looped, lim.climb);
        if e.corridor > lim.corridor as f64 {
            e.why.push(format!("{} cells of corridor", py::round(e.corridor)));
        }
        if e.length > lim.walk as f64 {
            e.why.push(format!("{} cells to its tip", py::round(e.length)));
        }
        if e.climb >= lim.climb {
            e.why.push(format!("climbs {} storeys to nothing", e.climb));
        }
        e.allowed = e.why.is_empty();
        found.push(e);
        for (seen, &has) in covered.iter_mut().zip(&past.has) {
            *seen |= has;
        }
    }
    found.sort_by(|p, q| p.allowed.cmp(&q.allowed).then_with(|| py::cmp_f64(q.length, p.length)).then_with(|| p.root.cmp(&q.root)));
    found
}

/// Rooms every place of which is in `past`, sorted.
fn rooms_in(g: &Graph, past: &Reached) -> Vec<String> {
    let (mut mine, mut other): (BTreeSet<&str>, BTreeSet<&str>) = (BTreeSet::new(), BTreeSet::new());
    for (k, p) in g.places.iter().enumerate() {
        (if past.has[k] { &mut mine } else { &mut other }).insert(p.room.as_str());
    }
    mine.difference(&other).map(|r| r.to_string()).collect()
}

/// One dead end: the places past way `i`, going from place `a` into place `b`.
///
/// Two passes go along the ways the search took (`came`): a room counts for every place on the
/// way to it (`full`), and a deck's storeys are counted from the last room on the way to it
/// (`base`, `rise`). Both walk the places in the order the search settled them (`reached`), so
/// **a place is always measured after the place it was reached through, whatever their keys**:
/// the search settles a place only while working from one it has settled already.
///
/// What is given out (`places`, `tip`, `bare`, and the order `cut` and `leaf` are looked for
/// in) is still nearest first and then by key (`order`), as in the Python. The two orders are
/// the same unless a place is no further off than the one it was reached through and its key
/// comes first. Only a way of no length from the very block a place was entered by does that:
/// a drop, stepped off where its room was come into.
///
/// The Python (`deadends._measure`, frozen: it is the answer key's source) makes both passes
/// in the order by key, and still has the fault. There, for such a pair, a place that is no
/// room to be in is looked up before the one it was reached through (KeyError; this port used
/// to panic), and one that is a room does not count for the places before the one it was
/// reached through, which are taken for a climb to nothing unless another room counts for
/// them. Everywhere else the two give the same answer. No generated level has had such a pair
/// (none of 651 looked at when this was put right); the unit tests make both.
#[allow(clippy::too_many_arguments)]
fn measure(g: &Graph, out: &[Vec<(usize, usize)>], i: usize, a: usize, b: usize, past: &Reached, rooms: Vec<String>, looped: bool, limit: i32) -> DeadEnd {
    let n = g.places.len();
    let key = |k: usize| g.places[k].key.as_str();
    let corridor = |v: &Way| if CORRIDOR.contains(&v.kind) { v.length } else { 0.0 };
    let w = &g.ways[i];
    let root_port = g.port(w, key(a));
    // dist: cells walked to arrive in each place; port: where it is entered; came: the place before it
    let mut dist: Vec<Option<f64>> = vec![None; n];
    let mut cor = vec![0.0; n];
    let mut port: Vec<Node> = vec![(0, 0, 0); n];
    let mut came: Vec<Option<usize>> = vec![None; n];
    dist[b] = Some(w.length);
    cor[b] = corridor(w);
    port[b] = g.port(w, key(b));
    let mut heap: BinaryHeap<Reverse<(Cost, &str, usize)>> = BinaryHeap::from([Reverse((Cost(w.length), key(b), b))]);
    // the places as the search settles them: each once (no step is less than nothing, so none
    // is made nearer afterwards), and after the place before it, which was being worked from
    let mut reached: Vec<usize> = Vec::new();
    while let Some(Reverse((Cost(d), _, k))) = heap.pop() {
        if dist[k].is_some_and(|known| d > known) {
            continue;
        }
        reached.push(k);
        for &(j, m) in &out[k] {
            if j == i || !past.has[m] {
                continue;
            }
            let v = &g.ways[j];
            let step = g.inner(key(k), port[k], g.port(v, key(k))) + v.length;
            if d + step < dist[m].unwrap_or(1e9) {
                dist[m] = Some(d + step);
                port[m] = g.port(v, key(m));
                came[m] = Some(k);
                cor[m] = cor[k] + corridor(v);
                heap.push(Reverse((Cost(d + step), key(m), m)));
            }
        }
    }
    let far = |k: usize| dist[k].expect("a place of the dead end");
    // the same places as `reached`, nearest first and then by key: the order they are given out in
    let mut order: Vec<usize> = (0..n).filter(|&k| dist[k].is_some()).collect();
    order.sort_by(|&p, &q| py::cmp_f64(far(p), far(q)).then_with(|| key(p).cmp(key(q))));
    let mut idx = vec![0; n];
    for &k in &order {
        idx[k] = index(g.places[k].level) as i32;
    }
    let z0 = (if LEVELS.contains(&root_port.2) { index(root_port.2) } else { index(g.places[a].level) }) as i32;
    let mine: BTreeSet<&str> = rooms.iter().map(|r| r.as_str()).collect();
    // is this place a room to be in (a pod, or the floor of a room of the dead end), not just deck?
    let somewhere = |k: usize| {
        let p = &g.places[k];
        p.kind == "pod" || (p.kind == "floor" && mine.contains(p.room.as_str()))
    };
    // a room at this place, or past it
    let mut full = vec![false; n];
    for &k in &order {
        full[k] = somewhere(k);
    }
    for &k in reached.iter().rev() {
        if let (true, Some(q)) = (full[k], came[k]) {
            full[q] = true;
        }
    }
    let bare: Vec<usize> = order.iter().copied().filter(|&k| !full[k]).collect();
    // storeys from the last room on the way (or the root) to each place
    let (mut rise, mut base): (Vec<Option<i32>>, Vec<Option<i32>>) = (vec![None; n], vec![None; n]);
    for &k in &reached {
        let from = if somewhere(k) { idx[k] } else { came[k].map_or(z0, |q| base[q].expect("the place before it was settled first")) };
        base[k] = Some(from);
        rise[k] = Some((idx[k] - from).abs());
    }
    // per room, the highest storey a bare deck may stay on
    let mut cut: Vec<(String, Option<i32>)> = Vec::new();
    for &k in &bare {
        if rise[k].is_some_and(|r| r >= limit) {
            let name = &g.places[k].room;
            let keep = (0..n).filter(|&q| g.places[q].room == *name && (!past.has[q] || (dist[q].is_some() && full[q]) || rise[q].unwrap_or(0) < limit));
            let top = keep.map(|q| g.places[q].level).max();
            match cut.iter_mut().find(|(r, _)| r == name) {
                Some(entry) => entry.1 = top,
                None => cut.push((name.clone(), top)),
            }
        }
    }
    // the farthest room nothing else in here is reached through
    let mut leaf = String::new();
    for &k in order.iter().rev() {
        let name = &g.places[k].room;
        if !somewhere(k) || !mine.contains(name.as_str()) {
            continue;
        }
        let pods = hosted(g, name);
        let gone: Vec<bool> = (0..n).map(|q| past.has[q] && (g.places[q].room == *name || pods.contains(g.places[q].room.as_str()))).collect();
        let left = reach(out, a, None, None, Some(&gone)).expect("nowhere to stop");
        if (0..n).all(|q| !past.has[q] || gone[q] || left.has[q]) {
            leaf = name.clone();
            break;
        }
    }
    let routes: BTreeSet<usize> = g.ways.iter().filter(|v| past.has[g.number(&v.a)] || past.has[g.number(&v.b)]).filter_map(|v| v.route).collect();
    let tip = order[order.len() - 1];
    DeadEnd {
        root: key(a).to_string(),
        way: w.clone(),
        places: order.iter().map(|&k| key(k).to_string()).collect(),
        tip: key(tip).to_string(),
        length: far(tip),
        corridor: order.iter().map(|&k| cor[k]).fold(f64::NEG_INFINITY, f64::max),
        climb: bare.iter().filter_map(|&k| rise[k]).max().unwrap_or(0),
        storeys: order.iter().map(|&k| (idx[k] - z0).abs()).max().expect("a dead end has a place"),
        rooms,
        routes: routes.into_iter().collect(),
        looped,
        bare: bare.iter().map(|&k| key(k).to_string()).collect(),
        cut,
        leaf,
        allowed: true,
        why: Vec::new(),
    }
}

/// The pods that open onto this room's decks.
fn hosted<'a>(g: &'a Graph, name: &str) -> BTreeSet<&'a str> {
    g.ways.iter().filter(|w| w.kind == "pod" && g.place(&w.a).room == name).map(|w| g.place(&w.b).room.as_str()).collect()
}

/// Somewhere a dead end could be joined: (room, storey, other room, the storeys it may take the
/// door on, cells apart).
pub type Join = (String, i32, String, Vec<i32>, f64);

/// Where a dead end could be joined to the rest of the level, from the dead end's tip back
/// towards its root, the nearest rooms first (at most `each` for a place). The Python's defaults
/// are `REACH`, `STOREYS` and `EACH`.
///
/// `room` on `storey` is a place of the dead end that can take a door; the other room lies
/// within `reach` cells of it and has a door storey within `storeys` of it that is no part of
/// the dead end (so the room it hangs off will do, by another door: a loop out of a hall and
/// back). The gap is in cells, plus a ramp's run for each storey apart.
pub fn joins(g: &Graph, sp: &[Space], e: &DeadEnd, reach: i32, storeys: i32, each: usize) -> Vec<Join> {
    let whole: BTreeSet<&str> = e.rooms.iter().map(|r| r.as_str()).collect();
    let mut mine: HashMap<&str, BTreeSet<i32>> = HashMap::new();
    for k in &e.places {
        let p = g.place(k);
        mine.entry(p.room.as_str()).or_default().insert(p.level);
    }
    let mut out: Vec<Join> = Vec::new();
    let mut seen: BTreeSet<(&str, i32)> = BTreeSet::new();
    for k in e.places.iter().rev() {
        let p = g.place(k);
        let Some(s) = sp.iter().find(|s| s.key == p.room) else { continue };
        if !s.host.is_empty() || !s.door_levels().contains(&p.level) || !seen.insert((p.room.as_str(), p.level)) {
            continue;
        }
        let apart = |z: i32| (index(z) as i32 - index(p.level) as i32).abs();
        let mut near: Vec<(f64, &str, Vec<i32>)> = Vec::new();
        for t in sp {
            if t.key == s.key || whole.contains(t.key.as_str()) || !t.host.is_empty() {
                continue;
            }
            let (dx, dy) = rect_gap(s.rect(), t.rect());
            let gap = dx + dy;
            let own = mine.get(t.key.as_str());
            let levels: Vec<i32> = t.door_levels().into_iter().filter(|z| !own.is_some_and(|m| m.contains(z)) && apart(*z) <= storeys).collect();
            if gap > reach || levels.is_empty() {
                continue;
            }
            let up = levels.iter().map(|&z| apart(z)).min().expect("it has a storey");
            near.push((gap.max(5 * up) as f64 + 2.5 * up as f64, t.key.as_str(), levels));
        }
        near.sort_by(|p, q| py::cmp_f64(p.0, q.0).then_with(|| p.1.cmp(q.1)));
        for (sep, t, levels) in near.into_iter().take(each) {
            out.push((p.room.clone(), p.level, t.to_string(), levels, sep));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::tests::{room as lot, space};

    /// A row of a graph made by hand: a place, or a way.
    enum Row {
        Place(&'static str, i32, &'static str),
        Way(String, String, &'static str, f64, bool),
    }

    fn at(key: &'static str, z: i32, kind: &'static str) -> Row {
        Row::Place(key, z, kind)
    }

    fn way(a: &str, b: &str, kind: &'static str, length: f64) -> Row {
        Row::Way(a.into(), b.into(), kind, length, false)
    }

    /// A way that goes from a to b only.
    fn fall(a: &str, b: &str) -> Row {
        Row::Way(a.into(), b.into(), "drop", 0.0, true)
    }

    /// A graph of places by hand.
    fn places(rows: Vec<Row>) -> Graph {
        let (mut ps, mut ws): (Vec<Place>, Vec<Way>) = (Vec::new(), Vec::new());
        for row in rows {
            match row {
                Row::Place(key, z, kind) => ps.push(Place { key: key.into(), room: key.split('@').next().unwrap().into(), level: z, kind }),
                Row::Way(a, b, kind, length, oneway) => {
                    let z = |k: &str| ps.iter().find(|p| p.key == k).unwrap().level;
                    ws.push(Way { pa: (0, 0, z(&a)), pb: (0, 0, z(&b)), a, b, kind, length, oneway, route: None });
                }
            }
        }
        Graph::new(ps, ws, HashMap::new())
    }

    /// Rooms in a loop (a, b, c, ...): the middle of a level, which everything else hangs off.
    fn middle(n: usize) -> Vec<Row> {
        let keys = &["a@0", "b@0", "c@0", "m@0", "n@0", "o@0", "p@0", "q@0"][..n];
        let mut rows: Vec<Row> = keys.iter().map(|k| at(k, 0, "floor")).collect();
        rows.extend((0..n).map(|i| way(keys[i], keys[(i + 1) % n], "corridor", 6.0)));
        rows
    }

    fn with(mut rows: Vec<Row>, more: Vec<Row>) -> Graph {
        rows.extend(more);
        places(rows)
    }

    fn found(g: &Graph) -> Vec<DeadEnd> {
        find(g, &Limits::default(), &HashMap::new())
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_room_off_a_main_area_is_a_camp_and_one_down_a_long_corridor_is_not() {
        let g = with(middle(3), vec![at("d@0", 0, "floor"), at("e@0", 0, "floor"), way("a@0", "d@0", "corridor", 5.0), way("b@0", "e@0", "corridor", 12.0)]);
        let ends = found(&g);
        let (long, camp) = (&ends[0], &ends[1]); // the ones not allowed come first
        assert_eq!(ends.len(), 2);
        assert_eq!((camp.root.as_str(), camp.tip.as_str(), &camp.rooms, camp.allowed), ("a@0", "d@0", &names(&["d"]), true));
        assert_eq!((long.root.as_str(), &long.rooms, long.corridor, long.allowed), ("b@0", &names(&["e"]), 12.0, false));
        assert_eq!((&long.why, long.leaf.as_str(), long.cut.len()), (&names(&["12 cells of corridor"]), "e", 0));
        assert_eq!(long.describe(), "b@0 -> e@0 (e): 12 cells, 12 of corridor, 0 storeys  [12 cells of corridor]");
        assert_eq!(camp.describe(), "a@0 -> d@0 (d): 5 cells, 5 of corridor, 0 storeys");
        assert_eq!((camp.row().length, camp.row().allowed, long.row().why.len()), (5.0, true, 1));
        // the limits are the caller's
        assert!(find(&g, &Limits { corridor: 12, ..Limits::default() }, &HashMap::new())[0].allowed);
    }

    /// A hall off the middle with decks on three storeys, a ramp from each to the next.
    fn hall() -> Vec<Row> {
        vec![
            at("h@0", 0, "floor"),
            way("a@0", "h@0", "corridor", 3.0),
            way("h@0", "b@0", "corridor", 3.0),
            at("h@276", 276, "deck"),
            at("h@551", 551, "deck"),
            at("h@827", 827, "deck"),
            way("h@0", "h@276", "ramp", 4.0),
            way("h@276", "h@551", "ramp", 4.0),
            way("h@551", "h@827", "ramp", 4.0),
        ]
    }

    #[test]
    fn a_climb_to_nothing_is_cut_but_a_pod_two_ramps_up_is_a_lookout() {
        let ends = found(&with(middle(3), hall()));
        let e = &ends[0];
        assert_eq!(ends.len(), 1);
        assert_eq!((e.root.as_str(), e.tip.as_str(), e.rooms.len(), e.climb, e.allowed), ("h@0", "h@827", 0, 3, false));
        assert_eq!(e.cut, [("h".to_string(), Some(276))]); // a balcony may stay: one storey is no climb
        assert_eq!(e.why, ["climbs 3 storeys to nothing"]);
        assert_eq!(e.describe(), "h@0 -> h@827 (decks): 12 cells, 0 of corridor, 3 storeys  [climbs 3 storeys to nothing]");
        let pod = || vec![at("p", 551, "pod"), way("h@551", "p", "pod", 1.0)];
        let mut rows = hall();
        rows.extend(pod());
        let ends = found(&with(middle(3), rows));
        let e = &ends[0];
        // only the deck above the pod goes
        assert_eq!((&e.rooms, &e.bare, &e.cut), (&names(&["p"]), &names(&["h@827"]), &vec![("h".to_string(), Some(551))]));
        // ... and without it
        let mut rows = hall();
        rows.remove(8);
        rows.extend(pod());
        let ends = found(&with(middle(3), rows));
        let e = &ends[0];
        assert_eq!((e.tip.as_str(), e.length, e.climb, e.storeys, e.allowed), ("p", 9.0, 0, 2, true));
    }

    #[test]
    fn a_room_at_the_top_of_a_long_climb_is_too_far() {
        let keys: Vec<&'static str> = vec!["s@0", "s@276", "s@551", "s@827", "s@1102", "s@1378", "s@1654"];
        let rows = || {
            let mut rows = vec![at("s@0", 0, "floor"), way("a@0", "s@0", "corridor", 3.0), way("s@0", "b@0", "corridor", 3.0)];
            for (n, hi) in keys.iter().enumerate().skip(1) {
                rows.extend([at(hi, LEVELS[n + 1], "deck"), way(keys[n - 1], hi, "ramp", 4.0)]);
            }
            rows.extend([at("loft@1654", 1654, "floor"), way("s@1654", "loft@1654", "stub", 2.0)]);
            rows
        };
        let ends = found(&with(middle(3), rows()));
        let e = &ends[0];
        assert_eq!(ends.len(), 1);
        assert_eq!((&e.rooms, e.length, e.storeys, e.climb), (&names(&["loft"]), 26.0, 6, 0));
        assert_eq!((e.allowed, &e.why, e.leaf.as_str()), (false, &names(&["26 cells to its tip"]), "loft"));
        assert!(find(&with(middle(3), rows()), &Limits { walk: 30, ..Limits::default() }, &HashMap::new())[0].allowed);
    }

    #[test]
    fn a_drop_is_a_way_out_but_not_a_way_back() {
        let spur = || vec![at("u@276", 276, "floor"), way("a@0", "u@276", "corridor", 12.0)];
        assert_eq!(found(&with(middle(3), spur())).len(), 1);
        // in by the door, out by the hole
        let mut rows = spur();
        rows.push(fall("u@276", "b@0"));
        assert!(found(&with(middle(3), rows)).is_empty());
        let mut rows = vec![at("d@0", 0, "floor")];
        rows.extend(spur());
        rows.extend([way("a@0", "d@0", "corridor", 12.0), fall("u@276", "d@0"), way("u@276", "b@0", "corridor", 5.0)]);
        let ends = found(&with(middle(3), rows));
        // dropped into, it is still walked out of the way in
        assert_eq!((ends.len(), &ends[0].rooms), (1, &names(&["d"])));
    }

    #[test]
    fn a_place_is_measured_after_the_one_it_was_reached_through_whatever_their_keys() {
        // u is entered at the very block of its hole, so the floor under the hole is no further
        // off than u, and its key comes first. It is a piece of c's floor walled off from the
        // rest, so c is not wholly in the dead end and the piece is no room to be in: its storeys
        // are counted from u, which must have been measured by then
        let g = with(middle(3), vec![at("u@276", 276, "floor"), at("c@0.2", 0, "floor"), way("a@0", "u@276", "corridor", 5.0), fall("u@276", "c@0.2")]);
        let ends = found(&g);
        let e = &ends[0];
        assert_eq!((ends.len(), e.root.as_str(), &e.rooms, e.length, e.corridor), (1, "a@0", &names(&["u"]), 5.0, 5.0));
        // listed nearest first and then by key, as ever; the tip is the last of them
        assert_eq!((&e.places, e.tip.as_str()), (&names(&["c@0.2", "u@276"]), "u@276"));
        // a storey down from the room to a floor with no room at or past it
        assert_eq!((&e.bare, e.climb, e.storeys, e.cut.len(), e.leaf.as_str(), e.allowed), (&names(&["c@0.2"]), 1, 1, 0, "", true));
        // two storeys down is too far to nothing
        let g = with(middle(3), vec![at("u@551", 551, "floor"), at("c@0.2", 0, "floor"), way("a@0", "u@551", "corridor", 5.0), fall("u@551", "c@0.2")]);
        let ends = found(&g);
        assert_eq!((ends[0].climb, ends[0].storeys, &ends[0].why), (2, 2, &names(&["climbs 2 storeys to nothing"])));
    }

    #[test]
    fn a_room_dropped_into_at_no_cost_counts_for_every_deck_on_the_way_to_it() {
        // up c's decks by a ramp to a hole at its top, and down the hole into b2, whose key comes
        // before the deck's: b2 is a room past both decks, so neither is a climb to nothing
        let rows = vec![
            at("c@276", 276, "deck"),
            at("c@551", 551, "deck"),
            at("b2@276", 276, "floor"),
            way("a@0", "c@276", "corridor", 3.0),
            way("c@276", "c@551", "ramp", 4.0),
            fall("c@551", "b2@276"),
        ];
        let ends = found(&with(middle(3), rows));
        let e = &ends[0];
        assert_eq!((ends.len(), e.root.as_str(), &e.rooms, e.length), (1, "a@0", &names(&["b2"]), 7.0));
        assert_eq!((&e.places, e.tip.as_str()), (&names(&["c@276", "b2@276", "c@551"]), "c@551"));
        assert_eq!((e.bare.len(), e.climb, e.storeys, e.cut.len(), e.allowed), (0, 0, 2, 0, true));
    }

    #[test]
    fn rooms_in_a_loop_of_their_own_are_a_district_if_there_are_enough_of_them() {
        let ring = |n: usize| {
            let keys = &["r0@0", "r1@0", "r2@0", "r3@0"][..n];
            let mut rows: Vec<Row> = keys.iter().map(|k| at(k, 0, "floor")).collect();
            rows.push(way("a@0", keys[0], "corridor", 12.0));
            rows.extend((0..n).map(|i| way(keys[i], keys[(i + 1) % n], "corridor", 4.0)));
            rows
        };
        assert!(found(&with(middle(5), ring(4))).is_empty());
        let ends = found(&with(middle(5), ring(3)));
        let e = &ends[0];
        assert_eq!((ends.len(), &e.rooms, e.looped, e.allowed), (1, &names(&["r0", "r1", "r2"]), true, false));
        // what hangs off a district is still looked at
        let mut hang = ring(4);
        hang.extend([at("x@0", 0, "floor"), way("r2@0", "x@0", "corridor", 12.0)]);
        let ends = found(&with(middle(5), hang));
        assert_eq!((ends.len(), ends[0].root.as_str(), &ends[0].rooms), (1, "r2@0", &names(&["x"])));
        // the bigger loop is the middle; the other hangs off it
        let ends = found(&with(middle(3), ring(4)));
        assert_eq!((ends.len(), ends[0].root.as_str(), &ends[0].rooms), (1, "r0@0", &names(&["a", "b", "c"])));
        // of two loops with as many rooms, the one with more floor is the middle
        let sizes: HashMap<String, i32> = [("a", 100), ("b", 100), ("c", 100)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let mut rows = vec![at("x@0", 0, "floor"), at("y@0", 0, "floor"), at("z@0", 0, "floor")];
        rows.extend([way("x@0", "y@0", "corridor", 4.0), way("y@0", "z@0", "corridor", 4.0), way("z@0", "x@0", "corridor", 4.0)]);
        rows.push(way("a@0", "x@0", "corridor", 12.0));
        let g = with(middle(3), rows);
        assert_eq!(find(&g, &Limits::default(), &sizes)[0].rooms, ["x", "y", "z"]);
        // and of two as big in every way, the one whose least place comes last
        assert_eq!(found(&g)[0].rooms, ["a", "b", "c"]);
    }

    #[test]
    fn joins_are_listed_from_the_tip_to_rooms_that_are_no_part_of_the_dead_end() {
        let sp = vec![
            lot("a", (0, 0, 6, 6)),
            lot("b", (0, 10, 6, 16)),
            lot("c", (10, 10, 16, 16)),
            lot("d", (10, 0, 14, 4)),
            lot("e", (18, 0, 22, 4)),
            lot("far", (60, 60, 66, 66)),
            space("up", (24, 0, 28, 4), 276, 512, "medium"),
        ];
        let g = with(middle(3), vec![at("d@0", 0, "floor"), at("e@0", 0, "floor"), way("a@0", "d@0", "corridor", 6.0), way("d@0", "e@0", "corridor", 6.0)]);
        let ends = found(&g);
        let e = &ends[0];
        assert_eq!((ends.len(), &e.rooms, e.tip.as_str(), e.leaf.as_str()), (1, &names(&["d", "e"]), "e@0", "e"));
        let list = joins(&g, &sp, e, REACH, STOREYS, EACH);
        assert_eq!(list[0].0, "e"); // the tip first
        assert_eq!(list.iter().map(|j| j.0.as_str()).collect::<BTreeSet<_>>(), BTreeSet::from(["d", "e"]));
        // not itself, and not across the level
        assert!(list.iter().all(|j| !["d", "e", "far"].contains(&j.2.as_str())));
        // the room it hangs off will do, by another door
        assert!(list.contains(&("d".to_string(), 0, "a".to_string(), vec![0], 4.0)));
        // a storey up: a ramp's run further off
        let up = list.iter().find(|j| j.2 == "up").unwrap();
        assert_eq!((up.0.as_str(), &up.3, up.4), ("e", &vec![276], 7.5));
        // at most `each` for a place, the nearest first
        let pairs = |list: Vec<Join>| list.into_iter().map(|j| (j.0, j.2)).collect::<Vec<_>>();
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(pairs(joins(&g, &sp, e, REACH, STOREYS, 1)), [pair("e", "up"), pair("d", "a")]);
        // and only as far off as asked, or as many storeys
        assert_eq!(pairs(joins(&g, &sp, e, 3, STOREYS, EACH)), [pair("e", "up")]);
        assert!(joins(&g, &sp, e, REACH, 0, EACH).iter().all(|j| j.2 != "up"));
    }

    #[test]
    fn a_way_meets_each_place_at_its_own_port_and_a_pod_is_crossed_in_a_straight_line() {
        let g = places(vec![at("h@0", 0, "floor"), at("p", 276, "pod"), way("h@0", "p", "pod", 1.0)]);
        let w = &g.ways[0];
        assert_eq!((g.port(w, "h@0"), g.port(w, "p")), ((0, 0, 0), (0, 0, 276)));
        assert_eq!(g.inner("p", (0, 0, 276), (0, 0, 276)), 0.0);
        assert_eq!(g.inner("p", (0, 0, 276), (3, 4, 276)), 3.5);
        assert_eq!((g.places().len(), g.number("p"), g.place("p").kind), (2, 1, "pod"));
    }
}
