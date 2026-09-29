//! The save files' bit packing (`savebuffer.c`'s save half) and their
//! checksum (`crc.c`). Every PD save file body is a bitstream written MSB
//! first into a 220-byte buffer: `savebuffer_write_bits(value, numbits)`, read
//! back in the same order. Strings are ten bytes, zero-padded, without their
//! line break. `pd_core::pak` stores the bodies; the files that fill them
//! (`gamefile.c`, `bossfile.c`, `mplayer.c`'s player and setup files) are the
//! menus' (`pd_menu::files`).
//!
//! C strings are byte slices here: a string ends at its first `\0` (or the
//! slice's end), as PD's `char[]` does.

use crate::rng::Rng;

/// `MAX_USERSTRING_LEN` (`constants.h:33`): a name's length in a file.
pub const MAX_USERSTRING_LEN: usize = 10;

/// `struct fileguid` (`types.h:102`): a file's id on its device and the
/// device's serial.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FileGuid {
    pub fileid: i32,
    pub deviceserial: u16,
}

/// `struct savebuffer` (`types.h:4071`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveBuffer {
    pub bitpos: u32,
    pub bytes: [u8; 220],
}

impl Default for SaveBuffer {
    fn default() -> Self {
        SaveBuffer { bitpos: 0, bytes: [0; 220] }
    }
}

impl SaveBuffer {
    /// A buffer as `savebuffer_reset` leaves it.
    pub fn new() -> SaveBuffer {
        SaveBuffer::default()
    }

    /// `savebuffer_write_bits` (`savebuffer.c:385`): the low `numbits` of
    /// `value`, MSB first. It only sets bits (the buffer starts zeroed).
    pub fn write_bits(&mut self, value: u32, numbits: i32) {
        let mut bit = 1u32 << (numbits - 1);
        while bit != 0 {
            if bit & value != 0 {
                let bitindex = self.bitpos % 8;
                let mask = 1u8 << (7 - bitindex);
                let byteindex = (self.bitpos / 8) as usize;
                self.bytes[byteindex] |= mask;
            }
            self.bitpos += 1;
            bit >>= 1;
        }
    }

    /// `savebuffer_align_to_buffer` (`savebuffer.c:411`): `value` written at
    /// this buffer's bit position into `dst` (setting and clearing bits).
    pub fn align_to_buffer(&mut self, value: u32, numbits: i32, dst: &mut [u8]) {
        let mut bit = 1u32 << (numbits - 1);
        while bit != 0 {
            let bitindex = self.bitpos % 8;
            let mask = 1u8 << (7 - bitindex);
            let byteindex = (self.bitpos / 8) as usize;
            if bit & value != 0 {
                dst[byteindex] |= mask;
            } else {
                dst[byteindex] &= !mask;
            }
            self.bitpos += 1;
            bit >>= 1;
        }
    }

    /// `savebuffer_read_bits` (`savebuffer.c:438`).
    pub fn read_bits(&mut self, numbits: i32) -> u32 {
        let mut bit = 1u32 << (numbits - 1);
        let mut value = 0;
        while bit != 0 {
            let bitindex = self.bitpos % 8;
            let mask = 1u8 << (7 - bitindex);
            let byteindex = (self.bitpos / 8) as usize;
            if self.bytes[byteindex] & mask != 0 {
                value |= bit;
            }
            self.bitpos += 1;
            bit >>= 1;
        }
        value
    }

    /// `savebuffer_reset` (`savebuffer.c:459`).
    pub fn reset(&mut self) {
        self.bitpos = 0;
        self.bytes = [0; 220];
    }

    /// `savebuffer_prepare_string` (`savebuffer.c:477`): `len` bytes of `src`
    /// at the start of the buffer, to be read back.
    pub fn prepare_string(&mut self, src: &[u8], len: usize) {
        self.bitpos = 0;
        for i in 0..len {
            self.bytes[i] = src.get(i).copied().unwrap_or(0);
        }
    }

