//! Room boxes by hand: what the editor's room tool asks of a recipe. New here (the Python has
//! none); the plan is in `docs/AUTOLEVEL.md`, "Track B", milestone 3.
//!
//! A person drags a room's box on the plan, pulls one of its sides, draws a new one, deletes
//! one, or puts its floor on another storey. Each is a `Change`, and `trial` says what it would
//! come to without making it: the recipe with the change in it (as the recipe's own lot edits:
//! `move`, `resize`, `add`, `delete`, `pin`), the lots as they would then lie, and, if the lots
//! would break a rule, the refusal in plain words (`recipe::Recipe::rooms_on`: the scatter's
//! overlap rule, the site, a spire's walls, a pod's place in its hall and its way in). Nothing
//! is routed or furnished for it, so it can be asked on every move of the pointer.
//!
//! The recipe is kept short. A change that undoes an earlier one takes that edit out again
//! rather than adding to the list: a room moved twice has one `move`, a room dragged back to
//! where the seed put it has none, and a room made by hand and then moved has only its `add`,
//! with the new place in it. That is done by trying, never by rule: an edit is left out only if
//! the lots come out exactly the same without it.
//!
//! What a `Change` settles that the recipe's edits leave open:
//! - `Rect` pulls a room's sides and leaves a hall's pods where they are (the recipe's `move`
//!   takes them along, so each pod is moved back);
//! - `Delete` takes out, with the room, every later edit that names it or one of its pods (a
//!   join to it, its cover), and says which in `Trial::dropped`; a room made by hand goes as if
//!   it had never been added.
//!
//! ```text
//! let base = recipe.scattered()?;                       // once for a seed and its siting
//! let t = boxes::trial(&recipe, &Change::Move { room: "tower".into(), to: (12, 30) }, &base);
//! match &t.refused { None => recipe = t.recipe, Some(why) => show(why) }
//! ```

use crate::cells::Rect;
use crate::place::{Placed, Problem, LEAST};
use crate::recipe::{Edit, Recipe, Stage};
use crate::scatter::{Lot, STOREY};
use crate::storeys::{index, CEILINGS, LEVELS};

/// One change to the room boxes, as a person makes it.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// The room's low corner goes to this cell, its size kept. A hall's pods go with it.
    Move { room: String, to: (i32, i32) },
    /// The room's box becomes this rectangle (x0, y0, x1, y1) in cells. A hall's pods stay
    /// where they are.
    Rect { room: String, rect: Rect },
    /// The room's floor goes to this storey, its height kept. A hall's pods go as many storeys.
    Pin { room: String, floor: i32 },
    /// The room stands from the storey `floor` to the ceiling line `top` (the side view's drag of
    /// a box up or down, or of its top or bottom edge). A hall's pods go as many storeys as its
    /// floor. Made a `pin` where the room keeps its height, else a `height`.
    Height { room: String, floor: i32, top: i32 },
    /// A new room: `kind` one of the scale's, `floor` a storey's and `top` a ceiling line.
    Add { key: String, kind: String, rect: Rect, floor: i32, top: i32 },
    /// The room goes, a hall's pods with it, and every later edit that names any of them.
    Delete { room: String },
}

impl Change {
    /// The room it changes (`Add`: the one it makes).
    pub fn room(&self) -> &str {
        match self {
            Change::Move { room, .. } | Change::Rect { room, .. } | Change::Pin { room, .. } | Change::Height { room, .. } | Change::Delete { room } => room,
            Change::Add { key, .. } => key,
        }
    }
}

/// The part of a box a drag has hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The whole of it: a drag moves the room.
    Body,
    /// A side or a corner: which way it faces along x and along y (-1 west or south, 0 neither,
    /// 1 east or north; not both 0). A drag pulls it.
    Edge(i8, i8),
}

/// The box a drag of `part` by `by` cells leaves of `r`: moved whole, or with the sides the
/// part holds pulled that far, never to under 2 cells.
pub fn dragged(r: Rect, part: Part, by: (i32, i32)) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = r;
    match part {
        Part::Body => return (x0 + by.0, y0 + by.1, x1 + by.0, y1 + by.1),
        Part::Edge(sx, sy) => {
            match sx {
                -1 => x0 = (x0 + by.0).min(x1 - LEAST),
                1 => x1 = (x1 + by.0).max(x0 + LEAST),
                _ => {}
            }
            match sy {
                -1 => y0 = (y0 + by.1).min(y1 - LEAST),
                1 => y1 = (y1 + by.1).max(y0 + LEAST),
                _ => {}
            }
        }
    }
    (x0, y0, x1, y1)
}

