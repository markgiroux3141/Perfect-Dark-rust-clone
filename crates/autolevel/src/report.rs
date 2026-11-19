//! A level in numbers, and the score a sweep keeps the best seeds by. From
//! `autolevel/level.py` (`report`, `_is_ring`, `score`).
//!
//! `score` turns what a good level is supposed to be (the user's notes on Pumpworks and Stratum)
//! into one number: rooms of very different sizes, sprawl with no hub and ring, tall rooms with
//! catwalks circling the walls and ramps between them, lookouts that poke into halls, room to
//! breathe (few corridors), loops, few rooms with one door, and no long corridor or climb that
//! ends in nothing.

use crate::build::Fix;
use crate::deadends::Row;
use crate::furnish::Interior;
use crate::level::Level;
use crate::py;
use crate::route::{RAMP, WIDE};
use crate::scatter::hops;
use std::collections::BTreeSet;

/// A level's numbers (the Python's `report` dict, its keys in this order).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Report {
    pub seed: u64,
    /// "grand" or "standard".
    pub scale: &'static str,
    /// Rooms, pods too.
    pub rooms: usize,
    /// Rooms of the plan: those, and every corridor piece and vent.
    pub plan_rooms: usize,
    /// Pairs of rooms joined: by a corridor, stub or vent, or as a pod and its hall.
    pub links: usize,
    pub loops: i64,
    pub one_door_rooms: Vec<String>,
    /// What hangs off the level by one way in.
    pub dead_ends: Vec<Row>,
    /// Those that are too long, too far or too high.
    pub dead_ends_long: usize,
    /// Rooms off a main area that go nowhere.
    pub camps: usize,
    pub dead_end_fixes: Vec<Fix>,
    /// The most links any room has, as a share of all links.
    pub hub_share: f64,
    /// The most doors between any two rooms.
    pub diameter: usize,
    /// The largest room's floor over the median room's, and over the smallest's.
    pub size_ratio: f64,
    pub largest_over_smallest: f64,
    /// Rooms 500 or more tall.
    pub tall_rooms: usize,
    pub tallest: i32,
    pub largest: i32,
    /// Rooms whose floor is at 551 or above, pods aside.
    pub high_rooms: usize,
    pub spirals: usize,
    /// The ramps in each spiral.
    pub spiral_storeys: Vec<usize>,
    pub switchbacks: usize,
    pub bridges: usize,
    pub layouts: Vec<&'static str>,
    pub shaped: Vec<&'static str>,
    pub pillars: usize,
    pub crates: usize,
    /// How far the largest room's middle is from the level's, as a share of half its diagonal.
    pub centre_offset: f64,
    pub floors: Vec<i32>,
    /// The level's width and depth in cells.
    pub extent: (i32, i32),
    /// Corridor floor as a share of all floor.
    pub corridor_share: f64,
    pub corridors: usize,
    pub vents: usize,
    pub sloped_vents: usize,
    /// Windows from corridors into halls.
    pub overlooks: usize,
    pub drops: usize,
    pub lightwells: usize,
    pub grates: usize,
    pub stubs: usize,
    pub corridor_ramps: usize,
    pub longest_corridor: f64,
    pub upper_doors: usize,
    pub catwalk_rooms: usize,
    pub catwalk_cells: i64,
    pub ring_rooms: usize,
    pub room_ramps: usize,
    pub pods: usize,
    pub pods_dropped: usize,
    /// Panes of glass.
    pub windows: usize,
    pub pools: usize,
    /// Pillars, crates and cross walls standing on the floors. (The Python's dict gives this the
    /// key "cover", which first held the level's cover option: the count replaces it.)
    pub cover: usize,
    pub rails: usize,
    pub walks: usize,
    pub score: f64,
    /// New in the Rust (not the Python's dict, nor the key's): platforms laid off the catwalks
    /// (`furnish::platforms`), and the halls' empty air: cells of it in the rooms with storeys
    /// over their floor, as a share of those rooms' cells, and the biggest rectangle of it in one
    /// room, cells.
    pub platforms: usize,
    /// Galleries (`Space::galleries`).
    pub galleries: usize,
    /// Drops through platforms (`furnish::pits`), and of them spans.
    pub holes: usize,
    pub spans: usize,
    /// Cellars with their hatch (`Space::cellar`).
    pub cellars: usize,
    /// Islands (`furnish::angled`), and the angled catwalks: their spokes and the shortcuts.
    pub islands: usize,
    pub angled: usize,
    /// Flooded rooms (`furnish::flood`), and their rafts.
    pub flooded: usize,
    pub rafts: usize,
    /// Shaped rooms (`furnish::shape`; the Python's `shaped` is its notched and partitioned rooms).
    pub shaped_rooms: usize,
    pub empty_air: f64,
    pub largest_void: f64,
}

