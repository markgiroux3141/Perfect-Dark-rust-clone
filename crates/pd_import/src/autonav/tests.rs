//! The repair of a generated level's waypoints: on a ledge made here, and on
//! seed 40 as the importer makes it.

use autolevel::recipe::Recipe as Seed;
use autolevel_build::built;
use pd_core::ids::*;
use pd_sim::stage::{GeomPoly, LevelGeom};

use super::*;
use crate::autowalk::SimWalk;

fn floor(lo: (f32, f32), hi: (f32, f32), y: f32) -> GeomPoly {
    let v = |x: f32, z: f32| Vec3::new(x, y, z);
    GeomPoly::from_tile(1, GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2, 0, vec![v(lo.0, lo.1), v(lo.0, hi.1), v(hi.0, hi.1), v(hi.0, lo.1)])
}

/// The links a chr may walk, as (from, to) positions' x.
fn links(g: &NavGraph) -> Vec<(i32, i32)> {
    let x = |w: usize| g.waypoint_pos(w).x.round() as i32;
    let mut out: Vec<(i32, i32)> = directed_links(g.waypoints.len(), |w| &g.waypoints[w].neighbours).into_iter().map(|(a, b, _)| (x(a), x(b))).collect();
    out.sort();
    out
}

#[test]
fn a_way_the_graph_lacks_is_linked_and_one_way_where_it_drops() {
    // A floor, and one two metres lower beside it: a ledge to drop off.
    let level = TileLevel::new(LevelGeom { polys: vec![floor((0.0, -200.0), (400.0, 200.0), 0.0), floor((400.0, -200.0), (800.0, 200.0), -200.0)], rooms: vec![1] });
    let pad = |x: f32, y: f32| NavPad { pos: Vec3::new(x, y + PAD_HEIGHT, 0.0), room: Some(1), flags: PadFlags::default() };
    // Two waypoints on each, each pair linked: nothing joins the floors.
    let graph = graph_from_links(vec![pad(100.0, 0.0), pad(300.0, 0.0), pad(500.0, -200.0), pad(700.0, -200.0)], &[vec![1], vec![0], vec![3], vec![2]], 200.0);
    assert_eq!(links(&graph), [(100, 300), (300, 100), (500, 700), (700, 500)]);
    // The generator's ways: off the ledge, and along the lower floor.
    let legs = [
        Leg { a: Vec3::new(350.0, 0.0, 0.0), b: Vec3::new(500.0, -200.0, 0.0), what: "drop_upper_lower".into() },
        Leg { a: Vec3::new(500.0, -200.0, 0.0), b: Vec3::new(700.0, -200.0, 0.0), what: "room_lower".into() },
    ];
    let (out, done) = repair(&graph, &level, &legs);
    assert_eq!((done.legs, done.joined, done.added, &done.unwalked), (2, 1, 1, &vec![]));
    assert_eq!(done.linked, [("drop_upper_lower".to_string(), true)]);
    assert!(done.line().contains("1 linked with 1 waypoints added (1 drop (one way))"), "{}", done.line());
    // A waypoint at the ledge, tied both ways to the two it can walk to, and
    // the drop from it one way, onto the waypoint already at its foot. Nothing
    // climbs the ledge, so the floor below is where nobody gets back from,
    // and the largest part is what is kept: the three above.
    assert_eq!(done.outside, 2);
    assert_eq!(links(&out), [(100, 300), (100, 350), (300, 100), (300, 350), (350, 100), (350, 300)]);
    // With a way back up (a ramp's leg), all five stay, the drop among them.
    let ramp = GeomPoly::from_tile(1, GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2, 0, vec![Vec3::new(400.0, 0.0, 200.0), Vec3::new(400.0, 0.0, 300.0), Vec3::new(800.0, -200.0, 300.0), Vec3::new(800.0, -200.0, 200.0)]);
    let level = TileLevel::new(LevelGeom {
        polys: vec![floor((0.0, -200.0), (400.0, 300.0), 0.0), floor((400.0, -200.0), (800.0, 200.0), -200.0), floor((800.0, -200.0), (1000.0, 300.0), -200.0), ramp],
        rooms: vec![1],
    });
    let mut legs = legs.to_vec();
    legs.push(Leg { a: Vec3::new(850.0, -200.0, 250.0), b: Vec3::new(350.0, 0.0, 250.0), what: "open ramp in room".into() });
    let (out, done) = repair(&graph, &level, &legs);
    assert_eq!((done.joined, done.added, done.outside, &done.unwalked), (1, 3, 0, &vec![]));
    assert_eq!(done.linked, [("drop_upper_lower".to_string(), true), ("open ramp in room".to_string(), false)]);
    let l = links(&out);
    assert!(l.contains(&(350, 500)) && !l.contains(&(500, 350)), "the drop, one way: {l:?}");
    assert!(l.contains(&(850, 350)) && l.contains(&(350, 850)), "the ramp, both ways: {l:?}");
    assert_eq!(crate::place::waypoint_parts(&out).iter().filter(|&&p| p != 0).count(), 0);
    // The waypoints put in are never skipped past, the old ones as they were.
    let flags: Vec<bool> = (0..out.waypoints.len()).map(|w| out.waypoint_flags(w).walkdirect).collect();
    assert_eq!(flags, [false, false, false, false, true, true, true]);
    // And asked again, there is nothing to do.
    let (again, done) = repair(&out, &level, &legs);
    assert_eq!((done.joined, done.linked.len(), done.added, done.outside), (3, 0, 0, 0));
    assert_eq!(links(&again), l);
}