/// The change a drag of a room's `part` by `by` cells is. None if it moves nothing.
pub fn drag(lot: &Lot, part: Part, by: (i32, i32)) -> Option<Change> {
    let to = dragged(lot.rect(), part, by);
    if to == lot.rect() {
        return None;
    }
    Some(match part {
        Part::Body => Change::Move { room: lot.key.clone(), to: (to.0, to.1) },
        Part::Edge(..) => Change::Rect { room: lot.key.clone(), rect: to },
    })
}

/// What a change would come to.
#[derive(Clone, Debug, PartialEq)]
pub struct Trial {
    /// The recipe with the change made. (The recipe as it was, if the change itself can't be
    /// read: it names no room, a key that is taken, a kind the scale has not.)
    pub recipe: Recipe,
    /// The lots as they would then lie, whether or not they keep the rules.
    pub lots: Vec<Lot>,
    /// Why it can't be made, in plain words. None: the lots keep every rule held to before
    /// anything is routed (a corridor may still not fit: that is found by making the level).
    pub refused: Option<String>,
    /// Edits a `Delete` takes out with the room, as the edit list words them.
    pub dropped: Vec<String>,
    /// Something to look at afterwards: the recipe takes a corridor out by its key, and keys
    /// are dealt out again when the rooms change.
    pub warn: Option<String>,
}

impl Trial {
    /// Does it change anything (and may it)?
    pub fn changes(&self, from: &Recipe) -> bool {
        self.refused.is_none() && self.recipe != *from
    }
}

/// The lots a recipe's edits leave of the seed's own (`base`), not held to any rule.
fn lots_of(r: &Recipe, base: &Placed) -> Result<Placed, String> {
    r.lots_on(base.clone())
}

/// Do two recipes leave the same lots? (And the same of the seed's own taken out: a room made
/// by hand and deleted again is no matter.)
fn same(a: &Recipe, b: &Recipe, base: &Placed) -> bool {
    let gone = |p: &Placed| p.gone.iter().filter(|(k, _)| base.lots.iter().any(|l| l.key == *k)).cloned().collect::<Vec<_>>();
    match (lots_of(a, base), lots_of(b, base)) {
        (Ok(p), Ok(q)) => p.lots == q.lots && gone(&p) == gone(&q),
        _ => false,
    }
}

/// Is this a lot edit that names `room` (and only moves, sizes or pins it, with `shifts`)?
fn names(e: &Edit, room: &str, shifts: bool) -> bool {
    let kind = if shifts { matches!(e, Edit::Move { .. } | Edit::Resize { .. } | Edit::Pin { .. } | Edit::Height { .. }) } else { e.stage() == Stage::Lots };
    kind && e.rooms() == [room]
}

/// The recipe with what the last change made needless left out: for each of `rooms`, a made
/// room's later moves folded into its `add`, and then every lot edit naming one of them without
/// which the lots are the same. The level it makes is the same.
fn tidy(mut r: Recipe, rooms: &[String], base: &Placed) -> Recipe {
    for key in rooms {
        let Some(at) = r.edits.iter().rposition(|e| matches!(e, Edit::Add { key: k, .. } if k == key)) else { continue };
        let Ok(now) = lots_of(&r, base) else { continue };
        let (Some(lot), Edit::Add { kind, .. }) = (now.lots.iter().find(|a| a.key == *key), &r.edits[at]) else { continue };
        let mut t = r.clone();
        t.edits[at] = Edit::Add { key: key.clone(), kind: kind.clone(), rect: lot.rect(), floor: lot.floor, top: lot.top };
        let mut n = 0;
        t.edits.retain(|e| {
            n += 1;
            n - 1 <= at || !names(e, key, true)
        });
        if same(&t, &r, base) {
            r = t;
        }
    }
    loop {
        let before = r.edits.len();
        for n in (0..r.edits.len()).rev() {
            if !rooms.iter().any(|key| names(&r.edits[n], key, false)) {
                continue;
            }
            let mut t = r.clone();
            t.edits.remove(n);
            if same(&t, &r, base) {
                r = t;
            }
        }
        if r.edits.len() == before {
            return r;
        }
    }
}

