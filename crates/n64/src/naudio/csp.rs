//! The compact sequence player (`N_ALCSPlayer`): `n_csplayer.c`'s voice
//! handler and MIDI interpreter, `n_seqplayer.c`'s voice and channel
//! helpers, `n_event.c`'s event queue and the game's calls (`n_csp*.c`,
//! `n_seqpstop.c`).
//!
//! The synth calls [`CSPlayer::voice_handler`] when the player's next event
//! is due; it handles every event due now (a sequence's next MIDI event, a
//! note's end, an envelope segment, an oscillator tick, a call the game
//! queued) and returns the microseconds to the next. The game's calls only
//! post events, which the handler picks up within `AL_USEC_PER_FRAME` (the
//! `AL_SEQP_API_EVT` it keeps reposting).

use std::sync::Arc;

use super::bank::{Bank, Instrument};
use super::cseq::{CSeq, SeqEvent};
use super::osc::{depth2cents, OscPool, CSP_TIME_LOOKUP};
use super::synth::{Synth, VoiceId};
use super::*;

/// MIDI status bytes (`libaudio.h:453`).
pub const AL_MIDI_NOTE_OFF: u8 = 0x80;
pub const AL_MIDI_NOTE_ON: u8 = 0x90;
pub const AL_MIDI_POLY_KEY_PRESSURE: u8 = 0xa0;
pub const AL_MIDI_CONTROL_CHANGE: u8 = 0xb0;
pub const AL_MIDI_PROGRAM_CHANGE: u8 = 0xc0;
pub const AL_MIDI_CHANNEL_PRESSURE: u8 = 0xd0;
pub const AL_MIDI_PITCH_BEND_CHANGE: u8 = 0xe0;

/// Controllers (`enum AL_MIDIctrl`, `libaudio.h:495`).
pub const AL_MIDI_OSC_CTRL: u8 = 0x01;
pub const AL_MIDI_PITCH_CTRL: u8 = 0x02;
pub const AL_MIDI_BENDRANGE_MINOR_CTRL: u8 = 0x03;
pub const AL_MIDI_BENDRANGE_MAJOR_CTRL: u8 = 0x04;
pub const AL_MIDI_VOLUME_CTRL: u8 = 0x07;
pub const AL_MIDI_PAN_CTRL: u8 = 0x0a;
pub const AL_MIDI_VIBTYPE_CTRL: u8 = 0x0b;
pub const AL_MIDI_VIBRATE_CTRL: u8 = 0x0c;
pub const AL_MIDI_VIBDEPTH_CTRL: u8 = 0x0d;
pub const AL_MIDI_VIBDELAY_CTRL: u8 = 0x0e;
pub const AL_MIDI_TREMTYPE_CTRL: u8 = 0x0f;
pub const AL_MIDI_PRIORITY_CTRL: u8 = 0x10;
pub const AL_MIDI_TREMRATE_CTRL: u8 = 0x11;
pub const AL_MIDI_TREMDEPTH_CTRL: u8 = 0x12;
pub const AL_MIDI_TREMDELAY_CTRL: u8 = 0x13;
pub const AL_MIDI_ATTACKTIME_CTRL: u8 = 0x14;
pub const AL_MIDI_ATTACKVOL_CTRL: u8 = 0x15;
pub const AL_MIDI_DECAYTIME_CTRL: u8 = 0x16;
pub const AL_MIDI_DECAYVOL_CTRL: u8 = 0x17;
pub const AL_MIDI_RELEASETIME_CTRL: u8 = 0x18;
pub const AL_MIDI_TIMEINDEX_CTRL: u8 = 0x19;
pub const AL_MIDI_MP3_CTRL: u8 = 0x1a;
pub const AL_MIDI_OSMESG_CTRL: u8 = 0x1e;
pub const AL_MIDI_INST_MAJOR_CTRL: u8 = 0x20;
pub const AL_MIDI_UNK11_CTRL: u8 = 0x21;
pub const AL_MIDI_UNK12_CTRL: u8 = 0x22;
pub const AL_MIDI_UNK13_CTRL: u8 = 0x23;
pub const AL_MIDI_SUSTAIN_CTRL: u8 = 0x40;
pub const AL_MIDI_FXMIX80_CTRL: u8 = 0x41;
pub const AL_MIDI_FXMIX7F_CTRL: u8 = 0x5b;
pub const AL_MIDI_FXBUS_CTRL: u8 = 0x5c;
pub const AL_MIDI_FADEEND_CTRL: u8 = 0xfc;
pub const AL_MIDI_FADESPEED_CTRL: u8 = 0xfd;
pub const AL_MIDI_SETFADEINC_CTRL: u8 = 0xfe;
pub const AL_MIDI_FADESTART_CTRL: u8 = 0xff;

/// Envelope phases (`libaudio.h:661`).
const AL_PHASE_ATTACK: u8 = 0;
const AL_PHASE_NOTEON: u8 = 0;
const AL_PHASE_DECAY: u8 = 1;
const AL_PHASE_SUSTAIN: u8 = 2;
const AL_PHASE_RELEASE: u8 = 3;
const AL_PHASE_SUSTREL: u8 = 4;

const AL_SUSTAIN: u8 = 63;
const AL_PAN_CENTER: i32 = 64;
const AL_DEFAULT_PRIORITY: u8 = 5;
const AL_DEFAULT_FXMIX: u8 = 0;

/// `ALMIDIEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Midi {
    pub ticks: u32,
    pub status: u8,
    pub byte1: u8,
    pub byte2: u8,
    pub duration: u32,
}

/// `N_ALEvent`: the player's event types (`enum ALMsg`, `libaudio.h:419`)
/// with their payloads. Voices are the player's voice-state indexes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    SeqRef,
    SeqMidi(Midi),
    SeqpMidi(Midi),
    Tempo { status: u8, ty: u8, byte1: u8, byte2: u8, byte3: u8 },
    SeqEnd,
    NoteEnd(usize),
    Env { vs: usize, vol: u8, delta: i32 },
    Api,
    Vol(i16),
    Priority { chan: u8, priority: u8 },
    Seq,
    Bank,
    Play,
    Stop,
    Stopping,
    TrackEnd,
    LoopStart,
    LoopEnd,
    CspNoteOff(Midi),
    TremOsc { vs: usize, osc: usize },
    VibOsc { vs: usize, osc: usize, chan: u8 },
    /// An empty queue (`evt->type = -1`).
    None,
}

impl Ev {
    fn kind(&self) -> u8 {
        match self {
            Ev::SeqRef => 0,
            Ev::SeqMidi(_) => 1,
            Ev::SeqpMidi(_) => 2,
            Ev::Tempo { .. } => 3,
            Ev::SeqEnd => 4,
            Ev::NoteEnd(_) => 5,
            Ev::Env { .. } => 6,
            Ev::Api => 9,
            Ev::Vol(_) => 10,
            Ev::Priority { .. } => 12,
            Ev::Seq => 13,
            Ev::Bank => 14,
            Ev::Play => 15,
            Ev::Stop => 16,
            Ev::Stopping => 17,
            Ev::TrackEnd => 18,
            Ev::LoopStart => 19,
            Ev::LoopEnd => 20,
            Ev::CspNoteOff(_) => 21,
            Ev::TremOsc { .. } => 22,
            Ev::VibOsc { .. } => 23,
            Ev::None => 255,
        }
    }
}

