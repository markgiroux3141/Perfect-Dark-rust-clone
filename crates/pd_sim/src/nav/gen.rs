//! **Our replacement for PD's hand-placed waypoints**: a waypoint graph generated
//! from level geometry and emitted in PD's own format ([`NavGraph`]), so PD's
//! ported routing (`pd_nav`) runs on it unchanged.
//!
//! The input is only a [`TileLevel`], i.e. a generic [`LevelGeom`] (floor, wall,
//! blocker, ladder and crouch polygons, optional rooms). Nothing here reads PD
//! pads or PD's graph, so the same generator can serve levels made in our editor.
//!
//! 1. **Sample** every floor surface on an XZ lattice. A sample is kept if a chr
//!    cylinder with a margin fits there by PD's own volume test, and PD's ground
//!    finder puts a chr standing there on that floor.
//! 2. **Link** lattice neighbours with a kinematic probe that follows the movement
//!    code's rules: the 69 cm step probe, drops (one-way), walls, and the crouch
//!    height near crouch tiles. Ladders add a climb link from their base to their top.
//! 3. **Sparsify** by greedy cover: the widest-clearance uncovered sample becomes a
//!    node and covers every sample it can see within `cover` cm along the lattice.
//!    Samples go to their nearest node (Voronoi on the lattice); nodes whose regions
//!    touch become candidate links.
//! 4. **Validate** each candidate with the probe, node to node. A link that fails
//!    gets the two lattice samples where the regions touch inserted as nodes, and
//!    the step repeats.
//! 5. **Emit** PD's format: waypoints (pads 53 cm above the floor, as PD places
//!    them) with `WPSEGFLAG_*` on one-way links, and waygroups = the room's nodes
//!    split into strongly connected parts, because PD routes inside a group only
//!    through that group.
//!
//! Not PD: this is our replacement for PD's hand-placed graphs (the Complex
//! spike's decision, SPIKE_PD_COMPLEX.md), selectable per match. PD's arenas
//! route on PD's own graph by default.
//!
//! Source: the old repo's `pd_spike/navgen.rs`, unchanged but for the
//! collision calls' `checkvertical` (always `CHECKVERTICAL_YES` for a chr).

use std::collections::{BinaryHeap, HashMap, VecDeque};

use glam::{Vec2, Vec3};

use super::{NavGraph, NavPad, NavWaygroup, NavWaypoint, PadFlags};
use crate::stage::{CdResult, FloorKind, LevelGeom, TileFlag, TileLevel, WPSEGFLAG_INWARDSONLY, WPSEGFLAG_OUTWARDSONLY};

#[derive(Clone, Copy, Debug)]
pub struct GenParams {
    /// Lattice spacing (cm).
    pub spacing: f32,
    /// Chr radius (`chr->radius`) and the extra wall clearance a sample needs.
    pub radius: f32,
    pub margin: f32,
    /// A node covers samples within this lattice distance that it can see (cm).
    pub cover: f32,
    /// Step height every chr climbs (`manground + 69`).
    pub step: f32,
    /// The highest a chr may step: PD probes the ground from `max(manground + 69,
    /// prop->pos.y)` (`chr.c:830`), and a running bot's root is 87–120 cm up
    /// (measured over the spike's bodies), so a tall one mounts what a short one
    /// can't. Paths and samples avoid anything a bot could step onto by accident.
    pub step_high: f32,
    /// Waygroups gather nodes within this distance (cm) of a seed node, over
    /// two-way links (see [`group_nodes`]).
    pub group_radius: f32,
    /// A walk link is dropped when a two-link detour is at most this much longer.
    pub prune: f32,
    /// Put nodes in crouch zones (crawl spaces). Off, bots route around them.
    pub crouch_zones: bool,
}

impl Default for GenParams {
    fn default() -> Self {
        GenParams { spacing: 50.0, radius: 20.0, margin: 3.0, cover: 200.0, step: 69.0, step_high: 120.0, group_radius: 200.0, prune: 1.1, crouch_zones: true }
    }
}

/// How a chr gets from one sample (or node) to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkKind {
    Walk,
    /// Off a ledge: one way.
    Drop,
    /// Up a ladder: one way (bots climb ladders but can't descend them).
    Climb,
}

