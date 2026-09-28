//! Sound events to engine voices: a PD sound ref to `assets/sfx/<num>.wav` with
//! the manifest's volume, played at PD's per-call pitch and volume.
//!
//! An `SFXMAP_*` ref resolves through the manifest's alias to its bank sound
//! (`g_AudioRussMappings`, snd.c:2111). The alias entries also carry the mapping's
//! audio config (volume percentage, pitch), which only the prop-sound and
//! `snd_start_extra` paths apply; a bare `snd_start`, which is how the menus play,
//! does not, so this plays the bank sound's own entry.
//!
//! Later: loops stopped by id (M4), and the `n64::audio` TV-speaker chain on an
//! engine DSP track (M5).

use std::collections::HashMap;
use std::path::PathBuf;

use engine::audio::Audio;
use pd_core::assets::AssetDir;
use pd_core::events::Event;

struct Entry {
    path: PathBuf,
    volume: f32,
}

pub struct SfxBank {
    /// By bank sound number.
    sounds: HashMap<u16, Entry>,
    /// `SFXMAP_*` refs to bank sound numbers.
    maps: HashMap<u16, u16>,
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
                sounds.insert(num, Entry { path: assets.root().join("sfx").join(file), volume });
            } else if let Some(rest) = key.strip_prefix("SFXMAP_") {
                if let (Some(r), Some(num)) = (hex4(rest), e["sound"].as_str().and_then(hex4)) {
                    maps.insert(r, num);
                }
            }
        }
        Ok(SfxBank { sounds, maps })
    }

    /// The bank sound a ref plays.
    fn resolve(&self, sound: u16) -> Option<&Entry> {
        let num = if sound & 0x8000 != 0 { *self.maps.get(&sound)? } else { sound & 0x7ff };
        self.sounds.get(&num)
    }

    pub fn play(&self, audio: Option<&mut Audio>, events: &[Event]) {
        let Some(audio) = audio else { return };
        for ev in events {
            match *ev {
                Event::Sound { sound, pitch, volume, pan } => match self.resolve(sound) {
                    Some(e) => {
                        audio.play_voice(&e.path, e.volume * volume, pitch as f64, pan, false);
                    }
                    None => log::debug!("sfx: no sample for sound {sound:#06x}"),
                },
            }
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
