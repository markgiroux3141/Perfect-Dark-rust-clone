//! What a gun's object does once it is in the world (`propobj.c`):
//! `weapon_tick` (`:4308`: grenade fuses, the Devastator's wall hugger, the
//! N-Bomb, rockets, timed, remote and proximity mines, the Dragon's and the
//! grenade's proximity functions, the bolt's quiver), `prop_explode` (`:4223`),
//! `obj_damage` for weapon objects (`:14399`) and the proximity registry
//! (`coord_trigger_proxies`, `:17286`).

use glam::{Mat3, Vec3};
use pd_core::ids::*;
use pd_core::math;

use super::explosions::ExpCam;
use crate::fx::boltbeam::BoltOwner;
use super::Obj;
use crate::world::World;

impl World {
    /// The camera an explosion made in player `pi`'s pass is judged from.
    pub(crate) fn exp_cam(&self, pi: usize) -> ExpCam {
        let p = &self.players[pi.min(self.players.len() - 1)];
        ExpCam { pos: p.cam.pos(), lodscalez: p.cam.c_lodscalez }
    }

    /// `prop_explode` (`propobj.c:4223`): stuck to the BG, the blast scorches the
    /// surface it sits on; otherwise `explosion_create_complex` (a scorch on the
    /// floor below if it reaches). `// M6:` an object embedded in a chr blows at
    /// the chr's position.
    pub(crate) fn prop_explode(&mut self, o: &Obj, exptype: usize, pi: usize) -> bool {
        let owner = o.owner() as i32;
        let cam = self.exp_cam(pi);
        let level = self.level.clone();
        let world = crate::world::StageExp { level: &level };
        let mut out = super::explosions::ExpOut::default();
        let attached_only = o.has(OBJHFLAG_ATTACHED) && !o.has(OBJHFLAG_EMBEDDED) && o.projectile.is_none();
        let ok = if attached_only {
            let ymin = o.bbox.ymin;
            let n = o.realrot.y_axis;
            let scorch = o.pos + n * ymin;
            self.explosions.create(&mut self.rng, &mut self.fx.smokes, &world, None, o.pos, exptype, owner, true, scorch, n, cam, &mut out)
        } else {
            self.explosions.create_complex(&mut self.rng, &mut self.fx.smokes, &world, None, o.pos, exptype, owner, cam, &mut out)
        };
        self.apply_explosion_out(out);
        ok
    }

    /// A board struck by a projectile: `fr_calculate_hit` scoring it, or
    /// `obj_damage` on it.
    pub(crate) fn board_struck(&mut self, b: usize, damage: f32) {
        if let Some(board) = self.boards.get_mut(b) {
            board.hits += 1;
            board.damage += damage;
            board.flash = 1.0;
        }
    }

