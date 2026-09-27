//! `pd_snapshot <outdir> <what> [script...]`: render frames offscreen and write PNGs,
//! so every visual change can be checked without driving a window. Targets, as
//! milestones land: `menu` (scripted N64 inputs, as `pd_combat_sim_snapshot` did),
//! `guns` (every weapon, single and dual, plus feature sequences), `stage` (spawn-pad
//! views of a stage), `match` (scripted match frames).

fn main() {
    env_logger::init();
    eprintln!("pd_snapshot: no targets yet (M0 skeleton). See docs/MILESTONES.md.");
}
