//! `bondgun.c`, first half: part visibility, the gun scripts, ammo, and the
//! idle / reload / autoswitch / change-function states, then firing
//! (`bgun0f09a3f8`, `bgun0f09a6f8`).

use pd_core::anim::AnimCtx;
use pd_core::ids::*;

use super::gset::{AmmoDef, FuncDef, GunCmd, ScriptId};
use super::{max_pitch, GunCtx, CmdPtr, NORMMPLAYERISRUNNING, SFXMAP_804F_RELOAD_DEFAULT};

impl GunCtx<'_> {
    // ─── visibility (339-445) ────────────────────────────────────────────────

    /// `bgun_set_part_visible` (`:364`): hand parts go to the hand model.
    pub fn bgun_set_part_visible(&mut self, h: usize, partnum: i32, visible: bool) {
        let hand = &mut self.b.hands[h];
        if partnum == MODELPART_HAND_LEFT || partnum == MODELPART_HAND_RIGHT {
            if let Some(m) = hand.handmodel.as_mut() {
                m.set_part_visible(partnum, visible);
            }
        } else if let Some(m) = hand.gunmodel.as_mut() {
            m.set_part_visible(partnum, visible);
        }
    }

    /// `bgun_execute_gun_vis_commands` (`:389`) with `bgun_test_gun_vis_command`.
    pub(crate) fn bgun_execute_gun_vis_commands(&mut self, h: usize) {
        let Some(w) = self.weapon(self.b.hands[h].weaponnum) else { return };
        let cmds = w.gunviscmds.clone();
        for cmd in cmds {
            let result = match cmd.ctype {
                4 => ((self.b.hands[h].upgradewant >> cmd.param) & 1) != 0,
                5 => h == HAND_LEFT,
                6 => h == HAND_RIGHT,
                _ => true,
            };
            if result {
                match cmd.op {
                    GUNVISOP_IFTRUE_SETVISIBLE | GUNVISOP_SETVISIBILITY => self.bgun_set_part_visible(h, cmd.partnum, true),
                    GUNVISOP_IFTRUE_SETHIDDEN => self.bgun_set_part_visible(h, cmd.partnum, false),
                    _ => {}
                }
            } else if cmd.op == GUNVISOP_SETVISIBILITY {
                self.bgun_set_part_visible(h, cmd.partnum, false);
            }
        }
    }

    /// `bgun_update_ammo_visibility` (`:425`).
    pub(crate) fn bgun_update_ammo_visibility(&mut self, h: usize) {
        self.bgun_execute_gun_vis_commands(h);
        self.bgun_set_part_visible(h, MODELPART_0042, false);
        let Some(w) = self.weapon(self.b.hands[h].weaponnum).cloned() else { return };
        for i in 0..2 {
            if let Some(a) = &w.ammos[i] {
                if a.flags & AMMOFLAG_QTYAFFECTSPARTVIS != 0 {
                    for j in 0..self.b.hands[h].clipsizes[i] {
                        let vis = j < self.b.hands[h].loadedammo[i];
                        self.bgun_set_part_visible(h, j + 100, vis);
                    }
                }
            }
        }
    }

    // ─── gun scripts (447-843) ───────────────────────────────────────────────

    pub(crate) fn anim_num_frames(&self, h: usize) -> i32 {
        self.b.hands[h].anim.num_frames(self.bank)
    }

    /// `bgun_get_current_keyframe` (`:447`).
    pub fn bgun_get_current_keyframe(&self, h: usize) -> f32 {
        let hand = &self.b.hands[h];
        if hand.animmode == HANDANIMMODE_BUSY {
            if let Some(p) = hand.animcmd {
                if let GunCmd::PlayAnimation { params, .. } = self.gset.cmd(p) {
                    if *params < 0 {
                        return self.anim_num_frames(h) as f32 - hand.anim.cur_frame();
                    }
                }
                return hand.anim.cur_frame();
            }
        }
        0.0
    }

    /// `bgun_tick_anim` (`:460`), NTSC (integer keyframes, full-speed ticking).
    pub(crate) fn bgun_tick_anim(&mut self, h: usize) {
        self.b.hands[h].ejectcount = 0;
        if self.b.hands[h].animmode == HANDANIMMODE_BUSY && self.bgun_get_current_keyframe(h) >= (self.anim_num_frames(h) - 1) as f32 {
            self.b.hands[h].animmode = HANDANIMMODE_IDLE;
        }
        if !(self.b.hands[h].animmode == HANDANIMMODE_BUSY || self.b.hands[h].animload >= 0) {
            return;
        }
        if self.b.hands[h].gangstarot > 0.0 {
            self.b.hands[h].animframeinc = 0;
        }
        if self.b.hands[h].animload >= 0 {
            let mut animspeedmult = 1.0;
            let params = match self.b.hands[h].animcmd.map(|p| self.gset.cmd(p).clone()) {
                Some(GunCmd::PlayAnimation { params, .. }) => params,
                _ => 10000,
            };
            let animspeed = params as f32 / 10000.0;
            if self.b.hands[h].unk0d0e_07 && self.b.hands[HAND_LEFT].inuse {
                animspeedmult = self.randomfrac() * 0.77 + 0.7;
            }
            let animload = self.b.hands[h].animload as u16;
            let bank = self.bank;
            let mut ctx = AnimCtx { bank, skel: 0, scale: 1.0, chrinfo: None, merging_enabled: true };
            let hand = &mut self.b.hands[h];
            hand.anim.set_animation(&mut ctx, animload, false, 0.0, animspeedmult * animspeed, 0.0);
            if hand.animcmd.is_some() && animspeed < 0.0 {
                let n = hand.anim.num_frames(bank) as f32;
                hand.anim.set_frame(bank, n);
            }
            hand.animload = -1;
            hand.animmode = HANDANIMMODE_BUSY;
        }
        if self.b.hands[h].unk0cc8_02 {
            self.b.hands[h].animframeinc = 0;
        }

        let mut oldkeyframe = self.bgun_get_current_keyframe(h) as i32;
        let mut newkeyframe = oldkeyframe + self.b.hands[h].animframeinc;
        if oldkeyframe == 0 && newkeyframe > 0 {
            oldkeyframe -= 1;
        }

        // Before the tick: part visibility, WAITFORZRELEASED, REPEATUNTILFULL.
        if let Some(start) = self.b.hands[h].animcmd {
            let mut parts: Vec<(i32, i32, bool)> = Vec::new(); // (partnum, keyframe, visible)
            let mut i = start.1;
            loop {
                let cmd = self.gset.cmd((start.0, i)).clone();
                match cmd {
                    GunCmd::End => break,
                    GunCmd::ShowPart { keyframe, part } | GunCmd::HidePart { keyframe, part } => {
                        if newkeyframe >= keyframe {
                            let show = matches!(cmd, GunCmd::ShowPart { .. });
                            match parts.iter_mut().find(|p| p.0 == part) {
                                Some(p) => {
                                    if keyframe > p.1 {
                                        p.1 = keyframe;
                                        p.2 = show;
                                    }
                                }
                                None => parts.push((part, keyframe, show)),
                            }
                        }
                    }
                    GunCmd::WaitForZReleased { keyframe } => {
                        if self.b.hands[h].unk0cc8_01 && newkeyframe >= keyframe && oldkeyframe < keyframe && oldkeyframe < newkeyframe {
                            let mut tmp = keyframe - self.bgun_get_current_keyframe(h) as i32;
                            tmp /= 2;
                            if self.b.hands[h].animframeinc > tmp {
                                self.b.hands[h].animframeinc = tmp;
                            }
                            newkeyframe = oldkeyframe + self.b.hands[h].animframeinc;
                        }
                    }
                    GunCmd::RepeatUntilFull { keyframe, gotokeyframe } => {
                        if self.b.hands[h].incrementalreloading && newkeyframe >= keyframe && oldkeyframe < keyframe && oldkeyframe < newkeyframe {
                            let foundkeyframe = gotokeyframe + ((newkeyframe - keyframe) % ((keyframe - gotokeyframe) + 1));
                            oldkeyframe = foundkeyframe;
                            self.b.hands[h].animframeinc = 0;
                            self.b.hands[h].anim.set_frame(self.bank, foundkeyframe as f32);
                            self.b.hands[h].animloopcount += 1;
                            newkeyframe = foundkeyframe;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            for (part, _, vis) in parts {
                self.bgun_set_part_visible(h, part, vis);
            }
        }

        // model_tick_anim(&hand->gunmodel, hand->animframeinc, true)
        {
            let mut ctx = AnimCtx { bank: self.bank, skel: 0, scale: 1.0, chrinfo: None, merging_enabled: true };
            let inc = self.b.hands[h].animframeinc;
            self.b.hands[h].anim.tick(&mut ctx, inc, true);
        }

        // After the tick: sounds, sound speed, casing ejection.
        let newkeyframe = self.bgun_get_current_keyframe(h) as i32;
        if let Some(start) = self.b.hands[h].animcmd {
            let mut speed = 1.0f32;
            let mut hasspeed = false;
            let mut i = start.1;
            loop {
                let cmd = self.gset.cmd((start.0, i)).clone();
                if cmd == GunCmd::End {
                    break;
                }
                let kf = cmd.keyframe();
                if newkeyframe >= kf && oldkeyframe < kf && oldkeyframe < newkeyframe {
                    match cmd {
                        GunCmd::PlaySound { sound, .. } => {
                            // snd_start_extra(..., cmd->soundnum, speed, ...)
                            self.sound(sound, if hasspeed { speed } else { 1.0 });
                            hasspeed = false;
                        }
                        GunCmd::SetSoundSpeed { speed: s, .. } => {
                            speed = s as f32 / 1000.0;
                            hasspeed = true;
                        }
                        GunCmd::PopOutSackOfPills { .. } => {
                            self.b.hands[h].ejectcount += 1;
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
        }
    }

    /// `bgun_test_condition` (`:709`).
    pub(crate) fn bgun_test_condition(&self, condition: u8, h: usize) -> bool {
        match condition {
            0 => true,
            1 => self.b.hands[HAND_LEFT].inuse,
            2 => self.b.hands[h].weaponfunc == FUNC_SECONDARY,
            _ => false,
        }
    }

    /// `bgun_start_animation` (`:728`).
    pub fn bgun_start_animation(&mut self, script: ScriptId, h: usize) {
        self.bgun_start_animation_at((script, 0), h);
    }

    pub(crate) fn bgun_start_animation_at(&mut self, cmd: CmdPtr, h: usize) {
        match self.gset.cmd(cmd).clone() {
            GunCmd::PlayAnimation { anim, .. } => {
                let hand = &mut self.b.hands[h];
                hand.animload = anim as i32;
                hand.animmode = HANDANIMMODE_IDLE;
                hand.unk0cc8_01 = false;
                hand.incrementalreloading = false;
                hand.animcmd = Some(cmd);
                hand.animloopcount = 0;
                hand.unk0cc8_02 = false;
                hand.unk0d0e_07 = false;
                hand.animcmd2 = Some(cmd);
            }
            _ => {
                let rand = self.rng.random() % 100;
                let mut done = false;
                let mut i = cmd.1;
                loop {
                    let c = self.gset.cmd((cmd.0, i)).clone();
                    if c == GunCmd::End {
                        break;
                    }
                    match c {
                        GunCmd::Include { condition, target } => {
                            if self.bgun_test_condition(condition, h) && !done && target != usize::MAX {
                                done = true;
                                self.bgun_start_animation_at((target, 0), h);
                            }
                        }
                        GunCmd::Random { probability, target } => {
                            if !done && target != usize::MAX && self.b.hands[h].animcmd2 != Some((target, 0)) && (rand as i32) < probability {
                                done = true;
                                self.bgun_start_animation_at((target, 0), h);
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
        }
    }

    /// `bgun_anim_allows_feature` (`:763`), NTSC integer compare.
    pub fn bgun_anim_allows_feature(&self, h: usize, feature: i32) -> bool {
        let hand = &self.b.hands[h];
        if hand.animmode == HANDANIMMODE_IDLE {
            return hand.animload == -1;
        }
        let Some(start) = hand.animcmd else { return true };
        let mut allowkeyframe = -1;
        let mut zreleasekeyframe = -1;
        let mut i = start.1;
        loop {
            let c = self.gset.cmd((start.0, i));
            if *c == GunCmd::End || allowkeyframe != -1 {
                break;
            }
            match *c {
                GunCmd::WaitForZReleased { keyframe } => zreleasekeyframe = keyframe,
                GunCmd::AllowFeature { keyframe, feature: f } if f == feature => allowkeyframe = keyframe,
                _ => {}
            }
            i += 1;
        }
        if allowkeyframe >= 0 {
            if hand.unk0cc8_01 && (self.bgun_get_current_keyframe(h) as i32) <= zreleasekeyframe {
                return false;
            }
            return self.bgun_get_current_keyframe(h) + hand.animframeinc as f32 >= allowkeyframe as f32;
        }
        true
    }

    /// `bgun_is_anim_busy`.
    pub(crate) fn bgun_is_anim_busy(&self, h: usize) -> bool {
        self.b.hands[h].animmode != HANDANIMMODE_IDLE
    }

    /// `bgun_reset_anim` (`:833`).
    pub(crate) fn bgun_reset_anim(&mut self, h: usize) {
        let hand = &mut self.b.hands[h];
        hand.animload = -1;
        hand.animmode = HANDANIMMODE_IDLE;
        hand.unk0cc8_01 = false;
        hand.incrementalreloading = false;
        hand.animcmd = None;
        hand.animloopcount = 0;
        hand.unk0cc8_02 = false;
        hand.unk0d0e_07 = false;
    }

    // ─── ammo (862-1034) ─────────────────────────────────────────────────────

    /// `bgun_get_ammo_state` (`:862`).
    pub fn bgun_get_ammo_state(&self, funcnum: usize, h: usize) -> i32 {
        let hand = &self.b.hands[h];
        let Some(func) = self.gset.func(hand.weaponnum, funcnum) else {
            return GUNAMMOSTATE_DEPLETED;
        };
        let mut state = GUNAMMOSTATE_CLIPFULL;
        if func.ammoindex != -1 {
            let ai = func.ammoindex as usize;
            if self.b.ctrl.ammotypes[ai] >= 0 && hand.loadedammo[ai] < hand.clipsizes[ai] {
                let mut minqty = 1;
                if hand.weaponnum == WEAPON_SHOTGUN && funcnum == FUNC_SECONDARY {
                    minqty = 2;
                }
                if hand.weaponnum == WEAPON_TRANQUILIZER && funcnum == FUNC_SECONDARY {
                    minqty = 4;
                }
                state = GUNAMMOSTATE_CLIPYES_HELDYES;
                if hand.loadedammo[ai] < minqty {
                    state = GUNAMMOSTATE_NEEDRELOAD;
                    if self.ammoheld(self.b.ctrl.ammotypes[ai]) == 0 {
                        state = GUNAMMOSTATE_DEPLETED;
                    }
                } else if self.ammoheld(self.b.ctrl.ammotypes[ai]) == 0 {
                    state = GUNAMMOSTATE_CLIPYES_HELDNO;
                }
            }
        }
        state
    }

    /// `bgun0f098df8` (`:904`): move ammo from the reserve into the clip.
    pub(crate) fn bgun_load_clip(&mut self, weaponfunc: usize, h: usize, onebullet: bool, checkunequipped: bool) {
        let Some(func) = self.func_by(h, weaponfunc) else { return };
        if func.ammoindex == -1 {
            return;
        }
        let ai = func.ammoindex as usize;
        let ammotype = self.b.ctrl.ammotypes[ai];
        if ammotype < 0 {
            return;
        }
        let hand = &self.b.hands[h];
        let mut amount = hand.clipsizes[ai] - hand.loadedammo[ai];
        let reloadindex = match hand.weaponnum {
            WEAPON_CROSSBOW => 0,
            WEAPON_SHOTGUN => 1,
            WEAPON_DY357MAGNUM => 2,
            WEAPON_DY357LX => 3,
            _ => -1,
        };
        if checkunequipped && reloadindex >= 0 {
            amount -= (hand.gunroundsspent[reloadindex as usize] >> 8) as i32;
        }
        if onebullet {
            amount = 1;
        }
        let held = self.ammoheld(ammotype);
        if amount > held {
            amount = held;
        }
        let flags = self.weapon(hand.weaponnum).and_then(|w| w.ammos[ai].as_ref()).map_or(0, |a| a.flags);
        self.b.hands[h].loadedammo[ai] += amount;
        self.b.p.ammoheldarr[ammotype as usize] -= amount;
        if flags & AMMOFLAG_NORESERVE != 0 {
            self.b.p.ammoheldarr[ammotype as usize] = 0;
        }
    }

    /// `bgun0f098f8c` (`:957`).
    pub(crate) fn bgun_load_all_clips(&mut self, h: usize) {
        for i in 0..2 {
            if self.func_by(h, i).is_some() {
                self.bgun_load_clip(i, h, false, true);
            }
        }
    }

    /// `bgun_clip_has_ammo` (`:968`).
    pub(crate) fn bgun_clip_has_ammo(&self, h: usize) -> bool {
        self.bgun_get_ammo_state(FUNC_PRIMARY, h) > GUNAMMOSTATE_NEEDRELOAD || self.bgun_get_ammo_state(FUNC_SECONDARY, h) > GUNAMMOSTATE_NEEDRELOAD
    }

    /// `bgun0f0990b0` (`:985`): is this function unusable for an autoswitch?
    pub(crate) fn bgun_func_unusable(&self, func: Option<&FuncDef>, weaponnum: u8) -> bool {
        let Some(f) = func else { return true };
        if f.ftype == INVENTORYFUNCTYPE_NONE || f.kind() == INVENTORYFUNCTYPE_MELEE || f.kind() == INVENTORYFUNCTYPE_SPECIAL {
            return true;
        }
        if f.kind() == INVENTORYFUNCTYPE_THROW && f.ammoindex <= -1 {
            return true;
        }
        if f.ammoindex >= 0 {
            if let Some(a) = self.weapon(weaponnum).and_then(|w| w.ammos[f.ammoindex as usize].as_ref()) {
                if self.bgun_get_ammo_count(a.ammotype) <= 0 {
                    return true;
                }
            }
        }
        false
    }

    /// `bgun0f099188` (`:1024`).
    pub(crate) fn bgun_other_func_unusable(&self, h: usize, gunfunc: usize) -> bool {
        if self.bgun_is_using_secondary_function() as usize == gunfunc {
            return false;
        }
        let w = self.b.hands[h].weaponnum;
        self.bgun_func_unusable(self.gset.func(w, gunfunc), w)
    }

    // ─── the hand state machine (1036-3131) ──────────────────────────────────

    /// `bgun_tick_inc_idle` (`:1036`).
    pub(crate) fn bgun_tick_inc_idle(&mut self, h: usize, lvupdate: i32) -> i32 {
        let gunfunc = self.bgun_is_using_secondary_function() as usize;
        {
            let hand = &mut self.b.hands[h];
            hand.lastdirvalid = false;
            hand.burstbullets = 0;
            hand.shotremainder = 0.0;
        }
        if self.bgun_is_ready_to_switch(h) && self.bgun_set_state(h, HANDSTATE_CHANGEGUN) {
            return lvupdate;
        }
        if gunfunc == self.b.hands[h].weaponfunc {
            self.b.hands[h].unk0cc8_07 = false;
        }
        self.b.hands[h].unk0cc8_08 = false;

        if self.b.hands[h].inuse {
            let ammostate = self.bgun_get_ammo_state(self.b.hands[h].weaponfunc, h);
            let weaponnum = self.b.hands[h].weaponnum;

            if gunfunc != self.b.hands[h].weaponfunc && self.b.hands[h].modenext != HANDMODE_RELOAD {
                let mut changefunc = true;
                if self.b.hands[h].unk0cc8_07 && self.bgun_get_ammo_state(1 - self.b.hands[h].weaponfunc, h) < 0 {
                    changefunc = false;
                }
                if changefunc && weaponnum == WEAPON_COMBATKNIFE {
                    if ammostate == GUNAMMOSTATE_NEEDRELOAD {
                        self.b.hands[h].count60 = 0;
                        self.b.hands[h].count = 0;
                        self.b.hands[h].weaponfunc = gunfunc;
                        if self.bgun_set_state(h, HANDSTATE_RELOAD) {
                            return lvupdate;
                        }
                    } else if ammostate <= GUNAMMOSTATE_DEPLETED {
                        changefunc = false;
                    }
                }
                if changefunc {
                    self.b.hands[h].unk0cc8_07 = false;
                    if self.bgun_set_state(h, HANDSTATE_CHANGEFUNC) {
                        return lvupdate;
                    }
                }
            }

            if ammostate <= GUNAMMOSTATE_DEPLETED {
                if self.gset.has_flag(weaponnum, WEAPONFLAG_THROWABLE) && (weaponnum != WEAPON_REMOTEMINE || h != HAND_LEFT) && self.bgun_set_state(h, HANDSTATE_AUTOSWITCH) {
                    return lvupdate;
                }
                let usesec = self.funcissec() as usize;
                if usesec == gunfunc {
                    let mut ammostate2 = self.bgun_get_ammo_state(1 - self.b.hands[h].weaponfunc, h);
                    if self.bgun_other_func_unusable(h, 1 - self.b.hands[h].weaponfunc) && weaponnum != WEAPON_REAPER {
                        if self.b.ctrl.wantammo {
                            let f = self.func_by(h, 1 - self.b.hands[h].weaponfunc);
                            if f.is_none_or(|f| f.kind() != INVENTORYFUNCTYPE_MELEE) {
                                ammostate2 = GUNAMMOSTATE_DEPLETED;
                            }
                        } else {
                            ammostate2 = GUNAMMOSTATE_DEPLETED;
                        }
                    }
                    if ammostate2 <= GUNAMMOSTATE_DEPLETED {
                        self.b.hands[h].unk0cc8_08 = true;
                    } else if !self.gset.has_flag(weaponnum, WEAPONFLAG_KEEPFUNCWHENEMPTY) || self.b.hands[h].weaponfunc == FUNC_SECONDARY {
                        self.b.hands[h].unk0cc8_07 = true;
                        if self.bgun_set_state(h, HANDSTATE_CHANGEFUNC) {
                            return lvupdate;
                        }
                    }
                }
            } else if ammostate == GUNAMMOSTATE_NEEDRELOAD {
                if self.b.hands[h].triggeron && weaponnum != WEAPON_NONE {
                    self.b.hands[h].unk0cc8_01 = false;
                    if self.bgun_set_state(h, HANDSTATE_ATTACKEMPTY) {
                        return lvupdate;
                    }
                } else {
                    self.b.hands[h].count60 = 0;
                    self.b.hands[h].count = 0;
                    if self.bgun_set_state(h, HANDSTATE_RELOAD) {
                        return lvupdate;
                    }
                }
            } else {
                if (self.b.hands[h].triggeron || (self.b.hands[h].activatesecondary && self.b.hands[h].weaponfunc == FUNC_SECONDARY)) && weaponnum != WEAPON_NONE {
                    self.b.p.doautoselect = false;
                    let hand = &mut self.b.hands[h];
                    hand.mode = HANDMODE_ATTACK;
                    hand.count = 0;
                    hand.count60 = 0;
                    hand.triggerreleased = false;
                    hand.activatesecondary = false;
                    if self.bgun_set_state(h, HANDSTATE_ATTACK) {
                        return lvupdate;
                    }
                }
                if self.b.hands[h].modenext != HANDMODE_NONE {
                    let next = self.b.hands[h].modenext;
                    let hand = &mut self.b.hands[h];
                    hand.mode = hand.modenext;
                    hand.count60 = 0;
                    hand.count = 0;
                    hand.modenext = HANDMODE_NONE;
                    if next == HANDMODE_RELOAD && (GUNAMMOSTATE_NEEDRELOAD..GUNAMMOSTATE_CLIPYES_HELDNO).contains(&ammostate) && self.bgun_set_state(h, HANDSTATE_RELOAD) {
                        return lvupdate;
                    }
                }
            }
        }

        if h == HAND_RIGHT {
            if self.b.ctrl.wantammo {
                self.bgun_auto_switch_weapon();
            } else {
                let (r, l) = (&self.b.hands[0], &self.b.hands[1]);
                if (r.unk0cc8_08 || !r.inuse) && (l.unk0cc8_08 || !l.inuse) && (r.triggeron || l.triggeron) {
                    self.bgun_auto_switch_weapon();
                }
                self.b.hands[0].unk0cc8_08 = false;
                self.b.hands[1].unk0cc8_08 = false;
            }
        }
        0
    }

    /// `bgun_set_arm_pitch` (`:1214`).
    pub(crate) fn bgun_set_arm_pitch(&mut self, h: usize, angle: f32) {
        let hand = &mut self.b.hands[h];
        hand.useposrot = true;
        hand.posrotmtx = pd_core::math::load_x_rotation(angle);
        hand.posrotmtx.w_axis = glam::Vec4::new(0.0, (1.0 - angle.cos()) * -80.0, angle.sin() * 15.0, 1.0);
    }

    /// `bgun_tick_inc_autoswitch` (`:1225`): the throwables-ran-out path.
    pub(crate) fn bgun_tick_inc_autoswitch(&mut self, h: usize, lvupdate: i32) -> i32 {
        let gunfunc = self.bgun_is_using_secondary_function() as usize;
        if !self.b.hands[h].inuse && self.bgun_set_state(h, HANDSTATE_IDLE) {
            return lvupdate;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_AUTOSWITCH_UNEQUIP {
            let delay = if NORMMPLAYERISRUNNING { 12 } else { 16 };
            if self.b.hands[h].stateframes >= delay {
                self.b.hands[h].stateminor += 1;
            } else {
                let a = self.b.hands[h].stateframes as f32 * max_pitch() / delay as f32;
                self.bgun_set_arm_pitch(h, a);
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_AUTOSWITCH_DELETE {
            self.b.hands[h].lastdirvalid = false;
            self.b.hands[h].shotremainder = 0.0;
            if self.bgun_is_ready_to_switch(h) && self.bgun_set_state(h, HANDSTATE_CHANGEGUN) {
                // M8: playermgr_delete_weapon(handnum) (multiplayer: the
                // empty throwable leaves the inventory).
                self.b.events.push(super::GunEvent::FreeHeldRocket { hand: h });
                self.b.hands[h].mode = HANDMODE_6;
                self.b.hands[h].stateminor = HANDSTATEMINOR_AUTOSWITCH_2;
                self.b.hands[h].count = 0;
                return 0;
            }
            if self.b.hands[h].inuse {
                let ammostate = self.bgun_get_ammo_state(gunfunc, h);
                let weaponnum = self.bgun_get_weapon_num(h);
                if weaponnum == WEAPON_TIMEDMINE || weaponnum == WEAPON_PROXIMITYMINE {
                    self.b.hands[h].weaponfunc = gunfunc;
                }
                if weaponnum == WEAPON_REMOTEMINE && gunfunc != self.b.hands[h].weaponfunc && self.bgun_set_state(h, HANDSTATE_CHANGEFUNC) {
                    return lvupdate;
                }
                if self.b.p.doautoselect {
                    let o = 1 - h;
                    let mut ready = true;
                    if self.b.hands[o].inuse {
                        if self.bgun_get_ammo_state(FUNC_PRIMARY, o) > GUNAMMOSTATE_DEPLETED {
                            ready = false;
                        }
                        if self.bgun_get_ammo_state(FUNC_SECONDARY, o) > GUNAMMOSTATE_DEPLETED {
                            ready = false;
                        }
                        if self.bgun_other_func_unusable(o, self.b.hands[o].weaponfunc) {
                            ready = true;
                        }
                    }
                    if self.b.hands[o].state != HANDSTATE_IDLE && self.b.hands[o].state != HANDSTATE_AUTOSWITCH {
                        ready = false;
                    }
                    if ready {
                        self.bgun_auto_switch_weapon();
                    }
                }
                if (GUNAMMOSTATE_NEEDRELOAD..=GUNAMMOSTATE_CLIPYES_HELDYES).contains(&ammostate) && self.b.hands[1 - h].state != HANDSTATE_RELOAD {
                    self.b.hands[h].count60 = 0;
                    self.b.hands[h].count = 0;
                    if self.bgun_set_state(h, HANDSTATE_RELOAD) {
                        if weaponnum == WEAPON_COMBATKNIFE {
                            let hand = &mut self.b.hands[h];
                            hand.mode = HANDMODE_11;
                            hand.pausetime60 = 17;
                            hand.count60 = 0;
                            hand.count = -1;
                            hand.stateminor = HANDSTATEMINOR_AUTOSWITCH_2;
                        }
                        return lvupdate;
                    }
                }
                if self.b.hands[h].modenext != HANDMODE_NONE {
                    let hand = &mut self.b.hands[h];
                    hand.mode = hand.modenext;
                    hand.count60 = 0;
                    hand.count = 0;
                    hand.modenext = HANDMODE_NONE;
                }
            }
            self.bgun_set_arm_pitch(h, max_pitch());
        }
        0
    }

    /// `bgun_is_reloading`.
    pub fn bgun_is_reloading(&self, h: usize) -> bool {
        self.b.hands[h].state == HANDSTATE_RELOAD
    }

    /// The ammo definition a function loads from.
    fn ammodef(&self, weaponnum: u8, f: &Option<FuncDef>) -> Option<AmmoDef> {
        let f = f.as_ref()?;
        if f.ammoindex < 0 {
            return None;
        }
        self.weapon(weaponnum).and_then(|w| w.ammos[f.ammoindex as usize].clone())
    }

    /// `bgun_tick_inc_reload` (`:1354`).
    pub(crate) fn bgun_tick_inc_reload(&mut self, h: usize, lvupdate: i32) -> i32 {
        let func = self.func_of(h);
        let weaponnum = self.b.hands[h].weaponnum;
        if self.pl.isdead {
            self.b.hands[h].animmode = HANDANIMMODE_IDLE;
            self.b.hands[h].animload = -1;
            if self.bgun_set_state(h, HANDSTATE_IDLE) {
                return lvupdate;
            }
        }
        if self.b.hands[h].statecycles == 0 {
            self.b.hands[h].gs_int1 = -1;
            self.b.hands[h].gs_int2 = 0;
            let other = &self.b.hands[1 - h];
            if other.state == HANDSTATE_RELOAD && other.stateframes < 20 {
                self.b.hands[h].stateminor = HANDSTATEMINOR_RELOAD_WAIT;
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_RELOAD_WAIT {
            let other = &self.b.hands[1 - h];
            if other.state == HANDSTATE_RELOAD && other.stateframes < 20 {
                return 0;
            }
            let hand = &mut self.b.hands[h];
            hand.stateframes = 0;
            hand.statecycles = 0;
            hand.stateminor = HANDSTATEMINOR_RELOAD_MAIN;
            hand.statelastframe = 0;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_RELOAD_MAIN {
            if self.b.hands[h].statecycles == 0 {
                if func.as_ref().is_some_and(|f| f.ammoindex == 0 || f.ammoindex == 1) {
                    let a = self.ammodef(weaponnum, &func);
                    match a.as_ref().and_then(|a| a.reload_animation) {
                        Some(script) if weaponnum != WEAPON_COMBATKNIFE => {
                            self.bgun_start_animation(script, h);
                            self.b.hands[h].unk0d0e_07 = true;
                            if a.as_ref().is_some_and(|a| a.flags & AMMOFLAG_INCREMENTALRELOAD != 0) {
                                self.b.hands[h].incrementalreloading = true;
                            }
                            if weaponnum == WEAPON_GRENADE || weaponnum == WEAPON_NBOMB {
                                self.b.hands[h].ejectstate = EJECTSTATE_INACTIVE;
                            }
                        }
                        _ => {
                            self.b.hands[h].stateminor += 1;
                        }
                    }
                } else if self.bgun_set_state(h, HANDSTATE_IDLE) {
                    return lvupdate;
                }
            } else {
                let a = self.ammodef(weaponnum, &func);
                let incremental = a.as_ref().is_some_and(|a| a.flags & AMMOFLAG_INCREMENTALRELOAD != 0);
                if incremental {
                    if self.bgun_anim_allows_feature(h, GUNFEATURE_RELOAD) {
                        if self.b.hands[h].stateflags & HANDSTATEFLAG_BUSY == 0 {
                            let wf = self.b.hands[h].weaponfunc;
                            self.bgun_load_clip(wf, h, true, false);
                            self.b.hands[h].stateflags |= HANDSTATEFLAG_BUSY;
                            let ammostate = self.bgun_get_ammo_state(wf, h);
                            if ammostate >= GUNAMMOSTATE_CLIPYES_HELDNO || ammostate == GUNAMMOSTATE_DEPLETED {
                                self.b.hands[h].incrementalreloading = false;
                            }
                        }
                    } else {
                        self.b.hands[h].stateflags = 0;
                    }
                    if self.b.hands[h].triggeron {
                        self.b.hands[h].incrementalreloading = false;
                    }
                } else if self.b.hands[h].stateflags & HANDSTATEFLAG_BUSY == 0 && self.bgun_anim_allows_feature(h, GUNFEATURE_RELOAD) {
                    let wf = self.b.hands[h].weaponfunc;
                    self.bgun_load_clip(wf, h, false, false);
                    self.b.hands[h].stateflags |= HANDSTATEFLAG_BUSY;
                }
                if self.b.hands[h].animmode != HANDANIMMODE_BUSY && self.bgun_set_state(h, HANDSTATE_IDLE) {
                    return lvupdate;
                }
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_RELOAD_LOWER {
            if self.b.hands[h].count60 > 15 || !self.b.hands[h].visible {
                let hand = &mut self.b.hands[h];
                hand.mode = HANDMODE_11;
                hand.stateminor += 1;
                hand.pausetime60 = 17;
                hand.count60 = 0;
                hand.count = 0;
            } else {
                let a = self.b.hands[h].count60 as f32 * max_pitch() / 16.0;
                self.bgun_set_arm_pitch(h, a);
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_RELOAD_SOUND {
            if self.b.hands[h].count == 0 {
                if weaponnum == WEAPON_COMBATKNIFE {
                    if let Some(script) = self.ammodef(weaponnum, &func).and_then(|a| a.reload_animation) {
                        self.bgun_start_animation(script, h);
                        self.b.hands[h].unk0cc8_02 = true;
                    }
                }
                if self.b.hands[h].stateflags & HANDSTATEFLAG_BUSY == 0 {
                    let wf = self.b.hands[h].weaponfunc;
                    self.bgun_load_clip(wf, h, false, false);
                }
                if !self.pl.isdead
                    && !matches!(weaponnum, WEAPON_NONE | WEAPON_UNARMED | WEAPON_COMBATKNIFE | WEAPON_LASER | WEAPON_GRENADE | WEAPON_TIMEDMINE | WEAPON_PROXIMITYMINE | WEAPON_REMOTEMINE)
                {
                    self.sound(SFXMAP_804F_RELOAD_DEFAULT, 1.0);
                }
            }
            if self.b.hands[h].count60 >= self.b.hands[h].pausetime60 && self.b.hands[h].count >= 2 {
                let hand = &mut self.b.hands[h];
                hand.mode = HANDMODE_12;
                hand.stateminor += 1;
                hand.count60 = 0;
                hand.count = 0;
            } else {
                self.bgun_set_arm_pitch(h, max_pitch());
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_RELOAD_RAISE {
            if weaponnum == WEAPON_COMBATKNIFE {
                self.b.hands[h].animmode = HANDANIMMODE_IDLE;
            }
            if self.b.hands[h].count == 0 {
                self.b.p.doautoselect = false;
            }
            if self.b.hands[h].count60 >= 23 || !self.gset.has_model(weaponnum) || !self.gset.has_flag(weaponnum, WEAPONFLAG_00000040) || self.gset.has_flag(weaponnum, WEAPONFLAG_00000080) {
                let hand = &mut self.b.hands[h];
                hand.mode = HANDMODE_NONE;
                hand.count60 = 0;
                hand.count = 0;
                if self.bgun_set_state(h, HANDSTATE_IDLE) {
                    return lvupdate;
                }
            } else {
                let a = (23 - self.b.hands[h].count60) as f32 * max_pitch() / 23.0;
                self.bgun_set_arm_pitch(h, a);
            }
        }
        0
    }

    /// `bgun_tick_inc_changefunc` (`:1559`).
    pub(crate) fn bgun_tick_inc_changefunc(&mut self, h: usize, lvupdate: i32) -> i32 {
        let mut more = false;
        if self.b.hands[h].statecycles == 0 {
            let w = self.weapon(self.b.hands[h].weaponnum).cloned();
            let cmd = if self.b.hands[h].weaponfunc == FUNC_PRIMARY {
                self.b.hands[h].weaponfunc = FUNC_SECONDARY;
                w.and_then(|w| w.pritosec_animation)
            } else {
                self.b.hands[h].weaponfunc = FUNC_PRIMARY;
                w.and_then(|w| w.sectopri_animation)
            };
            if let Some(script) = cmd {
                self.bgun_start_animation(script, h);
                more = true;
            }
        } else if self.b.hands[h].animmode == HANDANIMMODE_BUSY {
            more = true;
        }
        if !more && self.bgun_set_state(h, HANDSTATE_IDLE) {
            return lvupdate;
        }
        0
    }

    /// `bgun0f09a3f8` (`:1593`): may this function fire this tick?
    /// -1 stop, 0 wait, 1 fire and keep the burst going, 2 fire and finish.
    pub(crate) fn bgun_should_fire(&mut self, h: usize, func: &FuncDef) -> i32 {
        let mut burst = false;
        let mut smallburst = false;
        let bb = self.b.hands[h].burstbullets;
        if func.flags & FUNCFLAG_BURST3 != 0 && bb < 3 && (!self.pl.insightaimmode || !func.is_auto()) {
            smallburst = true;
        }
        if func.flags & FUNCFLAG_BURST2 != 0 && bb < 2 {
            smallburst = true;
        }
        if func.flags & FUNCFLAG_BURST5 != 0 && bb < 5 {
            smallburst = true;
        }
        if func.flags & FUNCFLAG_BURST50 != 0 && bb < 50 {
            burst = true;
        }
        if smallburst {
            burst = true;
        }
        let hand_trig = self.b.hands[h].triggeron;
        let busy = self.b.hands[h].stateflags & HANDSTATEFLAG_BUSY != 0;
        let shoot = func.shoot.clone().unwrap_or_default();
        let lv60 = self.lv.lvupdate60freal;
        if hand_trig || !busy || burst {
            if func.ammoindex >= 0 && self.b.hands[h].loadedammo[func.ammoindex as usize] == 0 && self.b.ctrl.ammotypes[func.ammoindex as usize] >= 0 {
                return -1;
            }
            if func.is_auto() {
                let hand = &mut self.b.hands[h];
                if shoot.turretaccel > 0.0 {
                    if hand.gs_barrelspeedfrac < 1.0 {
                        hand.gs_barrelspeedfrac += lv60 / shoot.turretaccel;
                        if hand.gs_barrelspeedfrac > 1.0 {
                            hand.gs_barrelspeedfrac = 1.0;
                        }
                    }
                } else {
                    hand.gs_barrelspeedfrac = 1.0;
                }
                return 1;
            }
            self.b.hands[h].gs_barrelspeedfrac = 1.0;
            if smallburst {
                if self.b.hands[h].burstbullets > 0 {
                    let delay = if self.b.hands[h].weaponnum == WEAPON_SHOTGUN { 13 } else { 3 };
                    if self.b.hands[h].stateframes < delay {
                        return 0;
                    }
                }
                self.b.hands[h].stateframes = 0;
            }
            if func.flags & FUNCFLAG_BURST3 != 0 && bb == 2 {
                smallburst = false;
            }
            if func.flags & FUNCFLAG_BURST2 != 0 && bb == 1 {
                smallburst = false;
            }
            if func.flags & FUNCFLAG_BURST5 != 0 && bb == 4 {
                smallburst = false;
            }
            return if smallburst { 1 } else { 2 };
        }
        if func.is_auto() {
            let hand = &mut self.b.hands[h];
            if shoot.turretdecel > 0.0 {
                if hand.gs_barrelspeedfrac > 0.0 {
                    hand.gs_barrelspeedfrac -= lv60 / shoot.turretdecel;
                    if hand.gs_barrelspeedfrac < 0.0 {
                        hand.gs_barrelspeedfrac = 0.0;
                        return -1;
                    }
                    return 1;
                }
            } else {
                hand.gs_barrelspeedfrac = 0.0;
            }
            return -1;
        }
        -1
    }

    /// `bgun0f09a6f8` (`:1710`): fire one tick's worth of rounds.
    pub(crate) fn bgun_fire(&mut self, h: usize, func: &FuncDef) {
        let mut usesammo = true;
        self.b.hands[h].firing = true;
        let shoot = func.shoot.clone().unwrap_or_default();
        let lv60 = self.lv.lvupdate60freal;
        let lvframe60 = self.lv.lvframe60;
        if func.is_auto() {
            let hand = &mut self.b.hands[h];
            let tmp = shoot.initialrpm + (shoot.maxrpm - shoot.initialrpm) * hand.gs_barrelspeedfrac;
            let tmp2 = tmp / 60.0 * (lv60 / 60.0) + hand.shotremainder;
            hand.shotstotake = tmp2 as i32;
            hand.shotremainder = tmp2 - hand.shotstotake as f32;
            if hand.shotstotake <= 0 {
                if hand.stateflags & HANDSTATEFLAG_BUSY == 0 {
                    hand.shotstotake += 1;
                } else {
                    hand.firing = false;
                }
            }
        } else {
            self.b.hands[h].shotstotake = 1;
            if self.b.hands[h].weaponnum == WEAPON_LASER {
                usesammo = false;
            }
        }
        let hand = &mut self.b.hands[h];
        hand.burstbullets += hand.shotstotake;
        hand.flashon = func.flags & FUNCFLAG_NOMUZZLEFLASH == 0;
        self.bgun_start_slide(h);
        self.b.hands[h].loadslide = 0.0;

        if self.b.hands[h].firing {
            let hand = &mut self.b.hands[h];
            hand.statevar1 = hand.stateframes;
            hand.stateflags |= HANDSTATEFLAG_FIRED | HANDSTATEFLAG_BUSY;
            if usesammo && func.ammoindex >= 0 {
                let ai = func.ammoindex as usize;
                hand.loadedammo[ai] -= hand.shotstotake;
                if hand.loadedammo[ai] < 0 {
                    hand.shotstotake += hand.loadedammo[ai];
                    hand.loadedammo[ai] = 0;
                }
            }
            hand.attacktype = match func.ftype & 0xff00 {
                0x200 => HANDATTACKTYPE_SHOOTPROJECTILE,
                _ => HANDATTACKTYPE_SHOOT,
            };
            // A fire slot limits a looping shot sound to one per duration60,
            // else one per shot.
            let mut playsound = false;
            if shoot.duration60 > 0 {
                if lvframe60 != self.b.hands[1 - h].lastshootframe60 && lvframe60 > self.b.hands[h].allowshootframe {
                    self.b.hands[h].allowshootframe = lvframe60 + shoot.duration60;
                    playsound = true;
                }
            } else {
                playsound = true;
            }
            if playsound && shoot.shootsound != 0 {
                self.b.hands[h].lastshootframe60 = lvframe60;
                let mut speed = 1.0;
                if self.b.hands[h].weaponnum == WEAPON_MAULER {
                    let charge = self.b.hands[h].matmot1 as i32;
                    let frac = (charge as f32 / 3.0).min(1.0);
                    speed = 1.0 - frac * 0.4;
                }
                self.sound(shoot.shootsound, speed);
            }
        }
    }
}
