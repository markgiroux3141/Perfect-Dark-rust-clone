//! The match: `mplayer/mplayer.c` (pause, limits, rankings, the end of the
//! match), `mpstats.c` (who killed whom, the shot counts), the match half of
//! `lv.c` (the time and score limits in `lv_tick`), `scenarios.c` (the Combat
//! rules and the match-start messages) and `hudmsg.c` ([`hudmsg`]). The
//! awards at the end are [`awards`].
//!
//! PD keeps a match's counters in the mpchrconfigs (`g_PlayerConfigsArray`,
//! `g_BotConfigsArray`) by chr slot, and each human's in `g_Vars.playerstats`
//! and `struct player`. Here they are [`MpMatch`]: `chrs` by chr slot (the
//! shared `pd_core::mp::MpChrStats`, which the menus' rankings read too),
//! `playerstats` and `players` by player number.
//!
//! Chr indexes (`g_MpAllChrPtrs`, the world's `chrs`: the players, then the
//! simulants) and chr slots (`chrslots` bits: players 0..3, simulants 4..11)
//! differ; `mp_chrindex_to_chrslot` goes from one to the other.
//!
//! Source: `reference/pd-decomp/src/game/mplayer/`, `mpstats.c`, `lv.c`. Not
//! in any spike; the spikes only counted kills and deaths.

pub mod awards;
pub mod hudmsg;
pub mod scenario;

use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::lang::{tx, LANGBANK_GUN, LANGBANK_MISC};
use pd_core::mp::{mp_get_player_rankings, MatchSetup, MpChrStats, MpPlayerResult, MpScoring, PlayerRankings, ScenarioScores, MAX_MPCHRS};

pub use hudmsg::{HudMessage, HudMsgs};

use crate::world::World;

/// `g_MiscAudioHandle`'s voice (the time limit's alarm).
pub const MISC_AUDIO_HANDLE: u32 = 0x300;

/// `g_MpScenarioOverviews[].name` (`scenarios.c:253`): `L_MPMENU_246` ..
/// `_251` in `MPSCENARIO_*` order (the menus' table holds the same ids).
pub const MP_SCENARIO_NAMES: [u16; 6] = [246, 247, 248, 249, 250, 251];

/// `COMPARE_*` (`chr_compare_teams`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Any,
    Friends,
    Enemies,
}

/// `struct playerstats` (`types.h:70`): a human's statistics for the match,
/// as `lv_reset` (`lv.c:345`) starts them.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerStats {
    /// By `SHOTREGION_*`: fired, then hits on the head, body, limbs, gun, hat,
    /// objects.
    pub shotcount: [i32; 7],
    pub killcount: i32,
    pub ggkillcount: i32,
    pub kills: [i32; 4],
    /// How often a chr was drawn in this player's view (the "Most Cowardly").
    pub drawplayercount: i32,
    /// Walked, cm.
    pub distance: f32,
    /// Players this one shot in the back.
    pub backshotcount: i32,
    pub armourcount: f32,
    pub fastest2kills: i32,
    pub slowest2kills: i32,
    pub longestlife: i32,
    pub shortestlife: i32,
    pub maxkills: i32,
    pub maxsimulkills: i32,
    /// Of received damage.
    pub damagescale: f32,
    pub tokenheldtime: i32,
    /// `mpindex`: the player's chr slot.
    pub mpindex: usize,
    pub damreceived: f32,
    pub damtransmitted: f32,
}

impl PlayerStats {
    fn new(mpindex: usize) -> PlayerStats {
        PlayerStats {
            shotcount: [0; 7],
            killcount: 0,
            ggkillcount: 0,
            kills: [0; 4],
            drawplayercount: 0,
            distance: 0.0,
            backshotcount: 0,
            armourcount: 0.0,
            fastest2kills: i32::MAX,
            slowest2kills: 0,
            longestlife: 0,
            shortestlife: i32::MAX,
            maxkills: 0,
            maxsimulkills: 0,
            damagescale: 1.0,
            tokenheldtime: 0,
            mpindex,
            damreceived: 0.0,
            damtransmitted: 0.0,
        }
    }
}

/// `struct gunheld`: how long a weapon pair was held (`inv_increment_held_time`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GunHeld {
    pub weapon1: u8,
    pub weapon2: u8,
    /// -1: unused (`inv_reset`, `invreset.c:21`).
    pub totaltime240_60: i32,
}

/// The fields of `struct player` the match keeps, by player number.
#[derive(Clone, Debug, PartialEq)]
pub struct MpPlayerVars {
    pub killsthislife: i32,
    pub deathcount: i32,
    /// `lastkilltime60`, `lastkilltime60_2` .. `_4` (-1: none).
    pub lastkilltime60: [i32; 4],
    pub lifestarttime60: i32,
    /// `bondviewlevtime60`: the player's time in the match
    /// (`player_get_mission_time`).
    pub bondviewlevtime60: i32,
    /// `award1` / `award2`, as `g_AwardNames` indexes (the end screen's).
    pub award1: Option<u8>,
    pub award2: Option<u8>,
    /// Chose End Game.
    pub aborted: bool,
    pub gunheldarr: [GunHeld; 10],
    /// `joybutinhibit`: N64 buttons (controller 1 low, 2 high) ignored until
    /// released.
    pub joybutinhibit: u32,
    /// `g_PlayersWithControl`: false while the player's menu is open.
    pub withcontrol: bool,
    /// The buttons held last frame, for "pressed this frame".
    pub prevbuttons: u16,
}

