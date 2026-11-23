//! `pd_edit --shot <code> <out.png> [--at x,y,z,yaw,pitch | --item <kind> <n>]
//! [--size WxH] [--waypoints] [--cover] [--solid] [--rotate] [--open]`: the editor's view of a level, as the
//! window draws it (the stage and its objects through `pd_render`, the
//! overlay over them), rendered offscreen to a PNG, for checking without a
//! window. `--item weapon 3` frames an item (selected, with its gizmo; `--rotate`:
//! the Rotate one) as the editor's F does (`--item door 3`); `--open`: every door
//! opened two seconds before (the Doors tab's preview); `--solid`: nothing drawn through walls;
//! without either, the view the editor opens on. Yaw and pitch are
//! in degrees (yaw 0 faces +z, 90 faces +x).
//!
//! `pd_edit --shot-generated <out.png> [--seed N | --recipe FILE] [--at x,y,z,yaw,pitch | --view NAME |
//! --room KEY] [--walk x,y,z,theta | --walk NAME [--forward TICKS] [--crouch]] [--plan] [--list] [--size WxH]`:
//! the generator's level in 3D as the window's "3D" view shows it while it is being shaped
//! ([`crate::preview`]: the level made from a seed or a recipe file as a preview, no arena, nothing
//! written but a scratch folder that is deleted), from a camera (`--at` as `--shot`'s; `--view`: one of
//! the generator's own views, the names `autolevel shots` draws, e.g. `colossal_deck551`; `--room`:
//! over a room from an upper corner; none: the view the window opens on), or, with `--walk`, as a
//! walker sees it: PD's player dropped from x,y,z onto the floor beneath, facing theta (degrees, PD's:
//! what the window's walking line and the game's F1 panel show; or standing at a view by its name),
//! then walked forward for `--forward` sixtieths of a second and left to stop (`--crouch`: squatting,
//! for a vent). It prints where the walker ended. `--plan`: the plan instead, with the camera's (or
//! the walker's) place and facing marked on it as the window's "Plan" marks it. `--list`: the views'
//! and rooms' names, and no picture.

use std::path::PathBuf;
use std::sync::Arc;

use engine::gpu::{HeadlessGpu, RenderTarget};
use engine::wgpu;
use glam::Vec3;
use pd_import::layout::Layout;
use pd_render::Renderer;
use pd_core::assets::AssetDir;
use pd_sim::world::WorldRes;

use crate::camera::FlyCam;
use crate::doc::Doc;
use crate::overlay::{self, Highlight, Show};
use crate::paths::EditPaths;
use crate::preview::{self, Preview, Walker};
use crate::scene::Scene;
use crate::space::{MARK, MARK_PX};

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

/// The BG the renderer holds, every room of it, as `view` sees it, into
/// `target` (cleared to the stage's sky first): how a preview is drawn, in
/// the window and here. (The rooms are not dimmed to PD's settled brightness,
/// as the editor's fly-through view of an imported level is not.)
pub fn draw_bg(renderer: &Renderer, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, target: &RenderTarget, view: &pd_render::View) {
    if let Some((_, depth)) = &target.depth {
        renderer.render(queue, enc, &target.view, depth, view, None);
    }
}

/// What `--shot-generated` is asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub out: Option<PathBuf>,
    pub seed: u64,
    pub recipe: Option<PathBuf>,
    pub size: (u32, u32),
    pub at: Option<[f32; 5]>,
    pub view: Option<String>,
    pub room: Option<String>,
    /// Dropped from x, y, z facing theta, or standing at a view by name.
    pub walk: Option<Result<[f32; 4], String>>,
    pub forward: usize,
    pub crouch: bool,
    pub plan: bool,
    pub list: bool,
    /// The room whose box and gizmo are drawn (the camera over it unless
    /// another is asked for).
    pub select: Option<String>,
    /// A handle of its gizmo dragged so many cells (or storeys, up and down):
    /// the box it would leave drawn, red if refused.
    pub drag: Option<(crate::boxgizmo::Handle, f32)>,
    /// The cutaway: no ceilings, the storeys over the one whose floor is this.
    pub no_ceilings: bool,
    pub up_to: Option<i32>,
    /// A pixel to pick the room under, as a click there would.
    pub pick: Option<(f32, f32)>,
}

