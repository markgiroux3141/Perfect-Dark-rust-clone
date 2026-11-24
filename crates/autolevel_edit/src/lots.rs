//! The room boxes as text: the lots a recipe places, a line each (`autolevel_edit --lots`: the
//! key the room flags name it by, its kind, its box in cells, its heights, and whose pod it is or
//! which pods it has), and what a change to them did to the recipe, in a line (`said`), in the
//! edit list's own words. The plan draws the same lots as boxes (`planview`); the changes
//! themselves are `autolevel::boxes`.

use autolevel::boxes::{Change, Trial};
use autolevel::recipe::{Edit, Recipe};
use autolevel::scatter::Lot;
use autolevel::storeys::snap_top;

/// A lot of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub kind: String,
    /// "33,2 to 59,22 (26 x 20)": its low and high corners in cells, and its size.
    pub place: String,
    /// "0 to 1338": its floor and its top, on the ceiling line the room is built to.
    pub heights: String,
    /// "pod of vast", or "pods small8 small9" ("" for a room with neither).
    pub pods: String,
}

/// Every lot, in the order they were scattered (made ones last).
pub fn rows(lots: &[Lot]) -> Vec<Row> {
    let row = |a: &Lot| {
        let pods: Vec<&str> = lots.iter().filter(|p| p.host == a.key).map(|p| p.key.as_str()).collect();
        Row {
            key: a.key.clone(),
            kind: a.kind.to_string(),
            place: format!("{},{} to {},{} ({} x {})", a.x0, a.y0, a.x1, a.y1, a.x1 - a.x0, a.y1 - a.y0),
            heights: format!("{} to {}", a.floor, snap_top(a.top)),
            pods: match (a.host.as_str(), pods.as_slice()) {
                ("", []) => String::new(),
                ("", pods) => format!("pods {}", pods.join(" ")),
                (host, _) => format!("pod of {host}"),
            },
        }
    };
    lots.iter().map(row).collect()
}

