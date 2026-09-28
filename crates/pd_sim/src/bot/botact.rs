//! What a simulant fires that isn't a round (`botact.c`, `chraction.c`): a
//! throw (`botact_throw`, `botact.c:339`: grenades, N-Bombs, mines, knives),
//! a launcher's projectile (`chr_shoot`'s projectile branch,
//! `chraction.c:10132`: rockets, crossbow bolts, the Devastator's and
//! SuperDragon's grenade rounds), and the Farsight's shot through walls
//! (`botact_shoot_farsight`, `botact.c:210`).

use glam::{Mat3, Mat4, Vec3};
use pd_core::ids::*;
use pd_core::math::{self, baddtor};

use crate::chr::DamageFrom;
use crate::gun::throw::chr_calculate_trajectory;
use crate::propsnd::DEFAULT_DISTS;
use crate::world::World;

/// `botact_get_projectile_throw_interval` (`botact.c:306`).
pub fn botact_get_projectile_throw_interval(weaponnum: u8) -> i32 {
    match weaponnum {
        WEAPON_COMBATKNIFE => 120,
        WEAPON_GRENADE | WEAPON_NBOMB => 90,
        _ => 60,
    }
}

impl World {
    /// `botact_throw` (`botact.c:339`): at the target (its feet for a grenade
    /// or N-Bomb, a player's chest) if it is within 30/256 of a turn of the
    /// facing, else ahead and 20° up; 16.7 cm a tick.
    pub(crate) fn botact_throw(&mut self, i: usize) {
        let (weaponnum, gunfunc) = (self.ab(i).weaponnum, self.ab(i).gunfunc);
        let sp80 = self.chrs[i].chr_get_aimx_angle();
        let pos = self.chrs[i].pos;
        let target = self.chrs[i].target;
        let sp152 = match target.filter(|&t| self.chrs[i].chr_is_pos_in_fov(self.chrs[t].pos, 30)) {
            Some(t) => {
                let tc = &self.chrs[t];
                let mut aim = tc.pos;
                if weaponnum == WEAPON_GRENADE || weaponnum == WEAPON_NBOMB {
                    aim.y = tc.manground;
                } else if tc.player.is_some() {
                    aim.y -= 25.0;
                }
                chr_calculate_trajectory(pos, 16.666_666, aim)
            }
            None => Vec3::new(baddtor(20.0).cos() * sp80.sin(), baddtor(20.0).sin(), baddtor(20.0).cos() * sp80.cos()),
        };
        let velocity = sp152 * 16.666_666;
        let mut sp164 = Mat4::IDENTITY;
        if weaponnum == WEAPON_COMBATKNIFE {
            sp164 = math::load_z_rotation(baddtor(180.0) * 1.5);
            let sp84 = math::load_x_rotation(baddtor(180.0));
            sp164 = math::mul(&sp84, &sp164);
        }
        sp164 = math::mul(&math::load_x_rotation(baddtor(20.0)), &sp164);
        sp164 = math::mul(&math::load_y_rotation(sp80), &sp164);
        if let Some(o) = self.bgun_create_thrown_projectile2(i, weaponnum, gunfunc, pos, &sp164, velocity) {
            self.props.objs.push(o);
        }
        if weaponnum == WEAPON_REMOTEMINE {
            self.ab_mut(i).flags |= BOTFLAG_THREWREMOTEMINE;
        }
    }

