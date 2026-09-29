//! The objects the guns put into the world (`propobj.c`, `projectile.c`): thrown
//! and fired weapons in flight ([`projectile`]), the fuses, mines, rockets and
//! bolts they become ([`weapon`]), the Laptop sentry ([`autogun`]), the N-Bomb
//! storm ([`nbomb`]), the [`explosions`], and the Combat Simulator's pickups
//! ([`pickup`]: the pads' weapons, crates and shields, and what dead chrs drop).
//!
//! An object is PD's `defaultobj` with the `weaponobj` / `autogunobj` fields it
//! needs ([`Obj`]); in flight it carries a `struct projectile` ([`Projectile`],
//! PD's `OBJHFLAG_PROJECTILE`). Objects tick in `lv_render`'s `props_tick_player`,
//! once per frame each: a projectile its owner threw or fired in that player's
//! pass, anything else in the first player's (`obj_tick_player`, `propobj.c:11105`).
//!
//! Collision is PD's own on the stage: a sticky projectile's segment against the
//! BG's display-list triangles (`bg_test_hit_in_room`, [`crate::stage::BgHitMesh`])
//! then the props, a non-sticky one's cylinder against the wall tiles
//! (`cd_test_cylmove_*`, `cd_test_volume_fromdir`) and the chrs' perimeters,
//! and floors by `cd_find_y`. The props a sticky projectile meets are the
//! firing range's boards and the chrs (their part boxes when drawn).
//!
//! Sources: the old repo's `pd_guns/props.rs`, `throw.rs`, `autogun.rs` and
//! `nbomb.rs`, re-checked against the decomp: where the spike stood in for PD's
//! collision with the range's boxes, this runs PD's tests on the stage.

pub mod autogun;
pub mod door;
pub mod explosions;
pub mod glass;
pub mod hover;
pub mod nbomb;
pub mod pickup;
pub mod projectile;
pub mod setup;
pub mod lift;
#[cfg(test)]
mod tests;
pub mod weapon;

use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3};
use pd_core::ids::*;
use pd_core::math::{self, baddtor, Quatf};
use pd_core::model::ModelDef;
use pd_core::rng::Rng;

pub use autogun::Autogun;
pub use nbomb::Nbombs;

/// `g_MaxWeaponSlots` (`setup.c:53`).
pub const MAX_WEAPON_SLOTS: usize = 50;
/// `g_MaxProjectiles` (`setup.c:57`, the 8 MB value).
pub const MAX_PROJECTILES: usize = 100;
/// `g_MaxThrownLaptops` in a match: one per MP chr (`setup.c:171`).
pub const MAX_THROWN_LAPTOPS: usize = 12;

/// `struct modelrodata_bbox`: the model file's BBOX node.
#[derive(Clone, Copy, Debug)]
pub struct Bbox {
    pub xmin: f32,
    pub xmax: f32,
    pub ymin: f32,
    pub ymax: f32,
    pub zmin: f32,
    pub zmax: f32,
}

impl Bbox {
    pub fn from_def(def: &ModelDef) -> Bbox {
        let b = def.bbox.unwrap_or([-10.0, 10.0, -10.0, 10.0, -10.0, 10.0]);
        Bbox { xmin: b[0], xmax: b[1], ymin: b[2], ymax: b[3], zmin: b[4], zmax: b[5] }
    }

    /// `obj_get_rotated_local_min` (`propobj.c:406`).
    fn rotated_local_min(&self, a1: f32, a2: f32, a3: f32) -> f32 {
        let mut sum = 0.0;
        sum += if a1 >= 0.0 { self.xmin * a1 } else { self.xmax * a1 };
        sum += if a2 >= 0.0 { self.ymin * a2 } else { self.ymax * a2 };
        sum += if a3 >= 0.0 { self.zmin * a3 } else { self.zmax * a3 };
        sum
    }

