//! Arena sets: which custom arenas the game lists. A set is a committed file,
//! `crates/pd_import/sets/<stem>.json` (beside the recipes' `levels/`): its
//! name and its arenas' codes in the arena menu's order. An arena may be in
//! any number of sets, or none (it stays on disk, out of the game).
//!
//! One set is active on a machine (`custom/arena_set.json`, gitignored like
//! the rest of `custom/`), and the game's list, `custom/levels.json`, is
//! written from it ([`write_levels`]): each of its arenas that is built, with
//! its recipe's name and group. A finished import joins the active set
//! (`write::register`). The editor's Home picks the set, makes new ones, and
//! takes arenas out of a set or deletes them ([`delete_arena`]).
//!
//! The first time there are no sets, [`ensure`] makes "Main" from the list
//! `levels.json` had, so nothing the game showed is lost.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const FORMAT: &str = "pd-arena-set/1";

/// One set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArenaSet {
    pub format: String,
    /// What Home calls it.
    pub name: String,
    /// The arenas' codes, in the menu's order.
    pub arenas: Vec<String>,
}

impl ArenaSet {
    pub fn new(name: &str) -> ArenaSet {
        ArenaSet { format: FORMAT.into(), name: name.into(), arenas: Vec::new() }
    }
}

/// `crates/pd_import/sets/`, beside the recipes' folder `levels`.
pub fn dir_for(levels: &Path) -> PathBuf {
    levels.parent().unwrap_or(levels).join("sets")
}

/// `custom/arena_set.json`: which set is active on this machine.
fn active_path(custom: &Path) -> PathBuf {
    custom.join("arena_set.json")
}

/// Every set in `dir`, by its file's stem, by name.
pub fn list(dir: &Path) -> Vec<(String, ArenaSet)> {
    let mut out: Vec<(String, ArenaSet)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), load(&p).ok()?)))
        .collect();
    out.sort_by(|a, b| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()));
    out
}

pub fn load(path: &Path) -> Result<ArenaSet, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let s: ArenaSet = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if s.format != FORMAT {
        return Err(format!("{}: format {:?}, expected {FORMAT}", path.display(), s.format));
    }
    Ok(s)
}

pub fn save(dir: &Path, stem: &str, set: &ArenaSet) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{stem}.json"));
    let text = serde_json::to_string_pretty(set).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The active set's stem, if one is named and its file is there.
pub fn active(dir: &Path, custom: &Path) -> Option<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(active_path(custom)).ok()?).ok()?;
    let stem = v["set"].as_str()?.to_owned();
    dir.join(format!("{stem}.json")).exists().then_some(stem)
}

/// Make `stem` the active set, and write the game's list from it.
pub fn set_active(dir: &Path, custom: &Path, levels: &Path, stem: &str) -> Result<(), String> {
    crate::write::write_json(&active_path(custom), &json!({"format": "pd-arena-active/1", "set": stem}))?;
    write_levels(dir, custom, levels)
}

/// Sets ready to use: with none yet, "Main" made from the arenas
/// `levels.json` lists (in its order); with no active one, the first by name
/// made active. Returns the active set's stem.
pub fn ensure(dir: &Path, custom: &Path, levels: &Path) -> Result<String, String> {
    if let Some(stem) = active(dir, custom) {
        return Ok(stem);
    }
    let sets = list(dir);
    let stem = match sets.first() {
        Some((stem, _)) => stem.clone(),
        None => {
            let mut main = ArenaSet::new("Main");
            main.arenas = listed(custom).into_iter().map(|row| row.0).collect();
            save(dir, "main", &main)?;
            "main".to_owned()
        }
    };
    set_active(dir, custom, levels, &stem)?;
    Ok(stem)
}

/// `levels.json`'s rows as they are: (code, name, group).
fn listed(custom: &Path) -> Vec<(String, String, String)> {
    let Ok(text) = std::fs::read_to_string(custom.join("levels.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    v["levels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l["code"].as_str()?.to_owned(), l["name"].as_str().unwrap_or_default().to_owned(), l["group"].as_str().unwrap_or_default().to_owned())))
        .collect()
}

/// An arena's recipe: `levels/<code>.json`, else a shelved one.
pub fn recipe_path(levels: &Path, code: &str) -> Option<PathBuf> {
    [levels.join(format!("{code}.json")), levels.join("shelved").join(format!("{code}.json"))].into_iter().find(|p| p.exists())
}

