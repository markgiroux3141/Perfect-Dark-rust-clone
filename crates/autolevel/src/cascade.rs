//! The cascade shown: the editing roadmap's item 7 (docs/AUTOLEVEL.md, "The cascade shown").
//! New here; the Python has none.
//!
//! After an edit the level is made again from the first stage it touches, and much can follow
//! from one change: corridors re-routed round a moved room, a hall's decks trimmed where a ramp
//! now lands. `diff` compares the level before with the one after and says what changed, each
//! change with where it was (a ghost), where it is now (lit), an arrow where a thing moved, and
//! words ("corridor medium1-vast re-routed, +6 cells").
//!
//! Things are matched by what names them for good: rooms by key, corridors and vents by the two
//! rooms they join (never by their own keys, which are dealt out again on every remake), a
//! room's inside by the room and the storey, windows by their hall and wall, drops by their two
//! rooms, grates by the room under them. Where two of a kind could be the same thing moved (a
//! pillar gone here, one of its size new there), the nearest are paired and drawn with an arrow.

use crate::cells::{rect_cells, runs, Cell, Cells, Rect};
use crate::level::Level;
use crate::route::Route;
use crate::space::{Space, SIDES};
use std::collections::{BTreeMap, BTreeSet};

/// How high a thing on a storey's floor is drawn (a storey's ceiling over its floor).
const STOREY: i32 = 236;

/// What became of a thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    New,
    Gone,
    /// The same thing somewhere else (an arrow from where it was).
    Moved,
    /// The same thing in place, made differently (its cells, its kind, its height).
    Changed,
}

/// A place a change is drawn: a box in half cells, the storey it is on, and the heights it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    /// (x0, y0, x1, y1), half cells.
    pub rect: Rect,
    /// The storey whose plan shows it.
    pub z: i32,
    /// Its heights, units: low and high.
    pub lo: i32,
    pub hi: i32,
}

impl Spot {
    /// The spot in cells, as the plan draws: (x0, y0, x1, y1).
    pub fn cells(&self) -> (f32, f32, f32, f32) {
        let (x0, y0, x1, y1) = self.rect;
        (x0 as f32 / 2.0, y0 as f32 / 2.0, x1 as f32 / 2.0, y1 as f32 / 2.0)
    }

    /// Its middle in cells, and its low height.
    pub fn middle(&self) -> (f32, f32, i32) {
        let (x0, y0, x1, y1) = self.cells();
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0, self.lo)
    }
}

/// One thing that changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// Whose it is: a room's key, or a corridor's two rooms ("medium1-vast").
    pub owner: String,
    /// What it is: "room", "corridor", "vent", "deck", "ramp", "cover", "pit", "inside",
    /// "window", "drop" or "grate".
    pub what: &'static str,
    pub how: How,
    /// Where it was (drawn as a ghost) and where it is now (lit).
    pub was: Vec<Spot>,
    pub now: Vec<Spot>,
    /// A thing moved: from its old middle to its new one, (x, y) in cells and a height.
    pub arrow: Option<((f32, f32, i32), (f32, f32, i32))>,
    /// What happened, in words.
    pub words: String,
}

impl Change {
    /// The rooms it concerns: its owner, or a corridor's two.
    pub fn rooms(&self) -> Vec<&str> {
        self.owner.split('-').collect()
    }
}

/// Everything that changed between two levels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cascade {
    pub changes: Vec<Change>,
}

