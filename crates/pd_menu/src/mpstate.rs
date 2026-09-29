//! `mplayer.c`, `challenge.c` and `challengeinit.c`: the multiplayer setup the
//! menus edit (`g_MpSetup`, `g_PlayerConfigsArray`, `g_BotConfigsArray`,
//! `g_BossFile`, the lock) and the unlock system that decides what the menus
//! offer (`g_MpFeaturesUnlocked`, challenge availability).
//!
//! Presets and challenges are data in the ROM (`mpconfigs.bin` +
//! `mpstringsE.bin`, `challenge_load_config`, challenge.c:420), loaded as-is.
//!
//! **Substitution:** PD derives some unlocks from the solo game file (weapons
//! found in missions, `fr_is_weapon_available_for_mp`; soundtracks from stage
//! best times, `mp_is_track_unlocked`) and the rest from completed challenges.
//! There is no game file here: [`Profile`] says which state to pretend —
//! a fresh file, or one where every challenge is done and every weapon found.

use super::generated::{self as gd, *};
use pd_core::lang::Tx;
use super::MenuSystem;

/// `struct mpchrconfig` (types.h:4012).
#[derive(Clone, Debug, Default)]
pub struct MpChrConfig {
    pub name: String,
    pub mpheadnum: u8,
    pub mpbodynum: u8,
    pub team: u8,
    pub displayoptions: u32,
    pub unk18: u16,
    pub unk1a: u16,
    pub unk1c: u16,
    /// The match's counters (`killcounts`, `numdeaths`, `numpoints`,
    /// `placement`, `rankablescore`), as the match left them.
    pub stats: pd_core::mp::MpChrStats,
}

/// `struct mpplayerconfig` (types.h:4029).
#[derive(Clone, Debug, Default)]
pub struct MpPlayerConfig {
    pub base: MpChrConfig,
    pub controlmode: u8,
    pub options: u32,
    pub fileid: u32,
    /// `kills` .. `survivormedals`: the player file's statistics.
    pub career: pd_core::mp::MpCareer,
    /// `MEDAL_*` won in the last match (`mp_calculate_awards`).
    pub medals: u8,
    pub title: u8,
    pub newtitle: u8,
    pub handicap: u16,
    /// `g_MpSetup`'s aim control / options use these (options.c).
    pub aimcontrol: u8,
}

#[derive(Clone, Debug, Default)]
pub struct MpBotConfig {
    pub base: MpChrConfig,
    pub ty: u8,
    pub difficulty: u8,
}

/// `struct mpsetup` (types.h:4039).
#[derive(Clone, Debug, Default)]
pub struct MpSetup {
    pub name: String,
    pub options: u32,
    pub scenario: u8,
    pub stagenum: u8,
    pub timelimit: u8,
    pub scorelimit: u8,
    pub teamscorelimit: u16,
    pub chrslots: u16,
    pub weapons: [u8; 6],
    pub fileid: u32,
}

/// `struct bossfile` (types.h:4061).
#[derive(Clone, Debug, Default)]
pub struct BossFile {
    pub teamnames: [String; 8],
    pub locktype: u8,
    pub tracknum: i8,
    pub multipletracknums: [u8; 8],
    pub usingmultipletunes: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MpLockInfo {
    pub lockedplayernum: i32,
    pub lastwinner: i32,
    pub lastloser: i32,
}

/// The runtime half of `struct challenge` (types.h:4105).
#[derive(Clone, Copy, Debug, Default)]
pub struct Challenge {
    pub availability: u8,
    pub completions: [u8; 4],
    pub unlockfeatures: [u8; 16],
}

/// One ROM `struct mpconfigfull` (types.h:4950).
#[derive(Clone, Debug, Default)]
pub struct MpConfig {
    pub setup: MpSetup,
    pub sims: [MpConfigSim; 8],
    pub description: String,
    pub aibotnames: [String; 8],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MpConfigSim {
    pub ty: u8,
    pub mpheadnum: u8,
    pub mpbodynum: u8,
    pub team: u8,
    pub difficulties: [u8; 4],
}

/// Which save file the menus pretend is loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// A new file: challenges 1-4 offered, the base roster only.
    Fresh,
    /// Every challenge completed (by player 1, at every player count) and
    /// every weapon found: the whole roster, all arenas and scenarios.
    Complete,
}

/// `mpconfigs.bin` + `mpstringsE.bin` (104 + 320 bytes per config, big-endian).
pub fn load_mpconfigs(assets: &pd_core::assets::AssetDir) -> Result<Vec<MpConfig>, String> {
    let cfg = assets.read(&assets.data("mpconfigs.bin"))?;
    let strs = assets.read(&assets.mpstrings())?;
    let cstr = |b: &[u8]| -> String {
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        b[..end].iter().map(|&c| c as char).collect()
    };
    let n = cfg.len() / 104;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let c = &cfg[i * 104..(i + 1) * 104];
        let be16 = |o: usize| u16::from_be_bytes([c[o], c[o + 1]]);
        let be32 = |o: usize| u32::from_be_bytes([c[o], c[o + 1], c[o + 2], c[o + 3]]);
        let setup = MpSetup {
            name: cstr(&c[0..12]),
            options: be32(12),
            scenario: c[16],
            stagenum: c[17],
            timelimit: c[18],
            scorelimit: c[19],
            teamscorelimit: be16(20),
            chrslots: be16(22),
            weapons: [c[24], c[25], c[26], c[27], c[28], c[29]],
            fileid: 0,
        };
        let mut sims = [MpConfigSim::default(); 8];
        for (j, s) in sims.iter_mut().enumerate() {
            let o = 40 + j * 8;
            *s = MpConfigSim { ty: c[o], mpheadnum: c[o + 1], mpbodynum: c[o + 2], team: c[o + 3], difficulties: [c[o + 4], c[o + 5], c[o + 6], c[o + 7]] };
        }
        let (description, aibotnames) = match strs.get(i * 320..(i + 1) * 320) {
            Some(s) => (cstr(&s[0..200]), std::array::from_fn(|j| cstr(&s[200 + j * 15..200 + (j + 1) * 15]))),
            None => (String::new(), Default::default()),
        };
        out.push(MpConfig { setup, sims, description, aibotnames });
    }
    Ok(out)
}

pub struct MpState {
    pub setup: MpSetup,
    /// `g_PlayerConfigsArray[MAX_MPPLAYERCONFIGS]` (6: 4 players + 2 for co-op swaps).
    pub players: [MpPlayerConfig; 6],
    pub bots: [MpBotConfig; 8],
    pub simdiffs: [[u8; 4]; 8],
    pub bossfile: BossFile,
    pub lockinfo: MpLockInfo,
    pub weaponsetnum: i32,
    pub features_unlocked: [u8; 80],
    pub features_force: [u8; 40],
    pub challenges: [Challenge; 30],
    pub preset_requirefeatures: Vec<[u8; 16]>,
    pub challenge_index: usize,
    /// `g_MpCurrentChallengeConfig`: the confignum loaded for the description.
    pub current_challenge: Option<usize>,
    pub profile: Profile,
}

