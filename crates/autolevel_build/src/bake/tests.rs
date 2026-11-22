//! The bake's tests: small rooms made by hand (a floor of squares, a slab, a wall), so none
//! needs a file, and one whole level.

use super::*;
use crate::csg::V3;
use crate::mesh::{self, Face};

fn room(number: usize) -> Room {
    Room { key: format!("r{number}"), name: format!("Room_{number:02X}"), number, verts: Vec::new(), faces: Vec::new() }
}

/// Add a face of the palette's `role`, facing the way its corners turn. Returns its number.
fn face(room: &mut Room, pts: &[V3], role: &str) -> usize {
    let verts: Vec<u32> = pts
        .iter()
        .map(|p| {
            room.verts.iter().position(|v| v == p).unwrap_or_else(|| {
                room.verts.push(*p);
                room.verts.len() - 1
            }) as u32
        })
        .collect();
    let material = cx::KIT.index(role).unwrap();
    room.faces.push(Face {
        uvs: vec![(0.0, 0.0); pts.len()],
        normal: csg::newell(pts),
        material,
        lit: None,
        colors: vec![[255; 3]; pts.len()],
        tris: mesh::triangulate(pts),
        verts,
    });
    room.faces.len() - 1
}

/// A floor of squares `step` wide over x0..x1, y0..y1 at height z, facing up.
fn floor(room: &mut Room, (x0, y0): (i32, i32), (x1, y1): (i32, i32), z: i32, step: i32) {
    for x in (x0..x1).step_by(step as usize) {
        for y in (y0..y1).step_by(step as usize) {
            face(room, &[[x, y, z], [x + step, y, z], [x + step, y + step, z], [x, y + step, z]], "floor");
        }
    }
}

/// A rectangle at height z over x0..x1, y0..y1, facing down.
fn slab(room: &mut Room, (x0, y0): (i32, i32), (x1, y1): (i32, i32), z: i32, role: &str) -> usize {
    face(room, &[[x0, y0, z], [x0, y1, z], [x1, y1, z], [x1, y0, z]], role)
}

/// An upright rectangle from a to b (x, y), z0 to z1.
fn wall(room: &mut Room, a: (i32, i32), b: (i32, i32), z0: i32, z1: i32, role: &str) -> usize {
    face(room, &[[a.0, a.1, z0], [b.0, b.1, z0], [b.0, b.1, z1], [a.0, a.1, z1]], role)
}

fn lamp(pos: [f64; 3], size: f64, spot: f64) -> Light {
    Light { pos, color: [230.0; 3], radius: 900.0, size, spot }
}

/// The colour of the first corner at a point on a face that faces up.
fn at(room: &Room, p: V3) -> [u8; 3] {
    let v = room.verts.iter().position(|v| *v == p).expect("a vertex there") as u32;
    let f = room.faces.iter().find(|f| f.normal[2] > 0.5 && f.verts.contains(&v)).expect("a floor there");
    f.colors[f.verts.iter().position(|x| *x == v).unwrap()]
}

/// Every face of the rooms one colour.
fn fill(room: &mut Room, color: [u8; 3]) {
    for f in &mut room.faces {
        f.colors = vec![color; f.verts.len()];
    }
}

/// The rooms lit with every gain 1 and no tint: the raw trace.
fn raw(rooms: &mut [Room], lights: &[Light], set: &Settings) {
    let scene = Scene::new(rooms, &cx::KIT);
    let traced = scene.trace(rooms, lights, set);
    for (r, t) in rooms.iter_mut().zip(&traced) {
        paint(r, t, lights, 1.0, [1.0; 3]);
    }
}

