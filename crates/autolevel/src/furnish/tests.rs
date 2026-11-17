//! The furnisher on rooms made by hand, after `FurnishTest` in the Python's
//! `tests/test_autolevel.py` (the answer key holds whole levels, and cannot single these out).

use super::*;
use crate::plan;
use crate::pods::pod_access;
use crate::route::tests::space;
use crate::spiral;

fn door(room: &str, side: char, a: i32, level: i32) -> Door {
    Door { room: room.into(), side, a, level, width: 1, to: String::new() }
}

fn rng(seed: u64) -> PyRandom {
    PyRandom::from_int(seed)
}

/// A tower nine cells square and three storeys tall, furnished.
fn tower(doors: &[Door], mode: &'static str, band: i32) -> Interior {
    let s = space("t", (0, 0, 9, 9), 0, 787, "tower");
    Interior::new(&s, doors, &[], rng(1), &[], Setup { tower: mode, band, ..Setup::default() }).build().unwrap()
}

fn up_and_down() -> [Door; 2] {
    [door("t", 'S', 3, 0), door("t", 'N', 4, 276)]
}

fn storeys_joined(it: &Interior) -> Vec<(i32, i32)> {
    let mut joined: Vec<(i32, i32)> = it.ramps.iter().map(|r| (r.lo, r.hi)).collect();
    joined.sort();
    joined
}

#[test]
fn cells_come_apart_into_rectangles_pieces_and_runs() {
    let mut l = rect_cells((0, 0, 4, 2));
    l.extend(rect_cells((0, 2, 2, 5)));
    assert_eq!(rectangles(&l), [(0, 0, 4, 2), (0, 2, 2, 5)]);
    assert_eq!(rectangles(&Cells::new()), []);
    let mut two = rect_cells((0, 0, 2, 2));
    two.extend(rect_cells((3, 0, 5, 2))); // a cell apart
    two.insert((2, 5)); // and one touching nothing, not even corner to corner
    let pieces = components(&two);
    assert_eq!(pieces.iter().map(|c| c.len()).collect::<Vec<_>>(), [4, 1, 4]);
    assert_eq!(pieces.iter().flatten().count(), two.len());
    assert!(touching(&rect_cells((0, 0, 2, 2)), &rect_cells((2, 0, 4, 2)), 2));
    assert!(!touching(&rect_cells((0, 0, 2, 2)), &rect_cells((2, 1, 4, 3)), 2)); // one cell of edge: nowhere to walk through
    assert!(!touching(&rect_cells((0, 0, 2, 2)), &rect_cells((3, 0, 5, 2)), 2));
    assert_eq!(runs([5, 1, 2, 9, 0, 6], None), [vec![0, 1, 2], vec![5, 6], vec![9]]);
    assert_eq!(runs([5, 1, 2, 9, 0, 6], Some(10)), [vec![9, 0, 1, 2], vec![5, 6]]); // round the loop
    assert_eq!(runs([0, 1, 2], Some(3)), [vec![0, 1, 2]]);
    assert_eq!(mid_block(&rect_cells((4, 8, 8, 12))), (5, 9));
    assert_eq!(mid_block(&rect_cells((4, 8, 6, 9))), (4, 8));
}

#[test]
fn a_ramps_way_on_and_off_is_on_its_centre_line() {
    let r = RampIn { lo: 0, hi: 276, rect: (4, 10, 12, 12), d: 2, top: rect_cells((0, 10, 4, 14)), foot: rect_cells((12, 10, 16, 14)) };
    assert_eq!((r.node(true), r.node(false)), ((2, 10), (12, 10))); // it climbs west
    assert_eq!(r.line(), ((6.0, 5.5), (2.0, 5.5)));
    let r = RampIn { lo: 0, hi: 276, rect: (4, 10, 8, 18), d: 1, top: Cells::new(), foot: Cells::new() }; // two cells wide, climbing north
    assert_eq!((r.node(true), r.node(false)), ((5, 18), (5, 8)));
    assert_eq!(r.line(), ((3.0, 5.0), (3.0, 9.0)));
    assert_eq!(r.cells().len(), 32);
}