impl Default for MpState {
    fn default() -> Self {
        MpState {
            setup: MpSetup::default(),
            players: Default::default(),
            bots: Default::default(),
            simdiffs: [[0; 4]; 8],
            bossfile: BossFile::default(),
            lockinfo: MpLockInfo::default(),
            weaponsetnum: 0,
            features_unlocked: [0; 80],
            features_force: [0; 40],
            challenges: [Challenge::default(); 30],
            preset_requirefeatures: vec![[0; 16]; MP_PRESETS.len()],
            challenge_index: 0,
            current_challenge: None,
            profile: Profile::Complete,
        }
    }
}

impl MenuSystem {
    pub fn lang(&self, t: Tx) -> String {
        self.draw.res.lang.get(t).to_string()
    }

    /// `MPCHR(i)`: players 0-3 then simulants.
    pub fn mpchr(&self, i: usize) -> Option<MpChrConfig> {
        if i < 4 {
            self.mp.players.get(i).map(|p| p.base.clone())
        } else {
            self.mp.bots.get(i - 4).map(|b| b.base.clone())
        }
    }
    pub fn mpchr_mut(&mut self, i: usize) -> Option<&mut MpChrConfig> {
        if i < 4 {
            self.mp.players.get_mut(i).map(|p| &mut p.base)
        } else {
            self.mp.bots.get_mut(i - 4).map(|b| &mut b.base)
        }
    }

    // ---- challenge.c ----

    pub fn challenge_is_feature_unlocked(&self, featurenum: i32) -> bool {
        featurenum == 0 || self.mp.features_unlocked.get(featurenum as usize).map(|f| f & 1 != 0).unwrap_or(false)
    }

    fn challenge_is_available_to_player(&self, chrnum: usize, ci: usize) -> bool {
        if self.mp.setup.chrslots & (1 << chrnum) == 0 {
            return false;
        }
        self.mp.challenges[ci].availability & (2 << chrnum) != 0
    }

    pub fn challenge_is_available_to_any_player(&self, ci: usize) -> bool {
        (self.mp.challenges[ci].availability as u32 & (((self.mp.setup.chrslots as u32 & 0xf) << 1) | 1)) != 0
    }

    pub fn challenge_is_completed_by_any_player_with_num_players(&self, index: usize, numplayers: usize) -> bool {
        self.mp.challenges[index].completions[numplayers - 1] & 1 != 0
    }

    pub fn challenge_is_completed_by_player_with_num_players(&self, mpchrnum: usize, index: usize, numplayers: usize) -> bool {
        self.mp.challenges[index].completions[numplayers - 1] & (2 << mpchrnum) != 0
    }

    fn any_complete(&self, ci: usize) -> bool {
        (1..=4).any(|n| self.challenge_is_completed_by_any_player_with_num_players(ci, n))
    }

    fn player_complete(&self, p: usize, ci: usize) -> bool {
        (1..=4).any(|n| self.challenge_is_completed_by_player_with_num_players(p, ci, n))
    }

    /// `challenge_determine_unlocked_features` (challenge.c:83).
    pub fn challenge_determine_unlocked_features(&mut self) {
        let n = self.mp.challenges.len();
        for c in self.mp.challenges.iter_mut() {
            c.availability = 0;
        }
        let mut numgifted = 0;
        for ci in 0..n {
            let mut flag = 0;
            if self.any_complete(ci) {
                flag = 1;
            } else if ci < 4 {
                flag = 1;
                numgifted += 1;
            } else if ci > 0 && self.any_complete(ci - 1) {
                flag = 1;
                numgifted += 1;
            }
            self.mp.challenges[ci].availability |= flag;
        }
        let mut ci = 0;
        while numgifted < 4 && ci < n {
            if self.mp.challenges[ci].availability & 1 == 0 {
                self.mp.challenges[ci].availability |= 1;
                numgifted += 1;
            }
            ci += 1;
        }
        for j in 0..4 {
            let mut numgifted = 0;
            for ci in 0..n {
                let mut flag = 0u8;
                if self.player_complete(j, ci) {
                    flag = 2 << j;
                } else if ci < 4 {
                    flag = 2 << j;
                    numgifted += 1;
                } else if ci > 0 && self.player_complete(j, ci - 1) {
                    flag = 2 << j;
                    numgifted += 1;
                }
                self.mp.challenges[ci].availability |= flag;
            }
            let mut ci = 0;
            while numgifted < 4 && ci < n {
                if self.mp.challenges[ci].availability & (2 << j) == 0 {
                    self.mp.challenges[ci].availability |= 2 << j;
                    numgifted += 1;
                }
                ci += 1;
            }
        }
        for j in 0..80usize {
            let mut flag = 0u8;
            for ci in 0..n {
                if self.challenge_is_available_to_any_player(ci) && self.mp.challenges[ci].unlockfeatures.iter().any(|&f| f as usize == j) {
                    flag |= 1;
                }
            }
            if self.mp.features_force.iter().any(|&f| f as usize == j) {
                flag |= 1;
            }
            for ci in 0..n {
                for p in 0..4 {
                    if self.challenge_is_available_to_player(p, ci) && self.mp.challenges[ci].unlockfeatures.iter().any(|&f| f as usize == j) {
                        flag |= 2 << p;
                    }
                }
            }
            self.mp.features_unlocked[j] = flag;
        }
        for w in MP_WEAPONS.iter() {
            if w.unlockfeature > 0 && self.fr_is_weapon_available_for_mp(w.weaponnum) {
                self.mp.features_unlocked[w.unlockfeature as usize] |= 1;
            }
        }
        self.mp_apply_weaponset_if_standard();
        if !self.challenge_is_feature_unlocked(MPFEATURE_8BOTS) {
            for k in 4..8 {
                if self.mp.setup.chrslots & (1 << (4 + k)) != 0 {
                    self.mp_remove_simulant(k);
                }
            }
            if self.vars.mpquickteamnumsims > 4 {
                self.vars.mpquickteamnumsims = 4;
            }
        }
    }

    /// `fr_is_weapon_available_for_mp` (training.c:226) — see [`Profile`].
    fn fr_is_weapon_available_for_mp(&self, weapon: i32) -> bool {
        weapon > 0 && self.mp.profile == Profile::Complete
    }

    /// `challenge_perform_sanity_checks` (challenge.c:258).
    pub fn challenge_perform_sanity_checks(&mut self) {
        if self.mp.bossfile.locktype == MPLOCKTYPE_CHALLENGE as u8 {
            let mut numplayers = 0;
            for i in 0..4 {
                if self.mp.setup.chrslots & (1 << i) != 0 {
                    self.mp.players[i].handicap = 0x80;
                    numplayers += 1;
                }
            }
            self.mp.setup.chrslots &= 0x000f;
            let np = (numplayers as usize).max(1);
            for i in 0..8 {
                self.mp.bots[i].difficulty = self.mp.simdiffs[i][np - 1];
                if self.mp.bots[i].difficulty as i32 != BOTDIFF_DISABLED {
                    self.mp.setup.chrslots |= 1 << (i + 4);
                }
            }
            if self.mp.setup.scenario as i32 == MPSCENARIO_KINGOFTHEHILL {
                self.vars.mphilltime = 10;
            }
        } else if !self.challenge_is_feature_unlocked(MPFEATURE_8BOTS) {
            self.mp.setup.chrslots &= 0x00ff;
        }
    }

    pub fn challenge_get_num_available(&self) -> i32 {
        (0..self.mp.challenges.len()).filter(|&c| self.challenge_is_available_to_any_player(c)).count() as i32
    }

