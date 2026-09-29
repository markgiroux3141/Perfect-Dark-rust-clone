//! `pak.c`'s file system: PD's save files as a list of headed files on a
//! device, as PD keeps them on the cartridge's 2 KB EEPROM (the Game Pak,
//! `SAVEDEVICE_GAMEPAK`) or in a game note on a Controller Pak.
//!
//! Every file is a 16-byte [`PakFileHeader`] (its checksums, type, lengths,
//! the device serial, a file id, a generation and whether it holds data)
//! followed by its body, aligned to the device's block. The files run from
//! offset 0 to a `PAKFILETYPE_TERMINATOR`. A new device gets PD's initial
//! files (`pak_create_initial_files`): two boss files and five each of MP
//! players, MP setups and agents, every one empty. One file of each type
//! stays the swap space: `pak_save_at_guid` writes the new body there and
//! turns the old copy into the next swap, so a save is never half-written.
//!
//! The Controller Paks (devices 0-3) are never plugged in here: PD's code
//! then sees `PAKTYPE_NONE` for them, as on a console with none inserted.
//! The EEPROM's bytes live in [`Paks::eeprom`]; keeping them between runs is
//! the game's (`pd_game` writes the image to a file when [`Paks::written`]).
//!
//! Left out: the header cache (`g_PakDebugPakCache`) only ever holds what
//! was last read from or written to the device, so reading the EEPROM
//! instead gives the same answers; the Controller Pak's note allocation,
//! rumble and Game Boy halves are not reached without one plugged in.

// pak.c's conditions as written (its offset checks, its branches that return
// the same code).
#![allow(clippy::nonminimal_bool, clippy::if_same_then_else)]

use crate::rng::Rng;
use crate::savebuffer::crc_calculate_u16_pair;

pub use crate::ids::{
    PAKFILETYPE_001, PAKFILETYPE_ALL, PAKFILETYPE_BLANK, PAKFILETYPE_BOSS, PAKFILETYPE_CAMERA, PAKFILETYPE_GAME, PAKFILETYPE_MPPLAYER, PAKFILETYPE_MPSETUP, PAKFILETYPE_TERMINATOR, PAKSTATE_07, PAKSTATE_17, PAKSTATE_18, PAKSTATE_22,
    PAKSTATE_MEM_DISPATCH, PAKSTATE_MEM_ENTER_FULL, PAKSTATE_MEM_POST_PREPARE, PAKSTATE_MEM_PREPARE, PAKSTATE_MEM_PRE_PREPARE, PAKSTATE_NOPAK, PAKSTATE_READY, PAKTYPE_MEMORY, PAKTYPE_NONE, PAK_ERR2_BADOFFSET, PAK_ERR2_CHECKSUM,
    PAK_ERR2_CORRUPT, PAK_ERR2_INCOMPLETE, PAK_ERR2_NOPAK, PAK_ERR2_OK, PAK_ERR2_VERSION, SAVEDEVICE_CONTROLLERPAK1, SAVEDEVICE_GAMEPAK, SAVEDEVICE_INVALID,
};

// `PAK_ERR1_*` (`constants.h:3392`) are libultra's PFS codes and the
// EEPROM's own.
pub const PAK_ERR1_OK: i32 = 0;
/// `PFS_ERR_NOPACK`.
pub const PAK_ERR1_NOPAK: i32 = 1;
pub const PAK_ERR1_EEPROMMISSING: i32 = 0x80;
pub const PAK_ERR1_EEPROMREADFAILED: i32 = 0x81;
pub const PAK_ERR1_EEPROMWRITEFAILED: i32 = 0x82;
pub const PAK_ERR1_EEPROMINVALIDOP: i32 = 0x83;

/// The EEPROM's size (`pak_query_note_state`'s `file_size`, `pak.c:1227`).
pub const EEPROM_SIZE: usize = 0x800;

const OS_READ: u8 = 0;
const OS_WRITE: u8 = 1;

/// `struct pakfileheader` (`types.h:5294`): 16 bytes, big-endian, the
/// bitfields MSB first as IDO packs them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PakFileHeader {
    /// Checksum of the header from `filetype` to its end.
    pub headersum: [u16; 2],
    pub bodysum: [u16; 2],
    /// `PAKFILETYPE_*` (9 bits).
    pub filetype: u32,
    /// Not aligned (11 bits).
    pub bodylen: u32,
    /// Aligned to the block (12 bits).
    pub filelen: u32,
    /// 13 bits.
    pub deviceserial: u32,
    /// 7 bits.
    pub fileid: u32,
    /// +1 each time the file is saved (9 bits).
    pub generation: u32,
    pub occupied: bool,
    /// 0 while the body is written, then 1.
    pub writecompleted: bool,
    /// 0 unless `-forceversion`.
    pub version: bool,
}

impl PakFileHeader {
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..2].copy_from_slice(&self.headersum[0].to_be_bytes());
        b[2..4].copy_from_slice(&self.headersum[1].to_be_bytes());
        b[4..6].copy_from_slice(&self.bodysum[0].to_be_bytes());
        b[6..8].copy_from_slice(&self.bodysum[1].to_be_bytes());
        let w0 = (self.filetype & 0x1ff) << 23 | (self.bodylen & 0x7ff) << 12 | (self.filelen & 0xfff);
        let w1 = (self.deviceserial & 0x1fff) << 19
            | (self.fileid & 0x7f) << 12
            | (self.generation & 0x1ff) << 3
            | (self.occupied as u32) << 2
            | (self.writecompleted as u32) << 1
            | self.version as u32;
        b[8..12].copy_from_slice(&w0.to_be_bytes());
        b[12..16].copy_from_slice(&w1.to_be_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> PakFileHeader {
        let u16at = |o: usize| u16::from_be_bytes([b[o], b[o + 1]]);
        let u32at = |o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let (w0, w1) = (u32at(8), u32at(12));
        PakFileHeader {
            headersum: [u16at(0), u16at(2)],
            bodysum: [u16at(4), u16at(6)],
            filetype: w0 >> 23,
            bodylen: (w0 >> 12) & 0x7ff,
            filelen: w0 & 0xfff,
            deviceserial: w1 >> 19,
            fileid: (w1 >> 12) & 0x7f,
            generation: (w1 >> 3) & 0x1ff,
            occupied: w1 & 4 != 0,
            writecompleted: w1 & 2 != 0,
            version: w1 & 1 != 0,
        }
    }
}

/// The fields of `struct pak` (`types.h`) the file system uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pak {
    /// `PAKTYPE_*`.
    pub ty: u8,
    /// `PAKSTATE_*`.
    pub state: u8,
    pub serial: u16,
    /// The last file id given out; a new file takes the next.
    pub maxfileid: i32,
    pub pdnumbytes: u32,
    pub pdnumblocks: u32,
    pub pdnumpages: u32,
    pub pdnumnotes: u32,
    pub pdnoteindex: i32,
    pub plugcount: i32,
}

