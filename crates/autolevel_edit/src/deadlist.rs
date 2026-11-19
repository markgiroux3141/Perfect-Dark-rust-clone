//! The dead-end list, for the editor: what hangs off a level by one way in. As the rooms and
//! corridors first fall, with where each of the long ones could be joined to the rest; what the
//! dead-end pass did about them; and what is left.
//!
//! The list itself is `autolevel::deadlist` (the command line prints the same one: its `text` is
//! what `python scripts/autolevel.py deadends SEED` prints, line for line). Here is what the
//! editor puts on top: each dead end's `Mark` (what to pick out on the plan), the rows as the
//! window lists them, each with its mark, and the plan's own numbering of what is left
//! (`numbered`, the "on the plan:" section).

use crate::planview::Mark;
use autolevel::build::Fix;
use autolevel::deadends::{DeadEnd, Join};
use autolevel::deadlist::{self as list, Of};
use autolevel::level::Level;
use autolevel::recipe::Recipe;
use std::ops::Deref;

pub use autolevel::deadlist::{fix_text, Kind, JOINS};

/// A dead end, where it could be joined, and what to pick out on the plan for it (`mark`).
pub type Entry = list::Entry<Mark>;

/// A level's dead ends, before the pass and after, each with its mark. It is the shared list
/// (`before`, `fixes`, `left`, `camps`, `long()`, `text()`), with the editor's rows on top.
#[derive(Clone, Debug, PartialEq)]
pub struct DeadList(pub list::DeadList<Mark>);

impl Deref for DeadList {
    type Target = list::DeadList<Mark>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A level and its dead-end list: what a recipe makes, for the editor.
#[derive(Clone, Debug)]
pub struct Made {
    pub level: Level,
    pub dead: DeadList,
}

/// Run a recipe, stopping before the dead-end pass to list the dead ends as the rooms fall.
pub fn make(recipe: &Recipe) -> Result<Made, String> {
    let (level, dead) = list::make_with(recipe, Mark::of)?;
    Ok(Made { level, dead: DeadList(dead) })
}

/// A row of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    pub text: String,
    /// What to pick out on the plan when the row is chosen.
    pub mark: Option<Mark>,
    /// For a `Join` row: the join, as a recipe's edit takes it.
    pub join: Option<Join>,
    /// For a row of what is left: the number the plan gives that dead end (from 1).
    pub number: Option<usize>,
}

/// What a fix touched, to pick out on the plan.
fn fix_mark(f: &Fix) -> Mark {
    match f {
        Fix::Joined { room, to, by, .. } => Mark { rooms: vec![room.clone(), to.clone()], routes: vec![by.clone()], ..Mark::default() },
        Fix::Cut { room, .. } | Fix::Dropped { room } => Mark { rooms: vec![room.clone()], ..Mark::default() },
    }
}

impl DeadList {
    /// The list as rows, in the order the Python prints it; then, where the Python stops, the
    /// allowed dead ends that are left too (the plan marks every one with its number).
    pub fn rows(&self) -> Vec<Row> {
        let fixes = self.fixes.as_deref().unwrap_or(&[]);
        let rows = self.0.rows().into_iter().map(|r| {
            let (mark, join, number) = match r.of {
                Of::Head => (None, None, None),
                Of::Before(n) => (Some(self.before[n].mark.clone()), None, None),
                Of::Join(n, k) => {
                    let j = &self.before[n].joins[k];
                    (Some(Mark { rooms: vec![j.0.clone(), j.2.clone()], ..Mark::default() }), Some(j.clone()), None)
                }
                Of::Fix(n) => (Some(fix_mark(&fixes[n])), None, None),
                Of::Left(n) => (Some(self.left[n].mark.clone()), None, Some(n + 1)),
            };
            Row { kind: r.kind, text: r.text, mark, join, number }
        });
        rows.collect()
    }

    /// What the plan's numbers are: every dead end left in the level, as the plan names it.
    pub fn numbered(&self) -> String {
        let mut out = String::new();
        for (n, e) in self.left.iter().enumerate() {
            out.push_str(&format!(
                "  {} {}: {}
",
                what(&e.end),
                n + 1,
                e.end.describe()
            ));
        }
        out
    }
}

