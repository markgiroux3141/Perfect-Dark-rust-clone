//! The middle of `bot_tick_unpaused` (`bot.c:2522-3340`): the scenario orders
//! a simulant with no human on its team gives itself, the main loop that picks
//! what to do from its orders (`aibot->command`), and each action's check that
//! it still makes sense.
//!
//! The orders a human gives from the pause menu (`AIBOTCMD_ATTACK`, `FOLLOW`,
//! `PROTECT`, `DEFEND`, `HOLD`) and the personalities' targets (Venge, Feud,
//! Judge, Prey, Coward's weapons) are M11's; the scenario orders are all here.

use glam::Vec3;
use pd_core::ids::*;

use super::{botinv, MyAction};
use crate::chr::Act;
use crate::mp::scenario::PropRef;
use crate::world::World;

impl World {
    /// `bot_apply_scenario_command` (`bot.c:1287`).
    fn bot_apply_scenario_command(&mut self, i: usize, command: u8) {
        let a = self.ab_mut(i);
        a.command = command;
        a.forcemainloop = true;
    }

    /// `bot_get_team_size` (`bot.c:2318`): every chr sharing its `chr->team`
    /// (with teams off, whatever teams the setup left them in).
    pub(crate) fn bot_get_team_size(&self, i: usize) -> i32 {
        let team = self.chrs[i].team;
        self.chrs.iter().filter(|c| c.team == team).count() as i32
    }

    /// `bot_get_count_in_team_doing_command` (`bot.c:2331`): simulants only.
    fn bot_get_count_in_team_doing_command(&self, i: usize, command: u8, includeself: bool) -> i32 {
        let team = self.chrs[i].team;
        (self.players.len()..self.chrs.len()).filter(|&j| self.chrs[j].team == team && (includeself || j != i) && self.chrs[j].aibot.as_ref().is_some_and(|a| a.command == command)).count() as i32
    }

    /// `bot_should_return_ctc_token` (`bot.c:2364`): carrying a case, and not a
    /// team of one whose own case is stolen.
    pub(crate) fn bot_should_return_ctc_token(&self, i: usize) -> bool {
        let a = self.ab(i);
        a.hascase && (!a.teamisonlyai || self.bot_get_team_size(i) >= 2 || !self.ctc_is_chrs_token_held(i))
    }

    /// `bot_get_num_teammates_defending_hill` (`bot.c:2375`): its team's
    /// simulants in the hill with a hill order.
    pub(crate) fn bot_get_num_teammates_defending_hill(&self, i: usize) -> i32 {
        let hill = self.mp.scenariodata.koh.hillroom;
        let team = self.chrs[i].team;
        (0..self.chrs.len())
            .filter(|&j| self.chrs[j].team == team && hill.is_some() && self.chr_room0(j) == hill)
            .filter(|&j| self.chrs[j].aibot.as_ref().is_some_and(|a| a.command == AIBOTCMD_DEFHILL || a.command == AIBOTCMD_HOLDHILL))
            .count() as i32
    }

    /// `bot_get_num_opponents_in_hill` (`bot.c:2399`): the most of any one
    /// other team in the hill.
    pub(crate) fn bot_get_num_opponents_in_hill(&self, i: usize) -> i32 {
        let hill = self.mp.scenariodata.koh.hillroom;
        let myteam = self.mp.teams[self.chrs[i].mpslot];
        let mut counts = [0i32; pd_core::mp::MAX_TEAMS];
        for j in 0..self.chrs.len() {
            if hill.is_some() && self.chr_room0(j) == hill {
                let t = self.mp.teams[self.chrs[j].mpslot];
                if t != myteam {
                    counts[t as usize & 7] += 1;
                }
            }
        }
        counts.into_iter().max().unwrap_or(0)
    }

    /// `bot_can_follow` (`bot.c:1775`): following `leader` wouldn't close a
    /// ring of followers.
    pub(crate) fn bot_can_follow(&self, bot: usize, leader: usize) -> bool {
        let mut leader = leader;
        for _ in 0..self.chrs.len() + 1 {
            let Some(a) = self.chrs[leader].aibot.as_ref() else { return true };
            let Some(next) = a.followingplayernum.filter(|_| a.myaction == MyAction::Follow) else { return true };
            leader = next;
            if leader == bot {
                return false;
            }
        }
        true
    }

