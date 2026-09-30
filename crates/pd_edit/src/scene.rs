//! A level as the editor shows it: the stage as the game loads it
//! (`custom/stages/<code>/`), its collision for picking, a world whose
//! objects (the weapons of a weapon set, the ammo crates, the doors) the view
//! draws, and its waypoint graph.

use std::sync::Arc;

use glam::Vec3;
use pd_core::assets::AssetDir;
use pd_core::mp::{MatchPlayer, MatchSetup};
use pd_sim::nav::NavGraph;
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

/// The waypoint graph, for drawing.
pub struct Graph {
    pub pos: Vec<Vec3>,
    /// Each link once: `(a, b, one_way)` (a one-way link runs a → b).
    pub links: Vec<(usize, usize, bool)>,
    /// Each waypoint's strongly connected part, 0 the largest (the one
    /// placement uses: the rest can't be reached and left).
    pub part: Vec<usize>,
}

impl Graph {
    pub fn of(stage: &Stage, level: &TileLevel) -> Graph {
        let nav = NavGraph::from_stage(stage, level);
        let pos = (0..stage.waypoints.len()).map(|w| stage.waypoint_pos(w)).collect();
        let directed = stage.waypoint_links();
        let mut links = Vec::new();
        for &(a, b, one_way) in &directed {
            if one_way {
                links.push((a, b, true));
            } else if a < b {
                links.push((a, b, false));
            }
        }
        Graph { pos, links, part: pd_import::place::waypoint_parts(&nav) }
    }

    /// How many waypoints are outside the main part.
    pub fn stranded(&self) -> usize {
        self.part.iter().filter(|&&p| p != 0).count()
    }

    /// The waypoint nearest `p` within `tol` cm.
    pub fn nearest(&self, p: Vec3, tol: f32) -> Option<usize> {
        (0..self.pos.len()).map(|i| (i, self.pos[i].distance(p))).filter(|&(_, d)| d <= tol).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
    }

    // What an edit does, shown at once (the placement that follows reloads
    // the graph as it really is).

    pub fn add(&mut self, p: Vec3) -> usize {
        self.pos.push(p);
        self.part.push(0);
        self.pos.len() - 1
    }

    pub fn remove(&mut self, i: usize) {
        self.pos.remove(i);
        self.part.remove(i);
        self.links.retain(|&(a, b, _)| a != i && b != i);
        for l in &mut self.links {
            if l.0 > i {
                l.0 -= 1;
            }
            if l.1 > i {
                l.1 -= 1;
            }
        }
    }

    /// Whether `a` and `b` are linked either way.
    pub fn linked(&self, a: usize, b: usize) -> bool {
        self.links.iter().any(|&(x, y, _)| (x == a && y == b) || (x == b && y == a))
    }

    pub fn link(&mut self, a: usize, b: usize, one_way: bool) {
        self.unlink(a, b);
        self.links.push(if one_way { (a, b, true) } else { (a.min(b), a.max(b), false) });
    }

    pub fn unlink(&mut self, a: usize, b: usize) {
        self.links.retain(|&(x, y, _)| !((x == a && y == b) || (x == b && y == a)));
    }
}

pub struct Scene {
    pub code: String,
    pub stage: Arc<Stage>,
    pub level: Arc<TileLevel>,
    pub res: Arc<WorldRes>,
    /// The objects, placed as a match places them (never ticked).
    pub world: World,
    pub graph: Graph,
    /// The box around the collision.
    pub lo: Vec3,
    pub hi: Vec3,
}

/// A match setup for looking at `stage` with `weapons` (six `MP_WEAPONS`
/// indices: a weapon set's slots), one player and no simulants.
fn setup(stage: &Stage, weapons: [u8; 6]) -> MatchSetup {
    MatchSetup { stagenum: stage.stagenum, players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], weapons, ..Default::default() }
}

/// A new world run for a moment, as a match's first frames run: the
/// pickups dropped to their floors, the objects posed.
fn settled(mut w: World) -> World {
    for _ in 0..20 {
        w.step(4, &[pd_sim::player::PlayerInput::default()]);
    }
    w
}

