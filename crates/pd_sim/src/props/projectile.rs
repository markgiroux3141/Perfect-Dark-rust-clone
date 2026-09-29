//! Objects in flight (`propobj.c`): `projectile_launch` (`:6230`),
//! `projectile_tick`'s AIRBORNE and SETTLING branches (`:6279`), the sticky
//! move `func0f06cd00` (`:3256`, the BG's triangles then the props) and the
//! non-sticky one `func0f06d37c` (`:3401`, a 10 cm cylinder against the wall
//! tiles), `obj_stick` (`:4150`), and `props_tick_player`'s object loop
//! (`prop.c:1701` → `obj_tick_player`, `propobj.c:11054`).
//!
//! PD's `SLIDING` branch pushes furniture and hoverprops; nothing the guns make
//! slides, so it is not ported.

use glam::{Mat3, Mat4, Vec3};
use pd_core::ids::*;

use super::{Embed, Obj};
use crate::fx::boltbeam::BoltOwner;
use crate::propsnd::DEFAULT_DISTS;
use crate::stage::bghit::SURFACETYPE_DEEPWATER;
use crate::stage::{CdResult, PropGeo};
use crate::world::World;

/// What a projectile's move met: PD's `g_EmbedProp` (and `g_EmbedTextureNum`'s
/// use). `None` from the tests is the background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedProp {
    Board(usize),
    /// A chr (`g_EmbedProp` a chr or player) and the part hit
    /// (`g_EmbedHitPart`; 0 for a perimeter hit).
    Chr { chr: usize, hitpart: i32 },
}

impl World {
    /// `props_tick_player`'s objects for player `pi`'s pass (`prop.c:1701`):
    /// every object once, projectiles last (`TICKOP_RETICK` moves them to the end
    /// of the list), a deleted object freed. The first pass of a frame marks
    /// everything not yet ticked (`PROPFLAG_NOTYETTICKED`).
    pub(crate) fn props_tick_player(&mut self, pi: usize) {
        // door_tick, lift_tick and hoverprop_tick, on the object's full tick
        // (the first pass). `// SUBST:` PD ticks each door, lift and hover prop
        // in its turn among the objects / every one of them first, in list
        // order: a door looks at its siblings and at what stands in its way, a
        // lift moves what stands on it, and a hover prop finds the floors
        // under it, all of which the loop below has taken out of the world.
        if pi == 0 {
            self.props_tick_machines();
        }
        self.tinted_glass_update_portals(pi);
        let mut objs = std::mem::take(&mut self.props.objs);
        if pi == 0 {
            for o in objs.iter_mut() {
                o.notyetticked = true;
            }
        }
        let order: Vec<usize> = (0..objs.len()).filter(|&i| objs[i].projectile.is_none()).chain((0..objs.len()).filter(|&i| objs[i].projectile.is_some())).collect();
        let mut keep = vec![true; objs.len()];
        for i in order {
            keep[i] = self.obj_tick_player(&mut objs[i], pi);
        }
        let mut k = keep.into_iter();
        objs.retain(|_| k.next().unwrap());
        // Objects made while ticking (none yet) go after the old ones.
        objs.append(&mut self.props.objs);
        self.props.objs = objs;
    }

    /// The doors, lifts and hover props' full ticks, in list order (see
    /// [`Self::props_tick_player`]).
    pub(crate) fn props_tick_machines(&mut self) {
        for i in 0..self.props.objs.len() {
            if self.props.objs[i].door.is_some() {
                self.door_tick(i);
            } else if self.props.objs[i].lift.is_some() {
                self.lift_tick(i);
            } else if self.props.objs[i].hov.is_some() {
                self.hoverprop_tick(i);
            }
        }
    }

    /// Not PD: a headless world's (no player to tick the objects in) freeing
    /// of the objects marked for it (`obj_tick_player`'s first step): a
    /// taken scenario token, a removed crate.
    pub(crate) fn props_free_deleting(&mut self) {
        let mut objs = std::mem::take(&mut self.props.objs);
        objs.retain_mut(|o| !o.is_deleting() || self.obj_free_deleting(o));
        objs.append(&mut self.props.objs);
        self.props.objs = objs;
    }

    /// `obj_tick_player`'s `OBJHFLAG_DELETING` step (`obj_free`,
    /// `propobj.c:2453`). False: the object goes.
    fn obj_free_deleting(&mut self, o: &mut Obj) -> bool {
        // obj_free: a bolt's trail lets go of it.
        if o.weaponnum == WEAPON_BOLT {
            if let Some(i) = self.fx.boltbeams.find(BoltOwner::Prop(o.id)) {
                self.fx.boltbeams.set_automatic(i, 1400.0);
            }
        }
        // obj_free: a held rocket's hand lets go of it.
        for p in self.players.iter_mut() {
            for h in p.gun.hands.iter_mut() {
                if h.rocket == Some(o.id) {
                    h.rocket = None;
                }
            }
        }
        // A setup object (broken glass) waits to come back; anything else goes.
        if o.hidden2 & OBJH2FLAG_CANREGEN != 0 {
            self.obj_free_to_regen(o);
            return true;
        }
        false
    }

