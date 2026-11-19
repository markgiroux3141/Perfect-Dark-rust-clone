//! The level as a plan: every room, pod, corridor, ramp and vent as boxes of air, and the links
//! between them. From `autolevel/level.py` (`_plan`, `_describe`).
//!
//! It also deals out what the plan has no place for: the rooms' names and wall schemes, window
//! glass, where water is, sloped rails, and the rooms' moods, ceilings, tints and red lights.
//! All of those come from one random stream (`"{seed}/plan"`), drawn in the Python's order.

use crate::cells::Cells;
use crate::furnish::{rectangles, Interior};
use crate::level::{Level, Pane, Start, ATRIUM_NAMES, HUBS, NAMES, SCHEMES, WALKABLE};
use crate::plan::{box3, cells as units, Box3, Plan, Rail, Ramp, Room};
use crate::py;
use crate::pyrand::PyRandom;
use crate::route::{Piece, WIDE};
use crate::space::{Space, DIRS};
use crate::storeys::storey;
use std::collections::{BTreeMap, BTreeSet};

/// A rectangle of cells between two heights, as a box in units.
fn box_c(r: (i32, i32, i32, i32), z0: i32, z1: i32) -> Box3 {
    box3(r.0 as f64, r.1 as f64, r.2 as f64, r.3 as f64, z0, z1)
}

/// A piece's flat cells, the lowest storey first.
fn by_storey(pc: &Piece) -> Vec<&(i32, Cells)> {
    let mut storeys: Vec<&(i32, Cells)> = pc.cells.iter().collect();
    storeys.sort_by_key(|(z, _)| *z);
    storeys
}

