//! The Items step: the palette of things to add and the list of what the
//! level has (left), the selection's settings (right), and the view's tool
//! bar (above the view). Doors are in the palette too; the simulants' paths
//! (the waypoints) are one click away, under the list.

use engine::egui::{self, Color32, CornerRadius, RichText, Stroke};
use pd_menu::generated::MP_WEAPON_SETS;

use super::{overlay, rgba, theme, App, Tab, Tool};
use crate::doc::{Item, Kind, Sel};
use crate::gizmo;

/// The palette: each kind, as a person calls it, and what placing one does.
const PALETTE: [(Kind, &str, &str); 8] = [
    (Kind::Spawn, "Spawn point", "Where players start and come back after dying"),
    (Kind::Weapon, "Weapon", "A spot for one gun of the weapon set chosen in the game"),
    (Kind::Ammo, "Ammo crate", "Ammo for the gun of the weapon it is placed beside"),
    (Kind::Hill, "Hill", "A hill for King of the Hill"),
    (Kind::Base, "Team base", "A team's base for Capture the Case"),
    (Kind::Respawn, "Team spawn", "Where a team starts in Capture the Case"),
    (Kind::Cover, "Cover", "Somewhere simulants take cover"),
    (Kind::Door, "Door", "A door, fitted to the doorway it is put in"),
];

/// A kind's name as the palette has it.
pub(super) fn kind_word(kind: Kind) -> &'static str {
    PALETTE.iter().find(|p| p.0 == kind).map_or(kind.name(), |p| p.1)
}

/// What a kind is for, in a line.
pub(super) fn kind_does(kind: Kind) -> &'static str {
    PALETTE.iter().find(|p| p.0 == kind).map_or("", |p| p.2)
}

const TEAM_NAMES: [&str; 4] = ["Red", "Yellow", "Blue", "Magenta"];

impl App {
    pub(super) fn items_left(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Add to the level");
        let w = (ui.available_width() - 12.0) / 3.0;
        egui::Grid::new("palette").num_columns(3).spacing(egui::vec2(6.0, 6.0)).show(ui, |ui| {
            for (i, (kind, name, does)) in PALETTE.into_iter().enumerate() {
                let on = match kind {
                    Kind::Door => self.tab == Tab::Doors,
                    k => self.tool == Tool::Add(k),
                };
                let b = egui::Button::new(RichText::new(name).size(12.5).color(if on { Color32::WHITE } else { theme::TEXT_SOFT }))
                    .fill(if on { theme::SELECTED } else { theme::RAISED })
                    .stroke(Stroke::new(1.0, if on { theme::ACCENT } else { theme::BORDER }))
                    .corner_radius(CornerRadius::same(9))
                    .min_size(egui::vec2(w, 44.0));
                if ui.add(b).on_hover_text(does).clicked() {
                    self.link_from = None;
                    if kind == Kind::Door {
                        self.tab = if on { Tab::Items } else { Tab::Doors };
                        self.tool = if !on && self.door_pick.is_some() { Tool::AddDoor } else { Tool::Select };
                    } else {
                        self.tab = Tab::Items;
                        self.tool = if on { Tool::Select } else { Tool::Add(kind) };
                    }
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        if matches!(self.tool, Tool::Add(Kind::Base | Kind::Respawn)) {
            ui.horizontal(|ui| {
                ui.label("Team");
                for t in 0..4u8 {
                    ui.selectable_value(&mut self.team, t, RichText::new(TEAM_NAMES[t as usize]).color(rgba(overlay::TEAMS[t as usize])));
                }
            });
        }
        if self.tool == Tool::Add(Kind::Ammo) {
            theme::help(ui, "The crate holds the ammo of the selected weapon, else of the nearest one.");
        }
        if self.tab == Tab::Doors {
            ui.add_space(6.0);
            ui.checkbox(&mut self.door_fit, "Fit a new door to its doorway");
            self.door_catalogue(ui);
            ui.horizontal(|ui| {
                if ui.button("Open every door").on_hover_text("A preview: they close again once the level is saved").clicked() {
                    self.preview_doors(true);
                }
                if ui.button("Close them").clicked() {
                    self.preview_doors(false);
                }
            });
        }
        ui.add_space(8.0);
        ui.separator();
        if self.tab == Tab::Waypoints {
            self.paths_left(ui);
            return;
        }
        theme::section(ui, "In this level");
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 44.0).show(ui, |ui| self.outliner(ui));
        ui.add_space(4.0);
        if ui.add(egui::Button::new("Simulant paths…").min_size(egui::vec2(ui.available_width(), 30.0))).on_hover_text("The routes simulants walk (advanced): they are made for you, and can be fixed by hand").clicked() {
            self.tab = Tab::Waypoints;
            self.tool = Tool::Select;
            self.sel = None;
        }
    }

    /// Everything the level has, by kind; a click selects and frames it.
    fn outliner(&mut self, ui: &mut egui::Ui) {
        let Some(level) = self.level.as_ref() else { return };
        let mut pick = None;
        for (kind, name, _) in PALETTE {
            let n = level.doc.count(kind);
            let id = ui.make_persistent_id(("outliner", name));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    ui.label(RichText::new(if kind == Kind::Cover { name.to_owned() } else { format!("{name}s") }).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(n.to_string()).color(theme::MUTED)));
                })
                .body(|ui| {
                    for index in 0..n {
                        let it = Item { kind, index };
                        let text = match kind {
                            Kind::Weapon => format!("Spot {}  ·  {}", index + 1, self.location_gun(level.doc.location(it).unwrap_or(0))),
                            Kind::Ammo => format!("Crate {}  ·  for spot {}", index + 1, level.doc.crate_weapon(it).unwrap_or(0) + 1),
                            Kind::Base | Kind::Respawn => format!("{} {}  ·  {}", name, index + 1, TEAM_NAMES[level.doc.team(it).unwrap_or(0) as usize % 4]),
                            Kind::Door => format!("Door {}  ·  {}", index + 1, self.catalogue.get(&level.doc.door(index).model).map_or(level.doc.door(index).model.clone(), |c| c.title())),
                            _ => format!("{name} {}", index + 1),
                        };
                        if theme::row(ui, self.sel == Some(Sel::Item(it)), text).clicked() {
                            pick = Some((it, level.doc.pos(it)));
                        }
                    }
                });
        }
        if let Some((it, p)) = pick {
            self.sel = Some(Sel::Item(it));
            self.tab = if it.kind == Kind::Door { Tab::Doors } else { Tab::Items };
            self.frame_on(if it.kind == Kind::Door { p + glam::Vec3::Y * 100.0 } else { p });
        }
    }

