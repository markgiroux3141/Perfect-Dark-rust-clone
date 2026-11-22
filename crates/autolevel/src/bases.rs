//! Bases: a column of rooms that crosses a hall's wall, its floor or its ceiling, with slit
//! windows into it at eye height. New in the Rust (docs/AUTOLEVEL.md, "Bases").
//!
//! A pod hangs wholly inside its hall and opens onto it. A base cuts into the hall from outside,
//! so its way in is from outside, a corridor like any room's:
//! - **through a wall**: the column stands half in the hall and half out, its door on the outside
//!   part;
//! - **through the floor**: the column rises from the storey under the hall into it, entered
//!   from below;
//! - **through the ceiling**: the column hangs from the storey over the hall into it, entered
//!   from above.
//!
//! A column is one room a storey (`split`), each its own `Space` with a whole floor, so a player
//! always stands at eye height with its slits, at any height the base reaches. Storeys are joined
//! as a cellar is to its hall (`Space::cellar`): a hatch down and a ladder up. The lower room of
//! each pair is a cell narrower than the upper on the ladder's side, so the climb comes out on
//! the upper room's floor. A storey wholly inside the hall has no door, only slits; the hall's
//! furnisher keeps out of its block (`Space::blocks`) as it does a pod's shell.

use crate::cells::Rect;
use crate::level::Options;
use crate::pyrand::PyRandom;
use crate::scatter::{apart, clear, rect_overlap, Lot, STOREY};
use crate::space::{Cellar, Space, DIRS};
use crate::storeys::{snap_top, storey, CEILINGS, LEVELS};

/// The kind a base's lots and rooms have (`place::BASE`).
pub const KIND: &str = "base";
/// A slit's bottom and top over its room's floor: a standing player's eye (159) is in it, a
/// crouching one's under its sill.
pub const SILL: i32 = 100;
pub const LINTEL: i32 = 190;
/// The least a base reaches into its hall, each way, and out of it through a wall (cells).
pub const DEEP: i32 = 2;
/// The most storeys a base has.
pub const MOST_STOREYS: usize = 4;
/// Cells a base through the floor or the ceiling keeps from its hall's walls (the hall's
/// catwalks and the floor in front of its doors), when the generator places it.
pub const INSET: i32 = 3;
/// The hall kinds a base may not cross: a spire's spiral and a basin's water fill them, and an
/// atrium is reached only at its floor and its top.
pub const NOT_IN: [&str; 3] = ["spire", "basin", "atrium"];

/// One storey of a base (`Space::base`).
#[derive(Clone, Debug, PartialEq)]
pub struct Storey {
    /// The base as drawn: the key of its column's lot (its lowest storey's room has it too).
    pub column: String,
    /// The hall it crosses.
    pub hall: String,
    /// Wholly inside the hall: it has no door of its own (its ways in are the hatches).
    pub inside: bool,
    /// The walls its slits are in (`DIRS` indices): those that look into the hall's air, all or
    /// some of them.
    pub windows: Vec<usize>,
    /// "band": one slit along each such wall; "posts": slits a cell wide, a cell apart.
    pub slits: &'static str,
}

/// The storeys (floors) a lot stands on, lowest first.
pub fn storeys(a: &Lot) -> Vec<i32> {
    let top = snap_top(a.top);
    LEVELS.iter().copied().filter(|&z| z >= a.floor && storey(z).1 <= top).collect()
}

/// The hall a base lot crosses: the first lot that is no base or pod, overlaps it in plan and
/// shares a height with it.
pub fn hall_of<'a>(b: &Lot, lots: &'a [Lot]) -> Option<&'a Lot> {
    lots.iter().find(|h| h.key != b.key && h.kind != KIND && h.host.is_empty() && rect_overlap(b.rect(), h.rect()).is_some() && !apart(b, h))
}

/// Is `inner` inside `outer`, in plan?
fn within(inner: Rect, outer: Rect) -> bool {
    outer.0 <= inner.0 && outer.1 <= inner.1 && inner.2 <= outer.2 && inner.3 <= outer.3
}

