//! `pd_snapshot <outdir> flow [--score n] [--minutes m] [--teams] [--seed s]
//! [--size WxH] <steps...>`: a Combat match on Complex with the menus over it, as the game
//! plays it (`pd_game::session`), under a scripted N64 controller 1 that
//! drives both the match and the menus.
//!
//! The match is started through the menus' own `mp_start_match` from a setup
//! with player 1 and one simulant ("Sim 1") and a score limit of `--score`
//! points (default 1; `--minutes` a time limit, default none; `--teams`: teams
//! on, the player Red and the simulant Yellow). The simulant's
//! brain is off and it stands 4 m in front of the player (not PD: the
//! snapshot's arrangement, as `match --duel`).
//!
//! Steps (the menu script's words, `pd_menu::script`, plus the match's):
//! * `w<N>`: N frames with nothing held;
//! * `a` `b` `z` `start` `up` `down` `left` `right` `l` `r` `cu` `cd` `cl` `cr`:
//!   tap that button (one frame down, then 6 frames up); the match reads A, B,
//!   Z, START, R and the C buttons, the menus all of them;
//! * `kill`: tap the trigger (Z, 6 frames down, 6 up) until the simulant dies
//!   (at most 12 s);
//! * `wend`: run until the end screens are up (at most 20 s), then 30 frames;
//! * `shot:<name>`: the frame as `flow_<name>.png`: the player's view with the
//!   menus over it, or once the end screens have closed, the menus alone.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use glam::Vec3;
use n64::pad::*;
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_menu::mpstate::Profile;
use pd_menu::{MenuSystem, Outcome};
use pd_render::Renderer;
use pd_sim::harness;
use pd_sim::player::PlayerInput;
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

/// Controller 1's held buttons as the match reads them (control style 1.1).
fn match_input(held: u16) -> PlayerInput {
    let b = |bit: u16| held & bit != 0;
    PlayerInput { pad: true, fire: b(Z_TRIG), aim: b(R_TRIG), use_held: b(B_BUTTON), a_held: b(A_BUTTON), start: b(START_BUTTON), c_up: b(U_CBUTTONS), c_down: b(D_CBUTTONS), c_left: b(L_CBUTTONS), c_right: b(R_CBUTTONS), ..PlayerInput::default() }
}

fn button(word: &str) -> Option<u16> {
    Some(match word {
        "a" => A_BUTTON,
        "b" => B_BUTTON,
        "z" => Z_TRIG,
        "start" => START_BUTTON,
        "up" => U_JPAD,
        "down" => D_JPAD,
        "left" => L_JPAD,
        "right" => R_JPAD,
        "l" => L_TRIG,
        "r" => R_TRIG,
        "cu" => U_CBUTTONS,
        "cd" => D_CBUTTONS,
        "cl" => L_CBUTTONS,
        "cr" => R_CBUTTONS,
        _ => return None,
    })
}

struct Flow {
    world: World,
    menu: MenuSystem,
    lv: Lv,
    gpu: HeadlessGpu,
    renderer: Renderer,
    target: RenderTarget,
    over: bool,
}

impl Flow {
    /// One 60 Hz frame with `held` on controller 1: the match and the menus
    /// (`pd_game::session::step`), and the end screen's blur when it ends.
    fn frame(&mut self, held: u16) {
        if self.over {
            self.menu.pads[0].next_frame(held, 0, 0);
            self.lv.frametime_apply(1, 4);
            self.menu.frame(&self.lv);
            return;
        }
        self.menu.pads[0].next_frame(held, 0, 0);
        self.menu.pads[0].connected = true;
        self.lv.frametime_apply(1, 4);
        let out = pd_game::session::step(&mut self.world, &mut self.menu, &self.lv, 4, &[match_input(held)]);
        if out.ended {
            // The blur behind the end screens: this frame without the menus.
            self.renderer.menu_layer.clear();
            let px = self.render();
            self.menu.draw.res.blur_from_image(Some((self.target.width as usize, self.target.height as usize, &px)));
        }
        if out.over {
            // Back to the Combat Simulator (the game's end_match).
            self.menu.return_from_match();
            self.over = true;
        }
    }

    fn render(&mut self) -> Vec<u8> {
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("flow") });
        self.renderer.render_player(&self.gpu.device, &self.gpu.queue, &mut enc, &self.target, &self.world, 0);
        self.gpu.queue.submit(Some(enc.finish()));
        self.target.read_rgba8(&self.gpu.device, &self.gpu.queue)
    }

    fn shot(&mut self, path: PathBuf) -> Result<PathBuf, String> {
        if self.over {
            let g = &self.menu.draw.gfx;
            crate::write_png(&path, g.w, g.h, &g.rgba8(true))?;
            return Ok(path);
        }
        self.renderer.menu_layer.clear();
        if pd_game::session::menu_showing(&self.menu) {
            self.renderer.menu_layer.extend_from_slice(&self.menu.draw.gfx.fb);
        }
        let px = self.render();
        crate::write_png(&path, self.target.width as usize, self.target.height as usize, &px)?;
        Ok(path)
    }
}

