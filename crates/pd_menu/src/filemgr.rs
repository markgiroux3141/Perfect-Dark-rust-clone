//! The file manager (`filemgr.c`), its file lists (`filelist.c`) and the
//! menus' queued saves (`menu_queue_save`, `menu_save_file`, `menu.c`): the
//! agent select at power on ("Choose Your Reality") and its Game Files
//! sibling (copy, delete), where a new file goes ("Select Location"), saving
//! and loading any file by its GUID, and the errors and confirmations
//! around them.
//!
//! The devices are PD's: the Game Pak (the cartridge's EEPROM) and four
//! Controller Paks, which are never plugged in, so their rows in each list
//! are blank and can't be chosen, as on a console with none inserted.
//! `menudata_filemgr` overlays the other `menudata_*` in PD's `struct menu`;
//! here it is a field of its own ([`FilemgrMenuData`]), as nothing reads one
//! through the other.

use pd_core::ids::{
    FILEERROR_ALREADYLOADED, FILEERROR_DELETEFAILED, FILEERROR_DELETENOTEFAILED, FILEERROR_LOADFAILED, FILEERROR_NOPAK, FILEERROR_OUTOFMEMORY, FILEERROR_PAKDAMAGED, FILEERROR_PAKREMOVED, FILEERROR_SAVEFAILED, FILEOP_LOAD_GAME, FILEOP_LOAD_MPPLAYER, FILEOP_LOAD_MPSETUP,
    FILEOP_READ_GAME, FILEOP_READ_MPPLAYER, FILEOP_READ_MPSETUP, FILEOP_SAVE_GAME_000, FILEOP_SAVE_GAME_001, FILEOP_SAVE_GAME_002, FILEOP_SAVE_MPPLAYER, FILEOP_SAVE_MPSETUP, FILEOP_WRITE_GAME, FILEOP_WRITE_MPPLAYER, FILEOP_WRITE_MPSETUP, FILESTATE_SELECTED,
    FILETYPE_GAME, FILETYPE_MPPLAYER, FILETYPE_MPSETUP, PAKFILETYPE_CAMERA, PAKFILETYPE_GAME, PAKFILETYPE_MPPLAYER, PAKFILETYPE_MPSETUP, SAVEDEVICE_CONTROLLERPAK1, SAVEDEVICE_CONTROLLERPAK2, SAVEDEVICE_CONTROLLERPAK3, SAVEDEVICE_CONTROLLERPAK4, SAVEDEVICE_GAMEPAK,
    SAVEDEVICE_INVALID, SOLOSTAGEINDEX_SKEDARRUINS, DIFF_PA,
};
use pd_core::lang::tx;
use pd_core::savebuffer::{cstr, cstr_to_string, savebuffer_bitstring_to_cstring, savebuffer_cstring_to_bitstring, FileGuid};
use pd_core::text::FontId;

use super::generated::*;
use super::types::*;
use super::MenuSystem;

type R = HRet;

fn ok() -> R {
    HRet::I(0)
}

/// `FILEOP_IS_SAVE` (`constants.h:930`).
fn fileop_is_save(op: u8) -> bool {
    (op as i32) < 100
}

/// `struct filelistfile` (`types.h:4082`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileListFile {
    pub fileid: i32,
    pub deviceserial: u16,
    /// The first 16 bytes of the file's body (its name, and more).
    pub name: [u8; 16],
}

/// `struct filelist` (`types.h:4091`): every file of a type on every device.
#[derive(Clone, Debug, Default)]
pub struct FileList {
    /// At most 30.
    pub files: Vec<FileListFile>,
    /// By device (Controller Paks, then the Game Pak): -1 when the device
    /// can't be read.
    pub spacesfree: [i8; 5],
    /// By device: the empty file a new one would go into.
    pub deviceguids: [FileGuid; 5],
    /// By display order (the Game Pak, then the Controller Paks): the
    /// device's first file, -1 without any.
    pub devicestartindexes: [i8; 5],
    /// By device: the empty files seen (the first is the swap space).
    pub unk305: [i8; 5],
    pub numdevices: u8,
    /// `FILETYPE_*`.
    pub filetype: u8,
    pub timeuntilupdate: u8,
    pub updatedthisframe: bool,
}

impl FileList {
    pub fn numfiles(&self) -> i32 {
        self.files.len() as i32
    }
}

/// `filelist.c`'s globals.
#[derive(Clone, Debug)]
pub struct FileLists {
    /// `g_FileLists[MAX_PLAYERS]`.
    pub lists: [Option<FileList>; 4],
    /// `g_FilelistKnownPlugCounts`.
    pub knownplugcounts: [i32; 5],
    /// `filelists_tick`'s `doneinit`.
    pub doneinit: bool,
    /// `var80075bd0[FILETYPE_*]`: a list of this type is rebuilt next tick.
    pub var80075bd0: [bool; 4],
}

impl Default for FileLists {
    fn default() -> Self {
        FileLists { lists: Default::default(), knownplugcounts: [-1; 5], doneinit: false, var80075bd0: [true; 4] }
    }
}

/// `struct menudata_filemgr` (`types.h:3758`): one player's file manager.
#[derive(Clone, Debug, Default)]
pub struct FilemgrMenuData {
    pub filetypeplusone: u32,
    pub isdeletingforsave: bool,
    pub unke2c: i32,
    /// `FILEERROR_*`.
    pub errno: u16,
    /// The file a delete names (PD points into its list; a copy here).
    pub filetodelete: Option<FileListFile>,
    pub device1: u8,
    pub filetypetodelete: u8,
    /// What `filemgr_push_select_location_dialog` was opened for: 0 a new
    /// agent, 1-4 a copy, 6 a player, 7 a setup, 9+ a retried save.
    pub unke3e: u8,
    pub listnum: u8,
    /// `FILEOP_*`.
    pub fileop: u8,
    /// `mpplayernum` (the union's `unke44`): the player a player file is for.
    pub mpplayernum: i32,
    /// `unke44` as a pointer: a copy's body, read then written.
    pub copybuf: Vec<u8>,
    pub fileid: u32,
    pub deviceserial: u32,
    pub isretryingsave: u16,
    pub device2: u8,
    pub filename: [u8; 16],
}

/// `filemgr.c`'s globals.
#[derive(Clone, Copy, Debug, Default)]
pub struct Filemgr {
    /// `g_FilemgrFileToCopy`, `var800a21e8` (where the copy goes),
    /// `g_FilemgrFileToDelete`.
    pub filetocopy: FileGuid,
    pub var800a21e8: FileGuid,
    pub filetodelete: FileGuid,
    /// `g_FilemgrLastPakError`.
    pub lastpakerror: i32,
}

/// `g_FileTypeSizes` (`filemgr.c:237`): game, setup, player, PerfectHead.
const FILE_TYPE_SIZES: [usize; 4] = [0xa0, 0x31, 0x4e, 0x4a0];

/// How `filemgr_get_device_name_or_start_index` answers.
enum NameOrIndex {
    Name(String),
    Index(i32),
}

impl MenuSystem {
    fn fm(&mut self) -> &mut FilemgrMenuData {
        let p = self.mpplayernum;
        &mut self.menus[p].fm
    }

    fn fmr(&self) -> &FilemgrMenuData {
        &self.menus[self.mpplayernum].fm
    }

    fn filelist(&self, listnum: usize) -> Option<&FileList> {
        self.filelists.lists.get(listnum).and_then(|l| l.as_ref())
    }

    fn cur_filelist(&self) -> Option<&FileList> {
        self.filelist(self.fmr().listnum as usize)
    }

    // ---- filelist.c ----

    /// `func0f110bf8` (`filelist.c:39`): every file list freed.
    pub fn filelists_free(&mut self) {
        self.filelists.lists = Default::default();
    }

    /// `filelist_create` (`filelist.c:54`): list `listnum` of `filetype`,
    /// built on the next tick.
    pub fn filelist_create(&mut self, listnum: usize, filetype: u8) {
        let l = self.filelists.lists[listnum].get_or_insert_with(FileList::default);
        l.timeuntilupdate = 1;
        l.filetype = filetype;
        self.filelists_active = true;
    }

    /// `filelist_find_or_create` (`filelist.c:71`).
    pub fn filelist_find_or_create(&mut self, filetype: u8) -> i32 {
        let mut bestindex = -1;
        for i in 0..4 {
            match &self.filelists.lists[i] {
                Some(l) => {
                    if l.filetype == filetype {
                        return i as i32;
                    }
                }
                None => {
                    if bestindex == -1 {
                        bestindex = i as i32;
                    }
                }
            }
        }
        if bestindex >= 0 {
            self.filelist_create(bestindex as usize, filetype);
            return bestindex;
        }
        -1
    }