impl Default for MpPlayerVars {
    fn default() -> Self {
        MpPlayerVars {
            killsthislife: 0,
            deathcount: 0,
            lastkilltime60: [-1; 4],
            lifestarttime60: 0,
            bondviewlevtime60: 0,
            award1: None,
            award2: None,
            aborted: false,
            gunheldarr: [GunHeld { weapon1: 0, weapon2: 0, totaltime240_60: -1 }; 10],
            joybutinhibit: 0,
            withcontrol: true,
            prevbuttons: 0,
        }
    }
}

/// A match's state.
#[derive(Clone, Debug)]
pub struct MpMatch {
    /// `g_MpSetup.paused` (`MPPAUSEMODE_*`).
    pub paused: u8,
    /// `g_MainIsEndscreen`: the match has ended.
    pub endscreen: bool,
    /// `g_StageTimeElapsed60`.
    pub stagetime60: i32,
    /// `g_MpTimeLimit60`, `g_MpScoreLimit`, `g_MpTeamScoreLimit`
    /// (`mp_apply_limits`); 0 is none.
    pub timelimit60: i32,
    pub scorelimit: i32,
    pub teamscorelimit: i32,
    /// `g_NumReasonsToEndMpMatch`: a score limit is reached (the match ends
    /// once no one is dying).
    pub numreasonstoend: i32,
    /// The mpchrconfigs' counters, by chr slot.
    pub chrs: [MpChrStats; MAX_MPCHRS],
    /// `mpchrconfig.team`, by chr slot.
    pub teams: [u8; MAX_MPCHRS],
    /// By player number.
    pub playerstats: Vec<PlayerStats>,
    pub players: Vec<MpPlayerVars>,
    /// The career statistics the players came in with, by player number.
    pub careers: Vec<pd_core::mp::MpCareer>,
    /// Each player's menu is open (`g_Menus[mpindex].curdialog`): the menus
    /// are the game's, which tells the world ([`World::set_menu_open`]).
    pub menuopen: Vec<bool>,
    /// `g_AllowRegionShot` (`mpstats.c:20`).
    pub allowregionshot: bool,
    /// `g_Vars.totalkills`.
    pub totalkills: i32,
    /// `g_MiscAudioHandle` is playing (the last ten seconds' alarm).
    pub alarm: bool,
    pub hudmsgs: HudMsgs,
    /// What the end of the match wrote back to each player's file.
    pub results: Vec<MpPlayerResult>,
    /// `g_ScenarioData`: the scenario's state.
    pub scenariodata: scenario::ScenarioData,
}

impl MpMatch {
    /// `mp_reset` (`mplayer.c:177`), `mp_apply_limits` (`:612`) and `lv_reset`'s
    /// player statistics (`lv.c:345`), for `setup`.
    pub fn new(setup: &MatchSetup) -> MpMatch {
        let mut teams = [0u8; MAX_MPCHRS];
        for p in &setup.players {
            teams[p.slot as usize] = p.chr.team;
        }
        for s in &setup.simulants {
            teams[s.slot as usize] = s.chr.team;
        }
        let n = setup.players.len();
        MpMatch {
            paused: MPPAUSEMODE_UNPAUSED,
            endscreen: false,
            stagetime60: 0,
            timelimit60: if setup.timelimit >= 60 { 0 } else { (setup.timelimit as i32 + 1) * 60 * 60 },
            scorelimit: if setup.scorelimit >= 100 { 0 } else { setup.scorelimit as i32 + 1 },
            teamscorelimit: if setup.teamscorelimit >= 400 { 0 } else { setup.mp_calculate_team_score_limit() + 1 },
            numreasonstoend: 0,
            chrs: [MpChrStats::default(); MAX_MPCHRS],
            teams,
            playerstats: setup.players.iter().map(|p| PlayerStats::new(p.slot as usize)).collect(),
            players: vec![MpPlayerVars::default(); n],
            careers: setup.players.iter().map(|p| p.career).collect(),
            menuopen: vec![false; n],
            allowregionshot: false,
            totalkills: 0,
            alarm: false,
            hudmsgs: HudMsgs::default(),
            results: Vec::new(),
            scenariodata: scenario::ScenarioData::default(),
        }
    }
}

/// `mp_find_max_int` (`mplayer.c:1304`): the player with the largest value
/// among the first `numplayers`, a tie decided by `random() % 2`.
pub(crate) fn mp_find_max_int(rng: &mut pd_core::rng::Rng, numplayers: usize, v: [i32; 4]) -> usize {
    let mut bestvalue = v[0];
    let mut bestplayer = 0;
    if numplayers >= 2 {
        if v[1] > bestvalue || (v[1] == bestvalue && !rng.random().is_multiple_of(2)) {
            bestplayer = 1;
            bestvalue = v[1];
        }
        if numplayers >= 3 {
            if v[2] > bestvalue || (v[2] == bestvalue && !rng.random().is_multiple_of(2)) {
                bestplayer = 2;
                bestvalue = v[2];
            }
            if numplayers >= 4 && (v[3] > bestvalue || (v[3] == bestvalue && !rng.random().is_multiple_of(2))) {
                bestplayer = 3;
            }
        }
    }
    bestplayer
}

