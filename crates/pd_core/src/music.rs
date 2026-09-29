//! Perfect Dark's music: the tunes as data, and the one music player the
//! game keeps. PD's music code is in three places, and so is this:
//!
//! * The synth and the three compact-sequence players (`n64::naudio`, what
//!   `snd_init` builds), which [`Music`] owns.
//! * `lib/music.c`'s event queue and `g_SeqChannels`, with `snd.c`'s
//!   `seq_play` and `seq_set_volume`: [`Music::tick_events`] and the queue
//!   calls. `game/music.c`'s half that isn't a match's (the menu track, the
//!   save/restore of the queue interval, `music_reset`, `music_stop`) is
//!   here too.
//! * `game/music.c`'s match half (a match's tune, the death tune and its
//!   timer, multiple-tune switching, `mp_choose_track`, which draws from
//!   the world's RNG) is the world's (`pd_sim::mp::music`), and the menus
//!   choose theirs (`menu_choose_music`, `pd_menu`).
//!
//! Both halves reach this one through [`MusicCall`]s in their event
//! streams (`Event::Music`), which the game applies in PD's frame order: the
//! menus' (`menu_tick`), then the world's, whose `lv_tick` asks for
//! `music_tick_events` ([`MusicCall::Tick`]) at `music_tick`'s place.

use std::sync::Arc;

use n64::naudio::bank::{AdpcmLoop, Bank, Envelope, Instrument, KeyMap, Sound, WaveTable};
use n64::naudio::synth::{SpeakerMode, SynConfig, FRAME_SAMPLES};
use n64::naudio::{Naudio, AL_PLAYING, AL_STOPPED, AL_VOL_FULL};
use serde_json::Value;

use crate::assets::AssetDir;
use crate::ids::*;

/// `ARRAYCOUNT(g_SeqInstances)` (`snd.c:62`).
pub const NUM_SEQ_INSTANCES: usize = 3;
/// `ARRAYCOUNT(g_MusicEventQueue)` (`game/music.c:18`).
const MUSIC_QUEUE_LEN: usize = 40;
/// `g_FadeTargetByTrackType` (`lib/music.c:17`).
const FADE_TARGET_BY_TRACK_TYPE: [u8; 6] = [0, 0, 0, 0, 0, 5];

/// The tunes: seq.ctl's bank with seq.tbl, and each sequence by its
/// `MUSIC_*` number with its `g_SeqVolumes` entry (`assets/music/`).
pub struct MusicData {
    pub bank: Arc<Bank>,
    pub seqs: Vec<Vec<u8>>,
    pub volumes: Vec<i16>,
    pub names: Vec<String>,
}

fn u8f(v: &Value, k: &str) -> u8 {
    v[k].as_u64().unwrap_or(0) as u8
}

fn i32f(v: &Value, k: &str) -> i32 {
    v[k].as_i64().unwrap_or(0) as i32
}

fn instrument(v: &Value) -> Instrument {
    Instrument {
        volume: u8f(v, "volume"),
        pan: u8f(v, "pan"),
        priority: u8f(v, "priority"),
        flags: u8f(v, "flags"),
        trem_type: u8f(v, "tremType"),
        trem_rate: u8f(v, "tremRate"),
        trem_depth: u8f(v, "tremDepth"),
        trem_delay: u8f(v, "tremDelay"),
        vib_type: u8f(v, "vibType"),
        vib_rate: u8f(v, "vibRate"),
        vib_depth: u8f(v, "vibDepth"),
        vib_delay: u8f(v, "vibDelay"),
        bend_range: i32f(v, "bendRange") as i16,
        sounds: v["sounds"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as usize).collect()).unwrap_or_default(),
    }
}

