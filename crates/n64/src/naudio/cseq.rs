//! The compact MIDI sequence reader (`n_csq.c`, `cseq.c`): an `ALCMidiHdr`
//! (16 big-endian track offsets and the division) and up to 16 tracks of
//! compressed MIDI, each read with running status, "backup" blocks (a
//! repeated run of earlier bytes, `AL_CMIDI_BLOCK_CODE`) and loop metas.
//! Note-ons carry their duration; there are no note-offs.
//!
//! The reader owns its copy of the data: a loop's end writes its remaining
//! count back into the sequence (`n_csq.c:117`), as PD does into its buffer.

/// `AL_CMIDI_*` (`libaudio.h:544`).
pub const AL_CMIDI_BLOCK_CODE: u8 = 0xfe;
pub const AL_CMIDI_LOOPSTART_CODE: u8 = 0x2e;
pub const AL_CMIDI_LOOPEND_CODE: u8 = 0x2d;
pub const AL_MIDI_META: u8 = 0xff;
pub const AL_MIDI_META_TEMPO: u8 = 0x51;
pub const AL_MIDI_META_EOT: u8 = 0x2f;

/// What `n_alCSeqNextEvent` returns (`N_ALEvent`'s sequence types).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SeqEvent {
    /// `AL_SEQ_MIDI_EVT`: status (with the track as its channel), bytes, and a
    /// note-on's duration in ticks.
    Midi { ticks: u32, status: u8, byte1: u8, byte2: u8, duration: u32 },
    /// `AL_TEMPO_EVT`.
    Tempo { ticks: u32, status: u8, ty: u8, byte1: u8, byte2: u8, byte3: u8 },
    /// `AL_TRACK_END`: a track ended, others play on.
    TrackEnd { ticks: u32 },
    /// `AL_SEQ_END_EVT`: the last track ended.
    SeqEnd { ticks: u32 },
    /// `AL_CSP_LOOPSTART` with its count.
    LoopStart { ticks: u32, count: u16 },
    /// `AL_CSP_LOOPEND`.
    LoopEnd { ticks: u32 },
}

/// `ALCSeq` (`libaudio.h`).
#[derive(Clone, Debug)]
pub struct CSeq {
    data: Vec<u8>,
    pub valid_tracks: u32,
    pub last_delta_ticks: u32,
    pub last_ticks: u32,
    pub delta_flag: bool,
    cur_loc: [usize; 16],
    cur_bu_ptr: [usize; 16],
    cur_bu_len: [u32; 16],
    last_status: [u8; 16],
    evt_delta_ticks: [u32; 16],
    /// Quarter notes per tick: 1 / division.
    pub qnpt: f32,
}

impl CSeq {
    /// `n_alCSeqNew` (`n_csq.c:11`).
    pub fn new(data: &[u8]) -> CSeq {
        let mut s = CSeq {
            data: data.to_vec(),
            valid_tracks: 0,
            last_delta_ticks: 0,
            last_ticks: 0,
            delta_flag: true,
            cur_loc: [0; 16],
            cur_bu_ptr: [0; 16],
            cur_bu_len: [0; 16],
            last_status: [0; 16],
            evt_delta_ticks: [0; 16],
            qnpt: 0.0,
        };
        for i in 0..16 {
            let off = s.be32(4 * i) as usize;
            if off != 0 {
                s.valid_tracks |= 1 << i;
                s.cur_loc[i] = off;
                s.evt_delta_ticks[i] = s.read_var_len(i);
            }
        }
        s.qnpt = 1.0 / s.be32(64) as f32;
        s
    }

    pub fn division(&self) -> u32 {
        self.be32(64)
    }

    fn be32(&self, at: usize) -> u32 {
        u32::from_be_bytes([self.byte_at(at), self.byte_at(at + 1), self.byte_at(at + 2), self.byte_at(at + 3)])
    }

    /// A byte of the buffer. SUBST: a sequence's end reads one delta past its
    /// last event (`n_csq.c:66`), which in PD is whatever follows in the
    /// buffer / 0 (the exporter's cross-check reads the same).
    fn byte_at(&self, at: usize) -> u8 {
        self.data.get(at).copied().unwrap_or(0)
    }

    /// `__getTrackByte` (`n_csq.c:278`).
    fn get_track_byte(&mut self, track: usize) -> u8 {
        if self.cur_bu_len[track] != 0 {
            let b = self.byte_at(self.cur_bu_ptr[track]);
            self.cur_bu_ptr[track] += 1;
            self.cur_bu_len[track] -= 1;
            return b;
        }
        let mut b = self.byte_at(self.cur_loc[track]);
        self.cur_loc[track] += 1;
        if b == AL_CMIDI_BLOCK_CODE {
            let next = self.byte_at(self.cur_loc[track]);
            self.cur_loc[track] += 1;
            if next != AL_CMIDI_BLOCK_CODE {
                let hi = next as usize;
                let lo = self.byte_at(self.cur_loc[track]) as usize;
                self.cur_loc[track] += 1;
                let len = self.byte_at(self.cur_loc[track]);
                self.cur_loc[track] += 1;
                let backup = (hi << 8) + lo;
                self.cur_bu_ptr[track] = self.cur_loc[track].wrapping_sub(backup + 4);
                self.cur_bu_len[track] = len as u32;
                b = self.byte_at(self.cur_bu_ptr[track]);
                self.cur_bu_ptr[track] += 1;
                self.cur_bu_len[track] = self.cur_bu_len[track].wrapping_sub(1);
            }
        }
        b
    }

