//! A room as the router and the furnisher see it: a scattered lot with its ceiling on a storey
//! line, its pool if it is a basin, and its spiral if it is a spire. From `autolevel/route.py`
//! (`Space`, `Door`, `spaces`, `pool_rect`, `shell_rect`, `door_pad`).
//!
//! Coordinates: x and y in cells, heights in units. Sets of floor are in half cells ("h").

use crate::cells::{rect_cells, Cells, Rect};
use crate::scatter::{rect_gap, Lot};
use crate::spiral;
use crate::storeys::{snap_top, storey, LEVELS};
use std::collections::BTreeMap;

/// The four ways, in the order everything walks them: east, north, west, south.
pub const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
/// A door's side is the direction it faces, out of its room: `SIDES[i]` goes with `DIRS[i]`.
pub const SIDES: [char; 4] = ['E', 'N', 'W', 'S'];

/// Which of `DIRS` a side letter is.
pub fn side_index(side: char) -> usize {
    SIDES.iter().position(|&s| s == side).unwrap_or_else(|| panic!("{side} is not a side"))
}

/// A basin's water: (x0, y0, x1, y1) in cells and its height.
pub type Pool = (i32, i32, i32, i32, i32);

/// A scattered lot with its ceiling on a storey line.
#[derive(Clone, Debug, PartialEq)]
pub struct Space {
    pub key: String,
    pub kind: &'static str,
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub floor: i32,
    pub top: i32,
    /// For a pod: the room it pokes into. Empty otherwise.
    pub host: String,
    /// Where a basin's water is.
    pub pool: Option<Pool>,
    /// Upstairs floor fixed before routing (a spiral tower's), by storey, in half cells.
    pub decks: Option<BTreeMap<i32, Cells>>,
    /// Half cells of its floor no door may open onto (under a fixed ramp).
    pub clear: Cells,
    /// Its fixed ramps.
    pub preset: Vec<spiral::Ramp>,
    /// A towering hall's high storeys (`towers`): those over the storeys ramped inside it
    /// (`ramped`) that hold a pod, and its galleries, lowest first. Each is a place of its own,
    /// reached by a door from outside and never ramped down. Empty for every other room.
    pub high: Vec<i32>,
    /// Of those, the galleries: high storeys with no pod, a catwalk of their own reached from
    /// outside (`level::galleries`, `Options::galleries`). New in the Rust.
    pub galleries: Vec<i32>,
    /// A cellar's hall and its hatch (`level::cellars`). None for every other room.
    pub cellar: Option<Cellar>,
    /// A flooded room's water: its height (`level::floods`, `furnish::flood`). Its whole floor is
    /// water under railed walkways that join its doors. None for every other room. New in the Rust.
    pub flood: Option<i32>,
    /// What is cut out of a shaped room's rectangle (`level::shapes`): (x0, y0, x1, y1) in cells,
    /// solid on every storey. Its footprint is the rest (an L, a U). Empty for every other room.
    /// New in the Rust.
    pub cuts: Vec<crate::cells::Rect>,
    /// A sunken floor (`level::sinks`): (x0, y0, x1, y1) in cells, `level::SUNK_DEPTH` under the
    /// room's floor, its pit kept from the storey under it. New in the Rust.
    pub sunk: Option<crate::cells::Rect>,
}

/// A cellar: a room a storey under a hall, inside its footprint, joined to it by a hatch in the
/// hall's floor over one of the cellar's walls (a drop down) and a ladder up that wall (the way
/// back). New in the Rust (docs/AUTOLEVEL.md, "Cellars").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cellar {
    /// The hall over it.
    pub hall: String,
    /// The hatch, cells: two cells square, against the wall the ladder is on.
    pub hatch: Rect,
    /// That wall of the cellar (0 south, 1 east, 2 north, 3 west): the hatch's open side in the
    /// hall, where a player climbs out.
    pub side: usize,
}

impl Cellar {
    /// The nav block (2 x 2 h) at the ladder's foot, against its wall, on storey `z`.
    pub fn foot(&self, z: i32) -> (i32, i32, i32) {
        let (x0, y0, x1, y1) = (2 * self.hatch.0, 2 * self.hatch.1, 2 * self.hatch.2, 2 * self.hatch.3);
        let (mx, my) = ((x0 + x1).div_euclid(2) - 1, (y0 + y1).div_euclid(2) - 1);
        let (x, y) = [(mx, y0), (x1 - 2, my), (mx, y1 - 2), (x0, my)][self.side];
        (x, y, z)
    }