#[test]
fn a_pods_block_runs_from_the_ceiling_below_it_to_the_floor_above() {
    let hall = space("hall", (0, 0, 16, 12), 0, 1063, "vast");
    let at = |floor: i32| shell_z(&space("pod", (6, 5, 9, 8), floor, floor + 236, "small"), &hall);
    assert_eq!((at(276), at(551), at(827)), ((236, 551), (512, 827), (787, 1063))); // the top one runs up to the hall's ceiling
    let low = space("low", (0, 0, 16, 12), -276, 236, "vast");
    assert_eq!(shell_z(&space("pod", (6, 5, 9, 8), -276, -39, "small"), &low), (-316, 0));
}

#[test]
fn a_tower_gets_a_catwalk_ring_on_each_storey_and_a_ramp_up_to_each() {
    let it = tower(&up_and_down(), "ring", 2);
    assert_eq!(it.upper, [276, 551]);
    for z in &it.upper {
        let slots: BTreeSet<i32> = it.deck[z].iter().flat_map(|&c| it.slot(c)).collect();
        assert_eq!(slots.len() as i32, 2 * (it.w + it.d)); // all the way round the walls
    }
    assert_eq!(storeys_joined(&it), [(0, 276), (276, 551)]);
    assert!(it.connected());
}

#[test]
fn catwalks_and_ramps_are_as_wide_as_the_doorways() {
    for cells in [1, 2] {
        let wide = 2 * cells; // half cells
        let it = tower(&up_and_down(), "ring", wide);
        let mut landings = Cells::new();
        for r in &it.ramps {
            landings.extend(r.top.iter().chain(&r.foot).copied());
        }
        for z in &it.upper {
            // the ring is that deep from every wall
            for &(x, y) in it.deck[z].difference(&landings) {
                assert!((x - it.x0).min(it.x1 - 1 - x).min(y - it.y0).min(it.y1 - 1 - y) < wide);
            }
        }
        for r in &it.ramps {
            let (x0, y0, x1, y1) = r.rect;
            assert_eq!(((x1 - x0).min(y1 - y0), (x1 - x0).max(y1 - y0)), (wide, RAMP_H));
            assert_eq!((r.top.len(), r.foot.len()), (16, 16)); // landings two cells square, either way
            for (end, landing) in [(true, &r.top), (false, &r.foot)] {
                // and the way on and off is in line with the ramp
                let (nx, ny) = r.node(end);
                assert!(landing.contains(&(nx, ny)) && landing.contains(&(nx + 1, ny + 1)));
            }
        }
        assert!(it.plan_ramps().iter().all(|p| p.width == 118 * cells && p.kind == "open"));
        assert!(it.connected());
    }
}

#[test]
fn a_tower_can_be_told_where_its_own_rings_stop() {
    let s = space("t", (0, 0, 9, 9), 0, 1063, "tower");
    let decked = |it: &Interior| it.upper.iter().copied().filter(|z| !it.deck[z].is_empty()).collect::<Vec<_>>();
    let full = Interior::new(&s, &up_and_down(), &[], rng(1), &[], Setup::default()).build().unwrap();
    assert_eq!(decked(&full), [276, 551, 827]);
    let cut = Interior::new(&s, &up_and_down(), &[], rng(1), &[], Setup { top: Some(551), ..Setup::default() }).build().unwrap();
    assert_eq!(decked(&cut), [276, 551]); // no climb past 551: nothing is up there
    assert_eq!(storeys_joined(&cut), [(0, 276), (276, 551)]);
    assert!(cut.connected());
}

#[test]
fn a_deck_that_ends_under_a_ramp_puts_no_rail_across_it() {
    let s = space("h", (0, 0, 16, 12), 0, 1063, "vast");
    let mut it = Interior::new(&s, &[door("h", 'S', 3, 0)], &[], rng(1), &[], Setup::default());
    let ramp = RampIn { lo: 276, hi: 551, rect: (2, 8, 4, 16), d: 3, top: rect_cells((0, 4, 4, 8)), foot: rect_cells((0, 16, 4, 20)) };
    let mut deck = rect_cells((0, 0, 2, 24));
    deck.extend(ramp.foot.iter().copied());
    deck.extend(rect_cells((0, 14, 32, 16))); // a bridge under its low end
    it.deck.insert(276, deck);
    it.deck.insert(551, ramp.top.clone());
    it.ramps.push(ramp);
    let rails = it.lay_rails();
    assert!(!rails.is_empty());
    for (a, b, _) in rails {
        if a.2 != 276 || a.1 != b.1 {
            continue; // (only rails running across the ramp's line matter)
        }
        let crosses = a.0.min(b.0) < plan::cells(1.5) && plan::cells(1.5) < a.0.max(b.0) && plan::cells(4.0) < a.1 && a.1 < plan::cells(8.0);
        assert!(!crosses, "{a:?} to {b:?}");
    }
}

