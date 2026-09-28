//! `autogunobj`: the Laptop Gun deployed as a sentry (`propobj.c`:
//! `laptop_deploy` `:17488`, `autogun_tick` `:8557`, `autogun_init_matrices`
//! `:9011`, `autogun_tick_shoot` `:9086`, `apply_speed` / `apply_rotation`
//! `:3562`).
//!
//! In a Combat Simulator match (`g_Vars.normmplayerisrunning`) a sentry tries
//! one MP chr a tick, round-robin, its owner and (with teams) its owner's team
//! skipped; it swivels at most ±12.56 rad, wakes for a target within 70° of its
//! aim and 50 m, fires every other tick at half the RC-P45's damage, and draws a
//! tracer every fourth round. Its targets are the world's chrs.
//!
//! Source: the old repo's `pd_guns/autogun.rs`, whose targets were the firing
//! range's boards (`fr_choose_autogun_target`, the training branch, which the
//! Combat Simulator never takes).

use glam::{Mat4, Vec3};
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::math::{self, baddtor, dtor};
use pd_core::model::{ModelDef, NodeKind};

use super::Obj;
use crate::fx::Beam;
use crate::propsnd::DEFAULT_DISTS;
use crate::stage::PerimCyl;
use crate::world::World;

/// `g_PropExplosionTypes[8 + MODEL_CHRAUTOGUN]` (`modeldata/general.c:853`): a
/// destroyed sentry's blast.
pub const EXPLOSIONTYPE_AUTOGUN_DESTROYED: usize = 7;

/// `struct autogunobj` beyond `defaultobj`.
#[derive(Clone, Debug, Default)]
pub struct Autogun {
    pub yzero: f32,
    pub xzero: f32,
    pub yrot: f32,
    pub xrot: f32,
    pub yspeed: f32,
    pub xspeed: f32,
    pub barrelspeed: f32,
    pub barrelrot: f32,
    pub ymaxleft: f32,
    pub ymaxright: f32,
    pub maxspeed: f32,
    pub aimdist: f32,
    pub firecount: i32,
    pub firing: bool,
    pub ammoquantity: i32,
    pub lastseebond60: i32,
    pub lastaimbond60: i32,
    pub allowsoundframe: i32,
    pub shotbondsum: f32,
    /// `autogun->target`: a player. M6: any MP chr.
    pub target: Option<usize>,
    /// `targetteam`: `~chr->team`, never 0 for a thrown laptop.
    pub targetteam: u8,
    /// `nextchrtest`: the round-robin over `g_MpAllChrPtrs`.
    pub nextchrtest: i32,
    /// `obj->damage` (the sentry is the only mortal object the guns make).
    pub damage: i32,
    /// `OBJH2FLAG_DESTROYED`.
    pub destroyed: bool,
    /// This tick's flashes (`chrgunfire` visibility).
    pub fireleft: bool,
    pub fireright: bool,
    /// `g_ThrownLaptopBeams[i]`.
    pub beam: Beam,
}

/// `apply_speed` (`propobj.c:3562`): move `distdone` towards `maxdist` with an
/// acceleration, a deceleration and a top speed, stopping exactly on it.
pub fn apply_speed(lv: &Lv, distdone: &mut f32, maxdist: f32, speedptr: &mut f32, accel: f32, decel: f32, maxspeed: f32) {
    let mut speed = *speedptr;
    for _ in 0..lv.lvupdate60 {
        let limit = speed * speed * 0.5 / decel;
        let distremaining = maxdist - *distdone;
        if distremaining > 0.0 {
            if speed > 0.0 && distremaining <= limit {
                speed -= decel;
                if speed < decel {
                    speed = decel;
                }
            } else if speed < maxspeed {
                if speed < 0.0 {
                    speed += decel;
                } else {
                    speed += accel;
                }
                if speed > maxspeed {
                    speed = maxspeed;
                }
            }
            if speed >= distremaining {
                *distdone = maxdist;
                break;
            }
            *distdone += speed;
        } else {
            if speed < 0.0 && -distremaining <= limit {
                speed += decel;
                if speed > -decel {
                    speed = -decel;
                }
            } else if speed > -maxspeed {
                if speed > 0.0 {
                    speed -= decel;
                } else {
                    speed -= accel;
                }
                if speed < -maxspeed {
                    speed = -maxspeed;
                }
            }
            if speed <= distremaining {
                *distdone = maxdist;
                break;
            }
            *distdone += speed;
        }
    }
    *speedptr = speed;
}

