//! `menu_render_model` (menu.c:1719): the 3D models inside menus — the
//! character-select carousel's body + head, and the hudpiece (the CI "eye"
//! that unfolds top-left and that the holoray lines are projected from).
//!
//! The menu logic (the `newparams` → `curparams` load handshake with its
//! `loaddelay`, the configure tween, the zoom, the animation bookkeeping) is
//! PD's, line for line. The model data and the matrix walk are
//! `pd_core::model`; the draw is `pd_core::model::draw` through the CPU RDP.
//!
//! Cameras: the dialog model is projected with `fovy 60`, the menu scissor's
//! aspect, `znear 10`, `zfar 300`, into a viewport the size of the scissor
//! (menu.c:2185); the hudpiece uses the stage's own camera (CI: near 15, far
//! 10000, full screen). Screen positions go through `cam0f0b4c3c` with the
//! full-screen camera either way — which is why the dialog model's offsets
//! shrink into its smaller viewport, as in PD.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use n64::rdp::{rgba, Cc, Filter, Texture, View};
use n64::rsp::Lights;
use pd_core::anim::Anim;
use pd_core::math::{dtor, load_rotation, quaternion0f096ca0, quaternion_slerp, quaternion_to_mtx, scale3};
use pd_core::model::draw::{draw_model, DrawOpts};
use pd_core::model::{Model, NodeKind, PoseParams, SKEL_CHR, SKEL_HEAD, SKEL_HUDPIECE};

use super::generated as gd;
use super::gfx::{cos_osc, linear_osc_pause_frac};
use super::menu::MenuModel;
use super::types::*;
use super::MenuSystem;

/// A menumodel's live model: PD's `bodymodel` with its `struct anim`, and the
/// hudpiece's liquid scroll.
#[derive(Clone)]
pub struct MenuModelInst {
    pub model: Model,
    pub anim: Anim,
    /// The hudpiece's scrolling "liquid" texture: added to every vertex `s` of
    /// MODELPART_HUDPIECE_0000, in S10.5 units (menu.c:2254).
    pub hud_s: i32,
}

/// `var80071468` (menu.c:1714): `gdSPDefLights1(0x96,0x96,0x96, 0xff,0xff,0xff,
/// 0xb2,0x4d,0x2e)`, the menu models' light.
pub fn menu_lights() -> Lights {
    let sb = |b: u8| b as i8 as f32 / 127.0;
    Lights { ambient: 150.0, diffuse: 255.0, dir: Vec3::new(sb(0xb2), sb(0x4d), sb(0x2e)), lookat_x: Vec3::X, lookat_y: Vec3::Y }
}

/// The stage camera (`g_Vars.currentplayer`) the menus run under: CI Training,
/// 320×220, fovy 60 (env.c:85 gives near 15, far 10000).
const CAM_W: f32 = 320.0;
const CAM_H: f32 = 220.0;
const CAM_FOVY: f32 = 60.0;

/// `c_scaley` / `c_scalex` (camera.c:77).
fn cam_scale() -> (f32, f32) {
    let halfh = CAM_H / 2.0;
    let halfw = CAM_W / 2.0;
    let sy = (CAM_FOVY.to_radians() / 2.0).tan() / halfh;
    let sx = sy * (CAM_W / CAM_H) * halfh / halfw;
    (sx, sy)
}

/// `cam0f0b4c3c`: a screen position → a unit view direction.
fn cam_screen_to_dir(sx: f32, sy: f32) -> Vec3 {
    let (scalex, scaley) = cam_scale();
    let y = (CAM_H / 2.0 - sy) * scaley;
    let x = (sx - CAM_W / 2.0) * scalex;
    Vec3::new(x, y, -1.0).normalize()
}

/// `cam0f0b4d04`: an eye-space point → a screen position.
fn cam_project(p: Vec3) -> (f32, f32) {
    let (scalex, scaley) = cam_scale();
    let v = 1.0 / p.z;
    (CAM_W / 2.0 - p.x * v / scalex, p.y * v / scaley + CAM_H / 2.0)
}

/// `body_calculate_head_offset` by `g_HeadsAndBodies` row.
fn head_offset(headnum: usize, bodynum: usize) -> f32 {
    pd_core::model::body_calculate_head_offset(gd::HEADS_AND_BODIES[headnum].ty, gd::HEADS_AND_BODIES[bodynum].ty)
}

