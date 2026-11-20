//! Placement on a generated level: what the generator knows of its own
//! floors, given to the placer ([`crate::place`]).
//!
//! A custom level's spawns, weapons and ammo stand on the waypoints of its
//! graph, spread by farthest-point sampling, with nothing known of what each
//! waypoint stands on. On a generated level that puts a gun on a ramp, a
//! spawn in a corridor, and a weapon's ammo a few metres off it through a
//! wall or on the storey above. The generator made every floor, so it says
//! ([`hints`]):
//!
//! - **each room's kind** (`hub`, `room`, `corridor`, `ramp`, `vent`,
//!   `perch`), by PD's room number, which is the plan's;
//! - **where nothing goes** (`clear`): every ramp, and half a cell past its
//!   foot and its top; every doorway's mouth; every hole in a floor and every
//!   pit through a platform;
//! - **the risky places** (`risky`), where the power weapon goes (the MP
//!   location 5, the set's sixth slot): islands, pillar tops, platforms,
//!   galleries, perches, cellars and sunken floors, the riskiest first.
//!
//! And the placer ([`Guide`]) then stands everything on flat floor (a ring
//! round the spot all on the same floor, the spot moved up to 50 cm to find
//! one), spreads it by walking distance along the waypoints' links rather
//! than straight lines, keeps spawns out of corridors, puts weapons against
//! walls, first in the dead ends (a room one way in) and one to a room
//! before any room has two (a big room more), puts each weapon's two crates
//! in its room a short walk from it, the hills in the middles of halls and
//! rooms, Capture the Case's bases in rooms with two ways in or more, and
//! the cover only on the spots out of the way.
//!
//! None of this is PD's: PD's arenas were placed by hand. A layout
//! (`levels/<code>.layout.json`, the editor's) still wins over all of it,
//! and `plain_placement` in the recipe's source puts it back as any custom
//! level's.

use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::FRAC_PI_4;

use autolevel::cells::Rect;
use autolevel::furnish::rectangles;
use autolevel::plan::{box3, Box3};
use autolevel_build::built::Built;
use glam::Vec3;
use pd_sim::nav::gen::dijkstra;
use pd_sim::nav::NavGraph;
use pd_sim::stage::{directed_links, TileLevel};

use crate::autolevel::pd_point;

/// A pad stands this far over its floor.
const PAD_HEIGHT: f32 = 53.0;
/// A flat zone holds floors this far (units) over or under its storey.
const STOREY: i32 = 20;
/// A ramp's clear zone runs this far past its foot and its top (half a cell:
/// the middle of the landing, as [`crate::autonav`]'s legs do) and this far
/// out of its sides, and holds floors this far over and under its ends.
const RAMP_END: i32 = 59;
const RAMP_SIDE: i32 = 30;
const RAMP_RISE: i32 = 30;
/// A doorway's mouth: this far either side of its plane, and this far past
/// its jambs.
const MOUTH: i32 = 150;
const JAMB: i32 = 30;
/// A hole in a floor: this far round its edge.
const RIM: i32 = 100;
/// Flat floor: every point of a ring this wide round the spot (cm) is on the
/// spot's floor, within this much (a 4° slope).
pub const FLAT_RING: f32 = 45.0;
const FLAT: f32 = 3.0;
/// A spot is moved this far (cm) at most to find flat floor, in steps of this.
const SETTLE: f32 = 50.0;
const SETTLE_STEP: f32 = 25.0;

/// Room kinds no spawn stands in (a corridor's, an enclosed ramp's, a
/// vent's), and the kinds no weapon or crate does (it may stand in a
/// corridor when the rooms have no room for it).
const NO_SPAWN: [&str; 3] = ["corridor", "ramp", "vent"];
const NO_ITEM: [&str; 2] = ["ramp", "vent"];

/// The spawns keep this far (cm walking) from the power weapon, and this far
/// from every weapon, and stand within the first of these walks of one where
/// they can, else the second.
const PRIZE_AWAY: f32 = 1000.0;
const SPAWN_AWAY: f32 = 500.0;
const SPAWN_REACH: [f32; 2] = [2500.0, 5000.0];
/// The least walk between two spawns, and between two weapons.
const SPAWN_GAP: f32 = 500.0;
const WEAPON_GAP: f32 = 600.0;
/// A weapon's crates: in its room this far from it walking (the nearer
/// ring first), else anywhere this far; at least this far from each other
/// and from anything else placed.
const AMMO_NEAR: (f32, f32) = (200.0, 500.0);
const AMMO_ROOM: (f32, f32) = (150.0, 900.0);
const AMMO_WIDE: (f32, f32) = (150.0, 1500.0);
const AMMO_GAP: f32 = 150.0;
const AMMO_CLEAR: f32 = 200.0;
/// A spot is against a wall when one is this near (cm) at knee height in
/// one of eight ways.
const WALL_NEAR: f32 = 100.0;
const KNEE: f32 = 40.0;
/// A room takes one weapon, and one more for each this many spots, before
/// any room takes more.
const ROOM_SPOTS: usize = 150;
/// Up to a third of the weapons go to the dead ends first.
const DEAD_END_SHARE: usize = 3;
/// The kinds of room a hill or a base is in (a corridor's never), and the
/// least spots its room has.
const ROOMY: [&str; 2] = ["hub", "room"];
const ROOM_MIN: usize = 8;
/// A dead end takes a weapon with this many spots (the weapon's and its
/// crates', and one to spare; the crates may go out of it).
const DEAD_END_MIN: usize = 4;
/// A base's room has at least this many spots (its six respawn pads 1.5 m
/// apart need them).
const BASE_ROOM_MIN: usize = 24;
/// A base's respawn pads: within this walk of it (the second if the first
/// has too few), and this far apart.
const RESPAWN_WALK: [f32; 2] = [1500.0, 3000.0];
const RESPAWN_GAP: f32 = 150.0;