/// `apply_rotation` (`propobj.c:3629`): `apply_speed` on an angle, the short
/// way round, kept in `0..BADDTOR(360)`.
pub fn apply_rotation(lv: &Lv, angle: &mut f32, maxrot: f32, speed: &mut f32, accel: f32, decel: f32, maxspeed: f32) {
    let mut maxrot = maxrot;
    let tmp = maxrot - *angle;
    if tmp < dtor(-180.0) {
        maxrot += baddtor(360.0);
    } else if tmp >= dtor(180.0) {
        maxrot -= baddtor(360.0);
    }
    apply_speed(lv, angle, maxrot, speed, accel, decel, maxspeed);
    if *angle < 0.0 {
        *angle += baddtor(360.0);
    }
    if *angle >= baddtor(360.0) {
        *angle -= baddtor(360.0);
    }
}

/// `chr_get_aim_limit_angle` (`chraction.c:9450`): how close the aim must be
/// to fire, by the target's squared distance.
pub fn chr_get_aim_limit_angle(sqdist: f32) -> f32 {
    if sqdist > 1600.0 * 1600.0 {
        baddtor(1.074_626_8)
    } else if sqdist > 800.0 * 800.0 {
        baddtor(2.155_688_5)
    } else if sqdist > 400.0 * 400.0 {
        baddtor(4.285_714)
    } else if sqdist > 200.0 * 200.0 {
        baddtor(8.571_428)
    } else {
        baddtor(14.4)
    }
}

/// Keep an angle difference in `-BADDTOR(180)..BADDTOR(180]` the way
/// `autogun_tick` does (one wrap each way).
fn wrap180(mut a: f32) -> f32 {
    if a < 0.0 {
        a += baddtor(360.0);
    }
    if a > baddtor(180.0) {
        a -= baddtor(360.0);
    }
    a
}

impl Autogun {
    /// `laptop_deploy`'s fields (`propobj.c:17488`) for a player's sentry with
    /// `ammo` rounds (the Laptop's reserve, at most 200).
    pub fn deployed(ammo: i32, targetteam: u8) -> Autogun {
        Autogun {
            aimdist: 5000.0,
            targetteam,
            nextchrtest: 0,
            lastseebond60: -1,
            lastaimbond60: -1,
            allowsoundframe: -1,
            ammoquantity: ammo,
            ymaxleft: 12.56,
            ymaxright: -12.56,
            maxspeed: 0.0697,
            beam: Beam::default(),
            ..Autogun::default()
        }
    }

