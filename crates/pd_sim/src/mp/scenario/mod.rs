//! The scenarios (`mplayer/scenarios.c`): the dispatch through `g_MpScenarios`
//! and the code they share, with each scenario's callbacks in its own file as
//! PD has them in `scenarios/*.inc`: Hold the Briefcase ([`htb`]), Hacker
//! Central ([`htm`]), Pop a Cap ([`pac`]), King of the Hill ([`koh`]) and
//! Capture the Case ([`ctc`]). Combat has no callbacks.
//!
//! PD keeps the scenario's state in `g_ScenarioData`, a union of the five
//! halves; only the running scenario's is ever read, so [`ScenarioData`]
//! holds them side by side.
//!
//! When PD calls a scenario (the frame order, `lv.c`, `setup.c`):
//! `scenario_reset` at the stage's load, before the props; `scenario_init_props`
//! after the setup's objects and the simulants' allocation, before the
//! players spawn; `scenario_tick` in `lv_tick` before `props_tick`;
//! `scenario_tick_chr(NULL)` in each player's `lv_render` pass after
//! `props_tick_player`, and `scenario_tick_chr(chr)` in each simulant's
//! `bot_tick` after `chr_tick`; `scenario_render_hud` after the player's HUD
//! (the drawing is `pd_render`'s, from [`World::scenario_hud`]).
//!
//! The radar's scenario parts (`scenario_radar_extra`, `scenario_radar_chr`)
//! are in [`crate::mp::radar`].
//!
//! Source: `reference/pd-decomp/src/game/mplayer/scenarios.c` and
//! `scenarios/*.inc` (NTSC final). Not in any spike.

pub mod ctc;
pub mod htb;
pub mod htm;
pub mod koh;
pub mod pac;
#[cfg(test)]
mod tests;

use glam::{Mat3, Vec3};
use pd_core::ids::*;
use pd_core::lang::{tx, LANGBANK_MPWEAPONS};
use pd_core::mp::{MatchSetup, ScenarioScores, MAX_MPCHRS};

use crate::props::Obj;
use crate::world::World;

/// A `struct prop *` a scenario keeps: an object (a case or the uplink lying
/// somewhere, the terminal) or a chr (a player's or a simulant's, by chr index:
/// `PROPTYPE_PLAYER` / `PROPTYPE_CHR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropRef {
    Obj(u32),
    Chr(usize),
}

impl PropRef {
    /// `prop->type == PROPTYPE_CHR || prop->type == PROPTYPE_PLAYER`.
    pub fn chr(self) -> Option<usize> {
        match self {
            PropRef::Chr(c) => Some(c),
            PropRef::Obj(_) => None,
        }
    }

    pub fn obj(self) -> Option<u32> {
        match self {
            PropRef::Obj(id) => Some(id),
            PropRef::Chr(_) => None,
        }
    }
}

/// `g_ScenarioData` (`types.h:4196`), every half.
#[derive(Clone, Debug, Default)]
pub struct ScenarioData {
    pub htb: htb::Htb,
    pub htm: htm::Htm,
    pub pac: pac::Pac,
    pub koh: koh::Koh,
    pub ctc: ctc::Ctc,
    /// `var800869ec` (`holdthebriefcase.inc:121`): the ammo crate the case or
    /// the uplink took the place of, kept gone while the token lies there.
    pub replacedcrate: Option<u32>,
}

/// An `L_MPWEAPONS_*` string.
fn l(n: u16) -> pd_core::lang::Tx {
    tx(LANGBANK_MPWEAPONS, n)
}

/// The weapons objects the scenarios drop in (`htb_create_token`'s template,
/// `holdthebriefcase.inc:184`, and its twins): falling, invincible, never
/// bouncing, immune to gunfire and explosions.
const TOKEN_FLAGS: u32 = OBJFLAG_FALL | OBJFLAG_INVINCIBLE | OBJFLAG_FORCENOBOUNCE;
const TOKEN_FLAGS2: u32 = OBJFLAG2_IMMUNETOGUNFIRE | OBJFLAG2_IMMUNETOEXPLOSIONS;

