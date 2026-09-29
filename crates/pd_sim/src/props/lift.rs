//! Lifts (`struct liftobj`, `propobj.c`): made from the setup's `lift()`
//! (`setup.c:1542`), each ticked once a frame (`lift_tick`, `propobj.c:8061`):
//! between two stops it moves from the one pad's centre towards the next
//! (`apply_speed`), carrying what stands on its floor with it
//! (`cd_get_props_on_platform`, `platform_displace_props`); at a stop it
//! opens that stop's door, and once the door is shut again (or with no door
//! at once) it sets off for the next stop.
//!
//! A lift's floor is a `GEOTYPE_TILE_F` with `GEOFLAG_LIFTFLOOR`
//! (`lift_update_tiles`), which the ground search returns with the lift:
//! that is how players and chrs know they are in one. The doors of Felicity's
//! lift call it to their stop (`OBJTYPE_LINKLIFTDOOR`, `door_call_lift`).
//!
//! Not ported: `OBJFLAG_LIFT_LATERALMOVEMENT` (no arena's lift moves
//! sideways) and the hover props' bob on a lift (none stand on one).

use glam::{Mat3, Vec2, Vec3};
use pd_core::ids::*;

use super::autogun::apply_speed;
use super::door::door_sounds;
use super::setup::{geo_part, obj_get_vertices_from_georodata, obj_update_all_geo};
use super::{Bbox, Obj};
use crate::stage::{PropFloor, PropGeo};
use crate::world::World;

/// `struct liftobj`'s own fields.
#[derive(Clone, Debug)]
pub struct Lift {
    /// The stops' pads (-1 none).
    pub pads: [i32; 4],
    /// The stops' doors, by object id.
    pub doors: [Option<u32>; 4],
    /// The setup's relative command numbers of the doors (0 none), resolved
    /// once every door exists.
    doorcmds: [i32; 4],
    /// The distance travelled from `levelcur`'s pad, and the speed.
    pub dist: f32,
    pub speed: f32,
    pub accel: f32,
    pub maxspeed: f32,
    pub soundtype: u8,
    pub levelcur: usize,
    pub levelaim: usize,
    pub prevpos: Vec3,
}

/// `struct linkliftdoorobj`: a door that calls `lift` to stop `stopnum`.
#[derive(Clone, Copy, Debug)]
pub struct LiftDoor {
    pub door: u32,
    pub lift: u32,
    pub stopnum: usize,
}

/// `obj_get_vertices_from_bbox` (`propobj.c:4884`): the box's bottom face
/// (at `ymin`) through the rotation, at the position.
fn obj_get_vertices_from_bbox(b: &Bbox, rot: &Mat3, pos: Vec3) -> [Vec3; 4] {
    let base = rot.y_axis * b.ymin + pos;
    let (x0, x1) = (rot.x_axis * b.xmin, rot.x_axis * b.xmax);
    let (z0, z1) = (rot.z_axis * b.zmin, rot.z_axis * b.zmax);
    [x0 + base + z0, x0 + base + z1, x1 + base + z1, x1 + base + z0]
}

/// `lift_get_y` (`propobj.c:4998`): the top of the lift's floor, else its position.
pub fn lift_get_y(o: &Obj) -> f32 {
    match o.floors.first() {
        Some(f) if f.flags & GEOFLAG_FLOOR1 != 0 => f.verts.iter().fold(f32::MIN, |a, v| a.max(v.y)),
        _ => o.pos.y,
    }
}