/// The recipe with `change` made, and the edits a `Delete` took out with the room. The lots are
/// not held to any rule here (`trial` does that). Err if the change can't be read: it names no
/// room, or a box under 2 x 2.
pub fn apply(recipe: &Recipe, change: &Change, base: &Placed) -> Result<(Recipe, Vec<String>), String> {
    let cur = lots_of(recipe, base)?;
    let find = |what: &str, room: &str| match cur.lots.iter().find(|a| a.key == room) {
        Some(a) => Ok(a),
        None if cur.gone.iter().any(|(k, _)| k == room) => Err(format!("{what}: {room} was deleted")),
        None => Err(format!("{what}: no room {room}")),
    };
    let pods = |room: &str| cur.lots.iter().filter(|a| a.host == room).collect::<Vec<_>>();
    let mut out = recipe.clone();
    let mut dropped = Vec::new();
    let mut rooms = vec![change.room().to_string()];
    match change {
        Change::Move { room, to } => {
            find("move", room)?;
            out.edits.push(Edit::Move { room: room.clone(), to: *to });
        }
        Change::Rect { room, rect } => {
            let a = find("resize", room)?;
            let size = (rect.2 - rect.0, rect.3 - rect.1);
            if size.0 < LEAST || size.1 < LEAST {
                return Err(format!("resize: a room is at least {LEAST} x {LEAST} cells, not {} x {}", size.0, size.1));
            }
            if (rect.0, rect.1) != (a.x0, a.y0) {
                out.edits.push(Edit::Move { room: room.clone(), to: (rect.0, rect.1) });
                // (the recipe's move takes a hall's pods along: put each back)
                for pod in pods(room) {
                    out.edits.push(Edit::Move { room: pod.key.clone(), to: (pod.x0, pod.y0) });
                    rooms.push(pod.key.clone());
                }
            }
            if size != (a.x1 - a.x0, a.y1 - a.y0) {
                out.edits.push(Edit::Resize { room: room.clone(), size });
            }
        }
        Change::Pin { room, floor } => {
            find("pin", room)?;
            out.edits.push(Edit::Pin { room: room.clone(), floor: *floor });
        }
        Change::Height { room, floor, top } => {
            let a = find("height", room)?;
            // (a room that keeps its height is pinned: the edit says what was done; a top is
            // the scatter's spelling of a ceiling line, as `add` keeps it)
            let top = if *top == CEILINGS[0] { LEVELS[0] + STOREY } else { *top };
            if top - floor == a.top - a.floor {
                out.edits.push(Edit::Pin { room: room.clone(), floor: *floor });
            } else {
                out.edits.push(Edit::Height { room: room.clone(), floor: *floor, top });
            }
        }
        Change::Add { key, kind, rect, floor, top } => {
            out.edits.push(Edit::Add { key: key.clone(), kind: kind.clone(), rect: *rect, floor: *floor, top: *top });
        }
        Change::Delete { room } => {
            find("delete", room)?;
            rooms.extend(pods(room).into_iter().map(|pod| pod.key.clone()));
            // what is done later to a room that goes, goes with it
            out.edits.retain(|e| {
                let goes = e.stage() != Stage::Lots && e.rooms().iter().any(|r| rooms.iter().any(|g| g == r));
                if goes {
                    dropped.push(e.describe());
                }
                !goes
            });
            // a room made by hand, with no pod in it: as if it had never been added
            let mut never = out.clone();
            never.edits.retain(|e| !names(e, room, false));
            out.edits.push(Edit::Delete { room: room.clone() });
            if rooms.len() == 1 && base.lots.iter().all(|a| a.key != *room) && same(&never, &out, base) {
                out = never;
            }
        }
    }
    Ok((tidy(out, &rooms, base), dropped))
}

