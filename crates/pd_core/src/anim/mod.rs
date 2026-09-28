//! PD's animation system: the bank decoder ([`AnimBank`], `lib/anim.c`) and
//! `struct anim` with the model-level animation functions of `lib/model.c`:
//! frame advance by whole or quarter ticks (`model_tick_anim`,
//! `model_tick_anim_quarter_speed`), lazy wrap and clamp
//! (`model_constrain_or_wrap_anim_frame`), loop windows, the merge from the
//! previous animation, speed and play-speed tweens, negative speed, and for a
//! model whose root is a `CHRINFO` node the clip's root motion
//! (`model_set_animation2`, `model_set_anim_frame2_with_chr_stuff`,
//! `model_update_chr_info`), including `ANIMFLAG_ABSOLUTETRANSLATION`.
//!
//! One `struct anim` for every model: the first-person gun (root `POSITION`,
//! never merging), the head-bob model (`g_PlayerModeldef`, whose root motion is
//! PD's walk displacement and head bob) and every chr body.
//!
//! Not yet ported: `anim->flip` in the decoder (the mirrored read of
//! `anim_get_rot_translate_scale`, through the skeleton's flip table). Guns and
//! the head model never flip; chrs do (M6).
//!
//! Target build: **NTSC final** (`VERSION_NTSC_FINAL` = 2 < `VERSION_PAL_BETA`), so
//! every `#if VERSION >= VERSION_PAL_BETA` block is the `#else` branch here.
//!
//! Source: the old repo's `pd_guns/anim.rs` + `animdata.rs`.

use glam::Vec3;

use crate::math::{baddtor, tween_rot_axis};

mod bank;
#[cfg(test)]
pub(crate) mod tests;

pub use bank::{AnimBank, AnimData, AnimMeta, ANIMFLAG_ABSOLUTETRANSLATION, ANIMFLAG_HASREPEATFRAMES, ANIMFLAG_LOOP};

/// `bg_get_stage_translation_thing()` (`bg.c:2058`): `unk1c / unk14` of the
/// stage's `g_Stages` row, which scales an `ANIMFLAG_ABSOLUTETRANSLATION`
/// animation's translation. 100 / 1 on every MP arena (`stagetable.c`).
pub const STAGE_TRANSLATION: f32 = 100.0;

/// `struct anim` (the fields these models use).
#[derive(Clone, Debug)]
pub struct Anim {
    pub animnum: u16,
    pub frame: f32,
    pub framea: i32,
    pub frameb: i32,
    pub frac: f32,
    pub speed: f32,
    pub endframe: f32,
    pub looping: bool,
    pub loopframe: f32,
    pub loopmerge: f32,
    pub playspeed: f32,
    pub timeplay: f32,
    pub elapseplay: f32,
    pub oldplay: f32,
    pub newplay: f32,
    pub timespeed: f32,
    pub elapsespeed: f32,
    pub oldspeed: f32,
    pub newspeed: f32,
    pub animscale: f32,
    /// `anim->average`: never set by these models.
    pub average: bool,
    /// `anim->flip`: toggled by `bhead_flip_animation` on each head-bob loop.
    pub flip: bool,

    pub animnum2: u16,
    pub frame2: f32,
    pub frame2a: i32,
    pub frame2b: i32,
    pub frac2: f32,
    pub speed2: f32,
    pub endframe2: f32,
    pub timespeed2: f32,
    pub elapsespeed2: f32,
    pub oldspeed2: f32,
    pub newspeed2: f32,
    pub flip2: bool,

    pub timemerge: f32,
    pub elapsemerge: f32,
    pub fracmerge: f32,
    /// Whether `anim->flipfunc` is set (`bhead_flip_animation`).
    pub flipfunc: bool,
}

impl Default for Anim {
    /// `anim_init` (`model.c:4251`).
    fn default() -> Self {
        Anim {
            animnum: 0,
            frame: 0.0,
            framea: 0,
            frameb: 0,
            frac: 0.0,
            speed: 1.0,
            endframe: -1.0,
            looping: false,
            loopframe: 0.0,
            loopmerge: 0.0,
            playspeed: 1.0,
            timeplay: 0.0,
            elapseplay: 0.0,
            oldplay: 0.0,
            newplay: 0.0,
            timespeed: 0.0,
            elapsespeed: 0.0,
            oldspeed: 0.0,
            newspeed: 0.0,
            animscale: 1.0,
            average: false,
            flip: false,
            animnum2: 0,
            frame2: 0.0,
            frame2a: 0,
            frame2b: 0,
            frac2: 0.0,
            speed2: 1.0,
            endframe2: -1.0,
            timespeed2: 0.0,
            elapsespeed2: 0.0,
            oldspeed2: 0.0,
            newspeed2: 0.0,
            flip2: false,
            timemerge: 0.0,
            elapsemerge: 0.0,
            fracmerge: 0.0,
            flipfunc: false,
        }
    }
}