/// `mp_find_min_int` (`mplayer.c:1335`).
pub(crate) fn mp_find_min_int(rng: &mut pd_core::rng::Rng, numplayers: usize, v: [i32; 4]) -> usize {
    let mut bestvalue = v[0];
    let mut bestplayer = 0;
    if numplayers >= 2 {
        if v[1] < bestvalue || (v[1] == bestvalue && !rng.random().is_multiple_of(2)) {
            bestplayer = 1;
            bestvalue = v[1];
        }
        if numplayers >= 3 {
            if v[2] < bestvalue || (v[2] == bestvalue && !rng.random().is_multiple_of(2)) {
                bestplayer = 2;
                bestvalue = v[2];
            }
            if numplayers >= 4 && (v[3] < bestvalue || (v[3] == bestvalue && !rng.random().is_multiple_of(2))) {
                bestplayer = 3;
            }
        }
    }
    bestplayer
}

/// `mp_find_max_float` (`mplayer.c:1366`), with PD's bug: the best value so
/// far is kept in an `s32`, so it is truncated before the later compares.
pub(crate) fn mp_find_max_float(rng: &mut pd_core::rng::Rng, numplayers: usize, v: [f32; 4]) -> usize {
    let mut bestplayer = 0;
    if numplayers >= 2 {
        let mut bestvalue: i32;
        if v[1] > v[0] || (v[1] == v[0] && !rng.random().is_multiple_of(2)) {
            bestplayer = 1;
            bestvalue = v[1] as i32;
        } else {
            bestvalue = v[0] as i32;
            bestplayer = 0;
        }
        if numplayers >= 3 {
            if v[2] > bestvalue as f32 || (v[2] == bestvalue as f32 && !rng.random().is_multiple_of(2)) {
                bestplayer = 2;
                bestvalue = v[2] as i32;
            }
            if numplayers >= 4 && (v[3] > bestvalue as f32 || (v[3] == bestvalue as f32 && !rng.random().is_multiple_of(2))) {
                bestplayer = 3;
            }
        }
    }
    bestplayer
}

/// `mp_find_min_float` (`mplayer.c:1399`), with the same bug.
pub(crate) fn mp_find_min_float(rng: &mut pd_core::rng::Rng, numplayers: usize, v: [f32; 4]) -> usize {
    let mut bestplayer = 0;
    if numplayers >= 2 {
        let mut bestvalue: i32;
        if v[1] < v[0] || (v[1] == v[0] && !rng.random().is_multiple_of(2)) {
            bestplayer = 1;
            bestvalue = v[1] as i32;
        } else {
            bestplayer = 0;
            bestvalue = v[0] as i32;
        }
        if numplayers >= 3 {
            if v[2] < bestvalue as f32 || (v[2] == bestvalue as f32 && !rng.random().is_multiple_of(2)) {
                bestplayer = 2;
                bestvalue = v[2] as i32;
            }
            if numplayers >= 4 && (v[3] < bestvalue as f32 || (v[3] == bestvalue as f32 && !rng.random().is_multiple_of(2))) {
                bestplayer = 3;
            }
        }
    }
    bestplayer
}

/// N64 buttons (`n64::pad`'s bits) as `bmove_process_input` reads a player's
/// controller: the parts of [`crate::player::PlayerInput`] a pad drives.
pub fn input_buttons(i: &crate::player::PlayerInput) -> u16 {
    const A: u16 = 0x8000;
    const B: u16 = 0x4000;
    const Z: u16 = 0x2000;
    const START: u16 = 0x1000;
    const R: u16 = 0x0010;
    const CU: u16 = 0x0008;
    const CD: u16 = 0x0004;
    const CL: u16 = 0x0002;
    const CR: u16 = 0x0001;
    let mut b = 0;
    for (on, bit) in [(i.a_held, A), (i.use_held, B), (i.fire, Z), (i.start, START), (i.aim, R), (i.c_up, CU), (i.c_down, CD), (i.c_left, CL), (i.c_right, CR)] {
        if on {
            b |= bit;
        }
    }
    b
}

/// The input with the buttons in `mask` let go.
fn input_without(mut i: crate::player::PlayerInput, mask: u16) -> crate::player::PlayerInput {
    let off = |bit: u16| mask & bit != 0;
    if off(0x8000) {
        i.a_held = false;
    }
    if off(0x4000) {
        i.use_held = false;
    }
    if off(0x2000) {
        i.fire = false;
    }
    if off(0x1000) {
        i.start = false;
    }
    if off(0x0010) {
        i.aim = false;
    }
    if off(0x0008) {
        i.c_up = false;
    }
    if off(0x0004) {
        i.c_down = false;
    }
    if off(0x0002) {
        i.c_left = false;
    }
    if off(0x0001) {
        i.c_right = false;
    }
    i
}

impl World {
    /// `mp_is_paused` (`mplayer.c:1167`): one player with its menu open, or
    /// the match paused (or over).
    pub fn mp_is_paused(&self) -> bool {
        if self.players.len() == 1 && self.mp.menuopen.first().copied().unwrap_or(false) {
            return true;
        }
        self.mp.paused != MPPAUSEMODE_UNPAUSED
    }

    /// `mp_set_paused` (`mplayer.c:1182`).
    pub fn mp_set_paused(&mut self, mode: u8) {
        self.mp.paused = mode;
    }

    /// The menus' state for player `pi`: `menu_push_root_dialog` takes the
    /// player's control away (`g_PlayersWithControl`, `menu.c:3491`), and
    /// closing the menu gives it back with every held button ignored until
    /// released (`menu_update_cur_frame`, `menu.c:1631`).
    pub fn set_menu_open(&mut self, pi: usize, open: bool) {
        let (Some(o), Some(v)) = (self.mp.menuopen.get_mut(pi), self.mp.players.get_mut(pi)) else { return };
        if *o && !open {
            v.joybutinhibit = 0xffff_ffff;
        }
        *o = open;
        v.withcontrol = !open;
    }

