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
    BUILT.get_or_init(|| built::make(&Seed::python(40), true).unwrap())
}

const RECIPE_40: &str = "{\n  \"format\": 1,\n  \"seed\": 40,\n  \"options\": {\n    \"flooded\": 0.0,\n    \"shaped\": 0.0,\n    \"sunken\": 0.0,\n    \"bases\": false,\n    \"big_pods\": false\n  }\n}";

/// The level's recipe, as `autolevel import` writes it.
fn recipe40() -> Recipe {
    autowalk::recipe("autolevel40", "Autolevel 40", Via::Recipe(RECIPE_40)).unwrap()
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
    assert!(report[0].starts_with("autolevel: seed 40, 61 rooms (0 with nothing drawn), 91 openings between them, 19 materials of the complex kit's (arena ref) on 16 pool textures, baked"), "{report:?}");
    // Every triangle and tile names its room, and lies in its box.
    assert_eq!(src.tris.len(), 15005); // (15,303 before the Rust's pillars stopped at the first storey; 14,995 before their ladders)
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
        let at = b.kit.index(role).unwrap();
        &src.materials[used.iter().position(|&m| m == at).unwrap_or_else(|| panic!("seed 40 draws no {role}"))]
    };
    let look = |m: &Material| (m.pd.as_ref().unwrap().texture, m.cull_back, m.blend, m.wrap);
    assert_eq!(look(of("floor")), (0x73e, true, Blend::Opaque, [Wrap::Repeat, Wrap::Repeat]));
    assert_eq!(look(of("grate")), (0x73e, false, Blend::Opaque, [Wrap::Repeat, Wrap::Repeat]));
    assert_eq!(look(of("panel")), (0x741, true, Blend::Opaque, [Wrap::Repeat, Wrap::Clamp]));
    assert_eq!(look(of("rail")), (0x190, false, Blend::Translucent, [Wrap::Repeat, Wrap::Clamp]));
    assert_eq!(look(of("window")), (0x190, false, Blend::Translucent, [Wrap::Repeat, Wrap::Clamp]));
    assert_eq!(look(of("mesh")), (0x748, false, Blend::Translucent, [Wrap::Repeat, Wrap::Clamp]));
    assert!(src.tris.iter().all(|t| t.xlu == (src.materials[t.material].blend == Blend::Translucent)));

    // Written: no texture file, the pool's numbers in bg.json, and vertices
    // in the pool's texels with the bake's bytes as they were.
    let custom = std::env::temp_dir().join(format!("pd_import_autolevel_bg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&custom);
    let paths = crate::Paths { assets: assets(), custom: custom.clone(), src: None, levels: None };
    let (stage_dir, _) = crate::geometry(&recipe40(), &paths).unwrap();
    assert_eq!(std::fs::read_dir(stage_dir.join("tex")).map_or(0, |d| d.count()), 0, "texture files written");
    let bg: Value = serde_json::from_str(&std::fs::read_to_string(stage_dir.join("bg.json")).unwrap()).unwrap();
    let textures = bg["textures"].as_object().unwrap();
    assert_eq!(textures.len(), 16);
    assert!(textures.keys().all(|k| k.parse::<u32>().unwrap() < 0x10000) && textures.values().all(|t| t["source"] == "pd"), "{textures:?}");
    assert_eq!(bg["rooms"].as_array().unwrap().len(), 61);
    assert_eq!(bg["portals"].as_array().unwrap().len(), 91);
    // 14,995 drawn (15,303 before the Rust's pillars stopped at the first
    // storey), and 30 more where the translucent layers' block trees cut a
    // railing or window across another's plane (`crate::bsp`).
    assert_eq!(bg["batches"].as_array().unwrap().iter().map(|b| b["nidx"].as_u64().unwrap()).sum::<u64>(), 15035 * 3);
    let xlu: Vec<&Value> = bg["nodes"].as_array().unwrap().iter().filter(|n| n["layer"] == "xlu").collect();
    assert!(!xlu.is_empty() && xlu.iter().all(|n| n["tree"].is_array()), "every translucent layer is ordered by a block tree");
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
    assert_eq!(report[1], "tiles: 4 translucent triangles facing up made floors (grates), 38 of 2183 ceiling triangles under 172 cm made walls, 724 wall triangles flagged as ramp walls");
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
    assert_eq!(src.collision.len(), 12860);
}

/// A theme draws the roles it names with another arena's textures, in that
/// arena's own rows (a role culled as none of its rows is: the row, only its
/// "cull" changed), and leaves the roles it doesn't name as Complex's.
#[test]
fn a_theme_dresses_the_roles_it_names_and_leaves_the_rest() {
    let kit = seed40().kit;
    let used: Vec<usize> = ["floor", "grate", "plain", "panel"].iter().map(|r| kit.palette.iter().position(|p| p.role == *r).unwrap()).collect();
    let rows = |stage: &str| -> Vec<Value> {
        let text = std::fs::read_to_string(assets().join("stages").join(stage).join("bg.json")).unwrap();
        serde_json::from_str::<Value>(&text).unwrap()["materials"].as_array().unwrap().clone()
    };
    let (jun, cryp) = (rows("jun"), rows("cryp"));
    // Temple's only row for 0x01b9 is back-culled; the kit's "grate" is drawn both sides.
    let temple = jun.iter().filter(|r| r["texture"]["id"] == 0x1b9).collect::<Vec<_>>();
    assert_eq!(temple.len(), 1);
    assert_eq!(temple[0]["cull"], "back");
    let theme = crate::recipe::Theme {
        stage: "jun".into(),
        roles: [("floor", "01B9"), ("grate", "01B9"), ("panel", "cryp:018D")].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
    };
    let plain = materials(&assets(), kit, &used, None).unwrap();
    let themed = materials(&assets(), kit, &used, Some(&theme)).unwrap();
    let pd = |m: &Material| m.pd.clone().unwrap();
    // floor: Temple's row as it is
    assert_eq!((pd(&themed[0]).texture, &pd(&themed[0]).row), (0x1b9, temple[0]));
    assert!(themed[0].cull_back);
    // grate: the same row drawn both sides, and nothing else changed
    let (mut both, row) = (pd(&themed[1]).row, temple[0].clone());
    assert_eq!((pd(&themed[1]).texture, &both["cull"]), (0x1b9, &Value::from("none")));
    assert!(!themed[1].cull_back);
    both["cull"] = row["cull"].clone();
    assert_eq!(both, row);
    // plain: not named, Complex's own
    assert_eq!((pd(&themed[2]).texture, &pd(&themed[2]).row), (pd(&plain[2]).texture, &pd(&plain[2]).row));
    assert_eq!(pd(&themed[2]).texture, 0x746);
    // panel: "stage:HEX" draws from that stage instead (G5's sandstone, its own row)
    assert_eq!(pd(&themed[3]).texture, 0x18d);
    assert!(cryp.contains(&pd(&themed[3]).row));
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

/// A stage's BG as triangles, for measuring an arena's materials (which
/// texture goes on floors, walls and ceilings, how high, at what scale):
/// `PD_PROBE_STAGE=mp11 PD_PROBE_OUT=tris.json cargo test -p pd_import
/// --release probe_stage_triangles -- --ignored`. Each row: room, texture
/// (or -1), its size, then the three corners as x, y, z, u, v.
#[test]
#[ignore]
fn probe_stage_triangles() {
    use pd_core::model::ModelDef;
    let code = std::env::var("PD_PROBE_STAGE").unwrap_or_else(|_| "mp11".into());
    let out = std::env::var("PD_PROBE_OUT").unwrap_or_else(|_| format!("{code}_tris.json"));
    let dir = assets().join("stages").join(&code);
    let bg: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("bg.json")).unwrap()).unwrap();
    let def = ModelDef::load_file(&pd_core::assets::AssetDir::new(&assets()), &dir.join("bg.json"), &dir.join("bg.bin")).unwrap();
    let mut rows = Vec::new();
    for b in &def.batches {
        let room = bg["nodes"][b.node]["room"].as_i64().unwrap_or(0);
        let tex = def.materials[b.material].texture.as_ref();
        let id = tex.map_or(-1, |t| t.id as i64);
        let size = tex.and_then(|t| bg["textures"][t.id.to_string()].as_object()).map_or([0, 0], |r| [r["w"].as_i64().unwrap_or(0), r["h"].as_i64().unwrap_or(0)]);
        for t in b.idx.chunks(3) {
            // [x, y, z, u, v, r, g, b]: the corner's colour after the uv
            let v: Vec<[f32; 8]> = t.iter().map(|&i| {
                let v = &b.verts[i as usize];
                [v.pos.x, v.pos.y, v.pos.z, v.uv[0], v.uv[1], v.c[0] as f32, v.c[1] as f32, v.c[2] as f32]
            }).collect();
            rows.push(serde_json::json!([room, id, size, v]));
        }
    }
    std::fs::write(&out, serde_json::to_string(&rows).unwrap()).unwrap();
    println!("{} triangles of {code} to {out}", rows.len());
}