/// `scenario_init` (`scenarios.c:449`)'s `g_MpSetup` half, on a setup that
/// did not come through the menus' (a test's or a probe's): King of the Hill
/// and Capture the Case force teams on, Capture the Case folds the teams into
/// four (`koh_init`, `ctc_init`). The menus do the same when the scenario is
/// chosen (`pd_menu::MenuSystem::scenario_init`).
pub fn scenario_init_setup(setup: &mut MatchSetup) {
    if setup.scenario == MPSCENARIO_KINGOFTHEHILL || setup.scenario == MPSCENARIO_CAPTURETHECASE {
        setup.options |= MPOPTION_TEAMSENABLED;
    }
    if setup.scenario == MPSCENARIO_CAPTURETHECASE {
        let max = ctc::ctc_get_max_teams() as u8;
        for c in setup.players.iter_mut().map(|p| &mut p.chr).chain(setup.simulants.iter_mut().map(|s| &mut s.chr)) {
            while c.team >= max {
                c.team -= max;
            }
        }
    }
}

impl World {
    /// `scenario_reset` (`scenarios.c:813`), at the stage's load: the
    /// scenario's stage-specific state cleared, then the setup's `intro[]`
    /// read again for the case pads and the hills.
    pub(crate) fn scenario_reset(&mut self) {
        match self.setup.scenario {
            MPSCENARIO_KINGOFTHEHILL => self.mp.scenariodata.koh.hillcount = 0,
            MPSCENARIO_CAPTURETHECASE => self.mp.scenariodata.ctc.reset(),
            MPSCENARIO_HACKERCENTRAL => self.mp.scenariodata.htm.htm_reset(),
            MPSCENARIO_HOLDTHEBRIEFCASE => self.mp.scenariodata.htb.htb_reset(),
            _ => {}
        }
        let intro = self.stage.intro.clone();
        for cmd in &intro {
            let ty = cmd.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let int = |k: &str| cmd.get(k).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            match ty {
                "case" | "case_respawn" => match self.setup.scenario {
                    MPSCENARIO_CAPTURETHECASE => self.mp.scenariodata.ctc.ctc_add_pad(ty == "case", int("id"), int("pad")),
                    MPSCENARIO_HACKERCENTRAL => self.mp.scenariodata.htm.htm_add_pad(int("pad") as i16),
                    MPSCENARIO_HOLDTHEBRIEFCASE => self.mp.scenariodata.htb.htb_add_pad(int("pad") as i16),
                    _ => {}
                },
                "hill" if self.setup.scenario == MPSCENARIO_KINGOFTHEHILL => self.mp.scenariodata.koh.koh_add_hill(int("pad") as i16),
                _ => {}
            }
        }
    }