/// `struct modelrwdata_chrinfo`: the CHRINFO root node's runtime state.
#[derive(Clone, Debug, Default)]
pub struct ChrInfo {
    pub unk00: bool,
    pub unk01: bool,
    pub unk02: bool,
    pub ground: f32,
    pub pos: Vec3,
    pub yrot: f32,
    pub unk18: f32,
    pub unk1c: f32,
    pub unk20: f32,
    pub unk24: Vec3,
    pub unk30: f32,
    pub unk34: Vec3,
    pub unk40: Vec3,
    pub unk4c: Vec3,
    pub unk58: f32,
    pub unk5c: f32,
}

/// What a model contributes to animation bookkeeping.
pub struct AnimCtx<'a> {
    pub bank: &'a AnimBank,
    /// `model->scale`.
    pub scale: f32,
    /// The CHRINFO root, if the model's root node is one, with its anim part.
    pub chrinfo: Option<(&'a mut ChrInfo, usize)>,
    /// `g_ModelAnimMergingEnabled`.
    pub merging_enabled: bool,
}

/// `model_constrain_or_wrap_anim_frame` (`model.c:1712`).
pub fn constrain_or_wrap(bank: &AnimBank, frame: i32, animnum: u16, endframe: f32) -> i32 {
    let n = bank.num_frames(animnum);
    let looped = bank.flags(animnum) & ANIMFLAG_LOOP != 0;
    if frame < 0 {
        if looped {
            n - (-frame % n.max(1))
        } else {
            0
        }
    } else if endframe >= 0.0 && frame > endframe as i32 {
        endframe.ceil() as i32
    } else if frame >= n {
        if looped {
            frame % n.max(1)
        } else {
            n - 1
        }
    } else {
        frame
    }
}

impl Anim {
    /// `model_set_anim_frame` (`model.c:2086`).
    pub fn set_frame(&mut self, bank: &AnimBank, frame: f32) {
        let framea = frame.floor() as i32;
        let forwards = self.speed >= 0.0;
        let frameb = if forwards { framea + 1 } else { framea - 1 };
        self.framea = constrain_or_wrap(bank, framea, self.animnum, self.endframe);
        self.frameb = constrain_or_wrap(bank, frameb, self.animnum, self.endframe);
        if self.framea == self.frameb {
            self.frac = 0.0;
            self.frame = self.framea as f32;
        } else if forwards {
            self.frac = frame - framea as f32;
            self.frame = self.framea as f32 + self.frac;
        } else {
            self.frac = 1.0 - (frame - frameb as f32);
            self.frame = self.frameb as f32 + (1.0 - self.frac);
        }
    }

    /// `model_set_anim_frame2` (`model.c:2115`).
    pub fn set_frame2(&mut self, bank: &AnimBank, frame1: f32, frame2: f32) {
        self.set_frame(bank, frame1);
        if self.animnum2 != 0 {
            let framea = frame2.floor() as i32;
            let forwards = self.speed2 >= 0.0;
            let frameb = if forwards { framea + 1 } else { framea - 1 };
            self.frame2a = constrain_or_wrap(bank, framea, self.animnum2, self.endframe2);
            self.frame2b = constrain_or_wrap(bank, frameb, self.animnum2, self.endframe2);
            if self.frame2a == self.frame2b {
                self.frac2 = 0.0;
                self.frame2 = self.frame2a as f32;
            } else if forwards {
                self.frac2 = frame2 - framea as f32;
                self.frame2 = self.frame2a as f32 + self.frac2;
            } else {
                self.frac2 = 1.0 - (frame2 - frameb as f32);
                self.frame2 = self.frame2b as f32 + (1.0 - self.frac2);
            }
        }
    }

