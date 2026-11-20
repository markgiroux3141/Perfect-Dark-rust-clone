//! A whole level from a seed, stage by stage. From `autolevel/level.py`.
//!
//! ```text
//! let lv = generate(87, &Options::default())?;   // Err if this seed can't be finished
//! lv.plan                 // rooms, corridors and the links between them
//! lv.glass, lv.pools      // what the plan can't hold: window glass, where water is
//! lv.walks                // routes for a walker: every corridor, every room door to door
//! lv.dead_ends, lv.fixes  // what still hangs off the level by one way in, and what was done about the rest
//! lv.report               // numbers: graph, sizes, what was built, score
//! ```
//!
//! Each stage is a function of the one before, so a level can stop after any of them, take an
//! edit and carry on (the editor's way in), and so each can be held to the answer key on its
//! own: `stage_spaces` (the lots placed, then `stage_rooms`), `stage_network`, `stage_interiors`,
//! `stage_dead_ends`, `stage_level`.
//! The last one's parts are in `windows`, `planner`, `walks` and `report`.
//!
//! Nothing is authored: the same seed gives the same level, and a different seed a different one.

use crate::build::{Build, Fix, Grate, Sided};
use crate::cells::{Cell, Rect};
use crate::deadends::DeadEnd;
use crate::furnish::{pod_interior, Interior, PodPlan};
use crate::network::{components, connect, Connect, Network};
use crate::place::{lots, Siting};
use crate::plan::{Plan, Point3};
use crate::pods::{pod_access, Access};
use crate::py;
use crate::pyrand::PyRandom;
use crate::report::{report, Report};
use crate::scatter::Lot;
use crate::space::{find, spaces, Space};
use crate::{planner, spiral, walks, windows};
use std::sync::Arc;

/// What kind of level to make (`level.Options` in the Python; fields arrive with the stages
/// that read them).
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Whose levels: "complex" (the default: everything below) or "felicity" (`felicity`: a
    /// building cut into rooms; it takes none of the options below).
    pub style: &'static str,
    /// A Felicity level's washrooms have their fittings (stalls, toilets, basins): on unless told
    /// otherwise; off leaves them bare and changes nothing else.
    pub washroom: bool,
    /// Which rooms are scattered, how many, and where they may fall.
    pub siting: Siting,
    /// What stands on the floors: "pillars" (floor to ceiling), "crates" (low housings), "none",
    /// or "mixed" (each room takes pillars or nothing).
    pub cover: &'static str,
    /// How much of it, 0 to 1.
    pub clutter: f64,
    /// Catwalks, bridges and open ramps this many cells wide (1: as the doorways).
    pub walkway: i32,
    /// "mixed" rooms may take crates too. (The player can't get on them, so by default a room
    /// that would have had them keeps only the pillar that breaks its long lane.)
    pub crates: bool,
    /// Single-storey rooms may be L-shaped or have a cross wall.
    pub shapes: bool,
    /// Enclosed ramp corridors between rooms two or more storeys apart.
    pub risers: usize,
    /// A one-door room gets a second door to a neighbour within this many cells (0: off).
    pub second: i32,
    /// Crawl ducts: for rooms still left with one door, then as short cuts.
    pub vents: usize,
    /// Upper corridors that run past a hall open onto it (glazed, or a railed balcony).
    pub windows: usize,
    /// Holes in a room's floor into the room underneath.
    pub drops: usize,
    /// Walkable grates in a corridor's floor over a room, with a beam of light through.
    pub grates: usize,
    /// Platforms off a hall's catwalks into its empty air: one for each 100 cells of it, times
    /// this (0: none, as the Python). New in the Rust (`furnish::platforms`).
    pub platforms: f64,
    /// Galleries in each towering hall, at most: high storeys with no pod given a catwalk of their
    /// own, reached from outside (`galleries`; 0: none, as the Python). New in the Rust.
    pub galleries: usize,
    /// The chance each platform has a drop through it to what stands under it (`furnish::pits`;
    /// 0: none, as the Python). New in the Rust.
    pub holes: f64,
    /// The chance a hall on the ground floor has a cellar under it, a drop through a hatch and a
    /// ladder back (`cellars`; 0: none, as the Python). New in the Rust.
    pub cellars: f64,
    /// Angled catwalks: the chance a big hall has an island reached only by them, and each of its
    /// catwalk storeys a shortcut across its air (`furnish::angled`; 0: none, as the Python). New
    /// in the Rust.
    pub angled: f64,
    /// The chance a room's floor is water under railed walkways that join its doors, with
    /// platforms and angled catwalks over it (`floods`, `furnish::flood`). New in the Rust.
    pub flooded: f64,
    /// The chance each room where it fits is shaped: an L or a U cut from its rectangle
    /// (`shapes`; `SHAPED` by default; 0: none, as the Python). New in the Rust.
    pub shaped: f64,
    /// The chance each room with open floor enough for it has a sunken floor: a rectangle of it a
    /// cell lower, open all round, a ramp down into it (`sinks`, `furnish::sunken`; `SUNKEN` by
    /// default; 0: none, as the Python). New in the Rust.
    pub sunken: f64,
    /// Join up, or cut short, every dead end longer than these (false: leave them):
    pub dead_ends: bool,
    /// cells of corridor to a room that goes nowhere (a camping spot may be that far off),
    pub dead_corridor: i32,
    /// cells walked to its far end, ramps and decks too,
    pub dead_walk: i32,
    /// and storeys climbed to a deck with no room on it (1: no balconies either).
    pub dead_climb: i32,
    /// Loops to add to the spanning tree, as a share of the number of rooms.
    pub loops: f64,
    /// How much further apart rooms with no storey in common count.
    pub level_change: f64,
    /// Times a room is furnished, each with its own random numbers, before the level is given up.
    pub tries: usize,
    /// How the level begins: the seed's own rooms, or none. Not one of the recipe's options (it
    /// is the recipe's own field, "start"), but here so every stage can see it.
    pub start: Start,
}

