//! Every multiplayer map in a Jedi Academy `base` directory's packages
//! (`maps/mp/*.bsp`), converted in one run: a recipe for each
//! (`levels/jka_<map>.json`, so each can be redone, edited in `pd_edit` and
//! exported alone), the GoldenEye Setup Editor's level files for each
//! ([`crate::ge64`]), and, if asked, each as an arena of the clone. One
//! recipe shape for all of them: nothing is tuned by hand here; what a map
//! needs besides goes into its recipe (a `substitute`, the counts) or its
//! layout (`pd_edit`).

use std::path::{Path, PathBuf};


use crate::pk3::Vfs;
use crate::recipe::Recipe;

pub struct Options {
    /// Import each into the clone's `custom/` too (the waypoints take seconds
    /// to minutes a map). Siege maps are left out unless `only` names them:
    /// they are objective missions over 700 m, whose waypoints alone run for
    /// hours (siege_desert: past two, unfinished).
    pub clone: bool,
    /// Only these maps (file stems: `ffa1`), else all.
    pub only: Vec<String>,
    pub ge64: crate::ge64::Options,
    /// The export's lighting detail (tolerance, min edge).
    pub light: (f32, f32),
}

/// The Setup Editor folder's guide, written beside the maps.
const README: &str = r#"JEDI ACADEMY MAPS for the GoldenEye Setup Editor
================================================

Every multiplayer map in Jedi Academy's packages, converted by the Perfect
Dark Rust clone's importer into the files the Setup Editor exports a level
as (the layout of its own export of a PD level, e.g. Complex). One folder a
map:

  level\LevelIndices.obj + .mtl   the visual: rooms primary_RoomXX (hex),
                                  vertex colours = the map's lightmaps,
                                  baked; textures as BMPs at N64 sizes
  portal\portals.obj/.mtl/.txt    the portals between the rooms
  clipping\clippingObj<code>.obj  collision, a group a room (ClipXXX), tagged
                                  ForceFloor / ForceWall / Railing /
                                  SolidLadder / TransparentLadder _SFX<n>
  markers.txt                     the map's own player starts and weapon
                                  spots (cm, facing, room), to place the
                                  setup by hand
  report.txt                      what the conversion did, and what it left
                                  out (missing textures, doors, lifts)

Units are PD's: 1 = 1 cm (Jedi Academy units x 2.65). Spawns, weapons, ammo
and paths are not included: place them in the editor.

