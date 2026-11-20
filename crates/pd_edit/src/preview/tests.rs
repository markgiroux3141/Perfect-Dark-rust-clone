//! A preview made of seed 40 (no GPU: the stage and its tiles as the game
//! would load them), walked on by PD's player, made again after an edit, and
//! what it leaves on the disk. Seed 40's places are the generator's own
//! views (`colossal_door`, `corridor_c09`, ...: a seed's level never changes
//! by accident, so they stay where they are).

use std::time::SystemTime;

use super::*;
use autolevel::recipe::Edit;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn assets() -> PathBuf {
    repo().join("assets")
}

/// A folder of this test's own for its previews, empty.
fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pd_edit_preview_test_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn seed40(root: &Path) -> Preview {
    Preview::make(&gen::Recipe::python(40), &assets(), root).expect("seed 40 makes a level")
}

fn res() -> Arc<WorldRes> {
    pd_import::autowalk::SimWalk::res(&assets()).expect("assets/")
}

/// How many folders `root` holds.
fn left(root: &Path) -> usize {
    std::fs::read_dir(root).map_or(0, |d| d.count())
}

fn idle() -> PlayerInput {
    PlayerInput::default()
}

fn forward() -> PlayerInput {
    PlayerInput { walk_y: 127, ..PlayerInput::default() }
}

/// A walker dropped at one of the generator's views, as "Walk" drops one
/// from a camera there.
fn at_view(pv: &Preview, res: &Arc<WorldRes>, name: &str) -> Walker {
    let cam = pv.view_camera(name).unwrap_or_else(|| panic!("no view {name}"));
    Walker::drop_at(pv, res.clone(), cam.pos, cam.yaw, 0.0).unwrap()
}

fn walk(w: &mut Walker, pv: &Preview, ticks: usize) {
    for _ in 0..ticks {
        w.step(pv, &forward());
    }
}

#[test]
fn a_preview_is_made_from_a_recipe_and_loads_from_a_scratch_folder_that_goes_with_it() {
    let root = root("made");
    let pv = seed40(&root);
    // the generator's 61 rooms (and PD's room 0), its 91 openings, a tile for each triangle that is one:
    // what `autolevel import` makes of seed 40 (pd_import::autowalk's test)
    assert_eq!((pv.stage.rooms.roomcount(), pv.stage.rooms.portals.len(), pv.level.geom.polys.len()), (62, 91, 12860));
    assert!(pv.line().starts_with("61 rooms, 91 portals, 12860 tiles, made in "), "{}", pv.line());
    // no waypoints, no spawns: the geometry alone
    assert!(pv.stage.waypoints.is_empty() && pv.stage.spawn_pads.is_empty());
    // the level's own rooms and views
    assert_eq!(pv.rooms.len(), 29);
    let colossal = pv.rooms.iter().find(|r| r.key == "colossal").unwrap();
    assert_eq!((colossal.rect, colossal.floor, colossal.top, colossal.host.as_str()), ((3, 15, 23, 42), 0, 1063, ""));
    assert_eq!(pv.views.first().map(|(n, _)| n.as_str()), Some("colossal_door"));
    // the view opens in the biggest hall, standing at a door at eye height, looking in
    let cam = pv.start_camera();
    assert!((cam.pos - world((21.5, 15.5), EYE)).length() < 0.01 && cam.pitch == 0.0, "{cam:?}");
    assert_eq!(pv.room_at(cam.pos).map(|r| r.key.as_str()), Some("colossal"));
    assert_eq!(pv.floor_under(cam.pos), Some(0.0));
    let ahead = cam.pos + cam.look() * 500.0;
    assert_eq!(pv.room_at(ahead).map(|r| r.key.as_str()), Some("colossal"));
    // the scratch folder is a custom tree with the one stage in it, under the root, and no list of levels
    let stage = pv.dir().join("stages").join(CODE);
    assert!(pv.dir().starts_with(&root) && left(&root) == 1);
    for f in ["bg.json", "bg.bin", "tiles.json", "pads.json", "setup.json"] {
        assert!(stage.join(f).is_file(), "{f}");
    }
    assert!(!pv.dir().join("levels.json").exists());
    // laid over assets/ as custom/ is: the stage's files come from the scratch
    let over = pv.asset_dir();
    assert_eq!(over.custom_dir(), Some(pv.dir()));
    assert!(Stage::load(&over, CODE).is_ok());
    // dropped: nothing is left
    let dir = pv.dir().to_path_buf();
    drop(pv);
    assert!(!dir.exists() && left(&root) == 0);
    // a recipe that makes no level (seed 25 with no rescue) leaves nothing either, and says why in
    // the generator's words
    let mut none = gen::Recipe::new(25);
    none.options.rescue = false;
    let why = Preview::make(&none, &assets(), &root).err().unwrap();
    assert_eq!(why, "seed 25: colossal: no set of ramps joins its storeys");
    assert_eq!(left(&root), 0);
    let _ = std::fs::remove_dir_all(&root);
}

