//! The active menu (`activemenu.c`, `activemenutick.c`): hold A and a 3×3
//! grid opens over the view. Screen 0 is the weapons (the inventory placed by
//! `g_AmMapping` and the favourites the weapon set gives), screen 1 the gun's
//! two functions, and with teams on one screen per simulant teammate
//! (`aibuddynums`) with the nine orders of `g_AmBotCommands`; R or L orders all
//! of them at once. The stick, the C-buttons or the D-pad pick a slot, Z
//! applies it and moves to the next screen, and letting go of A applies the
//! focused slot and closes the menu. Attack first asks for a target in the
//! "Pick Target" dialog (the menus' `MENUROOT_PICKTARGET`, [`Event::AmOpenPickTarget`]).
//!
//! The state is the sim's; `am_render` draws it (`pd_render::hud`) from
//! [`World::am_get_slot_details`] and [`ActiveMenu::am_calculate_slot_position`],
//! and its two writes (the selection's destination and `commandingaibot`) are
//! [`World::am_render_sim`], run at the end of the player's pass.
//!
//! `AMMODE_EDIT` is never set in NTSC final, so its branches are left out.
//! The keyboard plays the PC port's way (`activemenutick.c`, `#ifndef
//! PLATFORM_N64`): the mouse moves a virtual stick.

use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::lang::{tx, LANGBANK_MISC, LANGBANK_OPTIONS};
use pd_core::text::{measure, Font};
use n64::pad::*;

use super::PlayerInput;
use crate::bot::botcmd::bot_get_command_name;
use crate::world::World;

pub const AMMODE_CLOSED: u8 = 0;
pub const AMMODE_VIEW: u8 = 1;

/// `AMSLOTFLAG_*`: the current weapon or function (black), an active device
/// (pulsing), no ammo (black, no border).
pub const AMSLOTFLAG_CURRENT: u32 = 0x02;
pub const AMSLOTFLAG_ACTIVE: u32 = 0x08;
pub const AMSLOTFLAG_NOAMMO: u32 = 0x10;

pub const AMSLOTMODE_DEFAULT: u8 = 0;
pub const AMSLOTMODE_FOCUSED: u8 = 1;
pub const AMSLOTMODE_CURRENT: u8 = 2;

/// `DEVICESTATE_*` (`gset_get_device_state`).
pub const DEVICESTATE_UNEQUIPPED: i32 = -1;
pub const DEVICESTATE_INACTIVE: i32 = 0;
pub const DEVICESTATE_ACTIVE: i32 = 1;

/// `g_AmMapping` (`activemenu.c:48`): the weapon set's order (unarmed, then
/// the set's weapons) to `invindexes` places (0-3 the top row and left, 4-7
/// the right and the bottom row; the middle is the screen's title).
pub const G_AM_MAPPING: [u8; 8] = [0, 1, 6, 3, 4, 7, 5, 2];

/// `var800719a0`: a slot's number by row and column.
const SLOTNUMS: [[u8; 3]; 3] = [[0, 1, 2], [3, 4, 5], [6, 7, 8]];

/// The PC port's mouse scale for the virtual stick: `inputMouseGetAbsScaledDelta`
/// (0.022 / 3.5 × the default sensitivity 2.5) × the default `radialmenuspeed` 4.
const MOUSE_TO_STICK: f32 = 0.022 / 3.5 * 2.5 * 4.0;

/// `struct activemenu` (`types.h:4242`), one per player (`g_AmMenus`), plus
/// the PC port's virtual stick.
#[derive(Clone, Debug)]
pub struct ActiveMenu {
    pub screenindex: i32,
    pub xradius: i16,
    pub slotwidth: i16,
    pub selx: i16,
    pub sely: i16,
    pub dstx: i16,
    pub dsty: i16,
    /// 0-8, 4 the middle.
    pub slotnum: u8,
    pub fromslotnum: u8,
    pub cornertimer: i32,
    pub returntimer: i32,
    pub alphafrac: f32,
    pub selpulse: f32,
    /// Inventory indexes by place (0xff: not shown).
    pub invindexes: [u8; 8],
    /// Weapon numbers by place (0xff: no favourite).
    pub favourites: [u8; 8],
    pub togglefunc: bool,
    pub numitems: i32,
    pub allbots: bool,
    pub prevallbots: bool,
    pub origscreennum: i32,
    pub mousex: f32,
    pub mousey: f32,
}

impl Default for ActiveMenu {
    fn default() -> ActiveMenu {
        ActiveMenu {
            screenindex: 0,
            xradius: 0,
            slotwidth: 0,
            selx: 0,
            sely: 0,
            dstx: -123,
            dsty: 0,
            slotnum: 4,
            fromslotnum: 0,
            cornertimer: 0,
            returntimer: 0,
            alphafrac: 0.0,
            selpulse: 0.0,
            invindexes: [0xff; 8],
            favourites: [0xff; 8],
            togglefunc: false,
            numitems: 0,
            allbots: false,
            prevallbots: false,
            origscreennum: 0,
            mousex: 0.0,
            mousey: 0.0,
        }
    }
}

