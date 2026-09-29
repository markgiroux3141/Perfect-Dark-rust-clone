//! The synthesizer (`n_synthesizer.c`): physical voices pulled 184 samples
//! at a time through the load → resample → low-pass → envelope-mixer chain,
//! mixed onto their FX bus, reverbed, summed on the main bus and
//! interleaved. Parameter changes (`ALParam` updates) queue on a physical
//! voice, stamped with the sample they are for, and apply at the start of
//! the subframe that contains it (`SAMPLE184`).
//!
//! A virtual voice (`N_ALVoice`) is what a sequence player holds; this
//! keeps them all in [`Synth::vvoices`] so the physical voices can look at
//! the one they play (`pvoice->vvoice`), stale pointers included.

use std::collections::VecDeque;
use std::sync::Arc;

use super::abi::*;
use super::bank::Bank;
use super::reverb::Fx;
use super::{AL_PLAYING, AL_STOPPED, OUTPUT_RATE};

/// `SAMPLES` / `FIXED_SAMPLE` (`n_synthInternals.h:26`): one subframe.
pub const FIXED_SAMPLE: i32 = 184;
/// `g_AmgrFreqPerTick` (`audiomgr.c:77`): 22018 / 30, rounded up to a
/// whole subframe and one more: an audio frame's samples.
pub const FRAME_SAMPLES: i32 = 736;

/// `SAMPLE184` (`n_synthInternals.h:27`), with C's truncating division.
pub fn sample184(delta: i32) -> i32 {
    ((delta.wrapping_add(FIXED_SAMPLE - 1)) / FIXED_SAMPLE) * FIXED_SAMPLE
}

const ADPCMFBYTES: i32 = 9;
const ADPCMFSIZE: i32 = 16;
const LFSAMPLES: i32 = 4;
const ADPCMVSIZE: i32 = 8;
const N_EQPOWER_LENGTH: i32 = 128;

/// `n_eqpower` (`n_env.c:7`): equal-power pan and dry/wet curve.
#[rustfmt::skip]
pub const N_EQPOWER: [i16; 128] = [
    0x7fff, 0x7ffc, 0x7ff5, 0x7fe8, 0x7fd7, 0x7fc0, 0x7fa5, 0x7f84, 0x7f5f, 0x7f34, 0x7f05, 0x7ed0, 0x7e97, 0x7e58, 0x7e15, 0x7dcd,
    0x7d7f, 0x7d2d, 0x7cd6, 0x7c7a, 0x7c1a, 0x7bb4, 0x7b49, 0x7ada, 0x7a66, 0x79ed, 0x796f, 0x78ed, 0x7866, 0x77da, 0x7749, 0x76b4,
    0x761a, 0x757b, 0x74d8, 0x7430, 0x7384, 0x72d3, 0x721e, 0x7164, 0x70a6, 0x6fe3, 0x6f1c, 0x6e51, 0x6d81, 0x6cad, 0x6bd5, 0x6af9,
    0x6a18, 0x6933, 0x684a, 0x675d, 0x666c, 0x6577, 0x647e, 0x6381, 0x6280, 0x617c, 0x6073, 0x5f67, 0x5e57, 0x5d43, 0x5c2c, 0x5b11,
    0x59f2, 0x58d0, 0x57aa, 0x5681, 0x5555, 0x5425, 0x52f2, 0x51bc, 0x5082, 0x4f46, 0x4e06, 0x4cc3, 0x4b7d, 0x4a35, 0x48e9, 0x479b,
    0x4649, 0x44f5, 0x439e, 0x4245, 0x40e9, 0x3f8a, 0x3e29, 0x3cc6, 0x3b60, 0x39f8, 0x388d, 0x3721, 0x35b2, 0x3441, 0x32ce, 0x3159,
    0x2fe2, 0x2e69, 0x2cef, 0x2b72, 0x29f4, 0x2875, 0x26f3, 0x2570, 0x23ec, 0x2266, 0x20df, 0x1f57, 0x1dce, 0x1c43, 0x1ab7, 0x192a,
    0x179c, 0x160e, 0x147e, 0x12ed, 0x115c, 0x0fca, 0x0e38, 0x0ca5, 0x0b11, 0x097d, 0x07e9, 0x0654, 0x04c0, 0x032a, 0x0195, 0x0000,
];

fn eqp(i: i32) -> i32 {
    N_EQPOWER[i.clamp(0, N_EQPOWER_LENGTH - 1) as usize] as i32
}

/// `params_bus0_8mb` (`audiomgr.c:196`), `x ms` evaluated as C does
/// (`x * 40`, truncated).
#[rustfmt::skip]
pub const PARAMS_BUS0_8MB: [i32; 66] = [
    8, 7040,
    0, 192, 9830, -9830, 0, 0, 0, 0,
    192, 392, 9830, -9830, 11140, 0, 0, 0,
    880, 2816, 16384, -16384, 4587, 0, 0, 0,
    1056, 2112, 8192, -8192, 0, 0, 0, 0,
    3520, 6160, 16384, -16384, 4587, 0, 0, 0,
    3696, 5280, 8192, -8192, 0, 0, 0, 0,
    5280, 5944, 8192, -8192, 0, 0, 0, 0,
    0, 6512, 13000, -13000, 0, 380, 10, 0,
];

/// `params_bus1_8mb` (`audiomgr.c:209`).
pub const PARAMS_BUS1_8MB: [i32; 10] = [1, 2640, 0, 2200, 13108, 0, 29493, 0, 0, 0];

/// `alSurround_OutputType`'s modes (`SPEAKERMODE_*`, `constants.h:3904`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeakerMode {
    Mono,
    #[default]
    Stereo,
    Headphone,
    Surround,
}