    /// `chr_shoot`'s projectile branch (`chraction.c:10132`) for simulant `i`'s
    /// launcher in `hand`, `vector` the (spread) shot direction and `gunpos` the
    /// muzzle: the rocket, bolt or grenade round, aimed at the target's feet
    /// (rockets, grenades on an arc) or at it (the wall hugger, bolts) when it
    /// is within 30/256 of a turn, with the launch sound.
    pub(crate) fn chr_shoot_projectile(&mut self, i: usize, weaponnum: u8, weaponfunc: usize, gunpos: Vec3, mut vector: Vec3) {
        let Some(func) = self.res.gset.func(weaponnum, weaponfunc).cloned() else { return };
        let Some(proj) = func.proj.clone() else { return };
        let (projweapon, projfunc) = match weaponnum {
            WEAPON_ROCKETLAUNCHER | WEAPON_SLAYER | WEAPON_KINGSCEPTRE => (if func.flags & FUNCFLAG_HOMINGROCKET != 0 { WEAPON_HOMINGROCKET } else { WEAPON_ROCKET }, FUNC_PRIMARY),
            WEAPON_CROSSBOW => (WEAPON_BOLT, weaponfunc),
            WEAPON_DEVASTATOR => (WEAPON_GRENADEROUND, weaponfunc),
            WEAPON_SUPERDRAGON => (WEAPON_GRENADEROUND, FUNC_2),
            _ => return,
        };
        let Some(mut o) = self.weapon_create_projectile(weaponnum, weaponfunc, projweapon, projfunc, i) else { return };
        let sp168 = proj.speed * (1.0 / 0.6) / 60.0;
        let spcc = proj.traveldist * (1.0 / 0.6);
        let target = self.chrs[i].target;
        if let Some(t) = target.filter(|&t| self.chrs[i].chr_is_pos_in_fov(self.chrs[t].pos, 30)) {
            let tc = &self.chrs[t];
            let feet = Vec3::new(tc.pos.x, tc.manground, tc.pos.z);
            let aim = if weaponfunc == FUNC_PRIMARY && matches!(weaponnum, WEAPON_ROCKETLAUNCHER | WEAPON_KINGSCEPTRE | WEAPON_SLAYER) {
                vector = (feet - gunpos).normalize_or_zero();
                Some(feet)
            } else if (weaponnum == WEAPON_DEVASTATOR && weaponfunc == FUNC_PRIMARY) || weaponnum == WEAPON_SUPERDRAGON {
                vector = chr_calculate_trajectory(gunpos, spcc, feet);
                Some(feet)
            } else if (weaponnum == WEAPON_DEVASTATOR && weaponfunc == FUNC_SECONDARY) || weaponnum == WEAPON_CROSSBOW {
                let mut aim = tc.pos;
                if tc.player.is_some() {
                    aim.y -= 25.0;
                }
                vector = chr_calculate_trajectory(gunpos, spcc, aim);
                Some(aim)
            } else {
                None
            };
            if let Some(aim) = aim {
                // Turned by the target's bearing from the facing.
                let angle = self.chrs[i].chr_get_angle_to_pos(aim);
                let (sin, cos) = angle.sin_cos();
                let (x, z) = (vector.x, vector.z);
                vector.x = sin * z + cos * x;
                vector.z = cos * z - sin * x;
            }
        }
        let roty = self.chrs[i].chr_get_aimx_angle();
        let rotx = self.chrs[i].chr_get_aimy_angle();
        let projectilemtx = math::mul(&math::load_y_rotation(roty), &math::load_x_rotation(rotx));
        let sp15c = vector * sp168;
        let sp16c = sp15c * self.lv.lvupdate60freal + vector * spcc;
        o.timer240 = proj.timer60;
        if o.timer240 != -1 {
            o.timer240 *= 4;
        }
        o.hidden &= 0x0fff_ffff;
        o.hidden |= ((i as u32) << 28) & 0xf000_0000;
        self.bgun_configure_projectile(&mut o, gunpos, &projectilemtx, sp16c, Mat3::IDENTITY, i, gunpos);
        if let Some(p) = o.projectile.as_mut() {
            if func.flags & FUNCFLAG_PROJECTILE_LIGHTWEIGHT != 0 {
                p.flags |= PROJECTILEFLAG_LIGHTWEIGHT;
            } else if func.flags & FUNCFLAG_PROJECTILE_POWERED != 0 {
                p.flags |= PROJECTILEFLAG_POWERED;
            }
            p.accel = sp15c;
            p.pickuptimer240 = 240;
            p.hitspeedpreservationfrac = proj.hitspeedpreservationfrac;
            p.speeddecel = proj.speeddecel * (1.0 / 0.6);
            p.targetprop = target;
        }
        if func.soundnum > 0 {
            self.sound_at(func.soundnum, 1.0, o.pos, DEFAULT_DISTS);
        }
        self.props.objs.push(o);
    }

    /// `botact_shoot_farsight` (`botact.c:210`): a Farsight simulant that can't
    /// see its target fires through the walls: three times in ten, every chr in
    /// the line takes the Farsight's damage if it stands still (a moving one
    /// only once in fifteen; PD keeps the smaller chance for the rest of the
    /// chrs once one has moved).
    pub(crate) fn botact_shoot_farsight(&mut self, i: usize, vector: Vec3, gunpos: Vec3) {
        if self.ab(i).weaponnum != WEAPON_FARSIGHT {
            return;
        }
        let rand = (self.rng.random() % 100) as f32;
        if rand >= 30.0 {
            return;
        }
        let damage = self.chr_gset_damage(WEAPON_FARSIGHT, FUNC_PRIMARY, 0.0);
        let fallback = 30.0f32;
        let mut value = fallback;
        for k in 0..self.chrs.len() {
            let c = &self.chrs[k];
            let moving = match c.player {
                Some(p) => {
                    let pl = &self.players[p];
                    pl.speedforwards * pl.speedforwards + pl.speedsideways * pl.speedsideways > 0.0
                }
                None => c.actiontype != crate::chr::Act::Stand,
            };
            if moving {
                value = fallback * 0.05;
            }
            if k != i && value > rand && crate::chr::body::pos_is_facing_pos(gunpos, vector, c.pos, c.chr_get_hit_radius()) {
                let pos = c.pos;
                self.bgun_play_prop_hit_sound_chr(WEAPON_FARSIGHT, FUNC_PRIMARY, pos);
                self.chr_emit_sparks(k, HITPART_GENERAL, pos, vector);
                self.chr_damage_by_impact(k, damage, vector, DamageFrom::new(Some(i), WEAPON_FARSIGHT, FUNC_PRIMARY), HITPART_GENERAL);
            }
        }
    }
}