/// The player's screen as the menu places itself on it: `vi_get_view_left`,
/// `_top`, `_width`, `_height`, and the player count.
#[derive(Clone, Copy, Debug)]
pub struct AmView {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub playercount: usize,
    /// Player number (the split screen's odd/even nudge).
    pub playernum: usize,
    /// `options_get_screen_split() == SCREENSPLIT_VERTICAL`.
    pub vsplit: bool,
}

impl ActiveMenu {
    /// `am_is_cramped` (`activemenu.c:771`), NTSC: the weapon screen with 3+
    /// players, or a vertical two-player split (never the 4 MB console).
    pub fn am_is_cramped(&self, v: &AmView) -> bool {
        (self.screenindex == 0 && v.playercount >= 3) || (v.playercount == 2 && v.vsplit)
    }

    /// `am_calculate_slot_position` (`activemenu.c:852`, NTSC final): the
    /// centre of slot (`column`, `row`) on the screen (`g_UiScaleX` 1, lo-res).
    pub fn am_calculate_slot_position(&self, column: i16, row: i16, v: &AmView) -> (i16, i16) {
        let playercount = v.playercount;
        let mut x = self.xradius as i32 * (column as i32 - 1);
        let mut y = (row as i32 - 1) * 50;
        if column != 1 && row != 1 {
            x /= 2;
            y /= 2;
        }
        if self.am_is_cramped(v) {
            let offset = if row == 1 { 3 } else { 1 };
            if column == 0 {
                x = -(self.slotwidth as i32 / 2) - offset;
            } else if column == 2 {
                x = self.slotwidth as i32 / 2 + offset;
            }
        } else if playercount >= 2 && row == 1 {
            x = (x * 6) / 7;
        }
        if playercount >= 2 {
            y = (y * 3) / 5;
        }
        x += v.left + v.width / 2;
        y += v.top + v.height / 2;
        if (playercount == 2 && v.vsplit) || playercount >= 3 {
            x += if v.playernum.is_multiple_of(2) { 8 } else { -8 };
        }
        (x as i16, y as i16)
    }

    /// `g_AmFont1`/`g_AmFont2` (`am_reset`): the small font, the extra small
    /// one with two or more players.
    pub fn am_font(fonts: &pd_core::text::Fonts, playercount: usize) -> &Font {
        if playercount >= 2 {
            &fonts.xs
        } else {
            &fonts.sm
        }
    }
}

/// The N64 buttons the active menu reads for a player (`joy_get_buttons_on_sample`),
/// and its stick. The keyboard's (the PC port's `CONTROLMODE_PC`): Q is A, the
/// left and right mouse buttons Z and R, WASD the C-buttons.
fn am_buttons(input: &PlayerInput) -> u16 {
    let mut b = crate::mp::input_buttons(input);
    if !input.pad {
        for (on, bit) in [(input.walk_y > 0, U_CBUTTONS), (input.walk_y < 0, D_CBUTTONS), (input.walk_x < 0, L_CBUTTONS), (input.walk_x > 0, R_CBUTTONS)] {
            if on {
                b |= bit;
            }
        }
    }
    b
}

impl World {
    /// `mp_reset`'s `g_AmBotCommands` (`mplayer.c:286`): the orders' slots,
    /// by scenario.
    pub(crate) fn am_init_bot_commands(&mut self) {
        let mut c = [AIBOTCMD_NORMAL; 9];
        c[1] = AIBOTCMD_ATTACK;
        c[7] = AIBOTCMD_NORMAL;
        c[0] = AIBOTCMD_FOLLOW;
        c[2] = AIBOTCMD_PROTECT;
        c[3] = AIBOTCMD_DEFEND;
        c[5] = AIBOTCMD_HOLD;
        match self.setup.scenario {
            MPSCENARIO_CAPTURETHECASE => {
                c[3] = AIBOTCMD_GETCASE;
                c[5] = AIBOTCMD_SAVECASE;
            }
            MPSCENARIO_KINGOFTHEHILL => {
                c[6] = AIBOTCMD_DEFHILL;
                c[8] = AIBOTCMD_HOLDHILL;
            }
            MPSCENARIO_HACKERCENTRAL => {
                c[6] = AIBOTCMD_DOWNLOAD;
                c[8] = AIBOTCMD_DOWNLOAD;
            }
            MPSCENARIO_POPACAP => {
                c[6] = AIBOTCMD_POPCAP;
                c[8] = AIBOTCMD_POPCAP;
            }
            MPSCENARIO_HOLDTHEBRIEFCASE => {
                c[6] = AIBOTCMD_GETCASE2;
                c[8] = AIBOTCMD_GETCASE2;
            }
            _ => {}
        }
        self.mp.ambotcommands = c;
    }

