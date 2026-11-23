//! A level recorded as what made it, not as its geometry: a seed, the options, and the edits a
//! person made on the way. New here (the Python has none); the plan is in
//! `docs/AUTOLEVEL.md`, "The recipe".
//!
//! ```text
//! { "format": 1, "seed": 40, "options": { "cover": "pillars" },
//!   "edits": [ { "edit": "join", "room": "tower", "storey": 551, "to": "colossal" },
//!              { "edit": "remove", "route": "c07" } ] }
//! ```
//!
//! `load` and `save` are the file (pretty JSON; options that are the default are left out, so a
//! fresh recipe is four lines), and `Recipe::generate` runs the stages of `level` with each edit
//! applied after the stage it belongs to. With no edits that is exactly
//! `level::generate(seed, &options)`, and the same recipe always gives the same level.
//!
//! Edits name things as the generator does: rooms by their keys ("vast", "small7"), storeys by
//! their floor heights, routes by a piece's key ("c07"). Which stage each belongs to is
//! `Edit::stage`, and each `Edit`'s own comment says what it does and when it is an error:
//!
//! - `move`, `resize`, `add`, `delete`, `pin` (after placing the lots, before they become rooms):
//!   the lots are changed in the recipe's order and stay where they are put, and the set is then
//!   held to the rule the scatter places by (`place::Placed`, `Placed::check`). A hall's pods
//!   move, change storey and go with it; a small room that lands in a hall is its pod;
//! - `join`, `remove` (after routing): what the dead-end pass does itself, by hand;
//! - `cover`, `layout`, `reroll` (told to the furnisher: `build::Choice`), which hold however
//!   often the dead-end pass furnishes the room again;
//! - `keep` (told to the dead-end pass: the dead end that room is in is left alone) and `cut`
//!   (after the pass: a room's own decks stop at a storey);
//! - `brightness`, `ceiling` (on the finished level: they replace the planner's own choice).
//!
//! `"start": "empty"` (`level::Start::Empty`) begins with no rooms: every room is an `add`, and
//! the seed only decorates them (see `Options::stream`). Left out, the start is the seed's own.
//!
//! Later edits see earlier ones: a room that was moved is joined where it now is, and an edit
//! that names a room a `delete` took out is an error that says so. An edited level is held to
//! everything a seed's own is: no room cut off, every room furnished, the plan's check passed.

use crate::build::{takes, Build, JOIN_COST, JOIN_SEARCH, LAYOUTS};
use crate::cells::Rect;
use crate::level::{self, Level, Options, Put, Start, COVER};
use crate::network::{components, Network};
use crate::place::{self, Placed, Scale};
use crate::py;
use crate::route::{Terms, VentTerms};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The format this build writes, and the only one it reads.
pub const FORMAT: u32 = 1;

/// The ceilings a room may be given (the planner's roles).
pub const CEILINGS: [&str; 4] = ["plain", "ceiling_tile", "ceiling_dark", "grille"];

/// The options by name, as the file and a command line spell them, in the file's order.
pub const OPTIONS: [&str; 35] = [
    "style",
    "washroom",
    "scale",
    "size",
    "site",
    "cover",
    "clutter",
    "walkway",
    "crates",
    "shapes",
    "risers",
    "second",
    "vents",
    "windows",
    "drops",
    "grates",
    "platforms",
    "galleries",
    "holes",
    "cellars",
    "angled",
    "flooded",
    "shaped",
    "sunken",
    "bases",
    "big_pods",
    "rescue",
    "dead_ends",
    "dead_corridor",
    "dead_walk",
    "dead_climb",
    "loops",
    "shift",
    "level_change",
    "tries",
];

/// The stage an edit is applied after.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Placing the lots.
    Lots,
    /// Routing.
    Routes,
    /// Furnishing.
    Interiors,
    /// The dead-end pass.
    DeadEnds,
}

/// A doorway a person picked on a room's wall: the wall (E, N, W or S) and the cell along it
/// of the door's low end (x for a north or south wall, y for an east or west one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Doorway {
    pub side: char,
    pub at: i32,
}

impl Doorway {
    pub fn words(&self) -> String {
        let wall = match self.side {
            'E' => "east",
            'N' => "north",
            'W' => "west",
            _ => "south",
        };
        format!("its {wall} wall at {}", self.at)
    }

    /// Why it is no doorway of the room `s` ("medium3 has no doorway N 90 (its walls run 61 to
    /// 67)"), or None: it is one of its walls' cells.
    pub fn off(&self, s: &crate::space::Space) -> Option<String> {
        let (lo, hi) = if matches!(self.side, 'N' | 'S') { (s.x0, s.x1) } else { (s.y0, s.y1) };
        (!matches!(self.side, 'E' | 'N' | 'W' | 'S') || self.at < lo || self.at >= hi)
            .then(|| format!("{} has no doorway {} {} (its walls run {lo} to {})", s.key, self.side, self.at, hi - 1))
    }

    /// "E:26", as the command line takes it.
    pub fn short(&self) -> String {
        format!("{}:{}", self.side, self.at)
    }

    /// "E:26" (or "e:26") read back.
    pub fn parse(text: &str) -> Option<Doorway> {
        let (side, at) = text.split_once(':')?;
        let side = side.trim().to_ascii_uppercase();
        let side = side.chars().next().filter(|c| side.len() == 1 && matches!(c, 'E' | 'N' | 'W' | 'S'))?;
        Some(Doorway { side, at: at.trim().parse().ok()? })
    }
}

/// What a `join` is made as: a corridor (with ramps, or a stub through a wall), a vent, a
/// straight corridor (no turn: the router's own terms with a turn dearer than the whole budget
/// and no limit on a run's length; new here, an override no seed's recipe takes), or (none
/// given) a corridor, failing that a vent.
pub const VIAS: [&str; 3] = ["corridor", "vent", "straight"];

