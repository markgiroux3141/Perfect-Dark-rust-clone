//! The editor's window: a left panel (the seed, the tool in hand, the options, the recipe's
//! edits, the storey filter, what the tool has hold of, and the dead-end list) and the plan in
//! the middle, drawn from the same lists of primitives `--shot` rasterises. `Editor::panel` and
//! `Editor::canvas` take an `egui::Ui`, so in `pd_edit` they become another of its tabs;
//! `Window` is only what joins them to the engine's runner (`engine::app::Game`), which owns
//! the window, the GPU and the egui pass.
//!
//! In the plan, whatever the tool: scroll to zoom, double-click to fit, and a drag that starts
//! on nothing the tool takes hold of pans. Ctrl+Z undoes, Ctrl+Y or Ctrl+Shift+Z does again
//! (not while a text field has the keys). Every change makes the level again on a thread.
//!
//! With "Join" in hand a drag pans wherever it starts. Click a room to pick it: the corridors
//! that could join it to the rooms near it are found on a thread, drawn on the plan as pale
//! ghosts with a number each and listed in the panel with a "join" button each (the storey its
//! door is on is the one shown, or chosen in the panel; hovering a row lights its ghost). Click
//! a corridor to pick it, and "take it out" removes it, or says why it can't be. In the
//! dead-end list: click a row to pick it out on the plan; a "could join" row's button adds that
//! join to the recipe, a long dead end's "keep" leaves it alone.
//!
//! With "Rooms" in hand the plan shows the room boxes as the recipe places them (`Doc::boxes`),
//! over the level, or alone where there is none. Click a box to select it, and on nothing to
//! select nothing. Drag a box to move it, by whole cells; drag a side or a corner of the
//! selected one to pull it. While the button is down the box follows the pointer as a trial
//! (`Doc::try_change`: nothing changes), and one that would break a rule is shown refused, the
//! words in red in the panel. Letting go makes the change, one edit and one step to undo, or,
//! if it is refused, leaves the box where it was and says why. Escape lets a drag go with
//! nothing done. Delete or Backspace takes the selected room out. The panel's "Rooms" section
//! has the selected room's box as four numbers to type, its floor as a button a storey (one it
//! can't go to is greyed, and says why under the pointer), "Delete room", "New room" (a kind, a
//! floor, a top, and "Draw it on the plan": the next drag, wherever it starts, rubber-bands the
//! room, and Escape or the button again leaves that), and the list of rooms to select from.
//! The command line's `--rooms`, or `--select ROOM`, opens the window with this tool in hand.
//!
//! Under the selected room's box, once a level has been made, "This room's inside" has its
//! choices (`Doc::choices`, `set_choice`, `clear_choice`), a row each: its layout, its cover,
//! another inside (and back), its brightness (a slider that makes one edit when it is let go)
//! and its ceiling. In each row the one chosen is lit, "the seed's own" (or "the usual", "the
//! kit's own") takes the choice out again, what the room has in the level on show is said
//! beside it, and what the room can't take is greyed, with why under the pointer. A choice is
//! one edit and one step to undo, and a room keeps one edit of a kind however often it is
//! changed. If the recipe then makes no level, "undo that" by the reason takes the choice back.

use crate::cli::{self, Args};
use crate::controls::{self, Control};
use crate::deadlist::{Kind, Made, Row};
use crate::doc::Doc;
use crate::jobs::{Job, Search};
use crate::planview::{self, colour, Boxes, Hit, Prim, Rgba, Swatch, View, P};
use crate::raster::Fit;
use crate::sideview::{self, Axis, Section, SideHit, SidePart, SideView};
use crate::{routes, shot};
use autolevel::boxes::{self, Change, Part, Trial};
use autolevel::candidates::Removal;
use autolevel::cells::Rect;
use autolevel::level::{Options, Start, COVER};
use autolevel::place::{self, Placed};
use autolevel::recipe::{self, Edit, Recipe};
use autolevel::scatter::Lot;
use autolevel::storeys::{snap_top, LEVELS};
use engine::app::{Ctx, Game};
use engine::gpu::Frame;
use engine::{egui, wgpu};

fn col(c: Rgba) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Words in the colour of what can't be: a refusal, as it was given.
fn red(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(col(colour::DEAD_END))).wrap());
}

/// How near a side of the selected box the pointer takes hold of it, in pixels: the side of
/// the grips the plan draws on it, so what is drawn and what is caught agree.
const GRIP: f32 = planview::GRIP_PX;

/// What a control of "This room's inside" asks for: a choice made, or the seed's own again for
/// one kind of choice (`choices::KINDS`).
enum Ask {
    Set(Edit),
    Clear(&'static str),
}

/// The tool in hand: what a click and a drag on the plan do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Click a room to see what it could be joined to, or a corridor to take it out.
    Join,
    /// The room boxes: select, move, pull a side, draw a new one, delete.
    Rooms,
}

/// What a drag on the plan has hold of.
enum Held {
    /// The plan itself: it pans.
    Pan,
    /// A room's box, as it was when the button went down, by this part of it, from this place
    /// (cells).
    Box { lot: Lot, part: Part, from: P },
    /// A new room, drawn from this cell.
    New { from: (i32, i32) },
    /// A room's box in the side view, as it was when the button went down, by this part of it,
    /// from this place (cells across, cells up).
    Side { lot: Lot, part: SidePart, from: P },
}

/// What the drag in hand has come to: the change, what it would make of the lots, and for a
/// room being drawn its box (the trial has none for it while it is too small to be a lot).
struct Tried {
    change: Change,
    trial: Trial,
    drawn: Option<Lot>,
}

/// What a change to the room boxes did, for the status line: the edits the recipe gained (or,
/// where it undid earlier ones, lost), what a delete took with the room, and the warning.
fn done(was: &Recipe, t: &Trial) -> String {
    // (what a delete took with the room is said apart)
    let said = |e: &&Edit| !t.dropped.contains(&e.describe());
    let words = |a: &[Edit], b: &[Edit]| a.iter().filter(|e| !b.contains(e)).filter(said).map(Edit::describe).collect::<Vec<_>>().join("; ");
    let (gained, lost) = (words(&t.recipe.edits, &was.edits), words(&was.edits, &t.recipe.edits));
    let mut said = match (gained.is_empty(), lost.is_empty()) {
        (false, _) => gained,
        (true, false) => format!("taken out of the recipe: {lost}"),
        (true, true) => "nothing to change".to_string(),
    };
    if !t.dropped.is_empty() {
        said += &format!("; with it went: {}", t.dropped.join("; "));
    }
    if let Some(warn) = &t.warn {
        said += &format!(". Look: {warn}");
    }
    said
}

/// A change as it goes, for the line under the plan: "move medium1 to 12, 30".
fn going(change: &Change) -> String {
    let span = |r: &(i32, i32, i32, i32)| format!("{}, {} to {}, {} ({} x {})", r.0, r.1, r.2, r.3, r.2 - r.0, r.3 - r.1);
    match change {
        Change::Move { room, to } => format!("move {room} to {}, {}", to.0, to.1),
        Change::Rect { room, rect } => format!("{room}: {}", span(rect)),
        Change::Add { key, kind, rect, floor, top } => format!("draw {key} ({kind}, {floor} to {top}): {}", span(rect)),
        Change::Pin { room, floor } => format!("pin {room}'s floor at {floor}"),
        Change::Height { room, floor, top } => format!("{room} from {floor} up to {top}"),
        Change::Delete { room } => format!("delete {room}"),
    }
}

/// A side or a corner of a box, by the way it faces: "east side", "north-west corner".
fn side(sx: i8, sy: i8) -> String {
    let (ew, ns) = (if sx < 0 { "west" } else { "east" }, if sy < 0 { "south" } else { "north" });
    match (sx, sy) {
        (_, 0) => format!("{ew} side"),
        (0, _) => format!("{ns} side"),
        _ => format!("{ns}-{ew} corner"),
    }
}

/// Is a lot drawn with this storey shown: its floor is there, or it is a room whose air is?
fn on(a: &Lot, storey: Option<i32>) -> bool {
    storey.is_none_or(|z| a.floor == z || (a.host.is_empty() && a.floor <= z && z < a.top))
}