impl MusicData {
    /// `music/bank.json`, `music/seq.tbl` and the sequences in `music/index.json`.
    pub fn load(assets: &AssetDir) -> Result<MusicData, String> {
        let b: Value = assets.read_json(&assets.music("bank.json"))?;
        let tbl = assets.read(&assets.music("seq.tbl"))?;
        let arr = |k: &str| b[k].as_array().cloned().ok_or(format!("music/bank.json: no {k}"));
        let instruments = arr("instruments")?.iter().map(|i| if i.is_null() { None } else { Some(instrument(i)) }).collect();
        let percussion = if b["percussion"].is_null() { None } else { Some(instrument(&b["percussion"])) };
        let sounds = arr("sounds")?
            .iter()
            .map(|s| {
                let e = &s["envelope"];
                let k = &s["keyMap"];
                Sound {
                    envelope: Envelope { attack_time: i32f(e, "attackTime"), decay_time: i32f(e, "decayTime"), release_time: i32f(e, "releaseTime"), attack_volume: u8f(e, "attackVolume"), decay_volume: u8f(e, "decayVolume") },
                    key_map: KeyMap { velocity_min: u8f(k, "velocityMin"), velocity_max: u8f(k, "velocityMax"), key_min: u8f(k, "keyMin"), key_max: u8f(k, "keyMax"), key_base: u8f(k, "keyBase"), detune: i32f(k, "detune") as i8 },
                    wavetable: s["wavetable"].as_u64().unwrap_or(0) as usize,
                    sample_pan: u8f(s, "samplePan"),
                    sample_volume: u8f(s, "sampleVolume"),
                    flags: u8f(s, "flags"),
                }
            })
            .collect();
        let mut wavetables = Vec::new();
        for w in arr("wavetables")? {
            if w["type"].as_u64() != Some(0) {
                return Err("music/bank.json: a wavetable isn't ADPCM (the n_ loader plays only ADPCM, n_load.c)".into());
            }
            let book = &w["book"];
            let adpcm_loop = if w["loop"].is_null() {
                None
            } else {
                let l = &w["loop"];
                let mut state = [0i16; 16];
                for (i, x) in l["state"].as_array().into_iter().flatten().enumerate().take(16) {
                    state[i] = x.as_i64().unwrap_or(0) as i16;
                }
                Some(AdpcmLoop { start: i32f(l, "start") as u32, end: i32f(l, "end") as u32, count: i32f(l, "count"), state })
            };
            wavetables.push(WaveTable {
                base: i32f(&w, "base") as u32,
                len: Bank::frame_len(i32f(&w, "len")),
                order: i32f(book, "order"),
                npredictors: i32f(book, "npredictors"),
                book: book["book"].as_array().into_iter().flatten().map(|x| x.as_i64().unwrap_or(0) as i16).collect(),
                adpcm_loop,
            });
        }
        let bank = Arc::new(Bank { instruments, percussion, sounds, wavetables, tbl });

        let idx: Value = assets.read_json(&assets.music("index.json"))?;
        let (mut seqs, mut volumes, mut names) = (Vec::new(), Vec::new(), Vec::new());
        for s in idx["sequences"].as_array().ok_or("music/index.json: no sequences")? {
            seqs.push(assets.read(&assets.music(s["seq"].as_str().unwrap_or("")))?);
            volumes.push(i32f(s, "volume") as i16);
            names.push(s["id"].as_str().unwrap_or("").to_owned());
        }
        Ok(MusicData { bank, seqs, volumes, names })
    }
}

/// Which volume a start takes, as `game/music.c` picks it when it queues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicVolume {
    /// `music_get_volume()`.
    Music,
    /// `VOLUME(g_SfxVolume)`.
    Sfx,
    /// `VOLUME(g_SfxVolume) > music_get_volume() ? VOLUME(g_SfxVolume) : music_get_volume()`.
    MaxSfxMusic,
}

/// A call into PD's music from the menus or the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MusicCall {
    /// `music_queue_start_event(tracktype, tracknum, fadesecs, volume)`.
    Start { tracktype: i32, tracknum: i32, fadesecs: f32, volume: MusicVolume },
    /// `music_queue_stop_event(tracktype)`.
    Stop { tracktype: i32 },
    /// `music_queue_fade_event(tracktype, secs, keepafterfade)`.
    Fade { tracktype: i32, secs: f32, keep: bool },
    /// `music_queue_stop_all_event()`: the queue emptied and a stop-all run now.
    StopAll,
    /// `music_save_interval()` / `music_restore_interval()`: process the
    /// queue every frame until the queued interval comes back.
    SaveInterval,
    RestoreInterval,
    /// `g_MusicInterval240 = n` (the Soundtrack dialog: 80 open, 15 closed).
    SetInterval(i32),
    /// `music_start_track_as_menu(tracknum)`.
    StartTrackAsMenu(i32),
    /// `music_reset()` (`lv_reset`).
    Reset,
    /// `music_tick`'s `music_tick_events()`, with the frame's `diffframe240`.
    Tick { diffframe240: i32 },
}

/// `struct musicevent`.
#[derive(Clone, Copy, Debug, Default)]
struct MusicEvent {
    tracktype: i32,
    tracknum: i32,
    fadesecs: f32,
    volume: u16,
    eventtype: i32,
    /// `g_MusicNextEventId`'s stamp (PD only prints it).
    #[allow(dead_code)]
    id: u32,
    numattempts: i32,
    failcount: i32,
    keepafterfade: bool,
    timer240: i32,
}

/// `struct seqchannel` (`g_SeqChannels`): what a sequence player is for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeqChannel {
    pub tracktype: i32,
    pub inuse: bool,
    pub keepafterfade: bool,
    pub unk0c: u8,
}

