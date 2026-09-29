//! The RSP's audio ABI as Rare's `n_` library drives it (`n_abi.h`,
//! `abi.h`): a 4 KB DMEM of samples and the ops that work on it.
//!
//! The microcode isn't in the decomp. The PC port runs PD's command lists on
//! the CPU (`reference/pd-pcport/port/src/mixer.c`, the scalar paths), and
//! these ops follow it, with three differences, each noted where it is:
//! samples are in logical order (the port's little-endian `i ^ 1` swizzle in
//! `aEnvMixerImpl` pairs a sample with its neighbour's ramp step); `aMix`
//! adds `vmulf(in, gain)` as mupen64plus's RSP HLE does (the port's scalar
//! path also scales the accumulator by 0x7fff/0x8000, its SSE path doesn't);
//! the envelope ramp saturates (the port's `int32_t` wraps, which is UB in C;
//! the RSP's accumulators clamp).
//!
//! DMEM addresses are PD's (`n_synthInternals.h:30`), in bytes. Every address
//! wraps at 4 KB as the RSP's 12-bit DMEM addresses do.

/// `N_AL_DECODER_IN` .. `N_AL_AUX_R_OUT` (`n_synthInternals.h:30-40`).
pub const N_AL_DECODER_IN: i32 = 0;
pub const N_AL_RESAMPLER_OUT: i32 = 0;
pub const N_AL_TEMP_0: i32 = 0;
pub const N_AL_DECODER_OUT: i32 = 368;
pub const N_AL_TEMP_1: i32 = 368;
pub const N_AL_TEMP_2: i32 = 736;
pub const N_AL_MAIN_L_OUT: i32 = 1248;
pub const N_AL_MAIN_R_OUT: i32 = 1616;
pub const N_AL_AUX_L_OUT: i32 = 1984;
pub const N_AL_AUX_R_OUT: i32 = 2352;
pub const N_AL_DIVIDED: i32 = 368;

/// Command flags (`abi.h:56-67`).
pub const A_INIT: u8 = 0x01;
pub const A_CONTINUE: u8 = 0x00;
pub const A_LOOP: u8 = 0x02;
pub const A_LEFT: u8 = 0x02;
pub const A_RIGHT: u8 = 0x00;
pub const A_VOL: u8 = 0x04;
pub const A_RATE: u8 = 0x00;

/// `UNITY_PITCH` (`abi.h:260`) and `MAX_RATIO` (`abi.h:261`).
pub const UNITY_PITCH: i32 = 0x8000;
pub const MAX_RATIO: f32 = 1.99996;

const DMEM_BYTES: usize = 4096;
/// The samples an op works on: `NUM_SAMPLES` (mixer.c:37), one subframe.
const NUM_SAMPLES: i32 = 0xb8;
const NUM_BYTES: i32 = 0x170;

/// `ENVMIX_STATE` as the port's envelope mixer keeps it (mixer.c:470).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnvState {
    pub t: [i32; 2],
    pub rate: [i32; 2],
    pub tgt: [i16; 2],
    pub voldry: i16,
    pub volwet: i16,
}

/// The RSP's audio state: DMEM, the loaded ADPCM codebook, the envelope
/// mixer's volume registers and the loop state `aSetLoop` points at.
pub struct Rsp {
    dmem: Vec<u8>,
    adpcm_table: [i16; 128],
    vol: [i16; 2],
    target: [i16; 2],
    rate: [i32; 2],
    vol_dry: i16,
    vol_wet: i16,
    loop_state: [i16; 16],
}

impl Default for Rsp {
    fn default() -> Rsp {
        Rsp::new()
    }
}

fn clamp16(v: i32) -> i16 {
    v.clamp(-0x8000, 0x7fff) as i16
}

fn round_up_8(v: i32) -> i32 {
    (v + 7) & !7
}

fn round_up_16(v: i32) -> i32 {
    (v + 15) & !15
}

fn round_up_32(v: i32) -> i32 {
    (v + 31) & !31
}