#[derive(Clone, Debug)]
pub struct Sample {
    /// The floor point (cm).
    pub pos: Vec3,
    pub room: Option<u16>,
    pub crouch: bool,
    /// Distance to the nearest wall at chr height, from a few probe radii (cm).
    pub clearance: f32,
}

pub struct Generated {
    pub graph: NavGraph,
    pub samples: Vec<Sample>,
    /// Lattice links (sample → sample).
    pub links: Vec<(usize, usize, LinkKind)>,
    /// Each node's sample.
    pub node_samples: Vec<usize>,
    /// Node links (u → v, kind).
    pub node_links: Vec<(usize, usize, LinkKind)>,
    pub iterations: usize,
    /// Node count after the cover, then after each validation round.
    pub node_counts: Vec<usize>,
    /// Candidate links that still failed validation after the last iteration.
    pub unresolved: usize,
}

/// The probe keeps this much more than a radius from walls: it tests every 8 cm,
/// the real chr lands wherever its ~7.6 cm steps take it.
const PATH_MARGIN: f32 = 2.0;

/// A chr's root is about this far above its floor; the probes stand in for
/// `prop->pos` there, exactly as the movement code measures from it.
const ROOT: f32 = 50.0;

/// The movement code's height near duck/crouch tiles (`chr_update_position`).
fn chr_height_at(level: &TileLevel, floor: Vec3, radius: f32) -> f32 {
    let prop = floor + Vec3::Y * ROOT;
    if level.is_cyl_touching_tile_with_flags(TileFlag::Duck, prop, radius * 1.1, 185.0 - ROOT, -10.0 - ROOT) {
        135.0
    } else if level.is_cyl_touching_tile_with_flags(TileFlag::Crouch, prop, radius * 1.1, 135.0 - ROOT, -10.0 - ROOT) {
        90.0
    } else {
        185.0
    }
}

/// Is a chr cylinder of `radius` standing on `floor` clear of walls?
fn fits(level: &TileLevel, floor: Vec3, radius: f32, height: f32) -> bool {
    level.cd_test_volume_simple(floor + Vec3::Y * ROOT, radius, true, height - ROOT, 20.0 - ROOT, &[]) == CdResult::NoCollision
}

/// The kinematic probe: step a chr from floor point `a` towards `b` in ~8 cm steps
/// the way the movement code would — the floor found from 69 cm above the current
/// height, a drop when it falls away by more than a step, blocked by any wall —
/// and report how it got there (or `None` if it didn't end on `b`'s floor).
pub fn probe_walk(level: &TileLevel, a: Vec3, b: Vec3, p: &GenParams) -> Option<LinkKind> {
    probe_walk_why(level, a, b, p).ok()
}