/// `struct seqinstance`'s own fields (its player is `audio.players[i]`).
#[derive(Clone, Copy, Debug, Default)]
struct SeqInstance {
    tracknum: i32,
    volume: u16,
}

const RESULT_FAIL: i32 = 0;
const RESULT_OK_NEXT: i32 = 1;
const RESULT_OK_BREAK: i32 = 2;

/// The music player: the synth and its three sequence players, and the
/// queue that drives them.
pub struct Music {
    pub audio: Naudio,
    data: Arc<MusicData>,
    seqs: [SeqInstance; NUM_SEQ_INSTANCES],
    pub channels: [SeqChannel; NUM_SEQ_INSTANCES],
    queue: Vec<MusicEvent>,
    next_id: u32,
    /// `g_MusicInterval240`: the queue is processed every this many quarter-ticks.
    pub interval240: i32,
    saved_interval240: i32,
    sleep_remaining240: i32,
    diffframe240: i32,
    /// `g_MenuTrack`.
    pub menu_track: i32,
    /// `g_MusicVolume` (NTSC 1.0+).
    music_volume: u16,
    /// `g_SfxVolume`.
    pub sfx_volume: u16,
}

impl Music {
    /// `snd_init`'s music half, with `gamefile_load_defaults`' sound: stereo,
    /// music and SFX at 0x5000 (`gamefile.c:156`).
    pub fn new(data: Arc<MusicData>) -> Music {
        let audio = Naudio::new(data.bank.clone(), SynConfig::pd(), NUM_SEQ_INSTANCES);
        let mut m = Music {
            audio,
            data,
            seqs: [SeqInstance::default(); NUM_SEQ_INSTANCES],
            channels: [SeqChannel::default(); NUM_SEQ_INSTANCES],
            queue: Vec::with_capacity(MUSIC_QUEUE_LEN),
            next_id: 0,
            interval240: 15,
            saved_interval240: -1,
            sleep_remaining240: 0,
            diffframe240: 0,
            menu_track: -1,
            music_volume: 0x5000,
            sfx_volume: 0x5000,
        };
        m.snd_set_sound_mode(SOUNDMODE_STEREO);
        m
    }

    pub fn data(&self) -> &Arc<MusicData> {
        &self.data
    }

    /// `snd_set_sound_mode` (`snd.c:1257`).
    pub fn snd_set_sound_mode(&mut self, mode: i32) {
        let speaker = match mode {
            SOUNDMODE_MONO => SpeakerMode::Mono,
            SOUNDMODE_HEADPHONE => SpeakerMode::Headphone,
            SOUNDMODE_SURROUND => SpeakerMode::Surround,
            _ => SpeakerMode::Stereo,
        };
        let syn = &mut self.audio.syn;
        syn.surround_output_type(speaker);
        syn.surround_reverb_setup(0, 4);
        for i in 1..syn.max_aux_busses() {
            if matches!(mode, SOUNDMODE_STEREO | SOUNDMODE_HEADPHONE | SOUNDMODE_SURROUND) {
                syn.surround_reverb_setup(i, 4);
            }
        }
    }

    /// One audio frame (`amgr_handle_frame_msg`'s `n_alAudioFrame`): 736
    /// stereo samples at 22018 Hz appended to `out`.
    ///
    /// SUBST: PD makes a frame of 736, or 552 when the AI's buffer is full,
    /// each 30 Hz retrace pair / the caller asks for frames as its output
    /// needs them, always 736.
    pub fn render_frame(&mut self, out: &mut Vec<[i16; 2]>) {
        self.audio.audio_frame(FRAME_SAMPLES, out);
    }

    /// Apply a call from the menus or the world.
    pub fn apply(&mut self, call: MusicCall) {
        match call {
            MusicCall::Start { tracktype, tracknum, fadesecs, volume } => {
                let v = self.volume(volume);
                self.music_queue_start_event(tracktype, tracknum, fadesecs, v);
            }
            MusicCall::Stop { tracktype } => self.music_queue_stop_event(tracktype),
            MusicCall::Fade { tracktype, secs, keep } => self.music_queue_fade_event(tracktype, secs, keep),
            MusicCall::StopAll => self.music_queue_stop_all_event(),
            MusicCall::SaveInterval => self.music_save_interval(),
            MusicCall::RestoreInterval => self.music_restore_interval(),
            MusicCall::SetInterval(n) => self.interval240 = n,
            MusicCall::StartTrackAsMenu(t) => self.music_start_track_as_menu(t),
            MusicCall::Reset => self.music_reset(),
            MusicCall::Tick { diffframe240 } => self.tick_events(diffframe240),
        }
    }

