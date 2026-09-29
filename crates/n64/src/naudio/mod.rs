//! The N64's sequenced audio as Perfect Dark links it: Rare's `n_` variant of
//! libultra's audio library (`src/lib/naudio/`), headless. Sequences and a
//! soundbank in, 16-bit stereo samples at the AI rate out.
//!
//! * [`abi`]: the RSP half. The library builds an audio command list each
//!   frame and the RSP's audio microcode runs it. The decomp has only the CPU
//!   half, so the ops follow the PC port's `port/src/mixer.c`, which runs PD's
//!   command lists on a CPU. Here the commands run as they are built, on a
//!   model of DMEM, with PD's DMEM addresses.
//! * [`synth`]: `n_synthesizer.c` and the per-voice filters it pulls
//!   (`n_load.c`, `n_resample.c`, `n_resample2.c`, `n_env.c`), the buses
//!   (`n_auxbus.c`, `n_mainbus.c`, `n_save.c`), the voice calls (`n_syn*.c`)
//!   and the frame (`n_alAudioFrame`).
//! * [`reverb`]: `n_reverb.c` (Rare's stereo delay lines) and `n_alFxNew`.
//! * [`cseq`]: the compact MIDI reader (`n_csq.c`, `cseq.c`).
//! * [`csp`]: the compact sequence player (`n_csplayer.c`, `n_seqplayer.c`,
//!   `n_event.c`, `n_csp*.c`, `n_seqpstop.c`).
//! * [`osc`]: `osc.c`, the tremolo and vibrato oscillators.
//! * [`Naudio`]: the three sequence players on one synth, as `snd_init`
//!   (`snd.c:1420`) and `amgr_create` (`audiomgr.c:59`) set them up.
//!
//! Units are PD's: times in microseconds (`ALMicroTime`), synth time in
//! samples at the output rate, volumes `0..0x7fff` (`AL_VOL_FULL`) or MIDI
//! `0..127`, pans `0..127`. Floats are f32, as the N64 computes them.
//!
//! What is left out is what Perfect Dark's Combat Simulator never reaches:
//! the SFX player (PD's SFX are not synthesised here), MP3 streams on the main
//! bus, and Rare's two undocumented microcode ops (the per-voice low-pass and
//! "noop", `n_resample2.c`, `n_auxbus.c`), which only controllers 0x21-0x23 and
//! `AL_SEQP_FXPARAM_EVT` switch on, and no sequence sends them.

// PD's constants are kept digit for digit (`1.4141999483109f`, the cents ratios).
#![allow(clippy::excessive_precision)]

pub mod abi;
pub mod bank;
pub mod cseq;
pub mod csp;
pub mod osc;
pub mod reverb;
pub mod synth;

use std::sync::Arc;

pub use bank::Bank;
pub use csp::CSPlayer;
pub use synth::{SynConfig, Synth};

/// `AL_VOL_FULL` (`libaudio.h:78`): what the game passes as full volume.
pub const AL_VOL_FULL: i16 = 0x7fff;
/// `_AL_VOL_FULL` (`libaudio.h:77`): the MIDI full volume.
pub const AL_VOL_FULL_MIDI: u8 = 127;
/// `AL_USEC_PER_FRAME` (`libaudio.h:67`, NTSC).
pub const AL_USEC_PER_FRAME: i32 = 16000;
/// `AL_GAIN_CHANGE_TIME` (`libaudio.h:69`).
pub const AL_GAIN_CHANGE_TIME: i32 = 1000;
/// `AL_EVTQ_END` (`libaudio.h:451`).
pub const AL_EVTQ_END: i32 = 0x7fffffff;
/// `KILL_TIME` (`n_seqp.h:23`).
pub const KILL_TIME: i32 = 50000;

/// Sequence player states (`libaudio.h:404`).
pub const AL_STOPPED: i32 = 0;
pub const AL_PLAYING: i32 = 1;
pub const AL_STOPPING: i32 = 2;

/// `osAiSetFrequency(22020)` on NTSC (`audiomgr.c:64`, `aisetfreq.c`): the VI
/// clock over the nearest divider, 48681812 / 2211.
pub const OUTPUT_RATE: i32 = 22018;

/// `alCents2Ratio` (`cents2ratio.c:3`): 2^(cents/1200) by f32
/// square-and-multiply.
pub fn al_cents2ratio(cents: i32) -> f32 {
    let mut cents = cents;
    let mut x: f32 = if cents >= 0 {
        1.000_577_8
    } else {
        cents = -cents;
        0.999_422_54
    };
    let mut ratio = 1.0f32;
    while cents != 0 {
        if cents & 1 != 0 {
            ratio *= x;
        }
        x *= x;
        cents >>= 1;
    }
    ratio
}