/// The list as text, a lot a line, on a site of `site` cells. `would`: these are the lots as a
/// change being tried would leave them, not as the recipe has them.
pub fn text(lots: &[Lot], site: (i32, i32), would: bool) -> String {
    let rows = rows(lots);
    let wide = |f: fn(&Row) -> usize| rows.iter().map(f).max().unwrap_or(0);
    let (key, kind, place, heights) = (wide(|r| r.key.len()), wide(|r| r.kind.len()), wide(|r| r.place.len()), wide(|r| r.heights.len()));
    let which = if would { " as they would lie" } else { "" };
    let columns = "key, kind, low corner to high corner (cells wide x deep), floor to top";
    let mut out = format!("lots ({}){which}, on a site of {} x {} cells: {columns}\n", rows.len(), site.0, site.1);
    for r in &rows {
        let line = format!("  {:<key$}  {:<kind$}  {:<place$}  {:<heights$}  {}", r.key, r.kind, r.place, r.heights, r.pods);
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// What a change asks for, worded as the edit list would word it.
pub fn asked(c: &Change) -> String {
    match c {
        Change::Move { room, to } => Edit::Move { room: room.clone(), to: *to }.describe(),
        Change::Rect { room, rect } => format!("{room}'s box to {}, {} to {}, {}", rect.0, rect.1, rect.2, rect.3),
        Change::Pin { room, floor } => Edit::Pin { room: room.clone(), floor: *floor }.describe(),
        Change::Height { room, floor, top } => Edit::Height { room: room.clone(), floor: *floor, top: *top }.describe(),
        Change::Add { key, kind, rect, floor, top } => Edit::Add { key: key.clone(), kind: kind.clone(), rect: *rect, floor: *floor, top: *top }.describe(),
        Change::Delete { room } => Edit::Delete { room: room.clone() }.describe(),
    }
}

/// What the change `c` did (or would do) to the recipe `before`, whose lots were `lots`, in a
/// line: the edits it added, as the edit list words them; the edits it made needless, which
/// went; a hall's pods that go with it; and the later edits a delete took along. `t` is the
/// change's trial, which was not refused.
pub fn said(c: &Change, before: &Recipe, lots: &[Lot], t: &Trial) -> String {
    let added: Vec<String> = t.recipe.edits.iter().filter(|e| !before.edits.contains(e)).map(Edit::describe).collect();
    let left = |e: &&Edit| !t.recipe.edits.contains(e) && !t.dropped.contains(&e.describe());
    let taken: Vec<String> = before.edits.iter().filter(left).map(Edit::describe).collect();
    let mut out = if added.is_empty() { asked(c) } else { added.join("; ") };
    match (added.is_empty(), taken.is_empty()) {
        (_, false) if added.is_empty() => out.push_str(&format!(": the recipe needs no edit for it, and loses: {}", taken.join("; "))),
        (_, false) => out.push_str(&format!(" (in place of: {})", taken.join("; "))),
        (true, true) if t.recipe == *before => out.push_str(": nothing changes, it is so already"),
        _ => {}
    }
    let pods: Vec<&str> = lots.iter().filter(|a| a.key != c.room() && t.lots.iter().all(|b| b.key != a.key)).map(|a| a.key.as_str()).collect();
    if !pods.is_empty() {
        out.push_str(&format!("; its pods go with it: {}", pods.join(", ")));
    }
    if !t.dropped.is_empty() {
        out.push_str(&format!("; the edits that named them go too: {}", t.dropped.join("; ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;

    #[test]
    fn every_lot_is_listed_with_its_box_its_heights_and_its_pods() {
        let placed = Doc::new(Recipe::new(40), true).lots().unwrap();
        let rows = rows(&placed.lots);
        assert_eq!(rows.len(), 29);
        let row = |key: &str| rows.iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no row for {key}"));
        let lot = |key: &str| placed.lots.iter().find(|a| a.key == key).unwrap();
        // a hall names its pods, a pod its hall, a plain room neither
        let pods: Vec<&str> = placed.lots.iter().filter(|a| a.host == "vast").map(|a| a.key.as_str()).collect();
        assert!(pods.contains(&"small8"));
        assert_eq!(row("vast").pods, format!("pods {}", pods.join(" ")));
        assert_eq!((row("small8").pods.as_str(), row("medium1").pods.as_str(), row("small8").kind.as_str()), ("pod of vast", "", "small"));
        let a = lot("medium1");
        assert_eq!(row("medium1").place, format!("{},{} to {},{} ({} x {})", a.x0, a.y0, a.x1, a.y1, a.x1 - a.x0, a.y1 - a.y0));
        assert_eq!(row("medium1").heights, format!("{} to {}", a.floor, a.top));
        // (a scattered top is shown on the ceiling line the room is built to)
        assert_eq!((lot("medium5").top, row("medium5").heights.as_str()), (788, "276 to 787"));
        // the text: a line each under a heading that gives the site, the columns lined up
        let said = text(&placed.lots, placed.site, false);
        assert_eq!(said.lines().count(), 30);
        assert_eq!(
            said.lines().next().unwrap(),
            "lots (29), on a site of 80 x 60 cells: key, kind, low corner to high corner (cells wide x deep), floor to top"
        );
        assert!(said.contains("\n  medium1   medium    50,29 to 57,34 (7 x 5)    0 to 512\n"), "{said}");
        assert!(said.contains("\n  small8    small     44,29 to 48,32 (4 x 3)    551 to 787    pod of vast\n"), "{said}");
        assert!(said.contains("\n  vast      vast      37,20 to 47,38 (10 x 18)  0 to 787      pods small8 tiny2\n"), "{said}");
        assert!(said.lines().all(|l| l == l.trim_end()));
        // the lots a change being tried would leave are headed as such
        assert!(text(&placed.lots[..2], placed.site, true).starts_with("lots (2) as they would lie, on a site of 80 x 60 cells: "));
    }

    #[test]
    fn a_change_is_said_in_the_edit_lists_words() {
        let mut d = Doc::new(Recipe::new(40), true);
        let lots = d.lots().unwrap().lots;
        let a = lots.iter().find(|a| a.key == "medium1").unwrap().clone();
        // a side pulled in: the resize it added
        let pull = Change::Rect { room: "medium1".into(), rect: (a.x0, a.y0, a.x1 - 1, a.y1) };
        let (before, t) = (d.recipe.clone(), d.change(&pull).unwrap());
        assert_eq!(said(&pull, &before, &lots, &t), format!("resize medium1 to {} x {}", a.x1 - a.x0 - 1, a.y1 - a.y0));
        // pulled back: no edit is needed, and the resize goes
        let back = Change::Rect { room: "medium1".into(), rect: a.rect() };
        let (before, now) = (d.recipe.clone(), d.lots().unwrap().lots);
        let t = d.change(&back).unwrap();
        assert_eq!(
            said(&back, &before, &now, &t),
            format!(
                "medium1's box to {}, {} to {}, {}: the recipe needs no edit for it, and loses: resize medium1 to {} x {}",
                a.x0,
                a.y0,
                a.x1,
                a.y1,
                a.x1 - a.x0 - 1,
                a.y1 - a.y0
            )
        );
        // a move to where it is
        let same = Change::Move { room: "medium1".into(), to: (a.x0, a.y0) };
        let t = d.try_change(&same).unwrap();
        assert_eq!(said(&same, &d.recipe, &lots, &t), format!("move medium1 to {}, {}: nothing changes, it is so already", a.x0, a.y0));
        // a hall deleted: its pods go with it, and the edits that named any of them
        d.join("tower", 551, "vast", &[]).unwrap();
        let gone = Change::Delete { room: "vast".into() };
        let t = d.try_change(&gone).unwrap();
        let pods: Vec<&str> = lots.iter().filter(|a| a.host == "vast").map(|a| a.key.as_str()).collect();
        assert_eq!(
            said(&gone, &d.recipe, &lots, &t),
            format!("delete vast; its pods go with it: {}; the edits that named them go too: join tower on 551 to vast", pods.join(", "))
        );
        // what is asked, for each kind of change
        assert_eq!(asked(&Change::Pin { room: "tower".into(), floor: 276 }), "pin tower's floor at 276");
        let add = Change::Add { key: "room1".into(), kind: "medium".into(), rect: (1, 2, 7, 6), floor: 0, top: 236 };
        assert_eq!(asked(&add), "add room1 (medium) at 1, 2 to 7, 6, from 0 to 236");
    }
}
