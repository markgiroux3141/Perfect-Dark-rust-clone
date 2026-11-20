//! The Game Pak's EEPROM, kept in a file between runs.
//!
//! SUBST: PD keeps its save files on the cartridge's 16 Kbit EEPROM / its
//! 2 KB (`pd_core::pak::Paks::eeprom`) are read from a file at power on and
//! written back whenever the game writes the chip. The file holds the chip's
//! bytes in order, as N64 emulators keep a 16 Kbit EEPROM (`.eep`), so a
//! save from one loads here. `$PD_SAVE` names it; else it is
//! `save/perfect_dark.eep` beside the assets directory.
//!
//! Beside it, `<name>.custom.json` keeps the custom arena of each setup file
//! saved on one (`pd_menu::files::CustomSetups`): not PD's, so the EEPROM's
//! bytes stay as PD would write them.

use std::path::{Path, PathBuf};

use pd_core::pak::EEPROM_SIZE;
use pd_menu::files::CustomSetups;

/// Where the EEPROM image lives: `$PD_SAVE`, else `save/perfect_dark.eep`
/// next to `assets_root`.
pub fn locate(assets_root: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("PD_SAVE") {
        return PathBuf::from(p);
    }
    let base = assets_root.parent().unwrap_or(assets_root);
    base.join("save").join("perfect_dark.eep")
}

/// The image at `path`, or `None` (a blank chip) when there is none yet or
/// it isn't 2 KB.
pub fn load(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(b) if b.len() == EEPROM_SIZE => Some(b),
        Ok(b) => {
            log::warn!("save: {} is {} bytes, not a 16 Kbit EEPROM's {EEPROM_SIZE}; starting from a blank Game Pak", path.display(), b.len());
            None
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            log::warn!("save: {}: {e}; starting from a blank Game Pak", path.display());
            None
        }
    }
}

/// Where the setup files' custom arenas are kept, beside the image at
/// `eeprom`: `perfect_dark.custom.json`.
pub fn custom_setups_path(eeprom: &Path) -> PathBuf {
    eeprom.with_extension("custom.json")
}

/// The custom arenas kept beside the image at `eeprom` (none when there is
/// no file; a malformed one is logged and ignored).
pub fn load_custom_setups(eeprom: &Path) -> CustomSetups {
    let path = custom_setups_path(eeprom);
    match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
            log::warn!("save: {}: {e}; setups saved on custom arenas open on mp_init's", path.display());
            CustomSetups::default()
        }),
        Err(_) => CustomSetups::default(),
    }
}

/// Write the custom arenas beside the image at `eeprom` (as [`store`] does).
pub fn store_custom_setups(eeprom: &Path, setups: &CustomSetups) -> Result<(), String> {
    let path = custom_setups_path(eeprom);
    let text = serde_json::to_string_pretty(setups).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write the image to `path` (through a temporary file, so a crash can't
/// leave half of it).
pub fn store(path: &Path, eeprom: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("eep.tmp");
    std::fs::write(&tmp, eeprom).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_round_trips_and_a_wrong_size_is_blank() {
        let dir = std::env::temp_dir().join(format!("pd_save_test_{}", std::process::id()));
        let path = dir.join("save").join("perfect_dark.eep");
        assert_eq!(load(&path), None);
        let image: Vec<u8> = (0..EEPROM_SIZE).map(|i| (i * 7) as u8).collect();
        store(&path, &image).unwrap();
        assert_eq!(load(&path), Some(image));
        std::fs::write(&path, [1, 2, 3]).unwrap();
        assert_eq!(load(&path), None);
        // the custom arenas beside it
        assert_eq!(custom_setups_path(&path), dir.join("save").join("perfect_dark.custom.json"));
        assert_eq!(load_custom_setups(&path), CustomSetups::default());
        let mut setups = CustomSetups::default();
        setups.set(pd_core::savebuffer::FileGuid { fileid: 0x12, deviceserial: 0xbeef }, Some("kokiri"));
        store_custom_setups(&path, &setups).unwrap();
        assert_eq!(load_custom_setups(&path).code(pd_core::savebuffer::FileGuid { fileid: 0x12, deviceserial: 0xbeef }), Some("kokiri"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
