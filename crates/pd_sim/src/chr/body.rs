//! A chr's body as the sim sees it: the body and head models
//! (`body_instantiate_model_with_spawnflags`), the pose PD computes in
//! `chr_tick` (`model_set_matrices_with_anim` with `chr_handle_joint_positioned`)
//! and the held guns (`chr_tick_child`), the gun positions a simulant's shot
//! starts from (`chr_get_gun_pos`), whether the chr is on a player's screen
//! (`pos_is_onscreen`), and the player's shots against its part boxes
//! (`chr_test_hit`, the multiplayer "cheap" path).
//!
//! PD poses a chr in eye space for each player that sees it, and the joint
//! callback turns each matrix into world space and back around its twists.
//! Here the pose is in world space (`rendermtx` = identity), once per tick,
//! so those round trips drop out and the renderer multiplies by its own view.
//! The shots test the same world-space matrices with the world-space ray,
//! which is the same test (every box test is done in the box's own space).
//!
//! Source: the old repo's `pd_spike/model.rs` (`callback_rotation`) and
//! `gunpos.rs`, on PD's model files through `pd_core::model` instead of glTF.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use pd_core::ids::*;
use pd_core::math::{self, baddtor, baddtor4, M_BADTAU};
use pd_core::model::hit::{HitNode, HitPad};
use pd_core::model::{body_calculate_head_offset, Model, ModelDef, ModelStore, NodeKind, PoseParams, MODELPART_CHR_HEADSPOT, MODELPART_HEAD_HUDPIECE, MODELPART_HEAD_SUNGLASSES, SKEL_CHR, SKEL_HEAD};

use super::{Act, Chr};
use crate::player::camera::Camera;

/// A gun a chr holds (`weapons_held[hand]`, a `PROPTYPE_WEAPON` child prop):
/// its weapon and its model, posed on the hand.
#[derive(Clone)]
pub struct Held {
    pub weaponnum: u8,
    pub model: Model,
    /// `weapon_set_gunfire_visible`: the `CHRGUNFIRE` flash shows this tick.
    pub gunfire: bool,
}

impl Held {
    /// `chr_give_weapon` (`propobj.c:17852`): the weapon's held model
    /// (`lo_model`), its gunfire hidden (`weapon_create_for_chr` →
    /// `weapon_set_gunfire_visible(prop, false)`, `propobj.c:17356`).
    /// `extrascale` is 256 for every MP weapon (a scale of 1).
    pub fn new(store: &ModelStore, weaponnum: u8, stem: &str) -> Result<Held, String> {
        let mut h = Held { weaponnum, model: Model::new(store.get(stem)?), gunfire: false };
        h.weapon_set_gunfire_visible(false);
        Ok(h)
    }

    /// `weapon_set_gunfire_visible` (`propobj.c:17867`): the `CHRGUNFIRE`
    /// billboard (`gunfire`) and the `MODELPART_CHRGUN_0002` toggle's flash.
    /// True if a flash shows (the caller lights the room).
    pub fn weapon_set_gunfire_visible(&mut self, visible: bool) -> bool {
        if self.model.def.skel != pd_core::model::SKEL_CHRGUN {
            return false;
        }
        let mut flash = false;
        if self.model.def.get_part(pd_core::ids::MODELPART_CHRGUN_GUNFIRE).is_some() {
            self.gunfire = visible;
            flash |= visible;
        }
        if self.model.def.get_part(pd_core::ids::MODELPART_CHRGUN_0002).is_some() {
            self.model.set_toggle(false, pd_core::ids::MODELPART_CHRGUN_0002, visible);
            flash |= visible;
        }
        flash
    }
}

