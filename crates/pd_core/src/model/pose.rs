//! A live model (PD's `struct model`: definition, rwdata, matrices, attached
//! head) and `model_set_matrices_with_anim` (`model.c:1568`), which walks the
//! node tree in `model_update_matrices` order (`model.c:1502`) and fills the
//! matrix array:
//!
//! * `CHRINFO`: `model_update_chr_node_mtx` (`model.c:726`), the chr root at
//!   `rwdata->chrinfo.pos`/`yrot`, scaled by `model->scale`;
//! * `POSITION`: `model_update_position_node_mtx` (`model.c:1052`) through
//!   `model_position_joint_using_vec_rot` / `_quat_rot` (`model.c:834`, `:954`),
//!   including the elbow/knee helper matrices of `MODELNODETYPE_0100`/`0200`
//!   that every chr body uses;
//! * `POSITIONHELD`, and the `DISTANCE` / `TOGGLE` / `HEADSPOT` relations: a
//!   hidden toggle or distance prunes its subtree, and the head's nodes are
//!   walked in place of the headspot, taking their matrix from the body.
//!
//! A head has no matrices of its own: its display lists load matrix 0 from the
//! body's matrix segment (`SPSEGMENT_MODEL_MTX`, `model.c:3525`), the headspot's
//! parent joint, which is what [`Model::head_matrix`] returns.
//!
//! Sources: the old repo's `pd_menu/pdmodel.rs` (helpers, CHRINFO, heads) and
//! `pd_guns/model.rs` (the joint callback, the posing without an anim), merged
//! and checked against `model.c` / `quaternion.c`, where they differed from it:
//! the `0x0200` helper folds angles with `BADDTOR(360)` (the spike used 2π), the
//! merge slerps with PD's `quaternion_slerp` and builds matrices with
//! `quaternion_to_mtx` (the spike used glam's), the half-rotation is not
//! normalised, and an `ABSOLUTETRANSLATION` root is scaled by the stage's
//! translation factor. `model::tests` measures those differences against a
//! frozen copy of the spike walkers.

use std::sync::Arc;

use glam::{Mat4, Vec3};

use super::{ModelDef, NodeKind, MODELNODETYPE_0100, MODELNODETYPE_0200, MODELPART_CHR_HEADSPOT};
use crate::anim::{Anim, AnimBank, AnimCtx, ChrInfo, ANIMFLAG_ABSOLUTETRANSLATION, STAGE_TRANSLATION};
use crate::math::{
    self, baddtor, dtor, func0f096700, quaternion0f096ca0, quaternion0f0976c0, quaternion0f097044, quaternion0f097518, quaternion_slerp,
    quaternion_to_mtx, quaternion_to_transform_mtx, Quatf,
};

/// `g_ModelJointPositionedFunc`: called with each joint's matrix index and matrix
/// right after it is positioned under a parent, before its children inherit it.
pub type JointFn<'a> = &'a mut dyn FnMut(usize, &mut Mat4);

/// What `model_set_matrices_with_anim` reads from outside the model.
#[derive(Clone, Copy)]
pub struct PoseParams<'a> {
    /// `renderdata->rendermtx`: model to eye (or world) space for the root.
    pub rendermtx: Mat4,
    pub bank: &'a AnimBank,
    /// `PLAYERCOUNT()`. With two or more, an animation faster than 0.5 is posed on
    /// whole frames (`model.c:1576`).
    pub playercount: usize,
    /// `cam_get_lod_scale_z() * g_ModelDistanceScale` for the distance nodes, or
    /// `None` when `g_ModelDistanceDisabled` (every distance is then 0).
    pub lod_scale: Option<f32>,
}

impl<'a> PoseParams<'a> {
    /// One player, distances scaled 1 (a 60° field of view).
    pub fn new(rendermtx: Mat4, bank: &'a AnimBank) -> Self {
        PoseParams { rendermtx, bank, playercount: 1, lod_scale: Some(1.0) }
    }
}

/// `struct model`: the definition, per-node visibility (`rwdata`), the matrices,
/// the scale, the CHRINFO root's state and an attached head.
#[derive(Clone)]
pub struct Model {
    pub def: Arc<ModelDef>,
    /// `model_attach_head`: the head hung on the body's HEADSPOT.
    pub head: Option<Arc<ModelDef>>,
    /// Toggle / distance visibility by body node (unused for other kinds).
    pub vis: Vec<bool>,
    /// The same for the head's nodes.
    pub head_vis: Vec<bool>,
    pub matrices: Vec<Mat4>,
    /// `model->scale`.
    pub scale: f32,
    /// The CHRINFO root's `rwdata` (a chr body, the head-bob model).
    pub chrinfo: ChrInfo,
}

