//! A level from above, as a list of 2D primitives in cells (x east, y north): what the window
//! draws with egui and `shot` rasterises into a PNG. The shape is `pd_tools::lab`'s in the
//! clone (a `Prim` list, no drawing library here), with text that both sides draw.
//!
//! Everything is drawn storey by storey from the lowest up, each thing in its storey's colour:
//! rooms (pods too), corridors, stubs, vents (thin), enclosed ramps and the open ramps inside
//! rooms (an arrow uphill), decks and catwalks, pools and their bridges, cover, drops, grates,
//! windows and doors. Over that, every room's and corridor's outline again (so what lies under a
//! hall still shows), the names and keys, and the dead ends: the allowed ones ringed in green
//! ("camp" where there is a room to camp in, "deck" for a balcony), the others boxed in red.
//! `View::storey` shows one storey alone: rooms that are only air there are drawn hollow.
//!
//! On top of the level, what is being done to it: the room picked to join from (`View::pick`),
//! the corridors that could join it to others as pale ghosts with a numbered disc each
//! (`View::joins`, the one under the pointer in yellow), a corridor picked to take out
//! (`View::route`) and one just taken out (`View::gone`). `hit` is the other way round: what is
//! drawn on top at a place, so a click picks what the eye sees.
//!
//! Over all of that, with the room tool in hand, the room boxes (`View::boxes`, drawn by `lots`):
//! every lot as the recipe places it, edged in its storey's colour, pods over their halls. The
//! level underneath may be older than the lots, so a box the level does not have there is filled
//! in and says why ("moved", "resized", "new"), the place it left is dimmed, and a room with no
//! lot any more is crossed out. The selected box has a thick edge, eight grips sized in pixels
//! and its key, size and heights on a panel beside it; a change that is refused turns it red,
//! with the reason on the panel in lines short enough to read. `bare` is the boxes on the grid
//! alone, for a recipe that makes no level; `hit_box` is what a click takes hold of.
//!
//! Every colour is in `colour`, and nowhere else.

use autolevel::boxes::Part;
use autolevel::build::Fix;
use autolevel::candidates::{footprint, Asked, Candidate};
use autolevel::cells::{Cell, Cells, Rect};
use autolevel::deadends::DeadEnd;
use autolevel::furnish::rectangles;
use autolevel::level::Level;
use autolevel::plan::MODULE;
use autolevel::recipe::Doorway;
use autolevel::route::{Point, Route};
use autolevel::scatter::Lot;
use autolevel::space::{Space, DIRS};
use autolevel::storeys::{snap_top, LEVELS};

/// A place on the plan, in cells.
pub type P = (f32, f32);
pub type Rgba = [u8; 4];

/// One thing to draw. Places and radii are in cells; widths and text sizes in pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    /// A filled convex polygon.
    Poly {
        pts: Vec<P>,
        fill: Rgba,
    },
    Line {
        a: P,
        b: P,
        col: Rgba,
        width_px: f32,
    },
    Circle {
        c: P,
        r: f32,
        col: Rgba,
        filled: bool,
    },
    /// A line of text about `at` (or starting there), its middle at that height.
    Text {
        at: P,
        text: String,
        col: Rgba,
        size_px: f32,
        centred: bool,
    },
    /// A number on a disc `r_px` pixels in radius, over everything drawn before it, text too.
    Badge {
        at: P,
        text: String,
        fill: Rgba,
        col: Rgba,
        r_px: f32,
        size_px: f32,
    },
}

/// The picture's colours, all of them.
pub mod colour {
    use super::Rgba;

    pub const BACKGROUND: Rgba = [18, 20, 26, 255];
    pub const GRID: Rgba = [255, 255, 255, 10];
    pub const GRID_FOURTH: Rgba = [255, 255, 255, 26];
    /// A floor on each storey, lowest first (`storeys::LEVELS`).
    pub const STOREY: [Rgba; 17] = [
        [66, 84, 176, 255],   // -276: the low road
        [34, 140, 132, 255],  // 0
        [104, 160, 66, 255],  // 276
        [186, 164, 52, 255],  // 551
        [212, 124, 54, 255],  // 827
        [200, 92, 132, 255],  // 1102
        [164, 104, 200, 255], // 1378
        [208, 212, 224, 255], // 1654
        // an atrium's storeys: the same round of hues again, paler
        [120, 210, 204, 255], // 1929
        [168, 214, 130, 255], // 2205
        [232, 214, 120, 255], // 2480
        [240, 170, 120, 255], // 2756
        [236, 146, 182, 255], // 3031
        [200, 160, 240, 255], // 3307
        [130, 150, 236, 255], // 3583
        [150, 240, 170, 255], // 3858
        [255, 248, 200, 255], // 4134
    ];
    /// A room that is only air on the storey shown.
    pub const VOID: Rgba = [30, 33, 42, 255];
    pub const WALL: Rgba = [8, 9, 12, 255];
    pub const DOOR: Rgba = [255, 255, 255, 255];
    pub const RAMP_ARROW: Rgba = [255, 226, 96, 255];
    /// The rescue's ladders up the edges of decks (`furnish::Chute`), and its bridges' outlines.
    pub const LADDER: Rgba = [80, 255, 220, 255];
    pub const VENT: Rgba = [255, 150, 235, 255];
    pub const POOL: Rgba = [36, 104, 224, 255];
    pub const POOL_EDGE: Rgba = [140, 190, 255, 255];
    pub const COVER: Rgba = [22, 24, 30, 255];
    pub const COVER_EDGE: Rgba = [130, 134, 144, 255];
    pub const RAIL: Rgba = [255, 255, 255, 255];
    pub const GLASS: Rgba = [110, 225, 255, 255];
    pub const BALCONY: Rgba = [255, 170, 70, 255];
    pub const DROP: Rgba = [0, 0, 0, 255];
    pub const DROP_MARK: Rgba = [255, 226, 96, 255];
    pub const GRATE: Rgba = [52, 56, 66, 255];
    pub const GRATE_MARK: Rgba = [190, 196, 208, 255];
    pub const NAME: Rgba = [255, 255, 255, 255];
    pub const KEY: Rgba = [226, 230, 238, 255];
    pub const ROUTE_KEY: Rgba = [205, 210, 222, 255];
    pub const FAINT: Rgba = [150, 156, 170, 255];
    pub const CAMP: Rgba = [96, 240, 130, 255];
    pub const DEAD_END: Rgba = [255, 56, 56, 255];
    pub const MARK: Rgba = [255, 240, 70, 255];
    pub const TITLE: Rgba = [236, 238, 244, 255];
    /// The room picked to join from.
    pub const PICK: Rgba = [80, 232, 255, 255];
    /// The places on a picked room's walls a doorway can be asked for (a tick a cell), and the
    /// doorway asked for.
    pub const HANDLE: Rgba = [80, 232, 255, 120];
    pub const ASKED: Rgba = [255, 120, 255, 255];
    /// A corridor that could be made: its floor (pale, so what is under it shows), its edge, the
    /// disc its number is on and the number.
    pub const JOIN: Rgba = [244, 246, 255, 150];
    pub const JOIN_EDGE: Rgba = [255, 255, 255, 255];
    pub const JOIN_DISC: Rgba = [250, 250, 252, 255];
    pub const JOIN_NUMBER: Rgba = [10, 12, 18, 255];
    /// The other candidates while one is under the pointer.
    pub const JOIN_DIM: Rgba = [255, 255, 255, 50];
    pub const JOIN_DIM_EDGE: Rgba = [255, 255, 255, 110];
    /// The one under the pointer.
    pub const HOT: Rgba = [255, 240, 70, 190];
    /// A corridor just taken out.
    pub const GONE: Rgba = [255, 56, 56, 110];
    /// A room box the level drawn does not have there (moved, resized, new): its edge and words.
    pub const CHANGED: Rgba = [255, 255, 255, 255];
    /// Where the level still draws a room whose box has moved: dimmed.
    pub const WAS: Rgba = [18, 20, 26, 165];
    /// The grips of the selected box: squares, edged in the box's own colour.
    pub const GRIP: Rgba = [255, 255, 255, 255];
    /// What the selected box's words stand on, so they read over anything.
    pub const PANEL: Rgba = [10, 11, 15, 236];
    /// The words of a refusal, under their box's name in red; and "deleted" on a crossed-out room.
    pub const REFUSAL: Rgba = [255, 222, 222, 255];
    /// What was chosen for a room by hand: the words under its name.
    pub const CHOSEN: Rgba = [226, 196, 255, 255];

    /// The colour of the storey whose floor is `z`.
    pub fn storey(z: i32) -> Rgba {
        let i = autolevel::storeys::LEVELS.iter().position(|&l| l >= z).unwrap_or(STOREY.len() - 1);
        STOREY[i]
    }

    /// `a` with this much of `b` mixed in.
    pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
        let f = |k: usize| (a[k] as f32 + (b[k] as f32 - a[k] as f32) * t).round() as u8;
        [f(0), f(1), f(2), f(3)]
    }

    /// `c` let through: 0 clear, 255 solid.
    pub fn faded(c: Rgba, alpha: u8) -> Rgba {
        [c[0], c[1], c[2], alpha]
    }

    /// The edge of a room box on the storey whose floor is `z`.
    pub fn box_edge(z: i32) -> Rgba {
        lit(storey(z), 0.65)
    }

    /// Lighter by this much (darker below 0).
    pub fn lit(c: Rgba, t: f32) -> Rgba {
        let to = if t >= 0.0 { [255, 255, 255, c[3]] } else { [0, 0, 0, c[3]] };
        mix(c, to, t.abs())
    }
}

/// Something picked out on the plan (a row of the dead-end list): rooms, pieces of deck and
/// routes to outline, and where it hangs off the level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mark {
    /// Rooms wholly in it.
    pub rooms: Vec<String>,
    /// (room, storey) of its other places: decks, and the root's side of nothing.
    pub decks: Vec<(String, i32)>,
    /// Its routes, by their keys.
    pub routes: Vec<String>,
    /// Where its one way in arrives: the place, and the storey.
    pub at: Option<(P, i32)>,
}

impl Mark {
    /// The mark for a dead end. `routes` are the network's the dead end's numbers count in.
    pub fn of(e: &DeadEnd, routes: &[Route]) -> Mark {
        let decks = e
            .places
            .iter()
            .map(|k| place(k))
            .filter(|(room, _)| !e.rooms.iter().any(|r| r == room))
            .filter_map(|(room, z)| Some((room.to_string(), z?)))
            .collect();
        let keys = e.routes.iter().filter_map(|&n| routes.get(n)).map(|r| r.key().to_string()).collect();
        Mark { rooms: e.rooms.clone(), decks, routes: keys, at: Some(port(e)) }
    }
}

/// A place's room, and its storey if its key says ("spire1@827.2"; a pod's key is its room's).
pub fn place(key: &str) -> (&str, Option<i32>) {
    match key.split_once('@') {
        Some((room, rest)) => (room, rest.split('.').next().and_then(|z| z.parse().ok())),
        None => (key, None),
    }
}

/// Where a dead end's one way in arrives in it: the middle of that standing block, and its storey.
pub fn port(e: &DeadEnd) -> (P, i32) {
    let n = if e.way.a == e.root { e.way.pb } else { e.way.pa };
    (((n.0 + 1) as f32 / 2.0, (n.1 + 1) as f32 / 2.0), n.2)
}

/// What to show.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// One storey alone, by its floor (None: all of them, the lowest underneath).
    pub storey: Option<i32>,
    /// What is picked out.
    pub mark: Option<Mark>,
    /// Pixels to a cell where it will be drawn, as near as is known: which names fit their rooms.
    pub scale: f32,
    /// The room picked to join from, and the storey its door would be on.
    pub pick: Option<(String, i32)>,
    /// What is asked of its joins: the doorways (drawn on their walls), the room at the other
    /// end (framed), the way.
    pub asked: Asked,
    /// Nothing was found through what was asked: what the recipe's join says, written by the pick.
    pub none: Option<String>,
    /// Corridors that could be made from it, each with its number in the list.
    pub joins: Vec<(usize, Candidate)>,
    /// The number of the one under the pointer.
    pub hot: Option<usize>,
    /// The route picked, by its key.
    pub route: Option<String>,
    /// A route just taken out, drawn where it was.
    pub gone: Option<Route>,
    /// The room boxes, drawn over the level (the room tool is in hand).
    pub boxes: Option<Boxes>,
    /// What the recipe chooses for single rooms, to say on the plan: (the room's key, the
    /// words, as `choices::Chosen::words` gives them: "layout ring, cover none").
    pub chosen: Vec<(String, String)>,
}

/// The room boxes as the recipe places them, to draw over the level and take hold of: the
/// level underneath may be an older one, or there may be none for these lots at all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Boxes {
    /// The lots as the recipe places them, or as a change being tried would.
    pub lots: Vec<Lot>,
    /// The room selected: drawn with its grips.
    pub selected: Option<String>,
    /// Why the change being tried can't be made: the selected box is then drawn as refused,
    /// with these words.
    pub refused: Option<String>,
    /// The site the rooms stand on, in cells (from the origin), where it is known: the bare plan
    /// is framed on it, so an empty start has the whole site to draw on.
    pub site: Option<(i32, i32)>,
}

/// What of the room boxes a place on the plan shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxHit {
    /// The room, by its key.
    pub key: String,
    /// Its body, or one of the selected box's sides or corners.
    pub part: Part,
}

/// What of the room boxes is at `at` (cells), with one storey shown or all of them: a side or a
/// corner of the selected box if `at` is within `grip` cells of it (inside the box, within a
/// third of the way across at most), or else the body of the box drawn on top there, chosen as
/// `hit` chooses a room (the highest floor, a pod over its hall).
pub fn hit_box(lots: &[Lot], at: P, storey: Option<i32>, selected: Option<&str>, grip: f32) -> Option<BoxHit> {
    if let Some(a) = selected.and_then(|key| lots.iter().find(|a| a.key == key)) {
        let (x0, y0, x1, y1) = cell_box(a.rect());
        if x0 - grip <= at.0 && at.0 <= x1 + grip && y0 - grip <= at.1 && at.1 <= y1 + grip {
            // (inside the box a side's hold is a third of the way across at most, so a small
            // box keeps a middle to be moved by)
            let side = |v: f32, lo: f32, hi: f32| {
                let g = grip.min((hi - lo) / 3.0);
                if v <= lo + g {
                    -1
                } else if v >= hi - g {
                    1
                } else {
                    0
                }
            };
            let (sx, sy) = (side(at.0, x0, x1), side(at.1, y0, y1));
            if (sx, sy) != (0, 0) {
                return Some(BoxHit { key: a.key.clone(), part: Part::Edge(sx, sy) });
            }
        }
    }
    let (x, y) = (at.0.floor() as i32, at.1.floor() as i32);
    let inside = |a: &&Lot| a.x0 <= x && x < a.x1 && a.y0 <= y && y < a.y1;
    let on = |a: &&Lot| storey.is_none_or(|z| a.floor == z || (a.host.is_empty() && a.floor <= z && z < a.top));
    let top = lots.iter().filter(inside).filter(on).max_by_key(|a| (storey == Some(a.floor), a.floor, !a.host.is_empty()));
    top.map(|a| BoxHit { key: a.key.clone(), part: Part::Body })
}

impl Default for View {
    fn default() -> Self {
        View {
            storey: None,
            mark: None,
            scale: 25.0,
            pick: None,
            asked: Asked::default(),
            none: None,
            joins: Vec::new(),
            hot: None,
            route: None,
            gone: None,
            boxes: None,
            chosen: Vec::new(),
        }
    }
}

/// What a place on the plan shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A room (a pod too), by its key.
    Room(String),
    /// A corridor, stub or vent, by its route's key.
    Route(String),
}

/// What is drawn on top at `at` (cells), with one storey shown or all of them: a corridor, or
/// else a room. With every storey shown the highest thing there wins, as it is drawn last; with
/// one, a room that is only air on it still counts (it is drawn hollow).
pub fn hit(lv: &Level, at: P, storey: Option<i32>) -> Option<Hit> {
    let (x, y) = (at.0.floor() as i32, at.1.floor() as i32);
    let shown = |z: i32| storey.is_none_or(|s| s == z);
    let top = |r: &Route| footprint(r).into_iter().filter(|&(z, cx, cy)| cx == x && cy == y && shown(z)).map(|(z, _, _)| z).max();
    let route = lv.net.routes().iter().filter_map(|r| top(r).map(|z| (z, r.key()))).max_by_key(|(z, _)| *z);
    let inside = |s: &&Space| s.x0 <= x && x < s.x1 && s.y0 <= y && y < s.y1;
    let on = |s: &&Space| storey.is_none_or(|z| s.floor == z || (s.host.is_empty() && s.storeys().contains(&z)));
    let room = lv.spaces().iter().filter(inside).filter(on).max_by_key(|s| (storey == Some(s.floor), s.floor, !s.host.is_empty()));
    match (route, room) {
        // (a corridor under a hall's floor is under the hall)
        (Some((z, _)), Some(s)) if storey.is_none() && z < s.floor => Some(Hit::Room(s.key.clone())),
        (Some((_, key)), _) => Some(Hit::Route(key.to_string())),
        (None, Some(s)) => Some(Hit::Room(s.key.clone())),
        (None, None) => None,
    }
}