/// `ALSynConfig`, as `snd_init` and `amgr_create` fill it.
#[derive(Clone, Debug)]
pub struct SynConfig {
    pub max_pvoices: usize,
    pub max_updates: usize,
    pub output_rate: i32,
    /// `AL_FX_CUSTOM` params per FX bus (`maxFXbusses` is their count).
    pub fx_params: Vec<Vec<i32>>,
}

impl SynConfig {
    /// PD on an 8 MB console (`snd.c:1515`, `audiomgr.c:220`).
    pub fn pd() -> SynConfig {
        SynConfig { max_pvoices: 30, max_updates: 64, output_rate: OUTPUT_RATE, fx_params: vec![PARAMS_BUS0_8MB.to_vec(), PARAMS_BUS1_8MB.to_vec()] }
    }
}

/// A virtual voice: `vs->voice`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceId(pub u16);

/// `N_ALVoice`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Voice {
    pub priority: i16,
    pub unity_pitch: bool,
    pub fx_bus: u8,
    pub state: i32,
    pub pvoice: Option<usize>,
}

/// `ALStartParamAlt`.
#[derive(Clone, Copy, Debug)]
struct StartAlt {
    unity: bool,
    pan: u8,
    volume: i16,
    fxmix: u8,
    pitch: f32,
    unk14: u8,
    unk15: u8,
    unk18: f32,
    samples: i32,
    wave: usize,
}

/// An `ALParam` update and its type (`AL_FILTER_*`).
#[derive(Clone, Copy, Debug)]
enum ParamKind {
    StartVoiceAlt(StartAlt),
    SetVolume { vol: i16, samples: i32 },
    SetPan(i16),
    SetFxAmt(i32),
    SetPitch(f32),
    StopVoice,
    FreeVoice(usize),
    Filter11(u8),
    Filter12(u8),
    Filter13(f32),
}

#[derive(Clone, Copy, Debug)]
struct Param {
    delta: i32,
    kind: ParamKind,
}

/// `N_PVoice` (`n_synthInternals.h:52`): a physical voice and its filters' state.
#[derive(Clone, Debug)]
pub struct PVoice {
    pub vvoice: Option<VoiceId>,
    // ALLoadFilter
    dc_state: [i16; 16],
    dc_lstate: [i16; 16],
    dc_loop_start: u32,
    dc_loop_end: u32,
    dc_loop_count: i32,
    dc_table: Option<usize>,
    dc_book_size: i32,
    dc_sample: i32,
    dc_lastsam: i32,
    dc_first: i32,
    dc_memin: i32,
    // ALResampler
    rs_state: [i16; 16],
    rs_ratio: f32,
    rs_upitch: i32,
    rs_delta: f32,
    rs_first: i32,
    // ALEnvMixer
    em_state: EnvState,
    em_pan: i16,
    em_volume: i16,
    em_cvol_l: i16,
    em_cvol_r: i16,
    em_dryamt: i16,
    em_wetamt: i16,
    em_lratl: u16,
    em_lratm: i16,
    em_ltgt: i16,
    em_rratl: u16,
    em_rratm: i16,
    em_rtgt: i16,
    em_delta: i32,
    em_seg_end: i32,
    em_first: i32,
    em_ctrl: VecDeque<Param>,
    pub em_motion: i32,
    pub offset: i32,
    /// Rare's per-voice filter settings (`AL_FILTER_11` / `_12` / `_13`).
    unk8c: u8,
    fx_unk00: f32,
    fx_unk02: i16,
    unkb8: i32,
}

impl PVoice {
    /// `alN_PVoiceNew` (`n_drvrNew.c:236`).
    fn new() -> PVoice {
        PVoice {
            vvoice: None,
            dc_state: [0; 16],
            dc_lstate: [0; 16],
            dc_loop_start: 0,
            dc_loop_end: 0,
            dc_loop_count: 0,
            dc_table: None,
            dc_book_size: 0,
            dc_sample: 0,
            dc_lastsam: 0,
            dc_first: 1,
            dc_memin: 0,
            rs_state: [0; 16],
            rs_ratio: 1.0,
            rs_upitch: 0,
            rs_delta: 0.0,
            rs_first: 1,
            em_state: EnvState::default(),
            em_pan: 0,
            em_volume: 1,
            em_cvol_l: 1,
            em_cvol_r: 1,
            em_dryamt: 0,
            em_wetamt: 0,
            em_lratl: 0,
            em_lratm: 1,
            em_ltgt: 1,
            em_rratl: 0,
            em_rratm: 0,
            em_rtgt: 1,
            em_delta: 0,
            em_seg_end: 0,
            em_first: 1,
            em_ctrl: VecDeque::new(),
            em_motion: AL_STOPPED,
            offset: 0,
            unk8c: 0,
            fx_unk00: 0.0,
            fx_unk02: 0,
            unkb8: 0,
        }
    }
}

/// `N_ALAuxBus`: every physical voice is a source of every bus; the reverb.
struct AuxBus {
    fx: Option<Fx>,
}

/// `N_ALSynth`.
pub struct Synth {
    pub output_rate: i32,
    cur_samples: i32,
    /// `paramSamples`: the sample a handler's updates are stamped with.
    pub param_samples: i32,
    pub pvoices: Vec<PVoice>,
    free: VecDeque<usize>,
    lame: VecDeque<usize>,
    alloc: VecDeque<usize>,
    params_free: usize,
    pub vvoices: Vec<Voice>,
    aux: Vec<AuxBus>,
    pub rsp: Rsp,
    bank: Arc<Bank>,
    /// `var8009c340` (`alsurround.c`).
    surround: bool,
    mono: bool,
    headphone: bool,
    /// `var8009c344` (a stereo reverb), `var8009c346` (inverted), `var8009c348`.
    c344: [bool; 2],
    c346: [bool; 2],
    c348: [i32; 2],
}

