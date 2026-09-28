//! A simulant's weapons: its inventory and ammo (`botinv.c`, `botact.c:22-210`),
//! how it rates the set's weapons and picks what to hold (`botinv_score_weapon`,
//! `botinv_tick`, `botinv_switch_to_weapon`), what it goes and gets
//! (`bot_find_pickup`, `bot.c:1865`), collecting what it walks over
//! (`bot_check_pickups`, `bot_test_prop_for_pickup`, `bot_pickup_prop`,
//! `bot.c:319-760`), and dropping it all when it dies (`botinv_drop`).
//!
//! Unlike a player's, a simulant's inventory holds a weapon once: a second
//! from another pad turns the item into a pair (`botinv_give_dual_weapon`).

use pd_core::ids::*;
use pd_core::mp::{mp_get_weapon_slot_by_weapon_num, mp_has_shield, mp_weapon};

use super::Aibot;
use crate::gun::gset::{AmmoDef, BotWeaponPref};
use crate::gun::{Bgun, Gset};
use crate::props::pickup::{botact_get_weapon_by_ammo_type, weapon_get_pickup_ammo_qty, weapon_pickup_sound, SFXNUM_00EA_PICKUP_AMMO, SFXNUM_01CD_PICKUP_SHIELD};
use crate::propsnd::DEFAULT_DISTS;
use crate::world::World;

/// `botinv_init(chr, 10)` (`botmgr.c:274`).
pub const MAX_BOTINV_ITEMS: usize = 10;

/// `bot_find_pickup`'s criteria (`bot.c:37`).
pub const PICKUPCRITERIA_DEFAULT: i32 = 0;
pub const PICKUPCRITERIA_CRITICAL: i32 = 1;
pub const PICKUPCRITERIA_ANY: i32 = 2;

/// `NUM_MPWEAPONSLOTS`.
const NUM_MPWEAPONSLOTS: usize = 6;

/// A simulant's `struct invitem`: `INVITEMTYPE_WEAP` or `_DUAL`, the weapon,
/// and the pad it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BotInvItem {
    pub ty: i32,
    pub weapon1: u8,
    pub pickuppad: i32,
}

/// `g_BotWeaponConfigs[weaponnum]` (`botinv.c:21`); the rows the export has no
/// weapon for are all zero with a one-second reload.
pub fn bot_weapon_config(gset: &Gset, weaponnum: u8) -> BotWeaponPref {
    gset.weapon(weaponnum).and_then(|w| w.bot).unwrap_or(BotWeaponPref { reloaddelay: 1, ..Default::default() })
}

/// `gset_get_ammodef` (`gset.c:116`): the function's ammo.
pub fn gset_get_ammodef(gset: &Gset, weaponnum: u8, func: usize) -> Option<&AmmoDef> {
    let w = gset.weapon(weaponnum)?;
    let f = w.functions.get(func)?.as_ref()?;
    if f.ammoindex < 0 {
        return None;
    }
    w.ammos.get(f.ammoindex as usize)?.as_ref()
}

/// `botact_get_ammo_type_by_function` (`botact.c:22`).
pub fn botact_get_ammo_type_by_function(gset: &Gset, weaponnum: u8, func: usize) -> i32 {
    if (WEAPON_FALCON2..=WEAPON_SUICIDEPILL).contains(&weaponnum) {
        if let Some(a) = gset_get_ammodef(gset, weaponnum, func) {
            return a.ammotype;
        }
    }
    0
}

impl Aibot {
    /// `botact_get_ammo_quantity_by_weapon` (`botact.c:79`).
    pub fn botact_get_ammo_quantity_by_weapon(&self, gset: &Gset, weaponnum: u8, func: usize, include_equipped: bool) -> i32 {
        let ammotype = botact_get_ammo_type_by_function(gset, weaponnum, func);
        let mut qty = if self.flags & BOTFLAG_UNLIMITEDAMMO != 0 { Bgun::bgun_get_capacity_by_ammotype(ammotype) } else { self.ammoheld[ammotype.clamp(0, 32) as usize] };
        if include_equipped && botact_get_ammo_type_by_function(gset, self.weaponnum, self.gunfunc) == ammotype {
            qty += self.loadedammo[0] + self.loadedammo[1];
        }
        qty
    }

    /// `botact_get_ammo_quantity_by_type` (`botact.c:107`).
    pub fn botact_get_ammo_quantity_by_type(&self, gset: &Gset, ammotype: i32, include_equipped: bool) -> i32 {
        let mut qty = if self.flags & BOTFLAG_UNLIMITEDAMMO != 0 { Bgun::bgun_get_capacity_by_ammotype(ammotype) } else { self.ammoheld[ammotype.clamp(0, 32) as usize] };
        if include_equipped && botact_get_ammo_type_by_function(gset, self.weaponnum, self.gunfunc) == ammotype {
            qty += self.loadedammo[0] + self.loadedammo[1];
        }
        qty
    }

    /// `botact_try_remove_ammo_from_reserve` (`botact.c:134`): what it could take.
    pub fn botact_try_remove_ammo_from_reserve(&mut self, gset: &Gset, weaponnum: u8, func: usize, tryqty: i32) -> i32 {
        let t = botact_get_ammo_type_by_function(gset, weaponnum, func).clamp(0, 32) as usize;
        if self.ammoheld[t] <= 0 || tryqty <= 0 {
            return 0;
        }
        if self.flags & BOTFLAG_UNLIMITEDAMMO != 0 {
            return tryqty;
        }
        self.ammoheld[t] -= tryqty;
        if self.ammoheld[t] < 0 {
            let removed = tryqty + self.ammoheld[t];
            self.ammoheld[t] = 0;
            removed
        } else {
            tryqty
        }
    }

    /// `botact_give_ammo_by_weapon` (`botact.c:165`).
    pub fn botact_give_ammo_by_weapon(&mut self, gset: &Gset, weaponnum: u8, func: usize, qty: i32) {
        let t = botact_get_ammo_type_by_function(gset, weaponnum, func);
        if self.flags & BOTFLAG_UNLIMITEDAMMO == 0 && qty > 0 {
            let i = t.clamp(0, 32) as usize;
            self.ammoheld[i] = (self.ammoheld[i] + qty).min(Bgun::bgun_get_capacity_by_ammotype(t));
        }
    }

    /// `botact_give_ammo_by_type` (`botact.c:186`).
    pub fn botact_give_ammo_by_type(&mut self, ammotype: i32, qty: i32) {
        if self.flags & BOTFLAG_UNLIMITEDAMMO != 0 || qty <= 0 {
            return;
        }
        let i = ammotype.clamp(0, 32) as usize;
        self.ammoheld[i] = (self.ammoheld[i] + qty).min(Bgun::bgun_get_capacity_by_ammotype(ammotype));
    }

    /// `botinv_clear` (`botinv.c:140`).
    pub fn botinv_clear(&mut self) {
        self.items = [None; MAX_BOTINV_ITEMS];
    }

