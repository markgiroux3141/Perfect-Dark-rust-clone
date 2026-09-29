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
//! * `match [<code>] [--bots n] [--diff d] [--seed s] [--at s,s,...] [--duel]`:
//!   a Combat match with simulants, the player's whole frame (M6);
//! * `lab [<code>] [--bots n] [--diff d] [--seed s] [--at s] [--ours]`: `pd_lab`'s
//!   top-down map of a simulants-only match (M6);
//! * `flow [--stage code] [--score n] [--minutes m] [--seed s] <steps...>`: a match with the
//!   menus over it under a scripted controller: the pause menu, the kill feed,
//!   the end screens, back to the menus (M7; the steps are in
//!   `pd_tools::snapshot::flow`);
//! * `pickups [<code>] [--weapons w,...] [--bots n] [--highlight]`: a match as
//!   PD starts it, each weapon location's pickup, a pickup and its respawn (M8);
//! * `scenario <htb|htm|pac|koh|ctc> [<code>] [--seed s]`: a scenario's props,
//!   HUD, highlights and room tints from the player's view (M10).

use std::path::Path;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(outdir), Some(what)) = (args.first(), args.get(1)) else {
        eprintln!("usage: pd_snapshot <outdir> <what> [args...]   (what: model, menu, stage, guns, match)");
        std::process::exit(2);
    };
    let outdir = Path::new(outdir);
    let rest = &args[2..];
    let result = match what.as_str() {
        "model" => pd_tools::snapshot::model::run(outdir, rest).map(|p| vec![p]),
        "menu" => pd_tools::snapshot::menu::run(outdir, rest),
        "stage" => pd_tools::snapshot::stage::run(outdir, rest),
        "guns" => pd_tools::snapshot::guns::run(outdir, rest),
        "match" => pd_tools::snapshot::matchsnap::run(outdir, rest),
        "lab" => pd_tools::snapshot::matchsnap::run_lab(outdir, rest),
        "flow" => pd_tools::snapshot::flow::run(outdir, rest),
        "pickups" => pd_tools::snapshot::pickups::run(outdir, rest),
        "scenario" => pd_tools::snapshot::scenario::run(outdir, rest),
        other => Err(format!("unknown target {other:?} (model, menu, stage, guns, match, lab, flow, pickups, scenario)")),
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
