//! `mp_calculate_awards` (`mplayer.c:1962`): at the end of a match, each human
//! player's two awards, the medals, and the match added to the player's file
//! (with the title worked out again). PD's bugs are kept: the float searches
//! compare against a truncated best value, and the KillMaster medal skips the
//! kills on chr slot `playercount` (a stale loop index) and counts suicides.

use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::mp::{mp_calculate_player_title, MpPlayerResult, MAX_MPCHRS};

use super::{mp_find_max_float, mp_find_max_int, mp_find_min_float, mp_find_min_int};
use crate::world::World;

/// `struct awardmetrics`.
#[derive(Clone, Copy, Debug, Default)]
struct AwardMetrics {
    numshots: i32,
    numheadshots: i32,
    numkills: i32,
    numdeaths: i32,
    numsuicides: i32,
    ksratio: f32,
    kdratio: f32,
    backshotcount: i32,
    drawplayercount: i32,
    avgkmperhour: f32,
    armourcount: f32,
    awards: u32,
    longestlife: i32,
    shortestlife: i32,
    accuracyfrac: f32,
}

fn col_i(m: &[AwardMetrics; 4], f: fn(&AwardMetrics) -> i32) -> [i32; 4] {
    std::array::from_fn(|k| f(&m[k]))
}

fn col_f(m: &[AwardMetrics; 4], f: fn(&AwardMetrics) -> f32) -> [f32; 4] {
    std::array::from_fn(|k| f(&m[k]))
}

