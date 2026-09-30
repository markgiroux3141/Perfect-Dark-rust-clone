//! Hand edits to a level's waypoint graph ([`crate::layout::WaypointEdits`]),
//! replayed on the generated graph each time the level is placed: waypoints
//! removed, moved and added, links cut and made, each found by position.
//! The graph is then rebuilt in PD's format with its waygroups as the
//! generator builds them (`nav::gen::graph_from_links`), so an edited graph
//! routes like a generated one.

use glam::Vec3;
use pd_sim::nav::gen::{graph_from_links, GenParams};
use pd_sim::nav::{NavGraph, NavPad, PadFlags};
use pd_sim::stage::{directed_links, TileLevel};

use crate::layout::WaypointEdits;

/// A position keys the waypoint within this many cm of it.
pub const TOL: f32 = 30.0;

/// The live waypoint nearest `p` within [`TOL`].
fn find(pads: &[NavPad], alive: &[bool], p: Vec3) -> Option<usize> {
    (0..pads.len()).filter(|&i| alive[i]).map(|i| (i, pads[i].pos.distance(p))).filter(|&(_, d)| d <= TOL).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
}

/// `graph` with `edits` applied (`level` gives a moved or added waypoint its
/// room), and a report line.
pub fn apply(graph: &NavGraph, edits: &WaypointEdits, level: &TileLevel) -> (NavGraph, String) {
    let n = graph.waypoints.len();
    let mut pads: Vec<NavPad> = graph.waypoints.iter().map(|w| graph.pads[w.padnum].clone()).collect();
    let mut fwd: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, b, _) in directed_links(n, |w| &graph.waypoints[w].neighbours) {
        if !fwd[a].contains(&b) {
            fwd[a].push(b);
        }
    }
    let mut alive = vec![true; n];
    let mut missed = 0;
    let v = |p: [f32; 3]| Vec3::from(p);
    for &p in &edits.removed {
        match find(&pads, &alive, v(p)) {
            Some(i) => alive[i] = false,
            None => missed += 1,
        }
    }
    for m in &edits.moved {
        match find(&pads, &alive, v(m.from)) {
            Some(i) => {
                pads[i].pos = v(m.to);
                pads[i].room = level.floor_room(v(m.to), 20.0);
            }
            None => missed += 1,
        }
    }
    for &p in &edits.added {
        pads.push(NavPad { pos: v(p), room: level.floor_room(v(p), 20.0), flags: PadFlags::default() });
        fwd.push(Vec::new());
        alive.push(true);
    }
    for l in &edits.unlinked {
        match (find(&pads, &alive, v(l.a)), find(&pads, &alive, v(l.b))) {
            (Some(a), Some(b)) => {
                fwd[a].retain(|&x| x != b);
                if !l.one_way {
                    fwd[b].retain(|&x| x != a);
                }
            }
            _ => missed += 1,
        }
    }
    for l in &edits.linked {
        match (find(&pads, &alive, v(l.a)), find(&pads, &alive, v(l.b))) {
            (Some(a), Some(b)) if a != b => {
                if !fwd[a].contains(&b) {
                    fwd[a].push(b);
                }
                if !l.one_way && !fwd[b].contains(&a) {
                    fwd[b].push(a);
                }
            }
            _ => missed += 1,
        }
    }
    // The live waypoints, renumbered.
    let mut new_index = vec![usize::MAX; pads.len()];
    let mut out_pads = Vec::new();
    for (i, p) in pads.iter().enumerate().filter(|(i, _)| alive[*i]) {
        new_index[i] = out_pads.len();
        out_pads.push(p.clone());
    }
    let out_fwd: Vec<Vec<usize>> = (0..pads.len()).filter(|&i| alive[i]).map(|i| fwd[i].iter().filter(|&&j| alive[j]).map(|&j| new_index[j]).collect()).collect();
    let line = format!(
        "waypoint edits: {} removed, {} moved, {} added, {} links cut, {} made{}",
        edits.removed.len(),
        edits.moved.len(),
        edits.added.len(),
        edits.unlinked.len(),
        edits.linked.len(),
        if missed > 0 { format!("; {missed} found no waypoint (the geometry changed under them?)") } else { String::new() }
    );
    (graph_from_links(out_pads, &out_fwd, GenParams::default().group_radius), line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Link, Moved};

    /// A row of five waypoints 2 m apart, linked both ways in a chain, on a
    /// floor.
    fn chain() -> (NavGraph, TileLevel) {
        use pd_sim::stage::{GeomPoly, LevelGeom};
        let floor = GeomPoly::from_tile(1, pd_core::ids::GEOFLAG_FLOOR1 | pd_core::ids::GEOFLAG_FLOOR2, 0, vec![Vec3::new(-500.0, 0.0, -500.0), Vec3::new(1500.0, 0.0, -500.0), Vec3::new(1500.0, 0.0, 500.0), Vec3::new(-500.0, 0.0, 500.0)]);
        let level = TileLevel::new(LevelGeom { polys: vec![floor], rooms: vec![1] });
        let pads: Vec<NavPad> = (0..5).map(|k| NavPad { pos: Vec3::new(k as f32 * 200.0, 53.0, 0.0), room: None, flags: PadFlags::default() }).collect();
        let fwd: Vec<Vec<usize>> = (0..5).map(|k: usize| [k.wrapping_sub(1), k + 1].into_iter().filter(|&j| j < 5).collect()).collect();
        (graph_from_links(pads, &fwd, GenParams::default().group_radius), level)
    }

    #[test]
    fn edits_replay_on_the_generated_graph() {
        let (g, level) = chain();
        let e = WaypointEdits {
            // The middle one gone, the last moved sideways, one added beside the
            // first; the cut chain joined round the gap one way, a new link to the
            // added one, and one key that finds nothing.
            removed: vec![[400.0, 53.0, 0.0]],
            moved: vec![Moved { from: [800.0, 53.0, 0.0], to: [800.0, 53.0, 300.0] }],
            added: vec![[0.0, 53.0, 300.0]],
            unlinked: vec![Link { a: [5000.0, 0.0, 0.0], b: [0.0, 53.0, 0.0], one_way: false }],
            linked: vec![Link { a: [200.0, 53.0, 0.0], b: [600.0, 53.0, 0.0], one_way: true }, Link { a: [0.0, 53.0, 0.0], b: [0.0, 53.0, 310.0], one_way: false }],
        };
        let (out, line) = apply(&g, &e, &level);
        assert!(line.contains("1 found no waypoint"), "{line}");
        assert_eq!(out.waypoints.len(), 5);
        let at = |x: f32, z: f32| (0..out.waypoints.len()).find(|&w| out.waypoint_pos(w).distance(Vec3::new(x, 53.0, z)) < 1.0).unwrap();
        let links = directed_links(out.waypoints.len(), |w| &out.waypoints[w].neighbours);
        let has = |a: usize, b: usize| links.iter().any(|&(x, y, _)| x == a && y == b);
        assert!(has(at(200.0, 0.0), at(600.0, 0.0)) && !has(at(600.0, 0.0), at(200.0, 0.0)), "one way round the gap");
        assert!(has(at(0.0, 0.0), at(0.0, 300.0)) && has(at(0.0, 300.0), at(0.0, 0.0)), "the added one, both ways");
        assert!(has(at(600.0, 0.0), at(800.0, 300.0)), "the moved one keeps its links");
        // Every waypoint is in a group, and the groups cover them all.
        assert_eq!(out.waygroups.iter().map(|g| g.waypoints.len()).sum::<usize>(), 5);
    }
}