/// `ALEventQueue` (`n_event.c`): events in time order, each delta relative
/// to the one before; `capacity` items, the last free one only for posts
/// that insist (`arg3`).
#[derive(Clone, Debug)]
pub struct EventQueue {
    items: Vec<(i32, Ev)>,
    capacity: usize,
}

impl EventQueue {
    fn new(capacity: usize) -> EventQueue {
        EventQueue { items: Vec::with_capacity(capacity), capacity }
    }

    /// `n_alEvtqPostEvent` (`n_event.c:52`).
    pub fn post(&mut self, evt: Ev, delta: i32, arg3: bool) {
        let free = self.capacity - self.items.len();
        if free == 0 || (free == 1 && !arg3) {
            return;
        }
        let post_at_end = delta == AL_EVTQ_END;
        let mut delta = delta;
        for i in 0..=self.items.len() {
            if i == self.items.len() {
                self.items.push((if post_at_end { 0 } else { delta }, evt));
                return;
            }
            if delta < self.items[i].0 {
                self.items[i].0 -= delta;
                self.items.insert(i, (delta, evt));
                return;
            }
            delta -= self.items[i].0;
        }
    }

    /// `n_alEvtqNextEvent` (`n_event.c:24`).
    fn next_event(&mut self) -> (Ev, i32) {
        if self.items.is_empty() {
            return (Ev::None, 0);
        }
        let (d, e) = self.items.remove(0);
        (e, d)
    }

    /// Unlink item `i`, its delta moving to the next.
    fn remove(&mut self, i: usize) -> (i32, Ev) {
        let it = self.items.remove(i);
        if let Some(next) = self.items.get_mut(i) {
            next.0 += it.0;
        }
        it
    }

    /// `n_alEvtqFlushType` (`n_event.c:103`).
    fn flush_type(&mut self, kind: u8) {
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].1.kind() == kind {
                self.remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// `ALChanState` (`libaudio.h:686`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ChanState {
    pub instrument: Option<usize>,
    pub bend_range: i16,
    pub fx_id: u8,
    pub pan: u8,
    pub priority: u8,
    pub vol: u8,
    pub fxmix: u8,
    pub fxbus: u8,
    pub sustain: u8,
    pub fadevolcurrent: u8,
    pub fadevoltarget: u8,
    pub fadevolinc: u8,
    pub notemesgflags: u8,
    pub unk11: u8,
    pub unk12: u8,
    pub unk13: u8,
    pub pitch_bend: f32,
    pub attack_time: i32,
    pub decay_time: i32,
    pub release_time: i32,
    pub usechanparams: u8,
    pub attack_volume: u8,
    pub decay_volume: u8,
    pub pitch: i8,
    pub trem_type: u8,
    pub trem_rate: u8,
    pub trem_depth: u8,
    pub trem_delay: u8,
    pub vib_type: u8,
    pub vib_rate: u8,
    pub vib_depth: u8,
    pub vib_delay: u8,
    pub timeindex: u8,
    pub instmajor: u8,
}

/// `N_ALVoiceState` (`n_libaudio.h:212`).
#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceState {
    pub voice: VoiceId,
    pub sound: Option<usize>,
    pub env_end_time: i32,
    pub pitch: f32,
    pub vibrato: f32,
    pub env_gain: u8,
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    pub env_phase: u8,
    pub phase: u8,
    pub tremelo: u8,
    pub flags: u8,
    pub osc_state: Option<usize>,
    pub osc_state2: Option<usize>,
}

/// `N_ALCSPlayer` (`n_libaudio.h:264`), with the `seqinstance`'s sequence.
pub struct CSPlayer {
    pub index: usize,
    /// `node.samplesLeft`: the synth sample of the next callback.
    pub samples_left: i32,
    /// `g_SeqInstances[i].seq`: what `n_alCSeqNew` built; `target` is
    /// whether the player's `target` points at it.
    seq: Option<CSeq>,
    target: bool,
    has_bank: bool,
    pub cur_time: i32,
    pub uspt: i32,
    next_delta: i32,
    pub state: i32,
    pub chan_mask: u16,
    pub vol: i16,
    max_channels: usize,
    next_event: Ev,
    pub evtq: EventQueue,
    frame_time: i32,
    pub chan_state: Vec<ChanState>,
    pub vs: Vec<VoiceState>,
    /// `vAllocHead`..`vAllocTail`.
    alloc: Vec<usize>,
    /// `vFreeList`, top last.
    vfree: Vec<usize>,
    voicecount: u8,
    voicelimit: u8,
    fxmixmajor: f32,
    fxmixmega: f32,
    /// Note-ons played, dropped for want of a sound at the key, dropped for
    /// want of a voice (not PD's; for the tools).
    pub notes: [u32; 3],
}

impl CSPlayer {
    /// `n_alCSPNew` (`n_csplayer.c:84`) for player `index` of the synth,
    /// added at synth time `now` (`n_alSynAddSeqPlayer`).
    pub fn new(index: usize, max_voices: usize, max_events: usize, max_channels: usize, now: i32) -> CSPlayer {
        let mut p = CSPlayer {
            index,
            samples_left: now,
            seq: None,
            target: false,
            has_bank: false,
            cur_time: 0,
            uspt: 488,
            next_delta: 0,
            state: AL_STOPPED,
            chan_mask: 0xffff,
            vol: AL_VOL_FULL,
            max_channels,
            next_event: Ev::Api,
            evtq: EventQueue::new(max_events),
            frame_time: AL_USEC_PER_FRAME,
            chan_state: vec![ChanState::default(); max_channels],
            vs: (0..max_voices).map(|i| VoiceState { voice: VoiceId((index * max_voices + i) as u16), ..VoiceState::default() }).collect(),
            alloc: Vec::new(),
            vfree: (0..max_voices).collect(),
            voicecount: 0,
            voicelimit: max_voices as u8,
            fxmixmajor: 0.0,
            fxmixmega: 1.0,
            notes: [0; 3],
        };
        p.all_chan_on();
        p.init_chan_state();
        p
    }

    // --- the game's calls -------------------------------------------------

    /// `n_alCSPGetState`.
    pub fn get_state(&self) -> i32 {
        self.state
    }

    /// `n_alCSPPlay`.
    pub fn play(&mut self) {
        self.evtq.post(Ev::Play, 0, false);
    }

    /// `n_alSeqpStop`.
    pub fn stop(&mut self) {
        self.evtq.post(Ev::Stopping, 0, false);
    }

    /// `n_alCSPSetVol`.
    pub fn set_vol(&mut self, vol: i16) {
        self.evtq.post(Ev::Vol(vol), 0, false);
    }