    /// `obj_tick_player` (`propobj.c:11054`) for a gun's object. False: free it.
    fn obj_tick_player(&mut self, o: &mut Obj, pi: usize) -> bool {
        if o.is_deleting() {
            return self.obj_free_deleting(o);
        }
        let mut fulltick = o.notyetticked;
        o.notyetticked = false;
        // A player's projectile ticks in its owner's pass; a simulant's in the
        // first pass (`propobj.c:11113`).
        if let Some(owner) = o.projectile.as_ref().and_then(|p| p.ownerprop).filter(|&c| c < self.players.len()) {
            fulltick = owner == pi;
        }
        if fulltick {
            if o.projectile.is_some() {
                self.projectile_tick(o, pi);
            }
            if o.ty == OBJTYPE_AUTOGUN && o.flags & OBJFLAG_DEACTIVATED == 0 {
                self.autogun_tick(o);
            }
            // obj_child_tick_player (`:4755`).
            if o.ty == OBJTYPE_WEAPON {
                self.weapon_tick(o, pi);
            }
        }
        if o.hidden & OBJHFLAG_DAMAGEFORBOUNCE != 0 {
            o.hidden &= !OBJHFLAG_DAMAGEFORBOUNCE;
            // obj_damage(obj, RANDOMFRAC() * 4 + 2, pos, WEAPON_NONE, owner): a
            // weapon object is bounceable and invincible, so only the draw.
            let _ = self.rng.randomfrac();
        }
        if fulltick && o.ty == OBJTYPE_AUTOGUN {
            self.autogun_tick_shoot(o, pi);
        }
        true
    }

    /// Every chr's perimeter a moving object collides with (`CDTYPE_ALL`), the
    /// owner's left out (`prop_set_perim_enabled(ownerprop, false)`).
    fn obj_cyls(&self, owner: Option<usize>) -> Vec<PropGeo> {
        let players = self.players.iter().enumerate().filter(|&(j, p)| Some(j) != owner && !p.isdead).map(|(_, p)| p.perim());
        let chrs = self.chrs.iter().enumerate().filter(|&(j, c)| c.player.is_none() && Some(j) != owner).filter_map(|(_, c)| c.perim());
        players.chain(chrs).collect()
    }

    /// `projectile_launch` (`propobj.c:6230`): the first step, from where the
    /// object was placed to `nextsteppos` (the muzzle), with the owner's
    /// perimeter off.
    pub(crate) fn projectile_launch(&mut self, o: &mut Obj, pi: usize, arg2: &mut Vec3, arg3: &mut Vec3) -> CdResult {
        let Some(next) = o.projectile.as_ref().map(|p| p.nextsteppos) else { return CdResult::NoCollision };
        let (cdresult, _) = self.func0f06cd00(o, pi, next, arg2, arg3);
        if cdresult == CdResult::NoCollision {
            o.pos = next;
        } else if o.ty == OBJTYPE_WEAPON && (o.weaponnum == WEAPON_ROCKET || o.weaponnum == WEAPON_HOMINGROCKET) {
            o.timer240 = 0;
            o.pos = *arg2;
        }
        if let Some(p) = o.projectile.as_mut() {
            p.flags &= !PROJECTILEFLAG_LAUNCHING;
        }
        cdresult
    }

    /// `func0f06cd00` (`propobj.c:3256`) for a STICKY projectile: the segment
    /// from the object to `pos` against the BG (`bg_test_hit_in_room`), then the
    /// props along what is left of it (`projectile_find_colliding_prop`). On a
    /// collision `arg2` is the hit backed off 0.1 cm and `arg3` the unit normal.
    pub(crate) fn func0f06cd00(&mut self, o: &mut Obj, _pi: usize, pos: Vec3, arg2: &mut Vec3, arg3: &mut Vec3) -> (CdResult, Option<EmbedProp>) {
        let mut cdresult = CdResult::NoCollision;
        let mut embed = None;
        let sticky = o.projectile.as_ref().is_some_and(|p| p.flags & PROJECTILEFLAG_STICKY != 0);
        if o.pos == pos || !sticky {
            return (cdresult, embed);
        }
        let mut sp1c4 = pos;
        if let Some(hit) = self.stage.bghit.bg_test_hit(o.pos, sp1c4) {
            let mut s0 = true;
            if hit.surface.is_some_and(|s| s.surfacetype == SURFACETYPE_DEEPWATER) {
                s0 = false;
                self.fx.sparks.create(&mut self.rng, hit.pos, Vec3::ZERO, hit.normal, SPARKTYPE_DEEPWATER);
                self.sound_at(0x8080, 1.0, o.pos, DEFAULT_DISTS);
                o.hidden |= OBJHFLAG_DELETING;
            }
            let between = |a: f32, b: f32, h: f32| (a <= b && h <= b && a <= h) || (b <= a && b <= h && h <= a);
            if s0 && between(o.pos.x, sp1c4.x, hit.pos.x) && between(o.pos.y, sp1c4.y, hit.pos.y) && between(o.pos.z, sp1c4.z, hit.pos.z) && o.pos != hit.pos {
                cdresult = CdResult::Collision;
                sp1c4 = hit.pos;
                *arg3 = hit.normal;
            }
        }
        match self.projectile_find_colliding_prop(o, o.pos, sp1c4, arg2, arg3) {
            Some(e) => {
                cdresult = CdResult::Collision;
                embed = Some(e);
            }
            None => {
                if cdresult == CdResult::Collision {
                    *arg2 = sp1c4;
                }
            }
        }
        if cdresult != CdResult::NoCollision {
            let d = pos - o.pos;
            let distance = d.length();
            let mult = if distance > 0.1 { 0.1 / distance } else { 0.5 };
            *arg2 -= d * mult;
            if *arg3 != Vec3::ZERO {
                *arg3 = arg3.normalize();
            } else {
                arg3.z = 1.0;
            }
        }
        (cdresult, embed)
    }

