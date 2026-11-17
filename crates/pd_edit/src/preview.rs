//! The generated level in 3D while it is being shaped, and walked in place
//! (the generator's editor, milestone 5: docs/AUTOLEVEL.md).
//!
//! A [`Preview`] is the level a generator's recipe makes, as the game would
//! load it once imported, short of everything a look and a walk can do
//! without: the geometry step of the import alone
//! (`pd_import::autowalk::geometry_only`: the generator's rooms as PD's rooms,
//! its openings as portals, Complex's materials on the pool's textures, the
//! bake's colours, every drawn triangle a tile by the importer's rules), with
//! no waypoints, no placement, no check match and no layout. It is **not an
//! arena**: it is written into a scratch folder of its own under the system's
//! temporary directory ([`Scratch`], deleted when the preview is dropped),
//! laid over `assets/` as `custom/` is ([`Preview::asset_dir`]), so the
//! renderer loads its BG as any stage's; `custom/` and the levels folder are
//! never touched. Making one takes about 0.6 s for seed 40 (the level made
//! again from its recipe and baked 0.45 s, its geometry written and loaded
//! 0.12 s), on a thread ([`Job`]) in the window.
//!
//! A [`Walker`] is PD's own player (`pd_sim::player::Player`, alone, stepped
//! at 60 Hz as `pd_import::autowalk`'s is) on the preview's tiles: walls,
//! rails, ramps, drops, grates and vents as in the game. It is dropped where
//! the fly camera is, onto the floor beneath it ([`Walker::drop_at`]), and
//! when the level is made again under it, it is kept where it stands if the
//! floor is still there, else put on the floor nearest ([`Walker::reseat`]).
//!
//! Coordinates: the generator's plan is in cells of 118.11 units, x east and
//! y north; PD's world is (x, height, -y) ([`world`], [`cell`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use autolevel::recipe as gen;
use glam::Vec3;
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::ids::{CROUCHPOS_DUCK, CROUCHPOS_SQUAT, CROUCHPOS_STAND};
use pd_core::lv::{Lv, LvTickIn};
use pd_core::rng::Rng;
use pd_import::autowalk::{self, Via};
use pd_import::recipe::Source;
use pd_render::View;
use pd_sim::player::{Player, PlayerInput, WalkEnv};
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::WorldRes;

use crate::camera::FlyCam;

/// The stage code a preview is written and loaded under (in its own scratch
/// tree: no level of the levels folder has it).
pub const CODE: &str = "autolevelpreview";

/// A plan cell, in PD's units (`autolevel::plan::MODULE`: 3 m).
pub const CELL: f32 = autolevel::plan::MODULE as f32;

/// The player's standing eye height (`vv_eyeheight`).
pub const EYE: f32 = 159.0;

/// A plan place (cells, x east and y north) at height `z`, in the world.
pub fn world(cell: (f32, f32), z: f32) -> Vec3 {
    Vec3::new(cell.0 * CELL, z, -cell.1 * CELL)
}

/// A world place on the plan, in cells.
pub fn cell(p: Vec3) -> (f32, f32) {
    (p.x / CELL, -p.z / CELL)
}

/// The way a camera turned `yaw` (radians, the fly camera's: 0 faces +z,
/// a quarter turn +x) looks on the plan: a unit step in cells.
pub fn heading(yaw: f32) -> (f32, f32) {
    (yaw.sin(), -yaw.cos())
}

/// The camera's mark on a plan drawn north up: an arrowhead `size` pixels
/// long about the pixel `at`, pointing the way a camera turned `yaw` looks.
pub fn marker(at: (f32, f32), yaw: f32, size: f32) -> [(f32, f32); 3] {
    // (south, the world's +z, is down the picture)
    let point = |turn: f32, r: f32| (at.0 + (yaw + turn).sin() * r, at.1 + (yaw + turn).cos() * r);
    [point(0.0, size), point(2.5, size * 0.75), point(-2.5, size * 0.75)]
}

/// A scratch custom tree, deleted with everything in it when dropped.
#[derive(Debug)]
pub struct Scratch(PathBuf);

