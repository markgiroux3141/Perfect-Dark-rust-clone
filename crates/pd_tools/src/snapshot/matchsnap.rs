//! `pd_snapshot <outdir> match [<code>] [--bots n] [--diff d] [--seed s]
//! [--size WxH] [--at s,s,...] [--duel]`: a Combat match with simulants, the
//! player's whole frame (`Renderer::render_player`) on a headless GPU.
//!
//! * By default the player stands at their spawn while `--bots` simulants
//!   (default 4, `--diff` 0..5, default 2 = Normal, the spike's weapon mix)
//!   fight; at each `--at` second (default 4,8,12,16,20,30) the player is
//!   stood 3-5 m from a living simulant (each in turn) with a clear view of it
//!   (not PD: the snapshot's camera), and the frame written as
//!   `match_<code>_<s>s.png`.
//! * `--duel`: one simulant, its brain off, 4 m in front of the player: it
//!   standing, the player's Falcon hitting it (blood, flinch), it dying, and
//!   its corpse fading, as `duel_<code>_<n>_<what>.png`; `--dist` cm apart
//!   (default 400), `--gun` the simulant's weapon by `WEAPON_*` number
//!   (default the CMP150).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use glam::Vec3;
use pd_render::Renderer;
use pd_sim::harness::{self, NavChoice};
use pd_sim::player::PlayerInput;
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

/// A headless GPU with the renderer on a stage and a `w` × `h` target.
pub(crate) struct Gpu {
    gpu: HeadlessGpu,
    renderer: Renderer,
    target: RenderTarget,
    w: u32,
    h: u32,
}

impl Gpu {
    pub(crate) fn new(assets: &pd_core::assets::AssetDir, code: &str, w: u32, h: u32) -> Result<Gpu, String> {
        let gpu = HeadlessGpu::new()?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
        renderer.load_stage(&gpu.device, &gpu.queue, assets, code)?;
        let target = RenderTarget::on_device(&gpu.device, w, h, format, true);
        Ok(Gpu { gpu, renderer, target, w, h })
    }

    /// Player 0's frame as `path`.
    pub(crate) fn shot(&mut self, world: &World, path: PathBuf) -> Result<PathBuf, String> {
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snapshot") });
        self.renderer.render_player(&self.gpu.device, &self.gpu.queue, &mut enc, &self.target, world, 0);
        self.gpu.queue.submit(Some(enc.finish()));
        let rgba = self.target.read_rgba8(&self.gpu.device, &self.gpu.queue);
        crate::write_png(&path, self.w as usize, self.h as usize, &rgba)?;
        Ok(path)
    }
}

/// `vv_theta` (degrees; forward = (−sin, 0, cos)) from `a` to `b`, and the pitch.
pub(crate) fn face(a: Vec3, b: Vec3) -> (f32, f32) {
    let d = b - a;
    let t = (-d.x).atan2(d.z).to_degrees();
    let pitch = d.y.atan2((d.x * d.x + d.z * d.z).sqrt()).to_degrees();
    (if t < 0.0 { t + 360.0 } else { t }, pitch)
}