    /// `n_alCSeqNew(&seq->seq, data)` + `n_alCSPSetSeq(seqp, &seq->seq)`.
    pub fn set_seq(&mut self, data: &[u8]) {
        self.seq = Some(CSeq::new(data));
        self.evtq.post(Ev::Seq, 0, false);
    }

    /// `n_alCSPSetBank`.
    pub fn set_bank(&mut self) {
        self.evtq.post(Ev::Bank, 0, false);
    }

    /// `n_alCSPSendMidi` (`n_cspsendmidi.c`).
    pub fn send_midi(&mut self, ticks: i32, status: u8, byte1: u8, byte2: u8) {
        self.evtq.post(Ev::SeqpMidi(Midi { ticks: 0, status, byte1, byte2, duration: 0 }), ticks, false);
    }

    /// `n_alCSPChanFade` (`n_cspchan.c:38`).
    pub fn chan_fade(&mut self, chan: u8, targetvol: u8, incvol: u8) {
        self.send_midi(0, AL_MIDI_CONTROL_CHANGE | chan, AL_MIDI_FADESPEED_CTRL, incvol);
        self.send_midi(0, AL_MIDI_CONTROL_CHANGE | chan, AL_MIDI_FADESTART_CTRL, targetvol);
    }

    /// `n_alCSPAllChanOn` (`n_cspchan.c:5`).
    fn all_chan_on(&mut self) {
        self.chan_mask = 0xffff;
        for c in &mut self.chan_state {
            c.fadevoltarget = 255;
            c.fadevolcurrent = 255;
        }
    }

    pub fn voices_playing(&self) -> usize {
        self.alloc.len()
    }

    fn vid(&self, vs: usize) -> VoiceId {
        self.vs[vs].voice
    }

    // --- the handler ------------------------------------------------------

    /// `__n_CSPVoiceHandler` (`n_csplayer.c:148`).
    pub fn voice_handler(&mut self, syn: &mut Synth, osc: &mut OscPool) -> i32 {
        let bank = syn.bank().clone();
        loop {
            match self.next_event {
                Ev::SeqRef => self.handle_next_seq_event(syn, osc, &bank),
                Ev::Api => self.evtq.post(Ev::Api, self.frame_time, true),
                Ev::NoteEnd(vs) => {
                    let v = self.vid(vs);
                    syn.stop_voice(v);
                    syn.free_voice(v);
                    if self.vs[vs].flags != 0 {
                        self.stop_osc(osc, vs);
                    }
                    self.unmap_voice(vs);
                }
                Ev::Env { vs, vol, delta } => {
                    if self.vs[vs].env_phase == AL_PHASE_ATTACK {
                        self.vs[vs].env_phase = AL_PHASE_DECAY;
                    }
                    self.vs[vs].env_end_time = self.cur_time.wrapping_add(delta);
                    self.vs[vs].env_gain = vol;
                    let v = self.vid(vs);
                    let vol = self.vs_vol(vs, &bank);
                    syn.set_vol(v, vol, delta);
                }
                Ev::TremOsc { vs, osc: o } => {
                    let mut value = 0.0;
                    let delta = osc.update(o, &mut value);
                    self.vs[vs].tremelo = value as u8;
                    let v = self.vid(vs);
                    let (vol, d) = (self.vs_vol(vs, &bank), self.vs_delta(vs, self.cur_time));
                    syn.set_vol(v, vol, d);
                    self.evtq.post(Ev::TremOsc { vs, osc: o }, delta, false);
                }
                Ev::VibOsc { vs, osc: o, chan } => {
                    let mut value = 0.0;
                    let delta = osc.update(o, &mut value);
                    self.vs[vs].vibrato = value;
                    let v = self.vid(vs);
                    let c = self.chan_state[chan as usize];
                    syn.set_pitch(v, self.vs[vs].pitch * self.vs[vs].vibrato * c.pitch_bend);
                    if c.unk11 != 0 {
                        let s = &bank.sounds[self.vs[vs].sound.unwrap_or(0)];
                        let semis = c.unk12 as i32 + (self.vs[vs].key as i32 - s.key_map.key_base as i32) - 64;
                        syn.filter13(v, 440.0 * al_semitones2ratio(semis) * c.pitch_bend * self.vs[vs].vibrato);
                    }
                    self.evtq.post(Ev::VibOsc { vs, osc: o, chan }, delta, false);
                }
                Ev::SeqpMidi(m) => self.handle_midi(syn, osc, &bank, m, Some(Ev::SeqpMidi(m))),
                Ev::CspNoteOff(m) => self.handle_midi(syn, osc, &bank, m, Some(Ev::CspNoteOff(m))),
                Ev::Vol(vol) => {
                    self.vol = vol;
                    for i in 0..self.alloc.len() {
                        let vs = self.alloc[i];
                        let v = self.vid(vs);
                        let (vol, d) = (self.vs_vol(vs, &bank), self.vs_delta(vs, self.cur_time));
                        syn.set_vol(v, vol, d);
                    }
                }
                Ev::Play => {
                    if self.state != AL_PLAYING {
                        self.state = AL_PLAYING;
                        self.post_next_seq_event();
                    }
                }
                Ev::Stop => {
                    if self.state == AL_STOPPING {
                        while let Some(&vs) = self.alloc.first() {
                            let v = self.vid(vs);
                            syn.stop_voice(v);
                            syn.free_voice(v);
                            if self.vs[vs].flags != 0 {
                                self.stop_osc(osc, vs);
                            }
                            self.unmap_voice(vs);
                        }
                        self.state = AL_STOPPED;
                    }
                }
                Ev::Stopping => {
                    if self.state == AL_PLAYING {
                        self.evtq.flush_type(Ev::SeqRef.kind());
                        self.evtq.flush_type(Ev::CspNoteOff(Midi::default()).kind());
                        self.evtq.flush_type(Ev::SeqpMidi(Midi::default()).kind());
                        for i in 0..self.alloc.len() {
                            let vs = self.alloc[i];
                            if self.voice_needs_note_kill(vs, KILL_TIME) {
                                self.release_voice(syn, vs, KILL_TIME);
                            }
                        }
                        for chan in 0..16 {
                            let c = &mut self.chan_state[chan];
                            c.fadevolcurrent = c.fadevoltarget;
                            if c.fadevolcurrent == 0 {
                                self.chan_mask &= (1u16 << chan) ^ 0xffff;
                            } else {
                                self.chan_mask |= 1 << chan;
                            }
                        }
                        self.state = AL_STOPPING;
                        self.evtq.post(Ev::Stop, AL_EVTQ_END, false);
                    }
                }
                Ev::Priority { chan, priority } => self.chan_state[chan as usize].priority = priority,
                Ev::Seq => {
                    self.target = self.seq.is_some();
                    self.chan_mask = 0xffff;
                    if self.has_bank {
                        self.init_from_bank(&bank);
                    }
                }
                Ev::Bank => {
                    self.has_bank = true;
                    self.init_from_bank(&bank);
                }
                Ev::Tempo { .. } | Ev::SeqEnd | Ev::SeqMidi(_) | Ev::TrackEnd | Ev::LoopStart | Ev::LoopEnd | Ev::None => {}
            }
            let (e, d) = self.evtq.next_event();
            self.next_event = e;
            self.next_delta = d;
            if self.next_delta != 0 {
                break;
            }
        }
        self.cur_time = self.cur_time.wrapping_add(self.next_delta);
        self.next_delta
    }