const GENERATED_USAGE: &str = "usage: pd_edit --shot-generated <out.png> [--seed N | --recipe FILE] [--at x,y,z,yaw,pitch | --view NAME | --room KEY] [--walk x,y,z,theta | --walk NAME [--forward TICKS] [--crouch]] [--select ROOM [--drag HANDLE:N]] [--no-ceilings] [--up-to FLOOR] [--pick PX,PY] [--plan] [--list] [--size WxH]\n  HANDLE: west east south north top bottom (faces) or x y z (arrows); N cells, or storeys for top, bottom and z";

impl Generated {
    pub fn parse(args: &[String]) -> Result<Generated, String> {
        let mut g = Generated {
            out: None,
            seed: 40,
            recipe: None,
            size: (1280, 800),
            at: None,
            view: None,
            room: None,
            walk: None,
            forward: 0,
            crouch: false,
            plan: false,
            list: false,
            select: None,
            drag: None,
            no_ceilings: false,
            up_to: None,
            pick: None,
        };
        let numbers = |name: &str, v: &str| v.split(',').map(|s| s.trim().parse::<f32>()).collect::<Result<Vec<f32>, _>>().map_err(|e| format!("{name}: {e}"));
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut val = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
            match a.as_str() {
                "--seed" => g.seed = val("--seed")?.parse().map_err(|e| format!("--seed: {e}"))?,
                "--recipe" => g.recipe = Some(PathBuf::from(val("--recipe")?)),
                "--size" => {
                    let v = val("--size")?;
                    let (a, b) = v.split_once('x').ok_or("--size WxH")?;
                    g.size = (a.parse().map_err(|_| "--size WxH")?, b.parse().map_err(|_| "--size WxH")?);
                }
                "--at" => g.at = Some(numbers("--at", &val("--at")?)?.try_into().map_err(|_| "--at is x,y,z,yaw,pitch")?),
                "--view" => g.view = Some(val("--view")?),
                "--room" => g.room = Some(val("--room")?),
                "--walk" => {
                    let v = val("--walk")?;
                    g.walk = Some(match numbers("--walk", &v) {
                        Ok(n) => Ok(n.try_into().map_err(|_| "--walk is x,y,z,theta, or a view's name")?),
                        Err(_) => Err(v),
                    });
                }
                "--forward" => g.forward = val("--forward")?.parse().map_err(|e| format!("--forward: {e}"))?,
                "--crouch" => g.crouch = true,
                "--plan" => g.plan = true,
                "--list" => g.list = true,
                "--select" => g.select = Some(val("--select")?),
                "--drag" => {
                    let v = val("--drag")?;
                    let (h, n) = v.split_once(':').ok_or("--drag HANDLE:N")?;
                    let h = crate::boxgizmo::Handle::named(h).ok_or(format!("--drag {h}: west, east, south, north, top, bottom, x, y or z"))?;
                    g.drag = Some((h, n.parse().map_err(|_| "--drag HANDLE:N")?));
                }
                "--no-ceilings" => g.no_ceilings = true,
                "--up-to" => g.up_to = Some(val("--up-to")?.parse().map_err(|_| "--up-to FLOOR (a storey's height: 0, 276, 551, ...)")?),
                "--pick" => {
                    let v = numbers("--pick", &val("--pick")?)?;
                    let [x, y] = v[..] else { return Err("--pick PX,PY".into()) };
                    g.pick = Some((x, y));
                }
                s if !s.starts_with("--") && g.out.is_none() => g.out = Some(PathBuf::from(s)),
                s => return Err(format!("{s:?}?\n{GENERATED_USAGE}")),
            }
        }
        if g.out.is_none() && !g.list {
            return Err(GENERATED_USAGE.into());
        }
        if g.drag.is_some() && g.select.is_none() {
            return Err("--drag needs --select ROOM".into());
        }
        if let Some(z) = g.up_to.filter(|z| !autolevel::storeys::LEVELS.contains(z)) {
            return Err(format!("--up-to {z}: a storey's floor ({:?})", autolevel::storeys::LEVELS));
        }
        Ok(g)
    }

    /// The cutaway asked for.
    pub fn cut(&self) -> preview::Cutaway {
        preview::Cutaway { ceilings: self.no_ceilings, up_to: self.up_to.and_then(|z| autolevel::storeys::LEVELS.iter().position(|&l| l == z)) }
    }

    /// What the gizmo draws for `--select` (and `--drag`) on `recipe`, and a
    /// line of what the drag came to.
    pub fn gizmo(&self, recipe: &autolevel::recipe::Recipe) -> Result<(crate::boxgizmo::Shown, Option<String>), String> {
        use crate::boxgizmo::{change_by, Shown};
        let Some(key) = &self.select else { return Ok((Shown::default(), None)) };
        let base = recipe.scattered()?;
        let lots = recipe.lots_on(base.clone())?.lots;
        let lot = lots.iter().find(|a| &a.key == key).cloned().ok_or_else(|| format!("--select {key}: no room of that key has a box (there are: {})", lots.iter().map(|a| a.key.as_str()).collect::<Vec<_>>().join(", ")))?;
        let Some((h, n)) = self.drag else { return Ok((Shown { lot: Some(lot), ..Shown::default() }, None)) };
        // (a storey is 275.6 units: LEVELS' step)
        let by = n * if h.vertical() { 275.6 } else { preview::CELL };
        let Some(change) = change_by(&lot, h, by) else { return Ok((Shown { lot: Some(lot), hot: Some(h), ..Shown::default() }, Some(format!("--drag {}:{n} changes nothing", h.name())))) };
        let t = autolevel::boxes::trial(recipe, &change, &base);
        let tried = t.lots.iter().filter(|a| a.key == *key || a.host == *key).cloned().collect();
        let line = format!("drag {}:{n}: {}{}", h.name(), crate::space::words(&change), t.refused.as_ref().map_or(String::new(), |why| format!(": refused: {why}")));
        Ok((Shown { lot: Some(lot), tried, refused: t.refused.is_some(), hot: Some(h) }, Some(line)))
    }

    /// The generator's recipe asked for: the file's, or the seed's own.
    pub fn recipe(&self) -> Result<autolevel::recipe::Recipe, String> {
        match &self.recipe {
            Some(path) => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
                autolevel::recipe::load(&text).map_err(|e| format!("{}: {e}", path.display()))
            }
            None => Ok(autolevel::recipe::Recipe::new(self.seed)),
        }
    }

    /// The camera asked for on `pv`.
    pub fn camera(&self, pv: &Preview) -> Result<FlyCam, String> {
        let names = |what: &str, all: Vec<&str>| format!("{what} (there are: {})", all.join(", "));
        match (&self.at, &self.view, &self.room) {
            (Some([x, y, z, yaw, pitch]), _, _) => Ok(FlyCam::new(Vec3::new(*x, *y, *z), yaw.to_radians(), pitch.to_radians())),
            (None, Some(name), _) => pv.view_camera(name).ok_or_else(|| names(&format!("no view {name}"), pv.views.iter().map(|(n, _)| n.as_str()).collect())),
            (None, None, Some(key)) => pv.room_camera(key).ok_or_else(|| names(&format!("no room {key}"), pv.rooms.iter().map(|r| r.key.as_str()).collect())),
            (None, None, None) => Ok(self.select.as_deref().and_then(|k| pv.room_camera(k)).unwrap_or_else(|| pv.start_camera())),
        }
    }

    /// The walker asked for on `pv`, walked as asked and left to stop; none
    /// without `--walk`.
    pub fn walker(&self, pv: &Preview, res: Arc<WorldRes>) -> Result<Option<Walker>, String> {
        let Some(walk) = &self.walk else { return Ok(None) };
        let mut w = match walk {
            Ok([x, y, z, theta]) => Walker::drop_at(pv, res, Vec3::new(*x, *y, *z), -theta.to_radians(), 0.0).map_err(|e| e.replace("the camera", &format!("{x}, {y}, {z}")))?,
            Err(name) => {
                let cam = pv.view_camera(name).ok_or_else(|| format!("--walk: {name} is neither x,y,z,theta nor a view (--list names them)"))?;
                Walker::drop_at(pv, res, cam.pos, cam.yaw, 0.0)?
            }
        };
        let idle = pd_sim::player::PlayerInput::default();
        if self.crouch {
            // two presses: stand, duck, squat
            for _ in 0..2 {
                w.step(pv, &pd_sim::player::PlayerInput { crouch_down: true, ..idle });
                w.step(pv, &idle);
            }
        }
        // (the head settles, as `pd_snapshot stage` lets it, and a squat sinks)
        for _ in 0..40 {
            w.step(pv, &idle);
        }
        for _ in 0..self.forward {
            w.step(pv, &pd_sim::player::PlayerInput { walk_y: 127, ..idle });
        }
        if self.forward > 0 {
            // let go: PD's player runs on a little, and a fall lands
            let mut left = 600;
            while (left > 540 || w.p.isfalling) && left > 0 {
                w.step(pv, &idle);
                left -= 1;
            }
        }
        Ok(Some(w))
    }
}

