//! The command line: levels from a seed with no model in the loop and no Blender. From
//! `scripts/autolevel.py`: `sweep`, `plan`, `build`, `batch`, `walk`, `shots` and `deadends`,
//! with the level's options as flags, by the names the recipe gives them.
//!
//! Here, not in the program (`pd_import`'s `bin/autolevel.rs`, which adds `import`), so tests
//! reach it: the arguments read into what to do (`parse`), the files a plan is written as
//! (`report_json`, `walks_json`, `views_json`, `options_json`: what the Python's hold, the
//! report's keys in the Python's order), the sweep's table, and where things are looked for
//! (`find_root`, `pd_walk_exe`). `run` does the work: it generates, writes, builds (`geometry`,
//! `mesh`, `bake`, `obj`, `glb`) and walks (`walk`).
//!
//! The walker is the program's own when it has one (`run`'s `inside`: the `autolevel` binary is
//! `pd_import`'s, and walks on `pd_sim`), and pd-walk when that is named. The textures are the
//! game's pool by default (`assets/textures`), turned into the dumps a build embeds (`textures`).
//!
//! What differs from the Python: seeds are scored, and routes walked, on threads (the results in
//! order, so the output is the same every time); a plan's folder also gets `recipe.json`, and
//! `--recipe FILE` makes a level from one; there is no PNG of the plan.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use autolevel::build::Fix;
use autolevel::deadlist;
use autolevel::level::{self, Level, Options};
use autolevel::recipe::{self, Recipe};
use autolevel::report::Report;
use serde_json::{json, Value};

use crate::walk::{self, Outcome, PdWalk, Point, Walker};
use crate::{bake, glb, mesh, obj, svg, textures};

pub const USAGE: &str = "autolevel: Complex-style levels from a seed, with no model in the loop and no Blender

  autolevel sweep --seeds 1-300 --top 8 [--out DIR]     score seeds, draw the best plans
  autolevel plan SEED [--out DIR]                       one seed: plan.svg, report.json, walks.json, views.json,
                                                        options.json, recipe.json
  autolevel build SEED [--out DIR] [--no-bake]          ... then its polygons, the bake, LevelIndices.obj (with .mtl,
                                                        presets and textures) and autolevel_SEED.glb, and walk it
  autolevel batch --seeds 1-60 --top 4 [--out DIR] [--no-bake] [--shots]
                                                        sweep, then build and walk the best four
  autolevel walk DIR [names...] [-v]                    walk a built level again (a route named !x must fail)
  autolevel shots DIR [names...] [--code C]             its views drawn by the game, into DIR/shots (the level must
                                                        be imported: the game's pd_snapshot draws them); with
                                                        --pd-walk, pd-walk's frames of its OBJ instead
  autolevel deadends SEED                               what hung off the level by one way in, and what was done
  autolevel trek SEED [--storeys N] [--out DIR] [--no-bake]
                                                        an atrium N storeys tall (15) with a base on its top storey,
                                                        and a helix of rooms round it that is the only way up: laid
                                                        as a recipe (DIR/recipe.json, for the editor), then built
                                                        and walked as `build` does; DIR is out/autolevel/trek<SEED>
  autolevel import DIR [--code C] [--name N] [--stagenum N] [--gltf]
                                                        a built level into the game, by its recipe (the generator's
                                                        rooms and doorways as rooms and portals; --gltf: its .glb as
                                                        any glTF): crates/pd_import/levels/<code>.json (autolevel<seed>,
                                                        stage 124 unless named) and custom/stages/<code>

What kind of level (sweep, plan, build, batch and deadends all take these):
  --scale grand|standard|towering
                             grand (default): a colossal hall, spiral towers six or seven storeys tall and rooms
                             on the high storeys; standard: the first version's rooms; towering: halls up to
                             fifteen storeys whose high bases are reached only from outside, through other rooms
  --size N                   how big: N times the rooms, on a site N times the area (default 1: 29 rooms)
  --cover mixed|pillars|crates|none    what stands on the floors. pillars run floor to ceiling, crates are
                             low housings, none leaves the floors bare; mixed (default) gives each room
                             pillars or nothing
  --clutter 0..1             how much of it (default 0.5)
  --crates                   let mixed rooms take crates as well (off by default)
  --walkway 1|2              catwalks, bridges and open ramps this many cells wide: 1 (default) is the
                             doorways' 118, 2 the first version's
  --no-shapes                keep every room a plain rectangle (no L-shapes or cross walls)
  --no-washroom              a Felicity level's washrooms bare: no stalls, toilets or basins (on by default)
  --risers N                 enclosed ramp corridors between rooms two storeys apart (switchbacks), default 2
  --vents N                  crawl ducts, first for rooms left with one door (default 4); they slope
                             between storeys
  --windows N                corridors that run past a room open onto it, glazed or as a railed balcony (4)
  --drops N                  holes in a room's floor into the room underneath (2)
  --grates N                 walkable grates in a corridor's floor over a room, with a beam of light through (3)
  --platforms N              big platforms off the catwalks into a hall's empty air: one for each 100 cells
                             of it, times N (default 0: none)
  --galleries N              towering scale: up to N galleries in each towering hall, high storeys with no
                             base given a catwalk of their own, reached only from outside (default 0)
  --holes P                  the chance each platform has a drop through it to what stands under it: a
                             railed hole, or a span crossed by unrailed catwalks (default 0)
  --cellars P                the chance a hall on the ground floor has a cellar under it: a hatch in its
                             floor to drop down, a ladder back up (default 0)
  --angled P                 angled catwalks: the chance a big hall has an island in its empty air reached
                             only by them (spokes), and each of its catwalk storeys a shortcut across its
                             air, to a platform or another catwalk (default 0)
  --flooded P                the chance a room's floor is water, under railed walkways that join its
                             doors, with platforms and angled catwalks over it
  --shaped P                 the chance each room big enough for it is an L or a U: a rectangle cut from
                             it (default 0.1; 0: none)
  --site WxH                 cells; default 80x60 for grand, 64x48 for standard
  --dead-corridor N          a room that goes nowhere may be this many cells of corridor off a main area (8);
  --dead-walk N              and this many cells' walk in all, ramps and decks too (20);
  --dead-climb N             and no climb of this many storeys may end on a deck with no room (2). Anything
                             longer is joined to a room near it, or cut short
  --keep-dead-ends           leave them all as they fall
  (also the recipe's other options by name: --second --loops --shift --level-change --tries)
  --recipe FILE              plan, build, deadends: the level of a recipe (a seed, options and the edits made
                             in the editor) in place of SEED; options on the line then change it

Where things are:
  --out DIR                  default <root>/out/autolevel/seed<N> (sweep and batch: <root>/out/autolevel)
  --root DIR                 the game's repo; default the folder at or above the current one that holds
                             crates/ and assets/
  --textures DIR             the textures a build copies and embeds: the game's pool (<nnnn>.png) or a folder
                             of the Setup Editor's dumps (tempImgEd<NNNN>.bmp); default <root>/assets/textures
  --pd-walk PATH             walk with pd-walk (the Blender MCP repo's viewer/, as a program) in place of this
                             program's own walker; also $PD_WALK. `shots` then draws with it too
  --jobs N                   threads to score seeds and walk routes on; default one for each processor

`build` exits 0 only if the level was built and every walk came out as expected.";

/// The commands, by name.
pub const COMMANDS: [&str; 8] = ["sweep", "plan", "build", "batch", "walk", "shots", "deadends", "trek"];

/// What to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Help,
    /// Score the seeds `lo..=hi` and draw the best `top` plans.
    Sweep {
        seeds: (u64, u64),
        top: usize,
    },
    Plan,
    Build,
    /// Score the seeds, then build and walk the best `top` (and render their views).
    Batch {
        seeds: (u64, u64),
        top: usize,
        shots: bool,
    },
    Walk {
        dir: PathBuf,
        names: Vec<String>,
        verbose: bool,
    },
    Shots {
        dir: PathBuf,
        names: Vec<String>,
    },
    Deadends,
    /// An atrium this many storeys tall and the trek up to its base (`autolevel::trek`), built.
    Trek {
        storeys: usize,
    },
}

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub command: Command,
    /// The level: its seed (plan, build, deadends), its options, and the edits of a recipe file.
    pub recipe: Recipe,
    pub out: Option<PathBuf>,
    /// Bake the lighting (build, batch).
    pub bake: bool,
    pub root: Option<PathBuf>,
    pub textures: Option<PathBuf>,
    pub pd_walk: Option<PathBuf>,
    pub jobs: Option<usize>,
}

/// The options that are yes or no: named alone, they are yes.
const FLAGS: [&str; 4] = ["crates", "shapes", "dead_ends", "washroom"];

/// Does the command `cmd` take the flag `name` (underscores for its hyphens)?
fn takes(cmd: &str, name: &str) -> bool {
    let builds = matches!(cmd, "build" | "batch" | "trek");
    match name {
        "root" | "jobs" => true,
        "seeds" | "top" => matches!(cmd, "sweep" | "batch"),
        "out" => matches!(cmd, "sweep" | "plan" | "build" | "batch" | "trek"),
        "storeys" => cmd == "trek",
        "no_bake" | "textures" => builds,
        "shots" => cmd == "batch",
        "recipe" => matches!(cmd, "plan" | "build" | "deadends"),
        "pd_walk" => builds || matches!(cmd, "walk" | "shots"),
        // (a trek's options are its own: `trek::OPTIONS`)
        _ => !matches!(cmd, "walk" | "shots" | "trek") && (matches!(name, "no_shapes" | "keep_dead_ends" | "no_washroom") || recipe::OPTIONS.contains(&name)),
    }
}

/// Seeds as `LO-HI` (or one seed alone).
fn seeds(text: &str) -> Result<(u64, u64), String> {
    let (lo, hi) = text.split_once('-').unwrap_or((text, text));
    match (lo.trim().parse::<u64>(), hi.trim().parse::<u64>()) {
        (Ok(lo), Ok(hi)) if lo <= hi => Ok((lo, hi)),
        _ => Err(format!("--seeds: a range of seeds as 1-60, not {text}")),
    }
}

/// Read the arguments (the program's name left off). `read` is how a recipe file is read.
pub fn parse(args: &[String], read: &dyn Fn(&str) -> Result<String, String>) -> Result<Args, String> {
    let blank = |command| Args { command, recipe: Recipe::new(0), out: None, bake: true, root: None, textures: None, pd_walk: None, jobs: None };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(blank(Command::Help));
    }
    let names = COMMANDS.join(", ");
    let Some(cmd) = args.first().map(String::as_str) else { return Err(format!("no command: one of {names} (see --help)")) };
    if !COMMANDS.contains(&cmd) {
        return Err(format!("{cmd}: not a command: one of {names} (see --help)"));
    }
    let value = |n: usize| args.get(n + 1).map(String::as_str).ok_or_else(|| format!("{} needs a value", args[n]));
    let number = |n: usize| value(n).and_then(|v| v.parse::<u64>().map_err(|_| format!("{}: a whole number, not {v}", args[n])));
    let mut out = blank(Command::Help);
    // the recipe file first, wherever it is named: the rest changes it
    let from = args.iter().position(|a| a == "--recipe");
    if let Some(n) = from.filter(|_| takes(cmd, "recipe")) {
        let path = value(n)?;
        out.recipe = recipe::load(&read(path)?).map_err(|e| format!("{path}: {e}"))?;
    }
    let (mut plain, mut verbose, mut shots, mut storeys) = (Vec::new(), false, false, autolevel::trek::TrekOptions::default().storeys);
    let (mut range, mut top) = (if cmd == "sweep" { (1, 200) } else { (1, 60) }, if cmd == "sweep" { 8 } else { 4 });
    let mut n = 1;
    while n < args.len() {
        let arg = args[n].as_str();
        if arg == "-v" && cmd == "walk" {
            (verbose, n) = (true, n + 1);
            continue;
        }
        let Some(name) = arg.strip_prefix("--") else {
            if arg.starts_with('-') {
                return Err(format!("{arg}: not an option of {cmd} (see --help)"));
            }
            plain.push(arg);
            n += 1;
            continue;
        };
        let name = name.replace('-', "_");
        if !takes(cmd, &name) {
            return Err(format!("{arg}: not an option of {cmd} (see --help)"));
        }
        let mut took = 2;
        match name.as_str() {
            "recipe" => {}
            "seeds" => range = seeds(value(n)?)?,
            "top" => top = number(n)? as usize,
            "storeys" => storeys = number(n)? as usize,
            "jobs" => match number(n)? {
                0 => return Err("--jobs: at least one thread".to_string()),
                jobs => out.jobs = Some(jobs as usize),
            },
            "out" => out.out = Some(PathBuf::from(value(n)?)),
            "root" => out.root = Some(PathBuf::from(value(n)?)),
            "textures" => out.textures = Some(PathBuf::from(value(n)?)),
            "pd_walk" => out.pd_walk = Some(PathBuf::from(value(n)?)),
            "no_bake" => (out.bake, took) = (false, 1),
            "shots" => (shots, took) = (true, 1),
            "no_shapes" => (out.recipe.options.shapes, took) = (false, 1),
            "no_washroom" => (out.recipe.options.washroom, took) = (false, 1),
            "keep_dead_ends" => (out.recipe.options.dead_ends, took) = (false, 1),
            _ if FLAGS.contains(&name.as_str()) && !matches!(args.get(n + 1).map(String::as_str), Some("true" | "false")) => {
                recipe::set_option(&mut out.recipe.options, &name, "true")?;
                took = 1;
            }
            _ => recipe::set_option(&mut out.recipe.options, &name, value(n)?)?,
        }
        n += took;
    }
    out.recipe.options.check()?;
    let too_many = |most: usize, what: &str| match plain.get(most) {
        Some(extra) => Err(format!("{extra}: {cmd} takes {what}")),
        None => Ok(()),
    };
    out.command = match cmd {
        "sweep" | "batch" => {
            too_many(0, "no names, only options")?;
            if cmd == "sweep" {
                Command::Sweep { seeds: range, top }
            } else {
                Command::Batch { seeds: range, top, shots }
            }
        }
        "walk" | "shots" => {
            let dir = PathBuf::from(plain.first().ok_or_else(|| format!("{cmd} needs the folder of a built level"))?);
            let names = plain[1..].iter().map(|s| s.to_string()).collect();
            if cmd == "walk" {
                Command::Walk { dir, names, verbose }
            } else {
                Command::Shots { dir, names }
            }
        }
        _ => {
            too_many(1, "one seed")?;
            match (plain.first(), from) {
                (Some(seed), _) => out.recipe.seed = seed.parse().map_err(|_| format!("{cmd}: a seed is a whole number, not {seed}"))?,
                (None, Some(_)) => {}
                (None, None) => return Err(format!("{cmd} needs a seed (or --recipe FILE)")),
            }
            match cmd {
                "plan" => Command::Plan,
                "build" => Command::Build,
                "trek" => Command::Trek { storeys },
                _ => Command::Deadends,
            }
        }
    };
    Ok(out)
}