/// How a level begins: with the rooms the seed scatters (`Seed`: every seed's level), or with
/// none (`Empty`: a person draws them, and the seed only decorates them). New here: the Python
/// has no empty start. In an empty start each room, drop, window and corridor draws its random
/// numbers from a stream of its own (`Options::stream`), so an edit re-draws only what it
/// touches; a seed's level keeps the level-wide streams it has always had.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    #[default]
    Seed,
    Empty,
}

/// What `Options::style` may be.
pub const STYLES: [&str; 3] = ["complex", "felicity", "sculpt"];

/// What `Options::cover` may be.
pub const COVER: [&str; 4] = ["mixed", "pillars", "crates", "none"];

impl Default for Options {
    fn default() -> Self {
        Options {
            style: "complex",
            washroom: true,
            siting: Siting::default(),
            cover: "mixed",
            clutter: 0.5,
            walkway: 1,
            crates: false,
            shapes: true,
            risers: 2,
            second: 10,
            vents: 4,
            windows: 4,
            drops: 2,
            grates: 3,
            platforms: 0.0,
            galleries: 0,
            holes: 0.0,
            cellars: 0.0,
            angled: 0.0,
            flooded: FLOODED,
            shaped: SHAPED,
            sunken: SUNKEN,
            dead_ends: true,
            dead_corridor: 8,
            dead_walk: 20,
            dead_climb: 2,
            loops: 0.4,
            level_change: 10.0,
            tries: 8,
            start: Start::Seed,
        }
    }
}

impl Options {
    /// The defaults with the Rust's own rooms that are on by default left out (flooded, shaped and
    /// sunken: `FLOODED`, `SHAPED`, `SUNKEN`): a seed's level by the Python's rules, and the Rust's
    /// own that are always on. What a test pins a seed's level by, so that a feature on by default
    /// moves no pin.
    pub fn python() -> Options {
        Options { flooded: 0.0, shaped: 0.0, sunken: 0.0, ..Options::default() }
    }

    /// A catwalk's depth from its wall, in half cells.
    pub fn band(&self) -> i32 {
        2 * self.walkway
    }

    /// Err if the options make no sense (what `generate` refuses before it starts).
    pub fn check(&self) -> Result<(), String> {
        if !STYLES.contains(&self.style) {
            return Err(format!("style: one of ({})", STYLES.map(py::repr).join(", ")));
        }
        if !COVER.contains(&self.cover) {
            return Err(format!("cover: one of ({})", COVER.map(py::repr).join(", ")));
        }
        if !matches!(self.walkway, 1 | 2) {
            return Err("walkway: 1 or 2 cells".to_string());
        }
        if self.start == Start::Empty && self.style != "complex" {
            return Err(format!("an empty start is for a complex level, not a {} one", self.style));
        }
        Ok(())
    }

    /// Where one thing's random numbers come from. A seed's level draws them, in order, from the
    /// level's one stream (`shared`), as the Python does; an empty start gives the thing a stream
    /// of its own (`PyRandom::named(name())`, kept in `own`), so that adding a room elsewhere
    /// does not move them.
    pub fn stream<'a>(&self, shared: &'a mut PyRandom, own: &'a mut Option<PyRandom>, name: impl FnOnce() -> String) -> &'a mut PyRandom {
        match self.start {
            Start::Seed => shared,
            Start::Empty => own.insert(PyRandom::named(&name())),
        }
    }
}

/// The rooms before anything joins them.
#[derive(Clone, Debug, PartialEq)]
pub struct Spaces {
    /// Every room, in the order it was scattered (pods with no way in left out).
    pub sp: Vec<Space>,
    /// For each spire: (the wall its first ramp climbs, which way round).
    pub spin: Vec<(String, (usize, i32))>,
    /// For each pod: where it opens into its hall.
    pub access: Vec<(String, Access)>,
    /// Pods left out because nothing reaches them.
    pub dropped: Vec<String>,
}

/// The rooms scattered, settled and drawn together, the spires' spirals laid out (fixed before
/// anything is routed), and pods with no way in left out. Err if the site has no room for a lot.
pub fn stage_spaces(seed: u64, o: &Options) -> Result<Spaces, String> {
    Ok(stage_rooms(seed, o, &lots(seed, &o.siting)?.placed))
}

/// The rest of `stage_spaces`, from the lots as placed: the rooms, the spires' spirals and the
/// pods' ways in. (Its own stage so that a recipe can change the lots by hand first.)
pub fn stage_rooms(seed: u64, o: &Options, placed: &[Lot]) -> Spaces {
    let (placed, made) = cellars(seed, o, placed);
    let mut sp = spaces(&placed);
    for (key, c) in made {
        let h = |r: crate::cells::Rect| (2 * r.0, 2 * r.1, 2 * r.2, 2 * r.3);
        // no door of either room opens onto the hatch (the hall's, nor a cellar door under it)
        let hall = sp.iter_mut().find(|s| s.key == c.hall).expect("a cellar's hall");
        hall.clear.extend(crate::cells::rect_cells(crate::furnish::grow(h(c.hatch), 2)));
        let s = sp.iter_mut().find(|s| s.key == key).expect("the cellar");
        s.clear.extend(crate::cells::rect_cells(h(c.hatch)));
        s.cellar = Some(c);
    }
    let mut spin = Vec::new();
    for s in sp.iter_mut().filter(|s| s.kind == "spire") {
        let mut rng = PyRandom::named(&format!("{seed}/{}/spiral", s.key));
        let start = rng.below(4) as usize;
        let turn = *rng.choice(&[1, -1]);
        spin.push((s.key.clone(), (start, turn)));
        if let Some(lay) = spiral::layout(s.rect_h(), &s.storeys(), start, turn, o.band()) {
            s.preset = lay.ramps;
            s.decks = Some(lay.decks);
            s.clear = lay.clear;
        }
    }
    // pods need a way in from their hall, or they are left out
    let (mut access, mut dropped) = (Vec::new(), Vec::new());
    let pods: Vec<String> = sp.iter().filter(|s| !s.host.is_empty()).map(|s| s.key.clone()).collect();
    for key in pods {
        let pod = find(&sp, &key);
        let others: Vec<&Space> = sp.iter().filter(|q| q.host == pod.host).collect();
        match pod_access(find(&sp, &pod.host), pod, &others, o.band()) {
            Some(acc) => access.push((key, acc)),
            None => {
                sp.retain(|s| s.key != key);
                dropped.push(key);
            }
        }
    }
    // a towering hall's high storeys are its pods' (`Space::high`): none where a pod was left out
    let held: Vec<(String, i32)> = sp.iter().filter(|s| !s.host.is_empty()).map(|s| (s.host.clone(), s.floor)).collect();
    for s in sp.iter_mut() {
        s.high.retain(|z| held.iter().any(|(h, f)| *h == s.key && f == z));
    }
    galleries(seed, o, &mut sp);
    shapes(seed, o, &mut sp, &access);
    floods(seed, o, &mut sp);
    sinks(seed, o, &mut sp);
    Spaces { sp, spin, access, dropped }
}

