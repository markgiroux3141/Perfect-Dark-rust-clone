//! The static checks of SPIKE_PD_COMPLEX.md (S1–S3), measured the same way on
//! any two graphs in PD's format. S4 (walk every link with the chr movement
//! code) needs a world: `crate::harness::walk::s4_walk`.
//!
//! Source: the old repo's `pd_spike/navcheck.rs` and `walk.rs` (`pd_pad_floor`).

use glam::{Vec2, Vec3};
use pd_core::rng::Rng;

use super::{NavGraph, NavSeed};
use crate::stage::{wpseg_get_id, FloorKind, GeomPoly, Stage, TileLevel, WPSEGFLAG_INWARDSONLY, WPSEGFLAG_OUTWARDSONLY};

/// The floor a PD pad belongs to. PD places pads 52–121 cm above their floor
/// (measured over Complex's 144 waypoint and 19 spawn pads), and some pads sit on
/// a railing line or a walkway edge, just off their tile. So: the highest floor
/// polygon within 60 cm (XZ) of the pad whose height there is 40–130 cm below it.
/// Returns `(height, polygon)`.
pub fn pd_pad_floor(level: &TileLevel, pad: Vec3) -> Option<(f32, usize)> {
    let mut best: Option<(f32, usize)> = None;
    for (i, p) in level.geom.polys.iter().enumerate() {
        if !matches!(p.floor_kind(), Some(FloorKind::Flat | FloorKind::Ramp)) {
            continue;
        }
        let q = nearest_point_xz(p, Vec2::new(pad.x, pad.z));
        if q.distance(Vec2::new(pad.x, pad.z)) > 60.0 {
            continue;
        }
        let y = p.find_y(q.x, q.y);
        let above = pad.y - y;
        if (40.0..=130.0).contains(&above) && best.is_none_or(|(by, _)| y > by) {
            best = Some((y, i));
        }
    }
    best
}

