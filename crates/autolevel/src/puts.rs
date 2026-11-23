//! Single things by place: the editing roadmap's item 6 (docs/AUTOLEVEL.md, "Single things by
//! place"). New here; the Python has none.
//!
//! A window, a pillar or crate, and a drop are put by hand with a recipe edit that names where it
//! is (`window`, `piece`, `drop`), never an index (indices shift with every remake). Each is put
//! after the generator has drawn its own, so it moves no draw, and one that does not fit is left
//! out with why (`Level::unmet`). This is what an editor asks of them: what a click on a cell with
//! a tool in hand puts (`click`), the recipe with it put and kept short (`set`), a place's edits
//! taken out again (`clear`), and what the recipe put, with whether each took (`listed`).

use crate::cells::Cell;
use crate::level::Level;
use crate::recipe::{put_of, Edit, Recipe};
use crate::space::{Space, DIRS};
use crate::windows::{places, Place};

/// What a click puts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// A window made "glass", "rail" or "none".
    Window(&'static str),
    /// A "pillar" or "crate" stood, or "none": what stands there taken out.
    Piece(&'static str),
    /// A drop through a floor, or the one there taken out.
    Drop,
}

impl Tool {
    /// The tools, in the order an editor lists them, with a name for each.
    pub const ALL: [(Tool, &'static str); 7] = [
        (Tool::Window("glass"), "glass"),
        (Tool::Window("rail"), "rail"),
        (Tool::Window("none"), "no window"),
        (Tool::Piece("pillar"), "pillar"),
        (Tool::Piece("crate"), "crate"),
        (Tool::Piece("none"), "take out"),
        (Tool::Drop, "drop"),
    ];

    /// What a click with it does, for the line under the plan.
    pub fn hint(&self) -> &'static str {
        match self {
            Tool::Window(_) => "click a room's wall where a corridor runs along it (the places are dotted)",
            Tool::Piece("none") => "click a pillar or crate to take it out; drag one to move it",
            Tool::Piece(_) => "click a cell of a room's floor to stand it there (its low corner); drag a piece to move it",
            Tool::Drop => "click a cell of a room's floor over another room for a drop; click a drop to take it out",
        }
    }
}

/// The room a cell is in on storey `z` (pods aside): the one whose box holds it and whose floor
/// is `z`, else the one of those `z` is a storey of, else (no storey given) the highest floor.
pub fn room_at(lv: &Level, (x, y): Cell, z: Option<i32>) -> Option<&Space> {
    let holds: Vec<&Space> = lv.spaces().iter().filter(|s| s.host.is_empty() && s.x0 <= x && x < s.x1 && s.y0 <= y && y < s.y1).collect();
    match z {
        Some(z) => holds.iter().find(|s| s.floor == z).or_else(|| holds.iter().find(|s| s.storeys().contains(&z))).copied(),
        None => holds.into_iter().max_by_key(|s| s.floor),
    }
}

/// The window place a cell names on storey `z` (any storey: None): one whose wall holds it, or
/// the cell either side of that wall (the room's own edge, the corridor's).
pub fn window_at(lv: &Level, (x, y): Cell, z: Option<i32>) -> Option<Place> {
    let near = |p: &Place| {
        let (dx, dy) = DIRS[p.d];
        p.wall.iter().any(|&(wx, wy)| [(wx, wy), (wx + dx, wy + dy), (wx - dx, wy - dy)].contains(&(x, y)))
    };
    places(lv).into_iter().filter(|p| z.is_none_or(|z| p.z == z)).find(near)
}

/// The edit a click on the cell `at` (on storey `z`, if the view has one) with `tool` makes, or
/// why there is none. A window is named by the first cell of its wall, a piece taken out by its
/// low corner, so the same thing is always named the same way.
pub fn click(lv: &Level, tool: Tool, at: Cell, z: Option<i32>) -> Result<Edit, String> {
    match tool {
        Tool::Window(kind) => {
            let p = window_at(lv, at, z).ok_or("no window fits there: click a room's wall where a corridor runs along it a cell away")?;
            Ok(Edit::Window { room: p.hall, storey: p.z, at: p.wall[0], kind: kind.to_string() })
        }
        Tool::Piece(what) => {
            let s = room_at(lv, at, z).ok_or("no room there")?;
            let room = s.key.clone();
            if what == "none" {
                let it = lv.interior(&room);
                let c = it.piece_at(at).map(|i| it.cover[i]).ok_or_else(|| format!("nothing stands in {room} there"))?;
                return Ok(Edit::Piece { room, at: (c.0.div_euclid(2), c.1.div_euclid(2)), what: Some("none".to_string()), size: None, to: None });
            }
            Ok(Edit::Piece { room, at, what: Some(what.to_string()), size: None, to: None })
        }
        Tool::Drop => {
            let s = room_at(lv, at, z).ok_or("no room there")?;
            let room = s.key.clone();
            let here = lv.drops.iter().find(|d| d.0 == room && d.2 .0 <= at.0 && at.0 < d.2 .2 && d.2 .1 <= at.1 && at.1 < d.2 .3);
            Ok(Edit::Drop { room, at: if here.is_some() { None } else { Some(at) } })
        }
    }
}

/// The edit a drag of the piece over the cell `from` to the cell `to` makes: the piece moved by
/// as many cells, named by its low corner. Err if nothing stands there.
pub fn drag(lv: &Level, from: Cell, to: Cell, z: Option<i32>) -> Result<Edit, String> {
    let s = room_at(lv, from, z).ok_or("no room there")?;
    let it = lv.interior(&s.key);
    let c = it.piece_at(from).map(|i| it.cover[i]).ok_or_else(|| format!("nothing stands in {} there", s.key))?;
    let low = (c.0.div_euclid(2), c.1.div_euclid(2));
    let moved = (low.0 + to.0 - from.0, low.1 + to.1 - from.1);
    Ok(Edit::Piece { room: s.key.clone(), at: low, what: None, size: None, to: Some(moved) })
}

/// A piece's place in an edit: where it is named, and where it ends up (None: taken out).
fn piece(e: &Edit) -> Option<(&str, Cell, Option<Cell>)> {
    match e {
        Edit::Piece { room, at, to: Some(to), .. } => Some((room, *at, Some(*to))),
        Edit::Piece { room, at, what: Some(w), .. } if w == "none" => Some((room, *at, None)),
        Edit::Piece { room, at, .. } => Some((room, *at, Some(*at))),
        _ => None,
    }
}

/// The recipe with `e` (a `window`, `piece` or `drop` edit) put, kept short:
/// - a window takes the place of an earlier edit of the same window (its room, storey and first
///   wall cell);
/// - a drop takes the place of the room's earlier drop edit (a room has one drop at most);
/// - a piece stood where one was stood by hand takes that one's place; one stood by hand and
///   then moved is one edit, stood where it went; moved twice, one move; moved back, none; one
///   stood by hand and then taken out, none.
///
/// Any other edit is just added.
pub fn set(recipe: &Recipe, e: Edit) -> Recipe {
    let mut out = recipe.clone();
    match &e {
        Edit::Window { room, storey, at, .. } => {
            out.edits.retain(|o| !matches!(o, Edit::Window { room: r, storey: z, at: a, .. } if r == room && z == storey && a == at));
            out.edits.push(e);
        }
        Edit::Drop { room, .. } => {
            out.edits.retain(|o| !matches!(o, Edit::Drop { room: r, .. } if r == room));
            out.edits.push(e);
        }
        Edit::Piece { room, at, what, to, size } => {
            // the earlier edit that left a piece where this one names it
            let earlier = out.edits.iter().rposition(|o| piece(o).is_some_and(|(r, _, end)| r == room && end == Some(*at)));
            match (earlier, what.as_deref(), to) {
                (Some(i), _, Some(to)) => match &mut out.edits[i] {
                    // stood by hand, then moved: stood where it went
                    Edit::Piece { at: a, to: None, .. } => *a = *to,
                    // moved, then moved again: one move, or none if it is back
                    Edit::Piece { at: a, to: t @ Some(_), .. } => {
                        if a == to {
                            out.edits.remove(i);
                        } else {
                            *t = Some(*to);
                        }
                    }
                    _ => unreachable!("a piece edit"),
                },
                (Some(i), Some("none"), None) => match out.edits[i].clone() {
                    // stood by hand, then taken out: none
                    Edit::Piece { to: None, .. } => drop(out.edits.remove(i)),
                    // moved, then taken out: taken out where it was
                    Edit::Piece { room, at: a, .. } => out.edits[i] = Edit::Piece { room, at: a, what: Some("none".to_string()), size: None, to: None },
                    _ => unreachable!("a piece edit"),
                },
                (Some(i), Some(_), None) if matches!(&out.edits[i], Edit::Piece { at: a, to: None, .. } if a == at) => {
                    out.edits[i] = Edit::Piece { room: room.clone(), at: *at, what: what.clone(), size: *size, to: None };
                }
                _ => out.edits.push(e),
            }
        }
        _ => out.edits.push(e),
    }
    out
}

/// The recipe with every `window`, `piece` and `drop` edit of `room` that names the cell `at`
/// taken out (a drop's: every one of the room's), so the place is the generator's again.
pub fn clear(recipe: &Recipe, room: &str, at: Cell) -> Recipe {
    let mut out = recipe.clone();
    out.edits.retain(|e| match e {
        Edit::Window { room: r, at: a, .. } => !(r == room && *a == at),
        Edit::Piece { room: r, at: a, to, .. } => !(r == room && (*a == at || *to == Some(at))),
        Edit::Drop { room: r, .. } => r != room,
        _ => true,
    });
    out
}

/// What the recipe put by place, in its order, each with whether the level took it (Err: why
/// not, the level's words).
pub fn listed(recipe: &Recipe, lv: &Level) -> Vec<(Edit, Result<(), String>)> {
    recipe
        .edits
        .iter()
        .filter(|e| matches!(e, Edit::Window { .. } | Edit::Piece { .. } | Edit::Drop { .. }))
        .map(|e| {
            let why = put_of(e).ok().and_then(|p| lv.unmet.iter().find(|(q, _)| *q == p).map(|(_, why)| why.clone()));
            (e.clone(), why.map_or(Ok(()), Err))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Options;

    fn seed40() -> &'static Level {
        static LV: std::sync::OnceLock<Level> = std::sync::OnceLock::new();
        LV.get_or_init(|| Recipe::python(40).generate().unwrap())
    }

    fn piece(room: &str, at: Cell, what: Option<&str>, to: Option<Cell>) -> Edit {
        Edit::Piece { room: room.to_string(), at, what: what.map(str::to_string), size: None, to }
    }

    #[test]
    fn a_click_names_the_thing_by_where_it_is() {
        let lv = seed40();
        // medium1's window (wall (49, 29) to (49, 31) on 276), by its wall, its edge or the corridor
        for at in [(49, 30), (50, 31), (48, 29)] {
            let e = click(lv, Tool::Window("rail"), at, Some(276)).unwrap();
            assert_eq!(e, Edit::Window { room: "medium1".into(), storey: 276, at: (49, 29), kind: "rail".into() });
        }
        assert!(click(lv, Tool::Window("rail"), (49, 30), Some(0)).is_err());
        assert!(click(lv, Tool::Window("glass"), (49, 30), None).is_ok());
        // a piece taken out is named by its low corner, from either of its cells
        for at in [(41, 25), (42, 25)] {
            assert_eq!(click(lv, Tool::Piece("none"), at, Some(0)).unwrap(), piece("vast", (41, 25), Some("none"), None));
        }
        assert_eq!(click(lv, Tool::Piece("none"), (40, 30), Some(0)).unwrap_err(), "nothing stands in vast there");
        assert_eq!(click(lv, Tool::Piece("crate"), (40, 30), Some(0)).unwrap(), piece("vast", (40, 30), Some("crate"), None));
        assert_eq!(drag(lv, (42, 25), (41, 33), Some(0)).unwrap(), piece("vast", (41, 25), None, Some((40, 33))));
        // a drop: on a storey the room over the other is found; on the drop, it is taken out
        assert_eq!(click(lv, Tool::Drop, (42, 30), Some(827)).unwrap(), Edit::Drop { room: "loft2".into(), at: Some((42, 30)) });
        assert_eq!(click(lv, Tool::Drop, (42, 30), None).unwrap(), Edit::Drop { room: "loft2".into(), at: Some((42, 30)) });
        assert_eq!(click(lv, Tool::Drop, (15, 40), Some(0)).unwrap(), Edit::Drop { room: "colossal".into(), at: None });
        assert_eq!(room_at(lv, (42, 30), Some(0)).map(|s| s.key.as_str()), Some("vast"));
        assert_eq!(room_at(lv, (42, 30), Some(276)).map(|s| s.key.as_str()), Some("vast"));
    }

    #[test]
    fn the_recipe_is_kept_short() {
        let r = Recipe { seed: 40, options: Options::python(), edits: Vec::new() };
        let w = |kind: &str| Edit::Window { room: "medium1".into(), storey: 276, at: (49, 29), kind: kind.into() };
        assert_eq!(set(&set(&r, w("rail")), w("none")).edits, vec![w("none")]);
        let d = |at| Edit::Drop { room: "loft2".into(), at };
        assert_eq!(set(&set(&r, d(Some((42, 30)))), d(None)).edits, vec![d(None)]);
        // stood, then moved: stood where it went; then taken out: nothing
        let stood = set(&r, piece("vast", (40, 30), Some("crate"), None));
        let moved = set(&stood, piece("vast", (40, 30), None, Some((40, 32))));
        assert_eq!(moved.edits, vec![piece("vast", (40, 32), Some("crate"), None)]);
        assert!(set(&moved, piece("vast", (40, 32), Some("none"), None)).edits.is_empty());
        // stood again where it was: in its place
        assert_eq!(set(&moved, piece("vast", (40, 32), Some("pillar"), None)).edits, vec![piece("vast", (40, 32), Some("pillar"), None)]);
        // the generator's moved twice: one move; back: none; moved then taken out: out where it was
        let once = set(&r, piece("vast", (41, 25), None, Some((40, 32))));
        let twice = set(&once, piece("vast", (40, 32), None, Some((43, 34))));
        assert_eq!(twice.edits, vec![piece("vast", (41, 25), None, Some((43, 34)))]);
        assert!(set(&twice, piece("vast", (43, 34), None, Some((41, 25)))).edits.is_empty());
        assert_eq!(set(&twice, piece("vast", (43, 34), Some("none"), None)).edits, vec![piece("vast", (41, 25), Some("none"), None)]);
        // a place cleared
        let all = set(&set(&twice, w("rail")), d(Some((42, 30))));
        assert_eq!(clear(&all, "vast", (43, 34)).edits, vec![w("rail"), d(Some((42, 30)))]);
        assert_eq!(clear(&clear(&all, "medium1", (49, 29)), "loft2", (0, 0)).edits, twice.edits);
    }

    #[test]
    fn what_was_put_is_listed_with_whether_it_took() {
        let r = Recipe {
            seed: 40,
            options: Options::python(),
            edits: vec![
                Edit::Brightness { room: "vast".into(), value: 100 },
                piece("vast", (40, 30), Some("crate"), None),
                piece("vast", (41, 25), None, Some((41, 30))),
                Edit::Drop { room: "medium1".into(), at: Some((50, 30)) },
            ],
        };
        let lv = r.generate().unwrap();
        let got: Vec<(String, Result<(), String>)> = listed(&r, &lv).into_iter().map(|(e, took)| (e.describe(), took)).collect();
        assert_eq!(
            got,
            vec![
                ("vast: a crate at 40, 30".to_string(), Ok(())),
                ("vast: the piece at 41, 25 moved to 41, 30".to_string(), Err("it would clip a deck's edge, a ramp or a pod over it".to_string())),
                ("medium1: a drop at 50, 30".to_string(), Err("a drop is a cell in from medium1's walls".to_string())),
            ]
        );
    }
}