impl Cascade {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The changes in words, a line each; where a room has more than three of one kind, one
    /// line counts them ("vast's cover: 3 new, 2 gone, 1 moved").
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut done: BTreeSet<(&str, &str)> = BTreeSet::new();
        for c in &self.changes {
            let all: Vec<&Change> = self.changes.iter().filter(|d| d.owner == c.owner && d.what == c.what).collect();
            if all.len() <= 3 {
                out.push(c.words.clone());
            } else if done.insert((c.owner.as_str(), c.what)) {
                let count = |how: How| all.iter().filter(|d| d.how == how).count();
                let parts: Vec<String> = [(How::New, "new"), (How::Gone, "gone"), (How::Moved, "moved"), (How::Changed, "changed")]
                    .into_iter()
                    .filter(|(h, _)| count(*h) > 0)
                    .map(|(h, w)| format!("{} {w}", count(h)))
                    .collect();
                out.push(format!("{}'s {}: {}", c.owner, plural(c.what), parts.join(", ")));
            }
        }
        out
    }

    /// One line for a status bar: "3 changes: vast moved 2 east; ..." (at most `n` named).
    pub fn summary(&self, n: usize) -> String {
        let lines = self.lines();
        if lines.is_empty() {
            return "nothing else changed".to_string();
        }
        let more = if lines.len() > n { format!("; and {} more", lines.len() - n) } else { String::new() };
        let shown: Vec<&str> = lines.iter().take(n).map(String::as_str).collect();
        format!("{} change{}: {}{more}", lines.len(), if lines.len() == 1 { "" } else { "s" }, shown.join("; "))
    }

    /// The rooms any change concerns.
    pub fn rooms(&self) -> BTreeSet<&str> {
        self.changes.iter().flat_map(|c| c.rooms()).collect()
    }

    /// The changes that concern one room (its own, and the corridors to it).
    pub fn of<'a>(&'a self, room: &'a str) -> impl Iterator<Item = &'a Change> + 'a {
        self.changes.iter().filter(move |c| c.rooms().contains(&room))
    }

    /// Only the changes inside a box of cells (x0, y0, x1, y1), and those owned by `room`: what a
    /// small picture of one room shows.
    pub fn within(&self, room: &str, (x0, y0, x1, y1): Rect) -> Cascade {
        let meets = |s: &Spot| s.rect.0 < 2 * x1 && 2 * x0 < s.rect.2 && s.rect.1 < 2 * y1 && 2 * y0 < s.rect.3;
        let changes = self.changes.iter().filter(|c| c.owner == room || c.was.iter().chain(&c.now).any(meets)).cloned().collect();
        Cascade { changes }
    }
}

fn plural(what: &str) -> String {
    match what {
        "cover" | "inside" => what.to_string(),
        _ => format!("{what}s"),
    }
}

/// What changed from `before` to `after`.
pub fn diff(before: &Level, after: &Level) -> Cascade {
    let mut out = Vec::new();
    rooms(before, after, &mut out);
    routes(before, after, &mut out);
    insides(before, after, &mut out);
    windows(before, after, &mut out);
    drops(before, after, &mut out);
    Cascade { changes: out }
}

// ---------------------------------------------------------------- rooms

fn room_spot(s: &Space) -> Spot {
    let (x0, y0, x1, y1) = s.rect_h();
    Spot { rect: (x0, y0, x1, y1), z: s.floor, lo: s.floor, hi: s.top }
}

fn room_words(s: &Space) -> String {
    format!("{} x {}, {} to {}", s.x1 - s.x0, s.y1 - s.y0, s.floor, s.top)
}

/// Cells along x and y as words: "3 east, 2 north".
fn shift(dx: i32, dy: i32) -> String {
    let mut parts = Vec::new();
    if dx != 0 {
        parts.push(format!("{} {}", dx.abs(), if dx > 0 { "east" } else { "west" }));
    }
    if dy != 0 {
        parts.push(format!("{} {}", dy.abs(), if dy > 0 { "north" } else { "south" }));
    }
    parts.join(", ")
}

