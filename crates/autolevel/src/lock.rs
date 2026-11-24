//! A room held as it is while the rest is re-rolled (the editing roadmap's item 8; new here, the
//! Python has none). The plan is in `docs/AUTOLEVEL.md`, "Lock, copy, paste, mirror and presets".
//!
//! A room's inside is drawn from its own streams and laid to its doors, so holding it is holding
//! its doors: the dead-end pass makes none in a locked room and takes none away
//! (`Options::locked`). The corridors the pass made to it before it was locked are written into
//! the recipe as `join` edits of its own (through the doorways the route took), so that locking
//! changes nothing: on seed 40 the diff of a level against itself locked is empty.
//!
//! ```text
//! let r = lock(&recipe, &level, "vast")?;   // + join edits for the pass's corridors to vast, + lock
//! let r = rest(&r, 3);                     // every other room furnished from its third stream
//! ```

use crate::build::Fix;
use crate::level::Level;
use crate::recipe::{Doorway, Edit, Recipe};

/// The rooms a recipe locks, in its order.
pub fn locked(r: &Recipe) -> Vec<String> {
    r.edits.iter().filter_map(|e| if let Edit::Lock { room } = e { Some(room.clone()) } else { None }).collect()
}

/// Is `room` locked?
pub fn is_locked(r: &Recipe, room: &str) -> bool {
    r.edits.iter().any(|e| matches!(e, Edit::Lock { room: k } if k == room))
}

/// The rest's re-roll: which other stream every room not locked is furnished from (0: its own).
pub fn rest_of(r: &Recipe) -> u32 {
    r.edits.iter().rev().find_map(|e| if let Edit::Rest { n } = e { Some(*n) } else { None }).unwrap_or(0)
}

/// Why `edit` can't be made on `r` (it names a locked room), or None. What `Recipe::generate`
/// would refuse, asked before the edit is made: an editor that puts an edit in an earlier one's
/// place (`choices::set`) would get it past the lock otherwise.
pub fn refusal(r: &Recipe, edit: &Edit) -> Option<String> {
    if let Edit::Lock { .. } = edit {
        return None;
    }
    let room = edit.rooms().into_iter().find(|room| is_locked(r, room))?;
    Some(format!("{}: {room} is locked: unlock it to change it", edit.name()))
}

/// Err if going from `old` to `new` changes what the recipe says of a room locked in `old`: an
/// edit that names it made, changed or taken out, wherever in the list (an editor that puts a
/// choice in an earlier one's place would get it past the lock otherwise), its `lock` aside. So
/// unlocking (the `lock` alone taken out) and locking pass, and deleting a locked room (which
/// takes its lock with it) does not. The one guard every editor's change goes through.
pub fn guard(old: &Recipe, new: &Recipe) -> Result<(), String> {
    for room in &locked(old) {
        let of =
            |r: &Recipe| -> Vec<Edit> { r.edits.iter().filter(|e| !matches!(e, Edit::Lock { .. }) && e.rooms().contains(&room.as_str())).cloned().collect() };
        if of(old) != of(new) {
            return Err(format!("{room} is locked: unlock it to change it"));
        }
    }
    Ok(())
}

/// `r` with `room` locked: each corridor the dead-end pass made to or from it in `lv` (the level
/// `r` makes) written as a `join` of the recipe's own, through the doorways it took, each cut the
/// pass made to its decks as a `cut`, then the `lock`. Locked already: `r` as it is. Err for a
/// room that is not in the level, or a pod (it is held with its hall).
pub fn lock(r: &Recipe, lv: &Level, room: &str) -> Result<Recipe, String> {
    let s = lv.spaces().iter().find(|s| s.key == room).ok_or_else(|| format!("lock: no room {room}"))?;
    if !s.host.is_empty() {
        return Err(format!("lock: {room} is a pod of {}: lock its hall", s.host));
    }
    if is_locked(r, room) {
        return Ok(r.clone());
    }
    let mut out = r.clone();
    for f in &lv.fixes {
        match f {
            Fix::Joined { room: a, storey, to, by, kind, .. } if a == room || to == room => {
                let route = lv.net.routes().iter().find(|q| q.pieces.iter().any(|p| p.key == *by));
                let Some(route) = route else { continue };
                let (ea, eb) = if route.a.room == *a { (&route.a, &route.b) } else { (&route.b, &route.a) };
                out.edits.push(Edit::Join {
                    room: a.clone(),
                    storey: *storey,
                    to: to.clone(),
                    on: vec![eb.level],
                    door: Some(Doorway { side: ea.side, at: ea.a }),
                    to_door: Some(Doorway { side: eb.side, at: eb.a }),
                    via: Some(if *kind == "vent" { "vent" } else { "corridor" }.to_string()),
                });
            }
            Fix::Cut { room: a, above } if a == room => out.edits.push(Edit::Cut { room: a.clone(), above: *above }),
            _ => {}
        }
    }
    out.edits.push(Edit::Lock { room: room.to_string() });
    Ok(out)
}

/// `r` with `room` unlocked: its `lock` taken out. The joins locking wrote stay (they are the
/// level's corridors either way).
pub fn unlock(r: &Recipe, room: &str) -> Recipe {
    let mut out = r.clone();
    out.edits.retain(|e| !matches!(e, Edit::Lock { room: k } if k == room));
    out
}

/// `r` with the rest re-rolled from stream `n` (0: the seed's own again): one `rest` edit, in the
/// earlier one's place, or none.
pub fn rest(r: &Recipe, n: u32) -> Recipe {
    let mut out = r.clone();
    let at = out.edits.iter().position(|e| matches!(e, Edit::Rest { .. }));
    out.edits.retain(|e| !matches!(e, Edit::Rest { .. }));
    if n != 0 {
        out.edits.insert(at.unwrap_or(out.edits.len()), Edit::Rest { n });
    }
    out
}