/// The doorway on a room's wall at `at` (cells), if `at` is within `grip` cells of one of its
/// walls (inside or out) and along it: the nearest wall, and the cell along it.
pub fn wall_at(s: &Space, at: P, grip: f32) -> Option<Doorway> {
    wall_of(s.rect(), at, grip).map(|(_, d)| d)
}

/// `wall_at` for a rectangle of cells, with how far `at` is off the wall (cells).
pub fn wall_of(r: Rect, at: P, grip: f32) -> Option<(f32, Doorway)> {
    let (x0, y0, x1, y1) = cell_box(r);
    let walls = [('E', at.0 - x1, at.1, y0, y1), ('W', at.0 - x0, at.1, y0, y1), ('N', at.1 - y1, at.0, x0, x1), ('S', at.1 - y0, at.0, x0, x1)];
    let near = walls.into_iter().filter(|&(_, off, along, lo, hi)| off.abs() <= grip && lo <= along && along < hi);
    near.min_by(|p, q| p.1.abs().total_cmp(&q.1.abs())).map(|(side, off, along, _, _)| (off.abs(), Doorway { side, at: along.floor() as i32 }))
}

/// The room whose wall a click at `at` is on, and the doorway there: the rooms in `first` before
/// the rest (the picked room, the room asked for), and of the rest only those `hit` would pick on
/// the storey shown (pods are joined inside their halls: none of theirs).
pub fn wall_hit(lv: &Level, at: P, grip: f32, storey: Option<i32>, first: &[&str]) -> Option<(String, Doorway)> {
    let rooms = lv.spaces().iter().filter(|s| s.host.is_empty());
    let shown = |s: &&Space| storey.is_none_or(|z| s.storeys().contains(&z) || s.floor == z);
    let firsts = first.iter().filter_map(|k| lv.spaces().iter().find(|s| s.key == *k && s.host.is_empty()));
    firsts.chain(rooms.filter(shown)).find_map(|s| wall_at(s, at, grip).map(|d| (s.key.clone(), d)))
}

/// The storey a room picked on the plan is joined from: the one shown, if the room takes doors
/// there, or else its own floor.
pub fn pick_storey(s: &Space, shown: Option<i32>) -> i32 {
    shown.filter(|z| s.door_levels().contains(z)).unwrap_or(s.floor)
}

/// The place `f` of the way (0 to 1) along a route's centre line.
pub fn along(path: &[Point], f: f32) -> P {
    let at = |p: &Point| (p.0 as f32, p.1 as f32);
    let len = |w: &[Point]| ((w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs()) as f32;
    let mut left = path.windows(2).map(len).sum::<f32>() * f.clamp(0.0, 1.0);
    for w in path.windows(2) {
        let l = len(w);
        if left <= l && l > 0.0 {
            let (a, b) = (at(&w[0]), at(&w[1]));
            return (a.0 + (b.0 - a.0) * left / l, a.1 + (b.1 - a.1) * left / l);
        }
        left -= l;
    }
    path.last().map_or((0.0, 0.0), at)
}

/// A row of the legend.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub swatch: Swatch,
    pub col: Rgba,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    Fill,
    Line,
    Ring,
}

/// The plan of a level: what to draw, the bounds it lies in (cells), and its legend.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub prims: Vec<Prim>,
    pub lo: P,
    pub hi: P,
    pub legend: Vec<Key>,
}

/// The storeys a level has floor on, lowest first: rooms, decks and corridors.
pub fn storeys(lv: &Level) -> Vec<i32> {
    let mut on: Vec<i32> = lv.spaces().iter().map(|s| s.floor).collect();
    for (_, it) in &lv.interiors {
        on.extend(it.deck.iter().filter(|(_, cells)| !cells.is_empty()).map(|(&z, _)| z));
    }
    for r in lv.net.routes() {
        on.extend(r.pieces.iter().flat_map(|pc| pc.cells.iter().map(|(z, _)| *z)));
    }
    LEVELS.iter().copied().filter(|z| on.contains(z)).collect()
}

/// The level's bounds in cells, with a margin: (south-west, north-east).
pub fn bounds(lv: &Level) -> (P, P) {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let mut take = |r: Rect| (x0, y0, x1, y1) = (x0.min(r.0), y0.min(r.1), x1.max(r.2), y1.max(r.3));
    for s in lv.spaces() {
        take(s.rect());
    }
    for r in lv.net.routes() {
        for pc in &r.pieces {
            for &(x, y) in pc.cells.iter().flat_map(|(_, cells)| cells) {
                take((x, y, x + 1, y + 1));
            }
            pc.ramp_cells.into_iter().for_each(&mut take);
            pc.ducts.iter().for_each(|(_, rect)| take(*rect));
        }
    }
    if x0 > x1 {
        return ((0.0, 0.0), (1.0, 1.0));
    }
    ((x0 as f32 - 1.5, y0 as f32 - 1.5), (x1 as f32 + 1.5, y1 as f32 + 1.5))
}

/// The bounds of what a view shows of a level: the level's, grown to hold the corridors that
/// are not part of it (those that could be made, and one just taken out) and the room boxes.
pub fn bounds_of(lv: &Level, view: &View) -> (P, P) {
    let (mut lo, mut hi) = bounds(lv);
    for r in view.joins.iter().map(|(_, c)| &c.route).chain(&view.gone) {
        for (_, x, y) in footprint(r) {
            lo = (lo.0.min(x as f32 - 1.5), lo.1.min(y as f32 - 1.5));
            hi = (hi.0.max(x as f32 + 2.5), hi.1.max(y as f32 + 2.5));
        }
    }
    hold(lo, hi, view)
}

/// Bounds grown to hold the view's room boxes, with the level's own margin round them.
fn hold(mut lo: P, mut hi: P, view: &View) -> (P, P) {
    for a in view.boxes.iter().flat_map(|b| &b.lots) {
        lo = (lo.0.min(a.x0 as f32 - 1.5), lo.1.min(a.y0 as f32 - 1.5));
        hi = (hi.0.max(a.x1 as f32 + 1.5), hi.1.max(a.y1 as f32 + 1.5));
    }
    (lo, hi)
}

/// The bounds of `bare`: the room boxes' own and the site's (with neither, a cell).
pub fn bounds_bare(view: &View) -> (P, P) {
    let mut none = ((f32::INFINITY, f32::INFINITY), (f32::NEG_INFINITY, f32::NEG_INFINITY));
    if let Some((w, h)) = view.boxes.as_ref().and_then(|b| b.site) {
        none = ((-1.5, -1.5), (w as f32 + 1.5, h as f32 + 1.5));
    }
    match hold(none.0, none.1, view) {
        (lo, hi) if lo.0 < hi.0 => (lo, hi),
        _ => ((0.0, 0.0), (1.0, 1.0)),
    }
}

/// The edges of a set of cells, as runs: ((x0, y0), (x1, y1)) on the cells' own lattice.
pub fn edges(cells: &Cells) -> Vec<(Cell, Cell)> {
    // (level, start) of each unit of edge: those along x, then those along y
    let mut along_x: Vec<(i32, i32)> = Vec::new();
    let mut along_y: Vec<(i32, i32)> = Vec::new();
    for &(x, y) in cells {
        if !cells.contains(&(x, y - 1)) {
            along_x.push((y, x));
        }
        if !cells.contains(&(x, y + 1)) {
            along_x.push((y + 1, x));
        }
        if !cells.contains(&(x - 1, y)) {
            along_y.push((x, y));
        }
        if !cells.contains(&(x + 1, y)) {
            along_y.push((x + 1, y));
        }
    }
    let runs = |mut units: Vec<(i32, i32)>| {
        units.sort();
        units.dedup();
        let mut out: Vec<(i32, i32, i32)> = Vec::new();
        for (level, at) in units {
            match out.last_mut() {
                Some(run) if run.0 == level && run.2 == at => run.2 = at + 1,
                _ => out.push((level, at, at + 1)),
            }
        }
        out
    };
    let mut out: Vec<(Cell, Cell)> = runs(along_x).into_iter().map(|(y, a, b)| ((a, y), (b, y))).collect();
    out.extend(runs(along_y).into_iter().map(|(x, a, b)| ((x, a), (x, b))));
    out
}

/// A rectangle in cells: (x0, y0, x1, y1).
type Box2 = (f32, f32, f32, f32);

fn cell_box(r: Rect) -> Box2 {
    (r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32)
}

fn half_box(r: Rect) -> Box2 {
    (r.0 as f32 / 2.0, r.1 as f32 / 2.0, r.2 as f32 / 2.0, r.3 as f32 / 2.0)
}

/// The picture being made.
struct Pic<'a> {
    lv: &'a Level,
    view: &'a View,
    out: Vec<Prim>,
}

/// The few shapes everything is drawn with, onto a list of primitives: the level's picture
/// (`Pic`) and the room boxes, which may have no level under them, both draw with these.
trait Pen {
    fn list(&mut self) -> &mut Vec<Prim>;

    fn rect(&mut self, r: Box2, fill: Rgba) {
        self.list().push(Prim::Poly { pts: vec![(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)], fill });
    }

    fn line(&mut self, a: P, b: P, col: Rgba, width_px: f32) {
        self.list().push(Prim::Line { a, b, col, width_px });
    }

    fn frame(&mut self, r: Box2, col: Rgba, width_px: f32) {
        for (a, b) in [((r.0, r.1), (r.2, r.1)), ((r.2, r.1), (r.2, r.3)), ((r.2, r.3), (r.0, r.3)), ((r.0, r.3), (r.0, r.1))] {
            self.line(a, b, col, width_px);
        }
    }

    fn text(&mut self, at: P, text: &str, col: Rgba, size_px: f32) {
        self.list().push(Prim::Text { at, text: text.to_string(), col, size_px, centred: true });
    }

    /// A line in dashes `on` cells long, `off` apart.
    fn dashed(&mut self, a: P, b: P, col: Rgba, width_px: f32, on: f32, off: f32) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        let mut t = 0.0;
        while t < len {
            let u = (t + on).min(len);
            self.line((a.0 + dx * t / len, a.1 + dy * t / len), (a.0 + dx * u / len, a.1 + dy * u / len), col, width_px);
            t += on + off;
        }
    }

    /// A rectangle's outline in dashes.
    fn dashed_frame(&mut self, r: Box2, col: Rgba, width_px: f32, on: f32, off: f32) {
        for (a, b) in [((r.0, r.1), (r.2, r.1)), ((r.2, r.1), (r.2, r.3)), ((r.2, r.3), (r.0, r.3)), ((r.0, r.3), (r.0, r.1))] {
            self.dashed(a, b, col, width_px, on, off);
        }
    }

    /// An arrow from a to b, its head `head` cells long.
    fn arrow(&mut self, a: P, b: P, col: Rgba, width_px: f32, head: f32) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / len, dy / len);
        self.line(a, b, col, width_px);
        let back = (b.0 - ux * head, b.1 - uy * head);
        self.list().push(Prim::Poly {
            pts: vec![b, (back.0 - uy * head * 0.6, back.1 + ux * head * 0.6), (back.0 + uy * head * 0.6, back.1 - ux * head * 0.6)],
            fill: col,
        });
    }

    /// The cell grid over `lo` to `hi`, every fourth line a little stronger.
    fn grid(&mut self, lo: P, hi: P) {
        let (x0, x1) = (lo.0.ceil() as i32, hi.0.floor() as i32);
        let (y0, y1) = (lo.1.ceil() as i32, hi.1.floor() as i32);
        for x in x0..=x1 {
            self.line((x as f32, lo.1), (x as f32, hi.1), if x.rem_euclid(4) == 0 { colour::GRID_FOURTH } else { colour::GRID }, 1.0);
        }
        for y in y0..=y1 {
            self.line((lo.0, y as f32), (hi.0, y as f32), if y.rem_euclid(4) == 0 { colour::GRID_FOURTH } else { colour::GRID }, 1.0);
        }
    }
}

impl Pen for Vec<Prim> {
    fn list(&mut self) -> &mut Vec<Prim> {
        self
    }
}

impl Pen for Pic<'_> {
    fn list(&mut self) -> &mut Vec<Prim> {
        &mut self.out
    }
}

