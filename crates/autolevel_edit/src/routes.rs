//! A level's corridors, stubs and vents as a list: each route's key (what `--remove` and the
//! recipe's `remove` edit name it by), the two doors it joins, what it is, and what there is to
//! know before taking it out: that the dead-end pass added it, or that it is the only way to a
//! room. `autolevel_edit --routes` prints it; the window says the same of a corridor picked.
//! Also what to know of a corridor that could be made (`join_note`).

use autolevel::build::Fix;
use autolevel::candidates::{cut_off, footprint, how, Candidate, Candidates};
use autolevel::level::Level;
use autolevel::route::Route;

/// A route of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The route's key: its first piece's.
    pub key: String,
    /// Every piece's key, where there are several (any of them names the route).
    pub pieces: Vec<String>,
    /// "medium3 on 0 - vast on 276": its two doors.
    pub ends: String,
    /// "a corridor, 14 cells with 1 ramp".
    pub how: String,
    /// What to know before taking it out.
    pub note: Option<String>,
}

/// A route's two doors and what it is: "medium3 on 0 - vast on 276: a corridor, 14 cells with 1 ramp".
pub fn describe(r: &Route) -> String {
    format!("{}: {}", ends(r), how(r))
}

fn ends(r: &Route) -> String {
    format!("{} on {} - {} on {}", r.a.room, r.a.level, r.b.room, r.b.level)
}

/// What there is to know before taking a route out of this level.
pub fn note(lv: &Level, r: &Route) -> Option<String> {
    let pass = lv.fixes.iter().any(|f| matches!(f, Fix::Joined { by, .. } if by == r.key()));
    let alone = match cut_off(&lv.net, r).as_slice() {
        [] => None,
        [room] => Some(format!("the only way to {room}")),
        [room, more @ ..] => Some(format!("the only way to {room} and {} past it", more.len())),
    };
    match (pass, alone) {
        (true, Some(alone)) => Some(format!("added by the dead-end pass; {alone}")),
        (true, None) => Some("added by the dead-end pass".to_string()),
        (false, alone) => alone,
    }
}

/// What to know of a corridor that could be made, looking at the level as it stands: that it
/// lies where the dead-end pass has put a corridor of its own. (It is found on the level as
/// routed, before the pass; with it made, the pass does without its own or goes another way.)
pub fn join_note(lv: &Level, c: &Candidate) -> Option<String> {
    let cells = c.cells();
    let pass = |r: &&Route| lv.fixes.iter().any(|f| matches!(f, Fix::Joined { by, .. } if by == r.key()));
    let over = lv.net.routes().iter().filter(pass).find(|r| !footprint(r).is_disjoint(&cells))?;
    Some(format!("lies where the dead-end pass now adds {}", over.key()))
}

/// The corridors that could be made from a picked room, in words, numbered as the plan numbers
/// them, each with its note.
pub fn joins_text(lv: &Level, found: &Candidates) -> String {
    found.text_with(|c| join_note(lv, c))
}

/// Every route of a level, in the order they were made.
pub fn rows(lv: &Level) -> Vec<Row> {
    let row = |r: &Route| Row {
        key: r.key().to_string(),
        pieces: if r.pieces.len() > 1 { r.pieces.iter().map(|pc| pc.key.clone()).collect() } else { Vec::new() },
        ends: ends(r),
        how: how(r),
        note: note(lv, r),
    };
    lv.net.routes().iter().map(row).collect()
}

/// The list as text, a route a line.
pub fn text(lv: &Level) -> String {
    let rows = rows(lv);
    let wide = |f: fn(&Row) -> usize| rows.iter().map(f).max().unwrap_or(0);
    let (key, ends) = (wide(|r| r.key.len()), wide(|r| r.ends.len()));
    let mut out = format!("routes ({}): the key to name each by, the doors it joins, what it is\n", rows.len());
    for r in &rows {
        let mut line = format!("  {:<key$}  {:<ends$}  {}", r.key, r.ends, r.how);
        if r.pieces.len() > 1 {
            line.push_str(&format!(" (pieces {})", r.pieces.join(" ")));
        }
        if let Some(note) = &r.note {
            line.push_str(&format!("  [{note}]"));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    #[test]
    fn every_route_is_listed_with_what_taking_it_out_would_mean() {
        let lv = &fixture::seed40().level;
        let rows = rows(lv);
        assert_eq!(rows.len(), lv.net.routes().len());
        assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), lv.net.routes().iter().map(|r| r.key()).collect::<Vec<_>>());
        // the two corridors the dead-end pass added say so
        let pass: Vec<&Row> = rows.iter().filter(|r| r.note.as_ref().is_some_and(|n| n.starts_with("added by the dead-end pass"))).collect();
        assert_eq!(pass.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["c39a", "c42"]);
        assert_eq!(pass[0].pieces, ["c39a", "c39b"]);
        assert_eq!((pass[0].ends.as_str(), pass[0].how.as_str()), ("spire1 on 1102 - loft3 on 551", "a corridor, 14 cells with 2 ramps"));
        // a room with one corridor: that corridor is the only way to it
        let camp = rows.iter().find(|r| r.key == "c17").unwrap();
        assert_eq!(camp.note.as_deref(), Some("the only way to small7"));
        assert!(rows.iter().any(|r| r.note.is_none()) && rows.iter().any(|r| r.how.starts_with("a vent, ")));
        // the text: a line each under a heading, the keys in a column
        let text = text(lv);
        assert_eq!(text.lines().count(), rows.len() + 1);
        assert!(text.starts_with(&format!("routes ({}): ", rows.len())));
        assert!(text.contains("  c39a  spire1 on 1102 - loft3 on 551  ") && text.contains("(pieces c39a c39b)  [added by the dead-end pass]"), "{text}");
        assert_eq!(describe(&lv.net.routes()[0]), format!("{}: {}", rows[0].ends, rows[0].how));
    }
}
