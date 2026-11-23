//! The Layout step for a generated level, in plain words: a style to start
//! from, a variation (the seed), a size, the features it has, how much cover;
//! the plan's tools; what was changed. "Advanced" is the generator's own
//! panel, every option and list, as it was.
//!
//! Every control here sets the generator's options through its document
//! (`autolevel_edit::doc::Doc::set_option`, an undoable edit), so a level is
//! made only from what a person chose: nothing here changes a seed's level
//! by itself.

use autolevel::level::Options;
use autolevel::recipe;
use autolevel_edit::controls;
use autolevel_edit::ui::Tool as PlanTool;
use engine::egui::{self, Color32, CornerRadius, RichText, Stroke};

use super::{theme, App};

/// A style to start from: its name, what it makes, and the options it sets.
struct Style {
    name: &'static str,
    does: &'static str,
    sets: &'static [(&'static str, &'static str)],
}

const STYLES: [Style; 5] = [
    Style { name: "Classic complex", does: "Halls, corridors and small rooms on several floors", sets: &[("style", "complex"), ("scale", "grand"), ("size", "1")] },
    Style { name: "Small duel", does: "A few tight rooms for 2 to 4 players", sets: &[("style", "complex"), ("scale", "standard"), ("size", "0.5")] },
    Style { name: "Big and vertical", does: "More rooms, with platforms and drops", sets: &[("style", "complex"), ("scale", "grand"), ("size", "1.5")] },
    Style { name: "Towering halls", does: "Halls many floors tall, climbed from outside", sets: &[("style", "complex"), ("scale", "towering"), ("size", "1")] },
    Style { name: "Doors and offices", does: "A building cut into rooms, with doors", sets: &[("style", "felicity")] },
];

/// A feature switch: its name, the option, the value it is turned on at
/// ("true" for an option that is yes or no), and what it adds.
const FEATURES: [(&str, &str, &str, &str); 13] = [
    ("Vents", "vents", "4", "Crawl ducts, as short cuts"),
    ("Windows", "windows", "4", "Corridors that look onto a hall"),
    ("Drops", "drops", "2", "Holes in a floor into the room below"),
    ("Grates", "grates", "3", "Grates in a corridor's floor over a room"),
    ("Platforms", "platforms", "1", "Big platforms off a hall's catwalks"),
    ("Galleries", "galleries", "2", "High catwalks of their own in towering halls"),
    ("Cellars", "cellars", "0.3", "A room under a hall: a hatch down, a ladder up"),
    ("Angled walkways", "angled", "0.3", "Catwalks across a hall at an angle"),
    ("Flooded rooms", "flooded", "0.2", "A floor of water under railed walkways"),
    ("Sunken floors", "sunken", "0.2", "A pit in a hall's floor, a ramp down"),
    ("L and U rooms", "shaped", "0.2", "Rooms with a corner or a side cut away"),
    ("Bases", "bases", "true", "Rooms cut into a hall through a wall, the floor or the ceiling, slit windows into it"),
    ("Big pods", "big_pods", "true", "Rooms hung in a hall made bigger, where it has room"),
];

/// The value a feature switch is turned off at: "false" for one that is yes
/// or no, else none of it.
fn off_at(on_at: &str) -> &'static str {
    if on_at == "true" {
        "false"
    } else {
        "0"
    }
}

const SIZES: [(&str, &str, &str); 3] = [("Small", "0.75", "For 2 to 4 players"), ("Medium", "1", "For 4 to 8 players"), ("Large", "1.5", "For 8 to 12 players")];

/// Whether an option's value means it is on (yes, or a number above zero).
fn is_on(value: &str) -> bool {
    value == "true" || value.parse::<f64>().is_ok_and(|v| v > 0.0)
}

/// A new level's recipe in the editor: every feature switch on (the user,
/// 2026-11-20: "everything on"; doors come only with the "Doors and offices"
/// style). The generator's own defaults are left as they are (a seed's level
/// must never change by accident): each switch it leaves off is written into
/// the recipe at its "on" value, so the recipe says what it makes; those it
/// has on keep its values.
pub(super) fn new_recipe(seed: u64) -> recipe::Recipe {
    let mut r = recipe::Recipe::new(seed);
    for (_, name, on, _) in FEATURES {
        if !is_on(&recipe::option(&r.options, name).unwrap_or_default()) {
            recipe::set_option(&mut r.options, name, on).expect("a feature switch is an option");
        }
    }
    r
}