/// What the plan calls a dead end that is left: a room to camp in, a deck (a balcony, a pod's
/// gangway), or, too long, a DEAD END.
pub fn what(e: &DeadEnd) -> &'static str {
    match (e.allowed, e.rooms.is_empty()) {
        (false, _) => "DEAD END",
        (true, false) => "camp",
        (true, true) => "deck",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    /// What `python scripts/autolevel.py deadends SEED` printed (the Python is the reference).
    fn python(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    #[test]
    fn the_list_is_the_pythons_line_for_line() {
        // (the Python's levels: none of the Rust's own rooms that are on by default, flooded or shaped)
        let python_level = |seed: u64| {
            make(&Recipe { seed, options: autolevel::level::Options { flooded: 0.0, shaped: 0.0, ..Default::default() }, edits: Vec::new() }).unwrap()
        };
        assert_eq!(python_level(40).dead.text(), python(include_str!("../../autolevel/tests/deadlist/deadends_seed40.txt")));
        let seven = python_level(7);
        assert_eq!(seven.dead.text(), python(include_str!("../../autolevel/tests/deadlist/deadends_seed7.txt")));
        // a room left out by the pass is in the list as the rooms fell, and not in the level
        assert!(seven.dead.before.iter().any(|e| e.end.rooms.contains(&"small2".to_string())));
        assert!(seven.level.spaces().iter().all(|s| s.key != "small2"));
    }

    #[test]
    fn rows_carry_what_the_window_needs() {
        let dead = &fixture::seed40().dead;
        let rows = dead.rows();
        assert_eq!((dead.before.len(), dead.long(), dead.fixes.as_ref().map(Vec::len), dead.left.len(), dead.camps), (11, 2, Some(2), 10, 6));
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Head).count(), 3);
        // the long ones list where they could be joined, each as an edit would take it
        let joins: Vec<&Row> = rows.iter().filter(|r| r.kind == Kind::Join).collect();
        assert_eq!(joins.len(), 9);
        let (room, storey, to, on, sep) = joins[0].join.clone().unwrap();
        assert_eq!((room.as_str(), storey, to.as_str(), on, sep), ("spire1", 1102, "loft3", vec![551], 15.0));
        assert_eq!(joins[0].mark.as_ref().unwrap().rooms, ["spire1", "loft3"]);
        // a fix picks out its rooms and the corridor it added
        let fix = rows.iter().find(|r| r.kind == Kind::Fix).unwrap();
        assert_eq!(fix.text, "joined spire1 on 1102 to loft3 by a corridor, 14 cells with 2 ramps (c39a)");
        assert_eq!(fix.mark.as_ref().unwrap().routes, ["c39a"]);
        // what is left is numbered as the plan numbers it
        let left: Vec<&Row> = rows.iter().filter(|r| r.number.is_some()).collect();
        assert_eq!(left.iter().map(|r| r.number.unwrap()).collect::<Vec<_>>(), (1..=10).collect::<Vec<_>>());
        assert!(left.iter().all(|r| r.kind == Kind::Allowed && r.mark.as_ref().is_some_and(|m| m.at.is_some())));
        assert!(dead.numbered().starts_with("  camp 1: spire2@0 -> small7@-276 (small7): 20 cells, 5 of corridor, 1 storey\n"));
        assert_eq!(what(&dead.left[2].end), "deck");
        // a camp's mark is its room, the corridor to it, and the floor that corridor leaves from
        let camp = &dead.left[0].mark;
        assert_eq!(
            (camp.rooms.as_slice(), camp.routes.as_slice(), camp.decks.as_slice()),
            (&["small7".to_string()][..], &["c17".to_string()][..], &[("spire2".to_string(), -276)][..])
        );
    }

    #[test]
    fn a_level_that_keeps_its_dead_ends_lists_no_fixes() {
        let dead = &fixture::seed40_kept().dead;
        assert_eq!((dead.fixes.as_ref(), dead.left.len(), dead.left.iter().filter(|e| !e.end.allowed).count()), (None, 11, 2));
        let text = dead.text();
        assert!(!text.contains("done about them") && !text.contains("left:"));
        // the Python prints the same and stops there
        assert_eq!(text, fixture::seed40().dead.text().split("done about them").next().unwrap());
        assert!(dead.numbered().starts_with("  DEAD END 1: spire1@551 -> spire1@1102 (decks)"));
        assert_eq!(fix_text(&Fix::Dropped { room: "small2".into() }), "left out small2");
        assert_eq!(fix_text(&Fix::Cut { room: "spire1".into(), above: 827 }), "cut spire1: no deck of its own above 827");
        let vent = Fix::Joined { room: "a".into(), storey: 0, to: "b".into(), by: "v05".into(), kind: "vent", length: 6.5, ramps: 0 };
        assert_eq!(fix_text(&vent), "joined a on 0 to b by a vent, 6 cells (v05)");
    }
}