const MODELPART_HEAD_SUNGLASSES: i32 = 0x0000;
const MODELPART_HEAD_EYESCLOSED: i32 = 0x0003;
const MODELPART_HEAD_HUDPIECE: i32 = 0x0004;
const MODELPART_CHR_HEADSPOT: i32 = 0x0004;
const MODELPART_CHR_0006: i32 = 0x0006;
const MODELPART_HUDPIECE_0000: i32 = 0x0000;
const MODELPART_HUDPIECE_0001: i32 = 0x0001;
const MODELPART_HUDPIECE_0002: i32 = 0x0002;

impl MenuSystem {
    /// The model slot a menumodel's instance lives in: players 0-3, hudpiece 4.
    fn model_slot(&self, modeltype: i32) -> usize {
        if modeltype == MENUMODELTYPE_HUDPIECE {
            4
        } else {
            self.mpplayernum
        }
    }

    /// `modeldef_load(filenum)`, remembering the first failure.
    fn menu_modeldef(&mut self, filenum: u32) -> Option<Arc<pd_core::model::ModelDef>> {
        match self.draw.models.by_filenum(filenum) {
            Ok(d) => Some(d),
            Err(e) => {
                self.draw.error.get_or_insert(e);
                None
            }
        }
    }

    /// The loading half of the handshake (menu.c:1776): body + head for
    /// head/body params, or one model file.
    fn menu_load_model(&mut self, mm: &mut MenuModel, slot: usize) {
        let params = mm.newparams;
        self.draw.model_inst[slot] = None;
        if params & 0xffff == 0xffff || params & 0x8000_0000 != 0 {
            let (mut headnum, bodynum) = if params & 0x8000_0000 != 0 {
                ((params & 0x3ff) as i32, ((params & 0xffc00) >> 10) as i32)
            } else {
                let mpheadnum = ((params >> 16) & 0xff) as usize;
                let mpbodynum = ((params >> 24) & 0xff) as usize;
                let bodynum = self.mp_get_body_id(mpbodynum);
                // SUBST: past the MP heads are the camera-made "perfect heads" /
                // there is no Controller Pak camera here, so they clamp to the
                // last MP head.
                let headnum = self.mp_get_head_id(mpheadnum.min(gd::MP_HEADS.len() - 1));
                (headnum, bodynum)
            };
            let hb = &gd::HEADS_AND_BODIES;
            let (Some(b), Some(_)) = (hb.get(bodynum as usize), hb.get(headnum.max(0) as usize)) else { return };
            if b.unk00_01 {
                headnum = -1;
            }
            mm.headnum = headnum;
            mm.bodynum = bodynum;
            let Some(body) = self.menu_modeldef(b.filenum as u32) else { return };
            let head = if headnum >= 0 && body.skel == SKEL_CHR && body.get_part(MODELPART_CHR_HEADSPOT).is_some() {
                self.menu_modeldef(hb[headnum as usize].filenum as u32).map(|h| {
                    let off = head_offset(headnum as usize, bodynum as usize);
                    if h.skel == SKEL_HEAD && off != 0.0 {
                        Arc::new(h.with_head_offset(off))
                    } else {
                        h
                    }
                })
            } else {
                None
            };
            // body_instantiate_model_to_addr (body.c): scale, varyheight (the
            // menu passes varyheight = 1), sunglasses off, the hudpiece off.
            let mut scale = b.scale * 0.100_000_01;
            if head.is_some() && b.canvaryheight {
                let frac = self.rng.randomfrac() * 0.05;
                scale *= 2.0 * frac - 0.05 + 1.0;
            }
            let mut model = Model::with_head(body, head);
            model.scale = scale;
            let anim = Anim { animscale: b.animscale, ..Anim::default() };
            if model.head.as_ref().is_some_and(|h| h.skel == SKEL_HEAD) {
                model.set_toggle(true, MODELPART_HEAD_SUNGLASSES, false);
                model.set_toggle(true, MODELPART_HEAD_HUDPIECE, false);
            }
            self.draw.model_inst[slot] = Some(MenuModelInst { model, anim, hud_s: 0 });
        } else {
            mm.headnum = -1;
            mm.bodynum = -1;
            if let Some(def) = self.menu_modeldef(params & 0xffff) {
                self.draw.model_inst[slot] = Some(MenuModelInst { model: Model::new(def), anim: Anim::default(), hud_s: 0 });
            }
        }
    }

