//! A generated level (`autolevel`, `autolevel_build`) on the game's own
//! collision and player: the recipe a build folder is imported with, and
//! [`SimWalk`], which walks its routes in this process.
//!
//! `autolevel build SEED` writes a folder with the level's recipe
//! (`recipe.json`: what makes it), its files (a `.glb`, a GE64 OBJ) and its
//! routes (`walks.json`: every corridor, every room door to door, every
//! spiral to its top). Walking them is how a level is known to be sound, and
//! `autolevel_build::walk::Walker` is what walks. [`SimWalk`] is the walker
//! that asks the game itself: the folder's recipe goes through the importer's
//! source for generated levels ([`crate::autolevel`]) with the recipe
//! [`recipe_text`] gives (the one `autolevel import` writes), the geometry
//! step's stage files are written into a scratch folder and loaded with
//! [`Stage::load`], so the tiles, rooms and portals walked on are the ones
//! the game loads for the imported level; then each route is walked by a
//! [`Player`] of its own, stepped alone as `pd_sim`'s walk tests step one.
//! The waypoints and the placement are left out: a walk needs neither, and
//! they are most of an import's time.
//!
//! The walk itself ([`SimWalk::walk`]) is pd-walk's `walk_route` (the Blender
//! MCP repo's `viewer/src/main.rs`), rule for rule, so the two walkers can be
//! held against each other: steer straight at each point, a stop is let go of
//! and settled on, a point passed through is measured on the move.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use autolevel_build::walk::{self, Point, View, Walked, Walker};
use glam::{Vec2, Vec3};
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::ids::{CROUCHPOS_SQUAT, CROUCHPOS_STAND};
use pd_core::lv::{Lv, LvTickIn};
use pd_core::rng::Rng;
use pd_sim::player::{Player, PlayerInput, WalkEnv};
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::WorldRes;
use serde_json::{json, Value};

use crate::recipe::Recipe;
use crate::{write, Paths};

/// The arena menu's group for generated levels.
pub const GROUP: &str = "Autolevel";

/// The level a build folder holds: its `autolevel_<seed>.glb` and the seed.
/// Err, saying how to make one, if there is none (or more than one).
pub fn built_glb(dir: &Path) -> Result<(PathBuf, String), String> {
    let none = || format!("no built level in {} (no autolevel_<seed>.glb): autolevel build SEED --out {0}", dir.display());
    let mut found: Vec<(PathBuf, String)> = std::fs::read_dir(dir)
        .map_err(|_| none())?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let seed = name.strip_prefix("autolevel_")?.strip_suffix(".glb")?.to_owned();
            Some((e.path(), seed))
        })
        .collect();
    found.sort();
    match found.len() {
        0 => Err(none()),
        1 => Ok(found.remove(0)),
        n => Err(format!("{}: {n} levels (autolevel_*.glb), one is expected", dir.display())),
    }
}

/// The generator's recipe a build folder holds (`recipe.json`: its text),
/// and its seed. Err, saying how to make one, if there is none.
pub fn built_recipe(dir: &Path) -> Result<(String, u64), String> {
    let path = dir.join("recipe.json");
    let text = std::fs::read_to_string(&path).map_err(|_| format!("no built level in {} (no recipe.json): autolevel build SEED --out {0}", dir.display()))?;
    let seed = autolevel::recipe::load(&text).map_err(|e| format!("{}: {e}", path.display()))?.seed;
    Ok((text, seed))
}