impl Synth {
    /// `n_alSynNew` (`n_synthesizer.c:9`), with `nvvoices` virtual voices.
    pub fn new(bank: Arc<Bank>, c: SynConfig, nvvoices: usize) -> Synth {
        let buses = c.fx_params.len().clamp(1, 2);
        let aux = (0..buses).map(|i| AuxBus { fx: c.fx_params.get(i).map(|p| Fx::new(p, c.output_rate)) }).collect();
        let mut free = VecDeque::new();
        for i in 0..c.max_pvoices {
            free.push_front(i);
        }
        Synth {
            output_rate: c.output_rate,
            cur_samples: 0,
            param_samples: 0,
            pvoices: (0..c.max_pvoices).map(|_| PVoice::new()).collect(),
            free,
            lame: VecDeque::new(),
            alloc: VecDeque::new(),
            params_free: c.max_updates,
            vvoices: vec![Voice::default(); nvvoices],
            aux,
            rsp: Rsp::new(),
            bank,
            surround: false,
            mono: false,
            headphone: false,
            c344: [false; 2],
            c346: [false; 2],
            c348: [0; 2],
        }
    }

    pub fn bank(&self) -> &Arc<Bank> {
        &self.bank
    }

    pub fn cur_samples(&self) -> i32 {
        self.cur_samples
    }

    pub(super) fn advance(&mut self, n: i32) {
        self.cur_samples = self.cur_samples.wrapping_add(n);
    }

    /// How many voices are allocated (for tests and the debug panel).
    pub fn voices_playing(&self) -> usize {
        self.alloc.len()
    }

    /// `alSurround_OutputType` (`alsurround.c:20`).
    pub fn surround_output_type(&mut self, mode: SpeakerMode) {
        self.surround = mode == SpeakerMode::Surround;
        self.mono = mode == SpeakerMode::Mono;
        self.headphone = mode == SpeakerMode::Headphone;
        for i in 0..2 {
            self.surround_reverb_setup(i, 0);
        }
    }

    /// `alSurround_ReverbSetup` (`alsurround.c:43`).
    pub fn surround_reverb_setup(&mut self, index: usize, arg1: i32) {
        let arg1 = if arg1 == 0 { self.c348[index] } else { arg1 };
        self.c344[index] = false;
        self.c346[index] = false;
        match arg1 {
            2 => self.c346[index] = self.surround,
            3 => self.c344[index] = self.surround,
            4 => self.c344[index] = !self.mono,
            5 => {
                self.c344[index] = !self.mono;
                self.c346[index] = !self.mono;
            }
            _ => {}
        }
        self.c348[index] = arg1;
    }

    pub fn max_aux_busses(&self) -> usize {
        self.aux.len()
    }

    /// `_n_timeToSamplesNoRound` (`n_synthesizer.c:197`).
    pub fn time_to_samples_no_round(&self, micros: i32) -> i32 {
        (micros as f32 * self.output_rate as f32 / 1_000_000.0 + 0.5) as i32
    }

    /// `_n_timeToSamples`: rounded down to 16.
    pub fn time_to_samples(&self, micros: i32) -> i32 {
        self.time_to_samples_no_round(micros) & !0xf
    }

    // --- updates and physical voices -------------------------------------

    /// `__n_allocParam`: `None` once the 64 are all queued.
    fn alloc_param(&mut self) -> bool {
        if self.params_free == 0 {
            return false;
        }
        self.params_free -= 1;
        true
    }

    fn free_param(&mut self) {
        self.params_free += 1;
    }

    /// `n_alEnvmixerParam(AL_FILTER_ADD_UPDATE)`.
    fn add_update(&mut self, pv: usize, delta: i32, kind: ParamKind) {
        self.pvoices[pv].em_ctrl.push_back(Param { delta, kind });
    }

    /// An update on `v`'s physical voice, stamped `paramSamples + offset`
    /// (the `n_alSyn*` calls' shared shape; no update left: dropped).
    fn post(&mut self, v: VoiceId, kind: ParamKind) {
        let Some(pv) = self.vvoices[v.0 as usize].pvoice else { return };
        if !self.alloc_param() {
            return;
        }
        let delta = self.param_samples.wrapping_add(self.pvoices[pv].offset);
        self.add_update(pv, delta, kind);
    }

    /// `_n_collectPVoices` (`n_synthesizer.c:175`).
    pub(super) fn collect_pvoices(&mut self) {
        while let Some(pv) = self.lame.pop_front() {
            self.free.push_front(pv);
        }
    }

    /// `_n_freePVoice` (`n_synthesizer.c:184`).
    fn free_pvoice(&mut self, pv: usize) {
        if let Some(i) = self.alloc.iter().position(|&p| p == pv) {
            self.alloc.remove(i);
        }
        self.lame.push_front(pv);
    }

    /// `_allocatePVoice` (`n_synallocvoice.c:69`): the lame list, the free
    /// list, else the lowest-priority voice not already being stolen.
    fn allocate_pvoice(&mut self, mut priority: i16) -> (Option<usize>, bool) {
        if let Some(pv) = self.lame.pop_front() {
            self.alloc.push_front(pv);
            return (Some(pv), false);
        }
        if let Some(pv) = self.free.pop_front() {
            self.alloc.push_front(pv);
            return (Some(pv), false);
        }
        let mut found = None;
        for &pv in &self.alloc {
            let p = &self.pvoices[pv];
            let vprio = p.vvoice.map_or(0, |v| self.vvoices[v.0 as usize].priority);
            if vprio <= priority && p.offset == 0 {
                found = Some(pv);
                priority = vprio;
            }
        }
        (found, found.is_some())
    }