    /// `autogun_init_matrices` (`propobj.c:9011`) in world space: the turret
    /// (matrix 1) yaws in world terms at part 1's position; the gun (2) pitches
    /// on it; the barrels (parts 3 and 6) spin; part 4 follows the gun.
    pub fn init_matrices(&self, def: &ModelDef, scale: f32, mats: &mut [Mat4]) {
        let root = mats[0];
        let part = |p: i32| -> Option<(Vec3, usize)> {
            let node = def.get_part(p)?;
            match def.nodes[node].kind {
                NodeKind::Position { pos, mtx, .. } => Some((pos, mtx[0] as usize)),
                _ => None,
            }
        };
        let mut yrot = self.yrot + baddtor(90.0);
        if yrot >= baddtor(360.0) {
            yrot -= baddtor(360.0);
        }
        let xrot = -self.xrot;
        let Some((p1, _)) = part(MODELPART_AUTOGUN_0001) else { return };
        let sp4c = root.transform_point3(p1);
        let mut m1 = math::load_y_rotation(yrot);
        math::set_translation(&mut m1, sp4c);
        math::scale3(&mut m1, scale);
        if let Some(m) = mats.get_mut(1) {
            *m = m1;
        }
        let Some((p2, i2)) = part(MODELPART_AUTOGUN_0002) else { return };
        let mut m2 = math::load_z_rotation(xrot);
        math::set_translation(&mut m2, p2);
        let m2 = math::mul(&m1, &m2);
        if let Some(m) = mats.get_mut(2) {
            *m = m2;
        }
        // model_find_node_mtx(model, node2, 256): the node's second matrix,
        // half the pitch.
        if let Some(n2) = def.get_part(MODELPART_AUTOGUN_0002) {
            if let Some(i) = def.find_node_mtx_index(n2, 256).filter(|&i| i != i2) {
                let mut m = math::load_z_rotation(xrot * 0.5);
                math::set_translation(&mut m, p2);
                if let Some(t) = mats.get_mut(i) {
                    *t = math::mul(&m1, &m);
                }
            }
        }
        for (p, spin) in [(MODELPART_AUTOGUN_0003, true), (MODELPART_AUTOGUN_0004, false), (MODELPART_AUTOGUN_0006, true)] {
            if let Some((pp, i)) = part(p) {
                let mut m = if spin { math::load_x_rotation(self.barrelrot) } else { Mat4::IDENTITY };
                math::set_translation(&mut m, pp);
                if let Some(t) = mats.get_mut(i) {
                    *t = math::mul(&m2, &m);
                }
            }
        }
    }
}

/// `obj_damage` on a sentry (`propobj.c:14485`): damage × 250 (at least 1)
/// until destroyed; past `maxdamage` (1000) it is destroyed
/// (`obj_check_destroyed`) and deactivated. Returns true when this blow
/// destroyed it (its blast is the caller's).
pub fn autogun_damage(o: &mut Obj, damage: f32) -> bool {
    let Some(a) = o.autogun.as_mut() else { return false };
    let mut damage = damage;
    if !a.destroyed {
        damage *= 250.0;
        if damage < 1.0 {
            damage = 1.0;
        }
    } else {
        // obj_get_shots_taken: capped within the destroyed level.
        let max = 4.0 - ((a.damage & 0xff) % 4) as f32;
        damage = damage.clamp(1.0, max);
    }
    a.damage = (a.damage as f32 + damage).min(32767.0) as i32;
    o.flags |= OBJFLAG_AUTOGUN_DAMAGED;
    let mut destroyed_now = false;
    if !a.destroyed && a.damage > 1000 {
        a.damage = 0;
        a.destroyed = true;
        destroyed_now = true;
        // M9: obj_deform bends the model's vertices.
    }
    if a.destroyed && (a.damage >> 2) + 1 == 1 {
        o.flags |= OBJFLAG_DEACTIVATED;
    }
    destroyed_now
}

/// Where the segment `from → to` first meets a chr's perimeter cylinder
/// (`chr_get_geometry`'s `GEOTYPE_CYL`), as the fraction along it.
pub(crate) fn segment_cyl(from: Vec3, to: Vec3, c: &PerimCyl) -> Option<f32> {
    let d = to - from;
    let (fx, fz) = (from.x - c.x, from.z - c.z);
    let a = d.x * d.x + d.z * d.z;
    let b = 2.0 * (fx * d.x + fz * d.z);
    let cc = fx * fx + fz * fz - c.radius * c.radius;
    let mut t = if cc <= 0.0 {
        0.0
    } else {
        if a <= 0.0 {
            return None;
        }
        let disc = b * b - 4.0 * a * cc;
        if disc < 0.0 {
            return None;
        }
        (-b - disc.sqrt()) / (2.0 * a)
    };
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    let y = from.y + d.y * t;
    if y < c.ymin || y > c.ymax {
        // Through the cap: where the segment crosses the top or bottom inside the circle.
        let mut best = None;
        for cap in [c.ymin, c.ymax] {
            if d.y.abs() > 1e-9 {
                let tc = (cap - from.y) / d.y;
                if (0.0..=1.0).contains(&tc) {
                    let p = from + d * tc;
                    if (p.x - c.x).powi(2) + (p.z - c.z).powi(2) <= c.radius * c.radius && best.is_none_or(|b| tc < b) {
                        best = Some(tc);
                    }
                }
            }
        }
        t = best?;
    }
    Some(t)
}

