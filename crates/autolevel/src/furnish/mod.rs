//! A room's inside, worked out from its doors. A port of `autolevel/furnish.py`.
//!
//! Everything is on a grid of half cells ("h", 59 units), absolute (h = 2 x cell), one layer per
//! storey. For one room:
//!
//! - shells: pods that poke into it, solid blocks with half-cell walls, their roofs a floor;
//! - decks: catwalks along the walls on every upper storey that has a door, a pod's door or (by
//!   the room's kind) a balcony of its own: the shortest arc round the walls that joins them, or
//!   the whole ring. A hall may be given a layout: a ring, galleries down both long walls, or a
//!   mezzanine across one end. A spiral tower's decks and ramps are fixed beforehand (`spiral`);
//! - ramps: every separate piece of deck gets an open 30 degree ramp down to the storey below,
//!   found by trying every place it fits, with a landing two cells square at each end;
//! - pool: a basin's water, in a pit, railed all round, under a bridge. Never a floor;
//! - flood: a flooded room's water, all its floor but railed walkways joining its doors, with
//!   rafts, angled catwalks and tanks (`Space::flood`; new in the Rust, none by default);
//! - hole: a drop through the floor into the room underneath, railed on three sides;
//! - shape: a single-storey room may have a corner filled in (an L) or a cross wall;
//! - platforms: big squares of deck off the catwalks into a hall's emptiest air (by
//!   `Setup::platforms`; new in the Rust, none by default);
//! - angled catwalks: an island in a big hall's empty air reached only by them, and shortcuts
//!   across its air between catwalks and platforms (`Setup::angled`; new in the Rust, none by
//!   default);
//! - pits: drops through the platforms to what stands under them (`Setup::holes`; new in the
//!   Rust, none by default);
//! - cover: pillars, crates or nothing, never in front of a door or cutting the floor in two;
//! - rails: on every edge that drops, except where a ramp arrives, and down ramps' open sides;
//! - nav: every door must reach every other on foot, through spaces a player fits;
//! - rescue: a room no set of ramps joins, on every attempt, is furnished once more with ladders
//!   up the edges of the decks no ramp reaches and short bridges to pieces of deck left apart
//!   (`stitch`; new in the Rust: where the Python gives up, so no level that was made changes).
//!
//! `Interior::new(..).build()` is the room, or why it can't be furnished. The files here follow
//! the Python's sections: `decks`, `ramps`, `nav`, `cover`, `rails`, `plans` (what the room gives
//! the level's plan) and `pod` (a pod's own inside). `Access` and `pod_access` are in `pods`.
//!
//! The Python changes `BAND` and `WIDE_H` at run time (`set_walkway`); here they are the
//! interior's `band`, from the level's `Options::band()`.

use crate::cells::{rect_cells, Cell, Cells, Rect};
use crate::pods::Access;
use crate::pyrand::PyRandom;
use crate::space::{door_pad, shell_rect, Door, Space, DIRS};
use crate::storeys::{index, storey, LEVELS};
use std::collections::{BTreeMap, BTreeSet};

mod angled;
mod cover;
mod decks;
mod flood;
mod mend;
mod nav;
mod pits;
mod plans;
mod platforms;
mod pod;
mod rails;
mod ramps;
mod shape;
mod stitch;
mod sunken;
#[cfg(test)]
mod tests;

pub use angled::{Angled, End as AngledEnd};
pub use cover::Ladder;
pub use decks::SHAPES;
pub use nav::{Nav, Node};
pub use pits::{Pit, MAX_FALL};
pub use platforms::Void;
pub use pod::{pod_interior, Glass, PodDoor, PodPlan};
pub use rails::Rail;
pub use stitch::Chute;
pub use sunken::Sunk;

/// How far in front of a door nothing at all may stand, h.
pub const CLEAR: i32 = 4;
/// A ramp's run, h (four cells: one storey at 30 degrees).
pub const RAMP_H: i32 = 8;
/// A ramp's landings are at least this square, h: room to turn at each end.
pub const LAND: i32 = 4;
/// Complex's glass strips, units high.
pub const WINDOW: i32 = 94;
/// The railing on catwalk edges and down ramps, units high (`complex::RAIL_HEIGHT`).
pub const RAIL: i32 = 86;
/// The least a hall's short side may be for a ring of catwalk on its first upper storey, h,
pub const RING_MIN: i32 = 16;
/// or for galleries down both long walls,
pub const GALLERIES_MIN: i32 = 14;
/// and the least its long side may be for a mezzanine across one end.
pub const MEZZANINE_MIN: i32 = 20;

