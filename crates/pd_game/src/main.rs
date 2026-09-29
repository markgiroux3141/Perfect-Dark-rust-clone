//! `perfect_dark`: the game.
//!
//! A small state machine over the engine runner: Menus -> Match (the pause
//! menu and the end-of-match dialogs over it) -> Menus. It owns the glue and
//! nothing else:
//! - `controls`: keyboard, mouse and gamepads to N64 controllers (menus) and to
//!   each player's `PlayerInput` (a match);
//! - `audio`: `pd_menu`/`pd_sim` sound events to engine voices, with PD's pitch;
//! - `music`: PD's music (the synth and its queue) on an engine stream;
//! - `tvaudio`: the `n64::audio` N64 output + TV speaker chain on a DSP track;
//! - `presentation`: the panel's video and audio options;
//! - `states`: the menu and match states.
//!
//! `perfect_dark [--combat] [--fresh]`: start in the Combat Simulator rather than
//! the Perfect Menu; start on a new save file. F1 toggles the developer panel.

mod audio;
mod controls;
mod music;
mod presentation;
mod states;
mod tvaudio;

use engine::app::{AppConfig, Ctx, Game};
use engine::assets::AssetRoot;
use engine::egui;
use engine::gpu::{Filter, Frame, Presenter, RenderTarget};
use engine::input::{KeyCode, MouseButton};
use engine::wgpu;
use n64::pad::{A_BUTTON, MAX_PADS, START_BUTTON};
use n64::rdp::Gfx;
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::ids::stage_code;
use pd_core::lv::Lv;
use pd_core::mp::MatchSetup;
use pd_menu::mpstate::Profile;
use pd_menu::{MenuSystem, Outcome, FB_H, FB_W};
use pd_render::view::{VIEW_H, VIEW_W};
use pd_render::Renderer;
use pd_sim::world::World;

use n64::gpu::video::{tube_rect, Frame as VideoFrame, N64Video, VideoSettings};

use audio::SfxBank;
use controls::Controls;
use music::MusicPlayer;
use states::{MatchAssets, Screen};

/// The match view's format: not sRGB, the combiner writes display values.
const MATCH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// PD's frame rate: how many 60 Hz frames one PD frame lasts (`diffframe60`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rate {
    Hz60 = 1,
    Hz30 = 2,
    Hz20 = 3,
}

impl Rate {
    fn label(self) -> &'static str {
        match self {
            Rate::Hz60 => "60 Hz (PC port)",
            Rate::Hz30 => "30 Hz",
            Rate::Hz20 => "20 Hz (N64 in a busy frame)",
        }
    }
}

struct PdGame {
    assets: AssetDir,
    menu: MenuSystem,
    screen: Screen,
    /// The frame timing the menus run on (a match has its world's own).
    lv: Lv,
    rate: Rate,
    controls: Controls,
    sfx: Option<SfxBank>,
    music: Option<MusicPlayer>,
    /// The F1 panel's music switch and level (not PD's; PD's own volume is
    /// in the synth).
    music_on: bool,
    music_gain: f32,
    profile: Profile,
    /// The match stand-in's frame.
    stand_in: Gfx,
    target: Option<RenderTarget>,
    presenter: Option<Presenter>,
    show_panel: bool,
    /// Quantise the framebuffer to RGBA5551, as the N64's is.
    n64_colour: bool,
    match_assets: MatchAssets,
    renderer: Option<Renderer>,
    /// The stage the renderer has loaded.
    loaded_stage: Option<String>,
    /// The match view, `render_scale` × PD's 320 × 220 (or the N64 video's
    /// resolution), with depth.
    match_target: Option<RenderTarget>,
    render_scale: u32,
    /// The match's N64 video / CRT chain (off by default) and its passes.
    video: VideoSettings,
    n64v: Option<N64Video>,
    /// Seeds the next match's `random()`.
    next_seed: u64,
    /// The end screen's blurred backdrop is taken from the next match frame
    /// (`menu_set_background(MENUBG_BLUR)`'s screenshot).
    blur_pending: bool,
    /// A menu was open over the match last tick.
    menu_was_open: bool,
    /// The F1 panel's rumble switch (the Rumble Pak in the controllers).
    rumble: bool,
}

