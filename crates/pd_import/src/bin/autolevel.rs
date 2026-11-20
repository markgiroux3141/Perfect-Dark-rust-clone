//! `autolevel`: levels from a seed with no model in the loop and no Blender.
//! Thin: the arguments are read, and the work done, by `autolevel_build::cli`
//! (see its `USAGE`, or `autolevel --help`). What this program adds is why it
//! is `pd_import`'s: the walker (`pd_import::autowalk::SimWalk`: a level's
//! routes walked in this process, on `pd_sim`'s player and the tiles the
//! importer makes of the level, unless pd-walk is named), and one command:
//!
//! `autolevel import DIR [--code C] [--name N] [--root DIR]
//! [--gltf]`: the level built in DIR (`autolevel build SEED`) into the game.
//! It writes the level's recipe, `crates/pd_import/levels/<code>.json` (code
//! `autolevel<seed>`, name `Autolevel <seed>`, stage number 124 unless
//! named: 124-127 are the free ones), and imports it as `pd_import <code>`
//! does, into `custom/stages/<code>/`. The recipe holds the generator's own
//! (DIR's `recipe.json`), and the importer's source for generated levels
//! (`pd_import::autolevel`) makes the level again from it: the generator's
//! rooms and doorways as PD's rooms and portals, on the pool's textures.
//! `--gltf` imports DIR's `.glb` as any glTF instead (a split of its
//! footprint for rooms, the file's own copies of the textures).
//!
//! `autolevel shots DIR [names...] [--code C] [--root DIR]`: the level's
//! views (DIR's `views.json`: a door and a deck of each big room, the drops,
//! the grates), drawn by the game itself into `DIR/shots/<name>.png`. The
//! level must have been imported (`autolevel import DIR`): the views are
//! written as a list and `pd_snapshot stage CODE --views` (beside this
//! program) renders them all on one load of the stage. With `--pd-walk` or
//! `$PD_WALK` it is pd-walk that draws them instead, from the level's OBJ,
//! as before.
//!
//! Exits 0 when everything came out as expected, 1 when a build or a walk
//! did not (or the work could not be done), 2 for a command line it cannot
//! read.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use autolevel_build::cli;
use autolevel_build::walk::Walker;
use pd_import::autowalk::{self, SimWalk};
use pd_sim::world::WorldRes;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let help = argv.iter().any(|a| a == "--help" || a == "-h");
    if argv.first().map(String::as_str) == Some("import") && !help {
        let job = match Import::parse(&argv[1..]) {
            Ok(job) => job,
            Err(why) => {
                eprintln!("autolevel: {why}");
                std::process::exit(2);
            }
        };
        if let Err(why) = job.run(&cwd) {
            eprintln!("autolevel: {why}");
            std::process::exit(1);
        }
        return;
    }
    let pd_walk = argv.iter().any(|a| a == "--pd-walk") || std::env::var_os("PD_WALK").is_some();
    if argv.first().map(String::as_str) == Some("shots") && !help && !pd_walk {
        let done = Shots::parse(&argv[1..]).map_err(|why| (2, why)).and_then(|job| job.run(&cwd).map_err(|why| (1, why)));
        if let Err((code, why)) = done {
            eprintln!("autolevel: {why}");
            std::process::exit(code);
        }
        return;
    }
    let args = match cli::parse(&argv, &|path| std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("autolevel: {why}");
            std::process::exit(2);
        }
    };
    let env = cli::Env { cwd: cwd.clone(), pd_walk: std::env::var("PD_WALK").ok() };
    // The program's own walker. What every level shares is loaded when the
    // first one is opened, and kept for the rest (a batch).
    let assets = assets_dir(&root(args.root.as_deref(), &cwd));
    let shared: OnceLock<Result<Arc<WorldRes>, String>> = OnceLock::new();
    let open = |dir: &Path| -> Result<Box<dyn Walker>, String> {
        let t = Instant::now();
        let res = shared.get_or_init(|| SimWalk::res(&assets)).clone()?;
        let loaded = t.elapsed().as_secs_f64();
        let w = SimWalk::open(dir, &assets, res)?;
        println!("walking in this program: {} tiles in {} rooms, opened in {:.2}s (the game's data {loaded:.2}s)", w.level.geom.polys.len(), w.level.geom.rooms.len(), t.elapsed().as_secs_f64());
        Ok(Box::new(w))
    };
    match cli::run(&args, &env, Some(&open)) {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(why) => {
            eprintln!("autolevel: {why}");
            std::process::exit(1);
        }
    }
}

/// The game's repo: named, else the folder at or above the current one that
/// holds `crates/` and `assets/`, else the one this program was built in.
fn root(named: Option<&Path>, cwd: &Path) -> PathBuf {
    named.map(Path::to_path_buf).or_else(|| cli::find_root(cwd, &|p| p.is_dir())).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".."))
}