impl Default for Pak {
    /// `pak_set_defaults` (`pak.c:2868`).
    fn default() -> Self {
        Pak { ty: PAKTYPE_NONE, state: PAKSTATE_NOPAK, serial: 0, maxfileid: 8, pdnumbytes: 0, pdnumblocks: 0, pdnumpages: 0, pdnumnotes: 0, pdnoteindex: -1, plugcount: 0 }
    }
}

/// `g_Paks[5]` (four Controller Paks, then the Game Pak) and the EEPROM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paks {
    pub paks: [Pak; 5],
    /// `g_PakHasEeprom`.
    pub has_eeprom: bool,
    /// The cartridge's EEPROM, [`EEPROM_SIZE`] bytes.
    pub eeprom: Vec<u8>,
    /// Set by every EEPROM write since the game last took it
    /// ([`Paks::take_written`]).
    pub written: bool,
}

impl Paks {
    /// The paks as `paks_init` finds them on a console with no Controller
    /// Paks, over `eeprom` (a blank chip reads as all ones).
    pub fn new(eeprom: Option<Vec<u8>>) -> Paks {
        let mut eeprom = eeprom.unwrap_or_else(|| vec![0xff; EEPROM_SIZE]);
        eeprom.resize(EEPROM_SIZE, 0xff);
        Paks { paks: Default::default(), has_eeprom: true, eeprom, written: false }
    }

    /// Whether the EEPROM changed since the last call.
    pub fn take_written(&mut self) -> bool {
        std::mem::take(&mut self.written)
    }

    fn pak(&self, device: i8) -> &Pak {
        &self.paks[device as usize]
    }

    fn pak_mut(&mut self, device: i8) -> &mut Pak {
        &mut self.paks[device as usize]
    }

    /// `pak_probe_eeprom` (`pak.c:5393`) and the Game Pak's half of
    /// `pak_tick_state` through `mempak_prepare`, as `paks_init`'s
    /// `pak0f1169c8(SAVEDEVICE_GAMEPAK, true)` runs them.
    pub fn paks_init(&mut self, rng: &mut Rng) {
        for p in self.paks.iter_mut() {
            *p = Pak::default();
        }
        // osEepromProbe finds the 16 Kbit chip.
        self.has_eeprom = true;
        if self.pak_init_pak() == PAK_ERR1_OK {
            self.mempak_prepare(SAVEDEVICE_GAMEPAK, rng);
        }
    }

    /// `pak_init_pak` (`pak.c:1109`) for the Game Pak (no `OSPfs`).
    fn pak_init_pak(&self) -> i32 {
        if !self.has_eeprom {
            return PAK_ERR1_EEPROMMISSING;
        }
        PAK_ERR1_OK
    }

    // ---- sizes (pak.c:375-757) ----

    /// `pak_get_block_size` (`pak.c:375`).
    pub fn pak_get_block_size(device: i8) -> u32 {
        if device == SAVEDEVICE_GAMEPAK {
            0x10
        } else {
            0x20
        }
    }

    /// `pak_align` (`pak.c:380`).
    pub fn pak_align(device: i8, size: u32) -> u32 {
        if Self::pak_get_block_size(device) == 0x20 {
            (size + 0x1f) & !0x1f
        } else {
            (size + 0xf) & !0xf
        }
    }

    /// `pak_get_aligned_file_len_by_body_len` (`pak.c:390`).
    pub fn pak_get_aligned_file_len_by_body_len(device: i8, bodylen: u32) -> u32 {
        Self::pak_align(device, 16 + bodylen)
    }

    /// `pak_get_body_len_by_file_len` (`pak.c:395`).
    pub fn pak_get_body_len_by_file_len(filelen: u32) -> u32 {
        filelen.wrapping_sub(16)
    }

    /// `pak_get_max_file_size` (`pak.c:718`).
    pub fn pak_get_max_file_size(device: i8) -> u32 {
        if device != SAVEDEVICE_GAMEPAK {
            return 0x4c0;
        }
        0x100
    }

    /// `pak_get_body_len_by_type` (`pak.c:727`).
    pub fn pak_get_body_len_by_type(device: i8, filetype: u32) -> u32 {
        match filetype {
            PAKFILETYPE_TERMINATOR => Self::pak_get_max_file_size(device) - 16,
            PAKFILETYPE_BOSS => 0x5b,
            PAKFILETYPE_MPPLAYER => 0x4e,
            PAKFILETYPE_MPSETUP => 0x31,
            PAKFILETYPE_CAMERA => 0x4a0,
            PAKFILETYPE_GAME => 0xa0,
            _ => 0,
        }
    }

    pub fn pak_get_pd_num_bytes(&self, device: i8) -> u32 {
        self.pak(device).pdnumbytes
    }

    pub fn pak_get_serial(&self, device: i8) -> u16 {
        self.pak(device).serial
    }

    pub fn pak_get_plug_count(&self, device: i8) -> i32 {
        self.pak(device).plugcount
    }

    /// `pak_generate_serial` (`pak.c:401`): the Game Pak's is fixed.
    fn pak_generate_serial(&mut self, device: i8, rng: &mut Rng) -> u16 {
        if device == SAVEDEVICE_GAMEPAK {
            return 0xbaa;
        }
        // (A Controller Pak's mixes the pak's id, random() and the CPU count.)
        ((rng.random() % 496) + 16) as u16
    }

    // ---- readiness (pak.c:561, :1973) ----

    /// `mempak_is_ready` (`pak.c:561`).
    pub fn mempak_is_ready(&self, device: i8) -> bool {
        let p = self.pak(device);
        p.state == PAKSTATE_READY && p.ty == PAKTYPE_MEMORY
    }

    /// `pak0f119298` (`pak.c:1973`): 0 when the pak's file system can be used.
    pub fn pak0f119298(&self, device: i8) -> i32 {
        let p = self.pak(device);
        if p.ty != PAKTYPE_MEMORY {
            return 1;
        }
        match p.state {
            PAKSTATE_READY => 0,
            PAKSTATE_17 => 14,
            PAKSTATE_18 => 4,
            PAKSTATE_MEM_DISPATCH | PAKSTATE_MEM_PRE_PREPARE | PAKSTATE_MEM_PREPARE | PAKSTATE_MEM_POST_PREPARE | PAKSTATE_07 => 13,
            _ => 1,
        }
    }

    /// `pak0f1167d8` (`pak.c:442`).
    pub fn pak0f1167d8(&self, device: i8) -> i32 {
        self.pak0f119298(device)
    }

    /// `pak_find_by_serial` (`pak.c:5531`): the last ready pak with this serial.
    pub fn pak_find_by_serial(&self, findserial: i32) -> i8 {
        let mut device = -1;
        for i in 0..5 {
            if self.mempak_is_ready(i) && findserial == self.pak_get_serial(i) as i32 {
                device = i;
            }
        }
        device
    }

    // ---- block I/O (pak.c:1129, :2898, :5412) ----

