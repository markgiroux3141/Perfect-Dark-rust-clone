//! Placement on generated levels: what the generator's floors say, and what
//! the guided placement does with it on seed 40.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;
use crate::autowalk::{self, Via};
use crate::recipe::AutolevelSource;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets")
}

fn built(seed: u64, options: serde_json::Value) -> std::sync::Arc<Built> {
    let s = AutolevelSource { recipe: json!({"format": 1, "seed": seed, "options": options}), sky: None, plain_waypoints: false, plain_placement: false, theme: None };
    crate::autolevel::make(&s, None).unwrap()
}

#[test]
fn the_generator_says_where_nothing_goes_and_where_the_risky_places_are() {
    let b = built(40, json!({}));
    let h = hints(&b, 1.0);
    // every plan room's kind, by its number
    assert_eq!(h.kinds.len(), b.level.plan.rooms.len() + 1);
    assert!(b.level.plan.rooms.iter().all(|r| h.kinds[r.room_id] == r.kind));
    assert!(h.kinds.contains(&"corridor") && h.kinds.contains(&"hub"));
    // a zone for every ramp of the plan, and for every opening
    let ramps: usize = b.level.plan.rooms.iter().map(|r| r.ramps.len()).sum();
    assert_eq!(h.clear.iter().filter(|z| z.what == "ramp").count(), ramps);
    assert_eq!(h.clear.iter().filter(|z| z.what == "doorway" || z.what == "hole").count(), b.openings().len());
    // a ramp's zone holds its foot and its top
    let rp = b.level.plan.rooms.iter().flat_map(|r| &r.ramps).next().unwrap();
    for p in [rp.start, rp.end] {
        assert!(h.clear.iter().any(|z| z.what == "ramp" && z.holds(pd_point(p, 1.0))), "{p:?}");
    }
    // the default level's riskiest places are its pillars' tops
    assert!(!h.risky.is_empty() && h.risky[0].what == "pillar top", "{:?}", h.risky.first());

    // with the options that make them, islands first, then the rest in order
    // (seed 2's big hall takes an island; platforms would fill its air first)
    let b = built(2, json!({"angled": 1.0, "cellars": 1.0}));
    let h = hints(&b, 1.0);
    let kinds: Vec<&str> = h.risky.iter().map(|z| z.what).fold(Vec::new(), |mut v, w| {
        if v.last() != Some(&w) {
            v.push(w);
        }
        v
    });
    assert_eq!(kinds.first(), Some(&"island"), "{kinds:?}");
    for k in ["pillar top", "cellar"] {
        assert!(kinds.contains(&k), "{k} in {kinds:?}");
    }
    let order: Vec<usize> = kinds.iter().map(|k| RISKY.iter().position(|w| w == k).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{kinds:?}: each kind once, the riskiest first");
}

#[test]
fn seed_40_is_placed_on_flat_floor_out_of_corridors_with_its_ammo_by_it() {
    let custom = std::env::temp_dir().join(format!("pd_import_autoplace_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&custom);
    let paths = crate::Paths { assets: assets(), custom: custom.clone(), src: None, levels: None };
    let r = autowalk::recipe("autolevel40", "Autolevel 40", Via::Recipe("{\"format\": 1, \"seed\": 40}")).unwrap();
    let report = crate::import(&r, &paths, &crate::layout::Layout::default()).unwrap();
    std::fs::remove_dir_all(&custom).unwrap();
    let line = |start: &str| report.iter().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no {start:?} in {report:#?}")).clone();
    assert_eq!(line("spawns:"), "spawns: 16 out of corridors, ramps, vents and the risky places (0 let into corridors)");
    let weapons = line("weapons:");
    assert!(weapons.starts_with("weapons: 10 locations, location 5 on a pillar top, ") && !weapons.ends_with(" 0 in dead ends"), "{weapons}");
    let audit = line("placement audit:");
    assert!(audit.starts_with("placement audit: 0 of 46 items off flat floor, 0 on a ramp or in a doorway or by a hole, 0 of 16 spawns in corridors"), "{audit}");
    assert!(audit.ends_with("location 5 on pillar top"), "{audit}");
    // the crates a short walk from their weapons: 5.2 m on average, 15 m at most
    let num = |after: &str| audit.split(after).nth(1).and_then(|t| t.split(' ').next()).and_then(|n| n.trim_start_matches('(').parse::<f32>().ok()).unwrap();
    assert!(num("ammo ") < 600.0 && num("on average (") < 1600.0, "{audit}");
    // every crate in its weapon's room; every spawn within a 25 m walk of a
    // weapon (89 m when the spawns were placed before the weapons); the hills in
    // halls and rooms, the bases in rooms with two ways in
    assert!(audit.contains("; 0 of 20 crates not in their weapon's room"), "{audit}");
    let fair = line("placement fairness:");
    assert!(num_before(&fair, " cm (") <= 2500.0, "{fair}");
    assert!(fair.contains(", at most 1 weapons in a room, 0 of 5 hills not in a hall or a room, 0 of 4 bases with one way in"), "{fair}");
    for start in ["hills: 5 in rooms", "capture the case: bases at", "cover: 120 spots"] {
        line(start);
    }
}

/// The number just before the first `before` in `line`.
fn num_before(line: &str, before: &str) -> f32 {
    line.split(before).next().and_then(|t| t.rsplit(' ').next()).and_then(|n| n.parse::<f32>().ok()).unwrap_or_else(|| panic!("no number before {before:?} in {line}"))
}

/// The placement lines of any generated level, imported into a scratch
/// folder: `PD_RECIPE='{"format": 1, "seed": 2, "options": {"angled": 1}}'
/// cargo test -p pd_import --release probe_placement -- --ignored --nocapture`.
#[test]
#[ignore]
fn probe_placement() {
    let text = std::env::var("PD_RECIPE").unwrap_or_else(|_| "{\"format\": 1, \"seed\": 40}".to_string());
    let custom = std::env::temp_dir().join(format!("pd_import_autoplace_probe_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&custom);
    let paths = crate::Paths { assets: assets(), custom: custom.clone(), src: None, levels: None };
    let r = autowalk::recipe("autolevel_probe", "Probe", Via::Recipe(&text)).unwrap();
    let report = crate::import(&r, &paths, &crate::layout::Layout::default()).unwrap();
    std::fs::remove_dir_all(&custom).unwrap();
    for l in report.iter().filter(|l| ["placement", "spawns", "weapons", "ammo", "reachable"].iter().any(|s| l.starts_with(s))) {
        println!("{l}");
    }
}