impl Scene {
    pub fn load(assets: &AssetDir, code: &str, res: Arc<WorldRes>, weapons: [u8; 6]) -> Result<Scene, String> {
        let stage = Arc::new(Stage::load(assets, code)?);
        let level = Arc::new(TileLevel::for_stage(&stage));
        let world = settled(World::new(setup(&stage, weapons), stage.clone(), level.clone(), res.clone(), 0)?);
        let graph = Graph::of(&stage, &level);
        let (lo, hi) = level.geom.polys.iter().flat_map(|p| p.verts.iter()).fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), v| (a.min(*v), b.max(*v)));
        Ok(Scene { code: code.to_owned(), stage, level, res, world, graph, lo, hi })
    }

    /// The world again with another weapon set.
    pub fn set_weapons(&mut self, weapons: [u8; 6]) -> Result<(), String> {
        self.world = settled(World::new(setup(&self.stage, weapons), self.stage.clone(), self.level.clone(), self.res.clone(), 0)?);
        Ok(())
    }

    /// The first tile along the ray from `from` along unit `dir` (within
    /// `max` cm): where it meets, and whether it is a floor.
    pub fn pick(&self, from: Vec3, dir: Vec3, max: f32) -> Option<(Vec3, bool)> {
        let hit = self.level.first_tile_hit(from, from + dir * max)?;
        let p = &self.level.geom.polys[hit.poly];
        Some((hit.point, p.floor))
    }

    /// Where an item dropped at `p` stands: the floor under it (or at it)
    /// and PD's pad height over that.
    pub fn stand(&self, p: Vec3) -> Option<Vec3> {
        let (y, poly) = self.level.cd_find_ground_at_cyl(p + Vec3::Y * 30.0, 1.0);
        poly.map(|_| Vec3::new(p.x, y + crate::doc::PAD_HEIGHT, p.z))
    }

    /// Whether `p` is in sight of `eye` (sight-blocking tiles only).
    pub fn visible(&self, eye: Vec3, p: Vec3) -> bool {
        self.level.los(eye, p)
    }

    /// A camera looking at `target` from a few metres off and above it, from
    /// whichever of eight sides has a clear line to it (the one looking
    /// along `prefer_yaw` when that one does).
    pub fn frame(&self, target: Vec3, prefer_yaw: f32) -> crate::camera::FlyCam {
        let aim = target + Vec3::Y * 30.0;
        let mut best: Option<(f32, Vec3)> = None;
        for k in 0..8 {
            let yaw = prefer_yaw + k as f32 * std::f32::consts::FRAC_PI_4;
            let back = -Vec3::new(yaw.sin(), 0.0, yaw.cos());
            for dist in [350.0, 220.0, 120.0] {
                let pos = aim + back * dist + Vec3::Y * dist * 0.6;
                if self.level.los(aim, pos + (pos - aim).normalize() * 30.0) {
                    if best.is_none_or(|b| dist > b.0) {
                        best = Some((dist, pos));
                    }
                    break;
                }
            }
            if best.is_some_and(|b| b.0 >= 350.0) {
                break;
            }
        }
        let pos = best.map_or(aim + Vec3::Y * 150.0, |b| b.1);
        let d = aim - pos;
        crate::camera::FlyCam::new(pos, d.x.atan2(d.z), (d.y / d.length().max(1.0)).asin())
    }

    /// A good first view: over the level's middle, looking along it.
    pub fn start_camera(&self) -> crate::camera::FlyCam {
        let c = (self.lo + self.hi) * 0.5;
        let size = (self.hi - self.lo).max(Vec3::splat(100.0));
        // From a spawn if there is one, at eye height: the first whose facing
        // looks 8 m before a wall, else the first.
        let open = |p: &usize| {
            let pad = &self.stage.pads[*p];
            let eye = pad.pos + Vec3::Y * 110.0;
            let a = pad.look_angle();
            self.level.los(eye, eye + Vec3::new(a.sin(), 0.0, a.cos()) * 800.0)
        };
        if let Some(&p) = self.stage.spawn_pads.iter().find(|p| open(p)).or(self.stage.spawn_pads.first()) {
            // Stepped back 2 m and up half a metre where there's room, for
            // a little more of the room in view.
            let pad = &self.stage.pads[p];
            let (eye, a) = (pad.pos + Vec3::Y * 110.0, pad.look_angle());
            let back = eye - Vec3::new(a.sin(), -0.25, a.cos()) * 200.0;
            let pos = if self.level.los(eye, back + (back - eye).normalize() * 40.0) { back } else { eye };
            return crate::camera::FlyCam::new(pos, a, -0.2);
        }
        crate::camera::FlyCam::new(Vec3::new(c.x, self.hi.y + size.y * 0.5, self.lo.z - size.z * 0.2), 0.0, -0.5)
    }
}