// ---------------------------------------------------------------- where things are

/// What the command line is run in.
#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    /// The current folder.
    pub cwd: PathBuf,
    /// `PD_WALK` in the environment.
    pub pd_walk: Option<String>,
}

/// The game's repo: the nearest folder at or above `from` that holds `crates/` and `assets/`.
pub fn find_root(from: &Path, is_dir: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    from.ancestors().find(|at| is_dir(&at.join("crates")) && is_dir(&at.join("assets"))).map(Path::to_path_buf)
}

/// pd-walk, if it is asked for: the one named on the line, else the environment's.
pub fn pd_walk_exe(named: Option<&Path>, env: Option<&str>) -> Option<PathBuf> {
    match (named, env.filter(|e| !e.is_empty())) {
        (Some(path), _) => Some(path.to_path_buf()),
        (None, Some(path)) => Some(PathBuf::from(path)),
        (None, None) => None,
    }
}

/// What opens a built level's folder for walking in this program, on the game's own movement
/// code. The program that has one gives it to `run`.
pub type Open<'a> = &'a (dyn Fn(&Path) -> Result<Box<dyn Walker>, String> + Sync);

impl Args {
    /// The repo's root: named, or found above the current folder.
    fn root(&self, env: &Env) -> Option<PathBuf> {
        self.root.clone().or_else(|| find_root(&env.cwd, &|p| p.is_dir()))
    }

    /// Where output goes when `--out` does not say: `<root>/out/autolevel`.
    fn work(&self, env: &Env) -> Result<PathBuf, String> {
        match (&self.out, self.root(env)) {
            (Some(out), _) => Ok(out.clone()),
            (None, Some(root)) => Ok(root.join("out").join("autolevel")),
            (None, None) => Err("no --out, and no repo above this folder to put out/autolevel in (name it with --root)".to_string()),
        }
    }

    /// One seed's folder: `--out`, or `seed<N>` in the work folder.
    fn seed_dir(&self, env: &Env, seed: u64) -> Result<PathBuf, String> {
        if let Some(out) = &self.out {
            return Ok(out.clone());
        }
        Ok(self.work(env)?.join(format!("seed{seed}")))
    }