/// Is the arena built (its stage in the custom tree)?
fn built(custom: &Path, code: &str) -> bool {
    custom.join("stages").join(code).join("bg.json").exists()
}

/// Write the game's list, `custom/levels.json`, from the active set: each of
/// its arenas that is built, in the set's order, named and grouped as its
/// recipe says (as the list had it, for one with no recipe).
pub fn write_levels(dir: &Path, custom: &Path, levels: &Path) -> Result<(), String> {
    let stem = active(dir, custom).ok_or("no active arena set")?;
    let set = load(&dir.join(format!("{stem}.json")))?;
    let before = listed(custom);
    let mut rows = Vec::new();
    for code in set.arenas.iter().filter(|c| built(custom, c)) {
        let from_recipe = recipe_path(levels, code).and_then(|p| crate::recipe::Recipe::load(&p).ok()).map(|r| (r.name, r.group.unwrap_or_default()));
        let (name, group) = from_recipe.or_else(|| before.iter().find(|b| &b.0 == code).map(|b| (b.1.clone(), b.2.clone()))).unwrap_or_else(|| (code.clone(), String::new()));
        let mut row = json!({"code": code, "name": name});
        if !group.is_empty() {
            row["group"] = json!(group);
        }
        rows.push(row);
    }
    crate::write::write_json(&custom.join("levels.json"), &json!({"format": "pd-custom-levels/1", "levels": rows}))
}

/// Add an arena to the end of the active set (made ready first if need be),
/// and write the game's list.
pub fn join_active(dir: &Path, custom: &Path, levels: &Path, code: &str) -> Result<(), String> {
    let stem = ensure(dir, custom, levels)?;
    add(dir, &stem, code)?;
    write_levels(dir, custom, levels)
}

/// Add an arena to the end of a set (nothing if it is there).
pub fn add(dir: &Path, stem: &str, code: &str) -> Result<(), String> {
    let mut set = load(&dir.join(format!("{stem}.json")))?;
    if !set.arenas.iter().any(|c| c == code) {
        set.arenas.push(code.to_owned());
        save(dir, stem, &set)?;
    }
    Ok(())
}

/// Take an arena out of a set (it stays on disk).
pub fn remove(dir: &Path, stem: &str, code: &str) -> Result<(), String> {
    let mut set = load(&dir.join(format!("{stem}.json")))?;
    set.arenas.retain(|c| c != code);
    save(dir, stem, &set)
}

/// Delete an arena for good: its recipe and item layout (in `levels/` or
/// shelved), its built stage, and its place in every set; then the game's
/// list. (A recipe that was committed is still in git's history.)
pub fn delete_arena(dir: &Path, custom: &Path, levels: &Path, code: &str) -> Result<(), String> {
    if code.is_empty() || code.contains(['/', '\\', '.']) {
        return Err(format!("{code:?} is not an arena's code"));
    }
    for (stem, _) in list(dir) {
        remove(dir, &stem, code)?;
    }
    for folder in [levels.to_path_buf(), levels.join("shelved")] {
        for file in [format!("{code}.json"), format!("{code}.layout.json")] {
            let p = folder.join(file);
            if p.exists() {
                std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            }
        }
    }
    let stage = custom.join("stages").join(code);
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    }
    if active(dir, custom).is_some() {
        write_levels(dir, custom, levels)?;
    }
    Ok(())
}

/// Rename a set (its file keeps its stem).
pub fn rename(dir: &Path, stem: &str, name: &str) -> Result<(), String> {
    let mut set = load(&dir.join(format!("{stem}.json")))?;
    set.name = name.trim().to_owned();
    save(dir, stem, &set)
}