    /// `am_reset` (`activemenu.c:517`) for player `pi`: closed, and the
    /// favourites from the weapon set (the shield and empty slots skipped).
    pub(crate) fn am_reset(&mut self, pi: usize) {
        let weapons = self.setup.weapons;
        let p = &mut self.players[pi];
        p.activemenumode = AMMODE_CLOSED;
        let am = &mut p.am;
        am.togglefunc = false;
        am.favourites = [0xff; 8];
        let mut index = 0;
        am.favourites[G_AM_MAPPING[index] as usize] = WEAPON_UNARMED;
        index += 1;
        for j in 0..weapons.len().min(G_AM_MAPPING.len()) {
            let weaponnum = pd_core::mp::mp_weapon(&weapons, j).weaponnum;
            if weaponnum != WEAPON_NONE as i32 && weaponnum != WEAPON_MPSHIELD as i32 && weaponnum != WEAPON_DISABLED as i32 {
                am.favourites[G_AM_MAPPING[index] as usize] = weaponnum as u8;
                index += 1;
            }
        }
    }

    /// `playermgr_calculate_ai_buddy_nums` (`playermgr.c:652`): with teams
    /// on, the simulants on player `pi`'s team, in chr order.
    pub(crate) fn playermgr_calculate_ai_buddy_nums(&mut self, pi: usize) {
        let team = self.chrs[pi].team;
        let buddies: Vec<usize> = (self.players.len()..self.chrs.len()).filter(|&i| self.chrs[i].team == team).collect();
        self.players[pi].aibuddynums = buddies;
    }

    /// `am_assign_weapon_slots` (`activemenu.c:653`): the inventory into the
    /// places, favourites first, then the rest in `g_AmMapping` order.
    fn am_assign_weapon_slots(&mut self, pi: usize) {
        let p = &mut self.players[pi];
        let inv = &p.gun.p.inventory;
        let numitems = inv.inv_get_count();
        let am = &mut p.am;
        am.numitems = numitems;
        am.invindexes = [0xff; 8];
        let isfav = |w: u8| (WEAPON_UNARMED..=WEAPON_DISGUISE41).contains(&w) || w == WEAPON_SUICIDEPILL || w == WEAPON_BACKUPDISK || w == WEAPON_SUITCASE;
        for i in 0..numitems {
            let weaponnum = inv.inv_get_weapon_num_by_index(i);
            if isfav(weaponnum) {
                if let Some(j) = am.favourites.iter().position(|&f| f == weaponnum) {
                    if am.invindexes[j] == 0xff {
                        am.invindexes[j] = i as u8;
                    }
                }
            }
        }
        for i in 0..numitems {
            if am.invindexes.contains(&(i as u8)) {
                continue;
            }
            let weaponnum = inv.inv_get_weapon_num_by_index(i);
            if (WEAPON_UNARMED..=WEAPON_DISGUISE41).contains(&weaponnum) || weaponnum == WEAPON_SUICIDEPILL || weaponnum == WEAPON_SUITCASE {
                // Any mapping not yet used (PD's second search can never find
                // one the first didn't).
                let useindex = G_AM_MAPPING.iter().position(|&m| am.favourites[m as usize] == 0xff).or_else(|| G_AM_MAPPING.iter().position(|&m| am.invindexes[m as usize] == 0xff));
                if let Some(u) = useindex {
                    let m = G_AM_MAPPING[u] as usize;
                    am.invindexes[m] = i as u8;
                    am.favourites[m] = weaponnum;
                }
            }
        }
    }

    /// `am_open` (`activemenu.c:742`): A held past 15 ticks.
    pub(crate) fn am_open(&mut self, pi: usize) {
        // (Passive mode is solo's.)
        self.players[pi].activemenumode = AMMODE_VIEW;
        self.mp.players[pi].withcontrol = false;
        let am = &mut self.players[pi].am;
        am.screenindex = 0;
        am.selpulse = 0.0;
        self.am_assign_weapon_slots(pi);
        self.am_change_screen(pi, 0);
        let am = &mut self.players[pi].am;
        am.xradius = am.slotwidth + 5;
        am.alphafrac = 0.3;
        am.origscreennum = 0;
        am.prevallbots = false;
        am.allbots = false;
    }

    /// `am_close` (`activemenu.c:760`): the focused slot applied, the buttons
    /// held ignored until let go, control back.
    fn am_close(&mut self, pi: usize) {
        let slot = self.players[pi].am.slotnum;
        if slot != 4 {
            self.am_apply(pi, slot);
        }
        self.players[pi].activemenumode = AMMODE_CLOSED;
        self.mp.players[pi].joybutinhibit = 0xffff_ffff;
        self.mp.players[pi].withcontrol = true;
    }