    pub fn challenge_get_name(&self, ci: usize) -> String {
        self.lang(MP_CHALLENGES[ci].name)
    }

    fn challenge_slot_to_index(&self, slot: i32) -> Option<usize> {
        (0..self.mp.challenges.len()).filter(|&c| self.challenge_is_available_to_any_player(c)).nth(slot.max(0) as usize)
    }

    pub fn challenge_get_name_by_slot(&self, slot: i32) -> String {
        self.challenge_slot_to_index(slot).map(|c| self.challenge_get_name(c)).unwrap_or_default()
    }

    /// `challenge_set_current_by_slot` (challenge.c:344).
    pub fn challenge_set_current_by_slot(&mut self, slot: i32) {
        self.mp.challenge_index = self.challenge_slot_to_index(slot).unwrap_or(0);
        self.challenge_apply();
    }

    pub fn challenge_is_completed_by_any_chr_with_num_players_by_slot(&self, slot: i32, numplayers: usize) -> bool {
        self.challenge_slot_to_index(slot).map(|c| self.challenge_is_completed_by_any_player_with_num_players(c, numplayers)).unwrap_or(false)
    }

    /// `challenge_load_by_slot` (challenge.c:452): the confignum.
    pub fn challenge_config_by_slot(&self, slot: i32) -> Option<usize> {
        self.challenge_slot_to_index(slot).map(|c| MP_CHALLENGES[c].confignum as usize)
    }

    /// `challenge_force_unlock_feature` (challenge.c:474).
    fn force_unlock(featurenum: i32, array: &mut [u8], tail: usize) -> usize {
        if array[..tail].iter().any(|&f| f as i32 == featurenum) {
            return tail;
        }
        if tail < array.len() {
            array[tail] = featurenum as u8;
            tail + 1
        } else {
            tail
        }
    }

    /// `challenge_force_unlock_setup_features` (challenge.c:492).
    fn challenge_force_unlock_setup_features(setup: &MpSetup, array: &mut [u8]) -> usize {
        let mut index = 0;
        for &w in setup.weapons.iter() {
            let f = MP_WEAPONS.get(w as usize).map(|m| m.unlockfeature).unwrap_or(0);
            if f != 0 {
                index = Self::force_unlock(f, array, index);
            }
        }
        for a in MP_ARENAS.iter() {
            if a.stagenum == setup.stagenum as i32 && a.requirefeature != 0 {
                index = Self::force_unlock(a.requirefeature, array, index);
            }
        }
        if (setup.scenario as i32) <= MPSCENARIO_CAPTURETHECASE {
            let f = MP_SCENARIO_OVERVIEWS[setup.scenario as usize].requirefeature;
            if f != 0 {
                index = Self::force_unlock(f, array, index);
            }
        }
        if setup.options & MPOPTION_ONEHITKILLS as u32 != 0 {
            index = Self::force_unlock(MPFEATURE_ONEHITKILLS, array, index);
        }
        if setup.options & (MPOPTION_SLOWMOTION_ON | MPOPTION_SLOWMOTION_SMART) as u32 != 0 {
            index = Self::force_unlock(MPFEATURE_SLOWMOTION, array, index);
        }
        index
    }

    /// `mp_find_bot_profile` (mplayer.c:3166).
    pub fn mp_find_bot_profile(ty: i32, difficulty: i32) -> i32 {
        let pos = if ty == BOTTYPE_GENERAL { BOT_PROFILES.iter().position(|p| p.difficulty == difficulty) } else { BOT_PROFILES.iter().position(|p| p.ty == ty) };
        pos.map(|p| p as i32).unwrap_or(-1)
    }

    /// `challenge_force_unlock_config_features` (challenge.c:540).
    fn challenge_force_unlock_config_features(config: &MpConfig, array: &mut [u8], challengeindex: i32) {
        let mut index = Self::challenge_force_unlock_setup_features(&config.setup, array);
        for s in config.sims.iter() {
            let simtype = Self::mp_find_bot_profile(s.ty as i32, BOTDIFF_NORMAL);
            if simtype >= 0 {
                let f = BOT_PROFILES[simtype as usize].requirefeature;
                if f != 0 {
                    index = Self::force_unlock(f, array, index);
                }
            }
            for np in 0..4 {
                let simtype = Self::mp_find_bot_profile(0, s.difficulties[np] as i32);
                if simtype >= 0 {
                    let f = BOT_PROFILES[simtype as usize].requirefeature;
                    if f != 0 {
                        index = Self::force_unlock(f, array, index);
                    }
                }
            }
            if let Some(b) = MP_BODIES.get(s.mpbodynum as usize) {
                if b.requirefeature != 0 {
                    index = Self::force_unlock(b.requirefeature, array, index);
                }
            }
            if let Some(h) = MP_HEADS.get(s.mpheadnum as usize) {
                if h.requirefeature != 0 {
                    index = Self::force_unlock(h.requirefeature, array, index);
                }
            }
        }
        if challengeindex >= 25 {
            index = Self::force_unlock(MPFEATURE_BOTDIFF_DARK, array, index);
        } else if challengeindex >= 20 {
            index = Self::force_unlock(MPFEATURE_STAGE_CARPARK, array, index);
        } else if challengeindex >= 15 {
            index = Self::force_unlock(MPFEATURE_SCENARIO_PAC, array, index);
        }
        if challengeindex >= 10 {
            index = Self::force_unlock(MPFEATURE_8BOTS, array, index);
        }
        for a in array.iter_mut().skip(index) {
            *a = 0;
        }
    }

    /// `challenges_init` (challengeinit.c:9) + the profile's completions.
    pub fn challenges_init(&mut self) {
        for i in 0..self.mp.challenges.len() {
            let cfg = self.draw.res.mpconfigs[MP_CHALLENGES[i].confignum as usize].clone();
            let c = &mut self.mp.challenges[i];
            c.availability = 0;
            c.completions = [0; 4];
            if self.mp.profile == Profile::Complete {
                // Completed by "any player" and by player 1, with 1-4 players.
                c.completions = [0b11; 4];
            }
            let mut arr = [0u8; 16];
            Self::challenge_force_unlock_config_features(&cfg, &mut arr, i as i32);
            self.mp.challenges[i].unlockfeatures = arr;
        }
        for i in 0..MP_PRESETS.len() {
            let cfg = self.draw.res.mpconfigs[MP_PRESETS[i].confignum as usize].clone();
            let mut arr = [0u8; 16];
            Self::challenge_force_unlock_config_features(&cfg, &mut arr, -1);
            self.mp.preset_requirefeatures[i] = arr;
        }
        self.challenge_determine_unlocked_features();
    }