/// The chance a room is flooded unless told otherwise (`Options::flooded`; the user's call,
/// 2026-11-19: on by default, low).
pub const FLOODED: f64 = 0.1;

/// The chance a room where a cut fits is shaped unless told otherwise (`Options::shaped`; the
/// user's call, 2026-11-19: on by default, low, and only in rooms big enough for it).
pub const SHAPED: f64 = 0.1;
/// The least a shaped room's arms are across, cells: a hall's (its catwalks run round the cut) and a
/// room of one storey's; how far across a room of one storey is at least (a hall: twice its arms);
/// and how much of each side a cut may take.
pub const ARM: i32 = 5;
pub const ARM_LOW: i32 = 3;
pub const LOW_LEAST: i32 = 8;
const CUT: (f64, f64) = (0.3, 0.45);

/// Shaped rooms (`Options::shaped`): each room where a cut fits, with that chance from its own
/// stream ("{seed}/{key}/shape"), has a rectangle cut from it: a corner (an L) or the middle of a
/// side (a U), 30 to 45% of the room each way, its arms `ARM` cells across or more, two cells clear
/// of its pods' blocks and their ways in (`ARM_LOW` in a room of one storey). Not a pod, spire,
/// basin, atrium, cellar or cellar's hall, nor a hall under twice its arms across or a room of one
/// storey under `LOW_LEAST`. Ten tries, then it stays a rectangle. New in the Rust.
pub fn shapes(seed: u64, o: &Options, sp: &mut [Space], access: &[(String, Access)]) {
    // (an empty start's rooms are drawn by hand: each is as it was drawn)
    if o.shaped <= 0.0 || o.start == Start::Empty {
        return;
    }
    let halls: Vec<String> = sp.iter().filter_map(|s| s.cellar.as_ref().map(|c| c.hall.clone())).collect();
    for n in 0..sp.len() {
        let s = sp[n].clone();
        let (w, d) = (s.x1 - s.x0, s.y1 - s.y0);
        let (arm, least) = if s.tall() { (ARM, 2 * ARM) } else { (ARM_LOW, LOW_LEAST) };
        let fits = s.host.is_empty()
            && s.decks.is_none()
            && s.pool.is_none()
            && s.cellar.is_none()
            && !halls.contains(&s.key)
            && !matches!(s.kind, "spire" | "basin")
            && s.kind != crate::place::ATRIUM.name
            && w.min(d) >= least;
        if !fits {
            continue;
        }
        let mut rng = PyRandom::named(&format!("{seed}/{}/shape", s.key));
        if rng.random() >= o.shaped {
            continue;
        }
        // what the cut keeps clear of: its pods' blocks and their ways in (half cells)
        let mut keep = crate::cells::Cells::new();
        for p in sp.iter().filter(|p| p.host == s.key) {
            keep.extend(crate::cells::rect_cells(crate::furnish::grow(crate::space::shell_rect(p, &s), 4)));
        }
        for (_, a) in access.iter().filter(|(k, _)| sp.iter().any(|p| p.key == *k && p.host == s.key)) {
            keep.extend(a.pad.iter().flat_map(|c| crate::cells::rect_cells((c.0 - 4, c.1 - 4, c.0 + 5, c.1 + 5))));
        }
        for _ in 0..10 {
            let cw = py::round(w as f64 * rng.uniform(CUT.0, CUT.1)) as i32;
            let cd = py::round(d as f64 * rng.uniform(CUT.0, CUT.1)) as i32;
            let cut = if rng.random() < 0.5 {
                // an L: a corner
                let x = if rng.random() < 0.5 { s.x0 } else { s.x1 - cw };
                let y = if rng.random() < 0.5 { s.y0 } else { s.y1 - cd };
                (x, y, x + cw, y + cd)
            } else {
                // a U: the middle of a side
                let (mx, my) = ((s.x0 + s.x1 - cw).div_euclid(2), (s.y0 + s.y1 - cd).div_euclid(2));
                match rng.below(4) {
                    0 => (mx, s.y0, mx + cw, s.y0 + cd),
                    1 => (mx, s.y1 - cd, mx + cw, s.y1),
                    2 => (s.x0, my, s.x0 + cw, my + cd),
                    _ => (s.x1 - cw, my, s.x1, my + cd),
                }
            };
            let (x0, y0, x1, y1) = cut;
            // the arms left either side of it, and past it
            let arms = [x0 - s.x0, s.x1 - x1, y0 - s.y0, s.y1 - y1].into_iter().filter(|&a| a > 0).all(|a| a >= arm);
            let clear = crate::cells::rect_cells(crate::furnish::grow((2 * x0, 2 * y0, 2 * x1, 2 * y1), 4)).is_disjoint(&keep);
            if cw >= 2 && cd >= 2 && arms && clear {
                sp[n].cuts.push(cut);
                break;
            }
        }
    }
}

/// How far a flooded room's water lies under its floor, units (half a cell: under the walkways'
/// slabs, 40 deep).
pub const FLOOD_DEPTH: i32 = 59;
/// A flooded room's shorter side is at least this, cells.
pub const FLOOD_NARROWEST: i32 = 5;