/// `lift_update_tiles` (`propobj.c:5043`): the floor (part 0's box, or a
/// non-rectangular floor's two halves, parts 5 and 6), the walls (parts 1-3)
/// and, while moving, the door blocker (part 4), at the lift's position.
/// `// SUBST:` PD's wall tiles are `GEOTYPE_TILE_F`s with `GEOFLAG_WALL` / blocks
/// over the quad's outline, as the objects' wall quads are (`obj_update_all_geo`).
pub fn lift_update_tiles(o: &mut Obj, stationary: bool, room: Option<u16>) {
    o.geos.clear();
    o.floors.clear();
    let floorflags = GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2 | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT | GEOFLAG_LIFTFLOOR;
    let (rot, pos) = (o.realrot, o.pos);
    let floor = |o: &mut Obj, verts: [Vec3; 4]| {
        let id = o.id;
        o.floors.push(PropFloor { verts, flags: floorflags, prop: id, room });
    };
    let wall = |o: &mut Obj, v: &[Vec3]| {
        let q = obj_get_vertices_from_georodata(v, &rot, pos);
        let (lo, hi) = q.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        let xz: Vec<Vec2> = q.iter().map(|p| Vec2::new(p.x, p.z)).collect();
        o.geos.push(PropGeo::block(&xz, lo, hi));
    };
    // i == 0: the floor, non-rectangular with a fallback to the box.
    if let Some(v) = geo_part(&o.def, MODELPART_LIFT_FLOORNONRECT1) {
        floor(o, obj_get_vertices_from_georodata(&v, &rot, pos));
    } else {
        let b = o
            .def
            .get_part(MODELPART_LIFT_FLOORRECT)
            .and_then(|n| match &o.def.nodes[n].kind {
                pd_core::model::NodeKind::BBox { bbox, .. } => Some(Bbox { xmin: bbox[0], xmax: bbox[1], ymin: bbox[2], ymax: bbox[3], zmin: bbox[4], zmax: bbox[5] }),
                _ => None,
            })
            .unwrap_or(o.bbox);
        floor(o, obj_get_vertices_from_bbox(&b, &rot, pos));
    }
    for part in [MODELPART_LIFT_WALL1, MODELPART_LIFT_WALL2, MODELPART_LIFT_WALL3] {
        if let Some(v) = geo_part(&o.def, part) {
            wall(o, &v);
        }
    }
    if !stationary {
        if let Some(v) = geo_part(&o.def, MODELPART_LIFT_DOORBLOCK) {
            wall(o, &v);
        }
    }
    if let Some(v) = geo_part(&o.def, MODELPART_LIFT_FLOORNONRECT2) {
        floor(o, obj_get_vertices_from_georodata(&v, &rot, pos));
    }
}

/// What stands on a platform (`cd_get_props_on_platform`).
#[derive(Clone, Copy, Debug)]
enum OnPlatform {
    Obj(u32),
    Chr(usize),
    Player(usize),
}

/// `cd_get_props_on_platform` (`collision.c:821`)'s test: `pos` over one of
/// the floors and not below it.
fn is_on_floors(floors: &[PropFloor], pos: Vec3) -> bool {
    floors.iter().any(|f| {
        let lo = f.verts.iter().copied().fold(Vec3::splat(f32::MAX), Vec3::min);
        let hi = f.verts.iter().copied().fold(Vec3::splat(f32::MIN), Vec3::max);
        f.flags & (GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2) != 0
            && pos.x >= lo.x
            && pos.x <= hi.x
            && pos.z >= lo.z
            && pos.z <= hi.z
            && pos.y >= lo.y
            && f.xz_in(pos.x, pos.z)
            && pos.y >= f.find_y(pos.x, pos.z)
    })
}

