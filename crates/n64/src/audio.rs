//! The N64 output chain and the 90s CRT TV speaker, as a pure stereo processor:
//! the audio twin of the VI/CRT video chain (`gpu::video`). Two halves, each
//! switchable on its own:
//!
//! * **N64:** PD mixes at 22020 Hz (`audiomgr.c:64`, `osAiSetFrequency(22020)`).
//!   The mix is sampled at that clock and held (the DAC's zero-order hold), then
//!   optionally smoothed by a reconstruction low-pass. Nothing above 11010 Hz
//!   survives. That only bites on the 22 kHz and pitched-up sounds, because most
//!   of PD's samples are 11–16 kHz to begin with.
//! * **TV:** a small, overdriven speaker in a plastic box. The chain is: mono →
//!   speaker (low cut with a cone hump, a nasal mid bump, a high cut) → cabinet
//!   resonance (a damped feedback comb) → compressor → soft clip → volume.
//!   There is also an optional flyback whine at the NTSC line rate. It comes from
//!   the tube's transformer, not the speaker, so it is added after the speaker
//!   filters.
//!
//! [`TvChain`] is plain Rust, so tests and the `pd_tools` audio renderer run it
//! offline. The game adapts it to an engine DSP track (`pd_game::tvaudio`).
//! Everything off = the chain returns its input untouched, and the game routes
//! voices to the main track then, so the normal path is the plain one.
//!
//! Source: the old repo's `pd_guns/tvaudio.rs` (spike `spike/crt-tv-audio`; its
//! `TvTrack`/`TvLink` half and their test are in `pd_game::tvaudio`).

use std::f64::consts::PI;

/// PD's audio interface rate (`audiomgr.c:64`).
pub const N64_RATE: f64 = 22020.0;
/// The NTSC horizontal rate, 4.5 MHz / 286: the flyback transformer's whine.
pub const FLYBACK_HZ: f64 = 4_500_000.0 / 286.0;
/// The N64 half's anti-alias and reconstruction low-pass (8th order, so each
/// is −6 dB at 11 kHz and −21 dB at 14 kHz; 9 kHz loses 0.5 dB).
const N64_LP_HZ: f64 = 10_300.0;
/// Cabinet round trip: a ~30 cm plastic box, c / 2L ≈ 570 Hz.
const CABINET_MS: f64 = 1.75;
/// The damping in the cabinet's feedback loop (plastic is not a good reflector).
const CABINET_DAMP_HZ: f64 = 2500.0;

/// What the chain does. The default is everything off, with the 14" portable's
/// speaker ready for when the TV half is switched on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioSettings {
    /// The N64 half: sample the mix at 22020 Hz.
    pub n64: bool,
    /// Low-pass after the DAC hold. Off = the raw hold, whose images (22020 − f)
    /// fizz above 11 kHz. Whether the N64 board filters its DAC is not in the
    /// decomp, so this is unverified.
    pub dac_filter: bool,
    /// The TV half: the speaker chain below.
    pub tv: bool,
    /// Portables had one speaker.
    pub mono: bool,
    /// Speaker low cut (Hz): a tiny cone has no bass.
    pub low_cut: f32,
    /// Speaker high cut (Hz).
    pub high_cut: f32,
    /// The boxy, nasal mid bump: centre (Hz) and gain (dB).
    pub box_hz: f32,
    pub box_db: f32,
    /// Amp overdrive, 0..1: the clip ceiling drops from 0 to −18 dBFS. It is
    /// linear below half the ceiling, so it only bites when loud.
    pub drive: f32,
    /// Compressor amount, 0..1 (threshold −6 → −30 dB, ratio 1 → 8).
    pub squash: f32,
    /// Cabinet resonance, 0..1 (the comb's feedback).
    pub cabinet: f32,
    /// Wet/dry, 0..1: how much of the TV chain you hear.
    pub mix: f32,
    /// Output level after the TV chain (dB).
    pub volume_db: f32,
    /// The 15.734 kHz flyback whine (TV half only).
    pub whine: bool,
    pub whine_db: f32,
}

