//! A recipe's edits on real seeds (40, 7 and 1): each does what it says, each error case gives
//! its message, the same recipe gives the same level, and a few dozen edited levels are either
//! whole or refused for a reason a person can read. (The edits that were there first, `join`,
//! `remove`, `keep`, `cut`, `brightness` and `ceiling`, are tested in `src/recipe.rs`.)

use autolevel::furnish::Interior;
use autolevel::level::{Level, Options};
use autolevel::place::{self, Placed, Scale, Siting};
use autolevel::recipe::{load, save, Doorway, Edit, Recipe};
use autolevel::scatter::Lot;

fn s(text: &str) -> String {
    text.to_string()
}

fn mv(room: &str, to: (i32, i32)) -> Edit {
    Edit::Move { room: s(room), to }
}

fn resize(room: &str, size: (i32, i32)) -> Edit {
    Edit::Resize { room: s(room), size }
}

fn add(key: &str, kind: &str, rect: (i32, i32, i32, i32), floor: i32, top: i32) -> Edit {
    Edit::Add { key: s(key), kind: s(kind), rect, floor, top }
}

fn delete(room: &str) -> Edit {
    Edit::Delete { room: s(room) }
}

fn pin(room: &str, floor: i32) -> Edit {
    Edit::Pin { room: s(room), floor }
}

fn cover(room: &str, cover: &str) -> Edit {
    Edit::Cover { room: s(room), cover: s(cover) }
}

fn layout(room: &str, layout: &str) -> Edit {
    Edit::Layout { room: s(room), layout: s(layout) }
}

fn reroll(room: &str, n: u32) -> Edit {
    Edit::Reroll { room: s(room), n, part: None }
}

fn recipe(seed: u64, edits: Vec<Edit>) -> Recipe {
    Recipe { seed, options: Options::python(), edits }
}

fn made(seed: u64, edits: Vec<Edit>) -> Level {
    let r = recipe(seed, edits);
    r.generate().unwrap_or_else(|why| panic!("{:?}: {why}", r.edits))
}

fn err(seed: u64, edits: Vec<Edit>) -> String {
    match recipe(seed, edits).generate() {
        Ok(_) => panic!("a level, where an error was expected"),
        Err(why) => why,
    }
}

/// `err` with no rescue (`Options::rescue`): where a room no set of ramps can join fails the level.
fn err_unrescued(seed: u64, edits: Vec<Edit>) -> String {
    let r = Recipe { options: Options { rescue: false, ..Options::python() }, ..recipe(seed, edits) };
    match r.generate() {
        Ok(_) => panic!("a level, where an error was expected"),
        Err(why) => why,
    }
}

/// The lots after the lot edits, or why they may not stand so (no level is made).
fn lots(seed: u64, edits: Vec<Edit>) -> Result<Placed, String> {
    recipe(seed, edits).placed()
}

fn lot<'a>(p: &'a Placed, key: &str) -> &'a Lot {
    p.lots.iter().find(|a| a.key == key).unwrap_or_else(|| panic!("no lot {key}"))
}

/// What two levels would have to share to be the same level.
fn same(a: &Level, b: &Level) -> bool {
    a.report == b.report
        && a.plan == b.plan
        && a.walks == b.walks
        && a.views == b.views
        && a.fixes == b.fixes
        && a.dead_ends == b.dead_ends
        && a.moods == b.moods
}

/// A room's inside, as text: two that read the same are the same.
fn inside(it: &Interior) -> String {
    format!("{:?}", (it.cover_mode, &it.cover, it.layout, it.balcony, it.tower, it.shape, it.shaped, it.pillar, &it.deck, &it.ramps, &it.rails, &it.bridge))
}

/// The share of a room's walls that has deck along it on storey `z`.
fn decked(it: &Interior, z: i32) -> f64 {
    let have: std::collections::BTreeSet<i32> = it.deck[&z].iter().flat_map(|c| it.slot(*c)).collect();
    have.len() as f64 / (2 * (it.w + it.d)) as f64
}

// ---------------------------------------------------------------- the rule

#[test]
fn the_seeds_own_lots_keep_the_rule_edited_ones_are_held_to() {
    let mut levels = 0;
    let sitings = [
        (Siting::default(), 1..=60u64),
        (Siting { scale: Scale::Standard, ..Siting::default() }, 1..=30),
        (Siting { shift: 0, ..Siting::default() }, 1..=20),
        (Siting { size: 2.0, ..Siting::default() }, 1..=6),
    ];
    for (o, seeds) in sitings {
        for seed in seeds {
            let Ok(own) = place::lots(seed, &o) else { continue };
            for band in [2, 4] {
                Placed::new(own.placed.clone(), &o).check(band).unwrap_or_else(|why| panic!("seed {seed} {o:?}: {why}"));
            }
            // and whose pod each lot is follows from where they are: a move to where a lot
            // already is works it out again, and nothing changes
            let mut p = Placed::new(own.placed.clone(), &o);
            let first = own.placed[0].clone();
            p.move_to(&first.key, (first.x0, first.y0)).unwrap();
            assert_eq!(p.lots, own.placed, "seed {seed} {o:?}");
            levels += 1;
        }
    }
    assert!(levels > 100, "{levels}");
}

// ---------------------------------------------------------------- move

#[test]
fn a_room_is_moved_and_later_edits_see_it_there() {
    let plain = made(40, vec![]);
    let lv = made(40, vec![mv("medium3", (62, 24))]);
    assert_eq!((plain.space("medium3").rect(), lv.space("medium3").rect()), ((61, 3, 68, 11), (62, 24, 69, 32)));
    assert_eq!((lv.space("medium3").floor, lv.space("medium3").top), (0, 236));
    for q in plain.spaces().iter().filter(|q| q.key != "medium3") {
        assert_eq!((lv.space(&q.key).rect(), lv.space(&q.key).floor), (q.rect(), q.floor), "{}", q.key);
    }
    assert!(!same(&lv, &plain) && !lv.interior("medium3").doors.is_empty());
    // a join by hand is made to the room where it now is
    let join = Edit::Join { room: s("medium3"), storey: 0, to: s("medium1"), on: vec![], door: None, to_door: None, via: None };
    let joined = made(40, vec![join.clone(), mv("medium3", (62, 24))]); // the move comes first whatever the list's order
    assert_eq!(joined.net.routes().len(), lv.net.routes().len() + 1);
    let r = joined.net.routes().iter().find(|r| r.a.room == "medium3" && r.b.room == "medium1").expect("the corridor asked for");
    let unmoved = made(40, vec![join]);
    let far = unmoved.net.routes().iter().find(|r| r.a.room == "medium3" && r.b.room == "medium1").expect("the corridor asked for");
    assert!(r.length() < 12.0 && far.length() > 2.0 * r.length(), "{} {}", r.length(), far.length());
}

#[test]
fn a_halls_pods_move_with_it_and_a_pod_moved_clear_is_a_room() {
    // the vast hall has two pods, small8 on 551 and tiny2 on 276
    let p = lots(40, vec![mv("vast", (38, 21))]).unwrap();
    assert_eq!((lot(&p, "vast").rect(), lot(&p, "small8").rect(), lot(&p, "tiny2").rect()), ((38, 21, 48, 39), (45, 30, 49, 33), (39, 29, 41, 31)));
    assert_eq!((lot(&p, "small8").host.as_str(), lot(&p, "tiny2").host.as_str()), ("vast", "vast"));
    let lv = made(40, vec![mv("vast", (38, 21))]);
    assert!(lv.pods.iter().any(|(k, _)| k == "small8") && lv.pods.iter().any(|(k, _)| k == "tiny2") && lv.dropped.is_empty());
    // moved clear of the hall, small8 is a room of its own on the storey it was on
    let lv = made(40, vec![mv("small8", (60, 40))]);
    let room = lv.space("small8");
    assert_eq!((room.rect(), room.floor, room.top, room.host.as_str()), ((60, 40, 64, 43), 551, 787, ""));
    assert!(lv.pods.iter().all(|(k, _)| k != "small8") && !lv.interior("small8").doors.is_empty());
    assert_eq!(lv.interior("vast").pods.len(), 1);
    // and a small room moved onto a hall, on one of its pod storeys, is its pod
    let lv = made(40, vec![mv("small6", (19, 20))]);
    assert_eq!((lv.space("small6").host.as_str(), lv.space("small6").floor), ("colossal", 276));
    assert!(lv.pods.iter().any(|(k, _)| k == "small6") && lv.interior("colossal").pods.iter().any(|q| q.key == "small6"));
}

#[test]
fn a_move_that_breaks_the_rule_says_which_lots_and_why() {
    let why = |edits: Vec<Edit>| lots(40, edits).unwrap_err();
    assert_eq!(
        why(vec![mv("medium3", (60, 14))]),
        "long and medium3 overlap: rooms keep a cell between them, unless one is over the other with a 40-unit slab between \
         (long is at 49, 16 to 67, 21, from 0 to 512; medium3 is at 60, 14 to 67, 22, from 0 to 236)"
    );
    // wall to wall is as bad: there is no cell for the wall
    assert!(why(vec![mv("medium3", (59, 5))]).starts_with("spire1 and medium3 touch: rooms keep a cell between them"));
    assert_eq!(why(vec![mv("medium3", (75, 3))]), "medium3 is off the site (it is at 75, 3 to 82, 11, from 0 to 236; the site is 80 x 60 cells)");
    // a room that lands in a hall is its pod, and no floor changes unless a pin asks
    assert_eq!(
        why(vec![mv("small4", (40, 22))]),
        "small4 lands in vast, so it is its pod, and a pod's floor is one of its hall's first two upper storeys: 276, 551, not 0"
    );
    assert!(lots(40, vec![mv("small4", (40, 22)), pin("small4", 276)]).is_ok());
    assert!(lots(40, vec![pin("small4", 276), mv("small4", (40, 22))]).is_ok()); // (only the end is checked)
    assert_eq!(why(vec![mv("tiny2", (36, 28))]), "tiny2 reaches 1 cell into vast: a pod reaches at least 2 cells in (a room of its own stands a cell clear)");
    assert_eq!(why(vec![mv("small8", (38, 22)), resize("small8", (7, 6))]), "small8 takes 42 of vast's 180 cells of floor: a pod takes a fifth at most");
    assert_eq!(why(vec![mv("attic", (1, 1))]), "move: no room attic");
    assert_eq!(why(vec![delete("tiny4"), mv("tiny4", (1, 1))]), "move: tiny4 was deleted by an earlier edit");
    // a room over another needs the scatter's slab, 40 units: 512 to 551 is 39
    assert!(lots(40, vec![mv("small8", (60, 40)), mv("small8", (50, 30))]).is_err_and(|why| why.starts_with("medium1 and small8 overlap")));
    // and whatever the lots, the level is the generator's to refuse (or, by default, to rescue:
    // a room no set of ramps can join is joined with ladders and bridges)
    let high = vec![mv("small4", (40, 22)), pin("small4", 276)];
    assert_eq!(err_unrescued(40, high.clone()), "seed 40: vast: no set of ramps joins its storeys");
    made(40, high); // (the dead-end pass then changes vast's doors: its last inside needs no rescue)
}