    /// `model_copy_anim_for_merge` (`model.c:1733`).
    fn copy_for_merge(&mut self, merge: f32) {
        if merge > 0.0 && self.animnum != 0 {
            if self.animnum2 != 0 && self.fracmerge == 1.0 {
                return;
            }
            self.frame2 = self.frame;
            self.frac2 = self.frac;
            self.animnum2 = self.animnum;
            self.flip2 = self.flip;
            self.frame2a = self.framea;
            self.frame2b = self.frameb;
            self.speed2 = self.speed;
            self.newspeed2 = self.newspeed;
            self.oldspeed2 = self.oldspeed;
            self.timespeed2 = self.timespeed;
            self.elapsespeed2 = self.elapsespeed;
            self.endframe2 = self.endframe;
        } else {
            self.animnum2 = 0;
        }
    }

    /// `model_set_animation` (`model.c:1967`) + `model_set_animation2` (`:1777`).
    pub fn set_animation(&mut self, ctx: &mut AnimCtx, animnum: u16, flip: bool, startframe: f32, speed: f32, merge: f32) {
        let bank = ctx.bank;
        let mut merge = merge;
        if self.animnum != 0
            && bank.flags(self.animnum) & ANIMFLAG_ABSOLUTETRANSLATION != 0
            && bank.flags(animnum) & ANIMFLAG_ABSOLUTETRANSLATION == 0
        {
            merge = 0.0;
        }
        self.copy_for_merge(merge);

        let isfirstanim = self.animnum == 0;
        if self.animnum2 != 0 {
            self.timemerge = merge;
            self.elapsemerge = 0.0;
            self.fracmerge = 1.0;
        } else {
            self.timemerge = 0.0;
            self.fracmerge = 0.0;
        }
        self.animnum = animnum;
        self.flip = flip;
        self.endframe = -1.0;
        self.speed = speed;
        self.timespeed = 0.0;
        self.set_frame(bank, startframe);
        self.looping = false;

        // The CHRINFO root's bookkeeping (model.c:1807).
        if let Some((ci, animpart)) = ctx.chrinfo.as_mut() {
            let Some(ad) = bank.get(self.animnum) else { return };
            if ad.flags & ANIMFLAG_ABSOLUTETRANSLATION != 0 {
                // model.c:1828
                let sp64 = STAGE_TRANSLATION;
                ci.unk34 = ad.rot_translate_scale(*animpart, self.framea).1 * sp64;
                ci.unk30 = ci.yrot;
                if self.frac == 0.0 {
                    ci.unk01 = false;
                } else {
                    ci.unk24 = ad.rot_translate_scale(*animpart, self.frameb).1 * sp64;
                    ci.unk20 = ci.yrot;
                    ci.unk01 = true;
                }
                return;
            }
            let (mut translate, sp84) = ad.translate_angle(*animpart, self.frameb);
            let scale = ctx.scale * self.animscale;
            if scale != 1.0 {
                translate *= scale;
            }
            if self.average {
                translate.y = ci.pos.y - ci.ground;
            }
            let sp98 = ci.yrot.cos();
            let sp94 = ci.yrot.sin();
            if self.frac == 0.0 {
                ci.unk34 = Vec3::new(ci.pos.x, ci.pos.y - ci.ground, ci.pos.z);
                ci.unk30 = ci.yrot;
                ci.unk24 = Vec3::new(
                    ci.unk34.x + translate.x * sp98 + translate.z * sp94,
                    translate.y,
                    ci.unk34.z - translate.x * sp94 + translate.z * sp98,
                );
                if ci.unk18 == 0.0 {
                    ci.unk20 = ci.unk30 + sp84;
                    if ci.unk20 >= baddtor(360.0) {
                        ci.unk20 -= baddtor(360.0);
                    }
                }
                ci.unk01 = true;
            } else {
                let x = translate.x * sp98 + translate.z * sp94;
                let y = translate.y;
                let z = -translate.x * sp94 + translate.z * sp98;
                ci.unk24 = Vec3::new(ci.pos.x + x * (1.0 - self.frac), y, ci.pos.z + z * (1.0 - self.frac));
                ci.unk34.x = ci.unk24.x - x;
                ci.unk34.y = (ci.pos.y - ci.ground) - (y - (ci.pos.y - ci.ground)) * self.frac / (1.0 - self.frac);
                ci.unk34.z = ci.unk24.z - z;
                let mut angle = ci.yrot - sp84;
                if angle < 0.0 {
                    angle += baddtor(360.0);
                }
                ci.unk30 = tween_rot_axis(ci.yrot, angle, self.frac);
                if ci.unk18 == 0.0 {
                    ci.unk20 = ci.unk30 + sp84;
                    if ci.unk20 >= baddtor(360.0) {
                        ci.unk20 -= baddtor(360.0);
                    }
                }
                ci.unk01 = true;
            }
            if isfirstanim {
                ci.unk34.y = ci.unk24.y;
            }
        }
    }