    /// `projectile_find_colliding_prop` (`propobj.c:3140`): the nearest prop
    /// along `pos1 → pos2`, writing the hit point and normal: the boards, and
    /// the chrs but the owner (whose perimeter the move turns off). A simulant
    /// drawn this tick by its part boxes and triangles (`projectile_0f06c28c` →
    /// `chr_test_hit`), otherwise, like a player, by its perimeter
    /// (`projectile_0f06b488`, the torso).
    /// `// SUBST:` PD tests an object's model part boxes and triangles
    /// (`projectile_0f06b610`) / the firing range's boards are boxes; a
    /// shielded chr's boxes grow by 10 cm (`var8005efc0`) / they don't.
    fn projectile_find_colliding_prop(&self, o: &Obj, pos1: Vec3, pos2: Vec3, arg4: &mut Vec3, arg5: &mut Vec3) -> Option<EmbedProp> {
        let d = pos2 - pos1;
        let dist = d.length();
        if dist == 0.0 {
            return None;
        }
        let dir = d / dist;
        let mut best: Option<(f32, EmbedProp, Vec3, Vec3)> = None;
        for (i, b) in self.boards.iter().enumerate() {
            if let Some((t, n)) = crate::gun::shot::ray_box(b.min, b.max, pos1, dir, dist) {
                if best.as_ref().is_none_or(|(bt, ..)| t < *bt) {
                    best = Some((t, EmbedProp::Board(i), pos1 + dir * t, n));
                }
            }
        }
        let owner = o.projectile.as_ref().and_then(|p| p.ownerprop);
        for (ci, c) in self.chrs.iter().enumerate() {
            if Some(ci) == owner || c.actiontype == crate::chr::Act::Dead || c.player.is_some_and(|p| self.players[p].isdead) {
                continue;
            }
            let r = c.chr_get_hit_radius();
            let spd4 = (c.pos - pos1).dot(dir);
            if c.player.is_none() {
                if !(-r <= spd4 && spd4 <= dist + r && crate::chr::body::pos_is_facing_pos(pos1, dir, c.pos, r)) {
                    continue;
                }
                if c.onscreen {
                    if let Some(h) = c.chr_test_hit(pos1, dir, false) {
                        let t = (h.pos - pos1).dot(dir);
                        if t <= dist && best.as_ref().is_none_or(|(bt, ..)| t < *bt) {
                            best = Some((t, EmbedProp::Chr { chr: ci, hitpart: h.hitpart }, h.pos, h.normal));
                        }
                    }
                    continue;
                }
            }
            // projectile_0f06b488: the segment against the perimeter; the normal
            // faces back along the flight, level.
            let Some(perim) = (match c.player {
                Some(p) => Some(self.players[p].perim()),
                None => c.perim(),
            }) else {
                continue;
            };
            if let Some(f) = crate::props::autogun::segment_cyl(pos1, pos2, &perim) {
                let t = f * dist;
                if best.as_ref().is_none_or(|(bt, ..)| t < *bt) {
                    let n = Vec3::new(-dir.x, 0.0, -dir.z).try_normalize().unwrap_or(Vec3::Z);
                    let hitpart = if c.player.is_some() { 0 } else { HITPART_TORSO };
                    best = Some((t, EmbedProp::Chr { chr: ci, hitpart }, pos1 + dir * t, n));
                }
            }
        }
        let (_, e, p, n) = best?;
        *arg4 = p;
        *arg5 = n;
        Some(e)
    }

    /// `func0f06d37c` (`propobj.c:3401`): move a non-sticky object as a
    /// `obj_get_radius` (10 cm) cylinder with `CHECKVERTICAL_NO`. Blocked, it
    /// slides up to the wall (99 % of the way) if it can, and `arg2`/`arg3` get
    /// the contact point and the edge's normal. False: it hit something.
    pub(crate) fn func0f06d37c(&mut self, o: &mut Obj, arg1: Vec3, arg2: &mut Vec3, arg3: &mut Vec3) -> bool {
        let radius = 10.0;
        let mut result = true;
        let mut sp98 = false;
        let sp80 = arg1;
        if o.pos == arg1 || o.projectile.is_none() {
            return true;
        }
        let cyls = self.obj_cyls(o.projectile.as_ref().and_then(|p| p.ownerprop));
        let level = self.level.clone();
        let mut obstacle = None;
        let (r, obst) = level.cd_test_cylmove_oobok_findclosest_finddist(o.pos, sp80, radius, false, 0.0, 0.0, &cyls);
        if r != CdResult::Collision {
            let (r2, obst2) = level.cd_test_volume_fromdir(o.pos, sp80, radius, false, 0.0, 0.0, &cyls);
            if r2 != CdResult::Collision {
                o.pos = sp80;
            } else {
                result = false;
                obstacle = obst2;
            }
        } else {
            result = false;
            obstacle = obst;
        }
        if !result {
            let (sp64, sp58) = obstacle.map_or((Vec3::ZERO, Vec3::ZERO), |ob| ob.edge);
            *arg3 = Vec3::new(sp58.z - sp64.z, 0.0, sp64.x - sp58.x);
            if arg3.x != 0.0 || arg3.z != 0.0 {
                *arg3 = arg3.normalize();
            } else {
                arg3.z = 1.0;
            }
            if sp80 != o.pos {
                let sp8c = sp80 - o.pos;
                let mut c = crate::chr::chr_calculate_push_contact_pos(sp64, sp58, o.pos, sp8c);
                for a in 0..3 {
                    let (lo, hi) = if o.pos[a] < sp80[a] { (o.pos[a], sp80[a]) } else { (sp80[a], o.pos[a]) };
                    c[a] = c[a].clamp(lo, hi);
                }
                *arg2 = c;
                let f2 = obstacle.and_then(|ob| ob.dist).unwrap_or(0.0) * 0.99;
                let sp4c = Vec3::new(sp8c.x * f2 + o.pos.x, sp80.y, sp8c.z * f2 + o.pos.z);
                if level.cd_test_cylmove_oobok_findclosest(o.pos, sp4c, false, 0.0, 0.0, &cyls).0 != CdResult::Collision
                    && level.cd_test_volume_simple(sp4c, radius, false, 0.0, 0.0, &cyls) != CdResult::Collision
                {
                    o.pos = sp4c;
                    sp98 = true;
                }
            } else {
                *arg2 = sp80;
            }
            if !sp98 {
                o.pos.y = sp80.y;
            }
        }
        result
    }