    /// `weapon_tick` (`propobj.c:4308`), ticked in player `pi`'s pass.
    pub(crate) fn weapon_tick(&mut self, o: &mut Obj, pi: usize) {
        let lv240 = self.lv.lvupdate240;
        let w = o.weaponnum;
        let huge = if o.flags2 & OBJFLAG2_WEAPON_HUGEEXP != 0 { EXPLOSIONTYPE_HUGE17 } else { EXPLOSIONTYPE_ROCKET };
        if ((w == WEAPON_GRENADE && o.gunfunc == FUNC_PRIMARY) || w == WEAPON_GRENADEROUND) && o.timer240 >= 0 {
            if w == WEAPON_GRENADEROUND && o.gunfunc == FUNC_SECONDARY && o.timer240 > 0 {
                // The Devastator's wall hugger.
                if o.timer240 >= 2 {
                    o.timer240 -= lv240;
                    if o.timer240 < 8 {
                        // Time to fall: obj_ensure_projectile, airborne and sticky.
                        let lvframenum = self.lv.lvframenum;
                        let p = o.projectile.get_or_insert_with(Default::default);
                        *p = super::Projectile { startframe: lvframenum, ..Default::default() };
                        p.ownerprop = None;
                        p.flags |= PROJECTILEFLAG_AIRBORNE | PROJECTILEFLAG_STICKY;
                        p.speed = Vec3::new(0.0, -10.0, 0.0);
                        p.mtx = Mat3::IDENTITY;
                        o.timer240 = 1;
                    }
                }
            } else {
                o.timer240 -= lv240;
                if o.timer240 < 0 {
                    o.dangerous = false;
                    let t = if o.gunfunc == FUNC_2 { EXPLOSIONTYPE_SDGRENADE } else { huge };
                    self.prop_explode(o, t, pi);
                    o.hidden |= OBJHFLAG_DELETING;
                    self.slayer_rocket_gone(o.id);
                }
            }
        } else if w == WEAPON_NBOMB && o.gunfunc == FUNC_PRIMARY {
            // Impact N-Bombs go off on landing (projectile_tick); this is the
            // airborne-for-the-whole-fuse case.
            if o.timer240 >= 0 {
                o.timer240 -= lv240;
                if o.timer240 < 0 {
                    self.nbomb_create_storm(o.pos, Some(o.owner()));
                    o.dangerous = false;
                    o.hidden |= OBJHFLAG_DELETING;
                    self.slayer_rocket_gone(o.id);
                }
            }
        } else if w == WEAPON_ROCKET || w == WEAPON_HOMINGROCKET || w == WEAPON_SKROCKET {
            if o.timer240 == 0 {
                self.prop_explode(o, huge, pi);
                o.hidden |= OBJHFLAG_DELETING;
                self.slayer_rocket_gone(o.id);
            }
        } else if w == WEAPON_TIMEDMINE && o.timer240 >= 0 {
            if o.gunfunc == FUNC_PRIMARY {
                o.timer240 -= lv240;
                if o.timer240 < 0 && self.prop_explode(o, huge, pi) {
                    o.timer240 = -1;
                    o.hidden |= OBJHFLAG_DELETING;
                }
            }
        } else if w == WEAPON_REMOTEMINE {
            // Its owner pressed the detonator. (Mines thrown on oneself, the
            // prop->parent case, don't happen in a free-for-all.)
            if self.props.detonating & (1 << o.owner()) != 0 {
                o.timer240 = 0;
            }
            if o.timer240 >= 2 {
                o.timer240 -= lv240;
                if o.timer240 < 2 {
                    o.timer240 = 1;
                }
            } else if o.timer240 == 0 && self.prop_explode(o, huge, pi) {
                o.timer240 = -1;
                o.hidden |= OBJHFLAG_DELETING;
            }
        } else if w == WEAPON_PROXIMITYMINE || (w == WEAPON_DRAGON && o.gunfunc == FUNC_SECONDARY) || (w == WEAPON_GRENADE && o.gunfunc == FUNC_SECONDARY) || (w == WEAPON_NBOMB && o.gunfunc == FUNC_SECONDARY) {
            if o.timer240 >= 2 {
                // Arming (weapon_register_proxy once armed).
                o.timer240 -= lv240;
                if o.timer240 < 2 {
                    o.timer240 = 1;
                }
            } else if o.timer240 == 1 {
                // Armed: g_Vars.currentplayer within 2.5 m, the owner included.
                let playerpos = self.players[pi].pos;
                if (playerpos - o.pos).length_squared() < 250.0 * 250.0 {
                    o.timer240 = 0;
                }
            }
            if o.timer240 == 0 {
                if w == WEAPON_NBOMB {
                    self.nbomb_create_storm(o.pos, Some(o.owner()));
                    o.dangerous = false;
                    o.hidden |= OBJHFLAG_DELETING;
                    self.slayer_rocket_gone(o.id);
                } else {
                    let t = if w == WEAPON_DRAGON { EXPLOSIONTYPE_DRAGONBOMBSPY } else { huge };
                    if self.prop_explode(o, t, pi) {
                        o.timer240 = -1;
                        o.hidden |= OBJHFLAG_DELETING;
                    }
                }
            }
        } else if w == WEAPON_BOLT {
            // The quiver once stuck: timer240 13 → 2 rocks it about its tip.
            if o.timer240 >= 2 {
                let ival = o.timer240 - 1;
                let mut radians = 0.026_179_94 * (ival as f32 / 12.0);
                if ival < 12 {
                    radians += 0.026_179_94 * ((ival + 1) as f32 / 12.0);
                }
                if ival & 1 == 1 {
                    radians = -radians;
                }
                let spb8 = Mat3::from_mat4(math::load_y_rotation(radians));
                let zmax = o.bbox.zmax;
                // An embedded bolt rocks in its embedment matrix; a board never
                // moves, so the object's own transform stands for it.
                let sp6c = o.realrot * Vec3::new(0.0, 0.0, zmax);
                let sp78 = o.realrot * spb8;
                let sp60 = sp78 * Vec3::new(0.0, 0.0, zmax);
                o.realrot = sp78;
                o.pos -= sp60 - sp6c;
                o.timer240 -= 1;
            }
            // In flight (timer240 -1) the trail follows the bolt, 30 m at most,
            // and lets go once it has bounced or stopped.
            if o.timer240 < 0 {
                if let Some(i) = self.fx.boltbeams.find(BoltOwner::Prop(o.id)) {
                    self.fx.boltbeams.beams[i].tailpos = o.pos;
                    self.fx.boltbeams.increment_head_pos(i, 3000.0, false);
                    if o.projectile.as_ref().is_none_or(|p| p.bouncecount > 0) {
                        o.timer240 = 0;
                        self.fx.boltbeams.set_automatic(i, 1400.0);
                    }
                }
            }
        }
    }

