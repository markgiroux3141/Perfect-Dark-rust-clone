//! The generated-level source on a level made here (seed 40, as the answer
//! keys hold it): its rooms and portals are the generator's, its materials
//! Complex's on the pool's textures, its tiles PD's rules and these levels'
//! three, and whatever a player on any of its routes can see is drawn.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use autolevel::recipe::Recipe as Seed;
use autolevel_build::built::{self, Built};
use autolevel_build::walk;
use glam::Vec3;
use pd_core::ids::*;
use pd_sim::stage::portals::{bg_tick_portals, PortalCam};
use serde_json::Value;

use super::*;
use crate::autowalk::{self, SimWalk, Via};
use crate::recipe::Source;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets")
}

/// Seed 40, made once for every test here.
fn seed40() -> &'static Built {
    static BUILT: OnceLock<Built> = OnceLock::new();
    BUILT.get_or_init(|| built::make(&Seed::new(40), true).unwrap())
}

const RECIPE_40: &str = "{\n  \"format\": 1,\n  \"seed\": 40\n}";

/// The level's recipe, as `autolevel import` writes it.
fn recipe40() -> Recipe {
    autowalk::recipe("autolevel40", "Autolevel 40", autowalk::STAGENUM, Via::Recipe(RECIPE_40)).unwrap()
}

fn source40() -> (LevelSource, Vec<String>) {
    let r = recipe40();
    let Source::Autolevel(s) = &r.source else { panic!("an autolevel source") };
    let mut report = Vec::new();
    let src = level_source(&r, s, seed40(), &assets(), &mut report).unwrap();
    (src, report)
}

fn res() -> Arc<pd_sim::world::WorldRes> {
    static RES: OnceLock<Arc<pd_sim::world::WorldRes>> = OnceLock::new();
    RES.get_or_init(|| SimWalk::res(&assets()).unwrap()).clone()
}

