//! Navigation in PD's own format (pads, waypoints, waygroups): the
//! `padhalllv.c` port ([`NavGraph`]: `nav_find_route`,
//! `waypoint_find_closest_to_pos`, the seeded tie-breaks), our generator
//! ([`gen`]), which builds a graph in the same format from geometry alone, and
//! the static checks S1–S3 that compare two graphs ([`check`]; S4 walks a graph
//! with the chr movement code, `crate::harness`).
//!
//! How PD routes (`padhalllv.c:15-41`): Dijkstra with a cost of 1 per segment,
//! first between waygroups, then between waypoints inside each group along that
//! group route, writing at most `arrlen - 1` waypoints plus a terminator. Ties
//! between equally short paths are broken at random, or, when the caller has set
//! a nav seed (`chr_go_to_room_pos` always does), by that seed.
//!
//! The same code routes on PD's hand-placed graph ([`NavGraph::from_stage`]) and
//! on ours ([`gen::generate`]), so the graph is the only thing that differs
//! between them (docs/ARCHITECTURE.md, D8). PD's routing state (`step`) lives
//! behind a lock here, so routing needs only `&NavGraph`.
//!
//! `// SUBST:` PD finds a pad's room with the BSP (`setup_prepare_pads`) and a
//! room's neighbours through its portals (`bg_room_get_neighbours`) / a pad's
//! room is the room of the floor under it and neighbours come from
//! [`TileLevel::rooms_are_neighbours`] (M9: the BG's rooms). Candidate waypoints
//! are gathered by room exactly as PD does.
//!
//! Sources: the old repo's `pd_spike/pd_nav.rs`, `navgen.rs`, `navcheck.rs`,
//! `waypoints.rs`.

pub mod check;
pub mod gen;

use std::collections::BTreeMap;
use std::sync::Mutex;

use glam::{Vec2, Vec3};
use pd_core::ids::{PADFLAG_AICROUCH, PADFLAG_AIDUCK, PADFLAG_AIWALKDIRECT};
use pd_core::rng::Rng;

use crate::stage::{wpseg_get_id, CdResult, Stage, TileLevel, WPSEGFLAG_INWARDSONLY, WPSEGFLAG_OUTWARDSONLY};

/// `MAX_CHRWAYPOINTS` (`constants.h:20`).
pub const MAX_CHRWAYPOINTS: usize = 6;
const IGNORE_OUTWARDS: i32 = WPSEGFLAG_OUTWARDSONLY;
const IGNORE_INWARDS: i32 = WPSEGFLAG_INWARDSONLY;

/// The `PADFLAG_AI*` bits the go-to code reads (`chr_tick_gopos`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadFlags {
    /// `PADFLAG_AIWALKDIRECT`: don't skip past this pad.
    pub walkdirect: bool,
    /// `PADFLAG_AICROUCH` / `PADFLAG_AIDUCK`: heading here, crouch / duck.
    pub crouch: bool,
    pub duck: bool,
}

impl PadFlags {
    pub fn from_bits(flags: u32) -> PadFlags {
        PadFlags { walkdirect: flags & PADFLAG_AIWALKDIRECT != 0, crouch: flags & PADFLAG_AICROUCH != 0, duck: flags & PADFLAG_AIDUCK != 0 }
    }
}

#[derive(Clone, Debug)]
pub struct NavPad {
    pub pos: Vec3,
    pub room: Option<u16>,
    pub flags: PadFlags,
}

/// `struct waypoint`: `neighbours` in PD's encoding (id | `WPSEGFLAG_*`).
#[derive(Clone, Debug)]
pub struct NavWaypoint {
    pub padnum: usize,
    pub neighbours: Vec<i32>,
    pub groupnum: usize,
}

/// `struct waygroup`.
#[derive(Clone, Debug)]
pub struct NavWaygroup {
    pub neighbours: Vec<i32>,
    pub waypoints: Vec<usize>,
}

/// The nav seed (`g_NavSeed`, `nav_set_seed`): `(0, 0)` means "use `random()`".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavSeed(pub u32, pub u32);