fn initial_vis(def: &ModelDef) -> Vec<bool> {
    // model_init_rw_data: toggles start visible, distances hidden until tested.
    def.nodes.iter().map(|n| !matches!(n.kind, NodeKind::Distance { .. })).collect()
}

impl Model {
    pub fn new(def: Arc<ModelDef>) -> Model {
        let vis = initial_vis(&def);
        let n = def.nummatrices;
        Model { def, head: None, vis, head_vis: Vec::new(), matrices: vec![Mat4::IDENTITY; n], scale: 1.0, chrinfo: ChrInfo::default() }
    }

    /// A body with `head` attached at its HEADSPOT (`model_attach_head`,
    /// `model.c:4275`); a body without a headspot ignores the head.
    pub fn with_head(def: Arc<ModelDef>, head: Option<Arc<ModelDef>>) -> Model {
        let mut m = Model::new(def);
        let head = head.filter(|_| m.headspot().is_some());
        m.head_vis = head.as_deref().map_or(Vec::new(), initial_vis);
        m.head = head;
        m
    }

    /// The body's HEADSPOT node.
    pub fn headspot(&self) -> Option<usize> {
        self.def.get_part(MODELPART_CHR_HEADSPOT).filter(|&n| matches!(self.def.nodes[n].kind, NodeKind::HeadSpot))
    }

    /// The matrix the head is drawn with: the headspot's parent joint.
    pub fn head_matrix(&self) -> Option<Mat4> {
        self.headspot().and_then(|n| self.def.find_node_mtx_index(n, 0)).map(|m| self.matrices[m])
    }

    /// The model's `struct anim` context: the CHRINFO root when the root is one.
    pub fn anim_ctx<'a>(&'a mut self, bank: &'a AnimBank) -> AnimCtx<'a> {
        let chrinfo = match self.def.nodes.first().map(|n| &n.kind) {
            Some(NodeKind::ChrInfo { animpart, .. }) => Some((&mut self.chrinfo, *animpart as usize)),
            _ => None,
        };
        AnimCtx { bank, scale: self.scale, chrinfo, merging_enabled: true }
    }

    // ── toggles ──────────────────────────────────────────────────────────────

    /// Reset every toggle to visible, what `bgun_execute_model_cmd_list`'s
    /// compiled "toggle.visible = true" commands do at the top of every frame.
    pub fn reset_toggles(&mut self) {
        for (i, n) in self.def.nodes.iter().enumerate() {
            if matches!(n.kind, NodeKind::Toggle) {
                self.vis[i] = true;
            }
        }
    }

    /// Set a TOGGLE part's visibility (`rwdata->toggle.visible`) on the body
    /// (`head == false`) or the attached head.
    pub fn set_toggle(&mut self, head: bool, partnum: i32, visible: bool) {
        let (def, vis) = if head {
            match &self.head {
                Some(h) => (h.clone(), &mut self.head_vis),
                None => return,
            }
        } else {
            (self.def.clone(), &mut self.vis)
        };
        if let Some(n) = def.get_part(partnum) {
            if matches!(def.nodes[n].kind, NodeKind::Toggle) {
                vis[n] = visible;
            }
        }
    }

    /// `bgun_set_part_visible` on a part of the body.
    pub fn set_part_visible(&mut self, partnum: i32, visible: bool) {
        self.set_toggle(false, partnum, visible);
    }

    /// A body part's toggle state (false if there is no such toggle part).
    pub fn part_visible(&self, partnum: i32) -> bool {
        self.def.get_part(partnum).is_some_and(|n| matches!(self.def.nodes[n].kind, NodeKind::Toggle) && self.vis[n])
    }

    /// True if every toggle/distance at or above `node` is on.
    pub fn node_visible(&self, node: usize) -> bool {
        self.vis_at(&self.def, &self.vis, node) && Self::reaches(&self.def, &self.vis, node)
    }

    fn vis_at(&self, def: &ModelDef, vis: &[bool], node: usize) -> bool {
        !matches!(def.nodes[node].kind, NodeKind::Toggle | NodeKind::Distance { .. }) || vis[node]
    }