    /// `__n_CSPHandleNextSeqEvent` (`n_csplayer.c:392`).
    fn handle_next_seq_event(&mut self, syn: &mut Synth, osc: &mut OscPool, bank: &Arc<Bank>) {
        if !self.target {
            return;
        }
        let Some(seq) = self.seq.as_mut() else { return };
        match seq.next_event(true) {
            SeqEvent::Midi { ticks, status, byte1, byte2, duration } => {
                self.handle_midi(syn, osc, bank, Midi { ticks, status, byte1, byte2, duration }, None);
                self.post_next_seq_event();
            }
            SeqEvent::Tempo { status, ty, byte1, byte2, byte3, .. } => {
                self.handle_meta(status, ty, byte1, byte2, byte3);
                self.post_next_seq_event();
            }
            SeqEvent::SeqEnd { .. } => {
                // var8005f4dc is 0.
                self.state = AL_STOPPING;
                self.evtq.post(Ev::Stop, AL_EVTQ_END, false);
            }
            SeqEvent::TrackEnd { .. } | SeqEvent::LoopStart { .. } | SeqEvent::LoopEnd { .. } => self.post_next_seq_event(),
        }
    }

    /// `__n_CSPPostNextSeqEvent` (`n_csplayer.c:1216`).
    fn post_next_seq_event(&mut self) {
        if self.state != AL_PLAYING || !self.target {
            return;
        }
        let Some(seq) = self.seq.as_mut() else { return };
        let Some(ticks) = seq.next_delta() else { return };
        self.evtq.post(Ev::SeqRef, (ticks as i32).wrapping_mul(self.uspt), false);
    }

    /// `__n_CSPHandleMetaMsg` (`n_csplayer.c:1133`): a tempo change, and the
    /// pending note-offs re-timed to it.
    fn handle_meta(&mut self, status: u8, ty: u8, byte1: u8, byte2: u8, byte3: u8) {
        if status != cseq::AL_MIDI_META || ty != cseq::AL_MIDI_META_TEMPO {
            return;
        }
        let old_uspt = self.uspt;
        let tempo = ((byte1 as i32) << 16) | ((byte2 as i32) << 8) | byte3 as i32;
        self.set_uspt_from_tempo(tempo as f32);
        // Unlink the note-offs, each with its absolute time. PD links each
        // after the first, so the list reads first, last, ..., second.
        let mut temp: Vec<(i32, Ev)> = Vec::new();
        let mut cur_delta = 0i32;
        let mut i = 0;
        while i < self.evtq.items.len() {
            cur_delta = cur_delta.wrapping_add(self.evtq.items[i].0);
            if let Ev::CspNoteOff(_) = self.evtq.items[i].1 {
                let (d, e) = self.evtq.items.remove(i);
                let temp_delta = cur_delta;
                if i < self.evtq.items.len() {
                    cur_delta = cur_delta.wrapping_sub(d);
                    self.evtq.items[i].0 = self.evtq.items[i].0.wrapping_add(d);
                }
                if temp.is_empty() {
                    temp.push((temp_delta, e));
                } else {
                    temp.insert(1, (temp_delta, e));
                }
            } else {
                i += 1;
            }
        }
        for (d, e) in temp {
            let ticks = (d / old_uspt) as u32;
            let delta = (ticks as i32).wrapping_mul(self.uspt);
            self.repost(delta, e);
        }
    }

    /// `__n_CSPRepostEvent` (`n_csplayer.c:1177`): back into the queue, not
    /// through its free list.
    fn repost(&mut self, delta: i32, evt: Ev) {
        let mut delta = delta;
        let items = &mut self.evtq.items;
        for i in 0..=items.len() {
            if i == items.len() {
                items.push((delta, evt));
                return;
            }
            if delta < items[i].0 {
                items[i].0 -= delta;
                items.insert(i, (delta, evt));
                return;
            }
            delta -= items[i].0;
        }
    }

    /// `__n_setUsptFromTempo` (`n_csplayer.c:1207`).
    fn set_uspt_from_tempo(&mut self, tempo: f32) {
        self.uspt = match (&self.seq, self.target) {
            (Some(s), true) => (tempo * s.qnpt) as i32,
            _ => 488,
        };
    }

    /// `n_alCSPApplyChlVol` (`n_csplayer.c:441`).
    fn apply_chl_vol(&mut self, syn: &mut Synth, bank: &Bank, chan: u8) {
        for i in 0..self.alloc.len() {
            let vs = self.alloc[i];
            if self.vs[vs].channel == chan && self.vs[vs].env_phase != AL_PHASE_RELEASE {
                let v = self.vid(vs);
                let (vol, d) = (self.vs_vol(vs, bank), self.vs_delta(vs, self.cur_time));
                syn.set_vol(v, vol, d);
            }
        }
    }

    /// `func00034fb8` (`n_csplayer.c:455`): a channel's low-pass settings to its voices.
    fn apply_chl_filter(&mut self, syn: &mut Synth, bank: &Bank, chan: u8) {
        let c = self.chan_state[chan as usize];
        let sp29 = (c.unk12 as i8 as i32) - 64;
        for i in 0..self.alloc.len() {
            let vs = self.alloc[i];
            if self.vs[vs].channel == chan {
                let v = self.vid(vs);
                syn.filter12(v, c.unk11);
                if c.unk11 != 0 {
                    let s = &bank.sounds[self.vs[vs].sound.unwrap_or(0)];
                    let semis = self.vs[vs].key as i32 - s.key_map.key_base as i32 + sp29;
                    syn.filter13(v, al_semitones2ratio(semis) * 440.0 * c.pitch_bend);
                }
            }
        }
    }

