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
//!   fire, mid-reload and aiming, as `<name>_{1idle,2fire,2fire_b,3reload,4aim}.png`;
//! * `--n64`: every picture through the N64 video / CRT chain
//!   (`n64::gpu::video`, its default settings switched on), as the game's
//!   window would show it: the view at the chain's resolution and PD's own
//!   320 × 220 screen, the tube in a 1440 × 1080 frame (`--no-3pt`: without
//!   the RDP's 3-point filter);
//! * `--seq all | <name>...`: the old tool's scripted sequences for the
//!   projectiles, explosives and specials ([`SEQUENCES`]), as `seq_<name>_*.png`,
//!   each from a fresh range, every frame drawn so the frame-to-frame effects
//!   (the zoom blur) run.
//!
//! The view is the old tool's: 960 × 540, and the player's screen PD's 320 × 180
//! at 16:9, so the pictures frame the guns alike.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use glam::Vec3;
use n64::gpu::video::{tube_rect, Frame as VideoFrame, N64Video, VideoSettings};
use pd_core::ids::*;
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
    /// `--n64`: the chain, its settings and the "window" it presents into.
    n64: Option<(N64Video, VideoSettings, RenderTarget)>,
}

impl Snap {
    fn png(&mut self, world: &World, path: PathBuf) -> Result<PathBuf, String> {
        self.draw(world);
        let (t, w, h) = match &self.n64 {
            Some((_, _, present)) => (present, present.width, present.height),
            None => (&self.target, self.w, self.h),
        };
        let rgba = t.read_rgba8(&self.gpu.device, &self.gpu.queue);
        crate::write_png(&path, w as usize, h as usize, &rgba)?;
        Ok(path)
    }

    /// Save `seq_<name>.png`, and log what the world holds.
    fn seq(&mut self, world: &World, outdir: &Path, name: &str, paths: &mut Vec<PathBuf>) -> Result<(), String> {
        paths.push(self.png(world, outdir.join(format!("seq_{name}.png")))?);
        let p = &world.players[0];
        let objs: Vec<String> = world
            .props
            .objs
            .iter()
            .map(|o| {
                let state = if o.projectile.as_ref().is_some_and(|pr| pr.flags & PROJECTILEFLAG_AIRBORNE != 0) {
                    "fly"
                } else if o.embedded.is_some() {
                    "stuck"
                } else {
                    "rest"
                };
                format!("{:#x}@({:.0},{:.0},{:.0}) t{} {state}", o.weaponnum, o.pos.x, o.pos.y, o.pos.z, o.timer240)
            })
            .collect();
        eprintln!(
            "  {name}: smokes {} explosions {} wallhits {} shake {:.1} vision {} fx {:?} objs [{}]",
            world.fx.smokes.live(),
            world.explosions.live(),
            world.fx.wallhits.len(),
            world.vi.offset,
            p.visionmode,
            p.viewfx,
            objs.join(" ")
        );
        Ok(())
    }

    /// Step `n` frames, drawing each (unsaved).
    fn live(&mut self, world: &mut World, input: &PlayerInput, n: usize) -> Result<(), String> {
        for _ in 0..n {
            world.step(4, std::slice::from_ref(input));
            self.draw(world);
        }
        Ok(())
    }

    /// One frame, the way `pd_game` draws a match (through the chain with `--n64`).
    fn draw(&mut self, world: &World) {
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("guns") });
        if let Some((nv, video, _)) = &mut self.n64 {
            self.renderer.three_point = video.three_point;
            self.renderer.hud_in_frame = false;
            self.renderer.world_depth_copy = video.aa.then(|| nv.world_depth(device, self.target.width, self.target.height));
        }
        self.renderer.render_player(device, queue, &mut enc, &self.target, world, 0);
        if let Some((nv, video, present)) = &mut self.n64 {
            let hud = &self.renderer.hud_gfx;
            nv.upload_hud(device, queue, Some((&pd_render::hud::premultiplied_rgba8(hud), hud.w as u32, hud.h as u32)));
            let (znear, zfar) = self.renderer.z_range();
            let depth = &self.target.depth.as_ref().expect("depth").1;
            let vf = VideoFrame { color: &self.target.view, color_srgb: false, size: (self.target.width, self.target.height), gun_depth: Some(depth), world_near_far: (znear, zfar), gun_near_far: (1.5, 1000.0) };
            let rect = tube_rect(present.width, present.height, 0.0);
            nv.run(device, queue, &mut enc, &vf, &present.view, rect, video);
        }
        queue.submit(Some(enc.finish()));
    }
}

