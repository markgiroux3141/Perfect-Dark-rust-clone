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
    /// `MPDISPLAYOPTION_*` (highlight players, pickups or teams; the radar).
    #[serde(default)]
    pub displayoptions: u8,
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
    /// The player file's statistics, which the end of the match adds to.
    #[serde(default)]
    pub career: MpCareer,
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
    /// 1.1, PD's default options, the radar and the team highlight on
    /// (`mplayer.c:408`), no handicap.
    fn default() -> Self {
        let chr = MatchChr { displayoptions: MPDISPLAYOPTION_RADAR | MPDISPLAYOPTION_HIGHLIGHTTEAMS, ..MatchChr::default() };
        MatchPlayer { slot: 0, chr, controlmode: 0, options: Self::DEFAULT_OPTIONS, handicap: 128, career: MpCareer::default() }
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
    /// A challenge is being played (`g_BossFile.locktype == MPLOCKTYPE_CHALLENGE`).
    #[serde(default)]
    pub challenge: bool,
    /// `g_Vars.mphilltime`: King of the Hill's seconds to score, less 10 (the
    /// Hill Options' Time slider; `mp_init` sets 10, 20 s).
    #[serde(default = "default_mphilltime")]
    pub mphilltime: u8,
    /// `g_ScreenSplit` (`SCREENSPLIT_*`): two players one above the other or
    /// side by side (the Combat Simulator's Screen Split option).
    #[serde(default)]
    pub screensplit: u8,
    /// The Soundtrack settings the match's tunes come from.
    #[serde(default)]
    pub music: MatchMusic,
}

/// The Soundtrack settings a match chooses its tunes from (`g_BossFile`'s
/// `tracknum`, `usingmultipletunes` and `multipletracknums`), over the
/// unlocked `g_MpTracks` in slot order (`mp_get_track_num_at_slot_index`).
/// The default has no tracks: no music, and no draws from the RNG for it
/// (the harness's setups).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchMusic {
    /// `mp_get_current_track_slot_num()`: the chosen tune's slot, or -1 for
    /// a random one.
    pub slot: i32,
    /// `mp_get_using_multiple_tunes()`.
    pub usingmultipletunes: bool,
    pub tracks: Vec<MatchTrack>,
}

/// An unlocked `g_MpTracks` row (`mplayer.c:2679`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchTrack {
    /// Its `g_MpTracks` index.
    pub tracknum: i32,
    /// `MUSIC_*`.
    pub musicnum: i32,
    /// `duration`, in seconds.
    pub duration: i32,
    /// `mp_is_multi_track_slot_enabled`: one of the multiple tunes.
    pub enabled: bool,
}

fn default_mphilltime() -> u8 {
    10
}

/// `struct mpweapon` (`types.h:4933`): a row of `g_MpWeapons`
/// ([`crate::mpweapons::MP_WEAPONS`]). `model` is a `MODEL_*` number (the
/// weapon's third-person model), `hasweapon` whether a weapon location puts
/// the weapon on its pad or only its ammo crates (the grenades and mines).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MpWeapon {
    pub weaponnum: i32,
    pub priammotype: i32,
    pub priammoqty: i32,
    pub secammotype: i32,
    pub secammoqty: i32,
    pub hasweapon: i32,
    pub unlockfeature: i32,
    pub model: i32,
    pub extrascale: i32,
}

/// `g_MpWeapons[g_MpSetup.weapons[slot]]`.
pub fn mp_weapon(weapons: &[u8; 6], slot: usize) -> &'static MpWeapon {
    let m = crate::mpweapons::MP_WEAPONS;
    &m[(weapons[slot] as usize).min(m.len() - 1)]
}

/// `mp_get_mp_weapon_by_location` (`mplayer.c:942`): weapon location
/// `locationindex` (`WEAPON_MPLOCATION00 + n` in a setup file) takes the
/// set's slots in turn, skipping the disabled ones and wrapping; with every
/// slot disabled it is `g_MpWeapons[0]` (nothing).
pub fn mp_get_mp_weapon_by_location(weapons: &[u8; 6], locationindex: i32) -> &'static MpWeapon {
    let m = crate::mpweapons::MP_WEAPONS;
    let mut v0 = locationindex + 1;
    let mut slot = 0;
    let mut a2 = v0;
    let mut mpweaponnum = 0usize;
    while v0 > 0 {
        mpweaponnum = weapons[slot] as usize;
        if m[mpweaponnum.min(m.len() - 1)].weaponnum != WEAPON_DISABLED as i32 {
            v0 -= 1;
        }
        if v0 > 0 {
            slot += 1;
            if slot >= weapons.len() {
                slot = 0;
                if a2 == v0 {
                    mpweaponnum = 0;
                    v0 = 0;
                }
                a2 = v0;
            }
        }
    }
    &m[mpweaponnum.min(m.len() - 1)]
}