impl PdGame {
    fn new(assets: AssetDir, profile: Profile, combat: bool) -> Result<PdGame, String> {
        let mut menu = MenuSystem::new(&assets, profile)?;
        if combat {
            menu.open_combat_simulator();
        } else {
            menu.open_main_menu();
        }
        let sfx = SfxBank::load(&assets).map_err(|e| log::warn!("sfx: {e}; running silent")).ok();
        let music = MusicPlayer::load(&assets).map_err(|e| log::warn!("music: {e}; no music")).ok();
        let next_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Ok(PdGame {
            match_assets: MatchAssets::new(&assets),
            assets,
            menu,
            screen: Screen::Menus,
            lv: Lv::new(),
            rate: Rate::Hz60,
            controls: Controls::new(),
            sfx,
            music,
            music_on: true,
            music_gain: 1.0,
            profile,
            stand_in: Gfx::new(FB_W, FB_H),
            target: None,
            presenter: None,
            show_panel: true,
            n64_colour: true,
            renderer: None,
            loaded_stage: None,
            match_target: None,
            render_scale: 4,
            video: VideoSettings::default(),
            n64v: None,
            next_seed,
            blur_pending: false,
            menu_was_open: false,
            rumble: true,
        })
    }

    fn restart(&mut self) {
        match MenuSystem::new(&self.assets, self.profile) {
            Ok(mut m) => {
                if let Some(music) = &mut self.music {
                    music.stage_change();
                }
                m.open_main_menu();
                self.menu = m;
                self.screen = Screen::Menus;
            }
            Err(e) => log::error!("restart: {e}"),
        }
    }

    /// Controllers whose player is in the game (or waiting to join) stay plugged in.
    fn joined(&self) -> [bool; MAX_PADS] {
        std::array::from_fn(|i| self.menu.mp.setup.chrslots & (1 << i) != 0 || self.menu.vars.waitingtojoin[i])
    }

    /// Leave a match (or its stand-in) for the menus, as PD does once the end
    /// screens close (or at once, from the panel).
    fn end_match(&mut self, ctx: &mut Ctx) {
        if let Some(sfx) = &mut self.sfx {
            sfx.stop_all(ctx.audio.as_deref_mut());
        }
        // lv_stop's music_stop, then CI's lv_reset; the menus ask for their
        // tune as they come back (player_pause(MENUROOT_MPSETUP)).
        if let Some(music) = &mut self.music {
            music.stage_change();
        }
        if let Some(pads) = ctx.input.pads.as_mut() {
            for i in 0..MAX_PADS {
                pads.set_rumble(i, false);
            }
        }
        self.menu.return_from_match();
        self.screen = Screen::Menus;
        self.controls.captured = ctx.set_cursor_captured(false);
        if let Some(r) = self.renderer.as_mut() {
            r.menu_layer.clear();
        }
        self.blur_pending = false;
    }

    /// The menus handed over a match: play it if its arena is exported.
    fn start_match(&mut self, ctx: &mut Ctx, setup: MatchSetup) {
        let code = stage_code(setup.stagenum).unwrap_or("");
        if !self.match_assets.has_stage(code) {
            let lines = self.menu.describe_match(&setup);
            self.screen = Screen::StandIn { setup, lines };
            return;
        }
        let seed = self.next_seed;
        self.next_seed = self.next_seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        match self.load_match(ctx, &setup, code, seed) {
            Ok(world) => {
                log::info!("match: stage {code}, {} player(s), seed {seed:#x}", world.players.len());
                // CI's lv_stop: its music stops; the world's lv_reset starts the match's.
                if let Some(music) = &mut self.music {
                    music.stage_change();
                }
                self.screen = Screen::Match(Box::new(world));
                self.controls.captured = ctx.set_cursor_captured(true);
            }
            Err(e) => {
                log::error!("match: {e}");
                let mut lines = self.menu.describe_match(&setup);
                lines.push(format!("Could not start: {e}"));
                self.screen = Screen::StandIn { setup, lines };
            }
        }
    }