    /// The pause menu's inventory pick (`menuhandler_inventory_list`,
    /// `mainmenu.c:4176`): row `index` in both hands if the player has two.
    pub fn mp_equip_inventory(&mut self, pi: usize, index: usize) {
        let Some(p) = self.players.get_mut(pi) else { return };
        // menuhandler_inventory_list (`mainmenu.c:4176`): the row's weapon,
        // both hands if it is held twice.
        let w = p.gun.p.inventory.inv_get_weapon_num_by_index(index as i32);
        if w != 0 {
            p.gun.p.inventory.equipcuritem = index as i32;
            p.gun.select_weapon(w, true);
        }
    }

    /// `inv_get_current_index` (`inv.c:1056`) after `inv_calculate_current_index`
    /// for the gun in hand (the pause menu's Inventory opens on it).
    pub fn inv_get_current_index(&self, pi: usize) -> i32 {
        let g = &self.players[pi].gun;
        let mut inv = g.p.inventory.clone();
        inv.inv_calculate_current_index(g.bgun_get_weapon_num(HAND_RIGHT));
        inv.equipcuritem
    }

    /// `mp_chrindex_to_chrslot` (`mplayer.c:3285`).
    pub fn mp_chrindex_to_chrslot(&self, chrnum: usize) -> Option<usize> {
        self.chrs.get(chrnum).map(|c| c.mpslot)
    }

    /// The name `g_MpAllChrConfigPtrs[chrnum]->name` holds (with its line break).
    pub(crate) fn mp_chr_name(&self, chrnum: usize) -> String {
        format!("{}\n", self.chrs.get(chrnum).map_or("", |c| c.name.as_str()))
    }

    /// The scoring's view of the match, with the scenario's counters
    /// ([`World::scenario_scores`]) in `scenario`.
    pub fn mp_scoring<'a>(&'a self, scenario: &'a ScenarioScores) -> MpScoring<'a> {
        MpScoring { chrslots: self.setup.chrslots(), teams_enabled: self.setup.teams_enabled(), teams: &self.mp.teams, stats: &self.mp.chrs, scenario }
    }

    /// `mp_get_player_rankings` (`mplayer.c:640`) on the match's counters
    /// (writing the chrs' placements).
    pub fn mp_get_player_rankings(&mut self) -> PlayerRankings {
        let (slots, teams) = (self.setup.chrslots(), self.setup.teams_enabled());
        let scenario = self.scenario_scores();
        mp_get_player_rankings(slots, teams, &self.mp.teams, &mut self.mp.chrs, &scenario)
    }

    /// `chr_compare_teams` (`chraction.c:14853`) in a normal match: friends
    /// share a team with teams on; everyone else is an enemy.
    pub fn chr_compare_teams(&self, a: usize, b: usize, checktype: Compare) -> bool {
        let teams = self.setup.teams_enabled();
        match checktype {
            Compare::Any => true,
            Compare::Friends => teams && self.chrs[b].team == self.chrs[a].team,
            Compare::Enemies => !teams || self.chrs[b].team != self.chrs[a].team,
        }
    }

    /// `player_get_mission_time` (`player.c:5246`).
    pub fn player_get_mission_time(&self, pi: usize) -> i32 {
        self.mp.players.get(pi).map_or(0, |v| v.bondviewlevtime60)
    }

    /// The pad half of `bmove_process_input` (`bondmove.c:589`) before the
    /// walk: no control while the menu is open (`bmove_tick(0, 0, 0, 1)`),
    /// the inhibited buttons let go, then START: alive, it opens the pause
    /// menu; dead, it unpauses (one player) or opens the pause menu over the
    /// paused match (`bondmove.c:691`).
    pub(crate) fn bmove_process_input_mp(&mut self, pi: usize, input: &crate::player::PlayerInput) -> crate::player::PlayerInput {
        let idle = crate::player::PlayerInput::default();
        let v = &mut self.mp.players[pi];
        let mut input = if v.withcontrol { input.clone() } else { idle };
        let held = input_buttons(&input);
        let mut pressed = held & !v.prevbuttons;
        v.prevbuttons = held;
        if v.joybutinhibit & 0xffff != 0 {
            let inhibited = held & (v.joybutinhibit & 0xffff) as u16;
            input = input_without(input, inhibited);
            pressed &= !inhibited;
            v.joybutinhibit = (v.joybutinhibit & 0xffff_0000) | inhibited as u32;
        }
        let start = pressed & 0x1000 != 0;
        if !self.players[pi].isdead {
            if start {
                self.push_event(Event::MpPushPauseDialog { player: pi as u8 });
            }
        } else if self.players.len() == 1 {
            if self.mp_is_paused() && start && self.mp.paused != MPPAUSEMODE_GAMEOVER {
                self.mp_set_paused(MPPAUSEMODE_UNPAUSED);
            }
        } else if self.mp_is_paused() && start {
            self.push_event(Event::MpPushPauseDialog { player: pi as u8 });
        }
        input
    }