    /// `am_change_screen` (`activemenu.c:602`): `step` screens on (wrapping):
    /// weapons, functions, then with teams on a screen per simulant teammate
    /// (just one while ordering them all).
    fn am_change_screen(&mut self, pi: usize, step: i32) {
        let numaibuddies = self.players[pi].aibuddynums.len() as i32;
        let teams = self.setup.teams_enabled();
        let am = &mut self.players[pi].am;
        am.screenindex += step;
        // PD's @bug: all simulants on a team with none reaches screen 2 (and
        // crashes PD); here that screen orders nobody.
        let maxscreenindex = if teams { if am.allbots { 2 } else { numaibuddies + 1 } } else { 1 };
        if am.screenindex > maxscreenindex {
            am.screenindex = 0;
        }
        if am.screenindex < 0 {
            am.screenindex = maxscreenindex;
        }
        am.xradius = 10;
        am.dstx = -123;
        am.slotnum = 4;
        am.returntimer = 0;
        am.cornertimer = 0;
        am.alphafrac = 0.0;
        self.players[pi].am.slotwidth = self.am_calculate_slot_width(pi);
    }

    /// `am_calculate_slot_width` (`activemenu.c:571`): the widest label on
    /// this screen, plus 3 (split screen) or 4.
    fn am_calculate_slot_width(&self, pi: usize) -> i16 {
        let font = ActiveMenu::am_font(&self.res.fonts, self.players.len());
        let max = (0..9).map(|i| measure(font, &self.am_get_slot_details(pi, i).1, 0).1).max().unwrap_or(0);
        (max + if self.players.len() > 1 { 3 } else { 4 }) as i16
    }

    /// `gset_get_device_state` (`gset.c:330`) for player `pi`.
    pub fn gset_get_device_state(&self, pi: usize, weaponnum: u8) -> i32 {
        let Some(w) = self.res.gset.weapon(weaponnum) else { return DEVICESTATE_UNEQUIPPED };
        for f in w.functions.iter().flatten() {
            if f.kind() == INVENTORYFUNCTYPE_DEVICE {
                return if self.players[pi].devicesactive & f.device == 0 { DEVICESTATE_INACTIVE } else { DEVICESTATE_ACTIVE };
            }
        }
        DEVICESTATE_UNEQUIPPED
    }

    /// `gset_set_device_active` (`gset.c:356`): a vision device turns the
    /// others off.
    pub(crate) fn gset_set_device_active(&mut self, pi: usize, weaponnum: u8, active: bool) {
        let Some(w) = self.res.gset.weapon(weaponnum) else { return };
        let Some(device) = w.functions.iter().flatten().find(|f| f.kind() == INVENTORYFUNCTYPE_DEVICE).map(|f| f.device) else { return };
        let p = &mut self.players[pi];
        if active {
            let vision = DEVICE_NIGHTVISION | DEVICE_XRAYSCANNER | DEVICE_EYESPY | DEVICE_IRSCANNER;
            if device & vision != 0 {
                p.devicesactive &= !vision;
            }
            p.devicesactive |= device;
        } else {
            p.devicesactive &= !device;
        }
    }

    /// `bgun_has_ammo_for_weapon` (`bondgun.c:5558`): true unless every ammo
    /// the weapon's functions take is spent.
    fn bgun_has_ammo_for_weapon(&self, pi: usize, weaponnum: u8) -> bool {
        let Some(w) = self.res.gset.weapon(weaponnum) else { return true };
        let (mut exists, mut has) = (false, false);
        for f in w.functions.iter().flatten() {
            if f.ammoindex >= 0 {
                if let Some(a) = w.ammos.get(f.ammoindex as usize).and_then(|a| a.as_ref()) {
                    exists = true;
                    if self.players[pi].gun.bgun_get_ammo_count(a.ammotype) > 0 {
                        has = true;
                    }
                }
            }
        }
        !exists || has
    }

