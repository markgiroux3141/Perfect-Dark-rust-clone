//! `pd_snapshot <outdir> scenario <htb|htm|pac|koh|ctc> [<code>] [--seed s]
//! [--size WxH]`: a scenario's moments from the player's view, on a headless
//! GPU, as `scenario_<name>_<n>_<what>.png`. One player and one simulant (its
//! brain off, so nothing moves the props); the player is stood where the
//! camera needs it (not PD: the snapshot's camera).
//!
//! * `htb`: the briefcase on its pad (Highlight Briefcase's green); carrying
//!   it ("Picked up the Briefcase.", the 0:30 countdown); 20 s later (0:10).
//! * `htm`: the terminal and the uplink (Highlight Terminal); the uplink in
//!   hand; a download half done at the terminal (the bar).
//! * `pac`: the first victim (Highlight Target, "Get ..." / "You are the
//!   victim!"); the player as the victim (the minute's countdown).
//! * `koh`: the hill's room lit green; taken by the player's team ("We have
//!   the Hill!", the countdown, the room turning red).
//! * `ctc`: the player's base with its case (red), the enemy base with its case
//!   (yellow).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::Vec3;
use pd_core::ids::*;
use pd_core::mp::MatchSetup;
use pd_sim::harness::{self, NavChoice};
use pd_sim::mp::scenario::PropRef;
use pd_sim::player::PlayerInput;
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

use super::matchsnap::{face, Gpu};

/// A floor spot `dist` cm from `target` (its floor at `feet`) with a clear
/// view of it, trying 16 directions, not in room `notin`.
fn spot_near(level: &TileLevel, target: Vec3, feet: f32, dist: f32, notin: Option<u16>) -> Option<Vec3> {
    (0..16).find_map(|j| {
        let ang = j as f32 * std::f32::consts::TAU / 16.0;
        let p = Vec3::new(target.x + ang.sin() * dist, feet + 60.0, target.z + ang.cos() * dist);
        let (y, poly) = level.cd_find_ground_at_cyl(p, 30.0);
        let eye = Vec3::new(p.x, y + 159.0, p.z);
        let room_ok = notin.is_none() || level.floor_room(Vec3::new(p.x, y + 10.0, p.z), 1.0) != notin;
        (poly.is_some() && room_ok && (y - feet).abs() < 80.0 && level.los(eye, target)).then_some(Vec3::new(p.x, y, p.z))
    })
}

/// Stand the player at `at` looking at `target`.
fn look_from(w: &mut World, at: Vec3, target: Vec3) {
    let (theta, pitch) = face(at + Vec3::Y * 159.0, target);
    harness::place_player(w, 0, at, theta);
    w.players[0].verta = pitch;
}

/// Stand the player `dist` from `target` (on the floor at `feet`) looking at it.
fn look_at(w: &mut World, target: Vec3, feet: f32, dist: f32) -> Result<(), String> {
    look_at_outside(w, target, feet, dist, None)
}

/// [`look_at`] from outside room `notin`.
fn look_at_outside(w: &mut World, target: Vec3, feet: f32, dist: f32, notin: Option<u16>) -> Result<(), String> {
    let level = w.level.clone();
    let at = [1.0, 0.8, 0.6, 1.3, 1.6].iter().find_map(|&k| spot_near(&level, target, feet, dist * k, notin)).ok_or(format!("no view of {target}"))?;
    look_from(w, at, target);
    Ok(())
}

fn floor_y(w: &World, p: Vec3) -> f32 {
    w.level.cd_find_ground_at_cyl(p + Vec3::Y * 20.0, 30.0).0
}

fn steps(w: &mut World, n: usize, input: &PlayerInput) {
    for _ in 0..n {
        w.step(4, std::slice::from_ref(input));
    }
}

