//! Audio on kira: load and cache sounds by file path, then play them as voices
//! with a playback rate, volume and pan, looping or not; a voice can be stopped by
//! id. DSP tracks run a caller-supplied stereo processor on the audio thread. A
//! stream plays stereo frames the caller pushes at its own rate (a synth the
//! game runs), resampled to the device's. The engine does not know what the
//! sounds mean. The game maps its sound events onto voices.
//!
//! Everything is best-effort: no output device, or a missing or undecodable file,
//! logs a warning and plays nothing, so the game still runs silently.
//!
//! Source: the old engine's `AudioManager` (`play_voice`, `add_dsp_track`,
//! `TrackDsp`), which the gun and TV-audio spikes were verified against. Paths
//! are the caller's (the old one resolved names against a compile-time root);
//! the background-music track is dropped (a game's music is its own).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kira::effect::Effect;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::{Sound, SoundData};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Frame, Panning, PlaybackRate, Tween};

/// A caller-owned stereo effect for a mixer sub-track ([`Audio::add_dsp_track`]).
/// It runs on the audio thread, once per output frame, after the track's voices
/// are summed. Don't allocate or lock in [`Self::frame`] / [`Self::on_block`];
/// [`Self::init`] may allocate.
pub trait TrackDsp: Send + 'static {
    /// The output sample rate: once before the first block, and again if the
    /// device changes rate.
    fn init(&mut self, _sample_rate: u32) {}
    /// Once per block (a few ms of frames), before its frames: pick up new settings here.
    fn on_block(&mut self) {}
    /// One stereo frame in, one out.
    fn frame(&mut self, l: f32, r: f32) -> (f32, f32);
}

struct DspEffect<T>(T);

impl<T: TrackDsp> Effect for DspEffect<T> {
    fn init(&mut self, sample_rate: u32, _internal_buffer_size: usize) {
        self.0.init(sample_rate);
    }
    fn on_change_sample_rate(&mut self, sample_rate: u32) {
        self.0.init(sample_rate);
    }
    fn on_start_processing(&mut self) {
        self.0.on_block();
    }
    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &kira::info::Info) {
        for f in input {
            let (l, r) = self.0.frame(f.left, f.right);
            f.left = l;
            f.right = r;
        }
    }
}

struct StreamShared {
    /// Frames pushed and not yet taken by the audio thread.
    queue: Mutex<VecDeque<[f32; 2]>>,
    /// How many the audio thread holds.
    held: AtomicUsize,
    volume: AtomicU32,
    stop: AtomicBool,
}

/// The caller's side of a stream made by [`Audio::play_stream`]: push
/// frames, keep [`Stream::buffered`] above what a frame of the game lasts.
#[derive(Clone)]
pub struct Stream(Arc<StreamShared>);

impl Stream {
    /// Queue stereo frames (at the stream's rate).
    pub fn push(&self, frames: impl IntoIterator<Item = [f32; 2]>) {
        if let Ok(mut q) = self.0.queue.lock() {
            q.extend(frames);
        }
    }

    /// Frames waiting to play (the queue and what the audio thread holds).
    pub fn buffered(&self) -> usize {
        self.0.queue.lock().map_or(0, |q| q.len()) + self.0.held.load(Ordering::Relaxed)
    }