pub fn run(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut code = "ref".to_owned();
    let (mut w, mut h) = (640u32, 440u32);
    let (mut bots, mut diff, mut seed) = (4usize, 2u8, harness::SPIKE_SEED);
    let mut at: Vec<f32> = vec![4.0, 8.0, 12.0, 16.0, 20.0, 30.0];
    let mut duel = false;
    let mut dist = 400.0f32;
    let mut gun = pd_core::ids::WEAPON_CMP150;
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
            "--bots" => bots = val("--bots")?.parse().map_err(|e| format!("--bots: {e}"))?,
            "--diff" => diff = val("--diff")?.parse().map_err(|e| format!("--diff: {e}"))?,
            "--seed" => seed = val("--seed")?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--at" => at = val("--at")?.split(',').map(|s| s.parse::<f32>().map_err(|e| format!("--at: {e}"))).collect::<Result<_, _>>()?,
            "--duel" => duel = true,
            "--dist" => dist = val("--dist")?.parse().map_err(|e| format!("--dist: {e}"))?,
            "--gun" => gun = val("--gun")?.parse().map_err(|e| format!("--gun: {e}"))?,
            s if !s.starts_with("--") => code = s.to_owned(),
            s => return Err(format!("match: unknown argument {s:?}")),
        }
    }
    let assets = crate::assets();
    let stage = Arc::new(Stage::load(&assets, &code)?);
    let level = Arc::new(TileLevel::for_stage(&stage));
    let res = Arc::new(WorldRes::load(&assets)?);
    let mut g = Gpu::new(&assets, &code, w, h)?;
    if duel {
        return run_duel(outdir, &code, stage, level, res, &mut g, seed, dist, gun);
    }

    let setup = harness::setup(1, bots, diff);
    let mut world = harness::world(stage.clone(), level.clone(), res, setup, NavChoice::Pd, seed, true)?;
    let mut paths = Vec::new();
    let mut frame = 0usize;
    let mut shots = 0usize;
    for &t in &at {
        let until = (t * 60.0) as usize;
        while frame < until {
            // A dead player presses Z once the screen is black (a new life).
            let p = &world.players[0];
            let press = p.isdead && p.health.player_is_fade_complete();
            world.step(4, &[PlayerInput { fire: press, ..PlayerInput::default() }]);
            frame += 1;
        }
        // The snapshot's camera (not PD): the player stands 3-5 m from a
        // living simulant (the next in turn) with a clear view of it, facing it.
        let n = world.chrs.len();
        let mut placed = false;
        for k in 0..n.saturating_sub(1) {
            let i = 1 + (shots + k) % (n - 1);
            if world.chr_is_dead(i) {
                continue;
            }
            let c = &world.chrs[i];
            let (target, feet) = (c.pos + Vec3::Y * 20.0, c.manground);
            let spot = (0..16).find_map(|j| {
                let ang = j as f32 * std::f32::consts::TAU / 16.0;
                let dist = if j % 2 == 0 { 400.0 } else { 300.0 };
                let p = Vec3::new(c.pos.x + ang.sin() * dist, feet + 60.0, c.pos.z + ang.cos() * dist);
                let (y, poly) = level.cd_find_ground_at_cyl(p, 30.0);
                let eye = Vec3::new(p.x, y + 159.0, p.z);
                (poly.is_some() && (y - feet).abs() < 60.0 && level.los(eye, target)).then_some(Vec3::new(p.x, y, p.z))
            });
            if let Some(at) = spot {
                let (theta, pitch) = face(at + Vec3::Y * 159.0, target);
                harness::place_player(&mut world, 0, at, theta);
                world.players[0].verta = pitch;
                placed = true;
                break;
            }
        }
        shots += 1;
        if !placed {
            eprintln!("match: no view of a simulant at {t} s");
        }
        // Two frames so the camera, the on-screen tests and the pose catch up.
        for _ in 0..2 {
            world.step(4, &[PlayerInput::default()]);
            frame += 1;
        }
        paths.push(g.shot(&world, outdir.join(format!("match_{code}_{t:.0}s.png")))?);
    }
    Ok(paths)
}