    /// `challenge_force_unlock_bot_features` (challenge.c:611).
    pub fn challenge_force_unlock_bot_features(&mut self) {
        let mut arr = [0u8; 40];
        let mut index = Self::challenge_force_unlock_setup_features(&self.mp.setup.clone(), &mut arr);
        let mut numsims = 0;
        for i in 0..8 {
            let b = self.mp.bots[i].clone();
            let t = Self::mp_find_bot_profile(b.ty as i32, BOTDIFF_NORMAL);
            if t >= 0 && BOT_PROFILES[t as usize].requirefeature != 0 {
                index = Self::force_unlock(BOT_PROFILES[t as usize].requirefeature, &mut arr, index);
            }
            let t = Self::mp_find_bot_profile(BOTTYPE_GENERAL, b.difficulty as i32);
            if t >= 0 && BOT_PROFILES[t as usize].requirefeature != 0 {
                index = Self::force_unlock(BOT_PROFILES[t as usize].requirefeature, &mut arr, index);
            }
            if t >= 0 {
                numsims += 1;
            }
            if let Some(body) = MP_BODIES.get(b.base.mpbodynum as usize) {
                if body.requirefeature != 0 {
                    index = Self::force_unlock(body.requirefeature, &mut arr, index);
                }
            }
            if let Some(h) = MP_HEADS.get(b.base.mpheadnum as usize) {
                if h.requirefeature != 0 {
                    index = Self::force_unlock(h.requirefeature, &mut arr, index);
                }
            }
        }
        if numsims > 4 {
            index = Self::force_unlock(MPFEATURE_8BOTS, &mut arr, index);
        }
        for a in arr.iter_mut().skip(index) {
            *a = 0;
        }
        self.mp.features_force = arr;
        self.challenge_determine_unlocked_features();
    }

    pub fn challenge_remove_force_unlocks(&mut self) {
        self.mp.features_force = [0; 40];
        self.challenge_determine_unlocked_features();
    }

    /// `challenge_apply` (challenge.c:684).
    fn challenge_apply(&mut self) {
        let cfg = MP_CHALLENGES[self.mp.challenge_index].confignum as usize;
        self.mp_apply_config(cfg);
        self.mp_set_lock(MPLOCKTYPE_CHALLENGE, 5);
        for i in 0..4 {
            self.mp.players[i].base.team = 0;
        }
    }

    pub fn challenge_remove_player_lock(&mut self) {
        self.mp_set_lock(MPLOCKTYPE_NONE, 0);
    }
    pub fn challenge_load_and_store_current(&mut self) {
        self.mp.current_challenge = Some(MP_CHALLENGES[self.mp.challenge_index].confignum as usize);
    }
    pub fn challenge_unset_current(&mut self) {
        self.mp.current_challenge = None;
    }
    pub fn challenge_is_loaded(&self) -> bool {
        self.mp.current_challenge.is_some()
    }
    pub fn challenge_get_current_description(&self) -> String {
        self.mp.current_challenge.map(|c| self.draw.res.mpconfigs[c].description.clone()).unwrap_or_default()
    }

    /// `challenge_get_auto_focused_index` (challenge.c:757).
    pub fn challenge_get_auto_focused_index(&self, mpchrnum: usize) -> i32 {
        let mut index = 0;
        for ci in (0..self.mp.challenges.len()).rev() {
            if self.player_complete(mpchrnum, ci) {
                index = ci as i32 + 1;
                break;
            }
        }
        index.max(4)
    }

    // ---- mplayer.c ----

    /// `mp_handicap_to_value` (mplayer.c:111).
    pub fn mp_handicap_to_value(handicap: u8) -> f32 {
        if handicap < 127 {
            return (handicap as f32 / 127.0) * (handicap as f32 / 127.0) * 0.9 + 0.1;
        }
        if handicap == 127 {
            return 1.0;
        }
        let tmp = (handicap as f32 - 128.0) / 127.0 + 1.0;
        tmp * tmp * 3.0 - 2.0
    }

    pub fn mp_init_handicaps(&mut self, p: usize) {
        let b = &mut self.mp.players[p].base;
        b.unk18 = 80;
        b.unk1a = 80;
        b.unk1c = 75;
    }

    pub fn mp_init_limits(&mut self) {
        self.mp.setup.timelimit = 9;
        self.mp.setup.scorelimit = 9;
        self.mp.setup.teamscorelimit = 19;
    }

    /// `mp_player_set_defaults` (mplayer.c:370).
    pub fn mp_player_set_defaults(&mut self, p: usize, autonames: bool) {
        self.mp_init_handicaps(p);
        let body = match p {
            1 => MPBODY_CASSANDRA,
            2 => MPBODY_CARRINGTON,
            3 => MPBODY_CILABTECH,
            _ => MPBODY_DARK_COMBAT,
        };
        let head = self.mp_get_mpheadnum_by_mpbodynum(body);
        let name = if autonames { format!("{} {}\n", self.lang(pd_core::lang::tx(B_MISC, 437)), p + 1) } else { String::new() };
        let pl = &mut self.mp.players[p];
        pl.controlmode = CONTROLMODE_11 as u8;
        pl.options = (OPTION_LOOKAHEAD | OPTION_SIGHTONSCREEN | OPTION_AUTOAIM | OPTION_AMMOONSCREEN | OPTION_SHOWGUNFUNCTION | OPTION_HEADROLL | OPTION_0100 | OPTION_ALWAYSSHOWTARGET | OPTION_SHOWZOOMRANGE) as u32;
        pl.handicap = 128;
        pl.base.mpbodynum = body as u8;
        pl.base.mpheadnum = head as u8;
        pl.base.displayoptions = (MPDISPLAYOPTION_RADAR | MPDISPLAYOPTION_HIGHLIGHTTEAMS) as u32;
        pl.fileid = 0;
        pl.base.name = name;
        pl.career = pd_core::mp::MpCareer::default();
        pl.title = MPPLAYERTITLE_BEGINNER as u8;
    }

    pub fn mp_init_botconfig(&mut self, i: usize) {
        let b = &mut self.mp.bots[i];
        b.base.name.clear();
        b.base.mpheadnum = MPHEAD_DARK_COMBAT as u8;
        b.base.mpbodynum = MPBODY_DARK_COMBAT as u8;
        b.ty = BOTTYPE_GENERAL as u8;
        b.difficulty = BOTDIFF_DISABLED as u8;
    }

    /// `mp_init` (mplayer.c:461) + `bossfile_set_defaults` (bossfile.c:204) +
    /// `mp_set_default_names_if_empty` (mplayer.c:554).
    pub fn mp_init(&mut self) {
        self.mp.setup.scenario = MPSCENARIO_COMBAT as u8;
        self.mp.setup.stagenum = STAGE_MP_SKEDAR as u8;
        self.mp.setup.options = (MPOPTION_DISPLAYTEAM
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
            | MPOPTION_PAC_SHOWONRADAR) as u32;
        self.vars.mphilltime = 10;
        self.mp_init_limits();
        self.mp.setup.fileid = 0;
        self.mp.setup.name.clear();
        for i in 0..6 {
            self.mp_player_set_defaults(i, false);
        }
        for i in 0..8 {
            self.mp_init_botconfig(i);
        }
        self.mp_set_weaponset_slotnum(0);
        self.mp.lockinfo = MpLockInfo { lockedplayernum: 0, lastwinner: -1, lastloser: -1 };
        self.mp.setup.chrslots = 0;
        // bossfile_set_defaults
        self.mp.bossfile.teamnames = Default::default();
        self.mp.bossfile.tracknum = -1;
        self.mp_enable_all_multi_tracks();
        self.mp.bossfile.usingmultipletunes = false;
        self.mp.bossfile.locktype = MPLOCKTYPE_NONE as u8;
        // mp_set_default_names_if_empty
        if self.mp.setup.name.is_empty() {
            self.mp.setup.name = self.lang(pd_core::lang::tx(B_MISC, 438));
        }
        for i in 0..8 {
            if self.mp.bossfile.teamnames[i].is_empty() {
                self.mp.bossfile.teamnames[i] = self.lang(pd_core::lang::tx(B_OPTIONS, 8 + i as u16));
            }
        }
        for i in 0..4 {
            if self.mp.players[i].base.name.is_empty() {
                self.mp.players[i].base.name = format!("{} {}\n", self.lang(pd_core::lang::tx(B_MISC, 437)), i + 1);
            }
        }
        self.challenges_init();
        self.challenge_force_unlock_bot_features();
    }

