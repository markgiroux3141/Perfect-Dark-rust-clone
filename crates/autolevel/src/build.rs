//! A level's rooms being furnished, and its dead ends put right. From `autolevel/level.py`:
//! `_openings`, `_dress`, `_layout` and the class `_Build` (`refresh`, `_furnish`, `sided`, and
//! the dead-end pass: `ends`, `resolve`, `join`, `shorten`, `_respiral`).
//!
//! The network changes as dead ends are joined up or cut, and each change means new openings
//! and new insides for the rooms it touches, so those are worked out here, from the network as
//! it stands, as often as needed (a room whose doors, pods and openings are as they were is
//! kept). The rooms are the network's own (`net.spaces()`): the Python's `sp` is one dict that
//! the build and the grid share.
//!
//! New in the Rust: `Choice`, what a person chose for one room's inside (a recipe's `cover`,
//! `layout` and `reroll`). The build holds them (`Build::choices`) and a room is furnished by
//! its own every time, the dead-end pass included. With none, every room is as the Python's.

use crate::cells::{rect_cells, Cell, Cells, Rect};
use crate::deadends::{self, DeadEnd, Graph, Limits, NavCache};
use crate::furnish::{Interior, Setup, GALLERIES_MIN, MEZZANINE_MIN, RING_MIN};
use crate::level::Options;
use crate::network::{components, Network};
use crate::pods::Access;
use crate::py;
use crate::pyrand::PyRandom;
use crate::route::{Terms, VentTerms};
use crate::scatter::rect_gap;
use crate::space::{door_pad, Door, Space};
use crate::spiral;
use crate::storeys::{storey, LEVELS};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// The dearest corridor a dead end is joined up by (a three-storey switchback is about 55).
pub const JOIN_COST: f64 = 70.0;
/// Head positions A* looks at for one.
pub const JOIN_SEARCH: usize = 20000;
/// Times the dead ends are looked for again after something was done about them.
pub const ROUNDS: usize = 40;
/// Cells: how far off a dead end looks for something to join.
pub const NEAR: i32 = 10;
/// And how far it looks before its rooms are left out, if they have `FAR_AREA` cells of floor
/// between them.
pub const FAR: i32 = 16;
pub const FAR_AREA: i32 = 30;

/// What was done about a dead end (the Python's `fixes` are dicts: `{"joined": room, ...}`,
/// `{"cut": room, "above": storey}`, `{"dropped": room}`).
#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// `room`, on `storey`, was joined to the room `to` by the route whose first piece is `by`.
    Joined {
        room: String,
        storey: i32,
        to: String,
        by: String,
        /// The first piece's kind: corridor, ramp, stub or vent.
        kind: &'static str,
        /// The route's length in cells, to one decimal place.
        length: f64,
        ramps: usize,
    },
    /// `room`'s own decks stop at the storey `above`.
    Cut { room: String, above: i32 },
    /// `room` was left out of the level.
    Dropped { room: String },
}

/// A drop: (upper room, lower room, cells). A hole two cells square in the upper room's floor.
pub type DropHole = (String, String, Rect);
/// A drop with the side it is stepped into from (None: railed all round, a lightwell).
pub type Sided = (String, String, Rect, Option<usize>);
/// A grate: (corridor piece, room under it, cells, the corridor's floor).
pub type Grate = (String, String, Rect, i32);

