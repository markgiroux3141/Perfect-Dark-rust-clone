//! A simulant firing (`chraction.c`): `chr_set_hand_firing` (the trigger
//! `bot_tick_unpaused` pulls), `chr_tick_shots` and `chr_shoot` (the fire
//! rate, the gun position, the spread, the round's first hit, its damage and
//! effects, the ammo), `chr_update_fireslot` (the shot sound and tracer),
//! `chr_set_firing` (the muzzle flash) and `chr_punch_inflict_damage`.
//!
//! A simulant's round is purely geometric: the first thing along it takes the
//! hit (`cd_test_los_oobok_findclosest` with `g_Vars.useperimshoot`), a chr's or
//! a player's perimeter cylinder included, so it need not hit its target.
//!
//! A launcher fires its projectile instead (`crate::bot::botact`), and a
//! Farsight also shoots through walls at an unseen target.
//!
//! `// SUBST:` PD's round also meets objects (`CDTYPE_OBJS`: mines, the
//! sentry, pickups) / the BG and chrs only.
//!
//! Source: the old repo's `pd_spike/chraction.rs` (shooting) and
//! `pd_complex/fight.rs` (`bot_shot_effects`), checked against
//! `reference/pd_bot_port_sheet.md` §5.

use glam::{Vec2, Vec3};
use pd_core::ids::*;
use pd_core::math;

use super::{Chr, DamageFrom, HITPART_GENERAL};
use crate::bot::weapon_get_num_ticks_per_shot;
use crate::propsnd::DEFAULT_DISTS;
use crate::world::World;

/// What a simulant's round met first.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RoundHit {
    Chr(usize),
    Bg,
}

/// A ray against a vertical cylinder (`GEOTYPE_CYL` under `useperimshoot`):
/// the entry distance.
fn ray_vs_cylinder(o: Vec3, d: Vec3, x: f32, z: f32, r: f32, ymin: f32, ymax: f32) -> Option<f32> {
    let oc = Vec2::new(o.x - x, o.z - z);
    let dd = Vec2::new(d.x, d.z);
    let a = dd.length_squared();
    if a < 1e-9 {
        return None;
    }
    let b = oc.dot(dd);
    let cc = oc.length_squared() - r * r;
    let disc = b * b - a * cc;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / a;
    if t < 0.0 {
        return None;
    }
    let y = o.y + d.y * t;
    (y >= ymin && y <= ymax).then_some(t)
}

impl Chr {
    /// `chr_set_hand_firing` (`chraction.c:9429`): the trigger; let go, the
    /// flash goes (`chr_set_firing(chr, hand, false)`).
    pub fn chr_set_hand_firing(&mut self, hand: usize, firing: bool) {
        self.hand_firing[hand] = firing;
        if !firing {
            if let Some(h) = self.held[hand].as_mut() {
                h.weapon_set_gunfire_visible(false);
            }
        }
    }

    /// `chr_stop_firing` (`chraction.c:9417`): nothing for a simulant.
    pub fn chr_stop_firing(&mut self) {}
}

impl World {
    /// `chr_tick_shots` (`chraction.c:10550`).
    pub(crate) fn chr_tick_shots(&mut self, i: usize) {
        self.chrs[i].firesounddone = false;
        for hand in [HAND_RIGHT, HAND_LEFT] {
            if self.chrs[i].hand_firing[hand] {
                self.chr_shoot(i, hand);
                self.chrs[i].hand_firing[hand] = false;
            }
        }
    }

    /// `gset_get_damage` (`gset.c:475`) for a simulant's gun, with PD's bug: the
    /// damage doubles while the *current player* (the first) fires both hands.
    pub(crate) fn chr_gset_damage(&self, weaponnum: u8, func: usize, maulercharge: f32) -> f32 {
        let f = self.res.gset.func(weaponnum, func);
        let mut damage = match f {
            Some(f) if f.kind() == INVENTORYFUNCTYPE_SHOOT => f.shoot.as_ref().map_or(0.0, |s| s.damage),
            Some(f) if f.kind() == INVENTORYFUNCTYPE_MELEE => {
                let mut d = f.damage;
                if weaponnum == WEAPON_REAPER {
                    d *= self.lv.lvupdate60freal;
                }
                d
            }
            Some(f) if f.kind() == INVENTORYFUNCTYPE_THROW => f.damage,
            _ => 0.0,
        };
        if weaponnum == WEAPON_MAULER {
            damage *= (maulercharge as i32) as f32 / 3.0 + 1.0;
        }
        if let Some(p) = self.players.first() {
            if p.gun.hands[HAND_LEFT].firing && p.gun.hands[HAND_RIGHT].firing {
                damage += damage;
            }
        }
        damage
    }

