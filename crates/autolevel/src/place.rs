//! Where the rooms end up before anything joins them: what is scattered for a level's options,
//! lofts brought to storeys something reaches (`settle`), and everything drawn in towards the
//! middle (`compact`). The first steps of `level.generate` in the Python.
//!
//! New in the Rust: `Placed`, the placed lots as a person changes them (a recipe's `move`,
//! `resize`, `add`, `delete` and `pin`), held to the rule the scatter itself places by.

use crate::py;
use crate::scatter::{apart, clear, hosts, pod_levels, pod_misfit, rect_gap, rect_overlap, scatter, Kind, Lot, Misfit, Rect, STOREY};
use crate::storeys::{index, snap_top, storey, CEILINGS, LEVELS};
use crate::{space, spiral};
use std::collections::BTreeSet;

/// Cells the scatter keeps between lots that share a height.
pub const GAP: i32 = 1;
/// Cells a pod must reach into its hall.
pub const POD_DEPTH: i32 = 2;
/// The smallest room, in cells each way (the scatter's tiniest).
pub const LEAST: i32 = 2;

/// Which rooms are scattered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// A colossal hall, two spires seven or eight storeys tall, lofts on the high storeys.
    Grand,
    /// The first version's rooms: one vast hall, two towers.
    Standard,
    /// Grand's rooms reaching much higher: a colossal hall ten to fifteen storeys tall, spires and
    /// lofts up its height, and pods lifted to any of a towering hall's high storeys, which are
    /// reached only from outside (`space::towers`). New in the Rust (docs/AUTOLEVEL.md, "Atriums").
    Towering,
}

impl Scale {
    /// Its name in the Python's options, and in a level's report.
    pub fn name(self) -> &'static str {
        match self {
            Scale::Grand => "grand",
            Scale::Standard => "standard",
            Scale::Towering => "towering",
        }
    }
}

/// The first version's rooms (scatter.py's KINDS).
pub const STANDARD: [Kind; 7] = [
    Kind::new("vast", 1, (17, 21), (11, 14), &[0], &[748]).host(),
    Kind::new("tower", 2, (8, 10), (7, 9), &[0, -276], &[1024]).host(),
    Kind::new("long", 1, (18, 24), (4, 5), &[0, 276], &[236, 512]),
    Kind::new("basin", 1, (10, 13), (8, 10), &[0], &[512]).host(),
    Kind::new("medium", 5, (6, 9), (5, 7), &[-276, 0, 0, 276], &[236, 512]),
    Kind::new("small", 8, (3, 5), (3, 4), &[-276, 0, 0, 276, 276], &[236]).pod(),
    Kind::new("tiny", 5, (2, 3), (2, 2), &[0, 276, 276], &[236]).pod(),
];

/// Sizes and heights spread much further (level.py's KINDS["grand"]).
pub const GRAND: [Kind; 10] = [
    Kind::new("colossal", 1, (24, 30), (16, 22), &[0], &[1063, 1338]).host(),
    Kind::new("spire", 2, (9, 12), (9, 11), &[0, 0, -276], &[1614, 1890]),
    Kind::new("vast", 1, (15, 19), (10, 13), &[0], &[787, 1063]).host(),
    Kind::new("tower", 1, (8, 10), (7, 9), &[0, -276], &[1063, 1338]).host(),
    Kind::new("long", 1, (18, 26), (4, 5), &[0, 276], &[236, 512]),
    Kind::new("basin", 1, (10, 13), (8, 10), &[0], &[512, 787]).host(),
    Kind::new("medium", 5, (6, 9), (5, 7), &[-276, 0, 0, 276], &[236, 512]),
    Kind::new("loft", 4, (5, 9), (4, 6), &[551, 827, 1102], &[236, 236, 512]),
    Kind::new("small", 8, (3, 5), (3, 4), &[-276, 0, 0, 276, 276], &[236]).pod(),
    Kind::new("tiny", 5, (2, 3), (2, 2), &[0, 276, 276], &[236]).pod(),
];