/// `alSemitones2Ratio` (`n_drvrNew.c:92`).
pub fn al_semitones2ratio(semitones: i32) -> f32 {
    let mut n = semitones;
    let mut mult: f32 = if n >= 0 {
        1.059_463_1
    } else {
        n = -n;
        0.943_874_3
    };
    let mut value = 1.0f32;
    while n != 0 {
        if n & 1 != 0 {
            value *= mult;
        }
        mult *= mult;
        n >>= 1;
    }
    value
}

/// The sequence players and their synth: PD's music half of `snd_init`.
pub struct Naudio {
    pub syn: Synth,
    /// `g_SeqInstances[i].seqp`, in `seq_init` order. The synth's client list
    /// is them newest first (`n_alSynAddSeqPlayer` prepends).
    pub players: Vec<CSPlayer>,
    pub osc: osc::OscPool,
    bank: Arc<Bank>,
}

/// `seq_init`'s `ALSeqpConfig` (`snd.c:1380`).
pub const SEQP_MAX_VOICES: usize = 44;
pub const SEQP_MAX_EVENTS: usize = 64;
pub const SEQP_MAX_CHANNELS: usize = 16;

impl Naudio {
    /// `n_alSynNew`, then `players` × `seq_init` (`n_alCSPNew`,
    /// `n_alCSPSetBank`), then `osc_build_linkedlist(0, 60)`.
    ///
    /// SUBST: PD also adds its SFX player (`n_alSndpNew`) to the synth, which
    /// shares the 30 physical voices / PD's SFX are played by the engine, so
    /// the music has them all.
    pub fn new(bank: Arc<Bank>, config: SynConfig, players: usize) -> Naudio {
        let syn = Synth::new(bank.clone(), config, players * SEQP_MAX_VOICES);
        let mut n = Naudio { syn, players: Vec::new(), osc: osc::OscPool::new(60), bank };
        for i in 0..players {
            let mut p = CSPlayer::new(i, SEQP_MAX_VOICES, SEQP_MAX_EVENTS, SEQP_MAX_CHANNELS, n.syn.cur_samples());
            p.set_bank();
            n.players.push(p);
        }
        n
    }

    pub fn bank(&self) -> &Arc<Bank> {
        &self.bank
    }

    /// `n_alAudioFrame` (`n_synthesizer.c:98`): run every client whose next
    /// callback falls in this frame (early, at the frame's start: their
    /// updates carry the sample they are for), then `out_len` samples in
    /// 184-sample subframes, appended to `out` as (left, right).
    pub fn audio_frame(&mut self, out_len: i32, out: &mut Vec<[i16; 2]>) {
        loop {
            let (client, next) = self.next_sample_time();
            if next.wrapping_sub(self.syn.cur_samples()) >= out_len {
                self.syn.param_samples = next;
                break;
            }
            self.syn.param_samples = next & !0xf;
            let delta = self.players[client].voice_handler(&mut self.syn, &mut self.osc);
            let p = &mut self.players[client];
            p.samples_left = p.samples_left.wrapping_add(self.syn.time_to_samples_no_round(delta));
        }
        self.syn.param_samples &= !0xf;
        let mut left = out_len;
        while left > 0 {
            let n = left.min(synth::FIXED_SAMPLE);
            self.syn.save_pull(out);
            left -= n;
            self.syn.advance(n);
        }
        self.syn.collect_pvoices();
    }

    /// `__n_nextSampleTime` (`n_synthesizer.c:210`): the client due first. The
    /// list is newest first and a tie keeps the earlier entry.
    fn next_sample_time(&self) -> (usize, i32) {
        let cur = self.syn.cur_samples();
        let mut best = (0, i32::MAX);
        for i in (0..self.players.len()).rev() {
            let d = self.players[i].samples_left.wrapping_sub(cur);
            if d < best.1 {
                best = (i, d);
            }
        }
        (best.0, self.players[best.0].samples_left)
    }
}

#[cfg(test)]
mod tests {
    use super::bank::*;
    use super::*;

