//! What makes an `autolevel` layout a level to play: its polygons in Complex's grammar, room
//! meshes, the GE64 OBJ and GLB writers, a ray-traced bake and the command line.
//!
//! This is the Blender side of the Python (`ge_pd_tools`: `csg`, `sunbeam`, `styles/complex`,
//! `autolevel/geometry`, `build`, `blender_io`, `gltf_export`) with no Blender in it. The
//! polygon stage is held to an answer key as the layout is (`tests/key/`, written by
//! `scripts/autolevel_key.py polys lights`); the bake is held to a tolerance.
//! Plan: `docs/AUTOLEVEL.md`.
//!
//! Headless, like `autolevel`: no window and no GPU. A game that only wants the layout need not
//! carry this crate.
//!
//! So far: the polygons. `geometry::polys(&level, kit)` is every polygon of a level, in the Python's
//! order, each with its surface (a role of the kit's palette: `kit::Kit`, Complex's `complex::KIT`), and `complex::plan_lights` the
//! lamps a bake is given.

pub mod bake;
pub mod built;
pub mod cli;
pub mod complex;
pub mod csg;
pub mod felicity;
pub mod geometry;
pub mod glb;
pub mod kit;
pub mod mesh;
pub mod obj;
pub mod openings;
pub mod sunbeam;
pub mod svg;
pub mod sweep;
pub mod textures;
pub mod walk;