fn rooms(a: &Level, b: &Level, out: &mut Vec<Change>) {
    let was: BTreeMap<&str, &Space> = a.spaces().iter().map(|s| (s.key.as_str(), s)).collect();
    let now: BTreeMap<&str, &Space> = b.spaces().iter().map(|s| (s.key.as_str(), s)).collect();
    let pod = |s: &Space| if s.host.is_empty() { String::new() } else { format!(" (a pod in {})", s.host) };
    for s in b.spaces() {
        let Some(o) = was.get(s.key.as_str()) else {
            let words = format!("{} is new{}: {}", s.key, pod(s), room_words(s));
            out.push(Change { owner: s.key.clone(), what: "room", how: How::New, was: vec![], now: vec![room_spot(s)], arrow: None, words });
            continue;
        };
        let (w0, d0, w1, d1) = (o.x1 - o.x0, o.y1 - o.y0, s.x1 - s.x0, s.y1 - s.y0);
        let mut said = Vec::new();
        if (o.x0, o.y0) != (s.x0, s.y0) && (w0, d0) == (w1, d1) {
            said.push(format!("moved {}", shift(s.x0 - o.x0, s.y0 - o.y0)));
        } else if o.rect() != s.rect() {
            said.push(format!("is {w1} x {d1} at {}, {} (was {w0} x {d0} at {}, {})", s.x0, s.y0, o.x0, o.y0));
        }
        if (o.floor, o.top) != (s.floor, s.top) {
            said.push(format!("stands {} to {} (was {} to {})", s.floor, s.top, o.floor, o.top));
        }
        if o.kind != s.kind {
            said.push(format!("is a {} (was a {})", s.kind, o.kind));
        }
        if said.is_empty() {
            continue;
        }
        let moved = o.rect() != s.rect() || o.floor != s.floor;
        let arrow = moved.then(|| (room_spot(o).middle(), room_spot(s).middle()));
        let how = if (w0, d0, o.top - o.floor) == (w1, d1, s.top - s.floor) && moved { How::Moved } else { How::Changed };
        let words = format!("{} {}", s.key, said.join(", "));
        out.push(Change { owner: s.key.clone(), what: "room", how, was: vec![room_spot(o)], now: vec![room_spot(s)], arrow, words });
    }
    for o in a.spaces().iter().filter(|o| !now.contains_key(o.key.as_str())) {
        let words = format!("{} is gone{}", o.key, pod(o));
        out.push(Change { owner: o.key.clone(), what: "room", how: How::Gone, was: vec![room_spot(o)], now: vec![], arrow: None, words });
    }
}

// ---------------------------------------------------------------- corridors and vents

/// A route's two rooms in order, joined: its name for good.
fn ends(r: &Route) -> String {
    let (a, b) = (r.a.room.as_str(), r.b.room.as_str());
    if a <= b {
        format!("{a}-{b}")
    } else {
        format!("{b}-{a}")
    }
}

fn kind(r: &Route) -> &'static str {
    if r.pieces.iter().any(|p| p.kind == "vent") {
        "vent"
    } else {
        "corridor"
    }
}

/// The cells a route covers, by storey: its flat cells, its ramps' and its ducts' (on the
/// storey of their foot).
fn footprint(r: &Route) -> BTreeMap<i32, Cells> {
    let mut out: BTreeMap<i32, Cells> = BTreeMap::new();
    for pc in &r.pieces {
        for (z, cells) in &pc.cells {
            out.entry(*z).or_default().extend(cells.iter().copied());
        }
        if let (Some((foot, _)), Some(rect)) = (pc.ramp, pc.ramp_cells) {
            out.entry(foot.2).or_default().extend(rect_cells(rect));
        }
        for ((foot, top), rect) in &pc.ducts {
            out.entry(foot.2.min(top.2)).or_default().extend(rect_cells(*rect));
        }
    }
    out
}

fn count(f: &BTreeMap<i32, Cells>) -> usize {
    f.values().map(BTreeSet::len).sum()
}

/// Cells (whole) on storeys as spots: a spot a row of cells.
fn cell_spots(f: &BTreeMap<i32, Cells>) -> Vec<Spot> {
    f.iter()
        .flat_map(|(&z, cells)| runs(cells).into_iter().map(move |[y, x0, x1]| Spot { rect: (2 * x0, 2 * y, 2 * x1, 2 * y + 2), z, lo: z, hi: z + STOREY }))
        .collect()
}

/// Half cells on a storey as spots, `lo` to `hi` high.
fn half_spots(cells: &Cells, z: i32, lo: i32, hi: i32) -> Vec<Spot> {
    runs(cells).into_iter().map(|[y, x0, x1]| Spot { rect: (x0, y, x1, y + 1), z, lo, hi }).collect()
}

fn minus(a: &BTreeMap<i32, Cells>, b: &BTreeMap<i32, Cells>) -> BTreeMap<i32, Cells> {
    a.iter().map(|(z, cells)| (*z, b.get(z).map_or_else(|| cells.clone(), |o| cells - o))).filter(|(_, cells)| !cells.is_empty()).collect()
}