/// The body and head a chr is built from (`body_instantiate_model_to_addr`,
/// `body.c:168`): the body scaled by `g_HeadsAndBodies[].scale × 0.1`, the
/// head hung on its headspot, the animation scaled by `animscale`. A body that
/// `canvaryheight` with a head is scaled by 95–105% when `varyheight` draws
/// the `RANDOMFRAC` (PD's comment says 95–115%; the maths gives ±5%).
/// Returns the model, `animscale`, the body's height and sex.
pub fn chr_body(store: &ModelStore, bodies: &pd_core::model::Bodies, bodynum: usize, headnum: Option<usize>, varyheight: Option<&mut pd_core::rng::Rng>) -> Result<(Model, f32, f32, bool), String> {
    let row = bodies.rows.get(bodynum).ok_or_else(|| format!("no body {bodynum}"))?;
    let def: Arc<ModelDef> = store.get(row.stem.as_deref().ok_or_else(|| format!("body {bodynum} has no model"))?)?;
    let has_headspot = !row.unk00_01 && def.skel == SKEL_CHR && def.get_part(MODELPART_CHR_HEADSPOT).is_some();
    let headrow = headnum.filter(|&h| h > 0 && has_headspot).and_then(|h| bodies.rows.get(h));
    let head = match headrow.and_then(|h| h.stem.clone()) {
        // The MP path (`normmplayerisrunning && IS8MB()`): the head's own copy,
        // moved by body_calculate_head_offset.
        Some(stem) => {
            let h = store.get(&stem)?;
            let off = body_calculate_head_offset(headrow.unwrap().ty, row.ty);
            Some(if h.skel == SKEL_HEAD && off != 0.0 { Arc::new(h.with_head_offset(off)) } else { h })
        }
        None => None,
    };
    let mut scale = row.model_scale();
    if head.is_some() && row.canvaryheight {
        if let Some(rng) = varyheight {
            let frac = rng.randomfrac() * 0.05;
            scale *= 2.0 * frac - 0.05 + 1.0;
        }
    }
    let mut model = Model::with_head(def, head);
    model.scale = scale;
    if model.head.as_ref().is_some_and(|h| h.skel == SKEL_HEAD) {
        // No sunglasses (the spawn flags don't ask), and never the HUD piece.
        model.set_toggle(true, MODELPART_HEAD_SUNGLASSES, false);
        model.set_toggle(true, MODELPART_HEAD_HUDPIECE, false);
    }
    Ok((model, row.animscale, row.height as f32, row.ismale))
}

/// What `chr_handle_joint_positioned` (`chr.c:1602`) reads from the chr, copied
/// out so the model can be posed while the chr is borrowed.
#[derive(Clone, Copy, Debug, Default)]
pub struct JointFx {
    pub skel: i32,
    pub aimuplshoulder: f32,
    pub aimuprshoulder: f32,
    pub aimupback: f32,
    pub aimsideback: f32,
    /// `aibot->angleoffset`, or the player's `angleoffset`.
    pub angleoffset: f32,
    pub autoanim: bool,
    pub flip: bool,
    /// `flinchcnt >= 0`: the flinch amount (`chr_get_flinch_amount`), its type
    /// and whether it was a head shot.
    pub flinch: Option<(f32, u8, bool)>,
    /// `chr_get_aimx_angle`.
    pub aimangle: f32,
    /// Dizzy (`blurdrugamount > 1000`, alive): `drugheadsway`, in degrees.
    pub drugheadsway: Option<f32>,
}

