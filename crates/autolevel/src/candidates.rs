//! What a person could do next to a recipe's corridors: the joins that could be made from a room
//! on a storey, each with the route it would be, and what taking a route out comes to. New here
//! (the Python has none): the editor's milestone 2, headless so the command line lists the same.
//!
//! **What is previewed is what is made.** A candidate is found by applying the recipe's own
//! `join` edit (`recipe::join`: a corridor on the dead-end pass's terms, a vent if none fits) to
//! a copy of the network as it stands after routing and the recipe's edits (`Recipe::routed`),
//! with the other room's door held to one storey. That is the call the recipe makes when the
//! candidate's edit (`Candidate::edit`: `join` with `"on": [storey]`) is added to the end of its
//! list, on the same network, so the route is the same: its key, its cells, its doors. Each one
//! found is then tried in a whole level (`Recipe::generate` with the edit added), and only those
//! that come out the other end with that route in place are offered; the rest are listed with
//! the reason (a room that can't be furnished round the new door, say).
//!
//! `removal` says what taking a route out is: the recipe's `remove` edit for a corridor of the
//! level as routed (refused, with the rooms that would be cut off, if it is the only way
//! somewhere), or, for a corridor the dead-end pass added, keeping the dead end it joined up.

use crate::build::{Fix, FAR};
use crate::cells::Cell;
use crate::deadends;
use crate::deadlist::fix_text;
use crate::level::{Level, Options};
use crate::network::Network;
use crate::py;
use crate::recipe::{self, Edit, Recipe};
use crate::route::Route;
use crate::scatter::rect_gap;
use crate::storeys::index;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Cells: how far off a room looks for something to join by hand (as far as the dead-end pass
/// ever looks).
pub const REACH: i32 = FAR;
/// The most storeys between the picked storey and the door at the other end (the pass's own).
pub const STOREYS: i32 = deadends::STOREYS;

/// A cell on a storey: (the storey's floor, x, y).
pub type Spot = (i32, i32, i32);

/// Every cell a route takes on the grid: its flat cells, and its ramps' and sloped ducts' on
/// both the storeys they join (what `Grid::commit` marks).
pub fn footprint(r: &Route) -> BTreeSet<Spot> {
    let mut out = BTreeSet::new();
    let mut rect = |(x0, y0, x1, y1): (i32, i32, i32, i32), zs: [i32; 2]| {
        for z in zs {
            out.extend((x0..x1).flat_map(|x| (y0..y1).map(move |y| (z, x, y))));
        }
    };
    for pc in &r.pieces {
        if let (Some((foot, top)), Some(cells)) = (pc.ramp, pc.ramp_cells) {
            rect(cells, [foot.2, top.2]);
        }
        for &((lo, hi), cells) in &pc.ducts {
            rect(cells, [lo.2, hi.2]);
        }
    }
    for pc in &r.pieces {
        for (z, cells) in &pc.cells {
            out.extend(cells.iter().map(|&(x, y): &Cell| (*z, x, y)));
        }
    }
    out
}

/// A join that could be made.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// The room picked, and the storey its door would be on.
    pub room: String,
    pub storey: i32,
    /// The room it would reach, and the storey of the door there.
    pub to: String,
    pub on: i32,
    /// The route as it would be made: its key, its pieces and their cells, its two doors, its
    /// centre line.
    pub route: Route,
}

impl Candidate {
    /// The edit that makes it: added to the end of the recipe it was found for, it gives `route`.
    pub fn edit(&self) -> Edit {
        Edit::Join { room: self.room.clone(), storey: self.storey, to: self.to.clone(), on: vec![self.on] }
    }

    /// corridor (with or without ramps), stub (a door through the wall) or vent.
    pub fn kind(&self) -> &'static str {
        match self.route.pieces[0].kind {
            "ramp" => "corridor",
            kind => kind,
        }
    }

    /// The cells it would take (`footprint`).
    pub fn cells(&self) -> BTreeSet<Spot> {
        footprint(&self.route)
    }

    /// One line for a list: "to colossal on 551 by a corridor, 9 cells (c45)".
    pub fn text(&self) -> String {
        format!("to {} on {} by {} ({})", self.to, self.on, how(&self.route), self.route.key())
    }
}