    /// `n_alSynAllocVoice` (`n_synallocvoice.c:6`).
    pub fn alloc_voice(&mut self, v: VoiceId, priority: i16, fx_bus: u8, unity_pitch: bool) -> bool {
        {
            let voice = &mut self.vvoices[v.0 as usize];
            voice.priority = priority;
            voice.unity_pitch = unity_pitch;
            voice.fx_bus = fx_bus;
            voice.state = AL_STOPPED;
            voice.pvoice = None;
        }
        let (pv, stolen) = self.allocate_pvoice(priority);
        let Some(pv) = pv else { return false };
        if stolen {
            self.pvoices[pv].offset = 552;
            if let Some(old) = self.pvoices[pv].vvoice {
                self.vvoices[old.0 as usize].pvoice = None;
            }
            self.pvoices[pv].vvoice = Some(v);
            self.vvoices[v.0 as usize].pvoice = Some(pv);
            // Ramp the stolen voice down, then stop it.
            if self.alloc_param() {
                let d = self.param_samples;
                self.add_update(pv, d, ParamKind::SetVolume { vol: 0, samples: 368 });
            }
            if self.alloc_param() {
                let d = self.param_samples.wrapping_add(self.pvoices[pv].offset);
                self.add_update(pv, d, ParamKind::StopVoice);
            }
        } else {
            self.pvoices[pv].offset = 0;
            self.pvoices[pv].vvoice = Some(v);
            self.vvoices[v.0 as usize].pvoice = Some(pv);
        }
        true
    }

    /// `n_alSynFreeVoice` (`n_synfreevoice.c:6`).
    pub fn free_voice(&mut self, v: VoiceId) {
        let Some(pv) = self.vvoices[v.0 as usize].pvoice else { return };
        if self.pvoices[pv].offset != 0 {
            if !self.alloc_param() {
                return;
            }
            let d = self.param_samples.wrapping_add(self.pvoices[pv].offset);
            self.add_update(pv, d, ParamKind::FreeVoice(pv));
        } else {
            self.free_pvoice(pv);
        }
        self.vvoices[v.0 as usize].pvoice = None;
    }

    /// `n_alSynStartVoiceParams` (`n_synstartvoiceparam.c:6`).
    #[allow(clippy::too_many_arguments)]
    pub fn start_voice_params(&mut self, v: VoiceId, wave: usize, pitch: f32, vol: i16, pan: u8, fxmix: u8, arg6: u8, arg7: f32, arg8: u8, t: i32) {
        let unity = self.vvoices[v.0 as usize].unity_pitch;
        let samples = self.time_to_samples(t);
        self.post(v, ParamKind::StartVoiceAlt(StartAlt { unity, pan, volume: vol, fxmix, pitch, unk14: arg8, unk15: arg6, unk18: arg7, samples, wave }));
    }

    /// `n_alSynStopVoice`.
    pub fn stop_voice(&mut self, v: VoiceId) {
        self.post(v, ParamKind::StopVoice);
    }

    /// `n_alSynSetVol`: to `volume` over `t` microseconds.
    pub fn set_vol(&mut self, v: VoiceId, volume: i16, t: i32) {
        let samples = self.time_to_samples(t);
        self.post(v, ParamKind::SetVolume { vol: volume, samples });
    }

    /// `n_alSynSetPitch`.
    pub fn set_pitch(&mut self, v: VoiceId, pitch: f32) {
        self.post(v, ParamKind::SetPitch(pitch));
    }

    /// `n_alSynSetPan`.
    pub fn set_pan(&mut self, v: VoiceId, pan: u8) {
        self.post(v, ParamKind::SetPan(pan as i16));
    }

    /// `n_alSynSetFXMix`.
    pub fn set_fxmix(&mut self, v: VoiceId, fxmix: u8) {
        self.post(v, ParamKind::SetFxAmt(fxmix as i32));
    }

    /// `n_alSynSetPriority`.
    pub fn set_priority(&mut self, v: VoiceId, priority: i16) {
        self.vvoices[v.0 as usize].priority = priority;
    }

    /// `n_alSynFilter11` / `12` / `13` (Rare's per-voice low-pass).
    pub fn filter11(&mut self, v: VoiceId, x: u8) {
        self.post(v, ParamKind::Filter11(x));
    }

    pub fn filter12(&mut self, v: VoiceId, x: u8) {
        self.post(v, ParamKind::Filter12(x));
    }

    pub fn filter13(&mut self, v: VoiceId, x: f32) {
        self.post(v, ParamKind::Filter13(x));
    }

    // --- the pulls -------------------------------------------------------

    /// `n_alSavePull` (`n_save.c:4`): one subframe into `out`.
    pub(super) fn save_pull(&mut self, out: &mut Vec<[i16; 2]>) {
        self.main_bus_pull();
        self.rsp.interleave();
        self.rsp.save_frames(out);
    }

    /// `n_alMainBusPull` (`n_mainbus.c:7`).
    fn main_bus_pull(&mut self) {
        // No MP3 (`mp3_make_samples`): the main outputs start cleared.
        self.rsp.clear_buffer(N_AL_MAIN_L_OUT, N_AL_DIVIDED << 1);
        for i in 0..self.aux.len() {
            self.fx_pull(i);
            if self.c344[i] {
                if self.c346[i] {
                    self.rsp.mix(0x8000u16 as i16, N_AL_AUX_L_OUT, N_AL_MAIN_L_OUT);
                } else {
                    self.rsp.mix(0x7fff, N_AL_AUX_L_OUT, N_AL_MAIN_R_OUT);
                }
            } else {
                if self.c346[i] {
                    self.rsp.mix(0x8000u16 as i16, N_AL_AUX_L_OUT, N_AL_MAIN_R_OUT);
                } else {
                    self.rsp.mix(0x7fff, N_AL_AUX_L_OUT, N_AL_MAIN_R_OUT);
                }
                self.rsp.mix(0x7fff, N_AL_AUX_L_OUT, N_AL_MAIN_L_OUT);
            }
            // SUBST: the bus's output low-pass (`unk44`, n_mainbus.c:36) is
            // Rare's pole filter, not emulated; only `AL_SEQP_FXPARAM_EVT`
            // switches it on and nothing posts one.
        }
    }