    /// `filelist_invalidate_pak` (`filelist.c:97`).
    pub fn filelist_invalidate_pak(&mut self, device: i8) {
        self.filelists.knownplugcounts[device as usize] = -1;
    }

    /// `filelists_tick` (`filelist.c:103`, NTSC 1.0+): each list rebuilt
    /// when its timer runs out, its type was written or a pak came or went.
    pub fn filelists_tick(&mut self) {
        if !self.filelists.doneinit {
            self.filelists.knownplugcounts = [-1; 5];
            self.filelists.doneinit = true;
        }
        let mut updateall = false;
        for i in 0..5i8 {
            if self.paks.pak0f1167d8(i) != 0 && self.paks.pak_get_plug_count(i) != self.filelists.knownplugcounts[i as usize] {
                updateall = true;
                self.filelists.knownplugcounts[i as usize] = self.paks.pak_get_plug_count(i);
            }
        }
        for i in 0..4 {
            let Some(l) = self.filelists.lists[i].as_mut() else { continue };
            l.updatedthisframe = false;
            let mut update = updateall;
            if l.timeuntilupdate > 0 {
                l.timeuntilupdate -= 1;
                if l.timeuntilupdate == 0 {
                    update = true;
                }
            }
            if self.filelists.var80075bd0[l.filetype as usize & 3] {
                update = true;
            }
            if update {
                let mut l = self.filelists.lists[i].take().unwrap();
                self.filelist_update(&mut l);
                l.updatedthisframe = true;
                self.filelists.lists[i] = Some(l);
            }
        }
        self.filelists.var80075bd0 = [false; 4];
    }

    /// `filelist_update` (`filelist.c:173`): the files of the list's type on
    /// each device (the Game Pak first), their names, and each device's free
    /// spaces (its empty files less the swap).
    pub fn filelist_update(&mut self, list: &mut FileList) {
        let types = [PAKFILETYPE_GAME, PAKFILETYPE_MPSETUP, PAKFILETYPE_MPPLAYER, PAKFILETYPE_CAMERA];
        let dis2dev = [SAVEDEVICE_GAMEPAK, SAVEDEVICE_CONTROLLERPAK1, SAVEDEVICE_CONTROLLERPAK2, SAVEDEVICE_CONTROLLERPAK3, SAVEDEVICE_CONTROLLERPAK4];
        let dev2dis = [1usize, 2, 3, 4, 0];
        let mut ids: Vec<(u32, i8)> = Vec::new();
        list.numdevices = 0;
        for (i, &dev) in dis2dev.iter().enumerate() {
            list.unk305[dev as usize] = 0;
            list.devicestartindexes[i] = -1;
            let mut fileids = Vec::new();
            let ret = self.paks.pak_get_file_ids_by_type(dev, types[list.filetype as usize & 3], &mut fileids);
            if ret == 0 {
                for id in fileids {
                    ids.push((id, dev));
                }
                list.spacesfree[dev as usize] = 0;
                // (FILETYPE_CAMERA's free spaces are a Controller Pak's.)
                list.deviceguids[dev as usize] = FileGuid { fileid: 0, deviceserial: self.paks.pak_get_serial(dev) };
            } else {
                list.spacesfree[dev as usize] = -1;
                if ret == 13 {
                    list.timeuntilupdate = 5;
                }
            }
        }
        list.files.clear();
        for (id, dev) in ids {
            // SUBST: PD writes past `files[30]` / the list stops at 30 (a
            // Game Pak holds 5 files of a type).
            if list.files.len() >= 30 {
                break;
            }
            let mut name = [0u8; 16];
            let ret = self.paks.pak_read_body_at_guid(dev, id as i32, Some(&mut name), 16);
            if ret == 0 {
                let dis = dev2dis[dev as usize];
                if list.devicestartindexes[dis] == -1 {
                    list.numdevices += 1;
                    list.devicestartindexes[dis] = list.files.len() as i8;
                }
                list.files.push(FileListFile { fileid: id as i32, deviceserial: self.paks.pak_get_serial(dev), name });
            } else if ret == 10 {
                list.unk305[dev as usize] += 1;
                if list.unk305[dev as usize] >= 2 {
                    list.spacesfree[dev as usize] += 1;
                    if list.deviceguids[dev as usize].fileid == 0 {
                        list.deviceguids[dev as usize] = FileGuid { fileid: id as i32, deviceserial: self.paks.pak_get_serial(dev) };
                    }
                }
            }
        }
    }

    /// `menu_stop` (`menustop.c:30`): the file lists freed.
    pub fn menu_stop(&mut self) {
        if self.filelists_active {
            self.filelists_active = false;
            self.mpsetup_filelists_made = false;
            self.filelists_free();
        }
    }

    // ---- menu.c: the queued saves ----

    /// `menu_queue_save` (`menu.c:6046`): 0-3 a player's file, 4 the agent.
    pub fn menu_queue_save(&mut self, playernum: u8) {
        let n = self.menudata.numpendingsaves as usize;
        // SUBST: PD writes pendingsaves[-1] (`lastperfectheadfile`, unused
        // here) when the count went negative / the write is dropped.
        if n < self.menudata.pendingsaves.len() {
            self.menudata.pendingsaves[n] = playernum;
        }
        self.menudata.numpendingsaves += 1;
        self.menudata.savetimer = 0;
    }

    /// `menu_save_file` (`menu.c:1496`, NTSC 1.0+): queued save `arg0`. The
    /// agent waits while a dialog is two deep or a danger dialog is up (bar
    /// a challenge's verdict).
    pub fn menu_save_file(&mut self, arg0: i32) -> bool {
        let mut save = true;
        let pending = self.menudata.pendingsaves.get(arg0.max(0) as usize).copied().unwrap_or(0xff);
        if arg0 < 0 {
            // (PD reads pendingsaves[-1], which is savetimer's neighbour.)
            return false;
        }
        if pending == 4 {
            let prev = self.mpplayernum;
            for i in (0..4).rev() {
                if self.menus[i].curdialog.is_some() {
                    self.mpplayernum = i;
                }
            }
            if self.mr().depth >= 2 {
                save = false;
            }
            if let Some(def) = self.cur_def() {
                if def.ty == MENUDIALOGTYPE_DANGER {
                    save = false;
                    if std::ptr::eq(def, &G_MP_ENDSCREEN_CHALLENGE_CHEATED_MENU_DIALOG) || std::ptr::eq(def, &G_MP_ENDSCREEN_CHALLENGE_FAILED_MENU_DIALOG) {
                        save = true;
                    }
                }
            }
            if save {
                let guid = self.gamefileguid;
                self.filemgr_save_or_load(guid, FILEOP_SAVE_GAME_000, 0);
            }
            self.mpplayernum = prev;
        } else if pending < 4 {
            let prev = self.mpplayernum;
            self.mpplayernum = pending as usize;
            let guid = self.mp.players[self.mpplayernum].fileguid;
            self.filemgr_save_or_load(guid, FILEOP_SAVE_MPPLAYER, self.mpplayernum as u32);
            save = true;
            self.mpplayernum = prev;
        }
        if save {
            self.menudata.numpendingsaves -= 1;
        }
        save
    }

    /// `menu_tick`'s queued saves (`menutick.c:98`): one save once every
    /// dialog has settled (or after 50 frames, 40 in a match).
    pub fn menu_tick_pending_saves(&mut self, anyopen: bool) {
        if anyopen && self.menudata.numpendingsaves > 0 && self.menudata.bgsettled {
            let mut maxwait = 50;
            let mut busy = false;
            for m in &self.menus {
                if let Some(d) = m.curdialog {
                    let st = m.dialogs[d].state;
                    if st == MENUDIALOGSTATE_OPENING || st == MENUDIALOGSTATE_POPULATING || st == MENUDIALOGSTATE_PREOPEN {
                        busy = true;
                    }
                }
            }
            if self.in_match {
                maxwait = 40;
            }
            if self.menudata.savetimer > maxwait || !busy {
                let n = self.menudata.numpendingsaves - 1;
                self.menu_save_file(n);
            } else {
                self.menudata.savetimer += 1;
            }
        }
    }