/// How a generated level reaches the importer.
#[derive(Clone, Copy, Debug)]
pub enum Via<'a> {
    /// By the source of its own ([`crate::autolevel`]): the generator's
    /// recipe, as its file's text, kept in the level's recipe itself.
    Recipe(&'a str),
    /// As any glTF ([`crate::gltf`]), with the tile rules these levels are
    /// built on turned on (walkable grates, vents under low ceilings, open
    /// ramps): the `.glb`'s path. Its rooms are then a split of its
    /// footprint, and its textures the copies the file carries.
    Gltf(&'a str),
}

/// The recipe a generated level is imported with, as its file's text: the
/// level's own source (or the glTF one), and an arena's usual counts.
pub fn recipe_text(code: &str, name: &str, via: Via) -> String {
    let text = |s: &str| Value::String(s.to_owned()).to_string();
    let (source, rooms) = match via {
        // The generator's recipe as it is written, set in two levels.
        Via::Recipe(recipe) => (format!("\"game\": \"autolevel\",\n    \"recipe\": {}", recipe.trim().replace('\n', "\n    ")), ""),
        Via::Gltf(glb) => (format!("\"game\": \"gltf\",\n    \"path\": {},\n    \"grates\": true,\n    \"low_ceilings\": true,\n    \"ramp_walls\": true", text(glb)), "\n  \"rooms\": 60,"),
    };
    format!(
        "{{\n  \"format\": \"pd-import-recipe/1\",\n  \"code\": {},\n  \"name\": {},\n  \"group\": {},\n  \"scale\": 1.0,\n  \"source\": {{\n    {source}\n  }},{rooms}\n  \"spawns\": 16,\n  \"weapons\": 10,\n  \"hills\": 5\n}}\n",
        text(code),
        text(name),
        text(GROUP),
    )
}

/// [`recipe_text`], read: the same settings, by the same reader as a recipe
/// file's.
pub fn recipe(code: &str, name: &str, via: Via) -> Result<Recipe, String> {
    serde_json::from_str(&recipe_text(code, name, via)).map_err(|e| format!("the recipe of {code}: {e}"))
}

/// The geometry step of an import alone, into a custom tree of the caller's
/// own (`custom`, made if need be: `custom/stages/<code>/` gets the stage's
/// BG and tiles, and pads and a setup with only the source's own rows), then
/// loaded as the game loads a stage with that tree laid over `assets`. No
/// waypoints, no placement, no check match, and the level is not listed
/// (`custom/levels.json` is not written): what a walk and a preview need of
/// a level, in a fraction of an import's time. The folder is the caller's to
/// keep (a renderer loads the BG from it) or delete.
pub fn geometry_only(r: &Recipe, assets: &Path, custom: &Path) -> Result<(Stage, TileLevel), String> {
    geometry_only_drawn(r, assets, custom, &|_| {})
}

/// [`geometry_only`] with the drawn triangles put through `drawn` before the
/// BG is written ([`crate::geometry_drawn`]): the tiles are as ever.
pub fn geometry_only_drawn(r: &Recipe, assets: &Path, custom: &Path, drawn: &dyn Fn(&mut Vec<crate::source::Tri>)) -> Result<(Stage, TileLevel), String> {
    let paths = Paths { assets: assets.to_path_buf(), custom: custom.to_path_buf(), src: None, levels: None };
    let (stage_dir, data) = crate::geometry_drawn(r, &paths, drawn)?;
    write::write_pads(&stage_dir, r, &data.fixed)?;
    write::write_setup(&stage_dir, r, &data.fixed)?;
    let stage = Stage::load(&AssetDir::new(assets).with_custom_dir(custom), &r.code)?;
    let level = TileLevel::for_stage(&stage);
    Ok((stage, level))
}

/// A scratch folder's number, so two walkers opened on one folder at once
/// (tests, a batch) do not share one.
static SCRATCH: AtomicUsize = AtomicUsize::new(0);

/// One built level as the game loads it, walked on `pd_sim`'s player.
pub struct SimWalk {
    /// The stage, with no gameplay data: its rooms and portals are the
    /// import's.
    pub stage: Stage,
    /// Its tiles.
    pub level: TileLevel,
    res: Arc<WorldRes>,
    /// `PD_WALK_TRACE` is set: the walker's feet every 5 ticks, on stderr
    /// (as pd-walk; walk one route, on one thread).
    trace: bool,
}

impl SimWalk {
    /// What every walker shares, loaded once: the animation bank (the head's
    /// bob is part of the walk) and the rest of what a world loads.
    pub fn res(assets: &Path) -> Result<Arc<WorldRes>, String> {
        Ok(Arc::new(WorldRes::load(&AssetDir::new(assets))?))
    }

    /// The level built in `dir`, loaded once: made again from the folder's
    /// recipe, as an import of it is. `assets` is the game's `assets/`.
    pub fn open(dir: &Path, assets: &Path, res: Arc<WorldRes>) -> Result<SimWalk, String> {
        let (text, seed) = built_recipe(dir)?;
        Self::open_via(dir, seed, Via::Recipe(&text), assets, res)
    }

    /// The level built in `dir` as its `.glb` imports by the glTF source
    /// (`autolevel import --gltf`): the same walk on that import's tiles.
    pub fn open_gltf(dir: &Path, assets: &Path, res: Arc<WorldRes>) -> Result<SimWalk, String> {
        let (glb, seed) = built_glb(dir)?;
        let seed = seed.parse().map_err(|_| format!("{}: its seed is not a number", glb.display()))?;
        Self::open_via(dir, seed, Via::Gltf(&glb.to_string_lossy()), assets, res)
    }

    fn open_via(dir: &Path, seed: u64, via: Via, assets: &Path, res: Arc<WorldRes>) -> Result<SimWalk, String> {
        let r = recipe(&format!("autolevel{seed}"), &format!("Autolevel {seed}"), via)?;
        // The geometry step of an import, into a custom tree of its own under
        // the build folder, loaded as the game loads a stage and thrown away.
        let scratch = dir.join(format!(".simwalk-{}-{}", std::process::id(), SCRATCH.fetch_add(1, Ordering::Relaxed)));
        let loaded = geometry_only(&r, assets, &scratch);
        let _ = std::fs::remove_dir_all(&scratch);
        let (stage, level) = loaded.map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(SimWalk { stage, level, res, trace: std::env::var_os("PD_WALK_TRACE").is_some() })
    }

    /// A player of its own standing at `feet`, facing `theta` degrees.
    fn lone(&self, feet: Vec3, theta: f32) -> Result<Lone<'_>, String> {
        let mut rng = Rng::new(0x5eed);
        let p = Player::new(&self.res, feet, theta, 1, &mut rng)?;
        Ok(Lone { p, lv: Lv::new(), rng, events: Vec::new(), level: &self.level, res: &self.res })
    }

    /// Feet on the floor under `p` (or `p` itself over the void).
    fn settle(&self, p: Vec3) -> Vec3 {
        let (y, _) = self.level.cd_find_ground_at_cyl(p + Vec3::new(0.0, 100.0, 0.0), 30.0);
        if y > -100_000.0 {
            Vec3::new(p.x, y, p.z)
        } else {
            p
        }
    }
}

