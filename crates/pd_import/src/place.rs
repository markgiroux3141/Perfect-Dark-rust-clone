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

/// Place the gameplay data on `stage`, preferring `markers`, after what the
/// source brings in `fixed` (its pads and props keep their numbers).
pub fn place(r: &Recipe, markers: &[Marker], stage: &Stage, level: &TileLevel, fixed: Gameplay) -> Result<Placed, String> {
    let mut report = Vec::new();
    let t0 = std::time::Instant::now();
    let gen = generate(level, &GenParams::default());
    let graph: NavGraph = gen.graph;
    report.push(format!(
        "waypoints: {} ({} waygroups) from {} floor samples in {:.1} s, {} validation rounds, {} links unresolved",
        graph.waypoints.len(),
        graph.waygroups.len(),
        gen.samples.len(),
        t0.elapsed().as_secs_f32(),
        gen.iterations,
        gen.unresolved
    ));
    let n = graph.waypoints.len();
    if n == 0 {
        return Err("the generator found no floor to stand on".into());
    }
    let links = directed_links(n, |w| &graph.waypoints[w].neighbours);
    let comps = strong_components(n, &links);
    let main = &comps[0];
    report.push(format!(
        "reachable: {} of {} waypoints in the largest strongly connected part; the other parts: {:?}",
        main.len(),
        n,
        comps.iter().skip(1).take(8).map(|c| c.len()).collect::<Vec<_>>()
    ));
    let nodes: Vec<Node> = main
        .iter()
        .filter(|&&w| {
            let f = graph.waypoint_flags(w);
            !f.crouch && !f.duck
        })
        .map(|&w| Node { pos: graph.waypoint_pos(w), room: graph.waypoint_room(w) })
        .collect();
    let cands: Vec<Vec3> = nodes.iter().map(|n| n.pos).collect();
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

    // ── Spawns ─────────────────────────────────────────────────────────────
    // Not on the source's treasures: those are for the weapons.
    let prizes: Vec<Vec3> = markers.iter().filter(|m| m.kind == MarkerKind::Prize).filter_map(|m| snap(m.pos)).collect();
    // At most this many of each on the source's spots (`Recipe::marker_share`).
    let share = |n: usize| (n as f32 * r.marker_share.clamp(0.0, 1.0)).round() as usize;
    let mut spawns: Vec<Vec3> = Vec::new();
    let mut spawn_facing: Vec<Option<f32>> = Vec::new();
    for kind in [MarkerKind::Spawn, MarkerKind::Person] {
        for m in markers.iter().filter(|m| m.kind == kind) {
            if let Some(p) = snap(m.pos) {
                if spawns.len() < share(r.spawns) && spawns.iter().chain(&prizes).all(|s| s.distance(p) >= 500.0) {
                    spawns.push(p);
                    spawn_facing.push(Some(m.facing));
                }
            }
        }
    }
    let seeded = spawns.len();
    farthest(&cands, &mut spawns, r.spawns, 500.0, &prizes, 500.0);
    spawn_facing.resize(spawns.len(), None);
    report.push(format!("spawns: {} ({} at the source's starts and people)", spawns.len(), seeded));

    // ── Weapons and their ammo ─────────────────────────────────────────────
    let mut weapons: Vec<Vec3> = Vec::new();
    let mut prize = None;
    for kind in [MarkerKind::Prize, MarkerKind::Item] {
        for m in markers.iter().filter(|m| m.kind == kind) {
            if let Some(p) = snap(m.pos) {
                if weapons.len() < share(r.weapons) && weapons.iter().all(|w| w.distance(p) >= 700.0) && spawns.iter().all(|s| s.distance(p) >= 500.0) {
                    if kind == MarkerKind::Prize && prize.is_none() {
                        prize = Some(weapons.len());
                    }
                    weapons.push(p);
                }
            }
        }
    }
    let seeded = weapons.len();
    farthest(&cands, &mut weapons, r.weapons, 600.0, &spawns, 500.0);
    if let Some(i) = prize {
        let slot = 5.min(weapons.len() - 1);
        weapons.swap(i, slot);
    }
    let mut placed: Vec<Vec3> = spawns.iter().chain(&weapons).copied().collect();
    let mut ammo: Vec<[Vec3; 2]> = Vec::new();
    for w in &weapons {
        let ring = |lo: f32, hi: f32| -> Vec<Vec3> { cands.iter().copied().filter(|c| (lo..=hi).contains(&c.distance(*w))).collect() };
        let mut pair: Vec<Vec3> = vec![*w];
        for (lo, hi) in [(250.0, 700.0), (150.0, 1100.0)] {
            if pair.len() < 3 {
                farthest(&ring(lo, hi), &mut pair, 3, 150.0, &placed, 200.0);
            }
        }
        if pair.len() < 3 {
            return Err(format!("no room for ammo crates near the weapon at {w}"));
        }
        placed.extend(&pair[1..]);
        ammo.push([pair[1], pair[2]]);
    }
    report.push(format!("weapons: {} locations ({} at the source's pickups), two ammo crates each", weapons.len(), seeded));

    // ── Hills: the most spread-out rooms with room to stand in ─────────────
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
    let hills: Vec<Vec3> = hill_centres
        .iter()
        .map(|c| {
            let rm = hill_rooms[centres.iter().position(|x| x == c).unwrap()];
            *room_nodes[rm].iter().min_by(|a, b| a.distance(*c).total_cmp(&b.distance(*c))).unwrap()
        })
        .collect();
    report.push(format!("hills: {} in rooms {:?}", hills.len(), hills.iter().map(|h| stage.rooms.bg_find_rooms_by_pos(*h, 4).0).collect::<Vec<_>>()));

    // ── Capture the Case: four bases far apart, six respawns each ──────────
    let centre = cands.iter().copied().sum::<Vec3>() / cands.len() as f32;
    let first = *cands.iter().max_by(|a, b| a.distance(centre).total_cmp(&b.distance(centre))).unwrap();
    let mut bases = vec![first];
    farthest(&cands, &mut bases, CTC_TEAMS, 0.0, &[], 0.0);
    let mut respawns: Vec<Vec<Vec3>> = Vec::new();
    for (i, b) in bases.iter().enumerate() {
        let near: Vec<Vec3> = cands
            .iter()
            .copied()
            .filter(|c| c.distance(*b) < 1500.0 && c.distance(*b) >= 150.0 && bases.iter().enumerate().all(|(j, o)| j == i || c.distance(*b) < c.distance(*o)))
            .collect();
        let mut chosen = vec![*b];
        farthest(&near, &mut chosen, CTC_RESPAWNS + 1, 150.0, &[], 0.0);
        if chosen.len() < CTC_RESPAWNS + 1 {
            return Err(format!("base {i} at {b}: only {} respawn spots near it", chosen.len() - 1));
        }
        respawns.push(chosen[1..].to_vec());
    }
    report.push(format!("capture the case: bases at {:?}", bases.iter().map(|b| b.round()).collect::<Vec<_>>()));

    // ── Cover: waypoints with a wall at crouch height beside them ──────────
    let mut cover_spots: Vec<(Vec3, Vec3)> = Vec::new();
    for c in &cands {
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
    let mut chosen = Vec::new();
    farthest(&spots, &mut chosen, MAX_COVER, 300.0, &[], 0.0);
    let cover: Vec<(Vec3, Vec3)> = chosen.iter().map(|p| *cover_spots.iter().find(|c| c.0 == *p).unwrap()).collect();
    report.push(format!("cover: {} spots", cover.len()));

    // ── The pads and the setup ─────────────────────────────────────────────
    let mut g = fixed;
    let mut pad = |pos: Vec3, look: Vec3| -> usize {
        g.pads.push(PadRow::at(pos, look, 0));
        g.pads.len() - 1
    };
    let mut intro = Vec::new();
    for (i, s) in spawns.iter().enumerate() {
        // The source's facing, unless a wall stands in it (a marker snapped to
        // a waypoint beside where the source put it can face one).
        let f = spawn_facing[i].filter(|&f| reach(level, *s, f) >= 300.0).unwrap_or_else(|| open_facing(level, *s));
        intro.push(json!({"type": "spawn", "pad": pad(*s, look(f))}));
    }
    for (id, b) in bases.iter().enumerate() {
        intro.push(json!({"type": "case", "id": id, "pad": pad(*b, look(open_facing(level, *b)))}));
    }
    for (id, rs) in respawns.iter().enumerate() {
        for s in rs {
            intro.push(json!({"type": "case_respawn", "id": id, "pad": pad(*s, look(open_facing(level, *s)))}));
        }
    }
    for h in &hills {
        intro.push(json!({"type": "hill", "pad": pad(*h, Vec3::X)}));
    }
    intro.push(json!({"type": "outfit", "outfit": 0}));
    let mut props = Vec::new();
    for (i, w) in weapons.iter().enumerate() {
        props.push(json!({"type": "weapon", "scale": 512, "model": 0, "chr": pad(*w, Vec3::Z), "flags": OBJFLAG_FALL, "flags2": 0, "flags3": 0, "weapon": WEAPON_MPLOCATION00 as usize + i}));
        for a in &ammo[i] {
            props.push(json!({"type": "ammocratemulti", "scale": 153, "model": MODEL_MULTI_AMMO_CRATE, "pad": pad(*a, Vec3::Z), "flags": OBJFLAG_FALL, "flags2": 0, "flags3": 0, "maxdamage": 1000}));
        }
    }
    let base = g.pads.len();
    for p in &graph.pads {
        g.pads.push(PadRow::at(p.pos, Vec3::Z, pad_flags(&p.flags)));
    }
    g.waypoints = graph.waypoints.iter().map(|w| (base + w.padnum, w.groupnum, w.neighbours.clone())).collect();
    g.waygroups = graph.waygroups.iter().map(|wg| wg.neighbours.clone()).collect();
    g.cover = cover;
    g.intro.extend(intro);
    g.props.extend(props);
    Ok(Placed { gameplay: g, report })
}