/// The editor: the document and how it is being looked at.
pub struct Editor {
    pub doc: Doc,
    job: Option<Job>,
    /// The recipe's file, if it has one.
    path: Option<String>,
    /// The storey shown (None: all).
    storey: Option<i32>,
    /// The row of the dead-end list picked out.
    picked: Option<usize>,
    /// The plan's middle in cells and pixels to a cell (None: fit the level when it arrives).
    look: Option<(P, f32)>,
    seed_text: String,
    /// The option whose slider is held, and the value it is at (as `recipe::set_option` takes
    /// it): set when the slider is let go.
    typing: Option<(&'static str, String)>,
    /// What the last thing done came to, and how long the level then took to make.
    status: String,
    took: String,
    /// What the last frame drew: primitives of the plan, rows of the list.
    pub drawn: (usize, usize),
    /// What could be joined to the picked room, being found.
    search: Option<Search>,
    /// The number of the corridor that could be made whose row the pointer is on.
    hot: Option<usize>,
    /// Draw every corridor that could be made, not only each room's cheapest.
    all: bool,
    /// Whether the picked route can be taken out, for a count of the recipe's changes.
    removal: Option<(u64, String, Result<Removal, String>)>,
    /// How the last frame laid the plan on the window, and where it listed each corridor that
    /// could be made (its number, its row).
    pub fit: Option<Fit>,
    pub rows: Vec<(usize, egui::Rect)>,
    /// The tool in hand.
    pub tool: Tool,
    /// Why the room change being tried, or the last one asked for, can't be made.
    pub refused: Option<String>,
    /// Where the last frame put the panel's controls for the tool and the room boxes, by name
    /// ("tool rooms", "x", "floor 276", "delete", "draw", "kind medium", "lot medium1"): what a
    /// test aims the pointer at.
    pub spots: Vec<(String, egui::Rect)>,
    /// What the drag on the plan has hold of, and what it has come to.
    held: Option<Held>,
    tried: Option<Tried>,
    /// The next drag on the plan draws a new room.
    drawing: bool,
    /// The room "New room" makes: its kind, its floor and its top (None: the usual one).
    new: (Option<&'static str>, Option<i32>, Option<i32>),
    /// The number of the selected room's box being typed into (the room, which of x, y, cells
    /// wide and cells deep, the text).
    number: Option<(String, usize, String)>,
    /// Why the selected room's floor can't go to each storey, for a count of the recipe's
    /// changes and that room.
    pins: Option<(u64, String, Vec<Option<String>>)>,
    /// The brightness slider while it is held: the room, and the value it has come to. It is
    /// one edit when it is let go.
    bright: Option<(String, i32)>,
    /// The count of the recipe's changes just after the last choice made for a room: while the
    /// recipe is still that one and makes no level, the panel offers to undo the choice.
    chose: Option<u64>,
    /// The side view is shown in place of the plan, along this section (None: the plan).
    pub side: Option<Section>,
    /// The side view's middle (cells across, cells up) and pixels to a cell (None: fit it).
    side_look: Option<(P, f32)>,
    /// How the last frame laid the side view on the window.
    pub side_fit: Option<Fit>,
}

impl Editor {
    /// The editor as the command line asks for it: its recipe, the storey shown, and with
    /// `--rooms` or `--select ROOM` the room tool in hand (and that room's box selected).
    pub fn new(args: Args) -> Editor {
        let saved = args.from.is_some();
        let seed_text = args.recipe.seed.to_string();
        let tool = if args.draws_rooms() { Tool::Rooms } else { Tool::Join };
        let mut doc = Doc::new(args.recipe, saved);
        doc.select(args.select.as_deref());
        Editor {
            doc,
            job: None,
            path: args.from,
            storey: args.storey,
            picked: None,
            look: None,
            seed_text,
            typing: None,
            status: String::new(),
            took: String::new(),
            drawn: (0, 0),
            search: None,
            hot: None,
            all: false,
            removal: None,
            fit: None,
            rows: Vec::new(),
            tool,
            refused: None,
            spots: Vec::new(),
            held: None,
            tried: None,
            drawing: false,
            new: (None, None, None),
            number: None,
            pins: None,
            bright: None,
            chose: None,
            side: None,
            side_look: None,
            side_fit: None,
        }
        .with_side(args.side.as_ref())
    }

    /// The side view as the command line asks for it (`--side`).
    fn with_side(mut self, side: Option<&cli::Side>) -> Editor {
        let Some(side) = side else { return self };
        self.side = Some(match &side.at {
            cli::SideAt::Cell(at) => Section { axis: side.axis, at: *at },
            cli::SideAt::Room(room) => {
                let rect = self.doc.lots().ok().and_then(|p| p.lots.into_iter().find(|a| a.key == *room)).map(|a| a.rect());
                rect.map_or(Section { axis: side.axis, at: 0 }, |r| Section::through(side.axis, r))
            }
        });
        self.tool = Tool::Rooms;
        self
    }

    /// The selected room's box, as the recipe places it.
    fn selected_rect(&mut self) -> Option<Rect> {
        let key = self.doc.selected()?.to_string();
        self.doc.lots().ok()?.lots.into_iter().find(|a| a.key == key).map(|a| a.rect())
    }

    /// Show the side view (along x through the selected room's middle, or where it last cut)
    /// or the plan again. The side view's drags are the room tool's, so it is put in hand.
    pub fn set_side(&mut self, on: bool) {
        self.drop_drag();
        if !on {
            self.side = None;
            return;
        }
        let rect = self.selected_rect();
        let level = self.doc.made().map(|m| planview::bounds(&m.level));
        let was = self.side.unwrap_or(Section { axis: Axis::X, at: level.map_or(0, |(lo, hi)| ((lo.1 + hi.1) / 2.0) as i32) });
        self.side = Some(rect.map_or(was, |r| Section::through(was.axis, r)));
        self.side_look = None;
        self.set_tool(Tool::Rooms);
    }

    /// The switch between the plan and the side view, and with the side view shown, which way it
    /// cuts and where: a row each, `spots` named "view top", "view side", "side turn", "side
    /// back", "side on", "side selected".
    pub fn side_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("view");
            let top = ui.selectable_label(self.side.is_none(), "Top").on_hover_text("the plan, from above");
            let side = ui
                .selectable_label(self.side.is_some(), "Side")
                .on_hover_text("a section through the level, height upwards: drag a room up or down, or its top or bottom");
            self.spots.push(("view top".to_string(), top.rect));
            self.spots.push(("view side".to_string(), side.rect));
            if top.clicked() && self.side.is_some() {
                self.set_side(false);
            }
            if side.clicked() && self.side.is_none() {
                self.set_side(true);
            }
        });
        let Some(s) = self.side else { return };
        ui.horizontal(|ui| {
            let (back, on) = if s.axis == Axis::X { ("south", "north") } else { ("west", "east") };
            let turn = ui.button(if s.axis == Axis::X { "along x" } else { "along y" }).on_hover_text("cut the other way");
            let b = ui.button("<").on_hover_text(format!("cut a cell further {back}"));
            ui.label(format!("{} = {}", if s.axis == Axis::X { "y" } else { "x" }, s.at));
            let f = ui.button(">").on_hover_text(format!("cut a cell further {on}"));
            let sel = ui.add_enabled(self.doc.selected().is_some(), egui::Button::new("through the selected room"));
            for (name, r) in [("side turn", &turn), ("side back", &b), ("side on", &f), ("side selected", &sel)] {
                self.spots.push((name.to_string(), r.rect));
            }
            if turn.clicked() {
                self.turn_side();
            }
            if b.clicked() {
                self.nudge_side(-1);
            }
            if f.clicked() {
                self.nudge_side(1);
            }
            if sel.clicked() {
                self.side_through_selected();
            }
        });
    }

    /// The side view cuts the other way, through the selected room's middle if there is one.
    pub fn turn_side(&mut self) {
        let rect = self.selected_rect();
        if let Some(s) = self.side {
            self.side = Some(s.turned(rect));
            self.side_look = None;
        }
    }

    /// The side view's cut moves `by` cells across the plan (north or east).
    pub fn nudge_side(&mut self, by: i32) {
        if let Some(s) = &mut self.side {
            s.at += by;
        }
    }

    /// The side view's cut goes through the selected room's middle.
    pub fn side_through_selected(&mut self) {
        if let (Some(s), Some(r)) = (self.side, self.selected_rect()) {
            self.side = Some(Section::through(s.axis, r));
        }
    }

    /// Take a finished level, and start making the next if the recipe has changed.
    pub fn tick(&mut self) {
        if let Some(result) = self.job.as_ref().and_then(Job::poll) {
            let job = self.job.take().expect("a job was polled");
            if self.doc.accept(job.rev, result) {
                self.picked = None;
                self.took = format!("made in {:.2} s", job.started.elapsed().as_secs_f32());
            }
        }
        if self.job.is_none() && self.doc.stale() {
            self.job = Some(Job::spawn(self.doc.recipe.clone(), self.doc.rev()));
        }
        if let Some(result) = self.search.as_ref().and_then(Search::poll) {
            let search = self.search.take().expect("a search was polled");
            // (not taken if the pick or the recipe has changed since: it is looked for again)
            self.doc.accept_joins(search.rev(), &search.pick, result);
        }
        if self.search.is_none() && self.doc.wants_joins() {
            if let Some((room, storey)) = self.doc.picked() {
                self.search = Some(Search::spawn(self.doc.recipe.clone(), (room.to_string(), storey), self.doc.rev()));
            }
        }
    }

    /// An empty start (no rooms but the ones drawn; the seed only decorates them), or the
    /// seed's own rooms: what the panel's "empty" switch does.
    pub fn set_empty(&mut self, empty: bool) {
        let gone = self.doc.set_start(if empty { Start::Empty } else { Start::Seed });
        self.drop_drag();
        self.look = None; // (framed again: on the site, or on the seed's level)
        if empty {
            self.set_tool(Tool::Rooms);
        }
        let begun = if empty { "an empty start: draw a room (New room, below)" } else { "the seed's own rooms" };
        self.status = match gone {
            0 => begun.to_string(),
            n => format!("{begun}; {n} edits set aside (Undo brings them back)"),
        };
    }

    /// Is the level on show the recipe's own (made, or found impossible), and anything picked
    /// looked into?
    pub fn settled(&self) -> bool {
        self.job.is_none() && !self.doc.stale() && self.search.is_none() && !self.doc.wants_joins()
    }

    /// A click on the plan at `at` (cells): a room is picked to join from (on the storey shown,
    /// if it takes doors there, or else its own floor), a corridor is picked, and a click on
    /// nothing picks nothing.
    pub fn click(&mut self, at: P) {
        let Some(made) = self.doc.made().cloned() else { return };
        self.hot = None;
        match planview::hit(&made.level, at, self.storey) {
            Some(Hit::Room(key)) => {
                let s = made.level.space(&key);
                if s.host.is_empty() {
                    self.doc.pick(&key, planview::pick_storey(s, self.storey));
                    self.status.clear();
                } else {
                    self.status = format!("{key} is a pod: it is joined inside {}", s.host);
                }
            }
            Some(Hit::Route(key)) => {
                self.doc.pick_route(&key);
                self.status.clear();
            }
            None => self.doc.unpick(),
        }
    }

    // ------------------------------------------------------------ room boxes

    /// Take a tool in hand. What the other had hold of is let go: a room picked to join from
    /// (its corridors would be looked for again after every change to the boxes), a drag, a
    /// room being drawn.
    pub fn set_tool(&mut self, tool: Tool) {
        if tool == self.tool {
            return;
        }
        self.tool = tool;
        self.doc.unpick();
        self.hot = None;
        self.drop_drag();
        self.drawing = false;
    }

    /// Let the drag in hand go with nothing done.
    fn drop_drag(&mut self) {
        (self.held, self.tried, self.refused) = (None, None, None);
    }

    /// Select a room's box (None: none). With one storey shown that the room is not on, its
    /// own floor is shown instead, so the box selected is one that can be seen.
    pub fn select(&mut self, room: Option<&str>) {
        self.doc.select(room);
        self.refused = None;
        let lot = room.and_then(|key| self.doc.lots().ok().and_then(|p| p.lots.into_iter().find(|a| a.key == key)));
        if let Some(a) = lot.filter(|a| !on(a, self.storey)) {
            self.storey = Some(a.floor);
        }
    }

    /// Make a change to the room boxes, and say what was done, or why it can't be (nothing is
    /// then changed). True if it was made.
    pub fn change(&mut self, change: &Change) -> bool {
        let was = self.doc.recipe.clone();
        match self.doc.change(change) {
            Ok(t) => {
                self.status = done(&was, &t);
                // (the room is selected now: keep it in sight if its floor has changed)
                let room = self.doc.selected().map(str::to_string);
                self.select(room.as_deref());
                true
            }
            Err(why) => {
                self.status = why.clone();
                self.refused = Some(why);
                false
            }
        }
    }

    /// Make a choice for one room (a `layout`, `cover`, `reroll`, `brightness` or `ceiling`
    /// edit: the room's one edit of its kind), and say what was chosen, or in red why it can't
    /// be (nothing is then changed).
    pub fn set_choice(&mut self, e: Edit) {
        let (said, rev) = (e.describe(), self.doc.rev());
        match self.doc.set_choice(e) {
            Ok(()) => {
                self.refused = None;
                // (the choice it already had is no change, and nothing to say)
                if self.doc.rev() != rev {
                    self.status = said;
                    self.chose = Some(self.doc.rev());
                }
            }
            Err(why) => {
                self.status = why.clone();
                self.refused = Some(why);
            }
        }
    }

    /// The seed's own again for one kind of a room's choices (`choices::KINDS`).
    pub fn clear_choice(&mut self, room: &str, what: &str) {
        if self.doc.clear_choice(room, what) {
            self.refused = None;
            self.status = format!("{room}: the seed's own {} again", if what == "reroll" { "inside" } else { what });
            self.chose = Some(self.doc.rev());
        }
    }

    /// The room "New room" makes as things stand: its kind, floor and top. The floor is the
    /// storey shown (or 0) and the room one storey tall, until others are chosen.
    fn new_room(&self, placed: &Placed) -> (&'static str, i32, i32) {
        let kinds = boxes::kinds(placed);
        let kind = self.new.0.filter(|k| kinds.contains(k)).unwrap_or(if kinds.contains(&"medium") { "medium" } else { kinds[0] });
        let floor = self.new.1.or(self.storey).unwrap_or(0);
        let tops = boxes::tops(floor);
        (kind, floor, self.new.2.filter(|top| tops.contains(top)).unwrap_or(tops[0]))
    }

    /// A click on the plan at `at` (cells) with the room tool: the box there is selected, and
    /// a click on nothing selects nothing. `grip` is how near a side of the selected box
    /// still counts as it, in cells.
    pub fn click_box(&mut self, at: P, grip: f32) {
        if self.drawing {
            self.status = "drag on the plan to draw the room (Escape leaves)".to_string();
            return;
        }
        let lots = self.doc.lots().map_or(Vec::new(), |p| p.lots);
        let hit = planview::hit_box(&lots, at, self.storey, self.doc.selected(), grip);
        self.select(hit.as_ref().map(|h| h.key.as_str()));
    }

    /// The button went down at `from` (cells) and a drag has begun: what it has hold of. With
    /// the room tool that is a new room if one is to be drawn, or the box there (selected, if it
    /// was not: the same drag moves it), by a side or a corner if it is the selected one's; a
    /// drag that starts on nothing, or with another tool in hand, pans.
    fn hold(&mut self, from: P, placed: Option<&Placed>, grip: f32) -> Held {
        let Some(placed) = placed else { return Held::Pan };
        if self.drawing {
            return Held::New { from: (from.0.floor() as i32, from.1.floor() as i32) };
        }
        let hit = planview::hit_box(&placed.lots, from, self.storey, self.doc.selected(), grip);
        match hit.and_then(|h| placed.lots.iter().find(|a| a.key == h.key).map(|a| (a.clone(), h.part))) {
            Some((lot, part)) => {
                self.doc.select(Some(&lot.key));
                self.refused = None;
                Held::Box { lot, part, from }
            }
            None => Held::Pan,
        }
    }

    /// The drag has come to `at` (cells): the change that is so far, in whole cells, tried on
    /// the recipe with nothing changed (asked again only when the change is another).
    fn drag_to(&mut self, at: P, placed: &Placed) {
        let mut drawn = None;
        let change = match &self.held {
            Some(Held::Box { lot, part, from }) => boxes::drag(lot, *part, ((at.0 - from.0).round() as i32, (at.1 - from.1).round() as i32)),
            Some(Held::New { from }) => {
                let to = (at.0.floor() as i32, at.1.floor() as i32);
                let (x0, y0, x1, y1) = (from.0.min(to.0), from.1.min(to.1), from.0.max(to.0) + 1, from.1.max(to.1) + 1);
                let (key, (kind, floor, top)) = (boxes::new_key(placed), self.new_room(placed));
                drawn = Some(Lot { key: key.clone(), kind, x0, y0, x1, y1, floor, top, host: String::new() });
                Some(Change::Add { key, kind: kind.to_string(), rect: (x0, y0, x1, y1), floor, top })
            }
            Some(Held::Pan | Held::Side { .. }) | None => return,
        };
        if self.tried.as_ref().map(|t| &t.change) != change.as_ref() {
            self.tried = change.and_then(|change| self.doc.try_change(&change).ok().map(|trial| Tried { change, trial, drawn }));
        }
        self.refused = self.tried.as_ref().and_then(|t| t.trial.refused.clone());
    }

    /// The button is let go: the change the drag came to is made, or refused and said why. A
    /// room that was drawn is done with drawing; one refused may be drawn again.
    fn let_go(&mut self) {
        let held = self.held.take();
        match self.tried.take() {
            Some(Tried { change, .. }) => {
                if self.change(&change) && matches!(held, Some(Held::New { .. })) {
                    self.drawing = false;
                }
            }
            // (a box let go where it was picked up: nothing to do, and nothing to refuse)
            None if !matches!(held, Some(Held::Pan) | None) => self.refused = None,
            None => {}
        }
    }

    /// The room boxes for the plan: as the recipe places them, or as the drag in hand would.
    fn shown(&mut self) -> Boxes {
        let Some(Tried { trial, drawn, .. }) = &self.tried else { return self.doc.boxes(None) };
        let mut shown = self.doc.boxes(Some(trial));
        if let Some(a) = drawn {
            // (the room being drawn is the one a refusal is about; one too small to be a lot at
            // all is in no trial's lots, and is drawn all the same)
            if shown.lots.iter().all(|b| b.key != a.key) {
                shown.lots.push(a.clone());
            }
            shown.selected = Some(a.key.clone());
        }
        shown
    }

    /// What a click or a drag at `at` (cells) would do with the room tool, for the line under
    /// the plan.
    fn hint(&self, at: P, placed: &Placed, grip: f32) -> String {
        if let Some(t) = &self.tried {
            return format!("   {}{}", going(&t.change), if t.trial.refused.is_some() { ": refused" } else { "" });
        }
        if self.drawing {
            let (kind, floor, top) = self.new_room(placed);
            return format!("   drag: draw {} ({kind}, {floor} to {top})", boxes::new_key(placed));
        }
        match planview::hit_box(&placed.lots, at, self.storey, self.doc.selected(), grip) {
            Some(h) => match h.part {
                Part::Edge(sx, sy) => format!("   drag: pull {}'s {}", h.key, side(sx, sy)),
                Part::Body if self.doc.selected() == Some(h.key.as_str()) => format!("   drag: move {}", h.key),
                Part::Body => format!("   click: select {}   drag: move {}", h.key, h.key),
            },
            None => String::new(),
        }
    }

    /// Why the room's floor can't go to each storey of `LEVELS` (None: it can), asked once for
    /// each state of the recipe.
    fn pins(&mut self, room: &str) -> Vec<Option<String>> {
        if let Some((_, _, why)) = self.pins.as_ref().filter(|(rev, key, _)| *rev == self.doc.rev() && key == room) {
            return why.clone();
        }
        let ask = |doc: &mut Doc, floor: i32| match doc.try_change(&Change::Pin { room: room.to_string(), floor }) {
            Ok(t) => t.refused,
            Err(why) => Some(why),
        };
        let why: Vec<Option<String>> = LEVELS.iter().map(|&floor| ask(&mut self.doc, floor)).collect();
        self.pins = Some((self.doc.rev(), room.to_string(), why.clone()));
        why
    }

    /// The keys, when no text field has them: undo and redo; with the room tool, Delete (or
    /// Backspace) takes the selected room out, and Escape lets go of a drag and of drawing.
    fn keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().wants_keyboard_input() {
            return;
        }
        let (cmd, none) = (egui::Modifiers::COMMAND, egui::Modifiers::NONE);
        let (redo, undo, delete, escape) = ui.input_mut(|i| {
            // (the longer chord first: Ctrl+Z would take Ctrl+Shift+Z for its own)
            let redo = i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::Z) || i.consume_key(cmd, egui::Key::Y);
            let undo = i.consume_key(cmd, egui::Key::Z);
            let delete = i.consume_key(none, egui::Key::Delete) || i.consume_key(none, egui::Key::Backspace);
            (redo, undo, delete, i.consume_key(none, egui::Key::Escape))
        });
        if (undo && self.doc.undo()) || (redo && self.doc.redo()) {
            self.drop_drag();
            self.status = if undo { "undone" } else { "done again" }.to_string();
        }
        if self.tool != Tool::Rooms {
            return;
        }
        if escape {
            self.drop_drag();
            self.drawing = false;
        }
        let room = self.doc.selected().map(str::to_string);
        if let (true, Some(room)) = (delete, room) {
            if self.doc.lots().is_ok_and(|p| p.lots.iter().any(|a| a.key == room)) {
                self.change(&Change::Delete { room });
            }
        }
    }

    /// Make the `n`th corridor found for the pick (from 1): what its "join" button does.
    pub fn choose(&mut self, n: usize) {
        self.hot = None;
        self.status = match self.doc.choose(n) {
            Ok(c) => format!("joined {} on {} {}", c.room, c.storey, c.text()),
            Err(why) => why,
        };
    }

    /// Whether the picked route can be taken out (worked out once for each state of the recipe).
    fn can_remove(&mut self, key: &str) -> Result<Removal, String> {
        if self.doc.stale() {
            return Err("the level is being made again: a moment".to_string());
        }
        match &self.removal {
            Some((rev, k, result)) if *rev == self.doc.rev() && k == key => result.clone(),
            _ => {
                let result = self.doc.removal(key);
                self.removal = Some((self.doc.rev(), key.to_string(), result.clone()));
                result
            }
        }
    }

    /// Take the picked route out: what its button does.
    pub fn take_out(&mut self, key: &str) {
        self.status = match self.doc.remove_route(key) {
            Ok(done) => done.text(),
            Err(why) => why,
        };
    }

    fn save(&mut self) {
        let path = self.path.clone().unwrap_or_else(|| format!("autolevel_{}.json", self.doc.recipe.seed));
        match std::fs::write(&path, recipe::save(&self.doc.recipe)) {
            Ok(()) => {
                self.doc.mark_saved();
                self.status = format!("saved {path}");
                self.path = Some(path);
            }
            Err(e) => self.status = format!("{path}: {e}"),
        }
    }

    /// Both panels, laid out in a whole window (in `pd_edit` its tab calls `panel` and `canvas`).
    pub fn show(&mut self, egui: &egui::Context) {
        egui::SidePanel::left("recipe").resizable(true).default_width(430.0).show(egui, |ui| self.panel(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(egui, |ui| self.canvas(ui));
    }

    /// The left panel.
    pub fn panel(&mut self, ui: &mut egui::Ui) {
        self.spots.clear();
        self.keys(ui);
        ui.horizontal(|ui| {
            ui.heading("autolevel");
            if ui.add_enabled(self.doc.can_undo(), egui::Button::new("Undo")).on_hover_text("Ctrl+Z").clicked() {
                self.doc.undo();
                self.drop_drag();
            }
            if ui.add_enabled(self.doc.can_redo(), egui::Button::new("Redo")).on_hover_text("Ctrl+Y").clicked() {
                self.doc.redo();
                self.drop_drag();
            }
            if ui.button(if self.doc.dirty() { "Save recipe *" } else { "Save recipe" }).clicked() {
                self.save();
            }
        });
        ui.horizontal(|ui| {
            ui.label("tool");
            let tools = [
                (Tool::Join, "Join", "click a room to see what it could be joined to, or a corridor to take it out"),
                (Tool::Rooms, "Rooms", "the room boxes: click one to select it, drag it to move it, drag a side to pull it, draw a new one"),
            ];
            for (tool, name, does) in tools {
                let button = ui.selectable_label(self.tool == tool, name).on_hover_text(does);
                self.spots.push((format!("tool {}", name.to_lowercase()), button.rect));
                if button.clicked() {
                    self.set_tool(tool);
                }
            }
        });
        self.side_controls(ui);
        if !ui.memory(|m| m.focused().is_some()) {
            self.seed_text = self.doc.recipe.seed.to_string();
        }
        ui.horizontal(|ui| {
            ui.label("seed");
            if ui.button("<").clicked() {
                self.doc.set_seed(self.doc.recipe.seed.saturating_sub(1));
            }
            let field = ui.add(egui::TextEdit::singleline(&mut self.seed_text).desired_width(70.0));
            if field.lost_focus() {
                match self.seed_text.trim().parse() {
                    Ok(seed) => self.doc.set_seed(seed),
                    Err(_) => self.status = format!("seed: a whole number, not {}", self.seed_text),
                }
            }
            if ui.button(">").clicked() {
                self.doc.set_seed(self.doc.recipe.seed + 1);
            }
            let mut empty = self.doc.recipe.options.start == Start::Empty;
            let does = "an empty start: no rooms but the ones you draw; the seed then only decorates them";
            let switch = ui.checkbox(&mut empty, "empty").on_hover_text(does);
            self.spots.push(("empty".to_string(), switch.rect));
            if switch.changed() {
                self.set_empty(empty);
            }
        });
        if let Some(job) = &self.job {
            ui.label(format!("making the level... {:.1} s", job.started.elapsed().as_secs_f32()));
        } else if let Some(why) = self.doc.error().map(str::to_string) {
            red(ui, &format!("no level: {why}"));
            // (a choice for a room that only making the level could refuse: one press takes it back)
            if self.chose == Some(self.doc.rev()) {
                let undo = ui.button("undo that").on_hover_text("the choice just made for a room makes no level: take it back");
                self.spots.push(("undo that".to_string(), undo.rect));
                if undo.clicked() && self.doc.undo() {
                    self.status = "undone: the choice that made no level".to_string();
                }
            }
        } else if let Some(made) = self.doc.made() {
            ui.label(shot::title(made));
        }
        if !self.status.is_empty() && self.refused.as_deref() == Some(self.status.as_str()) {
            // (the last thing asked for was refused: nothing was done)
            red(ui, &self.status);
        } else if !self.status.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(&self.status).weak()).wrap());
        }
        if !self.took.is_empty() {
            ui.weak(&self.took);
        }
        ui.separator();
        // the lots as the recipe places them, for what the room tool lists
        let placed = (self.tool == Tool::Rooms).then(|| self.doc.lots());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.options(ui);
            self.edits(ui);
            self.storeys(ui, placed.as_ref().and_then(|p| p.as_ref().ok()));
            match &placed {
                Some(placed) => self.rooms(ui, placed),
                None => self.joining(ui),
            }
            self.dead_ends(ui);
            self.legend(ui);
        });
    }

    /// The level's options, each with its control (`controls`): a list, a switch or a slider. A
    /// slider makes its one edit when it is let go, as the brightness slider does.
    fn options(&mut self, ui: &mut egui::Ui) {
        let header = egui::CollapsingHeader::new("Options").show(ui, |ui| {
            let style = self.doc.recipe.options.style;
            if style != "complex" {
                ui.weak(format!("a {style} level reads only the options not greyed out"));
            }
            ui.spacing_mut().slider_width = 130.0;
            let base = Options::default();
            egui::Grid::new("options").num_columns(3).show(ui, |ui| {
                for name in recipe::OPTIONS {
                    let (control, help) = controls::control(name);
                    let now = recipe::option(&self.doc.recipe.options, name).unwrap_or_default();
                    let usual = recipe::option(&base, name).unwrap_or_default();
                    let read = controls::read_by(name, style);
                    let label = ui.add_enabled(read, egui::Label::new(name.replace('_', " ")));
                    label.on_hover_text(help).on_disabled_hover_text(format!("{help} (a {style} level does not read it)"));
                    let mut set = None;
                    ui.add_enabled_ui(read, |ui| set = self.option_control(ui, name, control, &now));
                    if now == usual {
                        ui.label("");
                    } else {
                        let reset = ui.small_button("reset").on_hover_text(format!("back to the default: {usual}"));
                        self.spots.push((format!("reset {name}"), reset.rect));
                        if reset.clicked() {
                            set = Some(usual);
                        }
                    }
                    ui.end_row();
                    if let Some(value) = set.filter(|v| *v != now) {
                        if let Err(why) = self.doc.set_option(name, &value) {
                            self.status = why;
                        }
                    }
                }
            });
        });
        self.spots.push(("options".to_string(), header.header_response.rect));
    }

    /// One option's control. Returns the value to set it to (as `recipe::set_option` takes it),
    /// if it was changed this frame: a list or a switch at once, a slider once it is let go.
    fn option_control(&mut self, ui: &mut egui::Ui, name: &'static str, control: Control, now: &str) -> Option<String> {
        // the value a slider of this option is moving through, until it is let go
        let held = self.typing.as_ref().filter(|(n, _)| *n == name).map(|(_, t)| t.clone());
        let let_go = |s: &egui::Response| !s.dragged() && !s.is_pointer_button_down_on();
        match control {
            Control::Choice(words) => {
                let mut pick = now.to_string();
                let combo = egui::ComboBox::from_id_salt(("option", name)).selected_text(&pick).width(130.0).show_ui(ui, |ui| {
                    for w in words {
                        let row = ui.selectable_value(&mut pick, w.to_string(), *w);
                        self.spots.push((format!("option {name} {w}"), row.rect));
                    }
                });
                self.spots.push((format!("option {name}"), combo.response.rect));
                (pick != now).then_some(pick)
            }
            Control::Switch => {
                let mut on = now == "true";
                let tick = ui.checkbox(&mut on, "");
                self.spots.push((format!("option {name}"), tick.rect));
                tick.changed().then(|| on.to_string())
            }
            Control::Whole { lo, hi, off } => {
                let mut v: i64 = held.as_deref().unwrap_or(now).parse().unwrap_or(lo);
                ui.horizontal(|ui| {
                    // (Never: a recipe's value outside the editor's range is shown at the end, not changed)
                    let s = ui.add(egui::Slider::new(&mut v, lo..=hi).show_value(false).clamping(egui::SliderClamping::Never));
                    self.spots.push((format!("option {name}"), s.rect));
                    if s.changed() {
                        self.typing = Some((name, v.to_string()));
                    }
                    ui.label(if off && v == 0 { "off".to_string() } else { v.to_string() });
                    let_go(&s).then(|| self.typing.take_if(|(n, _)| *n == name)).flatten().map(|(_, t)| t)
                })
                .inner
            }
            Control::Share { lo, hi, step } => {
                let mut v: f64 = held.as_deref().unwrap_or(now).parse().unwrap_or(lo);
                ui.horizontal(|ui| {
                    let s = ui.add(egui::Slider::new(&mut v, lo..=hi).step_by(step).show_value(false).clamping(egui::SliderClamping::Never));
                    self.spots.push((format!("option {name}"), s.rect));
                    if s.changed() {
                        self.typing = Some((name, controls::number(v)));
                    }
                    ui.label(controls::number(v));
                    let_go(&s).then(|| self.typing.take_if(|(n, _)| *n == name)).flatten().map(|(_, t)| t)
                })
                .inner
            }
            Control::Site => {
                // the scale's own (grown with the size) until one is given
                let own = now == "default";
                let size = |t: &str| -> Option<(i32, i32)> {
                    let (w, h) = t.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                };
                let (mut w, mut h) = held.as_deref().and_then(size).or(size(now)).unwrap_or_else(|| place::kinds_and_site(&self.doc.recipe.options.siting).1);
                ui.vertical(|ui| {
                    let mut mine = !own;
                    let flip = ui.checkbox(&mut mine, if own { format!("the scale's own: {w} x {h}") } else { "my own".to_string() });
                    self.spots.push((format!("option {name}"), flip.rect));
                    if flip.changed() {
                        // (made one's own at the size it has, so the level stays as it was until a slider moves)
                        return Some(if mine { format!("{w}x{h}") } else { "default".to_string() });
                    }
                    if own {
                        return None;
                    }
                    let (wide, deep) = (controls::SITE_WIDE, controls::SITE_DEEP);
                    let a = ui.horizontal(|ui| {
                        let s = ui.add(egui::Slider::new(&mut w, wide.0..=wide.1).show_value(false).clamping(egui::SliderClamping::Never));
                        ui.label(format!("{w} wide"));
                        s
                    });
                    let b = ui.horizontal(|ui| {
                        let s = ui.add(egui::Slider::new(&mut h, deep.0..=deep.1).show_value(false).clamping(egui::SliderClamping::Never));
                        ui.label(format!("{h} deep"));
                        s
                    });
                    let (a, b) = (a.inner, b.inner);
                    self.spots.push(("option site wide".to_string(), a.rect));
                    self.spots.push(("option site deep".to_string(), b.rect));
                    if a.changed() || b.changed() {
                        self.typing = Some((name, format!("{w}x{h}")));
                    }
                    (let_go(&a) && let_go(&b)).then(|| self.typing.take_if(|(n, _)| *n == name)).flatten().map(|(_, t)| t)
                })
                .inner
            }
        }
    }

    fn edits(&mut self, ui: &mut egui::Ui) {
        let n = self.doc.recipe.edits.len();
        egui::CollapsingHeader::new(format!("Edits ({n})")).default_open(true).show(ui, |ui| {
            let mut gone = None;
            for (i, e) in self.doc.recipe.edits.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button("x").clicked() {
                        gone = Some(i);
                    }
                    ui.label(e.describe());
                });
            }
            if let Some(i) = gone {
                self.doc.delete_edit(i);
            }
            if n == 0 {
                ui.weak("none: the seed's own level");
            }
        });
    }

    fn storeys(&mut self, ui: &mut egui::Ui, placed: Option<&Placed>) {
        let mut floors = self.doc.made().map_or(Vec::new(), |made| planview::storeys(&made.level));
        // (with the room tool the boxes' floors too: one may be where the level has nothing yet)
        floors.extend(placed.iter().flat_map(|p| p.lots.iter().map(|a| a.floor)));
        if floors.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("storey");
            ui.selectable_value(&mut self.storey, None, "all");
            for z in LEVELS.into_iter().filter(|z| floors.contains(z)) {
                ui.selectable_value(&mut self.storey, Some(z), z.to_string());
            }
        });
    }

    /// The room tool's section: any refusal, the selected room, the room "New room" makes, and
    /// every room, to select from.
    fn rooms(&mut self, ui: &mut egui::Ui, placed: &Result<Placed, String>) {
        self.rows.clear();
        self.hot = None;
        let (mut ask, mut select) = (None, None);
        egui::CollapsingHeader::new("Rooms").default_open(true).show(ui, |ui| {
            if let Some(why) = self.refused.clone() {
                red(ui, &why);
            }
            let placed = match placed {
                Ok(placed) => placed,
                Err(why) => return red(ui, &format!("the recipe's room edits can't be read: {why} (take the edit out of the list above)")),
            };
            let room = self.doc.selected().and_then(|key| placed.lots.iter().find(|a| a.key == key)).cloned();
            match room {
                Some(a) => {
                    ask = self.room(ui, placed, &a);
                    ui.separator();
                    self.inside(ui, &a.key);
                }
                None => {
                    let how = "click a room's box on the plan to select it; drag it to move it; drag a side or a corner of the selected one to pull it";
                    ui.add(egui::Label::new(egui::RichText::new(how).weak()).wrap());
                }
            }
            ui.separator();
            self.new_room_choices(ui, placed);
            ui.separator();
            select = self.room_list(ui, placed);
        });
        if let Some(key) = select {
            self.select(Some(&key));
        }
        if let Some(change) = ask {
            self.change(&change);
        }
    }

    /// The selected room: what it is, its box as four numbers to type, its floor as a button a
    /// storey, its height, and "Delete room". Returns the change asked for, if one was.
    fn room(&mut self, ui: &mut egui::Ui, placed: &Placed, a: &Lot) -> Option<Change> {
        let mut ask = None;
        let pods: Vec<&str> = placed.lots.iter().filter(|p| p.host == a.key).map(|p| p.key.as_str()).collect();
        let whose = match (a.host.is_empty(), pods.is_empty()) {
            (false, _) => format!(", a pod of {}", a.host),
            (true, false) => format!(", with {} {}", if pods.len() == 1 { "its pod" } else { "its pods" }, pods.join(", ")),
            (true, true) => String::new(),
        };
        ui.add(egui::Label::new(egui::RichText::new(format!("{}: {}{whose}", a.key, a.kind)).strong()).wrap());
        ui.horizontal(|ui| {
            let now = [a.x0, a.y0, a.x1 - a.x0, a.y1 - a.y0];
            for (n, name) in ["x", "y", "wide", "deep"].into_iter().enumerate() {
                ui.label(name);
                let was = now[n].to_string();
                let mut text = match &self.number {
                    Some((room, k, text)) if *room == a.key && *k == n => text.clone(),
                    _ => was.clone(),
                };
                let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(36.0));
                self.spots.push((name.to_string(), field.rect));
                if field.changed() {
                    self.number = Some((a.key.clone(), n, text.clone()));
                }
                if field.lost_focus() {
                    self.number = None;
                    let mut to = now;
                    match text.trim().parse() {
                        Ok(v) => to[n] = v,
                        Err(_) => self.status = format!("{name}: a whole number of cells, not {text}"),
                    }
                    // (a new corner alone is a move, which takes a hall's pods along as a drag
                    // of the box does; a new size leaves them where they are)
                    if to != now {
                        let (room, rect) = (a.key.clone(), (to[0], to[1], to[0] + to[2], to[1] + to[3]));
                        ask = Some(if n < 2 { Change::Move { room, to: (to[0], to[1]) } } else { Change::Rect { room, rect } });
                    }
                }
            }
            ui.weak("cells");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("floor");
            let pins = self.pins(&a.key);
            for (&z, why) in LEVELS.iter().zip(pins) {
                // (the floor it is on is lit; one it can't go to is greyed, and says why)
                let lit = z == a.floor;
                let button = ui.add_enabled(lit || why.is_none(), egui::SelectableLabel::new(lit, z.to_string()));
                self.spots.push((format!("floor {z}"), button.rect));
                let button = match why {
                    Some(why) if !lit => button.on_disabled_hover_text(why),
                    _ => button,
                };
                if button.clicked() && !lit {
                    ask = Some(Change::Pin { room: a.key.clone(), floor: z });
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("from {} to {}", a.floor, snap_top(a.top)));
            ui.weak("(its height is kept: the recipe has no edit for it. For another, delete the room and draw it again)");
        });
        let delete = ui.button("Delete room").on_hover_text("Delete; a hall's pods go with it, and every later edit that names any of them");
        self.spots.push(("delete".to_string(), delete.rect));
        if delete.clicked() {
            ask = Some(Change::Delete { room: a.key.clone() });
        }
        ask
    }

    /// "This room's inside": the selected room's choices, a row each (layout, cover, another
    /// inside, brightness, ceiling). The one chosen is lit, the first of a row is the seed's own
    /// again, what the room has in the level on show is said at the row's end, and what it
    /// can't take is greyed, with why under the pointer.
    fn inside(&mut self, ui: &mut egui::Ui, room: &str) {
        ui.strong("This room's inside");
        let c = match self.doc.choices(room) {
            Ok(c) => c,
            // (no level yet, or the room is not in the one on show: one just drawn)
            Err(why) => {
                ui.weak(format!("nothing to choose yet: {why}"));
                return;
            }
        };
        let mut ask = None;
        let pod = c.unfurnished.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label("layout");
            let own = ui.selectable_label(c.chosen.layout.is_none(), "the seed's own");
            self.spots.push(("own layout".to_string(), own.rect));
            if own.clicked() {
                ask = Some(Ask::Clear("layout"));
            }
            for (layout, why) in &c.layouts {
                let lit = c.chosen.layout.as_deref() == Some(layout);
                let button = ui.add_enabled(why.is_none(), egui::SelectableLabel::new(lit, *layout));
                self.spots.push((format!("layout {layout}"), button.rect));
                let button = match why {
                    Some(why) => button.on_disabled_hover_text(why),
                    None => button,
                };
                if button.clicked() && !lit {
                    ask = Some(Ask::Set(Edit::Layout { room: room.to_string(), layout: layout.to_string() }));
                }
            }
            ui.weak(format!("has: {}", c.layout.unwrap_or("none")));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("cover");
            let own = ui.add_enabled(!pod, egui::SelectableLabel::new(c.chosen.cover.is_none(), "the seed's own"));
            self.spots.push(("own cover".to_string(), own.rect));
            if own.clicked() {
                ask = Some(Ask::Clear("cover"));
            }
            for cover in COVER {
                let lit = c.chosen.cover.as_deref() == Some(cover);
                let button = ui.add_enabled(!pod, egui::SelectableLabel::new(lit, cover));
                self.spots.push((format!("cover {cover}"), button.rect));
                if button.clicked() && !lit {
                    ask = Some(Ask::Set(Edit::Cover { room: room.to_string(), cover: cover.to_string() }));
                }
            }
            if !pod {
                ui.weak(format!("has: {}, {} {}", c.cover, c.pieces, if c.pieces == 1 { "piece" } else { "pieces" }));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("inside");
            let n = c.chosen.reroll.unwrap_or(0);
            let own = ui.add_enabled(!pod, egui::SelectableLabel::new(n == 0, "the seed's own"));
            self.spots.push(("own inside".to_string(), own.rect));
            if own.clicked() {
                ask = Some(Ask::Clear("reroll"));
            }
            let another = ui.add_enabled(!pod, egui::Button::new("another")).on_hover_text("furnish it again, from other random numbers");
            self.spots.push(("inside another".to_string(), another.rect));
            if another.clicked() {
                ask = Some(Ask::Set(Edit::Reroll { room: room.to_string(), n: n + 1 }));
            }
            if n > 0 {
                let back = ui.button("back").on_hover_text("the inside before this one");
                self.spots.push(("inside back".to_string(), back.rect));
                if back.clicked() {
                    // (the one before the first is the seed's own: no edit)
                    ask = Some(if n == 1 { Ask::Clear("reroll") } else { Ask::Set(Edit::Reroll { room: room.to_string(), n: n - 1 }) });
                }
                ui.label(format!("inside {n}"));
            }
        });
        if let Some(why) = &c.unfurnished {
            ui.add(egui::Label::new(egui::RichText::new(why).weak()).wrap());
        }
        ui.horizontal(|ui| {
            ui.label("brightness");
            let own = ui.selectable_label(c.chosen.brightness.is_none(), "the usual").on_hover_text("what the level gives it");
            self.spots.push(("own brightness".to_string(), own.rect));
            if own.clicked() {
                ask = Some(Ask::Clear("brightness"));
            }
            // the slider starts from what the room has, or in the middle if the level sets none
            let has = c.chosen.brightness.or(c.brightness);
            let moving = self.bright.as_ref().filter(|(key, _)| key == room).map(|(_, v)| *v);
            let mut v = moving.or(has).unwrap_or(128);
            let slider = ui.add(egui::Slider::new(&mut v, 0..=255).show_value(false));
            self.spots.push(("brightness".to_string(), slider.rect));
            if slider.changed() {
                self.bright = Some((room.to_string(), v));
            }
            // (one edit, and one step to undo, when it is let go: not one for every value on the way)
            if !slider.dragged() && !slider.is_pointer_button_down_on() {
                if let Some((_, value)) = self.bright.take().filter(|(key, _)| key == room) {
                    ask = Some(Ask::Set(Edit::Brightness { room: room.to_string(), value }));
                }
            }
            if has.is_some() || self.bright.is_some() || matches!(ask, Some(Ask::Set(Edit::Brightness { .. }))) {
                ui.label(v.to_string());
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("ceiling");
            let own = ui.selectable_label(c.chosen.ceiling.is_none(), "the kit's own");
            self.spots.push(("own ceiling".to_string(), own.rect));
            if own.clicked() {
                ask = Some(Ask::Clear("ceiling"));
            }
            for ceiling in recipe::CEILINGS {
                let lit = c.chosen.ceiling.as_deref() == Some(ceiling);
                let button = ui.selectable_label(lit, ceiling);
                self.spots.push((format!("ceiling {ceiling}"), button.rect));
                if button.clicked() && !lit {
                    ask = Some(Ask::Set(Edit::Ceiling { room: room.to_string(), ceiling: ceiling.to_string() }));
                }
            }
            if let Some(ceiling) = c.ceiling {
                ui.weak(format!("has: {ceiling}"));
            }
        });
        ui.weak("(brightness and ceiling are for the built level: the plan shows them only as words)");
        // one row for each of the level's features, for this room alone (folded: the list of
        // rooms stays where it was)
        let open = !c.chosen.features.is_empty();
        let head = egui::CollapsingHeader::new("features (this room alone)").default_open(open).show(ui, |ui| {
            for f in &c.features {
                ui.horizontal(|ui| {
                    ui.label(f.name);
                    let auto = ui.selectable_label(f.chosen.is_none(), "level's chance");
                    let on = ui.add_enabled(f.cannot.is_none() || f.chosen == Some(true), egui::SelectableLabel::new(f.chosen == Some(true), "on"));
                    let on = match &f.cannot {
                        Some(why) => on.on_disabled_hover_text(why.as_str()),
                        None => on,
                    };
                    let off = ui.selectable_label(f.chosen == Some(false), "off");
                    for (name, r) in [("auto", &auto), ("on", &on), ("off", &off)] {
                        self.spots.push((format!("feature {} {name}", f.name), r.rect));
                    }
                    if auto.clicked() && f.chosen.is_some() {
                        ask = Some(Ask::Clear(f.name));
                    }
                    if on.clicked() && f.chosen != Some(true) {
                        ask = Some(Ask::Set(Edit::Feature { room: room.to_string(), feature: f.name.to_string(), on: true }));
                    }
                    if off.clicked() && f.chosen != Some(false) {
                        ask = Some(Ask::Set(Edit::Feature { room: room.to_string(), feature: f.name.to_string(), on: false }));
                    }
                    let has = match (f.has, f.chosen) {
                        (true, _) => "has: yes",
                        (false, Some(true)) => "has: no (it doesn't fit)",
                        (false, _) => "has: no",
                    };
                    ui.weak(has);
                });
            }
        });
        self.spots.push(("features".to_string(), head.header_response.rect));
        match ask {
            Some(Ask::Set(e)) => self.set_choice(e),
            Some(Ask::Clear(what)) => self.clear_choice(room, what),
            None => {}
        }
    }

    /// "New room": the kind, the floor and the top of the room the next drag draws, and the
    /// button that makes the next drag draw it.
    fn new_room_choices(&mut self, ui: &mut egui::Ui, placed: &Placed) {
        let (kind, floor, top) = self.new_room(placed);
        ui.strong("New room");
        ui.horizontal_wrapped(|ui| {
            ui.label("kind");
            for k in boxes::kinds(placed) {
                let button = ui.selectable_label(k == kind, k);
                self.spots.push((format!("kind {k}"), button.rect));
                if button.clicked() {
                    self.new.0 = Some(k);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("floor");
            for z in LEVELS {
                let button = ui.selectable_label(z == floor, z.to_string());
                self.spots.push((format!("new floor {z}"), button.rect));
                if button.clicked() && z != floor {
                    // (as many storeys tall as it was, or as many as there are above)
                    let storeys = boxes::tops(floor).iter().position(|t| *t == top).unwrap_or(0);
                    let tops = boxes::tops(z);
                    self.new = (self.new.0, Some(z), Some(tops[storeys.min(tops.len() - 1)]));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("top");
            for t in boxes::tops(floor) {
                let button = ui.selectable_label(t == top, t.to_string());
                self.spots.push((format!("new top {t}"), button.rect));
                if button.clicked() {
                    self.new.2 = Some(t);
                }
            }
        });
        let text = if self.drawing { "Drawing: drag on the plan (Escape leaves)" } else { "Draw it on the plan" };
        let does = "the next drag on the plan, wherever it starts, draws the room: at least 2 x 2 cells";
        let draw = ui.add(egui::Button::new(text).selected(self.drawing)).on_hover_text(does);
        self.spots.push(("draw".to_string(), draw.rect));
        if draw.clicked() {
            self.drawing = !self.drawing;
            self.drop_drag();
        }
    }

    /// Every room, by key, size and heights, a hall's pods under it: a row selects its room.
    /// Returns the room of the row clicked.
    fn room_list(&mut self, ui: &mut egui::Ui, placed: &Placed) -> Option<String> {
        let mut select = None;
        let selected = self.doc.selected().map(str::to_string);
        egui::CollapsingHeader::new(format!("All rooms ({})", placed.lots.len())).default_open(true).show(ui, |ui| {
            let mut row = |ui: &mut egui::Ui, a: &Lot, indent: &str| {
                let text = format!("{indent}{}   {} x {}   {} to {}", a.key, a.x1 - a.x0, a.y1 - a.y0, a.floor, snap_top(a.top));
                let button = ui.selectable_label(selected.as_deref() == Some(a.key.as_str()), text);
                self.spots.push((format!("lot {}", a.key), button.rect));
                if button.clicked() {
                    select = Some(a.key.clone());
                }
            };
            // (a pod whose hall is not among the lots, if there ever is one, is listed as a room)
            let hall = |a: &Lot| placed.lots.iter().any(|h| h.key == a.host);
            for a in placed.lots.iter().filter(|a| !hall(a)) {
                row(ui, a, "");
                for pod in placed.lots.iter().filter(|p| p.host == a.key) {
                    row(ui, pod, "      ");
                }
            }
        });
        select
    }

    /// What is picked on the plan: a room, with the corridors that could be made from it and a
    /// button for each, or a corridor, with a button to take it out.
    fn joining(&mut self, ui: &mut egui::Ui) {
        let Some(made) = self.doc.made().cloned() else { return };
        let mut rows = Vec::new();
        egui::CollapsingHeader::new("Join and take out").default_open(true).show(ui, |ui| {
            let picked = self.doc.picked().map(|(room, z)| (room.to_string(), z));
            let route = self.doc.picked_route().map(str::to_string);
            if picked.is_none() && route.is_none() {
                ui.weak("click a room on the plan to see what it could be joined to, or a corridor to take it out");
            }
            if let Some((room, storey)) = picked {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("join {room}, by a door on"));
                    let floors = made.level.spaces().iter().find(|s| s.key == room).map_or(Vec::new(), |s| s.door_levels());
                    for z in floors {
                        if ui.selectable_label(z == storey, z.to_string()).clicked() && z != storey {
                            self.doc.pick(&room, z);
                        }
                    }
                    if ui.small_button("x").on_hover_text("pick nothing").clicked() {
                        self.doc.unpick();
                    }
                });
                if let Some(why) = self.doc.joins_error() {
                    ui.colored_label(col(colour::DEAD_END), why);
                } else if let Some(found) = self.doc.joins().cloned() {
                    let said = found.text();
                    ui.label(said.lines().next().unwrap_or_default());
                    if found.found.len() > found.rooms {
                        ui.checkbox(&mut self.all, "draw every one of them, not only each room's cheapest");
                    }
                    let mut chosen = None;
                    for (k, c) in found.found.iter().enumerate() {
                        let n = k + 1;
                        if k == found.rooms {
                            ui.weak("the same rooms, by a door on another storey:");
                        }
                        let row = ui.horizontal(|ui| {
                            if ui.small_button("join").on_hover_text("make this corridor").clicked() {
                                chosen = Some(n);
                            }
                            let note = routes::join_note(&made.level, c).map_or(String::new(), |note| format!("  [{note}]"));
                            ui.label(format!("{n}  {}{note}", c.text()));
                        });
                        rows.push((n, row.response.rect));
                    }
                    for line in said.lines().filter(|l| l.starts_with("   -  ")) {
                        ui.add(egui::Label::new(egui::RichText::new(line.trim_start()).weak()).wrap());
                    }
                    if let Some(n) = chosen {
                        self.choose(n);
                    }
                } else {
                    ui.label("looking for corridors that fit...");
                }
            }
            if let Some(key) = route {
                match made.level.net.routes().iter().find(|r| r.key() == key) {
                    Some(r) => {
                        ui.add(egui::Label::new(format!("{key}  {}", routes::describe(r))).wrap());
                        match self.can_remove(&key) {
                            Ok(what) => {
                                if ui.button("take it out").clicked() {
                                    self.take_out(&key);
                                }
                                if let Removal::Keep { room, .. } = what {
                                    ui.weak(format!("the dead-end pass added it: taking it out keeps {room}'s dead end as it is"));
                                }
                            }
                            Err(why) => {
                                ui.add(egui::Label::new(egui::RichText::new(why).color(col(colour::DEAD_END))).wrap());
                            }
                        }
                    }
                    None => {
                        ui.weak(format!("{key}: being made..."));
                    }
                }
            }
        });
        // the row under the pointer lights its corridor on the plan
        let pointer = ui.input(|i| i.pointer.hover_pos());
        self.hot = pointer.and_then(|at| rows.iter().find(|(_, rect)| rect.contains(at)).map(|(n, _)| *n));
        self.rows = rows;
    }

    fn dead_ends(&mut self, ui: &mut egui::Ui) {
        let Some(made) = self.doc.made().cloned() else { return };
        let rows = made.dead.rows();
        self.drawn.1 = rows.len();
        egui::CollapsingHeader::new("Dead ends").default_open(true).show(ui, |ui| {
            for (n, row) in rows.iter().enumerate() {
                let Row { kind, text, .. } = row;
                let text = match row.number {
                    Some(k) => format!("{k}  {text}"),
                    None => text.clone(),
                };
                let shown = match kind {
                    Kind::Head => {
                        ui.add(egui::Label::new(egui::RichText::new(text).strong()).wrap());
                        continue;
                    }
                    Kind::Long => egui::RichText::new(format!("! {text}")).color(col(colour::DEAD_END)),
                    Kind::Allowed => egui::RichText::new(text),
                    Kind::Join => egui::RichText::new(format!("    {text}")).weak(),
                    Kind::Fix => egui::RichText::new(text).color(col(colour::CAMP)),
                };
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(self.picked == Some(n), shown).clicked() {
                        self.picked = if self.picked == Some(n) { None } else { Some(n) };
                    }
                    if let Some((room, storey, to, on, _)) = &row.join {
                        if ui.small_button("join").on_hover_text("add this join to the recipe").clicked() {
                            self.doc.join(room, *storey, to, on);
                        }
                    }
                    if *kind == Kind::Long && row.number.is_none() {
                        if let Some(room) = row.mark.as_ref().and_then(|m| m.rooms.first().or(m.decks.first().map(|(room, _)| room))) {
                            if ui.small_button("keep").on_hover_text("leave this dead end as it is").clicked() {
                                self.doc.add(Edit::Keep { room: room.clone() });
                            }
                        }
                    }
                });
            }
        });
    }

    fn legend(&mut self, ui: &mut egui::Ui) {
        let Some(made) = self.doc.made().cloned() else { return };
        let view = View { storey: self.storey, ..self.doc.look(self.all, None) };
        egui::CollapsingHeader::new("Legend").show(ui, |ui| {
            for k in planview::legend(&made.level, &view) {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 12.0), egui::Sense::hover());
                    match k.swatch {
                        Swatch::Fill => {
                            ui.painter().rect_filled(rect, 0.0, col(k.col));
                        }
                        Swatch::Line => {
                            ui.painter().line_segment([rect.left_center(), rect.right_center()], egui::Stroke::new(3.0, col(k.col)));
                        }
                        Swatch::Ring => {
                            ui.painter().circle_stroke(rect.center(), 5.0, egui::Stroke::new(2.0, col(k.col)));
                        }
                    }
                    ui.label(&k.text);
                });
            }
        });
    }

    /// The plan.
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        if let Some(s) = self.side {
            self.side_canvas(ui, s);
            return;
        }
        let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, col(colour::BACKGROUND));
        let made = self.doc.made().cloned();
        // with the room tool, the lots as the recipe places them: the boxes are drawn and taken
        // hold of whether or not there is a level of them
        let placed = if self.tool == Tool::Rooms { self.doc.lots().ok() } else { None };
        if made.is_none() && placed.is_none() {
            // (a recipe that makes no level has its room boxes all the same: the room tool shows them)
            let what = if self.doc.error().is_some() { "no level: the panel says why, and the Rooms tool shows the room boxes" } else { "making the level..." };
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, what, egui::FontId::proportional(18.0), col(colour::TITLE));
            return;
        }
        let (lo, hi) = bounds(made.as_deref(), placed.as_ref());
        if self.look.is_none() || resp.double_clicked() {
            let fit = Fit::inside(lo, hi, rect.width().max(1.0), rect.height().max(1.0));
            self.look = Some((((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0), fit.scale));
        }
        let (mut centre, mut zoom) = self.look.expect("just set");
        // north up: the plan's north-west corner in pixels, as `Fit` wants it
        let lie = |centre: P, zoom: f32| Fit::new(lo, hi, zoom, rect.center().x - (centre.0 - lo.0) * zoom, rect.center().y - (hi.1 - centre.1) * zoom);
        if resp.drag_started() {
            // (what the drag has hold of is what was under the pointer when the button went down)
            let from = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            let held = from.map(|at| self.hold(lie(centre, zoom).from((at.x, at.y)), placed.as_ref(), GRIP / zoom));
            (self.held, self.tried) = (held, None);
        }
        if resp.dragged() && matches!(self.held, Some(Held::Pan)) {
            let d = resp.drag_delta();
            centre = (centre.0 - d.x / zoom, centre.1 + d.y / zoom);
        }
        if let (true, Some(at)) = (resp.hovered(), ui.input(|i| i.pointer.hover_pos())) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                // (about the pointer: the cell under it stays under it)
                let was = zoom;
                zoom = (zoom * (1.0 + scroll * 0.002)).clamp(2.0, 400.0);
                let (dx, dy) = (at.x - rect.center().x, at.y - rect.center().y);
                centre = (centre.0 + dx / was - dx / zoom, centre.1 - dy / was + dy / zoom);
            }
        }
        self.look = Some((centre, zoom));
        let fit = lie(centre, zoom);
        self.fit = Some(fit);
        let grip = GRIP / zoom;
        if let Some(at) = resp.interact_pointer_pos().map(|at| fit.from((at.x, at.y))) {
            if resp.clicked() {
                match self.tool {
                    Tool::Join => self.click(at),
                    Tool::Rooms => self.click_box(at, grip),
                }
            }
            if let (true, Some(placed)) = (resp.dragged() || resp.drag_stopped(), &placed) {
                self.drag_to(at, placed);
            }
        }
        if resp.drag_stopped() {
            self.let_go();
        } else if self.held.is_some() && !resp.dragged() {
            // (a drag that ended where the plan was not told of it: nothing is done)
            (self.held, self.tried) = (None, None);
        }
        let to = |p: P| {
            let (x, y) = fit.to(p);
            egui::pos2(x, y)
        };
        let mark = made.as_ref().and_then(|made| self.picked.and_then(|n| made.dead.rows().get(n).and_then(|r| r.mark.clone())));
        let boxes = placed.is_some().then(|| self.shown());
        let view = View { storey: self.storey, mark, scale: zoom, boxes, ..self.doc.look(self.all, self.hot) };
        let pic = match &made {
            Some(made) => planview::picture(&made.level, &view),
            // (no level, and the room tool in hand: the boxes alone, to be put right)
            None => planview::bare(&view),
        };
        self.drawn.0 = pic.prims.len();
        let painter = painter.with_clip_rect(rect);
        paint(&painter, &pic.prims, &to, zoom);
        if let Some(at) = resp.hover_pos() {
            let (x, y) = fit.from((at.x, at.y));
            let cell = (x.floor() as i32, y.floor() as i32);
            let within = |r: (i32, i32, i32, i32)| r.0 <= cell.0 && cell.0 < r.2 && r.1 <= cell.1 && cell.1 < r.3;
            // what is there, and what a click or a drag there would do with the tool in hand
            let (under, does): (Vec<&str>, String) = match (&placed, &made) {
                (Some(placed), _) => (placed.lots.iter().filter(|a| within(a.rect())).map(|a| a.key.as_str()).collect(), self.hint((x, y), placed, grip)),
                (None, Some(made)) => {
                    let click = match planview::hit(&made.level, (x, y), self.storey) {
                        Some(Hit::Room(key)) => format!("   click: join {key} to another room"),
                        Some(Hit::Route(key)) => format!("   click: pick {key}"),
                        None => String::new(),
                    };
                    (made.level.spaces().iter().filter(|s| within(s.rect())).map(|s| s.key.as_str()).collect(), click)
                }
                (None, None) => (Vec::new(), String::new()),
            };
            let text = format!("cell {}, {}  {}{does}", cell.0, cell.1, under.join(", "));
            painter.text(rect.left_bottom() + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, text, egui::FontId::monospace(12.0), col(colour::KEY));
        }
    }
}