    /// `n_alFxPull` (`n_reverb.c:13`): the bus's voices, then its reverb.
    fn fx_pull(&mut self, bus: usize) {
        self.aux_bus_pull(bus);
        let (stereo, c346) = (self.c344[bus], self.c346[bus]);
        if let Some(fx) = self.aux[bus].fx.as_mut() {
            fx.pull(&mut self.rsp, stereo, c346);
        }
    }

    /// `n_alAuxBusPull` (`n_auxbus.c:4`): every source on this bus, those
    /// with a heavy low-pass first. Returns how many pulled.
    fn aux_bus_pull(&mut self, bus: usize) -> i32 {
        self.rsp.clear_buffer(N_AL_AUX_L_OUT, N_AL_TEMP_2);
        let mut numpulls = 0;
        let mut heavy = 0;
        for i in 0..self.pvoices.len() {
            if let Some(v) = self.pvoices[i].vvoice {
                let vv = self.vvoices[v.0 as usize];
                if vv.fx_bus as usize == bus && vv.pvoice.is_some_and(|p| self.pvoices[p].unk8c >= 64) {
                    self.envmixer_pull(i, self.cur_samples);
                    numpulls += 1;
                    heavy += 1;
                }
            }
        }
        // SUBST: with any such voice, PD filters the main outputs with
        // Rare's "noop" op (n_auxbus.c:39), whose effect isn't known; not
        // run. Only controller 0x23 sets a voice's level that high and no
        // sequence sends it.
        let _ = heavy;
        for i in 0..self.pvoices.len() {
            let pull = match self.pvoices[i].vvoice {
                Some(v) => {
                    let vv = self.vvoices[v.0 as usize];
                    vv.fx_bus as usize == bus && vv.pvoice.is_none_or(|p| self.pvoices[p].unk8c < 64)
                }
                None => bus == 0,
            };
            if pull && self.envmixer_pull(i, self.cur_samples) {
                numpulls += 1;
            }
        }
        numpulls
    }