/// Flooded rooms (`Options::flooded`): each room, with that chance from its own stream
/// ("{seed}/{key}/flooded"), has its floor `FLOOD_DEPTH` under water (`Space::flood`), unless it
/// is a pod, a spire, a basin, an atrium, a cellar or a cellar's hall, narrower than
/// `FLOOD_NARROWEST`, or has a room under it where the water's pit would be. New in the Rust.
pub fn floods(seed: u64, o: &Options, sp: &mut [Space]) {
    if o.flooded <= 0.0 {
        return;
    }
    let halls: Vec<String> = sp.iter().filter_map(|s| s.cellar.as_ref().map(|c| c.hall.clone())).collect();
    let all: Vec<Space> = sp.to_vec();
    for s in sp.iter_mut() {
        let (w, d) = (s.x1 - s.x0, s.y1 - s.y0);
        if !s.host.is_empty()
            || s.decks.is_some()
            || s.pool.is_some()
            || s.cellar.is_some()
            || !s.cuts.is_empty()
            || halls.contains(&s.key)
            || w.min(d) < FLOOD_NARROWEST
        {
            continue;
        }
        if matches!(s.kind, "spire" | "basin") || s.kind == crate::place::ATRIUM.name {
            continue;
        }
        let water = s.floor - FLOOD_DEPTH;
        let pit = (s.x0 - 1, s.y0 - 1, s.x1 + 1, s.y1 + 1);
        let under = all.iter().any(|b| b.key != s.key && b.floor < s.floor && b.top + 39 > water - 40 && crate::scatter::rect_gap(pit, b.rect()) == (0, 0));
        if !under && PyRandom::named(&format!("{seed}/{}/flooded", s.key)).random() < o.flooded {
            s.flood = Some(water);
        }
    }
}

/// The chance a room with open floor enough for it has a sunken floor unless told otherwise
/// (`Options::sunken`; the user's call, 2026-11-19: on by default, low).
pub const SUNKEN: f64 = 0.1;
/// How far a sunken floor lies under the room's, units (a cell: the user's call, 2026-11-19).
pub const SUNK_DEPTH: i32 = 118;
/// A sunken floor's sides, cells (the least is the ramp's run and two cells of floor), and how far
/// it keeps from the room's walls (the floor in front of its doors and along its catwalks' ramps).
pub const SUNK_SIDE: (i32, i32) = (4, 7);
pub const SUNK_INSET: i32 = 3;

/// Sunken floors (`Options::sunken`): each room with that chance from its own stream
/// ("{seed}/{key}/sunken") has a rectangle of its floor `SUNK_DEPTH` lower (`Space::sunk`), its
/// sides `SUNK_SIDE` cells, `SUNK_INSET` clear of the walls and two cells clear of its pods' blocks,
/// with nothing under it where its pit would be. Not a pod, tower, spire, basin, atrium, cellar or
/// cellar's hall, nor a flooded or shaped room; none in an empty start. Ten tries, then none. The
/// furnisher lays its ramp (`furnish::sunken`). New in the Rust.
pub fn sinks(seed: u64, o: &Options, sp: &mut [Space]) {
    if o.sunken <= 0.0 || o.start == Start::Empty {
        return;
    }
    let halls: Vec<String> = sp.iter().filter_map(|s| s.cellar.as_ref().map(|c| c.hall.clone())).collect();
    let all: Vec<Space> = sp.to_vec();
    for s in sp.iter_mut() {
        let (w, d) = (s.x1 - s.x0, s.y1 - s.y0);
        let room = SUNK_SIDE.0 + 2 * SUNK_INSET;
        let fits = s.host.is_empty()
            && s.decks.is_none()
            && s.pool.is_none()
            && s.flood.is_none()
            && s.cellar.is_none()
            && s.cuts.is_empty()
            && !halls.contains(&s.key)
            && !matches!(s.kind, "spire" | "basin")
            && s.kind != crate::place::ATRIUM.name
            && w.min(d) >= room;
        if !fits {
            continue;
        }
        let mut rng = PyRandom::named(&format!("{seed}/{}/sunken", s.key));
        if rng.random() >= o.sunken {
            continue;
        }
        // what it keeps clear of: its pods' blocks and their ways in (half cells)
        let mut keep = crate::cells::Cells::new();
        for p in all.iter().filter(|p| p.host == s.key) {
            keep.extend(crate::cells::rect_cells(crate::furnish::grow(crate::space::shell_rect(p, s), 4)));
        }
        let bottom = s.floor - SUNK_DEPTH;
        for _ in 0..10 {
            let sw = rng.randint(SUNK_SIDE.0 as i64, SUNK_SIDE.1.min(w - 2 * SUNK_INSET) as i64) as i32;
            let sd = rng.randint(SUNK_SIDE.0 as i64, SUNK_SIDE.1.min(d - 2 * SUNK_INSET) as i64) as i32;
            let x = rng.randint((s.x0 + SUNK_INSET) as i64, (s.x1 - SUNK_INSET - sw) as i64) as i32;
            let y = rng.randint((s.y0 + SUNK_INSET) as i64, (s.y1 - SUNK_INSET - sd) as i64) as i32;
            let r = (x, y, x + sw, y + sd);
            let clear = crate::cells::rect_cells((2 * r.0, 2 * r.1, 2 * r.2, 2 * r.3)).is_disjoint(&keep);
            let pit = (r.0 - 1, r.1 - 1, r.2 + 1, r.3 + 1);
            let under =
                all.iter().any(|b| b.key != s.key && b.floor < s.floor && b.top + 39 > bottom - 40 && crate::scatter::rect_gap(pit, b.rect()) == (0, 0));
            if clear && !under {
                s.sunk = Some(r);
                break;
            }
        }
    }
}

/// Cells a cellar keeps from its hall's walls, all round (a corridor's way past it, and the floor
/// in front of the hall's doors).
pub const CELLAR_INSET: i32 = 3;