    fn volume(&self, v: MusicVolume) -> u16 {
        let sfx = self.sfx_volume.min(0x5000);
        match v {
            MusicVolume::Music => self.music_get_volume(),
            MusicVolume::Sfx => sfx,
            MusicVolume::MaxSfxMusic => sfx.max(self.music_get_volume()),
        }
    }

    // --- game/music.c --------------------------------------------------

    /// `music_get_volume` (`game/music.c:75`).
    pub fn music_get_volume(&self) -> u16 {
        self.music_volume.min(0x5000)
    }

    /// `music_set_volume` (`game/music.c:96`).
    pub fn music_set_volume(&mut self, volume: u16) {
        let volume = volume.min(0x5000);
        for i in 0..NUM_SEQ_INSTANCES {
            if self.channels[i].tracktype != TRACKTYPE_NONE && self.channels[i].tracktype != TRACKTYPE_AMBIENT {
                self.seq_set_volume(i, volume);
            }
        }
        self.music_volume = volume;
    }

    fn push_event(&mut self, e: MusicEvent) {
        // SUBST: PD writes past its 40 events / the queue drops what doesn't fit.
        if self.queue.len() < MUSIC_QUEUE_LEN {
            self.queue.push(e);
        }
    }

    fn new_event(&mut self, tracktype: i32, eventtype: i32) -> MusicEvent {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        MusicEvent { tracktype, eventtype, id, ..MusicEvent::default() }
    }

    /// `music_queue_start_event` (`game/music.c:159`).
    pub fn music_queue_start_event(&mut self, tracktype: i32, tracknum: i32, secs: f32, volume: u16) {
        let mut e = self.new_event(tracktype, MUSICEVENTTYPE_PLAY);
        e.tracknum = tracknum;
        e.fadesecs = secs;
        e.volume = volume;
        self.push_event(e);
    }

    /// `music_queue_stop_event` (`game/music.c:175`).
    pub fn music_queue_stop_event(&mut self, tracktype: i32) {
        let e = self.new_event(tracktype, MUSICEVENTTYPE_STOP);
        self.push_event(e);
    }

    /// `music_queue_fade_event` (`game/music.c:188`).
    pub fn music_queue_fade_event(&mut self, tracktype: i32, secs: f32, keepafterfade: bool) {
        let mut e = self.new_event(tracktype, MUSICEVENTTYPE_FADE);
        e.fadesecs = secs;
        e.keepafterfade = keepafterfade;
        self.push_event(e);
    }

    /// `music_reset` (`game/music.c:202`), less the match's timers.
    pub fn music_reset(&mut self) {
        self.music_save_interval();
        self.music_queue_stop_all_event();
        self.music_restore_interval();
        self.menu_track = -1;
    }

    /// `music_queue_stop_all_event` (`game/music.c:228`).
    pub fn music_queue_stop_all_event(&mut self) {
        let e = self.new_event(TRACKTYPE_6, MUSICEVENTTYPE_STOPALL);
        self.queue.clear();
        self.queue.push(e);
        let d = self.diffframe240;
        self.tick_events(d);
    }

    /// `music_save_interval` (`game/music.c:245`).
    pub fn music_save_interval(&mut self) {
        self.saved_interval240 = self.interval240;
        self.interval240 = 0;
    }

    /// `music_restore_interval` (`game/music.c:251`), with its bug (it resets
    /// the first event's attempts, not the new one's).
    pub fn music_restore_interval(&mut self) {
        let mut e = self.new_event(TRACKTYPE_6, MUSICEVENTTYPE_SETINTERVAL);
        e.timer240 = self.saved_interval240;
        self.push_event(e);
        if let Some(first) = self.queue.first_mut() {
            first.numattempts = 0;
            first.failcount = 0;
        }
    }

    /// `music_stop` (`game/music.c:417`, `lv_stop`).
    pub fn music_stop(&mut self) {
        self.music_save_interval();
        self.music_queue_stop_all_event();
        self.music_restore_interval();
    }

    /// `music_start_track_as_menu` (`game/music.c:358`).
    pub fn music_start_track_as_menu(&mut self, tracknum: i32) {
        if tracknum != self.menu_track {
            let v = self.music_get_volume();
            self.music_queue_stop_event(TRACKTYPE_MENU);
            self.music_queue_stop_event(TRACKTYPE_DEATH);
            self.music_queue_fade_event(TRACKTYPE_PRIMARY, 0.5, true);
            self.music_queue_fade_event(TRACKTYPE_NRG, 0.5, true);
            self.music_queue_fade_event(TRACKTYPE_AMBIENT, 0.5, true);
            self.music_queue_start_event(TRACKTYPE_MENU, tracknum, 0.0, v);
        }
        self.menu_track = tracknum;
    }

