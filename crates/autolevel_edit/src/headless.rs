//! What the command line asks for that needs no window, done on a `Doc` the way the window
//! does it: change the room boxes (move, size, pin, add, delete; or only try one), make choices
//! for single rooms (layout, cover, inside, brightness, ceiling), take routes out, pick a room
//! and list what could be joined to it, make one of those, and then the snapshot and the lists
//! of the level that leaves. `main.rs` only writes the files and prints the text, so a test can
//! run all of it.
//!
//! A room change that breaks a rule is refused, and that is not the end of the run: the
//! snapshot is still made, of the boxes as they would lie with the refused one in red and the
//! reason by it, and `Done::refused` tells `main.rs` to fail once it is written. Nothing after
//! the refused change is done. A recipe whose rooms make no level is shown the same way, by its
//! boxes alone (`Done::failed`). A room's choice that is refused, or that no level can be made
//! with, is shown on the level as it was: the room's box in red, the reason by it.

use crate::cli::{Args, Choose};
use crate::doc::Doc;
use crate::planview::{Boxes, View};
use crate::raster::Image;
use crate::{chosen, lots, routes, shot};
use autolevel::boxes::{self, Change, Trial};
use autolevel::recipe::Recipe;

/// What a command line came to.
pub struct Done {
    /// The recipe, with the edits the room flags, the choices, `--remove` and `--join` made.
    pub recipe: Recipe,
    /// What to print.
    pub text: String,
    /// The snapshot, if one was asked for.
    pub shot: Option<Image>,
    /// Why a room change or a room's choice was refused. The text and the snapshot are there
    /// all the same, showing it; the run is then a failure.
    pub refused: Option<String>,
    /// Why the recipe makes no level: its rooms (the snapshot shows the room boxes instead), or
    /// with a room's choice (the snapshot shows the level as it was). A failure too.
    pub failed: Option<String>,
}

/// A room's choice the line could not make.
struct Stopped {
    room: String,
    /// Why, as it is printed.
    why: String,
    /// What the plan says by the room's box: the choice and why not.
    words: String,
    /// It was made, and then no level could be (not refused beforehand).
    failed: bool,
}

/// Make the line's choices for single rooms on the document, in the order given, each one step
/// with a line of text, and the level again after each: what a choice may be is looked up in
/// the level, and a choice that makes none is known for the one it was. Returns the choice that
/// stopped it, if one did: refused before the level was made, or no level with it (the choice
/// is then taken back, so the level on show is the recipe's). Err only if the seed's own again
/// makes no level.
fn choices(doc: &mut Doc, args: &Args, text: &mut String) -> Result<Option<Stopped>, String> {
    for ask in &args.choose {
        let before = doc.recipe.clone();
        match ask {
            Choose::Set(e) => {
                let room = e.rooms()[0].to_string();
                let known = doc.made().is_some_and(|made| made.level.spaces().iter().any(|s| s.key == room));
                let refused = if known { doc.set_choice(e.clone()).err() } else { Some(format!("{}: no room {room}", e.name())) };
                if let Some(why) = refused {
                    text.push_str(&format!("refused: {why}\n"));
                    // (on the plan: what was asked, then why not, without the edit's name twice)
                    let reason = why.strip_prefix(&format!("{}: ", e.name())).unwrap_or(&why);
                    return Ok(Some(Stopped { words: format!("{}: {reason}", e.describe()), room, why, failed: false }));
                }
                text.push_str(&format!("choice: {}\n", chosen::said(e, &before, &doc.recipe)));
                if doc.recipe == before {
                    continue;
                }
                if let Err(why) = doc.regenerate() {
                    text.push_str(&format!("no level: {why}\n"));
                    doc.undo();
                    return Ok(Some(Stopped { words: format!("{} makes no level: {why}", e.describe()), room, why, failed: true }));
                }
            }
            Choose::Own { room, what } => {
                let cleared: Vec<&str> = chosen::kinds(what).into_iter().filter(|kind| doc.clear_choice(room, kind)).collect();
                text.push_str(&format!("choice: {}\n", chosen::owned(room, what, &cleared)));
                if doc.recipe != before {
                    doc.regenerate().map_err(|why| format!("--own {room}@{}: no level: {why}", chosen::word(what)))?;
                }
            }
        }
    }
    Ok(None)
}