impl World {
    /// The lift from the setup's `lift()` at `p` (`setup.c:1542`): its speeds
    /// from PD's 16.16 fixed point, placed on its first pad like any object
    /// (without a core block), and its floor and walls built. Returns its id.
    pub(crate) fn setup_create_lift(&mut self, p: &serde_json::Value) -> Option<u32> {
        let int = |k: &str| p.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let flags = int("flags") as u32 & !OBJFLAG_CORE_GEO_INUSE;
        let mut o = self.setup_obj(int("model") as i32, OBJTYPE_LIFT, flags, int("flags2") as u32, int("flags3") as u32, int("maxdamage") as i32)?;
        let id = o.id;
        o.lift = Some(Box::new(Lift {
            pads: [int("pad1") as i32, int("pad2") as i32, int("pad3") as i32, int("pad4") as i32],
            doors: [None; 4],
            doorcmds: [int("door1") as i32, int("door2") as i32, int("door3") as i32, int("door4") as i32],
            dist: 0.0,
            speed: 0.0,
            accel: int("accel") as i32 as f32 / 65536.0,
            maxspeed: int("maxspeed") as i32 as f32 / 65536.0,
            // `unk84`'s first byte is `s8 soundtype` (`struct liftobj` 0x84).
            soundtype: ((int("unk84") as u32) >> 24) as u8,
            levelcur: 0,
            levelaim: 0,
            prevpos: Vec3::ZERO,
        }));
        let n = self.props.objs.len();
        self.setup_create_object(o, int("pad") as i32, int("scale") as i32);
        if self.props.objs.len() == n {
            return None;
        }
        let room = self.lift_room(n);
        let o = &mut self.props.objs[n];
        o.lift.as_mut().unwrap().prevpos = o.pos;
        lift_update_tiles(o, true, room);
        Some(id)
    }

    /// `prop->rooms`' first room for a lift. `// SUBST:` PD follows the lift
    /// through the portals (`los_find_final_room_exhaustive`) / the room its
    /// centre is in.
    fn lift_room(&self, i: usize) -> Option<u16> {
        let (inrooms, _, best) = self.stage.rooms.bg_find_rooms_by_pos(self.props.objs[i].pos, 8);
        best.or(inrooms.first().copied())
    }

    /// The lifts' doors and the doors that call lifts, from the setup's
    /// command numbers (`setup.c:1557`, `OBJTYPE_LINKLIFTDOOR` `setup.c:2041`).
    pub(crate) fn lifts_link_doors(&mut self, props: &[serde_json::Value], cmd_to_obj: &[Option<u32>], lift_cmds: &[(usize, u32)], liftdoor_cmds: &[usize]) {
        let obj_at = |rel: i64, cmd: usize| usize::try_from(cmd as i64 + rel).ok().and_then(|c| cmd_to_obj.get(c).copied().flatten());
        for &(cmd, id) in lift_cmds {
            let Some(l) = self.props.get_mut(id).and_then(|o| o.lift.as_mut()) else { continue };
            for k in 0..4 {
                if l.doorcmds[k] != 0 {
                    l.doors[k] = obj_at(l.doorcmds[k] as i64, cmd);
                }
            }
        }
        for &cmd in liftdoor_cmds {
            let p = &props[cmd];
            let int = |k: &str| p.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
            let (Some(door), Some(lift)) = (obj_at(int("dooroffset"), cmd), obj_at(int("liftoffset"), cmd)) else { continue };
            if self.props.get(door).is_none() || self.props.get(lift).is_none() {
                continue;
            }
            self.props.liftdoors.push(LiftDoor { door, lift, stopnum: int("stopnum") as usize });
            self.props.get_mut(door).unwrap().hidden |= OBJHFLAG_LIFTDOOR;
        }
    }

    /// `lift_activate` (`propobj.c:4978`).
    pub(crate) fn lift_activate(&mut self, id: u32, liftnum: u8) {
        if liftnum > 0 && (liftnum as usize) <= self.props.lifts.len() {
            self.props.lifts[liftnum as usize - 1] = Some(id);
        }
    }

    /// `lift_find_by_pad` (`propobj.c:4985`): the lift a pad's `liftnum` names.
    pub(crate) fn lift_find_by_pad(&self, pad: usize) -> Option<u32> {
        let liftnum = self.stage.pads.get(pad)?.liftnum as usize;
        if liftnum == 0 || liftnum > self.props.lifts.len() {
            return None;
        }
        self.props.lifts[liftnum - 1]
    }

    fn lift_mut(&mut self, i: usize) -> &mut Lift {
        self.props.objs[i].lift.as_mut().unwrap()
    }

    fn door_index(&self, id: Option<u32>) -> Option<usize> {
        let id = id?;
        self.props.objs.iter().position(|o| o.id == id && o.door.is_some())
    }