/// What `change` would come to on `recipe`, whose seed's own lots are `base`
/// (`Recipe::scattered`, for the recipe's seed and siting). Nothing is changed.
pub fn trial(recipe: &Recipe, change: &Change, base: &Placed) -> Trial {
    let now = || lots_of(recipe, base).map_or(Vec::new(), |p| p.lots);
    let (new, dropped) = match apply(recipe, change, base) {
        Ok(done) => done,
        Err(why) => return Trial { recipe: recipe.clone(), lots: now(), refused: Some(why), dropped: Vec::new(), warn: None },
    };
    let lots = match lots_of(&new, base) {
        Ok(p) => p.lots,
        Err(why) => return Trial { recipe: recipe.clone(), lots: now(), refused: Some(why), dropped: Vec::new(), warn: None },
    };
    let refused = new.rooms_on(base.clone()).err();
    let keys: Vec<&str> = new.edits.iter().filter_map(|e| if let Edit::Remove { route } = e { Some(route.as_str()) } else { None }).collect();
    let warn = (!keys.is_empty() && new != *recipe).then(|| {
        format!(
            "the recipe takes out {} by {}, and corridors' keys are dealt out again when rooms change: see that it is still the one meant",
            keys.join(", "),
            if keys.len() == 1 { "its key" } else { "their keys" }
        )
    });
    Trial { recipe: new, lots, refused, dropped, warn }
}

/// A key for a room made by hand that no room has or had: "room1", "room2", ...
pub fn new_key(p: &Placed) -> String {
    let taken = |k: &str| p.lots.iter().any(|a| a.key == k) || p.gone.iter().any(|(g, _)| g == k);
    (1..).map(|n| format!("room{n}")).find(|k| !taken(k)).expect("there is always another number")
}

/// The kinds a made room may be, at the recipe's scale.
pub fn kinds(p: &Placed) -> Vec<&'static str> {
    p.kinds().iter().map(|k| k.name).collect()
}

/// The top a new room of `kind` on `floor` is drawn with unless another is chosen: the lowest
/// it keeps the rule with: two storeys for a spire (its ramps climb through two storeys or more),
/// one for every other kind. New here (the user's call, 2026-11-24).
pub fn default_top(kind: &str, floor: i32) -> i32 {
    let tops = tops(floor);
    let storeys = if kind == "spire" { 2 } else { 1 };
    tops[(storeys - 1).min(tops.len() - 1)]
}

/// The ceiling lines a room with its floor on `floor` may reach to, lowest first.
pub fn tops(floor: i32) -> Vec<i32> {
    CEILINGS[index(floor)..].to_vec()
}