/// A box of PD's space whose floors (a pad's floor point: the pad less
/// [`PAD_HEIGHT`]) it holds.
#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub lo: Vec3,
    pub hi: Vec3,
    pub what: &'static str,
}

impl Zone {
    fn of(b: Box3, scale: f32, what: &'static str) -> Zone {
        let (a, c) = (pd_point(b.0, scale), pd_point(b.1, scale));
        Zone { lo: a.min(c), hi: a.max(c), what }
    }

    /// A rectangle of half cells at a storey's height (units).
    fn flat(r: Rect, z: i32, scale: f32, what: &'static str) -> Zone {
        Zone::of(box_h(r, z - STOREY, z + STOREY), scale, what)
    }

    pub fn holds(&self, floor: Vec3) -> bool {
        floor.cmpge(self.lo).all() && floor.cmple(self.hi).all()
    }

    fn middle(&self) -> Vec3 {
        (self.lo + self.hi) * 0.5
    }
}

/// A rectangle of half cells between two heights, as a box in units (the
/// furnisher's `box_h`).
fn box_h(r: Rect, z0: i32, z1: i32) -> Box3 {
    box3(r.0 as f64 / 2.0, r.1 as f64 / 2.0, r.2 as f64 / 2.0, r.3 as f64 / 2.0, z0, z1)
}

/// A rectangle grown by `by` on every side.
fn grow(r: Rect, by: i32) -> Rect {
    (r.0 - by, r.1 - by, r.2 + by, r.3 + by)
}

/// What the generator knows of its level's floors, for placement.
#[derive(Clone, Debug, Default)]
pub struct Hints {
    /// By PD room number (the plan's `room_id`): the plan room's kind.
    pub kinds: Vec<&'static str>,
    /// Where nothing is placed.
    pub clear: Vec<Zone>,
    /// Where the power weapon may go, the riskiest first (by `what`: every
    /// zone of the first kind before any of the next).
    pub risky: Vec<Zone>,
    /// Place by these hints; false: only audit what the plain placement did.
    pub guide: bool,
}

/// The risky places, the riskiest first: out in a hall's air reached only by
/// angled catwalks, on a pillar's top up a ladder, out on a platform, on a
/// high catwalk reached only from outside, in a lookout pod, down a hatch,
/// in a pit in the floor.
pub const RISKY: [&str; 7] = ["island", "pillar top", "platform", "gallery", "perch", "cellar", "sunken floor"];