    /// `__readVarLen` (`n_csq.c:318`).
    fn read_var_len(&mut self, track: usize) -> u32 {
        let mut value = self.get_track_byte(track) as u32;
        if value & 0x80 != 0 {
            value &= 0x7f;
            loop {
                let c = self.get_track_byte(track) as u32;
                value = (value << 7).wrapping_add(c & 0x7f);
                if c & 0x80 == 0 {
                    break;
                }
            }
        }
        value
    }

    /// `n_alCSeqNextEvent` (`n_csq.c:50`): the next event of whichever track
    /// is due first. `take_loops` is its third argument: 0 never jumps back
    /// at a loop end (the marker walks), the player passes 1.
    pub fn next_event(&mut self, take_loops: bool) -> SeqEvent {
        let mut first_time = u32::MAX;
        let mut first_track = 0;
        let last_ticks = self.last_delta_ticks;
        for i in 0..16 {
            if (self.valid_tracks >> i) & 1 != 0 {
                if self.delta_flag {
                    self.evt_delta_ticks[i] = self.evt_delta_ticks[i].wrapping_sub(last_ticks);
                }
                if self.evt_delta_ticks[i] < first_time {
                    first_time = self.evt_delta_ticks[i];
                    first_track = i;
                }
            }
        }
        let evt = self.get_track_event(first_track, take_loops, first_time);
        self.last_ticks = self.last_ticks.wrapping_add(first_time);
        self.last_delta_ticks = first_time;
        if !matches!(evt, SeqEvent::TrackEnd { .. }) {
            let d = self.read_var_len(first_track);
            self.evt_delta_ticks[first_track] = self.evt_delta_ticks[first_track].wrapping_add(d);
        }
        self.delta_flag = true;
        evt
    }

    /// `__n_alCSeqGetTrackEvent` (`n_csq.c:73`).
    fn get_track_event(&mut self, track: usize, take_loops: bool, ticks: u32) -> SeqEvent {
        let status = self.get_track_byte(track);
        if status == AL_MIDI_META {
            let ty = self.get_track_byte(track);
            if ty == AL_MIDI_META_TEMPO {
                let byte1 = self.get_track_byte(track);
                let byte2 = self.get_track_byte(track);
                let byte3 = self.get_track_byte(track);
                self.last_status[track] = 0;
                return SeqEvent::Tempo { ticks, status, ty, byte1, byte2, byte3 };
            }
            if ty == AL_MIDI_META_EOT {
                self.valid_tracks ^= 1 << track;
                return if self.valid_tracks != 0 { SeqEvent::TrackEnd { ticks } } else { SeqEvent::SeqEnd { ticks } };
            }
            if ty == AL_CMIDI_LOOPSTART_CODE {
                let hi = self.get_track_byte(track) as u16;
                let lo = self.get_track_byte(track) as u16;
                self.last_status[track] = 0;
                return SeqEvent::LoopStart { ticks, count: (hi << 8).wrapping_add(lo) };
            }
            if ty == AL_CMIDI_LOOPEND_CODE {
                let p = self.cur_loc[track];
                let loop_ct = self.byte_at(p);
                let cur_lp_ct = self.byte_at(p + 1);
                if cur_lp_ct == 0 || !take_loops {
                    self.set_byte(p + 1, loop_ct);
                    self.cur_loc[track] = p + 1 + 5;
                } else {
                    if cur_lp_ct != 0xff {
                        self.set_byte(p + 1, cur_lp_ct - 1);
                    }
                    let offset = u32::from_be_bytes([self.byte_at(p + 2), self.byte_at(p + 3), self.byte_at(p + 4), self.byte_at(p + 5)]) as usize;
                    self.cur_loc[track] = (p + 6).wrapping_sub(offset);
                }
                self.last_status[track] = 0;
                return SeqEvent::LoopEnd { ticks };
            }
            // Any other meta: PD leaves the event's type as it was (the
            // caller's stack); no sequence has one.
            return SeqEvent::TrackEnd { ticks };
        }
        let (st, byte1) = if status & 0x80 != 0 {
            let st = (status & 0xf0) | track as u8;
            let b1 = self.get_track_byte(track);
            self.last_status[track] = st;
            (st, b1)
        } else {
            (self.last_status[track], status)
        };
        let (mut byte2, mut duration) = (0, 0);
        if st & 0xf0 != 0xc0 && st & 0xf0 != 0xd0 {
            byte2 = self.get_track_byte(track);
            if st & 0xf0 == 0x90 {
                duration = self.read_var_len(track);
            }
        }
        SeqEvent::Midi { ticks, status: st, byte1, byte2, duration }
    }

    fn set_byte(&mut self, at: usize, v: u8) {
        if let Some(b) = self.data.get_mut(at) {
            *b = v;
        }
    }

    /// `__alCSeqNextDelta` (`cseq.c:10`): ticks to the next event, or `None`
    /// at the end of the sequence.
    pub fn next_delta(&mut self) -> Option<u32> {
        if self.valid_tracks == 0 {
            return None;
        }
        let mut first_time = u32::MAX;
        let last_ticks = self.last_delta_ticks;
        for i in 0..16 {
            if (self.valid_tracks >> i) & 1 != 0 {
                if self.delta_flag {
                    self.evt_delta_ticks[i] = self.evt_delta_ticks[i].wrapping_sub(last_ticks);
                }
                if self.evt_delta_ticks[i] < first_time {
                    first_time = self.evt_delta_ticks[i];
                }
            }
        }
        self.delta_flag = false;
        Some(first_time)
    }
}