    /// The ladder: its two ends along the wall, in cells (x, y), one cell wide in the middle of the
    /// hatch's side, and the way it faces (into the cellar).
    pub fn ladder(&self) -> ((f64, f64), (f64, f64), (i32, i32)) {
        let (x0, y0, x1, y1) = (self.hatch.0 as f64, self.hatch.1 as f64, self.hatch.2 as f64, self.hatch.3 as f64);
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        match self.side {
            0 => ((mx - 0.5, y0), (mx + 0.5, y0), (0, 1)),
            1 => ((x1, my - 0.5), (x1, my + 0.5), (-1, 0)),
            2 => ((mx - 0.5, y1), (mx + 0.5, y1), (0, -1)),
            _ => ((x0, my - 0.5), (x0, my + 0.5), (1, 0)),
        }
    }
}

impl Space {
    pub fn rect(&self) -> Rect {
        (self.x0, self.y0, self.x1, self.y1)
    }

    /// The room's rectangle in half cells.
    pub fn rect_h(&self) -> Rect {
        (2 * self.x0, 2 * self.y0, 2 * self.x1, 2 * self.y1)
    }

    pub fn size(&self) -> (i32, i32) {
        (self.x1 - self.x0, self.y1 - self.y0)
    }

    pub fn area(&self) -> i32 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }

    /// Floor levels of every storey with standing room in it.
    pub fn storeys(&self) -> Vec<i32> {
        LEVELS.iter().copied().filter(|&z| z >= self.floor && storey(z).1 <= self.top).collect()
    }

    pub fn tall(&self) -> bool {
        self.storeys().len() > 1
    }

    /// Is it a towering hall (`towers`)?
    pub fn towers(&self) -> bool {
        towers(self.kind, self.top)
    }

    /// Storeys a vent may open on: the floor (and a spiral's decks).
    pub fn vent_levels(&self) -> Vec<i32> {
        if self.decks.is_some() {
            self.door_levels()
        } else {
            self.storeys().into_iter().take(1).collect()
        }
    }

    /// Storeys a corridor may arrive on. Upper ones need a catwalk and a ramp down inside, so
    /// only rooms with the floor for that offer them.
    pub fn door_levels(&self) -> Vec<i32> {
        let storeys = self.storeys();
        if self.kind == crate::place::ATRIUM.name {
            // its floor and its top storey: the storeys between are reached by nobody (a door on
            // its first upper storey let the router climb from there into the middle of a trek)
            let mut out: Vec<i32> = storeys.iter().copied().take(1).collect();
            out.extend(storeys.last().filter(|z| !out.contains(z)));
            return out;
        }
        if let Some(decks) = &self.decks {
            // a spiral: every storey it reaches
            return storeys.into_iter().filter(|z| *z == self.floor || decks.get(z).is_some_and(|c| !c.is_empty())).collect();
        }
        let (w, d) = self.size();
        let (short, long) = (w.min(d), w.max(d));
        // (a pool leaves little floor for ramps)
        // (an atrium: its first upper storey let the router climb into the middle of a trek)
        let low = if !self.host.is_empty() || self.pool.is_some() || !(short >= 6 && long >= 8) || self.kind == crate::place::ATRIUM.name {
            1
        } else {
            ramped(self.kind, self.area())
        };
        let mut out: Vec<i32> = storeys.into_iter().take(low).collect();
        for &z in &self.high {
            if !out.contains(&z) {
                out.push(z);
            }
        }
        out
    }
}

/// Over this ceiling a hall towers: the storeys ramped inside it are only its lowest (`ramped`),
/// and a pod above them is reached from outside (`Space::high`). No room the Python scatters
/// reaches over it, so every seed's level is as it was.
pub const TOWERING: i32 = 1890;

/// Does a room of this kind with this top tower? An atrium always does; a spire never (its
/// spiral climbs every storey).
pub fn towers(kind: &str, top: i32) -> bool {
    kind == crate::place::ATRIUM.name || (top > TOWERING && kind != "spire")
}

/// How many of a hall's lowest storeys are joined by ramps inside it, and take doors: those a
/// corridor may arrive on in any tall room. A towering hall's others are reached from outside.
pub fn ramped(kind: &str, area: i32) -> usize {
    if kind == crate::place::ATRIUM.name {
        2
    } else if area >= 300 {
        4
    } else {
        3
    }
}

/// A door in a room's wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Door {
    pub room: String,
    /// E, N, W or S: the wall it is in.
    pub side: char,
    /// Its low end along the wall, cells.
    pub a: i32,
    /// The floor it is on.
    pub level: i32,
    pub width: i32,
    /// What is through it (a room or corridor key).
    pub to: String,
}

/// Half cells just inside a door of room `s`, `depth` deep (a catwalk's depth, usually), made
/// `widen` half cells wider each side.
pub fn door_pad(s: &Space, d: &Door, depth: i32, widen: i32) -> Cells {
    let (x0, y0, x1, y1) = s.rect_h();
    let (a0, a1) = (2 * d.a - widen, 2 * (d.a + d.width) + widen);
    let r = match d.side {
        'E' => (x1 - depth, a0, x1, a1),
        'W' => (x0, a0, x0 + depth, a1),
        'N' => (a0, y1 - depth, a1, y1),
        _ => (a0, y0, a1, y0 + depth),
    };
    rect_cells(r).into_iter().filter(|c| x0 <= c.0 && c.0 < x1 && y0 <= c.1 && c.1 < y1).collect()
}

