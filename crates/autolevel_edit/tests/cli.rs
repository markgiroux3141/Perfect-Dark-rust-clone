//! The command line, run as a user runs it: `--shot` writes the same PNG every time and prints
//! the dead-end list, and a recipe saved from a command line makes the same picture. A room is
//! picked and one of the corridors listed for it made (`--pick`, `--join`), a corridor is taken
//! out (`--remove`), and the recipe that leaves makes the same level by itself. A room's box is
//! moved, tried and refused (`--move`, `--try`, `--lots`): a refusal still writes its picture,
//! and then fails. A room's layout and cover are chosen, listed and taken back (`--layout`,
//! `--cover`, `--choices`, `--own`), and a choice that can't be made fails the same way.

use std::process::Command;

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_autolevel_edit")).args(args).output().expect("autolevel_edit runs");
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

/// What a command line that must fail says.
fn refused(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_autolevel_edit")).args(args).output().expect("autolevel_edit runs");
    assert!(!out.status.success(), "{args:?} should fail");
    String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n").trim().to_string()
}

/// What a command line that must fail prints before it does, and what it then says.
fn fails(args: &[&str]) -> (String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_autolevel_edit")).args(args).output().expect("autolevel_edit runs");
    assert_eq!(out.status.code(), Some(1), "{args:?} should fail");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    (text(&out.stdout), text(&out.stderr).trim().to_string())
}

