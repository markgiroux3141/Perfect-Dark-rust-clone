//! The editor's screens: the top bar (the level, its three steps, Play now),
//! Home (make a level, or pick one), the import dialog, the Test step, and
//! the status bar. Each level is made in the same three steps: Layout (its
//! rooms: the generator's, or the model it was imported from), Items
//! (spawns, weapons, doors...), and Test & play.

use engine::egui::{self, Align, Color32, CornerRadius, Layout, Margin, RichText, Stroke};

use super::{theme, App, Step, Tab, Tool};
use crate::doc::{Item, Kind, Sel};
use crate::generator;
use crate::jobs::Work;
use crate::newlevel::NewLevel;
use crate::ready::{self, Fix};
use pd_import::recipe::{Recipe, Source};
use theme::Tone;

/// Where a level came from, in a few words.
fn source_words(recipe: &Recipe) -> &'static str {
    match recipe.source {
        Source::Autolevel(_) => "Generated",
        Source::Gltf(_) => "Imported model",
        Source::Ge(_) => "From GoldenEye",
        Source::Jka(_) => "From Jedi Academy",
        Source::Oot(_) => "From Ocarina of Time",
        Source::Pd(_) => "From Perfect Dark",
    }
}

/// A dialog about arena sets on Home.
pub enum SetDialog {
    /// A new set, its name being typed.
    New(String),
    /// The active set renamed.
    Rename(String),
    /// Delete a set (its stem, its name).
    DeleteSet(String, String),
    /// Delete an arena for good (its code, its name).
    DeleteArena(String, String),
}

/// What a card was asked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CardAct {
    Open,
    AddToSet,
    RemoveFromSet,
    Delete,
}

/// A level on Home.
struct Card {
    code: String,
    /// In the set in use.
    in_set: bool,
    name: String,
    /// Its picture, once it has been opened (`App::thumb`).
    thumb: Option<egui::TextureHandle>,
    source: &'static str,
    generated: bool,
    imported: bool,
    path: std::path::PathBuf,
}

impl App {
    /// The step on show: Home with nothing open, Layout while the generator
    /// is in hand, else the open level's.
    pub(super) fn step(&self) -> Step {
        if self.generating.is_some() {
            Step::Layout
        } else if self.level.is_none() {
            Step::Home
        } else {
            self.step
        }
    }

    /// Go to a step of the open level (or of the level being generated).
    pub(super) fn go(&mut self, to: Step) {
        let now = self.step();
        if to == now {
            return;
        }
        if now == Step::Layout && self.generating.is_some() {
            // Leaving the generator: the open level's steps (its rooms as they
            // were), or, for a level not made yet, nowhere.
            if self.level.is_none() {
                return self.say("build the arena first (Build the arena, bottom left)", true);
            }
            self.generating = None;
        }
        if to == Step::Layout {
            if let Some(made) = self.level.as_ref().and_then(|l| generator::of(&l.recipe)) {
                self.generate(Some(made));
            }
        }
        if to == Step::Home {
            self.level = None;
            self.generating = None;
            self.sel = None;
        }
        self.step = if to == Step::Home { Step::Items } else { to };
        self.tool = Tool::Select;
        self.link_from = None;
        if to == Step::Test {
            self.tab = Tab::Items;
        }
    }

    // ── The top bar ────────────────────────────────────────────────────────

