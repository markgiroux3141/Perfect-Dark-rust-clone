//! Texture themes for generated levels: another arena's textures on the
//! kit's roles, the geometry as it is ([`crate::autolevel`]'s `materials`).
//! A theme is a file, `crates/pd_import/themes/<name>.json` (beside the
//! recipes' folder, as the arena sets are), drafted from an arena's own use
//! of its textures by `tools/autolevel-themes/draft_theme.py` and tuned by
//! hand; a level's recipe names it (`"theme": "temple"`), so a theme tuned
//! dresses every level that names it when they are imported again.
//!
//! ```json
//! { "format": "pd-import-theme/1", "name": "Temple", "stage": "jun",
//!   "roles": { "floor": "01B9", "plain": "01B4", "panel": "cryp:018D" } }
//! ```

use std::path::{Path, PathBuf};

use crate::recipe::{Theme, ThemeRef};

pub const FORMAT: &str = "pd-import-theme/1";

/// `crates/pd_import/themes/`, beside the recipes' folder `levels` (or the
/// crate's own, with none: tests, scratch imports).
pub fn dir(levels: Option<&Path>) -> PathBuf {
    match levels {
        Some(l) => l.parent().unwrap_or(l).join("themes"),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("themes"),
    }
}

/// Is `name` a theme file's name (a-z, 0-9, _ and -)?
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The theme `<dir>/<name>.json`.
pub fn load(dir: &Path, name: &str) -> Result<Theme, String> {
    if !plain_name(name) {
        return Err(format!("{name:?} is no theme's name (a-z, 0-9, _ and -)"));
    }
    let path = dir.join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("theme {name}: {}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if v["format"].as_str() != Some(FORMAT) {
        return Err(format!("{}: not a theme ({FORMAT})", path.display()));
    }
    let mut t: Theme = serde_json::from_value(v).map_err(|e| format!("{}: {e}", path.display()))?;
    if t.name.trim().is_empty() {
        t.name = name.to_owned();
    }
    Ok(t)
}

/// What a recipe's theme is: the file it names, read; or the one written in it.
pub fn resolve(dir: &Path, theme: &ThemeRef) -> Result<Theme, String> {
    match theme {
        ThemeRef::Named(name) => load(dir, name),
        ThemeRef::Inline(t) => Ok(t.clone()),
    }
}

/// Every theme in `dir`, by its file's name, sorted by what it is called.
/// (A file that is not one is left out.)
pub fn list(dir: &Path) -> Vec<(String, Theme)> {
    let mut out: Vec<(String, Theme)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let stem = path.file_stem()?.to_str()?.to_owned();
            (path.extension()? == "json").then_some(())?;
            load(dir, &stem).ok().map(|t| (stem, t))
        })
        .collect();
    out.sort_by(|a, b| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()).then(a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_is_read_by_its_name_and_listed() {
        let dir = std::env::temp_dir().join(format!("pd_import_themes_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("temple.json"), r#"{"format": "pd-import-theme/1", "name": "Temple", "stage": "jun", "roles": {"floor": "01B9"}}"#).unwrap();
        std::fs::write(dir.join("bare.json"), r#"{"format": "pd-import-theme/1", "stage": "mp1"}"#).unwrap();
        std::fs::write(dir.join("other.json"), r#"{"format": "pd-import-recipe/1"}"#).unwrap();
        let t = load(&dir, "temple").unwrap();
        assert_eq!((t.name.as_str(), t.stage.as_str(), t.roles.get("floor").map(String::as_str)), ("Temple", "jun", Some("01B9")));
        // a file with no name is called by its own
        assert_eq!(load(&dir, "bare").unwrap().name, "bare");
        assert!(load(&dir, "other").unwrap_err().contains("not a theme"));
        assert!(load(&dir, "nothing").is_err());
        assert!(load(&dir, "../temple").unwrap_err().contains("no theme's name"));
        let names: Vec<String> = list(&dir).into_iter().map(|(k, _)| k).collect();
        assert_eq!(names, ["bare", "temple"]);
        // a recipe's theme: a name, or the theme itself
        assert_eq!(resolve(&dir, &ThemeRef::Named("temple".into())).unwrap(), t);
        assert_eq!(resolve(&dir, &ThemeRef::Inline(t.clone())).unwrap(), t);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every theme committed is read, names an arena there is, and only the kit's roles.
    #[test]
    fn the_committed_themes_are_whole() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets");
        let roles: Vec<&str> = autolevel_build::complex::PALETTE.iter().map(|p| p.role).collect();
        let themes = list(&dir(None));
        assert!(!themes.is_empty());
        for (file, t) in themes {
            assert!(assets.join("stages").join(&t.stage).join("bg.json").exists(), "{file}: no stage {}", t.stage);
            for role in t.roles.keys() {
                assert!(roles.contains(&role.as_str()), "{file}: {role} is none of the kit's roles");
            }
        }
    }
}
