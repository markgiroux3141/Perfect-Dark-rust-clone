//! PD's save files, as the menus fill and read them: the agent file
//! (`gamefile.c`: the name, the options, solo progress and which challenges
//! anyone has completed), the boss file (`bossfile.c`: the team names, the
//! soundtrack, the last agent), the Combat Simulator's player files
//! (`mpplayerfile_*`, `mplayer.c:3325`: a player's name, character, career
//! statistics and the challenges they completed) and setup files
//! (`mpsetupfile_*`, `mplayer.c:3806`), and `paks_init`'s boot.
//!
//! Each is packed bit by bit into a `SaveBuffer` and stored through
//! `pd_core::pak` on the Game Pak's EEPROM. The options the agent file keeps
//! (`options.c`, the sound volumes, the screen) are [`Options`].

use pd_core::ids::*;
use pd_core::music::MusicCall;
use pd_core::savebuffer::{cstr, cstr_to_string, pak_has_bitflag, pak_set_bitflag, FileGuid, SaveBuffer};

use super::generated::{BOT_PROFILES, B_MISC, B_OPTIONS, CONTROLMODE_11, MPLOCKTYPE_NONE};
use super::MenuSystem;

/// `NUM_SOLOSTAGES` (`constants.h:42`).
pub const NUM_SOLOSTAGES: usize = 21;

/// `struct gamefile` (`types.h:3951`): the agent file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameFile {
    pub name: [u8; 11],
    /// The stage whose picture the agent select shows (5 bits).
    pub thumbnail: u8,
    pub autodifficulty: u8,
    pub autostageindex: u8,
    pub totaltime: u32,
    /// `GAMEFILEFLAG_*` bits.
    pub flags: [u8; 10],
    pub unk1e: u16,
    pub besttimes: [[u16; 3]; NUM_SOLOSTAGES],
    pub coopcompletions: [u32; 3],
    pub firingrangescores: [u8; 9],
    pub weaponsfound: [u8; 6],
}

impl Default for GameFile {
    fn default() -> Self {
        GameFile {
            name: [0; 11],
            thumbnail: 0,
            autodifficulty: 0,
            autostageindex: 0,
            totaltime: 0,
            flags: [0; 10],
            unk1e: 0,
            besttimes: [[0; 3]; NUM_SOLOSTAGES],
            coopcompletions: [0; 3],
            firingrangescores: [0; 9],
            weaponsfound: [0; 6],
        }
    }
}

/// What `options.c`, `snd.c` and `g_Vars` hold that the agent file keeps
/// (the screen's ratio and split are the menus' [`super::Vars`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// `g_InGameSubtitles`, `g_CutsceneSubtitles`.
    pub ingamesubtitles: bool,
    pub cutscenesubtitles: bool,
    /// `g_ScreenSize` (`SCREENSIZE_*`).
    pub screensize: u8,
    /// `g_HiResEnabled` (and so `g_ViRes` from the next stage).
    pub hires: bool,
    /// `g_SfxVolume` and `g_MusicVolume`.
    pub sfxvolume: u16,
    pub musicvolume: u16,
    /// `g_SoundMode` (`SOUNDMODE_*`).
    pub soundmode: i32,
    /// `g_Vars.langfilteron`, `pendingantiplayernum`, `coopradaron`,
    /// `coopfriendlyfire`, `antiradaron`.
    pub langfilteron: bool,
    pub pendingantiplayernum: i32,
    pub coopradaron: bool,
    pub coopfriendlyfire: bool,
    pub antiradaron: bool,
}

impl Default for Options {
    /// `options.c`'s initialisers and `snd_init`'s.
    fn default() -> Self {
        Options {
            ingamesubtitles: true,
            cutscenesubtitles: false,
            screensize: SCREENSIZE_FULL,
            hires: false,
            sfxvolume: 0x5000,
            musicvolume: 0x5000,
            soundmode: SOUNDMODE_STEREO,
            langfilteron: false,
            pendingantiplayernum: 0,
            coopradaron: true,
            coopfriendlyfire: true,
            antiradaron: true,
        }
    }
}

/// `g_PlayerConfigsArray[]` index of the solo player 1 and 2 (outside co-op
/// and counter-op, `gamefile_apply_options`' `player1`/`player2`).
const P1: usize = 4;
const P2: usize = 5;

/// The Rust string of a C `name` array.
pub fn name_of(b: &[u8]) -> String {
    cstr_to_string(b)
}

impl MenuSystem {
    // ---- options.c ----

    fn option_bit(&self, p: usize, bit: u16) -> bool {
        self.mp.players[p].options & bit as u32 != 0
    }

    fn set_option_bit(&mut self, p: usize, bit: u16, enable: bool) {
        if enable {
            self.mp.players[p].options |= bit as u32;
        } else {
            self.mp.players[p].options &= !(bit as u32);
        }
    }

    /// `snd_set_sfx_volume` (`snd.c:879`).
    pub fn snd_set_sfx_volume(&mut self, volume: u16) {
        let volume = volume.min(0x5000);
        self.options.sfxvolume = volume;
        self.music(MusicCall::SetSfxVolume(volume));
    }

    /// `options_set_music_volume` (`options.c:325`): `music_set_volume`.
    pub fn options_set_music_volume(&mut self, volume: u16) {
        self.options.musicvolume = volume.min(0x5000);
        self.music(MusicCall::SetVolume(volume));
    }

    /// `options_get_music_volume` (`options.c:318`): `music_get_volume`.
    pub fn options_get_music_volume(&self) -> u16 {
        self.options.musicvolume.min(0x5000)
    }

    // ---- gamefile.c ----

    /// `gamefile_set_flag` (`gamefile.c:29`).
    pub fn gamefile_set_flag(&mut self, flag: u32) {
        pak_set_bitflag(flag, &mut self.gamefile.flags, true);
    }

    /// `gamefile_has_flag` (`gamefile.c:39`).
    pub fn gamefile_has_flag(&self, flag: u32) -> bool {
        pak_has_bitflag(flag, &self.gamefile.flags)
    }