    /// Smart slow motion's test (`lv.c:2061`): one of a living player's rooms
    /// is on another living player's screen (`bg_room_is_on_player_screen`,
    /// `g_MpRoomVisibility`).
    pub(crate) fn lv_smart_slowmo_enemy_on_screen(&self) -> bool {
        let n = self.players.len();
        for p in 0..n {
            if self.players[p].isdead {
                continue;
            }
            for &room in &self.players[p].rooms {
                let vis = self.mp_room_visibility.get(room as usize).copied().unwrap_or(0);
                if (0..n).any(|o| o != p && !self.players[o].isdead && vis & (1 << o) != 0) {
                    return true;
                }
            }
        }
        false
    }

    /// `lv_tick`'s match ending (`lv.c:2182`): one minute left, the time limit
    /// (the alarm in the last ten seconds), and a score limit reached, which
    /// ends the match once no one is dying. Then the match clock.
    pub(crate) fn lv_tick_mp(&mut self) {
        self.mp.numreasonstoend = 0;
        let lv60 = self.lv.lvupdate60;
        if self.mp.timelimit60 > 0 {
            let limit = self.mp.timelimit60;
            let elapsed = self.mp.stagetime60;
            let nexttime = lv60 + elapsed;
            let warntime = limit - 3600;
            if elapsed < warntime && nexttime >= warntime {
                let text = self.res.lang.get(tx(LANGBANK_MISC, 68)).to_string();
                for i in 0..self.players.len() {
                    self.hudmsg_create(i, &text, HUDMSGTYPE_DEFAULT);
                }
            }
            if elapsed < limit && nexttime >= limit {
                // Match is ending due to time limit reached
                self.main_end_stage();
            }
            if nexttime >= limit - 600 && !self.mp.alarm && nexttime < limit {
                self.mp.alarm = true;
                self.push_event(Event::HandleSound { handle: MISC_AUDIO_HANDLE, sound: 0x00a3, pitch: 1.0, volume: 1.0, pan: 0.0 });
            }
        }
        if self.lv.lvupdate240 != 0 {
            let mut numdying = 0;
            for p in &self.players {
                if p.isdead && (!p.redbloodfinished || !p.deathanimfinished || !p.health.player_is_fade_complete()) {
                    numdying += 1;
                }
            }
            numdying += self.chrs.iter().filter(|c| c.actiontype == crate::chr::Act::Die).count();
            if self.mp.scorelimit > 0 {
                let limit = self.mp.scorelimit;
                let r = self.mp_get_player_rankings();
                self.mp.numreasonstoend += r.rankings.iter().filter(|x| x.score >= limit).count() as i32;
            }
            if self.mp.teamscorelimit > 0 {
                let limit = self.mp.teamscorelimit;
                let scenario = self.scenario_scores();
                let n = self.mp_scoring(&scenario).mp_get_team_rankings().iter().filter(|x| x.score >= limit).count() as i32;
                self.mp.numreasonstoend += n;
            }
            if self.mp.numreasonstoend > 0 && numdying == 0 {
                self.main_end_stage();
            }
        }
        self.mp.stagetime60 += lv60;
    }

    /// `main_end_stage` (`main.c:1112`) in a normal match: `mp_end_match` once.
    pub fn main_end_stage(&mut self) {
        if !self.mp.endscreen {
            self.mp_end_match();
        }
        self.mp.endscreen = true;
    }

    /// `menuhandler_mp_end_game` (`ingame.c:101`): player `pi` gave up.
    pub fn mp_end_game(&mut self, pi: usize) {
        if let Some(v) = self.mp.players.get_mut(pi) {
            v.aborted = true;
        }
        self.main_end_stage();
    }

    /// `mp_end_match` (`mplayer.c:2426`): over for good (`MPPAUSEMODE_GAMEOVER`),
    /// no one on a Slayer rocket, the awards, then the end screens.
    /// `// M12:` the menu music, `challenge_consider_marking_complete`.
    fn mp_end_match(&mut self) {
        self.mp_set_paused(MPPAUSEMODE_GAMEOVER);
        for i in 0..self.players.len() {
            self.mp.players[i].award1 = None;
            self.mp.players[i].award2 = None;
            if self.players[i].visionmode == VISIONMODE_SLAYERROCKET {
                self.players[i].visionmode = VISIONMODE_NORMAL;
            }
        }
        self.mp_calculate_awards();
        self.push_event(Event::MpEndMatch);
    }

    /// `scenario_tick` (`scenarios.c:528`): on the fifth frame, the
    /// scenario's name for every player (`scenario_create_match_start_hudmsgs`);
    /// then the scenario's own tick ([`scenario`]).
    pub(crate) fn scenario_tick(&mut self) {
        if self.lv.lvframenum == 5 {
            self.scenario_create_match_start_hudmsgs();
        }
        self.scenario_tick_callback();
    }

    /// `scenario_create_match_start_hudmsgs` (`scenarios.c:485`). `// M12:`
    /// a challenge's name first.
    fn scenario_create_match_start_hudmsgs(&mut self) {
        let name = self.res.lang.get(tx(pd_core::lang::LANGBANK_MPMENU, MP_SCENARIO_NAMES[self.setup.scenario.min(5) as usize])).trim_end_matches('\n').to_string();
        let text = format!("{name}\n");
        for i in 0..self.players.len() {
            self.hudmsg_create_with_flags(i, &text, HUDMSGTYPE_DEFAULT, HUDMSGFLAG_ONLYIFALIVE);
        }
    }

