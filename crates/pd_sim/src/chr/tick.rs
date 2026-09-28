//! `chr_tick` (`chr.c:2375`) for a simulant, once per player pass: on the full
//! tick (the first player's) the action (`chra_tick`: the go-to, the death,
//! the corpse's fade, the footsteps), the animation and position
//! (`chr_update_anim` → `model_update_chr_info` → `chr_update_position`), the
//! aim's tween, then the pose, the flinch and the shots.
//!
//! `// SUBST:` PD ticks a chr in a room off every screen less often, in the
//! background (`props_tick_player`'s prop states) / every chr is in the
//! foreground every frame, as every room is on screen until M9's portals.
//!
//! Source: the old repo's `pd_spike/chraction.rs` (`chr_tick`, footsteps),
//! checked against `chr.c` and `footstep.c`.

use pd_core::anim::update_chr_info_with;

use super::Act;
use crate::world::World;

/// `g_FootstepAnims` (`footstep.c:34`): the animations with footfalls, and
/// the two frames the feet land on.
const FOOTSTEP_ANIMS: [(u16, f32, f32); 34] = [
    (0x002b, 8.0, 25.0),
    (0x0029, 5.0, 14.0),
    (0x006b, 8.0, 25.0),
    (0x0028, 27.0, 8.0),
    (0x002a, 18.0, 6.0),
    (0x0052, 8.0, 25.0),
    (0x0053, 25.0, 8.0),
    (0x0054, 25.0, 8.0),
    (0x0055, 7.0, 18.0),
    (0x0056, 7.0, 18.0),
    (0x0057, 18.0, 7.0),
    (0x0058, 15.0, 5.0),
    (0x0059, 8.0, 20.0),
    (0x005a, 6.0, 15.0),
    (0x006c, 25.0, 8.0),
    (0x006d, 25.0, 8.0),
    (0x006e, 8.0, 19.0),
    (0x006f, 21.0, 8.0),
    (0x0070, 15.0, 5.0),
    (0x0071, 15.0, 5.0),
    (0x0072, 23.0, 8.0),
    (0x0073, 8.0, 19.0),
    (0x0093, 23.0, 10.0),
    (0x0094, 15.0, 5.0),
    (0x005f, 14.0, 1.0),
    (0x0016, 29.0, 10.0),
    (0x0018, 24.0, 46.0),
    (0x001b, 10.0, 28.0),
    (0x001d, 13.0, 2.0),
    (0x001e, 12.0, 1.0),
    (0x005c, 19.0, 42.0),
    (0x005d, 15.0, 5.0),
    (0x005e, 4.0, 12.0),
    (0x0392, 5.0, 20.0),
];

/// `footstep_is_running` (`footstep.c:70`).
fn footstep_is_running(animnum: u16) -> bool {
    matches!(animnum, 0x1d | 0x1e | 0x29 | 0x2a | 0x55 | 0x56 | 0x57 | 0x58 | 0x59 | 0x5a | 0x5d | 0x5e | 0x5f | 0x6e | 0x6f | 0x70 | 0x71 | 0x73 | 0x93 | 0x94)
}

impl super::Chr {
    /// `model_get_anim_end_frame`.
    pub fn anim_end_frame(&self, bank: &pd_core::anim::AnimBank) -> f32 {
        if self.anim.endframe >= 0.0 {
            self.anim.endframe
        } else {
            (self.anim.num_frames(bank) - 1) as f32
        }
    }
}

impl World {
    /// `chr_tick` for a simulant; `fulltick` on the frame's first player pass.
    pub(crate) fn chr_tick(&mut self, i: usize, fulltick: bool) {
        if fulltick {
            // chr_update_cloak: a simulant's cloak is M11's; chr_tick_poisoned: solo.
            self.chra_tick(i);
        }
        // Which animations advance (`chr.c:2466`): a go-to's, a standing chr's in
        // multiplayer, a dying one's; a dead one lies still.
        let act = self.chrs[i].actiontype;
        if fulltick && act != Act::Dead {
            self.chr_update_anim(i);
        }
        let p0 = self.players.first().map(|p| (p.cam, p.cam.pos(), p.cam.c_lodscalez));
        let modelscale = self.chrs[i].effective_scale();
        let pos = self.chrs[i].pos;
        // needsupdate: on the current (the first) player's screen.
        let mut needsupdate = p0.as_ref().is_some_and(|(cam, _, _)| super::body::pos_is_onscreen(cam, pos, modelscale));
        if fulltick {
            let lv = (self.lv.lvupdate60f, self.lv.lvupdate60);
            self.chrs[i].chr_tween_aim(lv.0, lv.1);
        }
        if pos.y < -65536.0 {
            needsupdate = false;
        }
        let anyscreen = needsupdate || self.players.iter().skip(1).any(|p| super::body::pos_is_onscreen(&p.cam, pos, modelscale));
        {
            let c = &mut self.chrs[i];
            c.onscreen = needsupdate;
            c.onanyscreenprev = c.onanyscreen;
            c.onanyscreen = anyscreen;
        }
        if needsupdate && fulltick && self.chrs[i].flinchcnt >= 0 {
            let lv60 = self.lv.lvupdate60;
            let c = &mut self.chrs[i];
            c.flinchcnt += lv60;
            if c.flinchcnt >= 30 {
                c.flinchcnt = -1;
            }
        }
        if anyscreen {
            // SUBST: PD poses a chr in each viewing player's eye space
            // (`model_set_matrices_with_anim`, `chr.c:2791`) / once, in world
            // space, when any player sees it; its far-LOD frame snapping is
            // judged from the first player's camera.
            let (campos, lodscalez) = p0.map_or((pos, 1.0), |(_, c, l)| (c, l));
            let res = self.res.clone();
            let slowmo = self.setup.slowmotion() != pd_core::lv::SlowMotion::Off;
            let playercount = self.players.len();
            self.chrs[i].pose(&res.bank, playercount, campos, lodscalez, slowmo);
        }
        if fulltick {
            self.chr_tick_shots(i);
        }
    }