    /// `pak_read_write_block` (`pak.c:2898`) + `_pak_read_write_block`
    /// (`:1129`): on the Game Pak, `osEepromLongRead`/`Write` from EEPROM
    /// block `address / 8` (a `u8`: it wraps, as the chip's addressing does).
    fn pak_read_write_block(&mut self, device: i8, flag: u8, address: u32, len: u32, buffer: &mut [u8]) -> i32 {
        let len = Self::pak_align(device, len) as usize;
        if device != SAVEDEVICE_GAMEPAK {
            // osPfsReadWriteFile on a Controller Pak: none is plugged in.
            return PAK_ERR1_NOPAK;
        }
        if !self.has_eeprom {
            return PAK_ERR1_EEPROMMISSING;
        }
        let block = (address / 8) as u8 as usize;
        match flag {
            OS_WRITE => {
                for i in 0..len {
                    self.eeprom[(block * 8 + i) % EEPROM_SIZE] = buffer.get(i).copied().unwrap_or(0);
                }
                self.written = true;
                PAK_ERR1_OK
            }
            OS_READ => {
                for i in 0..len {
                    if let Some(b) = buffer.get_mut(i) {
                        *b = self.eeprom[(block * 8 + i) % EEPROM_SIZE];
                    }
                }
                PAK_ERR1_OK
            }
            _ => PAK_ERR1_EEPROMINVALIDOP,
        }
    }

    // ---- headers and the file walk (pak.c:884, :2015, :3506) ----

    /// `pak_read_header_at_offset` (`pak.c:884`): the header at `offset`
    /// into `header` (filled, as PD's is, even when its checksum fails).
    pub fn pak_read_header_at_offset(&mut self, device: i8, offset: u32, header: &mut PakFileHeader) -> i32 {
        let blocknum = offset / Self::pak_get_block_size(device);
        if blocknum >= self.pak(device).pdnumblocks {
            return PAK_ERR2_BADOFFSET;
        }
        let mut sp38 = [0u8; 0x20];
        let result = self.pak_read_write_block(device, OS_READ, offset, 0x20, &mut sp38);
        if result != PAK_ERR1_OK {
            if result == PAK_ERR1_NOPAK {
                return PAK_ERR2_NOPAK;
            }
            return PAK_ERR2_BADOFFSET;
        }
        *header = PakFileHeader::from_bytes(&sp38[..16]);
        let checksum = crc_calculate_u16_pair(&sp38[0x08..0x10]);
        if header.headersum != checksum {
            return PAK_ERR2_CHECKSUM;
        }
        if !header.writecompleted {
            return PAK_ERR2_INCOMPLETE;
        }
        if header.version {
            return PAK_ERR2_VERSION;
        }
        if header.filelen == 0 {
            return PAK_ERR2_CORRUPT;
        }
        PAK_ERR2_OK
    }

    /// `pak_get_filesystem_length` (`pak.c:3506`): the offset past the
    /// terminator into `outlen`; true only when the pak is gone.
    pub fn pak_get_filesystem_length(&mut self, device: i8, outlen: &mut u32) -> bool {
        let mut header = PakFileHeader::default();
        let mut offset: u32 = 0;
        while offset < self.pak(device).pdnumbytes {
            let ret = self.pak_read_header_at_offset(device, offset, &mut header);
            offset = offset.wrapping_add(header.filelen);
            if ret == PAK_ERR2_NOPAK {
                return true;
            }
            if header.filetype == PAKFILETYPE_TERMINATOR {
                *outlen = offset;
                return false;
            }
            if ret != PAK_ERR2_OK {
                return false;
            }
        }
        false
    }

    /// `pak_find_file` (`pak.c:2015`): the offset of file `fileid` (its
    /// header into `headerptr`), `0xffff` if there is none, -1 if the pak
    /// is gone.
    pub fn pak_find_file(&mut self, device: i8, fileid: u32, headerptr: Option<&mut PakFileHeader>) -> i32 {
        let mut header = PakFileHeader::default();
        let mut offset: i32 = 0;
        // SUBST: PD leaves fslen unset when no terminator is found / 0 here.
        let mut fslen = 0u32;
        self.pak_get_filesystem_length(device, &mut fslen);
        let mut ret = self.pak_read_header_at_offset(device, offset as u32, &mut header);
        while ret == PAK_ERR2_OK && (offset as u32) < fslen {
            if fileid == header.fileid {
                if let Some(h) = headerptr {
                    *h = header;
                }
                return offset;
            }
            offset += header.filelen as i32;
            ret = self.pak_read_header_at_offset(device, offset as u32, &mut header);
        }
        if ret == PAK_ERR2_NOPAK {
            return -1;
        }
        0xffff
    }

    /// Whether an offset `pak_find_file` returned is a file's (PD's inline
    /// `offset == 0 || (offset < pdnumbytes && aligned)` test).
    fn offset_ok(&self, device: i8, offset: i32) -> bool {
        offset == 0 || (offset != 0 && (offset as u32) < self.pak_get_pd_num_bytes(device) && ((Self::pak_get_block_size(device) - 1) & offset as u32) == 0)
    }

    /// `_pak_get_file_ids_by_type` (`pak.c:1826`): the ids of the files of a
    /// type (`PAKFILETYPE_ALL`: every one, the terminator too), up to the
    /// first id 0 as PD's zero-terminated list reads.
    pub fn pak_get_file_ids_by_type(&mut self, device: i8, filetype: u32, fileids: &mut Vec<u32>) -> i32 {
        fileids.clear();
        let result = self.pak0f119298(device);
        if result != 0 {
            return result;
        }
        // SUBST: PD leaves fslen unset when no terminator is found / 0 here.
        let mut fslen = 0u32;
        if self.pak_get_filesystem_length(device, &mut fslen) {
            return 1;
        }
        let result = self.pak0f1167d8(device);
        if result != 0 {
            return result;
        }
        let mut header = PakFileHeader::default();
        let mut offset: u32 = 0;
        let mut result = self.pak_read_header_at_offset(device, offset, &mut header);
        while result == PAK_ERR2_OK {
            if (filetype & PAKFILETYPE_ALL) != 0 || (filetype & header.filetype) != 0 {
                fileids.push(header.fileid);
            }
            offset = offset.wrapping_add(header.filelen);
            if offset >= fslen {
                break;
            }
            result = self.pak_read_header_at_offset(device, offset, &mut header);
        }
        if let Some(z) = fileids.iter().position(|&id| id == 0) {
            fileids.truncate(z);
        }
        if result == PAK_ERR2_CHECKSUM {
            return 7;
        }
        if result == PAK_ERR2_NOPAK {
            return 1;
        }
        0
    }

    // ---- reading (pak.c:1778, :3540) ----