/// A pod's solid block in a hall, in half cells: its air and half-cell walls. Where that leaves
/// a slot under two cells wide beside one of the hall's walls, the block runs on to the wall
/// (nobody fits down the slot, and a catwalk along that wall would have to).
pub fn shell_rect(p: &Space, host: &Space) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (2 * p.x0 - 1, 2 * p.y0 - 1, 2 * p.x1 + 1, 2 * p.y1 + 1);
    let (hx0, hy0, hx1, hy1) = host.rect_h();
    if 0 < x0 - hx0 && x0 - hx0 < 4 {
        x0 = hx0;
    }
    if 0 < hx1 - x1 && hx1 - x1 < 4 {
        x1 = hx1;
    }
    if 0 < y0 - hy0 && y0 - hy0 < 4 {
        y0 = hy0;
    }
    if 0 < hy1 - y1 && hy1 - y1 < 4 {
        y1 = hy1;
    }
    (x0, y0, x1, y1)
}

/// Where a basin's water goes: the middle of the room, two cells of walkway all round. With
/// pods poking in, the half of the room away from them (catwalks to the pods need floor for
/// their ramps). None for a room that is not a basin, or too small.
pub fn pool_rect(lot: &Lot, pods: &[&Lot]) -> Option<Pool> {
    let (mut x0, mut y0, mut x1, mut y1) = lot.rect();
    if lot.kind != "basin" {
        return None;
    }
    if !pods.is_empty() {
        let n = pods.len() as f64;
        let px = pods.iter().map(|p| (p.x0 + p.x1) as f64 / 2.0).sum::<f64>() / n;
        let py = pods.iter().map(|p| (p.y0 + p.y1) as f64 / 2.0).sum::<f64>() / n;
        if x1 - x0 >= y1 - y0 {
            let half = (x0 + x1).div_euclid(2);
            (x0, x1) = if px < (x0 + x1) as f64 / 2.0 { (half, x1) } else { (x0, half) };
        } else {
            let half = (y0 + y1).div_euclid(2);
            (y0, y1) = if py < (y0 + y1) as f64 / 2.0 { (half, y1) } else { (y0, half) };
        }
    }
    if x1 - x0 < 7 || y1 - y0 < 7 {
        return None;
    }
    Some((x0 + 2, y0 + 2, x1 - 2, y1 - 2, lot.floor - 118))
}

/// The rooms for scattered lots, in the lots' order. A basin gets its pool here, unless a room
/// is under it.
pub fn spaces(lots: &[Lot]) -> Vec<Space> {
    let mut out: Vec<Space> = lots
        .iter()
        .map(|a| Space {
            key: a.key.clone(),
            kind: a.kind,
            x0: a.x0,
            y0: a.y0,
            x1: a.x1,
            y1: a.y1,
            floor: a.floor,
            top: snap_top(a.top),
            host: a.host.clone(),
            pool: None,
            decks: None,
            clear: Cells::new(),
            preset: Vec::new(),
            high: Vec::new(),
            galleries: Vec::new(),
            cellar: None,
            flood: None,
            cuts: Vec::new(),
            sunk: None,
        })
        .collect();
    // a towering hall's high storeys: the ones of its pods over the storeys ramped inside it
    for (n, a) in lots.iter().enumerate() {
        if !towers(a.kind, snap_top(a.top)) {
            continue;
        }
        let first = crate::storeys::index(a.floor) + ramped(a.kind, a.area());
        let mut high: Vec<i32> = lots.iter().filter(|p| p.host == a.key && crate::storeys::index(p.floor) >= first).map(|p| p.floor).collect();
        high.sort();
        high.dedup();
        out[n].high = high;
    }
    for (n, a) in lots.iter().enumerate() {
        let pods: Vec<&Lot> = lots.iter().filter(|b| b.host == a.key).collect();
        let Some(p) = pool_rect(a, &pods) else { continue };
        let pit = (p.0 - 1, p.1 - 1, p.2 + 1, p.3 + 1);
        let under = out.iter().any(|b| b.key != a.key && b.floor < a.floor && b.top + 39 > p.4 - 40 && rect_gap(pit, b.rect()) == (0, 0));
        if !under {
            out[n].pool = Some(p);
        }
    }
    out
}

/// The room with this key.
pub fn find<'a>(sp: &'a [Space], key: &str) -> &'a Space {
    sp.iter().find(|s| s.key == key).unwrap_or_else(|| panic!("no room {key}"))
}