impl AudioSettings {
    /// Whether anything is on (off = voices go to the main track).
    pub fn active(&self) -> bool {
        self.n64 || self.tv
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        let mut s = AudioSettings {
            n64: false,
            dac_filter: true,
            tv: false,
            mono: true,
            low_cut: 250.0,
            high_cut: 5000.0,
            box_hz: 2100.0,
            box_db: 6.0,
            drive: 0.35,
            squash: 0.4,
            cabinet: 0.5,
            mix: 1.0,
            volume_db: 4.0,
            whine: false,
            whine_db: -42.0,
        };
        SpeakerPreset::Portable.apply(&mut s);
        s
    }
}

/// From subtle to awful. Presets set the speaker, not the on/off switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeakerPreset {
    /// A 25" console set with two side-firing speakers.
    BigSet,
    /// A 14" portable with one small speaker (the default).
    Portable,
    /// A 5" black-and-white kitchen set.
    Kitchen,
}

impl SpeakerPreset {
    pub const ALL: [SpeakerPreset; 3] = [SpeakerPreset::BigSet, SpeakerPreset::Portable, SpeakerPreset::Kitchen];
    pub fn label(self) -> &'static str {
        match self {
            SpeakerPreset::BigSet => "big set",
            SpeakerPreset::Portable => "14\" portable",
            SpeakerPreset::Kitchen => "kitchen B&W",
        }
    }
    pub fn apply(self, s: &mut AudioSettings) {
        // (mono, low, high, box Hz, box dB, drive, squash, cabinet, volume dB)
        // The volume is make-up gain: it brings the A-weighted loudness of eight
        // PD guns and clicks back to what they measure with everything off
        // (`pd_tv_audio`, input at 0.8), so an A/B compares tone, not level.
        // Peaks then stay under 0 dBFS (the rail sits at −18·drive dB).
        let p = match self {
            SpeakerPreset::BigSet => (false, 110.0, 9000.0, 2500.0, 2.5, 0.15, 0.25, 0.3, 2.5),
            SpeakerPreset::Portable => (true, 250.0, 5000.0, 2100.0, 6.0, 0.35, 0.4, 0.5, 4.0),
            SpeakerPreset::Kitchen => (true, 450.0, 3500.0, 1700.0, 9.0, 0.7, 0.6, 0.8, 7.5),
        };
        (s.mono, s.low_cut, s.high_cut, s.box_hz, s.box_db, s.drive, s.squash, s.cabinet, s.volume_db) = p;
        s.mix = 1.0;
    }
}