    pub(super) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let step = self.step();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let logo = RichText::new("Arena Editor").strong().size(15.0).color(theme::TEXT);
            theme::logo(ui);
            if step == Step::Home {
                ui.label(logo);
            } else {
                if theme::ghost(ui, RichText::new("‹  Arenas").color(theme::TEXT_SOFT)).on_hover_text("Back to your arenas").clicked() {
                    self.go(Step::Home);
                }
                ui.add(egui::Separator::default().vertical().spacing(4.0));
                let name = match (&self.level, &self.generating) {
                    (_, Some(g)) => g.form.name.clone(),
                    (Some(l), None) => l.recipe.name.clone(),
                    _ => String::new(),
                };
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(name).strong().size(14.5));
                    let (said, col) = self.saved_state();
                    ui.label(RichText::new(said).size(11.5).color(col));
                });
            }
            // The steps, in the middle of what is left.
            if step != Step::Home {
                let left = ui.min_rect().right();
                let mid = ui.max_rect().center().x;
                ui.add_space((mid - left - 190.0).max(12.0));
                self.steps(ui, step);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if step != Step::Home && theme::primary(ui, "⏵  Play now", self.level.is_some() || self.generating.is_some()).on_hover_text("Walk the level in the Combat Simulator: the usual settings, no simulants").clicked() {
                    self.play_now();
                }
                if theme::ghost(ui, RichText::new("?").strong().size(15.0)).on_hover_text("Help: how to move around, and every shortcut").clicked() {
                    self.help = !self.help;
                }
                if step != Step::Home && self.generating.is_none() {
                    let (undo, redo) = self.level.as_ref().map_or((false, false), |l| (l.doc.can_undo(), l.doc.can_redo()));
                    if ui.add_enabled(redo, egui::Button::new("Redo").frame(false).min_size(egui::vec2(32.0, 32.0))).on_hover_text("Redo (Ctrl+Y)").clicked() {
                        self.undo(true);
                    }
                    if ui.add_enabled(undo, egui::Button::new("Undo").frame(false).min_size(egui::vec2(32.0, 32.0))).on_hover_text("Undo (Ctrl+Z)").clicked() {
                        self.undo(false);
                    }
                }
            });
        });
    }

    /// "All changes saved", or what is being done.
    fn saved_state(&self) -> (String, Color32) {
        if let Some(job) = &self.job {
            return (format!("{}… {:.0} s", job.work.label(), job.started.elapsed().as_secs_f32()), theme::WARN);
        }
        if self.generating.is_some() {
            return if self.level.is_some() { ("Shaping the rooms: Build the arena to keep them".into(), theme::MUTED) } else { ("Not built yet".into(), theme::MUTED) };
        }
        match &self.level {
            Some(l) if l.failed.is_some() => ("Could not be built".into(), theme::DANGER),
            Some(l) if l.pending.is_some() => ("Saving…".into(), theme::MUTED),
            Some(l) if !self.paths.is_imported(&l.recipe.code) => ("Not built yet".into(), theme::WARN),
            Some(_) => ("All changes saved".into(), theme::OK),
            None => (String::new(), theme::MUTED),
        }
    }

    fn steps(&mut self, ui: &mut egui::Ui, step: Step) {
        let built = self.level.is_some();
        egui::Frame::new().fill(theme::FIELD).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(3)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (k, s, name) in [(1, Step::Layout, "Layout"), (2, Step::Items, "Items"), (3, Step::Test, "Test & play")] {
                let on = s == step;
                let done = !on && built && (s as u8) < (step as u8);
                let mark = if done { "✔".to_string() } else { k.to_string() };
                let text = RichText::new(format!("{mark}   {name}")).strong().color(if on { Color32::WHITE } else { theme::TEXT_SOFT });
                let enabled = built || s == Step::Layout;
                let b = egui::Button::new(text).fill(if on { theme::SELECTED } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).min_size(egui::vec2(0.0, 30.0));
                let r = ui.add_enabled(enabled, b);
                let r = if enabled { r } else { r.on_disabled_hover_text("Build the arena first") };
                if r.clicked() {
                    self.go(s);
                }
            }
        });
    }

    // ── The status bar ─────────────────────────────────────────────────────

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            match self.step() {
                Step::Home => theme::help(ui, "Pick an arena to edit it, or make a new one."),
                Step::Layout if self.generating.is_some() => {
                    theme::key_hint(ui, "Drag", "a room to move it");
                    ui.add_space(10.0);
                    theme::key_hint(ui, "Drag", "an edge to resize");
                    ui.add_space(10.0);
                    theme::key_hint(ui, "Right-click", "a room to stand in it in 3D");
                }
                _ => self.tool_hints(ui),
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some((m, err)) = &self.message {
                    ui.label(RichText::new(m).size(12.5).color(if *err { theme::DANGER } else { theme::TEXT_SOFT }));
                } else if let Some(l) = &self.level {
                    let d = &l.doc;
                    let t = format!("{} spawns  ·  {} weapons  ·  {} ammo  ·  {} hills  ·  {} doors", d.count(Kind::Spawn), d.count(Kind::Weapon), d.count(Kind::Ammo), d.count(Kind::Hill), d.count(Kind::Door));
                    ui.label(RichText::new(t).size(12.5).color(theme::MUTED));
                }
            });
        });
    }

    fn tool_hints(&self, ui: &mut egui::Ui) {
        let hints: &[(&str, &str)] = match self.tool {
            Tool::Select => &[("Click", "select"), ("Drag", "the arrows to move"), ("R", "turn"), ("Shift+D", "duplicate"), ("Ctrl", "snap"), ("Del", "delete")],
            Tool::Add(_) => &[("Click", "a floor to place it"), ("Esc", "stop adding")],
            Tool::AddDoor => &[("Click", "inside a doorway: the door fits itself to it"), ("Esc", "stop adding")],
            Tool::AddNode => &[("Click", "a floor to add a waypoint"), ("Esc", "stop")],
            Tool::Link => &[("Click", "two waypoints to join them (or cut their link)"), ("Shift", "one way"), ("Esc", "stop")],
        };
        for (k, d) in hints {
            theme::key_hint(ui, k, d);
            ui.add_space(8.0);
        }
    }

    // ── Home ───────────────────────────────────────────────────────────────

    pub(super) fn home(&mut self, ui: &mut egui::Ui) {
        let sets_dir = pd_import::sets::dir_for(&self.paths.levels);
        let sets = pd_import::sets::list(&sets_dir);
        let active = pd_import::sets::active(&sets_dir, &self.paths.custom);
        let in_set: Vec<String> = sets.iter().find(|s| Some(&s.0) == active.as_ref()).map(|s| s.1.arenas.clone()).unwrap_or_default();
        let entries: Vec<_> = self.paths.recipes().into_iter().filter_map(|e| Recipe::load(&e.path).ok().map(|r| (e, r))).collect();
        let ctx = ui.ctx().clone();
        let mut cards: Vec<Card> = entries
            .into_iter()
            .map(|(e, r)| Card {
                in_set: in_set.contains(&e.code),
                code: e.code.clone(),
                name: if e.name.is_empty() { e.code.clone() } else { e.name.clone() },
                thumb: self.thumb(&ctx, &e.code),
                source: source_words(&r),
                generated: matches!(r.source, Source::Autolevel(_)),
                imported: self.paths.is_imported(&e.code),
                path: e.path,
            })
            .collect();
        // The set's arenas in its order (the arena menu's); every arena by name.
        cards.sort_by_key(|c| in_set.iter().position(|x| *x == c.code).unwrap_or(usize::MAX));
        let (set_count, all_count) = (cards.iter().filter(|c| c.in_set).count(), cards.len());
        let mut set_act: Option<(CardAct, String, String)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let width = ui.available_width().min(1180.0);
            let pad = ((ui.available_width() - width) / 2.0).max(16.0);
            egui::Frame::new().inner_margin(Margin { left: pad as i8, right: pad as i8, top: 36, bottom: 40 }).show(ui, |ui| {
                ui.set_max_width(width);
                ui.label(RichText::new("Make an arena").size(28.0).strong());
                ui.label(RichText::new("Build a level for the Combat Simulator, place weapons and spawn points, then walk it in the game.").size(15.0).color(theme::MUTED));
                ui.add_space(18.0);
                let mut act = None;
                ui.columns(3, |cols| {
                    let starts = [
                        ("🎲", "Generate an arena", "Pick a style and the editor builds the rooms, ramps and corridors. Reshape any room afterwards.", "Choose a style", 0),
                        ("✏", "Start from a blank plan", "Draw your own rooms on a grid. The editor adds the walls, stairs and doorways between them.", "Open the plan", 1),
                        ("📦", "Import a 3D model", "Bring in a .glb or .gltf file from Blender or anywhere else. You place the items; the walls come from the model.", "Choose a file", 2),
                    ];
                    for (col, (icon, title, text, go, k)) in cols.iter_mut().zip(starts) {
                        // (columns justify their text: not a card's)
                        if col.with_layout(Layout::top_down(Align::Min), |ui| start_card(ui, icon, title, text, go, k == 0)).inner {
                            act = Some(k);
                        }
                    }
                });
                match act {
                    Some(0) => self.generate(None),
                    Some(1) => {
                        self.generate(None);
                        if let Some(g) = self.generating.as_mut() {
                            g.ed.set_empty(true);
                        }
                    }
                    Some(2) => {
                        if let Some(p) = rfd::FileDialog::new().add_filter("3D model (glTF)", &["glb", "gltf"]).pick_file() {
                            self.begin_import(&p);
                        }
                    }
                    _ => {}
                }
                ui.add_space(34.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Your arenas").size(20.0).strong());
                    ui.add_space(12.0);
                    // The set the game shows.
                    ui.label(RichText::new("Set").color(theme::MUTED));
                    let name = sets.iter().find(|s| Some(&s.0) == active.as_ref()).map_or("(none)".to_owned(), |s| s.1.name.clone());
                    let mut pick = None;
                    egui::ComboBox::from_id_salt("arena set").selected_text(RichText::new(&name).strong()).width(180.0).show_ui(ui, |ui| {
                        for (stem, set) in &sets {
                            if ui.selectable_label(Some(stem) == active.as_ref(), format!("{}  ({})", set.name, set.arenas.len())).clicked() {
                                pick = Some(stem.clone());
                            }
                        }
                    })
                    .response
                    .on_hover_text("The game's Custom arenas are this set's (the built ones)");
                    if let Some(stem) = pick.filter(|s| Some(s) != active.as_ref()) {
                        match pd_import::sets::set_active(&sets_dir, &self.paths.custom, &self.paths.levels, &stem) {
                            Ok(()) => self.say("The game now shows this set's arenas", false),
                            Err(e) => self.say(e, true),
                        }
                    }
                    ui.menu_button("Set…", |ui| {
                        if ui.button("New set").clicked() {
                            self.set_dialog = Some(SetDialog::New(String::new()));
                            ui.close_menu();
                        }
                        if ui.button("Rename this set").clicked() {
                            self.set_dialog = Some(SetDialog::Rename(name.clone()));
                            ui.close_menu();
                        }
                        let others: Vec<&(String, pd_import::sets::ArenaSet)> = sets.iter().filter(|s| Some(&s.0) != active.as_ref()).collect();
                        if !others.is_empty() {
                            ui.separator();
                            for (stem, set) in others {
                                if ui.button(format!("Delete the set {}…", set.name)).clicked() {
                                    self.set_dialog = Some(SetDialog::DeleteSet(stem.clone(), set.name.clone()));
                                    ui.close_menu();
                                }
                            }
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.home_search).hint_text("Search arenas").desired_width(220.0));
                        for (k, name) in [(2u8, "Imported"), (1, "Generated"), (0, "All")] {
                            if ui.selectable_label(self.home_filter == k, name).clicked() {
                                self.home_filter = k;
                            }
                        }
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.selectable_label(!self.home_all, format!("In this set ({set_count})")).clicked() {
                        self.home_all = false;
                    }
                    if ui.selectable_label(self.home_all, format!("All arenas ({all_count})")).on_hover_text("Every arena on disk; add one to the set from its ··· menu").clicked() {
                        self.home_all = true;
                    }
                    theme::help(ui, if self.home_all { "Arenas not in the set are dimmed: the game doesn't show them." } else { "What the game shows, in its order. New arenas join this set." });
                });
                ui.add_space(8.0);
                let find = self.home_search.to_lowercase();
                let home_all = self.home_all;
                let shown: Vec<&Card> = cards.iter().filter(|c| home_all || c.in_set).filter(|c| match self.home_filter {
                    1 => c.generated,
                    2 => !c.generated,
                    _ => true,
                }).filter(|c| find.is_empty() || c.name.to_lowercase().contains(&find)).collect();
                if shown.is_empty() {
                    theme::help(ui, if cards.is_empty() { "None yet: make one above." } else if !home_all && set_count == 0 { "This set is empty: make an arena above, or add some from All arenas." } else { "No arena matches." });
                }
                let per_row = ((width + 16.0) / 290.0).floor().max(1.0) as usize;
                let mut open = None;
                for row in shown.chunks(per_row) {
                    ui.columns(per_row, |cols| {
                        for (col, c) in cols.iter_mut().zip(row) {
                            match col.with_layout(Layout::top_down(Align::Min), |ui| level_card(ui, c)).inner {
                                Some(CardAct::Open) => open = Some(c.path.clone()),
                                Some(act) => set_act = Some((act, c.code.clone(), c.name.clone())),
                                None => {}
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                if let Some(p) = open {
                    self.open(p);
                    self.step = Step::Items;
                }
            });
        });
        if let (Some((act, code, name)), Some(stem)) = (set_act, active) {
            let (dir, custom, levels) = (&sets_dir, &self.paths.custom, &self.paths.levels);
            let done = match act {
                CardAct::AddToSet => pd_import::sets::add(dir, &stem, &code).and_then(|()| pd_import::sets::write_levels(dir, custom, levels)).map(|()| format!("{name} is in the set")),
                CardAct::RemoveFromSet => pd_import::sets::remove(dir, &stem, &code).and_then(|()| pd_import::sets::write_levels(dir, custom, levels)).map(|()| format!("{name} is out of the set (still in All arenas)")),
                CardAct::Delete => {
                    self.set_dialog = Some(SetDialog::DeleteArena(code, name));
                    Ok(String::new())
                }
                CardAct::Open => Ok(String::new()),
            };
            match done {
                Ok(m) if !m.is_empty() => self.say(m, false),
                Ok(_) => {}
                Err(e) => self.say(e, true),
            }
        }
        self.set_dialog_ui(ui.ctx());
    }

    /// Home's dialogs about sets: a new set's name, renaming, and the two
    /// confirmations (a set, an arena for good).
    fn set_dialog_ui(&mut self, egui: &egui::Context) {
        let Some(dialog) = self.set_dialog.as_mut() else { return };
        let sets_dir = pd_import::sets::dir_for(&self.paths.levels);
        let (mut close, mut go) = (false, false);
        let frame = egui::Frame::new().fill(theme::RAISED).stroke(Stroke::new(1.0, theme::BORDER_STRONG)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(22));
        egui::Modal::new(egui::Id::new("set dialog")).frame(frame).show(egui, |ui| {
            ui.set_width(440.0);
            let (title, text, button, danger) = match dialog {
                SetDialog::New(_) => ("New arena set", "An empty set: the game shows its arenas once it is in use. Make arenas, or add them from All arenas.", "Make the set", false),
                SetDialog::Rename(_) => ("Rename this set", "", "Rename", false),
                SetDialog::DeleteSet(..) => ("Delete this set?", "Only the list goes: its arenas stay, in All arenas and in other sets.", "Delete the set", true),
                SetDialog::DeleteArena(..) => ("Delete this arena for good?", "Its recipe, the items you placed and its built files are deleted, and it leaves every set. This can't be undone here.", "Delete the arena", true),
            };
            let title = match dialog {
                SetDialog::DeleteSet(_, name) => format!("Delete the set {name}?"),
                SetDialog::DeleteArena(_, name) => format!("Delete {name} for good?"),
                _ => title.to_owned(),
            };
            ui.label(RichText::new(title).size(18.0).strong());
            ui.add_space(6.0);
            if !text.is_empty() {
                theme::help(ui, text);
                ui.add_space(8.0);
            }
            if let SetDialog::New(name) | SetDialog::Rename(name) = dialog {
                let r = ui.add(egui::TextEdit::singleline(name).hint_text("Its name").desired_width(f32::INFINITY));
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = true;
                }
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                let ready = !matches!(dialog, SetDialog::New(n) | SetDialog::Rename(n) if n.trim().is_empty());
                let b = egui::Button::new(RichText::new(button).strong().color(if danger { Color32::WHITE } else { theme::ON_ACCENT })).fill(if danger { Color32::from_rgb(0xb8, 0x3a, 0x3a) } else { theme::ACCENT });
                go |= ui.add_enabled(ready, b).clicked();
                close = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if close {
            self.set_dialog = None;
            return;
        }
        if !go {
            return;
        }
        let Some(dialog) = self.set_dialog.take() else { return };
        let (custom, levels) = (self.paths.custom.clone(), self.paths.levels.clone());
        let done = match dialog {
            SetDialog::New(name) => {
                let base = crate::newlevel::code_from(name.trim());
                let mut stem = base.clone();
                let mut k = 2;
                while sets_dir.join(format!("{stem}.json")).exists() {
                    stem = format!("{base}{k}");
                    k += 1;
                }
                pd_import::sets::save(&sets_dir, &stem, &pd_import::sets::ArenaSet::new(name.trim()))
                    .and_then(|()| pd_import::sets::set_active(&sets_dir, &custom, &levels, &stem))
                    .map(|()| format!("Made the set {}: the game shows its arenas", name.trim()))
            }
            SetDialog::Rename(name) => match pd_import::sets::active(&sets_dir, &custom) {
                Some(stem) => pd_import::sets::rename(&sets_dir, &stem, &name).map(|()| format!("Renamed the set {}", name.trim())),
                None => Err("no set in use".into()),
            },
            SetDialog::DeleteSet(stem, name) => pd_import::sets::delete_set(&sets_dir, &custom, &stem).map(|()| format!("Deleted the set {name}")),
            SetDialog::DeleteArena(code, name) => {
                self.thumbs.remove(&code);
                pd_import::sets::delete_arena(&sets_dir, &custom, &levels, &code).map(|()| format!("Deleted {name}"))
            }
        };
        match done {
            Ok(m) => self.say(m, false),
            Err(e) => self.say(e, true),
        }
    }

    // ── Importing a model ──────────────────────────────────────────────────

    /// The import dialog, for `file`.
    pub fn begin_import(&mut self, file: &std::path::Path) {
        self.new_level = Some(NewLevel::for_file(file, &self.paths));
        self.import_advanced = false;
    }

    pub(super) fn import_dialog(&mut self, egui: &egui::Context) {
        let Some(n) = self.new_level.as_mut() else { return };
        let mut close = false;
        let mut go = false;
        let modal = egui::Modal::new(egui::Id::new("import")).frame(egui::Frame::new().fill(theme::RAISED).stroke(Stroke::new(1.0, theme::BORDER_STRONG)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(22)));
        modal.show(egui, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new("Import a 3D model").size(20.0).strong());
            ui.add_space(10.0);
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(n.file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()).strong());
                let size = std::fs::metadata(&n.file).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
                theme::help(ui, &format!("{size:.1} MB  ·  {}", n.file.parent().map(|p| p.display().to_string()).unwrap_or_default()));
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Arena name").strong());
            let was = n.name.clone();
            ui.add(egui::TextEdit::singleline(&mut n.name).desired_width(f32::INFINITY));
            if n.name != was {
                n.code = crate::newlevel::code_from(&n.name);
            }
            theme::help(ui, "Shown in the Combat Simulator under Arena, Custom.");
            ui.add_space(8.0);
            ui.label(RichText::new("The model's units").strong());
            ui.horizontal(|ui| {
                for (scale, name) in [(100.0, "Metres"), (1.0, "Centimetres"), (2.54, "Inches")] {
                    if ui.selectable_label((n.scale - scale).abs() < 1e-3, name).clicked() {
                        n.scale = scale;
                    }
                }
            });
            theme::help(ui, "Most glTF files are in metres. A doorway in the arenas is about 2 m tall.");
            ui.add_space(8.0);
            ui.label(RichText::new("To start with").strong());
            theme::help(ui, "Placed for you on the model's floors. You move, add and remove them afterwards.");
            ui.horizontal(|ui| {
                ui.label("Spawn points");
                ui.add(egui::DragValue::new(&mut n.spawns).range(1..=32));
                ui.add_space(12.0);
                ui.label("Weapons");
                ui.add(egui::DragValue::new(&mut n.weapons).range(0..=16));
                ui.add_space(12.0);
                ui.label("Hills");
                ui.add(egui::DragValue::new(&mut n.hills).range(0..=12));
            });
            ui.add_space(6.0);
            ui.checkbox(&mut n.climb_ledges, "Let players climb low walls (up to 1.6 m; the original arenas have none)");
            egui::CollapsingHeader::new("Advanced").default_open(self.import_advanced).show(ui, |ui| {
                egui::Grid::new("import advanced").num_columns(2).show(ui, |ui| {
                    ui.label("Scale").on_hover_text("The model's units to centimetres");
                    ui.add(egui::DragValue::new(&mut n.scale).range(0.01..=10000.0).speed(1.0));
                    ui.end_row();
                    ui.label("Rooms").on_hover_text("0: one per ~250 m² of floor");
                    ui.add(egui::DragValue::new(&mut n.rooms).range(0..=150));
                    ui.end_row();
                    ui.label("Largest texture");
                    ui.add(egui::DragValue::new(&mut n.max_texture).range(8..=4096));
                    ui.end_row();
                });
            });
            let problem = n.problem(&self.paths);
            if let Some(p) = &problem {
                ui.colored_label(theme::WARN, p);
            }
            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                theme::help(ui, "Importing takes about a minute.");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    go = theme::primary(ui, "Import", problem.is_none() && self.job.is_none()).clicked();
                    close = ui.button("Cancel").clicked();
                });
            });
        });
        if go {
            let n = self.new_level.clone().unwrap();
            match n.write(&self.paths) {
                Ok(path) => {
                    self.open(path);
                    self.step = Step::Items;
                }
                Err(e) => self.say(e, true),
            }
        } else if close {
            self.new_level = None;
        }
    }

    // ── Layout, for a level that wasn't generated ──────────────────────────

    pub(super) fn model_panel(&mut self, ui: &mut egui::Ui) {
        let Some(level) = self.level.as_ref() else { return };
        let (source, report) = (source_words(&level.recipe), level.report.clone());
        ui.label(RichText::new("Model").size(17.0).strong());
        theme::help(ui, &format!("{source}. Its rooms and walls come from the source; edit them in your modelling tool, then rebuild here."));
        ui.add_space(8.0);
        let busy = self.job.is_some();
        if theme::primary(ui, "Rebuild from the source", !busy).on_hover_text("Convert the source again (after a change to it); your items are kept").clicked() {
            self.start(Work::Import);
        }
        ui.add_space(12.0);
        theme::section(ui, "View");
        ui.checkbox(&mut self.far, "See the whole level (no far limit)");
        ui.add_space(12.0);
        if !report.is_empty() {
            egui::CollapsingHeader::new("What the last build said").show(ui, |ui| {
                for l in &report {
                    ui.label(RichText::new(l).size(12.0).color(theme::MUTED));
                }
            });
        }
    }

    // ── Test & play ────────────────────────────────────────────────────────

    /// The checklist, worked out for the open level.
    pub(super) fn checks(&self) -> Vec<ready::Check> {
        let Some(level) = self.level.as_ref() else { return Vec::new() };
        let built = self.paths.is_imported(&level.recipe.code) && level.scene.is_some();
        ready::checks(&level.doc, built, level.scene.as_ref().map(|s| s.graph.stranded()))
    }

    pub(super) fn checklist_panel(&mut self, ui: &mut egui::Ui) {
        let checks = self.checks();
        let left = checks.iter().filter(|c| !c.ok).count();
        ui.label(RichText::new("Ready to play?").size(18.0).strong());
        theme::help(ui, &match left {
            0 => "Everything looks good. Play it, or keep editing.".to_owned(),
            1 => "1 suggestion. None stops you playing.".to_owned(),
            n => format!("{n} suggestions. None stops you playing."),
        });
        ui.add_space(8.0);
        let mut fix = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for c in &checks {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        let (bg, fg) = if c.ok { Tone::Ok } else { Tone::Warn }.colours();
                        egui::Frame::new().fill(bg).corner_radius(CornerRadius::same(11)).inner_margin(Margin::symmetric(6, 2)).show(ui, |ui| ui.label(RichText::new(if c.ok { "✔" } else { "!" }).strong().color(fg)));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&c.title).strong());
                            theme::help(ui, &c.detail);
                            if let Some((button, f)) = &c.fix {
                                if ui.button(*button).clicked() {
                                    fix = Some(f.clone());
                                }
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
        });
        if let Some(f) = fix {
            self.apply_fix(f);
        }
    }

    fn apply_fix(&mut self, fix: Fix) {
        match fix {
            Fix::Build => self.start(Work::Import),
            Fix::Add(kind) => {
                self.go(Step::Items);
                self.tab = Tab::Items;
                self.tool = Tool::Add(kind);
            }
            Fix::CratesFor(weapons) => {
                let Some(level) = self.level.as_mut() else { return };
                let mut made = 0;
                for w in weapons {
                    let p = level.doc.pos(Item { kind: Kind::Weapon, index: w });
                    // (beside it; the placement puts it on the floor)
                    if level.doc.add(Kind::Ammo, p + glam::Vec3::new(80.0, 0.0, 0.0), 0.0, 0, Some(w)).is_some() {
                        made += 1;
                    }
                }
                self.say(format!("{made} ammo crates added beside their weapons"), false);
                self.edited();
            }
            Fix::ShowPaths => {
                self.go(Step::Items);
                self.tab = Tab::Waypoints;
                self.show.waypoints = true;
                // (the first stranded waypoint, framed)
                let at = self.scene().and_then(|s| (0..s.graph.pos.len()).find(|&i| s.graph.part.get(i).copied().unwrap_or(0) != 0).map(|i| (i, s.graph.pos[i])));
                if let Some((i, p)) = at {
                    self.sel = Some(Sel::Node(i));
                    self.frame_on(p);
                }
            }
        }
    }

    pub(super) fn test_panel(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Simulant test");
        theme::help(ui, "Six simulants play a one-minute match on the level. It finds places where they get stuck, and rooms nobody reaches.");
        let busy = self.job.is_some();
        let test = self.level.as_ref().and_then(|l| l.test.clone());
        if let Some(t) = &test {
            // The numbers, two by two (the mockup's tiles).
            let tiles = [
                (t.kills.to_string(), "kills in a minute".to_owned(), false),
                (t.stalls.to_string(), if t.stalls == 1 { "time one got stuck".to_owned() } else { "times one got stuck".to_owned() }, t.stalls > 0),
                (format!("{} of {}", t.visited, t.rooms), "rooms visited".to_owned(), false),
                (t.ledge_falls.to_string(), if t.ledge_falls == 1 { "fall off a ledge".to_owned() } else { "falls off ledges".to_owned() }, false),
            ];
            let w = (ui.available_width() - 8.0) / 2.0;
            for pair in tiles.chunks(2) {
                ui.horizontal(|ui| {
                    for (big, small, warn) in pair {
                        theme::card().show(ui, |ui| {
                            ui.set_width(w - 26.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(big).size(22.0).strong().color(if *warn { theme::WARN } else { theme::TEXT }));
                                theme::help(ui, small);
                            });
                        });
                    }
                });
            }
            if let Some(&p) = t.unvisited.first() {
                ui.horizontal_wrapped(|ui| {
                    theme::help(ui, &format!("{} room{} nobody went to in the minute.", t.unvisited.len(), if t.unvisited.len() == 1 { "" } else { "s" }));
                    if ui.button("Show me").on_hover_text("Fly to the first of them").clicked() {
                        self.frame_on(p);
                    }
                });
            }
            if t.hits > 0 || t.shots > 0 {
                theme::help(ui, &format!("{} shots, {} hits.", t.shots, t.hits));
            }
        }
        let label = if test.is_some() { "↺  Run the test again" } else { "↺  Run the simulant test" };
        if ui.add_enabled(!busy, egui::Button::new(label).min_size(egui::vec2(ui.available_width(), 32.0))).clicked() {
            if let Some(l) = self.level.as_mut() {
                l.report.clear();
                l.test = None;
            }
            self.start(Work::Check);
        }
        ui.add_space(18.0);
        theme::section(ui, "Play");
        egui::Frame::new().fill(Color32::from_rgb(0x14, 0x20, 0x33)).stroke(Stroke::new(1.0, Color32::from_rgb(0x2b, 0x45, 0x66))).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Walk your level").size(15.0).strong());
            theme::help(ui, "Opens it in the Combat Simulator with the usual settings and no simulants. Close the game to come back here.");
            ui.add_space(6.0);
            let b = egui::Button::new(RichText::new("⏵  Play now").strong().color(theme::ON_ACCENT)).fill(theme::ACCENT).stroke(Stroke::NONE).min_size(egui::vec2(ui.available_width(), 40.0));
            if ui.add(b).clicked() {
                self.play_now();
            }
        });
        ui.add_space(6.0);
        let name = self.level.as_ref().map(|l| l.recipe.name.clone()).unwrap_or_default();
        theme::help(ui, &format!("To play for real (simulants, teams, scenarios), start the game and choose Combat Simulator, then Arena, Custom, {name}."));
    }

    /// Play now: the level's edits saved and placed, then the game started on
    /// it (`perfect_dark --play <code>`). While a job runs it waits for it.
    pub(super) fn play_now(&mut self) {
        if self.generating.is_some() {
            return self.say("build the arena first (Build the arena, bottom left)", true);
        }
        let Some(level) = self.level.as_ref() else { return };
        if self.job.is_some() || level.pending.is_some() {
            self.play_queued = true;
            if self.job.is_none() {
                self.start(Work::Place { fresh: false });
            }
            return self.say("saving the level, then the game starts…", false);
        }
        let code = level.recipe.code.clone();
        let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(if cfg!(windows) { "perfect_dark.exe" } else { "perfect_dark" })));
        match exe.filter(|e| e.exists()) {
            Some(e) => match std::process::Command::new(&e).args(["--play", &code]).spawn() {
                Ok(_) => self.say("the game is starting on this level", false),
                Err(err) => self.say(format!("{}: {err}", e.display()), true),
            },
            None => self.say("the game isn't built beside the editor (cargo build --release)", true),
        }
    }

    // ── Help ───────────────────────────────────────────────────────────────

    pub(super) fn help_window(&mut self, egui: &egui::Context) {
        let mut open = self.help;
        egui::Window::new("Help").open(&mut open).collapsible(false).resizable(false).default_width(420.0).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 60.0)).show(egui, |ui| {
            theme::section(ui, "Moving around the 3D view");
            for (k, d) in [("Right-drag", "look around"), ("W A S D", "fly"), ("Q / E", "down / up"), ("Shift", "faster"), ("Wheel", "flying speed"), ("F", "frame the selection")] {
                ui.horizontal(|ui| theme::key_hint(ui, k, d));
            }
            theme::section(ui, "Editing");
            for (k, d) in [("Click", "select"), ("Drag the arrows", "move along a line"), ("T", "switch the arrows and the turning ring"), ("R / Shift+R", "turn by 45°"), ("G", "drop onto the floor"), ("Shift+D", "duplicate"), ("Delete", "delete"), ("Esc", "stop adding / clear the selection"), ("Ctrl+Z / Ctrl+Y", "undo / redo")] {
                ui.horizontal(|ui| theme::key_hint(ui, k, d));
            }
            theme::section(ui, "The steps");
            theme::help(ui, "1 Layout: the rooms (the generator's plan, or the model it came from). 2 Items: spawns, weapons, ammo, hills, bases, doors. 3 Test & play: the checklist, the simulant test, and Play now.");
        });
        self.help = open;
    }
}