    /// The saves `menu_close_dialog` and `menu_save_and_close_all` run first
    /// (`menu.c:1604`, `:3410`): every queued one, newest first.
    pub fn menu_save_all_pending(&mut self) {
        if self.menudata.numpendingsaves > 0 {
            let mut i = self.menudata.numpendingsaves;
            while i >= 0 {
                self.menu_save_file(i);
                i -= 1;
            }
        }
    }

    // ---- filemgr.c: names ----

    /// `filemgr_get_device_name` (`filemgr.c:150`).
    pub fn filemgr_get_device_name(&self, index: i32) -> Option<String> {
        let names = [tx(B_OPTIONS, 112), tx(B_OPTIONS, 113), tx(B_OPTIONS, 114), tx(B_OPTIONS, 115), tx(B_OPTIONS, 111), tx(B_MPWEAPONS, 229)];
        names.get(index as usize).map(|&t| self.lang(t))
    }

    /// `filemgr_get_select_name` (`filemgr.c:186`): an agent's or setup's
    /// name, or a player's name and play time ("Joanna-1:05").
    pub fn filemgr_get_select_name(&self, file: &FileListFile, filetype: i32) -> String {
        let text = match filetype {
            FILETYPE_GAME | FILETYPE_MPSETUP => {
                let mut c = [0u8; 12];
                savebuffer_bitstring_to_cstring(&file.name, &mut c, false);
                cstr_to_string(&c)
            }
            FILETYPE_MPPLAYER => {
                let (name, total) = MenuSystem::mpplayerfile_get_overview(&file.name);
                let mut t = format!("{name}-");
                if total >= 0x7ffffff {
                    t.push_str("==:==");
                } else {
                    let totalinseconds = total / 60;
                    let totalinhours = totalinseconds / 60;
                    let minutes = totalinseconds % 60;
                    let days = totalinhours / 24;
                    let hours = totalinhours % 24;
                    if days == 0 {
                        t.push_str(&format!("{hours}:{minutes:02}"));
                    } else {
                        t.push_str(&format!("{days}:{hours:02}:{minutes:02}"));
                    }
                }
                t
            }
            _ => String::new(),
        };
        format!("{text}\n")
    }

    /// `filemgr_set_device1_by_serial` (`filemgr.c:261`).
    fn filemgr_set_device1_by_serial(&mut self, deviceserial: i32) {
        let device = self.paks.pak_find_by_serial(deviceserial);
        self.fm().device1 = if device >= 0 { device as u8 } else { SAVEDEVICE_INVALID as u8 };
    }

    /// `filemgr_set_file_to_delete` (`filemgr.c:281`).
    fn filemgr_set_file_to_delete(&mut self, file: FileListFile, filetype: u8) {
        let fm = self.fm();
        fm.filetypetodelete = filetype;
        fm.filetodelete = Some(file);
        self.filemgr_set_device1_by_serial(file.deviceserial as i32);
    }

    /// `filemgr_push_error_dialog` (`filemgr.c:370`).
    pub fn filemgr_push_error_dialog(&mut self, errno: u16) {
        self.fm().errno = errno;
        self.menu_push_dialog(&G_FILEMGR_ERROR_MENU_DIALOG);
    }

    /// `filemgr_get_device_name_or_start_index` (`filemgr.c:418`): list
    /// `listnum`'s `optionindex`th device group, by name or first file.
    fn filemgr_get_device_name_or_start_index(&self, listnum: usize, operation: i32, optionindex: i32) -> NameOrIndex {
        let names = [tx(B_OPTIONS, 111), tx(B_OPTIONS, 112), tx(B_OPTIONS, 113), tx(B_OPTIONS, 114), tx(B_OPTIONS, 115)];
        let mut remaining = optionindex;
        if let Some(l) = self.filelist(listnum) {
            for i in 0..5 {
                if l.devicestartindexes[i] != -1 {
                    if remaining == 0 {
                        if operation == MENUOP_GET_OPTGROUP_TEXT {
                            return NameOrIndex::Name(self.lang(names[i]));
                        }
                        return NameOrIndex::Index(l.devicestartindexes[i] as i32);
                    }
                    remaining -= 1;
                }
            }
        }
        NameOrIndex::Index(0)
    }

    pub(crate) fn filemgr_group_text(&self, listnum: usize, value: i32) -> R {
        match self.filemgr_get_device_name_or_start_index(listnum, MENUOP_GET_OPTGROUP_TEXT, value) {
            NameOrIndex::Name(s) => s.into(),
            NameOrIndex::Index(i) => HRet::I(i),
        }
    }

    pub(crate) fn filemgr_group_start(&self, listnum: usize, value: i32) -> i32 {
        match self.filemgr_get_device_name_or_start_index(listnum, MENUOP_GET_OPTGROUP_START_INDEX, value) {
            NameOrIndex::Index(i) => i,
            NameOrIndex::Name(_) => 0,
        }
    }

    // ---- filemgr.c: the operations ----

    /// `func0f10898c` (`filemgr.c:522`): a failed operation's clean-up.
    fn func0f10898c(&mut self) {
        let fm = self.fm();
        if matches!(fm.fileop as i32, FILEOP_WRITE_GAME | FILEOP_WRITE_MPSETUP | FILEOP_WRITE_MPPLAYER | FILEOP_READ_GAME | FILEOP_READ_MPSETUP | FILEOP_READ_MPPLAYER) {
            fm.copybuf.clear();
        }
    }

    /// `filemgr_handle_success` (`filemgr.c:549`): a loaded agent becomes the
    /// boss file's last one and the Perfect Menu opens; a copy's read goes on
    /// to its write.
    fn filemgr_handle_success(&mut self) {
        match self.fmr().fileop as i32 {
            FILEOP_WRITE_GAME | FILEOP_WRITE_MPSETUP | FILEOP_WRITE_MPPLAYER => self.fm().copybuf.clear(),
            FILEOP_LOAD_GAME => {
                self.vars.bossfileid = self.fmr().fileid as i32;
                self.vars.bossdeviceserial = self.fmr().deviceserial as u16;
                self.bossfile_save();
                self.menu_save_and_push_root_dialog(Some(&G_CI_MENU_VIA_PC_MENU_DIALOG), MENUROOT_MAINMENU);
            }
            FILEOP_READ_GAME | FILEOP_READ_MPSETUP | FILEOP_READ_MPPLAYER => {
                let guid = self.filemgr.var800a21e8;
                let op = self.fmr().fileop as i32 - 98;
                let p = self.fmr().mpplayernum as u32;
                self.filemgr_save_or_load(guid, op, p);
            }
            _ => {}
        }
    }

    /// `filemgr_retry_save` (`filemgr.c:756`): 0 on the reinsert dialog's
    /// tick, 1 on its OK, 2 on the error dialog's Try Again.
    fn filemgr_retry_save(&mut self, context: i32) {
        let device = self.paks.pak_find_by_serial(self.fmr().deviceserial as i32);
        if device == -1 {
            if context == 1 {
                self.filemgr_push_error_dialog(FILEERROR_NOPAK);
            }
            if context == 2 {
                self.menu_replace_current_dialog(&G_PAK_NOT_ORIGINAL_MENU_DIALOG);
            }
        } else if self.filemgr_attempt_operation(device, true) != 0 {
            if context == 2 {
                self.fm().device1 = device as u8;
                if fileop_is_save(self.fmr().fileop) {
                    self.filemgr_push_error_dialog(FILEERROR_SAVEFAILED);
                } else {
                    self.filemgr_push_error_dialog(FILEERROR_LOADFAILED);
                }
            } else {
                let serial = self.fmr().deviceserial as i32;
                self.filemgr_set_device1_by_serial(serial);
                if fileop_is_save(self.fmr().fileop) {
                    self.menu_replace_current_dialog(&G_FILEMGR_SAVE_ERROR_MENU_DIALOG);
                } else {
                    self.filemgr_erase_corrupt_file();
                }
            }
        }
    }

