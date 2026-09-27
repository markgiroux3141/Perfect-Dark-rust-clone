//! The handoff from menus to match. `MatchSetup` holds the arena, scenario, options
//! (`MPOPTION_*`), time/score/team limits, weapon set slots, players (name, body,
//! head, team, handicap, control style) and simulants (type, difficulty, body, head,
//! team). The menus produce it and `pd_sim` consumes it; neither depends on the other.
//!
//! Source: the fields of `pd_menu/mp.rs` `MpSetup`/`MpChrConfig`, which today end in
//! a text summary (`pd_menu/mod.rs` `start_match`).