    /// Whether the walk reaches `node`: no toggle or distance above it (not the
    /// node itself) is hidden. PD nulls a hidden relation's `child`
    /// (`model_apply_toggle_relations`), so the walk skips its subtree.
    pub fn reaches(def: &ModelDef, vis: &[bool], node: usize) -> bool {
        let mut cur = def.nodes[node].parent;
        while let Some(i) = cur {
            if matches!(def.nodes[i].kind, NodeKind::Toggle | NodeKind::Distance { .. }) && !vis[i] {
                return false;
            }
            cur = def.nodes[i].parent;
        }
        true
    }

    // ── the walk ─────────────────────────────────────────────────────────────

    /// `model_set_matrices_with_anim` (`model.c:1568`). `anim` is `model->anim`;
    /// `None` poses every joint at its rest offset (a model without a `struct
    /// anim`).
    pub fn set_matrices_with_anim(&mut self, p: &PoseParams, anim: Option<&Anim>, mut joint_fn: Option<JointFn>) {
        let mut local;
        let anim = match anim {
            Some(a) if a.animnum != 0 && p.playercount >= 2 && a.speed.abs() > 0.5 => {
                local = a.clone();
                local.frac = 0.0;
                local.frac2 = 0.0;
                Some(&local)
            }
            other => other,
        };
        let def = self.def.clone();
        for i in 0..def.nodes.len() {
            if !Self::reaches(&def, &self.vis, i) {
                continue;
            }
            match def.nodes[i].kind.clone() {
                NodeKind::ChrInfo { animpart, mtx } => {
                    let a = anim.cloned().unwrap_or_default();
                    self.update_chr_node(&def, i, animpart as usize, mtx, p, &a);
                }
                NodeKind::Position { pos, animpart, mtx, flags } => {
                    self.update_position_node(&def, i, pos, animpart as usize, mtx, flags, p, anim, &mut joint_fn);
                }
                NodeKind::PositionHeld { pos, mtx } => {
                    // model_update_position_held_node_mtx (model.c:1191)
                    let m = self.place(&def, i, p, Mat4::from_translation(pos));
                    if let Some(slot) = self.matrices.get_mut(mtx as usize) {
                        *slot = m;
                    }
                }
                NodeKind::Distance { near, far } => {
                    let d = self.distance(&def, i, p);
                    self.vis[i] = Self::distance_visible(d, near, far, self.scale);
                }
                NodeKind::HeadSpot => self.update_head(p),
                _ => {}
            }
        }
    }

    /// `model_update_distance_relations` (`model.c:1216`).
    fn distance_visible(distance: f32, near: f32, far: f32, scale: f32) -> bool {
        (distance > near * scale || near == 0.0) && distance <= far * scale
    }

    fn distance(&self, def: &ModelDef, node: usize, p: &PoseParams) -> f32 {
        match (p.lod_scale, def.find_node_mtx_index(node, 0)) {
            (Some(k), Some(m)) => -self.matrices[m].w_axis.z * k,
            _ => 0.0,
        }
    }

    /// The head's nodes, walked where the headspot is (`model_apply_head_relations`):
    /// only its distance relations need updating, measured on the body's matrix.
    fn update_head(&mut self, p: &PoseParams) {
        let Some(head) = self.head.clone() else { return };
        let spot = self.head_matrix();
        for i in 0..head.nodes.len() {
            if !Self::reaches(&head, &self.head_vis, i) {
                continue;
            }
            if let NodeKind::Distance { near, far } = head.nodes[i].kind {
                let d = match (p.lod_scale, spot) {
                    (Some(k), Some(m)) => -m.w_axis.z * k,
                    _ => 0.0,
                };
                self.head_vis[i] = Self::distance_visible(d, near, far, self.scale);
            }
        }
    }

    /// The matrix a node's parent chain provides, or `rendermtx` at the root:
    /// the `if (node->parent) ... else renderdata->rendermtx` head of every
    /// positioning function. `None` when the parent has no matrix.
    fn parent_mtx(&self, def: &ModelDef, node: usize, p: &PoseParams) -> Option<Mat4> {
        match def.nodes[node].parent {
            Some(par) => def.find_node_mtx_index(par, 0).map(|i| self.matrices[i]),
            None => Some(p.rendermtx),
        }
    }

    fn place(&self, def: &ModelDef, node: usize, p: &PoseParams, local: Mat4) -> Mat4 {
        match self.parent_mtx(def, node, p) {
            Some(par) => math::mul(&par, &local),
            None => local,
        }
    }

