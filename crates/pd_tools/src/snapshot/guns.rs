//! `pd_snapshot <outdir> guns [--all | <weapon>...] [--size WxH] [--frames n]`:
//! the guns in the firing range, through the game's own path (a `pd_sim` world
//! with one player and the range's boards, `pd_render`'s gun pass, effects and
//! HUD on a headless GPU).
//!
//! * `--all` (the default): every weapon in the loadout, single then dual,
//!   equipped and left idle `--frames` frames (150), as
//!   `all_<num>_<name>[_x2].png`, the frames the old `pd_gun_snapshot --all`
//!   wrote;
//! * `<weapon>...` (by name or short name): idle, first shot, eight frames of
//!   fire, mid-reload and aiming, as `<name>_{1idle,2fire,2fire_b,3reload,4aim}.png`.
//!
//! The view is the old tool's: 960 × 540, and the player's screen PD's 320 × 180
//! at 16:9, so the pictures frame the guns alike.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use pd_core::mp::{MatchPlayer, MatchSetup};
use pd_render::Renderer;
use pd_sim::player::PlayerInput;
use pd_sim::stage::{fixtures, Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

struct Snap {
    gpu: HeadlessGpu,
    renderer: Renderer,
    target: RenderTarget,
    w: u32,
    h: u32,
}

impl Snap {
    fn png(&mut self, world: &World, path: PathBuf) -> Result<PathBuf, String> {
        let depth = &self.target.depth.as_ref().expect("depth").1;
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("guns") });
        self.renderer.render_player(&self.gpu.device, &self.gpu.queue, &mut enc, &self.target.view, depth, world, 0);
        self.gpu.queue.submit(Some(enc.finish()));
        let rgba = self.target.read_rgba8(&self.gpu.device, &self.gpu.queue);
        crate::write_png(&path, self.w as usize, self.h as usize, &rgba)?;
        Ok(path)
    }
}

fn range(res: Arc<WorldRes>) -> Result<World, String> {
    let stage = Stage::fixture("range", fixtures::firing_range(), &[fixtures::FIRING_RANGE_SPAWN]);
    let level = TileLevel::new(stage.geom.clone());
    let setup = MatchSetup { players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
    let mut w = World::new(setup, Arc::new(stage), Arc::new(level), res, 0x1234_5678)?;
    w.boards = fixtures::firing_range_boards();
    // The old tool's 16:9 screen (`player_tick` sets the camera from these).
    let p = &mut w.players[0];
    p.viewwidth = 320.0;
    p.viewheight = 180.0;
    p.aspect = 16.0 / 9.0;
    Ok(w)
}

fn run_frames(w: &mut World, input: &PlayerInput, n: usize) {
    for _ in 0..n {
        w.step(4, std::slice::from_ref(input));
    }
}

fn file_name(res: &WorldRes, weaponnum: u8, short: bool) -> String {
    res.gset.weapon(weaponnum).map_or(format!("w{weaponnum}"), |d| {
        let n = if short && !d.short_name.is_empty() { &d.short_name } else { &d.name };
        n.replace(' ', "_")
    })
}

pub fn run(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let (mut w, mut h) = (960u32, 540u32);
    let mut frames = 150usize;
    let mut names: Vec<String> = Vec::new();
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
            "--frames" => frames = val("--frames")?.parse().map_err(|e| format!("--frames: {e}"))?,
            "--all" => names.clear(),
            s if !s.starts_with("--") => names.push(s.to_owned()),
            s => return Err(format!("guns: unknown argument {s:?}")),
        }
    }
    let assets = crate::assets();
    let res = Arc::new(WorldRes::load(&assets)?);
    let gpu = HeadlessGpu::new()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
    renderer.load_assets(&assets)?;
    let target = RenderTarget::on_device(&gpu.device, w, h, format, true);
    let mut snap = Snap { gpu, renderer, target, w, h };
    let idle = PlayerInput::default();
    let mut paths = Vec::new();

    if names.is_empty() {
        let mut world = range(res.clone())?;
        let inv = world.players[0].gun.p.inventory.clone();
        for (weaponnum, dual) in inv {
            for d in [false, true] {
                if d && !dual {
                    continue;
                }
                run_frames(&mut world, &PlayerInput { select: Some((weaponnum, d)), ..Default::default() }, 1);
                run_frames(&mut world, &idle, frames);
                let name = format!("all_{weaponnum:02}_{}{}.png", file_name(&res, weaponnum, false), if d { "_x2" } else { "" });
                paths.push(snap.png(&world, outdir.join(name))?);
            }
        }
        return Ok(paths);
    }

    for n in &names {
        let weaponnum = res
            .gset
            .weapons
            .values()
            .find(|d| d.short_name.eq_ignore_ascii_case(n) || d.name.eq_ignore_ascii_case(n) || d.name.replace(' ', "_").eq_ignore_ascii_case(n))
            .map(|d| d.weaponnum)
            .ok_or(format!("guns: no weapon {n:?}"))?;
        let name = file_name(&res, weaponnum, true);
        let mut world = range(res.clone())?;
        run_frames(&mut world, &PlayerInput { select: Some((weaponnum, false)), ..Default::default() }, 1);
        run_frames(&mut world, &idle, frames);
        paths.push(snap.png(&world, outdir.join(format!("{name}_1idle.png")))?);
        let fire = PlayerInput { fire: true, ..Default::default() };
        run_frames(&mut world, &fire, 1);
        paths.push(snap.png(&world, outdir.join(format!("{name}_2fire.png")))?);
        run_frames(&mut world, &fire, 8);
        paths.push(snap.png(&world, outdir.join(format!("{name}_2fire_b.png")))?);
        run_frames(&mut world, &idle, 40);
        run_frames(&mut world, &PlayerInput { reload: true, ..Default::default() }, 1);
        run_frames(&mut world, &idle, 25);
        paths.push(snap.png(&world, outdir.join(format!("{name}_3reload.png")))?);
        run_frames(&mut world, &idle, 120);
        run_frames(&mut world, &PlayerInput { aim: true, ..Default::default() }, 60);
        paths.push(snap.png(&world, outdir.join(format!("{name}_4aim.png")))?);
    }
    Ok(paths)
}
