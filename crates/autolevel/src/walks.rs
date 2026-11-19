//! Routes to walk a level by, and places to look at it from. From `autolevel/level.py`
//! (`_walks`, `_views`, `_same`).
//!
//! Walks: every corridor from inside one room to inside the other, every vent crawled both
//! ways, every room from door to door, every spiral climbed to the top of its last ramp, every
//! drop stepped into, and two that must fail: through a window, and into a pool (their names
//! start with `!`). Points are (x cell, y cell, the feet's height), with a flag on those passed
//! through or squatted to.
//!
//! Both are dicts in the Python: a name used again keeps its place and takes the new value.

use crate::cells::{rect_cells, Cell};
use crate::furnish::{bounds, meets, Interior, Node};
use crate::level::{Level, Step, View};
use crate::py;
use crate::route::Point;
use crate::space::{side_index, Door, DIRS};

/// `d[name] = value` on a dict kept as a list.
fn set<T>(d: &mut Vec<(String, T)>, name: String, value: T) {
    match d.iter_mut().find(|(k, _)| *k == name) {
        Some((_, v)) => *v = value,
        None => d.push((name, value)),
    }
}

/// A nav node (a block's low corner, in half cells) as the point at the block's middle, in cells.
fn middle(n: Node) -> (f64, f64, i32) {
    ((n.0 + 1) as f64 / 2.0, (n.1 + 1) as f64 / 2.0, n.2)
}

/// The door the room's own list holds for a route's door.
fn same<'a>(lv: &'a Level, d: &'a Door) -> &'a Door {
    lv.net.doors(&d.room).iter().find(|q| (q.side, q.a, q.level) == (d.side, d.a, d.level)).unwrap_or(d)
}

/// A point inside the room a route's door opens into: the middle of a pod, or the block just
/// inside the door. Err where the Python stops: the room's inside does not know the door, or
/// has no standing room at it.
fn inside(lv: &Level, d: &Door) -> Result<(f64, f64, i32), String> {
    let s = lv.space(&d.room);
    if !s.host.is_empty() {
        return Ok(((s.x0 + s.x1) as f64 / 2.0, (s.y0 + s.y1) as f64 / 2.0, s.floor));
    }
    let it = lv.interior(&d.room);
    let door = same(lv, d);
    let i = it.doors.iter().position(|q| q == door).ok_or_else(|| format!("seed {}: {} was furnished without its door to {}", lv.seed, d.room, door.to))?;
    let n = it.door_nodes()[i].ok_or_else(|| format!("seed {}: no standing room at {}'s door to {}", lv.seed, d.room, door.to))?;
    Ok(middle(n))
}

/// The first point, those between flagged as passed through, and the last. (As the Python
/// writes it, a walk of one point comes out as that point twice.)
fn passing(wp: &[(f64, f64, i32)]) -> Vec<Step> {
    let mut out = vec![Step::at(wp[0])];
    if wp.len() > 2 {
        out.extend(wp[1..wp.len() - 1].iter().map(|q| Step::by(*q, 1)));
    }
    out.push(Step::at(wp[wp.len() - 1]));
    out
}

