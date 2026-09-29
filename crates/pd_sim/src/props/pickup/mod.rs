//! The Combat Simulator's pickups: the setup's weapon locations, their ammo
//! crates and shields (`setup.c`), their respawn (`prop.c`, `obj_tick`), and a
//! player walking over one (`props_test_for_pickup`, `obj_test_for_pickup`,
//! `prop_pickup_by_player`, `propobj.c`), with PD's pickup sounds and HUD
//! messages. A simulant's side is `crate::bot::botinv`.
//!
//! A weapon location (`WEAPON_MPLOCATION00 + n` in `props[]`) takes the weapon
//! set's slots in turn (`mp_get_mp_weapon_by_location`); the `ammocratemulti`s
//! that follow it hold that weapon's ammo (`g_SetupCurMpLocation`). A location
//! holding the shield is a shield; the grenades and mines have only their
//! crates (`hasweapon` 0), whose pickup gives the weapon (`ammo_handle_pickup`).
//!
//! A pickup taken is gone for 20 s, the last one fading back in with the regen
//! chime; dropped weapons (a death's) don't come back.

use glam::{Mat3, Vec3};
use pd_core::ids::*;
use pd_core::lang::{tx, LANGBANK_PROPOBJ};

use super::Obj;
use crate::gun::Bgun;
use crate::propsnd::DEFAULT_DISTS;
use crate::world::World;

/// `prop_execute_tick_operation(TICKOP_FREE)`'s wait (`prop.c:1398`).
pub const REGEN_TIME60: i32 = 1200;

/// `SFXNUM_*` of the pickups (`propobj.c:15851`).
pub const SFXNUM_00E8_PICKUP_GUN: u16 = 0x00e8;
pub const SFXNUM_00E9_PICKUP_KNIFE: u16 = 0x00e9;
pub const SFXNUM_00EA_PICKUP_AMMO: u16 = 0x00ea;
pub const SFXNUM_00EB_PICKUP_MINE: u16 = 0x00eb;
pub const SFXNUM_00F2_PICKUP_LASER: u16 = 0x00f2;
pub const SFXNUM_00E5_PICKUP_KEYCARD: u16 = 0x00e5;
pub const SFXNUM_01CD_PICKUP_SHIELD: u16 = 0x01cd;
pub const SFXNUM_0052_REGEN: u16 = 0x0052;

fn l(n: u16) -> pd_core::lang::Tx {
    tx(LANGBANK_PROPOBJ, n)
}

/// `ammotype_play_pickup_sound` (`propobj.c:15851`).
pub fn ammotype_pickup_sound(ammotype: i32) -> Option<u16> {
    match ammotype {
        AMMOTYPE_PISTOL | AMMOTYPE_SMG | AMMOTYPE_RIFLE | AMMOTYPE_SHOTGUN | AMMOTYPE_GRENADE | AMMOTYPE_ROCKET | AMMOTYPE_MAGNUM | AMMOTYPE_DEVASTATOR | AMMOTYPE_REAPER | AMMOTYPE_HOMINGROCKET | AMMOTYPE_DART | AMMOTYPE_NBOMB | AMMOTYPE_SEDATIVE | AMMOTYPE_CLOAK | AMMOTYPE_BOOST | AMMOTYPE_TOKEN => {
            Some(SFXNUM_00EA_PICKUP_AMMO)
        }
        AMMOTYPE_REMOTE_MINE | AMMOTYPE_PROXY_MINE | AMMOTYPE_TIMED_MINE | AMMOTYPE_BUG | AMMOTYPE_MICROCAMERA | AMMOTYPE_PLASTIQUE | AMMOTYPE_ECM_MINE => Some(SFXNUM_00EB_PICKUP_MINE),
        AMMOTYPE_KNIFE => Some(SFXNUM_00E9_PICKUP_KNIFE),
        _ => None,
    }
}

