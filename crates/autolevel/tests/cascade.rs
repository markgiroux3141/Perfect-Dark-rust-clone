//! The cascade shown (`autolevel::cascade`) on seed 40: what an edit changed, and nothing else.

use autolevel::cascade::{diff, How};
use autolevel::level::{Level, Options};
use autolevel::recipe::{Edit, Recipe};

fn s(text: &str) -> String {
    text.to_string()
}

fn made(edits: Vec<Edit>) -> Level {
    let r = Recipe { seed: 40, options: Options::python(), edits };
    r.generate().unwrap_or_else(|why| panic!("{:?}: {why}", r.edits))
}

#[test]
fn a_level_against_itself_is_no_change() {
    let a = made(vec![]);
    let b = made(vec![]);
    let c = diff(&a, &b);
    assert!(c.is_empty(), "{:?}", c.lines());
    assert_eq!(c.summary(3), "nothing else changed");
}

#[test]
fn a_pillar_moved_is_only_that_pillar_in_vast() {
    let a = made(vec![]);
    let b = made(vec![Edit::Piece { room: s("vast"), at: (41, 25), what: None, size: None, to: Some((40, 32)) }]);
    let c = diff(&a, &b);
    assert_eq!(c.rooms().into_iter().collect::<Vec<_>>(), vec!["vast"], "{:?}", c.lines());
    assert!(c.changes.iter().all(|ch| ch.what == "cover"), "{:?}", c.lines());
    let moved: Vec<_> = c.changes.iter().filter(|ch| ch.how == How::Moved).collect();
    assert_eq!(moved.len(), 1, "{:?}", c.lines());
    assert_eq!(moved[0].words, "vast: a pillar moved from 41, 25 to 40, 32");
    let ((x0, y0, _), (x1, y1, _)) = moved[0].arrow.expect("an arrow");
    assert_eq!((x0, y0, x1, y1), (42.0, 25.5, 41.0, 32.5));
    assert_eq!(c.lines(), vec!["vast: a pillar moved from 41, 25 to 40, 32"]);
    // and the other way: back again
    let back = diff(&b, &a);
    assert_eq!(back.lines(), vec!["vast: a pillar moved from 40, 32 to 41, 25"]);
}

#[test]
fn a_room_moved_re_routes_its_corridors() {
    let a = made(vec![]);
    let b = made(vec![Edit::Move { room: s("medium3"), to: (62, 24) }]);
    let c = diff(&a, &b);
    let lines = c.lines();
    // what moved with medium3 inside it is no change
    assert!(!c.changes.iter().any(|ch| ch.owner == "medium3" && ch.what != "room"), "{lines:?}");
    // the room itself, with an arrow
    let room = c.changes.iter().find(|ch| ch.what == "room" && ch.owner == "medium3").unwrap_or_else(|| panic!("{lines:?}"));
    assert_eq!(room.how, How::Moved);
    assert!(room.arrow.is_some() && room.words.starts_with("medium3 moved"), "{}", room.words);
    // each corridor that changed joins medium3, by its two rooms (never its own key)
    let routes: Vec<_> = c.changes.iter().filter(|ch| ch.what == "corridor" || ch.what == "vent").collect();
    assert!(!routes.is_empty(), "{lines:?}");
    assert!(routes.iter().any(|ch| ch.rooms().contains(&"medium3")), "{lines:?}");
    for ch in &routes {
        assert!(!ch.owner.starts_with('c'), "{}", ch.owner);
        assert!(ch.how != How::Changed || ch.words.contains("re-routed") || ch.words.contains("now"), "{}", ch.words);
    }
}

#[test]
fn a_window_made_rail_is_one_change() {
    let a = made(vec![]);
    let b = made(vec![Edit::Window { room: s("medium1"), storey: 276, at: (49, 29), kind: s("rail") }]);
    let c = diff(&a, &b);
    assert_eq!(c.lines(), vec!["medium1's window in its west wall on 276 is rail now (was glass)"]);
    let c = diff(&a, &made(vec![Edit::Window { room: s("medium1"), storey: 276, at: (49, 29), kind: s("none") }]));
    assert_eq!(c.changes[0].how, How::Gone);
}