/// The `MPWEAPON_*` index of `weaponnum` in `g_MpWeapons` (the menus' slot
/// value for it).
pub fn mpweapon_index(weaponnum: u8) -> Option<u8> {
    crate::mpweapons::MP_WEAPONS.iter().position(|w| w.weaponnum == weaponnum as i32).map(|i| i as u8)
}

/// `mp_has_shield` (`botinv.c:425`).
pub fn mp_has_shield(weapons: &[u8; 6]) -> bool {
    (0..weapons.len()).any(|i| mp_weapon(weapons, i).weaponnum == WEAPON_MPSHIELD as i32)
}

/// `mp_get_weapon_slot_by_weapon_num` (`botinv.c:443`): the first slot holding
/// `weaponnum`, if any.
pub fn mp_get_weapon_slot_by_weapon_num(weapons: &[u8; 6], weaponnum: i32) -> Option<usize> {
    (0..weapons.len()).find(|&i| mp_weapon(weapons, i).weaponnum == weaponnum)
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
            challenge: false,
            mphilltime: 10,
            screensplit: SCREENSPLIT_HORIZONTAL,
            music: MatchMusic::default(),
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

    /// `mp_calculate_team_score_limit` (`mplayer.c:578`) for this setup.
    pub fn mp_calculate_team_score_limit(&self) -> i32 {
        mp_calculate_team_score_limit(self.teamscorelimit, self.challenge, self.scenario, self.players.len())
    }

    pub fn teams_enabled(&self) -> bool {
        self.options & MPOPTION_TEAMSENABLED != 0
    }

    /// `g_MpSetup.chrslots`: bits 0..3 players, 4..11 simulants.
    pub fn chrslots(&self) -> u16 {
        self.players.iter().map(|p| 1u16 << p.slot).chain(self.simulants.iter().map(|s| 1u16 << s.slot)).fold(0, |a, b| a | b)
    }
}

/// `mp_calculate_team_score_limit` (`mplayer.c:578`): a challenge's Combat or
/// King of the Hill team limit grows with the number of players.
pub fn mp_calculate_team_score_limit(teamscorelimit: u16, challenge: bool, scenario: u8, numplayers: usize) -> i32 {
    let limit = teamscorelimit as i32;
    if challenge && teamscorelimit != 400 && (scenario == MPSCENARIO_COMBAT || scenario == MPSCENARIO_KINGOFTHEHILL) {
        return match numplayers {
            2 => limit * 2 + 1,
            3 => (limit * 5 + 5) / 2 - 1,
            4 => limit * 3 + 2,
            _ => limit,
        };
    }
    limit
}

/// `MAX_MPCHRS`: four players and eight simulants, by chr slot.
pub const MAX_MPCHRS: usize = 12;
/// `MAX_TEAMS`.
pub const MAX_TEAMS: usize = 8;

/// The match's counters in `struct mpchrconfig` (`types.h:4012`), which
/// `mp_reset_mpchrconfig_for_match` (`mplayer.c:128`) zeroes and
/// `mpstats_record_death` (`mpstats.c:238`) counts, by chr slot (0..3 the
/// players, 4..11 the simulants, as `g_MpSetup.chrslots` numbers them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MpChrStats {
    /// `killcounts[slot]`: how often this chr killed chr slot `slot`; its own
    /// slot counts its suicides.
    pub killcounts: [i16; MAX_MPCHRS],
    /// Every death, suicides included.
    pub numdeaths: i16,
    pub numpoints: i16,
    /// Written by [`mp_get_player_rankings`]: 0 is first (in a team game, the
    /// team's place).
    pub placement: u8,
    pub rankablescore: u32,
}

/// The statistics half of `struct mpplayerconfig` (`types.h:4029`): the player
/// file's totals, which `mp_calculate_awards` adds each match to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MpCareer {
    pub kills: u32,
    pub deaths: u32,
    pub gamesplayed: u32,
    pub gameswon: u32,
    pub gameslost: u32,
    /// Seconds.
    pub time: u32,
    /// Units of 100 m (a match adds `playerstats.distance / 10000` cm).
    pub distance: u32,
    /// Thousandths.
    pub accuracy: u32,
    /// Tenths of a health bar.
    pub damagedealt: u32,
    pub painreceived: u32,
    pub headshots: u32,
    pub ammoused: u32,
    pub accuracymedals: u32,
    pub headshotmedals: u32,
    pub killmastermedals: u32,
    pub survivormedals: u32,
}