/// An atrium: a hall up to fifteen storeys tall, made by hand or by the trek planner, never
/// scattered (it is one of `Placed`'s kinds, not `kinds_and_site`'s, so every seed keeps its
/// level). Doors open on its floor and its top storey only (`Space::door_levels`), it
/// takes pods on its top storey too (`scatter::pod_levels`), and its top storey is not ramped
/// down to the rest: it is reached from outside, through other rooms (the furnisher's `bands`).
/// New in the Rust (docs/AUTOLEVEL.md, "Atriums").
pub const ATRIUM: Kind = Kind::new("atrium", 0, (8, 40), (8, 40), &[0], &[]).host();

/// Grand's kinds reaching up to fifteen storeys. Floors and ceilings by `storeys::LEVELS` and
/// `CEILINGS` (a height is floor to ceiling: 4094 from the ground is fifteen storeys).
pub const TOWERING_KINDS: [Kind; 10] = [
    Kind::new("colossal", 1, (24, 30), (16, 22), &[0], &[2716, 3267, 4094]).host(),
    Kind::new("spire", 3, (9, 12), (9, 11), &[0, 0, -276], &[1890, 2716, 3543]),
    Kind::new("vast", 1, (15, 19), (10, 13), &[0], &[1338, 2165, 2716]).host(),
    Kind::new("tower", 2, (8, 10), (7, 9), &[0, -276], &[1063, 1338, 2165]).host(),
    Kind::new("long", 1, (18, 26), (4, 5), &[0, 276, 1654], &[236, 512]),
    Kind::new("basin", 1, (10, 13), (8, 10), &[0], &[512, 787]).host(),
    Kind::new("medium", 6, (6, 9), (5, 7), &[-276, 0, 0, 276, 1102, 1929, 2756], &[236, 512]),
    Kind::new("loft", 8, (5, 9), (4, 6), &[551, 827, 1102, 1654, 2205, 2756, 3307], &[236, 236, 512]),
    Kind::new("small", 10, (3, 5), (3, 4), &[-276, 0, 0, 276, 276], &[236]).pod(),
    Kind::new("tiny", 6, (2, 3), (2, 2), &[0, 276, 276], &[236]).pod(),
];

/// The part of a level's options that decides where its rooms fall. (`level.Options` in the
/// Python; the rest of its fields arrive with the stages that read them.)
#[derive(Clone, Debug, PartialEq)]
pub struct Siting {
    pub scale: Scale,
    /// This many times the rooms of each kind, on a site of this many times the area.
    pub size: f64,
    /// The site in cells, if not the scale's own.
    pub site: Option<(i32, i32)>,
    /// How far (cells) each room may be drawn in towards the middle. 0: left where it fell.
    pub shift: i32,
}

impl Default for Siting {
    fn default() -> Self {
        Siting { scale: Scale::Grand, size: 1.0, site: None, shift: 10 }
    }
}

/// (what is scattered, the site in cells) for these options.
pub fn kinds_and_site(o: &Siting) -> (Vec<Kind>, (i32, i32)) {
    let (mut kinds, own) = match o.scale {
        Scale::Grand => (GRAND.to_vec(), (80, 60)),
        Scale::Standard => (STANDARD.to_vec(), (64, 48)),
        Scale::Towering => (TOWERING_KINDS.to_vec(), (88, 66)),
    };
    let mut site = o.site.unwrap_or(own);
    if o.size != 1.0 {
        for k in &mut kinds {
            k.count = py::round(k.count as f64 * o.size).max(1) as usize;
        }
        if o.site.is_none() {
            let grow = |v: i32| py::round(v as f64 * o.size.powf(0.5)) as i32;
            site = (grow(site.0), grow(site.1));
        }
    }
    (kinds, site)
}

/// The rooms of a level at each step before routing.
#[derive(Clone, Debug, PartialEq)]
pub struct Lots {
    pub site: (i32, i32),
    pub scattered: Vec<Lot>,
    pub settled: Vec<Lot>,
    /// What the router is given.
    pub placed: Vec<Lot>,
}

