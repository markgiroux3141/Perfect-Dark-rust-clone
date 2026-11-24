//! A room locked, and the rest re-rolled (`autolevel::lock`, the editing roadmap's item 8), on
//! seed 40: locking changes nothing, and the locked room is never in the cascade of a re-roll.

use autolevel::cascade::diff;
use autolevel::level::Level;
use autolevel::lock::{guard, is_locked, lock, locked, refusal, rest, rest_of, unlock};
use autolevel::recipe::{load, save, Edit, Recipe};

fn made(r: &Recipe) -> Level {
    r.generate().unwrap_or_else(|why| panic!("{:?}: {why}", r.edits))
}

fn inside(lv: &Level, room: &str) -> String {
    format!("{:?}", lv.interior(room))
}

fn s(text: &str) -> String {
    text.to_string()
}

#[test]
fn locking_a_room_changes_nothing() {
    let r = Recipe::python(40);
    let lv = made(&r);
    let rooms: Vec<String> = lv.spaces().iter().filter(|s| s.host.is_empty()).map(|s| s.key.clone()).collect();
    assert!(rooms.len() > 20);
    for room in &rooms {
        let l = lock(&r, &lv, room).unwrap();
        assert!(is_locked(&l, room) && locked(&l) == [room.clone()]);
        let c = diff(&lv, &made(&l));
        assert!(c.is_empty(), "{room} locked: {:?}", c.lines());
    }
    // colossal: the pass's corridor from tower is the recipe's own now, through its doorways
    let l = lock(&r, &lv, "colossal").unwrap();
    let joins: Vec<String> = l.edits.iter().map(|e| e.describe()).collect();
    assert_eq!(joins, ["join tower on 551 to colossal (on 551), from its west wall at 15, to its east wall at 15, by a corridor", "lock colossal"]);
    // the defaults too (with them the pass also joins loft2 to vast)
    let r = Recipe::new(40);
    let lv = made(&r);
    for room in ["vast", "colossal", "loft2"] {
        let l = lock(&r, &lv, room).unwrap();
        assert!(l.edits.iter().any(|e| matches!(e, Edit::Join { .. })), "{room}: {:?}", l.edits);
        let c = diff(&lv, &made(&l));
        assert!(c.is_empty(), "{room} locked: {:?}", c.lines());
    }
}

#[test]
fn a_locked_room_is_untouched_when_the_rest_is_re_rolled() {
    let r = Recipe::python(40);
    let lv = made(&r);
    for room in ["colossal", "vast", "medium3", "tower"] {
        let l = lock(&r, &lv, room).unwrap();
        for n in 1..=5 {
            let re = rest(&l, n);
            assert_eq!(rest_of(&re), n);
            let after = made(&re);
            let c = diff(&lv, &after);
            assert!(!c.is_empty(), "{room}, rest {n}: nothing re-rolled");
            let mine: Vec<&str> = c.of(room).map(|ch| ch.words.as_str()).collect();
            assert!(mine.is_empty(), "{room} locked, rest {n}: {mine:?}");
        }
    }
    // without the lock, colossal loses its door from tower on the first stream (the pass's join
    // was for tower's dead end, which tower's own re-roll no longer has)
    let c = diff(&lv, &made(&rest(&r, 1)));
    assert!(c.of("colossal").any(|ch| ch.words == "corridor colossal-tower is gone"), "{:?}", c.lines());
}

#[test]
fn the_rest_is_each_room_re_rolled_alone() {
    // a room the rest re-rolls is the room `reroll` n gives (the pass aside: medium3 is on none of its corridors)
    let r = Recipe::python(40);
    let by_rest = made(&rest(&r, 2));
    let alone = made(&Recipe { edits: vec![Edit::Reroll { room: s("medium3"), n: 2, part: None }], ..r.clone() });
    assert_eq!(inside(&by_rest, "medium3"), inside(&alone, "medium3"));
    // a room with a whole re-roll of its own keeps it; rest 0 is none
    let own = Recipe { edits: vec![Edit::Reroll { room: s("medium3"), n: 4, part: None }, Edit::Rest { n: 2 }], ..r.clone() };
    let mine = made(&Recipe { edits: vec![Edit::Reroll { room: s("medium3"), n: 4, part: None }], ..r.clone() });
    assert_eq!(inside(&made(&own), "medium3"), inside(&mine, "medium3"));
    assert_eq!(rest(&own, 0).edits, [Edit::Reroll { room: s("medium3"), n: 4, part: None }]);
    // one rest edit, in its place
    assert_eq!(rest(&own, 3).edits[1], Edit::Rest { n: 3 });
    assert_eq!(rest(&own, 3).edits.len(), 2);
}