/// Every file under `dir` with its size and when it was written.
fn listing(dir: &Path, out: &mut Vec<(PathBuf, u64, Option<SystemTime>)>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        match e.metadata() {
            Ok(m) if m.is_dir() => {
                out.push((path.clone(), 0, None));
                listing(&path, out);
            }
            Ok(m) => out.push((path, m.len(), m.modified().ok())),
            Err(_) => {}
        }
    }
}

#[test]
fn a_preview_leaves_custom_and_the_levels_folder_as_they_were() {
    // (the repo's own, and $PD_CUSTOM if the custom tree is elsewhere)
    let mut watched = vec![repo().join("custom"), repo().join("crates").join("pd_import").join("levels")];
    watched.extend(std::env::var_os("PD_CUSTOM").map(PathBuf::from));
    let look = || {
        let mut all = Vec::new();
        for dir in &watched {
            listing(dir, &mut all);
        }
        all.sort();
        all
    };
    let before = look();
    assert!(before.iter().any(|(p, ..)| p.extension().is_some_and(|x| x == "json")), "the levels folder has recipes in it");
    let root = root("untouched");
    let pv = seed40(&root);
    // made again after an edit, and through the thread the window uses
    let mut edited = gen::Recipe::python(40);
    edited.edits.push(Edit::Layout { room: "colossal".into(), layout: "ring".into() });
    let job = Job::spawn(edited, assets(), root.clone());
    let ring = loop {
        match job.poll() {
            Some(made) => break made.unwrap(),
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    assert!(ring.level.geom.polys.len() > pv.level.geom.polys.len());
    drop((pv, ring));
    assert!(look() == before, "custom/ and crates/pd_import/levels/ are as they were");
    assert_eq!(left(&root), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_walker_stands_on_the_floor_walks_a_corridor_and_is_stopped_by_a_wall_and_a_rail() {
    let root = root("walk");
    let (pv, res) = (seed40(&root), res());

    // Dropped from a camera a metre over head height in the hall: on its floor, at PD's eye
    // height, standing still. (From higher it is the catwalk over the door, at 276, it lands on.)
    let door = pv.view_camera("colossal_door").unwrap();
    let onto = |up: f32| Walker::drop_at(&pv, res.clone(), door.pos + Vec3::Y * up, door.yaw, -0.3).unwrap();
    assert!((onto(300.0).feet().y - 276.0).abs() < 0.01);
    let mut w = onto(100.0);
    assert!((w.feet() - world((21.5, 15.5), 0.0)).length() < 0.01, "{}", w.line());
    assert!(w.line().starts_with("feet (2539, 0, -1831) θ 147, standing"), "{}", w.line());
    // it looks the way the camera did: the fly camera at its eyes is that camera again
    let back = w.camera();
    let turn = (back.yaw - door.yaw).rem_euclid(std::f32::consts::TAU);
    assert!(turn.min(std::f32::consts::TAU - turn) < 0.01 && (back.pitch + 0.3).abs() < 0.01, "{back:?}");
    for _ in 0..60 {
        w.step(&pv, &idle());
    }
    assert!((w.feet() - world((21.5, 15.5), 0.0)).length() < 1.0 && !w.p.isfalling, "{}", w.line());
    assert!((w.p.pos.y - EYE).abs() < 15.0 && w.stance() == "standing", "eye at {}", w.p.pos.y);
    // and walks: two seconds forward are some 8 m into the hall, on its floor
    walk(&mut w, &pv, 120);
    let gone = (w.feet() - world((21.5, 15.5), 0.0)).length();
    assert!((600.0..1200.0).contains(&gone) && w.feet().y.abs() < 1.0, "{gone} cm: {}", w.line());
    assert_eq!(pv.room_at(w.feet()).map(|r| r.key.as_str()), Some("colossal"));

    // No floor under a camera outside the level: no walker, and the words for it.
    let none = Walker::drop_at(&pv, res.clone(), pv.lo - Vec3::splat(500.0), 0.0, 0.0).err();
    assert_eq!(none.as_deref(), Some("no floor under the camera: fly over a floor first"));

    // Along a corridor (c09, east from cell 29.5, 51.5): three cells on, its end wall stops the walker
    // a player's radius short of it (the wall is at cell 33), and holds it there.
    let mut w = at_view(&pv, &res, "corridor_c09");
    let start = cell(w.feet());
    assert!((start.0 - 29.5).abs() < 0.01 && (start.1 - 51.5).abs() < 0.01);
    walk(&mut w, &pv, 30);
    let early = cell(w.feet());
    assert!(early.0 > start.0 + 0.8 && early.0 < 32.0, "half a second in: {early:?}");
    walk(&mut w, &pv, 150);
    let stopped = cell(w.feet());
    walk(&mut w, &pv, 120);
    let held = cell(w.feet());
    for c in [stopped, held] {
        assert!((c.0 - 32.75).abs() < 0.03 && (c.1 - 51.5).abs() < 0.1, "{c:?}");
    }
    assert!(w.feet().y.abs() < 1.0 && !w.p.isfalling);

    // On the hall's catwalk at 551 (one cell wide, x 22 to 23), walking south-west into its rail:
    // the rail holds the walker on the deck (a radius from it) while it slides south along it.
    let mut w = at_view(&pv, &res, "colossal_deck551");
    assert!((w.feet().y - 551.0).abs() < 0.01);
    walk(&mut w, &pv, 180);
    let c = cell(w.feet());
    assert!((c.0 - 22.25).abs() < 0.03 && c.1 > 24.0, "{c:?}");
    assert!((w.feet().y - 551.0).abs() < 1.0 && !w.p.isfalling, "{}", w.line());
    // (past the rail there is no deck: the hall's floor, 551 down)
    assert_eq!(pv.floor_under(world((21.5, c.1), 560.0)), Some(0.0));

    // A vent (v02, off the hall's east wall at cell 23): standing, its mouth stops the walker;
    // squatting (two presses of the crouch key), it crawls through.
    let mut w = at_view(&pv, &res, "vent_v02_colossal");
    walk(&mut w, &pv, 240);
    assert!((cell(w.feet()).0 - 22.75).abs() < 0.03, "standing: {:?}", cell(w.feet()));
    let mut w = at_view(&pv, &res, "vent_v02_colossal");
    w.advance(&pv, Controls { crouch_down: true, ..Controls::default() }, 1.0 / 60.0);
    assert_eq!(w.stance(), "ducked");
    w.advance(&pv, Controls { crouch_down: true, ..Controls::default() }, 1.0 / 60.0);
    assert_eq!(w.stance(), "squatting");
    for _ in 0..40 {
        w.step(&pv, &idle());
    }
    assert!(w.p.pos.y < 80.0, "the squatting eye is at {}", w.p.pos.y);
    walk(&mut w, &pv, 240);
    let c = cell(w.feet());
    assert!(c.0 > 25.0 && (c.1 - 28.5).abs() < 0.1 && w.feet().y.abs() < 1.0, "squatting: {c:?}");
    // and Space stands up a level
    w.advance(&pv, Controls { crouch_up: true, ..Controls::default() }, 1.0 / 60.0);
    assert_eq!(w.stance(), "ducked");

    // The window's time: a quarter of a frame at a time or a frame at once, the player's frames are the same.
    let mut a = at_view(&pv, &res, "colossal_door");
    let mut b = at_view(&pv, &res, "colossal_door");
    let ahead = Controls { forward: 1.0, ..Controls::default() };
    for _ in 0..60 {
        for _ in 0..4 {
            a.advance(&pv, ahead, 1.0 / 240.0);
        }
        b.advance(&pv, ahead, 1.0 / 60.0);
    }
    assert!((a.feet() - b.feet()).length() < 1.0 && (a.feet() - world((21.5, 15.5), 0.0)).length() > 200.0, "{} / {}", a.line(), b.line());
    // a stall is not caught up: a second at once is eight frames
    let before = b.feet();
    b.advance(&pv, ahead, 1.0);
    assert!((b.feet() - before).length() < 100.0);
    // the mouse turns it right for a move to the right (PD's θ rises: from +z towards -x, which
    // is on the right of a player facing +z; the fly camera's yaw falls), at the game's own rate;
    // the right arrow key turns it the same way, about 120° a second
    let theta = a.p.theta;
    a.advance(&pv, Controls { look: (200.0, 0.0), ..Controls::default() }, 1.0 / 60.0);
    let turned = (a.p.theta - theta).rem_euclid(360.0);
    assert!((turned - 11.0).abs() < 1.0, "θ {theta} -> {}", a.p.theta);
    assert!(a.camera().yaw < (-theta).to_radians());
    let theta = b.p.theta;
    for _ in 0..30 {
        b.advance(&pv, Controls { turn: 1.0, ..Controls::default() }, 1.0 / 60.0);
    }
    let turned = (b.p.theta - theta).rem_euclid(360.0);
    assert!((turned - 60.0).abs() < 6.0, "θ {theta} -> {}", b.p.theta);
    drop(pv);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_edit_gives_another_preview_and_a_walker_is_put_back_on_a_floor() {
    let root = root("edit");
    let (pv, res) = (seed40(&root), res());
    // medium1 moved ten cells east (the edit a drag of its box makes)
    let mut moved = gen::Recipe::python(40);
    moved.edits.push(Edit::Move { room: "medium1".into(), to: (60, 29) });
    let pv2 = Preview::make(&moved, &assets(), &root).unwrap();
    let was = pv.rooms.iter().find(|r| r.key == "medium1").unwrap();
    let now = pv2.rooms.iter().find(|r| r.key == "medium1").unwrap();
    assert_eq!((was.rect, now.rect), ((50, 29, 57, 34), (60, 29, 67, 34)));
    assert_ne!(pv.level.geom.polys.len(), pv2.level.geom.polys.len());
    assert!(pv.dir() != pv2.dir() && left(&root) == 2, "each preview in a folder of its own");
    // where the room was there is no floor now, and where it is there is
    let (old, new) = (world((53.5, 31.5), 60.0), world((63.5, 31.5), 60.0));
    assert_eq!((pv.floor_under(old), pv2.floor_under(old)), (Some(0.0), None));
    assert_eq!(pv2.floor_under(new), Some(0.0));
    assert_eq!((pv.room_at(old).map(|r| r.key.as_str()), pv2.room_at(new).map(|r| r.key.as_str())), (Some("medium1"), Some("medium1")));

    // A walker in the hall, which the edit left alone: kept as it is.
    let mut w = at_view(&pv, &res, "colossal_door");
    walk(&mut w, &pv, 60);
    let feet = w.feet();
    assert_eq!(w.reseat(&pv2), Ok(Reseat::Kept));
    assert_eq!(w.feet(), feet);
    walk(&mut w, &pv2, 60);
    assert!((w.feet() - feet).length() > 200.0 && w.feet().y.abs() < 1.0, "it walks on in the new level: {}", w.line());

    // A walker in the room that moved away: on the nearest floor, a few metres off, facing as it did.
    let mut w = Walker::at(res.clone(), world((53.5, 31.5), 0.0), 200.0).unwrap();
    let Ok(Reseat::Moved(far)) = w.reseat(&pv2) else { panic!("the floor is gone") };
    assert!((100.0..RESEAT_REACH).contains(&far), "{far}");
    let feet = w.feet();
    assert!((pv2.floor_under(feet + Vec3::Y * 60.0).unwrap() - feet.y).abs() < 1.0 && w.p.theta == 200.0, "{}", w.line());

    // A floor that moved a little under the walker: it stands on it. (A walker a metre in the
    // air over the hall's floor is what a floor sunk by a metre leaves.)
    let mut w = Walker::at(res.clone(), world((21.5, 15.5), 100.0), 0.0).unwrap();
    assert_eq!(w.reseat(&pv2), Ok(Reseat::Lifted));
    assert!(w.feet().y.abs() < 0.01);

    // Nothing within reach (far outside the level): the walker is no use, and it says so.
    let mut w = Walker::at(res.clone(), pv2.lo - Vec3::splat(RESEAT_REACH * 2.0), 0.0).unwrap();
    assert_eq!(w.reseat(&pv2).err().as_deref(), Some("the floor the walker stood on is gone, and there is none near"));

    // The nearest floor is the one underfoot where there is one.
    let near = pv2.nearest_floor(world((21.5, 15.5), 30.0), 500.0).unwrap();
    assert!(near.y.abs() < 0.01 && (near - world((21.5, 15.5), 0.0)).length() < 40.0, "{near}");
    assert_eq!(pv2.nearest_floor(pv2.hi + Vec3::splat(3000.0), 500.0), None);
    drop((pv, pv2));
    assert_eq!(left(&root), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plan_cells_and_the_world_and_where_a_camera_is_put() {
    // x east, y north on the plan; PD's (x, height, -y), a cell 118.11 units
    assert_eq!(world((21.5, 15.5), 159.0), Vec3::new(21.5 * CELL, 159.0, -15.5 * CELL));
    let c = cell(Vec3::new(2539.365, 0.0, -1830.705));
    assert!((c.0 - 21.5).abs() < 1e-4 && (c.1 - 15.5).abs() < 1e-4);
    // yaw 0 faces +z, which is south on the plan; a quarter turn faces +x, east
    let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
    assert!(near(heading(0.0), (0.0, -1.0)) && near(heading(std::f32::consts::FRAC_PI_2), (1.0, 0.0)));
    // the mark on a plan drawn north up points the same way: down the picture for south, right for east
    let south = marker((100.0, 100.0), 0.0, 16.0);
    assert!(near(south[0], (100.0, 116.0)) && south[1].1 < 100.0 && south[2].1 < 100.0);
    let east = marker((100.0, 100.0), std::f32::consts::FRAC_PI_2, 16.0);
    assert!(near(east[0], (116.0, 100.0)) && east[1].0 < 100.0 && east[2].0 < 100.0);

    let root = root("places");
    let pv = seed40(&root);
    // a camera sent to a plan place stands on the floor there nearest under where it was:
    // under the hall's catwalk from the floor, on it from above; turned as it was
    let deck = (22.5, 15.5);
    let low = pv.camera_at_cell(deck, EYE, 1.0).unwrap();
    let high = pv.camera_at_cell(deck, 551.0 + EYE, 1.0).unwrap();
    assert_eq!((low.pos, high.pos, low.yaw), (world(deck, EYE), world(deck, 551.0 + EYE), 1.0));
    // from below a loft's floor: the highest floor there (loft1 is at 1102)
    let loft = pv.camera_at_cell((35.5, 39.0), -300.0, 0.0).unwrap();
    assert_eq!(loft.pos, world((35.5, 39.0), 1102.0 + EYE));
    // off the plan there is no floor
    assert!(pv.camera_at_cell((1.0, 1.0), EYE, 0.0).is_none());
    // a room's camera is in the room, over its floor, looking down at its middle
    for key in ["colossal", "loft1", "small2", "medium2"] {
        let cam = pv.room_camera(key).unwrap();
        let room = pv.rooms.iter().find(|r| r.key == key).unwrap();
        assert_eq!(pv.room_at(cam.pos).map(|r| r.key.as_str()), Some(key), "{cam:?}");
        assert!(cam.pos.y > room.floor as f32 + 90.0 && cam.pos.y < room.top as f32 && cam.pitch < 0.0, "{key}: {cam:?}");
    }
    assert!(pv.room_camera("nowhere").is_none() && pv.view_camera("nowhere").is_none());
    // the generator's views are cameras standing at them
    let v = pv.view_camera("colossal_deck551").unwrap();
    assert_eq!(v.pos, world((22.5, 15.5), 551.0 + EYE));
    drop(pv);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_keys_and_the_mouse_gathered_are_the_players_input() {
    let held = Controls { forward: 1.0, right: -1.0, turn: 0.0, look: (3.0, -2.0), crouch_down: true, crouch_up: false };
    // the next tick's: the keys as they are now, the mouse's travel and the presses added up
    let next = Controls { forward: 0.0, right: 0.0, turn: 1.0, look: (1.0, 1.0), crouch_down: false, crouch_up: true };
    let both = held.and(next);
    assert_eq!(both, Controls { forward: 0.0, right: 0.0, turn: 1.0, look: (4.0, -1.0), crouch_down: true, crouch_up: true });
    let input = held.input();
    assert_eq!((input.walk_y, input.walk_x, input.mouse_dx, input.mouse_dy, input.crouch_down, input.crouch_up), (127, -127, 3.0, -2.0, true, false));
    assert_eq!(both.input().mouse_dx, 4.0 + TURN_KEY);
}

#[test]
fn a_scratch_folder_goes_when_it_is_dropped_and_a_fresh_one_is_not_swept() {
    let root = root("sweep");
    let fresh = Scratch::new(&root).unwrap();
    assert!(fresh.path().is_dir());
    Scratch::sweep(&root);
    assert!(fresh.path().is_dir(), "a folder in use is not a day old");
    let path = fresh.path().to_path_buf();
    drop(fresh);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(&root);
}
