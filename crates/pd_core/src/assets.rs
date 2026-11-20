//! The asset layout, in one place: every path under `assets/` that the game reads
//! is built here from ids (texture number, model stem, animation number, sfx id,
//! stage code, ...). Loaders take an [`AssetDir`] (a root the engine found, or a
//! test's `CARGO_MANIFEST_DIR`) so this crate never guesses where the repo is.
//! The layout is docs/ARCHITECTURE.md § Assets; `tools/pd-assets/build_assets.py`
//! writes it.
//!
//! **Custom levels** live in a second tree with the same layout, laid over
//! `assets/` ([`AssetDir::with_custom_levels`]): `custom/levels.json` lists
//! them ([`CustomLevel`]) and `custom/stages/<code>/` holds each one's four
//! stage files, exactly as an arena's. A level's own models (GoldenEye's
//! doors) are `custom/models/<stem>.json + .bin`, listed in
//! `custom/models/index.json` beside `assets/`' index, with `MODEL_*` numbers
//! from [`CUSTOM_MODELNUMS`]. `pd_import` writes it; it is never committed (a
//! level converted from another game is that game's data).

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Deserialize;

/// The root of a generated `assets/` tree, and optionally the custom-levels
/// tree laid over it.
#[derive(Clone, Debug)]
pub struct AssetDir {
    root: PathBuf,
    /// `custom/`: its files win over `assets/`'s at the same relative path.
    custom: Option<PathBuf>,
}

/// The stage numbers custom levels are handed at run time, in
/// `custom/levels.json`'s order: past PD's last (`STAGE_4MBMENU`, 0x5d), up
/// to what a stage number's byte holds, so 160 custom arenas are listed. A
/// custom level is known by its code everywhere it is kept (its recipe,
/// `levels.json`, a saved MP setup: `pd_menu::files`), never by its number,
/// which another run may give another level.
///
/// SUBST: PD's stage numbers are fixed, and an MP setup file keeps one in 7
/// bits / custom levels' numbers are given as the game starts, past 0x7f too,
/// so more arenas than 7 bits allow can be listed (a saved setup on one keeps
/// its code beside the file instead).
pub const CUSTOM_STAGENUMS: std::ops::RangeInclusive<u8> = 0x60..=0xff;

/// The `MODEL_*` numbers custom models take: past PD's last (`MODEL_*` runs to
/// 0x1bd), so a setup row names one as it names PD's.
pub const CUSTOM_MODELNUMS: std::ops::RangeInclusive<i32> = 0x1000..=0x1fff;

/// One entry of `custom/levels.json`: a converted level, playable as an arena.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CustomLevel {
    /// Its directory, `custom/stages/<code>/`.
    pub code: String,
    /// Its `STAGE_*` number for this run, from [`CUSTOM_STAGENUMS`] in the
    /// file's order ([`AssetDir::custom_levels`]). A row's own `stagenum`
    /// (written before 2026-11-20) is ignored.
    #[serde(skip)]
    pub stagenum: u8,
    /// The arena menu's name for it.
    pub name: String,
    /// The arena menu's group it is listed in ("Jedi Academy"); empty for
    /// the "Custom" group.
    #[serde(default)]
    pub group: String,
}

/// The custom levels in the arena menu's order: the "Custom" group's (no
/// group) first, then each named group's by name; by stage number (the
/// file's order) within.
pub fn sort_custom_levels(levels: &mut [CustomLevel]) {
    levels.sort_by(|a, b| (!a.group.is_empty(), &a.group, a.stagenum).cmp(&(!b.group.is_empty(), &b.group, b.stagenum)));
}

