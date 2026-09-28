//! TEST ONLY: frozen copies of the two spike walkers the merged walker replaces,
//! the M1 migration oracle. `gun_walk` is the old repo's `pd_guns/model.rs`
//! `Model::set_matrices_with_anim`; `menu_walk` is `pd_menu/pdmodel.rs`
//! `Inst::set_matrices_with_anim`. Only their types are adapted (to this crate's
//! `ModelDef`/`NodeKind`); their arithmetic, glam quaternions included, is as
//! they were. `model::tests` compares [`super::Model::set_matrices_with_anim`]
//! against them. Delete once M2's menu goldens and M4's gun snapshots pass on the
//! merged walker.

use glam::{Mat4, Quat, Vec3};

use super::{ModelDef, NodeKind, MODELNODETYPE_0100, MODELNODETYPE_0200};
use crate::anim::{Anim, AnimBank, ChrInfo, ANIMFLAG_ABSOLUTETRANSLATION};
use crate::math as pdmtx;

fn euler_quat(rot: Vec3) -> Quat {
    Quat::from_euler(glam::EulerRot::ZYX, rot.z, rot.y, rot.x)
}

fn walk_reaches(def: &ModelDef, vis: &[bool], node: usize) -> bool {
    let mut cur = def.nodes[node].parent;
    while let Some(i) = cur {
        if matches!(def.nodes[i].kind, NodeKind::Toggle | NodeKind::Distance { .. }) && !vis[i] {
            return false;
        }
        cur = def.nodes[i].parent;
    }
    true
}

/// The gun spike's `find_node_mtx_index` (no helper slots, no -1 check).
fn gun_find(def: &ModelDef, node: usize) -> Option<usize> {
    let mut cur = Some(node);
    while let Some(i) = cur {
        match &def.nodes[i].kind {
            NodeKind::Position { mtx, .. } => return Some(mtx[0] as usize),
            NodeKind::PositionHeld { mtx, .. } => return Some(*mtx as usize),
            NodeKind::ChrInfo { mtx, .. } => return Some(*mtx as usize),
            _ => {}
        }
        cur = def.nodes[i].parent;
    }
    None
}