/// Make the line's room changes on the document, in the order given, each one step, with a line
/// of text for each. With `--try` the last is only tried. Returns the trial there is to show:
/// the change tried, or the one refused, after which nothing more is done. Its room is then the
/// one selected, so the plan draws it with its grips (or in red).
fn rooms(doc: &mut Doc, args: &Args, text: &mut String) -> Result<Option<Trial>, String> {
    for (n, change) in args.boxes.iter().enumerate() {
        let before = doc.lots()?;
        // (a room to make gets its key now: the next one free as the lots stand)
        let change = match change {
            Change::Add { key, kind, rect, floor, top } if key.is_empty() => {
                Change::Add { key: boxes::new_key(&before), kind: kind.clone(), rect: *rect, floor: *floor, top: *top }
            }
            other => other.clone(),
        };
        let t = doc.try_change(&change)?;
        let tried = args.trial && n + 1 == args.boxes.len();
        if let Some(why) = &t.refused {
            text.push_str(&format!("refused: {why}\n"));
        } else {
            let said = lots::said(&change, &doc.recipe, &before.lots, &t);
            text.push_str(&if tried { format!("tried: {said} (not made: --try)\n") } else { format!("room: {said}\n") });
            if let Some(warn) = &t.warn {
                text.push_str(&format!("note: {warn}\n"));
            }
        }
        if tried || t.refused.is_some() {
            doc.select(Some(change.room()));
            return Ok(Some(t));
        }
        doc.change(&change)?;
    }
    Ok(None)
}