    /// `pak0f11b86c` (`pak.c:3540`): the body of the file at `offset` into
    /// `data`: `bodylen` bytes, or the whole body when 0 (or when it equals
    /// the header's), or the whole aligned file less its header when -1 (and
    /// then an empty file reads too).
    fn pak0f11b86c(&mut self, device: i8, offset: u32, mut data: Option<&mut [u8]>, header: &mut PakFileHeader, bodylen: i32) -> i32 {
        let (negativebodylen, mut bodylen) = if bodylen == -1 { (true, 0u32) } else { (false, bodylen as u32) };
        let ret = self.pak_read_header_at_offset(device, offset, header);
        if ret != 0 {
            return ret;
        }
        if !negativebodylen && !header.occupied {
            return 10;
        }
        // (isoneblock needs a body in the header's block: never on 16-byte blocks.)
        if bodylen == header.bodylen {
            bodylen = 0;
        }
        let alignedfilelen = Self::pak_get_aligned_file_len_by_body_len(device, header.bodylen);
        let mut filelen = (if bodylen == 0 { header.bodylen } else { bodylen }) + 16;
        if negativebodylen {
            filelen = alignedfilelen;
        }
        let bs = Self::pak_get_block_size(device);
        let mut sp58 = [0u8; 128];
        let mut di = 0usize;
        for i in 0..filelen {
            let offsetinblock = i % bs;
            let blocknum = i / bs;
            if offsetinblock == 0 {
                let absoluteoffset = bs * blocknum + offset;
                let ret = self.pak_read_write_block(device, OS_READ, absoluteoffset, bs, &mut sp58);
                if ret != PAK_ERR1_OK {
                    if ret == 1 {
                        return 1;
                    }
                    return 4;
                }
            }
            if i >= 0x10 {
                if let Some(d) = data.as_deref_mut() {
                    if let Some(b) = d.get_mut(di) {
                        *b = sp58[offsetinblock as usize];
                    }
                    di += 1;
                }
            }
        }
        0
    }

    /// `_pak_read_body_at_guid` (`pak.c:1778`): file `fileid`'s body into
    /// `body` (see [`Paks::pak0f11b86c`] for `arg3`), checked against its
    /// checksum when whole. 10: the file is empty; 8: its body is corrupt.
    pub fn pak_read_body_at_guid(&mut self, device: i8, fileid: i32, mut body: Option<&mut [u8]>, arg3: i32) -> i32 {
        if self.pak0f1167d8(device) != 0 {
            return 6;
        }
        let offset = self.pak_find_file(device, fileid as u32, None);
        if offset == -1 {
            return 1;
        }
        if !self.offset_ok(device, offset) {
            return 3;
        }
        let mut header = PakFileHeader::default();
        let result = self.pak0f11b86c(device, offset as u32, body.as_deref_mut(), &mut header, arg3);
        if result != 0 {
            return result;
        }
        let arg3 = if arg3 == -1 { 0 } else { arg3 };
        if header.occupied {
            if arg3 == 0 {
                let len = header.bodylen as usize;
                let checksum = body.as_deref().map(|b| crc_calculate_u16_pair(&b[..len.min(b.len())])).unwrap_or([0, 0]);
                if header.bodysum != checksum {
                    return 8;
                }
            }
        } else {
            return 10;
        }
        0
    }

    // ---- writing (pak.c:3685, :992, :675) ----

    /// `pak_write_file_at_offset` (`pak.c:3685`): a file of `filetype` at
    /// `offset`: its header, then (when there is data) the blocks that differ
    /// from `olddata`, then the header again, complete. A new id is taken
    /// when `fileid` is 0. PD's padding repeats the start of the data.
    #[allow(clippy::too_many_arguments)]
    pub fn pak_write_file_at_offset(&mut self, device: i8, offset: u32, filetype: u32, newdata: Option<&[u8]>, bodylenarg: u32, outfileid: Option<&mut i32>, olddata: Option<&[u8]>, fileid: u32, generation: u32) -> i32 {
        let blocksize = Self::pak_get_block_size(device);
        let generation = generation & 0x1ff;
        let bodylen = if bodylenarg != 0 { bodylenarg } else { Self::pak_get_body_len_by_type(device, filetype) };
        let filelen = Self::pak_get_aligned_file_len_by_body_len(device, bodylen);
        let id = if fileid != 0 {
            fileid
        } else {
            self.pak_mut(device).maxfileid += 1;
            self.pak(device).maxfileid as u32
        };
        let mut header = PakFileHeader {
            fileid: id & 0x7f,
            deviceserial: self.pak(device).serial as u32 & 0x1fff,
            filelen: filelen & 0xfff,
            version: false,
            bodylen: bodylen & 0x7ff,
            generation,
            filetype: filetype & 0x1ff,
            ..PakFileHeader::default()
        };
        if let Some(o) = outfileid {
            *o = header.fileid as i32;
        }
        header.occupied = newdata.is_some();
        if let Some(d) = newdata {
            header.bodysum = crc_calculate_u16_pair(&d[..header.bodylen as usize]);
        } else {
            header.bodysum = [0xffff, 0xffff];
        }
        let paddinglen = filelen - bodylen - 16;
        let at = |d: Option<&[u8]>, i: u32| d.map_or(b'+', |d| d.get(i as usize).copied().unwrap_or(0));
        let mut newfilebytes: Vec<u8> = Vec::with_capacity(filelen as usize);
        let mut oldfilebytes: Vec<u8> = Vec::with_capacity(filelen as usize);
        newfilebytes.extend_from_slice(&header.to_bytes());
        oldfilebytes.extend_from_slice(&[b'+'; 16]);
        for i in 0..bodylen {
            newfilebytes.push(at(newdata, i));
            oldfilebytes.push(at(olddata, i));
        }
        for i in 0..paddinglen {
            newfilebytes.push(at(newdata, i));
            oldfilebytes.push(at(olddata, i));
        }
        let mut numblocks = filelen / blocksize;
        if filelen % blocksize != 0 {
            numblocks += 1;
        }
        for j in 0..2 {
            header.writecompleted = j == 1;
            let hb = header.to_bytes();
            header.headersum = crc_calculate_u16_pair(&hb[8..16]);
            newfilebytes[..16].copy_from_slice(&header.to_bytes());
            for i in 0..numblocks {
                let offsetinfile = blocksize * i;
                let mut writethisblock = false;
                if offsetinfile < 16 {
                    writethisblock = true;
                } else {
                    if j == 1 {
                        break;
                    }
                    if header.filetype == PAKFILETYPE_BLANK || newdata.is_none() {
                        break;
                    }
                    if olddata.is_some() {
                        for k in 0..blocksize {
                            let index = (i * blocksize + k) as usize;
                            if newfilebytes[index] != oldfilebytes[index] {
                                writethisblock = true;
                                break;
                            }
                        }
                    } else {
                        writethisblock = true;
                    }
                }
                if writethisblock {
                    let o = offsetinfile as usize;
                    let mut block: Vec<u8> = newfilebytes[o..(o + blocksize as usize).min(newfilebytes.len())].to_vec();
                    block.resize(blocksize as usize, 0);
                    let result = self.pak_read_write_block(device, OS_WRITE, offset + i * blocksize, blocksize, &mut block);
                    if result != PAK_ERR1_OK {
                        if result == PAK_ERR1_NOPAK {
                            return 1;
                        }
                        return 4;
                    }
                }
            }
        }
        0
    }