/// Scatter, settle and compact. Err if the site has no room for a lot.
pub fn lots(seed: u64, o: &Siting) -> Result<Lots, String> {
    let (kinds, site) = kinds_and_site(o);
    let scattered = scatter(seed, site, &kinds, GAP, 600, POD_DEPTH)?;
    let settled = settle(&scattered, 6);
    let placed = if o.shift != 0 { compact(&settled, 2, o.shift) } else { settled.clone() };
    Ok(Lots { site, scattered, settled, placed })
}

fn gap(a: &Lot, q: &Lot) -> i32 {
    let (dx, dy) = rect_gap(a.rect(), q.rect());
    dx + dy
}

/// Bring the rooms scattered on the high storeys to where something reaches them.
///
/// Each spire's nearest loft goes to the spire's top storey, so the climb up its walls leads
/// somewhere. Any other loft stays on its storey if a tall room (or another loft) within `reach`
/// cells has that storey too, and otherwise comes down a storey at a time until one does.
pub fn settle(lots: &[Lot], reach: i32) -> Vec<Lot> {
    let mut lots = lots.to_vec();

    fn clear(lots: &[Lot], a: usize) -> bool {
        lots.iter().enumerate().all(|(n, q)| {
            if n == a {
                return true;
            }
            let (dx, dy) = rect_gap(lots[a].rect(), q.rect());
            !(dx < 1 && dy < 1 && !apart(&lots[a], q))
        })
    }

    fn shift(lots: &mut [Lot], a: usize, f: i32) -> bool {
        let old = (lots[a].floor, lots[a].top);
        lots[a].floor = f;
        lots[a].top = f + (old.1 - old.0);
        if clear(lots, a) {
            return true;
        }
        (lots[a].floor, lots[a].top) = old;
        false
    }

    let lofts: Vec<usize> = (0..lots.len()).filter(|&n| lots[n].kind == "loft").collect();
    let spires: Vec<usize> = (0..lots.len()).filter(|&n| lots[n].kind == "spire").collect();
    let mut done: Vec<usize> = Vec::new();
    for sp in spires {
        let mut near: Vec<usize> = lofts.iter().copied().filter(|a| !done.contains(a) && gap(&lots[*a], &lots[sp]) <= reach + 2).collect();
        near.sort_by_key(|&a| gap(&lots[a], &lots[sp]));
        let ceiling = snap_top(lots[sp].top);
        let top = LEVELS.iter().copied().filter(|&z| z + STOREY <= ceiling).max().expect("a spire has a storey");
        if let Some(&a) = near.first() {
            lots[a].top = lots[a].floor + STOREY;
            if shift(&mut lots, a, top) {
                done.push(a);
            }
        }
    }
    for &a in &lofts {
        if done.contains(&a) {
            continue;
        }
        let here = lots[a].floor;
        let floors: Vec<i32> = std::iter::once(here).chain(LEVELS.iter().rev().copied().filter(|&z| 0 <= z && z < here)).collect();
        for f in floors {
            let reached = lots.iter().enumerate().any(|(n, q)| n != a && q.host.is_empty() && q.floor <= f && f + STOREY <= q.top && gap(&lots[a], q) <= reach);
            if reached && shift(&mut lots, a, f) {
                break;
            }
        }
    }
    lots
}

