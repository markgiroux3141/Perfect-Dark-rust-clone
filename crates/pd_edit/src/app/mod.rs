//! The editor's window (`pd_edit [<code>]`, see the crate's docs). Opens
//! the level whose recipe is `crates/pd_import/levels/<code>.json`, or a
//! start panel to pick one or make one from a glTF file.
//!
//! The view: hold the right mouse button to look, WASD to fly, Q/E down and
//! up, Shift faster, the wheel sets the speed. Click an item or a waypoint to
//! select it; drag the gizmo's arrows to move it along x, y or z, or (T) its
//! ring to turn it; Ctrl snaps. With an Add tool, click the floor to place
//! one. G drops the selection to the floor, Shift+D duplicates an item,
//! Delete removes, F frames, Ctrl+Z / Ctrl+Y undo and redo, Ctrl+S saves the
//! layout. Doors (their own tab): pick a model from the catalogue (every door
//! PD's and GoldenEye's setups place), + Door, click a doorway's floor; it is
//! fitted to the opening, and moved and turned like an item.
//!
//! "Generate…" hosts the level generator's own editor (`autolevel_edit`: the
//! plan of a seed's level, its room boxes, joins and each room's choices) in
//! place of the view, with the arena's name and code above it;
//! "Make it an arena" writes the level's recipe and imports it, and the
//! level then opens here as any other. A generated level has "Generator…"
//! beside its tabs, to shape it again.
//!
//! Over the generator's plan is a switch, "Plan | 3D". "3D" shows the level
//! the generator last made as the game will draw it (`pd_edit::preview`: a
//! preview import of the geometry alone, about 0.6 s on a thread after each
//! change, in a scratch folder; while it is made, or where the recipe makes
//! no level, the last one stays and a line says so), with the fly camera as
//! above. "Walk" (G) drops PD's player where the camera is, onto the floor
//! under it: WASD or the arrows walk (left and right turn), the right button
//! looks, C crouches a level down and Space up (two levels down fit a vent),
//! Escape flies again from the player's eyes. On the plan the camera is a
//! pink arrowhead, and a right click there sends the camera to that place.
//! "Go to <room>" puts it over the room selected in the generator's panel.
//!
//! `pd_edit --shot <code> <out.png> [...]`: the view to a PNG (see `shot`).
//! `pd_edit --shot-generated <out.png> [--seed N | --recipe FILE] [--at
//! x,y,z,yaw,pitch | --view NAME | --room KEY] [--walk x,y,z,theta | --walk
//! NAME [--forward TICKS] [--crouch]] [--plan] [--list]`: the generator's 3D
//! view, a walker's, or the plan with the camera marked, to a PNG (see `shot`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use engine::app::{Ctx, Game};
use engine::egui;
use engine::gpu::{Filter, Frame, Presenter, RenderTarget};
use engine::input::KeyCode;
use engine::wgpu;
use glam::Vec3;
use crate::camera::{self, FlyCam, Motion};
use crate::doc::{Doc, Item, Kind, Sel, PAD_HEIGHT};
use crate::draw3d::Draw3d;
use crate::generator::Form;
use crate::gizmo::{self, Handle};
use crate::jobs::{Job, Work};
use crate::newlevel::NewLevel;
use crate::overlay::{self, Highlight, Show};
use crate::paths::EditPaths;
use crate::preview::{self, Controls};
use crate::scene::Scene;
use crate::space::{Middle, Space};
use pd_import::doors::{Catalogue, Game as DoorGame};
use pd_import::layout::{Layout, Motion as DoorMotion, Side, Swing};
use pd_import::recipe::Recipe;
use pd_menu::generated::MP_WEAPON_SETS;
use pd_render::Renderer;
use pd_sim::world::WorldRes;

mod items;
mod screens;
mod shape;
pub mod shot_ui;
mod theme;

pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// An edit is placed this long after the last change.
const APPLY_AFTER: f32 = 0.35;

/// The screen on show: Home, or one of a level's three steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Home,
    /// Its rooms: the generator's plan, or the model it was imported from.
    Layout,
    /// Spawns, weapons, ammo, hills, bases, cover, doors (and the simulants' paths).
    Items,
    /// The checklist, the simulant test, Play now.
    Test,
}

/// What the Items step's left panel is about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Items,
    Waypoints,
    Doors,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Add(Kind),
    AddNode,
    Link,
    /// Place the picked catalogue door.
    AddDoor,
}

/// `door_play_opening_sound`'s types PD has (`door_sounds`), for the sound menu.
/// How a door moves, in the inspector's words.
fn motion_word(m: DoorMotion) -> &'static str {
    match m {
        DoorMotion::Slide => "slides sideways",
        DoorMotion::Flexi1 => "slides (three-part, 1)",
        DoorMotion::Flexi2 => "slides (three-part, 2)",
        DoorMotion::Flexi3 => "slides (three-part, 3)",
        DoorMotion::Up => "rises",
        DoorMotion::Down => "sinks",
        DoorMotion::Swing => "swings on a hinge",
        DoorMotion::Eyelid => "opens like an eyelid",
        DoorMotion::Iris => "opens like an iris",
        DoorMotion::Chair => "turns like a chair",
        DoorMotion::Hull => "swings up like a hatch",
    }
}

/// Home's picture of an arena, in its stage folder, and its size.
const THUMB: &str = "thumb.png";
const THUMB_SIZE: (u32, u32) = (480, 270);

const DOOR_SOUNDS: [u8; 21] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 28, 29];
/// An auto-close time that never comes (PD's setups' 0x0fffffff).
const NEVER: i32 = 0x0fff_ffff;

/// A gizmo drag: the gizmo's, what it moves, and where that started.
struct Dragging {
    drag: gizmo::Drag,
    sel: Sel,
    start: Vec3,
}

/// The open level.
struct Level {
    recipe: Recipe,
    layout_path: PathBuf,
    doc: Doc,
    /// No layout file yet: take the stage's placement once it loads.
    adopt: bool,
    scene: Option<Scene>,
    /// The renderer holds this level's BG.
    bg_loaded: bool,
    /// Loading its BG failed (said once; not tried again until it reloads).
    bg_failed: bool,
    report: Vec<String>,
    /// The last simulant test's numbers.
    test: Option<pd_import::CheckStats>,
    /// Why the last build or placement failed (cleared when another starts).
    failed: Option<String>,
    /// An edit not placed yet: when it was made.
    pending: Option<Instant>,
}

impl Level {
    /// Move an item (no undo step) and, for a pickup, its object in the view
    /// at once: the placement that follows comes a moment after the edit.
    fn set_pos(&mut self, it: Item, p: Vec3) {
        let from = self.doc.pos(it);
        self.doc.set_pos(it, p);
        if let (Kind::Weapon | Kind::Ammo, Some(scene)) = (it.kind, self.scene.as_mut()) {
            scene.move_pickup(from, self.doc.pos(it));
        }
    }

    /// A placement of `placed` just loaded: the pickups the user moved while
    /// it ran shown where the layout has them now (when the lists still
    /// match one for one).
    fn catch_up(&mut self, placed: &Layout) {
        let Some(scene) = self.scene.as_mut() else { return };
        let lists = [(Kind::Weapon, placed.weapons.as_ref().map(|w| w.iter().map(|x| x.pos).collect::<Vec<_>>())), (Kind::Ammo, placed.ammo.as_ref().map(|a| a.iter().map(|x| x.pos).collect()))];
        for (kind, was) in lists {
            let Some(was) = was.filter(|w| w.len() == self.doc.count(kind)) else { continue };
            for (index, from) in was.into_iter().enumerate() {
                let (from, to) = (Vec3::from(from), self.doc.pos(Item { kind, index }));
                if from != to {
                    scene.move_pickup(from, to);
                }
            }
        }
    }
}

/// The level generator's editor, hosted, and the arena its level becomes.
struct Generating {
    ed: autolevel_edit::ui::Editor,
    form: Form,
    /// The middle of the window: the plan, or the level in 3D.
    space: Space,
    /// The generator's renderer holds the BG of this level's preview.
    bg_loaded: bool,
    /// A new preview has come on show: its BG is to be loaded.
    bg_pending: bool,
}

impl Generating {
    fn new(recipe: autolevel::recipe::Recipe, form: Form, paths: &EditPaths) -> Generating {
        // (the generator's editor is made from its command line's reading)
        let mut args = autolevel_edit::cli::parse(&[], &|path| Err(format!("{path}: no file is read here"))).expect("an empty command line");
        args.recipe = recipe;
        let root = preview::Scratch::root();
        preview::Scratch::sweep(&root);
        let mut ed = autolevel_edit::ui::Editor::new(args);
        // (rooms saved to use again: beside the recipes, `levels/presets/`)
        ed.presets_dir = paths.levels.join("presets");
        Generating { ed, form, space: Space::new(paths.assets.clone(), root), bg_loaded: false, bg_pending: false }
    }

    /// The room in hand in the generator's panel: the one whose box is
    /// selected, else the one picked to join from.
    fn room(&self) -> Option<String> {
        self.ed.doc.selected().or(self.ed.doc.picked().map(|(room, _)| room)).map(str::to_owned)
    }
}

pub struct App {
    paths: EditPaths,
    /// Home's pictures of the arenas, by code: the thumbnail's time and its
    /// texture (`screens::level_card`; made by `App::make_thumb`).
    thumbs: std::collections::HashMap<String, (std::time::SystemTime, egui::TextureHandle)>,
    /// The level generator, in place of the view.
    generating: Option<Generating>,
    res: Option<Arc<WorldRes>>,
    renderer: Option<Renderer>,
    /// The generator's 3D view's own renderer (it holds a preview's BG while
    /// `renderer` keeps the open level's), made when the first preview arrives.
    gen_renderer: Option<Renderer>,
    presenter: Option<Presenter>,
    draw3d: Option<Draw3d>,
    target: Option<RenderTarget>,
    level: Option<Level>,
    job: Option<Job>,
    cam: FlyCam,
    looking: bool,
    /// Capture the cursor (or let it go) once the window is in reach.
    capture: Option<bool>,
    /// The window's title, as the panels last had it.
    title: String,
    hover_view: bool,
    /// Nothing in the panels has the keys (a text field, a focused slider):
    /// the generator's 3D view may take the arrows and Space.
    keys_free: bool,
    step: Step,
    tab: Tab,
    tool: Tool,
    /// The editor's look has been set on egui.
    themed: bool,
    /// The help window is open.
    help: bool,
    /// The view's "Moving around" tips are on show.
    tips: bool,
    /// The generator's own panel (every option) in place of the simple one.
    shape_advanced: bool,
    /// The cover slider's value while it is held.
    shape_clutter: Option<f64>,
    home_search: String,
    /// Home's filter: all, generated, imported.
    home_filter: u8,
    /// Home shows every arena, not only the active set's.
    home_all: bool,
    /// A dialog about arena sets open on Home.
    set_dialog: Option<screens::SetDialog>,
    import_advanced: bool,
    /// Play now was pressed while the level was being saved: start the game once it is.
    play_queued: bool,
    team: u8,
    weapon_set: usize,
    show: Show,
    far: bool,
    sel: Option<Sel>,
    hovered: Option<Sel>,
    gizmo_mode: gizmo::Mode,
    hot: Option<Handle>,
    dragging: Option<Dragging>,
    link_from: Option<usize>,
    /// A waypoint to select again once the graph reloads (by position).
    reselect_node: Option<Vec3>,
    /// A waypoint's position before the panel started editing it.
    node_edit_start: Option<Vec3>,
    /// This frame's overlay (drawn in `render`).
    built: overlay::Built,
    new_level: Option<NewLevel>,
    message: Option<(String, bool)>,
    open_code: Option<String>,
    /// Every door model the Doors tab offers.
    catalogue: Catalogue,
    door_filter: String,
    /// Which game's doors the list shows (none: both).
    door_game: Option<DoorGame>,
    /// The catalogue door + Door places: its stem and template.
    door_pick: Option<(String, usize)>,
    /// Fit a placed door to the doorway it is put in.
    door_fit: bool,
    /// The doors' preview: when it started, and the time not yet stepped.
    preview: Option<Instant>,
    preview_acc: f32,
}