    fn load_match(&mut self, ctx: &mut Ctx, setup: &MatchSetup, code: &str, seed: u64) -> Result<World, String> {
        let renderer = self.renderer.as_mut().ok_or("no renderer")?;
        if self.loaded_stage.as_deref() != Some(code) {
            renderer.load_stage(&ctx.gpu.device, &ctx.gpu.queue, &self.assets, code)?;
            self.loaded_stage = Some(code.to_owned());
        }
        // The setup's weapon slots are the pads' (`World::new`).
        self.match_assets.start(setup.clone(), code, seed)
    }

    /// One tick of a match: the world, then the menus over it (the pause menu
    /// and the end-of-match dialogs, which `lv_tick` runs as `menu_tick`).
    /// Returns false when the match is over and the menus are to come back.
    fn tick_match(&mut self, ctx: &mut Ctx) -> bool {
        let Screen::Match(world) = &mut self.screen else { return false };
        // The mouse looks around only while no menu is open, and is taken
        // back when the pause menu closes.
        let menu_open = self.menu.menudata.count > 0;
        if menu_open {
            if self.controls.captured {
                self.controls.captured = ctx.set_cursor_captured(false);
            }
        } else if std::mem::take(&mut self.menu_was_open) && !world.mp.endscreen {
            self.controls.captured = ctx.set_cursor_captured(true);
        } else if ctx.input.key_pressed(KeyCode::Escape) {
            self.controls.captured = ctx.set_cursor_captured(false);
        } else if !self.controls.captured && ctx.input.mouse_pressed(MouseButton::Left) && !world.mp.endscreen {
            self.controls.captured = ctx.set_cursor_captured(true);
        }
        // The controllers as the menus read them (every tick, so a press the
        // match saw is not a new one to the menu it opens).
        self.menu_was_open = menu_open;
        let joined: [bool; MAX_PADS] = std::array::from_fn(|i| self.menu.mp.setup.chrslots & (1 << i) != 0);
        let (readings, back2) = self.controls.read(ctx.input, joined);
        for (pad, r) in self.menu.pads.iter_mut().zip(readings) {
            pad.next_frame(r.buttons, r.stick.0, r.stick.1);
            pad.connected = r.connected;
        }
        self.menu.back2 = back2;
        let mut inputs = self.controls.read_match(ctx.input, world.players.len());
        let kb = self.controls.kb_player;
        if let (Some(slot), Some(p)) = (self.controls.slot_pressed(ctx.input), world.players.get(kb)) {
            inputs[kb].select = p.gun.p.inventory.weapons().get(slot).copied();
        }
        let out = pd_game::session::step(world, &mut self.menu, &self.lv, 4 * self.rate as i32, &inputs);
        // The Rumble Pak: pad k is player k's controller.
        if let Some(pads) = ctx.input.pads.as_mut() {
            for pi in 0..world.players.len() {
                pads.set_rumble(pi, world.rumble_motor(pi) && self.rumble);
            }
        }
        for e in &out.events {
            if let Event::Kill { killer, victim } = *e {
                let name = |i: u8| world.chrs.get(i as usize).map_or("?".to_string(), |c| c.name.clone());
                match killer {
                    Some(k) if k != victim => log::info!("{} killed {}", name(k), name(victim)),
                    _ => log::info!("{} died", name(victim)),
                }
            }
        }
        if let Some(sfx) = &mut self.sfx {
            sfx.play(ctx.audio.as_deref_mut(), &out.events);
            sfx.play(ctx.audio.as_deref_mut(), &out.menu_events);
        }
        // The menus' calls came from menu_tick, before the world's lv_tick
        // (whose music_tick asks for the queue's tick).
        if let Some(music) = &mut self.music {
            music.apply(&out.menu_events);
            music.apply(&out.events);
        }
        self.blur_pending |= out.ended;
        // The menus' frame is laid over the HUD while they show anything (not
        // on the frame the end screen's blur is taken from).
        let showing = pd_game::session::menu_showing(&self.menu) && !self.blur_pending;
        if let Some(r) = self.renderer.as_mut() {
            r.menu_layer.clear();
            if showing {
                r.menu_layer.extend_from_slice(&self.menu.draw.gfx.fb);
            }
        }
        !out.over
    }