    /// Each chr slot's `mpchrconfig.team`.
    pub fn mp_teams(&self) -> [u8; pd_core::mp::MAX_MPCHRS] {
        std::array::from_fn(|i| self.mpchr(i).map_or(0, |c| c.team))
    }

    fn mp_chr_stats(&self) -> [pd_core::mp::MpChrStats; pd_core::mp::MAX_MPCHRS] {
        std::array::from_fn(|i| self.mpchr(i).map_or_else(Default::default, |c| c.stats))
    }

    /// The scenario's own counters for the scores (the match's, through
    /// [`crate::MatchView`]), with the menus' scenario and options.
    fn mp_scenario_scores(&self) -> pd_core::mp::ScenarioScores {
        pd_core::mp::ScenarioScores { scenario: self.mp.setup.scenario, options: self.mp.setup.options, ..self.matchview.scenario }
    }

    /// `mp_get_team_rankings` (mplayer.c:810) over the mpchrconfigs.
    pub fn mp_get_team_rankings(&self) -> Vec<pd_core::mp::Ranking> {
        let (teams, stats) = (self.mp_teams(), self.mp_chr_stats());
        let teams_enabled = self.mp.setup.options & MPOPTION_TEAMSENABLED as u32 != 0;
        let scenario = self.mp_scenario_scores();
        pd_core::mp::MpScoring { chrslots: self.mp.setup.chrslots, teams_enabled, teams: &teams, stats: &stats, scenario: &scenario }.mp_get_team_rankings()
    }

    /// `mp_get_player_rankings` (mplayer.c:640) over the mpchrconfigs: it also
    /// writes their placements and the lock's last winner and loser.
    pub fn mp_get_player_rankings(&mut self) -> Vec<pd_core::mp::Ranking> {
        let (teams, mut stats) = (self.mp_teams(), self.mp_chr_stats());
        let teams_enabled = self.mp.setup.options & MPOPTION_TEAMSENABLED as u32 != 0;
        let scenario = self.mp_scenario_scores();
        let r = pd_core::mp::mp_get_player_rankings(self.mp.setup.chrslots, teams_enabled, &teams, &mut stats, &scenario);
        for (i, s) in stats.iter().enumerate() {
            if let Some(c) = self.mpchr_mut(i) {
                c.stats = *s;
            }
        }
        self.mp.lockinfo.lastwinner = r.lastwinner;
        self.mp.lockinfo.lastloser = r.lastloser;
        if self.mp.bossfile.locktype == MPLOCKTYPE_RANDOM as u8 {
            self.mp.lockinfo.lockedplayernum = self.mp_choose_random_lock_player();
        }
        r.rankings
    }

    /// `challenge_is_complete_for_endscreen` (challenge.c:784): no player
    /// aborted and team 0 (the players' side) came first.
    pub fn challenge_is_complete_for_endscreen(&self) -> bool {
        if self.matchview.players.iter().any(|p| p.aborted) {
            return false;
        }
        self.mp_get_team_rankings().first().is_some_and(|r| r.teamnum == 0)
    }

    /// `mp_calculate_team_score_limit` (mplayer.c:578).
    pub fn mp_calculate_team_score_limit(&self) -> i32 {
        let numchrs = (0..4).filter(|i| self.mp.setup.chrslots & (1 << i) != 0).count();
        pd_core::mp::mp_calculate_team_score_limit(self.mp.setup.teamscorelimit, self.mp.bossfile.locktype == MPLOCKTYPE_CHALLENGE as u8, self.mp.setup.scenario, numchrs)
    }

    // ---- weapons (mplayer.c:859-1165) ----

    pub fn mp_get_num_weapon_options(&self) -> i32 {
        MP_WEAPONS.iter().filter(|w| self.challenge_is_feature_unlocked(w.unlockfeature)).count() as i32
    }

    /// `mp_get_weapon_label` (mplayer.c:878).
    pub fn mp_get_weapon_label(&self, mut weaponnum: i32) -> String {
        for w in MP_WEAPONS.iter() {
            if self.challenge_is_feature_unlocked(w.unlockfeature) {
                if weaponnum == 0 {
                    if w.weaponnum == WEAPON_NONE {
                        return self.lang(pd_core::lang::tx(B_MPWEAPONS, 58));
                    }
                    if w.weaponnum == WEAPON_MPSHIELD {
                        return self.lang(pd_core::lang::tx(B_MPWEAPONS, 59));
                    }
                    if w.weaponnum == WEAPON_DISABLED {
                        return self.lang(pd_core::lang::tx(B_MPWEAPONS, 60));
                    }
                    return WEAPON_NAMES.get(w.weaponnum as usize).map(|t| self.lang(*t)).unwrap_or_default();
                }
                weaponnum -= 1;
            }
        }
        String::new()
    }

    /// `mp_set_weapon_slot` (mplayer.c:912).
    pub fn mp_set_weapon_slot(&mut self, slot: usize, mpweaponnum: i32) {
        let mut m = mpweaponnum;
        let mut optionindex = mpweaponnum;
        let mut i = 0;
        while i <= m {
            if !self.challenge_is_feature_unlocked(MP_WEAPONS.get(i as usize).map(|w| w.unlockfeature).unwrap_or(0)) {
                m += 1;
            }
            optionindex = m;
            i += 1;
        }
        self.mp.setup.weapons[slot] = optionindex as u8;
    }

    /// `mp_get_weapon_slot` (mplayer.c:928).
    pub fn mp_get_weapon_slot(&self, slot: usize) -> i32 {
        (0..self.mp.setup.weapons[slot] as usize).filter(|&i| self.challenge_is_feature_unlocked(MP_WEAPONS[i].unlockfeature)).count() as i32
    }

    fn weaponset_fully_unlocked(&self, i: usize) -> bool {
        MP_WEAPON_SETS[i].requirefeatures.iter().all(|&f| self.challenge_is_feature_unlocked(f))
    }

    fn weaponset_available(&self, i: usize) -> bool {
        self.weaponset_fully_unlocked(i) || MP_WEAPON_SETS[i].slotsiflocked[0] != WEAPON_DISABLED
    }

    /// `mp_mpweaponset_to_slotnum` (mplayer.c:984).
    fn mp_mpweaponset_to_slotnum(&self, mut slotnum: i32) -> i32 {
        let n = MP_WEAPON_SETS.len() as i32;
        let mut count = 0;
        if slotnum >= n {
            count = slotnum - n;
            slotnum = n;
        }
        for i in 0..slotnum.max(0) as usize {
            if self.weaponset_available(i) {
                count += 1;
            }
        }
        count
    }

    /// `mp_slotnum_to_mpweaponset` (mplayer.c:1004).
    fn mp_slotnum_to_mpweaponset(&self, mut mpweaponset: i32) -> i32 {
        let mut i = 0;
        while i < MP_WEAPON_SETS.len() {
            if self.weaponset_available(i) {
                if mpweaponset == 0 {
                    break;
                }
                mpweaponset -= 1;
            }
            i += 1;
        }
        i as i32 + mpweaponset
    }

