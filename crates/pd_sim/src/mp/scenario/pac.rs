//! Pop a Cap (`scenarios/popacap.inc`): one chr at a time is the victim, in
//! an order shuffled at the start. Killing the victim scores two points; the
//! victim scores one for each minute alive. When the victim dies the next in
//! the list is the victim.

use pd_core::ids::*;
use pd_core::mp::MAX_MPCHRS;

use super::{l, PropRef, ScenarioHud};
use crate::world::World;

/// A minute alive is a point: `TICKS(240 * 60)` quarter-ticks.
pub const PAC_SURVIVAL_TIME240: u32 = 240 * 60;

/// `scenariodata_pac` (`types.h:4158`), by chr index.
#[derive(Clone, Debug)]
pub struct Pac {
    pub age240: u16,
    /// Into `victims`; -1 before the first.
    pub victimindex: i32,
    /// The chr indexes, shuffled.
    pub victims: [i16; MAX_MPCHRS],
    pub killcounts: [i16; MAX_MPCHRS],
    pub survivalcounts: [i16; MAX_MPCHRS],
}

impl Default for Pac {
    fn default() -> Self {
        Pac { age240: 0, victimindex: -1, victims: [0; MAX_MPCHRS], killcounts: [0; MAX_MPCHRS], survivalcounts: [0; MAX_MPCHRS] }
    }
}

impl World {
    /// `pac_reset` (`popacap.inc:119`): no victim yet, no points, and every
    /// chr's turn drawn (`random() % g_MpNumChrs` until each is in once).
    pub(crate) fn pac_reset(&mut self) {
        let n = self.chrs.len().min(MAX_MPCHRS);
        let d = &mut self.mp.scenariodata.pac;
        d.victimindex = -1;
        d.age240 = 0;
        d.killcounts = [0; MAX_MPCHRS];
        d.survivalcounts = [0; MAX_MPCHRS];
        let mut i = 0;
        while i < n {
            let victimplayernum = (self.rng.random() % n as u32) as i16;
            let d = &mut self.mp.scenariodata.pac;
            if !d.victims[..i].contains(&victimplayernum) {
                d.victims[i] = victimplayernum;
                i += 1;
            }
        }
    }

    /// `pac_init_props` (`popacap.inc:168`).
    pub(crate) fn pac_init_props(&mut self) {
        self.pac_reset();
    }

    /// The victim's chr index, once there is one.
    pub fn pac_victim(&self) -> Option<usize> {
        let d = &self.mp.scenariodata.pac;
        (d.victimindex >= 0).then(|| d.victims[d.victimindex as usize] as usize)
    }

    /// `pac_highlight_prop` (`popacap.inc:173`): Highlight Target greens the victim.
    pub(crate) fn pac_highlight_prop(&self, p: PropRef) -> Option<[u8; 4]> {
        if self.setup.options & MPOPTION_PAC_HIGHLIGHTTARGET != 0 && p.chr().is_some() && p.chr() == self.pac_victim() {
            return Some([0, 0xff, 0, 0x40]);
        }
        None
    }

    /// `pac_apply_next_victim` (`popacap.inc:191`): the next in the list, and
    /// every player told ("You are the victim!", "Protect X!", "Get X!").
    pub fn pac_apply_next_victim(&mut self) {
        let n = self.chrs.len() as i32;
        let d = &mut self.mp.scenariodata.pac;
        d.victimindex += 1;
        if d.victimindex == n {
            d.victimindex = 0;
        }
        d.age240 = 0;
        let vplayernum = d.victims[d.victimindex as usize] as i32;
        for i in 0..self.players.len() as i32 {
            let text = if vplayernum == i {
                self.res.lang.get(l(13)).to_string()
            } else if self.scenario_chrs_are_same_team(vplayernum, i) {
                self.res.lang.get(l(14)).replacen("%s", &self.scenario_chr_name(vplayernum as usize), 1)
            } else {
                self.res.lang.get(l(15)).replacen("%s", &self.scenario_chr_name(vplayernum as usize), 1)
            };
            self.scenario_create_hudmsg(i, &text);
        }
    }

    /// `pac_handle_death` (`popacap.inc:229`), from `mpstats_record_death`:
    /// the victim killed by an enemy scores the killer two ("Well done!",
    /// "You popped a cap!", "Have 2 Points..."), by a teammate a telling-off;
    /// either way the next victim. A victim's suicide only restarts its minute.
    pub(crate) fn pac_handle_death(&mut self, aplayernum: i32, vplayernum: i32) {
        let d = &self.mp.scenariodata.pac;
        if d.victimindex < 0 || vplayernum != d.victims[d.victimindex as usize] as i32 {
            return;
        }
        if aplayernum != vplayernum {
            if aplayernum >= 0 {
                if self.scenario_chrs_are_same_team(aplayernum, vplayernum) {
                    for t in [8, 9] {
                        let text = self.res.lang.get(l(t)).to_string();
                        self.scenario_create_hudmsg(aplayernum, &text);
                    }
                } else {
                    self.mp.scenariodata.pac.killcounts[aplayernum as usize] += 1;
                    for t in [10, 11, 12] {
                        let text = self.res.lang.get(l(t)).to_string();
                        self.scenario_create_hudmsg(aplayernum, &text);
                    }
                }
            }
            self.pac_apply_next_victim();
        } else {
            self.mp.scenariodata.pac.age240 = 0;
        }
    }

    /// `pac_tick` (`popacap.inc:256`): the first victim; a living victim's
    /// minute ("Have a point for living!"). A dead human victim's clock stops
    /// until it respawns (a simulant's always runs).
    pub(crate) fn pac_tick(&mut self) {
        if self.mp.scenariodata.pac.victimindex == -1 {
            self.pac_apply_next_victim();
        }
        let Some(v) = self.pac_victim() else { return };
        if v >= self.players.len() || !self.players[v].isdead {
            let lv240 = self.lv.lvupdate240;
            let d = &mut self.mp.scenariodata.pac;
            d.age240 = d.age240.wrapping_add(lv240 as u16);
            if d.age240 as u32 > PAC_SURVIVAL_TIME240 {
                d.age240 = 0;
                d.survivalcounts[v] += 1;
                let text = self.res.lang.get(l(7)).to_string();
                self.scenario_create_hudmsg(v as i32, &text);
            }
        }
    }

    /// `pac_render_hud` (`popacap.inc:283`): a living victim's time to its
    /// next point.
    pub(crate) fn pac_render_hud(&self, pi: usize) -> ScenarioHud {
        let d = &self.mp.scenariodata.pac;
        // PD reads victims[victimindex] even at -1, before the first victim
        // is drawn: the s16 before the array, the low half of victimindex
        // itself (-1), which is no player.
        if Some(pi) != self.pac_victim() || self.players[pi].isdead {
            return ScenarioHud::None;
        }
        let mut time240 = PAC_SURVIVAL_TIME240 as i32 - d.age240 as i32;
        if time240 < 0 {
            time240 = 0;
        }
        let mins = time240 / (60 * 240);
        time240 -= 60 * 240 * mins;
        let secs = (time240 + (240 - 1)) / 240;
        ScenarioHud::Countdown { text: format!("{mins}:{secs:02}") }
    }
}
