//! Capture the Case (`scenarios/capturethecase.inc`): up to four teams, each
//! with a base (lit in its colour) and a briefcase on its home pad. Carry an
//! enemy case to your own and touch it to score three points; a carrier who
//! dies sends the case straight home. Teams spawn on their base's respawn pads.

use pd_core::ids::*;
use pd_core::mp::MAX_MPCHRS;

use super::{l, PropRef};
use crate::lights::LIGHTOP_HIGHLIGHT;
use crate::mp::G_TEAM_COLOURS;
use crate::world::World;

/// `ctc_get_max_teams` (`capturethecase.inc:475`).
pub fn ctc_get_max_teams() -> i32 {
    4
}

/// `struct ctcspawnpadsperteam`.
#[derive(Clone, Copy, Debug)]
pub struct CtcSpawnPads {
    pub homepad: i16,
    pub numspawnpads: i16,
    pub spawnpads: [i16; 6],
}

impl Default for CtcSpawnPads {
    fn default() -> Self {
        CtcSpawnPads { homepad: -1, numspawnpads: 0, spawnpads: [-1; 6] }
    }
}

/// `scenariodata_ctc` (`types.h:4188`): by team, and by base (the setup's
/// `case` id) for the pads.
#[derive(Clone, Debug)]
pub struct Ctc {
    pub playercountsperteam: [i16; 4],
    /// Each team's base (shuffled), -1 for a team with no one.
    pub teamindexes: [i16; 4],
    pub baserooms: [Option<u16>; 4],
    pub spawnpadsperteam: [CtcSpawnPads; 4],
    /// Each team's case: at home or dropped, or its carrier.
    pub tokens: [Option<PropRef>; 4],
}

impl Default for Ctc {
    /// `ctc_init` (`capturethecase.inc:120`): home pads 0..3 until the setup's.
    fn default() -> Self {
        let mut spawnpadsperteam = [CtcSpawnPads::default(); 4];
        for (i, s) in spawnpadsperteam.iter_mut().enumerate() {
            s.homepad = i as i16;
        }
        Ctc { playercountsperteam: [0; 4], teamindexes: [-1; 4], baserooms: [None; 4], spawnpadsperteam, tokens: [None; 4] }
    }
}

impl Ctc {
    /// `scenario_reset`'s Capture the Case half (`scenarios.c:823`).
    pub fn reset(&mut self) {
        self.spawnpadsperteam = [CtcSpawnPads::default(); 4];
        self.playercountsperteam = [0; 4];
        self.teamindexes = [-1; 4];
    }

    /// `ctc_add_pad` (`capturethecase.inc:441`): a base's home pad (`case`) or
    /// one of its respawn pads (`case_respawn`, at most six).
    pub fn ctc_add_pad(&mut self, home: bool, base: i32, pad: i32) {
        let Some(s) = self.spawnpadsperteam.get_mut(base as usize) else { return };
        if home {
            s.homepad = pad as i16;
        } else if let Some(k) = s.spawnpads.iter().position(|&p| p == -1) {
            s.spawnpads[k] = pad as i16;
            s.numspawnpads += 1;
        }
    }
}

impl World {
    /// `ctc_init_props` (`capturethecase.inc:166`): the bases shuffled among
    /// the teams, each chr's team as a bit again (`chr->team = 1 << team`), a
    /// case on each playing team's home pad, and its base room lit.
    pub(crate) fn ctc_init_props(&mut self) {
        let mut teamsdone = [false; 4];
        self.mp.scenariodata.ctc.playercountsperteam = [0; 4];
        for i in 0..4 {
            loop {
                let t = (self.rng.random() % 4) as i16;
                self.mp.scenariodata.ctc.teamindexes[i] = t;
                if !teamsdone[t as usize] {
                    break;
                }
            }
            teamsdone[self.mp.scenariodata.ctc.teamindexes[i] as usize] = true;
        }
        let max = ctc_get_max_teams() as u8;
        let chrslots = self.setup.chrslots();
        for k in 0..MAX_MPCHRS {
            if chrslots & (1 << k) == 0 {
                continue;
            }
            while self.mp.teams[k] >= max {
                self.mp.teams[k] -= max;
            }
            let team = self.mp.teams[k];
            if let Some(ci) = self.chrs.iter().position(|c| c.mpslot == k) {
                self.chrs[ci].team = 1 << team;
            }
            self.mp.scenariodata.ctc.playercountsperteam[team as usize] += 1;
        }
        let d = &mut self.mp.scenariodata.ctc;
        for i in 0..4 {
            if d.playercountsperteam[i] == 0 {
                d.teamindexes[i] = -1;
            }
        }
        d.tokens = [None; 4];
        d.baserooms = [None; 4];
        for t in 0..4 {
            let d = &self.mp.scenariodata.ctc;
            if d.playercountsperteam[t] == 0 {
                continue;
            }
            let homepad = d.spawnpadsperteam[d.teamindexes[t] as usize].homepad as i32;
            let id = self.scenario_place_token(MODEL_CHRBRIEFCASE, WEAPON_BRIEFCASE2, homepad, 256);
            if let Some(o) = id.and_then(|id| self.props.get_mut(id)) {
                o.team = t as u8;
            }
            let room = id.and_then(|id| self.props.get(id)).and_then(|o| o.room);
            let d = &mut self.mp.scenariodata.ctc;
            d.tokens[t] = id.map(PropRef::Obj);
            d.baserooms[t] = room;
        }
        for t in 0..4 {
            let d = &self.mp.scenariodata.ctc;
            if d.playercountsperteam[t] != 0 {
                if let Some(r) = d.baserooms[t].and_then(|r| self.lights.rooms.get_mut(r as usize)) {
                    r.lightop = LIGHTOP_HIGHLIGHT;
                }
            }
        }
    }