/// Why base `b` may not cross hall `h` as it stands, if it may not.
pub fn misfit(b: &Lot, h: &Lot) -> Option<String> {
    let (bk, hk) = (&b.key, &h.key);
    if NOT_IN.contains(&h.kind) {
        return Some(format!("{bk} crosses {hk}, a {}: a base may not cross a spire, a basin or an atrium", h.kind));
    }
    let ov = rect_overlap(b.rect(), h.rect())?;
    let (ow, od) = (ov.2 - ov.0, ov.3 - ov.1);
    if ow.min(od) < DEEP {
        return Some(format!("{bk} reaches {} cell into {hk}: a base reaches at least {DEEP} cells in", ow.min(od)));
    }
    if ow >= h.x1 - h.x0 || od >= h.y1 - h.y0 {
        return Some(format!("{bk} runs right across {hk}: a base crosses one of its walls, not two"));
    }
    let zs = storeys(b);
    if zs.len() > MOST_STOREYS {
        return Some(format!("{bk} is {} storeys tall: a base has {MOST_STOREYS} at most", zs.len()));
    }
    let hz = storeys(h);
    if !zs.iter().any(|z| hz.contains(z)) {
        return Some(format!("{bk} shares no whole storey with {hk}: a base's slits look into its hall on a storey of both"));
    }
    if within(b.rect(), h.rect()) {
        if b.floor >= h.floor && snap_top(b.top) <= snap_top(h.top) {
            return Some(format!("{bk} stands wholly inside {hk}: a base crosses its wall, its floor or its ceiling (a room inside a hall is a pod)"));
        }
    } else {
        let out = (h.x0 - b.x0).max(b.x1 - h.x1).max(h.y0 - b.y0).max(b.y1 - h.y1);
        if out < DEEP {
            return Some(format!("{bk} stands {out} cell out of {hk}: a base through a wall stands at least {DEEP} cells out, for its door"));
        }
    }
    if zs.len() > 1 && (b.x1 - b.x0).min(b.y1 - b.y0) < 2 + zs.len() as i32 {
        return Some(format!(
            "{bk} is {} x {} cells and {} storeys tall: each storey under another is a cell narrower (its ladder's ledge), so it is at least {} across",
            b.x1 - b.x0,
            b.y1 - b.y0,
            zs.len(),
            2 + zs.len()
        ));
    }
    None
}

/// The rectangle one cell narrower on the `DIRS[d]` side.
fn inset(r: Rect, d: usize) -> Rect {
    match d {
        0 => (r.0, r.1, r.2 - 1, r.3),
        1 => (r.0, r.1, r.2, r.3 - 1),
        2 => (r.0 + 1, r.1, r.2, r.3),
        _ => (r.0, r.1 + 1, r.2, r.3),
    }
}

/// A base's block in its hall, in half cells: its room and a half cell of wall round it, run on
/// to the hall's wall where it would leave a slot under two cells wide (as a pod's shell is), and
/// cut back to the hall.
pub fn block(r: Rect, hall: Rect) -> Rect {
    let (hx0, hy0, hx1, hy1) = (2 * hall.0, 2 * hall.1, 2 * hall.2, 2 * hall.3);
    let (mut x0, mut y0, mut x1, mut y1) = (2 * r.0 - 1, 2 * r.1 - 1, 2 * r.2 + 1, 2 * r.3 + 1);
    if x0 - hx0 < 4 {
        x0 = hx0;
    }
    if hx1 - x1 < 4 {
        x1 = hx1;
    }
    if y0 - hy0 < 4 {
        y0 = hy0;
    }
    if hy1 - y1 < 4 {
        y1 = hy1;
    }
    (x0.max(hx0), y0.max(hy0), x1.min(hx1), y1.min(hy1))
}

/// Bottom and top of a base storey's block in its hall, the storey's floor at `z`: from the
/// ceiling of the storey under it (or its floor's slab) to the floor of the storey over it (so
/// its roof is a floor there) or the hall's ceiling. As a pod's (`furnish::shell_z`).
pub fn block_z(z: i32, hall: &Space) -> (i32, i32) {
    let i = crate::storeys::index(z);
    let lo = if i > 0 { storey(LEVELS[i - 1]).1 } else { z - 40 };
    let hi = if i + 1 < LEVELS.len() && hall.storeys().contains(&LEVELS[i + 1]) { LEVELS[i + 1] } else { hall.top };
    (lo, hi)
}