#[test]
fn an_upstairs_door_opens_onto_deck_and_no_rail_crosses_a_ramp() {
    let it = tower(&[door("t", 'S', 3, 0), door("t", 'E', 2, 276)], "ends", 2);
    let pad = door_pad(&it.s, &it.doors[1], 2, 0);
    assert!(pad.is_subset(&it.deck[&276]));
    for r in &it.ramps {
        let (_, (tx, ty)) = r.line();
        let (x, y) = (plan::cells(tx), plan::cells(ty)); // the ramp's top edge, in units, must not lie on a rail
        for (p0, p1, _) in it.rails.iter().filter(|rail| rail.0 .2 == r.hi) {
            let on = p0.0.min(p1.0) <= x && x <= p0.0.max(p1.0) && p0.1.min(p1.1) <= y && y <= p0.1.max(p1.1);
            assert!(!(on && ((p0.0 == p1.0 && p1.0 == x) || (p0.1 == p1.1 && p1.1 == y))), "{r:?}: {p0:?} to {p1:?}");
        }
    }
}

#[test]
fn water_is_never_a_floor() {
    let mut s = space("b", (0, 0, 12, 9), 0, 512, "basin");
    s.pool = Some((2, 2, 10, 7, -118));
    let it = Interior::new(&s, &[door("b", 'S', 2, 0), door("b", 'N', 8, 0)], &[], rng(3), &[], Setup::default()).build().unwrap();
    let water: Cells = it.pool.difference(&it.bridge).copied().collect();
    assert!(!water.is_empty() && !it.bridge.is_empty());
    assert!(!meets(&water, &it.stand(0)));
    assert!(it.connected()); // round the walkway or over the bridge
    let edge = it.walk(0).iter().map(|c| DIRS.iter().filter(|(dx, dy)| water.contains(&(c.0 + dx, c.1 + dy))).count()).sum::<usize>();
    let railed: i32 = it.rails.iter().filter(|r| r.0 .2 == 0).map(|(p0, p1, _)| (p1.0 - p0.0).abs() + (p1.1 - p0.1).abs()).sum();
    assert_eq!((railed as f64 / 59.055).round() as usize, edge); // a rail on every edge the water is past
    assert_eq!(s.door_levels(), [0]);
    // the tanks stand in the water, clear of the bridge, and the pit is one of the room's boxes
    for &(x0, y0, x1, y1, a, b, tag) in &it.extra_solids {
        assert!(rect_cells((x0, y0, x1, y1)).is_subset(&water) && (a, b, tag) == (-118, 118, "tank"));
    }
    assert_eq!(it.boxes(), [((0, 0, 0), (1417, 1063, 512)), ((236, 236, -118), (1181, 827, 0))]);
}

#[test]
fn a_spiral_climbs_a_storey_a_wall_to_a_ring_at_the_top() {
    for turn in [1, -1] {
        let mut s = space("spire", (0, 0, 10, 9), 0, 1614, "spire");
        let lay = spiral::layout((0, 0, 20, 18), &s.storeys(), 1, turn, 2).unwrap();
        let ramps = lay.ramps.len();
        (s.preset, s.decks, s.clear) = (lay.ramps, Some(lay.decks), lay.clear);
        let it = Interior::new(&s, &[door("spire", 'W', 1, 0)], &[], rng(1), &[], Setup { cover: "none", ..Setup::default() }).build().unwrap();
        assert!(it.connected());
        let top = it.ramps[it.ramps.len() - 1].node(true);
        let start = it.door_nodes()[0].unwrap();
        let path = it.path(start, (top.0, top.1, *s.storeys().last().unwrap())).unwrap();
        let climbed: BTreeSet<i32> = path.iter().map(|n| n.2).collect();
        assert_eq!(climbed.into_iter().collect::<Vec<_>>(), s.storeys()); // every storey on the way
        assert_eq!((it.ramps.len(), it.ramp_rails().len()), (ramps, ramps)); // a rail down each open side
                                                                             // the walk's points are the path's corners: far fewer, the same ends, a storey at a time
        let points = it.waypoints(&path);
        assert!(points.len() < path.len() / 2);
        assert_eq!(points[0], ((start.0 + 1) as f64 / 2.0, (start.1 + 1) as f64 / 2.0, 0));
        assert_eq!(points[points.len() - 1].2, 1378);
        assert!(it.waypoints(&[]).is_empty());
    }
}

