//! `bondgun.c`, second half of the hand state machine: recoil, the attack
//! paths, the dry-fire click, weapon switching (with the gun-memory load
//! latency), gun functions, and `bgun_tick_gameplay`.

use glam::{Mat4, Vec3};
use pd_core::anim::Anim;
use pd_core::ids::*;
use pd_core::math::{self, baddtor, dtor};
use pd_core::model::Model;

use super::gset::ShootDef;
use super::{max_pitch, GunCtx, GunEvent, AMMO_CAPACITY, NORMMPLAYERISRUNNING, SFXMAP_8052_FIREEMPTY, SFXNUM_00E8_PICKUP_GUN};

impl GunCtx<'_> {
    /// `bgun_tick_recoil` (`:1851`). True when the gun may fire again.
    pub(crate) fn bgun_tick_recoil(&mut self, h: usize, shoot: &ShootDef) -> bool {
        let unk24 = shoot.unk24;
        let unk25 = shoot.unk25;
        let mut sum = unk24 + unk25;
        let unk26 = shoot.unk26;
        let unk27 = shoot.unk27;
        let recoverytime60 = shoot.recoverytime60;
        let weaponnum = self.b.hands[h].weaponnum;
        let posz = self.gset.weapon(weaponnum).map_or(0.0, |w| w.posz);
        let xpos = self.gset_get_xpos(h);
        let curframe = self.b.hands[h].stateframes - self.b.hands[h].statevar1;

        if sum <= 0 {
            sum = 0;
        } else {
            let hand = &mut self.b.hands[h];
            if hand.triggerreleased && hand.triggeron && curframe >= unk26 && unk26 > 0 && unk27 >= 0 && hand.stateflags & HANDSTATEFLAG_00000040 == 0 && curframe + unk27 < sum {
                // Fired again during the recoil.
                hand.stateflags |= HANDSTATEFLAG_00000040;
                hand.statevar1 = curframe;
                hand.rotxstart = hand.rotxoffset;
                hand.rotxend = 0.0;
                hand.posend = Vec3::ZERO;
                hand.posstart = hand.posoffset;
            }
            if hand.stateflags & HANDSTATEFLAG_00000040 != 0 {
                if curframe - hand.statevar1 < unk27 {
                    let mult1 = ((unk27 - curframe + hand.statevar1) as f32 * dtor(90.0) / unk27 as f32).cos() * 0.5 + 0.5;
                    hand.rotxoffset = math::tween_rot_axis(hand.rotxstart, hand.rotxend, mult1);
                    hand.useposrot = true;
                    hand.posoffset = (hand.posend - hand.posstart) * mult1 + hand.posstart;
                    hand.posrotmtx = math::load_x_rotation(hand.rotxoffset);
                    math::set_translation(&mut hand.posrotmtx, hand.posoffset);
                } else {
                    hand.posrotmtx = Mat4::IDENTITY;
                    hand.useposrot = false;
                    return true;
                }
            }
            if curframe < sum && hand.stateflags & HANDSTATEFLAG_00000040 == 0 {
                let recoildist = shoot.recoildist;
                let recoilangle = shoot.recoilangle;
                if hand.stateflags & HANDSTATEFLAG_00000080 == 0 {
                    hand.stateflags |= HANDSTATEFLAG_00000080;
                    hand.rotxstart = hand.rotxoffset;
                    hand.posstart = hand.posoffset;
                }
                // BADDTOR(360) - BADDTOR3(recoilangle)
                hand.rotxend = baddtor(360.0) - recoilangle * math::M_BADTAU / 360.0;
                hand.posend.x = (xpos - hand.aimpos.x) * recoildist / 1000.0;
                hand.posend.y = 0.0;
                hand.posend.z = (posz - hand.aimpos.z) * recoildist / 1000.0;
                let mult2 = if curframe < unk24 {
                    (curframe as f32 * dtor(90.0) / unk24 as f32).sin()
                } else {
                    ((curframe - unk24) as f32 * dtor(180.0) / unk25 as f32).cos() * 0.5 + 0.5
                };
                hand.rotxoffset = math::tween_rot_axis(hand.rotxstart, hand.rotxend, mult2);
                hand.useposrot = true;
                hand.posoffset = (hand.posend - hand.posstart) * mult2 + hand.posstart;
                hand.posrotmtx = math::load_x_rotation(hand.rotxoffset);
                math::set_translation(&mut hand.posrotmtx, hand.posoffset);
            }
        }
        if curframe >= sum {
            let hand = &self.b.hands[h];
            if (unk27 >= 0 && hand.triggerreleased && hand.triggeron) || sum + recoverytime60 <= curframe {
                return true;
            }
        }
        false
    }

    /// `bgun_tick_inc_attacking_shoot` (`:2009`).
    fn bgun_tick_inc_attacking_shoot(&mut self, h: usize) -> bool {
        let Some(func) = self.func_of(h) else { return true };
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_SHOOT_0 {
            let mut ready = true;
            if self.b.hands[h].statecycles == 0 {
                self.b.hands[h].gs_barrelspeedfrac = 0.0;
                if let Some(script) = func.fire_animation {
                    self.bgun_start_animation(script, h);
                    self.b.hands[h].unk0cc8_01 = true;
                }
                self.b.hands[h].burstbullets = 0;
            }
            if !self.bgun_anim_allows_feature(h, GUNFEATURE_ATTACK) {
                ready = false;
            }
            if ready {
                self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_SHOOT_1;
            }
            // mm_reaperspeedaim
            self.b.hands[h].matmot2 = self.b.hands[h].gs_barrelspeedfrac;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_SHOOT_1 {
            let r = self.bgun_should_fire(h, &func);
            if r > 0 {
                self.bgun_fire(h, &func);
            }
            if r < 0 || r == 2 {
                self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_SHOOT_2;
            }
            let hand = &mut self.b.hands[h];
            hand.matmot2 = hand.gs_barrelspeedfrac;
            if hand.triggeron && hand.matmot2 < 0.4 {
                hand.matmot2 = 0.4;
            }
            if hand.triggerreleased {
                hand.unk0cc8_01 = false;
            }
            return false;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_SHOOT_2 {
            let mut canfireagain = if self.b.hands[h].stateflags & HANDSTATEFLAG_FIRED != 0 {
                let shoot = func.shoot.clone().unwrap_or_default();
                self.bgun_tick_recoil(h, &shoot)
            } else {
                true
            };
            if self.b.hands[h].weaponnum == WEAPON_SHOTGUN && self.b.hands[h].animmode == HANDANIMMODE_BUSY {
                canfireagain = false;
            }
            let hand = &mut self.b.hands[h];
            hand.matmot2 = hand.gs_barrelspeedfrac;
            if canfireagain && !hand.triggeron {
                hand.matmot2 = 0.0;
            }
            if hand.weaponnum == WEAPON_MAULER {
                hand.matmot1 = 0.0;
            }
            return canfireagain;
        }
        false
    }

    /// `bgun_tick_inc_attacking_throw` (`:2110`). The projectile itself is made
    /// by `hand_tick_attack` → `bgun_create_thrown_projectile` (M5).
    fn bgun_tick_inc_attacking_throw(&mut self, h: usize) -> bool {
        let Some(func) = self.func_of(h) else { return true };
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_THROW_0 {
            if self.b.hands[h].statecycles == 0 {
                if func.flags & FUNCFLAG_DISCARDWEAPON != 0 {
                    // Laptop deploy / Dragon self-destruct: out of the inventory
                    // and lowered; the throw happens once it is down
                    // (bgun_tick_inc_changegun's `throwing`).
                    let w = self.b.hands[h].weaponnum;
                    self.b.inv_remove_item_by_num(w);
                    self.b.ctrl.throwing = true;
                    self.bgun_switch_to_previous();
                    self.b.hands[h].primetimer60 = 0;
                    return true;
                }
                if let Some(script) = func.fire_animation {
                    self.bgun_start_animation(script, h);
                    self.b.hands[h].unk0cc8_01 = true;
                }
            }
            if func.fire_animation.is_some() {
                if self.b.hands[h].triggerreleased {
                    self.b.hands[h].unk0cc8_01 = false;
                }
                if self.bgun_anim_allows_feature(h, GUNFEATURE_ATTACK) {
                    self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_THROW_1;
                    self.b.hands[h].unk0cc8_01 = false;
                }
            } else {
                self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_THROW_1;
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_THROW_1 {
            let hand = &mut self.b.hands[h];
            hand.firing = true;
            hand.attacktype = HANDATTACKTYPE_THROWPROJECTILE;
            if func.ammoindex >= 0 {
                hand.loadedammo[func.ammoindex as usize] -= 1;
            }
            hand.stateminor = HANDSTATEMINOR_ATTACK_THROW_2;
            return false;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_THROW_2 {
            if self.b.hands[h].stateframes > func.recoverytime60 {
                return true;
            }
            if self.b.hands[h].weaponnum == WEAPON_REMOTEMINE && self.bgun_is_using_secondary_function() && self.b.hands[h].triggerreleased && self.b.hands[h].triggeron {
                return true;
            }
            return false;
        }
        // Only after a grenade went off in the hand: wait 4 s for the flames.
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_THROW_GRENADEWAIT {
            self.bgun_reset_anim(h);
            return self.b.hands[h].stateframes > func.activatetime60 + 240;
        }
        // Still in THROW_0 (the trigger holds the throw): cooking.
        self.b.hands[h].primetimer60 = self.b.hands[h].stateframes;
        if self.b.hands[h].weaponnum == WEAPON_GRENADE && self.b.hands[h].weaponfunc == FUNC_PRIMARY && self.b.hands[h].primetimer60 > func.activatetime60 {
            let hand = &mut self.b.hands[h];
            hand.firing = true;
            hand.attacktype = HANDATTACKTYPE_THROWPROJECTILE;
            if func.ammoindex >= 0 {
                hand.loadedammo[func.ammoindex as usize] -= 1;
            }
            hand.stateminor = HANDSTATEMINOR_ATTACK_THROW_GRENADEWAIT;
            return false;
        }
        false
    }

    /// `bgun_tick_inc_attacking_melee` (`:2222`).
    fn bgun_tick_inc_attacking_melee(&mut self, h: usize) -> bool {
        let Some(func) = self.func_of(h) else { return true };
        if self.b.hands[h].weaponnum == WEAPON_REAPER {
            let lv60 = self.lv.lvupdate60freal;
            let hand = &mut self.b.hands[h];
            if hand.statecycles == 0 {
                hand.matmot2 = 0.1;
                hand.burstbullets = 0;
            }
            hand.firing = true;
            hand.attacktype = HANDATTACKTYPE_MELEE;
            hand.burstbullets += 1;
            if hand.triggeron {
                hand.matmot2 += 0.01 * lv60;
                if hand.matmot2 > 1.0 {
                    hand.matmot2 = 1.0;
                }
            } else {
                hand.matmot2 = 0.0;
                return true;
            }
            return false;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_MELEE_0 {
            if self.b.hands[h].statecycles == 0 {
                self.b.hands[h].firing = true;
                self.b.hands[h].attacktype = HANDATTACKTYPE_MELEENOUNCLOAK;
                if let Some(script) = func.fire_animation {
                    self.bgun_start_animation(script, h);
                    self.b.hands[h].unk0cc8_01 = true;
                }
            }
            if func.fire_animation.is_some() {
                if self.b.hands[h].triggerreleased {
                    self.b.hands[h].unk0cc8_01 = false;
                }
                if self.bgun_anim_allows_feature(h, GUNFEATURE_ATTACK) {
                    self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_MELEE_1;
                    self.b.hands[h].unk0cc8_01 = false;
                }
            } else {
                self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_MELEE_1;
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_MELEE_3 && self.bgun_anim_allows_feature(h, GUNFEATURE_ATTACKAGAIN) {
            self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_MELEE_1;
            self.b.hands[h].unk0cc8_01 = false;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_MELEE_1 {
            self.b.hands[h].firing = true;
            self.b.hands[h].attacktype = HANDATTACKTYPE_MELEE;
            if func.fire_animation.is_some() {
                if !self.bgun_anim_allows_feature(h, GUNFEATURE_ATTACKAGAIN) {
                    self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_MELEE_3;
                } else {
                    self.b.hands[h].stateminor = HANDSTATEMINOR_ATTACK_MELEE_2;
                }
            }
            return false;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_ATTACK_MELEE_2 {
            if !self.bgun_is_anim_busy(h) {
                return true;
            }
            return self.b.hands[h].stateframes > 60;
        }
        false
    }

    /// `bgun_tick_inc_attacking_special` (`:2330`).
    fn bgun_tick_inc_attacking_special(&mut self, h: usize) -> bool {
        let Some(func) = self.func_of(h) else { return true };
        let hand = &mut self.b.hands[h];
        if hand.stateminor == HANDSTATEMINOR_ATTACK_SPECIAL_START {
            hand.stateminor = HANDSTATEMINOR_ATTACK_SPECIAL_EXECUTE;
        }
        if hand.stateminor == HANDSTATEMINOR_ATTACK_SPECIAL_EXECUTE {
            hand.firing = true;
            hand.attacktype = func.specialfunc;
            if func.ammoindex >= 0 {
                hand.loadedammo[func.ammoindex as usize] -= 1;
            }
            hand.stateminor = HANDSTATEMINOR_ATTACK_SPECIAL_RECOVER;
            return false;
        }
        if hand.stateminor == HANDSTATEMINOR_ATTACK_SPECIAL_RECOVER {
            return hand.stateframes > func.recoverytime60;
        }
        false
    }

    /// `bgun_tick_inc_attackempty` (`:2365`): the dry-fire click.
    fn bgun_tick_inc_attackempty(&mut self, h: usize, lvupdate: i32) -> i32 {
        let weaponnum = self.b.hands[h].weaponnum;
        let mut playsound = false;
        let finger = matches!(
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
                | WEAPON_REAPER
                | WEAPON_TRANQUILIZER
        );
        if finger {
            // The weapons whose trigger finger animates.
            if self.b.hands[h].stateframes > 25 {
                self.b.hands[h].stateframes -= 25;
                self.b.hands[h].stateflags = 0;
                self.bgun_reset_anim(h);
            }
            if self.b.hands[h].animmode != HANDANIMMODE_BUSY {
                let mut restartedanim = false;
                if self.b.hands[h].stateflags & HANDSTATEFLAG_BUSY == 0 {
                    if let Some(script) = self.func_of(h).and_then(|f| f.fire_animation) {
                        self.bgun_start_animation(script, h);
                        restartedanim = true;
                    }
                }
                if !restartedanim && self.b.hands[h].stateframes > 25 {
                    playsound = true;
                }
            } else if self.bgun_anim_allows_feature(h, GUNFEATURE_CLICK) {
                playsound = true;
            }
        } else if self.b.hands[h].stateframes > 25 {
            playsound = true;
            self.b.hands[h].stateframes -= 25;
            self.b.hands[h].stateflags = 0;
            self.bgun_reset_anim(h);
        }
        self.b.hands[h].mode = HANDMODE_13;
        self.b.hands[h].count60 = 0;
        self.b.hands[h].count = 0;
        if playsound && self.b.hands[h].stateflags & HANDSTATEFLAG_BUSY == 0 {
            self.b.hands[h].stateflags |= HANDSTATEFLAG_BUSY;
            match weaponnum {
                WEAPON_PHOENIX | WEAPON_CALLISTO | WEAPON_FARSIGHT => {
                    // The Maian weapons: a wet click, then (falling through) the fast click.
                    self.sound(0x8080, 2.07);
                    self.sound(SFXMAP_8052_FIREEMPTY, 1.5);
                }
                WEAPON_TRANQUILIZER => self.sound(SFXMAP_8052_FIREEMPTY, 1.5),
                WEAPON_UNARMED | WEAPON_COMBATKNIFE | WEAPON_GRENADE | WEAPON_NBOMB | WEAPON_TIMEDMINE | WEAPON_PROXIMITYMINE | WEAPON_REMOTEMINE | WEAPON_COMBATBOOST => {}
                _ => self.sound(SFXMAP_8052_FIREEMPTY, 1.0),
            }
        }
        if !self.b.hands[h].triggeron {
            self.b.hands[h].mode = HANDMODE_NONE;
            self.b.hands[h].count60 = 0;
            self.b.hands[h].count = 0;
            if self.bgun_set_state(h, HANDSTATE_IDLE) {
                return lvupdate;
            }
            self.bgun_reset_anim(h);
        }
        0
    }

    /// `bgun_tick_inc_attack` (`:2525`).
    fn bgun_tick_inc_attack(&mut self, h: usize, lvupdate: i32) -> i32 {
        let mut finished = true;
        if let Some(func) = self.func_of(h) {
            finished = match func.kind() {
                INVENTORYFUNCTYPE_SHOOT => self.bgun_tick_inc_attacking_shoot(h),
                INVENTORYFUNCTYPE_THROW => self.bgun_tick_inc_attacking_throw(h),
                INVENTORYFUNCTYPE_MELEE => self.bgun_tick_inc_attacking_melee(h),
                INVENTORYFUNCTYPE_SPECIAL => self.bgun_tick_inc_attacking_special(h),
                _ => true,
            };
        }
        if finished {
            if self.b.hands[h].weaponnum == WEAPON_REAPER && self.b.hands[h].triggeron {
                self.b.hands[h].weaponfunc = FUNC_SECONDARY;
                finished = false;
            }
            if finished && self.bgun_set_state(h, HANDSTATE_IDLE) {
                return lvupdate;
            }
        }
        0
    }

    /// `bgun_is_ready_to_switch` (`:2570`).
    pub(crate) fn bgun_is_ready_to_switch(&self, h: usize) -> bool {
        let r = &self.b.hands[HAND_RIGHT];
        let l = &self.b.hands[HAND_LEFT];
        if h == HAND_RIGHT && l.inuse && l.state == HANDSTATE_AUTOSWITCH && l.stateminor == 0 {
            return false;
        }
        if self.b.ctrl.switchtoweaponnum.is_some() {
            return true;
        }
        if h == HAND_LEFT {
            if r.state == HANDSTATE_RELOAD || r.state == HANDSTATE_CHANGEFUNC || r.state == HANDSTATE_ATTACK {
                return false;
            }
            if l.inuse && !self.b.ctrl.dualwielding {
                return true;
            }
            if !l.inuse && self.b.ctrl.dualwielding {
                return true;
            }
        }
        false
    }

    /// `bgun_can_free_weapon` (`:2620`).
    fn bgun_can_free_weapon(&self, h: usize) -> bool {
        let hand = &self.b.hands[h];
        hand.state == HANDSTATE_CHANGEGUN && hand.stateminor == HANDSTATEMINOR_CHANGEGUN_LOAD && hand.count >= 3 && !self.b.ctrl.throwing
    }

    /// `bgun0f09bf44` (`:2634`): may the new gun come up?
    fn bgun_may_raise(&self, h: usize) -> bool {
        let mut result = self.bgun_is_loaded();
        if self.b.ctrl.switchtoweaponnum.is_some() {
            result = false;
        }
        if h == HAND_LEFT && self.b.ctrl.dualwielding != self.b.hands[h].inuse {
            result = false;
        }
        if self.b.ctrl.gunmemnew.is_some() {
            result = false;
        }
        if self.b.hands[1 - h].state == HANDSTATE_RELOAD {
            result = false;
        }
        result
    }

    /// `bgun_tick_inc_changegun` (`:2662`).
    fn bgun_tick_inc_changegun(&mut self, h: usize, lvupdate: i32) -> i32 {
        let weaponnum = self.b.hands[h].weaponnum;
        let w = self.gset.weapon(weaponnum).cloned();
        if self.b.hands[h].statecycles == 0 {
            self.b.hands[h].pausetime60 = 0;
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_CHANGEGUN_UNEQUIP {
            let mut skipanim = false;
            if self.gset.has_flag(weaponnum, WEAPONFLAG_THROWABLE) && !(weaponnum == WEAPON_REMOTEMINE && h == HAND_LEFT) && self.bgun_get_ammo_state(FUNC_PRIMARY, h) <= GUNAMMOSTATE_DEPLETED {
                skipanim = true;
            }
            self.b.hands[h].count = 0;
            if !skipanim {
                let unequip = w.as_ref().and_then(|w| w.unequip_animation).filter(|_| self.b.hands[h].inuse && !(self.b.hands[h].ejectstate != EJECTSTATE_INACTIVE && self.b.hands[h].ejecttype == EJECTTYPE_GUN));
                if let Some(unequip) = unequip {
                    if self.b.hands[h].statecycles == 0 {
                        self.bgun_start_animation(unequip, h);
                    } else if self.b.hands[h].animmode == HANDANIMMODE_IDLE {
                        self.b.hands[h].stateminor += 1;
                    }
                } else {
                    self.b.hands[h].stateflags |= HANDSTATEFLAG_00000001;
                    if self.b.hands[h].ejectstate == EJECTSTATE_INIT {
                        return 0;
                    }
                    self.b.hands[h].stateminor += 1;
                }
            } else {
                self.b.hands[h].stateminor += 1;
            }
            if self.b.hands[h].stateminor == HANDSTATEMINOR_CHANGEGUN_LOWER {
                self.b.hands[h].stateframes = 0;
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_CHANGEGUN_LOWER {
            let mut delay = if NORMMPLAYERISRUNNING { 12 } else { 16 };
            self.b.hands[h].count = 0;
            if w.as_ref().is_some_and(|w| w.unequip_animation.is_some()) && self.b.hands[h].stateflags & HANDSTATEFLAG_00000001 == 0 {
                delay = 1;
            }
            if !self.b.hands[h].inuse {
                delay = 1;
            }
            let throwing = self.b.ctrl.throwing || (self.b.hands[h].ejecttype == EJECTTYPE_GUN && (self.b.hands[h].ejectstate == EJECTSTATE_INIT || self.b.hands[h].ejectstate == EJECTSTATE_AIRBORNE));
            if self.b.hands[h].stateframes >= delay {
                if !throwing {
                    self.b.events.push(GunEvent::FreeHeldRocket { hand: h });
                    self.b.hands[h].mode = HANDMODE_6;
                    self.b.hands[h].stateminor += 1;
                } else {
                    self.bgun_set_arm_pitch(h, max_pitch());
                    // The Laptop deploy etc.: the lowered gun is thrown as the secondary.
                    if self.b.ctrl.throwing && self.b.hands[h].inuse {
                        let hand = &mut self.b.hands[h];
                        hand.firing = true;
                        hand.attacktype = HANDATTACKTYPE_THROWPROJECTILE;
                        hand.weaponfunc = FUNC_SECONDARY;
                    }
                }
            } else {
                let a = self.b.hands[h].stateframes as f32 * max_pitch() / delay as f32;
                self.bgun_set_arm_pitch(h, a);
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_CHANGEGUN_LOAD {
            self.b.hands[h].animmode = HANDANIMMODE_IDLE;
            if self.b.hands[h].pausechange == 0 || self.b.hands[h].pausetime60 <= self.b.hands[h].count60 {
                if self.b.hands[h].mode == HANDMODE_6 {
                    if self.bgun_may_raise(h) {
                        self.b.hands[h].mode = HANDMODE_7;
                        if !self.b.hands[h].inuse && self.bgun_set_state(h, HANDSTATE_IDLE) {
                            return lvupdate;
                        }
                    }
                } else if self.bgun_is_loaded() {
                    // The new weapon's definition: bgun_tick_switch2 updated the
                    // hand's gset while we waited.
                    let neww = self.gset.weapon(self.b.hands[h].weaponnum).cloned();
                    if let Some(script) = neww.as_ref().and_then(|w| w.equip_animation) {
                        self.bgun_start_animation(script, h);
                        self.b.hands[h].unk0cc8_02 = true;
                    }
                    let hand = &mut self.b.hands[h];
                    hand.mode = HANDMODE_EQUIP;
                    hand.stateminor += 1;
                    hand.count60 = 0;
                    hand.count = 0;
                }
            }
            if self.b.hands[h].mode == HANDMODE_6 || self.b.hands[h].mode == HANDMODE_7 {
                self.bgun_set_arm_pitch(h, max_pitch());
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_CHANGEGUN_RAISE {
            let weaponnum = self.b.hands[h].weaponnum;
            let neww = self.gset.weapon(weaponnum).cloned();
            let mut delay = if NORMMPLAYERISRUNNING { 12 } else { 23 };
            if self.gset.has_flag(weaponnum, WEAPONFLAG_00004000) {
                self.b.hands[h].animmode = HANDANIMMODE_IDLE;
            } else if neww.as_ref().is_some_and(|w| w.equip_animation.is_some()) {
                delay = 1;
            }
            if self.b.hands[h].count == 0 {
                self.bgun_load_all_clips(h);
                if self.gset.has_flag(weaponnum, WEAPONFLAG_THROWABLE)
                    && (weaponnum != WEAPON_REMOTEMINE || h != HAND_LEFT)
                    && self.bgun_get_ammo_state(FUNC_PRIMARY, h) <= GUNAMMOSTATE_DEPLETED
                    && self.bgun_set_state(h, HANDSTATE_AUTOSWITCH)
                {
                    self.b.hands[h].stateminor = HANDSTATEMINOR_AUTOSWITCH_DELETE;
                    return lvupdate;
                }
                self.b.p.doautoselect = false;
                if !self.pl.isdead {
                    match weaponnum {
                        WEAPON_TRANQUILIZER => self.sound(SFXNUM_00E8_PICKUP_GUN, 1.5),
                        WEAPON_REAPER => self.sound(SFXNUM_00E8_PICKUP_GUN, 0.85),
                        WEAPON_LASER => self.sound(0x00f2, 1.0),
                        WEAPON_COMBATKNIFE => self.sound(0x00e9, 1.0),
                        WEAPON_REMOTEMINE | WEAPON_TIMEDMINE | WEAPON_PROXIMITYMINE => {
                            if h == HAND_RIGHT || weaponnum != WEAPON_REMOTEMINE {
                                self.sound(0x00eb, 1.0)
                            }
                        }
                        WEAPON_NONE | WEAPON_UNARMED | WEAPON_LAPTOPGUN | WEAPON_CROSSBOW | WEAPON_GRENADE | WEAPON_NBOMB | WEAPON_COMBATBOOST => {}
                        _ => self.sound(SFXNUM_00E8_PICKUP_GUN, 1.0),
                    }
                }
            }
            if self.b.hands[h].count60 >= delay || !self.gset.has_model(weaponnum) || !self.gset.has_flag(weaponnum, WEAPONFLAG_00000040) || self.gset.has_flag(weaponnum, WEAPONFLAG_00000080) {
                let hand = &mut self.b.hands[h];
                hand.mode = HANDMODE_NONE;
                hand.stateminor += 1;
                if !self.gset.has_flag(weaponnum, WEAPONFLAG_00004000) {
                    self.b.hands[h].unk0cc8_02 = false;
                }
                self.b.hands[h].count60 = 0;
                self.b.hands[h].count = 0;
            } else {
                let a = (delay - self.b.hands[h].count60) as f32 * max_pitch() / delay as f32;
                self.bgun_set_arm_pitch(h, a);
            }
        }
        if self.b.hands[h].stateminor == HANDSTATEMINOR_CHANGEGUN_EQUIP {
            let weaponnum = self.b.hands[h].weaponnum;
            let has_equip = self.gset.weapon(weaponnum).is_some_and(|w| w.equip_animation.is_some());
            if has_equip && !self.gset.has_flag(weaponnum, WEAPONFLAG_00004000) {
                if self.b.hands[h].animmode == HANDANIMMODE_IDLE && self.bgun_set_state(h, HANDSTATE_IDLE) {
                    return lvupdate;
                }
            } else if self.bgun_set_state(h, HANDSTATE_IDLE) {
                return lvupdate;
            }
        }
        0
    }

    /// `bgun_tick_inc` (`:3018`).
    fn bgun_tick_inc(&mut self, h: usize, lvupdate: i32) -> i32 {
        let prevstate = self.b.hands[h].state;
        {
            let (lv240, lv60) = (self.lv.lvupdate240, self.lv.lvupdate60);
            let hand = &mut self.b.hands[h];
            hand.firing = false;
            hand.flashon = false;
            hand.stateframes += lvupdate;
            if lv240 > 0 {
                hand.count60 += lv60;
                hand.count += 1;
            }
            hand.useposrot = false;
        }
        let result = match self.b.hands[h].state {
            HANDSTATE_IDLE => self.bgun_tick_inc_idle(h, lvupdate),
            HANDSTATE_RELOAD => self.bgun_tick_inc_reload(h, lvupdate),
            HANDSTATE_ATTACK => self.bgun_tick_inc_attack(h, lvupdate),
            HANDSTATE_CHANGEGUN => self.bgun_tick_inc_changegun(h, lvupdate),
            HANDSTATE_ATTACKEMPTY => self.bgun_tick_inc_attackempty(h, lvupdate),
            HANDSTATE_AUTOSWITCH => self.bgun_tick_inc_autoswitch(h, lvupdate),
            HANDSTATE_CHANGEFUNC => self.bgun_tick_inc_changefunc(h, lvupdate),
            _ => 0,
        };
        let hand = &mut self.b.hands[h];
        hand.statelastframe = hand.stateframes;
        if hand.state != prevstate {
            hand.statelastframe = -result;
        } else {
            hand.stateframes -= result;
            hand.statecycles += 1;
        }
        result
    }

    /// `bgun_set_state` (`:3074`).
    pub(crate) fn bgun_set_state(&mut self, h: usize, state: i32) -> bool {
        if state == HANDSTATE_CHANGEFUNC && self.func_by(h, 1 - self.b.hands[h].weaponfunc).is_none() {
            return false;
        }
        let hand = &mut self.b.hands[h];
        hand.state = state;
        hand.stateframes = 0;
        hand.stateflags = 0;
        hand.statecycles = 0;
        hand.stateminor = 0;
        hand.statelastframe = 0;
        true
    }

    /// `bgun_tick_hand` (`:3096`).
    fn bgun_tick_hand(&mut self, h: usize) {
        let mut lvupdate = self.lv.lvupdate60;
        self.b.hands[h].animframeinc = self.lv.lvupdate60;
        let mut i = 20;
        while i >= 0 {
            lvupdate = self.bgun_tick_inc(h, lvupdate);
            i -= 1;
            if lvupdate <= 0 {
                break;
            }
        }
    }

    // ─── the gun memory (3440-4122) ──────────────────────────────────────────

    /// `bgun_is_loaded` (`:3440`).
    pub fn bgun_is_loaded(&self) -> bool {
        self.b.ctrl.gunmemtype == WEAPON_NONE || (self.b.ctrl.gunmemnew.is_none() && self.b.ctrl.load_steps <= 0)
    }

    /// `bgun_set_gun_mem_weapon` (`:3491`): start loading a new gun.
    ///
    /// SUBST: PD streams the hand model, the gun model, its textures (three a
    /// call) and the cartridge model into gun memory over several
    /// `bgun_tick_master_load` calls, one per 8 quarter-ticks (`bgun_tick_load`,
    /// `:4059`) / the files are already loaded, so the same number of steps is
    /// counted from them and the switch waits exactly as long.
    fn bgun_set_gun_mem_weapon(&mut self, weaponnum: u8) {
        self.b.ctrl.gunmemnew = Some(weaponnum);
        let mut steps = 1; // MASTERLOADSTATE_FLUX -> HANDS
        let stem = self.gset.weapon(weaponnum).and_then(|w| w.model.clone());
        let hashands = self.gset.has_flag(weaponnum, WEAPONFLAG_HASHANDS);
        if hashands && self.b.ctrl.handfilenum != self.b.hand_model {
            let hand = self.b.hand_model.clone();
            steps += self.load_steps_for(&hand);
        }
        steps += 1; // HANDS -> GUN
        if let Some(stem) = &stem {
            steps += self.load_steps_for(stem);
        }
        steps += 2; // CARTS: a cartridge model and the compile step
        self.b.ctrl.load_steps = steps;
    }

    /// GUNLOADSTATE_MODEL (1) + ceil(textures / 3) + GUNLOADSTATE_DLS (1).
    fn load_steps_for(&self, stem: &str) -> i32 {
        let ntex = self.models.get(stem).map_or(0, |m| m.textures.len()) as i32;
        2 + (ntex + 2) / 3
    }

    /// `bgun_tick_load` (`:4059`) + `bgun_tick_master_load`, counted.
    pub(crate) fn bgun_tick_load(&mut self) {
        let Some(w) = self.b.ctrl.gunmemnew else { return };
        let mut i = 0;
        while i < self.lv.lvupdate240 {
            self.b.ctrl.load_steps -= 1;
            i += 8;
        }
        if self.b.ctrl.load_steps <= 0 {
            if self.gset.has_flag(w, WEAPONFLAG_HASHANDS) {
                self.b.ctrl.handfilenum = self.b.hand_model.clone();
            }
            self.b.ctrl.gunmemtype = w;
            self.b.ctrl.gunmemnew = None;
            self.b.ctrl.load_steps = 0;
            self.instantiate_models();
        }
    }

    /// `model_init` of both hands' gun and hand models once loaded (`:3998`).
    fn instantiate_models(&mut self) {
        let w = self.b.ctrl.gunmemtype;
        let gundef = self.gset.weapon(w).and_then(|w| w.model.clone()).and_then(|s| self.models.get(&s).ok());
        let handdef = if self.gset.has_flag(w, WEAPONFLAG_HASHANDS) { self.models.get(&self.b.hand_model).ok() } else { None };
        for h in 0..2 {
            self.b.hands[h].gunmodel = gundef.clone().map(Model::new);
            self.b.hands[h].handmodel = handdef.clone().map(Model::new);
        }
    }

    // ─── switching (5228-5803) ───────────────────────────────────────────────

    /// `bgun_free_weapon` (`:5228`): loaded rounds go back into the reserve.
    fn bgun_free_weapon(&mut self, h: usize) {
        if self.b.hands[h].inuse {
            for i in 0..2 {
                if self.b.ctrl.ammotypes[i] >= 0 {
                    let spaceinclip = self.b.hands[h].clipsizes[i] - self.b.hands[h].loadedammo[i];
                    let index = match self.b.ctrl.weaponnum {
                        WEAPON_CROSSBOW => 0,
                        WEAPON_SHOTGUN => 1,
                        WEAPON_DY357MAGNUM => 2,
                        WEAPON_DY357LX => 3,
                        _ => -1,
                    };
                    if index != -1 {
                        self.b.hands[h].gunroundsspent[index as usize] = ((spaceinclip << 8) | 0xff) as u16;
                    }
                    if self.b.hands[h].loadedammo[i] > 0 {
                        let t = self.b.ctrl.ammotypes[i] as usize;
                        self.b.p.ammoheldarr[t] += self.b.hands[h].loadedammo[i];
                    }
                    self.b.hands[h].loadedammo[i] = 0;
                }
            }
        }
        // bondgun.c:5262. Without it the launcher's rocket, recreated by the
        // lowering frame's pose, rode along on the next gun.
        self.b.events.push(GunEvent::FreeHeldRocket { hand: h });
    }

    /// `bgun_tick_switch2` (`:5265`).
    fn bgun_tick_switch2(&mut self) {
        if let Some(s) = self.b.ctrl.switchtoweaponnum {
            if self.bgun_can_free_weapon(HAND_RIGHT) && self.bgun_can_free_weapon(HAND_LEFT) {
                let weaponnum = self.b.ctrl.weaponnum;
                let previnuse = self.b.hands[HAND_LEFT].inuse;
                if self.b.ctrl.dualwielding && !self.b.inv_has_double(s) {
                    self.b.ctrl.dualwielding = false;
                }
                self.bgun_free_weapon(HAND_LEFT);
                self.bgun_free_weapon(HAND_RIGHT);
                if s == WEAPON_NONE {
                    self.b.hands[HAND_LEFT].inuse = false;
                    self.b.hands[HAND_RIGHT].inuse = false;
                    self.b.ctrl.weaponnum = WEAPON_NONE;
                } else {
                    self.bgun_set_gun_mem_weapon(s);
                    self.b.ctrl.weaponnum = s;
                    self.b.hands[HAND_LEFT].inuse = true;
                    self.b.hands[HAND_RIGHT].inuse = true;
                }
                if self.b.ctrl.weaponnum == WEAPON_REMOTEMINE {
                    self.b.ctrl.dualwielding = true;
                }
                if !self.b.ctrl.dualwielding {
                    self.b.hands[HAND_LEFT].inuse = false;
                }
                if (WEAPON_UNARMED..=WEAPON_PSYCHOSISGUN).contains(&weaponnum) {
                    self.b.ctrl.prevweaponnum = weaponnum;
                }
                self.b.ctrl.prevwasdualwielding = previnuse;
                self.b.ctrl.invertgunfunc = false;
                for i in 0..2 {
                    let wn = self.b.ctrl.weaponnum;
                    let hand = &mut self.b.hands[i];
                    hand.ejectstate = EJECTSTATE_INACTIVE;
                    hand.ejecttype = EJECTTYPE_GUN;
                    hand.unk0d0f_02 = false;
                    hand.activatesecondary = false;
                    hand.matmot1 = 0.0;
                    hand.matmot2 = 0.0;
                    hand.matmot3 = 0.0;
                    hand.gunsmokepoint = 0.0;
                    hand.burstbullets = 0;
                    hand.loadslide = 0.0;
                    hand.allowshootframe = 0;
                    hand.lastshootframe60 = 0;
                    hand.weaponfunc = FUNC_PRIMARY;
                    hand.weaponnum = wn;
                    hand.gangstarot = 0.0;
                    self.bgun_init_clips(i);
                    self.b.hands[i].anim = Anim::default();
                    if self.b.hands[i].audiohandle {
                        self.b.hands[i].audiohandle = false;
                        self.b.events.push(GunEvent::StopSound { hand: i });
                    }
                }
                self.b.ctrl.switchtoweaponnum = None;
                self.b.ctrl.throwing = false;
            }
        } else if ((self.b.hands[HAND_LEFT].inuse && !self.b.ctrl.dualwielding) || (!self.b.hands[HAND_LEFT].inuse && self.b.ctrl.dualwielding)) && self.bgun_can_free_weapon(HAND_LEFT) {
            self.bgun_free_weapon(HAND_LEFT);
            self.b.hands[HAND_LEFT].inuse = self.b.ctrl.dualwielding;
        }
    }

    /// `bgun_get_switch_to_weapon` (`:5454`).
    fn bgun_get_switch_to_weapon(&self, h: usize) -> u8 {
        let mut w = self.b.ctrl.switchtoweaponnum.unwrap_or(self.b.ctrl.weaponnum);
        if !self.b.ctrl.dualwielding && h == HAND_LEFT {
            w = WEAPON_NONE;
        }
        w
    }

    /// `inv_choose_cycle_forward_weapon`'s order over the inventory: unarmed,
    /// then each weapon once, a weapon held twice offered single then dual
    /// (`inv.c`).
    fn cycle_list(&self) -> Vec<(u8, bool)> {
        let mut out = vec![(WEAPON_UNARMED, false)];
        for (w, dual) in &self.b.p.inventory {
            if *w == WEAPON_UNARMED {
                continue;
            }
            out.push((*w, false));
            if *dual {
                out.push((*w, true));
            }
        }
        out
    }

    /// `bgun_cycle_forward` / `bgun_cycle_back` (`:5494`, `:5521`).
    pub fn bgun_cycle(&mut self, forward: bool) {
        let cur = (self.bgun_get_switch_to_weapon(HAND_RIGHT), self.bgun_get_switch_to_weapon(HAND_LEFT) != WEAPON_NONE);
        let list = self.cycle_list();
        let idx = list.iter().position(|e| *e == cur).unwrap_or(0) as i32;
        let n = list.len() as i32;
        let next = list[((idx + if forward { 1 } else { -1 }).rem_euclid(n)) as usize];
        self.b.ctrl.dualwielding = next.1;
        self.b.bgun_equip_weapon(next.0);
    }

    /// `bgun_auto_switch_weapon` (`:5669`).
    pub(crate) fn bgun_auto_switch_weapon(&mut self) {
        const PRIMARY: [u8; 35] = [
            WEAPON_RCP120,
            WEAPON_SUPERDRAGON,
            WEAPON_K7AVENGER,
            WEAPON_AR34,
            WEAPON_CALLISTO,
            WEAPON_LAPTOPGUN,
            WEAPON_DRAGON,
            WEAPON_CMP150,
            WEAPON_CYCLONE,
            WEAPON_FARSIGHT,
            WEAPON_SHOTGUN,
            WEAPON_REAPER,
            WEAPON_DY357LX,
            WEAPON_MAULER,
            WEAPON_DY357MAGNUM,
            WEAPON_MAGSEC4,
            WEAPON_PHOENIX,
            WEAPON_FALCON2_SCOPE,
            WEAPON_FALCON2,
            WEAPON_FALCON2_SILENCER,
            WEAPON_SNIPERRIFLE,
            WEAPON_CROSSBOW,
            WEAPON_TRANQUILIZER,
            WEAPON_LASER,
            WEAPON_SUPERDRAGON,
            WEAPON_DEVASTATOR,
            WEAPON_ROCKETLAUNCHER,
            WEAPON_SLAYER,
            WEAPON_GRENADE,
            WEAPON_NBOMB,
            WEAPON_PROXIMITYMINE,
            WEAPON_TIMEDMINE,
            WEAPON_REMOTEMINE,
            WEAPON_COMBATKNIFE,
            WEAPON_UNARMED,
        ];
        let cur = self.b.ctrl.weaponnum;
        let mut newweaponnum: Option<u8> = None;
        let mut firstweaponnum: Option<u8> = None;
        let mut foundsuperdragon = false;
        let mut foundcurrent = false;
        let mut i = 0;
        loop {
            let wn = PRIMARY[i];
            if self.b.inv_has_single(wn) {
                let mut usable = false;
                let f = self.gset.func(wn, FUNC_PRIMARY);
                if !self.bgun_func_unusable(f, wn) && f.is_some_and(|f| f.flags & FUNCFLAG_AUTOSWITCHUNSELECTABLE == 0) {
                    usable = true;
                }
                if wn == WEAPON_SUPERDRAGON && !foundsuperdragon {
                    foundsuperdragon = true;
                } else {
                    let f = self.gset.func(wn, FUNC_SECONDARY);
                    if !self.bgun_func_unusable(f, wn) && f.is_some_and(|f| f.flags & FUNCFLAG_AUTOSWITCHUNSELECTABLE == 0) {
                        usable = true;
                    }
                }
                if wn == cur {
                    foundcurrent = true;
                } else if usable {
                    newweaponnum = Some(wn);
                    if firstweaponnum.is_none() {
                        firstweaponnum = Some(wn);
                    }
                }
            }
            i += 1;
            if i >= PRIMARY.len() || (newweaponnum.is_some() && foundcurrent) {
                break;
            }
        }
        if !foundcurrent {
            newweaponnum = firstweaponnum;
        }
        let newweaponnum = newweaponnum.unwrap_or(WEAPON_UNARMED);
        if newweaponnum != cur {
            self.b.ctrl.dualwielding = self.b.inv_has_double(newweaponnum);
            self.b.bgun_equip_weapon(newweaponnum);
        }
    }

    /// `bgun_switch_to_previous` (`:5471`, NTSC 1.0+).
    pub fn bgun_switch_to_previous(&mut self) {
        let prev = self.b.ctrl.prevweaponnum;
        if self.b.inv_has_single(prev) {
            self.b.ctrl.dualwielding = self.b.inv_has_double(prev) && self.b.ctrl.prevwasdualwielding;
            self.b.bgun_equip_weapon(prev);
        } else {
            self.bgun_auto_switch_weapon();
        }
    }

    /// `bgun_start_detonate_animation` (`:6396`): the left hand's detonator
    /// press (`var80070200`: ANIM_0434 at full speed).
    pub fn bgun_start_detonate_animation(&mut self) {
        if self.b.hands[HAND_LEFT].weaponnum == WEAPON_REMOTEMINE {
            if let Some(script) = self.gset.detonate_script {
                self.bgun_start_animation(script, HAND_LEFT);
            }
        }
    }

    /// `bgun_start_slide` (`:5868`).
    pub(crate) fn bgun_start_slide(&mut self, h: usize) {
        self.b.hands[h].slideinc = true;
    }

    /// `bgun_update_slide` (`:5879`).
    pub(crate) fn bgun_update_slide(&mut self, h: usize) {
        let slidemax = self.func_of(h).and_then(|f| f.shoot).map_or(0.0, |s| s.slidemax);
        let lv = self.lv.lvupdate60freal;
        if self.b.hands[h].slideinc {
            let hand = &mut self.b.hands[h];
            if hand.slidetrans < slidemax {
                hand.slidetrans += slidemax * 0.25 * lv;
            }
            if hand.slidetrans >= slidemax {
                hand.slidetrans = slidemax;
                hand.slideinc = false;
            }
        } else if self.b.hands[h].loadedammo[FUNC_PRIMARY] > 0 && self.bgun_anim_allows_feature(h, GUNFEATURE_ATTACKAGAIN) {
            let hand = &mut self.b.hands[h];
            if hand.slidetrans > 0.0 {
                hand.slidetrans -= slidemax * 0.166_666_67 * lv;
            }
            if hand.slidetrans < 0.0 {
                hand.slidetrans = 0.0;
            }
        }
    }

    /// `bgun0f0abd30` (`:10413`): the clip sizes of a newly equipped weapon.
    pub(crate) fn bgun_init_clips(&mut self, h: usize) {
        let w = self.gset.weapon(self.b.hands[h].weaponnum).cloned();
        for i in 0..2 {
            if h == HAND_RIGHT {
                self.b.ctrl.ammotypes[i] = -1;
            }
            if let Some(a) = w.as_ref().and_then(|w| w.ammos[i].as_ref()) {
                if h == HAND_RIGHT {
                    self.b.ctrl.ammotypes[i] = a.ammotype;
                }
                self.b.hands[h].clipsizes[i] = a.clipsize;
                if h == HAND_LEFT && self.b.hands[h].weaponnum == WEAPON_REMOTEMINE {
                    self.b.hands[h].clipsizes[i] = 0;
                }
                self.b.hands[h].loadedammo[i] = 0;
            }
        }
        self.b.hands[h].upgrademult = [1.0; 2];
        self.b.hands[h].finalmult = [1.0; 2];
    }

    // ─── functions and the trigger (8937-9230) ───────────────────────────────

    /// `bgun_set_trigger_on` (`:8937`).
    fn bgun_set_trigger_on(&mut self, h: usize, on: bool) {
        let hand = &mut self.b.hands[h];
        hand.triggerprev = hand.triggeron;
        hand.triggeron = on;
        if !on {
            hand.triggerreleased = true;
        }
    }

    fn set_func(&mut self, secondary: bool) {
        let w = self.b.ctrl.weaponnum;
        if !(WEAPON_UNARMED..=WEAPON_COMBATBOOST).contains(&w) {
            return;
        }
        let (i, bit) = (((w - 1) >> 3) as usize, 1u8 << ((w - 1) & 7));
        if secondary {
            self.b.p.gunfuncs[i] |= bit;
        } else {
            self.b.p.gunfuncs[i] &= !bit;
        }
    }

    /// `bgun_consider_toggle_gun_function` (`:8963`).
    pub fn bgun_consider_toggle_gun_function(&mut self, usedowntime: i32, trigpressed: bool) -> i32 {
        match self.bgun_get_weapon_num(HAND_RIGHT) {
            WEAPON_SNIPERRIFLE => {
                self.b.ctrl.invertgunfunc = true;
                if trigpressed {
                    return USETIMER_STOP;
                }
                if usedowntime < 50 || self.b.hands[HAND_RIGHT].weaponfunc != FUNC_SECONDARY {
                    return USETIMER_CONTINUE;
                }
                self.b.hands[HAND_RIGHT].activatesecondary = true;
                USETIMER_REPEAT
            }
            WEAPON_RCP120 | WEAPON_LAPTOPGUN | WEAPON_DRAGON | WEAPON_REMOTEMINE => {
                self.b.ctrl.invertgunfunc = true;
                USETIMER_STOP
            }
            WEAPON_MAULER | WEAPON_CMP150 | WEAPON_K7AVENGER | WEAPON_AR34 | WEAPON_FARSIGHT | WEAPON_TIMEDMINE => {
                if !trigpressed {
                    let sec = self.funcissec();
                    self.set_func(!sec);
                    return USETIMER_STOP;
                }
                USETIMER_CONTINUE
            }
            _ => {
                if trigpressed {
                    self.b.ctrl.invertgunfunc = true;
                } else {
                    let sec = self.funcissec();
                    self.set_func(!sec);
                }
                USETIMER_STOP
            }
        }
    }

    /// `bgun_tick_gameplay` (`:9073`): the trigger's routing (with dual-wield
    /// alternation) and both hands' state machines.
    pub fn bgun_tick_gameplay(&mut self, triggeron: bool) {
        let lv240 = self.lv.lvupdate240;
        let mut gunsfiring = [false, false];
        let p = &mut self.b.p;
        p.playertriggerprev = p.playertriggeron;
        p.playertriggeron = triggeron;
        if !triggeron && p.playertriggerprev {
            p.doautoselect = true;
        }
        if self.b.p.playertriggeron {
            self.b.p.playertrigtime240 += lv240;
            let cur = self.b.p.curguntofire;
            if self.b.hands[HAND_LEFT].inuse && self.b.hands[HAND_RIGHT].inuse && self.b.ctrl.weaponnum != WEAPON_REMOTEMINE {
                if self.b.p.playertrigtime240 > 80 {
                    gunsfiring[cur] = true;
                    if self.bgun_clip_has_ammo(1 - cur) || self.b.hands[1 - cur].triggeron {
                        gunsfiring[1 - cur] = true;
                    }
                } else {
                    if !self.b.p.playertriggerprev && (self.bgun_clip_has_ammo(1 - cur) || !self.bgun_clip_has_ammo(cur)) {
                        self.b.p.curguntofire = 1 - cur;
                    }
                    let cur = self.b.p.curguntofire;
                    gunsfiring[cur] = true;
                    gunsfiring[1 - cur] = false;
                }
            } else {
                if !self.b.hands[cur].inuse && self.b.hands[1 - cur].inuse {
                    self.b.p.curguntofire = 1 - cur;
                }
                if self.b.ctrl.weaponnum == WEAPON_REMOTEMINE {
                    self.b.p.curguntofire = 0;
                }
                let cur = self.b.p.curguntofire;
                gunsfiring[cur] = true;
                gunsfiring[1 - cur] = false;
            }
        } else {
            self.b.p.playertrigtime240 = 0;
        }
        self.bgun_set_trigger_on(HAND_RIGHT, gunsfiring[0]);
        self.bgun_set_trigger_on(HAND_LEFT, gunsfiring[1]);

        if lv240 > 0 {
            self.bgun_tick_hand(HAND_RIGHT);
            self.bgun_tick_hand(HAND_LEFT);
            self.bgun_tick_switch2();
            if self.b.p.unlimited_ammo {
                // bgun_give_max_ammo(false)
                for t in 1..AMMO_CAPACITY.len() {
                    self.b.p.ammoheldarr[t] = AMMO_CAPACITY[t];
                }
            }
        }
    }

    /// `bgun_tick_mauler_charge` (`:7887`).
    pub(crate) fn bgun_tick_mauler_charge(&mut self) {
        let lv = self.lv.lvupdate60freal;
        let lv240 = self.lv.lvupdate240;
        for i in 0..2 {
            if !self.b.hands[i].inuse {
                continue;
            }
            let mut charging = false;
            if self.bgun_is_reloading(i) {
                self.b.hands[i].matmot1 = 0.0;
            } else if self.b.hands[i].weaponfunc == FUNC_SECONDARY {
                let hand = &mut self.b.hands[i];
                let oldvalue = hand.matmot1 as i32;
                if hand.loadedammo[0] >= 2 && hand.matmot1 < 5.0 {
                    charging = true;
                    hand.matmot1 += lv * 0.05;
                }
                if hand.matmot1 > 5.0 {
                    hand.matmot1 = 5.0;
                }
                let newvalue = hand.matmot1 as i32;
                if oldvalue != newvalue && hand.loadedammo[0] >= 2 {
                    hand.loadedammo[0] -= 1;
                }
            } else {
                let hand = &mut self.b.hands[i];
                hand.matmot1 -= lv * 0.005;
                if hand.matmot1 < 0.0 {
                    hand.matmot1 = 0.0;
                }
            }
            if !self.b.hands[i].audiohandle && self.b.hands[i].matmot1 > 0.1 && charging && lv240 != 0 {
                self.b.hands[i].audiohandle = true;
                self.b.events.push(GunEvent::Sound { id: 0x8065, speed: 0.5, handle: Some(i) });
            }
            if self.b.hands[i].audiohandle && (self.b.hands[i].matmot1 < 0.1 || !charging) {
                self.b.hands[i].audiohandle = false;
                self.b.events.push(GunEvent::StopSound { hand: i });
            }
        }
    }

    /// `gset_get_xpos` (`gset.c:155`).
    pub(crate) fn gset_get_xpos(&self, h: usize) -> f32 {
        let x = self.gset.weapon(self.bgun_get_weapon_num(h)).map_or(0.0, |w| w.posx);
        if h == HAND_RIGHT {
            x
        } else {
            -x
        }
    }
}