    /// A Slayer rocket that is gone loses its signal (`weapon_tick`'s NTSC 1.0
    /// loop over the players): the view goes to static.
    pub(crate) fn slayer_rocket_gone(&mut self, id: u32) {
        for p in self.players.iter_mut() {
            if p.slayerrocket == Some(id) {
                p.slayerrocket = None;
                p.visionmode = VISIONMODE_SLAYERROCKETSTATIC;
            }
        }
    }

    /// `obj_damage` (`propobj.c:14399`) for the guns' objects: the attacker's
    /// number goes into `hidden` (not a deployed laptop's, whose bits name its
    /// owner), and an explosive's fuse is cut. `weaponnum` is `WEAPON_NONE` for
    /// a bounce, which a weapon object ignores. True: it destroyed a sentry,
    /// whose blast is the caller's (`obj_check_destroyed`).
    pub(crate) fn obj_damage(&mut self, id: u32, damage: f32, weaponnum: u8, playernum: i32) -> bool {
        let Some(o) = self.obj_mut(id) else { return false };
        if o.ty != OBJTYPE_AUTOGUN {
            o.hidden &= 0x0fff_ffff;
            o.hidden |= ((playernum as u32) << 28) & 0xf000_0000;
        }
        if weaponnum == WEAPON_NONE {
            // obj_defaults_to_bounceable_invincible_pickupable: weapons.
            if o.ty == OBJTYPE_WEAPON {
                return false;
            }
        } else if o.flags & OBJFLAG_INVINCIBLE != 0 {
            return false;
        } else if o.ty == OBJTYPE_WEAPON {
            let w = o.weaponnum;
            if matches!(w, WEAPON_GRENADE | WEAPON_TIMEDMINE | WEAPON_REMOTEMINE | WEAPON_PROXIMITYMINE | WEAPON_ROCKET | WEAPON_HOMINGROCKET | WEAPON_GRENADEROUND) || (w == WEAPON_DRAGON && o.gunfunc == FUNC_SECONDARY) {
                // Homing rockets don't go off from a remote mine's blast.
                if w != WEAPON_HOMINGROCKET || weaponnum != WEAPON_REMOTEMINE {
                    o.timer240 = 0;
                }
            }
            return false;
        }
        o.ty == OBJTYPE_AUTOGUN && super::autogun::autogun_damage(o, damage)
    }

    /// The object with `id`, wherever it is (ticking detaches the list).
    pub(crate) fn obj_mut(&mut self, id: u32) -> Option<&mut Obj> {
        self.props.get_mut(id)
    }

    /// `coord_trigger_proxies` (`propobj.c:17286`) for every living player
    /// (`chrs_trigger_proxies`, from `alarm_tick` after the last player's props):
    /// an armed proximity weapon within 2.5 m (a Dragon's 3.5 m) goes off; a
    /// proximity grenade only for `arg1` (every chr passes true).
    pub(crate) fn chrs_trigger_proxies(&mut self) {
        let positions: Vec<Vec3> = self.players.iter().map(|p| p.pos).collect();
        for o in self.props.objs.iter_mut() {
            let proxy = o.ty == OBJTYPE_WEAPON
                && (o.weaponnum == WEAPON_PROXIMITYMINE || (o.weaponnum == WEAPON_DRAGON && o.gunfunc == FUNC_SECONDARY) || (o.weaponnum == WEAPON_GRENADE && o.gunfunc == FUNC_SECONDARY) || (o.weaponnum == WEAPON_NBOMB && o.gunfunc == FUNC_SECONDARY));
            if !proxy || o.timer240 != 1 {
                continue;
            }
            let mut range = 250.0 * 250.0;
            if o.weaponnum == WEAPON_DRAGON {
                range += range;
            }
            for &pos in &positions {
                if (pos - o.pos).length_squared() < range {
                    o.timer240 = 0;
                }
            }
        }
    }
}