impl JointFx {
    /// `chr_handle_joint_positioned` for a human skeleton (`g_SkelChr`: neck 0,
    /// waist 1, lshoulder 2, rshoulder 3), on a world-space matrix. No DK mode
    /// (a cheat).
    pub fn apply(&self, joint: usize, mtx: &mut Mat4) {
        if self.skel != SKEL_CHR {
            return;
        }
        let (neck, waist, lshoulder, rshoulder) = (0usize, 1usize, 2usize, 3usize);
        if joint != lshoulder && joint != rshoulder && joint != waist && joint != neck {
            return;
        }
        let (mut xrot, mut yrot, mut zrot) = (0.0f32, 0.0f32, 0.0f32);
        if joint == rshoulder {
            xrot = self.aimuprshoulder;
        } else if joint == lshoulder {
            xrot = self.aimuplshoulder;
        } else if joint == waist {
            xrot = self.aimupback;
            if self.autoanim {
                if xrot > baddtor(60.0) {
                    xrot -= baddtor(60.0);
                } else if xrot < baddtor(-50.0) {
                    xrot += baddtor(50.0);
                } else {
                    xrot = 0.0;
                }
            }
            yrot = self.aimsideback + self.angleoffset;
        } else if joint == neck {
            if self.autoanim {
                xrot = self.aimupback.clamp(baddtor(-50.0), baddtor(60.0));
            } else if self.flip {
                xrot = self.aimuplshoulder;
            } else {
                xrot = self.aimuprshoulder;
            }
            // A dizzy head lolls (chr.c:1756).
            if let Some(sway) = self.drugheadsway {
                zrot = baddtor4(sway);
                xrot -= (28.0 - sway.abs()) / 250.0 * baddtor(360.0);
            }
        }
        // The flinch (humans).
        if let Some((amount, flinchtype, headshotted)) = self.flinch {
            if headshotted {
                if joint == neck {
                    let mut degrees = 60.0;
                    if flinchtype & 1 == 0 {
                        degrees = 85.0;
                    }
                    let k = amount * (M_BADTAU * degrees / 360.0);
                    if (5..8).contains(&flinchtype) {
                        zrot -= k;
                    } else if flinchtype > 0 && flinchtype < 4 {
                        zrot += k;
                    }
                    if flinchtype == 7 || flinchtype == 0 || flinchtype == 1 {
                        xrot += k;
                    } else if (3..6).contains(&flinchtype) {
                        xrot -= k;
                    }
                }
            } else if joint == rshoulder || joint == lshoulder {
                let f = amount * baddtor(15.0);
                xrot -= f;
                if flinchtype < 3 {
                    yrot -= f;
                } else if flinchtype < 6 {
                    yrot += f;
                }
            } else if joint == waist {
                xrot += amount * baddtor(15.0);
                if flinchtype < 3 {
                    yrot += amount * baddtor(15.0);
                } else if flinchtype < 6 {
                    yrot -= amount * baddtor(15.0);
                }
                if matches!(flinchtype, 2 | 5 | 7) {
                    zrot += amount * baddtor(10.0);
                } else if matches!(flinchtype, 1 | 4 | 6) {
                    zrot -= amount * baddtor(10.0);
                }
            }
        }
        if xrot == 0.0 && yrot == 0.0 && zrot == 0.0 {
            return;
        }
        let aimangle = self.aimangle;
        xrot = if xrot < 0.0 { -xrot } else { baddtor(360.0) - xrot };
        if yrot < 0.0 {
            yrot += baddtor(360.0);
        }
        // mtx00015be0(cam_get_projection_mtxf(), mtx): already world space.
        let pos = mtx.w_axis.truncate();
        mtx.w_axis = glam::Vec4::W;
        if xrot != 0.0 || zrot != 0.0 {
            yrot -= aimangle;
            if yrot < 0.0 {
                yrot += baddtor(360.0);
            }
            *mtx = math::mul(&math::load_y_rotation(yrot), mtx);
            if xrot != 0.0 {
                *mtx = math::mul(&math::load_x_rotation(xrot), mtx);
            }
            if zrot != 0.0 {
                *mtx = math::mul(&math::load_z_rotation(zrot), mtx);
            }
            *mtx = math::mul(&math::load_y_rotation(aimangle), mtx);
        } else {
            *mtx = math::mul(&math::load_y_rotation(yrot), mtx);
        }
        mtx.w_axis = pos.extend(1.0);
    }
}

/// `chr_get_flinch_amount` (`chr.c:1566`).
pub fn chr_get_flinch_amount(flinchcnt: i32, headshotted: bool) -> f32 {
    let value = flinchcnt as f32;
    if headshotted {
        if value < 4.0 {
            (value * baddtor(90.0) / 4.0).sin()
        } else {
            1.0 - ((value - 4.0) * 0.060_405_626_893_044).sin()
        }
    } else if value < 10.0 {
        (value * baddtor(90.0) / 10.0).sin()
    } else {
        1.0 - ((value - 10.0) * 0.078_527_316_451_073).sin()
    }
}