/// What a route is, in the dead-end pass's words for a join: "a corridor, 14 cells with 2
/// ramps", "a door through the wall", "a vent, 6 cells".
pub fn how(r: &Route) -> String {
    let by = &r.pieces[0];
    let fix =
        Fix::Joined { room: String::new(), storey: 0, to: String::new(), by: String::new(), kind: by.kind, length: py::round1(r.length()), ramps: r.ramps() };
    // ("joined  on 0 to  by a corridor, 9 cells ()": what lies between "by" and the key)
    let said = fix_text(&fix);
    let how = said.split_once(" by ").map_or(said.as_str(), |(_, how)| how);
    how.strip_suffix(" ()").unwrap_or(how).to_string()
}

/// The joins that could be made from a room on a storey.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidates {
    pub room: String,
    pub storey: i32,
    /// Those that can be made. Lists and the plan number them from 1: first each room's
    /// cheapest, the cheapest of those first (`rooms` of them), then the other storeys those
    /// rooms could take the door on, again the cheapest first.
    pub found: Vec<Candidate>,
    /// How many of `found` are a room's cheapest: what a plan shows unless asked for them all.
    pub rooms: usize,
    /// Corridors that fit, with which no level is made, and why.
    pub refused: Vec<(Candidate, String)>,
    /// (room, storey) within reach that no corridor fits to.
    pub none: Vec<(String, i32)>,
}

impl Candidates {
    /// The `n`th that can be made, counting from 1.
    pub fn get(&self, n: usize) -> Option<&Candidate> {
        n.checked_sub(1).and_then(|k| self.found.get(k))
    }

    /// The list in words, numbered as the plan numbers it.
    pub fn text(&self) -> String {
        self.text_with(|_| None)
    }

    /// `text`, with what `note` has to say of each join in brackets after it.
    pub fn text_with(&self, note: impl Fn(&Candidate) -> Option<String>) -> String {
        let (room, storey) = (&self.room, self.storey);
        let s = |n: usize| if n == 1 { "" } else { "s" };
        let mut out = match (self.rooms, self.found.len()) {
            (0, _) => format!("{room} on {storey}: nothing within reach can be joined to it\n"),
            (rooms, ways) => format!("{room} on {storey} could be joined to {rooms} room{} ({ways} way{}):\n", s(rooms), s(ways)),
        };
        for (n, c) in self.found.iter().enumerate() {
            if n == self.rooms {
                out.push_str("  the same rooms, by a door on another storey:\n");
            }
            let note = note(c).map_or(String::new(), |note| format!("  [{note}]"));
            out.push_str(&format!("  {:>2}  {}{note}\n", n + 1, c.text()));
        }
        for (c, why) in &self.refused {
            out.push_str(&format!("   -  {}: no level with it ({why})\n", c.text()));
        }
        // (a room's storeys together: "vast on 0, 276")
        let mut by_room: Vec<(&str, Vec<String>)> = Vec::new();
        for (to, z) in &self.none {
            match by_room.iter_mut().find(|(k, _)| k == to) {
                Some((_, zs)) => zs.push(z.to_string()),
                None => by_room.push((to, vec![z.to_string()])),
            }
        }
        if !by_room.is_empty() {
            let rooms: Vec<String> = by_room.iter().map(|(to, zs)| format!("{to} on {}", zs.join(", "))).collect();
            out.push_str(&format!("   -  no corridor fits to: {}\n", rooms.join("; ")));
        }
        out
    }
}

/// The joins that could be made from `room` on `storey`, to every room within `REACH` cells on
/// every storey it takes doors on within `STOREYS` of that one.
pub fn candidates(recipe: &Recipe, room: &str, storey: i32) -> Result<Candidates, String> {
    candidates_within(recipe, room, storey, REACH, STOREYS)
}

/// `candidates`, looking `reach` cells off and `storeys` storeys up and down.
///
/// Err if the recipe makes no network, `room` is not one of its rooms or is a pod, or it takes
/// no door on `storey`.
pub fn candidates_within(recipe: &Recipe, room: &str, storey: i32, reach: i32, storeys: i32) -> Result<Candidates, String> {
    let (_, net) = recipe.routed()?;
    let (fits, none) = routes(&net, &recipe.options, room, storey, reach, storeys)?;
    let checked = verdicts(recipe, &fits);
    let (mut found, mut refused) = (Vec::new(), Vec::new());
    for (c, verdict) in fits.into_iter().zip(checked) {
        match verdict {
            Ok(()) => found.push(c),
            Err(why) => refused.push((c, why)),
        }
    }
    // each room's cheapest first (the list is by cost already, so the first met is the cheapest)
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let (best, rest): (Vec<Candidate>, Vec<Candidate>) = found.into_iter().partition(|c| seen.insert(c.to.clone()));
    let rooms = best.len();
    Ok(Candidates { room: room.to_string(), storey, found: best.into_iter().chain(rest).collect(), rooms, refused, none })
}