    /// `savebuffer_read_string` (`savebuffer.c:498`): ten bytes into `dst`,
    /// then a line break if asked and the terminator. PD's @bug: an empty
    /// name leaves `dst[0]` as it was (the index only moves on a character),
    /// so the break and terminator land at 1 and 2.
    pub fn read_string(&mut self, dst: &mut [u8], addlinebreak: bool) {
        let mut foundnull = false;
        let mut index = 0;
        for i in 0..MAX_USERSTRING_LEN {
            let byte = self.read_bits(8) as u8;
            if !foundnull {
                if byte == 0 {
                    foundnull = true;
                } else {
                    dst[i] = byte;
                    index = i;
                }
            }
        }
        if addlinebreak {
            index += 1;
            dst[index] = b'\n';
        }
        index += 1;
        dst[index] = 0;
    }

    /// `savebuffer_write_string` (`savebuffer.c:525`): ten bytes, stopping at
    /// the terminator or the line break, zero-padded.
    pub fn write_string(&mut self, src: &[u8]) {
        let mut done = false;
        for i in 0..MAX_USERSTRING_LEN {
            if !done {
                let c = src.get(i).copied().unwrap_or(0);
                if c == 0 || c == b'\n' {
                    done = true;
                } else {
                    self.write_bits(c as u32, 8);
                }
            }
            if done {
                self.write_bits(0, 8);
            }
        }
    }

    /// `savebuffer_write_guid` (`savebuffer.c:591`).
    pub fn write_guid(&mut self, guid: &FileGuid) {
        self.write_bits(guid.fileid as u32, 7);
        self.write_bits(guid.deviceserial as u32, 13);
    }

    /// `savebuffer_read_guid` (`savebuffer.c:597`).
    pub fn read_guid(&mut self) -> FileGuid {
        let fileid = self.read_bits(7) as i32;
        let deviceserial = self.read_bits(13) as u16;
        FileGuid { fileid, deviceserial }
    }
}

/// `savebuffer_bitstring_to_cstring` (`savebuffer.c:557`): a file's
/// ten-byte name as a C string, with a line break if asked.
pub fn savebuffer_bitstring_to_cstring(bitstring: &[u8], cstring: &mut [u8], addlinebreak: bool) {
    let mut buffer = SaveBuffer::new();
    buffer.prepare_string(bitstring, MAX_USERSTRING_LEN);
    buffer.read_string(cstring, addlinebreak);
}

/// `savebuffer_cstring_to_bitstring` (`savebuffer.c:572`): a C string (ended
/// by its terminator or line break, at most ten bytes) into a file's name.
pub fn savebuffer_cstring_to_bitstring(bitstring: &mut [u8], cstring: &[u8]) {
    let mut buffer = SaveBuffer::new();
    buffer.prepare_string(bitstring, MAX_USERSTRING_LEN);
    let mut done = false;
    for i in 0..MAX_USERSTRING_LEN {
        if !done {
            let c = cstring.get(i).copied().unwrap_or(0);
            if c == 0 || c == b'\n' {
                done = true;
            } else {
                buffer.align_to_buffer(c as u32, 8, bitstring);
            }
        }
        if done {
            buffer.align_to_buffer(0, 8, bitstring);
        }
    }
}

/// `crc_calculate_u16_pair` (`crc.c:30`): the pak's checksum of
/// `data`, two halves (forwards, then backwards through `rng_rotate_seed`).
pub fn crc_calculate_u16_pair(data: &[u8]) -> [u16; 2] {
    let mut salt: u32 = 0;
    let mut seed: u64 = 0x8f809f473108b3c1;
    let (mut sum1, mut sum2) = (0u32, 0u32);
    for &b in data {
        seed = seed.wrapping_add(((b as i32) << (salt & 0x0f)) as u64);
        sum1 ^= Rng::rotate_seed(&mut seed);
        salt = salt.wrapping_add(7);
    }
    for &b in data.iter().rev() {
        seed = seed.wrapping_add(((b as i32) << (salt & 0x0f)) as u64);
        sum2 ^= Rng::rotate_seed(&mut seed);
        salt = salt.wrapping_add(3);
    }
    [(sum1 & 0xffff) as u16, (sum2 & 0xffff) as u16]
}

/// A Rust string as a C string of `N` bytes (zero-padded, cut at `N - 1`).
pub fn cstr<const N: usize>(s: &str) -> [u8; N] {
    let mut out = [0u8; N];
    for (o, b) in out.iter_mut().zip(s.bytes().take(N.saturating_sub(1))) {
        *o = b;
    }
    out
}

/// A C string (up to its terminator) as a Rust string.
pub fn cstr_to_string(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| c as char).collect()
}

