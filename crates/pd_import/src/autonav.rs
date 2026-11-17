//! Simulants on a generated level: what the generator knows of its own
//! navigation, given to the waypoint graph.
//!
//! The waypoints of a custom level are made from its collision alone
//! ([`pd_sim::nav::gen`]: floors sampled on a 50 cm lattice, neighbours
//! joined by a probe that walks as a chr does). On a generated level that
//! leaves ways out which the generator knows are there, because it made them
//! and walks them (`Level::walks`: every corridor, every room door to door,
//! every spiral, every vent and drop; and the plan's ramps):
//!
//! - **the foot of an open ramp.** The floor runs on under a ramp, and where
//!   the ramp is less than 30 cm over it the sampler takes the two for one
//!   surface, tries the lower, and finds a chr there stands on the ramp: no
//!   sample. The ramp's first lattice row is then missing, the rows either
//!   side are 100 cm apart, and nothing joins them: a ramp climbed from a
//!   floor is cut off at its foot unless the lattice happens to fall within
//!   3 cm of it (seed 40: the ramp out of the Spire's pit, with the low road
//!   behind it, and the Cistern's gallery).
//! - **a drop through a hole.** A lattice link falls only if the edge comes
//!   in the first third of its 50 cm, so a hole is found by luck.
//! - **vents.** No chr fits a 118 cm duct standing; PD's designers flag the
//!   floor of a crawl space `GEOFLAG_AIBOTCROUCH` (Complex's), which
//!   [`crate::autolevel`] now does for every vent, and the maker then samples
//!   and joins them itself.
//!
//! So the generic graph is kept (it knows where a chr fits and how rooms'
//! floors are best covered) and **repaired**: every leg of every route the
//! generator walks, and every ramp from before its foot to past its top, is
//! tried with the maker's own probe ([`pd_sim::nav::gen::probe_walk`]: what a
//! chr can do, one-way where it drops), and where the graph does not already
//! join the leg's ends, waypoints are put at them, linked to each other and
//! to the graph's waypoints they can walk to and from. The graph is then
//! rebuilt in PD's format as a hand-edited one is
//! ([`pd_sim::nav::gen::graph_from_links`]).
//!
//! What stays outside the largest part after that is where nobody can stand
//! and get back from: the water of a railed pool, the top of a tank, a pod's
//! roof no deck reaches.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use autolevel_build::built::Built;
use autolevel_build::walk;
use glam::Vec3;
use pd_sim::nav::gen::{graph_from_links, probe_walk, GenParams, LinkKind};
use pd_sim::nav::{NavGraph, NavPad, PadFlags};
use pd_sim::stage::{directed_links, TileFlag, TileLevel};

use crate::autolevel::pd_point;

/// A pad stands this far over its floor (`nav::gen`'s, as PD places them).
const PAD_HEIGHT: f32 = 53.0;
/// A leg's end is tied to the graph's waypoints within this far of it (cm,
/// over the floor), at most this many, and on its own level.
const REACH: f32 = 300.0;
const TIES: usize = 2;
const LEVEL: f32 = 40.0;
/// An existing waypoint this close to a leg's end stands for it.
const SAME: f32 = 30.0;
/// A leg is joined by the graph if its ends' waypoints are within this many
/// times its length of each other along the links (and the ties' reach).
const DETOUR: f32 = 3.0;
/// A ramp's leg starts this far before its foot and ends this far past its
/// top (half a cell: the middle of the landing).
const GATE: f32 = 59.0;

/// A way the generator knows: a straight walk from one floor point to
/// another.
#[derive(Clone, Debug)]
pub struct Leg {
    pub a: Vec3,
    pub b: Vec3,
    /// The route or ramp it is of.
    pub what: String,
}

/// Every way of the level `b`, in PD's coordinates: each ramp of the plan end
/// to end, then each leg of each route (those named `!x`, which must fail,
/// left out). A leg met twice (a route and its way back) is given once.
pub fn legs(b: &Built, scale: f32) -> Vec<Leg> {
    let mut out: Vec<Leg> = Vec::new();
    let mut push = |a: Vec3, b: Vec3, what: String| {
        let same = |p: Vec3, q: Vec3| p.distance(q) < 1.0;
        if a.distance(b) > 20.0 && !out.iter().any(|l| (same(l.a, a) && same(l.b, b)) || (same(l.a, b) && same(l.b, a))) {
            out.push(Leg { a, b, what });
        }
    };
    for room in &b.level.plan.rooms {
        for r in &room.ramps {
            let (s, e) = (pd_point(r.start, scale), pd_point(r.end, scale));
            let along = (e - s).with_y(0.0).normalize_or_zero();
            push(s - along * GATE * scale, e + along * GATE * scale, format!("{} ramp in {}", r.kind, room.key));
        }
    }
    for (name, steps) in &b.level.walks {
        if walk::must_fail(name) {
            continue;
        }
        let pts: Vec<Vec3> = steps
            .iter()
            .map(|s| {
                let (x, y, z) = walk::ge(s.x, s.y, s.z as f64);
                Vec3::new(x.round() as f32, y as f32, z.round() as f32) * scale
            })
            .collect();
        for w in pts.windows(2) {
            push(w[0], w[1], name.clone());
        }
    }
    out
}