    /// `am_get_slot_details` (`activemenu.c:413`): slot `slot`'s flags and
    /// label on player `pi`'s current screen.
    pub fn am_get_slot_details(&self, pi: usize, slot: u8) -> (u32, String) {
        let p = &self.players[pi];
        let am = &p.am;
        let lang = &self.res.lang;
        let mut flags = 0;
        match am.screenindex {
            0 => {
                if slot == 4 {
                    return (0, lang.get(tx(LANGBANK_MISC, 170)).to_string()); // "Weapon"
                }
                let slot = if slot > 4 { slot - 1 } else { slot } as usize;
                let inv = &p.gun.p.inventory;
                let invindex = am.invindexes[slot] as i32;
                if self.inv_get_current_index(pi) == invindex {
                    flags |= AMSLOTFLAG_CURRENT;
                }
                let weaponnum = inv.inv_get_weapon_num_by_index(invindex);
                let label = if invindex >= inv.inv_get_count() {
                    String::new()
                } else if weaponnum == WEAPON_CLOAKINGDEVICE {
                    // "Cloak %d": the seconds left, rounded up.
                    let qty = p.gun.bgun_get_ammo_count(AMMOTYPE_CLOAK);
                    let secs = qty / 60;
                    let modulo = (qty - secs * 60) * 100 / 60;
                    lang.get(tx(LANGBANK_OPTIONS, 491)).replacen("%d", &(secs + (modulo > 0) as i32).to_string(), 1)
                } else {
                    self.res.gset.weapon(weaponnum).map_or(String::new(), |w| format!("{}\n", w.short_name.trim_end()))
                };
                if self.gset_get_device_state(pi, weaponnum) == DEVICESTATE_ACTIVE {
                    flags |= AMSLOTFLAG_ACTIVE;
                }
                if !self.bgun_has_ammo_for_weapon(pi, weaponnum) {
                    flags |= AMSLOTFLAG_NOAMMO;
                }
                (flags, label)
            }
            1 => {
                if slot == 4 {
                    return (0, lang.get(tx(LANGBANK_MISC, 171)).to_string()); // "Function"
                }
                if slot != 1 && slot != 7 {
                    return (0, String::new());
                }
                let w = p.gun.hands[HAND_RIGHT].weaponnum;
                let pri = self.res.gset.func(w, FUNC_PRIMARY);
                let sec = self.res.gset.func(w, FUNC_SECONDARY);
                let funcissec = p.gun.funcissec();
                let (mine, current) = if slot == 1 { (pri, sec.is_none() || !funcissec) } else { (sec, pri.is_none() || funcissec) };
                if current {
                    flags |= AMSLOTFLAG_CURRENT;
                }
                (flags, mine.map_or(String::new(), |f| format!("{}\n", f.name.trim_end())))
            }
            _ => {
                if slot == 4 {
                    return (0, lang.get(tx(LANGBANK_MISC, 172)).to_string()); // "Orders"
                }
                (0, lang.get(bot_get_command_name(self.mp.ambotcommands[slot as usize])).to_string())
            }
        }
    }

    /// `am_apply` (`activemenu.c:304`): what slot `slot` does on player `pi`'s
    /// screen: equip a weapon (both hands if held twice) or switch a device,
    /// choose a function, or order the simulant(s).
    fn am_apply(&mut self, pi: usize, slot: u8) {
        match self.players[pi].am.screenindex {
            0 => {
                let slot = if slot > 4 { slot - 1 } else { slot } as usize;
                let invindex = self.players[pi].am.invindexes[slot] as i32;
                let inv = &self.players[pi].gun.p.inventory;
                if invindex >= inv.inv_get_count() {
                    return;
                }
                let weaponnum = inv.inv_get_weapon_num_by_index(invindex);
                if weaponnum != 0 {
                    let state = self.gset_get_device_state(pi, weaponnum);
                    if state != DEVICESTATE_UNEQUIPPED {
                        self.gset_set_device_active(pi, weaponnum, state == DEVICESTATE_INACTIVE);
                        return;
                    }
                }
                // (The firing range's weapon check is solo's.)
                let g = &mut self.players[pi].gun;
                g.p.inventory.equipcuritem = invindex;
                g.select_weapon(weaponnum, true);
            }
            1 => {
                let g = &self.players[pi].gun;
                let w = g.ctrl.weaponnum;
                let sec = (WEAPON_UNARMED..=WEAPON_COMBATBOOST).contains(&w) && g.funcissec();
                // Slot 1 is the primary: toggle if on the secondary, and the
                // other way round.
                if (sec && slot == 1) || (!sec && slot != 1) {
                    self.players[pi].am.togglefunc = true;
                }
            }
            s => {
                let command = self.mp.ambotcommands[slot as usize];
                let bots: Vec<usize> = if self.players[pi].am.allbots {
                    self.players[pi].aibuddynums.clone()
                } else {
                    self.players[pi].aibuddynums.get(s as usize - 2).copied().into_iter().collect()
                };
                for b in bots {
                    if self.botcmd_apply(b, command, pi) {
                        self.am_open_pick_target(pi);
                    }
                }
            }
        }
    }

    /// `am_open_pick_target` (`activemenu.c:64`): the active menu gives way to
    /// the "Pick Target" dialog (control stays off until the dialog closes).
    fn am_open_pick_target(&mut self, pi: usize) {
        if self.mp_is_paused() {
            return;
        }
        let p = &mut self.players[pi];
        if p.activemenumode == AMMODE_CLOSED {
            // Already handed over (every simulant asks when ordering all).
            return;
        }
        p.am.prevallbots = p.am.allbots;
        p.activemenumode = AMMODE_CLOSED;
        let targets = self.am_pick_target_list(pi);
        self.push_event(Event::AmOpenPickTarget { player: pi as u8, targets });
    }