    /// A bank of one instrument whose one sound is a looped square wave: two
    /// ADPCM frames with a zero codebook (each sample is its nibble << 12),
    /// +7 then -8, so a 32-sample period at unity pitch.
    fn square_bank(release_us: i32, decay_us: i32) -> Arc<Bank> {
        let mut tbl = vec![0xc0u8];
        tbl.extend([0x77u8; 8]);
        tbl.push(0xc0);
        tbl.extend([0x88u8; 8]);
        let wave = WaveTable { base: 0, len: 18, order: 2, npredictors: 1, book: vec![0; 16], adpcm_loop: Some(AdpcmLoop { start: 0, end: 32, count: -1, state: [7 << 12; 16] }) };
        let sound = Sound {
            envelope: Envelope { attack_time: 0, decay_time: decay_us, release_time: release_us, attack_volume: 127, decay_volume: 127 },
            key_map: KeyMap { velocity_min: 0, velocity_max: 127, key_min: 0, key_max: 127, key_base: 60, detune: 0 },
            wavetable: 0,
            sample_pan: 64,
            sample_volume: 127,
            flags: 0,
        };
        let inst = Instrument { volume: 127, pan: 64, priority: 5, bend_range: 200, sounds: vec![0], ..Instrument::default() };
        Arc::new(Bank { instruments: vec![Some(inst)], percussion: None, sounds: vec![sound], wavetables: vec![wave], tbl })
    }

    fn varlen(mut v: u32, out: &mut Vec<u8>) {
        let mut bytes = vec![(v & 0x7f) as u8];
        v >>= 7;
        while v != 0 {
            bytes.push((v & 0x7f) as u8 | 0x80);
            v >>= 7;
        }
        bytes.reverse();
        out.extend(bytes);
    }

    /// A compact MIDI sequence of one track: program 0, `key` held for
    /// `dur` ticks, the track ending `end` ticks in (division 384).
    fn one_note(key: u8, dur: u32, end: u32) -> Vec<u8> {
        let mut d = vec![0u8; 68];
        d[0..4].copy_from_slice(&68u32.to_be_bytes());
        d[64..68].copy_from_slice(&384u32.to_be_bytes());
        varlen(0, &mut d);
        d.extend([0xc0, 0x00]);
        varlen(0, &mut d);
        d.extend([0x90, key, 0x7f]);
        varlen(dur, &mut d);
        varlen(end, &mut d);
        d.extend([0xff, 0x2f]);
        d
    }

    fn render(n: &mut Naudio, frames: usize) -> Vec<[i16; 2]> {
        let mut out = Vec::new();
        for _ in 0..frames {
            n.audio_frame(synth::FRAME_SAMPLES, &mut out);
        }
        out
    }

    fn crossings(s: &[[i16; 2]]) -> usize {
        s.windows(2).filter(|w| (w[0][0] >= 0) != (w[1][0] >= 0)).count()
    }

    fn start(bank: Arc<Bank>, seq: &[u8]) -> Naudio {
        let mut n = Naudio::new(bank, SynConfig::pd(), 3);
        n.syn.surround_output_type(synth::SpeakerMode::Stereo);
        n.syn.surround_reverb_setup(0, 4);
        n.syn.surround_reverb_setup(1, 4);
        n.players[0].set_seq(seq);
        n.players[0].set_vol(AL_VOL_FULL);
        n.players[0].play();
        n
    }

    /// Key 60 on a key-base-60 sound plays the wave as recorded (a period of
    /// 32 samples); an octave up halves it; the note sounds for its
    /// duration (384 ticks at the default 488 us a tick) and is silent once
    /// released; the sequence then ends and the player stops.
    #[test]
    fn a_note_plays_at_its_pitch_for_its_duration() {
        let mut n = start(square_bank(20_000, 10_000_000), &one_note(60, 384, 1000));
        let out = render(&mut n, 12);
        assert_eq!(n.players[0].get_state(), AL_PLAYING);
        let live = &out[1000..3000];
        let period = 2.0 * live.len() as f32 / crossings(live) as f32;
        assert!((period - 32.0).abs() < 0.5, "period {period}");
        let peak = live.iter().map(|f| f[0].unsigned_abs()).max().unwrap();
        assert!(peak > 10_000, "peak {peak}");
        // 384 * 488 us = 187 ms = 4126 samples, then a 20 ms release.
        let note_end = (384.0 * 488.0 * 22018.0 / 1e6) as usize;
        assert!(out[note_end - 400..note_end - 200].iter().any(|f| f[0].unsigned_abs() > 10_000));
        assert!(out[note_end + 1500..note_end + 2000].iter().all(|f| f[0].unsigned_abs() < 50), "released");
        let _ = render(&mut n, 12);
        assert_eq!(n.players[0].get_state(), AL_STOPPED, "the sequence ended");
        assert_eq!(n.syn.voices_playing(), 0);

        let mut n = start(square_bank(20_000, 10_000_000), &one_note(72, 384, 1000));
        let out = render(&mut n, 4);
        let live = &out[1000..2500];
        let period = 2.0 * live.len() as f32 / crossings(live) as f32;
        assert!((period - 16.0).abs() < 0.5, "an octave up: period {period}");
    }

