//! `FrameClock`: a fixed-tick accumulator with a substep cap, a frame pacer, and an
//! interpolation alpha for rendering between ticks. The tick rate is set by the
//! game, and can change while running (a game whose frame rate is itself a
//! setting ticks at that rate and converts each tick to its own time units).
//!
//! Source: the old engine's `platform/frame.rs` `FrameClock` (accumulator, backlog
//! drop, deadline pacing), plus a settable tick rate and the alpha.

use std::time::{Duration, Instant};

/// A render-loop clock: real frame time, a fixed-tick accumulator and a pacer.
pub struct FrameClock {
    tick_hz: f64,
    max_substeps: u32,
    frame_budget: Duration,
    last_frame: Option<Instant>,
    /// Elapsed time not yet consumed by a tick, in seconds.
    accumulator: f64,
    /// The next instant a frame may be rendered (the FPS cap).
    next_frame: Option<Instant>,
    /// Smoothed frames per second, for a debug readout.
    fps: f32,
}

impl FrameClock {
    /// `tick_hz`: the fixed tick rate. `max_substeps`: the most ticks one frame
    /// may run (a longer backlog is dropped). `max_fps`: the render pacing ceiling.
    pub fn new(tick_hz: f64, max_substeps: u32, max_fps: u32) -> Self {
        FrameClock {
            tick_hz,
            max_substeps: max_substeps.max(1),
            frame_budget: Duration::from_micros(1_000_000 / max_fps.max(1) as u64),
            last_frame: None,
            accumulator: 0.0,
            next_frame: None,
            fps: 0.0,
        }
    }

    pub fn tick_hz(&self) -> f64 {
        self.tick_hz
    }

    /// Change the tick rate; the time already accumulated carries over.
    pub fn set_tick_hz(&mut self, hz: f64) {
        self.tick_hz = hz.max(1.0);
    }

    pub fn tick_dt(&self) -> f64 {
        1.0 / self.tick_hz
    }

    /// Call once at the start of a rendered frame. Returns the real time since the
    /// previous frame (clamped to 0.25 s so a stall cannot inject a huge step) and
    /// adds it to the accumulator.
    pub fn begin_frame(&mut self, now: Instant) -> f64 {
        let dt = self.last_frame.replace(now).map(|t| (now - t).as_secs_f64()).unwrap_or(0.0).min(0.25);
        self.accumulator += dt;
        if dt > 0.0 {
            let inst = (1.0 / dt) as f32;
            self.fps = if self.fps == 0.0 { inst } else { self.fps * 0.9 + inst * 0.1 };
        }
        dt
    }

    /// How many ticks to run this frame, draining the accumulator. At the cap the
    /// rest of the backlog is dropped.
    pub fn take_ticks(&mut self) -> u32 {
        // A microsecond of slack: frame times that sum to a whole number of ticks
        // (a 60 Hz display under a 60 Hz tick) must not lose one to rounding.
        let dt = self.tick_dt() - 1e-6;
        let mut n = 0;
        while self.accumulator >= dt && n < self.max_substeps {
            self.accumulator = (self.accumulator - self.tick_dt()).max(0.0);
            n += 1;
        }
        if n == self.max_substeps && self.accumulator >= dt {
            self.accumulator = 0.0;
        }
        n
    }

    /// How far the frame is between the last tick and the next, 0..1.
    pub fn alpha(&self) -> f32 {
        (self.accumulator / self.tick_dt()).clamp(0.0, 1.0) as f32
    }

    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// Frame pacing: whether to render now, and when the next frame is due. The
    /// deadline advances by the budget (steady pacing) and resyncs after a stall.
    pub fn pace(&mut self, now: Instant) -> (bool, Instant) {
        let next = self.next_frame.get_or_insert(now);
        let redraw = now >= *next;
        if redraw {
            *next += self.frame_budget;
            if *next < now {
                *next = now + self.frame_budget;
            }
        }
        (redraw, *next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_accumulate_and_carry_the_remainder() {
        let mut c = FrameClock::new(60.0, 8, 240);
        let t0 = Instant::now();
        c.begin_frame(t0);
        assert_eq!(c.take_ticks(), 0);
        // 2.5 ticks of time: two ticks now, half a tick left over.
        c.begin_frame(t0 + Duration::from_secs_f64(2.5 / 60.0));
        assert_eq!(c.take_ticks(), 2);
        assert!((c.alpha() - 0.5).abs() < 1e-3);
        c.begin_frame(t0 + Duration::from_secs_f64(3.0 / 60.0));
        assert_eq!(c.take_ticks(), 1);
    }

    #[test]
    fn a_stall_is_capped_and_its_backlog_dropped() {
        let mut c = FrameClock::new(60.0, 4, 240);
        let t0 = Instant::now();
        c.begin_frame(t0);
        c.begin_frame(t0 + Duration::from_millis(200));
        assert_eq!(c.take_ticks(), 4);
        assert_eq!(c.alpha(), 0.0);
    }

    #[test]
    fn a_slower_tick_rate_takes_fewer_ticks() {
        let mut c = FrameClock::new(60.0, 8, 240);
        c.set_tick_hz(20.0);
        let t0 = Instant::now();
        c.begin_frame(t0);
        c.begin_frame(t0 + Duration::from_secs_f64(0.1));
        assert_eq!(c.take_ticks(), 2);
    }

    #[test]
    fn pacing_keeps_to_the_budget() {
        let mut c = FrameClock::new(60.0, 8, 100);
        let t0 = Instant::now();
        assert!(c.pace(t0).0);
        let (redraw, next) = c.pace(t0 + Duration::from_millis(3));
        assert!(!redraw);
        assert_eq!(next, t0 + Duration::from_millis(10));
    }
}
