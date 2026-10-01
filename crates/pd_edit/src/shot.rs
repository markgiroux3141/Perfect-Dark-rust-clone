//! `pd_edit --shot <code> <out.png> [--at x,y,z,yaw,pitch | --item <kind> <n>]
//! [--size WxH] [--waypoints] [--cover] [--solid] [--rotate] [--open]`: the editor's view of a level, as the
//! window draws it (the stage and its objects through `pd_render`, the
//! overlay over them), rendered offscreen to a PNG, for checking without a
//! window. `--item weapon 3` frames an item (selected, with its gizmo; `--rotate`:
//! the Rotate one) as the editor's F does (`--item door 3`); `--open`: every door
//! opened two seconds before (the Doors tab's preview); `--solid`: nothing drawn through walls;
//! without either, the view the editor opens on. Yaw and pitch are
//! in degrees (yaw 0 faces +z, 90 faces +x).

use std::path::PathBuf;
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use glam::Vec3;
use pd_import::layout::Layout;
use pd_render::Renderer;
use pd_sim::world::WorldRes;

use crate::doc::Doc;
use crate::overlay::{self, Highlight, Show};
use crate::paths::EditPaths;
use crate::scene::Scene;

pub fn run(args: &[String]) -> Result<PathBuf, String> {
    let mut pos: Vec<&String> = Vec::new();
    let (mut w, mut h) = (1280u32, 800u32);
    let mut at: Option<[f32; 5]> = None;
    let mut show = Show::default();
    let mut item: Option<crate::doc::Item> = None;
    let mut rotate = false;
    // `--open`: every door opened first (the Doors tab's preview, two seconds on).
    let mut open: Option<bool> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => {
                let v = it.next().ok_or("--size WxH")?;
                let (a, b) = v.split_once('x').ok_or("--size WxH")?;
                (w, h) = (a.parse().map_err(|_| "--size WxH")?, b.parse().map_err(|_| "--size WxH")?);
            }
            "--at" => {
                let v: Vec<f32> = it.next().ok_or("--at x,y,z,yaw,pitch")?.split(',').map(|s| s.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| format!("--at: {e}"))?;
                at = Some(v.try_into().map_err(|_| "--at is x,y,z,yaw,pitch")?);
            }
            "--item" => {
                let kind = it.next().ok_or("--item <kind> <index>")?;
                let index: usize = it.next().ok_or("--item <kind> <index>")?.parse().map_err(|_| "--item <kind> <index>")?;
                let kind = crate::doc::Kind::ALL.into_iter().find(|k| k.name().to_ascii_lowercase().starts_with(&kind.to_ascii_lowercase())).ok_or("--item: spawn, weapon, ammo, hill, ctc base, ctc respawn, cover, door")?;
                item = Some(crate::doc::Item { kind, index });
            }
            "--waypoints" => show.waypoints = true,
            "--cover" => show.cover = true,
            "--solid" => show.through_walls = false,
            "--rotate" => rotate = true,
            "--open" => open = Some(true),
            _ => pos.push(a),
        }
    }
    let [code, out] = pos[..] else { return Err("usage: pd_edit --shot <code> <out.png> [--at x,y,z,yaw,pitch] [--size WxH] [--waypoints] [--cover] [--solid] [--rotate]".into()) };
    let paths = EditPaths::discover()?;
    let assets = paths.asset_dir();
    let res = Arc::new(WorldRes::load(&assets)?);
    let weapons = pd_menu::generated::MP_WEAPON_SETS[0].slots.map(|x| x as u8);
    let mut scene = Scene::load(&assets, code, res, weapons)?;
    if let Some(o) = open {
        scene.world.doors_preview(o);
        for _ in 0..120 {
            scene.world.step(4, &[pd_sim::player::PlayerInput::default()]);
        }
    }
    let lp = Layout::path_for(&paths.levels.join(format!("{code}.json")));
    let layout = match Layout::load(&lp)? {
        Some(l) => l,
        None => Layout::adopt(&paths.stage_dir(code))?.0,
    };
    let mut doc = Doc::new(layout, true);
    if doc.layout.doors.is_none() {
        // As the window does: the stage's own doors.
        doc.adopt_doors(Layout::adopt(&paths.stage_dir(code))?.0.doors.unwrap_or_default());
    }
    let mut selected = None;
    let cam = match (at, item) {
        (Some([x, y, z, yaw, pitch]), _) => crate::camera::FlyCam::new(Vec3::new(x, y, z), yaw.to_radians(), pitch.to_radians()),
        (None, Some(it)) if it.index < doc.count(it.kind) => {
            selected = Some(crate::doc::Sel::Item(it));
            scene.frame(doc.pos(it) + Vec3::Y * (crate::doc::PAD_HEIGHT - it.kind.height()), 0.0)
        }
        (None, Some(it)) => return Err(format!("there are {} of {}", doc.count(it.kind), it.kind.name())),
        (None, None) => scene.start_camera(),
    };

    let gpu = HeadlessGpu::new()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
    renderer.load_stage(&gpu.device, &gpu.queue, &assets, code)?;
    let (znear, zfar) = renderer.z_range();
    let view = cam.view(w as f32 / h as f32, znear, zfar);
    let target = RenderTarget::on_device(&gpu.device, w, h, format, true);
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pd_edit shot") });
    renderer.render_free(&gpu.device, &gpu.queue, &mut enc, &target, &scene.world, &view);
    // The overlay as the window draws it, with the selection's gizmo (in
    // `gizmo` mode: Move, or Rotate for what has a facing).
    let mut built = overlay::build(&doc, &scene, view.eye, &show, &Highlight { selected, ..Default::default() });
    if let Some(crate::doc::Sel::Item(it)) = selected {
        let p = doc.pos(it);
        let mode = if rotate && doc.facing(it).is_some() { crate::gizmo::Mode::Rotate } else { crate::gizmo::Mode::Move };
        crate::gizmo::draw(&mut built.mesh, mode, p, crate::gizmo::size(view.eye, p), None, doc.facing(it));
    }
    let draw3d = crate::draw3d::Draw3d::new(&gpu.device, format);
    draw3d.draw(&gpu.device, &mut enc, &target, crate::camera::world_to_clip(&view), &built.mesh, show.through_walls);
    gpu.queue.submit(Some(enc.finish()));
    let rgba = target.read_rgba8(&gpu.device, &gpu.queue);
    let out = PathBuf::from(out);
    image::RgbaImage::from_raw(w, h, rgba).ok_or("image size")?.save(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out)
}