fn rgba(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

impl App {
    pub fn new(paths: EditPaths, open_code: Option<String>) -> App {
        // The arena sets ready (the first time: "Main", from the game's list).
        let sets = pd_import::sets::ensure(&pd_import::sets::dir_for(&paths.levels), &paths.custom, &paths.levels).err();
        let mut app = App {
            paths,
            thumbs: Default::default(),
            generating: None,
            res: None,
            renderer: None,
            gen_renderer: None,
            presenter: None,
            draw3d: None,
            target: None,
            level: None,
            job: None,
            cam: FlyCam::new(Vec3::new(0.0, 500.0, 0.0), 0.0, 0.0),
            looking: false,
            capture: None,
            title: "pd_edit".into(),
            hover_view: false,
            keys_free: true,
            step: Step::Items,
            tab: Tab::Items,
            tool: Tool::Select,
            themed: false,
            help: false,
            tips: true,
            shape_advanced: false,
            shape_clutter: None,
            home_search: String::new(),
            home_filter: 0,
            home_all: false,
            set_dialog: None,
            import_advanced: false,
            play_queued: false,
            team: 0,
            weapon_set: 0,
            show: Show::default(),
            far: false,
            sel: None,
            hovered: None,
            gizmo_mode: gizmo::Mode::Move,
            hot: None,
            dragging: None,
            link_from: None,
            reselect_node: None,
            node_edit_start: None,
            built: overlay::Built::default(),
            new_level: None,
            message: None,
            open_code,
            catalogue: Catalogue::default(),
            door_filter: String::new(),
            door_game: None,
            door_pick: None,
            door_fit: true,
            preview: None,
            preview_acc: 0.0,
        };
        if let Some(e) = sets {
            app.say(format!("arena sets: {e}"), true);
        }
        app
    }

    fn say(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            log::warn!("{text}");
        }
        self.message = Some((text, error));
    }

    /// The preview weapon set's six `MP_WEAPONS` slots.
    fn weapons(&self) -> [u8; 6] {
        MP_WEAPON_SETS.get(self.weapon_set).map_or([0; 6], |s| s.slots.map(|x| x as u8))
    }

    fn scene(&self) -> Option<&Scene> {
        self.level.as_ref().and_then(|l| l.scene.as_ref())
    }

    /// Open the level of the recipe at `path`: its layout (or the stage's
    /// placement, adopted), its stage (imported first if it must be).
    fn open(&mut self, path: PathBuf) {
        self.open_with(path, false);
    }

    /// [`Self::open`]; `fresh`: imported again even if it has been (its
    /// recipe has just been written anew).
    fn open_with(&mut self, path: PathBuf, fresh: bool) {
        if self.job.is_some() {
            return self.say("wait for the running job to finish", true);
        }
        let recipe = match Recipe::load(&path) {
            Ok(r) => r,
            Err(e) => return self.say(e, true),
        };
        let layout_path = Layout::path_for(&path);
        let (layout, adopt) = match Layout::load(&layout_path) {
            Ok(Some(l)) => (l, false),
            Ok(None) => (Layout::new(), true),
            Err(e) => return self.say(e, true),
        };
        let code = recipe.code.clone();
        let name = recipe.name.clone();
        self.level = Some(Level { recipe, layout_path, doc: Doc::new(layout, !adopt), adopt, scene: None, bg_loaded: false, bg_failed: false, report: Vec::new(), test: None, failed: None, pending: None });
        self.tool = Tool::Select;
        self.sel = None;
        self.link_from = None;
        self.new_level = None;
        if self.paths.is_imported(&code) && !fresh {
            let Some(res) = self.res.clone() else { return };
            match Scene::load(&self.paths.asset_dir(), &code, res, self.weapons()) {
                Ok(scene) => {
                    self.scene_loaded(scene, true);
                    // Bring the stage up to the layout file (it may have been
                    // edited, or the stage placed without it).
                    if !adopt {
                        self.start(Work::Place { fresh: false });
                    }
                }
                Err(e) => {
                    self.say(format!("{e}; importing it again"), true);
                    self.start(Work::Import);
                }
            }
        } else {
            self.start(Work::Import);
        }
        self.say(format!("Opened {name}"), false);
    }

    /// A stage (re)loaded: swap it in; `new_bg` loads its BG and puts the
    /// camera at its start.
    fn scene_loaded(&mut self, scene: Scene, new_bg: bool) {
        let Some(level) = self.level.as_mut() else { return };
        if level.adopt {
            match Layout::adopt(&self.paths.stage_dir(&level.recipe.code)) {
                Ok((l, _)) => {
                    level.doc = Doc::new(l, false);
                    level.adopt = false;
                }
                Err(e) => log::warn!("adopting the placement: {e}"),
            }
        }
        if new_bg || !level.bg_loaded {
            // (loaded by `load_bg`, which has the GPU)
            level.bg_loaded = false;
            level.bg_failed = false;
            self.cam = scene.start_camera();
        }
        // A layout file without doors (one made before doors could be edited):
        // the stage's own (a GoldenEye level's) are the doors.
        if level.doc.layout.doors.is_none() {
            match Layout::adopt(&self.paths.stage_dir(&level.recipe.code)) {
                Ok((l, left)) => {
                    level.doc.adopt_doors(l.doors.unwrap_or_default());
                    for m in left.iter().filter(|m| m.starts_with("door")) {
                        log::warn!("{m}");
                    }
                }
                Err(e) => log::warn!("adopting the stage's doors: {e}"),
            }
        }
        // The graph was rebuilt: waypoint indices changed.
        let node_at = match self.sel {
            Some(Sel::Node(i)) => self.reselect_node.take().or_else(|| level.scene.as_ref().and_then(|s| s.graph.pos.get(i).copied())),
            _ => self.reselect_node.take(),
        };
        if let Some(p) = node_at {
            self.sel = scene.graph.nearest(p, 40.0).map(Sel::Node);
        }
        self.link_from = None;
        let level = self.level.as_mut().unwrap();
        level.scene = Some(scene);
        self.validate_sel();
    }

    /// Drop a selection that no longer exists (after an undo, a reload).
    fn validate_sel(&mut self) {
        let Some(level) = self.level.as_ref() else {
            self.sel = None;
            return;
        };
        let ok = match self.sel {
            Some(Sel::Item(it)) => it.index < level.doc.count(it.kind),
            Some(Sel::Node(i)) => level.scene.as_ref().is_some_and(|s| i < s.graph.pos.len()),
            None => true,
        };
        if !ok {
            self.sel = None;
        }
    }

    fn start(&mut self, work: Work) {
        let (Some(level), Some(res)) = (self.level.as_mut(), self.res.clone()) else { return };
        if self.job.is_some() {
            return;
        }
        if matches!(work, Work::Place { .. }) && !self.paths.is_imported(&level.recipe.code) {
            return self.start(Work::Import);
        }
        level.pending = None;
        // Saving is automatic: the layout file is written as its edits are
        // placed (the window's Save button is gone; Ctrl+S still saves).
        if matches!(work, Work::Place { .. }) && level.doc.dirty() {
            match level.doc.layout.save(&level.layout_path) {
                Ok(()) => level.doc.mark_saved(),
                Err(e) => log::warn!("saving the layout: {e}"),
            }
        }
        let weapons = MP_WEAPON_SETS.get(self.weapon_set).map_or([0; 6], |s| s.slots.map(|x| x as u8));
        // A level with no layout file yet is imported with a blank one: every
        // kind of item generated, then adopted (`scene_loaded`). The doc's
        // layout has an empty list of each kind (so each can be edited), and
        // to the importer an empty list means none: "no spawns".
        let layout = if level.adopt { Layout::new() } else { level.doc.layout.clone() };
        if work != Work::Check {
            level.failed = None;
        }
        self.job = Some(Job::spawn(work, level.recipe.clone(), layout, self.paths.clone(), res, weapons));
    }

    fn poll_job(&mut self) {
        let Some(done) = self.job.as_ref().and_then(|j| j.poll()) else { return };
        let Some(job) = self.job.take() else { return };
        let secs = job.started.elapsed().as_secs_f32();
        match done.result {
            Ok((report, scene)) => {
                if let Some(level) = self.level.as_mut() {
                    if done.work == Work::Check {
                        level.report.extend(report);
                        level.test = done.stats;
                    } else {
                        level.report = report;
                    }
                }
                if let Some(scene) = scene {
                    self.scene_loaded(scene, done.work == Work::Import);
                    if let Some(level) = self.level.as_mut() {
                        level.catch_up(&job.layout);
                    }
                }
                if done.work == Work::GeDoors {
                    self.catalogue = Catalogue::load(&self.paths.asset_dir());
                }
                if done.work != (Work::Place { fresh: false }) {
                    self.say(format!("{} done in {secs:.1} s", done.work.label()), false);
                }
            }
            Err(e) => {
                if done.work != Work::Check {
                    if let Some(level) = self.level.as_mut() {
                        level.failed = Some(e.clone());
                    }
                }
                self.say(format!("{}: {e}", done.work.label()), true)
            }
        }
    }

    fn save(&mut self) {
        let Some(level) = self.level.as_mut() else { return };
        match level.doc.layout.save(&level.layout_path) {
            Ok(()) => {
                level.doc.mark_saved();
                let msg = format!("saved {}", level.layout_path.display());
                self.say(msg, false);
            }
            Err(e) => self.say(e, true),
        }
    }

    fn edited(&mut self) {
        if let Some(l) = self.level.as_mut() {
            l.pending = Some(Instant::now());
        }
        self.validate_sel();
    }

    fn undo(&mut self, redo: bool) {
        let done = self.level.as_mut().is_some_and(|l| if redo { l.doc.redo() } else { l.doc.undo() });
        if done {
            self.edited();
        }
    }


    /// Look at `p` from a clear side, near the way the camera faces now.
    fn frame_on(&mut self, p: Vec3) {
        if let Some(scene) = self.scene() {
            let speed = self.cam.speed;
            self.cam = scene.frame(p, self.cam.yaw);
            self.cam.speed = speed;
        }
    }

    // ── The selection ──────────────────────────────────────────────────────

    /// Where the selection is (a pad position), if anything is selected.
    fn sel_pos(&self) -> Option<Vec3> {
        let level = self.level.as_ref()?;
        match self.sel? {
            Sel::Item(it) => Some(level.doc.pos(it)),
            Sel::Node(i) => level.scene.as_ref()?.graph.pos.get(i).copied(),
        }
    }

    fn sel_facing(&self) -> Option<f32> {
        match self.sel? {
            Sel::Item(it) => self.level.as_ref()?.doc.facing(it),
            Sel::Node(_) => None,
        }
    }

    /// The gizmo the selection gets: Rotate only for what has a facing.
    fn gizmo_for_sel(&self) -> gizmo::Mode {
        if self.gizmo_mode == gizmo::Mode::Rotate && self.sel_facing().is_some() {
            gizmo::Mode::Rotate
        } else {
            gizmo::Mode::Move
        }
    }

    /// Move the selection to `p` (no undo step for items: a drag's is taken
    /// when it starts; waypoints are shown moved and recorded when it ends).
    fn move_sel_live(&mut self, p: Vec3) {
        let Some(level) = self.level.as_mut() else { return };
        match self.sel {
            Some(Sel::Item(it)) => level.set_pos(it, p),
            Some(Sel::Node(i)) => {
                if let Some(s) = level.scene.as_mut() {
                    s.graph.pos[i] = p;
                }
            }
            None => {}
        }
    }

    fn delete_sel(&mut self) {
        let Some(sel) = self.sel else { return };
        let Some(level) = self.level.as_mut() else { return };
        match sel {
            Sel::Item(it) => level.doc.delete(it),
            Sel::Node(i) => {
                let Some(scene) = level.scene.as_mut() else { return };
                let p = scene.graph.pos[i];
                level.doc.wp_remove(p);
                scene.graph.remove(i);
            }
        }
        self.sel = None;
        self.edited();
    }

    /// Drop the selection onto the floor under it.
    fn ground_sel(&mut self) {
        let (Some(sel), Some(p)) = (self.sel, self.sel_pos()) else { return };
        let Some(level) = self.level.as_mut() else { return };
        let Some(scene) = level.scene.as_ref() else { return };
        let lift = match sel {
            Sel::Item(it) => PAD_HEIGHT - it.kind.height(),
            Sel::Node(_) => 0.0,
        };
        let Some(q) = scene.stand(p + Vec3::Y * (20.0 + lift)).map(|q| q - Vec3::Y * lift) else { return self.say("no floor under it", true) };
        match sel {
            Sel::Item(it) => {
                level.doc.begin_drag();
                level.set_pos(it, q);
            }
            Sel::Node(i) => {
                level.doc.wp_move(p, q);
                if let Some(scene) = level.scene.as_mut() {
                    scene.graph.pos[i] = q;
                }
                self.reselect_node = Some(q);
            }
        }
        self.edited();
    }

    fn duplicate_sel(&mut self) {
        let Some(Sel::Item(it)) = self.sel else { return };
        let Some(level) = self.level.as_mut() else { return };
        if it.kind == Kind::Door {
            // Beside it, along the wall.
            let mut d = level.doc.door(it.index).clone();
            let across = Vec3::Y.cross(d.front());
            d.pos = (Vec3::from(d.pos) + across * (d.size[0] + 20.0)).to_array();
            d.sibling = None;
            let new = level.doc.add_door(d);
            self.sel = Some(Sel::Item(new));
            self.edited();
            return;
        }
        let p = level.doc.pos(it) + Vec3::new(60.0, 0.0, 60.0);
        let (facing, team, weapon) = (level.doc.facing(it).unwrap_or(0.0), level.doc.team(it).unwrap_or(0), level.doc.crate_weapon(it));
        if let Some(new) = level.doc.add(it.kind, p, facing, team, weapon) {
            self.sel = Some(Sel::Item(new));
            self.edited();
        }
    }

    /// Link the waypoints the Link tool joins: a link between them cut, else
    /// made (only `a` → `b` if `one_way`).
    fn toggle_link(&mut self, a: usize, b: usize, one_way: bool) {
        let Some(level) = self.level.as_mut() else { return };
        let Some(scene) = level.scene.as_mut() else { return };
        let (pa, pb) = (scene.graph.pos[a], scene.graph.pos[b]);
        if scene.graph.linked(a, b) {
            level.doc.wp_unlink(pa, pb);
            scene.graph.unlink(a, b);
        } else {
            level.doc.wp_link(pa, pb, one_way);
            scene.graph.link(a, b, one_way);
        }
        self.edited();
    }

    /// Link the selected waypoint to every waypoint within 4.5 m a chr can
    /// walk to or from (the generator's own probe), as edits.
    fn auto_link(&mut self) {
        let Some(Sel::Node(i)) = self.sel else { return };
        let Some(level) = self.level.as_mut() else { return };
        let Some(scene) = level.scene.as_mut() else { return };
        let params = pd_sim::nav::gen::GenParams::default();
        let floor = |p: Vec3| p - Vec3::Y * PAD_HEIGHT;
        let p = scene.graph.pos[i];
        let mut made = 0;
        for j in 0..scene.graph.pos.len() {
            let q = scene.graph.pos[j];
            if j == i || q.distance(p) > 450.0 || scene.graph.linked(i, j) {
                continue;
            }
            let there = pd_sim::nav::gen::probe_walk(&scene.level, floor(p), floor(q), &params).is_some();
            let back = pd_sim::nav::gen::probe_walk(&scene.level, floor(q), floor(p), &params).is_some();
            match (there, back) {
                (true, true) => level.doc.wp_link(p, q, false),
                (true, false) => level.doc.wp_link(p, q, true),
                (false, true) => level.doc.wp_link(q, p, true),
                (false, false) => continue,
            }
            match (there, back) {
                (true, true) => scene.graph.link(i, j, false),
                (true, false) => scene.graph.link(i, j, true),
                _ => scene.graph.link(j, i, true),
            }
            made += 1;
        }
        self.say(format!("{made} links made"), false);
        if made > 0 {
            self.edited();
        }
    }

    // ── The panel ──────────────────────────────────────────────────────────


    /// Take the level generator in hand: on a level made before (its form
    /// and the generator's recipe), or on seed 40 as it falls.
    fn generate(&mut self, made: Option<(Form, autolevel::recipe::Recipe)>) {
        let (form, recipe) = made.unwrap_or_else(|| (Form::for_seed(40, &self.paths), shape::new_recipe(40)));
        self.generating = Some(Generating::new(recipe, form, &self.paths));
        self.new_level = None;
        self.hover_view = false;
    }

    /// The generator's panel: the arena its level becomes, then its own.
    fn generator_panel(&mut self, ui: &mut egui::Ui) {
        let Some(g) = self.generating.as_mut() else { return };
        g.form.follow(g.ed.doc.recipe.seed, &self.paths);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Arena name").strong());
            let was = g.form.name.clone();
            ui.add(egui::TextEdit::singleline(&mut g.form.name).desired_width(f32::INFINITY));
            if g.form.name != was {
                // (its code follows its name)
                g.form.code = crate::newlevel::code_from(&g.form.name);
                g.form.auto = false;
            }
        });
        let problem = g.form.problem(&self.paths);
        if let Some(p) = &problem {
            ui.colored_label(theme::WARN, p);
        }
        if g.form.has_layout(&self.paths) {
            theme::help(ui, "Items you placed by hand are kept; check them in the Items step if rooms moved.");
        }
        // (a recipe that makes no level makes no arena; the generator's panel says why)
        // (nor a plan with problems: rooms drawn over each other, a spire too short)
        let ready = problem.is_none() && self.job.is_none() && g.ed.doc.error().is_none() && g.ed.doc.made().is_some() && g.ed.doc.problems().is_empty();
        let (mut make, mut close) = (false, false);
        ui.horizontal(|ui| {
            make = theme::primary(ui, "Build the arena", ready).on_hover_text("Make it an arena you can play (about 8 seconds); then place items on it").clicked();
            if self.level.is_some() {
                close = ui.button("Cancel").on_hover_text("Leave the rooms as they were").clicked();
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.shape_advanced, false, "Simple");
            ui.selectable_value(&mut self.shape_advanced, true, "Advanced").on_hover_text("Every setting the generator has, its lists of rooms, joins and dead ends");
        });
        ui.separator();
        if self.shape_advanced {
            g.ed.panel(ui);
        } else {
            self.shape_simple(ui);
        }
        let Some(g) = self.generating.as_mut() else { return };
        if make {
            let written = g.form.write(&g.ed.doc.recipe, &self.paths);
            match written {
                Ok(path) => {
                    self.generating = None;
                    self.open_with(path, true);
                    self.step = Step::Items;
                }
                Err(e) => self.say(e, true),
            }
        } else if close {
            self.generating = None;
        }
    }





    /// The gun a location holds under the preview set.
    fn location_gun(&self, loc: u8) -> String {
        let w = pd_core::mp::mp_get_mp_weapon_by_location(&self.weapons(), loc as i32);
        self.res.as_ref().and_then(|r| r.gset.weapon(w.weaponnum as u8).map(|g| g.name.clone())).unwrap_or_else(|| if w.weaponnum == pd_core::ids::WEAPON_MPSHIELD as i32 { "shield".into() } else { format!("weapon {}", w.weaponnum) })
    }

    fn sel_buttons(&mut self, ui: &mut egui::Ui, item: bool) {
        let mut act = None;
        ui.horizontal(|ui| {
            if item && ui.button("Duplicate").on_hover_text("Shift+D").clicked() {
                act = Some('d');
            }
            if ui.button("Frame").on_hover_text("F").clicked() {
                act = Some('f');
            }
            if ui.button(egui::RichText::new("Delete").color(theme::DANGER)).on_hover_text("Delete key").clicked() {
                act = Some('x');
            }
        });
        match act {
            Some('d') => self.duplicate_sel(),
            Some('f') => {
                if let Some(p) = self.sel_pos() {
                    self.frame_on(p);
                }
            }
            Some('x') => self.delete_sel(),
            _ => {}
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let Some(Sel::Item(sel)) = self.sel else { return };
        if sel.kind == Kind::Door {
            return self.door_inspector(ui, sel.index);
        }
        let Some(level) = self.level.as_ref() else { return };
        let doc = &level.doc;
        let (pos, facing, team, loc, cw) = (doc.pos(sel), doc.facing(sel), doc.team(sel), doc.location(sel), doc.crate_weapon(sel));
        let nweapons = doc.count(Kind::Weapon);
        let crates = (sel.kind == Kind::Weapon).then(|| (0..doc.count(Kind::Ammo)).filter(|&i| doc.crate_weapon(Item { kind: Kind::Ammo, index: i }) == Some(sel.index)).count());
        let gun = loc.map(|l| self.location_gun(l));
        let guns: Vec<String> = (0..nweapons).map(|i| self.location_gun(doc.location(Item { kind: Kind::Weapon, index: i }).unwrap_or(0))).collect();
        let set = self.res.as_ref().map_or(String::new(), |r| r.lang.get(MP_WEAPON_SETS[self.weapon_set].name).to_owned());
        let title = match sel.kind {
            Kind::Weapon => format!("Weapon spot {}", sel.index + 1),
            k => format!("{} {}", items::kind_word(k), sel.index + 1),
        };
        ui.label(egui::RichText::new(title).size(16.0).strong());
        theme::help(ui, items::kind_does(sel.kind));
        let mut changed = false;
        let level = self.level.as_mut().unwrap();
        if let Some(mut l) = loc {
            theme::section(ui, "What it holds");
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(gun.clone().unwrap_or_default()).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| theme::help(ui, &format!("in {set}")));
                });
            });
            ui.horizontal(|ui| {
                ui.label("Slot in the weapon set");
                if ui.add(egui::DragValue::new(&mut l).range(0..=15)).on_hover_text("Which of the set's guns appears here (its MP location)").changed() {
                    level.doc.set_location(sel, l);
                    changed = true;
                }
            });
            theme::help(ui, "The gun changes with the weapon set chosen in the game; the set to show is above the view.");
        }
        theme::section(ui, "Position");
        let mut p = pos;
        ui.horizontal(|ui| {
            for (k, name) in [(0, "X"), (1, "Y"), (2, "Z")] {
                ui.label(egui::RichText::new(name).color(theme::MUTED));
                let r = ui.add(egui::DragValue::new(&mut p[k]).speed(5.0).max_decimals(0));
                if r.drag_started() || r.gained_focus() {
                    level.doc.begin_drag();
                }
                if r.changed() {
                    level.set_pos(sel, p);
                    changed = true;
                }
            }
        });
        if let Some(mut f) = facing {
            ui.horizontal(|ui| {
                ui.label("Facing");
                if ui.add(egui::Slider::new(&mut f, 0.0..=359.9).step_by(1.0).suffix("°")).changed() {
                    level.doc.set_facing(sel, f);
                    changed = true;
                }
            });
        }
        if let Some(mut t) = team {
            theme::section(ui, "Team");
            ui.horizontal(|ui| {
                for (k, name) in ["Red", "Yellow", "Blue", "Magenta"].into_iter().enumerate() {
                    if ui.selectable_value(&mut t, k as u8, egui::RichText::new(name).color(rgba(overlay::TEAMS[k]))).changed() {
                        level.doc.set_team(sel, t);
                        changed = true;
                    }
                }
            });
        }
        if let Some(mut w) = cw {
            theme::section(ui, "Ammo for");
            let before = w;
            let name = |i: usize| format!("Spot {}  ·  {}", i + 1, guns.get(i).cloned().unwrap_or_default());
            egui::ComboBox::from_id_salt("crate for").selected_text(name(w)).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                for i in 0..nweapons {
                    ui.selectable_value(&mut w, i, name(i));
                }
            });
            if w != before {
                level.doc.set_crate_weapon(sel, w);
                changed = true;
            }
        }
        let mut add_crate = false;
        if let Some(n) = crates {
            theme::section(ui, "Ammo for this gun");
            ui.horizontal(|ui| {
                ui.label(match n {
                    0 => "No crate yet".to_owned(),
                    1 => "1 crate".to_owned(),
                    n => format!("{n} crates"),
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| add_crate = ui.button("Add a crate").clicked());
            });
        }
        if add_crate {
            let at = level.doc.pos(sel) + Vec3::new(80.0, 0.0, 0.0);
            level.doc.add(Kind::Ammo, at, 0.0, 0, Some(sel.index));
            // (still the weapon in hand)
            self.sel = Some(Sel::Item(sel));
            changed = true;
        }
        if changed {
            self.edited();
        }
        ui.add_space(10.0);
        self.sel_buttons(ui, true);
    }


    /// The simulants' paths (the waypoints): their tools and what they are.
    fn paths_left(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::section(ui, "Simulant paths");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Done").clicked() {
                    self.tab = Tab::Items;
                    self.tool = Tool::Select;
                }
            });
        });
        theme::help(ui, "The routes simulants walk, made for you from the level's floors. Red: places they can't reach and get back from.");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "Select");
            ui.selectable_value(&mut self.tool, Tool::AddNode, "Add a waypoint").on_hover_text("Click a floor to place one");
            if ui.selectable_value(&mut self.tool, Tool::Link, "Join").on_hover_text("Click one waypoint, then another: joins them (Shift: one way), or cuts them apart").changed() {
                self.link_from = None;
            }
        });
        if self.tool == Tool::Link {
            theme::help(ui, "Click a waypoint, then another: they are joined both ways (hold Shift for one way, first to second), or cut apart if they were joined. The second becomes the next first. Esc stops.");
        }
        if let Some(scene) = self.scene() {
            let g = &scene.graph;
            let stuck = g.stranded();
            ui.label(format!("{} waypoints, {} joins", g.pos.len(), g.links.len()));
            if stuck > 0 {
                ui.colored_label(theme::WARN, format!("{stuck} in red: simulants can't reach them and get back"));
            }
        }
        let edits = self.level.as_ref().map_or(0, |l| l.doc.wp_edit_count());
        theme::help(ui, &format!("{edits} changes by hand, kept each time the paths are made again."));
        ui.horizontal(|ui| {
            if ui.add_enabled(self.job.is_none(), egui::Button::new("Make them again")).on_hover_text("From the level's floors, then your changes on top").clicked() {
                self.start(Work::Place { fresh: true });
            }
            if ui.add_enabled(edits > 0, egui::Button::new("Undo my changes")).clicked() {
                if let Some(l) = self.level.as_mut() {
                    l.doc.wp_clear();
                }
                self.sel = None;
                self.edited();
            }
        });
    }

    /// The selected waypoint: where it is and what it is joined to.
    fn waypoint_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(Sel::Node(i)) = self.sel else { return };
        let Some(scene) = self.scene() else { return };
        let Some(&pos) = scene.graph.pos.get(i) else { return };
        let links: Vec<(usize, bool, bool)> = scene.graph.links.iter().filter(|l| l.0 == i || l.1 == i).map(|&(a, b, one_way)| if a == i { (b, one_way, true) } else { (a, one_way, false) }).collect();
        let part = scene.graph.part.get(i).copied().unwrap_or(0);
        ui.label(egui::RichText::new(format!("Waypoint {i}")).strong());
        if part != 0 {
            ui.colored_label(egui::Color32::from_rgb(255, 120, 90), "outside the main part: link it to the rest both ways");
        }
        let mut p = pos;
        let mut commit = false;
        ui.horizontal(|ui| {
            for (k, name) in [(0, "x"), (1, "y"), (2, "z")] {
                ui.label(name);
                let r = ui.add(egui::DragValue::new(&mut p[k]).speed(5.0).max_decimals(1));
                if (r.drag_started() || r.gained_focus()) && self.node_edit_start.is_none() {
                    self.node_edit_start = Some(pos);
                }
                if r.drag_stopped() || r.lost_focus() {
                    commit = true;
                }
            }
        });
        if p != pos {
            self.move_sel_live(p);
        }
        if commit {
            if let (Some(start), Some(level)) = (self.node_edit_start.take(), self.level.as_mut()) {
                level.doc.wp_move(start, p);
                self.reselect_node = Some(p);
                self.edited();
            }
        }
        ui.label(format!("{} links:", links.len()));
        let mut cut = None;
        for (j, one_way, out) in &links {
            ui.horizontal(|ui| {
                let arrow = match (one_way, out) {
                    (false, _) => "both ways",
                    (true, true) => "one way, out",
                    (true, false) => "one way, in",
                };
                ui.label(format!("to {j} ({arrow})"));
                if ui.small_button("cut").clicked() {
                    cut = Some(*j);
                }
            });
        }
        if let Some(j) = cut {
            self.toggle_link(i, j, false);
        }
        ui.horizontal(|ui| {
            if ui.button("Auto-link").on_hover_text("To every waypoint within 4.5 m a chr can walk to or from").clicked() {
                self.auto_link();
            }
        });
        self.sel_buttons(ui, false);
    }


    /// The catalogue: every door model PD's and GoldenEye's setups place, each
    /// with the ways it is placed (its templates).
    fn door_catalogue(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Door models").strong());
        ui.horizontal(|ui| {
            ui.label("Find");
            ui.text_edit_singleline(&mut self.door_filter);
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.door_game, None, "Both games");
            ui.selectable_value(&mut self.door_game, Some(DoorGame::Pd), "Perfect Dark");
            ui.selectable_value(&mut self.door_game, Some(DoorGame::Ge), "GoldenEye");
        });
        if !self.catalogue.doors.iter().any(|d| d.game == DoorGame::Ge) {
            ui.colored_label(egui::Color32::from_rgb(255, 190, 90), "GoldenEye's doors aren't extracted yet (they come from the ROM Facility's recipe names).");
            if ui.add_enabled(self.job.is_none() && self.level.is_some(), egui::Button::new("Extract GoldenEye's doors")).clicked() {
                self.start(Work::GeDoors);
            }
        }
        let filter = self.door_filter.to_ascii_lowercase();
        let game = self.door_game;
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("doorcat").max_height(230.0).show(ui, |ui| {
            for c in self.catalogue.doors.iter().filter(|c| game.is_none_or(|g| c.game == g)) {
                let title = c.title();
                let seen = c.seen.join(", ");
                if !filter.is_empty() && ![&title, &c.stem, &seen].iter().any(|t| t.to_ascii_lowercase().contains(&filter)) {
                    continue;
                }
                let motion = c.templates.first().map(|t| c.motion(t).name()).unwrap_or("");
                let tag = if c.game == DoorGame::Ge { "GE" } else { "PD" };
                let on = self.door_pick.as_ref().is_some_and(|(s, _)| *s == c.stem);
                let r = ui.selectable_label(on, format!("{tag}  {title}  · {motion}")).on_hover_text(format!("{}\nseen on: {seen}", c.stem));
                if r.clicked() {
                    picked = Some(c.stem.clone());
                }
            }
        });
        if let Some(stem) = picked {
            self.door_pick = Some((stem, 0));
            self.tool = Tool::AddDoor;
        }
        let Some((stem, t)) = self.door_pick.clone() else { return };
        let Some(c) = self.catalogue.get(&stem) else { return };
        let label = |k: usize| {
            let x = &c.templates[k];
            format!("{}: {}, {:.0} x {:.0} cm (placed {}x)", k + 1, c.motion(x).name(), x.size[0], x.size[1], x.count)
        };
        let mut chosen = t;
        egui::ComboBox::from_label("as placed").selected_text(label(t.min(c.templates.len() - 1))).show_ui(ui, |ui| {
            for k in 0..c.templates.len() {
                ui.selectable_value(&mut chosen, k, label(k));
            }
        });
        if chosen != t {
            self.door_pick = Some((stem, chosen));
        }
    }

    /// Every setting of door `i`.
    /// The selected door's settings, as the other items' are: what it is,
    /// where it stands and how big, how it opens, how it closes, and what
    /// it does (its PD flags in plain words; the PD names are the hovers).
    fn door_inspector(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(level) = self.level.as_ref() else { return };
        if i >= level.doc.count(Kind::Door) {
            return;
        }
        let d = level.doc.door(i).clone();
        let ring = level.doc.ring(i);
        let it = Item { kind: Kind::Door, index: i };
        let cat = self.catalogue.get(&d.model).cloned();
        ui.label(egui::RichText::new(format!("Door {}", i + 1)).size(16.0).strong());
        theme::help(ui, items::kind_does(Kind::Door));
        let mut e = d.clone();
        let (mut live, mut step, mut began) = (false, false, false);
        let drag = |r: &egui::Response, live: &mut bool, began: &mut bool| {
            if r.drag_started() || r.gained_focus() {
                *began = true;
            }
            if r.changed() {
                *live = true;
            }
        };
        let mut act = None;

        theme::section(ui, "Model");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            match &cat {
                Some(c) => {
                    ui.label(egui::RichText::new(c.title()).strong());
                    theme::help(ui, &format!("From {}{}", if c.game == DoorGame::Ge { "GoldenEye" } else { "Perfect Dark" }, if c.seen.is_empty() { String::new() } else { format!(": {}", c.seen.join(", ")) }));
                }
                None => {
                    ui.label(egui::RichText::new(&d.model).strong());
                    ui.colored_label(theme::DANGER, "This model is missing (GoldenEye's doors need extracting first).");
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if let Some((stem, _)) = self.door_pick.as_ref().filter(|(s, _)| *s != d.model) {
                let title = self.catalogue.get(stem).map_or(stem.clone(), |c| c.title());
                if ui.button("Use the picked model").on_hover_text(title).clicked() {
                    act = Some("model");
                }
            }
            if ui.button("Its usual settings").on_hover_text("How the model opens, closes and sounds in its own game (the picked template, else its most used)").clicked() {
                act = Some("template");
            }
        });

        // Where it stands (the other leaves move and turn with it).
        theme::section(ui, "Position");
        let mut p = Vec3::from(d.pos);
        let mut facing = d.facing;
        ui.horizontal(|ui| {
            for (k, name) in [(0, "X"), (1, "Y"), (2, "Z")] {
                ui.label(egui::RichText::new(name).color(theme::MUTED));
                let r = ui.add(egui::DragValue::new(&mut p[k]).speed(5.0).max_decimals(0));
                drag(&r, &mut live, &mut began);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Facing");
            let r = ui.add(egui::Slider::new(&mut facing, 0.0..=359.9).step_by(1.0).suffix("°"));
            drag(&r, &mut live, &mut began);
        });

        theme::section(ui, "Size");
        ui.horizontal(|ui| {
            for (k, name) in [(0, "Width"), (1, "Height"), (2, "Thick")] {
                ui.label(egui::RichText::new(name).color(theme::MUTED));
                let r = ui.add(egui::DragValue::new(&mut e.size[k]).speed(1.0).range(1.0..=5000.0).max_decimals(0).suffix(" cm"));
                drag(&r, &mut live, &mut began);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(ring.len() == 1, egui::Button::new("Fit to the doorway")).on_hover_text("Its width and height to the opening it stands in (a single door)").clicked() {
                act = Some("fit");
            }
            if ring.len() == 1 && ui.button("Make it a double door").on_hover_text("Two leaves of half its size opening together (top and bottom for one that rises or sinks)").clicked() {
                act = Some("double");
            }
            if ring.len() > 1 && ui.button("Make them separate").on_hover_text("Each leaf opens alone").clicked() {
                act = Some("separate");
            }
        });
        if ring.len() > 1 {
            theme::help(ui, &format!("Opens together with door{} {}.", if ring.len() > 2 { "s" } else { "" }, ring[1..].iter().map(|j| (j + 1).to_string()).collect::<Vec<_>>().join(", ")));
        }

        // How it opens.
        theme::section(ui, "How it opens");
        let before = e.motion;
        ui.horizontal(|ui| {
            ui.label("It");
            egui::ComboBox::from_id_salt("door motion").selected_text(motion_word(e.motion)).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                for m in DoorMotion::ALL {
                    ui.selectable_value(&mut e.motion, m, motion_word(m));
                }
            });
        });
        if e.motion != before {
            step = true;
            // A fraction and degrees don't convert: a sensible opening.
            if e.motion.turns() != before.turns() {
                e.maxfrac = if e.motion.turns() { 90 << 16 } else { 62259 };
                e.perimfrac = if e.motion.turns() { 1000 << 16 } else { 65536 };
            }
        }
        if matches!(e.motion, DoorMotion::Eyelid | DoorMotion::Iris) && cat.as_ref().is_some_and(|c| c.motion(&c.templates[0]) != e.motion) {
            theme::help(ui, "Only Caverns' eyelid and iris models move like that: this one will stand still.");
        }
        let (side_word, left, right) = match e.motion {
            DoorMotion::Swing | DoorMotion::Hull | DoorMotion::Chair => ("Hinge", "Left", "Right"),
            DoorMotion::Up | DoorMotion::Down | DoorMotion::Eyelid | DoorMotion::Iris => ("Hinge side", "Left", "Right"),
            _ => ("Slides to the", "Left", "Right"),
        };
        ui.horizontal(|ui| {
            ui.label(side_word);
            step |= ui.selectable_value(&mut e.side, Side::Left, left).changed();
            step |= ui.selectable_value(&mut e.side, Side::Right, right).changed();
            theme::help(ui, "(from the front)");
        });
        if matches!(e.motion, DoorMotion::Swing | DoorMotion::Hull | DoorMotion::Chair) {
            ui.horizontal(|ui| {
                ui.label("Swings");
                step |= ui.selectable_value(&mut e.swing, Swing::Back, "Away").on_hover_text("Away from its front").changed();
                step |= ui.selectable_value(&mut e.swing, Swing::Front, "Towards").on_hover_text("Towards its front").changed();
                step |= ui.selectable_value(&mut e.swing, Swing::Both, "Either way").on_hover_text("Away from whoever opens it").changed();
            });
        }
        ui.horizontal(|ui| {
            ui.label("Opens");
            if e.motion.turns() {
                let mut deg = e.maxfrac as f32 / 65536.0;
                let r = ui.add(egui::Slider::new(&mut deg, 1.0..=180.0).step_by(1.0).suffix("°"));
                drag(&r, &mut live, &mut began);
                e.maxfrac = (deg * 65536.0).round() as i32;
            } else {
                let mut pct = e.maxfrac as f32 / 655.36;
                let r = ui.add(egui::Slider::new(&mut pct, 5.0..=100.0).step_by(1.0).suffix("% of the way"));
                drag(&r, &mut live, &mut began);
                e.maxfrac = (pct * 655.36).round() as i32;
            }
        });
        let secs = pd_sim::props::door::door_open_frames(e.maxfrac, e.accel, e.decel, e.maxspeed).map(|f| f as f32 / 60.0);
        ui.horizontal(|ui| {
            ui.label("Speed");
            for (name, k) in [("Slower", 0.8f32), ("Faster", 1.25)] {
                if ui.button(name).clicked() {
                    // Speed and acceleration together: the same motion, quicker.
                    e.maxspeed = ((e.maxspeed as f32) * k).round().max(1.0) as i32;
                    e.accel = ((e.accel as f32) * k * k).round().max(1.0) as i32;
                    e.decel = ((e.decel as f32) * k * k).round().max(1.0) as i32;
                    step = true;
                }
            }
            theme::help(ui, &match secs {
                Some(t) => format!("open in {t:.1} s"),
                None => "never opens all the way".into(),
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Show it open").clicked() {
                act = Some("open");
            }
            if ui.button("Show it closed").clicked() {
                act = Some("close");
            }
        });

        theme::section(ui, "Closing and sound");
        let mut closes = e.autoclosetime < NEVER;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut closes, "Closes by itself after").changed() {
                e.autoclosetime = if closes { 900 } else { NEVER };
                step = true;
            }
            if closes {
                let mut s = e.autoclosetime as f32 / 60.0;
                let r = ui.add(egui::DragValue::new(&mut s).range(0.0..=600.0).speed(0.1).suffix(" s"));
                drag(&r, &mut live, &mut began);
                e.autoclosetime = (s * 60.0).round() as i32;
            }
        });
        let sound = |k: u8| if k == 0 { "Silent".to_owned() } else { format!("Sound {k}") };
        ui.horizontal(|ui| {
            ui.label("Sound");
            egui::ComboBox::from_id_salt("door sound").selected_text(sound(e.soundtype)).show_ui(ui, |ui| {
                for &k in &DOOR_SOUNDS {
                    step |= ui.selectable_value(&mut e.soundtype, k, sound(k)).changed();
                }
            });
        });

        theme::section(ui, "What it does");
        let flag = |ui: &mut egui::Ui, bits: &mut u16, bit: u16, text: &str, tip: &str| {
            let mut on = *bits & bit != 0;
            let changed = ui.checkbox(&mut on, text).on_hover_text(tip).changed();
            if changed {
                *bits ^= bit;
            }
            changed
        };
        step |= flag(ui, &mut e.doorflags, 0x0010, "Opens when someone walks up", "Like the arenas' sliding doors (DOORFLAG_AUTOMATIC)");
        step |= flag(ui, &mut e.doorflags, 0x0200, "Can be opened from further away", "From 4 m, not 2 (DOORFLAG_LONGRANGE)");
        let mut keep = e.flags & pd_core::ids::OBJFLAG_DOOR_KEEPOPEN != 0;
        if ui.checkbox(&mut keep, "Starts open").on_hover_text("OBJFLAG_DOOR_KEEPOPEN").changed() {
            e.flags ^= pd_core::ids::OBJFLAG_DOOR_KEEPOPEN;
            step = true;
        }
        let mut ai = e.flags2 & pd_core::ids::OBJFLAG2_AICANNOTUSE != 0;
        if ui.checkbox(&mut ai, "Simulants can't open it").on_hover_text("OBJFLAG2_AICANNOTUSE").changed() {
            e.flags2 ^= pd_core::ids::OBJFLAG2_AICANNOTUSE;
            step = true;
        }
        egui::CollapsingHeader::new(egui::RichText::new("More").color(theme::LABEL)).id_salt("door more").show(ui, |ui| {
            step |= flag(ui, &mut e.doorflags, 0x0004, "Hidden where it slides into the wall", "Its model is cut where it has slid into the frame (DOORFLAG_0004)");
            step |= flag(ui, &mut e.doorflags, 0x0008, "Mirrored", "The model mirrored through the door's plane (DOORFLAG_FLIP)");
            step |= flag(ui, &mut e.doorflags, 0x0002, "See-through", "Its glass keeps the doorway's view open (DOORFLAG_WINDOWED; for a model with windows)");
        });

        let level = self.level.as_mut().unwrap();
        if began {
            level.doc.begin_drag();
        }
        if p != Vec3::from(d.pos) {
            level.doc.set_pos(it, p);
        }
        if facing != d.facing {
            level.doc.set_facing_live(it, facing);
        }
        let shape = |x: &pd_import::layout::Door| (x.size, x.maxfrac, x.autoclosetime);
        if step {
            let (pos, fc) = (level.doc.door(i).pos, level.doc.door(i).facing);
            level.doc.edit_door(i, |x| *x = pd_import::layout::Door { pos, facing: fc, sibling: x.sibling, ..e.clone() });
        } else if live && shape(&e) != shape(&d) {
            level.doc.edit_door_live(i, |x| {
                x.size = e.size;
                x.maxfrac = e.maxfrac;
                x.autoclosetime = e.autoclosetime;
            });
        }
        if step || live || began {
            self.edited();
        }
        match act {
            Some("fit") => self.fit_sel_door(i),
            Some("double") => {
                if let Some(j) = self.level.as_mut().and_then(|l| l.doc.make_double(i)) {
                    self.say(format!("Door {} is now a double door with door {}", i + 1, j + 1), false);
                    self.edited();
                }
            }
            Some("separate") => {
                if let Some(l) = self.level.as_mut() {
                    l.doc.separate(i);
                }
                self.edited();
            }
            Some("model") => {
                if let (Some((stem, _)), Some(l)) = (self.door_pick.clone(), self.level.as_mut()) {
                    l.doc.edit_door(i, |x| x.model = stem);
                    self.edited();
                }
            }
            Some("template") => self.apply_template(i),
            Some("open") => self.preview_doors(true),
            Some("close") => self.preview_doors(false),
            _ => {}
        }
        ui.add_space(10.0);
        self.sel_buttons(ui, true);
    }

    /// Door `i` given its model's settings as the catalogue has them (the
    /// picked template if it is that model, else the most used), keeping
    /// where it stands, its size and its side.
    fn apply_template(&mut self, i: usize) {
        let Some(level) = self.level.as_mut() else { return };
        let d = level.doc.door(i).clone();
        let Some(c) = self.catalogue.get(&d.model) else { return self.say(format!("{} isn't in the catalogues", d.model), true) };
        let t = self.door_pick.as_ref().filter(|(s, _)| *s == d.model).map_or(0, |p| p.1);
        let fresh = c.door(t, Vec3::from(d.pos), d.facing);
        level.doc.edit_door(i, |x| *x = pd_import::layout::Door { pos: d.pos, facing: d.facing, size: d.size, side: d.side, sibling: d.sibling, ..fresh });
        self.edited();
    }

    /// Door `i` fitted to the doorway it stands in.
    fn fit_sel_door(&mut self, i: usize) {
        let Some(level) = self.level.as_mut() else { return };
        let Some(scene) = level.scene.as_ref() else { return };
        let d = level.doc.door(i).clone();
        let Some((sill, f, w, h)) = scene.fit_doorway(Vec3::from(d.pos), d.facing) else { return self.say("no doorway here: no walls within 4 m either side", true) };
        // Of the facing and its opposite, the one nearer the door's own.
        let f = if (f - d.facing + 540.0).rem_euclid(360.0) - 180.0 > 90.0 || (f - d.facing + 540.0).rem_euclid(360.0) - 180.0 < -90.0 { (f + 180.0) % 360.0 } else { f };
        level.doc.edit_door(i, |x| {
            x.pos = sill.to_array();
            x.facing = f;
            x.size[0] = w;
            if let Some(h) = h {
                x.size[1] = h;
            }
        });
        let msg = format!("fitted: {w:.0} cm wide{}", h.map_or(String::new(), |h| format!(", {h:.0} cm high")));
        self.say(msg, false);
        self.edited();
    }

    /// Every door of the view's world opened or closed (it runs for a few
    /// seconds; the next placement shuts them again).
    fn preview_doors(&mut self, open: bool) {
        if let Some(scene) = self.level.as_mut().and_then(|l| l.scene.as_mut()) {
            scene.world.doors_preview(open);
            self.preview = Some(Instant::now());
            self.preview_acc = 0.0;
        }
    }


    // ── The view ───────────────────────────────────────────────────────────

    fn view(&self, w: f32, h: f32) -> pd_render::View {
        let (znear, zfar) = self.renderer.as_ref().map_or((15.0, 10000.0), |r| r.z_range());
        self.cam.view(w / h.max(1.0), znear, if self.far { zfar.max(60000.0) } else { zfar })
    }

    /// What the click at the ray `o + t d` selects: the nearest pick box in
    /// front of the first wall or floor it meets.
    fn pick_at(&self, o: Vec3, d: Vec3) -> Option<Sel> {
        let scene = self.scene()?;
        let wall = scene.level.first_tile_hit(o, o + d * 100_000.0).map_or(f32::INFINITY, |h| h.dist);
        self.built
            .picks
            .iter()
            .filter_map(|p| gizmo::ray_aabb(o, d, p.lo, p.hi).map(|t| (p.sel, t)))
            .filter(|&(_, t)| t <= wall + 40.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(s, _)| s)
    }

    fn viewport(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        self.hover_view = resp.hovered() || self.looking;
        if resp.hovered() && ui.input(|i| i.pointer.secondary_pressed()) {
            self.capture = Some(true);
            self.looking = true;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.cam.speed = (self.cam.speed * 1.15f32.powf(scroll / 50.0)).clamp(50.0, 20000.0);
            }
        }
        let (w, h) = (rect.width(), rect.height());
        let view = self.view(w, h);
        let painter = ui.painter_at(rect);
        if self.scene().is_none() {
            self.built = overlay::Built::default();
            let failed = self.level.as_ref().and_then(|l| l.failed.clone()).filter(|_| self.job.is_none());
            let Some(why) = failed else {
                let text = match (&self.level, &self.job) {
                    (None, _) => "Open a level (left)".to_owned(),
                    (Some(_), Some(job)) => format!("{}…", job.work.label()),
                    (Some(_), None) => "Loading the stage…".to_owned(),
                };
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(18.0), egui::Color32::GRAY);
                return;
            };
            // The build failed: say why, and offer it again.
            let mut again = false;
            let area = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width().min(560.0), 200.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
                ui.label(egui::RichText::new("The arena could not be built").size(18.0).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&why).color(theme::DANGER));
                ui.add_space(10.0);
                again = theme::primary(ui, "Try again", true).clicked();
            });
            if again {
                self.start(Work::Import);
            }
            return;
        }
        let local = |p: egui::Pos2| [p.x - rect.min.x, p.y - rect.min.y];
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p) && !self.looking).map(local);
        let ray = pointer.map(|p| camera::ray(&view, p[0], p[1], w, h));
        let (ctrl, shift) = ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
        // The gizmo on the selection, and the handle under the pointer.
        let giz = self.sel_pos().map(|p| (p, gizmo::size(view.eye, p), self.gizmo_for_sel()));
        self.hot = match (&self.dragging, giz, ray) {
            (Some(d), _, _) => Some(d.drag.handle),
            (None, Some((p, s, mode)), Some((o, d))) if self.tool == Tool::Select || matches!(self.sel, Some(Sel::Node(_))) => gizmo::pick(mode, p, s, o, d),
            _ => None,
        };
        self.hovered = if self.hot.is_some() { None } else { ray.and_then(|(o, d)| self.pick_at(o, d)) };

        let mut edited = false;
        let pressed = !self.looking && resp.hovered() && ui.input(|i| i.pointer.primary_pressed());
        if let (true, Some((o, d))) = (pressed, ray) {
            if let (Some(handle), Some(sel), Some((p, _, _))) = (self.hot, self.sel, giz) {
                if let Some(drag) = gizmo::Drag::start(handle, p, self.sel_facing().unwrap_or(0.0), o, d) {
                    if let (Sel::Item(_), Some(level)) = (sel, self.level.as_mut()) {
                        level.doc.begin_drag();
                    }
                    self.dragging = Some(Dragging { drag, sel, start: p });
                }
            } else {
                let floor_hit = self.scene().and_then(|s| s.pick(o, d, 100_000.0).and_then(|(hit, _)| s.stand(hit)));
                match self.tool {
                    Tool::Select => self.sel = self.hovered,
                    Tool::Add(kind) => {
                        if let Some(p) = floor_hit {
                            let facing = self.cam.yaw.to_degrees().rem_euclid(360.0);
                            let weapon = match self.sel {
                                Some(Sel::Item(s)) if s.kind == Kind::Weapon => Some(s.index),
                                _ => None,
                            };
                            let team = self.team;
                            let added = self.level.as_mut().and_then(|l| l.doc.add(kind, p, facing, team, weapon));
                            match added {
                                Some(it) => {
                                    self.sel = Some(Sel::Item(it));
                                    edited = true;
                                }
                                None => self.say("place a weapon first: a crate holds its weapon's ammo", true),
                            }
                        }
                    }
                    Tool::AddNode => {
                        if let (Some(p), Some(level)) = (floor_hit, self.level.as_mut()) {
                            level.doc.wp_add(p);
                            if let Some(scene) = level.scene.as_mut() {
                                self.sel = Some(Sel::Node(scene.graph.add(p)));
                            }
                            self.reselect_node = Some(p);
                            edited = true;
                        }
                    }
                    Tool::AddDoor => {
                        if let (Some(p), Some((stem, t))) = (floor_hit, self.door_pick.clone()) {
                            let floor = p - Vec3::Y * PAD_HEIGHT;
                            // Its front towards the camera, square to 15°.
                            let facing = ((self.cam.yaw.to_degrees() + 180.0) / 15.0).round() * 15.0;
                            let eye = self.cam.pos;
                            let fit = if self.door_fit { self.scene().and_then(|s| s.fit_doorway(floor, facing)) } else { None };
                            if let Some(c) = self.catalogue.get(&stem) {
                                let mut d = c.door(t, floor, facing);
                                if let Some((sill, f, w, h)) = fit {
                                    let front = pd_import::layout::look(f);
                                    d.pos = sill.to_array();
                                    d.facing = if front.dot(eye - sill) >= 0.0 { f } else { (f + 180.0) % 360.0 };
                                    d.size[0] = w;
                                    if let Some(h) = h {
                                        d.size[1] = h;
                                    }
                                }
                                if let Some(level) = self.level.as_mut() {
                                    self.sel = Some(Sel::Item(level.doc.add_door(d)));
                                    edited = true;
                                }
                            }
                        }
                    }
                    Tool::Link => match self.hovered {
                        Some(Sel::Node(j)) => {
                            if let Some(i) = self.link_from.filter(|&i| i != j) {
                                self.toggle_link(i, j, shift);
                            }
                            self.link_from = Some(j);
                            self.sel = Some(Sel::Node(j));
                        }
                        _ => self.link_from = None,
                    },
                }
            }
        }
        // A drag follows the pointer's ray.
        if let Some(dr) = &self.dragging {
            let (drag, sel, start) = (dr.drag, dr.sel, dr.start);
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some((o, d)) = ray {
                    match drag.handle {
                        Handle::Axis(_) => {
                            if let Some(p) = drag.moved(o, d, ctrl) {
                                self.move_sel_live(p);
                            }
                        }
                        Handle::Ring => {
                            if let (Some(f), Sel::Item(it), Some(level)) = (drag.turned(o, d, ctrl), sel, self.level.as_mut()) {
                                level.doc.set_facing_live(it, f);
                            }
                        }
                    }
                }
            } else {
                self.dragging = None;
                if let (Sel::Node(_), Some(end)) = (sel, self.sel_pos()) {
                    if let Some(level) = self.level.as_mut() {
                        level.doc.wp_move(start, end);
                    }
                    self.reselect_node = Some(end);
                }
                edited = true;
            }
        }
        // This frame's overlay, the gizmo on top.
        let show = Show { waypoints: self.show.waypoints || self.tab == Tab::Waypoints, ..self.show };
        let hl = Highlight { selected: self.sel, hovered: self.hovered, link_from: self.link_from };
        let mut built = {
            let level = self.level.as_ref().unwrap();
            overlay::build(&level.doc, level.scene.as_ref().unwrap(), view.eye, &show, &hl)
        };
        if let Some((p, _, mode)) = giz {
            let s = gizmo::size(view.eye, p);
            gizmo::draw(&mut built.mesh, mode, self.sel_pos().unwrap_or(p), s, self.hot, self.sel_facing());
        }
        // The labels, in the window only (over what's in sight, or faintly).
        let clip = camera::world_to_clip(&view);
        let off = |x: f32, y: f32| egui::pos2(x + rect.min.x, y + rect.min.y);
        if let Some(scene) = self.scene() {
            for l in &built.labels {
                let Some((x, y, _)) = camera::project(&clip, l.at, w, h) else { continue };
                let seen = scene.visible(view.eye, l.at);
                if !seen && !self.show.through_walls {
                    continue;
                }
                let mut col = rgba(l.col);
                if !seen {
                    col = col.gamma_multiply(0.4);
                }
                painter.text(off(x + 1.0, y + 1.0), egui::Align2::CENTER_BOTTOM, &l.text, egui::FontId::proportional(13.0), egui::Color32::from_black_alpha(if seen { 200 } else { 80 }));
                painter.text(off(x, y), egui::Align2::CENTER_BOTTOM, &l.text, egui::FontId::proportional(13.0), col);
            }
        }
        self.built = built;
        // What the tool does (a toast while one is in hand), and how to move
        // (until the tips are closed).
        let hint = match self.tool {
            Tool::Select => None,
            Tool::Add(k) => Some(format!("Click a floor to add a {}  ·  Esc to stop", items::kind_word(k).to_lowercase())),
            Tool::AddNode => Some("Click a floor to add a waypoint  ·  Esc to stop".to_owned()),
            Tool::Link => Some("Click two waypoints to join them (Shift: one way), or cut them apart  ·  Esc to stop".to_owned()),
            Tool::AddDoor => Some("Click inside a doorway: the door fits itself to it  ·  Esc to stop".to_owned()),
        };
        if let Some(hint) = hint {
            let galley = painter.layout_no_wrap(hint, egui::FontId::proportional(13.5), theme::TEXT);
            let at = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 14.0);
            let r = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(14.0, 8.0));
            painter.rect(r, 8.0, egui::Color32::from_rgba_unmultiplied(29, 42, 61, 245), egui::Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
            painter.galley(at, galley, theme::TEXT);
        }
        if self.tips {
            let area = egui::Rect::from_min_size(rect.left_bottom() + egui::vec2(12.0, -150.0), egui::vec2(280.0, 138.0));
            let mut close = false;
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
                egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(18, 21, 27, 240)).stroke(egui::Stroke::new(1.0, theme::BORDER_STRONG)).corner_radius(10).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        theme::section(ui, "Moving around");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| close = ui.small_button("Hide").on_hover_text("The help (?) has these too").clicked());
                    });
                    ui.spacing_mut().item_spacing = egui::vec2(5.0, 4.0);
                    ui.horizontal(|ui| theme::key_hint(ui, "Right-drag", "look around"));
                    ui.horizontal(|ui| {
                        theme::key_hint(ui, "W A S D", "fly");
                        theme::key_hint(ui, "Q E", "down, up");
                    });
                    ui.horizontal(|ui| {
                        theme::key_hint(ui, "Wheel", "speed");
                        theme::key_hint(ui, "F", "frame the selection");
                    });
                });
            });
            if close {
                self.tips = false;
            }
        }
        if edited {
            self.edited();
        }
    }

    fn shortcuts(&mut self, egui: &egui::Context) {
        use egui::{Key, Modifiers};
        let (save, redo, undo) = egui.input_mut(|i| {
            let save = i.consume_key(Modifiers::COMMAND, Key::S);
            let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
            let undo = i.consume_key(Modifiers::COMMAND, Key::Z);
            (save, redo, undo)
        });
        if save {
            self.save();
        }
        if redo {
            self.undo(true);
        } else if undo {
            self.undo(false);
        }
        if egui.wants_keyboard_input() || self.looking {
            return;
        }
        let (del, rot, frame, esc, toggle, ground, dup) = egui.input(|i| {
            let rot = if i.key_pressed(Key::R) { Some(if i.modifiers.shift { -45.0 } else { 45.0 }) } else { None };
            (i.key_pressed(Key::Delete), rot, i.key_pressed(Key::F), i.key_pressed(Key::Escape), i.key_pressed(Key::T), i.key_pressed(Key::G), i.key_pressed(Key::D) && i.modifiers.shift)
        });
        if esc {
            if self.tool != Tool::Select || self.link_from.is_some() {
                self.tool = Tool::Select;
                self.link_from = None;
            } else {
                self.sel = None;
            }
        }
        if toggle {
            self.gizmo_mode = if self.gizmo_mode == gizmo::Mode::Move { gizmo::Mode::Rotate } else { gizmo::Mode::Move };
        }
        if self.sel.is_none() {
            return;
        }
        if del {
            self.delete_sel();
        } else if ground {
            self.ground_sel();
        } else if dup {
            self.duplicate_sel();
        } else if frame {
            if let Some(p) = self.sel_pos() {
                self.frame_on(p);
            }
        } else if let (Some(r), Some(Sel::Item(it))) = (rot, self.sel) {
            if let Some(level) = self.level.as_mut() {
                if let Some(f) = level.doc.facing(it) {
                    level.doc.set_facing(it, f + r);
                    self.edited();
                }
            }
        }
    }
}