/// `resample_table` (mixer.c:52): the 4-tap interpolator, 64 phases.
#[rustfmt::skip]
const RESAMPLE_TABLE: [[i16; 4]; 64] = {
    const fn h(v: u16) -> i16 { v as i16 }
    [
        [h(0x0c39), h(0x66ad), h(0x0d46), h(0xffdf)], [h(0x0b39), h(0x6696), h(0x0e5f), h(0xffd8)],
        [h(0x0a44), h(0x6669), h(0x0f83), h(0xffd0)], [h(0x095a), h(0x6626), h(0x10b4), h(0xffc8)],
        [h(0x087d), h(0x65cd), h(0x11f0), h(0xffbf)], [h(0x07ab), h(0x655e), h(0x1338), h(0xffb6)],
        [h(0x06e4), h(0x64d9), h(0x148c), h(0xffac)], [h(0x0628), h(0x643f), h(0x15eb), h(0xffa1)],
        [h(0x0577), h(0x638f), h(0x1756), h(0xff96)], [h(0x04d1), h(0x62cb), h(0x18cb), h(0xff8a)],
        [h(0x0435), h(0x61f3), h(0x1a4c), h(0xff7e)], [h(0x03a4), h(0x6106), h(0x1bd7), h(0xff71)],
        [h(0x031c), h(0x6007), h(0x1d6c), h(0xff64)], [h(0x029f), h(0x5ef5), h(0x1f0b), h(0xff56)],
        [h(0x022a), h(0x5dd0), h(0x20b3), h(0xff48)], [h(0x01be), h(0x5c9a), h(0x2264), h(0xff3a)],
        [h(0x015b), h(0x5b53), h(0x241e), h(0xff2c)], [h(0x0101), h(0x59fc), h(0x25e0), h(0xff1e)],
        [h(0x00ae), h(0x5896), h(0x27a9), h(0xff10)], [h(0x0063), h(0x5720), h(0x297a), h(0xff02)],
        [h(0x001f), h(0x559d), h(0x2b50), h(0xfef4)], [h(0xffe2), h(0x540d), h(0x2d2c), h(0xfee8)],
        [h(0xffac), h(0x5270), h(0x2f0d), h(0xfedb)], [h(0xff7c), h(0x50c7), h(0x30f3), h(0xfed0)],
        [h(0xff53), h(0x4f14), h(0x32dc), h(0xfec6)], [h(0xff2e), h(0x4d57), h(0x34c8), h(0xfebd)],
        [h(0xff0f), h(0x4b91), h(0x36b6), h(0xfeb6)], [h(0xfef5), h(0x49c2), h(0x38a5), h(0xfeb0)],
        [h(0xfedf), h(0x47ed), h(0x3a95), h(0xfeac)], [h(0xfece), h(0x4611), h(0x3c85), h(0xfeab)],
        [h(0xfec0), h(0x4430), h(0x3e74), h(0xfeac)], [h(0xfeb6), h(0x424a), h(0x4060), h(0xfeaf)],
        [h(0xfeaf), h(0x4060), h(0x424a), h(0xfeb6)], [h(0xfeac), h(0x3e74), h(0x4430), h(0xfec0)],
        [h(0xfeab), h(0x3c85), h(0x4611), h(0xfece)], [h(0xfeac), h(0x3a95), h(0x47ed), h(0xfedf)],
        [h(0xfeb0), h(0x38a5), h(0x49c2), h(0xfef5)], [h(0xfeb6), h(0x36b6), h(0x4b91), h(0xff0f)],
        [h(0xfebd), h(0x34c8), h(0x4d57), h(0xff2e)], [h(0xfec6), h(0x32dc), h(0x4f14), h(0xff53)],
        [h(0xfed0), h(0x30f3), h(0x50c7), h(0xff7c)], [h(0xfedb), h(0x2f0d), h(0x5270), h(0xffac)],
        [h(0xfee8), h(0x2d2c), h(0x540d), h(0xffe2)], [h(0xfef4), h(0x2b50), h(0x559d), h(0x001f)],
        [h(0xff02), h(0x297a), h(0x5720), h(0x0063)], [h(0xff10), h(0x27a9), h(0x5896), h(0x00ae)],
        [h(0xff1e), h(0x25e0), h(0x59fc), h(0x0101)], [h(0xff2c), h(0x241e), h(0x5b53), h(0x015b)],
        [h(0xff3a), h(0x2264), h(0x5c9a), h(0x01be)], [h(0xff48), h(0x20b3), h(0x5dd0), h(0x022a)],
        [h(0xff56), h(0x1f0b), h(0x5ef5), h(0x029f)], [h(0xff64), h(0x1d6c), h(0x6007), h(0x031c)],
        [h(0xff71), h(0x1bd7), h(0x6106), h(0x03a4)], [h(0xff7e), h(0x1a4c), h(0x61f3), h(0x0435)],
        [h(0xff8a), h(0x18cb), h(0x62cb), h(0x04d1)], [h(0xff96), h(0x1756), h(0x638f), h(0x0577)],
        [h(0xffa1), h(0x15eb), h(0x643f), h(0x0628)], [h(0xffac), h(0x148c), h(0x64d9), h(0x06e4)],
        [h(0xffb6), h(0x1338), h(0x655e), h(0x07ab)], [h(0xffbf), h(0x11f0), h(0x65cd), h(0x087d)],
        [h(0xffc8), h(0x10b4), h(0x6626), h(0x095a)], [h(0xffd0), h(0x0f83), h(0x6669), h(0x0a44)],
        [h(0xffd8), h(0x0e5f), h(0x6696), h(0x0b39)], [h(0xffdf), h(0x0d46), h(0x66ad), h(0x0c39)],
    ]
};