/// A scratch folder's number within this process.
static SCRATCH: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    /// The folder previews are kept in unless another is named: the system's
    /// temporary directory's `pd_edit_preview`.
    pub fn root() -> PathBuf {
        std::env::temp_dir().join("pd_edit_preview")
    }

    /// A new, empty folder of this process's own under `root`.
    pub fn new(root: &Path) -> Result<Scratch, String> {
        let dir = root.join(format!("{}-{}", std::process::id(), SCRATCH.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Scratch(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Delete what earlier runs left under `root` (an editor that was killed
    /// deletes nothing): every folder there untouched for a day.
    pub fn sweep(root: &Path) {
        let old = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|d| d > Duration::from_secs(24 * 3600)));
        for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
            if e.path().is_dir() && old(&e.path()) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A room of the level, as the plan has it.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomBox {
    pub key: String,
    /// Its rectangle in cells: x0, y0, x1, y1.
    pub rect: (i32, i32, i32, i32),
    pub floor: i32,
    pub top: i32,
    /// For a pod: the hall it is in. Empty otherwise.
    pub host: String,
}

/// A generated level to look at and walk: its stage and tiles as the game
/// would load them, in a scratch tree of its own.
pub struct Preview {
    pub stage: Arc<Stage>,
    pub level: Arc<TileLevel>,
    /// The box around its tiles.
    pub lo: Vec3,
    pub hi: Vec3,
    /// The generator's own places to look from, by name (`<room>_door`,
    /// `<room>_deck<z>`, `corridor_<key>`, ...: what `autolevel shots` draws):
    /// plan cell x, y, the floor stood on, the cell looked at.
    pub views: Vec<(String, autolevel::level::View)>,
    pub rooms: Vec<RoomBox>,
    /// How long it took to make, seconds.
    pub secs: f32,
    assets: PathBuf,
    scratch: Scratch,
}

impl Preview {
    /// The level `recipe` makes, written into a new scratch folder under
    /// `root` and loaded. `assets` is the game's `assets/`. Err in the
    /// generator's words where the recipe makes no level (nothing is left
    /// behind).
    pub fn make(recipe: &gen::Recipe, assets: &Path, root: &Path) -> Result<Preview, String> {
        let started = Instant::now();
        let scratch = Scratch::new(root)?;
        let r = autowalk::recipe(CODE, "Preview", autowalk::STAGENUM, Via::Recipe(&gen::save(recipe)))?;
        // (a recipe the generator stops dead on is an error here, as in its own editor)
        let (stage, level) = autowalk::geometry_only(&r, assets, scratch.path())?;
        // The level itself, for its views and rooms: the importer has just
        // made it for the geometry and hands the same one back.
        let Source::Autolevel(src) = &r.source else { return Err("the preview's recipe is not a generated level's".to_string()) };
        let built = pd_import::autolevel::make(src, None)?;
        let rooms = built.level.spaces().iter().map(|s| RoomBox { key: s.key.clone(), rect: s.rect(), floor: s.floor, top: s.top, host: s.host.clone() }).collect();
        let (lo, hi) = level.geom.polys.iter().flat_map(|p| p.verts.iter()).fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), v| (a.min(*v), b.max(*v)));
        Ok(Preview { stage: Arc::new(stage), level: Arc::new(level), lo, hi, views: built.level.views.clone(), rooms, secs: started.elapsed().as_secs_f32(), assets: assets.to_path_buf(), scratch })
    }

    /// `assets/` with this preview's scratch tree over it: what the renderer
    /// loads the BG of [`CODE`] from.
    pub fn asset_dir(&self) -> AssetDir {
        AssetDir::new(&self.assets).with_custom_dir(self.scratch.path())
    }

    /// The scratch folder it is kept in (deleted when the preview is dropped).
    pub fn dir(&self) -> &Path {
        self.scratch.path()
    }

    /// One line of what it is: its rooms, tiles and how long it took.
    pub fn line(&self) -> String {
        // (PD's room 0 is no room)
        format!("{} rooms, {} portals, {} tiles, made in {:.2} s", self.stage.rooms.roomcount().saturating_sub(1), self.stage.rooms.portals.len(), self.level.geom.polys.len(), self.secs)
    }

    /// The floor under `p` (the highest below it that a player's cylinder
    /// there touches), if there is one.
    pub fn floor_under(&self, p: Vec3) -> Option<f32> {
        let (y, poly) = self.level.cd_find_ground_at_cyl(p, 30.0);
        poly.map(|_| y)
    }

    /// The place on a floor nearest `p` within `reach`, a player's radius in
    /// from the tile's edge where the tile is wide enough.
    pub fn nearest_floor(&self, p: Vec3, reach: f32) -> Option<Vec3> {
        let mut best: Option<(f32, Vec3)> = None;
        for poly in self.level.geom.polys.iter().filter(|q| q.floor && q.verts.len() >= 3) {
            let v = &poly.verts;
            let mid = v.iter().copied().sum::<Vec3>() / v.len() as f32;
            // the nearest point of the outline's fan of triangles
            let mut near = (f32::INFINITY, mid);
            for i in 1..v.len() - 1 {
                let q = closest_on_triangle(p, v[0], v[i], v[i + 1]);
                let d = q.distance(p);
                if d < near.0 {
                    near = (d, q);
                }
            }
            if near.0 > reach || best.is_some_and(|b| near.0 >= b.0) {
                continue;
            }
            // pulled in from the edge towards the tile's middle
            let pull = (mid - near.1).length();
            let q = if pull > 1.0 { near.1 + (mid - near.1) * (35.0 / pull).min(1.0) } else { near.1 };
            best = Some((near.0, Vec3::new(q.x, poly.find_y(q.x, q.z), q.z)));
        }
        best.map(|b| b.1)
    }

    /// A camera at one of the generator's views, by name: standing there at
    /// eye height, facing where the view looks.
    pub fn view_camera(&self, name: &str) -> Option<FlyCam> {
        let (_, (x, y, z, tx, ty)) = self.views.iter().find(|(n, _)| n == name)?;
        let eye = world((*x as f32, *y as f32), *z as f32 + EYE);
        let to = world((*tx as f32, *ty as f32), *z as f32 + EYE) - eye;
        Some(FlyCam::new(eye, to.x.atan2(to.z), 0.0))
    }

    /// The camera the 3D view opens on: the generator's first view (the
    /// biggest hall from a door), else over the level's middle.
    pub fn start_camera(&self) -> FlyCam {
        if let Some(cam) = self.views.first().and_then(|(name, _)| self.view_camera(name)) {
            return cam;
        }
        let c = (self.lo + self.hi) * 0.5;
        FlyCam::new(Vec3::new(c.x, self.hi.y + 400.0, c.z), 0.0, -1.2)
    }

    /// A camera looking over the room `key` from high in one of its corners,
    /// or from the middle of a wall (or as far in again as a catwalk round
    /// the walls is wide): the first of them, from 5 m over the floor down,
    /// with a clear line to the middle of the floor, nothing over it (not
    /// under a catwalk) and nothing under it but the room's floor.
    pub fn room_camera(&self, key: &str) -> Option<FlyCam> {
        let room = self.rooms.iter().find(|r| r.key == key)?;
        let (x0, y0, x1, y1) = (room.rect.0 as f32, room.rect.1 as f32, room.rect.2 as f32, room.rect.3 as f32);
        let (floor, top) = (room.floor as f32, room.top as f32);
        let (mx, my) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        let aim = world((mx, my), floor + 60.0);
        // by the walls, then (a catwalk may run round them) a cell and a half further in
        let ring = |inset: f32| {
            let inset = inset.min((x1 - x0) * 0.25).min((y1 - y0) * 0.25);
            let (w, e, s, n) = (x0 + inset, x1 - inset, y0 + inset, y1 - inset);
            [(w, s), (e, s), (e, n), (w, n), (mx, s), (e, my), (mx, n), (w, my)]
        };
        let places: Vec<(f32, f32)> = ring(0.45).into_iter().chain(ring(1.95)).collect();
        let clear = |eye: Vec3| {
            let open_above = self.level.first_tile_hit(eye, Vec3::new(eye.x, top - 5.0, eye.z)).is_none();
            let on_floor = self.level.first_tile_hit(eye, Vec3::new(eye.x, floor + 5.0, eye.z)).is_none();
            open_above && on_floor && self.level.los(eye, aim)
        };
        let heights = [500.0f32, 380.0, 260.0].map(|h| (floor + h).min(top - 40.0).max(floor + 100.0));
        let eyes = heights.iter().flat_map(|&h| places.iter().map(move |&c| world(c, h)));
        let eye = eyes.clone().find(|&e| clear(e)).or_else(|| eyes.clone().find(|&e| self.level.los(e, aim))).unwrap_or(world(places[0], heights[0]));
        let d = aim - eye;
        Some(FlyCam::new(eye, d.x.atan2(d.z), (d.y / d.length().max(1.0)).asin()))
    }

    /// The room whose box holds `p` (a pod before the hall it is in), if any.
    pub fn room_at(&self, p: Vec3) -> Option<&RoomBox> {
        let (x, y) = cell(p);
        let holds = |r: &&RoomBox| (r.rect.0 as f32) <= x && x < r.rect.2 as f32 && (r.rect.1 as f32) <= y && y < r.rect.3 as f32 && (r.floor as f32 - 45.0) <= p.y && p.y <= r.top as f32 + 45.0;
        self.rooms.iter().filter(holds).min_by_key(|r| r.host.is_empty())
    }

    /// A camera standing at the plan place `at` (cells), turned `yaw`: at eye
    /// height over the floor there nearest under `near_y` (the height the
    /// camera is at now), else the highest floor there. None where the plan
    /// has no floor.
    pub fn camera_at_cell(&self, at: (f32, f32), near_y: f32, yaw: f32) -> Option<FlyCam> {
        let below = |y: f32| self.floor_under(world(at, y));
        let floor = below(near_y + 1.0).or_else(|| below(self.hi.y + 100.0))?;
        Some(FlyCam::new(world(at, floor + EYE), yaw, 0.0))
    }
}

/// The point of triangle `a b c` nearest `p` (Ericson, Real-Time Collision
/// Detection 5.1.5).
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// A preview being made on a thread.
pub struct Job {
    pub started: Instant,
    rx: Receiver<Result<Preview, String>>,
}

impl Job {
    /// Start making `recipe`'s preview under `root`.
    pub fn spawn(recipe: gen::Recipe, assets: PathBuf, root: PathBuf) -> Job {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            // (the generator has places where it stops dead: an error here, as in its own editor)
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Preview::make(&recipe, &assets, &root)));
            let _ = tx.send(made.unwrap_or_else(|_| Err("the generator stopped on this recipe (see the terminal)".to_string())));
        });
        Job { started: Instant::now(), rx }
    }

    /// The result, once: None while it is still being made.
    pub fn poll(&self) -> Option<Result<Preview, String>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the preview's thread ended with nothing to show".to_string())),
        }
    }
}