impl Default for MpCareer {
    /// `mp_player_set_defaults` (`mplayer.c:370`): everything zero, accuracy 100%.
    fn default() -> Self {
        MpCareer {
            kills: 0,
            deaths: 0,
            gamesplayed: 0,
            gameswon: 0,
            gameslost: 0,
            time: 0,
            distance: 0,
            accuracy: 1000,
            damagedealt: 0,
            painreceived: 0,
            headshots: 0,
            ammoused: 0,
            accuracymedals: 0,
            headshotmedals: 0,
            killmastermedals: 0,
            survivormedals: 0,
        }
    }
}

/// `mp_calculate_player_title` (`mplayer.c:1537`, the NTSC 1.0+ tiers): one
/// tally for each tier a counter reaches, over ten counters, the sum (at most
/// 100) divided by five, at most `MPPLAYERTITLE_PERFECT`.
pub fn mp_calculate_player_title(c: &MpCareer) -> u8 {
    const TIERS: [u32; 10] = [2, 4, 8, 16, 28, 60, 100, 150, 210, 300];
    // MULT(val) is val * 3 from NTSC 1.0.
    let counters: [(u32, u32); 10] = [
        (c.kills, 20 * 3),
        (c.gameswon, 3),
        (c.accuracymedals, 3),
        (c.headshotmedals, 3),
        (c.killmastermedals, 3),
        (c.time, 1200 * 3),
        (c.distance, 100 * 3),
        (c.damagedealt, 3),
        (c.ammoused, 500 * 3),
        (c.survivormedals, 3),
    ];
    let sum: u32 = counters.iter().map(|&(value, mult)| TIERS.iter().take_while(|&&t| value >= t * mult).count() as u32).sum();
    ((sum.min(100) / 5) as u8).min(MPPLAYERTITLE_PERFECT)
}

/// What `mp_calculate_awards` (`mplayer.c:1962`) leaves in a player's
/// mpplayerconfig at the end of a match: the file's statistics with the match
/// added, the medals won (`MEDAL_*`) and the title worked out again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MpPlayerResult {
    /// The player's chr slot.
    pub slot: usize,
    pub career: MpCareer,
    pub medals: u8,
    pub title: u8,
}

/// `struct ranking`: one row of the rankings, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ranking {
    /// `mpchr`: a player row's chr slot; `None` in a team row.
    pub mpchr: Option<usize>,
    /// A team row's team; in a player row the chr slot (PD stores
    /// `chrnums[j]` here).
    pub teamnum: usize,
    pub positionindex: usize,
    pub score: i32,
}

/// What the scenario's score callback reads beyond the mpchrconfigs: the
/// scenario and its options, and Hacker Central's and Pop a Cap's own counters
/// (`g_ScenarioData.htm.numpoints`, `.pac.killcounts`, `.pac.survivalcounts`).
/// PD keeps those by chr index and looks each chr slot's up
/// (`mp_chrslot_to_chrindex`); they are held here by chr slot, looked up the
/// same way by whoever fills them in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScenarioScores {
    /// `MPSCENARIO_*`.
    pub scenario: u8,
    /// `g_MpSetup.options` (`MPOPTION_KILLSSCORE`).
    pub options: u32,
    pub htm_numpoints: [i32; MAX_MPCHRS],
    pub pac_killcounts: [i32; MAX_MPCHRS],
    pub pac_survivalcounts: [i32; MAX_MPCHRS],
}

impl ScenarioScores {
    /// A scenario with no counters of its own yet.
    pub fn new(scenario: u8, options: u32) -> ScenarioScores {
        ScenarioScores { scenario, options, ..Default::default() }
    }
}

/// What the scoring reads of `g_MpSetup` and the mpchrconfigs: the chrs taking
/// part, whether teams are on, each chr slot's team and counters, and the
/// scenario's own.
#[derive(Clone, Copy, Debug)]
pub struct MpScoring<'a> {
    pub chrslots: u16,
    pub teams_enabled: bool,
    /// `mpchrconfig.team` by chr slot.
    pub teams: &'a [u8; MAX_MPCHRS],
    pub stats: &'a [MpChrStats; MAX_MPCHRS],
    pub scenario: &'a ScenarioScores,
}