    /// `obj_get_rotated_local_y_min_by_mtx3` (`propobj.c:384`): how far below
    /// its position the rotated box reaches.
    pub fn rotated_y_min(&self, r: &Mat3) -> f32 {
        self.rotated_local_min(r.x_axis.y, r.y_axis.y, r.z_axis.y)
    }

    /// `obj_get_rotated_local_max` (`propobj.c:429`).
    fn rotated_local_max(&self, a1: f32, a2: f32, a3: f32) -> f32 {
        let mut sum = 0.0;
        sum += if a1 <= 0.0 { self.xmin * a1 } else { self.xmax * a1 };
        sum += if a2 <= 0.0 { self.ymin * a2 } else { self.ymax * a2 };
        sum += if a3 <= 0.0 { self.zmin * a3 } else { self.zmax * a3 };
        sum
    }

    /// `obj_get_rotated_local_y_max_by_mtx3` (`propobj.c:389`).
    pub fn rotated_y_max(&self, r: &Mat3) -> f32 {
        self.rotated_local_max(r.x_axis.y, r.y_axis.y, r.z_axis.y)
    }
}

/// `struct projectile` (`types.h`), the fields the Combat Simulator's objects use.
#[derive(Clone, Debug)]
pub struct Projectile {
    pub flags: u32,
    pub speed: Vec3,
    pub accel: Vec3,
    /// The spin applied once per quarter-tick (`projectile->mtx`), a rotation.
    pub mtx: Mat3,
    /// `ownerprop`: the chr (a player's chr index is the player's) who threw,
    /// fired or dropped it, whose perimeter the flight ignores; a player's
    /// pass ticks a player's.
    pub ownerprop: Option<usize>,
    pub bouncecount: i32,
    pub bounceframe: i32,
    pub collisionframe: i32,
    pub losttimer240: i32,
    pub flighttime240: i32,
    pub powerlimit240: i32,
    pub pickuptimer240: i32,
    pub hitspeedpreservationfrac: f32,
    pub speeddecel: f32,
    pub missileyaccel: f32,
    pub missiley: f32,
    pub missileyspeed: f32,
    pub nextsteppos: Vec3,
    pub settledrotfrac: f32,
    pub settledrotinc: f32,
    pub unk068: Quatf,
    pub unk078: Quatf,
    pub unk0b8: [f32; 3],
    pub lastwooshframe: i32,
    pub startframe: i32,
    /// `targetprop`: what a homing rocket steers at (`trackedprops[0]`). M6.
    pub targetprop: Option<usize>,
}

impl Default for Projectile {
    /// `projectile_reset` (`propobj.c:1054`).
    fn default() -> Projectile {
        Projectile {
            flags: 0,
            speed: Vec3::ZERO,
            accel: Vec3::ZERO,
            mtx: Mat3::IDENTITY,
            ownerprop: None,
            bouncecount: 0,
            bounceframe: -1,
            collisionframe: -1,
            losttimer240: 0,
            flighttime240: 0,
            powerlimit240: -1,
            pickuptimer240: 0,
            hitspeedpreservationfrac: 0.05,
            speeddecel: 0.0,
            missileyaccel: 0.0,
            missiley: 0.0,
            missileyspeed: 0.0,
            nextsteppos: Vec3::ZERO,
            settledrotfrac: 1.0,
            settledrotinc: 0.0,
            unk068: [1.0, 0.0, 0.0, 0.0],
            unk078: [1.0, 0.0, 0.0, 0.0],
            unk0b8: [1.0; 3],
            lastwooshframe: -1,
            startframe: 0,
            targetprop: None,
        }
    }
}

/// What a stuck object is embedded in (`obj_embed`, `OBJHFLAG_EMBEDDED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Embed {
    /// A firing-range board, which never moves.
    Board(usize),
}

