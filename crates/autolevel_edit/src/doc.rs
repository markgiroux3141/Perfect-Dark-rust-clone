//! The document: a recipe being edited, its undo history, and the level it makes. Laid out as
//! `pd_edit`'s `Doc` in the clone: the data, an undo and a redo stack of whole snapshots (500 at
//! most), a copy as last saved that `dirty` compares with, and a method for each edit.
//!
//! Every edit changes the recipe and leaves the level stale: `regenerate` makes it again here
//! and now (tests, `--shot`), or the window runs a `jobs::Job` on a thread and hands the result
//! to `accept`. Until then the last level made stays to look at.
//!
//! What is picked is here too, though it is no part of the recipe and undo leaves it alone: a
//! room and a storey to join from (`pick`), what is asked of the joins from it (`ask`: a
//! doorway on its wall, the room at the other end and a doorway on that one's, the way), with
//! the corridors that could be made from there (`find_joins` here and now, or a `jobs::Search`
//! handed to `accept_joins`; `choose` adds one to the recipe as a `join` edit), or a corridor (`pick_route`; `remove_route` takes it out, or
//! says why not).
//!
//! Room boxes (milestone 3) are changed through `autolevel::boxes`: `select` a room, ask what a
//! `Change` would come to with `try_change` (nothing changes: this is what a drag shows as it
//! goes, and a refusal comes back in plain words), and make it with `change`, one step to undo,
//! which is refused with the same words if the lots would break a rule. `boxes` is what the plan
//! then draws: the lots as the recipe places them, whether or not a level has been made of them.
//!
//! One room's choices (milestone 4) go through `autolevel::choices`: `choices` reads what the
//! selected room has and could take, `set_choice` makes one (a `layout`, `cover`, `reroll`,
//! `brightness` or `ceiling` edit, the room's only one of its kind) or refuses it in plain
//! words, and `clear_choice` gives the seed's own back.
//!
//! The cascade (roadmap item 7): when a level arrives for a newer recipe than the last one made
//! (an edit, an undo, a re-roll kept), the one it replaces is kept (`before`) and the two are
//! compared (`cascade`: `autolevel::cascade::diff`), so the plan can light what is new, ghost
//! what went and say it in words.
//!
//! Locks, the rest and copies (roadmap item 8): every change goes through one place (`set`),
//! which refuses what would change a locked room (`autolevel::lock::guard`). `lock` and `unlock`
//! a room, `set_rest` re-rolls every room not locked; `copy` puts a room on the clipboard (or
//! `set_clipboard` a preset), `mirror_clipboard`, `try_paste` (what the plan draws as the pointer
//! moves) and `paste`; `mirror` flips a room in place.

use crate::deadlist::{make, Made};
use crate::planview::{Boxes, Things, View};
use autolevel::boxes::{self, Change, Trial};
use autolevel::candidates::{self, Asked, Candidate, Candidates, Removal};
use autolevel::cascade::{diff, Cascade};
use autolevel::choices::{self, Choices};
use autolevel::copy::{self, Copy};
use autolevel::level::Start;
use autolevel::lock;
use autolevel::place::{Placed, Problem, Siting};
use autolevel::puts;
use autolevel::recipe::{self, Edit, Recipe};
use std::sync::Arc;

/// The seed's own lots, with the seed and siting they were scattered for.
type Base = (u64, Siting, Start, Result<Arc<Placed>, String>);

/// The most steps kept to undo.
pub const UNDO: usize = 500;

/// A room picked to join from: its key, the storey, and what is asked of the joins.
pub type Pick = (String, i32, Asked);

/// What a search for joins came to: the count of the recipe's changes and the pick it was
/// made for, and what it found (or why it could not look).
type Found = (u64, Pick, Result<Arc<Candidates>, String>);

/// A recipe, its history and its level.
pub struct Doc {
    pub recipe: Recipe,
    undo: Vec<Recipe>,
    redo: Vec<Recipe>,
    /// The recipe as last saved (None: never).
    saved: Option<Recipe>,
    /// Counts the recipe's changes: a level is for the recipe of one count.
    rev: u64,
    /// The last level made, and the count of the recipe it was made from.
    made: Option<(u64, Arc<Made>)>,
    /// The level made before it (for an older recipe), and what changed from that one to it.
    before: Option<Arc<Made>>,
    cascade: Option<Arc<Cascade>>,
    /// Why the recipe as it stands makes no level, and for which count.
    failed: Option<(u64, String)>,
    /// The room picked to join from, and the storey.
    pick: Option<(String, i32)>,
    /// What is asked of the joins from it (nothing with no pick).
    asked: Asked,
    /// What could be joined to a pick: the last search's result.
    found: Option<Found>,
    /// The route picked, by its key.
    route: Option<String>,
    /// The room whose box is selected.
    selected: Option<String>,
    /// The seed's own lots, kept while the seed and the siting stay as they are.
    base: Option<Base>,
    /// A room copied (or a preset read), to paste.
    clipboard: Option<Copy>,
    /// The problems with the recipe's room boxes, for a count of its changes (`problems`).
    problems: Option<(u64, Arc<Vec<Problem>>)>,
}

impl Doc {
    /// Editing `recipe`; `saved` says whether it is what a file holds.
    pub fn new(recipe: Recipe, saved: bool) -> Doc {
        let was = saved.then(|| recipe.clone());
        Doc {
            recipe,
            undo: Vec::new(),
            redo: Vec::new(),
            saved: was,
            rev: 0,
            made: None,
            before: None,
            cascade: None,
            failed: None,
            pick: None,
            asked: Asked::default(),
            found: None,
            route: None,
            selected: None,
            base: None,
            clipboard: None,
            problems: None,
        }
    }

