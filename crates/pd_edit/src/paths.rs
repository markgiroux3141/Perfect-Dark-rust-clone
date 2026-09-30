//! Where the editor reads and writes: the repo's `assets/`, the custom tree
//! (`custom/`, or `$PD_CUSTOM`), and the recipes and layouts
//! (`crates/pd_import/levels/`).

use std::path::{Path, PathBuf};

use pd_core::assets::AssetDir;

#[derive(Clone, Debug)]
pub struct EditPaths {
    pub assets: PathBuf,
    pub custom: PathBuf,
    pub levels: PathBuf,
}

/// A recipe in the levels directory.
#[derive(Clone, Debug)]
pub struct RecipeEntry {
    pub code: String,
    pub name: String,
    pub path: PathBuf,
}

impl EditPaths {
    /// From the asset root found as the game finds it (`PD_ASSETS`, else
    /// `assets/` beside the executable or above it).
    pub fn discover() -> Result<EditPaths, String> {
        let root = engine::assets::AssetRoot::discover("PD_ASSETS", "assets", "MANIFEST.json")?;
        let assets = root.root().to_path_buf();
        let repo = assets.parent().map(Path::to_path_buf).unwrap_or_default();
        let custom = std::env::var_os("PD_CUSTOM").map_or_else(|| repo.join("custom"), PathBuf::from);
        Ok(EditPaths { assets, custom, levels: repo.join("crates").join("pd_import").join("levels") })
    }

    pub fn import_paths(&self) -> pd_import::Paths {
        pd_import::Paths { assets: self.assets.clone(), custom: self.custom.clone(), src: None }
    }

    /// `assets/` with the custom tree over it, as the game loads a stage.
    pub fn asset_dir(&self) -> AssetDir {
        AssetDir::new(&self.assets).with_custom_dir(&self.custom)
    }

    pub fn stage_dir(&self, code: &str) -> PathBuf {
        self.custom.join("stages").join(code)
    }

    /// Whether the level has been imported by an importer that keeps its
    /// source's data (so it can be placed again without importing).
    pub fn is_imported(&self, code: &str) -> bool {
        let d = self.stage_dir(code);
        ["bg.json", "tiles.json", "pads.json", "setup.json", "source.json"].iter().all(|f| d.join(f).exists())
    }

    /// Every recipe in the levels directory, by name.
    pub fn recipes(&self) -> Vec<RecipeEntry> {
        let mut out: Vec<RecipeEntry> = std::fs::read_dir(&self.levels)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json") && !p.to_string_lossy().ends_with(".layout.json"))
            .filter_map(|p| {
                let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
                Some(RecipeEntry { code: v["code"].as_str()?.to_owned(), name: v["name"].as_str().unwrap_or("").to_owned(), path: p })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}
