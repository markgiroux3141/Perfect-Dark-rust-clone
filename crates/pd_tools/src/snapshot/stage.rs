//! `pd_snapshot <outdir> stage <code> [--size WxH] [--spawns n] [--frames n] [--all-rooms] [--full] [--farsight] [--at x,y,z,theta] [--views file.json]`:
//! a player's view of a stage from its spawn pads, through the game's own path
//! (a `pd_sim` world with one player, `pd_render`'s BG on a headless GPU).
//! Each shot places the player as `player_start_new_life` would at spawn pad
//! k, lets `--frames` idle frames run (default 40, so the head bob settles),
//! and writes `<code>_spawn<k>_pad<pad>.png`.
//!
//! The default size is PD's 320 × 220 view at 2×; the aspect is the image's,
//! so `--size 960x540` frames the view like the old `pd_complex_snapshot`.
//!
//! The rooms drawn are those the player's portals find, lit as the stage lights
//! them, unless the size has another aspect than the view's or `--all-rooms`.
//!
//! `--farsight`: the player's whole frame instead (`Renderer::render_player`),
//! the Farsight up and aimed for 80 frames, so the view is its x-ray, as
//! `<code>_spawn<k>_farsight.png`. `--full`: the player's whole frame
//! (objects, the sky, the HUD) as `<code>_spawn<k>_full.png`. `--at`: one view instead,
//! from the player's feet at `x,y,z` facing `theta` degrees, as the game's F1 panel
//! shows them (`P1 feet (x, y, z) θ`), as `<code>_at.png`. `--views`: many such views in one
//! run (the stage is loaded once), from a JSON list of `{"name", "x", "y", "z", "theta"}`
//! (feet and facing as `--at`'s), each written as `<name>.png`; the level generator's
//! `autolevel shots` writes such a list for a generated arena.

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
    let mut farsight = false;
    let mut all_rooms = false;
    let mut full = false;
    let mut at: Option<[f32; 4]> = None;
    let mut views: Option<Vec<(String, [f32; 4])>> = None;
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
            "--farsight" => farsight = true,
            "--all-rooms" => all_rooms = true,
            "--full" => full = true,
            "--at" => {
                let v: Vec<f32> = val("--at")?.split(',').map(|s| s.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| format!("--at: {e}"))?;
                at = Some(v.try_into().map_err(|_| "--at is x,y,z,theta")?);
            }
            "--views" => views = Some(read_views(&val("--views")?)?),
            s if !s.starts_with("--") && code.is_none() => code = Some(s.to_owned()),
            s => return Err(format!("stage: unknown argument {s:?}")),
        }
    }
    let code = code.ok_or("stage: which stage? (e.g. ref)")?;
    let assets = crate::assets();
    let stage = Arc::new(Stage::load(&assets, &code)?);
    let level = Arc::new(TileLevel::for_stage(&stage));
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
    // A pad's place and facing (tan2f(look.x, look.z)), or --at's: PD's
    // theta faces (-sin θ, 0, cos θ), so the facing is -θ.
    let feet = |[x, y, z, theta]: [f32; 4]| (glam::Vec3::new(x, y + 53.0, z), -theta.to_radians());
    let starts: Vec<(String, glam::Vec3, f32)> = match (views, at) {
        (Some(views), _) => views.into_iter().map(|(name, v)| (name, feet(v).0, feet(v).1)).collect(),
        (None, Some(v)) => vec![(format!("{code}_at"), feet(v).0, feet(v).1)],
        (None, None) => stage.spawn_pads.iter().enumerate().take(spawns).map(|(k, &pad)| (format!("{code}_spawn{k}_pad{pad:04x}"), stage.pads[pad].pos, stage.pads[pad].look_angle())).collect(),
    };
    for (k, (name, pos, facing)) in starts.into_iter().enumerate() {
        world.players[0].start_new_life(&level, &[], pos, facing);
        for _ in 0..frames {
            world.step(4, &[PlayerInput::default()]);
        }
        if farsight {
            world.step(4, &[PlayerInput { select: Some((pd_core::ids::WEAPON_FARSIGHT, false)), ..Default::default() }]);
            for _ in 0..150 {
                world.step(4, &[PlayerInput::default()]);
            }
            // Every frame drawn, so the zoom blur has its last frame.
            for _ in 0..80 {
                world.step(4, &[PlayerInput { aim: true, ..Default::default() }]);
                let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snapshot") });
                renderer.render_player(&gpu.device, &gpu.queue, &mut enc, &target, &world, 0);
                gpu.queue.submit(Some(enc.finish()));
            }
            let rgba = target.read_rgba8(&gpu.device, &gpu.queue);
            let path = outdir.join(format!("{code}_spawn{k}_farsight.png"));
            crate::write_png(&path, w as usize, h as usize, &rgba)?;
            paths.push(path);
            continue;
        }
        if full {
            // The match view: objects, the sky, the HUD.
            let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snapshot") });
            renderer.render_player(&gpu.device, &gpu.queue, &mut enc, &target, &world, 0);
            gpu.queue.submit(Some(enc.finish()));
            let rgba = target.read_rgba8(&gpu.device, &gpu.queue);
            let path = outdir.join(format!("{code}_spawn{k}_full.png"));
            crate::write_png(&path, w as usize, h as usize, &rgba)?;
            paths.push(path);
            continue;
        }
        let mut view = View::for_player(&world.players[0], znear, zfar);
        view.aspect = w as f32 / h as f32;
        // The portals are the player's 320 × 220 view's: at another aspect
        // every room is drawn, unlit.
        let same_aspect = (view.aspect - 320.0 / 220.0).abs() < 0.01;
        let frame = Renderer::bg_frame(&world, 0, w, h).filter(|_| same_aspect && !all_rooms);
        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snapshot") });
        renderer.render(&gpu.queue, &mut enc, &target.view, depth, &view, frame.as_ref());
        gpu.queue.submit(Some(enc.finish()));
        let rgba = target.read_rgba8(&gpu.device, &gpu.queue);
        let path = outdir.join(format!("{name}.png"));
        crate::write_png(&path, w as usize, h as usize, &rgba)?;
        paths.push(path);
    }
    Ok(paths)
}

/// `--views`' file: a JSON list of `{"name", "x", "y", "z", "theta"}`, each a view from the
/// player's feet at x, y, z facing theta degrees (as `--at`), to be written as `<name>.png`.
/// A name is letters, digits, `_`, `-` and `.` only, since it becomes a file's.
fn read_views(path: &str) -> Result<Vec<(String, [f32; 4])>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("--views {path}: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("--views {path}: {e}"))?;
    let list = v.as_array().ok_or_else(|| format!("--views {path}: a list of views"))?;
    let mut out = Vec::new();
    for (n, view) in list.iter().enumerate() {
        let name = view.get("name").and_then(|v| v.as_str()).ok_or_else(|| format!("--views {path}: view {n} has no name"))?;
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
            return Err(format!("--views {path}: {name:?} is no file name (letters, digits, _ - .)"));
        }
        let num = |key: &str| view.get(key).and_then(|v| v.as_f64()).map(|v| v as f32).ok_or_else(|| format!("--views {path}: {name} has no {key}"));
        out.push((name.to_owned(), [num("x")?, num("y")?, num("z")?, num("theta")?]));
    }
    Ok(out)
}
