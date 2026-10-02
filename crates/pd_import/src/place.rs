//! The gameplay data a PD arena carries hand-placed, generated for a
//! converted level on the stage as the game loads it:
//!
//! - **Waypoints**: `pd_sim::nav::gen` over the stage's collision (the
//!   generator that passed the Complex A/B), emitted in PD's own format and
//!   baked into `pads.json`, so the world routes on them exactly as it routes
//!   on an arena's own graph (ARCHITECTURE.md D8).
//! - Everything else stands on a waypoint of the graph's largest strongly
//!   connected part (a bot can get there and back), spread by farthest-point
//!   sampling, preferring the places the source game marks:
//!   - **spawns** (`intro[]`'s `spawn`): the source's player starts and NPCs,
//!     then the spots furthest from them, facing their most open direction;
//!   - **weapons** (`props[]`'s `weapon`, `WEAPON_MPLOCATION00 + n`) away
//!     from the spawns, the source's treasure on location 5 (the set's
//!     sixth slot, which only one location takes: `mp_get_mp_weapon_by_location`
//!     gives locations 0-5 the slots in turn, then wraps), each with two ammo
//!     crates (`ammocratemulti`) a few metres off, as PD's arenas pair them;
//!   - **hills** (King of the Hill: the hill is the pad's room) in the rooms
//!     most spread over the level;
//!   - **Capture the Case bases** (`case`, ids 0-3) as far apart as the level
//!     allows, each with six respawn pads (`case_respawn`) around it;
//!   - **cover** (`pads.json` `cover[]`, what simulants hold in a room) on
//!     waypoints with a sight-blocking wall at crouch height beside them,
//!     facing away from it.

use glam::Vec3;
use pd_core::ids::*;
use pd_sim::nav::gen::{generate, GenParams};
use pd_sim::nav::NavGraph;
use pd_sim::stage::{directed_links, Stage, TileLevel};
use serde_json::json;

use crate::layout::Layout;
use crate::recipe::Recipe;
use crate::source::{Marker, MarkerKind};
use crate::write::{pad_flags, Gameplay, PadRow};

/// PD's pads sit this far above their floor (waypoints and item pads alike).
const PAD_HEIGHT: f32 = 53.0;

/// The Combat Simulator's teams in Capture the Case (`ctc_get_max_teams`).
const CTC_TEAMS: usize = 4;
/// `case_respawn` pads per base, as PD's arenas have (24 for four bases).
const CTC_RESPAWNS: usize = 6;
/// How many cover spots at most (`cover_unpack` reads `covers[(u8)i]`).
const MAX_COVER: usize = 120;

pub struct Placed {
    pub gameplay: Gameplay,
    pub report: Vec<String>,
}

/// A waypoint the placement may use: its pad's position and room.
#[derive(Clone, Copy)]
struct Node {
    pos: Vec3,
    room: Option<u16>,
}

/// The strongly connected parts of a directed graph (Kosaraju), largest first.
fn strong_components(n: usize, links: &[(usize, usize, bool)]) -> Vec<Vec<usize>> {
    let mut fwd = vec![Vec::new(); n];
    let mut back = vec![Vec::new(); n];
    for &(a, b, _) in links {
        fwd[a].push(b);
        back[b].push(a);
    }
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    for s in 0..n {
        if seen[s] {
            continue;
        }
        let mut stack = vec![(s, 0usize)];
        seen[s] = true;
        while let Some(&mut (v, ref mut i)) = stack.last_mut() {
            if let Some(&w) = fwd[v].get(*i) {
                *i += 1;
                if !seen[w] {
                    seen[w] = true;
                    stack.push((w, 0));
                }
            } else {
                order.push(v);
                stack.pop();
            }
        }
    }
    let mut comp = vec![usize::MAX; n];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &s in order.iter().rev() {
        if comp[s] != usize::MAX {
            continue;
        }
        let c = out.len();
        let mut part = vec![s];
        comp[s] = c;
        let mut k = 0;
        while k < part.len() {
            let v = part[k];
            k += 1;
            for &w in &back[v] {
                if comp[w] == usize::MAX {
                    comp[w] = c;
                    part.push(w);
                }
            }
        }
        out.push(part);
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.len()));
    out
}