/// Routes for a walker (`lv.walks`). Err if a corridor's door is not one its room was furnished with.
pub fn walks(lv: &mut Level) -> Result<(), String> {
    let mut walks: Vec<(String, Vec<Step>)> = Vec::new();

    for r in lv.net.routes() {
        let a = inside(lv, &r.a)?;
        let b = inside(lv, &r.b)?;
        // not the ramps' own ends: PD eases the feet onto a slope, so they lag there. The flat
        // blocks either side are on the same line.
        let ends: Vec<Point> = r.pieces.iter().filter_map(|pc| pc.ramp).flat_map(|(foot, top)| [foot, top]).collect();
        let mid: Vec<Point> = r.path.iter().copied().filter(|p| !ends.contains(p)).collect();
        let (key, ra, rb) = (r.key(), &r.a.room, &r.b.room);
        if r.pieces[0].kind == "vent" {
            // squat (flag 2) from the room, through the duct and out
            let mut out = vec![Step::at(a)];
            out.extend(mid.iter().map(|p| Step::by(*p, 3)));
            out.extend([Step::by(b, 3), Step::at(b)]);
            set(&mut walks, format!("vent_{key}_{ra}_{rb}"), out);
            // and back: a vent is a way out of a dead end only if it is crawled from that end
            let mut back = vec![Step::at(b)];
            back.extend(mid.iter().rev().map(|p| Step::by(*p, 3)));
            back.extend([Step::by(a, 3), Step::at(a)]);
            set(&mut walks, format!("back_{key}_{rb}_{ra}"), back);
            continue;
        }
        let mut out = vec![Step::at(a)];
        out.extend(mid.iter().map(|p| Step::by(*p, 1)));
        out.push(Step::at(b));
        set(&mut walks, format!("link_{key}_{ra}_{rb}"), out);
    }
    for (key, it) in &lv.interiors {
        let nodes = it.door_nodes();
        if nodes.len() < 2 {
            continue;
        }
        let g = it.nav();
        // a towering hall's high storeys are walked each on its own: nothing joins them to the
        // rest of it inside (`Space::high`). Every other room is one band, walked as ever.
        let mut bands: Vec<(i32, Vec<Option<Node>>)> = Vec::new();
        for n in nodes {
            let band = n.map_or(i32::MIN, |n| it.band_of(n.2));
            match bands.iter_mut().find(|(b, _)| *b == band) {
                Some((_, v)) => v.push(n),
                None => bands.push((band, vec![n])),
            }
        }
        for (band, nodes) in bands {
            let mut pts: Vec<(f64, f64, i32)> = Vec::new();
            for w in nodes.windows(2) {
                let (Some(a), Some(b)) = (w[0], w[1]) else { continue };
                let Some(path) = Interior::path_in(&g, a, b) else { continue };
                let wp = it.waypoints(&path);
                pts.extend_from_slice(if pts.is_empty() { &wp } else { &wp[1..] });
            }
            if pts.len() >= 2 {
                let name = if band == i32::MIN { format!("room_{key}") } else { format!("room_{key}_{band}") };
                set(&mut walks, name, passing(&pts));
            }
        }
    }
    // a spiral is climbed from the floor to the top of its last ramp
    for (key, it) in &lv.interiors {
        let s = lv.space(key);
        if s.decks.is_none() || it.ramps.is_empty() {
            continue;
        }
        let start = it.door_nodes().into_iter().flatten().find(|n| n.2 == s.floor).or_else(|| it.blocks(s.floor).first().map(|c| (c.0, c.1, s.floor)));
        let last = py::first_max(&it.ramps, |r| r.hi).expect("it has ramps");
        let top = last.node(true);
        let path = start.and_then(|start| it.path(start, (top.0, top.1, last.hi)));
        if let Some(path) = path.filter(|path| !path.is_empty()) {
            set(&mut walks, format!("climb_{key}"), passing(&it.waypoints(&path)));
        }
    }
    // walk to the hole's open side, step off, land in the room below
    for (up, low, r, side) in &lv.drops {
        let it = lv.interior(up);
        let start = it.door_nodes().into_iter().flatten().next();
        let path = match (side, start) {
            (Some(side), Some(start)) => it.path(start, it.hole_approach(*side)),
            _ => None,
        };
        if let Some(path) = path.filter(|path| !path.is_empty()) {
            let wp = it.waypoints(&path);
            let mut out = vec![Step::at(wp[0])];
            out.extend(wp[1..].iter().map(|q| Step::by(*q, 1)));
            out.push(Step::at(((r.0 + r.2) as f64 / 2.0, (r.1 + r.3) as f64 / 2.0, lv.space(low).floor)));
            set(&mut walks, format!("drop_{up}_{low}"), out);
        }
    }
    // walk to a pit's open side (or onto a span's plank), step off, land on what is under it
    for (_, it) in &lv.interiors {
        for (n, p) in it.pits.iter().enumerate() {
            let band = it.band_of(p.z);
            let start = it.door_nodes().into_iter().flatten().find(|d| it.band_of(d.2) == band);
            let Some(path) = start.and_then(|s| it.path(s, p.from)).filter(|path| !path.is_empty()) else { continue };
            let wp = it.waypoints(&path);
            let mut out = vec![Step::at(wp[0])];
            out.extend(wp[1..].iter().map(|q| Step::by(*q, 1)));
            out.push(Step::at(((p.to.0 + 1) as f64 / 2.0, (p.to.1 + 1) as f64 / 2.0, p.land)));
            set(&mut walks, pit_name(it, n), out);
        }
    }
    // the glass (or the rail) is not walked through
    for w in &lv.windows {
        let (s, it) = (lv.space(&w.hall), lv.interior(&w.hall));
        let (dx, dy) = DIRS[w.d];
        let (wx, wy) = w.wall[w.wall.len() / 2];
        let below = rect_cells((2 * (wx + dx), 2 * (wy + dy), 2 * (wx + dx) + 2, 2 * (wy + dy) + 2));
        // (only where stepping through would land on the hall's floor)
        if s.pool.is_some()
            || it.upper.iter().any(|q| meets(&below, &it.deck[q]) || meets(&below, &it.roof[q]) || meets(&below, &it.shell[q]))
            || it.ramps.iter().any(|r| meets(&r.cells(), &below))
            || it.cover.iter().any(|c| meets(&rect_cells((c.0, c.1, c.2, c.3)), &below))
            || it.piers.iter().any(|c| meets(&rect_cells((c.0, c.1, c.2, c.3)), &below))
        {
            continue;
        }
        let (fx, fy) = (wx as f64 + 0.5, wy as f64 + 0.5);
        let (dx, dy) = (dx as f64, dy as f64);
        set(&mut walks, format!("!window_{}_{}", w.piece, w.hall), vec![Step::at((fx - dx, fy - dy, w.z)), Step::at((fx + dx, fy + dy, s.floor))]);
    }
    // water is not a floor
    for (key, z) in &lv.pools {
        let s = lv.space(key);
        let pool = s.pool.unwrap_or_else(|| panic!("{key} has no pool"));
        let (px0, py0, px1, py1) = (pool.0 as f64, pool.1 as f64, pool.2 as f64, pool.3 as f64);
        let bx = bounds(&lv.interior(key).bridge);
        let (bx0, by0, bx1, by1) = (bx.0 as f64 / 2.0, bx.1 as f64 / 2.0, bx.2 as f64 / 2.0, bx.3 as f64 / 2.0);
        // from the walkway towards the middle of the larger piece of water
        let out = if bx1 - bx0 >= px1 - px0 - 0.01 {
            // the bridge runs along x: water north and south of it
            let ya = if by0 - py0 >= py1 - by1 { (py0 + by0) / 2.0 } else { (by1 + py1) / 2.0 };
            vec![Step::at((px0 - 1.0, ya, s.floor)), Step::at(((px0 + px1) / 2.0, ya, *z))]
        } else {
            let xa = if bx0 - px0 >= px1 - bx1 { (px0 + bx0) / 2.0 } else { (bx1 + px1) / 2.0 };
            vec![Step::at((xa, py0 - 1.0, s.floor)), Step::at((xa, (py0 + py1) / 2.0, *z))]
        };
        set(&mut walks, format!("!pool_{key}"), out);
    }
    lv.walks = walks;
    Ok(())
}