impl Rsp {
    pub fn new() -> Rsp {
        Rsp { dmem: vec![0; DMEM_BYTES], adpcm_table: [0; 128], vol: [0; 2], target: [0; 2], rate: [0; 2], vol_dry: 0, vol_wet: 0, loop_state: [0; 16] }
    }

    fn at(addr: i32) -> usize {
        (addr as usize) & (DMEM_BYTES - 1)
    }

    /// The sample at byte address `addr`.
    pub fn s16(&self, addr: i32) -> i16 {
        let a = Rsp::at(addr);
        i16::from_ne_bytes([self.dmem[a], self.dmem[Rsp::at(addr + 1)]])
    }

    pub fn set_s16(&mut self, addr: i32, v: i16) {
        let b = v.to_ne_bytes();
        self.dmem[Rsp::at(addr)] = b[0];
        self.dmem[Rsp::at(addr + 1)] = b[1];
    }

    fn u8(&self, addr: i32) -> u8 {
        self.dmem[Rsp::at(addr)]
    }

    /// `aClearBuffer` (mixer.c:130).
    pub fn clear_buffer(&mut self, addr: i32, nbytes: i32) {
        for i in 0..round_up_16(nbytes) {
            self.dmem[Rsp::at(addr + i)] = 0;
        }
    }

    /// `aLoadBuffer` of bytes (the sample data; mixer.c:135): `nbytes`
    /// rounded up to 8, past the end of `src` reads 0.
    pub fn load_bytes(&mut self, dst: i32, src: &[u8], start: usize, nbytes: i32) {
        for i in 0..round_up_8(nbytes) {
            self.dmem[Rsp::at(dst + i)] = src.get(start + i as usize).copied().unwrap_or(0);
        }
    }

    /// `aLoadBuffer` of samples (a delay line): `nbytes` rounded up to 8.
    /// SUBST: past the end of `src` the RSP reads whatever DRAM follows / 0.
    pub fn load_samples(&mut self, dst: i32, src: &[i16], start: usize, nbytes: i32) {
        for i in 0..round_up_8(nbytes) / 2 {
            self.set_s16(dst + 2 * i, src.get(start + i as usize).copied().unwrap_or(0));
        }
    }

    /// `aSaveBuffer` into samples (mixer.c:139): `nbytes` rounded up to 8.
    /// Nothing is written past the end of `dst` (PD's section lengths are all
    /// whole multiples of 8 samples, so it never is).
    pub fn save_samples(&self, src: i32, dst: &mut [i16], start: usize, nbytes: i32) {
        for i in 0..round_up_8(nbytes) / 2 {
            if let Some(d) = dst.get_mut(start + i as usize) {
                *d = self.s16(src + 2 * i);
            }
        }
    }

    /// `aLoadADPCM` (mixer.c:143): `nbytes` of codebook.
    pub fn load_adpcm(&mut self, book: &[i16], nbytes: i32) {
        let n = ((nbytes / 2) as usize).min(self.adpcm_table.len()).min(book.len());
        self.adpcm_table[..n].copy_from_slice(&book[..n]);
    }