/// What moves a walker: the keys held and the mouse's travel, gathered over
/// the window's ticks until the player's next 60 Hz frame takes them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    /// -1..1: back and forward, left and right (W/S or the arrows, A/D).
    pub forward: f32,
    pub right: f32,
    /// -1..1: turn left and right with the keys (the arrows).
    pub turn: f32,
    /// Mouse travel in pixels while looking.
    pub look: (f32, f32),
    /// Crouch key presses: down a level (stand, duck, squat), up a level.
    pub crouch_down: bool,
    pub crouch_up: bool,
}

/// Pixels of mouse travel a frame that a held turn key is worth (at the
/// game's mouse rate, 0.055° a pixel: 2° a frame, 120° a second).
const TURN_KEY: f32 = 36.0;

impl Controls {
    /// These controls with `more` gathered after them: the keys as they are
    /// now, the mouse's travel and the presses added up.
    pub fn and(self, more: Controls) -> Controls {
        Controls { look: (self.look.0 + more.look.0, self.look.1 + more.look.1), crouch_down: self.crouch_down || more.crouch_down, crouch_up: self.crouch_up || more.crouch_up, ..more }
    }

    /// As the player's input for one frame (the game's keyboard and mouse
    /// mapping, `pd_game::controls`).
    pub fn input(&self) -> PlayerInput {
        PlayerInput {
            walk_y: (self.forward.clamp(-1.0, 1.0) * 127.0) as i32,
            walk_x: (self.right.clamp(-1.0, 1.0) * 127.0) as i32,
            mouse_dx: self.look.0 + self.turn.clamp(-1.0, 1.0) * TURN_KEY,
            mouse_dy: self.look.1,
            crouch_down: self.crouch_down,
            crouch_up: self.crouch_up,
            ..PlayerInput::default()
        }
    }
}