    /// `mpstats_record_death` (`mpstats.c:238`): chr `aplayernum` killed chr
    /// `vplayernum` (chr indexes, -1 for none; the same for a suicide). The
    /// victim's death and the killer's kill (or the suicide) are counted by
    /// chr slot, the players are told ("Killed by", "Killed", the counts),
    /// and a killed simulant remembers who did it.
    pub(crate) fn mpstats_record_death(&mut self, aplayernum: i32, vplayernum: i32) {
        if self.setup.scenario == MPSCENARIO_POPACAP {
            self.pac_handle_death(aplayernum, vplayernum);
        }
        let ampindex = if aplayernum >= 0 { self.mp_chrindex_to_chrslot(aplayernum as usize) } else { None };
        let vmpindex = if vplayernum >= 0 { self.mp_chrindex_to_chrslot(vplayernum as usize) } else { None };
        let playercount = self.players.len() as i32;
        if vplayernum >= 0 && aplayernum == vplayernum {
            // Player suicide
            if let Some(v) = vmpindex {
                self.mp.chrs[v].numdeaths += 1;
                self.mp.chrs[v].killcounts[v] += 1;
            }
            if vplayernum < playercount {
                self.mpstats_record_player_suicide(vplayernum as usize);
            }
        } else {
            // Normal kill
            if vplayernum >= 0 {
                if let Some(v) = vmpindex {
                    self.mp.chrs[v].numdeaths += 1;
                }
                if vplayernum < playercount {
                    if aplayernum >= 0 {
                        // "Killed by %s"
                        let text = format!("{} {}", self.res.lang.get(tx(LANGBANK_MISC, 183)), self.mp_chr_name(aplayernum as usize));
                        self.hudmsg_create(vplayernum as usize, &text, HUDMSGTYPE_DEFAULT);
                    }
                    self.mpstats_record_player_death(vplayernum as usize);
                }
            }
            if let (Some(a), Some(v)) = (ampindex, vmpindex) {
                self.mp.chrs[a].killcounts[v] += 1;
            }
            if aplayernum >= 0 && aplayernum < playercount {
                if vplayernum >= 0 {
                    // "Killed %s"
                    let text = format!("{} {}", self.res.lang.get(tx(LANGBANK_MISC, 184)), self.mp_chr_name(vplayernum as usize));
                    self.hudmsg_create(aplayernum as usize, &text, HUDMSGTYPE_DEFAULT);
                }
                self.mpstats_record_player_kill(aplayernum as usize);
            }
            // If someone killed an aibot
            if aplayernum >= 0 && vplayernum >= playercount && aplayernum != vplayernum {
                if let Some(a) = self.chrs[vplayernum as usize].aibot.as_mut() {
                    a.lastkilledbyplayernum = aplayernum;
                }
            }
        }
        self.mpstats_record_bot_kill(aplayernum, vplayernum);
        self.mp.totalkills += 1;
        let killer = (aplayernum >= 0).then_some(aplayernum as u8);
        self.push_event(Event::Kill { killer, victim: vplayernum.max(0) as u8 });
    }

    /// The simultaneous-kill bookkeeping `mpstats_record_player_kill` and
    /// `_suicide` share (`mpstats.c:112`): the slowest and fastest two kills,
    /// the last four kill times, and kills within two seconds of each other.
    fn mpstats_kill_times(&mut self, pi: usize) {
        let time = self.player_get_mission_time(pi);
        let (stats, v) = (&mut self.mp.playerstats[pi], &mut self.mp.players[pi]);
        if stats.killcount > 1 {
            let duration = time - v.lastkilltime60[0];
            if duration > stats.slowest2kills {
                stats.slowest2kills = duration;
            }
            if duration < stats.fastest2kills {
                stats.fastest2kills = duration;
            }
        }
        v.lastkilltime60 = [time, v.lastkilltime60[0], v.lastkilltime60[1], v.lastkilltime60[2]];
        let mut simulkills = 1;
        let t = v.lastkilltime60;
        if t[1] != -1 && t[0] - t[1] < 120 {
            simulkills += 1;
            if t[2] != -1 && t[0] - t[2] < 120 {
                simulkills += 1;
                if t[3] != -1 && t[0] - t[3] < 120 {
                    simulkills += 1;
                }
            }
        }
        if simulkills > stats.maxsimulkills {
            stats.maxsimulkills = simulkills;
        }
    }

    /// `mpstats_record_player_kill` (`mpstats.c:92`): "Kill count: N".
    fn mpstats_record_player_kill(&mut self, pi: usize) {
        self.mp.playerstats[pi].killcount += 1;
        self.mp.players[pi].killsthislife += 1;
        let text = format!("{}: {}\n", self.res.lang.get(tx(LANGBANK_GUN, 1)), self.mp.playerstats[pi].killcount);
        self.hudmsg_create(pi, &text, HUDMSGTYPE_DEFAULT);
        self.mpstats_kill_times(pi);
    }

    /// `mpstats_record_player_death` (`mpstats.c:159`): "Died once", "Died N times".
    fn mpstats_record_player_death(&mut self, pi: usize) {
        self.mp.players[pi].deathcount += 1;
        let n = self.mp.players[pi].deathcount;
        let lang = &self.res.lang;
        let text = if n == 1 { lang.get(tx(LANGBANK_GUN, 2)).to_string() } else { format!("{} {} {}\n", lang.get(tx(LANGBANK_GUN, 3)), n, lang.get(tx(LANGBANK_GUN, 4))) };
        self.hudmsg_create(pi, &text, HUDMSGTYPE_DEFAULT);
    }