/// Draw scattered lots in towards the middle, each by at most `shift` cells, keeping `gap` cells
/// between rooms on the same storey. The scatter leaves rooms far apart, and every cell between
/// two of them is corridor. Sizes, storeys, pods and which room is near which stay as they fell.
pub fn compact(lots: &[Lot], gap: i32, shift: i32) -> Vec<Lot> {
    let mut lots = lots.to_vec();
    // each room that is not a pod, with its pods: they move together
    let groups: Vec<Vec<usize>> = (0..lots.len())
        .filter(|&a| lots[a].host.is_empty())
        .map(|a| std::iter::once(a).chain((0..lots.len()).filter(|&b| lots[b].host == lots[a].key)).collect())
        .collect();
    let total: i32 = lots.iter().map(|a| a.area()).sum();
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    for a in &lots {
        sum_x += a.centre().0 * a.area() as f64;
        sum_y += a.centre().1 * a.area() as f64;
    }
    let (cx, cy) = (sum_x / total as f64, sum_y / total as f64);
    let mut moved = vec![0; groups.len()];

    let ok = |lots: &[Lot], mine: &[usize], dx: i32, dy: i32| {
        mine.iter().all(|&m| {
            let r = (lots[m].x0 + dx, lots[m].y0 + dy, lots[m].x1 + dx, lots[m].y1 + dy);
            lots.iter().enumerate().all(|(b, q)| {
                if mine.contains(&b) {
                    return true;
                }
                let (gx, gy) = rect_gap(r, q.rect());
                gx >= gap || gy >= gap || apart(&lots[m], q)
            })
        })
    };

    for _ in 0..shift {
        let off = |lots: &[Lot], g: usize| {
            let (ax, ay) = lots[groups[g][0]].centre();
            (ax - cx).abs() + (ay - cy).abs()
        };
        let mut order: Vec<usize> = (0..groups.len()).collect();
        order.sort_by(|&p, &q| py::cmp_f64(off(&lots, p), off(&lots, q)));
        let mut any_move = false;
        for g in order {
            if moved[g] >= shift {
                continue;
            }
            let (ax, ay) = lots[groups[g][0]].centre();
            // the longer way to the middle first (Python: sorted(..., reverse=True) on (distance, step))
            let mut steps = [((ax - cx).abs(), (if cx > ax { 1 } else { -1 }, 0)), ((ay - cy).abs(), (0, if cy > ay { 1 } else { -1 }))];
            steps.sort_by(|p, q| py::cmp_f64(q.0, p.0).then(q.1.cmp(&p.1)));
            for (dist, (dx, dy)) in steps {
                if dist < 1.0 || !ok(&lots, &groups[g], dx, dy) {
                    continue;
                }
                for &m in &groups[g] {
                    lots[m].x0 += dx;
                    lots[m].x1 += dx;
                    lots[m].y0 += dy;
                    lots[m].y1 += dy;
                }
                moved[g] += 1;
                any_move = true;
                break;
            }
        }
        if !any_move {
            break;
        }
    }
    lots
}

/// The placed lots as a person changes them: a recipe's `move`, `resize`, `add`, `delete` and
/// `pin`, in the recipe's order. The lots stay where they are put (nothing is settled or drawn
/// together again), and `check` then holds the whole set to what the scatter guarantees.
///
/// What the plain reading of a change leaves open is settled so:
/// - a hall's pods are part of it: they move with it, change storey with it and go when it goes;
/// - whose pod a lot is follows from where the lots are, after every change: a lot of a pod kind
///   that overlaps a hall and shares a height with it is that hall's pod (so a pod moved clear
///   of its hall is a room of its own on the storey it was on, and a small room moved onto a
///   hall is its pod);
/// - no floor changes unless a `pin` asks: a lot that lands in a hall on a storey that takes no
///   pod is an error that says which storeys do. It is not lifted there, as the scatter would.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// In the order they were scattered, added ones last.
    pub lots: Vec<Lot>,
    /// The site, in cells.
    pub site: (i32, i32),
    /// What the scale scatters: the kinds a lot may be.
    kinds: Vec<Kind>,
    /// Lots taken out: (key, how: "deleted", or "deleted with its hall vast").
    pub gone: Vec<(String, String)>,
    /// Lots a change named.
    pub touched: BTreeSet<String>,
    /// An empty start (`Placed::empty`): the last lot may be deleted too.
    pub blank: bool,
}

/// "at 3, 4 to 9, 8, from 0 to 236": where a lot is, for a message.
fn whereabouts(a: &Lot) -> String {
    format!("at {}, {} to {}, {}, from {} to {}", a.x0, a.y0, a.x1, a.y1, a.floor, snap_top(a.top))
}

fn list(zs: &[i32]) -> String {
    zs.iter().map(|z| z.to_string()).collect::<Vec<_>>().join(", ")
}

