//! The handoff from the menus to a match. `pd_menu` builds a [`MatchSetup`] when
//! the player starts, and `pd_sim` builds the world from it; neither depends on
//! the other, and a test can write one by hand.
//!
//! The fields are PD's, in PD's encodings, from `struct mpsetup`,
//! `struct mpchrconfig`, `struct mpplayerconfig` and `struct mpbotconfig`
//! (`types.h:3965-4060`): the limits are stored as the menu leaves them (a time
//! limit of 9 is ten minutes; 60, 100 and 400 mean none, `ingame.c:148`),
//! bodies and heads are `g_MpBodies` / `g_MpHeads` indexes, the weapon slots are
//! `g_MpSetup.weapons` (`MPWEAPON_*`). The seed and the per-tick inputs are the
//! world's own inputs, not part of the setup.
//!
//! Source: the fields of the old repo's `pd_menu/mp.rs` `MpSetup` / `MpChrConfig`,
//! which ended in a text summary (`pd_menu/mod.rs` `start_match`).

use serde::{Deserialize, Serialize};

use crate::ids::*;

/// Chr slots (`g_MpSetup.chrslots`): four players, then eight simulants.
pub const MAX_PLAYERS: usize = 4;
pub const MAX_SIMULANTS: usize = 8;

/// `struct mpchrconfig`: what a player and a simulant have in common.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchChr {
    pub name: String,
    /// `g_MpBodies` index (`MPBODY_*`).
    pub mpbodynum: u8,
    /// `g_MpHeads` index (`MPHEAD_*`).
    pub mpheadnum: u8,
    /// 0..7 when teams are on.
    pub team: u8,
}

/// `struct mpplayerconfig`: one human player.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchPlayer {
    /// The chr slot, 0..3 (`chrslots` bit).
    pub slot: u8,
    pub chr: MatchChr,
    /// `CONTROLMODE_*` (1.1 is 0).
    pub controlmode: u8,
    /// `OPTION_*` (auto-aim, look-ahead, sight on screen, ...).
    pub options: u16,
    /// `handicap`: 0..255, 128 is none (`mp_get_handicap_mult`).
    pub handicap: u8,
}

impl MatchPlayer {
    /// `mp_init_player`'s options (`mplayer.c:379`).
    pub const DEFAULT_OPTIONS: u16 = OPTION_LOOKAHEAD | OPTION_SIGHTONSCREEN | OPTION_AUTOAIM | OPTION_AMMOONSCREEN | OPTION_SHOWGUNFUNCTION | OPTION_HEADROLL | OPTION_0100 | OPTION_ALWAYSSHOWTARGET | OPTION_SHOWZOOMRANGE;

    pub fn has_option(&self, option: u16) -> bool {
        self.options & option != 0
    }
}

impl Default for MatchPlayer {
    /// A fresh player config (`mp_init_player`, `mplayer.c:370`): control style
    /// 1.1, PD's default options, no handicap.
    fn default() -> Self {
        MatchPlayer { slot: 0, chr: MatchChr::default(), controlmode: 0, options: Self::DEFAULT_OPTIONS, handicap: 128 }
    }
}

/// `struct mpbotconfig`: one simulant.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchSimulant {
    /// The chr slot, 4..11.
    pub slot: u8,
    pub chr: MatchChr,
    /// `BOTTYPE_*`.
    pub bottype: u8,
    /// `BOTDIFF_*`.
    pub difficulty: u8,
}

/// `struct mpsetup` plus the chrs taking part: everything a match starts from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchSetup {
    /// `STAGE_MP_*`.
    pub stagenum: u8,
    /// `MPSCENARIO_*`.
    pub scenario: u8,
    /// `MPOPTION_*` flags.
    pub options: u32,
    /// Minutes − 1; 60 is no limit.
    pub timelimit: u8,
    /// Points − 1; 100 is no limit.
    pub scorelimit: u8,
    /// Team points − 1; 400 is no limit.
    pub teamscorelimit: u16,
    /// `g_MpSetup.weapons`: the six slots, `MPWEAPON_*`.
    pub weapons: [u8; 6],
    pub players: Vec<MatchPlayer>,
    pub simulants: Vec<MatchSimulant>,
    /// The team names (`g_BossFile.teamnames`).
    pub teamnames: Vec<String>,
}