/// The level's numbers.
pub fn report(lv: &Level) -> Report {
    let sp = lv.spaces();
    let g = lv.plan.graph();
    let mut areas: Vec<i32> = sp.iter().map(|s| s.area()).collect();
    areas.sort();
    let mut heights: Vec<i32> = sp.iter().map(|s| s.top - s.floor).collect();
    heights.sort();
    let edges = lv.net.edges();
    let deg = |key: &str| edges.iter().map(|(a, b)| (a == key) as usize + (b == key) as usize).sum::<usize>();
    let (x0, y0) = (sp.iter().map(|s| s.x0).min().expect("a level has rooms"), sp.iter().map(|s| s.y0).min().expect("a level has rooms"));
    let (x1, y1) = (sp.iter().map(|s| s.x1).max().expect("a level has rooms"), sp.iter().map(|s| s.y1).max().expect("a level has rooms"));
    let big = py::first_max(sp, |s| s.area()).expect("a level has rooms");
    // (Python's `** 0.5`, which is the C library's pow and may differ from sqrt in the last place)
    let half = (((x1 - x0).pow(2) + (y1 - y0).pow(2)) as f64).powf(0.5) / 2.0;
    let mid = |a: i32, b: i32| (a + b) as f64 / 2.0;
    let off = ((mid(big.x0, big.x1) - mid(x0, x1)).powi(2) + (mid(big.y0, big.y1) - mid(y0, y1)).powi(2)).powf(0.5) / half;
    let room_cells: i32 = sp.iter().filter(|s| s.host.is_empty()).map(|s| s.area()).sum();
    let routes = lv.net.routes();
    let pieces = || routes.iter().flat_map(|r| &r.pieces);
    let corridor_cells = pieces().filter(|pc| pc.kind != "vent").flat_map(|pc| &pc.cells).map(|(_, cs)| cs.len() as i32).sum::<i32>()
        + pieces().filter(|pc| pc.ramp.is_some()).map(|_| RAMP * WIDE).sum::<i32>();
    let its = || lv.interiors.iter().map(|(_, it)| it.as_ref());
    let first = |r: &crate::route::Route| r.pieces[0].kind;
    let mut one_door_rooms: Vec<String> = sp.iter().filter(|s| deg(&s.key) == 1).map(|s| s.key.clone()).collect();
    one_door_rooms.sort();
    let most_links = sp.iter().map(|s| deg(&s.key)).max().expect("a level has rooms");
    let diameter = sp.iter().map(|s| hops(&edges, &s.key).into_iter().map(|(_, far)| far).max().expect("a room is no doors from itself")).max();
    let sorted = |mut v: Vec<&'static str>| {
        v.sort();
        v
    };
    let mut spiral_storeys: Vec<usize> = sp.iter().filter(|s| s.decks.is_some()).map(|s| s.preset.len()).collect();
    spiral_storeys.sort();
    let mut out = Report {
        seed: lv.seed,
        scale: lv.options.siting.scale.name(),
        rooms: sp.len(),
        plan_rooms: g.rooms,
        links: edges.len(),
        loops: edges.len() as i64 - sp.len() as i64 + 1,
        one_door_rooms,
        dead_ends: lv.dead_ends.iter().map(|e| e.row()).collect(),
        dead_ends_long: lv.dead_ends.iter().filter(|e| !e.allowed).count(),
        camps: lv.dead_ends.iter().filter(|e| e.allowed && !e.rooms.is_empty()).count(),
        dead_end_fixes: lv.fixes.clone(),
        hub_share: py::round_to(most_links as f64 / edges.len() as f64, 2),
        diameter: diameter.expect("a level has rooms"),
        size_ratio: py::round_to(areas[areas.len() - 1] as f64 / areas[areas.len() / 2] as f64, 1),
        largest_over_smallest: py::round_to(areas[areas.len() - 1] as f64 / areas[0] as f64, 1),
        tall_rooms: sp.iter().filter(|s| s.top - s.floor >= 500).count(),
        tallest: heights[heights.len() - 1],
        largest: areas[areas.len() - 1],
        high_rooms: sp.iter().filter(|s| s.floor >= 551 && s.host.is_empty()).count(),
        spirals: sp.iter().filter(|s| !s.preset.is_empty()).count(),
        spiral_storeys,
        switchbacks: routes.iter().filter(|r| r.switchback()).count(),
        bridges: its().map(|it| it.bridges).sum(),
        layouts: sorted(its().filter_map(|it| it.layout).collect()),
        shaped: sorted(its().map(|it| it.shaped).filter(|s| !s.is_empty()).collect()),
        pillars: its().flat_map(|it| &it.cover).filter(|c| c.5 != "wall" && c.4 > 118).count(),
        crates: its().flat_map(|it| &it.cover).filter(|c| c.4 <= 118).count(),
        centre_offset: py::round_to(off, 2),
        floors: sp.iter().map(|s| s.floor).collect::<BTreeSet<i32>>().into_iter().collect(),
        extent: (x1 - x0, y1 - y0),
        corridor_share: py::round_to(corridor_cells as f64 / (corridor_cells + room_cells) as f64, 2),
        corridors: routes.iter().filter(|r| !matches!(first(r), "stub" | "vent")).count(),
        vents: routes.iter().filter(|r| first(r) == "vent").count(),
        sloped_vents: routes.iter().filter(|r| !r.pieces[0].ducts.is_empty()).count(),
        overlooks: lv.windows.len(),
        drops: lv.drops.iter().filter(|d| d.3.is_some()).count(),
        lightwells: lv.drops.iter().filter(|d| d.3.is_none()).count(),
        grates: lv.grates.len(),
        stubs: routes.iter().filter(|r| first(r) == "stub").count(),
        corridor_ramps: routes.iter().map(|r| r.ramps()).sum(),
        longest_corridor: routes.iter().map(|r| r.length()).fold(0.0, |most: f64, len| if len > most { len } else { most }),
        upper_doors: sp.iter().map(|s| lv.net.doors(&s.key).iter().filter(|d| d.level != s.floor).count()).sum(),
        catwalk_rooms: its().filter(|it| it.upper.iter().any(|z| !it.deck[z].is_empty())).count(),
        catwalk_cells: py::round(its().map(|it| it.upper.iter().map(|z| it.deck[z].len()).sum::<usize>()).sum::<usize>() as f64 / 4.0),
        ring_rooms: its().filter(|it| it.upper.iter().any(|&z| is_ring(it, z))).count(),
        room_ramps: its().map(|it| it.ramps.len()).sum(),
        pods: lv.pods.len(),
        pods_dropped: lv.dropped.len(),
        windows: lv.glass.len(),
        pools: lv.pools.len(),
        cover: its().map(|it| it.cover.len()).sum(),
        rails: its().map(|it| it.rails.len()).sum(),
        walks: lv.walks.len(),
        score: 0.0,
        platforms: its().map(|it| it.platforms.len()).sum(),
        galleries: sp.iter().map(|s| s.galleries.len()).sum(),
        holes: its().map(|it| it.pits.len()).sum(),
        spans: its().flat_map(|it| &it.pits).filter(|p| !p.planks.is_empty()).count(),
        cellars: lv.drops.iter().filter(|d| d.3.is_some() && lv.space(&d.1).cellar.is_some()).count(),
        islands: its().map(|it| it.islands.len()).sum(),
        angled: its().map(|it| it.angled.len()).sum(),
        flooded: its().filter(|it| it.flood.is_some()).count(),
        rafts: its().map(|it| it.rafts.len()).sum(),
        shaped_rooms: its().filter(|it| !it.cut.is_empty()).count(),
        empty_air: 0.0,
        largest_void: 0.0,
    };
    let voids: Vec<crate::furnish::Void> = its().filter(|it| !it.upper.is_empty()).map(|it| it.void()).collect();
    let (empty, room) = voids.iter().fold((0.0, 0.0), |(e, r), v| (e + v.empty, r + v.room));
    out.empty_air = if room > 0.0 { py::round_to(empty / room, 2) } else { 0.0 };
    out.largest_void = voids.iter().map(|v| v.largest).fold(0.0, f64::max);
    out.score = score(&out);
    out
}