/// The hints of the built level `b`, in PD's space at `scale`.
pub fn hints(b: &Built, scale: f32) -> Hints {
    let plan = &b.level.plan;
    let mut kinds = vec![""; plan.rooms.len() + 1];
    for room in &plan.rooms {
        if let Some(k) = kinds.get_mut(room.room_id) {
            *k = room.kind;
        }
    }

    let mut clear = Vec::new();
    // Every ramp of the plan (the furnished rooms' open ramps, the spirals',
    // the risers', the sunken floors', the vents' ducts), from before its
    // foot to past its top.
    for room in &plan.rooms {
        for rp in &room.ramps {
            let (a, e) = (rp.start, rp.end);
            let half = rp.width / 2 + RAMP_SIDE;
            let (z0, z1) = (a.2.min(e.2) - RAMP_RISE, a.2.max(e.2) + RAMP_RISE);
            let b = if a.0 == e.0 {
                ((a.0 - half, a.1.min(e.1) - RAMP_END, z0), (a.0 + half, a.1.max(e.1) + RAMP_END, z1))
            } else {
                ((a.0.min(e.0) - RAMP_END, a.1 - half, z0), (a.0.max(e.0) + RAMP_END, a.1 + half, z1))
            };
            clear.push(Zone::of(b, scale, "ramp"));
        }
    }
    // Every opening: a doorway's mouth, or a hole in a floor and its rim.
    for o in b.openings() {
        let (lo, hi) = (o.lo, o.hi);
        let b = match o.axis {
            2 => ((lo.0 - RIM, lo.1 - RIM, lo.2 - STOREY), (hi.0 + RIM, hi.1 + RIM, lo.2 + STOREY)),
            0 => ((lo.0 - MOUTH, lo.1 - JAMB, lo.2 - STOREY), (lo.0 + MOUTH, hi.1 + JAMB, lo.2 + STOREY)),
            _ => ((lo.0 - JAMB, lo.1 - MOUTH, lo.2 - STOREY), (hi.0 + JAMB, lo.1 + MOUTH, lo.2 + STOREY)),
        };
        clear.push(Zone::of(b, scale, if o.axis == 2 { "hole" } else { "doorway" }));
    }

    let mut risky: Vec<Zone> = Vec::new();
    let room_floors = |key: &str, what: &'static str, out: &mut Vec<Zone>| {
        if let Some(room) = plan.get(key) {
            for (lo, hi) in &room.boxes {
                out.push(Zone::of(((lo.0, lo.1, lo.2 - STOREY), (hi.0, hi.1, lo.2 + STOREY)), scale, what));
            }
        }
    };
    for (_, int) in &b.level.interiors {
        // a pit through a platform (a hole, or a span of unrailed planks)
        for pit in &int.pits {
            clear.push(Zone::flat(grow(pit.hole, 1), pit.z, scale, "pit"));
        }
        for (z, r) in &int.islands {
            risky.push(Zone::flat(*r, *z, scale, "island"));
        }
        let floor = int.levels.first().copied().unwrap_or(int.s.floor);
        for l in &int.ladders {
            if let Some(c) = int.cover.get(l.pillar) {
                risky.push(Zone::flat((c.0, c.1, c.2, c.3), floor + c.4, scale, "pillar top"));
            }
        }
        for (z, r) in &int.platforms {
            risky.push(Zone::flat(*r, *z, scale, "platform"));
        }
        for z in &int.s.galleries {
            if let Some(deck) = int.deck.get(z) {
                risky.extend(rectangles(deck).into_iter().map(|r| Zone::flat(r, *z, scale, "gallery")));
            }
        }
        if int.s.cellar.is_some() {
            room_floors(&int.s.key, "cellar", &mut risky);
        }
        if let Some(sk) = &int.sunk {
            risky.push(Zone::flat(sk.rect, sk.z, scale, "sunken floor"));
        }
    }
    for room in plan.rooms.iter().filter(|r| r.kind == "perch") {
        room_floors(&room.key, "perch", &mut risky);
    }
    risky.sort_by_key(|z| RISKY.iter().position(|w| *w == z.what).unwrap_or(RISKY.len()));
    Hints { kinds, clear, risky, guide: true }
}

impl Hints {
    fn kind(&self, room: Option<u16>) -> &'static str {
        room.and_then(|r| self.kinds.get(r as usize)).copied().unwrap_or("")
    }

    fn clear_at(&self, floor: Vec3) -> Option<&Zone> {
        self.clear.iter().find(|z| z.holds(floor))
    }

    fn risky_at(&self, floor: Vec3) -> Option<usize> {
        self.risky.iter().position(|z| z.holds(floor))
    }
}

/// Is the floor at `f` flat round it: the ground under every point of a
/// ring `r` wide round it (and under it) the same floor, within [`FLAT`]?
/// A slope, a ledge's edge, a step, a hole or a wall inside the ring fails.
pub fn flat(level: &TileLevel, f: Vec3, r: f32) -> bool {
    let ground = |p: Vec3| level.cd_find_ground_at_cyl(p + Vec3::Y * 30.0, 0.5);
    let (y0, poly) = ground(f);
    if poly.is_none() || (y0 - f.y).abs() > 15.0 {
        return false;
    }
    (0..8).all(|k| {
        let a = k as f32 * FRAC_PI_4;
        let (y, poly) = ground(f + Vec3::new(a.sin(), 0.0, a.cos()) * r);
        poly.is_some() && (y - y0).abs() <= FLAT
    })
}

/// A pad near the waypoint pad `pad` whose floor is flat ([`flat`]): itself,
/// or moved up to [`SETTLE`] in sight of it at knee and pad height (no wall
/// between), the nearest first.
fn settle(level: &TileLevel, pad: Vec3) -> Option<Vec3> {
    let floor = pad - Vec3::Y * PAD_HEIGHT;
    let mut tries = vec![Vec3::ZERO];
    let steps = (SETTLE / SETTLE_STEP) as usize;
    for s in 1..=steps {
        for k in 0..8 {
            let a = k as f32 * FRAC_PI_4;
            tries.push(Vec3::new(a.sin(), 0.0, a.cos()) * (s as f32 * SETTLE_STEP));
        }
    }
    tries.into_iter().find_map(|d| {
        let f = floor + d;
        let (y, poly) = level.cd_find_ground_at_cyl(f + Vec3::Y * 30.0, 0.5);
        poly?;
        let f = Vec3::new(f.x, y, f.z);
        let seen = d == Vec3::ZERO || [30.0, PAD_HEIGHT].iter().all(|h| level.los(floor + Vec3::Y * *h, f + Vec3::Y * *h));
        (seen && (y - floor.y).abs() <= FLAT && flat(level, f, FLAT_RING)).then_some(f + Vec3::Y * PAD_HEIGHT)
    })
}

