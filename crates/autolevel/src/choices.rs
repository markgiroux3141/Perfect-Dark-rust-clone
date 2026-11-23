//! One room's choices: what a person may choose for a single room in place of what the seed
//! gives it, which is what the editor's room panel shows and sets. New here (the Python has
//! none); the plan is in `docs/AUTOLEVEL.md`, "Track B", milestone 4.
//!
//! The choices are the recipe's own edits: `layout` (a hall's first upper storey), `cover`
//! (what stands on its floor), `reroll` (its inside from another stream of random numbers),
//! `brightness` (what its floor is baked to) and `ceiling`; and `feature`, one of
//! `level::FEATURES` on or off (an L or U shape, water, a sunken floor, a cellar, platforms,
//! drops through them, angled catwalks), each feature a kind of its own. `of` reads, for one room of a made
//! level, what it has, what the recipe chose for it, and which layouts it could take (with the
//! reason for each it could not, in plain words). `set` is the recipe with a choice made, and
//! `clear` the recipe with the seed's own again.
//!
//! A room has one edit of a kind at most: the last of a kind is the one the generator goes by
//! (`recipe::Recipe::furnished`, `finish`), so `set` puts the new one in the earlier one's place
//! and the list does not grow as a choice is changed and changed again.
//!
//! What can be known without making the level is refused here (`refusal`: a pod's cover, a
//! layout the room is too small for, a brightness out of range). What can't (a layout no
//! attempt can be furnished with) comes back from making it, as any failed recipe does.

use crate::build::{takes, LAYOUTS};
use crate::level::{Level, COVER, FEATURES, PARTS};
use crate::py;
use crate::recipe::{Edit, Recipe, CEILINGS};

/// The kinds of choice, by their edits' names, in the order a panel shows them.
pub const KINDS: [&str; 5] = ["layout", "cover", "reroll", "brightness", "ceiling"];

/// The kinds of a part's re-roll (a `reroll` edit with a `part`), in `PARTS`' order: each is a
/// kind of its own, so a room has one re-roll of each part and one of its whole inside.
pub const PART_KINDS: [&str; 4] = ["reroll catwalks", "reroll ramps", "reroll cover", "reroll windows"];

/// What the recipe's edits choose for one room: the last of each kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chosen {
    pub layout: Option<String>,
    pub cover: Option<String>,
    pub reroll: Option<u32>,
    pub brightness: Option<i32>,
    pub ceiling: Option<String>,
    /// Features chosen on or off, in `FEATURES`' order.
    pub features: Vec<(&'static str, bool)>,
    /// Parts re-rolled alone (`PARTS`), with which of their other streams, in `PARTS`' order.
    pub parts: Vec<(&'static str, u32)>,
}

impl Chosen {
    /// Is nothing chosen: the room is the seed's own?
    pub fn is_empty(&self) -> bool {
        *self == Chosen::default()
    }

    /// What was chosen, in a few words, in `KINDS`' order: "layout ring, cover none, inside 2,
    /// brightness 90, ceiling grille". Empty if nothing was.
    pub fn words(&self) -> String {
        let mut out = Vec::new();
        out.extend(self.layout.as_ref().map(|v| format!("layout {v}")));
        out.extend(self.cover.as_ref().map(|v| format!("cover {v}")));
        out.extend(self.reroll.map(|n| format!("inside {n}")));
        out.extend(self.brightness.map(|v| format!("brightness {v}")));
        out.extend(self.ceiling.as_ref().map(|v| format!("ceiling {v}")));
        out.extend(self.features.iter().map(|(f, on)| format!("{f} {}", if *on { "on" } else { "off" })));
        out.extend(self.parts.iter().map(|(p, n)| format!("{p} {n}")));
        out.join(", ")
    }
}

/// One room's choices: what it has in the level, what was chosen for it, what it could take.
#[derive(Clone, Debug, PartialEq)]
pub struct Choices {
    pub room: String,
    pub kind: &'static str,
    /// Its hall, if it is a pod (empty otherwise).
    pub host: String,
    /// Its first upper storey as furnished: "ring", "galleries", "mezzanine", or None.
    pub layout: Option<&'static str>,
    /// Each layout that could be chosen, "none" first, with why this room can't take it.
    pub layouts: Vec<(&'static str, Option<String>)>,
    /// What stands on its floor as furnished (the furnisher's word: "pillars", "crates",
    /// "none", or "lanes", the pillars that break a long lane), and how many pieces.
    pub cover: &'static str,
    pub pieces: usize,
    /// Why its cover and its inside can't be chosen (a pod is furnished with its hall).
    pub unfurnished: Option<String>,
    /// The brightness its floor is baked to, where the planner or the recipe set one (None:
    /// the usual).
    pub brightness: Option<i32>,
    /// Its ceiling's role, where the planner or the recipe set one (None: the kit's own).
    pub ceiling: Option<&'static str>,
    /// What the recipe's edits choose for it.
    pub chosen: Chosen,
    /// Each of `FEATURES`: whether the room has it in the level, and why it can't (None: it may).
    pub features: Vec<Feature>,
    /// Each of `PARTS`: which stream it is drawn from, and why re-rolling it would change nothing.
    pub parts: Vec<Part>,
}

/// One part of a room's inside, as its choices show it.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: &'static str,
    /// Which of its other streams it is drawn from (0: the room's own).
    pub n: u32,
    /// How many of it the room has in the level made (pieces of catwalk storeys, ramps, pieces
    /// of cover, windows).
    pub count: usize,
    /// Why a re-roll of it would change nothing (a pod; none of it in the room). None: it may.
    pub cannot: Option<String>,
}

/// One feature of a room, as its choices show it.
#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    pub name: &'static str,
    /// Does the room have it in the level made?
    pub has: bool,
    /// On or off by the recipe's edit (None: the level's chance decides).
    pub chosen: Option<bool>,
    /// Why the room can't have it, as far as can be told without making the level.
    pub cannot: Option<String>,
}