    /// `n_alEnvmixerPull` (`n_env.c:30`): apply the voice's updates due in
    /// this subframe and mix it. Returns whether it emitted anything.
    fn envmixer_pull(&mut self, pv: usize, sample_offset: i32) -> bool {
        let mut emitted = false;
        let mut inp = N_AL_RESAMPLER_OUT;
        let mut loutp: i32 = 0;
        let mut this_offset = sample_offset;
        let mut out_count = FIXED_SAMPLE;
        while let Some(&param) = self.pvoices[pv].em_ctrl.front() {
            let last_offset = this_offset;
            this_offset = param.delta;
            let samples = sample184(this_offset.wrapping_sub(last_offset));
            if samples == 0 {
                this_offset = last_offset;
            }
            if samples > out_count {
                break;
            }
            match param.kind {
                ParamKind::StartVoiceAlt(p) => {
                    let (surround, headphone, mono) = (self.surround, self.headphone, self.mono);
                    if p.unity {
                        self.pvoices[pv].rs_upitch = 1;
                    }
                    self.load_set_wavetable(pv, p.wave);
                    let e = &mut self.pvoices[pv];
                    e.em_motion = AL_PLAYING;
                    e.em_first = 1;
                    e.em_delta = 0;
                    e.em_seg_end = sample184(p.samples);
                    let tmp = (p.volume as i32 + p.volume as i32) / 2;
                    e.em_volume = tmp as i16;
                    e.em_pan = p.pan as i16;
                    e.em_dryamt = ((eqp((p.fxmix & 0x7f) as i32) & 0xfffe) | (p.fxmix >> 7) as i32) as i16;
                    if !surround {
                        e.em_dryamt = (e.em_dryamt as i32 & 0xfffe) as i16;
                    }
                    e.em_wetamt = (eqp(N_EQPOWER_LENGTH - (p.fxmix & 0x7f) as i32 - 1) & 0xfffe) as i16;
                    if headphone {
                        e.em_pan = (e.em_pan >> 1) + 32;
                    } else if mono {
                        e.em_pan = 64;
                    }
                    if p.samples != 0 {
                        e.em_cvol_l = 1;
                        e.em_cvol_r = 1;
                    } else {
                        e.em_cvol_l = ((e.em_volume as i32 * eqp(e.em_pan as i32)) >> 15) as i16;
                        e.em_cvol_r = ((e.em_volume as i32 * eqp(N_EQPOWER_LENGTH - e.em_pan as i32 - 1)) >> 15) as i16;
                    }
                    e.rs_ratio = p.pitch;
                    e.fx_unk02 = p.unk15 as i16;
                    e.fx_unk00 = p.unk18;
                    e.unkb8 = 1;
                    e.unk8c = p.unk14;
                }
                ParamKind::SetFxAmt(_) | ParamKind::SetPan(_) | ParamKind::SetVolume { .. } => {
                    emitted |= self.pull_sub_frame(pv, &mut inp, samples);
                    let (surround, headphone, mono) = (self.surround, self.headphone, self.mono);
                    let e = &mut self.pvoices[pv];
                    if e.em_delta >= e.em_seg_end {
                        e.em_ltgt = ((e.em_volume as i32 * eqp(e.em_pan as i32)) >> 15) as i16;
                        e.em_rtgt = ((e.em_volume as i32 * eqp(N_EQPOWER_LENGTH - e.em_pan as i32 - 1)) >> 15) as i16;
                        e.em_delta = e.em_seg_end;
                        e.em_cvol_l = e.em_ltgt;
                        e.em_cvol_r = e.em_rtgt;
                    } else {
                        e.em_cvol_l = get_vol(e.em_cvol_l, e.em_delta, e.em_lratm, e.em_lratl);
                        e.em_cvol_r = get_vol(e.em_cvol_r, e.em_delta, e.em_rratm, e.em_rratl);
                    }
                    if e.em_cvol_l == 0 {
                        e.em_cvol_l = 1;
                    }
                    if e.em_cvol_r == 0 {
                        e.em_cvol_r = 1;
                    }
                    match param.kind {
                        ParamKind::SetPan(pan) => {
                            e.em_pan = if headphone {
                                (pan >> 1) + 32
                            } else if mono {
                                64
                            } else {
                                pan
                            };
                        }
                        ParamKind::SetVolume { vol, samples } => {
                            e.em_delta = 0;
                            let fvol = (vol as i32 + vol as i32) / 2;
                            e.em_volume = fvol as i16;
                            e.em_seg_end = sample184(samples);
                        }
                        ParamKind::SetFxAmt(data) => {
                            if (((e.em_dryamt ^ e.em_wetamt) & 1) as i32 ^ ((data + 1) >> 7)) != 0 && surround {
                                if e.em_pan > 64 {
                                    e.em_dryamt ^= 1;
                                } else {
                                    e.em_wetamt ^= 1;
                                }
                            }
                            e.em_dryamt = ((eqp(data & 0x7f) & 0xfffe) | (e.em_dryamt as i32 & 1)) as i16;
                            e.em_wetamt = ((eqp(N_EQPOWER_LENGTH - (data & 0x7f) - 1) & 0xfffe) | (e.em_wetamt as i32 & 1)) as i16;
                        }
                        _ => {}
                    }
                    e.em_first = 1;
                }
                ParamKind::StopVoice => {
                    emitted |= self.pull_sub_frame(pv, &mut inp, samples);
                    self.envmixer_reset(pv);
                }
                ParamKind::FreeVoice(target) => {
                    self.pvoices[target].offset = 0;
                    self.free_pvoice(target);
                }
                ParamKind::SetPitch(pitch) => {
                    emitted |= self.pull_sub_frame(pv, &mut inp, samples);
                    self.pvoices[pv].rs_ratio = pitch;
                }
                ParamKind::Filter11(x) => {
                    emitted |= self.pull_sub_frame(pv, &mut inp, samples);
                    self.pvoices[pv].unk8c = x;
                }
                ParamKind::Filter12(x) => {
                    emitted |= self.pull_sub_frame(pv, &mut inp, samples);
                    self.pvoices[pv].fx_unk02 = x as i16;
                    self.pvoices[pv].unkb8 |= 2;
                }
                ParamKind::Filter13(f) => {
                    emitted |= self.pull_sub_frame(pv, &mut inp, samples);
                    self.pvoices[pv].fx_unk00 = f;
                    self.pvoices[pv].unkb8 |= 2;
                }
            }
            loutp = loutp.wrapping_add(samples << 1);
            out_count -= samples;
            self.pvoices[pv].em_ctrl.pop_front();
            self.free_param();
        }
        let _ = loutp;
        emitted |= self.pull_sub_frame(pv, &mut inp, out_count);
        let e = &mut self.pvoices[pv];
        if e.em_delta > e.em_seg_end {
            e.em_delta = e.em_seg_end;
        }
        emitted
    }

    /// `n_alEnvmixerParam(AL_FILTER_RESET)` and down the chain.
    fn envmixer_reset(&mut self, pv: usize) {
        let e = &mut self.pvoices[pv];
        e.em_first = 1;
        e.em_motion = AL_STOPPED;
        e.em_volume = 1;
        e.em_seg_end = 0;
        e.rs_delta = 0.0;
        e.rs_first = 1;
        e.rs_upitch = 0;
        e.fx_unk02 = 0;
        e.dc_lastsam = 0;
        e.dc_first = 1;
        e.dc_sample = 0;
        if let Some(t) = e.dc_table {
            let w = &self.bank.wavetables[t];
            e.dc_memin = w.base as i32;
            if let Some(l) = w.adpcm_loop {
                e.dc_loop_count = l.count;
            }
        }
    }

    /// `_pullSubFrame` (`n_env.c:289`): one subframe of the voice through
    /// its chain into the outputs, if it plays.
    fn pull_sub_frame(&mut self, pv: usize, inp: &mut i32, out_count: i32) -> bool {
        if self.pvoices[pv].em_motion != AL_PLAYING || out_count == 0 {
            return false;
        }
        self.lpfilter_pull(pv, inp);
        let e = &mut self.pvoices[pv];
        if e.em_first != 0 {
            e.em_first = 0;
            e.em_ltgt = ((e.em_volume as i32 * eqp(e.em_pan as i32)) >> 15) as i16;
            e.em_lratm = get_rate(e.em_cvol_l as f32, e.em_ltgt as f32, e.em_seg_end, &mut e.em_lratl);
            e.em_rtgt = ((e.em_volume as i32 * eqp(N_EQPOWER_LENGTH - e.em_pan as i32 - 1)) >> 15) as i16;
            e.em_rratm = get_rate(e.em_cvol_r as f32, e.em_rtgt as f32, e.em_seg_end, &mut e.em_rratl);
            let (cl, dry, wet, rt, rm, rl, lt, lm, ll, cr) = (e.em_cvol_l, e.em_dryamt, e.em_wetamt, e.em_rtgt, e.em_rratm, e.em_rratl, e.em_ltgt, e.em_lratm, e.em_lratl, e.em_cvol_r);
            self.rsp.set_volume(A_LEFT | A_VOL, cl, dry, wet);
            self.rsp.set_volume(A_RIGHT | A_VOL, rt, rm, rl as i16);
            self.rsp.set_volume(A_RATE, lt, lm, ll as i16);
            self.rsp.env_mixer(A_INIT, &mut self.pvoices[pv].em_state, cr);
        } else {
            self.rsp.env_mixer(A_CONTINUE, &mut self.pvoices[pv].em_state, 0);
        }
        *inp += FIXED_SAMPLE << 1;
        self.pvoices[pv].em_delta += FIXED_SAMPLE;
        true
    }

