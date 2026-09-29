//! PD's music on an engine stream: the one [`pd_core::music::Music`], fed the
//! menus' and the world's `Event::Music` calls in PD's frame order, with
//! audio frames made as the device needs them.
//!
//! The synth runs on the game thread, as PD's audio thread runs beside its
//! game: 736-sample frames at 22018 Hz go to an engine stream (resampled to
//! the device) and are kept about four frames ahead of what it has played.
//! Without an output device the frames are made on the clock and dropped,
//! so the sequence players run as they would.

use std::sync::Arc;

use engine::audio::{Audio, Stream, TrackId};
use n64::naudio::OUTPUT_RATE;
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::music::{Music, MusicCall, MusicData};

/// How far ahead of the device the stream is kept: four audio frames (133
/// ms), more than a 20 Hz game frame.
const AHEAD: usize = 4 * 736;

pub struct MusicPlayer {
    pub music: Music,
    stream: Option<(Stream, Option<TrackId>)>,
    frames: Vec<[i16; 2]>,
    /// Without a device: samples due by the clock.
    due: f64,
}

impl MusicPlayer {
    pub fn load(assets: &AssetDir) -> Result<MusicPlayer, String> {
        let data = Arc::new(MusicData::load(assets)?);
        Ok(MusicPlayer { music: Music::new(data), stream: None, frames: Vec::new(), due: 0.0 })
    }

    /// Apply the `Event::Music` calls among `events`, in order.
    pub fn apply(&mut self, events: &[Event]) {
        for e in events {
            if let Event::Music(c) = e {
                self.music.apply(*c);
            }
        }
    }

    /// `music_tick`'s `music_tick_events` for a frame with no world (the
    /// menus run on CI's `lv_tick`, whose `music_tick` has nothing of a
    /// match's to do).
    pub fn tick(&mut self, diffframe240: i32) {
        self.music.apply(MusicCall::Tick { diffframe240 });
    }

    /// `lv_stop`'s `music_stop` and the next stage's `music_reset`.
    pub fn stage_change(&mut self) {
        self.music.music_stop();
        self.music.music_reset();
    }

    /// The stream's level (the panel's; 1 is PD's mix).
    pub fn set_gain(&mut self, g: f32) {
        if let Some((s, _)) = &self.stream {
            s.set_volume(g);
        }
    }

    /// Make audio frames: onto the stream while it holds less than [`AHEAD`]
    /// (restarting it when the track to play on changes), or, with no
    /// device, as `dt` seconds of the clock pass.
    pub fn pump(&mut self, audio: Option<&mut Audio>, track: Option<TrackId>, dt: f64) {
        let Some(audio) = audio else {
            self.due += dt * OUTPUT_RATE as f64;
            while self.due >= 736.0 {
                self.frames.clear();
                self.music.render_frame(&mut self.frames);
                self.due -= 736.0;
            }
            return;
        };
        if self.stream.as_ref().is_some_and(|(_, t)| *t != track) {
            if let Some((s, _)) = self.stream.take() {
                s.stop();
            }
        }
        if self.stream.is_none() {
            self.stream = audio.play_stream(track, OUTPUT_RATE as f64).map(|s| (s, track));
        }
        let Some((stream, _)) = &self.stream else { return };
        let mut n = 0;
        while stream.buffered() < AHEAD && n < 8 {
            self.frames.clear();
            self.music.render_frame(&mut self.frames);
            stream.push(self.frames.iter().map(|f| [f[0] as f32 / 32768.0, f[1] as f32 / 32768.0]));
            n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use pd_core::ids::*;

    use super::*;

    /// With no device the clock drives the synth: the Combat Simulator's
    /// tune, asked for by the menus, is playing a second later.
    #[test]
    fn the_clock_runs_the_music_without_a_device() {
        let mut p = MusicPlayer::load(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))).unwrap();
        p.apply(&[Event::Music(MusicCall::StartTrackAsMenu(MUSIC_COMBATSIM_MENU))]);
        for _ in 0..60 {
            p.tick(4);
            p.pump(None, None, 1.0 / 60.0);
        }
        assert!(p.music.music_is_track_type_playing(TRACKTYPE_MENU));
    }
}