    /// `filemgr_attempt_operation` (`filemgr.c:793`): the operation on
    /// `device`; 0 when it worked.
    pub fn filemgr_attempt_operation(&mut self, device: i8, closeonsuccess: bool) -> i32 {
        let mut errno = 0;
        let mut showfilesaved = self.fmr().isretryingsave & 1 != 0;
        let filetypes = [PAKFILETYPE_GAME, PAKFILETYPE_MPSETUP, PAKFILETYPE_MPPLAYER, PAKFILETYPE_CAMERA];
        let (fileid, serial) = (self.fmr().fileid as i32, self.fmr().deviceserial as u16);
        let fileop = self.fmr().fileop as i32;
        match fileop {
            FILEOP_SAVE_GAME_000 | FILEOP_SAVE_GAME_001 | FILEOP_SAVE_GAME_002 => {
                if fileop == FILEOP_SAVE_GAME_002 {
                    showfilesaved = true;
                }
                errno = self.gamefile_save(device, fileid, serial);
            }
            FILEOP_SAVE_MPPLAYER => {
                let p = self.fmr().mpplayernum as usize;
                errno = self.mpplayerfile_save(p, device, fileid, serial);
            }
            FILEOP_SAVE_MPSETUP => {
                errno = self.mpsetupfile_save(device, fileid, serial);
                showfilesaved = true;
            }
            FILEOP_WRITE_GAME | FILEOP_WRITE_MPSETUP | FILEOP_WRITE_MPPLAYER => {
                let mut newfileid = 0;
                let filename = self.fmr().filename;
                let mut body = std::mem::take(&mut self.fm().copybuf);
                savebuffer_cstring_to_bitstring(&mut body, &filename);
                errno = self.paks.pak_save_at_guid(device, fileid, filetypes[(fileop - 6) as usize], &body, Some(&mut newfileid));
                self.fm().copybuf = body;
                self.filelists.var80075bd0[(fileop - 6) as usize] = true;
            }
            FILEOP_LOAD_GAME => errno = self.gamefile_load(device),
            FILEOP_LOAD_MPPLAYER => {
                let p = self.fmr().mpplayernum as usize;
                errno = self.mpplayerfile_load(p, device, fileid, serial);
            }
            FILEOP_LOAD_MPSETUP => errno = self.mpsetupfile_load(device, fileid, serial),
            FILEOP_READ_GAME | FILEOP_READ_MPSETUP | FILEOP_READ_MPPLAYER => {
                let mut body = std::mem::take(&mut self.fm().copybuf);
                errno = self.paks.pak_read_body_at_guid(device, fileid, Some(&mut body), 0);
                self.fm().copybuf = body;
            }
            _ => {}
        }
        if errno == 0 && closeonsuccess {
            self.menu_close_dialog();
        }
        if fileop_is_save(self.fmr().fileop) {
            if errno == 0 {
                self.filemgr_handle_success();
            }
            if showfilesaved && errno == 0 {
                self.menu_push_dialog(&G_FILEMGR_FILE_SAVED_MENU_DIALOG);
            }
        } else if errno == 0 {
            self.filemgr_handle_success();
        }
        self.menu_update_cur_frame();
        errno
    }

    /// `filemgr_save_or_load` (`filemgr.c:920`): save or load file `guid`
    /// (`fileop` -1: the last operation again); `playernum` is the player a
    /// player file is for. On a failure the error dialog opens.
    pub fn filemgr_save_or_load(&mut self, guid: FileGuid, fileop: i32, playernum: u32) -> bool {
        if fileop != -1 {
            let fm = self.fm();
            fm.fileop = fileop as u8;
            fm.mpplayernum = playernum as i32;
            fm.isretryingsave = 0;
            self.filemgr.lastpakerror = 0;
        }
        {
            let fm = self.fm();
            fm.fileid = guid.fileid as u32;
            fm.deviceserial = guid.deviceserial as u32;
        }
        let device = self.paks.pak_find_by_serial(self.fmr().deviceserial as i32);
        if device == -1 {
            self.fm().isretryingsave |= 1;
            self.menu_push_dialog(&G_PAK_NOT_ORIGINAL_MENU_DIALOG);
            return false;
        }
        if self.filemgr_attempt_operation(device, false) != 0 {
            self.fm().isretryingsave |= 1;
            let serial = self.fmr().deviceserial as i32;
            self.filemgr_set_device1_by_serial(serial);
            if fileop_is_save(self.fmr().fileop) {
                self.menu_push_dialog(&G_FILEMGR_SAVE_ERROR_MENU_DIALOG);
            } else {
                self.filemgr_erase_corrupt_file();
            }
            return false;
        }
        true
    }

    /// `filemgr_erase_corrupt_file` (`filemgr.c:665`): a file that won't
    /// load is deleted.
    fn filemgr_erase_corrupt_file(&mut self) {
        let device = self.paks.pak_find_by_serial(self.fmr().deviceserial as i32);
        if device >= 0 {
            let id = self.fmr().fileid as i32;
            self.paks.pak_delete_file(device, id);
        }
        for l in self.filelists.lists.iter_mut().flatten() {
            l.timeuntilupdate = 1;
        }
        self.menu_push_dialog(&G_FILEMGR_FILE_LOST_MENU_DIALOG);
    }

    /// `filemgr_delete_current_file` (`filemgr.c:970`, not JPN): a loaded
    /// player whose file goes is reset to the defaults.
    fn filemgr_delete_current_file(&mut self) {
        let mut error = false;
        let del = self.filemgr.filetodelete;
        let device = self.paks.pak_find_by_serial(del.deviceserial as i32);
        if device >= 0 {
            if self.paks.pak_delete_file(device, del.fileid) != 0 {
                error = true;
            }
        } else {
            error = true;
        }
        let listnum = self.fmr().listnum as usize;
        if let Some(Some(l)) = self.filelists.lists.get_mut(listnum) {
            l.timeuntilupdate = 1;
        }
        if error {
            self.fm().device1 = device as u8;
            self.filemgr_push_error_dialog(FILEERROR_DELETEFAILED);
        } else {
            for i in 0..4 {
                if del.fileid == self.mp.players[i].fileguid.fileid && del.deviceserial == self.mp.players[i].fileguid.deviceserial {
                    self.mp_player_set_defaults(i, true);
                }
            }
        }
    }

    /// `func0f1097d0` (`filemgr.c:1246`): a copy: read the file into memory,
    /// then (on success) write it to `device`'s free file.
    fn func0f1097d0(&mut self, device: i8) {
        if let Some(l) = self.filelist(0) {
            self.filemgr.var800a21e8 = l.deviceguids[device as usize];
            let size = FILE_TYPE_SIZES[(self.fmr().filetypeplusone as usize).saturating_sub(1).min(3)];
            let guid = self.filemgr.filetocopy;
            let op = self.fmr().filetypeplusone as i32 + 103;
            self.fm().copybuf = vec![0; (size + 15) & !15];
            self.filemgr_save_or_load(guid, op, 0);
            let t = (self.fmr().filetypeplusone as usize).saturating_sub(1).min(3);
            self.filelists.var80075bd0[t] = true;
        }
    }

    /// `filemgr_save_game_to_device` (`filemgr.c:1275`): a new agent into
    /// `device`'s free file.
    fn filemgr_save_game_to_device(&mut self, device: i8) {
        if let Some(l) = self.filelist(0) {
            self.gamefileguid = l.deviceguids[device as usize];
            let guid = self.gamefileguid;
            self.filemgr_save_or_load(guid, FILEOP_SAVE_GAME_000, 0);
        }
    }

    /// `filemgr_get_file_name` (`filemgr.c:1286`).
    fn filemgr_get_file_name(&self, file: &FileListFile) -> String {
        match self.cur_filelist().map(|l| l.filetype as i32) {
            Some(FILETYPE_GAME) | Some(FILETYPE_MPSETUP) => {
                let mut c = [0u8; 12];
                savebuffer_bitstring_to_cstring(&file.name, &mut c, false);
                cstr_to_string(&c)
            }
            Some(FILETYPE_MPPLAYER) => MenuSystem::mpplayerfile_get_overview(&file.name).0,
            _ => String::new(),
        }
    }

    /// `filemgr_get_rename_name` (`filemgr.c:1310`): the name of what is
    /// being saved.
    fn filemgr_get_rename_name(&self) -> String {
        match self.fmr().unke3e {
            0 | 9 | 10 | 11 => cstr_to_string(&self.gamefile.name),
            1..=4 | 15..=17 => cstr_to_string(&self.fmr().filename),
            6 | 12 => self.mp.players[self.mpplayernum].base.name.replace('\n', ""),
            7 | 13 => self.mp.setup.name.clone(),
            _ => String::new(),
        }
    }

    /// `filemgr_set_rename_name` (`filemgr.c:1356`).
    fn filemgr_set_rename_name(&mut self, name: &str) {
        match self.fmr().unke3e {
            0 | 9 | 10 | 11 => self.gamefile.name = cstr::<11>(name),
            1..=4 | 15..=17 => self.fm().filename = cstr::<16>(name),
            6 | 12 => {
                let p = self.mpplayernum;
                self.mp.players[p].base.name = format!("{name}\n");
            }
            7 | 13 => self.mp.setup.name = name.to_string(),
            _ => {}
        }
    }

