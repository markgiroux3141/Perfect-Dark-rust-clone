//! The generator's level in the middle of the window: its plan, or the level
//! itself in 3D ([`Middle`]), and everything the 3D view is short of the GPU
//! and the devices, so the tests reach it.
//!
//! [`Space`] keeps the preview on show up to the generator's recipe
//! ([`Space::follow`]: a preview is made on a thread whenever the recipe is
//! not the one on show; while it is being made, or where the recipe makes no
//! level, the last one stays and [`Space::status`] says so), holds the fly
//! camera and, while walking, PD's player ([`Space::walk`],
//! [`Space::stop_walking`]; a level made again under the walker puts it back
//! on a floor or stops the walk), and lays out its egui: the switch and the
//! buttons over the middle ([`Space::bar`]), the keys ([`Space::keys`]: G
//! walks, Escape flies again), the words over the 3D view
//! ([`Space::over_view`]) and the camera's mark on the plan with the right
//! click that sends the camera there ([`Space::over_plan`]).
//!
//! In the 3D view a click picks the generator's room under the pointer, and
//! the room selected gets a box gizmo to reshape it by ([`Space::gizmo`],
//! [`crate::boxgizmo`]): a drag is tried as it goes and made when let go, as
//! on the plan. The cutaway ([`Space::cut`]: no ceilings, the storeys over one
//! left out) is made into the preview, so changing it makes one again.
//!
//! The window (`main.rs`) does the rest: it gathers the keys and the mouse
//! into a [`Motion`] and [`Controls`], loads a new preview's BG into its
//! renderer when [`Space::follow`] says one arrived, and draws
//! [`Space::view`] with [`Space::overlay`] over it.

use std::path::PathBuf;
use std::sync::Arc;

use autolevel::boxes::{Change, Trial};
use autolevel::recipe as gen;
use autolevel::storeys::{CEILINGS, LEVELS};
use autolevel_edit::raster::Fit;
use autolevel_edit::ui::{Editor, Tool};
use engine::egui;
use pd_render::View;
use pd_sim::world::WorldRes;

use crate::boxgizmo::{self, Grab, Shown};
use crate::camera::{self, FlyCam, Motion};
use crate::draw3d::Mesh3d;
use crate::preview::{self, Controls, Cutaway, Job, Preview, Reseat, Walker, EYE};

/// What the middle of the window shows of the generator's level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Middle {
    /// The generator's own plan.
    Plan,
    /// The level in 3D, as the game will draw it.
    Space,
}

/// The plan's colour for the camera's mark, and its size in pixels.
pub const MARK: [u8; 4] = [255, 96, 200, 255];
pub const MARK_PX: f32 = 16.0;

const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 190, 90);
const RED: egui::Color32 = egui::Color32::from_rgb(255, 110, 100);

/// A drag of the box gizmo: what it took hold of, and the change it has come
/// to with what that would make (`boxes::trial`).
#[derive(Clone, Debug)]
struct Held {
    grab: Grab,
    tried: Option<(Change, Trial)>,
}

/// A change to a room's box in a few words, for the line over the view.
pub fn words(c: &Change) -> String {
    match c {
        Change::Move { room, to } => format!("{room} to cell {}, {}", to.0, to.1),
        Change::Rect { room, rect } => format!("{room} {} x {} cells, from {}, {}", rect.2 - rect.0, rect.3 - rect.1, rect.0, rect.1),
        Change::Pin { room, floor } => format!("{room}'s floor to {floor}"),
        Change::Height { room, floor, top } => format!("{room} from {floor} up to {top}"),
        Change::Add { key, .. } => format!("a new room {key}"),
        Change::Delete { room } => format!("{room} deleted"),
    }
}