/// A build folder holding nothing but the generator's recipe, and the level
/// as the game loads it from there.
fn loaded(tag: &str) -> (PathBuf, SimWalk) {
    let dir = std::env::temp_dir().join(format!("pd_import_autolevel_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("recipe.json"), RECIPE_40).unwrap();
    let w = SimWalk::open(&dir, &assets(), res()).unwrap();
    (dir, w)
}

/// A plan axis as PD's direction: x east is +x, y north is -z, z up is +y.
fn pd_axis(axis: usize) -> Vec3 {
    [Vec3::X, -Vec3::Z, Vec3::Y][axis]
}

#[test]
fn the_generators_rooms_and_doorways_are_pds_rooms_and_portals() {
    let b = seed40();
    let (src, report) = source40();
    let own = src.rooms.as_ref().expect("the level's own rooms");
    let openings = b.openings();
    // One room for each of the plan's, one portal for each opening.
    assert_eq!((own.boxes.len(), own.portals.len(), openings.len()), (61, 91, 91));
    assert!(report[0].starts_with("autolevel: seed 40, 61 rooms (0 with nothing drawn), 91 openings between them, 17 materials of Complex's on 16 pool textures, baked"), "{report:?}");
    // Every triangle and tile names its room, and lies in its box.
    assert_eq!(src.tris.len(), 15303);
    for (room, pts) in src.tris.iter().map(|t| (t.room, t.v.map(|v| v.pos).to_vec())).chain(src.collision.iter().map(|p| (p.room, p.verts.clone()))) {
        assert!((1..=61).contains(&room), "room {room}");
        let (lo, hi) = own.boxes[room as usize - 1];
        assert!(pts.iter().all(|p| p.cmpge(lo).all() && p.cmple(hi).all()), "room {room}: {pts:?} outside {lo} .. {hi}");
    }
    // A room's triangles are the generator's mesh of it, and its box holds its air.
    for room in &b.level.plan.rooms {
        let mesh = b.rooms.iter().find(|m| m.key == room.key).unwrap();
        assert_eq!(src.tris.iter().filter(|t| t.room as usize == room.room_id).count(), mesh.triangles(), "{}", room.key);
        let (lo, hi) = own.boxes[room.room_id - 1];
        let (alo, ahi) = (pd_point(room.lo(), 1.0), pd_point(room.hi(), 1.0));
        assert!(alo.min(ahi).cmpge(lo).all() && alo.max(ahi).cmple(hi).all(), "{}", room.key);
    }

    // As the game loads it: the same rooms, and every portal kept the way it
    // was written through PD's `bg_init_portal`, facing from the room under
    // its opening to the room over it.
    let (dir, w) = loaded("rooms");
    let rooms = &w.stage.rooms;
    assert_eq!((rooms.roomcount(), rooms.portals.len()), (62, 91));
    assert_eq!(w.level.geom.polys.len(), src.collision.len());
    for (i, (o, p)) in openings.iter().zip(&rooms.portals).enumerate() {
        let number = |key: &str| b.level.plan.get(key).unwrap().room_id as u16;
        assert_eq!((p.room1, p.room2), (number(&o.low), number(&o.high)), "{o:?}: PD swapped its rooms");
        assert!(p.metric.normal.distance(pd_axis(o.axis)) < 1e-4, "{o:?}: its normal is {}", p.metric.normal);
        assert!(p.metric.max - p.metric.min < 0.01, "{o:?} is not flat");
        // A line through it crosses from the one room into the other, both ways.
        let c = rooms.portal_get_centre(i);
        let (back, front) = (c - p.metric.normal * 5.0, c + p.metric.normal * 5.0);
        assert_eq!(rooms.portal_find_rooms(back, front, &[p.room1]).0, vec![p.room2], "{o:?}");
        assert_eq!(rooms.portal_find_rooms(front, back, &[p.room2]).0, vec![p.room1], "{o:?}");
        // and it lies in both its rooms' boxes (on the wall they share)
        for room in [p.room1, p.room2] {
            let (blo, bhi) = (rooms.rooms[room as usize].bbmin, rooms.rooms[room as usize].bbmax);
            assert!(p.verts.iter().all(|v| v.cmpge(blo).all() && v.cmple(bhi).all()), "{o:?}");
        }
    }
    // Every floor tile is in a room whose box holds a point just over it,
    // and that room is the one PD finds for a camera standing there.
    let mut p = pd_sim::player::Player::new(&res(), Vec3::ZERO, 0.0, 1, &mut pd_core::rng::Rng::new(1)).unwrap();
    let mut stood = 0;
    for poly in w.level.geom.polys.iter().filter(|p| p.floor) {
        let mid = poly.verts.iter().copied().sum::<Vec3>() / poly.verts.len() as f32;
        let eye = mid + Vec3::Y * 60.0;
        let room = poly.room.unwrap();
        assert!(rooms.bg_room_contains_coord(eye, room as usize), "a floor tile of room {room} at {mid}");
        // (where this is the floor stood on: not under a deck's edge)
        if w.level.cd_find_ground_at_cyl_ctfril(eye, 5.0, &[]).room == Some(room) {
            p.player_move_camera_from_pos_rooms(eye, Some((eye, &[room])), rooms, &w.level);
            assert_eq!(p.cam_room, room as usize, "the camera over {mid}");
            stood += 1;
        }
    }
    assert!(stood > 2500, "{stood} floor tiles stood on");
    // The portals are what joins rooms for the simulants too (a room's
    // neighbours are across its portals): the plan's links, each way.
    for l in &b.level.plan.links {
        let number = |key: &str| b.level.plan.get(key).unwrap().room_id;
        assert!(rooms.bg_rooms_are_neighbours(number(&l.a), number(&l.b)) && rooms.bg_rooms_are_neighbours(number(&l.b), number(&l.a)), "{} - {}", l.a, l.b);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_opening_whose_rooms_both_lean_over_it_gets_a_wider_box() {
    // Two rooms hooked round each other: the opening at x = 200 faces +x, the
    // room behind it reaches far past it and the room in front far back.
    let rect = [Vec3::new(200.0, 0.0, 0.0), Vec3::new(200.0, 0.0, -100.0), Vec3::new(200.0, 200.0, -100.0), Vec3::new(200.0, 200.0, 0.0)];
    let n = (rect[1] - rect[0]).cross(rect[2] - rect[0]).normalize();
    assert_eq!(n, Vec3::X);
    let mut boxes = vec![(Vec3::new(0.0, 0.0, -500.0), Vec3::new(1000.0, 200.0, 0.0)), (Vec3::new(-1000.0, 0.0, -100.0), Vec3::new(300.0, 200.0, 0.0))];
    let why = portal_between(1, 2, rect, &boxes).unwrap_err();
    assert!(why.contains("both rooms' boxes have their middles on the far side"), "{why}");
    widen(&mut boxes[0], rect);
    assert_eq!(boxes[0], (Vec3::new(-602.0, 0.0, -500.0), Vec3::new(1000.0, 200.0, 0.0)));
    let p = portal_between(1, 2, rect, &boxes).unwrap();
    assert_eq!(p.rooms, [1, 2]);
    // One room leaning over is nothing PD minds: its other room says which way.
    let boxes = vec![(Vec3::new(0.0, 0.0, -500.0), Vec3::new(1000.0, 200.0, 0.0)), (Vec3::new(200.0, 0.0, -100.0), Vec3::new(300.0, 200.0, 0.0))];
    assert_eq!(portal_between(1, 2, rect, &boxes).unwrap().rooms, [1, 2]);
    // The portal is wound for PD: clockwise from in front (its normal is the winding's, negated).
    assert_eq!(p.verts, [rect[3], rect[2], rect[1], rect[0]]);
}

#[test]
fn the_level_is_drawn_on_the_pools_textures_with_complexs_materials() {
    let b = seed40();
    let (src, _) = source40();
    assert!(src.textures.is_empty(), "no image of the level's own");
    let complex: Value = serde_json::from_str(&std::fs::read_to_string(assets().join("stages/ref/bg.json")).unwrap()).unwrap();
    for m in &src.materials {
        let pd = m.pd.as_ref().expect("one of PD's materials");
        assert!(complex["materials"].as_array().unwrap().contains(&pd.row), "{:#06x}", pd.texture);
        assert_eq!(pd.row["texture"]["id"].as_u64(), Some(pd.texture as u64));
        assert!(assets().join(format!("textures/{:04x}.png", pd.texture)).is_file(), "{:#06x} is in the pool", pd.texture);
        assert_eq!(pd.texture_row["file"].as_str().unwrap(), format!("textures/{:04x}.png", pd.texture));
    }
    // The palette's flags are the rows': the grate deck is drawn both sides,
    // the rail and the glass both sides in the translucent layer, clamped up
    // the wall as every wall strip is; the floor wraps.
    let used: Vec<usize> = b.rooms.iter().flat_map(|r| r.faces.iter().map(|f| f.material)).fold(Vec::new(), |mut u, m| {
        if !u.contains(&m) {
            u.push(m);
        }
        u
    });
    let of = |role: &str| {
        let at = cx::PALETTE.iter().position(|p| p.role == role).unwrap();
        &src.materials[used.iter().position(|&m| m == at).unwrap_or_else(|| panic!("seed 40 draws no {role}"))]
    };
    let look = |m: &Material| (m.pd.as_ref().unwrap().texture, m.cull_back, m.blend, m.wrap);
    assert_eq!(look(of("floor")), (0x73e, true, Blend::Opaque, [Wrap::Repeat, Wrap::Repeat]));
    assert_eq!(look(of("grate")), (0x73e, false, Blend::Opaque, [Wrap::Repeat, Wrap::Repeat]));
    assert_eq!(look(of("panel")), (0x741, true, Blend::Opaque, [Wrap::Repeat, Wrap::Clamp]));
    assert_eq!(look(of("rail")), (0x748, false, Blend::Translucent, [Wrap::Repeat, Wrap::Clamp]));
    assert_eq!(look(of("window")), (0x190, false, Blend::Translucent, [Wrap::Repeat, Wrap::Clamp]));
    assert!(src.tris.iter().all(|t| t.xlu == (src.materials[t.material].blend == Blend::Translucent)));

    // Written: no texture file, the pool's numbers in bg.json, and vertices
    // in the pool's texels with the bake's bytes as they were.
    let custom = std::env::temp_dir().join(format!("pd_import_autolevel_bg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&custom);
    let paths = crate::Paths { assets: assets(), custom: custom.clone(), src: None };
    let (stage_dir, _) = crate::geometry(&recipe40(), &paths).unwrap();
    assert_eq!(std::fs::read_dir(stage_dir.join("tex")).map_or(0, |d| d.count()), 0, "texture files written");
    let bg: Value = serde_json::from_str(&std::fs::read_to_string(stage_dir.join("bg.json")).unwrap()).unwrap();
    let textures = bg["textures"].as_object().unwrap();
    assert_eq!(textures.len(), 16);
    assert!(textures.keys().all(|k| k.parse::<u32>().unwrap() < 0x10000) && textures.values().all(|t| t["source"] == "pd"), "{textures:?}");
    assert_eq!(bg["rooms"].as_array().unwrap().len(), 61);
    assert_eq!(bg["portals"].as_array().unwrap().len(), 91);
    assert_eq!(bg["batches"].as_array().unwrap().iter().map(|b| b["nidx"].as_u64().unwrap()).sum::<u64>(), 15303 * 3);
    // A floor triangle's texels: 32 to every 39.37 units, in s and in t.
    let floor = src.materials.iter().position(|m| m.pd.as_ref().unwrap().texture == 0x73e && m.cull_back).unwrap();
    let t = src.tris.iter().find(|t| t.material == floor).unwrap();
    let (dp, duv) = (t.v[1].pos - t.v[0].pos, (t.v[1].uv - t.v[0].uv) * 32.0);
    assert!((duv.length() / dp.length() - 32.0 / 39.37).abs() < 1e-3, "{dp} is {duv} texels");
    // The baked colours are the mesh's bytes.
    let mesh = &b.rooms[0];
    let n = b.level.plan.get(&mesh.key).unwrap().room_id as u16;
    let first = src.tris.iter().find(|t| t.room == n).unwrap();
    let f = &mesh.faces[0];
    let c = f.colors[f.tris[0][0] as usize];
    assert_eq!(first.v[0].col, [c[0], c[1], c[2], 255]);
    std::fs::remove_dir_all(&custom).unwrap();
}

#[test]
fn its_tiles_are_pds_rules_and_these_levels_three() {
    let (src, report) = source40();
    // What the glTF source's three options made of the same level.
    assert_eq!(report[1], "tiles: 4 translucent triangles facing up made floors (grates), 38 of 2222 ceiling triangles under 172 cm made walls, 724 wall triangles flagged as ramp walls");
    let count = |f: &dyn Fn(&ColPoly) -> bool| src.collision.iter().filter(|p| f(p)).count();
    let floor = GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2;
    // grates: a translucent triangle facing up is a floor seen and shot through
    assert_eq!(count(&|p| p.flags == floor), 4);
    // every other translucent triangle that is a tile is a wall seen and shot through (a rail, glass)
    assert!(count(&|p| p.flags == GEOFLAG_WALL) > 100);
    // low ceilings: walls facing down, each less than a standing head over a floor
    let low: Vec<&ColPoly> = src.collision.iter().filter(|p| p.flags & GEOFLAG_WALL != 0 && (p.verts[1] - p.verts[0]).cross(p.verts[2] - p.verts[0]).normalize().y < -0.5).collect();
    assert_eq!(low.len(), 38);
    assert!(low.iter().all(|p| (1..=61).contains(&p.room)));
    // ramp walls: upright, with a sloped edge
    assert_eq!(count(&|p| p.flags & GEOFLAG_RAMPWALL != 0), 724);
    // and the rest are PD's: opaque floors block sight and shots, and carry their colour
    let floors = count(&|p| p.flags & floor != 0 && p.flags & GEOFLAG_BLOCK_SIGHT != 0 && p.flags & GEOFLAG_BLOCK_SHOOT != 0);
    assert!(floors > 2500 && count(&|p| p.flags & floor != 0 && p.floorcol != 0) > floors / 2, "{floors} floors");
    assert_eq!(src.collision.len(), 13119);
}

#[test]
fn what_a_player_on_any_route_can_see_is_drawn() {
    let b = seed40();
    let (dir, w) = loaded("sight");
    let rooms = &w.stage.rooms;
    let open = rooms.initial_portal_flags();
    let (mut views, mut rays, mut most) = (0, 0, 0);
    let mut unseen: Vec<String> = Vec::new();
    for (name, steps) in &b.level.walks {
        for s in steps {
            let (x, y, z) = walk::ge(s.x, s.y, s.z as f64);
            let feet = Vec3::new(x as f32, y as f32, z as f32);
            let ground = w.level.cd_find_ground_at_cyl_ctfril(feet + Vec3::Y * 100.0, 30.0, &[]);
            let Some(room) = ground.room else { continue };
            // (standing; squatting where the route squats, in a vent)
            let squat = s.flags.is_some_and(|f| f & 2 != 0);
            let eye = Vec3::new(feet.x, ground.y + if squat { 70.0 } else { 159.0 }, feet.z);
            for turn in 0..8 {
                let t = (turn as f32 * 45.0 + 10.0).to_radians();
                let look = Vec3::new(-t.sin(), 0.0, t.cos());
                let mut c = pd_sim::player::camera::Camera::default();
                c.player_allocate_matrices(eye, look, Vec3::Y);
                let cam = PortalCam {
                    world_to_screen: c.world_to_screen,
                    cam_pos: eye,
                    c_screenleft: c.c_screenleft,
                    c_screentop: c.c_screentop,
                    c_halfwidth: c.c_halfwidth,
                    c_halfheight: c.c_halfheight,
                    c_recipscalex: 1.0 / c.c_scalex,
                    c_recipscaley: 1.0 / c.c_scaley,
                    view: PortalCam::screen_properties(0.0, 0.0, 320.0, 220.0, 320.0, 220.0),
                    zfar: rooms.zrange.1,
                };
                let v = bg_tick_portals(rooms, &open, &cam, room as usize);
                views += 1;
                most = most.max(v.drawslots.len());
                // Rays well inside the view (60 degrees up and down, more across).
                let right = look.cross(Vec3::Y);
                for i in -3..=3 {
                    for j in -2..=2 {
                        let dir = (look + right * (i as f32 * 0.16) + Vec3::Y * (j as f32 * 0.2)).normalize();
                        rays += 1;
                        if let Some(hit) = w.stage.bghit.bg_test_hit(eye, eye + dir * 20000.0) {
                            if !v.is_onscreen(hit.room as usize) && unseen.len() < 8 {
                                unseen.push(format!("{name}: from {eye} in room {room} looking {dir}, room {} at {} is seen and not drawn", hit.room, hit.pos));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(unseen.is_empty(), "{unseen:#?}");
    assert!(views > 2000 && rays > 70_000, "{views} views, {rays} rays");
    // PD draws 60 rooms at most (its draw slots); with rooms that are rooms, a view takes few.
    assert!(most < 40, "{most} rooms in one view");
    std::fs::remove_dir_all(&dir).unwrap();
}