/// `CHRNAVSEED(chr)` (`constants.h:59`): `(lvframe60 >> 9) * 128 + chrnum * 8`,
/// both halves of the seed, so a chr's routes are stable for ~8.5 s.
/// `// SUBST:` PD's `chr->chrnum` / the chr's index in the world's chr list.
pub fn chrnavseed(lvframe60: i32, chrnum: usize) -> NavSeed {
    let v = ((lvframe60 >> 9) as u32).wrapping_mul(128).wrapping_add(chrnum as u32 * 8);
    NavSeed(v, v)
}

#[derive(Debug)]
pub struct NavGraph {
    pub pads: Vec<NavPad>,
    pub waypoints: Vec<NavWaypoint>,
    pub waygroups: Vec<NavWaygroup>,
    /// `g_Rooms[room].firstwaypoint/numwaypoints`: waypoints by their pad's room.
    room_waypoints: BTreeMap<u16, Vec<usize>>,
    /// The `step` fields PD keeps in each waypoint and waygroup.
    steps: Mutex<Steps>,
}

/// `waypoint.step` and `waygroup.step`: the routing's scratch.
#[derive(Debug)]
struct Steps {
    wp: Vec<i32>,
    group: Vec<i32>,
}

impl Clone for NavGraph {
    fn clone(&self) -> Self {
        NavGraph::new(self.pads.clone(), self.waypoints.clone(), self.waygroups.clone())
    }
}

impl NavGraph {
    pub fn new(pads: Vec<NavPad>, waypoints: Vec<NavWaypoint>, waygroups: Vec<NavWaygroup>) -> Self {
        let mut room_waypoints: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
        for (i, w) in waypoints.iter().enumerate() {
            if let Some(r) = pads[w.padnum].room {
                room_waypoints.entry(r).or_default().push(i);
            }
        }
        let steps = Mutex::new(Steps { wp: vec![-1; waypoints.len()], group: vec![-1; waygroups.len()] });
        NavGraph { pads, waypoints, waygroups, room_waypoints, steps }
    }

    /// PD's own graph for a stage (its pads file): every pad with its
    /// `PADFLAG_AI*` bits and, `// SUBST:` for `setup_prepare_pads`' BSP lookup,
    /// the room of the floor under it.
    pub fn from_stage(stage: &Stage, level: &TileLevel) -> Self {
        let pads = stage.pads.iter().map(|p| NavPad { pos: p.pos, room: level.floor_room(p.pos, 20.0), flags: PadFlags::from_bits(p.flags) }).collect();
        let waypoints = stage.waypoints.iter().map(|w| NavWaypoint { padnum: w.padnum, neighbours: w.neighbours.clone(), groupnum: w.groupnum }).collect();
        let waygroups = stage.waygroups.iter().map(|g| NavWaygroup { neighbours: g.neighbours.clone(), waypoints: g.waypoints.clone() }).collect();
        Self::new(pads, waypoints, waygroups)
    }

    /// A graph with no waypoints at all: every go-to fails.
    pub fn empty() -> Self {
        Self::new(Vec::new(), Vec::new(), Vec::new())
    }