/// The room and the kind of a choice edit ("layout", "cover", ...). None for any other edit.
pub fn kind(e: &Edit) -> Option<(&str, &'static str)> {
    match e {
        // (each part's re-roll is a kind of its own)
        Edit::Reroll { room, part: Some(part), .. } => PARTS.iter().position(|p| p == part).map(|i| (room.as_str(), PART_KINDS[i])),
        Edit::Layout { room, .. } | Edit::Cover { room, .. } | Edit::Reroll { room, .. } | Edit::Brightness { room, .. } | Edit::Ceiling { room, .. } => {
            Some((room, e.name()))
        }
        // (each feature is a kind of its own: a room has one edit for each)
        Edit::Feature { room, feature, .. } => FEATURES.iter().find(|(f, _)| f == feature).map(|(f, _)| (room.as_str(), *f)),
        _ => None,
    }
}

/// What the recipe's edits choose for `room`.
pub fn chosen(recipe: &Recipe, room: &str) -> Chosen {
    let mut c = Chosen::default();
    for e in recipe.edits.iter().filter(|e| kind(e).is_some_and(|(r, _)| r == room)) {
        match e {
            Edit::Layout { layout, .. } => c.layout = Some(layout.clone()),
            Edit::Cover { cover, .. } => c.cover = Some(cover.clone()),
            Edit::Reroll { n, part: None, .. } => c.reroll = Some(*n),
            Edit::Reroll { n, part: Some(part), .. } => {
                let what = PARTS[PARTS.iter().position(|p| p == part).expect("a part's kind")];
                c.parts.retain(|(p, _)| *p != what);
                c.parts.push((what, *n));
            }
            Edit::Brightness { value, .. } => c.brightness = Some(*value),
            Edit::Ceiling { ceiling, .. } => c.ceiling = Some(ceiling.clone()),
            Edit::Feature { on, .. } => {
                let (_, what) = kind(e).expect("a feature's kind");
                c.features.retain(|(f, _)| *f != what);
                c.features.push((what, *on));
            }
            _ => {}
        }
    }
    c.features.sort_by_key(|(f, _)| FEATURES.iter().position(|(g, _)| g == f));
    c.parts.sort_by_key(|(p, _)| PARTS.iter().position(|q| q == p));
    c
}

/// How many of a part (`PARTS`) `room` has in `lv`: storeys with catwalk, ramps, pieces of
/// cover, windows.
fn count(lv: &Level, room: &str, part: &str) -> usize {
    let it = lv.interiors.iter().find(|(k, _)| k == room).map(|(_, it)| it);
    match part {
        "catwalks" => it.map_or(0, |it| it.deck.values().filter(|cells| !cells.is_empty()).count()),
        "ramps" => it.map_or(0, |it| it.ramps.len()),
        "cover" => it.map_or(0, |it| it.cover.len()),
        "windows" => lv.windows.iter().filter(|w| w.hall == room).count(),
        _ => 0,
    }
}

