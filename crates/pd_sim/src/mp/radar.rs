//! What `radar.c`'s `radar_render` draws for a player, worked out in the sim
//! (the drawing is `pd_render::radar`): the dots in PD's order (the other
//! humans, the simulants, the scenario's own marks, the player itself), each
//! with its offset from the player and its two colours, and the scenarios'
//! `radarextra` / `radarchr` callbacks (`scenarios.c:684`, `:698`): Hold the
//! Briefcase's case or its carrier, Hacker Central's uplink and terminal, Pop a
//! Cap's victim, King of the Hill's hill, Capture the Case's cases and
//! carriers. Also Display Team's line colour (`scenario_render_hud`).
//!
//! No R-Tracker in the Combat Simulator (`DEVICE_RTRACKER` is a solo device),
//! so `radar_render_r_tracked_props` never runs.

use glam::Vec3;
use pd_core::ids::*;

use super::scenario::PropRef;
use super::{radar_get_team_index, G_TEAM_COLOURS};
use crate::world::World;

/// `var80087ce4` (`radar.c:36`): the team colours as RGBA5551 fill words
/// (Display Team's line).
pub const VAR80087CE4: [u32; 8] = [0xf801f801, 0xffc1ffc1, 0x003f003f, 0xf83ff83f, 0x07ff07ff, 0xfc55fc55, 0xfc63fc63, 0x8a158a15];

/// One of `radar_draw_dot`'s marks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadarDot {
    /// The prop's position less the player's (`dist`).
    pub dist: Vec3,
    /// `colour1`, `colour2` (RGBA with alpha 0; the drawing adds it) and
    /// `swapcolours`: which is the outline and which the fill.
    pub colour1: u32,
    pub colour2: u32,
    pub swapcolours: bool,
    /// `prop == g_Vars.currentplayer->prop`: the player's own box.
    pub isself: bool,
}

/// `radar_render`'s input for one player.
#[derive(Clone, Debug, PartialEq)]
pub struct RadarIn {
    /// The player's `vv_theta` (degrees), which turns the dots.
    pub theta: f32,
    /// `g_RadarYIndicatorsEnabled`: the up/down triangles.
    pub yindicators: bool,
    pub dots: Vec<RadarDot>,
}

/// Green, the colour of a dot with teams off (`0x00ff0000`).
const GREEN: u32 = 0x00ff0000;

impl World {
    /// `radar_render` (`radar.c:244`) for player `pi`: nothing with the
    /// match's No Radar, the player's radar option off, its menu open or dead.
    pub fn radar_render_in(&self, pi: usize) -> Option<RadarIn> {
        if self.setup.options & MPOPTION_NORADAR != 0 {
            return None;
        }
        let displayoptions = self.setup.players.get(pi).map_or(MPDISPLAYOPTION_RADAR, |p| p.chr.displayoptions);
        if displayoptions & MPDISPLAYOPTION_RADAR == 0 {
            return None;
        }
        let p = self.players.get(pi)?;
        if self.mp.menuopen.get(pi).copied().unwrap_or(false) || p.isdead {
            return None;
        }
        let teams = self.setup.teams_enabled();
        let me = self.chrs[pi].pos;
        let mut dots = Vec::new();
        // The other humans, then the simulants.
        let order = (0..self.players.len()).filter(|&i| i != pi && !self.players[i].isdead).chain((self.players.len()..self.chrs.len()).filter(|&i| !self.chr_is_dead(i)));
        for i in order {
            let c = &self.chrs[i];
            if c.cloak.cloaked {
                continue;
            }
            if let Some(d) = self.scenario_radar_chr(pi, i) {
                dots.push(d);
                continue;
            }
            let colour = if teams { G_TEAM_COLOURS[radar_get_team_index(c.team)] } else { GREEN };
            dots.push(RadarDot { dist: c.pos - me, colour1: colour, colour2: 0, swapcolours: false, isself: false });
        }
        dots.extend(self.scenario_radar_extra(pi));
        // The player itself.
        if let Some(d) = self.scenario_radar_chr(pi, pi) {
            dots.push(d);
        } else {
            let colour = if teams { G_TEAM_COLOURS[radar_get_team_index(self.chrs[pi].team)] } else { GREEN };
            dots.push(RadarDot { dist: Vec3::ZERO, colour1: colour, colour2: 0, swapcolours: false, isself: true });
        }
        Some(RadarIn { theta: p.theta, yindicators: true, dots })
    }