    /// `gamefile_apply_options` (`gamefile.c:53`): the file's flags into the
    /// options (the solo players' are `g_PlayerConfigsArray[4]` and `[5]`).
    pub fn gamefile_apply_options(&mut self) {
        let f = self.gamefile.flags;
        let has = |flag: u32| pak_has_bitflag(flag, &f);
        for (p, flags) in [
            (P1, [GAMEFILEFLAG_P1_FORWARDPITCH, GAMEFILEFLAG_P1_AUTOAIM, GAMEFILEFLAG_P1_AIMCONTROL, GAMEFILEFLAG_P1_SIGHTONSCREEN, GAMEFILEFLAG_P1_LOOKAHEAD, GAMEFILEFLAG_P1_AMMOONSCREEN, GAMEFILEFLAG_P1_HEADROLL, GAMEFILEFLAG_P1_SHOWGUNFUNCTION, GAMEFILEFLAG_P1_ALWAYSSHOWTARGET, GAMEFILEFLAG_P1_SHOWZOOMRANGE, GAMEFILEFLAG_P1_SHOWMISSIONTIME, GAMEFILEFLAG_P1_PAINTBALL]),
            (P2, [GAMEFILEFLAG_P2_FORWARDPITCH, GAMEFILEFLAG_P2_AUTOAIM, GAMEFILEFLAG_P2_AIMCONTROL, GAMEFILEFLAG_P2_SIGHTONSCREEN, GAMEFILEFLAG_P2_LOOKAHEAD, GAMEFILEFLAG_P2_AMMOONSCREEN, GAMEFILEFLAG_P2_HEADROLL, GAMEFILEFLAG_P2_SHOWGUNFUNCTION, GAMEFILEFLAG_P2_ALWAYSSHOWTARGET, GAMEFILEFLAG_P2_SHOWZOOMRANGE, GAMEFILEFLAG_P2_SHOWMISSIONTIME, GAMEFILEFLAG_P2_PAINTBALL]),
        ] {
            for (flag, bit) in flags.iter().zip(OPTION_BITS) {
                self.set_option_bit(p, bit, has(*flag));
            }
        }
        self.options.ingamesubtitles = has(GAMEFILEFLAG_INGAMESUBTITLES);
        self.options.cutscenesubtitles = has(GAMEFILEFLAG_CUTSCENESUBTITLES);
        // (options_set_paintball(player2) a second time.)
        self.options.langfilteron = has(GAMEFILEFLAG_LANGFILTERON);
        // Not a 4 MB console: the hi-res flag stands.
        self.options.hires = has(GAMEFILEFLAG_HIRES);
        self.vars.screensplit = has(GAMEFILEFLAG_SCREENSPLIT) as u8;
        self.vars.screenratio = has(GAMEFILEFLAG_SCREENRATIO) as u8;
        self.options.screensize = if has(GAMEFILEFLAG_SCREENSIZE_CINEMA) {
            SCREENSIZE_CINEMA
        } else if has(GAMEFILEFLAG_SCREENSIZE_WIDE) {
            SCREENSIZE_WIDE
        } else {
            SCREENSIZE_FULL
        };
        self.options.pendingantiplayernum = has(GAMEFILEFLAG_ANTIPLAYERNUM) as i32;
        self.options.coopradaron = has(GAMEFILEFLAG_COOPRADARON);
        self.options.coopfriendlyfire = has(GAMEFILEFLAG_COOPFRIENDLYFIRE);
        self.options.antiradaron = has(GAMEFILEFLAG_ANTIRADARON);
    }

    /// `gamefile_load_defaults` (`gamefile.c:144`): a new agent, "Dark", with
    /// PD's default options, nothing done and no challenge completed by
    /// anyone.
    pub fn gamefile_load_defaults(&mut self) {
        let f = &mut self.gamefile;
        f.name = cstr::<11>("Dark");
        f.thumbnail = 0;
        f.autodifficulty = 0;
        f.autostageindex = 0;
        f.totaltime = 0;
        self.snd_set_sfx_volume(0x5000);
        self.options_set_music_volume(0x5000);
        self.options.soundmode = SOUNDMODE_STEREO;
        self.mp.players[P1].controlmode = CONTROLMODE_11 as u8;
        self.mp.players[P2].controlmode = CONTROLMODE_11 as u8;
        let flags = &mut self.gamefile.flags;
        // pak_clear_all_bitflags
        for i in 0..=GAMEFILEFLAG_4E {
            pak_set_bitflag(i, flags, false);
        }
        for (flag, on) in [
            (GAMEFILEFLAG_P1_FORWARDPITCH, false),
            (GAMEFILEFLAG_P1_AUTOAIM, true),
            (GAMEFILEFLAG_P1_AIMCONTROL, false),
            (GAMEFILEFLAG_P1_SIGHTONSCREEN, true),
            (GAMEFILEFLAG_P1_LOOKAHEAD, true),
            (GAMEFILEFLAG_P1_AMMOONSCREEN, true),
            (GAMEFILEFLAG_P1_HEADROLL, true),
            (GAMEFILEFLAG_P1_SHOWGUNFUNCTION, true),
            (GAMEFILEFLAG_INGAMESUBTITLES, true),
            (GAMEFILEFLAG_P1_ALWAYSSHOWTARGET, true),
            (GAMEFILEFLAG_P1_SHOWZOOMRANGE, true),
            (GAMEFILEFLAG_P1_SHOWMISSIONTIME, false),
            (GAMEFILEFLAG_P1_PAINTBALL, false),
            (GAMEFILEFLAG_P2_FORWARDPITCH, false),
            (GAMEFILEFLAG_P2_AUTOAIM, true),
            (GAMEFILEFLAG_P2_AIMCONTROL, false),
            (GAMEFILEFLAG_P2_SIGHTONSCREEN, true),
            (GAMEFILEFLAG_P2_LOOKAHEAD, true),
            (GAMEFILEFLAG_P2_AMMOONSCREEN, true),
            (GAMEFILEFLAG_P2_HEADROLL, true),
            (GAMEFILEFLAG_P2_SHOWGUNFUNCTION, true),
            (GAMEFILEFLAG_CUTSCENESUBTITLES, false),
            (GAMEFILEFLAG_P2_ALWAYSSHOWTARGET, true),
            (GAMEFILEFLAG_P2_SHOWZOOMRANGE, true),
            (GAMEFILEFLAG_P2_SHOWMISSIONTIME, false),
            (GAMEFILEFLAG_P2_PAINTBALL, false),
            (GAMEFILEFLAG_SCREENSPLIT, false),
            (GAMEFILEFLAG_SCREENRATIO, false),
            (GAMEFILEFLAG_SCREENSIZE_CINEMA, false),
            (GAMEFILEFLAG_SCREENSIZE_WIDE, false),
            (GAMEFILEFLAG_HIRES, false),
            (GAMEFILEFLAG_LANGFILTERON, false),
            (GAMEFILEFLAG_FOUNDTIMEDMINE, false),
            (GAMEFILEFLAG_FOUNDPROXYMINE, false),
            (GAMEFILEFLAG_FOUNDREMOTEMINE, false),
            (GAMEFILEFLAG_COOPRADARON, true),
            (GAMEFILEFLAG_COOPFRIENDLYFIRE, true),
            (GAMEFILEFLAG_ANTIRADARON, true),
            (GAMEFILEFLAG_ANTIPLAYERNUM, true),
        ] {
            pak_set_bitflag(flag, flags, on);
        }
        let f = &mut self.gamefile;
        f.unk1e = 0;
        f.besttimes = [[0; 3]; NUM_SOLOSTAGES];
        for i in 0..self.mp.challenges.len() {
            for j in 1..=4 {
                self.challenge_set_completed_by_any_player_with_num_players(i, j, false);
            }
        }
        self.challenge_determine_unlocked_features();
        self.gamefile.coopcompletions = [0; 3];
        self.gamefile.firingrangescores = [0; 9];
        self.gamefile.weaponsfound = [0; 6];
        self.gamefile_apply_options();
    }