impl Default for MatchSetup {
    /// `mp_init` (`mplayer.c:466`) and `mp_init_limits` (`:363`), with no chrs.
    fn default() -> Self {
        MatchSetup {
            stagenum: STAGE_MP_SKEDAR,
            scenario: MPSCENARIO_COMBAT,
            options: MPOPTION_DISPLAYTEAM
                | MPOPTION_KILLSSCORE
                | MPOPTION_HTB_HIGHLIGHTBRIEFCASE
                | MPOPTION_HTB_SHOWONRADAR
                | MPOPTION_CTC_SHOWONRADAR
                | MPOPTION_KOH_HILLONRADAR
                | MPOPTION_KOH_MOBILEHILL
                | MPOPTION_00010000
                | MPOPTION_HTM_HIGHLIGHTTERMINAL
                | MPOPTION_HTM_SHOWONRADAR
                | MPOPTION_PAC_HIGHLIGHTTARGET
                | MPOPTION_PAC_SHOWONRADAR,
            timelimit: 9,
            scorelimit: 9,
            teamscorelimit: 19,
            weapons: [0; 6],
            players: Vec::new(),
            simulants: Vec::new(),
            teamnames: Vec::new(),
        }
    }
}

impl MatchSetup {
    /// `lv_get_slow_motion_type` (`lv.c:1945`) in a match: the option's.
    pub fn slowmotion(&self) -> crate::lv::SlowMotion {
        if self.options & MPOPTION_SLOWMOTION_ON != 0 {
            crate::lv::SlowMotion::On
        } else if self.options & MPOPTION_SLOWMOTION_SMART != 0 {
            crate::lv::SlowMotion::Smart
        } else {
            crate::lv::SlowMotion::Off
        }
    }

    /// The time limit in minutes, or `None` for no limit.
    pub fn time_limit_minutes(&self) -> Option<u32> {
        (self.timelimit < 60).then_some(self.timelimit as u32 + 1)
    }

    /// The score limit in points, or `None` for no limit.
    pub fn score_limit(&self) -> Option<u32> {
        (self.scorelimit < 100).then_some(self.scorelimit as u32 + 1)
    }

    /// The team score limit, or `None` for no limit.
    pub fn team_score_limit(&self) -> Option<u32> {
        (self.teamscorelimit < 400).then_some(self.teamscorelimit as u32 + 1)
    }

    pub fn teams_enabled(&self) -> bool {
        self.options & MPOPTION_TEAMSENABLED != 0
    }

    /// `g_MpSetup.chrslots`: bits 0..3 players, 4..11 simulants.
    pub fn chrslots(&self) -> u16 {
        self.players.iter().map(|p| 1u16 << p.slot).chain(self.simulants.iter().map(|s| 1u16 << s.slot)).fold(0, |a, b| a | b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pds_defaults_and_limit_encodings() {
        let mut s = MatchSetup::default();
        assert_eq!((s.time_limit_minutes(), s.score_limit(), s.team_score_limit()), (Some(10), Some(10), Some(20)));
        s.timelimit = 60;
        s.scorelimit = 100;
        s.teamscorelimit = 400;
        assert_eq!((s.time_limit_minutes(), s.score_limit(), s.team_score_limit()), (None, None, None));
        assert!(!s.teams_enabled());
        s.players.push(MatchPlayer { slot: 0, handicap: 128, ..Default::default() });
        s.simulants.push(MatchSimulant { slot: 4, difficulty: BOTDIFF_DARK, ..Default::default() });
        assert_eq!(s.chrslots(), 0x11);
        let back: MatchSetup = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
