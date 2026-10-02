//! The importer on a level made here: a walled yard (30 × 20 m) with a 50 cm
//! plinth (a step), a 120 cm terrace reached only by climbing its ledge, and
//! fog. No other game's data is needed.

use glam::{Vec2, Vec3};
use pd_core::ids::*;

use crate::recipe::{OotSource, Recipe, Source};
use crate::rooms::Partition;
use crate::source::*;

const FLOOR: u32 = GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2 | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT;
const WALL: u32 = GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT;

fn col(verts: [Vec3; 4], flags: u32) -> ColPoly {
    ColPoly { verts: verts.to_vec(), flags, floortype: FLOORTYPE_DIRT, grab: true, floorcol: 0 }
}

/// A horizontal quad at `y` over `lo..hi` (x, z).
fn flat(lo: Vec2, hi: Vec2, y: f32) -> [Vec3; 4] {
    [Vec3::new(lo.x, y, lo.y), Vec3::new(hi.x, y, lo.y), Vec3::new(hi.x, y, hi.y), Vec3::new(lo.x, y, hi.y)]
}

/// A vertical quad from `a` to `b` (x, z) between `y0` and `y1`.
fn upright(a: Vec2, b: Vec2, y0: f32, y1: f32) -> [Vec3; 4] {
    [Vec3::new(a.x, y0, a.y), Vec3::new(b.x, y0, b.y), Vec3::new(b.x, y1, b.y), Vec3::new(a.x, y1, a.y)]
}

/// The four sides of the box `lo..hi` from `y0` to `y1`.
fn sides(lo: Vec2, hi: Vec2, y0: f32, y1: f32) -> Vec<[Vec3; 4]> {
    let (a, b, c, d) = (lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y));
    vec![upright(a, b, y0, y1), upright(b, c, y0, y1), upright(c, d, y0, y1), upright(d, a, y0, y1)]
}

const TERRACE: (Vec2, Vec2) = (Vec2::new(1800.0, 400.0), Vec2::new(2800.0, 1400.0));
const PLINTH: (Vec2, Vec2) = (Vec2::new(400.0, 400.0), Vec2::new(800.0, 800.0));