/// What became of a walker when the level was made again under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reseat {
    /// The floor it stands on is as it was: nothing changed.
    Kept,
    /// The floor under it moved up or down: it stands on it.
    Lifted,
    /// No floor under it any more: put on the nearest, this far off.
    Moved(f32),
}

/// How far a walker is moved to a floor when the one under it has gone,
/// before the walk is given up.
pub const RESEAT_REACH: f32 = 1500.0;

/// PD's player alone on a preview's tiles.
pub struct Walker {
    pub p: Player,
    lv: Lv,
    rng: Rng,
    events: Vec<Event>,
    res: Arc<WorldRes>,
    /// Time not yet stepped, and the controls gathered for the next frame.
    acc: f32,
    held: Controls,
}

impl Walker {
    /// A player standing at `feet` (on the floor there), facing `theta`
    /// degrees (PD's: forward = (-sin, 0, cos)).
    pub fn at(res: Arc<WorldRes>, feet: Vec3, theta: f32) -> Result<Walker, String> {
        let mut rng = Rng::new(0x5eed);
        let p = Player::new(&res, feet, theta, 1, &mut rng)?;
        Ok(Walker { p, lv: Lv::new(), rng, events: Vec::new(), res, acc: 0.0, held: Controls::default() })
    }

    /// A player dropped from `eye` onto the floor beneath it, facing as a
    /// camera turned `yaw` and tilted `pitch` (radians, the fly camera's).
    /// Err where there is no floor under the place.
    pub fn drop_at(pv: &Preview, res: Arc<WorldRes>, eye: Vec3, yaw: f32, pitch: f32) -> Result<Walker, String> {
        let floor = pv.floor_under(eye).ok_or("no floor under the camera: fly over a floor first")?;
        let mut w = Walker::at(res, Vec3::new(eye.x, floor, eye.z), (-yaw).to_degrees().rem_euclid(360.0))?;
        w.p.verta = pitch.to_degrees().clamp(-80.0, 80.0);
        Ok(w)
    }