/// `weapon_play_pickup_sound` (`propobj.c:15916`); `prop_play_pickup_sound`
/// (`:15887`, a simulant's) is the same list less the bolt and the keycards.
pub fn weapon_pickup_sound(weaponnum: u8) -> u16 {
    match weaponnum {
        WEAPON_COMBATKNIFE => SFXNUM_00E9_PICKUP_KNIFE,
        WEAPON_REMOTEMINE | WEAPON_PROXIMITYMINE | WEAPON_TIMEDMINE | WEAPON_TRACERBUG | WEAPON_TARGETAMPLIFIER | WEAPON_COMMSRIDER | WEAPON_ECMMINE => SFXNUM_00EB_PICKUP_MINE,
        WEAPON_GRENADE | WEAPON_GRENADEROUND | WEAPON_ROCKET | WEAPON_HOMINGROCKET => SFXNUM_00EA_PICKUP_AMMO,
        WEAPON_LASER => SFXNUM_00F2_PICKUP_LASER,
        WEAPON_BOLT => SFXNUM_00E8_PICKUP_GUN,
        WEAPON_EYESPY => SFXNUM_00E5_PICKUP_KEYCARD,
        w if w > WEAPON_PSYCHOSISGUN => SFXNUM_00E5_PICKUP_KEYCARD,
        _ => SFXNUM_00E8_PICKUP_GUN,
    }
}

/// `ammocrate_get_pickup_ammo_qty`'s weapon twin, `weapon_get_pickup_ammo_qty`
/// (`propobj.c:16075`) in normal multiplayer: the primary's ammo a weapon
/// pickup comes with.
pub fn weapon_get_pickup_ammo_qty(o: &Obj, ammotype: i32) -> i32 {
    if o.weaponnum == WEAPON_COMBATKNIFE || o.weaponnum == WEAPON_BOLT {
        return 1;
    }
    if o.flags & OBJFLAG_WEAPON_NOAMMO != 0 {
        return 0;
    }
    match ammotype {
        AMMOTYPE_PISTOL => 10,
        AMMOTYPE_SMG => 20,
        AMMOTYPE_CROSSBOW => 5,
        AMMOTYPE_RIFLE => 20,
        AMMOTYPE_SHOTGUN => 10,
        AMMOTYPE_FARSIGHT => 4,
        AMMOTYPE_MAGNUM => 10,
        AMMOTYPE_DEVASTATOR => 3,
        AMMOTYPE_REAPER => 200,
        AMMOTYPE_DART => 10,
        AMMOTYPE_CLOAK => 1200,
        AMMOTYPE_SEDATIVE => 16,
        AMMOTYPE_BOOST => 1,
        _ => 1,
    }
}

/// `botact_get_weapon_by_ammo_type` (`botact.c:325`): the weapon an ammo type
/// *is* (the throwables).
pub fn botact_get_weapon_by_ammo_type(ammotype: i32) -> u8 {
    match ammotype {
        AMMOTYPE_NBOMB => WEAPON_NBOMB,
        AMMOTYPE_GRENADE => WEAPON_GRENADE,
        AMMOTYPE_KNIFE => WEAPON_COMBATKNIFE,
        AMMOTYPE_REMOTE_MINE => WEAPON_REMOTEMINE,
        AMMOTYPE_PROXY_MINE => WEAPON_PROXIMITYMINE,
        AMMOTYPE_TIMED_MINE => WEAPON_TIMEDMINE,
        _ => 0,
    }
}

impl Obj {
    /// Gone while its respawn waits (`OBJHFLAG_GONE`): not drawn, not
    /// collected, not in the way.
    pub fn is_gone(&self) -> bool {
        self.hidden & OBJHFLAG_GONE != 0
    }

    /// Something a player or simulant collects: a weapon, a crate, a shield.
    pub fn is_pickup(&self) -> bool {
        matches!(self.ty, OBJTYPE_WEAPON | OBJTYPE_AMMOCRATE | OBJTYPE_MULTIAMMOCRATE | OBJTYPE_SHIELD)
    }

    /// `obj_defaults_to_bounceable_invincible_pickupable` (`propobj.c:14357`).
    pub fn obj_defaults_to_bounceable_invincible_pickupable(&self) -> bool {
        matches!(self.ty, OBJTYPE_KEY | OBJTYPE_AMMOCRATE | OBJTYPE_WEAPON | OBJTYPE_HAT | OBJTYPE_MULTIAMMOCRATE | OBJTYPE_SHIELD)
    }
}