    /// `model_set_anim_looping` (`model.c:1988`).
    pub fn set_looping(&mut self, loopframe: f32, loopmerge: f32) {
        self.looping = true;
        self.loopframe = loopframe;
        self.loopmerge = loopmerge;
    }

    /// `model_set_anim_end_frame` (`model.c:1997`).
    pub fn set_end_frame(&mut self, bank: &AnimBank, endframe: f32) {
        self.endframe = if self.animnum != 0 && endframe < bank.num_frames(self.animnum) as f32 - 1.0 {
            endframe
        } else {
            -1.0
        };
    }

    /// `model_set_anim_speed` (`model.c:2026`).
    pub fn set_speed(&mut self, speed: f32, startframe: f32) {
        if startframe > 0.0 {
            self.timespeed = startframe;
            self.newspeed = speed;
            self.elapsespeed = 0.0;
            self.oldspeed = self.speed;
        } else {
            self.speed = speed;
            self.timespeed = 0.0;
        }
    }

    /// `model_set_anim_play_speed` (`model.c:2062`).
    pub fn set_play_speed(&mut self, speed: f32, startframe: f32) {
        if startframe > 0.0 {
            self.timeplay = startframe;
            self.newplay = speed;
            self.elapseplay = 0.0;
            self.oldplay = self.playspeed;
        } else {
            self.playspeed = speed;
            self.timeplay = 0.0;
        }
    }

    /// `model_get_num_anim_frames`.
    pub fn num_frames(&self, bank: &AnimBank) -> i32 {
        if self.animnum == 0 {
            0
        } else {
            bank.num_frames(self.animnum)
        }
    }

    /// `model_get_cur_anim_frame`.
    pub fn cur_frame(&self) -> f32 {
        if self.animnum == 0 {
            0.0
        } else {
            self.frame
        }
    }

    /// `model_get_abs_anim_speed` (`model.c:1682`).
    pub fn abs_speed(&self) -> f32 {
        let s = self.speed.abs();
        s * self.playspeed
    }