    /// `aSetLoop` (mixer.c:199): the loop state the next `A_LOOP` decode starts from.
    pub fn set_loop(&mut self, state: &[i16; 16]) {
        self.loop_state = *state;
    }

    /// `aDMEMMove` (mixer.c:195).
    pub fn dmem_move(&mut self, from: i32, to: i32, nbytes: i32) {
        let n = round_up_16(nbytes);
        let tmp: Vec<u8> = (0..n).map(|i| self.u8(from + i)).collect();
        for (i, b) in tmp.into_iter().enumerate() {
            self.dmem[Rsp::at(to + i as i32)] = b;
        }
    }

    /// `aADPCMdecImpl`'s scalar path (mixer.c:203, :337-351): 16 samples of
    /// history (zero, the loop's or `state`), then `nbytes` of output (rounded
    /// to 32: whole frames) decoded from 9-byte frames at `inofs`; `state`
    /// ends as the last 16 samples.
    pub fn adpcm_dec(&mut self, flags: u8, state: &mut [i16; 16], nbytes: i32, inofs: i32, outofs: i32) {
        let mut inp = inofs;
        let mut out = outofs;
        let mut nbytes = round_up_32(nbytes);
        for i in 0..16 {
            let v = if flags & A_INIT != 0 {
                0
            } else if flags & A_LOOP != 0 {
                self.loop_state[i as usize]
            } else {
                state[i as usize]
            };
            self.set_s16(out + 2 * i, v);
        }
        out += 32;
        while nbytes > 0 {
            let hdr = self.u8(inp);
            let shift = (hdr >> 4) as i32;
            // The predictor indexes an 8-entry table (the bank's are all < npredictors).
            let t = ((hdr & 0xf) & 7) as usize * 16;
            inp += 1;
            for _ in 0..2 {
                let prev1 = self.s16(out - 2) as i32;
                let prev2 = self.s16(out - 4) as i32;
                let mut ins = [0i16; 8];
                for j in 0..4 {
                    let b = self.u8(inp) as i32;
                    ins[j * 2] = ((((b >> 4) << 28) >> 28) << shift) as i16;
                    ins[j * 2 + 1] = ((((b & 0xf) << 28) >> 28) << shift) as i16;
                    inp += 1;
                }
                for j in 0..8 {
                    let mut acc = self.adpcm_table[t + j] as i32 * prev2 + self.adpcm_table[t + 8 + j] as i32 * prev1 + ((ins[j] as i32) << 11);
                    for k in 0..j {
                        acc += self.adpcm_table[t + 8 + (j - k) - 1] as i32 * ins[k] as i32;
                    }
                    acc >>= 11;
                    self.set_s16(out, clamp16(acc));
                    out += 2;
                }
            }
            nbytes -= 32;
        }
        for i in 0..16 {
            state[i as usize] = self.s16(out - 32 + 2 * i);
        }
    }

    /// `aResampleImpl`'s scalar path (mixer.c:337): 184 samples out at 0 (or
    /// 0x170 with `outflag`), read from `inofs` at `pitch` (a 1.15 ratio) with
    /// the 4-tap table; `state` carries the last 4 input samples and the phase.
    pub fn resample(&mut self, flags: u8, pitch: u16, state: &mut [i16; 16], inofs: i32, outflag: u8) {
        let mut tmp = [0i16; 16];
        let in_initial = inofs;
        let mut inp = inofs;
        let mut out = if outflag & 3 != 0 { NUM_BYTES } else { 0 };
        let mut nbytes = round_up_16(NUM_BYTES);
        if flags & A_INIT == 0 {
            tmp = *state;
        }
        if flags & 2 != 0 {
            for i in 0..8 {
                self.set_s16(inp - 16 + 2 * i, tmp[8 + i as usize]);
            }
            inp -= (tmp[5] as i32 / 2) * 2;
        }
        inp -= 8;
        let mut acc = tmp[4] as u16 as u32;
        for i in 0..4 {
            self.set_s16(inp + 2 * i, tmp[i as usize]);
        }
        while nbytes > 0 {
            for _ in 0..8 {
                let tbl = &RESAMPLE_TABLE[((acc * 64) >> 16) as usize];
                let mut sample = 0i32;
                for (k, &c) in tbl.iter().enumerate() {
                    sample += (self.s16(inp + 2 * k as i32) as i32 * c as i32 + 0x4000) >> 15;
                }
                self.set_s16(out, clamp16(sample));
                out += 2;
                acc += (pitch as u32) << 1;
                inp += 2 * (acc >> 16) as i32;
                acc %= 0x10000;
            }
            nbytes -= 16;
        }
        state[4] = acc as u16 as i16;
        for i in 0..4 {
            state[i as usize] = self.s16(inp + 2 * i);
        }
        let mut i = ((inp - in_initial) / 2 + 4) & 7;
        inp -= 2 * i;
        if i != 0 {
            i = -8 - i;
        }
        state[5] = i as i16;
        for k in 0..8 {
            state[8 + k as usize] = self.s16(inp + 2 * k);
        }
    }

