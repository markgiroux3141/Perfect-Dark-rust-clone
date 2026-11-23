//! Sculpted levels from a seed (`--style sculpt`): each room a box whose faces are subdivided
//! and extruded, level by level, into a blocky shape (`shape`); the rooms placed near each other,
//! overlapping ones cut back a wall's thickness (`layout::stamp`); doorways cut through shared
//! walls and corridors routed through the solid between the others, with ramps where they climb
//! (`layout`); then every doorway and floor of each room joined to the rest by ramps and
//! catwalks wherever a player could not get there and back (`repair`). Not a port of anything.
//!
//! ```text
//! let lv = sculpt::generate(1, &Options { style: "sculpt", ..Options::default() })?;
//! lv.plan          // rooms (their air as boxes), corridors, ramps, catwalk rails, links
//! lv.walks         // every doorway of a room to every other, and to each of its floors
//! ```
//!
//! Everything is built on a grid of cells a module (118) across and 59 up (`vox`): air or solid,
//! the air each zone's (a room or a corridor). The plan's rooms are their zones' air cut into
//! boxes; what is left solid inside a room (a platform, a buttress, a catwalk's slab) is simply
//! not carved, so Complex's polygon stage draws it. A ramp is an open sheet over the cells under
//! it (`plan::Ramp`, kind "plate"). Dressed in Complex's kit (`autolevel_build::sculpt`).
//!
//! Two random streams: `<seed>/sculpt` for the rooms' shapes and `<seed>/sculpt/layout` for
//! where they go and how they are joined.

pub mod layout;
pub mod nav;
pub mod repair;
pub mod shape;
pub mod vox;

use crate::cells::Cells;
use crate::level::{Level, Options, Step, View};
use crate::network::Network;
use crate::plan::{cells, Link, Plan, Ramp, Room};
use crate::pyrand::PyRandom;
use crate::report::Report;
use crate::route::Grid;
use crate::space::Space;
use nav::{Move, Nav, Sheet};
use repair::Catwalk;
use shape::{Kind, Spec};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use vox::{Ix, Vox, Z};

/// The rooms a level is made of, in the order they are placed (shuffled first).
pub const KINDS: [Kind; 6] = [Kind::Wide, Kind::Tall, Kind::Both, Kind::Plain, Kind::Plain, Kind::Plain];
/// Loops to add to the spanning tree, as a share of the rooms.
pub const LOOPS: f64 = 0.35;
/// Layouts drawn before the seed is given up.
const TRIES: usize = 12;

/// A doorway as the walks know it: the room (zone), a cell of its floor at the doorway, and the
/// way out.
#[derive(Clone, Copy, Debug)]
struct Door {
    zone: u16,
    cell: Ix,
    d: Ix,
}

/// The level a seed makes.
pub fn generate(seed: u64, o: &Options) -> Result<Level, String> {
    let mut srng = PyRandom::named(&format!("{seed}/sculpt"));
    let mut lrng = PyRandom::named(&format!("{seed}/sculpt/layout"));
    let mut why = String::new();
    for _ in 0..TRIES {
        let mut kinds = KINDS.to_vec();
        srng.shuffle(&mut kinds);
        let rooms: Vec<(Spec, Vox)> = kinds
            .iter()
            .map(|&k| {
                let s = Spec::draw(&mut srng, k);
                let v = shape::sculpt(&mut srng, &s);
                (s, v)
            })
            .collect();
        match build(seed, o, &mut lrng, rooms) {
            Ok(lv) => return Ok(lv),
            Err(e) => why = e,
        }
    }
    Err(format!("seed {seed}: no sculpted level in {TRIES} tries ({why})"))
}

/// A zone's key in the plan.
fn key(zone: u16, rooms: usize) -> String {
    if (zone as usize) <= rooms {
        format!("r{zone}")
    } else {
        format!("c{}", zone as usize - rooms)
    }
}