/// `pd_guns::model::Model::set_matrices_with_anim`, no joint callback.
pub fn gun_walk(def: &ModelDef, vis: &[bool], matrices: &mut [Mat4], scale: f32, chrinfo: &ChrInfo, rendermtx: &Mat4, anim: Option<&Anim>, bank: &AnimBank) {
    let parent_mtx = |m: &[Mat4], node: usize| match def.nodes[node].parent {
        Some(p) => gun_find(def, p).map(|i| m[i]),
        None => Some(*rendermtx),
    };
    for i in 0..def.nodes.len() {
        if !walk_reaches(def, vis, i) {
            continue;
        }
        match def.nodes[i].kind.clone() {
            NodeKind::Position { pos: rodata_pos, animpart, mtx, .. } => {
                let animpart = animpart as usize;
                let is_root = i == 0;
                let parent = parent_mtx(matrices, i);
                let Some(anim) = anim else {
                    let local = Mat4::from_translation(rodata_pos);
                    matrices[mtx[0] as usize] = match parent {
                        Some(p) => pdmtx::mul(&p, &local),
                        None => local,
                    };
                    continue;
                };
                let mut rot1 = Vec3::ZERO;
                let mut translate1 = Vec3::ZERO;
                let mut scale1 = Vec3::ONE;
                let mut sp128 = false;
                if anim.animnum != 0 {
                    if let Some(ad) = bank.get(anim.animnum) {
                        sp128 = ad.flags & ANIMFLAG_ABSOLUTETRANSLATION != 0 && is_root;
                        let (r, t, s) = ad.rot_translate_scale(animpart, anim.framea);
                        rot1 = r;
                        translate1 = t;
                        scale1 = s;
                        if anim.frac != 0.0 {
                            let (r2, t2, _) = ad.rot_translate_scale(animpart, anim.frameb);
                            rot1 = pdmtx::tween_rot(rot1, r2, anim.frac);
                            if sp128 {
                                translate1 += (t2 - translate1) * anim.frac;
                            }
                        }
                    }
                }
                let pos_of = |t: Vec3| {
                    if t != Vec3::ZERO {
                        let mut t = t * anim.animscale;
                        if !is_root {
                            t += rodata_pos;
                        }
                        t
                    } else if !is_root {
                        rodata_pos
                    } else {
                        t
                    }
                };
                let local = if anim.fracmerge != 0.0 {
                    let mut rot3 = Vec3::ZERO;
                    if let Some(ad2) = bank.get(anim.animnum2) {
                        rot3 = ad2.rot_translate_scale(animpart, anim.frame2a).0;
                        if anim.frac2 != 0.0 {
                            let r4 = ad2.rot_translate_scale(animpart, anim.frame2b).0;
                            rot3 = pdmtx::tween_rot(rot3, r4, anim.frac2);
                        }
                    }
                    let q1 = euler_quat(rot1);
                    let mut q3 = euler_quat(rot3);
                    if q1.dot(q3) < 0.0 {
                        q3 = -q3;
                    }
                    Mat4::from_rotation_translation(q1.slerp(q3, anim.fracmerge), pos_of(translate1))
                } else {
                    let pos = if sp128 { translate1 } else { pos_of(translate1) };
                    let mut l = pdmtx::load_rotation_translation(pos, rot1);
                    if sp128 && scale != 1.0 {
                        pdmtx::scale3(&mut l, scale);
                    }
                    l
                };
                let mut local = local;
                if scale1.x != 1.0 {
                    pdmtx::scale_col0(&mut local, scale1.x);
                }
                if scale1.y != 1.0 {
                    pdmtx::scale_col1(&mut local, scale1.y);
                }
                if scale1.z != 1.0 {
                    pdmtx::scale_col2(&mut local, scale1.z);
                }
                matrices[mtx[0] as usize] = match parent {
                    Some(p) => pdmtx::mul(&p, &local),
                    None => local,
                };
            }
            NodeKind::PositionHeld { pos, mtx } => {
                let local = Mat4::from_translation(pos);
                let m = match parent_mtx(matrices, i) {
                    Some(p) => pdmtx::mul(&p, &local),
                    None => local,
                };
                if let Some(slot) = matrices.get_mut(mtx as usize) {
                    *slot = m;
                }
            }
            NodeKind::ChrInfo { animpart, mtx } => {
                let Some(anim) = anim else { continue };
                let animpart = animpart as usize;
                let parent = parent_mtx(matrices, i);
                let mut rot1 = Vec3::ZERO;
                if let Some(ad) = bank.get(anim.animnum) {
                    rot1 = ad.rot_translate_scale(animpart, anim.framea).0;
                    if anim.frac != 0.0 {
                        let r2 = ad.rot_translate_scale(animpart, anim.frameb).0;
                        rot1 = pdmtx::tween_rot(rot1, r2, anim.frac);
                    }
                }
                let rotm = pdmtx::load_rotation(rot1);
                let ci = chrinfo;
                let mut yrot = ci.yrot;
                if ci.unk18 != 0.0 {
                    yrot = pdmtx::tween_rot_axis(yrot, ci.unk1c, ci.unk18);
                }
                let mut sp198 = pdmtx::load_y_rotation(yrot);
                pdmtx::set_translation(&mut sp198, ci.pos);
                let mut sp158 = pdmtx::mul(&sp198, &rotm);
                if scale != 1.0 {
                    sp158.x_axis *= scale;
                    sp158.y_axis *= scale;
                    sp158.z_axis *= scale;
                    sp158.x_axis.w = 0.0;
                    sp158.y_axis.w = 0.0;
                    sp158.z_axis.w = 0.0;
                }
                matrices[mtx as usize] = match parent {
                    Some(p) => pdmtx::mul(&p, &sp158),
                    None => sp158,
                };
            }
            _ => {}
        }
    }
}

