//! A room copied, pasted, mirrored and saved as a preset (`autolevel::copy`, the editing
//! roadmap's item 8), and the edits it is made of (`draws`, `mirror`), on seed 40.

use autolevel::cascade::diff;
use autolevel::copy::{copy, file_name, load_preset, mirror, paste, save_preset, Copy};
use autolevel::level::{flipped, Level, Options};
use autolevel::recipe::{load, save, Edit, Recipe};

fn made(r: &Recipe) -> Level {
    r.generate().unwrap_or_else(|why| panic!("{:?}: {why}", r.edits))
}

fn s(text: &str) -> String {
    text.to_string()
}

/// The first place (scanning the site) a paste of `c` on its own floor is taken.
fn pasted(r: &Recipe, c: &Copy) -> (Recipe, String) {
    for y in (0..70).step_by(3) {
        for x in (0..90).step_by(3) {
            if let Ok(done) = paste(r, c, (x, y), c.floor) {
                return done;
            }
        }
    }
    panic!("nowhere to paste {}", c.describe())
}

#[test]
fn a_room_drawn_as_itself_is_the_same_and_as_another_only_it_changes() {
    let r = Recipe::python(40);
    let lv = made(&r);
    let same = Recipe { edits: vec![Edit::Draws { room: s("medium3"), from: s("40/medium3") }], ..r.clone() };
    assert!(diff(&lv, &made(&same)).is_empty());
    // drawn as seed 41's: its inside is drawn again, nothing else moves
    let other = Recipe { edits: vec![Edit::Draws { room: s("medium3"), from: s("41/medium3") }], ..r.clone() };
    let c = diff(&lv, &made(&other));
    assert!(!c.is_empty());
    assert_eq!(c.rooms().into_iter().collect::<Vec<_>>(), ["medium3"], "{:?}", c.lines());
    // the file, and what it can't be
    assert_eq!(load(&save(&other)).unwrap(), other);
    assert_eq!(other.edits[0].describe(), "medium3: drawn as 41/medium3");
    for bad in ["vast", "40/", "x/vast", "40/vast/2"] {
        let r = Recipe { edits: vec![Edit::Draws { room: s("medium3"), from: s(bad) }], ..r.clone() };
        assert_eq!(r.generate().unwrap_err(), format!("draws: a seed and a room, as \"40/vast\", not {bad:?}"));
    }
}

#[test]
fn a_copy_pasted_is_the_room_with_its_choices_where_it_is_put() {
    let r = Recipe {
        edits: vec![
            Edit::Cover { room: s("medium3"), cover: s("crates") },
            Edit::Brightness { room: s("medium3"), value: 90 },
            Edit::Piece { room: s("medium3"), at: (64, 26), what: Some(s("crate")), size: None, to: None },
        ],
        ..Recipe::python(40)
    };
    let lv = made(&r);
    let m3 = lv.spaces().iter().find(|s| s.key == "medium3").unwrap().clone();
    let c = copy(&r, &lv, "medium3").unwrap();
    assert_eq!((c.kind.as_str(), c.size, c.storeys, c.floor), ("medium", (m3.x1 - m3.x0, m3.y1 - m3.y0), m3.storeys().len(), m3.floor));
    assert_eq!(c.draws, "40/medium3");
    assert_eq!(c.choices.len(), 2);
    // the crate's cell from the room's low corner, its size as it stood (none where it was not put)
    let size = lv.interior("medium3").cover.iter().any(|c| (c.0, c.1) == (128, 52) && c.5 == "crate").then_some(1);
    assert_eq!(c.puts, [Edit::Piece { room: s("medium3"), at: (64 - m3.x0, 26 - m3.y0), what: Some(s("crate")), size, to: None }]);
    let (p, key) = pasted(&r, &c);
    assert_eq!(key, "room1");
    let after = made(&p);
    let new = after.spaces().iter().find(|s| s.key == key).expect("the paste").clone();
    assert_eq!((new.kind, new.x1 - new.x0, new.y1 - new.y0, new.floor, new.top), (m3.kind, m3.x1 - m3.x0, m3.y1 - m3.y0, m3.floor, m3.top));
    let edits: Vec<String> = p.edits[3..].iter().map(|e| e.describe()).collect();
    let at = (new.x0 + 64 - m3.x0, new.y0 + 26 - m3.y0);
    assert_eq!(
        edits,
        [
            format!("add room1 (medium) at {}, {} to {}, {}, from {} to {}", new.x0, new.y0, new.x1, new.y1, new.floor, new.top),
            s("room1: drawn as 40/medium3"),
            s("room1: cover crates"),
            s("room1: brightness 90"),
            format!("room1: a crate at {}, {}{}", at.0, at.1, if size.is_some() { " (1 cells)" } else { "" }),
        ]
    );
    // its crate is there, or the level says why not
    let stood = after.interior(&key).cover.iter().any(|c| (c.0, c.1) == (2 * at.0, 2 * at.1) && c.5 == "crate");
    assert!(stood || after.unmet.iter().any(|(e, _)| e.room() == key), "{:?}", after.unmet);
    assert_eq!(after.moods.iter().find(|(k, _)| *k == key).map(|(_, v)| *v), Some(90));
    // nowhere it can't go
    assert!(paste(&r, &c, (m3.x0, m3.y0), m3.floor).is_err());
    assert_eq!(
        paste(&r, &c, (2, 2), 100).unwrap_err(),
        "paste: 100 is no storey's floor (one of -276, 0, 276, 551, 827, 1102, 1378, 1654, 1929, 2205, 2480, 2756, 3031, 3307, 3583, 3858, 4134)"
    );
    let tall = Copy { storeys: 2, ..c.clone() };
    assert_eq!(paste(&r, &tall, (2, 2), 4134).unwrap_err(), "paste: 2 storeys from 4134 go over the highest ceiling");
}

