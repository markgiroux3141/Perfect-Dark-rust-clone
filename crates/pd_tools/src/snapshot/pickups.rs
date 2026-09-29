//! `pd_snapshot <outdir> pickups [<code>] [--weapons w,w,w,w,w,w] [--bots n]
//! [--highlight] [--size WxH]`: a Combat match as PD starts it (everyone
//! unarmed, the pads holding the set), the player's whole frame.
//!
//! * One view of each weapon location's pickup and its crates, from 2 m
//!   (`pickups_<code>_<n>_<what>.png`; not PD: the snapshot's camera);
//! * the player walked onto the first weapon: the pickup message, the gun
//!   raised (`..._picked.png`), the pad fading back in and back 20 s later
//!   (`..._fadein.png`, `..._back.png`), and a shield taken, the health bar's
//!   ring full (`..._shield.png`).
//!
//! `--weapons` names the six slots by `WEAPON_*` number (default the Falcon 2,
//! CMP150, shotgun, grenades, rocket launcher, shield); `--highlight` turns on
//! the player's "highlight pickups".

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use glam::Vec3;
use pd_core::ids::*;
use pd_render::Renderer;
use pd_sim::harness;
use pd_sim::player::PlayerInput;
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

fn shot(gpu: &HeadlessGpu, renderer: &mut Renderer, target: &RenderTarget, world: &World, w: u32, h: u32, path: PathBuf) -> Result<PathBuf, String> {
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snapshot") });
    renderer.render_player(&gpu.device, &gpu.queue, &mut enc, target, world, 0);
    gpu.queue.submit(Some(enc.finish()));
    let rgba = target.read_rgba8(&gpu.device, &gpu.queue);
    crate::write_png(&path, w as usize, h as usize, &rgba)?;
    Ok(path)
}

/// `vv_theta` (degrees) and the pitch from `a` to `b`.
fn face(a: Vec3, b: Vec3) -> (f32, f32) {
    let d = b - a;
    let t = (-d.x).atan2(d.z).to_degrees();
    (if t < 0.0 { t + 360.0 } else { t }, d.y.atan2((d.x * d.x + d.z * d.z).sqrt()).to_degrees())
}

