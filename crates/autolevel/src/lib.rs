//! Complex-style levels from a seed, with nobody in the loop: rooms of very unequal sizes are
//! scattered, joined by corridors, furnished, checked and scored.
//!
//! This is a port of the Python package `ge_pd_tools.autolevel` (in `addon/` of the repo it
//! started in), module by module, and it gives the same level for the same seed: each stage is
//! held to an answer key dumped from the Python (`tests/key/`, written by
//! `scripts/autolevel_key.py`). The plan and what is ported so far:
//! `docs/AUTOLEVEL.md`.
//!
//! Headless: no window, no GPU, no file formats of a particular game. It is meant to move into
//! the Perfect Dark clone's workspace as it stands.
//!
//! Ported: `pyrand` (Python's random generator), `scatter`, `place` (settle, compact), `spiral`,
//! `space` and `pods` (the rooms as the router sees them), `level::stage_spaces`, `route` (the
//! grid, corridors, stubs and vents), `network` (which rooms are joined), `level::stage_network`,
//! `furnish` (a room's inside: decks, ramps, pool, cover, rails, and a pod's), `build` (the
//! drops and grates, and every room furnished from the network as it stands),
//! `level::stage_interiors`, `deadends` (the places a player stands, the ways between them, and
//! what hangs off the level by one way in), the build's dead-end pass (`Build::resolve`: each
//! joined up or cut short), `level::stage_dead_ends`, `plan` (rooms as boxes, links, and the
//! check that every opening between two rooms was meant), and the last stage,
//! `level::stage_level`: `windows`, `planner` (the level as a plan), `walks` (routes to walk it
//! by, and views) and `report` (its numbers and its score). `level::generate` runs them all: the
//! whole layout, from a seed to a `Level`.
//! New in the Rust, with nothing in the Python behind it: `recipe`, a level recorded as its seed,
//! its options and the edits made by hand (the editor's file), which runs the same stages with
//! each edit applied after the one it belongs to.
//! To come, in pipeline order: polygons from the plan (the Python's `geometry`, `csg`, `sunbeam`
//! and `styles/complex`), the writers, the bake and a command line.

pub mod balcony;
pub mod boxes;
pub mod build;
pub mod candidates;
pub mod cells;
pub mod choices;
pub mod circulation;
pub mod deadends;
pub mod deadlist;
pub mod felicity;
pub mod furnish;
pub mod level;
pub mod network;
pub mod place;
pub mod plan;
pub mod planner;
pub mod pods;
pub mod py;
pub mod pyrand;
pub mod recipe;
pub mod report;
pub mod route;
pub mod scatter;
pub mod sha512;
pub mod space;
pub mod spiral;
pub mod stairs;
pub mod storeys;
pub mod walks;
pub mod windows;