    /// `filemgr_is_name_available` (`filemgr.c:1389`): no file on `device`
    /// has this name (ignoring case).
    fn filemgr_is_name_available(&self, device: i8) -> bool {
        let lookup = [1usize, 2, 3, 4, 0];
        let deviceindex = lookup[device as usize];
        let Some(filelist) = self.cur_filelist() else { return true };
        let startindex = filelist.devicestartindexes[deviceindex];
        if startindex == -1 {
            return true;
        }
        let mut endindex = filelist.numfiles();
        for i in (deviceindex + 1..=4).rev() {
            if filelist.devicestartindexes[i] != -1 {
                endindex = filelist.devicestartindexes[i] as i32;
            }
        }
        let upper = |s: String| -> String { s.split('\n').next().unwrap_or("").to_ascii_uppercase() };
        let findname = upper(self.filemgr_get_rename_name());
        for i in startindex as i32..endindex {
            let loopname = upper(self.filemgr_get_file_name(&filelist.files[i as usize]));
            if findname == loopname {
                return false;
            }
        }
        true
    }

    /// `filemgr_save_to_device` (`filemgr.c:1485`): a new file (or a copy)
    /// onto `device2`, unless its name is taken there.
    fn filemgr_save_to_device(&mut self) {
        let device2 = self.fmr().device2 as i8;
        let spaces = self.cur_filelist().map_or(0, |l| l.spacesfree[device2 as usize]);
        if spaces > 0 {
            if !self.filemgr_is_name_available(device2) {
                self.menu_push_dialog(&G_FILEMGR_DUPLICATE_NAME_MENU_DIALOG);
            } else {
                self.menu_pop_dialog();
                let unke3e = self.fmr().unke3e;
                let guid = self.cur_filelist().map_or(FileGuid::default(), |l| l.deviceguids[device2 as usize]);
                if unke3e == 0 {
                    self.filemgr_save_game_to_device(device2);
                } else if unke3e == 5 {
                    // empty
                } else if unke3e == 6 {
                    let p = self.mpplayernum as u32;
                    self.filemgr_save_or_load(guid, FILEOP_SAVE_MPPLAYER, p);
                } else if unke3e == 7 {
                    self.filemgr_save_or_load(guid, FILEOP_SAVE_MPSETUP, 0);
                } else if unke3e >= 9 {
                    self.filemgr_save_or_load(guid, -1, 0);
                } else {
                    self.func0f1097d0(device2);
                }
            }
        }
    }

    /// `filemgr_push_select_location_dialog` (`filemgr.c:1817`).
    pub fn filemgr_push_select_location_dialog(&mut self, arg0: i32, filetype: i32) {
        self.fm().unke3e = arg0 as u8;
        let l = self.filelist_find_or_create(filetype as u8);
        self.fm().listnum = l as u8;
        self.filelists_tick();
        self.menu_push_dialog(&G_FILEMGR_SELECT_LOCATION_MENU_DIALOG);
    }

    /// `filemgr_is_file_in_use` (`filemgr.c:1970`, not JPN): the loaded agent,
    /// setup or a joined player's file (not in the agent select, where none
    /// is loaded yet), or the file being copied.
    fn filemgr_is_file_in_use(&self, file: &FileListFile) -> bool {
        if self.menu_is_dialog_open(&G_FILEMGR_COPY_MENU_DIALOG) && file.fileid == self.filemgr.filetocopy.fileid && file.deviceserial == self.filemgr.filetocopy.deviceserial {
            return true;
        }
        // (g_FilemgrFileSelect4MbMenuDialog: the 4 MB menus aren't here.)
        if self.menudata.root == MENUROOT_FILEMGR {
            return false;
        }
        if file.fileid == self.gamefileguid.fileid && file.deviceserial == self.gamefileguid.deviceserial {
            return true;
        }
        if file.fileid == self.mp.setup.fileguid.fileid && file.deviceserial == self.mp.setup.fileguid.deviceserial {
            return true;
        }
        for i in 0..4 {
            if self.mp.setup.chrslots & (1 << i) != 0 && self.mp.players[i].fileguid.fileid == file.fileid && self.mp.players[i].fileguid.deviceserial == file.deviceserial {
                return true;
            }
        }
        false
    }

    /// `filemgr_push_delete_file_dialog` (`filemgr.c:2162`).
    fn filemgr_push_delete_file_dialog(&mut self, listnum: u8) {
        let ft = self.filelist(listnum as usize).map(|l| l.filetype as u32 + 1);
        let fm = self.fm();
        fm.listnum = listnum;
        fm.isdeletingforsave = false;
        fm.filetypeplusone = ft.unwrap_or(1);
        self.menu_push_dialog(&G_FILEMGR_DELETE_MENU_DIALOG);
    }

    /// `filemgr_consider_pushing_file_select_dialog` (`filemgr.c:2799`): the
    /// agent select, as power on opens it.
    pub fn filemgr_consider_pushing_file_select_dialog(&mut self) -> bool {
        if self.mr().openinhibit == 0 {
            self.m().playernum = 0;
            self.menu_push_root_dialog(&G_FILEMGR_FILE_SELECT_MENU_DIALOG, MENUROOT_FILEMGR);
            return true;
        }
        false
    }

    /// `menu_is_dialog_open` (`menu.c:6053`): `def` is on the current
    /// player's stack.
    pub fn menu_is_dialog_open(&self, def: &'static MenuDialogDef) -> bool {
        let m = self.mr();
        if m.curdialog.is_some() {
            for i in 0..m.depth {
                for j in 0..m.layers[i].numsiblings as usize {
                    if std::ptr::eq(m.dialogs[m.layers[i].siblings[j]].def(), def) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The agent select's picture of solo stage `stage` (0: a new agent),
    /// `TEX_GENERAL_NEWAGENT + stage`, drawn flipped as PD's `t = 1152,
    /// dtdy = -1` draws it (`filemgr.c:2649`).
    fn draw_agent_picture(&mut self, x: i32, y: i32, stage: usize, colour: u32) {
        let us = self.draw.gfx.uiscale;
        let env = 0xffffff00 | (colour & 0xff);
        let Some(tex) = self.draw.res.agent_pictures.get(stage) else { return };
        self.draw.gfx.tex_rect((x + 4) * us, y + 2, (x + 60) * us, y + 38, tex, 0.0, 36.0, 1.0 / us as f32, -1.0, n64::rdp::Cc::TexEnv, n64::rdp::rgba(env), n64::rdp::Filter::Point);
    }
}

// ---------------------------------------------------------------------------
// The tables' handlers and texts (filemgr.c, in its order)
// ---------------------------------------------------------------------------

/// `filemgr_device_name_menu_handler` (`filemgr.c:170`).
pub fn filemgr_device_name_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && (pd.fmr().device1 & 0x7f) as i8 >= SAVEDEVICE_INVALID {
        return HRet::I(1);
    }
    ok()
}

/// `filemgr_menu_text_device_name` (`filemgr.c:181`).
pub fn filemgr_menu_text_device_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    pd.filemgr_get_device_name((pd.fmr().device1 & 0x7f) as i32).unwrap_or_default()
}

/// `filemgr_file_name_menu_handler` (`filemgr.c:238`).
pub fn filemgr_file_name_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && pd.fmr().filetodelete.is_none() {
        return HRet::I(1);
    }
    ok()
}

/// `filemgr_menu_text_delete_file_name` (`filemgr.c:249`).
pub fn filemgr_menu_text_delete_file_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    match pd.fmr().filetodelete {
        Some(f) => pd.filemgr_get_select_name(&f, pd.fmr().filetypetodelete as i32),
        None => String::new(),
    }
}

/// `filemgr_menu_text_fail_reason` (`filemgr.c:288`).
pub fn filemgr_menu_text_fail_reason(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let reasons = [322u16, 323, 324, 325, 326, 327, 328, 329, 330];
    pd.lang(tx(B_OPTIONS, reasons[(pd.fmr().errno as usize).min(8)]))
}