#[test]
fn a_hall_comes_with_its_pods_into_another_level() {
    let r = Recipe::python(40);
    let lv = made(&r);
    let c = copy(&r, &lv, "vast").unwrap();
    let pods = lv.spaces().iter().filter(|s| s.host == "vast").count();
    assert!(pods > 0 && c.pods.len() == pods, "{}", c.describe());
    assert_eq!(copy(&r, &lv, &c.pods[0].room.room).unwrap_err(), format!("copy: {} is a pod of vast: copy its hall", c.pods[0].room.room));
    // into an empty start, as the only room: the hall and its pods, drawn as seed 40's
    let (p, key) = paste(&Recipe::empty(7), &c, (4, 4), c.floor).unwrap();
    let after = made(&p);
    let new: Vec<_> = after.spaces().iter().filter(|s| s.host == key).collect();
    assert_eq!(new.len(), pods, "{:?}", p.edits);
    for (pod, n) in new.iter().zip(&c.pods) {
        assert_eq!((pod.x0 - 4, pod.y0 - 4), n.at);
    }
    assert!(p.edits.iter().any(|e| *e == Edit::Draws { room: key.clone(), from: s("40/vast") }));
}

#[test]
fn a_copy_mirrored_flips_its_pods_and_puts_and_twice_is_itself() {
    let r = Recipe {
        edits: vec![
            Edit::Piece { room: s("vast"), at: (40, 30), what: Some(s("crate")), size: None, to: None },
            Edit::Piece { room: s("vast"), at: (41, 25), what: None, size: None, to: Some((40, 32)) },
        ],
        ..Recipe::python(40)
    };
    let lv = made(&r);
    let c = copy(&r, &lv, "vast").unwrap();
    let m = c.mirrored("x");
    assert_eq!(m.mirror, ["x"]);
    assert_eq!(m.mirrored("x"), c);
    assert_eq!(m.mirrored("y").mirrored("y"), m);
    assert_eq!(c.mirrored("z"), c);
    let w = c.size.0;
    let Edit::Piece { at, size: Some(1), .. } = &c.puts[0] else { panic!("{:?}", c.puts) };
    let Edit::Piece { at: flipped_at, .. } = &m.puts[0] else { panic!() };
    assert_eq!(*flipped_at, (w - 1 - at.0, at.1));
    for (a, b) in c.pods.iter().zip(&m.pods) {
        assert_eq!(b.at, (w - a.room.size.0 - a.at.0, a.at.1));
    }
    // pasted, it is mirrored: a mirror edit for the hall
    let (p, key) = paste(&Recipe::empty(7), &m, (4, 4), m.floor).unwrap();
    assert!(p.edits.contains(&Edit::Mirror { room: key.clone(), axis: s("x") }));
    made(&p);
    assert!(m.describe().ends_with("mirrored x)"), "{}", m.describe());
}