/// Err unless a room can stand from `floor` to `top`: its floor on a storey, a storey or more
/// tall, and under the highest ceiling.
fn stands(key: &str, floor: i32, top: i32) -> Result<(), String> {
    let highest = CEILINGS[CEILINGS.len() - 1];
    if !LEVELS.contains(&floor) {
        return Err(format!("{key}'s floor would be at {floor}: floors are at {}", list(&LEVELS)));
    }
    if top > highest {
        return Err(format!("{key} would stand from {floor} to {top}, above the highest ceiling ({highest})"));
    }
    if snap_top(top) < storey(floor).1 {
        return Err(format!("{key} would be under a storey tall ({floor} to {top})"));
    }
    Ok(())
}

impl Placed {
    /// The lots as they were placed for these options, nothing changed yet.
    pub fn new(lots: Vec<Lot>, o: &Siting) -> Placed {
        let (mut kinds, site) = kinds_and_site(o);
        kinds.push(ATRIUM); // never scattered: only an edit adds one
        Placed { lots, site, kinds, gone: Vec::new(), touched: BTreeSet::new(), blank: false }
    }

    /// No lots at all, on the site the siting gives: an empty start (`level::Start::Empty`).
    pub fn empty(o: &Siting) -> Placed {
        Placed { blank: true, ..Placed::new(Vec::new(), o) }
    }

    /// Which lot has this key. Err says if an earlier change took it out.
    fn at(&self, what: &str, room: &str) -> Result<usize, String> {
        if let Some(n) = self.lots.iter().position(|a| a.key == room) {
            return Ok(n);
        }
        Err(match self.gone.iter().find(|(k, _)| k == room) {
            Some((_, how)) => format!("{what}: {room} was {how} by an earlier edit"),
            None => format!("{what}: no room {room}"),
        })
    }

    /// The lot and its pods, the lot first.
    fn group(&self, n: usize) -> Vec<usize> {
        let key = &self.lots[n].key;
        std::iter::once(n).chain((0..self.lots.len()).filter(|&m| self.lots[m].host == *key)).collect()
    }

    /// Whose pod each lot is, from where the lots are: a lot of a pod kind is the pod of the
    /// first hall it overlaps and shares a height with. (Lots as the scatter leaves them are
    /// as they were.)
    fn rehost(&mut self) {
        for n in 0..self.lots.len() {
            if !self.kinds.iter().any(|k| k.pod && k.name == self.lots[n].kind) {
                continue;
            }
            let a = &self.lots[n];
            let hall = self
                .lots
                .iter()
                .find(|o| o.key != a.key && hosts(&self.kinds, o.kind) && o.host.is_empty() && rect_overlap(a.rect(), o.rect()).is_some() && !apart(a, o));
            self.lots[n].host = hall.map_or_else(String::new, |o| o.key.clone());
        }
    }

    fn touch(&mut self, room: &str) {
        self.touched.insert(room.to_string());
        self.rehost();
    }

    /// What the scale scatters: the kinds a lot may be.
    pub fn kinds(&self) -> &[Kind] {
        &self.kinds
    }

    /// Has anything been changed?
    pub fn changed(&self) -> bool {
        !self.touched.is_empty() || !self.gone.is_empty()
    }

    /// `move`: the lot's low corner goes to the cell `to`, its size kept, its pods with it.
    pub fn move_to(&mut self, room: &str, to: (i32, i32)) -> Result<(), String> {
        let n = self.at("move", room)?;
        let (dx, dy) = (to.0 - self.lots[n].x0, to.1 - self.lots[n].y0);
        for m in self.group(n) {
            let a = &mut self.lots[m];
            (a.x0, a.x1, a.y0, a.y1) = (a.x0 + dx, a.x1 + dx, a.y0 + dy, a.y1 + dy);
        }
        self.touch(room);
        Ok(())
    }

    /// `resize`: the lot is `size` cells wide and deep from its low corner. Err under 2 x 2.
    pub fn resize(&mut self, room: &str, size: (i32, i32)) -> Result<(), String> {
        let n = self.at("resize", room)?;
        if size.0 < LEAST || size.1 < LEAST {
            return Err(format!("resize: a room is at least {LEAST} x {LEAST} cells, not {} x {}", size.0, size.1));
        }
        let a = &mut self.lots[n];
        (a.x1, a.y1) = (a.x0 + size.0, a.y0 + size.1);
        self.touch(room);
        Ok(())
    }

