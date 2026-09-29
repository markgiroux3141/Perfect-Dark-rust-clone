//! Sound events to engine voices: a PD sound ref to `assets/sfx/<num>.wav` with
//! the manifest's volume, played at PD's per-call pitch and volume.
//!
//! An `SFXMAP_*` ref resolves through the manifest's alias to its bank sound
//! (`g_AudioRussMappings`, snd.c:2111). The alias entries also carry the mapping's
//! audio config (volume percentage, pitch), which only the prop-sound and
//! `snd_start_extra` paths apply; a bare `snd_start`, which is how the menus play,
//! does not, so this plays the bank sound's own entry.
//!
//! A sound on a handle (a hand's `audiohandle`: the Reaper's spin, the Mauler's
//! charge) keeps its voice until it is stopped or replaced, and loops if its
//! sample has a loop in the bank.
//!
//! Every voice is scaled by [`SFX_MIX`], the synth's gain for an SFX voice
//! (`g_SfxVolume` and the centre pan), so the SFX sit against the music
//! (`crate::music`, which comes out of the synth at PD's level) as PD mixes them.
//!
//! With the `n64::audio` chain switched on ([`SfxBank::set_tv`]), new voices play
//! on its DSP track ([`crate::tvaudio`]); with it off they play on the main track.
//! A voice stays on the track it started on.

use std::collections::HashMap;
use std::path::PathBuf;

use engine::audio::{Audio, VoiceId};
use n64::audio::AudioSettings;
use pd_core::assets::AssetDir;
use pd_core::events::Event;

use crate::tvaudio::TvRoute;

/// An SFX voice's gain in PD's synth: `n_sndplayer`'s volume table at
/// `g_SfxVolume` (0x5000 after `gamefile_load_defaults`, `snd.c:890`), then
/// the envelope mixer's centre pan (`n_eqpower[64]` = 0x59f2, `n_env.c`). The
/// manifest's volume is the sound's own (`sampleVolume`, `attackVolume`).
pub const SFX_MIX: f32 = (0x5000 as f32 / 0x7fff as f32) * (0x59f2 as f32 / 0x7fff as f32);

struct Entry {
    path: PathBuf,
    volume: f32,
    looping: bool,
}

pub struct SfxBank {
    /// By bank sound number.
    sounds: HashMap<u16, Entry>,
    /// `SFXMAP_*` refs to bank sound numbers.
    maps: HashMap<u16, u16>,
    /// The voices playing on sound handles, with their sample's own volume.
    handles: HashMap<u32, (VoiceId, f32)>,
    /// The N64 output + TV speaker chain, and which track voices play on.
    tv: TvRoute,
}

fn hex4(s: &str) -> Option<u16> {
    s.get(..4).and_then(|h| u16::from_str_radix(h, 16).ok())
}

impl SfxBank {
    pub fn load(assets: &AssetDir) -> Result<SfxBank, String> {
        let m: serde_json::Value = assets.read_json(&assets.sfx_manifest())?;
        let obj = m.as_object().ok_or("sfx/manifest.json: not an object")?;
        let (mut sounds, mut maps) = (HashMap::new(), HashMap::new());
        for (key, e) in obj {
            if key.len() == 4 {
                let (Some(num), Some(file)) = (hex4(key), e["file"].as_str()) else { continue };
                let volume = e["volume"].as_f64().unwrap_or(1.0) as f32;
                let looping = !e["loop"].is_null();
                sounds.insert(num, Entry { path: assets.root().join("sfx").join(file), volume, looping });
            } else if let Some(rest) = key.strip_prefix("SFXMAP_") {
                if let (Some(r), Some(num)) = (hex4(rest), e["sound"].as_str().and_then(hex4)) {
                    maps.insert(r, num);
                }
            }
        }
        Ok(SfxBank { sounds, maps, handles: HashMap::new(), tv: TvRoute::default() })
    }

    /// The bank sound a ref plays.
    fn resolve(&self, sound: u16) -> Option<&Entry> {
        let num = if sound & 0x8000 != 0 { *self.maps.get(&sound)? } else { sound & 0x7ff };
        self.sounds.get(&num)
    }

    pub fn play(&mut self, audio: Option<&mut Audio>, events: &[Event]) {
        let Some(audio) = audio else { return };
        let track = self.tv.track(audio);
        for ev in events {
            match *ev {
                Event::Sound { sound, pitch, volume, pan } => match self.resolve(sound) {
                    Some(e) => {
                        audio.play_voice_on(track, &e.path, SFX_MIX * e.volume * volume, pitch as f64, pan, false);
                    }
                    None => log::debug!("sfx: no sample for sound {sound:#06x}"),
                },
                Event::HandleSound { handle, sound, pitch, volume, pan } => {
                    if let Some((v, _)) = self.handles.remove(&handle) {
                        audio.stop_voice(v);
                    }
                    match self.resolve(sound) {
                        Some(e) => {
                            let base = SFX_MIX * e.volume;
                            if let Some(v) = audio.play_voice_on(track, &e.path, base * volume, pitch as f64, pan, e.looping) {
                                self.handles.insert(handle, (v, base));
                            }
                        }
                        None => log::debug!("sfx: no sample for sound {sound:#06x}"),
                    }
                }
                Event::StopSound { handle } => {
                    if let Some((v, _)) = self.handles.remove(&handle) {
                        audio.stop_voice(v);
                    }
                }
                Event::SoundParams { handle, pitch, volume } => {
                    if let Some(&(v, base)) = self.handles.get(&handle) {
                        audio.set_voice(v, base * volume, pitch as f64);
                    }
                }
                // SUBST: sndp_stop_all stops every voice / the kept ones stop
                // (the alarm, the loops); the one-shots play out, as the
                // engine keeps no list of them.
                Event::StopAllSounds => {
                    for (_, (v, _)) in self.handles.drain() {
                        audio.stop_voice(v);
                    }
                }
                Event::Kill { .. } | Event::MpPushPauseDialog { .. } | Event::AmOpenPickTarget { .. } | Event::MpCloseMenus { .. } | Event::MpEndMatch | Event::Music(_) => {}
            }
        }
    }

    /// The track new voices (and the music's stream) play on.
    pub fn track(&mut self, audio: &mut Audio) -> Option<engine::audio::TrackId> {
        self.tv.track(audio)
    }

    /// The N64 output + TV speaker settings in use.
    pub fn tv(&self) -> AudioSettings {
        self.tv.settings()
    }

    /// Change the N64 output + TV speaker settings (all off by default). The
    /// DSP track is made on the next sound once something is on.
    pub fn set_tv(&mut self, s: AudioSettings) {
        self.tv.set(s);
    }

    /// Stop every handled sound (the match ended).
    pub fn stop_all(&mut self, audio: Option<&mut Audio>) {
        let Some(audio) = audio else { return };
        for (_, (v, _)) in self.handles.drain() {
            audio.stop_voice(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_sounds_resolve() {
        let bank = SfxBank::load(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))).unwrap();
        // SFXMAP_8098_EXPLOSION plays SFXNUM_00B4 (snd.c:324); the menus' own sounds are bank numbers.
        assert!(bank.resolve(0x8098).unwrap().path.ends_with("00b4.wav"));
        assert!(bank.resolve(0x8040).unwrap().path.ends_with("006e.wav"));
        for num in [0x05bb, 0x05bc, 0x043e, 0x0441, 0x05dd, 0x002b, 0x00ea] {
            let e = bank.resolve(num).unwrap_or_else(|| panic!("{num:#x}"));
            assert!(e.path.exists() && e.volume > 0.0, "{num:#x}");
        }
    }
}
