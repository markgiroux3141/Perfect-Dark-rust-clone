//! The N64 output chain and the 90s TV speaker, as a pure stereo processor:
//! 22020 Hz sample-and-hold mix with anti-alias/DAC filters, then speaker, cabinet,
//! compressor, rail and whine presets. The game adapts it to an engine DSP track.
//! The same code renders offline in tests and in the `pd_tools` audio renderer.
//!
//! Source: `pd_guns/tvaudio.rs` (spike `spike/crt-tv-audio`, 11 tests).
