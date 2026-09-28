//! `bondmove.c`: controls → look and turn speeds, aim mode, crouch requests, the
//! gun's controls (the trigger, B's function toggle and reload, A's weapon
//! cycling, the manual zoom), the crosshair swivel, the view basis.

use glam::Vec3;
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::math::{self, baddtor2};
use pd_core::rng::Rng;

use super::{Player, PlayerInput, PLAYER_DEFAULT_FOV};
use crate::world::WorldRes;

/// `bmove_dampen_shotspeed` (`bondmove.c:1841`): the shove from a hit dies away.
pub fn bmove_dampen_shotspeed(s: &mut Vec3, lvupdate60freal: f32) {
    if s.x != 0.0 || s.z != 0.0 {
        let mut hyp = (s.x * s.x + s.z * s.z).sqrt();
        if hyp > 1.5 {
            s.x *= 1.5 / hyp;
            s.z *= 1.5 / hyp;
            hyp = 1.5;
        }
        for k in 0..3 {
            let v = &mut s[k];
            if hyp > 0.0001 {
                if *v > 0.0 {
                    *v -= (1.0 / 30.0) * lvupdate60freal * *v / hyp;
                    if *v < 0.0 {
                        *v = 0.0;
                    }
                } else if *v < 0.0 {
                    *v -= (1.0 / 30.0) * lvupdate60freal * *v / hyp;
                    if *v > 0.0 {
                        *v = 0.0;
                    }
                }
            } else {
                *v = 0.0;
            }
        }
    }
}

impl Player {
    /// `bmove_get_speed_verta_limit` / the theta-control limit (`bondmove.c:329`).
    fn speed_limit(value: f32, fov: f32) -> f32 {
        if value > 0.0 {
            (fov * value * -0.7) / PLAYER_DEFAULT_FOV
        } else if value < 0.0 {
            (fov * -value * 0.7) / PLAYER_DEFAULT_FOV
        } else {
            0.0
        }
    }

    /// `bmove_update_speed_verta` / `bmove_update_speed_theta_control` (`:342`, `:397`).
    fn update_speed(cur: &mut f32, value: f32, fov: f32, lv60: f32) {
        let mult = fov / PLAYER_DEFAULT_FOV;
        let limit = Self::speed_limit(value, fov);
        if value > 0.0 {
            *cur -= if *cur > 0.0 { 0.05 } else { 0.0125 } * lv60 * mult;
            if *cur < limit {
                *cur = limit;
            }
        } else if value < 0.0 {
            *cur += if *cur < 0.0 { 0.05 } else { 0.0125 } * lv60 * mult;
            if *cur > limit {
                *cur = limit;
            }
        } else if *cur > limit {
            *cur -= 0.05 * lv60 * mult;
            if *cur < limit {
                *cur = limit;
            }
        } else {
            *cur += 0.05 * lv60 * mult;
            if *cur > limit {
                *cur = limit;
            }
        }
    }

    /// `player_tween_fov_y` + `player_update_zoom` (`player.c:2068`, `:2101`).
    fn tween_fov(&mut self, target: f32, lv60: f32) {
        let cur_target = if self.zoomintimemax > self.zoomintime { self.zoominfovynew } else { self.zoominfovy };
        if cur_target != target {
            self.zoomintime = 0.0;
            self.zoomintimemax = (self.zoominfovy - target).abs() * 15.0 / 30.0;
            self.zoominfovyold = self.zoominfovy;
            self.zoominfovynew = target;
        }
        if self.zoomintime < self.zoomintimemax {
            self.zoomintime = (self.zoomintime + lv60).min(self.zoomintimemax);
            self.zoominfovy = self.zoominfovyold + (self.zoomintime * (self.zoominfovynew - self.zoominfovyold)) / self.zoomintimemax;
        } else {
            self.zoomintime = self.zoomintimemax;
            self.zoominfovy = self.zoominfovynew;
        }
        // playermgr_set_fov_y + vi_set_fov_y (player.c:2123).
        self.cam.vi_set_fov_y(self.zoominfovy);
    }