/// One change made by hand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "edit", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    /// A room's low corner goes to this cell, its size kept. A hall's pods move with it. A pod
    /// moved clear of its hall is a room of its own on the storey it was on; a small or tiny
    /// room moved onto a hall is its pod, if its floor is one of the hall's two pod storeys
    /// (no floor changes without a `pin`). Err if the lots then break the scatter's rule.
    Move { room: String, to: (i32, i32) },
    /// A room is this many cells wide and deep, from its low corner (a hall's pods stay where
    /// they are; a spire's spiral is laid out again). Err under 2 x 2, for a spire with a wall
    /// under 8 cells, or if the lots then break the scatter's rule.
    Resize { room: String, size: (i32, i32) },
    /// A room the scatter did not make, after the others: `kind` is one of the scale's (it
    /// decides how the room is dressed and named, not its size), `rect` (x0, y0, x1, y1) in
    /// cells, `floor` a storey's and `top` the ceiling line of its top storey. Err for a key
    /// that is taken or could be a corridor's ("c07"), a kind the scale does not have, under
    /// 2 x 2, heights no room stands at, or if the lots then break the scatter's rule.
    Add { key: String, kind: String, rect: Rect, floor: i32, top: i32 },
    /// A room is left out before anything is routed, a hall's pods with it. Any other edit that
    /// names it (or one of those pods) is then an error that says it was deleted.
    Delete { room: String },
    /// A room's floor is on this storey, its height kept; a hall's pods go as many storeys up
    /// or down with it. A pod pinned out of its hall's height is a room of its own. Err for a
    /// height that is no storey's floor, a top above the highest ceiling, or if the lots then
    /// break the scatter's rule (a pod stays on one of its hall's first two upper storeys).
    Pin { room: String, floor: i32 },
    /// A room's floor is on the storey `floor` and its top on the ceiling line `top`; a hall's
    /// pods go as many storeys up or down as its floor does. The side view's edit, and the only
    /// one that changes a seeded room's height. Err for heights no room stands at, a pod with no
    /// storey to go to, or if the lots then break the scatter's rule. New here: the Python has
    /// none.
    Height { room: String, floor: i32, top: i32 },
    /// A corridor (failing that a vent) from `room`'s storey to the room `to`, which takes the
    /// door on one of the storeys `on` (none given: any it offers). New (2026-11-23, the
    /// editing roadmap's item 4; none of the Python's): `door` and `to_door`, the one doorway
    /// each room may take (none given: the router's choice), and `via`, one of [`VIAS`] (none:
    /// a corridor, failing that a vent). A join without them is made as it always was.
    Join {
        room: String,
        storey: i32,
        to: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on: Vec<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        door: Option<Doorway>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_door: Option<Doorway>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<String>,
    },
    /// The route with a piece of this key is taken out.
    Remove { route: String },
    /// A hall's first upper storey: "ring", "galleries", "mezzanine" or "none", in place of the
    /// one drawn for it. Err for a pod, a room of one storey, a tower or spire, a room too small
    /// for it (`build::takes`), if the room cannot be furnished with it on any attempt, or if
    /// its decks are afterwards cut below it. The room is never given another layout instead.
    Layout { room: String, layout: String },
    /// What stands on a room's floor: one of `level::COVER`, in place of its dress's own (its
    /// shape and pillars stay). "mixed" is what a level of mixed cover would draw for it. Err
    /// for a pod.
    Cover { room: String, cover: String },
    /// The floor brightness a room is baked to, 0 to 255.
    Brightness { room: String, value: i32 },
    /// A room's ceiling: one of `CEILINGS`.
    Ceiling { room: String, ceiling: String },
    /// A room is furnished from its `n`th other stream of random numbers (0: the seed's own):
    /// its dress (cover, shape, pillars) and every attempt at its inside. Its doors stay, and a
    /// spire's spiral, which is laid out before anything is routed. Err for a pod.
    ///
    /// With a `part` (one of `level::PARTS`: "catwalks", "ramps", "cover", "windows"; new here,
    /// the editing roadmap's item 5), only that part is drawn from a stream of its own, and the
    /// rest of the room from the stream it had: the draws the part would have made are made
    /// and thrown away, so the parts after it draw the numbers they did (they still fit
    /// themselves round what the part became). One of each part per room; with no `part` it
    /// is the whole inside, as ever.
    Reroll {
        room: String,
        n: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<String>,
    },
    /// The dead end this room is in is left as it is.
    Keep { room: String },
    /// A room's own decks stop at the storey `above`.
    Cut { room: String, above: i32 },
    /// One of `level::FEATURES` for a single room: `on`, it has it wherever it fits (the
    /// level's chance for it is 1), or not at all; in place of the level's chance, from the
    /// room's own stream as ever, so no other room's draw moves. A room the feature does not fit
    /// (a pod, one too narrow, a cellar under a hall not on the ground) is left without it, and
    /// the room's choices say so. Err for a feature that is none of them. New here.
    Feature { room: String, feature: String, on: bool },
    /// The window into `room` on `storey` whose wall holds the cell `at` (a cell of wall between
    /// the room and a corridor running along it a cell away: `windows::places`) is `as`: "glass",
    /// "rail" (only where the corridor is above the room's floor) or "none". A place the level
    /// left without a window is given one, past the level's limits. New here (the editing
    /// roadmap's item 6); one that does not fit is left out and says why (`Level::unmet`).
    Window {
        room: String,
        storey: i32,
        at: (i32, i32),
        #[serde(rename = "as")]
        kind: String,
    },
    /// What stands on `room`'s floor at the cell `at`: with `as` ("pillar" or "crate"), one is
    /// stood with its low corner there, `size` cells square (none given: a pillar as wide as the
    /// room's colonnade's, a crate one cell), in place of what stood there; with `as` "none", the
    /// pillar or crate over the cell is taken out; with `to`, it is moved so its low corner is on
    /// that cell. Put once the room's own cover is laid, so it moves no draw; one that does not
    /// fit (off the floor, before a door, on a deck's edge, cutting a door off) is left out and
    /// says why. Err without one of `as` and `to`. New here.
    Piece {
        room: String,
        at: (i32, i32),
        #[serde(rename = "as", default, skip_serializing_if = "Option::is_none")]
        what: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<(i32, i32)>,
    },
    /// `room`'s drop: a hole two cells square through its floor with its low corner on the cell
    /// `at`, into the room under it, in place of the one the generator gave it (a room has one at
    /// most); none given, it has none. A cellar's hatch stays. One that does not fit (by a door,
    /// nothing under it, a room between, too far to fall) is left out and says why. New here.
    Drop {
        room: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<(i32, i32)>,
    },
}

impl Edit {
    /// Its name in the file.
    pub fn name(&self) -> &'static str {
        match self {
            Edit::Move { .. } => "move",
            Edit::Resize { .. } => "resize",
            Edit::Add { .. } => "add",
            Edit::Delete { .. } => "delete",
            Edit::Pin { .. } => "pin",
            Edit::Height { .. } => "height",
            Edit::Join { .. } => "join",
            Edit::Remove { .. } => "remove",
            Edit::Layout { .. } => "layout",
            Edit::Cover { .. } => "cover",
            Edit::Brightness { .. } => "brightness",
            Edit::Ceiling { .. } => "ceiling",
            Edit::Reroll { .. } => "reroll",
            Edit::Keep { .. } => "keep",
            Edit::Cut { .. } => "cut",
            Edit::Feature { .. } => "feature",
            Edit::Window { .. } => "window",
            Edit::Piece { .. } => "piece",
            Edit::Drop { .. } => "drop",
        }
    }

    /// The stage it belongs to: a level is made again from there.
    pub fn stage(&self) -> Stage {
        match self {
            Edit::Move { .. } | Edit::Resize { .. } | Edit::Add { .. } | Edit::Delete { .. } | Edit::Pin { .. } | Edit::Height { .. } => Stage::Lots,
            Edit::Join { .. } | Edit::Remove { .. } => Stage::Routes,
            Edit::Layout { .. } | Edit::Cover { .. } | Edit::Brightness { .. } | Edit::Ceiling { .. } | Edit::Reroll { .. } => Stage::Interiors,
            // (drops are chosen as the rooms are furnished, windows once they are)
            Edit::Window { .. } | Edit::Piece { .. } | Edit::Drop { .. } => Stage::Interiors,
            Edit::Keep { .. } | Edit::Cut { .. } => Stage::DeadEnds,
            // (the room's shape, water, pit and cellar are rolled with the rooms; the rest by its furnisher)
            Edit::Feature { feature, .. } => match crate::level::FEATURES.iter().find(|(f, _)| f == feature) {
                Some((_, true)) => Stage::Lots,
                _ => Stage::Interiors,
            },
        }
    }

    /// Does `generate` apply it? Every edit, now.
    pub fn supported(&self) -> bool {
        true
    }

    /// The rooms it names (`add`: the one it makes; `remove`: none, a route is not a room).
    pub fn rooms(&self) -> Vec<&str> {
        match self {
            Edit::Join { room, to, .. } => vec![room, to],
            Edit::Add { key, .. } => vec![key],
            Edit::Remove { .. } => Vec::new(),
            Edit::Move { room, .. }
            | Edit::Resize { room, .. }
            | Edit::Delete { room }
            | Edit::Pin { room, .. }
            | Edit::Height { room, .. }
            | Edit::Layout { room, .. }
            | Edit::Cover { room, .. }
            | Edit::Brightness { room, .. }
            | Edit::Ceiling { room, .. }
            | Edit::Reroll { room, .. }
            | Edit::Keep { room }
            | Edit::Cut { room, .. }
            | Edit::Feature { room, .. }
            | Edit::Window { room, .. }
            | Edit::Piece { room, .. }
            | Edit::Drop { room, .. } => vec![room],
        }
    }

    /// One line for a list: "join tower on 551 to colossal".
    pub fn describe(&self) -> String {
        match self {
            Edit::Move { room, to } => format!("move {room} to {}, {}", to.0, to.1),
            Edit::Resize { room, size } => format!("resize {room} to {} x {}", size.0, size.1),
            Edit::Add { key, kind, rect, floor, top } => {
                format!("add {key} ({kind}) at {}, {} to {}, {}, from {floor} to {top}", rect.0, rect.1, rect.2, rect.3)
            }
            Edit::Delete { room } => format!("delete {room}"),
            Edit::Pin { room, floor } => format!("pin {room}'s floor at {floor}"),
            Edit::Height { room, floor, top } => format!("{room} from {floor} up to {top}"),
            Edit::Join { room, storey, to, on, door, to_door, via } => {
                let mut s = format!("join {room} on {storey} to {to}");
                if !on.is_empty() {
                    s += &format!(" (on {})", list(on));
                }
                if let Some(d) = door {
                    s += &format!(", from {}", d.words());
                }
                if let Some(d) = to_door {
                    s += &format!(", to {}", d.words());
                }
                if let Some(v) = via {
                    s += &format!(", by a {v}");
                }
                s
            }
            Edit::Remove { route } => format!("remove {route}"),
            Edit::Layout { room, layout } => format!("{room}: layout {layout}"),
            Edit::Cover { room, cover } => format!("{room}: cover {cover}"),
            Edit::Brightness { room, value } => format!("{room}: brightness {value}"),
            Edit::Ceiling { room, ceiling } => format!("{room}: ceiling {ceiling}"),
            Edit::Reroll { room, n, part: None } => format!("{room}: inside {n}"),
            Edit::Reroll { room, n, part: Some(part) } => format!("{room}: {part} {n}"),
            Edit::Keep { room } => format!("keep {room}'s dead end"),
            Edit::Cut { room, above } => format!("cut {room}: no deck of its own above {above}"),
            Edit::Feature { room, feature, on } => format!("{room}: {feature} {}", if *on { "on" } else { "off" }),
            Edit::Window { room, storey, at, kind } => format!("{room}: window at {}, {} on {storey}: {kind}", at.0, at.1),
            Edit::Piece { room, at, to: Some(to), .. } => format!("{room}: the piece at {}, {} moved to {}, {}", at.0, at.1, to.0, to.1),
            Edit::Piece { room, at, what, size, .. } => {
                let what = what.as_deref().unwrap_or("?");
                match (what, size) {
                    ("none", _) => format!("{room}: nothing at {}, {}", at.0, at.1),
                    (_, Some(n)) => format!("{room}: a {what} at {}, {} ({n} cells)", at.0, at.1),
                    _ => format!("{room}: a {what} at {}, {}", at.0, at.1),
                }
            }
            Edit::Drop { room, at: Some(at) } => format!("{room}: a drop at {}, {}", at.0, at.1),
            Edit::Drop { room, at: None } => format!("{room}: no drop"),
        }
    }
}

