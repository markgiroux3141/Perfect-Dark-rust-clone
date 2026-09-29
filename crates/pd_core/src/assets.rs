//! The asset layout, in one place: every path under `assets/` that the game reads
//! is built here from ids (texture number, model stem, animation number, sfx id,
//! stage code, ...). Loaders take an [`AssetDir`] (a root the engine found, or a
//! test's `CARGO_MANIFEST_DIR`) so this crate never guesses where the repo is.
//! The layout is docs/ARCHITECTURE.md § Assets; `tools/pd-assets/build_assets.py`
//! writes it.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// The root of a generated `assets/` tree.
#[derive(Clone, Debug)]
pub struct AssetDir {
    root: PathBuf,
}

impl AssetDir {
    pub fn new(root: impl Into<PathBuf>) -> AssetDir {
        AssetDir { root: root.into() }
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
    /// (e.g. a model's texture `file`).
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// `textures/<num>.png`: the global pool, by PD texture number.
    pub fn texture(&self, num: u16) -> PathBuf {
        self.root.join("textures").join(format!("{num:04x}.png"))
    }

    pub fn texture_index(&self) -> PathBuf {
        self.root.join("textures").join("index.json")
    }

    /// `models/<stem>.json`: a model's header.
    pub fn model_json(&self, stem: &str) -> PathBuf {
        self.root.join("models").join(format!("{stem}.json"))
    }

    /// `models/<stem>.bin`: a model's vertices and indices.
    pub fn model_bin(&self, stem: &str) -> PathBuf {
        self.root.join("models").join(format!("{stem}.bin"))
    }

    pub fn model_index(&self) -> PathBuf {
        self.root.join("models").join("index.json")
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

    /// `stages/<code>/`, by PD's stage code (`ref` is Complex).
    pub fn stage(&self, code: &str) -> PathBuf {
        self.root.join("stages").join(code)
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