/// `(score + 0x8000) << 16 | (0xffff - deaths)`: the score, fewer deaths
/// breaking a tie (`mp_get_player_rankings`, `mp_calculate_team_score`).
fn rankable(score: i32, deaths: i32) -> u32 {
    (score.wrapping_add(0x8000).wrapping_shl(16) | (0xffff - deaths)) as u32
}

impl MpScoring<'_> {
    /// The kills half every score shares (`scenarios.c:661`): a kill scores a
    /// point, a suicide or a teammate's death costs one; `teamcheck` is
    /// whether teammates count against (King of the Hill and Capture the Case
    /// check always, the others with teams on).
    fn kills_score(&self, chrnum: usize, teamcheck: bool) -> i32 {
        let mpchr = &self.stats[chrnum];
        let mut score = 0i32;
        for i in 0..MAX_MPCHRS {
            let k = mpchr.killcounts[i] as i32;
            if i == chrnum {
                score -= k;
            } else if teamcheck {
                if self.teams[i] == self.teams[chrnum] {
                    score -= k;
                } else {
                    score += k;
                }
            } else {
                score += k;
            }
        }
        score
    }

    /// `scenario_calculate_player_score` (`scenarios.c:651`): the scenario's
    /// callback, else Combat's (the kills). The scenarios score their own
    /// points, plus the kills with Kills Score on: Hold the Briefcase and King
    /// of the Hill a point each (`htb_calculate_player_score`,
    /// `holdthebriefcase.inc:440`; `koh_`, `kingofthehill.inc:605`), Hacker
    /// Central two a download (`htm_`, `hackthatmac.inc:619`), Pop a Cap two a
    /// victim popped and one a minute survived (`pac_`, `popacap.inc:334`),
    /// Capture the Case three a capture (`ctc_`, `capturethecase.inc:346`).
    /// Also the chr's deaths.
    pub fn scenario_calculate_player_score(&self, chrnum: usize) -> (i32, i32) {
        let mpchr = &self.stats[chrnum];
        let sc = self.scenario;
        let killsscore = sc.options & MPOPTION_KILLSSCORE != 0;
        let kills = |teamcheck: bool| if killsscore { self.kills_score(chrnum, teamcheck) } else { 0 };
        let score = match sc.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE => mpchr.numpoints as i32 + kills(self.teams_enabled),
            MPSCENARIO_HACKERCENTRAL => sc.htm_numpoints[chrnum] * 2 + kills(self.teams_enabled),
            MPSCENARIO_POPACAP => sc.pac_killcounts[chrnum] * 2 + sc.pac_survivalcounts[chrnum] + kills(self.teams_enabled),
            MPSCENARIO_KINGOFTHEHILL => mpchr.numpoints as i32 + kills(true),
            MPSCENARIO_CAPTURETHECASE => mpchr.numpoints as i32 * 3 + kills(true),
            _ => self.kills_score(chrnum, self.teams_enabled),
        };
        (score, mpchr.numdeaths as i32)
    }

    /// `mp_calculate_team_score` (`mplayer.c:773`): the team's rankable score
    /// (0 for a team with no chrs) and its summed score (unset then).
    pub fn mp_calculate_team_score(&self, teamnum: usize) -> (u32, Option<i32>) {
        let (mut teamscore, mut teamdeaths, mut exists) = (0i32, 0i32, false);
        for i in 0..MAX_MPCHRS {
            if self.chrslots & (1 << i) != 0 && self.teams[i] as usize == teamnum {
                let (score, deaths) = self.scenario_calculate_player_score(i);
                exists = true;
                teamscore += score;
                teamdeaths += deaths;
            }
        }
        if exists {
            (rankable(teamscore, teamdeaths), Some(teamscore))
        } else {
            (0, None)
        }
    }

    /// `mp_get_team_rankings` (`mplayer.c:810`): the teams that have chrs, best
    /// first; of equal rankable scores the lower team number goes first.
    pub fn mp_get_team_rankings(&self) -> Vec<Ranking> {
        let mut apparentscores = [-8000i32; MAX_TEAMS];
        let mut rankablescores = [0u32; MAX_TEAMS];
        for i in 0..MAX_TEAMS {
            let (r, s) = self.mp_calculate_team_score(i);
            rankablescores[i] = r;
            if let Some(s) = s {
                apparentscores[i] = s;
            }
        }
        let mut out = Vec::new();
        loop {
            let mut best = 0u32;
            let mut thisteamnum: i32 = -8000;
            for i in 0..MAX_TEAMS {
                let t = 7 - i;
                if apparentscores[t] > -8000 && rankablescores[t] >= best {
                    thisteamnum = t as i32;
                    best = rankablescores[t];
                }
            }
            if thisteamnum <= -8000 {
                return out;
            }
            let t = thisteamnum as usize;
            out.push(Ranking { mpchr: None, teamnum: t, positionindex: out.len() + 1, score: apparentscores[t] });
            apparentscores[t] = -8000;
        }
    }
}