/// The walls of room `r` that look into the hall's air (`DIRS` indices), each with the span of it
/// that does, in half cells along it: the block's face is a cell or more from the hall's walls,
/// and the span two half cells or more once a half cell is kept at each end.
pub fn faces(r: Rect, hall: Rect) -> Vec<(usize, (i32, i32))> {
    let (hx0, hy0, hx1, hy1) = (2 * hall.0, 2 * hall.1, 2 * hall.2, 2 * hall.3);
    let (bx0, by0, bx1, by1) = block(r, hall);
    let mut out = Vec::new();
    for (d, &(dx, dy)) in DIRS.iter().enumerate() {
        let (face, lo, hi, a, b) = match (dx, dy) {
            (1, _) => (bx1, hx0, hx1, 2 * r.1, 2 * r.3),
            (-1, _) => (bx0, hx0, hx1, 2 * r.1, 2 * r.3),
            (_, 1) => (by1, hy0, hy1, 2 * r.0, 2 * r.2),
            _ => (by0, hy0, hy1, 2 * r.0, 2 * r.2),
        };
        if !(lo + 2 <= face && face <= hi - 2) {
            continue; // the block runs to the hall's wall there, or past it
        }
        let (alo, ahi) = if dx != 0 { (hy0, hy1) } else { (hx0, hx1) };
        let (u0, u1) = (a.max(alo + 1) + 1, b.min(ahi - 1) - 1);
        if u1 - u0 >= 2 {
            out.push((d, (u0, u1)));
        }
    }
    out
}

/// A storey's slit openings: (the room's wall, `DIRS` index; the opening through the block's
/// wall, half cells), from the room's face to the hall's air. `sp` is the level's rooms: a slit
/// is cut only where the hall is clear a cell in front of it (no pod's shell or other base's
/// block there on its storey), so each opens onto the hall's air.
pub fn slits(s: &Space, sp: &[Space]) -> Vec<(usize, Rect)> {
    let Some(b) = &s.base else { return Vec::new() };
    let Some(hall) = sp.iter().find(|h| h.key == b.hall) else { return Vec::new() };
    let mut solid: Vec<Rect> = Vec::new();
    for o in sp.iter().filter(|o| o.key != s.key && o.floor == s.floor) {
        if o.host == hall.key {
            solid.push(crate::space::shell_rect(o, hall));
        } else if o.base.as_ref().is_some_and(|ob| ob.hall == hall.key) {
            solid.push(block(o.rect(), hall.rect()));
        }
    }
    let meets = |a: Rect, c: Rect| a.0 < c.2 && c.0 < a.2 && a.1 < c.3 && c.1 < a.3;
    let mut out = Vec::new();
    let (bx0, by0, bx1, by1) = block(s.rect(), hall.rect());
    for (d, (u0, u1)) in faces(s.rect(), hall.rect()) {
        if !b.windows.contains(&d) {
            continue;
        }
        let spans: Vec<(i32, i32)> = if b.slits == "posts" {
            // a cell of slit, a cell of wall, from the middle out
            let n = ((u1 - u0 + 2) / 4).max(1);
            let start = (u0 + u1 - (4 * n - 2)).div_euclid(2);
            (0..n).map(|k| (start + 4 * k, start + 4 * k + 2)).filter(|&(a, b)| a >= u0 && b <= u1).collect()
        } else {
            vec![(u0, u1)]
        };
        let (x0, y0, x1, y1) = (2 * s.x0, 2 * s.y0, 2 * s.x1, 2 * s.y1);
        for (a, b) in spans {
            let (r, front) = match d {
                0 => ((x1, a, bx1, b), (bx1, a, bx1 + 2, b)),
                1 => ((a, y1, b, by1), (a, by1, b, by1 + 2)),
                2 => ((bx0, a, x0, b), (bx0 - 2, a, bx0, b)),
                _ => ((a, by0, b, y0), (a, by0 - 2, b, by0)),
            };
            if !solid.iter().any(|&c| meets(front, c)) {
                out.push((d, r));
            }
        }
    }
    out
}

/// A base's column as storey rooms, from its lot: what `split` makes of one.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub lots: Vec<Lot>,
    pub storeys: Vec<(String, Storey)>,
    /// (the lower room of a pair, its hatch and ladder up into the upper)
    pub hatches: Vec<(String, Cellar)>,
}