/// A `defaultobj` with its `weaponobj` or `autogunobj` fields.
#[derive(Clone)]
pub struct Obj {
    /// Stable identity (PD's prop pointer).
    pub id: u32,
    /// `OBJTYPE_WEAPON` or `OBJTYPE_AUTOGUN`.
    pub ty: u8,
    pub weaponnum: u8,
    pub gunfunc: usize,
    pub timer240: i32,
    pub def: Arc<ModelDef>,
    pub bbox: Bbox,
    /// `model->scale`.
    pub scale: f32,
    /// `prop->pos`.
    pub pos: Vec3,
    /// `obj->realrot`, the model scale folded in as `obj_place` does.
    pub realrot: Mat3,
    /// `obj->projectile` while `OBJHFLAG_PROJECTILE`.
    pub projectile: Option<Projectile>,
    /// `obj->hidden`: `OBJHFLAG_*`, and the owning MP chr in the top four bits.
    pub hidden: u32,
    pub flags: u32,
    pub flags2: u32,
    pub flags3: u32,
    /// `prop_set_dangerous`: simulants flee it (M6).
    pub dangerous: bool,
    pub embedded: Option<Embed>,
    /// The sentry's state, for `OBJTYPE_AUTOGUN`.
    pub autogun: Option<Autogun>,
    /// `PROPFLAG_NOTYETTICKED`: not ticked yet this frame.
    pub notyetticked: bool,
    /// `obj->pad`: the pad a setup object stands on (-1 none).
    pub pad: i32,
    /// `obj->hidden2`: `OBJH2FLAG_*` (`CANREGEN` for the setup's pickups).
    pub hidden2: u32,
    /// `prop->timetoregen`: while positive the pickup is gone, the last 60
    /// ticks fading back in (`obj_tick`).
    pub timetoregen: i32,
    /// A multi ammo crate's `slots[ammotype - 1].quantity`.
    pub ammoslots: [i32; 19],
    /// A shield's `amount` and `initialamount` (fractions of a full shield).
    pub shieldamount: f32,
    pub shieldinitialamount: f32,
    /// The model's toggle and LOD visibility (`Model::vis`); empty draws every
    /// toggle and the nearest LOD.
    pub vis: Vec<bool>,
    /// `obj->modelnum` (`MODEL_*`), -1 for the guns' own objects.
    pub modelnum: i32,
    /// `obj->damage` / `obj->maxdamage` (`maxdamage` from the setup, in
    /// PD's quarter units: a setup's 1000 is 250 of health).
    pub damage: f32,
    pub maxdamage: f32,
    /// `obj->geo`: what walls the object puts in the chrs' way (its core
    /// block, `OBJFLAG_CORE_GEO_INUSE`, and its wall quads), and the floors
    /// it offers (its floor quads), in world space (`obj_update_all_geo`).
    pub geos: Vec<crate::stage::PropGeo>,
    pub floors: Vec<crate::stage::PropFloor>,
    /// `glass->portalnum` / `tintedglass->portalnum`: the portal the glass fills.
    pub portalnum: Option<usize>,
    /// A door's own state (`OBJTYPE_DOOR`).
    pub door: Option<Box<door::Door>>,
    /// A lift's own state (`OBJTYPE_LIFT`).
    pub lift: Option<Box<lift::Lift>>,
    /// A tinted pane's (`OBJTYPE_TINTEDGLASS`).
    pub tinted: Option<glass::TintedGlass>,
    /// A hover prop's float (`OBJTYPE_HOVERPROP`).
    pub hov: Option<hover::Hov>,
    /// `weaponobj->team`: a Capture the Case briefcase's team.
    pub team: u8,
    /// `prop->rooms[0]` for a setup object (its floors' room). `// SUBST:`
    /// PD keeps the rooms the object's box enters, followed as it moves /
    /// the room its position is in when placed.
    pub room: Option<u16>,
}