#[test]
fn a_lamp_lights_the_floor_under_it_more_than_far_away() {
    let set = Settings::default();
    let mut rooms = [room(1)];
    floor(&mut rooms[0], (0, 0), (800, 800), 0, 100);
    raw(&mut rooms, &[lamp([100.0, 100.0, 200.0], 20.0, 0.4)], &set);
    let (under, near, far) = (at(&rooms[0], [100, 100, 0])[0], at(&rooms[0], [300, 100, 0])[0], at(&rooms[0], [800, 800, 0])[0]);
    // straight under: Lambert 1, falloff (1 - 200/900) squared, all seven samples seen
    assert_eq!(under as f64, (52.0 + 230.0 * (7.0f64 / 9.0).powi(2)).round());
    assert!(under > near && near > far, "{under} {near} {far}");
    assert_eq!(far, 52, "past the lamp's radius there is only the ambient light");
    assert!(rooms[0].faces.iter().all(|f| f.colors.iter().all(|c| c[0] == c[1] && c[1] == c[2])), "a grey lamp lights grey");
    // what faces down gets the bounce, and nothing from a lamp above it
    let mut rooms = [room(1)];
    floor(&mut rooms[0], (0, 0), (200, 200), 0, 100);
    let lid = slab(&mut rooms[0], (0, 0), (200, 200), 100, "ceiling_tile");
    raw(&mut rooms, &[lamp([100.0, 100.0, 300.0], 0.0, 0.0)], &set);
    assert!(rooms[0].faces[lid].colors.iter().all(|c| *c == [85; 3]), "{:?}", rooms[0].faces[lid].colors);
}

#[test]
fn a_slab_shadows_the_floor_and_a_see_through_one_does_not() {
    let set = Settings::default();
    let lights = [lamp([400.0, 400.0, 300.0], 0.0, 0.0)];
    let lit = |cover: Option<&str>| {
        let mut rooms = [room(1), room(2)];
        floor(&mut rooms[0], (0, 0), (800, 800), 0, 100);
        if let Some(role) = cover {
            // in another room: everything in the level blocks light, whichever room it is in
            slab(&mut rooms[1], (300, 300), (500, 500), 150, role);
        }
        raw(&mut rooms, &lights, &set);
        (at(&rooms[0], [400, 400, 0])[0], at(&rooms[0], [0, 400, 0])[0])
    };
    let (open, open_side) = lit(None);
    let (shadowed, side) = lit(Some("floor"));
    assert!(open > 150 && open_side > 52, "{open} {open_side}");
    assert_eq!(shadowed, 52, "under the slab only the ambient light is left");
    assert_eq!(side, open_side, "and beside it nothing changes");
    assert_eq!(lit(Some("window")), (open, open_side), "glass lets the light through");
    assert_eq!(lit(Some("rail")), (open, open_side), "and so does a grate");
    // a lamp with a size is half hidden at the slab's edge: a soft shadow
    let soft = |cover: bool| {
        let mut rooms = [room(1)];
        floor(&mut rooms[0], (0, 0), (800, 800), 0, 100);
        if cover {
            slab(&mut rooms[0], (0, 300), (400, 500), 150, "ceiling_tile");
        }
        raw(&mut rooms, &[lamp([400.0, 400.0, 300.0], 20.0, 0.0)], &set);
        at(&rooms[0], [400, 400, 0])[0]
    };
    assert!(soft(true) > 52 && soft(true) < soft(false), "{} {}", soft(true), soft(false));
}

#[test]
fn calibration_brings_each_floor_to_its_target() {
    let set = Settings::default();
    // two rooms far apart, each under its own lamps, and a third that is only a wall
    let mut rooms = [room(1), room(2), room(3)];
    floor(&mut rooms[0], (0, 0), (800, 800), 0, 100);
    floor(&mut rooms[1], (5000, 0), (5800, 800), 0, 100);
    wall(&mut rooms[2], (0, 800), (800, 800), 0, 200, "plain");
    let mut lights = Vec::new();
    for x in [200.0, 600.0] {
        for y in [200.0, 600.0] {
            lights.push(lamp([x, y, 230.0], 20.0, 0.4));
            lights.push(lamp([x + 5000.0, y, 230.0], 20.0, 0.4));
        }
    }
    let scene = Scene::new(&rooms, &cx::KIT);
    let gains = scene.bake(&mut rooms, &lights, &[167.0, 126.0, 167.0], &[None, None, None], &set);
    let reader = Reader::new();
    let med = |i: usize| scene.floor_median(&rooms[i], i, &reader);
    assert!((med(0).unwrap() - 167.0).abs() < 4.0, "{:?}", med(0));
    assert!((med(1).unwrap() - 126.0).abs() < 4.0, "{:?}", med(1));
    assert!(gains[0] > gains[1], "the dimmer room's lamps are turned down: {gains:?}");
    assert!(gains.iter().all(|g| *g == py::round_to(*g, 3)));
    // the wall has no floor to measure: it takes the median gain of the others (the upper of two)
    assert_eq!(med(2), None);
    assert_eq!(gains[2], gains[0]);
    // a room nothing lights can't be brought up: it too takes the others' gain, and stays dark
    let mut dark = [room(1), room(2)];
    floor(&mut dark[0], (0, 0), (800, 800), 0, 100);
    floor(&mut dark[1], (9000, 0), (9800, 800), 0, 100);
    let scene = Scene::new(&dark, &cx::KIT);
    let gains = scene.bake(&mut dark, &lights, &[167.0, 167.0], &[None, None], &set);
    assert_eq!(gains[1], gains[0]);
    assert!(dark[1].faces.iter().all(|f| f.colors.iter().all(|c| *c == [52; 3])));
}