/// The column of rooms base lot `b` makes, crossing hall `h`: one room a storey, the lowest
/// keeping the lot's key and the rest `{key}_2`, `{key}_3` upwards. Each room under another is a
/// cell narrower on its ladder's side; the hatch is two cells square against that wall, in its
/// middle. The walls with slits and how they are cut come from the base's own stream
/// ("{seed}/{key}/base").
pub fn split(seed: u64, b: &Lot, h: &Lot) -> Column {
    let mut rng = PyRandom::named(&format!("{seed}/{}/base", b.key));
    let zs = storeys(b);
    let hz = storeys(h);
    let n = zs.len();
    let key = |k: usize| if k == 0 { b.key.clone() } else { format!("{}_{}", b.key, k + 1) };
    // from the top down: each room under another a cell in on its ladder's side, a side that
    // looks out of the hall where one does (no slit is given up for a ladder), else any
    let mut rects = vec![b.rect(); n];
    let mut sides = vec![0usize; n];
    for k in (0..n.saturating_sub(1)).rev() {
        let up = rects[k + 1];
        let looking: Vec<usize> = faces(up, h.rect()).into_iter().map(|(d, _)| d).collect();
        // (not the wall the hatch over this room is against: the drop from above lands there, so
        // this room's own hatch goes elsewhere, across the room where it can)
        let above = (k + 2 < n).then(|| sides[k + 1]);
        let free = |d: &usize| above != Some(*d);
        let mut pick: Vec<usize> = (0..4).filter(|d| !looking.contains(d) && free(d)).collect();
        if pick.is_empty() {
            pick = (0..4).filter(free).collect();
        }
        if let Some(a) = above {
            pick = vec![(a + 2) % 4];
        }
        let d = *rng.choice(&pick);
        sides[k] = d;
        rects[k] = inset(up, d);
    }
    let mut col = Column { lots: Vec::new(), storeys: Vec::new(), hatches: Vec::new() };
    for (k, &z) in zs.iter().enumerate() {
        let r = rects[k];
        col.lots.push(Lot { key: key(k), kind: KIND, x0: r.0, y0: r.1, x1: r.2, y1: r.3, floor: z, top: z + STOREY, host: String::new() });
        let inhall = hz.contains(&z);
        let looking: Vec<usize> = if inhall { faces(r, h.rect()).into_iter().map(|(d, _)| d).collect() } else { Vec::new() };
        // all of them, or one (the one facing the hall's middle), two or three
        let windows = match looking.len() {
            0 | 1 => looking.clone(),
            m => {
                let x = rng.random();
                let keep = if x < 0.4 {
                    m
                } else if x < 0.65 {
                    1
                } else {
                    (2 + rng.below(2) as usize).min(m)
                };
                let (cx, cy) = ((h.x0 + h.x1) as f64 / 2.0 - (r.0 + r.2) as f64 / 2.0, (h.y0 + h.y1) as f64 / 2.0 - (r.1 + r.3) as f64 / 2.0);
                let mut by: Vec<usize> = looking.clone();
                if keep == 1 {
                    by.sort_by(|&p, &q| {
                        let f = |d: usize| -(DIRS[d].0 as f64 * cx + DIRS[d].1 as f64 * cy);
                        f(p).total_cmp(&f(q))
                    });
                } else {
                    rng.shuffle(&mut by);
                }
                let mut w: Vec<usize> = by.into_iter().take(keep).collect();
                w.sort();
                w
            }
        };
        let slits = if rng.random() < 0.6 { "band" } else { "posts" };
        let inside = inhall && within(r, h.rect());
        col.storeys.push((key(k), Storey { column: b.key.clone(), hall: h.key.clone(), inside, windows, slits }));
        if k + 1 < n {
            // the hatch: two cells square against the narrowed wall, towards one end of it and the
            // next pair's towards the other (with the walls opposite, the drop from above lands
            // diagonally across the room from the hole below, not against its rail); a cell from
            // the corner where the wall is long enough
            let d = sides[k];
            let (x0, y0, x1, y1) = r;
            let end = |lo: i32, hi: i32| {
                let m = i32::from(hi - lo >= 7);
                if k % 2 == 0 {
                    lo + m
                } else {
                    hi - 2 - m
                }
            };
            let (mx, my) = (end(x0, x1), end(y0, y1));
            let hatch = match d {
                0 => (x1 - 2, my, x1, my + 2),
                1 => (mx, y1 - 2, mx + 2, y1),
                2 => (x0, my, x0 + 2, my + 2),
                _ => (mx, y0, mx + 2, y0 + 2),
            };
            // (a cellar's sides: 0 south, 1 east, 2 north, 3 west)
            col.hatches.push((key(k), Cellar { hall: key(k + 1), hatch, side: (d + 1) % 4 }));
        }
    }
    col
}