impl World {
    /// A pickup before placing: its model at `g_ModelStates[].scale / 4096`
    /// (`obj_init`; `setup_create_object` applies the setup's extra scale),
    /// regenerating in a match (`OBJH2FLAG_CANREGEN`, `setup.c:305`).
    pub(crate) fn pickup_obj(&mut self, stem: &str, _extrascale: i32, ty: u8, weaponnum: u8, flags: u32, pad: i32) -> Option<Obj> {
        let def = self.res.models.get(stem).ok()?;
        let scale = self.res.models.modelstate_scale(stem);
        let id = self.props.alloc_id();
        let mut o = Obj::weapon(id, def, scale, weaponnum, FUNC_PRIMARY, 0);
        o.ty = ty;
        o.hidden = 0;
        o.flags = flags;
        o.pad = pad;
        o.hidden2 |= OBJH2FLAG_CANREGEN;
        if ty != OBJTYPE_WEAPON {
            o.vis.clear();
        }
        Some(o)
    }

    /// `obj_tick`'s respawn (`propobj.c:10951`), from `props_tick`: a taken
    /// pickup waits 20 s; with 1 s left it is back (drawn fading in), and when
    /// the wait ends it can be taken again, with the regen chime (a shield back
    /// at full).
    pub(crate) fn pickups_tick_regen(&mut self) {
        let lv60 = self.lv.lvupdate60;
        let mut chimes = Vec::new();
        for o in self.props.objs.iter_mut() {
            if o.timetoregen <= 0 {
                continue;
            }
            let fadingin = o.timetoregen < 60;
            o.timetoregen -= lv60;
            // prop_can_regen is always true.
            if o.timetoregen <= 0 {
                o.timetoregen = 0;
            } else if o.timetoregen < 60 && !fadingin {
                // prop_enable + obj_detect_rooms: back in the world.
                o.hidden &= !OBJHFLAG_GONE;
                if o.ty == OBJTYPE_SHIELD {
                    o.shieldamount = o.shieldinitialamount;
                }
                chimes.push(o.pos);
            }
        }
        for pos in chimes {
            self.sound_at(SFXNUM_0052_REGEN, 1.0, pos, DEFAULT_DISTS);
        }
    }

    /// `prop_execute_tick_operation(prop, TICKOP_FREE)` (`prop.c:1391`) for an
    /// object: a setup pickup is taken for [`REGEN_TIME60`]; anything else goes.
    pub(crate) fn obj_free_pickup(&mut self, id: u32) {
        let Some(i) = self.props.objs.iter().position(|o| o.id == id) else { return };
        let o = &mut self.props.objs[i];
        if o.hidden2 & OBJH2FLAG_CANREGEN != 0 {
            o.timetoregen = REGEN_TIME60;
            o.hidden |= OBJHFLAG_GONE;
            o.hidden &= !OBJHFLAG_DELETING;
            o.hidden2 &= !OBJH2FLAG_DESTROYED;
        } else {
            self.props.objs.remove(i);
        }
    }

    /// `props_test_for_pickup` (`prop.c:2326`) for player `pi`, in its
    /// `lv_render` pass (`lv.c:1304`).
    ///
    /// `// SUBST:` PD tests the props in the player's rooms and their
    /// neighbours (`room_get_props`) / every object, in list order: the pickup
    /// tests' 1 m / ±2 m range and line of sight decide the same.
    pub(crate) fn props_test_for_pickup(&mut self, pi: usize) {
        // A launcher's loaded rocket is the gun's child (`prop->parent`), in no
        // room's list.
        let ids: Vec<u32> = self.props.objs.iter().filter(|o| o.timetoregen <= 0 && o.flags & OBJFLAG_HELDROCKET == 0).map(|o| o.id).collect();
        for id in ids {
            if self.obj_test_for_pickup(pi, id) {
                self.obj_free_pickup(id);
            }
        }
    }