    /// `mpstats_record_player_suicide` (`mpstats.c:179`): "Suicide count: N".
    fn mpstats_record_player_suicide(&mut self, pi: usize) {
        let mpindex = self.mp.playerstats[pi].mpindex;
        let text = format!("{}: {}\n", self.res.lang.get(tx(LANGBANK_GUN, 5)), self.mp.chrs[mpindex].killcounts[mpindex]);
        self.hudmsg_create(pi, &text, HUDMSGTYPE_DEFAULT);
        self.mpstats_kill_times(pi);
    }

    /// `mpstats_increment_player_shotcount` (`mpstats.c:39`): a shot fired
    /// (`SHOTREGION_TOTAL`) allows one region hit to be counted for it; a
    /// `WEAPONFLAG_DONTCOUNTSHOTS` weapon counts nothing.
    pub(crate) fn mpstats_increment_player_shotcount(&mut self, pi: usize, weaponnum: u8, region: usize) {
        let counts = !self.res.gset.has_flag(weaponnum, WEAPONFLAG_DONTCOUNTSHOTS);
        let Some(stats) = self.mp.playerstats.get_mut(pi) else { return };
        if region == SHOTREGION_TOTAL {
            if counts {
                self.mp.allowregionshot = true;
                stats.shotcount[region] += 1;
            }
        } else if self.mp.allowregionshot {
            if counts {
                stats.shotcount[region] += 1;
            }
            self.mp.allowregionshot = false;
        }
    }

    /// `mpstats_end_shot` (`mpstats.c:57`).
    pub(crate) fn mpstats_end_shot(&mut self) {
        self.mp.allowregionshot = false;
    }

    /// Chr `i`'s kills of other chrs (its killcounts but its own slot's).
    pub fn mp_chr_kills(&self, i: usize) -> u32 {
        let s = self.chrs[i].mpslot;
        (0..MAX_MPCHRS).filter(|&j| j != s).map(|j| self.mp.chrs[s].killcounts[j] as u32).sum()
    }

    /// Chr `i`'s deaths, suicides included.
    pub fn mp_chr_deaths(&self, i: usize) -> u32 {
        self.mp.chrs[self.chrs[i].mpslot].numdeaths as u32
    }

    /// Shots player `pi` has fired (`mpstats_get_player_shotcount_by_region(SHOTREGION_TOTAL)`).
    pub fn shots_fired(&self, pi: usize) -> i32 {
        self.mp.playerstats.get(pi).map_or(0, |s| s.shotcount[SHOTREGION_TOTAL])
    }

    /// `player_update_damage_stats` (`chraction.c:4217`): the damage a player
    /// dealt and took (chr indexes; a player's chr index is its number).
    pub(crate) fn player_update_damage_stats(&mut self, attacker: Option<usize>, victim: usize, damage: f32) {
        let n = self.players.len();
        if let Some(a) = attacker.filter(|&a| a < n) {
            self.mp.playerstats[a].damtransmitted += damage;
        }
        if victim < n {
            self.mp.playerstats[victim].damreceived += damage;
        }
    }

    /// `player_check_if_shot_in_back` (`player.c:4862`): player `attacker`'s
    /// shot came from within 90° of player `victim`'s back.
    pub(crate) fn player_check_if_shot_in_back(&mut self, attacker: usize, victim: usize, x: f32, z: f32) {
        let angle = x.atan2(z);
        let mut finalangle = self.players[victim].theta - (360.0 - angle * 180.0 / std::f32::consts::PI);
        if finalangle < 0.0 {
            finalangle = -finalangle;
        }
        if !(90.0..=270.0).contains(&finalangle) {
            self.mp.playerstats[attacker].backshotcount += 1;
        }
    }

    /// `inv_increment_held_time` (`inv.c:1103`): the weapon pair in player
    /// `pi`'s hands this frame, for the weapon of choice.
    pub(crate) fn inv_increment_held_time(&mut self, pi: usize) {
        let gset = self.res.gset.clone();
        let weapon1 = self.players[pi].gun.bgun_get_weapon_num(HAND_RIGHT);
        let mut weapon2 = self.players[pi].gun.bgun_get_weapon_num(HAND_LEFT);
        if !gset.has_flag(weapon1, WEAPONFLAG_TRACKTIMEUSED) {
            return;
        }
        if !gset.has_flag(weapon2, WEAPONFLAG_TRACKTIMEUSED) {
            weapon2 = 0;
        }
        let lv60 = self.lv.lvupdate60;
        let arr = &mut self.mp.players[pi].gunheldarr;
        let mut leastusedtime = i32::MAX;
        let mut leastusedindex = 0;
        let mut i = 0;
        while i < arr.len() {
            let time = arr[i].totaltime240_60;
            if time >= 0 {
                if weapon1 == arr[i].weapon1 && weapon2 == arr[i].weapon2 {
                    arr[i].totaltime240_60 = time + lv60;
                    break;
                }
                if time < leastusedtime {
                    leastusedtime = time;
                    leastusedindex = i;
                }
            } else {
                leastusedindex = i;
                i = arr.len();
                break;
            }
            i += 1;
        }
        if i == arr.len() {
            arr[leastusedindex] = GunHeld { weapon1, weapon2, totaltime240_60: lv60 };
        }
    }

    /// `inv_get_weapon_of_choice` (`inv.c:1148`): the pair held longest.
    pub fn inv_get_weapon_of_choice(&self, pi: usize) -> (u8, u8) {
        let mut mosttime = -1;
        let mut out = (0, 0);
        for g in &self.mp.players[pi].gunheldarr {
            if g.totaltime240_60 >= 0 && g.totaltime240_60 > mosttime {
                mosttime = g.totaltime240_60;
                out = (g.weapon1, g.weapon2);
            }
        }
        out
    }

