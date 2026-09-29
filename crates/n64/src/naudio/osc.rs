//! `osc.c`: the tremolo and vibrato oscillators the sequence players run for
//! an instrument (or a channel's parameters) with a nonzero osc type.
//! `osc_build_linkedlist(0, 60)` gives the three players one pool of 60
//! states (`audiomgr.c:233`).
//!
//! PD's bank uses only type 1 (tremolo) and 128 (vibrato); the other types
//! come from `osc_init_other`, reachable only through the oscillator
//! controllers no Combat Simulator sequence sends, and are ported with PD's
//! `osc_stop` bug (a state of another type is pushed on the free list twice).

use super::{al_cents2ratio, AL_USEC_PER_FRAME};

pub const OSCTYPE_TREM: u8 = 1;
pub const OSCTYPE_VIB: u8 = 128;

/// `g_CspTimeLookup` (`n_csplayer.c:18`): a controller byte to microseconds.
pub const CSP_TIME_LOOKUP: [i32; 128] = [
    0, 10000, 20000, 30000, 40000, 50000, 60000, 70000, 80000, 90000, 100000, 110000, 110000, 120000, 130000, 140000, 150000, 160000, 170000, 190000, 200000, 220000, 230000, 250000, 270000, 290000, 310000, 330000, 350000, 380000, 410000, 440000, 470000, 500000, 540000, 580000, 620000, 660000, 710000, 760000, 820000, 880000, 940000, 1000000, 1000000, 1100000, 1200000, 1300000, 1400000, 1500000, 1600000, 1700000, 1800000, 2000000, 2100000, 2300000, 2400000, 2600000, 2800000, 3000000, 3200000, 3500000, 3700000, 4000000, 4300000, 4600000, 4900000, 5300000, 5700000, 6100000, 6500000, 7000000, 7500000, 8100000, 8600000, 9300000, 9900000, 10000000, 11000000, 12000000, 13000000, 14000000, 15000000, 16000000, 17000000, 18000000, 19000000, 21000000, 22000000, 24000000, 26000000, 28000000, 30000000, 32000000, 34000000, 37000000, 39000000, 42000000, 45000000, 49000000, 50000000, 55000000, 60000000, 65000000, 70000000, 75000000, 80000000, 85000000, 90000000, 95000000, 100000000, 105000000, 110000000, 115000000, 120000000, 125000000, 130000000, 135000000, 140000000, 145000000, 150000000, 155000000, 160000000, 165000000, 170000000, 175000000, 180000000, 0,
];

/// `g_CspRateLookup` (`n_csplayer.c:52`).
pub const CSP_RATE_LOOKUP: [f32; 100] = [
    0.05, 0.05, 0.06, 0.06, 0.06, 0.07, 0.07, 0.08, 0.08, 0.09, 0.10, 0.11, 0.13, 0.14, 0.17, 0.20, 0.25, 0.33, 0.5, 1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75, 3.0, 3.25, 3.5, 3.75, 4.0, 4.25, 4.5, 4.75, 5.0, 5.25, 5.5, 5.75, 6.0, 6.25, 6.5, 6.75, 7.0, 7.25, 7.5, 7.75, 8.0, 8.25, 8.5, 8.75, 9.0, 9.25, 9.5, 9.75, 10.0, 10.25, 10.5, 10.75, 11.0, 11.25, 11.5, 11.75, 12.0, 12.25, 12.5, 12.75, 13.0, 13.25, 13.5, 13.75, 14.0, 14.25, 14.5, 14.75, 15.0, 15.25, 15.5, 15.75, 16.0, 16.25, 16.5, 16.75, 17.0, 17.25, 17.5, 17.75, 18.0, 18.25, 18.5, 18.75, 19.0, 19.25, 19.5, 19.75, 20.0, 20.25, 20.5, 20.75, 21.0,
];

/// `oscData` (`osc.c:37`).
#[derive(Clone, Copy, Debug, Default)]
pub struct OscData {
    next: Option<usize>,
    pub osc_type: u8,
    cur_count: i32,
    /// `unk0c`, `unk10`: the other types' depth and offset (the OSC
    /// controller writes `unk0c`, `n_csplayer.c:1036`).
    pub unk0c: f32,
    pub unk10: f32,
    unk14: u16,
    unk16: u16,
    unk18: f32,
    unk1c: f32,
    unk22: u16,
    unk24: u16,
    trem_depth: u8,
    trem_base: u8,
    vib_depth: f32,
}