// ---------------------------------------------------------------- resize

#[test]
fn a_room_is_resized_from_its_low_corner() {
    let plain = made(40, vec![]);
    let lv = made(40, vec![resize("medium3", (5, 5))]);
    assert_eq!(lv.space("medium3").rect(), (61, 3, 66, 8));
    assert!(!same(&lv, &plain));
    // a spire's spiral is laid out again for its new walls, the same way round
    let lv = made(40, vec![resize("spire1", (11, 10))]);
    let (was, is) = (plain.space("spire1"), lv.space("spire1"));
    assert_eq!((was.rect(), is.rect()), ((49, 4, 59, 14), (49, 4, 60, 14)));
    assert_eq!((was.preset.len(), is.preset.len()), (5, 5));
    assert!(was.preset.iter().zip(&is.preset).all(|(a, b)| (a.lo, a.hi, a.dir) == (b.lo, b.hi, b.dir)));
    assert!(was.preset.iter().zip(&is.preset).any(|(a, b)| a.rect != b.rect));
    assert!(lv.walks.iter().any(|(name, _)| name == "climb_spire1"));
    // a hall's pods stay where they are: shrunk from under them, they are rooms
    let p = lots(40, vec![resize("vast", (6, 6))]).unwrap();
    assert_eq!((lot(&p, "small8").rect(), lot(&p, "small8").host.as_str(), lot(&p, "tiny2").host.as_str()), ((44, 29, 48, 32), "", ""));
    let why = |edits: Vec<Edit>| lots(40, edits).unwrap_err();
    assert_eq!(why(vec![resize("medium3", (1, 5))]), "resize: a room is at least 2 x 2 cells, not 1 x 5");
    assert_eq!(
        why(vec![resize("spire1", (7, 10))]),
        "spire1 is a spire: its ramps climb walls at least 8 cells long, through two storeys or more (it is 7 x 10, from -276 to 1338)"
    );
    assert!(why(vec![resize("spire1", (12, 9))]).starts_with("spire1 and medium3 touch: "));
    assert_eq!(why(vec![resize("attic", (4, 4))]), "resize: no room attic");
}

// ---------------------------------------------------------------- add

#[test]
fn a_room_is_added_of_one_of_the_scales_kinds() {
    let plain = made(40, vec![]);
    let lv = made(40, vec![add("annexe", "medium", (62, 30, 69, 36), 0, 512)]);
    assert_eq!(lv.spaces().len(), plain.spaces().len() + 1);
    let room = lv.spaces().last().unwrap(); // after the scattered ones
    assert_eq!((room.key.as_str(), room.kind, room.rect(), room.floor, room.top), ("annexe", "medium", (62, 30, 69, 36), 0, 512));
    assert!(!lv.interior("annexe").doors.is_empty() && lv.walks.iter().any(|(name, _)| name == "room_annexe"));
    // its kind names it
    let names = autolevel::level::NAMES.iter().find(|(k, _)| *k == "medium").unwrap().1;
    assert!(names.contains(&lv.plan.get("annexe").unwrap().name.as_str()));
    // a small one that lands in a hall, on one of its pod storeys, is its pod
    let lv = made(40, vec![add("perch", "small", (3, 20, 6, 24), 551, 787)]);
    assert_eq!(lv.space("perch").host, "colossal");
    assert!(lv.pods.iter().any(|(k, _)| k == "perch"));
    // the low road's ceiling is spelt as the scatter spells it, so a room can go under another
    let p = lots(40, vec![add("cellar", "small", (62, 4, 66, 8), -276, -39)]).unwrap();
    assert_eq!((lot(&p, "cellar").floor, lot(&p, "cellar").top, lot(&p, "medium3").floor), (-276, -40, 0));
    assert!(lots(40, vec![delete("tiny4"), add("tiny4", "tiny", (58, 25, 60, 27), 276, 512)]).is_ok()); // a key that is free again

    let why = |edit: Edit| lots(40, vec![edit]).unwrap_err();
    assert_eq!(why(add("vast", "medium", (62, 30, 69, 36), 0, 512)), "add: there is a room vast already");
    assert_eq!(why(add("c07", "medium", (62, 30, 69, 36), 0, 512)), "add: c07 could be a corridor's or a vent's key: give the room another");
    assert_eq!(why(add("v02", "medium", (62, 30, 69, 36), 0, 512)), "add: v02 could be a corridor's or a vent's key: give the room another");
    assert!(why(add("c12b", "medium", (62, 30, 69, 36), 0, 512)).contains("could be a corridor's"));
    assert!(lots(40, vec![add("cab", "medium", (62, 30, 69, 36), 0, 512), add("vault2", "medium", (71, 30, 78, 36), 0, 512)]).is_ok());
    for key in ["Annexe", "an/nexe", "2nd", "", "an@0", "a_key_that_goes_on_and_on_and_on"] {
        assert_eq!(
            why(add(key, "medium", (62, 30, 69, 36), 0, 512)),
            format!("add: a room's key is a lower-case letter, then letters, digits or _ (24 at most), not {key:?}")
        );
    }
    assert_eq!(
        why(add("annexe", "attic", (62, 30, 69, 36), 0, 512)),
        "add: no kind attic at this scale: one of colossal, spire, vast, tower, long, basin, medium, loft, small, tiny, atrium, base"
    );
    let standard = Recipe {
        options: Options { siting: Siting { scale: Scale::Standard, ..Siting::default() }, ..Options::default() },
        ..recipe(40, vec![add("annexe", "loft", (2, 2, 8, 8), 551, 787)])
    };
    assert_eq!(standard.placed().unwrap_err(), "add: no kind loft at this scale: one of vast, tower, long, basin, medium, small, tiny, atrium, base");
    assert_eq!(why(add("annexe", "medium", (62, 30, 63, 36), 0, 512)), "add: a room is at least 2 x 2 cells, not 1 x 6");
    assert_eq!(
        why(add("annexe", "medium", (62, 30, 69, 36), 0, 500)),
        "add: annexe's top would be at 500: ceilings are at -39, 236, 512, 787, 1063, 1338, 1614, 1890, 2165, 2441, 2716, 2992, 3267, 3543, 3819, 4094, 4370"
    );
    assert_eq!(
        why(add("annexe", "medium", (62, 30, 69, 36), 100, 512)),
        "add: annexe's floor would be at 100: floors are at -276, 0, 276, 551, 827, 1102, 1378, 1654, 1929, 2205, 2480, 2756, 3031, 3307, 3583, 3858, 4134"
    );
    assert_eq!(why(add("annexe", "medium", (62, 30, 69, 36), 276, 236)), "add: annexe would be under a storey tall (276 to 236)");
    assert!(why(add("annexe", "medium", (60, 20, 67, 26), 0, 512)).starts_with("long and annexe overlap: "));
    assert!(why(add("annexe", "spire", (62, 30, 69, 40), 0, 1614)).starts_with("annexe is a spire: its ramps climb walls at least 8 cells long"));
    // a room drawn by hand may go off the site, past any edge (the user's call, 2026-11-24); the
    // seed's own stay on it (`medium3` moved off it is refused, above)
    // (accepted and made; this one is 18 cells of corridor from the rest, and the dead-end pass
    // leaves it out as it would anywhere: `keep` holds it)
    let off = recipe(40, vec![add("annexe", "medium", (76, 30, 83, 36), 0, 512)]);
    assert!(off.placed().unwrap().lots.iter().any(|a| a.key == "annexe" && a.x1 == 83));
    let kept = recipe(40, vec![add("annexe", "medium", (76, 30, 83, 36), 0, 512), Edit::Keep { room: "annexe".into() }]);
    assert!(kept.generate().unwrap().spaces().iter().any(|s| s.key == "annexe" && s.x1 == 83));
    let r = recipe(40, vec![add("annexe", "medium", (76, 30, 83, 36), 0, 512), mv("annexe", (82, 30))]);
    assert!(r.placed().unwrap().lots.iter().any(|a| a.key == "annexe" && a.x0 == 82), "a drawn room moves anywhere too");
    let empty = Recipe {
        edits: vec![
            add("west", "medium", (-16, -8, -4, 4), 0, 236),
            add("middle", "medium", (0, -8, 12, 4), 0, 236),
            add("east", "medium", (16, -8, 28, 4), 0, 236),
        ],
        ..Recipe::empty(40)
    };
    let lv = empty.generate().unwrap();
    assert!(lv.spaces().iter().any(|s| s.key == "west" && s.x0 == -16 && s.y0 == -8));
    assert!(lv.net.routes().iter().any(|r| r.a.room == "west" || r.b.room == "west"), "west is joined");
}

#[test]
fn a_pod_put_where_nothing_reaches_it_is_an_error_not_left_out() {
    // four pods in the colossal hall, a cell apart round a gap: each has its way in
    let tiny = |key: &str, x: i32, y: i32| add(key, "tiny", (x, y, x + 2, y + 2), 276, 512);
    let ring = vec![tiny("north", 8, 36), tiny("south", 8, 30), tiny("east", 11, 33), tiny("west", 5, 33)];
    let lv = made(40, ring.clone());
    assert!(lv.dropped.is_empty() && ["north", "south", "east", "west"].iter().all(|k| lv.pods.iter().any(|(q, _)| q == k)));
    // a fifth in the gap is walled in by their shells. The generator leaves such a pod out of
    // a seed's own level without a word; one a person put there is refused
    let walled: Vec<Edit> = ring.into_iter().chain([tiny("mid", 8, 33)]).collect();
    assert!(recipe(40, walled.clone()).placed().is_ok()); // the lots themselves keep the rule
    assert_eq!(err(40, walled), "mid has no way in from colossal: a pod needs room in its hall for a door, and a deck from it to one of the hall's walls");
}