fn list(zs: &[i32]) -> String {
    zs.iter().map(|z| z.to_string()).collect::<Vec<_>>().join(", ")
}

/// What makes a level.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub seed: u64,
    pub options: Options,
    /// In the order they were made. Each is applied after its stage, those of one stage in this order.
    pub edits: Vec<Edit>,
}

/// The options as the file holds them: every one may be left out.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Opts {
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    washroom: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site: Option<(i32, i32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clutter: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    walkway: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crates: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shapes: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    risers: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    second: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vents: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    windows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    drops: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    grates: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    platforms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    galleries: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    holes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cellars: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    angled: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    flooded: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shaped: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sunken: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bases: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    big_pods: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rescue: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_ends: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_corridor: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_walk: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_climb: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loops: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shift: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level_change: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tries: Option<usize>,
}

impl Opts {
    /// The options that differ from `base` (the defaults, for a file), or with `all` every one.
    fn of(o: &Options, base: &Options, all: bool) -> Opts {
        fn pick<T: PartialEq>(all: bool, v: T, was: T) -> Option<T> {
            (all || v != was).then_some(v)
        }
        let (s, b) = (&o.siting, &base.siting);
        Opts {
            style: pick(all, o.style, base.style).map(str::to_string),
            washroom: pick(all, o.washroom, base.washroom),
            scale: pick(all, s.scale, b.scale).map(|v| v.name().to_string()),
            size: pick(all, s.size, b.size),
            site: if s.site != b.site || all { s.site } else { None },
            cover: pick(all, o.cover, base.cover).map(str::to_string),
            clutter: pick(all, o.clutter, base.clutter),
            walkway: pick(all, o.walkway, base.walkway),
            crates: pick(all, o.crates, base.crates),
            shapes: pick(all, o.shapes, base.shapes),
            risers: pick(all, o.risers, base.risers),
            second: pick(all, o.second, base.second),
            vents: pick(all, o.vents, base.vents),
            windows: pick(all, o.windows, base.windows),
            drops: pick(all, o.drops, base.drops),
            grates: pick(all, o.grates, base.grates),
            platforms: pick(all, o.platforms, base.platforms),
            galleries: pick(all, o.galleries, base.galleries),
            holes: pick(all, o.holes, base.holes),
            cellars: pick(all, o.cellars, base.cellars),
            angled: pick(all, o.angled, base.angled),
            flooded: pick(all, o.flooded, base.flooded),
            shaped: pick(all, o.shaped, base.shaped),
            sunken: pick(all, o.sunken, base.sunken),
            bases: pick(all, o.bases, base.bases),
            big_pods: pick(all, o.big_pods, base.big_pods),
            rescue: pick(all, o.rescue, base.rescue),
            dead_ends: pick(all, o.dead_ends, base.dead_ends),
            dead_corridor: pick(all, o.dead_corridor, base.dead_corridor),
            dead_walk: pick(all, o.dead_walk, base.dead_walk),
            dead_climb: pick(all, o.dead_climb, base.dead_climb),
            loops: pick(all, o.loops, base.loops),
            shift: pick(all, s.shift, b.shift),
            level_change: pick(all, o.level_change, base.level_change),
            tries: pick(all, o.tries, base.tries),
        }
    }

    /// Set on `o` every option this holds. Err for a scale or a cover nobody knows.
    fn apply(self, o: &mut Options) -> Result<(), String> {
        fn set<T>(to: &mut T, v: Option<T>) {
            if let Some(v) = v {
                *to = v;
            }
        }
        if let Some(name) = self.style {
            o.style = crate::level::STYLES
                .into_iter()
                .find(|s| *s == name)
                .ok_or_else(|| format!("style: one of ({})", crate::level::STYLES.map(py::repr).join(", ")))?;
        }
        set(&mut o.washroom, self.washroom);
        if let Some(name) = self.scale {
            o.siting.scale = [Scale::Grand, Scale::Standard, Scale::Towering]
                .into_iter()
                .find(|s| s.name() == name)
                .ok_or("scale: one of ('grand', 'standard', 'towering')")?;
        }
        if let Some(name) = self.cover {
            o.cover = COVER.into_iter().find(|c| *c == name).ok_or_else(|| format!("cover: one of ({})", COVER.map(py::repr).join(", ")))?;
        }
        set(&mut o.siting.size, self.size);
        if self.site.is_some() {
            o.siting.site = self.site;
        }
        set(&mut o.siting.shift, self.shift);
        set(&mut o.clutter, self.clutter);
        set(&mut o.walkway, self.walkway);
        set(&mut o.crates, self.crates);
        set(&mut o.shapes, self.shapes);
        set(&mut o.risers, self.risers);
        set(&mut o.second, self.second);
        set(&mut o.vents, self.vents);
        set(&mut o.windows, self.windows);
        set(&mut o.drops, self.drops);
        set(&mut o.grates, self.grates);
        set(&mut o.platforms, self.platforms);
        set(&mut o.galleries, self.galleries);
        set(&mut o.holes, self.holes);
        set(&mut o.cellars, self.cellars);
        set(&mut o.angled, self.angled);
        set(&mut o.flooded, self.flooded);
        set(&mut o.shaped, self.shaped);
        set(&mut o.sunken, self.sunken);
        set(&mut o.bases, self.bases);
        set(&mut o.big_pods, self.big_pods);
        set(&mut o.rescue, self.rescue);
        set(&mut o.dead_ends, self.dead_ends);
        set(&mut o.dead_corridor, self.dead_corridor);
        set(&mut o.dead_walk, self.dead_walk);
        set(&mut o.dead_climb, self.dead_climb);
        set(&mut o.loops, self.loops);
        set(&mut o.level_change, self.level_change);
        set(&mut o.tries, self.tries);
        Ok(())
    }
}