impl Pic<'_> {
    /// Cells (on a lattice `unit` cells to a step) filled, with their outline.
    fn patch(&mut self, cells: &Cells, unit: f32, fill: Rgba, edge: Rgba, width_px: f32) {
        for r in rectangles(cells) {
            self.rect((r.0 as f32 * unit, r.1 as f32 * unit, r.2 as f32 * unit, r.3 as f32 * unit), fill);
        }
        self.outline(cells, unit, edge, width_px);
    }

    fn outline(&mut self, cells: &Cells, unit: f32, edge: Rgba, width_px: f32) {
        for (a, b) in edges(cells) {
            self.line((a.0 as f32 * unit, a.1 as f32 * unit), (b.0 as f32 * unit, b.1 as f32 * unit), edge, width_px);
        }
    }

    /// Is storey `z` shown?
    fn shown(&self, z: i32) -> bool {
        self.view.storey.is_none_or(|s| s == z)
    }

    /// A door as a bar across its wall.
    fn door(&mut self, s: &Space, side: char, a: f32, width: f32) {
        self.door_in(s, side, a, width, colour::DOOR);
    }

    fn door_in(&mut self, s: &Space, side: char, a: f32, width: f32, col: Rgba) {
        let (x0, y0, x1, y1) = cell_box(s.rect());
        let (p, q) = match side {
            'E' => ((x1, a), (x1, a + width)),
            'W' => ((x0, a), (x0, a + width)),
            'N' => ((a, y1), (a + width, y1)),
            _ => ((a, y0), (a + width, y0)),
        };
        self.line(p, q, col, 4.0);
    }

    /// Everything whose floor is on storey `z`.
    fn storey(&mut self, z: i32) {
        let (lv, view) = (self.lv, self.view);
        let shown = move |q: i32| view.storey.is_none_or(|s| s == q);
        let floor = colour::storey(z);
        // rooms, and what stands in them
        for s in lv.spaces().iter().filter(|s| s.host.is_empty() && s.floor == z) {
            self.rect(cell_box(s.rect()), floor);
            self.frame(cell_box(s.rect()), colour::WALL, 2.0);
            let Some((_, it)) = lv.interiors.iter().find(|(k, _)| *k == s.key) else { continue };
            if !it.pool.is_empty() {
                self.patch(&it.pool, 0.5, colour::POOL, colour::POOL_EDGE, 1.5);
                self.patch(&it.bridge, 0.5, colour::lit(floor, 0.25), colour::WALL, 1.0);
            }
            for &(x0, y0, x1, y1, _, _) in &it.cover {
                self.rect(half_box((x0, y0, x1, y1)), colour::COVER);
                self.frame(half_box((x0, y0, x1, y1)), colour::COVER_EDGE, 1.0);
            }
            for &(x0, y0, x1, y1, _, _, _) in &it.extra_solids {
                self.rect(half_box((x0, y0, x1, y1)), colour::COVER);
                self.frame(half_box((x0, y0, x1, y1)), colour::COVER_EDGE, 1.0);
            }
        }
        // corridors, stubs and enclosed ramps; then vents, which are thin
        for r in lv.net.routes().iter().filter(|r| r.pieces[0].kind != "vent") {
            for pc in &r.pieces {
                if let Some(cells) = pc.cells_at(z) {
                    self.patch(cells, 1.0, colour::lit(floor, 0.22), colour::WALL, 1.5);
                }
                if let (Some((foot, top)), Some(rect)) = (pc.ramp, pc.ramp_cells) {
                    if foot.2 == z || (top.2 == z && !shown(foot.2)) {
                        self.rect(cell_box(rect), colour::mix(colour::storey(foot.2), colour::storey(top.2), 0.5));
                        self.frame(cell_box(rect), colour::WALL, 1.5);
                    }
                }
            }
        }
        for r in lv.net.routes().iter().filter(|r| r.pieces[0].kind == "vent") {
            for w in r.path.windows(2) {
                let (a, b) = (w[0], w[1]);
                // (drawn with its lower end's storey, or its upper end's if that is the one shown)
                let (low, high) = (a.2.min(b.2), a.2.max(b.2));
                if low != z && (high != z || shown(low)) {
                    continue;
                }
                let (p, q) = ((a.0 as f32, a.1 as f32), (b.0 as f32, b.1 as f32));
                let (dx, dy) = ((q.0 - p.0).signum() * 0.2, (q.1 - p.1).signum() * 0.2);
                let (lo, hi) = (
                    (p.0.min(q.0) - dx.abs() - dy.abs(), p.1.min(q.1) - dx.abs() - dy.abs()),
                    (p.0.max(q.0) + dx.abs() + dy.abs(), p.1.max(q.1) + dx.abs() + dy.abs()),
                );
                self.rect((lo.0, lo.1, hi.0, hi.1), colour::lit(floor, -0.45));
                self.line(p, q, colour::VENT, 2.0);
            }
        }
        // decks: the upper floors of rooms that start lower down, and pods' roofs
        for (key, it) in &lv.interiors {
            let s = lv.space(key);
            if s.floor >= z {
                continue;
            }
            if let Some(deck) = it.deck.get(&z).filter(|d| !d.is_empty()) {
                self.patch(deck, 0.5, floor, colour::lit(floor, 0.55), 1.0);
            }
            if let Some(roof) = it.roof.get(&z).filter(|d| !d.is_empty()) {
                self.patch(roof, 0.5, colour::lit(floor, -0.2), colour::lit(floor, 0.55), 1.0);
            }
            for rp in it.ramps.iter().filter(|rp| rp.hi == z || (rp.lo == z && !shown(rp.hi))) {
                self.rect(half_box(rp.rect), colour::mix(colour::storey(rp.lo), colour::storey(rp.hi), 0.5));
                self.frame(half_box(rp.rect), colour::lit(floor, 0.55), 1.0);
            }
            if self.view.storey.is_some() {
                for (a, b, _) in it.rails.iter().filter(|(a, _, _)| a.2 == z) {
                    let cell = |p: &(i32, i32, i32)| (p.0 as f32 / MODULE as f32, p.1 as f32 / MODULE as f32);
                    self.dashed(cell(a), cell(b), colour::RAIL, 2.0, 0.22, 0.28);
                }
            }
        }
        if self.view.storey.is_some() {
            // the rails of rooms whose own floor this is (round a pool, a drop)
            for (key, it) in &lv.interiors {
                if lv.space(key).floor != z {
                    continue;
                }
                for (a, b, _) in it.rails.iter().filter(|(a, _, _)| a.2 == z) {
                    let cell = |p: &(i32, i32, i32)| (p.0 as f32 / MODULE as f32, p.1 as f32 / MODULE as f32);
                    self.dashed(cell(a), cell(b), colour::RAIL, 2.0, 0.22, 0.28);
                }
            }
        }
        // pods: rooms that poke into a hall
        for s in lv.spaces().iter().filter(|s| !s.host.is_empty() && s.floor == z) {
            self.rect(cell_box(s.rect()), colour::lit(floor, -0.25));
            self.frame(cell_box(s.rect()), colour::lit(floor, 0.7), 2.0);
            if let Some(d) = lv.pods.iter().find(|(k, _)| *k == s.key).and_then(|(_, p)| p.door.as_ref()) {
                self.door(s, d.side, d.a as f32, d.width as f32);
            }
        }
        // openings: drops and grates, windows, doors
        for (up, _, r, side) in &lv.drops {
            if lv.space(up).floor != z {
                continue;
            }
            let b = cell_box(*r);
            self.rect(b, colour::DROP);
            self.line((b.0, b.1), (b.2, b.3), colour::DROP_MARK, 1.5);
            self.line((b.0, b.3), (b.2, b.1), colour::DROP_MARK, 1.5);
            // railed on every side but the one stepped off (0 south, 1 east, 2 north, 3 west)
            let sides = [((b.0, b.1), (b.2, b.1)), ((b.2, b.1), (b.2, b.3)), ((b.2, b.3), (b.0, b.3)), ((b.0, b.3), (b.0, b.1))];
            for (n, (p, q)) in sides.into_iter().enumerate() {
                if *side != Some(n) {
                    self.line(p, q, colour::DROP_MARK, 2.0);
                }
            }
        }
        if self.view.storey.is_some() {
            // where a drop lands, and what a grate lights
            let lands = lv.drops.iter().map(|(_, low, r, _)| (low, r)).chain(lv.grates.iter().map(|(_, low, r, _)| (low, r)));
            for (_, r) in lands.filter(|(low, _)| lv.space(low).floor == z) {
                let b = cell_box(*r);
                for (p, q) in [((b.0, b.1), (b.2, b.1)), ((b.2, b.1), (b.2, b.3)), ((b.2, b.3), (b.0, b.3)), ((b.0, b.3), (b.0, b.1))] {
                    self.dashed(p, q, colour::DROP_MARK, 1.5, 0.3, 0.25);
                }
            }
        }
        for (_, _, r, gz) in &lv.grates {
            if *gz != z {
                continue;
            }
            let b = cell_box(*r);
            self.rect(b, colour::GRATE);
            let mut t = 0.25;
            while t < (b.2 - b.0) + (b.3 - b.1) {
                // hatching: lines of slope -1 across the rectangle
                let p = (b.0 + t.min(b.2 - b.0), b.1 + (t - (b.2 - b.0)).max(0.0));
                let q = (b.0 + (t - (b.3 - b.1)).max(0.0), b.1 + t.min(b.3 - b.1));
                self.line(p, q, colour::GRATE_MARK, 1.0);
                t += 0.25;
            }
            self.frame(b, colour::GRATE_MARK, 1.5);
        }
        for w in lv.windows.iter().filter(|w| w.z == z && !w.wall.is_empty()) {
            let (dx, _) = DIRS[w.d];
            let least = |f: fn(&Cell) -> i32| w.wall.iter().map(f).min().unwrap_or(0) as f32;
            let most = |f: fn(&Cell) -> i32| w.wall.iter().map(f).max().unwrap_or(0) as f32;
            let (x0, y0, x1, y1) = (least(|c| c.0), least(|c| c.1), most(|c| c.0) + 1.0, most(|c| c.1) + 1.0);
            let (p, q) = if dx != 0 { (((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1)) } else { ((x0, (y0 + y1) / 2.0), (x1, (y0 + y1) / 2.0)) };
            self.line(p, q, if w.kind == "glass" { colour::GLASS } else { colour::BALCONY }, 4.0);
        }
        for s in lv.spaces().iter().filter(|s| s.host.is_empty()) {
            for d in lv.net.doors(&s.key).iter().filter(|d| d.level == z) {
                self.door(s, d.side, d.a as f32, d.width as f32);
            }
        }
    }

    /// Uphill arrows: the open ramps inside rooms, the enclosed ones of corridors, and vents' slopes.
    fn arrows(&mut self) {
        let lv = self.lv;
        let touches = |lo: i32, hi: i32| self.view.storey.is_none_or(|s| s == lo || s == hi);
        let mut todo: Vec<(P, P)> = Vec::new();
        for (_, it) in &lv.interiors {
            for rp in it.ramps.iter().filter(|rp| touches(rp.lo, rp.hi)) {
                let ((fx, fy), (tx, ty)) = rp.line();
                todo.push(((fx as f32, fy as f32), (tx as f32, ty as f32)));
            }
        }
        for r in lv.net.routes() {
            for pc in &r.pieces {
                let slopes = pc.ramp.into_iter().chain(pc.ducts.iter().map(|(line, _)| *line));
                for (foot, top) in slopes.filter(|(foot, top)| touches(foot.2, top.2)) {
                    todo.push(((foot.0 as f32, foot.1 as f32), (top.0 as f32, top.1 as f32)));
                }
            }
        }
        for (a, b) in todo {
            // (a little short of each end, so arrows nose to tail stay apart)
            let (dx, dy) = ((b.0 - a.0).signum() * 0.3, (b.1 - a.1).signum() * 0.3);
            self.arrow((a.0 + dx, a.1 + dy), (b.0 - dx, b.1 - dy), colour::RAMP_ARROW, 2.5, 0.7);
        }
        // the rescue's bridges, outlined over their deck; its ladders: the ladder on the deck's
        // edge, and an arrow up it from its foot
        for (_, it) in &lv.interiors {
            for (_, cells) in it.crossings.iter().filter(|(z, _)| touches(*z, *z)) {
                self.outline(cells, 0.5, colour::LADDER, 2.5);
            }
            for c in it.chutes.iter().filter(|c| touches(c.foot.2, c.top.2)) {
                let ((ax, ay), (bx, by), _) = c.line();
                self.line((ax as f32, ay as f32), (bx as f32, by as f32), colour::LADDER, 5.0);
                let ((fx, fy, _), (tx, ty, _)) = c.ends();
                self.arrow((fx as f32, fy as f32), (tx as f32, ty as f32), colour::LADDER, 2.5, 0.5);
            }
        }
    }

    /// A room that is only air on the storey shown: hollow, with its outline.
    fn hollow(&mut self, s: &Space) {
        self.rect(cell_box(s.rect()), colour::VOID);
        let b = cell_box(s.rect());
        for (p, q) in [((b.0, b.1), (b.2, b.1)), ((b.2, b.1), (b.2, b.3)), ((b.2, b.3), (b.0, b.3)), ((b.0, b.3), (b.0, b.1))] {
            self.dashed(p, q, colour::lit(colour::storey(s.floor), 0.2), 1.5, 0.5, 0.3);
        }
    }

    /// Every room's and corridor's outline again, thin, over whatever lies on top of it.
    fn outlines(&mut self) {
        let lv = self.lv;
        for s in lv.spaces() {
            self.frame(cell_box(s.rect()), colour::lit(colour::storey(s.floor), 0.45), 1.0);
        }
        for r in lv.net.routes().iter().filter(|r| r.pieces[0].kind != "vent") {
            for pc in &r.pieces {
                for (z, cells) in &pc.cells {
                    self.outline(cells, 1.0, colour::lit(colour::storey(*z), 0.45), 1.0);
                }
            }
        }
    }

    /// Does the text fit in this many cells at this size?
    fn fits(&self, text: &str, size_px: f32, cells: f32) -> bool {
        let dot = (size_px / 8.0).round().max(1.0);
        text.chars().count() as f32 * 6.0 * dot + 6.0 <= cells * self.view.scale
    }

    /// Names and keys: rooms by their names, keys and heights, routes by their keys.
    fn labels(&mut self) {
        let lv = self.lv;
        let line = 11.0 / self.view.scale; // cells between lines of small text
        for s in lv.spaces() {
            let (x0, y0, x1, y1) = cell_box(s.rect());
            let (mid, w, h) = (((x0 + x1) / 2.0, (y0 + y1) / 2.0), x1 - x0, y1 - y0);
            let here = self.shown(s.floor);
            let air = s.host.is_empty() && self.view.storey.is_some_and(|z| s.storeys().contains(&z));
            if !(here || air) {
                continue;
            }
            if !here {
                self.text(mid, &format!("{} (floor {})", s.key, s.floor), colour::FAINT, 8.0);
                continue;
            }
            let name = lv.plan.get(&s.key).map_or("", |r| r.name.as_str());
            let heights = format!("{} to {}", s.floor, s.top);
            let both = format!("{}  {heights}", s.key);
            if !s.host.is_empty() {
                self.text(mid, &s.key, colour::KEY, 8.0);
            } else if self.fits(name, 16.0, w) && h * self.view.scale >= 70.0 {
                self.text((mid.0, mid.1 + line * 0.9), name, colour::NAME, 16.0);
                self.text((mid.0, mid.1 - line * 0.9), &both, colour::KEY, 8.0);
            } else if self.fits(name, 8.0, w) && self.fits(&both, 8.0, w) && h * self.view.scale >= 44.0 {
                self.text((mid.0, mid.1 + line / 2.0), name, colour::NAME, 8.0);
                self.text((mid.0, mid.1 - line / 2.0), &both, colour::KEY, 8.0);
            } else if self.fits(&s.key, 8.0, w) && self.fits(&heights, 8.0, w) && h * self.view.scale >= 30.0 {
                self.text((mid.0, mid.1 + line / 2.0), &s.key, colour::NAME, 8.0);
                self.text((mid.0, mid.1 - line / 2.0), &heights, colour::KEY, 8.0);
            } else {
                self.text(mid, &s.key, colour::NAME, 8.0);
            }
        }
        for r in lv.net.routes() {
            // the middle of its longest level run that is shown, or of the whole of it
            let runs = r.path.windows(2).filter(|w| w[0].2 == w[1].2 && self.shown(w[0].2));
            let best = runs.max_by(|a, b| {
                let len = |w: &&[autolevel::route::Point]| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs();
                len(a).total_cmp(&len(b)).then(std::cmp::Ordering::Greater)
            });
            let ends = match best {
                Some(w) => Some((w[0], w[1])),
                None if r.path.iter().any(|p| self.shown(p.2)) => r.path.first().copied().zip(r.path.last().copied()),
                None => None,
            };
            let Some((a, b)) = ends else { continue };
            let text = if r.switchback() { format!("{} sw", r.key()) } else { r.key().to_string() };
            self.text((((a.0 + b.0) / 2.0) as f32, ((a.1 + b.1) / 2.0) as f32), &text, colour::ROUTE_KEY, 8.0);
        }
    }

    /// What was chosen for a room by hand (`View::chosen`), in small letters by the room's own
    /// words: in lines that fit across the room, at the height nearest its middle (under it
    /// before over it) where they lie on no other words and on none of its pods. Where they
    /// will not go whole, the one word "chosen"; where that will not go either, nothing (the
    /// list has it). A room that is only air on the storey shown has them too: a layout is its
    /// upper storey's.
    fn chosen(&mut self) {
        let (lv, view) = (self.lv, self.view);
        let (px, line) = (1.0 / view.scale, 11.0 / view.scale);
        for (room, words) in &view.chosen {
            let on = |s: &&Space| self.shown(s.floor) || (s.host.is_empty() && view.storey.is_some_and(|z| s.storeys().contains(&z)));
            let Some(s) = lv.spaces().iter().filter(|s| s.key == *room).find(on) else { continue };
            let r = cell_box(s.rect());
            let mid = ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
            let letters = (((r.2 - r.0) * view.scale - 8.0) / 6.0).floor().max(0.0) as usize;
            // what the words may not lie on: every word drawn so far, and the room's pods
            let mut taken: Vec<Box2> = self.out.iter().filter_map(|p| text_box(p, px)).collect();
            let pod = |p: &Space| (p.x0 as f32 - 4.0 * px, p.y0 as f32 - 4.0 * px, p.x1 as f32 + 4.0 * px, p.y1 as f32 + 4.0 * px);
            taken.extend(lv.spaces().iter().filter(|p| p.host == s.key).map(pod));
            let fits = |lines: &[String], top: f32| {
                lines.iter().enumerate().all(|(n, l)| {
                    // (a letter and a half clear of the words beside it, or they read as one line)
                    let (half, y) = ((l.chars().count() as f32 * 3.0 + 9.0) * px, top - n as f32 * line);
                    let b = (mid.0 - half, y - 5.0 * px, mid.0 + half, y + 5.0 * px);
                    r.1 + 2.0 * px <= b.1 && b.3 <= r.3 - 2.0 * px && taken.iter().all(|t| t.2 <= b.0 || b.2 <= t.0 || t.3 <= b.1 || b.3 <= t.1)
                })
            };
            let steps = ((r.3 - r.1) / line).ceil() as i32;
            let tops = (0..=steps).flat_map(|k| [mid.1 - k as f32 * line / 2.0, mid.1 + k as f32 * line / 2.0]);
            for lines in [wrap(words, letters), vec!["chosen".to_string()]] {
                if lines.iter().any(|l| l.chars().count() > letters) {
                    continue;
                }
                if let Some(top) = tops.clone().find(|top| fits(&lines, *top)) {
                    for (n, l) in lines.iter().enumerate() {
                        self.text((mid.0, top - n as f32 * line), l, colour::CHOSEN, 8.0);
                    }
                    break;
                }
            }
        }
    }

    /// Outline what a mark holds.
    fn ring(&mut self, m: &Mark, col: Rgba, width_px: f32) {
        let (lv, view) = (self.lv, self.view);
        let shown = move |q: i32| view.storey.is_none_or(|s| s == q);
        for s in lv.spaces().iter().filter(|s| m.rooms.contains(&s.key)) {
            if self.shown(s.floor) || self.view.storey.is_some_and(|z| s.storeys().contains(&z)) {
                let b = cell_box(s.rect());
                self.frame((b.0 - 0.15, b.1 - 0.15, b.2 + 0.15, b.3 + 0.15), col, width_px);
            }
        }
        for (room, z) in m.decks.iter().filter(|(_, z)| shown(*z)) {
            let Some((_, it)) = lv.interiors.iter().find(|(k, _)| k == room) else { continue };
            if let Some(deck) = it.deck.get(z).filter(|d| !d.is_empty()) {
                self.outline(deck, 0.5, col, width_px);
            }
        }
        for r in lv.net.routes().iter().filter(|r| m.routes.iter().any(|k| k == r.key())) {
            for pc in &r.pieces {
                for (z, cells) in &pc.cells {
                    if self.shown(*z) {
                        self.outline(cells, 1.0, col, width_px);
                    }
                }
                if let Some(rect) = pc.ramp_cells {
                    self.frame(cell_box(rect), col, width_px);
                }
                for (_, rect) in &pc.ducts {
                    self.frame(cell_box(*rect), col, width_px);
                }
            }
        }
    }

    /// A route that is not in the level: its floor in `fill` with `edge` round it (on a storey
    /// that is not shown, the edge alone), its ramps with their arrows, a vent as a thin strip.
    fn ghost(&mut self, r: &Route, fill: Rgba, edge: Rgba, width_px: f32) {
        if r.pieces[0].kind == "vent" {
            for w in r.path.windows(2) {
                if !(self.shown(w[0].2) || self.shown(w[1].2)) {
                    continue;
                }
                let (p, q) = ((w[0].0 as f32, w[0].1 as f32), (w[1].0 as f32, w[1].1 as f32));
                self.rect((p.0.min(q.0) - 0.25, p.1.min(q.1) - 0.25, p.0.max(q.0) + 0.25, p.1.max(q.1) + 0.25), fill);
                self.line(p, q, edge, width_px + 1.0);
            }
            return;
        }
        for pc in &r.pieces {
            for (z, cells) in &pc.cells {
                if self.shown(*z) {
                    self.patch(cells, 1.0, fill, edge, width_px);
                } else {
                    self.outline(cells, 1.0, colour::mix(edge, colour::BACKGROUND, 0.5), 1.0);
                }
            }
            if let (Some((foot, top)), Some(rect)) = (pc.ramp, pc.ramp_cells) {
                if self.shown(foot.2) || self.shown(top.2) {
                    self.rect(cell_box(rect), fill);
                }
                self.frame(cell_box(rect), edge, width_px);
                let (a, b) = ((foot.0 as f32, foot.1 as f32), (top.0 as f32, top.1 as f32));
                let (dx, dy) = ((b.0 - a.0).signum() * 0.3, (b.1 - a.1).signum() * 0.3);
                self.arrow((a.0 + dx, a.1 + dy), (b.0 - dx, b.1 - dy), edge, 2.0, 0.6);
            }
        }
    }

    /// The room picked to join from, and the corridors that could be made from it: ghosts, the
    /// door each would make at either end, and a numbered disc on each, clear of the others'.
    fn joins(&mut self) {
        let (lv, view) = (self.lv, self.view);
        if let Some((room, z)) = &view.pick {
            if let Some(s) = lv.spaces().iter().find(|s| s.key == *room) {
                let b = cell_box(s.rect());
                self.frame((b.0 - 0.25, b.1 - 0.25, b.2 + 0.25, b.3 + 0.25), colour::PICK, 3.0);
                self.text(((b.0 + b.2) / 2.0, b.1 - 0.25 - 8.0 / view.scale), &format!("joining {room} by a door on {z}"), colour::PICK, 8.0);
            }
        }
        // the one under the pointer last, so it lies over the others
        let mut order: Vec<&(usize, Candidate)> = view.joins.iter().collect();
        order.sort_by_key(|(n, _)| view.hot == Some(*n));
        for (n, c) in &order {
            let (fill, edge) = match view.hot {
                Some(hot) if hot == *n => (colour::HOT, colour::MARK),
                Some(_) => (colour::JOIN_DIM, colour::JOIN_DIM_EDGE),
                None => (colour::JOIN, colour::JOIN_EDGE),
            };
            self.ghost(&c.route, fill, edge, 2.0);
            for d in [&c.route.a, &c.route.b] {
                if let Some(s) = lv.spaces().iter().find(|s| s.key == d.room) {
                    self.door_in(s, d.side, d.a as f32, d.width as f32, edge);
                }
            }
        }
        // a disc big enough for two digits, each somewhere along its own route where no other
        // is (placed in the list's order, so pointing at one moves none of them)
        let big = view.scale >= 18.0;
        // (a little wider once the numbers run to two digits)
        let wide = if view.joins.iter().any(|(n, _)| *n >= 10) { 2.0 } else { 0.0 };
        let r_px = if big { 13.0 } else { 8.0 } + wide;
        let r = r_px / view.scale;
        let mut placed: Vec<P> = Vec::new();
        for (n, c) in &view.joins {
            let clear = |p: P, placed: &[P]| placed.iter().all(|q| (p.0 - q.0).abs().max((p.1 - q.1).abs()) >= 2.1 * r);
            let spots = [0.5, 0.35, 0.65, 0.2, 0.8, 0.05, 0.95].map(|f| along(&c.route.path, f));
            // (a stub is one point: step off it sideways instead)
            let aside = (1..6).map(|k| (spots[0].0 + 2.2 * r * k as f32, spots[0].1));
            let at = spots.into_iter().chain(aside).find(|p| clear(*p, &placed)).unwrap_or(spots[0]);
            placed.push(at);
            let fill = if view.hot == Some(*n) { colour::MARK } else { colour::JOIN_DISC };
            self.out.push(Prim::Badge { at, text: n.to_string(), fill, col: colour::JOIN_NUMBER, r_px, size_px: if big { 16.0 } else { 8.0 } });
        }
    }

    /// What is asked of the picked room's joins, over everything (the room boxes too): the
    /// places on its walls a doorway can be asked for and the one asked for, the room asked for
    /// framed with its doorway, and why nothing fits through them, in red.
    fn asked(&mut self) {
        let (lv, view) = (self.lv, self.view);
        let Some(s) = view.pick.as_ref().and_then(|(room, _)| lv.spaces().iter().find(|s| s.key == *room)) else { return };
        self.doorways(s, view.asked.door);
        if let Some(t) = view.asked.to.as_ref().and_then(|to| lv.spaces().iter().find(|s| s.key == *to)) {
            let b = cell_box(t.rect());
            self.frame((b.0 - 0.25, b.1 - 0.25, b.2 + 0.25, b.3 + 0.25), colour::HANDLE, 2.0);
            self.text(((b.0 + b.2) / 2.0, b.1 - 0.25 - 8.0 / view.scale), &format!("to {} alone", t.key), colour::PICK, 8.0);
            self.doorways(t, view.asked.to_door);
        }
        if let Some(why) = &view.none {
            let b = cell_box(s.rect());
            let at = ((b.0 + b.2) / 2.0, b.1 - 0.25 - 20.0 / view.scale);
            let w = why.chars().count() as f32 * 6.0 / view.scale / 2.0 + 0.2;
            self.rect((at.0 - w, at.1 - 6.0 / view.scale, at.0 + w, at.1 + 6.0 / view.scale), colour::PANEL);
            self.text(at, why, colour::DEAD_END, 8.0);
        }
    }

    /// The places on a room's walls a doorway can be asked for: a short tick outside the wall at
    /// each cell's middle (where a cell is big enough to see one), and the doorway asked for, a
    /// cell of wall drawn thick and named.
    fn doorways(&mut self, s: &Space, asked: Option<Doorway>) {
        let (x0, y0, x1, y1) = cell_box(s.rect());
        let out = 0.25;
        if self.view.scale >= 8.0 {
            for x in s.x0..s.x1 {
                let m = x as f32 + 0.5;
                self.line((m, y1 + out * 0.4), (m, y1 + out), colour::HANDLE, 2.0);
                self.line((m, y0 - out * 0.4), (m, y0 - out), colour::HANDLE, 2.0);
            }
            for y in s.y0..s.y1 {
                let m = y as f32 + 0.5;
                self.line((x1 + out * 0.4, m), (x1 + out, m), colour::HANDLE, 2.0);
                self.line((x0 - out * 0.4, m), (x0 - out, m), colour::HANDLE, 2.0);
            }
        }
        let Some(d) = asked else { return };
        let a = d.at as f32;
        // (the words inside the room, where its corridor is not)
        let (p, q, label) = match d.side {
            'E' => ((x1, a), (x1, a + 1.0), (x1 - 1.6, a + 0.5)),
            'W' => ((x0, a), (x0, a + 1.0), (x0 + 1.6, a + 0.5)),
            'N' => ((a, y1), (a + 1.0, y1), (a + 0.5, y1 - 0.6)),
            _ => ((a, y0), (a + 1.0, y0), (a + 0.5, y0 + 0.6)),
        };
        self.line(p, q, colour::ASKED, 7.0);
        self.text(label, &format!("door {}", d.short()), colour::ASKED, 8.0);
    }

    /// A route just taken out, where it was.
    fn gone(&mut self, r: &Route) {
        self.ghost(r, colour::GONE, colour::DEAD_END, 3.0);
        let at = along(&r.path, 0.5);
        let size = if self.view.scale >= 18.0 { 16.0 } else { 8.0 };
        self.text((at.0, at.1 + 0.5 + size * 0.7 / self.view.scale), &format!("{} taken out", r.key()), colour::DEAD_END, size);
    }

    /// The level's dead ends: the allowed ones ringed, the others boxed and named, loudly.
    fn dead_ends(&mut self) {
        let lv = self.lv;
        for (n, e) in lv.dead_ends.iter().enumerate() {
            let m = Mark::of(e, lv.net.routes());
            let (at, z) = port(e);
            if e.allowed {
                self.ring(&Mark { decks: Vec::new(), ..m }, colour::CAMP, 1.5);
                if self.shown(z) {
                    self.out.push(Prim::Circle { c: at, r: 0.42, col: colour::CAMP, filled: false });
                    let what = if e.rooms.is_empty() { "deck" } else { "camp" };
                    self.text((at.0, at.1 - 0.85), &format!("{what} {}", n + 1), colour::CAMP, 8.0);
                }
            } else {
                self.ring(&m, colour::DEAD_END, 4.0);
                self.out.push(Prim::Circle { c: at, r: 0.6, col: colour::DEAD_END, filled: true });
                self.text((at.0, at.1 - 1.2), &format!("DEAD END {}", n + 1), colour::DEAD_END, 16.0);
            }
        }
    }
}