/// What `routes` finds: the corridors that fit, and the (room, storey) none fits to.
pub type Fits = (Vec<Candidate>, Vec<(String, i32)>);

/// The routes a join from `room` on `storey` would add to this network: to each room within
/// `reach` cells, on each storey it takes doors on within `storeys` of that one, the cheapest
/// first; and the (room, storey) no corridor fits to. Nothing is made: `net` is as it was.
pub fn routes(net: &Network, o: &Options, room: &str, storey: i32, reach: i32, storeys: i32) -> Result<Fits, String> {
    let sp = net.spaces();
    let a = sp.iter().find(|s| s.key == room).ok_or_else(|| format!("no room {room}"))?;
    if !a.host.is_empty() {
        return Err(format!("{room} is a pod: it is joined inside {}", a.host));
    }
    if !a.door_levels().contains(&storey) {
        let on: Vec<String> = a.door_levels().iter().map(|z| z.to_string()).collect();
        return Err(format!("{room} takes doors on {}, not on {storey}", on.join(", ")));
    }
    let apart = |z: i32| (index(z) as i32 - index(storey) as i32).abs();
    let (mut fits, mut none) = (Vec::new(), Vec::new());
    for t in sp.iter().filter(|t| t.key != room && t.host.is_empty()) {
        let (dx, dy) = rect_gap(a.rect(), t.rect());
        if dx + dy > reach {
            continue;
        }
        for z in t.door_levels().into_iter().filter(|z| apart(*z) <= storeys) {
            // the recipe's own join, on a copy: what it adds is what the edit will add
            let mut tried = net.clone();
            match recipe::join(&mut tried, o, room, storey, &t.key, &[z]) {
                Ok(()) => {
                    let route = tried.routes().last().expect("a join adds a route").clone();
                    fits.push(Candidate { room: room.to_string(), storey, to: t.key.clone(), on: z, route });
                }
                Err(_) => none.push((t.key.clone(), z)),
            }
        }
    }
    fits.sort_by(|p, q| py::cmp_f64(p.route.cost, q.route.cost).then_with(|| (&p.to, p.on).cmp(&(&q.to, q.on))));
    Ok((fits, none))
}

/// Is a level made with this join, its route in place as found? Err says why not.
pub fn fits(recipe: &Recipe, c: &Candidate) -> Result<(), String> {
    let mut with = recipe.clone();
    with.edits.push(c.edit());
    // (the generator has places where it stops dead, as the Python does: no level, then)
    let lv = catch_unwind(AssertUnwindSafe(|| with.generate())).unwrap_or_else(|_| Err("the generator stopped on it".to_string()))?;
    match lv.net.routes().iter().find(|r| r.key() == c.route.key()) {
        Some(r) if *r == c.route => Ok(()),
        Some(_) => Err(format!("{} came out another way", c.route.key())),
        None => match [&c.room, &c.to].into_iter().find(|k| lv.spaces().iter().all(|s| s.key != **k)) {
            Some(gone) => Err(format!("the dead-end pass then leaves {gone} out")),
            None => Err(format!("{} is not in the level", c.route.key())),
        },
    }
}

/// `fits` for each, in order. A level apiece, so they are made side by side on threads.
fn verdicts(recipe: &Recipe, found: &[Candidate]) -> Vec<Result<(), String>> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(found.len());
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Result<(), String>>>> = Mutex::new(vec![None; found.len()]);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let n = next.fetch_add(1, Ordering::Relaxed);
                let Some(c) = found.get(n) else { break };
                let verdict = fits(recipe, c);
                out.lock().expect("no worker stops with the list held")[n] = Some(verdict);
            });
        }
    });
    out.into_inner().expect("the workers are done").into_iter().map(|v| v.expect("every one was tried")).collect()
}

// ------------------------------------------------------------ taking a route out