/// The lots with cellars added (`Options::cellars`), and each cellar's hall and hatch. A hall
/// (a colossal, vast or tower lot on the ground floor) takes one with that chance, drawn from its
/// own stream ("{seed}/{key}/cellar"): a `medium` room on the storey under it (-276, one storey
/// tall), 4 to 9 cells by 4 to 8, inside its footprint `CELLAR_INSET` cells from its walls, clear
/// of every other lot by the scatter's rule. Its hatch is two cells square against one of its
/// walls, a cell from its corners, and clear of the hall's pods. Twenty tries, then none. New in
/// the Rust; with the option at 0 (the default) the lots are as they were.
pub fn cellars(seed: u64, o: &Options, lots: &[Lot]) -> (Vec<Lot>, Vec<(String, crate::space::Cellar)>) {
    use crate::scatter::{clear, rect_gap};
    let mut out = lots.to_vec();
    let mut made = Vec::new();
    if o.cellars <= 0.0 {
        return (out, made);
    }
    for hall in lots.iter().filter(|l| l.host.is_empty() && l.floor == 0 && matches!(l.kind, "colossal" | "vast" | "tower")) {
        let mut rng = PyRandom::named(&format!("{seed}/{}/cellar", hall.key));
        if rng.random() >= o.cellars {
            continue;
        }
        let (room_w, room_d) = (hall.x1 - hall.x0 - 2 * CELLAR_INSET, hall.y1 - hall.y0 - 2 * CELLAR_INSET);
        if room_w < 4 || room_d < 4 {
            continue;
        }
        let pods: Vec<&Lot> = lots.iter().filter(|p| p.host == hall.key).collect();
        for _ in 0..20 {
            let w = rng.randint(4, room_w.min(9) as i64) as i32;
            let d = rng.randint(4, room_d.min(8) as i64) as i32;
            let x = rng.randint((hall.x0 + CELLAR_INSET) as i64, (hall.x1 - CELLAR_INSET - w) as i64) as i32;
            let y = rng.randint((hall.y0 + CELLAR_INSET) as i64, (hall.y1 - CELLAR_INSET - d) as i64) as i32;
            let key = format!("cellar{}", made.len() + 1);
            let lot = Lot { key: key.clone(), kind: "medium", x0: x, y0: y, x1: x + w, y1: y + d, floor: -276, top: -40, host: String::new() };
            if !out.iter().all(|q| q.key == hall.key || clear(&lot, q, crate::place::GAP)) {
                continue;
            }
            let side = rng.below(4) as usize;
            let along = if side.is_multiple_of(2) { w } else { d };
            let a = rng.randint(1, (along - 3) as i64) as i32;
            let hatch = match side {
                0 => (x + a, y, x + a + 2, y + 2),
                1 => (x + w - 2, y + a, x + w, y + a + 2),
                2 => (x + a, y + d - 2, x + a + 2, y + d),
                _ => (x, y + a, x + 2, y + a + 2),
            };
            if pods.iter().any(|p| {
                let (gx, gy) = rect_gap(hatch, p.rect());
                gx.max(gy) < 1
            }) {
                continue;
            }
            out.push(lot);
            made.push((key, crate::space::Cellar { hall: hall.key.clone(), hatch, side }));
            break;
        }
    }
    (out, made)
}

/// Storeys between a gallery and any other high storey of its hall, or the top of the storeys
/// ramped inside it.
pub const GALLERY_GAP: usize = 2;

/// A towering hall's galleries (`Space::galleries`): up to `o.galleries` of its storeys over the
/// ramped ones that hold no pod, `GALLERY_GAP` storeys or more from each other, its pods' and its
/// ramped storeys, and at a height some other room has a door on (else no corridor could reach
/// it). Each becomes a high storey (`Space::high`): a place of its own that the router must reach
/// from outside, through other rooms, as it does a high pod. Drawn from the hall's own stream,
/// "{seed}/{key}/galleries", so nothing else in the level moves; none unless asked for.
fn galleries(seed: u64, o: &Options, sp: &mut [Space]) {
    use crate::storeys::{index, LEVELS};
    if o.galleries == 0 {
        return;
    }
    let reach: Vec<(String, Vec<i32>)> = sp.iter().filter(|s| s.host.is_empty()).map(|s| (s.key.clone(), s.door_levels())).collect();
    for s in sp.iter_mut().filter(|s| s.host.is_empty() && s.towers() && s.decks.is_none()) {
        let first = index(s.floor) + crate::space::ramped(s.kind, s.area());
        let taken: Vec<usize> = s.high.iter().map(|&z| index(z)).chain(std::iter::once(first - 1)).collect();
        let mut free: Vec<i32> = (first..LEVELS.len())
            .filter(|&i| LEVELS[i] + crate::scatter::STOREY <= s.top)
            .filter(|&i| taken.iter().all(|&t| i.abs_diff(t) >= GALLERY_GAP))
            .map(|i| LEVELS[i])
            .filter(|z| reach.iter().any(|(k, levels)| *k != s.key && levels.contains(z)))
            .collect();
        PyRandom::named(&format!("{seed}/{}/galleries", s.key)).shuffle(&mut free);
        let mut chosen: Vec<i32> = Vec::new();
        for z in free {
            if chosen.len() < o.galleries && chosen.iter().all(|&c| index(c).abs_diff(index(z)) >= GALLERY_GAP) {
                chosen.push(z);
            }
        }
        chosen.sort();
        s.high.extend(chosen.iter().copied());
        s.high.sort();
        s.galleries = chosen;
    }
}

/// Every corridor, stub and vent between the rooms. Err if a room is cut off.
pub fn stage_network(seed: u64, o: &Options, sp: &[Space]) -> Result<Network, String> {
    let how = Connect { loops: o.loops, level_change: o.level_change, risers: o.risers, second: o.second, vents: o.vents, ..Connect::default() };
    let mut net = connect(sp.to_vec(), o.band(), &how);
    // a towering hall's high storey that no route reached (nothing stands that high near enough):
    // its pods (or its gallery) are left out, and the level is routed again without them
    let cut = unreached(&net);
    if !cut.is_empty() {
        let mut sp = sp.to_vec();
        sp.retain(|s| !cut.contains(&(s.host.clone(), s.floor)));
        for s in sp.iter_mut() {
            s.high.retain(|z| !cut.contains(&(s.key.clone(), *z)));
            s.galleries.retain(|z| !cut.contains(&(s.key.clone(), *z)));
        }
        net = connect(sp, o.band(), &how);
    }
    if components(&net) != 1 {
        return Err(format!("seed {seed}: rooms cut off ({} links found no corridor)", net.failed.len()));
    }
    Ok(net)
}