    /// The per-sub-tick body shared by `model_tick_anim` (`model.c:2659`, step 1)
    /// and `model_tick_anim_quarter_speed` (`:2515`, step 0.25).
    fn tick_inner(&mut self, ctx: &mut AnimCtx, ticks: i32, step: f32, arg2: bool) {
        if self.animnum == 0 && self.animnum2 == 0 {
            // `if (anim && ...)` — PD still runs with animnum 0 but nothing moves.
        }
        if ticks <= 0 {
            return;
        }
        let bank = ctx.bank;
        let mut frame = self.frame;
        let mut frame2 = self.frame2;
        for _ in 0..ticks {
            if self.timeplay > 0.0 {
                self.elapseplay += step;
                if self.elapseplay < self.timeplay {
                    self.playspeed = self.oldplay + (self.newplay - self.oldplay) * self.elapseplay / self.timeplay;
                } else {
                    self.timeplay = 0.0;
                    self.playspeed = self.newplay;
                }
            }
            if self.timemerge > 0.0 {
                self.elapsemerge += self.playspeed * step;
                if self.elapsemerge == 0.0 {
                    self.fracmerge = 1.0;
                } else if self.elapsemerge < self.timemerge {
                    self.fracmerge = (self.timemerge - self.elapsemerge) / self.timemerge;
                } else {
                    self.timemerge = 0.0;
                    self.fracmerge = 0.0;
                    self.animnum2 = 0;
                }
            }
            if self.timespeed > 0.0 {
                self.elapsespeed += self.playspeed * step;
                if self.elapsespeed < self.timespeed {
                    self.speed = self.oldspeed + (self.newspeed - self.oldspeed) * self.elapsespeed / self.timespeed;
                } else {
                    self.timespeed = 0.0;
                    self.speed = self.newspeed;
                }
            }
            let speed = self.speed;
            frame += self.playspeed * speed * step;

            if self.animnum2 != 0 {
                if self.timespeed2 > 0.0 {
                    self.elapsespeed2 += self.playspeed * step;
                    if self.elapsespeed2 < self.timespeed2 {
                        self.speed2 =
                            self.oldspeed2 + (self.newspeed2 - self.oldspeed2) * self.elapsespeed2 / self.timespeed2;
                    } else {
                        self.timespeed2 = 0.0;
                        self.speed2 = self.newspeed2;
                    }
                }
                frame2 += self.playspeed * self.speed2 * step;
            }

            if self.looping {
                let realendframe = self.endframe;
                let n = bank.num_frames(self.animnum) as f32;
                let (startframe, endframe) = if speed >= 0.0 {
                    let mut end = n - 1.0;
                    if realendframe >= 0.0 && end > realendframe {
                        end = realendframe;
                    }
                    (self.loopframe, end)
                } else {
                    let mut start = n - 1.0;
                    if realendframe >= 0.0 && start > realendframe {
                        start = realendframe;
                    }
                    (start, self.loopframe)
                };
                if (speed >= 0.0 && frame >= endframe) || (speed < 0.0 && frame <= endframe) {
                    let (pn, po, pt, pe) = (self.newspeed, self.oldspeed, self.timespeed, self.elapsespeed);
                    if arg2 {
                        let cur = self.frame;
                        self.set_frame2_with_chr_stuff(ctx, cur, endframe, 0.0, 0.0);
                    } else {
                        self.set_frame2(bank, endframe, 0.0);
                    }
                    let (animnum, flip, cur_speed, loopmerge) = (self.animnum, self.flip, self.speed, self.loopmerge);
                    self.set_animation(ctx, animnum, flip, startframe, cur_speed, loopmerge);
                    self.looping = true;
                    self.endframe = realendframe;
                    self.newspeed = pn;
                    self.oldspeed = po;
                    self.timespeed = pt;
                    self.elapsespeed = pe;
                    frame2 = frame;
                    frame = startframe + frame - endframe;
                    if self.flipfunc {
                        // bhead_flip_animation (bondhead.c:19)
                        self.flip = !self.flip;
                    }
                }
            }
        }
        if arg2 {
            let cur = self.frame;
            let cur2 = self.frame2;
            if self.animnum2 != 0 {
                self.set_frame2_with_chr_stuff(ctx, cur, frame, cur2, frame2);
            } else {
                self.set_frame2_with_chr_stuff(ctx, cur, frame, 0.0, 0.0);
            }
        } else if self.animnum2 != 0 {
            self.set_frame2(bank, frame, frame2);
        } else {
            self.set_frame2(bank, frame, 0.0);
        }
    }

    /// `model_tick_anim` (NTSC, `model.c:2659`): `lvupdate` whole frames at speed.
    pub fn tick(&mut self, ctx: &mut AnimCtx, lvupdate: i32, arg2: bool) {
        self.tick_inner(ctx, lvupdate, 1.0, arg2);
    }

    /// `model_tick_anim_quarter_speed` (`model.c:2515`): `lvupdate240` quarter frames.
    pub fn tick_quarter(&mut self, ctx: &mut AnimCtx, lvupdate240: i32, arg2: bool) {
        self.tick_inner(ctx, lvupdate240, 0.25, arg2);
    }