fn db_to_amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// An RBJ-cookbook biquad, transposed direct form II.
#[derive(Clone, Copy, Debug)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 3]) -> Self {
        Biquad { b0: b[0] / a[0], b1: b[1] / a[0], b2: b[2] / a[0], a1: a[1] / a[0], a2: a[2] / a[0], z1: 0.0, z2: 0.0 }
    }
    fn w0(fs: f64, f: f64) -> f64 {
        2.0 * PI * f.clamp(10.0, 0.45 * fs) / fs
    }
    fn lowpass(fs: f64, f: f64, q: f64) -> Self {
        let w = Self::w0(fs, f);
        let (c, al) = (w.cos(), w.sin() / (2.0 * q));
        Self::new([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }
    fn highpass(fs: f64, f: f64, q: f64) -> Self {
        let w = Self::w0(fs, f);
        let (c, al) = (w.cos(), w.sin() / (2.0 * q));
        Self::new([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }
    fn peak(fs: f64, f: f64, q: f64, db: f64) -> Self {
        let w = Self::w0(fs, f);
        let (c, al, a) = (w.cos(), w.sin() / (2.0 * q), 10f64.powf(db / 40.0));
        Self::new([1.0 + al * a, -2.0 * c, 1.0 - al * a], [1.0 + al / a, -2.0 * c, 1.0 - al / a])
    }
    /// New coefficients, same state (so a slider drag doesn't click to silence).
    fn retune(&mut self, other: Biquad) {
        *self = Biquad { z1: self.z1, z2: self.z2, ..other };
    }
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
    fn flush(&mut self) {
        flush(&mut self.z1);
        flush(&mut self.z2);
    }
}

/// Zero a decayed state before it goes denormal (slow on x86).
fn flush(z: &mut f64) {
    if z.abs() < 1e-25 {
        *z = 0.0;
    }
}

/// A one-pole low-pass (the high-pass is `x - lp(x)`).
#[derive(Clone, Copy, Debug, Default)]
struct OnePole {
    a: f64,
    z: f64,
}

impl OnePole {
    fn set(&mut self, fs: f64, f: f64) {
        self.a = 1.0 - (-2.0 * PI * f.clamp(1.0, 0.45 * fs) / fs).exp();
    }
    #[inline]
    fn lp(&mut self, x: f64) -> f64 {
        self.z += self.a * (x - self.z);
        self.z
    }
    #[inline]
    fn hp(&mut self, x: f64) -> f64 {
        x - self.lp(x)
    }
}

/// Butterworth 8th order as four biquads.
fn butter8_lp(fs: f64, f: f64) -> [Biquad; 4] {
    [0.509_795_6, 0.601_344_9, 0.899_976_2, 2.562_915_4].map(|q| Biquad::lowpass(fs, f, q))
}

#[inline]
fn run_all(bs: &mut [Biquad; 4], x: f64) -> f64 {
    bs.iter_mut().fold(x, |x, b| b.run(x))
}

/// One channel of the N64 half: anti-alias → sample at 22020 → hold → (DAC filter).
#[derive(Clone, Debug)]
struct N64Stage {
    aa: [Biquad; 4],
    dac: [Biquad; 4],
    /// 22020 / fs: how far the N64 clock advances per output sample.
    step: f64,
    phase: f64,
    prev_in: f64,
    held: f64,
}

impl N64Stage {
    fn new(fs: f64) -> Self {
        N64Stage { aa: butter8_lp(fs, N64_LP_HZ), dac: butter8_lp(fs, N64_LP_HZ), step: N64_RATE / fs, phase: 0.0, prev_in: 0.0, held: 0.0 }
    }
    #[inline]
    fn run(&mut self, x: f64, dac_filter: bool) -> f64 {
        let x = run_all(&mut self.aa, x);
        let out = if self.step >= 1.0 {
            x // the device is slower than the N64: nothing to hold
        } else {
            let p0 = self.phase;
            self.phase += self.step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                // The N64 tick falls at `t` through this output sample's interval.
                // Take the sample there, and output the staircase's average over the
                // interval (old value for `t`, new for the rest), which keeps the
                // hold's timing exact instead of snapping edges to the device grid.
                let t = (1.0 - p0) / self.step;
                let sample = self.prev_in + (x - self.prev_in) * t;
                let avg = self.held * t + sample * (1.0 - t);
                self.held = sample;
                avg
            } else {
                self.held
            }
        };
        self.prev_in = x;
        if dac_filter {
            run_all(&mut self.dac, out)
        } else {
            out
        }
    }
    fn flush(&mut self) {
        for b in self.aa.iter_mut().chain(self.dac.iter_mut()) {
            b.flush();
        }
    }
}

/// One channel of the TV half.
#[derive(Clone, Debug)]
struct TvStage {
    hp: Biquad,
    coupling: OnePole,
    bump: Biquad,
    lp: Biquad,
    breakup: OnePole,
    comb: Vec<f64>,
    comb_pos: usize,
    comb_damp: OnePole,
    env: f64,
}

impl TvStage {
    fn new(fs: f64) -> Self {
        let n = ((CABINET_MS * 1e-3 * fs).round() as usize).max(1);
        let mut st = TvStage {
            hp: Biquad::highpass(fs, 250.0, 0.9),
            coupling: OnePole::default(),
            bump: Biquad::peak(fs, 2000.0, 1.0, 0.0),
            lp: Biquad::lowpass(fs, 5000.0, 0.707),
            breakup: OnePole::default(),
            comb: vec![0.0; n],
            comb_pos: 0,
            comb_damp: OnePole::default(),
            env: 0.0,
        };
        st.comb_damp.set(fs, CABINET_DAMP_HZ);
        st
    }
    fn retune(&mut self, fs: f64, s: &AudioSettings) {
        let (lo, hi) = (s.low_cut as f64, s.high_cut as f64);
        // Q 0.9: a little hump at the cone's resonance, as small drivers have.
        self.hp.retune(Biquad::highpass(fs, lo, 0.9));
        // The amp's coupling cap: another 6 dB/oct under the cone.
        self.coupling.set(fs, lo * 0.5);
        self.bump.retune(Biquad::peak(fs, s.box_hz as f64, 1.0, s.box_db as f64));
        self.lp.retune(Biquad::lowpass(fs, hi, 0.707));
        // Cone break-up: the top falls off faster than 12 dB/oct.
        self.breakup.set(fs, hi * 1.5);
    }
    #[inline]
    fn run(&mut self, x: f64, k: &Coeffs) -> f64 {
        // The speaker.
        let x = self.coupling.hp(x);
        let x = self.hp.run(x);
        let x = self.bump.run(x);
        let x = self.lp.run(x);
        // The cabinet: y = x + fb · damp(y[n − D]).
        let back = self.comb_damp.lp(self.comb[self.comb_pos]);
        let y = x + k.feedback * back;
        self.comb[self.comb_pos] = y;
        self.comb_pos = (self.comb_pos + 1) % self.comb.len();
        // The compressor (feed-forward, peak detector).
        let a = y.abs();
        let c = if a > self.env { k.attack } else { k.release };
        self.env = a + c * (self.env - a);
        let gain = if k.ratio > 1.0 && self.env > k.threshold {
            (k.threshold / self.env).powf(1.0 - 1.0 / k.ratio)
        } else {
            1.0
        };
        // The rail, last. Clipping *before* the speaker's high-pass (the amp's
        // place in the circuit) lets the high-pass rebuild spikes on every flat
        // top: the renders' crest factor went up 9–14 dB, the opposite of a
        // squashed little set. Here it bounds what comes out, like a cone at its
        // excursion limit. The cone's break-up low-pass comes after it, so the
        // clip's harmonics don't fizz above the speaker's range (a one-pole has
        // no overshoot, so the peaks stay bounded).
        self.breakup.lp(soft_clip(y * gain, k.ceiling)) * k.volume
    }
    fn flush(&mut self) {
        self.hp.flush();
        self.bump.flush();
        self.lp.flush();
        flush(&mut self.coupling.z);
        flush(&mut self.breakup.z);
        flush(&mut self.comb_damp.z);
        flush(&mut self.env);
        for z in &mut self.comb {
            flush(z);
        }
    }
}

/// The amp running out of rail: linear up to half the `ceiling`, then a tanh
/// knee that never passes the ceiling (slope 1 at the knee, so no kink).
#[inline]
fn soft_clip(x: f64, ceiling: f64) -> f64 {
    let knee = 0.5 * ceiling;
    let a = x.abs();
    if a <= knee {
        x
    } else {
        let room = ceiling - knee;
        (knee + room * ((a - knee) / room).tanh()).copysign(x)
    }
}

/// Per-settings constants for [`TvStage::run`].
#[derive(Clone, Copy, Debug)]
struct Coeffs {
    ceiling: f64,
    feedback: f64,
    attack: f64,
    release: f64,
    threshold: f64,
    ratio: f64,
    volume: f64,
    mix: f64,
    whine_step: f64,
    whine_amp: f64,
}

impl Coeffs {
    fn new(fs: f64, s: &AudioSettings) -> Self {
        let squash = s.squash.clamp(0.0, 1.0) as f64;
        Coeffs {
            ceiling: db_to_amp(-18.0 * s.drive.clamp(0.0, 1.0) as f64),
            feedback: 0.45 * s.cabinet.clamp(0.0, 1.0) as f64,
            attack: (-1.0 / (0.002 * fs)).exp(),
            release: (-1.0 / (0.150 * fs)).exp(),
            threshold: db_to_amp(-6.0 - 24.0 * squash),
            ratio: 1.0 + 7.0 * squash,
            volume: db_to_amp(s.volume_db as f64),
            mix: s.mix.clamp(0.0, 1.0) as f64,
            whine_step: FLYBACK_HZ / fs,
            // A whine above the device's Nyquist would alias: drop it.
            whine_amp: if s.whine && FLYBACK_HZ < 0.5 * fs { db_to_amp(s.whine_db as f64) } else { 0.0 },
        }
    }
}

/// The whole chain, both halves, for one stereo stream.
#[derive(Clone, Debug)]
pub struct TvChain {
    fs: f64,
    s: AudioSettings,
    k: Coeffs,
    n64: [N64Stage; 2],
    tv: [TvStage; 2],
    whine_phase: f64,
}

impl TvChain {
    pub fn new(sample_rate: u32, s: AudioSettings) -> Self {
        let fs = sample_rate.max(1) as f64;
        let mut c = TvChain {
            fs,
            s,
            k: Coeffs::new(fs, &s),
            n64: [N64Stage::new(fs), N64Stage::new(fs)],
            tv: [TvStage::new(fs), TvStage::new(fs)],
            whine_phase: 0.0,
        };
        c.set(s);
        c
    }

    pub fn settings(&self) -> AudioSettings {
        self.s
    }

    /// Change settings. Filter states carry over, except that switching a half
    /// on starts it from silence, so a stale tail doesn't burst out.
    pub fn set(&mut self, s: AudioSettings) {
        if s.n64 && !self.s.n64 {
            self.n64 = [N64Stage::new(self.fs), N64Stage::new(self.fs)];
        }
        if s.tv && !self.s.tv {
            self.tv = [TvStage::new(self.fs), TvStage::new(self.fs)];
        }
        self.s = s;
        self.k = Coeffs::new(self.fs, &s);
        for t in &mut self.tv {
            t.retune(self.fs, &s);
        }
    }

    /// Zero decayed filter states; call once per block.
    pub fn flush(&mut self) {
        for n in &mut self.n64 {
            n.flush();
        }
        for t in &mut self.tv {
            t.flush();
        }
    }

    #[inline]
    pub fn frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        let s = &self.s;
        if !s.active() {
            return (l, r);
        }
        let (mut a, mut b) = (l as f64, r as f64);
        if s.n64 {
            a = self.n64[0].run(a, s.dac_filter);
            b = self.n64[1].run(b, s.dac_filter);
        }
        if s.tv {
            let k = self.k;
            let (wa, wb) = if s.mono {
                let m = self.tv[0].run(0.5 * (a + b), &k);
                (m, m)
            } else {
                (self.tv[0].run(a, &k), self.tv[1].run(b, &k))
            };
            a += k.mix * (wa - a);
            b += k.mix * (wb - b);
            if k.whine_amp > 0.0 {
                let w = k.whine_amp * (2.0 * PI * self.whine_phase).sin();
                self.whine_phase = (self.whine_phase + k.whine_step).fract();
                a += w;
                b += w;
            }
        }
        (a as f32, b as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: u32 = 48_000;

    /// Single-bin DFT magnitude (amplitude units) of `x` at `f`.
    fn tone_amp(x: &[f64], f: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let p = 2.0 * PI * f * i as f64 / FS as f64;
            re += v * p.cos();
            im += v * p.sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f64
    }

    /// Feed a sine (left = right) through `c` and return the left output after it settles.
    fn run_sine(c: &mut TvChain, f: f64, amp: f64, secs: f64) -> Vec<f64> {
        let n = (secs * FS as f64) as usize;
        let settle = FS as usize / 4;
        let mut out = Vec::with_capacity(n);
        for i in 0..settle + n {
            let x = (amp * (2.0 * PI * f * i as f64 / FS as f64).sin()) as f32;
            let (l, _) = c.frame(x, x);
            if i % 512 == 0 {
                c.flush();
            }
            if i >= settle {
                out.push(l as f64);
            }
        }
        out
    }

    /// Steady-state gain (dB) at `f`, measured at the input frequency only.
    fn gain_db(s: AudioSettings, f: f64, amp: f64) -> f64 {
        let mut c = TvChain::new(FS, s);
        // Whole cycles, so the single-bin DFT doesn't leak.
        let secs = (0.5 * f).round() / f;
        let out = run_sine(&mut c, f, amp, secs);
        20.0 * (tone_amp(&out, f) / amp).log10()
    }

    /// The speaker's filters alone: no clip, compressor or cabinet.
    fn speaker_only(p: SpeakerPreset) -> AudioSettings {
        let mut s = AudioSettings { tv: true, ..AudioSettings::default() };
        p.apply(&mut s);
        s.drive = 0.0;
        s.squash = 0.0;
        s.cabinet = 0.0;
        s.volume_db = 0.0;
        s
    }

    #[test]
    fn everything_off_is_bit_identical() {
        let mut c = TvChain::new(FS, AudioSettings::default());
        assert!(!c.settings().active());
        let mut rng = 12345u32;
        for _ in 0..10_000 {
            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let l = (rng as f32 / u32::MAX as f32) * 4.0 - 2.0;
            let r = -l * 0.37;
            let (a, b) = c.frame(l, r);
            assert_eq!((a.to_bits(), b.to_bits()), (l.to_bits(), r.to_bits()));
        }
    }

    #[test]
    fn mono_folds_both_sides_into_one_speaker() {
        let mut c = TvChain::new(FS, AudioSettings { tv: true, ..AudioSettings::default() });
        let mut max_diff = 0.0f32;
        let mut max_out = 0.0f32;
        for i in 0..FS {
            let x = (0.3 * (2.0 * PI * 1000.0 * i as f64 / FS as f64).sin()) as f32;
            let (l, r) = c.frame(x, 0.0); // hard left
            max_diff = max_diff.max((l - r).abs());
            max_out = max_out.max(r.abs());
        }
        assert_eq!(max_diff, 0.0);
        assert!(max_out > 0.05, "the right side hears the left: {max_out}");
    }

    #[test]
    fn the_portable_speaker_has_no_bass_no_treble_and_a_nasal_middle() {
        let s = speaker_only(SpeakerPreset::Portable);
        let g = |f| gain_db(s, f, 0.05);
        let (g100, g1k, g2k, g10k) = (g(100.0), g(1000.0), g(2100.0), g(10_000.0));
        assert!(g100 - g1k < -15.0, "100 Hz {g100:.1} vs 1 kHz {g1k:.1}");
        assert!(g10k - g1k < -15.0, "10 kHz {g10k:.1} vs 1 kHz {g1k:.1}");
        assert!(g2k - g1k > 3.0, "the box bump: 2.1 kHz {g2k:.1} vs 1 kHz {g1k:.1}");
        assert!((g2k - 6.0).abs() < 1.5, "bump peak ≈ +6 dB: {g2k:.1}");
        // −3 dB-ish at the corners (the cone hump and break-up move them a little).
        assert!((g(250.0) - g1k).abs() < 5.0);
        assert!(g(5000.0) - g1k < -3.0 && g(5000.0) - g1k > -9.0);
    }

    #[test]
    fn presets_run_from_subtle_to_awful() {
        let band = |p| {
            let s = speaker_only(p);
            let g1k = gain_db(s, 1000.0, 0.05);
            (gain_db(s, 150.0, 0.05) - g1k, gain_db(s, 6000.0, 0.05) - g1k)
        };
        let (big, port, kit) = (band(SpeakerPreset::BigSet), band(SpeakerPreset::Portable), band(SpeakerPreset::Kitchen));
        assert!(big.0 > port.0 && port.0 > kit.0, "bass: {big:?} {port:?} {kit:?}");
        assert!(big.1 > port.1 && port.1 > kit.1, "treble: {big:?} {port:?} {kit:?}");
        assert!(big.0 > -6.0 && big.1 > -3.0, "the big set is subtle: {big:?}");
    }

    #[test]
    fn the_amp_clips_only_when_loud() {
        // Linear below half the ceiling, exactly.
        for c in [1.0, 0.5, 0.125] {
            for x in [-0.49 * c, -0.1 * c, 0.0, 0.2 * c, 0.5 * c] {
                assert_eq!(soft_clip(x, c), x);
            }
            for x in [0.6 * c, 2.0 * c, 50.0] {
                let y = soft_clip(x, c);
                assert!(y < x && y <= c && y > 0.5 * c, "{x} → {y} (ceiling {c})");
                assert_eq!(soft_clip(-x, c), -y);
            }
        }
        // End to end: loud notes lose level, quiet ones don't.
        let mut s = speaker_only(SpeakerPreset::Portable);
        s.drive = 0.6;
        let quiet = gain_db(s, 1000.0, 0.01);
        let loud = gain_db(s, 1000.0, 0.9);
        let clean = gain_db(speaker_only(SpeakerPreset::Portable), 1000.0, 0.01);
        assert!((quiet - clean).abs() < 0.01, "quiet {quiet:.2} vs clean {clean:.2}");
        assert!(loud < quiet - 6.0, "loud {loud:.1} vs quiet {quiet:.1}");
    }

    #[test]
    fn the_compressor_squashes_loud_and_leaves_quiet() {
        let mut s = speaker_only(SpeakerPreset::Portable);
        s.squash = 1.0;
        let quiet = gain_db(s, 1000.0, 0.005);
        // 0.4: under the amp's knee (−6 dBFS at drive 0), so only the compressor acts.
        let loud = gain_db(s, 1000.0, 0.4);
        assert!(loud < quiet - 10.0, "loud {loud:.1} vs quiet {quiet:.1}");
        s.squash = 0.0;
        assert!((gain_db(s, 1000.0, 0.4) - gain_db(s, 1000.0, 0.005)).abs() < 0.01);
    }

    #[test]
    fn the_cabinet_rings_but_stays_bounded() {
        let mut s = speaker_only(SpeakerPreset::Portable);
        let flat: Vec<f64> = (0..40).map(|i| gain_db(s, 400.0 + 50.0 * i as f64, 0.05)).collect();
        s.cabinet = 1.0;
        let boxy: Vec<f64> = (0..40).map(|i| gain_db(s, 400.0 + 50.0 * i as f64, 0.05)).collect();
        let diff: Vec<f64> = flat.iter().zip(&boxy).map(|(a, b)| b - a).collect();
        let (lo, hi) = diff.iter().fold((f64::MAX, f64::MIN), |(l, h), &d| (l.min(d), h.max(d)));
        assert!(hi - lo > 4.0, "a comb ripple: {lo:.1}..{hi:.1} dB");
        assert!(hi < 7.0, "peaks stay under +7 dB: {hi:.1}");
    }

    #[test]
    fn the_n64_half_keeps_the_band_and_drops_everything_above_11k() {
        let s = AudioSettings { n64: true, ..AudioSettings::default() };
        let g4k = gain_db(s, 4000.0, 0.3);
        assert!(g4k > -2.0 && g4k < 0.5, "4 kHz passes: {g4k:.2}");
        // 14 kHz can't exist at 22020 Hz: it's gone (what survives is its alias at 8020).
        let mut c = TvChain::new(FS, s);
        let out = run_sine(&mut c, 14_000.0, 0.3, 0.5);
        let at = 20.0 * (tone_amp(&out, 14_000.0) / 0.3).log10();
        assert!(at < -40.0, "14 kHz at {at:.1} dB");
    }

    #[test]
    fn without_the_dac_filter_the_hold_leaves_its_image() {
        // A zero-order hold at 22020 mirrors 4 kHz to 18020 Hz at sinc(18020/22020)
        // / sinc(4000/22020) ≈ −13 dB (the average over each output sample adds
        // about −2 dB more at 18 kHz).
        let s = AudioSettings { n64: true, dac_filter: false, ..AudioSettings::default() };
        let mut c = TvChain::new(FS, s);
        let out = run_sine(&mut c, 4000.0, 0.3, 0.5);
        let image = 20.0 * (tone_amp(&out, N64_RATE - 4000.0) / tone_amp(&out, 4000.0)).log10();
        assert!(image > -18.0 && image < -11.0, "image at {image:.1} dB");
        let s = AudioSettings { n64: true, ..AudioSettings::default() };
        let mut c = TvChain::new(FS, s);
        let out = run_sine(&mut c, 4000.0, 0.3, 0.5);
        let image = 20.0 * (tone_amp(&out, N64_RATE - 4000.0) / tone_amp(&out, 4000.0)).log10();
        assert!(image < -30.0, "filtered image at {image:.1} dB");
    }

    #[test]
    fn the_whine_is_the_line_rate_and_only_when_asked() {
        let mut s = AudioSettings { tv: true, ..AudioSettings::default() };
        let mut c = TvChain::new(FS, s);
        let out = run_sine(&mut c, 1000.0, 0.0, 0.5);
        assert!(out.iter().all(|&v| v == 0.0), "silence in, silence out");
        s.whine = true;
        s.whine_db = -40.0;
        let mut c = TvChain::new(FS, s);
        let out = run_sine(&mut c, 1000.0, 0.0, 0.5);
        let amp = tone_amp(&out, FLYBACK_HZ);
        assert!((20.0 * amp.log10() + 40.0).abs() < 0.5, "{amp}");
        // The whine is the tube, not the N64: no whine with only the N64 half on.
        let mut c = TvChain::new(FS, AudioSettings { n64: true, tv: false, whine: true, ..s });
        assert!(run_sine(&mut c, 1000.0, 0.0, 0.1).iter().all(|&v| v == 0.0));
    }
}
