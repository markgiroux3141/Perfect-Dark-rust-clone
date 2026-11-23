//! `pd_edit --shot-ui <out.png> [<code>] [--size WxH] [--frames N] [--step
//! home|layout|items|test] [--select KIND INDEX] [--generate [SEED]] [--room
//! KEY] [--side] [--import FILE.glb] [--tool KIND] [--hover X,Y] [--run-test]`: the whole editor window (its panels
//! and the view between them) as a PNG, with no window. The app runs headless
//! (`App::ui` on an egui context with no window, `App::update` between
//! passes) until the level asked for has loaded and nothing is running, then
//! the view is drawn into what the panels leave free and the panels over it.
//! For Claude to look at the editor's screens; the user uses the window.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use engine::debug_ui::OffscreenUi;
use engine::egui;
use engine::gpu::{HeadlessGpu, RenderTarget};

use super::{theme, App, Step, FORMAT};
use crate::doc::{Item, Kind, Sel};
use crate::paths::EditPaths;

/// What the picture is of.
#[derive(Default)]
struct Shot {
    out: PathBuf,
    code: Option<String>,
    size: (u32, u32),
    frames: u32,
    step: Option<Step>,
    select: Option<(Kind, usize)>,
    generate: Option<u64>,
    /// A generated level's room to select (its key: `vast`, `medium1`).
    room: Option<String>,
    /// Run the simulant test first (Test & play's numbers).
    run_test: bool,
    /// Show a generated level from the side (through `room`, if it is given).
    side: bool,
    import: Option<PathBuf>,
    tool: Option<Kind>,
    hover: Option<egui::Pos2>,
}

fn kind_named(name: &str) -> Result<Kind, String> {
    Kind::ITEMS.into_iter().chain([Kind::Door]).find(|k| k.name().eq_ignore_ascii_case(name) || k.name().to_ascii_lowercase().starts_with(&name.to_ascii_lowercase())).ok_or_else(|| format!("no kind of item {name:?}"))
}

fn parse(args: &[String]) -> Result<Shot, String> {
    let mut s = Shot { size: (1600, 1000), frames: 12, ..Shot::default() };
    let mut it = args.iter();
    let val = |it: &mut std::slice::Iter<String>, what: &str| it.next().cloned().ok_or(format!("{what}: a value"));
    let mut out = None;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => {
                let v = val(&mut it, "--size")?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                s.size = (w.parse().map_err(|_| "--size WxH")?, h.parse().map_err(|_| "--size WxH")?);
            }
            "--frames" => s.frames = val(&mut it, "--frames")?.parse().map_err(|_| "--frames N")?,
            "--step" => {
                s.step = Some(match val(&mut it, "--step")?.as_str() {
                    "home" => Step::Home,
                    "layout" => Step::Layout,
                    "items" => Step::Items,
                    "test" => Step::Test,
                    o => return Err(format!("--step {o}: home, layout, items or test")),
                })
            }
            "--select" => {
                let k = kind_named(&val(&mut it, "--select")?)?;
                s.select = Some((k, val(&mut it, "--select")?.parse().map_err(|_| "--select KIND INDEX")?));
            }
            "--tool" => s.tool = Some(kind_named(&val(&mut it, "--tool")?)?),
            "--generate" => s.generate = Some(40),
            "--seed" => s.generate = Some(val(&mut it, "--seed")?.parse().map_err(|_| "--seed N")?),
            "--run-test" => s.run_test = true,
            "--room" => s.room = Some(val(&mut it, "--room")?),
            "--side" => s.side = true,
            "--import" => s.import = Some(PathBuf::from(val(&mut it, "--import")?)),
            "--hover" => {
                let v = val(&mut it, "--hover")?;
                let (x, y) = v.split_once(',').ok_or("--hover X,Y")?;
                s.hover = Some(egui::pos2(x.parse().map_err(|_| "--hover X,Y")?, y.parse().map_err(|_| "--hover X,Y")?));
            }
            o if o.starts_with("--") => return Err(format!("{o}: not an option of --shot-ui")),
            o if out.is_none() => out = Some(PathBuf::from(o)),
            o => s.code = Some(o.to_owned()),
        }
    }
    s.out = out.ok_or("--shot-ui <out.png> [<code>] ...")?;
    Ok(s)
}

