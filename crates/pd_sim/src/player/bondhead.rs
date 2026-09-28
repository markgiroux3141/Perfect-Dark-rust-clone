//! `bondhead.c` / `bondheadreset.c`: the head-bob model. `g_PlayerModeldef`
//! plays the walk (ANIM_002B) or run (ANIM_0029) clip at the speed the stick
//! asks for; its root motion, damped, is the head bob and the distance walked.

use glam::{Mat4, Vec3};
use pd_core::anim::{update_chr_info, AnimBank, AnimCtx, ChrInfo};
use pd_core::ids::CROUCHPOS_SQUAT;
use pd_core::lv::Lv;
use pd_core::model::PoseParams;
use pd_core::rng::Rng;

use super::{Player, HEADANIM_MOVING, HEADANIM_RESTING};

/// `struct headanim` (`types.h:4549`), with `translateperframe` measured at reset.
#[derive(Clone, Copy, Debug)]
pub(super) struct HeadAnim {
    pub animnum: u16,
    pub loopframe: f32,
    pub endframe: f32,
    pub translateperframe: f32,
    pub maxspeed: f32,
}

/// The head model's scale (`bhead_reset`: `model->scale = 0.1`).
const HEAD_SCALE: f32 = 0.100_000_01;

fn ctx<'a>(bank: &'a AnimBank, ci: &'a mut ChrInfo, merging: bool) -> AnimCtx<'a> {
    AnimCtx { bank, skel: 0x0b, scale: HEAD_SCALE, chrinfo: Some((ci, 0)), merging_enabled: merging }
}

impl Player {
    fn pose_head(&mut self) {
        let bank = self.bank.clone();
        let params = PoseParams { rendermtx: Mat4::IDENTITY, bank: &bank, playercount: self.playercount, lod_scale: Some(1.0) };
        self.head.set_matrices_with_anim(&params, Some(&self.head_anim), None);
    }

    /// `bhead_reset` (`bondheadreset.c:37`).
    pub(super) fn bhead_reset(&mut self, hold_anim: u16, rng: &mut Rng) {
        let bank = self.bank.clone();
        self.head.scale = HEAD_SCALE;
        self.head_anim = Default::default();
        self.head_anim.set_play_speed(1.0, 0.0);
        // translateperframe: the summed root-motion z over the loop window × 0.1.
        for ha in self.headanims.iter_mut() {
            let mut total = 0i32;
            if let Some(ad) = bank.get(ha.animnum) {
                let mut f = ha.loopframe as i32;
                while (f as f32) < ha.endframe {
                    total += ad.pos_angle_as_int(0, f).0[2] as i32;
                    f += 1;
                }
            }
            ha.translateperframe = (total as f32 * HEAD_SCALE) / (ha.endframe - ha.loopframe);
        }
        // standheight: measured off ANIM_TWO_GUN_HOLD (bondheadreset.c:117).
        {
            let mut ci = std::mem::take(&mut self.head.chrinfo);
            let mut c = ctx(&bank, &mut ci, true);
            self.head_anim.set_animation(&mut c, hold_anim, false, 0.0, 0.5, 0.0);
            update_chr_info(&self.head_anim, &mut ci);
            self.head.chrinfo = ci;
        }
        self.pose_head();
        self.standheight = self.head.matrices[0].w_axis.y;
        let ha = self.headanims[self.headanim as usize];
        {
            let mut ci = std::mem::take(&mut self.head.chrinfo);
            let mut c = ctx(&bank, &mut ci, true);
            self.head_anim.set_animation(&mut c, ha.animnum, false, ha.loopframe, 0.5, 0.0);
            self.head.chrinfo = ci;
        }
        self.head_anim.set_looping(ha.loopframe, 0.0);
        self.head_anim.set_end_frame(&bank, ha.endframe);
        self.head_anim.flipfunc = true;
        self.bhead_update_idle_roll(rng);
    }

    /// `bhead_start_death_animation` (`bondhead.c:297`).
    pub(super) fn bhead_start_death_animation(&mut self, animnum: u16, flip: bool, fstarttime: f32, speed: f32) {
        let bank = self.bank.clone();
        let mut ci = std::mem::take(&mut self.head.chrinfo);
        {
            let mut c = ctx(&bank, &mut ci, true);
            self.head_anim.set_animation(&mut c, animnum, flip, fstarttime, speed * 0.5, 12.0);
        }
        self.head.chrinfo = ci;
        self.headanim = -1;
    }

    /// `bhead_set_speed` (`bondhead.c:303`).
    pub(super) fn bhead_set_speed(&mut self, speed: f32) {
        self.head_anim.set_speed(speed * 0.5, 0.0);
    }

