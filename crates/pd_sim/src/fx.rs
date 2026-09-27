//! Effect state that the world ticks: tracers and beams, sparks, wall hits and
//! bullet holes, casings, smoke. It is simulation state because PD ticks it with
//! `lvupdate240`; drawing it is `pd_render`'s job.
//!
//! Sources: `pd_guns/fx.rs`, `smoke.rs`.