    pub fn menu_render_model(&mut self, mm: &mut MenuModel, modeltype: i32) {
        let slot = self.model_slot(modeltype);
        // The load handshake (menu.c:1756).
        if mm.newparams != 0 {
            if mm.newparams == mm.curparams {
                mm.newparams = 0;
                mm.loaddelay = 0;
            } else {
                if mm.loaddelay == 0 {
                    mm.loaddelay = 1;
                    return;
                }
                mm.loaddelay -= 1;
                if mm.loaddelay != 0 {
                    return;
                }
                self.menu_load_model(mm, slot);
                mm.curparams = mm.newparams;
                mm.curanimnum = 0;
                mm.newparams = 0;
            }
        }
        if mm.curparams == 0 {
            // menu_unset_model nulls bodymodeldef.
            self.draw.model_inst[slot] = None;
            return;
        }
        let Some(mut inst) = self.draw.model_inst[slot].take() else { return };
        self.draw_menu_model(mm, modeltype, &mut inst);
        self.draw.model_inst[slot] = Some(inst);
    }

    /// Everything after the load in `menu_render_model` (menu.c:1860-2330).
    fn draw_menu_model(&mut self, mm: &mut MenuModel, modeltype: i32, inst: &mut MenuModelInst) {
        let def = inst.model.def.clone();

        // The z-buffer and the scissor (menu.c:1864); the hudpiece draws with
        // usezbuf false, on whatever the frame's z holds — nothing, here.
        self.draw.gfx.clear_z();
        if modeltype != MENUMODELTYPE_HUDPIECE {
            self.menu_apply_scissor();
        } else {
            // Drawn with the backgrounds, before any dialog scissor.
            self.draw.gfx.full_scissor();
        }

        // Zoom (the character select dialog), menu.c:1887.
        let mut haszoom = false;
        let mut zoompos = Vec3::ZERO;
        let mut zoomy = 1.0;
        if mm.zoom > 0.0 {
            let mut dodefault = true;
            if def.skel == SKEL_CHR {
                if let Some(node) = def.get_part(MODELPART_CHR_0006) {
                    if let NodeKind::Position { pos, .. } = def.nodes[node].kind {
                        let frac = linear_osc_pause_frac(mm.zoomtimer60 as f32 / 480.0);
                        zoompos = Vec3::new(0.0, -(pos.y / 7.6 * (1.0 - frac * frac)), 0.0);
                        haszoom = true;
                        mm.zoom = 100.0 + (1.0 - frac) * 270.0;
                        zoomy = mm.zoom / (pos.y / 2.0);
                        dodefault = false;
                    }
                }
            }
            if dodefault {
                if let Some(b) = def.bbox {
                    zoompos = -Vec3::new(b[1] - (b[1] - b[0]) * 0.5, b[3] - (b[3] - b[2]) * 0.5, b[5] - (b[5] - b[4]) * 0.5);
                    haszoom = true;
                    zoomy = mm.zoom / ((b[3] - b[2]) * 0.5);
                }
            }
        }

        // Position / rotation / scale, with the tweens (menu.c:1929).
        let (posx, posy, posz, scale);
        let rotmtx;
        if modeltype == MENUMODELTYPE_HUDPIECE {
            let k = 0.002f32;
            for _ in 0..self.lv.diffframe60 {
                if mm.curposx != mm.newposx {
                    mm.curposx = mm.newposx * k + (1.0 - k) * mm.curposx;
                }
                if mm.curposy != mm.newposy {
                    mm.curposy = mm.newposy * k + (1.0 - k) * mm.curposy;
                }
                if mm.curposz != mm.newposz {
                    mm.curposz = mm.newposz * k + (1.0 - k) * mm.curposz;
                }
                if mm.curscale != mm.newscale {
                    mm.curscale = mm.newscale * k + (1.0 - k) * mm.curscale;
                }
            }
            posx = mm.curposx;
            posy = mm.curposy;
            posz = mm.curposz;
            scale = mm.curscale;
            mm.currotx = mm.newrotx;
            mm.curroty = mm.newroty;
            mm.currotz = mm.newrotz;
            rotmtx = load_rotation(Vec3::new(mm.currotx, mm.curroty, mm.currotz));
        } else {
            let mut tween = None;
            if mm.configuring {
                mm.configurefrac += self.lv.diffframe60f / 40.0;
                if mm.configurefrac > 1.0 {
                    mm.configuring = false;
                    mm.curposx = mm.newposx;
                    mm.curposy = mm.newposy;
                    mm.curposz = mm.newposz;
                    mm.curscale = mm.newscale;
                } else {
                    let fracnew = -(mm.configurefrac * dtor(180.0)).cos() * 0.5 + 0.5;
                    let fraccur = 1.0 - fracnew;
                    let (px, py, pz) = if mm.flags & MENUMODELFLAG_HASPOSITION != 0 {
                        (mm.curposx * fraccur + fracnew * mm.newposx, mm.curposy * fraccur + fracnew * mm.newposy, mm.curposz * fraccur + fracnew * mm.newposz)
                    } else {
                        mm.curposx = mm.newposx;
                        mm.curposy = mm.newposy;
                        mm.curposz = mm.newposz;
                        (mm.newposx, mm.newposy, mm.newposz)
                    };
                    let sc = if mm.flags & MENUMODELFLAG_HASSCALE != 0 {
                        mm.curscale * fraccur + fracnew * mm.newscale
                    } else {
                        mm.curscale = mm.newscale;
                        mm.newscale
                    };
                    let rm = if mm.flags & MENUMODELFLAG_HASROTATION != 0 {
                        // menu.c:2023: PD's quaternions, no sign flip (the spike
                        // flipped q2 onto q1's hemisphere and used glam's slerp).
                        let q1 = quaternion0f096ca0(Vec3::new(mm.currotx, mm.curroty, mm.currotz));
                        let q2 = quaternion0f096ca0(Vec3::new(mm.newrotx, mm.newroty, mm.newrotz));
                        quaternion_to_mtx(quaternion_slerp(q1, q2, fracnew))
                    } else {
                        mm.currotx = mm.newrotx;
                        mm.curroty = mm.newroty;
                        mm.currotz = mm.newrotz;
                        load_rotation(Vec3::new(mm.newrotx, mm.newroty, mm.newrotz))
                    };
                    tween = Some((px, py, pz, sc, rm));
                }
            }
            match tween {
                Some((px, py, pz, sc, rm)) => {
                    posx = px;
                    posy = py;
                    posz = pz;
                    scale = sc;
                    rotmtx = rm;
                }
                None => {
                    mm.curposx = mm.newposx;
                    mm.curposy = mm.newposy;
                    mm.curposz = mm.newposz;
                    mm.curscale = mm.newscale;
                    mm.currotx = mm.newrotx;
                    mm.curroty = mm.newroty;
                    mm.currotz = mm.newrotz;
                    posx = mm.curposx;
                    posy = mm.curposy;
                    posz = mm.curposz;
                    scale = mm.curscale;
                    rotmtx = load_rotation(Vec3::new(mm.currotx, mm.curroty, mm.currotz));
                }
            }
        }

        // Screen position → eye position at z = −100 + posz (menu.c:2082).
        let us = self.draw.gfx.uiscale as f32;
        let screenz = -100.0 + posz;
        let (sx, sy) = if modeltype == MENUMODELTYPE_HUDPIECE { (mm.curposx * us, mm.curposy) } else { (posx * us + CAM_W * 0.5, posy + CAM_H * 0.5) };
        let dir = cam_screen_to_dir(sx, sy);
        let at = dir * (screenz / dir.z);

        // Part visibility (menu.c:2094): the head carousel's list.
        if mm.hideheadparts {
            for part in [MODELPART_HEAD_SUNGLASSES, MODELPART_HEAD_EYESCLOSED, MODELPART_HEAD_HUDPIECE] {
                inst.model.set_toggle(false, part, false);
            }
        }

        let mut posmtx = Mat4::from_translation(at);
        scale3(&mut posmtx, if haszoom { scale * zoomy } else { scale });
        let sp204 = Mat4::from_translation(if haszoom { zoompos } else { Vec3::new(mm.displacex, mm.displacey, mm.displacez) });
        let menumtx = posmtx * rotmtx * sp204;

        // Projection.
        let view = if modeltype == MENUMODELTYPE_HUDPIECE {
            View { proj: Mat4::perspective_rh_gl(CAM_FOVY.to_radians(), CAM_W / CAM_H, 15.0, 10000.0), vp: [0.0, 0.0, CAM_W * us, CAM_H], near: 15.0 }
        } else {
            let [x1, y1, x2, y2] = self.scissor_menu;
            let aspect = (x2 - x1) as f32 / (y2 - y1).max(1) as f32;
            View {
                proj: Mat4::perspective_rh_gl(CAM_FOVY.to_radians(), aspect, 10.0, 300.0),
                vp: [x1 as f32 * us, y1 as f32, (x2 - x1) as f32 * us, (y2 - y1) as f32],
                near: 10.0,
            }
        };

        // Animation (menu.c:2213).
        let bank = &self.draw.bank;
        if mm.newanimnum != 0 && mm.curanimnum != mm.newanimnum {
            let mut ctx = inst.model.anim_ctx(bank);
            let animnum = mm.newanimnum as u16;
            if mm.reverseanim {
                inst.anim.set_animation(&mut ctx, animnum, false, 0.0, -0.5, 0.0);
                let n = inst.anim.num_frames(bank) as f32;
                inst.anim.set_frame(bank, n);
            } else {
                inst.anim.set_animation(&mut ctx, animnum, false, 0.0, 0.5, 0.0);
            }
            mm.curanimnum = mm.newanimnum;
        }
        mm.newanimnum = 0;
        if mm.curanimnum != 0 {
            let diff240 = self.lv.diffframe240f as i32;
            let mut ctx = inst.model.anim_ctx(bank);
            inst.anim.tick_quarter(&mut ctx, diff240, true);
            let n = inst.anim.num_frames(bank) as f32;
            let frame = if mm.reverseanim { n - inst.anim.cur_frame() } else { inst.anim.cur_frame() };
            if frame >= n - 1.0 {
                mm.curanimnum = 0;
            }
        }
        mm.anim_frame = inst.anim.cur_frame();

        // Matrices.
        for m in inst.model.matrices.iter_mut() {
            *m = Mat4::IDENTITY;
        }
        inst.model.matrices[0] = menumtx;
        inst.model.set_matrices_with_anim(&PoseParams::new(menumtx, bank), Some(&inst.anim), None);

        if def.skel == SKEL_HUDPIECE {
            // The liquid texture scrolls (menu.c:2254).
            if let Some(node) = def.get_part(MODELPART_HUDPIECE_0000) {
                inst.hud_s -= 100 * self.lv.diffframe60;
                let min_s = def.nodes[node].batches.iter().flat_map(|&b| def.batches[b].verts.iter()).map(|v| (v.uv[0] * 32.0) as i32).min().unwrap_or(0);
                while min_s + inst.hud_s < -0x6000 {
                    inst.hud_s += 0x2000;
                }
            }
            // The rotor (menu.c:2272).
            if let Some(node) = def.get_part(MODELPART_HUDPIECE_0002) {
                if let Some(mi) = def.find_node_mtx_index(node, 0) {
                    let r = Mat4::from_rotation_x(cos_osc(self.frac20, 4.0));
                    inst.model.matrices[mi] *= r;
                }
            }
            // The holoray lines come from the eye (menu.c:2287).
            if let Some(node) = def.get_part(MODELPART_HUDPIECE_0001) {
                if matches!(self.menudata.root, MENUROOT_MAINMENU | MENUROOT_FILEMGR | MENUROOT_MPSETUP | MENUROOT_TRAINING) {
                    if let Some(mi) = def.find_node_mtx_index(node, 0) {
                        let p = inst.model.matrices[mi].w_axis.truncate();
                        let (px, py) = cam_project(p);
                        self.draw.text.holoray_fromx = ((px as i32) - (CAM_W * us) as i32 / 2) / self.draw.gfx.uiscale;
                        self.draw.text.holoray_fromy = py as i32 - CAM_H as i32 / 2;
                    }
                }
            }
        }

        let opts = DrawOpts { hud_s: inst.hud_s, ..DrawOpts::default() };
        let d = &mut self.draw;
        draw_model(&mut d.gfx, &mut d.textures, &inst.model, &view, &menu_lights(), &opts);
        if let Some(e) = d.textures.error.take() {
            d.error.get_or_insert(e);
        }
    }

