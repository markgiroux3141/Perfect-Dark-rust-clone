//! Hold the Briefcase (`scenarios/holdthebriefcase.inc`): a briefcase lies
//! somewhere in the arena; whoever holds it scores a point every 30 seconds.
//! Its carrier can't take shields and moves a little slower, but keeps every
//! weapon. The case falls where its carrier dies.

use glam::Vec3;
use pd_core::ids::*;

use super::{l, PropRef, ScenarioHud};
use crate::world::World;

/// `scenariodata_htb` (`types.h:4125`).
#[derive(Clone, Debug)]
pub struct Htb {
    /// The case: on the ground, or its carrier.
    pub token: Option<PropRef>,
    /// Where it is (`htb_tick`; the radar's).
    pub pos: Vec3,
    pub tokenpad: i32,
    pub nextindex: i16,
    /// The setup's case pads (`intro[]`'s `case` and `case_respawn` rows).
    pub padnums: [i16; 60],
}

impl Default for Htb {
    fn default() -> Self {
        Htb { token: None, pos: Vec3::ZERO, tokenpad: 0, nextindex: 0, padnums: [-1; 60] }
    }
}

impl Htb {
    /// `htb_add_pad` (`holdthebriefcase.inc:130`).
    pub fn htb_add_pad(&mut self, padnum: i16) {
        if (self.nextindex as usize) < self.padnums.len() {
            self.padnums[self.nextindex as usize] = padnum;
            self.nextindex += 1;
        }
    }

    /// `htb_reset` (`holdthebriefcase.inc:171`).
    pub fn htb_reset(&mut self) {
        self.nextindex = 0;
        self.padnums = [-1; 60];
    }
}

/// The case is worth a point after `TICKS(7200)` quarter-ticks held: 30 s.
pub const HTB_POINT_TIME240: i32 = 7200;

impl World {
    /// `htb_create_token` (`holdthebriefcase.inc:182`): the case takes the
    /// place of a random ammo crate (of the newest 20), else goes on a random
    /// case pad, else on pad 0.
    pub(crate) fn htb_create_token(&mut self) {
        let pad = match self.scenario_replace_crate() {
            Some(pad) => pad,
            None => {
                let d = &self.mp.scenariodata.htb;
                if d.nextindex > 0 {
                    let n = d.nextindex as u32;
                    let k = (self.rng.random() % n) as usize;
                    self.mp.scenariodata.htb.padnums[k] as i32
                } else {
                    0
                }
            }
        };
        self.mp.scenariodata.htb.tokenpad = pad;
        let id = self.scenario_place_token(MODEL_CHRBRIEFCASE, WEAPON_BRIEFCASE2, pad, 256);
        self.mp.scenariodata.htb.token = id.map(PropRef::Obj);
    }

    /// `htb_init_props` (`holdthebriefcase.inc:269`).
    pub(crate) fn htb_init_props(&mut self) {
        self.mp.scenariodata.replacedcrate = None;
        self.htb_create_token();
    }

    /// `htb_tick` (`holdthebriefcase.inc:275`): where the case is now — on the
    /// ground, with a player, with a simulant — and a new one when it is
    /// nowhere.
    pub(crate) fn htb_tick(&mut self) {
        let token = self.mp.scenariodata.htb.token;
        self.scenario_hold_replaced_crate(token);
        let mut token = None;
        // On the ground (the last in the list).
        for o in self.props.objs.iter().rev() {
            if o.ty == OBJTYPE_WEAPON && o.weaponnum == WEAPON_BRIEFCASE2 {
                token = Some(PropRef::Obj(o.id));
            }
        }
        if token.is_none() {
            token = (0..self.players.len()).find(|&i| self.inv_has_briefcase(i)).map(PropRef::Chr);
        }
        if token.is_none() {
            token = (self.players.len()..self.chrs.len()).find(|&i| self.chrs[i].aibot.as_ref().is_some_and(|a| a.hasbriefcase)).map(PropRef::Chr);
        }
        self.mp.scenariodata.htb.token = token;
        if token.is_none() {
            self.htb_create_token();
        }
        let pos = self.mp.scenariodata.htb.token.and_then(|t| self.scenario_prop_pos(t)).unwrap_or(Vec3::ZERO);
        self.mp.scenariodata.htb.pos = pos;
    }

    /// `htb_tick_chr` (`holdthebriefcase.inc:351`): a carrier's 30 s to a
    /// point ("1 Point!" for a player), with the score sound.
    pub(crate) fn htb_tick_chr(&mut self, chr: Option<usize>, pi: usize) {
        let lv240 = self.lv.lvupdate240;
        if let Some(c) = chr {
            let a = self.ab_mut(c);
            if a.hasbriefcase {
                a.htbheldtimer60 += lv240;
                if a.htbheldtimer60 >= HTB_POINT_TIME240 {
                    a.htbheldtimer60 = 0;
                    self.sound(SFXNUM_05B8_MP_SCOREPOINT, 1.0);
                    let slot = self.chrs[c].mpslot;
                    self.mp.chrs[slot].numpoints += 1;
                }
            } else {
                a.htbheldtimer60 = 0;
            }
        } else if self.inv_has_briefcase(pi) {
            self.mp.playerstats[pi].tokenheldtime += lv240;
            if self.mp.playerstats[pi].tokenheldtime >= HTB_POINT_TIME240 {
                self.sound(SFXNUM_05B8_MP_SCOREPOINT, 1.0);
                // g_MpAllChrConfigPtrs[currentplayernum]: the player's chr.
                let slot = self.chrs[pi].mpslot;
                self.mp.chrs[slot].numpoints += 1;
                let text = self.res.lang.get(l(24)).to_string();
                self.hudmsg_create_with_flags(pi, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                self.mp.playerstats[pi].tokenheldtime = 0;
            }
        } else {
            self.mp.playerstats[pi].tokenheldtime = 0;
        }
    }

    /// `htb_render_hud` (`holdthebriefcase.inc:392`): the carrier's time to
    /// the next point, "0:30" down to "0:01". PD's bug is kept: the minutes
    /// are taken by 7200 rather than 14400, which only matters past 30 s.
    pub(crate) fn htb_render_hud(&self, pi: usize) -> ScenarioHud {
        if !self.inv_has_briefcase(pi) {
            return ScenarioHud::None;
        }
        let mut time240 = HTB_POINT_TIME240 - self.mp.playerstats[pi].tokenheldtime;
        let mins = time240 / 7200;
        time240 -= 7200 * mins;
        let secs = (time240 + 240 - 1) / 240;
        ScenarioHud::Countdown { text: format!("{mins}:{secs:02}") }
    }
}

/// `SFXNUM_05B8_MP_SCOREPOINT`.
pub const SFXNUM_05B8_MP_SCOREPOINT: u16 = 0x05b8;