impl World {
    /// The MP chrs a sentry can target, as `g_MpAllChrPtrs`: the players. M6:
    /// the simulants after them.
    fn mp_num_chrs(&self) -> usize {
        self.chrs.len()
    }

    /// `autogun_tick` (`propobj.c:8557`), the regular behaviour (not the
    /// malfunctioning or windmill sentries of the solo missions).
    pub(crate) fn autogun_tick(&mut self, o: &mut Obj) {
        let lv = self.lv.clone();
        let owner = o.owner();
        let gunpos = o.pos;
        let numchrs = self.mp_num_chrs();
        let teams = self.setup.options & MPOPTION_TEAMSENABLED != 0;
        let chr_team: Vec<u8> = self.chrs.iter().map(|c| c.team).collect();
        // CHRCFLAG_HIDDEN, cloaked, dead: not a target, not tracked.
        let untargetable: Vec<bool> = (0..numchrs).map(|i| self.chrs[i].cloaked || self.chr_is_dead(i)).collect();
        let isplayer: Vec<bool> = self.chrs.iter().map(|c| c.player.is_some()).collect();
        let positions: Vec<Vec3> = self.chrs.iter().map(|c| c.pos).collect();
        let level = self.level.clone();
        let flags = o.flags;
        let Some(a) = o.autogun.as_mut() else { return };
        let mut target: Option<usize> = None;
        let mut awake = false;
        let mut spinup = false;
        let mut insight = false;
        let mut limitangle = 0.0;
        if a.ammoquantity == 0 {
            // No target.
        } else if a.target.is_some() {
            target = a.target;
        } else if a.targetteam != 0 {
            // One chr tried a tick, round-robin.
            loop {
                a.nextchrtest += 1;
                if a.nextchrtest >= numchrs as i32 {
                    a.nextchrtest = -1;
                    break;
                }
                let i = a.nextchrtest as usize;
                if i == owner {
                    continue;
                }
                if teams && chr_team[i] & a.targetteam == 0 {
                    continue;
                }
                if !untargetable[i] {
                    target = Some(i);
                    break;
                }
            }
        }
        let mut goalyrot = a.yzero;
        let mut goalxrot = a.xzero;
        if let Some(t) = target {
            let tp = positions[t];
            let (xdist, mut ydist, zdist) = (tp.x - gunpos.x, tp.y - gunpos.y, tp.z - gunpos.z);
            // PROPTYPE_PLAYER: aim 20 cm below the eye.
            if isplayer[t] {
                ydist -= 20.0;
            }
            let mut sqdist = xdist * xdist + zdist * zdist;
            let mut dist = sqdist.sqrt();
            let horizdist = dist;
            if flags & OBJFLAG_AUTOGUN_3DRANGE != 0 {
                sqdist += ydist * ydist;
                dist = sqdist.sqrt();
            }
            limitangle = chr_get_aim_limit_angle(sqdist);
            if dist <= a.aimdist {
                let targetangleh = math::atan2f(xdist, zdist);
                let targetanglev = math::atan2f(ydist, horizdist);
                if flags & OBJFLAG_AUTOGUN_DAMAGED != 0 || flags & OBJFLAG_AUTOGUN_SEENTARGET != 0 {
                    awake = true;
                } else {
                    let f12 = wrap180(targetangleh - a.yrot);
                    if f12 < baddtor(70.0) && f12 > baddtor(-70.0) {
                        awake = true;
                    }
                }
                if awake {
                    let mut relangleh = targetangleh - a.yzero;
                    if relangleh < dtor(-180.0) {
                        relangleh += baddtor(360.0);
                    } else if relangleh >= dtor(180.0) {
                        relangleh -= baddtor(360.0);
                    }
                    // Tracked unless dead, hidden or cloaked.
                    let track = !untargetable[t];
                    // cd_test_los_oobfail(..., CDTYPE_ALL, GEOFLAG_BLOCK_SIGHT),
                    // both perimeters off. SUBST: the other chrs' perimeters don't
                    // block the look.
                    if relangleh <= a.ymaxleft && relangleh >= a.ymaxright && track && level.los(gunpos, tp) {
                        o.flags |= OBJFLAG_AUTOGUN_SEENTARGET;
                        insight = true;
                        goalxrot = targetanglev;
                        goalyrot = targetangleh;
                        if a.target.is_none() {
                            a.target = Some(t);
                        }
                    } else if a.lastseebond60 >= 0 && a.lastseebond60 > lv.lvframe60 - 120 {
                        goalyrot = a.yrot;
                        goalxrot = a.xrot;
                    } else {
                        awake = false;
                    }
                }
            }
        }
        if !awake {
            a.target = None;
        }
        // The turret swivels left and right while firing.
        if a.firing {
            goalyrot += limitangle * 0.8 * ((lv.lvframe60 % 120) as f32 * baddtor(3.0)).sin();
            if goalyrot < 0.0 {
                goalyrot += baddtor(360.0);
            }
            if goalyrot >= baddtor(360.0) {
                goalyrot -= baddtor(360.0);
            }
        }
        let mut f0 = goalyrot - a.yzero;
        if f0 < dtor(-180.0) {
            f0 += baddtor(360.0);
        } else if f0 >= dtor(180.0) {
            f0 -= baddtor(360.0);
        }
        if f0 > a.ymaxleft {
            goalyrot = a.yzero + a.ymaxleft;
        } else if f0 < a.ymaxright {
            goalyrot = a.yzero + a.ymaxright;
        }
        if goalyrot < 0.0 {
            goalyrot += baddtor(360.0);
        }
        if goalyrot >= baddtor(360.0) {
            goalyrot -= baddtor(360.0);
        }
        let acc = 0.000_872_525_7;
        let maxspeed = a.maxspeed;
        apply_rotation(&lv, &mut a.yrot, goalyrot, &mut a.yspeed, acc, acc, maxspeed);
        apply_rotation(&lv, &mut a.xrot, goalxrot, &mut a.xspeed, acc, acc, maxspeed);
        let f12 = wrap180(goalyrot - a.yrot);
        let f2 = wrap180(goalxrot - a.xrot);
        a.firing = false;
        if awake {
            if f12 < limitangle && -limitangle < f12 && f2 < limitangle && -limitangle < f2 {
                a.firing = true;
                spinup = true;
                if insight {
                    a.lastseebond60 = lv.lvframe60;
                    a.lastaimbond60 = lv.lvframe60;
                }
            } else {
                let f0 = 2.0 * limitangle;
                if f12 < f0 && -f0 < f12 && f2 < f0 && -f0 < f2 {
                    a.firing = true;
                    spinup = true;
                    if insight {
                        a.lastseebond60 = lv.lvframe60;
                    }
                } else if a.lastseebond60 >= 0 && a.lastseebond60 > lv.lvframe60 - 120 {
                    a.firing = true;
                    spinup = true;
                }
            }
        }
        if spinup {
            a.barrelspeed = (a.barrelspeed + 0.009_971_722 * lv.lvupdate60freal).min(0.598_303_3);
        } else if a.barrelspeed > 0.0 {
            for _ in 0..lv.lvupdate60 {
                a.barrelspeed *= 0.99;
            }
            if a.barrelspeed <= 0.0001 {
                a.barrelspeed = 0.0;
            }
        }
        if a.barrelspeed > 0.0 {
            a.barrelrot += a.barrelspeed * lv.lvupdate60freal;
            while a.barrelrot >= baddtor(360.0) {
                a.barrelrot -= baddtor(360.0);
            }
        }
    }