    /// `__n_CSPHandleMIDIMsg` (`n_csplayer.c:480`). `repost` is the event to
    /// post again when a channel fade steps (the event being handled).
    fn handle_midi(&mut self, syn: &mut Synth, osc: &mut OscPool, bank: &Arc<Bank>, mut midi: Midi, repost: Option<Ev>) {
        let status = midi.status & 0xf0;
        let chan = midi.status & 0x0f;
        let key = midi.byte1;
        let vel = midi.byte2;
        let byte1 = midi.byte1;
        let mut byte2 = midi.byte2;
        let ch = chan as usize;

        match status {
            AL_MIDI_NOTE_ON | AL_MIDI_NOTE_OFF => {
                if status == AL_MIDI_NOTE_ON && vel != 0 {
                    self.note_on(syn, osc, bank, midi, chan, key, vel);
                    return;
                }
                // A note off (or a note on at velocity 0).
                let Some(vs) = self.lookup_voice(key, chan) else { return };
                let c = self.chan_state[ch];
                if self.vs[vs].phase == AL_PHASE_SUSTAIN {
                    self.vs[vs].phase = AL_PHASE_SUSTREL;
                } else {
                    self.vs[vs].phase = AL_PHASE_RELEASE;
                    let t = if c.usechanparams != 0 { c.release_time } else { bank.sounds[self.vs[vs].sound.unwrap_or(0)].envelope.release_time };
                    self.release_voice(syn, vs, t);
                }
            }
            AL_MIDI_POLY_KEY_PRESSURE => {
                let Some(vs) = self.lookup_voice(key, chan) else { return };
                self.vs[vs].velocity = byte2;
                let v = self.vid(vs);
                let (vol, d) = (self.vs_vol(vs, bank), self.vs_delta(vs, self.cur_time));
                syn.set_vol(v, vol, d);
            }
            AL_MIDI_CHANNEL_PRESSURE => {
                for i in 0..self.alloc.len() {
                    let vs = self.alloc[i];
                    if self.vs[vs].channel == chan {
                        self.vs[vs].velocity = byte1;
                        let v = self.vid(vs);
                        let (vol, d) = (self.vs_vol(vs, bank), self.vs_delta(vs, self.cur_time));
                        syn.set_vol(v, vol, d);
                    }
                }
            }
            AL_MIDI_CONTROL_CHANGE => {
                let mut ctrl = byte1;
                // FADESTART falls through into SETFADEINC; FXMIX7F into FXMIX80.
                if ctrl == AL_MIDI_FADESTART_CTRL {
                    let c = &mut self.chan_state[ch];
                    if c.fadevolinc == 0 {
                        c.fadevolinc = 0x90;
                    }
                    if byte2 == c.fadevoltarget {
                        return;
                    }
                    let fading = c.fadevoltarget != c.fadevolcurrent;
                    c.fadevoltarget = byte2;
                    if fading {
                        return;
                    }
                    midi.byte1 = AL_MIDI_SETFADEINC_CTRL;
                    ctrl = AL_MIDI_SETFADEINC_CTRL;
                }
                if ctrl == AL_MIDI_FXMIX7F_CTRL {
                    let c = &mut self.chan_state[ch];
                    c.fxmix = (c.fxmix & 0x80) | byte2;
                    byte2 = c.fxmix >> 7;
                    ctrl = AL_MIDI_FXMIX80_CTRL;
                }
                match ctrl {
                    AL_MIDI_PAN_CTRL => {
                        self.chan_state[ch].pan = byte2;
                        for i in 0..self.alloc.len() {
                            let vs = self.alloc[i];
                            if self.vs[vs].channel == chan {
                                let pan = self.vs_pan(vs, bank);
                                syn.set_pan(self.vid(vs), pan);
                            }
                        }
                    }
                    AL_MIDI_FADESPEED_CTRL => self.chan_state[ch].fadevolinc = byte2,
                    AL_MIDI_SETFADEINC_CTRL => {
                        let c = &mut self.chan_state[ch];
                        let mut cur = c.fadevolcurrent;
                        let target = c.fadevoltarget;
                        let mut inc = c.fadevolinc;
                        let mut step = target as i32 - cur as i32;
                        if step > 0 {
                            if inc & 0x80 != 0 {
                                inc = (inc & 0x7f) << 1;
                            }
                            if step > inc as i32 {
                                step = inc as i32;
                            }
                        } else {
                            inc &= 0x7f;
                            if step < -(inc as i32) {
                                step = -(inc as i32);
                            }
                        }
                        cur = (cur as i32 + step) as u8;
                        c.fadevolcurrent = cur;
                        if cur != target {
                            if let Some(ev) = repost {
                                let ev = match ev {
                                    Ev::SeqpMidi(_) => Ev::SeqpMidi(midi),
                                    Ev::CspNoteOff(_) => Ev::CspNoteOff(midi),
                                    other => other,
                                };
                                self.evtq.post(ev, self.uspt.wrapping_mul(100), false);
                            }
                        }
                        if cur != 0 {
                            self.chan_mask |= 1 << chan;
                        } else {
                            self.chan_mask &= !(1u16 << chan);
                        }
                        self.apply_chl_vol(syn, bank, chan);
                    }
                    AL_MIDI_FADEEND_CTRL => {
                        let c = &mut self.chan_state[ch];
                        c.fadevolcurrent = byte2;
                        c.fadevoltarget = byte2;
                        if byte2 == 0 {
                            self.chan_mask &= (1u16 << chan) ^ 0xffff;
                        } else {
                            self.chan_mask |= 1 << chan;
                        }
                        self.apply_chl_vol(syn, bank, chan);
                    }
                    AL_MIDI_UNK11_CTRL => {
                        self.chan_state[ch].unk11 = byte2;
                        self.apply_chl_filter(syn, bank, chan);
                    }
                    AL_MIDI_UNK12_CTRL => {
                        self.chan_state[ch].unk12 = byte2;
                        self.apply_chl_filter(syn, bank, chan);
                    }
                    AL_MIDI_UNK13_CTRL => {
                        self.chan_state[ch].unk13 = byte2;
                        for i in 0..self.alloc.len() {
                            let vs = self.alloc[i];
                            if self.vs[vs].channel == chan {
                                syn.filter11(self.vid(vs), byte2);
                            }
                        }
                    }
                    AL_MIDI_OSMESG_CTRL => {} // seqp->queue is never set.
                    AL_MIDI_VOLUME_CTRL => {
                        self.chan_state[ch].vol = byte2;
                        for i in 0..self.alloc.len() {
                            let vs = self.alloc[i];
                            if self.vs[vs].channel == chan && self.vs[vs].env_phase != AL_PHASE_RELEASE {
                                let v = self.vid(vs);
                                let (vol, d) = (self.vs_vol(vs, bank), self.vs_delta(vs, self.cur_time));
                                syn.set_vol(v, vol, d);
                            }
                        }
                    }
                    AL_MIDI_PRIORITY_CTRL => self.chan_state[ch].priority = byte2,
                    AL_MIDI_SUSTAIN_CTRL => {
                        self.chan_state[ch].sustain = byte2;
                        for i in 0..self.alloc.len() {
                            let vs = self.alloc[i];
                            if self.vs[vs].channel != chan || self.vs[vs].phase == AL_PHASE_RELEASE {
                                continue;
                            }
                            if byte2 > AL_SUSTAIN {
                                if self.vs[vs].phase == AL_PHASE_NOTEON {
                                    self.vs[vs].phase = AL_PHASE_SUSTAIN;
                                }
                            } else if self.vs[vs].phase == AL_PHASE_SUSTAIN {
                                self.vs[vs].phase = AL_PHASE_NOTEON;
                            } else if self.vs[vs].phase == AL_PHASE_SUSTREL {
                                self.vs[vs].phase = AL_PHASE_RELEASE;
                                // PD's `chanstate` here is uninitialised (@bug);
                                // this reads the channel's, and `vstate` is this voice.
                                let c = self.chan_state[ch];
                                let t = if c.usechanparams != 0 { c.release_time } else { bank.sounds[self.vs[vs].sound.unwrap_or(0)].envelope.release_time };
                                self.release_voice(syn, vs, t.max(AL_USEC_PER_FRAME));
                            }
                        }
                    }
                    AL_MIDI_FXMIX80_CTRL => {
                        let c = &mut self.chan_state[ch];
                        // `byte2 << 7` in an int, kept to the u8: only its low bit counts.
                        c.fxmix = ((c.fxmix & 0x7f) as u32 | ((byte2 as u32) << 7)) as u8;
                        let fxmix = c.fxmix;
                        for i in 0..self.alloc.len() {
                            let vs = self.alloc[i];
                            if self.vs[vs].channel == chan {
                                syn.set_fxmix(self.vid(vs), fxmix);
                            }
                        }
                    }
                    AL_MIDI_FXBUS_CTRL => {
                        if (byte2 as usize) < syn.max_aux_busses() {
                            self.chan_state[ch].fxbus = byte2;
                        }
                    }
                    // SUBST: `snd_start_mp3_by_filenum` / PD's MP3s aren't
                    // played by the synth; no sequence sends 0x1a.
                    AL_MIDI_MP3_CTRL => {}
                    AL_MIDI_INST_MAJOR_CTRL => self.chan_state[ch].instmajor = byte2,
                    AL_MIDI_ATTACKTIME_CTRL => self.set_chan_param(ch, |c| c.attack_time = CSP_TIME_LOOKUP[byte2 as usize & 0x7f]),
                    AL_MIDI_ATTACKVOL_CTRL => self.set_chan_param(ch, |c| c.attack_volume = byte2),
                    AL_MIDI_DECAYTIME_CTRL => self.set_chan_param(ch, |c| c.decay_time = CSP_TIME_LOOKUP[byte2 as usize & 0x7f]),
                    AL_MIDI_DECAYVOL_CTRL => self.set_chan_param(ch, |c| c.decay_volume = byte2),
                    AL_MIDI_RELEASETIME_CTRL => self.set_chan_param(ch, |c| c.release_time = CSP_TIME_LOOKUP[byte2 as usize & 0x7f]),
                    AL_MIDI_PITCH_CTRL => self.set_chan_param(ch, |c| c.pitch = (byte2 as i32 - 64) as i8),
                    AL_MIDI_BENDRANGE_MINOR_CTRL => {
                        let c = &mut self.chan_state[ch];
                        c.bend_range = (c.bend_range / 100) * 100 + byte2 as i16;
                    }
                    AL_MIDI_BENDRANGE_MAJOR_CTRL => {
                        let c = &mut self.chan_state[ch];
                        c.bend_range = (c.bend_range % 100).wrapping_add((byte2 as i16).wrapping_mul(100));
                    }
                    AL_MIDI_VIBTYPE_CTRL => {
                        let t = if byte2 != 0 { byte2.wrapping_add(0x80) } else { 0 };
                        self.set_chan_param(ch, |c| c.vib_type = t);
                    }
                    AL_MIDI_VIBRATE_CTRL => self.set_chan_param(ch, |c| c.vib_rate = byte2),
                    AL_MIDI_VIBDEPTH_CTRL => self.set_chan_param(ch, |c| c.vib_depth = byte2.wrapping_mul(2)),
                    AL_MIDI_VIBDELAY_CTRL => self.set_chan_param(ch, |c| c.vib_delay = byte2),
                    AL_MIDI_TREMTYPE_CTRL => self.set_chan_param(ch, |c| c.trem_type = byte2),
                    AL_MIDI_TREMRATE_CTRL => self.set_chan_param(ch, |c| c.trem_rate = byte2),
                    AL_MIDI_TREMDEPTH_CTRL => self.set_chan_param(ch, |c| c.trem_depth = byte2),
                    AL_MIDI_TREMDELAY_CTRL => self.set_chan_param(ch, |c| c.trem_delay = byte2),
                    AL_MIDI_OSC_CTRL => {
                        let depth = byte2.wrapping_mul(2);
                        for i in 0..self.alloc.len() {
                            let vs = self.alloc[i];
                            let Some(o) = self.vs[vs].osc_state2 else { continue };
                            if self.vs[vs].channel != chan {
                                continue;
                            }
                            let s = &mut osc.states[o];
                            match s.osc_type & 0x7f {
                                0x02 => {
                                    s.unk10 = -depth2cents(depth);
                                    s.unk0c = depth2cents(depth);
                                }
                                0x03..=0x05 => s.unk0c = depth2cents(depth),
                                0x07 | 0x09 | 0x0d => s.unk0c = depth2cents(depth) / 2.0,
                                0x0a => s.unk0c = depth2cents(depth) * 2.0,
                                _ => s.unk0c = depth2cents(depth),
                            }
                        }
                    }
                    AL_MIDI_TIMEINDEX_CTRL => self.chan_state[ch].timeindex = byte2,
                    _ => {}
                }
            }
            AL_MIDI_PROGRAM_CHANGE => {
                let n = ((self.chan_state[ch].instmajor as usize) << 7) + key as usize;
                if n < bank.instruments.len() {
                    if let Some(inst) = bank.instruments[n].as_ref() {
                        self.set_inst_chan_state(bank, n, inst, ch);
                    }
                }
            }
            AL_MIDI_PITCH_BEND_CHANGE => {
                let bend_val = ((byte2 as i32) << 7) + byte1 as i32 - 8192;
                let cents = self.chan_state[ch].bend_range as i32 * bend_val / 8192;
                let ratio = al_cents2ratio(cents);
                self.chan_state[ch].pitch_bend = ratio;
                let c = self.chan_state[ch];
                for i in 0..self.alloc.len() {
                    let vs = self.alloc[i];
                    if self.vs[vs].channel == chan {
                        let v = self.vid(vs);
                        syn.set_pitch(v, self.vs[vs].pitch * ratio * self.vs[vs].vibrato);
                        if c.unk11 != 0 {
                            let s = &bank.sounds[self.vs[vs].sound.unwrap_or(0)];
                            let semis = self.vs[vs].key as i32 - s.key_map.key_base as i32 + c.unk12 as i32 - 64;
                            syn.filter13(v, 440.0 * al_semitones2ratio(semis) * ratio * self.vs[vs].vibrato);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn set_chan_param(&mut self, ch: usize, f: impl FnOnce(&mut ChanState)) {
        f(&mut self.chan_state[ch]);
        self.chan_state[ch].usechanparams = 1;
    }

    /// The note-on half of `__n_CSPHandleMIDIMsg` (`n_csplayer.c:533`).
    #[allow(clippy::too_many_arguments)]
    fn note_on(&mut self, syn: &mut Synth, osc: &mut OscPool, bank: &Arc<Bank>, midi: Midi, chan: u8, key: u8, vel: u8) {
        let ch = chan as usize;
        if self.state != AL_PLAYING || self.chan_mask & (1 << chan) == 0 {
            if midi.duration != 0 {
                let evt = Ev::CspNoteOff(Midi { ticks: midi.ticks, status: chan | AL_MIDI_NOTE_OFF, byte1: key, byte2: 0, duration: 0 });
                self.evtq.post(evt, self.uspt.wrapping_mul(midi.duration as i32), false);
            }
            return;
        }
        let Some(sound_ix) = self.lookup_sound_quick(bank, key, vel, chan) else {
            self.notes[1] += 1;
            return;
        };
        let c = self.chan_state[ch];
        let Some(vs) = self.map_voice(key, vel, chan) else {
            self.notes[2] += 1;
            return;
        };
        self.notes[0] += 1;
        let v = self.vid(vs);
        syn.alloc_voice(v, c.priority as i16, c.fxbus, false);
        let sound = bank.sounds[sound_ix];
        let st = &mut self.vs[vs];
        st.sound = Some(sound_ix);
        st.env_phase = AL_PHASE_ATTACK;
        st.phase = if c.sustain > AL_SUSTAIN { AL_PHASE_SUSTAIN } else { AL_PHASE_NOTEON };
        let mut cents = (key as i32 - sound.key_map.key_base as i32) * 100 + sound.key_map.detune as i32;
        if c.usechanparams != 0 {
            cents += c.pitch as i32;
        }
        st.pitch = al_cents2ratio(cents);
        if c.usechanparams != 0 {
            st.env_gain = c.attack_volume;
            st.env_end_time = self.cur_time.wrapping_add(c.attack_time);
        } else {
            st.env_gain = sound.envelope.attack_volume;
            st.env_end_time = self.cur_time.wrapping_add(sound.envelope.attack_time);
        }
        st.flags = 0;

        // The tremolo and vibrato: the channel's parameters, or its instrument's.
        let inst = c.instrument.and_then(|i| bank.instruments.get(i)).and_then(|x| x.as_ref());
        let o8 = |f: fn(&Instrument) -> u8| inst.map_or(0, f);
        let (trem, vib) = if c.usechanparams != 0 {
            ([c.trem_type, c.trem_rate, c.trem_depth, c.trem_delay], [c.vib_type, c.vib_rate, c.vib_depth, c.vib_delay])
        } else {
            (
                [o8(|i| i.trem_type), o8(|i| i.trem_rate), o8(|i| i.trem_depth), o8(|i| i.trem_delay)],
                [o8(|i| i.vib_type), o8(|i| i.vib_rate), o8(|i| i.vib_depth), o8(|i| i.vib_delay)],
            )
        };
        let mut osc_state: Option<usize> = None;
        let mut osc_value: f32 = AL_VOL_FULL_MIDI as f32;
        if trem[0] != 0 {
            let dt = osc.init(&mut osc_state, &mut osc_value, trem[0], trem[1], trem[2], trem[3], c.timeindex);
            if dt != 0 {
                if let Some(o) = osc_state {
                    self.evtq.post(Ev::TremOsc { vs, osc: o }, dt, false);
                    self.vs[vs].flags |= 0x01;
                    self.vs[vs].osc_state = Some(o);
                }
            }
        }
        self.vs[vs].tremelo = osc_value as u8;
        osc_value = 1.0;
        if vib[0] != 0 {
            let dt = osc.init(&mut osc_state, &mut osc_value, vib[0], vib[1], vib[2], vib[3], c.timeindex);
            if dt != 0 {
                if let Some(o) = osc_state {
                    self.evtq.post(Ev::VibOsc { vs, osc: o, chan }, dt, false);
                    self.vs[vs].flags |= 0x02;
                    self.vs[vs].osc_state2 = Some(o);
                }
            }
        }
        self.vs[vs].vibrato = osc_value;

        let pitch = self.vs[vs].pitch * c.pitch_bend * self.vs[vs].vibrato;
        let fxmix = self.vs_mix(vs);
        let sp76 = c.unk11;
        let sp70 = if sp76 != 0 { 440.0 * al_semitones2ratio(cents / 100 + c.unk12 as i32 - 64) * c.pitch_bend } else { 127.0 };
        let pan = self.vs_pan(vs, bank);
        let vol = self.vs_vol(vs, bank);
        let attack = if c.usechanparams != 0 { c.attack_time } else { sound.envelope.attack_time };
        syn.start_voice_params(v, sound.wavetable, pitch, vol, pan, fxmix, sp76, sp70, c.unk13, attack);
        let (dvol, ddelta) = if c.usechanparams != 0 { (c.decay_volume, c.decay_time) } else { (sound.envelope.decay_volume, sound.envelope.decay_time) };
        self.evtq.post(Ev::Env { vs, vol: dvol, delta: ddelta }, attack, false);
        if midi.duration != 0 {
            let evt = Ev::CspNoteOff(Midi { ticks: midi.ticks, status: chan | AL_MIDI_NOTE_OFF, byte1: key, byte2: 0, duration: 0 });
            self.evtq.post(evt, self.uspt.wrapping_mul(midi.duration as i32), false);
        }
    }

    // --- n_seqplayer.c ----------------------------------------------------

    /// `__n_unmapVoice` (`n_seqplayer.c:9`).
    fn unmap_voice(&mut self, vs: usize) {
        if let Some(i) = self.alloc.iter().position(|&x| x == vs) {
            self.alloc.remove(i);
            self.vfree.push(vs);
            self.voicecount = self.voicecount.wrapping_sub(1);
        }
    }

    /// `__n_seqpReleaseVoice` (`n_seqplayer.c:42`).
    fn release_voice(&mut self, syn: &mut Synth, vs: usize, delta_time: i32) {
        if self.vs[vs].env_phase == AL_PHASE_ATTACK {
            let mut i = 0;
            while i < self.evtq.items.len() {
                if matches!(self.evtq.items[i].1, Ev::Env { vs: x, .. } if x == vs) {
                    self.evtq.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        let st = &mut self.vs[vs];
        st.velocity = 0;
        st.env_phase = AL_PHASE_RELEASE;
        st.env_gain = 0;
        st.env_end_time = self.cur_time.wrapping_add(delta_time);
        let v = st.voice;
        syn.set_priority(v, 0);
        syn.set_vol(v, 0, delta_time);
        self.evtq.post(Ev::NoteEnd(vs), delta_time.wrapping_add(AL_USEC_PER_FRAME * 2), false);
    }

    /// `__n_voiceNeedsNoteKill` (`n_seqplayer.c:94`).
    fn voice_needs_note_kill(&mut self, vs: usize, kill_time: i32) -> bool {
        let mut item_time = 0i32;
        for i in 0..self.evtq.items.len() {
            item_time = item_time.wrapping_add(self.evtq.items[i].0);
            if self.evtq.items[i].1 == Ev::NoteEnd(vs) {
                if item_time > kill_time {
                    self.evtq.remove(i);
                    return true;
                }
                return false;
            }
        }
        true
    }

    /// `__n_mapVoice` (`n_seqplayer.c:130`).
    fn map_voice(&mut self, key: u8, vel: u8, channel: u8) -> Option<usize> {
        if self.voicecount > self.voicelimit {
            return None;
        }
        let vs = self.vfree.pop()?;
        self.alloc.push(vs);
        let st = &mut self.vs[vs];
        st.channel = channel;
        st.key = key;
        st.velocity = vel;
        self.voicecount += 1;
        Some(vs)
    }

    /// `__n_lookupVoice` (`n_seqplayer.c:163`).
    fn lookup_voice(&self, key: u8, channel: u8) -> Option<usize> {
        self.alloc.iter().copied().find(|&vs| {
            let s = &self.vs[vs];
            s.key == key && s.channel == channel && s.phase != AL_PHASE_RELEASE && s.phase != AL_PHASE_SUSTREL
        })
    }

    /// `__n_lookupSoundQuick` (`n_seqplayer.c:182`): a binary search of the
    /// channel's instrument's sounds by key and velocity.
    fn lookup_sound_quick(&self, bank: &Bank, key: u8, vel: u8, chan: u8) -> Option<usize> {
        let inst = bank.instruments.get(self.chan_state[chan as usize].instrument?)?.as_ref()?;
        let (mut l, mut r) = (1i32, inst.sounds.len() as i32);
        while r >= l {
            let i = (l + r) / 2;
            let s = inst.sounds[(i - 1) as usize];
            let km = bank.sounds[s].key_map;
            if key >= km.key_min && key <= km.key_max && vel >= km.velocity_min && vel <= km.velocity_max {
                return Some(s);
            } else if key < km.key_min || (vel < km.velocity_min && key <= km.key_max) {
                r = i - 1;
            } else {
                l = i + 1;
            }
        }
        None
    }

    /// `__n_vsVol` (`n_seqplayer.c:211`).
    fn vs_vol(&self, vs: usize, bank: &Bank) -> i16 {
        let s = &self.vs[vs];
        let c = &self.chan_state[s.channel as usize];
        let sample_volume = s.sound.map_or(0, |i| bank.sounds[i].sample_volume) as u32;
        let mut t1 = (s.tremelo as u32 * s.velocity as u32 * s.env_gain as u32) >> 6;
        let mut t2 = (sample_volume * self.vol as i32 as u32 * c.vol as u32) >> 14;
        if c.fadevolcurrent != 0xff {
            t2 = (c.fadevolcurrent as u32 * t2 + 1) >> 8;
        }
        t1 = t1.wrapping_mul(t2);
        t1 >>= 15;
        t1 as i16
    }

    /// `__n_vsMix` (`n_seqplayer.c:226`).
    fn vs_mix(&self, vs: usize) -> u8 {
        let c = &self.chan_state[self.vs[vs].channel as usize];
        let sign = c.fxmix & 0x80;
        let fxmix = (((c.fxmix & 0x7f) as i32 + (self.fxmixmajor * 127.0) as i32) as f32 * self.fxmixmega) as i32;
        fxmix.clamp(0, 127) as u8 | sign
    }

    /// `__n_vsDelta` (`n_seqplayer.c:234`).
    fn vs_delta(&self, vs: usize, t: i32) -> i32 {
        let delta = self.vs[vs].env_end_time.wrapping_sub(t);
        if delta >= 0 {
            delta
        } else {
            AL_GAIN_CHANGE_TIME
        }
    }

    /// `__n_vsPan` (`n_seqplayer.c:252`).
    fn vs_pan(&self, vs: usize, bank: &Bank) -> u8 {
        let s = &self.vs[vs];
        let pan = s.sound.map_or(0, |i| bank.sounds[i].sample_pan) as i32;
        let tmp = self.chan_state[s.channel as usize].pan as i32 - AL_PAN_CENTER + pan;
        tmp.clamp(0, 127) as u8
    }

    /// `__n_initFromBank` (`n_seqplayer.c:263`). PD's bank has no
    /// percussion (whose setup writes one channel past the end).
    fn init_from_bank(&mut self, bank: &Bank) {
        let Some((n, inst)) = bank.instruments.iter().enumerate().find_map(|(i, x)| x.as_ref().map(|x| (i, x))) else { return };
        for i in 0..self.max_channels {
            self.reset_perf_chan_state(i);
            self.set_inst_chan_state(bank, n, inst, i);
        }
    }

    /// `__n_initChanState` (`n_seqplayer.c:289`).
    fn init_chan_state(&mut self) {
        for i in 0..self.max_channels {
            self.chan_state[i].instrument = None;
            self.reset_perf_chan_state(i);
        }
    }

    /// `__n_resetPerfChanState` (`n_seqplayer.c:299`).
    fn reset_perf_chan_state(&mut self, chan: usize) {
        let c = &mut self.chan_state[chan];
        c.fx_id = 0;
        c.fxmix = AL_DEFAULT_FXMIX;
        c.pan = AL_PAN_CENTER as u8;
        c.vol = AL_VOL_FULL_MIDI;
        c.priority = AL_DEFAULT_PRIORITY;
        c.sustain = 0;
        c.bend_range = 200;
        c.pitch_bend = 1.0;
        c.notemesgflags = 0;
        c.fadevolcurrent = 255;
        c.fadevoltarget = 255;
        c.fadevolinc = 0;
        c.fxbus = 0;
        c.unk13 = 0;
        c.unk12 = 0;
        c.unk11 = 0;
        c.instmajor = 0;
    }

    /// `__n_setInstChanState` (`n_seqplayer.c:320`).
    fn set_inst_chan_state(&mut self, bank: &Bank, n: usize, inst: &Instrument, chan: usize) {
        let c = &mut self.chan_state[chan];
        c.instrument = Some(n);
        c.pan = inst.pan;
        c.vol = inst.volume;
        c.priority = inst.priority;
        c.bend_range = inst.bend_range;
        let Some(&s) = inst.sounds.first() else { return };
        let env = bank.sounds[s].envelope;
        c.attack_time = env.attack_time;
        c.decay_time = env.decay_time;
        c.release_time = env.release_time;
        c.attack_volume = env.attack_volume;
        c.decay_volume = env.decay_volume;
        c.pitch = 0;
        c.trem_type = inst.trem_type;
        c.trem_rate = inst.trem_rate;
        c.trem_depth = inst.trem_depth;
        c.trem_delay = inst.trem_delay;
        c.vib_type = inst.vib_type;
        c.vib_rate = inst.vib_rate;
        c.vib_depth = inst.vib_depth;
        c.vib_delay = inst.vib_delay;
        c.usechanparams = 0;
        c.timeindex = 0;
    }

    /// `__n_seqpStopOsc` (`n_seqplayer.c:352`).
    fn stop_osc(&mut self, osc: &mut OscPool, vs: usize) {
        let mut i = 0;
        while i < self.evtq.items.len() {
            let o = match self.evtq.items[i].1 {
                Ev::TremOsc { vs: x, osc } if x == vs => Some((osc, 0xfe)),
                Ev::VibOsc { vs: x, osc, .. } if x == vs => Some((osc, 0xfd)),
                _ => None,
            };
            if let Some((o, mask)) = o {
                osc.stop(o);
                self.evtq.remove(i);
                self.vs[vs].flags &= mask;
                if self.vs[vs].flags == 0 {
                    return;
                }
            } else {
                i += 1;
            }
        }
    }
}