/// The chance each hall gets a base when they may have them (`Options::bases`).
pub const CHANCE: f64 = 0.75;

/// Bases the generator adds (`Options::bases`): each hall (a room of no other kind than the
/// halls', no pod) takes one with the chance `CHANCE`, from its own stream ("{seed}/{key}/bases"), through
/// one of its walls, its floor or its ceiling, 3 to 8 cells a side (up to 10 through a wall),
/// clear of every other lot by the scatter's rule. Twenty-five tries, then none. Not in an
/// empty start (its rooms are drawn by hand).
pub fn random(seed: u64, o: &Options, lots: &[Lot]) -> Vec<Lot> {
    let mut out = lots.to_vec();
    if !o.bases || o.start == crate::level::Start::Empty {
        return out;
    }
    let halls: Vec<Lot> = lots
        .iter()
        .filter(|h| h.host.is_empty() && h.kind != KIND && !NOT_IN.contains(&h.kind) && (h.x1 - h.x0).min(h.y1 - h.y0) >= 8 && h.area() >= 80)
        .cloned()
        .collect();
    let mut made = 0;
    for h in &halls {
        let mut rng = PyRandom::named(&format!("{seed}/{}/bases", h.key));
        if rng.random() >= CHANCE {
            continue;
        }
        let hz = storeys(h);
        let (hw, hd) = (h.x1 - h.x0, h.y1 - h.y0);
        for _ in 0..25 {
            let how = rng.below(3);
            let w = rng.randint(3, 8) as i32;
            let d = rng.randint(3, 8) as i32;
            let lot = match how {
                0 => {
                    // through a wall: a storey or two of the hall's, up to ten cells along it
                    let side = rng.below(4) as usize;
                    let tall = rng.randint(1, 2.min(hz.len() as i64)) as usize;
                    let first = rng.below((hz.len() - tall + 1) as u64) as usize;
                    let (floor, top) = (hz[first], hz[first + tall - 1] + STOREY);
                    let along = rng.randint(3, 10) as i32;
                    let (inn, out) = (rng.randint(DEEP as i64, 5) as i32, rng.randint(DEEP as i64, 4) as i32);
                    let (span, deep) = if side.is_multiple_of(2) { (hd, hw) } else { (hw, hd) };
                    if along + 2 > span || inn + 3 > deep {
                        continue;
                    }
                    let a = rng.randint(1, (span - along - 1) as i64) as i32;
                    let r = match side {
                        0 => (h.x1 - inn, h.y0 + a, h.x1 + out, h.y0 + a + along),
                        1 => (h.x0 + a, h.y1 - inn, h.x0 + a + along, h.y1 + out),
                        2 => (h.x0 - out, h.y0 + a, h.x0 + inn, h.y0 + a + along),
                        _ => (h.x0 + a, h.y0 - out, h.x0 + a + along, h.y0 + inn),
                    };
                    Lot { key: String::new(), kind: KIND, x0: r.0, y0: r.1, x1: r.2, y1: r.3, floor, top, host: String::new() }
                }
                _ => {
                    // through the floor (from the storey under it) or the ceiling (to the storey
                    // over it), a storey or two into the hall, `INSET` cells from its walls
                    if w + 2 * INSET > hw || d + 2 * INSET > hd {
                        continue;
                    }
                    let into = rng.randint(1, 2.min(hz.len() as i64 - 1).max(1)) as usize;
                    let (floor, top) = if how == 1 {
                        let i = LEVELS.iter().position(|&z| z == h.floor).expect("a hall's floor is a storey's");
                        if i == 0 {
                            continue;
                        }
                        (LEVELS[i - 1], hz[into - 1] + STOREY)
                    } else {
                        let i = LEVELS.iter().position(|&z| z == hz[hz.len() - 1]).expect("a storey");
                        if i + 1 >= LEVELS.len() || CEILINGS[i + 1] > CEILINGS[CEILINGS.len() - 1] {
                            continue;
                        }
                        (hz[hz.len() - into], LEVELS[i + 1] + STOREY)
                    };
                    let x = rng.randint((h.x0 + INSET) as i64, (h.x1 - INSET - w) as i64) as i32;
                    let y = rng.randint((h.y0 + INSET) as i64, (h.y1 - INSET - d) as i64) as i32;
                    Lot { key: String::new(), kind: KIND, x0: x, y0: y, x1: x + w, y1: y + d, floor, top, host: String::new() }
                }
            };
            let lot = Lot { key: format!("base{}", made + 1), ..lot };
            if lot.x0 < 0 || lot.y0 < 0 || misfit(&lot, h).is_some() || hall_of(&lot, &out).map(|x| &x.key) != Some(&h.key) {
                continue;
            }
            if !out.iter().all(|q| q.key == h.key || clear(&lot, q, crate::place::GAP)) {
                continue;
            }
            out.push(lot);
            made += 1;
            break;
        }
    }
    out
}