    /// `model_set_anim_frame2_with_chr_stuff` (`model.c:2158`): walk every whole
    /// frame crossed between `curframe` and `endframe`, accumulating the clip's
    /// root motion into the CHRINFO's `unk34` (current) / `unk24` (next)
    /// positions; an `ABSOLUTETRANSLATION` clip instead reads its position
    /// straight from the frames either side of `endframe`.
    pub fn set_frame2_with_chr_stuff(&mut self, ctx: &mut AnimCtx, curframe: f32, endframe: f32, curframe2: f32, endframe2: f32) {
        let bank = ctx.bank;
        let merging_enabled = ctx.merging_enabled;
        let scale_model = ctx.scale;
        let Some((ci, animpart)) = ctx.chrinfo.as_mut() else {
            self.set_frame2(bank, endframe, endframe2);
            return;
        };
        let animpart = *animpart;
        if ci.unk00 {
            self.set_frame2(bank, endframe, endframe2);
            return;
        }
        let absolute = bank.flags(self.animnum) & ANIMFLAG_ABSOLUTETRANSLATION != 0;
        let Some(ad) = bank.get(self.animnum) else {
            self.set_frame2(bank, endframe, endframe2);
            return;
        };
        let scale = scale_model * self.animscale;
        let mut spe0 = ci.unk34;
        let mut f30 = ci.unk30;
        let mut spd0 = ci.unk24;
        let mut spcc = ci.unk20;
        let mut spc8 = ci.unk01;
        let absspeed = self.speed.abs();
        let absspeed2 = self.speed2.abs();
        let forwards = curframe <= endframe;
        let (mut floorcur, floorend) = if forwards {
            (curframe.floor() as i32 + 1, endframe.floor() as i32)
        } else {
            (curframe.ceil() as i32 - 1, endframe.ceil() as i32)
        };
        if absolute {
            // model.c:2235
            let f20 = STAGE_TRANSLATION;
            if floorend != self.framea {
                let s0frame = constrain_or_wrap(bank, floorend, self.animnum, self.endframe);
                self.framea = s0frame;
                if spc8 && floorend == self.frameb {
                    spe0 = spd0;
                } else {
                    spe0 = ad.rot_translate_scale(animpart, s0frame).1 * f20;
                }
                floorcur = if forwards { floorend + 1 } else { floorend - 1 };
                let s0frame = constrain_or_wrap(bank, floorcur, self.animnum, self.endframe);
                self.frameb = s0frame;
                spc8 = true;
                spd0 = ad.rot_translate_scale(animpart, s0frame).1 * f20;
            }
        }
        // The relative path (model.c:2289): every whole frame crossed.
        loop {
            if absolute {
                break;
            }
            if forwards {
                if floorend < floorcur {
                    break;
                }
            } else if floorend > floorcur {
                break;
            }
            let s0frame = constrain_or_wrap(bank, floorcur, self.animnum, self.endframe);
            self.framea = s0frame;
            if spc8 {
                spe0 = spd0;
                if ci.unk18 == 0.0 {
                    f30 = spcc;
                }
            } else {
                let (mut translate, mut f22) = ad.translate_angle(animpart, s0frame);
                if scale != 1.0 {
                    translate *= scale;
                }
                if !forwards {
                    translate.x = -translate.x;
                    translate.z = -translate.z;
                    if f22 > 0.0 {
                        f22 = baddtor(360.0) - f22;
                    }
                }
                if self.average {
                    translate.y = ci.pos.y - ci.ground;
                }
                let c = ci.yrot.cos();
                let s = ci.yrot.sin();
                spe0.x += translate.x * c + translate.z * s;
                spe0.y = translate.y;
                spe0.z += -translate.x * s + translate.z * c;
                if ci.unk18 == 0.0 {
                    f30 += f22;
                    if f30 >= baddtor(360.0) {
                        f30 -= baddtor(360.0);
                    }
                }
            }
            if forwards {
                floorcur += 1;
            } else {
                floorcur -= 1;
            }
            let s0frame = constrain_or_wrap(bank, floorcur, self.animnum, self.endframe);
            self.frameb = s0frame;
            if self.frameb != self.framea {
                let (mut translate, mut f22) = ad.translate_angle(animpart, s0frame);
                spc8 = true;
                if scale != 1.0 {
                    translate *= scale;
                }
                if !forwards {
                    translate.x = -translate.x;
                    translate.z = -translate.z;
                    if f22 > 0.0 {
                        f22 = baddtor(360.0) - f22;
                    }
                }
                if self.average {
                    translate.y = ci.unk34.y;
                }
                let c = ci.unk30.cos();
                let s = ci.unk30.sin();
                if merging_enabled && self.animnum2 != 0 {
                    spd0.x = translate.x * c + translate.z * s;
                    spd0.z = -translate.x * s + translate.z * c;
                    if absspeed > 0.0 {
                        let mut f0 = self.fracmerge - self.playspeed / (absspeed * self.timemerge);
                        if f0 < 0.0 {
                            f0 = 0.0;
                        }
                        f0 = (f0 + self.fracmerge) / 2.0;
                        let sp90x = (ci.unk40.x - ci.unk4c.x) * absspeed2 / absspeed;
                        let sp90z = (ci.unk40.z - ci.unk4c.z) * absspeed2 / absspeed;
                        spd0.x += (sp90x - spd0.x) * f0;
                        spd0.z += (sp90z - spd0.z) * f0;
                    } else {
                        spd0.x += (ci.unk40.x - ci.unk4c.x) * self.fracmerge;
                        spd0.z += (ci.unk40.z - ci.unk4c.z) * self.fracmerge;
                    }
                    spd0.x += spe0.x;
                    spd0.z += spe0.z;
                    spd0.y = translate.y;
                } else {
                    spd0.x = spe0.x + translate.x * c + translate.z * s;
                    spd0.y = translate.y;
                    spd0.z = spe0.z - translate.x * s + translate.z * c;
                }
                if ci.unk5c > 0.0 && absspeed > 0.0 {
                    let mut increment = 1.0 / absspeed;
                    if increment > ci.unk5c {
                        increment = ci.unk5c;
                        ci.unk5c = 0.0;
                    } else {
                        ci.unk5c -= increment;
                    }
                    f22 += ci.unk58 * increment;
                    if f22 < 0.0 {
                        f22 += baddtor(360.0);
                    } else if f22 >= baddtor(360.0) {
                        f22 -= baddtor(360.0);
                    }
                }
                if ci.unk18 == 0.0 {
                    spcc = f30 + f22;
                    if spcc >= baddtor(360.0) {
                        spcc -= baddtor(360.0);
                    }
                }
            }
        }
        ci.unk34 = spe0;
        ci.unk30 = f30;
        ci.unk24 = spd0;
        ci.unk20 = spcc;
        ci.unk01 = spc8;

        if self.framea == self.frameb {
            self.frac = 0.0;
            self.frame = self.framea as f32;
        } else if forwards {
            self.frac = endframe - floorend as f32;
            self.frame = self.framea as f32 + self.frac;
        } else {
            self.frac = floorend as f32 - endframe;
            self.frame = self.frameb as f32 + (1.0 - self.frac);
        }

        if self.animnum2 != 0 && !absolute {
            let floorcur2 = curframe2.floor() as i32;
            let floorend2 = endframe2.floor() as i32;
            if (forwards && floorcur2 < floorend2) || (!forwards && floorend2 < floorcur2) {
                if ci.unk02 {
                    ci.unk4c.y = ci.unk40.y;
                } else {
                    ci.unk4c.y = ci.unk34.y;
                }
                self.frame2a = constrain_or_wrap(bank, floorend2, self.animnum2, self.endframe2);
                let s0frame = constrain_or_wrap(bank, floorend2 + 1, self.animnum2, self.endframe2);
                self.frame2b = s0frame;
                let mut ty = bank.get(self.animnum2).map_or(0.0, |a| a.translate_angle(animpart, s0frame).0.y);
                if scale != 1.0 {
                    ty *= scale;
                }
                if self.average {
                    ty = ci.unk4c.y;
                }
                ci.unk40.y = ty;
                ci.unk02 = true;
            }
            if forwards {
                self.frac2 = endframe2 - floorend2 as f32;
                self.frame2 = self.frame2a as f32 + self.frac2;
            } else {
                self.frac2 = 1.0 - (endframe2 - floorend2 as f32);
                self.frame2 = self.frame2b as f32 + (1.0 - self.frac2);
            }
        } else {
            ci.unk02 = false;
        }
    }
}

/// `model_update_chr_info` (`model.c:629`): where the CHRINFO root sits this frame.
pub fn update_chr_info(anim: &Anim, ci: &mut ChrInfo) {
    if ci.unk00 {
        return;
    }
    let mut sp34 = ci.unk34;
    ci.yrot = ci.unk30;
    let frac = anim.frac;
    if frac != 0.0 && ci.unk01 {
        sp34 += (ci.unk24 - sp34) * frac;
        ci.yrot = tween_rot_axis(ci.unk30, ci.unk20, frac);
    }
    if (anim.animnum2 != 0 || anim.fracmerge != 0.0) && ci.unk02 {
        let mut y = ci.unk4c.y;
        if anim.frac2 != 0.0 {
            y += (ci.unk40.y - y) * anim.frac2;
        }
        sp34.y += (y - sp34.y) * anim.fracmerge;
    }
    ci.pos = Vec3::new(sp34.x, ci.ground + sp34.y, sp34.z);
}