impl Obj {
    /// A weapon object as `weapon_create_projectile_from_gset` makes it
    /// (`propobj.c:17632`): `OBJFLAG_FALL`, `timer240` -1, the owner in `hidden`.
    pub fn weapon(id: u32, def: Arc<ModelDef>, modelscale: f32, weaponnum: u8, gunfunc: usize, owner: usize) -> Obj {
        let bbox = Bbox::from_def(&def);
        let mut o = Obj {
            id,
            ty: OBJTYPE_WEAPON,
            weaponnum,
            gunfunc,
            timer240: -1,
            def,
            bbox,
            scale: modelscale,
            pos: Vec3::ZERO,
            realrot: Mat3::IDENTITY,
            projectile: None,
            hidden: ((owner as u32) << 28) & 0xf000_0000,
            flags: OBJFLAG_FALL,
            flags2: 0,
            flags3: 0,
            dangerous: false,
            embedded: None,
            autogun: None,
            notyetticked: false,
            pad: -1,
            hidden2: 0,
            timetoregen: 0,
            ammoslots: [0; 19],
            shieldamount: 0.0,
            shieldinitialamount: 0.0,
            vis: Vec::new(),
            modelnum: -1,
            damage: 0.0,
            maxdamage: 0.0,
            geos: Vec::new(),
            floors: Vec::new(),
            portalnum: None,
            door: None,
            lift: None,
            tinted: None,
            hov: None,
            team: 0,
            room: None,
        };
        // weapon_init (`propobj.c:17353`): the gunfire hidden.
        o.weapon_set_gunfire_visible(false);
        o
    }

    /// `weapon_set_gunfire_visible` (`propobj.c:17867`) for the object's own
    /// model: a chr gun's `CHRGUNFIRE` flash (drawn only while `gunfire`) and its
    /// `MODELPART_CHRGUN_0002` toggle. Every weapon object starts with both off
    /// (`weapon_init`, `:17353`). True if the model has either.
    pub fn weapon_set_gunfire_visible(&mut self, visible: bool) -> bool {
        if self.def.skel != pd_core::model::SKEL_CHRGUN {
            return false;
        }
        let mut flash = self.def.get_part(MODELPART_CHRGUN_GUNFIRE).is_some();
        if let Some(n) = self.def.get_part(MODELPART_CHRGUN_0002) {
            if self.vis.is_empty() {
                self.vis = pd_core::model::near_lod_vis(&self.def);
            }
            self.vis[n] = visible;
            flash = true;
        }
        flash && visible
    }

    /// `(obj->hidden & 0xf0000000) >> 28`: the player (MP chr) who owns it.
    pub fn owner(&self) -> usize {
        ((self.hidden & 0xf000_0000) >> 28) as usize
    }

    pub fn has(&self, hflag: u32) -> bool {
        self.hidden & hflag != 0
    }

    pub fn is_deleting(&self) -> bool {
        self.has(OBJHFLAG_DELETING)
    }

    /// The model's root matrix in world space (`realrot` + `pos`).
    pub fn root_matrix(&self) -> Mat4 {
        // A door's is `door_get_mtx` (`door_init_matrices`, `propobj.c:7841`).
        if let Some(d) = &self.door {
            return door::door_get_mtx(self, d);
        }
        let mut m = Mat4::from_mat3(self.realrot);
        math::set_translation(&mut m, self.pos);
        m
    }

    /// `obj_init_matrices` (`propobj.c:10898`) in world space: matrix 0 is the
    /// object's transform; a weapon's others are identity (`weapon_init_matrices`,
    /// `:10851`), the sentry's are its turret (`autogun_init_matrices`).
    pub fn init_matrices(&self) -> Vec<Mat4> {
        let mut out = vec![Mat4::IDENTITY; self.def.nummatrices.max(1)];
        out[0] = self.root_matrix();
        if let Some(a) = &self.autogun {
            a.init_matrices(&self.def, self.scale, &mut out);
        }
        out
    }