    /// Keep the music's stream ahead of the device, on the TV track while
    /// the N64 / TV chain is on.
    fn pump_music(&mut self, ctx: &mut Ctx) {
        let dt = self.rate as i32 as f64 / 60.0;
        let Some(music) = &mut self.music else { return };
        let track = match (ctx.audio.as_deref_mut(), &mut self.sfx) {
            (Some(a), Some(sfx)) => sfx.track(a),
            _ => None,
        };
        music.pump(ctx.audio.as_deref_mut(), track, dt);
        music.set_gain(if self.music_on { self.music_gain } else { 0.0 });
    }

    fn ensure_match_target(&mut self, ctx: &mut Ctx) {
        // N64 video: the view renders at the N64's resolution, whatever the window.
        let (w, h) = if self.video.active() { self.video.resolution.size() } else { (VIEW_W * self.render_scale, VIEW_H * self.render_scale) };
        if self.match_target.as_ref().is_none_or(|t| t.width != w || t.height != h) {
            self.match_target = Some(RenderTarget::new(ctx.gpu, w, h, MATCH_FORMAT, true));
        }
    }

    fn match_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let Screen::Match(world) = &self.screen else { return false };
        ui.separator();
        ui.label(format!("MATCH on {} · {} player(s) · frame {}", world.stage.code, world.players.len(), world.lv.lvframenum));
        for (i, p) in world.players.iter().enumerate() {
            ui.label(format!("P{} feet ({:.0}, {:.0}, {:.0}) θ {:.1}° pitch {:.1}° room {:?}", i + 1, p.pos.x, p.manground, p.pos.z, p.theta, p.verta, p.floorroom));
            let crouch = ["squat", "duck", "stand"].get(p.crouchpos as usize).copied().unwrap_or("?");
            let state = if p.onladder {
                " · ladder"
            } else if p.isfalling {
                " · falling"
            } else {
                ""
            };
            ui.label(format!("   speed fwd {:.2} side {:.2} · {crouch}{state}", p.speedforwards, p.speedsideways));
            ui.label(format!("   health {:.2}{}", p.bondhealth, if p.isdead { " · dead (fire/A to respawn once black)" } else { "" }));
        }
        for (i, c) in world.chrs.iter().enumerate() {
            let Some(a) = c.aibot.as_ref() else {
                ui.label(format!("{}: K{} D{}", c.name, world.mp_chr_kills(i), world.mp_chr_deaths(i)));
                continue;
            };
            let weapon = world.res.gset.weapon(a.weaponnum).map_or("unarmed".to_string(), |w| w.name.clone());
            ui.label(format!(
                "{}: K{} D{} · {:?} · dmg {:.1}/{:.0} · {} · target {:?} {}",
                c.name,
                world.mp_chr_kills(i),
                world.mp_chr_deaths(i),
                c.actiontype,
                c.damage,
                c.maxdamage,
                weapon,
                c.target,
                a.distmode.map_or("", |d| d.label())
            ));
        }
        let m = &world.mp;
        let limit = |v: i32| if v > 0 { v.to_string() } else { "none".into() };
        ui.label(format!(
            "time {} / {} · score limit {} · team limit {} · {}",
            pd_core::text::format_time(m.stagetime60, pd_core::text::TIMEPRECISION_SECONDS),
            if m.timelimit60 > 0 { pd_core::text::format_time(m.timelimit60, pd_core::text::TIMEPRECISION_SECONDS) } else { "no limit".into() },
            limit(m.scorelimit),
            limit(m.teamscorelimit),
            if m.endscreen { "over" } else if world.mp_is_paused() { "paused" } else { "playing" }
        ));
        ui.label(
            egui::RichText::new("WASD move · mouse look (click to capture, Esc frees it) · LMB fire · RMB aim · E/MMB use (hold: gun function) · R reload · Q next gun · 1-0 pick a gun · ↑/↓ zoom · Ctrl/C crouch down · Space crouch up · Enter or pad START: the pause menu (arrows, Enter A, Esc B, Space START)")
                .weak(),
        );
        let mut scale = self.render_scale;
        ui.add_enabled_ui(!self.video.active(), |ui| egui::ComboBox::from_label("resolution").selected_text(format!("{}×{}", VIEW_W * scale, VIEW_H * scale)).show_ui(ui, |ui| {
            for s in [1, 2, 3, 4] {
                ui.selectable_value(&mut scale, s, format!("{}×{}{}", VIEW_W * s, VIEW_H * s, if s == 1 { " (N64)" } else { "" }));
            }
        }));
        self.render_scale = scale;
        ui.button("End the match (PD's results, then the menus)").clicked()
    }
}