/// The high storeys (`Space::high`) no route arrives on, as (hall, storey): neither the hall nor a
/// pod on that storey has a door there.
fn unreached(net: &Network) -> Vec<(String, i32)> {
    let at = |room: &str, z: i32| net.routes().iter().any(|r| [&r.a, &r.b].iter().any(|d| d.room == room && d.level == z));
    let mut out = Vec::new();
    for s in net.spaces().iter().filter(|s| !s.high.is_empty()) {
        for &z in &s.high {
            let pods = net.spaces().iter().filter(|p| p.host == s.key && p.floor == z);
            if !at(&s.key, z) && !pods.clone().any(|p| net.routes().iter().any(|r| r.a.room == p.key || r.b.room == p.key)) {
                out.push((s.key.clone(), z));
            }
        }
    }
    out
}

/// Openings (drops and grates) and every room's inside, before anything is done about dead ends:
/// the build, with its `interiors`, `drops` and `grates`. The rooms are the network's (the
/// Python passes `sp` as well; it is the same dict). Err if a room can't be furnished.
pub fn stage_interiors(seed: u64, o: &Options, net: Network, access: Vec<(String, Access)>, spin: Vec<(String, (usize, i32))>) -> Result<Build, String> {
    let mut b = Build::new(seed, o.clone(), net, access, spin);
    b.refresh()?;
    Ok(b)
}

/// The dead-end pass (unless the level's options say to leave them): every dead end that is too
/// long, too far or too high is joined up or cut short, and the rooms that touches are furnished
/// again. What was done is in the build's `fixes`. Err if the level can't be put right.
pub fn stage_dead_ends(mut b: Build) -> Result<Build, String> {
    if b.o.dead_ends {
        b.resolve()?;
    }
    Ok(b)
}

/// Solid tags whose tops players stand on, in every room of a level's plan.
pub const WALKABLE: [&str; 4] = ["trim", "grate", "core", "ledge"];
/// The kinds of room that are landmarks (hubs in the plan).
pub const HUBS: [&str; 7] = ["colossal", "vast", "spire", "tower", "basin", "long", "atrium"];
/// Names for the rooms of each kind, dealt out at random (in this order of kinds).
pub const NAMES: [(&str, &[&str]); 10] = [
    ("colossal", &["The Drydock", "The Foundry", "The Assembly Hall", "The Turbine Hall"]),
    ("vast", &["The Hangar", "The Depot", "The Machine Hall", "The Works"]),
    ("spire", &["The Spire", "The Flue", "The Lift Shaft", "The Stack"]),
    ("tower", &["The Shaft", "The Chimney", "The Silo", "The Well"]),
    ("loft", &["The Loft", "Gantry Room", "Upper Stores", "Signal Room", "The Crow's Nest", "High Relay"]),
    ("long", &["The Long Gallery", "The Concourse", "The Arcade"]),
    ("basin", &["The Reservoir", "The Cistern", "The Basin"]),
    ("medium", &["Workshop", "Pump Hall", "The Exchange", "Stores", "Plant Room", "Switch Hall", "The Annexe"]),
    ("small", &["Relay", "Control Room", "Gauge Room", "Monitor Room", "Valve Room", "Cellar", "Booth", "Store", "Meter Room", "Lock-up"]),
    ("tiny", &["Lobby", "Closet", "Locker", "Gantry", "Switch Closet", "Nook", "Cupboard"]),
];
/// An atrium's names. Not in `NAMES`, which a seed's level shuffles kind by kind from its own
/// stream: an atrium draws its name from a stream of its own, as every room of an empty start does.
pub const ATRIUM_NAMES: [&str; 5] = ["The Atrium", "The Light Well", "The Vault", "The Abyss", "The Stairwell"];
/// The wall schemes a room of each kind may be built in.
pub const SCHEMES: [(&str, &[&str]); 11] = [
    ("colossal", &["standard", "striped", "pipework"]),
    ("atrium", &["standard", "striped", "pipework"]),
    ("spire", &["chimney", "shaft", "flue"]),
    ("loft", &["striped", "plain", "pipework"]),
    ("vast", &["standard", "striped"]),
    ("tower", &["chimney", "shaft"]),
    ("long", &["striped", "standard"]),
    ("basin", &["reservoir"]),
    ("medium", &["striped", "pipework", "standard"]),
    ("small", &["standard", "striped", "pipework", "plain"]),
    ("tiny", &["plain", "pipework"]),
];

/// Glass across an opening: (the room it belongs to, its two ends (x, y) in cells, the floor it
/// stands on, the way it faces).
pub type Pane = (String, (f64, f64), (f64, f64), i32, (i32, i32, i32));
/// A beam of light through an opening: (the room lit, the opening (x0, y0, x1, y1) in cells,
/// the height of the opening's top, and of its bottom).
pub type Beam = (String, Rect, i32, i32);
/// A place to look at the level from: (x, y) in cells, the floor stood on, and the (x, y) looked at.
pub type View = (f64, f64, i32, f64, f64);

/// A sloped rail down a ramp's open side: (the room, its two ends in units, the way it faces).
pub type SlopedRail = (String, Point3, Point3, (i32, i32, i32));

/// A doorway through a wall between two rooms, where a door may stand: the Felicity planner's
/// (`felicity`). Complex's levels have none: their openings are corridors' ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Doorway {
    /// The rooms either side: `a` on the low side of the wall, `b` on the high.
    pub a: String,
    pub b: String,
    /// The air through the wall, from one room's face of it to the other's (units). Half of it
    /// is each room's, so the rooms' air meets on the wall's middle plane: the opening.
    pub lo: Point3,
    pub hi: Point3,
    /// The axis the wall is square to (0: a wall along y, 1: along x).
    pub axis: usize,
    /// Its door, if it has one.
    pub door: Option<DoorKind>,
}

impl Doorway {
    /// The wall's middle plane, along `axis` (units).
    pub fn plane(&self) -> i32 {
        let (lo, hi) = (crate::plan::at(self.lo, self.axis), crate::plan::at(self.hi, self.axis));
        (lo + hi) / 2
    }

