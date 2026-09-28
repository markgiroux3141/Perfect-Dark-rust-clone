//! `bondgun.c`, the half that places the gun on screen every frame: the sway
//! blend, the crosshair swivel, shot spread, the gangsta tilt, the per-weapon
//! model tweaks (slide, Reaper barrels, sniper scope, Devastator loader,
//! shotgun star), the muzzle flash, casings and beams, and `bgun0f0a5550`, which
//! turns all of it into the gun model's camera-space matrices.

use glam::{Mat4, Vec3, Vec4};
use pd_core::ids::*;
use pd_core::math::{self, baddtor, dtor};
use pd_core::model::{NodeKind, PoseParams};
use pd_core::rng::Rng;

use super::gset::{FuncDef, Gset};
use super::{Bgun, GunCtx, GunEvent};

/// `func0f096b70` (`game_096b20.c:15`): Catmull-Rom through four points.
fn catmull(a: Vec3, b: Vec3, c: Vec3, d: Vec3, t: f32) -> Vec3 {
    let sq = t * t;
    let cu = sq * t;
    let m0 = sq - 0.5 * (t + cu);
    let m1 = 1.5 * cu - 2.5 * sq + 1.0;
    let m2 = -1.5 * cu + 2.0 * sq + 0.5 * t;
    let m3 = 0.5 * (cu - sq);
    a * m0 + b * m1 + c * m2 + d * m3
}

/// `bgun_calculate_blend` (`:3228`): pick the next random sway key.
pub(crate) fn bgun_calculate_blend(b: &mut Bgun, gset: &Gset, rng: &mut Rng, h: usize) {
    let sway = gset.weapon(b.bgun_get_weapon_num(h)).map_or(1.0, |w| w.sway);
    let sp60 = ((b.hands[h].curblendpos + 2) % 4) as usize;
    let sp58 = (b.hands[h].curblendpos + 1) % 4;
    b.hands[h].curblendpos = sp58;
    let r: [f32; 8] = std::array::from_fn(|_| rng.randomfrac());
    let hand = &mut b.hands[h];
    hand.blendlook[sp60] = Vec3::new((r[0] - 0.5) * 0.08 * sway, (r[1] - 0.5) * 0.1 * sway, -1.0);
    hand.blendup[sp60] = Vec3::new((r[2] - 0.5) * 0.1 * sway, 1.0, (r[3] - 0.5) * 0.1 * sway);
    hand.blendpos[sp60] = Vec3::new(r[4] * 0.75 + 1.5, (2.0 + r[5]) * hand.blendscale1, (r[6] - 0.5) * 2.5);
    if hand.sideflag < 0 {
        hand.blendpos[sp60].x *= -1.0;
        hand.sideflag = if hand.sideflag == -2 { 1 } else { -2 };
    } else {
        hand.sideflag = if hand.sideflag == 2 { -1 } else { 2 };
    }
    hand.blendscale1 = -hand.blendscale1;
}

