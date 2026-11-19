//! Every corridor, stub and vent of a level: which rooms are joined, and in what order. A port
//! of `autolevel/route.py` (`Network`, `separation`, `storeys_apart`, `connect`, `components`).
//!
//! A spanning tree over the shortest gaps comes first, so nothing is cut off; then risers
//! (enclosed ramps between rooms close together but storeys apart), loops between rooms at
//! least three doors apart, a second door for rooms left with one, and vents. Each link has to
//! find an actual corridor on the grid, and rooms on a common storey come first.

use crate::py;
use crate::route::{Grid, Route, Terms, VentTerms, RAMP, WIDE};
use crate::scatter::{hops, rect_gap};
use crate::space::{Door, Space};
use crate::storeys::index;
use std::collections::{BTreeMap, BTreeSet};

/// The rooms and what joins them.
#[derive(Clone, Debug)]
pub struct Network {
    /// Who owns each cell: the rooms (`grid.sp`), their doors and the routes are kept here.
    pub grid: Grid,
    /// (pod, host): joined inside the host by the furnisher.
    pub pods: Vec<(String, String)>,
    /// Spanning-tree pairs that found no corridor.
    pub failed: Vec<(String, String)>,
}

impl Network {
    /// The rooms, in the order they were scattered.
    pub fn spaces(&self) -> &[Space] {
        &self.grid.sp
    }

    /// Corridors, stubs and vents, in the order routed.
    pub fn routes(&self) -> &[Route] {
        &self.grid.routes
    }

    /// Every pair of rooms joined: by a route, then pods and their hosts.
    pub fn edges(&self) -> Vec<(String, String)> {
        self.grid.routes.iter().map(|r| (r.a.room.clone(), r.b.room.clone())).chain(self.pods.iter().cloned()).collect()
    }

    pub fn doors(&self, key: &str) -> &[Door] {
        self.grid.doors(key)
    }

    /// Take a room out of the level, with its pods and every corridor to them.
    pub fn drop(&mut self, key: &str) {
        let pods: Vec<String> = self.grid.sp.iter().filter(|q| q.host == key).map(|q| q.key.clone()).collect();
        for q in pods {
            self.drop(&q);
        }
        self.grid.drop(key);
        self.pods.retain(|(a, b)| a != key && b != key);
    }
}

/// The fewest storeys between a storey of one list and a storey of the other.
fn storeys_between(la: &[i32], lb: &[i32]) -> i32 {
    la.iter().flat_map(|&x| lb.iter().map(move |&y| (index(x) as i32 - index(y) as i32).abs())).min().expect("a room has a storey")
}

/// How far apart two rooms are to join, in cells. Rooms with a storey in common are as far
/// apart as the gap between them. Rooms without one need an enclosed ramp, the dearest thing a
/// corridor can hold, so they count as much further: links then run along one storey (a high
/// road, a low road), and the level changes happen inside the tall rooms.
pub fn separation(a: &Space, b: &Space, level_change: f64) -> f64 {
    let (dx, dy) = rect_gap(a.rect(), b.rect());
    let (la, lb) = (a.door_levels(), b.door_levels());
    if la.iter().any(|z| lb.contains(z)) {
        return (dx + dy) as f64;
    }
    let run = storeys_between(&la, &lb) * (RAMP + WIDE);
    (dx + dy).max(run) as f64 + 0.5 * run as f64 + level_change
}

/// The fewest storeys a corridor between two rooms has to climb.
pub fn storeys_apart(a: &Space, b: &Space) -> i32 {
    storeys_between(&a.door_levels(), &b.door_levels())
}

/// How the rooms are joined up (`connect`'s keyword arguments).
#[derive(Clone, Debug)]
pub struct Connect {
    /// Loops to add, as a share of the number of rooms.
    pub loops: f64,
    /// The furthest apart (cells) two rooms may be for a loop.
    pub reach: i32,
    /// The dearest corridor of the spanning tree, and of a loop.
    pub tree_cost: f64,
    pub loop_cost: f64,
    /// The most links a room may have, if not by its floor area.
    pub degree: Option<fn(&Space) -> usize>,
    /// How much further apart rooms with no storey in common count.
    pub level_change: f64,
    /// Enclosed ramp corridors between rooms two or more storeys apart: how many, the dearest,
    /// and the furthest apart.
    pub risers: usize,
    pub riser_cost: f64,
    pub riser_reach: i32,
    /// A one-door room gets a second door to a neighbour within this many cells (0: off).
    pub second: i32,
    /// Crawl ducts: how many, and the furthest apart.
    pub vents: usize,
    pub vent_reach: i32,
}