/// [`probe_walk`], saying why it failed.
pub fn probe_walk_why(level: &TileLevel, a: Vec3, b: Vec3, p: &GenParams) -> Result<LinkKind, String> {
    let d = Vec2::new(b.x - a.x, b.z - a.z);
    let n = (d.length() / 8.0).ceil().max(1.0) as usize;
    let mut m = a.y;
    let mut dropped = false;
    // Distance walked at the top of a drop that couldn't start yet.
    let mut hover = 0.0f32;
    let step_len = d.length() / n as f32;
    let mut next_height = chr_height_at(level, a, p.radius);
    for k in 1..=n {
        let height = next_height;
        let t = k as f32 / n as f32;
        let (x, z) = (a.x + d.x * t, a.z + d.y * t);
        let (ground, _) = level.cd_find_ground_at_cyl(Vec3::new(x, m + p.step, z), p.radius);
        if ground < -100_000.0 {
            return Err(format!("no floor at ({x:.0}, {z:.0})"));
        }
        let (high, _) = level.cd_find_ground_at_cyl(Vec3::new(x, m + p.step_high, z), p.radius);
        if high > ground + 1.0 {
            return Err(format!("a surface a tall bot would step onto at ({x:.0}, {z:.0}): {high:.0} over {ground:.0}"));
        }
        if ground < m - p.step {
            dropped = true;
            // A falling chr can't descend while its cylinder overlaps a wall
            // (`chr_ascend`); it keeps walking at the height it was and falls once
            // the whole drop is clear. Past a ledge that's a few cm on, clear of the
            // ledge's own face. Longer "hovers" (along a gap between walls) happen
            // in PD too but are fragile: a link may hover at most 30 cm.
            let top = Vec3::new(x, m, z);
            let clear = level.cd_test_volume_simple(top + Vec3::Y * ROOT, p.radius + PATH_MARGIN, true, height - ROOT, ground + 20.0 - m - ROOT, &[]);
            if clear != CdResult::NoCollision {
                hover += step_len;
                if k == n || hover > 30.0 {
                    return Err(format!("a wall in the way of the drop at ({x:.0}, {z:.0}), {m:.0} -> {ground:.0}"));
                }
                continue;
            }
            hover = 0.0;
        } else if hover > 0.0 {
            // The floor came back before the fall could start: that was a gap
            // crossed by hovering, which a slightly different line would fall into.
            return Err(format!("crosses a gap only by hovering, at ({x:.0}, {z:.0})"));
        }
        m = ground;
        let here = Vec3::new(x, m, z);
        // Entering a crouch zone, the real chr's move is refused against the low
        // ceiling and slides along it until it's within the crouch trigger (22 cm,
        // 2 cm inside its radius), then ducks under: test with the height it ends up at.
        let height = height.min(chr_height_at(level, here, p.radius));
        // A chr on a go-to within 2.5 radii of a ladder climbs it (`chr.c:618`):
        // a walk must stay out of that reach (a Climb link goes in on purpose).
        let prop = here + Vec3::Y * ROOT;
        if level.cd_find_ladder(prop, p.radius * 2.5 + PATH_MARGIN, height - ROOT, 1.0 - ROOT).is_some() {
            return Err(format!("within a ladder's reach at ({x:.0}, {m:.0}, {z:.0})"));
        }
        if !fits(level, here, p.radius + PATH_MARGIN, height) {
            let walls = level.walls_touching(here + Vec3::Y * ROOT, p.radius, height - ROOT, 20.0 - ROOT);
            return Err(format!("wall at ({x:.0}, {m:.0}, {z:.0}): polys {walls:?}"));
        }
        next_height = chr_height_at(level, here, p.radius);
    }
    if (m - b.y).abs() > 30.0 {
        return Err(format!("ended at y {m:.0}, goal floor {:.0}", b.y));
    }
    Ok(if dropped { LinkKind::Drop } else { LinkKind::Walk })
}

pub fn generate(level: &TileLevel, p: &GenParams) -> Generated {
    let geom = &level.geom;
    let (samples, cells) = sample(level, geom, p);
    let mut links = lattice_links(level, &samples, &cells, p);
    links.extend(ladder_links(level, geom, &samples));
    links.sort();
    links.dedup();

    // Undirected lattice adjacency with XZ lengths, for cover and regions.
    let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); samples.len()];
    for &(a, b, _) in &links {
        let w = Vec2::new(samples[a].pos.x - samples[b].pos.x, samples[a].pos.z - samples[b].pos.z).length().max(1.0);
        adj[a].push((b, w));
        adj[b].push((a, w));
    }

    // Ladder ends are always nodes: a climb is a link of its own.
    let mut nodes: Vec<usize> = Vec::new();
    for &(a, b, k) in &links {
        if k == LinkKind::Climb {
            for s in [a, b] {
                if !nodes.contains(&s) {
                    nodes.push(s);
                }
            }
        }
    }
    // `owner[s]`: the node that covered sample `s`, which can walk to it straight.
    let mut owner = vec![usize::MAX; samples.len()];
    for (k, &n) in nodes.iter().enumerate() {
        owner[n] = k;
    }
    greedy_cover(level, &samples, &adj, &mut nodes, &mut owner, p);
    let mut node_counts = vec![nodes.len()];

    // Link nodes that can walk to each other straight, then repair reachability:
    // for every lattice link a → b, a's covering node must reach b's. Where it
    // can't, a and b become nodes, joined by forced links: covering node → a (it
    // covered a by a straight walk) and a → b (the lattice link itself).
    let mut forced: Vec<(usize, usize, LinkKind)> = Vec::new(); // sample → sample
    let mut iterations = 0;
    let mut node_links;
    let mut unresolved;
    loop {
        iterations += 1;
        node_links = node_links_by_walking(level, &samples, &nodes, &links, p);
        let node_of: HashMap<usize, usize> = nodes.iter().enumerate().map(|(k, &smp)| (smp, k)).collect();
        for &(sa, sb, k) in &forced {
            if let (Some(&u), Some(&v)) = (node_of.get(&sa), node_of.get(&sb)) {
                if !node_links.iter().any(|&(x, y, _)| x == u && y == v) {
                    node_links.push((u, v, k));
                }
            }
        }
        let reach = closure(nodes.len(), &node_links);
        let mut seen: std::collections::HashSet<(usize, usize)> = Default::default();
        let mut add: Vec<usize> = Vec::new();
        for &(sa, sb, k) in &links {
            let (u, v) = (owner[sa], owner[sb]);
            if u == usize::MAX || v == usize::MAX || u == v || reach[u][v] || !seen.insert((u, v)) {
                continue;
            }
            for smp in [sa, sb] {
                if !node_of.contains_key(&smp) && !add.contains(&smp) {
                    add.push(smp);
                }
            }
            forced.push((nodes[u], sa, LinkKind::Walk));
            forced.push((sa, sb, k));
            forced.push((nodes[v], sb, LinkKind::Walk));
        }
        unresolved = seen.len();
        if add.is_empty() || iterations >= 12 {
            break;
        }
        for smp in add {
            owner[smp] = nodes.len();
            nodes.push(smp);
        }
        node_counts.push(nodes.len());
    }

    let graph = emit(level, &samples, &nodes, &node_links, &links, p);
    Generated { graph, samples, links, node_samples: nodes, node_links, iterations, node_counts, unresolved }
}