fn build(seed: u64, o: &Options, rng: &mut PyRandom, rooms: Vec<(Spec, Vox)>) -> Result<Level, String> {
    let t0 = std::time::Instant::now();
    let dbg = std::env::var_os("SCULPT_DEBUG").is_some();
    let specs: Vec<Spec> = rooms.iter().map(|(s, _)| *s).collect();
    let n = rooms.len();
    let placed = layout::place(rng, rooms.into_iter().map(|(_, v)| v).collect());
    if placed.len() < n {
        return Err(format!("{} of {n} rooms placed", placed.len()));
    }
    let (mut v, _) = layout::stamp(&placed);
    let bx = layout::boxes(&v, n);
    let pairs = layout::graph(&bx, LOOPS);
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut doors: Vec<Door> = Vec::new();
    let mut next_zone = n as u16 + 1;
    let mut joined: Vec<(u16, u16, &'static str)> = Vec::new();
    for &(a, b, tree) in &pairs {
        if let Some((ca, cb)) = layout::doorway(&mut v, rng, a, b) {
            let d = (cb.0 - ca.0, cb.1 - ca.1, 0);
            let unit = (d.0.signum(), d.1.signum(), 0);
            doors.push(Door { zone: a, cell: ca, d: unit });
            doors.push(Door { zone: b, cell: cb, d: (-unit.0, -unit.1, 0) });
            joined.push((a, b, "doorway"));
            continue;
        }
        let (Some(ba), Some(bb)) = (bx[a as usize - 1], bx[b as usize - 1]) else { continue };
        let mid = |b: (Ix, Ix)| ((b.0 .0 + b.1 .0) as f64 / 2.0, (b.0 .1 + b.1 .1) as f64 / 2.0);
        let sa = layout::spots(&v, rng, a, ba, mid(bb));
        let sb = layout::spots(&v, rng, b, bb, mid(ba));
        let mut tries: Vec<(i32, layout::Spot, layout::Spot)> = Vec::new();
        for x in &sa {
            for y in &sb {
                let dist = (x.at.0 - y.at.0).abs() + (x.at.1 - y.at.1).abs() + (x.at.2 - y.at.2).abs();
                tries.push((dist, *x, *y));
            }
        }
        tries.sort_by_key(|t| t.0);
        let found = tries.iter().take(12).find_map(|(_, x, y)| layout::corridor(&v, x, y).map(|r| (*x, *y, r)));
        match found {
            Some((x, y, places)) => {
                let zone = next_zone;
                next_zone += 1;
                sheets.extend(layout::carve(&mut v, zone, &places));
                doors.push(Door { zone: a, cell: x.cells[0], d: x.d });
                doors.push(Door { zone: b, cell: y.cells[0], d: y.d });
                joined.push((a, zone, "door"));
                joined.push((b, zone, "door"));
            }
            None if tree => {
                if dbg {
                    eprintln!("sculpt: no corridor r{a} to r{b} ({:.2}s)", t0.elapsed().as_secs_f64());
                }
                return Err(format!("no corridor from r{a} to r{b}"));
            }
            None => {}
        }
    }
    if dbg {
        eprintln!("sculpt: joined in {:.2}s ({} pairs, grid {}x{}x{})", t0.elapsed().as_secs_f64(), pairs.len(), v.nx, v.ny, v.nz);
    }
    // each room made walkable, doorway to doorway and to its floors
    let mut catwalks: Vec<Catwalk> = Vec::new();
    let mut given_up = 0;
    for zone in 1..=n as u16 {
        let Some((lo, hi)) = bx[zone as usize - 1] else { continue };
        let mine: Vec<Ix> = doors
            .iter()
            .filter(|d| d.zone == zone)
            .flat_map(|d| {
                let t = if d.d.0 != 0 { (0, 1, 0) } else { (1, 0, 0) };
                [d.cell, vox::add(d.cell, t)]
            })
            .collect();
        // nothing is built in front of a doorway
        let keep: HashSet<(i32, i32)> = doors
            .iter()
            .filter(|d| d.zone == zone)
            .flat_map(|d| {
                let t = if d.d.0 != 0 { (0, 1, 0) } else { (1, 0, 0) };
                (0..3).flat_map(move |j| (0..2).map(move |l| (d.cell.0 - d.d.0 * j + t.0 * l, d.cell.1 - d.d.1 * j + t.1 * l)))
            })
            .collect();
        let r = repair::repair(&mut v, &mut sheets, &mut catwalks, zone, lo, hi, &mine, &keep);
        given_up += r.given_up.len();
        if dbg {
            eprintln!(
                "sculpt: r{zone} {}: {} ramps, {} catwalks, {} floors left out",
                specs[zone as usize - 1].kind.name(),
                r.ramps,
                r.catwalks,
                r.given_up.len()
            );
        }
    }
    if dbg {
        eprintln!("sculpt: repaired in {:.2}s ({given_up} floors left out)", t0.elapsed().as_secs_f64());
    }
    finish(seed, o, &v, &specs, n, &sheets, &catwalks, &doors, given_up)
}

#[allow(clippy::too_many_arguments)]
fn finish(
    seed: u64,
    o: &Options,
    v: &Vox,
    specs: &[Spec],
    n: usize,
    sheets: &[Sheet],
    catwalks: &[Catwalk],
    doors: &[Door],
    given_up: usize,
) -> Result<Level, String> {
    let zones: BTreeSet<u16> = v.each().map(|p| v.get(p)).filter(|&z| z != 0).collect();
    let base = v.each().filter(|&p| v.air(p)).map(|p| p.2).min().ok_or("no air")?;
    let zu = |k: i32| (k - base) * Z;
    let xu = |i: i32| cells(i as f64);
    let nav = Nav::build(v, sheets, None);
    let floor_of = |zone: u16| nav.regions.iter().filter(|r| r.zone == zone).map(|r| r.k).min();
    let mut srng = PyRandom::named(&format!("{seed}/sculpt/dress"));
    let mut plan = Plan::new(&format!("sculpt_{seed}"));
    for &zone in &zones {
        let k = key(zone, n);
        let boxes = v.boxes(zone).into_iter().map(|(a, b)| ((xu(a.0), xu(a.1), zu(a.2)), (xu(b.0), xu(b.1), zu(b.2)))).collect();
        let mut room = Room::new(&k, &k, floor_of(zone).map_or(0, zu), boxes);
        room.walkable = Vec::new();
        if (zone as usize) <= n {
            let s = &specs[zone as usize - 1];
            room.kind = if matches!(s.kind, Kind::Wide | Kind::Both) { "hub" } else { "room" };
            room.scheme = match s.kind {
                Kind::Tall => "chimney",
                _ => *srng.choice(&["standard", "striped", "pipework"]),
            };
            room.hub = s.kind.name().to_string();
        } else {
            room.kind = "corridor";
            room.scheme = "plain";
        }
        let pt = |p: (i32, i32, i32)| (xu(p.0), xu(p.1), zu(p.2));
        for s in sheets.iter().filter(|s| s.zone == zone) {
            room.ramps.push(Ramp {
                start: pt(s.start),
                end: pt(s.end),
                width: xu(2),
                kind: "plate",
                open_sides: if s.open { vec![-1, 1] } else { Vec::new() },
            });
        }
        for c in catwalks.iter().filter(|c| c.zone == zone) {
            for (a, b, f) in &c.rails {
                room.rails.push((pt(*a), pt(*b), *f));
            }
        }
        plan.room(room)?;
    }
    // a link for every pair of zones whose air touches
    let mut touching: BTreeSet<(u16, u16)> = BTreeSet::new();
    for p in v.each() {
        let a = v.get(p);
        if a == 0 {
            continue;
        }
        for d in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
            let b = v.get(vox::add(p, d));
            if b != 0 && b != a {
                touching.insert((a.min(b), a.max(b)));
            }
        }
    }
    for &(a, b) in &touching {
        plan.links.push(Link { a: key(a, n), b: key(b, n), kind: "door", note: String::new() });
    }
    let problems = plan.check();
    if !problems.is_empty() {
        return Err(format!("the plan: {}", problems.join("; ")));
    }

    // ---- walks: each room doorway to doorway and to its floors; each corridor end to end
    let cellpt = |c: Ix| (c.0 as f64 + 0.5, c.1 as f64 + 0.5, zu(c.2));
    // columns next to a ramp's ends or under it: a player stopped there stands on its slope, so
    // the walker can't hold a stop there to a floor's height
    let by_sheet: HashSet<(i32, i32)> = sheets
        .iter()
        .flat_map(|s| s.cover.iter().map(|&(x, y, _)| (x, y)).chain(s.foot.iter().chain(s.top.iter()).map(|c| (c.0, c.1))).collect::<Vec<_>>())
        .flat_map(|(x, y)| (-1..=1).flat_map(move |dx| (-1..=1).map(move |dy| (x + dx, y + dy))))
        .collect();
    let target = |r: &nav::Region| -> Ix {
        let m = r.middle();
        r.cells.iter().copied().filter(|c| !by_sheet.contains(&(c.0, c.1))).min_by_key(|c| (c.0 - m.0).pow(2) + (c.1 - m.1).pow(2)).unwrap_or(m)
    };
    let route = |a: Ix, b: Ix| -> Vec<Step> {
        let Some(path) = nav.path(a, b) else { return vec![Step::at(cellpt(a)), Step::at(cellpt(b))] };
        let mut keep: Vec<Ix> = Vec::new();
        for (i, &(c, how)) in path.iter().enumerate() {
            let turn = i > 0 && i + 1 < path.len() && {
                let (p, q) = (path[i - 1].0, path[i + 1].0);
                (c.0 - p.0, c.1 - p.1) != (q.0 - c.0, q.1 - c.1)
            };
            let special = matches!(how, Move::Drop | Move::Ramp) || path.get(i + 1).is_some_and(|n| matches!(n.1, Move::Drop | Move::Ramp));
            if i == 0 || i + 1 == path.len() || turn || special || c.2 != keep.last().map_or(c.2, |l| l.2) {
                keep.push(c);
            }
        }
        // the ends, and where a drop lands, are stops: a point passed through counts as reached
        // a little short of it, which for the cell past an edge is still on the edge
        let landed: HashSet<Ix> = path.iter().filter(|(_, how)| *how == Move::Drop).map(|(c, _)| *c).collect();
        let last = keep.len() - 1;
        keep.iter()
            .enumerate()
            .map(
                |(i, &c)| {
                    if i == 0 || i == last || (landed.contains(&c) && !by_sheet.contains(&(c.0, c.1))) {
                        Step::at(cellpt(c))
                    } else {
                        Step::by(cellpt(c), 1)
                    }
                },
            )
            .collect()
    };
    let mut walks: Vec<(String, Vec<Step>)> = Vec::new();
    let mut views: Vec<(String, View)> = Vec::new();
    for zone in 1..=n as u16 {
        let k = key(zone, n);
        let mine: Vec<&Door> = doors.iter().filter(|d| d.zone == zone).collect();
        for i in 0..mine.len() {
            for j in i + 1..mine.len() {
                walks.push((format!("{k}_door{i}_door{j}"), route(mine[i].cell, mine[j].cell)));
            }
        }
        let floors: Vec<&nav::Region> = nav.regions.iter().filter(|r| r.zone == zone && r.cells.len() >= repair::MIN_AREA).collect();
        if let Some(first) = mine.first() {
            for (f, r) in floors.iter().enumerate() {
                if r.cells.contains(&first.cell) {
                    continue;
                }
                walks.push((format!("{k}_door0_floor{f}"), route(first.cell, target(r))));
            }
        }
        // views: in from each doorway, and from its highest floor, at the room's middle
        let Some((lo, hi)) = v.bounds(zone) else { continue };
        let mid = ((lo.0 + hi.0) as f64 / 2.0, (lo.1 + hi.1) as f64 / 2.0);
        for (i, d) in mine.iter().enumerate() {
            let c = d.cell;
            views.push((format!("{k}_door{i}"), (c.0 as f64 + 0.5, c.1 as f64 + 0.5, zu(c.2), mid.0, mid.1)));
        }
        if let Some(top) = floors.iter().max_by_key(|r| (r.k, r.cells.len())) {
            let c = target(top);
            views.push((format!("{k}_top"), (c.0 as f64 + 0.5, c.1 as f64 + 0.5, zu(c.2), mid.0, mid.1)));
        }
    }
    // a corridor's two doorways are its rooms' doorways onto it: walked room to room
    let by_corridor: BTreeMap<u16, Vec<&Door>> = {
        let mut m: BTreeMap<u16, Vec<&Door>> = BTreeMap::new();
        for d in doors {
            let out = vox::add(d.cell, d.d);
            let z = v.get(out);
            if z as usize > n {
                m.entry(z).or_default().push(d);
            }
        }
        m
    };
    for (z, ds) in &by_corridor {
        if let [a, b, ..] = ds[..] {
            walks.push((key(*z, n), route(a.cell, b.cell)));
        }
    }

    // ---- the network's spaces (each zone's box), for what reads them
    let spaces: Vec<Space> = zones
        .iter()
        .filter_map(|&zone| {
            let (lo, hi) = v.bounds(zone)?;
            Some(Space {
                key: key(zone, n),
                kind: if (zone as usize) <= n { "room" } else { "corridor" },
                x0: lo.0,
                y0: lo.1,
                x1: hi.0,
                y1: hi.1,
                floor: floor_of(zone).map_or(0, zu),
                top: zu(hi.2),
                host: String::new(),
                pool: None,
                decks: None,
                clear: Cells::default(),
                preset: Vec::new(),
                high: Vec::new(),
                galleries: Vec::new(),
                cellar: None,
                flood: None,
                cuts: Vec::new(),
                sunk: None,
                base: None,
                blocks: Vec::new(),
            })
        })
        .collect();
    let links = touching.len();
    let report = Report {
        seed,
        scale: "standard",
        rooms: n,
        plan_rooms: zones.len(),
        links,
        loops: links as i64 - zones.len() as i64 + 1,
        one_door_rooms: (1..=n as u16).filter(|z| doors.iter().filter(|d| d.zone == *z).count() == 1).map(|z| key(z, n)).collect(),
        tallest: (1..=n as u16).filter_map(|z| v.bounds(z)).map(|(a, b)| (b.2 - a.2) * Z).max().unwrap_or(0),
        ..Report::default()
    };
    let _ = given_up;
    Ok(Level {
        seed,
        options: o.clone(),
        net: Network { grid: Grid::new(spaces, 2), pods: Vec::new(), hatches: Vec::new(), failed: Vec::new(), cut_off: Vec::new() },
        interiors: Vec::new(),
        pods: Vec::new(),
        plan,
        glass: Vec::new(),
        pools: Vec::new(),
        walks,
        views,
        ramp_rails: ramp_rails(sheets, n, &zu),
        moods: Vec::new(),
        ceilings: Vec::new(),
        glows: Vec::new(),
        drops: Vec::new(),
        grates: Vec::new(),
        windows: Vec::new(),
        beams: Vec::new(),
        dropped: Vec::new(),
        dead_ends: Vec::new(),
        fixes: Vec::new(),
        doorways: Vec::new(),
        arches: Vec::new(),
        braces: Vec::new(),
        surrounds: Vec::new(),
        portals: Vec::new(),
        fittings: Vec::new(),
        lamps: Vec::new(),
        report,
        unmet: Vec::new(),
    })
}