    pub fn mp_get_num_weaponset_slots(&self, full: bool) -> i32 {
        let n = MP_WEAPON_SETS.len() as i32;
        self.mp_mpweaponset_to_slotnum(if full { n + 3 } else { n })
    }

    pub fn mp_get_custom_weaponset_slot(&self) -> i32 {
        self.mp_mpweaponset_to_slotnum(MP_WEAPON_SETS.len() as i32 + 2)
    }

    /// `mp_get_weaponset_name_by_slotnum` (mplayer.c:1031).
    pub fn mp_get_weaponset_name_by_slotnum(&self, index: i32) -> String {
        let index = self.mp_slotnum_to_mpweaponset(index);
        let n = MP_WEAPON_SETS.len() as i32;
        if index < 0 || index >= n + 2 {
            return self.lang(pd_core::lang::tx(B_MPWEAPONS, 41));
        }
        if index == n + 1 {
            return self.lang(pd_core::lang::tx(B_MPWEAPONS, 42));
        }
        if index == n {
            return self.lang(pd_core::lang::tx(B_MPWEAPONS, 43));
        }
        self.lang(MP_WEAPON_SETS[index as usize].name)
    }

    /// `mp_find_weaponsetnum_by_weapons` (mplayer.c:1050).
    pub fn mp_find_weaponsetnum_by_weapons(&mut self) {
        for i in 0..MP_WEAPON_SETS.len() {
            let slots = if self.weaponset_fully_unlocked(i) {
                Some(MP_WEAPON_SETS[i].slots)
            } else if MP_WEAPON_SETS[i].slotsiflocked[0] != WEAPON_DISABLED {
                Some(MP_WEAPON_SETS[i].slotsiflocked)
            } else {
                None
            };
            if let Some(slots) = slots {
                let ok = (0..6).all(|j| {
                    let mut w = slots[j];
                    if w == WEAPON_MPSHIELD && !self.challenge_is_feature_unlocked(MPFEATURE_WEAPON_SHIELD) {
                        w = 0;
                    }
                    w == MP_WEAPONS[self.mp.setup.weapons[j] as usize].weaponnum
                });
                if ok {
                    self.mp.weaponsetnum = i as i32;
                    return;
                }
            }
        }
        self.mp.weaponsetnum = WEAPONSET_CUSTOM;
    }

    /// `mp_apply_weaponset` (mplayer.c:1095).
    fn mp_apply_weaponset(&mut self) {
        let n = MP_WEAPON_SETS.len() as i32;
        let ws = self.mp.weaponsetnum;
        if ws >= 0 && ws < n {
            let i = ws as usize;
            let slots = if self.weaponset_fully_unlocked(i) {
                Some(MP_WEAPON_SETS[i].slots)
            } else if MP_WEAPON_SETS[i].slotsiflocked[0] != WEAPON_DISABLED {
                Some(MP_WEAPON_SETS[i].slotsiflocked)
            } else {
                None
            };
            if let Some(slots) = slots {
                for s in 0..6 {
                    let mut weaponnum = slots[s];
                    if weaponnum == WEAPON_MPSHIELD && !self.challenge_is_feature_unlocked(MPFEATURE_WEAPON_SHIELD) {
                        weaponnum = 0;
                    }
                    let mp = MP_WEAPONS.iter().position(|w| w.weaponnum == weaponnum).unwrap_or(MPWEAPON_NONE as usize);
                    self.mp.setup.weapons[s] = mp as u8;
                }
            }
        } else if ws == WEAPONSET_RANDOM {
            let n = self.mp_get_num_weapon_options().max(1);
            for s in 0..6 {
                let r = (self.rng.random() % n as u32) as i32;
                self.mp_set_weapon_slot(s, r);
            }
        } else if ws == WEAPONSET_RANDOMFIVE {
            let n = (self.mp_get_num_weapon_options() - 2).max(1);
            for s in 0..5 {
                let r = (self.rng.random() % n as u32) as i32 + 1;
                self.mp_set_weapon_slot(s, r);
            }
            let last = self.mp_get_num_weapon_options() - 1;
            self.mp_set_weapon_slot(5, last);
        }
    }

    pub fn mp_set_weaponset_slotnum(&mut self, slotnum: i32) {
        self.mp.weaponsetnum = self.mp_slotnum_to_mpweaponset(slotnum);
        self.mp_apply_weaponset();
    }

    fn mp_apply_weaponset_if_standard(&mut self) {
        if self.mp.weaponsetnum < MP_WEAPON_SETS.len() as i32 {
            self.mp_apply_weaponset();
        }
    }

    pub fn mp_get_weaponset_slotnum(&self) -> i32 {
        self.mp_mpweaponset_to_slotnum(self.mp.weaponsetnum)
    }

    // ---- heads and bodies (mplayer.c:2459) ----

    pub fn mp_get_num_heads(&self) -> i32 {
        MP_HEADS.len() as i32
    }
    pub fn mp_get_head_id(&self, n: usize) -> i32 {
        MP_HEADS.get(n).map(|h| h.headnum).unwrap_or(0)
    }
    pub fn mp_get_num_bodies(&self) -> i32 {
        MP_BODIES.len() as i32
    }
    /// `mp_get_body_id` (mplayer.c:2494).
    pub fn mp_get_body_id(&self, bodynum: usize) -> i32 {
        if bodynum > MP_BODIES.len() {
            if bodynum == MP_BODIES.len() + 1 {
                return BODY_DRCAROLL;
            }
            return BODY_DARK_COMBAT;
        }
        MP_BODIES.get(bodynum).map(|b| b.bodynum).unwrap_or(BODY_DARK_COMBAT)
    }
    pub fn mp_get_body_name(&self, mpbodynum: usize) -> String {
        let n = if mpbodynum > MP_BODIES.len() { 0 } else { mpbodynum.min(MP_BODIES.len() - 1) };
        self.lang(MP_BODIES[n].name)
    }
    pub fn mp_get_body_required_feature(&self, mpbodynum: usize) -> i32 {
        let n = if mpbodynum > MP_BODIES.len() { 0 } else { mpbodynum.min(MP_BODIES.len() - 1) };
        MP_BODIES[n].requirefeature
    }

    /// `mp_get_mpheadnum_by_mpbodynum` (mplayer.c:2548).
    pub fn mp_get_mpheadnum_by_mpbodynum(&mut self, mpbodynum: i32) -> i32 {
        let mpbodynum = if mpbodynum >= HEAD_VD { 0 } else { mpbodynum } as usize;
        let mut headnum = MP_BODIES[mpbodynum].headnum;
        if headnum == 1000 {
            let male = HEADS_AND_BODIES.get(MP_BODIES[mpbodynum].bodynum as usize).map(|h| h.ismale).unwrap_or(true);
            headnum = if male {
                MP_MALE_HEADS[(self.rng.random() % MP_MALE_HEADS.len() as u32) as usize]
            } else {
                MP_FEMALE_HEADS[(self.rng.random() % MP_FEMALE_HEADS.len() as u32) as usize]
            };
        }
        MP_HEADS.iter().rposition(|h| h.headnum == headnum).map(|i| i as i32).unwrap_or(0)
    }

    // ---- lock (mplayer.c:2611) ----