pub fn run(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut name = None;
    let mut code = "ref".to_owned();
    let (mut w, mut h) = (640u32, 440u32);
    let mut seed = harness::SPIKE_SEED;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |n: &str| it.next().cloned().ok_or(format!("{n} needs a value"));
        match a.as_str() {
            "--size" => {
                let v = val("--size")?;
                let (a, b) = v.split_once('x').ok_or("--size is WxH")?;
                w = a.parse().map_err(|e| format!("--size: {e}"))?;
                h = b.parse().map_err(|e| format!("--size: {e}"))?;
            }
            "--seed" => seed = val("--seed")?.parse().map_err(|e| format!("--seed: {e}"))?,
            s if !s.starts_with("--") && name.is_none() => name = Some(s.to_owned()),
            s if !s.starts_with("--") => code = s.to_owned(),
            s => return Err(format!("scenario: unknown argument {s:?}")),
        }
    }
    let name = name.ok_or("scenario: which? htb, htm, pac, koh or ctc")?;
    let scenario = match name.as_str() {
        "htb" => MPSCENARIO_HOLDTHEBRIEFCASE,
        "htm" => MPSCENARIO_HACKERCENTRAL,
        "pac" => MPSCENARIO_POPACAP,
        "koh" => MPSCENARIO_KINGOFTHEHILL,
        "ctc" => MPSCENARIO_CAPTURETHECASE,
        other => return Err(format!("scenario: unknown scenario {other:?}")),
    };
    let assets = crate::assets();
    let stage = Arc::new(Stage::load(&assets, &code)?);
    let level = Arc::new(TileLevel::for_stage(&stage));
    let res = Arc::new(WorldRes::load(&assets)?);
    let mut g = Gpu::new(&assets, &code, w, h)?;
    let mut setup = MatchSetup { stagenum: stage.stagenum, scenario, ..harness::with_weapons(harness::setup(1, 1, BOTDIFF_NORMAL), &harness::DEFAULT_SET) };
    if matches!(scenario, MPSCENARIO_KINGOFTHEHILL | MPSCENARIO_CAPTURETHECASE) {
        setup.options |= MPOPTION_TEAMSENABLED;
        setup.simulants[0].chr.team = 1;
    }
    let mut world = harness::world(stage, level, res, setup, NavChoice::Pd, seed, false)?;
    world.bot_brains = false;
    let idle = PlayerInput::default();
    steps(&mut world, 3, &idle);
    let mut paths = Vec::new();
    let mut n = 0;
    let mut shot = |g: &mut Gpu, world: &World, what: &str, paths: &mut Vec<PathBuf>| -> Result<(), String> {
        n += 1;
        paths.push(g.shot(world, outdir.join(format!("scenario_{name}_{n}_{what}.png")))?);
        Ok(())
    };
    let obj_pos = |w: &World, p: Option<PropRef>| p.and_then(|p| w.scenario_prop_pos(p)).ok_or("no prop".to_string());
    match scenario {
        MPSCENARIO_HOLDTHEBRIEFCASE => {
            let case = obj_pos(&world, world.mp.scenariodata.htb.token)?;
            let feet = floor_y(&world, case);
            look_at(&mut world, case, feet, 250.0)?;
            steps(&mut world, 2, &idle);
            shot(&mut g, &world, "case", &mut paths)?;
            look_from(&mut world, Vec3::new(case.x, feet, case.z), case + Vec3::new(300.0, 100.0, 0.0));
            steps(&mut world, 20, &idle);
            shot(&mut g, &world, "carrying", &mut paths)?;
            steps(&mut world, 20 * 60, &idle);
            shot(&mut g, &world, "countdown", &mut paths)?;
        }
        MPSCENARIO_HACKERCENTRAL => {
            let d = &world.mp.scenariodata.htm;
            let term = obj_pos(&world, d.terminals[0].prop.map(PropRef::Obj))?;
            let up = obj_pos(&world, d.uplink)?;
            let tfeet = floor_y(&world, term);
            look_at(&mut world, term, tfeet, 180.0)?;
            steps(&mut world, 2, &idle);
            shot(&mut g, &world, "terminal", &mut paths)?;
            let ufeet = floor_y(&world, up);
            look_at(&mut world, up, ufeet, 250.0)?;
            steps(&mut world, 2, &idle);
            shot(&mut g, &world, "uplink", &mut paths)?;
            harness::place_player(&mut world, 0, Vec3::new(up.x, ufeet, up.z), 0.0);
            steps(&mut world, 3, &idle);
            steps(&mut world, 1, &PlayerInput { select: Some((WEAPON_DATAUPLINK, false)), ..Default::default() });
            steps(&mut world, 90, &idle);
            look_at(&mut world, term, tfeet, 150.0)?;
            steps(&mut world, 2, &idle);
            shot(&mut g, &world, "holding", &mut paths)?;
            steps(&mut world, 3, &PlayerInput { use_held: true, ..Default::default() });
            steps(&mut world, 10 * 60, &idle);
            shot(&mut g, &world, "download", &mut paths)?;
        }
        MPSCENARIO_POPACAP => {
            // The simulant's 2 s fade-in.
            steps(&mut world, 150, &idle);
            let v = world.pac_victim().ok_or("no victim")?;
            if v != 0 {
                let p = world.chrs[v].pos + Vec3::Y * 20.0;
                let feet = world.chrs[v].manground;
                look_at(&mut world, p, feet, 350.0)?;
                steps(&mut world, 2, &idle);
                shot(&mut g, &world, "victim", &mut paths)?;
            }
            while world.pac_victim() != Some(0) {
                world.pac_apply_next_victim();
            }
            steps(&mut world, 30, &idle);
            shot(&mut g, &world, "you_are_the_victim", &mut paths)?;
        }
        MPSCENARIO_KINGOFTHEHILL => {
            let hp = world.mp.scenariodata.koh.hillpos;
            let hill = world.mp.scenariodata.koh.hillroom;
            look_at_outside(&mut world, hp + Vec3::Y * 20.0, hp.y, 500.0, hill)?;
            steps(&mut world, 30, &idle);
            shot(&mut g, &world, "hill", &mut paths)?;
            look_from(&mut world, hp, hp + Vec3::new(400.0, 60.0, 200.0));
            steps(&mut world, 3 * 60, &idle);
            shot(&mut g, &world, "ours", &mut paths)?;
        }
        _ => {
            let own = obj_pos(&world, world.mp.scenariodata.ctc.tokens[0])?;
            let enemy = obj_pos(&world, world.mp.scenariodata.ctc.tokens[1])?;
            let feet = floor_y(&world, own);
            look_at(&mut world, own, feet, 250.0)?;
            steps(&mut world, 30, &idle);
            shot(&mut g, &world, "our_base", &mut paths)?;
            let feet = floor_y(&world, enemy);
            look_at(&mut world, enemy, feet, 250.0)?;
            steps(&mut world, 30, &idle);
            shot(&mut g, &world, "their_base", &mut paths)?;
        }
    }
    Ok(paths)
}
