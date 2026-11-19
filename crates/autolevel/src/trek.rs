//! The trek: an atrium many storeys tall with a base on its top storey, and the rooms that climb
//! to it from outside. The way up is not inside the atrium (its top storey is never ramped down:
//! `place::ATRIUM`): it is a helix of rooms round it, each entered high at one end and left a
//! storey or two lower, from a door onto the atrium's top walkway down to its floor. New in the
//! Rust, with nothing in the Python behind it (docs/AUTOLEVEL.md, "Atriums").
//!
//! Goal-first: the atrium and its base are laid, then the trek from the top down, a room at a
//! time along a ring round the atrium (a second ring outside the first when one lap is not
//! enough). Each room shares one storey with the one before (its top) and one with the one after
//! (its floor), so the router can only join them in order; the last stands on the atrium's floor.
//! What comes out is a recipe on an empty start (an `add` for every room, and `OPTIONS`), so the
//! editor can change any of it. A layout whose rooms break the placing rule, or whose way up to
//! the base misses a room of the trek (a shortcut), is laid again from the next stream
//! (`{seed}/trek/{attempt}`).

use crate::level::Level;
use crate::place::ATRIUM;
use crate::pyrand::PyRandom;
use crate::recipe::{set_option, Edit, Recipe};
use crate::scatter::Rect;
use crate::storeys::{CEILINGS, LEVELS};
use std::collections::{BTreeMap, VecDeque};

/// The atrium's key, and its base's.
pub const ATRIUM_KEY: &str = "atrium";
pub const BASE_KEY: &str = "base";
/// How many layouts are tried before giving up.
pub const ATTEMPTS: u32 = 40;
/// Cells between the atrium and the first ring, and between one ring and the next.
const OFF: i32 = 1;
const RING: i32 = 10;
/// Cells between two rooms along a ring.
const SPACE: i32 = 2;
/// The level's options a trek is made with: the router adds no joins of its own beyond the ones
/// that make the level one piece (no loops, risers or vents, each a way round the climb), and the
/// dead-end pass leaves it as it falls (the trek is the level: it would join it up). Drops stay:
/// a hole into the room below is a quick way down, never up.
pub const OPTIONS: [(&str, &str); 4] = [("loops", "0"), ("risers", "0"), ("vents", "0"), ("dead_ends", "false")];

/// What a trek is laid for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrekOptions {
    /// The atrium's storeys, its floor at 0 one of them: 15 puts its top storey's floor at 3858.
    pub storeys: usize,
    /// The site, cells.
    pub site: (i32, i32),
}

impl Default for TrekOptions {
    fn default() -> Self {
        TrekOptions { storeys: 15, site: (80, 60) }
    }
}

/// A laid trek: the recipe, the trek's rooms from the top down, and the stream it came from.
#[derive(Clone, Debug)]
pub struct Trek {
    pub recipe: Recipe,
    pub rooms: Vec<String>,
    pub attempt: u32,
}

/// The rooms of a trek for this seed, as a recipe on an empty start, and the level it makes.
/// Err if no layout of `ATTEMPTS` makes a level whose way from the atrium's floor to the base
/// goes through every room of the trek.
pub fn lay(seed: u64, o: &TrekOptions) -> Result<(Trek, Level), String> {
    if !(4..LEVELS.len()).contains(&o.storeys) {
        return Err(format!("an atrium is 4 to {} storeys tall, not {}", LEVELS.len() - 1, o.storeys));
    }
    let mut base = Recipe::empty(seed);
    for (name, value) in OPTIONS {
        set_option(&mut base.options, name, value)?;
    }
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        let mut rng = PyRandom::named(&format!("{seed}/trek/{attempt}"));
        let Some((edits, rooms)) = layout(&mut rng, o) else {
            last = "the rings ran out".to_string();
            continue;
        };
        let recipe = Recipe { edits, ..base.clone() };
        if let Err(why) = recipe.placed() {
            last = why;
            continue;
        }
        match recipe.generate() {
            Ok(lv) => match climb(&lv) {
                Some(way) if rooms.iter().all(|r| way.contains(r)) => return Ok((Trek { recipe, rooms, attempt }, lv)),
                Some(way) => last = format!("a way up past part of the trek: {}", way.join(", ")),
                None => last = "no way up to the base".to_string(),
            },
            Err(why) => last = why,
        }
    }
    Err(format!("no trek for seed {seed} in {ATTEMPTS} tries (the last: {last})"))
}