    /// `music_is_track_state` (`game/music.c:113`).
    pub fn music_is_track_state(&self, tracktype: i32, state: i32) -> bool {
        for c in &self.channels {
            if c.tracktype == tracktype {
                return match state {
                    AL_STOPPED => !c.inuse,
                    AL_PLAYING => c.inuse,
                    _ => c.keepafterfade,
                };
            }
        }
        false
    }

    /// `music_is_track_type_playing` (`lib/music.c:510`).
    pub fn music_is_track_type_playing(&self, tracktype: i32) -> bool {
        (0..NUM_SEQ_INSTANCES).any(|i| self.channels[i].tracktype == tracktype && self.audio.players[i].get_state() == AL_PLAYING)
    }

    /// The tune sequence player `i` last started (`g_SeqInstances[i].tracknum`).
    pub fn tracknum(&self, i: usize) -> i32 {
        self.seqs[i].tracknum
    }

    // --- snd.c ---------------------------------------------------------

    /// `seq_play` (`snd.c:1606`): tune `tracknum` on sequence player `i`,
    /// if it has stopped. The player only reads as playing once the synth
    /// has run it (the race `lib/music.c:62` describes).
    fn seq_play(&mut self, i: usize, tracknum: i32) -> bool {
        let state = self.audio.players[i].get_state();
        self.seqs[i].tracknum = tracknum;
        if state != AL_STOPPED {
            return false;
        }
        let Some(data) = self.data.seqs.get(tracknum as usize) else { return false };
        let data = data.clone();
        self.audio.players[i].set_seq(&data);
        let v = self.seqs[i].volume;
        self.seq_set_volume(i, v);
        self.audio.players[i].play();
        true
    }

    /// `seq_set_volume` (`snd.c:1670`).
    fn seq_set_volume(&mut self, i: usize, volume: u16) {
        let sv = self.data.volumes.get(self.seqs[i].tracknum as usize).copied().unwrap_or(0) as i32;
        let tmp = ((sv * volume as i32) as u32 >> 15).min(AL_VOL_FULL as u32);
        self.seqs[i].volume = volume;
        self.audio.players[i].set_vol(tmp as i16);
    }

    // --- lib/music.c -----------------------------------------------------

    fn release(&mut self, i: usize) {
        self.audio.players[i].stop();
        self.channels[i] = SeqChannel::default();
    }

    /// `music_handle_play_event` (`lib/music.c:21`).
    fn handle_play_event(&mut self, ev: usize, mut result: i32) -> i32 {
        let e = self.queue[ev];
        for i in 0..NUM_SEQ_INSTANCES {
            if e.tracktype == self.channels[i].tracktype && self.audio.players[i].get_state() == AL_PLAYING {
                // Already playing: unpause it.
                let value = if e.tracktype == TRACKTYPE_AMBIENT { 24 } else { 32 };
                for j in 0..16 {
                    self.audio.players[i].chan_fade(j, 0xff, value);
                }
                self.channels[i].keepafterfade = false;
                self.channels[i].unk0c = 0;
                self.queue[ev].eventtype = 0;
                result = RESULT_OK_BREAK;
                break;
            }
        }
        if result == RESULT_FAIL {
            for i in 0..NUM_SEQ_INSTANCES {
                if self.audio.players[i].get_state() == AL_STOPPED {
                    if self.seq_play(i, e.tracknum) {
                        self.seq_set_volume(i, e.volume);
                        self.channels[i] = SeqChannel { tracktype: e.tracktype, inuse: true, keepafterfade: false, unk0c: 0 };
                        result = RESULT_OK_BREAK;
                    }
                    break;
                }
            }
            if result == RESULT_FAIL {
                let mut index = None;
                for i in 0..NUM_SEQ_INSTANCES {
                    if (self.channels[i].tracktype == TRACKTYPE_NONE || e.tracktype == self.channels[i].tracktype) && self.audio.players[i].get_state() != AL_STOPPED {
                        index = Some(i);
                        break;
                    }
                }
                if index.is_none() && e.failcount >= 3 {
                    index = (0..NUM_SEQ_INSTANCES).find(|&i| self.channels[i].tracktype == TRACKTYPE_AMBIENT && self.audio.players[i].get_state() != AL_STOPPED);
                }
                match index {
                    Some(i) => self.release(i),
                    None => {
                        self.queue[ev].failcount += 1;
                        if self.queue[ev].failcount >= 6 {
                            result = RESULT_OK_BREAK;
                        }
                    }
                }
            }
        }
        result
    }