    fn rts(bank: &AnimBank, animnum: u16, part: usize, frame: i32) -> (Vec3, Vec3, Vec3) {
        match bank.get(animnum) {
            Some(ad) => ad.rot_translate_scale(part, frame),
            None => (Vec3::ZERO, Vec3::ZERO, Vec3::ONE),
        }
    }

    /// The merge's source rotation: the old animation's, tweened by `frac2`.
    fn rot3(bank: &AnimBank, a: &Anim, part: usize) -> Vec3 {
        let mut rot3 = Self::rts(bank, a.animnum2, part, a.frame2a).0;
        if a.frac2 != 0.0 {
            let rot4 = Self::rts(bank, a.animnum2, part, a.frame2b).0;
            rot3 = math::tween_rot(rot3, rot4, a.frac2);
        }
        rot3
    }

    /// `model_update_chr_node_mtx` (`model.c:726`).
    fn update_chr_node(&mut self, def: &ModelDef, node: usize, animpart: usize, slot: i16, p: &PoseParams, a: &Anim) {
        if slot < 0 {
            return;
        }
        let bank = p.bank;
        let parent = self.parent_mtx(def, node, p);
        let mut rot1 = Self::rts(bank, a.animnum, animpart, a.framea).0;
        if a.frac != 0.0 {
            let rot2 = Self::rts(bank, a.animnum, animpart, a.frameb).0;
            rot1 = math::tween_rot(rot1, rot2, a.frac);
        }
        let abs = bank.flags(a.animnum) & ANIMFLAG_ABSOLUTETRANSLATION != 0;
        let sp1d8 = if a.fracmerge != 0.0 {
            let rot3 = Self::rot3(bank, a, animpart);
            let mut spec = if abs && bank.flags(a.animnum2) & ANIMFLAG_ABSOLUTETRANSLATION == 0 {
                let sp38 = math::mul(&math::load_y_rotation(self.chrinfo.yrot), &math::load_rotation(rot3));
                quaternion0f097044(&sp38)
            } else {
                quaternion0f096ca0(rot3)
            };
            let spfc = quaternion0f096ca0(rot1);
            quaternion0f0976c0(spfc, &mut spec);
            quaternion_to_mtx(quaternion_slerp(spfc, spec, a.fracmerge))
        } else {
            math::load_rotation(rot1)
        };
        let ci = &self.chrinfo;
        let sp198 = if abs {
            Mat4::from_translation(ci.pos)
        } else {
            let mut yrot = ci.yrot;
            if ci.unk18 != 0.0 {
                yrot = math::tween_rot_axis(yrot, ci.unk1c, ci.unk18);
            }
            let mut m = math::load_y_rotation(yrot);
            math::set_translation(&mut m, ci.pos);
            m
        };
        let mut sp158 = math::mul(&sp198, &sp1d8);
        if self.scale != 1.0 {
            math::scale3x3(&mut sp158, self.scale);
        }
        self.matrices[slot as usize] = match parent {
            Some(par) => math::mul(&par, &sp158),
            None => sp158,
        };
    }

    /// `model_update_position_node_mtx` (`model.c:1052`).
    #[allow(clippy::too_many_arguments)]
    fn update_position_node(
        &mut self,
        def: &ModelDef,
        node: usize,
        rodata_pos: Vec3,
        animpart: usize,
        mtx: [i16; 3],
        nodeflags: u32,
        p: &PoseParams,
        anim: Option<&Anim>,
        joint_fn: &mut Option<JointFn>,
    ) {
        let is_root = node == 0;
        let Some(a) = anim else {
            // No struct anim: the rest offset (model.c:1177).
            let m = self.place(def, node, p, Mat4::from_translation(rodata_pos));
            self.matrices[mtx[0] as usize] = m;
            return;
        };
        let bank = p.bank;
        let (mut rot1, mut translate1, mut scale1) = (Vec3::ZERO, Vec3::ZERO, Vec3::ONE);
        let mut sp128 = false;
        if a.animnum != 0 {
            sp128 = bank.flags(a.animnum) & ANIMFLAG_ABSOLUTETRANSLATION != 0 && is_root;
            (rot1, translate1, scale1) = Self::rts(bank, a.animnum, animpart, a.framea);
            if a.frac != 0.0 {
                let (rot2, translate2, _) = Self::rts(bank, a.animnum, animpart, a.frameb);
                rot1 = math::tween_rot(rot1, rot2, a.frac);
                if sp128 {
                    translate1 += (translate2 - translate1) * a.frac;
                }
            }
        }
        let rel = |t: Vec3| if is_root { t } else { t + rodata_pos };
        if a.fracmerge != 0.0 {
            let rot3 = Self::rot3(bank, a, animpart);
            let sp88 = quaternion0f096ca0(rot1);
            let mut sp78 = quaternion0f096ca0(rot3);
            quaternion0f0976c0(sp88, &mut sp78);
            let sp68 = quaternion_slerp(sp88, sp78, a.fracmerge);
            let pos = if translate1 != Vec3::ZERO {
                rel(translate1 * a.animscale)
            } else if !is_root {
                rodata_pos
            } else {
                translate1
            };
            self.position_joint(def, node, mtx, nodeflags, p, JointRot::Quat(sp68), pos, false, scale1, joint_fn);
        } else if sp128 {
            let pos = translate1 * STAGE_TRANSLATION;
            self.position_joint(def, node, mtx, nodeflags, p, JointRot::Euler(rot1), pos, true, scale1, joint_fn);
        } else {
            let pos = if translate1 != Vec3::ZERO {
                rel(translate1 * a.animscale)
            } else if !is_root {
                rodata_pos
            } else {
                translate1
            };
            self.position_joint(def, node, mtx, nodeflags, p, JointRot::Euler(rot1), pos, false, scale1, joint_fn);
        }
    }

