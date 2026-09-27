//! A stage as the simulation needs it. It is built from `pd_core` stage data:
//! - the generic geometry input (`LevelGeom`/`GeomPoly`: floors, walls, sight/shot
//!   blockers, ladders, crouch zones, floor types, rooms), which any future level
//!   source can also supply;
//! - collision (`TileLevel`: the `lib/collision.c` primitives: volume tests,
//!   cylinder paths, ground finding, ladders, sight/shot rays, room adjacency);
//! - pads, cover and spawn points, weapon/ammo pads from the MP setup.
//!
//! Sources: `pd_spike/level_geom.rs`, `tile_level.rs`, `pd_tiles.rs` (which read the
//! decomp at run time; the stage now comes from `assets/stages/<code>/`).