    /// `botinv_get_item` (`botinv.c:182`): the slot holding `weaponnum`.
    pub fn botinv_get_item(&self, weaponnum: u8) -> Option<usize> {
        self.items.iter().position(|i| i.is_some_and(|i| (i.ty == INVITEMTYPE_WEAP || i.ty == INVITEMTYPE_DUAL) && i.weapon1 == weaponnum))
    }

    /// `botinv_get_item_type` (`botinv.c:239`): 0 for none.
    pub fn botinv_get_item_type(&self, weaponnum: u8) -> i32 {
        self.botinv_get_item(weaponnum).and_then(|s| self.items[s]).map_or(0, |i| i.ty)
    }

    /// `botinv_remove_item` (`botinv.c:210`).
    pub fn botinv_remove_item(&mut self, weaponnum: u8) {
        if let Some(s) = self.botinv_get_item(weaponnum) {
            self.items[s] = None;
        }
    }

    /// `botinv_give_single_weapon` (`botinv.c:261`): true if it was new.
    pub fn botinv_give_single_weapon(&mut self, weaponnum: u8) -> bool {
        if self.botinv_get_item_type(weaponnum) != 0 {
            return false;
        }
        if let Some(s) = self.items.iter().position(|i| i.is_none()) {
            self.items[s] = Some(BotInvItem { ty: INVITEMTYPE_WEAP, weapon1: weaponnum, pickuppad: -1 });
        }
        true
    }

    /// `botinv_give_dual_weapon` (`botinv.c:292`): the single becomes a pair.
    pub fn botinv_give_dual_weapon(&mut self, weaponnum: u8) {
        if let Some(s) = self.botinv_get_item(weaponnum) {
            if let Some(i) = self.items[s].as_mut() {
                i.ty = INVITEMTYPE_DUAL;
            }
        }
    }

    /// `botinv_get_weapon_pad` (`botinv.c:308`).
    pub fn botinv_get_weapon_pad(&self, weaponnum: u8) -> i32 {
        match self.botinv_get_item(weaponnum).and_then(|s| self.items[s]) {
            Some(i) if i.ty == INVITEMTYPE_WEAP => i.pickuppad,
            _ => -1,
        }
    }
}

impl World {
    /// `botinv_give_prop` (`botinv.c:324`): a weapon (noting its pad), or a
    /// crate's throwables. No ammo.
    fn botinv_give_prop(&mut self, i: usize, id: u32) -> bool {
        let Some(o) = self.props.get(id) else { return false };
        let (ty, weaponnum, pad, slots) = (o.ty, o.weaponnum, o.pad, o.ammoslots);
        let a = self.ab_mut(i);
        if ty == OBJTYPE_WEAPON {
            let result = a.botinv_give_single_weapon(weaponnum);
            if result {
                if let Some(s) = a.botinv_get_item(weaponnum) {
                    if let Some(item) = a.items[s].as_mut() {
                        item.pickuppad = pad;
                    }
                }
            }
            return result;
        }
        if ty == OBJTYPE_MULTIAMMOCRATE {
            for (k, &q) in slots.iter().enumerate() {
                if q > 0 {
                    let w = botact_get_weapon_by_ammo_type(k as i32 + 1);
                    if w > 0 {
                        a.botinv_give_single_weapon(w);
                    }
                }
            }
        }
        false
    }

    /// `bot_get_weapon_num` (`bot.c:804`): what chr `c` holds.
    pub(crate) fn bot_get_weapon_num(&self, c: usize) -> u8 {
        match (&self.chrs[c].aibot, self.chrs[c].player) {
            (Some(a), _) => a.weaponnum,
            (None, Some(p)) => self.players[p].gun.hands[HAND_RIGHT].weaponnum,
            _ => WEAPON_NONE,
        }
    }

    /// `bot_get_targets_weapon_num` (`bot.c:813`).
    fn bot_get_targets_weapon_num(&self, i: usize) -> u8 {
        self.chrs[i].target.map_or(WEAPON_NONE, |t| self.bot_get_weapon_num(t))
    }

    /// `botinv_score_all_weapons` (`botinv.c:373`): the set's six slots, each by
    /// its better function, sorted by score1 descending (a selection sort).
    fn botinv_score_all_weapons(&self, i: usize) -> ([u8; NUM_MPWEAPONSLOTS], [i32; NUM_MPWEAPONSLOTS], [i32; NUM_MPWEAPONSLOTS]) {
        let mut weaponnums = [0u8; NUM_MPWEAPONSLOTS];
        let mut scores1 = [0i32; NUM_MPWEAPONSLOTS];
        let mut scores2 = [0i32; NUM_MPWEAPONSLOTS];
        for s in 0..NUM_MPWEAPONSLOTS {
            let w = mp_weapon(&self.setup.weapons, s).weaponnum as u8;
            weaponnums[s] = w;
            let (p1, p2) = self.botinv_score_weapon(i, w, FUNC_PRIMARY, -1, false, false, true);
            let (s1, s2) = self.botinv_score_weapon(i, w, FUNC_SECONDARY, -1, false, false, true);
            scores1[s] = if p1 >= s1 { p1 } else { s1 };
            scores2[s] = if p2 >= s2 { p2 } else { s2 };
        }
        for a in 0..NUM_MPWEAPONSLOTS {
            let mut swap = a;
            for b in a + 1..NUM_MPWEAPONSLOTS {
                if scores1[b] > scores1[swap] {
                    swap = b;
                }
            }
            if swap != a {
                scores1.swap(swap, a);
                scores2.swap(swap, a);
                weaponnums.swap(swap, a);
            }
        }
        (weaponnums, scores1, scores2)
    }