    /// `lift_go_to_stop` (`propobj.c:5120`, NTSC 1.0+): at rest (or with the
    /// stop's door still open) the lift just takes the new stop; moving, it
    /// keeps going if the new stop lies on its way on every axis, else turns
    /// round where it is.
    pub(crate) fn lift_go_to_stop(&mut self, i: usize, stopnum: usize) {
        let l = self.props.objs[i].lift.as_ref().unwrap();
        if l.pads[stopnum] < 0 || l.levelaim == stopnum {
            return;
        }
        let curdoor = self.door_index(l.doors[l.levelcur]);
        if l.levelcur == l.levelaim || curdoor.is_some_and(|d| !self.props.objs[d].door.as_ref().unwrap().is_closed()) {
            // Sanity check to make sure lift is actually not moving.
            if l.dist == 0.0 && l.speed == 0.0 {
                self.lift_mut(i).levelaim = stopnum;
                return;
            }
        }
        let pad = |k: usize| self.stage.pads[l.pads[k] as usize].pos;
        let (cur, aim, req) = (pad(l.levelcur), pad(l.levelaim), pad(stopnum));
        let ahead = |c: f32, a: f32, r: f32| (a >= c && r >= c) || (c >= a && c >= r);
        let levelcur = l.levelcur;
        let l = self.lift_mut(i);
        if stopnum != levelcur && ahead(cur.x, aim.x, req.x) && ahead(cur.y, aim.y, req.y) && ahead(cur.z, aim.z, req.z) {
            // Same direction.
            l.levelaim = stopnum;
        } else {
            // Reverse direction.
            let result = (aim - cur).length();
            l.levelcur = l.levelaim;
            l.dist = result - l.dist;
            l.speed = -l.speed;
            l.levelaim = stopnum;
        }
    }

    /// `lift_tick` (`propobj.c:8061`).
    pub(crate) fn lift_tick(&mut self, i: usize) {
        let pos = self.props.objs[i].pos;
        self.lift_mut(i).prevpos = pos;
        let l = self.props.objs[i].lift.as_ref().unwrap().clone();
        if l.levelcur != l.levelaim {
            // Not at the stop it wants: move, unless disabled or the door
            // needs to shut first.
            let curdoor = self.door_index(l.doors[l.levelcur]);
            if self.props.objs[i].flags & OBJFLAG_DEACTIVATED != 0 {
                return;
            }
            if let Some(d) = curdoor.filter(|&d| !self.props.objs[d].door.as_ref().unwrap().is_closed()) {
                self.doors_request_mode(d, super::door::DOORMODE_CLOSING);
                return;
            }
            let prevpos = pos;
            let onplatform = self.cd_get_props_on_platform(i);
            let (opening, _, opened, _) = door_sounds(l.soundtype);
            if l.dist == 0.0 && l.speed == 0.0 {
                self.door_play(i, opening);
                self.lift_trigger_disable(i);
            }
            let curcentre = self.stage.pads[l.pads[l.levelcur] as usize].centre();
            let padcur = self.stage.pads[l.pads[l.levelcur] as usize].pos;
            let padaim = self.stage.pads[l.pads[l.levelaim] as usize].pos;
            let diff = padaim - padcur;
            let segdist = diff.length();
            let lv = self.lv.clone();
            let lm = self.lift_mut(i);
            let prevdist = lm.dist;
            let (accel, maxspeed) = (lm.accel, lm.maxspeed);
            apply_speed(&lv, &mut lm.dist, segdist, &mut lm.speed, accel, accel, maxspeed);
            // Arriving: set the distance exactly.
            if lm.speed < 1.0 && lm.speed > -1.0 {
                if prevdist < segdist && lm.dist >= segdist {
                    lm.dist = segdist;
                } else if prevdist > 0.0 && lm.dist <= 0.0 {
                    lm.dist = 0.0;
                }
            }
            let frac = if segdist == 0.0 { 0.0 } else { lm.dist / segdist };
            let newpos = curcentre + diff * frac;
            let mut arrived = None;
            if segdist == lm.dist {
                lm.dist = 0.0;
                lm.speed = 0.0;
                lm.levelcur = lm.levelaim;
                arrived = Some(lm.doors[lm.levelcur]);
            }
            if let Some(door) = arrived {
                self.door_play(i, [opened, 0, 0]);
                self.lift_trigger_disable(i);
                if let Some(d) = self.door_index(door).filter(|&d| self.props.objs[d].door.as_ref().unwrap().keyflags == 0) {
                    self.doors_request_mode(d, super::door::DOORMODE_OPENING);
                }
            }
            self.props.objs[i].pos = newpos;
            let stationary = {
                let l = self.props.objs[i].lift.as_ref().unwrap();
                l.levelcur == l.levelaim
            };
            let room = self.lift_room(i);
            // obj_onmoved (the lift has no core block and no basic parts), then
            // lift_update_tiles.
            obj_update_all_geo(&mut self.props.objs[i]);
            lift_update_tiles(&mut self.props.objs[i], stationary, room);
            self.platform_displace_props(i, &onplatform, prevpos, newpos);
        } else {
            // At the stop it wants: once its door is shut (or with none), on
            // to the next stop.
            let door = self.door_index(l.doors[l.levelcur]);
            let go = match door {
                None => true,
                Some(d) => {
                    let d = self.props.objs[d].door.as_ref().unwrap();
                    d.is_closed() && d.keyflags == 0
                }
            };
            if go {
                let mut stop = l.levelaim;
                loop {
                    stop = (stop + 1) % 4;
                    if l.pads[stop] >= 0 {
                        break;
                    }
                }
                self.lift_go_to_stop(i, stop);
            }
        }
    }