/// Something standing on the floor: (x0, y0, x1, y1) h, its height in units, and the tag it is
/// built with ("column", "pier", "housing" or "wall").
pub type Cover = (i32, i32, i32, i32, i32, &'static str);
/// A pod's pier: (x0, y0, x1, y1) h, from height a to height b.
pub type Pier = (i32, i32, i32, i32, i32, i32);
/// A solid of the room's own (a tank in the water): (x0, y0, x1, y1) h, heights a to b, its tag.
pub type Solid = (i32, i32, i32, i32, i32, i32, &'static str);

/// An open ramp inside a room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RampIn {
    /// The storey floors it joins.
    pub lo: i32,
    pub hi: i32,
    /// (x0, y0, x1, y1) h.
    pub rect: Rect,
    /// The way it climbs: an index into `DIRS`.
    pub d: usize,
    /// The landing past its top, on `hi`.
    pub top: Cells,
    /// And past its foot, on `lo`.
    pub foot: Cells,
}

impl RampIn {
    /// The nav block (2 x 2 h) just past its top (or its foot), on its centre line.
    pub fn node(&self, top: bool) -> Cell {
        let (x0, y0, x1, y1) = self.rect;
        let (dx, dy) = DIRS[self.d];
        if dx != 0 {
            return (if (dx > 0) == top { x1 } else { x0 - 2 }, y0 + (y1 - y0 - 2).div_euclid(2));
        }
        (x0 + (x1 - x0 - 2).div_euclid(2), if (dy > 0) == top { y1 } else { y0 - 2 })
    }

    pub fn cells(&self) -> Cells {
        rect_cells(self.rect)
    }

    /// Its centre line in cells: (foot (x, y), top (x, y)).
    pub fn line(&self) -> ((f64, f64), (f64, f64)) {
        let (x0, y0, x1, y1) = self.rect;
        let (dx, dy) = DIRS[self.d];
        if dx != 0 {
            let (a, b) = if dx > 0 { (x0, x1) } else { (x1, x0) };
            let y = (y0 + y1) as f64 / 4.0;
            return ((a as f64 / 2.0, y), (b as f64 / 2.0, y));
        }
        let (a, b) = if dy > 0 { (y0, y1) } else { (y1, y0) };
        let x = (x0 + x1) as f64 / 4.0;
        ((x, a as f64 / 2.0), (x, b as f64 / 2.0))
    }
}

/// The floor just inside a door.
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    pub cells: Cells,
    pub level: i32,
    pub door: Option<Door>,
}

/// Bottom and top of a pod's block: the ceiling of the storey below it and the floor of the
/// storey above (so its roof is a floor), or the hall's own ceiling.
pub fn shell_z(p: &Space, host: &Space) -> (i32, i32) {
    let i = index(p.floor);
    let lo = if i > 0 { storey(LEVELS[i - 1]).1 } else { p.floor - 40 };
    let hi = if i + 1 < LEVELS.len() && host.storeys().contains(&LEVELS[i + 1]) { LEVELS[i + 1] } else { host.top };
    (lo, hi)
}

/// A set of grid cells as rectangles (x0, y0, x1, y1) that don't overlap: the lowest row first,
/// each as wide as its row runs and as deep as that width stays filled.
pub fn rectangles(cells: &Cells) -> Vec<Rect> {
    let mut left: BTreeSet<(i32, i32)> = cells.iter().map(|&(x, y)| (y, x)).collect(); // (y, x): the lowest row first
    let mut out = Vec::new();
    while let Some(&(y0, x0)) = left.first() {
        let mut x1 = x0;
        while left.contains(&(y0, x1 + 1)) {
            x1 += 1;
        }
        let mut y1 = y0;
        while (x0..=x1).all(|x| left.contains(&(y1 + 1, x))) {
            y1 += 1;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                left.remove(&(y, x));
            }
        }
        out.push((x0, y0, x1 + 1, y1 + 1));
    }
    out
}