/// What taking a route out of a recipe's level is.
#[derive(Clone, Debug, PartialEq)]
pub enum Removal {
    /// A corridor of the level as routed (one a `join` edit made, too): the `remove` edit.
    Remove { route: String },
    /// A corridor the dead-end pass added, joining up the dead end `room` is in: that dead end
    /// is kept as it is, so the pass adds nothing.
    Keep { route: String, room: String },
}

impl Removal {
    /// The edit that does it, to add to the end of the recipe's list.
    pub fn edit(&self) -> Edit {
        match self {
            Removal::Remove { route } => Edit::Remove { route: route.clone() },
            Removal::Keep { room, .. } => Edit::Keep { room: room.clone() },
        }
    }

    /// What was done, in words.
    pub fn text(&self) -> String {
        match self {
            Removal::Remove { route } => format!("{route} is taken out"),
            Removal::Keep { route, room } => {
                format!("{route} was the dead-end pass's: {room}'s dead end is now kept as it is, so the pass adds no corridor to it")
            }
        }
    }
}

/// The rooms left on the smaller side when `r` is taken out of `net` (none: the level holds
/// together without it). `net` is the network with the route still in it.
pub fn cut_off(net: &Network, r: &Route) -> Vec<String> {
    let mut without = net.clone();
    without.grid.remove(r);
    let mut nb: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let edges = without.edges();
    for (a, b) in &edges {
        nb.entry(a.as_str()).or_default().push(b);
        nb.entry(b.as_str()).or_default().push(a);
    }
    fn side<'a>(nb: &BTreeMap<&'a str, Vec<&'a str>>, from: &'a str) -> BTreeSet<&'a str> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![from];
        while let Some(q) = todo.pop() {
            if seen.insert(q) {
                todo.extend(nb.get(q).into_iter().flatten().copied());
            }
        }
        seen
    }
    let (a, b) = (side(&nb, r.a.room.as_str()), side(&nb, r.b.room.as_str()));
    if a.contains(r.b.room.as_str()) {
        return Vec::new();
    }
    // (in the order the rooms were scattered, the far end of the corridor first)
    let (small, end) = if b.len() <= a.len() { (b, &r.b.room) } else { (a, &r.a.room) };
    let mut out = vec![end.clone()];
    out.extend(without.spaces().iter().map(|s| &s.key).filter(|k| small.contains(k.as_str()) && *k != end).cloned());
    out
}