    /// `autogun_tick_shoot` (`propobj.c:9086`), the multiplayer branch: a round
    /// every other tick from the flash along the aim; the first chr (by its
    /// perimeter) or shot-blocking tile it meets takes it; a chr takes half the
    /// RC-P45's damage unless it is the owner (then nothing fires); a miss sparks
    /// and ricochets; every fourth round draws a tracer.
    pub(crate) fn autogun_tick_shoot(&mut self, o: &mut Obj, _pi: usize) {
        if self.lv.lvupdate240 == 0 {
            return;
        }
        let mats = o.init_matrices();
        let def = o.def.clone();
        let objpos = o.pos;
        let owner = o.owner();
        let deactivated = o.flags & OBJFLAG_DEACTIVATED != 0;
        let lv = self.lv.clone();
        let level = self.level.clone();
        let teams = self.setup.teams_enabled();
        let Some(a) = o.autogun.as_mut() else { return };
        a.fireleft = false;
        a.fireright = false;
        if !a.firing || deactivated {
            return;
        }
        a.firecount += 1;
        let mut fireleft = a.firecount % 2 == 0;
        // PD asks for FLASHLEFT here, not FLASHRIGHT.
        let mut fireright = def.get_part(MODELPART_AUTOGUN_FLASHLEFT).is_some() && a.firecount % 2 == 1;
        if fireleft || fireright {
            let mut makebeam = a.firecount % 4 == 0;
            let flashnode = if a.firecount & 7 != 0 || def.get_part(MODELPART_AUTOGUN_FLASHRIGHT).is_none() { def.get_part(MODELPART_AUTOGUN_FLASHLEFT) } else { def.get_part(MODELPART_AUTOGUN_FLASHRIGHT) };
            // The flash's position (PROPFLAG_ONTHISSCREENTHISTICK: a sentry is
            // always drawn by someone), or the prop's if a wall is between.
            let mut gunpos = objpos;
            if let Some((mi, pos)) = flashnode.and_then(|n| gunfire_node(&def, n)) {
                let p = mats[mi].transform_point3(pos);
                gunpos = if level.raycast_shoot(objpos, (p - objpos).normalize_or_zero(), (p - objpos).length()).is_some() { objpos } else { p };
            }
            let dir = Vec3::new(a.xrot.cos() * a.yrot.sin(), a.xrot.sin(), a.xrot.cos() * a.yrot.cos());
            let mut hitpos = gunpos + dir * 65536.0;
            let mut missed = false;
            // cd_test_los_oobok_findclosest(..., CDTYPE_ALL, GEOFLAG_BLOCK_SHOOT).
            let bg = level.raycast_shoot(gunpos, dir, 65536.0).map(|h| h.dist);
            let mut hitchr: Option<(usize, f32)> = None;
            for (i, c) in self.chrs.iter().enumerate() {
                let Some(perim) = c.perim() else { continue };
                if let Some(t) = segment_cyl(gunpos, hitpos, &perim) {
                    let d = t * 65536.0;
                    if bg.is_none_or(|b| d < b) && hitchr.is_none_or(|(_, bd)| d < bd) {
                        hitchr = Some((i, d));
                    }
                }
            }
            let mut struck: Option<(usize, Vec3)> = None;
            if let Some((i, d)) = hitchr {
                hitpos = gunpos + dir * d;
                if i == owner || (owner < self.chrs.len() && teams && self.chrs[i].team == self.chrs[owner].team) {
                    // A teammate entered the line of fire (chr_compare_teams
                    // COMPARE_FRIENDS, propobj.c:9224).
                    makebeam = false;
                    fireleft = false;
                    fireright = false;
                }
                if fireleft || fireright {
                    struck = Some((i, hitpos));
                }
            } else if let Some(d) = bg {
                hitpos = gunpos + dir * d;
                missed = true;
            }
            a.fireleft = fireleft;
            a.fireright = fireright;
            if (fireleft || fireright) && a.ammoquantity > 0 && a.ammoquantity != 255 {
                a.ammoquantity -= 1;
            }
            if makebeam {
                a.beam.create(&mut self.rng, WEAPON_RCP45 as i32, gunpos, hitpos);
            }
            if let Some((i, pos)) = struck {
                // bgun_play_prop_hit_sound, chr_emit_sparks and
                // chr_damage_by_impact at the RC-P45's damage × 0.5.
                self.bgun_play_prop_hit_sound_chr(WEAPON_RCP45, FUNC_PRIMARY, pos);
                self.chr_emit_sparks(i, crate::chr::HITPART_GENERAL, pos, dir);
                let damage = self.res.gset.func(WEAPON_RCP45, FUNC_PRIMARY).and_then(|f| f.shoot.as_ref()).map_or(1.8, |s| s.damage) * 0.5;
                self.chr_damage_by_impact(i, damage, dir, crate::chr::DamageFrom::new(Some(owner), WEAPON_RCP45, FUNC_PRIMARY), crate::chr::HITPART_GENERAL);
            }
            if missed {
                self.fx.sparks.create(&mut self.rng, hitpos, Vec3::ZERO, Vec3::ZERO, SPARKTYPE_DEFAULT);
                self.bgun_play_bg_hit_sound(owner, WEAPON_RCP45, FUNC_PRIMARY, hitpos, None);
            }
        }
        let Some(a) = o.autogun.as_mut() else { return };
        if a.allowsoundframe < lv.lvframe60 {
            // MODEL_CHRAUTOGUN's fire: SFXMAP_8044, at most one every 4 ticks.
            a.allowsoundframe = 4 + lv.lvframe60;
            self.sound_at(0x8044, 1.0, objpos, DEFAULT_DISTS);
        }
    }