    /// `tex_select(TEX_GENERAL_ENVSTAR)` + `gSPTextureRectangle` with
    /// `TEXEL0 × ENVIRONMENT` and `G_TF_POINT` (setup.c:740): the challenge /
    /// medal star, flipped vertically as PD's `t = 0x160, dtdy = -1` draws it.
    /// `TEX_GENERAL_ENVSTAR` in the env colour over `x1..x2 × y1..y2`
    /// (`gSPTextureRectangle` from texel `s0, t0`, `dtdy` per line; `x` is
    /// scaled by `g_UiScaleX`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_envstar_rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, env: u32, s0: f32, t0: f32, dtdy: f32) {
        let us = self.draw.gfx.uiscale;
        let tex: &Texture = &self.draw.res.envstar;
        self.draw.gfx.tex_rect(x1 * us, y1, x2 * us, y2, tex, s0, t0, 1.0 / us as f32, dtdy, Cc::TexEnv, rgba(env), Filter::Point);
    }

    pub fn draw_star(&mut self, x: i32, y: i32, size: i32, env: u32, flip: bool) {
        let us = self.draw.gfx.uiscale;
        let (t0, dtdy) = if flip { (11.0, -1.0) } else { (0.0, 1.0) };
        let tex: &Texture = &self.draw.res.envstar;
        self.draw.gfx.tex_rect(x * us, y, (x + size) * us, y + size, tex, 0.0, t0, 1.0 / us as f32, dtdy, Cc::TexEnv, rgba(env), Filter::Point);
    }
}