/// Does `room` have the feature `what` in `lv`?
fn has(lv: &Level, room: &str, what: &str) -> bool {
    let s = lv.spaces().iter().find(|s| s.key == room);
    let it = lv.interiors.iter().find(|(k, _)| k == room).map(|(_, it)| it);
    match what {
        "shaped" => s.is_some_and(|s| !s.cuts.is_empty()),
        "flooded" => s.is_some_and(|s| s.flood.is_some()),
        "sunken" => s.is_some_and(|s| s.sunk.is_some()),
        "cellar" => lv.spaces().iter().any(|c| c.cellar.as_ref().is_some_and(|c| c.hall == room)),
        "platforms" => it.is_some_and(|it| !it.platforms.is_empty()),
        "holes" => it.is_some_and(|it| !it.pits.is_empty()),
        "angled" => it.is_some_and(|it| !it.angled.is_empty() || !it.islands.is_empty()),
        _ => false,
    }
}

/// Why `s` can't have the feature `what`, as far as can be told from the room alone: a pod is
/// furnished with its hall, a cellar goes only under a big hall on the ground, and the rest need
/// floor or air enough (only making the level tells: None).
fn cannot(s: &crate::space::Space, what: &str) -> Option<String> {
    if !s.host.is_empty() {
        return Some(format!("{} is a pod of {}: it has its hall's", s.key, s.host));
    }
    let tall = s.storeys().len() > 1;
    match what {
        "cellar" if !(s.floor == 0 && matches!(s.kind, "colossal" | "vast" | "tower")) => {
            Some("only a big hall (colossal, vast or tower) on the ground floor has a cellar".to_string())
        }
        "platforms" | "holes" | "angled" if !tall => Some(format!("{} is one storey: {what} hang in a hall's air", s.key)),
        "shaped" | "flooded" | "sunken" if matches!(s.kind, "spire" | "basin") || s.kind == crate::place::ATRIUM.name => {
            Some(format!("a {} keeps its own shape and floor", s.kind))
        }
        _ => None,
    }
}

/// Every room the recipe chooses something for, in the order the edits first name them, each
/// with what was chosen.
pub fn all(recipe: &Recipe) -> Vec<(String, Chosen)> {
    let mut rooms: Vec<&str> = Vec::new();
    for (room, _) in recipe.edits.iter().filter_map(kind) {
        if !rooms.contains(&room) {
            rooms.push(room);
        }
    }
    rooms.into_iter().map(|room| (room.to_string(), chosen(recipe, room))).collect()
}

/// Why a pod's floor is not dressed by itself.
fn pod(room: &str, host: &str) -> String {
    format!("{room} is a pod of {host}: it is not furnished by itself")
}

/// One room's choices in `lv`, a level made from `recipe`. Err if the level has no such room.
pub fn of(recipe: &Recipe, lv: &Level, room: &str) -> Result<Choices, String> {
    let s = lv.spaces().iter().find(|s| s.key == room).ok_or_else(|| format!("no room {room}"))?;
    let it = lv.interiors.iter().find(|(k, _)| k == room).map(|(_, it)| it);
    let layouts = std::iter::once("none").chain(LAYOUTS).map(|lay| (lay, takes(s, lay).err())).collect();
    Ok(Choices {
        room: room.to_string(),
        kind: s.kind,
        host: s.host.clone(),
        layout: it.and_then(|it| it.layout),
        layouts,
        cover: it.map_or("none", |it| it.cover_mode),
        pieces: it.map_or(0, |it| it.cover.len()),
        unfurnished: (!s.host.is_empty()).then(|| pod(room, &s.host)),
        brightness: lv.moods.iter().find(|(k, _)| k == room).map(|(_, v)| *v),
        ceiling: lv.ceilings.iter().find(|(k, _)| k == room).map(|(_, role)| *role),
        chosen: chosen(recipe, room),
        features: FEATURES
            .iter()
            .map(|&(name, _)| Feature {
                name,
                has: has(lv, room, name),
                chosen: chosen(recipe, room).features.iter().find(|(f, _)| *f == name).map(|(_, on)| *on),
                cannot: cannot(s, name),
            })
            .collect(),
        parts: PARTS
            .iter()
            .map(|&name| {
                let n = chosen(recipe, room).parts.iter().find(|(p, _)| *p == name).map_or(0, |(_, n)| *n);
                let count = count(lv, room, name);
                // (pillars and a lane's pillars stand where the rule puts them: only crates and
                // a room's notch or cross wall are drawn)
                let ruled = name == "cover" && it.is_some_and(|it| it.shaped.is_empty() && matches!(it.cover_mode, "pillars" | "lanes" | "none"));
                let cannot = if !s.host.is_empty() {
                    Some(pod(room, &s.host))
                } else if ruled && n == 0 {
                    Some(format!("{room}'s cover is {}: it stands where the rule puts it, nothing is drawn", it.map_or("none", |it| it.cover_mode)))
                } else if count == 0 && n == 0 {
                    Some(format!("{room} has no {name} to re-roll"))
                } else {
                    None
                };
                Part { name, n, count, cannot }
            })
            .collect(),
    })
}