/// A route's point in the game's coordinates, in whole units as pd-walk is
/// given it (`walk::route_string`).
fn world(p: &Point) -> Vec3 {
    let (x, y, z) = walk::ge(p.x, p.y, p.z);
    let whole = |v: f64| format!("{v:.0}").parse::<f32>().unwrap_or(v as f32);
    Vec3::new(whole(x), whole(y), whole(z))
}

/// `vv_theta` (degrees; forward = (-sin, 0, cos)) facing along (dx, dz).
fn theta(dx: f32, dz: f32) -> f32 {
    (-dx).atan2(dz).to_degrees().rem_euclid(360.0)
}

/// The player alone on the level, stepped at 60 Hz.
struct Lone<'a> {
    p: Player,
    lv: Lv,
    rng: Rng,
    events: Vec<Event>,
    level: &'a TileLevel,
    res: &'a WorldRes,
}

impl Lone<'_> {
    fn tick(&mut self, input: &PlayerInput) {
        self.lv.frame(4, LvTickIn::default());
        let env = WalkEnv { level: self.level, cyls: &[], floors: &[], fastmovement: false, shieldfrac: 0.0, menuopen: false, briefcase: false };
        self.p.tick(input, &self.lv, &env, self.res, &mut self.rng, &mut self.events);
        self.events.clear();
    }
}