    /// `OBJFLAG_LIFT_TRIGGERDISABLE`: the lift disables itself once it has moved.
    fn lift_trigger_disable(&mut self, i: usize) {
        let o = &mut self.props.objs[i];
        if o.flags & OBJFLAG_LIFT_TRIGGERDISABLE != 0 {
            o.flags &= !OBJFLAG_LIFT_TRIGGERDISABLE;
            o.flags |= OBJFLAG_DEACTIVATED;
        }
    }

    /// `cd_get_props_on_platform` (`collision.c:821`): the props standing over
    /// the platform's floors, at or above them. `// SUBST:` PD asks the props in
    /// the platform's rooms / every prop, which the floor test then narrows.
    fn cd_get_props_on_platform(&self, i: usize) -> Vec<OnPlatform> {
        let platform = &self.props.objs[i];
        let floors = &platform.floors;
        let mut out = Vec::new();
        for o in &self.props.objs {
            if o.id != platform.id && !o.is_gone() && is_on_floors(floors, o.pos) {
                out.push(OnPlatform::Obj(o.id));
            }
        }
        for (k, c) in self.chrs.iter().enumerate() {
            if c.player.is_none() && is_on_floors(floors, c.pos) {
                out.push(OnPlatform::Chr(k));
            }
        }
        for (k, p) in self.players.iter().enumerate() {
            if is_on_floors(floors, p.pos) {
                out.push(OnPlatform::Player(k));
            }
        }
        out
    }