    /// `mp_player_get_weapon_of_choice_name` (`title.c:108`): `bgun_get_name`
    /// of the first of the pair.
    pub fn mp_player_get_weapon_of_choice_name(&self, pi: usize) -> String {
        let (weapon1, _) = self.inv_get_weapon_of_choice(pi);
        self.res.gset.weapon(weapon1).map_or(String::new(), |w| w.name.clone())
    }

    /// What `mp_render_modal_text` (`mplayer.c:1196`) shows over player `pi`'s
    /// view: "Paused" (at the top while its menu is open), or "Press START"
    /// once a dead player may start again (with a challenge's countdown).
    pub fn mp_modal_text(&self, pi: usize) -> ModalText {
        let p = &self.players[pi];
        if self.mp.paused == MPPAUSEMODE_PAUSED {
            return ModalText::Paused { menuopen: self.mp.menuopen.get(pi).copied().unwrap_or(false) };
        }
        if !self.mp.endscreen && self.mp.paused == MPPAUSEMODE_UNPAUSED && p.isdead && p.redbloodfinished && p.deathanimfinished && self.mp.numreasonstoend == 0 {
            return ModalText::PressStart { deadtimer: -1 };
        }
        ModalText::None
    }
}

/// `g_TeamColours` (`radar.c:25`): red, yellow, blue, magenta, cyan, orange,
/// pink, brown.
pub const G_TEAM_COLOURS: [u32; 8] = [0xff000000, 0xffff0000, 0x0000ff00, 0xff00ff00, 0x00ffff00, 0xff885500, 0x8800ff00, 0x88445500];

/// `radar_get_team_index` (`radar.c:99`): a chr's team bit's index.
pub fn radar_get_team_index(team: u8) -> usize {
    (0..8).find(|&i| team & (1 << i) != 0).unwrap_or(0)
}

impl World {
    /// `scenario_highlight_prop` (`scenarios.c:712`) for chr `i` as player
    /// `pi` sees it: the scenario's own highlight first (a case's carrier, the
    /// victim); then with teams on and the player's "highlight teams", the
    /// team's colour at 75/255; else with "highlight players", a pulsing blue;
    /// none with Combat's "No Player Highlight". `// M11:` the simulant being
    /// given orders pulses.
    pub fn scenario_highlight_chr(&self, pi: usize, i: usize) -> Option<[u8; 4]> {
        if let Some(c) = self.scenario_highlight_prop_callback(scenario::PropRef::Chr(i)) {
            return Some(c);
        }
        let displayoptions = self.setup.players.get(pi).map_or(0, |p| p.chr.displayoptions);
        if self.setup.scenario == MPSCENARIO_COMBAT && self.setup.options & MPOPTION_NOPLAYERHIGHLIGHT != 0 {
            return None;
        }
        if self.setup.teams_enabled() && displayoptions & MPDISPLAYOPTION_HIGHLIGHTTEAMS != 0 {
            let c = G_TEAM_COLOURS[radar_get_team_index(self.chrs[i].team)];
            return Some([(c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, 75]);
        }
        if displayoptions & MPDISPLAYOPTION_HIGHLIGHTPLAYERS != 0 {
            return Some([0, 0xcd, 0xff, (self.menu_get_sin_osc_frac(20.0) * 205.0) as u8]);
        }
        None
    }

    /// `menu_get_sin_osc_frac(20)` (`menu.c`) on `g_20SecIntervalFrac`: 0..1.
    fn menu_get_sin_osc_frac(&self, mult: f32) -> f32 {
        ((mult * self.frac20 + mult * self.frac20) * std::f32::consts::PI).sin() / 2.0 + 0.5
    }

    /// `scenario_highlight_prop`'s object half (`scenarios.c:720`): unless No
    /// Pickup Highlight is on in a Combat match, a player with "highlight
    /// pickups" sees weapons, crates and shields in a pulsing blue.
    pub fn scenario_highlight_obj(&self, pi: usize, o: &crate::props::Obj) -> Option<[u8; 4]> {
        if let Some(c) = self.scenario_highlight_prop_callback(scenario::PropRef::Obj(o.id)) {
            return Some(c);
        }
        let displayoptions = self.setup.players.get(pi).map_or(0, |p| p.chr.displayoptions);
        if (self.setup.scenario != MPSCENARIO_COMBAT || self.setup.options & MPOPTION_NOPICKUPHIGHLIGHT == 0)
            && displayoptions & MPDISPLAYOPTION_HIGHLIGHTPICKUPS != 0
            && matches!(o.ty, OBJTYPE_AMMOCRATE | OBJTYPE_WEAPON | OBJTYPE_LINKGUNS | OBJTYPE_MULTIAMMOCRATE | OBJTYPE_SHIELD)
        {
            return Some([0, 0xcd, 0xff, (self.menu_get_sin_osc_frac(20.0) * 255.0) as u8]);
        }
        None
    }
}

/// `mp_render_modal_text`'s choice (see [`World::mp_modal_text`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalText {
    None,
    /// "Paused", at the top of the view when the player's menu is open.
    Paused { menuopen: bool },
    /// "Press START", and a challenge's seconds to wait (`deadtimer`, -1: none).
    PressStart { deadtimer: i32 },
}

#[cfg(test)]
mod tests;