    /// `obj_test_for_pickup` (`propobj.c:16513`): is the object collectable,
    /// does the player want it (not at full ammo, a second of a dual-wield
    /// weapon only from another pad, a shield better than theirs), not looking
    /// steeply down, within 1 m across and 2 m up or down, in sight. True: it
    /// was picked up and is freed (`TICKOP_FREE`).
    fn obj_test_for_pickup(&mut self, pi: usize, id: u32) -> bool {
        let gset = self.res.gset.clone();
        let Some(o) = self.props.get(id) else { return false };
        if o.is_deleting() || o.is_gone() || !o.is_pickup() {
            return false;
        }
        if o.obj_defaults_to_bounceable_invincible_pickupable() {
            if o.flags & OBJFLAG_UNCOLLECTABLE != 0 {
                return false;
            }
        } else if o.flags & OBJFLAG_COLLECTABLE == 0 {
            return false;
        }
        if o.flags & OBJFLAG_THROWNLAPTOP != 0 {
            return false;
        }
        // A thrown weapon can't be caught back within a second unless it bounced.
        // (Disarms, which set `pickupby`, are solo.)
        if let Some(p) = o.projectile.as_ref() {
            if p.pickuptimer240 > 0 && p.bouncecount == 0 {
                return false;
            }
        }
        let gun = &self.players[pi].gun;
        let inv = &gun.p.inventory;
        match o.ty {
            OBJTYPE_WEAPON => {
                let w = o.weaponnum;
                if matches!(w, WEAPON_GRENADE | WEAPON_GRENADEROUND | WEAPON_NBOMB | WEAPON_SKROCKET) && o.timer240 >= 0 {
                    return false;
                }
                if (matches!(w, WEAPON_REMOTEMINE | WEAPON_PROXIMITYMINE | WEAPON_TIMEDMINE | WEAPON_TRACERBUG | WEAPON_TARGETAMPLIFIER | WEAPON_COMMSRIDER | WEAPON_ECMMINE) || (w == WEAPON_DRAGON && o.gunfunc == FUNC_SECONDARY)) && o.timer240 >= 0 {
                    return false;
                }
                if matches!(w, WEAPON_ROCKET | WEAPON_HOMINGROCKET | WEAPON_BOLT | WEAPON_COMBATKNIFE) && o.projectile.is_some() {
                    return false;
                }
                if inv.inv_has_single_weapon_exc_all_guns(w) && Bgun::bgun_get_ammo_type_for_weapon(&gset, w, FUNC_PRIMARY) != 0 {
                    let mut maybe = gun.bgun_get_ammo_qty_for_weapon(&gset, w, FUNC_PRIMARY) >= Bgun::bgun_get_ammo_capacity_for_weapon(&gset, w, FUNC_PRIMARY);
                    if w == WEAPON_SUPERDRAGON && gun.bgun_get_ammo_qty_for_weapon(&gset, w, FUNC_SECONDARY) < Bgun::bgun_get_ammo_capacity_for_weapon(&gset, w, FUNC_SECONDARY) {
                        maybe = false;
                    }
                    if maybe {
                        // Full: only the second of a dual-wield weapon from another pad.
                        if gset.has_flag(w, WEAPONFLAG_DUALWIELD) && !inv.inv_has_double_weapon_exc_all_guns(w, w) {
                            if inv.pickuppad(w) == Some(o.pad) || o.pad < 0 {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                }
            }
            OBJTYPE_MULTIAMMOCRATE => {
                let mut ignore = true;
                for i in 0..=(AMMOTYPE_NBOMB as usize) {
                    let ammotype = i as i32 + 1;
                    if o.ammoslots[i] > 0 {
                        if gun.bgun_get_reserved_ammo_count(&gset, ammotype) < Bgun::bgun_get_capacity_by_ammotype(ammotype) {
                            ignore = false;
                            break;
                        }
                        let w = match ammotype {
                            AMMOTYPE_GRENADE => WEAPON_GRENADE,
                            AMMOTYPE_CLOAK => WEAPON_CLOAKINGDEVICE,
                            AMMOTYPE_BOOST => WEAPON_COMBATBOOST,
                            AMMOTYPE_NBOMB => WEAPON_NBOMB,
                            AMMOTYPE_REMOTE_MINE => WEAPON_REMOTEMINE,
                            AMMOTYPE_PROXY_MINE => WEAPON_PROXIMITYMINE,
                            AMMOTYPE_TIMED_MINE => WEAPON_TIMEDMINE,
                            AMMOTYPE_KNIFE => WEAPON_COMBATKNIFE,
                            _ => WEAPON_NONE,
                        };
                        if w != WEAPON_NONE && !inv.inv_has_single_weapon_exc_all_guns(w) {
                            ignore = false;
                            break;
                        }
                    }
                }
                if ignore {
                    return false;
                }
            }
            OBJTYPE_SHIELD => {
                // (Hold the Briefcase's briefcase carrier: M10.)
                if o.shieldamount <= self.player_get_shield_frac(pi) {
                    return false;
                }
            }
            _ => {}
        }
        let p = &self.players[pi];
        // BADDTOR3(vv_verta) < BADDTOR(-45) with magnetattracttime -1
        // (`bondgunreset.c:225`, never set): looking steeply down, nothing.
        if p.verta < -45.0 {
            return false;
        }
        let d = o.pos - p.pos;
        let pickup = d.x * d.x + d.z * d.z <= 100.0 * 100.0 && d.y >= -200.0 && d.y <= 200.0;
        if !pickup {
            return false;
        }
        if o.flags2 & OBJFLAG2_PICKUPWITHOUTLOS == 0 && !self.level.los_autoflags(p.pos, o.pos) {
            return false;
        }
        self.prop_pickup_by_player(pi, id, true)
    }

    /// `prop_pickup_by_player` (`propobj.c:16247`): give player `pi` the object.
    /// True for `TICKOP_FREE` (every Combat Simulator pickup).
    fn prop_pickup_by_player(&mut self, pi: usize, id: u32, showhudmsg: bool) -> bool {
        if self.players[pi].isdead || self.lv.lvupdate240 == 0 {
            return false;
        }
        let gset = self.res.gset.clone();
        let Some(o) = self.props.get(id).cloned() else { return false };
        match o.ty {
            OBJTYPE_MULTIAMMOCRATE => {
                for i in 0..19 {
                    self.ammo_handle_pickup(pi, i as i32 + 1, o.ammoslots[i], false, showhudmsg);
                }
                self.sound(SFXNUM_00EA_PICKUP_AMMO, 1.0);
            }
            OBJTYPE_WEAPON => {
                let w = o.weaponnum;
                let mut sp70 = false;
                let count;
                // (The briefcase and the data uplink: M10's scenarios.)
                self.sound(weapon_pickup_sound(w), 1.0);
                let mut showhudmsg = showhudmsg;
                if w == WEAPON_BOLT {
                    count = 1;
                    self.ammo_handle_pickup(pi, AMMOTYPE_CROSSBOW, 1, true, true);
                    showhudmsg = false;
                    sp70 = true;
                } else {
                    count = self.players[pi].gun.p.inventory.inv_give_weapons_by_prop(&gset, w, o.pad);
                    if count != 0 {
                        sp70 = true;
                    }
                    // inv_get_pickup_text_by_weapon_num: no text overrides in a match.
                    if showhudmsg && sp70 {
                        self.current_player_queue_pickup_weapon_hudmsg(pi, w, count == 2);
                    }
                }
                let gun = &mut self.players[pi].gun;
                if count == 2 && gun.bgun_get_weapon_num(HAND_RIGHT) == w && gun.bgun_get_weapon_num(HAND_LEFT) != w {
                    // bgun_equip_weapon2(HAND_LEFT, weaponnum) (`bondgun.c:5788`):
                    // the left hand comes up with the second.
                    gun.ctrl.dualwielding = true;
                }
                let ammotype = Bgun::bgun_get_ammo_type_for_weapon(&gset, w, FUNC_PRIMARY);
                if ammotype != 0 {
                    let pickupqty = weapon_get_pickup_ammo_qty(&o, ammotype);
                    if pickupqty > 0 {
                        let gun = &mut self.players[pi].gun;
                        let heldqty = gun.bgun_get_reserved_ammo_count(&gset, ammotype);
                        if heldqty < Bgun::bgun_get_capacity_by_ammotype(ammotype) {
                            gun.bgun_set_ammo_quantity(&gset, ammotype, heldqty + pickupqty);
                            if !sp70 && showhudmsg {
                                self.current_player_queue_pickup_ammo_hudmsg(pi, ammotype, pickupqty);
                            }
                        }
                    }
                }
                if w == WEAPON_SUPERDRAGON {
                    let pickupqty = weapon_get_pickup_ammo_qty(&o, ammotype);
                    let gun = &mut self.players[pi].gun;
                    let held = gun.bgun_get_reserved_ammo_count(&gset, AMMOTYPE_DEVASTATOR);
                    if held < Bgun::bgun_get_capacity_by_ammotype(AMMOTYPE_DEVASTATOR) {
                        gun.bgun_set_ammo_quantity(&gset, AMMOTYPE_DEVASTATOR, held + 5);
                        if !sp70 && showhudmsg {
                            self.current_player_queue_pickup_ammo_hudmsg(pi, AMMOTYPE_DEVASTATOR, pickupqty);
                        }
                    }
                }
            }
            OBJTYPE_SHIELD => {
                self.player_set_shield_frac(pi, o.shieldamount);
                self.sound(SFXNUM_01CD_PICKUP_SHIELD, 1.0);
                if showhudmsg {
                    let text = if self.hudmsg_full_text() { self.res.lang.get(l(41)) } else { self.res.lang.get(l(42)) }.to_string();
                    self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_DEFAULT, HUDMSGFLAG_ONLYIFALIVE);
                }
            }
            _ => return false,
        }
        true
    }

    /// `PLAYERCOUNT() <= 2 && !(2 && (SCREENSPLIT_VERTICAL || IS4MB()))`: the
    /// long form of a pickup message ("Picked up a ...").
    fn hudmsg_full_text(&self) -> bool {
        // SUBST: two players read the split from the options / horizontal until
        // split-screen (M12).
        self.players.len() <= 2
    }

    /// `ammo_handle_pickup` (`propobj.c:16003`): the reserve topped up (unless
    /// full), the message, the sound, and a throwable's weapon.
    pub(crate) fn ammo_handle_pickup(&mut self, pi: usize, ammotype: i32, quantity: i32, withsound: bool, withhudmsg: bool) {
        if quantity <= 0 {
            return;
        }
        let gset = self.res.gset.clone();
        let gun = &mut self.players[pi].gun;
        let held = gun.bgun_get_reserved_ammo_count(&gset, ammotype);
        if held < Bgun::bgun_get_capacity_by_ammotype(ammotype) {
            gun.bgun_set_ammo_quantity(&gset, ammotype, held + quantity);
            if withhudmsg {
                self.current_player_queue_pickup_ammo_hudmsg(pi, ammotype, quantity);
            }
        }
        if withsound {
            if let Some(s) = ammotype_pickup_sound(ammotype) {
                self.sound(s, 1.0);
            }
        }
        let weapon = match ammotype {
            AMMOTYPE_GRENADE => Some(WEAPON_GRENADE),
            AMMOTYPE_REMOTE_MINE => Some(WEAPON_REMOTEMINE),
            AMMOTYPE_PROXY_MINE => Some(WEAPON_PROXIMITYMINE),
            AMMOTYPE_TIMED_MINE => Some(WEAPON_TIMEDMINE),
            AMMOTYPE_NBOMB => Some(WEAPON_NBOMB),
            AMMOTYPE_KNIFE => Some(WEAPON_COMBATKNIFE),
            AMMOTYPE_ECM_MINE => Some(WEAPON_ECMMINE),
            AMMOTYPE_TOKEN => Some(WEAPON_BRIEFCASE2),
            AMMOTYPE_CLOAK => Some(WEAPON_CLOAKINGDEVICE),
            AMMOTYPE_BOOST => Some(WEAPON_COMBATBOOST),
            _ => None,
        };
        if let Some(w) = weapon {
            self.players[pi].gun.p.inventory.inv_give_single_weapon(w);
        }
    }

    /// `current_player_queue_pickup_weapon_hudmsg` (`propobj.c:16237`).
    fn current_player_queue_pickup_weapon_hudmsg(&mut self, pi: usize, weaponnum: u8, dual: bool) {
        let text = self.weapon_get_pickup_text(weaponnum, dual);
        self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_DEFAULT, HUDMSGFLAG_ONLYIFALIVE | HUDMSGFLAG_ALLOWDUPES);
    }

    /// `current_player_queue_pickup_ammo_hudmsg` (`propobj.c:15995`).
    fn current_player_queue_pickup_ammo_hudmsg(&mut self, pi: usize, ammotype: i32, qty: i32) {
        let text = self.ammotype_get_pickup_message(ammotype, qty);
        self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_DEFAULT, HUDMSGFLAG_ONLYIFALIVE);
    }