    /// `gamefile_load` (`gamefile.c:269`): the agent `g_GameFileGuid` names,
    /// from `device`. 0 when it loaded.
    pub fn gamefile_load(&mut self, device: i8) -> i32 {
        if device < 0 {
            return -1;
        }
        let mut buffer = SaveBuffer::new();
        let ret = self.paks.pak_read_body_at_guid(device, self.gamefileguid.fileid, Some(&mut buffer.bytes), 0);
        self.filemgr.lastpakerror = ret;
        if ret != 0 {
            return -1;
        }
        // (cheats_init: there are no cheats.)
        buffer.read_string(&mut self.gamefile.name, false);
        self.gamefile.thumbnail = buffer.read_bits(5) as u8;
        self.gamefile.totaltime = buffer.read_bits(32);
        self.gamefile.autodifficulty = buffer.read_bits(2) as u8;
        self.gamefile.autostageindex = buffer.read_bits(5) as u8;
        let mut volume = buffer.read_bits(6) * 4;
        if volume >= 252 {
            volume = 255;
        }
        self.snd_set_sfx_volume(((volume & 0x1ff) * 128) as u16);
        let mut volume = buffer.read_bits(6) * 4;
        if volume >= 252 {
            volume = 255;
        }
        self.options_set_music_volume(((volume & 0x1ff) * 128) as u16);
        self.options.soundmode = buffer.read_bits(2) as i32;
        self.mp.players[P1].controlmode = buffer.read_bits(3) as u8;
        self.mp.players[P2].controlmode = buffer.read_bits(3) as u8;
        for i in 0..self.gamefile.flags.len() {
            self.gamefile.flags[i] = buffer.read_bits(8) as u8;
        }
        self.gamefile.unk1e = buffer.read_bits(16) as u16;
        for i in 0..NUM_SOLOSTAGES {
            for j in 0..3 {
                self.gamefile.besttimes[i][j] = buffer.read_bits(12) as u16;
            }
        }
        for i in 0..self.mp.challenges.len() {
            for j in 1..=4 {
                let done = buffer.read_bits(1) != 0;
                self.challenge_set_completed_by_any_player_with_num_players(i, j, done);
            }
        }
        self.challenge_determine_unlocked_features();
        for i in 0..3 {
            self.gamefile.coopcompletions[i] = buffer.read_bits(NUM_SOLOSTAGES as i32);
        }
        for i in 0..9 {
            let numbits = if i == 8 { 2 } else { 8 };
            self.gamefile.firingrangescores[i] = buffer.read_bits(numbits) as u8;
        }
        for i in 0..4 {
            self.gamefile.weaponsfound[i] = buffer.read_bits(8) as u8;
        }
        if self.gamefile_has_flag(GAMEFILEFLAG_FOUNDTIMEDMINE) {
            self.fr_set_weapon_found(WEAPON_TIMEDMINE as i32);
        }
        if self.gamefile_has_flag(GAMEFILEFLAG_FOUNDPROXYMINE) {
            self.fr_set_weapon_found(WEAPON_PROXIMITYMINE as i32);
        }
        if self.gamefile_has_flag(GAMEFILEFLAG_FOUNDREMOTEMINE) {
            self.fr_set_weapon_found(WEAPON_REMOTEMINE as i32);
        }
        self.gamefile_apply_options();
        0
    }