/// The levels in order, numbered from [`CUSTOM_STAGENUMS`]: a code already
/// listed, a code that is one of PD's, and those past the last number are
/// left out (logged).
fn number_custom_levels(levels: Vec<CustomLevel>, path: &Path) -> Vec<CustomLevel> {
    let mut out: Vec<CustomLevel> = Vec::with_capacity(levels.len());
    let mut numbers = CUSTOM_STAGENUMS;
    for mut l in levels {
        if out.iter().any(|o| o.code == l.code) || crate::ids::STAGE_CODES.iter().any(|(_, c)| *c == l.code) {
            log::warn!("{}: {:?} is listed twice or is one of PD's stages; left out", path.display(), l.code);
            continue;
        }
        let Some(n) = numbers.next() else {
            log::warn!("{}: more than {} custom levels; {:?} and those after it are left out", path.display(), CUSTOM_STAGENUMS.count(), l.code);
            break;
        };
        l.stagenum = n;
        out.push(l);
    }
    out
}

#[derive(Deserialize)]
struct CustomLevelsFile {
    format: String,
    levels: Vec<CustomLevel>,
}

impl AssetDir {
    pub fn new(root: impl Into<PathBuf>) -> AssetDir {
        AssetDir { root: root.into(), custom: None }
    }

    /// With the custom levels in `$PD_CUSTOM`, else in `custom/` beside the
    /// asset root, when that exists.
    pub fn with_custom_levels(self) -> AssetDir {
        let dir = std::env::var_os("PD_CUSTOM").map(PathBuf::from).or_else(|| self.root.parent().map(|p| p.join("custom")));
        match dir {
            Some(d) if d.is_dir() => self.with_custom_dir(d),
            _ => self,
        }
    }

    /// With the custom levels in `dir`.
    pub fn with_custom_dir(mut self, dir: impl Into<PathBuf>) -> AssetDir {
        self.custom = Some(dir.into());
        self
    }

    /// The custom-levels tree, if one is laid over the assets.
    pub fn custom_dir(&self) -> Option<&Path> {
        self.custom.as_deref()
    }

    /// `custom/levels.json`'s levels (none without a custom tree), each handed
    /// its stage number in the file's order. A level past the last number, or
    /// a code listed twice, is logged and left out; a malformed index is
    /// logged and ignored: the arenas still play.
    pub fn custom_levels(&self) -> Vec<CustomLevel> {
        let Some(dir) = &self.custom else { return Vec::new() };
        let path = dir.join("levels.json");
        if !path.exists() {
            return Vec::new();
        }
        match self.read_json::<CustomLevelsFile>(&path) {
            Ok(f) if f.format == "pd-custom-levels/1" => number_custom_levels(f.levels, &path),
            Ok(f) => {
                log::warn!("{}: format {:?}, expected pd-custom-levels/1", path.display(), f.format);
                Vec::new()
            }
            Err(e) => {
                log::warn!("{e}");
                Vec::new()
            }
        }
    }

    /// A stage's code: PD's (`ids::stage_code`), else a custom level's.
    pub fn stage_code(&self, stagenum: u8) -> Option<String> {
        crate::ids::stage_code(stagenum).map(str::to_owned).or_else(|| self.custom_levels().into_iter().find(|l| l.stagenum == stagenum).map(|l| l.code))
    }

    /// A stage's number from its code: PD's (`ids::STAGE_CODES`), else the
    /// one a custom level is handed this run.
    pub fn stage_num(&self, code: &str) -> Option<u8> {
        crate::ids::STAGE_CODES.iter().find(|(_, c)| *c == code).map(|(n, _)| *n).or_else(|| self.custom_levels().into_iter().find(|l| l.code == code).map(|l| l.stagenum))
    }

    /// `rel` in the custom tree if it is there, else under the root.
    fn overlaid(&self, rel: &Path) -> PathBuf {
        if let Some(c) = &self.custom {
            let p = c.join(rel);
            if p.exists() {
                return p;
            }
        }
        self.root.join(rel)
    }

