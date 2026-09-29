//! An FX bus's reverb: `n_alFxNew` (`n_drvrNew.c:149`) and Rare's
//! `n_alFxPull` (`n_reverb.c:13`), the SGI delay-line reverb with a second
//! line for stereo (`var8009c344`, set by `alSurround_ReverbSetup`).
//!
//! A bus's delay lines are `length` samples each, written 184 at a time at
//! `input`; each section reads its `input` and `output` taps behind it,
//! feeds back and forward, and adds its output at `gain` into the bus's
//! output. A section with a chorus rate reads its output tap through a
//! resampler swept by a triangle (`_doModFunc`).

use super::abi::*;
use super::synth::FIXED_SAMPLE;

/// `RANGE` (`n_reverb.c:5`).
const RANGE: f32 = 2.0;
/// `CONVERT` (`n_reverb.c:176`): 120000 / ln 2.
const CONVERT: f32 = 173_123.4;

/// A section's chorus resampler (`ALResampler` in `ALDelay`).
#[derive(Clone, Debug, Default)]
pub struct DelayResampler {
    pub state: [[i16; 16]; 2],
    pub delta: f32,
    pub first: i32,
}

/// A section's low-pass (`ALLowPass`): `n_alFxInitlpfilter`'s coefficients.
#[derive(Clone, Debug, Default)]
pub struct LowPass {
    pub fc: i16,
    pub fgain: i16,
    pub fccoef: [i16; 16],
    pub first: i32,
}

/// `ALDelay` (`synthInternals.h:203`).
#[derive(Clone, Debug, Default)]
pub struct Delay {
    pub input: u32,
    pub output: u32,
    pub ffcoef: i16,
    pub fbcoef: i16,
    pub gain: i16,
    pub rsinc: f32,
    pub rsval: f32,
    pub rsdelta: i32,
    pub rsgain: f32,
    pub lp: Option<LowPass>,
    pub rs: Option<DelayResampler>,
}

/// `ALFx` (`synthInternals.h:218`): the sections and the two delay lines.
#[derive(Clone, Debug)]
pub struct Fx {
    pub length: u32,
    pub delay: Vec<Delay>,
    pub base: [Vec<i16>; 2],
    /// Each line's write position (`r->input[j] - r->base[j]`, `0..=length`).
    pub input: [i64; 2],
}

/// `n_alFxInitlpfilter` (`n_drvrNew.c:65`).
fn init_lpfilter(lp: &mut LowPass) {
    const SCALE: f32 = 16384.0;
    let temp = (lp.fc as f32 * SCALE) as i32;
    let fc = (temp >> 15) as i16;
    lp.fgain = (SCALE as i32 - fc as i32) as i16;
    lp.first = 0;
    lp.fccoef = [0; 16];
    lp.fccoef[8] = fc;
    let ffc = fc as f32 / SCALE;
    let mut fcoef = ffc;
    for i in 9..16 {
        fcoef *= ffc;
        lp.fccoef[i] = (fcoef * SCALE) as i16;
    }
}

impl Fx {
    /// `n_alFxNew` for `AL_FX_CUSTOM` params: `{sections, length, then per
    /// section input, output, fbcoef, ffcoef, gain, chorus rate, chorus
    /// depth, filter coef}`, lengths in samples.
    pub fn new(param: &[i32], output_rate: i32) -> Fx {
        let mut j = 0;
        let mut next = || {
            let v = param.get(j).copied().unwrap_or(0);
            j += 1;
            v
        };
        let section_count = next() as usize;
        let length = next() as u32;
        let mut delay = Vec::with_capacity(section_count);
        for _ in 0..section_count {
            let mut d = Delay { input: next() as u32, output: next() as u32, fbcoef: next() as i16, ffcoef: next() as i16, gain: next() as i16, ..Delay::default() };
            let rate = next();
            let depth = next();
            if rate != 0 {
                d.rsinc = ((rate as f32 / 1000.0) * RANGE) / output_rate as f32;
                d.rsgain = (depth as f32 / CONVERT) * d.output.wrapping_sub(d.input) as f32;
                d.rsval = 1.0;
                d.rsdelta = 0;
                d.rs = Some(DelayResampler { first: 1, ..DelayResampler::default() });
            }
            let fc = next();
            if fc != 0 {
                let mut lp = LowPass { fc: fc as i16, ..LowPass::default() };
                init_lpfilter(&mut lp);
                d.lp = Some(lp);
            }
            delay.push(d);
        }
        Fx { length, delay, base: [vec![0; length as usize], vec![0; length as usize]], input: [0, 0] }
    }

    /// A position in line `j` as a DRAM address in samples: the two lines
    /// are allocated back to back (`n_alFxNew`), which is what the pull's
    /// `in_ptr == prev_out_ptr` compares.
    fn addr(&self, j: usize, pos: i64) -> i64 {
        j as i64 * self.length as i64 + pos
    }

