//! Audio on kira: load and cache sounds by asset path, then play them as voices
//! with a playback rate, volume and pan, looping or not; a voice can be stopped
//! by id. DSP tracks run a caller-supplied stereo processor on the audio thread.
//! The engine does not know what the sounds mean. The game maps its sound events
//! onto voices.
//!
//! Source: the old engine's `AudioManager` (`play_voice`, `add_dsp_track`,
//! `TrackDsp`), which the gun and TV-audio spikes were verified against.