    pub fn dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.recipe)
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.recipe.clone());
    }

    /// Before an edit: remember the recipe for undo.
    fn push(&mut self) {
        self.undo.push(self.recipe.clone());
        if self.undo.len() > UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.rev += 1;
    }

    /// The recipe becomes `new`: one step to undo, none if it changes nothing (false). Err, and
    /// nothing changed, if it changes what the recipe says of a locked room (`lock::guard`):
    /// every change the document makes goes through here.
    fn set(&mut self, new: Recipe) -> Result<bool, String> {
        lock::guard(&self.recipe, &new)?;
        if new == self.recipe {
            return Ok(false);
        }
        self.push();
        self.recipe = new;
        Ok(true)
    }

    /// Start a drag: one undo step for the whole move. (The recipe is then changed in place.)
    pub fn begin_drag(&mut self) {
        self.push();
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(r) => {
                self.redo.push(std::mem::replace(&mut self.recipe, r));
                self.rev += 1;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(r) => {
                self.undo.push(std::mem::replace(&mut self.recipe, r));
                self.rev += 1;
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ------------------------------------------------------------ edits

    /// Another seed: another level. The edits are kept (they may no longer apply).
    pub fn set_seed(&mut self, seed: u64) {
        if seed != self.recipe.seed {
            self.push();
            self.recipe.seed = seed;
        }
    }

    /// Begin with the seed's own rooms or with none (`Start::Empty`). The edits are set aside
    /// (they name the other start's rooms); undo brings them back. Returns how many went.
    pub fn set_start(&mut self, start: Start) -> usize {
        if start == self.recipe.options.start {
            return 0;
        }
        self.push();
        self.recipe.options.start = start;
        std::mem::take(&mut self.recipe.edits).len()
    }

    /// Set an option by its name, from text (`recipe::set_option`).
    pub fn set_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        let mut o = self.recipe.options.clone();
        recipe::set_option(&mut o, name, value)?;
        if o != self.recipe.options {
            self.push();
            self.recipe.options = o;
        }
        Ok(())
    }

    /// Add an edit to the end of the recipe's list. Err if it names a locked room.
    pub fn add(&mut self, e: Edit) -> Result<(), String> {
        let mut new = self.recipe.clone();
        new.edits.push(e);
        lock::guard(&self.recipe, &new)?;
        self.push();
        self.recipe = new;
        Ok(())
    }

    /// Join `room`'s `storey` to the room `to`, which takes the door on one of `on` (none: any).
    pub fn join(&mut self, room: &str, storey: i32, to: &str, on: &[i32]) -> Result<(), String> {
        self.add(Edit::Join { room: room.to_string(), storey, to: to.to_string(), on: on.to_vec(), door: None, to_door: None, via: None })
    }

    /// Take out the route with a piece of this key: the `remove` edit, or for a corridor the
    /// dead-end pass added, a `keep` of the dead end it joined up. Returns what was done. Err,
    /// and nothing changed, if it is the only way to a room, there is no such route, or the
    /// level on show is not the recipe's own yet.
    pub fn remove_route(&mut self, key: &str) -> Result<Removal, String> {
        let done = self.removal(key)?;
        self.add(done.edit())?;
        self.route = None;
        Ok(done)
    }

    /// What `remove_route` would do, or why it can't.
    pub fn removal(&self, key: &str) -> Result<Removal, String> {
        let made = self.made.as_ref().filter(|(rev, _)| *rev == self.rev).map(|(_, made)| made);
        let made = made.ok_or("the level is being made again: a moment")?;
        candidates::removal(&self.recipe, &made.level, key)
    }

    /// Leave the dead end this room is in as it is.
    pub fn keep(&mut self, room: &str) -> Result<(), String> {
        self.add(Edit::Keep { room: room.to_string() })
    }

    /// A room's own decks stop at the storey `above`.
    pub fn cut(&mut self, room: &str, above: i32) -> Result<(), String> {
        self.add(Edit::Cut { room: room.to_string(), above })
    }

    /// Take the `n`th edit out of the list. False if there is none; Err if it is one of a locked
    /// room's (taking out its `lock` unlocks it).
    pub fn delete_edit(&mut self, n: usize) -> Result<bool, String> {
        if n >= self.recipe.edits.len() {
            return Ok(false);
        }
        let mut new = self.recipe.clone();
        new.edits.remove(n);
        self.set(new)
    }

    // ------------------------------------------------------------ what is picked

    /// Pick a room and the storey to join it from. What could be joined to it is then to be
    /// found: `find_joins`, or on a thread. What was asked stays for the same room on another
    /// storey (its doorways are on its walls whichever), and goes for another room.
    pub fn pick(&mut self, room: &str, storey: i32) {
        if self.pick.as_ref().is_none_or(|(r, _)| r != room) {
            self.asked = Asked::default();
        }
        self.pick = Some((room.to_string(), storey));
        self.route = None;
    }

    /// Nothing is picked: no room, no route.
    pub fn unpick(&mut self) {
        (self.pick, self.route, self.asked) = (None, None, Asked::default());
    }

    pub fn picked(&self) -> Option<(&str, i32)> {
        self.pick.as_ref().map(|(room, z)| (room.as_str(), *z))
    }

    /// The pick with what is asked of it: what a search is made for.
    pub fn pick_asked(&self) -> Option<Pick> {
        self.pick.as_ref().map(|(room, z)| (room.clone(), *z, self.asked.clone()))
    }

    /// Ask this of the joins from the pick (a doorway, the room at the other end, the way): they
    /// are then looked for again. No edit: nothing to undo. False with no pick.
    pub fn ask(&mut self, asked: Asked) -> bool {
        if self.pick.is_none() {
            return false;
        }
        self.asked = asked;
        true
    }

    /// What is asked of the joins from the pick.
    pub fn asked(&self) -> &Asked {
        &self.asked
    }

    /// The search's result for the pick and the recipe as they stand.
    fn found(&self) -> Option<&Result<Arc<Candidates>, String>> {
        let (rev, pick, result) = self.found.as_ref()?;
        (*rev == self.rev && Some(pick) == self.pick_asked().as_ref()).then_some(result)
    }

    /// Is a room picked, with nothing found for it and the recipe as they stand?
    pub fn wants_joins(&self) -> bool {
        self.pick.is_some() && self.found().is_none()
    }

    /// Find what could be joined to the pick, here and now.
    pub fn find_joins(&mut self) -> Result<Arc<Candidates>, String> {
        let pick = self.pick_asked().ok_or("no room is picked")?;
        let result = candidates::candidates_asked(&self.recipe, &pick.0, pick.1, &pick.2);
        self.accept_joins(self.rev, &pick, result);
        self.found().expect("just found").clone()
    }

    /// Take a search's result, made for `pick` and the recipe as it was at `rev`. False (and
    /// nothing kept) if either has changed since.
    pub fn accept_joins(&mut self, rev: u64, pick: &Pick, result: Result<Candidates, String>) -> bool {
        if rev != self.rev || Some(pick) != self.pick_asked().as_ref() {
            return false;
        }
        self.found = Some((rev, pick.clone(), result.map(Arc::new)));
        true
    }

    /// What could be joined to the pick: None while it is not known (or could not be found).
    pub fn joins(&self) -> Option<&Arc<Candidates>> {
        self.found().and_then(|result| result.as_ref().ok())
    }

    /// Why nothing could be looked for from the pick (a pod, a storey with no doors).
    pub fn joins_error(&self) -> Option<&str> {
        self.found().and_then(|result| result.as_ref().err()).map(String::as_str)
    }

    /// Make the `n`th of them (from 1, as they are listed): its `join` edit goes on the end of
    /// the recipe, one step to undo. The pick is done with, and the new corridor is picked
    /// instead, so the plan shows where it is.
    pub fn choose(&mut self, n: usize) -> Result<Candidate, String> {
        let found = self.joins().ok_or("nothing has been found to join yet")?;
        let c = found.get(n).ok_or_else(|| format!("no join {n}: there are {}", found.found.len()))?.clone();
        self.add(c.edit())?;
        (self.pick, self.asked) = (None, Asked::default());
        self.route = Some(c.route.key().to_string());
        Ok(c)
    }

    /// Pick a corridor, stub or vent, by its route's key.
    pub fn pick_route(&mut self, key: &str) {
        self.route = Some(key.to_string());
        (self.pick, self.asked) = (None, Asked::default());
    }

    pub fn picked_route(&self) -> Option<&str> {
        self.route.as_deref()
    }

    /// What is picked, for the plan to show: the room, the corridors that could be made from
    /// it (each room's cheapest, or with `all` every one; the one under the pointer, `hot`,
    /// either way), and the route. The caller sets the rest of the view.
    pub fn look(&self, all: bool, hot: Option<usize>) -> View {
        let joins = self.joins().map_or(Vec::new(), |found| {
            let shown = |n: usize| all || n <= found.rooms || hot == Some(n);
            found.found.iter().enumerate().map(|(k, c)| (k + 1, c.clone())).filter(|(n, _)| shown(*n)).collect()
        });
        let chosen = choices::all(&self.recipe).into_iter().map(|(room, c)| (room, c.words())).collect();
        // (nothing found through what was asked: the plan says so where the doorway is)
        let none = self.joins().filter(|found| found.found.is_empty() && !found.asked.plain()).map(|found| found.none_fits());
        let asked = if self.pick.is_some() { self.asked.clone() } else { Asked::default() };
        let things = Things { tool: None, put: self.puts() };
        let locked = lock::locked(&self.recipe);
        View {
            pick: self.pick.clone(),
            asked,
            none,
            joins,
            hot,
            route: self.route.clone(),
            chosen,
            things,
            cascade: self.cascade.clone(),
            locked,
            ..View::default()
        }
    }

    // ------------------------------------------------------------ room boxes

    /// Select a room's box (None: none). No edit: nothing to undo.
    pub fn select(&mut self, room: Option<&str>) {
        self.selected = room.map(str::to_string);
    }

    /// The room whose box is selected.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The seed's own lots, as scattered for the recipe's seed and siting (none, for an empty
    /// start): what the recipe's lot edits are made on. Worked out once, and again when any of
    /// the three changes. Err if the site has no room for a lot.
    pub fn base(&mut self) -> Result<Arc<Placed>, String> {
        let (seed, siting, start) = (self.recipe.seed, &self.recipe.options.siting, self.recipe.options.start);
        if !self.base.as_ref().is_some_and(|(s, o, b, _)| *s == seed && o == siting && *b == start) {
            self.base = Some((seed, siting.clone(), start, self.recipe.scattered().map(Arc::new)));
        }
        self.base.as_ref().expect("just set").3.clone()
    }

    /// The lots as the recipe places them now, held to no rule (so they can be shown even
    /// where the recipe makes no level). Err if a lot edit of the recipe cannot be read.
    pub fn lots(&mut self) -> Result<Placed, String> {
        let base = self.base()?;
        self.recipe.lots_on((*base).clone())
    }

    /// What a change to the room boxes would come to (`boxes::trial`): the recipe with it, the
    /// lots as they would lie, and the refusal if they would break a rule. Nothing changes.
    /// Err only if the seed scatters no lots at all.
    pub fn try_change(&mut self, change: &Change) -> Result<Trial, String> {
        let base = self.base()?;
        Ok(boxes::trial(&self.recipe, change, &base))
    }

    /// Make a change to the room boxes: one step to undo (none if it changes nothing). The room
    /// changed is then the one selected (a deleted one: none; and it is no longer picked to join
    /// from). Err with the refusal, and nothing changed, if the lots would break a rule. The
    /// trial comes back for what it says besides (`dropped`, `warn`).
    pub fn change(&mut self, change: &Change) -> Result<Trial, String> {
        let t = self.try_change(change)?;
        if let Some(why) = &t.refused {
            return Err(why.clone());
        }
        self.set(t.recipe.clone())?;
        if let Change::Delete { room } = change {
            self.selected = None;
            if self.pick.as_ref().is_some_and(|(r, _)| r == room) {
                (self.pick, self.asked) = (None, Asked::default());
            } else if self.asked.to.as_ref() == Some(room) {
                (self.asked.to, self.asked.to_door) = (None, None);
            }
        } else {
            self.selected = Some(change.room().to_string());
        }
        Ok(t)
    }

    /// A change to the room boxes made as a sketch: kept even where the lots then break the
    /// scatter's rule (two rooms overlap, a spire too short), so a plan can be drawn wrong and put
    /// right; the recipe then makes no level until it is (`problems` says what is wrong, the plan
    /// draws it in red, and the last level made stays on show). What the window does; `change`,
    /// which refuses, is the command line's. Err, and nothing changed, for what can't be made at
    /// all (a room that is not there, a box under 2 x 2) or would change a locked room.
    pub fn sketch(&mut self, change: &Change) -> Result<Trial, String> {
        let t = self.try_change(change)?;
        if t.recipe == self.recipe {
            if let Some(why) = &t.refused {
                // (nothing came of it: the change itself could not be made)
                return Err(why.clone());
            }
        }
        self.set(t.recipe.clone())?;
        match change {
            Change::Delete { room } => {
                self.selected = None;
                if self.pick.as_ref().is_some_and(|(r, _)| r == room) {
                    (self.pick, self.asked) = (None, Asked::default());
                }
            }
            _ => self.selected = Some(change.room().to_string()),
        }
        Ok(t)
    }

    /// What is wrong with the recipe's room boxes (`boxes::problems`): none when the plan keeps
    /// the scatter's rule. Worked out once for each state of the recipe.
    pub fn problems(&mut self) -> Arc<Vec<Problem>> {
        if let Some((rev, found)) = &self.problems {
            if *rev == self.rev {
                return found.clone();
            }
        }
        let found = Arc::new(match self.base() {
            Ok(base) => boxes::problems(&self.recipe, &base),
            Err(words) => vec![Problem { rooms: Vec::new(), words }],
        });
        self.problems = Some((self.rev, found.clone()));
        found
    }

    /// Why a tool that needs the recipe's level can't be used yet: the plan has problems.
    pub fn unready(&mut self) -> Option<String> {
        let n = self.problems().len();
        (n > 0).then(|| format!("the plan has {n} problem{}: put the rooms in red right first", if n == 1 { "" } else { "s" }))
    }

    /// The room boxes for the plan to draw over the level: the lots as the recipe places them
    /// (or as `trial`, a change being tried, would: its refusal goes with them), and the one
    /// selected, if there is still such a room.
    pub fn boxes(&mut self, trial: Option<&Trial>) -> Boxes {
        let placed = self.lots().ok();
        let site = placed.as_ref().map(|p| p.site);
        let lots = match trial {
            Some(t) => t.lots.clone(),
            None => placed.map_or(Vec::new(), |p| p.lots),
        };
        let selected = self.selected.clone().filter(|key| lots.iter().any(|a| a.key == *key));
        // (what is wrong with the plan as it stands: not with one a drag is trying)
        let problems = if trial.is_none() { (*self.problems()).clone() } else { Vec::new() };
        Boxes { lots, selected, refused: trial.and_then(|t| t.refused.clone()), site, ghost: None, problems }
    }

    // ------------------------------------------------------------ one room's choices

    /// One room's choices: what it has in the last level made (which may be an older one while
    /// the recipe's own is being made), what the recipe chooses for it, and what it could
    /// take. Err if there is no level yet, or no such room in it.
    pub fn choices(&self, room: &str) -> Result<Choices, String> {
        let made = self.made().ok_or("no level has been made yet")?;
        choices::of(&self.recipe, &made.level, room)
    }

    /// Make a choice for one room: `e` is a `layout`, `cover`, `reroll`, `brightness` or
    /// `ceiling` edit, and becomes the room's only edit of its kind (a `reroll` of 0 is the
    /// seed's own inside: no edit). One step to undo; none if it changes nothing. Err, and
    /// nothing changed, if it can be told without making the level that it can't be
    /// (`choices::refusal`: against the recipe's own level if it has been made, by the values
    /// alone while it is still being made). What only making the level shows (a
    /// layout no attempt can be furnished with) comes back as the level's `error`.
    pub fn set_choice(&mut self, e: Edit) -> Result<(), String> {
        // (the room is looked up only in the recipe's own level: an older one may lack a room
        // just made)
        if let Some(why) = self.unready() {
            return Err(why);
        }
        let own = self.made.as_ref().filter(|(rev, _)| *rev == self.rev).map(|(_, made)| &made.level);
        if let Some(why) = choices::refusal(own, &e) {
            return Err(why);
        }
        let new = choices::set(&self.recipe, e)?;
        self.set(new).map(drop)
    }

    /// The seed's own again for one kind of choice (`choices::KINDS`: "layout", "cover",
    /// "reroll", "brightness", "ceiling") in one room. False if nothing was chosen; Err for a
    /// locked room.
    pub fn clear_choice(&mut self, room: &str, what: &str) -> Result<bool, String> {
        self.set(choices::clear(&self.recipe, room, what))
    }

    /// Put a thing by place: `e` is a `window`, `piece` or `drop` edit (`puts::click` makes one
    /// from a click), put with the recipe kept short (`puts::set`: a piece stood and then moved is
    /// one edit). One step to undo; none if it changes nothing. One that does not fit is still
    /// put, and the level says why (`puts`). Err in a locked room.
    pub fn put(&mut self, e: Edit) -> Result<bool, String> {
        if let Some(why) = self.unready() {
            return Err(why);
        }
        self.set(puts::set(&self.recipe, e))
    }

    /// The generator's own again at a place (`puts::clear`). False if nothing was put there; Err
    /// in a locked room.
    pub fn clear_put(&mut self, room: &str, at: (i32, i32)) -> Result<bool, String> {
        self.set(puts::clear(&self.recipe, room, at))
    }

    // ------------------------------------------------------------ locks, the rest, copies

    /// The recipe's own level, if it has been made: what a lock, a copy or a mirror is read from.
    fn own(&mut self) -> Result<Arc<Made>, String> {
        if let Some(why) = self.unready() {
            return Err(why);
        }
        let made = self.made.as_ref().filter(|(rev, _)| *rev == self.rev).map(|(_, made)| made.clone());
        made.ok_or_else(|| "the level is being made again: a moment".to_string())
    }

    /// Lock a room (`lock::lock`: the dead-end pass's corridors to it made the recipe's own, then
    /// the `lock`). One step to undo. Err for a pod, a room not there, or while the level is
    /// being made.
    pub fn lock(&mut self, room: &str) -> Result<bool, String> {
        let own = self.own()?;
        let new = lock::lock(&self.recipe, &own.level, room)?;
        self.set(new)
    }

    /// Unlock a room: its `lock` taken out. False if it was not locked.
    pub fn unlock(&mut self, room: &str) -> bool {
        self.set(lock::unlock(&self.recipe, room)).unwrap_or(false)
    }

    /// The rooms locked.
    pub fn locked(&self) -> Vec<String> {
        lock::locked(&self.recipe)
    }

    /// Re-roll every room not locked from stream `n` (0: the seed's own again). False if it was so.
    pub fn set_rest(&mut self, n: u32) -> bool {
        if self.unready().is_some() {
            return false;
        }
        self.set(lock::rest(&self.recipe, n)).unwrap_or(false)
    }

    /// Which stream the rest is re-rolled from (0: the seed's own).
    pub fn rest(&self) -> u32 {
        lock::rest_of(&self.recipe)
    }

    /// Copy a room of the recipe's own level to the clipboard. Err for a pod, a room not there,
    /// or while the level is being made.
    pub fn copy(&mut self, room: &str) -> Result<&Copy, String> {
        let own = self.own()?;
        let c = copy::copy(&self.recipe, &own.level, room)?;
        Ok(self.clipboard.insert(c))
    }

    /// What is on the clipboard.
    pub fn clipboard(&self) -> Option<&Copy> {
        self.clipboard.as_ref()
    }

    /// Put a copy (a preset read) on the clipboard, or take it off.
    pub fn set_clipboard(&mut self, c: Option<Copy>) {
        self.clipboard = c;
    }

    /// Mirror what is on the clipboard on "x" or "y". False with nothing there.
    pub fn mirror_clipboard(&mut self, axis: &str) -> bool {
        let Some(c) = &self.clipboard else { return false };
        self.clipboard = Some(c.mirrored(axis));
        true
    }

    /// What pasting the clipboard with its low corner on `at` and its floor on `floor` would come
    /// to (`copy::paste`): the recipe and the new room's key, or why it can't. Nothing changes.
    pub fn try_paste(&self, at: (i32, i32), floor: i32) -> Result<(Recipe, String), String> {
        let c = self.clipboard.as_ref().ok_or("nothing is copied")?;
        copy::paste(&self.recipe, c, at, floor)
    }

    /// Paste the clipboard there: one step to undo; the new room is selected. Returns its key.
    pub fn paste(&mut self, at: (i32, i32), floor: i32) -> Result<String, String> {
        let (new, key) = self.try_paste(at, floor)?;
        self.set(new)?;
        self.selected = Some(key.clone());
        Ok(key)
    }

    /// Mirror a room in place on "x" or "y" (`copy::mirror`): one step to undo.
    pub fn mirror(&mut self, room: &str, axis: &str) -> Result<bool, String> {
        let own = self.own()?;
        let new = copy::mirror(&self.recipe, &own.level, room, axis)?;
        self.set(new)
    }

    /// What the recipe put by place, each with whether the last level made took it (`puts::listed`;
    /// all taken while there is none).
    pub fn puts(&self) -> Vec<(Edit, Result<(), String>)> {
        match self.made() {
            Some(made) => puts::listed(&self.recipe, &made.level),
            None => self
                .recipe
                .edits
                .iter()
                .filter(|e| matches!(e, Edit::Window { .. } | Edit::Piece { .. } | Edit::Drop { .. }))
                .map(|e| (e.clone(), Ok(())))
                .collect(),
        }
    }

    // ------------------------------------------------------------ the level

    /// The count of the recipe's changes: what a level made on a thread is handed back with.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Is there no level (and no failure) for the recipe as it stands?
    pub fn stale(&self) -> bool {
        self.made.as_ref().is_none_or(|(rev, _)| *rev != self.rev) && self.failed.as_ref().is_none_or(|(rev, _)| *rev != self.rev)
    }

    /// Make the level here and now.
    pub fn regenerate(&mut self) -> Result<(), String> {
        let result = make(&self.recipe);
        self.accept(self.rev, result);
        self.error().map_or(Ok(()), |why| Err(why.to_string()))
    }

    /// Take a level made for the recipe as it was at `rev`. False (and nothing kept) if the
    /// recipe has changed since.
    pub fn accept(&mut self, rev: u64, result: Result<Made, String>) -> bool {
        if rev != self.rev {
            return false;
        }
        match result {
            Ok(made) => {
                self.take(rev, Arc::new(made));
                self.failed = None;
            }
            Err(why) => self.failed = Some((rev, why)),
        }
        true
    }

    /// Keep a re-roll from the variants strip: its edit made as a choice (`set_choice`), and the
    /// level already made for it taken as the recipe's own, so nothing is made again. The level
    /// is taken only if the recipe then is the one it was made from (`recipe`).
    pub fn adopt(&mut self, e: Edit, recipe: &Recipe, made: Arc<Made>) -> Result<(), String> {
        self.set_choice(e)?;
        if self.recipe == *recipe {
            self.take(self.rev, made);
            self.failed = None;
        }
        Ok(())
    }

    /// A level for the recipe at `rev`: the one it replaces is kept as `before`, with what
    /// changed, if it was made for another recipe (the same one made again changes nothing).
    fn take(&mut self, rev: u64, made: Arc<Made>) {
        if let Some((was, old)) = self.made.take() {
            if was != rev {
                self.cascade = Some(Arc::new(diff(&old.level, &made.level)));
                self.before = Some(old);
            }
        }
        self.made = Some((rev, made));
    }

    /// The level made before the last one, for an older recipe.
    pub fn before(&self) -> Option<&Arc<Made>> {
        self.before.as_ref()
    }

    /// What changed from `before` to the last level made (None: only one has been made).
    pub fn cascade(&self) -> Option<&Arc<Cascade>> {
        self.cascade.as_ref()
    }

    /// The last level made: the recipe's own unless `stale` or `error`.
    pub fn made(&self) -> Option<&Arc<Made>> {
        self.made.as_ref().map(|(_, made)| made)
    }

    /// Why the recipe as it stands makes no level.
    pub fn error(&self) -> Option<&str> {
        self.failed.as_ref().filter(|(rev, _)| *rev == self.rev).map(|(_, why)| why.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_and_its_undo_each_say_what_they_changed() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        d.regenerate().unwrap();
        // the first level is compared with nothing
        assert!(d.cascade().is_none() && d.before().is_none());
        let first = d.made().unwrap().clone();
        assert!(d.put(Edit::Piece { room: "vast".into(), at: (41, 25), what: None, size: None, to: Some((40, 32)) }).unwrap());
        // (still the old level until the new one arrives)
        assert!(d.cascade().is_none());
        d.regenerate().unwrap();
        assert!(Arc::ptr_eq(d.before().unwrap(), &first));
        assert_eq!(d.cascade().unwrap().lines(), ["vast: a pillar moved from 41, 25 to 40, 32"]);
        // undone: the way back
        assert!(d.undo());
        d.regenerate().unwrap();
        assert_eq!(d.cascade().unwrap().lines(), ["vast: a pillar moved from 40, 32 to 41, 25"]);
        // the same recipe's level taken again changes nothing kept
        let rev = d.rev();
        let again = make(&d.recipe).unwrap();
        assert!(d.accept(rev, Ok(again)));
        assert_eq!(d.cascade().unwrap().lines(), ["vast: a pillar moved from 40, 32 to 41, 25"]);
        // and it is on the plan's view
        assert!(d.look(false, None).cascade.is_some());
    }

    #[test]
    fn edits_undo_redo_and_dirty() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        assert!(!d.dirty() && !d.can_undo() && !d.can_redo() && d.stale());
        d.set_seed(40); // no change: no step
        d.set_option("cover", "mixed").unwrap();
        assert!(!d.can_undo() && d.rev() == 0);
        d.set_seed(7);
        d.set_option("cover", "pillars").unwrap();
        d.join("tower", 551, "colossal", &[276, 551]).unwrap();
        d.add(Edit::Remove { route: "c07".into() }).unwrap();
        d.keep("tiny3").unwrap();
        d.cut("spire1", 827).unwrap();
        assert!(d.dirty() && d.rev() == 6);
        assert_eq!((d.recipe.seed, d.recipe.options.cover, d.recipe.edits.len()), (7, "pillars", 4));
        assert_eq!(
            d.recipe.edits[0],
            Edit::Join { room: "tower".into(), storey: 551, to: "colossal".into(), on: vec![276, 551], door: None, to_door: None, via: None }
        );
        // an option nobody knows changes nothing and is no step
        assert_eq!(d.set_option("cover", "hedges").unwrap_err(), "cover: one of ('mixed', 'pillars', 'crates', 'none')");
        assert_eq!(d.rev(), 6);
        assert!(d.delete_edit(1).unwrap() && !d.delete_edit(9).unwrap());
        assert_eq!(d.recipe.edits.iter().map(Edit::name).collect::<Vec<_>>(), ["join", "keep", "cut"]);
        // back, step by step, to the recipe as saved
        let last = d.recipe.clone();
        let mut steps = 0;
        while d.undo() {
            steps += 1;
        }
        assert_eq!((steps, d.recipe.clone(), d.dirty()), (7, Recipe::before_bases(40), false));
        assert!(!d.undo() && d.can_redo());
        while d.redo() {}
        assert_eq!(d.recipe, last);
        // a new edit after an undo drops what could be redone
        d.undo();
        d.set_seed(8);
        assert!(!d.can_redo() && !d.redo());
        d.mark_saved();
        assert!(!d.dirty());
        d.undo();
        assert!(d.dirty());
        // a drag is one step, however much it changes
        let was = d.recipe.clone();
        d.begin_drag();
        for seed in 100..120 {
            d.recipe.seed = seed;
        }
        assert!(d.undo() && d.recipe == was);
        // a recipe that was never saved is dirty from the start
        assert!(Doc::new(Recipe::before_bases(1), false).dirty());
    }

    #[test]
    fn undo_keeps_five_hundred_steps() {
        let mut d = Doc::new(Recipe::before_bases(0), true);
        for seed in 1..=600 {
            d.set_seed(seed);
        }
        let mut steps = 0;
        while d.undo() {
            steps += 1;
        }
        assert_eq!((steps, d.recipe.seed), (UNDO, 100));
    }

    #[test]
    fn a_level_is_for_the_recipe_it_was_made_from() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        assert!(d.made().is_none() && d.error().is_none());
        d.regenerate().unwrap();
        assert!(!d.stale() && d.made().unwrap().level.seed == 40);
        // an edit leaves the level stale, and the last one made stays to look at
        let old = d.rev();
        d.set_option("walkway", "3").unwrap();
        assert!(d.stale() && d.made().unwrap().level.seed == 40);
        // a level made on a thread for the recipe as it was is not taken
        assert!(!d.accept(old, Err("late".into())));
        assert!(d.stale() && d.error().is_none());
        // a recipe that makes no level says why, and is not stale either
        assert_eq!(d.regenerate().unwrap_err(), "walkway: 1 or 2 cells");
        assert!(!d.stale() && d.error() == Some("walkway: 1 or 2 cells") && d.made().is_some());
        // undo is another change: the level is to be made again, and the old failure is forgotten
        d.undo();
        assert!(d.stale() && d.error().is_none());
        let made = crate::deadlist::make(&d.recipe);
        assert!(d.accept(d.rev(), made) && !d.stale() && d.made().unwrap().dead.left.len() == 10);
    }

    #[test]
    fn a_job_makes_the_level_on_a_thread() {
        let job = crate::jobs::Job::spawn(Recipe { seed: 1, options: autolevel::level::Options { walkway: 3, ..Default::default() }, edits: Vec::new() }, 5);
        let result = loop {
            match job.poll() {
                Some(result) => break result,
                None => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        };
        assert_eq!((job.rev, result.unwrap_err().as_str()), (5, "walkway: 1 or 2 cells"));
    }

    #[test]
    fn a_room_is_picked_a_join_chosen_and_it_is_made_as_it_was_shown() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        d.regenerate().unwrap();
        assert!(d.picked().is_none() && !d.wants_joins() && d.joins().is_none());
        assert_eq!(d.find_joins().unwrap_err(), "no room is picked");
        assert_eq!(d.choose(1).unwrap_err(), "nothing has been found to join yet");
        // a pick is no edit: nothing to undo, the level still the recipe's
        d.pick("tower", 551);
        assert!(d.wants_joins() && !d.can_undo() && !d.stale() && d.picked() == Some(("tower", 551)));
        let found = d.find_joins().unwrap();
        assert!(!d.wants_joins() && d.joins().is_some_and(|f| Arc::ptr_eq(f, &found)) && d.joins_error().is_none());
        assert_eq!((found.rooms, found.found.len()), (9, 21));
        // the plan is shown each room's cheapest, or them all; the one pointed at either way
        let numbers = |v: View| v.joins.iter().map(|(n, _)| *n).collect::<Vec<_>>();
        assert_eq!(numbers(d.look(false, None)), (1..=9).collect::<Vec<_>>());
        assert_eq!(numbers(d.look(false, Some(14))), [1, 2, 3, 4, 5, 6, 7, 8, 9, 14]);
        assert_eq!(numbers(d.look(true, None)), (1..=21).collect::<Vec<_>>());
        assert_eq!(d.look(false, Some(2)).pick, Some(("tower".to_string(), 551)));
        // choosing one is one step: its join on the end of the recipe, and the corridor picked
        assert_eq!(d.choose(99).unwrap_err(), "no join 99: there are 21");
        let c = d.choose(4).unwrap();
        assert_eq!(c, found.found[3]);
        assert_eq!(d.recipe.edits, [Edit::Join { room: "tower".into(), storey: 551, to: c.to.clone(), on: vec![c.on], door: None, to_door: None, via: None }]);
        assert!(d.picked().is_none() && d.picked_route() == Some(c.route.key()) && d.stale() && d.dirty());
        assert_eq!(d.look(false, None).route.as_deref(), Some(c.route.key()));
        // made: the level has the route that was shown, cell for cell
        d.regenerate().unwrap();
        let made = d.made().unwrap().clone();
        let r = made.level.net.routes().iter().find(|r| r.key() == c.route.key()).expect("the corridor chosen");
        assert_eq!((r, autolevel::candidates::footprint(r)), (&c.route, c.cells()));
        // undo is the recipe without it; redo, with it: the same level again
        assert!(d.undo() && d.recipe == Recipe::before_bases(40) && !d.can_undo());
        d.regenerate().unwrap();
        assert!(d.made().unwrap().level.net.routes().iter().all(|r| *r != c.route));
        assert!(d.redo() && d.recipe.edits.len() == 1);
        d.regenerate().unwrap();
        assert_eq!(format!("{:?}", d.made().unwrap().level.net.routes()), format!("{:?}", made.level.net.routes()));
        // saved, loaded and made again: the same level
        let again = crate::deadlist::make(&recipe::load(&recipe::save(&d.recipe)).unwrap()).unwrap();
        assert_eq!(format!("{:?}", again.level.net.routes()), format!("{:?}", made.level.net.routes()));
        assert!(again.level.plan == made.level.plan && again.level.report == made.level.report);
        // what was found is for one pick and one recipe: another pick, or an edit, and it is looked for again
        d.pick("tower", 551);
        assert!(d.wants_joins() && d.joins().is_none());
        let rev = d.rev();
        let pick = |z: i32| ("tower".to_string(), z, Asked::default());
        assert!(!d.accept_joins(rev, &pick(276), Ok((*found).clone())));
        assert!(!d.accept_joins(rev - 1, &pick(551), Ok((*found).clone())));
        let vent = Asked { via: Some("vent".into()), ..Asked::default() };
        assert!(!d.accept_joins(rev, &("tower".to_string(), 551, vent), Ok((*found).clone())));
        assert!(d.accept_joins(rev, &pick(551), Err("no room tower".into())));
        assert_eq!((d.joins().is_none(), d.joins_error()), (true, Some("no room tower")));
        d.pick("small8", 551);
        assert_eq!(d.find_joins().unwrap_err(), "small8 is a pod: it is joined inside vast");
        d.unpick();
        assert!(d.picked().is_none() && d.picked_route().is_none() && !d.wants_joins());
    }

    #[test]
    fn a_corridor_is_picked_and_taken_out_or_the_reason_given() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        // nothing can be said of a corridor until the level is the recipe's own
        assert_eq!(d.remove_route("c23").unwrap_err(), "the level is being made again: a moment");
        d.regenerate().unwrap();
        d.pick_route("c23");
        assert!(d.picked_route() == Some("c23") && d.picked().is_none());
        assert_eq!(d.look(false, None).route.as_deref(), Some("c23"));
        // the only way to a room stays, with the reason, and nothing changes
        assert_eq!(d.remove_route("c17").unwrap_err(), "c17 is the only way to small7: without it the level is in two");
        assert_eq!(d.remove_route("c99").unwrap_err(), "no route c99 in this level");
        assert!(!d.can_undo() && !d.dirty() && d.picked_route() == Some("c23"));
        // a corridor of a loop goes: one step, by the recipe's remove edit
        assert_eq!(d.removal("c23").unwrap(), Removal::Remove { route: "c23".into() });
        assert_eq!(d.remove_route("c23").unwrap(), Removal::Remove { route: "c23".into() });
        assert_eq!(d.recipe.edits, [Edit::Remove { route: "c23".into() }]);
        assert!(d.picked_route().is_none() && d.stale());
        d.regenerate().unwrap();
        assert!(d.made().unwrap().level.net.routes().iter().all(|r| r.key() != "c23"));
        assert!(d.undo() && d.recipe.edits.is_empty());
        d.regenerate().unwrap();
        assert!(d.made().unwrap().level.net.routes().iter().any(|r| r.key() == "c23"));
        assert!(d.redo() && d.recipe.edits.len() == 1 && !d.redo());
        d.undo();
        d.regenerate().unwrap();
        // one the dead-end pass added goes by keeping its dead end (any piece's key names it)
        assert_eq!(d.remove_route("c39b").unwrap(), Removal::Keep { route: "c39a".into(), room: "spire1".into() });
        assert_eq!(d.recipe.edits, [Edit::Keep { room: "spire1".into() }]);
        d.regenerate().unwrap();
        let made = d.made().unwrap();
        assert!(made.level.net.routes().iter().all(|r| r.key() != "c39a") && made.dead.left.iter().any(|e| !e.end.allowed));
    }

    #[test]
    fn a_box_is_moved_in_one_step_or_refused_in_plain_words() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        let lots = d.lots().unwrap().lots;
        let at = |lots: &[autolevel::scatter::Lot], key: &str| lots.iter().find(|a| a.key == key).map(|a| a.rect()).unwrap();
        let (a, b) = (at(&lots, "medium1"), at(&lots, "medium2"));
        assert!(d.selected().is_none() && d.boxes(None).lots == lots && d.boxes(None).selected.is_none());
        d.select(Some("medium1"));
        assert_eq!((d.boxes(None).selected.as_deref(), d.selected(), d.can_undo()), (Some("medium1"), Some("medium1"), false));
        // tried: nothing changes, and the lots it would leave come back to draw
        let onto = Change::Move { room: "medium1".into(), to: (b.0, b.1) };
        let t = d.try_change(&onto).unwrap();
        let why = t.refused.clone().expect("it lands on another room");
        assert!(why.starts_with("medium1 and ") && why.contains("rooms keep a cell between them"), "{why}");
        assert!(d.recipe.edits.is_empty() && !d.can_undo() && d.rev() == 0);
        let shown = d.boxes(Some(&t));
        assert_eq!((at(&shown.lots, "medium1").0, shown.refused.as_deref(), shown.selected.as_deref()), (b.0, Some(why.as_str()), Some("medium1")));
        // made: refused with the same words, and still nothing changed
        assert_eq!(d.change(&onto).unwrap_err(), why);
        assert!(d.recipe.edits.is_empty() && !d.can_undo() && !d.dirty());
        // a side pulled in by a cell: one step, the level stale, the box where it was put
        let pulled = Change::Rect { room: "medium1".into(), rect: (a.0, a.1, a.2 - 1, a.3) };
        assert!(d.change(&pulled).unwrap().refused.is_none());
        assert_eq!(d.recipe.edits, [Edit::Resize { room: "medium1".into(), size: (a.2 - a.0 - 1, a.3 - a.1) }]);
        assert!(d.can_undo() && d.dirty() && d.stale() && d.rev() == 1);
        assert_eq!(at(&d.boxes(None).lots, "medium1"), (a.0, a.1, a.2 - 1, a.3));
        d.regenerate().unwrap();
        assert_eq!(d.made().unwrap().level.space("medium1").rect(), (a.0, a.1, a.2 - 1, a.3));
        // pulled back: the seed's own recipe again (a step all the same, so it can be undone)
        d.change(&Change::Rect { room: "medium1".into(), rect: a }).unwrap();
        assert!(d.recipe.edits.is_empty() && !d.dirty() && d.rev() == 2);
        // a change that changes nothing is no step
        d.change(&Change::Move { room: "medium1".into(), to: (a.0, a.1) }).unwrap();
        assert_eq!(d.rev(), 2);
        assert!(d.undo() && d.recipe.edits.len() == 1);
    }

    #[test]
    fn a_room_is_made_and_deleted_and_the_selection_follows() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        let placed = d.lots().unwrap();
        let key = boxes::new_key(&placed);
        let clear = |r: (i32, i32, i32, i32)| placed.lots.iter().all(|a| a.x1 + 2 <= r.0 || r.2 + 2 <= a.x0 || a.y1 + 2 <= r.1 || r.3 + 2 <= a.y0);
        let mut spots = (0..placed.site.1 - 5).flat_map(|y| (0..placed.site.0 - 6).map(move |x| (x, y, x + 6, y + 5)));
        let spot = spots.find(|r| clear(*r)).expect("a free place on seed 40's site");
        let add = Change::Add { key: key.clone(), kind: "medium".into(), rect: spot, floor: 0, top: 236 };
        d.change(&add).unwrap();
        assert_eq!((d.selected(), d.recipe.edits.len()), (Some(key.as_str()), 1));
        d.regenerate().unwrap();
        assert_eq!(d.made().unwrap().level.space(&key).rect(), spot);
        // the made room is picked to join from, then deleted: nothing is left picked or selected
        d.pick(&key, 0);
        let gone = d.change(&Change::Delete { room: key.clone() }).unwrap();
        assert!(gone.dropped.is_empty() && d.recipe.edits.is_empty() && d.selected().is_none() && d.picked().is_none());
        // undo brings the room back, not the selection; a selection that names no room is not shown
        assert!(d.undo() && d.recipe.edits.len() == 1);
        d.select(Some("attic"));
        assert_eq!(d.boxes(None).selected, None);
        // another seed, other lots: the seed's own are worked out again, the made room on them
        d.set_seed(7);
        let other = d.lots().unwrap();
        assert!(other.lots.iter().any(|a| a.key == key) && other.lots[0] != placed.lots[0]);
        d.undo();
        assert_eq!(d.lots().unwrap().lots.len(), placed.lots.len() + 1);
    }

    #[test]
    fn a_choice_for_a_room_is_one_edit_however_often_it_is_changed() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        assert_eq!(d.choices("vast").unwrap_err(), "no level has been made yet");
        d.regenerate().unwrap();
        let was = d.choices("vast").unwrap();
        assert!(was.chosen.is_empty() && was.unfurnished.is_none() && was.kind == "vast");
        // chosen, and chosen again: one edit, two steps
        d.set_choice(Edit::Cover { room: "vast".into(), cover: "pillars".into() }).unwrap();
        d.set_choice(Edit::Cover { room: "vast".into(), cover: "none".into() }).unwrap();
        assert_eq!((d.recipe.edits.clone(), d.rev()), (vec![Edit::Cover { room: "vast".into(), cover: "none".into() }], 2));
        // the same again is no step
        d.set_choice(Edit::Cover { room: "vast".into(), cover: "none".into() }).unwrap();
        assert_eq!(d.rev(), 2);
        // what it has is read from the last level made until the new one is; what was chosen, from the recipe
        let stale = d.choices("vast").unwrap();
        assert_eq!((stale.cover, stale.chosen.cover.as_deref()), (was.cover, Some("none")));
        d.regenerate().unwrap();
        let now = d.choices("vast").unwrap();
        assert_eq!((now.cover, now.pieces), ("none", 0));
        // refused before anything is made, in plain words, and nothing changed
        let why = d.set_choice(Edit::Cover { room: "small8".into(), cover: "none".into() }).unwrap_err();
        assert_eq!(why, "cover: small8 is a pod of vast: it is not furnished by itself");
        assert_eq!(d.set_choice(Edit::Layout { room: "tower".into(), layout: "ring".into() }).unwrap_err(), "layout: tower is a tower: its decks are its own");
        assert_eq!(d.set_choice(Edit::Brightness { room: "vast".into(), value: -1 }).unwrap_err(), "brightness: 0 to 255, not -1");
        assert_eq!(d.set_choice(Edit::Keep { room: "vast".into() }).unwrap_err(), "keep: not a choice for one room");
        assert_eq!(d.set_choice(Edit::Cover { room: "attic".into(), cover: "none".into() }).unwrap_err(), "cover: no room attic");
        assert_eq!((d.recipe.edits.len(), d.rev()), (1, 2));
        // brightness, a ceiling, another inside: the level has them
        d.set_choice(Edit::Brightness { room: "vast".into(), value: 90 }).unwrap();
        d.set_choice(Edit::Ceiling { room: "vast".into(), ceiling: "grille".into() }).unwrap();
        d.set_choice(Edit::Reroll { room: "colossal".into(), n: 1, part: None }).unwrap();
        d.regenerate().unwrap();
        let now = d.choices("vast").unwrap();
        assert_eq!((now.brightness, now.ceiling), (Some(90), Some("grille")));
        assert_eq!(d.choices("colossal").unwrap().chosen.reroll, Some(1));
        // the plan is told what was chosen, room by room, in a few words
        assert_eq!(
            d.look(false, None).chosen,
            [("vast".to_string(), "cover none, brightness 90, ceiling grille".to_string()), ("colossal".to_string(), "inside 1".to_string())]
        );
        // the seed's own again: the edit goes; nothing chosen, nothing to clear
        assert!(d.clear_choice("vast", "cover").unwrap() && !d.clear_choice("vast", "cover").unwrap() && !d.clear_choice("tower", "layout").unwrap());
        d.set_choice(Edit::Reroll { room: "colossal".into(), n: 0, part: None }).unwrap();
        assert_eq!(d.recipe.edits.iter().map(Edit::name).collect::<Vec<_>>(), ["brightness", "ceiling"]);
        assert!(d.undo() && d.recipe.edits.len() == 3);
    }

    #[test]
    fn a_search_finds_the_joins_on_a_thread() {
        let mut d = Doc::new(Recipe::before_bases(40), true);
        d.pick("tower", 551);
        let search = crate::jobs::Search::spawn(d.recipe.clone(), d.pick_asked().unwrap(), d.rev());
        let result = loop {
            match search.poll() {
                Some(result) => break result,
                None => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        };
        assert_eq!((search.rev(), search.pick.0.as_str(), search.pick.1), (0, "tower", 551));
        assert!(d.accept_joins(search.rev(), &search.pick, result));
        assert_eq!(**d.joins().unwrap(), *crate::fixture::tower_joins());
    }
}