/// Do what `args` asks for short of opening the window or writing a file. Err, saying why, if
/// the recipe makes no level, or a route can't be taken out or a join made. A room change that
/// is refused is no Err: see `Done::refused`.
pub fn run(args: &Args) -> Result<Done, String> {
    let mut doc = Doc::new(args.recipe.clone(), args.from.is_some());
    let mut text = String::new();
    let level = args.shot.is_some() || args.list || args.routes || args.changes() || !args.choices.is_empty();
    if !(level || args.lots) {
        return Ok(Done { recipe: doc.recipe, text, shot: None, refused: None, failed: None });
    }
    // the room boxes first, as lots come first in the generator; the level is made after them
    let trial = rooms(&mut doc, args, &mut text)?;
    let mut refused = trial.as_ref().and_then(|t| t.refused.clone());
    if let (Some(room), None) = (&args.select, &trial) {
        if doc.lots()?.lots.iter().all(|a| a.key != *room) {
            return Err(format!("--select: no room {room} (--lots names them)"));
        }
        doc.select(Some(room));
    }
    let mut boxes = args.draws_rooms().then(|| doc.boxes(trial.as_ref()));
    if args.lots {
        let placed = doc.lots().map_err(|why| format!("--lots: {why}"))?;
        text.push_str(&lots::text(trial.as_ref().map_or(&placed.lots, |t| &t.lots), placed.site, trial.is_some()));
    }
    if !level {
        return Ok(Done { recipe: doc.recipe, text, shot: None, refused, failed: None });
    }
    if let Err(why) = doc.regenerate() {
        // (with the room boxes to show, a recipe that makes no level still has its picture)
        let Some(boxes) = boxes else { return Err(why) };
        text.push_str(&format!("no level: {why}\n"));
        let view = View { storey: args.storey, boxes: Some(boxes), ..View::default() };
        let shot = args.shot.is_some().then(|| shot::render_bare(doc.recipe.seed, &why, &view, args.width));
        return Ok(Done { recipe: doc.recipe, text, shot, refused, failed: Some(why) });
    }
    if let (Some(shown), None) = (&boxes, &trial) {
        // (the generator leaves out a room nothing reaches, and a pod with no way in: say so)
        let made = doc.made().expect("the level was made");
        let out: Vec<&str> = shown.lots.iter().filter(|a| made.level.spaces().iter().all(|s| s.key != a.key)).map(|a| a.key.as_str()).collect();
        if !out.is_empty() {
            text.push_str(&format!("left out of the level by the generator: {}\n", out.join(", ")));
        }
    }
    // a room's choices next: they are looked up in the level, and it is made again after each
    let mut failed = None;
    if let Some(stop) = if refused.is_none() { choices(&mut doc, args, &mut text)? } else { None } {
        // (shown by the room's box, in red, on the level as it was)
        doc.select(Some(&stop.room));
        boxes = Some(Boxes { refused: Some(stop.words), ..doc.boxes(None) });
        if stop.failed {
            failed = Some(stop.why);
        } else {
            refused = Some(stop.why);
        }
    }
    let stopped = refused.is_some() || failed.is_some();
    let mut gone = None;
    // (after a refusal nothing more is changed: the level shown is the one refused of)
    for key in args.remove.iter().filter(|_| !stopped) {
        let was = doc.made().and_then(|made| made.level.net.routes().iter().find(|r| r.pieces.iter().any(|pc| pc.key == *key)).cloned());
        let done = doc.remove_route(key).map_err(|why| format!("--remove {key}: {why}"))?;
        text.push_str(&format!("removed: {}\n", done.text()));
        doc.regenerate().map_err(|why| format!("--remove {key}: no level without it: {why}"))?;
        gone = was;
    }
    if let Some((room, storey)) = args.pick.as_ref().filter(|_| !stopped) {
        let made = doc.made().expect("the level was made").clone();
        let s = made.level.spaces().iter().find(|s| s.key == *room).ok_or_else(|| format!("--pick: no room {room} (the plan names them)"))?;
        doc.pick(room, storey.unwrap_or(s.floor));
        let found = doc.find_joins().map_err(|why| format!("--pick: {why}"))?;
        text.push_str(&routes::joins_text(&made.level, &found));
        if let Some(n) = args.join {
            let c = doc.choose(n).map_err(|why| format!("--join: {why}"))?;
            text.push_str(&format!("joined: {n}  {}\n", c.text()));
            doc.regenerate().map_err(|why| format!("--join {n}: {why}"))?;
        }
    }
    let made = doc.made().expect("the level was made").clone();
    for room in &args.choices {
        let found = doc.choices(room).map_err(|why| format!("--choices: {why} (--lots names them)"))?;
        text.push_str(&chosen::text(&found, made.level.space(room)));
    }
    let view = View { storey: args.storey, gone, boxes, ..doc.look(args.all, None) };
    let shot = args.shot.is_some().then(|| shot::render(&made, &view, args.width));
    if args.shot.is_some() || args.list {
        text.push_str(&format!("{}\n{}on the plan:\n{}", shot::title(&made), made.dead.text(), made.dead.numbered()));
    }
    if args.routes {
        text.push_str(&routes::text(&made.level));
    }
    Ok(Done { recipe: doc.recipe, text, shot, refused, failed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;
    use autolevel::recipe::Edit;

    fn args(line: &str) -> Args {
        // (bases and grown pods left out: the levels these tests pin were made before they were on)
        let words: Vec<String> = format!("{line} --no-bases --no-big-pods").split_whitespace().map(str::to_string).collect();
        cli::parse(&words, &|path| Err(format!("{path}: no files here"))).unwrap()
    }

    #[test]
    fn an_empty_start_makes_only_the_rooms_added() {
        let none = run(&args("--empty --shot plan.png --width 600")).unwrap();
        assert_eq!(none.failed.as_deref(), Some("no room yet: an empty start (draw one)"));
        assert!(none.shot.is_some(), "the bare plan is still drawn");
        let two = run(&args("--empty --add medium@30,20,40,28@0@512 --add vast@44,16,58,34@0@787 --shot plan.png --width 600")).unwrap();
        assert!(two.failed.is_none() && two.refused.is_none(), "{:?} {:?}", two.failed, two.refused);
        assert!(two.text.contains("autolevel seed 40: 2 rooms"), "{}", two.text);
        assert!(autolevel::recipe::save(&two.recipe).contains("\"start\": \"empty\""));
    }

    #[test]
    fn a_plain_line_makes_no_level_unless_it_needs_one() {
        let done = run(&args("--seed 7 --walkway 3 --save-recipe out.json")).unwrap();
        assert!(done.text.is_empty() && done.shot.is_none() && done.recipe.options.walkway == 3);
        assert_eq!(run(&args("--seed 7 --walkway 3 --list")).err().unwrap(), "walkway: 1 or 2 cells");
    }

    #[test]
    fn a_pick_lists_what_could_be_joined_and_a_join_makes_one() {
        let seen = run(&args("--seed 40 --pick tower@551 --shot plan.png --width 900")).unwrap();
        assert!(seen.recipe.edits.is_empty());
        assert!(seen.text.starts_with("tower on 551 could be joined to "), "{}", seen.text);
        assert!(seen.text.contains("\n   1  to ") && seen.text.contains("\nautolevel seed 40: 29 rooms, 44 links"));
        // the third lies where the dead-end pass puts a corridor of its own, and says so
        let third = "to colossal on 551 by a corridor, 6 cells (c39)";
        assert!(seen.text.contains(&format!("\n   3  {third}  [lies where the dead-end pass now adds c42]\n")), "{}", seen.text);
        // choosing a number adds its join, and the level then has that corridor (and the pass,
        // with one dead end fewer to put right, adds one fewer of its own)
        let done = run(&args("--seed 40 --pick tower@551 --join 3 --shot plan.png --width 900 --routes")).unwrap();
        assert!(done.text.contains(&format!("joined: 3  {third}\n")), "{}", done.text);
        assert_eq!(done.recipe.edits, [Edit::Join { room: "tower".into(), storey: 551, to: "colossal".into(), on: vec![551] }]);
        assert!(
            done.text.contains("\nroutes (40): ") && done.text.contains("\n  c39   tower on 551 - colossal on 551    a corridor, 6 cells\n"),
            "{}",
            done.text
        );
        let after = done.text.split_once("joined: 3").unwrap().1;
        assert!(after.contains("done about them (1):") && !after.contains("c42"), "{}", done.text);
        // the two pictures differ: the corridors that could be, then the one that is
        assert_ne!(seen.shot.unwrap().png().unwrap(), done.shot.unwrap().png().unwrap());
        // a number past the end, a pod, a storey with no door
        let err = |line: &str| run(&args(line)).err().unwrap();
        assert!(err("--seed 40 --pick tower@551 --join 99").starts_with("--join: no join 99: there are "));
        assert_eq!(err("--seed 40 --pick small8"), "--pick: small8 is a pod: it is joined inside vast");
        assert_eq!(err("--seed 40 --pick attic"), "--pick: no room attic (the plan names them)");
        assert!(err("--seed 40 --pick tower@1654").starts_with("--pick: tower takes doors on "));
    }

    /// A room of the level a recipe makes: its box.
    fn room(recipe: &Recipe, key: &str) -> (i32, i32, i32, i32) {
        crate::deadlist::make(recipe).unwrap().level.space(key).rect()
    }

    fn png(done: &Done) -> Vec<u8> {
        done.shot.as_ref().expect("a snapshot").png().unwrap()
    }

    #[test]
    fn a_room_is_moved_and_the_level_then_has_it_there() {
        let plain = run(&args("--seed 40 --rooms --shot plan.png --width 900")).unwrap();
        assert!(plain.recipe.edits.is_empty() && plain.refused.is_none() && plain.failed.is_none());
        assert!(plain.text.starts_with("autolevel seed 40: 29 rooms, 44 links"), "{}", plain.text);
        // the boxes are another picture than the plain plan
        assert_ne!(png(&plain), png(&run(&args("--seed 40 --shot plan.png --width 900")).unwrap()));
        let done = run(&args("--seed 40 --move medium1@60,29 --shot plan.png --width 900")).unwrap();
        assert_eq!(done.recipe.edits, [Edit::Move { room: "medium1".into(), to: (60, 29) }]);
        assert!(done.text.starts_with("room: move medium1 to 60, 29\nautolevel seed 40: 29 rooms, "), "{}", done.text);
        assert!(done.refused.is_none() && done.failed.is_none() && png(&done) != png(&plain));
        assert_eq!(room(&done.recipe, "medium1"), (60, 29, 67, 34));
        // a move to where it is changes nothing, and says so
        let same = run(&args("--seed 40 --move medium1@50,29 --lots")).unwrap();
        assert!(
            same.recipe.edits.is_empty() && same.text.starts_with("room: move medium1 to 50, 29: nothing changes, it is so already\nlots (29), on a site"),
            "{}",
            same.text
        );
        // a room the generator then leaves out of the level is named
        let far = run(&args("--seed 40 --move small7@74,54 --list")).unwrap();
        assert!(
            far.text.starts_with("room: move small7 to 74, 54\nleft out of the level by the generator: small7\nautolevel seed 40: 28 rooms"),
            "{}",
            far.text
        );
    }

    #[test]
    fn a_refused_change_still_has_its_picture_and_says_why() {
        let plain = run(&args("--seed 40 --rooms --shot plan.png --width 900")).unwrap();
        // medium1 onto small3: refused, and nothing after it is done
        let done =
            run(&args("--seed 40 --move tiny5@1,1 --move medium1@48,41 --delete small7 --remove c23 --pick tower@551 --shot plan.png --width 900 --lots"))
                .unwrap();
        let why = done.refused.clone().expect("it lands on small3");
        assert!(why.starts_with("medium1 and small3 overlap: rooms keep a cell between them") && why.contains("medium1 is at 48, 41 to 55, 46"), "{why}");
        assert!(done.text.starts_with(&format!("room: move tiny5 to 1, 1\nrefused: {why}\nlots (29) as they would lie, on a site")), "{}", done.text);
        assert!(!done.text.contains("delete small7") && !done.text.contains("removed:") && !done.text.contains("could be joined"), "{}", done.text);
        // the recipe has what was made before it, and the level shown is that recipe's
        assert_eq!(done.recipe.edits, [Edit::Move { room: "tiny5".into(), to: (1, 1) }]);
        assert!(done.text.contains("\n  medium1   medium    48,41 to 55,46 (7 x 5)") && done.text.contains("\nautolevel seed 40: "), "{}", done.text);
        // the picture is there, and is neither the plain boxes nor the same change tried elsewhere
        assert!(done.failed.is_none() && png(&done) != png(&plain));
        let alone = run(&args("--seed 40 --move medium1@48,41 --shot plan.png --width 900")).unwrap();
        let tried = run(&args("--seed 40 --move medium1@60,29 --try --shot plan.png --width 900")).unwrap();
        assert!(alone.refused.as_ref() == Some(&why) && alone.recipe.edits.is_empty() && png(&alone) != png(&plain) && png(&alone) != png(&tried));
        // a change that can't be read at all is refused the same way
        let none = run(&args("--seed 40 --move attic@1,1 --shot plan.png --width 900")).unwrap();
        assert_eq!(none.refused.as_deref(), Some("move: no room attic"));
        assert!(none.text.starts_with("refused: move: no room attic\nautolevel seed 40: 29 rooms") && png(&none) != png(&plain), "{}", none.text);
        let kind = run(&args("--seed 40 --add attic@70,40,74,44@0@236 --try")).unwrap();
        assert!(kind.refused.as_deref().is_some_and(|w| w.starts_with("add: no kind attic at this scale: one of colossal, ")), "{:?}", kind.refused);
        // a room to select that is not there is an error, as a room to pick is
        assert_eq!(run(&args("--seed 40 --select attic --shot plan.png")).err().unwrap(), "--select: no room attic (--lots names them)");
    }

    #[test]
    fn a_change_is_tried_and_the_recipe_left_as_it_was() {
        let plain = run(&args("--seed 40 --rooms --shot plan.png --width 900")).unwrap();
        let done = run(&args("--seed 40 --delete small7 --move medium1@60,29 --try --lots --shot plan.png --width 900")).unwrap();
        // the changes before the last are made; the last is tried, and shown as it would lie
        assert_eq!(done.recipe.edits, [Edit::Delete { room: "small7".into() }]);
        assert!(
            done.text.starts_with("room: delete small7\ntried: move medium1 to 60, 29 (not made: --try)\nlots (28) as they would lie, on a site"),
            "{}",
            done.text
        );
        assert!(done.text.contains("\n  medium1   medium    60,29 to 67,34 (7 x 5)") && done.text.contains("\nautolevel seed 40: 28 rooms"), "{}", done.text);
        assert!(done.refused.is_none() && done.failed.is_none() && png(&done) != png(&plain));
        assert_eq!(room(&done.recipe, "medium1"), (50, 29, 57, 34));
        // tried alone: the seed's own recipe, and the picture is not the one of the move made
        let tried = run(&args("--seed 40 --move medium1@60,29 --try --shot plan.png --width 900")).unwrap();
        let made = run(&args("--seed 40 --move medium1@60,29 --shot plan.png --width 900")).unwrap();
        assert!(tried.recipe == Recipe::before_bases(40) && png(&tried) != png(&made) && png(&tried) != png(&plain));
    }

    #[test]
    fn rooms_are_added_sized_pinned_and_deleted_in_the_order_given() {
        let done = run(&args(
            "--seed 40 --add small@70,40,74,44@276@512 --add tiny@70,50,72,52@0@236 --rect medium1@50,29,59,36 --delete small7 --pin small1@551 --lots --list",
        ))
        .unwrap();
        // a line for each, in the edit list's words; a made room's key is the next free one
        let said = "room: add room1 (small) at 70, 40 to 74, 44, from 276 to 512\n\
                    room: add room2 (tiny) at 70, 50 to 72, 52, from 0 to 236\n\
                    room: resize medium1 to 9 x 7\n\
                    room: delete small7\n\
                    room: pin small1's floor at 551\n\
                    lots (30), on a site of 80 x 60 cells: ";
        assert!(done.text.starts_with(said), "{}", done.text);
        assert_eq!(
            done.recipe.edits,
            [
                Edit::Add { key: "room1".into(), kind: "small".into(), rect: (70, 40, 74, 44), floor: 276, top: 512 },
                Edit::Add { key: "room2".into(), kind: "tiny".into(), rect: (70, 50, 72, 52), floor: 0, top: 236 },
                Edit::Resize { room: "medium1".into(), size: (9, 7) },
                Edit::Delete { room: "small7".into() },
                Edit::Pin { room: "small1".into(), floor: 551 },
            ]
        );
        assert!(
            done.text.contains("\n  room1     small     70,40 to 74,44 (4 x 4)    276 to 512\n")
                && done.text.contains("\n  small1    small     51,23 to 55,27 (4 x 4)    551 to 787\n"),
            "{}",
            done.text
        );
        assert!(!done.text.contains("\n  small7 ") && done.text.contains("\nautolevel seed 40: 30 rooms, "), "{}", done.text);
        // the level has them so
        let made = crate::deadlist::make(&done.recipe).unwrap();
        let at = |key: &str| (made.level.space(key).rect(), made.level.space(key).floor);
        assert_eq!(
            (at("room1"), at("room2"), at("medium1"), at("small1")),
            (((70, 40, 74, 44), 276), ((70, 50, 72, 52), 0), ((50, 29, 59, 36), 0), ((51, 23, 55, 27), 551))
        );
        assert!(made.level.spaces().iter().all(|s| s.key != "small7"));
        // a hall deleted: its pods go with it, and it is said; a hall's west side pulled: its pods are put back
        let hall = run(&args("--seed 40 --delete vast --rect colossal@4,15,23,42 --try")).unwrap();
        let said = "room: delete vast; its pods go with it: small8, tiny2\ntried: move colossal to 4, 15; move small2 to 10, 24; move tiny1 to 17, 37; resize colossal to 19 x 27 (not made: --try)\n";
        assert_eq!(hall.text, said);
        // a note where the recipe names a corridor by a key the change may shift
        let mut with = args("--seed 40 --move medium1@60,29 --try");
        with.recipe.edits.push(Edit::Remove { route: "c23".into() });
        let noted = run(&with).unwrap();
        assert!(
            noted.text.contains("\nnote: the recipe takes out c23 by its key, and corridors' keys are dealt out again when rooms change"),
            "{}",
            noted.text
        );
    }

    #[test]
    fn the_lots_are_listed_without_a_level_and_drawn_alone_where_there_is_none() {
        // the list needs no level: a recipe that makes none still has its lots
        let listed = run(&args("--seed 7 --walkway 3 --lots")).unwrap();
        assert!(listed.text.starts_with("lots (29), on a site of 80 x 60 cells: ") && listed.text.lines().count() == 30, "{}", listed.text);
        assert!(listed.shot.is_none() && listed.failed.is_none());
        // seed 25 makes no level with no rescue. With the boxes to draw it has a picture of them,
        // and fails after
        let why = "seed 25: colossal: no set of ramps joins its storeys";
        let done = run(&args("--seed 25 --no-rescue --select tower --shot plan.png --width 900")).unwrap();
        assert_eq!((done.failed.as_deref(), done.refused.as_deref()), (Some(why), None));
        assert_eq!(done.text, format!("no level: {why}\n"));
        // (the bare plan is framed on the site, and draws its edge)
        let placed = Doc::new(Recipe::before_bases(25), true).lots().unwrap();
        let view = View {
            boxes: Some(crate::planview::Boxes { lots: placed.lots, selected: Some("tower".into()), refused: None, site: Some(placed.site) }),
            ..View::default()
        };
        assert_eq!(png(&done), shot::render_bare(25, why, &view, 900).png().unwrap());
        // one storey of it, with a change made first
        let one = run(&args("--seed 25 --no-rescue --delete tiny1 --storey 276 --shot plan.png --width 900")).unwrap();
        assert!(one.failed.as_deref() == Some(why) && one.text == format!("room: delete tiny1\nno level: {why}\n") && png(&one) != png(&done), "{}", one.text);
        assert_eq!(one.recipe.edits, [Edit::Delete { room: "tiny1".into() }]);
        // without the boxes it is the error it was
        assert_eq!(run(&args("--seed 25 --no-rescue --shot plan.png")).err().unwrap(), why);
        // and with the hall that can't be furnished deleted, the seed makes a level (as it does
        // with the rescue: `Options::rescue`)
        let cured = run(&args("--seed 25 --no-rescue --delete colossal --list")).unwrap();
        assert!(
            cured.failed.is_none()
                && cured.text.starts_with("room: delete colossal; its pods go with it: small3, small6, small7, tiny4\nautolevel seed 25: 24 rooms, "),
            "{}",
            cured.text
        );
        // (options that make no sense refuse every room change, in the options' own words)
        assert_eq!(run(&args("--seed 7 --walkway 3 --delete tower --try")).unwrap().refused.as_deref(), Some("walkway: 1 or 2 cells"));
    }

    #[test]
    fn a_rooms_choices_are_made_in_the_order_given_and_the_level_has_them() {
        let plain = run(&args("--seed 40 --shot plan.png --width 900")).unwrap();
        let done = run(&args("--seed 40 --layout colossal@ring --cover vast@none --inside medium1@3 --brightness vast@90 --ceiling vast@grille --choices vast --shot plan.png --width 900"))
            .unwrap();
        // a line for each in the edit list's words, then the room's choices as the level now has them
        let said =
            "choice: colossal: layout ring\nchoice: vast: cover none\nchoice: medium1: inside 3\nchoice: vast: brightness 90\nchoice: vast: ceiling grille\n\
                    choices for vast (a vast room, 10 x 18 cells, 0 to 787): what it has, and whose choice that is\n";
        assert!(done.text.starts_with(said), "{}", done.text);
        let row = |name: &str| done.text.lines().find(|l| l.trim_start().starts_with(name)).unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(row("cover"), "cover none, 0 pieces chosen in the recipe");
        assert_eq!((row("brightness"), row("ceiling")), ("brightness 90 chosen in the recipe".to_string(), "ceiling grille chosen in the recipe".to_string()));
        assert!(row("layout").ends_with(" the seed's own") && row("inside") == "inside 0 the seed's own", "{}", done.text);
        assert!(done.text.contains("\n  could take layout mezzanine\nautolevel seed 40: 29 rooms, "), "{}", done.text);
        assert_eq!(
            done.recipe.edits,
            [
                Edit::Layout { room: "colossal".into(), layout: "ring".into() },
                Edit::Cover { room: "vast".into(), cover: "none".into() },
                Edit::Reroll { room: "medium1".into(), n: 3 },
                Edit::Brightness { room: "vast".into(), value: 90 },
                Edit::Ceiling { room: "vast".into(), ceiling: "grille".into() },
            ]
        );
        assert!(done.refused.is_none() && done.failed.is_none() && png(&done) != png(&plain));
        // the level made of the recipe has them
        let made = crate::deadlist::make(&done.recipe).unwrap();
        assert_eq!((made.level.interior("colossal").layout, made.level.interior("vast").cover.len()), (Some("ring"), 0));
        assert!(made.level.moods.contains(&("vast".to_string(), 90)) && made.level.ceilings.contains(&("vast".to_string(), "grille")));
        // the list alone changes nothing, and is of the seed's own level
        let listed = run(&args("--seed 40 --choices colossal --choices small8")).unwrap();
        assert!(
            listed.recipe.edits.is_empty() && listed.text.starts_with("choices for colossal (a colossal room, 20 x 27 cells, 0 to 1063): "),
            "{}",
            listed.text
        );
        assert!(
            listed.text.contains("\nchoices for small8 (a small room, a pod of vast, 4 x 3 cells, 551 to 787): ") && listed.text.lines().count() == 20,
            "{}",
            listed.text
        );
        assert_eq!(run(&args("--seed 40 --choices attic")).err().unwrap(), "--choices: no room attic (--lots names them)");
    }

    #[test]
    fn a_layout_a_cover_or_an_inside_chosen_is_another_picture_and_a_brightness_is_not() {
        use crate::planview::picture;
        let plain = picture(&crate::fixture::seed40().level, &View::default());
        // (drawn with no word of what was chosen: the room's inside itself differs)
        let drawn = |line: &str| {
            // (as the fixture is made: `Options::python`)
            let recipe = run(&args(&format!("{line} --flooded 0 --shaped 0 --sunken 0"))).unwrap().recipe;
            assert_eq!(recipe.edits.len(), 1, "{line}");
            picture(&crate::deadlist::make(&recipe).unwrap().level, &View::default())
        };
        let (ring, galleries, mezzanine) =
            (drawn("--seed 40 --layout colossal@ring"), drawn("--seed 40 --layout colossal@galleries"), drawn("--seed 40 --layout colossal@mezzanine"));
        assert!(ring.prims != plain.prims && galleries.prims != plain.prims && mezzanine.prims != plain.prims);
        assert!(ring.prims != galleries.prims && galleries.prims != mezzanine.prims && ring.prims != mezzanine.prims);
        assert!(drawn("--seed 40 --cover colossal@pillars").prims != plain.prims);
        assert!(drawn("--seed 40 --inside colossal@1").prims != plain.prims);
        // brightness and ceiling are for the built level: the plan is the same but for the words
        assert!(drawn("--seed 40 --brightness colossal@90").prims == plain.prims);
        assert!(drawn("--seed 40 --ceiling colossal@grille").prims == plain.prims);
    }

    #[test]
    fn a_choice_that_cant_be_is_refused_and_one_no_inside_takes_makes_no_level() {
        // the long hall is too narrow for a ring: refused before the level is made, and nothing after it is done
        let before = run(&args("--seed 40 --layout colossal@ring --shot plan.png --width 900")).unwrap();
        let done = run(&args("--seed 40 --layout colossal@ring --layout long@ring --cover vast@none --remove c23 --shot plan.png --width 900")).unwrap();
        let why = "layout: long is too small for a ring: it is 18 x 5 cells, and that needs 8 by 8";
        assert_eq!((done.refused.as_deref(), done.failed.as_deref()), (Some(why), None));
        assert!(done.text.starts_with(&format!("choice: colossal: layout ring\nrefused: {why}\nautolevel seed 40: 29 rooms, ")), "{}", done.text);
        assert!(!done.text.contains("cover none") && !done.text.contains("removed:"), "{}", done.text);
        assert_eq!(done.recipe.edits, [Edit::Layout { room: "colossal".into(), layout: "ring".into() }]);
        // the picture is the level as it was, with the room's box in red: not the one without the refusal
        assert!(png(&done) != png(&before));
        // what else is refused beforehand, in the generator's words
        let refused = |line: &str| run(&args(line)).unwrap().refused.unwrap();
        assert_eq!(refused("--seed 40 --cover small8@none"), "cover: small8 is a pod of vast: it is not furnished by itself");
        assert_eq!(refused("--seed 40 --inside small8@1"), "reroll: small8 is a pod of vast: it is not furnished by itself");
        assert_eq!(refused("--seed 40 --layout tower@ring"), "layout: tower is a tower: its decks are its own");
        assert_eq!(refused("--seed 40 --layout vast@maze"), "layout: one of ('ring', 'galleries', 'mezzanine', 'none')");
        assert_eq!(refused("--seed 40 --cover vast@hedges"), "cover: one of ('mixed', 'pillars', 'crates', 'none')");
        assert_eq!(refused("--seed 40 --brightness vast@300"), "brightness: 0 to 255, not 300");
        assert_eq!(refused("--seed 40 --ceiling vast@glass"), "ceiling: one of ('plain', 'ceiling_tile', 'ceiling_dark', 'grille')");
        assert_eq!(refused("--seed 40 --cover attic@none"), "cover: no room attic");
        // a ring the basin could take by its size, but no inside can be furnished with (with no
        // rescue): no level. The choice is taken back, the picture is the level as it was with the
        // reason by the room
        let done = run(&args("--seed 40 --no-rescue --layout basin@ring --cover vast@none --shot plan.png --width 900")).unwrap();
        let why = "seed 40: basin cannot be furnished with a ring (basin: no set of ramps joins its storeys)";
        assert_eq!((done.failed.as_deref(), done.refused.as_deref()), (Some(why), None));
        assert!(done.text.starts_with(&format!("choice: basin: layout ring\nno level: {why}\nautolevel seed 40: 29 rooms, 44 links")), "{}", done.text);
        assert!(done.recipe.edits.is_empty() && !done.text.contains("cover none"));
        let plain = run(&args("--seed 40 --no-rescue --shot plan.png --width 900")).unwrap();
        assert!(png(&done) != png(&plain) && png(&done) != png(&run(&args("--seed 40 --no-rescue --rooms --shot plan.png --width 900")).unwrap()));
        // after a room change that is refused, no choice is made either
        let done = run(&args("--seed 40 --move medium1@48,41 --cover vast@none")).unwrap();
        assert!(done.refused.is_some() && !done.text.contains("choice:") && done.recipe.edits.is_empty(), "{}", done.text);
    }

    #[test]
    fn a_choice_is_changed_and_the_seeds_own_had_again() {
        let done = run(&args("--seed 40 --cover vast@none --cover vast@crates --inside vast@2 --inside vast@0 --brightness vast@90 --own vast@cover --own vast@all --own tower@layout --choices vast")).unwrap();
        let said = "choice: vast: cover none\n\
                    choice: vast: cover crates (in place of: vast: cover none)\n\
                    choice: vast: inside 2\n\
                    choice: vast: inside 0, the seed's own (in place of: vast: inside 2)\n\
                    choice: vast: brightness 90\n\
                    choice: vast: the seed's own cover again\n\
                    choice: vast: the seed's own brightness again\n\
                    choice: tower: nothing was chosen for its layout\n\
                    choices for vast (";
        assert!(done.text.starts_with(said), "{}", done.text);
        // the recipe is the seed's own again, and so is the room
        assert!(done.recipe.edits.is_empty() && done.refused.is_none() && done.failed.is_none());
        assert_eq!(
            done.text.split_once("choices for vast").unwrap().1,
            run(&args("--seed 40 --choices vast")).unwrap().text.split_once("choices for vast").unwrap().1
        );
        assert!(!done.text.contains("chosen in the recipe"));
        // a choice made again as it is, is said to change nothing; one for all of a room with none, the same
        let mut again = args("--seed 40 --cover vast@none --own colossal@all");
        again.recipe.edits.push(Edit::Cover { room: "vast".into(), cover: "none".into() });
        assert_eq!(run(&again).unwrap().text, "choice: vast: cover none: nothing changes, it is so already\nchoice: colossal: nothing was chosen for it\n");
    }

    #[test]
    fn a_route_is_taken_out_or_the_reason_given() {
        // c17 is the only way to small7; c39a is the dead-end pass's own
        assert_eq!(run(&args("--seed 40 --remove c17")).err().unwrap(), "--remove c17: c17 is the only way to small7: without it the level is in two");
        assert_eq!(run(&args("--seed 40 --remove c99")).err().unwrap(), "--remove c99: no route c99 in this level");
        let done = run(&args("--seed 40 --remove c39b --list")).unwrap();
        assert_eq!(done.recipe.edits, [Edit::Keep { room: "spire1".into() }]);
        assert!(done.text.starts_with("removed: c39a was the dead-end pass's: spire1's dead end is now kept as it is"), "{}", done.text);
        assert!(done.text.contains("autolevel seed 40: 29 rooms, 43 links"), "{}", done.text);
    }
}