    /// `add`: a lot the scatter did not make, of one of the scale's kinds, after the others.
    /// `top` is the ceiling of its top storey (a ceiling line; -40 will do for the low road's).
    /// Err for a key that is taken or could be mistaken for a corridor's, a kind the scale
    /// does not have, a rectangle under 2 x 2, or heights no room stands at.
    pub fn add(&mut self, key: &str, kind: &str, rect: Rect, floor: i32, top: i32) -> Result<(), String> {
        // (keys go into the names of random streams, walks and the dead-end finder's places)
        let plain = key.len() <= 24
            && key.starts_with(|c: char| c.is_ascii_lowercase())
            && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !plain {
            return Err(format!("add: a room's key is a lower-case letter, then letters, digits or _ (24 at most), not {key:?}"));
        }
        let number = key[1..].trim_end_matches(|c: char| ('a'..='h').contains(&c));
        if key.starts_with(['c', 'v']) && key.len() - 1 - number.len() <= 1 && !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("add: {key} could be a corridor's or a vent's key: give the room another"));
        }
        if self.lots.iter().any(|a| a.key == key) {
            return Err(format!("add: there is a room {key} already"));
        }
        let Some(k) = self.kinds.iter().find(|k| k.name == kind) else {
            let names: Vec<&str> = self.kinds.iter().map(|k| k.name).collect();
            return Err(format!("add: no kind {kind} at this scale: one of {}", names.join(", ")));
        };
        if rect.2 - rect.0 < LEAST || rect.3 - rect.1 < LEAST {
            return Err(format!("add: a room is at least {LEAST} x {LEAST} cells, not {} x {}", rect.2 - rect.0, rect.3 - rect.1));
        }
        // the top as the scatter spells it: a storey above the top storey's floor (the low
        // road's ceiling line is a unit above that)
        let up = CEILINGS.iter().position(|&c| c == top || (c, top) == (CEILINGS[0], LEVELS[0] + STOREY));
        let Some(up) = up else {
            return Err(format!("add: {key}'s top would be at {top}: ceilings are at {}", list(&CEILINGS)));
        };
        let top = LEVELS[up] + STOREY;
        stands(key, floor, top).map_err(|why| format!("add: {why}"))?;
        self.lots.push(Lot { key: key.to_string(), kind: k.name, x0: rect.0, y0: rect.1, x1: rect.2, y1: rect.3, floor, top, host: String::new() });
        self.gone.retain(|(k, _)| k != key); // a room again
        self.touch(key);
        Ok(())
    }

    /// `delete`: the lot is left out, and its pods with it. Err if nothing would be left.
    pub fn delete(&mut self, room: &str) -> Result<(), String> {
        let n = self.at("delete", room)?;
        let keys: Vec<String> = self.group(n).into_iter().map(|m| self.lots[m].key.clone()).collect();
        if keys.len() == self.lots.len() && !self.blank {
            return Err(format!("delete: without {room} the level has no room at all"));
        }
        for pod in &keys[1..] {
            self.gone.push((pod.clone(), format!("deleted with its hall {room}")));
        }
        self.gone.push((room.to_string(), "deleted".to_string()));
        self.lots.retain(|a| !keys.contains(&a.key));
        self.rehost();
        Ok(())
    }

    /// `pin`: the lot's floor is on the storey `floor`, its height kept, its pods as many
    /// storeys up or down with it. Err for a height that is no storey's floor, or if it (or a
    /// pod of it) would stand above the highest ceiling or below the lowest floor.
    pub fn pin(&mut self, room: &str, floor: i32) -> Result<(), String> {
        let n = self.at("pin", room)?;
        let was = self.lots[n].floor;
        stands(room, floor, self.lots[n].top + floor - was).map_err(|why| format!("pin: {why}"))?;
        // (storeys are not all as far apart, so pods go by storeys, not by units)
        let up = index(floor) as i32 - index(was) as i32;
        let group = self.group(n);
        let mut floors = vec![floor];
        for &m in &group[1..] {
            let pod = &self.lots[m];
            let to = usize::try_from(index(pod.floor) as i32 + up).ok().and_then(|i| LEVELS.get(i));
            let to = to.ok_or_else(|| format!("pin: with {room} on {floor}, its pod {} would have no storey to be on", pod.key))?;
            floors.push(*to);
        }
        for (m, floor) in group.into_iter().zip(floors) {
            let a = &mut self.lots[m];
            (a.floor, a.top) = (floor, floor + a.top - a.floor);
        }
        self.touch(room);
        Ok(())
    }

    /// Err, naming the lots and the rule, unless the lots are as the scatter could have left
    /// them: each on the site, at least 2 x 2 cells, on a storey and a storey or more tall; a
    /// spire with walls long enough and storeys enough for its ramps (`band`: a catwalk's depth
    /// in half cells); each pod lawfully in its hall (one storey tall, on one of the hall's
    /// first two upper storeys, at least two cells in, a fifth of its floor at most); and every
    /// other pair clear of each other (`scatter::clear`).
    pub fn check(&self, band: i32) -> Result<(), String> {
        for a in &self.lots {
            let (w, d) = (a.x1 - a.x0, a.y1 - a.y0);
            if w < LEAST || d < LEAST {
                return Err(format!("{} is {w} x {d} cells: a room is at least {LEAST} x {LEAST}", a.key));
            }
            if a.x0 < 0 || a.y0 < 0 || a.x1 > self.site.0 || a.y1 > self.site.1 {
                return Err(format!("{} is off the site (it is {}; the site is {} x {} cells)", a.key, whereabouts(a), self.site.0, self.site.1));
            }
            stands(&a.key, a.floor, a.top)?;
            if a.kind == "spire" {
                let s = &space::spaces(std::slice::from_ref(a))[0];
                if spiral::layout(s.rect_h(), &s.storeys(), 0, 1, band).is_none() {
                    let need = (spiral::RUN + 2 * spiral::LAND.max(band)) / 2;
                    return Err(format!(
                        "{} is a spire: its ramps climb walls at least {need} cells long, through two storeys or more (it is {w} x {d}, from {} to {})",
                        a.key,
                        a.floor,
                        snap_top(a.top)
                    ));
                }
            }
        }
        for (n, a) in self.lots.iter().enumerate() {
            for b in &self.lots[n + 1..] {
                let (pod, hall) = if a.host == b.key {
                    (a, b)
                } else if b.host == a.key {
                    (b, a)
                } else {
                    if !clear(a, b, GAP) {
                        let how = if rect_overlap(a.rect(), b.rect()).is_some() { "overlap" } else { "touch" };
                        return Err(format!(
                            "{} and {} {how}: rooms keep a cell between them, unless one is over the other with a 40-unit slab between ({} is {}; {} is {})",
                            a.key,
                            b.key,
                            a.key,
                            whereabouts(a),
                            b.key,
                            whereabouts(b)
                        ));
                    }
                    continue;
                };
                let (p, h) = (&pod.key, &hall.key);
                let levels = pod_levels(hall);
                if !levels.contains(&pod.floor) {
                    let which = if levels.is_empty() { format!("{h} has none a pod fits") } else { format!("{}, not {}", list(&levels), pod.floor) };
                    return Err(format!("{p} lands in {h}, so it is its pod, and a pod's floor is one of its hall's first two upper storeys: {which}"));
                }
                if pod.top != pod.floor + STOREY {
                    return Err(format!("{p} lands in {h}, so it is its pod, and a pod is one storey tall, not from {} to {}", pod.floor, pod.top));
                }
                let ov = rect_overlap(pod.rect(), hall.rect()).expect("a pod overlaps its hall");
                match pod_misfit(ov, hall, POD_DEPTH) {
                    Some(Misfit::Shallow(deep)) => {
                        return Err(format!(
                            "{p} reaches {deep} cell into {h}: a pod reaches at least {POD_DEPTH} cells in (a room of its own stands a cell clear)"
                        ));
                    }
                    Some(Misfit::Big(cells)) => {
                        return Err(format!("{p} takes {cells} of {h}'s {} cells of floor: a pod takes a fifth at most", hall.area()));
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }
}
