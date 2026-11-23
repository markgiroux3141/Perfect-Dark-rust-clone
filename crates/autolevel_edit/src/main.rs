//! `autolevel_edit`: the command line, and the window. Thin: what to do is read by `cli` and
//! done by `headless` (the snapshot, the lists, a room's box changed, a route taken out, a join
//! made), and the window
//! is `ui` (the panels, which take an `egui::Ui`) run by the engine (`engine::app`), as
//! `pd_edit`'s is.
//!
//! `autolevel_edit --seed 40 --shot plan.png` writes the plan view and prints the dead-end list,
//! with no window and no GPU. Without anything of that kind to do it opens the window.

use autolevel::recipe;
use autolevel_edit::{cli, headless};

#[cfg(feature = "window")]
use autolevel_edit::ui;

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", cli::USAGE);
        return Ok(());
    }
    let args = cli::parse(&argv, &|path| std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}")))?;
    let save = |r: &recipe::Recipe| match &args.save_recipe {
        Some(path) => std::fs::write(path, recipe::save(r)).map(|()| println!("recipe: {path}")).map_err(|e| format!("{path}: {e}")),
        None => Ok(()),
    };
    // (a recipe that is only written is written whether or not it makes a level; one the line
    // changes by what is in its level is written once that is done)
    if !args.changes() {
        save(&args.recipe)?;
    }
    let done = headless::run(&args)?;
    // (a room change that was refused, or rooms that make no level: the snapshot and the text
    // show it, so they are written first, and then the run fails; no recipe is written)
    let stopped = done.refused.as_ref().map(|why| format!("refused: {why}")).or_else(|| done.failed.as_ref().map(|why| format!("no level: {why}")));
    if args.changes() && stopped.is_none() {
        save(&done.recipe)?;
    }
    if let (Some(path), Some(img)) = (&args.shot, &done.shot) {
        std::fs::write(path, img.png()?).map_err(|e| format!("{path}: {e}"))?;
        println!("shot: {path} ({} x {})", img.w, img.h);
    }
    if let (Some(path), Some(img)) = (&args.shot, &done.strip) {
        // (beside the snapshot: plan.png's strip is plan.variants.png)
        let path = match path.strip_suffix(".png") {
            Some(stem) => format!("{stem}.variants.png"),
            None => format!("{path}.variants.png"),
        };
        std::fs::write(&path, img.png()?).map_err(|e| format!("{path}: {e}"))?;
        println!("variants: {path} ({} x {})", img.w, img.h);
    }
    print!("{}", done.text);
    if let Some(why) = stopped {
        return Err(why);
    }
    if args.headless() {
        return Ok(());
    }
    window(args)
}

#[cfg(feature = "window")]
fn window(args: cli::Args) -> Result<(), String> {
    use engine::app::AppConfig;
    use engine::gpu::{GpuConfig, PresentPref};
    let title = format!("autolevel_edit: seed {}", args.recipe.seed);
    // (an editor: a picture a tick is plenty, in step with the display)
    let gpu = GpuConfig { present: PresentPref::Vsync, ..GpuConfig::default() };
    let config = AppConfig { title, size: (1500, 950), tick_hz: 60.0, max_fps: 60, render_on_tick_only: true, gpu, ..AppConfig::default() };
    engine::app::run(config, ui::Window::new(args))
}

#[cfg(not(feature = "window"))]
fn window(_: cli::Args) -> Result<(), String> {
    Err("built without the window (the `window` feature): use --shot, --list, --routes, --lots, --pick, --remove, a room flag or --save-recipe".to_string())
}

fn main() {
    if let Err(why) = run() {
        eprintln!("autolevel_edit: {why}");
        std::process::exit(1);
    }
}