    /// `weapon_get_pickup_text` (`propobj.c:16135`, NTSC): "Picked up a Falcon 2.",
    /// "Double Falcon 2.", "Picked up some N-Bombs."; the short name and
    /// capitalised determiner on a small view.
    pub fn weapon_get_pickup_text(&self, weaponnum: u8, dual: bool) -> String {
        let lang = &self.res.lang;
        let full = self.hudmsg_full_text();
        let gset = &self.res.gset;
        let mut buffer = String::new();
        if dual {
            buffer.push_str(lang.get(l(1)));
        } else if full {
            buffer.push_str(lang.get(l(0)));
            let textid = if gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_F_SOME) {
                2
            } else if gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_F_AN) {
                6
            } else if gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_F_THE) {
                8
            } else {
                4
            };
            buffer.push_str(lang.get(l(textid)));
        } else {
            let textid = if gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_S_SOME) {
                3
            } else if gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_S_AN) {
                7
            } else if gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_S_THE) {
                9
            } else {
                5
            };
            buffer.push_str(lang.get(l(textid)));
        }
        let def = gset.weapon(weaponnum);
        let plural = if full {
            buffer.push_str(def.map_or("", |d| d.name.as_str()));
            gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_F_SOME)
        } else {
            buffer.push_str(def.map_or("", |d| d.short_name.as_str()));
            gset.has_flag(weaponnum, WEAPONFLAG_DETERMINER_S_SOME)
        };
        // The names end in a line break (lang_get), removed.
        while buffer.ends_with('\n') {
            buffer.pop();
        }
        if plural {
            buffer.push('s');
        }
        buffer.push_str(".\n");
        buffer
    }

    /// `ammotype_get_pickup_message` (`propobj.c:15950`, NTSC): "Picked up some
    /// ammo.", "Picked up a grenade."
    pub fn ammotype_get_pickup_message(&self, ammotype: i32, qty: i32) -> String {
        let lang = &self.res.lang;
        let full = self.hudmsg_full_text();
        let mut dst = String::new();
        if full {
            dst.push_str(lang.get(l(0)));
        }
        // ammotype_get_determiner (`propobj.c:15700`).
        let (mut a, mut an, mut some, mut the) = (false, false, false, false);
        match ammotype {
            AMMOTYPE_CLOAK => a = true,
            AMMOTYPE_PISTOL | AMMOTYPE_SMG | AMMOTYPE_RIFLE | AMMOTYPE_SEDATIVE | AMMOTYPE_PSYCHOSIS | AMMOTYPE_PLASTIQUE => some = true,
            AMMOTYPE_CROSSBOW | AMMOTYPE_SHOTGUN | AMMOTYPE_GRENADE | AMMOTYPE_ROCKET | AMMOTYPE_KNIFE | AMMOTYPE_MAGNUM | AMMOTYPE_DEVASTATOR | AMMOTYPE_REMOTE_MINE | AMMOTYPE_PROXY_MINE | AMMOTYPE_TIMED_MINE | AMMOTYPE_REAPER | AMMOTYPE_HOMINGROCKET | AMMOTYPE_DART | AMMOTYPE_BOOST | AMMOTYPE_BUG | AMMOTYPE_MICROCAMERA => {
                if qty == 1 {
                    a = true;
                } else {
                    some = true;
                }
            }
            AMMOTYPE_FARSIGHT | AMMOTYPE_NBOMB | AMMOTYPE_ECM_MINE => {
                if qty == 1 {
                    an = true;
                } else {
                    some = true;
                }
            }
            AMMOTYPE_TOKEN => {
                if qty == 1 {
                    the = true;
                } else {
                    some = true;
                }
            }
            _ => {}
        }
        for (on, f, s) in [(a, 4, 5), (an, 6, 7), (some, 2, 3), (the, 8, 9)] {
            if on {
                dst.push_str(lang.get(l(if full { f } else { s })));
            }
        }
        // ammotype_get_pickup_name (`propobj.c:15798`).
        if ammotype == AMMOTYPE_PISTOL || ammotype == AMMOTYPE_SMG || ammotype == AMMOTYPE_RIFLE {
            dst.push_str(lang.get(l(10)));
        } else if ammotype == AMMOTYPE_KNIFE {
            dst.push_str(lang.get(l(21)));
            dst.push_str(lang.get(l(if qty == 1 { 22 } else { 23 })));
        } else {
            let textnum = match ammotype {
                AMMOTYPE_CROSSBOW => Some(45),
                AMMOTYPE_SHOTGUN => Some(11),
                AMMOTYPE_FARSIGHT => Some(46),
                AMMOTYPE_GRENADE => Some(14),
                AMMOTYPE_ROCKET => Some(16),
                AMMOTYPE_MAGNUM => Some(12),
                AMMOTYPE_DEVASTATOR => Some(15),
                AMMOTYPE_REMOTE_MINE => Some(18),
                AMMOTYPE_PROXY_MINE => Some(19),
                AMMOTYPE_TIMED_MINE => Some(20),
                AMMOTYPE_REAPER => Some(47),
                AMMOTYPE_HOMINGROCKET => Some(17),
                AMMOTYPE_DART => Some(25),
                AMMOTYPE_NBOMB => Some(26),
                AMMOTYPE_SEDATIVE | AMMOTYPE_PSYCHOSIS => Some(27),
                AMMOTYPE_BUG => Some(35),
                AMMOTYPE_MICROCAMERA => Some(36),
                AMMOTYPE_TOKEN => Some(38),
                AMMOTYPE_PLASTIQUE => Some(39),
                AMMOTYPE_CLOAK => Some(48),
                AMMOTYPE_BOOST => Some(49),
                _ => None,
            };
            if let Some(t) = textnum {
                dst.push_str(lang.get(l(t)));
            }
            if qty >= 2 && ammotype != AMMOTYPE_REAPER && ammotype != AMMOTYPE_SEDATIVE && ammotype != AMMOTYPE_CLOAK {
                dst.push_str(lang.get(l(24)));
            }
        }
        dst.push_str(".\n");
        dst
    }
}

