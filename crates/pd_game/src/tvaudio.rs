//! The `n64::audio` chain on an engine DSP track: [`TvTrack`] runs a
//! [`TvChain`] on the audio thread, [`TvLink`] is the UI's side of it, and
//! [`TvRoute`] decides which track a new voice plays on.
//!
//! The sub-track is only made once something in the settings is switched on,
//! and voices go to the main track whenever everything is off, so a session that
//! never touches the settings plays exactly as it would without the chain.
//!
//! Source: the old repo's `pd_guns/tvaudio.rs` (`TvTrack`, `TvLink`) and
//! `pd_guns/app.rs` (`set_tv_audio`, `flush_sounds`' track choice).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use engine::audio::{Audio, TrackDsp, TrackId};
use n64::audio::{AudioSettings, TvChain};

struct Shared {
    settings: Mutex<AudioSettings>,
    version: AtomicU64,
}

/// The UI's handle to a [`TvTrack`] on the audio thread.
#[derive(Clone)]
pub struct TvLink(Arc<Shared>);

impl TvLink {
    /// A link plus the track DSP it drives (hand the track to `add_dsp_track`).
    pub fn new(s: AudioSettings) -> (TvLink, TvTrack) {
        let shared = Arc::new(Shared { settings: Mutex::new(s), version: AtomicU64::new(0) });
        let track = TvTrack { chain: TvChain::new(48_000, s), shared: shared.clone(), seen: 0 };
        (TvLink(shared), track)
    }

    pub fn set(&self, s: AudioSettings) {
        if let Ok(mut g) = self.0.settings.lock() {
            *g = s;
        }
        self.0.version.fetch_add(1, Ordering::Release);
    }
}

/// [`TvChain`] as a kira sub-track effect. New settings arrive through a
/// `try_lock` once per block, which never blocks the audio thread: a
/// contended block just picks them up next time.
pub struct TvTrack {
    chain: TvChain,
    shared: Arc<Shared>,
    seen: u64,
}

impl TvTrack {
    fn current(&self) -> AudioSettings {
        self.shared.settings.lock().map(|g| *g).unwrap_or(self.chain.settings())
    }
}

impl TrackDsp for TvTrack {
    fn init(&mut self, sample_rate: u32) {
        self.chain = TvChain::new(sample_rate, self.current());
        self.seen = self.shared.version.load(Ordering::Acquire);
    }
    fn on_block(&mut self) {
        self.chain.flush();
        let v = self.shared.version.load(Ordering::Acquire);
        if v != self.seen {
            if let Ok(g) = self.shared.settings.try_lock() {
                let s = *g;
                drop(g);
                self.chain.set(s);
                self.seen = v;
            }
        }
    }
    #[inline]
    fn frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.chain.frame(l, r)
    }
}

/// The settings the UI last set, and the sub-track once one is needed.
#[derive(Default)]
pub struct TvRoute {
    settings: AudioSettings,
    track: Option<(TrackId, TvLink)>,
    /// `add_dsp_track` failed (it warned): don't retry on every voice.
    failed: bool,
}

impl TvRoute {
    pub fn settings(&self) -> AudioSettings {
        self.settings
    }

    /// Apply new settings; the audio thread picks them up on its next block.
    pub fn set(&mut self, s: AudioSettings) {
        if s == self.settings {
            return;
        }
        self.settings = s;
        if let Some((_, link)) = &self.track {
            link.set(s);
        }
    }

    /// The track a new voice plays on: the TV track while anything is on (made
    /// on first use), the main track (`None`) otherwise.
    pub fn track(&mut self, audio: &mut Audio) -> Option<TrackId> {
        if !self.settings.active() {
            return None;
        }
        if self.track.is_none() && !self.failed {
            let (link, dsp) = TvLink::new(self.settings);
            self.track = audio.add_dsp_track(dsp).map(|id| (id, link));
            self.failed = self.track.is_none();
        }
        self.track.as_ref().map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: u32 = 48_000;

    #[test]
    fn the_link_retunes_the_track_on_the_next_block() {
        let (link, mut track) = TvLink::new(AudioSettings::default());
        track.init(FS);
        assert_eq!(track.frame(0.25, -0.5), (0.25, -0.5));
        link.set(AudioSettings { tv: true, ..AudioSettings::default() });
        assert_eq!(track.frame(0.25, -0.5), (0.25, -0.5), "not before the block");
        track.on_block();
        assert!(track.chain.settings().tv);
        let (l, r) = track.frame(0.25, -0.5);
        assert_eq!(l, r, "mono now");
    }
}
