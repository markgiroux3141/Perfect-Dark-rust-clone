//! A player's inventory (`inv.c`): the item list PD keeps sorted by weapon
//! (`inv_insert_item`, `inv_sort_item`), single weapons and dual pairs as
//! separate items, the multiplayer rule that two of a dual-wield weapon come
//! from two different pads (`inv_give_weapons_by_prop`), cycling
//! (`inv_choose_cycle_forward_weapon` / `_back_weapon`) and the pause menu's
//! rows (`inv_get_count`, `inv_get_weapon_num_by_index`).
//!
//! PD's list is circular and doubly linked with its head at the lowest
//! weapon; here it is a `Vec` in the same order. There are no prop items (keys
//! and the like are solo) and no all-guns cheat (`equipallguns` is false).

use pd_core::ids::*;

use super::Gset;

/// `struct invitem` (`types.h`), the weapon kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvItem {
    /// `INVITEMTYPE_WEAP`: one weapon, and the pad it was picked up from (-1:
    /// given, or dropped by a chr).
    Weap { weapon1: u8, pickuppad: i32 },
    /// `INVITEMTYPE_DUAL`: a pair, right then left.
    Dual { weapon1: u8, weapon2: u8 },
}

impl InvItem {
    /// `inv_sort_item`'s key: (weapon1, weapon2), -1 for a single.
    fn key(&self) -> (i32, i32) {
        match *self {
            InvItem::Weap { weapon1, .. } => (weapon1 as i32, -1),
            InvItem::Dual { weapon1, weapon2 } => (weapon1 as i32, weapon2 as i32),
        }
    }
}

/// `player->weapons` (the list) and `equipcuritem`.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub items: Vec<InvItem>,
    /// `equipcuritem`: the pause menu's row of the gun in hand.
    pub equipcuritem: i32,
}

impl Inventory {
    /// `inv_clear` (`inv.c:14`).
    pub fn inv_clear(&mut self) {
        self.items.clear();
        self.equipcuritem = 0;
    }

    /// `inv_insert_item` + `inv_sort_item` (`inv.c:96`, `:32`): the item goes
    /// in front of the first one that sorts at or after it.
    fn inv_insert_item(&mut self, item: InvItem) {
        let k = item.key();
        let at = self.items.iter().position(|c| {
            let ck = c.key();
            ck.0 >= k.0 && (k.0 != ck.0 || k.1 <= ck.1)
        });
        match at {
            Some(i) => self.items.insert(i, item),
            None => self.items.push(item),
        }
    }

    /// `inv_find_single_weapon` (`inv.c:192`).
    pub fn inv_find_single_weapon(&mut self, weaponnum: u8) -> Option<&mut InvItem> {
        self.items.iter_mut().find(|i| matches!(i, InvItem::Weap { weapon1, .. } if *weapon1 == weaponnum))
    }

    /// `inv_has_single_weapon_exc_all_guns` (`inv.c:212`).
    pub fn inv_has_single_weapon_exc_all_guns(&self, weaponnum: u8) -> bool {
        self.items.iter().any(|i| matches!(i, InvItem::Weap { weapon1, .. } if *weapon1 == weaponnum))
    }

    /// `inv_has_double_weapon_exc_all_guns` (`inv.c:239`).
    pub fn inv_has_double_weapon_exc_all_guns(&self, w1: u8, w2: u8) -> bool {
        self.items.iter().any(|i| matches!(*i, InvItem::Dual { weapon1, weapon2 } if weapon1 == w1 && weapon2 == w2))
    }

    /// `inv_has_double_weapon_inc_all_guns` (`inv.c:343`): no second weapon
    /// always counts.
    pub fn inv_has_double_weapon_inc_all_guns(&self, w1: u8, w2: u8) -> bool {
        w2 == WEAPON_NONE || self.inv_has_double_weapon_exc_all_guns(w1, w2)
    }

    /// `inv_give_single_weapon` (`inv.c:360`): true if it was new.
    pub fn inv_give_single_weapon(&mut self, weaponnum: u8) -> bool {
        if self.inv_has_single_weapon_exc_all_guns(weaponnum) {
            return false;
        }
        self.inv_insert_item(InvItem::Weap { weapon1: weaponnum, pickuppad: -1 });
        true
    }

    /// `inv_give_double_weapon` (`inv.c:388`): only a dual-wield weapon.
    pub fn inv_give_double_weapon(&mut self, gset: &Gset, w1: u8, w2: u8) -> bool {
        if self.inv_has_double_weapon_exc_all_guns(w1, w2) || !gset.has_flag(w1, WEAPONFLAG_DUALWIELD) {
            return false;
        }
        self.inv_insert_item(InvItem::Dual { weapon1: w1, weapon2: w2 });
        true
    }