    /// `ctc_choose_spawn_location`'s pads (`capturethecase.inc:460`): chr
    /// `ci`'s team's base's respawn pads, if it has any.
    pub(crate) fn ctc_spawn_pads(&self, ci: usize) -> Option<Vec<usize>> {
        let index = self.chr_team_index(ci);
        let d = &self.mp.scenariodata.ctc;
        let base = *d.teamindexes.get(index)?;
        let s = d.spawnpadsperteam.get(usize::try_from(base).ok()?)?;
        (s.numspawnpads > 0).then(|| s.spawnpads[..s.numspawnpads as usize].iter().map(|&p| p as usize).collect())
    }

    /// `bot_should_return_ctc_token`'s helper `bot_is_chrs_ctc_token_held`
    /// (`bot.c:2350`): chr `ci`'s team's case is being carried.
    pub(crate) fn ctc_is_chrs_token_held(&self, ci: usize) -> bool {
        let team = self.mp.teams[self.chrs[ci].mpslot] as usize;
        self.mp.scenariodata.ctc.tokens.get(team).copied().flatten().is_some_and(|t| t.chr().is_some())
    }

    /// Team `team`'s case, just dropped as object `id`, sent home: the token
    /// again, on its base's home pad (`scenario_handle_dropped_token`,
    /// `scenarios.c:1356`).
    pub(crate) fn ctc_send_token_home(&mut self, team: usize, id: u32) {
        let d = &self.mp.scenariodata.ctc;
        let base = d.teamindexes[team];
        let homepad = if base >= 0 { d.spawnpadsperteam[base as usize].homepad as i32 } else { -1 };
        self.mp.scenariodata.ctc.tokens[team] = Some(PropRef::Obj(id));
        let Some((pos, rot, room)) = usize::try_from(homepad).ok().and_then(|p| self.scenario_pad_basis(p)) else { return };
        let others: Vec<(glam::Vec3, crate::stage::PropGeo)> = self.props.objs.iter().filter(|x| x.id != id && !x.geos.is_empty()).map(|x| (x.pos, x.geos[0])).collect();
        let level = self.level.clone();
        let Some(o) = self.props.get_mut(id) else { return };
        // obj_free_projectile: it stops falling.
        o.projectile = None;
        o.team = team as u8;
        // mtx00016d58(-look, up) then mtx00015f04(model->scale).
        let mut mtx = glam::Mat4::from_mat3(rot);
        pd_core::math::scale3(&mut mtx, o.scale);
        let (p, r) = crate::props::setup::obj_place_3d(&level, o, &mtx, pos, &others);
        o.pos = p;
        o.realrot = r;
        o.room = room;
    }

