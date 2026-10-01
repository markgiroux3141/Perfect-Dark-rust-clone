//! A level as the editor shows it: the stage as the game loads it
//! (`custom/stages/<code>/`), its collision for picking, a world whose
//! objects (the weapons of a weapon set, the ammo crates, the doors) the view
//! draws, and its waypoint graph.

use std::collections::HashMap;
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
    /// The pickups an edit moved before the placement that follows: each
    /// one's object id and where its pad is now.
    moved: HashMap<u32, Vec3>,
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
        Ok(Scene { code: code.to_owned(), stage, level, res, world, graph, lo, hi, moved: HashMap::new() })
    }

    /// The world again with another weapon set.
    pub fn set_weapons(&mut self, weapons: [u8; 6]) -> Result<(), String> {
        self.world = settled(World::new(setup(&self.stage, weapons), self.stage.clone(), self.level.clone(), self.res.clone(), 0)?);
        self.moved.clear();
        Ok(())
    }

    /// Show the pickup on the pad at `from` (a weapon's, an ammo crate's) as
    /// it will stand once its pad is at `to`, without waiting for the
    /// placement (which reloads the scene): moved across with it, and kept
    /// as high over the floor under the pad as it was (`obj_place_3d` puts
    /// it on that floor). None there (a weapon slot the preview set leaves
    /// empty): nothing to move.
    pub fn move_pickup(&mut self, from: Vec3, to: Vec3) {
        let pad_at = |o: &pd_sim::props::Obj| self.moved.get(&o.id).copied().or_else(|| usize::try_from(o.pad).ok().and_then(|i| self.stage.pads.get(i)).map(|p| p.pos));
        let found = self.world.props.objs.iter().enumerate().filter(|(_, o)| o.door.is_none()).filter_map(|(i, o)| pad_at(o).map(|p| (i, p.distance(from)))).filter(|&(_, d)| d < 2.0).min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((i, _)) = found else { return };
        let floor = |p: Vec3| self.level.cd_find_room_at_pos_ycnp(p).map(|(y, _)| y);
        let mut pos = self.world.props.objs[i].pos + Vec3::new(to.x - from.x, 0.0, to.z - from.z);
        pos.y += match (floor(from), floor(to)) {
            (Some(a), Some(b)) => b - a,
            _ => to.y - from.y,
        };
        let (inrooms, _, best) = self.stage.rooms.bg_find_rooms_by_pos(pos, 8);
        let o = &mut self.world.props.objs[i];
        o.pos = pos;
        o.room = best.or(inrooms.first().copied()).or(o.room);
        pd_sim::props::setup::obj_update_all_geo(o);
        self.moved.insert(o.id, to);
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

    /// The doorway around a point on its floor: its jambs found by rays
    /// across it both ways (at 60 and 120 cm up, the nearer wall kept), its
    /// lintel by a ray up. Of the two ways across (`facing` and a quarter
    /// turn from it), the narrower one with both jambs within 4 m is the
    /// doorway. Returns the middle of its sill, the facing (the one of the two
    /// nearest `facing`), its width and its height (none without a lintel
    /// within 8 m: the door's own height stays).
    pub fn fit_doorway(&self, floor: Vec3, facing: f32) -> Option<(Vec3, f32, f32, Option<f32>)> {
        const REACH: f32 = 400.0;
        let ray = |from: Vec3, dir: Vec3, max: f32| self.level.first_tile_hit(from, from + dir * max).map(|h| h.dist);
        let mut best: Option<(Vec3, f32, f32)> = None;
        for turn in [0.0f32, 90.0] {
            let f = facing + turn;
            let across = Vec3::Y.cross(pd_import::layout::look(f));
            let side = |dir: Vec3| [60.0, 120.0].into_iter().filter_map(|y| ray(floor + Vec3::Y * y, dir, REACH)).reduce(f32::min);
            let (Some(r), Some(l)) = (side(across), side(-across)) else { continue };
            let width = r + l;
            if best.is_none_or(|b| width < b.2) {
                best = Some((floor + across * ((r - l) * 0.5), f, width));
            }
        }
        let (mid, f, width) = best?;
        let height = ray(mid + Vec3::Y * 20.0, Vec3::Y, 800.0).map(|d| d + 20.0);
        let sill = self.stand(mid + Vec3::Y * 20.0).map_or(mid, |p| p - Vec3::Y * crate::doc::PAD_HEIGHT);
        Some((sill, f.rem_euclid(360.0), width, height))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A dragged pickup's object follows its pad at once, along the floor
    /// (Kokiri Forest's first weapon; skipped without it imported).
    #[test]
    fn a_moved_pickup_follows_its_pad() {
        let Ok(paths) = crate::paths::EditPaths::discover() else { return };
        if !paths.is_imported("kokiri") {
            eprintln!("skipped: Kokiri Forest isn't imported");
            return;
        }
        let assets = paths.asset_dir();
        let res = Arc::new(WorldRes::load(&assets).unwrap());
        let mut scene = Scene::load(&assets, "kokiri", res, [1, 2, 3, 4, 5, 6]).unwrap();
        let (layout, _) = pd_import::layout::Layout::adopt(&paths.stage_dir("kokiri")).unwrap();
        let pad = Vec3::from(layout.weapons.unwrap()[0].pos);
        let id = |s: &Scene| s.world.props.objs.iter().find(|o| o.pad >= 0 && s.stage.pads[o.pad as usize].pos.distance(pad) < 2.0).map(|o| (o.id, o.pos)).unwrap();
        let (obj, was) = id(&scene);
        let pos = |s: &Scene| s.world.props.objs.iter().find(|o| o.id == obj).unwrap().pos;
        // Up off the floor: the object stays on it.
        let up = pad + Vec3::Y * 200.0;
        scene.move_pickup(pad, up);
        assert_eq!(pos(&scene), was);
        // Across: it goes too, and on from where the pad went.
        let over = up + Vec3::new(30.0, 0.0, 0.0);
        scene.move_pickup(up, over);
        let now = pos(&scene);
        assert!((now.x - was.x - 30.0).abs() < 0.01 && (now.z - was.z).abs() < 0.01, "{was} -> {now}");
    }

    /// GoldenEye's doors fill their doorways, so fitting one from where it
    /// stands finds its own width: Facility's single doors (skipped without
    /// Facility imported: it needs the GoldenEye ROM).
    #[test]
    fn facilitys_doors_fit_their_doorways() {
        let Ok(paths) = crate::paths::EditPaths::discover() else { return };
        if !paths.is_imported("facility") {
            eprintln!("skipped: Facility isn't imported");
            return;
        }
        let assets = paths.asset_dir();
        let res = Arc::new(WorldRes::load(&assets).unwrap());
        let scene = Scene::load(&assets, "facility", res, [0; 6]).unwrap();
        let (layout, _) = pd_import::layout::Layout::adopt(&paths.stage_dir("facility")).unwrap();
        let doors = layout.doors.unwrap();
        let (mut fitted, mut tried) = (0, 0);
        for d in doors.iter().filter(|d| d.sibling.is_none()) {
            tried += 1;
            let Some((sill, f, w, h)) = scene.fit_doorway(Vec3::from(d.pos), d.facing) else { continue };
            let turned = ((f - d.facing).rem_euclid(180.0) - 0.0).abs() > 1.0;
            if !turned && (w - d.size[0]).abs() < 15.0 && sill.distance(Vec3::from(d.pos)) < 20.0 && h.is_none_or(|h| h >= d.size[1] - 15.0) {
                fitted += 1;
            } else {
                eprintln!("door at {:?} facing {}: fitted {sill} facing {f}, {w:.0} x {h:?} (it is {:?})", d.pos, d.facing, d.size);
            }
        }
        assert!(fitted * 10 >= tried * 8, "{fitted} of {tried} single doors fitted");
    }
}