/// A set of cells as its pieces: cells joined edge to edge. (The Python's come in no order;
/// these come by their least cell. Nothing may depend on the order.)
pub fn components(cells: &Cells) -> Vec<Cells> {
    let mut left = cells.clone();
    let mut out = Vec::new();
    while let Some(first) = left.pop_first() {
        let mut todo = vec![first];
        let mut comp = Cells::new();
        while let Some(c) = todo.pop() {
            comp.insert(c);
            for (dx, dy) in DIRS {
                let n = (c.0 + dx, c.1 + dy);
                if left.remove(&n) {
                    todo.push(n);
                }
            }
        }
        out.push(comp);
    }
    out
}

/// Do two cell sets share an edge at least `need` cells long (somewhere to walk through)?
pub fn touching(a: &Cells, b: &Cells, need: usize) -> bool {
    DIRS.iter().any(|&(dx, dy)| a.iter().filter(|c| b.contains(&(c.0 + dx, c.1 + dy))).count() >= need)
}

/// The block position in the middle of a 4 x 4 h landing (or the nearest to it).
fn mid_block(cells: &Cells) -> Cell {
    let xs: BTreeSet<i32> = cells.iter().map(|c| c.0).collect();
    let ys: BTreeSet<i32> = cells.iter().map(|c| c.1).collect();
    let mid = |vs: &BTreeSet<i32>| vs.iter().copied().nth((vs.len() / 2).saturating_sub(1)).unwrap_or(0);
    (mid(&xs), mid(&ys))
}

/// The rectangle round a set of cells, which must not be empty.
pub(crate) fn bounds(cells: &Cells) -> Rect {
    let xs = || cells.iter().map(|c| c.0);
    let ys = || cells.iter().map(|c| c.1);
    let none = "bounds of no cells";
    (xs().min().expect(none), ys().min().expect(none), xs().max().expect(none) + 1, ys().max().expect(none) + 1)
}

pub(crate) fn grow(r: Rect, n: i32) -> Rect {
    (r.0 - n, r.1 - n, r.2 + n, r.3 + n)
}

/// Values, sorted, as lists of consecutive ones. `wrap` joins the last run to the first round a
/// loop of that length.
pub fn runs(values: impl IntoIterator<Item = i32>, wrap: Option<i32>) -> Vec<Vec<i32>> {
    let mut vs: Vec<i32> = values.into_iter().collect();
    vs.sort();
    let mut out: Vec<Vec<i32>> = Vec::new();
    for v in vs {
        match out.last_mut() {
            Some(run) if v == run[run.len() - 1] + 1 => run.push(v),
            _ => out.push(vec![v]),
        }
    }
    if let Some(wrap) = wrap {
        if wrap != 0 && out.len() > 1 && out[0][0] == 0 && out[out.len() - 1].last() == Some(&(wrap - 1)) {
            let mut last = out.pop().expect("more than one run");
            last.append(&mut out[0]);
            out[0] = last;
        }
    }
    out
}

/// Do two sets have a cell in common?
pub(crate) fn meets(a: &Cells, b: &Cells) -> bool {
    !a.is_disjoint(b)
}

/// How many cells two sets have in common.
pub(crate) fn common(a: &Cells, b: &Cells) -> usize {
    a.intersection(b).count()
}

/// Do two rectangles overlap (neither being empty)?
fn overlap(a: Rect, b: Rect) -> bool {
    a.0.max(b.0) < a.2.min(b.2) && a.1.max(b.1) < a.3.min(b.3)
}