// ---------------------------------------------------------------- delete

#[test]
fn a_room_is_deleted_with_its_pods_and_edits_that_name_it_say_so() {
    let plain = made(40, vec![]);
    let lv = made(40, vec![delete("tiny4")]);
    assert_eq!(lv.spaces().len(), plain.spaces().len() - 1);
    assert!(lv.spaces().iter().all(|q| q.key != "tiny4") && lv.plan.get("tiny4").is_none());
    assert!(lv.net.routes().iter().all(|r| r.a.room != "tiny4" && r.b.room != "tiny4"));
    assert!(lv.fixes.iter().all(|f| !matches!(f, autolevel::build::Fix::Dropped { room } if room == "tiny4"))); // deleted, not left out by the pass
                                                                                                                // a hall goes with its pods
    let lv = made(40, vec![delete("vast")]);
    assert_eq!(lv.spaces().len(), plain.spaces().len() - 3);
    assert!(lv.spaces().iter().all(|q| !matches!(q.key.as_str(), "vast" | "small8" | "tiny2")));

    assert_eq!(err(40, vec![delete("vast"), reroll("tiny2", 1)]), "reroll: tiny2 was deleted with its hall vast");
    assert_eq!(err(40, vec![delete("tiny4"), cover("tiny4", "none")]), "cover: tiny4 was deleted");
    // an edit of a later stage is applied after the delete wherever it is in the list
    let join = Edit::Join { room: s("tiny4"), storey: 276, to: s("small1"), on: vec![], door: None, to_door: None, via: None };
    assert_eq!(err(40, vec![join, delete("tiny4")]), "join: tiny4 was deleted");
    assert_eq!(err(40, vec![delete("tiny4"), Edit::Keep { room: s("tiny4") }]), "keep: tiny4 was deleted");
    assert_eq!(err(40, vec![delete("tiny4"), Edit::Brightness { room: s("tiny4"), value: 99 }]), "brightness: tiny4 was deleted");
    let why = |edits: Vec<Edit>| lots(40, edits).unwrap_err();
    assert_eq!(why(vec![delete("tiny4"), delete("tiny4")]), "delete: tiny4 was deleted by an earlier edit");
    assert_eq!(why(vec![delete("vast"), pin("small8", 276)]), "pin: small8 was deleted with its hall vast by an earlier edit");
    assert_eq!(why(vec![delete("attic")]), "delete: no room attic");
    // (a move then a delete is no error: the room was there to move)
    assert!(lots(40, vec![mv("tiny4", (70, 40)), delete("tiny4")]).is_ok());
    let all: Vec<Edit> = lots(40, vec![]).unwrap().lots.iter().filter(|a| a.host.is_empty()).map(|a| delete(&a.key)).collect();
    assert_eq!(lots(40, all).unwrap_err(), "delete: without tiny5 the level has no room at all");
}

// ---------------------------------------------------------------- pin

#[test]
fn a_rooms_floor_is_pinned_to_a_storey() {
    let plain = made(40, vec![]);
    let lv = made(40, vec![pin("loft3", 276)]);
    assert_eq!((plain.space("loft3").floor, plain.space("loft3").top), (551, 1063));
    assert_eq!((lv.space("loft3").floor, lv.space("loft3").top, lv.space("loft3").rect()), (276, 787, plain.space("loft3").rect()));
    assert!(lv.interior("loft3").doors.iter().all(|d| d.level == 276));
    // a pod to its hall's other pod storey
    let lv = made(40, vec![pin("tiny2", 551)]);
    assert_eq!((lv.space("tiny2").floor, lv.space("tiny2").top, lv.space("tiny2").host.as_str()), (551, 787, "vast"));
    // a hall's pods go as many storeys with it (storeys are not all as far apart)
    let p = lots(40, vec![delete("loft1"), delete("loft2"), pin("vast", 276)]).unwrap();
    let at = |key: &str| (lot(&p, key).floor, lot(&p, key).top, lot(&p, key).host.clone());
    assert_eq!((at("vast"), at("tiny2"), at("small8")), ((276, 1063, s("")), (551, 787, s("vast")), (827, 1063, s("vast"))));
    // a pod pinned clear of its hall's height is a room of its own, over it
    let p = lots(7, vec![pin("small3", 1378)]).unwrap();
    assert_eq!((lot(&p, "small3").floor, lot(&p, "small3").top, lot(&p, "small3").host.as_str()), (1378, 1614, ""));

    let why = |edits: Vec<Edit>| lots(40, edits).unwrap_err();
    assert_eq!(
        why(vec![pin("loft2", 300)]),
        "pin: loft2's floor would be at 300: floors are at -276, 0, 276, 551, 827, 1102, 1378, 1654, 1929, 2205, 2480, 2756, 3031, 3307, 3583, 3858, 4134"
    );
    assert_eq!(why(vec![pin("loft2", 4134)]), "pin: loft2 would stand from 4134 to 4646, above the highest ceiling (4370)");
    assert_eq!(
        why(vec![pin("tiny2", 0)]),
        "tiny2 lands in vast, so it is its pod, and a pod's floor is one of its hall's first two upper storeys: 276, 551, not 0"
    );
    assert!(why(vec![pin("small8", 827)]).starts_with("loft2 and small8 overlap: ")); // over the hall, where a loft already is
    assert!(why(vec![pin("vast", 276)]).starts_with("vast and loft1 overlap: ")); // 1063 to 1102 is a slab of 39
                                                                                  // (only lots that are no lawful pods can be carried off the top: here one over the basin)
                                                                                  // (with the atrium's storeys a pod carried this high has a storey, but none a pod may be on)
    assert_eq!(
        why(vec![mv("small8", (33, 42)), pin("basin", 1378)]),
        "small8 lands in basin, so it is its pod, and a pod's floor is one of its hall's first two upper storeys: basin has none a pod fits"
    );
    assert_eq!(
        why(vec![delete("loft1"), pin("basin", 827), mv("tiny3", (33, 42)), pin("tiny3", 1102)]),
        "tiny3 lands in basin, so it is its pod, and a pod's floor is one of its hall's first two upper storeys: basin has none a pod fits"
    );
    assert_eq!(why(vec![pin("attic", 0)]), "pin: no room attic");
}

// ---------------------------------------------------------------- cover

#[test]
fn a_rooms_cover_is_chosen_and_nothing_else_of_its_dress() {
    let plain = recipe(40, vec![]).furnished().unwrap().0;
    let (was, hall) = (plain.interior("vast").unwrap(), plain.interior("colossal").unwrap());
    assert_eq!((was.cover_mode, hall.cover_mode, hall.cover.len()), ("lanes", "lanes", 1));
    let b = recipe(40, vec![cover("vast", "pillars"), cover("colossal", "none"), cover("small4", "crates")]).furnished().unwrap().0;
    let is = b.interior("vast").unwrap();
    assert_eq!((is.cover_mode, is.shape, is.pillar, is.layout), ("pillars", was.shape, was.pillar, was.layout));
    assert!(is.cover.len() > was.cover.len() && is.cover.iter().all(|c| c.5 == was.pillar), "{:?}", is.cover);
    assert!(b.interior("colossal").unwrap().cover.is_empty());
    assert_eq!(b.interior("small4").unwrap().cover_mode, "crates"); // real crates, asked for by name
                                                                    // no other room is touched
    for (key, it) in plain.interiors.iter().filter(|(k, _)| !matches!(k.as_str(), "vast" | "colossal" | "small4")) {
        assert_eq!(inside(it), inside(b.interior(key).unwrap()), "{key}");
    }
    // the last word counts, and "mixed" is what a level of mixed cover gives the room
    let b = recipe(40, vec![cover("vast", "pillars"), cover("vast", "mixed")]).furnished().unwrap().0;
    assert_eq!(inside(b.interior("vast").unwrap()), inside(was));
    let pillars = Recipe { options: Options { cover: "pillars", ..Options::default() }, ..recipe(40, vec![cover("vast", "mixed")]) };
    assert_eq!(pillars.furnished().unwrap().0.interior("vast").unwrap().cover_mode, "lanes");
    // the level is whole, and the choice holds through the dead-end pass, which furnishes again
    let lv = made(40, vec![cover("vast", "pillars")]);
    assert!(!lv.fixes.is_empty() && lv.interior("vast").cover_mode == "pillars");

    assert_eq!(err(40, vec![cover("vast", "hedges")]), "cover: one of ('mixed', 'pillars', 'crates', 'none')");
    assert_eq!(err(40, vec![cover("small8", "none")]), "cover: small8 is a pod of vast: it is not furnished by itself");
    assert_eq!(err(40, vec![cover("attic", "none")]), "cover: no room attic");
}

// ---------------------------------------------------------------- layout