impl Default for Connect {
    fn default() -> Self {
        Connect {
            loops: 0.4,
            reach: 14,
            tree_cost: 70.0,
            loop_cost: 34.0,
            degree: None,
            level_change: 10.0,
            risers: 0,
            riser_cost: 64.0,
            riser_reach: 9,
            second: 0,
            vents: 0,
            vent_reach: 8,
        }
    }
}

/// The most links a room takes, by its floor area.
fn cap_by_area(a: &Space) -> usize {
    match a.area() {
        ..=6 => 2,
        7..=20 => 3,
        21..=70 => 4,
        _ => 7,
    }
}

/// A join the spanning tree tries: (the gap, room a, room b, the storeys a's door may be on,
/// and b's; None: any it offers).
type TreeJoin = (f64, usize, usize, Option<Vec<i32>>, Option<Vec<i32>>);

/// Route everything: a spanning tree over the shortest gaps first, so nothing is cut off, then
/// loops between rooms at least three doors apart (each link has to find an actual corridor,
/// and rooms on a common storey come first: see `separation`). Pods join their hosts without
/// one. `band` is a catwalk's depth in half cells.
pub fn connect(sp: Vec<Space>, band: i32, c: &Connect) -> Network {
    // rooms are known by their place in `sp` here; nothing is dropped until the network is made
    let n = sp.len();
    let cap = |a: &Space| c.degree.map_or_else(|| cap_by_area(a), |f| f(a));
    let mut g = Grid::new(sp, band);
    let at = |g: &Grid, key: &str| g.sp.iter().position(|s| s.key == key).unwrap_or_else(|| panic!("no room {key}"));
    let pods: Vec<(usize, usize)> = (0..n).filter(|&i| !g.sp[i].host.is_empty()).map(|i| (i, at(&g, &g.sp[i].host))).collect();
    // A towering hall's high storeys are places of their own, which nothing joins to the rest of
    // it inside (`Space::high`). Each is a node of the tree's own, past the rooms' (`n + k`), and
    // a join to the hall is tried for each of its places, the route held to that one's storeys.
    // A level with no towering hall has no such node and the same joins in the same order as ever.
    let tops: Vec<(usize, i32)> = (0..n).flat_map(|i| g.sp[i].high.iter().map(move |&z| (i, z))).collect();
    let node = |i: usize, level: i32| tops.iter().position(|&(a, z)| a == i && z == level).map_or(i, |k| n + k);
    // the storeys of each place of a room: None for a room that is one place
    let bands: Vec<Vec<Option<Vec<i32>>>> = (0..n)
        .map(|i| {
            let high = &g.sp[i].high;
            if high.is_empty() {
                return vec![None];
            }
            let low: Vec<i32> = g.sp[i].door_levels().into_iter().filter(|z| !high.contains(z)).collect();
            std::iter::once(Some(low)).chain(high.iter().map(|&z| Some(vec![z]))).collect()
        })
        .collect();
    let band_node = |i: usize, band: &Option<Vec<i32>>| match band.as_deref() {
        Some(&[z]) => node(i, z),
        _ => i,
    };
    let mut parent: Vec<usize> = (0..n + tops.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    let mut deg = vec![0usize; n];
    for &(a, b) in &pods {
        let root = find(&mut parent, node(b, g.sp[a].floor));
        let from = find(&mut parent, a);
        parent[from] = root;
        deg[a] += 1;
        deg[b] += 1;
    }
    let by_keys = |g: &Grid, p: (usize, usize), q: (usize, usize)| g.sp[p.0].key.cmp(&g.sp[q.0].key).then_with(|| g.sp[p.1].key.cmp(&g.sp[q.1].key));
    let mut cand: Vec<(f64, usize, usize)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&g.sp[i], &g.sp[j]);
            if a.host != b.key && b.host != a.key {
                cand.push((separation(a, b, c.level_change), i, j));
            }
        }
    }
    cand.sort_by(|p, q| py::cmp_f64(p.0, q.0).then_with(|| by_keys(&g, (p.1, p.2), (q.1, q.2))));
    // the tree's candidates: each pair once for each place of each of the two
    let tree: Vec<TreeJoin> = cand
        .iter()
        .flat_map(|&(d, a, b)| {
            let lbs = &bands[b];
            bands[a].iter().flat_map(move |la| lbs.iter().map(move |lb| (d, a, b, la.clone(), lb.clone())))
        })
        .filter(|(_, _, _, la, lb)| la.as_ref().is_none_or(|l| !l.is_empty()) && lb.as_ref().is_none_or(|l| !l.is_empty()))
        .collect();
    let mut failed: Vec<(usize, usize)> = Vec::new();
    let mut links: Vec<(usize, usize)> = pods.clone();
    // Twice over: first no corridor may be much longer than the gap it crosses (a corridor that
    // wanders round a neighbour walls that neighbour in), then whatever is still cut off gets
    // the full allowance.
    for full in [false, true] {
        failed = Vec::new();
        for (d, a, b, la, lb) in &tree {
            let (a, b, d) = (*a, *b, *d);
            let (na, nb) = (band_node(a, la), band_node(b, lb));
            if find(&mut parent, na) == find(&mut parent, nb) {
                continue;
            }
            let allowance = if full { c.tree_cost } else { c.tree_cost.min(2.5 * d + 20.0) };
            let Some(r) = g.route(&g.sp[a], &g.sp[b], &Terms { la: la.clone(), lb: lb.clone(), ..Terms::within(allowance) }) else {
                failed.push((a, b));
                continue;
            };
            g.commit(r);
            let root = find(&mut parent, nb);
            let from = find(&mut parent, na);
            parent[from] = root;
            links.push((a, b));
            deg[a] += 1;
            deg[b] += 1;
        }
        let roots: BTreeSet<usize> = (0..n + tops.len()).map(|k| find(&mut parent, k)).collect();
        if roots.len() == 1 {
            break;
        }
    }
    // how many doors apart two rooms are (99: no way between them)
    let doors_apart = |links: &[(usize, usize)], a: usize, b: usize| hops(links, &a).iter().find(|(k, _)| *k == b).map_or(99, |(_, far)| *far);
    // risers: enclosed ramp corridors between rooms close together but two or more storeys
    // apart. Short of room to run straight, they come out as switchbacks.
    if c.risers > 0 {
        let mut up: Vec<(i32, i32, usize, usize)> = Vec::new();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&g.sp[i], &g.sp[j]);
                if !a.host.is_empty() || !b.host.is_empty() {
                    continue;
                }
                let storeys = storeys_apart(a, b);
                let (dx, dy) = rect_gap(a.rect(), b.rect());
                if storeys >= 2 && dx + dy <= c.riser_reach {
                    up.push((storeys, dx + dy, i, j));
                }
            }
        }
        up.sort_by(|p, q| (p.0, p.1).cmp(&(q.0, q.1)).then_with(|| by_keys(&g, (p.2, p.3), (q.2, q.3))));
        let mut made = 0;
        for (_, _, a, b) in up {
            if made >= c.risers {
                break;
            }
            if deg[a] >= cap(&g.sp[a]) || deg[b] >= cap(&g.sp[b]) || doors_apart(&links, a, b) < 3 {
                continue;
            }
            let Some(r) = g.route(&g.sp[a], &g.sp[b], &Terms::within(c.riser_cost)) else { continue };
            g.commit(r);
            links.push((a, b));
            deg[a] += 1;
            deg[b] += 1;
            made += 1;
        }
    }
    let want = links.len() + py::round(c.loops * n as f64) as usize;
    for &(d, a, b) in &cand {
        if links.len() >= want || d > c.reach as f64 {
            break;
        }
        if deg[a] >= cap(&g.sp[a]) || deg[b] >= cap(&g.sp[b]) {
            continue;
        }
        if doors_apart(&links, a, b) < 3 {
            continue;
        }
        let Some(r) = g.route(&g.sp[a], &g.sp[b], &Terms::within(c.loop_cost)) else { continue };
        g.commit(r);
        links.push((a, b));
        deg[a] += 1;
        deg[b] += 1;
    }
    let pair = |a: usize, b: usize| (a.min(b), a.max(b));
    // a second way out of a room that has only one door (pods and closets aside), where a
    // neighbour it isn't joined to is within `second` cells
    if c.second != 0 {
        let mut joined: BTreeSet<(usize, usize)> = links.iter().map(|&(a, b)| pair(a, b)).collect();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&p, &q| g.sp[q].area().cmp(&g.sp[p].area()).then_with(|| g.sp[p].key.cmp(&g.sp[q].key)));
        for k in order {
            if deg[k] != 1 || !g.sp[k].host.is_empty() || g.sp[k].area() < 9 {
                continue;
            }
            let mut near: Vec<(f64, usize)> = (0..n)
                .filter(|&j| j != k && g.sp[j].host.is_empty() && !joined.contains(&pair(k, j)))
                .map(|j| (separation(&g.sp[k], &g.sp[j], c.level_change), j))
                .collect();
            near.sort_by(|p, q| py::cmp_f64(p.0, q.0).then_with(|| g.sp[p.1].key.cmp(&g.sp[q.1].key)));
            for &(d, j) in near.iter().take(5) {
                if d > c.second as f64 {
                    break;
                }
                if deg[j] > cap(&g.sp[j]) {
                    continue;
                }
                let Some(r) = g.route(&g.sp[k], &g.sp[j], &Terms::within(c.loop_cost.min(2.5 * d + 20.0))) else { continue };
                g.commit(r);
                links.push((k, j));
                joined.insert(pair(k, j));
                deg[k] += 1;
                deg[j] += 1;
                break;
            }
        }
    }
    // vents: first for the rooms that still have one door, then as short cuts between rooms
    // close together but three or more doors apart
    if c.vents > 0 {
        let mut joined: BTreeSet<(usize, usize)> = links.iter().map(|&(a, b)| pair(a, b)).collect();
        let mut cand: Vec<(i32, usize, usize)> = Vec::new();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&g.sp[i], &g.sp[j]);
                if !a.host.is_empty() || !b.host.is_empty() || joined.contains(&pair(i, j)) {
                    continue;
                }
                let up = storeys_between(&a.vent_levels(), &b.vent_levels());
                let (dx, dy) = rect_gap(a.rect(), b.rect());
                if dx + dy <= c.vent_reach && up <= 2 {
                    cand.push((dx + dy + 6 * up, i, j));
                }
            }
        }
        cand.sort_by(|p, q| p.0.cmp(&q.0).then_with(|| by_keys(&g, (p.1, p.2), (q.1, q.2))));
        let mut made = 0;
        for dead in [true, false] {
            for &(_, a, b) in &cand {
                if made >= c.vents {
                    break;
                }
                if joined.contains(&pair(a, b)) {
                    continue;
                }
                if dead != (deg[a] == 1 || deg[b] == 1) || (!dead && doors_apart(&links, a, b) < 3) {
                    continue;
                }
                let Some(r) = g.vent(&g.sp[a], &g.sp[b], &VentTerms::default()) else { continue };
                g.commit(r);
                links.push((a, b));
                joined.insert(pair(a, b));
                deg[a] += 1;
                deg[b] += 1;
                made += 1;
            }
        }
    }
    let name = |g: &Grid, (a, b): (usize, usize)| (g.sp[a].key.clone(), g.sp[b].key.clone());
    let pods = pods.iter().map(|&e| name(&g, e)).collect();
    let failed = failed.iter().map(|&e| name(&g, e)).collect();
    Network { grid: g, pods, failed }
}