/// `filemgr_device_name_for_error_menu_handler` (`filemgr.c:314`).
pub fn filemgr_device_name_for_error_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN {
        if (pd.fmr().device1 & 0x7f) as i8 >= SAVEDEVICE_INVALID {
            return HRet::I(1);
        }
        match pd.fmr().errno {
            FILEERROR_SAVEFAILED | FILEERROR_LOADFAILED | FILEERROR_DELETEFAILED | FILEERROR_PAKREMOVED | FILEERROR_DELETENOTEFAILED => {}
            FILEERROR_OUTOFMEMORY | FILEERROR_ALREADYLOADED | FILEERROR_PAKDAMAGED => return HRet::I(1),
            _ => return HRet::I(1),
        }
    }
    HRet::I(0)
}

/// `filemgr_menu_text_device_name_for_error` (`filemgr.c:339`): the device,
/// its line break turned into a colon (bar a pak that was removed).
pub fn filemgr_menu_text_device_name_for_error(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let mut s = pd.filemgr_get_device_name((pd.fmr().device1 & 0x7f) as i32).unwrap_or_default();
    if pd.fmr().errno != FILEERROR_PAKREMOVED {
        s.pop();
        s.push_str(":\n");
    }
    s
}

/// `filemgr_menu_text_error_title` (`filemgr.c:448`), as a dialog title.
pub fn title_filemgr_menu_text_error_title(pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    let messages = [331u16, 332, 333, 334, 335, 336, 337, 338, 339];
    let i = match pd.fmr().fileop as i32 {
        FILEOP_LOAD_GAME | FILEOP_LOAD_MPSETUP => 0,
        FILEOP_SAVE_GAME_000 | FILEOP_SAVE_GAME_001 | FILEOP_SAVE_GAME_002 | FILEOP_SAVE_MPSETUP => 1,
        FILEOP_LOAD_MPPLAYER => 2,
        FILEOP_SAVE_MPPLAYER => 3,
        FILEOP_READ_GAME | FILEOP_READ_MPSETUP | FILEOP_READ_MPPLAYER => 6,
        FILEOP_WRITE_GAME | FILEOP_WRITE_MPSETUP | FILEOP_WRITE_MPPLAYER => 7,
        _ => 8,
    };
    pd.lang(tx(B_OPTIONS, messages[i]))
}

/// `filemgr_menu_text_file_type` (`filemgr.c:489`).
pub fn filemgr_menu_text_file_type(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let names = [103u16, 104, 105, 106];
    let i = match pd.fmr().fileop as i32 {
        FILEOP_SAVE_MPSETUP | FILEOP_WRITE_MPSETUP | FILEOP_LOAD_MPSETUP | FILEOP_READ_MPSETUP => 1,
        FILEOP_SAVE_MPPLAYER | FILEOP_WRITE_MPPLAYER | FILEOP_LOAD_MPPLAYER | FILEOP_READ_MPPLAYER => 2,
        _ => 0,
    };
    pd.lang(tx(B_OPTIONS, names[i]))
}

/// `filemgr_retry_save_menu_handler` (`filemgr.c:591`).
pub fn filemgr_retry_save_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.filemgr_retry_save(2);
    }
    ok()
}

/// `filemgr_save_elsewhere_yes_menu_handler` (`filemgr.c:600`).
pub fn filemgr_save_elsewhere_yes_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_close_dialog();
        let fileop = pd.fmr().fileop as i32;
        let filetype = match fileop {
            FILEOP_SAVE_GAME_000 | FILEOP_SAVE_GAME_001 | FILEOP_SAVE_GAME_002 | FILEOP_WRITE_GAME => FILETYPE_GAME,
            FILEOP_SAVE_MPPLAYER | FILEOP_WRITE_MPPLAYER => FILETYPE_MPPLAYER,
            FILEOP_SAVE_MPSETUP | FILEOP_WRITE_MPSETUP => FILETYPE_MPSETUP,
            // (PD's filetype is unset for the other operations.)
            _ => FILETYPE_GAME,
        };
        pd.filemgr_push_select_location_dialog(fileop + 9, filetype);
    }
    ok()
}

/// `filemgr_cancel_save2_menu_handler` (`filemgr.c:630`).
pub fn filemgr_cancel_save2_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_close_dialog();
        pd.func0f10898c();
        pd.menu_update_cur_frame();
    }
    ok()
}

/// `filemgr_acknowledge_file_lost_menu_handler` (`filemgr.c:653`).
pub fn filemgr_acknowledge_file_lost_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_close_dialog();
        pd.func0f10898c();
        pd.menu_update_cur_frame();
    }
    ok()
}

/// `filemgr_insert_original_pak_menu_dialog` (`filemgr.c:686`): the save
/// retried every tick while the pak is missing.
pub fn filemgr_insert_original_pak_menu_dialog(pd: &mut MenuSystem, op: i32, def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_TICK && pd.cur_def().is_some_and(|d| std::ptr::eq(d, def)) {
        pd.filemgr_retry_save(0);
    }
    0
}

/// `filemgr_reinserted_ok_menu_handler` (`filemgr.c:698`).
pub fn filemgr_reinserted_ok_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        // (pak_execute_debug_operations: nothing for the Game Pak.)
        pd.filemgr_retry_save(1);
    }
    ok()
}

/// `filemgr_reinserted_cancel_menu_handler` (`filemgr.c:708`).
pub fn filemgr_reinserted_cancel_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let fileop = pd.fmr().fileop;
        if fileop_is_save(fileop) && fileop as i32 != FILEOP_SAVE_GAME_001 {
            pd.menu_replace_current_dialog(&G_FILEMGR_SAVE_ELSEWHERE_MENU_DIALOG);
        } else {
            pd.menu_pop_dialog();
        }
    }
    ok()
}

/// `filemgr_menu_text_insert_original_pak` (`filemgr.c:722`): "Please
/// insert the Controller Pak containing your ... into any controller.",
/// wrapped to 120.
pub fn filemgr_menu_text_insert_original_pak(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let name = filemgr_menu_text_file_type(pd, item);
    let name = name.split('\n').next().unwrap_or("").to_string();
    let full = pd.lang(tx(B_OPTIONS, 363)).replacen("%s", &name, 1);
    pd_core::text::wrap(120, &full, pd.draw.res.fonts.get(FontId::Sm))
}

/// `filemgr_confirm_rename_menu_handler` (`filemgr.c:1521`).
pub fn filemgr_confirm_rename_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_KEYBOARD_STRING => data.string = cstr::<11>(&pd.filemgr_get_rename_name()),
        MENUOP_SET_KEYBOARD_STRING => pd.filemgr_set_rename_name(&cstr_to_string(&data.string)),
        MENUOP_CONFIRM => pd.filemgr_save_to_device(),
        _ => {}
    }
    ok()
}

/// `filemgr_duplicate_rename_menu_handler` (`filemgr.c:1542`).
pub fn filemgr_duplicate_rename_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
        pd.menu_push_dialog(&G_FILEMGR_RENAME_MENU_DIALOG);
    }
    ok()
}

/// `filemgr_duplicate_cancel_menu_handler` (`filemgr.c:1554`).
pub fn filemgr_duplicate_cancel_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
        pd.menu_pop_dialog();
    }
    ok()
}

/// `filemgr_menu_text_device_name_containing_duplicate_file` (`filemgr.c:1566`).
pub fn filemgr_menu_text_device_name_containing_duplicate_file(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    pd.filemgr_get_device_name(pd.fmr().device2 as i32).unwrap_or_default()
}

/// `filemgr_menu_text_duplicate_file_name` (`filemgr.c:1573`).
pub fn filemgr_menu_text_duplicate_file_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    format!("{}\n", pd.filemgr_get_rename_name())
}

/// `filemgr_menu_text_location_name2` (`filemgr.c:1686`): the device's name,
/// blank when it can't be read.
pub fn filemgr_menu_text_location_name2(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let names = [tx(B_OPTIONS, 112), tx(B_OPTIONS, 113), tx(B_OPTIONS, 114), tx(B_OPTIONS, 115), tx(B_OPTIONS, 111), tx(B_OPTIONS, 4)];
    let Some(l) = pd.cur_filelist() else { return String::new() };
    let d = item.param.clamp(0, 4) as usize;
    if l.spacesfree[d] < 0 {
        return pd.lang(names[5]);
    }
    pd.lang(names[d])
}

/// `filemgr_menu_text_save_location_spaces` (`filemgr.c:1714`).
pub fn filemgr_menu_text_save_location_spaces(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let Some(l) = pd.cur_filelist() else { return String::new() };
    let spacesfree = l.spacesfree[item.param.clamp(0, 4) as usize];
    if spacesfree < 0 {
        return "\n".into();
    }
    if spacesfree == 0 {
        return pd.lang(tx(B_OPTIONS, 372));
    }
    spacesfree.to_string()
}

