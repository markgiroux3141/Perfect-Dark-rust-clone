//! `pd_edit [<code>]`: the level editor (see the `pd_edit` library). Opens
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
//! `pd_edit --shot <code> <out.png> [...]`: the view to a PNG (see `shot`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use engine::app::{AppConfig, Ctx, Game};
use engine::egui;
use engine::gpu::{Filter, Frame, Presenter, RenderTarget};
use engine::input::KeyCode;
use engine::wgpu;
use glam::Vec3;
use pd_edit::camera::{self, FlyCam, Motion};
use pd_edit::doc::{Doc, Item, Kind, Sel, PAD_HEIGHT};
use pd_edit::draw3d::Draw3d;
use pd_edit::gizmo::{self, Handle};
use pd_edit::jobs::{Job, Work};
use pd_edit::newlevel::NewLevel;
use pd_edit::overlay::{self, Highlight, Show};
use pd_edit::paths::EditPaths;
use pd_edit::scene::Scene;
use pd_import::doors::{Catalogue, Game as DoorGame};
use pd_import::layout::{Layout, Motion as DoorMotion, Side, Swing};
use pd_import::recipe::Recipe;
use pd_menu::generated::MP_WEAPON_SETS;
use pd_render::Renderer;
use pd_sim::world::WorldRes;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// An edit is placed this long after the last change.
const APPLY_AFTER: f32 = 0.35;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Items,
    Waypoints,
    Doors,
    Level,
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
    report: Vec<String>,
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