/// Why a choice can't be made, as far as can be told without making the level: a value that
/// is none of those it may be, and, with a level to look the room up in, a pod's cover or
/// inside and a layout the room can't take. None: it may be tried.
pub fn refusal(lv: Option<&Level>, e: &Edit) -> Option<String> {
    let one_of = |what: &str, all: &[&str]| format!("{what}: one of ({})", all.iter().map(|v| py::repr(v)).collect::<Vec<_>>().join(", "));
    let space = |room: &str| lv.and_then(|lv| lv.spaces().iter().find(|s| s.key == room));
    // (a level to look in, and no such room in it)
    if let (Some(_), Some((room, what))) = (lv, kind(e)) {
        if space(room).is_none() {
            return Some(format!("{what}: no room {room}"));
        }
    }
    match e {
        Edit::Layout { room, layout } => {
            if layout != "none" && !LAYOUTS.contains(&layout.as_str()) {
                return Some(format!("layout: one of ({}, 'none')", LAYOUTS.map(py::repr).join(", ")));
            }
            space(room).and_then(|s| takes(s, layout).err()).map(|why| format!("layout: {why}"))
        }
        Edit::Cover { room, cover } => {
            if !COVER.contains(&cover.as_str()) {
                return Some(one_of("cover", &COVER));
            }
            space(room).filter(|s| !s.host.is_empty()).map(|s| format!("cover: {}", pod(room, &s.host)))
        }
        Edit::Reroll { part: Some(part), .. } if !PARTS.contains(&part.as_str()) => {
            Some(format!("reroll: a part is one of {}, not {part:?}", PARTS.join(", ")))
        }
        Edit::Reroll { room, .. } => space(room).filter(|s| !s.host.is_empty()).map(|s| format!("reroll: {}", pod(room, &s.host))),
        Edit::Brightness { value, .. } => (!(0..=255).contains(value)).then(|| format!("brightness: 0 to 255, not {value}")),
        Edit::Ceiling { ceiling, .. } => (!CEILINGS.contains(&ceiling.as_str())).then(|| one_of("ceiling", &CEILINGS)),
        Edit::Feature { room, feature, on } => {
            let names: Vec<&str> = FEATURES.iter().map(|(f, _)| *f).collect();
            if !names.contains(&feature.as_str()) {
                return Some(one_of("feature", &names));
            }
            // (off is always allowed: it is what the room has anyway where it can't)
            space(room).filter(|_| *on).and_then(|s| cannot(s, feature)).map(|why| format!("{feature}: {why}"))
        }
        _ => Some(format!("{}: not a choice for one room", e.name())),
    }
}

/// The recipe with a choice made: `e` (a `layout`, `cover`, `reroll`, `brightness` or `ceiling`
/// edit) is the room's one edit of its kind, in the place of the first it had, or on the end.
/// A `reroll` of 0 is the seed's own inside: no edit at all. Err for any other edit.
pub fn set(recipe: &Recipe, e: Edit) -> Result<Recipe, String> {
    let (room, what) = kind(&e).ok_or_else(|| format!("{}: not a choice for one room", e.name()))?;
    let (room, mut out) = (room.to_string(), recipe.clone());
    if matches!(e, Edit::Reroll { n: 0, .. }) {
        return Ok(clear(recipe, &room, what));
    }
    let same = |old: &Edit| kind(old) == Some((room.as_str(), what));
    match out.edits.iter().position(same) {
        Some(at) => {
            let mut n = 0;
            out.edits.retain(|old| {
                n += 1;
                n - 1 <= at || !same(old)
            });
            out.edits[at] = e;
        }
        None => out.edits.push(e),
    }
    Ok(out)
}