    /// `chra_tick` (`chraction.c:13316`), a simulant's: its sleep, its action,
    /// the footsteps. (Its AI list only keeps `maxdamage` at 8.)
    fn chra_tick(&mut self, i: usize) {
        if self.lv.lvupdate240 < 1 {
            return;
        }
        let lv60 = self.lv.lvupdate60;
        let dead = self.chr_is_dead(i);
        let c = &mut self.chrs[i];
        c.sleep -= lv60;
        if c.sleep < 0 || dead {
            c.sleep = 0;
            match c.actiontype {
                Act::GoPos => self.chr_tick_gopos(i),
                Act::Die => self.chr_tick_die(i),
                Act::Dead => self.chr_tick_dead(i),
                Act::Stand | Act::BondMulti => {}
            }
            self.footstep_check_default(i);
        } else {
            self.footstep_check_magic(i);
        }
    }

    /// `chr_update_anim(chr, lvupdate240, true)` (`chr.c:1953`): the root before
    /// (`chr->prevpos`), the animation's quarter ticks, then
    /// `model_update_info` with the position callback.
    fn chr_update_anim(&mut self, i: usize) {
        let res = self.res.clone();
        let lv240 = self.lv.lvupdate240;
        {
            let c = &mut self.chrs[i];
            c.prevpos = c.model.chrinfo.pos;
            let mut ctx = c.model.anim_ctx(&res.bank);
            c.anim.tick_quarter(&mut ctx, lv240, true);
        }
        let anim = self.chrs[i].anim.clone();
        let mut ci = std::mem::take(&mut self.chrs[i].model.chrinfo);
        let mut forced = false;
        {
            let mut cb = |arg1: glam::Vec3, arg2: &mut glam::Vec3, ground: &mut f32| {
                forced = self.chr_update_position(i, arg1, arg2, ground);
                true
            };
            update_chr_info_with(&anim, &mut ci, Some(&mut cb));
        }
        if forced {
            // chr.c:880: the root motion's next height becomes the current one.
            ci.unk34.y = ci.unk24.y;
        }
        self.chrs[i].model.chrinfo = ci;
    }

    /// `footstep_check_default` (`footstep.c:155`): with one human playing, a
    /// chr whose animation is in `g_FootstepAnims` steps when its frame passes
    /// either footfall frame.
    fn footstep_check_default(&mut self, i: usize) {
        if self.players.len() != 1 {
            return;
        }
        let c = &mut self.chrs[i];
        c.footstep = 0;
        c.magicanim = None;
        c.magicframe = 0.0;
        let frame = c.anim.frame;
        let prevframe = c.oldframe;
        c.oldframe = frame;
        let animnum = c.anim.animnum;
        let Some(index) = FOOTSTEP_ANIMS.iter().position(|a| a.0 == animnum) else { return };
        let (_, f1, f2) = FOOTSTEP_ANIMS[index];
        if frame >= f1 && prevframe < f1 {
            c.footstep = 1;
        } else if frame >= f2 && prevframe < f2 {
            c.footstep = 2;
        }
        self.footstep_play(i, index);
        let c = &mut self.chrs[i];
        c.magicanim = Some(index);
        c.magicspeed = c.anim.speed * 0.25;
    }

    /// `footstep_check_magic` (`footstep.c:218`): a sleeping chr keeps stepping
    /// on its last walk's rhythm while the player is within 17 m.
    fn footstep_check_magic(&mut self, i: usize) {
        if self.players.len() != 1 {
            return;
        }
        let Some(index) = self.chrs[i].magicanim else { return };
        let lv240 = self.lv.lvupdate240 as f32;
        let playerpos = self.players[0].pos;
        let c = &mut self.chrs[i];
        c.magicframe += lv240 * c.magicspeed;
        let xdiff = playerpos.x - c.pos.x;
        let ydiff = (playerpos.y - c.pos.y).abs();
        let zdiff = playerpos.z - c.pos.z;
        let frame = c.magicframe;
        let prevframe = c.oldframe;
        c.footstep = 0;
        c.oldframe = frame;
        if ydiff < 250.0 && xdiff * xdiff + zdiff * zdiff < 3_000_000.0 {
            let (_, f1, f2) = FOOTSTEP_ANIMS[index];
            if frame >= f1 && prevframe < f1 {
                c.footstep = 1;
            } else if frame >= f2 && prevframe < f2 {
                c.footstep = 2;
            }
            self.footstep_play(i, index);
        }
        let c = &mut self.chrs[i];
        let wrap = if footstep_is_running(FOOTSTEP_ANIMS[index].0) { 22.0 } else { 34.0 };
        if c.magicframe > wrap {
            c.magicframe -= wrap;
        }
    }

    /// `footstep_choose_sound` + its `ps_create` at the chr.
    fn footstep_play(&mut self, i: usize, index: usize) {
        if self.chrs[i].footstep == 0 {
            return;
        }
        let running = footstep_is_running(FOOTSTEP_ANIMS[index].0);
        let (floortype, mut last) = (self.chrs[i].floortype, self.chrs[i].lastfootsample);
        let sound = super::footstep_choose_sound(&mut self.rng, floortype, &mut last, running);
        self.chrs[i].lastfootsample = last;
        if sound != 0 {
            let pos = self.chrs[i].pos;
            self.sound_at(sound, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
        }
    }
}