    /// `n_alLPFilterPull` (`n_resample2.c:8`).
    fn lpfilter_pull(&mut self, pv: usize, outp: &mut i32) {
        self.resample_pull(pv, outp);
        // SUBST: Rare's per-voice low-pass (`n_aNoop` for unk8c 1..63, the
        // pole filter for fx.unk02 > 0) isn't emulated / skipped; only
        // controllers 0x21-0x23 set them and no sequence sends those.
    }

    /// `n_alResamplePull` (`n_resample.c:4`).
    fn resample_pull(&mut self, pv: usize, outp: &mut i32) {
        let mut inp = N_AL_DECODER_OUT;
        if self.pvoices[pv].rs_upitch != 0 {
            self.adpcm_pull(pv, &mut inp, FIXED_SAMPLE);
            self.rsp.dmem_move(inp, *outp, FIXED_SAMPLE << 1);
            return;
        }
        let e = &mut self.pvoices[pv];
        if e.rs_ratio > MAX_RATIO {
            e.rs_ratio = MAX_RATIO;
        }
        e.rs_ratio = (e.rs_ratio * UNITY_PITCH as f32) as i32 as f32;
        e.rs_ratio /= UNITY_PITCH as f32;
        let fin_count = e.rs_delta + e.rs_ratio * FIXED_SAMPLE as f32;
        let in_count = fin_count as i32;
        e.rs_delta = fin_count - in_count as f32;
        self.adpcm_pull(pv, &mut inp, in_count);
        let e = &mut self.pvoices[pv];
        let incr = (e.rs_ratio * UNITY_PITCH as f32) as i32;
        let first = e.rs_first as u8;
        e.rs_first = 0;
        self.rsp.resample(first, incr as u16, &mut self.pvoices[pv].rs_state, inp, 0);
    }

    /// `n_alLoadParam(AL_FILTER_SET_WAVETABLE)` (`n_load.c:198`).
    fn load_set_wavetable(&mut self, pv: usize, wave: usize) {
        let w = &self.bank.wavetables[wave];
        let e = &mut self.pvoices[pv];
        e.dc_table = Some(wave);
        e.dc_memin = w.base as i32;
        e.dc_sample = 0;
        e.dc_book_size = 2 * w.order * w.npredictors * ADPCMVSIZE;
        match w.adpcm_loop {
            Some(l) => {
                e.dc_loop_start = l.start;
                e.dc_loop_end = l.end;
                e.dc_loop_count = l.count;
                e.dc_lstate = l.state;
            }
            None => {
                e.dc_loop_start = 0;
                e.dc_loop_end = 0;
                e.dc_loop_count = 0;
            }
        }
    }

    /// `n_alAdpcmPull` (`n_load.c:12`): `out_count` samples of the voice's
    /// wavetable decoded at `*outp` (moved past the history the decoder
    /// writes first), looping or zero-filling past its end.
    fn adpcm_pull(&mut self, pv: usize, outp: &mut i32, out_count: i32) {
        if out_count == 0 {
            return;
        }
        let mut out_count = out_count;
        let inp = N_AL_DECODER_IN;
        let bank = self.bank.clone();
        let Some(table) = self.pvoices[pv].dc_table else { return };
        let w = &bank.wavetables[table];
        self.rsp.load_adpcm(&w.book, self.pvoices[pv].dc_book_size);
        let e = &self.pvoices[pv];
        let looped = (out_count.wrapping_add(e.dc_sample) as u32 > e.dc_loop_end) && e.dc_loop_count != 0;
        let mut n_sam = if looped { e.dc_loop_end as i32 - e.dc_sample } else { out_count };
        let n_left = if e.dc_lastsam != 0 { ADPCMFSIZE - e.dc_lastsam } else { 0 };
        let mut tsam = (n_sam - n_left).max(0);
        let mut nframes = (tsam + ADPCMFSIZE - 1) >> LFSAMPLES;
        let mut nbytes = nframes * ADPCMFBYTES;

        if looped {
            let first = self.pvoices[pv].dc_first as u8;
            self.decode_chunk(pv, tsam, nbytes, *outp, inp, first);
            let e = &mut self.pvoices[pv];
            if e.dc_lastsam != 0 {
                *outp += e.dc_lastsam << 1;
            } else {
                *outp += ADPCMFSIZE << 1;
            }
            e.dc_lastsam = (e.dc_loop_start & 0xf) as i32;
            e.dc_memin = w.base as i32 + ADPCMFBYTES * ((e.dc_loop_start >> LFSAMPLES) as i32 + 1);
            e.dc_sample = e.dc_loop_start as i32;
            let mut b_end = *outp;
            while out_count > n_sam {
                out_count -= n_sam;
                let op = (b_end + ((nframes + 1) << (LFSAMPLES + 1)) + 16) & !0x1f;
                b_end += n_sam << 1;
                let e = &mut self.pvoices[pv];
                if e.dc_loop_count != -1 && e.dc_loop_count != 0 {
                    e.dc_loop_count -= 1;
                }
                n_sam = out_count.min(e.dc_loop_end.wrapping_sub(e.dc_loop_start) as i32);
                tsam = (n_sam - ADPCMFSIZE + e.dc_lastsam).max(0);
                nframes = (tsam + ADPCMFSIZE - 1) >> LFSAMPLES;
                nbytes = nframes * ADPCMFBYTES;
                let flags = e.dc_first as u8 | A_LOOP;
                self.decode_chunk(pv, tsam, nbytes, op, inp, flags);
                let lastsam = self.pvoices[pv].dc_lastsam;
                self.rsp.dmem_move(op + (lastsam << 1), b_end, n_sam << 1);
            }
            let e = &mut self.pvoices[pv];
            e.dc_lastsam = (out_count + e.dc_lastsam) & 0xf;
            e.dc_sample += out_count;
            e.dc_memin += ADPCMFBYTES * nframes;
            return;
        }

        // The unlooped case.
        let n_sam = nframes << LFSAMPLES;
        let e = &self.pvoices[pv];
        let over_flow = (e.dc_memin + nbytes - (w.base as i32 + w.len)).max(0);
        let mut n_over = (over_flow / ADPCMFBYTES) << LFSAMPLES;
        if n_over > n_sam + n_left {
            n_over = n_sam + n_left;
        }
        nbytes -= over_flow;
        let mut decoded = false;
        if n_over - (n_over & 0xf) < out_count {
            decoded = true;
            let first = e.dc_first as u8;
            self.decode_chunk(pv, n_sam - n_over, nbytes, *outp, inp, first);
            let e = &mut self.pvoices[pv];
            if e.dc_lastsam != 0 {
                *outp += e.dc_lastsam << 1;
            } else {
                *outp += ADPCMFSIZE << 1;
            }
            e.dc_lastsam = (out_count + e.dc_lastsam) & 0xf;
            e.dc_sample += out_count;
            e.dc_memin += ADPCMFBYTES * nframes;
        } else {
            let e = &mut self.pvoices[pv];
            e.dc_lastsam = 0;
            e.dc_memin += ADPCMFBYTES * nframes;
        }
        if n_over != 0 {
            self.pvoices[pv].dc_lastsam = 0;
            let start_zero = if decoded { (n_left + n_sam - n_over) << 1 } else { 0 };
            self.rsp.clear_buffer(start_zero + *outp, n_over << 1);
        }
    }