    /// `inv_remove_item_by_num` (`inv.c:412`): its single and any pair it is in.
    pub fn inv_remove_item_by_num(&mut self, weaponnum: u8) {
        self.items.retain(|i| match *i {
            InvItem::Weap { weapon1, .. } => weapon1 != weaponnum,
            InvItem::Dual { weapon1, weapon2 } => weapon1 != weaponnum && weapon2 != weaponnum,
        });
    }

    /// `inv_give_weapons_by_prop` (`inv.c:498`) for a weapon object from pad
    /// `pad` (-1: not from a pad) in normal multiplayer: the weapon, and a
    /// dual-wield weapon's second once it comes from another pad. How many
    /// were given (2: now held twice). PD's `dualweapon` pairs are solo.
    pub fn inv_give_weapons_by_prop(&mut self, gset: &Gset, weaponnum: u8, pad: i32) -> i32 {
        let mut numgiven = 0;
        if self.inv_give_single_weapon(weaponnum) {
            numgiven = 1;
        }
        if gset.has_flag(weaponnum, WEAPONFLAG_DUALWIELD) && !self.inv_has_double_weapon_exc_all_guns(weaponnum, weaponnum) {
            let mut double = false;
            if let Some(InvItem::Weap { pickuppad, .. }) = self.inv_find_single_weapon(weaponnum) {
                if *pickuppad < 0 {
                    if pad >= 0 {
                        *pickuppad = pad;
                    }
                } else if pad >= 0 && *pickuppad != pad {
                    double = true;
                }
            }
            if double {
                numgiven = if self.inv_give_double_weapon(gset, weaponnum, weaponnum) { 2 } else { 0 };
            }
        }
        numgiven
    }

    /// The pad `weaponnum`'s single item came from (`item->type_weap.pickuppad`).
    pub fn pickuppad(&self, weaponnum: u8) -> Option<i32> {
        self.items.iter().find_map(|i| match *i {
            InvItem::Weap { weapon1, pickuppad } if weapon1 == weaponnum => Some(pickuppad),
            _ => None,
        })
    }

    /// `inv_choose_cycle_forward_weapon(ptr1, ptr2, arg2)` (`inv.c:575`) with
    /// `usable` as `bgun0f0a1a10`: the next item after (w1, w2), wrapping.
    pub fn inv_choose_cycle_forward_weapon(&self, w1: i32, w2: i32, arg2: bool, usable: &dyn Fn(i32) -> bool) -> (i32, i32) {
        let (mut weapon1, mut weapon2) = (w1, w2);
        if self.items.is_empty() {
            return (weapon1, weapon2);
        }
        let n = self.items.len();
        let mut i = 0;
        loop {
            match self.items[i] {
                InvItem::Weap { weapon1: a, .. } => {
                    let a = a as i32;
                    if a < NUM_CYCLEABLE_WEAPONS as i32 && a > weapon1 && (!arg2 || usable(a)) {
                        return (a, WEAPON_NONE as i32);
                    }
                }
                InvItem::Dual { weapon1: a, weapon2: b } => {
                    let (a, b) = (a as i32, b as i32);
                    if (a > weapon1 || (weapon1 == a && b > weapon2)) && (!arg2 || usable(a) || usable(b)) {
                        return (a, b);
                    }
                }
            }
            i = (i + 1) % n;
            if i == 0 {
                if arg2 {
                    return (weapon1, weapon2);
                }
                weapon1 = -1;
                weapon2 = -1;
            }
        }
    }

    /// `inv_choose_cycle_back_weapon(ptr1, ptr2, arg2)` (`inv.c:643`).
    pub fn inv_choose_cycle_back_weapon(&self, w1: i32, w2: i32, arg2: bool, usable: &dyn Fn(i32) -> bool) -> (i32, i32) {
        let (mut weapon1, mut weapon2) = (w1, w2);
        if self.items.is_empty() {
            return (weapon1, weapon2);
        }
        let n = self.items.len();
        let mut i = n - 1;
        loop {
            match self.items[i] {
                InvItem::Weap { weapon1: a, .. } => {
                    let a = a as i32;
                    if a < NUM_CYCLEABLE_WEAPONS as i32 && (a < weapon1 || (weapon1 == a && weapon2 > 0)) && (!arg2 || usable(a)) {
                        return (a, WEAPON_NONE as i32);
                    }
                }
                InvItem::Dual { weapon1: a, weapon2: b } => {
                    let (a, b) = (a as i32, b as i32);
                    if (a < weapon1 || (weapon1 == a && b < weapon2)) && (!arg2 || usable(a) || usable(b)) {
                        return (a, b);
                    }
                }
            }
            if i == 0 {
                if arg2 {
                    return (weapon1, weapon2);
                }
                weapon1 = 1000;
                weapon2 = 1000;
            }
            i = if i == 0 { n - 1 } else { i - 1 };
        }
    }