#[test]
fn a_locked_room_refuses_what_would_change_it() {
    let r = Recipe::python(40);
    let lv = made(&r);
    let l = lock(&r, &lv, "vast").unwrap();
    let mv = Edit::Move { room: s("vast"), to: (30, 20) };
    assert_eq!(refusal(&l, &mv).unwrap(), "move: vast is locked: unlock it to change it");
    assert!(refusal(&l, &Edit::Move { room: s("medium3"), to: (62, 24) }).is_none());
    let join = Edit::Join { room: s("tower"), storey: 0, to: s("vast"), on: vec![], door: None, to_door: None, via: None };
    assert_eq!(refusal(&l, &join).unwrap(), "join: vast is locked: unlock it to change it");
    // and generate says the same for one made after the lock
    let mut bad = l.clone();
    bad.edits.push(Edit::Cover { room: s("vast"), cover: s("crates") });
    assert_eq!(bad.generate().unwrap_err(), "cover: vast is locked: unlock it to change it");
    // before the lock it is the room's own
    let mut ok = l.clone();
    ok.edits.insert(0, Edit::Cover { room: s("vast"), cover: s("crates") });
    made(&ok);
    // locked twice is once; unlocked, the lock is gone and the edit can be made
    assert_eq!(lock(&l, &lv, "vast").unwrap(), l);
    let u = unlock(&l, "vast");
    assert!(!is_locked(&u, "vast") && refusal(&u, &mv).is_none());
    // the guard the editors go through: any change to what the recipe says of it, wherever in the
    // list; unlocking and locking pass; deleting it (its lock goes with it) does not
    let mut early = l.clone();
    early.edits.insert(0, Edit::Cover { room: s("vast"), cover: s("crates") });
    assert_eq!(guard(&l, &early).unwrap_err(), "vast is locked: unlock it to change it");
    assert!(guard(&l, &u).is_ok() && guard(&r, &l).is_ok());
    let gone = Recipe { edits: vec![Edit::Delete { room: s("vast") }], ..r.clone() };
    assert_eq!(guard(&l, &gone).unwrap_err(), "vast is locked: unlock it to change it");
    assert!(guard(&l, &rest(&l, 2)).is_ok());
    // a pod is held with its hall; a room that is not there
    let pod = lv.spaces().iter().find(|s| s.host == "vast").expect("vast has pods").key.clone();
    assert_eq!(lock(&r, &lv, &pod).unwrap_err(), format!("lock: {pod} is a pod of vast: lock its hall"));
    assert_eq!(lock(&r, &lv, "attic").unwrap_err(), "lock: no room attic");
    let by_hand = Recipe { edits: vec![Edit::Lock { room: pod.clone() }], ..r.clone() };
    assert_eq!(by_hand.generate().unwrap_err(), format!("lock: {pod} is a pod of vast: it is not furnished by itself"));
}

#[test]
fn locks_and_the_rest_are_in_the_file() {
    let r = Recipe::python(40);
    let lv = made(&r);
    let l = rest(&lock(&r, &lv, "colossal").unwrap(), 3);
    let text = save(&l);
    assert!(text.contains("\"edit\": \"lock\"") && text.contains("\"edit\": \"rest\""), "{text}");
    assert_eq!(load(&text).unwrap(), l);
    assert_eq!(l.edits.last().unwrap().describe(), "the rest: inside 3");
    assert!(load("{\"format\": 1, \"seed\": 40, \"edits\": [{\"edit\": \"lock\"}]}").unwrap_err().contains("missing field `room`"));
}