    /// `gamefile_save` (`gamefile.c:375`): the options into the flags, then
    /// the agent to file `fileid` on `device` (which takes its new id).
    pub fn gamefile_save(&mut self, device: i8, fileid: i32, deviceserial: u16) -> i32 {
        self.filelists.var80075bd0[FILETYPE_GAME as usize] = true;
        for (p, flags) in [
            (P1, [GAMEFILEFLAG_P1_FORWARDPITCH, GAMEFILEFLAG_P1_AUTOAIM, GAMEFILEFLAG_P1_AIMCONTROL, GAMEFILEFLAG_P1_SIGHTONSCREEN, GAMEFILEFLAG_P1_LOOKAHEAD, GAMEFILEFLAG_P1_AMMOONSCREEN, GAMEFILEFLAG_P1_HEADROLL, GAMEFILEFLAG_P1_SHOWGUNFUNCTION, GAMEFILEFLAG_P1_ALWAYSSHOWTARGET, GAMEFILEFLAG_P1_SHOWZOOMRANGE, GAMEFILEFLAG_P1_SHOWMISSIONTIME, GAMEFILEFLAG_P1_PAINTBALL]),
            (P2, [GAMEFILEFLAG_P2_FORWARDPITCH, GAMEFILEFLAG_P2_AUTOAIM, GAMEFILEFLAG_P2_AIMCONTROL, GAMEFILEFLAG_P2_SIGHTONSCREEN, GAMEFILEFLAG_P2_LOOKAHEAD, GAMEFILEFLAG_P2_AMMOONSCREEN, GAMEFILEFLAG_P2_HEADROLL, GAMEFILEFLAG_P2_SHOWGUNFUNCTION, GAMEFILEFLAG_P2_ALWAYSSHOWTARGET, GAMEFILEFLAG_P2_SHOWZOOMRANGE, GAMEFILEFLAG_P2_SHOWMISSIONTIME, GAMEFILEFLAG_P2_PAINTBALL]),
        ] {
            for (flag, bit) in flags.iter().zip(OPTION_BITS) {
                let on = self.option_bit(p, bit);
                pak_set_bitflag(*flag, &mut self.gamefile.flags, on);
            }
        }
        let o = self.options.clone();
        let (split, ratio) = (self.vars.screensplit, self.vars.screenratio);
        let mines = [self.fr_is_weapon_found(WEAPON_TIMEDMINE as i32), self.fr_is_weapon_found(WEAPON_PROXIMITYMINE as i32), self.fr_is_weapon_found(WEAPON_REMOTEMINE as i32)];
        let flags = &mut self.gamefile.flags;
        pak_set_bitflag(GAMEFILEFLAG_SCREENSPLIT, flags, split != 0);
        pak_set_bitflag(GAMEFILEFLAG_SCREENRATIO, flags, ratio != 0);
        pak_set_bitflag(GAMEFILEFLAG_SCREENSIZE_WIDE, flags, o.screensize == SCREENSIZE_WIDE);
        pak_set_bitflag(GAMEFILEFLAG_SCREENSIZE_CINEMA, flags, o.screensize == SCREENSIZE_CINEMA);
        pak_set_bitflag(GAMEFILEFLAG_HIRES, flags, o.hires);
        pak_set_bitflag(GAMEFILEFLAG_INGAMESUBTITLES, flags, o.ingamesubtitles);
        pak_set_bitflag(GAMEFILEFLAG_CUTSCENESUBTITLES, flags, o.cutscenesubtitles);
        pak_set_bitflag(GAMEFILEFLAG_LANGFILTERON, flags, o.langfilteron);
        pak_set_bitflag(GAMEFILEFLAG_FOUNDTIMEDMINE, flags, mines[0]);
        pak_set_bitflag(GAMEFILEFLAG_FOUNDPROXYMINE, flags, mines[1]);
        pak_set_bitflag(GAMEFILEFLAG_FOUNDREMOTEMINE, flags, mines[2]);
        pak_set_bitflag(GAMEFILEFLAG_ANTIPLAYERNUM, flags, o.pendingantiplayernum == 1);
        pak_set_bitflag(GAMEFILEFLAG_COOPRADARON, flags, o.coopradaron);
        pak_set_bitflag(GAMEFILEFLAG_COOPFRIENDLYFIRE, flags, o.coopfriendlyfire);
        pak_set_bitflag(GAMEFILEFLAG_ANTIRADARON, flags, o.antiradaron);
        if device < 0 {
            return -1;
        }
        let mut buffer = SaveBuffer::new();
        let f = self.gamefile.clone();
        buffer.write_string(&f.name);
        buffer.write_bits(f.thumbnail as u32, 5);
        buffer.write_bits(f.totaltime, 32);
        buffer.write_bits(f.autodifficulty as u32, 2);
        buffer.write_bits(f.autostageindex as u32, 5);
        // VOLUME(g_SfxVolume) >> 7, then >> 2 into 6 bits.
        let value = o.sfxvolume.min(0x5000) as u32 >> 7;
        buffer.write_bits(value >> 2, 6);
        let value = self.options_get_music_volume() as u32 >> 7;
        buffer.write_bits(value >> 2, 6);
        buffer.write_bits(o.soundmode as u32, 2);
        buffer.write_bits(self.mp.players[P1].controlmode as u32, 3);
        buffer.write_bits(self.mp.players[P2].controlmode as u32, 3);
        for i in 0..f.flags.len() {
            buffer.write_bits(f.flags[i] as u32, 8);
        }
        buffer.write_bits(f.unk1e as u32, 16);
        for i in 0..NUM_SOLOSTAGES {
            for j in 0..3 {
                buffer.write_bits(f.besttimes[i][j] as u32, 12);
            }
        }
        for i in 0..self.mp.challenges.len() {
            for j in 1..=4 {
                buffer.write_bits(self.challenge_is_completed_by_any_player_with_num_players(i, j) as u32, 1);
            }
        }
        for i in 0..3 {
            buffer.write_bits(f.coopcompletions[i], NUM_SOLOSTAGES as i32);
        }
        for i in 0..9 {
            buffer.write_bits(f.firingrangescores[i] as u32, if i == 8 { 2 } else { 8 });
        }
        for i in 0..4 {
            buffer.write_bits(f.weaponsfound[i] as u32, 8);
        }
        let mut newfileid = 0;
        let ret = self.paks.pak_save_at_guid(device, fileid, PAKFILETYPE_GAME, &buffer.bytes, Some(&mut newfileid));
        self.filemgr.lastpakerror = ret;
        if ret == 0 {
            self.gamefileguid = FileGuid { fileid: newfileid, deviceserial };
            return 0;
        }
        -1
    }

