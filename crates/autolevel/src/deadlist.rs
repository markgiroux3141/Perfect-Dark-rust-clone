//! The dead-end list: what hangs off a level by one way in. As the rooms and corridors first
//! fall, with where each of the long ones could be joined to the rest; what the dead-end pass
//! did about them; and what is left. From `deadends` in `scripts/autolevel.py`: `DeadList::text`
//! is what `python scripts/autolevel.py deadends SEED` prints, line for line.
//!
//! A pure function of the stages (`make` runs a recipe, stopping before the dead-end pass to
//! look), so the command line prints it and the editor lists it from the same rows. A caller may
//! hang something of its own on each dead end, made from the dead end and the routes its numbers
//! count in (`make_with`: the editor hangs what to pick out on its plan); `make` hangs nothing.

use crate::build::Fix;
use crate::deadends::{self, DeadEnd, Join};
use crate::level::Level;
use crate::py;
use crate::recipe::Recipe;
use crate::route::Route;

/// The most joins listed for one dead end (the Python's command lists six).
pub const JOINS: usize = 6;

/// A dead end, where it could be joined, and what the caller hung on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry<M = ()> {
    pub end: DeadEnd,
    /// (room, storey, other room, the storeys it may take the door on, cells apart): for the
    /// long ones as the rooms fall, nearest the tip first.
    pub joins: Vec<Join>,
    pub mark: M,
}

/// A level's dead ends, before the pass and after.
#[derive(Clone, Debug, PartialEq)]
pub struct DeadList<M = ()> {
    pub seed: u64,
    /// The limits a dead end is allowed within: cells of corridor, cells walked, storeys climbed.
    pub limits: (i32, i32, i32),
    /// As the rooms and corridors first fall (with the recipe's own joins and removals).
    pub before: Vec<Entry<M>>,
    /// What the pass did, in order. None: the level's options say to leave its dead ends.
    pub fixes: Option<Vec<Fix>>,
    /// What is left in the finished level.
    pub left: Vec<Entry<M>>,
    /// How many of those are rooms to camp in.
    pub camps: usize,
}

/// Run a recipe, stopping before the dead-end pass to list the dead ends as the rooms fall.
/// The level, and its list.
pub fn make(recipe: &Recipe) -> Result<(Level, DeadList), String> {
    make_with(recipe, |_, _| ())
}

/// `make`, with `mark(dead end, the network's routes its numbers count in)` hung on each entry.
pub fn make_with<M>(recipe: &Recipe, mark: impl Fn(&DeadEnd, &[Route]) -> M) -> Result<(Level, DeadList<M>), String> {
    let (b, dropped) = recipe.furnished()?;
    let mut was = b.clone();
    let (g, ends) = was.ends();
    let before = ends
        .into_iter()
        .map(|end| {
            let mut joins = if end.allowed { Vec::new() } else { deadends::joins(&g, was.spaces(), &end, deadends::REACH, deadends::STOREYS, deadends::EACH) };
            joins.truncate(JOINS);
            Entry { mark: mark(&end, was.net.routes()), joins, end }
        })
        .collect();
    let level = recipe.finish(b, dropped)?;
    let o = &level.options;
    let left = level.dead_ends.iter().map(|end| Entry { mark: mark(end, level.net.routes()), joins: Vec::new(), end: end.clone() }).collect();
    let dead = DeadList {
        seed: level.seed,
        limits: (o.dead_corridor, o.dead_walk, o.dead_climb),
        before,
        fixes: o.dead_ends.then(|| level.fixes.clone()),
        left,
        camps: level.report.camps,
    };
    Ok((level, dead))
}

/// What a row of the list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A heading.
    Head,
    /// A dead end that is too long, too far or too high.
    Long,
    /// One that is allowed.
    Allowed,
    /// Somewhere the dead end above could be joined.
    Join,
    /// Something the pass did.
    Fix,
}

/// What a row is about: where it is in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Of {
    /// A heading: nothing.
    Head,
    /// `before[n]`.
    Before(usize),
    /// `before[n].joins[k]`.
    Join(usize, usize),
    /// `fixes[n]`.
    Fix(usize),
    /// `left[n]`.
    Left(usize),
}

/// A row of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    pub text: String,
    pub of: Of,
}

impl Row {
    /// The row as the Python's command prints it. None for a row it does not print (of what is
    /// left, it lists only the long ones).
    pub fn line(&self) -> Option<String> {
        Some(match (self.kind, self.of) {
            (Kind::Head, _) => self.text.clone(),
            (Kind::Long, _) => format!("  ! {}", self.text),
            (Kind::Allowed, Of::Left(_)) => return None,
            (Kind::Allowed, _) | (Kind::Fix, _) => format!("    {}", self.text),
            (Kind::Join, _) => format!("        {}", self.text),
        })
    }
}

/// One thing the pass did, in words.
pub fn fix_text(f: &Fix) -> String {
    match f {
        Fix::Joined { room, storey, to, by, kind, length, ramps } => {
            let cells = py::round(*length);
            let how = match *kind {
                "stub" => "a door through the wall".to_string(),
                "vent" => format!("a vent, {cells} cells"),
                _ if *ramps > 0 => format!("a corridor, {cells} cells with {ramps} ramp{}", if *ramps > 1 { "s" } else { "" }),
                _ => format!("a corridor, {cells} cells"),
            };
            format!("joined {room} on {storey} to {to} by {how} ({by})")
        }
        Fix::Cut { room, above } => format!("cut {room}: no deck of its own above {above}"),
        Fix::Dropped { room } => format!("left out {room}"),
    }
}

