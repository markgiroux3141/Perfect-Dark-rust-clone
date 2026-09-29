//! `botcmd.c`: how an attacking simulant decides to close in, hold or back off
//! (`botcmd_tick_dist_mode`), by its weapon's distance band (`g_BotDistConfigs`).
//!
//! Source: the old repo's `pd_spike/botcmd.rs`.

use pd_core::ids::{BOTDIFF_EASY, BOTDIFF_MEAT};

use super::{botinv_get_dist_config, MyAction};
use crate::chr::Act;
use crate::world::World;

/// `BOTDISTMODE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistMode {
    Backup = 1,
    Ok = 2,
    Advance = 3,
    Goto = 4,
}

impl DistMode {
    pub fn label(self) -> &'static str {
        match self {
            DistMode::Backup => "BACKUP",
            DistMode::Ok => "OK",
            DistMode::Advance => "ADVANCE",
            DistMode::Goto => "GOTO",
        }
    }
}

/// One `g_BotDistConfigs` row (cm): back up nearer than `min`, hold up to
/// `max`, advance up to `limit3`, beyond it go to the target.
#[derive(Clone, Copy, Debug)]
pub struct DistConfig {
    pub min: f32,
    pub max: f32,
    pub limit3: f32,
}

/// `g_BotDistConfigs` (`botcmd.c:29`).
pub const BOT_DIST_CONFIGS: [DistConfig; 8] = [
    DistConfig { min: 0.0, max: 120.0, limit3: 10000.0 },    // BOTDISTCFG_CLOSE
    DistConfig { min: 300.0, max: 450.0, limit3: 4500.0 },   // BOTDISTCFG_PISTOL
    DistConfig { min: 300.0, max: 600.0, limit3: 4500.0 },   // BOTDISTCFG_DEFAULT
    DistConfig { min: 600.0, max: 1200.0, limit3: 4500.0 },  // BOTDISTCFG_SHOOTEXPLOSIVE
    DistConfig { min: 150.0, max: 250.0, limit3: 4500.0 },   // BOTDISTCFG_KAZE
    DistConfig { min: 1000.0, max: 2000.0, limit3: 3000.0 }, // BOTDISTCFG_FARSIGHT
    DistConfig { min: 0.0, max: 250.0, limit3: 10000.0 },    // BOTDISTCFG_FOLLOW
    DistConfig { min: 450.0, max: 700.0, limit3: 4500.0 },   // BOTDISTCFG_THROWEXPLOSIVE
];

pub const BOTDISTCFG_CLOSE: usize = 0;

impl World {
    /// `botcmd_tick_dist_mode` (`botcmd.c:61`), the free-for-all branch (no
    /// follow, no KazeSim).
    pub(crate) fn botcmd_tick_dist_mode(&mut self, i: usize) {
        let gset = self.res.gset.clone();
        let a = self.ab(i);
        let confignum = botinv_get_dist_config(&gset, a.weaponnum, a.gunfunc);
        let (targetprop, insight) = match (a.myaction, a.attackingplayernum) {
            (MyAction::Attack, Some(p)) => (Some(p), a.chrsinsight[p]),
            _ => (self.chrs[i].target, a.targetinsight),
        };
        let prevmode = a.distmode;
        let Some(targetprop) = targetprop else { return };
        let limits = BOT_DIST_CONFIGS[confignum.min(7)];
        let target_pos = self.chrs[targetprop].pos;
        // chr_get_distance_to_coord: 3D, from the root.
        let distance = self.chrs[i].pos.distance(target_pos);
        let mut minattackdistance = limits.min;
        let mut maxattackdistance = limits.max;
        let limit3 = limits.limit3;
        match a.config.difficulty {
            BOTDIFF_MEAT => minattackdistance *= 0.35,
            BOTDIFF_EASY => minattackdistance *= 0.5,
            _ => {}
        }
        match a.distmode {
            Some(DistMode::Backup) => minattackdistance += 25.0,
            Some(DistMode::Advance) | Some(DistMode::Goto) => maxattackdistance -= 25.0,
            _ => {}
        }
        let mut newmode = if distance < minattackdistance {
            DistMode::Backup
        } else if distance < maxattackdistance {
            DistMode::Ok
        } else if distance < limit3 {
            DistMode::Advance
        } else {
            DistMode::Goto
        };
        let lvupdate60 = self.lv.lvupdate60;
        let r = if newmode == DistMode::Backup && !insight { Some(self.rng.random()) } else { None };
        let actiontype = self.chrs[i].actiontype;
        let a = self.ab_mut(i);
        a.last_dist = distance;
        if newmode == DistMode::Backup && insight && a.distoverrideprop == Some(targetprop) {
            // don't unset
        } else {
            a.distoverrideprop = None;
            a.distoverridetimer60 = 0;
        }
        if newmode == DistMode::Ok {
            if !insight {
                newmode = DistMode::Advance;
            }
        } else if newmode == DistMode::Backup {
            // Backing up with the target out of sight becomes advancing; once it
            // is back in sight, hold OK for a random while before backing up
            // again (no backup/advance loop round a corner).
            if !insight {
                newmode = DistMode::Advance;
                a.distoverrideprop = Some(targetprop);
                a.distoverridetimer60 = 20 + (r.unwrap() % 120) as i32;
            } else if a.distoverrideprop.is_some() {
                if lvupdate60 < a.distoverridetimer60 {
                    a.distoverridetimer60 -= lvupdate60;
                    newmode = DistMode::Ok;
                } else {
                    a.distoverrideprop = None;
                    a.distoverridetimer60 = 0;
                }
            }
        }
        a.distmode = Some(newmode);
        if a.distmodettl60 >= 0 {
            a.distmodettl60 -= lvupdate60;
        }
        let reissue = Some(newmode) != prevmode || (newmode != DistMode::Ok && (actiontype == Act::Stand || a.distmodettl60 <= 0));
        if reissue {
            match newmode {
                DistMode::Backup => {
                    self.chr_run_from_pos(i, 10000.0, target_pos);
                }
                DistMode::Ok => {
                    self.chr_try_stop(i);
                }
                DistMode::Advance | DistMode::Goto => {
                    // chr_go_to_prop (`chraction.c:7268`): the target's rooms.
                    let rooms = self.chrs[targetprop].rooms.clone();
                    self.chr_go_to_room_pos(i, target_pos, &rooms);
                }
            }
            self.ab_mut(i).distmodettl60 = 60;
        }
    }
}