    /// `aSetVolumeImpl` (mixer.c:586): the envelope mixer's registers.
    pub fn set_volume(&mut self, flags: u8, v: i16, t: i16, r: i16) {
        let rate = ((t as u16 as u32) << 16 | r as u16 as u32) as i32;
        if flags & A_VOL != 0 {
            if flags & A_LEFT != 0 {
                self.vol[0] = v;
                self.vol_dry = t;
                self.vol_wet = r;
            } else {
                self.target[1] = v;
                self.rate[1] = rate;
            }
        } else {
            self.target[0] = v;
            self.rate[0] = rate;
        }
    }

    /// `aEnvMixerImpl` (mixer.c:461): one subframe from `N_AL_RESAMPLER_OUT`
    /// into the main (dry) and aux (wet) outputs, each channel's volume
    /// ramping linearly to its target.
    pub fn env_mixer(&mut self, flags: u8, state: &mut EnvState, rvol: i16) {
        self.vol[1] = rvol;
        let (mut t, mut rate, tgt, voldry, volwet) = if flags & A_INIT != 0 {
            let t = [(self.vol[0] as i32) << 16, (self.vol[1] as i32) << 16];
            let rate = [self.rate[0] >> 3, self.rate[1] >> 3];
            let tgt = [(self.target[0] as i32) << 16, (self.target[1] as i32) << 16];
            (t, rate, tgt, self.vol_dry, self.vol_wet)
        } else {
            (state.t, state.rate, [(state.tgt[0] as i32) << 16, (state.tgt[1] as i32) << 16], state.voldry, state.volwet)
        };
        for i in 0..NUM_SAMPLES {
            let mut vol = [0i32; 2];
            for j in 0..2 {
                t[j] = t[j].saturating_add(rate[j]);
                if (rate[j] <= 0 && t[j] <= tgt[j]) || (rate[j] > 0 && t[j] >= tgt[j]) {
                    t[j] = tgt[j];
                    rate[j] = 0;
                }
                vol[j] = (t[j] >> 16) as i16 as i32;
            }
            let gain = [
                clamp16((vol[0] * voldry as i32 + 0x4000) >> 15) as i32,
                clamp16((vol[1] * voldry as i32 + 0x4000) >> 15) as i32,
                clamp16((vol[0] * volwet as i32 + 0x4000) >> 15) as i32,
                clamp16((vol[1] * volwet as i32 + 0x4000) >> 15) as i32,
            ];
            let insamp = self.s16(N_AL_RESAMPLER_OUT + 2 * i) as i32;
            for (k, base) in [N_AL_MAIN_L_OUT, N_AL_MAIN_R_OUT, N_AL_AUX_L_OUT, N_AL_AUX_R_OUT].into_iter().enumerate() {
                let a = base + 2 * i;
                let v = clamp16(self.s16(a) as i32 + ((insamp * gain[k]) >> 15));
                self.set_s16(a, v);
            }
        }
        state.t = t;
        state.rate = rate;
        state.tgt = [(tgt[0] >> 16) as i16, (tgt[1] >> 16) as i16];
        state.voldry = voldry;
        state.volwet = volwet;
    }