    /// `_n_loadBuffer` (`n_reverb.c:292`): `count` samples of line `j` from
    /// `curr` (wrapped) into DMEM at `buff`.
    fn load_buffer(&self, rsp: &mut Rsp, j: usize, mut curr: i64, buff: i32, count: i32) {
        let len = self.length as i64;
        if curr < 0 {
            curr += len;
        }
        let updated = curr + count as i64;
        if updated > len {
            let after_end = (updated - len) as i32;
            let before_end = (len - curr) as i32;
            rsp.load_samples(buff, &self.base[j], curr as usize, before_end << 1);
            rsp.load_samples(buff + (before_end << 1), &self.base[j], 0, after_end << 1);
        } else {
            rsp.load_samples(buff, &self.base[j], curr as usize, count << 1);
        }
    }

    /// `_n_saveBuffer` (`n_reverb.c:321`): a subframe from `buff` into line
    /// `j` at `curr` (wrapped).
    fn save_buffer(&mut self, rsp: &Rsp, j: usize, mut curr: i64, buff: i32) {
        let len = self.length as i64;
        if curr < 0 {
            curr += len;
        }
        let updated = curr + FIXED_SAMPLE as i64;
        if updated > len {
            let after_end = (updated - len) as i32;
            let before_end = (len - curr) as i32;
            rsp.save_samples(buff, &mut self.base[j], curr as usize, before_end << 1);
            rsp.save_samples(buff + (before_end << 1), &mut self.base[j], 0, after_end << 1);
        } else {
            rsp.save_samples(buff, &mut self.base[j], curr as usize, FIXED_SAMPLE << 1);
        }
    }

    /// `_doModFunc` (`n_reverb.c:358`): the chorus sweep.
    fn do_mod_func(d: &mut Delay, count: i32) -> f32 {
        d.rsval += d.rsinc * count as f32;
        d.rsval = if d.rsval > RANGE { d.rsval - RANGE * 2.0 } else { d.rsval };
        let mut val = d.rsval.abs();
        val -= RANGE / 2.0;
        d.rsgain * val
    }

    /// `_n_loadOutputBuffer` (`n_reverb.c:246`): section `i`'s output tap of
    /// line `j` into DMEM at `buff`, through its chorus resampler if it has one.
    fn load_output_buffer(&mut self, rsp: &mut Rsp, i: usize, j: usize, buff: i32) {
        let input = self.input[j];
        let d = &mut self.delay[i];
        if d.rs.is_some() {
            let rbuff = N_AL_TEMP_2;
            let incount = FIXED_SAMPLE;
            let length = d.output.wrapping_sub(d.input) as i32;
            let mut delta = Fx::do_mod_func(d, incount);
            delta /= length as f32;
            delta = (delta * UNITY_PITCH as f32) as i32 as f32;
            delta /= UNITY_PITCH as f32;
            let fratio = 1.0 - delta;
            let rs = d.rs.as_mut().unwrap();
            let fincount = rs.delta + fratio * incount as f32;
            let count = fincount as i32;
            rs.delta = fincount - count as f32;
            let out_ptr = input - (d.output as i64 - d.rsdelta as i64);
            // (intptr_t)out_ptr & 7 >> 1 with 16-byte aligned lines.
            let ramalign = (out_ptr & 3) as i32;
            let ratio = (fratio * UNITY_PITCH as f32) as i32;
            let first = rs.first;
            rs.first = 0;
            d.rsdelta += count - incount;
            let mut state = rs.state[j];
            self.load_buffer(rsp, j, out_ptr - ramalign as i64, rbuff, count + ramalign);
            rsp.resample(first as u8, ratio as u16, &mut state, rbuff + (ramalign << 1), (buff >> 8) as u8);
            self.delay[i].rs.as_mut().unwrap().state[j] = state;
        } else {
            let out_ptr = input - d.output as i64;
            self.load_buffer(rsp, j, out_ptr, buff, FIXED_SAMPLE);
        }
    }

