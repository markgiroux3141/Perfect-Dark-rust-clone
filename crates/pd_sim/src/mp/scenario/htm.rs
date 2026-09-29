//! Hacker Central (`scenarios/hackthatmac.inc`, PD's "Hack that Mac"): a data
//! uplink and a terminal appear at random spots. Whoever carries the uplink,
//! holds it and uses the terminal (B, or fire with the uplink in hand) and
//! stays within 2.5 m of it, facing it, for 20 seconds downloads its data: two
//! points. The uplink has no model to drop, so it is lost when its carrier
//! dies and a new one appears.

use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::math::{atan2f, baddtor, rtod2};
use pd_core::mp::MAX_MPCHRS;

use super::{l, PropRef, ScenarioHud};
use crate::world::World;

/// `HTM_NUM_TERMINALS`.
pub const HTM_NUM_TERMINALS: usize = 1;
/// `htm_num_props`: the terminals and the uplink.
pub fn htm_num_props() -> usize {
    HTM_NUM_TERMINALS + 1
}

/// A download takes more than `20 * TICKS(240)` quarter-ticks.
pub const HTM_DOWNLOAD_TIME240: i32 = 20 * 240;

/// `SFXNUM_01BF`: the download's loop on the terminal; `SFXNUM_01C1`: done;
/// `SFXNUM_01CC`: broken off, or used without the uplink.
pub const SFXNUM_01BF: u16 = 0x01bf;
pub const SFXNUM_01C1: u16 = 0x01c1;
pub const SFXNUM_01CC: u16 = 0x01cc;

/// The terminal's download loop (`ps_create(NULL, terminal, SFXNUM_01BF, ...,
/// PSFLAG_REPEATING)`), by terminal.
pub const HTM_TERMINAL_HANDLE: u32 = 0x320;

/// `struct htmterminal`.
#[derive(Clone, Copy, Debug)]
pub struct HtmTerminal {
    pub prop: Option<u32>,
    pub padnum: i16,
    pub team: u8,
}

impl Default for HtmTerminal {
    fn default() -> Self {
        HtmTerminal { prop: None, padnum: -1, team: 255 }
    }
}

/// `scenariodata_htm` (`types.h:4142`), by chr index where PD's are.
#[derive(Clone, Debug)]
pub struct Htm {
    pub numpads: i16,
    pub numterminals: i16,
    pub padnums: [i16; 60],
    /// PD has room for 7; only the first is made.
    pub terminals: [HtmTerminal; 7],
    pub dlplayernum: i16,
    pub playernuminrange: i16,
    pub dlterminalnum: i32,
    pub numpoints: [i32; MAX_MPCHRS],
    pub dltime240: [i32; MAX_MPCHRS],
    pub uplink: Option<PropRef>,
}

impl Default for Htm {
    fn default() -> Self {
        Htm { numpads: 0, numterminals: 0, padnums: [-1; 60], terminals: [HtmTerminal::default(); 7], dlplayernum: -1, playernuminrange: -1, dlterminalnum: -1, numpoints: [0; MAX_MPCHRS], dltime240: [0; MAX_MPCHRS], uplink: None }
    }
}

impl Htm {
    /// `htm_add_pad` (`hackthatmac.inc:136`).
    pub fn htm_add_pad(&mut self, padnum: i16) {
        if (self.numpads as usize) < self.padnums.len() {
            self.padnums[self.numpads as usize] = padnum;
            self.numpads += 1;
        }
    }

    /// `htm_reset` (`hackthatmac.inc:151`).
    pub fn htm_reset(&mut self) {
        let uplink = self.uplink;
        *self = Htm { uplink, ..Htm::default() };
    }
}