    /// `music_handle_stop_event` (`lib/music.c:185`).
    fn handle_stop_event(&mut self, ev: usize) -> i32 {
        let tt = self.queue[ev].tracktype;
        if let Some(i) = (0..NUM_SEQ_INSTANCES).find(|&i| self.channels[i].tracktype == tt) {
            self.release(i);
        }
        RESULT_OK_NEXT
    }

    /// `music_handle_fade_event` (`lib/music.c:204`): every channel of the
    /// track's player fades to its target, 32 a step (whatever the seconds).
    fn handle_fade_event(&mut self, ev: usize) -> i32 {
        let e = self.queue[ev];
        for i in 0..NUM_SEQ_INSTANCES {
            if e.tracktype == self.channels[i].tracktype && self.channels[i].inuse {
                let target = FADE_TARGET_BY_TRACK_TYPE[e.tracktype.clamp(0, 5) as usize];
                for j in 0..16 {
                    self.audio.players[i].chan_fade(j, target, 32);
                }
                self.channels[i].inuse = e.keepafterfade;
                self.channels[i].keepafterfade = e.keepafterfade;
                self.channels[i].unk0c = self.audio.players[i].chan_state[0].fadevolcurrent;
            }
        }
        RESULT_OK_NEXT
    }

    /// `music_handle_stop_all_event` (`lib/music.c:224`).
    fn handle_stop_all_event(&mut self) -> i32 {
        for i in 0..NUM_SEQ_INSTANCES {
            self.release(i);
        }
        RESULT_OK_NEXT
    }

    /// `music_tick_events` (`lib/music.c:249`): release the players whose
    /// fade has finished, drop the events later ones supersede, then (on the
    /// interval) run the queue until a play is done or one can't be yet.
    pub fn tick_events(&mut self, diffframe240: i32) {
        self.diffframe240 = diffframe240;
        for i in 0..NUM_SEQ_INSTANCES {
            if !self.channels[i].inuse && self.audio.players[i].get_state() == AL_PLAYING {
                let cur = self.audio.players[i].chan_state[0].fadevolcurrent;
                let target = FADE_TARGET_BY_TRACK_TYPE[self.channels[i].tracktype.clamp(0, 5) as usize];
                if cur <= target || cur == self.channels[i].unk0c {
                    self.release(i);
                }
            }
        }

        // Mark the superseded events (tracktype none), then compact.
        for i in (0..self.queue.len()).rev() {
            let e = self.queue[i];
            if e.eventtype == MUSICEVENTTYPE_SETINTERVAL || e.tracktype == TRACKTYPE_NONE {
                continue;
            }
            for j in (0..i).rev() {
                if e.eventtype == MUSICEVENTTYPE_STOPALL {
                    self.queue[j].tracktype = TRACKTYPE_NONE;
                    continue;
                }
                let earlier = self.queue[j];
                if earlier.eventtype == MUSICEVENTTYPE_SETINTERVAL || earlier.tracktype == TRACKTYPE_NONE {
                    continue;
                }
                if earlier.tracktype == e.tracktype {
                    let drop = match e.eventtype {
                        MUSICEVENTTYPE_STOP => true,
                        MUSICEVENTTYPE_PLAY => matches!(earlier.eventtype, MUSICEVENTTYPE_PLAY | MUSICEVENTTYPE_FADE),
                        MUSICEVENTTYPE_FADE => earlier.eventtype == MUSICEVENTTYPE_FADE,
                        _ => false,
                    };
                    if drop {
                        self.queue[j].tracktype = TRACKTYPE_NONE;
                    }
                }
            }
        }
        self.queue.retain(|e| e.tracktype != TRACKTYPE_NONE);

        if self.interval240 == 0 || self.sleep_remaining240 < diffframe240 {
            self.sleep_remaining240 = self.interval240;
            while !self.queue.is_empty() {
                self.queue[0].numattempts += 1;
                let result = match self.queue[0].eventtype {
                    MUSICEVENTTYPE_PLAY => self.handle_play_event(0, RESULT_FAIL),
                    MUSICEVENTTYPE_STOP => self.handle_stop_event(0),
                    MUSICEVENTTYPE_FADE => self.handle_fade_event(0),
                    MUSICEVENTTYPE_STOPALL => self.handle_stop_all_event(),
                    MUSICEVENTTYPE_SETINTERVAL => {
                        self.interval240 = self.queue[0].timer240;
                        RESULT_OK_NEXT
                    }
                    _ => RESULT_FAIL,
                };
                if result == RESULT_FAIL {
                    break;
                }
                self.queue.remove(0);
                if result == RESULT_OK_BREAK {
                    break;
                }
            }
        }
        if self.interval240 != 0 {
            self.sleep_remaining240 -= diffframe240;
        } else {
            self.sleep_remaining240 = 0;
        }
    }