    /// The textures a build copies: named, or the game's pool. Err, saying why, if there are none.
    fn textures(&self, env: &Env) -> Result<PathBuf, String> {
        let dir = match (&self.textures, self.root(env)) {
            (Some(dir), _) => dir.clone(),
            (None, Some(root)) => root.join("assets").join("textures"),
            (None, None) => return Err("no --textures, and no repo above this folder to find assets/textures in".to_string()),
        };
        if dir.is_dir() {
            Ok(dir)
        } else {
            Err(format!("no textures: {} is not a folder", dir.display()))
        }
    }

    /// Is there something to walk with? pd-walk if it is named (Err if it is not where it is
    /// said to be), else the program's own walker (Err if it has none). Asked before a build, so
    /// a level is not made only to find there is nothing to walk it.
    fn can_walk(&self, env: &Env, inside: Option<Open>) -> Result<(), String> {
        match (pd_walk_exe(self.pd_walk.as_deref(), env.pd_walk.as_deref()), inside) {
            (Some(exe), _) => PdWalk::found(&exe),
            (None, Some(_)) => Ok(()),
            (None, None) => Err("no walker: name pd-walk with --pd-walk PATH or in the environment as PD_WALK".to_string()),
        }
    }

    /// The walker, opened on the level built in `dir`: pd-walk if it is named, else the program's own.
    fn walker(&self, env: &Env, dir: &Path, inside: Option<Open>) -> Result<Box<dyn Walker>, String> {
        match (pd_walk_exe(self.pd_walk.as_deref(), env.pd_walk.as_deref()), inside) {
            (Some(exe), _) => Ok(Box::new(PdWalk::open(&exe, dir)?)),
            (None, Some(open)) => open(dir),
            (None, None) => self.can_walk(env, inside).map(|()| unreachable!("no walker is an error")),
        }
    }

    fn jobs(&self) -> usize {
        self.jobs.unwrap_or_else(walk::threads)
    }
}

// ---------------------------------------------------------------- JSON, as the Python writes it

/// JSON whose objects keep the order they are given in (`serde_json`'s sort their keys).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// A number, a string, true, false or null.
    Leaf(Value),
    List(Vec<Json>),
    Dict(Vec<(String, Json)>),
}

impl From<Value> for Json {
    fn from(v: Value) -> Json {
        match v {
            Value::Array(a) => Json::List(a.into_iter().map(Json::from).collect()),
            Value::Object(o) => Json::Dict(o.into_iter().map(|(k, v)| (k, Json::from(v))).collect()),
            leaf => Json::Leaf(leaf),
        }
    }
}

impl Json {
    /// A dict from (name, value) pairs, in that order.
    fn dict<const N: usize>(items: [(&str, Value); N]) -> Json {
        Json::Dict(items.into_iter().map(|(k, v)| (k.to_string(), Json::from(v))).collect())
    }

    /// The value under a name of a dict.
    pub fn get(&self, name: &str) -> Option<&Json> {
        match self {
            Json::Dict(items) => items.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// As text, the way Python's `json.dumps` lays it out: on one line (`, ` and `: ` between
    /// things), or with `indent` spaces a level, one item a line.
    pub fn dump(&self, indent: Option<usize>) -> String {
        let mut out = String::new();
        self.put(&mut out, indent, 0);
        out
    }

    fn put(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        let open = |out: &mut String, depth: usize| {
            if let Some(n) = indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * depth));
            }
        };
        let between = if indent.is_some() { "," } else { ", " };
        match self {
            Json::Leaf(v) => out.push_str(&v.to_string()),
            Json::List(items) if items.is_empty() => out.push_str("[]"),
            Json::Dict(items) if items.is_empty() => out.push_str("{}"),
            Json::List(items) => {
                out.push('[');
                for (n, item) in items.iter().enumerate() {
                    out.push_str(if n > 0 { between } else { "" });
                    open(out, depth + 1);
                    item.put(out, indent, depth + 1);
                }
                open(out, depth);
                out.push(']');
            }
            Json::Dict(items) => {
                out.push('{');
                for (n, (name, item)) in items.iter().enumerate() {
                    out.push_str(if n > 0 { between } else { "" });
                    open(out, depth + 1);
                    out.push_str(&Value::String(name.clone()).to_string());
                    out.push_str(": ");
                    item.put(out, indent, depth + 1);
                }
                open(out, depth);
                out.push('}');
            }
        }
    }
}

/// The names of a JSON object in the order its text has them.
pub fn keys_in_order(text: &str) -> Vec<String> {
    let (mut depth, mut key, mut out) = (0usize, false, Vec::new());
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '{' | '[' => {
                depth += 1;
                key = c == '{' && depth == 1;
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 1 => key = true,
            '"' => {
                // to the quote that closes it
                let (mut end, mut escaped) = (text.len(), false);
                for (j, d) in chars.by_ref() {
                    match d {
                        _ if escaped => escaped = false,
                        '\\' => escaped = true,
                        '"' => {
                            end = j;
                            break;
                        }
                        _ => {}
                    }
                }
                if depth == 1 && key {
                    out.extend(text.get(i..=end).and_then(|s| serde_json::from_str::<String>(s).ok()));
                    key = false;
                }
            }
            _ => {}
        }
    }
    out
}

/// A JSON object's (name, value) pairs in the order its text has them.
pub fn in_order(text: &str) -> Result<Vec<(String, Value)>, String> {
    let Value::Object(mut map) = serde_json::from_str(text).map_err(|e| e.to_string())? else { return Err("not a JSON object".to_string()) };
    let mut out: Vec<(String, Value)> = keys_in_order(text).into_iter().filter_map(|k| map.remove(&k).map(|v| (k, v))).collect();
    out.extend(map);
    Ok(out)
}

/// What was done about a dead end, as the Python's dict.
fn fix_json(f: &Fix) -> Json {
    match f {
        Fix::Joined { room, storey, to, by, kind, length, ramps } => Json::dict([
            ("joined", json!(room)),
            ("storey", json!(storey)),
            ("to", json!(to)),
            ("by", json!(by)),
            ("kind", json!(kind)),
            ("length", json!(length)),
            ("ramps", json!(ramps)),
        ]),
        Fix::Cut { room, above } => Json::dict([("cut", json!(room)), ("above", json!(above))]),
        Fix::Dropped { room } => Json::dict([("dropped", json!(room))]),
    }
}

/// A level's report as the Python's dict, its keys in the Python's order.
pub fn report_json(r: &Report) -> Json {
    let row = |e: &autolevel::deadends::Row| {
        Json::dict([
            ("root", json!(e.root)),
            ("tip", json!(e.tip)),
            ("rooms", json!(e.rooms)),
            ("length", json!(e.length)),
            ("corridor", json!(e.corridor)),
            ("storeys", json!(e.storeys)),
            ("climb", json!(e.climb)),
            ("allowed", json!(e.allowed)),
            ("why", json!(e.why)),
        ])
    };
    let Json::Dict(mut items) = Json::dict([
        ("seed", json!(r.seed)),
        ("scale", json!(r.scale)),
        ("cover", json!(r.cover)),
        ("rooms", json!(r.rooms)),
        ("plan_rooms", json!(r.plan_rooms)),
        ("links", json!(r.links)),
        ("loops", json!(r.loops)),
        ("one_door_rooms", json!(r.one_door_rooms)),
        ("dead_ends", Value::Null),
        ("dead_ends_long", json!(r.dead_ends_long)),
        ("camps", json!(r.camps)),
        ("dead_end_fixes", Value::Null),
        ("hub_share", json!(r.hub_share)),
        ("diameter", json!(r.diameter)),
        ("size_ratio", json!(r.size_ratio)),
        ("largest_over_smallest", json!(r.largest_over_smallest)),
        ("tall_rooms", json!(r.tall_rooms)),
        ("tallest", json!(r.tallest)),
        ("largest", json!(r.largest)),
        ("high_rooms", json!(r.high_rooms)),
        ("spirals", json!(r.spirals)),
        ("spiral_storeys", json!(r.spiral_storeys)),
        ("switchbacks", json!(r.switchbacks)),
        ("bridges", json!(r.bridges)),
        ("layouts", json!(r.layouts)),
        ("shaped", json!(r.shaped)),
        ("pillars", json!(r.pillars)),
        ("crates", json!(r.crates)),
        ("centre_offset", json!(r.centre_offset)),
        ("floors", json!(r.floors)),
        ("extent", json!([r.extent.0, r.extent.1])),
        ("corridor_share", json!(r.corridor_share)),
        ("corridors", json!(r.corridors)),
        ("vents", json!(r.vents)),
        ("sloped_vents", json!(r.sloped_vents)),
        ("overlooks", json!(r.overlooks)),
        ("drops", json!(r.drops)),
        ("lightwells", json!(r.lightwells)),
        ("grates", json!(r.grates)),
        ("stubs", json!(r.stubs)),
        ("corridor_ramps", json!(r.corridor_ramps)),
        ("longest_corridor", json!(r.longest_corridor)),
        ("upper_doors", json!(r.upper_doors)),
        ("catwalk_rooms", json!(r.catwalk_rooms)),
        ("catwalk_cells", json!(r.catwalk_cells)),
        ("ring_rooms", json!(r.ring_rooms)),
        ("room_ramps", json!(r.room_ramps)),
        ("pods", json!(r.pods)),
        ("pods_dropped", json!(r.pods_dropped)),
        ("windows", json!(r.windows)),
        ("pools", json!(r.pools)),
        ("rails", json!(r.rails)),
        ("walks", json!(r.walks)),
        ("score", json!(r.score)),
        // the Rust's own (`RUSTS_OWN`)
        ("platforms", json!(r.platforms)),
        ("galleries", json!(r.galleries)),
        ("holes", json!(r.holes)),
        ("spans", json!(r.spans)),
        ("cellars", json!(r.cellars)),
        ("islands", json!(r.islands)),
        ("angled", json!(r.angled)),
        ("flooded", json!(r.flooded)),
        ("rafts", json!(r.rafts)),
        ("shaped_rooms", json!(r.shaped_rooms)),
        ("empty_air", json!(r.empty_air)),
        ("largest_void", json!(r.largest_void)),
    ]) else {
        unreachable!("a dict is a dict")
    };
    // the two whose own dicts keep their order
    for (name, item) in &mut items {
        match name.as_str() {
            "dead_ends" => *item = Json::List(r.dead_ends.iter().map(row).collect()),
            "dead_end_fixes" => *item = Json::List(r.dead_end_fixes.iter().map(fix_json).collect()),
            _ => {}
        }
    }
    Json::Dict(items)
}