/// The file.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    seed: u64,
    #[serde(default, skip_serializing_if = "is_seed")]
    start: Start,
    #[serde(default)]
    options: Opts,
    #[serde(default)]
    edits: Vec<Edit>,
}

fn is_seed(s: &Start) -> bool {
    *s == Start::Seed
}

/// A recipe from its file's text. Err, saying what, for text that is not JSON, a format this
/// build does not read, or a field, option or edit it does not know.
pub fn load(text: &str) -> Result<Recipe, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("recipe: {e}"))?;
    match v.get("format") {
        None => return Err(format!("recipe: no \"format\" (this build reads format {FORMAT})")),
        Some(f) if f.as_u64() != Some(FORMAT as u64) => return Err(format!("recipe: format {f} is not one this build reads (format {FORMAT})")),
        _ => {}
    }
    let file: File = serde_json::from_str(text).map_err(|e| format!("recipe: {e}"))?;
    let mut options = Options::default();
    file.options.apply(&mut options).map_err(|e| format!("recipe: {e}"))?;
    options.start = file.start;
    Ok(Recipe { seed: file.seed, options, edits: file.edits })
}

/// A recipe as its file's text: pretty JSON, the fields always in the same order, and only the
/// options that are not the default.
pub fn save(r: &Recipe) -> String {
    let file = File { format: FORMAT, seed: r.seed, start: r.options.start, options: Opts::of(&r.options, &Options::default(), false), edits: r.edits.clone() };
    serde_json::to_string_pretty(&file).expect("a recipe is plain data") + "\n"
}

/// Set one option by its name (`OPTIONS`; a hyphen will do for the underscore) from text, as a
/// command line or a text field gives it: "pillars", "0.3", "true", "80x60" (a site; "default"
/// for the scale's own).
pub fn set_option(o: &mut Options, name: &str, value: &str) -> Result<(), String> {
    let name = name.replace('-', "_");
    if !OPTIONS.contains(&name.as_str()) {
        return Err(format!("no option {name}: one of {}", OPTIONS.join(", ")));
    }
    let value = value.trim();
    if name == "site" {
        if matches!(value, "" | "default" | "none" | "null") {
            o.siting.site = None;
            return Ok(());
        }
        let size: Vec<i32> = value.split(['x', 'X', ',']).filter_map(|v| v.trim().parse().ok()).collect();
        return match size[..] {
            [w, h] if w > 0 && h > 0 && value.split(['x', 'X', ',']).count() == 2 => {
                o.siting.site = Some((w, h));
                Ok(())
            }
            _ => Err(format!("site: cells wide by cells deep, as 80x60, not {value}")),
        };
    }
    let v: Value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()));
    let mut one = serde_json::Map::new();
    one.insert(name.clone(), v);
    let opts: Opts = serde_json::from_value(Value::Object(one)).map_err(|e| format!("{name}: {e}"))?;
    opts.apply(o)
}

/// An option's value as text (what `set_option` takes), by its name.
pub fn option(o: &Options, name: &str) -> Option<String> {
    let all = serde_json::to_value(Opts::of(o, o, true)).expect("options are plain data");
    Some(match all.get(name.replace('-', "_")) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("x"),
        Some(v) => v.to_string(),
        None if name == "site" => "default".to_string(),
        None => return None,
    })
}

impl Recipe {
    /// A seed's level as it falls: the default options and no edits.
    pub fn new(seed: u64) -> Recipe {
        Recipe { seed, options: Options::default(), edits: Vec::new() }
    }

    /// A seed's level by `Options::python`: what a test pins.
    pub fn python(seed: u64) -> Recipe {
        Recipe { seed, options: Options::python(), edits: Vec::new() }
    }

    /// A seed's level by the defaults with bases and grown pods left out (`Options::before_bases`).
    pub fn before_bases(seed: u64) -> Recipe {
        Recipe { seed, options: Options::before_bases(), edits: Vec::new() }
    }

    /// An empty start: no rooms until they are added, the seed only for how they are decorated.
    pub fn empty(seed: u64) -> Recipe {
        Recipe { seed, options: Options { start: Start::Empty, ..Options::default() }, edits: Vec::new() }
    }

    /// The level this makes. Err if the options make no sense, the seed can't be finished, or an
    /// edit can't be applied (it names something that is not there, the lots would break the
    /// scatter's rule, no corridor fits, it cuts the level in two, a room can't be furnished as
    /// chosen).
    pub fn generate(&self) -> Result<Level, String> {
        if self.options.style != "complex" {
            self.options.check()?;
            if let Some(e) = self.edits.first() {
                return Err(format!("{}: a {} level takes no edits yet", e.describe(), self.options.style));
            }
            return level::generate(self.seed, &self.options);
        }
        let made = self.furnished().and_then(|(b, dropped)| self.finish(b, dropped));
        match (made, self.plain()) {
            (Err(why), Some(plain)) => plain.generate().map_err(|_| why),
            (made, _) => made,
        }
    }