    /// `gamefile_get_overview` (`gamefile.c:533`): an agent file's name,
    /// stage, difficulty and time from the first 15 bytes of its body.
    pub fn gamefile_get_overview(body: &[u8]) -> (String, u8, u8, u32) {
        let mut buffer = SaveBuffer::new();
        buffer.prepare_string(body, 15);
        let mut name = [0u8; 12];
        buffer.read_string(&mut name, false);
        let stage = buffer.read_bits(5) as u8;
        let time = buffer.read_bits(32);
        let difficulty = buffer.read_bits(2) as u8;
        (name_of(&name), stage, difficulty, time)
    }

    // ---- training.c: what the agent found ----

    /// `fr_is_weapon_found` (`training.c:186`).
    pub fn fr_is_weapon_found(&self, weaponnum: i32) -> bool {
        if weaponnum <= WEAPON_UNARMED as i32 {
            return true;
        }
        if weaponnum < (self.gamefile.weaponsfound.len() * 8) as i32 {
            return self.gamefile.weaponsfound[(weaponnum >> 3) as usize] & (1 << (weaponnum % 8)) != 0;
        }
        false
    }

    /// `fr_set_weapon_found` (`training.c:207`).
    pub fn fr_set_weapon_found(&mut self, weaponnum: i32) {
        if weaponnum < (self.gamefile.weaponsfound.len() * 8) as i32 {
            self.gamefile.weaponsfound[(weaponnum >> 3) as usize] |= 1 << (weaponnum % 8);
        }
    }

    /// `ci_is_stage_complete` (`training.c:219`).
    pub fn ci_is_stage_complete(&self, stageindex: usize) -> bool {
        self.gamefile.besttimes[stageindex].iter().any(|&t| t != 0)
    }

    // ---- bossfile.c ----

    /// `bossfile_find_file_id` (`bossfile.c:62`): the occupied boss file, or
    /// else the first empty one.
    pub fn bossfile_find_file_id(&mut self) -> u32 {
        let mut fileids = Vec::new();
        let mut candidate = 0;
        if self.paks.pak_get_file_ids_by_type(SAVEDEVICE_GAMEPAK, PAKFILETYPE_BOSS, &mut fileids) == 0 {
            for &id in &fileids {
                let mut header = Default::default();
                self.paks.pak_find_file(SAVEDEVICE_GAMEPAK, id, Some(&mut header));
                if !header.occupied {
                    candidate = id;
                    break;
                }
            }
            for &id in &fileids {
                let mut header = Default::default();
                self.paks.pak_find_file(SAVEDEVICE_GAMEPAK, id, Some(&mut header));
                if header.occupied {
                    candidate = id;
                    break;
                }
            }
        }
        candidate
    }

    /// `bossfile_load` (`bossfile.c:92`), with `bossfile_load_full`: the
    /// boss file, or its defaults (saved) when there is none.
    pub fn bossfile_load(&mut self) {
        let mut failed = false;
        let mut buffer = SaveBuffer::new();
        let fileid = self.bossfile_find_file_id();
        if fileid == 0 {
            failed = true;
        } else if self.paks.pak_read_body_at_guid(SAVEDEVICE_GAMEPAK, fileid as i32, Some(&mut buffer.bytes), 0) != 0 {
            failed = true;
        }
        if !failed {
            let guid = buffer.read_guid();
            self.vars.bossfileid = guid.fileid;
            self.vars.bossdeviceserial = guid.deviceserial;
            self.mp.bossfile.unk89 = buffer.read_bits(1) as u8;
            self.vars.language = buffer.read_bits(4) as u8;
            for i in 0..8 {
                let mut name = cstr::<13>(&self.mp.bossfile.teamnames[i]);
                buffer.read_string(&mut name, true);
                self.mp.bossfile.teamnames[i] = name_of(&name);
            }
            let tracknum = buffer.read_bits(8) as u8;
            self.mp.bossfile.tracknum = if tracknum == 0xff { -1 } else { tracknum as i8 };
            for i in 0..self.mp.bossfile.multipletracknums.len() {
                self.mp.bossfile.multipletracknums[i] = buffer.read_bits(8) as u8;
            }
            self.mp.bossfile.usingmultipletunes = buffer.read_bits(1) != 0;
            self.mp.bossfile.alttitleunlocked = buffer.read_bits(1) as u8;
            self.mp.bossfile.alttitleenabled = buffer.read_bits(1) != 0;
        }
        if failed {
            self.bossfile_set_defaults();
            self.bossfile_save();
        }
    }

    /// `bossfile_save` (`bossfile.c:153`).
    pub fn bossfile_save(&mut self) {
        let mut buffer = SaveBuffer::new();
        buffer.write_guid(&FileGuid { fileid: self.vars.bossfileid, deviceserial: self.vars.bossdeviceserial });
        buffer.write_bits(self.mp.bossfile.unk89 as u32, 1);
        buffer.write_bits(self.vars.language as u32, 4);
        for i in 0..8 {
            let name = cstr::<13>(&self.mp.bossfile.teamnames[i]);
            buffer.write_string(&name);
        }
        if self.mp.bossfile.tracknum == -1 {
            buffer.write_bits(0xff, 8);
        } else {
            buffer.write_bits(self.mp.bossfile.tracknum as u8 as u32, 8);
        }
        for i in 0..self.mp.bossfile.multipletracknums.len() {
            buffer.write_bits(self.mp.bossfile.multipletracknums[i] as u32, 8);
        }
        buffer.write_bits(self.mp.bossfile.usingmultipletunes as u32, 1);
        buffer.write_bits(self.mp.bossfile.alttitleunlocked as u32, 1);
        buffer.write_bits(self.mp.bossfile.alttitleenabled as u32, 1);
        let fileid = self.bossfile_find_file_id();
        if fileid == 0 {
            log::error!("bossfile.c:375: fileGuid");
        }
        self.paks.pak_save_at_guid(SAVEDEVICE_GAMEPAK, fileid as i32, PAKFILETYPE_BOSS, &buffer.bytes, None);
    }