/// PD's `pak_set_bitflag` (`pak.c:5434`): flag `flagnum` of a byte array.
pub fn pak_set_bitflag(flagnum: u32, bitstream: &mut [u8], set: bool) {
    let byteindex = (flagnum / 8) as usize;
    let mask = 1u8 << (flagnum % 8);
    if set {
        bitstream[byteindex] |= mask;
    } else {
        bitstream[byteindex] &= !mask;
    }
}

/// `pak_has_bitflag` (`pak.c:5446`).
pub fn pak_has_bitflag(flagnum: u32, bitstream: &[u8]) -> bool {
    bitstream[(flagnum / 8) as usize] & (1 << (flagnum % 8)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bits go in MSB first and come back in the same order; a write only
    /// ORs bits in.
    #[test]
    fn bits_round_trip_msb_first() {
        let mut b = SaveBuffer::new();
        b.write_bits(0b101, 3);
        b.write_bits(0x1234, 13);
        b.write_bits(0xffff_ffff, 32);
        b.write_bits(0, 5);
        assert_eq!(b.bitpos, 53);
        assert_eq!(b.bytes[0], 0b1011_0010, "101 then the top of 1_0010_0011_0100");
        b.bitpos = 0;
        assert_eq!(b.read_bits(3), 0b101);
        assert_eq!(b.read_bits(13), 0x1234);
        assert_eq!(b.read_bits(32), 0xffff_ffff);
        assert_eq!(b.read_bits(5), 0);
    }

    /// Names are ten bytes without their break; reading one back adds the
    /// break. An empty name keeps the destination's first byte (PD's bug).
    #[test]
    fn strings_are_ten_bytes_and_empty_names_keep_a_stale_first_byte() {
        let mut b = SaveBuffer::new();
        b.write_string(b"Joanna\n\0");
        b.write_string(b"ABCDEFGHIJKL");
        b.write_string(b"\n");
        assert_eq!(b.bitpos, 3 * 80);
        assert_eq!(&b.bytes[..10], b"Joanna\0\0\0\0");
        assert_eq!(&b.bytes[10..20], b"ABCDEFGHIJ");
        b.bitpos = 0;
        let mut name = [0u8; 15];
        b.read_string(&mut name, true);
        assert_eq!(cstr_to_string(&name), "Joanna\n");
        let mut name = [0u8; 15];
        b.read_string(&mut name, false);
        assert_eq!(cstr_to_string(&name), "ABCDEFGHIJ");
        let mut name = cstr::<15>("Zed\n");
        b.read_string(&mut name, true);
        assert_eq!(cstr_to_string(&name), "Z\n");
    }

    #[test]
    fn bitstrings_and_guids() {
        let mut bits = [0xaau8; 10];
        savebuffer_cstring_to_bitstring(&mut bits, b"Dark\n");
        assert_eq!(&bits, b"Dark\0\0\0\0\0\0");
        let mut c = [0u8; 12];
        savebuffer_bitstring_to_cstring(&bits, &mut c, true);
        assert_eq!(cstr_to_string(&c), "Dark\n");
        let mut b = SaveBuffer::new();
        let g = FileGuid { fileid: 0x55, deviceserial: 0x1aba };
        b.write_guid(&g);
        assert_eq!(b.bitpos, 20);
        b.bitpos = 0;
        assert_eq!(b.read_guid(), g);
    }

    /// `crc_calculate_u16_pair` over nothing is its two untouched sums; over
    /// bytes it changes with every byte and with their order.
    #[test]
    fn the_checksum_sees_every_byte_and_their_order() {
        assert_eq!(crc_calculate_u16_pair(&[]), [0, 0]);
        let a = crc_calculate_u16_pair(&[1, 2, 3, 4]);
        assert_ne!(a, crc_calculate_u16_pair(&[1, 2, 3, 5]));
        assert_ne!(a, crc_calculate_u16_pair(&[4, 3, 2, 1]));
        assert_eq!(a, crc_calculate_u16_pair(&[1, 2, 3, 4]));
    }

    #[test]
    fn bitflags() {
        let mut f = [0u8; 10];
        pak_set_bitflag(0x22, &mut f, true);
        assert_eq!(f[4], 0b100);
        assert!(pak_has_bitflag(0x22, &f));
        pak_set_bitflag(0x22, &mut f, false);
        assert!(!pak_has_bitflag(0x22, &f));
    }
}