#[test]
fn a_halls_layout_is_chosen_and_never_swapped_for_another() {
    let plain = recipe(40, vec![]).furnished().unwrap().0;
    let (hall, vast) = (plain.interior("colossal").unwrap(), plain.interior("vast").unwrap());
    assert_eq!((hall.layout, vast.layout), (None, Some("galleries")));
    let of = |lay: &str| recipe(40, vec![layout("colossal", lay)]).furnished().unwrap().0.interior("colossal").unwrap().clone();
    // a ring: deck all the way round, but for where a pod stands in it
    let ring = of("ring");
    assert_eq!(ring.layout, Some("ring"));
    assert!(decked(&ring, 276) > 0.9 && decked(hall, 276) < 0.7, "{} {}", decked(&ring, 276), decked(hall, 276));
    // galleries: both long walls (the hall is 20 x 27 cells: its long walls are 54 half cells)
    let galleries = of("galleries");
    let along = |it: &Interior, x: i32| (it.y0..it.y1).filter(|&y| it.deck[&276].contains(&(x, y))).count();
    assert_eq!(galleries.layout, Some("galleries"));
    assert!(
        along(&galleries, galleries.x0) >= 48 && along(&galleries, galleries.x1 - 1) >= 48,
        "{} {}",
        along(&galleries, galleries.x0),
        along(&galleries, galleries.x1 - 1)
    );
    // a mezzanine: a floor across one end, 16 half cells deep in a hall this long
    let mezzanine = of("mezzanine");
    let rows = |it: &Interior, y: i32| (it.x0..it.x1).filter(|&x| it.deck[&276].contains(&(x, y))).count() as i32;
    let deep = |it: &Interior| (it.y0..it.y1).filter(|&y| rows(it, y) >= it.w - 8).count();
    assert_eq!(mezzanine.layout, Some("mezzanine"));
    assert!(deep(&mezzanine) >= 16 && deep(hall) <= 2 && deep(&ring) <= 8, "{} {} {}", deep(&mezzanine), deep(hall), deep(&ring));
    // none: the galleries the seed drew for the vast hall are gone
    let none = recipe(40, vec![layout("vast", "none")]).furnished().unwrap().0.interior("vast").unwrap().clone();
    assert_eq!(none.layout, None);
    assert!(none.deck[&276].len() < vast.deck[&276].len());
    // each is a whole level, with the layout still there after the dead-end pass
    for lay in ["ring", "galleries", "mezzanine"] {
        let lv = made(40, vec![layout("colossal", lay)]);
        assert!(lv.interior("colossal").layout == Some(lay) && !lv.fixes.is_empty(), "{lay}");
    }

    assert_eq!(err(40, vec![layout("vast", "balcony")]), "layout: one of ('ring', 'galleries', 'mezzanine', 'none')");
    assert_eq!(err(40, vec![layout("small4", "ring")]), "layout: small4 has one storey: a layout is for a hall's first upper storey");
    assert_eq!(err(40, vec![layout("tower", "ring")]), "layout: tower is a tower: its decks are its own");
    assert_eq!(err(40, vec![layout("spire1", "none")]), "layout: spire1 is a spire: its decks are its own");
    assert_eq!(err(40, vec![layout("long", "ring")]), "layout: long is too small for a ring: it is 18 x 5 cells, and that needs 8 by 8");
    assert_eq!(err(40, vec![layout("long", "galleries")]), "layout: long is too small for galleries: it is 18 x 5 cells, and that needs 7 by 8");
    assert_eq!(err(40, vec![layout("medium1", "mezzanine")]), "layout: medium1 is too small for a mezzanine: it is 7 x 5 cells, and that needs 5 by 10");
    assert_eq!(err(40, vec![layout("small8", "ring")]), "layout: small8 is a pod of vast: it is not furnished by itself");
    assert_eq!(err(40, vec![layout("attic", "ring")]), "layout: no room attic");
    // one that fits the rules but not the room, with its doors and its pool, is refused by name
    // (with no rescue; by default the room keeps it, joined by ladders and bridges)
    assert_eq!(err_unrescued(40, vec![layout("basin", "ring")]), "seed 40: basin cannot be furnished with a ring (basin: no set of ramps joins its storeys)");
    let basin = made(40, vec![layout("basin", "ring")]);
    assert!(basin.interior("basin").layout == Some("ring") && basin.interior("basin").rescue);
    // (so is none, where the hall's upstairs doors can only be joined by one: seed 12's)
    assert_eq!(
        err_unrescued(12, vec![layout("colossal", "none")]),
        "seed 12: colossal cannot be furnished with no layout (colossal: no set of ramps joins its storeys)"
    );
    let colossal = made(12, vec![layout("colossal", "none")]);
    assert!(colossal.interior("colossal").layout.is_none() && colossal.interior("colossal").rescue);
    // and so is one a cut then takes down again (seed 7's basin has no door upstairs)
    let cut = Edit::Cut { room: s("basin"), above: 0 };
    assert!(made(7, vec![layout("basin", "galleries")]).interior("basin").layout == Some("galleries"));
    assert_eq!(
        err(7, vec![layout("basin", "galleries"), cut]),
        "layout: basin keeps no galleries: its decks are cut above 0 (by a cut, or by the dead-end pass: keep basin's dead end)"
    );
}

// ---------------------------------------------------------------- reroll

#[test]
fn a_rooms_inside_is_rolled_again_from_another_stream() {
    let plain = recipe(40, vec![]).furnished().unwrap().0;
    let roll = |room: &str, n: u32| recipe(40, vec![reroll(room, n)]).furnished().unwrap().0;
    // 0 is the seed's own
    let b = roll("vast", 0);
    assert!(plain.interiors.iter().all(|(k, it)| inside(it) == inside(b.interior(k).unwrap())));
    // another stream is another inside for that room, the same every time, and for no other
    let b = roll("vast", 1);
    assert_ne!(inside(b.interior("vast").unwrap()), inside(plain.interior("vast").unwrap()));
    assert_eq!(b.interior("vast").unwrap().doors, plain.interior("vast").unwrap().doors);
    assert_eq!(inside(b.interior("vast").unwrap()), inside(roll("vast", 1).interior("vast").unwrap()));
    assert_ne!(inside(b.interior("vast").unwrap()), inside(roll("vast", 2).interior("vast").unwrap()));
    for (key, it) in plain.interiors.iter().filter(|(k, _)| k != "vast") {
        assert_eq!(inside(it), inside(b.interior(key).unwrap()), "{key}");
    }
    // the dress is rolled again too, so a room's shape can change (medium3 has a cross wall)
    assert_eq!(plain.interior("medium3").unwrap().shape, Some("partition"));
    let shapes: Vec<Option<&str>> = (1..=8).map(|n| roll("medium3", n).interior("medium3").unwrap().shape).collect();
    assert!(shapes.contains(&None) && shapes.contains(&Some("notch")), "{shapes:?}");
    // with a cover and a layout chosen as well: all three hold, in a whole level
    let lv = made(40, vec![reroll("vast", 3), cover("vast", "pillars"), layout("vast", "ring")]);
    let it = lv.interior("vast");
    assert_eq!((it.cover_mode, it.layout), ("pillars", Some("ring")));
    assert!(!same(&lv, &made(40, vec![reroll("vast", 4), cover("vast", "pillars"), layout("vast", "ring")])));

    assert_eq!(err(40, vec![reroll("small8", 1)]), "reroll: small8 is a pod of vast: it is not furnished by itself");
    assert_eq!(err(40, vec![reroll("attic", 1)]), "reroll: no room attic");
}

fn part(room: &str, part: &str, n: u32) -> Edit {
    Edit::Reroll { room: s(room), n, part: Some(s(part)) }
}

#[test]
fn a_part_of_a_rooms_inside_is_rolled_again_and_the_rest_stays() {
    let plain = recipe(40, vec![]).furnished().unwrap().0;
    let roll = |edits: Vec<Edit>| recipe(40, edits).furnished().unwrap().0;
    let was = plain.interior("vast").unwrap();
    // the parts of an inside, as text
    let decks = |it: &Interior| format!("{:?}", (&it.deck, &it.bridge, it.bridges, &it.shapes));
    let ramps = |it: &Interior| format!("{:?}", it.ramps);
    let placed = |it: &Interior| format!("{:?}", (it.cover_mode, &it.cover));
    // the cover: where it stands changes, the decks and the ramps under it do not (crates are
    // drawn; vast's lane pillars stand by rule)
    assert_eq!(was.cover_mode, "lanes");
    let crates = || cover("vast", "crates");
    let base = roll(vec![crates()]);
    let old = base.interior("vast").unwrap();
    let b = roll(vec![crates(), part("vast", "cover", 1)]);
    let it = b.interior("vast").unwrap();
    assert_ne!(placed(it), placed(old));
    assert_eq!((decks(it), ramps(it), it.cover_mode), (decks(old), ramps(old), "crates"));
    assert_eq!(placed(it), placed(roll(vec![crates(), part("vast", "cover", 1)]).interior("vast").unwrap()), "the same every time");
    assert_ne!(placed(it), placed(roll(vec![crates(), part("vast", "cover", 2)]).interior("vast").unwrap()));
    for (key, it) in base.interiors.iter().filter(|(k, _)| k != "vast") {
        assert_eq!(inside(it), inside(b.interior(key).unwrap()), "{key}");
    }
    // the ramps: where they go changes, what was laid before them does not (the decks are
    // trimmed where a ramp lands, so they follow the ramps)
    let laid = |it: &Interior| format!("{:?}", (it.layout, it.balcony, it.tower, &it.bridge, it.bridges, &it.shapes));
    let rolled: Vec<String> = (1..=4).map(|n| ramps(roll(vec![part("vast", "ramps", n)]).interior("vast").unwrap())).collect();
    assert!(rolled.iter().any(|r| *r != ramps(was)), "a ramp moves on some stream");
    for n in 1..=4 {
        assert_eq!(laid(roll(vec![part("vast", "ramps", n)]).interior("vast").unwrap()), laid(was));
    }
    // the catwalks: another set of decks on some stream
    let rolled: Vec<String> = (1..=4).map(|n| decks(roll(vec![part("colossal", "catwalks", n)]).interior("colossal").unwrap())).collect();
    assert!(rolled.iter().any(|d| *d != decks(plain.interior("colossal").unwrap())), "the catwalks change on some stream");
    // no other room's inside moves, for any of them
    for e in [part("medium3", "cover", 1), part("vast", "ramps", 2), part("colossal", "catwalks", 3)] {
        let b = roll(vec![e.clone()]);
        let room = e.rooms()[0];
        for (key, it) in plain.interiors.iter().filter(|(k, _)| k != room) {
            assert_eq!(inside(it), inside(b.interior(key).unwrap()), "{key} under {}", e.describe());
        }
    }
    // with the whole inside re-rolled too, the part goes on from that one: only its part moves
    let whole = roll(vec![crates(), reroll("vast", 2)]);
    let both = roll(vec![crates(), reroll("vast", 2), part("vast", "cover", 1)]);
    let (w, b) = (whole.interior("vast").unwrap(), both.interior("vast").unwrap());
    assert_eq!((decks(w), ramps(w)), (decks(b), ramps(b)));
    assert_ne!(placed(w), placed(b));
    // windows: a hall's windows go elsewhere along its walls, or to rail, on some stream; no room's
    // inside moves
    let lv = made(40, vec![]);
    let hall = lv.windows.first().expect("seed 40 has windows").hall.clone();
    let theirs = |lv: &Level| format!("{:?}", lv.windows.iter().filter(|w| w.hall == hall).collect::<Vec<_>>());
    let rolled: Vec<Level> = (1..=6).map(|n| made(40, vec![part(&hall, "windows", n)])).collect();
    assert!(rolled.iter().any(|r| theirs(r) != theirs(&lv)), "{hall}'s windows change on some stream");
    for r in &rolled {
        assert!(lv.interiors.iter().all(|(k, it)| inside(it) == inside(r.interior(k))));
    }
    // the file both ways, and the errors
    let r = recipe(40, vec![part("vast", "catwalks", 2), reroll("vast", 1)]);
    let text = save(&r);
    assert!(text.contains("\"part\": \"catwalks\""), "{text}");
    assert_eq!(load(&text).unwrap(), r);
    assert!(!save(&recipe(40, vec![reroll("vast", 1)])).contains("part"));
    assert_eq!(err(40, vec![part("vast", "pillars", 1)]), "reroll: a part is one of catwalks, ramps, cover, windows, not \"pillars\"");
    assert_eq!(err(40, vec![part("small8", "cover", 1)]), "reroll: small8 is a pod of vast: it is not furnished by itself");
    // 0 is the room's own: the same level
    assert!(same(&made(40, vec![part("vast", "cover", 0)]), &lv));
}

