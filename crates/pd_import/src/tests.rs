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
    ColPoly { verts: verts.to_vec(), flags, floortype: FLOORTYPE_DIRT, grab: true }
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
    assert_eq!(a.custom_levels(), vec![pd_core::assets::CustomLevel { code: "test_yard".into(), stagenum: 0x7f, name: "Test Yard".into() }]);
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