#[test]
fn a_tint_colours_a_room_after_it_is_calibrated() {
    let set = Settings::default();
    let lights: Vec<Light> = [(200.0, 200.0), (600.0, 600.0), (200.0, 600.0), (600.0, 200.0)].iter().map(|(x, y)| lamp([*x, *y, 230.0], 20.0, 0.4)).collect();
    let lit = |tint: Option<[f64; 3]>| {
        let mut rooms = [room(1)];
        floor(&mut rooms[0], (0, 0), (800, 800), 0, 100);
        let scene = Scene::new(&rooms, &cx::KIT);
        let gains = scene.bake(&mut rooms, &lights, &[150.0], &[tint], &set);
        (gains[0], at(&rooms[0], [400, 400, 0]), at(&rooms[0], [200, 200, 0]))
    };
    let (plain, mid, white) = lit(None);
    let (gain, cyan, _) = lit(Some([0.5, 1.0, 1.0]));
    assert_eq!(gain, plain, "the gain is found with no tint");
    assert_eq!((cyan[1], cyan[2]), (mid[1], mid[2]));
    assert!((cyan[0] as f64 - mid[0] as f64 * 0.5).abs() <= 1.0, "{cyan:?} {mid:?}");
    let (_, _, yellow) = lit(Some([1.0, 1.0, 0.7]));
    assert!(yellow[2] < yellow[0] && yellow[0] == white[0] && yellow[1] == white[1], "{yellow:?} {white:?}");
}

#[test]
fn a_beam_brightens_only_the_faces_it_lights() {
    let mut r = room(1);
    let a = face(&mut r, &[[0, 0, 0], [100, 0, 0], [100, 100, 0], [0, 100, 0]], "floor");
    let b = face(&mut r, &[[100, 0, 0], [200, 0, 0], [200, 100, 0], [100, 100, 0]], "floor");
    let w = wall(&mut r, (0, 100), (200, 100), 0, 100, "plain");
    let back = wall(&mut r, (200, 0), (0, 0), 0, 100, "plain");
    fill(&mut r, [30, 40, 250]);
    (r.faces[b].lit, r.faces[w].lit, r.faces[back].lit) = (Some(0), Some(0), Some(1));
    let beam = Beam { direction: [0.3, 0.4, -1.0], apertures: Vec::new(), occluders: Vec::new() };
    assert_eq!(add_beam(&mut r, 0, &beam, 220.0, &Reader::new()), 2);
    // the floor: 220 x 1 / |(0.3, 0.4, -1)|, about 197 more, and no channel past 255
    let add = 220.0 / 1.25f64.sqrt();
    assert!(r.faces[b].colors.iter().all(|c| *c == [(30.0 + add).round() as u8, (40.0 + add).round() as u8, 255]), "{:?}", r.faces[b].colors);
    // the wall it shines on (its normal is -y, the beam goes +y): 0.4 of that
    assert_eq!(r.faces[w].normal, [0.0, -1.0, 0.0]);
    assert!(r.faces[w].colors.iter().all(|c| c[0] == (30.0 + 0.4 * add).round() as u8), "{:?}", r.faces[w].colors);
    // the face next to the lit one shares two of its corners and keeps its colour: a hard edge
    assert!(r.faces[a].colors.iter().all(|c| *c == [30, 40, 250]));
    // another beam's face is left alone, and a face turned away from its own beam gains nothing
    assert!(r.faces[back].colors.iter().all(|c| *c == [30, 40, 250]));
    assert_eq!(add_beam(&mut r, 1, &beam, 220.0, &Reader::new()), 1);
    assert!(r.faces[back].colors.iter().all(|c| *c == [30, 40, 250]));
}