    /// `scenario_radar_chr` (`scenarios.c:698`): the scenario's own mark for
    /// chr `ci` (the case's carrier, the victim, ...), which takes the place of
    /// its ordinary dot.
    fn scenario_radar_chr(&self, pi: usize, ci: usize) -> Option<RadarDot> {
        let o = self.setup.options;
        let teams = self.setup.teams_enabled();
        let sd = &self.mp.scenariodata;
        let me = self.chrs[pi].pos;
        let c = &self.chrs[ci];
        let teamcolour = |c: &crate::chr::Chr| if teams { G_TEAM_COLOURS[radar_get_team_index(c.team)] } else { GREEN };
        let dot = |colour1: u32, colour2: u32| RadarDot { dist: c.pos - me, colour1, colour2, swapcolours: true, isself: ci == pi };
        match self.setup.scenario {
            // htb_radar_chr (`holdthebriefcase.inc:485`).
            MPSCENARIO_HOLDTHEBRIEFCASE if o & MPOPTION_HTB_SHOWONRADAR != 0 && sd.htb.token == Some(PropRef::Chr(ci)) => Some(dot(teamcolour(c), 0)),
            // htm_radar_chr (`hackthatmac.inc:737`).
            MPSCENARIO_HACKERCENTRAL if o & MPOPTION_HTM_SHOWONRADAR != 0 && sd.htm.uplink == Some(PropRef::Chr(ci)) => Some(dot(teamcolour(c), 0)),
            // pac_radar_chr (`popacap.inc:374`).
            MPSCENARIO_POPACAP if o & MPOPTION_PAC_SHOWONRADAR != 0 && sd.pac.victimindex >= 0 && sd.pac.victims[sd.pac.victimindex as usize] as usize == ci => Some(dot(teamcolour(c), 0)),
            // ctc_radar_chr (`capturethecase.inc:394`): the case's team colour
            // ringed in the carrier's.
            MPSCENARIO_CAPTURETHECASE if o & MPOPTION_CTC_SHOWONRADAR != 0 => {
                let i = (0..self.scenario_get_max_teams() as usize).find(|&i| sd.ctc.tokens[i] == Some(PropRef::Chr(ci)))?;
                Some(dot(G_TEAM_COLOURS[i], G_TEAM_COLOURS[radar_get_team_index(c.team)]))
            }
            _ => None,
        }
    }

