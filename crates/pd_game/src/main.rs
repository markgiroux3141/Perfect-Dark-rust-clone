//! `perfect_dark`: the game.
//!
//! A small state machine over the engine runner: Menus -> Match -> (pause menu,
//! end-of-match scores) -> Menus. It owns the glue and nothing else:
//! - `controls`: keyboard and gamepads to N64 controllers (later the PC port's
//!   mouse aim);
//! - `audio`: `pd_menu`/`pd_sim` sound events to engine voices, with PD's pitch;
//! - `states`: the menu and match states.
//!
//! `perfect_dark [--combat] [--fresh]`: start in the Combat Simulator rather than
//! the Perfect Menu; start on a new save file. F1 toggles the developer panel.

mod audio;
mod controls;
mod states;

use engine::app::{AppConfig, Ctx, Game};
use engine::assets::AssetRoot;
use engine::egui;
use engine::gpu::{Filter, Frame, Presenter, RenderTarget};
use engine::input::KeyCode;
use engine::wgpu;
use n64::pad::{A_BUTTON, MAX_PADS, START_BUTTON};
use n64::rdp::Gfx;
use pd_core::assets::AssetDir;
use pd_core::lv::Lv;
use pd_menu::mpstate::Profile;
use pd_menu::{MenuSystem, Outcome, FB_H, FB_W};

use audio::SfxBank;
use controls::Controls;
use states::Screen;

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
    /// The frame timing the menus run on.
    lv: Lv,
    rate: Rate,
    controls: Controls,
    sfx: Option<SfxBank>,
    profile: Profile,
    /// The match stand-in's frame.
    stand_in: Gfx,
    target: Option<RenderTarget>,
    presenter: Option<Presenter>,
    show_panel: bool,
    /// Quantise the framebuffer to RGBA5551, as the N64's is.
    n64_colour: bool,
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
        Ok(PdGame {
            assets,
            menu,
            screen: Screen::Menus,
            lv: Lv::new(),
            rate: Rate::Hz60,
            controls: Controls::new(),
            sfx,
            profile,
            stand_in: Gfx::new(FB_W, FB_H),
            target: None,
            presenter: None,
            show_panel: true,
            n64_colour: true,
        })
    }

    fn restart(&mut self) {
        match MenuSystem::new(&self.assets, self.profile) {
            Ok(mut m) => {
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
}

impl Game for PdGame {
    fn init(&mut self, ctx: &mut Ctx) {
        self.target = Some(RenderTarget::new(ctx.gpu, FB_W as u32, FB_H as u32, wgpu::TextureFormat::Rgba8Unorm, false));
        self.presenter = Some(Presenter::new(ctx.gpu));
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        if ctx.input.key_pressed(KeyCode::F1) {
            self.show_panel = !self.show_panel;
        }
        let (readings, back2) = self.controls.read(ctx.input, self.joined());
        for (pad, r) in self.menu.pads.iter_mut().zip(readings) {
            pad.next_frame(r.buttons, r.stick.0, r.stick.1);
            pad.connected = r.connected;
        }
        self.menu.back2 = back2;
        let n = self.rate as i32;
        self.lv.frametime_apply(n, 4 * n);
        match &self.screen {
            Screen::Menus => {
                self.menu.frame(&self.lv);
                if let Some(Outcome::StartMatch(setup)) = self.menu.take_outcome() {
                    let lines = self.menu.describe_match(&setup);
                    self.screen = Screen::Match { setup, lines };
                }
            }
            Screen::Match { .. } => {
                if self.menu.pads.iter().any(|p| p.pressed(START_BUTTON | A_BUTTON) != 0) {
                    self.menu.return_from_match();
                    self.screen = Screen::Menus;
                }
            }
        }
        let events = self.menu.take_events();
        if let Some(sfx) = &self.sfx {
            sfx.play(ctx.audio.as_deref_mut(), &events);
        }
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
        egui::SidePanel::left("pd").resizable(false).default_width(250.0).show(egui, |ui| {
            ui.heading("PERFECT DARK");
            ui.label(egui::RichText::new("Combat Simulator, from the decomp").weak());
            ui.label(format!("{fps:.0} fps · {pads} pad(s)"));
            ui.label(format!("root {} · {dialog}", self.menu.menudata.root));
            if let Screen::Match { setup, .. } = &self.screen {
                ui.label(format!("match: stage {:#x}, {} player(s), {} sim(s)", setup.stagenum, setup.players.len(), setup.simulants.len()));
            }
            if let Some(e) = &self.menu.draw.error {
                ui.colored_label(egui::Color32::YELLOW, e);
            }
            ui.separator();
            ui.label("Keyboard: arrows/WASD D-pad · Enter A · Esc B · Space START · Z Z · Q/E L/R · Backspace delete · F1 panel · F2-F4 START on 2-4");
            ui.separator();
            egui::ComboBox::from_label("frame rate").selected_text(rate.label()).show_ui(ui, |ui| {
                for r in [Rate::Hz60, Rate::Hz30, Rate::Hz20] {
                    ui.selectable_value(&mut rate, r, r.label());
                }
            });
            ui.checkbox(&mut self.n64_colour, "RGBA5551 framebuffer");
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
    }

    fn render(&mut self, ctx: &mut Ctx, frame: &mut Frame) {
        let (Some(target), Some(presenter)) = (&self.target, &self.presenter) else { return };
        let gfx = match &self.screen {
            Screen::Menus => &self.menu.draw.gfx,
            Screen::Match { lines, .. } => {
                states::draw_match_stand_in(&mut self.stand_in, &self.menu.draw.res.fonts, lines);
                &self.stand_in
            }
        };
        target.upload(ctx.gpu, &gfx.rgba8(self.n64_colour));
        // The VI shows PD's 220 lines inside a 240-line, 4:3 picture.
        presenter.present(ctx.gpu, frame, target, (320, 240), Filter::Nearest);
    }
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
            let m = pd_core::mp::MatchSetup { players: vec![pd_core::mp::MatchPlayer { slot: 0, chr: pd_core::mp::MatchChr { name: "Player 1".into(), ..Default::default() }, handicap: 128, ..Default::default() }], ..Default::default() };
            states::draw_match_stand_in(&mut stand_in, &game.menu.draw.res.fonts, &game.menu.describe_match(&m));
            image::save_buffer(out, &stand_in.rgba8(true), FB_W as u32, FB_H as u32, image::ColorType::Rgba8).unwrap();
        }
    }
}