/// `filemgr_select_location_menu_handler` (`filemgr.c:1751`, NTSC 1.0+):
/// `item.param` is the device.
pub fn filemgr_select_location_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    let Some(l) = pd.cur_filelist() else { return ok() };
    if op == MENUOP_IS_DISABLED && l.spacesfree[item.param.clamp(0, 4) as usize] < 1 {
        return HRet::I(1);
    }
    if op == MENUOP_CONFIRM {
        pd.fm().device2 = item.param as u8;
        pd.filemgr_save_to_device();
    }
    ok()
}

/// `filemgr_cancel_save_menu_handler` (`filemgr.c:1798`).
pub fn filemgr_cancel_save_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
    }
    ok()
}

/// `filemgr_delete_files_for_save_menu_handler` (`filemgr.c:1807`).
pub fn filemgr_delete_files_for_save_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let listnum = pd.fmr().listnum;
        pd.filemgr_push_delete_file_dialog(listnum);
        pd.fm().isdeletingforsave = true;
    }
    ok()
}

/// `filemgr_confirm_delete_menu_handler` (`filemgr.c:1879`, not JPN).
pub fn filemgr_confirm_delete_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
        pd.filemgr_delete_current_file();
    }
    ok()
}

/// `filemgr_menu_text_file_in_use_description` (`filemgr.c:1904`).
pub fn filemgr_menu_text_file_in_use_description(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    if pd.menu_is_dialog_open(&G_FILEMGR_COPY_MENU_DIALOG) {
        return pd.lang(tx(B_MPWEAPONS, 240));
    }
    pd.lang(tx(B_MPWEAPONS, 160))
}

/// `filemgr_file_to_copy_or_delete_list_menu_handler` (`filemgr.c:2020`):
/// the files of list 0 (copying) or of the current list (deleting), a file
/// in use drawn red when deleting.
fn filemgr_file_to_copy_or_delete_list_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData, isdelete: bool) -> R {
    let listnum = if item.param == 1 { pd.fmr().listnum as usize } else { 0 };
    let Some(list) = pd.filelist(listnum).cloned() else { return ok() };
    match op {
        MENUOP_GET_SELECTED_INDEX => data.value = 0x0fffff,
        MENUOP_GET_OPTION_COUNT => data.value = list.numfiles(),
        MENUOP_RENDER => {
            let Some(rd) = data.render else { return ok() };
            if pd.fmr().filetypeplusone == 4 {
                // (A PerfectHead's thumbnail: there are none here.)
                return ok();
            }
            let Some(file) = list.files.get(data.unk04.max(0) as usize).copied() else { return ok() };
            let mut colour = rd.colour;
            if isdelete && pd.filemgr_is_file_in_use(&file) {
                colour = 0xff333300 | (colour & 0xff);
            }
            let (mut x, mut y) = (rd.x + 2, rd.y + 2);
            let text = pd.filemgr_get_select_name(&file, pd.fmr().filetypeplusone as i32 - 1);
            let (vw, vh) = (pd.draw.gfx.w as i32, pd.draw.gfx.h as i32);
            pd.tc().render_v2(&mut x, &mut y, &text, FontId::Sm, colour, vw, vh, 0, 1);
        }
        MENUOP_GET_OPTION_HEIGHT => data.value = 11,
        MENUOP_GET_OPTGROUP_COUNT => data.value = list.numdevices as i32,
        MENUOP_GET_OPTGROUP_TEXT => return pd.filemgr_group_text(listnum, data.value),
        MENUOP_GET_OPTGROUP_START_INDEX => {
            data.groupstartindex = pd.filemgr_group_start(listnum, data.value);
            return ok();
        }
        _ => {}
    }
    ok()
}

/// `filemgr_file_to_delete_list_menu_handler` (`filemgr.c:2093`).
pub fn filemgr_file_to_delete_list_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let Some(list) = pd.cur_filelist().cloned() else { return ok() };
    if op == MENUOP_CONFIRM {
        if let Some(file) = list.files.get(data.value.max(0) as usize).copied() {
            let ft = list.filetype;
            if pd.filemgr_is_file_in_use(&file) {
                pd.filemgr_set_file_to_delete(file, ft);
                pd.menu_push_dialog(&G_FILEMGR_FILE_IN_USE_MENU_DIALOG);
            } else {
                pd.filemgr_set_file_to_delete(file, ft);
                pd.filemgr.filetodelete = FileGuid { fileid: file.fileid, deviceserial: file.deviceserial };
                pd.menu_push_dialog(&G_FILEMGR_CONFIRM_DELETE_MENU_DIALOG);
            }
        }
    }
    filemgr_file_to_copy_or_delete_list_menu_handler(pd, op, item, data, true)
}

/// `filemgr_file_to_copy_list_menu_handler` (`filemgr.c:2123`).
pub fn filemgr_file_to_copy_list_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let Some(list) = pd.filelist(0).cloned() else { return ok() };
    if op == MENUOP_CONFIRM {
        if let Some(file) = list.files.get(data.value.max(0) as usize).copied() {
            pd.filemgr.filetocopy = FileGuid { fileid: file.fileid, deviceserial: file.deviceserial };
            let name = pd.filemgr_get_file_name(&file);
            pd.fm().filename = cstr::<16>(&name);
            let ft = pd.fmr().filetypeplusone as i32;
            pd.filemgr_push_select_location_dialog(ft, ft - 1);
        }
    }
    filemgr_file_to_copy_or_delete_list_menu_handler(pd, op, item, data, false)
}

/// `filemgr_copy_or_delete_list_menu_dialog` (`filemgr.c:2148`).
pub fn filemgr_copy_or_delete_list_menu_dialog(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_CLOSE {
        if pd.fmr().isdeletingforsave {
            pd.fm().isdeletingforsave = false;
        } else {
            pd.filelist_create(0, FILETYPE_GAME as u8);
            pd.fm().filetypeplusone = 0;
        }
    }
    0
}

/// `filemgr_open_copy_file_menu_handler` (`filemgr.c:2483`).
pub fn filemgr_open_copy_file_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.fm().filetypeplusone = item.param as u32 + 1;
        pd.filelist_create(0, item.param as u8);
        let fm = pd.fm();
        fm.listnum = 0;
        fm.isdeletingforsave = false;
        pd.menu_push_dialog(&G_FILEMGR_COPY_MENU_DIALOG);
    }
    ok()
}

/// `filemgr_open_delete_file_menu_handler` (`filemgr.c:2500`).
pub fn filemgr_open_delete_file_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.fm().filetypeplusone = item.param as u32 + 1;
        pd.filelist_create(0, item.param as u8);
        pd.fm().unke3e = 0xff;
        pd.filemgr_push_delete_file_dialog(0);
    }
    ok()
}

/// `filemgr_agent_name_keyboard_menu_handler` (`filemgr.c:2512`).
pub fn filemgr_agent_name_keyboard_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    if pd.filelist(0).is_none() {
        return ok();
    }
    match op {
        MENUOP_GET_KEYBOARD_STRING => data.string = pd.gamefile.name,
        MENUOP_SET_KEYBOARD_STRING => pd.gamefile.name = data.string,
        MENUOP_CONFIRM => {
            pd.filemgr_push_select_location_dialog(0, FILETYPE_GAME);
            pd.fm().unke2c = 1;
        }
        _ => {}
    }
    ok()
}