    /// `projectile_tick` (`propobj.c:6279`).
    pub(crate) fn projectile_tick(&mut self, o: &mut Obj, pi: usize) -> bool {
        if self.lv.lvupdate240 <= 0 || o.projectile.is_none() {
            return false;
        }
        // WEAPON_SKROCKET flies by rocket_tick_fbw: a solo weapon.
        o.hidden &= !OBJHFLAG_ATTACHED;
        let mut sp5e8 = Vec3::ZERO;
        let mut sp5f4 = Vec3::ZERO;
        if o.projectile.as_ref().unwrap().flags & PROJECTILEFLAG_LAUNCHING != 0 {
            self.projectile_launch(o, pi, &mut sp5e8, &mut sp5f4);
        }
        let sp5dc = o.pos;
        let lv240 = self.lv.lvupdate240;
        {
            let p = o.projectile.as_mut().unwrap();
            if p.pickuptimer240 > 0 {
                p.pickuptimer240 -= lv240;
            }
        }
        let flags = o.projectile.as_ref().unwrap().flags;
        if flags & PROJECTILEFLAG_AIRBORNE != 0 {
            self.tick_airborne(o, pi, sp5dc, sp5e8, sp5f4)
        } else if flags & PROJECTILEFLAG_SETTLING != 0 {
            self.tick_settling(o, sp5dc)
        } else {
            false
        }
    }