#[test]
fn pillars_stand_to_the_ceiling_or_to_the_deck_over_them() {
    let hall = space("hall", (0, 0, 20, 14), 0, 787, "vast");
    let doors = [door("hall", 'S', 2, 0), door("hall", 'N', 16, 0)];
    let with = |how: Setup| Interior::new(&hall, &doors, &[], rng(2), &[], how).build().unwrap();
    let it = with(Setup { cover: "pillars", clutter: 1.0, layout: Some("galleries"), ..Setup::default() });
    let pillars: Vec<&Cover> = it.cover.iter().filter(|c| c.5 != "wall").collect();
    assert!(pillars.len() >= 6);
    for &&(x0, y0, x1, y1, h, _) in &pillars {
        let under = rect_cells((x0, y0, x1, y1)).is_subset(&it.deck[&276]);
        assert_eq!(h, if under { 236 } else { 787 }); // the gallery's underside, or the ceiling
        assert!(under || !meets(&rect_cells((x0 - 2, y0 - 2, x1 + 2, y1 + 2)), &it.deck[&276]));
    }
    assert!(it.bridges >= 1);
    let bare = with(Setup { cover: "none", layout: Some("galleries"), ..Setup::default() });
    assert!(bare.cover.is_empty());
    let crates = with(Setup { cover: "crates", clutter: 1.0, ..Setup::default() });
    assert!(crates.cover.iter().any(|c| c.5 == "housing"));
    assert!(crates.cover.iter().filter(|c| c.5 == "housing").all(|c| c.4 == 118));
    // every solid the plan is given stands on the floor or hangs under a deck's own floor
    assert_eq!(it.solids().len(), rectangles(&it.deck[&276]).len() + rectangles(&it.deck[&551]).len() + it.cover.len());
}

#[test]
fn a_room_can_be_an_l_or_two_bays() {
    let s = space("m", (0, 0, 9, 7), 0, 236, "medium");
    let doors = [door("m", 'W', 1, 0), door("m", 'E', 4, 0)];
    let mut shapes = BTreeSet::new();
    for seed in 0..6 {
        for shape in ["notch", "partition"] {
            let it = Interior::new(&s, &doors, &[], rng(seed), &[], Setup { cover: "none", shape: Some(shape), ..Setup::default() }).build().unwrap();
            assert!(it.shaped.is_empty() || it.shaped == shape);
            shapes.insert(it.shaped);
            assert!(it.connected());
            assert!(it.cover.iter().all(|c| (c.4, c.5) == (236, "wall"))); // wall, floor to ceiling
        }
    }
    assert!(shapes.contains("notch") && shapes.contains("partition"));
}

#[test]
fn a_hole_in_the_floor_is_railed_on_three_sides_and_nothing_stands_under_an_opening() {
    let s = space("m", (0, 0, 8, 6), 276, 512, "medium");
    let doors = [door("m", 'W', 1, 276), door("m", 'E', 3, 276)];
    let how = Setup { cover: "pillars", clutter: 1.0, hole: Some((6, 4, 10, 8)), keep: vec![(12, 2, 16, 6)], ..Setup::default() };
    let it = Interior::new(&s, &doors, &[], rng(1), &[], how).build().unwrap();
    let hole = rect_cells((6, 4, 10, 8));
    assert_eq!(it.hole, hole);
    assert!(!meets(&hole, &it.stand(276)));
    let open = it.hole_open.expect("a side a door can be walked to from");
    assert!(it.nav().contains_key(&it.hole_approach(open))); // somewhere to step off from
    assert_eq!(it.rails.len(), 3);
    let railed: i32 = it.rails.iter().map(|(a, b, _)| (b.0 - a.0).abs() + (b.1 - a.1).abs()).sum();
    assert!((railed - 3 * 236).abs() <= 4);
    assert!(it.connected());
    let mut kept = rect_cells((10, 0, 18, 8));
    kept.extend(rect_cells((4, 2, 12, 10)));
    assert!(it.cover.iter().all(|c| !meets(&rect_cells((c.0, c.1, c.2, c.3)), &kept)));
}

