//! Hurting chrs (`chraction.c`): `chr_damage` in normal multiplayer, both its
//! branches (a player victim's health, a simulant's damage), the hit parts'
//! multipliers, flinches, grunts, blood (`chr_emit_sparks`), `chr_die`, and the
//! simulant's death animation and fade (`chr_tick_die`, `chr_tick_dead`).
//!
//! Not yet: shields (`cshield`, M8), `ONEHITKILLS` and the shot-count stats (M7),
//! disarming (`FUNCFLAG_DISARM`, `bgun_disarm` / `bot_disarm`, M8), the
//! solo-only branches (knockouts, argh animations, difficulty scaling).
//!
//! Source: the old repo's `pd_spike/chraction.rs` (damage) and
//! `pd_complex/fight.rs` (`player_damage`), checked against
//! `reference/pd_bot_port_sheet.md` §8.

use glam::{Mat4, Vec3};
use pd_core::ids::*;
use pd_core::math::baddtor;
use pd_core::model::NodeKind;

use super::Act;
use crate::world::World;

pub use pd_core::ids::HITPART_GENERAL;

/// `CHOKETYPE_*` (`chr_grunt`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choke {
    None,
    Gurgle,
    Cough,
}

/// Where each voice's grunt list is up to (`chr_grunt`'s statics).
#[derive(Clone, Copy, Debug, Default)]
pub struct GruntNext {
    pub maian: usize,
    pub shock: usize,
    pub male: usize,
    pub female: usize,
}

/// Who and what did the damage: `aprop` (a chr index) and the `gset`.
#[derive(Clone, Copy, Debug)]
pub struct DamageFrom {
    pub attacker: Option<usize>,
    pub weaponnum: u8,
    pub weaponfunc: usize,
}

impl DamageFrom {
    pub fn new(attacker: Option<usize>, weaponnum: u8, weaponfunc: usize) -> DamageFrom {
        DamageFrom { attacker, weaponnum, weaponfunc }
    }
}

/// `mp_handicap_to_value` (`mplayer.c`).
pub fn mp_handicap_to_value(handicap: u8) -> f32 {
    if handicap < 127 {
        return (handicap as f32 / 127.0) * (handicap as f32 / 127.0) * 0.9 + 0.1;
    }
    if handicap == 127 {
        return 1.0;
    }
    let tmp = (handicap as f32 - 128.0) / 127.0 + 1.0;
    tmp * tmp * 3.0 - 2.0
}

impl World {
    /// `chr_is_dead` (`chraction.c:9588`): dying or dead, or a dead player's.
    pub fn chr_is_dead(&self, i: usize) -> bool {
        let c = &self.chrs[i];
        c.is_dying_or_dead() || c.player.is_some_and(|p| self.players[p].isdead)
    }