    /// The repo's `assets/` for a crate at `crates/<name>`: pass
    /// `env!("CARGO_MANIFEST_DIR")`. For tests and tools.
    pub fn from_manifest_dir(manifest_dir: &str) -> AssetDir {
        AssetDir::new(Path::new(manifest_dir).join("..").join("..").join("assets"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A path relative to the root, as the asset files themselves store them
    /// (e.g. a model's texture `file`); a custom level's own files are found
    /// in the custom tree.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.overlaid(Path::new(rel))
    }

    /// `textures/<num>.png`: the global pool, by PD texture number.
    pub fn texture(&self, num: u16) -> PathBuf {
        self.root.join("textures").join(format!("{num:04x}.png"))
    }

    pub fn texture_index(&self) -> PathBuf {
        self.root.join("textures").join("index.json")
    }

    /// `models/<stem>.json`: a model's header (a custom level's own models
    /// are in the custom tree).
    pub fn model_json(&self, stem: &str) -> PathBuf {
        self.overlaid(&Path::new("models").join(format!("{stem}.json")))
    }

    /// `models/<stem>.bin`: a model's vertices and indices.
    pub fn model_bin(&self, stem: &str) -> PathBuf {
        self.overlaid(&Path::new("models").join(format!("{stem}.bin")))
    }

    pub fn model_index(&self) -> PathBuf {
        self.root.join("models").join("index.json")
    }

    /// `custom/models/index.json`, the custom levels' models, if there is one.
    pub fn custom_model_index(&self) -> Option<PathBuf> {
        self.custom.as_ref().map(|c| c.join("models").join("index.json")).filter(|p| p.exists())
    }

    /// `anims/<num>.bin`: one animation of the bank, raw.
    pub fn anim(&self, num: u16) -> PathBuf {
        self.root.join("anims").join(format!("{num:04x}.bin"))
    }

    pub fn anim_index(&self) -> PathBuf {
        self.root.join("anims").join("index.json")
    }

    /// `fonts/<name>.bin` (e.g. `handelgothicsm`, `numeric`).
    pub fn font(&self, name: &str) -> PathBuf {
        self.root.join("fonts").join(format!("{name}.bin"))
    }

    /// `lang/<lang>.json` (only `en` is exported).
    pub fn lang(&self, lang: &str) -> PathBuf {
        self.root.join("lang").join(format!("{lang}.json"))
    }

    /// `lang/mpstringsE.bin`: the ROM's MP strings (preset and challenge text).
    pub fn mpstrings(&self) -> PathBuf {
        self.root.join("lang").join("mpstringsE.bin")
    }

    /// `sfx/<num>.wav`, by bank sound number.
    pub fn sfx(&self, num: u16) -> PathBuf {
        self.root.join("sfx").join(format!("{num:04x}.wav"))
    }

    pub fn sfx_manifest(&self) -> PathBuf {
        self.root.join("sfx").join("manifest.json")
    }

    /// `music/<name>`: `bank.json`, `seq.tbl`, `index.json`, and the
    /// sequences `seq/<num>.seq` (as `index.json` names them).
    pub fn music(&self, name: &str) -> PathBuf {
        self.root.join("music").join(name)
    }

    /// `data/<name>` (`weapons.json`, `bodies.json`, `mpconfigs.bin`).
    pub fn data(&self, name: &str) -> PathBuf {
        self.root.join("data").join(name)
    }

    /// `stages/<code>/`, by PD's stage code (`ref` is Complex), or a custom
    /// level's.
    pub fn stage(&self, code: &str) -> PathBuf {
        self.overlaid(&Path::new("stages").join(code))
    }

    /// Read a file, with its path in the error.
    pub fn read(&self, path: &Path) -> Result<Vec<u8>, String> {
        std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Read and parse a JSON file, with its path in the error.
    pub fn read_json<T: DeserializeOwned>(&self, path: &Path) -> Result<T, String> {
        let bytes = self.read(path)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Decode a PNG to `(width, height, RGBA8)`.
    pub fn read_png(&self, path: &Path) -> Result<(usize, usize, Vec<u8>), String> {
        let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
        Ok((img.width() as usize, img.height() as usize, img.into_raw()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A custom tree lays its stages and files over the assets, lists its
    /// levels numbered in the file's order (a row's own number ignored; a
    /// code twice, or one of PD's, left out), and resolves numbers and codes
    /// both ways; PD's codes win.
    #[test]
    fn custom_levels_are_laid_over_the_assets() {
        let dir = std::env::temp_dir().join(format!("pd_custom_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("stages").join("kok")).unwrap();
        std::fs::write(
            dir.join("levels.json"),
            r#"{"format":"pd-custom-levels/1","levels":[{"code":"kok","stagenum":127,"name":"Kok"},{"code":"ref","name":"Clash"},{"code":"two","stagenum":31,"name":"Two","group":"G"},{"code":"kok","name":"Again"}]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("stages").join("kok").join("tex.png"), b"x").unwrap();
        let a = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR")).with_custom_dir(&dir);
        let level = |code: &str, stagenum: u8, name: &str, group: &str| CustomLevel { code: code.into(), stagenum, name: name.into(), group: group.into() };
        assert_eq!(a.custom_levels(), vec![level("kok", 0x60, "Kok", ""), level("two", 0x61, "Two", "G")]);
        assert_eq!(a.stage("kok"), dir.join("stages").join("kok"));
        assert_eq!(a.path("stages/kok/tex.png"), dir.join("stages/kok/tex.png"));
        assert_eq!(a.stage("ref"), a.root().join("stages").join("ref"));
        assert_eq!((a.stage_code(0x60).as_deref(), a.stage_code(0x61).as_deref(), a.stage_code(0x62)), (Some("kok"), Some("two"), None));
        assert_eq!(a.stage_code(crate::ids::STAGE_MP_COMPLEX).as_deref(), Some("ref"));
        assert_eq!((a.stage_num("two"), a.stage_num("ref"), a.stage_num("gone")), (Some(0x61), Some(crate::ids::STAGE_MP_COMPLEX), None));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 160 custom levels are listed, numbered 0x60 to 0xff; the next is left out.
    #[test]
    fn custom_levels_run_to_the_last_stage_number() {
        let rows: Vec<CustomLevel> = (0..161).map(|i| CustomLevel { code: format!("l{i}"), stagenum: 0, name: String::new(), group: String::new() }).collect();
        let listed = number_custom_levels(rows, Path::new("levels.json"));
        assert_eq!((listed.len(), listed[0].stagenum, listed[159].stagenum, listed[159].code.as_str()), (160, 0x60, 0xff, "l159"));
    }

    /// A custom tree's models are listed beside PD's: one with a custom
    /// `MODEL_*` number is found by it and loads from the custom tree; one
    /// named as a PD model, or numbered as one, is refused.
    #[test]
    fn custom_models_join_the_model_store() {
        let dir = std::env::temp_dir().join(format!("pd_custom_models_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let a = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
        // A PD model's files, under a custom stem.
        std::fs::copy(a.model_json("tdoor"), dir.join("models").join("ge_door.json")).unwrap();
        std::fs::copy(a.model_bin("tdoor"), dir.join("models").join("ge_door.bin")).unwrap();
        let row = |modelnum: i32| format!(r#"{{"filenum":{},"file":"GE_X","kind":"prop","source":"test","tris":12,"modelnum":{modelnum},"statescale":4096}}"#, 0x10000 + modelnum);
        std::fs::write(
            dir.join("models").join("index.json"),
            format!(r#"{{"ge_door":{},"tdoor":{},"ge_bad":{}}}"#, row(0x109b), row(0x109c), row(195)),
        )
        .unwrap();
        let a = a.with_custom_dir(&dir);
        let store = crate::model::ModelStore::load(&a).unwrap();
        assert_eq!(store.stem_of_modelnum(0x109b), Some("ge_door"));
        assert_eq!(a.model_json("ge_door"), dir.join("models").join("ge_door.json"));
        assert!(store.get("ge_door").is_ok());
        assert_eq!(store.stem_of_modelnum(0x109c), None, "a PD stem is not replaced");
        assert_eq!(store.stem_of_modelnum(195), Some("tdoor"), "a PD MODEL number stays PD's");
        assert!(!store.index.contains_key("ge_bad"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