    /// `obj_test_hit` + `obj_attachment_test_hit` (`propobj.c:14747`, `:14642`):
    /// a shot's ray in eye space (`pos`, `dir`) against the object posed through
    /// `w2e`: the quick depth check against `distance`, then each part box the
    /// ray passes (`model_test_for_hit`) until one's triangles are hit. Returns
    /// the hit's eye depth, point and normal (eye space).
    pub fn test_hit(&self, w2e: &Mat4, lodscale: f32, pos: Vec3, dir: Vec3, distance: f32) -> Option<(f32, Vec3, Vec3)> {
        let mut m = pd_core::model::Model::new(self.def.clone());
        m.scale = self.scale;
        m.matrices = self.init_matrices().iter().map(|x| *w2e * *x).collect();
        m.update_distance_relations(lodscale);
        // obj_get_rotated_local_z_max_by_mtx4: the box's nearest reach towards the eye.
        let r = m.matrices[0];
        let b = self.bbox;
        let zmax = |a: f32, lo: f32, hi: f32| if a >= 0.0 { hi * a } else { lo * a };
        let reach = zmax(r.x_axis.z, b.xmin, b.xmax) + zmax(r.y_axis.z, b.ymin, b.ymax) + zmax(r.z_axis.z, b.zmin, b.zmax);
        if -(r.w_axis.z + reach) > distance {
            return None;
        }
        let mut from = None;
        loop {
            let (_hitpart, node) = m.test_for_hit(pos, dir, from, pd_core::model::hit::HitPad(0.0))?;
            if let pd_core::model::hit::HitNode::Body(n) = node {
                if let Some((_, p, nrm)) = m.hit_tris(n, pos, dir) {
                    let depth = -p.z;
                    return (depth <= distance).then_some((depth, p, nrm));
                }
            }
            from = Some(node);
        }
    }

    /// `gset_has_function_flags(&weapon->gset, flag)` (the grenade round and
    /// the bolt borrow their guns' functions, [`crate::gun::Gset::func`]).
    pub fn gset_flags(&self, gset: &crate::gun::Gset) -> u32 {
        gset.func(self.weaponnum, self.gunfunc).map_or(0, |f| f.flags)
    }
}

/// Every object in the world, in prop-list order, and the prop code's globals.
#[derive(Clone, Default)]
pub struct Props {
    pub objs: Vec<Obj>,
    next_id: u32,
    /// `g_PlayersDetonatingMines`: a bit per player who pressed a detonator.
    pub detonating: u32,
    /// `g_ThrownLaptops[]`: each player's deployed sentry, by object id.
    pub thrown_laptops: [Option<u32>; MAX_THROWN_LAPTOPS],
    /// The storms (`g_Nbombs`).
    pub nbombs: Nbombs,
    /// `var80069bc4`: the homing rockets' controller memory, one static.
    pub homing_prevangle: f32,
    /// `g_Lifts`: the lifts by lift number − 1 (`lift_activate`), by object id.
    pub lifts: [Option<u32>; 10],
    /// `g_LiftDoors`: the doors that call a lift (`OBJTYPE_LINKLIFTDOOR`).
    pub liftdoors: Vec<lift::LiftDoor>,
}