/// `filemgr_choose_agent_list_menu_handler` (`filemgr.c:2536`): the agents
/// on each device (with their picture, stage and mission time), then "New
/// Agent...".
pub fn filemgr_choose_agent_list_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let Some(list) = pd.filelist(0).cloned() else { return ok() };
    let numfiles = list.numfiles();
    match op {
        MENUOP_GET_SELECTED_INDEX => data.value = 0xfffff,
        MENUOP_GET_OPTION_INDEX2 => {
            let mut pass = false;
            if data.unk04 == 1 {
                if data.groupstartindex == 1 && pd.fmr().unke2c == 1 {
                    for (i, f) in list.files.iter().enumerate() {
                        if pd.gamefileguid.fileid == f.fileid && pd.gamefileguid.deviceserial == f.deviceserial {
                            data.value = i as i32;
                        }
                    }
                    pd.fm().unke2c = 0;
                }
                if pd.filelist(0).is_some_and(|l| l.updatedthisframe) {
                    pass = true;
                }
            } else {
                pass = true;
                pd.fm().unke2c = 0;
            }
            if pass && pd.vars.bossfileid != 0 {
                for (j, f) in list.files.iter().enumerate() {
                    if pd.vars.bossfileid == f.fileid && pd.vars.bossdeviceserial == f.deviceserial {
                        data.value = j as i32;
                        pd.vars.bossfileid = 0;
                    }
                }
            }
        }
        MENUOP_GET_OPTION_COUNT => data.value = numfiles + 1,
        MENUOP_RENDER => {
            let Some(rd) = data.render else { return ok() };
            let mut file = None;
            let (mut stage, mut seconds, mut minutes, mut hours, mut days) = (0u8, 0u32, 0u32, 0u32, 0u32);
            let mut name = String::new();
            if data.unk04 != numfiles {
                file = list.files.get(data.unk04.max(0) as usize).copied();
                if let Some(f) = file {
                    let (n, s, mut difficulty, mut time) = MenuSystem::gamefile_get_overview(&f.name);
                    name = n;
                    stage = s;
                    seconds = time % 60;
                    time /= 60;
                    if stage as i32 > SOLOSTAGEINDEX_SKEDARRUINS + 1 {
                        stage = (SOLOSTAGEINDEX_SKEDARRUINS + 1) as u8;
                    }
                    if difficulty as i32 > DIFF_PA {
                        difficulty = DIFF_PA as u8;
                    }
                    let _ = difficulty;
                    days = time / 1440;
                    hours = (time - days * 1440) / 60;
                    minutes = (time - days * 1440) - hours * 60;
                }
            }
            pd.draw_agent_picture(rd.x, rd.y, stage as usize, rd.colour);
            let (vw, vh) = (pd.draw.gfx.w as i32, pd.draw.gfx.h as i32);
            let (mut x, mut y) = (rd.x + 62, rd.y + 4);
            if data.unk04 == numfiles {
                let t = pd.lang(tx(B_OPTIONS, 403));
                pd.tc().render_v2(&mut x, &mut y, &t, FontId::Md, rd.colour, vw, vh, 0, 0);
            } else if file.is_some() {
                pd.tc().render_v2(&mut x, &mut y, &name, FontId::Md, rd.colour, vw, vh, 0, 1);
                y = rd.y + 18;
                x = rd.x + 62;
                let mut buffer = if stage > 0 {
                    let (n1, n2) = SOLO_STAGE_NAMES[stage as usize - 1];
                    format!("{} {}", pd.lang(n1), pd.lang(n2))
                } else {
                    pd.lang(tx(B_OPTIONS, 404))
                };
                buffer.push('\n');
                pd.tc().render_v2(&mut x, &mut y, &buffer, FontId::Sm, rd.colour, vw, vh, 0, 0);
                x = rd.x + 62;
                y += 1;
                let label = pd.lang(tx(B_OPTIONS, 405));
                let buffer = if days > 0 { format!("{label} {days}:{hours:02}:{minutes:02}") } else { format!("{label} {hours:02}:{minutes:02}") };
                pd.tc().render_v2(&mut x, &mut y, &buffer, FontId::Sm, rd.colour, vw, vh, 0, 0);
                y += 1;
                x += 1;
                let buffer = format!(".{seconds:02}");
                pd.tc().render_v2(&mut x, &mut y, &buffer, FontId::Xs, rd.colour, vw, vh, 0, 0);
            }
        }
        MENUOP_GET_OPTION_HEIGHT => data.value = 40,
        MENUOP_CONFIRM => {
            if data.value == numfiles {
                pd.gamefile_load_defaults();
                pd.menu_push_dialog(&G_FILEMGR_ENTER_NAME_MENU_DIALOG);
            } else if let Some(f) = list.files.get(data.value.max(0) as usize).copied() {
                pd.gamefileguid = FileGuid { fileid: f.fileid, deviceserial: f.deviceserial };
                let guid = pd.gamefileguid;
                pd.filemgr_save_or_load(guid, FILEOP_LOAD_GAME, 0);
            }
        }
        MENUOP_GET_OPTGROUP_COUNT => data.value = list.numdevices as i32 + 1,
        MENUOP_GET_OPTGROUP_TEXT => {
            if data.value >= list.numdevices as i32 {
                return pd.lang(tx(B_OPTIONS, 402)).into();
            }
            return pd.filemgr_group_text(0, data.value);
        }
        MENUOP_GET_OPTGROUP_START_INDEX => {
            data.groupstartindex = if data.value >= list.numdevices as i32 { numfiles } else { pd.filemgr_group_start(0, data.value) };
            return ok();
        }
        _ => {}
    }
    ok()
}

/// `filemgr_main_menu_dialog` (`filemgr.c:2768`): the agent select opens
/// on a fresh Combat Simulator setup and the agents' list.
pub fn filemgr_main_menu_dialog(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    match op {
        MENUOP_ON_OPEN => {
            pd.fm().filetypeplusone = 0;
            pd.filelist_create(0, FILETYPE_GAME as u8);
            pd.mp_init();
            for i in 0..4 {
                if pd.mp.players[i].base.name.is_empty() {
                    pd.mp.players[i].base.name = format!("{} {}\n", pd.lang(tx(B_MISC, 437)), i + 1);
                }
            }
        }
        MENUOP_ON_CLOSE => pd.filelists_free(),
        _ => {}
    }
    0
}

/// `menuhandler_change_agent` (`mainmenu.c:2243`): "Change Agent...", Yes.
pub fn menuhandler_change_agent(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_save_and_push_root_dialog(None, MENUROOT_CHANGE_AGENT);
    }
    ok()
}

impl MenuSystem {
    /// The agent select as power on opens it at CI (`menu_tick`,
    /// `menutick.c:270`: `g_FileState` unselected, `player_pause(MENUROOT_FILEMGR)`).
    pub fn open_file_select(&mut self) {
        self.mpplayernum = 0;
        for m in self.menus.iter_mut() {
            m.openinhibit = 0;
        }
        self.filemgr_consider_pushing_file_select_dialog();
        self.filestate = FILESTATE_SELECTED;
        self.music_start_menu();
    }

    /// SUBST: PD always opens the agent select at power on / a shortcut
    /// into the Combat Simulator (`perfect_dark --combat`, the headless
    /// flows) loads an agent without it: the one the boss file names, else
    /// the Game Pak's first, else a new "Dark" saved to the Game Pak, as the
    /// agent select's New Agent would. Returns the agent's name.
    pub fn load_agent_without_select(&mut self) -> String {
        self.filelist_create(0, FILETYPE_GAME as u8);
        self.filelists_tick();
        let list = self.filelists.lists[0].clone().unwrap_or_default();
        let boss = FileGuid { fileid: self.vars.bossfileid, deviceserial: self.vars.bossdeviceserial };
        let pick = list.files.iter().find(|f| f.fileid == boss.fileid && f.deviceserial == boss.deviceserial).or(list.files.first()).copied();
        let mut loaded = false;
        if let Some(f) = pick {
            self.gamefileguid = FileGuid { fileid: f.fileid, deviceserial: f.deviceserial };
            loaded = self.gamefile_load(SAVEDEVICE_GAMEPAK) == 0;
        }
        if !loaded {
            self.gamefile_load_defaults();
            let guid = list.deviceguids[SAVEDEVICE_GAMEPAK as usize];
            if self.gamefile_save(SAVEDEVICE_GAMEPAK, guid.fileid, guid.deviceserial) != 0 {
                log::warn!("pd_menu: the Game Pak has no room for a new agent");
            }
        }
        self.vars.bossfileid = self.gamefileguid.fileid;
        self.vars.bossdeviceserial = self.gamefileguid.deviceserial;
        self.bossfile_save();
        self.filelists_free();
        self.filestate = FILESTATE_SELECTED;
        cstr_to_string(&self.gamefile.name)
    }

    /// `MENUROOT_CHANGE_AGENT` (`menutick.c:566`): the menus stop, the agent
    /// goes back to the defaults and CI reloads, where (`lv.c:2411`) the
    /// explosion plays and the agent select opens again.
    pub fn change_agent(&mut self) {
        self.menu_stop();
        self.gamefile_load_defaults();
        self.gamefile_apply_options();
        self.music(pd_core::music::MusicCall::StopAll);
        self.menu_reset();
        self.draw.res.blur_from_image(None);
        self.mp_set_default_names_if_empty();
        self.menu_play_sound(MENUSOUND_EXPLOSION);
        self.open_file_select();
    }
}