/// `cam_is_pos_in_screen_box` (`camera.c:497`) with `pos_is_onscreen`'s rules
/// (`propobj.c:19021`): in front of the camera, inside the four edge planes of
/// the screen box by at least `-modelscale`, and within 320 m.
/// `// SUBST:` PD's box is the union of the draw slots of the chr's on-screen
/// rooms (portals, `bg_get_room_draw_slot`) / the whole view, every room being
/// on screen until M9's portals. No fog or fade distance on an MP arena.
pub fn pos_is_onscreen(cam: &Camera, pos: Vec3, modelscale: f32) -> bool {
    let m = cam.projection;
    let (c0, c1, c2, c3) = (m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate(), m.w_axis.truncate());
    // var8009dd6c: the camera's z axis at its position.
    if c2.dot(c3) + modelscale < c2.dot(pos) {
        return false;
    }
    let plane = |n: Vec3, pos: Vec3| n.dot(c3) + modelscale < n.dot(pos);
    let (xmin, xmax, ymin, ymax) = (cam.c_screenleft, cam.c_screenleft + cam.c_screenwidth, cam.c_screentop, cam.c_screentop + cam.c_screenheight);
    let mut sp38 = (xmin - cam.c_screenleft - cam.c_halfwidth) * cam.c_scalex;
    let sp3c = 1.0 / (sp38 * sp38 + 1.0).sqrt();
    sp38 *= sp3c;
    let sp24 = -sp3c;
    if plane(c0 * sp24 - c2 * sp38, pos) {
        return false;
    }
    let mut sp38 = -(xmax - cam.c_screenleft - cam.c_halfwidth) * cam.c_scalex;
    let sp30 = 1.0 / (sp38 * sp38 + 1.0).sqrt();
    sp38 *= sp30;
    let sp20 = -sp30;
    if plane(c0 * -sp20 - c2 * sp38, pos) {
        return false;
    }
    let mut sp34 = (cam.c_halfheight - (ymin - cam.c_screentop)) * cam.c_scaley;
    let sp2c = 1.0 / (sp34 * sp34 + 1.0).sqrt();
    sp34 *= sp2c;
    let sp1c = -sp2c;
    if plane(c1 * -sp1c + c2 * sp34, pos) {
        return false;
    }
    let mut sp34 = -(cam.c_halfheight - (ymax - cam.c_screentop)) * cam.c_scaley;
    let sp28 = 1.0 / (sp34 * sp34 + 1.0).sqrt();
    sp34 *= sp28;
    let sp18 = -sp28;
    if plane(c1 * sp18 + c2 * sp34, pos) {
        return false;
    }
    (pos - c3).length_squared() <= 32000.0 * 32000.0
}

/// `obj_find_hitthing_by_bboxrodata_mtx` (`propobj.c:13923`): where the ray
/// `pos + t·dir` enters part box `bbox` in the space of `mtx`, and the face's
/// normal, both in that box's space; `None` when it misses. From inside the box:
/// the start point, facing up.
pub fn bbox_hitthing(bbox: &[f32; 6], mtx: &Mat4, pos: Vec3, dir: Vec3, pad: f32) -> Option<(Vec3, Vec3)> {
    let inv = mtx000172f0(mtx);
    let spb8 = inv.transform_point3(pos);
    let spac = inv.transform_vector3(dir);
    let min = Vec3::new(bbox[0] - pad, bbox[2] - pad, bbox[4] - pad);
    let max = Vec3::new(bbox[1] + pad, bbox[3] + pad, bbox[5] + pad);
    let mut side = [2u8; 3];
    let mut sp88 = Vec3::ZERO;
    let mut reset = true;
    for i in 0..3 {
        if spb8[i] < min[i] {
            side[i] = 1;
            sp88[i] = min[i];
            reset = false;
        } else if spb8[i] > max[i] {
            side[i] = 0;
            sp88[i] = max[i];
            reset = false;
        }
    }
    if reset {
        return Some((spb8, Vec3::Y));
    }
    let mut sp7c = Vec3::splat(-1.0);
    for i in 0..3 {
        if side[i] != 2 && spac[i] != 0.0 {
            sp7c[i] = (sp88[i] - spb8[i]) / spac[i];
        }
    }
    let mut maxindex = 0;
    for i in 1..3 {
        if sp7c[i] > sp7c[maxindex] {
            maxindex = i;
        }
    }
    if sp7c[maxindex] < 0.0 {
        return None;
    }
    let mut hit = Vec3::ZERO;
    for i in 0..3 {
        if i != maxindex {
            hit[i] = spb8[i] + sp7c[maxindex] * spac[i];
            if hit[i] < min[i] || hit[i] > max[i] {
                return None;
            }
        } else {
            hit[i] = sp88[i];
        }
    }
    let mut normal = Vec3::ZERO;
    normal[maxindex] = if side[maxindex] == 0 { 1.0 } else { -1.0 };
    Some((hit, normal))
}

