//! `pd_snapshot <outdir> <what> [args...]`: render frames offscreen and write PNGs,
//! so every visual change can be checked without driving a window.
//!
//! Targets, as milestones land:
//! * `model <stem> [--head <stem>|none] [--gun <stem>] [--anim <num|name>] [--frame <f>] [--views <n>]`:
//!   a turntable of one model file (M1);
//! * `menu` (scripted N64 inputs, as `pd_combat_sim_snapshot` did), `guns`, `stage`,
//!   `match`: later milestones.

use std::path::Path;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(outdir), Some(what)) = (args.first(), args.get(1)) else {
        eprintln!("usage: pd_snapshot <outdir> <what> [args...]   (what: model)");
        std::process::exit(2);
    };
    let rest = &args[2..];
    let result = match what.as_str() {
        "model" => pd_tools::snapshot::model::run(Path::new(outdir), rest),
        other => Err(format!("unknown target {other:?} (M1 has: model)")),
    };
    match result {
        Ok(path) => println!("{}", path.display()),
        Err(e) => {
            eprintln!("pd_snapshot: {e}");
            std::process::exit(1);
        }
    }
}