    /// `_pak_save_at_guid` (`pak.c:992`): file `fileid`'s new body, written
    /// into the swap space of its type (another empty file of that type),
    /// which then becomes `fileid` a generation on, while the old copy turns
    /// into the next swap. The new id goes to `outfileid`.
    pub fn pak_save_at_guid(&mut self, device: i8, fileid: i32, filetype: u32, newdata: &[u8], outfileid: Option<&mut i32>) -> i32 {
        let mut header = PakFileHeader::default();
        let oldoffset = self.pak_find_file(device, fileid as u32, Some(&mut header));
        if oldoffset != 0 && (oldoffset == 0 || oldoffset as u32 >= self.pak_get_pd_num_bytes(device) || ((Self::pak_get_block_size(device) - 1) & oldoffset as u32) != 0) {
            return 3;
        }
        if filetype != header.filetype {
            return 12;
        }
        let mut fileids = Vec::new();
        self.pak_get_file_ids_by_type(device, header.filetype, &mut fileids);
        let mut swapoffset: i32 = 0xeeeeeeeeu32 as i32;
        let mut swapfileid = 0u32;
        for &id in &fileids {
            let mut swapheader = PakFileHeader::default();
            swapoffset = self.pak_find_file(device, id, Some(&mut swapheader));
            if swapoffset == -1 {
                return 1;
            }
            if !swapheader.occupied && swapheader.fileid != fileid as u32 {
                swapfileid = swapheader.fileid;
                break;
            }
        }
        // The Game Pak reads the swap's old bytes so that unchanged blocks are
        // not rewritten.
        let mut olddata = vec![0u8; 0x800];
        let mut olddataptr: Option<&[u8]> = None;
        if device == SAVEDEVICE_GAMEPAK {
            let result = self.pak_read_body_at_guid(device, swapfileid as i32, Some(&mut olddata), -1);
            if result == 0 || result == 10 {
                olddataptr = Some(&olddata);
            }
        }
        let olddata_copy = olddataptr.map(|d| d.to_vec());
        let result = self.pak_write_file_at_offset(device, swapoffset as u32, filetype, Some(newdata), 0, outfileid, olddata_copy.as_deref(), fileid as u32, header.generation + 1);
        if result != 0 {
            return 4;
        }
        if oldoffset == -1 {
            return 1;
        }
        if oldoffset != 0xeeeeeeeeu32 as i32 {
            self.pak_write_file_at_offset(device, oldoffset as u32, filetype, None, 0, None, None, swapfileid, header.generation);
        }
        0
    }

    /// `_pak_delete_file` (`pak.c:675`): file `fileid` emptied, under a new
    /// id, a generation on.
    pub fn pak_delete_file(&mut self, device: i8, fileid: i32) -> i32 {
        let mut header = PakFileHeader::default();
        let result = self.pak_find_file(device, fileid as u32, Some(&mut header));
        if result == -1 {
            return 1;
        }
        let result = self.pak_write_file_at_offset(device, result as u32, header.filetype, None, 0, None, None, 0, header.generation + 1);
        if result != 0 {
            return result;
        }
        0
    }

    /// `pak_replace_file_at_offset_with_blank` (`pak.c:3667`).
    fn pak_replace_file_at_offset_with_blank(&mut self, device: i8, offset: u32) -> bool {
        let mut header = PakFileHeader::default();
        if self.pak_read_header_at_offset(device, offset, &mut header) == PAK_ERR2_OK {
            return self.pak_write_file_at_offset(device, offset, PAKFILETYPE_BLANK, None, header.filelen - 16, None, None, 0, 1) == 0;
        }
        false
    }

    /// `pak_write_blank_file` (`pak.c:2071`).
    fn pak_write_blank_file(&mut self, device: i8, offset: u32, header: &PakFileHeader) -> bool {
        self.pak_write_file_at_offset(device, offset, PAKFILETYPE_BLANK, None, Self::pak_get_body_len_by_file_len(header.filelen), None, None, 0, 1) == 0
    }

    /// `pak0f118b04` (`pak.c:1750`): the file blanked out.
    pub fn pak0f118b04(&mut self, device: i8, fileid: u32) -> i32 {
        if self.pak0f1167d8(device) != 0 {
            return 6;
        }
        let offset = self.pak_find_file(device, fileid, None);
        if offset == -1 {
            return 1;
        }
        if self.offset_ok(device, offset) {
            if !self.pak_replace_file_at_offset_with_blank(device, offset as u32) {
                return 4;
            }
        } else {
            return 3;
        }
        0
    }

    /// `pak0f118674` (`pak.c:1528`): a new, empty file of `filetype` in the
    /// first blank space or at the terminator (moving it on), unless the
    /// device can't take it (14).
    pub fn pak0f118674(&mut self, device: i8, filetype: u32, outfileid: Option<&mut i32>) -> i32 {
        let filelen = Self::pak_get_aligned_file_len_by_body_len(device, Self::pak_get_body_len_by_type(device, filetype));
        let mut bestoffset: i64 = -1;
        let mut offset: u32 = 0;
        let mut foundperfectblank = false;
        let mut foundblank = false;
        if self.pak0f1167d8(device) != 0 {
            return self.pak0f1167d8(device);
        }
        let mut header = PakFileHeader::default();
        while offset < self.pak(device).pdnumbytes {
            let ret = self.pak_read_header_at_offset(device, offset, &mut header);
            if ret == PAK_ERR2_OK {
                if header.filetype & PAKFILETYPE_TERMINATOR != 0 {
                    if offset + filelen > self.pak(device).pdnumbytes - 0x20 {
                        return 14;
                    }
                    bestoffset = offset as i64;
                    break;
                }
                if header.filetype & PAKFILETYPE_BLANK != 0 {
                    if header.filelen == filelen {
                        foundperfectblank = true;
                        bestoffset = offset as i64;
                        break;
                    }
                    foundblank = true;
                    bestoffset = offset as i64;
                    break;
                }
                offset += header.filelen;
            } else if ret == PAK_ERR2_NOPAK {
                return 1;
            } else {
                offset += Self::pak_get_block_size(device);
            }
        }
        if offset == 0 || (offset < self.pak_get_pd_num_bytes(device) && ((Self::pak_get_block_size(device) - 1) & offset) == 0) {
            if bestoffset == -1 {
                return 14;
            }
            let bestoffset = bestoffset as u32;
            if self.pak_write_file_at_offset(device, bestoffset, filetype, None, 0, outfileid, None, 0, 1) == 0 {
                if foundblank {
                    let mut endoffset = bestoffset + filelen;
                    self.pak_repair_as_blank(device, &mut endoffset, None);
                    return 0;
                }
                if foundperfectblank || foundblank {
                    return 0;
                }
                let next = bestoffset + Self::pak_get_aligned_file_len_by_body_len(device, Self::pak_get_body_len_by_type(device, filetype));
                if self.pak_write_file_at_offset(device, next, PAKFILETYPE_TERMINATOR, None, 0, None, None, 0, 1) == 0 {
                    return 0;
                }
                return 4;
            }
            return 4;
        }
        let p = self.pak_mut(device);
        p.state = PAKSTATE_MEM_ENTER_FULL;
        p.ty = PAKTYPE_MEMORY;
        4
    }

    // ---- repair and creation (pak.c:2103-2800, :3461) ----