#[test]
fn a_pods_pier_is_walked_round_on_every_deck_it_stands_through() {
    let hall = space("hall", (0, 0, 16, 12), 0, 1063, "vast");
    let mut pod = space("pod", (6, 5, 9, 8), 551, 787, "small");
    pod.host = "hall".into();
    let acc = pod_access(&hall, &pod, &[&pod], 2).unwrap();
    assert!(acc.island);
    let how = Setup { layout: Some("mezzanine"), ..Setup::default() };
    let mut it = Interior::new(&hall, &[door("hall", 'N', 3, 0)], std::slice::from_ref(&pod), rng(1), &[("pod".to_string(), acc)], how);
    assert_eq!((it.shell[&551].len(), it.roof[&827].len(), it.roof[&551].len()), (64, 64, 0)); // its block, and its roof a floor
    it.deck.insert(276, rect_cells((0, 0, 32, 24))); // a deck right across the storey under the pod
    it.piers.push((12, 10, 16, 14, 0, 512));
    let pier = rect_cells((12, 10, 16, 14));
    assert!(!meets(&pier, &it.stand(276)));
    assert!(!meets(&pier, &it.stand(0)));
    assert!(pier.is_subset(&it.walk(276))); // the deck is there; the pier stands on it
}

#[test]
fn a_pod_gets_a_door_onto_deck_and_windows_with_glass() {
    let hall = space("hall", (0, 0, 16, 12), 0, 787, "vast");
    let mut pod = space("pod", (5, -2, 9, 3), 276, 512, "small");
    pod.host = "hall".into();
    let acc = pod_access(&hall, &pod, &[&pod], 2).unwrap();
    assert_eq!(acc.length, 0); // it pokes through the wall: the pad is on that wall
    let access = [("pod".to_string(), acc.clone())];
    let it = Interior::new(&hall, &[door("hall", 'N', 3, 0)], std::slice::from_ref(&pod), rng(1), &access, Setup::default()).build().unwrap();
    assert!(acc.pad.is_subset(&it.deck[&276]));
    assert!(it.connected()); // the pod's door is one of the targets
    assert_eq!(it.targets().len(), 2);
    assert_eq!(it.boxes().len(), 5); // the hall's air round the pod's block: under it, over it, behind it and to either side
    let pp = pod_interior(&hall, &pod, Some(&acc), &[&pod]);
    assert!(pp.glass.len() >= 2);
    assert_eq!(pp.boxes.len(), 2 + pp.glass.len()); // its own air, its door, and an opening for each window
    let d = pp.door.unwrap();
    assert_eq!((d.room.as_str(), d.to.as_str(), d.level, d.width), ("pod", "hall", 276, 1.0));
    // with no way in it has no door, and a window where the door would have been
    let shut = pod_interior(&hall, &pod, None, &[&pod]);
    assert!(shut.door.is_none() && shut.boxes.len() == 1 + shut.glass.len());
}

#[test]
fn an_impossible_room_says_so() {
    let s = space("t", (0, 0, 4, 4), 0, 512, "medium"); // an upstairs door, and no floor for a ramp to it
    let why = Interior::new(&s, &[door("t", 'S', 1, 0), door("t", 'N', 1, 276)], &[], rng(1), &[], Setup::default()).build().unwrap_err();
    assert_eq!(why, "t: no set of ramps joins its storeys");
    // and so does a basin whose pool has no room for a bridge two cells wide
    let mut b = space("b", (0, 0, 7, 7), 0, 512, "basin");
    b.pool = Some((2, 2, 5, 5, -118));
    let why = Interior::new(&b, &[door("b", 'S', 2, 0)], &[], rng(1), &[], Setup { band: 4, ..Setup::default() }).build().unwrap_err();
    assert_eq!(why, "b: its pool is too narrow for a bridge");
}