/// Farthest-point sampling: add to `chosen` the candidate furthest from
/// everything chosen (and from `keep_away`, which must stay `away` off),
/// until `count` are chosen or the best is nearer than `gap`.
fn farthest(cands: &[Vec3], chosen: &mut Vec<Vec3>, count: usize, gap: f32, keep_away: &[Vec3], away: f32) {
    while chosen.len() < count {
        let best = cands
            .iter()
            .filter(|c| keep_away.iter().all(|k| k.distance(**c) >= away))
            .map(|c| (*c, chosen.iter().map(|x| x.distance(*c)).fold(f32::INFINITY, f32::min)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((c, d)) if d >= gap => chosen.push(c),
            _ => break,
        }
    }
}

/// How far (of a few steps, cm) the view from `pos` (a pad) reaches facing `a`.
fn reach(level: &TileLevel, pos: Vec3, a: f32) -> f32 {
    let eye = pos + Vec3::Y * 100.0;
    let d = Vec3::new(a.sin(), 0.0, a.cos());
    [3000.0, 2000.0, 1200.0, 600.0, 300.0].into_iter().find(|&l| level.los(eye, eye + d * l)).unwrap_or(0.0)
}

/// The way from `pos` (a pad) that looks furthest before a wall, of eight.
fn open_facing(level: &TileLevel, pos: Vec3) -> f32 {
    let mut best = (0.0, 0.0);
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let r = reach(level, pos, a);
        if r > best.1 {
            best = (a, r);
        }
    }
    best.0
}

fn look(facing: f32) -> Vec3 {
    Vec3::new(facing.sin(), 0.0, facing.cos())
}

/// What placement makes of the kinds the layout leaves out.
pub enum How<'a> {
    /// Generated from the recipe's counts, preferring the spots the source
    /// marks.
    Generate(&'a [Marker]),
    /// The source's own rows kept (a PD arena rebuilt brings its setup), and
    /// cover added in these rooms (the ones the source added or changed).
    Keep(&'a [u16]),
}

/// The waypoint graph over the stage's collision (`nav::gen`), and a line
/// for the report.
pub fn generate_graph(level: &TileLevel) -> (NavGraph, String) {
    let t0 = std::time::Instant::now();
    let gen = generate(level, &GenParams::default());
    let line = format!(
        "waypoints: {} ({} waygroups) from {} floor samples in {:.1} s, {} validation rounds, {} links unresolved",
        gen.graph.waypoints.len(),
        gen.graph.waygroups.len(),
        gen.samples.len(),
        t0.elapsed().as_secs_f32(),
        gen.iterations,
        gen.unresolved
    );
    (gen.graph, line)
}

/// The waypoints placement may use: those of one strongly connected part of
/// the graph a chr stands up on: the largest, or (with `starts`) the one the
/// most of them are nearest a waypoint of (the largest on a tie or none).
fn usable(graph: &NavGraph, starts: Option<&[Vec3]>, report: &mut Vec<String>) -> Result<Vec<Node>, String> {
    let n = graph.waypoints.len();
    if n == 0 {
        return Err("the generator found no floor to stand on".into());
    }
    let links = directed_links(n, |w| &graph.waypoints[w].neighbours);
    let comps = strong_components(n, &links);
    let mut pick = 0;
    if let Some(starts) = starts {
        let mut part = vec![0; n];
        for (c, comp) in comps.iter().enumerate() {
            for &w in comp {
                part[w] = c;
            }
        }
        let mut votes = vec![0usize; comps.len()];
        for s in starts {
            let near = (0..n).map(|w| (w, graph.waypoint_pos(w))).filter(|(_, p)| (p.y - PAD_HEIGHT - s.y).abs() < 150.0).min_by(|a, b| a.1.distance(*s).total_cmp(&b.1.distance(*s)));
            if let Some((w, p)) = near {
                if (p.x - s.x).hypot(p.z - s.z) < 300.0 {
                    votes[part[w]] += 1;
                }
            }
        }
        // The most votes; the earlier (larger) part on a tie.
        pick = (0..comps.len()).fold(0, |best, c| if votes[c] > votes[best] { c } else { best });
    }
    let main = &comps[pick];
    report.push(format!(
        "reachable: {} of {} waypoints in the {} strongly connected part; the other parts: {:?}",
        main.len(),
        n,
        if starts.is_some() { "player starts'" } else { "largest" },
        comps.iter().enumerate().filter(|&(c, _)| c != pick).take(8).map(|(_, c)| c.len()).collect::<Vec<_>>()
    ));
    Ok(main
        .iter()
        .filter(|&&w| {
            let f = graph.waypoint_flags(w);
            !f.crouch && !f.duck
        })
        .map(|&w| Node { pos: graph.waypoint_pos(w), room: graph.waypoint_room(w) })
        .collect())
}

/// Each waypoint's strongly connected part, 0 the largest (the one placement
/// uses), so the editor can show the rest.
pub fn waypoint_parts(graph: &NavGraph) -> Vec<usize> {
    let n = graph.waypoints.len();
    let links = directed_links(n, |w| &graph.waypoints[w].neighbours);
    let mut part = vec![0; n];
    for (c, comp) in strong_components(n, &links).iter().enumerate() {
        for &w in comp {
            part[w] = c;
        }
    }
    part
}

/// Cover on `cands` (waypoints with a sight-blocking wall at crouch height
/// beside them, facing away from it), spread out, after `have` (up to
/// [`MAX_COVER`] in all).
fn cover(level: &TileLevel, cands: &[Vec3], have: &[(Vec3, Vec3)]) -> Vec<(Vec3, Vec3)> {
    let mut cover_spots: Vec<(Vec3, Vec3)> = Vec::new();
    for c in cands {
        let eye = *c - Vec3::Y * PAD_HEIGHT + Vec3::Y * 60.0;
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let d = Vec3::new(a.sin(), 0.0, a.cos());
            if !level.los(eye, eye + d * 90.0) && level.los(eye, eye - d * 400.0) {
                cover_spots.push((*c, -d));
                break;
            }
        }
    }
    let spots: Vec<Vec3> = cover_spots.iter().map(|c| c.0).collect();
    let mut chosen: Vec<Vec3> = have.iter().map(|c| c.0).collect();
    farthest(&spots, &mut chosen, MAX_COVER, 300.0, &[], 0.0);
    chosen[have.len()..].iter().map(|p| *cover_spots.iter().find(|c| c.0 == *p).unwrap()).collect()
}