impl World {
    /// `mp_calculate_awards` (`mplayer.c:1962`, NTSC 1.0+). No cheats here, so
    /// the files always take the match.
    pub(crate) fn mp_calculate_awards(&mut self) {
        let playercount = self.players.len();
        // duration60 = player_get_mission_time() of the current player (the
        // last one the frame ticked).
        let duration60 = self.player_get_mission_time(playercount.saturating_sub(1));
        self.push_event(Event::StopAllSounds);
        self.mp.alarm = false;
        let rankings = self.mp_get_player_rankings();
        let numchrs = rankings.rankings.len();
        let scenario = self.scenario_scores();
        let numteams = if self.setup.teams_enabled() { self.mp_scoring(&scenario).mp_get_team_rankings().len() } else { 0 };
        let chrslots = self.setup.chrslots();
        let teams = self.setup.teams_enabled();
        let mut metrics = [AwardMetrics::default(); 4];
        let mut careers = self.mp.careers.clone();
        let mut medals = vec![0u8; playercount];

        // Iterate all human players and update their character stats.
        for i in 0..playercount {
            // mp_get_chr_index_by_slot_num(i): the i-th chr taking part, a player.
            let chrnum = self.setup.players[i].slot as usize;
            let mpchr = self.mp.chrs[chrnum];
            self.mp.players[i].award1 = None;
            self.mp.players[i].award2 = None;
            let s = &self.mp.playerstats[i];
            let m = &mut metrics[i];
            m.numshots = s.shotcount[SHOTREGION_TOTAL];
            m.numheadshots = s.shotcount[SHOTREGION_HEAD];
            for j in 0..MAX_MPCHRS {
                if chrnum == j {
                    m.numsuicides += mpchr.killcounts[j] as i32;
                } else {
                    m.numkills += mpchr.killcounts[j] as i32;
                }
            }
            for j in 0..MAX_MPCHRS {
                m.numdeaths += self.mp.chrs[j].killcounts[chrnum] as i32;
            }
            m.ksratio = m.numkills as f32 * 100.0 / (m.numshots as f32 + 1.0);
            m.kdratio = m.numkills as f32 * 100.0 / (m.numdeaths as f32 + 1.0);
            m.backshotcount = s.backshotcount;
            m.drawplayercount = s.drawplayercount;
            m.avgkmperhour = s.distance / 100000.0 / ((duration60 + 1) as f32 / (3600.0 * 60.0));
            m.armourcount = s.armourcount;
            m.awards = 0;
            m.longestlife = s.longestlife;
            m.shortestlife = s.shortestlife;
            let sum = s.shotcount[SHOTREGION_HEAD] + s.shotcount[SHOTREGION_BODY] + s.shotcount[SHOTREGION_LIMB] + s.shotcount[SHOTREGION_GUN] + s.shotcount[SHOTREGION_HAT] + s.shotcount[SHOTREGION_OBJECT];
            m.accuracyfrac = if m.numshots > 0 { sum as f32 / m.numshots as f32 } else { 0.0 };
            if m.accuracyfrac > 1.0 {
                m.accuracyfrac = 1.0;
            }
            // The player's file.
            let c = &mut careers[i];
            c.kills += m.numkills as u32;
            c.deaths += m.numdeaths as u32;
            c.gamesplayed += 1;
            c.time += (duration60 / 60) as u32;
            c.distance += (s.distance / 10000.0) as u32;
            if m.numshots > 0 {
                c.accuracy = if c.gamesplayed < 2 { (m.accuracyfrac * 1000.0) as u32 } else { ((m.accuracyfrac * 0.3 + c.accuracy as f32 / 1000.0 * 0.7) * 1000.0) as u32 };
            }
            c.damagedealt += (s.damtransmitted / 0.1) as u32;
            c.painreceived += (s.damreceived / 0.1) as u32;
            c.headshots += m.numheadshots as u32;
            c.ammoused += m.numshots as u32;
            if (numchrs >= 2 && !teams) || numteams >= 2 {
                let me = self.mp.chrs[chrnum];
                let myteam = self.mp.teams[chrnum];
                let tied = |j: usize| chrslots & (1 << j) != 0 && self.mp.chrs[j].rankablescore == me.rankablescore && j != chrnum && !(teams && self.mp.teams[j] == myteam);
                if me.placement == 0 && !(0..MAX_MPCHRS).any(tied) {
                    c.gameswon += 1;
                }
                let last = if teams { numteams } else { numchrs };
                if last == me.placement as usize + 1 && !(0..MAX_MPCHRS).any(tied) {
                    c.gameslost += 1;
                }
            }
        }

        // Choose which players are eligible for which awards
        let rng = &mut self.rng;
        let i = mp_find_max_int(rng, playercount, col_i(&metrics, |m| m.numsuicides));
        if metrics[i].numsuicides > 0 {
            metrics[i].awards |= AWARD_MOSTSUICIDAL;
        }
        let i = mp_find_min_int(rng, playercount, col_i(&metrics, |m| m.numshots));
        if metrics[i].numshots < 100 {
            metrics[i].awards |= AWARD_WHONEEDSAMMO;
        }
        let i = mp_find_min_float(rng, playercount, col_f(&metrics, |m| m.armourcount));
        if metrics[i].armourcount <= 2.0 {
            metrics[i].awards |= AWARD_LEASTSHIELDED;
        }
        let i = mp_find_max_float(rng, playercount, col_f(&metrics, |m| m.armourcount));
        if metrics[i].armourcount > 6.0 {
            metrics[i].awards |= AWARD_BESTPROTECTED;
        }
        let i = mp_find_max_int(rng, playercount, col_i(&metrics, |m| m.numheadshots));
        if metrics[i].numheadshots > 0 {
            metrics[i].awards |= AWARD_MARKSMANSHIP;
        }
        let i = mp_find_max_float(rng, playercount, col_f(&metrics, |m| m.ksratio));
        if metrics[i].ksratio > 0.0 {
            metrics[i].awards |= AWARD_MOSTPROFESSIONAL;
        }
        let i = mp_find_max_float(rng, playercount, col_f(&metrics, |m| m.kdratio));
        if metrics[i].kdratio > 0.0 {
            metrics[i].awards |= AWARD_MOSTDEADLY;
        }
        let i = mp_find_min_float(rng, playercount, col_f(&metrics, |m| m.kdratio));
        if playercount >= 2 {
            metrics[i].awards |= AWARD_MOSTHARMLESS;
        }
        let i = mp_find_min_int(rng, playercount, col_i(&metrics, |m| m.drawplayercount));
        if playercount >= 2 {
            metrics[i].awards |= AWARD_MOSTCOWARDLY;
        }
        let i = mp_find_max_float(rng, playercount, col_f(&metrics, |m| m.avgkmperhour));
        if metrics[i].avgkmperhour > 10.0 {
            metrics[i].awards |= AWARD_MOSTFRANTIC;
        }
        let i = mp_find_min_int(rng, playercount, col_i(&metrics, |m| m.backshotcount));
        metrics[i].awards |= AWARD_MOSTHONORABLE;
        let i = mp_find_max_int(rng, playercount, col_i(&metrics, |m| m.backshotcount));
        if metrics[i].backshotcount > 0 && metrics[i].awards & AWARD_MOSTHONORABLE == 0 {
            metrics[i].awards |= AWARD_MOSTDISHONORABLE;
        }
        let i = mp_find_max_int(rng, playercount, col_i(&metrics, |m| m.longestlife));
        if metrics[i].longestlife > 0 {
            metrics[i].awards |= AWARD_LONGESTLIFE;
        }
        let i = mp_find_min_int(rng, playercount, col_i(&metrics, |m| m.shortestlife));
        if metrics[i].shortestlife > 0 && metrics[i].numdeaths > 0 {
            metrics[i].awards |= AWARD_SHORTESTLIFE;
        }
        for i in 0..playercount {
            match self.mp.playerstats[i].maxsimulkills {
                4 => metrics[i].awards |= AWARD_QUADKILL,
                3 => metrics[i].awards |= AWARD_TRIPLEKILL,
                2 => metrics[i].awards |= AWARD_DOUBLEKILL,
                _ => {}
            }
        }

        // For each player, choose which two awards to actually give them:
        // quad kill first, then at random.
        for i in 0..playercount {
            let m = &mut metrics[i];
            let v = &mut self.mp.players[i];
            let mut numdone = 0;
            let mut awardindex = 16u32;
            if playercount == 1 {
                // In a single player game, only allow the following awards
                m.awards &= AWARD_MARKSMANSHIP | AWARD_MOSTPROFESSIONAL | AWARD_MOSTDEADLY | AWARD_MOSTFRANTIC | AWARD_MOSTHONORABLE | AWARD_DOUBLEKILL | AWARD_TRIPLEKILL | AWARD_QUADKILL;
            }
            while numdone == 0 {
                if m.awards & (1 << awardindex) != 0 {
                    m.awards &= !(1 << awardindex);
                    v.award1 = Some(awardindex as u8);
                    numdone = 1;
                }
                if m.awards == 0 {
                    numdone = 1;
                }
                awardindex = self.rng.random() % 17;
            }
            while numdone < 2 {
                awardindex = self.rng.random() % 17;
                if m.awards & (1 << awardindex) != 0 {
                    m.awards &= !(1 << awardindex);
                    v.award2 = Some(awardindex as u8);
                    numdone = 2;
                }
                if m.awards == 0 {
                    numdone = 2;
                }
            }
        }

        // Calculate KillMaster and Survivor medals
        if numchrs >= 2 {
            let (mut mostkillsvalue, mut mostkillsplayer) = (0i32, -1i32);
            let (mut leastdeathsvalue, mut leastdeathsplayer) = (0xffffffi32, -1i32);
            // PD's @bug: the loop compares with `i`, the award loop's index,
            // which ended at playercount.
            let i = playercount;
            for k in 0..MAX_MPCHRS {
                if chrslots & (1 << k) != 0 {
                    let mpchr = &self.mp.chrs[k];
                    let totalkills: i32 = (0..MAX_MPCHRS).filter(|&j| j != i).map(|j| mpchr.killcounts[j] as i32).sum();
                    if totalkills == mostkillsvalue {
                        mostkillsplayer = -1;
                    }
                    if totalkills > mostkillsvalue {
                        mostkillsplayer = k as i32;
                        mostkillsvalue = totalkills;
                    }
                    let d = mpchr.numdeaths as i32;
                    if d == leastdeathsvalue {
                        leastdeathsplayer = -1;
                    }
                    if d < leastdeathsvalue {
                        leastdeathsplayer = k as i32;
                        leastdeathsvalue = d;
                    }
                }
            }
            // A chr slot below 4 is a player: its file's medal.
            let player_of_slot = |slot: i32| self.setup.players.iter().position(|p| p.slot as i32 == slot);
            if (0..4).contains(&mostkillsplayer) {
                if let Some(p) = player_of_slot(mostkillsplayer) {
                    medals[p] |= MEDAL_KILLMASTER;
                    careers[p].killmastermedals += 1;
                }
            }
            if (0..4).contains(&leastdeathsplayer) {
                if let Some(p) = player_of_slot(leastdeathsplayer) {
                    medals[p] |= MEDAL_SURVIVOR;
                    careers[p].survivormedals += 1;
                }
            }
        }

        // Calculate Headshot and Accuracy medals
        if playercount >= 2 {
            let (mut mostheadshotvalue, mut mostheadshotplayer) = (0i32, -1i32);
            let (mut mostaccuratevalue, mut mostaccurateplayer) = (0.5f32, -1i32);
            for (i, m) in metrics.iter().enumerate().take(playercount) {
                if mostheadshotvalue == m.numheadshots {
                    mostheadshotplayer = -1;
                }
                if m.numheadshots > mostheadshotvalue {
                    mostheadshotplayer = i as i32;
                    mostheadshotvalue = m.numheadshots;
                }
                if m.accuracyfrac > mostaccuratevalue {
                    mostaccurateplayer = i as i32;
                    mostaccuratevalue = m.accuracyfrac;
                }
            }
            if mostheadshotplayer >= 0 {
                medals[mostheadshotplayer as usize] |= MEDAL_HEADSHOT;
                careers[mostheadshotplayer as usize].headshotmedals += 1;
            }
            if mostaccurateplayer >= 0 {
                medals[mostaccurateplayer as usize] |= MEDAL_ACCURACY;
                careers[mostaccurateplayer as usize].accuracymedals += 1;
            }
        }

        // Recalculate title for all players
        self.mp.results = (0..playercount)
            .map(|i| MpPlayerResult { slot: self.setup.players[i].slot as usize, career: careers[i], medals: medals[i], title: mp_calculate_player_title(&careers[i]) })
            .collect();
    }
}