    /// `bgun_calculate_bot_shot_spread` (`bondgun.c:5142`), with the current
    /// player's (the first's) camera scale and field of view, as PD reads them.
    fn bgun_calculate_bot_shot_spread(&mut self, dir: Vec3, weaponnum: u8, func: usize, burstsdone: bool, squat: bool, dual: bool) -> Vec3 {
        let mut spread = match self.res.gset.func(weaponnum, func) {
            Some(f) if f.kind() == INVENTORYFUNCTYPE_SHOOT => f.shoot.as_ref().map_or(0.0, |s| s.spread),
            _ => 0.0,
        };
        if burstsdone && self.res.gset.has_aim_flag(weaponnum, INVAIMFLAG_ACCURATESINGLESHOT) {
            spread *= 0.25;
        }
        if squat {
            spread *= 0.5;
        }
        if dual {
            spread *= 1.5;
        }
        let (scalex, scaley, fovy) = self.players.first().map_or((0.005_248_7, 0.005_248_7, 60.0), |p| (p.cam.c_scalex, p.cam.c_scaley, p.cam.c_perspfovy));
        let radius = 120.0 * spread / fovy;
        let x = (self.rng.randomfrac() - 0.5) * self.rng.randomfrac() * radius;
        let y = (self.rng.randomfrac() - 0.5) * self.rng.randomfrac() * radius;
        let v = Vec3::new(scalex * x, scaley * y, -1.0).normalize();
        math::look_basis(Vec3::ZERO, dir, Vec3::new(0.0, -1.0, 0.0)).transform_vector3(v)
    }

