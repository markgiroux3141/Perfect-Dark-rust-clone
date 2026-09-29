//! An instrument bank as `alBnkfNew` leaves it (`libaudio.h:192-265`): the
//! bank's instruments, their sounds and the wavetables the sounds play, plus
//! the sample data the wavetables' `base` offsets point into. Pointers become
//! indexes; the game's loader fills these from `assets/music/bank.json`.

/// `ALEnvelope` (`libaudio.h:192`): times in microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Envelope {
    pub attack_time: i32,
    pub decay_time: i32,
    pub release_time: i32,
    pub attack_volume: u8,
    pub decay_volume: u8,
}

/// `ALKeyMap` (`libaudio.h:200`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyMap {
    pub velocity_min: u8,
    pub velocity_max: u8,
    pub key_min: u8,
    pub key_max: u8,
    pub key_base: u8,
    pub detune: i8,
}

/// `ALADPCMloop` (`libaudio.h:179`): in samples, with the decoder's state at
/// `start`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AdpcmLoop {
    pub start: u32,
    pub end: u32,
    pub count: i32,
    pub state: [i16; 16],
}

/// `ALWaveTable` (`libaudio.h:217`), ADPCM only: every wavetable in PD's
/// music bank is (the `n_` loader has no raw16 path, `n_load.c`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaveTable {
    /// Offset into the sample data.
    pub base: u32,
    /// Bytes.
    pub len: i32,
    /// `ALADPCMBook`: `order`, `npredictors`, then `order * npredictors * 8`
    /// coefficients.
    pub order: i32,
    pub npredictors: i32,
    pub book: Vec<i16>,
    pub adpcm_loop: Option<AdpcmLoop>,
}

/// `ALSound` (`libaudio.h:229`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sound {
    pub envelope: Envelope,
    pub key_map: KeyMap,
    /// Into [`Bank::wavetables`].
    pub wavetable: usize,
    pub sample_pan: u8,
    pub sample_volume: u8,
    pub flags: u8,
}

/// `ALInstrument` (`libaudio.h:238`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Instrument {
    pub volume: u8,
    pub pan: u8,
    pub priority: u8,
    pub flags: u8,
    pub trem_type: u8,
    pub trem_rate: u8,
    pub trem_depth: u8,
    pub trem_delay: u8,
    pub vib_type: u8,
    pub vib_rate: u8,
    pub vib_depth: u8,
    pub vib_delay: u8,
    pub bend_range: i16,
    /// Into [`Bank::sounds`], in `soundArray` order.
    pub sounds: Vec<usize>,
}

/// `ALBank` (`libaudio.h:256`) with everything it points at.
#[derive(Clone, Debug, Default)]
pub struct Bank {
    /// `instArray`, `instCount` long (an entry may be empty).
    pub instruments: Vec<Option<Instrument>>,
    pub percussion: Option<Instrument>,
    pub sounds: Vec<Sound>,
    pub wavetables: Vec<WaveTable>,
    /// The `.tbl` the wavetables' bases point into.
    pub tbl: Vec<u8>,
}

impl Bank {
    /// `n_alLoadParam(AL_FILTER_SET_WAVETABLE)`'s truncation of an ADPCM
    /// wavetable to whole 9-byte frames (`n_load.c:203`). The loader applies it
    /// once; PD applies it at every note (the same value).
    pub fn frame_len(len: i32) -> i32 {
        9 * (len / 9)
    }
}