/// The report's keys that are the Rust's own, after the Python's (which end with the score).
pub const RUSTS_OWN: [&str; 12] =
    ["platforms", "galleries", "holes", "spans", "cellars", "islands", "angled", "flooded", "rafts", "shaped_rooms", "empty_air", "largest_void"];

/// A level's walks as walks.json holds them: name -> `[[x, y, z], [x, y, z, flags], ...]`.
pub fn walks_json(lv: &Level) -> Json {
    let step = |s: &level::Step| match s.flags {
        None => json!([s.x, s.y, s.z]),
        Some(flags) => json!([s.x, s.y, s.z, flags]),
    };
    Json::Dict(lv.walks.iter().map(|(name, steps)| (name.clone(), Json::from(Value::Array(steps.iter().map(step).collect())))).collect())
}

/// A level's views as views.json holds them: name -> `[x, y, z, look at x, look at y]`.
pub fn views_json(lv: &Level) -> Json {
    Json::Dict(lv.views.iter().map(|(name, v)| (name.clone(), Json::from(json!([v.0, v.1, v.2, v.3, v.4])))).collect())
}

/// The options options.json holds, in the Python's order (`OPTION_KEYS` in `scripts/autolevel.py`).
pub const OPTION_KEYS: [&str; 17] = [
    "scale",
    "size",
    "cover",
    "clutter",
    "crates",
    "walkway",
    "shapes",
    "risers",
    "vents",
    "windows",
    "drops",
    "grates",
    "site",
    "dead_ends",
    "dead_corridor",
    "dead_walk",
    "dead_climb",
];

/// A level's options as options.json holds them (a site only if one was given).
pub fn options_json(o: &Options) -> Json {
    let mut items = Vec::new();
    for name in OPTION_KEYS {
        let Some(text) = recipe::option(o, name) else { continue };
        let v = match (name, o.siting.site) {
            ("site", Some((w, h))) => json!([w, h]),
            ("site", None) => continue,
            _ => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        };
        items.push((name.to_string(), Json::from(v)));
    }
    Json::Dict(items)
}

// ---------------------------------------------------------------- the sweep's table

/// The report's numbers a sweep prints, in its columns' order.
pub const COLUMNS: [&str; 20] = [
    "score",
    "loops",
    "hub_share",
    "diameter",
    "size_ratio",
    "corridor_share",
    "catwalk_rooms",
    "spirals",
    "switchbacks",
    "high_rooms",
    "bridges",
    "pods",
    "pools",
    "pillars",
    "vents",
    "overlooks",
    "drops",
    "grates",
    "camps",
    "dead_ends_long",
];

/// The table's heading.
pub fn heading() -> String {
    format!("seed  {}", COLUMNS.join("  "))
}

/// A level's row of the table: each number under its column's name, to the right.
pub fn table_row(r: &Report) -> String {
    let all = report_json(r);
    let cell = |name: &str| match all.get(name) {
        Some(Json::Leaf(v)) => format!("{:>1$}", v.to_string(), name.len()),
        _ => " ".repeat(name.len()),
    };
    format!("{:<5} {}", r.seed, COLUMNS.map(cell).join("  "))
}

/// Why a seed made no level, in a word or two: what the generator said, without the seed in
/// front and whatever follows a colon (so the same trouble in two seeds counts as one).
pub fn why(err: &str) -> String {
    let rest = err.split_once(": ").map_or(err, |(_, rest)| rest);
    rest.split(':').next().unwrap_or(rest).to_string()
}

/// The seeds that made no level, counted by why, as Python prints a dict: `{'vast': 2, 'colossal': 1}`.
pub fn failed(errors: &[String]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for e in errors {
        let w = why(e);
        match counts.iter_mut().find(|(k, _)| *k == w) {
            Some((_, n)) => *n += 1,
            None => counts.push((w, 1)),
        }
    }
    format!("{{{}}}", counts.iter().map(|(k, n)| format!("{}: {n}", autolevel::py::repr(k))).collect::<Vec<_>>().join(", "))
}

/// The level's numbers in a line, as the Python titles its plan.
pub fn title(lv: &Level) -> String {
    let r = &lv.report;
    format!("autolevel seed {}: {} rooms, {} links, {} loops, score {:?}", lv.seed, r.rooms, r.links, r.loops, r.score)
}

// ---------------------------------------------------------------- doing it