pub fn run(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut code = "ref".to_owned();
    let (mut w, mut h) = (640u32, 440u32);
    let mut weapons: Vec<u8> = vec![WEAPON_FALCON2, WEAPON_CMP150, WEAPON_SHOTGUN, WEAPON_GRENADE, WEAPON_ROCKETLAUNCHER, WEAPON_MPSHIELD];
    let mut bots = 0usize;
    let mut highlight = false;
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
            "--weapons" => weapons = val("--weapons")?.split(',').map(|s| s.parse::<u8>().map_err(|e| format!("--weapons: {e}"))).collect::<Result<_, _>>()?,
            "--bots" => bots = val("--bots")?.parse().map_err(|e| format!("--bots: {e}"))?,
            "--highlight" => highlight = true,
            s if !s.starts_with("--") => code = s.to_owned(),
            s => return Err(format!("pickups: unknown argument {s:?}")),
        }
    }
    let assets = crate::assets();
    let stage = Arc::new(Stage::load(&assets, &code)?);
    let level = Arc::new(TileLevel::for_stage(&stage));
    let res = Arc::new(WorldRes::load(&assets)?);
    let gpu = HeadlessGpu::new()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
    renderer.load_stage(&gpu.device, &gpu.queue, &assets, &code)?;
    let target = RenderTarget::on_device(&gpu.device, w, h, format, true);

    let mut setup = harness::setup(1, bots, BOTDIFF_NORMAL);
    for (s, &wn) in weapons.iter().take(6).enumerate() {
        setup.weapons[s] = pd_core::mp::mpweapon_index(wn).ok_or(format!("weapon {wn} is not in g_MpWeapons"))?;
    }
    if highlight {
        setup.players[0].chr.displayoptions |= MPDISPLAYOPTION_HIGHLIGHTPICKUPS;
    }
    let mut world = World::new(setup, stage.clone(), level.clone(), res.clone(), harness::SPIKE_SEED)?;
    // Past the match-start message, so the pickup's has the slot.
    for _ in 0..400 {
        world.step(4, &[PlayerInput::default()]);
    }
    let mut paths = Vec::new();
    // Each pickup, weapon locations first: a view from 2 m with the floor
    // level and in sight (not PD: the snapshot's camera).
    let objs: Vec<(u32, u8, u8, Vec3)> = world.props.objs.iter().filter(|o| o.is_pickup()).map(|o| (o.id, o.ty, o.weaponnum, o.pos)).collect();
    let name = |ty: u8, wn: u8| match ty {
        OBJTYPE_MULTIAMMOCRATE => "crate".to_string(),
        OBJTYPE_SHIELD => "shield".to_string(),
        _ => res.gset.weapon(wn).map_or(format!("w{wn}"), |d| d.short_name.trim_end().to_lowercase().replace(' ', "")),
    };
    let viewpoint = |pos: Vec3| {
        (0..16).find_map(|j| {
            let ang = j as f32 * std::f32::consts::TAU / 16.0;
            let p = Vec3::new(pos.x + ang.sin() * 200.0, pos.y + 60.0, pos.z + ang.cos() * 200.0);
            let (y, poly) = level.cd_find_ground_at_cyl(p, 30.0);
            let eye = Vec3::new(p.x, y + 159.0, p.z);
            (poly.is_some() && (y - pos.y).abs() < 80.0 && level.los(eye, pos + Vec3::Y * 10.0)).then_some(Vec3::new(p.x, y, p.z))
        })
    };
    let mut first_weapon = None;
    for (n, &(id, ty, wn, pos)) in objs.iter().enumerate() {
        if ty == OBJTYPE_WEAPON && first_weapon.is_none() {
            first_weapon = Some((id, pos));
        }
        // One crate per location is enough.
        if ty == OBJTYPE_MULTIAMMOCRATE && n > 0 && objs[n - 1].1 == OBJTYPE_MULTIAMMOCRATE {
            continue;
        }
        let Some(at) = viewpoint(pos) else { continue };
        let (theta, pitch) = face(at + Vec3::Y * 159.0, pos);
        harness::place_player(&mut world, 0, at, theta);
        world.players[0].verta = pitch;
        for _ in 0..2 {
            world.step(4, &[PlayerInput::default()]);
        }
        paths.push(shot(&gpu, &mut renderer, &target, &world, w, h, outdir.join(format!("pickups_{code}_{n:02}_{}.png", name(ty, wn))))?);
    }
    // Walk onto the first weapon: the message and the gun.
    if let Some((id, pos)) = first_weapon {
        let at = viewpoint(pos).ok_or("no view of the first weapon")?;
        let (theta, _) = face(at, pos);
        harness::place_player(&mut world, 0, at, theta);
        world.players[0].verta = 0.0;
        let mut frames = 0;
        while world.props.get(id).is_some_and(|o| !o.is_gone()) && frames < 300 {
            world.step(4, &[PlayerInput { walk_y: 127, ..PlayerInput::default() }]);
            frames += 1;
        }
        // PD doesn't switch to a pickup: two taps of A (nothing → fists → it).
        for _ in 0..2 {
            world.step(4, &[PlayerInput { cycle_next: true, ..PlayerInput::default() }]);
            world.step(4, &[PlayerInput::default()]);
        }
        for _ in 0..40 {
            world.step(4, &[PlayerInput::default()]);
        }
        paths.push(shot(&gpu, &mut renderer, &target, &world, w, h, outdir.join(format!("pickups_{code}_picked.png")))?);
        // Back off and watch it come back 20 s later.
        let (theta, pitch) = face(at + Vec3::Y * 159.0, pos);
        harness::place_player(&mut world, 0, at, theta);
        world.players[0].verta = pitch;
        for _ in 0..(1200 - 40 - 30) {
            world.step(4, &[PlayerInput::default()]);
        }
        paths.push(shot(&gpu, &mut renderer, &target, &world, w, h, outdir.join(format!("pickups_{code}_fadein.png")))?);
        for _ in 0..40 {
            world.step(4, &[PlayerInput::default()]);
        }
        paths.push(shot(&gpu, &mut renderer, &target, &world, w, h, outdir.join(format!("pickups_{code}_back.png")))?);
    }
    // Walk onto a shield: the health bar opens with its ring full.
    if let Some((id, pos)) = world.props.objs.iter().find(|o| o.ty == OBJTYPE_SHIELD).map(|o| (o.id, o.pos)) {
        if let Some(at) = viewpoint(pos) {
            let (theta, _) = face(at, pos);
            harness::place_player(&mut world, 0, at, theta);
            world.players[0].verta = 0.0;
            let mut frames = 0;
            while world.props.get(id).is_some_and(|o| !o.is_gone()) && frames < 300 {
                world.step(4, &[PlayerInput { walk_y: 127, ..PlayerInput::default() }]);
                frames += 1;
            }
            for _ in 0..50 {
                world.step(4, &[PlayerInput::default()]);
            }
            paths.push(shot(&gpu, &mut renderer, &target, &world, w, h, outdir.join(format!("pickups_{code}_shield.png")))?);
        }
    }
    Ok(paths)
}