    /// The opening's width across the doorway and its height (units).
    pub fn size(&self) -> (i32, i32) {
        let across = 1 - self.axis;
        (crate::plan::at(self.hi, across) - crate::plan::at(self.lo, across), self.hi.2 - self.lo.2)
    }
}

/// An arched opening: a rectangular opening whose top corners are filled in under a curve, a
/// semicircle where it rises half its width, else a flatter segment of a circle (drawn by the
/// kit: `autolevel_build::felicity`). The Felicity planner's doorless doorways between corridors,
/// halls and stair rooms, and the arches of the hall's screens. Complex's levels have none.
#[derive(Clone, Debug, PartialEq)]
pub struct Arch {
    /// The rooms either side: `a` on the low side of the wall, `b` on the high (the same room for
    /// a screen standing in one).
    pub a: String,
    pub b: String,
    /// The axis the wall is square to, the wall's two faces along it, and the plane where the
    /// rooms meet.
    pub axis: usize,
    pub t: (i32, i32),
    pub mid: i32,
    /// The opening along the wall, its floor, where its curve springs, and its top (the crown).
    pub s: (i32, i32),
    pub floor: i32,
    pub spring: i32,
    pub top: i32,
    /// "doorway" or "screen": how the kit dresses it.
    pub kind: &'static str,
}

impl Arch {
    /// The curve from one side of the opening to the other as (along the wall, height), in `n`
    /// pieces: a circle through both springing points and the crown.
    pub fn curve(&self, n: usize) -> Vec<(f64, f64)> {
        let (s0, s1) = (self.s.0 as f64, self.s.1 as f64);
        let (c, h) = ((s1 - s0) / 2.0, (self.top - self.spring) as f64);
        let r = (c * c + h * h) / (2.0 * h);
        let (mid, zc) = ((s0 + s1) / 2.0, self.top as f64 - r);
        let (from, to) = ((r - h).atan2(-c), (r - h).atan2(c));
        (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f64 / n as f64;
                // the ends exactly on the springing points, the middle on the crown
                match i {
                    0 => (s0, self.spring as f64),
                    _ if i == n => (s1, self.spring as f64),
                    _ => (mid + r * a.cos(), zc + r * a.sin()),
                }
            })
            .collect()
    }
}

/// What door stands in a doorway.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoorKind {
    /// Swinging (else sliding).
    pub swing: bool,
    /// One leaf, or two that open together (a double doorway).
    pub leaves: u8,
}

/// Where a corridor that runs past a hall is opened onto it.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// The corridor's piece.
    pub piece: String,
    pub hall: String,
    /// The corridor's floor.
    pub z: i32,
    /// The cells of wall taken out, in order along the wall.
    pub wall: Vec<Cell>,
    /// The way from the corridor into the hall: an index into `DIRS`.
    pub d: usize,
    /// "glass" (94 high) or "rail" (a balcony over the hall).
    pub kind: &'static str,
}

/// A point of a walk: (x, y) in cells and the height of the feet there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub x: f64,
    pub y: f64,
    pub z: i32,
    /// None: stop here. 1: pass through. 2: squat on the way here. 3: both.
    pub flags: Option<u8>,
}

impl Step {
    /// A point to stop at.
    pub fn at(p: (f64, f64, i32)) -> Step {
        Step { x: p.0, y: p.1, z: p.2, flags: None }
    }

    /// A point on the way.
    pub fn by(p: (f64, f64, i32), flags: u8) -> Step {
        Step { x: p.0, y: p.1, z: p.2, flags: Some(flags) }
    }
}

/// A finished level (`level.Level`). The rooms are the network's (`spaces()`).
#[derive(Clone, Debug)]
pub struct Level {
    pub seed: u64,
    pub options: Options,
    /// The rooms and every corridor, stub and vent between them.
    pub net: Network,
    /// Every room's inside (pods aside), the rooms in the order they were scattered.
    pub interiors: Vec<(String, Arc<Interior>)>,
    /// Each pod's own inside.
    pub pods: Vec<(String, PodPlan)>,
    pub plan: Plan,
    /// Window glass, which the plan can't hold.
    pub glass: Vec<Pane>,
    /// Room -> the height of its water.
    pub pools: Vec<(String, i32)>,
    /// Routes for a walker, by name, in the order they were made. A name that starts with `!`
    /// must fail (glass and water are not walked through).
    pub walks: Vec<(String, Vec<Step>)>,
    /// Places to look at the level from, by name.
    pub views: Vec<(String, View)>,
    /// Sloped rails down ramps' open sides.
    pub ramp_rails: Vec<SlopedRail>,
    /// Room -> the floor brightness to bake to, where not the usual.
    pub moods: Vec<(String, i32)>,
    /// Room -> its ceiling's role, where not the kit's own choice.
    pub ceilings: Vec<(String, &'static str)>,
    /// Red corner lights: (the corridor's piece, (x, y) in units).
    pub glows: Vec<(String, (i32, i32))>,
    /// Holes in a room's floor into the room underneath, each with its open side.
    pub drops: Vec<Sided>,
    /// Walkable grates in a corridor's floor over a room.
    pub grates: Vec<Grate>,
    pub windows: Vec<Window>,
    /// A hard-edged beam through each drop and grate.
    pub beams: Vec<Beam>,
    /// Pods with no way in, left out.
    pub dropped: Vec<String>,
    /// What still hangs off the level by one way in.
    pub dead_ends: Vec<DeadEnd>,
    /// What was done about the others.
    pub fixes: Vec<Fix>,
    /// Doorways through walls, where doors may stand (a Felicity level's; none in Complex's).
    pub doorways: Vec<Doorway>,
    /// Arched openings (a Felicity level's; none in Complex's).
    pub arches: Vec<Arch>,
    /// Ceiling braces (a Felicity level's dressing, `dress`; none in Complex's).
    pub braces: Vec<crate::dress::Brace>,
    /// Hazard surrounds on doorways' faces (a Felicity level's dressing, `dress`; none in Complex's).
    pub surrounds: Vec<crate::dress::Surround>,
    /// Portals across industrial corridors (a Felicity level's dressing, `dress`; none in Complex's).
    pub portals: Vec<crate::dress::Portal>,
    /// The washrooms' fittings: stalls' partitions, toilets, basins (a Felicity level's dressing,
    /// `dress::washrooms`, unless `Options::washroom` is off; none in Complex's).
    pub fittings: Vec<crate::dress::Fitting>,
    /// Lamps a room is lit by in place of its lamp grid: (room, point). A braced room's, one under
    /// each light panel; none in Complex's.
    pub lamps: Vec<(String, (i32, i32, i32))>,
    pub report: Report,
}

impl Level {
    /// The rooms, in the order they were scattered.
    pub fn spaces(&self) -> &[Space] {
        self.net.spaces()
    }