    /// The options the stages are made with: the recipe's, with what its `feature` edits chose
    /// for single rooms (`Options::forced`), its part re-rolls (`rerolled`) and the things it put
    /// by place (`puts`: `window`, `piece`, `drop`). The recipe's own when it has none. Err for a
    /// feature that is none of `level::FEATURES`.
    pub fn effective(&self) -> Result<std::borrow::Cow<'_, Options>, String> {
        let (mut forced, mut rerolled, mut puts) = (Vec::new(), Vec::new(), Vec::new());
        for e in &self.edits {
            match e {
                Edit::Feature { room, feature, on } => {
                    let Some(&(name, _)) = level::FEATURES.iter().find(|(f, _)| f == feature) else {
                        let names: Vec<&str> = level::FEATURES.iter().map(|(f, _)| *f).collect();
                        return Err(format!("feature: one of {}, not {feature:?}", names.join(", ")));
                    };
                    forced.push((room.clone(), name, *on));
                }
                Edit::Reroll { room, n, part: Some(part) } => {
                    let Some(&name) = level::PARTS.iter().find(|p| *p == part) else {
                        return Err(format!("reroll: a part is one of {}, not {part:?}", level::PARTS.join(", ")));
                    };
                    rerolled.push((room.clone(), name, *n));
                }
                e @ (Edit::Window { .. } | Edit::Piece { .. } | Edit::Drop { .. }) => puts.push(put_of(e)?),
                _ => {}
            }
        }
        if forced.is_empty() && rerolled.is_empty() && puts.is_empty() {
            return Ok(std::borrow::Cow::Borrowed(&self.options));
        }
        Ok(std::borrow::Cow::Owned(Options { forced, rerolled, puts, ..self.options.clone() }))
    }

    /// The recipe with the generator's own bases and grown pods left out, if it has any to leave
    /// out (a seed's level with `Options::bases` or `big_pods` on): what a level that cannot be
    /// made with them is made from instead, as the rescue makes every seed a level (new in the
    /// Rust: docs/AUTOLEVEL.md, "Bases").
    pub fn plain(&self) -> Option<Recipe> {
        let o = &self.options;
        (o.start == Start::Seed && (o.bases || o.big_pods)).then(|| {
            let mut r = self.clone();
            (r.options.bases, r.options.big_pods) = (false, false);
            r
        })
    }

    /// The first three stages, with the edits that belong to them: every room furnished, before
    /// anything is done about dead ends. With it come the pods `stage_spaces` left out.
    /// (`finish` is the rest. The editor stops here to list the dead ends as the rooms fall.)
    pub fn furnished(&self) -> Result<(Build, Vec<String>), String> {
        match (self.furnished_as_drawn(), self.plain()) {
            (Err(why), Some(plain)) => plain.furnished().map_err(|_| why),
            (made, _) => made,
        }
    }

    /// `furnished`, with no fallback.
    fn furnished_as_drawn(&self) -> Result<(Build, Vec<String>), String> {
        let o = self.effective()?;
        let (seed, o) = (self.seed, o.as_ref());
        let (rooms, net) = self.routed()?;
        // (`level::stage_interiors`, with what was chosen for a room told to the build first)
        let mut b = Build::new(seed, o.clone(), net, rooms.access, rooms.spin);
        for e in &self.edits {
            match e {
                Edit::Cover { room, cover } => {
                    let cover = COVER.into_iter().find(|c| c == cover).ok_or_else(|| format!("cover: one of ({})", COVER.map(py::repr).join(", ")))?;
                    b.choose("cover", room)?.cover = Some(cover);
                }
                Edit::Layout { room, layout } => {
                    let lay = match layout.as_str() {
                        "none" => None,
                        name => Some(
                            LAYOUTS.into_iter().find(|l| *l == name).ok_or_else(|| format!("layout: one of ({}, 'none')", LAYOUTS.map(py::repr).join(", ")))?,
                        ),
                    };
                    b.choose("layout", room)?;
                    let s = b.spaces().iter().find(|s| s.key == *room).expect("a room that was chosen for");
                    takes(s, lay.unwrap_or("none")).map_err(|why| format!("layout: {why}"))?;
                    b.choose("layout", room)?.layout = Some(lay);
                }
                Edit::Reroll { room, n, part: None } => b.choose("reroll", room)?.reroll = *n,
                // (told through `Options::rerolled`, by `effective`; a pod is refused here)
                Edit::Reroll { room, part: Some(_), .. } => drop(b.choose("reroll", room)?),
                _ => {}
            }
        }
        b.refresh()?;
        Ok((b, rooms.dropped))
    }

    /// The first two stages, with the edits that belong to them: the rooms, and everything that
    /// joins them, before any room is furnished. (`furnished` goes on from here. `candidates`
    /// stops here to try the joins a person could make next, on the network a `join` edit added
    /// to the end of the list would be applied to.)
    pub fn routed(&self) -> Result<(level::Spaces, Network), String> {
        let o = self.effective()?;
        let (seed, o) = (self.seed, o.as_ref());
        let (_, rooms) = self.rooms()?;
        let mut net = level::stage_network(seed, o, &rooms.sp)?;
        for e in &self.edits {
            match e {
                Edit::Join { room, storey, to, on, door, to_door, via } => join(&mut net, o, room, *storey, to, on, (*door, *to_door), via.as_deref())?,
                Edit::Remove { route } => remove(&mut net, route)?,
                _ => {}
            }
        }
        Ok((rooms, net))
    }

    /// The first stage, with the edits that belong to it: the lots as placed and the rooms they
    /// make, before anything is routed. Everything `routed` refuses before it routes is refused
    /// here, and it is quick (no corridor is looked for).
    pub fn rooms(&self) -> Result<(Placed, level::Spaces), String> {
        self.options.check()?;
        self.rooms_on(self.scattered()?)
    }

    /// `rooms`, from the seed's own lots already placed (`scattered`): for a caller that tries
    /// one change after another on the same seed and options (the editor, as a box is dragged).
    pub fn rooms_on(&self, base: Placed) -> Result<(Placed, level::Spaces), String> {
        let o = self.effective()?;
        let (seed, o) = (self.seed, o.as_ref());
        o.check()?;
        let own = base.lots.clone();
        let placed = self.lots_on(base)?;
        if placed.lots.is_empty() {
            return Err("no room yet: an empty start (draw one)".to_string());
        }
        if placed.changed() {
            placed.check(o.band())?;
        }
        let rooms = level::stage_rooms(seed, o, &placed.lots);
        if placed.changed() {
            // a pod with no way in is left out, as ever, but not one a person put there: only
            // those the seed's own level leaves out, where they were left
            let own = level::stage_rooms(seed, o, &own).dropped;
            if let Some(pod) = rooms.dropped.iter().find(|k| placed.touched.contains(*k) || !own.contains(k)) {
                let hall = placed.lots.iter().find(|a| a.key == *pod).map_or("", |a| a.host.as_str());
                return Err(format!("{pod} has no way in from {hall}: a pod needs room in its hall for a door, and a deck from it to one of the hall's walls"));
            }
        }
        for e in self.edits.iter().filter(|e| e.stage() != Stage::Lots) {
            for room in e.rooms() {
                if let Some((_, how)) = placed.gone.iter().find(|(k, _)| k == room) {
                    return Err(format!("{}: {room} was {how}", e.name()));
                }
                if rooms.dropped.iter().any(|k| k == room) {
                    return Err(format!("{}: {room} is a pod with no way in, left out of the level", e.name()));
                }
            }
        }
        Ok((placed, rooms))
    }

    /// The lots as placed, with the edits that belong there made in the recipe's order, and
    /// the set then held to the scatter's rule (`Placed::check`). With no such edit they are
    /// the seed's own, unchecked.
    pub fn placed(&self) -> Result<Placed, String> {
        let p = self.lots_on(self.scattered()?)?;
        if p.changed() {
            p.check(self.options.band())?;
        }
        Ok(p)
    }

    /// The seed's own lots as the scatter, the settling and the drawing together leave them:
    /// what the recipe's lot edits are made on. Err if the site has no room for a lot.
    pub fn scattered(&self) -> Result<Placed, String> {
        let o = &self.options;
        if o.start == Start::Empty {
            return Ok(Placed::empty(&o.siting));
        }
        Ok(Placed::new(place::lots(self.seed, &o.siting)?.placed, &o.siting))
    }

    /// The recipe's lot edits made on `base` (the seed's own lots: `scattered`) in the recipe's
    /// order, the set **not** held to the scatter's rule: `Placed::check` is the caller's, so
    /// that lots that break it can still be shown where they are. Err if an edit itself cannot
    /// be made (it names no room, a size under 2 x 2, a floor that is no storey's).
    pub fn lots_on(&self, base: Placed) -> Result<Placed, String> {
        let mut p = base;
        for e in &self.edits {
            match e {
                Edit::Move { room, to } => p.move_to(room, *to)?,
                Edit::Resize { room, size } => p.resize(room, *size)?,
                Edit::Add { key, kind, rect, floor, top } => p.add(key, kind, *rect, *floor, *top)?,
                Edit::Delete { room } => p.delete(room)?,
                Edit::Pin { room, floor } => p.pin(room, *floor)?,
                Edit::Height { room, floor, top } => p.height(room, *floor, *top)?,
                _ => {}
            }
        }
        Ok(p)
    }

    /// The dead-end pass and the level, from `furnished`'s build, with the edits that belong there.
    pub fn finish(&self, b: Build, dropped: Vec<String>) -> Result<Level, String> {
        match (self.finish_as_drawn(b, dropped), self.plain()) {
            (Err(why), Some(plain)) => plain.furnished().and_then(|(b, dropped)| plain.finish(b, dropped)).map_err(|_| why),
            (made, _) => made,
        }
    }

    /// `finish`, with no fallback.
    fn finish_as_drawn(&self, mut b: Build, dropped: Vec<String>) -> Result<Level, String> {
        let known = |b: &Build, what: &str, room: &str| match b.spaces().iter().any(|s| s.key == room) {
            true => Ok(()),
            false => Err(format!("{what}: no room {room}")),
        };
        for e in &self.edits {
            if let Edit::Keep { room } = e {
                known(&b, "keep", room)?;
                b.keep.insert(room.clone());
            }
        }
        let mut b = level::stage_dead_ends(b)?;
        for e in &self.edits {
            if let Edit::Cut { room, above } = e {
                b.cut(room, *above)?;
            }
        }
        // a layout chosen by hand is on the room's first upper storey: it must still be there
        for (room, choice) in &b.choices {
            let (Some(Some(lay)), Some(&top)) = (choice.layout, b.caps.get(room)) else { continue };
            if b.spaces().iter().any(|s| s.key == *room && s.storeys().get(1).is_some_and(|&z| z > top)) {
                return Err(format!(
                    "layout: {room} keeps no {lay}: its decks are cut above {top} (by a cut, or by the dead-end pass: keep {room}'s dead end)"
                ));
            }
        }
        let mut lv = level::stage_level(b, dropped)?;
        for e in &self.edits {
            match e {
                Edit::Brightness { room, value } => {
                    if lv.spaces().iter().all(|s| s.key != *room) {
                        return Err(format!("brightness: no room {room}"));
                    }
                    if !(0..=255).contains(value) {
                        return Err(format!("brightness: 0 to 255, not {value}"));
                    }
                    put(&mut lv.moods, room, *value);
                }
                Edit::Ceiling { room, ceiling } => {
                    if lv.spaces().iter().all(|s| s.key != *room) {
                        return Err(format!("ceiling: no room {room}"));
                    }
                    let role = CEILINGS.into_iter().find(|c| c == ceiling).ok_or_else(|| format!("ceiling: one of ({})", CEILINGS.map(py::repr).join(", ")))?;
                    put(&mut lv.ceilings, room, role);
                }
                _ => {}
            }
        }
        Ok(lv)
    }
}

