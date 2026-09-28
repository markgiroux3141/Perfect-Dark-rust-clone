//! `pd_snapshot <outdir> <what> [args...]`: render frames offscreen and write PNGs,
//! so every visual change can be checked without driving a window.
//!
//! Targets, as milestones land:
//! * `model <stem> [--head <stem>|none] [--gun <stem>] [--anim <num|name>] [--frame <f>] [--views <n>] [--pitch <deg>]`:
//!   a turntable of one model file (M1);
//! * `menu [--scale <n>] [--fresh] [--combat] <steps...>`: the menus under a
//!   scripted N64 controller, one PNG per `shot:<name>` (M2; the format is in
//!   `pd_menu::script`);
//! * `stage <code> [--size WxH] [--spawns n] [--frames n]`: a player's view from
//!   the stage's spawn pads, on the GPU (M3);
//! * `guns [--all | <weapon>...] [--size WxH] [--frames n]`: the guns, their
//!   effects and the HUD in the firing range, on the GPU (M4);
//! * `match`: later milestones.

use std::path::Path;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(outdir), Some(what)) = (args.first(), args.get(1)) else {
        eprintln!("usage: pd_snapshot <outdir> <what> [args...]   (what: model, menu, stage, guns)");
        std::process::exit(2);
    };
    let outdir = Path::new(outdir);
    let rest = &args[2..];
    let result = match what.as_str() {
        "model" => pd_tools::snapshot::model::run(outdir, rest).map(|p| vec![p]),
        "menu" => pd_tools::snapshot::menu::run(outdir, rest),
        "stage" => pd_tools::snapshot::stage::run(outdir, rest),
        "guns" => pd_tools::snapshot::guns::run(outdir, rest),
        other => Err(format!("unknown target {other:?} (M4 has: model, menu, stage, guns)")),
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