    /// `am_pick_target_menu_list`'s rows (`activemenu.c:91`), as (chr index,
    /// chr slot): for all simulants, the player itself and every chr on
    /// another team (PD counts the other team's simulants and every human, the
    /// player included); for one, everyone but that simulant.
    fn am_pick_target_list(&self, pi: usize) -> Vec<(u8, u8)> {
        let p = &self.players[pi];
        let bot = p.aibuddynums.get((p.am.screenindex - 2).max(0) as usize).copied();
        let team = self.chrs[pi].team;
        let rows: Vec<usize> = if p.am.prevallbots {
            let count = (self.players.len()..self.chrs.len()).filter(|&i| self.chrs[i].team != team).count() + self.players.len();
            (0..self.chrs.len()).filter(|&i| i == pi || self.chrs[i].team != team).take(count).collect()
        } else {
            (0..self.chrs.len()).filter(|&i| Some(i) != bot).collect()
        };
        rows.into_iter().map(|i| (i as u8, self.chrs[i].mpslot as u8)).collect()
    }

    /// The Pick Target dialog's confirm (`am_pick_target_menu_list`'s
    /// `MENUOP_CONFIRM`): the simulant, or all of player `pi`'s, attack chr
    /// `target`.
    pub fn am_pick_target(&mut self, pi: usize, target: usize) {
        if target >= self.chrs.len() || pi >= self.players.len() {
            return;
        }
        let p = &mut self.players[pi];
        let bots: Vec<usize> = if std::mem::take(&mut p.am.prevallbots) {
            p.aibuddynums.clone()
        } else {
            p.aibuddynums.get((p.am.screenindex - 2).max(0) as usize).copied().into_iter().collect()
        };
        for b in bots {
            self.bot_apply_attack(b, target);
        }
    }

    /// `am_tick` (`activemenutick.c:16`) for every player, from `lv_tick`,
    /// reading the players' raw controls (a closed menu's player may have no
    /// control).
    pub(crate) fn am_tick(&mut self, inputs: &[PlayerInput]) {
        let idle = PlayerInput::default();
        for pi in 0..self.players.len() {
            let input = inputs.get(pi).unwrap_or(&idle);
            if self.players[pi].am.togglefunc {
                let res = self.res.clone();
                let p = &mut self.players[pi];
                if p.gun_ctx(&res, &mut self.rng, &self.lv).bgun_consider_toggle_gun_function(60, false, true) > 0 {
                    self.players[pi].am.togglefunc = false;
                }
            }
            // (Solo re-assigns the slots as the inventory changes.)
            let held = am_buttons(input);
            let pressed = held & !self.players[pi].am_prevbuttons;
            self.players[pi].am_prevbuttons = held;
            if self.players[pi].activemenumode != AMMODE_CLOSED {
                // One joy sample per 60 Hz tick.
                for sample in 0..self.lv.lvupdate60.max(1) {
                    let pressed = if sample == 0 { pressed } else { 0 };
                    if !self.am_tick_sample(pi, input, held, pressed, sample == 0) {
                        break;
                    }
                }
            } else {
                let am = &mut self.players[pi].am;
                am.mousex = 0.0;
                am.mousey = 0.0;
            }
            let freal = self.lv.lvupdate60freal;
            let am = &mut self.players[pi].am;
            if am.dstx != -123 {
                am.selx = ((am.selx as i32 + am.dstx as i32) / 2) as i16;
                am.sely = ((am.sely as i32 + am.dsty as i32) / 2) as i16;
                if (am.selx - am.dstx).abs() <= 1 {
                    am.selx = am.dstx;
                }
                if (am.sely - am.dsty).abs() <= 1 {
                    am.sely = am.dsty;
                }
            }
            // The expanding effect of a new screen.
            let dstradius = am.slotwidth + 5;
            am.xradius = ((am.xradius as i32 * 3 + dstradius as i32) / 4) as i16;
            if (am.xradius - dstradius).abs() <= 1 {
                am.xradius = dstradius;
            }
            if am.alphafrac < 1.0 {
                am.alphafrac += freal / 30.0;
            }
            if am.alphafrac > 1.0 {
                am.alphafrac = 1.0;
            }
            am.selpulse += freal / 5.0;
            if am.selpulse > 18.849_556 {
                am.selpulse -= 18.849_556;
            }
        }
    }