impl World {
    /// `current_player_drop_all_items` (`propobj.c:20130`): each weapon player
    /// `pi` holds (one of each; not the fists or anything undroppable) falls
    /// from where the player stands (`weapon_create_for_player_drop`).
    pub(crate) fn current_player_drop_all_items(&mut self, pi: usize) {
        let gset = self.res.gset.clone();
        for w in WEAPON_UNARMED..=WEAPON_SUICIDEPILL {
            let hasmodel = gset.weapon(w).is_some_and(|d| d.tp_model.is_some());
            if hasmodel && self.players[pi].gun.p.inventory.inv_has_single_weapon_exc_all_guns(w) && !gset.has_flag(w, WEAPONFLAG_UNDROPPABLE) {
                self.weapon_create_for_chr_drop(pi, w);
            }
        }
    }

    /// `weapon_create_for_chr(chr, model, weaponnum, OBJFLAG_WEAPON_AICANNOTUSE)`
    /// then `obj_set_dropped(DROPTYPE_DEFAULT)` and `obj_drop(prop, true)`
    /// (`propobj.c:17763`, `:13259`, `:13495`): a weapon object at chr `ci`'s
    /// position, falling with a random push and spin
    /// (`projectile_load_random_speed_rotation`, `projectile.c:25`). It does
    /// not respawn; the chr who dropped it owns it.
    pub(crate) fn weapon_create_for_chr_drop(&mut self, ci: usize, weaponnum: u8) {
        let Some(stem) = self.res.gset.weapon(weaponnum).and_then(|w| w.tp_model.clone()) else { return };
        let Ok(def) = self.res.models.get(&stem) else { return };
        let scale = self.res.models.modelstate_scale(&stem);
        self.props.make_room_for_weapon();
        let id = self.props.alloc_id();
        let mut o = Obj::weapon(id, def, scale, weaponnum, FUNC_PRIMARY, ci);
        o.flags = OBJFLAG_WEAPON_AICANNOTUSE | OBJFLAG_ASSIGNEDTOCHR;
        // obj_set_dropped: in a match with simulants the slot may be reused.
        o.flags3 |= OBJFLAG3_CANHARDFREE;
        // obj_drop (lazy): no collision test, from the root prop's position.
        let x = self.rng.randomfrac() * (10.0 / 6.0) * 4.0 - (10.0 / 3.0);
        let y = self.rng.randomfrac() * (10.0 / 6.0) * 4.0;
        let z = self.rng.randomfrac() * (10.0 / 6.0) * 4.0 - (10.0 / 3.0);
        let mtx = super::projectile_load_random_rotation(&mut self.rng);
        let lvframenum = self.lv.lvframenum;
        o.projectile = Some(super::Projectile { speed: Vec3::new(x, y, z), mtx, ownerprop: Some(ci), flags: PROJECTILEFLAG_AIRBORNE, startframe: lvframenum, ..Default::default() });
        o.pos = self.chrs[ci].pos;
        o.realrot = Mat3::from_diagonal(Vec3::splat(o.scale));
        o.hidden |= OBJHFLAG_SUSPICIOUS;
        self.props.objs.push(o);
    }
}

#[cfg(test)]
mod tests;