/// The game's `assets/`: `$PD_ASSETS`, else the repo's.
fn assets_dir(root: &Path) -> PathBuf {
    std::env::var_os("PD_ASSETS").map_or_else(|| root.join("assets"), PathBuf::from)
}

/// `autolevel import`: what it was asked.
struct Import {
    dir: PathBuf,
    code: Option<String>,
    name: Option<String>,
    root: Option<PathBuf>,
    /// By the glTF source, from the folder's `.glb`.
    gltf: bool,
}

impl Import {
    fn parse(args: &[String]) -> Result<Import, String> {
        let mut job = Import { dir: PathBuf::new(), code: None, name: None, root: None, gltf: false };
        let mut dirs = Vec::new();
        let mut n = 0;
        while n < args.len() {
            let arg = args[n].as_str();
            if !arg.starts_with('-') {
                dirs.push(arg);
                n += 1;
                continue;
            }
            if arg == "--gltf" {
                job.gltf = true;
                n += 1;
                continue;
            }
            let value = args.get(n + 1).ok_or_else(|| format!("{arg} needs a value"))?;
            match arg {
                "--code" => job.code = Some(value.clone()),
                "--name" => job.name = Some(value.clone()),
                "--stagenum" => return Err("--stagenum: no longer needed (the game numbers custom levels as it starts)".to_string()),
                "--root" => job.root = Some(PathBuf::from(value)),
                _ => return Err(format!("{arg}: not an option of import (see --help)")),
            }
            n += 2;
        }
        match dirs[..] {
            [dir] => job.dir = PathBuf::from(dir),
            [] => return Err("import needs the folder of a built level".to_string()),
            [_, extra, ..] => return Err(format!("{extra}: import takes one folder")),
        }
        if let Some(code) = &job.code {
            if code.is_empty() || !code.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                return Err(format!("--code: a stage code is lower-case letters, digits and _, not {code}"));
            }
        }
        Ok(job)
    }

    /// Write the level's recipe, import it, and say how to look at it.
    fn run(&self, cwd: &Path) -> Result<(), String> {
        let root = root(self.root.as_deref(), cwd);
        let whole = |p: &Path| std::path::absolute(p).map_err(|e| format!("{}: {e}", p.display()));
        // What the level's recipe names: the generator's own recipe, kept in
        // it whole (the file then makes the level on any machine); or, by
        // --gltf, the .glb, from the repo's root where it is under it (as the
        // commands in CLAUDE.md are run). With it, the seed, and the file the
        // importer is given in the recipe's place (the .glb itself, so this
        // works from any folder).
        let (seed, named, src) = if self.gltf {
            let (glb, seed) = autowalk::built_glb(&self.dir)?;
            let (glb, root_whole) = (whole(&glb)?, whole(&root)?);
            (seed, glb.strip_prefix(&root_whole).unwrap_or(&glb).to_string_lossy().replace('\\', "/"), Some(glb))
        } else {
            let (recipe, seed) = autowalk::built_recipe(&self.dir)?;
            (seed.to_string(), recipe, None)
        };
        let via = if self.gltf { autowalk::Via::Gltf(&named) } else { autowalk::Via::Recipe(&named) };
        let code = self.code.clone().unwrap_or_else(|| format!("autolevel{seed}"));
        let name = self.name.clone().unwrap_or_else(|| format!("Autolevel {seed}"));
        let levels = root.join("crates").join("pd_import").join("levels");
        let path = levels.join(format!("{code}.json"));
        let text = autowalk::recipe_text(&code, &name, via);
        if let Ok(old) = std::fs::read_to_string(&path) {
            let group = serde_json::from_str::<serde_json::Value>(&old).ok().and_then(|v| v["group"].as_str().map(str::to_owned));
            if group.as_deref() != Some(autowalk::GROUP) {
                return Err(format!("{} is not a generated level's recipe: name another --code", path.display()));
            }
        }
        std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
        let r = pd_import::recipe::Recipe::load(&path)?;
        let layout = pd_import::layout::Layout::load(&pd_import::layout::Layout::path_for(&path))?.unwrap_or_else(pd_import::layout::Layout::new);
        let custom = std::env::var_os("PD_CUSTOM").map_or_else(|| root.join("custom"), PathBuf::from);
        let paths = pd_import::Paths { assets: assets_dir(&root), custom: custom.clone(), src, levels: Some(levels.clone()) };
        let t = Instant::now();
        for line in pd_import::import(&r, &paths, &layout)? {
            println!("{line}");
        }
        println!("imported in {:.1}s: {} (\"{name}\" in the arena menu's {} group)", t.elapsed().as_secs_f64(), custom.join("stages").join(&code).display(), autowalk::GROUP);
        println!("look:  cargo run --release -p pd_tools --bin pd_snapshot -- out/autolevel/snap stage {code}   (PNGs from its spawn pads; --at x,y,z,theta for one view)");
        println!("again: cargo run --release -p pd_import --bin pd_import -- {code}");
        Ok(())
    }
}