/// The pool: `oscStates[60]` and `freeOscStateList`.
pub struct OscPool {
    pub states: Vec<OscData>,
    free: Option<usize>,
}

/// `_depth2Cents` (`osc.c:71`).
pub fn depth2cents(depth: u8) -> f32 {
    let mut x: f32 = 1.030_993;
    let mut cents = 1.0f32;
    let mut d = depth;
    while d != 0 {
        if d & 1 != 0 {
            cents *= x;
        }
        x *= x;
        d >>= 1;
    }
    cents
}

/// `osc_s16_to_sine` (`osc.c:176`).
fn osc_s16_to_sine(value: f32) -> f32 {
    (value / 10430.38).sin()
}

impl OscPool {
    /// `osc_build_linkedlist(0, count)` (`osc.c:396`).
    pub fn new(count: usize) -> OscPool {
        let mut states = vec![OscData::default(); count];
        for (i, s) in states.iter_mut().enumerate() {
            s.next = if i + 1 < count { Some(i + 1) } else { None };
        }
        OscPool { states, free: Some(0) }
    }

    /// `osc_init` (`osc.c:86`): a state for the oscillator, its first value,
    /// and when to update it (0 = don't run it).
    pub fn init(&mut self, state: &mut Option<usize>, init_val: &mut f32, osc_type: u8, rate: u8, depth: u8, delay: u8, arg6: u8) -> i32 {
        if delay == 0 {
            return 0;
        }
        if osc_type != OSCTYPE_TREM && osc_type != OSCTYPE_VIB {
            return self.init_other(state, init_val, osc_type, rate, depth, delay, arg6);
        }
        let mut result = 0;
        if let Some(i) = self.free {
            self.free = self.states[i].next;
            let s = &mut self.states[i];
            s.osc_type = osc_type;
            *state = Some(i);
            result = (delay as i32) << 14;
            match osc_type {
                OSCTYPE_TREM => {
                    s.unk24 = 0;
                    s.unk22 = 259 - rate as u16;
                    s.trem_depth = depth >> 1;
                    s.trem_base = 127 - s.trem_depth;
                    *init_val = s.trem_base as f32;
                }
                _ => {
                    s.vib_depth = depth2cents(depth);
                    s.unk24 = 0;
                    s.unk22 = 259 - rate as u16;
                    *init_val = 1.0;
                }
            }
        }
        result
    }

    /// `osc_update` (`osc.c:128`).
    pub fn update(&mut self, state: usize, update_val: &mut f32) -> i32 {
        let s = &mut self.states[state];
        if s.osc_type != OSCTYPE_TREM && s.osc_type != OSCTYPE_VIB {
            return self.update_other(state, update_val);
        }
        s.unk24 += 1;
        if s.unk24 >= s.unk22 {
            s.unk24 = 0;
        }
        let phase = s.unk24 as f32 / s.unk22 as f32;
        // DTOR(360) as PD's f32.
        let sine = (phase * (360.0f32 * std::f32::consts::PI / 180.0)).sin();
        if s.osc_type == OSCTYPE_TREM {
            *update_val = s.trem_base as f32 + sine * s.trem_depth as f32;
        } else {
            *update_val = al_cents2ratio((sine * s.vib_depth) as i32);
        }
        AL_USEC_PER_FRAME
    }

    /// `osc_stop` (`osc.c:161`), with its double free for the other types.
    pub fn stop(&mut self, state: usize) {
        if self.states[state].osc_type != OSCTYPE_TREM && self.states[state].osc_type != OSCTYPE_VIB {
            self.push_free(state);
        }
        self.push_free(state);
    }

    fn push_free(&mut self, state: usize) {
        self.states[state].next = self.free;
        self.free = Some(state);
    }

