//! The level generator in the editor: a generated level's place among the
//! custom levels. The generator's own editor (`autolevel_edit::ui::Editor`:
//! the plan, room boxes, joins, one room's choices) is hosted by the window;
//! this is what joins its recipe to the importer: the arena's name and code
//! ([`Form`]; the game numbers custom levels itself), the level's recipe file written with the
//! generator's recipe in it ([`Form::write`]: what `autolevel import` writes,
//! `pd_import::autowalk::recipe_text`), and a generated level's recipe read
//! back ([`of`]), so a level made here can be reshaped later.
//!
//! The importer's source for generated levels makes the level again from
//! that recipe, so nothing else is written: no build folder, no glTF.

use std::path::PathBuf;

use autolevel::recipe as gen;
use pd_import::autowalk::{self, Via};
use pd_import::recipe::{Recipe, Source};

use crate::newlevel::code_from;
use crate::paths::EditPaths;

/// The arena a generated level becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    pub code: String,
    /// The name and code still follow the seed (nobody has typed one).
    pub auto: bool,
}

/// Is the recipe file of this code a generated level's (so it may be
/// written over), or is there none?
fn ours_or_free(paths: &EditPaths, code: &str) -> bool {
    match std::fs::read_to_string(paths.levels.join(format!("{code}.json"))) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|v| v["source"]["game"].as_str() == Some("autolevel")),
        Err(_) => !pd_core::ids::STAGE_CODES.iter().any(|(_, c)| *c == code),
    }
}

impl Form {
    /// The form for a seed's level: `autolevel<seed>`, "Autolevel <seed>".
    pub fn for_seed(seed: u64, _paths: &EditPaths) -> Form {
        Form { name: format!("Autolevel {seed}"), code: format!("autolevel{seed}"), auto: true }
    }

    /// The seed changed: the name and code follow it, unless one was typed.
    pub fn follow(&mut self, seed: u64, paths: &EditPaths) {
        if self.auto && self.code != format!("autolevel{seed}") {
            *self = Form::for_seed(seed, paths);
        }
    }

    /// What's wrong with it, if anything.
    pub fn problem(&self, paths: &EditPaths) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("give it a name".into());
        }
        if self.code.is_empty() || code_from(&self.code) != self.code {
            return Some(format!("the code may hold only a-z, 0-9 and _ (e.g. {})", code_from(&self.code)));
        }
        if !ours_or_free(paths, &self.code) {
            return Some(format!("the code {:?} is another level's, not a generated one: give this one another", self.code));
        }
        None
    }

    /// Does this make a level again that has a layout placed by hand? (It
    /// is kept: its items may then be out of place.)
    pub fn has_layout(&self, paths: &EditPaths) -> bool {
        pd_import::layout::Layout::path_for(&paths.levels.join(format!("{}.json", self.code))).exists()
    }

    /// Write the level's recipe, with the generator's in it; returns its
    /// path. (The importer then makes the level from it.)
    pub fn write(&self, recipe: &gen::Recipe, paths: &EditPaths) -> Result<PathBuf, String> {
        if let Some(p) = self.problem(paths) {
            return Err(p);
        }
        let text = autowalk::recipe_text(&self.code, self.name.trim(), Via::Recipe(&gen::save(recipe)));
        let path = paths.levels.join(format!("{}.json", self.code));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

/// A generated level's form and the generator's recipe it was made from.
/// None for any other level, and for a generated one whose recipe names a
/// file instead of holding the generator's recipe.
pub fn of(recipe: &Recipe) -> Option<(Form, gen::Recipe)> {
    let Source::Autolevel(src) = &recipe.source else { return None };
    if !src.recipe.is_object() {
        return None;
    }
    let made = gen::load(&src.recipe.to_string()).ok()?;
    let auto = recipe.code == format!("autolevel{}", made.seed);
    Some((Form { name: recipe.name.clone(), code: recipe.code.clone(), auto }, made))
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::recipe::Edit;

    /// Empty levels and custom folders of this test's own.
    fn paths(name: &str) -> EditPaths {
        let dir = std::env::temp_dir().join(format!("pd_edit_generator_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("levels")).unwrap();
        EditPaths { assets: dir.join("assets"), custom: dir.join("custom"), levels: dir.join("levels") }
    }

    #[test]
    fn a_generated_level_is_written_as_the_importer_reads_it_and_read_back() {
        let paths = paths("write");
        let form = Form::for_seed(40, &paths);
        assert_eq!((form.name.as_str(), form.code.as_str(), form.auto), ("Autolevel 40", "autolevel40", true));
        assert_eq!(form.problem(&paths), None);
        let mut made = gen::Recipe::new(40);
        made.edits.push(Edit::Cover { room: "vast".into(), cover: "none".into() });
        made.edits.push(Edit::Move { room: "medium1".into(), to: (60, 29) });
        let path = form.write(&made, &paths).unwrap();
        // the importer's own reader takes it, as a generated level, with the generator's recipe whole
        let recipe = Recipe::load(&path).unwrap();
        assert_eq!((recipe.code.as_str(), recipe.name.as_str(), recipe.group.as_deref()), ("autolevel40", "Autolevel 40", Some("Autolevel")));
        assert_eq!(of(&recipe), Some((form.clone(), made.clone())));
        // made again: its own code is no problem, and its form comes back as it was
        assert_eq!(form.problem(&paths), None);
        assert_eq!(Form::for_seed(40, &paths), form);
        assert!(!form.has_layout(&paths));
        let _ = std::fs::remove_dir_all(paths.levels.parent().unwrap());
    }

    #[test]
    fn what_is_wrong_with_a_form_is_said() {
        let paths = paths("problem");
        let form = Form::for_seed(40, &paths);
        form.write(&gen::Recipe::new(40), &paths).unwrap();
        // a level that is not a generated one keeps its code
        std::fs::write(paths.levels.join("kokiri.json"), r#"{"code": "kokiri", "name": "Kokiri", "stagenum": 100, "source": {"game": "oot"}}"#).unwrap();
        let named = |code: &str| Form { name: "A level".into(), code: code.into(), auto: false };
        assert_eq!(named("kokiri").problem(&paths).unwrap(), "the code \"kokiri\" is another level's, not a generated one: give this one another");
        assert_eq!(named("ref").problem(&paths).unwrap(), "the code \"ref\" is another level's, not a generated one: give this one another");
        assert_eq!(named("My Level").problem(&paths).unwrap(), "the code may hold only a-z, 0-9 and _ (e.g. my_level)");
        assert_eq!(Form { name: " ".into(), ..named("mine") }.problem(&paths).unwrap(), "give it a name");
        assert_eq!(named("mine").problem(&paths), None);
        assert!(named("kokiri").write(&gen::Recipe::new(1), &paths).is_err());
        // the name and code follow the seed until one is typed
        let mut form = Form::for_seed(40, &paths);
        form.follow(41, &paths);
        assert_eq!((form.code.as_str(), form.name.as_str()), ("autolevel41", "Autolevel 41"));
        form.auto = false;
        form.follow(42, &paths);
        assert_eq!(form.code, "autolevel41");
        // any other level is no generated one
        assert!(of(&Recipe::load(&paths.levels.join("autolevel40.json")).unwrap()).is_some());
        let _ = std::fs::remove_dir_all(paths.levels.parent().unwrap());
    }
}