/// The 3D view of the level being shaped.
pub struct Space {
    pub middle: Middle,
    assets: PathBuf,
    root: PathBuf,
    /// The preview on show, and the recipe it is of.
    shown: Option<(gen::Recipe, Preview)>,
    /// The one being made, and the recipe it is of.
    job: Option<(gen::Recipe, Job)>,
    /// The recipe that last made no preview, and why.
    failed: Option<(gen::Recipe, String)>,
    /// The fly camera (while walking, where it was left).
    pub cam: FlyCam,
    /// The camera has been put somewhere (else the first preview places it).
    placed: bool,
    pub walker: Option<Walker>,
    /// What last happened that is worth a line (and whether it went wrong).
    pub note: Option<(String, bool)>,
    /// How many previews have been shown.
    pub made: usize,
    /// Where the last frame put the bar's controls, by name ("plan", "3d",
    /// "walk", "stop", "go", "ceilings", "cut down", "cut up"): what a test
    /// aims the pointer at.
    pub spots: Vec<(String, egui::Rect)>,
    /// What the 3D view leaves undrawn to see into the level (made into the
    /// preview: a change makes one again).
    pub cut: Cutaway,
    /// A drag of the gizmo under way.
    held: Option<Held>,
    /// What the gizmo draws this frame (the last [`Self::gizmo`] says).
    pub drawn: Shown,
    /// Where the last frame had the 3D view (what its rays are cast through).
    pub rect: Option<egui::Rect>,
}

impl Space {
    /// The view over `assets` (the game's `assets/`), its previews kept
    /// under `root` ([`preview::Scratch::root`], in the window).
    pub fn new(assets: PathBuf, root: PathBuf) -> Space {
        Space {
            middle: Middle::Plan,
            assets,
            root,
            shown: None,
            job: None,
            failed: None,
            cam: FlyCam::new(glam::Vec3::new(0.0, 500.0, 0.0), 0.0, 0.0),
            placed: false,
            walker: None,
            note: None,
            made: 0,
            spots: Vec::new(),
            cut: Cutaway::default(),
            held: None,
            drawn: Shown::default(),
            rect: None,
        }
    }

    /// The preview on show.
    pub fn preview(&self) -> Option<&Preview> {
        self.shown.as_ref().map(|(_, pv)| pv)
    }