/// Sloped rails down both sides of every ramp in a room.
fn ramp_rails(sheets: &[Sheet], n: usize, zu: &impl Fn(i32) -> i32) -> Vec<crate::level::SlopedRail> {
    let mut out = Vec::new();
    for s in sheets.iter().filter(|s| s.open) {
        let along_x = s.start.1 == s.end.1;
        for side in [-1, 1] {
            let off = |p: (i32, i32, i32)| {
                if along_x {
                    (cells(p.0 as f64), cells((p.1 + side) as f64), zu(p.2))
                } else {
                    (cells((p.0 + side) as f64), cells(p.1 as f64), zu(p.2))
                }
            };
            let facing = if along_x { (0, side, 0) } else { (side, 0, 0) };
            out.push((key(s.zone, n), off(s.start), off(s.end), facing));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate(seed: u64) -> Result<Level, String> {
        super::generate(seed, &Options { style: "sculpt", ..Options::default() })
    }

    #[test]
    fn a_seed_makes_the_same_level_and_its_plan_checks() {
        for seed in 1..=4 {
            let a = generate(seed).unwrap_or_else(|e| panic!("{e}"));
            let b = generate(seed).unwrap();
            assert_eq!(a.plan, b.plan);
            assert!(a.plan.check().is_empty());
            assert!(a.plan.rooms.len() > KINDS.len(), "rooms and corridors");
            assert!(!a.walks.is_empty());
        }
    }
}