fn yard() -> LevelSource {
    let mut collision = Vec::new();
    // The ground, in 5 m tiles.
    for i in 0..6 {
        for k in 0..4 {
            let lo = Vec2::new(i as f32 * 500.0, k as f32 * 500.0);
            collision.push(col(flat(lo, lo + Vec2::splat(500.0), 0.0), FLOOR));
        }
    }
    for w in sides(Vec2::ZERO, Vec2::new(3000.0, 2000.0), 0.0, 300.0) {
        collision.push(col(w, WALL));
    }
    collision.push(col(flat(TERRACE.0, TERRACE.1, 120.0), FLOOR));
    for w in sides(TERRACE.0, TERRACE.1, 0.0, 120.0) {
        collision.push(col(w, WALL));
    }
    collision.push(col(flat(PLINTH.0, PLINTH.1, 50.0), FLOOR));
    for w in sides(PLINTH.0, PLINTH.1, 0.0, 50.0) {
        collision.push(col(w, WALL));
    }
    // Every collision polygon drawn, one textured material.
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([120, 160, 90, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let tris = collision
        .iter()
        .flat_map(|p| {
            let v: Vec<Vert> = p.verts.iter().map(|&pos| Vert { pos, uv: Vec2::new(pos.x, pos.z) / 500.0, col: [200, 200, 200, 255] }).collect();
            [[v[0], v[1], v[2]], [v[0], v[2], v[3]]]
        })
        .map(|v| Tri { v, material: 0, xlu: false })
        .collect();
    LevelSource {
        textures: vec![Texture { png: png.into_inner(), w: 2, h: 2 }],
        materials: vec![Material { texture: Some(0), wrap: [Wrap::Repeat; 2], blend: Blend::Opaque, cull_back: false, zwrite: true, decal: false, surface: 7 }],
        tris,
        collision,
        markers: vec![
            Marker { kind: MarkerKind::Spawn, pos: Vec3::new(300.0, 0.0, 1700.0), facing: 0.0 },
            Marker { kind: MarkerKind::Prize, pos: Vec3::new(2300.0, 120.0, 900.0), facing: 0.0 },
        ],
        env: Env { sky: [200, 200, 150], near: 15.0, far: 10000.0, fog: Some((990, 1000)) },
    }
}

fn recipe() -> Recipe {
    Recipe {
        format: "pd-import-recipe/1".into(),
        code: "test_yard".into(),
        name: "Test Yard".into(),
        stagenum: 0x7f,
        scale: 1.0,
        source: Source::Oot(OotSource { scene_dir: String::new(), scene: String::new(), layer: 0, light_setting: 0, sun_dir: None, draw_rooms: None, remove: Vec::new(), walls: Vec::new() }),
        rooms: 6,
        spawns: 6,
        weapons: 4,
        hills: 2,
        marker_share: 1.0,
        play_area: crate::recipe::PlayArea::Largest,
        group: None,
    }
}

/// The plinth's 50 cm sides are steps (dropped); the terrace's 120 cm sides
/// are ledges: a ladder strip in the middle of each, climbable by players the
/// rest of the way; the yard's 3 m walls stay walls; a wall the source says
/// can't be grabbed stays one too.
#[test]
fn steps_are_dropped_and_ledges_become_ladders() {
    let mut src = yard();
    let n = src.collision.len();
    assert_eq!(src.drop_step_risers(), 4);
    assert_eq!(src.collision.len(), n - 4);
    assert_eq!(src.mark_ledges(), 4);
    let ladders: Vec<&ColPoly> = src.collision.iter().filter(|p| p.flags & GEOFLAG_LADDER != 0).collect();
    let player_only = src.collision.iter().filter(|p| p.flags & GEOFLAG_LADDER_PLAYERONLY != 0).count();
    assert_eq!((ladders.len(), player_only), (4, 8), "a strip and two player-only sides per ledge");
    for l in &ladders {
        let (lo, hi) = l.verts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(((hi - lo).with_y(0.0).length() - LEDGE_STRIP).abs() < 0.5, "a {LEDGE_STRIP} cm strip: {lo} {hi}");
        assert!(l.flags & GEOFLAG_WALL != 0);
    }
    assert_eq!(src.collision.iter().filter(|p| p.flags == WALL).count(), 4, "the yard's walls");

    let mut nograb = yard();
    for p in &mut nograb.collision {
        p.grab = false;
    }
    assert_eq!(nograb.mark_ledges(), 0);
}

/// The rooms tile the footprint, every cut piece lies in its room, and every
/// pair of rooms sharing a side has a portal on it.
#[test]
fn rooms_tile_the_level_and_portals_join_them() {
    let src = yard();
    let part = Partition::build(&src, 6);
    assert!((4..=10).contains(&part.rooms()), "{} rooms", part.rooms());
    let area: f32 = part.rects.iter().map(|r| (r.hi - r.lo).x * (r.hi - r.lo).y).sum();
    assert!((area - 3000.0 * 2000.0).abs() < 1.0, "the rooms cover the yard once: {area}");
    let mut pieces_area = 0.0;
    for p in src.collision.iter().filter(|p| p.flags & GEOFLAG_FLOOR1 != 0 && p.verts[0].y == 0.0) {
        for (room, piece) in part.cut(p.verts.clone()) {
            let r = part.rects[room as usize - 1];
            for v in &piece {
                assert!(v.x >= r.lo.x - 0.01 && v.x <= r.hi.x + 0.01 && v.z >= r.lo.y - 0.01 && v.z <= r.hi.y + 0.01, "{v} outside room {room}");
            }
            pieces_area += (1..piece.len() - 1).map(|k| (piece[k] - piece[0]).cross(piece[k + 1] - piece[0]).length() * 0.5).sum::<f32>();
        }
    }
    assert!((pieces_area - 3000.0 * 2000.0).abs() < 10.0, "cutting keeps the ground's area: {pieces_area}");
    let portals = part.portals();
    for (i, a) in part.rects.iter().enumerate() {
        for (j, b) in part.rects.iter().enumerate().skip(i + 1) {
            let touch_x = (a.hi.x == b.lo.x || b.hi.x == a.lo.x) && a.lo.y.max(b.lo.y) < a.hi.y.min(b.hi.y);
            let touch_z = (a.hi.y == b.lo.y || b.hi.y == a.lo.y) && a.lo.x.max(b.lo.x) < a.hi.x.min(b.hi.x);
            let joined = portals.iter().any(|p| p.rooms == [i as u16 + 1, j as u16 + 1]);
            assert_eq!(touch_x || touch_z, joined, "rooms {} and {}", i + 1, j + 1);
        }
    }
}

/// The whole pipeline: the yard becomes a stage the game loads (the four
/// files, fog, rooms, portals), every spawn stands on a floor, the terrace is
/// reachable by climbing and a weapon sits on it (the prize), each weapon has
/// its two crates, the scenarios have their pads, and a minute of simulants
/// plays on it.
#[test]
fn a_level_becomes_a_stage_the_game_plays() {
    let dir = std::env::temp_dir().join(format!("pd_import_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets");
    let paths = crate::Paths { assets: assets.clone(), custom: dir.clone(), src: None };
    let r = recipe();
    let report = crate::build(&r, yard(), &paths).unwrap();
    let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&dir);
    assert_eq!(a.custom_levels(), vec![pd_core::assets::CustomLevel { code: "test_yard".into(), stagenum: 0x7f, name: "Test Yard".into(), group: String::new() }]);
    assert_eq!(a.stage_code(0x7f).as_deref(), Some("test_yard"));

    let stage = pd_sim::stage::Stage::load(&a, "test_yard").unwrap();
    assert_eq!(stage.stagenum, 0x7f);
    assert_eq!(stage.spawn_pads.len(), 6);
    assert!(stage.rooms.roomcount() > 4 && !stage.rooms.portals.is_empty());
    let count = |t: &str| stage.props.iter().filter(|p| p["type"] == t).count();
    assert_eq!((count("weapon"), count("ammocratemulti")), (4, 8));
    let intro = |t: &str| stage.intro.iter().filter(|p| p["type"] == t).count();
    assert_eq!((intro("case"), intro("case_respawn"), intro("hill")), (4, 24, 2));
    // The prize (on the terrace) takes location 5 when there are six; with
    // four, the last.
    let prize = stage.props.iter().filter(|p| p["type"] == "weapon").find(|p| stage.pads[p["chr"].as_u64().unwrap() as usize].pos.y > 150.0);
    assert!(prize.is_some(), "a weapon on the terrace");
    // The terrace's waypoints are reached: climbed to, dropped from.
    let links = stage.waypoint_links();
    let up = |w: usize| stage.waypoint_pos(w).y > 150.0;
    assert!(links.iter().any(|&(a, b, _)| !up(a) && up(b)), "a way up");
    assert!(links.iter().any(|&(a, b, _)| up(a) && !up(b)), "a way down");
    let bg = std::fs::read_to_string(dir.join("stages/test_yard/bg.json")).unwrap();
    assert!(bg.contains("\"fogenvironment\"") && bg.contains("\"fog_shade\":true"));
    assert!(report.iter().any(|l| l.starts_with("check: a minute")), "{report:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// GoldenEye's Facility, the whole solo level, from the ROM through
/// `tools/ge-extract` (`#[ignore]`: it needs the ROM the recipe names and
/// Python, and the waypoint generator takes about a minute; without the ROM it
/// says so and passes). The stage loads with GE's own 77 rooms and portals;
/// its 46 doors are made shut, each on GE's own model; the level is one piece
/// to the simulants; and a player's use opens a swinging door (the toilets')
/// and a sliding one, PD's door code driving GE's doors.
#[test]
#[ignore]
fn facility_comes_from_the_goldeneye_rom() {
    use pd_sim::props::door::{DOORTYPE_SLIDING, DOORTYPE_SWINGING};
    use pd_sim::player::PlayerInput;
    use std::sync::Arc;
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let r = Recipe::load(&manifest.join("levels").join("facility.json")).unwrap();
    let Source::Ge(g) = &r.source else { panic!("facility.json is not a GoldenEye recipe") };
    if !std::path::Path::new(&g.rom).exists() {
        eprintln!("skipped: no GoldenEye ROM at {}", g.rom);
        return;
    }
    let dir = std::env::temp_dir().join(format!("pd_import_ge_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = manifest.join("..").join("..").join("assets");
    let paths = crate::Paths { assets: assets.clone(), custom: dir.clone(), src: None };
    let report = crate::import(&r, &paths, &crate::layout::Layout::new()).unwrap();
    let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&dir);
    let stage = Arc::new(pd_sim::stage::Stage::load(&a, "facility").unwrap());
    assert_eq!(stage.rooms.roomcount(), 78, "GE's rooms 1..77 and PD's room 0");
    assert_eq!(stage.rooms.portals.len(), 109);
    let reach = report.iter().find(|l| l.starts_with("reachable:")).unwrap();
    let (main, all): (usize, usize) = {
        let w: Vec<&str> = reach.split_whitespace().collect();
        (w[1].parse().unwrap(), w[3].parse().unwrap())
    };
    assert!(main * 100 >= all * 95, "the level is in pieces: {reach}");

    let level = Arc::new(pd_sim::stage::TileLevel::for_stage(&stage));
    let res = Arc::new(pd_sim::world::WorldRes::load(&a).unwrap());
    let setup = pd_core::mp::MatchSetup { stagenum: stage.stagenum, players: vec![pd_core::mp::MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
    let mut w = pd_sim::world::World::new(setup, stage.clone(), level.clone(), res, 1).unwrap();
    let doors: Vec<usize> = (0..w.props.objs.len()).filter(|&i| w.props.objs[i].door.is_some()).collect();
    assert_eq!(doors.len(), 46);
    for &i in &doors {
        let o = &w.props.objs[i];
        assert!(pd_core::assets::CUSTOM_MODELNUMS.contains(&o.modelnum), "door {i} is on model {}", o.modelnum);
        assert!(o.door.as_ref().unwrap().is_closed(), "door {i} starts open");
    }
    for ty in [DOORTYPE_SWINGING, DOORTYPE_SLIDING] {
        let i = doors
            .iter()
            .copied()
            .find(|&i| {
                let d = w.props.objs[i].door.as_ref().unwrap();
                d.doortype == ty && d.portalnum.is_some() && d.sibling.is_none()
            })
            .unwrap_or_else(|| panic!("no lone door of type {ty} with a portal"));
        let pad = w.props.objs[i].door.as_ref().unwrap().pad.clone();
        let (n, centre) = (pad.normal(), pad.centre());
        let side = [1.0f32, -1.0].into_iter().find(|s| level.cd_find_room_at_pos_ycnp(centre + n * 120.0 * *s).is_some()).unwrap();
        let face = -n * side;
        w.players[0].start_new_life(&level, &[], centre + n * 120.0 * side, pd_core::math::atan2f(face.x, face.z));
        for f in 0..34 {
            w.step(4, &[PlayerInput { use_held: (30..33).contains(&f), ..Default::default() }]);
        }
        let opened = (0..600).any(|_| {
            w.step(4, &[PlayerInput::default()]);
            let d = w.props.objs[i].door.as_ref().unwrap();

            d.is_open()
        });
        assert!(opened, "door {i} (type {ty}) never opened for a use");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A rebuilt arena's portals are found from its open edges alone: on
/// Complex's own rooms, that finds every room pair Complex joins and no other,
/// each of its portals' corners on the opening found (two portals between one
/// pair, in one wall, come out as one opening over both).
#[test]
fn portals_are_found_where_complex_has_them() {
    use std::collections::HashSet;
    let (rooms, portals) = crate::pd::test_access::arena_rooms("ref");
    let found = crate::pd::test_access::portals_of(&rooms);
    let want: HashSet<[u16; 2]> = portals.iter().map(|p| p.0).collect();
    let got: HashSet<[u16; 2]> = found.iter().map(|p| p.0).collect();
    assert_eq!(got, want, "the room pairs joined");
    for (rs, verts) in &portals {
        for v in verts {
            assert!(found.iter().any(|(fr, hull)| fr == rs && crate::pd::test_access::on(*v, hull)), "portal {rs:x?}: corner {v} is on no opening found");
        }
    }
}

/// Very Complex (the Blender MCP repo's Complex extended underground), rebuilt
/// over Complex: Complex's 44 rooms and 60 portals kept, its 4 changed and 8
/// added rooms joined by the portals found, its whole setup where Complex has
/// it, and the new rooms routed. Needs the repo's glTF (the recipe's path);
/// without it, says so and passes.
#[test]
fn very_complex_is_complex_rebuilt() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let r = Recipe::load(&manifest.join("levels").join("verycomplex.json")).unwrap();
    let Source::Pd(p) = &r.source else { panic!("verycomplex.json is not a rebuilt PD arena") };
    if !std::path::Path::new(&p.glb).exists() {
        eprintln!("skipped: no glTF at {}", p.glb);
        return;
    }
    let dir = std::env::temp_dir().join(format!("pd_import_vc_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = manifest.join("..").join("..").join("assets");
    let paths = crate::Paths { assets: assets.clone(), custom: dir.clone(), src: None };
    let report = crate::import(&r, &paths, &crate::layout::Layout::new()).unwrap();
    let line = |start: &str| report.iter().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no {start:?} in {report:#?}")).clone();
    assert!(line("base:").ends_with("40 the arena's, changed [14, 22, 25, 2a], added [2d, 2e, 2f, 30, 31, 32, 33, 34]"), "{}", line("base:"));
    assert!(line("tiles:").contains("14 dropped"), "the pool's 8 walls and the 3 walls opened: {}", line("tiles:"));

    let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&dir);
    let vc = pd_sim::stage::Stage::load(&a, "verycomplex").unwrap();
    let complex = pd_sim::stage::Stage::load(&a, "ref").unwrap();
    assert_eq!(vc.rooms.roomcount(), 53, "rooms 1..0x34 and PD's room 0");
    assert_eq!(vc.rooms.portals.len(), 60 + 12);
    // Complex's setup, on Complex's pads where Complex has them.
    assert_eq!(vc.intro, complex.intro);
    assert_eq!(vc.props, complex.props);
    assert_eq!(vc.spawn_pads, complex.spawn_pads);
    for prop in vc.intro.iter().chain(&vc.props) {
        for k in ["pad", "chr"] {
            if let Some(pad) = prop[k].as_u64() {
                assert_eq!(vc.pads[pad as usize].pos, complex.pads[pad as usize].pos, "{prop}");
            }
        }
    }
    // The portals found stay as written through PD's `bg_init_portal` (its
    // room swaps go by the rooms' centres, which a rebuilt room needn't keep
    // on its side), and the portal pass sees through the two that came out
    // flipped or short at first: the undercroft's exit to the red corridor,
    // the hall's windows into the base, grilles and all.
    let bg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("stages/verycomplex/bg.json")).unwrap()).unwrap();
    for (i, p) in vc.rooms.portals.iter().enumerate().skip(60) {
        let rs = &bg["portals"][i]["rooms"];
        assert_eq!([p.room1, p.room2], [rs[0].as_u64().unwrap() as u16, rs[1].as_u64().unwrap() as u16], "portal {i}: PD swapped its rooms");
        let mut n = Vec3::ZERO;
        for k in 0..p.verts.len() {
            let (a, b) = (p.verts[k], p.verts[(k + 1) % p.verts.len()]);
            n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        assert!(p.metric.normal.dot(-n) > 0.0, "portal {i}: PD turned its normal");
    }
    let open = vc.rooms.initial_portal_flags();
    // The F1 panel's feet and θ (the look is (-sin θ, cos θ)).
    for (feet, theta, room, behind) in [(Vec3::new(-4269.0, -552.0, -1351.0), 166.1f32, 0x2d, 0x2f), (Vec3::new(-969.0, -552.0, -1395.0), 50.6, 0x31, 0x33)] {
        let t = theta.to_radians();
        let mut c = pd_sim::player::camera::Camera::default();
        let eye = feet + Vec3::Y * 159.0;
        c.player_allocate_matrices(eye, Vec3::new(-t.sin(), 0.0, t.cos()), Vec3::Y);
        let cam = pd_sim::stage::portals::PortalCam {
            world_to_screen: c.world_to_screen,
            cam_pos: eye,
            c_screenleft: c.c_screenleft,
            c_screentop: c.c_screentop,
            c_halfwidth: c.c_halfwidth,
            c_halfheight: c.c_halfheight,
            c_recipscalex: 1.0 / c.c_scalex,
            c_recipscaley: 1.0 / c.c_scaley,
            view: pd_sim::stage::portals::PortalCam::screen_properties(0.0, 0.0, 320.0, 220.0, 320.0, 220.0),
            zfar: 10000.0,
        };
        let v = pd_sim::stage::portals::bg_tick_portals(&vc.rooms, &open, &cam, room);
        assert!(v.is_onscreen(behind), "from room {room:#x} at {feet}, room {behind:#x} isn't drawn: {:?}", v.drawslots.iter().map(|s| s.roomnum).collect::<Vec<_>>());
        if room == 0x31 {
            // The windows' portal reaches down past the grilles to the base's floor.
            let w = vc.rooms.portals.iter().find(|p| [p.room1, p.room2].contains(&0x31) && [p.room1, p.room2].contains(&0x33)).unwrap();
            assert_eq!(w.verts.iter().map(|v| v.y).fold(f32::MAX, f32::min), -552.0);
        }
    }
    // Every new room is on the simulants' graph.
    let routed = line("reachable waypoints in the new and changed rooms:");
    for part in routed.split_once(": ").unwrap().1.split(", ") {
        let n: usize = part.split_once(": ").unwrap().1.parse().unwrap();
        assert!(n > 0, "room {part}: no waypoint reaches it ({routed})");
    }
    assert!(line("the setup:").contains(" 0 of them over 3 m"), "{}", line("the setup:"));
    assert!(report.iter().any(|l| l.starts_with("check: a minute")), "{report:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// CI Felicity (the Blender MCP repo's `felicity_ci.py`): CI Training, moved
/// 60 m west, and Felicity, turned 120° and moved, joined by a hall. Laid over
/// both stages: CI's from the decomp (it is no arena), Felicity's from
/// `assets/`, each room recognised where the recipe's `bases` put it. The
/// geometry step only (the waypoints take minutes). Needs the repo's glTF and
/// the decomp; without them, says so and passes.
#[test]
fn ci_felicity_is_two_stages_fused() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let r = Recipe::load(&manifest.join("levels").join("cifelicity.json")).unwrap();
    let Source::Pd(p) = &r.source else { panic!("cifelicity.json is not a rebuilt PD level") };
    let decomp = manifest.join("..").join("..").join("reference").join("pd-decomp").join("src");
    if !std::path::Path::new(&p.glb).exists() || !decomp.is_dir() {
        eprintln!("skipped: no glTF at {} or no decomp at {}", p.glb, decomp.display());
        return;
    }
    let dir = std::env::temp_dir().join(format!("pd_import_cf_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = manifest.join("..").join("..").join("assets");
    let paths = crate::Paths { assets: assets.clone(), custom: dir.clone(), src: None };
    let (stage_dir, data) = crate::geometry(&r, &paths).unwrap();
    let line = |start: &str| data.report.iter().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no {start:?} in {:#?}", data.report)).clone();
    // CI's rooms 1..=0x8c, Felicity's after them; the rooms the hall cut into
    // changed (CI's 0x44 and Felicity's 0x10, now 0x9c, each a doorway; CI's
    // 0x2c and 0x3d lost a face each), the hall added.
    assert!(line("base:").ends_with("175 the arena's, changed [2c, 3d, 44, 9c], added [b4]"), "{}", line("base:"));
    // Felicity's rooms land on its own triangles turned 120°, as CI's moved.
    assert!(line("conversion:").contains("within 0.03 texels and 0 colour steps"), "{}", line("conversion:"));
    assert!(line("portals:").ends_with("2 found to the rebuilt rooms: 44->b4 (6 verts), 9c->b4 (7 verts)"), "{}", line("portals:"));
    // 56 (58 before M18: the door catalogue's models, CI's doors among
    // them, brought two of CI's textures into the pool).
    assert!(line("textures:").contains("56 of them not in assets/"), "{}", line("textures:"));
    let crate::SourceHow::Generate(markers) = &data.how else { panic!("a fused level's setup is generated") };
    assert_eq!(markers.len(), 12 + 10, "Felicity's MP spawns and weapons");

    // As the game loads it (no gameplay data yet).
    crate::write::write_pads(&stage_dir, &r, &crate::write::Gameplay::default()).unwrap();
    crate::write::write_setup(&stage_dir, &r, &crate::write::Gameplay::default()).unwrap();
    let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&dir);
    let cf = pd_sim::stage::Stage::load(&a, "cifelicity").unwrap();
    let fel = pd_sim::stage::Stage::load(&a, "mp11").unwrap();
    assert_eq!(cf.rooms.roomcount(), 0xb5, "rooms 1..0xb4 and PD's room 0");
    assert_eq!(cf.rooms.portals.len(), 178 + fel.rooms.portals.len() + 2);
    // A portal of Felicity's, moved, joins the same rooms, and PD's
    // `bg_init_portal` keeps its sides.
    let (i, fp) = fel.rooms.portals.iter().enumerate().next().unwrap();
    let moved = &cf.rooms.portals[178 + i];
    assert_eq!([moved.room1, moved.room2], [fp.room1 + 0x8c, fp.room2 + 0x8c]);
    let t = 120f32.to_radians();
    let turned = Vec3::new(fp.metric.normal.x * t.cos() + fp.metric.normal.z * t.sin(), fp.metric.normal.y, fp.metric.normal.z * t.cos() - fp.metric.normal.x * t.sin());
    assert!(moved.metric.normal.normalize().dot(turned.normalize()) > 0.999, "{} vs {turned}", moved.metric.normal);
    // The hall's ceiling is CI's 0x249, whose surface types (footsteps, shot
    // effects) come from the stage's own texture entry: no arena draws it.
    let hit = pd_sim::stage::bghit::BgHitMesh::load(&a, "cifelicity").unwrap().bg_test_hit(Vec3::new(-9700.0, 400.0, -1670.0), Vec3::new(-9700.0, 700.0, -1670.0)).unwrap();
    assert_eq!(hit.room, 0xb4);
    assert_eq!(hit.surface, Some(pd_sim::stage::bghit::TexSurface { soundsurfacetype: 1, surfacetype: 8 }));
    assert!(dir.join("textures").join("0249.png").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Catacombs, a Jedi Academy map, with the game's `base` directory: Korriban's
/// textures from the packages, its lightmaps baked into split triangles, the
/// whole pipeline through the check match. Needs the user's copy of the game
/// (the recipe's `base` and `bsp`); without it, says so and passes.
#[test]
fn catacombs_is_a_jedi_academy_map() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let r = Recipe::load(&manifest.join("levels").join("catacombs.json")).unwrap();
    let Source::Jka(j) = &r.source else { panic!("catacombs.json is not a Jedi Academy map") };
    if !std::path::Path::new(&j.bsp).is_file() || !std::path::Path::new(&j.base).join("assets1.pk3").is_file() {
        eprintln!("skipped: no map at {} or no game at {}", j.bsp, j.base);
        return;
    }
    let dir = std::env::temp_dir().join(format!("pd_import_jka_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = manifest.join("..").join("..").join("assets");
    let paths = crate::Paths { assets: assets.clone(), custom: dir.clone(), src: None };
    let report = crate::import(&r, &paths, &crate::layout::Layout::new()).unwrap();
    let line = |start: &str| report.iter().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no {start:?} in {report:#?}")).clone();
    let jka = line("jka: 178 surfaces");
    assert!(jka.contains("(20 patches, 0 sky)") && jka.contains("19 materials, 19 textures") && jka.contains("1810 Quake triangles") && jka.contains("11 markers"), "{jka}");
    // Every image found: the one the map shipped with (not in the game) drawn
    // as its sibling, by the recipe (the user's first playtest: six vine
    // walls were grey checkers). `noshader` is only on brush sides.
    assert_eq!(line("jka: the recipe's substitutes"), "jka: the recipe's substitutes: textures/yavin/temple_vines as textures/yavin/temple_vines2");
    assert!(!report.iter().any(|l| l.starts_with("jka: images not found")), "{report:#?}");
    // Lit by its lightmaps: the triangles split where the light varies (an
    // empty lightmap rectangle once held every sample to one texel: none).
    let added: usize = jka.split(" added").next().unwrap().rsplit(' ').next().unwrap().parse().unwrap();
    assert!((5000..40000).contains(&added), "{jka}");
    assert!(line("source:").contains("61 x 54 x 8 m"), "{}", line("source:"));
    assert!(line("spawns:").contains("(3 at the source's starts"), "{}", line("spawns:"));
    let reach = line("reachable:");
    let n: Vec<usize> = reach.split(|c: char| !c.is_ascii_digit()).filter_map(|s| s.parse().ok()).take(2).collect();
    assert!(n[0] * 10 >= n[1] * 8, "most of the level in one part: {reach}");
    assert!(line("check:").contains("0 stalls"), "{}", line("check:"));
    // A floor's colour is its lightmap's (the corridors are lit, not black).
    let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&dir);
    let stage = pd_sim::stage::Stage::load(&a, "catacombs").unwrap();
    let level = pd_sim::stage::TileLevel::for_stage(&stage);
    let pad = &stage.pads[stage.spawn_pads[0]];
    let (_, floor) = level.cd_find_ground_at_cyl(pad.pos, 30.0);
    assert!(floor.is_some(), "a floor under spawn 0");

    // As the GoldenEye Setup Editor's level files: its groups, its tags,
    // every face's corners present, every material defined.
    let out = dir.join("ge64");
    let opts = crate::ge64::Options { rooms: 32, max_texels: 2048, max_side: 64 };
    let report = crate::export_ge64(&r, &paths, &out, &opts, Some((64.0, 192.0))).unwrap();
    assert!(report.iter().any(|l| l.starts_with("textures: 19 BMPs (18 at 32x32, 1 at 32x64)")), "{report:#?}");
    let obj = std::fs::read_to_string(out.join("level/LevelIndices.obj")).unwrap();
    let nv = obj.lines().filter(|l| l.starts_with("v ")).count();
    assert_eq!(nv, obj.lines().filter(|l| l.starts_with("#vcolor")).count());
    assert!(obj.lines().filter(|l| l.starts_with("f ")).flat_map(|l| l.split_whitespace().skip(1)).all(|c| c.split('/').next().unwrap().parse::<usize>().is_ok_and(|i| (1..=nv).contains(&i))));
    assert!(obj.contains("
g primary_Room01
"));
    let mtl = std::fs::read_to_string(out.join("level/LevelIndices.mtl")).unwrap();
    assert!(obj.lines().filter_map(|l| l.strip_prefix("usemtl ")).all(|m| mtl.contains(&format!("newmtl {m}
"))));
    let clip = std::fs::read_to_string(out.join("clipping/clippingObjcatacombs.obj")).unwrap();
    assert!(clip.contains("
g Clip000
") && clip.contains("_ForceFloor_SFX2
") && clip.contains("_SolidLadder_SFX0
"));
    let portals = std::fs::read_to_string(out.join("portal/portals.txt")).unwrap();
    assert!(portals.lines().next().is_some_and(|l| l.len() == 15 && l.ends_with(" 0000 04")), "{portals:.40}");
    std::fs::remove_dir_all(&dir).unwrap();
}