    /// `pak_repair_as_blank` (`pak.c:2103`, NTSC final): a blank file from
    /// `*offsetptr` up to the next real file, or a terminator there when the
    /// device ends first.
    pub fn pak_repair_as_blank(&mut self, device: i8, offsetptr: &mut u32, header: Option<&PakFileHeader>) -> bool {
        let maxfilesize = Self::pak_get_max_file_size(device);
        let start = *offsetptr;
        let start2 = *offsetptr;
        let mut offset = *offsetptr;
        if let Some(h) = header {
            offset += h.filelen;
        }
        let mut iterheader = PakFileHeader::default();
        while offset < self.pak(device).pdnumbytes {
            let result = self.pak_read_header_at_offset(device, offset, &mut iterheader);
            if result == PAK_ERR2_OK {
                if (iterheader.filetype & PAKFILETYPE_BLANK) == 0 && offset > start2 {
                    break;
                }
            } else if result == PAK_ERR2_NOPAK {
                return false;
            }
            offset += Self::pak_get_block_size(device);
            if device != SAVEDEVICE_GAMEPAK && offset - start > maxfilesize {
                *offsetptr = offset;
                return false;
            }
            if offset >= self.pak(device).pdnumbytes {
                self.pak_write_file_at_offset(device, start, PAKFILETYPE_TERMINATOR, None, 0, None, None, 0, 1);
                return true;
            }
        }
        let bodylen = Self::pak_get_body_len_by_file_len(offset - start);
        let result = self.pak_write_file_at_offset(device, start, PAKFILETYPE_BLANK, None, bodylen, None, None, 0, 1);
        *offsetptr = offset;
        result == 0
    }

    /// `pak_repair_filesystem` (`pak.c:2250`, NTSC final): unreadable
    /// headers and the older of two copies of a file blanked, a file running
    /// past the device turned into the terminator, one device serial kept (by
    /// majority). 0: fine or repaired; -1: start again; 1: the pak is gone.
    pub fn pak_repair_filesystem(&mut self, device: i8) -> i32 {
        let mut fatal = false;
        let mut foundotherversion = false;
        let mut headers: Vec<PakFileHeader> = Vec::new();
        let mut headeroffsets: Vec<i64> = Vec::new();
        self.pak_mut(device).serial = 0xbaba;
        if self.pak0f1167d8(device) != 0 {
            return 1;
        }
        let mut header = PakFileHeader::default();
        let mut offset: u32 = 0;
        while !fatal && offset < self.pak(device).pdnumbytes {
            let ret = self.pak_read_header_at_offset(device, offset, &mut header);
            if ret == PAK_ERR2_OK {
                if header.filetype & PAKFILETYPE_BLANK != 0 {
                    break;
                }
                if header.filetype & PAKFILETYPE_TERMINATOR != 0 {
                    break;
                }
                if offset + header.filelen >= self.pak(device).pdnumbytes {
                    let ret = self.pak_write_file_at_offset(device, offset, PAKFILETYPE_TERMINATOR, None, 0, None, None, 0, 1);
                    if ret != 0 {
                        fatal = true;
                    } else {
                        break;
                    }
                } else {
                    let mut foundduplicate = false;
                    for i in 0..headers.len() {
                        if headeroffsets[i] != -1 && header.fileid == headers[i].fileid {
                            foundduplicate = true;
                            if header.generation < headers[i].generation {
                                fatal = !self.pak_repair_as_blank(device, &mut offset, Some(&header));
                            } else {
                                let mut o = headeroffsets[i] as u32;
                                let h = headers[i];
                                fatal = !self.pak_repair_as_blank(device, &mut o, Some(&h));
                                headeroffsets[i] = -1;
                                headers.push(header);
                                headeroffsets.push(offset as i64);
                                offset += header.filelen;
                            }
                            break;
                        }
                    }
                    if !foundduplicate && !fatal {
                        headers.push(header);
                        headeroffsets.push(offset as i64);
                        offset += header.filelen;
                    }
                }
            } else if ret == PAK_ERR2_NOPAK {
                return 1;
            } else if ret == PAK_ERR2_CHECKSUM {
                fatal = !self.pak_repair_as_blank(device, &mut offset, None);
            } else if ret == PAK_ERR2_INCOMPLETE {
                let h = header;
                if !self.pak_repair_as_blank(device, &mut offset, Some(&h)) {
                    fatal = true;
                    break;
                }
            } else if ret == PAK_ERR2_VERSION {
                foundotherversion = true;
                break;
            } else {
                fatal = true;
                break;
            }
        }
        offset = 0;
        while !foundotherversion && !fatal && offset < self.pak(device).pdnumbytes {
            let ret = self.pak_read_header_at_offset(device, offset, &mut header);
            if ret == 0 {
                if header.filetype & PAKFILETYPE_BLANK != 0 {
                    // empty
                } else if offset != 0 {
                    // (NTSC final checks the serials below.)
                } else {
                    self.pak_mut(device).serial = header.deviceserial as u16;
                    if header.filetype & PAKFILETYPE_TERMINATOR != 0 {
                        return 0;
                    }
                }
                if header.filetype & PAKFILETYPE_TERMINATOR == 0 {
                    offset += header.filelen;
                } else {
                    break;
                }
            } else if ret == PAK_ERR2_VERSION {
                foundotherversion = true;
                offset += header.filelen;
            } else if ret == PAK_ERR2_NOPAK {
                return 1;
            } else {
                return 1;
            }
        }
        if !foundotherversion && !fatal {
            // One serial across every file: the majority's.
            let mut serials: Vec<(u32, i32)> = Vec::new();
            offset = 0;
            while offset < self.pak(device).pdnumbytes {
                let ret = self.pak_read_header_at_offset(device, offset, &mut header);
                if ret != PAK_ERR2_OK {
                    break;
                }
                if header.filetype & PAKFILETYPE_BLANK == 0 {
                    if header.filetype & PAKFILETYPE_TERMINATOR != 0 {
                        break;
                    }
                    let mut found = false;
                    for s in serials.iter_mut() {
                        if s.0 == header.deviceserial {
                            found = true;
                            s.1 += 1;
                        }
                    }
                    if !found {
                        serials.push((header.deviceserial, 1));
                    }
                }
                offset += header.filelen;
            }
            if serials.len() >= 2 {
                let (mut bestindex, mut bestcount) = (-1i32, -1i32);
                for (i, s) in serials.iter().enumerate() {
                    if s.1 > bestcount {
                        bestindex = i as i32;
                        bestcount = s.1;
                    }
                }
                if bestindex != -1 {
                    self.pak_mut(device).serial = serials[bestindex as usize].0 as u16;
                    offset = 0;
                    while offset < self.pak(device).pdnumbytes {
                        let ret = self.pak_read_header_at_offset(device, offset, &mut header);
                        if ret != PAK_ERR2_OK {
                            break;
                        }
                        if header.filetype & PAKFILETYPE_BLANK == 0 {
                            if header.filetype & PAKFILETYPE_TERMINATOR != 0 {
                                break;
                            }
                            if header.deviceserial != self.pak(device).serial as u32 {
                                let h = header;
                                self.pak_write_blank_file(device, offset, &h);
                            }
                        }
                        offset += header.filelen;
                    }
                }
            } else if let Some(s) = serials.first() {
                self.pak_mut(device).serial = s.0 as u16;
            } else {
                // SUBST: PD reads serials[0] of an empty list (stack garbage) /
                // the serial read above stands.
            }
        }
        if fatal || foundotherversion {
            return -1;
        }
        if device != SAVEDEVICE_GAMEPAK && self.pak(device).serial == 0 {
            return -1;
        }
        0
    }