impl App {
    /// The simple Layout panel (under the arena's name and Build button).
    pub(super) fn shape_simple(&mut self, ui: &mut egui::Ui) {
        let Some(g) = self.generating.as_mut() else { return };
        let o = g.ed.doc.recipe.options.clone();
        let now = |name: &str| recipe::option(&o, name).unwrap_or_default();
        let style = o.style;
        let mut set: Vec<(&str, String)> = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // What the generator made of it.
            if let Some(why) = g.ed.doc.error() {
                ui.colored_label(theme::WARN, format!("These choices make no level: {why}. Undo, or change something."));
            } else if g.ed.doc.stale() || !g.ed.settled() {
                theme::help(ui, "Making the level…");
            } else if let Some(made) = g.ed.doc.made() {
                theme::help(ui, &format!("Made: {} floors, and every room can be reached.", autolevel_edit::planview::storeys(&made.level).len()));
            }
            theme::section(ui, "Style");
            for s in &STYLES {
                let on = s.sets.iter().all(|(k, v)| now(k) == *v || (*k == "size" && now(k).parse::<f64>().ok() == v.parse::<f64>().ok()));
                let r = egui::Frame::new()
                    .fill(if on { Color32::from_rgb(0x16, 0x22, 0x33) } else { theme::RAISED })
                    .stroke(Stroke::new(1.0, if on { theme::ACCENT } else { theme::BORDER }))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(s.name).strong());
                        ui.label(RichText::new(s.does).size(12.5).color(theme::MUTED));
                    });
                let click = ui.interact(r.response.rect, ui.id().with(s.name), egui::Sense::click());
                if click.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
                    set.extend(s.sets.iter().map(|(k, v)| (*k, v.to_string())));
                }
            }
            theme::section(ui, "Variation");
            ui.horizontal(|ui| {
                let seed = g.ed.doc.recipe.seed;
                if ui.button(" ‹ ").on_hover_text("The one before").clicked() {
                    g.ed.doc.set_seed(seed.saturating_sub(1));
                }
                ui.label(RichText::new(format!("No. {seed}")).strong().size(15.0));
                if ui.button(" › ").on_hover_text("The next one").clicked() {
                    g.ed.doc.set_seed(seed + 1);
                }
                if ui.button("Surprise me").on_hover_text("Another layout in the same style").clicked() {
                    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64);
                    g.ed.doc.set_seed(t % 100_000);
                }
            });
            theme::help(ui, "Each number is a different layout in the same style.");
            if controls::read_by("size", style) {
                theme::section(ui, "Size");
                ui.horizontal(|ui| {
                    let size: f64 = now("size").parse().unwrap_or(1.0);
                    for (name, v, does) in SIZES {
                        let on = (size - v.parse::<f64>().unwrap_or(1.0)).abs() < 1e-6;
                        if ui.selectable_label(on, name).on_hover_text(does).clicked() && !on {
                            set.push(("size", v.to_string()));
                        }
                    }
                });
            }
            theme::section(ui, "Features");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for (name, opt, on_at, does) in FEATURES {
                    let read = controls::read_by(opt, style);
                    let on = is_on(&now(opt));
                    let chip = egui::Button::new(RichText::new(name).size(13.0).color(if on { Color32::WHITE } else { theme::TEXT_SOFT }))
                        .fill(if on { theme::SELECTED } else { theme::RAISED })
                        .stroke(Stroke::new(1.0, if on { theme::ACCENT } else { theme::BORDER_STRONG }))
                        .corner_radius(CornerRadius::same(14));
                    let r = ui.add_enabled(read, chip).on_hover_text(does).on_disabled_hover_text(format!("{does} (not in this style)"));
                    if r.clicked() {
                        set.push((opt, if on { off_at(on_at).to_string() } else { on_at.to_string() }));
                    }
                }
            });
            if controls::read_by("clutter", style) {
                theme::section(ui, "Cover");
                let mut clutter: f64 = self.shape_clutter.unwrap_or_else(|| now("clutter").parse().unwrap_or(0.5));
                let r = ui.add(egui::Slider::new(&mut clutter, 0.0..=1.0).step_by(0.05).show_value(false));
                if r.changed() {
                    self.shape_clutter = Some(clutter);
                }
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    self.shape_clutter = None;
                    set.push(("clutter", controls::number(clutter)));
                }
                ui.horizontal(|ui| {
                    theme::help(ui, "Open floors");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| theme::help(ui, "Lots to hide behind"));
                });
            }
            theme::section(ui, "Change the plan");
            ui.horizontal(|ui| {
                if ui.selectable_label(g.ed.tool == PlanTool::Rooms, "Move and resize rooms").on_hover_text("Drag a room to move it, an edge to resize it; draw a new one on empty ground").clicked() {
                    g.ed.set_tool(PlanTool::Rooms);
                }
                if ui.selectable_label(g.ed.tool == PlanTool::Join, "Join rooms").on_hover_text("Click a room to see what it could be joined to, or a corridor to take it out").clicked() {
                    g.ed.set_tool(PlanTool::Join);
                    g.ed.set_side(false);
                }
            });
            ui.horizontal(|ui| {
                let side = g.ed.side;
                if ui.selectable_label(side.is_none(), "From above").on_hover_text("The plan: rooms seen from above").clicked() && side.is_some() {
                    g.ed.set_side(false);
                }
                if ui.selectable_label(side.is_some(), "From the side").on_hover_text("A slice through the level: drag a room up or down a floor, or its top or bottom to make it taller or shorter").clicked() && side.is_none() {
                    g.ed.set_side(true);
                }
            });
            if let Some(s) = g.ed.side {
                ui.horizontal(|ui| {
                    let across = s.axis == autolevel_edit::sideview::Axis::X;
                    if ui.button("Turn the slice").on_hover_text(if across { "Look along the other way (south to north)" } else { "Look along the other way (west to east)" }).clicked() {
                        g.ed.turn_side();
                    }
                    if ui.button(" ‹ ").on_hover_text(if across { "Slice a step further south" } else { "Slice a step further west" }).clicked() {
                        g.ed.nudge_side(-1);
                    }
                    if ui.button(" › ").on_hover_text(if across { "Slice a step further north" } else { "Slice a step further east" }).clicked() {
                        g.ed.nudge_side(1);
                    }
                    if ui.add_enabled(g.ed.doc.selected().is_some(), egui::Button::new("Through the room")).on_hover_text("Slice through the middle of the selected room").clicked() {
                        g.ed.side_through_selected();
                    }
                });
                theme::help(ui, "Drag a room up or down to move it a floor; drag its top or bottom edge to change how tall it is.");
            }
            let edits = &g.ed.doc.recipe.edits;
            theme::section(ui, "Your changes");
            if edits.is_empty() {
                theme::help(ui, "None yet: the layout as it comes.");
            }
            for e in edits.iter().rev().take(6) {
                ui.label(RichText::new(e.describe()).size(12.5).color(theme::TEXT_SOFT));
            }
            if edits.len() > 6 {
                theme::help(ui, &format!("and {} more (Advanced lists them all)", edits.len() - 6));
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(g.ed.doc.can_undo(), egui::Button::new("Undo")).clicked() {
                    g.ed.doc.undo();
                }
                if ui.add_enabled(g.ed.doc.can_redo(), egui::Button::new("Redo")).clicked() {
                    g.ed.doc.redo();
                }
            });
            ui.add_space(8.0);
            let usual = Options::default();
            let changed = recipe::OPTIONS.iter().filter(|n| recipe::option(&o, n) != recipe::option(&usual, n)).count();
            theme::help(ui, &format!("Advanced (at the top) has every setting the generator has{}.", if changed > 0 { format!("; {changed} differ from the usual") } else { String::new() }));
        });
        for (name, value) in set {
            if let Some(g) = self.generating.as_mut() {
                if let Err(why) = g.ed.doc.set_option(name, &value) {
                    self.say(why, true);
                }
            }
        }
    }
}