fn range(res: Arc<WorldRes>, n64: bool) -> Result<World, String> {
    let stage = Stage::fixture("range", fixtures::firing_range(), &[fixtures::FIRING_RANGE_SPAWN]);
    let level = TileLevel::for_stage(&stage);
    let setup = MatchSetup { players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
    let mut w = World::new(setup, Arc::new(stage), Arc::new(level), res, 0x1234_5678)?;
    w.boards = fixtures::firing_range_boards();
    // The old tool's 16:9 screen (`player_tick` sets the camera from these);
    // through the N64 chain, PD's own 320 × 220.
    if n64 {
        return Ok(w);
    }
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
    let mut seqs: Vec<String> = Vec::new();
    let mut n64 = false;
    let mut three_point = true;
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
            "--n64" => n64 = true,
            "--no-3pt" => three_point = false,
            "--seq" => {
                let v = val("--seq")?;
                seqs.extend(if v == "all" { SEQUENCES.iter().map(|s| s.to_string()).collect() } else { vec![v] });
                seqs.extend(it.by_ref().take_while(|a| !a.starts_with("--")).cloned());
            }
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
    let video = VideoSettings { n64: true, three_point, ..VideoSettings::default() };
    let (tw, th) = if n64 { video.resolution.size() } else { (w, h) };
    let target = RenderTarget::on_device(&gpu.device, tw, th, format, true);
    let chain = n64.then(|| (N64Video::new(&gpu.device, &gpu.queue, format), video, RenderTarget::on_device(&gpu.device, 1440, 1080, format, false)));
    let mut snap = Snap { gpu, renderer, target, w, h, n64: chain };
    let idle = PlayerInput::default();
    let mut paths = Vec::new();

    if !seqs.is_empty() {
        for name in &seqs {
            eprintln!("{name}:");
            let mut world = range(res.clone(), n64)?;
            run_sequence(&mut snap, &mut world, outdir, name, &mut paths)?;
        }
        return Ok(paths);
    }

    if names.is_empty() {
        let mut world = range(res.clone(), n64)?;
        let inv = world.players[0].gun.p.inventory.weapons();
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
        let mut world = range(res.clone(), n64)?;
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

// ─── scripted sequences (`--seq <name>...`) ────────────────────────────────────

/// Every sequence, in the old tool's order.
pub const SEQUENCES: &[&str] = &["smoke", "explosion", "grenade", "cook", "pinball", "mines", "knife", "nbomb", "rocket", "devastator", "superdragon", "crossbow", "slayer", "phoenix", "laptop", "farsight", "boost", "cloak"];

fn input(f: impl FnOnce(&mut PlayerInput)) -> PlayerInput {
    let mut i = PlayerInput::default();
    f(&mut i);
    i
}

/// Equip `w` and let it come up.
fn equip(world: &mut World, w: u8) {
    run_frames(world, &input(|i| i.select = Some((w, false))), 1);
    run_frames(world, &PlayerInput::default(), 150);
}

/// Hold B past 25 ticks (the gun function toggles), then let go.
fn hold_use(world: &mut World, n: usize) {
    run_frames(world, &input(|i| i.use_held = true), n);
    run_frames(world, &PlayerInput::default(), 2);
}

/// Tap the trigger `n` times, `gap` frames apart.
fn taps(world: &mut World, n: usize, gap: usize) {
    for _ in 0..n {
        run_frames(world, &input(|i| i.fire = true), 1);
        run_frames(world, &PlayerInput::default(), gap);
    }
}

/// Face `target` from where the player stands.
fn face(world: &mut World, target: Vec3) {
    let p = &mut world.players[0];
    let d = target - p.pos;
    p.theta = (-d.x).atan2(d.z).to_degrees().rem_euclid(360.0);
    p.verta = (d.y / (d.x * d.x + d.z * d.z).sqrt()).atan().to_degrees();
}

fn run_sequence(s: &mut Snap, world: &mut World, outdir: &Path, name: &str, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let idle = PlayerInput::default();
    let fire = input(|i| i.fire = true);
    let aim = input(|i| i.aim = true);
    let w = world;
    macro_rules! png {
        ($n:expr) => {
            s.seq(w, outdir, $n, paths)?
        };
    }
    match name {
        // Muzzle smoke (Falcon taps, CMP150 burst, shotgun) and the bullet
        // holes' flame and puff on the floor ahead.
        "smoke" => {
            equip(w, WEAPON_FALCON2);
            w.players[0].verta = -38.0;
            taps(w, 5, 5);
            png!("smoke_1_falcon_taps");
            run_frames(w, &idle, 20);
            png!("smoke_2_falcon_after20");
            equip(w, WEAPON_CMP150);
            w.players[0].verta = -38.0;
            // Automatics only smoke after 14+ rounds (`bgun_update_smoke`).
            run_frames(w, &fire, 80);
            run_frames(w, &idle, 6);
            png!("smoke_3_cmp_burst");
            equip(w, WEAPON_SHOTGUN);
            w.players[0].verta = -38.0;
            taps(w, 1, 8);
            png!("smoke_4_shotgun");
        }
        // A rocket-sized blast four metres ahead on the floor: the flare frames,
        // the smoke it leaves, the scorch, the shake.
        "explosion" => {
            equip(w, WEAPON_FALCON2);
            w.players[0].verta = -12.0;
            let p = w.players[0].pos;
            w.explosion_create_simple(0, Vec3::new(p.x, 20.0, p.z + 400.0), EXPLOSIONTYPE_ROCKET);
            let mut t = 0;
            for (n, label) in [(2, "a"), (10, "b"), (25, "c"), (50, "d"), (90, "e"), (200, "f")] {
                run_frames(w, &idle, n - t);
                t = n;
                png!(&format!("explosion_{label}_t{n}"));
            }
        }
        // A grenade thrown down the range: flight, bounce, rest, the blast.
        "grenade" => {
            equip(w, WEAPON_GRENADE);
            w.players[0].verta = -6.0;
            run_frames(w, &fire, 3);
            run_frames(w, &idle, 50);
            png!("grenade_1_thrown");
            run_frames(w, &idle, 150);
            png!("grenade_2_resting");
            run_frames(w, &idle, 44);
            png!("grenade_3_blast");
            run_frames(w, &idle, 30);
            png!("grenade_4_blast_b");
        }
        // Hold the trigger past the fuse: it goes off in the hand.
        "cook" => {
            equip(w, WEAPON_GRENADE);
            run_frames(w, &fire, 262);
            png!("cook_1_blast");
            run_frames(w, &fire, 20);
            png!("cook_2_blast_b");
        }
        // Secondary: the proximity pinball.
        "pinball" => {
            equip(w, WEAPON_GRENADE);
            hold_use(w, 30);
            run_frames(w, &idle, 30);
            w.players[0].verta = -20.0;
            run_frames(w, &fire, 3);
            run_frames(w, &idle, 70);
            png!("pinball_1");
            run_frames(w, &idle, 60);
            png!("pinball_2");
        }
        // A timed mine at the right, a remote mine on the side wall detonated
        // (hold B + fire), a proximity mine at our feet.
        "mines" => {
            equip(w, WEAPON_TIMEDMINE);
            w.players[0].theta = 330.0;
            w.players[0].verta = -15.0;
            run_frames(w, &fire, 3);
            run_frames(w, &idle, 60);
            png!("mines_1_timed_stuck");
            run_frames(w, &idle, 190);
            png!("mines_2_timed_blast");
            run_frames(w, &idle, 150);
            equip(w, WEAPON_REMOTEMINE);
            w.players[0].theta = 90.0;
            w.players[0].verta = 5.0;
            run_frames(w, &fire, 3);
            run_frames(w, &idle, 70);
            png!("mines_3_remote_stuck");
            run_frames(w, &input(|i| i.use_held = true), 30);
            run_frames(w, &input(|i| (i.use_held, i.fire) = (true, true)), 3);
            run_frames(w, &input(|i| i.use_held = true), 8);
            png!("mines_4_remote_detonated");
            run_frames(w, &idle, 200);
            equip(w, WEAPON_PROXIMITYMINE);
            w.players[0].theta = 0.0;
            w.players[0].verta = -45.0;
            run_frames(w, &fire, 3);
            run_frames(w, &idle, 80);
            png!("mines_5_proxy_floor");
        }
        // The throwing knife (secondary) into the first board.
        "knife" => {
            equip(w, WEAPON_COMBATKNIFE);
            hold_use(w, 30);
            run_frames(w, &idle, 40);
            w.players[0].verta = -3.0;
            run_frames(w, &fire, 3);
            run_frames(w, &idle, 12);
            png!("knife_1_flight");
            run_frames(w, &idle, 60);
            png!("knife_2_stuck");
        }
        // The N-Bomb's dome from outside, then the overlay from inside.
        "nbomb" => {
            equip(w, WEAPON_NBOMB);
            w.players[0].verta = 2.0;
            run_frames(w, &fire, 3);
            run_frames(w, &idle, 110);
            png!("nbomb_1_dome");
            run_frames(w, &idle, 60);
            png!("nbomb_2_dome_b");
            if let Some(n) = w.props.nbombs.bombs.iter().find(|n| n.age240 >= 0) {
                let p = n.pos;
                w.players[0].pos.x = p.x;
                w.players[0].pos.z = p.z - 150.0;
            }
            run_frames(w, &idle, 4);
            png!("nbomb_3_inside");
        }
        // The rocket in the tube, the launch, the flight, the blast.
        "rocket" => {
            equip(w, WEAPON_ROCKETLAUNCHER);
            png!("rocket_1_loaded");
            run_frames(w, &fire, 1);
            run_frames(w, &idle, 6);
            png!("rocket_2_launch");
            run_frames(w, &idle, 14);
            png!("rocket_3_flight");
            run_frames(w, &idle, 40);
            png!("rocket_4_impact");
        }
        // An arcing grenade round, then the wall hugger on the side wall.
        "devastator" => {
            equip(w, WEAPON_DEVASTATOR);
            w.players[0].verta = -10.0;
            run_frames(w, &fire, 1);
            run_frames(w, &idle, 12);
            png!("devastator_1_round");
            run_frames(w, &idle, 30);
            png!("devastator_2_blast");
            run_frames(w, &idle, 120);
            hold_use(w, 30);
            run_frames(w, &idle, 60);
            w.players[0].theta = 270.0;
            w.players[0].verta = 0.0;
            run_frames(w, &fire, 1);
            run_frames(w, &idle, 30);
            png!("devastator_3_hugger_stuck");
            run_frames(w, &idle, 110);
            png!("devastator_4_hugger_drop");
        }
        // The SuperDragon's grenade launcher.
        "superdragon" => {
            equip(w, WEAPON_SUPERDRAGON);
            hold_use(w, 30);
            run_frames(w, &idle, 60);
            w.players[0].verta = -12.0;
            run_frames(w, &fire, 1);
            run_frames(w, &idle, 8);
            png!("superdragon_1_round");
            run_frames(w, &idle, 26);
            png!("superdragon_2_blast");
        }
        // A crossbow bolt into the first board.
        "crossbow" => {
            equip(w, WEAPON_CROSSBOW);
            run_frames(w, &fire, 1);
            run_frames(w, &idle, 5);
            png!("crossbow_1_flight");
            run_frames(w, &idle, 20);
            png!("crossbow_2_stuck");
        }
        // A primary rocket, then fly-by-wire: the rocket's view with its
        // interlace, steered, then blown (the frame of static).
        "slayer" => {
            equip(w, WEAPON_SLAYER);
            run_frames(w, &fire, 1);
            run_frames(w, &idle, 20);
            png!("slayer_1_rocket");
            run_frames(w, &idle, 80);
            hold_use(w, 30);
            run_frames(w, &idle, 80);
            run_frames(w, &fire, 1);
            s.live(w, &idle, 30)?;
            png!("slayer_2_fbw_view");
            s.live(w, &input(|i| i.walk_x = 127), 40)?;
            png!("slayer_3_fbw_turned");
            run_frames(w, &fire, 1);
            png!("slayer_4_static");
            s.live(w, &idle, 3)?;
            png!("slayer_5_back");
        }
        // The Phoenix's explosive shells on the first board.
        "phoenix" => {
            equip(w, WEAPON_PHOENIX);
            hold_use(w, 30);
            run_frames(w, &idle, 60);
            run_frames(w, &fire, 1);
            run_frames(w, &idle, 12);
            png!("phoenix_1_shell");
        }
        // The Laptop Gun deployed as a sentry (hold B + fire) on the floor
        // ahead, shooting at the boards; then close up while it fires.
        "laptop" => {
            equip(w, WEAPON_LAPTOPGUN);
            w.players[0].verta = -35.0;
            run_frames(w, &input(|i| i.use_held = true), 30);
            run_frames(w, &input(|i| (i.use_held, i.fire) = (true, true)), 4);
            run_frames(w, &idle, 200);
            w.players[0].verta = -10.0;
            png!("laptop_1_sentry_floor");
            if let Some(pos) = w.props.objs.iter().find(|o| o.autogun.is_some()).map(|o| o.pos) {
                w.players[0].pos.x = pos.x - 120.0;
                w.players[0].pos.z = pos.z - 60.0;
                face(w, pos);
            }
            for k in 0..4 {
                s.live(w, &idle, 1)?;
                png!(&format!("laptop_2_close_{k}"));
            }
        }
        // Aim → x-ray (the zoom blur smearing in, then settled), zoomed down
        // the hall, a round through a board.
        "farsight" => {
            equip(w, WEAPON_FARSIGHT);
            png!("farsight_0_normal");
            s.live(w, &aim, 1)?;
            png!("farsight_1_xray_first");
            s.live(w, &aim, 8)?;
            png!("farsight_2_xray_smear");
            s.live(w, &aim, 60)?;
            png!("farsight_3_xray_settled");
            s.live(w, &input(|i| (i.aim, i.zoom_in) = (true, true)), 120)?;
            s.live(w, &aim, 30)?;
            png!("farsight_4_zoomed");
            s.live(w, &input(|i| (i.aim, i.fire) = (true, true)), 1)?;
            s.live(w, &aim, 6)?;
            png!("farsight_5_shot");
        }
        // A pill: the wipe in (zoom blur and white fade) at steps 8 and 15,
        // then boosted.
        "boost" => {
            equip(w, WEAPON_COMBATBOOST);
            s.live(w, &idle, 2)?;
            s.live(w, &fire, 1)?;
            s.live(w, &idle, 7)?;
            png!("boost_1_wipe_8");
            s.live(w, &idle, 6)?;
            png!("boost_2_wipe_15");
            s.live(w, &idle, 30)?;
            png!("boost_3_on");
        }
        // The RC-P120's cloak (hold B + fire): the gun fading, then cloaked.
        "cloak" => {
            equip(w, WEAPON_RCP120);
            run_frames(w, &input(|i| i.use_held = true), 30);
            run_frames(w, &input(|i| (i.use_held, i.fire) = (true, true)), 4);
            run_frames(w, &idle, 28);
            png!("cloak_1_fading");
            run_frames(w, &idle, 60);
            png!("cloak_2_cloaked");
        }
        other => return Err(format!("guns: no sequence {other:?} (one of {})", SEQUENCES.join(", "))),
    }
    Ok(())
}