    /// `aMixImpl` (mixer.c:519): a subframe from `from` added into `to` at
    /// `gain` (1.15; 0x8000 is -1). The product is the RSP's `vmulf`,
    /// rounded (mupen64plus `alist_mix`).
    pub fn mix(&mut self, gain: i16, from: i32, to: i32) {
        for i in 0..round_up_16(NUM_BYTES) / 2 {
            let x = self.s16(from + 2 * i) as i32;
            let prod = (x * gain as i32 * 2 + 0x8000) >> 16;
            let v = clamp16(self.s16(to + 2 * i) as i32 + prod);
            self.set_s16(to + 2 * i, v);
        }
    }

    /// `aInterleaveImpl` (mixer.c:147): the main outputs interleaved to 0.
    /// The port interleaves 192 (the count rounded to 16); the 8 past a
    /// subframe land in `N_AL_TEMP_2`, which nothing reads before writing.
    pub fn interleave(&mut self) {
        let mut tmp = Vec::with_capacity(2 * NUM_SAMPLES as usize);
        for i in 0..NUM_SAMPLES {
            tmp.push(self.s16(N_AL_MAIN_L_OUT + 2 * i));
            tmp.push(self.s16(N_AL_MAIN_R_OUT + 2 * i));
        }
        for (i, v) in tmp.into_iter().enumerate() {
            self.set_s16(2 * i as i32, v);
        }
    }

    /// `aSaveBuffer` of the interleaved subframe (`n_alSavePull`): 184 stereo frames.
    pub fn save_frames(&self, out: &mut Vec<[i16; 2]>) {
        for i in 0..NUM_SAMPLES {
            out.push([self.s16(4 * i), self.s16(4 * i + 2)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_is_vmulf() {
        let mut r = Rsp::new();
        for i in 0..184 {
            r.set_s16(N_AL_AUX_L_OUT + 2 * i, 1000);
            r.set_s16(N_AL_MAIN_L_OUT + 2 * i, 10);
        }
        r.mix(0x7fff, N_AL_AUX_L_OUT, N_AL_MAIN_L_OUT);
        assert_eq!(r.s16(N_AL_MAIN_L_OUT), 1010);
        r.mix(0x8000u16 as i16, N_AL_AUX_L_OUT, N_AL_MAIN_L_OUT);
        assert_eq!(r.s16(N_AL_MAIN_L_OUT), 10);
        r.mix(0x5a82, N_AL_AUX_L_OUT, N_AL_MAIN_L_OUT);
        assert_eq!(r.s16(N_AL_MAIN_L_OUT), 10 + 707);
    }

    /// Unity pitch passes the input through the 4-tap filter's phase-0 row,
    /// delayed by the 4 history samples.
    #[test]
    fn resample_at_unity_follows_the_input() {
        let mut r = Rsp::new();
        for i in 0..400 {
            r.set_s16(N_AL_DECODER_OUT + 2 * i, (i * 50) as i16);
        }
        let mut st = [0i16; 16];
        r.resample(A_INIT, 0x8000, &mut st, N_AL_DECODER_OUT, 0);
        // Phase 0 weights 0x0c39, 0x66ad, 0x0d46, -0x21 around sample n-3.
        let y = r.s16(2 * 100) as i32;
        let x = 50 * (100 - 3);
        assert!((y - x).abs() < 60, "{y} vs {x}");
        assert_eq!(st[4], 0, "unity keeps phase 0");
    }

    #[test]
    fn the_envelope_ramps_to_its_target() {
        let mut r = Rsp::new();
        for i in 0..184 {
            r.set_s16(2 * i, 0x4000);
        }
        r.set_volume(A_LEFT | A_VOL, 0, 0x7fff, 0);
        r.set_volume(A_RIGHT | A_VOL, 0x7fff, 0x0100, 0);
        r.set_volume(A_RATE, 0x7fff, 0x0100, 0);
        let mut st = EnvState::default();
        r.env_mixer(A_INIT, &mut st, 0);
        let first = r.s16(N_AL_MAIN_L_OUT);
        let last = r.s16(N_AL_MAIN_L_OUT + 2 * 183);
        assert!(first < last && last > 0, "{first} {last}");
        assert_eq!(r.s16(N_AL_AUX_L_OUT + 2 * 183), 0, "no wet");
    }
}
