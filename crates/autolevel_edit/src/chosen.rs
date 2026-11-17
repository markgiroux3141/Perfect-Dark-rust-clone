//! One room's choices as text (`autolevel_edit --choices ROOM`): what the room has in the level
//! (its layout, its cover, which inside, its brightness and ceiling), what the recipe chose for
//! it, and for each layout whether the room could take it and if not why, in plain words. Also
//! what a choice did to the recipe, in a line (`said`, `owned`), in the edit list's own words.
//! The plan says the same of a room in a few words under its name (`planview`, `View::chosen`);
//! the choices themselves are `autolevel::choices`.

use autolevel::choices::{kind, Choices, KINDS};
use autolevel::recipe::{Edit, Recipe};
use autolevel::space::Space;

/// A kind of choice as a person names it: the `reroll` edit is the room's "inside".
pub fn word(what: &str) -> &str {
    if what == "reroll" {
        "inside"
    } else {
        what
    }
}

/// One room's choices, a line for each kind and a line for each layout. `s` is the room in the
/// level they were read from.
pub fn text(c: &Choices, s: &Space) -> String {
    let (w, d) = (s.x1 - s.x0, s.y1 - s.y0);
    let what = if c.host.is_empty() { format!("a {} room", c.kind) } else { format!("a {} room, a pod of {}", c.kind, c.host) };
    let mut out = format!("choices for {} ({what}, {w} x {d} cells, {} to {}): what it has, and whose choice that is\n", c.room, s.floor, s.top);
    let whose = |chosen: bool| if chosen { "chosen in the recipe" } else { "the seed's own" };
    // (a pod's floor is dressed with its hall: nothing to choose there)
    let dressed = |chosen: bool| c.unfurnished.as_deref().unwrap_or(whose(chosen)).to_string();
    let rows = [
        ("layout", c.layout.unwrap_or("none").to_string(), whose(c.chosen.layout.is_some()).to_string()),
        ("cover", format!("{}, {} piece{}", c.cover, c.pieces, if c.pieces == 1 { "" } else { "s" }), dressed(c.chosen.cover.is_some())),
        ("inside", c.chosen.reroll.unwrap_or(0).to_string(), dressed(c.chosen.reroll.is_some())),
        ("brightness", c.brightness.map_or("the usual".to_string(), |v| v.to_string()), whose(c.chosen.brightness.is_some()).to_string()),
        ("ceiling", c.ceiling.unwrap_or("the kit's own").to_string(), whose(c.chosen.ceiling.is_some()).to_string()),
    ];
    let wide = rows.iter().map(|(_, has, _)| has.len()).max().unwrap_or(0);
    for (name, has, whose) in &rows {
        out.push_str(&format!("  {name:<10}  {has:<wide$}  {whose}\n"));
    }
    for (lay, why) in &c.layouts {
        match why {
            None => out.push_str(&format!("  could take layout {lay}\n")),
            Some(why) => out.push_str(&format!("  can't take layout {lay}: {why}\n")),
        }
    }
    out
}

/// What making the choice `e` did to the recipe `before`, in a line: the edit as the edit list
/// words it, and the room's earlier choice of the kind, whose place it took.
pub fn said(e: &Edit, before: &Recipe, after: &Recipe) -> String {
    let own = matches!(e, Edit::Reroll { n: 0, .. });
    let mut out = if own { format!("{}, the seed's own", e.describe()) } else { e.describe() };
    let was: Vec<String> = before.edits.iter().filter(|old| kind(old) == kind(e) && *old != e).map(Edit::describe).collect();
    if after == before {
        out.push_str(": nothing changes, it is so already");
    } else if !was.is_empty() {
        out.push_str(&format!(" (in place of: {})", was.join("; ")));
    }
    out
}

/// What giving a room the seed's own again did, in a line: `cleared` are the kinds of choice
/// (`choices::KINDS`) that had one, of those asked for (`what`: a kind, or "all").
pub fn owned(room: &str, what: &str, cleared: &[&str]) -> String {
    let words = |kinds: &[&str]| kinds.iter().map(|k| word(k)).collect::<Vec<_>>().join(", ");
    match (cleared, what) {
        ([], "all") => format!("{room}: nothing was chosen for it"),
        ([], _) => format!("{room}: nothing was chosen for its {}", word(what)),
        _ => format!("{room}: the seed's own {} again", words(cleared)),
    }
}