/// `mtx000172f0` (`mtx.c:588`): the inverse of an affine matrix by its 3×3
/// cofactors.
fn mtx000172f0(a: &Mat4) -> Mat4 {
    let m = |c: usize, r: usize| a.col(c)[r];
    let mut f0 = 0.0f32;
    f0 += m(0, 0) * m(1, 1) * m(2, 2);
    f0 += m(0, 1) * m(1, 2) * m(2, 0);
    f0 += m(0, 2) * m(1, 0) * m(2, 1);
    f0 -= m(0, 2) * m(1, 1) * m(2, 0);
    f0 -= m(0, 1) * m(1, 0) * m(2, 2);
    f0 -= m(0, 0) * m(1, 2) * m(2, 1);
    f0 = 1.0 / f0;
    let mut o = [[0.0f32; 4]; 4];
    o[0][0] = (m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1)) * f0;
    o[1][0] = (m(1, 2) * m(2, 0) - m(1, 0) * m(2, 2)) * f0;
    o[2][0] = (m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0)) * f0;
    o[0][1] = (m(0, 2) * m(2, 1) - m(0, 1) * m(2, 2)) * f0;
    o[1][1] = (m(0, 0) * m(2, 2) - m(0, 2) * m(2, 0)) * f0;
    o[2][1] = (m(0, 1) * m(2, 0) - m(0, 0) * m(2, 1)) * f0;
    o[0][2] = (m(0, 1) * m(1, 2) - m(0, 2) * m(1, 1)) * f0;
    o[1][2] = (m(0, 2) * m(1, 0) - m(0, 0) * m(1, 2)) * f0;
    o[2][2] = (m(0, 0) * m(1, 1) - m(0, 1) * m(1, 0)) * f0;
    o[3][0] = -(m(3, 0) * o[0][0] + m(3, 1) * o[1][0] + m(3, 2) * o[2][0]);
    o[3][1] = -(m(3, 0) * o[0][1] + m(3, 1) * o[1][1] + m(3, 2) * o[2][1]);
    o[3][2] = -(m(3, 0) * o[0][2] + m(3, 1) * o[1][2] + m(3, 2) * o[2][2]);
    o[3][3] = 1.0;
    Mat4::from_cols_array_2d(&o)
}