    /// `_decodeChunk` (`n_load.c:260`). PD's audio DMA (`adma_exec`) hands
    /// back the bytes at some alignment and the decoder starts that far in;
    /// the decoded samples are the same, so this loads them at 0.
    fn decode_chunk(&mut self, pv: usize, tsam: i32, nbytes: i32, outp: i32, inp: i32, flags: u8) {
        if nbytes > 0 {
            let memin = self.pvoices[pv].dc_memin;
            let n = nbytes + 8 - (nbytes & 0x7);
            let tbl = &self.bank.tbl;
            self.rsp.load_bytes(inp, tbl, memin.max(0) as usize, n);
        }
        if flags & A_LOOP != 0 {
            let l = self.pvoices[pv].dc_lstate;
            self.rsp.set_loop(&l);
        }
        let mut state = self.pvoices[pv].dc_state;
        self.rsp.adpcm_dec(flags, &mut state, tsam << 1, inp, outp);
        self.pvoices[pv].dc_state = state;
        self.pvoices[pv].dc_first = 0;
    }
}

/// `_getRate` (`n_env.c:347`): the ramp from `vol` to `tgt` over `count`
/// samples, as the mixer's 16.16 rate per 8 samples.
fn get_rate(vol: f32, tgt: f32, count: i32, ratel: &mut u16) -> i16 {
    if count == 0 {
        if tgt >= vol {
            *ratel = 0xffff;
            return 0x7fff;
        }
        *ratel = 0;
        return -0x8000;
    }
    let invn = 1.0 / count as f32;
    let tgt = if tgt < 1.0 { 1.0 } else { tgt };
    let vol = if vol <= 0.0 { 1.0 } else { vol };
    let a = (tgt - vol) * invn * 8.0;
    let mut s = a as i16;
    let mut f = a - s as f32;
    s = s.wrapping_sub(1);
    f += 1.0;
    let tmp = f as i16;
    s = s.wrapping_add(tmp);
    f -= tmp as f32;
    *ratel = (65535.0 * f) as u16;
    s
}

/// `_getVol` (`n_env.c:393`): the volume after `samples` of a ramp.
fn get_vol(ivol: i16, samples: i32, ratem: i16, ratel: u16) -> i16 {
    let samples = samples >> 3;
    if samples == 0 {
        return ivol;
    }
    let mut sp4 = (ratel as i32).wrapping_mul(samples);
    sp4 >>= 16;
    sp4 = sp4.wrapping_add((ratem as i32).wrapping_mul(samples));
    (ivol as i32).wrapping_add(sp4) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subframes_round_up() {
        assert_eq!(sample184(0), 0);
        assert_eq!(sample184(1), 184);
        assert_eq!(sample184(184), 184);
        assert_eq!(sample184(185), 368);
        assert_eq!(sample184(-100), 0);
    }

    #[test]
    fn a_ramp_reaches_its_target() {
        let mut l = 0;
        let m = get_rate(1.0, 1001.0, 184, &mut l);
        let v = get_vol(1, 184, m, l);
        assert!((v as i32 - 1001).abs() <= 8, "{v}");
        assert_eq!(get_rate(5.0, 10.0, 0, &mut l), 0x7fff);
    }

    #[test]
    fn times_convert_at_the_output_rate() {
        let s = Synth::new(Arc::new(Bank::default()), SynConfig::pd(), 1);
        assert_eq!(s.time_to_samples_no_round(1_000_000), 22018);
        assert_eq!(s.time_to_samples(16000), 352);
    }
}