impl Editor {
    /// The side view in the window, along `s`: the room boxes it cuts are selected with a click,
    /// and a drag of one moves it along the section or up or down a storey, or sets its top or
    /// bottom, or pulls an end (`sideview::drag`), tried at each whole step while the button is
    /// down and made when it is let go, as on the plan. A drag on nothing pans; the wheel zooms.
    pub fn side_canvas(&mut self, ui: &mut egui::Ui, s: Section) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, col(colour::BACKGROUND));
        let made = self.doc.made().cloned();
        let placed = self.doc.lots().ok();
        let boxes = placed.is_some().then(|| self.shown());
        let view = SideView { section: Some(s), boxes, scale: 1.0 };
        let (lo, hi) = sideview::bounds(made.as_ref().map(|m| &m.level), &view);
        if self.side_look.is_none() || resp.double_clicked() {
            let fit = Fit::inside(lo, hi, rect.width().max(1.0), rect.height().max(1.0));
            let mut look = (((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0), fit.scale);
            // (a section is long and low: with a room in hand, its storeys fill most of the
            // height and the room is in the middle, the rest a pan away)
            if let Some(r) = self.selected_rect().filter(|r| s.crosses(*r)) {
                let (u0, u1) = s.span(r);
                let tall = rect.height().max(1.0) * 0.7 / (hi.1 - lo.1);
                look = (((u0 + u1) as f32 / 2.0, look.0 .1), fit.scale.max(tall.min(rect.width().max(1.0) / (u1 - u0 + 8) as f32)));
            }
            self.side_look = Some(look);
        }
        let (mut centre, mut zoom) = self.side_look.expect("just set");
        let lie = |centre: P, zoom: f32| Fit::new(lo, hi, zoom, rect.center().x - (centre.0 - lo.0) * zoom, rect.center().y - (hi.1 - centre.1) * zoom);
        let grip = GRIP / zoom;
        if resp.drag_started() {
            let from = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            let mut held = Held::Pan;
            if let (Some(at), Some(placed)) = (from, placed.as_ref()) {
                let at = lie(centre, zoom).from((at.x, at.y));
                let hit = sideview::hit(&placed.lots, &s, at, self.doc.selected(), grip);
                if let Some((lot, part)) = hit.and_then(|h| placed.lots.iter().find(|a| a.key == h.key).map(|a| (a.clone(), h.part))) {
                    self.doc.select(Some(&lot.key));
                    self.refused = None;
                    held = Held::Side { lot, part, from: at };
                }
            }
            (self.held, self.tried) = (Some(held), None);
        }
        if resp.dragged() && matches!(self.held, Some(Held::Pan)) {
            let d = resp.drag_delta();
            centre = (centre.0 - d.x / zoom, centre.1 + d.y / zoom);
        }
        if let (true, Some(at)) = (resp.hovered(), ui.input(|i| i.pointer.hover_pos())) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let was = zoom;
                zoom = (zoom * (1.0 + scroll * 0.002)).clamp(2.0, 400.0);
                let (dx, dy) = (at.x - rect.center().x, at.y - rect.center().y);
                centre = (centre.0 + dx / was - dx / zoom, centre.1 - dy / was + dy / zoom);
            }
        }
        self.side_look = Some((centre, zoom));
        let fit = lie(centre, zoom);
        self.side_fit = Some(fit);
        if let Some(at) = resp.interact_pointer_pos().map(|at| fit.from((at.x, at.y))) {
            if resp.clicked() {
                let lots = placed.as_ref().map_or(Vec::new(), |p| p.lots.clone());
                let hit = sideview::hit(&lots, &s, at, self.doc.selected(), grip);
                self.select(hit.as_ref().map(|h| h.key.as_str()));
            }
            if resp.dragged() || resp.drag_stopped() {
                if let Some(Held::Side { lot, part, from }) = &self.held {
                    let change = sideview::drag(lot, &s, *part, *from, at);
                    if self.tried.as_ref().map(|t| &t.change) != change.as_ref() {
                        self.tried = change.and_then(|change| self.doc.try_change(&change).ok().map(|trial| Tried { change, trial, drawn: None }));
                    }
                    self.refused = self.tried.as_ref().and_then(|t| t.trial.refused.clone());
                }
            }
        }
        if resp.drag_stopped() {
            self.let_go();
        } else if self.held.is_some() && !resp.dragged() {
            (self.held, self.tried) = (None, None);
        }
        // (drawn after the drag is tried, so the box is where the pointer has it)
        let boxes = placed.is_some().then(|| self.shown());
        let view = SideView { section: Some(s), boxes, scale: zoom };
        let pic = sideview::picture(made.as_ref().map(|m| &m.level), &view);
        self.drawn.0 = pic.prims.len();
        let to = |p: P| {
            let (x, y) = fit.to(p);
            egui::pos2(x, y)
        };
        let painter = painter.with_clip_rect(rect);
        paint(&painter, &pic.prims, &to, zoom);
        let line = match (&self.tried, resp.hover_pos()) {
            (Some(t), _) => format!("{}{}", going(&t.change), if t.trial.refused.is_some() { ": refused" } else { "" }),
            (None, Some(at)) => {
                let (u, v) = fit.from((at.x, at.y));
                let hit = placed.as_ref().and_then(|p| sideview::hit(&p.lots, &s, (u, v), self.doc.selected(), grip));
                let does = match hit {
                    Some(SideHit { key, part: SidePart::Top }) => format!("   drag: {key}'s top up or down"),
                    Some(SideHit { key, part: SidePart::Bottom }) => format!("   drag: {key}'s floor up or down, its top kept"),
                    Some(SideHit { key, part: SidePart::End(_) }) => format!("   drag: pull {key}'s end"),
                    Some(SideHit { key, part: SidePart::Body }) => format!("   drag: move {key} across, or up or down a floor"),
                    None => String::new(),
                };
                format!("{} {}, height {:.0}{does}", if s.axis == Axis::X { "x" } else { "y" }, u.floor(), v * sideview::UNITS)
            }
            (None, None) => String::new(),
        };
        painter.text(rect.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP, s.words(), egui::FontId::proportional(13.0), col(colour::TITLE));
        painter.text(rect.left_bottom() + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, line, egui::FontId::monospace(12.0), col(colour::KEY));
    }
}