pub fn run(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let (mut w, mut h) = (640u32, 440u32);
    let (mut score, mut minutes, mut seed, mut teams) = (1u8, None::<u8>, harness::SPIKE_SEED, false);
    let mut words: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--size" => {
                let v = val("--size")?;
                let (a, b) = v.split_once('x').ok_or("--size is WxH")?;
                w = a.parse().map_err(|e| format!("--size: {e}"))?;
                h = b.parse().map_err(|e| format!("--size: {e}"))?;
            }
            "--score" => score = val("--score")?.parse().map_err(|e| format!("--score: {e}"))?,
            "--minutes" => minutes = Some(val("--minutes")?.parse().map_err(|e| format!("--minutes: {e}"))?),
            "--seed" => seed = val("--seed")?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--teams" => teams = true,
            s => words.push(s.to_owned()),
        }
    }
    let assets = crate::assets();
    // The menus' setup: player 1 and "Sim 1" on Complex, then mp_start_match.
    let mut menu = MenuSystem::new(&assets, Profile::Complete)?;
    menu.open_combat_simulator();
    {
        let mp = &mut menu.mp;
        mp.setup.stagenum = STAGE_MP_COMPLEX;
        mp.setup.scorelimit = score.saturating_sub(1).min(100);
        mp.setup.timelimit = minutes.map_or(60, |m| m.saturating_sub(1).min(60));
        mp.setup.chrslots = 0b1_0001;
        mp.bots[0].base.name = "Sim 1\n".into();
        mp.bots[0].difficulty = BOTDIFF_NORMAL;
        if teams {
            mp.setup.options |= MPOPTION_TEAMSENABLED;
            mp.players[0].base.team = 0;
            mp.bots[0].base.team = 1;
        }
    }
    menu.start_match();
    let Some(Outcome::StartMatch(setup)) = menu.take_outcome() else { return Err("the menus started no match".into()) };
    let stage = Arc::new(Stage::load(&assets, "ref")?);
    let level = Arc::new(TileLevel::new(stage.geom.clone()));
    let res = Arc::new(WorldRes::load(&assets)?);
    let weapons = MenuSystem::weapon_set_weaponnums(&setup.weapons);
    let mut world = World::new(setup, stage.clone(), level.clone(), res, seed)?;
    // Not PD: the player starts with the set's guns (the harness loadout) so
    // `kill` can shoot at once; the pads hold them too.
    world.harness_give_loadout(weapons);
    world.bot_brains = false;
    // A spawn pad with 4 m of level floor ahead: the player there, the
    // simulant at the far end (as `match --duel`).
    let ground = |p: Vec3| Vec3::new(p.x, level.cd_find_ground_at_cyl(p, 30.0).0, p.z);
    let (a, b) = stage
        .spawn_pads
        .iter()
        .find_map(|&p| {
            let pad = &stage.pads[p];
            let a = ground(pad.pos);
            let d = Vec3::new(pad.look.x, 0.0, pad.look.z).normalize_or_zero();
            let b = ground(a + d * 400.0 + Vec3::Y * 60.0);
            ((b.y - a.y).abs() < 1.0 && level.los(a + Vec3::Y * 150.0, b + Vec3::Y * 150.0)).then_some((a, b))
        })
        .ok_or("no spawn pad with open floor ahead")?;
    world.step(4, &[PlayerInput::default()]);
    let angle = pd_core::math::wrap_pos(pd_core::math::atan2f(a.x - b.x, a.z - b.z));
    harness::place(&mut world, 1, b, angle);
    let d = b - a;
    let theta = (-d.x).atan2(d.z).to_degrees();
    harness::place_player(&mut world, 0, a, if theta < 0.0 { theta + 360.0 } else { theta });

    let gpu = HeadlessGpu::new()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
    renderer.load_stage(&gpu.device, &gpu.queue, &assets, "ref")?;
    let target = RenderTarget::on_device(&gpu.device, w, h, format, true);
    let mut f = Flow { world, menu, lv: Lv::new(), gpu, renderer, target, over: false };

    let mut paths = Vec::new();
    for word in &words {
        if let Some(name) = word.strip_prefix("shot:") {
            paths.push(f.shot(outdir.join(format!("flow_{name}.png")))?);
        } else if let Some(bit) = button(word) {
            f.frame(bit);
            for _ in 0..6 {
                f.frame(0);
            }
        } else if word == "kill" {
            // Aim at the chest.
            let p = f.world.players[0].pos;
            let c = f.world.chrs[1].pos + Vec3::Y * 20.0;
            f.world.players[0].verta = (c.y - p.y).atan2(((c.x - p.x).powi(2) + (c.z - p.z).powi(2)).sqrt()).to_degrees();
            for t in 0..60 * 12 {
                f.frame(if (t / 6) % 2 == 0 { Z_TRIG } else { 0 });
                if f.world.chr_is_dead(1) {
                    break;
                }
            }
            if !f.world.chr_is_dead(1) {
                return Err("kill: the simulant survived".into());
            }
        } else if word == "wend" {
            let mut n = 0;
            while f.menu.menudata.root != pd_menu::types::MENUROOT_MPENDSCREEN || f.menu.menudata.count == 0 {
                f.frame(0);
                n += 1;
                if n > 60 * 20 {
                    return Err("wend: the match did not end".into());
                }
            }
            for _ in 0..30 {
                f.frame(0);
            }
        } else if let Some(n) = word.strip_prefix('w') {
            let n: u32 = n.parse().map_err(|_| format!("bad wait {word}"))?;
            for _ in 0..n {
                f.frame(0);
            }
        } else {
            return Err(format!("flow: unknown step {word}"));
        }
    }
    Ok(paths)
}