    /// A linear gain.
    pub fn set_volume(&self, v: f32) {
        self.0.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    /// Stop playing (the voice ends on the next block).
    pub fn stop(&self) {
        self.0.stop.store(true, Ordering::Relaxed);
    }
}

/// The audio thread's side: takes what was pushed at the start of each block
/// (`try_lock`, never waiting), and reads it at `rate` with linear
/// interpolation. Running dry plays silence and waits for more.
struct StreamSound {
    shared: Arc<StreamShared>,
    held: VecDeque<[f32; 2]>,
    rate: f64,
    pos: f64,
}

impl Sound for StreamSound {
    fn on_start_processing(&mut self) {
        if let Ok(mut q) = self.shared.queue.try_lock() {
            let room = self.held.capacity() - self.held.len();
            let n = q.len().min(room);
            self.held.extend(q.drain(..n));
        }
        self.shared.held.store(self.held.len(), Ordering::Relaxed);
    }

    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &kira::info::Info) {
        let step = self.rate * dt;
        let vol = f32::from_bits(self.shared.volume.load(Ordering::Relaxed));
        for f in out {
            if self.held.len() < 2 {
                *f = Frame::ZERO;
                continue;
            }
            let (a, b) = (self.held[0], self.held[1]);
            let t = self.pos as f32;
            *f = Frame::new((a[0] + (b[0] - a[0]) * t) * vol, (a[1] + (b[1] - a[1]) * t) * vol);
            self.pos += step;
            while self.pos >= 1.0 && self.held.len() >= 2 {
                self.held.pop_front();
                self.pos -= 1.0;
            }
        }
    }

    fn finished(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }
}

struct StreamData {
    shared: Arc<StreamShared>,
    rate: f64,
}

impl SoundData for StreamData {
    type Error = ();
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn Sound>, ()), ()> {
        // A second of frames, allocated here rather than on the audio thread.
        let cap = self.rate.ceil() as usize;
        Ok((Box::new(StreamSound { shared: self.shared, held: VecDeque::with_capacity(cap), rate: self.rate, pos: 0.0 }), ()))
    }
}

/// A mixer sub-track made by [`Audio::add_dsp_track`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackId(usize);

/// A voice started by [`Audio::play_voice`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

/// A linear gain (1 unchanged, 0.5 half) as decibels, kept away from −∞.
fn amplitude_to_db(amplitude: f32) -> Decibels {
    Decibels(20.0 * amplitude.max(1e-4).log10())
}

/// The mixer, the decoded-sound cache and the live voices.
pub struct Audio {
    manager: AudioManager<DefaultBackend>,
    sounds: HashMap<PathBuf, Option<StaticSoundData>>,
    voices: HashMap<VoiceId, StaticSoundHandle>,
    next_voice: u64,
    tracks: Vec<TrackHandle>,
}