// ---------------------------------------------------------------- the file, and twice the same

/// One recipe with all eight of the new edits in it, which makes a level.
fn all_eight() -> Recipe {
    recipe(
        40,
        vec![
            cover("vast", "pillars"),
            mv("medium3", (62, 24)),
            resize("medium3", (6, 6)),
            add("annexe", "medium", (62, 5, 69, 12), 0, 512),
            delete("tiny4"),
            pin("tiny2", 551),
            layout("colossal", "ring"),
            reroll("long", 2),
        ],
    )
}

#[test]
fn a_recipe_comes_back_from_its_file_and_makes_the_same_level_twice() {
    let r = all_eight();
    let text = save(&r);
    assert_eq!(load(&text).unwrap(), r);
    assert_eq!(save(&load(&text).unwrap()), text);
    for name in ["move", "resize", "add", "delete", "pin", "layout", "cover", "reroll"] {
        assert!(text.contains(&format!("\"edit\": \"{name}\"")), "{name}");
    }
    let lv = r.generate().unwrap();
    assert!(same(&lv, &r.generate().unwrap()) && same(&lv, &load(&text).unwrap().generate().unwrap()));
    // every one of them is in the level
    assert_eq!((lv.space("medium3").rect(), lv.space("annexe").kind, lv.space("tiny2").floor), ((62, 24, 68, 30), "medium", 551));
    assert!(lv.spaces().iter().all(|q| q.key != "tiny4"));
    assert_eq!((lv.interior("vast").cover_mode, lv.interior("colossal").layout), ("pillars", Some("ring")));
    assert!(lv.dead_ends.iter().all(|e| e.allowed));
    // and with no edits the level is the seed's own, for each seed used here
    for seed in [40, 7, 1] {
        let own = autolevel::level::generate(seed, &Options::python()).unwrap();
        assert!(same(&made(seed, vec![]), &own), "seed {seed}");
        assert_eq!(recipe(seed, vec![]).placed().unwrap().lots, place::lots(seed, &Siting::default()).unwrap().placed);
    }
}

// ---------------------------------------------------------------- a few dozen edited levels

/// What an edited level may be refused for: (what the message holds, whose rule it is).
const REASONS: [(&str, &str); 16] = [
    (": rooms keep a cell between them", "the lots: two overlap or touch"),
    ("is off the site", "the lots: off the site"),
    ("so it is its pod", "the lots: a pod on the wrong storey"),
    ("a pod reaches at least", "the lots: a pod too shallow"),
    ("a pod takes a fifth at most", "the lots: a pod too big"),
    ("has no way in from", "the lots: a pod with no way in"),
    ("is a spire: its ramps", "the lots: a spire too small"),
    ("pin: ", "pin: no such storey, or too high"),
    ("layout: ", "layout: the room cannot take one"),
    ("cannot be furnished with", "layout: the room cannot be furnished with it"),
    ("rooms cut off", "the generator's own: rooms cut off"),
    ("no set of ramps joins its storeys", "the generator's own: a room cannot be furnished"),
    ("doors don't all join", "the generator's own: a room cannot be furnished"),
    ("too narrow for a bridge", "the generator's own: a room cannot be furnished"),
    ("cut the level in two", "the generator's own: the dead-end pass cut the level in two"),
    ("plan check", "the generator's own: the plan's check"),
];

/// A deterministic handful of edits for a seed, of every one of the eight kinds.
fn handful(seed: u64) -> Vec<Vec<Edit>> {
    let own = recipe(seed, vec![]).placed().unwrap();
    let rooms: Vec<&Lot> = own.lots.iter().filter(|a| a.host.is_empty()).collect();
    let mut out: Vec<Vec<Edit>> = Vec::new();
    // the first rooms a few cells along, each a different way
    let ways = [(3, 0), (0, -3), (-4, 2), (2, 5), (-2, -2), (6, 1)];
    for (a, (dx, dy)) in rooms.iter().zip(ways) {
        out.push(vec![mv(&a.key, (a.x0 + dx, a.y0 + dy))]);
    }
    // lofts to other storeys, and a pod to its hall's other one
    for (n, a) in own.lots.iter().filter(|a| a.kind == "loft").enumerate() {
        out.push(vec![pin(&a.key, [276, 827, 551, 1378][n % 4])]);
    }
    if let Some(pod) = own.lots.iter().find(|a| !a.host.is_empty()) {
        out.push(vec![pin(&pod.key, if pod.floor == 276 { 551 } else { 276 })]);
    }
    // small rooms deleted, and a hall with its pods
    for a in rooms.iter().filter(|a| matches!(a.kind, "small" | "tiny")).take(3) {
        out.push(vec![delete(&a.key)]);
    }
    out.push(vec![delete("vast")]);
    // rooms a cell bigger or smaller
    for (n, a) in rooms.iter().filter(|a| matches!(a.kind, "medium" | "spire")).take(4).enumerate() {
        let grow = if n % 2 == 0 { 1 } else { -1 };
        out.push(vec![resize(&a.key, (a.x1 - a.x0 + grow, a.y1 - a.y0 + grow))]);
    }
    // a room added in the first free place found, on the ground and up on 551
    for (key, kind, w, d, floor, top) in [("annexe", "medium", 7, 6, 0, 236), ("eyrie", "loft", 6, 5, 551, 787)] {
        let free = (2..own.site.0 - w - 2).flat_map(|x| (2..own.site.1 - d - 2).map(move |y| (x, y))).find(|&(x, y)| {
            let mut p = own.clone();
            p.add(key, kind, (x - 2, y - 2, x + w + 2, y + d + 2), floor, top).is_ok() && p.check(2).is_ok()
        });
        let (x, y) = free.expect("a site has a free corner");
        out.push(vec![add(key, kind, (x, y, x + w, y + d), floor, top)]);
    }
    // halls dressed, laid out and rolled again
    for (room, what) in [("colossal", "none"), ("vast", "pillars"), ("basin", "crates"), ("long", "mixed")] {
        out.push(vec![cover(room, what)]);
    }
    for (room, lay) in [("colossal", "ring"), ("colossal", "mezzanine"), ("vast", "ring"), ("vast", "none"), ("basin", "galleries"), ("long", "mezzanine")] {
        out.push(vec![layout(room, lay)]);
    }
    for (room, n) in [("colossal", 1), ("vast", 2), ("basin", 1), ("tower", 3)] {
        out.push(vec![reroll(room, n)]);
    }
    out
}

/// Every edit of the handful gives a level, or an error that is one of `REASONS`. The counts
/// are printed (`cargo test --test recipe -- --nocapture`). With `AUTOLEVEL_EDITS_DIR` set, the
/// recipe of each level made is saved there, to be built and walked by the command line.
fn whole_or_refused(seed: u64) {
    let handful = handful(seed);
    let (mut levels, mut refused): (usize, Vec<(&str, String)>) = (0, Vec::new());
    for (n, edits) in handful.iter().enumerate() {
        let r = recipe(seed, edits.clone());
        match r.generate() {
            Ok(lv) => {
                if let Ok(dir) = std::env::var("AUTOLEVEL_EDITS_DIR") {
                    std::fs::write(format!("{dir}/seed{seed}_{n:02}_{}.json", edits[0].name()), save(&r)).expect("the folder is there to write to");
                }
                // whole: one piece, every room furnished, walks for every route
                assert!(autolevel::network::components(&lv.net) == 1 && lv.report.walks > 30, "{edits:?}");
                assert_eq!(lv.interiors.len() + lv.pods.len(), lv.spaces().len(), "{edits:?}");
                levels += 1;
            }
            Err(why) => {
                let reason = REASONS.iter().find(|(part, _)| why.contains(part)).unwrap_or_else(|| panic!("{edits:?}: {why}"));
                refused.push((reason.1, format!("{}: {why}", edits[0].describe())));
            }
        }
    }
    println!("seed {seed}: {} edited levels, {levels} whole, {} refused", handful.len(), refused.len());
    for (reason, why) in &refused {
        println!("  [{reason}] {why}");
    }
    assert!(levels * 2 > handful.len(), "seed {seed}: only {levels} of {} edits made a level", handful.len());
}

#[test]
fn edited_levels_of_seed_40_are_whole_or_refused_for_a_reason() {
    whole_or_refused(40);
}

#[test]
fn edited_levels_of_seed_7_are_whole_or_refused_for_a_reason() {
    whole_or_refused(7);
}

#[test]
fn edited_levels_of_seed_1_are_whole_or_refused_for_a_reason() {
    whole_or_refused(1);
}

// ---- an empty start (`"start": "empty"`, docs/AUTOLEVEL.md, "Blank levels: your edit wins") ----