/// Draw a picture's primitives, `to` putting a place (cells) on the window, `zoom` pixels to a cell.
fn paint(painter: &egui::Painter, prims: &[Prim], to: &dyn Fn(P) -> egui::Pos2, zoom: f32) {
    for p in prims {
        match p {
            Prim::Poly { pts, fill } => {
                painter.add(egui::Shape::convex_polygon(pts.iter().map(|&q| to(q)).collect(), col(*fill), egui::Stroke::NONE));
            }
            Prim::Line { a, b, col: c, width_px } => {
                painter.line_segment([to(*a), to(*b)], egui::Stroke::new(*width_px, col(*c)));
            }
            Prim::Circle { c, r, col: k, filled } => {
                let r = (r * zoom).max(2.0);
                if *filled {
                    painter.circle_filled(to(*c), r, col(*k));
                } else {
                    painter.circle_stroke(to(*c), r, egui::Stroke::new(1.5, col(*k)));
                }
            }
            Prim::Text { at, text, col: c, size_px, centred } => {
                let align = if *centred { egui::Align2::CENTER_CENTER } else { egui::Align2::LEFT_CENTER };
                let font = egui::FontId::proportional(size_px + 3.0);
                // (a dark copy underneath, as the snapshot edges its letters)
                painter.text(to(*at) + egui::vec2(1.0, 1.0), align, text, font.clone(), col([12, 13, 17, 220]));
                painter.text(to(*at), align, text, font, col(*c));
            }
            Prim::Badge { at, text, fill, col: c, r_px, size_px } => {
                painter.circle(to(*at), *r_px, col(*fill), egui::Stroke::new(1.5, col(*c)));
                painter.text(to(*at), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(size_px + 2.0), col(*c));
            }
        }
    }
}