/// How a room is to be furnished: `Interior`'s keyword arguments, with the Python's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    /// The wall (0 south, 1 east, 2 north, 3 west) a room nothing arrives in upstairs gets a
    /// balcony on.
    pub balcony: Option<usize>,
    /// A tower's decks: "ring" (catwalks all round) or "ends" (decks at alternate ends).
    pub tower: &'static str,
    /// "pillars", "crates", "lanes" (crates' pillars without the crates) or "none".
    pub cover: &'static str,
    /// How much of it: 1 is as much as fits the rules.
    pub clutter: f64,
    /// A hall's first upper storey: "ring", "galleries" or "mezzanine".
    pub layout: Option<&'static str>,
    /// The layout (or none) is a person's choice: the first upper storey is laid out as it says
    /// and no other way, so it is not given the ring a vast hall or a basin may draw for itself.
    pub fixed: bool,
    /// A single-storey room: "notch" or "partition".
    pub shape: Option<&'static str>,
    /// The tag pillars are built with: "column" (grille and door panel), "pier" (the room's own
    /// wall strips) or "housing" (grey).
    pub pillar: &'static str,
    /// (x0, y0, x1, y1) h: a drop through the floor.
    pub hole: Option<Rect>,
    /// (x0, y0, x1, y1) h: nothing stands here (a drop lands, a grate lights).
    pub keep: Vec<Rect>,
    /// The highest storey that gets deck of the room's own (a tower's rings, a balcony): above
    /// it, only what a door or a pod needs. The level sets it where a climb would end in nothing.
    pub top: Option<i32>,
    /// A catwalk's depth along a wall, and the width of open ramps and bridges, h.
    pub band: i32,
    /// Platforms off the catwalks into the empty air (`lay_platforms`): one for each 100 cells
    /// of it, times this. 0 (the default, and every seed's level): none.
    pub platforms: f64,
    /// The chance each platform has a drop through it (`lay_pits`). 0 (the default): none.
    pub holes: f64,
    /// Angled catwalks (`lay_angled`): the chance the room has an island reached only by them, and
    /// each of its catwalk storeys a shortcut. 0 (the default): none.
    pub angled: f64,
    /// The room's own stream's name ("{seed}/{key}"), which the pits draw theirs from.
    pub stream: String,
    /// Which attempt at furnishing the room this is (the first is 0).
    pub attempt: usize,
    /// The side of the hole that stays open, if it is a cellar's hatch (the ladder's: a player
    /// climbs out there). None: the furnisher chooses (`hole_side`).
    pub hole_side: Option<usize>,
    /// The rescue: ladders and bridges where ramps can't join the room (`stitch`). Only once
    /// every attempt without it has failed (`Build::furnish`).
    pub rescue: bool,
    /// Parts re-rolled by hand (`level::PARTS`: "catwalks", "ramps", "cover"), each with the
    /// name of its own streams ("{seed}/{key}/catwalks2"; an attempt draws from
    /// "{name}/{attempt}"). None (every seed's level): everything from the room's stream.
    pub parts: Vec<(&'static str, String)>,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            balcony: None,
            tower: "ring",
            cover: "crates",
            clutter: 1.0,
            layout: None,
            fixed: false,
            shape: None,
            pillar: "column",
            hole: None,
            keep: Vec::new(),
            top: None,
            band: 2,
            platforms: 0.0,
            holes: 0.0,
            angled: 0.0,
            stream: String::new(),
            attempt: 0,
            hole_side: None,
            rescue: false,
            parts: Vec::new(),
        }
    }
}