struct App {
    paths: EditPaths,
    res: Option<Arc<WorldRes>>,
    renderer: Option<Renderer>,
    presenter: Option<Presenter>,
    draw3d: Option<Draw3d>,
    target: Option<RenderTarget>,
    level: Option<Level>,
    job: Option<Job>,
    cam: FlyCam,
    looking: bool,
    hover_view: bool,
    tab: Tab,
    tool: Tool,
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
    fn new(paths: EditPaths, open_code: Option<String>) -> App {
        App {
            paths,
            res: None,
            renderer: None,
            presenter: None,
            draw3d: None,
            target: None,
            level: None,
            job: None,
            cam: FlyCam::new(Vec3::new(0.0, 500.0, 0.0), 0.0, 0.0),
            looking: false,
            hover_view: false,
            tab: Tab::Items,
            tool: Tool::Select,
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
        }
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
    fn open(&mut self, path: PathBuf, ctx: &mut Ctx) {
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
        self.level = Some(Level { recipe, layout_path, doc: Doc::new(layout, !adopt), adopt, scene: None, bg_loaded: false, report: Vec::new(), pending: None });
        self.tool = Tool::Select;
        self.sel = None;
        self.link_from = None;
        self.new_level = None;
        if self.paths.is_imported(&code) {
            let Some(res) = self.res.clone() else { return };
            match Scene::load(&self.paths.asset_dir(), &code, res, self.weapons()) {
                Ok(scene) => {
                    self.scene_loaded(scene, true, ctx);
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
        self.say(format!("opened {code}"), false);
    }

    /// A stage (re)loaded: swap it in; `new_bg` loads its BG and puts the
    /// camera at its start.
    fn scene_loaded(&mut self, scene: Scene, new_bg: bool, ctx: &mut Ctx) {
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
            if let Some(r) = self.renderer.as_mut() {
                match r.load_stage(&ctx.gpu.device, &ctx.gpu.queue, &self.paths.asset_dir(), &level.recipe.code) {
                    Ok(()) => level.bg_loaded = true,
                    Err(e) => {
                        let msg = format!("loading the stage's BG: {e}");
                        self.say(msg, true);
                        return;
                    }
                }
            }
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
        let weapons = MP_WEAPON_SETS.get(self.weapon_set).map_or([0; 6], |s| s.slots.map(|x| x as u8));
        self.job = Some(Job::spawn(work, level.recipe.clone(), level.doc.layout.clone(), self.paths.clone(), res, weapons));
    }

    fn poll_job(&mut self, ctx: &mut Ctx) {
        let Some(done) = self.job.as_ref().and_then(|j| j.poll()) else { return };
        let Some(job) = self.job.take() else { return };
        let secs = job.started.elapsed().as_secs_f32();
        match done.result {
            Ok((report, scene)) => {
                if let Some(level) = self.level.as_mut() {
                    if done.work == Work::Check {
                        level.report.extend(report);
                    } else {
                        level.report = report;
                    }
                }
                if let Some(scene) = scene {
                    self.scene_loaded(scene, done.work == Work::Import, ctx);
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
            Err(e) => self.say(format!("{}: {e}", done.work.label()), true),
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

    fn play(&mut self) {
        let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(if cfg!(windows) { "perfect_dark.exe" } else { "perfect_dark" })));
        match exe.filter(|e| e.exists()) {
            Some(e) => match std::process::Command::new(&e).arg("--combat").spawn() {
                Ok(_) => self.say("the game started: Combat Simulator, Arena, Custom", false),
                Err(err) => self.say(format!("{}: {err}", e.display()), true),
            },
            None => self.say("no perfect_dark executable beside pd_edit (cargo build --release)", true),
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

    fn panel(&mut self, ui: &mut egui::Ui, ctx: &mut Ctx) {
        ui.horizontal(|ui| {
            ui.heading("pd_edit");
            if ui.button("Open…").on_hover_text("A level's recipe (crates/pd_import/levels/*.json)").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("recipe", &["json"]).set_directory(&self.paths.levels).pick_file() {
                    self.open(p, ctx);
                }
            }
            if ui.button("New from glTF…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("glTF", &["glb", "gltf"]).pick_file() {
                    self.new_level = Some(NewLevel::for_file(&p, &self.paths));
                }
            }
        });
        if let Some(job) = &self.job {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{}… {:.0} s", job.work.label(), job.started.elapsed().as_secs_f32()));
            });
        }
        if let Some((m, err)) = &self.message {
            ui.colored_label(if *err { egui::Color32::from_rgb(255, 120, 90) } else { egui::Color32::from_rgb(150, 220, 150) }, m);
        }
        ui.separator();
        if self.new_level.is_some() {
            self.new_level_form(ui, ctx);
            return;
        }
        if self.level.is_none() {
            self.start_list(ui, ctx);
            return;
        }
        let dirty = self.level.as_ref().is_some_and(|l| l.doc.dirty());
        let name = self.level.as_ref().map(|l| format!("{} ({}){}", l.recipe.name, l.recipe.code, if dirty { " *" } else { "" })).unwrap_or_default();
        ui.label(egui::RichText::new(name).strong());
        ui.horizontal(|ui| {
            if ui.add_enabled(dirty, egui::Button::new("Save")).on_hover_text("Ctrl+S: the layout file").clicked() {
                self.save();
            }
            if ui.button("Undo").on_hover_text("Ctrl+Z").clicked() {
                self.undo(false);
            }
            if ui.button("Redo").on_hover_text("Ctrl+Y").clicked() {
                self.undo(true);
            }
            if ui.button("Play").on_hover_text("Start the game (the level is under Arena, Custom)").clicked() {
                self.play();
            }
        });
        ui.horizontal(|ui| {
            for (t, name) in [(Tab::Items, "Items"), (Tab::Waypoints, "Waypoints"), (Tab::Doors, "Doors"), (Tab::Level, "Level")] {
                if ui.selectable_label(self.tab == t, name).clicked() && self.tab != t {
                    self.tab = t;
                    self.tool = Tool::Select;
                    self.link_from = None;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
            Tab::Items => self.items_tab(ui),
            Tab::Waypoints => self.waypoints_tab(ui),
            Tab::Doors => self.doors_tab(ui),
            Tab::Level => self.level_tab(ui),
        });
    }

    fn start_list(&mut self, ui: &mut egui::Ui, ctx: &mut Ctx) {
        ui.label("Open a level:");
        let mut open = None;
        for e in self.paths.recipes() {
            let imported = self.paths.is_imported(&e.code);
            let text = format!("{} ({}){}", e.name, e.code, if imported { "" } else { ": not imported yet" });
            if ui.button(text).clicked() {
                open = Some(e.path);
            }
        }
        if let Some(p) = open {
            self.open(p, ctx);
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Or make one from a glTF file (New from glTF…). Modelling is done elsewhere; here you place items, waypoints and doors.").weak());
    }

    fn new_level_form(&mut self, ui: &mut egui::Ui, ctx: &mut Ctx) {
        let Some(n) = self.new_level.as_mut() else { return };
        ui.label(egui::RichText::new("New level").strong());
        ui.label(egui::RichText::new(n.file.display().to_string()).weak());
        egui::Grid::new("newlevel").num_columns(2).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut n.name);
            ui.end_row();
            ui.label("Code");
            ui.text_edit_singleline(&mut n.code);
            ui.end_row();
            ui.label("Stage number");
            ui.add(egui::DragValue::new(&mut n.stagenum).range(0x60..=0x7f).hexadecimal(2, false, true));
            ui.end_row();
            ui.label("Scale").on_hover_text("File units to cm: glTF's are metres (100)");
            ui.add(egui::DragValue::new(&mut n.scale).range(0.01..=10000.0).speed(1.0));
            ui.end_row();
            ui.label("Rooms").on_hover_text("0: one per ~250 m² of floor");
            ui.add(egui::DragValue::new(&mut n.rooms).range(0..=150));
            ui.end_row();
            ui.label("Spawns");
            ui.add(egui::DragValue::new(&mut n.spawns).range(1..=32));
            ui.end_row();
            ui.label("Weapons");
            ui.add(egui::DragValue::new(&mut n.weapons).range(0..=16));
            ui.end_row();
            ui.label("Hills");
            ui.add(egui::DragValue::new(&mut n.hills).range(0..=12));
            ui.end_row();
            ui.label("Largest texture");
            ui.add(egui::DragValue::new(&mut n.max_texture).range(8..=4096));
            ui.end_row();
        });
        ui.checkbox(&mut n.climb_ledges, "walls up to 1.6 m between floors are climbable (PD's arenas have none)");
        ui.label(egui::RichText::new("The first placement is generated from these counts; you move things from there.").weak());
        let problem = n.problem(&self.paths);
        if let Some(p) = &problem {
            ui.colored_label(egui::Color32::from_rgb(255, 170, 90), p);
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(problem.is_none() && self.job.is_none(), egui::Button::new("Create and import")).clicked() {
                let n = self.new_level.clone().unwrap();
                match n.write(&self.paths) {
                    Ok(path) => self.open(path, ctx),
                    Err(e) => self.say(e, true),
                }
            }
            if ui.button("Cancel").clicked() {
                self.new_level = None;
            }
        });
    }

    fn gizmo_buttons(&mut self, ui: &mut egui::Ui, rotate: bool) {
        ui.horizontal(|ui| {
            ui.label("Gizmo");
            ui.selectable_value(&mut self.gizmo_mode, gizmo::Mode::Move, "Move").on_hover_text("T switches");
            if rotate {
                ui.selectable_value(&mut self.gizmo_mode, gizmo::Mode::Rotate, "Rotate").on_hover_text("T switches (for what has a facing)");
            }
            ui.label(egui::RichText::new("Ctrl snaps").weak());
            if ui.add_enabled(self.sel.is_some(), egui::Button::new("Drop to floor")).on_hover_text("G: the selection onto the floor under it").clicked() {
                self.ground_sel();
            }
        });
    }

    fn items_tab(&mut self, ui: &mut egui::Ui) {
        self.gizmo_buttons(ui, true);
        ui.label("Tool");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "Select");
            for k in Kind::ITEMS {
                ui.selectable_value(&mut self.tool, Tool::Add(k), format!("+ {}", k.name())).on_hover_text("Click the floor to place one");
            }
        });
        if matches!(self.tool, Tool::Add(Kind::Base | Kind::Respawn)) {
            ui.horizontal(|ui| {
                ui.label("Team");
                for t in 0..4u8 {
                    ui.selectable_value(&mut self.team, t, egui::RichText::new(format!("{t}")).color(rgba(overlay::TEAMS[t as usize])));
                }
            });
        }
        if self.tool == Tool::Add(Kind::Ammo) {
            ui.label(egui::RichText::new("A crate holds its weapon's ammo: it goes to the selected weapon, else the nearest.").weak());
        }
        ui.separator();
        let res = self.res.clone();
        let set_name = |i: usize| res.as_ref().map_or(format!("set {i}"), |r| r.lang.get(MP_WEAPON_SETS[i].name).to_owned());
        let before = self.weapon_set;
        egui::ComboBox::from_label("preview weapon set").selected_text(set_name(self.weapon_set)).show_ui(ui, |ui| {
            for i in 0..MP_WEAPON_SETS.len() {
                ui.selectable_value(&mut self.weapon_set, i, set_name(i));
            }
        });
        if self.weapon_set != before {
            let w = self.weapons();
            if let Some(scene) = self.level.as_mut().and_then(|l| l.scene.as_mut()) {
                if let Err(e) = scene.set_weapons(w) {
                    log::warn!("{e}");
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.show.items, "items");
            ui.checkbox(&mut self.show.cover, "cover");
            ui.checkbox(&mut self.show.waypoints, "waypoints");
            ui.checkbox(&mut self.show.through_walls, "faint through walls");
            ui.checkbox(&mut self.show.labels, "labels");
        });
        ui.separator();
        self.inspector(ui);
        ui.separator();
        self.item_lists(ui);
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
            if ui.button("Delete").on_hover_text("Delete key").clicked() {
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
        let Some(Sel::Item(sel)) = self.sel else {
            ui.label(egui::RichText::new("Nothing selected: click an item in the view.").weak());
            return;
        };
        if sel.kind == Kind::Door {
            return self.door_inspector(ui, sel.index);
        }
        let Some(level) = self.level.as_ref() else { return };
        let doc = &level.doc;
        let (pos, facing, team, loc, cw) = (doc.pos(sel), doc.facing(sel), doc.team(sel), doc.location(sel), doc.crate_weapon(sel));
        let nweapons = doc.count(Kind::Weapon);
        let gun = loc.map(|l| self.location_gun(l));
        ui.label(egui::RichText::new(format!("{} {}", sel.kind.name(), sel.index)).strong());
        let mut changed = false;
        let level = self.level.as_mut().unwrap();
        let mut p = pos;
        ui.horizontal(|ui| {
            for (k, name) in [(0, "x"), (1, "y"), (2, "z")] {
                ui.label(name);
                let r = ui.add(egui::DragValue::new(&mut p[k]).speed(5.0).max_decimals(1));
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
            if ui.add(egui::Slider::new(&mut f, 0.0..=359.9).text("facing (°)").step_by(0.1)).changed() {
                level.doc.set_facing(sel, f);
                changed = true;
            }
        }
        if let Some(mut t) = team {
            ui.horizontal(|ui| {
                ui.label("team");
                for k in 0..4u8 {
                    if ui.selectable_value(&mut t, k, egui::RichText::new(format!("{k}")).color(rgba(overlay::TEAMS[k as usize]))).changed() {
                        level.doc.set_team(sel, t);
                        changed = true;
                    }
                }
            });
        }
        if let Some(mut l) = loc {
            if ui.add(egui::Slider::new(&mut l, 0..=15).text("MP location")).changed() {
                level.doc.set_location(sel, l);
                changed = true;
            }
            ui.label(format!("holds: {} (under the preview set)", gun.unwrap_or_default()));
        }
        if let Some(mut w) = cw {
            let before = w;
            egui::ComboBox::from_label("belongs to").selected_text(format!("weapon {w}")).show_ui(ui, |ui| {
                for i in 0..nweapons {
                    ui.selectable_value(&mut w, i, format!("weapon {i}"));
                }
            });
            if w != before {
                level.doc.set_crate_weapon(sel, w);
                changed = true;
            }
        }
        if changed {
            self.edited();
        }
        self.sel_buttons(ui, true);
    }

    fn item_lists(&mut self, ui: &mut egui::Ui) {
        let Some(level) = self.level.as_ref() else { return };
        let mut pick = None;
        for kind in Kind::ITEMS {
            let n = level.doc.count(kind);
            egui::CollapsingHeader::new(format!("{}s: {n}", kind.name())).id_salt(kind.name()).show(ui, |ui| {
                for index in 0..n {
                    let it = Item { kind, index };
                    let p = level.doc.pos(it);
                    let extra = match kind {
                        Kind::Weapon => format!(" loc {}", level.doc.location(it).unwrap_or(0)),
                        Kind::Ammo => format!(" for W{}", level.doc.crate_weapon(it).unwrap_or(0)),
                        Kind::Base | Kind::Respawn => format!(" team {}", level.doc.team(it).unwrap_or(0)),
                        _ => String::new(),
                    };
                    let text = format!("{index}{extra}  ({:.0}, {:.0}, {:.0})", p.x, p.y, p.z);
                    if ui.selectable_label(self.sel == Some(Sel::Item(it)), text).clicked() {
                        pick = Some((it, p));
                    }
                }
            });
        }
        if let Some((it, p)) = pick {
            self.sel = Some(Sel::Item(it));
            self.frame_on(p);
        }
    }

    fn waypoints_tab(&mut self, ui: &mut egui::Ui) {
        self.gizmo_buttons(ui, false);
        ui.label("Tool");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "Select");
            ui.selectable_value(&mut self.tool, Tool::AddNode, "+ Waypoint").on_hover_text("Click the floor to place one");
            if ui.selectable_value(&mut self.tool, Tool::Link, "Link").on_hover_text("Click one waypoint, then another: links them (Shift: one way), or cuts their link").changed() {
                self.link_from = None;
            }
        });
        if self.tool == Tool::Link {
            ui.label(egui::RichText::new("Click a waypoint, then another: a link is made both ways (hold Shift for one way, first → second), or cut if they're linked. The second becomes the next first. Esc stops.").weak());
        }
        if let Some(scene) = self.scene() {
            let g = &scene.graph;
            ui.label(format!("{} waypoints, {} links; {} outside the main part (red: simulants can't reach them and get back)", g.pos.len(), g.links.len(), g.stranded()));
        }
        let edits = self.level.as_ref().map_or(0, |l| l.doc.wp_edit_count());
        ui.label(format!("{edits} hand edits, replayed on the generated graph each time the level is placed"));
        ui.horizontal(|ui| {
            if ui.add_enabled(self.job.is_none(), egui::Button::new("Generate again")).on_hover_text("From the collision, then your edits on top").clicked() {
                self.start(Work::Place { fresh: true });
            }
            if ui.add_enabled(edits > 0, egui::Button::new("Discard my edits")).clicked() {
                if let Some(l) = self.level.as_mut() {
                    l.doc.wp_clear();
                }
                self.sel = None;
                self.edited();
            }
        });
        ui.separator();
        let Some(Sel::Node(i)) = self.sel else {
            ui.label(egui::RichText::new("Nothing selected: click a waypoint in the view.").weak());
            return;
        };
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

    fn doors_tab(&mut self, ui: &mut egui::Ui) {
        self.gizmo_buttons(ui, true);
        ui.horizontal(|ui| {
            ui.label("Tool");
            ui.selectable_value(&mut self.tool, Tool::Select, "Select");
            ui.add_enabled_ui(self.door_pick.is_some(), |ui| {
                ui.selectable_value(&mut self.tool, Tool::AddDoor, "+ Door").on_hover_text("Click a doorway's floor to put the picked door in it");
            });
        });
        ui.checkbox(&mut self.door_fit, "fit a new door to its doorway (the walls either side, the lintel)");
        ui.separator();
        self.door_catalogue(ui);
        ui.separator();
        match self.sel {
            Some(Sel::Item(it)) if it.kind == Kind::Door => self.door_inspector(ui, it.index),
            _ => {
                ui.label(egui::RichText::new("Nothing selected: click a door in the view (or pick a model above and use + Door).").weak());
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Open every door").on_hover_text("A preview: they close again once the level is placed").clicked() {
                self.preview_doors(true);
            }
            if ui.button("Close every door").clicked() {
                self.preview_doors(false);
            }
        });
        let Some(level) = self.level.as_ref() else { return };
        let n = level.doc.count(Kind::Door);
        let mut pick = None;
        egui::CollapsingHeader::new(format!("Doors: {n}")).id_salt("doorlist").show(ui, |ui| {
            for index in 0..n {
                let it = Item { kind: Kind::Door, index };
                let d = level.doc.door(index);
                let pair = d.sibling.map_or(String::new(), |s| format!(", with {s}"));
                let text = format!("{index}  {} {}{pair}  ({:.0}, {:.0}, {:.0})", d.model, d.motion.name(), d.pos[0], d.pos[1], d.pos[2]);
                if ui.selectable_label(self.sel == Some(Sel::Item(it)), text).clicked() {
                    pick = Some((it, level.doc.pos(it)));
                }
            }
        });
        if let Some((it, p)) = pick {
            self.sel = Some(Sel::Item(it));
            self.frame_on(p + Vec3::Y * 100.0);
        }
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
    fn door_inspector(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(level) = self.level.as_ref() else { return };
        if i >= level.doc.count(Kind::Door) {
            return;
        }
        let d = level.doc.door(i).clone();
        let ring = level.doc.ring(i);
        let it = Item { kind: Kind::Door, index: i };
        let cat = self.catalogue.get(&d.model).cloned();
        ui.label(egui::RichText::new(format!("Door {i}: {}", cat.as_ref().map_or(d.model.clone(), |c| c.title()))).strong());
        match &cat {
            Some(c) => ui.label(egui::RichText::new(format!("{} ({}), seen on {}", c.stem, if c.game == DoorGame::Ge { "GoldenEye" } else { "Perfect Dark" }, c.seen.join(", "))).weak()),
            None => ui.colored_label(egui::Color32::from_rgb(255, 120, 90), format!("{}: not in the catalogues (GoldenEye's need extracting)", d.model)),
        };
        if ring.len() > 1 {
            ui.label(format!("opens with door{} {}", if ring.len() > 2 { "s" } else { "" }, ring[1..].iter().map(|j| j.to_string()).collect::<Vec<_>>().join(", ")));
        }
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
        // Where it stands (the ring moves and turns with it).
        let mut p = Vec3::from(d.pos);
        let mut facing = d.facing;
        ui.horizontal(|ui| {
            for (k, name) in [(0, "x"), (1, "y"), (2, "z")] {
                ui.label(name);
                let r = ui.add(egui::DragValue::new(&mut p[k]).speed(5.0).max_decimals(1));
                drag(&r, &mut live, &mut began);
            }
        });
        let r = ui.add(egui::Slider::new(&mut facing, 0.0..=359.9).text("front faces (°)").step_by(0.1));
        drag(&r, &mut live, &mut began);
        ui.horizontal(|ui| {
            for (k, name) in [(0, "width"), (1, "height"), (2, "thickness")] {
                ui.label(name);
                let r = ui.add(egui::DragValue::new(&mut e.size[k]).speed(1.0).range(1.0..=5000.0).max_decimals(1));
                drag(&r, &mut live, &mut began);
            }
        });
        // How it opens.
        let before = e.motion;
        egui::ComboBox::from_label("moves").selected_text(e.motion.name()).show_ui(ui, |ui| {
            for m in DoorMotion::ALL {
                ui.selectable_value(&mut e.motion, m, m.name());
            }
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
            ui.label(egui::RichText::new("(only Caverns' eyelid and iris models move so: this one stands still)").weak());
        }
        let side_word = match e.motion {
            DoorMotion::Swing | DoorMotion::Hull | DoorMotion::Chair => "hinged on the",
            DoorMotion::Up | DoorMotion::Down | DoorMotion::Eyelid | DoorMotion::Iris => "turned so its hinge side is",
            _ => "slides to the",
        };
        ui.horizontal(|ui| {
            ui.label(side_word);
            step |= ui.selectable_value(&mut e.side, Side::Left, "left").changed();
            step |= ui.selectable_value(&mut e.side, Side::Right, "right").changed();
            ui.label(egui::RichText::new("(seen from the front)").weak());
        });
        if matches!(e.motion, DoorMotion::Swing | DoorMotion::Hull | DoorMotion::Chair) {
            ui.horizontal(|ui| {
                ui.label("opens");
                step |= ui.selectable_value(&mut e.swing, Swing::Back, "away from the front").changed();
                step |= ui.selectable_value(&mut e.swing, Swing::Front, "towards it").changed();
                step |= ui.selectable_value(&mut e.swing, Swing::Both, "either way").on_hover_text("Away from whoever opens it (two-way)").changed();
            });
        }
        if e.motion.turns() {
            let mut deg = e.maxfrac as f32 / 65536.0;
            let r = ui.add(egui::Slider::new(&mut deg, 1.0..=180.0).text("opens by (°)"));
            drag(&r, &mut live, &mut began);
            e.maxfrac = (deg * 65536.0).round() as i32;
        } else {
            let mut pct = e.maxfrac as f32 / 655.36;
            let r = ui.add(egui::Slider::new(&mut pct, 5.0..=100.0).text("opens by (% of its size)"));
            drag(&r, &mut live, &mut began);
            e.maxfrac = (pct * 655.36).round() as i32;
        }
        let secs = pd_sim::props::door::door_open_frames(e.maxfrac, e.accel, e.decel, e.maxspeed).map(|f| f as f32 / 60.0);
        ui.horizontal(|ui| {
            ui.label(match secs {
                Some(t) => format!("opens in {t:.1} s"),
                None => "never opens all the way".into(),
            });
            for (name, k) in [("slower", 0.8f32), ("faster", 1.25)] {
                if ui.button(name).clicked() {
                    // Speed and acceleration together: the same motion, quicker.
                    e.maxspeed = ((e.maxspeed as f32) * k).round().max(1.0) as i32;
                    e.accel = ((e.accel as f32) * k * k).round().max(1.0) as i32;
                    e.decel = ((e.decel as f32) * k * k).round().max(1.0) as i32;
                    step = true;
                }
            }
        });
        let mut closes = e.autoclosetime < NEVER;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut closes, "closes itself after").changed() {
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
        egui::ComboBox::from_label("sound").selected_text(if e.soundtype == 0 { "silent".to_owned() } else { format!("sound {}", e.soundtype) }).show_ui(ui, |ui| {
            for &k in &DOOR_SOUNDS {
                step |= ui.selectable_value(&mut e.soundtype, k, if k == 0 { "silent".to_owned() } else { format!("sound {k}") }).changed();
            }
        });
        let mut flag = |ui: &mut egui::Ui, bits: &mut u16, bit: u16, text: &str, tip: &str| {
            let mut on = *bits & bit != 0;
            if ui.checkbox(&mut on, text).on_hover_text(tip).changed() {
                *bits ^= bit;
                step = true;
            }
        };
        ui.horizontal_wrapped(|ui| {
            flag(ui, &mut e.doorflags, 0x0010, "automatic", "Opens as someone walks at it (DOORFLAG_AUTOMATIC)");
            flag(ui, &mut e.doorflags, 0x0004, "retracts into its frame", "Its display list is cut where it has slid into the wall (DOORFLAG_0004)");
            flag(ui, &mut e.doorflags, 0x0200, "long range", "Can be used from 4 m, not 2 (DOORFLAG_LONGRANGE)");
            flag(ui, &mut e.doorflags, 0x0008, "mirrored", "The model mirrored through the door's plane (DOORFLAG_FLIP)");
            flag(ui, &mut e.doorflags, 0x0002, "windowed", "Its glass keeps its doorway's portal open while seen through (DOORFLAG_WINDOWED; a windowed model's)");
        });
        ui.horizontal_wrapped(|ui| {
            let mut keep = e.flags & pd_core::ids::OBJFLAG_DOOR_KEEPOPEN != 0;
            if ui.checkbox(&mut keep, "starts open").on_hover_text("OBJFLAG_DOOR_KEEPOPEN").changed() {
                e.flags ^= pd_core::ids::OBJFLAG_DOOR_KEEPOPEN;
                step = true;
            }
            let mut ai = e.flags2 & pd_core::ids::OBJFLAG2_AICANNOTUSE != 0;
            if ui.checkbox(&mut ai, "simulants can't open it").on_hover_text("OBJFLAG2_AICANNOTUSE").changed() {
                e.flags2 ^= pd_core::ids::OBJFLAG2_AICANNOTUSE;
                step = true;
            }
        });
        // Buttons.
        let mut act = None;
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(ring.len() == 1, egui::Button::new("Fit to doorway")).on_hover_text("Its width and height to the opening it stands in (a single door)").clicked() {
                act = Some("fit");
            }
            if ring.len() == 1 && ui.button("Make double").on_hover_text("Two leaves of half its size opening together (top and bottom for one that rises or sinks)").clicked() {
                act = Some("double");
            }
            if ring.len() > 1 && ui.button("Separate").on_hover_text("Each leaf opens alone").clicked() {
                act = Some("separate");
            }
            let pick = self.door_pick.clone();
            if let Some((stem, _)) = pick.as_ref().filter(|(s, _)| *s != d.model) {
                if ui.button("Use the picked model").on_hover_text(stem.as_str()).clicked() {
                    act = Some("model");
                }
            }
            if ui.button("Template's settings").on_hover_text("How the model is placed in its game (the picked template, else its most used)").clicked() {
                act = Some("template");
            }
            if ui.button("Preview open").clicked() {
                act = Some("open");
            }
            if ui.button("Close").clicked() {
                act = Some("close");
            }
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
                    self.say(format!("door {i} is now a double door with {j}"), false);
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

    fn level_tab(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.far, "see far (the stage's own far plane off)");
        let busy = self.job.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Import again")).on_hover_text("The source converted anew (after a change to the glTF)").clicked() {
                self.start(Work::Import);
            }
            if ui.add_enabled(!busy, egui::Button::new("Check match")).on_hover_text("A minute of six simulants on the level").clicked() {
                self.start(Work::Check);
            }
        });
        let Some(level) = self.level.as_ref() else { return };
        ui.label(format!("stage {:#x}, recipe {}.json, layout {}", level.recipe.stagenum, level.recipe.code, level.layout_path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
        ui.separator();
        ui.label("The last report:");
        for l in &level.report {
            ui.label(egui::RichText::new(l).monospace().size(11.0));
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

    fn viewport(&mut self, ui: &mut egui::Ui, ctx: &mut Ctx) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        self.hover_view = resp.hovered() || self.looking;
        if resp.hovered() && ui.input(|i| i.pointer.secondary_pressed()) {
            ctx.set_cursor_captured(true);
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
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, if self.level.is_some() { "Loading the stage…" } else { "Open a level (left)" }, egui::FontId::proportional(18.0), egui::Color32::GRAY);
            self.built = overlay::Built::default();
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
        // What the tool does, and where the camera is.
        let hint = match self.tool {
            Tool::Select => "Click to select; drag the gizmo's arrows (T: its ring). Right button: look, WASD: fly",
            Tool::Add(_) => "Click the floor to place one (Esc: back to Select)",
            Tool::AddNode => "Click the floor to add a waypoint (Esc: back to Select)",
            Tool::Link => "Click two waypoints to link them (Shift: one way) or cut their link",
            Tool::AddDoor => "Click a doorway's floor to put the picked door in it (Esc: back to Select)",
        };
        painter.text(rect.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(13.0), egui::Color32::from_gray(230));
        let c = self.cam;
        painter.text(
            rect.left_bottom() + egui::vec2(8.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            format!("({:.0}, {:.0}, {:.0}) yaw {:.0}° pitch {:.0}° · speed {:.0} cm/s", c.pos.x, c.pos.y, c.pos.z, c.yaw.to_degrees().rem_euclid(360.0), c.pitch.to_degrees(), c.speed),
            egui::FontId::monospace(11.0),
            egui::Color32::from_gray(220),
        );
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
        self.renderer = Some(Renderer::new(&ctx.gpu.device, &ctx.gpu.queue, FORMAT));
        self.presenter = Some(Presenter::new(ctx.gpu));
        self.draw3d = Some(Draw3d::new(&ctx.gpu.device, FORMAT));
        match WorldRes::load(&self.paths.asset_dir()) {
            Ok(r) => self.res = Some(Arc::new(r)),
            Err(e) => self.say(format!("loading the game's resources: {e}"), true),
        }
        self.catalogue = Catalogue::load(&self.paths.asset_dir());
        if let Some(code) = self.open_code.take() {
            let p = self.paths.levels.join(format!("{code}.json"));
            self.open(p, ctx);
        }
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
        // Shift+D duplicates; it doesn't strafe.
        let m = if ctx.input.key_down(KeyCode::ShiftLeft) && ctx.input.key_down(KeyCode::KeyD) && !self.looking { Motion { right: 0.0, ..m } } else { m };
        let dt = ctx.clock.tick_dt() as f32;
        self.cam.update(&m, dt);
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
        self.poll_job(ctx);
        let due = self.level.as_ref().and_then(|l| l.pending).is_some_and(|t| t.elapsed().as_secs_f32() >= APPLY_AFTER) && self.dragging.is_none();
        if due && self.job.is_none() {
            self.start(Work::Place { fresh: false });
        }
    }

    fn debug_ui(&mut self, ctx: &mut Ctx, egui: &egui::Context) {
        // The right button as egui sees it: the engine's input never gets a
        // press egui wants (the view is an egui area that takes clicks), but
        // egui has every press and release, captured cursor or not.
        if self.looking && !egui.input(|i| i.pointer.secondary_down()) {
            self.looking = false;
            ctx.set_cursor_captured(false);
        }
        self.shortcuts(egui);
        egui::SidePanel::left("pd_edit").resizable(true).default_width(370.0).show(egui, |ui| self.panel(ui, ctx));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(egui, |ui| self.viewport(ui, ctx));
        let title = match &self.level {
            Some(l) => format!("pd_edit: {}{}", l.recipe.name, if l.doc.dirty() { " *" } else { "" }),
            None => "pd_edit".into(),
        };
        ctx.window.set_title(&title);
    }

    fn render(&mut self, ctx: &mut Ctx, frame: &mut Frame) {
        let [_, _, vw, vh] = frame.viewport;
        let (w, h) = ((vw.round() as u32).max(1), (vh.round() as u32).max(1));
        let scene = self.level.as_ref().filter(|l| l.bg_loaded).and_then(|l| l.scene.as_ref());
        let (Some(renderer), Some(presenter), Some(draw3d), Some(scene)) = (self.renderer.as_mut(), self.presenter.as_ref(), self.draw3d.as_ref(), scene) else {
            frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pd_edit clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.08, g: 0.08, b: 0.1, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            return;
        };
        if self.target.as_ref().is_none_or(|t| t.width != w || t.height != h) {
            self.target = Some(RenderTarget::new(ctx.gpu, w, h, FORMAT, true));
        }
        let target = self.target.as_ref().unwrap();
        let (znear, zfar) = renderer.z_range();
        let view = self.cam.view(w as f32 / h as f32, znear, if self.far { zfar.max(60000.0) } else { zfar });
        renderer.render_free(&ctx.gpu.device, &ctx.gpu.queue, &mut frame.encoder, target, &scene.world, &view);
        draw3d.draw(&ctx.gpu.device, &mut frame.encoder, target, camera::world_to_clip(&view), &self.built.mesh, self.show.through_walls);
        presenter.present(ctx.gpu, frame, target, (w, h), Filter::Linear);
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,pd_edit=info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--shot") {
        match pd_edit::shot::run(&args[1..]) {
            Ok(p) => println!("{}", p.display()),
            Err(e) => {
                eprintln!("pd_edit: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let run = || -> Result<(), String> {
        let paths = EditPaths::discover()?;
        let app = App::new(paths, args.first().cloned());
        engine::app::run(AppConfig { title: "pd_edit".into(), size: (1600, 1000), tick_hz: 240.0, max_substeps: 8, ..AppConfig::default() }, app)
    };
    if let Err(e) = run() {
        eprintln!("pd_edit: {e}");
        std::process::exit(1);
    }
}