/// The shortest way up from the atrium's floor to the base, by the fewest rooms: the rooms passed
/// through in order, from the atrium to the base. A room is one place but the atrium, whose floor
/// and top storey are two (nothing joins them inside); a join is a route (corridor, stub, vent)
/// or a pod's door. Drops are left out: they only go down. None if there is no way up.
pub fn climb(lv: &Level) -> Option<Vec<String>> {
    let top = lv.space(ATRIUM_KEY).storeys().last().copied()?;
    let place = |room: &str, level: i32| if room == ATRIUM_KEY && level == top { format!("{ATRIUM_KEY}@top") } else { room.to_string() };
    let mut next: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut join = |a: String, b: String| {
        next.entry(a.clone()).or_default().push(b.clone());
        next.entry(b).or_default().push(a);
    };
    for r in lv.net.routes() {
        join(place(&r.a.room, r.a.level), place(&r.b.room, r.b.level));
    }
    for s in lv.spaces().iter().filter(|s| !s.host.is_empty()) {
        join(s.key.clone(), place(&s.host, s.floor));
    }
    let mut from: BTreeMap<String, String> = BTreeMap::from([(ATRIUM_KEY.to_string(), String::new())]);
    let mut todo = VecDeque::from([ATRIUM_KEY.to_string()]);
    while let Some(at) = todo.pop_front() {
        if at == BASE_KEY {
            let mut way = vec![at];
            while let Some(back) = from.get(way.last()?).filter(|b| !b.is_empty()) {
                way.push(back.clone());
            }
            way.reverse();
            return Some(way.into_iter().map(|p| p.trim_end_matches("@top").to_string()).collect());
        }
        for n in next.get(&at).into_iter().flatten() {
            if !from.contains_key(n) {
                from.insert(n.clone(), at.clone());
                todo.push_back(n.clone());
            }
        }
    }
    None
}

/// The edits of one layout and the trek's rooms, top down; None if the rings run out.
fn layout(rng: &mut PyRandom, o: &TrekOptions) -> Option<(Vec<Edit>, Vec<String>)> {
    let int = |rng: &mut PyRandom, lo: i32, hi: i32| rng.randint(lo as i64, hi as i64) as i32;
    // the atrium, near the middle of the site
    let (w, d) = (int(rng, 16, 22), int(rng, 12, 16));
    let ax0 = (o.site.0 - w) / 2 + int(rng, -4, 4);
    let ay0 = (o.site.1 - d) / 2 + int(rng, -3, 3);
    let atrium: Rect = (ax0, ay0, ax0 + w, ay0 + d);
    let top = o.storeys; // the top storey's index in LEVELS (LEVELS[1] is the floor, 0)
    let mut edits = vec![Edit::Add { key: ATRIUM_KEY.into(), kind: ATRIUM.name.into(), rect: atrium, floor: 0, top: CEILINGS[top] }];
    // the base: an island in its top storey, three cells and more from the walls
    let (mut bw, mut bd) = (int(rng, 3, 5), int(rng, 3, 4));
    if rng.random() < 0.5 {
        (bw, bd) = (bd, bw);
    }
    let bx0 = int(rng, ax0 + 3, atrium.2 - 3 - bw);
    let by0 = int(rng, ay0 + 3, atrium.3 - 3 - bd);
    edits.push(Edit::Add { key: BASE_KEY.into(), kind: "small".into(), rect: (bx0, by0, bx0 + bw, by0 + bd), floor: LEVELS[top], top: CEILINGS[top] });

    // the sides of a ring in the order walked, each (side, travelling towards its high end)
    let ccw = rng.random() < 0.5;
    let order: [(usize, bool); 4] = if ccw { [(0, true), (1, true), (2, false), (3, false)] } else { [(0, false), (3, true), (2, true), (1, false)] };
    let start = int(rng, 0, 3) as usize;
    let (mut side, mut along, mut ring) = (0usize, 0i32, 0i32);
    let mut rooms = Vec::new();
    let mut at = top; // the storey the trek has come down to
    while at > 1 {
        let drop = if at == 2 || rng.random() < 0.3 { 1 } else { 2 };
        let wide = int(rng, 6, 10);
        let deep = if wide < 8 { int(rng, 8, 9) } else { int(rng, 6, 8) };
        // along the side, or round the corner onto the next (and out to the next ring)
        let rect = loop {
            let (s, up) = order[(start + side) % 4];
            let length = if s % 2 == 0 { w } else { d };
            if along + wide <= length {
                break on_side(atrium, s, up, along, wide, deep, OFF + ring * RING);
            }
            (side, along) = (side + 1, 0);
            if side % 4 == 0 {
                ring += 1;
                if ring > 1 {
                    return None;
                }
            }
        };
        along += wide + SPACE;
        let key = format!("trek{}", rooms.len() + 1);
        let kind = if drop == 2 { "tower" } else { "medium" };
        edits.push(Edit::Add { key: key.clone(), kind: kind.into(), rect, floor: LEVELS[at - drop], top: CEILINGS[at] });
        rooms.push(key);
        at -= drop;
    }
    Some((edits, rooms))
}