    /// `// SUBST:` a test stage (the simulant spike's arena) has no hand-placed
    /// waypoints / a regular grid over its floor at `spacing` cm: points where a
    /// 30 cm cylinder fits, 8-neighbours linked where a chr's cylinder can sweep
    /// between them, raised to PD's usual 53 cm pad height, one waygroup. It
    /// routes by hop count like any PD graph.
    pub fn grid(level: &TileLevel, spacing: f32) -> Self {
        let (lo, hi) = level.geom.bounds();
        let nx = ((hi.x - lo.x) / spacing).floor() as i32;
        let nz = ((hi.z - lo.z) / spacing).floor() as i32;
        let ox = lo.x + ((hi.x - lo.x) - (nx - 1) as f32 * spacing) * 0.5;
        let oz = lo.z + ((hi.z - lo.z) - (nz - 1) as f32 * spacing) * 0.5;
        let mut pos: Vec<Vec3> = Vec::new();
        let mut index = vec![vec![None; nz.max(0) as usize]; nx.max(0) as usize];
        for ix in 0..nx {
            for iz in 0..nz {
                let p = Vec2::new(ox + ix as f32 * spacing, oz + iz as f32 * spacing);
                let (ground, poly) = level.cd_find_ground_at_cyl(Vec3::new(p.x, hi.y, p.y), 20.0);
                if poly.is_none() {
                    continue;
                }
                let floor = Vec3::new(p.x, ground, p.y);
                if level.cd_test_volume_simple(floor + Vec3::Y * 50.0, 30.0, true, 135.0, -30.0, &[]) == CdResult::NoCollision {
                    index[ix as usize][iz as usize] = Some(pos.len());
                    pos.push(floor);
                }
            }
        }
        let mut edges: Vec<Vec<i32>> = vec![Vec::new(); pos.len()];
        for ix in 0..nx {
            for iz in 0..nz {
                let Some(a) = index[ix as usize][iz as usize] else { continue };
                for (dx, dz) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
                    let (jx, jz) = (ix + dx, iz + dz);
                    if jx < 0 || jz < 0 || jx >= nx || jz >= nz {
                        continue;
                    }
                    let Some(b) = index[jx as usize][jz as usize] else { continue };
                    let (pa, pb) = (pos[a] + Vec3::Y * 50.0, pos[b] + Vec3::Y * 50.0);
                    let d = Vec2::new(pb.x - pa.x, pb.z - pa.z).normalize_or_zero();
                    let clear = [-20.0f32, 0.0, 20.0].iter().all(|&side| {
                        let off = Vec3::new(d.y * side, 0.0, -d.x * side);
                        level.cd_test_cylmove_oobok(pa + off, pb + off, 135.0, -30.0, &[]) == CdResult::NoCollision
                    });
                    if clear {
                        edges[a].push(b as i32);
                        edges[b].push(a as i32);
                    }
                }
            }
        }
        let pads = pos
            .iter()
            .map(|p| {
                let pos = *p + Vec3::Y * 53.0;
                NavPad { pos, room: level.floor_room(pos, 20.0), flags: PadFlags::default() }
            })
            .collect();
        let waypoints = edges.into_iter().enumerate().map(|(k, e)| NavWaypoint { padnum: k, neighbours: e, groupnum: 0 }).collect();
        let waygroups = vec![NavWaygroup { neighbours: Vec::new(), waypoints: (0..pos.len()).collect() }];
        NavGraph::new(pads, waypoints, waygroups)
    }

    pub fn waypoint_pos(&self, w: usize) -> Vec3 {
        self.pads[self.waypoints[w].padnum].pos
    }

    pub fn waypoint_room(&self, w: usize) -> Option<u16> {
        self.pads[self.waypoints[w].padnum].room
    }

    pub fn waypoint_flags(&self, w: usize) -> PadFlags {
        self.pads[self.waypoints[w].padnum].flags
    }

    // ─── Candidate search ────────────────────────────────────────────────────

    /// `waypoint_find_closest_to_pos` (`padhalllv.c:74`): the ten nearest waypoints
    /// in `rooms` and their neighbouring rooms, nearest first; the first with no
    /// floor in the way (`cd_test_los_oobfail`, floors only) and a clear line to
    /// its pad (`cd_test_cylmove_oobfail_findclosest`, zero height) wins. Failing
    /// that, the first whose blocking edge can be stepped round; failing that, the
    /// nearest.
    pub fn waypoint_find_closest_to_pos(&self, level: &TileLevel, pos: Vec3, rooms: &[u16]) -> Option<usize> {
        let mut allrooms: Vec<u16> = rooms.to_vec();
        for &r in rooms {
            for n in level.room_neighbours(r) {
                if !allrooms.contains(&n) {
                    allrooms.push(n);
                }
            }
        }
        // Candidates sorted by distance, at most 10 (insertion as PD does it).
        let mut cands: Vec<(usize, f32)> = Vec::new();
        for r in &allrooms {
            for &w in self.room_waypoints.get(r).map_or(&[][..], |v| v.as_slice()) {
                let sqdist = pos.distance_squared(self.waypoint_pos(w));
                let index = cands.iter().position(|&(_, d)| sqdist < d).unwrap_or(cands.len());
                if index < 10 {
                    cands.insert(index, (w, sqdist));
                    cands.truncate(10);
                }
            }
        }
        let mut checkmore: Vec<Option<(Vec3, Vec3)>> = vec![None; cands.len()];
        for (i, &(w, _)) in cands.iter().enumerate() {
            let padpos = self.waypoint_pos(w);
            if !level.los_floors(pos, padpos) {
                continue;
            }
            let (r, edge) = level.cd_test_cylmove_oobfail_findclosest(pos, padpos, 20.0, 0.0, 0.0, &[]);
            match r {
                CdResult::Error => {}
                CdResult::Collision => checkmore[i] = edge,
                CdResult::NoCollision => return Some(w),
            }
        }
        // No line of sight to any: step round the first blocking edge that allows it.
        for (i, &(w, _)) in cands.iter().enumerate() {
            let Some((a, b)) = checkmore[i] else { continue };
            if a.x == b.x && a.z == b.z {
                continue;
            }
            let d = Vec3::new(a.x - b.x, 0.0, a.z - b.z);
            let d = d * (10.0 / (d.x * d.x + d.z * d.z).sqrt());
            for tmppos in [Vec3::new(a.x + d.x, pos.y, a.z + d.z), Vec3::new(b.x - d.x, pos.y, b.z - d.z)] {
                if level.cd_test_cylmove_oobok(pos, tmppos, 0.0, 0.0, &[]) != CdResult::Collision {
                    return Some(w);
                }
            }
        }
        cands.first().map(|c| c.0)
    }

    // ─── Tie-breaks ──────────────────────────────────────────────────────────

    /// The 50% "keep looking" coin both `*_choose_neighbour` functions and
    /// `waypoint_find_segment_into_group` flip. With a nav seed set, PD rotates a
    /// *copy* of the seed each time, so every flip in one routing call lands the
    /// same way: all take the first match, or all the last.
    fn stop_here(seed: NavSeed, rng: &mut Rng) -> bool {
        if seed.0 == 0 && seed.1 == 0 {
            rng.random().is_multiple_of(2)
        } else {
            let mut s = ((seed.0 as u64) << 32) | seed.1 as u64;
            Rng::rotate_seed(&mut s).is_multiple_of(2)
        }
    }

    // ─── Group level ─────────────────────────────────────────────────────────

    /// `waygroup_choose_neighbour` (`padhalllv.c:251`).
    fn waygroup_choose_neighbour(&self, st: &mut Steps, groupnums: &[i32], step: i32, ignoremask: i32, seed: NavSeed, rng: &mut Rng) -> Option<usize> {
        let mut best = None;
        for &g in groupnums {
            if g & ignoremask == 0 {
                let group = wpseg_get_id(g);
                if st.group[group] == step {
                    best = Some(group);
                    if Self::stop_here(seed, rng) {
                        break;
                    }
                }
            }
        }
        best
    }

    /// `waygroup_set_step_if_undiscovered` (`padhalllv.c:286`).
    fn waygroup_set_step_if_undiscovered(&self, st: &mut Steps, groupnums: &[i32], step: i32, ignoremask: i32) {
        for &g in groupnums {
            if g & ignoremask == 0 {
                let s = &mut st.group[wpseg_get_id(g)];
                if *s < 0 {
                    *s = step;
                }
            }
        }
    }

    /// `waygroup_discover_one_step` (`padhalllv.c:306`): one pass over every group.
    fn waygroup_discover_one_step(&self, st: &mut Steps, step: i32, ignoremask: i32) -> bool {
        let mut discovered = false;
        for (g, group) in self.waygroups.iter().enumerate() {
            if st.group[g] == step {
                discovered = true;
                self.waygroup_set_step_if_undiscovered(st, &group.neighbours, step + 1, ignoremask);
            }
        }
        discovered
    }

    /// `waygroup_discover_steps` (`padhalllv.c:333`).
    fn waygroup_discover_steps(&self, st: &mut Steps, from: usize, to: usize, discoverall: bool, ignoremask: i32) -> bool {
        for s in st.group.iter_mut() {
            *s = -1;
        }
        st.group[from] = 0;
        let mut result = true;
        let mut step = 0;
        while (discoverall || st.group[to] < 0) && result {
            result = self.waygroup_discover_one_step(st, step, ignoremask);
            step += 1;
        }
        result
    }

    /// `waygroup_find_route` (`padhalllv.c:358`): marks the chosen group route with
    /// steps ≥ 10000.
    fn waygroup_find_route(&self, st: &mut Steps, from: usize, to: usize, seed: NavSeed, rng: &mut Rng) -> bool {
        let result = self.waygroup_discover_steps(st, from, to, false, IGNORE_INWARDS);
        if result {
            let mut curto = to;
            let mut step = st.group[curto] - 1;
            while step >= 0 {
                st.group[curto] += 10000;
                match self.waygroup_choose_neighbour(st, &self.waygroups[curto].neighbours, step, IGNORE_OUTWARDS, seed, rng) {
                    Some(g) => curto = g,
                    None => return result,
                }
                step -= 1;
            }
            st.group[curto] += 10000;
        }
        result
    }

    // ─── Waypoint level ──────────────────────────────────────────────────────

    /// `waypoint_choose_neighbour` (`padhalllv.c:397`).
    fn waypoint_choose_neighbour(&self, st: &mut Steps, pointnums: &[i32], step: i32, groupnum: usize, ignoremask: i32, seed: NavSeed, rng: &mut Rng) -> Option<usize> {
        let mut best = None;
        for &p in pointnums {
            if p & ignoremask == 0 {
                let point = wpseg_get_id(p);
                if self.waypoints[point].groupnum == groupnum && st.wp[point] == step {
                    best = Some(point);
                    if Self::stop_here(seed, rng) {
                        break;
                    }
                }
            }
        }
        best
    }

    /// `waypoint_set_step_if_undiscovered` (`padhalllv.c:434`).
    fn waypoint_set_step_if_undiscovered(&self, st: &mut Steps, pointnums: &[i32], value: i32, groupnum: usize, ignoremask: i32) {
        for &p in pointnums {
            if p & ignoremask == 0 {
                let point = wpseg_get_id(p);
                if self.waypoints[point].groupnum == groupnum && st.wp[point] < 0 {
                    st.wp[point] = value;
                }
            }
        }
    }

    /// `waypoint_discover_one_step` (`padhalllv.c:455`): one pass over the group.
    fn waypoint_discover_one_step(&self, st: &mut Steps, groupnum: usize, step: i32, ignoremask: i32) -> bool {
        let mut result = false;
        for &p in &self.waygroups[groupnum].waypoints {
            if st.wp[p] == step {
                result = true;
                self.waypoint_set_step_if_undiscovered(st, &self.waypoints[p].neighbours, step + 1, groupnum, ignoremask);
            }
        }
        result
    }

    /// `waypoint_discover_steps` (`padhalllv.c:486`): `from` and `to` must share a group.
    fn waypoint_discover_steps(&self, st: &mut Steps, from: usize, to: usize, discoverall: bool, ignoremask: i32) {
        let groupnum = self.waypoints[from].groupnum;
        for &p in &self.waygroups[groupnum].waypoints {
            st.wp[p] = -1;
        }
        st.wp[from] = 0;
        let mut more = true;
        let mut i = 0;
        while (discoverall || st.wp[to] < 0) && more {
            more = self.waypoint_discover_one_step(st, groupnum, i, ignoremask);
            i += 1;
        }
    }

    /// `waypoint_find_route` (`padhalllv.c:514`): marks the route with steps ≥ 10000.
    fn waypoint_find_route(&self, st: &mut Steps, from: usize, to: usize, seed: NavSeed, rng: &mut Rng) {
        self.waypoint_discover_steps(st, from, to, false, IGNORE_INWARDS);
        let groupnum = self.waypoints[from].groupnum;
        let mut value = st.wp[to] - 1;
        let mut curto = to;
        while value >= 0 {
            st.wp[curto] += 10000;
            match self.waypoint_choose_neighbour(st, &self.waypoints[curto].neighbours, value, groupnum, IGNORE_OUTWARDS, seed, rng) {
                Some(p) => curto = p,
                None => return,
            }
            value -= 1;
        }
        st.wp[curto] += 10000;
    }

    /// `waypoint_collect_local` (`padhalllv.c:540`): append the in-group route
    /// `from → to` (at most `arrlen - 1` waypoints) to `arr`. Returns the count PD
    /// returns, which includes the NULL terminator.
    fn waypoint_collect_local(&self, st: &mut Steps, from: usize, to: usize, arr: &mut Vec<usize>, arrlen: i32, seed: NavSeed, rng: &mut Rng) -> i32 {
        let before = arr.len();
        if arrlen >= 2 {
            self.waypoint_find_route(st, from, to, seed, rng);
            arr.push(from);
            let groupnum = self.waypoints[from].groupnum;
            let mut curfrom = from;
            let limit = arrlen + 9999;
            let mut step = 10001;
            while step <= st.wp[to] && step < limit {
                match self.waypoint_choose_neighbour(st, &self.waypoints[curfrom].neighbours, step, groupnum, IGNORE_INWARDS, seed, rng) {
                    Some(p) => {
                        curfrom = p;
                        arr.push(p);
                    }
                    None => break,
                }
                step += 1;
            }
        }
        (arr.len() - before) as i32 + 1
    }

    /// `waypoint_find_segment_into_group` (`padhalllv.c:574`): a link from a
    /// waypoint of `fromgroup` into `togroup`, the last one found unless the coin
    /// stops the search earlier.
    fn waypoint_find_segment_into_group(&self, fromgroup: usize, togroup: usize, seed: NavSeed, rng: &mut Rng) -> Option<(usize, usize)> {
        let mut found = None;
        'outer: for &fromwp in &self.waygroups[fromgroup].waypoints {
            for &n in &self.waypoints[fromwp].neighbours {
                if n & IGNORE_INWARDS == 0 {
                    let neighbour = wpseg_get_id(n);
                    if self.waypoints[neighbour].groupnum == togroup {
                        found = Some((fromwp, neighbour));
                        if Self::stop_here(seed, rng) {
                            // PD's `break` leaves only the inner loop.
                            continue 'outer;
                        }
                    }
                }
            }
        }
        found
    }

    /// `nav_find_route` (`padhalllv.c:630`): the route `frompoint → topoint`, at
    /// most `arrlen - 1` waypoints of it. Returns the waypoints and PD's count,
    /// which **includes the NULL terminator**, so PD's `numwaypoints > 1` means
    /// "at least one waypoint".
    pub fn nav_find_route(&self, frompoint: usize, topoint: usize, arrlen: usize, seed: NavSeed, rng: &mut Rng) -> (Vec<usize>, i32) {
        let mut arr = Vec::new();
        if self.waygroups.is_empty() {
            return (arr, 1);
        }
        let mut guard = self.steps.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        let mut arrlen = arrlen as i32;
        let fromgroup = self.waypoints[frompoint].groupnum;
        let togroup = self.waypoints[topoint].groupnum;
        if self.waygroup_find_route(st, fromgroup, togroup, seed, rng) {
            let mut curfrompoint = frompoint;
            let mut curfromgroup = fromgroup;
            let mut step = st.group[fromgroup] + 1;
            while step <= st.group[togroup] && arrlen >= 2 {
                let Some(nextfromgroup) = self.waygroup_choose_neighbour(st, &self.waygroups[curfromgroup].neighbours, step, IGNORE_INWARDS, seed, rng) else {
                    break;
                };
                let Some((lastwp, nextfirstwp)) = self.waypoint_find_segment_into_group(curfromgroup, nextfromgroup, seed, rng) else {
                    break;
                };
                let numwritten = self.waypoint_collect_local(st, curfrompoint, lastwp, &mut arr, arrlen, seed, rng) - 1;
                arrlen -= numwritten;
                curfrompoint = nextfirstwp;
                curfromgroup = nextfromgroup;
                step += 1;
            }
            self.waypoint_collect_local(st, curfrompoint, topoint, &mut arr, arrlen, seed, rng);
        }
        let count = arr.len() as i32 + 1;
        (arr, count)
    }

    /// Every directed link a chr may walk ([`crate::stage::directed_links`]).
    pub fn links(&self) -> Vec<(usize, usize, bool)> {
        crate::stage::directed_links(self.waypoints.len(), |w| &self.waypoints[w].neighbours)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring of 6 waypoints in two groups (0-2 | 3-5), linked 0-1-2-3-4-5-0.
    fn ring() -> NavGraph {
        let pads = (0..6)
            .map(|k| {
                let a = k as f32 * std::f32::consts::TAU / 6.0;
                NavPad { pos: Vec3::new(a.sin() * 500.0, 50.0, a.cos() * 500.0), room: Some(0), flags: PadFlags::default() }
            })
            .collect();
        let waypoints = (0..6).map(|k: usize| NavWaypoint { padnum: k, neighbours: vec![((k + 5) % 6) as i32, ((k + 1) % 6) as i32], groupnum: k / 3 }).collect();
        let waygroups = vec![NavWaygroup { neighbours: vec![1], waypoints: vec![0, 1, 2] }, NavWaygroup { neighbours: vec![0], waypoints: vec![3, 4, 5] }];
        NavGraph::new(pads, waypoints, waygroups)
    }

    #[test]
    fn routes_go_group_by_group_and_count_the_terminator() {
        let g = ring();
        let mut rng = Rng::new(1);
        let (r, n) = g.nav_find_route(1, 4, MAX_CHRWAYPOINTS, NavSeed(7, 7), &mut rng);
        assert_eq!(n as usize, r.len() + 1);
        assert_eq!(r.first(), Some(&1));
        assert_eq!(r.last(), Some(&4));
        for w in r.windows(2) {
            assert!(g.waypoints[w[0]].neighbours.contains(&(w[1] as i32)), "{r:?}");
        }
        // Same waypoint: just itself.
        assert_eq!(g.nav_find_route(2, 2, MAX_CHRWAYPOINTS, NavSeed(7, 7), &mut rng).0, vec![2]);
        // A short array keeps only the first `arrlen - 1` waypoints.
        let (short, n) = g.nav_find_route(0, 4, 3, NavSeed(7, 7), &mut rng);
        assert_eq!((short.len(), n), (2, 3));
    }

    #[test]
    fn a_one_way_link_is_only_used_in_its_direction() {
        let mut g = ring();
        // 2 → 3 outwards only (a ledge): 3's entry for 2 is inwards only.
        g.waypoints[2].neighbours = vec![1, 3 | WPSEGFLAG_OUTWARDSONLY];
        g.waypoints[3].neighbours = vec![2 | WPSEGFLAG_INWARDSONLY, 4];
        let g = NavGraph::new(g.pads, g.waypoints, g.waygroups);
        let mut rng = Rng::new(1);
        let (r, _) = g.nav_find_route(3, 2, 8, NavSeed(3, 3), &mut rng);
        assert!(!r.windows(2).any(|w| w == [3, 2]), "{r:?}");
    }

    #[test]
    fn the_arena_grid_routes_around_the_pillars() {
        let level = TileLevel::new(crate::stage::fixtures::arena());
        let g = NavGraph::grid(&level, 250.0);
        assert!(g.waypoints.len() > 20, "{} points", g.waypoints.len());
        let at = |x: f32, z: f32| {
            let p = Vec3::new(x, 50.0, z);
            let rooms: Vec<u16> = level.floor_room(p, 20.0).into_iter().collect();
            g.waypoint_find_closest_to_pos(&level, p, &rooms).unwrap()
        };
        let (a, b) = (at(-650.0, -650.0), at(650.0, 650.0));
        let mut rng = Rng::new(1);
        let (r, _) = g.nav_find_route(a, b, 100, NavSeed(1, 1), &mut rng);
        assert!(r.len() > 2 && r.last() == Some(&b), "{r:?}");
    }

    /// PD's own graph on Complex: the documented counts (SPIKE_PD_COMPLEX.md).
    #[test]
    fn complexs_graph_loads_with_its_rooms_and_flags() {
        let (stage, level) = crate::testutil::complex();
        let g = NavGraph::from_stage(stage, level);
        assert_eq!((g.waypoints.len(), g.waygroups.len()), (144, 20));
        assert_eq!(g.links().len(), 401);
        let roomless: Vec<usize> = (0..g.waypoints.len()).filter(|&w| g.waypoint_room(w).is_none()).collect();
        assert!(roomless.is_empty(), "waypoints with no room: {roomless:x?}");
        assert!((0..g.waypoints.len()).any(|w| g.waypoint_flags(w).crouch), "the crawl space's crouch pads");
    }
}