impl Props {
    pub fn alloc_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn get(&self, id: u32) -> Option<&Obj> {
        self.objs.iter().find(|o| o.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Obj> {
        self.objs.iter_mut().find(|o| o.id == id)
    }

    /// `weapon_create`'s slot reuse (`propobj.c:16946`) before a new weapon
    /// object. `// SUBST:` PD takes the next free slot of 50 in a ring, else an
    /// off-screen reusable one / with every slot taken the oldest reusable one
    /// (not in flight, not a held rocket) goes; objects are never off-screen here.
    pub fn make_room_for_weapon(&mut self) {
        let weapons = self.objs.iter().filter(|o| o.ty == OBJTYPE_WEAPON).count();
        if weapons >= MAX_WEAPON_SLOTS {
            if let Some(i) = self.objs.iter().position(|o| o.ty == OBJTYPE_WEAPON && o.projectile.is_none() && o.flags & OBJFLAG_HELDROCKET == 0) {
                self.objs.remove(i);
            }
        }
    }

    /// `projectile_allocate` (`propobj.c:1095`): with every slot in use, the
    /// oldest projectile's object is deleted.
    pub fn make_room_for_projectile(&mut self) {
        let flying = self.objs.iter().filter(|o| o.projectile.is_some()).count();
        if flying >= MAX_PROJECTILES {
            if let Some(o) = self.objs.iter_mut().filter(|o| o.projectile.is_some() && o.projectile.as_ref().unwrap().startframe > 0).min_by_key(|o| o.projectile.as_ref().unwrap().startframe) {
                o.projectile = None;
                o.hidden |= OBJHFLAG_DELETING;
            }
        }
    }
}

/// `projectile_load_random_rotation` (`projectile.c:14`): up to ±1.4° a
/// quarter-tick about each axis.
pub fn projectile_load_random_rotation(rng: &mut Rng) -> Mat3 {
    let mut r = || rng.randomfrac() * baddtor(360.0) * (1.0 / 128.0) - baddtor(180.0) / 128.0;
    let rot = Vec3::new(r(), r(), r());
    Mat3::from_mat4(math::load_rotation(rot))
}

/// `func0f06e9cc` (`propobj.c:3943`): the orientation that stands an object's
/// local +y on the surface normal `n`.
pub fn func0f06e9cc(n: Vec3) -> Mat4 {
    let f0 = n.length();
    let (x, y, z) = (n.x / f0, n.y / f0, n.z / f0);
    let (sp124, sp120, sp11c, sp118, sp114);
    if x == 0.0 && z == 0.0 {
        sp124 = 0.0;
        sp120 = 0.0;
        sp11c = y;
        sp118 = 1.0;
        sp114 = 0.0;
    } else {
        let a = (x * x + z * z).sqrt();
        let b = x / a;
        sp118 = z / a;
        sp114 = -b;
        sp124 = y * b;
        sp120 = -a;
        sp11c = y * sp118;
    }
    let spf4 = math::atan2f(sp118, sp114);
    let spb0 = math::load_y_rotation(-spf4);
    let sp24 = math::rotate(&spb0, Vec3::new(sp124, sp120, sp11c));
    let spf0 = math::atan2f(sp24.x, sp24.y);
    let sp70 = math::load_y_rotation(baddtor(-90.0) + spf4);
    let sp30 = math::load_x_rotation(baddtor(-90.0) - spf0);
    math::mul(&sp70, &sp30)
}

/// `mtx3_to_mtx4`.
fn m4(r: &Mat3) -> Mat4 {
    Mat4::from_mat3(*r)
}

impl Obj {
    /// `projectile_settle` (`propobj.c:3658`): pick the face the object comes to
    /// rest on and start slerping to it. `arg1` is `realrot` before this tick's
    /// spin; `lvupdate60freal` divides the last tick's turn.
    pub fn projectile_settle(&mut self, arg1: &Mat3, rng: &mut Rng, lvupdate60freal: f32) {
        self.hidden &= !OBJHFLAG_IMMUNETOBOUNCES;
        let bbox = self.bbox;
        let realrot = self.realrot;
        let scale = self.scale;
        let flags3 = self.flags3;
        let Some(p) = self.projectile.as_mut() else { return };
        p.ownerprop = None;
        p.flags &= !PROJECTILEFLAG_AIRBORNE;
        p.flags |= PROJECTILEFLAG_SETTLING;
        p.flags &= !PROJECTILEFLAG_STICKY;

        let sp148 = m4(&realrot);
        let sp188 = math::mtx4_get_rotation(&sp148);
        let sp108 = math::load_rotation(sp188);
        p.unk068 = math::quaternion0f096ca0(sp188);
        let spc8 = math::load_rotation_from(&sp108);
        let sp88 = spc8 * sp148;
        p.unk0b8 = [sp88.x_axis.truncate().length(), sp88.y_axis.truncate().length(), sp88.z_axis.truncate().length()];

        let next = |i: usize| (i + 1) % 3;
        let prev = |i: usize| (i + 2) % 3;
        let col = |m: &Mat3, i: usize| m.col(i);
        let localsizes = [bbox.xmax - bbox.xmin, bbox.ymax - bbox.ymin, bbox.zmax - bbox.zmin];
        let mut unksizes = [0.0f32; 3];
        let mut rotatedsizes = [0.0f32; 3];
        for i in 0..3 {
            unksizes[i] = localsizes[i] * p.unk0b8[i];
            rotatedsizes[i] = (col(&realrot, i).y * localsizes[i]).abs();
        }
        let (mut lside, mut sside, mut mside): (i32, i32, i32) = (-1, -1, -1);
        if flags3 & (OBJFLAG3_SETTLEROT_BYACTUALSIZE | OBJFLAG3_SETTLEROT_UPRIGHT | OBJFLAG3_SETTLEROT_LAPTOP) != 0 {
            if flags3 & OBJFLAG3_SETTLEROT_BYACTUALSIZE != 0 {
                for i in 0..3 {
                    if unksizes[i] < unksizes[next(i)] && unksizes[i] < unksizes[prev(i)] {
                        sside = i as i32;
                        break;
                    }
                }
            } else {
                sside = 1;
            }
            // PD indexes with sside even when BYACTUALSIZE found none (-1):
            // NEXT(-1) = 0, PREV(-1) = 1 in C's `%`.
            let (n, pv) = if sside >= 0 { (next(sside as usize), prev(sside as usize)) } else { (0, 1) };
            if rotatedsizes[n] >= rotatedsizes[pv] {
                lside = n as i32;
                mside = pv as i32;
            } else {
                lside = pv as i32;
                mside = n as i32;
            }
        }
        if lside < 0 {
            // One side three times the others (a gun): lie along it.
            for i in 0..3 {
                if unksizes[i] > unksizes[next(i)] * 3.0 && unksizes[i] > unksizes[prev(i)] * 3.0 {
                    lside = i as i32;
                    if unksizes[next(i)] > unksizes[prev(i)] * 2.0 {
                        sside = prev(i) as i32;
                        mside = next(i) as i32;
                    } else if unksizes[prev(i)] > unksizes[next(i)] * 2.0 {
                        sside = next(i) as i32;
                        mside = prev(i) as i32;
                    } else if rng.random().is_multiple_of(2) {
                        sside = prev(i) as i32;
                        mside = next(i) as i32;
                    } else {
                        sside = next(i) as i32;
                        mside = prev(i) as i32;
                    }
                    break;
                }
            }
        }
        if lside < 0 {
            // Squarish: any side three times another.
            for i in 0..3 {
                if unksizes[i] > unksizes[next(i)] * 3.0 || unksizes[i] > unksizes[prev(i)] * 3.0 {
                    let s = if unksizes[i] > unksizes[next(i)] * 3.0 { next(i) } else { prev(i) };
                    sside = s as i32;
                    if rotatedsizes[next(s)] >= rotatedsizes[prev(s)] {
                        lside = next(s) as i32;
                        mside = prev(s) as i32;
                    } else {
                        lside = prev(s) as i32;
                        mside = next(s) as i32;
                    }
                    break;
                }
            }
        }
        if lside < 0 {
            // Cubish. PD's @bug (>= where <= was meant) is why grenades land upright.
            for i in 0..3 {
                if rotatedsizes[i] >= rotatedsizes[next(i)] && rotatedsizes[i] >= rotatedsizes[prev(i)] {
                    sside = i as i32;
                    if rotatedsizes[next(i)] >= rotatedsizes[prev(i)] {
                        mside = prev(i) as i32;
                        lside = next(i) as i32;
                    } else {
                        lside = prev(i) as i32;
                        mside = next(i) as i32;
                    }
                    break;
                }
            }
        }
        if lside < 0 || sside < 0 {
            lside = 0;
            sside = 1;
            mside = 2;
        }
        let (l, s, m) = (lside as usize, sside as usize, mside as usize);
        let (mut xrot, mut zrot) = (col(&realrot, l).x, col(&realrot, l).z);
        if xrot != 0.0 || zrot != 0.0 {
            let f0 = (xrot * xrot + zrot * zrot).sqrt();
            if f0 > 0.0 {
                xrot /= f0;
                zrot /= f0;
            } else {
                xrot = 0.0;
                zrot = 1.0;
            }
        } else {
            xrot = 0.0;
            zrot = 1.0;
        }
        let laptop = flags3 & OBJFLAG3_SETTLEROT_LAPTOP != 0;
        let sy = col(&realrot, s).y;
        let mut cols = [Vec3::ZERO; 3];
        cols[l] = Vec3::new(xrot, 0.0, zrot);
        cols[m] = if ((sy >= 0.0 || laptop) && m == next(s)) || (sy <= 0.0 && !laptop && m == prev(s)) { Vec3::new(-zrot, 0.0, xrot) } else { Vec3::new(zrot, 0.0, -xrot) };
        cols[s] = if sy >= 0.0 || laptop { Vec3::Y } else { -Vec3::Y };
        let spc8 = Mat4::from_mat3(Mat3::from_cols(cols[0], cols[1], cols[2]));
        let sp188 = math::mtx4_get_rotation(&spc8);
        p.unk078 = math::quaternion0f096ca0(sp188);
        math::quaternion0f0976c0(p.unk068, &mut p.unk078);
        p.settledrotfrac = 0.0;
        let sp6c = cols[l].dot(sp108.col(l).truncate()).clamp(-1.0, 1.0).acos();
        let (ry, ay) = (col(&realrot, l).y, col(arg1, l).y);
        p.settledrotinc = if (sp6c > 0.0 && ry > 0.0 && ry > ay) || (sp6c > 0.0 && ry < 0.0 && ry < ay) {
            0.05 / (sp6c * 0.636_721_13)
        } else {
            let f2 = (col(arg1, l).dot(col(&realrot, l)) / (scale * scale)).clamp(-1.0, 1.0).acos() / lvupdate60freal;
            if sp6c != 0.0 {
                f2 / sp6c
            } else {
                1.0
            }
        };
        p.settledrotinc = p.settledrotinc.abs().clamp(0.03, 0.15);
    }

    /// `obj_stick_default` (`propobj.c:4006`): stand local +y on the normal,
    /// the box's bottom on the surface.
    pub fn obj_stick_default(&mut self, pos: Vec3, rot: Vec3) {
        let mut sp40 = func0f06e9cc(rot);
        math::scale3(&mut sp40, self.scale);
        let ymin = self.bbox.ymin;
        self.pos = pos - sp40.y_axis.truncate() * ymin;
        self.realrot = Mat3::from_mat4(sp40);
    }

    /// `obj_stick_bolt` (`propobj.c:4026`): keep the flight orientation, sink
    /// the tip in, and start the quiver (`timer240` 13).
    pub fn obj_stick_bolt(&mut self, rng: &mut Rng, pos: Vec3) {
        self.timer240 = 13;
        let zmax = self.bbox.zmax - (25.0 + 2.0 * rng.randomfrac());
        self.pos = pos - self.realrot.z_axis * zmax;
    }

    /// `obj_stick_knife` (`propobj.c:4061`): the handle along the normal, jiggled.
    pub fn obj_stick_knife(&mut self, rng: &mut Rng, pos: Vec3, rot: Vec3) {
        // Copied from obj_stick_bolt, then unused (zmax = 0); the RANDOMFRAC stays.
        let _ = self.bbox.zmin - (25.0 + 2.0 * rng.randomfrac());
        let x = rng.randomfrac() * 0.8 + rot.x - 0.4;
        let y = rng.randomfrac() * 0.8 + rot.y - 0.4;
        let z = rng.randomfrac() * 0.8 + rot.z - 0.4;
        let sp90 = func0f06e9cc(Vec3::new(x, y, z));
        let sp50 = math::load_x_rotation(baddtor(-90.0));
        let mut spd0 = math::mul(&sp90, &sp50);
        math::scale3(&mut spd0, self.scale);
        self.pos = pos;
        self.realrot = Mat3::from_mat4(spd0);
    }
}