/// Fill in the level's plan, and its glass, pools, sloped rails, moods, ceilings and glows.
/// Err if a room is added twice or a link names a room the plan lacks (the Python stops there).
pub fn plan(lv: &mut Level, rng: &mut PyRandom) -> Result<(), String> {
    let sp = lv.spaces();
    let mut p = Plan::new(&lv.plan.name);
    let mut glass: Vec<Pane> = Vec::new();
    let (mut pools, mut ramp_rails, mut moods, mut ceilings, mut glows) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());

    let (seed, empty) = (lv.seed, lv.options.start == Start::Empty);
    let mut names: Vec<(&str, Vec<&str>)> = NAMES.iter().map(|(k, v)| (*k, v.to_vec())).collect();
    if !empty {
        for (_, v) in &mut names {
            rng.shuffle(v);
        }
    }
    let mut named: BTreeSet<String> = BTreeSet::new();
    let mut stubs: BTreeMap<&str, Vec<Box3>> = sp.iter().map(|s| (s.key.as_str(), Vec::new())).collect();
    let mut links: Vec<(String, String, &'static str)> = Vec::new();
    // boxes and rails added to a room for its openings
    let mut extra: BTreeMap<&str, Vec<Box3>> = BTreeMap::new();
    let mut rails: BTreeMap<&str, Vec<Rail>> = BTreeMap::new();
    for (up, low, r, side) in &lv.drops {
        extra.entry(up).or_default().push(box_c(*r, lv.space(low).top, lv.space(up).floor));
        links.push((up.clone(), low.clone(), if side.is_some() { "drop" } else { "overlook" }));
    }
    for (piece, low, r, z) in &lv.grates {
        extra.entry(piece).or_default().push(box_c(*r, lv.space(low).top, *z));
        links.push((piece.clone(), low.clone(), "overlook"));
    }
    for w in &lv.windows {
        let (dx, dy) = DIRS[w.d];
        let least = |f: fn(&(i32, i32)) -> i32| w.wall.iter().map(f).min().expect("a window has wall cells");
        let most = |f: fn(&(i32, i32)) -> i32| w.wall.iter().map(f).max().expect("a window has wall cells");
        let (x0, y0, x1, y1) = (least(|c| c.0), least(|c| c.1), most(|c| c.0) + 1, most(|c| c.1) + 1);
        let (lo, hi) = storey(w.z);
        extra.entry(&w.piece).or_default().push(box_c((x0, y0, x1, y1), lo, hi));
        // the glass or the rail stands on the hall's side of the opening
        let (a, b) = if dx != 0 {
            let x = if dx > 0 { x1 } else { x0 };
            ((x, y0), (x, y1))
        } else {
            let y = if dy > 0 { y1 } else { y0 };
            ((x0, y), (x1, y))
        };
        if w.kind == "glass" {
            glass.push((w.piece.clone(), (a.0 as f64, a.1 as f64), (b.0 as f64, b.1 as f64), w.z, (dx, dy, 0)));
        } else {
            let end = |c: (i32, i32)| (units(c.0 as f64), units(c.1 as f64), w.z);
            rails.entry(&w.piece).or_default().push((end(a), end(b), (dx, dy, 0)));
        }
        links.push((w.piece.clone(), w.hall.clone(), "overlook"));
    }
    for r in lv.net.routes() {
        let pc = &r.pieces[0];
        if pc.kind == "stub" {
            // a stub's air goes to one of its two rooms: one that is no pod, then the smaller
            let owner = py::first_min([&r.a.room, &r.b.room], |k| {
                let s = lv.space(k);
                (!s.host.is_empty(), s.area(), (*k).clone())
            });
            let owner = owner.expect("a stub has two rooms").as_str();
            let (lo, hi) = storey(pc.floor);
            let cells = pc.cells_at(pc.floor).unwrap_or_else(|| panic!("stub {} has no cells on its floor", pc.key));
            for rect in rectangles(cells) {
                stubs.get_mut(owner).unwrap_or_else(|| panic!("no room {owner}")).push(box_c(rect, lo, hi));
            }
            links.push((r.a.room.clone(), r.b.room.clone(), "door"));
        }
    }

    for s in sp {
        let key = s.key.as_str();
        // a name of its kind's while they last, then its key. (The Python would title-case the
        // key if its list of names came back empty, which the list it falls back on never is.)
        let name = if s.kind == crate::place::ATRIUM.name {
            let mut pool = ATRIUM_NAMES.to_vec();
            PyRandom::named(&format!("{seed}/{key}/name")).shuffle(&mut pool);
            pool.into_iter().find(|n| !named.contains(*n)).map_or_else(|| s.key.clone(), str::to_string)
        } else if empty {
            // (an empty start: the room's own draw of its kind's names, the first one no earlier
            // room has, so a room added later never renames one before it)
            let mut pool = names.iter().find(|(k, _)| *k == s.kind).map_or(Vec::new(), |(_, pool)| pool.clone());
            PyRandom::named(&format!("{seed}/{key}/name")).shuffle(&mut pool);
            pool.into_iter().find(|n| !named.contains(*n)).map_or_else(|| s.key.clone(), str::to_string)
        } else {
            match names.iter_mut().find(|(k, _)| *k == s.kind).and_then(|(_, pool)| pool.pop()) {
                Some(name) => name.to_string(),
                None => s.key.clone(),
            }
        };
        named.insert(name.clone());
        let mut own = None;
        let rng = lv.options.stream(rng, &mut own, || format!("{seed}/{key}/plan"));
        let mut scheme = *rng.choice(SCHEMES.iter().find(|(k, _)| *k == s.kind).map_or(&["standard"][..], |(_, v)| *v));
        let mut boxes;
        if !s.host.is_empty() {
            let pp = lv.pods.iter().find(|(k, _)| k == key).map_or_else(|| panic!("pod {key} has no inside"), |(_, pp)| pp);
            boxes = pp.boxes.clone();
            boxes.extend(stubs[key].iter().copied());
            let room = Room {
                kind: "perch",
                scheme: "plain",
                walkable: WALKABLE.to_vec(),
                note: format!("pod: pokes into {}", s.host),
                ..Room::new(key, &name, s.floor, boxes)
            };
            p.room(room)?;
            glass.extend(pp.glass.iter().map(|(a, b, z, f)| (s.key.clone(), *a, *b, *z, *f)));
            links.push((s.key.clone(), s.host.clone(), "door"));
            continue;
        }
        let it = lv.interior(key);
        if s.floor < 0 && !s.tall() {
            scheme = "low";
        }
        let hub = HUBS.contains(&s.kind);
        boxes = it.boxes();
        boxes.extend(stubs[key].iter().copied());
        boxes.extend(extra.get(key).into_iter().flatten().copied());
        let room = Room {
            kind: if hub { "hub" } else { "room" },
            scheme,
            solids: it.solids(),
            ramps: it.plan_ramps(),
            rails: it.rails.clone(),
            walkable: WALKABLE.to_vec(),
            hub: if hub { describe(s, it) } else { String::new() },
            ..Room::new(key, &name, s.floor, boxes)
        };
        p.room(room)?;
        if let Some(pool) = s.pool {
            pools.push((s.key.clone(), pool.4));
        }
        if let Some(water) = it.flood {
            pools.push((s.key.clone(), water)); // (a flooded room's)
        }
        ramp_rails.extend(it.ramp_rails().into_iter().map(|(a, b, f)| (s.key.clone(), a, b, f)));
        let x = rng.random(); // most rooms are lit as Complex is; some dim, a few bright
        if scheme != "low" && x < 0.3 {
            moods.push((s.key.clone(), if x < 0.18 { 128 } else { 198 }));
        }
        let x = rng.random();
        if s.tall() && x < 0.3 {
            ceilings.push((s.key.clone(), if x < 0.2 { "ceiling_dark" } else { "grille" }));
        } else if !s.tall() {
            let role = if x < 0.2 {
                "ceiling_tile"
            } else if x < 0.35 {
                "ceiling_dark"
            } else {
                "plain"
            };
            ceilings.push((s.key.clone(), role));
        }
    }

    for r in lv.net.routes() {
        let first = &r.pieces[0];
        if first.kind == "stub" {
            continue;
        }
        if first.kind == "vent" {
            // a crawl duct: one cell square, half a storey high
            let pc = first;
            let mut boxes = Vec::new();
            for (z, cells) in by_storey(pc) {
                boxes.extend(rectangles(cells).into_iter().map(|rect| box_c(rect, *z, z + 118)));
            }
            let ducts: Vec<Ramp> = pc
                .ducts
                .iter()
                .map(|((lo, hi), _)| Ramp {
                    start: (units(lo.0), units(lo.1), lo.2),
                    end: (units(hi.0), units(hi.1), hi.2),
                    width: units(1.0),
                    kind: "duct",
                    open_sides: Vec::new(),
                })
                .collect();
            p.room(Room { kind: "vent", scheme: "plain", ramps: ducts, walkable: WALKABLE.to_vec(), ..Room::new(&pc.key, "Vent", pc.floor, boxes) })?;
            links.push((r.a.room.clone(), pc.key.clone(), "vent"));
            links.push((pc.key.clone(), r.b.room.clone(), "vent"));
            continue;
        }
        let mut chain = vec![r.a.room.clone()];
        for (i, pc) in r.pieces.iter().enumerate() {
            // (an empty start: each piece its own stream, named by its route's rooms, not by its
            // key, which is dealt out again when the rooms change)
            let mut own = None;
            let rng = lv.options.stream(rng, &mut own, || format!("{seed}/{}/{}/{i}/plan", r.a.room, r.b.room));
            let mut boxes = Vec::new();
            for (z, cells) in by_storey(pc) {
                let (lo, hi) = storey(*z);
                boxes.extend(rectangles(cells).into_iter().map(|rect| box_c(rect, lo, hi)));
            }
            let own_rails = || rails.get(pc.key.as_str()).cloned().unwrap_or_default();
            if let Some(((fx, fy, fz), (tx, ty, tz))) = pc.ramp {
                let rect = pc.ramp_cells.unwrap_or_else(|| panic!("ramp {} has no cells", pc.key));
                boxes.insert(0, box_c(rect, fz, storey(tz).1)); // the ramp's box comes first
                boxes.extend(extra.get(pc.key.as_str()).into_iter().flatten().copied());
                let ramps = vec![Ramp {
                    start: (units(fx), units(fy), fz),
                    end: (units(tx), units(ty), tz),
                    width: units(WIDE as f64),
                    kind: "enclosed",
                    open_sides: Vec::new(),
                }];
                let rails = own_rails();
                let tint = if rng.random() < 0.6 { "yellow" } else { "" };
                p.room(Room { kind: "ramp", scheme: "plain", ramps, rails, tint, walkable: WALKABLE.to_vec(), ..Room::new(&pc.key, "Ramp", fz, boxes) })?;
            } else {
                let low = pc.floor < 0;
                let tint = if !low && rng.random() < 0.15 { "cyan" } else { "" };
                boxes.extend(extra.get(pc.key.as_str()).into_iter().flatten().copied());
                let scheme = if low { "low" } else { *rng.choice(&["plain", "plain", "standard"]) };
                p.room(Room {
                    kind: "corridor",
                    scheme,
                    rails: own_rails(),
                    tint,
                    walkable: WALKABLE.to_vec(),
                    ..Room::new(&pc.key, "Passage", pc.floor, boxes)
                })?;
                let bend = r
                    .path
                    .windows(3)
                    .map(|w| (w[0], w[1], w[2]))
                    .find(|(a, b, c)| a.2 == b.2 && b.2 == c.2 && (b.0 - a.0) * (c.1 - b.1) != (b.1 - a.1) * (c.0 - b.0));
                if let Some((_, b, _)) = bend {
                    // a red warning light at a bend
                    if tint.is_empty() && glows.len() < 3 && rng.random() < 0.3 {
                        glows.push((pc.key.clone(), (units(b.0), units(b.1))));
                    }
                }
            }
            chain.push(pc.key.clone());
        }
        chain.push(r.b.room.clone());
        for w in chain.windows(2) {
            let ramp = r.pieces.iter().any(|q| (q.key == w[0] || q.key == w[1]) && q.ramp.is_some());
            links.push((w[0].clone(), w[1].clone(), if ramp { "ramp" } else { "door" }));
        }
    }
    // (an opening between two rooms already joined needs no second link)
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (a, b, kind) in &links {
        let pair = if a <= b { (a.as_str(), b.as_str()) } else { (b.as_str(), a.as_str()) };
        if seen.insert(pair) {
            p.link(a, b, kind, "")?;
        }
    }
    lv.plan = p;
    lv.glass = glass;
    lv.pools = pools;
    lv.ramp_rails = ramp_rails;
    lv.moods = moods;
    lv.ceilings = ceilings;
    lv.glows = glows;
    Ok(())
}

/// What a hub is, in words: its size, its catwalks, ramps, pods and pool.
fn describe(s: &Space, it: &Interior) -> String {
    let (w, d) = s.size();
    let mut bits = vec![format!("{w} x {d} cells, {} tall", s.top - s.floor)];
    let decks: Vec<String> = it.upper.iter().filter(|z| !it.deck[*z].is_empty()).map(|z| z.to_string()).collect();
    if !decks.is_empty() {
        bits.push(format!("catwalks at {}", decks.join(", ")));
    }
    let plural = |n: usize| if n > 1 { "s" } else { "" };
    if !it.ramps.is_empty() {
        bits.push(format!("{} ramp{}", it.ramps.len(), plural(it.ramps.len())));
    }
    if !it.pods.is_empty() {
        bits.push(format!("{} pod{} with windows", it.pods.len(), plural(it.pods.len())));
    }
    if s.pool.is_some() {
        bits.push("a railed pool under a bridge (water is never walked on)".to_string());
    }
    bits.join("; ")
}