impl World {
    /// `htb_create_uplink` (`hackthatmac.inc:183`): the uplink in a random
    /// ammo crate's place, else on a random case or crate pad.
    pub(crate) fn htb_create_uplink(&mut self) {
        let pad = match self.scenario_replace_crate() {
            Some(pad) => pad,
            None => {
                let d = &self.mp.scenariodata.htm;
                if d.numpads > 0 {
                    let n = d.numpads as u32;
                    let k = (self.rng.random() % n) as usize;
                    self.mp.scenariodata.htm.padnums[k] as i32
                } else {
                    0
                }
            }
        };
        let id = self.scenario_place_token(MODEL_CHRDATATHIEF, WEAPON_DATAUPLINK, pad, 512);
        self.mp.scenariodata.htm.uplink = id.map(PropRef::Obj);
    }

    /// `htm_init_props` (`hackthatmac.inc:266`): the ammo crates' pads join
    /// the case pads; the terminal goes on a random one (drawn for every prop,
    /// PD's bug: the uplink's draw is wasted) and its crate is removed; then
    /// the uplink.
    pub(crate) fn htm_init_props(&mut self) {
        let pads: Vec<i16> = self.props.objs.iter().rev().filter(|o| matches!(o.ty, OBJTYPE_AMMOCRATE | OBJTYPE_MULTIAMMOCRATE) && o.modelnum == MODEL_MULTI_AMMO_CRATE).map(|o| o.pad as i16).collect();
        for p in pads {
            self.mp.scenariodata.htm.htm_add_pad(p);
        }
        self.mp.scenariodata.htm.numterminals = 0;
        while (self.mp.scenariodata.htm.numterminals as usize) < htm_num_props() {
            let numpads = self.mp.scenariodata.htm.numpads as u32;
            if numpads == 0 {
                break;
            }
            let (rand, padnum) = loop {
                let rand = (self.rng.random() % numpads) as usize;
                let padnum = self.mp.scenariodata.htm.padnums[rand];
                if padnum > 0 {
                    break (rand, padnum);
                }
            };
            let d = &mut self.mp.scenariodata.htm;
            d.terminals[d.numterminals as usize].padnum = padnum;
            d.numterminals += 1;
            d.padnums[rand] = -1;
        }
        for i in 0..HTM_NUM_TERMINALS {
            let pad = self.mp.scenariodata.htm.terminals[i].padnum as i32;
            let prop = self.scenario_create_obj(MODEL_GOODPC, pad, 0.2, OBJFLAG_FALL | OBJFLAG_INVINCIBLE | OBJFLAG_FORCENOBOUNCE, OBJFLAG2_IMMUNETOGUNFIRE | OBJFLAG2_IMMUNETOEXPLOSIONS, OBJFLAG3_HTMTERMINAL | OBJFLAG3_INTERACTABLE);
            self.mp.scenariodata.htm.terminals[i].prop = prop;
            self.htb_remove_ammo_crate_at_pad(pad);
        }
        self.mp.scenariodata.replacedcrate = None;
        self.htb_create_uplink();
    }

    /// `scenario_create_obj` (`scenarios.c:988`): a basic object of `MODEL_*`
    /// `modelnum` on `padnum` at `scale` (`extrascale = scale * 256`).
    pub(crate) fn scenario_create_obj(&mut self, modelnum: i32, padnum: i32, scale: f32, flags: u32, flags2: u32, flags3: u32) -> Option<u32> {
        let o = self.setup_obj(modelnum, OBJTYPE_BASIC, flags, flags2, flags3, 1000)?;
        let id = o.id;
        let before = self.props.objs.len();
        self.setup_create_object(o, padnum, (scale * 256.0) as i16 as i32);
        (self.props.objs.len() > before).then_some(id)
    }

    /// `htb_remove_ammo_crate_at_pad` (`holdthebriefcase.inc:150`): the newest
    /// MP ammo crate on `padnum` goes for good.
    pub(crate) fn htb_remove_ammo_crate_at_pad(&mut self, padnum: i32) {
        if let Some(o) = self.props.objs.iter_mut().rev().find(|o| o.pad == padnum && matches!(o.ty, OBJTYPE_AMMOCRATE | OBJTYPE_MULTIAMMOCRATE) && o.modelnum == MODEL_MULTI_AMMO_CRATE) {
            o.hidden |= OBJHFLAG_DELETING;
            o.hidden2 &= !OBJH2FLAG_CANREGEN;
        }
    }

