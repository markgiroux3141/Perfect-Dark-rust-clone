//! The level editor for `autolevel`: everything but the window.
//!
//! Plan: `docs/AUTOLEVEL.md`, "Track B". It is built to merge into the Perfect
//! Dark clone's `pd_edit`, so it is laid out the same way: a headless document with snapshot
//! undo (`doc`), the slow work on a thread (`jobs`), and a picture that is a list of primitives
//! (`planview`, after `pd_tools::lab`) which the window draws with egui and `shot` rasterises to
//! a PNG (`raster`, `font`). The window is for the user; `autolevel_edit --shot` writes the same
//! view to a file, which is how it is checked. Only `ui` (the window's panels, which take an
//! `egui::Ui`, so `pd_edit` can host them) uses the engine and egui, and it is behind the
//! crate's `window` feature: without it the lib and `--shot` build with neither.
//!
//! Milestone 1: the plan view of a generated level, with `--shot`, and the dead-end list beside
//! it (`deadlist`). Milestone 2 (this): join two places and take a corridor out. A room and a
//! storey are picked, the corridors that could be made from there are found
//! (`autolevel::candidates`: each is the route the recipe's `join` edit would make, so what is
//! drawn is what is made), drawn on the plan with a number each, and one is chosen; a corridor
//! is picked and taken out, or the reason it can't be is given. The document holds what is
//! picked (`doc`), `planview::hit` says what a click on the plan picks, `routes` lists a
//! level's routes by the keys they are named by, and `headless` runs all of it from a command
//! line (`--pick`, `--join`, `--remove`, `--routes`), which is how it is checked.
//!
//! Milestone 3: room boxes by hand. The changes are `autolevel::boxes` (move a room, pull its
//! sides, pin its floor, make one, delete one), made through the document (`doc`), which also
//! says what one would come to without making it. `planview` draws the lots as boxes over the
//! level (or over nothing, `bare`, where the recipe makes no level): the selected one with its
//! grips, a refused change in red with its reason, what the level under them does not show yet
//! marked as moved, new or deleted. `lots` lists them as text, and `headless` makes the changes
//! from a command line (`--rooms`, `--select`, `--lots`, `--move`, `--rect`, `--pin`,
//! `--delete`, `--add`, `--try`).
//!
//! Milestone 4: one room's choices (its layout, its cover, another inside, its brightness, its
//! ceiling). They are `autolevel::choices`, made through the document (`doc`), which refuses
//! what can be refused before the level is made. `planview` says what was chosen for a room in
//! a few words under its name, `chosen` lists a room's choices as text, and `headless` makes
//! them from a command line (`--layout`, `--cover ROOM@..`, `--inside`, `--brightness`,
//! `--ceiling`, `--own`, `--choices`).

pub mod chosen;
pub mod cli;
pub mod controls;
pub mod deadlist;
pub mod doc;
pub mod font;
pub mod headless;
pub mod jobs;
pub mod lots;
pub mod planview;
pub mod raster;
pub mod routes;
pub mod shot;
/// The window's panels (egui, through the engine): behind the `window` feature, so the rest of
/// the lib builds without the engine. Public so that `pd_edit` can host them.
#[cfg(feature = "window")]
pub mod ui;

/// Levels the tests share: each is made once.
#[cfg(test)]
pub(crate) mod fixture {
    use crate::deadlist::{make, Made};
    use autolevel::candidates::{candidates, Candidates};
    use autolevel::level::Options;
    use autolevel::recipe::Recipe;
    use std::sync::OnceLock;

    /// Seed 40 as it falls out of the generator.
    pub fn seed40() -> &'static Made {
        static MADE: OnceLock<Made> = OnceLock::new();
        MADE.get_or_init(|| make(&Recipe::new(40)).expect("seed 40 makes a level"))
    }

    /// Seed 40 with its dead ends left in.
    pub fn seed40_kept() -> &'static Made {
        static MADE: OnceLock<Made> = OnceLock::new();
        let recipe = Recipe { seed: 40, options: Options { dead_ends: false, ..Options::default() }, edits: Vec::new() };
        MADE.get_or_init(|| make(&recipe).expect("seed 40 makes a level"))
    }

    /// What seed 40's tower could be joined to by a door on its storey at 551.
    pub fn tower_joins() -> &'static Candidates {
        static FOUND: OnceLock<Candidates> = OnceLock::new();
        FOUND.get_or_init(|| candidates(&Recipe::new(40), "tower", 551).expect("the tower takes doors on 551"))
    }
}