/// The plan of a level, as `view` asks for it.
pub fn picture(lv: &Level, view: &View) -> Picture {
    let (lo, hi) = bounds_of(lv, view);
    let mut pic = Pic { lv, view, out: Vec::new() };
    pic.grid(lo, hi);
    match view.storey {
        None => {
            for z in LEVELS {
                pic.storey(z);
            }
            pic.outlines();
        }
        Some(z) => {
            for s in lv.spaces().iter().filter(|s| s.host.is_empty() && s.floor != z && s.storeys().contains(&z)) {
                pic.hollow(s);
            }
            pic.storey(z);
        }
    }
    pic.arrows();
    pic.labels();
    pic.dead_ends();
    pic.chosen();
    if let Some(m) = &view.mark {
        pic.ring(m, colour::MARK, 3.0);
        if let Some((at, _)) = m.at {
            pic.out.push(Prim::Circle { c: at, r: 0.75, col: colour::MARK, filled: false });
        }
    }
    if let Some(key) = &view.route {
        pic.ring(&Mark { routes: vec![key.clone()], ..Mark::default() }, colour::MARK, 4.0);
    }
    if let Some(r) = &view.gone {
        pic.gone(r);
    }
    pic.joins();
    lots(&mut pic.out, view, Some(lv), (lo, hi));
    pic.asked();
    Picture { prims: pic.out, lo, hi, legend: legend(lv, view) }
}

/// Where a drawn word or badge lies, in cells, with a pixel `px` cells wide: its box as the
/// snapshot letters it (6 pixels a letter, 8 a line, at size 8). None for a shape.
fn text_box(p: &Prim, px: f32) -> Option<Box2> {
    match p {
        Prim::Text { at, text, size_px, centred, .. } => {
            let dot = (size_px / 8.0).round().max(1.0);
            let (w, h) = (text.chars().count() as f32 * 6.0 * dot * px, 8.0 * dot * px);
            let x0 = if *centred { at.0 - w / 2.0 } else { at.0 };
            Some((x0, at.1 - h / 2.0, x0 + w, at.1 + h / 2.0))
        }
        Prim::Badge { at, r_px, .. } => Some((at.0 - r_px * px, at.1 - r_px * px, at.0 + r_px * px, at.1 + r_px * px)),
        _ => None,
    }
}

/// The plan where there is no level to draw (the recipe makes none, or none yet): the grid and
/// the room boxes of `view.boxes` alone, each with its key and heights, in bounds that hold them.
pub fn bare(view: &View) -> Picture {
    let (lo, hi) = bounds_bare(view);
    let mut out = Vec::new();
    out.grid(lo, hi);
    let site = view.boxes.as_ref().and_then(|b| b.site);
    if let Some((w, h)) = site {
        out.dashed_frame((0.0, 0.0, w as f32, h as f32), colour::FAINT, 1.5, 0.6, 0.4);
    }
    lots(&mut out, view, None, (lo, hi));
    let on = |z: &i32| view.boxes.iter().flat_map(|b| &b.lots).any(|a| a.floor == *z) && view.storey.is_none_or(|s| s == *z);
    let floor = |z: i32| Key { swatch: Swatch::Fill, col: colour::storey(z), text: format!("floor {z}") };
    let mut legend: Vec<Key> = LEVELS.into_iter().filter(on).map(floor).collect();
    if let Some((w, h)) = site {
        legend.push(Key { swatch: Swatch::Line, col: colour::FAINT, text: format!("the site, {w} x {h} cells: rooms stand inside it") });
    }
    legend.extend(box_keys(view, None));
    Picture { prims: out, lo, hi, legend }
}

/// How a lot stands to the level on show: None if the level has that room there, or else the
/// word its box is given: "new", "moved", "resized", "floor was 276", "top was 512" (several,
/// with commas), or "left out" for a room the generator itself left out of the level.
pub fn differs(lv: &Level, a: &Lot) -> Option<String> {
    let Some(s) = lv.spaces().iter().find(|s| s.key == a.key) else {
        let out =
            lv.dropped.contains(&a.key) || lv.net.cut_off.contains(&a.key) || lv.fixes.iter().any(|f| matches!(f, Fix::Dropped { room } if *room == a.key));
        return Some(if out { "left out" } else { "new" }.to_string());
    };
    if s.base.is_some() {
        // a base: its column of storeys as the generator splits it (`autolevel::bases::split`),
        // each under another a cell narrower: the lot stands as drawn
        return None;
    }
    let mut said: Vec<String> = Vec::new();
    if (s.x0, s.y0) != (a.x0, a.y0) {
        said.push("moved".to_string());
    }
    if (s.x1 - s.x0, s.y1 - s.y0) != (a.x1 - a.x0, a.y1 - a.y0) {
        said.push("resized".to_string());
    }
    // (a room's top is its lot's on the nearest ceiling line)
    if s.floor != a.floor {
        said.push(format!("floor was {}", s.floor));
    } else if s.top != snap_top(a.top) {
        said.push(format!("top was {}", s.top));
    }
    (!said.is_empty()).then(|| said.join(", "))
}

/// The rooms of the level on show that have no lot any more.
pub fn deleted<'a>(lv: &'a Level, lots: &[Lot]) -> Vec<&'a Space> {
    // (a base's upper storeys are its column's lot's: `Space::base`)
    fn column(s: &Space) -> &str {
        s.base.as_ref().map_or(s.key.as_str(), |b| b.column.as_str())
    }
    lv.spaces().iter().filter(|s| lots.iter().all(|a| a.key != column(s))).collect()
}

/// How a box from `floor` to `top` shows with one storey on show, or all of them: its floor is
/// there (true), it is only air there (false: drawn dashed), or it is left out (None). A pod is
/// its floor alone. The same boxes `hit_box` takes hold of.
fn shows(floor: i32, top: i32, pod: bool, storey: Option<i32>) -> Option<bool> {
    match storey {
        None => Some(true),
        Some(z) if floor == z => Some(true),
        Some(z) if !pod && floor < z && z < top => Some(false),
        Some(_) => None,
    }
}

/// `text` in lines of at most `width` letters, broken between words (a longer word has a line
/// to itself).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match out.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => out.push(word.to_string()),
        }
    }
    out
}