Rebuild (in the clone's repo):
  cargo run --release -p pd_import --bin pd_import -- --jka "<this folder>" --base "<GameData\base>" [--clone] [--only ffa1,duel1]
One map:
  cargo run --release -p pd_import --bin pd_import -- --ge64 jka_ffa1 "<folder>" [--rooms N] [--light T,E] [--texels N]

Known gaps (the same in every map): doors, lifts and other moving parts are
left out (their doorways stand open, a lift's shaft is a drop); skies are not
drawn (no sky box in PD: leave the room tops open or close them); kill
volumes over bottomless pits are not converted; Jedi Academy jumps where PD
climbs (ledges up to 1.6 m are SolidLadder). The textures are Raven
Software's, from your copy of the game: for your own ROM, not for sharing.
"#;

/// The map's name: its worldspawn `message`, unless that is a string-table
/// reference (`@...`) or empty.
fn title(stem: &str, message: Option<String>) -> String {
    match message {
        Some(m) if !m.trim().is_empty() && !m.starts_with('@') => m.trim().to_string(),
        _ => stem.to_ascii_uppercase(),
    }
}

/// A folder name from a map's file stem and name (`ffa1 - Vjun Sentinel`).
fn folder(stem: &str, name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    format!("{stem} - {}", clean.trim())
}

/// Convert every multiplayer map in `base`'s packages into `out` (and the
/// clone's custom tree, with `opts.clone`), its recipe written into
/// `levels`. Returns the report: a line a map, and each map's own.
pub fn jka(base: &Path, out: &Path, levels: &Path, paths: &crate::Paths, opts: &Options) -> Result<Vec<String>, String> {
    let vfs = Vfs::open(base)?;
    let mut stems: Vec<String> = vfs.names().filter_map(|n| n.strip_prefix("maps/mp/")?.strip_suffix(".bsp").map(str::to_owned)).filter(|s| !s.contains('/')).collect();
    stems.sort();
    stems.dedup();
    if !opts.only.is_empty() {
        stems.retain(|s| opts.only.iter().any(|o| o.eq_ignore_ascii_case(s)));
    }
    if stems.is_empty() {
        return Err(format!("{}: no maps/mp/*.bsp in its packages", base.display()));
    }

    // Stage numbers: a map's own if its recipe exists, else the next free.
    let recipe_path = |stem: &str| levels.join(format!("jka_{stem}.json"));
    let mut taken: Vec<u8> = Vec::new();
    for e in std::fs::read_dir(levels).map_err(|e| format!("{}: {e}", levels.display()))?.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "json") && !p.to_string_lossy().ends_with(".layout.json") {
            if let Ok(r) = Recipe::load(&p) {
                taken.push(r.stagenum);
            }
        }
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join("README.txt"), README.replace('\n', "\r\n")).map_err(|e| format!("{}: {e}", out.display()))?;

    let mut report = Vec::new();
    let mut failed = Vec::new();
    for stem in &stems {
        let rp = recipe_path(stem);
        let result = (|| -> Result<Vec<String>, String> {
            let bytes = vfs.read(&format!("maps/mp/{stem}.bsp"))?.ok_or("not readable")?;
            let name = title(stem, crate::jka::map_title(&bytes));
            let stagenum = match Recipe::load(&rp) {
                Ok(r) => r.stagenum,
                Err(_) => {
                    let n = pd_core::assets::CUSTOM_STAGENUMS.clone().find(|n| !taken.contains(n)).ok_or("no custom stage number left (0x60-0x7f)")?;
                    taken.push(n);
                    n
                }
            };
            if !rp.exists() {
                std::fs::write(&rp, recipe_text(stem, &name, stagenum, base)).map_err(|e| format!("{}: {e}", rp.display()))?;
            }
            let r = Recipe::load(&rp)?;
            let dir = out.join(folder(stem, &r.name));
            let mut lines = crate::export_ge64(&r, paths, &dir, &opts.ge64, Some(opts.light))?;
            let siege = stem.starts_with("siege_") && opts.only.is_empty();
            if opts.clone && siege {
                lines.push("── the clone: left out (a siege map; --only names it to import it) ──".into());
            }
            if opts.clone && !siege {
                let layout = crate::layout::Layout::load(&crate::layout::Layout::path_for(&rp))?.unwrap_or_else(crate::layout::Layout::new);
                lines.push("── the clone ──".into());
                lines.extend(crate::import(&r, paths, &layout)?);
            }
            std::fs::write(dir.join("report.txt"), (lines.join("\n") + "\n").replace('\n', "\r\n")).map_err(|e| format!("{}: {e}", dir.display()))?;
            Ok(lines)
        })();
        match result {
            Ok(lines) => {
                let get = |p: &str| lines.iter().find(|l| l.starts_with(p)).cloned().unwrap_or_default();
                report.push(format!("{stem}: ok. {} | {}", get("level:"), get("check:")));
            }
            Err(e) => {
                report.push(format!("{stem}: FAILED: {e}"));
                failed.push(stem.clone());
            }
        }
    }
    report.push(format!("{} maps, {} failed{}", stems.len(), failed.len(), if failed.is_empty() { String::new() } else { format!(": {}", failed.join(", ")) }));
    Ok(report)
}

/// A map's recipe, laid out as the hand-written ones are.
pub fn recipe_text(stem: &str, name: &str, stagenum: u8, base: &Path) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let base = q(&base.to_string_lossy().replace('\\', "/"));
    let name = q(name);
    format!(
        r#"{{
  "format": "pd-import-recipe/1",
  "code": "jka_{stem}",
  "name": {name},
  "group": "Jedi Academy",
  "stagenum": {stagenum},
  "scale": 2.65,
  "source": {{
    "game": "jka",
    "base": {base},
    "bsp": "maps/mp/{stem}.bsp"
  }},
  "spawns": 12,
  "weapons": 10,
  "hills": 4,
  "play_area": "starts"
}}
"#
    )
}

/// `base`'s default: `extra maps/base` in the repo (from the crate's
/// manifest directory, `crates/pd_import`).
pub fn default_base(manifest: &Path) -> PathBuf {
    manifest.ancestors().nth(2).unwrap_or(manifest).join("extra maps").join("base")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_folders() {
        assert_eq!(title("ffa1", Some("Vjun Sentinel".into())), "Vjun Sentinel");
        assert_eq!(title("siege_korriban", Some("@siege_korr_loc_10".into())), "SIEGE_KORRIBAN");
        assert_eq!(title("duel8", None), "DUEL8");
        assert_eq!(folder("ffa3", "Tatooine: FFA!"), "ffa3 - Tatooine_ FFA_");
        let text = recipe_text("ffa1", "Vjun \"Sentinel\"", 0x66, Path::new(r"D:\x\base"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!((v["code"].as_str(), v["name"].as_str(), v["group"].as_str(), v["stagenum"].as_u64()), (Some("jka_ffa1"), Some("Vjun \"Sentinel\""), Some("Jedi Academy"), Some(0x66)));
        assert_eq!(v["source"]["base"], "D:/x/base");
    }
}