    /// The AIRBORNE branch (`propobj.c:6735`).
    fn tick_airborne(&mut self, o: &mut Obj, pi: usize, mut sp5dc: Vec3, mut sp5e8: Vec3, mut sp5f4: Vec3) -> bool {
        let lv = self.lv.clone();
        let lv60 = lv.lvupdate60freal;
        let mut settle = false;
        let mut atground = false;
        let mut handled = false;
        {
            let p = o.projectile.as_mut().unwrap();
            p.losttimer240 += lv.lvupdate240;
            if (p.flags & PROJECTILEFLAG_NOTIMELIMIT == 0 && p.losttimer240 > 40 * 240) || o.pos.y < -20000.0 || o.pos.y > 32000.0 || o.pos.x < -32000.0 || o.pos.x > 32000.0 || o.pos.z < -32000.0 || o.pos.z > 32000.0 {
                o.hidden |= OBJHFLAG_DELETING;
            }
            p.flighttime240 += lv.lvupdate240;
        }
        let realrot = o.realrot;
        // PROJECTILEFLAG_MISSILE is implemented but no gun sets it.
        // A homing rocket turns towards `targetprop` (`propobj.c:6790`), by a
        // PD controller whose memory is one static for every rocket.
        if o.ty == OBJTYPE_WEAPON && o.weaponnum == WEAPON_HOMINGROCKET {
            if let Some(t) = o.projectile.as_ref().and_then(|p| p.targetprop).filter(|&t| t < self.chrs.len()) {
                let tpos = self.chrs[t].pos;
                let r = o.realrot;
                let sp29c = (r.x_axis.x * r.x_axis.x + r.y_axis.x * r.y_axis.x + r.z_axis.x * r.z_axis.x).sqrt();
                let mtx = Mat4::from_mat3(r * (1.0 / sp29c));
                let sp290 = (tpos - o.pos).normalize_or_zero();
                let p = o.projectile.as_mut().unwrap();
                let sp2ec = p.speed.normalize_or_zero();
                let sp28c = sp2ec.dot(sp290).clamp(-1.0, 1.0).acos();
                if !(-0.001..=0.001).contains(&sp28c) {
                    // kkd 20, kkp 120, kkg 3 (`main_override_variable`'s defaults).
                    let tmp = ((20.0 / 100.0 * self.props.homing_prevangle / lv60) + (120.0 / 100.0 * sp28c * lv60)) * (3.0 / 100.0);
                    self.props.homing_prevangle = sp28c;
                    let sp280 = Vec3::new(sp2ec.y * sp290.z - sp2ec.z * sp290.y, -(sp2ec.x * sp290.z - sp2ec.z * sp290.x), sp2ec.x * sp290.y - sp2ec.y * sp290.x);
                    let (s, c) = (tmp * 0.5).sin_cos();
                    let sp260 = [c, sp280.x * s, sp280.y * s, sp280.z * s];
                    let sp20c = pd_core::math::quaternion_to_mtx(sp260);
                    p.accel = Vec3::ZERO;
                    p.speed = sp20c.transform_vector3(p.speed);
                    let sp270 = pd_core::math::quaternion0f097044(&mtx);
                    let sp250 = pd_core::math::quaternion_mult_quaternion(sp270, sp260);
                    o.realrot = Mat3::from_mat4(pd_core::math::quaternion_to_mtx(sp250)) * sp29c;
                }
            }
        }
        {
            let p = o.projectile.as_mut().unwrap();
            if p.flags & PROJECTILEFLAG_POWERED == 0 {
                p.speed.y += (p.accel.y + p.missileyaccel) * lv60;
                let fallspeed = if p.flags & PROJECTILEFLAG_LIGHTWEIGHT != 0 { p.speed.y - (1.0 / 7.2) * lv60 } else { p.speed.y - (1.0 / 3.6) * lv60 };
                sp5dc.y += lv60 * (p.speed.y + fallspeed) * 0.5;
                p.speed.y = fallspeed;
            } else {
                p.speed.y += (p.accel.y + p.missileyaccel) * lv60;
                sp5dc.y += p.speed.y * lv60;
            }
            p.speed.x += p.accel.x * lv60;
            p.speed.z += p.accel.z * lv60;
            sp5dc.x += p.speed.x * lv60;
            sp5dc.z += p.speed.z * lv60;
            // projectile_update_matrix (`projectile.c:49`): the spin, once a quarter-tick.
            for _ in 0..lv.lvupdate240 {
                o.realrot = p.mtx * o.realrot;
            }
        }
        let prevpos = o.pos;
        let sticky = o.projectile.as_ref().unwrap().flags & PROJECTILEFLAG_STICKY != 0;
        let (mut cdresult, embed) = if sticky {
            self.func0f06cd00(o, pi, sp5dc, &mut sp5e8, &mut sp5f4)
        } else {
            let ok = self.func0f06d37c(o, sp5dc, &mut sp5e8, &mut sp5f4);
            (if ok { CdResult::NoCollision } else { CdResult::Collision }, None)
        };
        let moved = true;

        if sticky && cdresult == CdResult::Collision {
            let mut stick = false;
            if o.ty == OBJTYPE_AUTOGUN {
                // Thrown laptops stick to the BG but not props.
                stick = embed.is_none();
            } else if o.ty == OBJTYPE_WEAPON
                && (matches!(o.weaponnum, WEAPON_REMOTEMINE | WEAPON_TIMEDMINE | WEAPON_PROXIMITYMINE | WEAPON_COMMSRIDER | WEAPON_TRACERBUG | WEAPON_TARGETAMPLIFIER | WEAPON_BOLT | WEAPON_COMBATKNIFE | WEAPON_ECMMINE)
                    || o.gset_flags(&self.res.gset) & FUNCFLAG_STICKTOWALL != 0)
                {
                    stick = true;
                    if o.weaponnum == WEAPON_GRENADEROUND && o.gunfunc == FUNC_SECONDARY {
                        if o.timer240 == 1 {
                            stick = false;
                            o.timer240 = 0;
                        } else {
                            o.timer240 = 480;
                        }
                    }
                }
            // A board is an object that is not a projectile, not shielded and not
            // glass, so nothing above refuses the stick. Nothing sticks to a
            // shielded chr.
            if let Some(EmbedProp::Chr { chr, .. }) = embed {
                if self.chrs[chr].cshield > 0.0 {
                    stick = false;
                }
            }
            if !handled && embed.is_some() && o.ty == OBJTYPE_WEAPON {
                // var8009ce78: the flight's direction.
                let dir = (sp5dc - prevpos).normalize_or_zero();
                let owner = o.projectile.as_ref().and_then(|p| p.ownerprop);
                match o.weaponnum {
                    WEAPON_BOLT | WEAPON_COMBATKNIFE => match embed {
                        // Embed into an object: MODEL_TARGET's face scores
                        // (fr_calculate_hit).
                        Some(EmbedProp::Board(b)) => self.board_struck(b, 0.0),
                        // Into a chr (`propobj.c:7056`): the weapon's damage.
                        Some(EmbedProp::Chr { chr, hitpart }) => {
                            let p = o.projectile.as_ref().unwrap();
                            if p.flags & PROJECTILEFLAG_AIRBORNE != 0 && p.bouncecount <= 0 {
                                let ownershield = self.chrs[chr].cshield;
                                let damage = self.chr_gset_damage(o.weaponnum, o.gunfunc, 0.0);
                                self.chr_damage_by_impact(chr, damage, dir, crate::chr::DamageFrom::new(owner, o.weaponnum, o.gunfunc), hitpart);
                                if ownershield <= 0.0 {
                                    self.chr_emit_sparks(chr, hitpart, sp5e8, sp5f4);
                                }
                            }
                        }
                        None => {}
                    },
                    WEAPON_ROCKET | WEAPON_HOMINGROCKET => {
                        match embed {
                            // obj_damage(g_EmbedProp->obj, 100, ...).
                            Some(EmbedProp::Board(b)) => self.board_struck(b, 100.0),
                            // chr_damage_by_impact(chr, 2, ..., the owner by its bits).
                            Some(EmbedProp::Chr { chr, hitpart }) => {
                                let ownerchr = o.owner();
                                let attacker = (ownerchr < self.chrs.len()).then_some(ownerchr);
                                self.chr_damage_by_impact(chr, 2.0, dir, crate::chr::DamageFrom::new(attacker, o.weaponnum, o.gunfunc), hitpart);
                            }
                            None => {}
                        }
                        handled = true;
                        o.timer240 = 0;
                    }
                    // Grenades, N-Bombs: shieldhits only.
                    _ => {}
                }
            }
            if !handled && stick {
                handled = true;
                if o.ty == OBJTYPE_WEAPON && (o.weaponnum == WEAPON_BOLT || o.weaponnum == WEAPON_COMBATKNIFE) {
                    // mpstats_increment_player_shotcount_projectiles (M7), then
                    // the sparks off a BG or object hit.
                    let dir = o.projectile.as_ref().unwrap().speed.normalize_or_zero();
                    self.fx.sparks.create(&mut self.rng, sp5e8, dir, sp5f4, SPARKTYPE_PROJECTILE);
                }
                self.obj_stick(o, sp5e8, sp5f4, embed);
            }
        }
        if sticky && !handled {
            if cdresult != CdResult::Collision {
                o.pos = sp5dc;
            } else if matches!(embed, Some(EmbedProp::Chr { .. })) {
                // Against a chr only y moves (`propobj.c:7246`).
                sp5dc.x = o.pos.x;
                sp5dc.z = o.pos.z;
                o.pos = sp5dc;
            } else {
                sp5dc = sp5e8;
                o.pos = sp5dc;
            }
        }

        if !handled {
            let sp37c = o.bbox.rotated_y_min(&o.realrot);
            let sp5ac = Vec3::new(o.pos.x, o.pos.y + sp37c, o.pos.z);
            let level = self.level.clone();
            let ceiling = level.cd_find_ceiling_room_at_pos_ycfn(sp5ac);
            let mut roomfound;
            match ceiling {
                // The bottom sank through a floor this tick.
                Some((sp390, poly)) if o.pos.y + sp37c < sp390 && !level.los_floors(prevpos, sp5ac) => {
                    roomfound = true;
                    settle = true;
                    sp5f4 = level.geom.polys[poly].normal.normalize_or_zero();
                    sp5e8 = Vec3::new(o.pos.x, sp390, o.pos.z);
                    cdresult = CdResult::Collision;
                    // Landing on a GEOFLAG_DIE tile deletes it (`propobj.c:7304`).
                    if level.geom.polys[poly].geoflags & GEOFLAG_DIE != 0 {
                        o.hidden |= OBJHFLAG_DELETING;
                    }
                }
                _ => {
                    roomfound = level.cd_find_room_at_pos_ycnp(o.pos).is_some();
                    let p = o.projectile.as_mut().unwrap();
                    if !roomfound && p.flags & PROJECTILEFLAG_STICKY == 0 {
                        if p.flags & PROJECTILEFLAG_DONEOOBSEARCH == 0 {
                            p.flags |= PROJECTILEFLAG_DONEOOBSEARCH;
                            // SUBST: cd_find_room_at_pos asks the BSP rooms /
                            // is there a floor under the point.
                            if level.cd_find_room_at_pos_ycnp(prevpos).is_some() {
                                p.flags |= PROJECTILEFLAG_INROOM;
                            }
                        }
                        if p.flags & PROJECTILEFLAG_INROOM != 0 {
                            o.pos = prevpos;
                            roomfound = level.cd_find_room_at_pos_ycnp(o.pos).is_some();
                            p.speed.x = 0.0;
                            p.speed.z = 0.0;
                        }
                    }
                }
            }
            {
                let p = o.projectile.as_mut().unwrap();
                if roomfound {
                    p.flags |= PROJECTILEFLAG_INROOM;
                } else {
                    p.flags &= !PROJECTILEFLAG_INROOM;
                }
            }

            if cdresult == CdResult::Collision {
                // Bouncing.
                {
                    let p = o.projectile.as_mut().unwrap();
                    if (p.speed.y <= 0.0 && prevpos.y <= o.pos.y) || (p.flags & PROJECTILEFLAG_STICKY == 0 && settle) {
                        atground = true;
                    }
                    if p.hitspeedpreservationfrac > 0.0 {
                        let f0 = p.speed.dot(sp5f4) * -(p.hitspeedpreservationfrac + 1.0);
                        let oldyspeed = p.speed.y;
                        p.speed += sp5f4 * f0;
                        if oldyspeed <= 0.0 && p.speed.y >= 0.0 {
                            atground = true;
                        }
                    }
                }
                if o.ty == OBJTYPE_WEAPON && o.weaponnum == WEAPON_GRENADE && o.gunfunc == FUNC_SECONDARY && o.projectile.as_ref().unwrap().hitspeedpreservationfrac > 0.0 {
                    self.fx.smokes.smoke_create_at_prop(o.id, o.pos, SMOKETYPE_PINBALL);
                }
                if atground {
                    o.pos.y = sp5e8.y - sp37c;
                    if settle {
                        o.pos.y += obj_get_ground_clearance(o);
                    }
                }
                let newmtx = {
                    let p = o.projectile.as_ref().unwrap();
                    p.flags & PROJECTILEFLAG_BOUNCEKEEPROT == 0 && (p.bounceframe < 0 || p.bounceframe < lv.lvframe60 - 60)
                };
                if newmtx {
                    let m = super::projectile_load_random_rotation(&mut self.rng);
                    o.projectile.as_mut().unwrap().mtx = m;
                }
                let (bouncecount, sticky, hsp, speedy, forcegood) = {
                    let p = o.projectile.as_mut().unwrap();
                    p.bouncecount += 1;
                    p.bounceframe = lv.lvframe60;
                    (p.bouncecount, p.flags & PROJECTILEFLAG_STICKY != 0, p.hitspeedpreservationfrac, p.speed.y, p.flags & PROJECTILEFLAG_FORCEGOODBOUNCE != 0)
                };
                if o.hidden & OBJHFLAG_IMMUNETOBOUNCES == 0 {
                    o.hidden |= OBJHFLAG_DAMAGEFORBOUNCE;
                }
                if atground {
                    if !sticky && bouncecount >= 6 {
                        if settle {
                            o.projectile_settle(&realrot, &mut self.rng, lv60);
                        }
                    } else if hsp > 0.0 {
                        if (0.0..2.222_222_3).contains(&speedy) {
                            if forcegood && bouncecount == 1 {
                                o.projectile.as_mut().unwrap().speed.y = 2.222_222_3;
                            } else if settle {
                                o.projectile_settle(&realrot, &mut self.rng, lv60);
                            }
                        }
                    } else if settle {
                        o.projectile_settle(&realrot, &mut self.rng, lv60);
                    }
                }
            }

            if o.ty == OBJTYPE_WEAPON {
                self.weapon_flight_extras(o, cdresult == CdResult::Collision, atground, prevpos);
            }
        }
        moved
    }