fn shared(a: &BTreeMap<i32, Cells>, b: &BTreeMap<i32, Cells>) -> usize {
    a.iter().map(|(z, cells)| b.get(z).map_or(0, |o| cells.intersection(o).count())).sum()
}

fn routes(a: &Level, b: &Level, out: &mut Vec<Change>) {
    let group = |lv: &Level| {
        let mut g: BTreeMap<String, Vec<(&'static str, BTreeMap<i32, Cells>)>> = BTreeMap::new();
        for r in lv.net.routes() {
            g.entry(ends(r)).or_default().push((kind(r), footprint(r)));
        }
        g
    };
    let (was, now) = (group(a), group(b));
    let names: BTreeSet<&String> = was.keys().chain(now.keys()).collect();
    for name in names {
        let mut olds: Vec<_> = was.get(name).cloned().unwrap_or_default();
        let mut news: Vec<_> = now.get(name).cloned().unwrap_or_default();
        // the same route on both sides is no change
        news.retain(|n| match olds.iter().position(|o| o == n) {
            Some(i) => {
                olds.remove(i);
                false
            }
            None => true,
        });
        // the rest paired by the cells they share, most first
        while !olds.is_empty() && !news.is_empty() {
            let (i, j) = (0..olds.len())
                .flat_map(|i| (0..news.len()).map(move |j| (i, j)))
                .max_by_key(|&(i, j)| (shared(&olds[i].1, &news[j].1), std::cmp::Reverse((i, j))))
                .unwrap();
            let ((k0, f0), (k1, f1)) = (olds.remove(i), news.remove(j));
            let grown = count(&f1) as i64 - count(&f0) as i64;
            let mut words = if k0 == k1 { format!("{k1} {name} re-routed") } else { format!("{name} is a {k1} now (was a {k0})") };
            if grown != 0 {
                words += &format!(", {grown:+} cells");
            }
            out.push(Change {
                owner: name.clone(),
                what: k1,
                how: How::Changed,
                was: cell_spots(&minus(&f0, &f1)),
                now: cell_spots(&minus(&f1, &f0)),
                arrow: None,
                words,
            });
        }
        for (k, f) in news {
            let words = format!("new {k} {name}, {} cells", count(&f));
            out.push(Change { owner: name.clone(), what: k, how: How::New, was: vec![], now: cell_spots(&f), arrow: None, words });
        }
        for (k, f) in olds {
            let words = format!("{k} {name} is gone");
            out.push(Change { owner: name.clone(), what: k, how: How::Gone, was: cell_spots(&f), now: vec![], arrow: None, words });
        }
    }
}

// ---------------------------------------------------------------- a room's inside

/// Pairs things gone with things new that could be the same thing moved (`same`), nearest first;
/// returns (moved pairs, new, gone).
fn pair<T: Clone>(gone: Vec<T>, new: Vec<T>, same: impl Fn(&T, &T) -> bool, at: impl Fn(&T) -> (f32, f32)) -> (Vec<(T, T)>, Vec<T>, Vec<T>) {
    let (mut gone, mut new) = (gone, new);
    let mut moved = Vec::new();
    loop {
        let best = (0..gone.len()).flat_map(|i| (0..new.len()).map(move |j| (i, j))).filter(|&(i, j)| same(&gone[i], &new[j])).min_by(|&(i, j), &(k, l)| {
            let d = |i: usize, j: usize| {
                let (p, q) = (at(&gone[i]), at(&new[j]));
                (p.0 - q.0).abs() + (p.1 - q.1).abs()
            };
            d(i, j).total_cmp(&d(k, l))
        });
        let Some((i, j)) = best else { break };
        moved.push((gone.remove(i), new.remove(j)));
    }
    (moved, new, gone)
}

/// Things in one list and not the other, each way: (new, gone).
fn both_ways<T: Clone + PartialEq>(was: &[T], now: &[T]) -> (Vec<T>, Vec<T>) {
    (now.iter().filter(|t| !was.contains(t)).cloned().collect(), was.iter().filter(|t| !now.contains(t)).cloned().collect())
}

fn mid((x0, y0, x1, y1): Rect) -> (f32, f32) {
    ((x0 + x1) as f32 / 4.0, (y0 + y1) as f32 / 4.0)
}

/// Half cells as cells, in words: "41" or "41.5".
fn half(h: i32) -> String {
    if h % 2 == 0 {
        format!("{}", h / 2)
    } else {
        format!("{:.1}", h as f32 / 2.0)
    }
}

/// The low corner of a box of half cells, in cells, as words: "41, 25".
fn corner(r: Rect) -> String {
    format!("{}, {}", half(r.0), half(r.1))
}

/// How a room that moved whole moved: (x, y) in half cells, and storeys up. A room's inside is
/// compared in its own frame, so what moved with it is no change.
struct Frame {
    dx: i32,
    dy: i32,
    dz: isize,
}

impl Frame {
    fn of(o: &Space, s: &Space) -> Frame {
        let whole = (o.x1 - o.x0, o.y1 - o.y0) == (s.x1 - s.x0, s.y1 - s.y0);
        if !whole {
            return Frame { dx: 0, dy: 0, dz: 0 };
        }
        let dz = crate::storeys::index(s.floor) as isize - crate::storeys::index(o.floor) as isize;
        Frame { dx: 2 * (s.x0 - o.x0), dy: 2 * (s.y0 - o.y0), dz }
    }

    fn z(&self, z: i32) -> i32 {
        let i = crate::storeys::index(z) as isize + self.dz;
        if self.dz == 0 || i < 0 || i as usize >= crate::storeys::LEVELS.len() {
            return z + 276 * self.dz as i32;
        }
        crate::storeys::LEVELS[i as usize]
    }

    fn rect(&self, (x0, y0, x1, y1): Rect) -> Rect {
        (x0 + self.dx, y0 + self.dy, x1 + self.dx, y1 + self.dy)
    }

    fn cells(&self, cells: &Cells) -> Cells {
        cells.iter().map(|&(x, y)| (x + self.dx, y + self.dy)).collect()
    }
}

fn insides(a: &Level, b: &Level, out: &mut Vec<Change>) {
    for (key, it) in &b.interiors {
        let Some((_, o)) = a.interiors.iter().find(|(k, _)| k == key) else { continue };
        let floor = it.s.floor;
        let f = Frame::of(&o.s, &it.s);
        // what it is laid out as
        let lay = |i: &crate::furnish::Interior| i.layout.unwrap_or(if i.tower.is_empty() { "none" } else { i.tower });
        let mut said = Vec::new();
        // (a layout's name alone may change with nothing built otherwise: said only with its decks)
        let decks_moved = o.deck.iter().map(|(&z, cells)| (f.z(z), f.cells(cells))).collect::<BTreeMap<i32, Cells>>() != it.deck;
        if lay(o) != lay(it) && decks_moved {
            said.push(format!("catwalks are {} now (were {})", lay(it), lay(o)));
        }
        let shape = |i: &crate::furnish::Interior| match i.cuts.len() {
            0 => "a rectangle".to_string(),
            1 => "an L".to_string(),
            n => format!("shaped ({n} cuts)"),
        };
        if o.cuts.iter().map(|&r| f.rect(r)).collect::<Vec<_>>() != it.cuts {
            said.push(format!("is {} now (was {})", shape(it), shape(o)));
        }
        if o.flood != it.flood {
            said.push(if it.flood.is_some() { "is flooded now".to_string() } else { "is dry now".to_string() });
        }
        if o.angled.len() != it.angled.len() || o.islands.iter().map(|&(z, r)| (f.z(z), f.rect(r))).collect::<Vec<_>>() != it.islands {
            said.push(format!("has {} angled walkways (had {})", it.angled.len(), o.angled.len()));
        }
        if !said.is_empty() {
            let spot = room_spot(&it.s);
            out.push(Change {
                owner: key.clone(),
                what: "inside",
                how: How::Changed,
                was: vec![],
                now: vec![spot],
                arrow: None,
                words: format!("{key}'s {}", said.join(", ")),
            });
        }
        // decks, storey by storey
        let old_deck: BTreeMap<i32, Cells> = o.deck.iter().map(|(&z, cells)| (f.z(z), f.cells(cells))).collect();
        let storeys: BTreeSet<i32> = old_deck.keys().chain(it.deck.keys()).copied().collect();
        let empty = Cells::new();
        for z in storeys {
            let (d0, d1) = (old_deck.get(&z).unwrap_or(&empty), it.deck.get(&z).unwrap_or(&empty));
            if d0 == d1 {
                continue;
            }
            let (more, less) = (d1 - d0, d0 - d1);
            let cells = |n: usize| (n as f32 / 4.0).round() as i64;
            let words = match (more.is_empty(), less.is_empty()) {
                (false, true) => format!("{key}'s deck on {z}: +{} cells", cells(more.len()).max(1)),
                (true, false) => format!("{key}'s deck on {z}: -{} cells", cells(less.len()).max(1)),
                _ => format!("{key}'s deck on {z} reshaped: +{}, -{} cells", cells(more.len()), cells(less.len())),
            };
            let how = if d0.is_empty() {
                How::New
            } else if d1.is_empty() {
                How::Gone
            } else {
                How::Changed
            };
            out.push(Change {
                owner: key.clone(),
                what: "deck",
                how,
                was: half_spots(&less, z, z - 20, z),
                now: half_spots(&more, z, z - 20, z),
                arrow: None,
                words,
            });
        }
        // ramps: (lo, hi, rect, way)
        let ramp = |r: &crate::furnish::RampIn| (r.lo, r.hi, r.rect, r.d);
        let old: Vec<_> = o.ramps.iter().map(|r| (f.z(r.lo), f.z(r.hi), f.rect(r.rect), r.d)).collect();
        let (new, gone) = both_ways(&old, &it.ramps.iter().map(ramp).collect::<Vec<_>>());
        let (moved, new, gone) = pair(gone, new, |p, q| (p.0, p.1) == (q.0, q.1), |r| mid(r.2));
        let spot = |r: &(i32, i32, Rect, usize)| Spot { rect: r.2, z: r.0, lo: r.0, hi: r.1 };
        for (p, q) in moved {
            let (how, words) = if corner(p.2) == corner(q.2) {
                (How::Changed, format!("{key}: the ramp {} to {} at {} is laid another way", p.0, p.1, corner(q.2)))
            } else {
                (How::Moved, format!("{key}: the ramp {} to {} moved from {} to {}", p.0, p.1, corner(p.2), corner(q.2)))
            };
            out.push(Change {
                owner: key.clone(),
                what: "ramp",
                how,
                was: vec![spot(&p)],
                now: vec![spot(&q)],
                arrow: Some((spot(&p).middle(), spot(&q).middle())),
                words,
            });
        }
        for q in new {
            let words = format!("{key}: a new ramp {} to {} at {}", q.0, q.1, corner(q.2));
            out.push(Change { owner: key.clone(), what: "ramp", how: How::New, was: vec![], now: vec![spot(&q)], arrow: None, words });
        }
        for p in gone {
            let words = format!("{key}: the ramp {} to {} at {} is gone", p.0, p.1, corner(p.2));
            out.push(Change { owner: key.clone(), what: "ramp", how: How::Gone, was: vec![spot(&p)], now: vec![], arrow: None, words });
        }
        // cover: pillars and crates
        let old: Vec<_> = o
            .cover
            .iter()
            .map(|c| {
                let r = f.rect((c.0, c.1, c.2, c.3));
                (r.0, r.1, r.2, r.3, c.4, c.5)
            })
            .collect();
        let (new, gone) = both_ways(&old, &it.cover);
        // (a pillar is as tall as it can reach, so one moved may be another height)
        // and paired with one of its kind within 8 cells, whatever its size)
        let near = |p: &crate::furnish::Cover, q: &crate::furnish::Cover| {
            let (a, b) = (mid((p.0, p.1, p.2, p.3)), mid((q.0, q.1, q.2, q.3)));
            (a.0 - b.0).abs() + (a.1 - b.1).abs() <= 8.0
        };
        let (moved, new, gone) = pair(gone, new, |p, q| (p.4 <= 118, p.5) == (q.4 <= 118, q.5) && near(p, q), |c| mid((c.0, c.1, c.2, c.3)));
        let size = |c: &crate::furnish::Cover| format!("{} x {}", half(c.2 - c.0), half(c.3 - c.1));
        let spot = |c: &crate::furnish::Cover| Spot { rect: (c.0, c.1, c.2, c.3), z: floor, lo: floor, hi: floor + c.4 };
        let name = |c: &crate::furnish::Cover| if c.4 <= 118 { "crate" } else { "pillar" };
        for (p, q) in moved {
            let mut words = format!("{key}: a {} moved from {} to {}", name(&q), corner((p.0, p.1, p.2, p.3)), corner((q.0, q.1, q.2, q.3)));
            if size(&p) != size(&q) {
                words += &format!(", {} now (was {})", size(&q), size(&p));
            }
            out.push(Change {
                owner: key.clone(),
                what: "cover",
                how: How::Moved,
                was: vec![spot(&p)],
                now: vec![spot(&q)],
                arrow: Some((spot(&p).middle(), spot(&q).middle())),
                words,
            });
        }
        for q in new {
            let words = format!("{key}: a {} stood at {}", name(&q), corner((q.0, q.1, q.2, q.3)));
            out.push(Change { owner: key.clone(), what: "cover", how: How::New, was: vec![], now: vec![spot(&q)], arrow: None, words });
        }
        for p in gone {
            let words = format!("{key}: the {} at {} is gone", name(&p), corner((p.0, p.1, p.2, p.3)));
            out.push(Change { owner: key.clone(), what: "cover", how: How::Gone, was: vec![spot(&p)], now: vec![], arrow: None, words });
        }
        // pits through platforms: (storey, opening)
        let pit = |p: &crate::furnish::Pit| (p.z, p.land, p.hole);
        let old: Vec<_> = o.pits.iter().map(|p| (f.z(p.z), f.z(p.land), f.rect(p.hole))).collect();
        let (new, gone) = both_ways(&old, &it.pits.iter().map(pit).collect::<Vec<_>>());
        let spot = |p: &(i32, i32, Rect)| Spot { rect: p.2, z: p.0, lo: p.1, hi: p.0 };
        for q in new {
            let words = format!("{key}: a new pit on {} at {}", q.0, corner(q.2));
            out.push(Change { owner: key.clone(), what: "pit", how: How::New, was: vec![], now: vec![spot(&q)], arrow: None, words });
        }
        for p in gone {
            let words = format!("{key}: the pit on {} at {} is gone", p.0, corner(p.2));
            out.push(Change { owner: key.clone(), what: "pit", how: How::Gone, was: vec![spot(&p)], now: vec![], arrow: None, words });
        }
    }
}

// ---------------------------------------------------------------- windows, drops, grates

/// The hall's wall a window is in, as a word.
fn wall(d: usize) -> &'static str {
    match SIDES[(d + 2) % 4] {
        'E' => "east",
        'N' => "north",
        'W' => "west",
        _ => "south",
    }
}

