//! Corridors between scattered rooms, routed after the rooms are placed. A port of
//! `autolevel/route.py` (`Piece`, `Route`, `Grid`); the network over them is in `network`.
//!
//! A corridor is one cell wide and a storey high: 118 by 236, taller than wide, the section of
//! Complex's corridors and doorways. It is the sweep of a "head" block `WIDE` cells square that
//! A* moves over the cell grid, one storey at a time, with enclosed 30 degree ramps (four cells
//! for a storey) between storeys. Two rules keep the plan free of accidental openings (air that
//! touches air is open): every head position stays a full cell clear of every other room and
//! corridor, except the first and last, which sit flush against their room's wall. That contact
//! is the door.
//!
//! Rooms one or two cells apart on the same storey are joined by a stub instead (a door frame
//! through the wall between them). A vent is the same idea one cell square and half a storey
//! high: a crawl duct between two rooms' floors, sloping where they are on different storeys.
//!
//! Coordinates: x and y in cells, heights in units.

use crate::cells::{Cell, Cells, Rect};
use crate::py;
use crate::scatter::rect_gap;
use crate::space::{door_pad, find, shell_rect, side_index, Door, Space, DIRS, SIDES};
use crate::storeys::{index, storey, LEVELS};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};

/// Cells a ramp runs to climb one storey.
pub const RAMP: i32 = 4;
/// The most moves a corridor makes in a line before it must jog.
pub const STRAIGHT: i32 = 8;
/// And the fewest: a jog only as wide as the corridor lets a diagonal through.
pub const JOG: i32 = 3;
/// Corridor and door width, cells.
pub const WIDE: i32 = 1;
/// Head positions A* looks at before a link is given up.
pub const SEARCH: usize = 60000;
/// Cells a switchback steps sideways between its flights (a cell of wall between them).
pub const HAIRPIN: i32 = WIDE + 1;
/// Clear cells kept round the rooms at the grid's edge.
pub const MARGIN: i32 = 8;

/// A point on a centre line: x and y in cells (whole or half), z in units.
pub type Point = (f64, f64, i32);
/// Where the head block is: (x, y, storey).
type Head = (i32, i32, i32);
/// A corridor search's state: (x, y, storey, direction, moves made in a line; -1 straight off a ramp).
type State = (i32, i32, i32, i32, i32);
/// A vent search's state: (x, y, storey, direction).
type Crawl = (i32, i32, i32, i32);
/// Per storey: window sums over (every owned cell, every owned cell but some rooms').
type Sums = Vec<(Vec<i32>, Vec<i32>)>;

/// One room of a corridor: flat cells on one or two storeys, and at most one ramp.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub key: String,
    /// corridor, ramp, stub (a stub becomes part of a room) or vent.
    pub kind: &'static str,
    pub floor: i32,
    /// Its flat cells by storey, the storeys in the order the route came to them.
    pub cells: Vec<(i32, Cells)>,
    /// The ramp's centre line, foot then top.
    pub ramp: Option<(Point, Point)>,
    pub ramp_cells: Option<Rect>,
    /// A vent's sloped runs: ((foot, top), cells).
    pub ducts: Vec<((Point, Point), Rect)>,
}

impl Piece {
    /// Its flat cells on one storey.
    pub fn cells_at(&self, z: i32) -> Option<&Cells> {
        self.cells.iter().find(|(l, _)| *l == z).map(|(_, cs)| cs)
    }
}

/// A corridor, stub or vent between two doors.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub a: Door,
    pub b: Door,
    pub pieces: Vec<Piece>,
    /// The centre line, for walking it.
    pub path: Vec<Point>,
    pub cost: f64,
}

impl Route {
    /// The key of its first piece, which no other route ever has.
    pub fn key(&self) -> &str {
        &self.pieces[0].key
    }

    pub fn length(&self) -> f64 {
        self.path.windows(2).map(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs()).sum()
    }

    pub fn ramps(&self) -> usize {
        self.pieces.iter().filter(|p| p.ramp.is_some()).count()
    }

    /// Do two of its flights run back alongside each other?
    pub fn switchback(&self) -> bool {
        let fl: Vec<(Point, Point)> = self.pieces.iter().filter_map(|p| p.ramp).collect();
        fl.windows(2).any(|w| {
            let ((a0, a1), (b0, b1)) = (w[0], w[1]);
            (a1.0 - a0.0) * (b1.0 - b0.0) + (a1.1 - a0.1) * (b1.1 - b0.1) < 0.0
        })
    }
}

/// What a corridor may cost and where it may go (`Grid.route`'s keyword arguments). Costs are
/// in cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Terms {
    /// The dearest corridor worth having.
    pub max_cost: f64,
    /// What a turn costs, over the cell it moves.
    pub turn: f64,
    /// What a ramp costs, over its length.
    pub ramp_cost: f64,
    /// What a door on an upper storey of a tall room costs, a storey (each is a catwalk to build).
    pub upper: f64,
    /// The most moves in a line (and none shorter than `JOG`). 0: no such rule.
    pub straight: i32,
    /// The storeys the first room may take the door on (None: any it offers).
    pub la: Option<Vec<i32>>,
    /// The storeys the second room may take the door on.
    pub lb: Option<Vec<i32>>,
    /// The one doorway the first room may take: the wall (E, N, W or S) and the cell along it
    /// of the door's low end (None: any). New here (a person's `join`): the Python has none.
    pub da: Option<(char, i32)>,
    /// The second room's.
    pub db: Option<(char, i32)>,
    /// Head positions looked at before the link is given up.
    pub search: usize,
    /// The search knows what the storeys still to climb will cost. It finds a way between rooms
    /// several storeys apart far sooner, but among ways that cost the same it may pick another,
    /// so a level's first corridors are routed without it.
    pub storeyed: bool,
}

impl Default for Terms {
    fn default() -> Self {
        Terms {
            max_cost: 70.0,
            turn: 2.0,
            ramp_cost: 8.0,
            upper: 3.0,
            straight: STRAIGHT,
            la: None,
            lb: None,
            da: None,
            db: None,
            search: SEARCH,
            storeyed: false,
        }
    }
}

impl Terms {
    /// The usual terms, with this allowance.
    pub fn within(max_cost: f64) -> Terms {
        Terms { max_cost, ..Terms::default() }
    }
}

/// What a vent may cost and where it may open (`Grid.vent`'s keyword arguments).
#[derive(Clone, Debug, PartialEq)]
pub struct VentTerms {
    pub max_cost: f64,
    pub turn: f64,
    pub ramp_cost: f64,
    /// The storeys each end may open on (None: any the room offers).
    pub la: Option<Vec<i32>>,
    pub lb: Option<Vec<i32>>,
    /// The one opening each end may take (as [`Terms::da`]).
    pub da: Option<(char, i32)>,
    pub db: Option<(char, i32)>,
}

impl Default for VentTerms {
    fn default() -> Self {
        VentTerms { max_cost: 24.0, turn: 0.5, ramp_cost: 4.0, la: None, lb: None, da: None, db: None }
    }
}

/// A cost in a search's heap: floats compared exactly (none is ever NaN).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cost(pub(crate) f64);

impl Eq for Cost {}

