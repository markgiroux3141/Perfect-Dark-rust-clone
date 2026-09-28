//! `pd_lab [<stage code>]`: the simulant and route debug viewer. A match of
//! simulants only (no human), from above (`pd_tools::lab`): the floors by
//! height, the route graph, each simulant with its facing, go-to route, target
//! and distance mode. Pause, step and speed it up; restart with other counts,
//! difficulties, seeds or PD's graph against ours; click a simulant to follow
//! its brain. Scroll zooms, dragging pans.
//!
//! Source: the old repo's `pd_spike/viewer.rs` (the spike's egui viewer), on
//! a `pd_sim` world.

use std::sync::Arc;

use engine::app::{AppConfig, Ctx, Game};
use engine::assets::AssetRoot;
use engine::egui;
use engine::gpu::Frame;
use engine::wgpu;
use glam::Vec2;
use pd_core::assets::AssetDir;
use pd_sim::harness::{self, NavChoice};
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};
use pd_tools::lab::{self, LabOpts, Prim};

const DIFFS: [&str; 6] = ["Meat", "Easy", "Normal", "Hard", "Perfect", "Dark"];

struct Lab {
    stage: Arc<Stage>,
    level: Arc<TileLevel>,
    res: Arc<WorldRes>,
    world: World,
    bots: usize,
    diff: u8,
    nav: NavChoice,
    seed: u64,
    mix: bool,
    paused: bool,
    speed: u32,
    step_once: bool,
    opts: LabOpts,
    /// The view: the world x/z at the canvas centre, and pixels per cm.
    centre: Vec2,
    zoom: f32,
    error: Option<String>,
}

impl Lab {
    fn new(assets: &AssetDir, code: &str) -> Result<Lab, String> {
        let stage = Arc::new(Stage::load(assets, code)?);
        let level = Arc::new(TileLevel::new(stage.geom.clone()));
        let res = Arc::new(WorldRes::load(assets)?);
        let world = harness::world(stage.clone(), level.clone(), res.clone(), harness::setup(0, 4, 2), NavChoice::Pd, harness::SPIKE_SEED, true)?;
        let (lo, hi) = lab::bounds(&level);
        Ok(Lab {
            stage,
            level,
            res,
            world,
            bots: 4,
            diff: 2,
            nav: NavChoice::Pd,
            seed: harness::SPIKE_SEED,
            mix: true,
            paused: false,
            speed: 1,
            step_once: false,
            opts: LabOpts::default(),
            centre: (lo + hi) * 0.5,
            zoom: 0.12,
            error: None,
        })
    }