/// A room on side `s` of the atrium (0 south, 1 east, 2 north, 3 west), `along` cells from the
/// corner the walk starts at (the low end if `up`), `wide` along the side and `deep` out from it,
/// `off` cells clear of the atrium.
fn on_side(a: Rect, s: usize, up: bool, along: i32, wide: i32, deep: i32, off: i32) -> Rect {
    let (ax0, ay0, ax1, ay1) = a;
    let span = |lo: i32, hi: i32| if up { (lo + along, lo + along + wide) } else { (hi - along - wide, hi - along) };
    match s {
        0 => {
            let (x0, x1) = span(ax0, ax1);
            (x0, ay0 - off - deep, x1, ay0 - off)
        }
        2 => {
            let (x0, x1) = span(ax0, ax1);
            (x0, ay1 + off, x1, ay1 + off + deep)
        }
        1 => {
            let (y0, y1) = span(ay0, ay1);
            (ax1 + off, y0, ax1 + off + deep, y1)
        }
        _ => {
            let (y0, y1) = span(ay0, ay1);
            (ax0 - off - deep, y0, ax0 - off, y1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_go_round_the_atrium_clear_of_it() {
        let a = (30, 22, 48, 36);
        assert_eq!(on_side(a, 0, true, 0, 8, 7, 1), (30, 14, 38, 21));
        assert_eq!(on_side(a, 0, false, 0, 8, 7, 1), (40, 14, 48, 21));
        assert_eq!(on_side(a, 1, true, 0, 8, 7, 1), (49, 22, 56, 30));
        assert_eq!(on_side(a, 2, false, 10, 8, 6, 1), (30, 37, 38, 43));
        assert_eq!(on_side(a, 3, false, 0, 6, 9, 11), (10, 30, 19, 36));
    }

    #[test]
    fn a_trek_climbs_from_the_atriums_floor_to_its_base() {
        for seed in 1..=8 {
            let (trek, lv) = lay(seed, &TrekOptions::default()).unwrap_or_else(|e| panic!("{e}"));
            // the way up: the atrium's floor, every room of the trek from the last to the first,
            // the atrium's top storey, the base
            let mut want = vec![ATRIUM_KEY.to_string()];
            want.extend(trek.rooms.iter().rev().cloned());
            want.extend([ATRIUM_KEY.to_string(), BASE_KEY.to_string()]);
            assert_eq!(climb(&lv).unwrap(), want, "seed {seed}");
            assert!(trek.rooms.len() >= 7, "seed {seed}: {} rooms", trek.rooms.len());
            // the atrium is joined on its floor and its top storey only
            let doors: std::collections::BTreeSet<i32> =
                lv.net.routes().iter().flat_map(|r| [&r.a, &r.b]).filter(|d| d.room == ATRIUM_KEY).map(|d| d.level).collect();
            assert_eq!(doors.into_iter().collect::<Vec<_>>(), [0, LEVELS[15]], "seed {seed}");
        }
    }

    #[test]
    fn the_same_seed_lays_the_same_trek() {
        let a = lay(3, &TrekOptions::default()).unwrap().0;
        let b = lay(3, &TrekOptions::default()).unwrap().0;
        assert_eq!((a.recipe, a.attempt), (b.recipe, b.attempt));
    }
}