/// The plan of `recipe`'s level `w` by `h`, with a camera at `eye` turned
/// `yaw` marked on it: what the window's "Plan" shows of the 3D view's camera.
pub fn plan_with_camera(recipe: &autolevel::recipe::Recipe, eye: Vec3, yaw: f32, w: u32, h: u32) -> Result<autolevel_edit::raster::Image, String> {
    use autolevel_edit::{planview, raster};
    let mut doc = autolevel_edit::doc::Doc::new(recipe.clone(), false);
    doc.regenerate()?;
    let made = doc.made().cloned().ok_or("no level")?;
    let (lo, hi) = planview::bounds(&made.level);
    let fit = raster::Fit::inside(lo, hi, w as f32, h as f32);
    let view = planview::View { scale: fit.scale, ..doc.look(false, None) };
    let pic = planview::picture(&made.level, &view);
    let mut img = raster::rasterise(&pic.prims, &fit, w as usize, h as usize, planview::colour::BACKGROUND);
    let at = fit.to(preview::cell(eye));
    img.poly(&preview::marker(at, yaw, MARK_PX + 4.0), [12, 13, 17, 255]);
    img.poly(&preview::marker(at, yaw, MARK_PX), MARK);
    Ok(img)
}

/// `pd_edit --shot-generated ...` (see the file's header): the lines it
/// prints, the picture's path last (if one was asked for).
pub fn run_generated(args: &[String]) -> Result<Vec<String>, String> {
    let g = Generated::parse(args)?;
    let recipe = g.recipe()?;
    let paths = EditPaths::discover()?;
    let root = preview::Scratch::root();
    preview::Scratch::sweep(&root);
    let pv = Preview::make_cut(&recipe, g.cut(), &paths.assets, &root)?;
    let cut = if pv.cut.none() { String::new() } else { format!(" ({})", pv.cut.words()) };
    let mut said = vec![format!("preview of seed {}{}{cut}: {}", recipe.seed, if recipe.edits.is_empty() { String::new() } else { format!(" with {} edits", recipe.edits.len()) }, pv.line())];
    if g.list {
        for (name, _) in &pv.views {
            let c = pv.view_camera(name).expect("a view of the preview's own");
            said.push(format!("view {name}: --at {:.0},{:.0},{:.0},{:.1},0", c.pos.x, c.pos.y, c.pos.z, c.yaw.to_degrees()));
        }
        for r in &pv.rooms {
            said.push(format!("room {}: cells {}, {} to {}, {}, floor {}, top {}{}", r.key, r.rect.0, r.rect.1, r.rect.2, r.rect.3, r.floor, r.top, if r.host.is_empty() { String::new() } else { format!(", a pod in {}", r.host) }));
        }
    }
    let Some(out) = g.out.clone() else { return Ok(said) };
    let (w, h) = g.size;
    let walker = match &g.walk {
        Some(_) => g.walker(&pv, Arc::new(WorldRes::load(&AssetDir::new(&paths.assets))?))?,
        None => None,
    };
    let cam = match &walker {
        Some(wk) => {
            said.push(format!("walker: {}{}", wk.line(), pv.room_at(wk.feet()).map_or(String::new(), |r| format!(", in {}", r.key))));
            wk.camera()
        }
        None => g.camera(&pv)?,
    };
    if g.plan {
        let img = plan_with_camera(&recipe, cam.pos, cam.yaw, w, h)?;
        std::fs::write(&out, img.png()?).map_err(|e| format!("{}: {e}", out.display()))?;
        said.push(out.display().to_string());
        return Ok(said);
    }
    let gpu = HeadlessGpu::new()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&gpu.device, &gpu.queue, format);
    let loading = std::time::Instant::now();
    renderer.load_stage(&gpu.device, &gpu.queue, &pv.asset_dir(), preview::CODE)?;
    said.push(format!("its BG loaded onto the GPU in {:.2} s", loading.elapsed().as_secs_f32()));
    let (znear, zfar) = renderer.z_range();
    let aspect = w as f32 / h as f32;
    let view = match &walker {
        Some(wk) => wk.view(aspect, znear, zfar),
        None => cam.view(aspect, znear, zfar),
    };
    if let Some((px, py)) = g.pick {
        let (o, d) = crate::camera::ray(&view, px, py, w as f32, h as f32);
        said.push(format!("picked at {px:.0}, {py:.0}: {}", crate::boxgizmo::pick_room(&pv, o, d).unwrap_or_else(|| "no room".to_string())));
    }
    let (shown, line) = g.gizmo(&recipe)?;
    said.extend(line);
    let target = RenderTarget::on_device(&gpu.device, w, h, format, true);
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pd_edit shot-generated") });
    draw_bg(&renderer, &gpu.queue, &mut enc, &target, &view);
    if shown.lot.is_some() {
        let mut mesh = crate::draw3d::Mesh3d::default();
        crate::boxgizmo::draw(&mut mesh, &shown, cam.pos);
        crate::draw3d::Draw3d::new(&gpu.device, format).draw(&gpu.device, &mut enc, &target, crate::camera::world_to_clip(&view), &mesh, true);
    }
    gpu.queue.submit(Some(enc.finish()));
    let rgba = target.read_rgba8(&gpu.device, &gpu.queue);
    image::RgbaImage::from_raw(w, h, rgba).ok_or("image size")?.save(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    said.push(out.display().to_string());
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_generated_shots_command_line_is_read() {
        let g = Generated::parse(&words("out.png")).unwrap();
        assert_eq!((g.out.as_deref(), g.seed, g.size, g.walk.is_none(), g.plan), (Some(std::path::Path::new("out.png")), 40, (1280, 800), true, false));
        assert_eq!(g.recipe().unwrap(), autolevel::recipe::Recipe::new(40));
        let g = Generated::parse(&words("--seed 7 a.png --at 1,2.5,-3,90,-10 --size 640x400")).unwrap();
        assert_eq!((g.seed, g.at, g.size), (7, Some([1.0, 2.5, -3.0, 90.0, -10.0]), (640, 400)));
        let g = Generated::parse(&words("a.png --view colossal_deck551 --room loft1 --plan")).unwrap();
        assert_eq!((g.view.as_deref(), g.room.as_deref(), g.plan), (Some("colossal_deck551"), Some("loft1"), true));
        // a walk from a place, or from one of the generator's views by its name
        let g = Generated::parse(&words("a.png --walk 2539,300,-1831,147 --forward 120 --crouch")).unwrap();
        assert_eq!((g.walk, g.forward, g.crouch), (Some(Ok([2539.0, 300.0, -1831.0, 147.0])), 120, true));
        let g = Generated::parse(&words("a.png --walk corridor_c09")).unwrap();
        assert_eq!(g.walk, Some(Err("corridor_c09".to_string())));
        // --list needs no picture; anything else does
        assert!(Generated::parse(&words("--list")).is_ok());
        assert!(Generated::parse(&words("--seed 40")).unwrap_err().starts_with("usage: pd_edit --shot-generated"));
        assert!(Generated::parse(&words("a.png --walk 1,2,3")).unwrap_err().contains("--walk is x,y,z,theta"));
        assert!(Generated::parse(&words("a.png --sideways")).unwrap_err().starts_with("\"--sideways\"?"));
        assert!(Generated::parse(&words("a.png --recipe no/such/file.json")).unwrap().recipe().unwrap_err().starts_with("no/such/file.json: "));
    }

    /// What `--shot-generated` does short of the GPU: the preview, the
    /// camera asked for, the walker walked, and the plan with the camera on
    /// it.
    #[test]
    fn a_generated_shots_camera_walker_and_plan() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets");
        let root = std::env::temp_dir().join(format!("pd_edit_shot_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pv = Preview::make(&autolevel::recipe::Recipe::new(40), &assets, &root).unwrap();
        let res = Arc::new(WorldRes::load(&AssetDir::new(&assets)).unwrap());
        let shot = |line: &str| Generated::parse(&words(line)).unwrap();
        // the cameras: the view the window opens on, a view by name, a room, a place
        assert_eq!(shot("a.png").camera(&pv).unwrap().pos, pv.start_camera().pos);
        assert_eq!(shot("a.png --view colossal_deck551").camera(&pv).unwrap().pos, preview::world((22.5, 15.5), 551.0 + preview::EYE));
        assert!(shot("a.png --view nowhere").camera(&pv).unwrap_err().starts_with("no view nowhere (there are: colossal_door, colossal_deck551, "));
        assert!(shot("a.png --room nowhere").camera(&pv).unwrap_err().starts_with("no room nowhere (there are: colossal, spire1, "));
        let at = shot("a.png --at 100,200,300,90,-45").camera(&pv).unwrap();
        assert!(at.pos == Vec3::new(100.0, 200.0, 300.0) && (at.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6 && (at.pitch + std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        // no walker unless asked for
        assert!(shot("a.png").walker(&pv, res.clone()).unwrap().is_none());
        // dropped from over the hall's door (θ 147, as the generator's view there faces) and
        // walked two seconds: in the hall, on its floor, stopped
        let w = shot("a.png --walk 2539,200,-1831,146.8 --forward 120").walker(&pv, res.clone()).unwrap().unwrap();
        let gone = (w.feet() - preview::world((21.5, 15.5), 0.0)).length();
        assert!((700.0..1300.0).contains(&gone) && w.feet().y.abs() < 1.0 && !w.p.isfalling, "{}", w.line());
        assert_eq!(pv.room_at(w.feet()).map(|r| r.key.as_str()), Some("colossal"));
        // the same from the view's name
        let named = shot("a.png --walk colossal_door --forward 120").walker(&pv, res.clone()).unwrap().unwrap();
        assert!((named.feet() - w.feet()).length() < 5.0, "{} / {}", named.line(), w.line());
        // squatting through the vent the standing walker is stopped at
        let stood = shot("a.png --walk vent_v02_colossal --forward 240").walker(&pv, res.clone()).unwrap().unwrap();
        let crawled = shot("a.png --walk vent_v02_colossal --forward 240 --crouch").walker(&pv, res.clone()).unwrap().unwrap();
        assert!(preview::cell(stood.feet()).0 < 22.8 && preview::cell(crawled.feet()).0 > 25.0 && crawled.stance() == "squatting", "{} / {}", stood.line(), crawled.line());
        // nowhere to stand
        let none = shot("a.png --walk -9000,0,9000,0").walker(&pv, res.clone()).err().unwrap();
        assert_eq!(none, "no floor under -9000, 0, 9000: fly over a floor first");
        assert!(shot("a.png --walk nowhere").walker(&pv, res).err().unwrap().starts_with("--walk: nowhere is neither x,y,z,theta nor a view"));

        // The plan with the camera marked: the mark's pink where the camera is (the hall's door),
        // and nowhere else on the plan.
        let cam = pv.start_camera();
        let (w, h) = (900u32, 700u32);
        let img = plan_with_camera(&autolevel::recipe::Recipe::new(40), cam.pos, cam.yaw, w, h).unwrap();
        let made = autolevel_edit::deadlist::make(&autolevel::recipe::Recipe::new(40)).unwrap();
        let (lo, hi) = autolevel_edit::planview::bounds(&made.level);
        let fit = autolevel_edit::raster::Fit::inside(lo, hi, w as f32, h as f32);
        let (x, y) = fit.to((21.5, 15.5));
        assert_eq!(img.get(x.round() as usize, y.round() as usize), MARK);
        let pink = |img: &autolevel_edit::raster::Image| (0..h as usize).flat_map(|y| (0..w as usize).map(move |x| (x, y))).filter(|&(x, y)| img.get(x, y) == MARK).collect::<Vec<_>>();
        let marked = pink(&img);
        assert!(marked.len() > 60 && marked.iter().all(|&(px, py)| (px as f32 - x).abs() < MARK_PX + 2.0 && (py as f32 - y).abs() < MARK_PX + 2.0), "{} pixels", marked.len());
        // it points the way the camera looks (north-west from that door: up and left on the plan):
        // its farthest point from the camera is its tip, MARK_PX that way
        let look = preview::heading(cam.yaw);
        let tip = (x + look.0 * MARK_PX, y - look.1 * MARK_PX);
        assert!(look.0 < -0.5 && look.1 > 0.8, "{look:?}");
        let far = marked.iter().copied().max_by(|a, b| ((a.0 as f32 - x).hypot(a.1 as f32 - y)).total_cmp(&(b.0 as f32 - x).hypot(b.1 as f32 - y))).unwrap();
        assert!((far.0 as f32 - tip.0).abs() < 2.5 && (far.1 as f32 - tip.1).abs() < 2.5, "{far:?} for {tip:?}");
        drop(pv);
        let _ = std::fs::remove_dir_all(&root);
    }
}