/// How many pieces the level is in (1: every room reaches every other).
pub fn components(net: &Network) -> usize {
    let edges = net.edges();
    let mut nb: BTreeMap<&str, BTreeSet<&str>> = net.spaces().iter().map(|s| (s.key.as_str(), BTreeSet::new())).collect();
    for (a, b) in &edges {
        nb.get_mut(a.as_str()).unwrap_or_else(|| panic!("no room {a}")).insert(b);
        nb.get_mut(b.as_str()).unwrap_or_else(|| panic!("no room {b}")).insert(a);
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut n = 0;
    for s in net.spaces() {
        if seen.contains(s.key.as_str()) {
            continue;
        }
        n += 1;
        let mut todo = vec![s.key.as_str()];
        while let Some(q) = todo.pop() {
            if seen.insert(q) {
                todo.extend(nb[q].iter().copied());
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::tests::{room, space};

    #[test]
    fn rooms_with_no_storey_in_common_count_as_further_apart() {
        let (a, b) = (room("a", (0, 0, 6, 6)), room("b", (9, 2, 14, 7)));
        assert_eq!(separation(&a, &b, 10.0), 3.0);
        let up = space("up", (9, 2, 14, 7), 551, 787, "medium");
        assert_eq!(storeys_apart(&a, &up), 2);
        assert_eq!(separation(&a, &up, 10.0), 10.0 + 5.0 + 10.0); // two ramps' run, half as much again, and the change
    }

    #[test]
    fn a_room_can_be_taken_out_again() {
        let sp = vec![room("a", (0, 0, 6, 6)), room("b", (14, 9, 20, 15)), room("c", (24, 9, 28, 15))];
        let mut net = connect(sp, 2, &Connect::default());
        assert_eq!((components(&net), net.routes().len(), net.failed.len()), (1, 2, 0));
        assert!(net.doors("b").iter().any(|d| net.routes().iter().any(|r| r.b.room == "c" && d.to == r.key())));
        let taken: Vec<_> = net.routes().iter().filter(|r| r.a.room == "c" || r.b.room == "c").cloned().collect();
        net.drop("c");
        assert!(net.spaces().iter().all(|s| s.key != "c"));
        assert!(net.routes().iter().all(|r| r.a.room != "c" && r.b.room != "c"));
        assert!(net.doors("b").iter().all(|d| taken.iter().all(|r| d.to != r.key())));
        assert_eq!((net.routes().len(), net.grid.doors.len()), (1, 2));
        // its corridor's cells are free again, and so are its own
        for r in &taken {
            for (z, cells) in r.pieces.iter().flat_map(|pc| &pc.cells) {
                assert!(cells.iter().all(|&(x, y)| net.grid.owner_at(x, y, *z) == 0));
            }
        }
        assert_eq!(net.grid.owner_at(25, 10, 0), 0);
        assert_eq!(components(&net), 1);
    }

    #[test]
    fn a_pod_goes_with_its_hall_and_gives_its_block_back() {
        let hall = space("hall", (0, 0, 12, 10), 0, 787, "vast");
        let mut pod = space("pod", (0, 3, 3, 6), 276, 512, "small");
        pod.host = "hall".into();
        let sp = vec![hall, pod, room("b", (15, 2, 20, 8))];
        let mut net = connect(sp.clone(), 2, &Connect::default());
        assert_eq!(net.pods, [("pod".to_string(), "hall".to_string())]);
        assert_eq!(components(&net), 1);
        let (hall_id, pod_id) = (net.grid.id_of("hall").unwrap(), net.grid.id_of("pod").unwrap());
        assert_eq!((net.grid.owner_at(1, 4, 276), net.grid.owner_at(1, 4, 0), net.grid.owner_at(3, 6, 276)), (pod_id, hall_id, pod_id));
        let mut one = net.clone();
        one.grid.drop("pod");
        assert_eq!((one.grid.owner_at(1, 4, 276), one.grid.owner_at(3, 6, 276)), (hall_id, hall_id));
        net.drop("hall");
        assert_eq!(net.spaces().iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["b"]);
        assert!(net.pods.is_empty() && net.routes().is_empty() && net.doors("b").is_empty());
        assert_eq!((components(&net), net.grid.owner_at(1, 4, 276), net.grid.owner_at(5, 5, 0)), (1, 0, 0));
    }
}