/// `obj_is_any_node_in_range` (`propobj.c:753`) for a chr's body and head in
/// the eye space `w2s` gives: over the part boxes that overlap the
/// `centre ± half` window in x and y, the largest and smallest z (`None` if
/// none do). `hand_inflict_melee_damage` asks it with a 73 × 55 cm window.
pub fn obj_is_any_node_in_range(model: &Model, w2s: &Mat4, centre: [f32; 2], half: [f32; 2]) -> Option<(f32, f32)> {
    let mut out: Option<(f32, f32)> = None;
    let mut test = |bbox: &[f32; 6], m: Mat4| {
        let m = *w2s * m;
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for k in 0..8 {
            let c = Vec3::new(bbox[k & 1], bbox[2 + ((k >> 1) & 1)], bbox[4 + ((k >> 2) & 1)]);
            let p = m.transform_point3(c);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        if centre[0] - half[0] <= hi.x && centre[0] + half[0] >= lo.x && centre[1] - half[1] <= hi.y && centre[1] + half[1] >= lo.y {
            out = Some(match out {
                None => (hi.z, lo.z),
                Some((mx, mn)) => (mx.max(hi.z), mn.min(lo.z)),
            });
        }
    };
    for (n, node) in model.def.nodes.iter().enumerate() {
        if let NodeKind::BBox { bbox, .. } = &node.kind {
            if let Some(m) = model.def.find_node_mtx_index(n, 0).map(|i| model.matrices[i]) {
                test(bbox, m);
            }
        }
    }
    if let (Some(head), Some(hm)) = (model.head.as_deref(), model.head_matrix()) {
        for (n, node) in head.nodes.iter().enumerate() {
            if let NodeKind::BBox { bbox, .. } = &node.kind {
                let m = head.find_node_mtx_index(n, 0).and_then(|i| model.matrices.get(i).copied()).unwrap_or(hm);
                test(bbox, m);
            }
        }
    }
    out
}

/// `pos_is_facing_pos` (`propobj.c:2603`): does the ray pass within `toradius`
/// of `topos`, ahead of `frompos`?
pub fn pos_is_facing_pos(frompos: Vec3, dir: Vec3, topos: Vec3, toradius: f32) -> bool {
    let relpos = topos - frompos;
    let value = dir.dot(relpos);
    if value > 0.0 {
        let a = dir.dot(dir);
        let b = relpos.dot(relpos);
        return (b - toradius * toradius) * a <= value * value;
    }
    false
}

/// A shot's round meeting a chr's part box (`hit_create` from `chr_test_hit`).
#[derive(Clone, Copy, Debug)]
pub struct ChrHit {
    pub hitpart: i32,
    pub node: HitNode,
    /// World space.
    pub pos: Vec3,
    pub normal: Vec3,
}

impl Chr {
    /// `model_get_effective_scale(chr->model)`.
    pub fn effective_scale(&self) -> f32 {
        self.model.def.scale * self.model.scale
    }

    /// `chr_get_hit_radius` (`chr.c:4471`): the model's radius plus the
    /// biggest held gun's, 10 cm more with a shield.
    pub fn chr_get_hit_radius(&self) -> f32 {
        let mut highest = 0.0f32;
        for h in self.held.iter().flatten() {
            highest = highest.max(h.model.def.scale * h.model.scale * self.model.scale);
        }
        let mut result = self.effective_scale() + highest;
        if self.cshield > 0.0 {
            result += 10.0;
        }
        result
    }

    /// What the joint callback reads now.
    pub fn joint_fx(&self) -> JointFx {
        JointFx {
            skel: self.model.def.skel,
            aimuplshoulder: self.aimuplshoulder,
            aimuprshoulder: self.aimuprshoulder,
            aimupback: self.aimupback,
            aimsideback: self.aimsideback,
            angleoffset: self.angleoffset(),
            autoanim: self.autoanim,
            flip: self.anim.flip,
            flinch: (self.flinchcnt >= 0).then(|| (chr_get_flinch_amount(self.flinchcnt, self.headshotted), self.flinchtype, self.headshotted)),
            aimangle: self.chr_get_aimx_angle(),
            drugheadsway: (self.blurdrugamount > 1000 && !matches!(self.actiontype, Act::Dead | Act::Die)).then_some(self.drugheadsway),
        }
    }

    /// `chr_tick`'s pose (`chr.c:2752`): `model_set_matrices_with_anim` in world
    /// space with the joint callback, then each held gun on its hand
    /// (`chr_tick_child`, `chr.c:1983`: the left hand's is turned 180° about z).
    /// Far from the camera (`campos`, 7 m, 20 m with slow motion) a fast
    /// animation is posed on its nearest whole frame, as PD does to save work.
    pub fn pose(&mut self, bank: &pd_core::anim::AnimBank, playercount: usize, campos: Vec3, lodscalez: f32, slowmo: bool) {
        let fx = self.joint_fx();
        let limit: f32 = if slowmo { 2000.0 * 2000.0 } else { 700.0 * 700.0 };
        let mut anim = self.anim.clone();
        if anim.animnum != 0 && self.pos.distance_squared(campos) * lodscalez * lodscalez > limit {
            if anim.frac != 0.0 && anim.speed * anim.playspeed >= 0.25 {
                if anim.frac > 0.5 {
                    anim.framea = anim.frameb;
                }
                anim.frac = 0.0;
            }
            if anim.fracmerge != 0.0 && anim.speed2 * anim.playspeed >= 0.25 && anim.frac2 != 0.0 {
                if anim.frac2 > 0.5 {
                    anim.frame2a = anim.frame2b;
                }
                anim.frac2 = 0.0;
            }
        }
        // SUBST: the distance relations choose the body's LOD by the eye's depth
        // / posed for no one, every distance is 0 (the near LOD): the renderer
        // picks its own for its view.
        let params = PoseParams { rendermtx: Mat4::IDENTITY, bank, playercount, lod_scale: None };
        let mut joint = |j: usize, m: &mut Mat4| fx.apply(j, m);
        self.model.set_matrices_with_anim(&params, Some(&anim), Some(&mut joint));
        for (hand, held) in self.held.iter_mut().enumerate() {
            let Some(held) = held else { continue };
            let part = if hand == HAND_RIGHT { MODELPART_CHR_RIGHTHAND } else { MODELPART_CHR_LEFTHAND };
            let Some(mtx0) = self.model.def.get_part(part).and_then(|n| self.model.def.find_node_mtx_index(n, 0)).map(|i| self.model.matrices[i]) else {
                continue;
            };
            let rendermtx = if hand == HAND_LEFT { math::mul(&mtx0, &math::load_z_rotation(baddtor(180.0))) } else { mtx0 };
            held.model.set_matrices_with_anim(&PoseParams { rendermtx, bank, playercount, lod_scale: None }, None, None);
        }
    }

    /// `chr_get_gun_pos` (`chraction.c:9640`): where the held gun's
    /// `CHRGUNFIRE` node (`MODELPART_0000`) is, or its `MODELPART_0001` node,
    /// if the chr and its gun were drawn this tick; `None` otherwise.
    pub fn chr_get_gun_pos(&self, hand: usize) -> Option<Vec3> {
        let held = self.held[hand].as_ref()?;
        if !self.onscreen {
            return None;
        }
        let def = &held.model.def;
        if let Some(node) = def.get_part(MODELPART_0000) {
            let m = held.model.matrices[def.find_node_mtx_index(node, 0)?];
            let pos = match def.nodes[node].kind {
                NodeKind::ChrGunfire { pos, .. } => pos,
                _ => Vec3::ZERO,
            };
            return Some(m.transform_point3(pos));
        }
        if let Some(node) = def.get_part(MODELPART_0001) {
            return Some(held.model.matrices[def.find_node_mtx_index(node, 0)?].w_axis.truncate());
        }
        None
    }

    /// `chr_test_hit` (`chr.c:4503`): if the ray passes within the hit radius
    /// of the model's root, the first part box along the tree it enters
    /// (`model_test_for_hit`). On the `cheap` path (every multiplayer shot,
    /// and a query with two or more players) where it enters that box
    /// (`obj_find_hitthing_by_bboxrodata_mtx`); otherwise (the crosshair's
    /// query with one player) where it meets the model's triangles
    /// (`projectile_0f06bea0`). `pos`/`dir` are the shot in world space. PD
    /// skips a chr not drawn this tick; the caller asks [`pos_is_onscreen`] first.
    ///
    /// `// SUBST:` `projectile_0f06bea0` walks the tree for the first box whose
    /// triangles the ray meets / the part is the first box the ray enters and
    /// the point the nearest of the body's triangles.
    pub fn chr_test_hit(&self, pos: Vec3, dir: Vec3, cheap: bool) -> Option<ChrHit> {
        let root = self.model.matrices.first()?.w_axis.truncate();
        if !pos_is_facing_pos(pos, dir, root, self.chr_get_hit_radius()) {
            return None;
        }
        if !cheap {
            let (hitpart, node) = self.model.test_for_hit(pos, dir, None, HitPad(0.0))?;
            if hitpart <= 0 {
                return None;
            }
            let (_, p, n) = self.model.hit_tris(0, pos, dir)?;
            return Some(ChrHit { hitpart, node, pos: p, normal: n.normalize_or_zero() });
        }
        let mut from = None;
        while let Some((hitpart, node)) = self.model.test_for_hit(pos, dir, from, HitPad(0.0)) {
            if hitpart <= 0 {
                break;
            }
            let (def, bbox) = match node {
                HitNode::Body(n) => (&*self.model.def, &self.model.def.nodes[n]),
                HitNode::Head(n) => (self.model.head.as_deref()?, &self.model.head.as_deref()?.nodes[n]),
            };
            let NodeKind::BBox { bbox, .. } = &bbox.kind else { break };
            let n = match node {
                HitNode::Body(n) | HitNode::Head(n) => n,
            };
            let mtx = match node {
                HitNode::Body(_) => def.find_node_mtx_index(n, 0).map(|m| self.model.matrices[m]),
                HitNode::Head(_) => def.find_node_mtx_index(n, 0).map(|m| self.model.matrices[m]).or_else(|| self.model.head_matrix()),
            };
            if let Some(m) = mtx {
                if let Some((p, nrm)) = bbox_hitthing(bbox, &m, pos, dir, 0.0) {
                    return Some(ChrHit { hitpart, node, pos: m.transform_point3(p), normal: m.transform_vector3(nrm).normalize_or_zero() });
                }
            }
            from = Some(node);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callback_rotation_is_pds_matrix_order() {
        // A waist twist with no pitch is Ry(y), whatever the aim angle, but for
        // PD's M_BADPI: a pitch of 0 becomes BADDTOR(360), 0.001 rad short of a
        // full turn, so the twist tips by that much.
        let fx = JointFx { skel: SKEL_CHR, angleoffset: 0.3, aimangle: 1.1, ..Default::default() };
        let mut m = Mat4::from_translation(Vec3::new(5.0, 6.0, 7.0));
        fx.apply(1, &mut m);
        let d = (m.transform_vector3(Vec3::Z) - Mat4::from_rotation_y(0.3).transform_vector3(Vec3::Z)).length();
        assert!(d < 2e-3 && d > 1e-4, "{d}");
        assert_eq!(m.w_axis.truncate(), Vec3::new(5.0, 6.0, 7.0), "rotated about the joint");
        // Aiming up tilts the right shoulder's forward vector up.
        let fx = JointFx { skel: SKEL_CHR, aimuprshoulder: 0.4, ..Default::default() };
        let mut m = Mat4::IDENTITY;
        fx.apply(3, &mut m);
        assert!(m.transform_vector3(Vec3::Z).y > 0.3);
        // ... along the aim direction, whichever way the chr faces.
        let fx = JointFx { skel: SKEL_CHR, aimuprshoulder: 0.4, aimangle: std::f32::consts::FRAC_PI_2, ..Default::default() };
        let mut m = Mat4::IDENTITY;
        fx.apply(3, &mut m);
        assert!(m.transform_vector3(Vec3::X).y > 0.3, "{}", m.transform_vector3(Vec3::X));
    }

    #[test]
    fn a_ray_enters_a_part_box_on_the_near_face() {
        let bbox = [-10.0, 10.0, 0.0, 50.0, -5.0, 5.0];
        let m = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
        let (p, n) = bbox_hitthing(&bbox, &m, Vec3::new(100.0, 20.0, -100.0), Vec3::Z, 0.0).unwrap();
        assert!((p - Vec3::new(0.0, 20.0, -5.0)).length() < 1e-4, "{p}");
        assert_eq!(n, Vec3::new(0.0, 0.0, -1.0));
        assert!(bbox_hitthing(&bbox, &m, Vec3::new(100.0, 80.0, -100.0), Vec3::Z, 0.0).is_none());
        assert!(pos_is_facing_pos(Vec3::ZERO, Vec3::Z, Vec3::new(10.0, 0.0, 100.0), 11.0));
        assert!(!pos_is_facing_pos(Vec3::ZERO, Vec3::Z, Vec3::new(10.0, 0.0, -100.0), 11.0));
    }

    #[test]
    fn the_flinch_rises_in_ten_ticks_and_falls_away() {
        assert!(chr_get_flinch_amount(0, false).abs() < 1e-6);
        assert!((chr_get_flinch_amount(10, false) - 1.0).abs() < 1e-3);
        assert!(chr_get_flinch_amount(29, false) < 0.1);
        assert!((chr_get_flinch_amount(4, true) - 1.0).abs() < 1e-3);
    }
}