/// One room's inside.
#[derive(Clone)]
pub struct Interior {
    pub s: Space,
    pub doors: Vec<Door>,
    pub pods: Vec<Space>,
    rng: PyRandom,
    /// Where each of its pods opens into it.
    pub access: Vec<(String, Access)>,
    /// A catwalk's depth along a wall, h (the Python's `BAND` and `WIDE_H`).
    pub band: i32,
    /// The room in half cells, and its width and depth.
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub w: i32,
    pub d: i32,
    pub room: Cells,
    /// Its storeys, the floor first, and those above the floor.
    pub levels: Vec<i32>,
    pub upper: Vec<i32>,
    /// By storey: pods' blocks, pods' roofs (a floor), and deck.
    pub shell: BTreeMap<i32, Cells>,
    pub roof: BTreeMap<i32, Cells>,
    pub deck: BTreeMap<i32, Cells>,
    /// Storeys whose only floor is a pod's roof.
    pub claim: BTreeSet<i32>,
    pub pool: Cells,
    pub bridge: Cells,
    pub ramps: Vec<RampIn>,
    pub cover: Vec<Cover>,
    pub extra_solids: Vec<Solid>,
    pub pads: Vec<Pad>,
    pub balcony: Option<usize>,
    pub tower: &'static str,
    pub piers: Vec<Pier>,
    pub rails: Vec<Rail>,
    pub cover_mode: &'static str,
    pub clutter: f64,
    pub layout: Option<&'static str>,
    /// The layout was chosen by hand (`Setup::fixed`).
    pub fixed: bool,
    pub shape: Option<&'static str>,
    pub pillar: &'static str,
    /// Bridges laid between galleries.
    pub bridges: usize,
    /// The shape it was given: "notch", "partition" or "".
    pub shaped: &'static str,
    /// Pieces of deck a gangway has been added to.
    extended: BTreeSet<Cells>,
    pub hole: Cells,
    /// The side the hole is entered from (0 south, 1 east, 2 north, 3 west).
    pub hole_open: Option<usize>,
    pub keep: Vec<Rect>,
    pub top: Option<i32>,
    /// `Setup::platforms`, and the platforms laid: (storey, rectangle h).
    pub platform_share: f64,
    pub platforms: Vec<(i32, Rect)>,
    /// `Setup::holes` and `Setup::stream`, and the drops laid through the platforms.
    pub hole_share: f64,
    pub stream: String,
    pub pits: Vec<Pit>,
    /// `Setup::angled`, the islands laid (storey, rectangle h), and the angled catwalks: the
    /// islands' spokes and the shortcuts.
    pub angled_share: f64,
    pub islands: Vec<(i32, Rect)>,
    pub angled: Vec<Angled>,
    /// `Setup::attempt`, and a towering tower's storey shapes as drawn: (storey, one of
    /// `decks::SHAPES`).
    pub attempt: usize,
    pub shapes: Vec<(i32, &'static str)>,
    /// `Setup::hole_side`.
    pub forced_side: Option<usize>,
    /// A flooded room's water height (`Space::flood`), and the rafts laid in it (rectangles h, on
    /// the floor; `flood`).
    pub flood: Option<i32>,
    pub rafts: Vec<Rect>,
    /// A shaped room's cuts (`Space::cuts`) in half cells, and their cells: not the room's
    /// (`shape`).
    pub cuts: Vec<Rect>,
    pub cut: Cells,
    /// The ladders up the pillars whose tops are clear (`cover::lay_ladders`).
    pub ladders: Vec<Ladder>,
    /// A sunken floor (`Space::sunk`) as laid, and its pit's cells: not the floor's (`sunken`).
    pub sunk: Option<Sunk>,
    pub sunk_cells: Cells,
    /// `Setup::rescue`; the ladders and bridges it put up (`stitch`; a bridge's cells are the
    /// deck's too, or the floor's bridge on the ground), the pieces of deck the ramps were let off
    /// (a ladder or a bridge joins them after), and pods' roofs it made a floor (a pod's door onto
    /// one), by storey.
    pub rescue: bool,
    pub chutes: Vec<Chute>,
    pub crossings: Vec<(i32, Cells)>,
    stranded: Vec<Cells>,
    pub landed: BTreeMap<i32, Cells>,
    /// `Setup::parts`.
    pub parts: Vec<(&'static str, String)>,
}

impl std::fmt::Debug for Interior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Interior({}: {} doors, {} pods, {} ramps, {} cover)", self.s.key, self.doors.len(), self.pods.len(), self.ramps.len(), self.cover.len())
    }
}