    /// `htm_tick` (`hackthatmac.inc:335`): where the uplink is — on the
    /// ground, with a player, with a simulant — and a new one when it is gone.
    pub(crate) fn htm_tick(&mut self) {
        let uplink = self.mp.scenariodata.htm.uplink;
        self.scenario_hold_replaced_crate(uplink);
        let mut uplink = None;
        for o in self.props.objs.iter().rev() {
            if o.ty == OBJTYPE_WEAPON && o.weaponnum == WEAPON_DATAUPLINK {
                uplink = Some(PropRef::Obj(o.id));
            }
        }
        if uplink.is_none() {
            uplink = (0..self.players.len()).find(|&i| self.inv_has_data_uplink(i)).map(PropRef::Chr);
        }
        if uplink.is_none() {
            uplink = (self.players.len()..self.chrs.len()).find(|&i| self.chrs[i].aibot.as_ref().is_some_and(|a| a.hasuplink)).map(PropRef::Chr);
        }
        self.mp.scenariodata.htm.uplink = uplink;
        if uplink.is_none() {
            self.htb_create_uplink();
        }
    }

    /// `htm_tick_chr` (`hackthatmac.inc:396`): simulant `chr`, or player `pi`.
    /// A simulant with the uplink starts a download wherever it is (and has
    /// it broken off until it reaches the terminal); a player starts one by
    /// using the terminal with the uplink in hand. The download holds while
    /// its downloader stays within 250 cm across and 200 cm up or down,
    /// facing the terminal within 45°, holding the uplink (a simulant: its
    /// fists); past 20 s it is a download.
    pub(crate) fn htm_tick_chr(&mut self, chr: Option<usize>, pi: usize) {
        let (hasuplink, playernum) = match chr {
            Some(c) => (self.ab(c).hasuplink, c),
            None => (self.inv_has_data_uplink(pi) && self.players[pi].gun.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_DATAUPLINK, pi),
        };
        for i in 0..HTM_NUM_TERMINALS {
            let Some(tid) = self.mp.scenariodata.htm.terminals[i].prop else { continue };
            let Some(hidden) = self.props.get(tid).map(|o| o.hidden) else { continue };
            let mut activatedbyplayernum: i32 = -1;
            if chr.is_some() {
                if hasuplink {
                    activatedbyplayernum = playernum as i32;
                }
            } else if hidden & OBJHFLAG_ACTIVATED_BY_BOND != 0 {
                activatedbyplayernum = ((hidden & 0xf000_0000) >> 28) as i32;
            }
            if playernum as i32 != activatedbyplayernum {
                continue;
            }
            if let Some(o) = self.props.get_mut(tid) {
                o.hidden &= !OBJHFLAG_ACTIVATED_BY_BOND;
                o.hidden &= !0xf000_0000;
            }
            if hasuplink {
                if self.mp.scenariodata.htm.dlterminalnum == -1 {
                    let d = &mut self.mp.scenariodata.htm;
                    d.dlterminalnum = i as i32;
                    d.dlplayernum = playernum as i16;
                    d.playernuminrange = playernum as i16;
                    d.dltime240[playernum] = 0;
                    if chr.is_none() {
                        let text = self.res.lang.get(l(18)).to_string();
                        self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                        self.htm_terminal_sound(i, true);
                    }
                }
            } else if chr.is_none() {
                // "You need to use the Data Uplink."
                let text = self.res.lang.get(l(19)).to_string();
                self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                // SUBST: PD's snd_start_extra plays the sample's loop until its
                // envelope's 1.3 s gate / the sample plays once (0.5 s).
                self.sound(SFXNUM_01CC, 1.0);
            }
        }
        let d = &self.mp.scenariodata.htm;
        if playernum as i32 != d.dlplayernum as i32 || d.dlterminalnum == -1 {
            return;
        }
        let term = d.dlterminalnum as usize;
        let Some(terminalpos) = d.terminals[term].prop.and_then(|id| self.props.get(id)).map(|o| o.pos) else { return };
        let (chrpos, degrees, holdinguplink) = match chr {
            Some(c) => (self.chrs[c].pos, rtod2(baddtor(360.0) - self.chrs[c].theta()), self.ab(c).weaponnum == WEAPON_UNARMED),
            None => (self.players[pi].pos, self.players[pi].theta, self.players[pi].gun.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_DATAUPLINK),
        };
        let dist = terminalpos - chrpos;
        let rangexz = (dist.x * dist.x + dist.z * dist.z).sqrt();
        let rangey = dist.y.abs();
        let mut reldegrees = degrees + rtod2(atan2f(dist.x, dist.z));
        while reldegrees < 180.0 {
            reldegrees += 360.0;
        }
        while reldegrees > 180.0 {
            reldegrees -= 360.0;
        }
        if reldegrees <= 0.0 {
            reldegrees = -reldegrees;
        }
        if rangexz > 250.0 || rangey > 200.0 || reldegrees > 45.0 || !holdinguplink {
            let inrange = rangexz < 250.0 && rangey < 200.0;
            self.mp.scenariodata.htm.playernuminrange = if inrange { playernum as i16 } else { -1 };
            if chr.is_none() {
                // "Connection broken."
                let text = self.res.lang.get(l(17)).to_string();
                self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                self.htm_terminal_sound(term, false);
                self.sound(SFXNUM_01CC, 1.0);
            }
            let d = &mut self.mp.scenariodata.htm;
            d.dlterminalnum = -1;
            d.dlplayernum = -1;
            d.dltime240[playernum] = 0;
        } else {
            let lv240 = self.lv.lvupdate240;
            let d = &mut self.mp.scenariodata.htm;
            d.dltime240[playernum] += lv240;
            if d.dltime240[playernum] > HTM_DOWNLOAD_TIME240 {
                d.numpoints[playernum] += 1;
                d.playernuminrange = playernum as i16;
                if chr.is_none() {
                    // "Download successful."
                    let text = self.res.lang.get(l(16)).to_string();
                    self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                    self.htm_terminal_sound(term, false);
                    self.sound(SFXNUM_01C1, 1.0);
                }
                let d = &mut self.mp.scenariodata.htm;
                d.dlterminalnum = -1;
                d.dlplayernum = -1;
                d.dltime240[playernum] = 0;
            }
        }
    }

