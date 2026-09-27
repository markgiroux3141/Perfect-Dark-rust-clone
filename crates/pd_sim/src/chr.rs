//! Chrs, player and simulant alike: the `chrdata` subset, movement
//! (`chr_calculate_push_pos`, the ground half of `chr_update_position`, falls,
//! ladders, duck/squat), go-to (`chr_go_to_room_pos`, `chr_tick_gopos`), damage and
//! hit parts (`chr_damage`), death, spawn (`player_choose_spawn_location`,
//! `chr_adjust_pos_for_spawn`), footsteps, and the body pose, computed here with
//! `pd_core::model` so gun positions come from the sim, not from the renderer.
//!
//! Sources: `pd_spike/chr.rs`, `chraction.rs`, `thirdperson.rs`, `gunpos.rs`.