    /// `pak_create_filesystem` (`pak.c:3461`): a terminator at 0 under a new
    /// serial, and a block of random bytes after it.
    pub fn pak_create_filesystem(&mut self, device: i8, rng: &mut Rng) -> i32 {
        let mut data = [0u8; 32];
        for d in data.iter_mut() {
            *d = rng.random() as u8;
        }
        let address = Self::pak_get_aligned_file_len_by_body_len(device, Self::pak_get_body_len_by_type(device, PAKFILETYPE_TERMINATOR));
        self.pak_mut(device).maxfileid = 0x10;
        let serial = self.pak_generate_serial(device, rng);
        self.pak_mut(device).serial = serial;
        self.pak_write_file_at_offset(device, 0, PAKFILETYPE_TERMINATOR, None, 0, None, None, 0, 1);
        let bs = Self::pak_get_block_size(device);
        if self.pak_read_write_block(device, OS_WRITE, address, bs, &mut data) != PAK_ERR1_OK {
            return -1;
        }
        self.pak(device).serial as i32
    }

    /// `pak_find_max_file_id` (`pak.c:2727`).
    pub fn pak_find_max_file_id(&mut self, device: i8) -> i32 {
        let mut fileids = Vec::new();
        if self.pak_get_file_ids_by_type(device, PAKFILETYPE_ALL, &mut fileids) != 0 {
            return -1;
        }
        let mut max = 0;
        for id in fileids {
            let mut header = PakFileHeader::default();
            if self.pak_find_file(device, id, Some(&mut header)) == -1 {
                return -1;
            }
            max = max.max(header.fileid as i32);
        }
        max
    }

    /// `pak_create_initial_files` (`pak.c:2634`): up to two boss files and
    /// five each of MP players, MP setups and agents (and, on a Controller
    /// Pak, three PerfectHeads), less those already there.
    pub fn pak_create_initial_files(&mut self, device: i8) -> bool {
        let filetypes = [PAKFILETYPE_BOSS, PAKFILETYPE_CAMERA, PAKFILETYPE_MPPLAYER, PAKFILETYPE_MPSETUP, PAKFILETYPE_GAME];
        let mut filecounts = [2, 3, 5, 5, 5];
        let mut fileids = Vec::new();
        if self.pak_get_file_ids_by_type(device, PAKFILETYPE_ALL, &mut fileids) != 0 {
            return false;
        }
        for id in fileids {
            let mut header = PakFileHeader::default();
            if self.pak_find_file(device, id, Some(&mut header)) == -1 {
                return false;
            }
            for j in 0..filetypes.len() {
                if header.filetype == filetypes[j] {
                    if filecounts[j] != 0 {
                        filecounts[j] -= 1;
                    }
                    break;
                }
            }
        }
        for i in 0..filetypes.len() {
            if filecounts[i] != 0 && !(device == SAVEDEVICE_GAMEPAK && i == 1) {
                for _ in 0..filecounts[i] {
                    let ret = self.pak0f118674(device, filetypes[i], None);
                    if ret != 0 {
                        return ret == 14;
                    }
                }
            }
        }
        true
    }

    /// `mempak_prepare` (`pak.c:3030`, NTSC 1.0+) for the Game Pak: the
    /// EEPROM is PD's note (`pak_find_note` says so at once), so its file
    /// system is repaired (or made anew), then given its initial files.
    fn mempak_prepare(&mut self, device: i8, rng: &mut Rng) -> bool {
        let mut error2 = false;
        {
            let p = self.pak_mut(device);
            p.ty = PAKTYPE_MEMORY;
            // pak_find_note: the EEPROM is note 0.
            p.pdnoteindex = 0;
            // pak_query_pd_size: pak_query_note_state's 0x800 bytes.
            p.pdnumbytes = EEPROM_SIZE as u32;
            p.pdnumblocks = p.pdnumbytes / Self::pak_get_block_size(device);
            p.pdnumpages = p.pdnumbytes / 256;
            p.pdnumnotes = p.pdnumbytes / (256 * 28);
            p.state = PAKSTATE_READY;
        }
        if self.pak_repair_filesystem(device) == -1 {
            let serial = self.pak_create_filesystem(device, rng);
            if serial != -1 {
                self.pak_mut(device).serial = serial as u16;
            } else {
                error2 = true;
            }
        }
        if !error2 {
            let maxfileid = self.pak_find_max_file_id(device);
            if maxfileid != -1 {
                self.pak_mut(device).maxfileid = maxfileid;
                let mut fileids = Vec::new();
                if self.pak_get_file_ids_by_type(device, PAKFILETYPE_TERMINATOR, &mut fileids) == 0 && self.pak_create_initial_files(device) {
                    self.pak_mut(device).state = PAKSTATE_READY;
                    return true;
                }
            }
        }
        self.pak_mut(device).state = PAKSTATE_22;
        false
    }