/// A `window`, `piece` or `drop` edit as the stages are told it (`level::Put`). Err for what it
/// cannot be (a window as a door, a piece neither stood nor moved, a size out of range).
pub fn put_of(e: &Edit) -> Result<Put, String> {
    match e {
        Edit::Window { room, storey, at, kind } => {
            let kind = level::WINDOW_KINDS
                .into_iter()
                .find(|k| k == kind)
                .ok_or_else(|| format!("window: as one of {}, not {kind:?}", level::WINDOW_KINDS.join(", ")))?;
            Ok(Put::Window { room: room.clone(), z: *storey, at: *at, kind })
        }
        Edit::Piece { room, at, what, size, to } => match (what.as_deref(), to) {
            (None, Some(to)) if size.is_none() => Ok(Put::Shift { room: room.clone(), at: *at, to: Some(*to) }),
            (Some("none"), None) if size.is_none() => Ok(Put::Shift { room: room.clone(), at: *at, to: None }),
            (Some(what), None) if what != "none" => {
                let what =
                    level::PIECES[..2].iter().find(|p| **p == what).ok_or_else(|| format!("piece: as one of {}, not {what:?}", level::PIECES.join(", ")))?;
                if let Some(n) = size.filter(|n| !(1..=3).contains(n)) {
                    return Err(format!("piece: a size of 1 to 3 cells, not {n}"));
                }
                Ok(Put::Stand { room: room.clone(), at: *at, what, size: *size })
            }
            _ => Err("piece: one of \"as\" (pillar, crate, none) and \"to\" (where it moves), and a size only with a pillar or crate".to_string()),
        },
        Edit::Drop { room, at } => Ok(Put::Drop { room: room.clone(), at: *at }),
        _ => Err(format!("{}: not a thing put by place", e.name())),
    }
}

/// Set a room's entry in one of the level's lists, in its place if it has one.
fn put<T>(rows: &mut Vec<(String, T)>, room: &str, v: T) {
    match rows.iter_mut().find(|(k, _)| k == room) {
        Some(row) => row.1 = v,
        None => rows.push((room.to_string(), v)),
    }
}

/// A join by hand, on the dead-end pass's terms (`Build::join`): a corridor from `room`'s storey
/// to `to`, a vent if none fits and the level has vents.
pub(crate) fn join(
    net: &mut Network,
    o: &Options,
    room: &str,
    storey: i32,
    to: &str,
    on: &[i32],
    doors: (Option<Doorway>, Option<Doorway>),
    via: Option<&str>,
) -> Result<(), String> {
    let find = |key: &str| net.spaces().iter().find(|s| s.key == key).ok_or_else(|| format!("join: no room {key}"));
    let (a, b) = (find(room)?, find(to)?);
    if room == to {
        return Err(format!("join: {room} to itself"));
    }
    for s in [a, b] {
        if !s.host.is_empty() {
            return Err(format!("join: {} is a pod, joined inside {}", s.key, s.host));
        }
    }
    if !a.door_levels().contains(&storey) {
        return Err(format!("join: {room} takes doors on {}, not on {storey}", list(&a.door_levels())));
    }
    if let Some(z) = on.iter().find(|z| !b.door_levels().contains(z)) {
        return Err(format!("join: {to} takes doors on {}, not on {z}", list(&b.door_levels())));
    }
    if let Some(v) = via.filter(|v| !VIAS.contains(v)) {
        return Err(format!("join: by a {v}? (a corridor, a vent or straight)"));
    }
    for (d, s) in [(doors.0, a), (doors.1, b)] {
        if let Some(why) = d.and_then(|d| d.off(s)) {
            return Err(format!("join: {why}"));
        }
    }
    let (da, db) = (doors.0.map(|d| (d.side, d.at)), doors.1.map(|d| (d.side, d.at)));
    let lb = (!on.is_empty()).then(|| on.to_vec());
    let mut terms = Terms { max_cost: JOIN_COST, la: Some(vec![storey]), lb: lb.clone(), da, db, search: JOIN_SEARCH, storeyed: true, ..Terms::default() };
    if via == Some("straight") {
        // (a turn costs more than a corridor may: none is taken; and a run may be any length)
        (terms.turn, terms.straight) = (JOIN_COST + 1.0, 0);
    }
    let mut r = if via == Some("vent") { None } else { net.grid.route(a, b, &terms) };
    let vents = o.vents != 0 || via == Some("vent");
    if r.is_none() && matches!(via, None | Some("vent")) && vents && a.vent_levels().contains(&storey) {
        r = net.grid.vent(a, b, &VentTerms { la: Some(vec![storey]), lb, da, db, ..VentTerms::default() });
    }
    let what = match via {
        Some("vent") => "vent",
        Some("straight") => "straight corridor",
        _ => "corridor",
    };
    let through = match doors {
        (None, None) => String::new(),
        _ => " through the doorways asked for".to_string(),
    };
    let r = r.ok_or_else(|| format!("join: no {what} fits from {room} on {storey} to {to}{through}"))?;
    net.grid.commit(r);
    Ok(())
}