    /// `bossfile_set_defaults` (`bossfile.c:204`): no team names (the
    /// defaults fill them later), a random tune, every tune in the mix, no
    /// lock, no last agent. PD saves it at once.
    pub fn bossfile_set_defaults(&mut self) {
        self.mp.bossfile.teamnames = Default::default();
        self.mp.bossfile.tracknum = -1;
        self.mp_enable_all_multi_tracks();
        self.mp.bossfile.usingmultipletunes = false;
        self.mp.bossfile.unk89 = 0;
        self.mp.bossfile.locktype = MPLOCKTYPE_NONE as u8;
        self.vars.bossfileid = 0;
        self.vars.bossdeviceserial = 0;
        self.vars.language = 0;
        self.mp.bossfile.alttitleunlocked = 0;
        self.mp.bossfile.alttitleenabled = false;
        self.bossfile_save();
    }

    // ---- mplayer.c: the player file ----

    /// `mpplayerfile_load_gun_funcs` (`mplayer.c:3325`): 35 bits.
    fn mpplayerfile_load_gun_funcs(&mut self, buffer: &mut SaveBuffer, playernum: usize) {
        let mut bitsremaining = 35;
        let mut i = 0;
        while bitsremaining > 0 {
            let numbits = bitsremaining.min(8);
            self.mp.players[playernum].gunfuncs[i] = buffer.read_bits(numbits) as u8;
            bitsremaining -= 8;
            i += 1;
        }
    }

    /// `mpplayerfile_save_gun_funcs` (`mplayer.c:3344`).
    fn mpplayerfile_save_gun_funcs(&self, buffer: &mut SaveBuffer, playernum: usize) {
        let mut bitsremaining = 35;
        let mut i = 0;
        while bitsremaining > 0 {
            let numbits = bitsremaining.min(8);
            buffer.write_bits(self.mp.players[playernum].gunfuncs[i] as u32, numbits);
            bitsremaining -= 8;
            i += 1;
        }
    }

    /// `mpplayerfile_load_wad` (`mplayer.c:3363`): a player file into
    /// player `playernum`'s config (and its character when `arg2`), then the
    /// unlocks and its title.
    pub fn mpplayerfile_load_wad(&mut self, playernum: usize, buffer: &mut SaveBuffer, arg2: bool) {
        let mut name = cstr::<15>(&self.mp.players[playernum].base.name);
        buffer.read_string(&mut name, true);
        self.mp.players[playernum].base.name = name_of(&name);
        self.mp.players[playernum].career.time = buffer.read_bits(28);
        if arg2 {
            self.mp.players[playernum].base.mpheadnum = buffer.read_bits(7) as u8;
            self.mp.players[playernum].base.mpbodynum = buffer.read_bits(7) as u8;
            let _guid = buffer.read_guid();
            if self.mp.players[playernum].base.mpheadnum as i32 >= self.mp_get_num_heads() {
                // SUBST: PD keeps a PerfectHead whose camera file is named
                // (a Controller Pak's) / there are no PerfectHeads here, so
                // the player gets PD's fallback head.
                self.mp.players[playernum].base.mpheadnum = MPHEAD_DARK_COMBAT;
            }
        } else {
            buffer.read_bits(7);
            buffer.read_bits(7);
            buffer.read_guid();
        }
        let p = &mut self.mp.players[playernum];
        p.base.displayoptions = buffer.read_bits(8);
        let c = &mut p.career;
        c.kills = buffer.read_bits(20);
        c.deaths = buffer.read_bits(20);
        c.gamesplayed = buffer.read_bits(19);
        c.gameswon = buffer.read_bits(19);
        c.gameslost = buffer.read_bits(19);
        c.distance = buffer.read_bits(25);
        c.accuracy = buffer.read_bits(10);
        c.damagedealt = buffer.read_bits(26);
        c.painreceived = buffer.read_bits(26);
        c.headshots = buffer.read_bits(20);
        c.ammoused = buffer.read_bits(30);
        c.accuracymedals = buffer.read_bits(18);
        c.headshotmedals = buffer.read_bits(18);
        c.killmastermedals = buffer.read_bits(18);
        c.survivormedals = buffer.read_bits(16);
        p.controlmode = buffer.read_bits(2) as u8;
        p.options = buffer.read_bits(12);
        for i in 0..self.mp.challenges.len() {
            for j in 1..=4 {
                let done = buffer.read_bits(1) != 0;
                self.challenge_set_completed_by_player_with_num_players(playernum, i, j, done);
            }
        }
        self.challenge_determine_unlocked_features();
        self.mp.players[playernum].title = pd_core::mp::mp_calculate_player_title(&self.mp.players[playernum].career);
        self.mpplayerfile_load_gun_funcs(buffer, playernum);
    }

