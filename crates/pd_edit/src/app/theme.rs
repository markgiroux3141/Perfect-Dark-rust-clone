//! The editor's look: one dark palette (the design mockup's), egui's visuals
//! and spacing set from it, and the few widgets every screen shares (a
//! primary button, a section label, a status pill, a card).

use engine::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};
use engine::wgpu;

pub const BG: Color32 = Color32::from_rgb(0x0d, 0x0f, 0x13);
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x15, 0x1b);
pub const RAISED: Color32 = Color32::from_rgb(0x15, 0x19, 0x20);
pub const FIELD: Color32 = Color32::from_rgb(0x0f, 0x12, 0x17);
pub const BORDER: Color32 = Color32::from_rgb(0x23, 0x28, 0x33);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x2c, 0x32, 0x40);
pub const HOVER: Color32 = Color32::from_rgb(0x22, 0x27, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xee);
pub const TEXT_SOFT: Color32 = Color32::from_rgb(0xc9, 0xce, 0xd8);
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xb3);
pub const LABEL: Color32 = Color32::from_rgb(0x8d, 0x96, 0xa7);
pub const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x9e, 0xff);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x7c, 0xc0, 0xff);
/// Text on an accent fill.
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x06, 0x12, 0x1f);
/// A selected row's or tab's fill.
pub const SELECTED: Color32 = Color32::from_rgb(0x1d, 0x2a, 0x3d);
pub const OK: Color32 = Color32::from_rgb(0x62, 0xd6, 0x9c);
pub const OK_BG: Color32 = Color32::from_rgb(0x12, 0x30, 0x1f);
pub const WARN: Color32 = Color32::from_rgb(0xf3, 0xb2, 0x59);
pub const WARN_BG: Color32 = Color32::from_rgb(0x3a, 0x2a, 0x10);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x8a, 0x8a);
/// What the view is cleared to where nothing is drawn.
pub const VIEW_CLEAR: wgpu::Color = wgpu::Color { r: 0x0b as f64 / 255.0, g: 0x0d as f64 / 255.0, b: 0x11 as f64 / 255.0, a: 1.0 };

/// Set the editor's visuals and spacing on `ctx` (once; egui keeps them).
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = RAISED;
    v.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = RAISED;
    v.code_bg_color = FIELD;
    v.hyperlink_color = ACCENT_SOFT;
    v.warn_fg_color = WARN;
    v.error_fg_color = DANGER;
    v.selection.bg_fill = SELECTED;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.indent_has_left_vline = false;
    let r = CornerRadius::same(7);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SOFT);
    w.noninteractive.corner_radius = r;
    for (wv, fill, stroke, text) in [(&mut w.inactive, Color32::from_rgb(0x1a, 0x1e, 0x26), BORDER_STRONG, TEXT), (&mut w.hovered, HOVER, Color32::from_rgb(0x3b, 0x4b, 0x63), Color32::WHITE), (&mut w.active, SELECTED, ACCENT, Color32::WHITE), (&mut w.open, SELECTED, ACCENT, Color32::WHITE)] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::new(1.0, stroke);
        wv.fg_stroke = Stroke::new(1.5, text);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 7.0);
    s.button_padding = egui::vec2(12.0, 6.0);
    s.interact_size = egui::vec2(36.0, 30.0);
    s.window_margin = Margin::same(16);
    s.menu_margin = Margin::same(6);
    s.indent = 16.0;
    s.slider_width = 150.0;
    s.combo_width = 150.0;
    s.text_edit_width = 220.0;
    s.scroll.bar_width = 8.0;
    use egui::{FontFamily::Proportional, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, egui::FontFamily::Monospace)),
    ]
    .into();
    ctx.set_style(style);
}

/// The frame of a side panel (left or right).
pub fn side_frame() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(14, 12)).stroke(Stroke::new(1.0, BORDER))
}

/// A small upper-case section label ("ADD TO THE LEVEL").
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text.to_uppercase()).size(11.5).strong().color(LABEL));
}

/// A line of help under a control.
pub fn help(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.5).color(MUTED)).wrap());
}

/// The screen's main action: an accent button.
pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let b = egui::Button::new(RichText::new(text).strong().color(if enabled { ON_ACCENT } else { MUTED })).fill(if enabled { ACCENT } else { RAISED }).stroke(Stroke::NONE).corner_radius(CornerRadius::same(8)).min_size(egui::vec2(0.0, 34.0));
    ui.add_enabled(enabled, b)
}

/// A button with no frame until it is hovered (top bars, icon rows).
pub fn ghost(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(text).frame(false).min_size(egui::vec2(32.0, 32.0)))
}

/// What a pill says about something: good, a suggestion, or not done yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Todo,
}

impl Tone {
    pub fn colours(self) -> (Color32, Color32) {
        match self {
            Tone::Ok => (OK_BG, OK),
            Tone::Warn => (WARN_BG, WARN),
            Tone::Todo => (Color32::from_rgb(0x1f, 0x24, 0x30), Color32::from_rgb(0xb3, 0xbb, 0xc8)),
        }
    }
}

/// A rounded status pill ("Ready to play").
pub fn pill(ui: &mut egui::Ui, text: &str, tone: Tone) -> egui::Response {
    let (bg, fg) = tone.colours();
    egui::Frame::new().fill(bg).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(9, 3)).show(ui, |ui| ui.label(RichText::new(text).size(12.0).strong().color(fg))).response
}

/// A raised card's frame (a start card, a checklist row, a stat).
pub fn card() -> egui::Frame {
    egui::Frame::new().fill(RAISED).stroke(Stroke::new(1.0, BORDER)).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(12))
}

/// A row in a list that can be selected: the whole width, a fill when on.
pub fn row(ui: &mut egui::Ui, on: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let b = egui::Button::new(text).fill(if on { SELECTED } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).min_size(egui::vec2(ui.available_width(), 28.0));
    ui.add(b)
}

/// A key name, drawn as a key cap, then what it does.
pub fn key_hint(ui: &mut egui::Ui, key: &str, does: &str) {
    egui::Frame::new().fill(Color32::from_rgb(0x23, 0x28, 0x33)).stroke(Stroke::new(1.0, Color32::from_rgb(0x30, 0x37, 0x47))).corner_radius(CornerRadius::same(4)).inner_margin(Margin::symmetric(6, 1)).show(ui, |ui| {
        ui.label(RichText::new(key).size(11.5).strong().color(TEXT));
    });
    ui.label(RichText::new(does).size(12.5).color(MUTED));
}

/// The editor's mark: a hexagon with a visor across it, drawn (no font has it).
pub fn logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
    let c = rect.center();
    let p = |x: f32, y: f32| c + egui::vec2(x, y);
    let hex = vec![p(-10.0, -4.0), p(0.0, -10.5), p(10.0, -4.0), p(10.0, 4.0), p(0.0, 10.5), p(-10.0, 4.0)];
    ui.painter().add(egui::Shape::closed_line(hex, Stroke::new(1.8, ACCENT)));
    ui.painter().add(egui::Shape::convex_polygon(vec![p(-5.0, -0.5), p(5.0, -0.5), p(3.0, 3.0), p(-3.0, 3.0)], ACCENT, Stroke::NONE));
}