/// Take out the route that has a piece of this key. Err if there is none (a corridor the
/// dead-end pass adds is not there yet: `keep` its dead end instead), or the level would be in two.
fn remove(net: &mut Network, key: &str) -> Result<(), String> {
    let r = net.routes().iter().find(|r| r.pieces.iter().any(|pc| pc.key == key)).cloned();
    let r = r.ok_or_else(|| format!("remove: no route {key} after routing (one the dead-end pass adds is stopped by keeping its dead end)"))?;
    net.grid.remove(&r);
    if components(net) != 1 {
        return Err(format!("remove: without {key} the level is in two"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::Fix;

    fn every_edit() -> Vec<Edit> {
        let s = |s: &str| s.to_string();
        vec![
            Edit::Move { room: s("medium3"), to: (12, 30) },
            Edit::Resize { room: s("medium3"), size: (8, 6) },
            Edit::Add { key: s("extra1"), kind: s("medium"), rect: (2, 3, 9, 8), floor: 0, top: 236 },
            Edit::Delete { room: s("tiny4") },
            Edit::Pin { room: s("loft1"), floor: 551 },
            Edit::Join { room: s("tower"), storey: 551, to: s("colossal"), on: vec![276, 551], door: None, to_door: None, via: None },
            Edit::Join { room: s("tower"), storey: 0, to: s("vast"), on: vec![], door: None, to_door: None, via: None },
            Edit::Remove { route: s("c07") },
            Edit::Layout { room: s("colossal"), layout: s("ring") },
            Edit::Cover { room: s("vast"), cover: s("pillars") },
            Edit::Brightness { room: s("vast"), value: 128 },
            Edit::Ceiling { room: s("vast"), ceiling: s("grille") },
            Edit::Reroll { room: s("vast"), n: 2, part: None },
            Edit::Keep { room: s("tiny3") },
            Edit::Cut { room: s("spire1"), above: 827 },
            Edit::Height { room: s("medium3"), floor: 276, top: 787 },
            Edit::Feature { room: s("vast"), feature: s("flooded"), on: true },
            Edit::Window { room: s("medium1"), storey: 276, at: (49, 30), kind: s("rail") },
            Edit::Piece { room: s("vast"), at: (40, 30), what: Some(s("crate")), size: None, to: None },
            Edit::Piece { room: s("vast"), at: (41, 25), what: None, size: None, to: Some((40, 32)) },
            Edit::Drop { room: s("colossal"), at: None },
        ]
    }

    #[test]
    fn a_fresh_recipe_is_short_and_comes_back_the_same() {
        let r = Recipe::new(40);
        let text = save(&r);
        assert_eq!(text, "{\n  \"format\": 1,\n  \"seed\": 40,\n  \"options\": {},\n  \"edits\": []\n}\n");
        assert_eq!(load(&text).unwrap(), r);
        // options and edits may be left out of a file
        assert_eq!(load("{\"format\": 1, \"seed\": 40}").unwrap(), r);
    }

    #[test]
    fn every_option_and_edit_loads_and_saves() {
        let mut o = Options::before_bases();
        let values = [
            ("style", "felicity"),
            ("washroom", "false"),
            ("scale", "standard"),
            ("size", "2"),
            ("site", "70x50"),
            ("cover", "pillars"),
            ("clutter", "0.25"),
            ("walkway", "2"),
            ("crates", "true"),
            ("shapes", "false"),
            ("risers", "3"),
            ("second", "0"),
            ("vents", "1"),
            ("windows", "6"),
            ("drops", "0"),
            ("grates", "5"),
            ("platforms", "1.5"),
            ("galleries", "2"),
            ("holes", "0.5"),
            ("cellars", "0.5"),
            ("angled", "0.75"),
            ("flooded", "0.3"),
            ("shaped", "0.5"),
            ("sunken", "0.4"),
            ("bases", "false"),
            ("big_pods", "false"),
            ("rescue", "false"),
            ("dead-ends", "false"),
            ("dead-corridor", "6"),
            ("dead_walk", "14"),
            ("dead_climb", "1"),
            ("loops", "0.6"),
            ("shift", "0"),
            ("level_change", "12.5"),
            ("tries", "4"),
        ];
        assert_eq!(values.map(|(name, _)| name.replace('-', "_")), OPTIONS);
        for (name, v) in values {
            set_option(&mut o, name, v).unwrap();
            let shown = option(&o, name).unwrap();
            assert_eq!(shown, if name == "size" { "2.0" } else { v }, "{name}");
        }
        assert_eq!((o.siting.scale, o.siting.size, o.siting.site, o.siting.shift), (Scale::Standard, 2.0, Some((70, 50)), 0));
        assert_eq!((o.cover, o.walkway, o.crates, o.shapes, o.dead_ends, o.tries), ("pillars", 2, true, false, false, 4));
        let r = Recipe { seed: 7, options: o, edits: every_edit() };
        let text = save(&r);
        assert_eq!(load(&text).unwrap(), r);
        assert_eq!(save(&load(&text).unwrap()), text);
        // the fields come in the file's own order, and only what is not the default is written
        let at = |what: &str| text.find(what).unwrap_or_else(|| panic!("{what} is not in {text}"));
        assert!(at("\"format\"") < at("\"seed\"") && at("\"seed\"") < at("\"options\"") && at("\"options\"") < at("\"edits\""));
        assert!(at("\"scale\"") < at("\"size\"") && at("\"size\"") < at("\"cover\"") && at("\"cover\"") < at("\"tries\""));
        assert!(at("\"edit\": \"join\"") < at("\"edit\": \"remove\""));
        let one = Recipe { seed: 7, options: Options { vents: 0, ..Options::default() }, edits: Vec::new() };
        assert_eq!(save(&one), "{\n  \"format\": 1,\n  \"seed\": 7,\n  \"options\": {\n    \"vents\": 0\n  },\n  \"edits\": []\n}\n");
        // a site goes back to the scale's own
        let mut o = one.options.clone();
        set_option(&mut o, "site", "80x60").unwrap();
        set_option(&mut o, "site", "default").unwrap();
        assert_eq!((o.siting.site, option(&o, "site").unwrap().as_str()), (None, "default"));
    }

    #[test]
    fn what_it_cannot_read_it_says() {
        let err = |text: &str| load(text).unwrap_err();
        assert_eq!(err("{\"format\": 2, \"seed\": 40}"), "recipe: format 2 is not one this build reads (format 1)");
        assert_eq!(err("{\"seed\": 40}"), "recipe: no \"format\" (this build reads format 1)");
        assert!(err("{\"format\": 1}").contains("missing field `seed`"));
        assert!(err("{\"format\": 1, \"seed\": 40, \"sead\": 1}").contains("unknown field `sead`"));
        assert!(err("{\"format\": 1, \"seed\": 40, \"options\": {\"covers\": \"none\"}}").contains("unknown field `covers`"));
        assert_eq!(err("{\"format\": 1, \"seed\": 40, \"options\": {\"cover\": \"hedges\"}}"), "recipe: cover: one of ('mixed', 'pillars', 'crates', 'none')");
        assert!(err("{\"format\": 1, \"seed\": 40, \"edits\": [{\"edit\": \"paint\", \"room\": \"vast\"}]}").contains("unknown variant `paint`"));
        assert!(err("{\"format\": 1, \"seed\": 40, \"edits\": [{\"edit\": \"remove\", \"route\": \"c07\", \"why\": 1}]}").contains("unknown field `why`"));
        assert!(err("{\"format\": 1, \"seed\": 40, \"edits\": [{\"edit\": \"remove\"}]}").contains("missing field `route`"));
        assert!(err("{\"format\": 1,").starts_with("recipe: EOF"));
        let mut o = Options::default();
        assert!(set_option(&mut o, "colour", "red").unwrap_err().starts_with("no option colour: one of style, washroom, scale, size"));
        assert!(set_option(&mut o, "risers", "many").unwrap_err().starts_with("risers: invalid type"));
        assert_eq!(set_option(&mut o, "site", "wide").unwrap_err(), "site: cells wide by cells deep, as 80x60, not wide");
        assert_eq!(o, Options::default());
    }

    /// What two levels would have to share to be the same level.
    fn same(a: &Level, b: &Level) -> bool {
        a.report == b.report
            && a.plan == b.plan
            && a.walks == b.walks
            && a.views == b.views
            && a.fixes == b.fixes
            && a.dead_ends == b.dead_ends
            && a.moods == b.moods
    }

    #[test]
    fn with_no_edits_it_is_the_seeds_own_level() {
        for (seed, o) in [(40, Options::before_bases()), (7, Options { cover: "pillars", dead_ends: false, ..Options::before_bases() })] {
            let r = Recipe { seed, options: o.clone(), edits: Vec::new() };
            let (ours, theirs) = (r.generate().unwrap(), level::generate(seed, &o).unwrap());
            assert!(same(&ours, &theirs), "seed {seed}");
            assert_eq!(format!("{:?}", ours.net.routes()), format!("{:?}", theirs.net.routes()));
            // and through its file
            assert!(same(&load(&save(&r)).unwrap().generate().unwrap(), &theirs));
        }
        let r = Recipe { seed: 1, options: Options { walkway: 3, ..Options::before_bases() }, edits: Vec::new() };
        assert_eq!(r.generate().unwrap_err(), "walkway: 1 or 2 cells");
    }

    #[test]
    fn every_edit_is_applied_and_names_its_rooms() {
        assert!(every_edit().iter().all(|e| e.supported()));
        let edits = every_edit();
        let rooms: Vec<Vec<&str>> = edits.iter().map(|e| e.rooms()).collect();
        assert_eq!(rooms[0], ["medium3"]); // move
        assert_eq!(rooms[2], ["extra1"]); // add: the room it makes
        assert_eq!(rooms[5], ["tower", "colossal"]); // join: both ends
        assert!(rooms[7].is_empty()); // remove: a route is no room
        assert!(rooms.iter().enumerate().all(|(n, r)| r.len() == 1 || matches!(n, 5..=7)));
        // (what each does on a real seed, and when it is an error, is in tests/recipe.rs)
    }

    #[test]
    fn a_join_by_hand_is_a_corridor_the_pass_need_not_add() {
        // seed 40's tower climbs two storeys to nothing; the pass joins its 551 to the colossal hall
        let plain = Recipe::before_bases(40).generate().unwrap();
        let theirs = plain.fixes.iter().any(|f| matches!(f, Fix::Joined { room, storey: 551, to, .. } if room == "tower" && to == "colossal"));
        assert!(theirs, "{:?}", plain.fixes);
        let join = Edit::Join { room: "tower".into(), storey: 551, to: "colossal".into(), on: Vec::new(), door: None, to_door: None, via: None };
        let r = Recipe { seed: 40, options: Options::before_bases(), edits: vec![join] };
        let lv = r.generate().unwrap();
        let ours = lv.net.routes().iter().find(|r| r.a.room == "tower" && r.a.level == 551 && r.b.room == "colossal").expect("the corridor asked for");
        assert!(!same(&lv, &plain));
        // made after routing, so before anything the pass adds, and the pass has one thing less to do
        assert!(lv.fixes.iter().all(|f| !matches!(f, Fix::Joined { room, .. } if room == "tower")), "{:?}", lv.fixes);
        assert_eq!(lv.fixes.len(), plain.fixes.len() - 1);
        assert!(lv.dead_ends.iter().all(|e| e.allowed));
        assert!(lv.interior("tower").doors.iter().any(|d| d.level == 551 && d.to == ours.key()));
        // the same recipe is the same level
        assert!(same(&lv, &load(&save(&r)).unwrap().generate().unwrap()));
        // what can't be joined says why
        let bad = |room: &str, storey: i32, to: &str| {
            Recipe {
                seed: 40,
                options: Options::before_bases(),
                edits: vec![Edit::Join { room: room.into(), storey, to: to.into(), on: Vec::new(), door: None, to_door: None, via: None }],
            }
            .generate()
            .unwrap_err()
        };
        assert_eq!(bad("tower", 551, "nowhere"), "join: no room nowhere");
        assert_eq!(bad("tower", 551, "tower"), "join: tower to itself");
        assert!(bad("tower", 1654, "colossal").starts_with("join: tower takes doors on "));
        assert_eq!(bad("small8", 276, "tower"), "join: small8 is a pod, joined inside vast");
    }

    #[test]
    fn a_route_can_be_taken_out_unless_the_level_falls_in_two() {
        let (b, _) = Recipe::before_bases(40).furnished().unwrap();
        // a corridor of a loop goes; one that is the only way to a room does not
        let (mut gone, mut bridge) = (None, None);
        for r in b.net.routes() {
            let mut net = b.net.clone();
            net.grid.remove(r);
            match components(&net) {
                1 if gone.is_none() => gone = Some(r.clone()),
                n if n > 1 && bridge.is_none() => bridge = Some(r.key().to_string()),
                _ => {}
            }
        }
        let (gone, bridge) = (gone.expect("a level has loops"), bridge.expect("and rooms with one corridor"));
        let last = gone.pieces.last().unwrap().key.clone(); // any piece's key names the route
        let r = Recipe { seed: 40, options: Options::before_bases(), edits: vec![Edit::Remove { route: last }] };
        let lv = r.generate().unwrap();
        assert!(lv.net.routes().iter().all(|q| q.key() != gone.key()));
        assert!(lv.plan.get(gone.key()).is_none() && lv.walks.iter().all(|(name, _)| !name.contains(gone.key())));
        assert!(lv.interior(&gone.a.room).doors.iter().all(|d| d.to != gone.key()));
        let plain = Recipe::before_bases(40).generate().unwrap();
        assert!(plain.plan.get(gone.key()).is_some() || gone.pieces[0].kind == "stub");
        assert!(!same(&lv, &plain));
        let err =
            |route: &str| Recipe { seed: 40, options: Options::before_bases(), edits: vec![Edit::Remove { route: route.into() }] }.generate().unwrap_err();
        assert_eq!(err(&bridge), format!("remove: without {bridge} the level is in two"));
        assert!(err("c999").starts_with("remove: no route c999 after routing"));
    }

    #[test]
    fn a_dead_end_can_be_kept_or_cut_by_hand() {
        // seed 7: tiny3 is nine cells down a corridor of its own, and the pass joins it up
        let plain = Recipe::before_bases(7).generate().unwrap();
        assert!(plain.fixes.iter().any(|f| matches!(f, Fix::Joined { room, .. } if room == "tiny3")));
        let keep = Recipe { seed: 7, options: Options::before_bases(), edits: vec![Edit::Keep { room: "tiny3".into() }] };
        let lv = keep.generate().unwrap();
        assert!(lv.fixes.iter().all(|f| !matches!(f, Fix::Joined { room, .. } if room == "tiny3")), "{:?}", lv.fixes);
        let left: Vec<&crate::deadends::DeadEnd> = lv.dead_ends.iter().filter(|e| !e.allowed).collect();
        assert_eq!((left.len(), left[0].rooms.as_slice()), (1, &["tiny3".to_string()][..]));
        assert_eq!(Recipe { edits: vec![Edit::Keep { room: "attic".into() }], ..keep }.generate().unwrap_err(), "keep: no room attic");
        // seed 40: the basin's balcony goes when its decks are cut at its floor
        let plain = Recipe::before_bases(40).generate().unwrap();
        assert!(plain.dead_ends.iter().any(|e| e.tip == "basin@276") && !plain.interior("basin").deck[&276].is_empty());
        let cut = Recipe { seed: 40, options: Options::before_bases(), edits: vec![Edit::Cut { room: "basin".into(), above: 0 }] };
        let lv = cut.generate().unwrap();
        assert!(lv.dead_ends.iter().all(|e| e.tip != "basin@276"));
        assert!(lv.interior("basin").deck.get(&276).is_none_or(|deck| deck.is_empty()));
        assert_eq!(lv.fixes, plain.fixes); // by hand: not one of the pass's own
        let err = |room: &str, above: i32| Recipe { edits: vec![Edit::Cut { room: room.into(), above }], ..cut.clone() }.generate().unwrap_err();
        assert_eq!(err("basin", 1654), "cut: basin has no storey at 1654");
        assert!(err("colossal", 0).starts_with("cut: colossal has a door on "));
    }

    #[test]
    fn a_rooms_brightness_and_ceiling_can_be_chosen() {
        let edits = vec![
            Edit::Brightness { room: "vast".into(), value: 77 },
            Edit::Ceiling { room: "vast".into(), ceiling: "grille".into() },
            Edit::Brightness { room: "vast".into(), value: 88 },
        ];
        let r = Recipe { seed: 40, options: Options::before_bases(), edits };
        let (lv, plain) = (r.generate().unwrap(), Recipe::before_bases(40).generate().unwrap());
        let of = |rows: &[(String, i32)]| rows.iter().filter(|(k, _)| k == "vast").map(|(_, v)| *v).collect::<Vec<_>>();
        assert_eq!(of(&lv.moods), [88]); // one entry, the last word
        assert_eq!(lv.ceilings.iter().filter(|(k, _)| k == "vast").map(|(_, c)| *c).collect::<Vec<_>>(), ["grille"]);
        assert!(lv.plan == plain.plan && lv.report == plain.report);
        let err = |e: Edit| Recipe { seed: 40, options: Options::before_bases(), edits: vec![e] }.generate().unwrap_err();
        assert_eq!(err(Edit::Brightness { room: "vast".into(), value: 300 }), "brightness: 0 to 255, not 300");
        assert_eq!(err(Edit::Ceiling { room: "vast".into(), ceiling: "sky".into() }), "ceiling: one of ('plain', 'ceiling_tile', 'ceiling_dark', 'grille')");
        assert_eq!(err(Edit::Ceiling { room: "attic".into(), ceiling: "plain".into() }), "ceiling: no room attic");
    }
}