impl Interior {
    /// A room with nothing in it yet. `access` is every pod's way in (those of other rooms'
    /// pods are left out).
    pub fn new(s: &Space, doors: &[Door], pods: &[Space], rng: PyRandom, access: &[(String, Access)], how: Setup) -> Interior {
        let (x0, y0, x1, y1) = s.rect_h();
        let cuts: Vec<Rect> = s.cuts.iter().map(|c| (2 * c.0, 2 * c.1, 2 * c.2, 2 * c.3)).collect();
        let cut: Cells = cuts.iter().flat_map(|&c| rect_cells(c)).collect();
        let room: Cells = rect_cells((x0, y0, x1, y1)).into_iter().filter(|c| !cut.contains(c)).collect();
        let levels = s.storeys();
        let upper = levels[1..].to_vec();
        let empty = || levels.iter().map(|&z| (z, Cells::new())).collect::<BTreeMap<i32, Cells>>();
        let (mut shell, mut roof) = (empty(), empty());
        let mut keep = how.keep.clone();
        if let (Some(side), Some(r)) = (how.hole_side, how.hole) {
            // the floor a player climbs out onto, and steps in from (`pits::approach`)
            let n = pits::approach(r, side, levels[0]);
            keep.push((n.0 - 1, n.1 - 1, n.0 + 3, n.1 + 3));
        }
        for p in pods {
            let cells: Cells = rect_cells(shell_rect(p, s)).intersection(&room).copied().collect();
            let i = index(p.floor);
            if i + 1 < LEVELS.len() {
                if let Some(over) = roof.get_mut(&LEVELS[i + 1]) {
                    over.extend(cells.iter().copied());
                }
            }
            shell.get_mut(&p.floor).unwrap_or_else(|| panic!("{}: its pod {} is on no storey of it", s.key, p.key)).extend(cells);
        }
        // the storeys of bases that stand in it (`Space::blocks`): solid on their storey, as a
        // pod's shell is, and their roof a floor over them, unless the base goes on up there
        let mut based: BTreeMap<i32, Cells> = BTreeMap::new();
        for (key, r, z) in &s.blocks {
            let cells: Cells = rect_cells(crate::bases::block(*r, s.rect())).intersection(&room).copied().collect();
            shell.get_mut(z).unwrap_or_else(|| panic!("{}: its base storey {key} is on no storey of it", s.key)).extend(cells.iter().copied());
            based.entry(*z).or_default().extend(cells.iter().copied());
            let i = index(*z);
            if i + 1 < LEVELS.len() {
                if let Some(over) = roof.get_mut(&LEVELS[i + 1]) {
                    over.extend(cells);
                }
            }
        }
        for (z, cells) in &based {
            if let Some(over) = roof.get_mut(z) {
                over.retain(|c| !cells.contains(c));
            }
        }
        Interior {
            s: s.clone(),
            doors: doors.to_vec(),
            pods: pods.to_vec(),
            rng,
            access: access.iter().filter(|(k, _)| pods.iter().any(|p| p.key == *k)).cloned().collect(),
            band: how.band,
            x0,
            y0,
            x1,
            y1,
            w: x1 - x0,
            d: y1 - y0,
            room,
            deck: empty(),
            levels,
            upper,
            shell,
            roof,
            claim: BTreeSet::new(),
            pool: s.pool.map_or_else(Cells::new, |p| rect_cells((2 * p.0, 2 * p.1, 2 * p.2, 2 * p.3))),
            bridge: Cells::new(),
            ramps: Vec::new(),
            cover: Vec::new(),
            extra_solids: Vec::new(),
            pads: Vec::new(),
            balcony: how.balcony,
            tower: how.tower,
            piers: Vec::new(),
            rails: Vec::new(),
            cover_mode: how.cover,
            clutter: how.clutter,
            layout: how.layout,
            fixed: how.fixed,
            shape: how.shape,
            pillar: how.pillar,
            bridges: 0,
            shaped: "",
            extended: BTreeSet::new(),
            hole: how.hole.map_or_else(Cells::new, rect_cells),
            hole_open: None,
            keep,
            top: how.top,
            platform_share: how.platforms,
            platforms: Vec::new(),
            hole_share: how.holes,
            stream: how.stream,
            pits: Vec::new(),
            angled_share: how.angled,
            islands: Vec::new(),
            angled: Vec::new(),
            attempt: how.attempt,
            shapes: Vec::new(),
            forced_side: how.hole_side,
            flood: s.flood,
            rafts: Vec::new(),
            cuts,
            cut,
            ladders: Vec::new(),
            sunk: None,
            sunk_cells: s.sunk.map_or_else(Cells::new, |r| rect_cells((2 * r.0, 2 * r.1, 2 * r.2, 2 * r.3))),
            rescue: how.rescue,
            chutes: Vec::new(),
            crossings: Vec::new(),
            stranded: Vec::new(),
            landed: BTreeMap::new(),
            parts: how.parts,
        }
    }

    /// The name of a re-rolled part's own streams (`Setup::parts`), None if it is not re-rolled.
    pub(crate) fn part_stream(&self, part: &str) -> Option<&str> {
        self.parts.iter().find(|(p, _)| *p == part).map(|(_, name)| name.as_str())
    }

    /// One part of the inside (`level::PARTS`) laid by `lay`. Re-rolled, it draws from its own
    /// stream ("{name}/{attempt}") in place of the room's, and the room's stream is left where
    /// laying it from there would have left it (it is laid so on a copy, which is thrown away),
    /// so the parts after it draw what they would have. Not re-rolled: just `lay`.
    fn part(&mut self, part: &str, lay: impl Fn(&mut Interior) -> Result<(), String>) -> Result<(), String> {
        let Some(name) = self.part_stream(part).map(|n| format!("{n}/{}", self.attempt)) else {
            return lay(self);
        };
        let mut copy = self.clone();
        copy.parts.clear();
        let _ = lay(&mut copy);
        self.rng = PyRandom::named(&name);
        let made = lay(self);
        self.rng = copy.rng;
        made
    }