fn put(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

/// plan.svg, report.json, walks.json, views.json, options.json and recipe.json for one level,
/// into `out`. The plan's path.
pub fn write_plan(lv: &Level, recipe: &Recipe, out: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let plan = out.join("plan.svg");
    put(&plan, &svg::svg(&lv.plan, &svg::PANELS, svg::SCALE, &title(lv)))?;
    put(&out.join("report.json"), &report_json(&lv.report).dump(Some(1)))?;
    put(&out.join("walks.json"), &walks_json(lv).dump(None))?;
    put(&out.join("views.json"), &views_json(lv).dump(None))?;
    put(&out.join("options.json"), &options_json(&lv.options).dump(None))?;
    put(&out.join("recipe.json"), &recipe::save(recipe))?;
    Ok(plan)
}

/// Every seed of a range scored, on threads: (seed, its report or why it made no level), in seed order.
fn score(seeds: (u64, u64), o: &Options, jobs: usize) -> Vec<(u64, Result<Report, String>)> {
    let count = (seeds.1 - seeds.0 + 1) as usize;
    walk::parallel(count, jobs, |n| {
        let seed = seeds.0 + n as u64;
        (seed, level::generate(seed, o).map(|lv| lv.report))
    })
}

/// The reports of the seeds that made a level, the best first (of two as good, the lower seed).
fn best(scored: &[(u64, Result<Report, String>)]) -> Vec<&Report> {
    let mut rows: Vec<&Report> = scored.iter().filter_map(|(_, r)| r.as_ref().ok()).collect();
    rows.sort_by(|a, b| autolevel::py::cmp_f64(b.score, a.score));
    rows
}

/// A stub's answer, from a stage that is still to be written: said, and not held against the build.
fn unwritten(why: &str) -> bool {
    why.contains("not written yet")
}

/// One level, from its recipe to its files in `out`, then walked. True if it was built whole
/// and every walk came out as expected.
fn build(a: &Args, env: &Env, recipe: &Recipe, out: &Path, quiet: bool, inside: Option<Open>) -> Result<bool, String> {
    a.can_walk(env, inside)?;
    let t = Instant::now();
    let lv = recipe.generate()?;
    let plan = write_plan(&lv, recipe, out)?;
    let planned = secs(t);
    if !quiet {
        println!("{}", plan.display());
        println!("{}", report_json(&lv.report).dump(Some(1)));
    }
    let kit = crate::kit::named(lv.options.style)?;
    let (polys, lit) = (kit.polys)(&lv, kit)?;
    let mut rooms = mesh::emit(&lv, &polys, kit)?;
    let meshed = secs(t);
    let mut sound = true;
    if a.bake {
        match bake::light(&lv, &mut rooms, &lit, kit, &bake::Settings::default()) {
            Ok(gains) => println!("baked: {} rooms", gains.len()),
            Err(why) if unwritten(&why) => println!("not baked: {why}"),
            Err(why) => {
                println!("BAKE FAILED: {why}");
                sound = false;
            }
        }
    }
    let baked = secs(t);
    // the presets this level draws, in the order its rooms first use them
    let presets = kit.presets(rooms.iter().flat_map(|r| r.faces.iter().map(|f| f.material)));
    let textures = match a.textures(env).and_then(|dir| textures::dumps(&dir, out, &presets)) {
        Ok(dir) => Some(dir),
        Err(why) => {
            println!("warning: {why}");
            None
        }
    };
    let report = obj::write(&out.join("LevelIndices.obj"), &rooms, kit, textures.as_deref())?;
    println!("built: {} rooms, {} triangles, {} materials", report.rooms, report.triangles, report.materials.len());
    for w in &report.warnings {
        println!("warning: {w}");
    }
    let name = format!("autolevel_{}", lv.seed);
    match glb::write(&out.join(format!("{name}.glb")), &rooms, kit, &name, textures.as_deref()) {
        Ok(g) => {
            println!("saved: {} ({} rooms, {} materials, {} bytes)", out.join(format!("{name}.glb")).display(), g.rooms, g.materials, g.bytes);
            for w in &g.warnings {
                println!("warning: {w}");
            }
        }
        Err(why) if unwritten(&why) => println!("no glb: {why}"),
        Err(why) => {
            println!("GLB FAILED: {why}");
            sound = false;
        }
    }
    println!(
        "took {:.1}s: plan {planned:.2}s, polygons and meshes {:.2}s, bake {:.2}s, files {:.2}s",
        secs(t),
        meshed - planned,
        baked - meshed,
        secs(t) - baked
    );
    let walker = a.walker(env, out, inside)?;
    Ok(walk_dir(out, &[], false, walker.as_ref(), a.jobs())? && sound)
}

/// One line for a route's outcome: OK or FAIL, and where it stuck.
pub fn outcome_line(o: &Outcome, pts: &[Point]) -> String {
    let mark = if o.ok { "OK" } else { "FAIL" };
    match (&o.walked, o.stuck()) {
        (Err(why), _) => format!("{}: {mark} {why}", o.name),
        (Ok(w), Some(n)) => {
            let p = pts[(n + 1).min(pts.len() - 1)];
            format!("{}: {mark} stuck before point {} ({:?}, {:?}, {}): {}", o.name, n + 1, p.x, p.y, p.z, w.reports[n])
        }
        (Ok(_), None) => format!("{}: {mark}", o.name),
    }
}

/// The routes of a walks.json, in the order it lists them.
pub fn routes_from(text: &str) -> Result<Vec<(String, Vec<Point>)>, String> {
    in_order(text)?.into_iter().map(|(name, v)| walk::route_from(&v).map(|pts| (name.clone(), pts)).map_err(|e| format!("{name}: {e}"))).collect()
}

/// Those of `all` that are named, in the order they are named (none named: all). Err for a name that is not there.
fn chosen<T: Clone>(all: Vec<(String, T)>, names: &[String], what: &str) -> Result<Vec<(String, T)>, String> {
    if names.is_empty() {
        return Ok(all);
    }
    names.iter().map(|n| all.iter().find(|(k, _)| k == n).cloned().ok_or_else(|| format!("no {what} named {n}"))).collect()
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Walk a built level's routes (those named, or all of walks.json), print the ones that did not
/// come out as expected (with `verbose`, all), and write walk_results.json. True if all did.
pub fn walk_dir(dir: &Path, names: &[String], verbose: bool, walker: &dyn Walker, jobs: usize) -> Result<bool, String> {
    let path = dir.join("walks.json");
    let routes = chosen(routes_from(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?, names, "walk")?;
    let t = Instant::now();
    let outcomes = walk::walk_all(walker, &routes, jobs);
    for (o, (_, pts)) in outcomes.iter().zip(&routes) {
        if !o.ok || verbose {
            println!("{}", outcome_line(o, pts));
        }
    }
    let good = outcomes.iter().filter(|o| o.ok).count();
    println!("walked in {:.1}s on {} threads", secs(t), jobs.clamp(1, routes.len().max(1)));
    println!("{good} of {} walks as expected", routes.len());
    let results = Json::Dict(outcomes.iter().map(|o| (o.name.clone(), Json::Leaf(json!(o.ok)))).collect());
    put(&dir.join("walk_results.json"), &results.dump(Some(1)))?;
    Ok(good == routes.len())
}

/// Render a built level's views (those named, or all of views.json) into `dir/shots`, and a
/// contact sheet if ImageMagick is there to make one. True if every frame was rendered.
pub fn shots_dir(dir: &Path, names: &[String], walker: &dyn Walker, jobs: usize) -> Result<bool, String> {
    let path = dir.join("views.json");
    let mut views = Vec::new();
    for (name, v) in in_order(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))? {
        views.push((name.clone(), walk::view_from(&v).map_err(|e| format!("{}: {name}: {e}", path.display()))?));
    }
    // (the Python leaves out names it does not know; here they are said)
    let views = chosen(views, names, "view")?;
    let out = dir.join("shots");
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    // (each frame is a renderer of its own on the graphics card: a few at a time)
    let shots = walk::shoot_all(walker, &views, &out, jobs.min(4));
    let mut files = Vec::new();
    for (name, shot) in &shots {
        match shot {
            Ok(png) => {
                println!("{name} ok");
                files.push(png.clone());
            }
            Err(why) => println!("{name} FAILED: {why}"),
        }
    }
    println!("{} of {} frames in {}", files.len(), shots.len(), out.display());
    if !files.is_empty() {
        let sheet = out.join("sheet.jpg");
        let montage = std::process::Command::new("magick")
            .arg("montage")
            .args(&files)
            .args(["-tile", "4x", "-geometry", "640x360+4+4", "-background", "black"])
            .arg(&sheet)
            .output();
        match montage {
            Ok(done) if done.status.success() => println!("sheet: {}", sheet.display()),
            Ok(done) => println!("no contact sheet: {}", String::from_utf8_lossy(&done.stderr).trim()),
            Err(_) => println!("no contact sheet: ImageMagick (magick) is not on the path"),
        }
    }
    Ok(files.len() == shots.len())
}

/// Score a range of seeds, print the table of the best, and write their plans.
fn sweep(a: &Args, env: &Env, seeds: (u64, u64), top: usize) -> Result<bool, String> {
    let out = a.work(env)?;
    let t = Instant::now();
    let scored = score(seeds, &a.recipe.options, a.jobs());
    let errors: Vec<String> = scored.iter().filter_map(|(_, r)| r.as_ref().err().cloned()).collect();
    let rows = best(&scored);
    println!("{} of {} seeds made a level in {:.1}s; could not finish: {}", rows.len(), scored.len(), secs(t), failed(&errors));
    println!("{}", heading());
    let rows = &rows[..top.min(rows.len())];
    for r in rows {
        println!("{}", table_row(r));
    }
    // the best ones' plans (a sweep keeps each seed's numbers, not its level: they are made again)
    let written = walk::parallel(rows.len(), a.jobs(), |n| {
        let recipe = Recipe { seed: rows[n].seed, options: a.recipe.options.clone(), edits: Vec::new() };
        recipe.generate().and_then(|lv| write_plan(&lv, &recipe, &out.join(format!("seed{}", lv.seed))))
    });
    written.into_iter().collect::<Result<Vec<_>, _>>()?;
    println!("plans in {}", out.display());
    Ok(true)
}

/// Score a range of seeds, then build and walk the best few: levels with nobody in the loop.
fn batch(a: &Args, env: &Env, seeds: (u64, u64), top: usize, shots: bool, inside: Option<Open>) -> Result<bool, String> {
    let out = a.work(env)?;
    a.can_walk(env, inside)?;
    let t = Instant::now();
    let scored = score(seeds, &a.recipe.options, a.jobs());
    let rows = best(&scored);
    println!("{} of {} seeds made a level in {:.1}s; building the best {top}", rows.len(), scored.len(), secs(t));
    let rows = &rows[..top.min(rows.len())];
    let mut good = 0;
    for r in rows {
        println!("== seed {} (score {:?})", r.seed, r.score);
        let dir = out.join(format!("seed{}", r.seed));
        let recipe = Recipe { seed: r.seed, options: a.recipe.options.clone(), edits: Vec::new() };
        match build(a, env, &recipe, &dir, true, inside) {
            Ok(ok) => good += ok as usize,
            Err(why) => println!("BUILD FAILED: {why}"),
        }
        if shots && dir.join("LevelIndices.obj").is_file() {
            shots_dir(&dir, &[], a.walker(env, &dir, inside)?.as_ref(), a.jobs())?;
        }
    }
    println!("{good} of {} levels built and walked clean, in {} ({:.1}s in all)", rows.len(), out.display(), secs(t));
    Ok(good == rows.len())
}

/// Do what the arguments ask. False: it was done, and something did not come out as expected
/// (a walk, a build); Err: it could not be done. `inside` is the program's own walker, if it has
/// one: what levels are walked with unless pd-walk is named.
pub fn run(a: &Args, env: &Env, inside: Option<Open>) -> Result<bool, String> {
    match &a.command {
        Command::Help => {
            println!("{USAGE}");
            Ok(true)
        }
        Command::Sweep { seeds, top } => sweep(a, env, *seeds, *top),
        Command::Batch { seeds, top, shots } => batch(a, env, *seeds, *top, *shots, inside),
        Command::Plan => {
            let lv = a.recipe.generate()?;
            let plan = write_plan(&lv, &a.recipe, &a.seed_dir(env, lv.seed)?)?;
            println!("{}", plan.display());
            println!("{}", report_json(&lv.report).dump(Some(1)));
            Ok(true)
        }
        Command::Build => build(a, env, &a.recipe, &a.seed_dir(env, a.recipe.seed)?, false, inside),
        Command::Walk { dir, names, verbose } => walk_dir(dir, names, *verbose, a.walker(env, dir, inside)?.as_ref(), a.jobs()),
        Command::Shots { dir, names } => shots_dir(dir, names, a.walker(env, dir, inside)?.as_ref(), a.jobs()),
        Command::Deadends => {
            print!("{}", deadlist::make(&a.recipe)?.1.text());
            Ok(true)
        }
        Command::Trek { storeys } => {
            let seed = a.recipe.seed;
            let (trek, lv) = autolevel::trek::lay(seed, &autolevel::trek::TrekOptions { storeys: *storeys, ..Default::default() })?;
            let way = autolevel::trek::climb(&lv).unwrap_or_default();
            println!("trek {seed} (try {}): {} rooms; the way up: {}", trek.attempt, trek.rooms.len(), way.join(" > "));
            let dir = match &a.out {
                Some(out) => out.clone(),
                None => a.work(env)?.join(format!("trek{seed}")),
            };
            build(a, env, &trek.recipe, &dir, false, inside)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::Walked;
    use autolevel::place::Scale;

    fn read(path: &str) -> Result<String, String> {
        match path {
            "seven.json" => {
                Ok("{\"format\": 1, \"seed\": 7, \"options\": {\"vents\": 0}, \"edits\": [{\"edit\": \"remove\", \"route\": \"c07\"}]}".to_string())
            }
            _ => Err(format!("{path}: no such file")),
        }
    }

    fn args(line: &str) -> Result<Args, String> {
        let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        parse(&words, &read)
    }

    #[test]
    fn commands_and_what_they_take() {
        let a = args("sweep").unwrap();
        assert_eq!(
            (a.command.clone(), a.recipe.options.clone(), a.out.clone(), a.bake),
            (Command::Sweep { seeds: (1, 200), top: 8 }, Options::default(), None, true)
        );
        let a = args("sweep --seeds 1-300 --top 5 --out plans --jobs 3").unwrap();
        assert_eq!((a.command, a.out, a.jobs), (Command::Sweep { seeds: (1, 300), top: 5 }, Some(PathBuf::from("plans")), Some(3)));
        assert_eq!(args("batch").unwrap().command, Command::Batch { seeds: (1, 60), top: 4, shots: false });
        let a = args("batch --seeds 1-12 --top 3 --no-bake --shots --out ../work/b --textures tex --pd-walk bin/pd-walk").unwrap();
        assert_eq!((a.command, a.bake), (Command::Batch { seeds: (1, 12), top: 3, shots: true }, false));
        assert_eq!((a.textures, a.pd_walk), (Some(PathBuf::from("tex")), Some(PathBuf::from("bin/pd-walk"))));
        assert_eq!(args("sweep --seeds 9").unwrap().command, Command::Sweep { seeds: (9, 9), top: 8 });
        let a = args("plan 40 --out here").unwrap();
        assert_eq!((a.command, a.recipe, a.out), (Command::Plan, Recipe::new(40), Some(PathBuf::from("here"))));
        let a = args("build --no-bake 14 --root /repo").unwrap();
        assert_eq!((a.command, a.recipe.seed, a.bake, a.root), (Command::Build, 14, false, Some(PathBuf::from("/repo"))));
        assert_eq!((args("deadends 7").unwrap().command, args("deadends 7").unwrap().recipe.seed), (Command::Deadends, 7));
        let a = args("trek 5 --storeys 12 --no-bake").unwrap();
        assert_eq!((a.command, a.recipe.seed, a.bake), (Command::Trek { storeys: 12 }, 5, false));
        assert_eq!(args("trek 5").unwrap().command, Command::Trek { storeys: 15 });
        assert!(args("trek 5 --loops 2").is_err(), "a trek's options are its own");
        assert!(args("build 5 --storeys 3").is_err());
        let a = args("walk out/seed40 !pool_basin link_c01_a_b -v").unwrap();
        assert_eq!(
            a.command,
            Command::Walk { dir: PathBuf::from("out/seed40"), names: vec!["!pool_basin".to_string(), "link_c01_a_b".to_string()], verbose: true }
        );
        assert_eq!(args("walk out/seed40").unwrap().command, Command::Walk { dir: PathBuf::from("out/seed40"), names: vec![], verbose: false });
        assert_eq!(
            args("shots out/seed40 vast_door").unwrap().command,
            Command::Shots { dir: PathBuf::from("out/seed40"), names: vec!["vast_door".to_string()] }
        );
        assert_eq!(args("build 4 --help").unwrap().command, Command::Help);
        assert_eq!(args("-h").unwrap().command, Command::Help);
    }

    #[test]
    fn the_levels_options_are_the_pythons_flags() {
        let a = args(
            "build 9 --scale standard --size 2 --cover pillars --clutter 1.0 --crates --walkway 2 --no-shapes --risers 3 --vents 5 --windows 6 --drops 1 --grates 0 \
             --site 70x50 --dead-corridor 6 --dead-walk 15 --dead-climb 3 --keep-dead-ends",
        )
        .unwrap();
        let o = &a.recipe.options;
        assert_eq!(
            (o.siting.scale, o.siting.size, o.siting.site, o.cover, o.clutter, o.crates, o.walkway, o.shapes),
            (Scale::Standard, 2.0, Some((70, 50)), "pillars", 1.0, true, 2, false)
        );
        assert_eq!((o.risers, o.vents, o.windows, o.drops, o.grates), (3, 5, 6, 1, 0));
        assert_eq!((o.dead_corridor, o.dead_walk, o.dead_climb, o.dead_ends), (6, 15, 3, false));
        // the same on every command that makes a level, and the recipe's other options by name
        for cmd in ["sweep", "plan 3", "batch", "deadends 3"] {
            let o = args(&format!("{cmd} --cover none --walkway 2 --second 0 --loops 0.2")).unwrap().recipe.options;
            assert_eq!((o.cover, o.walkway, o.second, o.loops), ("none", 2, 0, 0.2), "{cmd}");
        }
        assert_eq!(args("plan 3").unwrap().recipe.options, Options::default());
        let o = args("plan 3 --crates false --shapes false --dead_ends true").unwrap().recipe.options;
        assert_eq!((o.crates, o.shapes, o.dead_ends), (false, false, true));
    }

    #[test]
    fn a_recipe_file_in_place_of_a_seed() {
        let a = args("build --vents 2 --recipe seven.json").unwrap();
        assert_eq!((a.command, a.recipe.seed, a.recipe.options.vents, a.recipe.edits.len()), (Command::Build, 7, 2, 1));
        // a seed on the line as well is the seed
        assert_eq!(args("plan 12 --recipe seven.json").unwrap().recipe.seed, 12);
        assert_eq!(args("deadends --recipe none.json").unwrap_err(), "none.json: no such file");
        assert_eq!(args("sweep --recipe seven.json").unwrap_err(), "--recipe: not an option of sweep (see --help)");
    }

    #[test]
    fn what_it_cannot_read_it_says() {
        assert_eq!(args("").unwrap_err(), "no command: one of sweep, plan, build, batch, walk, shots, deadends, trek (see --help)");
        assert_eq!(args("make 4").unwrap_err(), "make: not a command: one of sweep, plan, build, batch, walk, shots, deadends, trek (see --help)");
        assert_eq!(args("plan").unwrap_err(), "plan needs a seed (or --recipe FILE)");
        assert_eq!(args("plan forty").unwrap_err(), "plan: a seed is a whole number, not forty");
        assert_eq!(args("plan 4 5").unwrap_err(), "5: plan takes one seed");
        assert_eq!(args("sweep 4").unwrap_err(), "4: sweep takes no names, only options");
        assert_eq!(args("walk").unwrap_err(), "walk needs the folder of a built level");
        assert_eq!(args("plan 4 --sead 4").unwrap_err(), "--sead: not an option of plan (see --help)");
        assert_eq!(args("plan 4 --top 3").unwrap_err(), "--top: not an option of plan (see --help)");
        assert_eq!(args("plan 4 --no-bake").unwrap_err(), "--no-bake: not an option of plan (see --help)");
        assert_eq!(args("walk dir --cover none").unwrap_err(), "--cover: not an option of walk (see --help)");
        assert_eq!(args("shots dir -v").unwrap_err(), "-v: not an option of shots (see --help)");
        assert_eq!(args("build 4 --out").unwrap_err(), "--out needs a value");
        assert_eq!(args("sweep --top many").unwrap_err(), "--top: a whole number, not many");
        assert_eq!(args("sweep --seeds 9-3").unwrap_err(), "--seeds: a range of seeds as 1-60, not 9-3");
        assert_eq!(args("sweep --jobs 0").unwrap_err(), "--jobs: at least one thread");
        assert_eq!(args("sweep --jobs").unwrap_err(), "--jobs needs a value");
        assert_eq!(args("plan 4 --cover hedges").unwrap_err(), "cover: one of ('mixed', 'pillars', 'crates', 'none')");
        assert_eq!(args("plan 4 --scale huge").unwrap_err(), "scale: one of ('grand', 'standard', 'towering')");
        assert_eq!(args("plan 4 --walkway 3").unwrap_err(), "walkway: 1 or 2 cells");
        assert_eq!(args("plan 4 --site wide").unwrap_err(), "site: cells wide by cells deep, as 80x60, not wide");
    }

    #[test]
    fn where_things_are_looked_for() {
        let dirs = ["/repo/crates", "/repo/assets", "/repo/crates/assets", "/other/crates"].map(PathBuf::from);
        let is_dir = |p: &Path| dirs.iter().any(|d| d == p);
        assert_eq!(find_root(Path::new("/repo/crates/autolevel"), &is_dir), Some(PathBuf::from("/repo")));
        assert_eq!(find_root(Path::new("/repo"), &is_dir), Some(PathBuf::from("/repo")));
        assert_eq!(find_root(Path::new("/other/crates"), &is_dir), None);
        // pd-walk, when it is asked for: the line's, then the environment's
        assert_eq!(pd_walk_exe(Some(Path::new("a/pd-walk")), Some("b/pd-walk")), Some(PathBuf::from("a/pd-walk")));
        assert_eq!(pd_walk_exe(None, Some("b/pd-walk")), Some(PathBuf::from("b/pd-walk")));
        assert_eq!(pd_walk_exe(None, Some("")), None);
        assert_eq!(pd_walk_exe(None, None), None);
        // output: --out, or the repo's scratch folder; a seed's own folder under it
        let env = Env { cwd: PathBuf::from("/nowhere/at/all"), pd_walk: None };
        let a = args("plan 40 --root /repo").unwrap();
        assert_eq!(a.seed_dir(&env, 40), Ok(Path::new("/repo").join("out").join("autolevel").join("seed40")));
        assert_eq!(args("plan 40 --out here").unwrap().seed_dir(&env, 40), Ok(PathBuf::from("here")));
        assert_eq!(args("sweep --out here").unwrap().work(&env), Ok(PathBuf::from("here")));
        assert!(args("sweep").unwrap().work(&env).unwrap_err().contains("--root"));
        assert!(args("build 4 --root /repo").unwrap().textures(&env).unwrap_err().contains("assets"));
        // the walker: pd-walk if it is named, else the program's own, else none
        struct Still;
        impl Walker for Still {
            fn walk(&self, _: &[Point]) -> Result<walk::Walked, String> {
                Ok(walk::Walked { reached: true, reports: Vec::new() })
            }
            fn shot(&self, _: &walk::View, _: &Path) -> Result<(), String> {
                Err("no renderer".to_string())
            }
        }
        let own = |_: &Path| -> Result<Box<dyn Walker>, String> { Ok(Box::new(Still)) };
        let dir = Path::new("no/such/level");
        assert!(args("build 4").unwrap().can_walk(&env, None).unwrap_err().contains("--pd-walk PATH"));
        assert!(args("build 4").unwrap().walker(&env, dir, None).is_err());
        assert_eq!(args("build 4").unwrap().can_walk(&env, Some(&own)), Ok(()));
        assert!(args("build 4").unwrap().walker(&env, dir, Some(&own)).is_ok_and(|w| w.walk(&[]).is_ok_and(|w| w.reached)));
        let named = args("build 4 --pd-walk no/such/pd-walk").unwrap();
        assert!(named.can_walk(&env, Some(&own)).unwrap_err().starts_with("pd-walk is not at no"));
        assert!(named.walker(&env, dir, Some(&own)).is_err_and(|why| why.starts_with("pd-walk is not at no")));
        let by_env = Env { cwd: PathBuf::from("/nowhere/at/all"), pd_walk: Some("env/pd-walk".to_string()) };
        assert!(args("build 4").unwrap().can_walk(&by_env, Some(&own)).unwrap_err().starts_with("pd-walk is not at env"));
    }

    #[test]
    fn json_is_laid_out_as_python_lays_it_out() {
        let j = Json::dict([
            ("seed", json!(40)),
            ("scale", json!("grand")),
            ("rooms", json!(["b", "a"])),
            ("why", json!([])),
            ("length", json!(20.0)),
            ("share", json!(0.2)),
        ]);
        // json.dumps(d): one line, the keys in the order given (not sorted)
        assert_eq!(j.dump(None), "{\"seed\": 40, \"scale\": \"grand\", \"rooms\": [\"b\", \"a\"], \"why\": [], \"length\": 20.0, \"share\": 0.2}");
        // json.dumps(d, indent=1)
        assert_eq!(
            j.dump(Some(1)),
            "{\n \"seed\": 40,\n \"scale\": \"grand\",\n \"rooms\": [\n  \"b\",\n  \"a\"\n ],\n \"why\": [],\n \"length\": 20.0,\n \"share\": 0.2\n}"
        );
        let nested = Json::Dict(vec![
            ("fixes".to_string(), Json::List(vec![Json::dict([("cut", json!("spire1")), ("above", json!(827))])])),
            ("none".to_string(), Json::Dict(vec![])),
        ]);
        assert_eq!(nested.dump(Some(1)), "{\n \"fixes\": [\n  {\n   \"cut\": \"spire1\",\n   \"above\": 827\n  }\n ],\n \"none\": {}\n}");
        assert_eq!(nested.dump(None), "{\"fixes\": [{\"cut\": \"spire1\", \"above\": 827}], \"none\": {}}");
        assert_eq!(Json::from(json!({"b": 1, "a": [true, null, "x\"y"]})).dump(None), "{\"a\": [true, null, \"x\\\"y\"], \"b\": 1}");
        // and read back in the order the text has it
        let text = "{\"link_c02\": [[1.5, 2, 0]], \"!pool_basin\": [[3, 4, 0], [5, 6, -118]], \"a \\\"b\\\", {c}\": {\"zz\": [1, {\"k\": 2}]}, \"back\": []}";
        assert_eq!(keys_in_order(text), ["link_c02", "!pool_basin", "a \"b\", {c}", "back"]);
        assert_eq!(in_order(text).unwrap().iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["link_c02", "!pool_basin", "a \"b\", {c}", "back"]);
        assert_eq!(keys_in_order("[1, 2]"), Vec::<String>::new());
        assert!(in_order("[1, 2]").is_err() && in_order("{").is_err());
        let routes = routes_from("{\"b\": [[1.5, 2, 0], [2.5, 2, 0, 1]], \"a\": [[0, 0, 276]]}").unwrap();
        assert_eq!(routes.iter().map(|(k, pts)| (k.as_str(), pts.len())).collect::<Vec<_>>(), [("b", 2), ("a", 1)]);
        assert_eq!(routes_from("{\"b\": [[1.5, 2]]}").unwrap_err(), "b: a point is [x, y, z] or [x, y, z, flags], not [1.5,2]");
    }

    #[test]
    fn a_report_and_the_options_are_the_pythons_dicts() {
        let row = autolevel::deadends::Row {
            root: "vast@276".into(),
            tip: "small8".into(),
            rooms: vec!["small8".into()],
            length: 9.5,
            corridor: 0.0,
            storeys: 1,
            climb: 0,
            allowed: true,
            why: vec![],
        };
        let fixes = vec![
            Fix::Joined { room: "tower".into(), storey: 551, to: "colossal".into(), by: "c42".into(), kind: "corridor", length: 6.0, ramps: 0 },
            Fix::Cut { room: "spire1".into(), above: 827 },
            Fix::Dropped { room: "tiny4".into() },
        ];
        let r = Report {
            seed: 40,
            scale: "grand",
            cover: 13,
            rooms: 29,
            dead_ends: vec![row],
            dead_end_fixes: fixes,
            score: 37.85,
            size_ratio: 18.0,
            hub_share: 0.2,
            extent: (65, 50),
            ..Report::default()
        };
        let j = report_json(&r);
        let Json::Dict(items) = &j else { panic!("a report is a dict") };
        // the Python's keys, in its order: "cover" third (it first held the option), the score last;
        // then the Rust's own
        let names: Vec<&str> = items.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!((names.len(), &names[..5], names[53]), (54 + RUSTS_OWN.len(), &["seed", "scale", "cover", "rooms", "plan_rooms"][..], "score"));
        assert_eq!(names[54..], RUSTS_OWN);
        assert_eq!(
            j.get("dead_ends").unwrap().dump(None),
            "[{\"root\": \"vast@276\", \"tip\": \"small8\", \"rooms\": [\"small8\"], \"length\": 9.5, \"corridor\": 0.0, \"storeys\": 1, \"climb\": 0, \"allowed\": true, \"why\": []}]"
        );
        assert_eq!(
            j.get("dead_end_fixes").unwrap().dump(None),
            "[{\"joined\": \"tower\", \"storey\": 551, \"to\": \"colossal\", \"by\": \"c42\", \"kind\": \"corridor\", \"length\": 6.0, \"ramps\": 0}, \
             {\"cut\": \"spire1\", \"above\": 827}, {\"dropped\": \"tiny4\"}]"
        );
        assert_eq!(j.get("extent").unwrap().dump(None), "[65, 50]");
        assert!(j.dump(Some(1)).starts_with("{\n \"seed\": 40,\n \"scale\": \"grand\",\n \"cover\": 13,\n \"rooms\": 29,\n"));
        // the sweep's row: Python's str() of each number, under its column
        assert_eq!(heading(), "seed  score  loops  hub_share  diameter  size_ratio  corridor_share  catwalk_rooms  spirals  switchbacks  high_rooms  bridges  pods  pools  pillars  vents  overlooks  drops  grates  camps  dead_ends_long");
        let line = table_row(&r);
        assert!(line.starts_with("40    37.85      0        0.2         0        18.0             0.0              0        0"), "{line}");
        assert_eq!(line.len(), heading().len());
        // options.json: the Python's keys in its order; a site only when one is given
        assert_eq!(
            options_json(&Options::default()).dump(None),
            "{\"scale\": \"grand\", \"size\": 1.0, \"cover\": \"mixed\", \"clutter\": 0.5, \"crates\": false, \"walkway\": 1, \"shapes\": true, \"risers\": 2, \"vents\": 4, \
             \"windows\": 4, \"drops\": 2, \"grates\": 3, \"dead_ends\": true, \"dead_corridor\": 8, \"dead_walk\": 20, \"dead_climb\": 2}"
        );
        let o = args("plan 1 --site 70x50 --scale standard --keep-dead-ends").unwrap().recipe.options;
        let text = options_json(&o).dump(None);
        assert!(text.starts_with("{\"scale\": \"standard\", ") && text.contains("\"grates\": 3, \"site\": [70, 50], \"dead_ends\": false, "), "{text}");
    }

    /// Seed 40's plan, written and read back. The numbers are those of the files the Python
    /// wrote for the same seed (work/autolevel/ref/seed40, which is not kept in the repo).
    #[test]
    fn a_plans_files_hold_the_level() {
        let recipe = Recipe::python(40);
        let lv = recipe.generate().unwrap();
        let dir = std::env::temp_dir().join(format!("autolevel_cli_plan_{}", std::process::id()));
        let plan = write_plan(&lv, &recipe, &dir).unwrap();
        let file = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
        assert_eq!(title(&lv), "autolevel seed 40: 29 rooms, 44 links, 16 loops, score 37.85");
        let drawn = fs::read_to_string(&plan).unwrap();
        assert_eq!((svg::counts(&drawn), drawn.len(), drawn.contains(&title(&lv))), ((371, 888, 91), 140567, true));
        // the report: the Python's 54 keys in its order (and the Rust's own after them), and its numbers
        let report = in_order(&file("report.json")).unwrap();
        let Json::Dict(mine) = report_json(&lv.report) else { panic!("a report is a dict") };
        assert_eq!(report.iter().map(|(k, _)| k).collect::<Vec<_>>(), mine.iter().map(|(k, _)| k).collect::<Vec<_>>());
        let number = |name: &str| report.iter().find(|(k, _)| k == name).unwrap().1.clone();
        assert_eq!(
            (report.len(), number("cover"), number("walks"), number("score"), number("extent")),
            (54 + RUSTS_OWN.len(), json!(13), json!(71), json!(37.85), json!([65, 50]))
        );
        // the walks, in the level's order, are the routes a walker is given
        let routes = routes_from(&file("walks.json")).unwrap();
        assert_eq!(routes.iter().map(|(k, _)| k).collect::<Vec<_>>(), lv.walks.iter().map(|(k, _)| k).collect::<Vec<_>>());
        assert_eq!(
            (routes.len(), routes[0].0.as_str(), walk::route_string(&routes[0].1).as_str()),
            (71, "link_c01_medium4_colossal", "2539,0,-1476;2539,0,-1654,1;2539,0,-1831")
        );
        assert_eq!(routes.iter().filter(|(k, _)| walk::must_fail(k)).map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["!window_c26b_medium1", "!pool_basin"]);
        let views = in_order(&file("views.json")).unwrap();
        assert_eq!(
            (views.len(), views[0].0.as_str(), walk::view_from(&views[0].1).map(|v| walk::spawn_string(&v))),
            (39, "colossal_door", Ok("2539,0,-1831,146.8".to_string()))
        );
        assert_eq!(file("options.json"), options_json(&Options::default()).dump(None));
        assert_eq!(recipe::load(&file("recipe.json")), Ok(recipe));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn seeds_that_fail_are_counted_by_why_and_outcomes_are_one_line() {
        assert_eq!(why("seed 12: vast: no way up to the door at 551"), "vast");
        assert_eq!(why("seed 5: rooms cut off (2 links found no corridor)"), "rooms cut off (2 links found no corridor)");
        assert_eq!(why("no room"), "no room");
        let errors = ["seed 3: vast: a", "seed 9: colossal: b", "seed 11: vast: c"].map(str::to_string);
        assert_eq!(failed(&errors), "{'vast': 2, 'colossal': 1}");
        assert_eq!(failed(&[]), "{}");
        // the best first; of two as good, the lower seed
        let report = |seed, score| (seed, Ok(Report { seed, score, ..Report::default() }));
        let scored = vec![report(1, 30.5), (2, Err("seed 2: vast: x".to_string())), report(3, 38.0), report(4, 30.5), report(5, 37.85)];
        assert_eq!(best(&scored).iter().map(|r| r.seed).collect::<Vec<_>>(), [3, 5, 1, 4]);
        let pts = [Point { x: 1.5, y: 2.0, z: 0.0, flags: 0 }, Point { x: 2.5, y: 2.0, z: 0.0, flags: 1 }, Point { x: 3.5, y: 2.0, z: 276.0, flags: 0 }];
        let walked = |reached: &[bool]| Walked { reached: reached.iter().all(|r| *r), reports: reached.iter().map(|r| json!({"reached": r})).collect() };
        let o = Outcome { name: "link".into(), ok: true, walked: Ok(walked(&[true, true])) };
        assert_eq!(outcome_line(&o, &pts), "link: OK");
        let o = Outcome { name: "link".into(), ok: false, walked: Ok(walked(&[true, false])) };
        assert_eq!(outcome_line(&o, &pts), "link: FAIL stuck before point 2 (3.5, 2.0, 276): {\"reached\":false}");
        let o = Outcome { name: "!pool".into(), ok: false, walked: Ok(walked(&[true, true])) };
        assert_eq!(outcome_line(&o, &pts), "!pool: FAIL");
        let o = Outcome { name: "link".into(), ok: false, walked: Err("no level".into()) };
        assert_eq!(outcome_line(&o, &pts), "link: FAIL no level");
        assert_eq!(chosen(vec![("a".to_string(), 1), ("b".to_string(), 2)], &["b".to_string()], "walk"), Ok(vec![("b".to_string(), 2)]));
        assert_eq!(chosen(vec![("a".to_string(), 1)], &["c".to_string()], "walk"), Err("no walk named c".to_string()));
        assert!(unwritten("the bake is not written yet") && !unwritten("no lamps"));
    }
}