/// Every base lot split into its storeys (`split`), in the lots' order: (the lots, each storey's
/// `Storey`, the hatches). A base that crosses no hall lawfully stays one room of its own.
pub fn columns(seed: u64, lots: &[Lot]) -> Column {
    let mut col = Column { lots: Vec::new(), storeys: Vec::new(), hatches: Vec::new() };
    for a in lots {
        let hall = (a.kind == KIND).then(|| hall_of(a, lots)).flatten().filter(|h| misfit(a, h).is_none());
        match hall {
            Some(h) => {
                let c = split(seed, a, h);
                col.lots.extend(c.lots);
                col.storeys.extend(c.storeys);
                col.hatches.extend(c.hatches);
            }
            None => col.lots.push(a.clone()),
        }
    }
    col
}

/// A base's names, dealt out from its column's own stream ("{seed}/{column}/name").
pub const NAMES: [&str; 8] =
    ["The Pillbox", "The Blockhouse", "The Bunker", "The Strongpoint", "The Redoubt", "The Guard Post", "The Casemate", "The Watch Room"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::rect_cells;
    use crate::level::{generate, Options};
    use crate::recipe::{Edit, Recipe};

    fn add(key: &str, kind: &str, rect: Rect, floor: i32, top: i32) -> Edit {
        Edit::Add { key: key.to_string(), kind: kind.to_string(), rect, floor, top }
    }

    /// The three of the user's sketch (2026-11-22) in one hall: through its east wall, up through
    /// its floor, down through its ceiling; and rooms round it for corridors to reach them from.
    pub(crate) fn three() -> Recipe {
        let mut r = Recipe::empty(1);
        r.options.flooded = 0.0;
        r.edits = vec![
            add("hall", "colossal", (20, 20, 44, 38), 0, 787),
            add("wallbase", KIND, (40, 24, 47, 30), 0, 236),
            add("floorbase", KIND, (26, 30, 32, 35), -276, 236),
            add("ceilbase", KIND, (28, 23, 34, 28), 551, 1063),
            add("east", "medium", (50, 22, 57, 32), 0, 236),
            add("west", "medium", (8, 22, 16, 30), 0, 236),
            add("under", "medium", (8, 33, 16, 40), -276, -40),
            add("loft", "loft", (26, 8, 34, 15), 827, 1063),
            add("stair", "tower", (8, 8, 15, 15), 0, 1063),
        ];
        r
    }

    #[test]
    fn a_base_crosses_its_halls_wall_floor_or_ceiling_and_looks_into_it_through_slits() {
        let lv = three().generate().unwrap();
        let s = |k: &str| lv.space(k).clone();
        // each base a column of one-storey rooms, the lowest keeping the base's key
        for (key, n) in [("wallbase", 1), ("floorbase", 2), ("ceilbase", 2)] {
            let rooms: Vec<_> = lv.spaces().iter().filter(|r| r.base.as_ref().is_some_and(|b| b.column == key)).collect();
            assert_eq!(rooms.len(), n, "{key}");
            assert!(rooms.iter().all(|r| !r.tall() && r.kind == KIND && r.base.as_ref().unwrap().hall == "hall"));
        }
        // through the wall: half in, half out, its door outside the hall, slits on the inside
        let (w, hall) = (s("wallbase"), s("hall"));
        assert!(!w.base.as_ref().unwrap().inside && w.x0 < hall.x1 && w.x1 > hall.x1);
        assert!(!slits(&w, lv.spaces()).is_empty());
        for d in lv.net.doors("wallbase") {
            assert!(d.side == 'E' || d.a >= hall.x1, "{d:?} opens into the hall");
        }
        // through the floor: the room in the hall has slits and no door; the one under it the door,
        // and a ladder up through a hatch
        let (up, low) = (s("floorbase_2"), s("floorbase"));
        assert!(up.base.as_ref().unwrap().inside && up.floor == hall.floor && low.floor < hall.floor);
        assert!(lv.net.doors("floorbase_2").is_empty() && !lv.net.doors("floorbase").is_empty());
        assert_eq!(low.cellar.as_ref().map(|c| c.hall.as_str()), Some("floorbase_2"));
        assert!(!slits(&up, lv.spaces()).is_empty());
        // through the ceiling: the room in the hall hangs on its top storey, entered from above
        let (low, up) = (s("ceilbase"), s("ceilbase_2"));
        assert!(low.base.as_ref().unwrap().inside && low.top == hall.top && up.floor > hall.top);
        assert!(lv.net.doors("ceilbase").is_empty() && !lv.net.doors("ceilbase_2").is_empty());
        assert_eq!(low.cellar.as_ref().map(|c| c.hall.as_str()), Some("ceilbase_2"));
        // each room under another is a cell narrower on its ladder's side: the climb comes out on a floor
        for r in lv.spaces().iter().filter(|r| r.cellar.is_some() && r.base.is_some()) {
            let over = s(&r.cellar.as_ref().unwrap().hall);
            assert!(within(r.rect(), over.rect()), "{}", r.key);
            assert_eq!((over.x1 - over.x0 - r.x1 + r.x0) + (over.y1 - over.y0 - r.y1 + r.y0), 1, "{}", r.key);
        }
        // the hall keeps out of the blocks: none of its floor or ramps is in them
        let it = lv.interior("hall");
        for (key, r, z) in &hall.blocks {
            let b = rect_cells(block(*r, hall.rect()));
            assert!(it.walk(*z).is_disjoint(&b), "{key}: the hall's floor runs into it");
            assert!(it.ramps.iter().all(|rp| rp.cells().is_disjoint(&b) || (rp.lo != *z && rp.hi != *z)), "{key}: a ramp runs into it");
        }
        // a slit is a way to see, not to go: each is an opening (an overlook) into the hall
        for key in ["wallbase", "floorbase_2", "ceilbase"] {
            assert!(lv.plan.links.iter().any(|l| ((l.a == key && l.b == "hall") || (l.b == key && l.a == "hall")) && l.kind == "overlook"), "{key}");
            let v = lv.views.iter().find(|(n, _)| *n == format!("base_{key}")).map(|(_, v)| *v).unwrap();
            assert_eq!(v.2, s(key).floor);
        }
    }

    #[test]
    fn slits_are_at_eye_height() {
        // a standing player's eye (159) is in them, a crouching one's under their sill
        const { assert!(90 < SILL && SILL < 159 && 159 < LINTEL && LINTEL < STOREY) };
    }

    #[test]
    fn what_a_base_may_not_do_is_refused_in_words() {
        let refused = |rect: Rect, floor: i32, top: i32| {
            let mut r = three();
            r.edits.truncate(1);
            r.edits.push(add("b", KIND, rect, floor, top));
            r.rooms().err().unwrap_or_default()
        };
        assert!(refused((30, 26, 34, 30), 0, 236).contains("wholly inside"), "a pod's place");
        assert!(refused((43, 24, 47, 30), 0, 236).contains("reaches 1 cell into"));
        assert!(refused((42, 24, 45, 30), 0, 236).contains("stands 1 cell out"));
        assert!(refused((18, 24, 46, 30), 0, 236).contains("runs right across"));
        assert!(refused((26, 30, 29, 34), -276, 787).contains("at least"));
        assert!(refused((26, 30, 32, 35), -276, 1338).contains("storeys tall"));
        // and the lawful ones make their rooms
        assert_eq!(refused((40, 24, 47, 30), 0, 236), "");
        assert_eq!(refused((26, 30, 32, 35), -276, 236), "");
    }

    #[test]
    fn the_generator_adds_bases_of_its_own_and_no_seed_changes_without_them() {
        let (mut made, mut bases, mut shapes) = (0, 0, std::collections::BTreeSet::new());
        for seed in 1..=12 {
            let plain = generate(seed, &Options::python());
            let o = Options { bases: true, ..Options::python() };
            let Ok(lv) = generate(seed, &o) else { continue };
            assert!(plain.is_ok(), "seed {seed}: a base made a level that could not be made");
            made += 1;
            for s in lv.spaces().iter().filter(|s| s.base.is_some()) {
                let b = s.base.as_ref().unwrap();
                let hall = lv.space(&b.hall);
                if s.key == b.column {
                    bases += 1;
                }
                if hall.storeys().contains(&s.floor) {
                    shapes.insert(if !b.inside {
                        "wall"
                    } else if lv.spaces().iter().any(|q| q.key.starts_with(&b.column) && q.floor < hall.floor) {
                        "floor"
                    } else {
                        "ceiling"
                    });
                }
                if b.inside {
                    assert!(lv.net.doors(&s.key).is_empty(), "seed {seed}: {} opens into its hall", s.key);
                }
            }
        }
        assert!(made >= 8 && bases >= 9, "{made} levels, {bases} bases");
        assert_eq!(shapes.len(), 3, "{shapes:?}");
        assert!(generate(40, &Options::python()).unwrap().spaces().iter().all(|s| s.base.is_none()));
    }

    /// Long probe: every seed that makes a level without bases and big pods makes one with them
    /// (they are on by default). `SEEDS=1-60 cargo test -p autolevel --release -- --ignored
    /// --nocapture seeds_with_bases`.
    #[test]
    #[ignore]
    fn seeds_with_bases_make_levels_as_they_did() {
        use crate::place::Scale;
        let (lo, hi) =
            std::env::var("SEEDS").ok().and_then(|s| s.split_once('-').map(|(a, b)| (a.parse().unwrap(), b.parse().unwrap()))).unwrap_or((1u64, 40u64));
        let mut lost = Vec::new();
        for scale in [Scale::Grand, Scale::Standard, Scale::Towering] {
            let (mut both, mut bases, mut only_without, mut only_with) = (0, 0, 0, 0);
            for seed in lo..=hi {
                let mut on = Options::default();
                on.siting.scale = scale;
                let off = Options { bases: false, big_pods: false, ..on.clone() };
                let (a, b) = (generate(seed, &off), generate(seed, &on));
                match (&a, &b) {
                    (Ok(_), Ok(lv)) => {
                        both += 1;
                        bases += lv.report.bases;
                        if lv.report.bases == 0 {
                            println!("{scale:?} {seed}: no bases");
                        }
                    }
                    (Ok(_), Err(why)) => {
                        only_without += 1;
                        lost.push(format!("{scale:?} {seed}: {why}"));
                    }
                    (Err(_), Ok(_)) => only_with += 1,
                    _ => {}
                }
            }
            println!("{scale:?}: {both} both ({bases} bases), {only_without} lost with bases, {only_with} gained");
        }
        for l in &lost {
            println!("lost: {l}");
        }
        assert!(lost.is_empty());
    }

    #[test]
    fn big_pods_are_grown_where_their_hall_has_room_and_keep_a_way_in() {
        let (mut grown, mut pods) = (0, 0);
        for seed in 1..=8 {
            let Ok(plain) = generate(seed, &Options::python()) else { continue };
            let Ok(lv) = generate(seed, &Options { big_pods: true, ..Options::python() }) else { continue };
            for p in lv.spaces().iter().filter(|s| !s.host.is_empty()) {
                pods += 1;
                if plain.spaces().iter().find(|q| q.key == p.key).is_some_and(|q| q.area() < p.area()) {
                    grown += 1;
                    assert!(p.area() * 5 <= lv.space(&p.host).area());
                }
                assert!(lv.pods.iter().any(|(k, pp)| *k == p.key && pp.door.is_some()), "seed {seed}: {} has no door", p.key);
            }
        }
        assert!(grown * 5 >= pods && grown >= 4, "{grown} of {pods} pods grown");
    }
}