    /// Which band of the room storey `z` is in: storeys joined by ramps inside the room are one
    /// band. A towering hall's high storeys (`Space::high`) are each a band of their own, reached
    /// from outside and never ramped down; their band is their floor, the room's own `i32::MIN`.
    pub fn band_of(&self, z: i32) -> i32 {
        if self.s.high.contains(&z) {
            z
        } else {
            i32::MIN
        }
    }

    /// Where one of its pods opens into it.
    pub(crate) fn access_of(&self, pod: &str) -> Option<&Access> {
        self.access.iter().find(|(k, _)| k == pod).map(|(_, a)| a)
    }

    /// Is a half cell inside the room's walls? (Not in a shaped room's cut.)
    pub(crate) fn inside(&self, c: Cell) -> bool {
        self.x0 <= c.0 && c.0 < self.x1 && self.y0 <= c.1 && c.1 < self.y1 && !self.cut.contains(&c)
    }

    /// Is a rectangle of half cells (not an empty one) all inside the room's walls?
    pub(crate) fn holds(&self, r: Rect) -> bool {
        self.x0 <= r.0 && r.2 <= self.x1 && self.y0 <= r.1 && r.3 <= self.y1 && !self.cuts.iter().any(|&c| overlap(c, r))
    }

    /// The floor just inside each door on storey `z`, `depth` deep and a half cell wider each side.
    pub(crate) fn apron(&self, z: i32, depth: i32) -> Cells {
        let mut out = Cells::new();
        for d in self.doors.iter().filter(|d| d.level == z) {
            out.extend(door_pad(&self.s, d, depth, 1));
        }
        out
    }

    /// All of it: the pool, the decks, the ramps, the platforms, the cover and the rails. Err says why the room
    /// can't be furnished (the Python's `Unfurnishable`).
    pub fn build(mut self) -> Result<Interior, String> {
        let targets = if self.rescue { self.targets() } else { Vec::new() };
        for p in &self.pods {
            let Some(acc) = self.access.iter().find(|(k, _)| *k == p.key).map(|(_, a)| a) else { continue };
            if acc.island && p.x1 - p.x0 >= 2 && p.y1 - p.y0 >= 2 {
                let (cx, cy) = ((p.x0 + p.x1).div_euclid(2), (p.y0 + p.y1).div_euclid(2));
                // (in a flooded room, from the water up)
                let pier = (2 * cx - 2, 2 * cy - 2, 2 * cx + 2, 2 * cy + 2, self.flood.unwrap_or(self.s.floor), shell_z(p, &self.s).0);
                // (the rescue's: none through where a door is stepped through; the pod floats)
                let cells = rect_cells((pier.0, pier.1, pier.2, pier.3));
                if targets.iter().any(|(pad, z)| pier.4 <= *z && *z < pier.5 && !pad.is_disjoint(&cells)) {
                    continue;
                }
                self.piers.push(pier);
            }
        }
        for d in &self.doors {
            if d.level == self.levels[0] {
                self.pads.push(Pad { cells: door_pad(&self.s, d, self.band, 0), level: d.level, door: Some(d.clone()) });
            }
        }
        self.lay_pool()?;
        self.part("catwalks", |it| {
            it.lay_decks();
            it.shape_decks();
            Ok(())
        })?;
        self.lay_sunk();
        if self.rescue {
            self.land_roofs();
        }
        self.part("ramps", |it| it.place_ramps(40))?;
        if self.rescue {
            self.stitch()?;
        }
        if !self.connected() {
            return Err(format!("{}: doors don't all join", self.s.key));
        }
        self.clear_under_ramps();
        self.lay_flood()?;
        if self.flood.is_some() && !self.connected() {
            return Err(format!("{}: doors don't all join over the water", self.s.key));
        }
        self.lay_platforms();
        self.lay_angled();
        self.lay_tanks();
        self.lay_pits();
        self.part("cover", |it| {
            it.lay_cover();
            Ok(())
        })?;
        self.lay_ladders();
        let rails = self.lay_rails();
        self.rails = self.cut_rails(rails);
        Ok(self)
    }
}