    /// `chr_damage` (`chraction.c:4262`), normal multiplayer. `pos` of the hit
    /// is only the blood's (the caller emits it).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn chr_damage(&mut self, victim: usize, damage: f32, vector: Vec3, from: DamageFrom, hitpart: i32, damageshield: bool, explosion: bool) {
        let mut damage = damage;
        let mut hitpart = hitpart;
        let gset = self.res.gset.clone();
        let mut choketype = if hitpart == HITPART_HEAD { Choke::Gurgle } else { Choke::None };
        let mut grunt = true;
        if from.weaponnum == WEAPON_COMBATKNIFE {
            if from.weaponfunc == FUNC_2 {
                grunt = false;
            }
            if from.weaponfunc == FUNC_POISON {
                choketype = Choke::Cough;
            }
        } else if from.weaponnum == WEAPON_TRANQUILIZER && from.weaponfunc == FUNC_SECONDARY {
            choketype = Choke::Gurgle;
        }
        let func = gset.func(from.weaponnum, from.weaponfunc);
        let funcflags = func.map_or(0, |f| f.flags);
        let ismelee = func.is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_MELEE);
        let mut makedizzy = funcflags & FUNCFLAG_MAKEDIZZY != 0;
        let isshoot = !ismelee;
        let vplayer = self.chrs[victim].player;

        // Disarm only hurts an NPC in solo.
        if funcflags & FUNCFLAG_DISARM != 0 && from.weaponnum == WEAPON_UNARMED {
            damage = 0.0;
        }
        // Normal multiplayer: a player's damagescale (1).
        let aplayernum = from.attacker;

        // The shotgun, fired by a chr, by distance (`chraction.c:4517`).
        if let Some(a) = from.attacker.filter(|&a| self.chrs[a].player.is_none()) {
            if from.weaponnum == WEAPON_SHOTGUN {
                let sqdist = self.chrs[a].pos.distance_squared(self.chrs[victim].pos);
                if sqdist < 200.0 * 200.0 {
                    damage *= 4.0 + (self.rng.random() % 3) as f32;
                } else if sqdist < 400.0 * 400.0 {
                    damage *= 3.0 + (self.rng.random() % 2) as f32;
                } else if sqdist < 800.0 * 800.0 {
                    damage *= 2.0 + (self.rng.random() % 2) as f32;
                } else if sqdist < 1600.0 * 1600.0 {
                    damage *= 1.0 + (self.rng.random() % 2) as f32;
                }
            }
        }
        // The Farsight goes through shields, ×10.
        let mut _damageshield = damageshield;
        if from.weaponnum == WEAPON_FARSIGHT {
            _damageshield = true;
            damage *= 10.0;
        }
        // (Shields, hats and the shot-count stats: M8 / M7.)

        // A dying or dead chr: perhaps a head flinch, then done.
        if self.chr_is_dead(victim) {
            let c = &self.chrs[victim];
            if hitpart == HITPART_HEAD && c.actiontype == Act::Die && isshoot {
                let angle = c.chr_get_angle_to_pos(c.pos - vector);
                self.chr_flinch_head(victim, angle);
            }
            return;
        }
        let c = &self.chrs[victim];
        let angle = c.chr_get_angle_to_pos(c.pos - vector);
        // A knife in the back of an unalerted chr is lethal (a simulant is never alert).
        if from.weaponnum == WEAPON_COMBATKNIFE && from.weaponfunc == FUNC_PRIMARY && angle > baddtor(120.0) && angle < 4.188_123_703_002_9 {
            damage *= 1000.0;
        }
        let mut forceapplydamage = false;
        if funcflags & FUNCFLAG_BLUNTIMPACT != 0 {
            if angle < baddtor(60.0) || angle > baddtor(300.0) {
                damage *= 0.4;
            } else if angle < baddtor(120.0) || angle > 4.188_123_703_002_9 {
                damage *= 0.7;
            }
            // (The one-hit knockout is for solo NPCs.)
            forceapplydamage = true;
        }
        if hitpart == HITPART_GENERAL {
            // Halve the damage because it's doubled for the torso below.
            hitpart = HITPART_TORSO;
            damage *= 0.5;
        } else if hitpart == HITPART_GENERALHALF {
            hitpart = HITPART_TORSO;
            damage *= 0.25;
        }
        if hitpart == HITPART_HEAD {
            damage *= 4.0;
            if isshoot {
                self.chr_flinch_head(victim, angle);
                // headshotdamagescale is 1 in multiplayer.
                if from.weaponnum == WEAPON_COMBATKNIFE && from.weaponfunc != FUNC_POISON {
                    damage += damage;
                }
            }
        } else if hitpart == HITPART_TORSO {
            damage += damage;
        } else if hitpart == HITPART_GUN || hitpart == HITPART_HAT {
            damage = 0.0;
            makedizzy = false;
        }

        if let Some(pi) = vplayer {
            // A player shot (`chraction.c:4565`).
            let handicap = self.setup.players.get(pi).map_or(128, |p| p.handicap);
            damage /= mp_handicap_to_value(handicap);
            if !self.players[pi].isdead {
                if makedizzy {
                    let amount = gset_get_blur_amount(&gset, from.weaponnum, from.weaponfunc);
                    self.chrs[victim].blurdrugamount += amount;
                    self.chrs[victim].blurnumtimesdied = 0;
                }
                if damage > 0.0 {
                    let amount = damage * 0.125;
                    let bondhealth = self.players[pi].bondhealth;
                    self.players[pi].health.player_display_health(bondhealth);
                    self.players[pi].bondhealth -= amount;
                    let showdamage = true;
                    if self.players[pi].bondhealth <= 0.0 {
                        self.player_die_by_shooter(pi, aplayernum);
                        self.chrs[victim].blurnumtimesdied += 1;
                    }
                    if grunt {
                        self.chr_grunt(victim, choketype);
                    }
                    self.chr_flinch_body(victim);
                    let boostscale = if ismelee && from.weaponnum == WEAPON_REAPER { 0.1 } else { 0.75 };
                    self.players[pi].shotspeed.x += vector.x * boostscale;
                    self.players[pi].shotspeed.z += vector.z * boostscale;
                    if showdamage {
                        self.players[pi].health.player_display_damage();
                    }
                } else {
                    let boostscale = if ismelee && from.weaponnum == WEAPON_REAPER { 0.1 } else { 0.75 };
                    self.players[pi].shotspeed.x += vector.x * boostscale;
                    self.players[pi].shotspeed.z += vector.z * boostscale;
                }
            }
            return;
        }

        // A simulant (`chraction.c:4700`).
        if self.chrs[victim].damage < self.chrs[victim].maxdamage {
            if makedizzy {
                let amount = gset_get_blur_amount(&gset, from.weaponnum, from.weaponfunc);
                let c = &mut self.chrs[victim];
                c.blurdrugamount += amount;
                c.blurnumtimesdied = 0;
            }
            let boostscale = if ismelee && from.weaponnum == WEAPON_REAPER { 0.1 } else { 0.75 };
            if let Some(a) = self.chrs[victim].aibot.as_mut() {
                a.shotspeed.x += vector.x * boostscale;
                a.shotspeed.z += vector.z * boostscale;
            }
            let sp80 = if from.weaponnum == WEAPON_UNARMED { 2.0 } else { 0.0 };
            let _ = forceapplydamage;
            if damage > 0.0 {
                let c = &mut self.chrs[victim];
                c.damage += damage;
                if grunt {
                    self.chr_grunt(victim, choketype);
                }
                self.chr_flinch_body(victim);
                let c = &self.chrs[victim];
                if c.damage >= c.maxdamage {
                    self.chr_die(victim, aplayernum);
                }
                // The punch's push: prop2 (the victim) away from the attacker.
                if sp80 > 0.0 {
                    if let Some(a) = from.attacker {
                        let d = (self.chrs[victim].pos - self.chrs[a].pos).normalize_or_zero();
                        let c = &mut self.chrs[victim];
                        c.timeextra = sp80 * 15.0;
                        c.elapseextra = 0.0;
                        c.extraspeed = d * sp80;
                    }
                }
            }
        }
        let _ = explosion;
    }

    /// `chr_damage_by_impact` (`chraction.c:4138`): a shot or a thrown blade.
    pub(crate) fn chr_damage_by_impact(&mut self, victim: usize, damage: f32, vector: Vec3, from: DamageFrom, hitpart: i32) {
        self.chr_damage(victim, damage, vector, from, hitpart, true, false);
    }

    /// `chr_damage_by_general` (`chraction.c:4184`): a punch, a sentry's round.
    /// PD first asks `chr_calculate_shield_hit` for the part, whatever the
    /// shield (its test is `>= 0`).
    pub(crate) fn chr_damage_by_general(&mut self, victim: usize, damage: f32, vector: Vec3, from: DamageFrom, hitpart: i32) {
        let hitpart = self.chr_calculate_shield_hit(victim).unwrap_or(hitpart);
        self.chr_damage(victim, damage, vector, from, hitpart, true, false);
    }

    /// `chr_damage_by_explosion` (`chraction.c:4204`).
    pub(crate) fn chr_damage_by_explosion(&mut self, victim: usize, damage: f32, vector: Vec3, attacker: Option<usize>) {
        self.chr_damage(victim, damage, vector, DamageFrom::new(attacker, 0, 0), HITPART_GENERAL, true, true);
    }

    /// `chr_calculate_shield_hit` (`chraction.c:9690`), multiplayer, for a
    /// damage "from the chr's own position": on any screen, the part box whose
    /// joint is nearest the root; otherwise the torso's.
    fn chr_calculate_shield_hit(&self, i: usize) -> Option<i32> {
        let c = &self.chrs[i];
        let def = &c.model.def;
        if c.onanyscreen || c.onanyscreenprev {
            let mut best: Option<(f32, i32)> = None;
            for (n, node) in def.nodes.iter().enumerate() {
                if let NodeKind::BBox { hitpart, .. } = node.kind {
                    let m: Mat4 = def.find_node_mtx_index(n, 0).map_or(Mat4::IDENTITY, |k| c.model.matrices[k]);
                    let d = m.w_axis.truncate().distance_squared(c.pos);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, hitpart));
                    }
                }
            }
            if let Some((_, h)) = best {
                return Some(h);
            }
        }
        def.nodes.iter().find_map(|n| match n.kind {
            NodeKind::BBox { hitpart, .. } if hitpart == HITPART_TORSO => Some(hitpart),
            _ => None,
        })
    }

    /// `chr_flinch_body` (`chr.c:1532`).
    pub(crate) fn chr_flinch_body(&mut self, i: usize) {
        let c = &self.chrs[i];
        if c.actiontype != Act::Dead && c.flinchcnt < 0 {
            let r = self.rng.random();
            let c = &mut self.chrs[i];
            c.flinchcnt = 1;
            c.headshotted = false;
            c.flinchtype = ((r << 13) >> 13 & 7) as u8;
        }
    }

    /// `chr_flinch_head` (`chr.c:1541`).
    pub(crate) fn chr_flinch_head(&mut self, i: usize, angle: f32) {
        let c = &mut self.chrs[i];
        if c.flinchcnt < 0 {
            c.flinchcnt = 1;
        } else if c.flinchcnt > 8 {
            c.flinchcnt = 4;
        }
        c.headshotted = true;
        let value = ((angle + baddtor(22.5)) * 8.0 / baddtor(360.0)) as i32;
        c.flinchtype = value.clamp(0, 7) as u8;
    }

    /// `chr_grunt` (`chraction.c:3806`): the hurt voice by head, at the chr (a
    /// player hears their own centred).
    fn chr_grunt(&mut self, i: usize, choketype: Choke) {
        const MAIAN: [u16; 3] = [0x05df, 0x05e0, 0x05e1];
        const SHOCK: [u16; 14] = [0x86, 0x88, 0x8a, 0x8c, 0x8e, 0x90, 0x92, 0x94, 0x96, 0x98, 0x9a, 0x9c, 0x9e, 0x87];
        const JO: [u16; 10] = [0x02aa, 0x02ab, 0x02ac, 0x02ad, 0x02ae, 0x02af, 0x02b0, 0x02b1, 0x02b2, 0x02b3];
        const FEMALE: [u16; 3] = [0x0d, 0x0e, 0x0f];
        let c = &self.chrs[i];
        if let Some(p) = c.player {
            if self.players[p].isdead {
                return;
            }
        }
        let headnum = c.headnum.unwrap_or(0);
        let male = self.res.bodies.rows.get(headnum).is_some_and(|r| r.ismale);
        let mut human = true;
        let mut soundnum: Option<u16> = if matches!(headnum as i32, HEAD_THEKING | HEAD_ELVIS | HEAD_MAIAN_S | HEAD_ELVIS_GOGS) {
            human = false;
            let s = MAIAN[(self.rng.random() % 3) as usize];
            self.grunt_next.maian = (self.grunt_next.maian + 1) % 3;
            Some(s)
        } else if headnum as i32 == HEAD_DDSHOCK {
            let s = SHOCK[self.grunt_next.shock];
            self.grunt_next.shock = (self.grunt_next.shock + 1) % SHOCK.len();
            Some(s)
        } else if male {
            let s = 0x86 + self.grunt_next.male as u16;
            self.grunt_next.male = (self.grunt_next.male + 1) % 25;
            Some(s)
        } else if matches!(headnum as i32, HEAD_DARK_COMBAT | HEAD_DARK_FROCK | HEAD_DARKAQUA | HEAD_DARK_SNOW) {
            Some(JO[(self.rng.random() % 10) as usize])
        } else {
            let s = FEMALE[self.grunt_next.female];
            self.grunt_next.female = (self.grunt_next.female + 1) % 3;
            Some(s)
        };
        if human {
            match choketype {
                Choke::Gurgle => {
                    const GURGLE: [u16; 3] = [0x034e, 0x05b1, 0x05b2];
                    if self.rng.random().is_multiple_of(8) {
                        soundnum = Some(GURGLE[(self.rng.random() % 3) as usize]);
                    }
                }
                Choke::Cough => {
                    if male {
                        soundnum = Some(if self.rng.random().is_multiple_of(2) { 0x05af } else { 0x05b0 });
                    } else {
                        let index = (self.rng.random() % 4) as usize;
                        soundnum = Some([0x05ab, 0x05ac, 0x05ad, 0x05ae][index]);
                    }
                }
                Choke::None => {}
            }
        }
        let Some(sound) = soundnum else { return };
        let c = &self.chrs[i];
        if c.player.is_some() {
            // snd_start on the player's chokehandle: centred, full volume.
            self.sound(sound, 1.0);
        } else {
            // ps_create(..., PSTYPE_CHRCHOKE), unless one is already playing.
            // SUBST: PD keeps one choke per chr / every grunt plays (no
            // per-prop sound channels yet, M12).
            let pos = c.pos;
            self.sound_at(sound, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
        }
    }

    /// `chr_emit_sparks` (`chr.c:3658`): blood and flesh where a round met a
    /// chr (a spark on its gun).
    pub(crate) fn chr_emit_sparks(&mut self, i: usize, hitpart: i32, pos: Vec3, dir: Vec3) {
        use pd_core::ids::{SPARKTYPE_BLOOD, SPARKTYPE_DEFAULT, SPARKTYPE_FLESH, SPARKTYPE_FLESH_LARGE};
        let _ = i;
        if hitpart == HITPART_GUN || hitpart == HITPART_HAT {
            self.fx.sparks.create(&mut self.rng, pos, dir, Vec3::ZERO, SPARKTYPE_DEFAULT);
            return;
        }
        if self.rng.random() & 4 == 0 {
            self.fx.sparks.create(&mut self.rng, dir * 42.0 + pos, dir, Vec3::ZERO, SPARKTYPE_FLESH_LARGE);
        }
        self.fx.sparks.create(&mut self.rng, pos, dir, Vec3::ZERO, SPARKTYPE_BLOOD);
        self.fx.sparks.create(&mut self.rng, pos, dir, Vec3::ZERO, SPARKTYPE_FLESH);
    }

    /// `chr_die` (`chraction.c:5102`), a simulant's: `ACT_DIE`, the kill scored
    /// (`mpstats_record_death`), uncloaked. `// M8:` `botinv_drop_all`.
    pub(crate) fn chr_die(&mut self, victim: usize, attacker: Option<usize>) {
        if self.chrs[victim].actiontype == Act::Die {
            return;
        }
        self.chrs[victim].chr_stop_firing();
        if self.chrs[victim].cloaked {
            self.chrs[victim].cloaked = false;
            let pos = self.chrs[victim].pos;
            self.sound_at(0x005c, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
        }
        self.chrs[victim].actiontype = Act::Die;
        self.chrs[victim].blurnumtimesdied += 1;
        self.mpstats_record_death(attacker, victim);
    }

    /// `mpstats_record_death` (`mpstats.c`), the counts M6 keeps: the killer's
    /// kill (a suicide when it's the victim's own or no one's) and the death.
    /// `// M7:` the full stats and the kill feed.
    pub(crate) fn mpstats_record_death(&mut self, attacker: Option<usize>, victim: usize) {
        self.chrs[victim].deaths += 1;
        match attacker {
            Some(a) if a != victim => self.chrs[a].kills += 1,
            _ => self.chrs[victim].suicides += 1,
        }
        self.push_event(pd_core::events::Event::Kill { killer: attacker.map(|a| a as u8), victim: victim as u8 });
    }

    /// `chr_tick_die` (`chraction.c:8295`): when the death animation reaches its
    /// end, `chr_begin_dead` (a simulant's fade starts at once). No thuds: a
    /// simulant's `thudframe`s are -1 (`chr_die`).
    pub(crate) fn chr_tick_die(&mut self, i: usize) {
        let c = &mut self.chrs[i];
        if c.anim.cur_frame() >= c.anim_end_frame(&self.res.bank) {
            c.chr_stop_firing();
            c.actiontype = Act::Dead;
            c.fadetimer60 = if c.aibot.is_some() { 0 } else { -1 };
        }
    }

    /// `chr_tick_dead` (`chraction.c:8229`): a 90-tick fade, then `bot_spawn`.
    pub(crate) fn chr_tick_dead(&mut self, i: usize) {
        let lv60 = self.lv.lvupdate60;
        let c = &mut self.chrs[i];
        if c.fadetimer60 >= 0 {
            c.fadetimer60 += lv60;
            if c.fadetimer60 >= 90 {
                c.fadealpha = 0.0;
                if c.aibot.is_some() {
                    self.bot_spawn(i, true);
                }
            } else {
                c.fadealpha = (90 - c.fadetimer60) as f32 * 255.0 / 90.0;
            }
        }
    }
}

/// `gset_get_blur_amount` (`chraction.c:3696`) in multiplayer: how much a
/// dizzying hit (`FUNCFLAG_MAKEDIZZY`: a punch, the tranquilizer, the N-Bomb)
/// adds to `blurdrugamount`.
fn gset_get_blur_amount(_gset: &crate::gun::Gset, weaponnum: u8, _func: usize) -> i32 {
    match weaponnum {
        WEAPON_TRANQUILIZER => 2000,
        WEAPON_BOLT => 5000,
        WEAPON_NBOMB => 100,
        _ => 1000,
    }
}