/// A place the guided placement may use: a waypoint's, settled on flat floor.
#[derive(Clone, Copy, Debug)]
struct Spot {
    /// The waypoint it is by.
    w: usize,
    /// Its pad.
    pad: Vec3,
    room: Option<u16>,
    kind: &'static str,
    /// The risky zone it is in.
    risky: Option<usize>,
    /// Of eight ways, how many have a wall within [`WALL_NEAR`] at knee height.
    wall: u8,
}

/// Of eight ways from the pad `pad`, how many have a wall within
/// [`WALL_NEAR`] at knee height.
fn walls(level: &TileLevel, pad: Vec3) -> u8 {
    let knee = pad - Vec3::Y * (PAD_HEIGHT - KNEE);
    (0..8)
        .filter(|&k| {
            let a = k as f32 * FRAC_PI_4;
            !level.los(knee, knee + Vec3::new(a.sin(), 0.0, a.cos()) * WALL_NEAR)
        })
        .count() as u8
}

/// How far apart [`Guide::spread`] keeps what it chooses: `gap` from each
/// other and `away` from the pads `keep` (walking), and with `by_room`, no
/// more in a room than it takes ([`ROOM_SPOTS`]).
struct Apart<'k> {
    gap: f32,
    keep: &'k [Vec3],
    away: f32,
    by_room: bool,
}

/// Walking over a graph's links.
struct Walker {
    adj: Vec<Vec<(usize, f32)>>,
    /// The links the other way round (for walks to somewhere).
    radj: Vec<Vec<(usize, f32)>>,
    pads: Vec<Vec3>,
    rooms: Vec<Option<u16>>,
    /// Room -> the rooms a link goes to or comes from.
    ways: BTreeMap<u16, BTreeSet<u16>>,
}

impl Walker {
    fn new(graph: &NavGraph) -> Walker {
        let n = graph.waypoints.len();
        let pads: Vec<Vec3> = (0..n).map(|w| graph.waypoint_pos(w)).collect();
        let rooms: Vec<Option<u16>> = (0..n).map(|w| graph.waypoint_room(w)).collect();
        let mut adj = vec![Vec::new(); n];
        let mut radj = vec![Vec::new(); n];
        let mut ways: BTreeMap<u16, BTreeSet<u16>> = BTreeMap::new();
        for (a, b, _) in directed_links(n, |w| &graph.waypoints[w].neighbours) {
            adj[a].push((b, pads[a].distance(pads[b])));
            radj[b].push((a, pads[a].distance(pads[b])));
            if let (Some(ra), Some(rb)) = (rooms[a], rooms[b]) {
                if ra != rb {
                    ways.entry(ra).or_default().insert(rb);
                    ways.entry(rb).or_default().insert(ra);
                }
            }
        }
        Walker { adj, radj, pads, rooms, ways }
    }

    /// How many rooms the room `room` is linked with (1: a dead end).
    fn ways(&self, room: Option<u16>) -> usize {
        room.and_then(|r| self.ways.get(&r)).map_or(0, |w| w.len())
    }

    /// Walking distances (cm along the links) from the waypoints `from`.
    fn walk(&self, from: &[usize]) -> Vec<f32> {
        if from.is_empty() {
            return vec![f32::INFINITY; self.pads.len()];
        }
        dijkstra(&self.adj, from, f32::INFINITY).into_iter().map(|d| d.0).collect()
    }

    /// Walking distances (cm along the links) from every waypoint to the
    /// nearest of `to`.
    fn walk_to(&self, to: &[usize]) -> Vec<f32> {
        if to.is_empty() {
            return vec![f32::INFINITY; self.pads.len()];
        }
        dijkstra(&self.radj, to, f32::INFINITY).into_iter().map(|d| d.0).collect()
    }

    /// The waypoint nearest the pad `p` (on its level, else anywhere).
    fn near(&self, p: Vec3) -> usize {
        let d = |w: usize| self.pads[w].distance(p) + if (self.pads[w].y - p.y).abs() > 100.0 { 1e6 } else { 0.0 };
        (0..self.pads.len()).min_by(|&a, &b| d(a).total_cmp(&d(b))).unwrap_or(0)
    }

    /// The walk (cm) from the pad `from` to the pad `to`, by their nearest
    /// waypoints.
    fn between(&self, from: Vec3, to: Vec3) -> f32 {
        let (a, b) = (self.near(from), self.near(to));
        self.walk(&[a])[b] + self.pads[a].distance(from) + self.pads[b].distance(to)
    }
}

/// The guided placement over one level's graph.
pub struct Guide<'a> {
    hints: &'a Hints,
    walker: Walker,
    spots: Vec<Spot>,
    /// Room -> its spots.
    per_room: BTreeMap<Option<u16>, usize>,
    /// Of the usable waypoints, how many were left out: off flat floor, and
    /// in a clear zone (by its `what`).
    pub sloped: usize,
    pub cleared: Vec<(&'static str, usize)>,
}