    /// How many events wait in the queue.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use n64::naudio::abi::{Rsp, A_CONTINUE, A_INIT};
    use n64::naudio::cseq::{CSeq, SeqEvent};

    use super::*;

    fn assets() -> AssetDir {
        AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))
    }

    fn data() -> Arc<MusicData> {
        Arc::new(MusicData::load(&assets()).unwrap())
    }

    fn hash16(samples: &[i16]) -> u32 {
        samples.iter().fold(0u32, |h, &s| h.wrapping_mul(31).wrapping_add(s as u16 as u32))
    }

    /// The RSP's decoder over whole wavetables, 16 frames a command as the
    /// load filter feeds it, matches the exporter's Python decoder
    /// (pd_sfx.decode_vadpcm, the PC port's scalar aADPCMdec) sample for sample.
    #[test]
    fn adpcm_decodes_as_the_exporter_does() {
        let d = data();
        let idx: Value = assets().read_json(&assets().music("index.json")).unwrap();
        let checks = idx["adpcm_checks"].as_array().unwrap();
        assert!(checks.len() >= 3);
        for c in checks {
            let w = &d.bank.wavetables[c["wavetable"].as_u64().unwrap() as usize];
            let mut rsp = Rsp::new();
            rsp.load_adpcm(&w.book, 2 * w.order * w.npredictors * 8);
            let mut state = [0i16; 16];
            let mut pcm = Vec::new();
            let frames = w.len / 9;
            let mut f = 0;
            while f < frames {
                let n = (frames - f).min(16);
                rsp.load_bytes(0, &d.bank.tbl, (w.base as i32 + 9 * f) as usize, 9 * n);
                rsp.adpcm_dec(if f == 0 { A_INIT } else { A_CONTINUE }, &mut state, 32 * n, 0, 1024);
                for i in 0..16 * n {
                    pcm.push(rsp.s16(1024 + 32 + 2 * i));
                }
                f += n;
            }
            assert_eq!(pcm.len() as u64, c["samples"].as_u64().unwrap());
            let first: Vec<i16> = c["first"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap() as i16).collect();
            assert_eq!(&pcm[..first.len()], &first[..], "wavetable {}", c["wavetable"]);
            assert_eq!(hash16(&pcm) as u64, c["hash"].as_u64().unwrap(), "wavetable {}", c["wavetable"]);
        }
    }

    /// Every sequence read to its end, loops not taken, gives the exporter's
    /// counts (two readers of n_csq.c, one in Python).
    #[test]
    fn every_sequence_reads_as_the_exporter_reads_it() {
        let d = data();
        let idx: Value = assets().read_json(&assets().music("index.json")).unwrap();
        let seqs = idx["sequences"].as_array().unwrap();
        assert_eq!(seqs.len(), 119);
        assert_eq!(d.names[MUSIC_DARK_COMBAT as usize], "MUSIC_DARK_COMBAT");
        for (i, s) in seqs.iter().enumerate() {
            let mut seq = CSeq::new(&d.seqs[i]);
            let (mut events, mut notes) = (0u64, 0u64);
            loop {
                let e = seq.next_event(false);
                events += 1;
                if let SeqEvent::Midi { status, .. } = e {
                    if status & 0xf0 == 0x90 {
                        notes += 1;
                    }
                }
                if matches!(e, SeqEvent::SeqEnd { .. }) {
                    break;
                }
                assert!(events < 100_000, "{} runs on", s["id"]);
            }
            let c = &s["check"];
            assert_eq!((events, notes, seq.last_ticks as u64), (c["events"].as_u64().unwrap(), c["notes"].as_u64().unwrap(), c["ticks"].as_u64().unwrap()), "{}", s["id"]);
            assert_eq!(seq.division(), 384);
        }
    }

    fn frames(m: &mut Music, n: usize) -> Vec<[i16; 2]> {
        let mut out = Vec::new();
        for _ in 0..n {
            m.apply(MusicCall::Tick { diffframe240: 8 });
            m.render_frame(&mut out);
        }
        out
    }

    fn loud(out: &[[i16; 2]]) -> u16 {
        out.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0)
    }

    /// The Combat Simulator's menu tune starts on a free player as the menu
    /// track and plays; a match's reset stops it, its primary starts; the
    /// death tune pauses the primary to 0 while it plays, and its end fades
    /// it out and the primary comes back up.
    #[test]
    fn the_queue_starts_pauses_and_resumes_tunes() {
        let mut m = Music::new(data());
        m.apply(MusicCall::StartTrackAsMenu(MUSIC_COMBATSIM_MENU));
        let out = frames(&mut m, 30);
        let menu = m.channels.iter().position(|c| c.tracktype == TRACKTYPE_MENU).expect("a menu channel");
        assert!(m.channels[menu].inuse);
        assert_eq!(m.audio.players[menu].get_state(), AL_PLAYING);
        assert_eq!(m.tracknum(menu), MUSIC_COMBATSIM_MENU);
        assert!(loud(&out[out.len() / 2..]) > 1000, "the menu tune sounds");
        // The same tune again is ignored (g_MenuTrack).
        m.apply(MusicCall::StartTrackAsMenu(MUSIC_COMBATSIM_MENU));
        assert_eq!(m.queue_len(), 0);

        // lv_reset: music_reset, then the stage's primary.
        m.apply(MusicCall::Reset);
        m.apply(MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum: MUSIC_DARK_COMBAT, fadesecs: 0.0, volume: MusicVolume::Music });
        let _ = frames(&mut m, 30);
        let prim = m.channels.iter().position(|c| c.tracktype == TRACKTYPE_PRIMARY).expect("a primary channel");
        assert!(m.channels.iter().all(|c| c.tracktype != TRACKTYPE_MENU), "the menu tune stopped");
        assert_eq!(m.tracknum(prim), MUSIC_DARK_COMBAT);
        assert_eq!(m.audio.players[prim].chan_state[0].fadevolcurrent, 255);

        // music_start_mp_death.
        for c in [
            MusicCall::SaveInterval,
            MusicCall::Stop { tracktype: TRACKTYPE_MENU },
            MusicCall::Stop { tracktype: TRACKTYPE_DEATH },
            MusicCall::Stop { tracktype: TRACKTYPE_AMBIENT },
            MusicCall::Fade { tracktype: TRACKTYPE_PRIMARY, secs: 0.1, keep: true },
            MusicCall::SaveInterval,
            MusicCall::Start { tracktype: TRACKTYPE_DEATH, tracknum: MUSIC_DEATH_MP, fadesecs: 0.0, volume: MusicVolume::MaxSfxMusic },
            MusicCall::RestoreInterval,
            MusicCall::RestoreInterval,
        ] {
            m.apply(c);
        }
        let _ = frames(&mut m, 30);
        let death = m.channels.iter().position(|c| c.tracktype == TRACKTYPE_DEATH).expect("a death channel");
        assert_eq!(m.tracknum(death), MUSIC_DEATH_MP);
        assert_eq!(m.audio.players[prim].chan_state[0].fadevolcurrent, 0, "the primary paused");
        assert!(m.channels[prim].inuse && m.channels[prim].keepafterfade);
        // PD's bug: _music_start_mp_death saves the interval inside
        // music_start_mp_death's own save (game/music.c:452, :465), so the
        // saved 15 is lost and the queue runs every frame from now on.
        assert_eq!(m.interval240, 0, "the nested save lost the interval");

        // music_end_death: fade the death tune out, resume the primary.
        m.apply(MusicCall::Fade { tracktype: TRACKTYPE_DEATH, secs: 2.0, keep: false });
        m.apply(MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum: MUSIC_SKEDAR_MYSTERY, fadesecs: 2.0, volume: MusicVolume::Music });
        let _ = frames(&mut m, 60);
        assert_eq!(m.audio.players[prim].chan_state[0].fadevolcurrent, 255, "the primary is back up");
        assert_eq!(m.tracknum(prim), MUSIC_DARK_COMBAT, "resumed, not restarted");
        assert!(m.channels.iter().all(|c| c.tracktype != TRACKTYPE_DEATH), "the death tune released");

        // lv_stop: music_stop.
        m.music_stop();
        let _ = frames(&mut m, 10);
        assert!(m.audio.players.iter().all(|p| p.get_state() == AL_STOPPED));
        assert!(m.channels.iter().all(|c| *c == SeqChannel::default()));
    }

    /// Every tune the Combat Simulator can play renders for 5 seconds.
    #[test]
    fn every_mp_tune_renders() {
        let d = data();
        for t in [MUSIC_MAINMENU, MUSIC_COMBATSIM_MENU, MUSIC_COMBATSIM_COMPLETE, MUSIC_DEATH_MP, MUSIC_DARK_COMBAT, MUSIC_SKEDAR_MYSTERY, MUSIC_CI_OPERATIVE, MUSIC_DATADYNE_ACTION, MUSIC_MAIAN_TEARS, MUSIC_ALIEN_CONFLICT, MUSIC_CREDITS] {
            let mut m = Music::new(d.clone());
            m.apply(MusicCall::StartTrackAsMenu(t));
            let out = frames(&mut m, 150);
            assert!(loud(&out) > 1000, "{} is silent", d.names[t as usize]);
        }
    }
}