#[test]
fn a_rooms_box_is_listed_moved_tried_and_refused() {
    let dir = std::env::temp_dir().join(format!("autolevel_edit_rooms_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let (plain, rooms, moved, tried, no, recipe, kept) =
        (at("plain.png"), at("rooms.png"), at("moved.png"), at("tried.png"), at("no.png"), at("moved.json"), at("kept.json"));
    let bytes = |path: &str| std::fs::read(path).unwrap();
    // the lots, by the keys the room flags take; the boxes drawn are another picture than the plan
    let lots = run(&["--seed", "40", "--lots"]);
    assert!(
        lots.starts_with("lots (29), on a site of 80 x 60 cells: key, kind, ") && lots.contains("\n  medium1   medium    50,29 to 57,34 (7 x 5)    0 to 512\n"),
        "{lots}"
    );
    run(&["--seed", "40", "--shot", &plain, "--width", "900"]);
    run(&["--seed", "40", "--rooms", "--shot", &rooms, "--width", "900"]);
    assert!(bytes(&rooms) != bytes(&plain));
    // a move made: said, saved in the recipe, and the recipe by itself has the room there
    let made = run(&["--seed", "40", "--move", "medium1@60,29", "--save-recipe", &recipe, "--shot", &moved, "--width", "900"]);
    assert!(
        made.starts_with(&format!("recipe: {recipe}\nshot: {moved} (900 x ")) && made.contains("\nroom: move medium1 to 60, 29\nautolevel seed 40: 29 rooms"),
        "{made}"
    );
    assert!(std::fs::read_to_string(&recipe)
        .unwrap()
        .contains("\"edit\": \"move\",\n      \"room\": \"medium1\",\n      \"to\": [\n        60,\n        29\n      ]"));
    assert!(run(&["--recipe", &recipe, "--lots"]).contains("\n  medium1   medium    60,29 to 67,34 (7 x 5)    0 to 512\n"));
    assert!(bytes(&moved) != bytes(&rooms));
    // the same move tried: the picture of what it would come to, the recipe left as it was
    let seen = run(&["--seed", "40", "--move", "medium1@60,29", "--try", "--save-recipe", &kept, "--shot", &tried, "--width", "900"]);
    assert!(seen.contains("\ntried: move medium1 to 60, 29 (not made: --try)\n"), "{seen}");
    assert!(std::fs::read_to_string(&kept).unwrap().contains("\"edits\": []"));
    assert!(bytes(&tried) != bytes(&moved) && bytes(&tried) != bytes(&rooms));
    // onto another room: refused. The picture is written and the reason printed, then it fails,
    // and no recipe is written
    let none = at("none.json");
    let (out, err) = fails(&["--seed", "40", "--move", "medium1@48,41", "--delete", "small7", "--save-recipe", &none, "--shot", &no, "--width", "900"]);
    let why = "refused: medium1 and small3 overlap: rooms keep a cell between them, unless one is over the other with a 40-unit slab between";
    assert!(out.starts_with(&format!("shot: {no} (900 x ")) && out.contains(&format!("\n{why} (medium1 is at 48, 41 to 55, 46, ")), "{out}");
    assert!(!out.contains("delete small7") && out.contains("\nautolevel seed 40: 29 rooms"), "{out}");
    assert!(err.starts_with(&format!("autolevel_edit: {why}")), "{err}");
    assert!(bytes(&no).starts_with(b"\x89PNG") && bytes(&no) != bytes(&rooms) && bytes(&no) != bytes(&tried));
    assert!(!std::path::Path::new(&none).exists());
    // a seed that makes no level (with no rescue): its boxes alone are drawn, and it fails saying why
    let bare = at("bare.png");
    let (out, err) = fails(&["--seed", "25", "--no-rescue", "--rooms", "--shot", &bare, "--width", "900"]);
    assert!(out.starts_with(&format!("shot: {bare} (900 x ")) && out.ends_with(")\nno level: seed 25: colossal: no set of ramps joins its storeys\n"), "{out}");
    assert_eq!(err, "autolevel_edit: no level: seed 25: colossal: no set of ramps joins its storeys");
    assert!(bytes(&bare).starts_with(b"\x89PNG"));
    // what can't be read is an error, and the usage names all of it
    assert_eq!(
        refused(&["--seed", "40", "--move", "medium1"]),
        "autolevel_edit: --move: a room, then @ and the cell its low corner goes to, as tower@12,30, not medium1"
    );
    assert_eq!(refused(&["--seed", "40", "--try"]), "autolevel_edit: --try: of what? Name a change with --move, --rect, --pin, --delete or --add");
    let help = run(&["--help"]);
    let flags = [
        "--rooms",
        "--select ROOM",
        "--lots",
        "--move ROOM@X,Y",
        "--rect ROOM@X0,Y0,X1,Y1",
        "--pin ROOM@FLOOR",
        "--delete ROOM",
        "--add KIND@X0,Y0,X1,Y1@FLOOR@TOP",
        "--try",
    ];
    assert!(flags.iter().all(|flag| help.contains(flag)), "{help}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rooms_choices_are_made_listed_refused_and_taken_back() {
    let dir = std::env::temp_dir().join(format!("autolevel_edit_choices_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let (plain, chosen, no, recipe, own, none) = (at("plain.png"), at("chosen.png"), at("no.png"), at("chosen.json"), at("own.json"), at("none.json"));
    let bytes = |path: &str| std::fs::read(path).unwrap();
    run(&["--seed", "40", "--shot", &plain, "--width", "900"]);
    // two choices made: said, listed for the level they leave, saved, and another picture
    let made = run(&[
        "--seed",
        "40",
        "--layout",
        "colossal@ring",
        "--cover",
        "vast@none",
        "--choices",
        "colossal",
        "--save-recipe",
        &recipe,
        "--shot",
        &chosen,
        "--width",
        "900",
    ]);
    let said = "\nchoice: colossal: layout ring\nchoice: vast: cover none\nchoices for colossal (a colossal room, 20 x 27 cells, 0 to 1063): what it has, and whose choice that is\n  layout      ring";
    assert!(made.contains(said) && made.contains("  chosen in the recipe\n") && made.contains("\n  could take layout galleries\n"), "{made}");
    let text = std::fs::read_to_string(&recipe).unwrap();
    assert!(
        text.contains("\"edit\": \"layout\",\n      \"room\": \"colossal\",\n      \"layout\": \"ring\"") && text.contains("\"edit\": \"cover\""),
        "{text}"
    );
    assert!(bytes(&chosen) != bytes(&plain));
    // the recipe by itself has them
    let again = run(&["--recipe", &recipe, "--choices", "vast"]);
    assert!(again.contains("\n  cover       none, 0 pieces") && again.lines().filter(|l| l.ends_with("chosen in the recipe")).count() == 1, "{again}");
    // the long hall can't take a ring: the picture is written, the reason printed, and it fails
    let (out, err) = fails(&["--recipe", &recipe, "--layout", "long@ring", "--save-recipe", &none, "--shot", &no, "--width", "900"]);
    let why = "refused: layout: long is too small for a ring: it is 18 x 5 cells, and that needs 8 by 8";
    assert!(out.starts_with(&format!("shot: {no} (900 x ")) && out.contains(&format!("\n{why}\nautolevel seed 40: ")), "{out}");
    assert_eq!(err, format!("autolevel_edit: {why}"));
    assert!(bytes(&no).starts_with(b"\x89PNG") && bytes(&no) != bytes(&chosen) && !std::path::Path::new(&none).exists());
    // a ring the basin can't be furnished with (with no rescue): no level, said, and it fails
    let (out, err) = fails(&["--seed", "40", "--no-rescue", "--layout", "basin@ring", "--list"]);
    let why = "no level: seed 40: basin cannot be furnished with a ring (basin: no set of ramps joins its storeys)";
    assert!(out.starts_with(&format!("choice: basin: layout ring\n{why}\nautolevel seed 40: 29 rooms")), "{out}");
    assert_eq!(err, format!("autolevel_edit: {why}"));
    // the seed's own again: the recipe is as it was
    let back = run(&["--recipe", &recipe, "--own", "colossal@all", "--own", "vast@cover", "--save-recipe", &own]);
    assert!(back.contains("choice: colossal: the seed's own layout again\nchoice: vast: the seed's own cover again\n"), "{back}");
    assert!(std::fs::read_to_string(&own).unwrap().contains("\"edits\": []"));
    // what can't be read is an error, and the usage names all of it
    assert_eq!(
        refused(&["--seed", "40", "--layout", "ring"]),
        "autolevel_edit: --layout: a room, then @ and its layout (ring galleries mezzanine none), as vast@ring, not ring"
    );
    let help = run(&["--help"]);
    let flags = [
        "--layout ROOM@ring|galleries|mezzanine|none",
        "--cover ROOM@mixed|pillars|crates|none",
        "--inside ROOM@N",
        "--brightness ROOM@0..255",
        "--ceiling ROOM@plain|ceiling_tile|ceiling_dark|grille",
        "--own ROOM@layout|cover|inside|brightness|ceiling|all",
        "--choices ROOM",
    ];
    assert!(flags.iter().all(|flag| help.contains(flag)), "{help}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_join_is_picked_made_saved_and_taken_out_again() {
    let dir = std::env::temp_dir().join(format!("autolevel_edit_join_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let (plain, picked, joined, recipe, without) = (at("plain.png"), at("picked.png"), at("joined.png"), at("join.json"), at("without.json"));
    // a pick lists what could be joined, numbered, and draws it: another picture than the plain plan
    run(&["--seed", "40", "--shot", &plain, "--width", "900"]);
    let seen = run(&["--seed", "40", "--pick", "tower@551", "--shot", &picked, "--width", "900"]);
    let list = "tower on 551 could be joined to 9 rooms (21 ways):\n   1  to loft3 on 551 by a door through the wall (c39)\n";
    assert!(seen.contains(list) && seen.contains("\n   4  to medium5 on 276 by a corridor, 9 cells with 1 ramp (c39)\n"), "{seen}");
    assert!(std::fs::read(&picked).unwrap() != std::fs::read(&plain).unwrap());
    // the list alone needs no picture, and a room alone is picked on its own floor
    assert!(run(&["--seed", "40", "--pick", "tower@551"]).starts_with(list));
    assert!(run(&["--seed", "40", "--pick", "tower"]).starts_with("tower on 0 could be joined to "));
    // number 4 made: the join is in the recipe written, with the storey that names it
    let made = run(&["--seed", "40", "--pick", "tower@551", "--join", "4", "--save-recipe", &recipe, "--shot", &joined, "--width", "900", "--routes"]);
    assert!(made.contains("\njoined: 4  to medium5 on 276 by a corridor, 9 cells with 1 ramp (c39)\n"), "{made}");
    assert!(made.contains("\n  c39   tower on 551 - medium5 on 276"), "{made}");
    let edit = "\"edits\": [\n    {\n      \"edit\": \"join\",\n      \"room\": \"tower\",\n      \"storey\": 551,\n      \"to\": \"medium5\",\n      \"on\": [\n        276\n      ]\n    }\n  ]";
    assert!(std::fs::read_to_string(&recipe).unwrap().contains(edit));
    assert!(std::fs::read(&joined).unwrap() != std::fs::read(&plain).unwrap());
    // the recipe by itself makes the same level: the same lists, route for route
    let level = |text: &str| text.split_once("autolevel seed 40:").expect("a title").1.to_string();
    let again = run(&["--recipe", &recipe, "--list", "--routes"]);
    assert_eq!(level(&again), level(&made));
    // the corridor is taken out again: the recipe says so, and the level is without it
    let gone = run(&["--recipe", &recipe, "--remove", "c39", "--save-recipe", &without, "--routes"]);
    assert!(gone.starts_with(&format!("recipe: {without}\nremoved: c39 is taken out\nroutes (")) && !gone.contains("\n  c39 "), "{gone}");
    let text = std::fs::read_to_string(&without).unwrap();
    assert!(text.contains(edit.trim_end_matches("\n  ]")) && text.contains("\"edit\": \"remove\",\n      \"route\": \"c39\""), "{text}");
    assert_eq!(run(&["--recipe", &without, "--routes"]), gone.split_once("removed: c39 is taken out\n").unwrap().1);
    // what can't be done says why, and writes nothing
    let lone = at("lone.json");
    assert_eq!(
        refused(&["--seed", "40", "--remove", "c17", "--save-recipe", &lone]),
        "autolevel_edit: --remove c17: c17 is the only way to small7: without it the level is in two"
    );
    assert!(!std::path::Path::new(&lone).exists());
    assert_eq!(refused(&["--seed", "40", "--pick", "small8"]), "autolevel_edit: --pick: small8 is a pod: it is joined inside vast");
    assert_eq!(refused(&["--seed", "40", "--join", "2"]), "autolevel_edit: --join: of what? Name the room and storey with --pick");
    // and the usage names all of it
    let help = run(&["--help"]);
    assert!(["--pick ROOM@STOREY", "--join N", "--remove KEY", "--routes", "--all"].iter().all(|flag| help.contains(flag)), "{help}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_shot_is_written_twice_the_same_and_the_list_is_printed() {
    let dir = std::env::temp_dir().join(format!("autolevel_edit_cli_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let (png1, png2, png3, recipe) = (at("a.png"), at("b.png"), at("c.png"), at("seven.json"));
    let text = run(&["--seed", "7", "--cover", "pillars", "--shot", &png1, "--width", "900"]);
    run(&["--seed", "7", "--cover", "pillars", "--shot", &png2, "--width", "900"]);
    let (a, b) = (std::fs::read(&png1).unwrap(), std::fs::read(&png2).unwrap());
    assert!(a == b && a.starts_with(b"\x89PNG"));
    assert!(text.contains("seed 7: ") && text.contains(" dead ends as it falls, ") && text.contains("\nleft: ") && text.contains("\non the plan:\n"), "{text}");
    // the recipe written from that line makes the same picture, and holds only what was chosen
    run(&["--seed", "7", "--cover", "pillars", "--save-recipe", &recipe]);
    assert_eq!(
        std::fs::read_to_string(&recipe).unwrap(),
        "{\n  \"format\": 1,\n  \"seed\": 7,\n  \"options\": {\n    \"cover\": \"pillars\"\n  },\n  \"edits\": []\n}\n"
    );
    run(&["--recipe", &recipe, "--shot", &png3, "--width", "900"]);
    assert!(std::fs::read(&png3).unwrap() == a);
    // one storey is another picture; the list alone needs no file
    run(&["--recipe", &recipe, "--shot", &png3, "--width", "900", "--storey", "0"]);
    assert!(std::fs::read(&png3).unwrap() != a);
    let list = run(&["--recipe", &recipe, "--list"]);
    assert_eq!(list, text.split_once('\n').unwrap().1);
    // what makes no sense is refused, with a reason and a failing exit
    let bad = Command::new(env!("CARGO_BIN_EXE_autolevel_edit")).args(["--seed", "7", "--walkway", "3", "--list"]).output().unwrap();
    assert!(!bad.status.success() && String::from_utf8_lossy(&bad.stderr).contains("autolevel_edit: walkway: 1 or 2 cells"));
    std::fs::remove_dir_all(&dir).unwrap();
}
