//! The gun code that puts objects into the world (`bondgun.c`), run from
//! `hand_tick_attack`: `bgun_create_thrown_projectile` (`:4294`) and its `2`
//! (`:4199`), `bgun_configure_projectile` (`:4129`), `bgun_create_fired_projectile`
//! (`:4562`) and its `2` (`:4172`), the launcher's held rocket (`:4491`-`:4560`),
//! `chr_calculate_trajectory` (`chraction.c:9859`) for the functions that arc
//! onto what the crosshair is on, the remote mines' detonator
//! (`player_activate_remote_mine_detonator`, `propobj.c:17211`), the Laptop's
//! deployment (`laptop_deploy`, `propobj.c:17488`) and the Slayer's fly-by-wire
//! launch (`player_launch_slayer_rocket`, `player.c:3034`).
//!
//! Source: the old repo's `pd_guns/throw.rs`.

use glam::{Mat3, Mat4, Vec3, Vec4};
use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::math::{self, baddtor, baddtor2};

use crate::fx::boltbeam::BoltOwner;
use crate::props::{Autogun, Obj, Projectile};
use crate::world::World;

/// `chr_calculate_trajectory` (`chraction.c:9859`): the launch direction that
/// lands a throw of speed `arg1` (cm a tick) on `aimpos`, worked in metres and g.
pub fn chr_calculate_trajectory(frompos: Vec3, arg1: f32, aimpos: Vec3) -> Vec3 {
    let arg1 = arg1 * 0.599_999_99;
    let d = (aimpos - frompos) * 0.01;
    let vel = d.length();
    let latvel = (d.x * d.x + d.z * d.z).sqrt();
    let sp38 = latvel / vel;
    let mut sp40 = sp38.clamp(-1.0, 1.0).acos();
    if d.y < 0.0 {
        sp40 = -sp40;
    }
    let sp2c = ((vel * 9.81 * sp38 * sp38) / (arg1 * arg1) + d.y / vel).clamp(-1.0, 1.0);
    let sp3c = (sp2c.asin() - sp40) * 0.5 + sp40;
    Vec3::new(d.x / latvel * sp3c.cos(), sp3c.sin(), d.z / latvel * sp3c.cos())
}

/// The quaternion slerp in `bgun_create_thrown_projectile` (`:4392`) and the
/// fired version (`:4635`): turn `gundir` towards `sp140` by at most `limit`,
/// through `mtx00016b58` look matrices (up +y), `quaternion0f097044`,
/// `quaternion0f0976c0` and `quaternion_slerp`; the new direction is the
/// result's −z.
fn clamp_towards(gundir: Vec3, sp140: Vec3, limit: f32) -> Vec3 {
    let radians = gundir.dot(sp140).clamp(-1.0, 1.0).acos();
    if radians > limit || radians < -limit {
        let spf8 = math::look_at_basis(Vec3::ZERO, gundir, Vec3::Y);
        let spb8 = math::look_at_basis(Vec3::ZERO, sp140, Vec3::Y);
        let sp68 = math::quaternion0f097044(&spf8);
        let mut sp58 = math::quaternion0f097044(&spb8);
        math::quaternion0f0976c0(sp68, &mut sp58);
        let frac = (limit / radians).abs();
        let sp48 = math::quaternion_slerp(sp68, sp58, frac);
        let sp78 = math::quaternion_to_mtx(sp48);
        -sp78.z_axis.truncate()
    } else {
        sp140
    }
}

impl World {
    /// `bgun_configure_projectile` (`:4129`): place the object (its orientation
    /// times the model's scale), make it airborne and sticky with its spin, owned
    /// by `owner`. A bolt's trail starts at `beampos` (PD's `pos`).
    #[allow(clippy::too_many_arguments)]
    fn bgun_configure_projectile(&mut self, o: &mut Obj, pos: Vec3, matrix1: &Mat4, velocity: Vec3, spin: Mat3, owner: usize, beampos: Vec3) {
        let mut m = *matrix1;
        math::scale3(&mut m, o.scale);
        o.realrot = Mat3::from_mat4(m);
        o.pos = pos;
        if o.ty == OBJTYPE_WEAPON && o.weaponnum == WEAPON_BOLT {
            let bb = &mut self.fx.boltbeams;
            if let Some(i) = bb.find(BoltOwner::Prop(o.id)).or_else(|| bb.create(o.id)) {
                bb.beams[i].headpos = beampos;
                bb.beams[i].tailpos = beampos;
            }
        }
        self.props.make_room_for_projectile();
        o.projectile = Some(Projectile {
            flags: PROJECTILEFLAG_AIRBORNE | PROJECTILEFLAG_STICKY,
            ownerprop: Some(owner),
            mtx: spin,
            speed: velocity,
            startframe: self.lv.lvframenum,
            ..Projectile::default()
        });
    }