impl<'a> Guide<'a> {
    /// The guide over `graph`'s waypoints `usable` (the placement's: one
    /// strongly connected part, no crouch or duck spots).
    pub fn new(hints: &'a Hints, level: &TileLevel, graph: &NavGraph, usable: &[usize]) -> Guide<'a> {
        let walker = Walker::new(graph);
        let (mut sloped, mut cleared): (usize, Vec<(&'static str, usize)>) = (0, Vec::new());
        let mut spots = Vec::new();
        for &w in usable {
            let Some(pad) = settle(level, walker.pads[w]) else {
                sloped += 1;
                continue;
            };
            let floor = pad - Vec3::Y * PAD_HEIGHT;
            if let Some(z) = hints.clear_at(floor) {
                match cleared.iter_mut().find(|c| c.0 == z.what) {
                    Some(c) => c.1 += 1,
                    None => cleared.push((z.what, 1)),
                }
                continue;
            }
            let room = graph.waypoint_room(w);
            spots.push(Spot { w, pad, room, kind: hints.kind(room), risky: hints.risky_at(floor), wall: walls(level, pad) });
        }
        let mut per_room = BTreeMap::new();
        for s in &spots {
            *per_room.entry(s.room).or_insert(0) += 1;
        }
        Guide { hints, walker, spots, per_room, sloped, cleared }
    }

    /// How many spots there are to place on.
    pub fn len(&self) -> usize {
        self.spots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spots.is_empty()
    }

    /// Every spot's pad.
    pub fn pads(&self) -> Vec<Vec3> {
        self.spots.iter().map(|s| s.pad).collect()
    }

    /// The waypoint of the pad `p`: its spot's, else the nearest.
    fn w_of(&self, p: Vec3) -> usize {
        self.spots.iter().find(|s| s.pad == p).map_or_else(|| self.walker.near(p), |s| s.w)
    }

    /// The room the pad `p` is in: its spot's, else its nearest waypoint's.
    fn room_of(&self, p: Vec3) -> Option<u16> {
        self.spots.iter().find(|s| s.pad == p).map_or_else(|| self.walker.rooms[self.walker.near(p)], |s| s.room)
    }

    /// How many weapons the room `room` takes before any room takes more.
    fn takes(&self, room: Option<u16>) -> usize {
        1 + self.per_room.get(&room).copied().unwrap_or(0) / ROOM_SPOTS
    }

    /// Each room's middle, of the rooms `ok` takes (its spots' kind, how
    /// many rooms it is linked with, how many spots it has): the open spot
    /// (no wall beside it, no risky place) nearest the middle of its spots
    /// on its lowest floor.
    fn middles(&self, ok: impl Fn(&'static str, usize, usize) -> bool) -> Vec<usize> {
        let mut out = Vec::new();
        for (&room, &n) in &self.per_room {
            let Some(first) = self.spots.iter().find(|s| s.room == room) else { continue };
            if room.is_none() || !ok(first.kind, self.walker.ways(room), n) {
                continue;
            }
            let ground = self.spots.iter().filter(|s| s.room == room).map(|s| s.pad.y).fold(f32::INFINITY, f32::min);
            let low: Vec<usize> = (0..self.spots.len()).filter(|&i| self.spots[i].room == room && self.spots[i].pad.y < ground + 100.0 && self.spots[i].risky.is_none()).collect();
            if low.is_empty() {
                continue;
            }
            let m = low.iter().map(|&i| self.spots[i].pad).sum::<Vec3>() / low.len() as f32;
            let d = |i: usize| self.spots[i].pad.distance(m) + if self.spots[i].wall > 0 { 1e6 } else { 0.0 };
            out.extend(low.iter().copied().min_by(|&a, &b| d(a).total_cmp(&d(b))));
        }
        out
    }

    fn walk(&self, from: &[usize]) -> Vec<f32> {
        self.walker.walk(from)
    }

    /// The power weapon's spot: in the riskiest kind of place the level
    /// has, the zone of that kind nearest the level's middle, the spot
    /// nearest that zone's middle. With what the place is.
    pub fn prize(&self) -> Option<(Vec3, &'static str)> {
        let first = self.spots.iter().filter_map(|s| s.risky).min()?;
        let what = self.hints.risky[first].what;
        let middle = self.spots.iter().map(|s| s.pad).sum::<Vec3>() / self.spots.len() as f32;
        let zone = self
            .spots
            .iter()
            .filter_map(|s| s.risky)
            .filter(|&z| self.hints.risky[z].what == what)
            .min_by(|&a, &b| self.hints.risky[a].middle().distance(middle).total_cmp(&self.hints.risky[b].middle().distance(middle)))?;
        let m = self.hints.risky[zone].middle();
        let flat_d = |s: &Spot| (s.pad.x - m.x).hypot(s.pad.z - m.z);
        self.spots.iter().filter(|s| s.risky == Some(zone)).min_by(|a, b| flat_d(a).total_cmp(&flat_d(b))).map(|s| (s.pad, what))
    }

    /// Farthest-point sampling by walking distance: add to `chosen` (pads)
    /// the spot of `pool` furthest from everything chosen, while it is at
    /// least `gap` from it, until `count`; none nearer than `away` to the
    /// pads `keep`, and with `by_room` none in a room that has as many as it
    /// takes. With nothing chosen, the first is the furthest from the spot
    /// nearest the middle of the pool.
    fn spread(&self, pool: &[usize], chosen: &mut Vec<Vec3>, count: usize, apart: Apart) {
        let Apart { gap, keep, away, by_room } = apart;
        if pool.is_empty() {
            return;
        }
        let ws = |pads: &[Vec3]| pads.iter().map(|p| self.w_of(*p)).collect::<Vec<_>>();
        let kept = self.walk(&ws(keep));
        let pool: Vec<usize> = pool.iter().copied().filter(|&i| kept[self.spots[i].w] >= away).collect();
        if pool.is_empty() {
            return;
        }
        let mut near = if chosen.is_empty() {
            let middle = pool.iter().map(|&i| self.spots[i].pad).sum::<Vec3>() / pool.len() as f32;
            let centre = *pool.iter().min_by(|&&a, &&b| self.spots[a].pad.distance(middle).total_cmp(&self.spots[b].pad.distance(middle))).unwrap();
            let d = self.walk(&[self.spots[centre].w]);
            // (the centre itself is no choice yet: any gap lets the first through)
            d.into_iter().map(|x| x + gap).collect()
        } else {
            self.walk(&ws(chosen))
        };
        let mut held: BTreeMap<Option<u16>, usize> = BTreeMap::new();
        for p in chosen.iter() {
            *held.entry(self.room_of(*p)).or_insert(0) += 1;
        }
        while chosen.len() < count {
            let full = |i: usize| by_room && held.get(&self.spots[i].room).copied().unwrap_or(0) >= self.takes(self.spots[i].room);
            let best = pool.iter().copied().filter(|&i| !chosen.contains(&self.spots[i].pad) && !full(i)).max_by(|&a, &b| near[self.spots[a].w].total_cmp(&near[self.spots[b].w]).then(b.cmp(&a)));
            match best {
                Some(i) if near[self.spots[i].w] >= gap => {
                    chosen.push(self.spots[i].pad);
                    *held.entry(self.spots[i].room).or_insert(0) += 1;
                    for (x, d) in near.iter_mut().zip(self.walk(&[self.spots[i].w])) {
                        *x = x.min(d);
                    }
                }
                _ => break,
            }
        }
    }

    fn pool(&self, ok: impl Fn(&Spot) -> bool) -> Vec<usize> {
        (0..self.spots.len()).filter(|&i| ok(&self.spots[i])).collect()
    }

    /// `count` spawns, after the weapons (`weapons`, the power weapon
    /// `prize` among them): out of corridors, ramps and vents and off the
    /// risky places, [`PRIZE_AWAY`] from the power weapon and [`SPAWN_AWAY`]
    /// from every weapon, spread; within the walks of [`SPAWN_REACH`] of a
    /// weapon while that leaves room for them all, corridors let in only if the rest has
    /// none. With how many stand in corridors.
    pub fn spawns(&self, count: usize, prize: Option<Vec3>, weapons: &[Vec3]) -> (Vec<Vec3>, usize) {
        let ws = |pads: &[Vec3]| pads.iter().map(|p| self.w_of(*p)).collect::<Vec<_>>();
        let from_prize = prize.map_or_else(|| vec![f32::INFINITY; self.walker.pads.len()], |p| self.walk(&[self.w_of(p)]));
        // (a spawn's own walk to a weapon: a drop is quick one way only)
        let to_weapon = if weapons.is_empty() { vec![0.0; self.walker.pads.len()] } else { self.walker.walk_to(&ws(weapons)) };
        let ok = |s: &Spot, no: &[&str]| !no.contains(&s.kind) && s.risky.is_none() && from_prize[s.w] >= PRIZE_AWAY;
        let apart = || Apart { gap: SPAWN_GAP, keep: weapons, away: SPAWN_AWAY, by_room: false };
        let mut out = Vec::new();
        for reach in SPAWN_REACH {
            self.spread(&self.pool(|s| ok(s, &NO_SPAWN) && to_weapon[s.w] <= reach), &mut out, count, apart());
        }
        self.spread(&self.pool(|s| ok(s, &NO_SPAWN)), &mut out, count, apart());
        let rooms = out.len();
        self.spread(&self.pool(|s| ok(s, &NO_ITEM)), &mut out, count, apart());
        let in_corridors = out.len() - rooms;
        (out, in_corridors)
    }

    /// `count` weapon spots after `have` (the power weapon's, if any), out
    /// of ramps and vents, [`SPAWN_AWAY`] from `spawns` (a layout's: a
    /// generated level's spawns come after its weapons), spread: against a
    /// wall in the dead ends first (up to a third of them), then against a
    /// wall in the other rooms, as many to a room as it takes; then anywhere
    /// in the rooms (each with [`ROOM_MIN`] spots, room for the crates); then
    /// anywhere but ramps and vents.
    /// With how many went to the dead ends.
    pub fn weapons(&self, count: usize, have: &[Vec3], spawns: &[Vec3]) -> (Vec<Vec3>, usize) {
        // (a room with too few spots has no room for the weapon's crates)
        let roomy = |s: &Spot| is_room(s.kind) && self.per_room.get(&s.room).copied().unwrap_or(0) >= ROOM_MIN;
        let against = |s: &Spot| s.wall > 0;
        let apart = |by_room| Apart { gap: WEAPON_GAP, keep: spawns, away: SPAWN_AWAY, by_room };
        let mut out = have.to_vec();
        let first = (have.len() + count / DEAD_END_SHARE).min(count);
        let dead_end = |s: &Spot| is_room(s.kind) && self.walker.ways(s.room) == 1 && self.per_room.get(&s.room).copied().unwrap_or(0) >= DEAD_END_MIN;
        self.spread(&self.pool(|s| dead_end(s) && against(s)), &mut out, first, apart(true));
        let dead_ends = out.len() - have.len();
        for (pool, by_room) in [(self.pool(|s| roomy(s) && against(s)), true), (self.pool(roomy), true), (self.pool(roomy), false), (self.pool(|s| is_room(s.kind)), false), (self.pool(|s| !NO_ITEM.contains(&s.kind)), false)] {
            self.spread(&pool, &mut out, count, apart(by_room));
        }
        (out, dead_ends)
    }

    /// `count` hills: the middles of halls and rooms, spread (else of any
    /// room but a ramp's or a vent's).
    pub fn hills(&self, count: usize) -> Vec<Vec3> {
        let apart = || Apart { gap: 0.0, keep: &[], away: 0.0, by_room: false };
        let mut out = Vec::new();
        self.spread(&self.middles(|kind, _, n| ROOMY.contains(&kind) && n >= ROOM_MIN), &mut out, count, apart());
        self.spread(&self.middles(|kind, _, n| !NO_ITEM.contains(&kind) && n >= ROOM_MIN), &mut out, count, apart());
        out
    }

    /// `count` Capture the Case bases: the middles of halls and rooms with
    /// two ways in or more and [`BASE_ROOM_MIN`] spots, as far apart as walking allows (else any hall's
    /// or room's, else any room's).
    pub fn bases(&self, count: usize) -> Vec<Vec3> {
        let apart = || Apart { gap: 0.0, keep: &[], away: 0.0, by_room: false };
        let mut out = Vec::new();
        self.spread(&self.middles(|kind, ways, n| ROOMY.contains(&kind) && ways >= 2 && n >= BASE_ROOM_MIN), &mut out, count, apart());
        self.spread(&self.middles(|kind, _, n| ROOMY.contains(&kind) && n >= BASE_ROOM_MIN), &mut out, count, apart());
        self.spread(&self.middles(|kind, _, _| !NO_ITEM.contains(&kind)), &mut out, count, apart());
        out
    }

    /// `per` respawn pads for each base of `bases`: nearer it than any other
    /// base and within [`RESPAWN_WALK`] of it walking, out of corridors and
    /// off the risky places (corridors let in if that is too few), apart.
    pub fn respawns(&self, bases: &[Vec3], per: usize) -> Vec<Vec<Vec3>> {
        let walks: Vec<Vec<f32>> = bases.iter().map(|b| self.walk(&[self.w_of(*b)])).collect();
        let mut out = Vec::new();
        for (i, b) in bases.iter().enumerate() {
            let ours = |s: &Spot, within: f32| {
                let d = walks[i][s.w];
                d <= within && s.risky.is_none() && walks.iter().enumerate().all(|(j, o)| j == i || d < o[s.w])
            };
            let mut chosen = vec![*b];
            for within in RESPAWN_WALK {
                for no in [&NO_SPAWN[..], &NO_ITEM[..]] {
                    let near: Vec<Vec3> = self.spots.iter().filter(|s| ours(s, within) && !no.contains(&s.kind)).map(|s| s.pad).collect();
                    crate::place::farthest(&near, &mut chosen, per + 1, RESPAWN_GAP, &[], 0.0);
                }
            }
            out.push(chosen[1..].to_vec());
        }
        out
    }

    /// Two crates for the weapon at `weapon`: in its room, [`AMMO_NEAR`]
    /// from it walking, else [`AMMO_ROOM`], apart from each other and [`AMMO_CLEAR`] from
    /// `placed`; else anywhere within [`AMMO_WIDE`]. With whether the wide
    /// search was needed.
    pub fn ammo(&self, weapon: Vec3, placed: &[Vec3]) -> (Vec<Vec3>, bool) {
        let w = self.w_of(weapon);
        let room = self.room_of(weapon);
        let walk = self.walk(&[w]);
        let free = |s: &Spot| !NO_ITEM.contains(&s.kind) && placed.iter().all(|p| p.distance(s.pad) >= AMMO_CLEAR);
        let mut pair = vec![weapon];
        let mut wide = false;
        for (same_room, (lo, hi)) in [(true, AMMO_NEAR), (true, AMMO_ROOM), (false, AMMO_WIDE)] {
            if pair.len() >= 3 {
                break;
            }
            wide |= !same_room;
            let ring: Vec<Vec3> = self.spots.iter().filter(|s| (!same_room || s.room == room) && (lo..=hi).contains(&walk[s.w]) && free(s)).map(|s| s.pad).collect();
            crate::place::farthest(&ring, &mut pair, 3, AMMO_GAP, &[], 0.0);
        }
        (pair[1..].to_vec(), wide)
    }
}

/// A room of a kind a weapon goes in first: not a corridor, a ramp or a vent.
fn is_room(kind: &str) -> bool {
    !NO_ITEM.contains(&kind) && kind != "corridor"
}

/// What a placement placed, as pads (each crate with its weapon's index).
pub struct Placement<'p> {
    pub spawns: &'p [Vec3],
    pub weapons: &'p [Vec3],
    pub ammo: &'p [(Vec3, usize)],
    pub hills: &'p [Vec3],
    pub bases: &'p [Vec3],
}

/// What a placement did, as the hints judge it: two lines for the report,
/// where things stand and how fair it is.
pub fn audit(hints: &Hints, level: &TileLevel, graph: &NavGraph, p: &Placement) -> [String; 2] {
    let Placement { spawns, weapons, ammo, hills, bases } = *p;
    let walker = Walker::new(graph);
    let room = |p: Vec3| graph.waypoint_room(walker.near(p));
    let all: Vec<Vec3> = spawns.iter().chain(weapons).copied().chain(ammo.iter().map(|a| a.0)).collect();
    let off_flat = all.iter().filter(|p| !flat(level, **p - Vec3::Y * PAD_HEIGHT, 30.0)).count();
    let in_clear = all.iter().filter(|p| hints.clear_at(**p - Vec3::Y * PAD_HEIGHT).is_some()).count();
    let corridor = spawns.iter().filter(|p| NO_SPAWN.contains(&hints.kind(room(**p)))).count();
    let walks: Vec<f32> = ammo.iter().filter_map(|(a, i)| weapons.get(*i).map(|w| walker.between(*w, *a))).collect();
    let other_room = ammo.iter().filter(|(a, i)| weapons.get(*i).is_some_and(|w| room(*w) != room(*a))).count();
    let longest = walks.iter().copied().fold(0.0f32, f32::max);
    let mean = if walks.is_empty() { 0.0 } else { walks.iter().sum::<f32>() / walks.len() as f32 };
    let prize = weapons.get(5).map_or("none".to_string(), |w| hints.risky_at(*w - Vec3::Y * PAD_HEIGHT).map_or("not a risky place".to_string(), |z| hints.risky[z].what.to_string()));
    let line = format!(
        "placement audit: {off_flat} of {} items off flat floor, {in_clear} on a ramp or in a doorway or by a hole, {corridor} of {} spawns in corridors, ammo {mean:.0} cm walk from its weapon on average ({longest:.0} at most; {other_room} of {} crates not in their weapon's room), location 5 on {prize}",
        all.len(),
        spawns.len(),
        ammo.len()
    );

    // Fairness: each spawn's walk to its nearest weapon; the dead ends
    // (rooms one way in, not corridors) with a weapon; the most weapons in a
    // room; hills not in a hall or a room; bases with one way in.
    let weapon_ws: Vec<usize> = weapons.iter().map(|w| walker.near(*w)).collect();
    let to_any = walker.walk_to(&weapon_ws);
    let to_weapon: Vec<f32> = spawns.iter().map(|s| to_any[walker.near(*s)]).filter(|d| d.is_finite()).collect();
    let (near, far) = to_weapon.iter().fold((f32::INFINITY, 0.0f32), |(a, b), &d| (a.min(d), b.max(d)));
    let mean = if to_weapon.is_empty() { 0.0 } else { to_weapon.iter().sum::<f32>() / to_weapon.len() as f32 };
    let rooms_used: BTreeSet<u16> = walker.rooms.iter().flatten().copied().collect();
    let dead: Vec<u16> = rooms_used.iter().copied().filter(|&r| walker.ways(Some(r)) == 1 && is_room(hints.kind(Some(r)))).collect();
    let weapon_rooms: Vec<Option<u16>> = weapons.iter().map(|w| room(*w)).collect();
    let armed = dead.iter().filter(|&&r| weapon_rooms.contains(&Some(r))).count();
    let most = weapon_rooms.iter().map(|r| weapon_rooms.iter().filter(|x| *x == r).count()).max().unwrap_or(0);
    let hills_out = hills.iter().filter(|h| !ROOMY.contains(&hints.kind(room(**h)))).count();
    let bases_one = bases.iter().filter(|b| walker.ways(room(**b)) < 2).count();
    let fair = format!(
        "placement fairness: a spawn's walk to its nearest weapon {:.0} to {far:.0} cm ({mean:.0} on average), {armed} of {} dead ends with a weapon, at most {most} weapons in a room, {hills_out} of {} hills not in a hall or a room, {bases_one} of {} bases with one way in",
        if near.is_finite() { near } else { 0.0 },
        dead.len(),
        hills.len(),
        bases.len()
    );
    [line, fair]
}

#[cfg(test)]
mod tests;