fn wall_spot(cells: &[Cell], z: i32, hi: i32) -> Spot {
    let (x0, y0) = cells.iter().fold((i32::MAX, i32::MAX), |(x, y), c| (x.min(c.0), y.min(c.1)));
    let (x1, y1) = cells.iter().fold((i32::MIN, i32::MIN), |(x, y), c| (x.max(c.0 + 1), y.max(c.1 + 1)));
    Spot { rect: (2 * x0, 2 * y0, 2 * x1, 2 * y1), z, lo: z, hi: z + hi }
}

fn windows(a: &Level, b: &Level, out: &mut Vec<Change>) {
    let key = |w: &crate::level::Window| (w.hall.clone(), w.z, w.wall.clone(), w.d);
    let height = |kind: &str| if kind == "rail" { 40 } else { 94 };
    for w in &b.windows {
        let spot = wall_spot(&w.wall, w.z, height(w.kind));
        match a.windows.iter().find(|o| key(o) == key(w)) {
            Some(o) if o.kind == w.kind => {}
            Some(o) => {
                let words = format!("{}'s window in its {} wall on {} is {} now (was {})", w.hall, wall(w.d), w.z, w.kind, o.kind);
                out.push(Change {
                    owner: w.hall.clone(),
                    what: "window",
                    how: How::Changed,
                    was: vec![wall_spot(&o.wall, o.z, height(o.kind))],
                    now: vec![spot],
                    arrow: None,
                    words,
                });
            }
            None => {
                let words = format!("a window in {}'s {} wall on {} ({})", w.hall, wall(w.d), w.z, w.kind);
                out.push(Change { owner: w.hall.clone(), what: "window", how: How::New, was: vec![], now: vec![spot], arrow: None, words });
            }
        }
    }
    for o in a.windows.iter().filter(|o| !b.windows.iter().any(|w| key(w) == key(o))) {
        let words = format!("{}'s window in its {} wall on {} is gone", o.hall, wall(o.d), o.z);
        out.push(Change {
            owner: o.hall.clone(),
            what: "window",
            how: How::Gone,
            was: vec![wall_spot(&o.wall, o.z, height(o.kind))],
            now: vec![],
            arrow: None,
            words,
        });
    }
}