/// The recipe with the seed's own again for one kind of choice (`KINDS`: the edit's name) in
/// one room: every such edit taken out.
pub fn clear(recipe: &Recipe, room: &str, what: &str) -> Recipe {
    let mut out = recipe.clone();
    let known = KINDS.contains(&what) || PART_KINDS.contains(&what) || FEATURES.iter().any(|(f, _)| *f == what);
    out.edits.retain(|e| kind(e) != Some((room, what)) || !known);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn seed40() -> &'static Level {
        static LV: OnceLock<Level> = OnceLock::new();
        LV.get_or_init(|| Recipe::new(40).generate().expect("seed 40 makes a level"))
    }

    fn cover(room: &str, cover: &str) -> Edit {
        Edit::Cover { room: room.into(), cover: cover.into() }
    }

    #[test]
    fn a_choice_takes_the_place_of_the_rooms_earlier_one() {
        let mut r = Recipe::new(40);
        r.edits.push(Edit::Remove { route: "c23".into() });
        let r = set(&r, cover("vast", "pillars")).unwrap();
        let r = set(&r, Edit::Brightness { room: "vast".into(), value: 120 }).unwrap();
        let r = set(&r, cover("colossal", "none")).unwrap();
        // again for vast: in the first one's place, and the list no longer
        let r = set(&r, cover("vast", "crates")).unwrap();
        let names: Vec<String> = r.edits.iter().map(Edit::describe).collect();
        assert_eq!(names, ["remove c23", "vast: cover crates", "vast: brightness 120", "colossal: cover none"]);
        assert_eq!(chosen(&r, "vast"), Chosen { cover: Some("crates".into()), brightness: Some(120), ..Chosen::default() });
        assert!(chosen(&r, "tower").is_empty() && !chosen(&r, "colossal").is_empty());
        // every room with a choice, in the order first named, and each in a few words
        let words: Vec<(String, String)> = all(&r).into_iter().map(|(room, c)| (room, c.words())).collect();
        assert_eq!(words, [("vast".to_string(), "cover crates, brightness 120".to_string()), ("colossal".to_string(), "cover none".to_string())]);
        let every = Chosen {
            layout: Some("ring".into()),
            cover: Some("none".into()),
            reroll: Some(2),
            brightness: Some(90),
            ceiling: Some("grille".into()),
            features: vec![("flooded", true), ("cellar", false)],
            parts: vec![("catwalks", 3), ("windows", 1)],
        };
        assert_eq!(every.words(), "layout ring, cover none, inside 2, brightness 90, ceiling grille, flooded on, cellar off, catwalks 3, windows 1");
        assert_eq!(Chosen::default().words(), "");
        // a recipe written by hand may have two: both go for the one
        let mut twice = r.clone();
        twice.edits.push(cover("vast", "none"));
        let one = set(&twice, cover("vast", "mixed")).unwrap();
        assert_eq!(one.edits.iter().filter(|e| kind(e) == Some(("vast", "cover"))).count(), 1);
        assert_eq!(one.edits[1], cover("vast", "mixed"));
        // the seed's own again: the edit goes, the others stay
        let back = clear(&r, "vast", "cover");
        assert_eq!(back.edits.iter().map(Edit::describe).collect::<Vec<_>>(), ["remove c23", "vast: brightness 120", "colossal: cover none"]);
        assert_eq!(clear(&back, "vast", "remove"), back);
        // another inside, and the seed's own inside, which is no edit
        let rolled = set(&back, Edit::Reroll { room: "vast".into(), n: 2, part: None }).unwrap();
        assert_eq!(chosen(&rolled, "vast").reroll, Some(2));
        assert_eq!(set(&rolled, Edit::Reroll { room: "vast".into(), n: 0, part: None }).unwrap(), back);
        assert_eq!(set(&r, Edit::Keep { room: "vast".into() }).unwrap_err(), "keep: not a choice for one room");
    }

    #[test]
    fn a_rooms_choices_are_read_from_the_level_and_the_recipe() {
        let lv = seed40();
        let c = of(&Recipe::new(40), lv, "colossal").unwrap();
        assert_eq!((c.kind, c.host.as_str(), c.unfurnished.clone()), ("colossal", "", None));
        assert!(c.chosen.is_empty());
        // a hall takes every layout; what it has is the furnisher's own
        assert_eq!(
            c.layouts.iter().map(|(lay, why)| (*lay, why.is_none())).collect::<Vec<_>>(),
            [("none", true), ("ring", true), ("galleries", true), ("mezzanine", true)]
        );
        assert_eq!(c.layout, lv.interior("colossal").layout);
        assert_eq!((c.cover, c.pieces), (lv.interior("colossal").cover_mode, lv.interior("colossal").cover.len()));
        // a tower's decks are its own; a pod is furnished with its hall
        let t = of(&Recipe::new(40), lv, "tower").unwrap();
        assert!(t.layouts.iter().all(|(_, why)| why.as_deref() == Some("tower is a tower: its decks are its own")), "{:?}", t.layouts);
        let p = of(&Recipe::new(40), lv, "small8").unwrap();
        assert_eq!((p.host.as_str(), p.unfurnished.as_deref()), ("vast", Some("small8 is a pod of vast: it is not furnished by itself")));
        assert_eq!(of(&Recipe::new(40), lv, "attic").unwrap_err(), "no room attic");
        // brightness and ceiling as the level has them, and what the recipe chose
        for (key, v) in &lv.moods {
            assert_eq!(of(&Recipe::new(40), lv, key).unwrap().brightness, Some(*v));
        }
        let r = set(&Recipe::new(40), Edit::Ceiling { room: "colossal".into(), ceiling: "grille".into() }).unwrap();
        let made = r.generate().unwrap();
        let c = of(&r, &made, "colossal").unwrap();
        assert_eq!((c.ceiling, c.chosen.ceiling.as_deref()), (Some("grille"), Some("grille")));
    }

    #[test]
    fn what_can_be_refused_before_the_level_is_made_is() {
        let lv = Some(seed40());
        let layout = |room: &str, lay: &str| Edit::Layout { room: room.into(), layout: lay.into() };
        assert_eq!(refusal(lv, &layout("colossal", "ring")), None);
        assert_eq!(refusal(lv, &layout("tower", "ring")).as_deref(), Some("layout: tower is a tower: its decks are its own"));
        assert_eq!(refusal(lv, &layout("colossal", "maze")).as_deref(), Some("layout: one of ('ring', 'galleries', 'mezzanine', 'none')"));
        assert_eq!(refusal(lv, &cover("small8", "none")).as_deref(), Some("cover: small8 is a pod of vast: it is not furnished by itself"));
        assert_eq!(refusal(lv, &cover("vast", "hedges")).as_deref(), Some("cover: one of ('mixed', 'pillars', 'crates', 'none')"));
        assert_eq!(
            refusal(lv, &Edit::Reroll { room: "small8".into(), n: 1, part: None }).as_deref(),
            Some("reroll: small8 is a pod of vast: it is not furnished by itself")
        );
        assert_eq!(refusal(lv, &Edit::Brightness { room: "vast".into(), value: 300 }).as_deref(), Some("brightness: 0 to 255, not 300"));
        assert_eq!(
            refusal(lv, &Edit::Ceiling { room: "vast".into(), ceiling: "glass".into() }).as_deref(),
            Some("ceiling: one of ('plain', 'ceiling_tile', 'ceiling_dark', 'grille')")
        );
        assert_eq!(refusal(lv, &Edit::Ceiling { room: "vast".into(), ceiling: "grille".into() }), None);
        assert_eq!(refusal(lv, &cover("attic", "none")).as_deref(), Some("cover: no room attic"));
        assert_eq!(refusal(None, &cover("attic", "none")), None);
        // with no level to look the room up in, only the values are held to
        assert_eq!(refusal(None, &cover("small8", "none")), None);
        assert_eq!(refusal(None, &layout("tower", "ring")), None);
        assert!(refusal(None, &Edit::Keep { room: "vast".into() }).is_some());
        // what is refused here is what making the level would refuse
        for e in [layout("tower", "ring"), cover("small8", "none"), Edit::Brightness { room: "vast".into(), value: 300 }] {
            let made = set(&Recipe::new(40), e.clone()).unwrap().generate();
            assert_eq!(made.err(), refusal(lv, &e), "{}", e.describe());
        }
    }
}