// ─── 1. Samples ──────────────────────────────────────────────────────────────

fn sample(level: &TileLevel, geom: &LevelGeom, p: &GenParams) -> (Vec<Sample>, HashMap<(i32, i32), Vec<usize>>) {
    let (lo, hi) = geom.bounds();
    let floors: Vec<usize> =
        (0..geom.polys.len()).filter(|&i| matches!(geom.polys[i].floor_kind(), Some(FloorKind::Flat | FloorKind::Ramp))).collect();
    let mut samples = Vec::new();
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let nx = ((hi.x - lo.x) / p.spacing).ceil() as i32;
    let nz = ((hi.z - lo.z) / p.spacing).ceil() as i32;
    for ix in 0..=nx {
        for iz in 0..=nz {
            let (x, z) = (lo.x + ix as f32 * p.spacing, lo.z + iz as f32 * p.spacing);
            let mut heights: Vec<(f32, Option<u16>)> = Vec::new();
            for &f in &floors {
                let poly = &geom.polys[f];
                if poly.xz_in_convex(x, z) {
                    let y = poly.find_y(x, z);
                    if !heights.iter().any(|h| (h.0 - y).abs() < 30.0) {
                        heights.push((y, poly.room));
                    }
                }
            }
            for (y, room) in heights {
                let floor = Vec3::new(x, y, z);
                // A chr standing here stands on this floor...
                let (g, _) = level.cd_find_ground_at_cyl(Vec3::new(x, y + p.step, z), p.radius);
                let (gh, _) = level.cd_find_ground_at_cyl(Vec3::new(x, y + p.step_high, z), p.radius);
                if (g - y).abs() > 2.0 || (gh - y).abs() > 2.0 {
                    continue;
                }
                // ...and fits, with the margin...
                let height = chr_height_at(level, floor, p.radius);
                if !fits(level, floor, p.radius + p.margin, height) {
                    continue;
                }
                if height < 185.0 && !p.crouch_zones {
                    continue;
                }
                // ...and a chr on a go-to there wouldn't be climbing a ladder.
                if level.cd_find_ladder(floor + Vec3::Y * ROOT, p.radius * 2.5 + PATH_MARGIN, height - ROOT, 1.0 - ROOT).is_some() {
                    continue;
                }
                let clearance = [50.0, 80.0, 120.0, 170.0, 230.0]
                    .into_iter()
                    .find(|&r| !fits(level, floor, r, height))
                    .unwrap_or(300.0);
                cells.entry((ix, iz)).or_default().push(samples.len());
                samples.push(Sample { pos: floor, room, crouch: height < 185.0, clearance });
            }
        }
    }
    (samples, cells)
}