/// The bounds a plan is fitted to, in cells with a margin: the level's, and with the room tool
/// in hand every box's too (one may have been put clear of the level, or there may be no level).
fn bounds(made: Option<&Made>, placed: Option<&Placed>) -> (P, P) {
    let mut all = made.map(|made| planview::bounds(&made.level));
    // (an empty start: the whole site, to draw rooms anywhere on it)
    if let Some((w, h)) = placed.filter(|p| p.blank).map(|p| p.site) {
        let site = ((-1.5, -1.5), (w as f32 + 1.5, h as f32 + 1.5));
        all = Some(all.map_or(site, |(l, h): (P, P)| ((l.0.min(site.0 .0), l.1.min(site.0 .1)), (h.0.max(site.1 .0), h.1.max(site.1 .1)))));
    }
    for a in placed.iter().flat_map(|p| &p.lots) {
        let (lo, hi) = ((a.x0 as f32 - 1.5, a.y0 as f32 - 1.5), (a.x1 as f32 + 1.5, a.y1 as f32 + 1.5));
        all = Some(all.map_or((lo, hi), |(l, h)| ((l.0.min(lo.0), l.1.min(lo.1)), (h.0.max(hi.0), h.1.max(hi.1)))));
    }
    all.unwrap_or(((0.0, 0.0), (1.0, 1.0)))
}

/// The editor in a window of its own.
pub struct Window {
    ed: Editor,
    /// Close after this many frames of a settled level (`--frames`).
    frames: Option<u32>,
    shown: u32,
}

impl Window {
    pub fn new(args: Args) -> Window {
        let frames = args.frames;
        Window { ed: Editor::new(args), frames, shown: 0 }
    }
}

impl Game for Window {
    fn tick(&mut self, _ctx: &mut Ctx) {
        self.ed.tick();
    }

