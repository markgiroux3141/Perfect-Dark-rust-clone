//! `pd_import <code | recipe.json> [--src <dir>] [--custom <dir>]`: convert a
//! level into `custom/stages/<code>/` (see the `pd_import` library). A bare
//! code reads `crates/pd_import/levels/<code>.json`, and its layout
//! `<code>.layout.json` beside it if there is one.
//!
//! `pd_import --place <code> [--fresh] [--no-check]`: place a converted level
//! again with its layout, without converting it (the waypoints from the cache
//! unless `--fresh`).
//!
//! `pd_import --adopt <code> [--force]`: write `<code>.layout.json` from the
//! converted level's placement as it is (every kind), so it can be edited by
//! hand; `--force` replaces a layout that exists.
//!
//! `pd_import --check <code>`: load a converted level and play a minute of
//! simulants on it, without converting it again.
//!
//! `pd_import --explain <code> <x> <z>`: why the waypoint generator puts no
//! sample at a point (cm), one line per floor there.
//!
//! `pd_import --probe <code> <x0> <z0> <x1> <z1>`: the generator's kinematic
//! probe from the floor at one point to the floor at the other, both ways.

use std::path::{Path, PathBuf};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("pd_import: {e}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.join("..").join("..");
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let assets = std::env::var_os("PD_ASSETS").map_or_else(|| repo.join("assets"), PathBuf::from);
    let custom = opt("--custom").or_else(|| std::env::var_os("PD_CUSTOM").map(PathBuf::from)).unwrap_or_else(|| repo.join("custom"));
    let positional: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[i - 1].as_str(), "--src" | "--custom" | "--check" | "--place" | "--adopt"))).map(|(_, a)| a).collect();
    let recipe_path = |which: &str| if which.ends_with(".json") { PathBuf::from(which) } else { manifest.join("levels").join(format!("{which}.json")) };
    let paths = pd_import::Paths { assets: assets.clone(), custom: custom.clone(), src: opt("--src") };
    if let Some(which) = opt("--place") {
        let rp = recipe_path(&which.to_string_lossy());
        let r = pd_import::recipe::Recipe::load(&rp)?;
        let layout = pd_import::layout::Layout::load(&pd_import::layout::Layout::path_for(&rp))?.unwrap_or_else(pd_import::layout::Layout::new);
        let how = pd_import::Finish { check: !args.iter().any(|a| a == "--no-check"), fresh_graph: args.iter().any(|a| a == "--fresh") };
        for line in pd_import::replace(&r, &paths, &layout, how)? {
            println!("{line}");
        }
        return Ok(());
    }
    if let Some(which) = opt("--adopt") {
        let rp = recipe_path(&which.to_string_lossy());
        let r = pd_import::recipe::Recipe::load(&rp)?;
        let out = pd_import::layout::Layout::path_for(&rp);
        if out.exists() && !args.iter().any(|a| a == "--force") {
            return Err(format!("{} exists (--force replaces it)", out.display()));
        }
        let (layout, skipped) = pd_import::layout::Layout::adopt(&paths.stage_dir(&r.code))?;
        layout.save(&out)?;
        println!("{}: {} spawns, {} weapons, {} ammo crates, {} hills, {} bases, {} respawns, {} cover", out.display(),
            layout.spawns.as_ref().map_or(0, |v| v.len()), layout.weapons.as_ref().map_or(0, |v| v.len()), layout.ammo.as_ref().map_or(0, |v| v.len()),
            layout.hills.as_ref().map_or(0, |v| v.len()), layout.bases.as_ref().map_or(0, |v| v.len()), layout.respawns.as_ref().map_or(0, |v| v.len()), layout.cover.as_ref().map_or(0, |v| v.len()));
        for s in skipped {
            println!("not taken: {s}");
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--explain") {
        let [code, x, z] = [1, 2, 3].map(|k| args.get(i + k).cloned().unwrap_or_default());
        let (x, z): (f32, f32) = (x.parse().map_err(|_| "--explain <code> <x> <z>")?, z.parse().map_err(|_| "--explain <code> <x> <z>")?);
        let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&custom);
        let stage = pd_sim::stage::Stage::load(&a, &code)?;
        let level = pd_sim::stage::TileLevel::for_stage(&stage);
        for line in pd_sim::nav::gen::explain_point(&level, x, z, &pd_sim::nav::gen::GenParams::default()) {
            println!("{line}");
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--probe") {
        let code = args.get(i + 1).cloned().unwrap_or_default();
        let v: Vec<f32> = (2..6).filter_map(|k| args.get(i + k).and_then(|s| s.parse().ok())).collect();
        let [x0, z0, x1, z1] = v[..] else { return Err("--probe <code> <x0> <z0> <x1> <z1>".into()) };
        let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&custom);
        let stage = pd_sim::stage::Stage::load(&a, &code)?;
        let level = pd_sim::stage::TileLevel::for_stage(&stage);
        let floor = |x: f32, z: f32| glam::Vec3::new(x, level.cd_find_ground_at_cyl(glam::Vec3::new(x, 100_000.0, z), 20.0).0, z);
        let (p, q) = (floor(x0, z0), floor(x1, z1));
        let params = pd_sim::nav::gen::GenParams::default();
        println!("{p} -> {q}: {:?}", pd_sim::nav::gen::probe_walk_why(&level, p, q, &params));
        println!("{q} -> {p}: {:?}", pd_sim::nav::gen::probe_walk_why(&level, q, p, &params));
        return Ok(());
    }
    if let Some(code) = opt("--check") {
        let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&custom);
        for line in pd_import::check(&a, &code.to_string_lossy())? {
            println!("{line}");
        }
        return Ok(());
    }
    let Some(which) = positional.first() else {
        return Err("usage: pd_import <code | recipe.json> [--src <dir>] [--custom <dir>] | --check <code> | --place <code> [--fresh] [--no-check] | --adopt <code> [--force]".into());
    };
    let recipe = recipe_path(which);
    let r = pd_import::recipe::Recipe::load(&recipe)?;
    let layout = pd_import::layout::Layout::load(&pd_import::layout::Layout::path_for(&recipe))?.unwrap_or_else(pd_import::layout::Layout::new);
    for line in pd_import::import(&r, &paths, &layout)? {
        println!("{line}");
    }
    Ok(())
}