/// An empty start with these edits.
fn empty(seed: u64, edits: Vec<Edit>) -> Recipe {
    Recipe { edits, ..Recipe::empty(seed) }
}

/// Two rooms a corridor joins (the first test of the design: a room, then another).
fn two() -> Vec<Edit> {
    vec![add("room1", "medium", (30, 20, 40, 28), 0, 512), add("room2", "vast", (44, 16, 58, 34), 0, 787)]
}

fn name(lv: &Level, key: &str) -> String {
    lv.plan.rooms.iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no room {key}")).name.clone()
}

fn row<T: Clone>(rows: &[(String, T)], key: &str) -> Option<T> {
    rows.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

#[test]
fn an_empty_start_has_no_room_until_one_is_added() {
    assert_eq!(Recipe::empty(40).generate().unwrap_err(), "no room yet: an empty start (draw one)");
    let one = empty(40, two()[..1].to_vec()).generate().unwrap();
    assert_eq!(one.spaces().iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["room1"]);
    let lv = empty(40, two()).generate().unwrap();
    assert_eq!(lv.spaces().len(), 2);
    assert!(!lv.net.routes().is_empty(), "the two rooms are joined");
    // the file says so, and a seed's recipe does not mention it
    let text = save(&empty(40, two()));
    assert!(text.contains("\"start\": \"empty\""), "{text}");
    assert_eq!(load(&text).unwrap(), empty(40, two()));
    assert!(!save(&Recipe::new(40)).contains("start"));
    // a level made from a seed alone cannot start empty: its rooms are the recipe's
    let o = Recipe::empty(40).options;
    assert!(autolevel::level::generate(40, &o).unwrap_err().contains("recipe"));
    let felicity = Options { style: "felicity", ..o };
    assert_eq!(felicity.check().unwrap_err(), "an empty start is for a complex level, not a felicity one");
}

#[test]
fn an_empty_start_may_lose_its_last_room() {
    let mut edits = two();
    edits.extend([delete("room1"), delete("room2")]);
    assert_eq!(empty(40, edits).generate().unwrap_err(), "no room yet: an empty start (draw one)");
}

#[test]
fn in_an_empty_start_the_seed_routes_nothing_but_decorates() {
    // the router draws no random numbers: the same rooms, the same network on any seed. (The
    // dead-end pass after it can differ: it measures walks over the rooms' decks, which the seed
    // furnishes, so a seed whose deck leaves a dead end gets a corridor that another does not.)
    let routed = |seed| empty(seed, two()).routed().unwrap().1.routes().iter().map(|r| r.path.clone()).collect::<Vec<_>>();
    assert_eq!(routed(40), routed(41));
    let a = empty(40, two()).generate().unwrap();
    // (and the seed does reach the decoration: some room of the two is named, lit or roofed
    // otherwise on one of a few seeds)
    let looks = |lv: &Level| ["room1", "room2"].map(|k| (name(lv, k), row(&lv.moods, k), row(&lv.ceilings, k)));
    assert!((41..46).any(|seed| looks(&empty(seed, two()).generate().unwrap()) != looks(&a)));
}

#[test]
fn in_an_empty_start_a_room_added_elsewhere_leaves_the_others_as_they_were() {
    let before = empty(40, two()).generate().unwrap();
    for (n, far) in [add("room3", "small", (8, 44, 13, 49), 0, 236), add("room3", "tower", (64, 40, 72, 49), 0, 1063)].into_iter().enumerate() {
        let mut edits = two();
        edits.push(far);
        let after = empty(40, edits).generate().unwrap();
        for k in ["room1", "room2"] {
            assert_eq!(name(&after, k), name(&before, k), "{n}: {k}'s name");
            assert_eq!(row(&after.moods, k), row(&before.moods, k), "{n}: {k}'s brightness");
            assert_eq!(row(&after.ceilings, k), row(&before.ceilings, k), "{n}: {k}'s ceiling");
        }
    }
}

#[test]
fn in_an_empty_start_a_drop_stays_where_it_was_when_a_room_is_added_elsewhere() {
    // a room over another, a slab between: the upper one has a hole into the lower
    let stack = || {
        vec![
            add("room1", "medium", (30, 20, 40, 28), 0, 236),
            add("room2", "medium", (30, 20, 40, 28), 276, 512),
            add("room3", "vast", (44, 16, 58, 34), 0, 787),
        ]
    };
    let before = empty(40, stack()).generate().unwrap();
    assert!(!before.drops.is_empty(), "the stacked rooms have a drop");
    let mut edits = stack();
    edits.push(add("room4", "small", (8, 44, 13, 49), 0, 236));
    let after = empty(40, edits).generate().unwrap();
    assert_eq!(after.drops, before.drops);
}

// ---- an atrium (`place::ATRIUM`; docs/AUTOLEVEL.md, "Atriums") ----

/// An atrium fifteen storeys tall (room1), a base on its top storey, and seven rooms round it,
/// each two storeys lower than the one before: the trek from its top down to its floor. The
/// prototype the trek planner is to write edits like.
fn atrium_trek() -> Vec<Edit> {
    vec![
        add("room1", "atrium", (30, 22, 48, 36), 0, 4094),
        add("room2", "tower", (30, 14, 38, 21), 3307, 4094),
        add("room3", "tower", (40, 14, 48, 21), 2756, 3543),
        add("room4", "tower", (50, 22, 57, 30), 2205, 2992),
        add("room5", "tower", (50, 32, 57, 40), 1654, 2441),
        add("room6", "tower", (40, 38, 48, 45), 1102, 1890),
        add("room7", "tower", (30, 38, 38, 45), 551, 1338),
        add("room8", "tower", (22, 28, 28, 36), 0, 787),
        add("base", "small", (37, 27, 41, 31), 3858, 4094),
        Edit::Keep { room: "room2".into() },
    ]
}

#[test]
fn an_atriums_top_is_reached_through_other_rooms_and_never_ramped_down() {
    let lv = empty(1, atrium_trek()).generate().unwrap();
    let atrium = lv.space("room1");
    assert_eq!(atrium.door_levels(), [0, 3858], "its floor and its top storey");
    assert_eq!(lv.space("base").host, "room1", "the base is the atrium's pod, on its top storey");
    // the top storey's door comes from room2, the top of the trek
    let top: Vec<&str> = lv.net.routes().iter().filter(|r| [&r.a, &r.b].iter().any(|d| d.room == "room1" && d.level == 3858)).map(|r| r.key()).collect();
    assert!(!top.is_empty() && lv.net.routes().iter().filter(|r| top.contains(&r.key())).all(|r| r.a.room == "room2" || r.b.room == "room2"), "{top:?}");
    // inside: a walkway on the top storey, ramps only between the two lowest, and no pillars
    let it = lv.interior("room1");
    assert!(decked(it, 3858) > 0.0, "a walkway on the top storey");
    assert!(it.ramps.iter().all(|r| r.hi <= 276), "ramps: {:?}", it.ramps.iter().map(|r| (r.lo, r.hi)).collect::<Vec<_>>());
    assert!(it.deck.iter().all(|(z, d)| !(276 < *z && *z < 3858) || d.is_empty()), "nothing between");
    assert_ne!(it.cover_mode, "pillars");
    // the trek is what hangs off the rest: its tip the base, ten storeys and more up
    let trek = lv.dead_ends.iter().find(|d| d.tip == "base").expect("the trek ends at the base");
    assert!(trek.storeys >= 10 && trek.rooms.len() >= 4, "{trek:?}");
    // the same twice
    assert!(same(&lv, &empty(1, atrium_trek()).generate().unwrap()));
}

fn feature(room: &str, feature: &str, on: bool) -> Edit {
    Edit::Feature { room: s(room), feature: s(feature), on }
}

/// What each room has of the features rolled with the rooms: its cuts, water, pit, and whether a
/// cellar is under it.
fn rolled(lv: &Level) -> Vec<(String, String)> {
    lv.spaces()
        .iter()
        .map(|s| {
            let cellar = lv.spaces().iter().any(|c| c.cellar.as_ref().is_some_and(|c| c.hall == s.key));
            (s.key.clone(), format!("{:?}", (&s.cuts, s.flood, s.sunk, cellar)))
        })
        .collect()
}

#[test]
fn a_feature_is_given_to_one_room_or_kept_from_it_and_no_other_room_changes() {
    let at = |edits: Vec<Edit>| Recipe { seed: 40, options: Options::default(), edits };
    let base = at(vec![]).generate().unwrap();
    // on: the room has it, and every other room is as it was
    let on = at(vec![feature("medium1", "flooded", true), feature("colossal", "platforms", true), feature("vast", "cellar", true)]).generate().unwrap();
    assert!(on.space("medium1").flood.is_some(), "medium1 is flooded");
    assert!(!on.interior("colossal").platforms.is_empty(), "colossal has platforms");
    assert!(on.spaces().iter().any(|c| c.cellar.as_ref().is_some_and(|c| c.hall == "vast")), "vast has a cellar");
    let was: Vec<(String, String)> = rolled(&base).into_iter().filter(|(k, _)| k != "medium1" && k != "vast").collect();
    let now: Vec<(String, String)> = rolled(&on).into_iter().filter(|(k, _)| k != "medium1" && k != "vast" && !k.starts_with("cellar")).collect();
    assert_eq!(was, now, "no other room's roll moved");
    // off: with every room flooded by the level's chance, one kept dry
    let wet = Options { flooded: 1.0, ..Options::default() };
    let all = Recipe { seed: 40, options: wet.clone(), edits: vec![] }.generate().unwrap();
    let room = all.spaces().iter().find(|s| s.flood.is_some()).map(|s| s.key.clone()).expect("a flooded room at a chance of 1");
    let dry = Recipe { seed: 40, options: wet, edits: vec![feature(&room, "flooded", false)] }.generate().unwrap();
    assert!(dry.space(&room).flood.is_none(), "{room} is kept dry");
    assert!(dry.spaces().iter().any(|s| s.key != room && s.flood.is_some()), "the others are still flooded");
    // a room told off what it would not have had anyway: the same level
    let none = at(vec![feature("medium1", "flooded", false), feature("colossal", "angled", false)]).generate().unwrap();
    assert!(same(&none, &base));
    // a feature that is none of them
    let bad = at(vec![feature("vast", "lava", true)]).generate().unwrap_err();
    assert!(bad.starts_with("feature: one of shaped, flooded"), "{bad}");
    // and the file both ways
    let r = at(vec![feature("vast", "cellar", true)]);
    assert_eq!(load(&save(&r)).unwrap(), r);
}

// ---------------------------------------------------------------- join through a doorway

fn join(room: &str, storey: i32, to: &str, door: Option<Doorway>, via: Option<&str>) -> Edit {
    Edit::Join { room: s(room), storey, to: s(to), on: vec![], door, to_door: None, via: via.map(s) }
}

#[test]
fn a_join_takes_the_doorway_asked_for_or_says_none_fits_there() {
    // medium3 moved beside medium1, as above: the router's own choice of door first
    let plain = made(40, vec![mv("medium3", (62, 24)), join("medium3", 0, "medium1", None, None)]);
    let r = plain.net.routes().iter().find(|r| r.a.room == "medium3" && r.b.room == "medium1").expect("the corridor").clone();
    // every doorway along medium3's walls (62..69 x 24..32): each one made has its door there,
    // and those that fit no corridor say so
    let (mut made_at, mut refused) = (Vec::new(), 0);
    for (side, along) in [('N', 62..69), ('S', 62..69), ('E', 24..32), ('W', 24..32)] {
        for at in along {
            let d = Doorway { side, at };
            match recipe(40, vec![mv("medium3", (62, 24)), join("medium3", 0, "medium1", Some(d), None)]).generate() {
                Ok(lv) => {
                    let got = lv.net.routes().iter().find(|r| r.a.room == "medium3" && r.b.room == "medium1").expect("the corridor").a.clone();
                    assert_eq!((got.side, got.a), (side, at), "asked for {d:?}");
                    made_at.push(d);
                }
                Err(why) => {
                    assert!(why.contains("join: no corridor fits from medium3 on 0 to medium1 through the doorways asked for"), "{d:?}: {why}");
                    refused += 1;
                }
            }
        }
    }
    assert!(made_at.len() >= 3 && refused > 0, "{} made, {refused} refused", made_at.len());
    assert!(made_at.contains(&Doorway { side: r.a.side, at: r.a.a }), "the router's own door is one of them");
    // a doorway that is no place on its wall, and a way that is none
    let why = err(40, vec![join("medium3", 0, "medium1", Some(Doorway { side: 'N', at: 90 }), None)]);
    assert!(why.contains("join: medium3 has no doorway N 90 (its walls run 61 to 67)"), "{why}");
    assert!(err(40, vec![join("medium3", 0, "medium1", None, Some("tunnel"))]).contains("join: by a tunnel? (a corridor, a vent or straight)"));
    // by a vent: a vent it is, though the corridor would fit
    let vent = made(40, vec![mv("medium3", (62, 24)), join("medium3", 0, "medium1", None, Some("vent"))]);
    let v = vent.net.routes().iter().find(|r| r.a.room == "medium3" && r.b.room == "medium1").expect("the vent");
    assert_eq!(v.pieces[0].kind, "vent");
    // by a corridor where only... the plain join's own is a corridor already: the same level
    let corridor = made(40, vec![mv("medium3", (62, 24)), join("medium3", 0, "medium1", None, Some("corridor"))]);
    assert!(same(&corridor, &plain));
    // the file: the new fields are written only when given, and read back
    let e = Edit::Join {
        room: s("medium3"),
        storey: 0,
        to: s("medium1"),
        on: vec![],
        door: Some(Doorway { side: 'E', at: 26 }),
        to_door: Some(Doorway { side: 'W', at: 27 }),
        via: Some(s("vent")),
    };
    let text = save(&recipe(40, vec![e.clone()]));
    assert!(text.contains(r#""door": {"#) && text.contains(r#""side": "E""#) && text.contains(r#""via": "vent""#), "{text}");
    assert_eq!(load(&text).unwrap().edits, vec![e.clone()]);
    assert!(!save(&recipe(40, vec![join("medium3", 0, "medium1", None, None)])).contains("door"));
    assert_eq!(e.describe(), "join medium3 on 0 to medium1, from its east wall at 26, to its west wall at 27, by a vent");
}

// ---------------------------------------------------------------- single things by place

fn window(room: &str, storey: i32, at: (i32, i32), kind: &str) -> Edit {
    Edit::Window { room: s(room), storey, at, kind: s(kind) }
}

fn piece(room: &str, at: (i32, i32), what: &str) -> Edit {
    Edit::Piece { room: s(room), at, what: Some(s(what)), size: None, to: None }
}

fn nudge(room: &str, at: (i32, i32), to: (i32, i32)) -> Edit {
    Edit::Piece { room: s(room), at, what: None, size: None, to: Some(to) }
}

fn drop_at(room: &str, at: Option<(i32, i32)>) -> Edit {
    Edit::Drop { room: s(room), at }
}

/// Every room's inside but `room`'s is as it was.
fn the_rest_as_was(a: &Level, b: &Level, room: &str, what: &str) {
    for (key, it) in a.interiors.iter().filter(|(k, _)| k != room) {
        assert_eq!(inside(it), inside(b.interior(key)), "{key} under {what}");
    }
}

#[test]
fn a_window_is_named_by_its_place_made_glass_rail_or_none_and_given_where_none_was() {
    let plain = made(40, vec![]);
    // seed 40's one window: medium1's on 276, wall cells (49, 29) to (49, 31)
    let w = plain.windows.iter().find(|w| w.hall == "medium1").expect("medium1's window");
    assert_eq!((w.z, w.kind, w.wall.clone()), (276, "glass", vec![(49, 29), (49, 30), (49, 31)]));
    // rail, by any cell of its wall; nothing else changes
    for at in [(49, 29), (49, 31)] {
        let lv = made(40, vec![window("medium1", 276, at, "rail")]);
        let got: Vec<_> = lv.windows.iter().map(|w| (w.hall.as_str(), w.z, w.kind, w.wall.clone())).collect();
        assert_eq!(got, vec![("medium1", 276, "rail", w.wall.clone())]);
        assert!(lv.unmet.is_empty());
        the_rest_as_was(&plain, &lv, "", "a rail");
    }
    // none: taken out
    let lv = made(40, vec![window("medium1", 276, (49, 30), "none")]);
    assert!(lv.windows.is_empty() && lv.unmet.is_empty());
    // a level with no windows (`windows` 0) is given one where its place is
    let mut r = recipe(40, vec![window("medium1", 276, (49, 30), "glass")]);
    r.options.windows = 0;
    let lv = r.generate().unwrap();
    assert_eq!(lv.windows.iter().map(|w| (w.hall.as_str(), w.kind)).collect::<Vec<_>>(), vec![("medium1", "glass")]);
    r.edits.clear();
    assert!(r.generate().unwrap().windows.is_empty());
    // where no window fits, or a rail on the room's own floor: left out, and why
    let lv = made(40, vec![window("vast", 0, (36, 25), "glass"), window("medium1", 0, (49, 30), "rail")]);
    assert_eq!(lv.windows.len(), 1);
    let why: Vec<&str> = lv.unmet.iter().map(|(_, w)| w.as_str()).collect();
    assert!(why[0].starts_with("no window fits vast's wall there on 0") && why[1] == "a rail only where the corridor is above medium1's floor", "{why:?}");
    // the file both ways, and what it can't be
    let r = recipe(40, vec![window("medium1", 276, (49, 30), "rail")]);
    let text = save(&r);
    assert!(text.contains(r#""edit": "window""#) && text.contains(r#""as": "rail""#), "{text}");
    assert_eq!(load(&text).unwrap(), r);
    assert_eq!(err(40, vec![window("medium1", 276, (49, 30), "door")]), "window: as one of glass, rail, none, not \"door\"");
    assert_eq!(r.edits[0].describe(), "medium1: window at 49, 30 on 276: rail");
}

#[test]
fn a_pillar_or_crate_is_stood_taken_out_or_moved_by_where_it_is() {
    let plain = made(40, vec![]);
    let was = plain.interior("vast");
    // vast's two lane pillars, two cells by one
    assert_eq!(was.cover, vec![(82, 50, 86, 52, 236, "housing"), (82, 68, 86, 70, 236, "housing")]);
    // taken out, by either of its cells; no other room's inside moves
    for at in [(41, 25), (42, 25)] {
        let lv = made(40, vec![piece("vast", at, "none")]);
        assert_eq!(lv.interior("vast").cover, vec![(82, 68, 86, 70, 236, "housing")]);
        assert!(lv.unmet.is_empty());
        the_rest_as_was(&plain, &lv, "vast", "a pillar out");
        assert_eq!(lv.windows, plain.windows);
    }
    // moved: its size and height kept where it fits (clear of the catwalks' edges over it),
    // else left where it was, and why
    for to in [(40, 32), (43, 34)] {
        let lv = made(40, vec![nudge("vast", (41, 25), to)]);
        assert!(lv.unmet.is_empty(), "{to:?}: {:?}", lv.unmet);
        assert_eq!(lv.interior("vast").cover, vec![(82, 68, 86, 70, 236, "housing"), (2 * to.0, 2 * to.1, 2 * to.0 + 4, 2 * to.1 + 2, 236, "housing")]);
        the_rest_as_was(&plain, &lv, "vast", "a pillar moved");
    }
    let lv = made(40, vec![nudge("vast", (41, 25), (41, 30))]);
    assert_eq!((lv.interior("vast").cover.clone(), lv.unmet[0].1.as_str()), (was.cover.clone(), "it would clip a deck's edge, a ramp or a pod over it"));
    let lv = made(40, vec![nudge("vast", (41, 25), (60, 25))]);
    assert_eq!((lv.interior("vast").cover.clone(), lv.unmet[0].1.as_str()), (was.cover.clone(), "not on vast's floor (off it, water, a hole or a pit)"));
    let lv = made(40, vec![nudge("vast", (39, 30), (41, 30))]);
    assert_eq!(lv.unmet[0].1, "nothing stands in vast at 39, 30");
    // stood: a crate one cell square, a pillar as wide as the room's colonnade's; one stood on
    // another takes its place
    let lv = made(40, vec![piece("vast", (40, 30), "crate"), piece("vast", (40, 32), "pillar")]);
    assert!(lv.unmet.is_empty(), "{:?}", lv.unmet);
    let cover = &lv.interior("vast").cover;
    assert!(cover.contains(&(80, 60, 82, 62, 118, "housing")) && cover.contains(&(80, 64, 82, 66, 236, "housing")), "{cover:?}");
    the_rest_as_was(&plain, &lv, "vast", "a crate and a pillar");
    let lv = made(40, vec![piece("vast", (42, 34), "crate")]);
    assert_eq!(lv.interior("vast").cover, vec![(82, 50, 86, 52, 236, "housing"), (84, 68, 86, 70, 118, "housing")]);
    // a size given; one in a doorway's way, or off the floor, is left out and says why
    let lv = made(40, vec![Edit::Piece { room: s("vast"), at: (40, 32), what: Some(s("pillar")), size: Some(2), to: None }]);
    assert!(lv.interior("vast").cover.contains(&(80, 64, 84, 68, 236, "housing")), "{:?}", lv.unmet);
    let mut reasons = std::collections::BTreeSet::new();
    for x in 37..47 {
        for y in [20, 21, 36, 37] {
            let lv = made(40, vec![piece("vast", (x, y), "pillar")]);
            if let Some((_, why)) = lv.unmet.first() {
                reasons.insert(why.clone());
                assert_eq!(lv.interior("vast").cover, was.cover);
            }
        }
    }
    assert!(!reasons.is_empty(), "every cell along vast's walls took a pillar");
    // the file both ways, and what it can't be
    let r = recipe(40, vec![piece("vast", (40, 30), "crate"), nudge("vast", (41, 25), (41, 30)), piece("vast", (41, 34), "none")]);
    let text = save(&r);
    assert!(text.contains(r#""as": "crate""#) && text.contains(r#""to": ["#) && !text.contains("size"), "{text}");
    assert_eq!(load(&text).unwrap(), r);
    assert_eq!(
        r.edits.iter().map(|e| e.describe()).collect::<Vec<_>>(),
        ["vast: a crate at 40, 30", "vast: the piece at 41, 25 moved to 41, 30", "vast: nothing at 41, 34"]
    );
    let bad = |e: Edit| err(40, vec![e]);
    assert!(bad(Edit::Piece { room: s("vast"), at: (1, 1), what: None, size: None, to: None }).starts_with("piece: one of \"as\""));
    assert_eq!(bad(piece("vast", (40, 30), "barrel")), "piece: as one of pillar, crate, none, not \"barrel\"");
    assert_eq!(bad(Edit::Piece { room: s("vast"), at: (40, 30), what: Some(s("crate")), size: Some(5), to: None }), "piece: a size of 1 to 3 cells, not 5");
}

#[test]
fn a_drop_is_put_in_a_floor_or_taken_out_and_one_that_cannot_land_says_why() {
    let plain = made(40, vec![]);
    assert_eq!(plain.drops, vec![(s("colossal"), s("small7"), (14, 39, 16, 41), Some(0))]);
    // taken out: colossal and small7 lose it, nothing else moves
    let lv = made(40, vec![drop_at("colossal", None)]);
    assert!(lv.drops.is_empty() && lv.unmet.is_empty() && !lv.walks.iter().any(|(k, _)| k.starts_with("drop_")));
    for (key, it) in plain.interiors.iter().filter(|(k, _)| !["colossal", "small7"].contains(&k.as_str())) {
        assert_eq!(inside(it), inside(lv.interior(key)), "{key}");
    }
    // put: loft2 (on 827) over vast (up to 787), every spot a cell in from both rooms' walls
    let (mut made_, mut why) = (Vec::new(), std::collections::BTreeSet::new());
    for x in 41..=43 {
        for y in 28..=32 {
            let lv = made(40, vec![drop_at("loft2", Some((x, y)))]);
            match lv.unmet.first() {
                None => {
                    let d = lv.drops.iter().find(|d| d.0 == "loft2").expect("loft2's drop");
                    assert_eq!((d.1.as_str(), d.2), ("vast", (x, y, x + 2, y + 2)));
                    assert!(d.3.is_some() && lv.walks.iter().any(|(k, _)| k == "drop_loft2_vast"), "({x}, {y})");
                    assert!(lv.drops.iter().any(|d| d.0 == "colossal"), "the generator's own drop stays");
                    made_.push((x, y));
                }
                Some((_, w)) => {
                    assert!(lv.drops.iter().all(|d| d.0 != "loft2"));
                    why.insert(w.clone());
                }
            }
        }
    }
    assert!(!made_.is_empty(), "no drop from loft2 into vast: {why:?}");
    // in place of the room's own; refused where nothing is under it, and the room's own stays
    let lv = made(40, vec![drop_at("colossal", Some((5, 20)))]);
    assert_eq!(lv.drops, plain.drops);
    assert_eq!(lv.unmet[0].1, "no room under colossal there to land in (a cell in from its walls)");
    let lv = made(40, vec![drop_at("colossal", Some((14, 39)))]);
    assert_eq!((lv.drops.clone(), lv.unmet.len()), (plain.drops.clone(), 0));
    assert_eq!(made(40, vec![drop_at("medium1", Some((50, 30)))]).unmet[0].1, "a drop is a cell in from medium1's walls");
    // the file both ways
    let r = recipe(40, vec![drop_at("loft2", Some((42, 30))), drop_at("colossal", None)]);
    let text = save(&r);
    assert!(text.contains(r#""edit": "drop""#) && text.contains(r#""at": ["#), "{text}");
    assert_eq!(load(&text).unwrap(), r);
    assert_eq!(r.edits.iter().map(|e| e.describe()).collect::<Vec<_>>(), ["loft2: a drop at 42, 30", "colossal: no drop"]);
}

#[test]
fn a_room_drawn_in_a_spires_shaft_is_its_pod_with_a_gangway_to_the_spiral() {
    // (new 2026-11-24: the generator puts no pod in a spire; one drawn there stands in the
    // shaft, its way in a gangway straight across the air to the spiral's deck on its storey)
    let spire = add("shaft", "spire", (0, 0, 16, 16), 0, 1063);
    let near = add("room2", "medium", (20, 0, 30, 10), 0, 236);
    let core = |floor: i32| add("core", "small", (6, 6, 10, 10), floor, floor + 236);
    for floor in [276, 551] {
        let r = Recipe { edits: vec![spire.clone(), near.clone(), core(floor)], ..Recipe::empty(40) };
        let lv = r.generate().unwrap_or_else(|why| panic!("on {floor}: {why}"));
        let pod = lv.spaces().iter().find(|s| s.key == "core").expect("the core is in the level");
        assert_eq!(pod.host, "shaft", "on {floor}");
        let plan = &lv.pods.iter().find(|(k, _)| k == "core").expect("a pod's plan").1;
        assert!(plan.door.is_some(), "on {floor}: the core has a door onto the shaft");
        // its gangway is deck of the spire's storey, joined to the spiral's own deck there
        let deck = &lv.interior("shaft").deck[&floor];
        let spiral = lv.spaces().iter().find(|s| s.key == "shaft").unwrap().decks.as_ref().unwrap()[&floor].clone();
        assert!(deck.len() > spiral.len() && spiral.is_subset(deck), "on {floor}: the gangway is laid as deck");
    }
    // one a cell off the spire's corner reaches the deck beside it
    let r = Recipe { edits: vec![spire.clone(), near.clone(), add("core", "small", (1, 1, 5, 5), 276, 512)], ..Recipe::empty(40) };
    assert!(r.generate().unwrap().spaces().iter().any(|s| s.key == "core" && s.host == "shaft"));
}

#[test]
fn a_base_drawn_in_a_spire_stands_in_its_shaft_or_goes_through_its_floor() {
    // (new 2026-11-24: a base drawn in a spire, never through its wall, three cells or more from
    // each: in the shaft its lowest storey is the spire's pod, a gangway to the spiral, a hatch up)
    let spire = add("shaft", "spire", (0, 0, 16, 16), 0, 1063);
    let near = add("room2", "medium", (20, 0, 30, 10), 0, 236);
    let with = |base: Edit| Recipe { edits: vec![spire.clone(), near.clone(), base], ..Recipe::empty(40) };
    // standing in the shaft, two storeys
    let lv = with(add("core", "base", (5, 5, 11, 11), 276, 787)).generate().unwrap();
    let (low, up) = (lv.space("core"), lv.space("core_2"));
    assert_eq!((low.host.as_str(), low.floor, up.floor), ("shaft", 276, 551));
    assert!(low.base.is_some() && up.base.as_ref().is_some_and(|b| b.hall == "shaft"));
    assert_eq!(low.cellar.as_ref().map(|c| c.hall.as_str()), Some("core_2"), "its hatch up");
    let linked = |a: &str, b: &str, kind: &str| lv.plan.links.iter().any(|l| l.a == a && l.b == b && l.kind == kind);
    assert!(linked("core_2", "core", "drop") && linked("core_2", "shaft", "overlook"));
    let plan = &lv.pods.iter().find(|(k, _)| k == "core").expect("the lowest storey is a pod").1;
    assert!(plan.door.is_some());
    // down through the spire's floor, entered from below as any floor base is
    let under = add("under", "medium", (20, 12, 30, 20), -276, -40);
    let r = Recipe { edits: vec![spire.clone(), near.clone(), under, add("deep", "base", (5, 5, 11, 11), -276, 512)], ..Recipe::empty(40) };
    let lv = r.generate().unwrap();
    assert!(lv.spaces().iter().filter(|s| s.base.as_ref().is_some_and(|b| b.column == "deep")).all(|s| s.host.is_empty()));
    // what a spire won't take, and why
    let why = |base: Edit| with(base).generate().unwrap_err();
    assert!(why(add("core", "base", (12, 5, 18, 11), 276, 787)).contains("shaft is a spire: its ramps climb its walls"));
    assert!(why(add("core", "base", (2, 5, 8, 11), 276, 787)).starts_with("core stands 2 cells from shaft's wall"));
    assert!(why(add("core", "base", (5, 5, 11, 11), 0, 512)).starts_with("core stands on shaft's floor"));
}
