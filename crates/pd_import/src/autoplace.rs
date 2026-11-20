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
//! than straight lines, keeps spawns out of corridors, and puts each
//! weapon's two crates in its room a short walk from it.
//!
//! None of this is PD's: PD's arenas were placed by hand. A layout
//! (`levels/<code>.layout.json`, the editor's) still wins over all of it,
//! and `plain_placement` in the recipe's source puts it back as any custom
//! level's.

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

/// The spawns keep this far (cm walking) from the power weapon, and the
/// weapons this far from the spawns.
const PRIZE_AWAY: f32 = 1000.0;
const SPAWN_AWAY: f32 = 500.0;
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
}

/// Walking over a graph's links.
struct Walker {
    adj: Vec<Vec<(usize, f32)>>,
    pads: Vec<Vec3>,
}

impl Walker {
    fn new(graph: &NavGraph) -> Walker {
        let n = graph.waypoints.len();
        let pads: Vec<Vec3> = (0..n).map(|w| graph.waypoint_pos(w)).collect();
        let mut adj = vec![Vec::new(); n];
        for (a, b, _) in directed_links(n, |w| &graph.waypoints[w].neighbours) {
            adj[a].push((b, pads[a].distance(pads[b])));
        }
        Walker { adj, pads }
    }

    /// Walking distances (cm along the links) from the waypoints `from`.
    fn walk(&self, from: &[usize]) -> Vec<f32> {
        if from.is_empty() {
            return vec![f32::INFINITY; self.pads.len()];
        }
        dijkstra(&self.adj, from, f32::INFINITY).into_iter().map(|d| d.0).collect()
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
            spots.push(Spot { w, pad, room, kind: hints.kind(room), risky: hints.risky_at(floor) });
        }
        Guide { hints, walker, spots, sloped, cleared }
    }

    /// How many spots there are to place on.
    pub fn len(&self) -> usize {
        self.spots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spots.is_empty()
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
    /// pads `keep`. With nothing chosen, the first is the furthest from the
    /// spot nearest the middle of the pool.
    fn spread(&self, pool: &[usize], chosen: &mut Vec<Vec3>, count: usize, gap: f32, keep: &[Vec3], away: f32) {
        if pool.is_empty() {
            return;
        }
        let ws = |pads: &[Vec3]| pads.iter().map(|p| self.walker.near(*p)).collect::<Vec<_>>();
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
        while chosen.len() < count {
            let best = pool.iter().copied().filter(|&i| !chosen.contains(&self.spots[i].pad)).max_by(|&a, &b| near[self.spots[a].w].total_cmp(&near[self.spots[b].w]).then(b.cmp(&a)));
            match best {
                Some(i) if near[self.spots[i].w] >= gap => {
                    chosen.push(self.spots[i].pad);
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

    /// `count` spawns: out of corridors, ramps and vents and off the risky
    /// places, [`PRIZE_AWAY`] from the power weapon `prize`, spread. Corridors
    /// are let in only if the rest has no room for them all. With how many
    /// stand in corridors.
    pub fn spawns(&self, count: usize, prize: Option<Vec3>) -> (Vec<Vec3>, usize) {
        let keep: Vec<Vec3> = prize.into_iter().collect();
        let mut out = Vec::new();
        self.spread(&self.pool(|s| !NO_SPAWN.contains(&s.kind) && s.risky.is_none()), &mut out, count, SPAWN_GAP, &keep, PRIZE_AWAY);
        let rooms = out.len();
        self.spread(&self.pool(|s| !NO_ITEM.contains(&s.kind) && s.risky.is_none()), &mut out, count, SPAWN_GAP, &keep, PRIZE_AWAY);
        let in_corridors = out.len() - rooms;
        (out, in_corridors)
    }

    /// `count` weapon spots after `have` (the power weapon's, if any): out of
    /// ramps and vents, [`SPAWN_AWAY`] from the spawns, spread; corridors
    /// only if the rooms have no room for them all.
    pub fn weapons(&self, count: usize, have: &[Vec3], spawns: &[Vec3]) -> Vec<Vec3> {
        let mut out = have.to_vec();
        self.spread(&self.pool(|s| !NO_ITEM.contains(&s.kind) && s.kind != "corridor"), &mut out, count, WEAPON_GAP, spawns, SPAWN_AWAY);
        self.spread(&self.pool(|s| !NO_ITEM.contains(&s.kind)), &mut out, count, WEAPON_GAP, spawns, SPAWN_AWAY);
        out
    }

    /// Two crates for the weapon at `weapon`: in its room, [`AMMO_NEAR`]
    /// from it walking, else [`AMMO_ROOM`], apart from each other and [`AMMO_CLEAR`] from
    /// `placed`; else anywhere within [`AMMO_WIDE`]. With whether the wide
    /// search was needed.
    pub fn ammo(&self, weapon: Vec3, placed: &[Vec3]) -> (Vec<Vec3>, bool) {
        let w = self.walker.near(weapon);
        let room = self.spots.iter().find(|s| s.pad == weapon).or_else(|| self.spots.iter().find(|s| s.w == w)).and_then(|s| s.room);
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

/// What a placement did, as the hints judge it: a line for the report.
/// `spawns`, `weapons` and `ammo` (each crate with its weapon's index) are
/// pads.
pub fn audit(hints: &Hints, level: &TileLevel, graph: &NavGraph, spawns: &[Vec3], weapons: &[Vec3], ammo: &[(Vec3, usize)]) -> String {
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
    format!(
        "placement audit: {off_flat} of {} items off flat floor, {in_clear} on a ramp or in a doorway or by a hole, {corridor} of {} spawns in corridors, ammo {mean:.0} cm walk from its weapon on average ({longest:.0} at most; {other_room} of {} crates not in their weapon's room), location 5 on {prize}",
        all.len(),
        spawns.len(),
        ammo.len()
    )
}

#[cfg(test)]
mod tests;