    /// Capture the Case's half of `scenario_pick_up_briefcase`
    /// (`scenarios.c:1150`): touching your own team's case while carrying an
    /// enemy's scores (the enemy's case goes home); taking an enemy's case
    /// while carrying none makes you its carrier.
    pub(crate) fn ctc_pick_up_briefcase(&mut self, ci: usize, id: u32) -> i32 {
        let isbot = self.chrs[ci].aibot.is_some();
        let Some(o) = self.props.get(id).cloned() else { return TICKOP_NONE };
        let slot = self.chrs[ci].mpslot;
        let myteam = self.mp.teams[slot];
        let carrying = if isbot { self.ab(ci).hascase } else { self.inv_has_briefcase(ci) };
        let name = self.scenario_chr_name(ci);
        let shortname = self.bgun_get_short_name(WEAPON_BRIEFCASE2);
        let teamname = |w: &World, t: usize| format!("{}\n", w.setup.teamnames.get(t).map_or("", |s| s.as_str()));
        if o.team == myteam {
            if carrying {
                // A point: the enemy case goes home.
                self.mp.chrs[slot].numpoints += 1;
                let i = (0..4).find(|&i| self.mp.scenariodata.ctc.tokens[i] == Some(PropRef::Chr(ci))).unwrap_or(4);
                if isbot {
                    self.botinv_drop(ci, WEAPON_BRIEFCASE2, false);
                    self.ab_mut(ci).hascase = false;
                } else {
                    self.sound(crate::mp::scenario::htb::SFXNUM_05B8_MP_SCOREPOINT, 1.0);
                    self.weapon_create_for_chr_drop(ci, WEAPON_BRIEFCASE2);
                    self.players[ci].gun.p.inventory.inv_remove_item_by_num(WEAPON_BRIEFCASE2);
                }
                let tn = teamname(self, i);
                // "You captured\nthe %s%s", "%scaptured our\n%s", "%scaptured\nthe %s%s"
                let text1 = self.res.lang.get(l(4)).replacen("%s", &tn, 1).replacen("%s", &shortname, 1);
                let text2 = self.res.lang.get(l(5)).replacen("%s", &name, 1).replacen("%s", &shortname, 1);
                let text3 = self.res.lang.get(l(6)).replacen("%s", &name, 1).replacen("%s", &tn, 1).replacen("%s", &shortname, 1);
                for p in 0..self.players.len() {
                    // g_MpAllChrConfigPtrs[p]->team: player p's mpchrconfig.
                    let pteam = self.mp.teams[self.chrs[p].mpslot] as usize;
                    let text = if !isbot && p == ci {
                        &text1
                    } else if i == pteam {
                        &text2
                    } else {
                        &text3
                    };
                    let text = text.clone();
                    self.hudmsg_create_with_flags(p, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                }
            }
            return TICKOP_NONE;
        }
        if carrying {
            return TICKOP_NONE;
        }
        if isbot {
            self.ab_mut(ci).hascase = true;
            self.sound_at(crate::props::pickup::weapon_pickup_sound(o.weaponnum), 1.0, o.pos, crate::propsnd::DEFAULT_DISTS);
        }
        let team = o.team as usize;
        if let Some(t) = self.mp.scenariodata.ctc.tokens.get_mut(team) {
            *t = Some(PropRef::Chr(ci));
        }
        let tn = teamname(self, team);
        // "%shas the %s%s", "%shas our\n%s", "Got the %s%s"
        let text1 = self.res.lang.get(l(1)).replacen("%s", &name, 1).replacen("%s", &tn, 1).replacen("%s", &shortname, 1);
        let text2 = self.res.lang.get(l(2)).replacen("%s", &name, 1).replacen("%s", &shortname, 1);
        let text3 = self.res.lang.get(l(3)).replacen("%s", &tn, 1).replacen("%s", &shortname, 1);
        for p in 0..self.players.len() {
            let pteam = self.mp.teams[self.chrs[p].mpslot] as usize;
            let text = if !isbot && p == ci {
                &text3
            } else if team == pteam {
                &text2
            } else {
                &text1
            };
            let text = text.clone();
            self.hudmsg_create_with_flags(p, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
        }
        if isbot {
            self.ab_mut(ci).botinv_give_single_weapon(WEAPON_BRIEFCASE2);
            if let Some(o) = self.props.get_mut(id) {
                o.hidden |= OBJHFLAG_DELETING;
            }
            return TICKOP_NONE;
        }
        let gset = self.res.gset.clone();
        self.players[ci].gun.p.inventory.inv_give_weapons_by_prop(&gset, WEAPON_BRIEFCASE2, o.pad);
        TICKOP_FREE
    }

    /// `ctc_highlight_prop` (`capturethecase.inc:417`): a case lying anywhere,
    /// in its team's colour.
    pub(crate) fn ctc_highlight_prop(&self, p: PropRef) -> Option<[u8; 4]> {
        let o = self.props.get(p.obj()?)?;
        if o.ty == OBJTYPE_WEAPON && o.weaponnum == WEAPON_BRIEFCASE2 {
            let c = G_TEAM_COLOURS[o.team as usize & 7];
            return Some([(c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, 75]);
        }
        None
    }

    /// `ctc_highlight_room` (`capturethecase.inc:493`): a base in its team's
    /// colour (half and half with white).
    pub(crate) fn ctc_highlight_room(&self, room: u16) -> Option<[f32; 3]> {
        let i = self.mp.scenariodata.ctc.baserooms.iter().position(|&r| r == Some(room))?;
        let c = G_TEAM_COLOURS[i];
        let f = |x: u32| ((x & 0xff) as i32 + 0xff) as f32 * (1.0 / 512.0);
        Some([f(c >> 24), f(c >> 16), f(c >> 8)])
    }
}
