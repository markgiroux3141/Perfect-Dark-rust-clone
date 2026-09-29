//! `botcmd.c`: how an attacking simulant decides to close in, hold or back off
//! (`botcmd_tick_dist_mode`), by its weapon's distance band (`g_BotDistConfigs`).
//!
//! Source: the old repo's `pd_spike/botcmd.rs`.

use pd_core::ids::{BOTDIFF_EASY, BOTDIFF_MEAT, BOTTYPE_KAZE};

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
pub const BOTDISTCFG_KAZE: usize = 4;
pub const BOTDISTCFG_FOLLOW: usize = 6;

impl World {
    /// `botcmd_tick_dist_mode` (`botcmd.c:61`): how near to keep to the chr
    /// it attacks, or follows (`BOTDISTCFG_FOLLOW`; a close-range fighter
    /// takes on a target within 5 m of its leader).
    pub(crate) fn botcmd_tick_dist_mode(&mut self, i: usize) {
        let gset = self.res.gset.clone();
        let a = self.ab(i);
        let confignum = if a.config.bottype == BOTTYPE_KAZE { BOTDISTCFG_KAZE } else { botinv_get_dist_config(&gset, a.weaponnum, a.gunfunc) };
        let (limits, targetprop, insight) = match (a.myaction, a.followingplayernum, a.attackingplayernum) {
            (MyAction::Follow, Some(f), _) => {
                let (mut limits, mut t, mut insight) = (BOT_DIST_CONFIGS[BOTDISTCFG_FOLLOW], f, a.chrsinsight[f]);
                if let Some(target) = self.chrs[i].target {
                    if (confignum == BOTDISTCFG_CLOSE || confignum == BOTDISTCFG_KAZE) && self.chrs[f].pos.distance_squared(self.chrs[target].pos) < 500.0 * 500.0 {
                        limits = BOT_DIST_CONFIGS[confignum];
                        t = target;
                        insight = a.targetinsight;
                    }
                }
                (limits, Some(t), insight)
            }
            (MyAction::Attack, _, Some(p)) => (BOT_DIST_CONFIGS[confignum.min(7)], Some(p), a.chrsinsight[p]),
            _ => (BOT_DIST_CONFIGS[confignum.min(7)], self.chrs[i].target, a.targetinsight),
        };
        let prevmode = a.distmode;
        let Some(targetprop) = targetprop else { return };
        // bot_has_ground.
        if self.chrs[targetprop].ground < -20000.0 {
            return;
        }
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

/// `bot_get_command_name` (`bot.c:1219`): an order's name in the active menu,
/// by `AIBOTCMD_*` (anything else is "Normal").
pub fn bot_get_command_name(command: u8) -> pd_core::lang::Tx {
    use pd_core::lang::{tx, LANGBANK_MISC};
    const NAMES: [u16; 14] = [
        175, // "Follow"
        176, // "Attack"
        177, // "Defend"
        178, // "Hold"
        179, // "Normal"
        180, // "Download"
        181, // "Get Case"
        182, // "Tag Box"
        209, // "Save Case"
        210, // "Def Hill"
        211, // "Hold Hill"
        212, // "Get Case"
        213, // "Pop Cap"
        214, // "Protect"
    ];
    tx(LANGBANK_MISC, NAMES.get(command as usize).copied().unwrap_or(179))
}
