//! The developer panel's presentation options: how the match looks and sounds
//! on the way out (none of it changes the game). VIDEO is `n64::gpu::video`'s
//! N64 framebuffer, VI filters and CRT over the match view; AUDIO is
//! `n64::audio`'s N64 mix and TV speaker on the voices' DSP track.
//!
//! Source: the old repo's `pd_guns/app.rs` (`audio_panel`, `video_panel`).

use engine::egui;
use n64::audio::{AudioSettings, SpeakerPreset};
use n64::gpu::video::{Mask, Preset, Resolution, Signal, TvSet, VideoSettings};

/// The panel's VIDEO section: the N64's framebuffer and VI, then the tube.
pub fn video_panel(ui: &mut egui::Ui, v: &mut VideoSettings) {
    ui.label(egui::RichText::new("VIDEO").strong());
    ui.checkbox(&mut v.n64, "N64 video (the match)");
    ui.add_enabled_ui(v.n64, |ui| {
        egui::ComboBox::from_label("N64 resolution").selected_text(v.resolution.label()).show_ui(ui, |ui| {
            for r in Resolution::ALL {
                ui.selectable_value(&mut v.resolution, r, r.label());
            }
        });
        ui.indent("n64stages", |ui| {
            ui.checkbox(&mut v.three_point, "3-point texture filter");
            ui.checkbox(&mut v.fb16, "16-bit colour + Bayer dither");
            ui.add_enabled(v.fb16, egui::Checkbox::new(&mut v.dither_filter, "VI dither filter"));
            ui.checkbox(&mut v.aa, "VI anti-alias (depth edges)");
            ui.checkbox(&mut v.divot, "VI divot filter");
        });
        ui.checkbox(&mut v.crt, "CRT");
        ui.add_enabled_ui(v.crt, |ui| {
            ui.indent("crtopts", |ui| {
                ui.horizontal(|ui| {
                    for p in Preset::ALL {
                        if ui.small_button(p.label()).clicked() {
                            p.apply(v);
                        }
                    }
                });
                egui::ComboBox::from_label("signal").selected_text(v.signal.label()).show_ui(ui, |ui| {
                    for s in Signal::ALL {
                        ui.selectable_value(&mut v.signal, s, s.label());
                    }
                });
                egui::ComboBox::from_label("mask").selected_text(v.mask.label()).show_ui(ui, |ui| {
                    for m in Mask::ALL {
                        ui.selectable_value(&mut v.mask, m, m.label());
                    }
                });
                ui.add(egui::Slider::new(&mut v.mask_strength, 0.0..=1.0).text("mask strength"));
                egui::ComboBox::from_label("frame").selected_text(v.tv.label()).show_ui(ui, |ui| {
                    for t in TvSet::ALL {
                        ui.selectable_value(&mut v.tv, t, t.label());
                    }
                });
                ui.add(egui::Slider::new(&mut v.scanlines, 0.0..=1.0).text("scanlines"));
                ui.add(egui::Slider::new(&mut v.sharpness, 0.5..=2.5).text("signal sharpness"));
                ui.add(egui::Slider::new(&mut v.halation, 0.0..=0.3).text("halation"));
                ui.add(egui::Slider::new(&mut v.curvature, 0.0..=0.2).text("curvature"));
                ui.add(egui::Slider::new(&mut v.overscan, 0.0..=0.1).text("overscan"));
            });
        });
        if ui.small_button("reset video").clicked() {
            *v = VideoSettings { n64: true, ..VideoSettings::default() };
        }
    });
}

/// The panel's AUDIO section: the N64's 22020 Hz mix, and a cheap TV's speaker.
pub fn audio_panel(ui: &mut egui::Ui, a: &mut AudioSettings) {
    ui.label(egui::RichText::new("AUDIO").strong());
    ui.checkbox(&mut a.n64, "N64 mix (22020 Hz)");
    ui.add_enabled_ui(a.n64, |ui| {
        ui.indent("n64audio", |ui| {
            ui.checkbox(&mut a.dac_filter, "DAC filter (off = raw hold)");
        });
    });
    ui.checkbox(&mut a.tv, "TV speaker");
    ui.add_enabled_ui(a.tv, |ui| {
        ui.indent("tvspeaker", |ui| {
            ui.horizontal(|ui| {
                for p in SpeakerPreset::ALL {
                    if ui.small_button(p.label()).clicked() {
                        p.apply(a);
                    }
                }
            });
            ui.add(egui::Slider::new(&mut a.mix, 0.0..=1.0).text("mix"));
            ui.checkbox(&mut a.mono, "mono");
            ui.add(egui::Slider::new(&mut a.low_cut, 60.0..=800.0).logarithmic(true).text("low cut Hz"));
            ui.add(egui::Slider::new(&mut a.high_cut, 2000.0..=12000.0).logarithmic(true).text("high cut Hz"));
            ui.add(egui::Slider::new(&mut a.box_hz, 800.0..=4000.0).logarithmic(true).text("boxy mids Hz"));
            ui.add(egui::Slider::new(&mut a.box_db, 0.0..=12.0).text("boxy mids dB"));
            ui.add(egui::Slider::new(&mut a.drive, 0.0..=1.0).text("overdrive"));
            ui.add(egui::Slider::new(&mut a.squash, 0.0..=1.0).text("compressor"));
            ui.add(egui::Slider::new(&mut a.cabinet, 0.0..=1.0).text("cabinet"));
            ui.add(egui::Slider::new(&mut a.volume_db, -12.0..=12.0).text("volume dB"));
            ui.checkbox(&mut a.whine, "15.7 kHz flyback whine");
            ui.add_enabled(a.whine, egui::Slider::new(&mut a.whine_db, -70.0..=-20.0).text("whine dB"));
        });
    });
    if ui.small_button("reset audio").clicked() {
        *a = AudioSettings::default();
    }
}