#[allow(clippy::too_many_arguments)]
fn run_duel(outdir: &Path, code: &str, stage: Arc<Stage>, level: Arc<TileLevel>, res: Arc<WorldRes>, g: &mut Gpu, seed: u64, dist: f32, gun: u8) -> Result<Vec<PathBuf>, String> {
    let mut world = harness::world(stage.clone(), level.clone(), res, harness::setup(1, 1, 2), NavChoice::Pd, seed, true)?;
    world.bot_loadout = vec![Some((gun, false))];
    world.bot_brains = false;
    // A spawn pad with 4 m of level floor ahead.
    let ground = |p: Vec3| Vec3::new(p.x, level.cd_find_ground_at_cyl(p, 30.0).0, p.z);
    let (a, b) = stage
        .spawn_pads
        .iter()
        .find_map(|&p| {
            let pad = &stage.pads[p];
            let a = ground(pad.pos);
            let d = Vec3::new(pad.look.x, 0.0, pad.look.z).normalize_or_zero();
            let b = ground(a + d * dist + Vec3::Y * 60.0);
            ((b.y - a.y).abs() < 1.0 && level.los(a + Vec3::Y * 150.0, b + Vec3::Y * 150.0)).then_some((a, b))
        })
        .ok_or("no spawn pad with open floor ahead")?;
    world.step(4, &[PlayerInput::default()]);
    let angle = pd_core::math::wrap_pos(pd_core::math::atan2f(a.x - b.x, a.z - b.z));
    harness::place(&mut world, 1, b, angle);
    let (theta, _) = face(a, b);
    harness::place_player(&mut world, 0, a, theta);
    let mut paths = Vec::new();
    let mut n = 0;
    let mut shot = |world: &World, what: &str, paths: &mut Vec<PathBuf>| -> Result<(), String> {
        n += 1;
        paths.push(g.shot(world, outdir.join(format!("duel_{code}_{n}_{what}.png")))?);
        Ok(())
    };
    for _ in 0..150 {
        world.step(4, &[PlayerInput::default()]);
    }
    shot(&world, "standing", &mut paths)?;
    // Aim at the chest and tap the Falcon's trigger until it dies.
    let (_, pitch) = face(world.players[0].pos, b + Vec3::Y * 120.0);
    world.players[0].verta = pitch;
    let mut hit_shot = false;
    for t in 0..60 * 10 {
        let fire = (t / 6) % 2 == 0;
        let before = world.chrs[1].damage;
        world.step(4, &[PlayerInput { fire, ..PlayerInput::default() }]);
        if !hit_shot && world.chrs[1].damage > before {
            world.step(4, &[PlayerInput::default()]);
            shot(&world, "hit", &mut paths)?;
            hit_shot = true;
        }
        if world.chr_is_dead(1) {
            break;
        }
    }
    for k in 0..6 {
        for _ in 0..20 {
            world.step(4, &[PlayerInput::default()]);
        }
        shot(&world, &format!("dying{k}"), &mut paths)?;
    }
    for k in 0..3 {
        for _ in 0..25 {
            world.step(4, &[PlayerInput::default()]);
        }
        shot(&world, &format!("fading{k}"), &mut paths)?;
    }
    Ok(paths)
}

/// `pd_snapshot <outdir> lab [<code>] [--bots n] [--diff d] [--seed s]
/// [--at s] [--ours] [--mix] [--size WxH]`: `pd_lab`'s top-down map of a
/// simulants-only match after `--at` seconds (default 20), as
/// `lab_<code>_<s>s.png`: as PD starts it (unarmed, the pads holding
/// [`harness::DEFAULT_SET`]), or with `--mix` the spike's loadout.
pub fn run_lab(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut code = "ref".to_owned();
    let (mut w, mut h) = (1200usize, 1000usize);
    let (mut bots, mut diff, mut seed, mut at, mut nav) = (4usize, 2u8, harness::SPIKE_SEED, 20.0f32, NavChoice::Pd);
    let mut mix = false;
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
            "--bots" => bots = val("--bots")?.parse().map_err(|e| format!("--bots: {e}"))?,
            "--diff" => diff = val("--diff")?.parse().map_err(|e| format!("--diff: {e}"))?,
            "--seed" => seed = val("--seed")?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--at" => at = val("--at")?.parse().map_err(|e| format!("--at: {e}"))?,
            "--ours" => nav = NavChoice::Ours,
            "--mix" => mix = true,
            s if !s.starts_with("--") => code = s.to_owned(),
            s => return Err(format!("lab: unknown argument {s:?}")),
        }
    }
    let assets = crate::assets();
    let stage = Arc::new(Stage::load(&assets, &code)?);
    let level = Arc::new(TileLevel::for_stage(&stage));
    let res = Arc::new(WorldRes::load(&assets)?);
    let setup = harness::with_weapons(harness::setup(0, bots, diff), &harness::DEFAULT_SET);
    let mut world = harness::world(stage, level.clone(), res, setup, nav, seed, mix)?;
    for _ in 0..(at * 60.0) as usize {
        harness::step_idle(&mut world);
    }
    let opts = crate::lab::LabOpts { selected: Some(0), ..Default::default() };
    let rgba = crate::lab::rasterise(&crate::lab::picture(&world, &opts), crate::lab::bounds(&level), w, h);
    let path = outdir.join(format!("lab_{code}_{at:.0}s.png"));
    crate::write_png(&path, w, h, &rgba)?;
    Ok(vec![path])
}
