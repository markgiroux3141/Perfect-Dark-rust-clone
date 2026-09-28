//! The Combat Simulator world: everything that decides what happens, and nothing
//! that draws it or plays it.
//!
//! A `World` is built from a `pd_core::mp::MatchSetup` and a stage, then stepped
//! once per N64 frame with each human player's input. It publishes what happened as
//! [`events`] (sounds, hits, kills, effects) and exposes read-only views for the
//! renderer: chr poses, effect lists and HUD state. The renderer never writes back
//! into the world. In the spikes it did, through `Chr::gunpos_rendered`.

// PD's constants are kept digit for digit (the head-bob damping, gravity), and
// ported functions keep the C's shape (`!(a > b)` where NaN matters to PD,
// index loops, argument lists).
#![allow(clippy::excessive_precision, clippy::neg_cmp_op_on_partial_ord, clippy::type_complexity, clippy::too_many_arguments, clippy::needless_range_loop)]

pub mod bot;
pub mod chr;
pub mod events;
pub mod fx;
pub mod gun;
pub mod mp;
pub mod nav;
pub mod player;
pub mod props;
pub mod stage;
pub mod world;
