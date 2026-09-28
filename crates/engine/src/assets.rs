//! Run-time asset discovery. [`AssetRoot::discover`] checks, in order: an
//! environment variable, then a directory of the given name beside the executable
//! and in each directory above it (so `target/release/x.exe` finds the repo's
//! `assets/`), then the current directory and its parents. A directory counts when
//! it holds the given marker file. Readers for bytes, UTF-8, JSON and PNG. No
//! caching policy here: a game builds its own caches keyed by its own ids.

use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct AssetRoot {
    root: PathBuf,
}

impl AssetRoot {
    pub fn new(root: impl Into<PathBuf>) -> AssetRoot {
        AssetRoot { root: root.into() }
    }

    /// Find the asset root: `$env_var`, else `<dir>/<name>` holding `marker` for
    /// `dir` the executable's directory or any above it, else the same from the
    /// current directory.
    pub fn discover(env_var: &str, name: &str, marker: &str) -> Result<AssetRoot, String> {
        if let Some(p) = std::env::var_os(env_var) {
            let p = PathBuf::from(p);
            return if p.join(marker).exists() { Ok(AssetRoot::new(p)) } else { Err(format!("{env_var}={} has no {marker}", p.display())) };
        }
        let starts = [std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)), std::env::current_dir().ok()];
        for start in starts.into_iter().flatten() {
            for dir in start.ancestors() {
                let cand = dir.join(name);
                if cand.join(marker).exists() {
                    return Ok(AssetRoot::new(cand));
                }
            }
        }
        Err(format!("no {name}/ with {marker} beside the executable or above it; set {env_var}"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>, String> {
        let p = self.path(rel);
        std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn read_string(&self, rel: &str) -> Result<String, String> {
        let p = self.path(rel);
        std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn read_json(&self, rel: &str) -> Result<Value, String> {
        serde_json::from_str(&self.read_string(rel)?).map_err(|e| format!("{rel}: {e}"))
    }

    /// Decode a PNG to `(width, height, RGBA8)`.
    pub fn read_png(&self, rel: &str) -> Result<(u32, u32, Vec<u8>), String> {
        let p = self.path(rel);
        let img = image::open(&p).map_err(|e| format!("{}: {e}", p.display()))?.to_rgba8();
        Ok((img.width(), img.height(), img.into_raw()))
    }
}