/// `autolevel shots`: what it was asked.
struct Shots {
    dir: PathBuf,
    /// The views to draw, by name (none: all of them).
    names: Vec<String>,
    code: Option<String>,
    root: Option<PathBuf>,
}

impl Shots {
    fn parse(args: &[String]) -> Result<Shots, String> {
        let mut job = Shots { dir: PathBuf::new(), names: Vec::new(), code: None, root: None };
        let mut plain = Vec::new();
        let mut n = 0;
        while n < args.len() {
            let arg = args[n].as_str();
            if !arg.starts_with('-') {
                plain.push(arg.to_string());
                n += 1;
                continue;
            }
            let value = args.get(n + 1).ok_or_else(|| format!("{arg} needs a value"))?;
            match arg {
                "--code" => job.code = Some(value.clone()),
                "--root" => job.root = Some(PathBuf::from(value)),
                _ => return Err(format!("{arg}: not an option of shots (see --help)")),
            }
            n += 2;
        }
        if plain.is_empty() {
            return Err("shots needs the folder of a built level".to_string());
        }
        job.dir = PathBuf::from(plain.remove(0));
        job.names = plain;
        Ok(job)
    }

    /// The views as the game's snapshot tool takes them, `[{"name", "x", "y", "z", "theta"}]`:
    /// those named (all, if none is), from the build folder's `views.json`, each place and
    /// facing as `walk::spawn_string` gives them (the game's F1 panel's feet and theta).
    fn views(&self) -> Result<Vec<serde_json::Value>, String> {
        let path = self.dir.join("views.json");
        let text = std::fs::read_to_string(&path).map_err(|_| format!("no views in {} (no views.json): autolevel build SEED --out {0}", self.dir.display()))?;
        let all: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(name) = self.names.iter().find(|n| !all.contains_key(*n)) {
            let known: Vec<&str> = all.keys().map(String::as_str).take(8).collect();
            return Err(format!("no view {name} in {} (it has {} and others)", path.display(), known.join(", ")));
        }
        let mut out = Vec::new();
        for (name, v) in all.iter().filter(|(name, _)| self.names.is_empty() || self.names.contains(name)) {
            let n: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
            let [x, y, z, tx, ty] = n[..] else { return Err(format!("{}: {name} is not [x, y, z, look x, look y]", path.display())) };
            let spawn = autolevel_build::walk::spawn_string(&autolevel_build::walk::View { x, y, z, tx, ty });
            let at: Vec<f64> = spawn.split(',').filter_map(|v| v.parse().ok()).collect();
            out.push(serde_json::json!({ "name": name, "x": at[0], "y": at[1], "z": at[2], "theta": at[3] }));
        }
        Ok(out)
    }

    /// Write the list and have the game's snapshot tool draw it.
    fn run(&self, cwd: &Path) -> Result<(), String> {
        let root = root(self.root.as_deref(), cwd);
        let (_, seed) = autowalk::built_recipe(&self.dir)?;
        let code = self.code.clone().unwrap_or_else(|| format!("autolevel{seed}"));
        let custom = std::env::var_os("PD_CUSTOM").map_or_else(|| root.join("custom"), PathBuf::from);
        if !custom.join("stages").join(&code).is_dir() {
            return Err(format!("{code} is not in the game yet: autolevel import {} (or name the arena it was imported as with --code)", self.dir.display()));
        }
        let views = self.views()?;
        let out = self.dir.join("shots");
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let list = out.join("views.json");
        std::fs::write(&list, serde_json::to_string_pretty(&views).expect("plain data")).map_err(|e| format!("{}: {e}", list.display()))?;
        let tool = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(format!("pd_snapshot{}", std::env::consts::EXE_SUFFIX)))).filter(|p| p.is_file());
        let tool = tool.ok_or("the game's snapshot tool is not built beside this program: cargo build --release -p pd_tools --bin pd_snapshot")?;
        let t = Instant::now();
        let ran = std::process::Command::new(&tool)
            .arg(&out)
            .args(["stage", &code, "--views"])
            .arg(&list)
            .env("PD_ASSETS", assets_dir(&root))
            .env("PD_CUSTOM", &custom)
            .output()
            .map_err(|e| format!("{}: {e}", tool.display()))?;
        if !ran.status.success() {
            return Err(format!("pd_snapshot drew nothing: {}", String::from_utf8_lossy(&ran.stderr).trim()));
        }
        let drawn = views.iter().filter(|v| out.join(format!("{}.png", v["name"].as_str().unwrap_or_default())).is_file()).count();
        println!("{drawn} of {} views of {code} in {} ({:.1}s; the arena as it was last imported)", views.len(), out.display(), t.elapsed().as_secs_f64());
        if drawn == views.len() {
            Ok(())
        } else {
            Err("some views were not drawn".to_string())
        }
    }
}