    /// `botinv_score_weapon` (`botinv.c:465`): how much simulant `i` wants the
    /// weapon's function (`ifammo` -1: whatever its ammo goal), with PD's
    /// per-weapon adjustments, against its target if `comparewithtarget`, and
    /// what it has learnt (`learn`: kills per minute held against the match's
    /// rate, suicides, the random extra).
    pub(crate) fn botinv_score_weapon(&self, i: usize, weaponnum: u8, funcnum: usize, ifammo: i32, dual: bool, comparewithtarget: bool, learn: bool) -> (i32, i32) {
        let gset = &self.res.gset;
        let cfg = bot_weapon_config(gset, weaponnum);
        let a = self.ab(i);
        let (mut score1, mut score2) = (0, 0);
        if ifammo < 0 || (funcnum == FUNC_PRIMARY && ifammo == cfg.haspriammogoal) || (funcnum != FUNC_PRIMARY && ifammo == cfg.hassecammogoal) {
            if dual {
                score1 = cfg.dualscore1;
                score2 = cfg.dualscore2;
            } else {
                score1 = cfg.score1;
                score2 = cfg.score2;
            }
            let extra = match a.config.difficulty {
                BOTDIFF_MEAT => 100,
                BOTDIFF_EASY => 50,
                _ => 0,
            };
            if a.config.bottype == BOTTYPE_ROCKET {
                match weaponnum {
                    WEAPON_ROCKETLAUNCHER => score1 = extra + 300,
                    WEAPON_SLAYER => score1 = extra + 299,
                    WEAPON_DEVASTATOR => score1 = extra + 280,
                    WEAPON_SUPERDRAGON if funcnum != FUNC_PRIMARY => score1 = extra + 279,
                    WEAPON_PHOENIX if funcnum != FUNC_PRIMARY => score1 = extra + 260,
                    WEAPON_GRENADE => score1 = extra + 240,
                    _ => {}
                }
            } else if a.config.bottype == BOTTYPE_SHIELD && weaponnum == WEAPON_MPSHIELD {
                score1 = extra + 300;
            }
        }
        let sec = funcnum != FUNC_PRIMARY;
        let r1 = a.random1;
        let target = self.chrs[i].target;
        let targetblur = target.map_or(0, |t| self.chrs[t].blurdrugamount);
        match weaponnum {
            WEAPON_UNARMED => {
                if comparewithtarget && sec {
                    if target.is_some() && self.bot_get_targets_weapon_num(i) > WEAPON_UNARMED && a.config.difficulty > BOTDIFF_MEAT {
                        score1 = 26;
                        score2 = 26;
                    } else {
                        score1 = 0;
                        score2 = 0;
                    }
                }
            }
            WEAPON_FALCON2 if sec => (score1, score2) = (15, 15),
            WEAPON_FALCON2_SILENCER if sec => (score1, score2) = (14, 14),
            WEAPON_FALCON2_SCOPE if sec => (score1, score2) = (16, 16),
            WEAPON_MAGSEC4 if !sec => score1 = if dual { 91 } else { 63 },
            WEAPON_PHOENIX => {
                // PD compares the personality with BOTDIFF_HARD (`type != 3`,
                // BOTTYPE_ROCKET's number), kept.
                if a.config.bottype != BOTDIFF_HARD {
                    if sec {
                        if self.setup.options & MPOPTION_ONEHITKILLS != 0 && a.config.difficulty >= BOTDIFF_NORMAL {
                            score1 = 110;
                            score2 = 150;
                        }
                    } else {
                        score1 = if dual { 90 } else { 62 };
                    }
                }
            }
            WEAPON_DY357MAGNUM if sec => (score1, score2) = (17, 17),
            WEAPON_DY357LX if sec => (score1, score2) = (18, 18),
            WEAPON_CYCLONE | WEAPON_CALLISTO | WEAPON_SHOTGUN | WEAPON_ROCKETLAUNCHER => {
                if funcnum as u32 == r1 % 2 {
                    score1 -= 1;
                }
            }
            WEAPON_RCP120 => {
                if !a.cloakdeviceenabled && a.botact_get_ammo_quantity_by_weapon(gset, WEAPON_RCP120, FUNC_PRIMARY, true) > 500 && a.config.difficulty > BOTDIFF_MEAT {
                    score1 += (r1 % 10) as i32;
                    score2 += (r1 % 10) as i32;
                }
            }
            WEAPON_LAPTOPGUN | WEAPON_DRAGON | WEAPON_DEVASTATOR | WEAPON_COMBATKNIFE if sec => (score1, score2) = (0, 0),
            WEAPON_SUPERDRAGON => {
                if a.config.bottype != BOTDIFF_HARD && (r1 % 2) as usize == funcnum {
                    score1 -= 15;
                }
            }
            WEAPON_REAPER if sec => (score1, score2) = (19, 80),
            // SUBST: PD's simulants fly the Slayer's rocket along the waypoints
            // to an unseen target (`botact_create_slayer_rocket`, `botact.c:494`,
            // and the SK rocket's steering in `projectile_tick`) / the
            // fly-by-wire scores 0, so a simulant fires the Slayer's rockets
            // straight; the steering is left for later (Backlog).
            WEAPON_SLAYER if sec => (score1, score2) = (0, 0),
            WEAPON_SLAYER => {
                if sec {
                    let unseen = target.is_some_and(|t| !a.chrsinsight[t]) && r1.is_multiple_of(2);
                    if a.config.bottype == BOTDIFF_HARD {
                        if a.config.difficulty > BOTDIFF_MEAT {
                            if comparewithtarget {
                                if unseen {
                                    score1 += 10;
                                } else {
                                    score1 -= 10;
                                }
                            }
                        } else {
                            score1 = 0;
                            score2 = 0;
                        }
                    } else if a.config.difficulty >= BOTDIFF_NORMAL {
                        if comparewithtarget {
                            if unseen {
                                score1 = 178;
                                score2 = 188;
                            } else {
                                score1 -= 15;
                                score2 -= 15;
                            }
                        }
                    } else {
                        score1 = 0;
                        score2 = 0;
                    }
                }
            }
            WEAPON_CROSSBOW => {
                if sec {
                    if a.config.difficulty > BOTDIFF_MEAT {
                        score1 = 158;
                        score2 = 176;
                    } else {
                        score1 = 0;
                        score2 = 0;
                    }
                } else if comparewithtarget && target.is_some() && targetblur > 3500 {
                    score1 = 0;
                    score2 = 0;
                } else {
                    score1 = 49;
                    score2 = 188;
                }
            }
            WEAPON_TRANQUILIZER => {
                if comparewithtarget {
                    if sec {
                        if a.config.difficulty == BOTDIFF_MEAT {
                            (score1, score2) = (0, 0);
                        } else if (targetblur > 3500 && r1.is_multiple_of(2)) || r1.is_multiple_of(10) {
                            score1 = (r1 % 140) as i32 + 48;
                            score2 = 188;
                        } else {
                            (score1, score2) = (0, 0);
                        }
                    } else if targetblur >= 5000 {
                        score2 = 48;
                        if !r1.is_multiple_of(2) {
                            (score1, score2) = (0, 0);
                        }
                    } else if targetblur > 3500 {
                        let mut value = ((-targetblur * 16 + 80000) / 1500) as u32;
                        if value > 15 {
                            value = 15;
                        }
                        value = value * value;
                        value = value * value;
                        value = value * value;
                        if value < r1 {
                            score2 = 48;
                            if !r1.is_multiple_of(2) {
                                (score1, score2) = (0, 0);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if learn {
            let mut killrate = 1.0f32;
            if self.lv.lvframe60 > 0 {
                killrate = self.mp.totalkills as f32 * 3600.0 / (self.lv.lvframe60 * self.chrs.len() as i32) as f32;
                if killrate < 1.0 {
                    killrate = 1.0;
                }
            }
            if let Some(slot) = mp_get_weapon_slot_by_weapon_num(&self.setup.weapons, weaponnum as i32) {
                let f = funcnum.min(1);
                let mut extra = 0;
                let float2 = (a.equipdurations60[slot][f] as f32 * (1.0 / 3600.0)).ceil();
                if float2 > 0.0 {
                    let mut float1 = a.killsbygunfunc[slot][f];
                    if a.config.difficulty >= BOTDIFF_NORMAL {
                        float1 -= 3.0 * a.suicidesbygunfunc[slot][f];
                    }
                    float1 /= float2;
                    extra = (float1 * 10.0 / killrate) as i32;
                }
                if extra > 30 {
                    extra = 30;
                }
                extra += a.equipextrascores[slot];
                score1 = (score1 + extra).max(0);
                score2 = (score2 + extra).max(0);
            }
        }
        (score1, score2)
    }

    /// `botinv_allows_weapon` (`botinv.c:845`): a FistSim only uses close-range
    /// functions.
    fn botinv_allows_weapon(&self, i: usize, weaponnum: u8, funcnum: usize) -> bool {
        if self.ab(i).config.bottype != BOTTYPE_FIST {
            return true;
        }
        let cfg = bot_weapon_config(&self.res.gset, weaponnum);
        let d = if funcnum != FUNC_PRIMARY { cfg.secdistconfig } else { cfg.pridistconfig };
        d == super::botcmd::BOTDISTCFG_CLOSE
    }

    /// `botinv_tick` (`botinv.c:869`): the time each weapon has been held, the
    /// suicide memory cooling, the random extras and `random1` renewed, then
    /// (unless mid-burst) the best-scoring held weapon and function with ammo,
    /// switched to.
    pub(crate) fn botinv_tick(&mut self, i: usize) {
        let gset = self.res.gset.clone();
        let lv60 = self.lv.lvupdate60;
        let weapons = self.setup.weapons;
        {
            let (w, f) = (self.ab(i).weaponnum, self.ab(i).gunfunc.min(1));
            if let Some(slot) = mp_get_weapon_slot_by_weapon_num(&weapons, w as i32) {
                self.ab_mut(i).equipdurations60[slot][f] += lv60;
            }
        }
        self.ab_mut(i).dampensuicidesttl60 -= lv60;
        if self.ab(i).dampensuicidesttl60 < 0 {
            let r = self.rng.random();
            let a = self.ab_mut(i);
            a.dampensuicidesttl60 = 3600 + (r % 60) as i32;
            for s in a.suicidesbygunfunc.iter_mut() {
                s[0] *= 0.9;
                s[1] *= 0.9;
            }
        }
        self.ab_mut(i).equipextrascorestimer60 -= lv60;
        if self.ab(i).equipextrascorestimer60 < 0 {
            let r = self.rng.random();
            self.ab_mut(i).equipextrascorestimer60 = 600 + (r % 3000) as i32;
            for s in 0..NUM_MPWEAPONSLOTS {
                let r = self.rng.random();
                let v = match self.ab(i).config.difficulty {
                    BOTDIFF_MEAT => (r % 200) as i32 - 100,
                    BOTDIFF_EASY => (r % 100) as i32 - 50,
                    _ => (r % 30) as i32 - 15,
                };
                self.ab_mut(i).equipextrascores[s] = v;
            }
        }
        self.ab_mut(i).random1ttl60 -= lv60;
        if self.ab(i).random1ttl60 < 0 {
            let r1 = self.rng.random();
            let r2 = self.rng.random();
            let a = self.ab_mut(i);
            a.random1ttl60 = 120 + (r1 % 600) as i32;
            a.random1 = r2;
        }
        let a = self.ab(i);
        if a.cyclonedischarging != [false; 2] || a.burstsdone[0] > 0 || a.burstsdone[1] > 0 || a.reaperspeed[0] > 0 || a.reaperspeed[1] > 0 || a.skrocket.is_some() {
            return;
        }
        let mut newweaponnum = WEAPON_UNARMED;
        let mut newfuncnum = FUNC_PRIMARY;
        let mut keepcurrentweapon = false;
        // (MA_AIBOTDOWNLOAD: Hacker Central, M10.)
        if a.config.bottype == BOTTYPE_PEACE {
            newfuncnum = FUNC_SECONDARY;
            keepcurrentweapon = true;
        }
        if !keepcurrentweapon {
            let mut bestscore = 0;
            let items = a.items;
            for k in -1..MAX_BOTINV_ITEMS as i32 {
                let (weaponnum, dual) = if k < 0 {
                    (Some(WEAPON_UNARMED), false)
                } else {
                    match items[k as usize] {
                        Some(it) if it.ty == INVITEMTYPE_WEAP || it.ty == INVITEMTYPE_DUAL => (Some(it.weapon1), it.ty == INVITEMTYPE_DUAL),
                        _ => (None, false),
                    }
                };
                let Some(weaponnum) = weaponnum else { continue };
                let cfg = bot_weapon_config(&gset, weaponnum);
                for j in [FUNC_SECONDARY, FUNC_PRIMARY] {
                    let canuse = if j != FUNC_PRIMARY { cfg.hassecammogoal } else { cfg.haspriammogoal };
                    if canuse != 0 && self.botinv_allows_weapon(i, weaponnum, j) {
                        let (score1, _) = self.botinv_score_weapon(i, weaponnum, j, 1, dual, true, true);
                        if score1 >= bestscore && (botact_get_ammo_type_by_function(&gset, weaponnum, j) == 0 || self.ab(i).botact_get_ammo_quantity_by_weapon(&gset, weaponnum, j, true) > 0) {
                            bestscore = score1;
                            newweaponnum = weaponnum;
                            newfuncnum = j;
                        }
                    }
                }
            }
        }
        // Knives thrown from 2 to 15 m.
        if newweaponnum == WEAPON_COMBATKNIFE && self.ab(i).botact_get_ammo_quantity_by_weapon(&gset, WEAPON_COMBATKNIFE, FUNC_SECONDARY, true) >= 2 {
            if let Some(t) = self.chrs[i].target {
                let d = self.ab(i).chrdistances[t];
                if d > 200.0 && d < 1500.0 {
                    newfuncnum = FUNC_SECONDARY;
                }
            }
        }
        if self.ab(i).config.bottype == BOTTYPE_ROCKET {
            if newweaponnum == WEAPON_PHOENIX && self.ab(i).botact_get_ammo_quantity_by_weapon(&gset, WEAPON_PHOENIX, FUNC_SECONDARY, true) > 0 {
                newfuncnum = FUNC_SECONDARY;
            } else if newweaponnum == WEAPON_SUPERDRAGON && self.ab(i).botact_get_ammo_quantity_by_weapon(&gset, WEAPON_SUPERDRAGON, FUNC_SECONDARY, true) > 0 {
                newfuncnum = FUNC_SECONDARY;
            }
        }
        self.botinv_switch_to_weapon(i, newweaponnum, newfuncnum);
    }

    /// `botinv_switch_to_weapon` (`botinv.c:1019`): a weapon it holds (else
    /// the fists); a new gun puts the old one away and comes out after a
    /// second (`changeguntimer60`, `bot_tick_unpaused`); the loaded rounds go
    /// back to the reserve.
    pub(crate) fn botinv_switch_to_weapon(&mut self, i: usize, weaponnum: u8, funcnum: usize) -> bool {
        let gset = self.res.gset.clone();
        let (mut weaponnum, mut funcnum) = (weaponnum, funcnum);
        if weaponnum == WEAPON_BRIEFCASE2 {
            return true;
        }
        let mut item = None;
        if weaponnum != WEAPON_UNARMED {
            item = self.ab(i).botinv_get_item(weaponnum);
            if item.is_none() {
                weaponnum = WEAPON_UNARMED;
                funcnum = FUNC_PRIMARY;
            }
        }
        let (cur, curfunc) = (self.ab(i).weaponnum, self.ab(i).gunfunc);
        let changinggun = weaponnum != cur;
        let changingfunc = funcnum != curfunc;
        if changinggun {
            let a = self.ab_mut(i);
            a.changeguntimer60 = 60;
            a.cyclonedischarging = [false; 2];
            a.burstsdone = [0; 2];
            a.reaperspeed = [0; 2];
            self.chrs[i].held = [None, None];
        }
        if changingfunc || changinggun {
            let a = self.ab_mut(i);
            for h in 0..2 {
                if a.loadedammo[h] > 0 {
                    let q = a.loadedammo[h];
                    a.botact_give_ammo_by_weapon(&gset, cur, curfunc, q);
                    a.loadedammo[h] = 0;
                }
            }
        }
        {
            let a = self.ab_mut(i);
            a.gunfunc = funcnum;
            a.weaponnum = weaponnum;
        }
        if changingfunc && !changinggun {
            for h in 0..2 {
                if self.chrs[i].held[h].is_some() {
                    self.botact_reload(i, h, false);
                }
            }
        }
        if !changinggun {
            let dual = item.and_then(|s| self.ab(i).items[s]).is_some_and(|it| it.ty == INVITEMTYPE_DUAL);
            if dual && self.chrs[i].held[HAND_LEFT].is_none() && self.chr_give_weapon(i, weaponnum, HAND_LEFT) {
                self.botact_reload(i, HAND_LEFT, false);
            }
        }
        let melee = gset.func(weaponnum, funcnum).is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_MELEE);
        self.ab_mut(i).ismeleeweapon = melee;
        true
    }

    /// `chr_give_weapon(chr, playermgr_get_model_of_weapon(w), w, flags)`
    /// (`propobj.c:17852`): the weapon's held model in `hand`. False if it has
    /// none (the fists).
    pub(crate) fn chr_give_weapon(&mut self, i: usize, weaponnum: u8, hand: usize) -> bool {
        let res = self.res.clone();
        let Some(stem) = res.gset.weapon(weaponnum).and_then(|w| w.tp_model.clone()) else { return false };
        let Ok(held) = crate::chr::Held::new(&res.models, weaponnum, &stem) else { return false };
        self.chrs[i].held[hand] = Some(held);
        true
    }

    /// The change-gun timer at the top of `bot_tick_unpaused` (`bot.c:2491`):
    /// when it runs out the new weapon is in hand (twice for a pair), loaded.
    pub(crate) fn bot_tick_changegun(&mut self, i: usize) {
        if self.ab(i).changeguntimer60 <= 0 {
            return;
        }
        self.ab_mut(i).changeguntimer60 -= self.lv.lvupdate60;
        if self.ab(i).changeguntimer60 > 0 {
            return;
        }
        let w = self.ab(i).weaponnum;
        let item = self.ab(i).botinv_get_item(w).and_then(|s| self.ab(i).items[s]);
        let hasmodel = self.res.gset.weapon(w).is_some_and(|d| d.tp_model.is_some());
        if let (Some(it), true) = (item, hasmodel) {
            if self.chr_give_weapon(i, w, HAND_RIGHT) {
                self.botact_reload(i, HAND_RIGHT, false);
            }
            if it.ty == INVITEMTYPE_DUAL && self.chr_give_weapon(i, w, HAND_LEFT) {
                self.botact_reload(i, HAND_LEFT, false);
            }
        } else {
            let a = self.ab_mut(i);
            a.weaponnum = WEAPON_UNARMED;
            a.gunfunc = FUNC_PRIMARY;
            a.ismeleeweapon = true;
        }
        let a = self.ab_mut(i);
        a.throwtimer60 = 0;
        a.punchtimer60 = [0; 2];
    }

    /// `botinv_drop(chr, weaponnum, dropall)` (`botinv.c:1122`): each weapon
    /// (all of them, or `weaponnum`) falls from the simulant; back to the fists.
    pub(crate) fn botinv_drop(&mut self, i: usize, weaponnum: u8, dropall: bool) {
        let gset = self.res.gset.clone();
        let items = self.ab(i).items;
        for it in items.iter().flatten() {
            if (it.ty == INVITEMTYPE_WEAP || it.ty == INVITEMTYPE_DUAL) && (dropall || weaponnum == it.weapon1) && !gset.has_flag(it.weapon1, WEAPONFLAG_UNDROPPABLE) && gset.weapon(it.weapon1).is_some_and(|d| d.tp_model.is_some()) {
                self.weapon_create_for_chr_drop(i, it.weapon1);
            }
        }
        if (dropall && weaponnum >= WEAPON_FALCON2) || (!dropall && weaponnum == self.ab(i).weaponnum) {
            self.botinv_switch_to_weapon(i, WEAPON_UNARMED, FUNC_PRIMARY);
        }
        if !dropall {
            self.ab_mut(i).botinv_remove_item(weaponnum);
        }
    }

    // ─── pickups ─────────────────────────────────────────────────────────────

    /// `bot_is_obj_collectable` (`bot.c:664`): crates, shields and weapons, not
    /// the explosives in play.
    fn bot_is_obj_collectable(o: &crate::props::Obj) -> bool {
        match o.ty {
            OBJTYPE_AMMOCRATE | OBJTYPE_MULTIAMMOCRATE | OBJTYPE_SHIELD => true,
            OBJTYPE_WEAPON => !(matches!(o.weaponnum, WEAPON_NBOMB | WEAPON_GRENADE | WEAPON_GRENADEROUND | WEAPON_PROXIMITYMINE | WEAPON_REMOTEMINE | WEAPON_TIMEDMINE | WEAPON_SKROCKET) || (o.weaponnum == WEAPON_DRAGON && o.gunfunc == FUNC_SECONDARY)),
            _ => false,
        }
    }

    /// `bot_check_pickups` (`bot.c:699`), from `bot_tick`: whatever the
    /// simulant is standing on.
    ///
    /// `// SUBST:` PD looks in the simulant's rooms and their neighbours
    /// (`room_get_props`) / every object, newest first; the 1 m reach decides.
    pub(crate) fn bot_check_pickups(&mut self, i: usize) {
        let ids: Vec<u32> = self
            .props
            .objs
            .iter()
            .rev()
            .filter(|o| o.timetoregen == 0 && o.flags & OBJFLAG_HELDROCKET == 0)
            .filter(|o| o.projectile.as_ref().is_none_or(|p| p.pickuptimer240 <= 0 || p.bouncecount != 0))
            .filter(|o| Self::bot_is_obj_collectable(o))
            .map(|o| o.id)
            .collect();
        for id in ids {
            if self.bot_test_prop_for_pickup(id, i) {
                self.obj_free_pickup(id);
            }
        }
    }

    /// `bot_test_prop_for_pickup` (`bot.c:471`): wanted (not at full ammo for a
    /// single-only weapon or a pair, a crate with ammo it lacks, a shield better
    /// than its own), within 1 m across and 2 m up or down, in sight; then
    /// `bot_pickup_prop`. True: it was taken.
    fn bot_test_prop_for_pickup(&mut self, id: u32, i: usize) -> bool {
        if self.chr_is_dead(i) {
            return false;
        }
        let gset = self.res.gset.clone();
        let Some(o) = self.props.get(id) else { return false };
        if o.timetoregen != 0 || o.is_gone() {
            return false;
        }
        if o.obj_defaults_to_bounceable_invincible_pickupable() {
            if o.flags & OBJFLAG_UNCOLLECTABLE != 0 {
                return false;
            }
        } else if o.flags & OBJFLAG_COLLECTABLE == 0 {
            return false;
        }
        if o.is_deleting() || o.flags & OBJFLAG_THROWNLAPTOP != 0 {
            return false;
        }
        if o.projectile.as_ref().is_some_and(|p| p.pickuptimer240 > 0 && p.bouncecount == 0) {
            return false;
        }
        let a = self.ab(i);
        let mut give_crate_prop = false;
        match o.ty {
            OBJTYPE_WEAPON => {
                let itemtype = a.botinv_get_item_type(o.weaponnum);
                let singleonly = gset.weapon(o.weaponnum).is_some_and(|w| w.flags & WEAPONFLAG_DUALWIELD == 0);
                if o.weaponnum != WEAPON_BRIEFCASE2 {
                    if (itemtype == INVITEMTYPE_DUAL || (itemtype == INVITEMTYPE_WEAP && singleonly))
                        && a.botact_get_ammo_quantity_by_weapon(&gset, o.weaponnum, o.gunfunc, false) >= Bgun::bgun_get_capacity_by_ammotype(botact_get_ammo_type_by_function(&gset, o.weaponnum, o.gunfunc))
                    {
                        return false;
                    }
                    if matches!(o.weaponnum, WEAPON_ROCKET | WEAPON_HOMINGROCKET) && o.projectile.is_some() {
                        return false;
                    }
                }
            }
            OBJTYPE_MULTIAMMOCRATE => {
                let mut ignore = true;
                for k in 0..19 {
                    let w = botact_get_weapon_by_ammo_type(k as i32 + 1);
                    if o.ammoslots[k] > 0 && a.botact_get_ammo_quantity_by_type(&gset, k as i32 + 1, false) < Bgun::bgun_get_capacity_by_ammotype(k as i32 + 1) {
                        ignore = false;
                        if w != 0 && a.botinv_get_item_type(w) == 0 {
                            give_crate_prop = true;
                        }
                        break;
                    }
                }
                if ignore {
                    return false;
                }
            }
            OBJTYPE_SHIELD => {
                // (Hold the Briefcase: M10.)
                if o.shieldamount <= self.chr_get_shield(i) * 0.125 {
                    return false;
                }
            }
            _ => {}
        }
        if give_crate_prop {
            // bot.c:584: a crate of throwables gives the weapon as it is judged.
            self.botinv_give_prop(i, id);
        }
        let Some(o) = self.props.get(id) else { return false };
        let c = &self.chrs[i];
        let d = o.pos - c.pos;
        // `aibot->cheap` (250 cm) is never set: every simulant is fully ticked.
        let sqrange = 100.0 * 100.0;
        let mut sp3c = d.x * d.x + d.z * d.z <= sqrange && d.y >= -200.0 && d.y <= 200.0;
        if sp3c && o.flags2 & OBJFLAG2_PICKUPWITHOUTLOS == 0 && !self.level.los_autoflags(c.pos, o.pos) {
            sp3c = false;
        }
        sp3c && self.bot_pickup_prop(id, i) != 0
    }

    /// `bot_pickup_prop` (`bot.c:319`): 1 a new weapon, 2 ammo (or a weapon it
    /// has), 3 a shield; 0 nothing.
    fn bot_pickup_prop(&mut self, id: u32, i: usize) -> i32 {
        let gset = self.res.gset.clone();
        let Some(o) = self.props.get_mut(id) else { return 0 };
        o.flags3 &= !OBJFLAG3_ISFETCHTARGET;
        let o = o.clone();
        match o.ty {
            OBJTYPE_MULTIAMMOCRATE => {
                let a = self.ab_mut(i);
                for (k, &q) in o.ammoslots.iter().enumerate() {
                    if q != 0 {
                        a.botact_give_ammo_by_type(k as i32 + 1, q);
                    }
                }
                self.sound_at(SFXNUM_00EA_PICKUP_AMMO, 1.0, o.pos, DEFAULT_DISTS);
                2
            }
            OBJTYPE_WEAPON => {
                let itemtype = self.ab(i).botinv_get_item_type(o.weaponnum);
                // (The briefcase and the data uplink: M10.)
                // prop_play_pickup_sound: the same list as a player's but the bolt.
                self.sound_at(weapon_pickup_sound(o.weaponnum), 1.0, o.pos, DEFAULT_DISTS);
                let ammotype = Bgun::bgun_get_ammo_type_for_weapon(&gset, o.weaponnum, FUNC_PRIMARY);
                let qty = weapon_get_pickup_ammo_qty(&o, ammotype);
                if qty != 0 {
                    self.ab_mut(i).botact_give_ammo_by_weapon(&gset, o.weaponnum, o.gunfunc, qty);
                }
                if itemtype != 0 {
                    let dualwield = gset.weapon(o.weaponnum).is_some_and(|w| w.flags & WEAPONFLAG_DUALWIELD != 0);
                    let originalpad = self.ab(i).botinv_get_weapon_pad(o.weaponnum);
                    if itemtype == INVITEMTYPE_WEAP && dualwield && originalpad != o.pad {
                        self.ab_mut(i).botinv_give_dual_weapon(o.weaponnum);
                        1
                    } else {
                        2
                    }
                } else {
                    self.botinv_give_prop(i, id);
                    1
                }
            }
            OBJTYPE_SHIELD => {
                self.sound_at(SFXNUM_01CD_PICKUP_SHIELD, 1.0, o.pos, DEFAULT_DISTS);
                self.chr_set_shield(i, o.shieldamount * 8.0);
                3
            }
            _ => 0,
        }
    }

    /// `bot_find_pickup` (`bot.c:1865`): what simulant `i` goes and gets, by
    /// `criteria` (`PICKUPCRITERIA_*`): a shield when hurt enough and short of
    /// one, ammo for the best weapon it holds below its goal, the best weapon it
    /// lacks, and (for `ANY`) ammo for anything. The props considered are the
    /// nearest of each weapon and ammo type, with a 1/16 chance of skipping one
    /// and a 1/16 chance of a further one winning.
    pub(crate) fn bot_find_pickup(&mut self, i: usize, criteria: i32) -> Option<u32> {
        let gset = self.res.gset.clone();
        let (weaponnums, scores1, scores2) = self.botinv_score_all_weapons(i);
        let mut weapproplist: [Option<u32>; NUM_MPWEAPONSLOTS] = [None; NUM_MPWEAPONSLOTS];
        let mut weapdistlist = [0f32; NUM_MPWEAPONSLOTS];
        let mut ammoproplist: [Option<u32>; 33] = [None; 33];
        let mut ammodistlist = [0f32; 33];
        let invitems: Vec<bool> = weaponnums.iter().map(|&w| self.ab(i).botinv_get_item(w).is_some()).collect();
        // (King of the Hill's barelydominatinghill: M10.)
        let barelydominatinghill = false;
        let cpos = self.chrs[i].pos;
        // The active props, newest first (`prop_activate` puts a prop at the head).
        let props: Vec<(u32, u8, u8, u32, [i32; 19], glam::Vec3)> = self
            .props
            .objs
            .iter()
            .rev()
            .filter(|o| o.timetoregen == 0 && !o.is_gone() && o.flags & OBJFLAG_HELDROCKET == 0 && o.is_pickup())
            .map(|o| (o.id, o.ty, o.weaponnum, o.flags3, o.ammoslots, o.pos))
            .collect();
        for (id, ty, weaponnum, flags3, slots, pos) in props {
            if flags3 & OBJFLAG3_ISFETCHTARGET != 0 {
                continue;
            }
            let sqdist = cpos.distance_squared(pos);
            if ty == OBJTYPE_WEAPON {
                for k in 0..NUM_MPWEAPONSLOTS {
                    if weaponnums[k] > WEAPON_UNARMED && weaponnums[k] == weaponnum {
                        if !self.rng.random().is_multiple_of(16) && (weapproplist[k].is_none() || sqdist < weapdistlist[k] || self.rng.random().is_multiple_of(16)) {
                            weapproplist[k] = Some(id);
                            weapdistlist[k] = sqdist;
                        }
                        break;
                    }
                }
                let ammotype = botact_get_ammo_type_by_function(&gset, weaponnum, FUNC_PRIMARY);
                if ammotype > 0 && !self.rng.random().is_multiple_of(16) {
                    let t = ammotype as usize;
                    if ammoproplist[t].is_none() || sqdist < ammodistlist[t] || self.rng.random().is_multiple_of(16) {
                        ammoproplist[t] = Some(id);
                        ammodistlist[t] = sqdist;
                    }
                }
            } else if ty == OBJTYPE_MULTIAMMOCRATE {
                for (k, &q) in slots.iter().enumerate() {
                    let ammotype = k as i32 + 1;
                    if q > 0 {
                        let w = botact_get_weapon_by_ammo_type(ammotype);
                        if w > 0 {
                            for j in 0..NUM_MPWEAPONSLOTS {
                                if weaponnums[j] > WEAPON_UNARMED && w == weaponnums[j] {
                                    if !self.rng.random().is_multiple_of(16) && (weapproplist[j].is_none() || sqdist < weapdistlist[j] || self.rng.random().is_multiple_of(16)) {
                                        weapproplist[j] = Some(id);
                                        weapdistlist[j] = sqdist;
                                    }
                                    break;
                                }
                            }
                        }
                        if !self.rng.random().is_multiple_of(16) {
                            let t = ammotype as usize;
                            if ammoproplist[t].is_none() || sqdist < ammodistlist[t] || self.rng.random().is_multiple_of(16) {
                                ammoproplist[t] = Some(id);
                                ammodistlist[t] = sqdist;
                            }
                        }
                    }
                }
            } else if ty == OBJTYPE_SHIELD {
                for k in 0..NUM_MPWEAPONSLOTS {
                    if weaponnums[k] == WEAPON_MPSHIELD {
                        if self.rng.random().is_multiple_of(16) {
                            break;
                        }
                        if weapproplist[k].is_none() || sqdist < weapdistlist[k] || self.rng.random().is_multiple_of(16) {
                            weapproplist[k] = Some(id);
                            weapdistlist[k] = sqdist;
                        }
                        break;
                    }
                }
            }
        }
        // The best score among the weapons it may use that need ammo.
        let mut bestscore1 = 0;
        for k in 0..NUM_MPWEAPONSLOTS {
            let cfg = bot_weapon_config(&gset, weaponnums[k]);
            if (self.botinv_allows_weapon(i, weaponnums[k], FUNC_PRIMARY) || self.botinv_allows_weapon(i, weaponnums[k], FUNC_SECONDARY)) && (cfg.haspriammogoal != 0 || cfg.hassecammogoal != 0) && scores1[k] > bestscore1 {
                bestscore1 = scores1[k];
            }
        }
        let mut chosen = None;
        let mut done = false;
        // A shield, if hurt enough and short of one.
        let a = self.ab(i).clone();
        for k in 0..NUM_MPWEAPONSLOTS {
            if done {
                break;
            }
            if weaponnums[k] != WEAPON_MPSHIELD {
                continue;
            }
            let mut triggerathealth = 8.1f32;
            let mut desiredshield;
            let rf = a.randomfrac;
            if a.config.bottype == BOTTYPE_SHIELD {
                desiredshield = match criteria {
                    PICKUPCRITERIA_ANY => 7.9,
                    PICKUPCRITERIA_DEFAULT => 6.0 - (rf + rf),
                    _ => 4.0 - (rf + rf),
                };
            } else if barelydominatinghill {
                triggerathealth = 4.0 - (rf + rf);
                desiredshield = 1.0 - rf;
            } else {
                // (Capture the Case's carrier and Hacker Central's downloader: M10.)
                desiredshield = match criteria {
                    PICKUPCRITERIA_ANY => 7.9,
                    PICKUPCRITERIA_DEFAULT => 4.0 - (rf + rf),
                    _ => 2.0 - rf,
                };
            }
            match a.config.difficulty {
                BOTDIFF_MEAT => {
                    let r = a.random2 % 8;
                    if r < 2 {
                        desiredshield = 0.0;
                        triggerathealth = 0.0;
                    } else if r < 4 {
                        desiredshield = 0.0;
                        triggerathealth = 2.0 - rf;
                    } else {
                        desiredshield -= rf * 16.0;
                        if desiredshield <= 0.0 {
                            triggerathealth += desiredshield;
                            desiredshield = 0.0;
                        }
                    }
                }
                BOTDIFF_EASY => {
                    if a.random2.is_multiple_of(8) {
                        desiredshield = 0.0;
                        triggerathealth = 0.0;
                    } else {
                        desiredshield -= rf * 11.0;
                        if desiredshield <= 0.0 {
                            triggerathealth += desiredshield;
                            desiredshield = 0.0;
                        }
                    }
                }
                BOTDIFF_NORMAL => {
                    desiredshield -= rf * 4.0;
                    if desiredshield <= 0.0 {
                        triggerathealth += desiredshield;
                        desiredshield = 0.0;
                    }
                }
                _ => {}
            }
            let c = &self.chrs[i];
            if c.maxdamage - c.damage < triggerathealth && c.cshield <= desiredshield && weapproplist[k].is_some() && scores2[k] >= bestscore1 {
                chosen = weapproplist[k];
                done = true;
                break;
            }
        }
        // Ammo for the weapons it has, best first.
        let hasenough = |a: &Aibot, w: u8, func: usize, goal: i32| {
            let cfg = bot_weapon_config(&gset, w);
            let has = if func == FUNC_PRIMARY { cfg.haspriammogoal } else { cfg.hassecammogoal };
            has != 0 && a.botact_get_ammo_quantity_by_weapon(&gset, w, func, true) >= goal
        };
        for k in 0..NUM_MPWEAPONSLOTS {
            if done {
                break;
            }
            let cfg = bot_weapon_config(&gset, weaponnums[k]);
            if weaponnums[k] == WEAPON_MPSHIELD || !invitems[k] || (cfg.haspriammogoal == 0 && cfg.hassecammogoal == 0) || scores2[k] < bestscore1 {
                continue;
            }
            let (desiredpriammo, desiredsecammo);
            let mut include_equipped = true;
            let w = weaponnums[k];
            if barelydominatinghill {
                desiredpriammo = cfg.criticalammopri.min(1);
                desiredsecammo = cfg.criticalammosec.min(1);
                if hasenough(&a, w, FUNC_PRIMARY, desiredpriammo) || hasenough(&a, w, FUNC_SECONDARY, desiredsecammo) {
                    done = true;
                    break;
                }
            } else if criteria == PICKUPCRITERIA_ANY {
                desiredpriammo = Bgun::bgun_get_capacity_by_ammotype(botact_get_ammo_type_by_function(&gset, w, FUNC_PRIMARY));
                desiredsecammo = Bgun::bgun_get_capacity_by_ammotype(botact_get_ammo_type_by_function(&gset, w, FUNC_SECONDARY));
                if (cfg.haspriammogoal == 0 || a.botact_get_ammo_quantity_by_weapon(&gset, w, FUNC_PRIMARY, false) >= desiredpriammo) && (cfg.hassecammogoal == 0 || a.botact_get_ammo_quantity_by_weapon(&gset, w, FUNC_SECONDARY, false) >= desiredsecammo) {
                    continue;
                }
                include_equipped = false;
            } else if criteria == PICKUPCRITERIA_DEFAULT {
                desiredpriammo = cfg.targetammopri;
                desiredsecammo = cfg.targetammosec;
                if hasenough(&a, w, FUNC_PRIMARY, desiredpriammo) || hasenough(&a, w, FUNC_SECONDARY, desiredsecammo) {
                    done = true;
                    break;
                }
            } else {
                desiredpriammo = cfg.criticalammopri;
                desiredsecammo = cfg.criticalammosec;
                if hasenough(&a, w, FUNC_PRIMARY, desiredpriammo) || hasenough(&a, w, FUNC_SECONDARY, desiredsecammo) {
                    done = true;
                    break;
                }
            }
            for funcnum in 0..2 {
                if self.botinv_allows_weapon(i, w, funcnum) {
                    let ammotype = botact_get_ammo_type_by_function(&gset, w, funcnum);
                    if ammotype > 0 {
                        let goal = if funcnum != 0 { desiredsecammo } else { desiredpriammo };
                        let qty = a.botact_get_ammo_quantity_by_type(&gset, ammotype, include_equipped);
                        if qty < goal {
                            if let Some(p) = ammoproplist[ammotype as usize] {
                                chosen = Some(p);
                                done = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        // The best weapon it doesn't have.
        for k in 0..NUM_MPWEAPONSLOTS {
            if done {
                break;
            }
            if weaponnums[k] != WEAPON_MPSHIELD && !barelydominatinghill && (self.botinv_allows_weapon(i, weaponnums[k], FUNC_PRIMARY) || self.botinv_allows_weapon(i, weaponnums[k], FUNC_SECONDARY)) && !invitems[k] && weapproplist[k].is_some() {
                chosen = weapproplist[k];
                done = true;
                break;
            }
        }
        if criteria == PICKUPCRITERIA_ANY {
            // Ammo even for weapons it doesn't have.
            'outer: for k in 0..NUM_MPWEAPONSLOTS {
                if done {
                    break;
                }
                if weaponnums[k] == WEAPON_MPSHIELD {
                    continue;
                }
                for j in 0..2 {
                    if self.botinv_allows_weapon(i, weaponnums[k], j) {
                        let ammotype = botact_get_ammo_type_by_function(&gset, weaponnums[k], j);
                        if ammotype > 0 && a.botact_get_ammo_quantity_by_type(&gset, ammotype, false) < Bgun::bgun_get_capacity_by_ammotype(ammotype) {
                            if let Some(p) = ammoproplist[ammotype as usize] {
                                chosen = Some(p);
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
        chosen
    }

    /// `bot_check_fetch` (`bot.c:3680`), when a go-to's restart timer runs out:
    /// fetching and on the last leg, the pickup is marked so no simulant looks
    /// for it again (until it's taken); either way back to the main loop.
    /// Not fetching, route again.
    pub(crate) fn bot_check_fetch(&mut self, i: usize) {
        if self.ab(i).myaction == super::MyAction::GetItem {
            let c = &self.chrs[i];
            if c.act_gopos.curindex >= c.act_gopos.waypoints.len() {
                if let Some(id) = self.ab(i).gotoprop {
                    if let Some(o) = self.props.get_mut(id) {
                        if o.timetoregen == 0 {
                            o.flags3 |= OBJFLAG3_ISFETCHTARGET;
                        }
                    }
                }
            }
            self.ab_mut(i).forcemainloop = true;
            return;
        }
        let end = self.chrs[i].act_gopos.endpos;
        self.chr_go_to_room_pos(i, end);
    }

    /// The simulant's share of `mpstats_record_death` (`mpstats.c:331`): a kill
    /// (or a suicide) with the gun and function in hand, by the set's slot.
    pub(crate) fn mpstats_record_bot_kill(&mut self, aplayernum: i32, vplayernum: i32) {
        if aplayernum < 0 {
            return;
        }
        let weapons = self.setup.weapons;
        let Some(a) = self.chrs.get_mut(aplayernum as usize).and_then(|c| c.aibot.as_mut()) else { return };
        if let Some(slot) = mp_get_weapon_slot_by_weapon_num(&weapons, a.weaponnum as i32) {
            let f = a.gunfunc.min(1);
            if aplayernum == vplayernum {
                a.suicidesbygunfunc[slot][f] += 1.0;
            } else {
                a.killsbygunfunc[slot][f] += 1.0;
            }
        }
    }

    /// `bot_reset`'s shield (`bot.c:241`): a Turtle or Shield sim always has
    /// one, a DarkSim if the set has shields.
    pub(crate) fn bot_reset_shield(&mut self, i: usize) {
        let (ty, diff) = (self.ab(i).config.bottype, self.ab(i).config.difficulty);
        if ty == BOTTYPE_TURTLE || ty == BOTTYPE_SHIELD {
            self.chrs[i].cshield = 8.0;
        }
        if diff == BOTDIFF_DARK {
            // PD's @bug: this clears BOTFLAG_UNLIMITEDAMMO rather than setting it.
            self.ab_mut(i).flags &= !BOTFLAG_UNLIMITEDAMMO;
            if mp_has_shield(&self.setup.weapons) {
                self.chrs[i].cshield = 8.0;
            }
        }
    }
}