    /// Is a preview being made?
    /// Nothing is being made, and what the view wants is on show.
    pub fn settled(&self) -> bool {
        self.job.is_none() && (!self.wanted() || self.shown.is_some() || self.failed.is_some())
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Is the preview on show `recipe`'s own, with the cutaway asked for?
    pub fn current(&self, recipe: &gen::Recipe) -> bool {
        self.shown.as_ref().is_some_and(|(of, pv)| of == recipe && pv.cut == self.cut)
    }

    /// Has the 3D view anything to keep up with: is it on show, or has it
    /// been (so going back to it finds it up to date)?
    pub fn wanted(&self) -> bool {
        self.middle == Middle::Space || self.shown.is_some() || self.job.is_some()
    }

    /// Once a tick: take a preview that has been made, and start making
    /// `recipe`'s if it is not the one on show (nor one that made no level).
    /// True when a new preview has just come on show: its BG is then to be
    /// loaded into the renderer ([`Preview::asset_dir`], [`preview::CODE`]).
    pub fn follow(&mut self, recipe: &gen::Recipe) -> bool {
        let mut arrived = false;
        if let Some(result) = self.job.as_ref().and_then(|(_, job)| job.poll()) {
            let (of, _) = self.job.take().expect("a job was polled");
            match result {
                Ok(pv) => {
                    self.swap(of, pv);
                    arrived = true;
                }
                Err(why) => self.failed = Some((of, why)),
            }
        }
        let failed = self.failed.as_ref().is_some_and(|(of, _)| of == recipe);
        if self.job.is_none() && !self.current(recipe) && !failed && self.wanted() {
            self.job = Some((recipe.clone(), Job::spawn_cut(recipe.clone(), self.cut, self.assets.clone(), self.root.clone())));
        }
        arrived
    }

    /// [`Self::follow`] until `recipe`'s preview is on show or has failed
    /// (the snapshot path and the tests: the window never waits).
    pub fn settle(&mut self, recipe: &gen::Recipe) -> bool {
        let mut arrived = false;
        loop {
            arrived |= self.follow(recipe);
            if self.job.is_none() {
                return arrived;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// A new preview comes on show: the camera stays where it was (the first
    /// one places it), a walker is put back on a floor or its walk stopped.
    /// The preview it replaces is dropped, and its scratch folder with it.
    fn swap(&mut self, of: gen::Recipe, pv: Preview) {
        if !self.placed {
            self.cam = FlyCam { speed: self.cam.speed, ..pv.start_camera() };
            self.placed = true;
        }
        if let Some(w) = self.walker.as_mut() {
            match w.reseat(&pv) {
                Ok(Reseat::Kept) => {}
                Ok(Reseat::Lifted) => self.note = Some(("the floor under the walker moved: it stands on it".to_string(), false)),
                Ok(Reseat::Moved(d)) => self.note = Some((format!("the floor under the walker is gone: it was put on the nearest, {:.1} m off", d / 100.0), false)),
                Err(why) => {
                    self.cam = FlyCam { speed: self.cam.speed, ..w.camera() };
                    self.walker = None;
                    self.note = Some((format!("{why}: the walk is over, the camera is where the walker's eyes were"), true));
                }
            }
        }
        self.failed = None;
        self.shown = Some((of, pv));
        self.made += 1;
    }

    /// One line on how the 3D view stands to `recipe`, and whether it is a
    /// warning: what is on show, that it is being brought up to the plan, or
    /// that the recipe makes no level and the last one stays.
    pub fn status(&self, recipe: &gen::Recipe) -> (String, bool) {
        if let Some((_, why)) = self.failed.as_ref().filter(|(of, _)| of == recipe) {
            let stays = if self.shown.is_some() { ", so the last one stays" } else { "" };
            return (format!("this recipe makes no level{stays}: {why}"), true);
        }
        match (&self.job, &self.shown) {
            (Some((_, job)), Some(_)) => (format!("bringing the 3D view up to the plan... {:.1} s (the last level stays)", job.started.elapsed().as_secs_f32()), true),
            (Some((_, job)), None) => (format!("making the 3D view... {:.1} s", job.started.elapsed().as_secs_f32()), false),
            (None, Some((of, pv))) if of == recipe && pv.cut == self.cut => (pv.line(), false),
            (None, Some((of, _))) if of == recipe => ("the cutaway is being made...".to_string(), false),
            (None, Some(_)) => ("the plan has changed: the last level stays".to_string(), true),
            (None, None) => ("no 3D view yet".to_string(), false),
        }
    }

    /// Walking?
    pub fn walking(&self) -> bool {
        self.walker.is_some()
    }

    /// Drop a player where the camera is, onto the floor beneath it: the
    /// view is then the player's. Says why not where there is no floor.
    pub fn walk(&mut self, res: Option<&Arc<WorldRes>>) {
        let (Some((_, pv)), Some(res)) = (&self.shown, res) else {
            self.note = Some(("no level to walk yet".to_string(), true));
            return;
        };
        match Walker::drop_at(pv, res.clone(), self.cam.pos, self.cam.yaw, self.cam.pitch) {
            Ok(w) => {
                self.note = Some((format!("walking from {}", w.line()), false));
                self.walker = Some(w);
            }
            Err(why) => self.note = Some((why, true)),
        }
    }

    /// Back to the fly camera, at the player's eyes.
    pub fn stop_walking(&mut self) {
        if let Some(w) = self.walker.take() {
            self.cam = FlyCam { speed: self.cam.speed, ..w.camera() };
            self.note = Some(("flying again, from the walker's eyes".to_string(), false));
        }
    }

    /// `dt` seconds: the walker walks by `controls`, or else the camera
    /// flies by `motion`.
    pub fn advance(&mut self, motion: &Motion, controls: Controls, dt: f32) {
        match (&mut self.walker, &self.shown) {
            (Some(w), Some((_, pv))) => w.advance(pv, controls, dt),
            _ => self.cam.update(motion, dt),
        }
    }

    /// Where the view is from, as a fly camera: the walker's eyes, or the
    /// camera itself.
    pub fn eye(&self) -> FlyCam {
        self.walker.as_ref().map_or(self.cam, |w| FlyCam { speed: self.cam.speed, ..w.camera() })
    }

    /// The view to draw: the walker's, or the fly camera's.
    pub fn view(&self, aspect: f32, znear: f32, zfar: f32) -> View {
        match &self.walker {
            Some(w) => w.view(aspect, znear, zfar),
            None => self.cam.view(aspect, znear, zfar),
        }
    }

    /// Put the fly camera somewhere (a walk ends).
    fn place(&mut self, cam: FlyCam) {
        self.walker = None;
        self.cam = FlyCam { speed: self.cam.speed, ..cam };
        self.placed = true;
    }

    /// The camera over the room `key`, from one of its upper corners.
    pub fn go_to_room(&mut self, key: &str) -> bool {
        let Some(cam) = self.preview().and_then(|pv| pv.room_camera(key)) else {
            self.note = Some((format!("{key} is not in the level on show"), true));
            return false;
        };
        self.place(cam);
        self.note = Some((format!("over {key}"), false));
        true
    }

    /// The camera outside the room `key`, over its south-west corner, looking
    /// down at its middle from far enough off to see the whole box (and its
    /// gizmo).
    pub fn overlook(&mut self, key: &str) -> bool {
        let Some(r) = self.preview().and_then(|pv| pv.rooms.iter().find(|r| r.key == key)).cloned() else {
            self.note = Some((format!("{key} is not in the level on show"), true));
            return false;
        };
        let (lo, hi) = boxgizmo::room_box(&r);
        let mid = (lo + hi) * 0.5;
        let eye = mid + glam::Vec3::new(-0.55, 0.75, 0.6).normalize() * (hi - lo).length() * 1.1;
        let d = mid - eye;
        self.place(FlyCam::new(eye, d.x.atan2(d.z), (d.y / d.length()).asin()));
        self.note = Some((format!("over {key}"), false));
        true
    }

    /// The camera standing at the plan place `at` (cells), turned as it is,
    /// and the 3D view on show. With a preview: on the floor there nearest
    /// under the camera's height, else the highest. Without one yet: on the
    /// floor of the room of `level` (the generator's own, as the plan draws
    /// it) there. Says so where the plan has no floor.
    pub fn go_to_cell(&mut self, at: (f32, f32), level: Option<&autolevel::level::Level>) -> bool {
        let eye = self.eye();
        let cam = match (self.preview(), level) {
            (Some(pv), _) => pv.camera_at_cell(at, eye.pos.y, eye.yaw),
            (None, Some(lv)) => {
                let holds = |s: &&autolevel::space::Space| (s.x0 as f32) <= at.0 && at.0 < s.x1 as f32 && (s.y0 as f32) <= at.1 && at.1 < s.y1 as f32;
                // (a pod before the hall it is in, as a click on the plan takes them)
                let room = lv.spaces().iter().filter(holds).min_by_key(|s| s.host.is_empty());
                room.map(|s| FlyCam::new(preview::world(at, s.floor as f32 + EYE), eye.yaw, 0.0))
            }
            (None, None) => None,
        };
        let Some(cam) = cam else {
            self.note = Some((format!("no floor at cell {:.0}, {:.0}", at.0.floor(), at.1.floor()), true));
            return false;
        };
        self.place(cam);
        self.middle = Middle::Space;
        self.note = Some((format!("the camera is at cell {:.0}, {:.0}, {:.0} over the floor", at.0.floor(), at.1.floor(), EYE), false));
        true
    }

    /// The storeys the cut can stop at: from the lowest floor of the level on
    /// show to the storey under its highest ceiling line (indices into
    /// `LEVELS`). None with no preview.
    pub fn cut_steps(&self) -> Option<(usize, usize)> {
        let pv = self.preview()?;
        let low = pv.rooms.iter().filter_map(|r| LEVELS.iter().position(|&z| z == r.floor)).min()?;
        let top = pv.rooms.iter().map(|r| r.top).max()?;
        let high = CEILINGS.iter().rposition(|&c| c < top).unwrap_or(low).max(low);
        Some((low, high))
    }

    /// The cut a storey lower than it is (from none: under the highest
    /// ceiling line), never under the lowest floor.
    pub fn cut_lower(&self) -> Option<usize> {
        let (low, high) = self.cut_steps()?;
        Some(self.cut.up_to.map_or(high, |i| i.saturating_sub(1).max(low)))
    }

    /// The cut a storey higher; over the highest, none.
    pub fn cut_higher(&self) -> Option<usize> {
        let (_, high) = self.cut_steps()?;
        self.cut.up_to.and_then(|i| (i < high).then_some(i + 1))
    }

    /// The gizmo's part of a frame over the 3D view (`resp` is
    /// [`Self::over_view`]'s): a click picks the room under the pointer (the
    /// generator's panel then has it in hand), a drag of a handle of the
    /// selected room's gizmo is tried at each step (`Doc::try_change`, the
    /// box drawn where it would be, red with the refusal) and made when let
    /// go (`Editor::change`). Leaves what to draw in [`Self::drawn`].
    pub fn gizmo(&mut self, ui: &egui::Ui, resp: &egui::Response, ed: &mut Editor) {
        let rect = resp.rect;
        self.rect = Some(rect);
        if self.shown.is_none() || self.walking() || rect.width() < 1.0 || rect.height() < 1.0 {
            (self.held, self.drawn) = (None, Shown::default());
            return;
        }
        let view = self.cam.view(rect.width() / rect.height(), 15.0, 10000.0);
        let ray = |p: egui::Pos2| camera::ray(&view, p.x - rect.min.x, p.y - rect.min.y, rect.width(), rect.height());
        let eye = view.eye;
        let key = ed.doc.selected().map(str::to_owned);
        let lot = key.as_deref().and_then(|k| ed.doc.lots().ok()?.lots.into_iter().find(|a| a.key == k));
        let handle_at = |p: egui::Pos2| {
            let a = lot.as_ref()?;
            let (lo, hi) = boxgizmo::world_box(a);
            let (o, d) = ray(p);
            boxgizmo::pick(lo, hi, eye, o, d)
        };
        let primary = egui::PointerButton::Primary;
        if resp.drag_started_by(primary) {
            let from = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            self.held = from.and_then(|p| {
                let (h, a) = (handle_at(p)?, lot.as_ref()?);
                let (o, d) = ray(p);
                Grab::start(h, a, eye, o, d).map(|grab| Held { grab, tried: None })
            });
            ed.refused = None;
        }
        if let (Some(held), true) = (self.held.as_mut(), resp.dragged_by(primary) || resp.drag_stopped_by(primary)) {
            if let Some(p) = resp.interact_pointer_pos() {
                let (o, d) = ray(p);
                let change = held.grab.change(o, d);
                if held.tried.as_ref().map(|t| &t.0) != change.as_ref() {
                    held.tried = change.and_then(|c| ed.doc.try_change(&c).ok().map(|t| (c, t)));
                }
                ed.refused = held.tried.as_ref().and_then(|(_, t)| t.refused.clone());
            }
        }
        if resp.drag_stopped_by(primary) {
            if let Some(Held { tried: Some((change, _)), .. }) = self.held.take() {
                let made = ed.change(&change);
                self.note = Some(if made { (format!("{} (the level is being made again)", words(&change)), false) } else { (ed.refused.clone().unwrap_or_default(), true) });
            }
        } else if self.held.is_some() && !resp.dragged_by(primary) {
            self.held = None;
        }
        // With "Join" in hand and a room picked to join from, a click on a wall
        // asks for a doorway there (the plan's click on a wall, `Editor::doorway`).
        let joining = ed.tool == Tool::Join && ed.doc.picked().is_some();
        let wall_at = |p: egui::Pos2, pv: Option<&Preview>| {
            let (o, d) = ray(p);
            pv.filter(|_| joining).and_then(|pv| boxgizmo::doorway_at(pv, o, d))
        };
        // With "Put things" in hand, a click puts what the tool puts where the ray meets the
        // level (`Editor::click_thing`, as the plan's click does).
        let things = ed.tool == Tool::Things;
        let place_at = |p: egui::Pos2, pv: Option<&Preview>| {
            let (o, d) = ray(p);
            pv.filter(|_| things).and_then(|pv| boxgizmo::place_at(pv, o, d))
        };
        if resp.clicked() && things {
            if let Some((cell, z)) = resp.interact_pointer_pos().and_then(|p| place_at(p, self.preview())) {
                ed.click_thing(cell, Some(z));
                self.note = Some((ed.status().to_string(), ed.refused.is_some()));
            }
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos().filter(|p| handle_at(*p).is_none()) {
                let (o, d) = ray(p);
                if let Some((room, door, z)) = wall_at(p, self.preview()) {
                    ed.doorway(&room, door, Some(z));
                    self.note = Some((ed.status().to_string(), false));
                } else {
                    let picked = self.preview().and_then(|pv| boxgizmo::pick_room(pv, o, d));
                    ed.select(picked.as_deref());
                    self.note = Some(match &picked {
                        Some(k) => (format!("{k} selected: drag a face to size it, an arrow to move it"), false),
                        None => ("no room there (a corridor, or nothing)".to_string(), false),
                    });
                    // ("Join" in hand: picked to join from too, on the storey the click met)
                    if let (Some(k), Tool::Join) = (&picked, ed.tool) {
                        let z = self.preview().and_then(|pv| boxgizmo::storey_at(pv, o, d));
                        ed.pick_join(k, z);
                        if ed.doc.picked().is_some_and(|(r, _)| r == k) {
                            let z = ed.doc.picked().map_or(0, |(_, z)| z);
                            self.note = Some((format!("{k} selected and picked to join by a door on {z}: click a wall for a doorway there"), false));
                        }
                    }
                }
            }
        }
        let doors = ed.doc.made().map_or(Vec::new(), |made| {
            let asked = ed.doc.asked();
            let mut out = Vec::new();
            let space = |k: &str| made.level.spaces().iter().find(|s| s.key == k);
            if let Some((s, z, d)) = ed.doc.picked().and_then(|(r, z)| Some((space(r)?, z, asked.door?))) {
                out.push(boxgizmo::doorway_box(s.rect(), d, z));
            }
            if let Some((t, d)) = asked.to.as_deref().and_then(|to| Some((space(to)?, asked.to_door?))) {
                out.extend(t.door_levels().into_iter().map(|z| boxgizmo::doorway_box(t.rect(), d, z)));
            }
            out
        });
        let hover_wall = resp.hover_pos().filter(|p| self.held.is_none() && handle_at(*p).is_none()).and_then(|p| wall_at(p, self.preview()));
        // what it draws, and the line under the pointer
        let hot = self.held.as_ref().map(|h| h.grab.handle).or_else(|| resp.hover_pos().and_then(handle_at));
        let (tried, refused) = match self.held.as_ref().and_then(|h| h.tried.as_ref()) {
            Some((c, t)) => {
                let room = c.room();
                (t.lots.iter().filter(|a| a.key == room || a.host == room).cloned().collect(), t.refused.is_some())
            }
            None => (Vec::new(), false),
        };
        let marks = ed.doc.made().map_or(Vec::new(), |made| {
            ed.doc.puts().iter().filter_map(|(e, took)| boxgizmo::thing_box(&made.level, e, took.is_ok()).map(|(a, b)| (a, b, took.is_ok()))).collect()
        });
        let cascade = ed.cascade_shown().map_or(Vec::new(), |c| boxgizmo::cascade_boxes(&c));
        self.drawn = Shown { lot, tried, refused, hot, doors, marks, cascade };
        let line = match (self.held.as_ref().and_then(|h| h.tried.as_ref()), hot, key.as_deref()) {
            (Some((c, t)), _, _) => Some(match &t.refused {
                Some(why) => (format!("{}: refused: {why}", words(c)), true),
                None => (words(c), false),
            }),
            (None, Some(h), Some(k)) if !things => Some((h.does(k), false)),
            _ if things => resp.hover_pos().and_then(|p| place_at(p, self.preview())).and_then(|(cell, z)| {
                let made = ed.doc.made()?;
                Some(match autolevel::puts::click(&made.level, ed.thing, cell, Some(z)) {
                    Ok(e) => (format!("click: {}", e.describe()), false),
                    Err(why) => (why, true),
                })
            }),
            _ => hover_wall.map(|(room, d, z)| {
                let picked = ed.doc.picked().is_some_and(|(r, _)| r == room);
                let does = if picked { format!("{room}'s doorway {} on {z}", d.short()) } else { format!("only join to {room}, through {}", d.words()) };
                (format!("click: {does}"), false)
            }),
        };
        if let Some((line, wrong)) = line {
            let painter = ui.painter_at(rect);
            painter.text(rect.left_bottom() + egui::vec2(8.0, -24.0), egui::Align2::LEFT_BOTTOM, line, egui::FontId::proportional(14.0), if wrong { RED } else { egui::Color32::from_rgb(255, 215, 130) });
        }
    }

    /// What the 3D view draws over the level: the gizmo ([`Self::drawn`]).
    pub fn overlay(&self) -> Mesh3d {
        let mut mesh = Mesh3d::default();
        if self.middle == Middle::Space && !self.walking() {
            boxgizmo::draw(&mut mesh, &self.drawn, self.cam.pos);
        }
        mesh
    }

    // ── egui ───────────────────────────────────────────────────────────────

    /// The keys of the 3D view, when no text field has them: G drops a
    /// walker where the camera is, Escape goes back to the fly camera. Called
    /// before the generator's own panel, which would take Escape for itself.
    pub fn keys(&mut self, egui: &egui::Context, res: Option<&Arc<WorldRes>>) {
        if self.middle != Middle::Space || egui.wants_keyboard_input() {
            return;
        }
        if self.walking() {
            if egui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.stop_walking();
            }
        } else if egui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::G)) {
            self.walk(res);
        }
    }

    /// The strip over the middle of the window: "Plan | 3D", and with the 3D
    /// view on show "Walk" (or "Stop walking"), "Go to <room>" for the room
    /// selected or picked in the generator's panel (`room`), and how the view
    /// stands to `recipe`.
    pub fn bar(&mut self, ui: &mut egui::Ui, recipe: &gen::Recipe, room: Option<&str>, res: Option<&Arc<WorldRes>>) {
        self.spots.clear();
        let mut spots = Vec::new();
        let mut spot = |name: &str, r: &egui::Response| spots.push((name.to_string(), r.rect));
        let (mut walk, mut stop, mut go) = (false, false, false);
        let mut cut = self.cut;
        ui.horizontal(|ui| {
            let r = ui.selectable_label(self.middle == Middle::Plan, "Plan").on_hover_text("The generator's plan. Right-click it: the 3D view's camera goes there");
            spot("plan", &r);
            if r.clicked() {
                self.middle = Middle::Plan;
            }
            let r = ui.selectable_label(self.middle == Middle::Space, "3D").on_hover_text("The level as the game will draw it, kept up to the plan as it is shaped");
            spot("3d", &r);
            if r.clicked() {
                self.middle = Middle::Space;
            }
            ui.separator();
            if self.middle == Middle::Space {
                if self.walking() {
                    let r = ui.button("Stop walking").on_hover_text("Esc: back to the fly camera, at the walker's eyes");
                    spot("stop", &r);
                    stop = r.clicked();
                } else {
                    let r = ui.add_enabled(self.shown.is_some(), egui::Button::new("Walk")).on_hover_text("G: a player is dropped where the camera is, onto the floor under it, and walks the level as in the game");
                    spot("walk", &r);
                    walk = r.clicked();
                }
                if let Some(key) = room {
                    let r = ui.add_enabled(self.shown.is_some(), egui::Button::new(format!("Go to {key}"))).on_hover_text("The camera over the room selected in the panel");
                    spot("go", &r);
                    go = r.clicked();
                }
                ui.separator();
                let mut ceilings = self.cut.ceilings;
                let r = ui.checkbox(&mut ceilings, "No ceilings").on_hover_text("Leave every ceiling undrawn, to see into the rooms from over them (walking is the same)");
                spot("ceilings", &r);
                cut.ceilings = ceilings;
                let r = ui.add_enabled(self.cut_steps().is_some(), egui::Button::new("Cut lower")).on_hover_text("Cut the level off a storey lower: the storeys over it are not drawn");
                spot("cut down", &r);
                if r.clicked() {
                    cut.up_to = self.cut_lower();
                }
                let r = ui.add_enabled(self.cut.up_to.is_some(), egui::Button::new("Cut higher")).on_hover_text("Cut it a storey higher (at the top, nothing is cut)");
                spot("cut up", &r);
                if r.clicked() {
                    cut.up_to = self.cut_higher();
                }
                ui.label(match self.cut.up_to {
                    Some(i) => format!("storeys up to {}", Cutaway::storey_name(i)),
                    None => "every storey".to_string(),
                });
                ui.separator();
            }
            if self.wanted() {
                let (text, warn) = self.status(recipe);
                if warn {
                    ui.colored_label(AMBER, text);
                } else {
                    ui.label(egui::RichText::new(text).weak());
                }
            } else {
                ui.label(egui::RichText::new("3D: the level as the game draws it, to fly through and walk").weak());
            }
        });
        self.spots = spots;
        self.cut = cut;
        if stop {
            self.stop_walking();
        } else if walk {
            self.walk(res);
        }
        if let (true, Some(key)) = (go, room) {
            self.go_to_room(key);
        }
    }

    /// The words over the 3D view (the middle of the window, which the
    /// renderer has drawn under the panels): how to move, where the camera or
    /// the walker is, what last happened; with no preview yet, that it is
    /// being made. Returns the area's response: the window takes the pointer
    /// from it (the right button looks). The wheel sets the fly speed, and a
    /// press on the view takes the keys from whatever in the panel had them.
    pub fn over_view(&mut self, ui: &mut egui::Ui, recipe: &gen::Recipe) -> egui::Response {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        // A press on the view gives it the keys: a field or a slider of the
        // panel that had them lets go (the arrows and Space are the walker's).
        if resp.hovered() && ui.input(|i| i.pointer.any_pressed()) {
            ui.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        let painter = ui.painter_at(rect);
        let Some((_, pv)) = &self.shown else {
            let (text, warn) = self.status(recipe);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(18.0), if warn { AMBER } else { egui::Color32::GRAY });
            return resp;
        };
        if resp.hovered() && !self.walking() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.cam.speed = (self.cam.speed * 1.15f32.powf(scroll / 50.0)).clamp(50.0, 20000.0);
            }
        }
        let hint = if self.walking() {
            "Walking: WASD or the arrows (left and right turn), right button: look, C: crouch a level down, Space: up, Esc: fly again"
        } else {
            "Right button: look, WASD: fly, Q/E down and up, Shift faster, wheel: speed. G: walk from here. Click: pick a room"
        };
        painter.text(rect.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(13.0), egui::Color32::from_gray(230));
        if let Some((note, wrong)) = &self.note {
            painter.text(rect.left_top() + egui::vec2(8.0, 28.0), egui::Align2::LEFT_TOP, note, egui::FontId::proportional(13.0), if *wrong { AMBER } else { egui::Color32::from_rgb(150, 220, 150) });
        }
        let eye = self.eye();
        let (cx, cy) = preview::cell(eye.pos);
        let room = pv.room_at(eye.pos - glam::Vec3::Y * 20.0).map_or(String::new(), |r| format!(" in {}", r.key));
        let line = match &self.walker {
            Some(w) => format!("walking: {} · cell {:.1}, {:.1}{room}", w.line(), cx, cy),
            None => format!("({:.0}, {:.0}, {:.0}) yaw {:.0}° pitch {:.0}° · cell {:.1}, {:.1}{room} · speed {:.0} cm/s", eye.pos.x, eye.pos.y, eye.pos.z, eye.yaw.to_degrees().rem_euclid(360.0), eye.pitch.to_degrees(), cx, cy, eye.speed),
        };
        painter.text(rect.left_bottom() + egui::vec2(8.0, -6.0), egui::Align2::LEFT_BOTTOM, line, egui::FontId::monospace(11.0), egui::Color32::from_gray(220));
        resp
    }

    /// Over the plan (drawn in `rect`, laid out by `fit`: the generator's
    /// editor's `fit` after its `canvas`): the camera's mark, an arrowhead
    /// where the 3D view's camera or walker is, pointing the way it looks;
    /// and a right click on the plan sends the camera there
    /// ([`Self::go_to_cell`]) and shows the 3D view.
    pub fn over_plan(&mut self, ui: &egui::Ui, rect: egui::Rect, fit: Option<Fit>, level: Option<&autolevel::level::Level>) {
        let Some(fit) = fit else { return };
        if self.placed {
            let eye = self.eye();
            let at = fit.to(preview::cell(eye.pos));
            let shape = |size: f32, col: [u8; 4]| egui::Shape::convex_polygon(preview::marker(at, eye.yaw, size).iter().map(|&(x, y)| egui::pos2(x, y)).collect(), egui::Color32::from_rgba_unmultiplied(col[0], col[1], col[2], col[3]), egui::Stroke::NONE);
            let painter = ui.painter_at(rect);
            painter.add(shape(MARK_PX + 4.0, [12, 13, 17, 255]));
            painter.add(shape(MARK_PX, MARK));
        }
        let clicked = ui.input(|i| if i.pointer.secondary_clicked() { i.pointer.interact_pos() } else { None });
        if let Some(at) = clicked.filter(|p| rect.contains(*p)) {
            self.go_to_cell(fit.from((at.x, at.y)), level);
        }
    }
}

#[cfg(test)]
mod tests;