/// Diagnostic: why is there no sample at `(x, z)`? One line per floor surface there.
pub fn explain_point(level: &TileLevel, x: f32, z: f32, p: &GenParams) -> Vec<String> {
    let geom = &level.geom;
    let mut out = Vec::new();
    for (i, poly) in geom.polys.iter().enumerate() {
        if !matches!(poly.floor_kind(), Some(FloorKind::Flat | FloorKind::Ramp)) || !poly.xz_in_convex(x, z) {
            continue;
        }
        let y = poly.find_y(x, z);
        let floor = Vec3::new(x, y, z);
        let (g, _) = level.cd_find_ground_at_cyl(Vec3::new(x, y + p.step, z), p.radius);
        let (gh, _) = level.cd_find_ground_at_cyl(Vec3::new(x, y + p.step_high, z), p.radius);
        let height = chr_height_at(level, floor, p.radius);
        let fit = fits(level, floor, p.radius + p.margin, height);
        let walls = level.walls_touching(floor + Vec3::Y * ROOT, p.radius + p.margin, height - ROOT, 20.0 - ROOT);
        let ladder = level.cd_find_ladder(floor + Vec3::Y * ROOT, p.radius * 2.5 + PATH_MARGIN, height - ROOT, 1.0 - ROOT).is_some();
        out.push(format!(
            "poly {i} ({:?}) y {y:.1}: ground {g:.1}, ground from +{} {gh:.1}, height {height}, fits {fit} (walls {walls:?}), ladder {ladder}",
            poly.floor_kind(),
            p.step_high
        ));
    }
    out
}

// ─── 2. Links ────────────────────────────────────────────────────────────────