    /// `laptop_deploy` (`propobj.c:17488`)'s replacement of a player's earlier
    /// sentry: it blows up (`EXPLOSIONTYPE_LAPTOP`) and is freed.
    pub(crate) fn laptop_replace(&mut self, index: usize, pi: usize) {
        let Some(old) = self.props.thrown_laptops.get_mut(index).and_then(|s| s.take()) else { return };
        let Some(pos) = self.props.get(old).map(|o| o.pos) else { return };
        self.props.objs.retain(|o| o.id != old);
        let cam = self.exp_cam(pi);
        let level = self.level.clone();
        let mut out = super::explosions::ExpOut::default();
        self.explosions.create_simple(&mut self.rng, &mut self.fx.smokes, &crate::world::StageExp { level: &level }, None, pos, EXPLOSIONTYPE_LAPTOP, index as i32, cam, &mut out);
        self.apply_explosion_out(out);
    }

    /// A destroyed sentry's blast (`obj_check_destroyed`, `propobj.c:13813`).
    pub(crate) fn autogun_destroyed(&mut self, pos: Vec3, playernum: i32, pi: usize) {
        let cam = self.exp_cam(pi);
        let level = self.level.clone();
        let mut out = super::explosions::ExpOut::default();
        self.explosions.create_complex(&mut self.rng, &mut self.fx.smokes, &crate::world::StageExp { level: &level }, None, pos, EXPLOSIONTYPE_AUTOGUN_DESTROYED, playernum, cam, &mut out);
        self.apply_explosion_out(out);
    }
}