/// What taking out the route with a piece of this key is, for a recipe and the level it makes.
/// Err, saying why, if it is the only way to a room, or there is no such route.
pub fn removal(recipe: &Recipe, lv: &Level, key: &str) -> Result<Removal, String> {
    let (_, net) = recipe.routed()?;
    let has = |r: &&Route| r.pieces.iter().any(|pc| pc.key == key);
    if let Some(r) = net.routes().iter().find(has) {
        let cut = cut_off(&net, r);
        return match cut.as_slice() {
            [] => Ok(Removal::Remove { route: r.key().to_string() }),
            [room] => Err(format!("{} is the only way to {room}: without it the level is in two", r.key())),
            [room, more @ ..] => {
                let s = if more.len() == 1 { "" } else { "s" };
                Err(format!("{} is the only way to {room} and {} room{s} past it: without it the level is in two", r.key(), more.len()))
            }
        };
    }
    let r = lv.net.routes().iter().find(has).ok_or_else(|| format!("no route {key} in this level"))?;
    let joined = lv.fixes.iter().find_map(|f| match f {
        Fix::Joined { room, by, .. } if by == r.key() => Some(room.clone()),
        _ => None,
    });
    match joined {
        Some(room) => Ok(Removal::Keep { route: r.key().to_string(), room }),
        None => Err(format!("{} is not a route of the recipe's, nor one the dead-end pass added", r.key())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storeys::LEVELS;

    /// Every cell of the grid that one of a route's pieces owns.
    fn owned(net: &Network, r: &Route) -> BTreeSet<Spot> {
        let grid = &net.grid;
        let ids: Vec<i32> = r.pieces.iter().filter_map(|pc| grid.id_of(&pc.key)).collect();
        let cells = LEVELS.into_iter().flat_map(|z| (grid.ox..grid.ox + grid.w).flat_map(move |x| (grid.oy..grid.oy + grid.h).map(move |y| (z, x, y))));
        cells.filter(|&(z, x, y)| ids.contains(&grid.owner_at(x, y, z))).collect()
    }

    /// The level a recipe makes with a candidate chosen, through the recipe's file.
    fn chosen(recipe: &Recipe, c: &Candidate) -> (Recipe, Level) {
        let mut with = recipe.clone();
        with.edits.push(c.edit());
        let with = recipe::load(&recipe::save(&with)).unwrap();
        let lv = with.generate().unwrap_or_else(|why| panic!("{}: {why}", c.text()));
        (with, lv)
    }

    /// The level has the candidate's route exactly as it was previewed: the same key, pieces,
    /// doors and centre line, and on the grid the same cells and no others.
    fn assert_made(lv: &Level, c: &Candidate) {
        let r = lv.net.routes().iter().find(|r| r.key() == c.route.key()).unwrap_or_else(|| panic!("no {} in the level", c.route.key()));
        assert_eq!(*r, c.route, "{}", c.text());
        assert_eq!(owned(&lv.net, r), c.cells(), "{}", c.text());
        assert!(!c.cells().is_empty());
        for d in [&r.a, &r.b] {
            assert!(lv.net.doors(&d.room).contains(d), "{}: no door in {}", c.text(), d.room);
        }
        let ends = [(r.a.room.as_str(), r.a.level), (r.b.room.as_str(), r.b.level)];
        assert!(ends.contains(&(c.room.as_str(), c.storey)) && ends.contains(&(c.to.as_str(), c.on)), "{}: {ends:?}", c.text());
    }

    #[test]
    fn a_routes_footprint_is_what_the_grid_marks_for_it() {
        for seed in [40, 13] {
            let (_, net) = Recipe::before_bases(seed).routed().unwrap();
            assert!(net.routes().iter().any(|r| r.ramps() > 0) && net.routes().iter().any(|r| r.pieces[0].kind == "vent"));
            for r in net.routes() {
                assert_eq!(footprint(r), owned(&net, r), "seed {seed}: {}", r.key());
            }
        }
    }

    #[test]
    fn candidates_are_found_for_a_known_seed() {
        let recipe = Recipe::before_bases(40);
        let found = candidates(&recipe, "tower", 551).unwrap();
        assert_eq!((found.room.as_str(), found.storey), ("tower", 551));
        // the tower's neighbours first, each by its cheapest corridor, then their other storeys
        let to: Vec<(&str, i32)> = found.found.iter().map(|c| (c.to.as_str(), c.on)).collect();
        assert_eq!(&to[..3], [("loft3", 551), ("vast", 551), ("colossal", 551)]);
        assert_eq!((found.rooms, found.found.len()), (9, 21), "{}", found.text());
        let rooms: BTreeSet<&str> = to[..found.rooms].iter().map(|(room, _)| *room).collect();
        assert_eq!(rooms.len(), found.rooms);
        assert!(to[found.rooms..].iter().all(|(room, _)| rooms.contains(room)));
        for part in [&found.found[..found.rooms], &found.found[found.rooms..]] {
            assert!(part.windows(2).all(|w| w[0].route.cost <= w[1].route.cost));
        }
        // every one starts at the tower on 551, would be the next route made, and is in reach
        let (_, net) = recipe.routed().unwrap();
        let next = format!("c{:02}", net.grid.made + 1);
        for c in &found.found {
            let ends = [(c.route.a.room.as_str(), c.route.a.level), (c.route.b.room.as_str(), c.route.b.level)];
            assert!(ends.contains(&("tower", 551)) && ends.contains(&(c.to.as_str(), c.on)), "{}", c.text());
            assert!(c.route.key() == next || c.route.key() == format!("{next}a"), "{}", c.text());
            assert!(c.route.cost <= crate::build::JOIN_COST);
            assert_eq!(c.edit(), Edit::Join { room: "tower".into(), storey: 551, to: c.to.clone(), on: vec![c.on] });
        }
        assert_eq!((found.found[0].kind(), found.found[0].text().as_str()), ("stub", "to loft3 on 551 by a door through the wall (c39)"));
        assert!(found.found.iter().any(|c| c.kind() == "corridor" && c.route.ramps() == 0) && found.found.iter().any(|c| c.route.ramps() >= 2));
        // what no corridor fits to is said, a room's storeys together
        assert_eq!(found.none, [("long".to_string(), 0), ("tiny3".to_string(), 276)]);
        let text = found.text();
        assert!(text.starts_with("tower on 551 could be joined to 9 rooms (21 ways):\n   1  to loft3 on 551 by a door through the wall (c39)\n"), "{text}");
        assert!(
            text.contains("\n  the same rooms, by a door on another storey:\n  10  to ")
                && text.ends_with("   -  no corridor fits to: long on 0; tiny3 on 276\n")
        );
        assert_eq!((found.get(1), found.get(0), found.get(22)), (Some(&found.found[0]), None, None));
        // looking less far finds fewer
        let near = candidates_within(&recipe, "tower", 551, 2, 0).unwrap();
        assert_eq!(near.found.iter().map(|c| (c.to.as_str(), c.on)).collect::<Vec<_>>(), [("loft3", 551), ("vast", 551)]);
        // what can't be looked from says why
        let err = |room: &str, storey: i32| candidates(&recipe, room, storey).unwrap_err();
        assert_eq!(err("attic", 0), "no room attic");
        assert_eq!(err("small8", 276), "small8 is a pod: it is joined inside vast");
        assert_eq!(err("tower", 1654), "tower takes doors on 0, 276, 551, not on 1654");
        let bad = Recipe { options: Options { walkway: 3, ..Options::before_bases() }, ..recipe };
        assert_eq!(candidates(&bad, "tower", 551).unwrap_err(), "walkway: 1 or 2 cells");
    }

    #[test]
    fn what_is_previewed_is_what_is_made() {
        // a door through the wall, flat corridors, ramps and switchbacks; from a floor, a deck and a basement
        for (seed, room, storey) in [(40, "tower", 551), (40, "medium4", -276), (7, "tiny3", 276), (23, "spire1", 1102)] {
            let recipe = Recipe::before_bases(seed);
            let found = candidates(&recipe, room, storey).unwrap();
            assert!(found.found.len() >= 6, "seed {seed}: {}", found.text());
            let plain = recipe.generate().unwrap();
            // the first, the last of the rooms' cheapest, the one with most ramps, and the dearest
            let most = (0..found.found.len()).max_by_key(|&n| found.found[n].route.ramps()).unwrap();
            for n in BTreeSet::from([0, found.rooms - 1, most, found.found.len() - 1]) {
                let c = &found.found[n];
                let (with, lv) = chosen(&recipe, c);
                assert_made(&lv, c);
                assert_eq!(with.edits.len(), 1);
                // it is one route more than the recipe made after routing, and the same again
                assert!(plain.net.routes().iter().all(|r| r.key() != c.route.key() || lv.fixes != plain.fixes));
                assert_eq!(format!("{:?}", with.generate().unwrap().net.routes()), format!("{:?}", lv.net.routes()));
            }
        }
    }

    #[test]
    fn a_vent_is_previewed_where_no_corridor_fits() {
        // seed 13: nothing but a crawl duct fits from medium4's basement to tiny2 on the ground
        let recipe = Recipe::before_bases(13);
        let found = candidates(&recipe, "medium4", -276).unwrap();
        let vent = found.found.iter().find(|c| c.kind() == "vent").unwrap_or_else(|| panic!("no vent: {}", found.text()));
        assert_eq!((vent.to.as_str(), vent.on, vent.text().as_str()), ("tiny2", 0, "to tiny2 on 0 by a vent, 6 cells (v03)"));
        assert!(!vent.route.pieces[0].ducts.is_empty()); // (it slopes between the storeys)
        let (_, lv) = chosen(&recipe, vent);
        assert_made(&lv, vent);
        // with no vents in the level's options there is none to offer
        let dry = Recipe { options: Options { vents: 0, ..Options::before_bases() }, ..recipe };
        assert!(candidates(&dry, "medium4", -276).unwrap().found.iter().all(|c| c.kind() != "vent"));
    }

    #[test]
    fn candidates_follow_the_recipes_earlier_edits() {
        // one join made, a corridor taken out, and then another join: each is previewed on the
        // network as the edits before it left it
        let mut recipe = Recipe::before_bases(40);
        let first = candidates(&recipe, "tower", 551).unwrap().found[2].clone();
        recipe.edits.push(first.edit());
        let (_, net) = recipe.routed().unwrap();
        let loose = net.routes().iter().find(|r| r.key() != first.route.key() && cut_off(&net, r).is_empty()).unwrap().key().to_string();
        recipe.edits.push(Edit::Remove { route: loose.clone() });
        let found = candidates(&recipe, "tower", 551).unwrap();
        // the door the first join made is taken, and its key is spent
        assert!(found.found.iter().all(|c| c.route.key() != first.route.key() && c.route != first.route));
        assert!(found.found.iter().all(|c| c.cells().is_disjoint(&first.cells())));
        let second = found.found.iter().find(|c| c.to == first.to).expect("another way to the same room").clone();
        let (with, lv) = chosen(&recipe, &second);
        assert_made(&lv, &first);
        assert_made(&lv, &second);
        assert!(lv.net.routes().iter().all(|r| r.key() != loose));
        assert_eq!(with.edits.iter().map(Edit::name).collect::<Vec<_>>(), ["join", "remove", "join"]);
        // a recipe written before joins named their storey loads, and joins as it always did
        let old = "{\"format\": 1, \"seed\": 40, \"edits\": [{\"edit\": \"join\", \"room\": \"tower\", \"storey\": 551, \"to\": \"colossal\"}]}";
        let old = recipe::load(old).unwrap();
        assert_eq!(old.edits, [Edit::Join { room: "tower".into(), storey: 551, to: "colossal".into(), on: Vec::new() }]);
        assert!(old.generate().unwrap().net.routes().iter().any(|r| r.a.room == "tower" && r.a.level == 551 && r.b.room == "colossal"));
    }

    #[test]
    fn a_route_is_taken_out_by_the_edit_that_fits_it() {
        let recipe = Recipe::before_bases(40);
        let lv = recipe.generate().unwrap();
        let (_, net) = recipe.routed().unwrap();
        // a corridor of a loop: the remove edit, by the route's own key whichever piece was named
        let loose = net.routes().iter().find(|r| r.pieces.len() > 1 && cut_off(&net, r).is_empty()).unwrap();
        let done = removal(&recipe, &lv, &loose.pieces[1].key).unwrap();
        assert_eq!(done, Removal::Remove { route: loose.key().to_string() });
        assert_eq!((done.edit(), done.text()), (Edit::Remove { route: loose.key().to_string() }, format!("{} is taken out", loose.key())));
        let without = Recipe { edits: vec![done.edit()], ..recipe.clone() }.generate().unwrap();
        assert!(without.net.routes().iter().all(|r| r.key() != loose.key()));
        // the only way to a room stays, and the reason names the room
        assert_eq!(cut_off(&net, net.routes().iter().find(|r| r.key() == "c17").unwrap()), ["small7"]);
        assert_eq!(removal(&recipe, &lv, "c17").unwrap_err(), "c17 is the only way to small7: without it the level is in two");
        // (and how many more lie past it: some seed in a handful has a corridor like that)
        let deep = (1..12).find_map(|seed| {
            let other = Recipe::before_bases(seed);
            let (_, net) = other.routed().ok()?;
            let r = net.routes().iter().find(|r| cut_off(&net, r).len() > 1)?;
            Some((r.key().to_string(), cut_off(&net, r), removal(&other, &other.generate().ok()?, r.key())))
        });
        let (key, past, refused) = deep.expect("a corridor with more than one room past it");
        let s = if past.len() == 2 { "" } else { "s" };
        assert_eq!(
            refused.unwrap_err(),
            format!("{key} is the only way to {} and {} room{s} past it: without it the level is in two", past[0], past.len() - 1)
        );
        // one the dead-end pass added is not in the recipe's network: its dead end is kept instead
        let Some(Fix::Joined { room, by, .. }) = lv.fixes.first() else { panic!("{:?}", lv.fixes) };
        assert_eq!((room.as_str(), by.as_str()), ("spire1", "c39a"));
        let done = removal(&recipe, &lv, "c39b").unwrap();
        assert_eq!(done, Removal::Keep { route: "c39a".into(), room: "spire1".into() });
        assert_eq!(done.edit(), Edit::Keep { room: "spire1".into() });
        let kept = Recipe { edits: vec![done.edit()], ..recipe.clone() }.generate().unwrap();
        assert!(kept.net.routes().iter().all(|r| r.key() != "c39a") && kept.dead_ends.iter().any(|e| !e.allowed));
        assert_eq!(removal(&recipe, &lv, "c99").unwrap_err(), "no route c99 in this level");
        // a join made by hand is a route of the recipe's, and goes by the remove edit
        let joined = candidates(&recipe, "tower", 551).unwrap().found[2].clone();
        let (with, lv) = chosen(&recipe, &joined);
        assert_eq!(removal(&with, &lv, joined.route.key()).unwrap(), Removal::Remove { route: joined.route.key().to_string() });
    }
}