    /// A new weapon object for `projectilemodelnum`'s model
    /// (`weapon_create_projectile_from_gset`, `propobj.c:17632`).
    fn weapon_create_projectile(&mut self, weaponnum: u8, weaponfunc: usize, projweapon: u8, projfunc: usize, owner: usize) -> Option<Obj> {
        let proj = self.res.gset.func(weaponnum, weaponfunc)?.proj.clone()?;
        let def = self.res.models.get(proj.model.as_deref()?).ok()?;
        self.props.make_room_for_weapon();
        let id = self.props.alloc_id();
        Some(Obj::weapon(id, def, proj.modelscale, projweapon, projfunc, owner))
    }

    /// `bgun_create_thrown_projectile` (`bondgun.c:4294`) for player `pi`'s hand
    /// `h`: the object leaves the muzzle (or the player, if a wall is between),
    /// thrown along the aim with the player's own motion added.
    pub(crate) fn bgun_create_thrown_projectile(&mut self, pi: usize, h: usize, weaponnum: u8, weaponfunc: usize) {
        let hand = &self.players[pi].gun.hands[h];
        let muzzlepos = hand.muzzlepos;
        let mut sp1f4 = Mat4::IDENTITY;
        if weaponnum == WEAPON_COMBATKNIFE {
            sp1f4 = math::load_z_rotation(baddtor(270.0));
            let sp190 = math::load_x_rotation(baddtor(180.0));
            sp1f4 = math::mul(&sp190, &sp1f4);
        }
        let mm = hand.muzzlemat;
        let sp190 = Mat4::from_cols(
            mm.x_axis.truncate().normalize_or_zero().extend(mm.x_axis.w),
            mm.y_axis.truncate().normalize_or_zero().extend(mm.y_axis.w),
            mm.z_axis.truncate().normalize_or_zero().extend(mm.z_axis.w),
            Vec4::new(0.0, 0.0, 0.0, mm.w_axis.w),
        );
        sp1f4 = sp190 * sp1f4;

        // cd_test_los_oobok_getfinalroom_autoflags(player → muzzle, CDTYPE_ALL),
        // the player's perimeter off: the BG, then the other chrs' perimeters.
        let playerpos = self.players[pi].pos;
        let blocked = !self.level.los_autoflags(playerpos, muzzlepos)
            || self.players.iter().enumerate().any(|(j, p)| j != pi && crate::props::autogun::segment_cyl(playerpos, muzzlepos, &p.perim()).is_some());
        let spawnpos = if blocked { playerpos } else { muzzlepos };

        let res = self.res.clone();
        let gundir2d = {
            let p = &mut self.players[pi];
            let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
            g.bgun_calculate_player_shot_spread(h, true)
        };
        let mut gundir = self.players[pi].cam.projection.transform_vector3(gundir2d);
        let calc = self.res.gset.func(self.players[pi].gun.hands[h].weaponnum, self.players[pi].gun.hands[h].weaponfunc).is_some_and(|f| f.flags & FUNCFLAG_CALCULATETRAJECTORY != 0);
        let mut velocity = if calc {
            // prop_find_aiming_at(HAND_RIGHT, false, FINDPROPCONTEXT_QUERY) → the dot.
            self.prop_find_aiming_at(pi, HAND_RIGHT, false, false);
            let hand = &self.players[pi].gun.hands[h];
            if hand.hasdotinfo {
                let sp140 = chr_calculate_trajectory(spawnpos, 21.666_666, hand.dotpos);
                gundir = clamp_towards(gundir, sp140, baddtor2(20.0));
            }
            gundir * 21.666_666
        } else {
            let mut v = gundir * 16.666_666;
            if weaponnum == WEAPON_GRENADE || weaponnum == WEAPON_NBOMB {
                v.y += 1.666_666_6;
            } else {
                v.y += 5.0;
            }
            v
        };
        if weaponnum == WEAPON_LAPTOPGUN {
            let p = &mut self.players[pi];
            let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
            g.bgun_free_weapon(h);
        }
        if self.lv.lvupdate240 > 0 {
            // bondextrapos (a lift's push) is zero in the Combat Simulator's arenas.
            let p = &self.players[pi];
            velocity += (p.pos - p.prevpos) / self.lv.lvupdate60freal;
        }
        let Some(mut o) = self.bgun_create_thrown_projectile2(pi, weaponnum, weaponfunc, spawnpos, &sp1f4, velocity) else { return };
        let primetimer60 = self.players[pi].gun.hands[h].primetimer60;
        if o.ty == OBJTYPE_WEAPON && weaponnum == WEAPON_GRENADE && weaponfunc == FUNC_PRIMARY {
            // Cooked in the hand: the fuse has burned `primetimer60` already.
            if o.timer240 < primetimer60 * 4 {
                o.timer240 = 0;
            } else {
                o.timer240 -= primetimer60 * 4;
            }
            o.gunfunc = weaponfunc;
        }
        if let Some(p) = o.projectile.as_mut() {
            p.flags |= PROJECTILEFLAG_LAUNCHING;
            p.nextsteppos = muzzlepos;
            if weaponnum == WEAPON_GRENADE && weaponfunc == FUNC_SECONDARY {
                p.hitspeedpreservationfrac = 1.0;
            }
            if weaponnum == WEAPON_COMBATKNIFE {
                p.flags |= PROJECTILEFLAG_FORCEGOODBOUNCE;
                p.hitspeedpreservationfrac = 0.1;
                p.pickuptimer240 = 240;
                o.hidden |= OBJHFLAG_THROWNKNIFE;
            }
        }
        self.props.objs.push(o);
    }