    /// One joy sample of an open menu (`activemenutick.c:43`); false once it
    /// closed.
    fn am_tick_sample(&mut self, pi: usize, input: &PlayerInput, buttonsstate: u16, buttonspressed: u16, first: bool) -> bool {
        let (mut column, mut row) = (1i32, 1i32);
        let mut stayopen = false;
        let mut toggle = false;
        let mut gotonextscreen = false;
        let mut stickpushed = false;
        let (mut cstickx, mut csticky) = (input.look_x, input.look_y);
        // The keyboard's virtual stick (the PC port's first sample only).
        if first && !input.pad {
            let am = &mut self.players[pi].am;
            if input.mouse_dx != 0.0 || input.mouse_dy != 0.0 {
                am.mousex = (am.mousex + input.mouse_dx * MOUSE_TO_STICK).clamp(-128.0, 127.0);
                am.mousey = (am.mousey + input.mouse_dy * MOUSE_TO_STICK).clamp(-128.0, 127.0);
            }
            cstickx = (cstickx + am.mousex as i32).clamp(-128, 127);
            csticky = (csticky - am.mousey as i32).clamp(-128, 127);
        }
        self.players[pi].am.allbots = false;
        // Control style 1.1 (not 1.3 or 1.4): A keeps it open, R or L orders
        // all simulants.
        if buttonsstate & A_BUTTON != 0 {
            stayopen = true;
        }
        if buttonsstate & (R_TRIG | L_TRIG) != 0 {
            self.players[pi].am.allbots = true;
        }
        {
            let am = &mut self.players[pi].am;
            if am.allbots && am.screenindex >= 2 && am.origscreennum == 0 {
                am.origscreennum = am.screenindex;
                am.screenindex = 2;
                self.am_change_screen(pi, 0);
            }
            let am = &mut self.players[pi].am;
            if !am.allbots && am.origscreennum != 0 {
                am.screenindex = am.origscreennum;
                am.origscreennum = 0;
                self.am_change_screen(pi, 0);
            }
        }
        for (bits, r, c) in [(U_CBUTTONS | U_JPAD, Some(0), None), (D_CBUTTONS | D_JPAD, Some(2), None), (L_CBUTTONS | L_JPAD, None, Some(0)), (R_CBUTTONS | R_JPAD, None, Some(2))] {
            if buttonsstate & bits != 0 {
                if let Some(r) = r {
                    row = r;
                }
                if let Some(c) = c {
                    column = c;
                }
            }
        }
        let (absx, absy) = (cstickx.abs(), csticky.abs());
        if absx > 20 || absy > 20 {
            stickpushed = true;
            if (absy as f32) / (absx as f32) < 0.268 {
                column = if cstickx < 0 { 0 } else { 2 };
                row = 1;
            } else if (absx as f32) / (absy as f32) < 0.268 {
                column = 1;
                row = if csticky < 0 { 2 } else { 0 };
            } else {
                column = if cstickx < 0 { 0 } else { 2 };
                row = if csticky < 0 { 2 } else { 0 };
            }
        }
        if self.players[pi].isdead || self.lv.lvupdate240 == 0 {
            stayopen = false;
        }
        if !stayopen {
            self.am_close(pi);
            return false;
        }
        if buttonspressed & Z_TRIG != 0 {
            toggle = true;
        }
        if toggle {
            let am = &self.players[pi].am;
            let slot = am.slotnum;
            if am.screenindex >= 2 {
                // An order screen in a match.
                if self.mp.ambotcommands[slot as usize] == AIBOTCMD_ATTACK {
                    self.am_open_pick_target(pi);
                } else if !self.players[pi].am.allbots {
                    gotonextscreen = true;
                }
                if slot != 4 {
                    self.am_apply(pi, slot);
                }
            } else if slot == 4 {
                gotonextscreen = true;
            } else {
                self.am_apply(pi, slot);
            }
        }
        if self.players[pi].activemenumode == AMMODE_CLOSED {
            // Handed over to Pick Target.
            return false;
        }
        if gotonextscreen {
            self.am_change_screen(pi, 1);
            // A weapon with no functions skips its function screen.
            let w = self.players[pi].gun.hands[HAND_RIGHT].weaponnum;
            if self.players[pi].am.screenindex == 1 && self.res.gset.func(w, FUNC_PRIMARY).is_none() && self.res.gset.func(w, FUNC_SECONDARY).is_none() {
                self.am_change_screen(pi, 1);
            }
        }
        let slotnum = (column + row * 3) as u8;
        if slotnum == 4 {
            let am = &mut self.players[pi].am;
            if am.returntimer <= 0 {
                am.returntimer = 0;
                am.slotnum = slotnum;
            } else {
                am.returntimer -= 1;
            }
        } else {
            let mut gotoslot = !self.am_get_slot_details(pi, slotnum).1.is_empty();
            let am = &mut self.players[pi].am;
            // A corner reached with the buttons: a grace of two samples for
            // letting go of both, so it doesn't slip to a neighbour.
            if am.slotnum != 4 && !stickpushed && matches!(am.slotnum, 0 | 2 | 6 | 8) {
                if slotnum != am.fromslotnum {
                    am.cornertimer = 2;
                    am.fromslotnum = slotnum;
                    gotoslot = false;
                }
                if am.cornertimer > 0 && gotoslot {
                    gotoslot = false;
                    am.cornertimer -= 1;
                }
            }
            if gotoslot {
                am.returntimer = 15;
                am.slotnum = slotnum;
            }
        }
        true
    }

    /// `am_render`'s writes (`activemenu.c:1264`), at the end of player `pi`'s
    /// pass: the selection's destination (its first position, at once) and
    /// the simulant being ordered (`commandingaibot`, which pulses).
    pub(crate) fn am_render_sim(&mut self, pi: usize) {
        let view = self.am_view(pi);
        let p = &mut self.players[pi];
        p.commandingaibot = None;
        if p.activemenumode == AMMODE_CLOSED {
            return;
        }
        let am = &mut p.am;
        let (x, y) = am.am_calculate_slot_position((am.slotnum % 3) as i16, (am.slotnum / 3) as i16, &view);
        if am.dstx == -123 {
            am.selx = x;
            am.sely = y;
        }
        am.dstx = x;
        am.dsty = y;
        if am.screenindex >= 2 && !am.allbots {
            p.commandingaibot = p.aibuddynums.get(am.screenindex as usize - 2).copied();
        }
    }