    /// `n_alFxPull`'s reverb, after `n_alAuxBusPull` has mixed the bus's
    /// voices into `N_AL_AUX_L_OUT` / `_R_OUT`: the lines take this subframe,
    /// the sections run, and the reverb's output is left for the main bus in
    /// `N_AL_AUX_L_OUT` (and, in stereo, the left line's added to the main
    /// left here).
    pub fn pull(&mut self, rsp: &mut Rsp, stereo: bool, c346: bool) {
        let input = N_AL_AUX_L_OUT;
        let output = N_AL_AUX_R_OUT;
        let mut buff1 = N_AL_TEMP_0;
        let mut buff2 = N_AL_TEMP_1;
        if !stereo {
            rsp.mix(0xc000u16 as i16, N_AL_AUX_L_OUT, input);
            rsp.mix(0x4000, N_AL_AUX_R_OUT, input);
        }
        let in0 = self.input[0];
        self.save_buffer(rsp, 0, in0, input);
        if stereo {
            let in1 = self.input[1];
            self.save_buffer(rsp, 1, in1, 0x930);
        }
        let mut prev_out_ptr: Option<i64> = None;
        for j in 0..=(stereo as usize) {
            rsp.clear_buffer(output, FIXED_SAMPLE << 1);
            for i in 0..self.delay.len() {
                let inp = self.input[j];
                let (din, dout) = (self.delay[i].input as i64, self.delay[i].output as i64);
                let in_ptr = inp - din;
                let out_ptr = inp - dout;
                if c346 && stereo {
                    let d = &mut self.delay[i];
                    d.ffcoef = d.ffcoef.wrapping_neg();
                    d.fbcoef = d.fbcoef.wrapping_neg();
                }
                if Some(self.addr(j, in_ptr)) == prev_out_ptr {
                    std::mem::swap(&mut buff1, &mut buff2);
                } else {
                    self.load_buffer(rsp, j, in_ptr, buff1, FIXED_SAMPLE);
                }
                self.load_output_buffer(rsp, i, j, buff2);
                let d = &self.delay[i];
                let (ff, fb, gain, has_rs, has_lp) = (d.ffcoef, d.fbcoef, d.gain, d.rs.is_some(), d.lp.is_some());
                if ff != 0 {
                    rsp.mix(ff, buff1, buff2);
                    if !has_rs && !has_lp {
                        self.save_buffer(rsp, j, out_ptr, buff2);
                    }
                }
                if fb != 0 {
                    rsp.mix(fb, buff2, buff1);
                    self.save_buffer(rsp, j, in_ptr, buff1);
                }
                // SUBST: a section's low-pass is Rare's pole filter
                // (_n_filterBuffer, n_aPoleFilter), not emulated / skipped;
                // no section in PD's 8 MB reverbs has one.
                if !has_rs {
                    self.save_buffer(rsp, j, out_ptr, buff2);
                }
                if gain != 0 {
                    if stereo {
                        rsp.mix(gain, buff2, output);
                    } else {
                        let g = ((gain as f32 * 1.414_199_948_310_9) as i64 as u32).min(0x7fff);
                        rsp.mix(g as i16, buff2, output);
                    }
                }
                prev_out_ptr = Some(self.addr(j, inp + dout));
            }
            if stereo && j == 0 {
                let in1 = self.input[1];
                self.load_buffer(rsp, 1, in1, input, FIXED_SAMPLE);
                rsp.mix(0x5a82, output, if c346 { 0x650 } else { 0x4e0 });
            }
            rsp.dmem_move(output, N_AL_AUX_L_OUT, FIXED_SAMPLE << 1);
            self.input[j] += FIXED_SAMPLE as i64;
            if self.input[j] > self.length as i64 {
                self.input[j] -= self.length as i64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click into bus 1's one section (0 in, 55 ms out, 13108 feedback,
    /// 29493 gain) comes back 2200 samples later, then again, quieter.
    #[test]
    fn a_click_echoes_at_the_sections_delay() {
        let mut fx = Fx::new(&[1, 2640, 0, 2200, 13108, 0, 29493, 0, 0, 0], 22018);
        let mut rsp = Rsp::new();
        let mut out = Vec::new();
        for sub in 0..40 {
            rsp.clear_buffer(N_AL_AUX_L_OUT, 736);
            rsp.clear_buffer(N_AL_MAIN_L_OUT, 736);
            if sub == 0 {
                rsp.set_s16(N_AL_AUX_L_OUT, 20000);
            }
            fx.pull(&mut rsp, true, false);
            for i in 0..184 {
                out.push(rsp.s16(N_AL_MAIN_L_OUT + 2 * i));
            }
        }
        let peak = |from: usize, to: usize| (from..to).max_by_key(|&i| out[i].unsigned_abs()).unwrap();
        let first = peak(1, 3000);
        assert_eq!(first, 2200, "first echo");
        let second = peak(3000, 7000);
        assert_eq!(second, 4400, "second echo");
        assert!(out[second].unsigned_abs() < out[first].unsigned_abs());
    }

    #[test]
    fn pds_bus0_has_a_chorus_on_its_last_section() {
        let fx = Fx::new(&super::super::synth::PARAMS_BUS0_8MB, 22018);
        assert_eq!((fx.length, fx.delay.len()), (7040, 8));
        let d = &fx.delay[7];
        assert!(d.rs.is_some() && d.lp.is_none());
        assert!((d.rsgain - 10.0 / CONVERT * 6512.0).abs() < 1e-3);
    }
}