/// Where simulants stall on an imported generated level: the importer's
/// check match (six Normal simulants, a minute, seed 1), and for each 3-s
/// stall (a go-to that hasn't moved, as `abtest` counts them) the chr's
/// feet, its room (the generator's key) and the waypoint it is making for:
/// `PD_PROBE_STAGE=felicity10 cargo test -p pd_import --release
/// probe_stalls -- --ignored --nocapture`. The level's build folder is
/// `out/autolevel/<code>` (its recipe names the rooms).
#[test]
#[ignore]
fn probe_stalls() {
    use pd_core::assets::AssetDir;
    use pd_sim::chr::Act;
    let code = std::env::var("PD_PROBE_STAGE").unwrap_or_else(|_| "felicity10".into());
    let root = assets().join("..");
    let a = AssetDir::new(&assets()).with_custom_dir(&root.join("custom"));
    let stage = Arc::new(pd_sim::stage::Stage::load(&a, &code).unwrap());
    let level = Arc::new(pd_sim::stage::TileLevel::for_stage(&stage));
    // (a stage with no recipe, one of PD's arenas: its rooms by number)
    let b = std::fs::read_to_string(root.join("out/autolevel").join(&code).join("recipe.json")).ok().map(|t| built::make(&autolevel::recipe::load(&t).unwrap(), false).unwrap());
    let key = |room: u16| b.as_ref().and_then(|b| b.level.plan.rooms.iter().find(|r| r.room_id == room as usize).map(|r| r.key.clone())).unwrap_or_else(|| format!("room {room}"));
    let res = Arc::new(pd_sim::world::WorldRes::load(&a).unwrap());
    let setup = pd_sim::harness::with_weapons(pd_sim::harness::setup(0, 6, BOTDIFF_NORMAL), &pd_sim::harness::DEFAULT_SET);
    let setup = pd_core::mp::MatchSetup { stagenum: stage.stagenum, ..setup };
    let mut w = pd_sim::world::World::new(setup, stage.clone(), level, res, 1).unwrap();
    let mut still = vec![0i32; w.chrs.len()];
    // PD_PROBE_SECS: how long a match (60 s); each stall's nearest door is told too
    let secs: i32 = std::env::var("PD_PROBE_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let mut stalls = 0;
    // and each fall of more than 100 (the import check's "ledge falls"): from where, to where
    let mut fall_top: Vec<Option<(f32, glam::Vec3)>> = vec![None; w.chrs.len()];
    let mut falls = 0;
    for _ in 0..60 * secs {
        pd_sim::harness::step_idle(&mut w);
        let t = w.lv.lvframe60;
        for k in 0..w.chrs.len() {
            let c = &w.chrs[k];
            match (fall_top[k], c.fallspeed.y != 0.0) {
                (None, true) => fall_top[k] = Some((c.manground, c.pos)),
                (Some((top, from)), false) => {
                    if top - c.manground > 100.0 {
                        let rooms = |p: glam::Vec3| stage.rooms.bg_find_rooms_by_pos(p, 4).0.iter().map(|&r| key(r)).collect::<Vec<_>>();
                        println!("{:5.1}s chr {k} fell {:.0}: from {:.0} {:?} to {:.0} {:?}", t as f32 / 60.0, top - c.manground, from, rooms(from), c.pos, rooms(c.pos));
                        falls += 1;
                    }
                    fall_top[k] = None;
                }
                _ => {}
            }
            let moved = glam::Vec2::new(c.pos.x - c.prevpos.x, c.pos.z - c.prevpos.z).length() > 0.5;
            if w.chr_is_dead(k) || c.actiontype != Act::GoPos || moved {
                still[k] = t;
            } else if t - still[k] == 180 {
                let rooms = stage.rooms.bg_find_rooms_by_pos(c.pos, 4).0;
                let to = c.act_gopos.waypoints.get(c.act_gopos.curindex).copied();
                let towards = to.map(|wp| stage.pads[stage.waypoints[wp].padnum].pos);
                let door = w
                    .props
                    .objs
                    .iter()
                    .filter_map(|o| o.door.as_ref().map(|d| (o.pos.with_y(0.0).distance(c.pos.with_y(0.0)), d.frac, d.mode)))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map_or(String::new(), |(dist, frac, mode)| format!("; door {dist:.0} away, open {frac:.2}, mode {mode}"));
                println!("{:5.1}s chr {k} at {:.0} in {:?}, making for waypoint {to:?} at {towards:?}{door}", t as f32 / 60.0, c.pos, rooms.iter().map(|&r| key(r)).collect::<Vec<_>>());
                stalls += 1;
            }
        }
    }
    println!("{stalls} stalls, {falls} falls in {secs} s");
}
