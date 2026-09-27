//! `World`: one `Lv`, one `Rng`, the stage, all chrs (players and simulants in one
//! list, as PD has), props, effect state, the match, and the event queue. `step`
//! runs PD's frame order (input + `bgun_tick_gameplay` -> camera -> world ticks ->
//! `hands_tick_attack` -> `bgun_tick_gameplay2` -> chrs/bots -> props -> match ->
//! events) for all players at once. Sized for up to four human players; split-screen
//! is a renderer concern.
//!
//! Source: `pd_complex/fight.rs` `Fight::frame`, which glued the two spike sims.