/// One headless pass of the panels.
fn pass(app: &mut App, ctx: &egui::Context, started: Instant, size: (u32, u32), hover: Option<egui::Pos2>, painter: &mut OffscreenUi, gpu: &HeadlessGpu) -> egui::FullOutput {
    let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32))), time: Some(started.elapsed().as_secs_f64()), predicted_dt: 1.0 / 60.0, ..Default::default() };
    if let Some(p) = hover {
        raw.events.push(egui::Event::PointerMoved(p));
    }
    let out = ctx.run(raw, |c| app.ui(c));
    painter.update_textures(&gpu.device, &gpu.queue, &out.textures_delta);
    out
}

pub fn run(args: &[String]) -> Result<PathBuf, String> {
    let shot = parse(args)?;
    let gpu = HeadlessGpu::new()?;
    let paths = EditPaths::discover()?;
    let mut app = App::new(paths, shot.code.clone());
    app.init_gpu(&gpu.device, &gpu.queue);
    if let Some(seed) = shot.generate {
        app.generate_seed(seed);
    }
    if let Some(file) = &shot.import {
        app.begin_import(file);
    }
    let ctx = egui::Context::default();
    let mut painter = OffscreenUi::new(&gpu.device, FORMAT);
    let started = Instant::now();
    let deadline = Duration::from_secs(180);
    let mut n = 0;
    let mut tested = false;
    // Until the level has loaded (an import, a placement, the generator's
    // level and its preview) and the frames asked for have run.
    loop {
        let _ = pass(&mut app, &ctx, started, shot.size, shot.hover, &mut painter, &gpu);
        app.update(1.0 / 60.0);
        app.load_bg(&gpu.device, &gpu.queue);
        n += 1;
        if n == 2 {
            app.shot_setup(shot.step, shot.select.map(|(kind, index)| Sel::Item(Item { kind, index })), shot.tool);
            if let Some(room) = &shot.room {
                app.select_generated_room(room);
            }
            if shot.side {
                app.show_generated_side();
            }
        }
        if shot.run_test && !tested && n >= 3 && app.settled() {
            app.run_test();
            tested = true;
            continue;
        }
        if n >= shot.frames && app.settled() && tested == shot.run_test {
            break;
        }
        if started.elapsed() > deadline {
            return Err(format!("still busy after {} s", deadline.as_secs()));
        }
        if !app.settled() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let out = pass(&mut app, &ctx, started, shot.size, shot.hover, &mut painter, &gpu);
    let free = ctx.available_rect();
    let (w, h) = shot.size;
    // The view, in what the panels left free.
    let (fx, fy) = (free.min.x.max(0.0) as u32, free.min.y.max(0.0) as u32);
    let (fw, fh) = ((free.width() as u32).clamp(1, w - fx.min(w - 1)), (free.height() as u32).clamp(1, h - fy.min(h - 1)));
    let view = RenderTarget::on_device(&gpu.device, fw, fh, FORMAT, true);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let drawn = app.draw_view(&gpu.device, &gpu.queue, &mut encoder, &view);
    gpu.queue.submit([encoder.finish()]);
    let view_px = drawn.then(|| view.read_rgba8(&gpu.device, &gpu.queue));
    // The panels, over a transparent picture.
    let ui = RenderTarget::on_device(&gpu.device, w, h, FORMAT, false);
    painter.paint(&gpu.device, &gpu.queue, &ui.view, (w, h), &ctx, out);
    let ui_px = ui.read_rgba8(&gpu.device, &gpu.queue);
    let c = theme::VIEW_CLEAR;
    let clear = [(c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, 255];
    let mut img = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let mut base = clear;
            if let Some(v) = &view_px {
                if x >= fx && y >= fy && x < fx + fw && y < fy + fh {
                    let j = (((y - fy) * fw + (x - fx)) * 4) as usize;
                    base = [v[j], v[j + 1], v[j + 2], 255];
                }
            }
            // egui's colours are premultiplied: ui + base × (1 − α).
            let a = ui_px[i + 3] as u32;
            for k in 0..3 {
                img[i + k] = (ui_px[i + k] as u32 + base[k] as u32 * (255 - a) / 255).min(255) as u8;
            }
            img[i + 3] = 255;
        }
    }
    if let Some(dir) = shot.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    image::save_buffer(&shot.out, &img, w, h, image::ColorType::Rgba8).map_err(|e| format!("{}: {e}", shot.out.display()))?;
    Ok(shot.out)
}
