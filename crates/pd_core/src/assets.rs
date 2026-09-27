//! The asset layout, in one place: every path under `assets/` that the game reads
//! is built here from ids (texture number, model file, sfx id, stage code, ...).
//! Loaders take an `AssetDir` (a root path found by the engine or a test) so this
//! crate never guesses where the repo is. See docs/ARCHITECTURE.md, "Assets".
