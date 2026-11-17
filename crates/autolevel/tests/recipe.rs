//! A recipe's edits on real seeds (40, 7 and 1): each does what it says, each error case gives
//! its message, the same recipe gives the same level, and a few dozen edited levels are either
//! whole or refused for a reason a person can read. (The edits that were there first, `join`,
//! `remove`, `keep`, `cut`, `brightness` and `ceiling`, are tested in `src/recipe.rs`.)

use autolevel::furnish::Interior;
use autolevel::level::{Level, Options};
use autolevel::place::{self, Placed, Scale, Siting};
use autolevel::recipe::{load, save, Edit, Recipe};
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
    Edit::Reroll { room: s(room), n }
}

fn recipe(seed: u64, edits: Vec<Edit>) -> Recipe {
    Recipe { seed, options: Options::default(), edits }
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
    let join = Edit::Join { room: s("medium3"), storey: 0, to: s("medium1"), on: vec![] };
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
    // and whatever the lots, the level is the generator's to refuse
    assert_eq!(err(40, vec![mv("small4", (40, 22)), pin("small4", 276)]), "seed 40: vast: no set of ramps joins its storeys");
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
        "add: no kind attic at this scale: one of colossal, spire, vast, tower, long, basin, medium, loft, small, tiny"
    );
    let standard = Recipe {
        options: Options { siting: Siting { scale: Scale::Standard, ..Siting::default() }, ..Options::default() },
        ..recipe(40, vec![add("annexe", "loft", (2, 2, 8, 8), 551, 787)])
    };
    assert_eq!(standard.placed().unwrap_err(), "add: no kind loft at this scale: one of vast, tower, long, basin, medium, small, tiny");
    assert_eq!(why(add("annexe", "medium", (62, 30, 63, 36), 0, 512)), "add: a room is at least 2 x 2 cells, not 1 x 6");
    assert_eq!(
        why(add("annexe", "medium", (62, 30, 69, 36), 0, 500)),
        "add: annexe's top would be at 500: ceilings are at -39, 236, 512, 787, 1063, 1338, 1614, 1890"
    );
    assert_eq!(
        why(add("annexe", "medium", (62, 30, 69, 36), 100, 512)),
        "add: annexe's floor would be at 100: floors are at -276, 0, 276, 551, 827, 1102, 1378, 1654"
    );
    assert_eq!(why(add("annexe", "medium", (62, 30, 69, 36), 276, 236)), "add: annexe would be under a storey tall (276 to 236)");
    assert!(why(add("annexe", "medium", (60, 20, 67, 26), 0, 512)).starts_with("long and annexe overlap: "));
    assert!(why(add("annexe", "spire", (62, 30, 69, 40), 0, 1614)).starts_with("annexe is a spire: its ramps climb walls at least 8 cells long"));
    assert_eq!(
        why(add("annexe", "medium", (76, 30, 83, 36), 0, 512)),
        "annexe is off the site (it is at 76, 30 to 83, 36, from 0 to 512; the site is 80 x 60 cells)"
    );
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
    let join = Edit::Join { room: s("tiny4"), storey: 276, to: s("small1"), on: vec![] };
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
    assert_eq!(why(vec![pin("loft2", 300)]), "pin: loft2's floor would be at 300: floors are at -276, 0, 276, 551, 827, 1102, 1378, 1654");
    assert_eq!(why(vec![pin("loft2", 1654)]), "pin: loft2 would stand from 1654 to 2166, above the highest ceiling (1890)");
    assert_eq!(
        why(vec![pin("tiny2", 0)]),
        "tiny2 lands in vast, so it is its pod, and a pod's floor is one of its hall's first two upper storeys: 276, 551, not 0"
    );
    assert!(why(vec![pin("small8", 827)]).starts_with("loft2 and small8 overlap: ")); // over the hall, where a loft already is
    assert!(why(vec![pin("vast", 276)]).starts_with("vast and loft1 overlap: ")); // 1063 to 1102 is a slab of 39
                                                                                  // (only lots that are no lawful pods can be carried off the top: here one over the basin)
    assert_eq!(why(vec![mv("small8", (33, 42)), pin("basin", 1378)]), "pin: with basin on 1378, its pod small8 would have no storey to be on");
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
    assert_eq!(err(40, vec![layout("basin", "ring")]), "seed 40: basin cannot be furnished with a ring (basin: no set of ramps joins its storeys)");
    // (so is none, where the hall's upstairs doors can only be joined by one: seed 12's)
    assert_eq!(err(12, vec![layout("colossal", "none")]), "seed 12: colossal cannot be furnished with no layout (colossal: no set of ramps joins its storeys)");
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
        let own = autolevel::level::generate(seed, &Options::default()).unwrap();
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