    /// `scenario_radar_extra` (`scenarios.c:684`): the marks that are not chrs.
    fn scenario_radar_extra(&self, pi: usize) -> Vec<RadarDot> {
        let o = self.setup.options;
        let sd = &self.mp.scenariodata;
        let me = self.chrs[pi].pos;
        let mut out = Vec::new();
        let at = |pos: Vec3, colour1: u32, colour2: u32| RadarDot { dist: pos - me, colour1, colour2, swapcolours: true, isself: false };
        match self.setup.scenario {
            // htb_radar_extra: the case lying somewhere (`g_ScenarioData.htb.pos`).
            MPSCENARIO_HOLDTHEBRIEFCASE if o & MPOPTION_HTB_SHOWONRADAR != 0 => {
                if let Some(PropRef::Obj(_)) = sd.htb.token {
                    out.push(at(sd.htb.pos, GREEN, 0));
                }
            }
            // htm_radar_extra: the uplink lying somewhere, and the terminal
            // (green, or with teams on the team downloading it).
            MPSCENARIO_HACKERCENTRAL if o & MPOPTION_HTM_SHOWONRADAR != 0 => {
                if let Some(pos) = sd.htm.uplink.filter(|u| u.chr().is_none()).and_then(|u| self.scenario_prop_pos(u)) {
                    out.push(at(pos, GREEN, 0));
                }
                for t in &sd.htm.terminals {
                    let Some(pos) = t.prop.and_then(|id| self.props.get(id)).map(|o| o.pos) else { continue };
                    let colour = if t.team != 255 && self.setup.teams_enabled() { G_TEAM_COLOURS[radar_get_team_index(t.team)] } else { GREEN };
                    out.push(at(pos, colour, 0));
                }
            }
            // koh_radar_extra: the hill unless it moves, green or its team's.
            MPSCENARIO_KINGOFTHEHILL if o & MPOPTION_KOH_HILLONRADAR != 0 && !sd.koh.movehill => {
                let colour = if sd.koh.occupiedteam == -1 { GREEN } else { G_TEAM_COLOURS[sd.koh.occupiedteam as usize & 7] };
                out.push(at(sd.koh.hillpos, colour, 0));
            }
            // ctc_radar_extra: each team's case lying somewhere.
            MPSCENARIO_CAPTURETHECASE if o & MPOPTION_CTC_SHOWONRADAR != 0 => {
                for i in 0..self.scenario_get_max_teams() as usize {
                    if let Some(pos) = sd.ctc.tokens[i].filter(|t| t.chr().is_none()).and_then(|t| self.scenario_prop_pos(t)) {
                        out.push(at(pos, G_TEAM_COLOURS[i], 0));
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// Display Team's line (`scenario_render_hud`, `scenarios.c:589`): with
    /// teams on, the match's Display Team and 2+ players, player `pi`'s team
    /// colour as a 5551 fill word, drawn along the edge its view shares.
    pub fn scenario_display_team(&self, pi: usize) -> Option<u32> {
        let o = self.setup.options;
        (o & MPOPTION_TEAMSENABLED != 0 && o & MPOPTION_DISPLAYTEAM != 0 && self.players.len() >= 2).then(|| VAR80087CE4[radar_get_team_index(self.chrs[pi].team)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::PlayerInput;

    /// Two players and a simulant on Complex: each player's radar has the
    /// other player and the simulant as dots and itself as the box, last;
    /// No Radar and a dead player have none.
    #[test]
    fn the_radar_lists_the_others_then_the_player() {
        let (stage, level) = crate::testutil::complex_arc();
        let mut setup = crate::harness::setup(2, 1, BOTDIFF_NORMAL);
        setup.stagenum = stage.stagenum;
        let mut w = World::new(setup, stage, level, crate::testutil::res(), 3).unwrap();
        let idle = vec![PlayerInput::default(); 2];
        for _ in 0..10 {
            w.step(4, &idle);
        }
        let r = w.radar_render_in(0).expect("a radar");
        assert_eq!(r.dots.len(), 3);
        assert!(r.dots[..2].iter().all(|d| !d.isself && d.colour1 == GREEN && !d.swapcolours));
        assert_eq!(r.dots[0].dist, w.chrs[1].pos - w.chrs[0].pos, "the other player first");
        assert_eq!(r.dots[1].dist, w.chrs[2].pos - w.chrs[0].pos, "then the simulant");
        assert!(r.dots[2].isself && r.dots[2].dist == Vec3::ZERO);
        w.setup.options |= MPOPTION_NORADAR;
        assert!(w.radar_render_in(0).is_none());
        w.setup.options &= !MPOPTION_NORADAR;
        w.players[1].isdead = true;
        assert!(w.radar_render_in(1).is_none(), "no radar while dead");
        assert_eq!(w.radar_render_in(0).unwrap().dots.len(), 2, "no dot for a dead player");
    }
}
