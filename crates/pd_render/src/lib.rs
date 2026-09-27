//! Perfect Dark on the GPU. It reads `pd_sim` views and never writes to the world.
//! Every model (the BG, guns, hands, simulant bodies and heads, props) goes through
//! the one `n64::gpu` combiner path, so simulants are lit and textured the way the
//! guns and the level are. In the spike match they were drawn flat by the old
//! engine's glTF path.
//!
//! One `View` per human player (viewport, camera, fov), so split-screen needs no
//! special case.

pub mod bg;
pub mod fx;
pub mod hud;
pub mod models;
pub mod post;
pub mod view;
pub mod xray;