    /// The weapon half of the AIRBORNE branch (`propobj.c:7426`): the knife's
    /// woosh, the rocket's power and trail, the grenade round and impact
    /// N-Bomb going off on landing, and the collision sounds.
    fn weapon_flight_extras(&mut self, o: &mut Obj, collided: bool, atground: bool, prevpos: Vec3) {
        let lv = self.lv.clone();
        if o.weaponnum == WEAPON_COMBATKNIFE && o.gunfunc == FUNC_SECONDARY {
            // knife_play_woosh_sound (`:3919`).
            let p = o.projectile.as_mut().unwrap();
            if p.flags & PROJECTILEFLAG_AIRBORNE != 0 && p.bouncecount <= 0 && o.hidden & OBJHFLAG_THROWNKNIFE != 0 {
                let _index = self.rng.random() % 3;
                if p.lastwooshframe < lv.lvframe60 - 6 {
                    // ps_stop_sound(prop, PSTYPE_GENERAL): the last woosh is short.
                    p.lastwooshframe = lv.lvframe60;
                    self.sound_at(0x8074, 1.0, o.pos, DEFAULT_DISTS);
                }
            } else {
                o.hidden &= !OBJHFLAG_THROWNKNIFE;
            }
        } else if o.weaponnum == WEAPON_ROCKET {
            if collided {
                o.timer240 = 0;
            } else {
                let p = o.projectile.as_mut().unwrap();
                if p.speed.length_squared() > 27_777.773 {
                    p.accel = Vec3::ZERO;
                }
                if p.powerlimit240 >= 0 && p.flighttime240 > p.powerlimit240 {
                    p.missileyaccel = 0.0;
                    p.flags &= !(PROJECTILEFLAG_POWERED | PROJECTILEFLAG_MISSILE);
                } else {
                    let d = p.speed.normalize_or_zero();
                    self.fx.smokes.smoke_create_simple(o.pos - d * 20.0, SMOKETYPE_ROCKETTAIL);
                }
            }
        } else if o.weaponnum == WEAPON_HOMINGROCKET {
            if collided {
                o.timer240 = 0;
            } else {
                self.fx.smokes.smoke_create_simple(o.pos, SMOKETYPE_HOMINGTAIL);
            }
        } else if o.weaponnum == WEAPON_GRENADEROUND || (o.weaponnum == WEAPON_NBOMB && o.gunfunc == FUNC_PRIMARY) {
            let p = o.projectile.as_ref().unwrap();
            let slow = p.speed.abs().max_element() < 0.1;
            let still = (o.pos - prevpos).abs().max_element() < 0.1;
            if atground || p.flags & PROJECTILEFLAG_SETTLING != 0 || slow || still {
                if o.weaponnum != WEAPON_NBOMB || o.timer240 >= 0 {
                    o.timer240 = 0;
                }
            } else if o.weaponnum != WEAPON_NBOMB {
                self.fx.smokes.smoke_create_simple(o.pos, SMOKETYPE_GRENADETAIL);
            }
        }
        if collided {
            let p = o.projectile.as_mut().unwrap();
            if p.collisionframe < lv.lvframenum - 2 {
                if o.weaponnum == WEAPON_COMBATKNIFE {
                    self.sound_at(0x808b, 1.0, o.pos, DEFAULT_DISTS);
                } else if o.weaponnum == WEAPON_GRENADE && o.gunfunc == FUNC_SECONDARY {
                    const SOUNDS: [u16; 4] = [0x0027, 0x0028, 0x0029, 0x002a];
                    let s = SOUNDS[(self.rng.random() % 4) as usize];
                    self.sound_at(s, 1.0, o.pos, DEFAULT_DISTS);
                    self.sound_at(0x808c, 1.0, o.pos, DEFAULT_DISTS);
                } else {
                    self.sound_at(0x808c, 1.0, o.pos, DEFAULT_DISTS);
                }
            }
            o.projectile.as_mut().unwrap().collisionframe = lv.lvframenum;
        }
    }

