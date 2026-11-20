//! [`SimWalk`] on a level made here: seed 40 generated, built, baked and
//! written into a temporary folder with the game's textures, as `autolevel
//! build 40` does it, then walked on the tiles the importer makes of it: by
//! the source for generated levels, and by the glTF source the same.

use super::*;
use autolevel_build::cli;
use pd_core::ids::{GEOFLAG_FLOOR1, GEOFLAG_FLOOR2, GEOFLAG_RAMPWALL};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The spots a level's floors are stood on: every floor tile's middle, and
/// its corners pulled a twentieth of the way in.
fn spots(level: &TileLevel) -> Vec<Vec3> {
    let mut out = Vec::new();
    for p in level.geom.polys.iter().filter(|p| p.floor) {
        let mid = p.verts.iter().copied().sum::<Vec3>() / p.verts.len() as f32;
        for q in std::iter::once(mid).chain(p.verts.iter().map(|&v| v + (mid - v) * 0.05)) {
            out.push(Vec3::new(q.x, p.find_y(q.x, q.z), q.z));
        }
    }
    out
}

/// The highest floor under (x, z) below `eye`, looking at every floor tile.
fn floor_below(level: &TileLevel, x: f32, eye: f32, z: f32) -> f32 {
    let over = |p: &&pd_sim::stage::GeomPoly| p.floor && p.verts.iter().any(|v| v.x <= x) && p.verts.iter().any(|v| v.x >= x) && p.verts.iter().any(|v| v.z <= z) && p.verts.iter().any(|v| v.z >= z) && p.xz_in_convex(x, z);
    level.geom.polys.iter().filter(over).map(|p| p.find_y(x, z)).filter(|&y| y < eye).fold(f32::MIN, f32::max)
}

