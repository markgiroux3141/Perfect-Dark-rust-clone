//! `lang_get` (`lang.c`): PD's text ids resolved against the English banks
//! (`assets/lang/en.json`, every bank of `include/lang.h`).
//!
//! PD packs a text id as `(bank << 9) | index` (`lang.h`), `bank` a
//! `LANGBANK_*` number, so `L_MPMENU_017` is `(0x28 << 9) | 17`. The menus add
//! small offsets to an id (`L_MISC_082 + i`, `L_OPTIONS_008 + team`), which
//! [`Tx::add`] does on the index.
//!
//! The ROM's MP strings (preset and challenge names, `lang/mpstringsE.bin`) are
//! read with the MP configs that index them (M2).
//!
//! Source: the old repo's `pd_menu/lang.rs`, which numbered banks in its own
//! generated order; the numbers here are PD's.

use std::collections::HashMap;

use serde::Deserialize;

use crate::assets::AssetDir;

/// `LANGBANK_*` (`include/lang.h`) for the banks the Combat Simulator reads.
/// The rest (one per stage) are looked up by name with [`Lang::bank`].
pub const LANGBANK_GUN: u8 = 0x26;
pub const LANGBANK_TITLE: u8 = 0x27;
pub const LANGBANK_MPMENU: u8 = 0x28;
pub const LANGBANK_PROPOBJ: u8 = 0x29;
pub const LANGBANK_MPWEAPONS: u8 = 0x2a;
pub const LANGBANK_OPTIONS: u8 = 0x2b;
pub const LANGBANK_MISC: u8 = 0x2c;

/// A text id: `L_<BANK>_<index>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Tx {
    pub bank: u8,
    pub index: u16,
}

pub const fn tx(bank: u8, index: u16) -> Tx {
    Tx { bank, index }
}

impl Tx {
    /// A packed 16-bit text id.
    pub const fn from_raw(id: u16) -> Tx {
        Tx { bank: (id >> 9) as u8, index: id & 0x1ff }
    }

    /// The packed 16-bit text id.
    pub const fn raw(self) -> u16 {
        ((self.bank as u16) << 9) | self.index
    }

    /// `id + n`, as PD does with consecutive text ids.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, n: i32) -> Tx {
        Tx { bank: self.bank, index: (self.index as i32 + n).max(0) as u16 }
    }
}

#[derive(Deserialize)]
struct BankFile {
    langbank: u8,
    strings: Vec<String>,
}

#[derive(Deserialize)]
struct LangFile {
    banks: HashMap<String, BankFile>,
}

/// One language's banks.
pub struct Lang {
    banks: Vec<Vec<String>>,
    names: HashMap<String, u8>,
}

impl Lang {
    /// `lang/<lang>.json` (only `en` is exported).
    pub fn load(assets: &AssetDir, lang: &str) -> Result<Lang, String> {
        let f: LangFile = assets.read_json(&assets.lang(lang))?;
        let mut banks = vec![Vec::new(); 128];
        let mut names = HashMap::new();
        for (name, b) in f.banks {
            names.insert(name, b.langbank);
            banks[b.langbank as usize] = b.strings;
        }
        Ok(Lang { banks, names })
    }

    /// `lang_get(id)`: the string, or empty for an id no bank has.
    pub fn get(&self, t: Tx) -> &str {
        self.banks.get(t.bank as usize).and_then(|b| b.get(t.index as usize)).map_or("", String::as_str)
    }

    /// A bank's `LANGBANK_*` number by its name (`"ref"`, `"mp5"`, `"mpmenu"`).
    pub fn bank(&self, name: &str) -> Option<u8> {
        self.names.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_resolve_by_pds_text_ids() {
        let lang = Lang::load(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR")), "en").unwrap();
        // From the menu spike's test: L_MISC_445 and L_MPMENU_017.
        assert_eq!(lang.get(tx(LANGBANK_MISC, 445)).trim(), "Combat Simulator");
        assert_eq!(lang.get(tx(LANGBANK_MPMENU, 17)).trim(), "Game Setup");
        assert_eq!(lang.get(Tx::from_raw((0x28 << 9) | 17)).trim(), "Game Setup");
        assert_eq!(tx(LANGBANK_MPMENU, 17).raw(), 0x5011);
        assert_eq!(lang.bank("ref"), Some(0x1a));
        assert_eq!(lang.get(tx(0x7f, 0)), "");
    }
}