    /// The room with this key.
    pub fn space(&self, key: &str) -> &Space {
        find(self.net.spaces(), key)
    }

    /// A room's inside. The room must be one that was furnished (not a pod).
    pub fn interior(&self, key: &str) -> &Interior {
        self.interiors.iter().find(|(k, _)| k == key).map_or_else(|| panic!("{key} has no inside"), |(_, it)| it.as_ref())
    }
}

/// The level for a seed. Err if the options make no sense, or this seed can't be finished.
pub fn generate(seed: u64, o: &Options) -> Result<Level, String> {
    o.check()?;
    if o.start == Start::Empty {
        return Err("an empty start is made from a recipe (its rooms are the recipe's)".to_string());
    }
    if o.style == "felicity" {
        return crate::felicity::generate(seed, o);
    }
    if o.style == "sculpt" {
        return crate::sculpt::generate(seed, o);
    }
    let rooms = stage_spaces(seed, o)?;
    let net = stage_network(seed, o, &rooms.sp)?;
    let b = stage_interiors(seed, o, net, rooms.access, rooms.spin)?;
    stage_level(stage_dead_ends(b)?, rooms.dropped)
}

/// The last stage: the furnished build as a level. Each pod's own inside, the beams, the
/// windows, the plan (which must pass its own check), the walks, the views and the report.
/// `dropped` are the pods `stage_spaces` left out.
pub fn stage_level(mut b: Build, dropped: Vec<String>) -> Result<Level, String> {
    let seed = b.seed;
    let sp = b.spaces();
    let mut pods = Vec::new();
    for p in sp.iter().filter(|p| !p.host.is_empty()) {
        let others: Vec<&Space> = sp.iter().filter(|q| q.host == p.host).collect();
        let acc = b.access.iter().find(|(k, _)| *k == p.key).map(|(_, a)| a);
        pods.push((p.key.clone(), pod_interior(find(sp, &p.host), p, acc, &others)));
    }
    let drops = b.sided();
    let mut beams: Vec<Beam> = drops.iter().map(|(u, low, r, _)| (low.clone(), *r, find(sp, u).floor, find(sp, low).top)).collect();
    beams.extend(b.grates.iter().map(|(_, low, r, z)| (low.clone(), *r, *z, find(sp, low).top)));
    let dead_ends = b.ends().1;
    let mut lv = Level {
        seed,
        options: b.o,
        net: b.net,
        interiors: b.interiors,
        pods,
        plan: Plan::new(&format!("Autolevel {seed}")),
        glass: Vec::new(),
        pools: Vec::new(),
        walks: Vec::new(),
        views: Vec::new(),
        ramp_rails: Vec::new(),
        moods: Vec::new(),
        ceilings: Vec::new(),
        glows: Vec::new(),
        drops,
        grates: b.grates,
        windows: Vec::new(),
        beams,
        dropped,
        dead_ends,
        fixes: b.fixes,
        doorways: Vec::new(),
        arches: Vec::new(),
        braces: Vec::new(),
        surrounds: Vec::new(),
        portals: Vec::new(),
        fittings: Vec::new(),
        lamps: Vec::new(),
        report: Report::default(),
    };
    lv.windows = windows::windows(&lv);
    planner::plan(&mut lv, &mut PyRandom::named(&format!("{seed}/plan")))?;
    let problems = lv.plan.check();
    if !problems.is_empty() {
        return Err(format!("seed {seed}: plan check: {}", py::repr_list(&problems[..problems.len().min(3)])));
    }
    walks::walks(&mut lv)?;
    walks::views(&mut lv);
    lv.report = report(&lv);
    Ok(lv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_that_make_no_sense_are_refused() {
        let o = Options { cover: "hedges", ..Options::default() };
        assert_eq!(generate(1, &o).unwrap_err(), "cover: one of ('mixed', 'pillars', 'crates', 'none')");
        let o = Options { walkway: 3, ..Options::default() };
        assert_eq!(generate(1, &o).unwrap_err(), "walkway: 1 or 2 cells");
    }

    #[test]
    fn an_arch_springs_from_both_sides_and_crowns_at_its_top() {
        let arch = |w: i32, spring: i32| Arch {
            a: "a".into(),
            b: "b".into(),
            axis: 0,
            t: (-20, 20),
            mid: 0,
            s: (100, 100 + w),
            floor: 0,
            spring,
            top: 212,
            kind: "doorway",
        };
        // a semicircle on a cell, a flatter segment on two
        for (w, spring) in [(106, 159), (212, 159), (318, 133)] {
            let c = arch(w, spring).curve(8);
            assert_eq!(c.len(), 9);
            assert_eq!((c[0], c[8]), ((100.0, spring as f64), ((100 + w) as f64, spring as f64)));
            let crown = c[4];
            assert!((crown.0 - (100.0 + w as f64 / 2.0)).abs() < 1e-9 && (crown.1 - 212.0).abs() < 1e-9, "{crown:?}");
            // rising to the crown and falling after, all inside the opening
            assert!(c.windows(2).take(4).all(|p| p[1].1 >= p[0].1) && c.windows(2).skip(4).all(|p| p[1].1 <= p[0].1));
            assert!(c.iter().all(|&(x, z)| (100.0..=(100 + w) as f64).contains(&x) && (spring as f64..=212.0).contains(&z)));
        }
    }
}