#[test]
fn seed_40s_ways_are_all_in_its_waypoints() {
    let b = built::make(&Seed::new(40), false).unwrap();
    let all = legs(&b, 1.0);
    // every ramp of the plan end to end, and every leg of every route but the two that must fail
    let ramps: usize = b.level.plan.rooms.iter().map(|r| r.ramps.len()).sum();
    assert_eq!(all.iter().filter(|l| l.what.contains(" ramp in ")).count(), ramps);
    assert!(all.len() > 400 && all.iter().all(|l| !l.what.starts_with('!')), "{} legs", all.len());

    // The level as the game loads it, and the waypoints as the maker makes them.
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets");
    let dir = std::env::temp_dir().join(format!("pd_import_autonav_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("recipe.json"), autolevel::recipe::save(&Seed::new(40))).unwrap();
    let w = SimWalk::open(&dir, &assets, SimWalk::res(&assets).unwrap()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let (made, _) = crate::place::generate_graph(&w.level);

    // Vents are crawled: the maker itself puts crouching waypoints in every one.
    for vent in b.level.plan.rooms.iter().filter(|r| r.kind == "vent") {
        let inside = (0..made.waypoints.len()).filter(|&k| made.waypoint_room(k) == Some(vent.room_id as u16)).collect::<Vec<_>>();
        assert!(!inside.is_empty() && inside.iter().all(|&k| made.waypoint_flags(k).crouch), "{}: {} waypoints", vent.key, inside.len());
    }

    let (out, done) = repair(&made, &w.level, &all);
    assert_eq!(done.legs, all.len());
    assert!(done.unwalked.is_empty(), "{:?}", done.unwalked);
    // What the maker leaves out of seed 40 (while its sampler takes a ramp's
    // foot for the floor under it): the ramp out of the Spire's pit and the
    // Cistern's, and the drop into the Cellar.
    println!("{}", done.line());
    assert!(done.linked.iter().all(|(what, one_way)| what.contains(" ramp in ") || (what.starts_with("drop_") && *one_way)), "{:?}", done.linked);
    // One part, every room that is walked in it, and nothing left to put right.
    let parts = crate::place::waypoint_parts(&out);
    assert!(parts.iter().all(|&p| p == 0), "{} waypoints outside the largest part", parts.iter().filter(|&&p| p != 0).count());
    assert_eq!(out.waypoints.len(), made.waypoints.len() + done.added - done.outside);
    // (61 since seed 40's spires have their bases, 2026-11-24, every leg still joined; 44 before)
    assert!(done.outside < 70, "{} left out", done.outside);
    for key in ["colossal", "basin", "small7", "spire2", "v01"] {
        let room = b.level.plan.get(key).unwrap().room_id as u16;
        assert!((0..out.waypoints.len()).any(|k| out.waypoint_room(k) == Some(room)), "no waypoint in {key}");
    }
    let (_, again) = repair(&out, &w.level, &all);
    assert_eq!((again.joined, again.linked.len(), again.added, again.outside, again.unwalked.len()), (all.len(), 0, 0, 0, 0));
}

#[test]
fn a_doorways_mouth_is_cleared_and_its_axis_given_waypoints() {
    // Felicity 10, as the importer makes it: the maker's waypoints, then the doorways
    let mut seed = Seed::new(10);
    seed.options.style = "felicity";
    let b = built::make(&seed, false).unwrap();
    let gates = gates(&b, 1.0);
    assert_eq!(gates.len(), b.level.doorways.len());
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets");
    let dir = std::env::temp_dir().join(format!("pd_import_autonav_fel_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("recipe.json"), autolevel::recipe::save(&seed)).unwrap();
    let w = SimWalk::open(&dir, &assets, SimWalk::res(&assets).unwrap()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let (made, _) = crate::place::generate_graph(&w.level);
    let (out, done) = doorways(&made, &w.level, &gates);
    println!("{}", done.line());
    assert!(done.doorways * 10 >= gates.len() * 9, "{} of {} doorways", done.doorways, gates.len());
    assert!(done.out > 0);
    let floor = |k: usize| out.waypoint_pos(k) - Vec3::Y * PAD_HEIGHT;
    let linked: std::collections::HashSet<(usize, usize)> = directed_links(out.waypoints.len(), |k| &out.waypoints[k].neighbours).into_iter().map(|(a, b, _)| (a, b)).collect();
    let mut given = 0;
    for g in &gates {
        let near: Vec<usize> = (0..out.waypoints.len())
            .filter(|&k| {
                let d = floor(k) - g.mid;
                d.dot(g.through).abs() <= g.half_thick + MOUTH && d.dot(g.across).abs() <= g.half_wide + 20.0 && d.y.abs() <= LEVEL
            })
            .collect();
        // its own two: on its axis either side, never skipped, linked through it (a doorway the
        // probe can't walk straight through, a stair's foot, is left as it was)
        let axis: Vec<usize> = (0..out.waypoints.len())
            .filter(|&k| {
                let d = floor(k) - g.mid;
                out.waypoint_flags(k).walkdirect && d.dot(g.across).abs() < 1.0 && d.y.abs() <= 10.0 && [150.0, 100.0, 70.0].iter().any(|&r| (d.dot(g.through).abs() - r).abs() < 1.0)
            })
            .collect();
        if axis.len() == 2 {
            // nothing in its mouth off its axis, and none in the opening itself (a door's plane)
            for &k in &near {
                let d = floor(k) - g.mid;
                assert!(d.dot(g.across).abs() < 1.0 && d.dot(g.through).abs() >= 60.0, "a waypoint at {d:?} from a doorway's middle");
            }
            given += 1;
            let (p, q) = (axis[0], axis[1]);
            assert!(linked.contains(&(p, q)) && linked.contains(&(q, p)), "a doorway's two waypoints not linked through it");
        }
    }
    assert_eq!(given, done.doorways);
    // a level with no doorways is given back as it was
    let (same, none) = doorways(&made, &w.level, &[]);
    assert_eq!((none.doorways, same.waypoints.len()), (0, made.waypoints.len()));
}

/// How well PD's router walks a converted level's graph under other
/// waygroups: for random pairs of waypoints, the walk a simulant makes (a
/// route of `MAX_CHRWAYPOINTS`, walked to its end, routed again) against the
/// shortest walk along the links, and the time it takes. On an imported
/// stage (`PD_STAGE`, `autolevel40` by default, from `custom/`):
/// `cargo test -p pd_import --release probe_waygroups -- --ignored --nocapture`.
#[test]
#[ignore]
fn probe_waygroups() {
    use pd_core::rng::Rng;
    use pd_sim::nav::gen::{dijkstra, regroup};
    use pd_sim::nav::{NavSeed, MAX_CHRWAYPOINTS};
    use std::path::Path;
    let code = std::env::var("PD_STAGE").unwrap_or_else(|_| "autolevel40".to_string());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let assets = pd_core::assets::AssetDir::new(root.join("assets")).with_custom_dir(root.join("custom"));
    let stage = pd_sim::stage::Stage::load(&assets, &code).unwrap();
    let level = TileLevel::for_stage(&stage);
    let base = NavGraph::from_stage(&stage, &level);
    let n = base.waypoints.len();
    let pos: Vec<Vec3> = (0..n).map(|w| base.waypoint_pos(w)).collect();
    let mut adj = vec![Vec::new(); n];
    for (a, b, _) in base.links() {
        adj[a].push((b, pos[a].distance(pos[b])));
    }
    let mut rng = Rng::new(7);
    let pairs: Vec<(usize, usize)> = (0..400).map(|_| (rng.random() as usize % n, rng.random() as usize % n)).filter(|(a, b)| a != b).collect();
    let shortest: Vec<f32> = pairs.iter().map(|&(a, b)| dijkstra(&adj, &[a], f32::INFINITY)[b].0).collect();
    println!("{code}: {n} waypoints, {} pairs", pairs.len());
    let variants: Vec<(String, NavGraph)> = vec![
        ("as imported".to_string(), base.clone()),
        ("2 m".to_string(), regroup(&base, 200.0, false)),
        ("4 m, by room".to_string(), regroup(&base, 400.0, true)),
        ("6 m, by room".to_string(), regroup(&base, 600.0, true)),
        ("8 m, by room".to_string(), regroup(&base, 800.0, true)),
        ("12 m, by room".to_string(), regroup(&base, 1200.0, true)),
        ("a room each".to_string(), regroup(&base, f32::INFINITY, true)),
        ("8 m".to_string(), regroup(&base, 800.0, false)),
    ];
    for (name, g) in &variants {
        let t = std::time::Instant::now();
        let (mut ratios, mut stuck, mut routes) = (Vec::new(), 0, 0usize);
        for (k, &(a, b)) in pairs.iter().enumerate() {
            if !shortest[k].is_finite() {
                continue;
            }
            let (mut cur, mut len, mut done) = (a, 0.0f32, false);
            for _ in 0..2000 {
                if cur == b {
                    done = true;
                    break;
                }
                let (r, _) = g.nav_find_route(cur, b, MAX_CHRWAYPOINTS, NavSeed(1 + k as u32, 3), &mut rng);
                routes += 1;
                if r.len() < 2 {
                    break;
                }
                len += r.windows(2).map(|w| pos[w[0]].distance(pos[w[1]])).sum::<f32>();
                cur = *r.last().unwrap();
            }
            if done {
                ratios.push(len / shortest[k].max(1.0));
            } else {
                stuck += 1;
            }
        }
        ratios.sort_by(f32::total_cmp);
        let q = |f: f32| ratios.get(((ratios.len() as f32 - 1.0) * f) as usize).copied().unwrap_or(f32::NAN);
        let sizes: Vec<usize> = g.waygroups.iter().map(|w| w.waypoints.len()).collect();
        println!(
            "{name:>14}: {:4} groups ({:.1} each, largest {:3}); walk / shortest: median {:.2}, 90% {:.2}, worst {:.2}; {stuck} stuck; {routes} routes in {:.0} ms ({:.1} us each)",
            g.waygroups.len(),
            n as f32 / g.waygroups.len() as f32,
            sizes.iter().max().unwrap_or(&0),
            q(0.5),
            q(0.9),
            q(1.0),
            t.elapsed().as_secs_f64() * 1000.0,
            t.elapsed().as_secs_f64() * 1e6 / routes.max(1) as f64
        );
    }
}