/// Every problem with the recipe's room boxes, as placed on `base` (the seed's own lots): each
/// way the lots break the scatter's rule (`Placed::problems`), each with its rooms; else what
/// `Recipe::rooms_on` still refuses (a pod with no way in, an edit naming a deleted room), with
/// the rooms its words name. None: the boxes are good (the level may still fail later, routing
/// or furnishing). What a sketch, kept while it breaks a rule, draws in red. New here.
pub fn problems(recipe: &Recipe, base: &Placed) -> Vec<Problem> {
    let placed = match lots_of(recipe, base) {
        Ok(p) => p,
        Err(words) => return vec![Problem { rooms: Vec::new(), words }],
    };
    let found = placed.problems(recipe.options.band());
    if !found.is_empty() {
        return found;
    }
    match recipe.rooms_on(base.clone()) {
        Ok(_) => Vec::new(),
        Err(words) => {
            let named = |k: &str| words.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).any(|w| w == k);
            let rooms = placed.lots.iter().filter(|a| named(&a.key)).map(|a| a.key.clone()).collect();
            vec![Problem { rooms, words }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lot<'a>(lots: &'a [Lot], key: &str) -> &'a Lot {
        lots.iter().find(|a| a.key == key).unwrap_or_else(|| panic!("no lot {key}"))
    }

    /// A free place for a `w` x `d` box on the site, clear of every lot by two cells.
    fn free(p: &Placed, w: i32, d: i32) -> Rect {
        for y in 0..p.site.1 - d {
            for x in 0..p.site.0 - w {
                let r = (x, y, x + w, y + d);
                if p.lots.iter().all(|a| a.x1 + 2 <= r.0 || r.2 + 2 <= a.x0 || a.y1 + 2 <= r.1 || r.3 + 2 <= a.y0) {
                    return r;
                }
            }
        }
        panic!("no free place for {w} x {d}");
    }

    #[test]
    fn a_drag_moves_a_box_or_pulls_its_sides() {
        let r = (10, 20, 16, 25);
        assert_eq!(dragged(r, Part::Body, (3, -2)), (13, 18, 19, 23));
        assert_eq!(dragged(r, Part::Edge(1, 0), (3, 9)), (10, 20, 19, 25));
        assert_eq!(dragged(r, Part::Edge(-1, 1), (-2, 4)), (8, 20, 16, 29));
        assert_eq!(dragged(r, Part::Edge(0, -1), (5, 1)), (10, 21, 16, 25));
        // never under two cells: the side stops
        assert_eq!(dragged(r, Part::Edge(1, 0), (-9, 0)), (10, 20, 12, 25));
        assert_eq!(dragged(r, Part::Edge(-1, -1), (9, 9)), (14, 23, 16, 25));
        let a = Lot { key: "a".into(), kind: "medium", x0: 10, y0: 20, x1: 16, y1: 25, floor: 0, top: 236, host: String::new() };
        assert_eq!(drag(&a, Part::Body, (0, 0)), None);
        assert_eq!(drag(&a, Part::Body, (1, 0)), Some(Change::Move { room: "a".into(), to: (11, 20) }));
        assert_eq!(drag(&a, Part::Edge(1, 1), (1, 1)), Some(Change::Rect { room: "a".into(), rect: (10, 20, 17, 26) }));
        assert_eq!(drag(&a, Part::Edge(1, 0), (0, 5)), None);
    }

    #[test]
    fn a_room_moved_twice_has_one_move_and_moved_back_has_none() {
        let recipe = Recipe::new(40);
        let base = recipe.scattered().unwrap();
        let a = lot(&base.lots, "medium1").clone();
        let spot = free(&base, a.x1 - a.x0, a.y1 - a.y0);
        let t = trial(&recipe, &Change::Move { room: "medium1".into(), to: (spot.0, spot.1) }, &base);
        assert_eq!((t.refused.clone(), t.recipe.edits.clone()), (None, vec![Edit::Move { room: "medium1".into(), to: (spot.0, spot.1) }]));
        assert_eq!(lot(&t.lots, "medium1").rect(), spot);
        assert!(t.changes(&recipe) && t.dropped.is_empty() && t.warn.is_none());
        // again, a cell on: still one move
        let t2 = trial(&t.recipe, &Change::Move { room: "medium1".into(), to: (spot.0 + 1, spot.1) }, &base);
        assert_eq!(t2.recipe.edits, [Edit::Move { room: "medium1".into(), to: (spot.0 + 1, spot.1) }]);
        // back where the seed put it: the seed's own recipe again
        let t3 = trial(&t2.recipe, &Change::Move { room: "medium1".into(), to: (a.x0, a.y0) }, &base);
        assert_eq!((t3.refused.clone(), t3.recipe.clone()), (None, recipe.clone()));
        assert!(!t3.changes(&t3.recipe) && t3.changes(&t2.recipe));
        // a move to where it is changes nothing
        let t4 = trial(&recipe, &Change::Move { room: "medium1".into(), to: (a.x0, a.y0) }, &base);
        assert!(t4.recipe == recipe && !t4.changes(&recipe));
    }

    #[test]
    fn a_move_onto_another_room_is_refused_in_plain_words_and_the_lots_shown() {
        let recipe = Recipe::new(40);
        let base = recipe.scattered().unwrap();
        let (a, b) = (lot(&base.lots, "medium1").clone(), lot(&base.lots, "medium2").clone());
        let t = trial(&recipe, &Change::Move { room: "medium1".into(), to: (b.x0, b.y0) }, &base);
        // (the refusal names the first pair that breaks the rule: medium2, or a room beside it)
        let why = t.refused.clone().expect("it lands on medium2");
        assert!(why.starts_with("medium1 and ") && why.contains("rooms keep a cell between them") && why.contains("medium1 is at"), "{why}");
        // the recipe it would be and the lots as they would lie are there to show
        assert_eq!(t.recipe.edits.len(), 1);
        assert_eq!((lot(&t.lots, "medium1").x0, lot(&t.lots, "medium1").y0), (b.x0, b.y0));
        assert!(!t.changes(&recipe));
        // every problem at once, each with its rooms; the first is the trial's refusal
        let both = Recipe {
            edits: vec![Edit::Move { room: "medium1".into(), to: (b.x0, b.y0) }, Edit::Move { room: "medium3".into(), to: (-3, 0) }],
            ..recipe.clone()
        };
        let found = problems(&both, &base);
        assert!(found.len() >= 2, "{found:?}");
        assert_eq!(Some(found[0].words.clone()), both.rooms_on(base.clone()).err());
        assert!(found.iter().any(|p| p.rooms == ["medium3"] && p.words.starts_with("medium3 is off the site")), "{found:?}");
        assert!(found.iter().any(|p| p.rooms.contains(&"medium1".to_string()) && p.words.contains("rooms keep a cell")), "{found:?}");
        assert!(problems(&recipe, &base).is_empty());
        // off the site
        let t = trial(&recipe, &Change::Move { room: "medium1".into(), to: (-3, a.y0) }, &base);
        assert!(t.refused.as_deref().is_some_and(|w| w.contains("medium1 is off the site")), "{:?}", t.refused);
        // what can't be read at all leaves the recipe as it was
        let t = trial(&recipe, &Change::Move { room: "attic".into(), to: (0, 0) }, &base);
        assert_eq!((t.refused.as_deref(), &t.recipe), (Some("move: no room attic"), &recipe));
        let t = trial(&recipe, &Change::Rect { room: "medium1".into(), rect: (a.x0, a.y0, a.x0 + 1, a.y1) }, &base);
        assert_eq!(t.refused.as_deref(), Some(format!("resize: a room is at least 2 x 2 cells, not 1 x {}", a.y1 - a.y0).as_str()));
        let t = trial(&recipe, &Change::Pin { room: "medium1".into(), floor: 100 }, &base);
        assert!(t.refused.as_deref().is_some_and(|w| w.starts_with("pin: medium1's floor would be at 100")), "{:?}", t.refused);
    }

    #[test]
    fn a_side_pulled_leaves_a_halls_pods_where_they_are() {
        let recipe = Recipe::new(40);
        let base = recipe.scattered().unwrap();
        let hall = lot(&base.lots, "vast").clone();
        let pods: Vec<Lot> = base.lots.iter().filter(|a| a.host == "vast").cloned().collect();
        assert!(!pods.is_empty(), "seed 40's vast hall has pods");
        // the east and north sides: a resize alone
        let t = trial(&recipe, &Change::Rect { room: "vast".into(), rect: (hall.x0, hall.y0, hall.x1 - 1, hall.y1) }, &base);
        assert_eq!(t.recipe.edits, [Edit::Resize { room: "vast".into(), size: (hall.x1 - hall.x0 - 1, hall.y1 - hall.y0) }]);
        // the west side: the hall moves and is resized, and each pod is put back
        let rect = (hall.x0 + 1, hall.y0, hall.x1, hall.y1);
        let t = trial(&recipe, &Change::Rect { room: "vast".into(), rect }, &base);
        assert_eq!(lot(&t.lots, "vast").rect(), rect);
        for pod in &pods {
            assert_eq!(lot(&t.lots, &pod.key).rect(), pod.rect(), "{} stays", pod.key);
        }
        assert_eq!(t.recipe.edits.len(), 2 + pods.len());
        // a move takes them along
        let m = trial(&recipe, &Change::Move { room: "vast".into(), to: (hall.x0 + 1, hall.y0) }, &base);
        for pod in &pods {
            assert_eq!(lot(&m.lots, &pod.key).x0, pod.x0 + 1, "{} goes with its hall", pod.key);
        }
        // pulled back: nothing left of it
        let back = trial(&t.recipe, &Change::Rect { room: "vast".into(), rect: hall.rect() }, &base);
        assert_eq!(back.recipe, recipe);
    }

    #[test]
    fn a_room_is_made_moved_pinned_and_deleted_and_the_recipe_stays_short() {
        let recipe = Recipe::new(40);
        let base = recipe.scattered().unwrap();
        assert_eq!(new_key(&base), "room1");
        assert_eq!(kinds(&base), ["colossal", "spire", "vast", "tower", "long", "basin", "medium", "loft", "small", "tiny", "atrium", "base"]);
        assert_eq!(tops(551), [787, 1063, 1338, 1614, 1890, 2165, 2441, 2716, 2992, 3267, 3543, 3819, 4094, 4370]);
        let spot = free(&base, 6, 5);
        let add = Change::Add { key: "room1".into(), kind: "medium".into(), rect: spot, floor: 0, top: 236 };
        let t = trial(&recipe, &add, &base);
        assert_eq!(t.refused, None);
        assert_eq!(t.recipe.edits, [Edit::Add { key: "room1".into(), kind: "medium".into(), rect: spot, floor: 0, top: 236 }]);
        assert_eq!(new_key(&t.recipe.lots_on(base.clone()).unwrap()), "room2");
        // moved, sized and pinned: still its add alone, with where it now is
        let moved = trial(&t.recipe, &Change::Move { room: "room1".into(), to: (spot.0 + 1, spot.1) }, &base);
        let sized = trial(&moved.recipe, &Change::Rect { room: "room1".into(), rect: (spot.0 + 1, spot.1, spot.2, spot.3) }, &base);
        let pinned = trial(&sized.recipe, &Change::Pin { room: "room1".into(), floor: 276 }, &base);
        assert_eq!((pinned.refused.clone(), moved.refused.clone(), sized.refused.clone()), (None, None, None));
        assert_eq!(
            pinned.recipe.edits,
            [Edit::Add { key: "room1".into(), kind: "medium".into(), rect: (spot.0 + 1, spot.1, spot.2, spot.3), floor: 276, top: 512 }]
        );
        // what names it later goes with it when it is deleted, and so does its add
        let mut r = pinned.recipe.clone();
        r.edits.push(Edit::Cover { room: "room1".into(), cover: "pillars".into() });
        r.edits.push(Edit::Brightness { room: "vast".into(), value: 120 });
        let gone = trial(&r, &Change::Delete { room: "room1".into() }, &base);
        assert_eq!((gone.refused.clone(), gone.dropped.clone()), (None, vec!["room1: cover pillars".to_string()]));
        assert_eq!(gone.recipe.edits, [Edit::Brightness { room: "vast".into(), value: 120 }]);
        // one of the seed's own is deleted by the recipe's delete, its pods with it
        let pods: Vec<String> = base.lots.iter().filter(|a| a.host == "vast").map(|a| a.key.clone()).collect();
        let mut r = Recipe::new(40);
        r.edits.push(Edit::Join { room: "tower".into(), storey: 551, to: "vast".into(), on: vec![], door: None, to_door: None, via: None });
        r.edits.push(Edit::Cover { room: pods[0].clone(), cover: "none".into() });
        let gone = trial(&r, &Change::Delete { room: "vast".into() }, &base);
        assert_eq!(gone.recipe.edits, [Edit::Delete { room: "vast".into() }]);
        assert_eq!(gone.dropped.len(), 2);
        assert!(gone.lots.iter().all(|a| a.key != "vast" && !pods.contains(&a.key)));
        // a key that is taken, a kind the scale has not
        let t = trial(&recipe, &Change::Add { key: "vast".into(), kind: "medium".into(), rect: spot, floor: 0, top: 236 }, &base);
        assert_eq!((t.refused.as_deref(), &t.recipe), (Some("add: there is a room vast already"), &recipe));
        let t = trial(&recipe, &Change::Add { key: "room1".into(), kind: "attic".into(), rect: spot, floor: 0, top: 236 }, &base);
        assert!(t.refused.as_deref().is_some_and(|w| w.starts_with("add: no kind attic at this scale")), "{:?}", t.refused);
    }

    #[test]
    fn a_rooms_height_is_one_edit_its_pods_go_with_its_floor_and_back_is_none() {
        let recipe = Recipe::new(40);
        let base = recipe.scattered().unwrap();
        let hall = lot(&base.lots, "vast").clone();
        let pods: Vec<Lot> = base.lots.iter().filter(|a| a.host == "vast").cloned().collect();
        let up = |z: i32, n: usize| LEVELS[index(z) + n];
        let ceiling = |top: i32| CEILINGS[index(top - STOREY)];
        // a storey taller: its top alone, the pods where they were
        let taller = Change::Height { room: "vast".into(), floor: hall.floor, top: ceiling(hall.top) + (up(hall.top - STOREY, 1) - (hall.top - STOREY)) };
        let t = trial(&recipe, &taller, &base);
        let Change::Height { top, .. } = taller else { unreachable!() };
        assert_eq!(t.recipe.edits, [Edit::Height { room: "vast".into(), floor: hall.floor, top }]);
        assert_eq!((lot(&t.lots, "vast").floor, lot(&t.lots, "vast").top), (hall.floor, top));
        for pod in &pods {
            assert_eq!(lot(&t.lots, &pod.key).floor, pod.floor, "{} stays", pod.key);
        }
        // the same height a storey up is a pin, and the pods go up a storey with it
        let shifted = Change::Height { room: "vast".into(), floor: up(hall.floor, 1), top: hall.top + up(hall.floor, 1) - hall.floor };
        let p = trial(&recipe, &shifted, &base);
        assert_eq!(p.recipe.edits, [Edit::Pin { room: "vast".into(), floor: up(hall.floor, 1) }]);
        for pod in &pods {
            assert_eq!(lot(&p.lots, &pod.key).floor, up(pod.floor, 1), "{} goes up with it", pod.key);
        }
        // and back as it was: nothing
        let back = trial(&t.recipe, &Change::Height { room: "vast".into(), floor: hall.floor, top: hall.top }, &base);
        assert_eq!(back.recipe, recipe);
        // under a storey tall, or a top that is no ceiling line: refused, nothing changed
        let low = trial(&recipe, &Change::Height { room: "vast".into(), floor: 551, top: 512 }, &base);
        assert!(low.refused.as_deref().is_some_and(|w| w.contains("under a storey tall")), "{:?}", low.refused);
        let odd = trial(&recipe, &Change::Height { room: "vast".into(), floor: 0, top: 600 }, &base);
        assert!(odd.refused.as_deref().is_some_and(|w| w.starts_with("height: vast's top would be at 600")), "{:?}", odd.refused);
        assert_eq!((low.recipe.clone(), odd.recipe.clone()), (recipe.clone(), recipe.clone()));
        // a room made by hand keeps its add alone, with its new heights
        let spot = free(&base, 6, 5);
        let made = trial(&recipe, &Change::Add { key: "room1".into(), kind: "medium".into(), rect: spot, floor: 0, top: 236 }, &base);
        let tall = trial(&made.recipe, &Change::Height { room: "room1".into(), floor: 0, top: 787 }, &base);
        assert_eq!(tall.refused, None);
        assert_eq!(tall.recipe.edits, [Edit::Add { key: "room1".into(), kind: "medium".into(), rect: spot, floor: 0, top: 787 }]);
    }

    #[test]
    fn a_change_warns_of_a_corridor_named_by_its_key() {
        let mut recipe = Recipe::new(40);
        recipe.edits.push(Edit::Remove { route: "c23".into() });
        let base = recipe.scattered().unwrap();
        let a = lot(&base.lots, "medium1").clone();
        let spot = free(&base, a.x1 - a.x0, a.y1 - a.y0);
        let t = trial(&recipe, &Change::Move { room: "medium1".into(), to: (spot.0, spot.1) }, &base);
        assert!(t.warn.as_deref().is_some_and(|w| w.starts_with("the recipe takes out c23 by its key")), "{:?}", t.warn);
        // the edits of other stages keep their places in the list
        assert_eq!(t.recipe.edits[0], Edit::Remove { route: "c23".into() });
    }

    #[test]
    fn what_a_trial_passes_makes_the_lots_it_showed() {
        let recipe = Recipe::new(40);
        let base = recipe.scattered().unwrap();
        let spot = free(&base, 6, 5);
        let t = trial(&recipe, &Change::Add { key: "room1".into(), kind: "medium".into(), rect: spot, floor: 0, top: 236 }, &base);
        let (placed, rooms) = t.recipe.rooms().unwrap();
        assert_eq!(placed.lots, t.lots);
        assert!(rooms.sp.iter().any(|s| s.key == "room1"));
    }
}
