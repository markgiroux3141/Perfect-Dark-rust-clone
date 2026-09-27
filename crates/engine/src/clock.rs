//! `FrameClock`: a fixed-tick accumulator with a substep cap, a frame pacer, and an
//! interpolation alpha for rendering between ticks. The tick rate is set by the
//! game. Perfect Dark runs its world in N64 frames of 60, 30 or 20 Hz, so the game
//! picks the tick rate and converts each tick to PD's `lvupdate240`.