/// A CHRGUNFIRE node's joint and position (`rodata->chrgunfire.pos`).
pub fn gunfire_node(def: &ModelDef, node: usize) -> Option<(usize, Vec3)> {
    let NodeKind::ChrGunfire { pos, .. } = def.nodes.get(node)?.kind else { return None };
    let parent = def.nodes[node].parent?;
    Some((def.find_node_mtx_index(parent, 0)?, pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `apply_speed` accelerates, cruises and stops exactly on the target.
    #[test]
    fn apply_speed_stops_on_the_target() {
        let lv = Lv { lvupdate60: 1, ..Lv::new() };
        let (mut d, mut v) = (0.0, 0.0);
        for _ in 0..400 {
            apply_speed(&lv, &mut d, 10.0, &mut v, 0.01, 0.01, 0.2);
        }
        assert_eq!(d, 10.0);
    }

    /// A cylinder across the segment's path is met at its surface.
    #[test]
    fn segment_meets_a_perimeter() {
        let c = PerimCyl { x: 100.0, z: 0.0, radius: 30.0, ymin: 0.0, ymax: 200.0 };
        let t = segment_cyl(Vec3::new(0.0, 100.0, 0.0), Vec3::new(200.0, 100.0, 0.0), &c).unwrap();
        assert!((t * 200.0 - 70.0).abs() < 0.01, "{t}");
        assert!(segment_cyl(Vec3::new(0.0, 300.0, 0.0), Vec3::new(200.0, 300.0, 0.0), &c).is_none());
    }
}