    /// The SETTLING branch (`propobj.c:7508`): turn onto the resting face, slide
    /// to a stop on the floor, and let go of the projectile.
    fn tick_settling(&mut self, o: &mut Obj, mut sp5dc: Vec3) -> bool {
        let lv = self.lv.clone();
        let mut moved = false;
        let mut stop = true;
        {
            let p = o.projectile.as_mut().unwrap();
            if p.settledrotfrac < 1.0 {
                p.settledrotfrac += p.settledrotinc * lv.lvupdate60freal;
                if lv.lvupdate60 > 0 {
                    p.settledrotinc *= 1.1;
                }
                if p.settledrotfrac > 1.0 {
                    p.settledrotfrac = 1.0;
                }
                let q = pd_core::math::quaternion_slerp(p.unk068, p.unk078, p.settledrotfrac);
                let mut spac = pd_core::math::quaternion_to_mtx(q);
                pd_core::math::scale_col0_xyz(&mut spac, p.unk0b8[0]);
                pd_core::math::scale_col1_xyz(&mut spac, p.unk0b8[1]);
                pd_core::math::scale_col2_xyz(&mut spac, p.unk0b8[2]);
                o.realrot = Mat3::from_mat4(spac);
                stop = false;
            }
        }
        let (sx, sz, frac) = {
            let p = o.projectile.as_ref().unwrap();
            (p.speed.x, p.speed.z, p.settledrotfrac)
        };
        if sx != 0.0 || sz != 0.0 || frac < 1.0 {
            let sp98 = o.bbox.rotated_y_min(&o.realrot);
            stop = false;
            {
                let p = o.projectile.as_mut().unwrap();
                for _ in 0..lv.lvupdate60 {
                    sp5dc.x += p.speed.x;
                    sp5dc.z += p.speed.z;
                    if p.settledrotfrac >= 1.0 {
                        if p.speeddecel > 0.0 {
                            let dist = (p.speed.x * p.speed.x + p.speed.z * p.speed.z).sqrt();
                            if dist > 0.0 {
                                let f12 = p.speeddecel * lv.lvupdate60freal / dist;
                                if f12 >= 1.0 {
                                    p.speed.x = 0.0;
                                    p.speed.z = 0.0;
                                } else {
                                    p.speed.x -= p.speed.x * f12;
                                    p.speed.z -= p.speed.z * f12;
                                }
                            } else {
                                p.speed.x = 0.0;
                                p.speed.z = 0.0;
                            }
                        } else {
                            p.speed.x *= 0.9;
                            p.speed.z *= 0.9;
                        }
                    }
                }
            }
            let prevpos = o.pos;
            let (mut a, mut b) = (Vec3::ZERO, Vec3::ZERO);
            self.func0f06d37c(o, sp5dc, &mut a, &mut b);
            moved = true;
            let sp5ac = Vec3::new(o.pos.x, o.pos.y + sp98, o.pos.z);
            let level = self.level.clone();
            // cd_find_ceiling_room_at_pos_ycf, unless the bottom did not cross a
            // floor on the way (cd_test_los_oobok passes): then the floor under it.
            let mut ground = level.cd_find_ceiling_room_at_pos_ycfn(sp5ac);
            if ground.is_none() || level.los_floors(prevpos, sp5ac) {
                ground = level.cd_find_room_at_pos_ycnp(o.pos);
            }
            if ground.is_none() {
                o.pos.x = prevpos.x;
                o.pos.z = prevpos.z;
                ground = level.cd_find_room_at_pos_ycnp(o.pos);
                let p = o.projectile.as_mut().unwrap();
                p.speed.x = 0.0;
                p.speed.z = 0.0;
            }
            match ground {
                Some((spa4, poly)) => {
                    o.pos.y = spa4 - sp98 + obj_get_ground_clearance(o);
                    // A GEOFLAG_DIE floor deletes it (`propobj.c:7631`).
                    if level.geom.polys[poly].geoflags & GEOFLAG_DIE != 0 {
                        o.hidden |= OBJHFLAG_DELETING;
                    }
                }
                None => o.pos.y = prevpos.y,
            }
            let p = o.projectile.as_mut().unwrap();
            if p.speed.x.abs() < 0.1 && p.speed.z.abs() < 0.1 {
                p.speed.x = 0.0;
                p.speed.z = 0.0;
            }
        }
        if stop {
            // obj_free_projectile: it has come to rest.
            o.projectile = None;
        }
        moved
    }