    /// `mpplayerfile_save_wad` (`mplayer.c:3428`): player `playernum` into a
    /// player file, each statistic capped to its bits (the caps stay in the
    /// config, as PD's do).
    pub fn mpplayerfile_save_wad(&mut self, playernum: usize, buffer: &mut SaveBuffer) {
        let name = cstr::<15>(&self.mp.players[playernum].base.name);
        buffer.write_string(&name);
        let c = &mut self.mp.players[playernum].career;
        c.time = c.time.min(0x0fffffff);
        buffer.write_bits(c.time, 28);
        let (head, body) = (self.mp.players[playernum].base.mpheadnum, self.mp.players[playernum].base.mpbodynum);
        buffer.write_bits(head as u32, 7);
        buffer.write_bits(body as u32, 7);
        // (A head past g_MpHeads is a PerfectHead, whose guid PD writes; there
        // are none here.)
        buffer.write_guid(&FileGuid::default());
        buffer.write_bits(self.mp.players[playernum].base.displayoptions, 8);
        let c = &mut self.mp.players[playernum].career;
        for (v, bits) in [
            (&mut c.kills, 20),
            (&mut c.deaths, 20),
            (&mut c.gamesplayed, 19),
            (&mut c.gameswon, 19),
            (&mut c.gameslost, 19),
            (&mut c.distance, 25),
            (&mut c.accuracy, 10),
            (&mut c.damagedealt, 26),
            (&mut c.painreceived, 26),
            (&mut c.headshots, 20),
            (&mut c.ammoused, 30),
            (&mut c.accuracymedals, 18),
            (&mut c.headshotmedals, 18),
            (&mut c.killmastermedals, 18),
            (&mut c.survivormedals, 16),
        ] {
            let max = (1u32 << bits) - 1;
            if *v > max {
                *v = max;
            }
            buffer.write_bits(*v, bits);
        }
        buffer.write_bits(self.mp.players[playernum].controlmode as u32, 2);
        buffer.write_bits(self.mp.players[playernum].options, 12);
        for i in 0..self.mp.challenges.len() {
            for j in 1..=4 {
                buffer.write_bits(self.challenge_is_completed_by_player_with_num_players(playernum, i, j) as u32, 1);
            }
        }
        self.mpplayerfile_save_gun_funcs(buffer, playernum);
    }

    /// `mpplayerfile_get_overview` (`mplayer.c:3559`): a player file's name
    /// and play time from its first 15 bytes.
    pub fn mpplayerfile_get_overview(body: &[u8]) -> (String, u32) {
        let mut buffer = SaveBuffer::new();
        buffer.prepare_string(body, 15);
        let mut name = [0u8; 12];
        buffer.read_string(&mut name, false);
        let playtime = buffer.read_bits(28);
        (name_of(&name), playtime)
    }

    /// `mpplayerfile_save` (`mplayer.c:3569`).
    pub fn mpplayerfile_save(&mut self, playernum: usize, device: i8, fileid: i32, deviceserial: u16) -> i32 {
        if device < 0 {
            return -1;
        }
        let mut buffer = SaveBuffer::new();
        self.mpplayerfile_save_wad(playernum, &mut buffer);
        self.filelists.var80075bd0[FILETYPE_MPPLAYER as usize] = true;
        let mut newfileid = 0;
        let ret = self.paks.pak_save_at_guid(device, fileid, PAKFILETYPE_MPPLAYER, &buffer.bytes, Some(&mut newfileid));
        if ret == 0 {
            self.mp.players[playernum].fileguid = FileGuid { fileid: newfileid, deviceserial };
            return 0;
        }
        self.filemgr.lastpakerror = ret;
        -1
    }

    /// `mpplayerfile_load` (`mplayer.c:3597`).
    pub fn mpplayerfile_load(&mut self, playernum: usize, device: i8, fileid: i32, deviceserial: u16) -> i32 {
        if device < 0 {
            return -1;
        }
        let mut buffer = SaveBuffer::new();
        let ret = self.paks.pak_read_body_at_guid(device, fileid, Some(&mut buffer.bytes), 0);
        if ret == 0 {
            self.mp.players[playernum].fileguid = FileGuid { fileid, deviceserial };
            self.mpplayerfile_load_wad(playernum, &mut buffer, true);
            self.mp.players[playernum].handicap = 0x80;
            return 0;
        }
        self.filemgr.lastpakerror = ret;
        -1
    }

    // ---- mplayer.c: the setup file ----

    /// `scenario_read_save` (`scenarios.c:418`): King of the Hill's hill
    /// time, else a byte skipped.
    fn scenario_read_save(&mut self, buffer: &mut SaveBuffer) {
        if self.mp.setup.scenario == MPSCENARIO_KINGOFTHEHILL {
            self.vars.mphilltime = buffer.read_bits(8) as u8;
        } else {
            buffer.read_bits(8);
        }
    }

    /// `scenario_write_save` (`scenarios.c:435`).
    fn scenario_write_save(&self, buffer: &mut SaveBuffer) {
        if self.mp.setup.scenario == MPSCENARIO_KINGOFTHEHILL {
            buffer.write_bits(self.vars.mphilltime as u32, 8);
        } else {
            buffer.write_bits(0, 8);
        }
    }

    /// `mpsetupfile_load_wad` (`mplayer.c:3806`).
    pub fn mpsetupfile_load_wad(&mut self, buffer: &mut SaveBuffer) {
        let mut name = cstr::<16>(&self.mp.setup.name);
        buffer.read_string(&mut name, false);
        self.mp.setup.name = name_of(&name);
        buffer.read_bits(4);
        self.mp.setup.stagenum = buffer.read_bits(7) as u8;
        self.mp.setup.scenario = buffer.read_bits(3) as u8;
        self.scenario_init();
        self.scenario_read_save(buffer);
        self.mp.setup.options = buffer.read_bits(21);
        self.mp.setup.chrslots &= 0x000f;
        for i in 0..8 {
            self.mp.bots[i].base.name.clear();
            self.mp.bots[i].ty = buffer.read_bits(5) as u8;
            self.mp.bots[i].difficulty = buffer.read_bits(3) as u8;
            for j in 0..4 {
                self.mp.simdiffs[i][j] = self.mp.bots[i].difficulty;
            }
            if self.mp.bots[i].difficulty != BOTDIFF_DISABLED {
                self.mp.setup.chrslots |= 1 << (i + 4);
            }
            self.mp.bots[i].base.mpheadnum = buffer.read_bits(7) as u8;
            self.mp.bots[i].base.mpbodynum = buffer.read_bits(7) as u8;
            self.mp.bots[i].base.team = buffer.read_bits(3) as u8;
        }
        self.mp_generate_bot_names();
        for i in 0..6 {
            self.mp.setup.weapons[i] = buffer.read_bits(7) as u8;
        }
        self.mp_find_weaponsetnum_by_weapons();
        self.mp.setup.timelimit = buffer.read_bits(6) as u8;
        self.mp.setup.scorelimit = buffer.read_bits(7) as u8;
        self.mp.setup.teamscorelimit = buffer.read_bits(9) as u16;
        for i in 0..4 {
            self.mp.players[i].base.team = buffer.read_bits(3) as u8;
        }
        self.challenge_force_unlock_bot_features();
    }