impl Game for App {
    fn init(&mut self, ctx: &mut Ctx) {
        self.presenter = Some(Presenter::new(ctx.gpu));
        self.init_gpu(&ctx.gpu.device, &ctx.gpu.queue);
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        let key = |k: KeyCode| if ctx.input.key_down(k) { 1.0 } else { 0.0 };
        let active = self.hover_view || self.looking;
        let (dx, dy) = ctx.input.mouse_delta();
        let m = if active {
            Motion {
                forward: key(KeyCode::KeyW) - key(KeyCode::KeyS),
                right: key(KeyCode::KeyD) - key(KeyCode::KeyA),
                up: key(KeyCode::KeyE) - key(KeyCode::KeyQ),
                fast: ctx.input.key_down(KeyCode::ShiftLeft) || ctx.input.key_down(KeyCode::ShiftRight),
                look: if self.looking { (dx as f32, dy as f32) } else { (0.0, 0.0) },
            }
        } else {
            Motion::default()
        };
        // Shift+D duplicates; it doesn't strafe (the generator's 3D view has no such key).
        let m = if self.generating.is_none() && ctx.input.key_down(KeyCode::ShiftLeft) && ctx.input.key_down(KeyCode::KeyD) && !self.looking { Motion { right: 0.0, ..m } } else { m };
        let dt = ctx.clock.tick_dt() as f32;
        match self.generating.as_mut() {
            // The generator's 3D view: its own camera flies, or its walker walks.
            Some(g) if g.space.middle == Middle::Space => {
                let pressed = |k: KeyCode| ctx.input.key_pressed(k);
                let controls = if active && self.keys_free {
                    Controls {
                        forward: (key(KeyCode::KeyW) - key(KeyCode::KeyS) + key(KeyCode::ArrowUp) - key(KeyCode::ArrowDown)).clamp(-1.0, 1.0),
                        right: key(KeyCode::KeyD) - key(KeyCode::KeyA),
                        turn: key(KeyCode::ArrowRight) - key(KeyCode::ArrowLeft),
                        look: m.look,
                        // (the game's keys: C or Ctrl down a level, Space up)
                        crouch_down: pressed(KeyCode::KeyC) || pressed(KeyCode::ControlLeft) || pressed(KeyCode::ControlRight),
                        crouch_up: pressed(KeyCode::Space),
                    }
                } else {
                    Controls::default()
                };
                g.space.advance(&m, controls, dt);
            }
            Some(_) => {}
            None => self.cam.update(&m, dt),
        }
        self.update(dt);
        self.load_bg(&ctx.gpu.device, &ctx.gpu.queue);
    }