impl PartialOrd for Cost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cost {
    fn cmp(&self, other: &Self) -> Ordering {
        py::cmp_f64(self.0, other.0)
    }
}

/// Head positions flush against a room's wall, each with the door it would make, in the order
/// they were found (a Python dict).
#[derive(Default)]
struct Ends {
    doors: Vec<(Head, Door)>,
    at: HashMap<Head, usize>,
}

impl Ends {
    fn insert(&mut self, head: Head, door: Door) {
        match self.at.get(&head) {
            Some(&n) => self.doors[n].1 = door,
            None => {
                self.at.insert(head, self.doors.len());
                self.doors.push((head, door));
            }
        }
    }

    fn get(&self, head: Head) -> Option<&Door> {
        self.at.get(&head).map(|&n| &self.doors[n].1)
    }

    fn is_empty(&self) -> bool {
        self.doors.is_empty()
    }

    /// Does a head moving in direction `d` arrive here, face on to the door?
    fn arrives(&self, x: i32, y: i32, z: i32, d: i32) -> bool {
        self.get((x, y, z)).is_some_and(|g| side_index(g.side) as i32 == (d + 2) % 4)
    }
}

/// Does a room take up any of the storey whose floor is `z` (its slabs too)?
fn occupies(sp: &Space, z: i32) -> bool {
    let (lo, hi) = storey(z);
    sp.floor - 39 < hi && sp.top + 39 > lo
}

/// Cells of a ramp leaving the head block at (x, y) in direction d.
fn ramp_rect(x: i32, y: i32, d: i32) -> Rect {
    let (dx, dy) = DIRS[d as usize];
    let x0 = match dx {
        1 => x + WIDE,
        -1 => x - RAMP,
        _ => x,
    };
    let y0 = match dy {
        1 => y + WIDE,
        -1 => y - RAMP,
        _ => y,
    };
    (x0, y0, x0 + if dx != 0 { RAMP } else { WIDE }, y0 + if dy != 0 { RAMP } else { WIDE })
}

/// The head block's cells at (x, y).
fn block(x: i32, y: i32) -> Cells {
    (0..WIDE).flat_map(|i| (0..WIDE).map(move |j| (x + i, y + j))).collect()
}

/// The four cells a vent's sloped run takes, straight on from (x, y).
fn run_rect(x: i32, y: i32, d: i32) -> Rect {
    let (dx, dy) = DIRS[d as usize];
    let (xa, xb) = (x + dx, x + dx * RAMP);
    let (ya, yb) = (y + dy, y + dy * RAMP);
    (xa.min(xb), ya.min(yb), xa.max(xb) + 1, ya.max(yb) + 1)
}

/// The cells on storey `z` of a piece being put together, added if it has none there yet.
fn storey_cells(cells: &mut Vec<(i32, Cells)>, z: i32) -> &mut Cells {
    let n = cells.iter().position(|(l, _)| *l == z).unwrap_or_else(|| {
        cells.push((z, Cells::new()));
        cells.len() - 1
    });
    &mut cells[n].1
}

/// A pod's block in its hall, in the hall's cells.
fn shell_cells(pod: &Space, host: &Space) -> Rect {
    let (x0, y0, x1, y1) = shell_rect(pod, host);
    (x0.div_euclid(2).max(host.x0), y0.div_euclid(2).max(host.y0), (-(-x1).div_euclid(2)).min(host.x1), (-(-y1).div_euclid(2)).min(host.y1))
}

/// Who owns each cell on each storey. Rooms first, then corridors as they are routed.
#[derive(Clone, Debug)]
pub struct Grid {
    /// The rooms, in the order they were scattered. `drop` takes rooms out.
    pub sp: Vec<Space>,
    /// A catwalk's depth along a wall, half cells (the level's `Options::band`).
    pub band: i32,
    /// The grid's low corner, and its size, in cells.
    pub ox: i32,
    pub oy: i32,
    pub w: i32,
    pub h: i32,
    /// The number each room and piece owns its cells by (from 1; never given twice).
    ids: BTreeMap<String, i32>,
    /// By storey: the owner of each cell, 0 for none.
    owner: Vec<Vec<i32>>,
    /// Every room's doors in the order they were made, the rooms in `sp`'s order.
    pub doors: Vec<(String, Vec<Door>)>,
    /// Corridors, stubs and vents, in the order committed.
    pub routes: Vec<Route>,
    /// Routes ever committed, and the vents among them: the numbers in their keys (not the
    /// length of `routes`: a route taken out again must not free its key).
    pub made: usize,
    pub vents_made: usize,
}

impl Grid {
    pub fn new(sp: Vec<Space>, band: i32) -> Grid {
        Grid::with_margin(sp, band, MARGIN)
    }

    pub fn with_margin(sp: Vec<Space>, band: i32, margin: i32) -> Grid {
        let ox = sp.iter().map(|s| s.x0).min().expect("a level has rooms") - margin;
        let oy = sp.iter().map(|s| s.y0).min().expect("a level has rooms") - margin;
        let w = sp.iter().map(|s| s.x1).max().expect("a level has rooms") + margin - ox;
        let h = sp.iter().map(|s| s.y1).max().expect("a level has rooms") + margin - oy;
        let owner = vec![vec![0; (w * h) as usize]; LEVELS.len()];
        let doors = sp.iter().map(|s| (s.key.clone(), Vec::new())).collect();
        let mut g = Grid { sp, band, ox, oy, w, h, ids: BTreeMap::new(), owner, doors, routes: Vec::new(), made: 0, vents_made: 0 };
        // pods overwrite their hosts, and a base's storeys the hall they cross (new in the Rust:
        // no seed's level has a base unless asked for, so the order is as it was)
        let all = &g.sp;
        let rooms = |base: bool| (0..all.len()).filter(move |&n| all[n].host.is_empty() && all[n].base.is_some() == base);
        let order: Vec<usize> = rooms(false).chain(rooms(true)).chain((0..all.len()).filter(|&n| !all[n].host.is_empty())).collect();
        for n in order {
            let s = g.sp[n].clone();
            let i = g.id(&s.key);
            for z in LEVELS {
                let here = if s.host.is_empty() { occupies(&s, z) } else { z == s.floor };
                if here {
                    g.mark(s.rect(), z, i);
                }
            }
            if let Some(p) = s.pool {
                // the pit under the water
                if let Some(&below) = LEVELS.iter().rfind(|&&z| z < s.floor) {
                    g.mark((p.0, p.1, p.2, p.3), below, i);
                }
            }
            for (k, &cut) in s.cuts.iter().enumerate() {
                // (cut from a shaped room: solid, nobody's, no door onto it)
                let c = g.id(&format!("{}/cut{k}", s.key));
                for z in LEVELS.iter().copied().filter(|&z| occupies(&s, z)) {
                    g.mark(cut, z, c);
                }
            }
            if let Some(r) = s.sunk {
                // the pit under a sunken floor
                if let Some(&below) = LEVELS.iter().rfind(|&&z| z < s.floor) {
                    g.mark(r, below, i);
                }
            }
            if s.flood.is_some() {
                // (a flooded room's, all of it)
                if let Some(&below) = LEVELS.iter().rfind(|&&z| z < s.floor) {
                    g.mark(s.rect(), below, i);
                }
            }
            if !s.host.is_empty() {
                // its shell in the hall: no doors there
                let shell = shell_cells(&s, find(&g.sp, &s.host));
                g.mark(shell, s.floor, i);
            }
            if let Some(b) = &s.base {
                // a base storey's block in its hall: no doors there either
                let hall = find(&g.sp, &b.hall).clone();
                if hall.storeys().contains(&s.floor) {
                    let (x0, y0, x1, y1) = crate::bases::block(s.rect(), hall.rect());
                    g.mark((x0.div_euclid(2), y0.div_euclid(2), -(-x1).div_euclid(2), -(-y1).div_euclid(2)), s.floor, i);
                }
            }
        }
        g
    }

