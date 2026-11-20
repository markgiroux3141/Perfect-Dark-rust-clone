//! `pd_edit [<code>]`: the level editor's window (`pd_edit::app`), or one of its
//! pictures with no window: `--shot` (the 3D view), `--shot-generated` (the
//! generator's 3D view) and `--shot-ui` (the whole window, panels and all).

use engine::app::AppConfig;
use pd_edit::app::App;
use pd_edit::paths::EditPaths;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,pd_edit=info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--shot") {
        match pd_edit::shot::run(&args[1..]) {
            Ok(p) => println!("{}", p.display()),
            Err(e) => {
                eprintln!("pd_edit: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--shot-generated") {
        match pd_edit::shot::run_generated(&args[1..]) {
            Ok(lines) => println!("{}", lines.join("\n")),
            Err(e) => {
                eprintln!("pd_edit: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--shot-ui") {
        match pd_edit::app::shot_ui::run(&args[1..]) {
            Ok(p) => println!("{}", p.display()),
            Err(e) => {
                eprintln!("pd_edit: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let run = || -> Result<(), String> {
        let paths = EditPaths::discover()?;
        let app = App::new(paths, args.first().cloned());
        engine::app::run(AppConfig { title: "Arena Editor".into(), size: (1600, 1000), tick_hz: 240.0, max_substeps: 8, ..AppConfig::default() }, app)
    };
    if let Err(e) = run() {
        eprintln!("pd_edit: {e}");
        std::process::exit(1);
    }
}