/// What [`repair`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Repaired {
    /// The generator's ways tried.
    pub legs: usize,
    /// Those the graph already joined.
    pub joined: usize,
    /// Those it did not, now linked: (what, one way).
    pub linked: Vec<(String, bool)>,
    /// Those no chr walks straight (by the maker's probe) that the graph does
    /// not join either: left alone, and worth a look.
    pub unwalked: Vec<String>,
    /// Waypoints added.
    pub added: usize,
    /// Waypoints left out: outside the largest part once the ways are in.
    pub outside: usize,
}

impl Repaired {
    /// A line for the report.
    pub fn line(&self) -> String {
        let mut kinds: Vec<(String, usize)> = Vec::new();
        for (what, one_way) in &self.linked {
            // "open ramp in basin" is an open ramp; "drop_colossal_small7" a drop; "link_c07_..." a link
            let kind = if what.contains(" ramp in ") { what.split(" in ").next().unwrap_or(what).to_string() } else { what.split('_').next().unwrap_or(what).to_string() };
            let kind = if *one_way { format!("{kind} (one way)") } else { kind };
            match kinds.iter_mut().find(|k| k.0 == kind) {
                Some(k) => k.1 += 1,
                None => kinds.push((kind, 1)),
            }
        }
        let which = if kinds.is_empty() { String::new() } else { format!(" ({})", kinds.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ")) };
        let unwalked = if self.unwalked.is_empty() { String::new() } else { format!(" ({})", self.unwalked.join(", ")) };
        format!(
            "the generator's ways: {} legs of its routes and ramps, {} joined by the waypoints as made, {} linked with {} waypoints added{which}, {} neither walked straight by a chr nor joined{unwalked}; {} waypoints left out where nobody gets back from (a pool, a tank's top, a pod's roof)",
            self.legs,
            self.joined,
            self.linked.len(),
            self.added,
            self.unwalked.len(),
            self.outside
        )
    }
}

/// The graph as it is worked on: floor points and directed links.
struct Work<'a> {
    level: &'a TileLevel,
    p: GenParams,
    pads: Vec<NavPad>,
    fwd: Vec<Vec<usize>>,
}

impl Work<'_> {
    fn floor(&self, w: usize) -> Vec3 {
        self.pads[w].pos - Vec3::Y * PAD_HEIGHT
    }

    /// Is `to` reached from `from` along the links, within `limit` cm of
    /// walking? (Dijkstra's, given up past the limit.)
    fn reaches(&self, from: usize, to: usize, limit: f32) -> bool {
        if from == to {
            return true;
        }
        let mut best = vec![f32::INFINITY; self.pads.len()];
        let mut heap = BinaryHeap::from([(Reverse(0u32), from)]);
        best[from] = 0.0;
        while let Some((Reverse(d), u)) = heap.pop() {
            let d = d as f32;
            if u == to {
                return true;
            }
            if d > best[u] + 1.0 {
                continue;
            }
            for &v in &self.fwd[u] {
                let nd = d + self.pads[u].pos.distance(self.pads[v].pos);
                if nd <= limit && nd < best[v] {
                    best[v] = nd;
                    heap.push((Reverse(nd as u32), v));
                }
            }
        }
        false
    }

    /// The waypoints a chr at floor point `p` walks to and back from
    /// straight, the nearest first, at most [`TIES`].
    fn ties(&self, p: Vec3) -> Vec<usize> {
        let mut near: Vec<(f32, usize)> = (0..self.pads.len())
            .map(|w| (self.floor(w), w))
            .filter(|(f, _)| (f.y - p.y).abs() <= LEVEL)
            .map(|(f, w)| ((f - p).with_y(0.0).length(), w))
            .filter(|&(d, _)| d <= REACH)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let walk = |a: Vec3, b: Vec3| a.distance(b) < 1.0 || probe_walk(self.level, a, b, &self.p) == Some(LinkKind::Walk);
        let mut out = Vec::new();
        for (_, w) in near {
            let f = self.floor(w);
            if walk(p, f) && walk(f, p) {
                out.push(w);
                if out.len() == TIES {
                    break;
                }
            }
        }
        out
    }

    fn link(&mut self, a: usize, b: usize) {
        if a != b && !self.fwd[a].contains(&b) {
            self.fwd[a].push(b);
        }
    }