    /// One of the player's frames (a sixtieth of a second).
    pub fn step(&mut self, pv: &Preview, input: &PlayerInput) {
        self.lv.frame(4, LvTickIn::default());
        let env = WalkEnv { level: &pv.level, cyls: &[], floors: &[], fastmovement: false, shieldfrac: 0.0, menuopen: false, briefcase: false };
        self.p.tick(input, &self.lv, &env, &self.res, &mut self.rng, &mut self.events);
        self.events.clear();
    }

    /// `dt` seconds of the window's time with `controls` held: the player's
    /// frames that fall due are stepped (at most eight, so a stall is not
    /// caught up), the mouse's travel and the presses going to the first.
    pub fn advance(&mut self, pv: &Preview, controls: Controls, dt: f32) {
        self.held = self.held.and(controls);
        self.acc = (self.acc + dt).min(8.0 / 60.0);
        while self.acc >= 1.0 / 60.0 {
            self.acc -= 1.0 / 60.0;
            let input = self.held.input();
            self.step(pv, &input);
            self.held = Controls { look: (0.0, 0.0), crouch_down: false, crouch_up: false, ..self.held };
        }
    }

    /// Where the feet are.
    pub fn feet(&self) -> Vec3 {
        Vec3::new(self.p.pos.x, self.p.manground, self.p.pos.z)
    }

    /// The fly camera at the player's eyes, looking as the player does.
    pub fn camera(&self) -> FlyCam {
        FlyCam::new(self.p.pos, (-self.p.theta).to_radians(), self.p.verta.to_radians().clamp(-1.55, 1.55))
    }

    /// The player's view over `aspect`, clipping at `znear`..`zfar`.
    pub fn view(&self, aspect: f32, znear: f32, zfar: f32) -> View {
        View { aspect, ..View::for_player(&self.p, znear, zfar) }
    }

    /// "standing", "ducked" or "squatting".
    pub fn stance(&self) -> &'static str {
        match self.p.crouchpos {
            CROUCHPOS_SQUAT => "squatting",
            CROUCHPOS_DUCK => "ducked",
            CROUCHPOS_STAND => "standing",
            _ => "",
        }
    }

    /// The feet and facing as the game's F1 panel words them (what
    /// `--walk` and `pd_snapshot stage --at` take).
    pub fn line(&self) -> String {
        let f = self.feet();
        format!("feet ({:.0}, {:.0}, {:.0}) θ {:.0}, {}{}", f.x, f.y, f.z, self.p.theta.rem_euclid(360.0), self.stance(), if self.p.isfalling { ", falling" } else { "" })
    }

    /// The level was made again as `pv`: stay where the floor is as it was,
    /// stand on it where it has moved, else go to the floor nearest within
    /// [`RESEAT_REACH`]. Err (the walker is then no use) where there is none.
    pub fn reseat(&mut self, pv: &Preview) -> Result<Reseat, String> {
        let feet = self.feet();
        let (theta, verta) = (self.p.theta, self.p.verta);
        // in the air (a drop, a ramp's hop): the floor is where it lands
        let under = pv.floor_under(Vec3::new(feet.x, feet.y + 60.0, feet.z));
        match under {
            Some(y) if (y - feet.y).abs() < 1.0 || (self.p.isfalling && y < feet.y) => Ok(Reseat::Kept),
            Some(y) if (y - feet.y).abs() <= 120.0 => {
                self.p.place(Vec3::new(feet.x, y, feet.z), theta);
                self.p.verta = verta;
                Ok(Reseat::Lifted)
            }
            _ => {
                let to = pv.nearest_floor(feet, RESEAT_REACH).ok_or("the floor the walker stood on is gone, and there is none near")?;
                self.p.place(to, theta);
                self.p.verta = verta;
                Ok(Reseat::Moved(to.distance(feet)))
            }
        }
    }
}

#[cfg(test)]
mod tests;