    /// The terminal's download loop: started at the terminal
    /// (`ps_create(..., PSFLAG_REPEATING, PSFLAG2_MPPAUSABLE)`), or stopped
    /// (`ps_stop_sound(terminal, PSTYPE_GENERAL, 0xffff)`).
    fn htm_terminal_sound(&mut self, term: usize, start: bool) {
        let handle = HTM_TERMINAL_HANDLE + term as u32;
        if !start {
            self.push_event(Event::StopSound { handle });
            return;
        }
        let Some(pos) = self.mp.scenariodata.htm.terminals[term].prop.and_then(|id| self.props.get(id)).map(|o| o.pos) else { return };
        let (volume, pan) = crate::propsnd::vol_pan(&self.res.audio, SFXNUM_01BF, pos, crate::propsnd::DEFAULT_DISTS, &self.listeners());
        self.push_event(Event::HandleSound { handle, sound: SFXNUM_01BF, pitch: 1.0, volume, pan });
    }

    /// `htm_render_hud` (`hackthatmac.inc:567`): the downloader's progress bar.
    pub(crate) fn htm_render_hud(&self, pi: usize) -> ScenarioHud {
        let d = &self.mp.scenariodata.htm;
        if d.dlterminalnum != -1 && pi as i32 == d.dlplayernum as i32 {
            return ScenarioHud::DownloadBar { frac: d.dltime240[pi] as f32 / 4800.0 };
        }
        ScenarioHud::None
    }
}