/// Holes between things that lie over one another, chosen after routing and before any room is
/// furnished (the rooms keep clear of them). Returns (drops, grates).
///
/// A drop is two cells square in the upper room's floor, a cell in from both rooms' walls and
/// clear of the upper room's doors, into a single-storey room at most one empty storey below.
/// A grate is a piece of a corridor's floor (its width, two cells along) where it crosses a
/// room. A room is lit from above by one opening at most.
pub fn openings(seed: u64, net: &Network, o: &Options) -> (Vec<DropHole>, Vec<Grate>) {
    let mut rng = PyRandom::named(&format!("{seed}/openings"));
    let grid = &net.grid;
    let sp = net.spaces();
    let rooms: Vec<&Space> = sp.iter().filter(|s| s.host.is_empty()).collect();
    let (mut drops, mut grates): (Vec<DropHole>, Vec<Grate>) = (Vec::new(), Vec::new());
    let (mut lit, mut holed): (BTreeSet<&str>, BTreeSet<&str>) = (BTreeSet::new(), BTreeSet::new());
    let mut pairs: Vec<(&str, &str, Vec<Cell>)> = Vec::new();
    for up in &rooms {
        if up.decks.is_some() || up.pool.is_some() || o.drops == 0 {
            continue;
        }
        let mut aprons = Cells::new();
        for d in net.doors(&up.key).iter().filter(|d| d.level == up.floor) {
            aprons.extend(door_pad(up, d, 6, 1));
        }
        for low in &rooms {
            // (at most one empty storey between them: a shaft, not a chute)
            if low.key == up.key || low.tall() || low.pool.is_some() || low.decks.is_some() || !(0 < up.floor - low.top && up.floor - low.top <= 320) {
                continue;
            }
            let (x0, y0) = (up.x0.max(low.x0) + 1, up.y0.max(low.y0) + 1);
            let (x1, y1) = (up.x1.min(low.x1) - 1, up.y1.min(low.y1) - 1);
            let between: Vec<i32> = LEVELS.iter().copied().filter(|&z| low.top < z && storey(z).1 < up.floor).collect();
            let mut blocks = Vec::new();
            for x in x0..x1 - 1 {
                for y in y0..y1 - 1 {
                    if rect_cells((2 * x, 2 * y, 2 * x + 4, 2 * y + 4)).is_disjoint(&aprons)
                        && !between.iter().any(|&z| (x - 1..x + 3).any(|i| (y - 1..y + 3).any(|j| grid.owner_at(i, j, z) != 0)))
                    {
                        blocks.push((x, y));
                    }
                }
            }
            if !blocks.is_empty() {
                pairs.push((&up.key, &low.key, blocks));
            }
        }
    }
    rng.shuffle(&mut pairs);
    for (up, low, blocks) in &pairs {
        if drops.len() >= o.drops {
            break;
        }
        if holed.contains(up) || lit.contains(low) || lit.contains(up) || holed.contains(low) {
            continue;
        }
        let (x, y) = *rng.choice(blocks);
        drops.push((up.to_string(), low.to_string(), (x, y, x + 2, y + 2)));
        holed.insert(up);
        lit.insert(low);
    }
    // (not tall, piece, room, z, the places it could go)
    let mut cand: Vec<(bool, &str, &str, i32, Vec<Rect>)> = Vec::new();
    for r in net.routes() {
        for pc in &r.pieces {
            if !matches!(pc.kind, "corridor" | "ramp") || o.grates == 0 {
                continue;
            }
            for (z, cells) in &pc.cells {
                for low in &rooms {
                    if !(0 < z - low.top && z - low.top <= 40) {
                        continue;
                    }
                    let pods: Vec<Rect> = sp.iter().filter(|q| q.host == low.key).map(|q| q.rect()).collect();
                    let mut blocks: Vec<Rect> = Vec::new();
                    for &(x, y) in cells {
                        for (w, d) in [(crate::route::WIDE, 2), (2, crate::route::WIDE)] {
                            if (0..w).all(|i| (0..d).all(|j| cells.contains(&(x + i, y + j))))
                                && low.x0 < x
                                && x + w <= low.x1 - 1
                                && low.y0 < y
                                && y + d <= low.y1 - 1
                                && !pods.iter().any(|&q| {
                                    let (gx, gy) = rect_gap((x, y, x + w, y + d), q);
                                    gx.max(gy) < 2
                                })
                            {
                                blocks.push((x, y, x + w, y + d));
                            }
                        }
                    }
                    blocks.sort();
                    if !blocks.is_empty() {
                        cand.push((!low.tall(), &pc.key, &low.key, *z, blocks));
                    }
                }
            }
        }
    }
    cand.sort();
    let mut taken: BTreeSet<&str> = BTreeSet::new();
    for (_, piece, low, z, blocks) in &cand {
        if grates.len() >= o.grates {
            break;
        }
        if taken.contains(piece) || lit.contains(low) || holed.contains(low) {
            continue;
        }
        grates.push((piece.to_string(), low.to_string(), blocks[blocks.len() / 2], *z));
        taken.insert(piece);
        lit.insert(low);
    }
    (drops, grates)
}

/// What stands in a room and what shape it is: `Interior`'s cover, clutter, shape and pillar.
#[derive(Clone, Debug, PartialEq)]
pub struct Dress {
    pub cover: &'static str,
    pub clutter: f64,
    pub shape: Option<&'static str>,
    pub pillar: &'static str,
}