#[test]
fn a_glow_reddens_the_corners_that_see_its_line() {
    // a corridor floor 600 long, with a wall across it at x = 300 (in another room: any room's wall hides the line)
    let corridor = |role: &str| {
        let mut rooms = [room(1), room(2)];
        floor(&mut rooms[0], (0, 0), (600, 200), 0, 100);
        wall(&mut rooms[1], (300, 0), (300, 200), 0, 236, role);
        fill(&mut rooms[0], [120; 3]);
        let n = Scene::new(&rooms, &cx::KIT).glow(&mut rooms[0], 0, (100.0, 100.0), [141.0, 0.0, 0.0], 330.0, &Reader::new());
        assert!(n > 0);
        rooms
    };
    let rooms = corridor("plain");
    let r = &rooms[0];
    assert_eq!(at(r, [100, 100, 0]), [141, 0, 0], "on the line the colour is the glow's own");
    // 100 away: 100/330 of the grey is left
    let w: f64 = 1.0 - 100.0 / 330.0;
    let grey = (120.0 * (1.0 - w)).round() as u8;
    let want = [(120.0 * (1.0 - w) + 141.0 * w).round() as u8, grey, grey];
    assert_eq!(at(r, [200, 100, 0]), want);
    assert_eq!(at(r, [0, 100, 0]), want);
    assert_eq!(at(r, [400, 100, 0]), [120; 3], "300 away is in reach, but the wall hides the line");
    assert_eq!(at(r, [500, 100, 0]), [120; 3], "and 400 away is out of reach");
    // with the wall see-through, the corner behind it glows too
    let far = at(&corridor("window")[0], [400, 100, 0]);
    assert!(far[0] > 120 && far[1] < 120 && far[1] == far[2], "{far:?}");
}

#[test]
fn the_bake_is_the_same_on_one_thread_and_on_many() {
    // a hall with pillars and a deck, enough corners for several threads to share
    let build = || {
        let mut rooms = [room(1), room(2)];
        floor(&mut rooms[0], (0, 0), (1600, 1200), 0, 100);
        for (x, y) in [(400, 400), (800, 400), (1200, 800), (400, 800)] {
            wall(&mut rooms[0], (x, y), (x + 100, y), 0, 236, "plain");
            wall(&mut rooms[0], (x + 100, y), (x + 100, y + 100), 0, 236, "plain");
            wall(&mut rooms[0], (x + 100, y + 100), (x, y + 100), 0, 236, "plain");
            wall(&mut rooms[0], (x, y + 100), (x, y), 0, 236, "plain");
        }
        floor(&mut rooms[1], (0, 0), (1600, 300), 276, 100);
        slab(&mut rooms[1], (0, 0), (1600, 300), 261, "ceiling_tile");
        rooms
    };
    let mut lights = Vec::new();
    for i in 0..4 {
        for j in 0..3 {
            lights.push(lamp([200.0 + 400.0 * i as f64, 200.0 + 400.0 * j as f64, 500.0], 20.0, 0.4));
        }
    }
    let run = |threads: usize| {
        let mut rooms = build();
        let scene = Scene::new(&rooms, &cx::KIT);
        let gains = scene.bake(&mut rooms, &lights, &[167.0, 150.0], &[None, Some([1.0, 1.0, 0.7])], &Settings { threads, ..Settings::default() });
        (rooms, gains)
    };
    let one = run(1);
    let shades: std::collections::HashSet<u8> = one.0[0].faces.iter().flat_map(|f| &f.colors).map(|c| c[0]).collect();
    assert!(shades.len() > 40, "a floor of many shades, some in the pillars' shadows: {}", shades.len());
    for threads in [2, 3, 8, 0] {
        assert!(run(threads) == one, "{threads} threads");
    }
    // and the answers come back in order
    assert_eq!(each(1000, 7, |i| i * 3), (0..1000).map(|i| i * 3).collect::<Vec<_>>());
    assert_eq!(each(0, 4, |i| i), Vec::<usize>::new());
}