/// The menu spike's `quaternion0f097518(q, 0.5)` (normalised, 0.99999).
fn half_rotation(q: Quat) -> Quat {
    let t = 0.5f32;
    let (mut w0, mut sign) = (q.w, 1.0f32);
    if w0 < 0.0 {
        w0 = -w0;
        sign = -1.0;
    }
    let (w, k) = if !(-0.99999..=0.99999).contains(&w0) {
        (q.w * t + (1.0 - t) * sign, t)
    } else {
        let th = w0.acos();
        let s = th.sin();
        let k1 = (t * th).sin() / s;
        let k0 = ((1.0 - t) * th).sin() / s;
        (q.w * k1 + k0 * sign, k1)
    };
    Quat::from_xyzw(q.x * k, q.y * k, q.z * k, w).normalize()
}

/// `pd_menu::pdmodel::Inst::set_matrices_with_anim` (body only; the distance
/// relations the tests do not compare are left out).
pub fn menu_walk(def: &ModelDef, vis: &[bool], matrices: &mut [Mat4], scale: f32, chrinfo: &ChrInfo, rendermtx: &Mat4, a: &Anim, bank: &AnimBank) {
    let parent_mtx = |m: &[Mat4], node: usize| match def.nodes[node].parent {
        Some(p) => def.find_node_mtx_index(p, 0).map(|i| m[i]),
        None => Some(*rendermtx),
    };
    let rts = |animpart: usize, second: bool| -> Option<(Vec3, Vec3, Vec3)> {
        let (num, fa, fb, frac) = if second { (a.animnum2, a.frame2a, a.frame2b, a.frac2) } else { (a.animnum, a.framea, a.frameb, a.frac) };
        let ad = bank.get(num)?;
        let (mut r, t, s) = ad.rot_translate_scale(animpart, fa);
        if frac != 0.0 {
            let (r2, _, _) = ad.rot_translate_scale(animpart, fb);
            r = pdmtx::tween_rot(r, r2, frac);
        }
        Some((r, t, s))
    };
    for i in 0..def.nodes.len() {
        if !walk_reaches(def, vis, i) {
            continue;
        }
        match def.nodes[i].kind.clone() {
            NodeKind::ChrInfo { animpart, mtx: slot } => {
                if slot < 0 {
                    continue;
                }
                let animpart = animpart as usize;
                let parent = parent_mtx(matrices, i);
                let rot1 = rts(animpart, false).map_or(Vec3::ZERO, |r| r.0);
                let abs = bank.flags(a.animnum) & ANIMFLAG_ABSOLUTETRANSLATION != 0;
                let rotm = if a.fracmerge != 0.0 {
                    let rot3 = rts(animpart, true).map_or(Vec3::ZERO, |r| r.0);
                    let mut q3 = if abs && bank.flags(a.animnum2) & ANIMFLAG_ABSOLUTETRANSLATION == 0 {
                        Quat::from_mat4(&pdmtx::mul(&pdmtx::load_y_rotation(chrinfo.yrot), &pdmtx::load_rotation(rot3)))
                    } else {
                        euler_quat(rot3)
                    };
                    let q1 = euler_quat(rot1);
                    if q1.dot(q3) < 0.0 {
                        q3 = -q3;
                    }
                    Mat4::from_quat(q1.slerp(q3, a.fracmerge))
                } else {
                    pdmtx::load_rotation(rot1)
                };
                let ci = chrinfo;
                let sp198 = if abs {
                    Mat4::from_translation(ci.pos)
                } else {
                    let mut yrot = ci.yrot;
                    if ci.unk18 != 0.0 {
                        yrot = pdmtx::tween_rot_axis(yrot, ci.unk1c, ci.unk18);
                    }
                    let mut m = pdmtx::load_y_rotation(yrot);
                    pdmtx::set_translation(&mut m, ci.pos);
                    m
                };
                let mut sp158 = pdmtx::mul(&sp198, &rotm);
                if scale != 1.0 {
                    for c in [&mut sp158.x_axis, &mut sp158.y_axis, &mut sp158.z_axis] {
                        c.x *= scale;
                        c.y *= scale;
                        c.z *= scale;
                    }
                }
                matrices[slot as usize] = match parent {
                    Some(p) => pdmtx::mul(&p, &sp158),
                    None => sp158,
                };
            }
            NodeKind::Position { pos: rodata_pos, animpart, mtx, flags: nodeflags } => {
                let animpart = animpart as usize;
                let is_root = i == 0;
                let parent = parent_mtx(matrices, i);
                let (mut rot1, mut translate1, mut scale1, mut sp128) = (Vec3::ZERO, Vec3::ZERO, Vec3::ONE, false);
                if a.animnum != 0 {
                    if let Some(ad) = bank.get(a.animnum) {
                        sp128 = ad.flags & ANIMFLAG_ABSOLUTETRANSLATION != 0 && is_root;
                        let (r, t, s) = ad.rot_translate_scale(animpart, a.framea);
                        rot1 = r;
                        translate1 = t;
                        scale1 = s;
                        if a.frac != 0.0 {
                            let (r2, t2, _) = ad.rot_translate_scale(animpart, a.frameb);
                            rot1 = pdmtx::tween_rot(rot1, r2, a.frac);
                            if sp128 {
                                translate1 += (t2 - translate1) * a.frac;
                            }
                        }
                    }
                }
                let pos = if sp128 {
                    translate1
                } else if translate1 != Vec3::ZERO {
                    let mut t = translate1 * a.animscale;
                    if !is_root {
                        t += rodata_pos;
                    }
                    t
                } else if !is_root {
                    rodata_pos
                } else {
                    translate1
                };
                let (q, euler, allowscale) = if a.fracmerge != 0.0 {
                    let rot3 = rts(animpart, true).map_or(Vec3::ZERO, |r| r.0);
                    let q1 = euler_quat(rot1);
                    let mut q3 = euler_quat(rot3);
                    if q1.dot(q3) < 0.0 {
                        q3 = -q3;
                    }
                    (q1.slerp(q3, a.fracmerge), None, false)
                } else {
                    (euler_quat(rot1), Some(rot1), sp128)
                };
                let mut local = match euler {
                    Some(r) => pdmtx::load_rotation_translation(pos, r),
                    None => Mat4::from_rotation_translation(q, pos),
                };
                if allowscale && scale != 1.0 {
                    pdmtx::scale3(&mut local, scale);
                }
                if scale1.x != 1.0 {
                    local.x_axis *= scale1.x;
                }
                if scale1.y != 1.0 {
                    local.y_axis *= scale1.y;
                }
                if scale1.z != 1.0 {
                    local.z_axis *= scale1.z;
                }
                let place = |m: Mat4| match parent {
                    Some(p) => pdmtx::mul(&p, &m),
                    None => m,
                };
                if mtx[0] >= 0 {
                    matrices[mtx[0] as usize] = place(local);
                }
                if nodeflags & MODELNODETYPE_0100 != 0 && mtx[1] >= 0 {
                    matrices[mtx[1] as usize] = place(Mat4::from_rotation_translation(half_rotation(q), pos));
                }
                if nodeflags & MODELNODETYPE_0200 != 0 && mtx[2] >= 0 {
                    let full = std::f32::consts::TAU;
                    let mut roty = match euler {
                        Some(r) => r.y,
                        None => 2.0 * q.w.clamp(-1.0, 1.0).acos(),
                    };
                    roty = if roty < std::f32::consts::PI { roty * 0.5 } else { full - (full - roty) * 0.5 };
                    let mut m = pdmtx::load_y_rotation(roty);
                    if roty >= std::f32::consts::PI {
                        roty = full - roty;
                    }
                    let k = if roty < 51f32.to_radians() { (roty.sin() / roty.cos() + 1.0).sqrt() } else { 1.5 };
                    m.z_axis.x *= k;
                    m.z_axis.y *= k;
                    m.z_axis.z *= k;
                    pdmtx::set_translation(&mut m, pos);
                    matrices[mtx[2] as usize] = place(m);
                }
            }
            NodeKind::PositionHeld { pos, mtx } => {
                let local = Mat4::from_translation(pos);
                let m = match parent_mtx(matrices, i) {
                    Some(p) => pdmtx::mul(&p, &local),
                    None => local,
                };
                if let Some(slot) = matrices.get_mut(mtx as usize) {
                    *slot = m;
                }
            }
            _ => {}
        }
    }
}
