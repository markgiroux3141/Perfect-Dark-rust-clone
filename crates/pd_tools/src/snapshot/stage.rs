//! `pd_snapshot <outdir> stage <code> [--size WxH] [--spawns n] [--frames n]`:
//! a player's view of a stage from its spawn pads, through the game's own path
//! (a `pd_sim` world with one player, `pd_render`'s BG on a headless GPU).
//! Each shot places the player as `player_start_new_life` would at spawn pad
//! k, lets `--frames` idle frames run (default 40, so the head bob settles),
//! and writes `<code>_spawn<k>_pad<pad>.png`.
//!
//! The default size is PD's 320 × 220 view at 2×; the aspect is the image's,
//! so `--size 960x540` frames the view like the old `pd_complex_snapshot`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use pd_core::mp::{MatchPlayer, MatchSetup};
use pd_render::{Renderer, View};
use pd_sim::player::PlayerInput;
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

pub fn run(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut code: Option<String> = None;
    let (mut w, mut h) = (640u32, 440u32);
    let mut spawns = 8usize;
    let mut frames = 40usize;
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
            "--spawns" => spawns = val("--spawns")?.parse().map_err(|e| format!("--spawns: {e}"))?,
            "--frames" => frames = val("--frames")?.parse().map_err(|e| format!("--frames: {e}"))?,
            s if !s.starts_with("--") && code.is_none() => code = Some(s.to_owned()),
            s => return Err(format!("stage: unknown argument {s:?}")),
        }
    }
    let code = code.ok_or("stage: which stage? (e.g. ref)")?;
    let assets = crate::assets();
    let stage = Arc::new(Stage::load(&assets, &code)?);
    let level = Arc::new(TileLevel::new(stage.geom.clone()));
    let res = Arc::new(WorldRes::load(&assets)?);
    let setup = MatchSetup { stagenum: stage.stagenum, players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
    let mut world = World::new(setup, stage.clone(), level.clone(), res, 0)?;

    let gpu = HeadlessGpu::new()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
    renderer.load_stage(&gpu.device, &gpu.queue, &assets, &code)?;
    let (znear, zfar) = renderer.z_range();
    let target = RenderTarget::on_device(&gpu.device, w, h, format, true);
    let depth = &target.depth.as_ref().expect("depth").1;

    let mut paths = Vec::new();
    for (k, &pad) in stage.spawn_pads.iter().enumerate().take(spawns) {
        let p = &stage.pads[pad];
        world.players[0].start_new_life(&level, p.pos, p.look_angle());
        for _ in 0..frames {
            world.step(4, &[PlayerInput::default()]);
        }
        let mut view = View::for_player(&world.players[0], znear, zfar);
        view.aspect = w as f32 / h as f32;
        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snapshot") });
        renderer.render(&gpu.queue, &mut enc, &target.view, depth, &view);
        gpu.queue.submit(Some(enc.finish()));
        let rgba = target.read_rgba8(&gpu.device, &gpu.queue);
        let path = outdir.join(format!("{code}_spawn{k}_pad{pad:04x}.png"));
        crate::write_png(&path, w as usize, h as usize, &rgba)?;
        paths.push(path);
    }
    Ok(paths)
}