    fn restart(&mut self) {
        let setup = harness::setup(0, self.bots, self.diff);
        match harness::world(self.stage.clone(), self.level.clone(), self.res.clone(), setup, self.nav, self.seed, self.mix) {
            Ok(w) => {
                self.world = w;
                self.opts.selected = None;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn panel(&mut self, ui: &mut egui::Ui) {
        let w = &self.world;
        ui.heading("pd_lab");
        ui.label(format!("{} · frame {} · {:.1} s", self.stage.code, w.lv.lvframe60, w.lv.lvframe60 as f32 / 60.0));
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::YELLOW, e);
        }
        ui.horizontal(|ui| {
            if ui.button(if self.paused { "Run" } else { "Pause" }).clicked() {
                self.paused = !self.paused;
            }
            if ui.button("Step").clicked() {
                self.step_once = true;
                self.paused = true;
            }
        });
        ui.add(egui::Slider::new(&mut self.speed, 1..=32).text("ticks per frame"));
        ui.separator();
        ui.add(egui::Slider::new(&mut self.bots, 1..=8).text("simulants"));
        egui::ComboBox::from_label("difficulty").selected_text(DIFFS[self.diff as usize]).show_ui(ui, |ui| {
            for (k, name) in DIFFS.iter().enumerate() {
                ui.selectable_value(&mut self.diff, k as u8, *name);
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.nav, NavChoice::Pd, "PD's graph");
            ui.radio_value(&mut self.nav, NavChoice::Ours, "ours");
        });
        ui.checkbox(&mut self.mix, "the spike's weapon mix (else the set's)");
        let mut seed = format!("{:#x}", self.seed);
        if ui.add(egui::TextEdit::singleline(&mut seed).desired_width(160.0)).changed() {
            if let Ok(s) = u64::from_str_radix(seed.trim_start_matches("0x"), 16) {
                self.seed = s;
            }
        }
        if ui.button("Restart").clicked() {
            self.restart();
        }
        ui.separator();
        ui.checkbox(&mut self.opts.graph, "route graph");
        ui.checkbox(&mut self.opts.routes, "go-to routes");
        ui.checkbox(&mut self.opts.targets, "targets");
        ui.checkbox(&mut self.opts.names, "names");
        ui.add(egui::Slider::new(&mut self.opts.ymin, -500.0..=800.0).text("floors from y"));
        ui.add(egui::Slider::new(&mut self.opts.ymax, -500.0..=800.0).text("to y"));
        ui.separator();
        let w = &self.world;
        let s = &w.navstats;
        ui.label(format!("go-tos {} (no start {}, no end {}, no route {}) · re-paths {}", s.gotos, s.goto_no_start, s.goto_no_end, s.goto_no_route, s.repaths));
        ui.label(format!("rounds {} · hits {} ({:.0}%)", s.rounds, s.round_hits, 100.0 * s.round_hits as f32 / s.rounds.max(1) as f32));
        for (i, c) in w.chrs.iter().enumerate() {
            let a = c.aibot.as_ref().unwrap();
            let weapon = w.res.gset.weapon(a.weaponnum).map_or("unarmed".to_string(), |g| g.name.clone());
            let text = format!("{}: K{} D{} · {:?} · {}", c.name, c.kills, c.deaths, c.actiontype, weapon);
            if ui.selectable_label(self.opts.selected == Some(i), text).clicked() {
                self.opts.selected = Some(i);
            }
        }
        if let Some(i) = self.opts.selected {
            ui.separator();
            let c = &w.chrs[i];
            let a = c.aibot.as_ref().unwrap();
            ui.label(format!("pos ({:.0}, {:.0}, {:.0}) room {:?}", c.pos.x, c.manground, c.pos.z, c.floorroom));
            ui.label(format!("damage {:.1}/{:.0} · flinch {} · blur {}", c.damage, c.maxdamage, c.flinchcnt, c.blurdrugamount));
            ui.label(format!("target {:?} in sight {} · last seen {} · zero {:.1}°", c.target, a.targetinsight, a.targetlastseen60, a.zeroangle.to_degrees()));
            ui.label(format!("dist mode {:?} ({:.0} cm) ttl {}", a.distmode.map(|d| d.label()), a.last_dist, a.distmodettl60));
            ui.label(format!("ammo {:?} · reload {:?} · shoot delay {}", a.loadedammo, a.timeuntilreload60, a.shootdelaytimer60));
            ui.label(format!("anim {:#x} frame {:.1} speed {:.2} · choice {:?}", c.anim.animnum, c.anim.frame, c.anim.speed, a.last_choice));
            ui.label(format!("go-to: {} waypoints, at {} · age {}", c.act_gopos.waypoints.len(), c.act_gopos.curindex, c.act_gopos.age));
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;
        if resp.dragged() {
            let d = resp.drag_delta();
            self.centre -= Vec2::new(d.x, d.y) / self.zoom;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(0.01, 2.0);
            }
        }
        let (centre, zoom) = (self.centre, self.zoom);
        let to = |p: Vec2| egui::pos2(rect.center().x + (p.x - centre.x) * zoom, rect.center().y + (p.y - centre.y) * zoom);
        if resp.clicked() {
            if let Some(m) = resp.interact_pointer_pos() {
                let at = Vec2::new((m.x - rect.center().x) / zoom + centre.x, (m.y - rect.center().y) / zoom + centre.y);
                self.opts.selected = self.world.chrs.iter().enumerate().map(|(i, c)| (i, Vec2::new(c.pos.x, c.pos.z).distance(at))).filter(|&(_, d)| d < 80.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
            }
        }
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 22, 28));
        let col = |c: [u8; 4]| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
        for p in lab::picture(&self.world, &self.opts) {
            match p {
                Prim::Poly { pts, fill } => {
                    painter.add(egui::Shape::convex_polygon(pts.iter().map(|&q| to(q)).collect(), col(fill), egui::Stroke::NONE));
                }
                Prim::Line { a, b, col: c, width_px } => {
                    painter.line_segment([to(a), to(b)], egui::Stroke::new(width_px, col(c)));
                }
                Prim::Circle { c, r, col: k, filled } => {
                    let r = (r * zoom).max(2.0);
                    if filled {
                        painter.circle_filled(to(c), r, col(k));
                    } else {
                        painter.circle_stroke(to(c), r, egui::Stroke::new(1.5, col(k)));
                    }
                }
                Prim::Text { at, text, col: c } => {
                    painter.text(to(at), egui::Align2::LEFT_BOTTOM, text, egui::FontId::monospace(11.0), col(c));
                }
            }
        }
    }
}

impl Game for Lab {
    fn tick(&mut self, _ctx: &mut Ctx) {
        let n = if self.step_once {
            self.step_once = false;
            1
        } else if self.paused {
            0
        } else {
            self.speed
        };
        for _ in 0..n {
            harness::step_idle(&mut self.world);
        }
        self.world.take_events();
    }

    fn debug_ui(&mut self, _ctx: &mut Ctx, egui: &egui::Context) {
        egui::SidePanel::left("lab").resizable(true).default_width(340.0).show(egui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.panel(ui));
        });
        egui::CentralPanel::default().show(egui, |ui| self.canvas(ui));
    }

    fn render(&mut self, _ctx: &mut Ctx, frame: &mut Frame) {
        frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pd_lab"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,pd_lab=info")).init();
    let code = std::env::args().nth(1).unwrap_or_else(|| "ref".to_owned());
    let run = || -> Result<(), String> {
        let root = AssetRoot::discover("PD_ASSETS", "assets", "MANIFEST.json")?;
        let lab = Lab::new(&AssetDir::new(root.root()), &code)?;
        engine::app::run(AppConfig { title: "pd_lab".into(), size: (1600, 1000), tick_hz: 60.0, ..AppConfig::default() }, lab)
    };
    if let Err(e) = run() {
        eprintln!("pd_lab: {e}");
        std::process::exit(1);
    }
}
