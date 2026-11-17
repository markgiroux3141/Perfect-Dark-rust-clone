//! **The level editor** (`pd_edit`). A custom level's items, waypoints and
//! doors (any of PD's and GoldenEye's door models, `pd_import::doors`), placed
//! by hand over the stage as the game loads it,
//! in a fly-through view with a panel on the left. The edits are the level's
//! layout file (`crates/pd_import/levels/<code>.layout.json`, see
//! `pd_import::layout`); the editor runs the importer to place them (and to
//! import a new glTF), so what it shows is what the game plays.
//!
//! A level can also be made here by the level generator ([`generator`]: its
//! own editor's panels, hosted, and its recipe written as a level's), seen in
//! 3D while it is shaped and walked in place ([`preview`]: the level as the
//! game would load it, in a scratch folder; [`space`]: the view of it).
//!
//! The window is for the user; `pd_edit --shot` renders the same view to a
//! PNG ([`shot`]). Everything the window doesn't need to draw lives here so
//! the tests can reach it.

pub mod camera;
pub mod doc;
pub mod draw3d;
pub mod generator;
pub mod gizmo;
pub mod jobs;
pub mod newlevel;
pub mod overlay;
pub mod paths;
pub mod preview;
pub mod scene;
pub mod shot;
pub mod space;