impl Audio {
    /// `None` (with a warning) if there is no output device.
    pub fn new() -> Option<Audio> {
        match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(manager) => Some(Audio { manager, sounds: HashMap::new(), voices: HashMap::new(), next_voice: 1, tracks: Vec::new() }),
            Err(e) => {
                log::warn!("audio: device init failed, running silent: {e}");
                None
            }
        }
    }

    /// Decode and cache a sound (a failure is remembered and warned once).
    pub fn load(&mut self, path: &Path) {
        if self.sounds.contains_key(path) {
            return;
        }
        let data = match StaticSoundData::from_file(path) {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn!("audio: load {} failed: {e}", path.display());
                None
            }
        };
        self.sounds.insert(path.to_path_buf(), data);
    }

    /// Play a sound at `volume` (linear), `rate` (1 as recorded) and `pan` (−1
    /// left .. 1 right). Returns an id for [`Self::stop_voice`].
    pub fn play_voice(&mut self, path: &Path, volume: f32, rate: f64, pan: f32, looping: bool) -> Option<VoiceId> {
        self.play_voice_on(None, path, volume, rate, pan, looping)
    }

    /// Add a mixer sub-track whose summed voices pass through `dsp`.
    pub fn add_dsp_track<T: TrackDsp>(&mut self, dsp: T) -> Option<TrackId> {
        match self.manager.add_sub_track(TrackBuilder::new().with_built_effect(Box::new(DspEffect(dsp)))) {
            Ok(handle) => {
                self.tracks.push(handle);
                Some(TrackId(self.tracks.len() - 1))
            }
            Err(e) => {
                log::warn!("audio: add sub-track failed: {e}");
                None
            }
        }
    }

    /// [`Self::play_voice`] onto a sub-track (`None` is the main track).
    pub fn play_voice_on(&mut self, track: Option<TrackId>, path: &Path, volume: f32, rate: f64, pan: f32, looping: bool) -> Option<VoiceId> {
        self.load(path);
        let data = self.sounds.get(path)?.as_ref()?;
        let mut data = data.volume(amplitude_to_db(volume)).playback_rate(PlaybackRate(rate)).panning(Panning(pan.clamp(-1.0, 1.0)));
        if looping {
            data = data.loop_region(..);
        }
        self.voices.retain(|_, h| h.state() != kira::sound::PlaybackState::Stopped);
        let played = match track.and_then(|t| self.tracks.get_mut(t.0)) {
            Some(t) => t.play(data),
            None => self.manager.play(data),
        };
        match played {
            Ok(handle) => {
                let id = VoiceId(self.next_voice);
                self.next_voice += 1;
                self.voices.insert(id, handle);
                Some(id)
            }
            Err(e) => {
                log::warn!("audio: play {} failed: {e}", path.display());
                None
            }
        }
    }

    /// Start a stream of stereo frames at `rate` Hz on `track` (`None` is the
    /// main track); the caller pushes frames through the returned [`Stream`].
    pub fn play_stream(&mut self, track: Option<TrackId>, rate: f64) -> Option<Stream> {
        let shared = Arc::new(StreamShared { queue: Mutex::new(VecDeque::new()), held: AtomicUsize::new(0), volume: AtomicU32::new(1.0f32.to_bits()), stop: AtomicBool::new(false) });
        let data = StreamData { shared: shared.clone(), rate };
        let played = match track.and_then(|t| self.tracks.get_mut(t.0)) {
            Some(t) => t.play(data).map(|_| ()),
            None => self.manager.play(data).map(|_| ()),
        };
        match played {
            Ok(()) => Some(Stream(shared)),
            Err(e) => {
                log::warn!("audio: play stream failed: {e:?}");
                None
            }
        }
    }

    /// Retune a playing voice: `volume` (linear) and playback `rate`.
    pub fn set_voice(&mut self, id: VoiceId, volume: f32, rate: f64) {
        if let Some(h) = self.voices.get_mut(&id) {
            h.set_volume(amplitude_to_db(volume), Tween::default());
            h.set_playback_rate(PlaybackRate(rate), Tween::default());
        }
    }

    /// Stop a voice (no-op if it already finished).
    pub fn stop_voice(&mut self, id: VoiceId) {
        if let Some(mut h) = self.voices.remove(&id) {
            h.stop(Tween::default());
        }
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    /// A stream at half the device's rate reads each pushed frame twice,
    /// interpolating between; dry, it plays silence and resumes.
    #[test]
    fn a_stream_resamples_and_waits_when_dry() {
        let shared = Arc::new(StreamShared { queue: Mutex::new(VecDeque::new()), held: AtomicUsize::new(0), volume: AtomicU32::new(1.0f32.to_bits()), stop: AtomicBool::new(false) });
        let stream = Stream(shared.clone());
        let (mut sound, ()) = StreamData { shared, rate: 24_000.0 }.into_sound().unwrap();
        stream.push([[0.0, 0.0], [1.0, -1.0], [0.0, 0.0]]);
        assert_eq!(stream.buffered(), 3);
        sound.on_start_processing();
        let info = kira::info::MockInfoBuilder::new().build();
        let mut out = [Frame::ZERO; 6];
        sound.process(&mut out, 1.0 / 48_000.0, &info);
        let l: Vec<f32> = out.iter().map(|f| f.left).collect();
        assert_eq!(&l[..4], &[0.0, 0.5, 1.0, 0.5]);
        assert_eq!(&l[4..], &[0.0, 0.0], "dry");
        assert!(!sound.finished());
        stream.stop();
        assert!(sound.finished());
    }
}