    /// `platform_displace_props` (`propobj.c:7894`, NTSC 1.0+): what stood on
    /// the platform moves with it. An object (not attached, not flying) moves
    /// by the whole delta; a chr not falling rides up and down with its
    /// ground; a player in this lift, walking, not on a ladder and not
    /// falling, rides it vertically (checking the ceiling going down when the
    /// lift asks, `OBJFLAG_LIFT_CHECKCEILING`).
    fn platform_displace_props(&mut self, i: usize, onplatform: &[OnPlatform], prevpos: Vec3, newpos: Vec3) {
        let delta = newpos - prevpos;
        let (liftid, liftflags) = (self.props.objs[i].id, self.props.objs[i].flags);
        for &p in onplatform {
            match p {
                OnPlatform::Obj(id) => {
                    let Some(o) = self.props.get_mut(id) else { continue };
                    if o.hidden & OBJHFLAG_ATTACHED != 0 {
                        continue;
                    }
                    if o.projectile.as_ref().is_some_and(|pr| pr.flags & (PROJECTILEFLAG_SETTLING | PROJECTILEFLAG_SLIDING) == 0) {
                        continue;
                    }
                    o.pos += delta;
                    obj_update_all_geo(o);
                }
                OnPlatform::Chr(k) => {
                    let c = &mut self.chrs[k];
                    if c.fallspeed.y == 0.0 {
                        c.ground += delta.y;
                        c.manground += delta.y;
                        c.sumground = c.manground * 9.999_998;
                        c.pos += delta;
                        c.model.chrinfo.set_root_position(c.pos);
                        c.model.chrinfo.ground += delta.y;
                    }
                }
                OnPlatform::Player(pi) => {
                    let pl = &self.players[pi];
                    if pl.lift != Some(liftid) || !pl.inlift || pl.onladder || pl.isfalling || delta.y == 0.0 {
                        continue;
                    }
                    let ydist = delta.y;
                    self.players[pi].ground += ydist;
                    if ydist > 0.0 || liftflags & OBJFLAG_LIFT_CHECKCEILING == 0 {
                        let pl = &mut self.players[pi];
                        pl.pos.y += ydist;
                        pl.manground += ydist;
                        pl.sumground = pl.manground / 0.045_499_98;
                    } else {
                        let cyls = self.perims_except(pi);
                        let floors = self.prop_floors();
                        let (fastmovement, shieldfrac, menuopen, briefcase) = self.walk_opts(pi);
                        let env = crate::player::WalkEnv { level: &self.level, cyls: &cyls, floors: &floors, fastmovement, shieldfrac, menuopen, briefcase };
                        let pl = &mut self.players[pi];
                        if pl.bwalk_try_move_upwards(&env, ydist) == crate::stage::CdResult::NoCollision {
                            pl.manground += ydist;
                            pl.sumground = pl.manground / 0.045_499_98;
                        }
                    }
                    self.player_update_rooms(pi);
                    self.sync_player_chr(pi);
                }
            }
        }
    }