    /// `model_position_joint_using_vec_rot` / `_quat_rot` (`model.c:834`, `:954`).
    #[allow(clippy::too_many_arguments)]
    fn position_joint(
        &mut self,
        def: &ModelDef,
        node: usize,
        mtx: [i16; 3],
        nodeflags: u32,
        p: &PoseParams,
        rot: JointRot,
        pos: Vec3,
        allowscale: bool,
        s: Vec3,
        joint_fn: &mut Option<JointFn>,
    ) {
        let rendermtx = self.parent_mtx(def, node, p);
        let mut local = match rot {
            JointRot::Euler(r) => math::load_rotation_translation(pos, r),
            JointRot::Quat(q) => quaternion_to_transform_mtx(pos, q),
        };
        if allowscale && self.scale != 1.0 {
            // Only the vec-rot path takes allowscale (the quat path has none).
            math::scale3(&mut local, self.scale);
        }
        if s.x != 1.0 {
            math::scale_col0(&mut local, s.x);
        }
        if s.y != 1.0 {
            math::scale_col1(&mut local, s.y);
        }
        if s.z != 1.0 {
            math::scale_col2(&mut local, s.z);
        }
        let m0 = mtx[0] as usize;
        match rendermtx {
            Some(r) => {
                let mut m = math::mul(&r, &local);
                if let Some(f) = joint_fn.as_mut() {
                    f(m0, &mut m);
                }
                self.matrices[m0] = m;
            }
            None => self.matrices[m0] = local,
        }
        let place = |m: Mat4| match rendermtx {
            Some(r) => math::mul(&r, &m),
            None => m,
        };
        if nodeflags & MODELNODETYPE_0100 != 0 && mtx[1] >= 0 {
            // quaternion0f097518(q, 0.5): half the joint's rotation, the elbow /
            // knee skin between the two bones.
            let q = match rot {
                JointRot::Euler(r) => quaternion0f096ca0(r),
                JointRot::Quat(q) => q,
            };
            self.matrices[mtx[1] as usize] = place(quaternion_to_transform_mtx(pos, quaternion0f097518(q, 0.5)));
        }
        if nodeflags & MODELNODETYPE_0200 != 0 && mtx[2] >= 0 {
            let mut roty = match rot {
                JointRot::Euler(r) => r.y,
                JointRot::Quat(q) => 2.0 * q[0].acos(),
            };
            if roty < dtor(180.0) {
                roty *= 0.5;
            } else {
                roty = baddtor(360.0) - (baddtor(360.0) - roty) * 0.5;
            }
            let mut m = math::load_y_rotation(roty);
            if roty >= dtor(180.0) {
                roty = baddtor(360.0) - roty;
            }
            let k = if roty < dtor(51.0) { func0f096700(roty) } else { 1.5 };
            math::scale_col2_xyz(&mut m, k);
            math::set_translation(&mut m, pos);
            self.matrices[mtx[2] as usize] = place(m);
        }
    }
}

#[derive(Clone, Copy)]
enum JointRot {
    Euler(Vec3),
    Quat(Quatf),
}