/// A room's dress, by its kind and the level's options.
pub fn dress(s: &Space, o: &Options, rng: &mut PyRandom) -> Dress {
    let big = matches!(s.kind, "colossal" | "vast" | "basin" | "long");
    let mut cover = o.cover;
    if cover == "mixed" {
        let w: &[(&'static str, f64)] = if s.area() < 16 {
            &[("none", 0.7), ("crates", 0.3)]
        } else if matches!(s.kind, "tower" | "spire") {
            &[("none", 0.6), ("crates", 0.25), ("pillars", 0.15)]
        } else if big {
            &[("pillars", 0.6), ("none", 0.25), ("crates", 0.15)]
        } else {
            &[("pillars", 0.35), ("none", 0.35), ("crates", 0.3)]
        };
        let x = rng.random();
        let mut acc = 0.0;
        cover = w[w.len() - 1].0;
        for &(name, share) in w {
            acc += share;
            if x < acc {
                cover = name;
                break;
            }
        }
    }
    if o.cover == "mixed" && cover == "crates" && !o.crates {
        cover = "lanes";
    }
    let mut shape = None;
    if o.shapes && !s.tall() && s.host.is_empty() && s.pool.is_none() && s.area() >= 20 {
        let x = rng.random();
        shape = if x < 0.3 {
            Some("notch")
        } else if x < 0.5 && s.area() >= 35 {
            Some("partition")
        } else {
            None
        };
    }
    let pillars: [&'static str; 4] = ["column", "column", "pier", "housing"];
    let pillar = *rng.choice(&pillars);
    Dress { cover, clutter: o.clutter, shape, pillar }
}

/// (balcony wall, layout) for a tall room's first upper storey. A hall may get a ring of
/// catwalk, galleries down both long walls or a mezzanine across one end; a room nothing
/// arrives in upstairs may get a balcony of its own on one wall (0 south, 1 east, 2 north,
/// 3 west). Later attempts go without.
pub fn layout(s: &Space, doors: &[Door], pods: &[Space], rng: &mut PyRandom, attempt: usize) -> (Option<usize>, Option<&'static str>) {
    if !s.tall() || matches!(s.kind, "tower" | "spire") || attempt >= 4 {
        return (None, None);
    }
    let (w, d) = s.size();
    if w.min(d) < 5 || w.max(d) < 8 {
        return (None, None);
    }
    let x = rng.random();
    let lay = match s.kind {
        "colossal" | "vast" => {
            if x < 0.3 {
                Some("ring")
            } else if x < 0.6 {
                Some("galleries")
            } else if x < 0.8 {
                Some("mezzanine")
            } else {
                None
            }
        }
        "basin" => {
            if x < 0.3 {
                Some("ring")
            } else if x < 0.5 {
                Some("galleries")
            } else {
                None
            }
        }
        _ => (x < 0.4).then_some("mezzanine"),
    };
    if lay.is_some() {
        return (None, lay);
    }
    if doors.iter().any(|q| q.level != s.floor) || !pods.is_empty() {
        return (None, None);
    }
    let odds = match s.kind {
        "vast" | "colossal" => 0.85,
        "basin" | "long" => 0.6,
        _ => 0.5,
    };
    if rng.random() > odds {
        return (None, None);
    }
    let long_sides: &[usize] = if w >= d { &[0, 2] } else { &[1, 3] };
    let sides = if rng.random() < 0.7 { long_sides } else { &[0, 1, 2, 3] };
    (Some(*rng.choice(sides)), None)
}

/// The layouts a hall's first upper storey may be given by hand ("none" aside).
pub const LAYOUTS: [&str; 3] = ["ring", "galleries", "mezzanine"];

/// What a person chose for one room's inside, in place of what the seed gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    /// What stands on its floor: one of `level::COVER` ("mixed": what a level of mixed cover
    /// would give it).
    pub cover: Option<&'static str>,
    /// Its first upper storey: Some(Some("ring")), or Some(None) for no layout at all.
    pub layout: Option<Option<&'static str>>,
    /// Which stream of random numbers it is furnished from: 0 is the seed's own.
    pub reroll: u32,
}

/// Err unless a room can take this layout (one of `LAYOUTS`, or "none") on its first upper
/// storey: it has to be tall, no tower or spire (their decks are their own), and big enough
/// (`layout` and the furnisher's `lay_decks` would pass over it otherwise).
pub fn takes(s: &Space, lay: &str) -> Result<(), String> {
    let key = &s.key;
    if !s.host.is_empty() {
        return Err(format!("{key} is a pod: it has no storeys of its own"));
    }
    if !s.tall() {
        return Err(format!("{key} has one storey: a layout is for a hall's first upper storey"));
    }
    if matches!(s.kind, "tower" | "spire") {
        return Err(format!("{key} is a {}: its decks are its own", s.kind));
    }
    if lay == "none" {
        return Ok(());
    }
    let (w, d) = s.size();
    // (5 by 8 is the least `layout` draws one for)
    let (short, long) = match lay {
        "ring" => ((RING_MIN / 2).max(5), 8),
        "galleries" => ((GALLERIES_MIN / 2).max(5), 8),
        _ => (5, (MEZZANINE_MIN / 2).max(8)),
    };
    if w.min(d) < short || w.max(d) < long {
        return Err(format!("{key} is too small for {}: it is {w} x {d} cells, and that needs {short} by {long}", a(lay)));
    }
    Ok(())
}

/// A layout with its article, for a message: "a ring", "galleries".
fn a(lay: &str) -> String {
    if lay.ends_with('s') {
        lay.to_string()
    } else {
        format!("a {lay}")
    }
}

/// What a room was furnished from: the same again gives the same inside, so it is kept.
#[derive(Clone, Debug, PartialEq)]
struct Sig {
    key: String,
    /// Its doors: (side, low end, storey, width).
    doors: Vec<(char, i32, i32, i32)>,
    pods: Vec<String>,
    hole: Option<Rect>,
    keep: Vec<Rect>,
    cap: Option<i32>,
    preset: usize,
    /// What was chosen for it by hand.
    choice: Choice,
}

/// A room's inside and whether it has its hole, or why it can't be furnished.
type Made = Result<(Arc<Interior>, bool), String>;

/// A level's rooms being furnished (`level._Build`).
#[derive(Clone, Debug)]
pub struct Build {
    pub seed: u64,
    pub o: Options,
    /// The rooms and what joins them. The rooms are `net.spaces()`.
    pub net: Network,
    /// For each pod: where it opens into its hall.
    pub access: Vec<(String, Access)>,
    /// For each spire: (the wall its first ramp climbs, which way round).
    pub spin: Vec<(String, (usize, i32))>,
    /// Room -> the highest storey its own decks may be on.
    pub caps: BTreeMap<String, i32>,
    /// Every room's inside (pods aside), the rooms in the order they were scattered.
    pub interiors: Vec<(String, Arc<Interior>)>,
    pub drops: Vec<DropHole>,
    pub grates: Vec<Grate>,
    /// What was done about the dead ends, in the order it was done.
    pub fixes: Vec<Fix>,
    /// Rooms whose dead ends the pass leaves as they are (a recipe's "keep" edits; none otherwise).
    pub keep: BTreeSet<String>,
    /// Room -> what was chosen for its inside by hand (a recipe's "cover", "layout" and "reroll"
    /// edits; none otherwise). Set through `choose`.
    pub choices: BTreeMap<String, Choice>,
    /// Every inside worked out so far, by what it was made from.
    made: Vec<(Sig, Made)>,
    /// Each inside's standing blocks, for the dead-end finder (`made` keeps the insides alive).
    navs: NavCache,
    /// Every (room, storey, other room) a join has been asked for.
    tried: BTreeSet<(String, i32, String)>,
}

impl Build {
    pub fn new(seed: u64, o: Options, net: Network, access: Vec<(String, Access)>, spin: Vec<(String, (usize, i32))>) -> Build {
        Build {
            seed,
            o,
            net,
            access,
            spin,
            caps: BTreeMap::new(),
            interiors: Vec::new(),
            drops: Vec::new(),
            grates: Vec::new(),
            fixes: Vec::new(),
            keep: BTreeSet::new(),
            choices: BTreeMap::new(),
            made: Vec::new(),
            navs: NavCache::new(),
            tried: BTreeSet::new(),
        }
    }

    /// The rooms, in the order they were scattered.
    pub fn spaces(&self) -> &[Space] {
        self.net.spaces()
    }

    /// A room's inside.
    pub fn interior(&self, key: &str) -> Option<&Interior> {
        self.interiors.iter().find(|(k, _)| k == key).map(|(_, it)| it.as_ref())
    }

    /// What a person chooses for a room's inside, to change: the room is furnished by it from
    /// the next `refresh` on. Err if it is no room of the level, or a pod (a pod has no floor of
    /// its own to dress). `what` is the edit's name, for the message.
    pub fn choose(&mut self, what: &str, room: &str) -> Result<&mut Choice, String> {
        let s = self.spaces().iter().find(|s| s.key == room).ok_or_else(|| format!("{what}: no room {room}"))?;
        if !s.host.is_empty() {
            return Err(format!("{what}: {room} is a pod of {}: it is not furnished by itself", s.host));
        }
        Ok(self.choices.entry(room.to_string()).or_default())
    }

    /// Openings, and every room's inside, for the network as it stands. Err (the Python's
    /// `Failed`) if a room can't be furnished; the build is then as it was.
    pub fn refresh(&mut self) -> Result<(), String> {
        let (mut drops, grates) = openings(self.seed, &self.net, &self.o);
        let h = |r: Rect| (2 * r.0, 2 * r.1, 2 * r.2, 2 * r.3);
        let holes: Vec<(String, Rect)> = drops.iter().map(|(u, _, r)| (u.clone(), h(*r))).collect();
        let mut keeps: Vec<(&str, Rect)> = drops.iter().map(|(_, low, r)| (low.as_str(), h(*r))).collect();
        keeps.extend(grates.iter().map(|(_, low, r, _)| (low.as_str(), h(*r))));
        let mut interiors = Vec::new();
        let mut unholed: Vec<String> = Vec::new();
        let rooms: Vec<Space> = self.spaces().iter().filter(|s| s.host.is_empty()).cloned().collect();
        for s in &rooms {
            let key = &s.key;
            let pods: Vec<Space> = self.spaces().iter().filter(|p| p.host == *key).cloned().collect();
            let hole = holes.iter().find(|(u, _)| u == key).map(|(_, r)| *r);
            let keep: Vec<Rect> = keeps.iter().filter(|(low, _)| low == key).map(|(_, r)| *r).collect();
            let sig = Sig {
                key: key.clone(),
                doors: self.net.doors(key).iter().map(|d| (d.side, d.a, d.level, d.width)).collect(),
                pods: pods.iter().map(|p| p.key.clone()).collect(),
                hole,
                keep: keep.clone(),
                cap: self.caps.get(key).copied(),
                preset: s.preset.len(),
                choice: self.choices.get(key).cloned().unwrap_or_default(),
            };
            let made = match self.made.iter().find(|(g, _)| *g == sig) {
                Some((_, made)) => made.clone(),
                None => {
                    let made = self.furnish(s, &pods, hole, &keep).map(|(it, holed)| (Arc::new(it), holed));
                    self.made.push((sig, made.clone()));
                    made
                }
            };
            let (it, holed) = made.map_err(|why| format!("seed {}: {why}", self.seed))?;
            interiors.push((key.clone(), it));
            if hole.is_some() && !holed {
                unholed.push(key.clone()); // the room can't take the hole: go without it
            }
        }
        drops.retain(|d| !unholed.contains(&d.0));
        self.interiors = interiors;
        self.drops = drops;
        self.grates = grates;
        Ok(())
    }

    /// The room's inside and whether it has its hole, or why not (`_furnish`). Each attempt has
    /// its own random stream; with a hole, every attempt is made with it before any without.
    ///
    /// What was chosen for the room by hand (`choices`) comes in here. A re-roll `n` moves the
    /// dress and every attempt to streams of its own, "{seed}/{key}/reroll{n}/dress" and
    /// "{seed}/{key}/reroll{n}/{attempt}" (no other stream has four parts, and no key a slash).
    /// A chosen cover replaces the dress's and nothing else of it; a chosen layout replaces the
    /// one drawn on every attempt, the later ones too, and if none can be furnished with it the
    /// Err says so: the room is never given another instead.
    fn furnish(&self, s: &Space, pods: &[Space], hole: Option<Rect>, keep: &[Rect]) -> Result<(Interior, bool), String> {
        let (seed, key) = (self.seed, &s.key);
        let doors = self.net.doors(key);
        let pick = self.choices.get(key).cloned().unwrap_or_default();
        let stream = |part: &str| match pick.reroll {
            0 => format!("{seed}/{key}/{part}"),
            n => format!("{seed}/{key}/reroll{n}/{part}"),
        };
        let mut how = dress(s, &self.o, &mut PyRandom::named(&stream("dress")));
        match pick.cover {
            // what a level of mixed cover would have drawn for it, from the same stream
            Some("mixed") if self.o.cover != "mixed" => {
                how.cover = dress(s, &Options { cover: "mixed", ..self.o.clone() }, &mut PyRandom::named(&stream("dress"))).cover
            }
            Some("mixed") | None => {}
            Some(cover) => how.cover = cover,
        }
        let mut last = "None".to_string();
        let holes = if hole.is_some() { vec![hole, None] } else { vec![None] };
        for h in holes {
            for attempt in 0..self.o.tries {
                let mut rng = PyRandom::named(&stream(&attempt.to_string()));
                let tower = ["ring", "ends"][((rng.random() < 0.5) as usize) ^ (attempt % 2)];
                let (balcony, lay) = layout(s, doors, pods, &mut rng, attempt);
                let (balcony, lay) = match pick.layout {
                    None => (balcony, lay),
                    // (a balcony is drawn only when no layout was: it stays if so)
                    Some(None) => (if lay.is_none() { balcony } else { None }, None),
                    Some(chosen) => (None, chosen),
                };
                let setup = Setup {
                    balcony,
                    tower,
                    cover: how.cover,
                    clutter: how.clutter,
                    layout: lay,
                    fixed: pick.layout.is_some(),
                    shape: how.shape,
                    pillar: how.pillar,
                    hole: h,
                    keep: keep.to_vec(),
                    top: self.caps.get(key).copied(),
                    band: self.o.band(),
                };
                match Interior::new(s, doors, pods, rng, &self.access, setup).build() {
                    Ok(it) => return Ok((it, h.is_some())),
                    Err(why) => last = why,
                }
            }
        }
        match pick.layout {
            Some(lay) => Err(format!("{key} cannot be furnished with {} ({last})", lay.map_or("no layout".to_string(), a))),
            None => Err(last),
        }
    }

    /// The drops, each with the side it is stepped into from (None: railed all round, a lightwell).
    pub fn sided(&self) -> Vec<Sided> {
        self.drops
            .iter()
            .filter_map(|(u, low, r)| {
                let it = self.interior(u)?;
                (!it.hole.is_empty()).then(|| (u.clone(), low.clone(), *r, it.hole_open))
            })
            .collect()
    }

    /// (the graph of places, its dead ends) for the level as it stands.
    pub fn ends(&mut self) -> (Graph, Vec<DeadEnd>) {
        let drops = self.sided();
        let g = deadends::graph(self.net.spaces(), &self.net, &self.interiors, &drops, &mut self.navs);
        let sizes: HashMap<String, i32> = self.spaces().iter().map(|s| (s.key.clone(), s.area())).collect();
        let lim = Limits { corridor: self.o.dead_corridor, walk: self.o.dead_walk, climb: self.o.dead_climb, ..Limits::default() };
        let found = deadends::find(&g, &lim, &sizes);
        (g, found)
    }

    // ------------------------------------------------------------ dead ends

    /// Every dead end that is too long, too far or too high is joined to something near it that
    /// is not part of it, or else cut short: its bare decks taken down, or its farthest room
    /// left out. Again, until none is left or nothing more can be done. Err (the Python's
    /// `Failed`) if a room can't be furnished afterwards, or leaving one out cut the level in two.
    pub fn resolve(&mut self) -> Result<(), String> {
        for _ in 0..ROUNDS {
            let (g, ends) = self.ends();
            let bad: Vec<&DeadEnd> = ends.iter().filter(|e| !e.allowed && !self.kept(&g, e)).collect();
            if bad.is_empty() {
                return Ok(());
            }
            let mut touched: BTreeSet<String> = BTreeSet::new();
            for e in bad {
                if e.places.iter().any(|k| touched.contains(&g.place(k).room)) {
                    continue; // (something done this round changed it: look again)
                }
                let mut done = self.join(&g, e, NEAR)?;
                if done.is_none() && e.cut.is_empty() && e.rooms.iter().map(|k| self.net.grid.space(k).area()).sum::<i32>() >= FAR_AREA {
                    done = self.join(&g, e, FAR)?; // rooms worth a longer corridor than leaving them out
                }
                let done = match done {
                    Some(rooms) => Some(rooms),
                    None => self.shorten(e)?,
                };
                touched.extend(done.unwrap_or_default());
            }
            if touched.is_empty() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Is this a dead end the pass was told to leave (one of its places is in a room of `keep`)?
    pub fn kept(&self, g: &Graph, e: &DeadEnd) -> bool {
        !self.keep.is_empty() && e.places.iter().any(|k| self.keep.contains(&g.place(k).room))
    }

    /// By hand, what `shorten` does to a climb that ends in nothing: `room`'s own decks stop at
    /// the storey `above` (a spiral is laid out again with its ring there), and the rooms are
    /// furnished again. Nothing is added to `fixes`. Err if the room has a door higher up, or
    /// can't be furnished.
    pub fn cut(&mut self, room: &str, above: i32) -> Result<(), String> {
        let s = self.spaces().iter().find(|s| s.key == room).ok_or_else(|| format!("cut: no room {room}"))?;
        if !s.storeys().contains(&above) {
            return Err(format!("cut: {room} has no storey at {above}"));
        }
        if let Some(d) = self.net.doors(room).iter().find(|d| d.level > above) {
            return Err(format!("cut: {room} has a door on {} (to {}): remove its route first", d.level, d.to));
        }
        if s.decks.is_some() {
            self.respiral(room, above);
        } else {
            self.caps.insert(room.to_string(), above);
        }
        self.refresh()
    }

    /// A corridor (or failing that a vent) from a place of the dead end, its tip first, to a
    /// room within `reach` cells that is no part of it. Returns the two rooms, or None.
    fn join(&mut self, g: &Graph, e: &DeadEnd, reach: i32) -> Result<Option<BTreeSet<String>>, String> {
        let before: BTreeSet<(&str, i32)> = e.places.iter().map(|k| g.place(k)).map(|p| (p.room.as_str(), p.level)).collect();
        for (room, level, other, levels, sep) in deadends::joins(g, self.spaces(), e, reach, deadends::STOREYS, deadends::EACH) {
            if !self.tried.insert((room.clone(), level, other.clone())) {
                continue;
            }
            let grid = &self.net.grid;
            let (a, b) = (grid.space(&room), grid.space(&other));
            let terms = Terms {
                max_cost: JOIN_COST.min(2.5 * sep + 20.0),
                la: Some(vec![level]),
                lb: Some(levels.clone()),
                search: JOIN_SEARCH,
                storeyed: true,
                ..Terms::default()
            };
            let mut r = grid.route(a, b, &terms);
            if r.is_none() && self.o.vents != 0 && a.vent_levels().contains(&level) {
                r = grid.vent(a, b, &VentTerms { la: Some(vec![level]), lb: Some(levels), ..VentTerms::default() });
            }
            let Some(r) = r else { continue };
            self.net.grid.commit(r.clone());
            // (a room that can't be furnished with the new door is no help either)
            let helped = self.refresh().is_ok() && {
                let (g2, ends) = self.ends();
                let still: BTreeSet<(&str, i32)> =
                    ends.iter().filter(|x| !x.allowed).flat_map(|x| &x.places).map(|k| g2.place(k)).map(|p| (p.room.as_str(), p.level)).collect();
                !before.is_subset(&still)
            };
            if !helped {
                // (or the door came out on another piece of deck): take it back
                self.net.grid.remove(&r);
                self.refresh()?;
                continue;
            }
            let by = &r.pieces[0];
            self.fixes.push(Fix::Joined {
                room: room.clone(),
                storey: level,
                to: other.clone(),
                by: by.key.clone(),
                kind: by.kind,
                length: py::round1(r.length()),
                ramps: r.ramps(),
            });
            return Ok(Some(BTreeSet::from([room, other])));
        }
        Ok(None)
    }

    /// Cut a dead end short: the decks that climb to nothing first (a spiral stops, a tower's
    /// rings stop), otherwise its farthest room goes. Returns the rooms changed.
    fn shorten(&mut self, e: &DeadEnd) -> Result<Option<BTreeSet<String>>, String> {
        if !e.cut.is_empty() {
            for (room, top) in &e.cut {
                let s = self.net.grid.space(room);
                let top = top.unwrap_or(s.floor);
                if s.decks.is_some() {
                    self.respiral(room, top);
                } else {
                    self.caps.insert(room.clone(), top);
                }
                self.fixes.push(Fix::Cut { room: room.clone(), above: top });
            }
            self.refresh()?;
            return Ok(Some(e.cut.iter().map(|(room, _)| room.clone()).collect()));
        }
        if e.leaf.is_empty() {
            return Ok(None);
        }
        let leaf = &e.leaf;
        let mut near: BTreeSet<String> =
            self.net.routes().iter().filter(|r| r.a.room == *leaf || r.b.room == *leaf).flat_map(|r| [r.a.room.clone(), r.b.room.clone()]).collect();
        let host = &self.net.grid.space(leaf).host;
        if !host.is_empty() {
            near.insert(host.clone());
        }
        self.net.drop(leaf);
        if components(&self.net) != 1 {
            return Err(format!("seed {}: leaving out {leaf} cut the level in two", self.seed));
        }
        self.refresh()?;
        self.fixes.push(Fix::Dropped { room: leaf.clone() });
        near.insert(leaf.clone());
        Ok(Some(near))
    }

    /// A spire's spiral again, stopping at storey `top` (with its ring there).
    fn respiral(&mut self, key: &str, top: i32) {
        let (start, turn) = self.spin.iter().find(|(k, _)| k == key).map_or_else(|| panic!("{key} is no spire"), |(_, spin)| *spin);
        let band = self.o.band();
        let s = self.net.grid.sp.iter_mut().find(|s| s.key == key).unwrap_or_else(|| panic!("no room {key}"));
        let storeys: Vec<i32> = s.storeys().into_iter().filter(|&z| z <= top).collect();
        match spiral::layout(s.rect_h(), &storeys, start, turn, band) {
            Some(lay) => {
                s.preset = lay.ramps;
                s.decks = Some(lay.decks);
                s.clear = lay.clear;
            }
            None => {
                s.preset = Vec::new();
                s.decks = Some(BTreeMap::new());
                s.clear = Cells::new();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{connect, Connect};
    use crate::route::tests::{room, space};

    fn build(sp: Vec<Space>, o: Options) -> Build {
        Build::new(5, o, connect(sp, 2, &Connect::default()), Vec::new(), Vec::new())
    }

    #[test]
    fn a_room_whose_doors_are_as_they_were_keeps_its_inside() {
        let mut b = build(vec![room("a", (0, 0, 6, 6)), room("b", (14, 9, 20, 15)), room("c", (24, 9, 28, 15))], Options::default());
        b.refresh().unwrap();
        assert_eq!(b.interiors.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert!(b.interiors.iter().all(|(k, it)| it.connected() && it.doors == b.net.doors(k)));
        let first = b.interiors.clone();
        b.refresh().unwrap();
        assert!(first.iter().zip(&b.interiors).all(|(was, is)| Arc::ptr_eq(&was.1, &is.1)));
        // a cap on a room's decks is a new inside for it, and for nobody else
        b.caps.insert("a".into(), 0);
        b.refresh().unwrap();
        let same: Vec<bool> = first.iter().zip(&b.interiors).map(|(was, is)| Arc::ptr_eq(&was.1, &is.1)).collect();
        assert_eq!(same, [false, true, true]);
        assert_eq!((b.interior("a").unwrap().top, b.interior("b").unwrap().top), (Some(0), None));
    }

    #[test]
    fn a_room_over_another_gets_a_drop_into_it() {
        let rooms = || vec![space("up", (0, 0, 8, 6), 276, 512, "medium"), room("low", (0, 0, 8, 6)), room("side", (12, 0, 18, 6))];
        let mut b = build(rooms(), Options::default());
        b.refresh().unwrap();
        let (u, low, r) = b.drops[0].clone();
        assert_eq!((b.drops.len(), u.as_str(), low.as_str(), r.2 - r.0, r.3 - r.1), (1, "up", "low", 2, 2));
        assert!(1 <= r.0 && r.2 <= 7 && 1 <= r.1 && r.3 <= 5); // a cell in from the walls
        let h = (2 * r.0, 2 * r.1, 2 * r.2, 2 * r.3);
        let (up, under) = (b.interior("up").unwrap(), b.interior("low").unwrap());
        assert_eq!(up.hole, rect_cells(h));
        assert_eq!(under.keep, [h]); // nothing stands where the drop lands
        assert!(up.hole_open.is_some() && up.rails.len() == 3);
        assert_eq!(b.sided(), [(u, low, r, up.hole_open)]);
        assert!(b.grates.is_empty());
        // and none when the level asks for none
        let mut b = build(rooms(), Options { drops: 0, ..Options::default() });
        b.refresh().unwrap();
        assert!(b.drops.is_empty() && b.sided().is_empty() && b.interior("up").unwrap().hole.is_empty());
    }

    /// Three rooms in a loop (the middle of a level) and a fourth, `far`, sixteen cells off the
    /// second down a corridor of its own.
    fn loop_and_spur(far: Rect, o: Options) -> Build {
        let sp = vec![room("a", (0, 0, 6, 6)), room("b", (12, 0, 18, 6)), room("c", (6, 12, 12, 18)), room("far", far)];
        let mut grid = crate::route::Grid::new(sp, 2);
        for (a, b) in [("a", "b"), ("b", "c"), ("c", "a"), ("b", "far")] {
            let r = grid.route(grid.space(a), grid.space(b), &Terms::default()).unwrap();
            grid.commit(r);
        }
        let mut b = Build::new(5, o, Network { grid, pods: Vec::new(), failed: Vec::new() }, Vec::new(), Vec::new());
        b.refresh().unwrap();
        b
    }

    #[test]
    fn a_small_room_down_a_long_corridor_is_left_out() {
        let mut b = loop_and_spur((34, 0, 38, 4), Options::default());
        let (g, ends) = b.ends();
        assert_eq!(g.places().len(), 4);
        assert_eq!(ends.len(), 1);
        assert_eq!((ends[0].root.as_str(), ends[0].rooms.as_slice(), ends[0].leaf.as_str(), ends[0].allowed), ("b@0", &["far".to_string()][..], "far", false));
        assert_eq!(ends[0].routes, [3]);
        // nothing is within reach of it, and it is not worth a longer corridor
        b.resolve().unwrap();
        assert_eq!(b.fixes, [Fix::Dropped { room: "far".into() }]);
        assert_eq!(b.spaces().iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!((b.net.routes().len(), b.interiors.len(), b.net.doors("b").len()), (3, 3, 2));
        assert!(b.ends().1.is_empty());
    }

    #[test]
    fn a_bigger_one_is_joined_by_a_second_corridor() {
        let mut b = loop_and_spur((34, 0, 40, 6), Options::default());
        assert!(!b.ends().1[0].allowed);
        b.resolve().unwrap();
        let [Fix::Joined { room, storey, to, by, kind, length, ramps }] = &b.fixes[..] else { panic!("{:?}", b.fixes) };
        assert_eq!((room.as_str(), *storey, to.as_str(), by.as_str(), *kind, *length, *ramps), ("far", 0, "b", "c05", "corridor", 21.0, 0));
        // the route is there, by the key the fix names, and its doors are furnished round
        let r = b.net.routes().iter().find(|r| r.key() == by).unwrap();
        assert_eq!((r.a.room.as_str(), r.b.room.as_str(), py::round1(r.length())), ("far", "b", *length));
        assert_eq!((b.net.routes().len(), b.net.doors("far").len(), b.interior("far").unwrap().doors.len()), (5, 2, 2));
        assert!(b.ends().1.is_empty()); // a loop out of the middle and back is no dead end
                                        // and a level that asks for its dead ends to be left keeps them
        let o = Options { dead_ends: false, ..Options::default() };
        let mut left = crate::level::stage_dead_ends(loop_and_spur((34, 0, 40, 6), o)).unwrap();
        assert!(left.fixes.is_empty() && left.net.routes().len() == 4 && !left.ends().1[0].allowed);
    }

    #[test]
    fn a_spiral_can_be_stopped_at_a_storey() {
        let mut spire = space("spire", (0, 0, 10, 10), 0, 1063, "spire");
        let lay = spiral::layout(spire.rect_h(), &spire.storeys(), 0, 1, 2).unwrap();
        (spire.preset, spire.decks, spire.clear) = (lay.ramps, Some(lay.decks), lay.clear);
        let grid = crate::route::Grid::new(vec![spire, room("b", (14, 0, 20, 6))], 2);
        let mut b = Build::new(5, Options::default(), Network { grid, pods: Vec::new(), failed: Vec::new() }, Vec::new(), vec![("spire".into(), (0, 1))]);
        let s = |b: &Build| b.spaces()[0].clone();
        assert_eq!((s(&b).preset.len(), s(&b).door_levels()), (3, vec![0, 276, 551, 827]));
        b.respiral("spire", 551);
        assert_eq!((s(&b).preset.len(), s(&b).door_levels()), (2, vec![0, 276, 551]));
        assert!(s(&b).decks.unwrap().keys().all(|&z| z <= 551));
        // down to its floor, nothing of it is left, but it is still a room whose inside is fixed
        b.respiral("spire", 0);
        assert_eq!((s(&b).preset.len(), s(&b).decks, s(&b).door_levels()), (0, Some(BTreeMap::new()), vec![0]));
    }

    #[test]
    fn a_room_that_cannot_be_furnished_fails_the_level() {
        let mut b = build(vec![space("t", (0, 0, 4, 4), 0, 512, "medium"), room("b", (8, 0, 14, 6))], Options::default());
        b.refresh().unwrap();
        // an upstairs door, and no floor for a ramp to it
        b.net.grid.doors[0].1.push(Door { room: "t".into(), side: 'N', a: 1, level: 276, width: 1, to: String::new() });
        assert_eq!(b.refresh().unwrap_err(), "seed 5: t: no set of ramps joins its storeys");
        assert_eq!(b.interiors.len(), 2); // the build is as it was
    }

    #[test]
    fn a_rooms_dress_and_layout_follow_its_kind() {
        let o = Options::default();
        let closet = room("closet", (0, 0, 3, 3));
        let hall = space("hall", (0, 0, 20, 14), 0, 787, "vast");
        for n in 0..40 {
            let seed = format!("dress/{n}");
            // a small room never gets pillars or a shape; crates are left out unless asked for
            let d = dress(&closet, &o, &mut PyRandom::named(&seed));
            assert!(matches!(d.cover, "none" | "lanes") && d.shape.is_none() && d.clutter == 0.5);
            let d = dress(&closet, &Options { crates: true, ..o.clone() }, &mut PyRandom::named(&seed));
            assert!(matches!(d.cover, "none" | "crates"));
            // the level's own cover is every room's, and a tall room keeps its shape
            let d = dress(&hall, &Options { cover: "pillars", ..o.clone() }, &mut PyRandom::named(&seed));
            assert!(d.cover == "pillars" && d.shape.is_none() && matches!(d.pillar, "column" | "pier" | "housing"));
            // a hall gets a layout or a balcony on one wall, never both, and neither on later attempts
            let (balcony, lay) = layout(&hall, &[], &[], &mut PyRandom::named(&seed), 0);
            assert!(balcony.is_none() || lay.is_none());
            assert!(balcony.is_none_or(|side| side < 4) && lay.is_none_or(|l| matches!(l, "ring" | "galleries" | "mezzanine")));
            assert_eq!(layout(&hall, &[], &[], &mut PyRandom::named(&seed), 4), (None, None));
            assert_eq!(layout(&closet, &[], &[], &mut PyRandom::named(&seed), 0), (None, None));
        }
    }
}