    /// The first thing along a simulant's round: the BG's shot-blocking tiles
    /// or a chr's perimeter (the shooter's own left out, `chr_set_perim_enabled`).
    fn chr_round_first_hit(&self, i: usize, from: Vec3, dir: Vec3, maxdist: f32) -> Option<(f32, RoundHit)> {
        let mut best = self.level.raycast_shoot(from, dir, maxdist).map(|h| (h.dist, RoundHit::Bg));
        for (j, o) in self.chrs.iter().enumerate() {
            // A dead chr has no geometry; a dying one's still stops rounds.
            if j == i || o.actiontype == super::Act::Dead || o.player.is_some_and(|p| self.players[p].isdead) {
                continue;
            }
            if let Some(t) = ray_vs_cylinder(from, dir, o.pos.x, o.pos.z, o.radius, o.manground, o.manground + o.height) {
                if t <= maxdist && best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, RoundHit::Chr(j)));
                }
            }
        }
        best
    }

    /// `chr_shoot` (`chraction.c:9916`), a simulant's hitscan gun.
    fn chr_shoot(&mut self, i: usize, hand: usize) {
        let Some(weaponnum) = self.chrs[i].held[hand].as_ref().map(|h| h.weaponnum) else { return };
        let gset = self.res.gset.clone();
        let lv60 = self.lv.lvupdate60;
        let (gunfunc, burstsdone, reaperspeed, maulercharge) = {
            let a = self.ab(i);
            (a.gunfunc, a.burstsdone[hand] != 0, a.reaperspeed[hand], a.maulercharge[hand])
        };
        let mut tickspershot = weapon_get_num_ticks_per_shot(&gset, weaponnum, gunfunc);
        let mut shotdue = false;
        let mut makebeam = false;
        if tickspershot <= 0 {
            shotdue = true;
            makebeam = true;
        } else {
            if weaponnum == WEAPON_REAPER && gunfunc == FUNC_PRIMARY {
                let sp208 = (90 - reaperspeed) as f32 * (1.0 / 18.0);
                tickspershot = (tickspershot as f32 * (1.0 + sp208)) as i32;
            }
            let c = &mut self.chrs[i];
            c.firecount[hand] += lv60;
            if c.firecount[hand] >= tickspershot {
                c.firecount[hand] = 0;
                c.unk32c_12 ^= 1 << hand;
                shotdue = true;
                if c.unk32c_12 & (1 << hand) != 0 || weaponnum == WEAPON_LASER {
                    makebeam = true;
                }
            }
        }
        let mut firingthisframe = false;
        let mut normalshoot = true;
        let mut gunpos = Vec3::ZERO;
        let mut hitpos = Vec3::ZERO;
        if shotdue {
            let roty = self.chrs[i].chr_get_aimx_angle();
            firingthisframe = true;
            gunpos = self.chrs[i].chr_get_gun_pos(hand).unwrap_or_else(|| {
                // The gun is off screen: a quick, inexact position.
                let pp = self.chrs[i].pos;
                let mut g = Vec3::new(pp.x, pp.y + 30.0, pp.z);
                if hand == HAND_LEFT {
                    g.x += roty.cos() * 10.0;
                    g.z += -roty.sin() * 10.0;
                } else {
                    g.x += -roty.cos() * 10.0;
                    g.z += roty.sin() * 10.0;
                }
                g
            });
            // Don't fire a gun pushed through a wall or into another chr.
            let pp = self.chrs[i].pos;
            let to_gun = gunpos - pp;
            if self.chr_round_first_hit(i, pp, to_gun.normalize_or_zero(), to_gun.length()).is_some() {
                firingthisframe = false;
            }
            if firingthisframe {
                let c = &self.chrs[i];
                let dual = c.held[0].is_some() && c.held[1].is_some();
                let squat = super::thirdperson::bot_guess_crouch_pos(c.height) == CROUCHPOS_SQUAT;
                let dir0 = c.chr_shot_dir();
                let dir = self.bgun_calculate_bot_shot_spread(dir0, weaponnum, gunfunc, burstsdone, squat, dual);
                // The Farsight at an unseen target (`chraction.c:10074`).
                if weaponnum == WEAPON_FARSIGHT && !self.ab(i).targetinsight {
                    makebeam = true;
                    self.botact_shoot_farsight(i, dir, gunpos);
                }
                hitpos = gunpos + dir * 65536.0;
                let launcher = matches!(weaponnum, WEAPON_ROCKETLAUNCHER | WEAPON_SLAYER | WEAPON_DEVASTATOR | WEAPON_CROSSBOW | WEAPON_KINGSCEPTRE) || (weaponnum == WEAPON_SUPERDRAGON && gunfunc == FUNC_SECONDARY);
                let mut maulercharge = maulercharge;
                if launcher {
                    // Projectile launchers (`chraction.c:10132`): a simulant
                    // always fires, however close.
                    makebeam = false;
                    normalshoot = false;
                    self.chr_shoot_projectile(i, weaponnum, gunfunc, gunpos, dir);
                } else if weaponnum == WEAPON_MAULER && gunfunc == FUNC_SECONDARY {
                    // gset.maulercharge = aibot->maulercharge × 10, then spent.
                    maulercharge *= 10.0;
                    self.ab_mut(i).maulercharge[hand] = 0.0;
                }
                let hit = if normalshoot { self.chr_round_first_hit(i, gunpos, dir, 65536.0) } else { None };
                self.navstats.rounds += 1;
                if matches!(hit, Some((_, RoundHit::Chr(_)))) {
                    self.navstats.round_hits += 1;
                }
                if let Some((t, _)) = hit {
                    hitpos = gunpos + dir * t;
                }
                match hit {
                    Some((_, RoundHit::Chr(j))) => {
                        let damage = self.chr_gset_damage(weaponnum, gunfunc, maulercharge);
                        self.bgun_play_prop_hit_sound_chr(weaponnum, gunfunc, hitpos);
                        self.chr_emit_sparks(j, HITPART_GENERAL, hitpos, dir);
                        self.chr_damage_by_impact(j, damage, dir, DamageFrom::new(Some(i), weaponnum, gunfunc), HITPART_GENERAL);
                    }
                    Some((_, RoundHit::Bg)) => {
                        // bgun_play_bg_hit_sound(gset, hitpos, -1, rooms): no texture.
                        self.bgun_play_bg_hit_sound(0, weaponnum, gunfunc, hitpos, None);
                        self.fx.sparks.create(&mut self.rng, hitpos, Vec3::ZERO, Vec3::ZERO, SPARKTYPE_DEFAULT);
                    }
                    None => {}
                }
                if weaponnum == WEAPON_PHOENIX && gunfunc == FUNC_SECONDARY {
                    self.explosion_create_by_chr(i, hitpos, EXPLOSIONTYPE_PHOENIX);
                }
            }
        }
        if makebeam {
            makebeam = matches!(
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
                    | WEAPON_DRAGON
                    | WEAPON_K7AVENGER
                    | WEAPON_AR34
                    | WEAPON_SUPERDRAGON
                    | WEAPON_REAPER
                    | WEAPON_SNIPERRIFLE
                    | WEAPON_FARSIGHT
                    | WEAPON_TRANQUILIZER
                    | WEAPON_LASER
                    | WEAPON_PP9I
                    | WEAPON_CC13
                    | WEAPON_KL01313
                    | WEAPON_KF7SPECIAL
                    | WEAPON_ZZT
                    | WEAPON_DMC
                    | WEAPON_AR53
                    | WEAPON_RCP45
            );
        }
        self.chr_update_fireslot(i, hand, weaponnum, gunfunc, firingthisframe, firingthisframe && makebeam, gunpos, hitpos);
        let a = self.ab_mut(i);
        if firingthisframe && a.loadedammo[hand] > 0 {
            a.loadedammo[hand] -= 1;
        }
        // chr_set_firing (`chraction.c:9392`): the muzzle flash, which lights
        // the chr's room (`room_flash_lighting(room, 48, 128)`).
        let flash = self.chrs[i].held[hand].as_mut().is_some_and(|h| h.weapon_set_gunfire_visible(firingthisframe && normalshoot));
        if flash {
            if let Some(room) = self.chrs[i].rooms.first().copied() {
                self.lights.room(room).flash(48.0, 128);
            }
        }
    }

    /// `chr_update_fireslot` (`chraction.c:8803`): the gun's shot sound at the
    /// chr, at most once per chr per tick and not again until its
    /// `duration60` has run out; and the tracer.
    #[allow(clippy::too_many_arguments)]
    fn chr_update_fireslot(&mut self, i: usize, hand: usize, weaponnum: u8, func: usize, withsound: bool, withbeam: bool, from: Vec3, to: Vec3) {
        let shoot = self.res.gset.func(weaponnum, func).and_then(|f| f.shoot.clone());
        let (duration, soundnum) = shoot.map_or((0, 0), |s| (s.duration60, s.shootsound));
        let lvframe60 = self.lv.lvframe60;
        let c = &self.chrs[i];
        let playsound = withsound && (duration <= 0 || (!c.firesounddone && lvframe60 > c.fireslots[hand].endlvframe));
        if playsound {
            let pos = c.pos;
            if soundnum != 0 {
                self.sound_at(soundnum, 1.0, pos, DEFAULT_DISTS);
            }
            let c = &mut self.chrs[i];
            c.fireslots[hand].endlvframe = lvframe60 + duration;
            c.firesounddone = true;
        }
        if withbeam {
            let c = &mut self.chrs[i];
            c.fireslots[hand].beam.create(&mut self.rng, weaponnum as i32, from, to);
        }
    }

    /// `bgun_play_prop_hit_sound` (`bondgun.c:8651`) on a chr: a punch's thump,
    /// a knife's stab, else `SFXMAP_8076_HIT_CHR`.
    pub(crate) fn bgun_play_prop_hit_sound_chr(&mut self, weaponnum: u8, func: usize, pos: Vec3) {
        let rand1 = self.rng.random();
        let _rand2 = self.rng.random();
        if self.lv.lvupdate240 <= 0 {
            return;
        }
        let whip = func == FUNC_SECONDARY && matches!(weaponnum, WEAPON_FALCON2 | WEAPON_FALCON2_SILENCER | WEAPON_FALCON2_SCOPE | WEAPON_DY357MAGNUM | WEAPON_DY357LX);
        let sound = if matches!(weaponnum, WEAPON_REMOTEMINE | WEAPON_PROXIMITYMINE | WEAPON_TIMEDMINE) {
            0x80aa
        } else if weaponnum == WEAPON_COMBATKNIFE || weaponnum == WEAPON_BOLT {
            0x05f6
        } else if weaponnum == WEAPON_UNARMED || whip {
            [0x002f, 0x0030, 0x0031][(rand1 % 3) as usize]
        } else {
            0x8076
        };
        self.sound_at(sound, 1.0, pos, DEFAULT_DISTS);
    }

    /// `chr_punch_inflict_damage(chr, damage, range, false)` (`chraction.c:7732`):
    /// lands only inside a tight cone (20/256 of a turn), closer than `range`,
    /// with a clear line; the miss sound either way.
    pub(crate) fn chr_punch_inflict_damage(&mut self, i: usize, damage: f32, range: f32) {
        let (weaponnum, func) = (self.ab(i).weaponnum, self.ab(i).gunfunc);
        if let Some(t) = self.chrs[i].target {
            let (cp, tp) = (self.chrs[i].pos, self.chrs[t].pos);
            if self.chrs[i].chr_is_pos_in_fov(tp, 20) && cp.distance(tp) < range && self.level.los_autoflags(cp, tp) {
                let v = Vec3::new(tp.x - cp.x, 0.0, tp.z - cp.z).normalize_or_zero();
                self.bgun_play_prop_hit_sound_chr(weaponnum, func, tp);
                let d = self.chr_gset_damage(weaponnum, func, 0.0) * damage;
                self.chr_damage_by_general(t, d, v, DamageFrom::new(Some(i), weaponnum, func), 200);
            }
        }
        // weapon_play_melee_miss_sound(weaponnum, chr->prop).
        let pos = self.chrs[i].pos;
        self.weapon_play_melee_miss_sound_at(weaponnum, pos);
    }
}