/// The side of a grip of the selected box, in pixels.
pub const GRIP_PX: f32 = 11.0;
/// The letters to a line of a refusal.
const WRAP: usize = 46;

/// Where a block `w` by `h` cells goes beside the box `b` so that it does not lie on it and
/// stays inside `lo` to `hi`: its west and north edges. Over the box if there is room, or else
/// under it, east of it, west of it; with room nowhere, under it as far as the picture goes.
fn beside(b: Box2, (w, h): P, gap: f32, lo: P, hi: P) -> P {
    let within = |v: f32, least: f32, most: f32| v.min(most).max(least);
    let x = within((b.0 + b.2 - w) / 2.0, lo.0, hi.0 - w);
    let y = within((b.1 + b.3 + h) / 2.0, lo.1 + h, hi.1);
    if b.3 + gap + h <= hi.1 {
        (x, b.3 + gap + h)
    } else if b.1 - gap - h >= lo.1 {
        (x, b.1 - gap)
    } else if b.2 + gap + w <= hi.0 {
        (b.2 + gap, y)
    } else if b.0 - gap - w >= lo.0 {
        (b.0 - gap - w, y)
    } else {
        (x, within(b.1 - gap, lo.1 + h, hi.1))
    }
}

/// The room boxes of `view.boxes` over whatever is drawn already: the level `lv`, which may be
/// older than the lots, or nothing at all (`bare`). The bounds are the picture's, which the
/// selected box's words are kept inside.
fn lots(out: &mut Vec<Prim>, view: &View, lv: Option<&Level>, (lo, hi): (P, P)) {
    let Some(b) = &view.boxes else { return };
    let px = 1.0 / view.scale; // a pixel, in cells
    let fits = |text: &str, cells: f32| text.chars().count() as f32 * 6.0 + 6.0 <= cells * view.scale;
    let line = 11.0 * px; // cells between lines of small text
    let mid = |r: Box2| ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
    if let Some(lv) = lv {
        // (the level's own words for a room go where its box says something else of it: the
        // box names it again, as deleted, moved or resized)
        let unname = |out: &mut Vec<Prim>, s: &Space| {
            let r = cell_box(s.rect());
            let name = lv.plan.get(&s.key).map_or("", |room| room.name.as_str());
            let its = |text: &str| text == name || text == s.key || text.starts_with(&format!("{} ", s.key)) || text == format!("{} to {}", s.floor, s.top);
            // (and what was chosen for it, which is written under those)
            let inside = |at: &P| r.0 < at.0 && at.0 < r.2 && r.1 < at.1 && at.1 < r.3;
            out.retain(|p| !matches!(p, Prim::Text { at, text, col, .. } if inside(at) && (its(text) || *col == colour::CHOSEN)));
        };
        // a room the level draws that has no lot any more: crossed out
        for s in deleted(lv, &b.lots) {
            if shows(s.floor, s.top, !s.host.is_empty(), view.storey).is_none() {
                continue;
            }
            let r = cell_box(s.rect());
            out.rect(r, colour::GONE);
            out.frame(r, colour::DEAD_END, 2.0);
            out.line((r.0, r.1), (r.2, r.3), colour::DEAD_END, 2.0);
            out.line((r.0, r.3), (r.2, r.1), colour::DEAD_END, 2.0);
            // (a pod that goes with its hall keeps the level's word for it under its cross: the
            // hall says what happened)
            let pods: Vec<Box2> = lv.spaces().iter().filter(|p| p.host == s.key).map(|p| cell_box(p.rect())).collect();
            if deleted(lv, &b.lots).iter().any(|hall| hall.key == s.host) {
                continue;
            }
            unname(out, s);
            let said = format!("{} deleted", s.key);
            if said.chars().count() as f32 * 12.0 + 12.0 <= (r.2 - r.0) * view.scale && (r.3 - r.1) * view.scale >= 70.0 {
                // (large, as a hall's name is, and at a height where none of its pods is)
                let half = (said.chars().count() as f32 * 6.0 * px, 9.0 * px);
                let at = |f: f32| (mid(r).0, r.1 + (r.3 - r.1) * f);
                let clear = |c: P, p: &Box2| p.2 <= c.0 - half.0 || c.0 + half.0 <= p.0 || p.3 <= c.1 - half.1 || c.1 + half.1 <= p.1;
                let free = |f: &f32| pods.iter().all(|p| clear(at(*f), p));
                out.text(at([0.5, 0.25, 0.75, 0.12, 0.88].into_iter().find(free).unwrap_or(0.5)), &said, colour::REFUSAL, 16.0);
            } else if fits(&said, r.2 - r.0) {
                out.text(mid(r), &said, colour::REFUSAL, 8.0);
            } else {
                out.text((mid(r).0, mid(r).1 + line / 2.0), &s.key, colour::REFUSAL, 8.0);
                out.text((mid(r).0, mid(r).1 - line / 2.0), "deleted", colour::REFUSAL, 8.0);
            }
        }
        // where the level still draws a room whose box is elsewhere now: dimmed, with a line
        // to where the box is
        for a in &b.lots {
            let Some(s) = lv.spaces().iter().find(|s| s.key == a.key) else { continue };
            if differs(lv, a).is_none() || shows(s.floor, s.top, !s.host.is_empty(), view.storey).is_none() {
                continue;
            }
            unname(out, s);
            let (was, now) = (cell_box(s.rect()), cell_box(a.rect()));
            if was != now {
                out.rect(was, colour::WAS);
                out.dashed(mid(was), mid(now), colour::CHANGED, 1.5, 0.35, 0.3);
            }
        }
    }
    // the boxes, the lowest first and a pod after its hall, so each lies over what is under it
    let mut order: Vec<&Lot> = b.lots.iter().collect();
    order.sort_by_key(|a| (a.floor, !a.host.is_empty()));
    let selected = b.selected.as_deref().and_then(|key| b.lots.iter().find(|a| a.key == key));
    for a in order {
        let picked = selected.is_some_and(|s| s.key == a.key);
        // (the selected box is drawn whatever storey is shown: its grips are there to take)
        let Some(floor) = shows(a.floor, a.top, !a.host.is_empty(), view.storey).or(picked.then_some(false)) else { continue };
        let r = cell_box(a.rect());
        let (w, h) = (r.2 - r.0, r.3 - r.1);
        let col = colour::storey(a.floor);
        let word = lv.and_then(|lv| differs(lv, a));
        if !floor {
            out.dashed_frame(r, colour::box_edge(a.floor), 2.0, 0.5, 0.3);
            continue;
        }
        // a box the level under it does not show there is filled in as a room is (not quite
        // solid: the rooms over and under it are still to be seen); one the level does show is
        // that room with a tint and an edge to take hold of
        let fill = match (lv, &word) {
            (None, _) => 222,
            (Some(_), Some(_)) => 170,
            (Some(_), None) => 56,
        };
        out.rect(r, colour::faded(col, fill));
        let edge = if word.is_some() { colour::CHANGED } else { colour::box_edge(a.floor) };
        out.frame(r, edge, if word.is_some() { 3.0 } else { 2.0 });
        // (thick at its corners, as a thing to take hold of is: the level's own rooms are not)
        let tick = 0.6f32.min(w / 3.0).min(h / 3.0);
        for (cx, cy, sx, sy) in [(r.0, r.1, 1.0, 1.0), (r.2, r.1, -1.0, 1.0), (r.2, r.3, -1.0, -1.0), (r.0, r.3, 1.0, -1.0)] {
            out.line((cx, cy), (cx + sx * tick, cy), edge, 4.0);
            out.line((cx, cy), (cx, cy + sy * tick), edge, 4.0);
        }
        if picked {
            continue;
        }
        // its words: none where the level under it has them already
        let heights = format!("{} to {}", a.floor, snap_top(a.top));
        let (at, tall) = (mid(r), h * view.scale >= 30.0);
        match (&word, lv) {
            (Some(word), _) => {
                // (its key and why it stands out, then its heights if there is room: the level
                // under it says nothing of it here)
                let both = format!("{} {word}", a.key);
                let mut said: Vec<&str> = if fits(&both, w) || !tall { vec![&both] } else { vec![&a.key, word] };
                if h * view.scale >= 30.0 + 11.0 * said.len() as f32 {
                    said.push(&heights);
                }
                for (n, text) in said.iter().enumerate() {
                    out.text((at.0, at.1 + line * ((said.len() - 1) as f32 / 2.0 - n as f32)), text, colour::CHANGED, 8.0);
                }
            }
            (None, None) if a.host.is_empty() && tall && fits(&heights, w) => {
                out.text((at.0, at.1 + line / 2.0), &a.key, colour::NAME, 8.0);
                out.text((at.0, at.1 - line / 2.0), &heights, colour::KEY, 8.0);
            }
            (None, None) => out.text(at, &a.key, colour::NAME, 8.0),
            (None, Some(_)) => {}
        }
    }
    // the selected box over them all: a thick edge, its grips, and its words beside it
    let refused = b.refused.as_deref();
    let col = if refused.is_some() { colour::DEAD_END } else { colour::PICK };
    let mut words: Vec<(String, Rgba)> = Vec::new();
    let mut by: Option<Box2> = None;
    if let Some(a) = selected {
        let r = cell_box(a.rect());
        if refused.is_some() {
            out.rect(r, colour::GONE);
        }
        out.frame(r, col, 4.0);
        let (half, rim) = (GRIP_PX / 2.0 * px, 2.0 * px);
        for (gx, gy) in [(r.0, r.1), (mid(r).0, r.1), (r.2, r.1), (r.0, mid(r).1), (r.2, mid(r).1), (r.0, r.3), (mid(r).0, r.3), (r.2, r.3)] {
            out.rect((gx - half, gy - half, gx + half, gy + half), col);
            out.rect((gx - half + rim, gy - half + rim, gx + half - rim, gy + half - rim), colour::GRIP);
        }
        let mut head = format!("{}  {} x {}  {} to {}", a.key, a.x1 - a.x0, a.y1 - a.y0, a.floor, snap_top(a.top));
        if let Some(word) = lv.and_then(|lv| differs(lv, a)) {
            head.push_str(&format!("  ({word})"));
        }
        words.push((head, col));
        // what was chosen for it by hand, in the words' own colour
        if let Some((_, chosen)) = view.chosen.iter().find(|(room, _)| *room == a.key) {
            words.extend(wrap(&format!("chosen: {chosen}"), WRAP).into_iter().map(|l| (l, colour::CHOSEN)));
        }
        by = Some(r);
    }
    if let Some(why) = refused {
        words.push(("can't be done:".to_string(), colour::DEAD_END));
        words.extend(wrap(why, WRAP).into_iter().map(|l| (l, colour::REFUSAL)));
    }
    if words.is_empty() {
        return;
    }
    // (a panel under the words, so they read over a busy plan; the words are sized in pixels,
    // so the panel is too)
    let dot = if view.scale >= 18.0 { 2.0 } else { 1.0 };
    let (pad, step) = (7.0 * px, (8.0 * dot + 5.0) * px);
    let wide = words.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0) as f32 * 6.0 * dot * px + 2.0 * pad;
    let high = words.len() as f32 * step + 2.0 * pad - 5.0 * px;
    let edge = 0.2;
    let (x, y) = match by {
        Some(r) => beside(r, (wide, high), (GRIP_PX / 2.0 + 5.0) * px, (lo.0 + edge, lo.1 + edge), (hi.0 - edge, hi.1 - edge)),
        // (a refusal with no box to show it by, a room that is not there: the picture's corner)
        None => (lo.0 + 0.5, hi.1 - 0.5),
    };
    let panel = (x, y - high, x + wide, y);
    // (words drawn already that would lie under the panel go: a snapshot letters everything
    // after every shape, and they would run through the panel's own)
    let under = |p: &Prim| text_box(p, px).is_some_and(|b| b.0 < panel.2 && panel.0 < b.2 && b.1 < panel.3 && panel.1 < b.3);
    out.retain(|p| !under(p));
    out.rect(panel, colour::PANEL);
    out.frame(panel, col, 1.5);
    for (n, (text, col)) in words.into_iter().enumerate() {
        let at = (x + pad, y - pad - 3.5 * dot * px - n as f32 * step);
        out.push(Prim::Text { at, text, col, size_px: 8.0 * dot, centred: false });
    }
}

/// The legend's rows for the room boxes: what there is of them in this view.
fn box_keys(view: &View, lv: Option<&Level>) -> Vec<Key> {
    let Some(b) = &view.boxes else { return Vec::new() };
    let key = |swatch: Swatch, col: Rgba, text: &str| Key { swatch, col, text: text.to_string() };
    let mut out = vec![key(Swatch::Line, colour::box_edge(0), "room box (edged in its storey's colour)")];
    if view.storey.is_some() {
        out.push(key(Swatch::Line, colour::box_edge(0), "dashed: the box of a room that is only air here"));
    }
    if let Some(lv) = lv {
        if b.lots.iter().any(|a| differs(lv, a).is_some()) {
            out.push(key(Swatch::Line, colour::CHANGED, "box moved, resized, new or left out: not as the level drawn has it"));
        }
        if !deleted(lv, &b.lots).is_empty() {
            out.push(key(Swatch::Fill, colour::GONE, "room deleted (crossed out)"));
        }
    }
    match (&b.refused, &b.selected) {
        (Some(_), _) => out.push(key(Swatch::Line, colour::DEAD_END, "change refused: the room's box in red, the reason by it")),
        (None, Some(_)) => out.push(key(Swatch::Line, colour::PICK, "box selected: its grips pull its sides")),
        (None, None) => {}
    }
    out
}