    /// `bot_find_teammate_to_follow` (`bot.c:1799`): with teams on, not already
    /// following, and its follow chance (by difficulty) coming up, the nearest
    /// living teammate it can follow, if within `range`.
    pub(crate) fn bot_find_teammate_to_follow(&mut self, i: usize, range: f32) -> Option<usize> {
        if !self.setup.teams_enabled() || self.ab(i).myaction == MyAction::Follow {
            return None;
        }
        if (self.rng.random() % 100) as i32 >= self.ab(i).followchance {
            return None;
        }
        let mut closest: Option<(usize, f32)> = None;
        for j in 0..self.chrs.len() {
            if j != i && !self.chr_is_dead(j) && self.chrs[i].team == self.chrs[j].team && self.bot_can_follow(i, j) {
                let d = self.ab(i).chrdistances[j];
                if closest.is_none_or(|(_, cd)| d < cd) {
                    closest = Some((j, d));
                }
            }
        }
        closest.filter(|&(_, d)| d < range).map(|(j, _)| j)
    }

    /// `bot_passes_coward_check` (`bot.c:1557`): a CowardSim fights only chrs
    /// whose weapon scores 30 below its own.
    pub(crate) fn bot_passes_coward_check(&self, bot: usize, other: usize) -> bool {
        if self.ab(bot).config.bottype != BOTTYPE_COWARD {
            return true;
        }
        let (mine, _) = self.botinv_score_weapon(bot, self.ab(bot).weaponnum, FUNC_PRIMARY, 1, false, false, false);
        let (theirs, _) = self.botinv_score_weapon(bot, self.bot_get_weapon_num(other), FUNC_PRIMARY, 1, false, false, false);
        theirs < mine - 30
    }