/// Somewhere a dead end could be joined, in words.
pub fn join_text(j: &Join) -> String {
    let (room, level, other, levels, sep) = j;
    let on = levels.iter().map(|z| z.to_string()).collect::<Vec<_>>().join(", ");
    format!("could join {room} on {level} to {other} (on {on}; {} cells)", py::round(*sep))
}

impl<M> DeadList<M> {
    /// The dead ends as the rooms fall that are too long, too far or too high.
    pub fn long(&self) -> usize {
        self.before.iter().filter(|e| !e.end.allowed).count()
    }

    /// The list as rows, in the order the Python prints it; then, where the Python stops, the
    /// allowed dead ends that are left too.
    pub fn rows(&self) -> Vec<Row> {
        let (corridor, walk, climb) = self.limits;
        let head = |text: String| Row { kind: Kind::Head, text, of: Of::Head };
        let kind = |e: &Entry<M>| if e.end.allowed { Kind::Allowed } else { Kind::Long };
        let mut out = vec![head(format!(
            "seed {}: {} dead ends as it falls, {} of them too long, too far or too high (more than {corridor} cells of corridor, {walk} cells' walk, or {climb} storeys up to nothing)",
            self.seed,
            self.before.len(),
            self.long()
        ))];
        for (n, e) in self.before.iter().enumerate() {
            out.push(Row { kind: kind(e), text: e.end.describe(), of: Of::Before(n) });
            out.extend(e.joins.iter().enumerate().map(|(k, j)| Row { kind: Kind::Join, text: join_text(j), of: Of::Join(n, k) }));
        }
        let Some(fixes) = &self.fixes else { return out };
        out.push(head(format!("done about them ({}):", fixes.len())));
        out.extend(fixes.iter().enumerate().map(|(n, f)| Row { kind: Kind::Fix, text: fix_text(f), of: Of::Fix(n) }));
        let long = self.left.iter().filter(|e| !e.end.allowed).count();
        out.push(head(format!("left: {long} too long, {} allowed ({} of them rooms to camp in)", self.left.len() - long, self.camps)));
        out.extend(self.left.iter().enumerate().map(|(n, e)| Row { kind: kind(e), text: e.end.describe(), of: Of::Left(n) }));
        out
    }

    /// The list as the Python's `deadends` command prints it.
    pub fn text(&self) -> String {
        self.rows().iter().filter_map(Row::line).map(|line| line + "\n").collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Options;

    #[test]
    fn what_was_done_in_words() {
        assert_eq!(fix_text(&Fix::Dropped { room: "small2".into() }), "left out small2");
        assert_eq!(fix_text(&Fix::Cut { room: "spire1".into(), above: 827 }), "cut spire1: no deck of its own above 827");
        let join = |kind, length, ramps| Fix::Joined { room: "a".into(), storey: 0, to: "b".into(), by: "v05".into(), kind, length, ramps };
        // (lengths are rounded as Python rounds them: a half goes to the even number)
        assert_eq!(fix_text(&join("vent", 6.5, 0)), "joined a on 0 to b by a vent, 6 cells (v05)");
        assert_eq!(fix_text(&join("stub", 1.0, 0)), "joined a on 0 to b by a door through the wall (v05)");
        assert_eq!(fix_text(&join("corridor", 7.5, 0)), "joined a on 0 to b by a corridor, 8 cells (v05)");
        assert_eq!(fix_text(&join("ramp", 14.0, 1)), "joined a on 0 to b by a corridor, 14 cells with 1 ramp (v05)");
        assert_eq!(fix_text(&join("ramp", 14.0, 2)), "joined a on 0 to b by a corridor, 14 cells with 2 ramps (v05)");
        let j: Join = ("spire1".into(), 1102, "loft3".into(), vec![551, 827], 14.5);
        assert_eq!(join_text(&j), "could join spire1 on 1102 to loft3 (on 551, 827; 14 cells)");
    }

    /// Seed 40's list is the Python's, line for line. (The text the Python printed is the
    /// editor's fixture; `autolevel_edit` holds seed 7's to it as well.)
    #[test]
    fn the_list_is_the_pythons_line_for_line() {
        let python = include_str!("../tests/deadlist/deadends_seed40.txt").replace("\r\n", "\n");
        let (level, dead) = make(&Recipe::before_bases(40)).unwrap();
        assert_eq!(dead.text(), python);
        assert_eq!((dead.before.len(), dead.long(), dead.fixes.as_ref().map(Vec::len), dead.left.len(), dead.camps), (11, 2, Some(2), 10, 6));
        assert_eq!(level.dead_ends.len(), 10);
        // rows say where they are in the list, and the Python prints all but the allowed ones left
        let rows = dead.rows();
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Head).count(), 3);
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Join).count(), 9);
        let first = rows.iter().find(|r| r.kind == Kind::Join).unwrap();
        let Of::Join(n, k) = first.of else { panic!("a join row is of a join") };
        assert_eq!(
            (dead.before[n].joins[k].0.as_str(), k, first.line().unwrap().starts_with("        could join spire1 on 1102 to loft3")),
            ("spire1", 0, true)
        );
        assert_eq!(rows.iter().filter(|r| matches!(r.of, Of::Left(_))).count(), 10);
        assert_eq!(rows.iter().filter(|r| r.line().is_none()).count(), 10);
        // a level that keeps its dead ends: the same list of them, and the Python stops there
        let kept = Recipe { seed: 40, options: Options { dead_ends: false, ..Options::before_bases() }, edits: Vec::new() };
        let (_, kept) = make(&kept).unwrap();
        assert_eq!((kept.fixes.as_ref(), kept.left.len()), (None, 11));
        assert_eq!(kept.text(), python.split("done about them").next().unwrap());
    }
}