    /// `bgun_create_thrown_projectile2` (`bondgun.c:4199`): the object for a
    /// throw: a weapon with its fuse in quarter-ticks, or for the Laptop a
    /// sentry; the throw's woosh (`SFXMAP_80A9_THROW`).
    fn bgun_create_thrown_projectile2(&mut self, pi: usize, weaponnum: u8, weaponfunc: usize, pos: Vec3, arg4: &Mat4, velocity: Vec3) -> Option<Obj> {
        let func = self.res.gset.func(weaponnum, weaponfunc)?.clone();
        let proj = func.proj.clone()?;
        let spin = if weaponnum == WEAPON_COMBATKNIFE {
            // guRotateF(90 / (RANDOMFRAC() + 12.1) degrees about arg4's y axis).
            let deg = 90.0 / (self.rng.randomfrac() + 12.1);
            Mat3::from_mat4(math::gu_rotate_f(deg, arg4.y_axis.x, arg4.y_axis.y, arg4.y_axis.z))
        } else {
            crate::props::projectile_load_random_rotation(&mut self.rng)
        };
        let mut o = if weaponnum == WEAPON_LAPTOPGUN {
            self.laptop_deploy(pi, &proj)?
        } else {
            let mut o = self.weapon_create_projectile(weaponnum, weaponfunc, weaponnum, weaponfunc, pi)?;
            // Note this timer is converted to 240 time immediately below.
            o.timer240 = func.activatetime60;
            if o.timer240 >= 2 {
                o.timer240 *= 4;
            }
            if weaponnum == WEAPON_GRENADE || weaponnum == WEAPON_NBOMB {
                o.dangerous = true;
            }
            if matches!(proj.model.as_deref(), Some("chrremotemine" | "chrtimedmine" | "chrproximitymine")) {
                o.flags3 |= OBJFLAG3_SETTLEROT_BYACTUALSIZE;
            }
            o
        };
        self.bgun_configure_projectile(&mut o, pos, arg4, velocity, spin, pi, pos);
        o.hidden &= 0x0fff_ffff;
        o.hidden |= (pi as u32) << 28;
        if let Some(p) = o.projectile.as_mut() {
            p.flags |= PROJECTILEFLAG_FORCEGOODBOUNCE;
            p.hitspeedpreservationfrac = 0.1;
            p.pickuptimer240 = 240;
            self.sound_at(0x80a9, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
        }
        Some(o)
    }

    /// `laptop_deploy` (`propobj.c:17488`): player `pi`'s sentry, replacing (and
    /// blowing up) an earlier one, loaded with up to 200 of the Laptop's rounds
    /// from the player's reserve.
    fn laptop_deploy(&mut self, pi: usize, proj: &crate::gun::gset::ProjDef) -> Option<Obj> {
        if pi >= crate::props::MAX_THROWN_LAPTOPS {
            return None;
        }
        self.laptop_replace(pi, pi);
        let def = self.res.models.get(proj.model.as_deref()?).ok()?;
        let id = self.props.alloc_id();
        let mut o = Obj::weapon(id, def, proj.modelscale, WEAPON_LAPTOPGUN, FUNC_PRIMARY, pi);
        o.ty = OBJTYPE_AUTOGUN;
        o.flags = OBJFLAG_THROWNLAPTOP | OBJFLAG_01000000 | OBJFLAG_WEAPON_AICANNOTUSE;
        o.flags3 |= OBJFLAG3_INTERACTABLE | OBJFLAG3_SETTLEROT_LAPTOP;
        o.hidden |= OBJHFLAG_TAGGED;
        // bgun_get_ammo_qty_for_weapon(WEAPON_LAPTOPGUN, FUNC_PRIMARY), at most 200,
        // taken from the reserve.
        let gun = &mut self.players[pi].gun;
        let ammotype = self.res.gset.weapon(WEAPON_LAPTOPGUN).and_then(|w| w.ammos[0].as_ref()).map_or(-1, |a| a.ammotype);
        let qty = gun.ammoheld(ammotype);
        let ammo = qty.min(200);
        if ammotype >= 0 && !gun.p.unlimited_ammo {
            gun.p.ammoheldarr[ammotype as usize] = qty - ammo;
        }
        // chr->team is 1 << the MP team (`playerreset.c:388`).
        let team = self.setup.players.get(pi).map_or(0, |p| 1u8 << (p.chr.team & 7));
        o.autogun = Some(Autogun::deployed(ammo, !team));
        self.props.thrown_laptops[pi] = Some(id);
        Some(o)
    }

    /// `bgun_update_rocket_launcher` (`bondgun.c:6997`): a loaded launcher shows
    /// its rocket at the muzzle.
    pub(crate) fn bgun_update_rocket_launcher(&mut self, pi: usize, h: usize) {
        let hand = &self.players[pi].gun.hands[h];
        if hand.rocket.is_none() && hand.loadedammo[0] > 0 {
            self.bgun_create_held_rocket(pi, h);
        }
        if self.players[pi].gun.hands[h].rocket.is_some() {
            self.bgun_update_held_rocket(pi, h);
        }
    }

    /// `bgun_create_held_rocket` (`bondgun.c:4527`): a WEAPON_ROCKET object with
    /// `timer240` 1, `OBJFLAG_HELDROCKET` and `OBJFLAG2_THROWTHROUGH`.
    fn bgun_create_held_rocket(&mut self, pi: usize, h: usize) {
        if self.players[pi].gun.hands[h].rocket.is_some() {
            return;
        }
        self.players[pi].gun.hands[h].firedrocket = false;
        let func = self.players[pi].gun.hands[h].weaponfunc;
        let Some(mut o) = self.weapon_create_projectile(WEAPON_ROCKETLAUNCHER, func, WEAPON_ROCKET, FUNC_PRIMARY, pi) else { return };
        o.timer240 = 1;
        o.flags |= OBJFLAG_HELDROCKET;
        o.flags2 |= OBJFLAG2_THROWTHROUGH;
        o.realrot = Mat3::from_diagonal(Vec3::splat(o.scale));
        o.pos = self.players[pi].gun.hands[h].muzzlepos;
        self.players[pi].gun.hands[h].rocket = Some(o.id);
        self.props.objs.push(o);
    }

    /// `bgun_update_held_rocket` (`bondgun.c:4491`): until fired it sits at the
    /// muzzle in the hand's orientation (drawn with the gun, from `muzzlemat`).
    fn bgun_update_held_rocket(&mut self, pi: usize, h: usize) {
        let hand = &self.players[pi].gun.hands[h];
        let (Some(id), fired, posmtx, muzzlepos) = (hand.rocket, hand.firedrocket, hand.posmtx, hand.muzzlepos) else { return };
        if let Some(o) = self.props.get_mut(id) {
            if !fired {
                let mut m = posmtx;
                m.w_axis = Vec4::new(0.0, 0.0, 0.0, m.w_axis.w);
                math::scale3(&mut m, o.scale);
                o.realrot = Mat3::from_mat4(m);
                o.pos = muzzlepos;
            }
        }
    }

    /// `bgun_free_held_rocket` (`bondgun.c:4552`).
    pub(crate) fn bgun_free_held_rocket(&mut self, pi: usize, h: usize) {
        if let Some(id) = self.players[pi].gun.hands[h].rocket.take() {
            self.props.objs.retain(|o| o.id != id);
        }
    }

    /// `bgun_create_fired_projectile` (`bondgun.c:4562`): rockets (the
    /// launcher's own held one, or new), the Slayer's, crossbow bolts, and the
    /// Devastator's and SuperDragon's grenade rounds.
    pub(crate) fn bgun_create_fired_projectile(&mut self, pi: usize, h: usize) {
        let (weaponnum, weaponfunc) = {
            let hand = &self.players[pi].gun.hands[h];
            (hand.weaponnum, hand.weaponfunc)
        };
        let Some(func) = self.res.gset.func(weaponnum, weaponfunc).cloned() else { return };
        if func.ftype != INVENTORYFUNCTYPE_SHOOT_PROJECTILE {
            return;
        }
        let Some(proj) = func.proj.clone() else { return };
        let sp270 = Mat3::IDENTITY;
        let res = self.res.clone();
        let gundir2d = {
            let p = &mut self.players[pi];
            let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
            g.bgun_calculate_player_shot_spread(h, true)
        };
        let mut gundir = self.players[pi].cam.projection.transform_vector3(gundir2d);
        let mut spawnpos = self.players[pi].gun.hands[h].muzzlepos;
        if weaponnum == WEAPON_SLAYER && weaponfunc == FUNC_SECONDARY {
            spawnpos += gundir * 50.0;
        }
        let sp260 = proj.speed * 1.666_666_6 / 60.0;
        let sp25c = proj.traveldist * 1.666_666_6;
        if func.flags & FUNCFLAG_CALCULATETRAJECTORY != 0 {
            self.prop_find_aiming_at(pi, HAND_RIGHT, false, false);
            let hand = &self.players[pi].gun.hands[h];
            if hand.hasdotinfo {
                let sp1bc = chr_calculate_trajectory(spawnpos, sp25c, hand.dotpos);
                gundir = clamp_towards(gundir, sp1bc, baddtor2(10.0));
            }
        }
        let accel = gundir * sp260;
        let lv = self.lv.clone();
        let mut sp264 = accel * lv.lvupdate60freal + gundir * sp25c;
        if func.flags & FUNCFLAG_FLYBYWIRE == 0 && lv.lvupdate240 > 0 {
            let p = &self.players[pi];
            sp264 += (p.pos - p.prevpos) / lv.lvupdate60freal;
        }
        let mut sp210 = self.players[pi].gun.hands[h].posmtx;
        sp210.w_axis = Vec4::new(0.0, 0.0, 0.0, sp210.w_axis.w);

        let homing = func.flags & FUNCFLAG_HOMINGROCKET != 0;
        let held = self.players[pi].gun.hands[h].rocket;
        let mut o = if let Some(id) = held {
            self.players[pi].gun.hands[h].firedrocket = true;
            let Some(i) = self.props.objs.iter().position(|o| o.id == id) else { return };
            let mut o = self.props.objs.remove(i);
            o.flags2 &= !OBJFLAG2_THROWTHROUGH;
            o.flags &= !OBJFLAG_HELDROCKET;
            if homing {
                o.weaponnum = WEAPON_HOMINGROCKET;
            }
            o
        } else {
            let (wn, gf) = match weaponnum {
                WEAPON_ROCKETLAUNCHER | WEAPON_SLAYER => (if homing { WEAPON_HOMINGROCKET } else { WEAPON_ROCKET }, FUNC_PRIMARY),
                WEAPON_CROSSBOW => (WEAPON_BOLT, weaponfunc),
                WEAPON_DEVASTATOR => (WEAPON_GRENADEROUND, weaponfunc),
                WEAPON_SUPERDRAGON => (WEAPON_GRENADEROUND, FUNC_2),
                _ => (weaponnum, weaponfunc),
            };
            let Some(o) = self.weapon_create_projectile(weaponnum, weaponfunc, wn, gf, pi) else { return };
            o
        };
        o.timer240 = proj.timer60;
        if o.timer240 != -1 {
            o.timer240 *= 4;
        }
        o.hidden &= 0x0fff_ffff;
        o.hidden |= (pi as u32) << 28;
        // bgun_create_fired_projectile2 (`:4172`): placed at the player, stepped
        // to the muzzle by the launch.
        let playerpos = self.players[pi].pos;
        self.bgun_configure_projectile(&mut o, playerpos, &Mat4::from_mat3(Mat3::from_mat4(sp210)), sp264, sp270, pi, spawnpos);
        {
            let p = o.projectile.as_mut().unwrap();
            p.flags |= PROJECTILEFLAG_LAUNCHING;
            p.nextsteppos = spawnpos;
            if func.flags & FUNCFLAG_PROJECTILE_LIGHTWEIGHT != 0 {
                p.flags |= PROJECTILEFLAG_LIGHTWEIGHT;
            } else if func.flags & FUNCFLAG_PROJECTILE_POWERED != 0 {
                p.flags |= PROJECTILEFLAG_POWERED;
            }
            // M6: trackedprops[0], the rocket launcher's lock.
            p.targetprop = None;
        }
        if proj.scale != 1.0 {
            o.scale *= proj.scale;
            let mut m = Mat4::from_mat3(o.realrot);
            math::scale3(&mut m, proj.scale);
            o.realrot = Mat3::from_mat4(m);
        }
        {
            let oy = o.pos.y;
            let p = o.projectile.as_mut().unwrap();
            p.powerlimit240 = 1200;
            p.missiley = oy;
            p.missileyspeed = p.speed.y;
            p.accel = accel;
            p.pickuptimer240 = 240;
            p.hitspeedpreservationfrac = proj.hitspeedpreservationfrac;
            p.speeddecel = proj.speeddecel * 1.666_666_6;
        }
        if func.soundnum > 0 {
            self.sound_at(func.soundnum, 1.0, o.pos, crate::propsnd::DEFAULT_DISTS);
        }
        if func.flags & FUNCFLAG_FLYBYWIRE != 0 {
            self.player_launch_slayer_rocket(pi, o.id);
        }
        if o.projectile.as_ref().unwrap().flags & PROJECTILEFLAG_LAUNCHING != 0 {
            let (mut a, mut b) = (Vec3::ZERO, Vec3::ZERO);
            self.projectile_launch(&mut o, pi, &mut a, &mut b);
        }
        self.props.objs.push(o);
    }

    /// `player_launch_slayer_rocket` (`player.c:3034`): the camera rides the
    /// rocket; the vision devices go off.
    fn player_launch_slayer_rocket(&mut self, pi: usize, id: u32) {
        let p = &mut self.players[pi];
        p.slayerrocket = Some(id);
        p.visionmode = VISIONMODE_SLAYERROCKET;
        p.devicesactive &= !(DEVICE_NIGHTVISION | DEVICE_XRAYSCANNER | DEVICE_EYESPY | DEVICE_IRSCANNER);
        p.badrockettime = 0;
    }

    /// `player_activate_remote_mine_detonator` (`propobj.c:17211`).
    pub(crate) fn player_activate_remote_mine_detonator(&mut self, pi: usize) {
        self.props.detonating |= 1 << pi;
        self.push_event(Event::Sound { sound: 0x80ab, pitch: 1.0, volume: 1.0, pan: 0.0 });
        let res = self.res.clone();
        let p = &mut self.players[pi];
        let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
        g.bgun_start_detonate_animation();
    }
}