    /// The scenario orders a simulant alone with its kind gives itself
    /// (`bot.c:2578`), every 20-60 s.
    fn bot_tick_scenario_commands(&mut self, i: usize) {
        if !self.ab(i).teamisonlyai {
            return;
        }
        let lv60 = self.lv.lvupdate60;
        if self.ab(i).commandtimer60 > 0 {
            self.ab_mut(i).commandtimer60 -= lv60;
        }
        if self.ab(i).commandtimer60 > 0 {
            return;
        }
        let teamsize = self.bot_get_team_size(i);
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE => {
                let n = self.bot_get_count_in_team_doing_command(i, AIBOTCMD_GETCASE2, false);
                let c = if n <= 0 || n < (teamsize + 1) / 2 || self.rng.random() % 100 < 66 { AIBOTCMD_GETCASE2 } else { AIBOTCMD_NORMAL };
                self.bot_apply_scenario_command(i, c);
            }
            MPSCENARIO_HACKERCENTRAL => {
                let n = self.bot_get_count_in_team_doing_command(i, AIBOTCMD_DOWNLOAD, false);
                let c = if self.ab(i).hasuplink || n <= 0 || n < (teamsize + 1) / 2 || self.rng.random() % 100 < 50 { AIBOTCMD_DOWNLOAD } else { AIBOTCMD_NORMAL };
                self.bot_apply_scenario_command(i, c);
            }
            MPSCENARIO_POPACAP => {
                let n = self.bot_get_count_in_team_doing_command(i, AIBOTCMD_POPCAP, false);
                let c = if n <= 0 || n < (teamsize + 1) / 2 || self.rng.random() % 100 < 50 { AIBOTCMD_POPCAP } else { AIBOTCMD_NORMAL };
                self.bot_apply_scenario_command(i, c);
            }
            MPSCENARIO_KINGOFTHEHILL => {
                let mut numinhill = self.bot_get_num_teammates_defending_hill(i);
                // Not counting itself.
                let hill = self.mp.scenariodata.koh.hillroom;
                if hill.is_some() && self.chr_room0(i) == hill {
                    numinhill -= 1;
                }
                let c = if numinhill <= 0 || numinhill < teamsize / 2 {
                    AIBOTCMD_HOLDHILL
                } else if numinhill > self.bot_get_num_opponents_in_hill(i) {
                    if self.rng.random() % 100 < 50 { AIBOTCMD_DEFHILL } else { AIBOTCMD_NORMAL }
                } else {
                    AIBOTCMD_HOLDHILL
                };
                self.bot_apply_scenario_command(i, c);
            }
            MPSCENARIO_CAPTURETHECASE => {
                let c = if teamsize == 1 {
                    let numgetting = self.bot_get_count_in_team_doing_command(i, AIBOTCMD_GETCASE, true);
                    if self.bot_should_return_ctc_token(i) {
                        AIBOTCMD_GETCASE
                    } else if self.ctc_is_chrs_token_held(i) {
                        if self.rng.random() % 100 < 30 { AIBOTCMD_GETCASE } else { AIBOTCMD_SAVECASE }
                    } else if self.rng.random() % 100 < 70 || numgetting <= 0 {
                        AIBOTCMD_GETCASE
                    } else {
                        AIBOTCMD_SAVECASE
                    }
                } else {
                    let numgetting = self.bot_get_count_in_team_doing_command(i, AIBOTCMD_GETCASE, false);
                    let numsaving = self.bot_get_count_in_team_doing_command(i, AIBOTCMD_SAVECASE, false);
                    if self.bot_should_return_ctc_token(i) {
                        AIBOTCMD_GETCASE
                    } else if self.ctc_is_chrs_token_held(i) {
                        if numsaving <= 0 || self.rng.random() % 100 < 70 { AIBOTCMD_SAVECASE } else { AIBOTCMD_GETCASE }
                    } else if numgetting <= 0 || numgetting < teamsize / 3 {
                        AIBOTCMD_GETCASE
                    } else if numsaving <= 0 || numsaving < teamsize / 4 {
                        AIBOTCMD_SAVECASE
                    } else if self.rng.random() % 100 < 30 {
                        AIBOTCMD_GETCASE
                    } else if self.rng.random() % 100 < 30 {
                        AIBOTCMD_SAVECASE
                    } else {
                        AIBOTCMD_NORMAL
                    }
                };
                self.bot_apply_scenario_command(i, c);
            }
            _ => {}
        }
        // Again in 20 to 60 seconds.
        let r = self.rng.random();
        self.ab_mut(i).commandtimer60 = 1200 + (r % 2400) as i32;
    }

    /// Protect chr `c` if it can (`MA_AIBOTFOLLOW`, a 1 in 4 chance of leaving
    /// to fight): the "held by a teammate" branches.
    fn bot_choose_protect(&mut self, i: usize, c: usize) -> Option<MyAction> {
        if !self.bot_can_follow(i, c) {
            return None;
        }
        let r = self.rng.random();
        let a = self.ab_mut(i);
        a.canbreakfollow = r.is_multiple_of(4);
        a.followingplayernum = Some(c);
        Some(MyAction::Follow)
    }

    /// Attack chr `c` if it can (`MA_AIBOTATTACK`, not giving up while it is
    /// out of sight).
    fn bot_choose_attack(&mut self, i: usize, c: usize, checkdead: bool) -> Option<MyAction> {
        if (checkdead && self.chr_is_dead(c)) || self.bot_is_target_invisible(i, c) || !self.bot_passes_coward_check(i, c) {
            return None;
        }
        let a = self.ab_mut(i);
        a.attackingplayernum = Some(c);
        a.abortattacktimer60 = -1;
        Some(MyAction::Attack)
    }

    /// Go into the hill (`botroom_find_pos`): King of the Hill's hold and
    /// defend orders.
    fn bot_choose_hill_spot(&mut self, i: usize) -> Option<MyAction> {
        let hill = self.mp.scenariodata.koh.hillroom?;
        let (pos, _angle, padnum, covernum) = self.botroom_find_pos(hill)?;
        let inhill = self.chr_room0(i) == Some(hill);
        let a = self.ab_mut(i);
        a.gotopos = pos;
        a.gotorooms = vec![hill];
        a.inhill = inhill;
        a.hillpadnum = padnum;
        a.hillcovernum = covernum;
        a.lastknownhill = Some(hill);
        Some(MyAction::GotoPos)
    }

    /// What `aibot->command` asks for (`bot.c:2704`), the scenario orders.
    fn bot_choose_command_action(&mut self, i: usize) -> Option<MyAction> {
        let command = self.ab(i).command;
        let scen = self.setup.scenario;
        let teams = self.setup.teams_enabled();
        match command {
            AIBOTCMD_GETCASE if scen == MPSCENARIO_CAPTURETHECASE && !self.ab(i).hascase => {
                // The other teams' cases, but those carried by other teams.
                let botteam = self.chr_team_index(i);
                let mut tokens: Vec<PropRef> = Vec::new();
                for t in 0..4 {
                    let d = &self.mp.scenariodata.ctc;
                    if t == botteam || d.playercountsperteam[t] == 0 {
                        continue;
                    }
                    match d.tokens[t] {
                        Some(PropRef::Obj(id)) => tokens.push(PropRef::Obj(id)),
                        Some(PropRef::Chr(c)) if self.chrs[c].team == self.chrs[i].team => tokens.push(PropRef::Chr(c)),
                        _ => {}
                    }
                }
                if tokens.is_empty() {
                    return None;
                }
                // Preferably one within 10 m.
                let n = tokens.len();
                let mut index = (self.rng.random() % n as u32) as usize;
                let mut k = (index + 1) % n;
                loop {
                    let d2 = self.scenario_prop_pos(tokens[k]).map_or(f32::MAX, |p| p.distance_squared(self.chrs[i].pos));
                    if d2 < 1000.0 * 1000.0 {
                        index = k;
                        break;
                    }
                    if k == index {
                        break;
                    }
                    k = (k + 1) % n;
                }
                match tokens[index] {
                    PropRef::Obj(id) => {
                        self.ab_mut(i).gotoprop = Some(id);
                        Some(MyAction::GetItem)
                    }
                    PropRef::Chr(c) => self.bot_choose_protect(i, c),
                }
            }
            AIBOTCMD_SAVECASE if scen == MPSCENARIO_CAPTURETHECASE => {
                let team = self.chr_team_index(i);
                match self.mp.scenariodata.ctc.tokens[team] {
                    Some(PropRef::Chr(c)) if self.chrs[c].team == self.chrs[i].team => self.bot_choose_protect(i, c),
                    Some(PropRef::Chr(c)) => self.bot_choose_attack(i, c, true),
                    Some(PropRef::Obj(id)) => {
                        // Stand guard where it lies.
                        let (pos, room) = self.props.get(id).map(|o| (o.pos, o.room))?;
                        let a = self.ab_mut(i);
                        a.gotopos = pos;
                        a.gotorooms = room.into_iter().collect();
                        a.inhill = false;
                        Some(MyAction::GotoPos)
                    }
                    None => None,
                }
            }
            AIBOTCMD_DEFHILL if scen == MPSCENARIO_KINGOFTHEHILL => {
                let hill = self.mp.scenariodata.koh.hillroom;
                let target = self.chrs[i].target;
                if hill.is_some() && self.chr_room0(i) == hill && self.ab(i).targetinsight && target.is_some_and(|t| self.bot_passes_coward_check(i, t)) {
                    let a = self.ab_mut(i);
                    a.attackingplayernum = target;
                    a.abortattacktimer60 = 300;
                    Some(MyAction::Attack)
                } else {
                    self.bot_choose_hill_spot(i)
                }
            }
            AIBOTCMD_HOLDHILL if scen == MPSCENARIO_KINGOFTHEHILL => self.bot_choose_hill_spot(i),
            AIBOTCMD_DOWNLOAD if scen == MPSCENARIO_HACKERCENTRAL => match self.mp.scenariodata.htm.uplink {
                Some(PropRef::Chr(c)) if c == i => None,
                Some(PropRef::Chr(c)) if teams && self.chrs[c].team == self.chrs[i].team => self.bot_choose_protect(i, c),
                Some(PropRef::Chr(c)) => self.bot_choose_attack(i, c, false),
                Some(PropRef::Obj(id)) => {
                    self.ab_mut(i).gotoprop = Some(id);
                    Some(MyAction::GotoProp)
                }
                None => None,
            },
            AIBOTCMD_GETCASE2 if scen == MPSCENARIO_HOLDTHEBRIEFCASE => match self.mp.scenariodata.htb.token {
                Some(PropRef::Chr(c)) if c == i => None,
                Some(PropRef::Chr(c)) if teams && self.chrs[c].team == self.chrs[i].team => self.bot_choose_protect(i, c),
                Some(PropRef::Chr(c)) => self.bot_choose_attack(i, c, false),
                Some(PropRef::Obj(id)) => {
                    self.ab_mut(i).gotoprop = Some(id);
                    Some(MyAction::GotoProp)
                }
                None => None,
            },
            AIBOTCMD_POPCAP if scen == MPSCENARIO_POPACAP => {
                let v = self.pac_victim()?;
                if v == i {
                    None
                } else if teams && self.chrs[v].team == self.chrs[i].team {
                    self.bot_choose_protect(i, v)
                } else {
                    self.bot_choose_attack(i, v, false)
                }
            }
            _ => None,
        }
    }

    /// With nothing ordered (`bot.c:2986`): a case's carrier or the victim
    /// keeps near a teammate; Capture the Case's carrier heads home; Hacker
    /// Central's uplink carrier goes to the terminal, then downloads.
    fn bot_choose_scenario_fallback(&mut self, i: usize) -> Option<MyAction> {
        match self.setup.scenario {
            MPSCENARIO_HOLDTHEBRIEFCASE | MPSCENARIO_POPACAP => {
                let mine = if self.setup.scenario == MPSCENARIO_HOLDTHEBRIEFCASE { self.ab(i).hasbriefcase } else { self.pac_victim() == Some(i) };
                if !mine {
                    return None;
                }
                let playernum = if self.rng.random() % 100 < 66 { self.bot_find_teammate_to_follow(i, 100_000.0) } else { None };
                let p = playernum?;
                let r = self.rng.random();
                let a = self.ab_mut(i);
                a.canbreakfollow = r.is_multiple_of(4);
                a.followingplayernum = Some(p);
                Some(MyAction::Follow)
            }
            MPSCENARIO_CAPTURETHECASE if self.bot_should_return_ctc_token(i) => {
                let team = self.chr_team_index(i);
                let base = self.mp.scenariodata.ctc.teamindexes[team];
                let homepad = self.mp.scenariodata.ctc.spawnpadsperteam.get(base.max(0) as usize).map_or(-1, |s| s.homepad);
                let (pos, room) = self.stage.pads.get(homepad.max(0) as usize).map_or((Vec3::ZERO, None), |p| (p.pos, p.room));
                let a = self.ab_mut(i);
                a.gotopos = pos;
                a.gotorooms = room.into_iter().collect();
                a.inhill = false;
                Some(MyAction::GotoPos)
            }
            MPSCENARIO_HACKERCENTRAL if self.mp.scenariodata.htm.uplink == Some(PropRef::Chr(i)) => {
                if self.mp.scenariodata.htm.playernuminrange as i32 != i as i32 {
                    self.ab_mut(i).gotoprop = self.mp.scenariodata.htm.terminals[0].prop;
                    Some(MyAction::GotoProp)
                } else {
                    Some(MyAction::Download)
                }
            }
            _ => None,
        }
    }

    /// `chr_go_to_prop` (`chraction.c:7268`) for an object: its position, in
    /// its room.
    fn bot_go_to_obj(&mut self, i: usize, id: u32) -> bool {
        let Some((pos, room)) = self.props.get(id).map(|o| (o.pos, o.room)) else { return false };
        let rooms: Vec<u16> = room.into_iter().collect();
        self.chr_go_to_room_pos(i, pos, &rooms)
    }

    /// The main loop (`bot.c:2681`) and each action's check (`bot.c:3223`).
    pub(crate) fn bot_tick_mainloop(&mut self, i: usize) {
        let lvframe60 = self.lv.lvframe60;
        // The laser has unlimited ammo (bot.c:2523).
        if self.ab(i).weaponnum == WEAPON_LASER {
            self.ab_mut(i).loadedammo[HAND_RIGHT] = 999;
        }
        // (bot.c:2527: the cloaking device and the RC-P120's cloak, which a
        // downloader also uses: M11.)
        // A KazeSim attacks on sight.
        if self.ab(i).config.bottype == BOTTYPE_KAZE && self.chrs[i].target.is_some() && self.ab(i).targetinsight && self.ab(i).myaction != MyAction::Attack {
            self.ab_mut(i).forcemainloop = true;
        }
        self.bot_tick_scenario_commands(i);

        if self.ab(i).myaction == MyAction::MainLoop || self.ab(i).forcemainloop {
            {
                let a = self.ab_mut(i);
                a.forcemainloop = false;
                a.attackingplayernum = None;
            }
            let mut newaction = None;
            if self.ab(i).config.bottype == BOTTYPE_KAZE && self.ab(i).targetinsight {
                if let Some(t) = self.chrs[i].target {
                    let a = self.ab_mut(i);
                    a.attackingplayernum = Some(t);
                    a.abortattacktimer60 = -1;
                    newaction = Some(MyAction::Attack);
                }
            }
            if newaction.is_none() {
                let gotoprop = self.bot_find_pickup(i, botinv::PICKUPCRITERIA_DEFAULT);
                self.ab_mut(i).gotoprop = gotoprop;
                if gotoprop.is_some() {
                    newaction = Some(MyAction::GetItem);
                }
            }
            if newaction.is_none() {
                newaction = self.bot_choose_command_action(i);
            }
            if newaction.is_none() {
                newaction = self.bot_choose_scenario_fallback(i);
            }
            // (bot.c:3052: VengeSim, FeudSim, JudgeSim and PreySim's own
            // targets: M11.)
            if newaction.is_none() {
                if let Some(t) = self.chrs[i].target {
                    if self.bot_passes_coward_check(i, t) {
                        newaction = Some(MyAction::Attack);
                        self.ab_mut(i).abortattacktimer60 = -1;
                    }
                }
            }
            if newaction.is_none() {
                if let Some(p) = self.bot_find_teammate_to_follow(i, 300.0) {
                    let r = self.rng.random();
                    let a = self.ab_mut(i);
                    a.canbreakfollow = r.is_multiple_of(4);
                    a.followingplayernum = Some(p);
                    newaction = Some(MyAction::Follow);
                }
            }
            if newaction.is_none() {
                let gotoprop = self.bot_find_pickup(i, botinv::PICKUPCRITERIA_ANY);
                self.ab_mut(i).gotoprop = gotoprop;
                if gotoprop.is_some() {
                    newaction = Some(MyAction::GetItem);
                }
            }
            match newaction {
                Some(MyAction::GetItem) => {
                    if let Some(id) = self.ab(i).gotoprop {
                        self.bot_go_to_obj(i, id);
                        self.ab_mut(i).myaction = MyAction::GetItem;
                    }
                }
                Some(MyAction::Attack) => {
                    let a = self.ab_mut(i);
                    if a.myaction != MyAction::Attack {
                        a.myaction = MyAction::Attack;
                        a.distmode = None;
                    }
                }
                Some(MyAction::Follow) => {
                    if self.ab(i).myaction != MyAction::Follow {
                        let a = self.ab_mut(i);
                        a.myaction = MyAction::Follow;
                        a.distmode = None;
                        if a.canbreakfollow {
                            self.bot_set_target(i, None);
                        }
                    }
                }
                Some(MyAction::GotoPos) => {
                    let (pos, gp, inlift) = (self.chrs[i].pos, self.ab(i).gotopos, self.chrs[i].inlift);
                    let d = (pos - gp).abs();
                    if d.x > 20.0 || d.z > 20.0 || (d.y > 200.0 && !inlift) {
                        self.ab_mut(i).myaction = MyAction::GotoPos;
                        let rooms = self.ab(i).gotorooms.clone();
                        self.chr_go_to_room_pos(i, gp, &rooms);
                    } else {
                        self.chr_stand(i);
                    }
                }
                Some(MyAction::GotoProp) => {
                    if let Some(id) = self.ab(i).gotoprop {
                        self.ab_mut(i).myaction = MyAction::GotoProp;
                        self.bot_go_to_obj(i, id);
                    }
                }
                Some(MyAction::Download) => {
                    self.ab_mut(i).myaction = MyAction::Download;
                    self.chr_stand(i);
                }
                _ => {}
            }
        }

        // The action no longer makes sense: back to the main loop.
        let gopos = self.chrs[i].actiontype == Act::GoPos;
        match self.ab(i).myaction {
            MyAction::GetItem => {
                let gone = self.ab(i).gotoprop.and_then(|id| self.props.get(id)).is_none_or(|o| o.timetoregen != 0 || o.is_gone());
                if !gopos || gone {
                    self.ab_mut(i).myaction = MyAction::MainLoop;
                }
            }
            MyAction::Attack => {
                let invalid = match self.ab(i).attackingplayernum {
                    Some(p) => self.chr_is_dead(p) || !self.bot_passes_coward_check(i, p),
                    None => self.chrs[i].target.is_none_or(|t| self.chr_is_dead(t) || !self.bot_passes_coward_check(i, t)),
                };
                if invalid {
                    self.ab_mut(i).myaction = MyAction::MainLoop;
                } else {
                    self.botcmd_tick_dist_mode(i);
                    if self.bot_find_pickup(i, botinv::PICKUPCRITERIA_CRITICAL).is_some() {
                        // bot_can_do_critical_pickup.
                        self.ab_mut(i).myaction = MyAction::MainLoop;
                    } else {
                        let a = self.ab(i);
                        if a.abortattacktimer60 >= 0 && a.targetlastseen60 < lvframe60 - a.abortattacktimer60 {
                            self.ab_mut(i).myaction = MyAction::MainLoop;
                        }
                    }
                }
            }
            MyAction::Follow => {
                match self.ab(i).followingplayernum.filter(|&f| !self.chr_is_dead(f)) {
                    None => self.ab_mut(i).myaction = MyAction::MainLoop,
                    Some(f) => {
                        self.botcmd_tick_dist_mode(i);
                        let target = self.chrs[i].target;
                        if self.ab(i).canbreakfollow && self.ab(i).targetinsight && target.is_some_and(|t| self.bot_passes_coward_check(i, t)) {
                            let d = (self.chrs[i].pos - self.chrs[f].pos).abs();
                            // No y check (PD's).
                            if d.x < 500.0 && d.z < 500.0 {
                                let a = self.ab_mut(i);
                                a.myaction = MyAction::Attack;
                                a.attackingplayernum = target;
                                a.abortattacktimer60 = 300;
                                a.distmode = None;
                            }
                        }
                        if self.bot_find_pickup(i, botinv::PICKUPCRITERIA_CRITICAL).is_some() {
                            self.ab_mut(i).myaction = MyAction::MainLoop;
                        }
                    }
                }
            }
            MyAction::GotoPos => {
                if self.setup.scenario == MPSCENARIO_KINGOFTHEHILL && self.ab(i).inhill {
                    let hill = self.mp.scenariodata.koh.hillroom;
                    let a = self.ab(i);
                    if a.lastknownhill != hill {
                        // Someone scored the hill.
                        self.ab_mut(i).inhill = false;
                    } else if hill.is_some() && self.chr_room0(i) == hill {
                    } else if a.hillpadnum >= 0 {
                        let p = a.hillpadnum as usize;
                        if let Some(f) = self.mp.scenariodata.koh.pad_aibotinuse.get_mut(p) {
                            *f = true;
                        }
                    } else if a.hillcovernum >= 0 {
                        let c = a.hillcovernum as usize;
                        if let Some(f) = self.mp.scenariodata.koh.coverflags.get_mut(c) {
                            *f |= COVERFLAG_AIBOTINUSE;
                        }
                    }
                }
                if !gopos || self.bot_find_pickup(i, botinv::PICKUPCRITERIA_CRITICAL).is_some() {
                    self.ab_mut(i).myaction = MyAction::MainLoop;
                }
            }
            MyAction::GotoProp => {
                let missing = self.ab(i).gotoprop.and_then(|id| self.props.get(id)).is_none();
                if self.bot_find_pickup(i, botinv::PICKUPCRITERIA_CRITICAL).is_some() || !gopos || missing {
                    self.ab_mut(i).myaction = MyAction::MainLoop;
                } else if self.setup.scenario == MPSCENARIO_HACKERCENTRAL && self.mp.scenariodata.htm.uplink == Some(PropRef::Chr(i)) && self.mp.scenariodata.htm.playernuminrange as i32 == i as i32 {
                    self.ab_mut(i).myaction = MyAction::MainLoop;
                }
            }
            MyAction::Download => {
                if self.bot_find_pickup(i, botinv::PICKUPCRITERIA_CRITICAL).is_some() || self.mp.scenariodata.htm.playernuminrange as i32 != i as i32 {
                    self.ab_mut(i).myaction = MyAction::MainLoop;
                }
            }
            MyAction::MainLoop => {}
        }
    }
}