/// Places to look at the level from (`lv.views`): each hall from a door and from its highest
/// catwalk, each pod out of its window, the openings, the corridors that joined up dead ends,
/// and a few vents, corridors and ramps.
pub fn views(lv: &mut Level) {
    let sp = lv.spaces();
    let mut views: Vec<(String, View)> = Vec::new();
    let off = |n: &Node, x: f64, y: f64| ((n.0 + 1) as f64 / 2.0 - x).abs() + ((n.1 + 1) as f64 / 2.0 - y).abs();
    let centre = |r: (i32, i32, i32, i32)| ((r.0 + r.2) as f64 / 2.0, (r.1 + r.3) as f64 / 2.0);

    let mut rooms: Vec<&crate::space::Space> = sp.iter().filter(|s| s.host.is_empty()).collect();
    rooms.sort_by_key(|s| -s.area());
    let rest: Vec<&crate::space::Space> = rooms.iter().skip(7).filter(|s| matches!(s.kind, "spire" | "tower" | "loft")).copied().collect();
    rooms.truncate(7);
    rooms.extend(rest);
    for s in rooms {
        let it = lv.interior(&s.key);
        let (cx, cy) = centre(s.rect());
        let floor: Vec<Node> = it.door_nodes().into_iter().flatten().filter(|n| n.2 == s.floor).collect();
        if let Some(n) = py::first_max(floor, |n| off(n, cx, cy)) {
            let (x, y, z) = middle(n);
            set(&mut views, format!("{}_door", s.key), (x, y, z, cx, cy));
        }
        if let Some(&z) = it.upper.iter().rfind(|z| !it.blocks(**z).is_empty()) {
            let b = py::first_max(it.blocks(z), |c: &Cell| off(&(c.0, c.1, z), cx, cy)).expect("the storey has blocks");
            let (x, y, _) = middle((b.0, b.1, z));
            set(&mut views, format!("{}_deck{z}", s.key), (x, y, z, cx, cy));
        }
    }
    for p in sp.iter().filter(|s| !s.host.is_empty()) {
        let ((x, y), (hx, hy)) = (centre(p.rect()), centre(lv.space(&p.host).rect()));
        set(&mut views, format!("{}_window", p.key), (x, y, p.floor, hx, hy));
    }
    // a view of (x, y) from the door of room `key` farthest from it
    let from_door = |key: &str, x: f64, y: f64| -> Option<View> {
        let floor = lv.space(key).floor;
        let nodes: Vec<Node> = lv.interior(key).door_nodes().into_iter().flatten().filter(|n| n.2 == floor).collect();
        let (nx, ny, nz) = middle(py::first_max(nodes, |n| off(n, x, y))?);
        Some((nx, ny, nz, x, y))
    };
    // the hole from above, and where it lands
    for (up, low, r, _) in &lv.drops {
        let (x, y) = centre(*r);
        for (key, tag) in [(up, "hole"), (low, "landing")] {
            if let Some(v) = from_door(key, x, y) {
                set(&mut views, format!("drop_{up}_{tag}"), v);
            }
        }
    }
    // the beam under a grate, and the grate from the corridor
    for (piece, low, r, z) in &lv.grates {
        let (x, y) = centre(*r);
        if let Some(v) = from_door(low, x, y) {
            set(&mut views, format!("grate_{piece}_below"), v);
        }
        let cells = lv.net.routes().iter().flat_map(|r| &r.pieces).filter(|pc| pc.key == *piece).find_map(|pc| pc.cells_at(*z));
        let cells = cells.unwrap_or_else(|| panic!("grate's piece {piece} has no cells at {z}"));
        // the cell of it farthest from the grate (the least of those as far)
        let far = py::first_min(cells.iter().copied(), |c| (-((c.0 - r.0).abs() + (c.1 - r.1).abs()), *c)).expect("a piece's storey has cells");
        set(&mut views, format!("grate_{piece}_above"), (far.0 as f64 + 0.5, far.1 as f64 + 0.5, *z, x, y));
    }
    // each gallery from its door farthest from the hall's middle, looking across (none in the
    // Python's levels)
    for s in sp.iter().filter(|s| !s.galleries.is_empty()) {
        let it = lv.interior(&s.key);
        let (cx, cy) = centre(s.rect());
        for &z in &s.galleries {
            let at: Vec<Node> = it.door_nodes().into_iter().flatten().filter(|n| n.2 == z).collect();
            if let Some(n) = py::first_max(at, |n| off(n, cx, cy)) {
                let (x, y, _) = middle(n);
                set(&mut views, format!("gallery_{}_{z}", s.key), (x, y, z, cx, cy));
            }
        }
    }
    // each pit from where it is stepped off, looking across it
    for (_, it) in &lv.interiors {
        for (n, p) in it.pits.iter().enumerate() {
            let (x, y) = ((p.hole.0 + p.hole.2) as f64 / 4.0, (p.hole.1 + p.hole.3) as f64 / 4.0);
            let (fx, fy, _) = middle(p.from);
            set(&mut views, pit_name(it, n), (fx, fy, p.z, x, y));
        }
    }
    // each platform from the far end of its storey's catwalk (none in the Python's levels)
    for (key, it) in &lv.interiors {
        for (n, &(z, r)) in it.platforms.iter().enumerate() {
            let (x, y) = (centre(r).0 / 2.0, centre(r).1 / 2.0);
            if let Some(b) = py::first_max(it.blocks(z), |c: &Cell| off(&(c.0, c.1, z), x, y)) {
                let (bx, by, _) = middle((b.0, b.1, z));
                // (the second on a storey: _2)
                let k = it.platforms[..n].iter().filter(|p| p.0 == z).count();
                let name = if k == 0 { format!("platform_{key}_{z}") } else { format!("platform_{key}_{z}_{}", k + 1) };
                set(&mut views, name, (bx, by, z, x, y));
            }
        }
    }
    // through the opening from the corridor
    for w in &lv.windows {
        let (dx, dy) = (DIRS[w.d].0 as f64, DIRS[w.d].1 as f64);
        let (wx, wy) = w.wall[w.wall.len() / 2];
        let (fx, fy) = (wx as f64 + 0.5, wy as f64 + 0.5);
        set(&mut views, format!("window_{}_{}", w.piece, w.hall), (fx - dx, fy - dy, w.z, fx + 3.0 * dx, fy + 3.0 * dy));
    }
    // into each corridor that joins up a dead end
    let joined: Vec<&str> = lv.fixes.iter().filter_map(|f| if let crate::build::Fix::Joined { by, .. } = f { Some(by.as_str()) } else { None }).collect();
    for r in lv.net.routes() {
        if joined.contains(&r.key()) && r.path.len() >= 3 {
            let (a, b) = (r.path[0], r.path[2]);
            set(&mut views, format!("join_{}_{}", r.key(), r.a.room), (a.0, a.1, a.2, b.0, b.1));
        }
    }
    let count = |views: &[(String, View)], what: &str| views.iter().filter(|(k, _)| k.starts_with(what)).count();
    // a vent's mouth, from the room
    for r in lv.net.routes() {
        if r.pieces[0].kind == "vent" && count(&views, "vent") < 3 {
            let (dx, dy) = DIRS[side_index(r.a.side)];
            let (x, y, z) = r.path[0];
            set(&mut views, format!("vent_{}_{}", r.key(), r.a.room), (x - 3.5 * dx as f64, y - 3.5 * dy as f64, z, x, y));
        }
    }
    for r in lv.net.routes() {
        if r.pieces[0].kind == "corridor" && r.path.len() >= 4 && count(&views, "corridor") < 2 {
            let (a, b) = (r.path[0], r.path[2]);
            set(&mut views, format!("corridor_{}", r.key()), (a.0, a.1, a.2, b.0, b.1));
        }
        if r.ramps() > 0 && count(&views, "ramp") < 4 {
            // up each flight from its foot, and down it from its top
            for pc in &r.pieces {
                if let Some(((fx, fy, fz), (tx, ty, tz))) = pc.ramp {
                    let (ux, uy) = ((tx - fx) / 4.0, (ty - fy) / 4.0);
                    set(&mut views, format!("ramp_{}_up", pc.key), (fx - ux, fy - uy, fz, tx, ty));
                    set(&mut views, format!("ramp_{}_down", pc.key), (tx + ux, ty + uy, tz, fx, fy));
                }
            }
        }
    }
    lv.views = views;
}

/// A pit's name in the walks and views: `pit_<room>_<storey>` (`_2` for a second on that storey).
fn pit_name(it: &crate::furnish::Interior, n: usize) -> String {
    let p = &it.pits[n];
    let k = it.pits[..n].iter().filter(|q| q.z == p.z).count();
    if k == 0 {
        format!("pit_{}_{}", it.s.key, p.z)
    } else {
        format!("pit_{}_{}_{}", it.s.key, p.z, k + 1)
    }
}