fn nearest_point_xz(p: &GeomPoly, pt: Vec2) -> Vec2 {
    if p.xz_in_convex(pt.x, pt.y) {
        return pt;
    }
    let v = &p.verts;
    let mut best = Vec2::new(v[0].x, v[0].z);
    for i in 0..v.len() {
        let (a, b) = (Vec2::new(v[i].x, v[i].z), Vec2::new(v[(i + 1) % v.len()].x, v[(i + 1) % v.len()].z));
        let ab = b - a;
        let t = if ab.length_squared() > 0.0 { ((pt - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        let c = a + ab * t;
        if c.distance(pt) < best.distance(pt) {
            best = c;
        }
    }
    best
}

/// S1: a PD pad (waypoint or spawn) not covered by the graph: no node within
/// 150 cm (XZ) on the same floor (±40 cm) that the pad can see.
#[derive(Clone, Debug)]
pub struct Miss {
    pub pad: usize,
    pub pos: Vec3,
    /// XZ distance to the nearest same-floor node, seen or not.
    pub nearest: f32,
}

/// The floor a PD pad belongs to (see [`pd_pad_floor`]), else the one under it.
pub fn pad_floor(level: &TileLevel, pad: Vec3) -> f32 {
    pd_pad_floor(level, pad).map_or_else(|| level.drop_to_ground(pad).y, |f| f.0)
}

pub fn s1_coverage(level: &TileLevel, stage: &Stage, graph: &NavGraph) -> (Vec<Miss>, usize) {
    let mut pads: Vec<usize> = stage.waypoints.iter().map(|w| w.padnum).collect();
    pads.extend(stage.spawn_pads.iter().copied());
    let mut misses = Vec::new();
    for &p in &pads {
        let pos = stage.pads[p].pos;
        let floor = pad_floor(level, pos);
        let mut nearest = f32::INFINITY;
        let mut ok = false;
        for w in 0..graph.waypoints.len() {
            let n = graph.waypoint_pos(w);
            // Each node's own floor, found the same way for either graph.
            let nfloor = pad_floor(level, n);
            if (nfloor - floor).abs() > 40.0 {
                continue;
            }
            let d = Vec2::new(n.x - pos.x, n.z - pos.z).length();
            nearest = nearest.min(d);
            if d <= 150.0 && level.los(Vec3::new(pos.x, floor + 53.0, pos.z), Vec3::new(n.x, nfloor + 53.0, n.z)) {
                ok = true;
                break;
            }
        }
        if !ok {
            misses.push(Miss { pad: p, pos, nearest });
        }
    }
    (misses, pads.len())
}

/// S2: (weakly connected, strongly connected) component counts, links taken in
/// the directions PD's routing allows.
pub fn s2_components(graph: &NavGraph) -> (usize, usize) {
    let (w, comps) = s2_strong_components(graph);
    (w, comps.len())
}

/// S2 in detail: the weak count, and every strongly connected component (largest first).
pub fn s2_strong_components(graph: &NavGraph) -> (usize, Vec<Vec<usize>>) {
    let n = graph.waypoints.len();
    let fwd: Vec<Vec<usize>> = (0..n)
        .map(|a| {
            graph.waypoints[a]
                .neighbours
                .iter()
                .filter(|&&s| s & WPSEGFLAG_INWARDSONLY == 0)
                .map(|&s| wpseg_get_id(s))
                .filter(|&b| graph.waypoints[b].neighbours.iter().all(|&t| wpseg_get_id(t) != a || t & WPSEGFLAG_OUTWARDSONLY == 0))
                .collect()
        })
        .collect();
    // Weak: union-find.
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let next = p[c];
            p[c] = r;
            c = next;
        }
        r
    }
    for a in 0..n {
        for &b in &fwd[a] {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let weak = (0..n).filter(|&x| find(&mut parent, x) == x).count();
    // Strong: count distinct reach sets by forward+backward search (n is small).
    let reach = |start: usize, rev: bool| {
        let mut seen = vec![false; n];
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(u) = stack.pop() {
            let next: Vec<usize> = if rev { (0..n).filter(|&v| fwd[v].contains(&u)).collect() } else { fwd[u].clone() };
            for v in next {
                if !seen[v] {
                    seen[v] = true;
                    stack.push(v);
                }
            }
        }
        seen
    };
    let mut comp = vec![usize::MAX; n];
    let mut comps: Vec<Vec<usize>> = Vec::new();
    for s in 0..n {
        if comp[s] != usize::MAX {
            continue;
        }
        let (f, b) = (reach(s, false), reach(s, true));
        let members: Vec<usize> = (0..n).filter(|&v| f[v] && b[v]).collect();
        for &v in &members {
            comp[v] = comps.len();
        }
        comps.push(members);
    }
    comps.sort_by_key(|c| std::cmp::Reverse(c.len()));
    (weak, comps)
}

/// S3: for every ordered pair of spawn pads, the length (cm, along the floor
/// points) of the route `nav_find_route` gives, from spawn to spawn. `None` when
/// there is no route.
pub fn s3_route_lengths(level: &TileLevel, stage: &Stage, graph: &NavGraph) -> Vec<((usize, usize), Option<f32>)> {
    let spawns: Vec<Vec3> = stage.spawn_pads.iter().map(|&p| level.drop_to_ground(stage.pads[p].pos)).collect();
    let mut rng = Rng::new(1);
    let mut out = Vec::new();
    for (i, &a) in spawns.iter().enumerate() {
        for (j, &b) in spawns.iter().enumerate() {
            if i == j {
                continue;
            }
            let rooms = |p: Vec3| level.floor_room(p, 20.0).into_iter().collect::<Vec<_>>();
            let from = graph.waypoint_find_closest_to_pos(level, a + Vec3::Y * 50.0, &rooms(a));
            let to = graph.waypoint_find_closest_to_pos(level, b + Vec3::Y * 50.0, &rooms(b));
            let len = match (from, to) {
                (Some(f), Some(t)) => {
                    let (route, _) = graph.nav_find_route(f, t, 100_000, NavSeed(1, 1), &mut rng);
                    if route.last() != Some(&t) {
                        None
                    } else {
                        let mut pts = vec![a];
                        pts.extend(route.iter().map(|&w| graph.waypoint_pos(w) - Vec3::Y * 53.0));
                        pts.push(b);
                        Some(pts.windows(2).map(|w| w[0].distance(w[1])).sum())
                    }
                }
                _ => None,
            };
            out.push(((i, j), len));
        }
    }
    out
}