    fn mp_choose_random_lock_player(&mut self) -> i32 {
        let start = (self.rng.random() % 4) as i32;
        let mut i = (start + 1) % 4;
        loop {
            if self.mp.setup.chrslots & (1 << i) != 0 || i == start {
                break;
            }
            i = (i + 1) % 4;
        }
        i
    }

    pub fn mp_set_lock(&mut self, locktype: i32, playernum: i32) {
        self.mp.bossfile.locktype = locktype as u8;
        if locktype == MPLOCKTYPE_RANDOM {
            self.mp.lockinfo.lockedplayernum = self.mp_choose_random_lock_player();
        } else {
            self.mp.lockinfo.lockedplayernum = playernum;
        }
    }

    pub fn mp_get_lock_type(&self) -> i32 {
        self.mp.bossfile.locktype as i32
    }

    pub fn mp_is_player_locked_out(&self, playernum: i32) -> bool {
        if self.mp.bossfile.locktype as i32 == MPLOCKTYPE_NONE {
            return false;
        }
        self.mp.lockinfo.lockedplayernum != playernum
    }

    /// `mp_calculate_lock_if_last_winner_or_loser` (mplayer.c:2661).
    pub fn mp_calculate_lock_if_last_winner_or_loser(&mut self) {
        let lt = self.mp.bossfile.locktype as i32;
        if lt == MPLOCKTYPE_LASTWINNER && self.mp.lockinfo.lastwinner >= 0 {
            self.mp.lockinfo.lockedplayernum = self.mp.lockinfo.lastwinner;
        }
        if lt == MPLOCKTYPE_LASTLOSER && self.mp.lockinfo.lastloser >= 0 {
            self.mp.lockinfo.lockedplayernum = self.mp.lockinfo.lastloser;
        }
        let l = self.mp.lockinfo.lockedplayernum;
        if l >= 0 && lt != MPLOCKTYPE_CHALLENGE && self.mp.setup.chrslots & (1 << l) == 0 {
            self.mp.lockinfo.lastwinner = -1;
            self.mp.lockinfo.lastloser = -1;
            self.mp.lockinfo.lockedplayernum = self.mp_choose_random_lock_player();
        }
    }

    // ---- tracks (mplayer.c:2679) ----

    /// `mp_is_track_unlocked` (mplayer.c:2725) — see [`Profile`].
    fn mp_is_track_unlocked(&self, tracknum: usize) -> bool {
        let stage = MP_TRACKS[tracknum].unlockstage;
        stage < 0 || stage > SOLOSTAGEINDEX_SKEDARRUINS || self.mp.profile == Profile::Complete
    }
    fn mp_get_track_slot_index(&self, tracknum: usize) -> i32 {
        (0..tracknum).filter(|&i| self.mp_is_track_unlocked(i)).count() as i32
    }
    pub fn mp_get_track_num_at_slot_index(&self, slotindex: i32) -> usize {
        (0..MP_TRACKS.len()).filter(|&i| self.mp_is_track_unlocked(i)).nth(slotindex.max(0) as usize).unwrap_or(MP_TRACKS.len() - 1)
    }
    pub fn mp_get_num_unlocked_tracks(&self) -> i32 {
        self.mp_get_track_slot_index(MP_TRACKS.len())
    }
    pub fn mp_get_track_name(&self, slotindex: i32) -> String {
        self.lang(MP_TRACKS[self.mp_get_track_num_at_slot_index(slotindex)].name)
    }
    pub fn mp_is_multi_track_slot_enabled(&self, slot: i32) -> bool {
        let t = self.mp_get_track_num_at_slot_index(slot);
        self.mp.bossfile.multipletracknums[t >> 3] & (1 << (t & 7)) != 0
    }
    fn mp_set_multi_track_slot_enabled(&mut self, slot: i32, enable: bool) {
        let t = self.mp_get_track_num_at_slot_index(slot);
        if enable {
            self.mp.bossfile.multipletracknums[t >> 3] |= 1 << (t & 7);
        } else {
            self.mp.bossfile.multipletracknums[t >> 3] &= !(1 << (t & 7));
        }
    }
    pub fn mp_set_track_slot_enabled(&mut self, slot: i32) {
        if self.mp.bossfile.usingmultipletunes {
            let e = self.mp_is_multi_track_slot_enabled(slot);
            self.mp_set_multi_track_slot_enabled(slot, !e);
        } else {
            self.mp.bossfile.tracknum = self.mp_get_track_num_at_slot_index(slot) as i8;
        }
    }
    pub fn mp_enable_all_multi_tracks(&mut self) {
        self.mp.bossfile.multipletracknums = [0xff; 8];
    }
    pub fn mp_disable_all_multi_tracks(&mut self) {
        self.mp.bossfile.multipletracknums = [0; 8];
    }
    pub fn mp_randomise_multi_tracks(&mut self) {
        for i in 0..8 {
            self.mp.bossfile.multipletracknums[i] = self.rng.random() as u8;
        }
    }
    pub fn mp_get_current_track_slot_num(&self) -> i32 {
        if self.mp.bossfile.tracknum < 0 {
            return self.mp.bossfile.tracknum as i32;
        }
        self.mp_get_track_slot_index(self.mp.bossfile.tracknum as usize)
    }

    // ---- chrs and simulants (mplayer.c:2958-3252) ----

    /// `mp_get_chr_config_by_slot_num` (mplayer.c:2958): the MPCHR index.
    pub fn mp_get_chr_index_by_slot(&self, slot: i32) -> Option<usize> {
        (0..12).filter(|&i| self.mp.setup.chrslots & (1 << i) != 0).nth(slot.max(0) as usize).filter(|_| slot >= 0)
    }

    pub fn mp_get_num_chrs(&self) -> i32 {
        (0..12).filter(|&i| self.mp.setup.chrslots & (1 << i) != 0).count() as i32
    }

    fn mp_find_unused_team_num(&self) -> u8 {
        let mut teamnum = 0u8;
        while teamnum < 7 {
            let used = (0..12).any(|i| self.mp.setup.chrslots & (1 << i) != 0 && self.mpchr(i).map(|c| c.team == teamnum).unwrap_or(false));
            if !used {
                break;
            }
            teamnum += 1;
        }
        teamnum.min(7)
    }

    /// `mp_create_bot_from_profile` (mplayer.c:3045).
    pub fn mp_create_bot_from_profile(&mut self, botnum: usize, profilenum: usize) {
        let team = self.mp_find_unused_team_num();
        let prof = BOT_PROFILES[profilenum];
        self.mp.bots[botnum].ty = prof.ty as u8;
        self.mp.bots[botnum].difficulty = prof.difficulty as u8;
        for i in 0..4 {
            self.mp.simdiffs[botnum][i] = prof.difficulty as u8;
        }
        self.mp.setup.chrslots |= 1 << (botnum + 4);
        self.mp.bots[botnum].base.name = "Sim\n".into();
        self.mp.bots[botnum].base.team = team;
        let mut headnum;
        let mut guard = 0;
        loop {
            headnum = BOT_HEADS[(self.rng.random() % BOT_HEADS.len() as u32) as usize];
            let taken = (0..12).any(|i| self.mp.setup.chrslots & (1 << i) != 0 && self.mpchr(i).map(|c| c.mpheadnum as i32 == headnum).unwrap_or(false));
            guard += 1;
            if !taken || guard > 1000 {
                break;
            }
        }
        self.mp.bots[botnum].base.mpheadnum = headnum as u8;
        self.mp.bots[botnum].base.mpbodynum = prof.body as u8;
    }

