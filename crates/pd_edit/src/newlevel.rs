//! A new level from a glTF file: its recipe (`levels/<code>.json`, a
//! `"game": "gltf"` source), with a code and a custom stage number nobody
//! uses yet.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::paths::EditPaths;

#[derive(Clone, Debug)]
pub struct NewLevel {
    pub file: PathBuf,
    pub name: String,
    pub code: String,
    pub stagenum: u8,
    /// File units to centimetres (glTF's are metres: 100).
    pub scale: f32,
    /// 0: one per ~250 m² of floor.
    pub rooms: usize,
    pub spawns: usize,
    pub weapons: usize,
    pub hills: usize,
    pub max_texture: u32,
    pub climb_ledges: bool,
}

/// A stage code from a name: lower-case letters, digits and underscores.
pub fn code_from(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_owned();
    if s.is_empty() || s.as_bytes()[0].is_ascii_digit() {
        format!("level_{s}")
    } else {
        s
    }
}

/// A title from a file stem: separators to spaces, each word capitalised.
fn title_from(stem: &str) -> String {
    stem.split(['_', '-', ' ', '.'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The custom stage numbers the recipes and the custom tree use.
fn used_stagenums(paths: &EditPaths) -> Vec<u8> {
    let mut used: Vec<u8> = Vec::new();
    for e in paths.recipes() {
        if let Some(n) = std::fs::read_to_string(&e.path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v["stagenum"].as_u64()) {
            used.push(n as u8);
        }
    }
    let levels = paths.custom.join("levels.json");
    if let Some(v) = std::fs::read_to_string(levels).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        used.extend(v["levels"].as_array().into_iter().flatten().filter_map(|l| l["stagenum"].as_u64()).map(|n| n as u8));
    }
    used
}

fn code_taken(paths: &EditPaths, code: &str) -> bool {
    pd_core::ids::STAGE_CODES.iter().any(|(_, c)| *c == code) || paths.recipes().iter().any(|e| e.code == code) || paths.levels.join(format!("{code}.json")).exists()
}

impl NewLevel {
    /// Defaults for `file`: its name, a free code and stage number, metres.
    pub fn for_file(file: &Path, paths: &EditPaths) -> NewLevel {
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "level".into());
        let mut code = code_from(&stem);
        let base = code.clone();
        let mut k = 2;
        while code_taken(paths, &code) {
            code = format!("{base}{k}");
            k += 1;
        }
        let used = used_stagenums(paths);
        let stagenum = pd_core::assets::CUSTOM_STAGENUMS.clone().find(|n| !used.contains(n)).unwrap_or(*pd_core::assets::CUSTOM_STAGENUMS.end());
        NewLevel { file: file.to_path_buf(), name: title_from(&stem), code, stagenum, scale: 100.0, rooms: 0, spawns: 12, weapons: 10, hills: 4, max_texture: 256, climb_ledges: false }
    }

    /// What's wrong with it, if anything.
    pub fn problem(&self, paths: &EditPaths) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("give it a name".into());
        }
        if code_from(&self.code) != self.code {
            return Some(format!("the code may hold only a-z, 0-9 and _ (e.g. {})", code_from(&self.code)));
        }
        if code_taken(paths, &self.code) {
            return Some(format!("the code {:?} is taken", self.code));
        }
        if !pd_core::assets::CUSTOM_STAGENUMS.contains(&self.stagenum) {
            return Some("the stage number must be 0x60-0x7f".into());
        }
        if used_stagenums(paths).contains(&self.stagenum) {
            return Some(format!("stage {:#x} is taken", self.stagenum));
        }
        if self.spawns == 0 {
            return Some("a level needs spawns".into());
        }
        None
    }

    /// Write its recipe; returns the path.
    pub fn write(&self, paths: &EditPaths) -> Result<PathBuf, String> {
        if let Some(p) = self.problem(paths) {
            return Err(p);
        }
        let v = json!({
            "format": "pd-import-recipe/1",
            "code": self.code,
            "name": self.name.trim(),
            "stagenum": self.stagenum,
            "scale": self.scale,
            "source": {"game": "gltf", "path": self.file.to_string_lossy().replace('\\', "/"), "max_texture": self.max_texture, "climb_ledges": self.climb_ledges},
            "rooms": self.rooms,
            "spawns": self.spawns,
            "weapons": self.weapons,
            "hills": self.hills,
        });
        let path = paths.levels.join(format!("{}.json", self.code));
        std::fs::write(&path, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_titles_come_from_file_names() {
        assert_eq!(code_from("My Level (v2)"), "my_level_v2");
        assert_eq!(code_from("2fort"), "level_2fort");
        assert_eq!(title_from("dust_2-final"), "Dust 2 Final");
    }
}