impl Game for PdGame {
    fn init(&mut self, ctx: &mut Ctx) {
        self.target = Some(RenderTarget::new(ctx.gpu, FB_W as u32, FB_H as u32, wgpu::TextureFormat::Rgba8Unorm, false));
        self.presenter = Some(Presenter::new(ctx.gpu));
        self.renderer = Some(Renderer::new(&ctx.gpu.device, &ctx.gpu.queue, MATCH_FORMAT));
        self.n64v = Some(N64Video::new(&ctx.gpu.device, &ctx.gpu.queue, ctx.gpu.config.format));
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        if ctx.input.key_pressed(KeyCode::F1) {
            self.show_panel = !self.show_panel;
        }
        let n = self.rate as i32;
        self.lv.frametime_apply(n, 4 * n);
        if matches!(self.screen, Screen::Match(_)) {
            if !self.tick_match(ctx) {
                self.end_match(ctx);
            }
            self.pump_music(ctx);
            return;
        }
        let (readings, back2) = self.controls.read(ctx.input, self.joined());
        for (pad, r) in self.menu.pads.iter_mut().zip(readings) {
            pad.next_frame(r.buttons, r.stick.0, r.stick.1);
            pad.connected = r.connected;
        }
        self.menu.back2 = back2;
        match &self.screen {
            Screen::Menus => {
                self.menu.frame(&self.lv);
                if let Some(Outcome::StartMatch(setup)) = self.menu.take_outcome() {
                    self.start_match(ctx, setup);
                }
            }
            Screen::StandIn { .. } => {
                if self.menu.pads.iter().any(|p| p.pressed(START_BUTTON | A_BUTTON) != 0) {
                    self.end_match(ctx);
                }
            }
            Screen::Match(_) => {}
        }
        let events = self.menu.take_events();
        if let Some(sfx) = &mut self.sfx {
            sfx.play(ctx.audio.as_deref_mut(), &events);
        }
        if let Some(music) = &mut self.music {
            music.apply(&events);
            music.tick(self.lv.diffframe240);
        }
        self.pump_music(ctx);
    }