    /// `obj_stick` (`propobj.c:4150`) to the background or a board.
    fn obj_stick(&mut self, o: &mut Obj, pos: Vec3, rot: Vec3, embed: Option<EmbedProp>) {
        o.projectile = None;
        o.hidden |= OBJHFLAG_ATTACHED;
        if o.ty == OBJTYPE_WEAPON {
            // ECM mines, comms riders, tracer bugs and target amplifiers turn
            // invincible here; none is a Combat Simulator weapon.
            match o.weaponnum {
                WEAPON_BOLT => {
                    o.obj_stick_bolt(&mut self.rng, pos);
                    if let Some(i) = self.fx.boltbeams.find(BoltOwner::Prop(o.id)) {
                        self.fx.boltbeams.beams[i].tailpos = o.pos;
                        self.fx.boltbeams.set_automatic(i, 2100.0);
                    }
                }
                WEAPON_COMBATKNIFE => o.obj_stick_knife(&mut self.rng, pos, rot),
                _ => o.obj_stick_default(pos, rot),
            }
        } else if o.ty == OBJTYPE_AUTOGUN {
            o.obj_stick_default(pos, rot);
            if let Some(a) = o.autogun.as_mut() {
                a.yzero = pd_core::math::atan2f(rot.x, rot.z);
                a.xzero = pd_core::math::atan2f(rot.y, (rot.x * rot.x + rot.z * rot.z).sqrt());
                a.xrot = a.xzero;
                a.yrot = a.yzero;
            }
        }
        match embed {
            Some(EmbedProp::Chr { chr, .. }) => {
                // SUBST: PD embeds the blade or bolt in the chr's model
                // (`obj_embed`), carried until the chr lets go of it / it
                // stops where it hit and falls to the floor, to be picked up.
                let pos = self.chrs[chr].pos;
                self.bgun_play_prop_hit_sound_chr(o.weaponnum, o.gunfunc, pos);
                o.hidden &= !OBJHFLAG_ATTACHED;
                o.projectile = Some(super::Projectile { flags: PROJECTILEFLAG_AIRBORNE, startframe: self.lv.lvframenum, ..Default::default() });
                o.realrot = glam::Mat3::from_diagonal(Vec3::splat(o.scale));
            }
            Some(EmbedProp::Board(b)) => {
                if o.ty == OBJTYPE_WEAPON {
                    // bgun_play_prop_hit_sound: an object's.
                    let id = if self.rng.random().is_multiple_of(2) { 0x8089 } else { 0x808a };
                    self.sound_at(id, 1.0, pos, DEFAULT_DISTS);
                }
                // obj_embed into the (on-screen, unmoving) board.
                o.hidden |= OBJHFLAG_EMBEDDED;
                o.embedded = Some(Embed::Board(b));
            }
            None => {
                if o.ty == OBJTYPE_WEAPON {
                    // bgun_play_bg_hit_sound(&weapon->gset, pos, -1, rooms): the
                    // texture is -1, so no surface sound.
                    self.bgun_play_bg_hit_sound(0, o.weaponnum, o.gunfunc, pos, None);
                }
            }
        }
    }
}

/// `obj_get_ground_clearance` (`propobj.c:2190`).
pub(crate) fn obj_get_ground_clearance(o: &Obj) -> f32 {
    if o.ty == OBJTYPE_WEAPON {
        0.0
    } else {
        4.0
    }
}