    /// The waypoint at floor point `p`: one already there, or a new one tied
    /// to the graph round it. Never skipped past on a go-to
    /// (`PADFLAG_AIWALKDIRECT`): the ways put right here are the ones a
    /// straight line past them gets wrong.
    fn at(&mut self, p: Vec3, ties: &[usize], added: &mut usize) -> usize {
        if let Some(&w) = ties.first().filter(|&&w| (self.floor(w) - p).length() <= SAME) {
            return w;
        }
        let crouch = self.level.is_cyl_touching_tile_with_flags(TileFlag::Crouch, p + Vec3::Y * 50.0, self.p.radius * 1.1, 135.0 - 50.0, -10.0 - 50.0);
        let w = self.pads.len();
        self.pads.push(NavPad { pos: p + Vec3::Y * PAD_HEIGHT, room: self.level.floor_room(p, self.p.radius), flags: PadFlags { walkdirect: true, crouch, ..Default::default() } });
        self.fwd.push(Vec::new());
        *added += 1;
        for &t in ties {
            self.link(w, t);
            self.link(t, w);
        }
        w
    }
}

/// `graph` with the generator's `legs` in it: each leg a chr can walk
/// (either way, or one way where it drops) whose ends the graph does not
/// already join gets a waypoint at each end, linked to each other and tied to
/// the graph round them. Then the waypoints outside the largest strongly
/// connected part are left out: with every way the generator knows in the
/// graph, they are where nobody can stand and get back from (a pool's water,
/// a tank's top, a pod's roof no deck reaches), and a simulant sent to the
/// waypoint nearest a player would otherwise be sent to one of them.
/// Returns the graph and what was done.
pub fn repair(graph: &NavGraph, level: &TileLevel, legs: &[Leg]) -> (NavGraph, Repaired) {
    let n = graph.waypoints.len();
    let mut w = Work { level, p: GenParams::default(), pads: graph.waypoints.iter().map(|w| graph.pads[w.padnum].clone()).collect(), fwd: vec![Vec::new(); n] };
    for (a, b, _) in directed_links(n, |k| &graph.waypoints[k].neighbours) {
        w.link(a, b);
    }
    let mut done = Repaired { legs: legs.len(), ..Default::default() };
    for leg in legs {
        // The ends on their floors, as the probe wants them.
        let settle = |p: Vec3| {
            let (y, poly) = level.cd_find_ground_at_cyl(p + Vec3::Y * w.p.step, w.p.radius);
            (poly.is_some() && (y - p.y).abs() <= 30.0).then_some(Vec3::new(p.x, y, p.z))
        };
        let (Some(a), Some(b)) = (settle(leg.a), settle(leg.b)) else {
            done.unwalked.push(leg.what.clone());
            continue;
        };
        // What a chr can do of it straight (a player's walk slides round a
        // corner a chr's line would clip: such a leg is only asked after).
        let (there, back) = (probe_walk(level, a, b, &w.p).is_some(), probe_walk(level, b, a, &w.p).is_some());
        let (ta, tb) = (w.ties(a), w.ties(b));
        // Joined: one end's waypoints reach the other's without going far
        // round (a drop the graph only reaches by the stairs is not joined).
        let limit = DETOUR * a.distance(b) + 2.0 * REACH;
        let joins = |from: &[usize], to: &[usize]| from.iter().any(|&x| to.iter().any(|&y| w.reaches(x, y, limit)));
        if !there && !back {
            if joins(&ta, &tb) || joins(&tb, &ta) {
                done.joined += 1;
            } else {
                done.unwalked.push(leg.what.clone());
            }
            continue;
        }
        if (!there || joins(&ta, &tb)) && (!back || joins(&tb, &ta)) {
            done.joined += 1;
            continue;
        }
        let (wa, wb) = (w.at(a, &ta, &mut done.added), w.at(b, &tb, &mut done.added));
        if there {
            w.link(wa, wb);
        }
        if back {
            w.link(wb, wa);
        }
        done.linked.push((leg.what.clone(), !(there && back)));
    }
    // Only the largest part is kept.
    let whole = graph_from_links(w.pads.clone(), &w.fwd, w.p.group_radius);
    let part = crate::place::waypoint_parts(&whole);
    done.outside = part.iter().filter(|&&p| p != 0).count();
    if done.outside == 0 {
        return (whole, done);
    }
    let mut index = vec![usize::MAX; part.len()];
    let mut pads = Vec::new();
    for (i, pad) in w.pads.iter().enumerate().filter(|(i, _)| part[*i] == 0) {
        index[i] = pads.len();
        pads.push(pad.clone());
    }
    let fwd: Vec<Vec<usize>> = (0..part.len()).filter(|&i| part[i] == 0).map(|i| w.fwd[i].iter().filter(|&&j| part[j] == 0).map(|&j| index[j]).collect()).collect();
    (graph_from_links(pads, &fwd, w.p.group_radius), done)
}

#[cfg(test)]
mod tests;