/// A start card on Home; true when clicked.
fn start_card(ui: &mut egui::Ui, icon: &str, title: &str, text: &str, go: &str, recommended: bool) -> bool {
    let r = egui::Frame::new().fill(theme::RAISED).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(20)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(170.0);
        ui.horizontal(|ui| {
            egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x27, 0x38)).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(11, 8)).show(ui, |ui| ui.label(RichText::new(icon).size(20.0).color(theme::ACCENT_SOFT)));
            if recommended {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| theme::pill(ui, "Recommended", Tone::Ok));
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(title).size(17.0).strong());
        theme::help(ui, text);
        ui.add_space(4.0);
        ui.label(RichText::new(go).strong().color(theme::ACCENT_SOFT));
    });
    let resp = ui.interact(r.response.rect, ui.id().with(title), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(r.response.rect, CornerRadius::same(14), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A level's card on Home; true when clicked.
fn level_card(ui: &mut egui::Ui, c: &Card) -> Option<CardAct> {
    let mut act = None;
    let r = egui::Frame::new().fill(theme::RAISED).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Its picture (the view from a spawn), or a blank one until it is opened.
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, (w * 0.5).round()), egui::Sense::hover());
        let round = CornerRadius::same(8);
        match &c.thumb {
            Some(tex) => {
                // (cropped to the card's shape from the middle)
                let aspect = tex.aspect_ratio();
                let want = rect.width() / rect.height();
                let uv = if aspect > want {
                    let k = want / aspect;
                    egui::Rect::from_min_max(egui::pos2(0.5 - k / 2.0, 0.0), egui::pos2(0.5 + k / 2.0, 1.0))
                } else {
                    let k = aspect / want;
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.5 - k / 2.0), egui::pos2(1.0, 0.5 + k / 2.0))
                };
                egui::Image::new(tex).uv(uv).corner_radius(round).paint_at(ui, rect);
            }
            None => {
                ui.painter().rect_filled(rect, round, theme::FIELD);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, if c.imported { "Open it to take its picture" } else { "Not built yet" }, egui::FontId::proportional(12.5), theme::LABEL);
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(if c.generated { "🎲" } else { "📦" }).size(16.0).color(theme::ACCENT_SOFT));
            ui.label(RichText::new(&c.name).size(15.0).strong().color(if c.in_set { theme::TEXT } else { theme::MUTED }));
        });
        // (what a click opens: the picture and the name, not the menu below)
        let open = ui.min_rect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(c.source).size(12.5).color(theme::MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("···", |ui| {
                    let (label, a) = if c.in_set { ("Remove from this set", CardAct::RemoveFromSet) } else { ("Add to this set", CardAct::AddToSet) };
                    if ui.button(label).clicked() {
                        act = Some(a);
                        ui.close_menu();
                    }
                    if ui.button(RichText::new("Delete arena…").color(theme::DANGER)).clicked() {
                        act = Some(CardAct::Delete);
                        ui.close_menu();
                    }
                });
                if !c.in_set {
                    theme::pill(ui, "Not in set", Tone::Todo);
                } else if c.imported {
                    theme::pill(ui, "Built", Tone::Ok);
                } else {
                    theme::pill(ui, "Not built yet", Tone::Todo);
                }
            });
        });
        open
    });
    let resp = ui.interact(r.inner, ui.id().with(&c.path), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(r.response.rect, CornerRadius::same(12), Stroke::new(1.5, Color32::from_rgb(0x3b, 0x4b, 0x63)), egui::StrokeKind::Inside);
    }
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        return Some(CardAct::Open);
    }
    act
}