/// A room kind in plain words.
fn kind_word(kind: &str) -> String {
    match kind {
        "colossal" => "Huge hall".into(),
        "vast" => "Hall".into(),
        "atrium" => "Atrium".into(),
        "basin" => "Sunken hall".into(),
        "spire" => "Tall hall".into(),
        "long" => "Long room".into(),
        "medium" => "Room".into(),
        "small" => "Small room".into(),
        "tiny" => "Little room".into(),
        "loft" => "Loft".into(),
        "tower" => "Tower".into(),
        k => {
            let mut c = k.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// An upper storey's layout in plain words, and what it is.
fn layout_word(layout: &str) -> (&'static str, &'static str) {
    match layout {
        "none" => ("Open", "No walkways above: one tall open room."),
        "ring" => ("Ring", "A walkway round the walls above an open floor."),
        "galleries" => ("Galleries", "Walkways along its sides, above the floor."),
        "mezzanine" => ("Mezzanine", "A wide upper floor over part of the room."),
        _ => ("Other", ""),
    }
}

fn cover_word(cover: &str) -> &'static str {
    match cover {
        "mixed" => "Mixed",
        "pillars" => "Pillars",
        "crates" => "Crates",
        "none" => "None",
        "lanes" => "Pillars in the lanes",
        _ => "Other",
    }
}

fn ceiling_word(ceiling: &str) -> &'static str {
    match ceiling {
        "plain" => "Plain",
        "ceiling_tile" => "Tiles",
        "ceiling_dark" => "Dark tiles",
        "grille" => "Grille",
        _ => "Other",
    }
}

/// What the room panel was asked to do (done once it is drawn).
enum Ask {
    Set(recipe::Edit),
    Clear(&'static str),
    Reset,
    Floor(i32),
    /// The room's top on this ceiling line, its floor kept.
    Top(i32),
    Delete,
}

/// A storey's name: the generator's floor lines start a storey under the
/// ground.
fn floor_name(storey: Option<usize>) -> String {
    match storey {
        Some(0) => "Basement".to_owned(),
        Some(1) => "Ground floor".to_owned(),
        Some(n) => format!("Floor {}", n - 1),
        None => "A floor".to_owned(),
    }
}

impl App {
    /// The selected room's settings (the right panel on Layout): what it is,
    /// its floor, its inside, cover, ceiling and brightness, Reset and
    /// Delete. Each is one of the generator's own edits for one room
    /// (`autolevel_edit::ui::Editor::set_choice`, `change`), undoable as the
    /// others; Advanced has the generator's own panel for it.
    pub(super) fn room_panel(&mut self, ui: &mut egui::Ui) {
        let Some(g) = self.generating.as_mut() else { return };
        let Some(room) = g.ed.doc.selected().map(str::to_owned) else { return };
        let lot = g.ed.doc.lots().ok().and_then(|p| p.lots.into_iter().find(|a| a.key == room));
        let Some(lot) = lot else {
            theme::help(ui, "This room is not in the level yet.");
            return;
        };
        let storeys = autolevel::storeys::LEVELS;
        let storey = storeys.iter().position(|&l| l == lot.floor);
        let tall = storeys.iter().filter(|&&l| l >= lot.floor && l < lot.top).count().max(1);
        let metres = |cells: i32| cells as f32 * crate::preview::CELL / 100.0;
        // The name the level gave it ("The Drydock"), as the plan shows it.
        let name = g.ed.doc.made().and_then(|m| m.level.plan.rooms.iter().find(|r| r.key == room).map(|r| r.name.clone())).filter(|n| *n != room);
        let mut ask = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new(name.clone().unwrap_or_else(|| kind_word(lot.kind))).size(16.0).strong());
            let tall_text = if tall > 1 { format!("  ·  {tall} floors tall") } else { String::new() };
            let kind = if name.is_some() { format!("{}  ·  ", kind_word(lot.kind)) } else { String::new() };
            theme::help(ui, &format!("{kind}{}  ·  {:.0} × {:.0} m{tall_text}", floor_name(storey), metres(lot.x1 - lot.x0), metres(lot.y1 - lot.y0)));
            if !lot.host.is_empty() {
                theme::help(ui, "On the side of a hall, and furnished with it.");
            }
            theme::section(ui, "Floor");
            ui.horizontal(|ui| {
                let down = storey.filter(|&s| s > 0).map(|s| storeys[s - 1]);
                let up = storey.filter(|&s| s + 1 < storeys.len()).map(|s| storeys[s + 1]);
                if ui.add_enabled(down.is_some(), egui::Button::new("Down a floor")).clicked() {
                    ask = down.map(Ask::Floor);
                }
                if ui.add_enabled(up.is_some(), egui::Button::new("Up a floor")).clicked() {
                    ask = up.map(Ask::Floor);
                }
            });
            ui.horizontal(|ui| {
                // (the top is a ceiling line: a storey more or less over the same floor)
                let ceilings = autolevel::storeys::CEILINGS;
                let top = ceilings.iter().position(|&c| c == autolevel::storeys::snap_top(lot.top));
                let taller = top.and_then(|t| ceilings.get(t + 1)).copied();
                let shorter = top.filter(|_| tall > 1).map(|t| ceilings[t - 1]);
                if ui.add_enabled(taller.is_some(), egui::Button::new("Taller")).on_hover_text("A floor more on top").clicked() {
                    ask = taller.map(Ask::Top);
                }
                if ui.add_enabled(shorter.is_some(), egui::Button::new("Shorter")).on_hover_text("A floor less off the top").clicked() {
                    ask = shorter.map(Ask::Top);
                }
            });
            let c = match g.ed.doc.choices(&room) {
                Ok(c) => c,
                Err(_) => {
                    theme::help(ui, "Its inside can be chosen once the level is made.");
                    return;
                }
            };
            let pod = c.unfurnished.is_some();
            theme::section(ui, "Inside");
            ui.horizontal_wrapped(|ui| {
                let own = layout_word(c.layout.unwrap_or("none")).0.to_lowercase();
                if ui.selectable_label(c.chosen.layout.is_none(), "Auto").on_hover_text(format!("As the level makes it ({own})")).clicked() && c.chosen.layout.is_some() {
                    ask = Some(Ask::Clear("layout"));
                }
                for (layout, why) in &c.layouts {
                    let lit = c.chosen.layout.as_deref() == Some(*layout);
                    let (word, does) = layout_word(layout);
                    let b = ui.add_enabled(why.is_none(), egui::SelectableLabel::new(lit, word));
                    let b = match why {
                        Some(why) => b.on_disabled_hover_text(why.as_str()),
                        None => b.on_hover_text(does),
                    };
                    if b.clicked() && !lit {
                        ask = Some(Ask::Set(recipe::Edit::Layout { room: room.clone(), layout: layout.to_string() }));
                    }
                }
            });
            theme::help(ui, layout_word(c.chosen.layout.as_deref().or(c.layout).unwrap_or("none")).1);
            ui.horizontal(|ui| {
                ui.label("Cover");
                let now = c.chosen.cover.as_deref().map_or("Auto".to_owned(), |v| cover_word(v).to_owned());
                ui.add_enabled_ui(!pod, |ui| {
                    egui::ComboBox::from_id_salt("room cover").selected_text(now).show_ui(ui, |ui| {
                        if ui.selectable_label(c.chosen.cover.is_none(), format!("Auto ({})", cover_word(c.cover).to_lowercase())).clicked() && c.chosen.cover.is_some() {
                            ask = Some(Ask::Clear("cover"));
                        }
                        for cover in autolevel::level::COVER {
                            let lit = c.chosen.cover.as_deref() == Some(cover);
                            if ui.selectable_label(lit, cover_word(cover)).clicked() && !lit {
                                ask = Some(Ask::Set(recipe::Edit::Cover { room: room.clone(), cover: cover.to_string() }));
                            }
                        }
                    });
                });
                if !pod {
                    theme::help(ui, &format!("{} piece{}", c.pieces, if c.pieces == 1 { "" } else { "s" }));
                }
            });
            ui.horizontal(|ui| {
                let n = c.chosen.reroll.unwrap_or(0);
                if ui.add_enabled(!pod, egui::Button::new("Try another inside")).on_hover_text("Furnish it again, from other random numbers").clicked() {
                    ask = Some(Ask::Set(recipe::Edit::Reroll { room: room.clone(), n: n + 1 }));
                }
                if n > 0 && ui.button("Back").on_hover_text("The inside before this one").clicked() {
                    ask = Some(if n == 1 { Ask::Clear("reroll") } else { Ask::Set(recipe::Edit::Reroll { room: room.clone(), n: n - 1 }) });
                }
            });
            if let Some(why) = &c.unfurnished {
                theme::help(ui, why);
            }
            theme::section(ui, "Look");
            ui.horizontal(|ui| {
                ui.label("Ceiling");
                let now = c.chosen.ceiling.as_deref().map_or("Auto".to_owned(), |v| ceiling_word(v).to_owned());
                egui::ComboBox::from_id_salt("room ceiling").selected_text(now).show_ui(ui, |ui| {
                    if ui.selectable_label(c.chosen.ceiling.is_none(), "Auto").clicked() && c.chosen.ceiling.is_some() {
                        ask = Some(Ask::Clear("ceiling"));
                    }
                    for ceiling in recipe::CEILINGS {
                        let lit = c.chosen.ceiling.as_deref() == Some(ceiling);
                        if ui.selectable_label(lit, ceiling_word(ceiling)).clicked() && !lit {
                            ask = Some(Ask::Set(recipe::Edit::Ceiling { room: room.clone(), ceiling: ceiling.to_string() }));
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Brightness");
                let mut v = c.chosen.brightness.or(c.brightness).unwrap_or(128);
                let r = ui.add(egui::Slider::new(&mut v, 0..=255).show_value(false));
                // (one edit, and one step to undo, when it is let go)
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    ask = Some(Ask::Set(recipe::Edit::Brightness { room: room.clone(), value: v }));
                }
                if c.chosen.brightness.is_none() {
                    theme::help(ui, "Usual");
                } else if ui.small_button("Usual").clicked() {
                    ask = Some(Ask::Clear("brightness"));
                }
            });
            theme::help(ui, "The ceiling and brightness show in the built arena, not on the plan.");
            if let Some(why) = &g.ed.refused {
                ui.colored_label(theme::WARN, why);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!c.chosen.is_empty(), egui::Button::new("Reset room")).on_hover_text("Its inside, cover, ceiling and brightness as the level makes them").clicked() {
                    ask = Some(Ask::Reset);
                }
                if ui.button(RichText::new("Delete room").color(theme::DANGER)).clicked() {
                    ask = Some(Ask::Delete);
                }
            });
        });
        match ask {
            Some(Ask::Set(e)) => g.ed.set_choice(e),
            Some(Ask::Clear(what)) => g.ed.clear_choice(&room, what),
            Some(Ask::Reset) => {
                for what in autolevel::choices::KINDS {
                    g.ed.clear_choice(&room, what);
                }
            }
            Some(Ask::Floor(floor)) => {
                g.ed.change(&autolevel::boxes::Change::Pin { room: room.clone(), floor });
            }
            Some(Ask::Top(top)) => {
                g.ed.change(&autolevel::boxes::Change::Height { room: room.clone(), floor: lot.floor, top });
            }
            Some(Ask::Delete) => {
                g.ed.change(&autolevel::boxes::Change::Delete { room: room.clone() });
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new level has every feature switch on; those the generator has on
    /// by itself keep its values, and nothing else differs from its defaults.
    #[test]
    fn a_new_level_has_every_feature_on() {
        let (new, plain) = (new_recipe(40), recipe::Recipe::new(40));
        for (label, name, _, _) in FEATURES {
            let (now, was) = (recipe::option(&new.options, name).unwrap(), recipe::option(&plain.options, name).unwrap());
            assert!(is_on(&now), "{label} is off");
            if is_on(&was) {
                assert_eq!(now, was, "{label}: the generator's own value");
            }
        }
        let others: Vec<&str> = recipe::OPTIONS.into_iter().filter(|n| !FEATURES.iter().any(|f| f.1 == *n)).collect();
        assert!(others.iter().all(|n| recipe::option(&new.options, n) == recipe::option(&plain.options, n)));
        assert_eq!((new.seed, new.edits.len()), (40, 0));
    }

    /// Every feature switch's on and off values are ones its option takes,
    /// and read back as on and off.
    #[test]
    fn every_feature_switch_turns_on_and_off() {
        for (label, name, on_at, _) in FEATURES {
            let mut o = Options::default();
            for (v, want) in [(on_at, true), (off_at(on_at), false)] {
                recipe::set_option(&mut o, name, v).unwrap_or_else(|e| panic!("{label}: {e}"));
                assert_eq!(is_on(&recipe::option(&o, name).unwrap()), want, "{label} at {v}");
            }
        }
    }
}