#[test]
fn seed_40_is_built_and_every_route_walks_in_this_process() {
    let root = repo();
    let assets = root.join("assets");
    let dir = std::env::temp_dir().join(format!("pd_import_autowalk_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let res = SimWalk::res(&assets).expect("assets/");

    // `autolevel build 40`, walking with this walker as the program does.
    let words: Vec<String> = ["build", "40", "--flooded", "0", "--shaped", "0", "--sunken", "0", "--out", &dir.to_string_lossy(), "--root", &root.to_string_lossy(), "--jobs", "4"].map(str::to_owned).to_vec();
    let args = cli::parse(&words, &|path| Err(format!("{path}: no recipes here"))).unwrap();
    let env = cli::Env { cwd: root.clone(), pd_walk: None };
    let open = |d: &Path| -> Result<Box<dyn Walker>, String> { Ok(Box::new(SimWalk::open(d, &assets, res.clone())?)) };
    assert_eq!(cli::run(&args, &env, Some(&open)), Ok(true), "seed 40 is built, and every walk comes out as expected");
    let results: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("walk_results.json")).unwrap()).unwrap();
    assert!(results.as_object().is_some_and(|r| r.len() == 76 && r.values().all(|ok| ok == &Value::Bool(true))), "{results}");

    // The level as the game loads it, and nothing left behind in the folder.
    let w = SimWalk::open(&dir, &assets, res.clone()).unwrap();
    assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".simwalk")));
    let polys = &w.level.geom.polys;
    // (a tile for each triangle that is one, in the generator's 61 rooms and PD's room 0)
    assert_eq!((polys.len(), w.stage.rooms.roomcount(), w.stage.rooms.portals.len()), (12860, 62, 91));
    let flags = |f: u32| polys.iter().filter(|p| p.geoflags == f).count();
    let (grates, ramp_walls) = (flags(GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2), polys.iter().filter(|p| p.geoflags & GEOFLAG_RAMPWALL != 0).count());
    let low = polys.iter().filter(|p| p.wall && p.normal.y < -0.49).count();
    assert!(grates > 0 && ramp_walls > 0 && low > 0, "{grates} grate tiles, {ramp_walls} ramp walls, {low} low ceilings");

    // Every route, on threads: the ones named `!...` (glass, water) stop short, the rest arrive.
    let routes = cli::routes_from(&std::fs::read_to_string(dir.join("walks.json")).unwrap()).unwrap();
    let outcomes = walk::walk_all(&w, &routes, 4);
    assert_eq!(outcomes.len(), 76);
    for (o, (name, pts)) in outcomes.iter().zip(&routes) {
        let walked = o.walked.as_ref().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(walked.reached, !walk::must_fail(name), "{}", cli::outcome_line(o, pts));
        if walked.reached {
            assert_eq!(walked.reports.len(), pts.len() - 1, "{name}: a report for each point after the first");
        }
    }
    let must_fail: Vec<&str> = routes.iter().map(|(n, _)| n.as_str()).filter(|n| walk::must_fail(n)).collect();
    assert_eq!(must_fail, ["!window_c26b_medium1", "!pool_basin"]);
    // The folder's .glb through the glTF source (`autolevel import --gltf`: rooms split from
    // its footprint, tiles cut to them) walks the same: every route the same outcome, every
    // point reached at the same height.
    let g = SimWalk::open_gltf(&dir, &assets, res.clone()).unwrap();
    assert!(g.level.geom.polys.len() > polys.len() && g.stage.rooms.roomcount() != w.stage.rooms.roomcount());
    for ((o, og), (name, _)) in outcomes.iter().zip(&walk::walk_all(&g, &routes, 4)).zip(&routes) {
        let (a, b) = (o.walked.as_ref().unwrap(), og.walked.as_ref().unwrap());
        assert_eq!((a.reached, a.reports.len()), (b.reached, b.reports.len()), "{name}");
        for (p, q) in a.reports.iter().zip(&b.reports) {
            assert_eq!((&p["reached"], &p["want"]), (&q["reached"], &q["want"]), "{name}");
            assert!((p["feet"].as_f64().unwrap() - q["feet"].as_f64().unwrap()).abs() <= 1.0, "{name}: {p} against {q}");
        }
    }
    // A report is pd-walk's: the point, how close, the feet's height and the point's, the room.
    let first = &outcomes[0].walked.as_ref().unwrap().reports[0];
    assert_eq!((first["i"].as_u64(), first["reached"].as_bool(), first["want"].as_f64()), (Some(1), Some(true), Some(0.0)), "{first}");
    assert!(first["dist"].as_f64().is_some_and(|d| d < 40.0) && first["feet"].as_f64().is_some_and(|f| f.abs() < 8.0) && first["room"].as_str().is_some_and(|r| r.starts_with("room ")), "{first}");
    assert_eq!(w.walk(&routes[0].1[..1]).unwrap_err(), "a route is at least two points");

    // The ground under a player is the floor stood on, everywhere a floor is (by a corner under
    // stacked catwalks more than 20 floor tiles are in reach: pd_sim's ground search keeps the nearest).
    let all = spots(&w.level);
    let wrong: Vec<(Vec3, f32)> =
        all.iter().map(|&q| (q, w.level.cd_find_ground_at_cyl(q + Vec3::Y * 159.0, 30.0).0)).filter(|&(q, got)| (got - floor_below(&w.level, q.x, q.y + 159.0, q.z)).abs() > 0.5).collect();
    assert!(wrong.is_empty(), "{} of {} spots where the ground is not the floor under the player: {:?}", wrong.len(), all.len(), &wrong[..wrong.len().min(5)]);

    // In a vent nobody stands up (its ceiling, 118 up, is a wall tile): a squat rises to a duck, the
    // eye 114 up, and no further. In the room it stands.
    let (name, vent) = routes.iter().find(|(n, pts)| n.starts_with("vent_") && pts.iter().all(|p| p.z == pts[0].z)).expect("a level vent");
    let up = PlayerInput { crouch_up: true, ..PlayerInput::default() };
    let stands = |at: &Point| {
        let mut lone = w.lone(w.settle(world(at)), 0.0).unwrap();
        lone.p.crouchpos = CROUCHPOS_SQUAT;
        lone.p.set_crouch_offset(-90.0);
        for _ in 0..2 {
            lone.tick(&up);
            for _ in 0..60 {
                lone.tick(&PlayerInput::default());
            }
        }
        (lone.p.crouchpos, lone.p.crouchoffset)
    };
    assert_eq!(stands(&vent[vent.len() / 2]), (pd_core::ids::CROUCHPOS_DUCK, -45.0), "{name}: inside the vent");
    assert_eq!(stands(&vent[0]), (CROUCHPOS_STAND, 0.0), "{name}: its room's floor");

    // No renderer: the answer says what draws the level.
    let view = View { x: 21.5, y: 15.5, z: 0.0, tx: 13.0, ty: 28.5 };
    let why = w.shot(&view, &dir.join("shot.png")).unwrap_err();
    assert!(why.contains("pd_snapshot") && why.contains("--at 2539,0,-1831,146.8") && why.contains("--pd-walk"), "{why}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_generated_levels_recipe_holds_the_generators_own() {
    use crate::recipe::Source;
    // as `autolevel build` writes a seed's recipe, and one with options and an edit
    let plain = autolevel::recipe::save(&autolevel::recipe::Recipe::new(40));
    let text = recipe_text("autolevel40", "Autolevel 40", Via::Recipe(&plain));
    assert!(text.starts_with("{\n  \"format\": \"pd-import-recipe/1\",\n  \"code\": \"autolevel40\",\n  \"name\": \"Autolevel 40\",\n  \"group\": \"Autolevel\",\n  \"scale\": 1.0,\n") && text.ends_with("}\n"), "{text}");
    assert!(text.contains("  \"source\": {\n    \"game\": \"autolevel\",\n    \"recipe\": {\n      \"format\": 1,\n      \"seed\": 40"), "{text}");
    let r = recipe("autolevel40", "Autolevel 40", Via::Recipe(&plain)).unwrap();
    assert_eq!((r.code.as_str(), r.name.as_str(), r.group.as_deref(), r.scale), ("autolevel40", "Autolevel 40", Some(GROUP), 1.0));
    // no split to ask for: the rooms are the level's own
    assert_eq!((r.rooms, r.spawns, r.weapons, r.hills), (0, 16, 10, 5));
    let Source::Autolevel(a) = &r.source else { panic!("an autolevel source") };
    assert_eq!((a.sky, a.plain_waypoints), (None, false));
    // the recipe in it is the generator's, read back whole
    let back = autolevel::recipe::load(&crate::autolevel::recipe_text(a, None).unwrap()).unwrap();
    assert_eq!(back, autolevel::recipe::Recipe::new(40));
    let edited = "{ \"format\": 1, \"seed\": 7, \"options\": { \"cover\": \"pillars\" },\n  \"edits\": [ { \"edit\": \"keep\", \"room\": \"spire1\" } ] }\n";
    let Source::Autolevel(a) = recipe("x", "x", Via::Recipe(edited)).unwrap().source else { panic!("an autolevel source") };
    assert_eq!(autolevel::recipe::load(&crate::autolevel::recipe_text(&a, None).unwrap()).unwrap(), autolevel::recipe::load(edited).unwrap());
    // a recipe may name the generator's by its file or its build folder instead, and --src does
    let dir = std::env::temp_dir().join(format!("pd_import_autowalk_recipe_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("recipe.json"), &plain).unwrap();
    let by_path = crate::recipe::AutolevelSource { recipe: Value::String(dir.to_string_lossy().into_owned()), sky: None, plain_waypoints: false, plain_placement: false };
    assert_eq!(crate::autolevel::recipe_text(&by_path, None).unwrap(), plain);
    assert_eq!(crate::autolevel::recipe_text(&a, Some(&dir.join("recipe.json"))).unwrap(), plain);
    assert_eq!(built_recipe(&dir).unwrap(), (plain.clone(), 40));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(built_recipe(&dir).unwrap_err().starts_with("no built level in "));
    assert!(crate::autolevel::recipe_text(&by_path, None).unwrap_err().contains("autolevel build SEED"));
    // a name with a quote in it is still JSON
    assert_eq!(recipe("x", "The \"Pit\"", Via::Recipe(&plain)).unwrap().name, "The \"Pit\"");
}

#[test]
fn a_generated_levels_gltf_recipe_turns_its_tile_rules_on() {
    use crate::recipe::Source;
    let r = recipe("autolevel40", "Autolevel 40", Via::Gltf("out/autolevel/seed40/autolevel_40.glb")).unwrap();
    assert_eq!((r.code.as_str(), r.group.as_deref(), r.rooms, r.spawns, r.weapons, r.hills), ("autolevel40", Some(GROUP), 60, 16, 10, 5));
    let Source::Gltf(g) = &r.source else { panic!("a glTF source") };
    assert_eq!((g.path.as_str(), g.grates, g.low_ceilings, g.ramp_walls, g.climb_ledges), ("out/autolevel/seed40/autolevel_40.glb", true, true, true, false));
    assert_eq!(recipe("x", "x", Via::Gltf("C:\\levels\\x.glb")).unwrap().code, "x");
}

#[test]
fn a_folder_with_no_level_says_how_to_build_one() {
    let dir = std::env::temp_dir().join(format!("pd_import_autowalk_none_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let why = built_glb(&dir).unwrap_err();
    assert!(why.starts_with("no built level in ") && why.contains("autolevel build SEED --out"), "{why}");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(built_glb(&dir).unwrap_err().starts_with("no built level in "));
    std::fs::write(dir.join("autolevel_7.glb"), b"").unwrap();
    assert_eq!(built_glb(&dir).unwrap(), (dir.join("autolevel_7.glb"), "7".to_string()));
    std::fs::write(dir.join("autolevel_8.glb"), b"").unwrap();
    assert!(built_glb(&dir).unwrap_err().ends_with("2 levels (autolevel_*.glb), one is expected"));
    std::fs::remove_dir_all(&dir).unwrap();
}