    /// Player `pi`'s screen for the menu's layout.
    pub fn am_view(&self, pi: usize) -> AmView {
        let c = &self.players[pi].cam;
        AmView {
            left: c.c_screenleft as i32,
            top: c.c_screentop as i32,
            width: c.c_screenwidth as i32,
            height: c.c_screenheight as i32,
            playercount: self.players.len(),
            playernum: pi,
            // SUBST: PD reads the split option (`options_get_screen_split`) /
            // horizontal until split screen (M12).
            vsplit: false,
        }
    }

    /// The order slot a simulant is carrying out, for the screen's "current"
    /// marks (`am_render`): simulant `bot`'s `aibot->command`.
    pub fn am_bot_command(&self, bot: usize) -> Option<u8> {
        self.chrs.get(bot).and_then(|c| c.aibot.as_ref()).map(|a| a.command)
    }

    /// What `am_render` (`activemenu.c:1243`) draws for player `pi`, read by
    /// `pd_render::hud`: the open menu, and the ordered simulant's bars.
    pub fn am_render_in(&self, pi: usize) -> (Option<AmRender>, Option<AmBars>) {
        let p = &self.players[pi];
        let bars = p.commandingaibot.map(|b| {
            let c = &self.chrs[b];
            AmBars { healthfrac: (c.maxdamage - c.damage) / c.maxdamage, shieldfrac: c.cshield * 0.125 }
        });
        if p.activemenumode == AMMODE_CLOSED {
            return (None, bars);
        }
        let am = &p.am;
        let bot = if am.screenindex >= 2 { p.aibuddynums.get(am.screenindex as usize - 2).copied() } else { None };
        let slots = (0..9u8)
            .map(|slot| {
                let (flags, label) = self.am_get_slot_details(pi, slot);
                let mut mode = if slot == am.slotnum { AMSLOTMODE_FOCUSED } else { AMSLOTMODE_DEFAULT };
                // An order screen marks the order the simulant is carrying out.
                // (PD reads a missing simulant's aibot and crashes; none here.)
                if mode == AMSLOTMODE_DEFAULT && am.screenindex >= 2 && bot.and_then(|b| self.am_bot_command(b)) == Some(self.mp.ambotcommands[slot as usize]) {
                    mode = AMSLOTMODE_CURRENT;
                }
                AmSlot { flags, label, mode }
            })
            .collect();
        let lang = &self.res.lang;
        let info = (am.screenindex >= 2).then(|| {
            if am.allbots {
                AmInfo { title: lang.get(tx(LANGBANK_MISC, 215)).to_string(), weapon: None } // "All Simulants"
            } else {
                let b = bot.unwrap_or(0);
                let weaponnum = self.chrs.get(b).and_then(|c| c.aibot.as_ref()).map_or(0, |a| a.weaponnum);
                let weapon = if !(WEAPON_FALCON2..=WEAPON_HORIZONSCANNER).contains(&weaponnum) {
                    lang.get(tx(LANGBANK_MISC, 173)).to_string() // "No Weapon"
                } else {
                    self.res.gset.weapon(weaponnum).map_or(String::new(), |w| format!("{}
", w.short_name.trim_end()))
                };
                AmInfo { title: self.mp_chr_name(b), weapon: Some(weapon) }
            }
        });
        (Some(AmRender { am: am.clone(), view: self.am_view(pi), slots, info }), bars)
    }

    /// `SLOTNUMS[row][column]`.
    pub fn am_slotnum(row: usize, column: usize) -> u8 {
        SLOTNUMS[row][column]
    }
}

/// One slot as `am_render` draws it.
#[derive(Clone, Debug)]
pub struct AmSlot {
    pub flags: u32,
    pub label: String,
    pub mode: u8,
}

/// An order screen's heading (`am_render_aibot_info`): the simulant's name
/// and its weapon, or "All Simulants".
#[derive(Clone, Debug)]
pub struct AmInfo {
    pub title: String,
    pub weapon: Option<String>,
}

/// The open menu for the renderer.
#[derive(Clone, Debug)]
pub struct AmRender {
    pub am: ActiveMenu,
    pub view: AmView,
    /// By slot number, 0-8.
    pub slots: Vec<AmSlot>,
    pub info: Option<AmInfo>,
}

/// The ordered simulant's health and shield (fractions).
#[derive(Clone, Copy, Debug)]
pub struct AmBars {
    pub healthfrac: f32,
    pub shieldfrac: f32,
}

#[cfg(test)]
mod tests;