    /// The room with this key.
    pub fn space(&self, key: &str) -> &Space {
        find(&self.sp, key)
    }

    /// A room's doors, in the order they were made.
    pub fn doors(&self, key: &str) -> &[Door] {
        self.doors.iter().find(|(k, _)| k == key).map(|(_, d)| d.as_slice()).unwrap_or_else(|| panic!("no room {key}"))
    }

    fn doors_mut(&mut self, key: &str) -> &mut Vec<Door> {
        self.doors.iter_mut().find(|(k, _)| k == key).map(|(_, d)| d).unwrap_or_else(|| panic!("no room {key}"))
    }

    /// The number a room or piece owns its cells by.
    pub fn id_of(&self, key: &str) -> Option<i32> {
        self.ids.get(key).copied()
    }

    fn id(&mut self, key: &str) -> i32 {
        let next = self.ids.len() as i32 + 1;
        *self.ids.entry(key.to_string()).or_insert(next)
    }

    fn mark(&mut self, rect: Rect, z: i32, i: i32) {
        let (x0, y0, x1, y1) = rect;
        let (i0, j0) = (x0 - self.ox, y0 - self.oy);
        let (i1, j1) = ((x1 - self.ox).min(self.w), (y1 - self.oy).min(self.h));
        assert!(i0 >= 0 && j0 >= 0, "a rectangle to mark starts off the grid");
        let owner = &mut self.owner[index(z)];
        for a in i0..i1 {
            for b in j0..j1 {
                owner[(a * self.h + b) as usize] = i;
            }
        }
    }

    /// The owner of a cell (0 for none), or -1 off the grid.
    pub fn owner_at(&self, x: i32, y: i32, z: i32) -> i32 {
        let (i, j) = (x - self.ox, y - self.oy);
        if !(0 <= i && i < self.w && 0 <= j && j < self.h) {
            return -1;
        }
        self.owner[index(z)][(i * self.h + j) as usize]
    }

    // ------------------------------------------------------------ window sums

    /// Running sums of a storey's cells that `marked` picks, one row and column bigger than the grid.
    fn integral(&self, owner: &[i32], marked: impl Fn(i32) -> bool) -> Vec<i32> {
        let (w, h) = (self.w as usize, self.h as usize);
        let mut s = vec![0; (w + 1) * (h + 1)];
        for i in 0..w {
            let mut row = 0;
            for j in 0..h {
                row += marked(owner[i * h + j]) as i32;
                s[(i + 1) * (h + 1) + j + 1] = s[i * (h + 1) + j + 1] + row;
            }
        }
        s
    }

    /// Marked cells in [x0, x1) x [y0, y1); anything off the grid counts as marked.
    fn count(&self, s: &[i32], x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
        let (i0, j0, i1, j1) = (x0 - self.ox, y0 - self.oy, x1 - self.ox, y1 - self.oy);
        if i0 < 0 || j0 < 0 || i1 > self.w || j1 > self.h {
            return 1;
        }
        let at = |i: i32, j: i32| s[(i * (self.h + 1) + j) as usize];
        at(i1, j1) - at(i0, j1) - at(i1, j0) + at(i0, j0)
    }

    /// Per storey: (every owned cell, every owned cell but the rooms in `keep`).
    fn sums(&self, keep: &[&str]) -> Sums {
        let ids: Vec<i32> = keep.iter().map(|k| self.ids[*k]).collect();
        self.owner.iter().map(|o| (self.integral(o, |v| v != 0), self.integral(o, |v| v != 0 && !ids.contains(&v)))).collect()
    }

    // ------------------------------------------------------------ doors

    /// A door there has the room's own floor behind it (not a pod), and isn't hard by another.
    fn door_ok(&self, s: &Space, side: char, a: i32, z: i32, width: i32) -> bool {
        let i = self.ids[&s.key];
        let (dx, dy) = DIRS[side_index(side)];
        let ins = if dx != 0 {
            (if dx > 0 { s.x1 - 3 } else { s.x0 }, a, if dx > 0 { s.x1 } else { s.x0 + 3 }, a + width)
        } else {
            (a, if dy > 0 { s.y1 - 3 } else { s.y0 }, a + width, if dy > 0 { s.y1 } else { s.y0 + 3 })
        };
        let (x0, y0, x1, y1) = (ins.0.max(s.x0), ins.1.max(s.y0), ins.2.min(s.x1), ins.3.min(s.y1));
        if (x0..x1).any(|x| (y0..y1).any(|y| self.owner_at(x, y, z) != i)) {
            return false;
        }
        if s.decks.is_some() || !s.clear.is_empty() {
            // a room whose inside is already fixed
            let pad = door_pad(s, &Door { room: String::new(), side, a, level: z, width, to: String::new() }, self.band, 0);
            if z == s.floor {
                if !pad.is_disjoint(&s.clear) {
                    return false;
                }
            } else if let Some(decks) = &s.decks {
                if !decks.get(&z).is_some_and(|deck| pad.is_subset(deck)) {
                    return false;
                }
            }
        }
        !self.doors(&s.key).iter().any(|d| d.side == side && d.level == z && (d.a - a).abs() < width + 2)
    }

    /// Head positions flush against one of the room's walls, where a door may go (on the
    /// storeys in `levels` only, if given).
    fn terminals(&self, s: &Space, sums: &Sums, levels: Option<&[i32]>, only: Option<(char, i32)>) -> Ends {
        let mut out = Ends::default();
        for z in s.door_levels() {
            if levels.is_some_and(|l| !l.contains(&z)) {
                continue;
            }
            let (everything, others) = &sums[index(z)];
            for (&side, &(dx, dy)) in SIDES.iter().zip(DIRS.iter()) {
                let along = if dx != 0 { s.y0..s.y1 - WIDE + 1 } else { s.x0..s.x1 - WIDE + 1 };
                for a in along {
                    if only.is_some_and(|d| d != (side, a)) {
                        continue;
                    }
                    let (hx, hy) = if dx != 0 { (if dx > 0 { s.x1 } else { s.x0 - WIDE }, a) } else { (a, if dy > 0 { s.y1 } else { s.y0 - WIDE }) };
                    if self.count(everything, hx, hy, hx + WIDE, hy + WIDE) != 0 {
                        continue;
                    }
                    if self.count(others, hx - 1, hy - 1, hx + WIDE + 1, hy + WIDE + 1) != 0 {
                        continue;
                    }
                    if self.door_ok(s, side, a, z, WIDE) {
                        out.insert((hx, hy, z), Door { room: s.key.clone(), side, a, level: z, width: WIDE, to: String::new() });
                    }
                }
            }
        }
        out
    }