/// Delete a set (not its arenas). The active one can't be: switch first.
pub fn delete_set(dir: &Path, custom: &Path, stem: &str) -> Result<(), String> {
    if active(dir, custom).as_deref() == Some(stem) {
        return Err("the set in use can't be deleted: choose another first".into());
    }
    let p = dir.join(format!("{stem}.json"));
    std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch repo: `levels/` with recipes, `sets/`, and `custom/` with
    /// the game's list and built stages.
    fn scratch(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("pd_import_sets_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (levels, custom) = (root.join("levels"), root.join("custom"));
        std::fs::create_dir_all(&levels).unwrap();
        for (code, name, group) in [("kok", "Kok", ""), ("auto1", "Auto 1", "Autolevel"), ("auto2", "Auto 2", "Autolevel"), ("fresh", "Fresh", "")] {
            let g = if group.is_empty() { String::new() } else { format!(r#""group": "{group}","#) };
            std::fs::write(levels.join(format!("{code}.json")), format!(r#"{{"format": "pd-import-recipe/1", "code": "{code}", "name": "{name}", {g} "scale": 1.0, "source": {{"game": "gltf", "path": "x.glb"}}, "min": [0,0,0], "max": [1,1,1]}}"#)).unwrap();
            std::fs::create_dir_all(custom.join("stages").join(code)).unwrap();
            std::fs::write(custom.join("stages").join(code).join("bg.json"), "{}").unwrap();
        }
        std::fs::write(levels.join("auto1.layout.json"), "{}").unwrap();
        std::fs::write(custom.join("levels.json"), r#"{"format":"pd-custom-levels/1","levels":[{"code":"kok","name":"Kok"},{"code":"auto1","name":"Auto 1","group":"Autolevel"},{"code":"auto2","name":"Auto 2","group":"Autolevel"}]}"#).unwrap();
        (dir_for(&levels), custom, levels)
    }

    fn codes(custom: &Path) -> Vec<String> {
        listed(custom).into_iter().map(|r| r.0).collect()
    }

    /// The first time, "Main" is made from the game's list and made active;
    /// an import joins it; taking one out leaves it on disk and out of the
    /// game; another set switches the game's list; a deleted arena is gone
    /// from disk and every set.
    #[test]
    fn sets_decide_what_the_game_lists() {
        let (dir, custom, levels) = scratch("decide");
        assert_eq!(ensure(&dir, &custom, &levels).unwrap(), "main");
        assert_eq!(load(&dir.join("main.json")).unwrap().arenas, ["kok", "auto1", "auto2"]);
        assert_eq!(codes(&custom), ["kok", "auto1", "auto2"]);
        // an import joins the active set
        join_active(&dir, &custom, &levels, "fresh").unwrap();
        assert_eq!(codes(&custom), ["kok", "auto1", "auto2", "fresh"]);
        assert_eq!(listed(&custom)[1], ("auto1".into(), "Auto 1".into(), "Autolevel".into()), "named and grouped by its recipe");
        // out of the set: out of the game, still on disk
        remove(&dir, "main", "auto2").unwrap();
        write_levels(&dir, &custom, &levels).unwrap();
        assert_eq!(codes(&custom), ["kok", "auto1", "fresh"]);
        assert!(levels.join("auto2.json").exists() && custom.join("stages/auto2").exists());
        // another set
        let mut small = ArenaSet::new("Small");
        small.arenas = vec!["auto2".into(), "kok".into(), "unbuilt".into()];
        save(&dir, "small", &small).unwrap();
        set_active(&dir, &custom, &levels, "small").unwrap();
        assert_eq!(codes(&custom), ["auto2", "kok"], "an arena not built is not listed");
        assert_eq!(list(&dir).iter().map(|s| s.1.name.as_str()).collect::<Vec<_>>(), ["Main", "Small"]);
        assert!(delete_set(&dir, &custom, "small").is_err(), "the set in use");
        // deleted for good
        delete_arena(&dir, &custom, &levels, "auto1").unwrap();
        assert!(!levels.join("auto1.json").exists() && !levels.join("auto1.layout.json").exists() && !custom.join("stages/auto1").exists());
        assert!(!load(&dir.join("main.json")).unwrap().arenas.contains(&"auto1".to_owned()));
        assert!(delete_arena(&dir, &custom, &levels, "../x").is_err());
        // back to Main, renamed; Small deleted
        set_active(&dir, &custom, &levels, "main").unwrap();
        rename(&dir, "main", " Everything ").unwrap();
        delete_set(&dir, &custom, "small").unwrap();
        assert_eq!(list(&dir).iter().map(|s| (s.0.as_str(), s.1.name.as_str())).collect::<Vec<_>>(), [("main", "Everything")]);
        assert_eq!(codes(&custom), ["kok", "fresh"]);
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