impl GunCtx<'_> {
    pub(crate) fn bgun_calculate_blend(&mut self, h: usize) {
        bgun_calculate_blend(self.b, self.gset, self.rng, h);
    }

    /// `bgun_update_blend` (`:3271`): the damped spline sway → damppos/look/up.
    pub(crate) fn bgun_update_blend(&mut self, h: usize) {
        let xshift = self.b.hands[h].xshift;
        let amp = self.b.p.gunposamplitude;
        let lv240 = self.lv.lvupdate240;
        let hand = &mut self.b.hands[h];
        let pos = hand.curblendpos as usize;
        let i0 = (pos + 3) % 4;
        let i2 = (pos + 1) % 4;
        let i3 = (pos + 2) % 4;
        let t = hand.dampt;
        let mut sp5c = catmull(hand.blendpos[i0], hand.blendpos[pos], hand.blendpos[i2], hand.blendpos[i3], t);
        let sp50 = catmull(hand.blendlook[i0], hand.blendlook[pos], hand.blendlook[i2], hand.blendlook[i3], t);
        let sp44 = catmull(hand.blendup[i0], hand.blendup[pos], hand.blendup[i2], hand.blendup[i3], t);
        sp5c *= amp;
        sp5c.x += hand.adjustdamp.x;
        sp5c.y += hand.adjustdamp.y;
        sp5c.x += xshift;
        for _ in 0..lv240 {
            hand.damppossum = hand.damppossum * 0.9872 + sp5c;
            hand.damplooksum = hand.damplooksum * 0.9872 + sp50;
            hand.dampupsum = hand.dampupsum * 0.9872 + sp44;
        }
        hand.damppos = hand.damppossum * 0.012_799_978 * 2.0;
        hand.damplook = hand.damplooksum * 0.012_799_978;
        hand.dampup = hand.dampupsum * 0.012_799_978;
    }

    /// `bgun0f09d8dc` (`:3343`): drive the sway from movement. `breathing` is
    /// `bhead_get_breathing_value`, `arg1` the gun speed (the heart rate),
    /// `arg2` the vertical look and crouch speed, `arg3` the turn speed, `arg4`
    /// the strafe.
    pub fn bgun_update_sway(&mut self, breathing: f32, arg1: f32, arg2: f32, arg3: f32, arg4: f32) {
        let lv240 = self.lv.lvupdate240;
        let lv60 = self.lv.lvupdate60freal;
        let sp50 = arg2.abs();
        let (crouchpos, bondbreathing) = (self.pl.crouchpos, self.pl.bondbreathing);
        let p = &mut self.b.p;
        if arg1 > 0.8 {
            p.gunposamplitude = 1.0;
        } else if arg1 > 0.1 {
            let tmp = 1.0 - ((arg1 - 0.1) * baddtor(360.0) / 2.8).cos();
            p.gunposamplitude = 0.8 * tmp + 0.2;
        } else {
            p.gunposamplitude = 0.1;
        }
        if crouchpos != CROUCHPOS_SQUAT && p.gunposamplitude < 0.3 * bondbreathing {
            p.gunposamplitude = 0.3 * bondbreathing;
        }
        if p.gunposamplitude < 0.5 * sp50 {
            p.gunposamplitude = 0.5 * sp50;
        }
        for _ in 0..lv240 {
            p.gunampsum = 0.9872 * p.gunampsum + p.gunposamplitude;
        }
        p.gunposamplitude = 0.012_799_978 * p.gunampsum;
        let mut breathing = breathing;
        if breathing < (1.0 / 60.0) * sp50 {
            breathing = (1.0 / 60.0) * sp50;
        }
        for _ in 0..lv240 {
            p.cyclesum = 0.9872 * p.cyclesum + breathing;
        }
        let breathing = p.cyclesum * 0.012_799_978;
        let sp4c = breathing * lv60;
        let mut dampt0 = self.b.hands[0].dampt + sp4c;
        while dampt0 >= 1.0 {
            self.bgun_calculate_blend(HAND_RIGHT);
            dampt0 -= 1.0;
            self.b.p.syncoffset += 1;
        }
        self.b.p.synccount += lv60;
        if self.b.p.synccount > 60.0 {
            self.b.p.synccount = 0.0;
            self.b.p.syncchange = (self.rng.randomfrac() - 0.5) * 0.2 / 60.0;
        }
        if self.b.p.syncchange + sp4c > 0.0 {
            self.b.p.gunsync += self.b.p.syncchange;
        }
        let p = &mut self.b.p;
        if p.gunsync > 0.5 {
            p.gunsync = 0.5;
        } else if p.gunsync < -0.5 {
            p.gunsync = -0.5;
        } else if p.gunsync < 0.1 && p.gunsync > -0.1 {
            p.gunsync = if p.gunsync > 0.0 { -0.1 } else { 0.1 };
        }
        let mut dampt1 = dampt0 + self.b.p.syncoffset as f32 + self.b.p.gunsync;
        while dampt1 >= 1.0 {
            self.bgun_calculate_blend(HAND_LEFT);
            dampt1 -= 1.0;
            self.b.p.syncoffset -= 1;
        }
        let dampts = [dampt0, dampt1];
        for (i, hand) in self.b.hands.iter_mut().enumerate() {
            hand.dampt = dampts[i];
            hand.adjustdamp.x = -1.75 * arg3 + -0.8 * arg4;
            hand.adjustdamp.y = -2.0 * arg2;
        }
    }

    // ─── the swivel (4866-5140) ──────────────────────────────────────────────

    /// `bgun_swivel` (`:4866`), without the aim-assist dot (`hasdotinfo`,
    /// solo only).
    pub fn bgun_swivel(&mut self, screenx: f32, screeny: f32, crossdamp: f32, aimdamp: f32) {
        let c = *self.cam;
        let (sw, shh) = (c.c_screenwidth, c.c_screenheight);
        let mut x = [screenx, screenx];
        let mut y = [screeny, screeny];

        // The right hand alone, reloading: recentred until the reload is nearly done.
        if !self.b.hands[HAND_LEFT].inuse && self.b.hands[HAND_RIGHT].state == HANDSTATE_RELOAD && self.b.hands[HAND_RIGHT].animcmd.is_some() {
            let numframes = if self.b.hands[HAND_RIGHT].weaponnum == WEAPON_CROSSBOW { 5 } else { 25 };
            let n = self.b.hands[HAND_RIGHT].anim.num_frames(self.bank);
            if (self.bgun_get_current_keyframe(HAND_RIGHT) as i32) < n - numframes {
                x[HAND_RIGHT] = 0.0;
                y[HAND_RIGHT] = 0.0;
            }
        }
        if self.b.hands[HAND_RIGHT].weaponnum == WEAPON_UNARMED {
            x[HAND_RIGHT] = self.b.swivel_extra[0];
            y[HAND_RIGHT] = self.b.swivel_extra[1];
        }

        let lv240 = self.lv.lvupdate240;
        let p = &mut self.b.p;
        p.oldcrosspos = p.crosspos;
        if crossdamp != p.guncrossdamp {
            p.crosspossum[0] = p.crosspossum[0] * (1.0 - p.guncrossdamp) / (1.0 - crossdamp);
            p.crosspossum[1] = p.crosspossum[1] * (1.0 - p.guncrossdamp) / (1.0 - crossdamp);
            p.guncrossdamp = crossdamp;
        }
        if aimdamp != p.gunaimdamp {
            p.crosssum2[0] = p.crosssum2[0] * (1.0 - p.gunaimdamp) / (1.0 - aimdamp);
            p.crosssum2[1] = p.crosssum2[1] * (1.0 - p.gunaimdamp) / (1.0 - aimdamp);
            p.gunaimdamp = aimdamp;
        }
        for _ in 0..lv240 {
            p.crosspossum[0] = p.crosspossum[0] * crossdamp + screenx;
            p.crosspossum[1] = p.crosspossum[1] * crossdamp + screeny;
            for (hi, hand) in self.b.hands.iter_mut().enumerate() {
                hand.guncrosspossum[0] = 0.926_969_7 * hand.guncrosspossum[0] + x[hi];
                hand.guncrosspossum[1] = 0.926_969_7 * hand.guncrosspossum[1] + y[hi];
            }
        }
        let p = &mut self.b.p;
        p.crosspos[0] = (p.crosspossum[0] * (1.0 - crossdamp) * sw * 0.5 + sw * 0.5).clamp(3.0, sw - 4.0) + c.c_screenleft;
        p.crosspos[1] = (p.crosspossum[1] * (1.0 - crossdamp) * shh * 0.5 + shh * 0.5).clamp(3.0, shh - 4.0) + c.c_screentop;
        for hand in self.b.hands.iter_mut() {
            hand.crosspos[0] = (hand.guncrosspossum[0] * 0.073_030_29 * sw * 0.5 + sw * 0.5).clamp(3.0, sw - 4.0) + c.c_screenleft;
            hand.crosspos[1] = (hand.guncrosspossum[1] * 0.073_030_29 * shh * 0.5 + shh * 0.5).clamp(3.0, shh - 4.0) + c.c_screentop;
        }
        let p = &mut self.b.p;
        for _ in 0..lv240 {
            p.crosssum2[0] = p.crosssum2[0] * aimdamp + screenx;
            p.crosssum2[1] = p.crosssum2[1] * aimdamp + screeny;
        }
        p.crosspos2[0] = p.crosssum2[0] * (1.0 - aimdamp) * sw * 0.5 + sw * 0.5 + c.c_screenleft;
        p.crosspos2[1] = p.crosssum2[1] * (1.0 - aimdamp) * shh * 0.5 + shh * 0.5 + c.c_screentop;
        let aimpos = c.cam0f0b4c3c(self.b.p.crosspos2, 1000.0);
        self.bgun_set_aim_pos(aimpos);
    }

    /// `bgun_swivel_with_damp` (`:5034`).
    pub fn bgun_swivel_with_damp(&mut self, screenx: f32, screeny: f32, crossdamp: f32) {
        let w = self.gset.weapon(self.bgun_get_weapon_num(HAND_RIGHT)).map(|w| w.aim.aimdamp).unwrap_or(0.9767);
        let aimdamp = w.max(crossdamp);
        self.bgun_swivel(screenx, screeny, crossdamp, aimdamp);
    }

    /// `bgun_swivel_without_damp` (`:5052`).
    pub fn bgun_swivel_without_damp(&mut self, screenx: f32, screeny: f32) {
        let aimdamp = self.gset.weapon(self.bgun_get_weapon_num(HAND_RIGHT)).map(|w| w.aim.aimdamp).unwrap_or(0.9767);
        self.bgun_swivel(screenx, screeny, 0.945, aimdamp);
    }

    /// `bgun_set_aim_pos` (`:9246`).
    fn bgun_set_aim_pos(&mut self, coord: Vec3) {
        for h in 0..2 {
            let xs = self.b.hands[h].xshift;
            self.b.hands[h].aimpos = Vec3::new(xs + coord.x, coord.y, coord.z);
        }
    }

    /// `bgun_calculate_player_shot_spread` (`:5086`): the camera-space direction
    /// a round leaves in, from the eye through the crosshair plus the spread.
    pub fn bgun_calculate_player_shot_spread(&mut self, h: usize, dorandom: bool) -> Vec3 {
        let mut spread = 0.0;
        if let Some(s) = self.func_of(h).and_then(|f| f.shoot) {
            spread = s.spread;
        }
        if self.gset.has_aim_flag(self.bgun_get_weapon_num(h), INVAIMFLAG_ACCURATESINGLESHOT) && self.b.hands[h].burstbullets == 1 {
            spread *= 0.25;
        }
        if self.pl.crouchpos == CROUCHPOS_SQUAT {
            spread *= 0.5;
        }
        if self.b.hands[HAND_LEFT].inuse {
            spread *= 1.5;
        }
        let c = *self.cam;
        let scaledspread = 120.0 * spread / c.c_perspfovy;
        // vi_get_height(): the framebuffer's height, the view's for one player.
        let vi_height = c.c_screenheight;
        let mut rf = || if dorandom { (self.rng.randomfrac() - 0.5) * self.rng.randomfrac() } else { 0.0 };
        let rx = rf();
        let ry = rf();
        let cx = self.b.p.crosspos[0] + rx * scaledspread * c.c_screenwidth / (vi_height * c.c_perspaspect);
        let cy = self.b.p.crosspos[1] + (ry * scaledspread * c.c_screenheight) / vi_height;
        c.cam0f0b4c3c([cx, cy], 1.0)
    }

    // ─── per-weapon model tweaks (6421-7036) ─────────────────────────────────

    /// `bgun_update_gangsta` (`:6421`): the z roll to premultiply.
    fn bgun_update_gangsta(&mut self, h: usize, pos: &mut Vec3) -> Mat4 {
        let func = self.func_of(h);
        let lv240 = self.lv.lvupdate240;
        let lv60 = self.lv.lvupdate60freal;
        let gangsta = self.b.ctrl.gangsta;
        let hand = &mut self.b.hands[h];
        let state_ok = matches!(hand.state, HANDSTATE_IDLE | HANDSTATE_2 | HANDSTATE_ATTACKEMPTY | HANDSTATE_ATTACK);
        let shoots = func.as_ref().is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_SHOOT);
        if gangsta && shoots && state_ok {
            if hand.gangstarot < 1.0 {
                hand.ispare1 += lv240;
                if hand.ispare1 > 60 {
                    hand.gangstarot += lv60 / 30.0;
                    if hand.gangstarot > 1.0 {
                        hand.gangstarot = 1.0;
                    }
                }
            } else {
                hand.ispare1 = 0;
            }
        } else {
            let inversespeed = if hand.animmode == HANDANIMMODE_BUSY { 15.0 } else { 30.0 };
            if hand.gangstarot > 0.0 {
                let mut revert = false;
                hand.ispare1 += lv240;
                if hand.gangstarot < 1.0 {
                    hand.ispare1 = 244;
                }
                if hand.ispare1 > 120 {
                    revert = true;
                }
                if hand.animmode == HANDANIMMODE_BUSY && func.as_ref().is_some_and(|f| f.kind() != INVENTORYFUNCTYPE_SHOOT) {
                    revert = true;
                }
                if !state_ok {
                    revert = true;
                }
                if revert {
                    hand.gangstarot -= lv60 / inversespeed;
                }
                if hand.gangstarot < 0.0 {
                    hand.gangstarot = 0.0;
                }
            } else {
                hand.ispare1 = 0;
            }
        }
        let tmp = -(hand.gangstarot * dtor(180.0)).cos() * 0.5 + 0.5;
        let side = if h != HAND_RIGHT { 1.0 } else { -1.0 };
        let z = (tmp * 66.6 * 0.017_453_292) * side;
        pos.y += 4.0 * hand.gangstarot;
        pos.x += 2.0 * hand.gangstarot * side;
        math::load_rotation(Vec3::new(0.0, 0.0, z))
    }

    /// `bgun_update_reaper` (`:6740`): the barrels' spin and the joint
    /// callback's angle.
    fn bgun_update_reaper(&mut self, h: usize) {
        let lv60 = self.lv.lvupdate60freal;
        let lv240 = self.lv.lvupdate240;
        let hand = &mut self.b.hands[h];
        // mm_reaperspeedaim = matmot2, mm_reaperspeedcur = matmot3, mm_reaperrot = matmot1
        if hand.matmot3 <= hand.matmot2 {
            if hand.matmot2 < 0.0 {
                hand.matmot2 += 0.01 * lv60;
                if hand.matmot2 > 0.0 {
                    hand.matmot2 = 0.0;
                }
            }
            hand.matmot3 = hand.matmot2;
        } else {
            let mut f12 = lv60 * (1.0 / 200.0);
            if hand.matmot2 < 0.000_000_1 {
                hand.matmot2 = -0.14;
                if hand.matmot3 < 0.15 {
                    f12 *= 4.0;
                }
            }
            let mut f2 = hand.matmot3 - hand.matmot2;
            if f12 < f2 {
                f2 = f12;
            }
            hand.matmot3 -= f2;
        }
        let spin = (1.0 - (hand.matmot3 * dtor(180.0)).cos()) * 0.5 * lv60 * 0.2;
        if hand.matmot3 < 0.0 {
            hand.matmot1 -= spin;
        } else {
            hand.matmot1 += spin;
        }
        // PD's literal 3.14159, not M_PI.
        #[allow(clippy::approx_constant)]
        const PD_PI: f32 = 3.141_59;
        let tmp = (hand.matmot1 / (PD_PI * 2.0)) as i32;
        hand.matmot1 -= tmp as f32 * (PD_PI * 2.0);
        let rot = hand.matmot1;
        let start = !hand.audiohandle && hand.matmot3 > 0.1 && lv240 != 0;
        let stop = hand.audiohandle && hand.matmot3 < 0.1;
        self.b.reaper_rot = rot;
        if start {
            self.b.hands[h].audiohandle = true;
            self.b.events.push(GunEvent::Sound { id: 0x805e, speed: 1.0, handle: Some(h) });
        }
        if stop {
            self.b.hands[h].audiohandle = false;
            self.b.events.push(GunEvent::StopSound { hand: h });
        }
    }

    /// The matrix slot of a part on the hand's gun model.
    fn part_mtx(&self, h: usize, partnum: i32) -> Option<usize> {
        let m = self.b.hands[h].gunmodel.as_ref()?;
        let node = m.def.get_part(partnum)?;
        m.def.find_node_mtx_index(node, 0)
    }

    /// `bgun0f0a4e44` (`:7142`): orient and scale this tick's muzzle flash.
    fn bgun_orient_flash(&mut self, h: usize, maxburst: usize, muzzle_slot: usize, arg9: &Mat4, func: Option<&FuncDef>) {
        let weaponnum = self.b.hands[h].weaponnum;
        let muzzlez = self.gset.weapon(weaponnum).map_or(1.0, |w| w.muzzlez);
        let mut index = (self.b.hands[h].burstbullets as usize) % maxburst.max(1);
        let mut shotstotake = self.b.hands[h].shotstotake;
        let spb4 = self.rng.randomfrac() * 0.25 + 1.0;
        if func.is_some_and(|f| f.flags & FUNCFLAG_00000001 != 0) {
            let _ = self.rng.randomfrac(); // the overwritten random roll
        }
        let mut spd8 = math::load_z_rotation((self.rng.randomfrac() as f64 * 0.3 - 0.15) as f32);
        let aimpos = self.b.hands[h].aimpos;
        let Some(model) = self.b.hands[h].gunmodel.as_mut() else { return };
        spd8 = model.matrices[muzzle_slot] * spd8;
        math::scale3(&mut spd8, spb4);
        math::scale_col2(&mut spd8, muzzlez);
        model.matrices[muzzle_slot] = spd8;
        if shotstotake == 0 && weaponnum != WEAPON_REAPER {
            shotstotake += 1;
        }
        let mut on = [false; 3];
        for _ in 0..shotstotake {
            on[index] = true;
            index += 1;
            if index >= maxburst {
                index = 0;
            }
        }
        let toggles = self.b.hands[h].flash_toggles.clone();
        for (i, &node) in toggles.iter().enumerate().take(maxburst) {
            if on[i] {
                if let Some(m) = self.b.hands[h].gunmodel.as_mut() {
                    m.vis[node] = true;
                }
            }
        }
        if weaponnum == WEAPON_REAPER || weaponnum == WEAPON_SHOTGUN {
            return;
        }
        for partnum in 0x50..=0x52 {
            let (rodata_pos, slot) = {
                let Some(m) = self.b.hands[h].gunmodel.as_ref() else { return };
                let Some(node) = m.def.get_part(partnum) else { continue };
                let NodeKind::Position { pos, mtx, .. } = m.def.nodes[node].kind else { continue };
                (pos, mtx[0] as usize)
            };
            let sp60 = spd8.transform_point3(rodata_pos);
            let roll = self.rng.randomfrac() * baddtor(360.0);
            let mut sp70 = math::mtx4_align(roll, -sp60.x, -sp60.y, -sp60.z);
            math::scale3(&mut sp70, 0.100_000_01 * spb4);
            let m = self.b.hands[h].gunmodel.as_mut().unwrap();
            let root = m.matrices[0].w_axis.truncate();
            let d = root - aimpos;
            let arg10 = math::mtx00016e98(0.0, d.x, d.y, d.z);
            sp70 = arg10 * sp70;
            math::scale_row2(&mut sp70, muzzlez);
            sp70 = *arg9 * sp70;
            math::set_translation(&mut sp70, sp60);
            m.matrices[slot] = sp70;
        }
    }

    /// `bgun_create_fx` (`:7234`): the casing and the beam of a fired weapon.
    fn bgun_create_fx(&mut self, h: usize, weaponnum: u8) {
        self.b.ctrl.throwing = false;
        let func = self.func_of(h);
        if let Some(f) = &func {
            if weaponnum != WEAPON_DY357MAGNUM && weaponnum != WEAPON_DY357LX && self.b.hands[h].gunmodel.is_some() {
                let partnum = if weaponnum == WEAPON_REAPER {
                    if self.b.hands[h].burstbullets & 1 == 1 {
                        MODELPART_REAPER_CARTEJECTPOS1
                    } else {
                        MODELPART_REAPER_CARTEJECTPOS2
                    }
                } else {
                    MODELPART_GUN_CARTEJECTPOS
                };
                let casing = self.gset.weapon(weaponnum).and_then(|w| if f.ammoindex >= 0 { w.ammos[f.ammoindex as usize].as_ref() } else { None }).map_or(-1, |a| a.casingeject);
                let mtx = match self.part_mtx(h, partnum) {
                    Some(slot) => {
                        let mut m = self.b.hands[h].gunmodel.as_ref().unwrap().matrices[slot];
                        math::scale3(&mut m, 9.999_999);
                        self.cam.projection * m
                    }
                    None => self.b.hands[h].posmtx,
                };
                if casing >= 0 && f.kind() == INVENTORYFUNCTYPE_SHOOT {
                    self.b.events.push(GunEvent::Casing { hand: h, mtx, casing });
                }
                self.bgun_set_part_visible(h, MODELPART_GUN_CARTFLAPCLOSED, false);
                self.bgun_set_part_visible(h, MODELPART_GUN_CARTFLAPOPEN, true);
            }
            if f.ftype == INVENTORYFUNCTYPE_SHOOT_PROJECTILE || f.kind() == INVENTORYFUNCTYPE_THROW {
                self.b.events.push(GunEvent::UncloakTemporarily);
            }
        }
        let createbeam = match &func {
            Some(f) => !(f.kind() == INVENTORYFUNCTYPE_MELEE || f.ftype & INVENTORYFUNCTYPE_0200 != 0 || f.kind() == INVENTORYFUNCTYPE_SPECIAL || f.kind() == INVENTORYFUNCTYPE_THROW),
            None => true,
        };
        if createbeam
            && matches!(
                weaponnum,
                WEAPON_FALCON2
                    | WEAPON_FALCON2_SILENCER
                    | WEAPON_FALCON2_SCOPE
                    | WEAPON_MAGSEC4
                    | WEAPON_MAULER
                    | WEAPON_PHOENIX
                    | WEAPON_DY357MAGNUM
                    | WEAPON_DY357LX
                    | WEAPON_CMP150
                    | WEAPON_CYCLONE
                    | WEAPON_CALLISTO
                    | WEAPON_RCP120
                    | WEAPON_LAPTOPGUN
                    | WEAPON_DRAGON
                    | WEAPON_K7AVENGER
                    | WEAPON_AR34
                    | WEAPON_SUPERDRAGON
                    | WEAPON_REAPER
                    | WEAPON_SNIPERRIFLE
                    | WEAPON_FARSIGHT
                    | WEAPON_TRANQUILIZER
                    | WEAPON_LASER
            )
        {
            self.b.events.push(GunEvent::Beam { hand: h });
            self.b.hands[h].numfires += 1;
        }
    }

    /// `bgun_update_smoke` (`:6517`).
    fn bgun_update_smoke(&mut self, h: usize, weaponnum: u8) {
        let func = self.func_of(h);
        let lv60 = self.lv.lvupdate60freal;
        let dual = self.b.hands[HAND_LEFT].inuse;
        let hand = &mut self.b.hands[h];
        if hand.firing {
            if weaponnum == WEAPON_DY357MAGNUM || weaponnum == WEAPON_DY357LX {
                if func.as_ref().is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_SHOOT) {
                    hand.gunsmokepoint += 0.6;
                }
            } else {
                hand.gunsmokepoint += 0.2;
            }
        }
        hand.gunsmokepoint -= lv60 / 120.0;
        if hand.gunsmokepoint < 0.0 {
            hand.gunsmokepoint = 0.0;
        }
        if func.as_ref().is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_SHOOT) {
            let mult = if dual { 1.5 } else { 1.0 };
            hand.forcecreatesmoke = false;
            match weaponnum {
                WEAPON_FALCON2 | WEAPON_FALCON2_SCOPE => {
                    if hand.gunsmokepoint * mult > 0.66 {
                        hand.createsmoke = true;
                    }
                }
                WEAPON_MAGSEC4 | WEAPON_MAULER => {
                    if hand.gunsmokepoint * mult > 0.75 {
                        hand.createsmoke = true;
                    }
                }
                WEAPON_DY357MAGNUM | WEAPON_DY357LX => {
                    if hand.gunsmokepoint * mult > 0.9 {
                        hand.createsmoke = true;
                    }
                }
                WEAPON_CMP150 | WEAPON_DRAGON | WEAPON_K7AVENGER | WEAPON_AR34 | WEAPON_SUPERDRAGON => {
                    hand.forcecreatesmoke = true;
                    if hand.burstbullets > 14 {
                        hand.createsmoke = true;
                    }
                }
                WEAPON_CYCLONE | WEAPON_LAPTOPGUN => {
                    if hand.burstbullets > 20 {
                        hand.createsmoke = true;
                    }
                    hand.forcecreatesmoke = true;
                }
                WEAPON_RCP120 => {
                    hand.forcecreatesmoke = true;
                    if hand.burstbullets > 25 {
                        hand.createsmoke = true;
                    }
                }
                WEAPON_REAPER | WEAPON_SHOTGUN => {
                    if weaponnum == WEAPON_REAPER {
                        hand.forcecreatesmoke = true;
                    }
                    if hand.firing {
                        hand.createsmoke = true;
                    }
                }
                _ => {}
            }
        }
        if hand.createsmoke && (hand.state != HANDSTATE_ATTACK || hand.forcecreatesmoke) {
            let ty = match weaponnum {
                WEAPON_FALCON2 | WEAPON_FALCON2_SCOPE | WEAPON_MAGSEC4 | WEAPON_MAULER | WEAPON_DY357MAGNUM | WEAPON_DY357LX => SMOKETYPE_MUZZLE_PISTOL,
                WEAPON_REAPER => SMOKETYPE_MUZZLE_REAPER,
                WEAPON_SHOTGUN => SMOKETYPE_MUZZLE_SHOTGUN,
                _ => SMOKETYPE_MUZZLE_AUTOMATIC,
            };
            // The world clears `createsmoke` once smoke_create_for_hand
            // succeeds (`bondgun.c:6631`).
            hand.gunsmokepoint = 0.0;
            let pos = hand.muzzlepos;
            self.b.events.push(GunEvent::Smoke { hand: h, pos, ty });
        }
    }

    // ─── bgun0f0a5550: the per-hand pose (7336-7885) ─────────────────────────

    /// `bgun0f0a5550` (`:7336`): place the hand's gun for this frame, tick its
    /// animation, apply the model tweaks, find the muzzle, and queue the fx.
    fn bgun_pose_hand(&mut self, h: usize) {
        let weaponnum = self.bgun_get_weapon_num(h);
        let Some(weapondef) = self.gset.weapon(weaponnum).cloned() else {
            self.b.hands[h].visible = false;
            return;
        };
        let func = self.func_of(h);
        let shoot = func.as_ref().and_then(|f| f.shoot.clone());
        let lv60 = self.lv.lvupdate60freal;
        let lv240 = self.lv.lvupdate240;
        let cam = *self.cam;

        self.bgun_update_blend(h);

        let other_has_40 = self.gset.has_flag(self.bgun_get_weapon_num(1 - h), WEAPONFLAG_00000040);
        {
            let hand = &mut self.b.hands[h];
            if h == HAND_RIGHT {
                if other_has_40 {
                    hand.xshift = (hand.xshift + 2.0 * lv60 / 240.0).min(2.0);
                } else {
                    hand.xshift = (hand.xshift - 2.0 * lv60 / 240.0).max(0.0);
                }
            } else if other_has_40 {
                hand.xshift = (hand.xshift - 2.0 * lv60 / 240.0).max(-2.0);
            } else {
                hand.xshift = (hand.xshift + 2.0 * lv60 / 240.0).min(0.0);
            }
        }

        let xpos = self.gset_get_xpos(h);
        let hand = &self.b.hands[h];
        let mut sp274 = if h == HAND_RIGHT {
            Vec3::new(xpos + hand.damppos.x + hand.adjustpos.x, weapondef.posy + hand.damppos.y + hand.adjustpos.y, weapondef.posz + hand.damppos.z + hand.adjustpos.z)
        } else {
            Vec3::new(xpos + hand.damppos.x - hand.adjustpos.x, weapondef.posy + hand.damppos.y + hand.adjustpos.y, weapondef.posz + hand.damppos.z + hand.adjustpos.z)
        };
        sp274.y += self.pl.guncloseroffset * 5.0 / -90.0 * 50.0;
        sp274.z -= self.pl.guncloseroffset * 15.0 / -90.0 * 50.0;

        if self.b.hands[h].firing && lv240 != 0 {
            if let Some(r) = shoot.as_ref().and_then(|s| s.recoil) {
                let fm = self.b.hands[h].finalmult[0];
                sp274.x += (self.rng.randomfrac() - 0.5) * r.xrange * fm;
                sp274.y += (self.rng.randomfrac() - 0.5) * r.yrange * fm;
                sp274.z += (self.rng.randomfrac() - 0.5) * r.zrange * fm;
            }
        }

        // The gun follows the aim point (crosspos2) by guntrans side/up/down.
        let p = &self.b.p;
        let aim = &weapondef.aim;
        let fspare1 = (p.crosspos2[0] - cam.c_screenleft - cam.c_screenwidth * 0.5) * aim.guntransside / (cam.c_screenwidth * 0.5);
        let dy = p.crosspos2[1] - cam.c_screentop - cam.c_screenheight * 0.5;
        let fspare2 = if dy > 0.0 { dy * aim.guntransdown / (cam.c_screenheight * 0.5) } else { dy * aim.guntransup / (cam.c_screenheight * 0.5) };
        self.b.hands[h].fspare1 = fspare1;
        self.b.hands[h].fspare2 = fspare2;
        sp274.x += fspare1;
        sp274.y -= fspare2;

        let mode = self.b.hands[h].mode;
        let visible = self.gset.has_flag(weaponnum, WEAPONFLAG_00000040)
            && !self.gset.has_flag(weaponnum, WEAPONFLAG_00000080)
            && mode != HANDMODE_6
            && mode != HANDMODE_7
            && self.bgun_is_loaded()
            && self.b.hands[h].inuse
            && self.b.ctrl.gunmemtype != WEAPON_NONE
            && self.b.hands[h].gunmodel.is_some();
        self.b.hands[h].visible = visible;

        if visible {
            // bgun_execute_model_cmd_list: every toggle back to visible.
            if let Some(m) = self.b.hands[h].gunmodel.as_mut() {
                m.reset_toggles();
            }
            if let Some(m) = self.b.hands[h].handmodel.as_mut() {
                m.reset_toggles();
            }
            self.bgun_update_ammo_visibility(h);
            if self.gset.has_flag(weaponnum, WEAPONFLAG_HASGUNSCRIPT) {
                self.bgun_tick_anim(h);
            }
        }

        let mut sp234 = Mat4::IDENTITY;
        if self.gset.has_flag(weaponnum, WEAPONFLAG_GANGSTA) {
            let roll = self.bgun_update_gangsta(h, &mut sp274);
            sp234 = roll * sp234;
        }
        if self.b.hands[h].useposrot {
            let pr = self.b.hands[h].posrotmtx;
            sp274 += pr.w_axis.truncate();
            sp234 = math::mul(&pr, &sp234);
            sp234.w_axis = Vec4::new(0.0, 0.0, 0.0, 1.0);
        } else {
            let hand = &mut self.b.hands[h];
            hand.rotxoffset = 0.0;
            hand.posoffset = Vec3::ZERO;
        }
        let (dl, du) = (self.b.hands[h].damplook, self.b.hands[h].dampup);
        let sp284 = math::look_at_basis(Vec3::ZERO, dl, du);
        sp234 = math::mul(&sp284, &sp234);

        let sp164 = math::load_rotation(Vec3::new(0.0, dtor(180.0), 0.0));
        let sp118 = cam.cam0f0b4c3c(self.b.hands[h].crosspos, 1.0) * 1000.0;
        let ang = |a0: f32, a1: f32, a2: f32, a3: f32| -> f32 {
            let a = a0 - a2;
            (a / (a * a + (a1 - a3) * (a1 - a3)).sqrt()).asin()
        };
        let sp1a4 = Vec3::new(ang(sp118.y, sp118.z, sp274.y, sp274.z), -ang(sp118.x, sp118.z, sp274.x, sp274.z), 0.0);
        self.b.hands[h].lastrotangx = sp1a4.x;
        self.b.hands[h].lastrotangy = sp1a4.y;
        let sp124 = math::load_rotation(sp1a4);
        let sp284 = sp124 * sp164;
        sp234 = sp284 * sp234;
        let mut rendermtx = sp234;
        math::set_translation(&mut rendermtx, sp274);

        self.b.hands[h].cammtx = rendermtx;
        self.b.hands[h].prevmtx = self.b.hands[h].posmtx;
        self.b.hands[h].posmtx = math::mul(&cam.projection, &rendermtx);

        if visible {
            // The flash toggles 0x5a..0x5c, hidden unless this tick fires.
            let mut flash_toggles = Vec::new();
            if let Some(m) = self.b.hands[h].gunmodel.as_ref() {
                for j in MODELPART_GUN_MUZZLEFLASH1..=MODELPART_GUN_MUZZLEFLASH3 {
                    if let Some(node) = m.def.get_part(j) {
                        flash_toggles.push(node);
                    }
                }
            }
            self.b.hands[h].flash_toggles = flash_toggles.clone();

            self.b.hands[h].dualflip = self.gset.has_flag(weaponnum, WEAPONFLAG_DUALFLIP) && h == HAND_LEFT;
            if self.b.hands[h].dualflip {
                math::scale_col0_xyz(&mut rendermtx, -1.0);
            }
            math::scale3(&mut rendermtx, 0.100_000_01);

            if weaponnum == WEAPON_REAPER {
                self.bgun_update_reaper(h);
            }

            // model_set_matrices_with_anim(&renderdata, &hand->gunmodel)
            let reaper = weaponnum == WEAPON_REAPER;
            let (spin_slot, cyl_slots) = if reaper {
                (self.part_mtx(h, MODELPART_REAPER_002C), [self.part_mtx(h, MODELPART_REAPER_002D), self.part_mtx(h, MODELPART_REAPER_002E), self.part_mtx(h, MODELPART_REAPER_002F)])
            } else {
                (None, [None; 3])
            };
            let rot = self.b.reaper_rot;
            let params = PoseParams { rendermtx, bank: self.bank, playercount: self.pl.playercount, lod_scale: Some(cam.c_lodscalez) };
            let hand = &mut self.b.hands[h];
            let anim = hand.anim.clone();
            if let Some(model) = hand.gunmodel.as_mut() {
                // bgun0f0a256c: the Reaper's spinning barrels.
                let mut cb = |slot: usize, m: &mut Mat4| {
                    if Some(slot) == spin_slot {
                        *m *= math::load_rotation(Vec3::new(0.0, 0.0, rot));
                    }
                    if cyl_slots.contains(&Some(slot)) {
                        *m *= math::load_rotation(Vec3::new(0.0, 0.0, 2.0 * -rot));
                    }
                };
                let jf: Option<pd_core::model::JointFn> = if reaper { Some(&mut cb) } else { None };
                model.set_matrices_with_anim(&params, Some(&anim), jf);
            }

            // The slide (MODELPART_GUN_SLIDE) runs back along its own -z.
            if let Some(slot) = self.part_mtx(h, MODELPART_GUN_SLIDE) {
                self.bgun_update_slide(h);
                let t = self.b.hands[h].slidetrans;
                let m = &mut self.b.hands[h].gunmodel.as_mut().unwrap().matrices[slot];
                let v = math::rotate(m, Vec3::new(0.0, 0.0, -t));
                m.w_axis += v.extend(0.0);
            }

            for &node in &flash_toggles {
                if let Some(m) = self.b.hands[h].gunmodel.as_mut() {
                    m.vis[node] = false;
                }
            }

            match weaponnum {
                WEAPON_SNIPERRIFLE => self.bgun_update_sniper_rifle(h),
                WEAPON_DEVASTATOR => self.bgun_update_devastator(h),
                WEAPON_SHOTGUN => self.bgun_update_shotgun(h),
                _ => {}
            }

            let mut muzzle = self.part_mtx(h, MODELPART_GUN_MUZZLEPOS);
            if weaponnum == WEAPON_REAPER {
                let k = if self.b.hands[h].flashon || self.b.hands[h].firing { self.b.hands[h].burstbullets % 3 } else { self.lv.lvframenum % 3 };
                muzzle = self.part_mtx(h, MODELPART_REAPER_001E + k);
            }
            if let Some(slot) = muzzle {
                let m = self.b.hands[h].gunmodel.as_ref().unwrap().matrices[slot];
                self.b.hands[h].muzzlemat = m;
                self.b.hands[h].muzzlepos = math::transform(&cam.projection, m.w_axis.truncate());
                self.b.hands[h].muzzlez = -m.w_axis.z;
                if self.b.hands[h].flashon && !flash_toggles.is_empty() && weaponnum != WEAPON_SHOTGUN && lv240 != 0 {
                    let f = self.func_of(h);
                    self.bgun_orient_flash(h, flash_toggles.len(), slot, &sp234, f.as_ref());
                }
            } else if let Some(slot) = self.part_mtx(h, MODELPART_GUN_HOLDPOS) {
                let m = self.b.hands[h].gunmodel.as_ref().unwrap().matrices[slot];
                self.b.hands[h].muzzlemat = m;
                self.b.hands[h].muzzlepos = math::transform(&cam.projection, m.w_axis.truncate());
                self.b.hands[h].muzzlez = -m.w_axis.z;
            } else {
                let pm = self.b.hands[h].posmtx;
                self.b.hands[h].muzzlepos = pm.w_axis.truncate();
                self.b.hands[h].muzzlemat = pm;
                self.b.hands[h].muzzlez = -self.b.hands[h].cammtx.w_axis.z;
            }
        } else {
            let pm = self.b.hands[h].posmtx;
            self.b.hands[h].muzzlepos = pm.w_axis.truncate();
            self.b.hands[h].muzzlemat = pm;
            self.b.hands[h].muzzlez = -self.b.hands[h].cammtx.w_axis.z;
        }

        if weaponnum == WEAPON_ROCKETLAUNCHER {
            self.b.events.push(GunEvent::UpdateRocketLauncher { hand: h });
        }

        if self.b.hands[h].firing && lv240 != 0 {
            self.bgun_create_fx(h, weaponnum);
        }
        if lv240 != 0 {
            self.bgun_update_smoke(h, weaponnum);
        }
        self.b.hands[h].animframeinc = 0;
    }

    /// `bgun_update_sniper_rifle` (`:6842`): the scope telescopes with the zoom.
    fn bgun_update_sniper_rifle(&mut self, h: usize) {
        let f26 = 1.0 - (self.gset_get_gun_zoom_fov() - 2.0) / 58.0;
        for i in 0..4 {
            let Some(slot) = self.part_mtx(h, MODELPART_SNIPERRIFLE_SCOPE1 + i) else { continue };
            let f20 = f26 * 4.0;
            let mut v = f20 - i as f32;
            if f20 < i as f32 {
                v = 0.0;
            }
            v *= 100.0;
            let m = &mut self.b.hands[h].gunmodel.as_mut().unwrap().matrices[slot];
            let d = math::rotate(m, Vec3::new(0.0, 0.0, v));
            m.w_axis += d.extend(0.0);
        }
    }

    /// `bgun_update_devastator` (`:6886`).
    fn bgun_update_devastator(&mut self, h: usize) {
        let Some(slot) = self.part_mtx(h, MODELPART_DEVASTATOR_0028) else { return };
        let lv60 = self.lv.lvupdate60freal;
        let hand = &mut self.b.hands[h];
        hand.loadslide = (hand.loadslide + 0.01 * lv60).min(1.0);
        let x = hand.loadslide * -10.0 * 1.636;
        let m = &mut hand.gunmodel.as_mut().unwrap().matrices[slot];
        let d = math::rotate(m, Vec3::new(x, 0.0, 0.0));
        m.w_axis += d.extend(0.0);
    }

    /// `bgun_update_shotgun` (`:6919`): the starburst on the blast.
    fn bgun_update_shotgun(&mut self, h: usize) {
        let lv60 = self.lv.lvupdate60freal;
        let slot = self.part_mtx(h, MODELPART_SHOTGUN_0050);
        let hand = &mut self.b.hands[h];
        if hand.flashon {
            hand.matmot1 = 1.0;
        }
        if hand.matmot1 > 0.0 {
            hand.matmot1 -= lv60 / 6.0;
            if hand.matmot1 < 0.01 {
                hand.matmot1 = 0.0;
            }
        }
        if hand.matmot1 > 0.0 {
            if let Some(&node) = hand.flash_toggles.first() {
                if let Some(m) = hand.gunmodel.as_mut() {
                    m.vis[node] = true;
                }
            }
            if let Some(slot) = slot {
                let f = hand.matmot1;
                let m = &mut hand.gunmodel.as_mut().unwrap().matrices[slot];
                math::scale_col2(m, (1.0 - f) * 8.0 + 0.5);
                math::scale_col0(m, (1.0 - f) * 3.0 + 1.0);
                math::scale_col1(m, (1.0 - f) * 3.0 + 1.0);
            }
        }
    }

    fn gunzoomfov_index(&self) -> Option<usize> {
        match self.bgun_get_weapon_num(HAND_RIGHT) {
            WEAPON_SNIPERRIFLE => Some(0),
            WEAPON_FARSIGHT => Some(1),
            _ => None,
        }
    }

    /// `gset_zoom_out` (`gset.c:215`): widen by `1 + amount·0.1` a frame, the
    /// Farsight at half rate, up to 60°.
    pub fn gset_zoom_out(&mut self, fovpersec: f32) {
        let Some(i) = self.gunzoomfov_index() else { return };
        let mut amount = fovpersec * 0.25 * self.lv.lvupdate60freal;
        if self.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_FARSIGHT {
            amount *= 0.5;
        }
        self.b.p.gunzoomfovs[i] = (self.b.p.gunzoomfovs[i] * (1.0 + amount * 0.1)).min(60.0);
    }

    /// `gset_zoom_in` (`gset.c:250`): the same, narrowing, down to 2°.
    pub fn gset_zoom_in(&mut self, fovpersec: f32) {
        let Some(i) = self.gunzoomfov_index() else { return };
        let mut amount = fovpersec * 0.25 * self.lv.lvupdate60freal;
        if self.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_FARSIGHT {
            amount *= 0.5;
        }
        self.b.p.gunzoomfovs[i] = (self.b.p.gunzoomfovs[i] / (1.0 + amount * 0.1)).max(2.0);
    }

    /// `bgun_tick_gameplay2` (`:7964`): the load ticking and both hands' poses.
    /// The world runs `hands_tick_attack` just before it.
    pub fn bgun_tick_gameplay2(&mut self) {
        self.bgun_tick_load();
        if self.b.ctrl.weaponnum == WEAPON_MAULER {
            self.bgun_tick_mauler_charge();
        }
        let lv60 = self.lv.lvupdate60 as u16;
        for i in 0..2 {
            for s in self.b.hands[i].gunroundsspent.iter_mut() {
                *s = s.saturating_sub(lv60);
            }
        }
        self.bgun_pose_hand(HAND_RIGHT);
        if self.b.hands[HAND_LEFT].inuse {
            self.bgun_pose_hand(HAND_LEFT);
        } else {
            self.b.hands[HAND_LEFT].ejectstate = EJECTSTATE_INACTIVE;
            self.b.hands[HAND_LEFT].visible = false;
        }
    }
}