    /// `scenario_init_props` (`scenarios.c:474`): the scenario's own props
    /// (the cases, the uplink, the terminal, the hill's light), from
    /// `setup_create_props` (`setup.c:1997`).
    pub(crate) fn scenario_init_props(&mut self) {
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE => self.htb_init_props(),
            MPSCENARIO_HACKERCENTRAL => self.htm_init_props(),
            MPSCENARIO_POPACAP => self.pac_init_props(),
            MPSCENARIO_KINGOFTHEHILL => self.koh_init_props(),
            MPSCENARIO_CAPTURETHECASE => self.ctc_init_props(),
            _ => {}
        }
    }

    /// The scenario's `tickfunc` (`scenario_tick`, `scenarios.c:528`), after
    /// the match-start messages.
    pub(crate) fn scenario_tick_callback(&mut self) {
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE => self.htb_tick(),
            MPSCENARIO_HACKERCENTRAL => self.htm_tick(),
            MPSCENARIO_POPACAP => self.pac_tick(),
            MPSCENARIO_KINGOFTHEHILL => self.koh_tick(),
            _ => {}
        }
    }

    /// `scenario_tick_chr` (`scenarios.c:547`): simulant `chr`, or with `None`
    /// the current player `pi` (its `lv_render` pass).
    pub(crate) fn scenario_tick_chr(&mut self, chr: Option<usize>, pi: usize) {
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE => self.htb_tick_chr(chr, pi),
            MPSCENARIO_HACKERCENTRAL => self.htm_tick_chr(chr, pi),
            _ => {}
        }
    }

    /// `scenario_create_hudmsg` (`scenarios.c:1038`): a scenario message to
    /// player `playernum` (a chr index; a simulant's is ignored).
    pub(crate) fn scenario_create_hudmsg(&mut self, playernum: i32, message: &str) {
        if playernum >= 0 && (playernum as usize) < self.players.len() {
            self.hudmsg_create_with_flags(playernum as usize, message, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
        }
    }

    /// `scenario_chrs_are_same_team` (`scenarios.c:1058`), chr indexes.
    pub(crate) fn scenario_chrs_are_same_team(&self, a: i32, b: i32) -> bool {
        if self.setup.teams_enabled() && a >= 0 && b >= 0 {
            if let (Some(sa), Some(sb)) = (self.mp_chrindex_to_chrslot(a as usize), self.mp_chrindex_to_chrslot(b as usize)) {
                return self.mp.teams[sa] == self.mp.teams[sb];
            }
        }
        false
    }

    /// `mpchr->name` of chr `i` (the mpchrconfig's, with its line break).
    pub(crate) fn scenario_chr_name(&self, i: usize) -> String {
        self.mp_chr_name(i)
    }

    /// `bgun_get_short_name(weaponnum)` (the lang string, with its line break).
    fn bgun_get_short_name(&self, weaponnum: u8) -> String {
        self.res.gset.weapon(weaponnum).map_or(String::new(), |w| format!("{}\n", w.short_name.trim_end()))
    }

    /// `inv_has_briefcase` (`inv.c:753`): a living player holding the briefcase.
    pub fn inv_has_briefcase(&self, pi: usize) -> bool {
        let p = &self.players[pi];
        !p.isdead && p.gun.p.inventory.inv_has_single_weapon_exc_all_guns(WEAPON_BRIEFCASE2)
    }

    /// `inv_has_data_uplink` (`inv.c:762`).
    pub fn inv_has_data_uplink(&self, pi: usize) -> bool {
        let p = &self.players[pi];
        !p.isdead && p.gun.p.inventory.inv_has_single_weapon_exc_all_guns(WEAPON_DATAUPLINK)
    }

    /// A scenario weapon object before placing (`htb_create_token`'s
    /// template): `weaponnum` on `MODEL_*` `modelnum`.
    fn scenario_weapon_obj(&mut self, modelnum: i32, weaponnum: u8) -> Option<Obj> {
        let mut o = self.setup_obj(modelnum, OBJTYPE_WEAPON, TOKEN_FLAGS, TOKEN_FLAGS2, 0, 1000)?;
        o.weaponnum = weaponnum;
        o.gunfunc = FUNC_PRIMARY;
        o.timer240 = -1;
        Some(o)
    }

    /// `setup_place_weapon(weapon, cmdindex)` on the weapon's pad
    /// (`setup_create_object`) for a scenario token, then `OBJH2FLAG_CANREGEN`
    /// cleared (the token never respawns). The object's id.
    fn scenario_place_token(&mut self, modelnum: i32, weaponnum: u8, pad: i32, extrascale: i32) -> Option<u32> {
        let o = self.scenario_weapon_obj(modelnum, weaponnum)?;
        let id = o.id;
        let before = self.props.objs.len();
        self.setup_create_object(o, pad, extrascale);
        if self.props.objs.len() == before {
            return None;
        }
        let o = self.props.get_mut(id)?;
        o.hidden2 &= !OBJH2FLAG_CANREGEN;
        Some(id)
    }

    /// `obj_init`'s scenario half (`propobj.c:2134`) for a weapon object the
    /// game makes (a death's drop): a briefcase or an uplink is invincible and
    /// never bounces, and in its own scenario becomes the token.
    pub(crate) fn scenario_obj_init(&mut self, id: u32) {
        let Some(o) = self.props.get_mut(id) else { return };
        if o.ty != OBJTYPE_WEAPON || !matches!(o.weaponnum, WEAPON_BRIEFCASE2 | WEAPON_DATAUPLINK) {
            return;
        }
        o.flags |= OBJFLAG_INVINCIBLE | OBJFLAG_FORCENOBOUNCE;
        o.flags2 |= TOKEN_FLAGS2;
        if o.weaponnum == WEAPON_BRIEFCASE2 && self.setup.scenario == MPSCENARIO_HOLDTHEBRIEFCASE {
            self.mp.scenariodata.htb.token = Some(PropRef::Obj(id));
        } else if o.weaponnum == WEAPON_DATAUPLINK && self.setup.scenario == MPSCENARIO_HACKERCENTRAL {
            self.mp.scenariodata.htm.uplink = Some(PropRef::Obj(id));
        }
    }

    /// The ammo crates in the active props list, newest first
    /// (`g_Vars.activeprops`: `prop_activate` puts a prop at the head), at
    /// most 20 (`htb_create_token`'s candidates). A taken crate waiting to
    /// respawn is still in the list.
    fn scenario_crate_candidates(&self) -> Vec<u32> {
        self.props.objs.iter().rev().filter(|o| o.ty == OBJTYPE_MULTIAMMOCRATE).take(20).map(|o| o.id).collect()
    }

    /// Replace a random ammo crate (`htb_create_token`, `htb_create_uplink`):
    /// it goes (`OBJHFLAG_DELETING`, respawning: `OBJH2FLAG_CANREGEN`) and its
    /// pad is returned.
    fn scenario_replace_crate(&mut self) -> Option<i32> {
        let candidates = self.scenario_crate_candidates();
        if candidates.is_empty() {
            return None;
        }
        let k = (self.rng.random() % candidates.len() as u32) as usize;
        let id = candidates[k];
        self.mp.scenariodata.replacedcrate = Some(id);
        let o = self.props.get_mut(id)?;
        o.hidden |= OBJHFLAG_DELETING;
        o.hidden2 |= OBJH2FLAG_CANREGEN;
        Some(o.pad)
    }

    /// `htb_tick`'s and `htm_tick`'s first step: while the token lies where
    /// the crate was, the crate stays gone (its respawn wait held at 20 s);
    /// once the token is anywhere else, the crate is let go.
    fn scenario_hold_replaced_crate(&mut self, token: Option<PropRef>) {
        let Some(id) = self.mp.scenariodata.replacedcrate else { return };
        let Some(o) = self.props.get_mut(id) else { return };
        if token.is_none_or(|t| t.obj().is_none()) {
            self.mp.scenariodata.replacedcrate = None;
        } else {
            o.timetoregen = crate::props::pickup::REGEN_TIME60;
        }
    }

    /// Where a scenario token is: an object's position or a chr's.
    pub fn scenario_prop_pos(&self, p: PropRef) -> Option<Vec3> {
        match p {
            PropRef::Obj(id) => self.props.get(id).map(|o| o.pos),
            PropRef::Chr(c) => self.chrs.get(c).map(|c| c.pos),
        }
    }

    /// `scenario_pick_up_briefcase` (`scenarios.c:1084`): chr `ci` (a player's
    /// chr is its player's) takes briefcase object `id`. Returns the tick
    /// operation (`TICKOP_FREE`: the object is gone).
    pub(crate) fn scenario_pick_up_briefcase(&mut self, ci: usize, id: u32) -> i32 {
        let isbot = self.chrs[ci].aibot.is_some();
        let Some(o) = self.props.get(id).cloned() else { return TICKOP_NONE };
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE => {
                self.mp.scenariodata.htb.token = Some(PropRef::Chr(ci));
                if isbot {
                    // prop_play_pickup_sound(prop, weaponnum).
                    self.sound_at(crate::props::pickup::weapon_pickup_sound(o.weaponnum), 1.0, o.pos, crate::propsnd::DEFAULT_DISTS);
                    let a = self.ab_mut(ci);
                    a.hasbriefcase = true;
                    a.botinv_give_single_weapon(WEAPON_BRIEFCASE2);
                } else {
                    self.players[ci].gun.p.inventory.inv_give_single_weapon(WEAPON_BRIEFCASE2);
                    self.current_player_queue_pickup_weapon_hudmsg(ci, WEAPON_BRIEFCASE2, false);
                    self.sound(crate::props::pickup::weapon_pickup_sound(WEAPON_BRIEFCASE2), 1.0);
                }
                // "%shas the\n%s"
                let text1 = self.res.lang.get(l(0)).replacen("%s", &self.scenario_chr_name(ci), 1).replacen("%s", &self.bgun_get_short_name(WEAPON_BRIEFCASE2), 1);
                for i in 0..self.players.len() {
                    if isbot || i != ci {
                        self.hudmsg_create_with_flags(i, &text1, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                    }
                }
                if isbot {
                    if let Some(o) = self.props.get_mut(id) {
                        o.hidden |= OBJHFLAG_DELETING;
                    }
                    return TICKOP_NONE;
                }
                TICKOP_FREE
            }
            MPSCENARIO_CAPTURETHECASE => self.ctc_pick_up_briefcase(ci, id),
            _ => TICKOP_NONE,
        }
    }

    /// `scenario_handle_dropped_token` (`scenarios.c:1347`): a briefcase just
    /// dropped by chr `ci` (its carrier died, or scored). In Capture the Case
    /// it is warped home to its team's base.
    pub(crate) fn scenario_handle_dropped_token(&mut self, ci: usize, id: u32) {
        if self.setup.scenario != MPSCENARIO_CAPTURETHECASE {
            return;
        }
        for i in 0..4 {
            if self.mp.scenariodata.ctc.tokens[i] == Some(PropRef::Chr(ci)) {
                self.ctc_send_token_home(i, id);
            }
        }
    }

    /// `scenario_pick_up_uplink` (`scenarios.c:1388`).
    pub(crate) fn scenario_pick_up_uplink(&mut self, ci: usize, id: u32) -> i32 {
        if self.setup.scenario != MPSCENARIO_HACKERCENTRAL {
            return TICKOP_NONE;
        }
        let isbot = self.chrs[ci].aibot.is_some();
        self.mp.scenariodata.htm.uplink = Some(PropRef::Chr(ci));
        // "%shas the\n%s"
        let message = self.res.lang.get(l(0)).replacen("%s", &self.scenario_chr_name(ci), 1).replacen("%s", &self.bgun_get_short_name(WEAPON_DATAUPLINK), 1);
        for i in 0..self.players.len() {
            if isbot || i != ci {
                self.hudmsg_create_with_flags(i, &message, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
            }
        }
        if isbot {
            if let Some(pos) = self.props.get(id).map(|o| o.pos) {
                self.sound_at(crate::props::pickup::weapon_pickup_sound(WEAPON_DATAUPLINK), 1.0, pos, crate::propsnd::DEFAULT_DISTS);
            }
            let a = self.ab_mut(ci);
            a.botinv_give_single_weapon(WEAPON_DATAUPLINK);
            a.hasuplink = true;
            if let Some(o) = self.props.get_mut(id) {
                o.hidden |= OBJHFLAG_DELETING;
            }
            return TICKOP_NONE;
        }
        self.players[ci].gun.p.inventory.inv_give_single_weapon(WEAPON_DATAUPLINK);
        self.current_player_queue_pickup_weapon_hudmsg(ci, WEAPON_DATAUPLINK, false);
        self.sound(crate::props::pickup::weapon_pickup_sound(WEAPON_DATAUPLINK), 1.0);
        TICKOP_FREE
    }

    /// `scenario_handle_activated_prop` (`scenarios.c:1469`): chr `ci` used
    /// object `id`. Hacker Central's terminal remembers who (the top four
    /// bits of `obj->hidden`, `OBJHFLAG_ACTIVATED_BY_BOND`), unless someone
    /// already has.
    pub(crate) fn scenario_handle_activated_prop(&mut self, ci: usize, id: u32) {
        if self.setup.scenario != MPSCENARIO_HACKERCENTRAL {
            return;
        }
        let Some(o) = self.props.get_mut(id) else { return };
        if o.flags3 & OBJFLAG3_HTMTERMINAL != 0 && o.hidden & OBJHFLAG_ACTIVATED_BY_BOND == 0 {
            o.hidden &= 0x0fff_ffff;
            o.hidden |= ((ci as u32) << 28) & 0xf000_0000;
            o.hidden |= OBJHFLAG_ACTIVATED_BY_BOND;
        }
    }

    /// `scenario_highlight_prop`'s scenario callback (`scenarios.c:714`) for a
    /// chr or an object as player `pi` sees it: the token (a carrier, the
    /// victim, the terminal) green at 0x40; a Capture the Case briefcase in its
    /// team's colour.
    pub(crate) fn scenario_highlight_prop_callback(&self, p: PropRef) -> Option<[u8; 4]> {
        let o = self.setup.options;
        let green = Some([0, 0xff, 0, 0x40]);
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE if o & MPOPTION_HTB_HIGHLIGHTBRIEFCASE != 0 && self.mp.scenariodata.htb.token == Some(p) => green,
            MPSCENARIO_HACKERCENTRAL if o & MPOPTION_HTM_HIGHLIGHTTERMINAL != 0 => {
                let d = &self.mp.scenariodata.htm;
                let terminal = p.obj().is_some() && d.terminals.iter().take(htm::HTM_NUM_TERMINALS).any(|t| t.prop == p.obj());
                (d.uplink == Some(p) || terminal).then_some([0, 0xff, 0, 0x40])
            }
            MPSCENARIO_POPACAP => self.pac_highlight_prop(p),
            MPSCENARIO_CAPTURETHECASE => self.ctc_highlight_prop(p),
            _ => None,
        }
    }

    /// `scenario_highlight_room` (`scenarios.c:934`): what the scenario
    /// multiplies a highlighted room's colours by (King of the Hill's hill,
    /// Capture the Case's bases), per channel; `None` leaves them.
    pub fn scenario_highlight_room(&self, room: u16) -> Option<[f32; 3]> {
        match self.setup.scenario {
            MPSCENARIO_KINGOFTHEHILL => self.koh_highlight_room(room),
            MPSCENARIO_CAPTURETHECASE => self.ctc_highlight_room(room),
            _ => None,
        }
    }

    /// The rooms the scenario tints this frame, with the multipliers (for
    /// `lighting_tick`'s `highlightfrac` and the renderer's room colours).
    pub fn scenario_highlighted_rooms(&self) -> Vec<(u16, [f32; 3])> {
        (1..self.lights.rooms.len() as u16).filter(|&r| self.lights.rooms[r as usize].lightop == crate::lights::LIGHTOP_HIGHLIGHT).filter_map(|r| self.scenario_highlight_room(r).map(|t| (r, t))).collect()
    }

    /// `scenario_get_max_teams` (`scenarios.c:908`).
    pub fn scenario_get_max_teams(&self) -> i32 {
        if self.setup.scenario == MPSCENARIO_CAPTURETHECASE {
            ctc::ctc_get_max_teams()
        } else {
            pd_core::mp::MAX_TEAMS as i32
        }
    }

    /// `scenario_choose_spawn_location`'s pad list (`scenarios.c:794`): Capture
    /// the Case spawns chr `ci` on its team's respawn pads
    /// (`ctc_choose_spawn_location`); everyone else on the setup's spawns.
    pub(crate) fn scenario_spawn_pads(&self, ci: usize) -> Vec<usize> {
        if self.setup.scenario == MPSCENARIO_CAPTURETHECASE {
            if let Some(pads) = self.ctc_spawn_pads(ci) {
                return pads;
            }
        }
        self.stage.spawn_pads.clone()
    }

    /// The scenario's counters for the scores, by chr slot
    /// (`htm_calculate_player_score`'s and `pac_`'s `mp_chrslot_to_chrindex`).
    pub fn scenario_scores(&self) -> ScenarioScores {
        let mut s = ScenarioScores::new(self.setup.scenario, self.setup.options);
        for (ci, c) in self.chrs.iter().enumerate().take(MAX_MPCHRS) {
            let slot = c.mpslot;
            s.htm_numpoints[slot] = self.mp.scenariodata.htm.numpoints[ci];
            s.pac_killcounts[slot] = self.mp.scenariodata.pac.killcounts[ci] as i32;
            s.pac_survivalcounts[slot] = self.mp.scenariodata.pac.survivalcounts[ci] as i32;
        }
        s
    }

    /// `mp_calculate_team_is_only_ai` (`mplayer.c:330`), at the stage's load
    /// (`lv.c:447`): a simulant with no human on its team (every simulant with
    /// teams off) picks its own scenario orders.
    pub(crate) fn mp_calculate_team_is_only_ai(&mut self) {
        let n = self.players.len();
        for i in n..self.chrs.len() {
            let team = self.chrs[i].team;
            let alone = !(self.setup.teams_enabled() && (0..n).any(|j| self.chrs[j].team == team));
            if let Some(a) = self.chrs[i].aibot.as_mut() {
                a.teamisonlyai = alone;
            }
        }
    }

    /// The scenario's part of player `pi`'s HUD (`scenario_render_hud`,
    /// `scenarios.c:557`), for `pd_render`: nothing once the match is over
    /// or ending.
    pub fn scenario_hud(&self, pi: usize) -> ScenarioHud {
        if self.mp.paused == MPPAUSEMODE_GAMEOVER || self.mp.numreasonstoend != 0 {
            return ScenarioHud::None;
        }
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE => self.htb_render_hud(pi),
            MPSCENARIO_HACKERCENTRAL => self.htm_render_hud(pi),
            MPSCENARIO_POPACAP => self.pac_render_hud(pi),
            MPSCENARIO_KINGOFTHEHILL => self.koh_render_hud(pi),
            _ => ScenarioHud::None,
        }
    }

    /// A weapon object dropped by chr `ci`: `obj_init`'s marking of a scenario
    /// token, then `scenario_handle_dropped_token` for a briefcase
    /// (`weapon_create_for_player_drop`, `botinv_drop`).
    pub(crate) fn scenario_weapon_dropped(&mut self, ci: usize, id: u32, weaponnum: u8) {
        self.scenario_obj_init(id);
        if weaponnum == WEAPON_BRIEFCASE2 {
            self.scenario_handle_dropped_token(ci, id);
        }
    }

    /// `pad_unpack(pad, PADFIELD_POS | PADFIELD_LOOK | PADFIELD_UP)`'s basis as
    /// `mtx00016d58(-look, up)` builds it (a token placed on a pad).
    fn scenario_pad_basis(&self, pad: usize) -> Option<(Vec3, Mat3, Option<u16>)> {
        let p = self.stage.pads.get(pad)?;
        let m = pd_core::math::look_at_basis(Vec3::ZERO, -p.look, p.up);
        Some((p.pos, Mat3::from_mat4(m), p.room))
    }
}

/// What the scenario draws on a player's HUD (`scenario_render_hud`): a
/// countdown at the top centre in the numeric font (Hold the Briefcase's,
/// Pop a Cap's, King of the Hill's), or Hacker Central's download bar.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioHud {
    None,
    /// `text_render_v1` of `text` centred at the view's top + 10, green
    /// (0x00ff00a0) with the 0xa0 glow, over an invisible black box.
    Countdown { text: String },
    /// `htm_render_hud`: the bar a third of the view wide at its top (8..16),
    /// `frac` of it filled in 1-pixel strips every 2.
    DownloadBar { frac: f32 },
}