#[test]
fn a_stored_byte_reads_back_a_hair_off() {
    // which bytes the Python reads back below themselves in Blender 5.2 (the rest read above, 0 exactly):
    // it decides whether a floor exactly 4 from its target is calibrated once more
    let low: Vec<usize> = [11, 12, 15, 16, 18, 19, 21, 22, 23, 24].into_iter().chain(26..=47).chain([85, 255]).collect();
    let reader = Reader::new();
    assert_eq!(reader.read(0), 0.0);
    for b in 1..=255usize {
        let x = reader.read(b as u8) - b as f64;
        assert!(x != 0.0 && x.abs() < 4e-5, "{b}: {x}");
        assert_eq!(x < 0.0, low.contains(&b), "{b}: {x}");
    }
    assert_eq!((byte(-3.0), byte(0.49), byte(0.5), byte(254.6), byte(300.0)), (0, 0, 1, 255, 255));
}

#[test]
fn a_level_is_lit() {
    use crate::geometry;
    use autolevel::level::{generate, Options};
    let lv = generate(40, &Options::before_bases()).unwrap();
    let (polys, lit) = geometry::polys(&lv, &cx::KIT).unwrap();
    let mut rooms = mesh::emit(&lv, &polys, &cx::KIT).unwrap();
    let set = Settings::default();
    let gains = light(&lv, &mut rooms, &lit, &cx::KIT, &set).unwrap();
    // a gain for every room, in the plan's order
    let names: Vec<String> = lv.plan.rooms.iter().map(|r| r.object_name()).filter(|n| rooms.iter().any(|r| r.name == *n)).collect();
    assert_eq!(gains.iter().map(|g| g.0.clone()).collect::<Vec<_>>(), names);
    assert_eq!(gains.len(), 61);
    assert!(gains.iter().all(|g| (0.1..=10.0).contains(&g.1)), "{gains:?}");
    let scene = Scene::new(&rooms, &cx::KIT);
    let reader = Reader::new();
    let (mut tinted, mut low) = (0, 0);
    for (i, r) in rooms.iter().enumerate() {
        let p = lv.plan.get(&r.key).unwrap();
        let glows = lv.glows.iter().any(|g| g.0 == r.key);
        let beams = lit.iter().any(|l| l.room == r.key);
        let grey = r.faces.iter().flat_map(|f| &f.colors).all(|c| c[0] == c[1] && c[1] == c[2]);
        // a room with neither a tint nor a glow is grey
        assert_eq!(grey, p.tint.is_empty() && !glows, "{}", r.name);
        if p.tint == "cyan" && !glows {
            tinted += 1;
            assert!(r.faces.iter().flat_map(|f| &f.colors).all(|c| c[0] <= c[1] && c[1] == c[2]), "{}", r.name);
        }
        // the low road is dim: its floors sit at 126, not 167 (a beam's patch aside)
        if p.scheme == "low" && p.kind != "vent" && !beams && !glows && p.tint.is_empty() {
            low += 1;
            let med = scene.floor_median(r, i, &reader).unwrap();
            assert!((med - set.low_floor).abs() < CLOSE + 1.0, "{}: {med}", r.name);
        }
    }
    assert!(tinted + low > 0, "the level has rooms that are not the usual");
    // a beam leaves a hard edge: somewhere a lit floor and an unlit one share a vertex and differ there
    for (b, l) in lit.iter().enumerate() {
        let r = rooms.iter().find(|r| r.key == l.room).unwrap();
        let bright = |f: &Face, v: u32| f.colors[f.verts.iter().position(|x| *x == v).unwrap()][1] as i32;
        let floors = |lit: Option<usize>| r.faces.iter().filter(move |f| f.lit == lit && f.normal[2] > 0.5);
        let edge = floors(Some(b)).any(|f| f.verts.iter().any(|&v| floors(None).any(|g| g.verts.contains(&v) && bright(f, v) > bright(g, v) + 60)));
        assert!(edge || floors(Some(b)).count() == 0, "beam {b} into {}", r.name);
    }
    // a glow paints its corridor's bend red
    assert!(!lv.glows.is_empty());
    for (key, _) in &lv.glows {
        let r = rooms.iter().find(|r| r.key == *key).unwrap();
        assert!(r.faces.iter().flat_map(|f| &f.colors).any(|c| c[0] as i32 > c[1] as i32 + 50), "{}", r.name);
    }
}