/// The graph's pads after `g`'s, and its waypoints and waygroups on them.
fn emit_graph(g: &mut Gameplay, graph: &NavGraph) {
    let base = g.pads.len();
    for p in &graph.pads {
        g.pads.push(PadRow::at(p.pos, Vec3::Z, pad_flags(&p.flags)));
    }
    g.waypoints = graph.waypoints.iter().map(|w| (base + w.padnum, w.groupnum, w.neighbours.clone())).collect();
    g.waygroups = graph.waygroups.iter().map(|wg| wg.neighbours.clone()).collect();
}

/// Take the source's rows of the kinds the layout places out of `g` (their
/// pads stay, unused, so the other rows keep their numbers). A layout's
/// weapons take the ammo crates with them: a crate belongs to the weapon row
/// before it.
fn strip(g: &mut Gameplay, l: &Layout) {
    let takes = |t: &str| match t {
        "spawn" => l.spawns.is_some(),
        "hill" => l.hills.is_some(),
        "case" => l.bases.is_some(),
        "case_respawn" => l.respawns.is_some(),
        _ => false,
    };
    g.intro.retain(|v| !takes(v["type"].as_str().unwrap_or("")));
    if l.weapons.is_some() {
        g.props.retain(|v| !matches!(v["type"].as_str(), Some("weapon" | "ammocratemulti")));
    }
    if l.cover.is_some() {
        g.cover.clear();
    }
    if l.doors.is_some() {
        g.props.retain(|v| v["type"] != "door");
    }
}

/// The pads the rows of `g` stand on.
fn used_pads(g: &Gameplay) -> Vec<usize> {
    let mut used: Vec<usize> = g.intro.iter().chain(&g.props).flat_map(|v| ["pad", "chr"].map(|k| v[k].as_u64())).flatten().map(|p| p as usize).collect();
    used.sort();
    used.dedup();
    used
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::from(p)
}