fn lattice_links(level: &TileLevel, samples: &[Sample], cells: &HashMap<(i32, i32), Vec<usize>>, p: &GenParams) -> Vec<(usize, usize, LinkKind)> {
    let mut out = Vec::new();
    for (&(ix, iz), list) in cells {
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let Some(other) = cells.get(&(ix + dx, iz + dz)) else { continue };
                for &a in list {
                    for &b in other {
                        let dy = samples[b].pos.y - samples[a].pos.y;
                        if !(-700.0..=150.0).contains(&dy) {
                            continue;
                        }
                        if let Some(k) = probe_walk(level, samples[a].pos, samples[b].pos, p) {
                            out.push((a, b, k));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Each ladder (stacked ladder polygons in one plane merged) links a sample at
/// its foot to the nearest at its top (within 120 cm). The foot sample is the
/// nearest 55–100 cm from the ladder: just outside the 2.5-radius reach that makes a
/// go-to chr climb, so walking to it doesn't start a climb, and walking on from it
/// towards the top does.
fn ladder_links(level: &TileLevel, geom: &LevelGeom, samples: &[Sample]) -> Vec<(usize, usize, LinkKind)> {
    let _ = level;
    let mut ladders: Vec<(Vec2, f32, f32)> = Vec::new(); // (xz centre, bottom, top)
    for poly in geom.polys.iter().filter(|p| p.ladder) {
        let c = poly.verts.iter().copied().sum::<Vec3>() / poly.verts.len() as f32;
        let c2 = Vec2::new(c.x, c.z);
        let (lo, hi) = (poly.min_y(), poly.max_y());
        match ladders.iter_mut().find(|l| l.0.distance(c2) < 30.0 && (l.1 <= hi + 1.0 && lo <= l.2 + 1.0)) {
            Some(l) => {
                l.1 = l.1.min(lo);
                l.2 = l.2.max(hi);
            }
            None => ladders.push((c2, lo, hi)),
        }
    }
    let mut out = Vec::new();
    for (c, bottom, top) in ladders {
        let nearest = |y: f32, reach: f32| {
            samples
                .iter()
                .enumerate()
                .filter(|(_, s)| (s.pos.y - y).abs() < 40.0)
                .map(|(i, s)| (i, Vec2::new(s.pos.x, s.pos.z).distance(c)))
                .filter(|&(_, d)| d <= reach)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        let foot = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| (s.pos.y - bottom).abs() < 40.0)
            .map(|(i, s)| (i, Vec2::new(s.pos.x, s.pos.z).distance(c)))
            .filter(|&(_, d)| (55.0..=100.0).contains(&d))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        if let (Some(a), Some(b)) = (foot, nearest(top, 120.0)) {
            out.push((a, b, LinkKind::Climb));
        }
    }
    out
}

// ─── 3. Cover and regions ────────────────────────────────────────────────────

#[derive(PartialEq)]
struct Item(f32, usize);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// Lattice distances from `sources` up to `limit`.
fn dijkstra(adj: &[Vec<(usize, f32)>], sources: &[usize], limit: f32) -> Vec<(f32, usize)> {
    let mut dist = vec![(f32::INFINITY, usize::MAX); adj.len()];
    let mut heap = BinaryHeap::new();
    for (k, &s) in sources.iter().enumerate() {
        dist[s] = (0.0, k);
        heap.push(Item(0.0, s));
    }
    while let Some(Item(d, u)) = heap.pop() {
        if d > dist[u].0 {
            continue;
        }
        for &(v, w) in &adj[u] {
            let nd = d + w;
            if nd <= limit && nd < dist[v].0 {
                dist[v] = (nd, dist[u].1);
                heap.push(Item(nd, v));
            }
        }
    }
    dist
}

fn greedy_cover(level: &TileLevel, samples: &[Sample], adj: &[Vec<(usize, f32)>], nodes: &mut Vec<usize>, owner: &mut [usize], p: &GenParams) {
    // A node covers the samples within `cover` along the lattice that a chr can
    // walk to from it in a straight line (the probe, not just sight: a railing on
    // a ramp can hang above a sight line at pad height but not above a chr).
    let pad = |s: usize| samples[s].pos + Vec3::Y * 53.0;
    let mut covered = vec![false; samples.len()];
    let cover_from = |s: usize, k: usize, covered: &mut Vec<bool>, owner: &mut [usize]| {
        for (t, &(d, _)) in dijkstra(adj, &[s], p.cover).iter().enumerate() {
            if d.is_finite()
                && !covered[t]
                && level.los_autoflags(pad(s), pad(t))
                && probe_walk(level, samples[s].pos, samples[t].pos, p) == Some(LinkKind::Walk)
            {
                covered[t] = true;
                owner[t] = k;
            }
        }
        covered[s] = true;
        owner[s] = k;
    };
    for (k, &s) in nodes.clone().iter().enumerate() {
        cover_from(s, k, &mut covered, owner);
    }
    let mut order: Vec<usize> = (0..samples.len()).collect();
    order.sort_by(|&a, &b| samples[b].clearance.total_cmp(&samples[a].clearance).then(a.cmp(&b)));
    for s in order {
        if !covered[s] {
            nodes.push(s);
            cover_from(s, nodes.len() - 1, &mut covered, owner);
        }
    }
}

/// Node links: every pair of nodes within 1.8x the cover radius that can see
/// each other is probed both ways (walk or drop), ladder climbs come from the
/// lattice, and then a walk link is dropped when a two-link detour through
/// another node is at most `prune` times longer, which keeps the graph PD-sparse.
fn node_links_by_walking(
    level: &TileLevel,
    samples: &[Sample],
    nodes: &[usize],
    lattice: &[(usize, usize, LinkKind)],
    p: &GenParams,
) -> Vec<(usize, usize, LinkKind)> {
    let n = nodes.len();
    let pos = |k: usize| samples[nodes[k]].pos;
    let reach = p.cover * 1.8;
    let mut links: HashMap<(usize, usize), LinkKind> = HashMap::new();
    for u in 0..n {
        for v in (u + 1)..n {
            let (a, b) = (pos(u), pos(v));
            if Vec2::new(a.x - b.x, a.z - b.z).length() > reach || (a.y - b.y).abs() > 700.0 {
                continue;
            }
            if !level.los_autoflags(a + Vec3::Y * 53.0, b + Vec3::Y * 53.0) {
                continue;
            }
            if let Some(k) = probe_walk(level, a, b, p) {
                links.insert((u, v), k);
            }
            if let Some(k) = probe_walk(level, b, a, p) {
                links.insert((v, u), k);
            }
        }
    }
    let node_of: HashMap<usize, usize> = nodes.iter().enumerate().map(|(k, &s)| (s, k)).collect();
    for &(sa, sb, k) in lattice {
        if let (LinkKind::Climb, Some(&u), Some(&v)) = (k, node_of.get(&sa), node_of.get(&sb)) {
            links.insert((u, v), LinkKind::Climb);
        }
    }
    // Prune walks a detour almost matches.
    let len = |u: usize, v: usize| pos(u).distance(pos(v));
    let mut out_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(u, v) in links.keys() {
        out_of[u].push(v);
    }
    let mut keep = Vec::new();
    for (&(u, v), &k) in &links {
        let redundant = k == LinkKind::Walk
            && out_of[u].iter().any(|&w| {
                w != v
                    && links.get(&(u, w)) == Some(&LinkKind::Walk)
                    && links.get(&(w, v)) == Some(&LinkKind::Walk)
                    && len(u, w) + len(w, v) <= p.prune * len(u, v)
            });
        if !redundant {
            keep.push((u, v, k));
        }
    }
    keep.sort();
    keep
}

/// `chr_prop_can_move_to_pos_without_nav` from a chr standing on `floor` to the
/// pad at `topos`, as the go-to skip-ahead calls it (turn distance 1.2 radii):
/// three swept lines against walls only.
fn wall_skip_ok(level: &TileLevel, floor: Vec3, topos: Vec3, p: &GenParams) -> bool {
    let from = floor + Vec3::Y * ROOT;
    let (ymax, ymin) = (185.0 - ROOT, 20.0 - ROOT);
    if level.cd_test_cylmove_oobok(from, topos, ymax, ymin, &[]) == CdResult::Collision {
        return false;
    }
    let d = Vec2::new(topos.x - from.x, topos.z - from.z);
    if d == Vec2::ZERO {
        return true;
    }
    let d = d.normalize() * (p.radius * 1.2);
    for side in [1.0f32, -1.0] {
        let nf = Vec3::new(from.x + d.y * side, from.y, from.z - d.x * side);
        let nt = Vec3::new(topos.x + d.y * side, topos.y, topos.z - d.x * side);
        if level.cd_test_cylmove_oobok(from, nf, ymax, ymin, &[]) == CdResult::Collision
            || level.cd_test_cylmove_oobok(nf, nt, ymax, ymin, &[]) == CdResult::Collision
        {
            return false;
        }
    }
    true
}

/// `reach[u][v]`: can node `u` get to node `v` along the links?
fn closure(n: usize, links: &[(usize, usize, LinkKind)]) -> Vec<Vec<bool>> {
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(u, v, _) in links {
        out[u].push(v);
    }
    (0..n)
        .map(|s| {
            let mut seen = vec![false; n];
            let mut stack = vec![s];
            seen[s] = true;
            while let Some(u) = stack.pop() {
                for &v in &out[u] {
                    if !seen[v] {
                        seen[v] = true;
                        stack.push(v);
                    }
                }
            }
            seen
        })
        .collect()
}

// ─── 5. PD format ────────────────────────────────────────────────────────────

fn emit(
    level: &TileLevel,
    samples: &[Sample],
    nodes: &[usize],
    node_links: &[(usize, usize, LinkKind)],
    lattice: &[(usize, usize, LinkKind)],
    p: &GenParams,
) -> NavGraph {
    let n = nodes.len();
    // Directed reachability between nodes.
    let mut fwd: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(u, v, _) in node_links {
        if !fwd[u].contains(&v) {
            fwd[u].push(v);
        }
    }
    // `PADFLAG_AIWALKDIRECT`: every 10 ticks `chr_tick_gopos` skips one or two
    // waypoints ahead when `chr_prop_can_move_to_pos_without_nav` says the way is
    // clear, and that test sees walls only: not drops, gaps, ladders, low ceilings
    // or surfaces a tall bot would step onto. PD's designers flag the pads where
    // skipping would be unsafe. We flag a node when bypassing it — from a node
    // before it to one or two after — passes that walls-only test but fails the
    // probe: exactly where the skip test would be fooled. Ends of drops and
    // climbs are flagged too.
    let pos = |k: usize| samples[nodes[k]].pos;
    let skip_fooled = |u: usize, v: usize| {
        u != v && wall_skip_ok(level, pos(u), pos(v) + Vec3::Y * 53.0, p) && probe_walk(level, pos(u), pos(v), p).is_none()
    };
    let mut back: Vec<Vec<usize>> = vec![Vec::new(); n];
    for u in 0..n {
        for &v in &fwd[u] {
            back[v].push(u);
        }
    }
    let mut walkdirect = vec![false; n];
    for &(u, v, k) in node_links {
        if k != LinkKind::Walk {
            walkdirect[u] = true;
            walkdirect[v] = true;
        }
    }
    for w in 0..n {
        if walkdirect[w] {
            continue;
        }
        'found: for &u in &back[w] {
            for &x in &fwd[w] {
                if skip_fooled(u, x) {
                    walkdirect[w] = true;
                    break 'found;
                }
                for &y in &fwd[x] {
                    if skip_fooled(u, y) {
                        walkdirect[w] = true;
                        break 'found;
                    }
                }
            }
        }
    }
    let _ = lattice;

    let pads: Vec<NavPad> = nodes
        .iter()
        .enumerate()
        .map(|(k, &s)| NavPad {
            pos: samples[s].pos + Vec3::Y * 53.0,
            room: samples[s].room,
            flags: PadFlags {
                walkdirect: walkdirect[k],
                crouch: samples[s].crouch,
                duck: false,
            },
        })
        .collect();

    // Neighbour lists with PD's direction flags: a one-way u → v is OUTWARDSONLY in
    // u's list and INWARDSONLY in v's.
    let mut neighbours: Vec<Vec<i32>> = vec![Vec::new(); n];
    for u in 0..n {
        for &v in &fwd[u] {
            let back = fwd[v].contains(&u);
            if back {
                if u < v {
                    neighbours[u].push(v as i32);
                    neighbours[v].push(u as i32);
                }
            } else {
                neighbours[u].push(v as i32 | WPSEGFLAG_OUTWARDSONLY);
                neighbours[v].push(u as i32 | WPSEGFLAG_INWARDSONLY);
            }
        }
    }

    let groups = group_nodes(&pads, &fwd, p.group_radius);
    let mut groupnum = vec![usize::MAX; n];
    for (g, members) in groups.iter().enumerate() {
        for &k in members {
            groupnum[k] = g;
        }
    }
    let mut group_fwd: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
    for u in 0..n {
        for &v in &fwd[u] {
            let (gu, gv) = (groupnum[u], groupnum[v]);
            if gu != gv && !group_fwd[gu].contains(&gv) {
                group_fwd[gu].push(gv);
            }
        }
    }
    let mut group_neighbours: Vec<Vec<i32>> = vec![Vec::new(); groups.len()];
    for gu in 0..groups.len() {
        for &gv in &group_fwd[gu] {
            if group_fwd[gv].contains(&gu) {
                if gu < gv {
                    group_neighbours[gu].push(gv as i32);
                    group_neighbours[gv].push(gu as i32);
                }
            } else {
                group_neighbours[gu].push(gv as i32 | WPSEGFLAG_OUTWARDSONLY);
                group_neighbours[gv].push(gu as i32 | WPSEGFLAG_INWARDSONLY);
            }
        }
    }
    let waypoints = (0..n).map(|k| NavWaypoint { padnum: k, neighbours: neighbours[k].clone(), groupnum: groupnum[k] }).collect();
    let waygroups = groups
        .into_iter()
        .enumerate()
        .map(|(g, mut w)| {
            w.sort();
            NavWaygroup { neighbours: group_neighbours[g].clone(), waypoints: w }
        })
        .collect();
    let _ = level;
    NavGraph::new(pads, waypoints, waygroups)
}

/// Waygroups. PD routes group by group first (fewest groups), then waypoint by
/// waypoint inside each group, so a group should be compact — then "fewest
/// groups" tracks distance — and must be strongly connected, or PD's in-group
/// search can't reach the group's exit. Each group grows from the lowest unassigned
/// node over **two-way** links (which makes it strongly connected), taking nodes
/// within `radius` of that seed. Rooms aren't needed.
fn group_nodes(pads: &[NavPad], fwd: &[Vec<usize>], radius: f32) -> Vec<Vec<usize>> {
    let n = pads.len();
    let mut group = vec![usize::MAX; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for seed in 0..n {
        if group[seed] != usize::MAX {
            continue;
        }
        let g = groups.len();
        let mut members = vec![seed];
        group[seed] = g;
        let mut queue = VecDeque::from([seed]);
        while let Some(u) = queue.pop_front() {
            for &v in &fwd[u] {
                if group[v] == usize::MAX && fwd[v].contains(&u) && pads[v].pos.distance(pads[seed].pos) <= radius {
                    group[v] = g;
                    members.push(v);
                    queue.push_back(v);
                }
            }
        }
        members.sort();
        groups.push(members);
    }
    groups
}