/// `mp_get_player_rankings`' (`mplayer.c:640`) answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerRankings {
    pub rankings: Vec<Ranking>,
    /// `g_MpLockInfo.lastwinner` / `lastloser`: the best and the worst placed
    /// human player's slot, or -1.
    pub lastwinner: i32,
    pub lastloser: i32,
}

/// `mp_get_player_rankings` (`mplayer.c:640`): every chr taking part, best
/// first (a later slot goes after the ones it ties with). It writes each chr's
/// `placement` and `rankablescore`: in a team game, the team's place and 255 −
/// that place.
pub fn mp_get_player_rankings(chrslots: u16, teams_enabled: bool, teams: &[u8; MAX_MPCHRS], stats: &mut [MpChrStats; MAX_MPCHRS], scenario: &ScenarioScores) -> PlayerRankings {
    let scoring = MpScoring { chrslots, teams_enabled, teams, stats, scenario };
    let teamrankings = if teams_enabled { scoring.mp_get_team_rankings() } else { Vec::new() };
    let mut rows: Vec<(u32, i32, usize)> = Vec::new();
    for i in 0..MAX_MPCHRS {
        if chrslots & (1 << i) != 0 {
            let (score, deaths) = scoring.scenario_calculate_player_score(i);
            let r = rankable(score, deaths);
            let dstindex = rows.iter().position(|&(rs, _, _)| r > rs).unwrap_or(rows.len());
            rows.insert(dstindex, (r, score, i));
        }
    }
    let numteams = teamrankings.len() as i32;
    let (mut winner, mut loser) = (-1, -1);
    let mut rankings = Vec::with_capacity(rows.len());
    for (j, &(r, score, chrnum)) in rows.iter().enumerate() {
        rankings.push(Ranking { mpchr: Some(chrnum), teamnum: chrnum, positionindex: j, score });
        if teams_enabled {
            let mut placement = numteams - 1;
            for (k, t) in teamrankings.iter().enumerate() {
                if t.teamnum == teams[chrnum] as usize {
                    placement = k as i32;
                }
            }
            stats[chrnum].placement = placement as u8;
            stats[chrnum].rankablescore = (255 - placement) as u32;
        } else {
            stats[chrnum].placement = j as u8;
            stats[chrnum].rankablescore = r;
        }
        if chrnum < MAX_PLAYERS {
            loser = chrnum as i32;
            if winner == -1 {
                winner = chrnum as i32;
            }
        }
    }
    PlayerRankings { rankings, lastwinner: winner, lastloser: loser }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slot 0 killed slot 4 twice and itself once; slot 4 killed slot 5.
    /// A score is kills minus suicides; fewer deaths break a tie.
    #[test]
    fn rankings_score_kills_minus_suicides_and_break_ties_on_deaths() {
        let mut stats = [MpChrStats::default(); MAX_MPCHRS];
        stats[0].killcounts[4] = 2;
        stats[0].killcounts[0] = 1;
        stats[0].numdeaths = 1;
        stats[4].killcounts[5] = 1;
        stats[4].numdeaths = 2;
        stats[5].numdeaths = 1;
        let slots = 0b11_0001;
        let combat = ScenarioScores::default();
        let r = mp_get_player_rankings(slots, false, &[0; MAX_MPCHRS], &mut stats, &combat);
        let order: Vec<(Option<usize>, i32)> = r.rankings.iter().map(|x| (x.mpchr, x.score)).collect();
        // Slot 0: 2 - 1 = 1 with 1 death; slot 4: 1 with 2 deaths; slot 5: 0.
        assert_eq!(order, [(Some(0), 1), (Some(4), 1), (Some(5), 0)]);
        assert_eq!((stats[0].placement, stats[4].placement, stats[5].placement), (0, 1, 2));
        assert_eq!((r.lastwinner, r.lastloser), (0, 0));

        // Teams: 0 and 5 on team 2, 4 on team 1. Team 2 scores slot 0's point
        // (its kills of 4 count, its suicide costs) and slot 5's 0; team 1 scores
        // 4's kill of 5. Both have 2 deaths: the lower team number wins the tie.
        let mut teams = [0u8; MAX_MPCHRS];
        teams[0] = 2;
        teams[5] = 2;
        teams[4] = 1;
        let sc = MpScoring { chrslots: slots, teams_enabled: true, teams: &teams, stats: &stats, scenario: &combat };
        let t: Vec<(usize, i32)> = sc.mp_get_team_rankings().iter().map(|x| (x.teamnum, x.score)).collect();
        assert_eq!(t, [(1, 1), (2, 1)]);
        mp_get_player_rankings(slots, true, &teams, &mut stats, &combat);
        assert_eq!((stats[4].placement, stats[0].placement, stats[5].placement), (0, 1, 1));
        assert_eq!(stats[0].rankablescore, 254);
    }

    /// Slot 0 (team 0) scored 2 points, killed slot 4 (team 1) three times,
    /// its teammate slot 5 once and itself once. Each scenario's points, with
    /// and without Kills Score (Combat's count kills only).
    #[test]
    fn each_scenario_scores_its_own_points_and_the_kills_with_kills_score() {
        let mut stats = [MpChrStats::default(); MAX_MPCHRS];
        stats[0].numpoints = 2;
        stats[0].killcounts[4] = 3;
        stats[0].killcounts[5] = 1;
        stats[0].killcounts[0] = 1;
        let mut teams = [0u8; MAX_MPCHRS];
        teams[4] = 1;
        let score = |scenario: u8, options: u32, teams_enabled: bool| {
            let mut sc = ScenarioScores::new(scenario, options);
            sc.htm_numpoints[0] = 5;
            sc.pac_killcounts[0] = 1;
            sc.pac_survivalcounts[0] = 4;
            MpScoring { chrslots: 0x31, teams_enabled, teams: &teams, stats: &stats, scenario: &sc }.scenario_calculate_player_score(0).0
        };
        let ks = MPOPTION_KILLSSCORE;
        // Combat: 3 + 1 - 1 without teams, 3 - 1 - 1 with.
        assert_eq!((score(MPSCENARIO_COMBAT, 0, false), score(MPSCENARIO_COMBAT, 0, true)), (3, 1));
        assert_eq!((score(MPSCENARIO_HOLDTHEBRIEFCASE, 0, false), score(MPSCENARIO_HOLDTHEBRIEFCASE, ks, false)), (2, 5));
        assert_eq!((score(MPSCENARIO_HACKERCENTRAL, 0, false), score(MPSCENARIO_HACKERCENTRAL, ks, true)), (10, 11));
        assert_eq!((score(MPSCENARIO_POPACAP, 0, false), score(MPSCENARIO_POPACAP, ks, false)), (6, 9));
        // King of the Hill and Capture the Case count teammates against, teams or not.
        assert_eq!((score(MPSCENARIO_KINGOFTHEHILL, 0, true), score(MPSCENARIO_KINGOFTHEHILL, ks, false)), (2, 3));
        assert_eq!((score(MPSCENARIO_CAPTURETHECASE, 0, true), score(MPSCENARIO_CAPTURETHECASE, ks, false)), (6, 7));
    }

    #[test]
    fn a_new_player_is_a_beginner_and_the_title_climbs_with_the_counters() {
        let mut c = MpCareer::default();
        assert_eq!(mp_calculate_player_title(&c), 0);
        // Kills 120 reach the first tier (2 × 60); 12 games won the first two
        // (2 × 3, 4 × 3); 16 hours the first four: 7 tallies, title 1.
        c.kills = 120;
        c.gameswon = 12;
        c.time = 16 * 3600;
        assert_eq!(mp_calculate_player_title(&c), 1);
        let max = MpCareer { kills: u32::MAX, gameswon: u32::MAX, accuracymedals: u32::MAX, headshotmedals: u32::MAX, killmastermedals: u32::MAX, time: u32::MAX, distance: u32::MAX, damagedealt: u32::MAX, ammoused: u32::MAX, survivormedals: u32::MAX, ..MpCareer::default() };
        assert_eq!(mp_calculate_player_title(&max), MPPLAYERTITLE_PERFECT);
    }

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
        assert_eq!(s.mphilltime, 10);
    }
}