    /// The panels cover the window, so there is nothing of the editor's own to draw yet (the
    /// 3D view will be): the frame is only cleared, to the plan's background.
    fn render(&mut self, _ctx: &mut Ctx, frame: &mut Frame) {
        let [r, g, b, _] = colour::BACKGROUND.map(|v| v as f64 / 255.0);
        frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("autolevel_edit clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    fn debug_ui(&mut self, ctx: &mut Ctx, egui: &egui::Context) {
        self.ed.show(egui);
        let title = format!("autolevel_edit: seed {}{}", self.ed.doc.recipe.seed, if self.ed.doc.dirty() { " *" } else { "" });
        ctx.window.set_title(&title);
        let Some(frames) = self.frames else { return };
        if self.ed.settled() {
            self.shown += 1;
        }
        if self.shown >= frames {
            let what = match (self.ed.doc.made(), self.ed.doc.error()) {
                (_, Some(why)) => format!("no level: {why}"),
                (Some(made), None) => shot::title(made),
                (None, None) => "no level".to_string(),
            };
            println!(
                "window: {} frames of a settled level; the plan drew {} primitives, the list {} rows; {what}",
                self.shown, self.ed.drawn.0, self.ed.drawn.1
            );
            ctx.exit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;

    /// An editor on a command line, its level made (by `Options::python`: the Rust's own rooms on by
    /// default left out, so a seed's level is the one these tests pin).
    fn editor(line: &str) -> Editor {
        let words: Vec<String> = format!("{line} --flooded 0 --shaped 0 --sunken 0 --no-bases --no-big-pods").split_whitespace().map(str::to_string).collect();
        let mut ed = Editor::new(cli::parse(&words, &|path| Err(format!("{path}: no files here"))).unwrap());
        ed.tick(); // (starts the job)
        while !ed.settled() {
            std::thread::sleep(std::time::Duration::from_millis(5));
            ed.tick();
        }
        ed
    }

    /// One pass of the panels in a window 1500 by 950 that nobody opened, with these events;
    /// the texts drawn in it.
    fn frame(ed: &mut Editor, egui: &egui::Context, events: Vec<egui::Event>) -> Vec<String> {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1500.0, 950.0));
        let out = egui.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |egui| ed.show(egui));
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// Two passes, with these events in the second; the texts drawn in that one.
    fn pass(ed: &mut Editor, egui: &egui::Context, events: Vec<egui::Event>) -> Vec<String> {
        frame(ed, egui, Vec::new());
        frame(ed, egui, events)
    }

    /// The pointer goes to a place in the window, and the button is pressed and let go there.
    fn click(ed: &mut Editor, egui: &egui::Context, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(ed, egui, vec![egui::Event::PointerMoved(at)]);
        frame(ed, egui, vec![button(true)]);
        frame(ed, egui, vec![button(false)]);
    }

    /// Wait for the level, and for what could be joined to the pick.
    fn settle(ed: &mut Editor) {
        ed.tick();
        while !ed.settled() {
            std::thread::sleep(std::time::Duration::from_millis(5));
            ed.tick();
        }
    }

    fn has(said: &[String], what: &str) -> bool {
        said.iter().any(|t| t.contains(what))
    }

    /// An editor on seed 40 with the room tool in hand, drawn once.
    fn rooms() -> (Editor, egui::Context) {
        let (mut ed, egui) = (editor("--seed 40"), egui::Context::default());
        ed.set_tool(Tool::Rooms);
        pass(&mut ed, &egui, Vec::new());
        (ed, egui)
    }

    /// Where a place on the plan (cells) is in the window.
    fn px(ed: &Editor, cell: P) -> egui::Pos2 {
        let (x, y) = ed.fit.expect("the plan was drawn").to(cell);
        egui::pos2(x, y)
    }

    /// Where the last frame put one of the panel's controls, by its name.
    fn spot(ed: &Editor, name: &str) -> egui::Pos2 {
        ed.spots.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no {name} in the panel: {:?}", ed.spots)).1.center()
    }

    /// A room's box as the recipe places it now.
    fn lot(ed: &mut Editor, key: &str) -> Lot {
        ed.doc.lots().unwrap().lots.into_iter().find(|a| a.key == key).unwrap_or_else(|| panic!("no lot {key}"))
    }

    /// The middle of a room's box, in cells.
    fn middle(ed: &mut Editor, key: &str) -> P {
        let a = lot(ed, key);
        ((a.x0 + a.x1) as f32 / 2.0, (a.y0 + a.y1) as f32 / 2.0)
    }

    fn button(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    /// The button goes down at a place on the plan and the pointer goes to another, half way
    /// first; it is held there. The texts drawn with it held.
    fn drag(ed: &mut Editor, egui: &egui::Context, from: P, to: P) -> Vec<String> {
        let (a, b) = (px(ed, from), px(ed, to));
        frame(ed, egui, vec![egui::Event::PointerMoved(a)]);
        frame(ed, egui, vec![button(a, true)]);
        frame(ed, egui, vec![egui::Event::PointerMoved(a + (b - a) / 2.0)]);
        frame(ed, egui, vec![egui::Event::PointerMoved(b)]);
        // (the panel is laid out before the plan: it says what the drag has come to a frame on)
        frame(ed, egui, Vec::new())
    }

    /// The button is let go at a place on the plan. The texts drawn the frame after.
    fn release(ed: &mut Editor, egui: &egui::Context, at: P) -> Vec<String> {
        let at = px(ed, at);
        frame(ed, egui, vec![button(at, false)]);
        frame(ed, egui, Vec::new())
    }

    /// A key goes down and up.
    fn key(ed: &mut Editor, egui: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) -> Vec<String> {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        frame(ed, egui, vec![event(true)]);
        frame(ed, egui, vec![event(false)])
    }

    /// Ctrl, as the window gives it on this machine.
    const CTRL: egui::Modifiers = egui::Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

    fn edits(ed: &Editor) -> Vec<String> {
        ed.doc.recipe.edits.iter().map(Edit::describe).collect()
    }

    #[test]
    fn the_panels_draw_the_plan_and_the_list_with_no_window() {
        let mut ed = editor("--seed 40");
        let egui = egui::Context::default();
        let said = pass(&mut ed, &egui, Vec::new());
        let has = |said: &[String], what: &str| said.iter().any(|t| t.contains(what));
        // the panel: the level's numbers, the storeys, the dead-end list
        assert!(has(&said, "autolevel seed 40: 29 rooms, 44 links, 16 loops, score 37.85"), "{said:?}");
        assert!(has(&said, "seed 40: 11 dead ends as it falls") && has(&said, "joined tower on 551 to colossal") && has(&said, "left: 0 too long, 10 allowed"));
        assert!(has(&said, "could join spire1 on 1102 to loft3") && has(&said, "none: the seed's own level"));
        // the plan: every primitive of the picture, names and keys among them
        let all = ed.drawn;
        assert!(all.0 > 1000 && all.1 == 35, "{all:?}");
        assert!(has(&said, "The Drydock") && has(&said, "c42") && has(&said, "camp 1"));
        // one storey is a smaller picture
        ed.storey = Some(276);
        let said = pass(&mut ed, &egui, Vec::new());
        assert!(ed.drawn.0 < all.0 && has(&said, "colossal (floor 0)") && !has(&said, "The Drydock"));
    }

    #[test]
    fn a_change_makes_the_level_again_on_a_thread() {
        let mut ed = editor("--seed 40 --keep-dead-ends");
        let made = ed.doc.made().unwrap().clone();
        assert_eq!((made.dead.fixes.as_ref(), made.dead.left.iter().filter(|e| !e.end.allowed).count()), (None, 2));
        // the list's first "could join" row, as its button adds it
        let rows = made.dead.rows();
        let (room, storey, to, on, _) = rows.iter().find_map(|r| r.join.clone()).unwrap();
        ed.doc.join(&room, storey, &to, &on);
        assert!(!ed.settled() && ed.doc.dirty());
        ed.tick();
        while !ed.settled() {
            std::thread::sleep(std::time::Duration::from_millis(5));
            ed.tick();
        }
        // the level is the recipe's: one corridor more, one long dead end fewer
        let now = ed.doc.made().unwrap().clone();
        assert!(ed.doc.error().is_none(), "{:?}", ed.doc.error());
        assert_eq!(now.level.net.routes().len(), made.level.net.routes().len() + 1);
        assert_eq!(now.dead.left.iter().filter(|e| !e.end.allowed).count(), 1);
        let egui = egui::Context::default();
        let said = pass(&mut ed, &egui, Vec::new());
        assert!(said.iter().any(|t| t.contains(&format!("join {room} on {storey} to {to}"))), "{said:?}");
        // and undo is the level as it was, made again
        ed.doc.undo();
        ed.tick();
        while !ed.settled() {
            std::thread::sleep(std::time::Duration::from_millis(5));
            ed.tick();
        }
        assert_eq!(ed.doc.made().unwrap().level.net.routes().len(), made.level.net.routes().len());
    }

    #[test]
    fn a_click_picks_a_room_its_joins_are_listed_and_one_is_made() {
        let mut ed = editor("--seed 40");
        let egui = egui::Context::default();
        let said = pass(&mut ed, &egui, Vec::new());
        let has = |said: &[String], what: &str| said.iter().any(|t| t.contains(what));
        assert!(has(&said, "click a room on the plan to see what it could be joined to"), "{said:?}");
        // where the tower's middle is in the window: a click there picks it, on its own floor
        let made = ed.doc.made().unwrap().clone();
        let at = |ed: &Editor, cell: P| {
            let (x, y) = ed.fit.expect("the plan was drawn").to(cell);
            egui::pos2(x, y)
        };
        let (x0, y0, x1, y1) = made.level.space("tower").rect();
        let tower = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
        let there = at(&ed, tower);
        click(&mut ed, &egui, there);
        assert_eq!(ed.doc.picked(), Some(("tower", 0)));
        assert!(!ed.settled() && has(&pass(&mut ed, &egui, Vec::new()), "looking for corridors that fit..."));
        // with one storey shown, a room that takes doors there is picked on that storey
        ed.storey = Some(551);
        let there = at(&ed, tower);
        click(&mut ed, &egui, there);
        assert_eq!(ed.doc.picked(), Some(("tower", 551)));
        settle(&mut ed);
        let found = ed.doc.joins().expect("the search is done").clone();
        ed.storey = None;
        let said = pass(&mut ed, &egui, Vec::new());
        // the panel: the storeys to choose from, the list with a button a row, what can't be
        assert!(has(&said, "join tower, by a door on") && has(&said, "tower on 551 could be joined to 9 rooms (21 ways):"), "{said:?}");
        assert!(has(&said, "3  to colossal on 551 by a corridor, 6 cells (c39)  [lies where the dead-end pass now adds c42]"));
        assert!(has(&said, "the same rooms, by a door on another storey:") && has(&said, "no corridor fits to: long on 0; tiny3 on 276"));
        // (a button a row; the dead-end list has "join" buttons of its own)
        assert!(said.iter().filter(|t| *t == "join").count() >= 21 && ed.rows.len() == 21);
        // the plan: the pick's label, and a numbered disc for each room's cheapest
        assert!(has(&said, "joining tower by a door on 551"));
        let plain = planview::picture(&made.level, &View::default()).prims.len();
        assert!(ed.drawn.0 > plain + 9 * 3);
        // the pointer on a row lights that corridor (drawn though it is not a room's cheapest)
        let (n, row) = ed.rows[13];
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(row.center())]);
        assert_eq!((n, ed.hot), (14, Some(14)));
        let lit = ed.doc.look(ed.all, ed.hot);
        assert_eq!(lit.joins.len(), 10);
        let there = at(&ed, tower);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(there)]);
        assert_eq!(ed.hot, None);
        // "join" on the fourth row: the edit is in the list, and the level is made with it
        ed.choose(4);
        assert!(ed.doc.picked().is_none() && ed.doc.picked_route() == Some("c39"));
        settle(&mut ed);
        let said = pass(&mut ed, &egui, Vec::new());
        assert!(has(&said, "join tower on 551 to medium5 (on 276)") && has(&said, "joined tower on 551 to medium5 on 276 by a corridor"), "{said:?}");
        let now = ed.doc.made().unwrap().clone();
        let r = now.level.net.routes().iter().find(|r| r.key() == "c39").unwrap();
        assert_eq!(*r, found.found[3].route);
        // the new corridor is the one picked, and can be taken out again
        assert!(has(&said, "c39  tower on 551 - medium5 on 276: a corridor, 9 cells with 1 ramp") && has(&said, "take it out"), "{said:?}");
        // a click on a corridor picks it: one that is the only way to its room says so
        let lone = now.level.net.routes().iter().find(|r| r.key() == "c17").unwrap();
        let (_, cx, cy) = *autolevel::candidates::footprint(lone).iter().next().unwrap();
        let there = at(&ed, (cx as f32 + 0.5, cy as f32 + 0.5));
        click(&mut ed, &egui, there);
        assert_eq!(ed.doc.picked_route(), Some("c17"));
        let said = pass(&mut ed, &egui, Vec::new());
        assert!(has(&said, "c17 is the only way to small7: without it the level is in two") && !has(&said, "take it out"), "{said:?}");
        // "take it out" on the corridor just made: the recipe says so, and the level is without it
        ed.take_out("c39");
        settle(&mut ed);
        assert_eq!(ed.doc.recipe.edits.iter().map(Edit::name).collect::<Vec<_>>(), ["join", "remove"]);
        assert!(ed.doc.made().unwrap().level.net.routes().iter().all(|r| *r != found.found[3].route));
        assert!(has(&pass(&mut ed, &egui, Vec::new()), "c39 is taken out"));
        // a click on nothing picks nothing
        let (lo, _) = planview::bounds(&made.level);
        let there = at(&ed, (lo.0 + 0.3, lo.1 + 0.3));
        click(&mut ed, &egui, there);
        assert!(ed.doc.picked().is_none() && ed.doc.picked_route().is_none());
    }
    /// Click a number field of the selected room's box, type over what is in it, and press Enter.
    fn type_in(ed: &mut Editor, egui: &egui::Context, name: &str, text: &str) -> Vec<String> {
        let at = spot(ed, name);
        click(ed, egui, at);
        key(ed, egui, egui::Key::A, CTRL);
        frame(ed, egui, vec![egui::Event::Text(text.to_string())]);
        key(ed, egui, egui::Key::Enter, egui::Modifiers::NONE);
        frame(ed, egui, Vec::new())
    }

    #[test]
    fn the_command_line_puts_the_room_tool_in_hand() {
        let new = |line: &str| {
            let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
            Editor::new(cli::parse(&words, &|path| Err(format!("{path}: no files here"))).unwrap())
        };
        assert!(new("--seed 40").tool == Tool::Join && new("--seed 40 --rooms").tool == Tool::Rooms);
        // a room named is selected, in the room tool: its box's numbers are in the panel
        let mut ed = editor("--seed 40 --select medium3");
        let said = pass(&mut ed, &egui::Context::default(), Vec::new());
        assert!(ed.tool == Tool::Rooms && ed.doc.selected() == Some("medium3") && has(&said, "medium3: medium") && has(&said, "Delete room"), "{said:?}");
    }

    #[test]
    fn a_click_selects_a_box_and_a_click_on_nothing_selects_nothing() {
        let (mut ed, egui) = rooms();
        let said = frame(&mut ed, &egui, Vec::new());
        // the panel: how to, and every room, a hall's pods under it
        assert!(has(&said, "click a room's box on the plan to select it") && has(&said, "All rooms (29)"), "{said:?}");
        assert!(has(&said, "medium3   7 x 8   0 to 236") && said.iter().any(|t| t == "      small8   4 x 3   551 to 787"), "{said:?}");
        assert!(!has(&said, "click a room on the plan to see what it could be joined to"));
        // the line under the plan says what a click and a drag there would do
        let at = middle(&mut ed, "medium3");
        let there = px(&ed, at);
        let said = frame(&mut ed, &egui, vec![egui::Event::PointerMoved(there)]);
        assert!(has(&said, "medium3   click: select medium3   drag: move medium3"), "{said:?}");
        click(&mut ed, &egui, there);
        assert_eq!(ed.doc.selected(), Some("medium3"));
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "medium3: medium") && has(&said, "from 0 to 236") && has(&said, "Delete room") && has(&said, "   drag: move medium3"), "{said:?}");
        assert_eq!(ed.shown().selected.as_deref(), Some("medium3"));
        // a side of the selected box is taken hold of from just outside it too
        let a = lot(&mut ed, "medium3");
        let east = px(&ed, (a.x1 as f32 + 0.3, at.1));
        assert!(has(&frame(&mut ed, &egui, vec![egui::Event::PointerMoved(east)]), "drag: pull medium3's east side"));
        let corner = px(&ed, (a.x0 as f32, a.y1 as f32));
        assert!(has(&frame(&mut ed, &egui, vec![egui::Event::PointerMoved(corner)]), "drag: pull medium3's north-west corner"));
        // a row of the list selects its room: a hall says its pods, a pod its hall
        let row = spot(&ed, "lot vast");
        click(&mut ed, &egui, row);
        assert_eq!(ed.doc.selected(), Some("vast"));
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "vast: vast, with its pods small8, tiny2"));
        let row = spot(&ed, "lot small8");
        click(&mut ed, &egui, row);
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "small8: small, a pod of vast"));
        // with one storey shown, a room picked from the list that is not on it brings its own floor
        ed.storey = Some(0);
        ed.select(Some("loft1"));
        assert_eq!(ed.storey, Some(1102));
        ed.storey = None;
        // a click on nothing selects nothing; none of it was an edit
        let nothing = px(&ed, (70.0, 52.0));
        click(&mut ed, &egui, nothing);
        assert!(ed.doc.selected().is_none() && ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo());
    }

    #[test]
    fn a_drag_of_a_box_is_one_move_and_one_step_to_undo() {
        let (mut ed, egui) = rooms();
        let was = lot(&mut ed, "medium3");
        let at = middle(&mut ed, "medium3");
        let to = (at.0 + 2.0, at.1 + 1.0);
        // held: the box (selected by the press) follows the pointer by whole cells, and nothing is changed
        let said = drag(&mut ed, &egui, at, (at.0 + 1.6, at.1 + 1.4));
        assert_eq!(ed.doc.selected(), Some("medium3"));
        let shown = ed.shown();
        let rect = shown.lots.iter().find(|a| a.key == "medium3").unwrap().rect();
        assert_eq!((rect, shown.refused, shown.selected.as_deref()), ((was.x0 + 2, was.y0 + 1, was.x1 + 2, was.y1 + 1), None, Some("medium3")));
        assert!(has(&said, "   move medium3 to 63, 4") && ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo(), "{said:?}");
        // Escape lets it go with nothing done
        key(&mut ed, &egui, egui::Key::Escape, egui::Modifiers::NONE);
        release(&mut ed, &egui, to);
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo() && lot(&mut ed, "medium3") == was);
        // a drag of under half a cell moves nothing
        drag(&mut ed, &egui, at, (at.0 + 0.45, at.1));
        release(&mut ed, &egui, (at.0 + 0.45, at.1));
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo());
        // let go: one edit, said in the status line and listed
        drag(&mut ed, &egui, at, to);
        let said = release(&mut ed, &egui, to);
        assert_eq!((edits(&ed), ed.status.as_str(), ed.refused.as_deref()), (vec!["move medium3 to 63, 4".to_string()], "move medium3 to 63, 4", None));
        // (and the box's numbers are the new ones)
        assert!(said.iter().filter(|t| *t == "move medium3 to 63, 4").count() == 2, "{said:?}");
        assert!(said.iter().any(|t| t == "63") && said.iter().any(|t| t == "4"), "{said:?}");
        // the level made again has the room there
        settle(&mut ed);
        assert!(ed.doc.error().is_none(), "{:?}", ed.doc.error());
        assert_eq!(ed.doc.made().unwrap().level.space("medium3").rect(), (63, 4, 70, 12));
        // one step: Ctrl+Z is the seed's own recipe, Ctrl+Y and Ctrl+Shift+Z the move again
        key(&mut ed, &egui, egui::Key::Z, CTRL);
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo() && ed.doc.can_redo() && ed.status == "undone");
        key(&mut ed, &egui, egui::Key::Y, CTRL);
        assert_eq!(edits(&ed), ["move medium3 to 63, 4"]);
        key(&mut ed, &egui, egui::Key::Z, CTRL);
        key(&mut ed, &egui, egui::Key::Z, CTRL | egui::Modifiers::SHIFT);
        assert!(edits(&ed) == ["move medium3 to 63, 4"] && !ed.doc.can_redo() && ed.status == "done again");
        // dragged back where the seed put it: no edit is left, and that is a step too
        let at = middle(&mut ed, "medium3");
        drag(&mut ed, &egui, at, (at.0 - 2.0, at.1 - 1.0));
        release(&mut ed, &egui, (at.0 - 2.0, at.1 - 1.0));
        assert!(ed.doc.recipe.edits.is_empty() && ed.status == "taken out of the recipe: move medium3 to 63, 4" && lot(&mut ed, "medium3") == was);
        key(&mut ed, &egui, egui::Key::Z, CTRL);
        assert_eq!(edits(&ed), ["move medium3 to 63, 4"]);
    }

    #[test]
    fn a_drag_onto_another_room_shows_the_refusal_while_held_and_changes_nothing() {
        let (mut ed, egui) = rooms();
        let was = lot(&mut ed, "medium3");
        let at = middle(&mut ed, "medium3");
        let onto = (at.0 - 6.0, at.1);
        // held over the spire: the box is shown there, refused, with the words in the panel
        let said = drag(&mut ed, &egui, at, onto);
        let why = ed.refused.clone().expect("it lies on spire1");
        assert!(why.contains("medium3") && why.contains("spire1") && why.contains("rooms keep a cell between them"), "{why}");
        assert!(has(&said, &why) && has(&said, "   move medium3 to 55, 3: refused"), "{said:?}");
        let shown = ed.shown();
        assert_eq!((shown.lots.iter().find(|a| a.key == "medium3").unwrap().x0, shown.refused.as_deref()), (55, Some(why.as_str())));
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo());
        // on to a free place, still held: no longer refused
        let free = px(&ed, (at.0 + 1.0, at.1));
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(free)]);
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(ed.refused.is_none() && !has(&said, &why) && has(&said, "   move medium3 to 62, 3"), "{said:?}");
        // back onto the spire and let go: nothing is changed, the box is where it was, and the status says why
        let there = px(&ed, onto);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(there)]);
        let said = release(&mut ed, &egui, onto);
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo() && lot(&mut ed, "medium3") == was);
        assert_eq!((ed.status.as_str(), ed.refused.as_deref()), (why.as_str(), Some(why.as_str())));
        assert!(said.iter().filter(|t| **t == why).count() == 2, "{said:?}");
        let shown = ed.shown();
        assert!(shown.refused.is_none() && shown.lots.contains(&was));
        // the next thing done clears it
        ed.select(Some("medium3"));
        assert!(ed.refused.is_none());
    }

    #[test]
    fn a_side_pulled_is_a_resize_and_a_corner_pulls_two() {
        let (mut ed, egui) = rooms();
        ed.select(Some("medium3"));
        frame(&mut ed, &egui, Vec::new());
        let a = lot(&mut ed, "medium3");
        // the east side, two cells out (how far along it the pointer goes is no matter)
        let east = (a.x1 as f32, (a.y0 + a.y1) as f32 / 2.0);
        drag(&mut ed, &egui, east, (east.0 + 2.0, east.1 + 3.0));
        assert_eq!(ed.shown().lots.iter().find(|b| b.key == "medium3").unwrap().rect(), (a.x0, a.y0, a.x1 + 2, a.y1));
        release(&mut ed, &egui, (east.0 + 2.0, east.1 + 3.0));
        assert_eq!((edits(&ed), ed.status.as_str()), (vec!["resize medium3 to 9 x 8".to_string()], "resize medium3 to 9 x 8"));
        // the south-west corner, a cell out each way: the low corner moves and the box grows
        let corner = (a.x0 as f32, a.y0 as f32);
        drag(&mut ed, &egui, corner, (corner.0 - 1.0, corner.1 - 1.0));
        release(&mut ed, &egui, (corner.0 - 1.0, corner.1 - 1.0));
        assert_eq!(lot(&mut ed, "medium3").rect(), (a.x0 - 1, a.y0 - 1, a.x1 + 2, a.y1));
        assert_eq!(edits(&ed), ["move medium3 to 60, 2", "resize medium3 to 10 x 9"]);
        // a side pulled through the box stops at two cells
        let b = lot(&mut ed, "medium3");
        let north = ((b.x0 + b.x1) as f32 / 2.0, b.y1 as f32);
        drag(&mut ed, &egui, north, (north.0, north.1 - 20.0));
        release(&mut ed, &egui, (north.0, north.1 - 20.0));
        assert_eq!(lot(&mut ed, "medium3").rect(), (b.x0, b.y0, b.x1, b.y0 + 2));
        // three steps to undo, the box selected throughout
        assert!(ed.doc.selected() == Some("medium3") && ed.doc.undo() && ed.doc.undo() && ed.doc.undo() && !ed.doc.undo());
    }

    #[test]
    fn an_empty_start_is_drawn_on_from_nothing() {
        let (mut ed, egui) = rooms();
        ed.doc.add(Edit::Keep { room: "spire1".into() });
        pass(&mut ed, &egui, Vec::new());
        // the switch: no rooms, the edits set aside, the room tool in hand
        let at = spot(&ed, "empty");
        click(&mut ed, &egui, at);
        assert!(ed.doc.recipe.options.start == Start::Empty && ed.doc.recipe.edits.is_empty() && ed.tool == Tool::Rooms);
        assert!(ed.status.contains("1 edits set aside"), "{}", ed.status);
        settle(&mut ed);
        assert_eq!(ed.doc.error(), Some("no room yet: an empty start (draw one)"));
        // the bare plan is framed on the whole site (80 x 60 cells at the grand scale)
        pass(&mut ed, &egui, Vec::new());
        let (lo, hi) = (px(&ed, (0.0, 0.0)), px(&ed, (80.0, 60.0)));
        assert!(hi.x - lo.x > 600.0 && lo.x >= 0.0 && hi.x <= 1500.0 && lo.y >= 0.0 && hi.y <= 950.0, "{lo:?} {hi:?}");
        // a room, and a second beside it: each an add, the level made with them
        for (from, to, want) in [
            ((20.5, 20.5), (29.5, 27.5), "add room1 (medium) at 20, 20 to 30, 28, from 0 to 236"),
            ((34.5, 20.5), (43.5, 27.5), "add room2 (medium) at 34, 20 to 44, 28, from 0 to 236"),
        ] {
            let draw = spot(&ed, "draw");
            click(&mut ed, &egui, draw);
            drag(&mut ed, &egui, from, to);
            release(&mut ed, &egui, to);
            assert_eq!(edits(&ed).last().map(String::as_str), Some(want));
            settle(&mut ed);
            assert!(ed.doc.error().is_none(), "{:?}", ed.doc.error());
            pass(&mut ed, &egui, Vec::new());
        }
        let lv = &ed.doc.made().unwrap().level;
        assert_eq!(lv.spaces().len(), 2);
        assert!(!lv.net.routes().is_empty(), "the two rooms are joined");
        // undo takes the rooms out and then the empty start, the set-aside edit back with it
        for _ in 0..3 {
            ed.doc.undo();
        }
        assert!(ed.doc.recipe.options.start == Start::Seed && ed.doc.recipe.edits == [Edit::Keep { room: "spire1".into() }]);
    }

    #[test]
    fn a_room_drawn_with_new_room_is_an_add_and_is_selected() {
        let (mut ed, egui) = rooms();
        // the room it would make: a medium one on the ground, a storey tall, until others are chosen
        let placed = ed.doc.lots().unwrap();
        assert_eq!(ed.new_room(&placed), ("medium", 0, 236));
        for name in ["kind small", "new floor 276", "new top 787"] {
            let at = spot(&ed, name);
            click(&mut ed, &egui, at);
            frame(&mut ed, &egui, Vec::new());
        }
        assert_eq!(ed.new_room(&placed), ("small", 276, 787));
        // (another floor keeps it as many storeys tall)
        let at = spot(&ed, "new floor 0");
        click(&mut ed, &egui, at);
        assert_eq!(ed.new_room(&placed), ("small", 0, 512));
        let draw = spot(&ed, "draw");
        click(&mut ed, &egui, draw);
        let over = px(&ed, (66.5, 26.5));
        let said = frame(&mut ed, &egui, vec![egui::Event::PointerMoved(over)]);
        assert!(ed.drawing && has(&said, "Drawing: drag on the plan") && has(&said, "   drag: draw room1 (small, 0 to 512)"), "{said:?}");
        // a click draws nothing
        click(&mut ed, &egui, over);
        assert!(ed.drawing && ed.doc.recipe.edits.is_empty() && ed.doc.selected().is_none());
        // the drag rubber-bands whole cells: two by one is no room yet, in the generator's words, and is drawn all the same
        let said = drag(&mut ed, &egui, (66.5, 26.5), (67.5, 26.5));
        let why = ed.refused.clone().expect("2 x 1 is no room");
        assert!(
            why.contains("at least 2 x 2") && has(&said, &why) && has(&said, "   draw room1 (small, 0 to 512): 66, 26 to 68, 27 (2 x 1): refused"),
            "{why}: {said:?}"
        );
        let shown = ed.shown();
        assert_eq!((shown.lots.last().unwrap().rect(), shown.selected.as_deref(), shown.lots.len()), ((66, 26, 68, 27), Some("room1"), placed.lots.len() + 1));
        // grown to five by five on free ground: fine
        let to = px(&ed, (70.5, 30.5));
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(to)]);
        let shown = ed.shown();
        assert_eq!((shown.lots.last().unwrap().rect(), shown.refused, shown.lots.len()), ((66, 26, 71, 31), None, placed.lots.len() + 1));
        assert!(ed.doc.recipe.edits.is_empty());
        // let go: the add, the room selected, and drawing is done with
        release(&mut ed, &egui, (70.5, 30.5));
        assert_eq!(edits(&ed), ["add room1 (small) at 66, 26 to 71, 31, from 0 to 512"]);
        assert!(ed.doc.selected() == Some("room1") && !ed.drawing && ed.status == edits(&ed)[0]);
        let said = frame(&mut ed, &egui, Vec::new());
        // (its row is the list's last, below the window's edge)
        assert!(has(&said, "room1: small") && has(&said, "from 0 to 512") && has(&said, "All rooms (30)") && has(&said, "Draw it on the plan"), "{said:?}");
        assert!(ed.spots.iter().any(|(name, _)| name == "lot room1"));
        // the level is made with it
        settle(&mut ed);
        assert!(ed.doc.error().is_none(), "{:?}", ed.doc.error());
        assert_eq!(ed.doc.made().unwrap().level.space("room1").rect(), (66, 26, 71, 31));
        // (the room is in the level now, so its inside can be chosen: the panel below that has moved down)
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "This room's inside") && has(&said, "has: none") && !has(&said, "nothing to choose yet"), "{said:?}");
        // drawn over another room, the drag starting on its box: refused as it is drawn and on letting go, and still drawing
        let draw = spot(&ed, "draw");
        click(&mut ed, &egui, draw);
        let at = middle(&mut ed, "medium1");
        drag(&mut ed, &egui, at, (at.0 + 3.0, at.1 + 3.0));
        let why = ed.refused.clone().expect("it lies on medium1");
        assert!(why.contains("room2") && why.contains("medium1"), "{why}");
        release(&mut ed, &egui, (at.0 + 3.0, at.1 + 3.0));
        assert!(ed.drawing && edits(&ed).len() == 1 && ed.status == why && ed.doc.selected() == Some("room1"));
        // Escape leaves
        key(&mut ed, &egui, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!ed.drawing && ed.refused.is_none());
    }

    #[test]
    fn delete_takes_the_selected_room_out() {
        let (mut ed, egui) = rooms();
        // with nothing selected the key does nothing
        key(&mut ed, &egui, egui::Key::Delete, egui::Modifiers::NONE);
        assert!(ed.doc.recipe.edits.is_empty());
        ed.select(Some("medium3"));
        key(&mut ed, &egui, egui::Key::Delete, egui::Modifiers::NONE);
        assert_eq!((edits(&ed), ed.status.as_str(), ed.doc.selected()), (vec!["delete medium3".to_string()], "delete medium3", None));
        assert!(ed.doc.lots().unwrap().lots.iter().all(|a| a.key != "medium3"));
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "All rooms (28)") && !has(&said, "medium3   7 x 8"), "{said:?}");
        // Backspace too; and the level is made without them
        ed.select(Some("tiny5"));
        key(&mut ed, &egui, egui::Key::Backspace, egui::Modifiers::NONE);
        assert_eq!(edits(&ed), ["delete medium3", "delete tiny5"]);
        settle(&mut ed);
        assert!(ed.doc.error().is_none(), "{:?}", ed.doc.error());
        assert!(ed.doc.made().unwrap().level.spaces().iter().all(|s| s.key != "medium3" && s.key != "tiny5"));
        // the button, on a hall: its pods go with it, and so does a join that names it
        ed.doc.join("tower", 551, "vast", &[]);
        ed.select(Some("vast"));
        frame(&mut ed, &egui, Vec::new());
        let at = spot(&ed, "delete");
        click(&mut ed, &egui, at);
        assert_eq!(edits(&ed), ["delete medium3", "delete tiny5", "delete vast"]);
        assert_eq!(ed.status, "delete vast; with it went: join tower on 551 to vast");
        assert!(ed.doc.lots().unwrap().lots.iter().all(|a| !["vast", "small8", "tiny2"].contains(&a.key.as_str())));
        // a room made by hand goes as if it had never been added
        ed.change(&Change::Add { key: "room1".into(), kind: "medium".into(), rect: (66, 26, 71, 31), floor: 0, top: 236 });
        assert_eq!(ed.doc.selected(), Some("room1"));
        key(&mut ed, &egui, egui::Key::Delete, egui::Modifiers::NONE);
        assert_eq!(edits(&ed).len(), 3);
        assert_eq!(ed.status, "taken out of the recipe: add room1 (medium) at 66, 26 to 71, 31, from 0 to 236");
    }

    #[test]
    fn a_floor_button_pins_the_room_and_one_it_cannot_go_to_is_greyed() {
        let (mut ed, egui) = rooms();
        ed.select(Some("medium3"));
        frame(&mut ed, &egui, Vec::new());
        let at = spot(&ed, "floor 276");
        click(&mut ed, &egui, at);
        assert_eq!((edits(&ed), ed.status.as_str()), (vec!["pin medium3's floor at 276".to_string()], "pin medium3's floor at 276"));
        let a = lot(&mut ed, "medium3");
        assert_eq!((a.floor, a.top), (276, 512));
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "from 276 to 512"));
        // the floor it is on does nothing
        let at = spot(&ed, "floor 276");
        click(&mut ed, &egui, at);
        assert_eq!(edits(&ed).len(), 1);
        // with one storey shown, the room is kept in sight
        ed.storey = Some(276);
        frame(&mut ed, &egui, Vec::new());
        let at = spot(&ed, "floor 551");
        click(&mut ed, &egui, at);
        assert_eq!((edits(&ed), ed.storey), (vec!["pin medium3's floor at 551".to_string()], Some(551)));
        // the colossal hall can't go down a storey: the button is greyed, with why, and a click on it does nothing
        ed.select(Some("colossal"));
        frame(&mut ed, &egui, Vec::new());
        let why = ed.pins("colossal");
        assert!(why[0].is_some() && why[1].is_none() && why[2].is_none(), "{why:?}");
        let at = spot(&ed, "floor -276");
        click(&mut ed, &egui, at);
        assert!(edits(&ed).len() == 1 && ed.refused.is_none());
        // a hall's pods go with it: a storey up, all of them
        let at = spot(&ed, "floor 276");
        click(&mut ed, &egui, at);
        assert_eq!(edits(&ed), ["pin medium3's floor at 551", "pin colossal's floor at 276"]);
        assert_eq!((lot(&mut ed, "small2").floor, lot(&mut ed, "tiny1").floor), (551, 827));
    }

    #[test]
    fn the_number_fields_move_and_size_the_room() {
        let (mut ed, egui) = rooms();
        ed.select(Some("medium3"));
        frame(&mut ed, &egui, Vec::new());
        type_in(&mut ed, &egui, "x", "63");
        assert_eq!(edits(&ed), ["move medium3 to 63, 3"]);
        type_in(&mut ed, &egui, "y", "4");
        assert_eq!(edits(&ed), ["move medium3 to 63, 4"]);
        let said = type_in(&mut ed, &egui, "wide", "9");
        assert_eq!(edits(&ed), ["move medium3 to 63, 4", "resize medium3 to 9 x 8"]);
        assert!(said.iter().any(|t| t == "9") && lot(&mut ed, "medium3").rect() == (63, 4, 72, 12), "{said:?}");
        type_in(&mut ed, &egui, "deep", "6");
        assert_eq!(lot(&mut ed, "medium3").rect(), (63, 4, 72, 10));
        // what is no number is said, and nothing changes; nor does the same number again
        type_in(&mut ed, &egui, "deep", "tall");
        assert_eq!((ed.status.as_str(), edits(&ed).len()), ("deep: a whole number of cells, not tall", 2));
        let steps = ed.doc.rev();
        type_in(&mut ed, &egui, "wide", "9");
        assert_eq!(ed.doc.rev(), steps);
        // a number that puts it on another room is refused in the generator's words, and the field shows where it still is
        let said = type_in(&mut ed, &egui, "x", "55");
        let why = ed.refused.clone().expect("it lies on spire1");
        assert!(why.contains("spire1") && ed.status == why && edits(&ed).len() == 2 && said.iter().any(|t| t == "63"), "{why}: {said:?}");
        // under two cells
        type_in(&mut ed, &egui, "wide", "1");
        assert_eq!(ed.refused.as_deref(), Some("resize: a room is at least 2 x 2 cells, not 1 x 6"));
        // a hall typed along takes its pods, as a drag of its box does
        ed.select(Some("vast"));
        frame(&mut ed, &egui, Vec::new());
        let pod = lot(&mut ed, "small8");
        type_in(&mut ed, &egui, "x", "38");
        assert_eq!((lot(&mut ed, "vast").x0, lot(&mut ed, "small8").x0, ed.refused.as_deref()), (38, pod.x0 + 1, None));
        // while a field has the keys, Backspace and Ctrl+Z are its own: no room is deleted, nothing undone
        let (at, n) = (spot(&ed, "y"), edits(&ed).len());
        click(&mut ed, &egui, at);
        key(&mut ed, &egui, egui::Key::Backspace, egui::Modifiers::NONE);
        key(&mut ed, &egui, egui::Key::Z, CTRL);
        assert!(edits(&ed).len() == n && ed.doc.selected() == Some("vast") && lot(&mut ed, "vast").x0 == 38);
    }

    #[test]
    fn a_drag_that_starts_on_nothing_pans_the_plan() {
        let (mut ed, egui) = rooms();
        ed.select(Some("medium3"));
        let before = ed.fit.unwrap();
        // from bare ground, 40 pixels east and 25 south: the plan goes with the pointer
        let a = px(&ed, (70.0, 52.0));
        let b = a + egui::vec2(40.0, 25.0);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(a)]);
        frame(&mut ed, &egui, vec![button(a, true)]);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(a + egui::vec2(20.0, 10.0))]);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(b)]);
        frame(&mut ed, &egui, vec![button(b, false)]);
        let after = ed.fit.unwrap();
        assert!((after.x - before.x - 40.0).abs() < 0.01 && (after.y - before.y - 25.0).abs() < 0.01 && after.scale == before.scale, "{before:?} {after:?}");
        // nothing was changed by it, and what was selected still is
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo() && ed.doc.selected() == Some("medium3"));
        // the wheel zooms about the pointer
        frame(
            &mut ed,
            &egui,
            vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 120.0), modifiers: Default::default() }],
        );
        for _ in 0..30 {
            frame(&mut ed, &egui, Vec::new());
        }
        let near = ed.fit.unwrap();
        assert!(near.scale > after.scale * 1.1, "{after:?} {near:?}");
        let (x, y) = near.to(after.from((b.x, b.y)));
        assert!((x - b.x).abs() < 0.5 && (y - b.y).abs() < 0.5, "{x}, {y} is not {b:?}");
        // a double click fits the plan again
        for pressed in [true, false, true, false] {
            frame(&mut ed, &egui, vec![button(b, pressed)]);
        }
        frame(&mut ed, &egui, Vec::new());
        assert_eq!(ed.fit.unwrap(), before);
    }

    #[test]
    fn in_join_a_click_still_picks_a_room_and_a_drag_on_a_room_pans() {
        let (mut ed, egui) = rooms();
        ed.select(Some("medium3"));
        // the tool switch, at the top of the panel
        let at = spot(&ed, "tool join");
        click(&mut ed, &egui, at);
        assert_eq!(ed.tool, Tool::Join);
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "click a room on the plan to see what it could be joined to") && !has(&said, "All rooms") && !has(&said, "New room"), "{said:?}");
        // the plan draws no boxes: it is the picture it was
        let made = ed.doc.made().unwrap().clone();
        assert_eq!(ed.drawn.0, planview::picture(&made.level, &View { scale: ed.fit.unwrap().scale, ..View::default() }).prims.len());
        // a click picks the room to join from, and the line under the plan says so
        let tower = middle(&mut ed, "tower");
        let there = px(&ed, tower);
        assert!(has(&frame(&mut ed, &egui, vec![egui::Event::PointerMoved(there)]), "click: join tower to another room"));
        click(&mut ed, &egui, there);
        assert_eq!(ed.doc.picked(), Some(("tower", 0)));
        // a drag that starts on a room pans, and moves no room
        let before = ed.fit.unwrap();
        drag(&mut ed, &egui, tower, (tower.0 + 3.0, tower.1));
        let there = px(&ed, tower);
        frame(&mut ed, &egui, vec![button(there, false)]);
        let after = ed.fit.unwrap();
        assert!((after.x - before.x - 3.0 * before.scale).abs() < 0.01 && ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo(), "{before:?} {after:?}");
        // Delete is the room tool's: nothing goes
        key(&mut ed, &egui, egui::Key::Delete, egui::Modifiers::NONE);
        assert!(ed.doc.recipe.edits.is_empty() && ed.doc.selected() == Some("medium3"));
        // back to the room tool: the pick is let go (its corridors would be looked for after every change)
        let at = spot(&ed, "tool rooms");
        click(&mut ed, &egui, at);
        assert!(ed.tool == Tool::Rooms && ed.doc.picked().is_none() && ed.doc.selected() == Some("medium3"));
        settle(&mut ed);
    }

    #[test]
    fn with_no_level_the_boxes_are_drawn_alone_and_can_be_put_right() {
        // a recipe that puts a room on a spire makes no level at all
        let mut args = cli::parse(&["--seed".to_string(), "40".to_string()], &|path| Err(format!("{path}: no files here"))).unwrap();
        args.recipe.edits.push(Edit::Move { room: "medium3".into(), to: (55, 3) });
        let mut ed = Editor::new(args);
        settle(&mut ed);
        let why = ed.doc.error().expect("no level").to_string();
        assert!(ed.doc.made().is_none() && why.contains("spire1") && why.contains("rooms keep a cell between them"), "{why}");
        let egui = egui::Context::default();
        let said = pass(&mut ed, &egui, Vec::new());
        assert!(has(&said, &format!("no level: {why}")) && has(&said, "the Rooms tool shows the room boxes") && ed.fit.is_none(), "{said:?}");
        // with the room tool the plan is the boxes alone, and the panel lists them
        ed.set_tool(Tool::Rooms);
        let said = pass(&mut ed, &egui, Vec::new());
        assert!(!has(&said, "the Rooms tool shows the room boxes") && has(&said, "All rooms (29)") && has(&said, "storey") && ed.fit.is_some(), "{said:?}");
        let view = View { scale: ed.fit.unwrap().scale, boxes: Some(ed.doc.boxes(None)), ..View::default() };
        assert_eq!(ed.drawn.0, planview::bare(&view).prims.len());
        // the room is dragged back off the spire: the recipe is the seed's own again, and makes its level
        let at = middle(&mut ed, "medium3");
        let said = drag(&mut ed, &egui, at, (at.0 + 3.0, at.1));
        assert!(ed.refused.as_ref().is_some_and(|w| w.contains("spire1")) && has(&said, ": refused"), "{:?}", ed.refused);
        let free = px(&ed, (at.0 + 6.0, at.1));
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(free)]);
        release(&mut ed, &egui, (at.0 + 6.0, at.1));
        assert!(ed.doc.recipe.edits.is_empty() && ed.refused.is_none(), "{:?}", ed.status);
        settle(&mut ed);
        assert!(ed.doc.error().is_none() && ed.doc.made().is_some_and(|made| made.level.space("medium3").rect() == (61, 3, 68, 11)));
    }
    /// Click one of the panel's controls, by its name, and draw the panel once more.
    fn press(ed: &mut Editor, egui: &egui::Context, name: &str) -> Vec<String> {
        let at = spot(ed, name);
        click(ed, egui, at);
        frame(ed, egui, Vec::new())
    }

    /// An editor on seed 40 with the room tool in hand and this room selected, drawn once.
    fn inside(room: &str) -> (Editor, egui::Context) {
        let (mut ed, egui) = rooms();
        ed.select(Some(room));
        frame(&mut ed, &egui, Vec::new());
        (ed, egui)
    }

    #[test]
    fn a_layout_chosen_is_one_edit_and_the_level_is_made_with_it() {
        let (mut ed, egui) = inside("colossal");
        // the hall as the seed makes it: no upper storey of its own, one pillar in a lane
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "This room's inside") && has(&said, "has: none") && has(&said, "has: lanes, 1 piece"), "{said:?}");
        assert!(has(&said, "brightness and ceiling are for the built level"));
        // a ring: one edit, said in the status line, and the level made again has it
        press(&mut ed, &egui, "layout ring");
        assert_eq!((edits(&ed), ed.status.as_str()), (vec!["colossal: layout ring".to_string()], "colossal: layout ring"));
        settle(&mut ed);
        assert!(ed.doc.error().is_none(), "{:?}", ed.doc.error());
        assert_eq!(ed.doc.made().unwrap().level.interior("colossal").layout, Some("ring"));
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "has: ring"));
        // galleries instead: still one edit of the kind, in the same place
        press(&mut ed, &egui, "layout galleries");
        assert_eq!(edits(&ed), ["colossal: layout galleries"]);
        settle(&mut ed);
        assert_eq!(ed.doc.made().unwrap().level.interior("colossal").layout, Some("galleries"));
        // the one it has chosen, again: nothing changes, and it is no step
        let steps = ed.doc.rev();
        press(&mut ed, &egui, "layout galleries");
        assert_eq!(ed.doc.rev(), steps);
        // the seed's own: the edit is gone
        press(&mut ed, &egui, "own layout");
        assert!(ed.doc.recipe.edits.is_empty() && ed.status == "colossal: the seed's own layout again", "{}", ed.status);
        // three steps to undo: the ring, the galleries, the seed's own
        assert!(ed.doc.undo() && edits(&ed) == ["colossal: layout galleries"] && ed.doc.undo() && ed.doc.undo() && !ed.doc.undo());
    }

    #[test]
    fn what_a_room_cannot_take_is_greyed_and_does_nothing() {
        // a tower's decks are its own: every layout is greyed
        let (mut ed, egui) = inside("tower");
        for name in ["layout none", "layout ring", "layout galleries", "layout mezzanine"] {
            press(&mut ed, &egui, name);
        }
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo() && ed.refused.is_none());
        // (asked for all the same, it is refused in red, and nothing is changed)
        ed.set_choice(Edit::Layout { room: "tower".into(), layout: "ring".into() });
        let why = "layout: tower is a tower: its decks are its own";
        assert_eq!((ed.status.as_str(), ed.refused.as_deref(), ed.doc.can_undo()), (why, Some(why), false));
        assert!(frame(&mut ed, &egui, Vec::new()).iter().filter(|t| *t == why).count() == 2);
        // a pod is furnished with its hall: its cover and its inside are greyed, and it says why once
        ed.select(Some("small8"));
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(said.iter().filter(|t| *t == "small8 is a pod of vast: it is not furnished by itself").count() == 1 && !has(&said, "pieces"), "{said:?}");
        for name in ["own cover", "cover none", "cover crates", "own inside", "inside another"] {
            press(&mut ed, &egui, name);
        }
        assert!(ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo());
        // its brightness and its ceiling are its own all the same
        press(&mut ed, &egui, "ceiling grille");
        assert_eq!(edits(&ed), ["small8: ceiling grille"]);
    }

    #[test]
    fn a_cover_and_another_inside_are_one_edit_each_however_often_changed() {
        let (mut ed, egui) = inside("vast");
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "has: lanes, 2 pieces"));
        press(&mut ed, &egui, "cover crates");
        assert_eq!(edits(&ed), ["vast: cover crates"]);
        press(&mut ed, &egui, "cover none");
        assert_eq!((edits(&ed), ed.status.as_str()), (vec!["vast: cover none".to_string()], "vast: cover none"));
        // the level made again has a bare floor
        settle(&mut ed);
        assert!(ed.doc.error().is_none() && ed.doc.made().unwrap().level.interior("vast").cover.is_empty());
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "has: none, 0 pieces"));
        // another inside, and another: the one edit counts up, and "back" counts down to the seed's own
        let said = press(&mut ed, &egui, "inside another");
        assert!(edits(&ed) == ["vast: cover none", "vast: inside 1"] && has(&said, "inside 1") && has(&said, "back"), "{said:?}");
        let said = press(&mut ed, &egui, "inside another");
        assert!(edits(&ed) == ["vast: cover none", "vast: inside 2"] && said.iter().any(|t| t == "inside 2"), "{said:?}");
        press(&mut ed, &egui, "inside back");
        assert_eq!(edits(&ed), ["vast: cover none", "vast: inside 1"]);
        let said = press(&mut ed, &egui, "inside back");
        assert!(edits(&ed) == ["vast: cover none"] && ed.status == "vast: the seed's own inside again" && !has(&said, "back"), "{said:?}");
        // "the seed's own" from two on, and for the cover
        press(&mut ed, &egui, "inside another");
        press(&mut ed, &egui, "inside another");
        press(&mut ed, &egui, "own inside");
        press(&mut ed, &egui, "own cover");
        assert!(ed.doc.recipe.edits.is_empty() && ed.status == "vast: the seed's own cover again");
        // each was one step
        let mut steps = 0;
        while ed.doc.undo() {
            steps += 1;
        }
        assert_eq!(steps, 10);
    }

    #[test]
    fn the_brightness_slider_dragged_across_is_one_edit_with_the_value_it_was_let_go_at() {
        let (mut ed, egui) = inside("vast");
        // the level sets vast no brightness: "the usual", and no number yet
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "the usual") && !said.iter().any(|t| t == "128"), "{said:?}");
        let bar = ed.spots.iter().find(|(name, _)| name == "brightness").unwrap().1;
        let at = |f: f32| egui::pos2(bar.left() + bar.width() * f, bar.center().y);
        // pressed a quarter of the way along and dragged to three quarters: the value is shown as it goes, and nothing is changed
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.25))]);
        frame(&mut ed, &egui, vec![button(at(0.25), true)]);
        let low = ed.bright.clone().expect("the slider is held").1;
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.5))]);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.75))]);
        let said = frame(&mut ed, &egui, Vec::new());
        let (room, v) = ed.bright.clone().expect("the slider is held");
        assert!(room == "vast" && (40..90).contains(&low) && (170..215).contains(&v), "{low} {v}");
        assert!(said.iter().any(|t| *t == v.to_string()) && ed.doc.recipe.edits.is_empty() && !ed.doc.can_undo(), "{said:?}");
        // let go: one edit with the last value, one step
        frame(&mut ed, &egui, vec![button(at(0.75), false)]);
        let said = frame(&mut ed, &egui, Vec::new());
        assert_eq!((edits(&ed), ed.bright.clone()), (vec![format!("vast: brightness {v}")], None));
        assert!(ed.status == format!("vast: brightness {v}") && said.iter().any(|t| *t == v.to_string()), "{said:?}");
        assert!(ed.doc.undo() && !ed.doc.can_undo() && ed.doc.redo());
        // dragged again, from where it is: still the one edit. (The first put a row in the
        // list of edits above, so the slider is a row lower than it was.)
        frame(&mut ed, &egui, Vec::new());
        let lower = ed.spots.iter().find(|(name, _)| name == "brightness").unwrap().1;
        assert!(lower.top() > bar.top());
        let at = |f: f32| egui::pos2(lower.left() + lower.width() * f, lower.center().y);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.75))]);
        frame(&mut ed, &egui, vec![button(at(0.75), true)]);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.1))]);
        frame(&mut ed, &egui, vec![button(at(0.1), false)]);
        let dim = ed.doc.choices("vast").unwrap().chosen.brightness.expect("a brightness is chosen");
        assert!(dim < 40 && edits(&ed) == [format!("vast: brightness {dim}")], "{:?}", edits(&ed));
        // "the usual" takes it out
        press(&mut ed, &egui, "own brightness");
        assert!(ed.doc.recipe.edits.is_empty() && ed.status == "vast: the seed's own brightness again");
        // a room the level gives a brightness starts the slider there, and shows the number
        ed.select(Some("tower"));
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(said.iter().any(|t| t == "198") && ed.doc.choices("tower").unwrap().brightness == Some(198), "{said:?}");
    }

    #[test]
    fn the_options_are_lists_switches_and_sliders_and_a_slider_is_one_edit_when_let_go() {
        let (mut ed, egui) = (editor("--seed 40"), egui::Context::default());
        pass(&mut ed, &egui, Vec::new());
        press(&mut ed, &egui, "options");
        for _ in 0..10 {
            frame(&mut ed, &egui, Vec::new()); // (the header opens over a few frames)
        }
        // vents: pressed a quarter of the way along, dragged to three quarters; nothing changes while it is held
        let bar = ed.spots.iter().find(|(name, _)| name == "option vents").expect("the vents slider").1;
        let at = |f: f32| egui::pos2(bar.left() + bar.width() * f, bar.center().y);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.25))]);
        frame(&mut ed, &egui, vec![button(at(0.25), true)]);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.5))]);
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at(0.75))]);
        let said = frame(&mut ed, &egui, Vec::new());
        let held: usize = ed.typing.clone().filter(|(n, _)| *n == "vents").expect("the slider is held").1.parse().unwrap();
        assert!((8..=10).contains(&held) && said.iter().any(|t| *t == held.to_string()), "{held} {said:?}");
        assert!(ed.doc.recipe.options.vents == 4 && !ed.doc.can_undo());
        // let go: the option set, one step to undo
        frame(&mut ed, &egui, vec![button(at(0.75), false)]);
        frame(&mut ed, &egui, Vec::new());
        assert_eq!((ed.doc.recipe.options.vents, ed.typing.clone()), (held, None));
        assert!(ed.doc.undo() && !ed.doc.can_undo() && ed.doc.redo());
        settle(&mut ed);
        // "reset" puts the default back, and the recipe is the seed's own again (as `editor` made it)
        press(&mut ed, &egui, "reset vents");
        assert_eq!(ed.doc.recipe.options, Options::python());
        settle(&mut ed);
        // a switch
        press(&mut ed, &egui, "option shapes");
        assert!(!ed.doc.recipe.options.shapes);
        settle(&mut ed);
        // a list: opened, and a word chosen from it
        press(&mut ed, &egui, "option cover");
        press(&mut ed, &egui, "option cover pillars");
        assert_eq!(ed.doc.recipe.options.cover, "pillars");
        settle(&mut ed);
        // Felicity: the options it does not read are greyed, and their sliders do nothing
        press(&mut ed, &egui, "option style");
        let said = press(&mut ed, &egui, "option style felicity");
        assert!(ed.doc.recipe.options.style == "felicity" && has(&said, "reads only the options not greyed out"), "{said:?}");
        settle(&mut ed);
        frame(&mut ed, &egui, Vec::new());
        let bar = ed.spots.iter().find(|(name, _)| name == "option vents").unwrap().1;
        click(&mut ed, &egui, egui::pos2(bar.left() + bar.width() * 0.9, bar.center().y));
        frame(&mut ed, &egui, Vec::new());
        assert!(ed.doc.recipe.options.vents == 4 && ed.typing.is_none());
    }

    #[test]
    fn the_site_is_the_scales_own_until_made_ones_own_and_then_two_sliders() {
        let (mut ed, egui) = (editor("--seed 40"), egui::Context::default());
        pass(&mut ed, &egui, Vec::new());
        press(&mut ed, &egui, "options");
        for _ in 0..10 {
            frame(&mut ed, &egui, Vec::new());
        }
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "the scale's own: 80 x 60") && !ed.spots.iter().any(|(n, _)| n == "option site wide"), "{said:?}");
        // made one's own at the size it was: the same level, and two sliders
        press(&mut ed, &egui, "option site");
        assert_eq!(ed.doc.recipe.options.siting.site, Some((80, 60)));
        settle(&mut ed);
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "80 wide") && has(&said, "60 deep"), "{said:?}");
        let bar = ed.spots.iter().find(|(name, _)| name == "option site wide").expect("the width slider").1;
        click(&mut ed, &egui, egui::pos2(bar.right() - 1.0, bar.center().y));
        frame(&mut ed, &egui, Vec::new());
        let (w, h) = ed.doc.recipe.options.siting.site.expect("a site of its own");
        assert!(w > 190 && h == 60, "{w} x {h}");
        settle(&mut ed);
        // and the scale's own again
        press(&mut ed, &egui, "option site");
        assert_eq!(ed.doc.recipe.options.siting.site, None);
    }

    #[test]
    fn a_ceiling_chosen_is_one_edit_and_the_level_has_it() {
        let (mut ed, egui) = inside("colossal");
        assert!(!has(&frame(&mut ed, &egui, Vec::new()), "has: grille"));
        press(&mut ed, &egui, "ceiling plain");
        press(&mut ed, &egui, "ceiling grille");
        assert_eq!((edits(&ed), ed.status.as_str()), (vec!["colossal: ceiling grille".to_string()], "colossal: ceiling grille"));
        settle(&mut ed);
        assert_eq!(ed.doc.choices("colossal").unwrap().ceiling, Some("grille"));
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "has: grille"));
        press(&mut ed, &egui, "own ceiling");
        assert!(ed.doc.recipe.edits.is_empty() && ed.status == "colossal: the seed's own ceiling again");
        // a room the planner gave a ceiling says which
        ed.select(Some("spire1"));
        assert!(has(&frame(&mut ed, &egui, Vec::new()), "has: grille"));
    }

    #[test]
    fn a_choice_that_makes_no_level_can_be_undone_where_the_reason_is() {
        // seed 40's basin can take a ring by its size, but no attempt furnishes it with one (with
        // no rescue: by default ladders and bridges join it)
        let (mut ed, egui) = (editor("--seed 40 --no-rescue"), egui::Context::default());
        ed.set_tool(Tool::Rooms);
        pass(&mut ed, &egui, Vec::new());
        ed.select(Some("basin"));
        frame(&mut ed, &egui, Vec::new());
        press(&mut ed, &egui, "layout ring");
        assert_eq!(edits(&ed), ["basin: layout ring"]);
        settle(&mut ed);
        let why = "seed 40: basin cannot be furnished with a ring (basin: no set of ramps joins its storeys)";
        assert_eq!(ed.doc.error(), Some(why));
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, &format!("no level: {why}")) && has(&said, "undo that"), "{said:?}");
        // one press takes the choice back, and the level is the seed's own again
        press(&mut ed, &egui, "undo that");
        assert!(ed.doc.recipe.edits.is_empty() && ed.doc.can_redo() && ed.status == "undone: the choice that made no level");
        settle(&mut ed);
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(ed.doc.error().is_none() && !has(&said, "undo that") && !has(&said, "no level: "), "{said:?}");
        // a recipe that makes no level for another reason is not offered it
        ed.doc.add(Edit::Move { room: "medium3".into(), to: (55, 3) });
        settle(&mut ed);
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(ed.doc.error().is_some() && has(&said, "no level: ") && !has(&said, "undo that"), "{said:?}");
    }

    #[test]
    fn a_greyed_control_says_why_under_the_pointer() {
        // (the pointer rests on it for a second: the words come up beside it)
        let (mut ed, egui) = inside("tower");
        let at = spot(&ed, "layout ring");
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at)]);
        let mut said = Vec::new();
        for _ in 0..90 {
            said = frame(&mut ed, &egui, Vec::new());
        }
        assert!(has(&said, "tower is a tower: its decks are its own"), "{said:?}");
        // a floor the room can't go to, likewise
        ed.select(Some("colossal"));
        frame(&mut ed, &egui, Vec::new());
        let why = ed.pins("colossal")[0].clone().expect("colossal can't go down a storey");
        let at = spot(&ed, "floor -276");
        frame(&mut ed, &egui, vec![egui::Event::PointerMoved(at)]);
        for _ in 0..90 {
            said = frame(&mut ed, &egui, Vec::new());
        }
        assert!(has(&said, &why), "{why}: {said:?}");
    }

    /// Where a place in the side view (cells across, cells up) is in the window.
    fn side_px(ed: &Editor, at: P) -> egui::Pos2 {
        let (x, y) = ed.side_fit.expect("the side view was drawn").to(at);
        egui::pos2(x, y)
    }

    /// A drag in the side view from one place to another (cells across, cells up), let go there.
    fn side_drag(ed: &mut Editor, egui: &egui::Context, from: P, to: P) {
        let (a, b) = (side_px(ed, from), side_px(ed, to));
        frame(ed, egui, vec![egui::Event::PointerMoved(a)]);
        frame(ed, egui, vec![button(a, true)]);
        frame(ed, egui, vec![egui::Event::PointerMoved(a + (b - a) / 2.0)]);
        frame(ed, egui, vec![egui::Event::PointerMoved(b)]);
        frame(ed, egui, vec![button(b, false)]);
        frame(ed, egui, Vec::new());
    }

    #[test]
    fn in_the_side_view_a_rooms_top_is_dragged_up_and_its_body_up_a_floor() {
        let (mut ed, egui) = rooms();
        ed.select(Some("medium5"));
        pass(&mut ed, &egui, Vec::new());
        let at = spot(&ed, "view side");
        click(&mut ed, &egui, at);
        pass(&mut ed, &egui, Vec::new());
        let s = ed.side.expect("the side view is shown");
        let a = lot(&mut ed, "medium5");
        assert!(s.crosses(a.rect()), "it cuts through the selected room");
        let (u0, u1) = s.span(a.rect());
        let mid = (u0 + u1) as f32 / 2.0;
        let up = sideview::up;
        // its top a storey up: one height edit, the floor kept
        let top = snap_top(a.top);
        let higher = autolevel::storeys::CEILINGS.iter().copied().find(|&c| c > top).expect("a ceiling above");
        side_drag(&mut ed, &egui, (mid, up(top)), (mid, up(higher)));
        assert_eq!(edits(&ed), [format!("medium5 from {} up to {higher}", a.floor)]);
        settle(&mut ed);
        assert!(ed.doc.error().is_none(), "{:?}", ed.doc.error());
        // undone, then the body dragged up a storey: a pin
        ed.doc.undo();
        pass(&mut ed, &egui, Vec::new());
        let above = LEVELS.iter().copied().find(|&z| z > a.floor).expect("a floor above");
        let inside = (mid, up((a.floor + top) / 2));
        side_drag(&mut ed, &egui, inside, (inside.0, inside.1 + up(above - a.floor)));
        assert_eq!(edits(&ed), [format!("pin medium5's floor at {above}")]);
        // back to the plan
        let at = spot(&ed, "view top");
        click(&mut ed, &egui, at);
        assert_eq!(ed.side, None);
    }

    #[test]
    fn a_feature_turned_on_for_one_room_is_one_edit_and_auto_takes_it_out() {
        // (the editor's tests have the level's chance of water at nothing: on is the room's alone)
        let (mut ed, egui) = inside("medium1");
        settle(&mut ed);
        frame(&mut ed, &egui, Vec::new());
        press(&mut ed, &egui, "features");
        press(&mut ed, &egui, "feature flooded on");
        assert_eq!(edits(&ed), ["medium1: flooded on"]);
        settle(&mut ed);
        assert!(ed.doc.made().unwrap().level.space("medium1").flood.is_some(), "medium1 is flooded");
        frame(&mut ed, &egui, Vec::new());
        let said = frame(&mut ed, &egui, Vec::new());
        assert!(has(&said, "has: yes"), "{said:?}");
        press(&mut ed, &egui, "feature flooded auto");
        assert!(edits(&ed).is_empty());
    }
}