    /// `door_call_lift` (`propobj.c:176`): a lift door calls its lift to its
    /// stop, unless someone is in the lift. True: the activation is handled
    /// (the caller doesn't open or close the door). With `allowclose` (a
    /// player using the door), an open door of a lift that is there is left to
    /// the caller to close.
    pub(crate) fn door_call_lift(&mut self, door: usize, allowclose: bool) -> bool {
        let o = &self.props.objs[door];
        if o.hidden & OBJHFLAG_LIFTDOOR == 0 {
            return false;
        }
        let doorid = o.id;
        let links: Vec<LiftDoor> = self.props.liftdoors.iter().filter(|l| l.door == doorid).copied().collect();
        let mut handled = false;
        for link in links {
            let Some(li) = self.props.objs.iter().position(|x| x.id == link.lift) else { continue };
            handled = true;
            if self.props.objs[li].door.is_some() {
                // A door named as the lift: activate it (unused in PD's setups).
                self.doors_activate(li, allowclose);
            } else if self.props.objs[li].lift.is_some() {
                let d = self.props.objs[door].door.as_ref().unwrap();
                if allowclose && self.props.objs[door].ty == OBJTYPE_DOOR && !d.is_closed() {
                    handled = false;
                } else {
                    // @bug (kept): a dead chr still in the lift keeps it occupied.
                    let vacant = !self.players.iter().any(|p| p.lift == Some(link.lift)) && !self.chrs.iter().any(|c| c.player.is_none() && c.lift == Some(link.lift));
                    if vacant {
                        self.lift_go_to_stop(li, link.stopnum);
                    }
                }
            }
        }
        handled
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// The centre of an object's first floor, at its top.
    pub fn floor_centre(o: &Obj) -> Vec3 {
        let v = &o.floors[0].verts;
        let y = v.iter().fold(f32::MIN, |a, p| a.max(p.y));
        Vec3::new((v[0].x + v[2].x) * 0.5, y, (v[0].z + v[2].z) * 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil;

    /// Every arena with lifts makes them on their first pad, with a lift
    /// floor on a face of that pad's box, and activates them by number.
    #[test]
    fn every_arenas_lifts_are_made() {
        // Ravine, Pipes, Base, Sewers, Fortress, Grid.
        for (code, n) in [("arec", 1), ("crad", 1), ("mp1", 2), ("mp10", 3), ("mp12", 8), ("mp15", 1)] {
            let w = testutil::arena(code);
            let lifts: Vec<&Obj> = w.props.objs.iter().filter(|o| o.lift.is_some()).collect();
            assert_eq!(lifts.len(), n, "{code}: lifts");
            for o in &lifts {
                let l = o.lift.as_ref().unwrap();
                let pad = &w.stage.pads[l.pads[0] as usize];
                // A platform's floor is its box's top (the pad box's top); an
                // enclosed lift's (Grid's) its bottom.
                let (bottom, top) = (pad.pos.y + pad.bbox[2], pad.pos.y + pad.bbox[3]);
                let y = lift_get_y(o);
                assert!((y - top).abs() < 1.0 || (y - bottom).abs() < 1.0, "{code}: lift floor at {y} for a pad box {bottom}..{top}");
                assert!(o.floors[0].flags & GEOFLAG_LIFTFLOOR != 0);
                assert!(w.props.lifts.contains(&Some(o.id)), "{code}: lift {} not activated", o.id);
            }
        }
    }

    use super::tests_support::floor_centre;

    /// A player standing on each arena's lifts rides every one through all
    /// its stops (Grid's opening and shutting its doors on the way): the feet
    /// stay on the lift's floor all the way.
    #[test]
    fn the_player_rides_every_lift() {
        for code in ["arec", "crad", "mp1", "mp10", "mp12", "mp15"] {
            let ids: Vec<u32> = testutil::arena(code).props.objs.iter().filter(|o| o.lift.is_some()).map(|o| o.id).collect();
            for id in ids {
                let mut w = testutil::arena(code);
                let c = floor_centre(w.props.get(id).unwrap());
                let (floors, level) = (w.prop_floors(), w.level.clone());
                w.players[0].start_new_life(&level, &floors, c + Vec3::Y * 100.0, 0.0);
                assert!((w.players[0].manground - c.y).abs() < 1.0, "{code} lift {id}: not standing on it: {} for {}", w.players[0].manground, c.y);
                let idle = crate::player::PlayerInput::default();
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for f in 0..60 * 60 {
                    w.step(4, std::slice::from_ref(&idle));
                    let ly = lift_get_y(w.props.get(id).unwrap());
                    let p = &w.players[0];
                    assert!((p.manground - ly).abs() < 3.0, "{code} lift {id}, frame {f}: feet at {} on a lift at {ly}", p.manground);
                    assert!(!p.isdead, "{code} lift {id}: died");
                    lo = lo.min(ly);
                    hi = hi.max(ly);
                }
                let o = w.props.get(id).unwrap();
                let l = o.lift.as_ref().unwrap();
                let rise = o.floors[0].verts[0].y - o.pos.y;
                let heights: Vec<f32> = l.pads.iter().filter(|&&p| p >= 0).map(|&p| w.stage.pads[p as usize].centre().y + rise).collect();
                let (want_lo, want_hi) = heights.iter().fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
                assert!((hi - want_hi).abs() < 1.0 && (lo - want_lo).abs() < 1.0, "{code} lift {id} went {lo}..{hi}, its stops span {want_lo}..{want_hi}");
            }
        }
    }

    /// A simulant sent from Sewers' lower level to the top of its first lift
    /// waits at the bottom for the lift, rides it up and walks off at the top.
    #[test]
    fn a_simulant_rides_a_lift_up() {
        use crate::harness::{self, NavChoice};
        use crate::stage::{Stage, TileLevel};
        use std::sync::Arc;
        let stage = Arc::new(Stage::load(&testutil::assets(), "mp10").unwrap());
        let level = Arc::new(TileLevel::for_stage(&stage));
        let setup = pd_core::mp::MatchSetup { stagenum: stage.stagenum, ..harness::setup(1, 1, BOTDIFF_NORMAL) };
        let mut w = harness::world(stage.clone(), level.clone(), testutil::res(), setup, NavChoice::Pd, harness::SPIKE_SEED, false).unwrap();
        w.bot_brains = false;
        for _ in 0..200 {
            harness::step_idle(&mut w);
        }
        let sim = w.chrs.iter().position(|c| c.aibot.is_some()).unwrap();
        // Pad 0 (the lower level) to pad 98 (beside the lift's top stop).
        let ground = |p: Vec3| level.cd_find_ground_at_cyl(p, 20.0).0;
        let (from, to) = (stage.pads[0].pos, stage.pads[98].pos);
        harness::place(&mut w, sim, Vec3::new(from.x, ground(from), from.z), 0.0);
        harness::step_idle(&mut w);
        assert!(w.chr_go_to_pos(sim, to), "no route");
        let mut rode = false;
        let mut waited = false;
        for _ in 0..60 * 40 {
            harness::step_idle(&mut w);
            let c = &w.chrs[sim];
            rode |= c.inlift;
            waited |= c.liftaction == LIFTACTION_WAITINGFORLIFT;
            if c.actiontype != crate::chr::Act::GoPos {
                break;
            }
        }
        let c = &w.chrs[sim];
        assert!(waited && rode, "waited {waited}, rode {rode}");
        assert!(c.actiontype != crate::chr::Act::GoPos, "still going: at {} (liftaction {})", c.pos, c.liftaction);
        let top = ground(to);
        assert!((c.manground - top).abs() < 5.0 && Vec2::new(c.pos.x - to.x, c.pos.z - to.z).length() < 60.0, "stopped at {} (feet {}), not at {to} (floor {top})", c.pos, c.manground);
    }

    /// Grid's lift opens the door of the stop it arrives at and moves only
    /// with that door shut; a player using a lift door calls the lift to that
    /// door's stop rather than opening the door.
    #[test]
    fn grids_lift_works_its_doors() {
        let mut w = testutil::arena("mp15");
        let li = w.props.objs.iter().position(|o| o.lift.is_some()).unwrap();
        let id = w.props.objs[li].id;
        let doors = w.props.objs[li].lift.as_ref().unwrap().doors;
        assert!(doors[0].is_some() && doors[1].is_some(), "the lift's stops have doors: {doors:?}");
        assert_eq!(w.props.liftdoors.len(), 4);
        let frac = |w: &World, d: Option<u32>| w.props.get(d.unwrap()).unwrap().door.as_ref().unwrap().frac;
        let idle = crate::player::PlayerInput::default();
        let mut arrivals = 0;
        let mut prev = w.props.get(id).unwrap().lift.as_ref().unwrap().levelcur;
        for _ in 0..60 * 60 {
            w.step(4, std::slice::from_ref(&idle));
            let l = w.props.get(id).unwrap().lift.as_ref().unwrap();
            if l.levelcur != l.levelaim {
                // Moving (or about to): the departure stop's door is shut, or shutting first.
                assert!(l.dist == 0.0 || frac(&w, doors[l.levelcur]) == 0.0, "the lift moved with its door open");
            }
            if l.levelcur != prev {
                arrivals += 1;
                prev = l.levelcur;
            }
        }
        assert!(arrivals >= 2, "the lift arrived {arrivals} times in a minute");
        // A lift door (not the one at the lift's stop) calls the lift instead of opening.
        let mut w = testutil::arena("mp15");
        let li = w.props.objs.iter().position(|o| o.lift.is_some()).unwrap();
        let link = *w.props.liftdoors.iter().find(|l| l.stopnum == 1).unwrap();
        let di = w.props.objs.iter().position(|o| o.id == link.door).unwrap();
        w.doors_activate(di, true);
        let l = w.props.objs[li].lift.as_ref().unwrap();
        assert_eq!(l.levelaim, 1);
        assert_eq!(w.props.objs[di].door.as_ref().unwrap().mode, super::super::door::DOORMODE_IDLE, "the called door opened itself");
    }
}