    /// The head model's frame and end frame (`player_render_hud`'s death test).
    pub(super) fn head_anim_done(&self) -> bool {
        let end = if self.head_anim.endframe >= 0.0 { self.head_anim.endframe } else { (self.head_anim.num_frames(&self.bank) - 1) as f32 };
        self.head_anim.cur_frame() >= end
    }

    /// `bhead_update_idle_roll` (`bondhead.c:24`).
    fn bhead_update_idle_roll(&mut self, rng: &mut Rng) {
        let r = [rng.randomfrac(), rng.randomfrac(), rng.randomfrac(), rng.randomfrac()];
        self.bhead_update_idle_roll_seeded(r[0], r[1], r[2], r[3]);
    }

    /// `bhead_update_idle_roll` with its four randoms, in PD's draw order.
    fn bhead_update_idle_roll_seeded(&mut self, r0: f32, r1: f32, r2: f32, r3: f32) {
        let c = self.standcnt;
        self.standlook[c] = Vec3::new((r0 - 0.5) * 0.02, 0.0, 1.0);
        self.standup[c] = Vec3::new((r1 - 0.5) * 0.02, 1.0, 0.0);
        if c != 0 {
            self.standlook[c].y = r2 * 0.01;
            self.standup[c].z = r3 * -0.01;
        } else {
            self.standlook[c].y = r2 * -0.01;
            self.standup[c].z = r3 * 0.01;
        }
        self.standcnt = 1 - self.standcnt;
    }

    /// `bhead_adjust_animation` (`bondhead.c:259`).
    pub(super) fn bhead_adjust_animation(&mut self, speed: f32) {
        let bank = self.bank.clone();
        let mut speed = speed * self.headanims[HEADANIM_MOVING as usize].translateperframe;
        for i in 0..2 {
            let ha = self.headanims[i];
            if ha.maxspeed * ha.translateperframe >= speed {
                let prev = self.headanim;
                if i as i32 != prev {
                    let mut startframe = 0.0;
                    if prev >= 0 {
                        let ph = self.headanims[prev as usize];
                        startframe = (self.head_anim.frame - ph.loopframe) / (ph.endframe - ph.loopframe);
                        startframe = ha.loopframe + (ha.endframe - ha.loopframe) * startframe;
                    }
                    let flip = self.head_anim.flip;
                    let mut ci = std::mem::take(&mut self.head.chrinfo);
                    let mut c = ctx(&bank, &mut ci, true);
                    self.head_anim.set_animation(&mut c, ha.animnum, flip, startframe, 0.5, 12.0);
                    self.head.chrinfo = ci;
                    self.head_anim.set_looping(ha.loopframe, 0.0);
                    self.head_anim.set_end_frame(&bank, ha.endframe);
                    self.head_anim.flipfunc = true;
                    self.headanim = i as i32;
                }
                speed /= ha.translateperframe;
                self.head_anim.set_speed(speed * 0.5, 0.0);
                break;
            }
        }
    }

    /// `bhead_set_damp` (`bondhead.c:113`).
    fn bhead_set_damp(&mut self, headdamp: f32) {
        if headdamp != self.headdamp {
            let divisor = 1.0 - headdamp;
            self.headlooksum = self.headlooksum * (1.0 - self.headdamp) / divisor;
            self.headupsum = self.headupsum * (1.0 - self.headdamp) / divisor;
            self.headdamp = headdamp;
        }
    }