    /// `mpsetupfile_save_wad` (`mplayer.c:3860`).
    pub fn mpsetupfile_save_wad(&self, buffer: &mut SaveBuffer) {
        let name = cstr::<16>(&self.mp.setup.name);
        buffer.write_string(&name);
        let numsims = (0..8).filter(|i| self.mp.setup.chrslots & (1 << (i + 4)) != 0).count();
        buffer.write_bits(numsims as u32, 4);
        buffer.write_bits(self.mp.setup.stagenum as u32, 7);
        buffer.write_bits(self.mp.setup.scenario as u32, 3);
        self.scenario_write_save(buffer);
        buffer.write_bits(self.mp.setup.options, 21);
        for i in 0..8 {
            let b = &self.mp.bots[i];
            buffer.write_bits(b.ty as u32, 5);
            if self.mp.setup.chrslots & (1 << (i + 4)) != 0 {
                buffer.write_bits(b.difficulty as u32, 3);
            } else {
                buffer.write_bits(BOTDIFF_DISABLED as u32, 3);
            }
            buffer.write_bits(b.base.mpheadnum as u32, 7);
            let mpbodynum = if b.base.mpbodynum == 0xff {
                let mut profilenum = Self::mp_find_bot_profile(b.ty as i32, b.difficulty as i32);
                if profilenum < 0 || profilenum as usize >= BOT_PROFILES.len() {
                    profilenum = 0;
                }
                BOT_PROFILES[profilenum as usize].body as u32
            } else {
                b.base.mpbodynum as u32
            };
            buffer.write_bits(mpbodynum, 7);
            buffer.write_bits(b.base.team as u32, 3);
        }
        for i in 0..6 {
            buffer.write_bits(self.mp.setup.weapons[i] as u32, 7);
        }
        buffer.write_bits(self.mp.setup.timelimit as u32, 6);
        buffer.write_bits(self.mp.setup.scorelimit as u32, 7);
        buffer.write_bits(self.mp.setup.teamscorelimit as u32, 9);
        for i in 0..4 {
            buffer.write_bits(self.mp.players[i].base.team as u32, 3);
        }
    }

    /// `mpsetupfile_get_overview` (`mplayer.c:3922`): a setup file's name,
    /// simulant count, arena and scenario from its first 15 bytes.
    pub fn mpsetupfile_get_overview(body: &[u8]) -> (String, u16, u16, u16) {
        let mut buffer = SaveBuffer::new();
        buffer.prepare_string(body, 15);
        let mut name = [0u8; 12];
        buffer.read_string(&mut name, false);
        let numsims = buffer.read_bits(4) as u16;
        let stagenum = buffer.read_bits(7) as u16;
        let scenarionum = buffer.read_bits(3) as u16;
        (name_of(&name), numsims, stagenum, scenarionum)
    }

    /// `mpsetupfile_save` (`mplayer.c:3935`).
    pub fn mpsetupfile_save(&mut self, device: i8, fileid: i32, deviceserial: u16) -> i32 {
        if device < 0 {
            return -1;
        }
        let mut buffer = SaveBuffer::new();
        self.mpsetupfile_save_wad(&mut buffer);
        let mut newfileid = 0;
        let ret = self.paks.pak_save_at_guid(device, fileid, PAKFILETYPE_MPSETUP, &buffer.bytes, Some(&mut newfileid));
        self.filelists.var80075bd0[FILETYPE_MPSETUP as usize] = true;
        if ret == 0 {
            self.mp.setup.fileguid = FileGuid { fileid: newfileid, deviceserial };
            return 0;
        }
        self.filemgr.lastpakerror = ret;
        -1
    }

    /// `mpsetupfile_load` (`mplayer.c:3962`).
    pub fn mpsetupfile_load(&mut self, device: i8, fileid: i32, deviceserial: u16) -> i32 {
        if device < 0 {
            return -1;
        }
        let mut buffer = SaveBuffer::new();
        let ret = self.paks.pak_read_body_at_guid(device, fileid, Some(&mut buffer.bytes), 0);
        if ret == 0 {
            self.mp.setup.fileguid = FileGuid { fileid, deviceserial };
            self.mpsetupfile_load_wad(&mut buffer);
            return 0;
        }
        self.filemgr.lastpakerror = ret;
        -1
    }

    // ---- pak.c:1641 ----

    /// `paks_init` (`pak.c:1641`): the Game Pak's file system, the boss file,
    /// and a default agent (none is chosen yet: `g_GameFileGuid`'s serial 0).
    pub fn paks_init(&mut self) {
        self.paks.paks_init(&mut self.rng);
        self.filelist_invalidate_pak(SAVEDEVICE_GAMEPAK);
        self.bossfile_load();
        self.gamefile_load_defaults();
        self.gamefile_apply_options();
        self.gamefileguid.deviceserial = 0;
    }

    /// `mp_set_default_names_if_empty` (`mplayer.c:554`), which each stage
    /// load runs (`lv.c:367`).
    pub fn mp_set_default_names_if_empty(&mut self) {
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
    }
}

/// The `OPTION_*` bits in `gamefile_apply_options`' order.
const OPTION_BITS: [u16; 12] = [
    OPTION_FORWARDPITCH,
    OPTION_AUTOAIM,
    OPTION_AIMCONTROL,
    OPTION_SIGHTONSCREEN,
    OPTION_LOOKAHEAD,
    OPTION_AMMOONSCREEN,
    OPTION_HEADROLL,
    OPTION_SHOWGUNFUNCTION,
    OPTION_ALWAYSSHOWTARGET,
    OPTION_SHOWZOOMRANGE,
    OPTION_SHOWMISSIONTIME,
    OPTION_PAINTBALL,
];