    /// Each single weapon in list order, and whether it is also held twice
    /// (the keyboard's number keys, the snapshot tools).
    pub fn weapons(&self) -> Vec<(u8, bool)> {
        self.items
            .iter()
            .filter_map(|i| match *i {
                InvItem::Weap { weapon1, .. } => Some((weapon1, self.inv_has_double_weapon_exc_all_guns(weapon1, weapon1))),
                _ => None,
            })
            .collect()
    }

    /// `inv_get_count` (`inv.c:806`): the pause menu lists the singles.
    pub fn inv_get_count(&self) -> i32 {
        self.items.iter().filter(|i| matches!(i, InvItem::Weap { .. })).count() as i32
    }

    /// `inv_get_weapon_num_by_index` (`inv.c:942`): 0 past the end.
    pub fn inv_get_weapon_num_by_index(&self, index: i32) -> u8 {
        self.items
            .iter()
            .filter_map(|i| match *i {
                InvItem::Weap { weapon1, .. } => Some(weapon1),
                _ => None,
            })
            .nth(index.max(0) as usize)
            .filter(|_| index >= 0)
            .unwrap_or(0)
    }

    /// `inv_calculate_current_index` (`inv.c:1066`) for the gun in the right hand.
    pub fn inv_calculate_current_index(&mut self, curweaponnum: u8) {
        self.equipcuritem = 0;
        for i in 0..self.inv_get_count() {
            if self.inv_get_weapon_num_by_index(i) == curweaponnum {
                self.equipcuritem = i;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_sort_by_weapon_with_the_pair_after_its_single_and_cycling_wraps_through_both() {
        let gset = crate::testutil::res().gset.clone();
        let mut inv = Inventory::default();
        inv.inv_give_single_weapon(WEAPON_UNARMED);
        assert_eq!(inv.inv_give_weapons_by_prop(&gset, WEAPON_CMP150, 50), 1);
        assert_eq!(inv.inv_give_weapons_by_prop(&gset, WEAPON_FALCON2, 47), 1);
        // The same pad again (the pickup came back): no pair.
        assert_eq!(inv.inv_give_weapons_by_prop(&gset, WEAPON_FALCON2, 47), 0);
        // Another pad's Falcon: the pair.
        assert_eq!(inv.inv_give_weapons_by_prop(&gset, WEAPON_FALCON2, 48), 2);
        // The shotgun doesn't dual-wield.
        inv.inv_give_weapons_by_prop(&gset, WEAPON_SHOTGUN, 51);
        assert_eq!(inv.inv_give_weapons_by_prop(&gset, WEAPON_SHOTGUN, 52), 0);
        let keys: Vec<(i32, i32)> = inv.items.iter().map(|i| i.key()).collect();
        let (u, f, c, s) = (WEAPON_UNARMED as i32, WEAPON_FALCON2 as i32, WEAPON_CMP150 as i32, WEAPON_SHOTGUN as i32);
        assert_eq!(keys, vec![(u, -1), (f, -1), (f, f), (c, -1), (s, -1)]);
        let any = |_| true;
        let mut cur = (u, 0);
        let mut seen = vec![];
        for _ in 0..6 {
            cur = inv.inv_choose_cycle_forward_weapon(cur.0, cur.1, false, &any);
            seen.push(cur);
        }
        assert_eq!(seen, vec![(f, 0), (f, f), (c, 0), (s, 0), (u, 0), (f, 0)]);
        let back = inv.inv_choose_cycle_back_weapon(c, 0, false, &any);
        assert_eq!(back, (f, f));
        assert_eq!(inv.inv_get_count(), 4);
        assert_eq!(inv.inv_get_weapon_num_by_index(2), WEAPON_CMP150);
        inv.inv_remove_item_by_num(WEAPON_FALCON2);
        assert!(!inv.inv_has_single_weapon_exc_all_guns(WEAPON_FALCON2));
        assert!(!inv.inv_has_double_weapon_exc_all_guns(WEAPON_FALCON2, WEAPON_FALCON2));
    }
}