    /// At a pitch that isn't a whole ratio (a semitone up: 1.0595) every
    /// half-period is the same length to a sample, across the 184-sample
    /// subframes and the loop: the decoder's history, the resampler's state
    /// and its phase carry over.
    #[test]
    fn a_note_is_seamless_across_subframes() {
        let mut n = start(square_bank(20_000, 10_000_000), &one_note(61, 384, 1000));
        let out = render(&mut n, 6);
        let edges: Vec<usize> = (1000..3800).filter(|&i| (out[i - 1][0] >= 0) != (out[i][0] >= 0)).collect();
        let half = 16.0 / al_semitones2ratio(1);
        for w in edges.windows(2) {
            let d = (w[1] - w[0]) as f32;
            assert!((d - half).abs() <= 1.0, "a half-period of {d} at {} (want {half})", w[0]);
        }
        assert!(edges.len() > 150);
    }

    /// `n_alCSPChanFade` steps a channel's fade volume every 100 ticks of
    /// time by its increment until it reaches the target; the voice gets
    /// quieter (each step ramps over what is left of the note's envelope
    /// segment, `__n_vsDelta`, here a 50 ms decay long over); stopping the
    /// player kills the voices.
    #[test]
    fn a_channel_fades_and_the_player_stops() {
        let mut n = start(square_bank(20_000, 50_000), &one_note(60, 20_000, 20_000));
        let _ = render(&mut n, 2);
        n.players[0].chan_fade(0, 0, 32);
        let out = render(&mut n, 1);
        assert!(out.iter().any(|f| f[0].unsigned_abs() > 10_000), "loud before the fade");
        let mut last = 255;
        for _ in 0..40 {
            let _ = render(&mut n, 1);
            let c = n.players[0].chan_state[0].fadevolcurrent;
            assert!(c <= last);
            last = c;
        }
        assert_eq!(last, 0, "faded to 0 in 8 steps of 32");
        let out = render(&mut n, 1);
        assert!(out.iter().all(|f| f[0].unsigned_abs() < 200), "silent");
        n.players[0].stop();
        let _ = render(&mut n, 4);
        assert_eq!(n.players[0].get_state(), AL_STOPPED);
        assert_eq!(n.players[0].voices_playing(), 0);
    }

    /// A tempo meta retimes the sequence: at 500000 us a quarter note
    /// (1302 us a tick at division 384) the same 384-tick note lasts 500 ms.
    #[test]
    fn tempo_sets_the_tick_length() {
        let mut seq = vec![0u8; 68];
        seq[0..4].copy_from_slice(&68u32.to_be_bytes());
        seq[64..68].copy_from_slice(&384u32.to_be_bytes());
        varlen(0, &mut seq);
        seq.extend([0xff, 0x51, 0x07, 0xa1, 0x20]);
        varlen(0, &mut seq);
        seq.extend([0xc0, 0x00]);
        varlen(0, &mut seq);
        seq.extend([0x90, 60, 0x7f]);
        varlen(384, &mut seq);
        varlen(2000, &mut seq);
        seq.extend([0xff, 0x2f]);
        let mut n = start(square_bank(10_000, 10_000_000), &seq);
        let out = render(&mut n, 25);
        assert_eq!(n.players[0].uspt, 1302);
        let end = (384.0 * 1302.0 * 22018.0 / 1e6) as usize;
        assert!(out[end - 600..end - 300].iter().any(|f| f[0].unsigned_abs() > 10_000), "still playing near 500 ms");
        assert!(out[end + 1500..end + 2000].iter().all(|f| f[0].unsigned_abs() < 50), "released after");
    }

    #[test]
    fn cents_and_semitones() {
        assert_eq!(al_cents2ratio(0), 1.0);
        assert!((al_cents2ratio(1200) - 2.0).abs() < 1e-3);
        assert!((al_cents2ratio(-1200) - 0.5).abs() < 1e-3);
        assert!((al_semitones2ratio(12) - 2.0).abs() < 1e-3);
        assert!((al_semitones2ratio(-12) - 0.5).abs() < 1e-3);
    }
}