/// What the colours and marks mean, for the level's own storeys.
pub fn legend(lv: &Level, view: &View) -> Vec<Key> {
    let key = |swatch: Swatch, col: Rgba, text: &str| Key { swatch, col, text: text.to_string() };
    let mut out: Vec<Key> =
        storeys(lv).into_iter().filter(|z| view.storey.is_none_or(|s| s == *z)).map(|z| key(Swatch::Fill, colour::storey(z), &format!("floor {z}"))).collect();
    if view.storey.is_some() {
        out.push(key(Swatch::Fill, colour::VOID, "air of a room below"));
        out.push(key(Swatch::Line, colour::RAIL, "rail"));
    }
    out.extend([key(Swatch::Line, colour::DOOR, "door"), key(Swatch::Line, colour::RAMP_ARROW, "ramp, uphill (sw: a switchback)")]);
    if lv.interiors.iter().any(|(_, it)| !it.chutes.is_empty()) {
        out.push(key(Swatch::Line, colour::LADDER, "ladder up a deck's edge (the rescue's), uphill"));
    }
    if lv.interiors.iter().any(|(_, it)| !it.crossings.is_empty()) {
        out.push(key(Swatch::Line, colour::LADDER, "bridge's outline (the rescue's)"));
    }
    out.extend([
        key(Swatch::Line, colour::VENT, "vent (crawl)"),
        key(Swatch::Line, colour::GLASS, "window"),
        key(Swatch::Line, colour::BALCONY, "open balcony"),
        key(Swatch::Fill, colour::POOL, "water (railed)"),
        key(Swatch::Fill, colour::COVER, "pillar, crate, wall"),
        key(Swatch::Line, colour::DROP_MARK, "drop (X) / lands"),
        key(Swatch::Fill, colour::GRATE, "floor grate"),
        key(Swatch::Ring, colour::CAMP, "allowed dead end"),
        key(Swatch::Ring, colour::DEAD_END, "DEAD END: too long"),
    ]);
    if view.mark.is_some() {
        out.push(key(Swatch::Ring, colour::MARK, "picked in the list"));
    }
    if view.pick.is_some() {
        out.push(key(Swatch::Ring, colour::PICK, "room picked to join"));
        out.push(key(Swatch::Line, colour::HANDLE, "click its wall: a doorway there"));
    }
    if view.asked.doorways() {
        out.push(key(Swatch::Line, colour::ASKED, "doorway asked for"));
    }
    if !view.joins.is_empty() {
        out.push(key(Swatch::Fill, colour::JOIN_DISC, "could be made (numbered as listed)"));
    }
    if view.route.is_some() {
        out.push(key(Swatch::Line, colour::MARK, "corridor picked"));
    }
    if view.gone.is_some() {
        out.push(key(Swatch::Line, colour::DEAD_END, "corridor taken out"));
    }
    if view.chosen.iter().any(|(room, _)| lv.spaces().iter().any(|s| s.key == *room)) {
        out.push(key(Swatch::Line, colour::CHOSEN, "chosen by hand for the room"));
    }
    out.extend(box_keys(view, Some(lv)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::cells::rect_cells;

    #[test]
    fn a_patch_of_cells_has_its_outline_in_runs() {
        // an L: 3 x 1 with one more cell on top of its west end
        let mut cells = rect_cells((0, 0, 3, 1));
        cells.insert((0, 1));
        let mut got = edges(&cells);
        got.sort();
        assert_eq!(got, [((0, 0), (0, 2)), ((0, 0), (3, 0)), ((0, 2), (1, 2)), ((1, 1), (1, 2)), ((1, 1), (3, 1)), ((3, 0), (3, 1))]);
        assert!(edges(&Cells::new()).is_empty());
    }

    #[test]
    fn places_name_their_rooms_and_storeys() {
        assert_eq!(place("spire1@827"), ("spire1", Some(827)));
        assert_eq!(place("colossal@551.2"), ("colossal", Some(551)));
        assert_eq!(place("small7@-276"), ("small7", Some(-276)));
        assert_eq!(place("tiny5"), ("tiny5", None));
    }

    #[test]
    fn every_storey_has_its_own_colour() {
        let all: std::collections::BTreeSet<Rgba> = LEVELS.iter().map(|&z| colour::storey(z)).collect();
        assert_eq!(all.len(), LEVELS.len());
        assert_eq!((colour::storey(-276), colour::storey(4134)), (colour::STOREY[0], colour::STOREY[16]));
        assert_eq!(colour::mix([0, 0, 0, 255], [200, 100, 50, 255], 0.5), [100, 50, 25, 255]);
        assert_eq!(colour::lit([100, 100, 100, 255], 1.0), [255, 255, 255, 255]);
    }
}

#[cfg(test)]
mod picture_tests {
    use super::*;
    use crate::fixture;

    fn has_rect(prims: &[Prim], r: Box2) -> bool {
        let want = vec![(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)];
        prims.iter().any(|p| matches!(p, Prim::Poly { pts, .. } if *pts == want))
    }

    fn texts(prims: &[Prim]) -> Vec<&str> {
        prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.as_str()) } else { None }).collect()
    }

    #[test]
    fn every_room_and_route_is_in_the_picture() {
        let lv = &fixture::seed40().level;
        let pic = picture(lv, &View::default());
        let said = texts(&pic.prims);
        assert_eq!(lv.spaces().len(), 29);
        for s in lv.spaces() {
            assert!(has_rect(&pic.prims, cell_box(s.rect())), "no box for {}", s.key);
            assert!(said.iter().any(|t| *t == s.key || t.starts_with(&format!("{}  ", s.key))), "no key for {}", s.key);
        }
        for r in lv.net.routes() {
            let key = r.key();
            assert!(said.iter().any(|t| *t == key || *t == format!("{key} sw")), "no key for {key}");
            for pc in &r.pieces {
                if pc.kind == "vent" {
                    continue;
                }
                for (_, cells) in &pc.cells {
                    assert!(rectangles(cells).into_iter().all(|rect| has_rect(&pic.prims, cell_box(rect))), "no floor for {}", pc.key);
                }
                if let Some(rect) = pc.ramp_cells {
                    assert!(has_rect(&pic.prims, cell_box(rect)), "no ramp for {}", pc.key);
                }
            }
        }
        // a vent is thin, with its line down the middle of every run
        let vents: Vec<&Route> = lv.net.routes().iter().filter(|r| r.pieces[0].kind == "vent").collect();
        assert!(!vents.is_empty());
        let lines = pic.prims.iter().filter(|p| matches!(p, Prim::Line { col, .. } if *col == colour::VENT)).count();
        assert_eq!(lines, vents.iter().map(|r| r.path.len() - 1).sum::<usize>());
        // a bar for every door, a room's and a pod's
        let doors: usize = lv.spaces().iter().filter(|s| s.host.is_empty()).map(|s| lv.net.doors(&s.key).len()).sum();
        let pods = lv.pods.iter().filter(|(_, p)| p.door.is_some()).count();
        let bars = pic.prims.iter().filter(|p| matches!(p, Prim::Line { col, width_px, .. } if *col == colour::DOOR && *width_px == 4.0)).count();
        assert_eq!((bars, doors), (doors + pods, 2 * lv.net.routes().len()));
        // an arrow up every ramp: inside rooms, in corridors, in vents
        let inside: usize = lv.interiors.iter().map(|(_, it)| it.ramps.len()).sum();
        let between: usize = lv.net.routes().iter().flat_map(|r| &r.pieces).map(|pc| pc.ramp.iter().count() + pc.ducts.len()).sum();
        let heads = pic.prims.iter().filter(|p| matches!(p, Prim::Poly { pts, fill } if pts.len() == 3 && *fill == colour::RAMP_ARROW)).count();
        assert_eq!(heads, inside + between);
        // the pool, the drops, the grates and the windows
        assert!(pic.prims.iter().any(|p| matches!(p, Prim::Poly { fill, .. } if *fill == colour::POOL)));
        let count = |c: Rgba| pic.prims.iter().filter(|p| matches!(p, Prim::Poly { fill, .. } if *fill == c)).count();
        assert_eq!((count(colour::DROP), count(colour::GRATE)), (lv.drops.len(), lv.grates.len()));
        let panes = pic.prims.iter().filter(|p| matches!(p, Prim::Line { col, .. } if *col == colour::GLASS || *col == colour::BALCONY)).count();
        assert_eq!(panes, lv.windows.len());
        // every dead end left is ringed and numbered; none is too long
        for (n, e) in lv.dead_ends.iter().enumerate() {
            let name = format!("{} {}", if e.rooms.is_empty() { "deck" } else { "camp" }, n + 1);
            assert!(said.contains(&name.as_str()), "{name}");
        }
        let rings = pic.prims.iter().filter(|p| matches!(p, Prim::Circle { col, filled: false, .. } if *col == colour::CAMP)).count();
        assert_eq!(rings, lv.dead_ends.len());
        assert!(!said.iter().any(|t| t.starts_with("DEAD END")));
        // it all lies inside the bounds
        let within = |p: &P| pic.lo.0 <= p.0 && p.0 <= pic.hi.0 && pic.lo.1 <= p.1 && p.1 <= pic.hi.1;
        assert!(pic.prims.iter().all(|p| match p {
            Prim::Poly { pts, .. } => pts.iter().all(within),
            Prim::Line { a, b, .. } => within(a) && within(b),
            Prim::Circle { c, .. } => within(c),
            Prim::Text { at, .. } | Prim::Badge { at, .. } => within(at),
        }));
        assert_eq!(storeys(lv), [-276, 0, 276, 551, 827, 1102]);
        assert_eq!(pic.legend.iter().filter(|k| k.text.starts_with("floor ") && k.text != "floor grate").count(), 6);
    }

    #[test]
    fn one_storey_shows_its_own_floors_and_the_air_of_rooms_below() {
        let lv = &fixture::seed40().level;
        let pic = picture(lv, &View { storey: Some(276), ..View::default() });
        let said = texts(&pic.prims);
        let fill_of = |r: Box2| {
            let want = vec![(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)];
            pic.prims.iter().find_map(|p| match p {
                Prim::Poly { pts, fill } if *pts == want => Some(*fill),
                _ => None,
            })
        };
        for s in lv.spaces().iter().filter(|s| s.host.is_empty()) {
            let fill = fill_of(cell_box(s.rect()));
            if s.floor == 276 {
                assert_eq!(fill, Some(colour::storey(276)), "{}", s.key);
            } else if s.storeys().contains(&276) {
                assert_eq!(fill, Some(colour::VOID), "{}", s.key);
                assert!(said.contains(&format!("{} (floor {})", s.key, s.floor).as_str()));
            } else {
                assert_eq!(fill, None, "{}", s.key);
                assert!(!said.iter().any(|t| *t == s.key || t.starts_with(&format!("{} ", s.key))), "{}", s.key);
            }
        }
        // decks on this storey, with their rails; corridors of this storey only
        assert!(pic.prims.iter().any(|p| matches!(p, Prim::Line { col, .. } if *col == colour::RAIL)));
        for r in lv.net.routes().iter().filter(|r| r.pieces[0].kind != "vent") {
            let here = r.path.iter().any(|p| p.2 == 276);
            assert_eq!(said.iter().any(|t| *t == r.key() || *t == format!("{} sw", r.key())), here, "{}", r.key());
        }
        assert_eq!(pic.legend[0].text, "floor 276");
        assert!(pic.prims.len() < picture(lv, &View::default()).prims.len());
    }

    #[test]
    fn dead_ends_that_are_too_long_are_loud_and_a_picked_row_is_ringed() {
        let made = fixture::seed40_kept();
        let pic = picture(&made.level, &View::default());
        let said = texts(&pic.prims);
        assert!(said.contains(&"DEAD END 1") && said.contains(&"DEAD END 2") && said.contains(&"camp 3"));
        let loud = pic.prims.iter().filter(|p| matches!(p, Prim::Circle { col, filled: true, .. } if *col == colour::DEAD_END)).count();
        assert_eq!(loud, 2);
        assert!(pic.prims.iter().any(|p| matches!(p, Prim::Line { col, width_px, .. } if *col == colour::DEAD_END && *width_px == 4.0)));
        assert!(pic
            .prims
            .iter()
            .any(|p| matches!(p, Prim::Text { text, size_px, col, .. } if text == "DEAD END 1" && *size_px == 16.0 && *col == colour::DEAD_END)));
        // picking a row of the list rings its rooms and its way in
        assert!(!pic.prims.iter().any(|p| matches!(p, Prim::Line { col, .. } | Prim::Circle { col, .. } if *col == colour::MARK)));
        let mark = made.dead.left[2].mark.clone();
        assert_eq!(mark.rooms, ["small7"]);
        let picked = picture(&made.level, &View { mark: Some(mark), ..View::default() });
        assert!(picked.prims.iter().any(|p| matches!(p, Prim::Circle { col, .. } if *col == colour::MARK)));
        assert!(picked.prims.iter().filter(|p| matches!(p, Prim::Line { col, .. } if *col == colour::MARK)).count() >= 4);
        assert_eq!(picked.legend.last().unwrap().text, "picked in the list");
    }

    #[test]
    fn a_click_picks_what_is_drawn_on_top() {
        let lv = &fixture::seed40().level;
        let mid = |key: &str| {
            let (x0, y0, x1, y1) = lv.space(key).rect();
            ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0)
        };
        let room = |key: &str| Some(Hit::Room(key.to_string()));
        // a room, on any storey it has air on; a pod over its hall, and the hall under it
        assert_eq!(hit(lv, mid("tower"), None), room("tower"));
        assert_eq!((hit(lv, mid("tower"), Some(0)), hit(lv, mid("tower"), Some(551)), hit(lv, mid("tower"), Some(1378))), (room("tower"), room("tower"), None));
        assert_eq!((lv.space("small8").host.as_str(), lv.space("small8").floor), ("vast", 551));
        assert_eq!(
            (hit(lv, mid("small2"), None), hit(lv, mid("small2"), Some(276)), hit(lv, mid("small2"), Some(0))),
            (room("small2"), room("small2"), room("colossal"))
        );
        // (a corridor crosses over small8 at 827: with every storey shown it is on top)
        assert_eq!(
            (hit(lv, mid("small8"), None), hit(lv, mid("small8"), Some(551)), hit(lv, mid("small8"), Some(0))),
            (Some(Hit::Route("c26a".into())), room("small8"), room("vast"))
        );
        // every cell of every route is that route on its own storey; with every storey shown
        // it is the route too, unless a room's floor or another corridor lies over it
        let (mut under, mut seen) = (0, 0);
        for r in lv.net.routes() {
            for (z, x, y) in footprint(r) {
                let at = (x as f32 + 0.5, y as f32 + 0.5);
                assert_eq!(hit(lv, at, Some(z)), Some(Hit::Route(r.key().to_string())), "{} at {x}, {y} on {z}", r.key());
                let over = lv.spaces().iter().find(|s| s.x0 <= x && x < s.x1 && s.y0 <= y && y < s.y1 && s.floor > z);
                let higher = lv.net.routes().iter().any(|q| footprint(q).iter().any(|&(qz, qx, qy)| (qx, qy) == (x, y) && qz > z));
                match (hit(lv, at, None), over) {
                    (Some(Hit::Room(key)), Some(_)) => {
                        assert!(lv.space(&key).floor > z);
                        under += 1;
                    }
                    (got, None) if !higher => {
                        assert_eq!(got, Some(Hit::Route(r.key().to_string())), "{} at {x}, {y}", r.key());
                        seen += 1;
                    }
                    (got, _) => assert!(matches!(got, Some(Hit::Route(_))), "{got:?}"),
                }
            }
        }
        assert!(under > 0 && seen > 100, "{under} {seen}");
        // off the level there is nothing; a cell's edges are its own
        let (lo, hi) = bounds(lv);
        assert_eq!((hit(lv, (lo.0 + 0.2, lo.1 + 0.2), None), hit(lv, (hi.0 + 5.0, hi.1), None)), (None, None));
        let (x0, y0, _, _) = lv.space("tower").rect();
        assert_eq!(hit(lv, (x0 as f32 + 0.01, y0 as f32 + 0.01), Some(0)), room("tower"));
        assert_ne!(hit(lv, (x0 as f32 - 0.01, y0 as f32 - 0.01), Some(0)), room("tower"));
        // the storey a picked room is joined from: the one shown if it takes doors there
        let tower = lv.space("tower");
        assert_eq!((pick_storey(tower, None), pick_storey(tower, Some(551)), pick_storey(tower, Some(1654))), (0, 551, 0));
        assert_eq!(pick_storey(lv.space("loft3"), Some(0)), 551);
    }

    #[test]
    fn a_box_is_taken_hold_of_by_its_body_or_the_selected_ones_sides() {
        let lot = |key: &str, r: Rect, floor: i32, top: i32, host: &str| Lot {
            key: key.into(),
            kind: "medium",
            x0: r.0,
            y0: r.1,
            x1: r.2,
            y1: r.3,
            floor,
            top,
            host: host.into(),
        };
        let lots = [
            lot("hall", (0, 0, 20, 12), 0, 787, ""),
            lot("pod", (16, 4, 22, 8), 276, 512, "hall"),
            lot("low", (4, 2, 8, 6), -276, -40, ""),
            lot("loft", (30, 0, 36, 5), 551, 787, ""),
        ];
        let hit = |at: P, storey: Option<i32>, selected: Option<&str>| hit_box(&lots, at, storey, selected, 0.4).map(|h| (h.key, h.part));
        let body = |key: &str| Some((key.to_string(), Part::Body));
        // with every storey shown the highest floor is on top, a pod over its hall
        assert_eq!(hit((5.5, 3.5), None, None), body("hall"));
        assert_eq!(hit((17.5, 5.5), None, None), body("pod"));
        assert_eq!(hit((21.5, 5.5), None, None), body("pod"));
        assert_eq!(hit((25.0, 5.0), None, None), None);
        // with one storey shown: what has its floor there, or a room that is air there
        assert_eq!(hit((5.5, 3.5), Some(-276), None), body("low"));
        assert_eq!(hit((17.5, 5.5), Some(0), None), body("hall"));
        assert_eq!(hit((17.5, 5.5), Some(551), None), body("hall"));
        assert_eq!(hit((32.0, 2.0), Some(0), None), None);
        // the selected box's sides and corners come first, from just outside it too
        let edge = |key: &str, sx: i8, sy: i8| Some((key.to_string(), Part::Edge(sx, sy)));
        assert_eq!(hit((20.2, 6.0), None, Some("hall")), edge("hall", 1, 0));
        assert_eq!(hit((19.8, 11.9), None, Some("hall")), edge("hall", 1, 1));
        assert_eq!(hit((-0.3, 0.2), None, Some("hall")), edge("hall", -1, -1));
        assert_eq!(hit((10.0, 12.3), None, Some("hall")), edge("hall", 0, 1));
        assert_eq!(hit((10.0, 6.0), None, Some("hall")), body("hall"));
        // not selected: no sides to take hold of
        assert_eq!(hit((20.2, 6.0), None, Some("loft")), body("pod"));
        assert_eq!(hit((19.9, 11.9), None, None), body("hall"));
        // a narrow box with a wide grip: the nearer side
        let thin = [lot("thin", (0, 0, 2, 2), 0, 236, "")];
        assert_eq!(hit_box(&thin, (0.6, 1.0), None, Some("thin"), 1.2).map(|h| h.part), Some(Part::Edge(-1, 0)));
        assert_eq!(hit_box(&thin, (1.6, 1.7), None, Some("thin"), 1.2).map(|h| h.part), Some(Part::Edge(1, 1)));
    }

    /// Seed 40's lots as the seed scatters them, and a way to change one.
    fn seed40_lots() -> Vec<Lot> {
        crate::doc::Doc::new(autolevel::recipe::Recipe::new(40), true).lots().unwrap().lots
    }

    fn lot_mut<'a>(lots: &'a mut [Lot], key: &str) -> &'a mut Lot {
        lots.iter_mut().find(|a| a.key == key).unwrap_or_else(|| panic!("no lot {key}"))
    }

    fn boxed(lots: Vec<Lot>, selected: Option<&str>, refused: Option<&str>) -> View {
        View { boxes: Some(Boxes { lots, selected: selected.map(str::to_string), refused: refused.map(str::to_string), site: None }), ..View::default() }
    }

    /// The lines of one colour and width.
    fn lines(prims: &[Prim], c: Rgba, w: f32) -> usize {
        prims.iter().filter(|p| matches!(p, Prim::Line { col, width_px, .. } if *col == c && *width_px == w)).count()
    }

    /// The rectangles filled with one colour.
    fn fills(prims: &[Prim], c: Rgba) -> Vec<Box2> {
        let rect = |p: &Prim| match p {
            Prim::Poly { pts, fill } if *fill == c && pts.len() == 4 => Some((pts[0].0, pts[0].1, pts[2].0, pts[2].1)),
            _ => None,
        };
        prims.iter().filter_map(rect).collect()
    }

    #[test]
    fn the_room_boxes_lie_over_the_level_and_add_no_words_where_it_has_the_rooms() {
        let (lv, lots) = (&fixture::seed40().level, seed40_lots());
        // the seed's own lots are the level's rooms: none differs, none is deleted
        assert_eq!(lots.len(), lv.spaces().len());
        assert!(lots.iter().all(|a| differs(lv, a).is_none()) && deleted(lv, &lots).is_empty());
        let (plain, pic) = (picture(lv, &View::default()), picture(lv, &boxed(lots.clone(), None, None)));
        assert_eq!(pic.prims[..plain.prims.len()], plain.prims[..]);
        let over = &pic.prims[plain.prims.len()..];
        // each a tint of its storey's colour with an edge round it, thick at the corners
        for a in &lots {
            assert!(fills(over, colour::faded(colour::storey(a.floor), 56)).contains(&cell_box(a.rect())), "no box for {}", a.key);
        }
        let floors: std::collections::BTreeSet<i32> = lots.iter().map(|a| a.floor).collect();
        let count = |w: f32| floors.iter().map(|z| lines(over, colour::box_edge(*z), w)).sum::<usize>();
        assert_eq!((count(2.0), count(4.0)), (4 * lots.len(), 8 * lots.len()));
        assert!(texts(over).is_empty() && lines(over, colour::CHANGED, 3.0) == 0);
        // a pod is drawn after its hall, so it lies on top of it
        let at = |key: &str| {
            let r = cell_box(lots.iter().find(|a| a.key == key).unwrap().rect());
            over.iter().position(|p| matches!(p, Prim::Poly { pts, .. } if pts.len() == 4 && (pts[0].0, pts[0].1, pts[2].0, pts[2].1) == r)).unwrap()
        };
        assert!(at("small8") > at("vast") && at("small2") > at("colossal"));
        // the legend says what they are, and the picture is no bigger for them
        assert_eq!(pic.legend.last().unwrap().text, "room box (edged in its storey's colour)");
        assert_eq!(pic.legend.len(), plain.legend.len() + 1);
        assert_eq!((pic.lo, pic.hi), (plain.lo, plain.hi));
    }

    #[test]
    fn the_selected_box_has_eight_grips_and_its_size_and_heights_beside_it() {
        let (lv, lots) = (&fixture::seed40().level, seed40_lots());
        let r = cell_box(lots.iter().find(|a| a.key == "medium1").unwrap().rect());
        assert_eq!(r, (50.0, 29.0, 57.0, 34.0));
        for scale in [25.0, 8.0] {
            let view = View { scale, ..boxed(lots.clone(), Some("medium1"), None) };
            let pic = picture(lv, &view);
            // a thick edge in the pick colour, and a square on every corner and mid-side, the
            // same size in pixels whatever the zoom
            assert_eq!(lines(&pic.prims, colour::PICK, 4.0), 4);
            let grips = fills(&pic.prims, colour::GRIP);
            let mid = |g: &Box2| (((g.0 + g.2) / 2.0 * 100.0).round() / 100.0, ((g.1 + g.3) / 2.0 * 100.0).round() / 100.0);
            let mut at: Vec<P> = grips.iter().map(mid).collect();
            at.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(at, [(50.0, 29.0), (50.0, 31.5), (50.0, 34.0), (53.5, 29.0), (53.5, 34.0), (57.0, 29.0), (57.0, 31.5), (57.0, 34.0)]);
            assert!(grips.iter().all(|g| ((g.2 - g.0) * scale - (GRIP_PX - 4.0)).abs() < 0.01 && ((g.3 - g.1) * scale - (GRIP_PX - 4.0)).abs() < 0.01));
            assert_eq!(fills(&pic.prims, colour::PICK).len(), 8);
            // its key, size and heights on a panel that does not lie on the box, inside the picture
            let head = pic.prims.iter().find(|p| matches!(p, Prim::Text { text, .. } if text == "medium1  7 x 5  0 to 512")).expect("its words");
            assert!(
                matches!(head, Prim::Text { col, centred: false, size_px, .. } if *col == colour::PICK && *size_px == if scale >= 18.0 { 16.0 } else { 8.0 })
            );
            let panel = fills(&pic.prims, colour::PANEL)[0];
            assert!(panel.2 <= r.0 || r.2 <= panel.0 || panel.3 <= r.1 || r.3 <= panel.1, "{panel:?} lies on {r:?}");
            assert!(pic.lo.0 <= panel.0 && panel.2 <= pic.hi.0 && pic.lo.1 <= panel.1 && panel.3 <= pic.hi.1);
            assert!(matches!(head, Prim::Text { at, .. } if panel.0 < at.0 && at.0 < panel.2 && panel.1 < at.1 && at.1 < panel.3));
            assert_eq!(pic.legend.last().unwrap().text, "box selected: its grips pull its sides");
        }
        // a selection that names no lot draws nothing
        let none = picture(lv, &boxed(lots.clone(), Some("attic"), None));
        assert!(fills(&none.prims, colour::GRIP).is_empty() && fills(&none.prims, colour::PANEL).is_empty());
        // the panel goes under a box at the top of the picture, and stays in it by a box at its edge
        let hi = bounds(lv).1;
        assert_eq!(beside((10.0, hi.1 - 3.0, 14.0, hi.1 - 1.5), (6.0, 2.0), 0.5, (0.0, 0.0), hi), (9.0, hi.1 - 3.5));
        assert_eq!(beside((0.5, 10.0, 2.5, 12.0), (6.0, 2.0), 0.5, (0.0, 0.0), hi), (0.0, 14.5));
        assert_eq!(beside((0.0, 0.0, hi.0, hi.1), (6.0, 2.0), 0.5, (0.0, 0.0), hi), (hi.0 / 2.0 - 3.0, 2.0));
    }

    #[test]
    fn a_box_the_level_does_not_have_there_says_so_and_a_room_with_no_lot_is_crossed_out() {
        let (lv, mut lots) = (&fixture::seed40().level, seed40_lots());
        let was = cell_box(lot_mut(&mut lots, "medium1").rect());
        // medium1 ten cells east, small1 two storeys up, small7 gone, and a room the level has not
        (lot_mut(&mut lots, "medium1").x0, lot_mut(&mut lots, "medium1").x1) = (60, 67);
        (lot_mut(&mut lots, "small1").floor, lot_mut(&mut lots, "small1").top) = (551, 787);
        let gone = cell_box(lv.space("small7").rect());
        lots.retain(|a| a.key != "small7");
        lots.push(Lot { key: "room1".into(), kind: "medium", x0: 90, y0: 40, x1: 96, y1: 45, floor: 0, top: 236, host: String::new() });
        let word = |key: &str| differs(lv, lots.iter().find(|a| a.key == key).unwrap());
        assert_eq!(
            (word("medium1").as_deref(), word("small1").as_deref(), word("room1").as_deref(), word("tower")),
            (Some("moved"), Some("floor was 276"), Some("new"), None)
        );
        assert_eq!(deleted(lv, &lots).iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["small7"]);
        let pic = picture(lv, &boxed(lots.clone(), None, None));
        let said = texts(&pic.prims);
        // each stands out: filled in, edged in white, with its key and the word (and its heights)
        for key in ["medium1", "small1", "room1"] {
            let a = lots.iter().find(|a| a.key == key).unwrap();
            assert!(fills(&pic.prims, colour::faded(colour::storey(a.floor), 170)).contains(&cell_box(a.rect())), "{key}");
        }
        assert_eq!(lines(&pic.prims, colour::CHANGED, 3.0), 12);
        assert!(said.contains(&"medium1 moved") && said.contains(&"room1 new") && said.contains(&"0 to 236"), "{said:?}");
        // (too long for a box four cells wide: on two lines)
        assert!(said.contains(&"small1") && said.contains(&"floor was 276") && said.contains(&"551 to 787"), "{said:?}");
        // where the level has medium1 is dimmed and no longer named; a line runs to the box
        assert!(fills(&pic.prims, colour::WAS).contains(&was) && !said.contains(&"Plant Room"));
        assert!(picture(lv, &View::default()).prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "Plant Room")));
        assert!(lines(&pic.prims, colour::CHANGED, 1.5) > 5);
        // the room with no lot is crossed out in red and says so, once
        assert!(fills(&pic.prims, colour::GONE).contains(&gone) && lines(&pic.prims, colour::DEAD_END, 2.0) == 6);
        assert_eq!(said.iter().filter(|t| t.starts_with("small7")).copied().collect::<Vec<_>>(), ["small7 deleted"]);
        // the legend has a row for each, and the picture has grown to hold the new room
        let legend: Vec<&str> = pic.legend.iter().map(|k| k.text.as_str()).collect();
        assert!(legend.contains(&"box moved, resized, new or left out: not as the level drawn has it") && legend.contains(&"room deleted (crossed out)"));
        assert_eq!((pic.hi.0, pic.lo), (97.5, bounds(lv).0));
        // the other ways a box differs
        let mut a = lots.iter().find(|a| a.key == "tower").unwrap().clone();
        (a.x1, a.top) = (a.x1 + 1, 787);
        assert_eq!(differs(lv, &a).as_deref(), Some("resized, top was 1063"));
        (a.x0, a.x1) = (a.x0 + 1, a.x1 + 2);
        assert_eq!(differs(lv, &a).as_deref(), Some("moved, resized, top was 1063"));
        // (a top is a room's on the nearest ceiling line: 788 is the level's 787)
        let b = seed40_lots().into_iter().find(|a| a.key == "medium5").unwrap();
        assert_eq!((b.top, lv.space("medium5").top, differs(lv, &b)), (788, 787, None));
        // a hall deleted takes its pods: all crossed out, the hall alone saying so, in large
        let lots: Vec<Lot> = seed40_lots().into_iter().filter(|a| a.key != "vast" && a.host != "vast").collect();
        let pic = picture(lv, &boxed(lots, None, None));
        assert_eq!(fills(&pic.prims, colour::GONE).len(), 3);
        assert!(pic.prims.iter().any(|p| matches!(p, Prim::Text { text, size_px, .. } if text == "vast deleted" && *size_px == 16.0)));
        assert!(!texts(&pic.prims).iter().any(|t| t.ends_with("deleted") && *t != "vast deleted") && texts(&pic.prims).contains(&"small8"));
    }

    #[test]
    fn a_refused_change_is_red_with_its_reason_in_lines_short_enough_to_read() {
        let (lv, mut lots) = (&fixture::seed40().level, seed40_lots());
        let why = "medium1 and small3 overlap: rooms keep a cell between them, unless one is over the other with a 40-unit slab between \
                   (medium1 is at 48, 41 to 55, 46, from 0 to 512; small3 is at 52, 45 to 56, 50, from 276 to 512)";
        let a = lot_mut(&mut lots, "medium1");
        (a.x0, a.y0, a.x1, a.y1) = (48, 41, 55, 46);
        let r = cell_box(a.rect());
        let pic = picture(lv, &boxed(lots.clone(), Some("medium1"), Some(why)));
        // the box in the dead ends' red: a thick edge, a red wash, its grips
        assert_eq!((lines(&pic.prims, colour::DEAD_END, 4.0), lines(&pic.prims, colour::PICK, 4.0)), (4, 0));
        assert!(fills(&pic.prims, colour::GONE).contains(&r) && fills(&pic.prims, colour::GRIP).len() == 8);
        // its name, then the reason, whole, in lines of 46 letters at most
        let panel = fills(&pic.prims, colour::PANEL)[0];
        let own: Vec<(&str, Rgba, P)> = pic
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { at, text, col, centred: false, .. } => Some((text.as_str(), *col, *at)),
                _ => None,
            })
            .collect();
        assert_eq!((own[0].0, own[0].1), ("medium1  7 x 5  0 to 512  (moved)", colour::DEAD_END));
        assert_eq!((own[1].0, own[1].1), ("can't be done:", colour::DEAD_END));
        let reason: Vec<&str> = own[2..].iter().map(|l| l.0).collect();
        assert!(reason.len() >= 4 && reason.iter().all(|l| l.chars().count() <= 46), "{reason:?}");
        assert_eq!(reason.join(" "), why);
        assert!(own[2..].iter().all(|l| l.1 == colour::REFUSAL));
        // each line under the one before, all on the panel, the panel off the box and in the picture
        assert!(own.windows(2).all(|w| w[1].2 .1 < w[0].2 .1 && w[1].2 .0 == w[0].2 .0));
        assert!(own.iter().all(|l| panel.0 < l.2 .0 && l.2 .0 < panel.2 && panel.1 < l.2 .1 && l.2 .1 < panel.3));
        assert!(panel.2 <= r.0 || r.2 <= panel.0 || panel.3 <= r.1 || r.3 <= panel.1, "{panel:?} lies on {r:?}");
        assert!(pic.lo.0 <= panel.0 && panel.2 <= pic.hi.0 && pic.lo.1 <= panel.1 && panel.3 <= pic.hi.1);
        // none of the level's words is left to run through it
        let under = |p: &Prim| matches!(p, Prim::Text { at, centred: true, .. } if panel.0 < at.0 && at.0 < panel.2 && panel.1 < at.1 && at.1 < panel.3);
        assert!(!pic.prims.iter().any(under));
        assert!(picture(lv, &boxed(lots.clone(), None, None)).prims.iter().any(under), "the panel covers some of the level's words");
        assert_eq!(pic.legend.last().unwrap().text, "change refused: the room's box in red, the reason by it");
        // a refusal with no box to show it by (a room that is not there) is in the picture's corner
        let pic = picture(lv, &boxed(seed40_lots(), None, Some("move: no room attic")));
        let panel = fills(&pic.prims, colour::PANEL)[0];
        assert_eq!((panel.0, panel.3), (pic.lo.0 + 0.5, pic.hi.1 - 0.5));
        assert!(texts(&pic.prims).contains(&"move: no room attic") && fills(&pic.prims, colour::GRIP).is_empty());
        // words are broken between words, and a long one has a line to itself
        assert_eq!(wrap("rooms keep a cell between them", 12), ["rooms keep a", "cell between", "them"]);
        assert_eq!(wrap("a  incomprehensibilities b", 5), ["a", "incomprehensibilities", "b"]);
        assert!(wrap("", 5).is_empty());
    }

    #[test]
    fn one_storey_shows_the_boxes_with_floor_there_and_dashes_those_that_are_only_air() {
        let (lv, lots) = (&fixture::seed40().level, seed40_lots());
        let view = View { storey: Some(276), ..boxed(lots.clone(), None, None) };
        let (plain, pic) = (picture(lv, &View { storey: Some(276), ..View::default() }), picture(lv, &view));
        let over = &pic.prims[plain.prims.len()..];
        let (mut floor, mut air, mut out) = (0, 0, 0);
        for a in &lots {
            let filled = fills(over, colour::faded(colour::storey(a.floor), 56)).contains(&cell_box(a.rect()));
            // (a dash is a short line that starts on the box's west side)
            let dashed = over.iter().any(|p| matches!(p, Prim::Line { a: from, b: to, col, .. }
                if *col == colour::box_edge(a.floor) && from.0 == a.x0 as f32 && to.0 == a.x0 as f32 && (to.1 - from.1).abs() < 0.6 && a.y0 as f32 <= from.1 && from.1 <= a.y1 as f32));
            match shows(a.floor, a.top, !a.host.is_empty(), Some(276)) {
                Some(true) => {
                    assert!(filled && a.floor == 276, "{}", a.key);
                    floor += 1;
                }
                Some(false) => {
                    assert!(!filled && dashed && a.floor < 276 && a.top > 276, "{}", a.key);
                    air += 1;
                }
                None => {
                    assert!(!filled && (!dashed || lots.iter().any(|b| b.floor == a.floor && b.x0 == a.x0 && b.key != a.key)), "{}", a.key);
                    out += 1;
                }
            }
        }
        assert!(floor >= 5 && air >= 5 && out >= 5, "{floor} {air} {out}");
        // a pod is its own floor alone, and a room that stops under the storey is left out
        assert_eq!((shows(551, 787, true, Some(551)), shows(276, 512, true, Some(400)), shows(0, 236, false, Some(276))), (Some(true), None, None));
        assert_eq!((shows(0, 512, false, Some(276)), shows(0, 512, false, None), shows(276, 512, false, Some(0))), (Some(false), Some(true), None));
        assert!(pic.legend.iter().any(|k| k.text == "dashed: the box of a room that is only air here"));
        // the selected box keeps its grips whatever storey is shown
        let high = picture(lv, &View { storey: Some(1378), ..boxed(lots, Some("medium1"), None) });
        assert_eq!((fills(&high.prims, colour::GRIP).len(), lines(&high.prims, colour::PICK, 4.0)), (8, 4));
    }

    #[test]
    fn bare_is_the_grid_and_the_boxes_alone() {
        let lots = seed40_lots();
        let pic = bare(&boxed(lots.clone(), Some("tower"), None));
        // bounds that hold every lot with the level's margin, and the grid over them
        let most = |f: fn(&Lot) -> i32| lots.iter().map(f).max().unwrap() as f32;
        let least = |f: fn(&Lot) -> i32| lots.iter().map(f).min().unwrap() as f32;
        assert_eq!((pic.lo, pic.hi), ((least(|a| a.x0) - 1.5, least(|a| a.y0) - 1.5), (most(|a| a.x1) + 1.5, most(|a| a.y1) + 1.5)));
        assert_eq!((pic.lo, pic.hi), bounds_bare(&boxed(lots.clone(), None, None)));
        assert!(lines(&pic.prims, colour::GRID, 1.0) > 50 && lines(&pic.prims, colour::GRID_FOURTH, 1.0) > 20);
        // every lot filled in and named, since no level is there to name it; no box says "new"
        let said = texts(&pic.prims);
        for a in lots.iter().filter(|a| a.key != "tower") {
            assert!(fills(&pic.prims, colour::faded(colour::storey(a.floor), 222)).contains(&cell_box(a.rect())), "no box for {}", a.key);
            assert!(said.contains(&a.key.as_str()), "no key for {}", a.key);
        }
        assert!(said.contains(&"0 to 1063") && !said.iter().any(|t| t.ends_with(" new")) && lines(&pic.prims, colour::CHANGED, 3.0) == 0);
        // the selected one has its grips and its words on the panel
        assert_eq!(fills(&pic.prims, colour::GRIP).len(), 8);
        assert!(pic.prims.iter().any(|p| matches!(p, Prim::Text { text, centred: false, .. } if text == "tower  9 x 10  0 to 1063")));
        // the legend: the storeys the lots stand on, and the boxes' rows
        let legend: Vec<&str> = pic.legend.iter().map(|k| k.text.as_str()).collect();
        assert_eq!(
            legend,
            [
                "floor -276",
                "floor 0",
                "floor 276",
                "floor 551",
                "floor 827",
                "floor 1102",
                "room box (edged in its storey's colour)",
                "box selected: its grips pull its sides"
            ]
        );
        // one storey, and no boxes at all
        let one = bare(&View { storey: Some(551), ..boxed(lots, None, None) });
        assert_eq!(one.legend[0].text, "floor 551");
        assert!(one.prims.len() < pic.prims.len() && texts(&one.prims).contains(&"small8") && !texts(&one.prims).contains(&"medium2"));
        let none = bare(&View::default());
        assert_eq!((none.lo, none.hi, none.legend.len()), ((0.0, 0.0), (1.0, 1.0), 0));
        assert!(none.prims.iter().all(|p| matches!(p, Prim::Line { .. })));
    }

    #[test]
    fn what_was_chosen_for_a_room_is_written_by_its_name_clear_of_other_words() {
        let lv = &fixture::seed40().level;
        let said = |room: &str, words: &str| (room.to_string(), words.to_string());
        let chosen = vec![
            said("colossal", "layout ring, cover crates"),
            said("vast", "inside 2, brightness 90, ceiling grille"),
            said("basin", "cover none"),
            said("tiny4", "ceiling grille"),
            said("attic", "cover none"),
        ];
        let view = View { chosen: chosen.clone(), ..View::default() };
        let (plain, pic) = (picture(lv, &View::default()), picture(lv, &view));
        // the level is all there; the words are added, in their own colour
        assert_eq!(pic.prims[..plain.prims.len()], plain.prims[..]);
        let over = &pic.prims[plain.prims.len()..];
        assert!(over.iter().all(|p| matches!(p, Prim::Text { col, size_px, centred: true, .. } if *col == colour::CHOSEN && *size_px == 8.0)));
        // each room's words, whole and in order, in lines that fit across it; a room too
        // narrow for them says only that something was chosen; a room that is not there, nothing
        let px = 1.0 / view.scale;
        let of = |room: &str| {
            let r = cell_box(lv.space(room).rect());
            let inside = |p: &&Prim| matches!(p, Prim::Text { at, .. } if r.0 < at.0 && at.0 < r.2 && r.1 < at.1 && at.1 < r.3);
            let mut lines: Vec<(f32, &str)> =
                over.iter().filter(inside).filter_map(|p| if let Prim::Text { at, text, .. } = p { Some((at.1, text.as_str())) } else { None }).collect();
            lines.sort_by(|a, b| b.0.total_cmp(&a.0));
            lines.into_iter().map(|l| l.1).collect::<Vec<_>>()
        };
        assert_eq!(of("colossal"), ["layout ring, cover crates"]);
        assert_eq!(of("basin"), ["cover none"]);
        assert_eq!(of("tiny4"), ["chosen"]);
        let vast = of("vast").into_iter().filter(|l| *l != "chosen").collect::<Vec<_>>();
        assert_eq!(vast.join(" "), "inside 2, brightness 90, ceiling grille");
        assert_eq!(over.len(), 3 + vast.len());
        // every line lies in its room, on no other words and on none of the room's pods
        for (n, p) in over.iter().enumerate() {
            let b = text_box(p, px).unwrap();
            let clear = |t: Box2| t.2 <= b.0 || b.2 <= t.0 || t.3 <= b.1 || b.3 <= t.1;
            let others = pic.prims.iter().enumerate().filter(|(m, _)| *m != plain.prims.len() + n).filter_map(|(_, q)| text_box(q, px));
            assert!(others.into_iter().all(clear), "{p:?} lies on other words");
            let room = lv
                .spaces()
                .iter()
                .filter(|s| s.host.is_empty())
                .find(|s| cell_box(s.rect()).0 <= b.0 && b.2 <= cell_box(s.rect()).2 && cell_box(s.rect()).1 <= b.1 && b.3 <= cell_box(s.rect()).3);
            let room = room.unwrap_or_else(|| panic!("{p:?} is in no room"));
            assert!(lv.spaces().iter().filter(|s| s.host == room.key).all(|s| clear(cell_box(s.rect()))), "{p:?} lies on a pod of {}", room.key);
        }
        // the legend says what the colour is, where a room of the level has something chosen
        assert_eq!(pic.legend.last().unwrap().text, "chosen by hand for the room");
        let none = picture(lv, &View { chosen: vec![said("attic", "cover none")], ..View::default() });
        assert_eq!((none.prims.len(), none.legend.len()), (plain.prims.len(), plain.legend.len()));
        // one storey: the rooms with floor there, and the halls that are air there (a layout is
        // a hall's upper storey); a storey none of them reaches, nothing
        let count = |z: i32| {
            picture(lv, &View { storey: Some(z), ..view.clone() })
                .prims
                .iter()
                .filter(|p| matches!(p, Prim::Text { col, .. } if *col == colour::CHOSEN))
                .count()
        };
        assert!(count(276) >= 4 && count(551) >= 1 && count(1378) == 0, "{} {} {}", count(276), count(551), count(1378));
        // where the words are too long for the room they are broken, each line across it
        let narrow = picture(lv, &View { scale: 14.0, ..view.clone() });
        let lines: Vec<&str> = narrow
            .prims
            .iter()
            .filter_map(|p| if let Prim::Text { text, col, .. } = p { (*col == colour::CHOSEN).then_some(text.as_str()) } else { None })
            .collect();
        assert!(lines.contains(&"inside 2, brightness") && lines.contains(&"90, ceiling grille"), "{lines:?}");
        // zoomed far out there is no room for words, not even the one: nothing is written
        let far = picture(lv, &View { scale: 2.0, ..view.clone() });
        assert!(far.prims.iter().all(|p| !matches!(p, Prim::Text { col, .. } if *col == colour::CHOSEN)));
        // the selected box's panel has its room's choices too
        let boxed = View { chosen, ..boxed(seed40_lots(), Some("vast"), None) };
        let pic = picture(lv, &boxed);
        let panel = fills(&pic.prims, colour::PANEL)[0];
        let within = |at: &P| panel.0 < at.0 && at.0 < panel.2 && panel.1 < at.1 && at.1 < panel.3;
        let on_panel: Vec<&str> = pic
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { at, text, col, centred: false, .. } if *col == colour::CHOSEN && within(at) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        // (a letter over the panel's 46: on two lines)
        assert_eq!(on_panel, ["chosen: inside 2, brightness 90, ceiling", "grille"]);
        assert!(!picture(lv, &View { chosen: Vec::new(), ..boxed }).prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text.starts_with("chosen"))));
    }

    #[test]
    fn a_place_along_a_centre_line() {
        let path = [(0.5, 0.5, 0), (4.5, 0.5, 0), (4.5, 2.5, 276)];
        assert_eq!((along(&path, 0.0), along(&path, 0.5), along(&path, 1.0)), ((0.5, 0.5), (3.5, 0.5), (4.5, 2.5)));
        assert_eq!(along(&path, 5.0 / 6.0), (4.5, 1.5));
        // a stub is one point
        assert_eq!((along(&path[..1], 0.5), along(&[], 0.5)), ((0.5, 0.5), (0.0, 0.0)));
    }

    #[test]
    fn what_could_be_joined_is_drawn_pale_and_numbered() {
        let (lv, found) = (&fixture::seed40().level, fixture::tower_joins());
        let numbered: Vec<(usize, Candidate)> = found.found.iter().cloned().enumerate().map(|(k, c)| (k + 1, c)).collect();
        let best = numbered[..found.rooms].to_vec();
        let pick = Some(("tower".to_string(), 551));
        let view = View { pick: pick.clone(), joins: best.clone(), ..View::default() };
        let pic = picture(lv, &view);
        let plain = picture(lv, &View::default());
        // the level is all there, underneath
        assert_eq!(pic.prims[..plain.prims.len()], plain.prims[..]);
        let over = &pic.prims[plain.prims.len()..];
        // the room is framed and named
        assert_eq!(over.iter().filter(|p| matches!(p, Prim::Line { col, width_px, .. } if *col == colour::PICK && *width_px == 3.0)).count(), 4);
        assert!(texts(over).contains(&"joining tower by a door on 551"));
        // every cell of every candidate is under a pale patch, and each has its number on a disc
        let pale: Vec<Box2> = over
            .iter()
            .filter_map(|p| match p {
                Prim::Poly { pts, fill } if *fill == colour::JOIN && pts.len() == 4 => Some((pts[0].0, pts[0].1, pts[2].0, pts[2].1)),
                _ => None,
            })
            .collect();
        for (n, c) in &best {
            for (_, x, y) in c.cells() {
                let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
                assert!(pale.iter().any(|b| b.0 < x && x < b.2 && b.1 < y && y < b.3), "{n}: no floor at {x}, {y}");
            }
        }
        let discs: Vec<(P, &str, f32)> = over
            .iter()
            .filter_map(|p| match p {
                Prim::Badge { at, text, fill, r_px, .. } if *fill == colour::JOIN_DISC => Some((*at, text.as_str(), *r_px)),
                _ => None,
            })
            .collect();
        assert_eq!(discs.iter().map(|d| d.1).collect::<Vec<_>>(), ["1", "2", "3", "4", "5", "6", "7", "8", "9"]);
        // no disc lies on another, and every one is on the picture
        let r = discs[0].2 / view.scale;
        for (i, a) in discs.iter().enumerate() {
            for b in &discs[i + 1..] {
                assert!((a.0 .0 - b.0 .0).abs().max((a.0 .1 - b.0 .1).abs()) >= 2.0 * r, "{} on {}", a.1, b.1);
            }
            assert!(pic.lo.0 < a.0 .0 && a.0 .0 < pic.hi.0 && pic.lo.1 < a.0 .1 && a.0 .1 < pic.hi.1);
        }
        // a door bar at both ends of each, and the legend says what the pale ones are
        let bars = over.iter().filter(|p| matches!(p, Prim::Line { col, width_px, .. } if *col == colour::JOIN_EDGE && *width_px == 4.0)).count();
        assert_eq!(bars, 2 * best.len());
        let said: Vec<&str> = pic.legend.iter().map(|k| k.text.as_str()).collect();
        assert!(said.contains(&"room picked to join") && said.contains(&"could be made (numbered as listed)"));
        // the one under the pointer is yellow, the others fade, and no disc moves
        let hot = picture(lv, &View { hot: Some(4), ..view.clone() });
        let count = |prims: &[Prim], c: Rgba| prims.iter().filter(|p| matches!(p, Prim::Poly { fill, .. } if *fill == c)).count();
        assert!(count(&hot.prims, colour::HOT) > 0 && count(&hot.prims, colour::JOIN_DIM) > 0 && count(&hot.prims, colour::JOIN) == 0);
        let place = |prims: &[Prim]| {
            prims.iter().filter_map(|p| if let Prim::Badge { at, text, .. } = p { Some((*at, text.clone())) } else { None }).collect::<Vec<_>>()
        };
        assert_eq!(place(&hot.prims), place(&pic.prims));
        assert!(hot.prims.iter().any(|p| matches!(p, Prim::Badge { text, fill, .. } if text == "4" && *fill == colour::MARK)));
        // every one of them is a busier picture, with room made for corridors off the level's edge
        let all = picture(lv, &View { pick, joins: numbered.clone(), ..View::default() });
        assert_eq!(place(&all.prims).len(), found.found.len());
        let (lo, hi) = bounds_of(lv, &View { joins: numbered, ..View::default() });
        let (was_lo, was_hi) = bounds(lv);
        assert!(lo.0 <= was_lo.0 && lo.1 <= was_lo.1 && hi.0 >= was_hi.0 && hi.1 >= was_hi.1);
        assert_eq!((all.lo, all.hi), (lo, hi));
        // one storey: the candidates' own floor there is pale, the rest of each is an outline
        let one = picture(lv, &View { storey: Some(551), ..view });
        assert!(count(&one.prims, colour::JOIN) > 0 && count(&one.prims, colour::JOIN) < count(&pic.prims, colour::JOIN));
    }

    #[test]
    fn a_picked_corridor_is_ringed_and_one_taken_out_is_drawn_where_it_was() {
        let lv = &fixture::seed40().level;
        let r = lv.net.routes().iter().find(|r| r.key() == "c23").unwrap();
        let thick =
            |prims: &[Prim], c: Rgba, w: f32| prims.iter().filter(|p| matches!(p, Prim::Line { col, width_px, .. } if *col == c && *width_px == w)).count();
        let picked = picture(lv, &View { route: Some("c23".into()), ..View::default() });
        assert!(thick(&picked.prims, colour::MARK, 4.0) >= 4);
        assert_eq!(picked.legend.last().unwrap().text, "corridor picked");
        // a key that is not in the level rings nothing
        assert_eq!(thick(&picture(lv, &View { route: Some("c99".into()), ..View::default() }).prims, colour::MARK, 4.0), 0);
        let gone = picture(lv, &View { gone: Some(r.clone()), ..View::default() });
        assert!(thick(&gone.prims, colour::DEAD_END, 3.0) >= 4);
        assert!(texts(&gone.prims).contains(&"c23 taken out") && gone.prims.iter().any(|p| matches!(p, Prim::Poly { fill, .. } if *fill == colour::GONE)));
        assert_eq!(gone.legend.last().unwrap().text, "corridor taken out");
    }
}