    /// `osc_init_other` (`osc.c:185`).
    #[allow(clippy::too_many_arguments)]
    fn init_other(&mut self, state: &mut Option<usize>, init_val: &mut f32, osc_type: u8, rate: u8, depth: u8, delay: u8, arg6: u8) -> i32 {
        let rate = rate.min(99) as usize;
        let delay = delay.min(127) as usize;
        let arg6 = arg6.min(127) as usize;
        let Some(i) = self.free else { return 0 };
        let s = &mut self.states[i];
        if arg6 == 0 {
            s.unk18 = 1.0;
            s.unk1c = 0.0;
        } else {
            s.unk18 = 0.0;
            s.unk1c = 1.0 / (CSP_TIME_LOOKUP[arg6] as f32 / AL_USEC_PER_FRAME as f32);
        }
        s.osc_type = osc_type;
        s.unk14 = 0;
        s.unk16 = (1_000_000.0f32 / CSP_RATE_LOOKUP[rate] / AL_USEC_PER_FRAME as f32) as u16;
        s.cur_count = AL_USEC_PER_FRAME;
        let mut depthf = depth as f32;
        if osc_type & OSCTYPE_VIB != 0 {
            depthf = depth2cents(depthf as u8);
        }
        match osc_type & !OSCTYPE_VIB {
            2..=5 => {
                s.unk0c = depthf;
                s.unk10 = if osc_type & !OSCTYPE_VIB == 2 { -depthf } else { 0.0 };
                s.cur_count = (500_000.0f32 / CSP_RATE_LOOKUP[rate]) as i32;
            }
            6 | 8 | 11 | 12 => {
                s.unk10 = 0.0;
                s.unk0c = depthf;
            }
            7 | 9 | 13 => {
                s.unk10 = depthf / 2.0;
                s.unk0c = depthf / 2.0;
            }
            10 => {
                s.unk10 = -depthf;
                s.unk0c = depthf * 2.0;
            }
            _ => return 0,
        }
        *init_val = if s.osc_type & OSCTYPE_VIB != 0 { al_cents2ratio(s.unk10 as i32) } else { s.unk10 + 127.0 };
        *state = Some(i);
        self.free = self.states[i].next;
        if delay != 0 {
            CSP_TIME_LOOKUP[delay]
        } else {
            AL_USEC_PER_FRAME
        }
    }

    /// `osc_update_other` (`osc.c:257`).
    fn update_other(&mut self, state: usize, update_val: &mut f32) -> i32 {
        let s = &mut self.states[state];
        let kind = s.osc_type & !OSCTYPE_VIB;
        let mut sp20 = 0.0f32;
        if kind >= 6 {
            s.unk14 += 1;
            if s.unk14 >= s.unk16 {
                s.unk14 = 0;
            }
            sp20 = s.unk14 as f32 / s.unk16 as f32;
        }
        if s.unk1c != 0.0 {
            s.unk18 += s.unk1c;
            if s.unk18 >= 1.0 {
                s.unk18 = 1.0;
                s.unk1c = 0.0;
            }
        }
        let mut sp24 = s.unk0c;
        if s.unk18 != 1.0 {
            sp24 *= s.unk18;
        }
        let tri = |mut x: f32| {
            if x < 0.25 {
                x *= 4.0 * sp24;
            } else if x >= 0.75 {
                x -= 0.75;
                x *= 4.0 * sp24;
                x -= sp24;
            } else {
                x -= 0.25;
                x *= 4.0 * sp24;
                x = sp24 - x;
            }
            x
        };
        match kind {
            2..=5 => {
                sp20 = if s.unk14 != 0 { sp24 } else { s.unk10 };
                s.unk14 ^= 1;
            }
            6 | 7 => sp20 = tri(sp20) + s.unk10,
            8 | 9 => sp20 = osc_s16_to_sine(sp20 * 65536.0) * sp24 + s.unk10,
            10 | 11 => sp20 = sp20 * sp24 + s.unk10,
            12 | 13 => {
                let sp1c = s.unk10 + tri(sp20);
                let x = s.unk14 as f32 / s.unk16 as f32;
                sp20 = (osc_s16_to_sine(x * 65536.0) * sp24 + s.unk10 + sp1c) / 2.0;
            }
            _ => {}
        }
        *update_val = if s.osc_type & OSCTYPE_VIB != 0 { al_cents2ratio(sp20 as i32) } else { sp20 + 127.0 };
        s.cur_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vibrato_swings_both_ways_and_returns_its_state() {
        let mut pool = OscPool::new(60);
        let (mut st, mut v) = (None, 0.0);
        let delay = pool.init(&mut st, &mut v, OSCTYPE_VIB, 250, 20, 4, 0);
        assert_eq!((delay, v), (4 << 14, 1.0));
        let i = st.unwrap();
        let (mut lo, mut hi) = (1.0f32, 1.0f32);
        for _ in 0..20 {
            assert_eq!(pool.update(i, &mut v), AL_USEC_PER_FRAME);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo < 1.0 && hi > 1.0, "{lo} {hi}");
        pool.stop(i);
        let mut st2 = None;
        pool.init(&mut st2, &mut v, OSCTYPE_TREM, 250, 40, 1, 0);
        assert_eq!((st2, v), (Some(i), 107.0), "the freed state is reused first; 127 - 40/2");
    }
}
