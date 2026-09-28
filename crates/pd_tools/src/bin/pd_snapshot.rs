//! `pd_snapshot <outdir> <what> [args...]`: render frames offscreen and write PNGs,
//! so every visual change can be checked without driving a window.
//!
//! Targets, as milestones land:
//! * `model <stem> [--head <stem>|none] [--gun <stem>] [--anim <num|name>] [--frame <f>] [--views <n>] [--pitch <deg>]`:
//!   a turntable of one model file (M1);
//! * `menu [--scale <n>] [--fresh] [--combat] <steps...>`: the menus under a
//!   scripted N64 controller, one PNG per `shot:<name>` (M2; the format is in
//!   `pd_menu::script`);
//! * `guns`, `stage`, `match`: later milestones.

use std::path::Path;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(outdir), Some(what)) = (args.first(), args.get(1)) else {
        eprintln!("usage: pd_snapshot <outdir> <what> [args...]   (what: model, menu)");
        std::process::exit(2);
    };
    let outdir = Path::new(outdir);
    let rest = &args[2..];
    let result = match what.as_str() {
        "model" => pd_tools::snapshot::model::run(outdir, rest).map(|p| vec![p]),
        "menu" => pd_tools::snapshot::menu::run(outdir, rest),
        other => Err(format!("unknown target {other:?} (M2 has: model, menu)")),
    };
    match result {
        Ok(paths) => {
            for p in paths {
                println!("{}", p.display());
            }
        }
        Err(e) => {
            eprintln!("pd_snapshot: {e}");
            std::process::exit(1);
        }
    }
}