    /// A stub through the wall between two rooms one or two cells apart on a storey both have
    /// a floor on, or None. `la`, `lb`: the storeys each may take the door on.
    pub fn direct(&self, a: &Space, b: &Space, la: Option<&[i32]>, lb: Option<&[i32]>) -> Option<Route> {
        self.direct_at(a, b, la, lb, None, None)
    }

    /// [`Grid::direct`] through the doorways `da` and `db` alone where they are given.
    pub fn direct_at(&self, a: &Space, b: &Space, la: Option<&[i32]>, lb: Option<&[i32]>, da: Option<(char, i32)>, db: Option<(char, i32)>) -> Option<Route> {
        struct Stub<'a> {
            off: f64,
            rect: Rect,
            z: i32,
            first: &'a Space,
            second: &'a Space,
            sides: (char, char),
            at: i32,
            gap: i32,
        }
        let sums = self.sums(&[&a.key, &b.key]);
        let mut best: Option<Stub> = None;
        let b_levels = b.door_levels();
        for z in a.door_levels() {
            if !b_levels.contains(&z) || la.is_some_and(|l| !l.contains(&z)) || lb.is_some_and(|l| !l.contains(&z)) {
                continue;
            }
            let (everything, others) = &sums[index(z)];
            for axis in 0..2 {
                let (lo_a, hi_a) = if axis == 0 { (a.x0, a.x1) } else { (a.y0, a.y1) };
                let (lo_b, hi_b) = if axis == 0 { (b.x0, b.x1) } else { (b.y0, b.y1) };
                let gap = (lo_b - hi_a).max(lo_a - hi_b);
                if gap != 1 && gap != 2 {
                    continue;
                }
                let (first, second) = if lo_b >= hi_a { (a, b) } else { (b, a) }; // first is on the low side
                let g0 = if axis == 0 { first.x1 } else { first.y1 };
                // overlap along the wall
                let o0 = if axis == 0 { a.y0.max(b.y0) } else { a.x0.max(b.x0) };
                let o1 = if axis == 0 { a.y1.min(b.y1) } else { a.x1.min(b.x1) };
                for at in o0..o1 - WIDE + 1 {
                    let rect = if axis == 0 { (g0, at, g0 + gap, at + WIDE) } else { (at, g0, at + WIDE, g0 + gap) };
                    if self.count(everything, rect.0, rect.1, rect.2, rect.3) != 0 {
                        continue;
                    }
                    if self.count(others, rect.0 - 1, rect.1 - 1, rect.2 + 1, rect.3 + 1) != 0 {
                        continue;
                    }
                    let sides = if axis == 0 { ('E', 'W') } else { ('N', 'S') };
                    let (side_a, side_b) = if first.key == a.key { (sides.0, sides.1) } else { (sides.1, sides.0) };
                    if da.is_some_and(|d| d != (side_a, at)) || db.is_some_and(|d| d != (side_b, at)) {
                        continue;
                    }
                    if !(self.door_ok(first, sides.0, at, z, WIDE) && self.door_ok(second, sides.1, at, z, WIDE)) {
                        continue;
                    }
                    let up = a.storeys().iter().position(|&l| l == z).expect("a door storey is a storey");
                    let off = ((at as f64 + WIDE as f64 / 2.0) - (o0 + o1) as f64 / 2.0).abs() + 3.0 * up as f64;
                    if best.as_ref().is_none_or(|b| off < b.off) {
                        best = Some(Stub { off, rect, z, first, second, sides, at, gap });
                    }
                }
            }
        }
        let Stub { rect, z, first, second, sides, at, gap, .. } = best?;
        let key = format!("c{:02}", self.made + 1);
        let cells: Cells = (rect.0..rect.2).flat_map(|x| (rect.1..rect.3).map(move |y| (x, y))).collect();
        let pc = Piece { key, kind: "stub", floor: z, cells: vec![(z, cells)], ramp: None, ramp_cells: None, ducts: Vec::new() };
        let mid = ((rect.0 + rect.2) as f64 / 2.0, (rect.1 + rect.3) as f64 / 2.0, z);
        let d1 = Door { room: first.key.clone(), side: sides.0, a: at, level: z, width: WIDE, to: second.key.clone() };
        let d2 = Door { room: second.key.clone(), side: sides.1, a: at, level: z, width: WIDE, to: first.key.clone() };
        Some(Route { a: d1, b: d2, pieces: vec![pc], path: vec![mid], cost: gap as f64 })
    }

    // ------------------------------------------------------------ vents

    /// A crawl duct from `a` to `b`: one cell wide, opening at floor level in each, a cell
    /// clear of everything else. It may slope between storeys (four cells a storey, as the
    /// ramps do). A Route of one piece, kind "vent", or None.
    pub fn vent(&self, a: &Space, b: &Space, t: &VentTerms) -> Option<Route> {
        let sums = [self.sums(&[&a.key]), self.sums(&[&b.key])];
        let everything: Vec<&[i32]> = sums[0].iter().map(|s| s.0.as_slice()).collect();
        let mut ends: Vec<Ends> = Vec::new();
        for (n, (s, only, door)) in [(a, &t.la, t.da), (b, &t.lb, t.db)].into_iter().enumerate() {
            let mut found = Ends::default();
            for z in s.vent_levels() {
                if only.as_ref().is_some_and(|l| !l.contains(&z)) {
                    continue;
                }
                let others = &sums[n][index(z)].1;
                for (&side, &(dx, dy)) in SIDES.iter().zip(DIRS.iter()) {
                    let along = if dx != 0 { s.y0..s.y1 } else { s.x0..s.x1 };
                    for at in along {
                        if door.is_some_and(|d| d != (side, at)) {
                            continue;
                        }
                        let (hx, hy) = if dx != 0 { (if dx > 0 { s.x1 } else { s.x0 - 1 }, at) } else { (at, if dy > 0 { s.y1 } else { s.y0 - 1 }) };
                        if self.count(everything[index(z)], hx, hy, hx + 1, hy + 1) != 0 || self.count(others, hx - 1, hy - 1, hx + 2, hy + 2) != 0 {
                            continue;
                        }
                        if self.door_ok(s, side, at, z, 1) {
                            found.insert((hx, hy, z), Door { room: s.key.clone(), side, a: at, level: z, width: 1, to: String::new() });
                        }
                    }
                }
            }
            ends.push(found);
        }
        let goals = ends.pop().expect("two ends");
        let starts = ends.pop().expect("two ends");
        if starts.is_empty() || goals.is_empty() {
            return None;
        }
        let free = |x: i32, y: i32, z: i32| self.count(everything[index(z)], x - 1, y - 1, x + 2, y + 2) == 0;

        let mut heap: BinaryHeap<Reverse<(Cost, Crawl)>> = BinaryHeap::new();
        let mut cost: HashMap<Crawl, (f64, Option<Crawl>)> = HashMap::new();
        for ((x, y, z), door) in &starts.doors {
            let st = (*x, *y, *z, side_index(door.side) as i32);
            cost.insert(st, (0.0, None));
            heap.push(Reverse((Cost(0.0), st)));
        }
        let mut done: Option<Crawl> = None;
        while let Some(Reverse((Cost(g), st))) = heap.pop() {
            if g > cost.get(&st).map_or(1e9, |c| c.0) {
                continue;
            }
            let (x, y, z, d) = st;
            if goals.arrives(x, y, z, d) {
                done = Some(st);
                break;
            }
            if g >= t.max_cost {
                continue;
            }
            let mut moves: Vec<(Crawl, f64)> = Vec::new();
            for (nd, &(mx, my)) in DIRS.iter().enumerate() {
                let nd = nd as i32;
                if nd != (d + 2) % 4 && (free(x + mx, y + my, z) || goals.arrives(x + mx, y + my, z, nd)) {
                    moves.push(((x + mx, y + my, z, nd), 1.0 + if nd != d { t.turn } else { 0.0 }));
                }
            }
            let (dx, dy) = DIRS[d as usize];
            let zi = index(z) as i32;
            for dz in [1, -1] {
                if 0 <= zi + dz && zi + dz < LEVELS.len() as i32 {
                    let z2 = LEVELS[(zi + dz) as usize];
                    let (rx0, ry0, rx1, ry1) = run_rect(x, y, d);
                    let (sx, sy) = if dx != 0 { (0, 1) } else { (1, 0) };
                    let (nx, ny) = (x + dx * (RAMP + 1), y + dy * (RAMP + 1));
                    if [z, z2].iter().all(|&zz| self.count(everything[index(zz)], rx0 - sx, ry0 - sy, rx1 + sx, ry1 + sy) == 0)
                        && (free(nx, ny, z2) || goals.arrives(nx, ny, z2, d))
                    {
                        moves.push(((nx, ny, z2, d), (RAMP + 1) as f64 + t.ramp_cost));
                    }
                }
            }
            for (nst, c) in moves {
                if g + c < cost.get(&nst).map_or(1e9, |c| c.0) {
                    cost.insert(nst, (g + c, Some(st)));
                    heap.push(Reverse((Cost(g + c), nst)));
                }
            }
        }
        let done = done?;
        let total = cost[&done].0;
        if total > t.max_cost {
            return None;
        }
        let mut states: Vec<Crawl> = Vec::new();
        let mut at = Some(done);
        while let Some(st) = at {
            states.push(st);
            at = cost[&st].1;
        }
        states.reverse();
        let key = format!("v{:02}", self.vents_made + 1);
        let floor = states.iter().map(|q| q.2).min().expect("a vent has cells");
        let mut pc = Piece { key: key.clone(), kind: "vent", floor, cells: Vec::new(), ramp: None, ramp_cells: None, ducts: Vec::new() };
        let mut path: Vec<Point> = Vec::new();
        let mut last: Option<Crawl> = None;
        for &n in &states {
            let (x, y, z, d) = n;
            if let Some(q) = last.filter(|q| q.2 != z) {
                // a sloped run: from the last cell's face to this one's
                let (dx, dy) = DIRS[d as usize];
                let a = (q.0 as f64 + 0.5 + dx as f64 / 2.0, q.1 as f64 + 0.5 + dy as f64 / 2.0, q.2);
                let b = (x as f64 + 0.5 - dx as f64 / 2.0, y as f64 + 0.5 - dy as f64 / 2.0, z);
                pc.ducts.push((if q.2 < z { (a, b) } else { (b, a) }, run_rect(q.0, q.1, d)));
            }
            storey_cells(&mut pc.cells, z).insert((x, y));
            path.push((x as f64 + 0.5, y as f64 + 0.5, z));
            last = Some(n);
        }
        // a duct must not run beside or over the vent's own cells
        for &((lo, hi), (rx0, ry0, rx1, ry1)) in &pc.ducts {
            let body = |c: &Cell| rx0 - 1 <= c.0 && c.0 < rx1 + 1 && ry0 - 1 <= c.1 && c.1 < ry1 + 1;
            let run = |c: &Cell| rx0 <= c.0 && c.0 < rx1 && ry0 <= c.1 && c.1 < ry1;
            // how far a cell is from the nearest cell of the run
            let off = |c: &Cell| (rx0..rx1).flat_map(|i| (ry0..ry1).map(move |j| (c.0 - i).abs() + (c.1 - j).abs())).min().expect("a run has cells");
            let near = [lo.2, hi.2].into_iter().filter_map(|zz| pc.cells_at(zz)).flatten().filter(|c| body(c));
            for c in near {
                if off(c) != 1 || run(c) {
                    return None;
                }
            }
        }
        let (da, db) = (starts.get((states[0].0, states[0].1, states[0].2))?, goals.get((done.0, done.1, done.2))?);
        let da = Door { room: da.room.clone(), side: da.side, a: da.a, level: da.level, width: 1, to: key.clone() };
        let db = Door { room: db.room.clone(), side: db.side, a: db.a, level: db.level, width: 1, to: key };
        Some(Route { a: da, b: db, pieces: vec![pc], path, cost: total })
    }

    // ------------------------------------------------------------ A*

    /// Shortest corridor from `a` to `b`, or None. A turn costs `turn` more, a ramp `ramp_cost`
    /// more than its length, and a door on an upper storey of a tall room `upper` a storey.
    ///
    /// No flat run is longer than `straight` moves, and none shorter than `JOG`, so a long
    /// corridor is a chicane: each jog is wider than the corridor and nothing is seen end to
    /// end. If that finds no way, it is routed without the rule.
    pub fn route(&self, a: &Space, b: &Space, t: &Terms) -> Option<Route> {
        if let Some(r) = self.direct_at(a, b, t.la.as_deref(), t.lb.as_deref(), t.da, t.db) {
            return Some(r);
        }
        if t.straight != 0 {
            return self.route_with(a, b, t, t.straight).or_else(|| self.route_with(a, b, t, 0));
        }
        self.route_with(a, b, t, 0)
    }

    /// The search (`_route` in the Python), with at most `straight` moves in a line (0: any).
    fn route_with(&self, a: &Space, b: &Space, t: &Terms, straight: i32) -> Option<Route> {
        let (sums_a, sums_b) = (self.sums(&[&a.key]), self.sums(&[&b.key]));
        let starts = self.terminals(a, &sums_a, t.la.as_deref(), t.da);
        let goals = self.terminals(b, &sums_b, t.lb.as_deref(), t.db);
        if starts.is_empty() || goals.is_empty() {
            return None;
        }
        let everything: Vec<&[i32]> = sums_a.iter().map(|s| s.0.as_slice()).collect();
        let mut free_cache: HashMap<Head, bool> = HashMap::new();
        let mut free = |x: i32, y: i32, z: i32| {
            *free_cache.entry((x, y, z)).or_insert_with(|| self.count(everything[index(z)], x - 1, y - 1, x + WIDE + 1, y + WIDE + 1) == 0)
        };
        let empty = |z: i32, x0: i32, y0: i32, x1: i32, y1: i32| self.count(everything[index(z)], x0, y0, x1, y1) == 0;
        let ends: BTreeSet<i32> = goals.doors.iter().map(|(k, _)| index(k.2) as i32).collect();
        let h = |x: i32, y: i32, z: i32| -> f64 {
            let d = 0.max(b.x0 - (x + WIDE)).max(x - b.x1) + 0.max(b.y0 - (y + WIDE)).max(y - b.y1);
            if !t.storeyed {
                return d as f64;
            }
            // each storey is a ramp: its run, and its cost
            let n = ends.iter().map(|q| (index(z) as i32 - q).abs()).min().expect("a goal has a storey");
            d.max(n * (RAMP + WIDE)) as f64 + n as f64 * t.ramp_cost
        };
        let up = |s: &Space, z: i32| s.storeys().iter().position(|&l| l == z).expect("a door storey is a storey") as f64;

        let mut heap: BinaryHeap<Reverse<(Cost, Cost, State)>> = BinaryHeap::new();
        // the cheapest way found to each state, and the state it came from
        let mut best: HashMap<State, (f64, Option<State>)> = HashMap::new();
        for ((x, y, z), door) in &starts.doors {
            let st = (*x, *y, *z, side_index(door.side) as i32, 0);
            let g = t.upper * up(a, *z);
            best.insert(st, (g, None));
            heap.push(Reverse((Cost(g + h(*x, *y, *z)), Cost(g), st)));
        }
        let mut done: Option<(State, f64)> = None;
        let mut pops = 0;
        while let Some(Reverse((Cost(f), Cost(g), st))) = heap.pop() {
            if f > t.max_cost {
                return None; // (nothing left can arrive within the allowance)
            }
            let (known, came) = best.get(&st).map_or((1e9, None), |v| *v);
            if g > known {
                continue;
            }
            pops += 1;
            if pops > t.search {
                return None;
            }
            let (x, y, z, d, run) = st;
            if came.is_some() && goals.arrives(x, y, z, d) {
                done = Some((st, g + t.upper * up(b, z)));
                break;
            }
            if g > t.max_cost {
                continue;
            }
            let mut moves: Vec<(State, f64)> = Vec::new();
            for (nd, &(mx, my)) in DIRS.iter().enumerate() {
                let nd = nd as i32;
                if nd == (d + 2) % 4 {
                    continue;
                }
                if straight != 0 && ((nd == d && run >= straight) || (nd != d && run < JOG)) {
                    continue;
                }
                let (nx, ny) = (x + mx, y + my);
                if free(nx, ny, z) || goals.arrives(nx, ny, z, nd) {
                    let nrun = if straight == 0 {
                        0
                    } else if nd == d {
                        run.max(0) + 1
                    } else {
                        1
                    };
                    moves.push(((nx, ny, z, nd, nrun), 1.0 + if nd != d { t.turn } else { 0.0 }));
                }
            }
            let (dx, dy) = DIRS[d as usize];
            if run == -1 {
                // straight off a ramp: a hairpin, and the next flight runs back
                for (sx, sy) in [(-dy, dx), (dy, -dx)] {
                    if (1..=HAIRPIN).all(|k| free(x + sx * k, y + sy * k, z)) {
                        moves.push(((x + sx * HAIRPIN, y + sy * HAIRPIN, z, (d + 2) % 4, if straight != 0 { JOG } else { 0 }), HAIRPIN as f64 + t.turn));
                    }
                }
            }
            let zi = index(z) as i32;
            for dz in [1, -1] {
                // a ramp, straight on
                if !(0 <= zi + dz && zi + dz < LEVELS.len() as i32) {
                    continue;
                }
                let z2 = LEVELS[(zi + dz) as usize];
                let (nx, ny) = (x + dx * (RAMP + WIDE), y + dy * (RAMP + WIDE));
                let (rx0, ry0, rx1, ry1) = ramp_rect(x, y, d);
                let (sx, sy) = if dx != 0 { (0, 1) } else { (1, 0) }; // sideways
                if ![z, z2].iter().all(|&zz| empty(zz, rx0 - sx, ry0 - sy, rx1 + sx, ry1 + sy)) {
                    continue;
                }
                // air over or under either end would open into the ramp's box
                if !empty(z2, x, y, x + WIDE, y + WIDE) || !empty(z, nx, ny, nx + WIDE, ny + WIDE) {
                    continue;
                }
                if free(nx, ny, z2) || goals.arrives(nx, ny, z2, d) {
                    moves.push(((nx, ny, z2, d, -1), (RAMP + WIDE) as f64 + t.ramp_cost));
                }
            }
            for (nst, c) in moves {
                let ng = g + c;
                if ng < best.get(&nst).map_or(1e9, |v| v.0) {
                    best.insert(nst, (ng, Some(st)));
                    heap.push(Reverse((Cost(ng + h(nst.0, nst.1, nst.2)), Cost(ng), nst)));
                }
            }
        }
        let (last, cost) = done?;
        if cost > t.max_cost {
            return None;
        }
        let mut states: Vec<State> = Vec::new();
        let mut at = Some(last);
        while let Some(st) = at {
            states.push(st);
            at = best[&st].1;
        }
        states.reverse();
        let first = states[0];
        self.assemble(starts.get((first.0, first.1, first.2))?, goals.get((last.0, last.1, last.2))?, &states, cost)
    }

    /// Head positions -> pieces (one per ramp, or one flat corridor) and a centre line. None if
    /// the route doubles back over or beside one of its own ramps.
    fn assemble(&self, da: &Door, db: &Door, states: &[State], cost: f64) -> Option<Route> {
        /// One ramp of the route.
        struct Flight {
            /// The index in `flat` of its landing.
            at: usize,
            rect: Rect,
            /// Its centre line, foot then top.
            line: (Point, Point),
            /// The head before it and after it.
            before: Head,
            after: Head,
        }
        let key = format!("c{:02}", self.made + 1);
        let mut flat: Vec<Head> = Vec::new(); // head blocks
        let mut ramps: Vec<Flight> = Vec::new();
        let mut last: Option<State> = None;
        for &q in states {
            let (x, y, z, d, _) = q;
            if let Some(p) = last {
                if p.2 != z {
                    let (dx, dy) = DIRS[d as usize];
                    let (px, py, pz) = (p.0, p.1, p.2);
                    let hw = WIDE as f64 / 2.0;
                    // centre line: block's edge to landing's edge
                    let a = (px as f64 + hw + dx as f64 * hw, py as f64 + hw + dy as f64 * hw, pz);
                    let b = (x as f64 + hw - dx as f64 * hw, y as f64 + hw - dy as f64 * hw, z);
                    ramps.push(Flight {
                        at: flat.len(),
                        rect: ramp_rect(px, py, d),
                        line: if pz < z { (a, b) } else { (b, a) },
                        before: (px, py, pz),
                        after: (x, y, z),
                    });
                } else if (x - p.0).abs() + (y - p.1).abs() > 1 {
                    // a hairpin: the blocks it steps over
                    let n = (x - p.0).abs().max((y - p.1).abs());
                    for k in 1..n {
                        flat.push((p.0 + ((x - p.0) * k).div_euclid(n), p.1 + ((y - p.1) * k).div_euclid(n), z));
                    }
                }
            }
            flat.push((x, y, z));
            last = Some(q);
        }
        let mut cells: Vec<(i32, Cells)> = Vec::new();
        for &(x, y, z) in &flat {
            storey_cells(&mut cells, z).extend(block(x, y));
        }
        let at = |z: i32| cells.iter().find(|(l, _)| *l == z).map(|(_, cs)| cs);
        let meets = |z: i32, other: &Cells| at(z).is_some_and(|cs| !cs.is_disjoint(other));
        for (n, r) in ramps.iter().enumerate() {
            let (rx0, ry0, rx1, ry1) = r.rect;
            let (lo, hi) = r.line;
            let run_x = rx1 - rx0 == RAMP;
            let (sx, sy) = if run_x { (0, 1) } else { (1, 0) };
            let body: Cells = (rx0 - sx..rx1 + sx).flat_map(|i| (ry0 - sy..ry1 + sy).map(move |j| (i, j))).collect();
            if meets(lo.2, &body) || meets(hi.2, &body) {
                return None;
            }
            if meets(r.after.2, &block(r.before.0, r.before.1)) || meets(r.before.2, &block(r.after.0, r.after.1)) {
                return None;
            }
            for m in &ramps[n + 1..] {
                let shared = [m.line.0 .2, m.line.1 .2].iter().any(|z| *z == lo.2 || *z == hi.2);
                if shared && rect_gap(r.rect, m.rect) == (0, 0) {
                    return None;
                }
            }
        }
        let mut pieces: Vec<Piece> = Vec::new();
        if ramps.is_empty() {
            pieces.push(Piece { key, kind: "corridor", floor: flat[0].2, cells, ramp: None, ramp_cells: None, ducts: Vec::new() });
        } else {
            let cuts: Vec<usize> = ramps[1..].iter().map(|r| r.at).chain([flat.len()]).collect();
            let mut start = 0;
            for (n, r) in ramps.iter().enumerate() {
                let name = if ramps.len() == 1 { key.clone() } else { format!("{key}{}", b"abcdefgh"[n] as char) };
                let mut pc =
                    Piece { key: name, kind: "ramp", floor: r.line.0 .2, cells: Vec::new(), ramp: Some(r.line), ramp_cells: Some(r.rect), ducts: Vec::new() };
                for &(x, y, z) in &flat[start..cuts[n]] {
                    storey_cells(&mut pc.cells, z).extend(block(x, y));
                }
                pieces.push(pc);
                start = cuts[n];
            }
            // a cell belongs to the first piece that has it
            let mut seen: BTreeSet<(Cell, i32)> = BTreeSet::new();
            for pc in &mut pieces {
                for (z, cs) in &mut pc.cells {
                    cs.retain(|c| !seen.contains(&(*c, *z)));
                    seen.extend(cs.iter().map(|c| (*c, *z)));
                }
            }
        }
        let mut path: Vec<Point> = Vec::new();
        for (n, &(x, y, z)) in flat.iter().enumerate() {
            for r in ramps.iter().filter(|r| r.at == n) {
                let (lo, hi) = r.line;
                path.extend(if r.before.2 == lo.2 { [lo, hi] } else { [hi, lo] });
            }
            path.push((x as f64 + WIDE as f64 / 2.0, y as f64 + WIDE as f64 / 2.0, z));
        }
        let da = Door { to: pieces[0].key.clone(), ..da.clone() };
        let db = Door { to: pieces[pieces.len() - 1].key.clone(), ..db.clone() };
        Some(Route { a: da, b: db, pieces, path, cost })
    }

    /// Take a route's cells, and record its two doors.
    pub fn commit(&mut self, r: Route) -> &Route {
        for pc in &r.pieces {
            let i = self.id(&pc.key);
            for (z, cs) in &pc.cells {
                for &(x, y) in cs {
                    self.mark((x, y, x + 1, y + 1), *z, i);
                }
            }
            if let (Some((foot, top)), Some(rect)) = (pc.ramp, pc.ramp_cells) {
                for z in [foot.2, top.2] {
                    self.mark(rect, z, i);
                }
            }
            for &((lo, hi), rect) in &pc.ducts {
                for z in [lo.2, hi.2] {
                    self.mark(rect, z, i);
                }
            }
        }
        self.doors_mut(&r.a.room).push(r.a.clone());
        self.doors_mut(&r.b.room).push(r.b.clone());
        self.made += 1;
        self.vents_made += (r.pieces[0].kind == "vent") as usize;
        self.routes.push(r);
        &self.routes[self.routes.len() - 1]
    }

    /// Every cell this number owns is free again.
    fn clear(&mut self, i: i32) {
        for owner in &mut self.owner {
            for v in owner.iter_mut().filter(|v| **v == i) {
                *v = 0;
            }
        }
    }

    /// Take a committed route out again: its cells are free, and its two doors gone. (The
    /// Python knows a route and its doors by identity; here a route is known by its key, and a
    /// door by what it is, since no room has two doors alike.)
    pub fn remove(&mut self, r: &Route) {
        for pc in &r.pieces {
            if let Some(i) = self.id_of(&pc.key) {
                self.clear(i);
            }
        }
        for d in [&r.a, &r.b] {
            self.doors_mut(&d.room).retain(|q| q != d);
        }
        self.routes.retain(|q| q.key() != r.key());
    }

    /// Take a room out: every route to it, its doors and its cells (a pod's block goes back to
    /// its hall). Its pods are the caller's to drop first (`Network::drop` does).
    pub fn drop(&mut self, key: &str) {
        let s = self.space(key).clone();
        let gone: Vec<Route> = self.routes.iter().filter(|r| r.a.room == key || r.b.room == key).cloned().collect();
        for r in &gone {
            self.remove(r);
        }
        let i = self.ids[key];
        self.clear(i);
        if !s.host.is_empty() {
            let host = self.space(&s.host);
            let (x0, y0, x1, y1) = shell_cells(&s, host);
            let hall = self.ids[&host.key];
            let owner = &mut self.owner[index(s.floor)];
            for x in x0 - self.ox..(x1 - self.ox).min(self.w) {
                for y in y0 - self.oy..(y1 - self.oy).min(self.h) {
                    let v = &mut owner[(x * self.h + y) as usize];
                    if *v == 0 {
                        *v = hall;
                    }
                }
            }
        }
        self.doors.retain(|(k, _)| k != key);
        self.sp.retain(|q| q.key != key);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A room for a test: a storey high on the ground floor, unless said otherwise.
    pub(crate) fn space(key: &str, rect: Rect, floor: i32, top: i32, kind: &'static str) -> Space {
        let (x0, y0, x1, y1) = rect;
        Space {
            key: key.into(),
            kind,
            x0,
            y0,
            x1,
            y1,
            floor,
            top,
            host: String::new(),
            pool: None,
            decks: None,
            clear: Cells::new(),
            preset: Vec::new(),
            high: Vec::new(),
            galleries: Vec::new(),
            cellar: None,
            flood: None,
            cuts: Vec::new(),
            sunk: None,
            base: None,
            blocks: Vec::new(),
        }
    }

    pub(crate) fn room(key: &str, rect: Rect) -> Space {
        space(key, rect, 0, 236, "medium")
    }

    #[test]
    fn rooms_two_cells_apart_get_a_stub_through_the_wall() {
        let g = Grid::new(vec![room("a", (0, 0, 6, 6)), room("b", (8, 1, 14, 7))], 2);
        let r = g.route(g.space("a"), g.space("b"), &Terms::default()).unwrap();
        assert_eq!((r.pieces[0].kind, r.pieces[0].key.as_str(), r.cost), ("stub", "c01", 2.0));
        assert_eq!((r.a.to.as_str(), r.b.to.as_str(), r.a.side, r.b.side), ("b", "a", 'E', 'W'));
        assert_eq!(r.pieces[0].cells_at(0).unwrap().len(), 2);
    }

    #[test]
    fn a_storey_apart_takes_one_ramp_of_four_cells() {
        let g = Grid::new(vec![room("a", (0, 0, 6, 6)), space("b", (16, 0, 22, 6), 276, 512, "medium")], 2);
        let r = g.route(g.space("a"), g.space("b"), &Terms::default()).unwrap();
        assert_eq!(r.ramps(), 1);
        let (foot, top) = r.pieces[0].ramp.unwrap();
        assert_eq!((foot.2, top.2, (top.0 - foot.0).abs() + (top.1 - foot.1).abs()), (0, 276, RAMP as f64));
        assert_eq!((r.a.level, r.b.level), (0, 276));
    }

    #[test]
    fn long_corridors_jog() {
        let g = Grid::new(vec![room("a", (0, 0, 6, 6)), room("b", (30, 0, 36, 6))], 2);
        let r = g.route(g.space("a"), g.space("b"), &Terms::default()).unwrap();
        let (mut longest, mut n) = (1, 1);
        for w in r.path.windows(3) {
            let straight = (w[1].0 - w[0].0, w[1].1 - w[0].1) == (w[2].0 - w[1].0, w[2].1 - w[1].1);
            n = if straight { n + 1 } else { 1 };
            longest = longest.max(n);
        }
        assert!(longest <= STRAIGHT);
        assert!(r.path.iter().any(|p| p.1 != r.path[0].1));
    }

    #[test]
    fn two_storeys_apart_and_close_together_is_a_switchback() {
        let g = Grid::new(vec![room("low", (0, 0, 5, 4)), space("high", (0, 0, 4, 5), 551, 787, "medium")], 2);
        let r = g.route(g.space("high"), g.space("low"), &Terms::default()).unwrap();
        assert_eq!(r.ramps(), 2);
        assert!(r.switchback());
        assert_eq!(r.pieces.iter().map(|pc| pc.key.as_str()).collect::<Vec<_>>(), ["c01a", "c01b"]);
        let gap = rect_gap(r.pieces[0].ramp_cells.unwrap(), r.pieces[1].ramp_cells.unwrap());
        assert!(gap == (0, 1) || gap == (1, 0), "side by side, a cell of wall between: {gap:?}");
        let landing: usize = r.pieces.iter().filter_map(|pc| pc.cells_at(276)).map(|c| c.len()).sum();
        assert!(landing >= (HAIRPIN + 1) as usize); // the turn, on the middle storey
    }

    #[test]
    fn a_door_can_be_asked_for_on_one_storey() {
        let g = Grid::new(vec![space("hall", (0, 0, 10, 9), 0, 787, "vast"), space("up", (14, 2, 18, 6), 276, 512, "medium")], 2);
        let (up, hall) = (g.space("up"), g.space("hall"));
        // left to itself it arrives upstairs, on the level
        let r = g.route(up, hall, &Terms::default()).unwrap();
        assert_eq!((r.ramps(), r.b.level), (0, 276));
        // not upstairs, where it would arrive unasked
        let r = g.route(up, hall, &Terms { lb: Some(vec![0]), storeyed: true, ..Terms::default() }).unwrap();
        assert_eq!((r.ramps(), r.a.level, r.b.level), (1, 276, 0));
        // the room has no such storey
        assert_eq!(g.route(up, hall, &Terms { la: Some(vec![551]), ..Terms::default() }), None);
        // and a search that may look at nothing finds nothing
        assert_eq!(g.route(up, hall, &Terms { search: 0, ..Terms::default() }), None);
    }

    #[test]
    fn a_vent_is_one_cell_wide_and_slopes_between_storeys() {
        let mut g = Grid::new(vec![room("a", (0, 0, 6, 6)), room("b", (9, 1, 14, 7)), space("up", (0, 12, 6, 17), 276, 512, "medium")], 2);
        let flat = g.vent(g.space("a"), g.space("b"), &VentTerms::default()).unwrap();
        let flat = g.commit(flat).clone();
        let pc = &flat.pieces[0];
        assert_eq!((pc.kind, pc.key.as_str(), flat.a.width, flat.b.width, pc.ducts.len()), ("vent", "v01", 1, 1, 0));
        let cells = pc.cells_at(0).unwrap();
        assert_eq!(cells.len(), 3); // straight across the three cells between them
        assert_eq!(cells.iter().map(|c| c.1).collect::<BTreeSet<_>>().len(), 1);
        assert!(g.doors("a").contains(&flat.a));
        let slope = g.vent(g.space("up"), g.space("a"), &VentTerms::default()).unwrap();
        let ((foot, top), _) = slope.pieces[0].ducts[0];
        assert_eq!((foot.2, top.2), (0, 276));
        assert_eq!((top.0 - foot.0).abs() + (top.1 - foot.1).abs(), RAMP as f64); // four cells for the storey
        assert_eq!((slope.a.level, slope.b.level, slope.pieces[0].key.as_str()), (276, 0, "v02"));
        // a cell clear of the other vent and of every room
        let own = [0, g.id_of("a").unwrap(), g.id_of("up").unwrap()];
        for (z, cells) in &slope.pieces[0].cells {
            for &(x, y) in cells {
                assert!((-1..=1).all(|i| (-1..=1).all(|j| own.contains(&g.owner_at(x + i, y + j, *z)))));
            }
        }
        // only on the storeys asked for
        assert_eq!(g.vent(g.space("up"), g.space("a"), &VentTerms { lb: Some(vec![276]), ..VentTerms::default() }), None);
    }

    #[test]
    fn a_route_can_be_taken_out_again() {
        let mut g = Grid::new(vec![room("a", (0, 0, 6, 6)), room("b", (14, 9, 20, 15)), room("c", (24, 9, 28, 15))], 2);
        let r = g.route(g.space("a"), g.space("b"), &Terms::default()).unwrap();
        let r = g.commit(r).clone();
        let cells = r.pieces[0].cells_at(0).unwrap();
        assert!(cells.iter().all(|&(x, y)| g.owner_at(x, y, 0) > 0));
        assert_eq!((g.doors("a"), g.doors("b")), (&[r.a.clone()][..], &[r.b.clone()][..]));
        g.remove(&r);
        assert!(cells.iter().all(|&(x, y)| g.owner_at(x, y, 0) == 0));
        assert!(g.doors("a").is_empty() && g.doors("b").is_empty() && g.routes.is_empty());
        let again = g.route(g.space("a"), g.space("b"), &Terms::default()).unwrap();
        let again = g.commit(again).clone();
        assert_eq!(again.path, r.path); // the way is free again
        assert_eq!((r.key(), again.key()), ("c01", "c02")); // but a key is never used twice
        assert_eq!(g.owner_at(g.ox - 1, 0, 0), -1);
    }
}