/// Does a storey's deck run round (nearly) all of the room's walls?
fn is_ring(it: &Interior, z: i32) -> bool {
    let p = 2 * (it.w + it.d);
    let have: BTreeSet<i32> = it.deck[&z].iter().flat_map(|c| it.slot(*c)).collect();
    have.len() as f64 >= 0.85 * p as f64
}

/// Higher is better. Each term is one of the user's notes on what a good level is.
pub fn score(r: &Report) -> f64 {
    let least = |a: usize, b: usize| a.min(b) as f64;
    let layouts: BTreeSet<&str> = r.layouts.iter().copied().collect();
    let mut s = 0.0;
    s += r.size_ratio.min(12.0) * 0.5; // very unequal rooms
    s += least(r.diameter, 10) * 0.4; // sprawl
    s -= (r.hub_share - 0.12).max(0.0) * 20.0; // no one room everything hangs off
    s += r.centre_offset * 3.0; // the big hall is not in the middle
    s += least(r.ring_rooms, 3) * 1.5; // catwalks circling the walls
    s += least(r.catwalk_rooms, 6) * 0.7;
    s += least(r.room_ramps, 10) * 0.3;
    s += least(r.pods, 4) * 1.2; // lookouts with windows
    s += least(r.pools, 1) * 1.0;
    s -= (r.corridor_share - 0.2).max(0.0) * 25.0; // room to breathe
    s -= (r.longest_corridor - 20.0).max(0.0) * 0.15;
    s += (r.loops.min(9) as f64) * 0.5;
    s += least(r.spirals, 2) * 1.5; // towers climbed round the walls
    s += least(r.high_rooms, 4) * 0.5; // with somewhere to go at the top
    s += least(r.switchbacks, 2) * 1.0; // enclosed ramps that double back
    s += least(r.bridges, 2) * 0.5;
    s += least(r.vents, 4) * 0.3 + least(r.overlooks, 4) * 0.3; // ways through, and ways to see through
    s += least(r.drops, 2) * 0.4 + least(r.grates, 3) * 0.3;
    s += least(layouts.len(), 3) * 0.5 + least(r.shaped.len(), 3) * 0.3; // rooms unlike each other
    s -= r.one_door_rooms.len() as f64 * 0.5;
    s -= r.dead_ends_long as f64 * 3.0; // a long corridor, or a climb, that ends in nothing
    py::round_to(s, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_scored_as_python_scores_it() {
        // the numbers of seed 1's level, and what Python's score() makes of them
        let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let r = Report {
            size_ratio: 16.7,
            diameter: 6,
            hub_share: 0.21,
            centre_offset: 0.48,
            ring_rooms: 5,
            catwalk_rooms: 6,
            room_ramps: 20,
            pods: 2,
            pools: 1,
            corridor_share: 0.17,
            longest_corridor: 16.0,
            loops: 20,
            spirals: 2,
            high_rooms: 3,
            switchbacks: 0,
            bridges: 3,
            vents: 4,
            overlooks: 4,
            drops: 1,
            grates: 0,
            layouts: vec!["mezzanine", "ring", "ring"],
            shaped: vec!["notch", "notch", "notch"],
            one_door_rooms: own(&["small1", "small6"]),
            ..Report::default()
        };
        assert_eq!(score(&r), 36.84);
        // a poor one: a hub, long corridors, three one-door rooms, two long dead ends
        let r = Report {
            size_ratio: 4.5,
            diameter: 12,
            hub_share: 0.4,
            centre_offset: 0.07,
            ring_rooms: 1,
            catwalk_rooms: 2,
            room_ramps: 3,
            pods: 5,
            corridor_share: 0.31,
            longest_corridor: 27.5,
            loops: 2,
            switchbacks: 3,
            bridges: 1,
            vents: 1,
            drops: 2,
            grates: 5,
            layouts: vec!["galleries"],
            one_door_rooms: own(&["a", "b", "c"]),
            dead_ends_long: 2,
            ..Report::default()
        };
        assert_eq!(score(&r), 4.08);
        assert_eq!(score(&Report::default()), 0.0);
    }
}