    pub fn mp_set_bot_difficulty(&mut self, botnum: usize, difficulty: i32) {
        self.mp.bots[botnum].difficulty = difficulty as u8;
        for i in 0..4 {
            self.mp.simdiffs[botnum][i] = difficulty as u8;
        }
    }

    pub fn mp_get_slot_for_new_bot(&self) -> usize {
        let mut i = 0;
        while i < 7 && self.mp.setup.chrslots & (1 << (i + 4)) != 0 {
            i += 1;
        }
        i
    }

    pub fn mp_remove_simulant(&mut self, index: usize) {
        self.mp.setup.chrslots &= !(1 << (index + 4));
        self.mp.bots[index].base.name.clear();
        self.mp_init_botconfig(index);
        self.mp_generate_bot_names();
    }

    pub fn mp_has_unused_bot_slots(&self) -> bool {
        let mut numvacant = if self.challenge_is_feature_unlocked(MPFEATURE_8BOTS) { 8 } else { 4 };
        for i in 4..12 {
            if self.mp.setup.chrslots & (1 << i) != 0 {
                numvacant -= 1;
            }
        }
        numvacant > 0
    }

    pub fn mp_is_sim_slot_enabled(&self, slot: usize) -> bool {
        if self.mp.setup.chrslots & (1 << (slot + 4)) == 0 {
            let used = (0..8).filter(|i| self.mp.setup.chrslots & (1 << (i + 4)) != 0).count();
            return 8 - used > 0;
        }
        true
    }

    /// `mp_generate_bot_names` (mplayer.c:3191).
    pub fn mp_generate_bot_names(&mut self) {
        let n = BOT_PROFILES.len();
        let mut counts = vec![0i32; n];
        for i in 4..12 {
            if self.mp.setup.chrslots & (1 << i) != 0 {
                let b = &self.mp.bots[i - 4];
                let p = Self::mp_find_bot_profile(b.ty as i32, b.difficulty as i32);
                if p >= 0 && (p as usize) < n {
                    counts[p as usize] += 1;
                }
            }
        }
        for c in counts.iter_mut() {
            *c = if *c <= 1 { -1 } else { 0 };
        }
        for i in 4..12 {
            if self.mp.setup.chrslots & (1 << i) != 0 {
                let b = self.mp.bots[i - 4].clone();
                let p = Self::mp_find_bot_profile(b.ty as i32, b.difficulty as i32);
                if p >= 0 && (p as usize) < n {
                    let pname = self.lang(BOT_PROFILES[p as usize].name);
                    let pname = pname.trim_end_matches('\n');
                    let name = if counts[p as usize] >= 0 {
                        counts[p as usize] += 1;
                        format!("{}:{}\n", pname, counts[p as usize])
                    } else {
                        format!("{}\n", pname)
                    };
                    self.mp.bots[i - 4].base.name = name;
                }
            }
        }
    }

    // ---- presets (mplayer.c:3625) ----

    fn mp_is_preset_unlocked(&self, presetnum: usize) -> bool {
        self.mp.preset_requirefeatures[presetnum].iter().all(|&f| self.challenge_is_feature_unlocked(f as i32) || f as i32 == MPFEATURE_WEAPON_SHIELD)
    }

    pub fn mp_get_num_unlocked_presets(&self) -> i32 {
        (0..MP_PRESETS.len()).filter(|&i| self.mp_is_preset_unlocked(i)).count() as i32
    }

    pub fn mp_get_preset_name_by_slot(&self, slot: i32) -> String {
        (0..MP_PRESETS.len()).filter(|&i| self.mp_is_preset_unlocked(i)).nth(slot.max(0) as usize).map(|i| self.lang(MP_PRESETS[i].name)).unwrap_or_default()
    }

    /// `scenario_init` (scenarios.c:449), the menus' half: King of the Hill
    /// turns teams on (`koh_init`, kingofthehill.inc:160); Capture the Case
    /// does too and folds every chr's team into its four (`ctc_init`,
    /// capturethecase.inc:120).
    ///
    /// `// SUBST:` PD's init callbacks also reset `g_ScenarioData` here (and
    /// `pac_init`'s `pac_reset` draws `random()` for a victim list) / the
    /// world starts its scenario data with the match (`pd_sim::mp::scenario`),
    /// where `pac_init_props` draws the victims again anyway.
    pub fn scenario_init(&mut self) {
        let s = self.mp.setup.scenario as i32;
        if s == MPSCENARIO_KINGOFTHEHILL || s == MPSCENARIO_CAPTURETHECASE {
            self.mp.setup.options |= MPOPTION_TEAMSENABLED as u32;
        }
        if s == MPSCENARIO_CAPTURETHECASE {
            let max = self.scenario_get_max_teams() as u8;
            for k in 0..pd_core::mp::MAX_MPCHRS {
                if self.mp.setup.chrslots & (1 << k) != 0 {
                    if let Some(c) = self.mpchr_mut(k) {
                        while c.team >= max {
                            c.team -= max;
                        }
                    }
                }
            }
        }
    }

    /// `mp_apply_config` (mplayer.c:3716).
    pub fn mp_apply_config(&mut self, confignum: usize) {
        let cfg = self.draw.res.mpconfigs[confignum].clone();
        self.mp.setup.scenario = cfg.setup.scenario;
        self.scenario_init();
        let chrslots = self.mp.setup.chrslots;
        self.mp.setup = MpSetup { chrslots: cfg.setup.chrslots, ..cfg.setup.clone() };
        let _ = chrslots;
        for i in 0..8 {
            let s = cfg.sims[i];
            self.mp.bots[i].ty = s.ty;
            self.mp.simdiffs[i] = s.difficulties;
            self.mp.bots[i].difficulty = self.mp.simdiffs[i][0];
            self.mp.bots[i].base.name = cfg.aibotnames[i].clone();
            self.mp.bots[i].base.mpheadnum = s.mpheadnum;
            self.mp.bots[i].base.mpbodynum = s.mpbodynum;
            self.mp.bots[i].base.team = s.team;
        }
        if !self.challenge_is_feature_unlocked(MPFEATURE_WEAPON_SHIELD) {
            for w in self.mp.setup.weapons.iter_mut() {
                if *w as i32 == MPWEAPON_SHIELD {
                    *w = MPWEAPON_NONE as u8;
                }
            }
        }
        self.mp_find_weaponsetnum_by_weapons();
        self.challenge_remove_force_unlocks();
    }

    /// `mp_load_preset_by_slotnum` (mplayer.c:3773).
    pub fn mp_load_preset_by_slotnum(&mut self, slot: i32) {
        let confignum = (0..MP_PRESETS.len()).filter(|&i| self.mp_is_preset_unlocked(i)).nth(slot.max(0) as usize).map(|i| MP_PRESETS[i].confignum as usize).unwrap_or(0);
        self.mp_apply_config(confignum);
    }

    /// `scenario_get_max_teams` (scenarios.c:908): CTC is 4, the rest 8.
    pub fn scenario_get_max_teams(&self) -> i32 {
        if self.mp.setup.scenario as i32 == MPSCENARIO_CAPTURETHECASE {
            4
        } else {
            gd::MAX_TEAMS
        }
    }
}