    /// Every header on the device from offset 0 to the terminator, with its
    /// offset (for tests and tools).
    pub fn walk(&mut self, device: i8) -> Vec<(u32, PakFileHeader)> {
        let mut out = Vec::new();
        let mut offset = 0;
        let mut header = PakFileHeader::default();
        while offset < self.pak(device).pdnumbytes && self.pak_read_header_at_offset(device, offset, &mut header) == PAK_ERR2_OK {
            out.push((offset, header));
            if header.filetype & PAKFILETYPE_TERMINATOR != 0 {
                break;
            }
            offset += header.filelen;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Paks {
        let mut p = Paks::new(None);
        p.paks_init(&mut Rng::new(0));
        p
    }

    #[test]
    fn headers_pack_as_ido_bitfields() {
        let h = PakFileHeader { headersum: [1, 2], bodysum: [3, 4], filetype: PAKFILETYPE_GAME, bodylen: 0xa0, filelen: 0xb0, deviceserial: 0x1aba, fileid: 0x7f, generation: 0x1ff, occupied: true, writecompleted: true, version: false };
        let b = h.to_bytes();
        assert_eq!(&b[..8], &[0, 1, 0, 2, 0, 3, 0, 4]);
        assert_eq!(u32::from_be_bytes([b[8], b[9], b[10], b[11]]), 0x80 << 23 | 0xa0 << 12 | 0xb0);
        assert_eq!(PakFileHeader::from_bytes(&b), h);
    }

    /// A blank EEPROM gets PD's layout: two boss files, five MP players, five
    /// MP setups, five agents, all empty, 0x7c0 bytes in all, then the
    /// terminator; the serial is the repair's 0xbaba cut to 13 bits.
    #[test]
    fn a_blank_eeprom_gets_the_initial_files() {
        let mut p = fresh();
        assert!(p.mempak_is_ready(SAVEDEVICE_GAMEPAK));
        assert_eq!(p.pak_get_serial(SAVEDEVICE_GAMEPAK), 0x1aba);
        let files = p.walk(SAVEDEVICE_GAMEPAK);
        let types: Vec<u32> = files.iter().map(|f| f.1.filetype).collect();
        let mut want = vec![PAKFILETYPE_BOSS; 2];
        want.extend([PAKFILETYPE_MPPLAYER; 5]);
        want.extend([PAKFILETYPE_MPSETUP; 5]);
        want.extend([PAKFILETYPE_GAME; 5]);
        want.push(PAKFILETYPE_TERMINATOR);
        assert_eq!(types, want);
        assert_eq!(files.last().unwrap().0, 0x7c0);
        assert!(files.iter().all(|f| !f.1.occupied));
        let lens: Vec<u32> = files[..files.len() - 1].iter().map(|f| f.1.filelen).collect();
        assert_eq!(&lens[..3], &[0x70, 0x70, 0x60]);
        assert!(files.iter().all(|f| f.1.deviceserial == 0x1aba));
        // A second boot reads the same device back and changes nothing.
        let before = p.eeprom.clone();
        p.paks_init(&mut Rng::new(0));
        assert_eq!(p.eeprom, before);
        assert_eq!(p.pak_get_serial(SAVEDEVICE_GAMEPAK), 0x1aba);
    }

    /// Saving writes the body into the type's swap file under the old id, a
    /// generation on; the old copy becomes the swap. The body reads back and
    /// its checksum holds; an empty file reads as 10.
    #[test]
    fn saves_go_through_the_swap_file() {
        let mut p = fresh();
        let dev = SAVEDEVICE_GAMEPAK;
        let mut ids = Vec::new();
        assert_eq!(p.pak_get_file_ids_by_type(dev, PAKFILETYPE_MPPLAYER, &mut ids), 0);
        assert_eq!(ids.len(), 5);
        let (first, second) = (ids[0], ids[1]);
        let mut body = [0u8; 0x4e];
        for (i, b) in body.iter_mut().enumerate() {
            *b = i as u8 * 3;
        }
        let mut newid = 0;
        assert_eq!(p.pak_save_at_guid(dev, first as i32, PAKFILETYPE_MPPLAYER, &body, Some(&mut newid)), 0);
        assert_eq!(newid, first as i32);
        let mut h = PakFileHeader::default();
        let at = p.pak_find_file(dev, first, Some(&mut h));
        assert!(h.occupied && h.generation == 2);
        let mut h2 = PakFileHeader::default();
        let swapat = p.pak_find_file(dev, second, Some(&mut h2));
        assert!(!h2.occupied);
        assert_eq!((at, swapat), (0x140, 0xe0), "the ids swapped places");
        let mut back = [0u8; 220];
        assert_eq!(p.pak_read_body_at_guid(dev, first as i32, Some(&mut back), 0), 0);
        assert_eq!(&back[..0x4e], &body[..]);
        assert_eq!(p.pak_read_body_at_guid(dev, second as i32, Some(&mut back), 0), 10);
        // A second save goes back the other way.
        body[0] = 0x99;
        assert_eq!(p.pak_save_at_guid(dev, first as i32, PAKFILETYPE_MPPLAYER, &body, None), 0);
        assert_eq!(p.pak_find_file(dev, first, Some(&mut h)), 0xe0);
        assert_eq!(h.generation, 3);
        assert_eq!(p.pak_read_body_at_guid(dev, first as i32, Some(&mut back), 0), 0);
        assert_eq!(back[0], 0x99);
        // The wrong type is refused.
        assert_eq!(p.pak_save_at_guid(dev, first as i32, PAKFILETYPE_GAME, &body, None), 12);
    }

    /// A body whose bytes changed on the chip fails its checksum (8); a
    /// header that changed is blanked by the next boot's repair.
    #[test]
    fn corruption_is_caught() {
        let mut p = fresh();
        let dev = SAVEDEVICE_GAMEPAK;
        let mut ids = Vec::new();
        p.pak_get_file_ids_by_type(dev, PAKFILETYPE_GAME, &mut ids);
        let body = [7u8; 0xa0];
        assert_eq!(p.pak_save_at_guid(dev, ids[0] as i32, PAKFILETYPE_GAME, &body, None), 0);
        let off = p.pak_find_file(dev, ids[0], None) as usize;
        p.eeprom[off + 16 + 5] ^= 1;
        let mut back = [0u8; 220];
        assert_eq!(p.pak_read_body_at_guid(dev, ids[0] as i32, Some(&mut back), 0), 8);
        p.eeprom[off + 9] ^= 0x10;
        p.paks_init(&mut Rng::new(0));
        // The repair blanked the file; the initial files filled the blank
        // with a new, empty agent.
        assert_eq!(p.pak_find_file(dev, ids[0], None), 0xffff);
        let files = p.walk(dev);
        assert_eq!(files.iter().filter(|f| f.1.filetype == PAKFILETYPE_GAME).count(), 5);
        assert!(files.iter().all(|f| !f.1.occupied));
        assert_eq!(files.last().unwrap().1.filetype, PAKFILETYPE_TERMINATOR);
    }

    /// Deleting a file empties it under a new id; the next boot's
    /// initial-files pass finds five of each and adds none.
    #[test]
    fn deleting_empties_a_file_under_a_new_id() {
        let mut p = fresh();
        let dev = SAVEDEVICE_GAMEPAK;
        let mut ids = Vec::new();
        p.pak_get_file_ids_by_type(dev, PAKFILETYPE_MPSETUP, &mut ids);
        let body = [1u8; 0x31];
        p.pak_save_at_guid(dev, ids[0] as i32, PAKFILETYPE_MPSETUP, &body, None);
        let max = p.pak_find_max_file_id(dev);
        assert_eq!(p.pak_delete_file(dev, ids[0] as i32), 0);
        assert_eq!(p.pak_find_file(dev, ids[0], None), 0xffff);
        let mut after = Vec::new();
        p.pak_get_file_ids_by_type(dev, PAKFILETYPE_MPSETUP, &mut after);
        assert_eq!(after.len(), 5);
        assert!(after.contains(&(max as u32 + 1)));
        let before = p.walk(dev).len();
        p.paks_init(&mut Rng::new(0));
        assert_eq!(p.walk(dev).len(), before);
    }

    /// No Controller Paks: their file lists fail as an unplugged pak's do,
    /// and a serial finds only the Game Pak.
    #[test]
    fn controller_paks_are_not_plugged_in() {
        let mut p = fresh();
        let mut ids = Vec::new();
        assert_eq!(p.pak_get_file_ids_by_type(SAVEDEVICE_CONTROLLERPAK1, PAKFILETYPE_GAME, &mut ids), 1);
        assert_eq!(p.pak_find_by_serial(0x1aba), SAVEDEVICE_GAMEPAK);
        assert_eq!(p.pak_find_by_serial(0), -1);
    }
}