    fn debug_ui(&mut self, ctx: &mut Ctx, egui: &egui::Context) {
        if !self.show_panel {
            return;
        }
        let pads = ctx.input.pads.as_ref().map_or(0, |p| p.count());
        let fps = ctx.clock.fps();
        let dialog = self.menu.menus[0].curdialog.map(|d| self.menu.menus[0].dialogs[d].def().name).unwrap_or("-");
        let (mut rate, mut profile, mut kb) = (self.rate, self.profile, self.controls.kb_player);
        let mut restart = false;
        let mut end = false;
        egui::SidePanel::left("pd").resizable(false).default_width(250.0).show(egui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("PERFECT DARK");
                ui.label(egui::RichText::new("Combat Simulator, from the decomp").weak());
                ui.label(format!("{fps:.0} fps · {pads} pad(s)"));
                ui.label(format!("root {} · {dialog}", self.menu.menudata.root));
                if let Screen::StandIn { setup, .. } = &self.screen {
                    ui.label(format!("match: stage {:#x}, {} player(s), {} sim(s)", setup.stagenum, setup.players.len(), setup.simulants.len()));
                }
                if let Some(e) = &self.menu.draw.error {
                    ui.colored_label(egui::Color32::YELLOW, e);
                }
                end = self.match_panel(ui);
                ui.separator();
                ui.label("Menus: arrows/WASD D-pad · Enter A · Esc B · Space START · Z Z · Q/E L/R · Backspace delete · F1 panel · F2-F4 START on 2-4");
                ui.separator();
                egui::ComboBox::from_label("frame rate").selected_text(rate.label()).show_ui(ui, |ui| {
                    for r in [Rate::Hz60, Rate::Hz30, Rate::Hz20] {
                        ui.selectable_value(&mut rate, r, r.label());
                    }
                });
                ui.checkbox(&mut self.n64_colour, "RGBA5551 framebuffer (menus)");
                ui.checkbox(&mut self.rumble, "Rumble Pak (pads with force feedback)");
                ui.separator();
                ui.label("Save file (unlocks)");
                ui.radio_value(&mut profile, Profile::Complete, "Complete (everything unlocked)");
                ui.radio_value(&mut profile, Profile::Fresh, "Fresh (new file)");
                if ui.button("Restart at the Perfect Menu").clicked() {
                    restart = true;
                }
                ui.separator();
                ui.label("Players");
                egui::ComboBox::from_label("keyboard drives").selected_text(format!("controller {}", kb + 1)).show_ui(ui, |ui| {
                    for i in 0..MAX_PADS {
                        ui.selectable_value(&mut kb, i, format!("controller {}", i + 1));
                    }
                });
                ui.horizontal(|ui| {
                    for i in 1..MAX_PADS {
                        if ui.button(format!("START on {}", i + 1)).clicked() {
                            self.controls.start_taps[i] = true;
                        }
                    }
                });
                ui.label(egui::RichText::new("A second player joins the Combat Simulator by pressing START; then set 'keyboard drives' to that controller.").weak());
                ui.separator();
                presentation::video_panel(ui, &mut self.video);
                if let Some(music) = &self.music {
                    ui.separator();
                    ui.label("MUSIC");
                    ui.checkbox(&mut self.music_on, "Music");
                    ui.add(egui::Slider::new(&mut self.music_gain, 0.0..=2.0).text("music level (1 = PD's mix)"));
                    let m = &music.music;
                    let names = &m.data().names;
                    for (i, c) in m.channels.iter().enumerate() {
                        if c.tracktype != 0 {
                            let t = m.tracknum(i);
                            let kind = ["-", "primary", "X", "menu", "death", "ambient", "?"].get(c.tracktype as usize).copied().unwrap_or("?");
                            let name = names.get(t as usize).map_or("?", |n| n.trim_start_matches("MUSIC_"));
                            let fade = m.audio.players[i].chan_state[0].fadevolcurrent;
                            ui.label(egui::RichText::new(format!("seq {i}: {kind} {name} · fade {fade}{}", if c.inuse { "" } else { " · releasing" })).weak());
                        }
                    }
                }
                if let Some(sfx) = &mut self.sfx {
                    ui.separator();
                    let mut a = sfx.tv();
                    presentation::audio_panel(ui, &mut a);
                    if a != sfx.tv() {
                        sfx.set_tv(a);
                    }
                }
            });
        });
        if rate != self.rate {
            self.rate = rate;
            ctx.clock.set_tick_hz(60.0 / rate as i32 as f64);
        }
        self.controls.kb_player = kb;
        if profile != self.profile {
            self.profile = profile;
            self.menu.set_profile(profile);
        }
        if restart {
            self.restart();
        }
        if end {
            // In a match, main_end_stage: the results, then the menus; the
            // stand-in has none.
            match &mut self.screen {
                Screen::Match(world) => world.main_end_stage(),
                _ => self.end_match(ctx),
            }
        }
    }

    fn render(&mut self, ctx: &mut Ctx, frame: &mut Frame) {
        if matches!(self.screen, Screen::Match(_)) {
            self.ensure_match_target(ctx);
            let (Screen::Match(world), Some(target), Some(presenter), Some(renderer), Some(nv)) = (&self.screen, &self.match_target, &self.presenter, &mut self.renderer, &mut self.n64v) else { return };
            let video = self.video;
            let n64 = video.active();
            let (device, queue) = (&ctx.gpu.device, &ctx.gpu.queue);
            let single = world.players.len() == 1;
            renderer.three_point = n64 && video.three_point;
            renderer.hud_in_frame = !n64;
            // SUBST: the VI's anti-aliasing reads the coverage PD's single
            // framebuffer keeps / its depth estimate needs one view's depth,
            // so split screen goes through the chain without it.
            renderer.world_depth_copy = (n64 && video.aa && single).then(|| nv.world_depth(device, target.width, target.height));
            renderer.render_views(device, queue, &mut frame.encoder, target, world);
            if std::mem::take(&mut self.blur_pending) {
                // menugfx_create_blur's screenshot: this frame, as it stands.
                let enc = std::mem::replace(&mut frame.encoder, device.create_command_encoder(&Default::default()));
                queue.submit(Some(enc.finish()));
                let px = target.read_rgba8(device, queue);
                self.menu.draw.res.blur_from_image(Some((target.width as usize, target.height as usize, &px)));
            }
            if n64 {
                // SUBST: PD draws lv_render's framebuffer effects over the HUD /
                // the chain lays the HUD on after them, in its RDP pass.
                let hud = &renderer.hud_frame;
                nv.upload_hud(device, queue, Some((&pd_render::hud::premultiplied_rgba8(hud), hud.w as u32, hud.h as u32)));
                let (znear, zfar) = renderer.z_range();
                let depth = target.depth.as_ref().filter(|_| single).map(|d| &d.1);
                let vf = VideoFrame { color: &target.view, color_srgb: false, size: (target.width, target.height), gun_depth: depth, world_near_far: (znear, zfar), gun_near_far: (1.5, 1000.0) };
                clear(&mut frame.encoder, &frame.view);
                let rect = tube_rect(frame.size.0, frame.size.1, frame.viewport[0]);
                nv.run(device, queue, &mut frame.encoder, &vf, &frame.view, rect, &video);
                return;
            }
            // The VI shows PD's 220 lines inside a 240-line, 4:3 picture.
            presenter.present(ctx.gpu, frame, target, (VIEW_W * self.render_scale, 240 * self.render_scale), Filter::Nearest);
            return;
        }
        let (Some(target), Some(presenter)) = (&self.target, &self.presenter) else { return };
        let gfx = match &self.screen {
            Screen::StandIn { lines, .. } => {
                states::draw_match_stand_in(&mut self.stand_in, &self.menu.draw.res.fonts, lines);
                &self.stand_in
            }
            _ => &self.menu.draw.gfx,
        };
        target.upload(ctx.gpu, &gfx.rgba8(self.n64_colour));
        // The VI shows PD's 220 lines inside a 240-line, 4:3 picture.
        presenter.present(ctx.gpu, frame, target, (320, 240), Filter::Nearest);
    }
}