fn drops(a: &Level, b: &Level, out: &mut Vec<Change>) {
    let floor = |lv: &Level, key: &str| lv.spaces().iter().find(|s| s.key == key).map(|s| (s.floor, s.top));
    let spot = |lv: &Level, d: &(String, String, Rect)| {
        let (lo, _) = floor(lv, &d.1).unwrap_or((0, 0));
        let (z, _) = floor(lv, &d.0).unwrap_or((0, 0));
        Spot { rect: (2 * d.2 .0, 2 * d.2 .1, 2 * d.2 .2, 2 * d.2 .3), z, lo, hi: z }
    };
    let of = |lv: &Level| lv.drops.iter().map(|(u, l, r, _)| (u.clone(), l.clone(), *r)).collect::<Vec<_>>();
    let (new, gone) = both_ways(&of(a), &of(b));
    let (moved, new, gone) = pair(gone, new, |p, q| (&p.0, &p.1) == (&q.0, &q.1), |d| mid((2 * d.2 .0, 2 * d.2 .1, 2 * d.2 .2, 2 * d.2 .3)));
    for (p, q) in moved {
        let (s0, s1) = (spot(a, &p), spot(b, &q));
        let words = format!("the drop from {} into {} moved from {}, {} to {}, {}", q.0, q.1, p.2 .0, p.2 .1, q.2 .0, q.2 .1);
        out.push(Change { owner: q.0.clone(), what: "drop", how: How::Moved, was: vec![s0], now: vec![s1], arrow: Some((s0.middle(), s1.middle())), words });
    }
    for q in new {
        let words = format!("a drop from {} into {} at {}, {}", q.0, q.1, q.2 .0, q.2 .1);
        out.push(Change { owner: q.0.clone(), what: "drop", how: How::New, was: vec![], now: vec![spot(b, &q)], arrow: None, words });
    }
    for p in gone {
        let words = format!("the drop from {} into {} is gone", p.0, p.1);
        out.push(Change { owner: p.0.clone(), what: "drop", how: How::Gone, was: vec![spot(a, &p)], now: vec![], arrow: None, words });
    }
    // grates: by the room under them, their cells and storey (a corridor's piece key is dealt
    // out again)
    let of = |lv: &Level| lv.grates.iter().map(|(_, room, r, z)| (room.clone(), *r, *z)).collect::<Vec<_>>();
    let (new, gone) = both_ways(&of(a), &of(b));
    let spot = |g: &(String, Rect, i32)| Spot { rect: (2 * g.1 .0, 2 * g.1 .1, 2 * g.1 .2, 2 * g.1 .3), z: g.2, lo: g.2 - 20, hi: g.2 };
    for q in new {
        let words = format!("a grate over {} at {}, {} on {}", q.0, q.1 .0, q.1 .1, q.2);
        out.push(Change { owner: q.0.clone(), what: "grate", how: How::New, was: vec![], now: vec![spot(&q)], arrow: None, words });
    }
    for p in gone {
        let words = format!("the grate over {} at {}, {} is gone", p.0, p.1 .0, p.1 .1);
        out.push(Change { owner: p.0.clone(), what: "grate", how: How::Gone, was: vec![spot(&p)], now: vec![], arrow: None, words });
    }
}