    /// The selection's settings (the right panel).
    pub(super) fn inspector_right(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.sel {
            None => {
                ui.add_space(8.0);
                ui.label(RichText::new("Nothing selected").size(15.0).strong());
                theme::help(ui, "Click something in the view, or in the list on the left, to see its settings here. Pick something under Add to the level to place one.");
            }
            Some(Sel::Item(it)) if it.kind == Kind::Door => self.door_inspector(ui, it.index),
            Some(Sel::Item(_)) => self.inspector(ui),
            Some(Sel::Node(_)) => self.waypoint_inspector(ui),
        });
    }

    /// The bar over the view: move, turn, drop; the weapon set shown; what is drawn.
    pub(super) fn view_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.gizmo_mode, gizmo::Mode::Move, "Move").on_hover_text("Drag the arrows to move along a line (T switches)");
            ui.selectable_value(&mut self.gizmo_mode, gizmo::Mode::Rotate, "Turn").on_hover_text("Drag the ring to turn (T switches)");
            if ui.add_enabled(self.sel.is_some(), egui::Button::new("Drop to floor")).on_hover_text("G: onto the floor under it").clicked() {
                self.ground_sel();
            }
            ui.add(egui::Separator::default().vertical());
            let res = self.res.clone();
            let set_name = |i: usize| res.as_ref().map_or(format!("set {i}"), |r| r.lang.get(MP_WEAPON_SETS[i].name).to_owned());
            let before = self.weapon_set;
            ui.label(RichText::new("Weapon set").color(theme::MUTED));
            egui::ComboBox::from_id_salt("weapon set").selected_text(set_name(self.weapon_set)).show_ui(ui, |ui| {
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
            ui.add(egui::Separator::default().vertical());
            ui.menu_button("Show…", |ui| {
                ui.checkbox(&mut self.show.items, "Items");
                ui.checkbox(&mut self.show.cover, "Cover");
                ui.checkbox(&mut self.show.waypoints, "Simulant paths");
                ui.checkbox(&mut self.show.labels, "Labels");
                ui.checkbox(&mut self.show.through_walls, "Markers through walls, faintly");
                ui.checkbox(&mut self.far, "The whole level (no far limit)");
            });
        });
    }
}