/// Clear the window before a pass that draws only part of it.
fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,perfect_dark=info,pd_menu=info,engine=info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let profile = if args.iter().any(|a| a == "--fresh") { Profile::Fresh } else { Profile::Complete };
    let combat = args.iter().any(|a| a == "--combat");
    let run = || -> Result<(), String> {
        let root = AssetRoot::discover("PD_ASSETS", "assets", "MANIFEST.json")?;
        log::info!("assets: {}", root.root().display());
        let game = PdGame::new(AssetDir::new(root.root()), profile, combat)?;
        let config = AppConfig { title: "Perfect Dark".into(), size: (1280, 960), tick_hz: 60.0, ..AppConfig::default() };
        engine::app::run(config, game)
    };
    if let Err(e) = run() {
        log::error!("perfect_dark: {e}");
        eprintln!("perfect_dark: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything `main` builds before the window: the asset root found from the
    /// test binary under `target/`, the menus on the Perfect Menu, the sounds.
    #[test]
    fn the_game_starts_from_the_repo_assets() {
        let root = AssetRoot::discover("PD_ASSETS", "assets", "MANIFEST.json").unwrap();
        let mut game = PdGame::new(AssetDir::new(root.root()), Profile::Complete, false).unwrap();
        assert!(game.sfx.is_some());
        let mut lv = Lv::new();
        for _ in 0..30 {
            lv.frametime_apply(1, 4);
            game.menu.frame(&lv);
        }
        assert_eq!(game.menu.menus[0].curdialog.map(|d| game.menu.menus[0].dialogs[d].def().name), Some("g_CiMenuViaPcMenuDialog"));
        assert!(game.menu.draw.error.is_none(), "{:?}", game.menu.draw.error);
        let mut stand_in = Gfx::new(FB_W, FB_H);
        states::draw_match_stand_in(&mut stand_in, &game.menu.draw.res.fonts, &["Arena: Skedar".into()]);
        assert!(stand_in.rgba8(false).chunks(4).any(|p| p[0] > 200 && p[1] > 200 && p[2] < 50), "the yellow title");
        if let Some(out) = std::env::var_os("PD_STAND_IN_PNG") {
            let m = MatchSetup { players: vec![pd_core::mp::MatchPlayer { slot: 0, chr: pd_core::mp::MatchChr { name: "Player 1".into(), ..Default::default() }, handicap: 128, ..Default::default() }], ..Default::default() };
            states::draw_match_stand_in(&mut stand_in, &game.menu.draw.res.fonts, &game.menu.describe_match(&m));
            image::save_buffer(out, &stand_in.rgba8(true), FB_W as u32, FB_H as u32, image::ColorType::Rgba8).unwrap();
        }
    }

    /// A match on every arena, as the menus would start it: the stage is
    /// exported, the world starts, and a second of walking moves the player.
    #[test]
    fn every_arena_starts_and_the_player_walks() {
        let root = AssetRoot::discover("PD_ASSETS", "assets", "MANIFEST.json").unwrap();
        let assets = AssetDir::new(root.root());
        let mut ma = MatchAssets::new(&assets);
        for code in pd_sim::stage::ARENAS {
            assert!(ma.has_stage(code), "{code}");
            let stagenum = (0..=255u8).find(|&n| stage_code(n) == Some(code)).unwrap();
            let setup = MatchSetup { stagenum, players: vec![pd_core::mp::MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
            let mut world = ma.start(setup, code, 3).unwrap();
            let start = world.players[0].pos;
            let fwd = pd_sim::player::PlayerInput { walk_y: 127, ..Default::default() };
            for _ in 0..60 {
                world.step(4, std::slice::from_ref(&fwd));
            }
            assert!(world.players[0].pos.distance(start) > 50.0, "{code}: walked from {start} to {}", world.players[0].pos);
        }
    }
}