    fn debug_ui(&mut self, ctx: &mut Ctx, egui: &egui::Context) {
        // The right button as egui sees it: the engine's input never gets a
        // press egui wants (the view is an egui area that takes clicks), but
        // egui has every press and release, captured cursor or not.
        if self.looking && !egui.input(|i| i.pointer.secondary_down()) {
            self.looking = false;
            self.capture = Some(false);
        }
        self.ui(egui);
        if let Some(on) = self.capture.take() {
            ctx.set_cursor_captured(on);
        }
        ctx.window.set_title(&self.title);
    }

    fn render(&mut self, ctx: &mut Ctx, frame: &mut Frame) {
        let [_, _, vw, vh] = frame.viewport;
        let (w, h) = ((vw.round() as u32).max(1), (vh.round() as u32).max(1));
        if self.target.as_ref().is_none_or(|t| t.width != w || t.height != h) {
            self.target = Some(RenderTarget::new(ctx.gpu, w, h, FORMAT, true));
        }
        let target = self.target.take().unwrap();
        let drawn = self.draw_view(&ctx.gpu.device, &ctx.gpu.queue, &mut frame.encoder, &target);
        match (drawn, self.presenter.as_ref()) {
            (true, Some(presenter)) => presenter.present(ctx.gpu, frame, &target, (w, h), Filter::Linear),
            _ => {
                frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("pd_edit clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &frame.view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(theme::VIEW_CLEAR), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
        }
        self.target = Some(target);
    }
}

impl App {
    /// What the window's `init` makes on its device (`--shot-ui` makes it on
    /// a headless one): the renderers, the game's resources, the catalogue,
    /// and the level asked for on the command line, opened.
    /// The view into `target`: the generator's 3D view (its preview's BG), else
    /// the open level with its markers; false if there is nothing to draw
    /// (the generator's plan covers the view, or no level is loaded).
    pub fn draw_view(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &RenderTarget) -> bool {
        let (w, h) = (target.width, target.height);
        let space = self.generating.as_ref().filter(|g| g.space.middle == Middle::Space && g.bg_loaded).map(|g| &g.space);
        if let (Some(space), Some(renderer)) = (space, self.gen_renderer.as_ref()) {
            let (znear, zfar) = renderer.z_range();
            let view = space.view(w as f32 / h as f32, znear, zfar);
            crate::shot::draw_bg(renderer, queue, encoder, target, &view);
            if let Some(draw3d) = self.draw3d.as_ref() {
                draw3d.draw(device, encoder, target, camera::world_to_clip(&view), &space.overlay(), true);
            }
            return true;
        }
        let scene = self.level.as_ref().filter(|l| l.bg_loaded && self.generating.is_none()).and_then(|l| l.scene.as_ref());
        let (Some(renderer), Some(draw3d), Some(scene)) = (self.renderer.as_mut(), self.draw3d.as_ref(), scene) else { return false };
        let (znear, zfar) = renderer.z_range();
        let view = self.cam.view(w as f32 / h as f32, znear, if self.far { zfar.max(60000.0) } else { zfar });
        renderer.render_free(device, queue, encoder, target, &scene.world, &view);
        draw3d.draw(device, encoder, target, camera::world_to_clip(&view), &self.built.mesh, self.show.through_walls);
        true
    }

    pub fn init_gpu(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.renderer = Some(Renderer::new(device, queue, FORMAT));
        self.draw3d = Some(Draw3d::new(device, FORMAT));
        match WorldRes::load(&self.paths.asset_dir()) {
            Ok(r) => self.res = Some(Arc::new(r)),
            Err(e) => self.say(format!("loading the game's resources: {e}"), true),
        }
        self.catalogue = Catalogue::load(&self.paths.asset_dir());
        if let Some(code) = self.open_code.take() {
            let p = self.paths.levels.join(format!("{code}.json"));
            self.open(p);
        }
    }
}

impl App {
    /// Everything a frame does that needs no window: the doors' preview, the
    /// generator, the jobs, and an edit's placement when it is due.
    pub fn update(&mut self, dt: f32) {
        if let Some(t0) = self.preview {
            if t0.elapsed().as_secs_f32() > 10.0 {
                self.preview = None;
            } else if let Some(scene) = self.level.as_mut().and_then(|l| l.scene.as_mut()) {
                self.preview_acc += dt;
                while self.preview_acc >= 1.0 / 60.0 {
                    scene.world.step(4, &[pd_sim::player::PlayerInput::default()]);
                    self.preview_acc -= 1.0 / 60.0;
                }
            }
        }
        if let Some(g) = self.generating.as_mut() {
            g.ed.tick();
            // The 3D view follows the recipe; a preview that has just come on
            // show has its BG loaded (a few hundredths of a second).
            if g.space.follow(&g.ed.doc.recipe) {
                g.bg_pending = true;
            }
        }
        self.poll_job();
        let due = self.level.as_ref().and_then(|l| l.pending).is_some_and(|t| t.elapsed().as_secs_f32() >= APPLY_AFTER) && self.dragging.is_none();
        if due && self.job.is_none() {
            self.start(Work::Place { fresh: false });
        }
        // Play now, once what it waited for is saved and placed.
        if self.play_queued && self.job.is_none() && self.level.as_ref().is_some_and(|l| l.pending.is_none()) {
            self.play_queued = false;
            self.play_now();
        }
    }

    /// Load the BG the view wants (the open level's, or the generator's
    /// preview's) into its renderer.
    pub fn load_bg(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if let Some(level) = self.level.as_mut().filter(|l| l.scene.is_some() && !l.bg_loaded && !l.bg_failed) {
            if let Some(r) = self.renderer.as_mut() {
                match r.load_stage(device, queue, &self.paths.asset_dir(), &level.recipe.code) {
                    Ok(()) => {
                        level.bg_loaded = true;
                        self.make_thumb(device, queue);
                    }
                    Err(e) => {
                        level.bg_failed = true;
                        let msg = format!("loading the stage's BG: {e}");
                        self.say(msg, true);
                    }
                }
            }
        }
        if let Some(g) = self.generating.as_mut().filter(|g| g.bg_pending) {
            g.bg_pending = false;
            if let Some(pv) = g.space.preview() {
                let renderer = self.gen_renderer.get_or_insert_with(|| Renderer::new(device, queue, FORMAT));
                match renderer.load_stage(device, queue, &pv.asset_dir(), preview::CODE) {
                    Ok(()) => g.bg_loaded = true,
                    Err(e) => g.space.note = Some((format!("loading the 3D view: {e}"), true)),
                }
            }
        }
    }

    /// Home's picture of the open level (`custom/stages/<code>/thumb.png`):
    /// the view from its first spawn, made when its BG has loaded and there is
    /// none newer than the BG (a fresh import empties the folder). Failing is
    /// no matter: the card shows no picture.
    fn make_thumb(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(level) = self.level.as_ref().filter(|l| l.bg_loaded) else { return };
        let Some(scene) = level.scene.as_ref() else { return };
        let dir = self.paths.stage_dir(&level.recipe.code);
        let (thumb, bg) = (dir.join(THUMB), dir.join("bg.json"));
        let mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        if matches!((mtime(&thumb), mtime(&bg)), (Some(t), Some(b)) if t >= b) {
            return;
        }
        let Some(renderer) = self.renderer.as_mut() else { return };
        let (w, h) = THUMB_SIZE;
        let target = RenderTarget::on_device(device, w, h, FORMAT, true);
        let (znear, zfar) = renderer.z_range();
        let view = scene.start_camera().view(w as f32 / h as f32, znear, zfar);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pd_edit thumb") });
        renderer.render_free(device, queue, &mut enc, &target, &scene.world, &view);
        queue.submit(Some(enc.finish()));
        let rgba = target.read_rgba8(device, queue);
        match image::RgbaImage::from_raw(w, h, rgba).map(|img| img.save(&thumb)) {
            Some(Ok(())) => {}
            Some(Err(e)) => log::warn!("{}: {e}", thumb.display()),
            None => log::warn!("{}: the picture's size", thumb.display()),
        }
    }

    /// Home's picture of an arena (None until it has been opened once).
    pub(super) fn thumb(&mut self, ctx: &egui::Context, code: &str) -> Option<egui::TextureHandle> {
        let path = self.paths.stage_dir(code).join(THUMB);
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        if let Some((t, tex)) = self.thumbs.get(code) {
            if *t == mtime {
                return Some(tex.clone());
            }
        }
        let img = image::open(&path).ok()?.to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let tex = ctx.load_texture(format!("thumb {code}"), egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR);
        self.thumbs.insert(code.to_owned(), (mtime, tex.clone()));
        Some(tex)
    }

    /// One pass of the window's panels (no window needed: `--shot-ui` runs it too).
    pub fn ui(&mut self, egui: &egui::Context) {
        if !self.themed {
            theme::apply(egui);
            self.themed = true;
        }
        self.keys_free = !egui.wants_keyboard_input() && egui.memory(|m| m.focused().is_none());
        // With the generator in hand the keys and the middle of the window are its own
        // (its 3D view's keys first: the generator's panel takes Escape for itself).
        match self.generating.as_mut() {
            Some(g) => g.space.keys(egui, self.res.as_ref()),
            None => self.shortcuts(egui),
        }
        let step = self.step();
        let bar = |fill| egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(12, 0)).stroke(egui::Stroke::new(1.0, theme::BORDER));
        egui::TopBottomPanel::top("top").frame(bar(theme::PANEL)).exact_height(54.0).show(egui, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("status").frame(bar(theme::PANEL)).exact_height(30.0).show(egui, |ui| self.status_bar(ui));
        match step {
            Step::Home => {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG)).show(egui, |ui| self.home(ui));
                self.hover_view = false;
                self.import_dialog(egui);
            }
            Step::Layout if self.generating.is_some() => {
                egui::SidePanel::left("generator").frame(theme::side_frame()).resizable(true).default_width(400.0).show(egui, |ui| self.generator_panel(ui));
                // The selected room's settings (Advanced has the generator's own).
                if !self.shape_advanced && self.generating.as_ref().is_some_and(|g| g.ed.doc.selected().is_some()) {
                    egui::SidePanel::right("room").frame(theme::side_frame()).resizable(true).default_width(300.0).show(egui, |ui| self.room_panel(ui));
                }
                if let Some(g) = self.generating.as_mut() {
                    let room = g.room();
                    egui::TopBottomPanel::top("generator view").frame(bar(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6))).show(egui, |ui| g.space.bar(ui, &g.ed.doc.recipe, room.as_deref(), self.res.as_ref()));
                }
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(egui, |ui| match self.generating.as_mut() {
                    Some(g) if g.space.middle == Middle::Plan => {
                        let rect = ui.max_rect();
                        g.ed.canvas(ui);
                        // The 3D view's camera on the plan; a right click sends it there.
                        let made = g.ed.doc.made().cloned();
                        g.space.over_plan(ui, rect, g.ed.fit, made.as_ref().map(|m| &m.level));
                        self.hover_view = false;
                    }
                    Some(g) => {
                        // The level in 3D is drawn under this (`render`); the right
                        // button looks, as in the ordinary view.
                        let resp = g.space.over_view(ui, &g.ed.doc.recipe);
                        // a click picks a room, the selected one's gizmo reshapes it
                        g.space.gizmo(ui, &resp, &mut g.ed);
                        self.hover_view = resp.hovered() || self.looking;
                        if resp.hovered() && ui.input(|i| i.pointer.secondary_pressed()) {
                            self.capture = Some(true);
                            self.looking = true;
                        }
                    }
                    None => {}
                });
            }
            _ => {
                egui::SidePanel::left("left").frame(theme::side_frame()).resizable(true).default_width(if step == Step::Test { 330.0 } else { 300.0 }).show(egui, |ui| match step {
                    Step::Layout => self.model_panel(ui),
                    Step::Test => self.checklist_panel(ui),
                    _ => self.items_left(ui),
                });
                egui::SidePanel::right("right").frame(theme::side_frame()).resizable(true).default_width(310.0).show(egui, |ui| match step {
                    Step::Test => self.test_panel(ui),
                    _ => self.inspector_right(ui),
                });
                if step == Step::Items {
                    egui::TopBottomPanel::top("view bar").frame(bar(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6))).show(egui, |ui| self.view_bar(ui));
                }
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(egui, |ui| self.viewport(ui));
            }
        }
        if self.help {
            self.help_window(egui);
        }
        self.title = match (&self.level, &self.generating) {
            (_, Some(g)) => format!("Arena Editor: {}", g.form.name),
            (Some(l), None) => format!("Arena Editor: {}", l.recipe.name),
            _ => "Arena Editor".into(),
        };
    }

    /// The generator, seed `seed` (for `--shot-ui --seed`).
    pub fn generate_seed(&mut self, seed: u64) {
        self.generate(Some((Form::for_seed(seed, &self.paths), shape::new_recipe(seed))));
    }

    /// `--shot-ui`'s step, selection and tool, once the level is open.
    /// Run the simulant test (`--shot-ui --run-test`).
    pub(crate) fn run_test(&mut self) {
        self.start(Work::Check);
    }

    /// Select a generated level's room on the plan (`--shot-ui --room`).
    pub(crate) fn select_generated_room(&mut self, key: &str) {
        if let Some(g) = self.generating.as_mut() {
            g.ed.select(Some(key));
        }
    }

    /// Open the variants strip of a generated level's room, for one part of
    /// its inside or ("inside") all of it (`--shot-ui --room KEY --vary PART`).
    pub(crate) fn vary_generated(&mut self, key: &str, part: &str) {
        if let Some(g) = self.generating.as_mut() {
            g.ed.vary(key, autolevel_edit::strip::part(part));
        }
    }

    /// Show a generated level in 3D, its ceilings left out if asked
    /// (`--shot-ui --3d [--no-ceilings]`).
    pub(crate) fn show_generated_3d(&mut self, no_ceilings: bool) {
        if let Some(g) = self.generating.as_mut() {
            g.space.middle = Middle::Space;
            g.space.cut.ceilings = no_ceilings;
        }
    }

    /// The 3D view's camera over the generated room selected, from outside it.
    pub(crate) fn overlook_generated_room(&mut self) {
        if let Some(g) = self.generating.as_mut() {
            if let Some(key) = g.ed.doc.selected().map(str::to_owned) {
                g.space.overlook(&key);
            }
        }
    }

    /// Show a generated level from the side, through its selected room (`--shot-ui --side`).
    pub(crate) fn show_generated_side(&mut self) {
        if let Some(g) = self.generating.as_mut() {
            g.ed.set_side(true);
        }
    }

    /// A thing put by place on a generated level (`--shot-ui --nudge`), the level made again.
    pub(crate) fn put_generated(&mut self, e: autolevel::recipe::Edit) {
        if let Some(g) = self.generating.as_mut() {
            g.ed.put(e);
        }
    }

    /// A room of a generated level locked, or copied to paste (`--shot-ui --lock ROOM --copy ROOM`).
    pub(crate) fn hold_generated(&mut self, lock: Option<&str>, copy: Option<&str>) {
        if let Some(g) = self.generating.as_mut() {
            if let Some(room) = lock {
                g.ed.set_locked(room, true);
            }
            if let Some(room) = copy {
                g.ed.copy_room(room);
            }
        }
    }

    /// "Put things" in hand, with what it puts (`--shot-ui --things TOOL`).
    pub(crate) fn things_generated(&mut self, tool: autolevel::puts::Tool) {
        if let Some(g) = self.generating.as_mut() {
            g.ed.set_tool(autolevel_edit::ui::Tool::Things);
            g.ed.thing = tool;
        }
    }

    pub(crate) fn shot_setup(&mut self, step: Option<Step>, sel: Option<Sel>, tool: Option<Kind>) {
        if let Some(s) = step {
            self.go(s);
        }
        if let Some(Sel::Item(it)) = sel {
            self.sel = Some(Sel::Item(it));
            self.tab = if it.kind == Kind::Door { Tab::Doors } else { Tab::Items };
        }
        if let Some(k) = tool {
            self.tool = if k == Kind::Door { Tool::AddDoor } else { Tool::Add(k) };
            self.tab = if k == Kind::Door { Tab::Doors } else { Tab::Items };
        }
    }

    /// Nothing is being made: no job, no edit waiting, the open level loaded,
    /// and the generator's level and its preview made.
    pub fn settled(&self) -> bool {
        let level = self.level.as_ref().is_none_or(|l| l.scene.is_some() && (l.bg_loaded || l.bg_failed) && l.pending.is_none());
        let gen = self.generating.as_ref().is_none_or(|g| g.ed.settled() && g.space.settled());
        self.job.is_none() && level && gen
    }
}