    /// `bhead_update` (`bondhead.c:129`).
    pub(super) fn bhead_update(&mut self, speedforwards: f32, speedsideways: f32, lv: &Lv, rng: &mut Rng) {
        let bank = self.bank.clone();
        let bondbreathing = self.bondbreathing;
        let mut headpos = Vec3::ZERO;
        let mut lookvel = Vec3::new(0.0, 0.0, 1.0);
        let mut upvel = Vec3::new(0.0, 1.0, 0.0);
        let mut animspeed = 0.0;
        let mut m0 = Mat4::IDENTITY;
        if self.head_anim.animnum != 0 && bank.num_frames(self.head_anim.animnum) > 0 {
            animspeed = self.head_anim.abs_speed();
            if self.headanim == HEADANIM_RESTING {
                self.headamplitude = if animspeed > 0.7 {
                    1.0
                } else if animspeed > 0.1 {
                    0.4 + (animspeed - 0.1) * 0.6 / 0.6
                } else {
                    0.4
                };
                self.sideamplitude = self.headamplitude;
            } else if self.headanim == HEADANIM_MOVING {
                self.headamplitude = 0.9;
                self.sideamplitude = 0.5;
            } else {
                self.headamplitude = 1.0;
                self.sideamplitude = 1.0;
            }
            let mut ci = std::mem::take(&mut self.head.chrinfo);
            {
                let mut c = ctx(&bank, &mut ci, false);
                self.head_anim.tick_quarter(&mut c, lv.lvupdate240, true);
            }
            update_chr_info(&self.head_anim, &mut ci);
            self.head.chrinfo = ci;
            self.pose_head();
            m0 = self.head.matrices[0];
            // modelpos -= matrices[0].xz; model_set_root_position
            let mut modelpos = self.head.chrinfo.pos;
            modelpos.x -= m0.w_axis.x;
            modelpos.z -= m0.w_axis.z;
            let ci = &mut self.head.chrinfo;
            let diff = Vec3::new(modelpos.x - ci.pos.x, 0.0, modelpos.z - ci.pos.z);
            ci.pos = modelpos;
            ci.unk24 += diff;
            ci.unk34 += diff;
            ci.unk40 += diff;
            ci.unk4c += diff;
        }
        if animspeed > 0.0 {
            m0.w_axis.x += speedsideways;
            m0.w_axis.z *= speedforwards;
            if lv.lvupdate240 > 0 {
                m0.w_axis.x /= lv.lvupdate60freal;
                m0.w_axis.z /= lv.lvupdate60freal;
            }
            headpos.x = m0.w_axis.x * self.headamplitude;
            headpos.y = (m0.w_axis.y - self.standheight) * self.headamplitude + self.standheight;
            headpos.z = m0.w_axis.z * self.headamplitude;
            if self.headanim >= 0 {
                lookvel = Vec3::new(m0.z_axis.x * self.sideamplitude, m0.z_axis.y * self.headamplitude, (m0.z_axis.z - 1.0) * self.headamplitude + 1.0);
                upvel = Vec3::new(m0.y_axis.x * self.headamplitude, (m0.y_axis.y - 1.0) * self.headamplitude + 1.0, m0.y_axis.z * self.headamplitude);
                self.headwalkingtime60 += lv.lvupdate60;
                if self.headwalkingtime60 > 60 {
                    self.bhead_set_damp(0.982);
                } else {
                    self.bhead_set_damp(0.997_489_99);
                }
            } else {
                lookvel = m0.z_axis.truncate();
                upvel = m0.y_axis.truncate();
                self.bhead_set_damp(0.96);
            }
        } else {
            headpos = Vec3::new(0.0, self.standheight, 0.0);
            self.headwalkingtime60 = 0;
            self.bhead_set_damp(0.997_489_99);
            if self.crouchpos != CROUCHPOS_SQUAT {
                self.standfrac += (0.008_333_334 + 0.025_000_002 * bondbreathing) * lv.lvupdate60freal;
                if self.standfrac >= 1.0 {
                    self.bhead_update_idle_roll(rng);
                    self.standfrac -= 1.0;
                }
                let c = self.standcnt;
                lookvel = (self.standlook[1 - c] - self.standlook[c]) * self.standfrac + self.standlook[c];
                lookvel.x *= 1.0 + 5.0 * bondbreathing;
                lookvel.y *= 1.0 + 5.0 * bondbreathing;
                upvel = (self.standup[1 - c] - self.standup[c]) * self.standfrac + self.standup[c];
                upvel.x *= 1.0 + 5.0 * bondbreathing;
                upvel.z *= 1.0 + 5.0 * bondbreathing;
            }
        }
        // bhead_update_pos
        if self.resetheadpos {
            self.headpossum = Vec3::new(0.0, headpos.y / 0.018_000_006, 0.0);
            self.resetheadpos = false;
        }
        for _ in 0..lv.lvupdate240 {
            self.headpossum = headpos + 0.982 * self.headpossum;
        }
        self.headpos = self.headpossum * 0.018_000_006;
        // bhead_update_rot
        if self.resetheadrot {
            self.headlooksum = lookvel / (1.0 - self.headdamp);
            self.headupsum = upvel / (1.0 - self.headdamp);
            self.resetheadrot = false;
        }
        for _ in 0..lv.lvupdate240 {
            self.headlooksum = lookvel + self.headdamp * self.headlooksum;
            self.headupsum = upvel + self.headdamp * self.headupsum;
        }
        self.headlook = self.headlooksum * (1.0 - self.headdamp);
        self.headup = self.headupsum * (1.0 - self.headdamp);
    }

    /// `bhead_get_breathing_value` (`bondhead.c:308`): the gun sway's breathing
    /// input (`bgun_update_sway`).
    pub(super) fn bhead_get_breathing_value(&self) -> f32 {
        if self.headanim >= 0 {
            let a = self.bondbreathing * 0.012_500_001 + 1.0 / 240.0;
            let b = self.head_anim.abs_speed();
            if b > 0.0 {
                let ha = self.headanims[self.headanim as usize];
                let c = b / (ha.endframe - ha.loopframe);
                return c.max(a);
            }
            return a;
        }
        0.0
    }
}