impl Walker for SimWalk {
    /// pd-walk's `walk_route`: steer at each point until within 30, let go,
    /// settle, and report. A point's flags: 1 = pass through without
    /// stopping, 2 = squat on the way to it (a vent); the walker stands again
    /// for the first point without it.
    fn walk(&self, route: &[Point]) -> Result<Walked, String> {
        if route.len() < 2 {
            return Err("a route is at least two points".to_string());
        }
        let pts: Vec<Vec3> = route.iter().map(world).collect();
        let first = self.settle(pts[0]);
        let d = pts[1] - first;
        let mut w = self.lone(first, theta(d.x, d.z))?;
        let idle = PlayerInput::default();
        let fwd = PlayerInput { walk_y: 127, ..PlayerInput::default() };
        let (down, up) = (PlayerInput { crouch_down: true, ..PlayerInput::default() }, PlayerInput { crouch_up: true, ..PlayerInput::default() });
        let mut ok = true;
        let mut reports = Vec::new();
        for (i, b) in pts.iter().enumerate().skip(1) {
            let squat = route[i].flags & 2 != 0;
            if squat && w.p.crouchpos != CROUCHPOS_SQUAT {
                for _ in 0..4 {
                    if w.p.crouchpos == CROUCHPOS_SQUAT {
                        break;
                    }
                    w.tick(&down);
                    w.tick(&idle);
                }
                for _ in 0..40 {
                    w.tick(&idle); // sink down
                }
            } else if !squat && w.p.crouchpos != CROUCHPOS_STAND {
                for _ in 0..4 {
                    if w.p.crouchpos == CROUCHPOS_STAND {
                        break;
                    }
                    w.tick(&up);
                    for _ in 0..20 {
                        w.tick(&idle);
                    }
                }
            }
            let far = Vec2::new(b.x - w.p.pos.x, b.z - w.p.pos.z).length();
            let max_ticks = (far * 1.5 + 600.0) as usize;
            // closest approach while steering (PD keeps moving for a while after the stick is
            // released, so the distance after stopping would count the overshoot)
            let (mut best, mut feet, mut under) = (f32::MAX, w.p.manground, w.p.ground);
            let mut t = 0;
            while t < max_ticks {
                let (dx, dz) = (b.x - w.p.pos.x, b.z - w.p.pos.z);
                let d = (dx * dx + dz * dz).sqrt();
                if d < best {
                    best = d;
                    feet = w.p.manground;
                    under = w.p.ground;
                }
                if d < 30.0 {
                    break;
                }
                w.p.theta = theta(dx, dz);
                for _ in 0..5 {
                    w.tick(&fwd);
                }
                t += 5;
                if self.trace {
                    eprintln!("  leg {i} t {t}: feet {:.0} {:.0} {:.0} ground {:.0}{}", w.p.pos.x, w.p.manground, w.p.pos.z, w.p.ground, if w.p.isfalling { " falling" } else { "" });
                }
            }
            let pass = route[i].flags & 1 != 0 && i + 1 < route.len();
            if !pass {
                // a drop: land first (a long fall outlasts the settling time)
                let mut airborne = 0;
                while w.p.isfalling && airborne < 600 {
                    w.tick(&idle);
                    airborne += 1;
                }
                for _ in 0..60 {
                    w.tick(&idle);
                }
                // the height once standing (PD's feet lag on slopes); keep the lower of the two misses
                if (w.p.manground - b.y).abs() < (feet - b.y).abs() {
                    feet = w.p.manground;
                }
            }
            // a pass-through point is measured on the move (feet lag on and just off slopes), so it only
            // has to show the walker is on the right level: its feet near it, or the floor under them the
            // point's own (running down flight after flight, the walker is still in the air past each
            // ramp's foot). A stop is measured standing
            let level = (feet - b.y).abs() < 40.0 || (under - b.y).abs() < 8.0;
            let reached = best < 40.0 && if pass { level } else { (feet - b.y).abs() < 8.0 };
            ok &= reached;
            // the game's room of the floor under the walker (the import's, not the level's own)
            let room = self.level.cd_find_ground_at_cyl_ctfril(w.p.pos, 30.0, &[]).room.map_or(String::new(), |r| format!("room {r}"));
            let tenth = |v: f32| (v as f64 * 10.0).round() / 10.0;
            reports.push(json!({"i": i, "reached": reached, "dist": tenth(best), "feet": tenth(feet), "want": tenth(b.y), "room": room}));
            if !reached {
                break;
            }
        }
        Ok(Walked { reached: ok, reports })
    }

    /// No renderer here: the game's own draws an imported level.
    fn shot(&self, view: &View, _: &Path) -> Result<(), String> {
        Err(format!(
            "this walker draws nothing: import the level (autolevel import DIR) and render it with the game (cargo run --release -p pd_tools --bin pd_snapshot -- OUT stage CODE --at {}), or name pd-walk (--pd-walk PATH)",
            walk::spawn_string(view)
        ))
    }
}

#[cfg(test)]
mod tests;
