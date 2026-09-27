//! Guns: the weapon table and gun scripts (`gset.c`, from `assets/data/weapons.json`,
//! the one weapon table for players and simulants alike), the hand state machine
//! (`bondgun.c`: fire, burst/auto, reload, dry fire, switch, gun functions, dual
//! wield), the on-screen pose (sway, recoil, swivel, zoom), and shots
//! (`shot_create`/`shot_calculate_hits`/`hands_tick_attack`, penetration, hit parts).
//!
//! Sources: `pd_guns/gset.rs`, `bgun.rs`, `bgun_state.rs`, `bgun_pose.rs`, the shot
//! half of `sim.rs`. `PlayerGun`'s screen and projection fields move to a per-player
//! view struct. Replaces `pd_spike/weapons.rs` (8 hand-copied guns).