/// The kinds `--own ROOM@WHAT` names: one, or all of them.
pub fn kinds(what: &str) -> Vec<&'static str> {
    KINDS.into_iter().filter(|k| what == "all" || *k == what).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;
    use crate::fixture;
    use autolevel::choices;

    #[test]
    fn a_rooms_choices_are_listed_with_whose_they_are_and_the_layouts_it_could_take() {
        let made = fixture::seed40();
        let lv = &made.level;
        let recipe = Recipe::new(40);
        let of = |r: &Recipe, room: &str| text(&choices::of(r, lv, room).unwrap(), lv.space(room));
        // a hall: every layout, everything the seed's own
        let said = of(&recipe, "colossal");
        let c = choices::of(&recipe, lv, "colossal").unwrap();
        assert_eq!(said.lines().next().unwrap(), "choices for colossal (a colossal room, 20 x 27 cells, 0 to 1063): what it has, and whose choice that is");
        assert_eq!(said.lines().count(), 10);
        let row = |said: &str, name: &str| said.lines().find(|l| l.trim_start().starts_with(name)).unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(row(&said, "layout"), format!("layout {} the seed's own", c.layout.unwrap_or("none")));
        assert_eq!((c.cover, c.pieces, row(&said, "cover").as_str()), ("lanes", 1, "cover lanes, 1 piece the seed's own"));
        assert_eq!(row(&said, "inside"), "inside 0 the seed's own");
        assert!(said.ends_with("  could take layout none\n  could take layout ring\n  could take layout galleries\n  could take layout mezzanine\n"), "{said}");
        // what the recipe chose says so (the level here is still the seed's: what it has is as it was)
        let mut r = recipe.clone();
        r.edits.push(Edit::Layout { room: "colossal".into(), layout: "ring".into() });
        r.edits.push(Edit::Reroll { room: "colossal".into(), n: 2 });
        r.edits.push(Edit::Brightness { room: "colossal".into(), value: 90 });
        let said = of(&r, "colossal");
        assert!(row(&said, "layout").ends_with(" chosen in the recipe") && row(&said, "cover").ends_with(" the seed's own"), "{said}");
        assert_eq!((row(&said, "inside"), row(&said, "brightness").ends_with("chosen in the recipe")), ("inside 2 chosen in the recipe".to_string(), true));
        // a tower's decks are its own; a room of one storey takes no layout; a pod is not dressed by itself
        let said = of(&recipe, "tower");
        assert!(said.contains("\n  can't take layout none: tower is a tower: its decks are its own\n") && !said.contains("could take"), "{said}");
        assert!(of(&recipe, "small4").contains("\n  can't take layout ring: small4 has one storey: a layout is for a hall's first upper storey\n"));
        let said = of(&recipe, "small8");
        assert!(said.starts_with("choices for small8 (a small room, a pod of vast, 4 x 3 cells, 551 to 787): "), "{said}");
        assert_eq!(row(&said, "cover"), "cover none, 0 pieces small8 is a pod of vast: it is not furnished by itself");
        assert!(said.contains("\n  can't take layout ring: small8 is a pod: it has no storeys of its own\n"), "{said}");
        // the columns line up
        let at: Vec<usize> = of(&recipe, "vast").lines().skip(1).take(5).map(|l| l.rfind("the seed's own").unwrap()).collect();
        assert!(at.windows(2).all(|w| w[0] == w[1]), "{at:?}");
    }

    #[test]
    fn a_choice_is_said_in_the_edit_lists_words() {
        let mut d = Doc::new(Recipe::new(40), true);
        d.regenerate().unwrap();
        let make = |d: &mut Doc, e: Edit| {
            let before = d.recipe.clone();
            d.set_choice(e.clone()).unwrap();
            said(&e, &before, &d.recipe)
        };
        let cover = |v: &str| Edit::Cover { room: "vast".into(), cover: v.into() };
        assert_eq!(make(&mut d, cover("none")), "vast: cover none");
        assert_eq!(make(&mut d, cover("crates")), "vast: cover crates (in place of: vast: cover none)");
        assert_eq!(make(&mut d, cover("crates")), "vast: cover crates: nothing changes, it is so already");
        assert_eq!(make(&mut d, Edit::Reroll { room: "vast".into(), n: 2 }), "vast: inside 2");
        assert_eq!(make(&mut d, Edit::Reroll { room: "vast".into(), n: 0 }), "vast: inside 0, the seed's own (in place of: vast: inside 2)");
        assert_eq!(make(&mut d, Edit::Reroll { room: "vast".into(), n: 0 }), "vast: inside 0, the seed's own: nothing changes, it is so already");
        assert_eq!(make(&mut d, Edit::Layout { room: "colossal".into(), layout: "ring".into() }), "colossal: layout ring");
        // the seed's own again, for one kind or for all
        assert_eq!(owned("vast", "cover", &["cover"]), "vast: the seed's own cover again");
        assert_eq!(owned("vast", "all", &["cover", "reroll"]), "vast: the seed's own cover, inside again");
        assert_eq!(owned("vast", "reroll", &[]), "vast: nothing was chosen for its inside");
        assert_eq!(owned("vast", "all", &[]), "vast: nothing was chosen for it");
        assert_eq!((kinds("all"), kinds("reroll"), kinds("maze")), (KINDS.to_vec(), vec!["reroll"], vec![]));
        assert_eq!((word("reroll"), word("layout")), ("inside", "layout"));
    }
}