    /// `bmove_process_input` (`bondmove.c:695`): the PC port's `CONTROLMODE_PC`
    /// with the mouse for keyboard play, control style 1.1 for a pad.
    pub(super) fn bmove_process_input(&mut self, input: &PlayerInput, lv: &Lv, res: &WorldRes, rng: &mut Rng) {
        let lv60 = lv.lvupdate60freal;
        let fov = self.zoominfovy;
        let mlookscale = if lv.lvupdate240 != 0 { 4.0 / lv.lvupdate240 as f32 } else { 4.0 };
        // inputMouseGetScaledDelta (pcport input.c:1256)
        let freelookdx = input.mouse_dx * (0.022 / 3.5) * self.mouse_sens;
        let freelookdy = input.mouse_dy * (0.022 / 3.5) * self.mouse_sens;

        // AIMCONTROL_HOLD: aim while R is held.
        self.insightaimmode = input.aim;
        let aiming = self.insightaimmode;
        let allowmcross = freelookdx != 0.0 || freelookdy != 0.0 || self.swivelpos[0] != 0.0 || self.swivelpos[1] != 0.0;

        let canswivelgun = !aiming;
        let canmanualaim = aiming;
        let pad = input.pad;
        // 1.1 (bondmove.c:1166): the stick walks (analogwalk = c1stickysafe) and
        // turns; strafing is digital on C-left/right, pitch digital on C-up/down.
        let (analogstrafe, analogwalk) = if pad {
            (0, if !aiming { input.look_y } else { 0 })
        } else if !aiming {
            (input.walk_x, input.walk_y)
        } else {
            (0, 0)
        };
        let unk14 = !pad && !aiming && (input.walk_x != 0 || input.walk_y != 0);
        let canlookahead = if pad { !aiming } else { !aiming && (input.walk_x != 0 || input.walk_y != 0) };
        let cannaturalpitch = !aiming && !pad;
        let digitalstep = if pad && !aiming { input.c_right as i32 - input.c_left as i32 } else { 0 };
        let cannaturalturn = !aiming;
        let mut speedvertadown = 0.0f32;
        let mut speedvertaup = 0.0f32;
        let mut aimturnleftspeed = 0.0f32;
        let mut aimturnrightspeed = 0.0f32;

        // C-up / C-down look (bondmove.c:1187; PD's non-inverted swap is folded
        // in, as in the aiming stick look below: C-up looks up).
        if pad && !aiming {
            if input.c_up {
                speedvertaup = 1.0;
            }
            if input.c_down {
                speedvertadown = 1.0;
            }
        }

        // Stick look while aiming (N64, bondmove.c:1207): push past 60 to turn.
        // The stick's y is used as it is, PD's `invertpitch` (the MP default,
        // bondmove.c:564): pushing up aims down and, past the edge, looks down,
        // the same way as the crosshair below.
        // SUBST: PD's one option also swaps the C-buttons (:1454) / they keep
        // C-up looking up, as the user playtested in M3.
        if aiming {
            let sy = input.look_y;
            if sy > 60 {
                speedvertadown = ((sy - 60) as f32 / 10.0).min(1.0);
            } else if sy < -60 {
                speedvertaup = ((-60 - sy) as f32 / 10.0).min(1.0);
            }
            if input.look_x < -60 {
                aimturnleftspeed = ((-60 - input.look_x) as f32 / 10.0).min(1.0);
            } else if input.look_x > 60 {
                aimturnrightspeed = ((input.look_x - 60) as f32 / 10.0).min(1.0);
            }
        }
        // Mouse crosshair at the screen edge turns the view (pcport :1446).
        if aiming && allowmcross {
            let eb = self.crosshairedgeboundary;
            if self.swivelpos[0] > eb {
                aimturnrightspeed += (self.swivelpos[0] - eb) / (1.0 - eb);
            } else if self.swivelpos[0] < -eb {
                aimturnleftspeed += (self.swivelpos[0] + eb) / -(1.0 - eb);
            }
            if self.swivelpos[1] > eb {
                speedvertadown += (self.swivelpos[1] - eb) / (1.0 - eb);
            } else if self.swivelpos[1] < -eb {
                speedvertaup += (self.swivelpos[1] + eb) / -(1.0 - eb);
            }
        } else if !aiming {
            self.swivelpos = [0.0; 2];
        }

        // Crouch (C-down / C-up while aiming, or the crouch keys).
        let mut crouchdown = input.crouch_down as i32;
        let mut crouchup = input.crouch_up as i32;
        let manualzoom = res.gset.has_aim_flag(self.gun.bgun_get_weapon_num(HAND_RIGHT), INVAIMFLAG_MANUALZOOM);
        if pad {
            // bondmove.c:1340: C-up/C-down presses while aiming crouch (the
            // zooming guns zoom instead, below); a short R tap uncrouches.
            let pressed_up = input.c_up && !self.prev_c_updown[0];
            let pressed_down = input.c_down && !self.prev_c_updown[1];
            if aiming && !manualzoom {
                if pressed_up {
                    if crouchdown > 0 {
                        crouchdown -= 1;
                    } else {
                        crouchup += 1;
                    }
                    self.aimtaptime = -1;
                }
                if pressed_down {
                    if crouchup > 0 {
                        crouchup -= 1;
                    } else {
                        crouchdown += 1;
                    }
                    self.aimtaptime = -1;
                }
            }
            // AIMCONTROL_HOLD (bondmove.c:1360).
            if aiming {
                if self.aimtaptime >= 0 {
                    self.aimtaptime += lv.lvupdate60;
                }
            } else {
                if self.aimtaptime > 0 && self.aimtaptime < 15 {
                    if crouchdown > 0 {
                        crouchdown -= 1;
                    } else {
                        crouchup += 1;
                    }
                }
                self.aimtaptime = 0;
            }
            self.prev_c_updown = [input.c_up, input.c_down];
        } else if aiming {
            if input.walk_y > 30 {
                crouchup += 1;
            }
            if input.walk_y < -30 {
                crouchdown += 1;
            }
        }
        // Lean: C-left/right while aiming (movedata.unk30/unk34), or A/D.
        let rleanleft = aiming && if pad { input.c_left } else { input.walk_x < -30 };
        let rleanright = aiming && if pad { input.c_right } else { input.walk_x > 30 };

        // B: hold 25 ticks to toggle the gun function, B+Z a temporary invert
        // (bondmove.c:1070, the N64 path).
        if input.use_held {
            if self.usedowntime >= -1 {
                let t = self.usedowntime;
                if input.fire && t > -1 && self.gun_ctx(res, rng, lv).bgun_consider_toggle_gun_function(t, true) != USETIMER_CONTINUE {
                    self.usedowntime = -3;
                }
                if self.usedowntime > -1 {
                    if self.usedowntime > 25 {
                        let t = self.usedowntime;
                        let r = self.gun_ctx(res, rng, lv).bgun_consider_toggle_gun_function(t, false);
                        self.usedowntime = match r {
                            USETIMER_STOP => -1,
                            USETIMER_REPEAT => -2,
                            _ => self.usedowntime + 1,
                        };
                    } else {
                        self.usedowntime += 1;
                    }
                }
            } else if self.usedowntime >= -2 {
                let t = self.usedowntime;
                self.gun_ctx(res, rng, lv).bgun_consider_toggle_gun_function(t, false);
            }
        } else {
            // B released after a short press: btapcount → bondactivateorreload
            // (bondmove.c:1303, lv.c:1293). M8: current_player_interact (the
            // pickups); with nothing to use it falls through to the reload.
            if self.usedowntime > 0 {
                self.gun.bgun_reload_if_possible(&res.gset, HAND_RIGHT);
                self.gun.bgun_reload_if_possible(&res.gset, HAND_LEFT);
            }
            self.usedowntime = 0;
            self.gun.bgun_release_use();
        }

        // A (bondmove.c:1236): a tap cycles forward on release, A + Z steps
        // back; a hold past 15 ticks opens the active menu (M7: not yet, so the
        // hold is swallowed).
        let fire_pressed = input.fire && !self.prev_fire;
        let mut weaponforward = false;
        let mut weaponback = false;
        if input.a_held {
            if self.invdowntime > -2 {
                if fire_pressed {
                    weaponback = true;
                    self.invdowntime = -1;
                }
                if self.invdowntime >= 0 && !input.fire {
                    if self.invdowntime > 15 {
                        self.invdowntime = -1;
                    } else {
                        self.invdowntime += lv.lvupdate60.max(1);
                    }
                }
            }
        } else {
            if self.invdowntime > 0 && !input.fire {
                weaponforward = true;
            }
            self.invdowntime = 0;
        }
        self.prev_fire = input.fire;
        if weaponforward {
            self.gun_ctx(res, rng, lv).bgun_cycle(true);
        }
        if weaponback {
            self.gun_ctx(res, rng, lv).bgun_cycle(false);
        }

        // Reload (the PC port's ALT1 / JO_ACTION_RELOAD).
        if input.reload {
            self.gun.bgun_reload_if_possible(&res.gset, HAND_RIGHT);
            self.gun.bgun_reload_if_possible(&res.gset, HAND_LEFT);
        }

        if self.waitforzrelease && !input.fire {
            self.waitforzrelease = false;
        }
        // Z doesn't fire while A is held (bondmove.c:1432).
        let triggeron = !self.waitforzrelease && input.fire && !input.a_held;
        self.gun_ctx(res, rng, lv).bgun_tick_gameplay(triggeron);

        // The manual zoom (bondmove.c:1320 → gset_zoom_out / gset_zoom_in,
        // bondmove.c:1480): C-down / C-up on the pad. PD's increment is 0.5 only
        // for a Farsight in the LEFT hand (its "@bug?"), so it is 1 here.
        let weaponnum = self.gun.bgun_get_weapon_num(HAND_RIGHT);
        if aiming && res.gset.has_aim_flag(weaponnum, INVAIMFLAG_MANUALZOOM) {
            if input.zoom_out || (pad && input.c_down) {
                self.gun_ctx(res, rng, lv).gset_zoom_out(1.0);
            }
            if input.zoom_in || (pad && input.c_up) {
                self.gun_ctx(res, rng, lv).gset_zoom_in(1.0);
            }
        }

        // The zoom (bondmove.c:1900).
        let mut zoomfov = PLAYER_DEFAULT_FOV;
        if aiming {
            zoomfov = self.gun.gset_get_gun_zoom_fov(&res.gset);
        }
        if self.gun.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_AR34 && self.gun.hands[HAND_RIGHT].weaponfunc == FUNC_SECONDARY {
            zoomfov = self.gun.gset_get_gun_zoom_fov(&res.gset);
        }
        if zoomfov <= 0.0 {
            zoomfov = PLAYER_DEFAULT_FOV;
        }
        self.tween_fov(zoomfov, lv60);

        self.bwalk_apply_move_data(analogstrafe, analogwalk, digitalstep, unk14, canlookahead, rleanleft, rleanright, crouchdown, crouchup, lv);

        // Speed boost after 3 s of full forward.
        if self.speedmaxtime60 >= 180 {
            if self.speedboost < 1.25 {
                self.speedboost += 0.01 * lv60;
            }
            if self.speedboost > 1.25 {
                self.speedboost = 1.25;
            }
        } else {
            if self.speedboost > 1.0 {
                self.speedboost -= 0.01 * lv60;
            }
            if self.speedboost < 1.0 {
                self.speedboost = 1.0;
            }
        }

        // Pitch.
        if cannaturalpitch {
            let tmp = fov / PLAYER_DEFAULT_FOV;
            let mut f = (input.look_y as f32 / 70.0).clamp(-1.0, 1.0);
            f = if f >= 0.0 { f * f } else { -(f * f) };
            // Up on the stick / mouse is up: PD's default (non-inverted) pitch.
            let f = -f + freelookdy * mlookscale;
            self.speedverta = -f * tmp;
        } else if speedvertadown > 0.0 {
            Self::update_speed(&mut self.speedverta, speedvertadown, fov, lv60);
        } else if speedvertaup > 0.0 {
            Self::update_speed(&mut self.speedverta, -speedvertaup, fov, lv60);
        } else {
            Self::update_speed(&mut self.speedverta, 0.0, fov, lv60);
        }
        self.verta += self.speedverta * lv60 * 3.5;

        // Turn.
        if cannaturalturn {
            let tmp = fov / PLAYER_DEFAULT_FOV;
            let mut f = (input.look_x as f32 / 70.0).clamp(-1.0, 1.0);
            f = if f >= 0.0 { f * f } else { -(f * f) };
            f += freelookdx * mlookscale;
            self.speedthetacontrol = f * tmp;
        } else if aimturnleftspeed > 0.0 {
            Self::update_speed(&mut self.speedthetacontrol, aimturnleftspeed, fov, lv60);
        } else if aimturnrightspeed > 0.0 {
            Self::update_speed(&mut self.speedthetacontrol, -aimturnrightspeed, fov, lv60);
        } else {
            Self::update_speed(&mut self.speedthetacontrol, 0.0, fov, lv60);
        }
        self.speedtheta = self.speedthetacontrol;
        // bwalk_update_speed_theta
        if self.crouchpos == CROUCHPOS_SQUAT {
            self.speedtheta *= 0.5;
        } else if self.crouchpos == CROUCHPOS_DUCK {
            self.speedtheta *= 0.75;
        }

        // Weapon switching: the keyboard's next / previous keys and number keys.
        if input.cycle_next {
            self.gun_ctx(res, rng, lv).bgun_cycle(true);
        }
        if input.cycle_prev {
            self.gun_ctx(res, rng, lv).bgun_cycle(false);
        }
        if let Some((w, dual)) = input.select {
            self.gun.select_weapon(w, dual);
        }

        // The crosshair swivel.
        self.gun.swivel_extra = [self.speedtheta * 0.3, -self.speedverta * 0.1];
        if canswivelgun {
            // bmoveApplyCrosshairSwivel (pcport :442): mouse turning sways less.
            let mouse_active = freelookdx != 0.0 || freelookdy != 0.0;
            let joy_active = input.look_x != 0 || input.look_y != 0;
            let (xs, ys) = if mouse_active && joy_active {
                (self.crosshairsway * 0.8, self.crosshairsway * 0.8)
            } else if mouse_active {
                (self.crosshairsway * 0.2, self.crosshairsway * 0.3)
            } else {
                (self.crosshairsway, self.crosshairsway)
            };
            let x = self.speedtheta * 0.3 * xs;
            let y = -self.speedverta * 0.1 * ys;
            self.gun_ctx(res, rng, lv).bgun_swivel_with_damp(x, y, 0.963);
        } else if canmanualaim {
            if allowmcross && input.look_x == 0 && input.look_y == 0 {
                let xcoeff = 320.0 / 1080.0;
                let ycoeff = 240.0 / 1080.0;
                let xscale = (self.mouseaimspeed * xcoeff) / self.aspect;
                let yscale = self.mouseaimspeed * ycoeff;
                let x = (self.swivelpos[0] + freelookdx * xscale).clamp(-1.0, 1.0);
                let y = (self.swivelpos[1] + freelookdy * yscale).clamp(-1.0, 1.0);
                self.swivelpos = [x, y];
                self.gun_ctx(res, rng, lv).bgun_swivel_with_damp(x, y, 0.01);
            } else {
                // `bondmove.c:1807`: the stick's y as it is (`invertpitch`, see
                // the stick look above), so pushing up moves the crosshair down.
                self.gun_ctx(res, rng, lv).bgun_swivel_without_damp(input.look_x as f32 * 0.65 / 80.0, input.look_y as f32 * 0.65 / 80.0);
            }
        }
    }

    /// `bmove_update_look` (`bondmove.c:1976`).
    pub(super) fn bmove_update_look(&mut self) {
        while self.verta < -180.0 {
            self.verta += 360.0;
        }
        while self.verta >= 180.0 {
            self.verta -= 360.0;
        }
        self.verta = self.verta.clamp(-90.0, 90.0);
    }

    /// `bmove_update_head_with_mtx`'s camera basis (`bondmove.c:2098`): pitch,
    /// then the head bob's look/up (`headroll`), then the turn.
    pub(super) fn update_camera_basis(&mut self) {
        let mut v360 = self.verta;
        if v360 < 0.0 {
            v360 += 360.0;
        }
        let mut sp180 = math::load_x_rotation(baddtor2(360.0 - v360));
        if self.headroll {
            let sp116 = math::look_at_basis(Vec3::ZERO, -self.headlook, self.headup);
            sp180 = sp116 * sp180;
        }
        let sp116 = math::load_y_rotation(baddtor2(360.0 - self.theta));
        sp180 = sp116 * sp180;
        self.look = sp180.z_axis.truncate();
        self.up = sp180.y_axis.truncate();
    }
}