/// Place the gameplay data on `stage`, after what the source brings in
/// `fixed` (its pads and props keep their numbers): every kind `layout` holds
/// exactly as it holds it, the rest as `how` says; then `graph`'s waypoints.
/// `models`: `MODEL_*` by stem, for the layout's doors.
#[allow(clippy::too_many_arguments)]
pub fn place(r: &Recipe, stage: &Stage, level: &TileLevel, graph: &NavGraph, fixed: Gameplay, layout: &Layout, how: &How, models: &std::collections::HashMap<String, i32>) -> Result<Placed, String> {
    let mut report = Vec::new();
    let starts: Option<Vec<Vec3>> = match how {
        How::Generate(m) if r.play_area == crate::recipe::PlayArea::Starts => Some(m.iter().filter(|m| m.kind == MarkerKind::Spawn).map(|m| m.pos).collect()),
        _ => None,
    };
    let nodes = usable(graph, starts.as_deref(), &mut report)?;
    let cands: Vec<Vec3> = nodes.iter().map(|n| n.pos).collect();
    let (gen, markers): (bool, &[Marker]) = match how {
        How::Generate(m) => (true, m),
        How::Keep(_) => (false, &[]),
    };
    let mut g = fixed;
    strip(&mut g, layout);
    let tag = |explicit: bool| if explicit { " (the layout's)" } else { "" };
    // A source marker as the nearest usable waypoint, if one is close.
    let snap = |p: Vec3| -> Option<Vec3> {
        cands
            .iter()
            .filter(|c| (c.y - PAD_HEIGHT - p.y).abs() < 150.0)
            .map(|c| (*c, (c.x - p.x).hypot(c.z - p.z)))
            .filter(|(_, d)| *d < 300.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(c, _)| c)
    };
    // A spawn stands on a floor (what `check` asks of a spawn pad): not on a
    // waypoint at a ledge's edge with the drop under its cylinder.
    let stands = |c: &Vec3| {
        let (y, poly) = level.cd_find_ground_at_cyl(*c, 30.0);
        poly.is_some() && c.y >= y && c.y - y <= 150.0
    };
    let spawn_cands: Vec<Vec3> = cands.iter().copied().filter(stands).collect();
    // At most this many of each on the source's spots (`Recipe::marker_share`).
    let share = |n: usize| (n as f32 * r.marker_share.clamp(0.0, 1.0)).round() as usize;
    // Where the source's own rows stand (kept ones): new items keep off them.
    let kept: Vec<Vec3> = used_pads(&g).iter().filter_map(|&p| g.pads.get(p).map(|x| x.pos)).collect();

    // ── Spawns: (pad position, facing in radians) ──────────────────────────
    // Not on the source's treasures: those are for the weapons.
    let prizes: Vec<Vec3> = markers.iter().filter(|m| m.kind == MarkerKind::Prize).filter_map(|m| snap(m.pos)).collect();
    let spawns: Vec<(Vec3, f32)> = match &layout.spawns {
        Some(v) => v.iter().map(|s| (v3(s.pos), s.facing.to_radians())).collect(),
        None if gen => {
            let mut spawns: Vec<Vec3> = Vec::new();
            let mut spawn_facing: Vec<Option<f32>> = Vec::new();
            for kind in [MarkerKind::Spawn, MarkerKind::Person] {
                for m in markers.iter().filter(|m| m.kind == kind) {
                    if let Some(p) = snap(m.pos).filter(stands) {
                        if spawns.len() < share(r.spawns) && spawns.iter().chain(&prizes).all(|s| s.distance(p) >= 500.0) {
                            spawns.push(p);
                            spawn_facing.push(Some(m.facing));
                        }
                    }
                }
            }
            let seeded = spawns.len();
            farthest(&spawn_cands, &mut spawns, r.spawns, 500.0, &prizes, 500.0);
            spawn_facing.resize(spawns.len(), None);
            report.push(format!("spawns: {} ({} at the source's starts and people)", spawns.len(), seeded));
            // The source's facing, unless a wall stands in it (a marker snapped
            // to a waypoint beside where the source put it can face one).
            spawns.iter().zip(spawn_facing).map(|(s, f)| (*s, f.filter(|&f| reach(level, *s, f) >= 300.0).unwrap_or_else(|| open_facing(level, *s)))).collect()
        }
        None => Vec::new(),
    };
    if layout.spawns.is_some() {
        report.push(format!("spawns: {}{}", spawns.len(), tag(true)));
    }
    let spawn_pos: Vec<Vec3> = spawns.iter().map(|s| s.0).chain(stage.spawn_pads.iter().filter_map(|&p| g.pads.get(p)).map(|p| p.pos)).collect();

    // ── Weapons (position, MP location) and their ammo (position, weapon) ──
    let weapons: Vec<(Vec3, u8)> = match &layout.weapons {
        Some(v) => v.iter().map(|w| (v3(w.pos), w.location)).collect(),
        None if gen => {
            let mut weapons: Vec<Vec3> = Vec::new();
            let mut prize = None;
            for kind in [MarkerKind::Prize, MarkerKind::Item] {
                for m in markers.iter().filter(|m| m.kind == kind) {
                    if let Some(p) = snap(m.pos) {
                        if weapons.len() < share(r.weapons) && weapons.iter().all(|w| w.distance(p) >= 700.0) && spawn_pos.iter().all(|s| s.distance(p) >= 500.0) {
                            if kind == MarkerKind::Prize && prize.is_none() {
                                prize = Some(weapons.len());
                            }
                            weapons.push(p);
                        }
                    }
                }
            }
            let seeded = weapons.len();
            farthest(&cands, &mut weapons, r.weapons, 600.0, &spawn_pos, 500.0);
            if let Some(i) = prize {
                let slot = 5.min(weapons.len() - 1);
                weapons.swap(i, slot);
            }
            report.push(format!("weapons: {} locations ({} at the source's pickups)", weapons.len(), seeded));
            weapons.into_iter().enumerate().map(|(i, w)| (w, i as u8)).collect()
        }
        None => Vec::new(),
    };
    if layout.weapons.is_some() {
        report.push(format!("weapons: {} locations{}", weapons.len(), tag(true)));
    }
    let ammo: Vec<(Vec3, usize)> = match &layout.ammo {
        Some(v) => v.iter().map(|a| (v3(a.pos), a.weapon)).collect(),
        None => {
            // Two crates a few metres off each weapon placed here, as PD's
            // arenas pair them.
            let mut placed: Vec<Vec3> = spawn_pos.iter().chain(&kept).copied().chain(weapons.iter().map(|w| w.0)).collect();
            let mut ammo = Vec::new();
            for (i, (w, _)) in weapons.iter().enumerate() {
                let ring = |lo: f32, hi: f32| -> Vec<Vec3> { cands.iter().copied().filter(|c| (lo..=hi).contains(&c.distance(*w))).collect() };
                let mut pair: Vec<Vec3> = vec![*w];
                for (lo, hi) in [(250.0, 700.0), (150.0, 1100.0)] {
                    if pair.len() < 3 {
                        farthest(&ring(lo, hi), &mut pair, 3, 150.0, &placed, 200.0);
                    }
                }
                if pair.len() < 3 {
                    report.push(format!("ammo: room for {} of 2 crates near the weapon at {w}", pair.len() - 1));
                }
                placed.extend(&pair[1..]);
                ammo.extend(pair[1..].iter().map(|&a| (a, i)));
            }
            ammo
        }
    };
    if !weapons.is_empty() || layout.ammo.is_some() {
        report.push(format!("ammo: {} crates{}", ammo.len(), tag(layout.ammo.is_some())));
    }

    // ── Hills: the most spread-out rooms with room to stand in ─────────────
    let hills: Vec<Vec3> = match &layout.hills {
        Some(v) => v.iter().map(|h| v3(h.pos)).collect(),
        None if gen => {
            let nrooms = stage.rooms.roomcount();
            let mut room_nodes: Vec<Vec<Vec3>> = vec![Vec::new(); nrooms];
            for n in &nodes {
                if let Some(rm) = n.room.filter(|&rm| (rm as usize) < nrooms) {
                    room_nodes[rm as usize].push(n.pos);
                }
            }
            let room_centre = |rm: usize| room_nodes[rm].iter().copied().sum::<Vec3>() / room_nodes[rm].len() as f32;
            let hill_rooms: Vec<usize> = (1..nrooms).filter(|&rm| room_nodes[rm].len() >= 8).collect();
            let centres: Vec<Vec3> = hill_rooms.iter().map(|&rm| room_centre(rm)).collect();
            let mut hill_centres = Vec::new();
            farthest(&centres, &mut hill_centres, r.hills, 0.0, &[], 0.0);
            hill_centres
                .iter()
                .map(|c| {
                    let rm = hill_rooms[centres.iter().position(|x| x == c).unwrap()];
                    *room_nodes[rm].iter().min_by(|a, b| a.distance(*c).total_cmp(&b.distance(*c))).unwrap()
                })
                .collect()
        }
        None => Vec::new(),
    };
    if gen || layout.hills.is_some() {
        report.push(format!("hills: {} in rooms {:?}{}", hills.len(), hills.iter().map(|h| stage.rooms.bg_find_rooms_by_pos(*h, 4).0).collect::<Vec<_>>(), tag(layout.hills.is_some())));
    }

    // ── Capture the Case: bases far apart, six respawns around each ────────
    let bases: Vec<(Vec3, f32, u8)> = match &layout.bases {
        Some(v) => v.iter().map(|b| (v3(b.pos), b.facing.to_radians(), b.team)).collect(),
        None if gen => {
            let centre = cands.iter().copied().sum::<Vec3>() / cands.len() as f32;
            let first = *cands.iter().max_by(|a, b| a.distance(centre).total_cmp(&b.distance(centre))).unwrap();
            let mut bases = vec![first];
            farthest(&cands, &mut bases, CTC_TEAMS, 0.0, &[], 0.0);
            bases.iter().enumerate().map(|(i, b)| (*b, open_facing(level, *b), i as u8)).collect()
        }
        None => Vec::new(),
    };
    let respawns: Vec<(Vec3, f32, u8)> = match &layout.respawns {
        Some(v) => v.iter().map(|s| (v3(s.pos), s.facing.to_radians(), s.team)).collect(),
        None if gen || layout.bases.is_some() => {
            let mut out = Vec::new();
            for (i, (b, _, team)) in bases.iter().enumerate() {
                let near: Vec<Vec3> = cands
                    .iter()
                    .copied()
                    .filter(|c| c.distance(*b) < 1500.0 && c.distance(*b) >= 150.0 && bases.iter().enumerate().all(|(j, o)| j == i || c.distance(*b) < c.distance(o.0)))
                    .collect();
                let mut chosen = vec![*b];
                farthest(&near, &mut chosen, CTC_RESPAWNS + 1, 150.0, &[], 0.0);
                if chosen.len() < CTC_RESPAWNS + 1 {
                    return Err(format!("base {i} at {b}: only {} respawn spots near it", chosen.len() - 1));
                }
                out.extend(chosen[1..].iter().map(|s| (*s, open_facing(level, *s), *team)));
            }
            out
        }
        None => Vec::new(),
    };
    if !bases.is_empty() || layout.respawns.is_some() {
        report.push(format!(
            "capture the case: bases at {:?}{}, {} respawn pads{}",
            bases.iter().map(|b| b.0.round()).collect::<Vec<_>>(),
            tag(layout.bases.is_some()),
            respawns.len(),
            tag(layout.respawns.is_some())
        ));
    }

    // ── Cover: waypoints with a wall at crouch height beside them ──────────
    match (&layout.cover, how) {
        (Some(v), _) => {
            g.cover = v.iter().map(|c| (v3(c.pos), look(c.facing.to_radians()))).collect();
            report.push(format!("cover: {} spots{}", g.cover.len(), tag(true)));
        }
        (None, How::Generate(_)) => {
            g.cover = cover(level, &cands, &[]);
            report.push(format!("cover: {} spots", g.cover.len()));
        }
        (None, How::Keep(rooms)) => {
            let per_room: Vec<String> = rooms.iter().map(|&rm| format!("{rm:#x}: {}", nodes.iter().filter(|n| n.room == Some(rm)).count())).collect();
            report.push(format!("reachable waypoints in the new and changed rooms: {}", per_room.join(", ")));
            let in_rooms: Vec<Vec3> = nodes.iter().filter(|n| n.room.is_some_and(|rm| rooms.contains(&rm))).map(|n| n.pos).collect();
            let added = cover(level, &in_rooms, &g.cover);
            report.push(format!("cover: {} of the source's, {} added in the new and changed rooms", g.cover.len(), added.len()));
            g.cover.extend(added);
        }
    }

    // ── The pads and the setup ─────────────────────────────────────────────
    let mut pad = |pos: Vec3, look: Vec3| -> usize {
        g.pads.push(PadRow::at(pos, look, 0));
        g.pads.len() - 1
    };
    let mut intro = Vec::new();
    for (s, f) in &spawns {
        intro.push(json!({"type": "spawn", "pad": pad(*s, look(*f))}));
    }
    for (b, f, team) in &bases {
        intro.push(json!({"type": "case", "id": team, "pad": pad(*b, look(*f))}));
    }
    for (s, f, team) in &respawns {
        intro.push(json!({"type": "case_respawn", "id": team, "pad": pad(*s, look(*f))}));
    }
    for h in &hills {
        intro.push(json!({"type": "hill", "pad": pad(*h, Vec3::X)}));
    }
    let mut props = Vec::new();
    for (i, (w, loc)) in weapons.iter().enumerate() {
        props.push(json!({"type": "weapon", "scale": 512, "model": 0, "chr": pad(*w, Vec3::Z), "flags": OBJFLAG_FALL, "flags2": 0, "flags3": 0, "weapon": WEAPON_MPLOCATION00 as usize + *loc as usize}));
        for (a, _) in ammo.iter().filter(|a| a.1 == i) {
            props.push(json!({"type": "ammocratemulti", "scale": 153, "model": MODEL_MULTI_AMMO_CRATE, "pad": pad(*a, Vec3::Z), "flags": OBJFLAG_FALL, "flags2": 0, "flags3": 0, "maxdamage": 1000}));
        }
    }
    // ── Doors: each on its own upright pad, its ring's siblings relative ──
    if let Some(doors) = &layout.doors {
        let first = g.props.len() + props.len();
        let (mut with_portal, mut too_big) = (0, Vec::new());
        for (i, d) in doors.iter().enumerate() {
            let model = *models.get(&d.model).ok_or_else(|| format!("door {i}: no model {:?} (GoldenEye's doors need extracting first: pd_import --ge-doors)", d.model))?;
            // Its sibling ring, for the portal test: a double door's leaves
            // share their doorway's portal.
            let mut ring = vec![d];
            let mut next = d.sibling;
            while let Some(s) = next.filter(|&s| s != i && ring.len() < doors.len()) {
                ring.push(&doors[s]);
                next = doors[s].sibling;
            }
            let portal = crate::doors::portal_for(&stage.rooms, d, &ring, 40.0);
            if portal.is_some() {
                with_portal += 1;
            } else if crate::doors::portal_for(&stage.rooms, d, &ring, f32::INFINITY).is_some() {
                too_big.push(i);
            }
            let pad = crate::doors::pad_of(d);
            g.pads.push(pad);
            let sibling = d.sibling.map_or(0, |s| s as i32 - i as i32);
            props.push(crate::doors::row_of(d, model, g.pads.len() - 1, portal.is_some(), sibling));
        }
        report.push(format!(
            "doors: {}{} ({with_portal} closing their doorway's portal; {} beside a portal bigger than their doorway, left open: {too_big:?}), rows from {first}",
            doors.len(),
            tag(true),
            too_big.len()
        ));
    }
    if !g.intro.iter().any(|v| v["type"] == "outfit") {
        intro.push(json!({"type": "outfit", "outfit": 0}));
    }
    g.intro.extend(intro);
    g.props.extend(props);
    if !g.intro.iter().any(|v| v["type"] == "spawn") {
        return Err("no spawns (the layout's spawns are empty, or the recipe asks for none)".into());
    }
    let used = used_pads(&g);
    let far: Vec<usize> = used.iter().copied().filter(|&p| g.pads.get(p).is_some_and(|pad| nodes.iter().all(|n| n.pos.distance(pad.pos) > 300.0))).collect();
    report.push(format!("the setup: {} pads, {} of them over 3 m from a reachable waypoint {far:?}", used.len(), far.len()));
    emit_graph(&mut g, graph);
    Ok(Placed { gameplay: g, report })
}