#[test]
fn a_room_mirrored_in_place_has_its_cut_and_pods_on_the_other_side() {
    // a shaped room has its cut flipped (the seeds' own levels rarely have one: an empty start's
    // room chosen shaped)
    let add = |key: &str, rect| Edit::Add { key: s(key), kind: s("medium"), rect, floor: 0, top: 236 };
    let r = Recipe {
        edits: vec![add("a", (4, 4, 18, 16)), add("b", (22, 4, 30, 12)), Edit::Feature { room: s("a"), feature: s("shaped"), on: true }],
        ..Recipe::empty(40)
    };
    let lv = made(&r);
    let shaped = lv.spaces().iter().find(|q| q.key == "a").unwrap().clone();
    assert!(!shaped.cuts.is_empty(), "a is not shaped");
    let m = mirror(&r, &lv, &shaped.key, "x").unwrap();
    assert_eq!(m.edits[3..], [Edit::Mirror { room: shaped.key.clone(), axis: s("x") }]);
    let after = made(&m);
    let now = after.spaces().iter().find(|s| s.key == shaped.key).unwrap();
    let want: Vec<_> = shaped.cuts.iter().map(|c| flipped(&shaped, (true, false), *c)).collect();
    // (the flipped cut must still keep clear of what it kept clear of: else another try was taken)
    assert!(now.cuts == want || now.cuts.len() == 1, "{:?} vs {:?}", now.cuts, want);
    assert_ne!(now.cuts, shaped.cuts);
    // twice is none
    assert_eq!(mirror(&m, &after, &shaped.key, "x").unwrap(), r);
    // a hall's pods go to the other side, and back
    let r = Recipe::python(40);
    let lv = made(&r);
    let m = mirror(&r, &lv, "vast", "y").unwrap();
    assert!(m.edits.iter().any(|e| matches!(e, Edit::Move { .. })), "{:?}", m.edits);
    let after = made(&m);
    let (v, before_pods, pods) = (
        lv.spaces().iter().find(|s| s.key == "vast").unwrap(),
        lv.spaces().iter().filter(|s| s.host == "vast").map(|s| (s.key.clone(), s.y0, s.y1)).collect::<Vec<_>>(),
        after.spaces().iter().filter(|s| s.host == "vast").map(|s| (s.key.clone(), s.y0, s.y1)).collect::<Vec<_>>(),
    );
    for (k, y0, y1) in &before_pods {
        let (_, ny0, ny1) = pods.iter().find(|(n, _, _)| n == k).expect("the pod is still vast's");
        assert_eq!((*ny0, *ny1), (v.y0 + v.y1 - y1, v.y0 + v.y1 - y0), "{k}");
    }
    assert_eq!(mirror(&m, &after, "vast", "y").unwrap(), r);
    // what can't be mirrored
    let pod = &before_pods[0].0;
    assert_eq!(mirror(&r, &lv, pod, "x").unwrap_err(), format!("mirror: {pod} is a pod of vast: mirror its hall"));
    assert_eq!(mirror(&r, &lv, "vast", "z").unwrap_err(), "mirror: axis \"x\" or \"y\", not \"z\"");
    let bad = Recipe { edits: vec![Edit::Mirror { room: s("vast"), axis: s("up") }], ..r.clone() };
    assert_eq!(bad.generate().unwrap_err(), "mirror: axis \"x\" or \"y\", not \"up\"");
    assert_eq!(load(&save(&m)).unwrap(), m);
}

#[test]
fn a_preset_is_a_copy_in_a_file() {
    let r = Recipe::python(40);
    let lv = made(&r);
    let c = copy(&r, &lv, "vast").unwrap().mirrored("x");
    let text = save_preset("Pump hall", "seed 40, vast", &c);
    let p = load_preset(&text).unwrap();
    assert_eq!((p.name.as_str(), p.from.as_str(), &p.room), ("Pump hall", "seed 40, vast", &c));
    assert!(text.starts_with("{\n  \"preset\": 1,\n  \"name\": \"Pump hall\""), "{text}");
    assert_eq!(load_preset("{\"preset\": 2}").unwrap_err(), "preset: format 2 is not one this build reads (format 1)");
    assert_eq!(load_preset("{\"name\": \"x\"}").unwrap_err(), "preset: no \"preset\" (this build reads format 1)");
    assert!(load_preset(&text.replace("\"name\"", "\"nmae\"")).unwrap_err().contains("unknown field `nmae`"));
    assert_eq!(file_name("Pump hall!"), "pump_hall");
    assert_eq!(file_name("  "), "preset");
    // it pastes as the copy does
    let (a, _) = paste(&Recipe::empty(3), &p.room, (4, 4), 0).unwrap();
    let (b, _) = paste(&Recipe::empty(3), &c, (4, 4), 0).unwrap();
    assert_eq!(a, b);
    let _ = Options::default();
}
