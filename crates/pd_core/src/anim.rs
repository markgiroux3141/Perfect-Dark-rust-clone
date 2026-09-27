//! PD's animation system: the bit-packed animation bank decoder (`anim.c`) and
//! `struct anim` (`model.c`): 240 Hz sub-tick frame advance, lazy wrap and clamp,
//! loop windows, the 16-tick merge, speed tweens, negative speed, flip, CHRINFO
//! root motion and `ANIMFLAG_ABSOLUTETRANSLATION`.
//!
//! Source: `pd_guns/anim.rs` + `pd_guns/animdata.rs`, the complete port.
//! Replaces `pd_spike/model.rs` `Anim`, which posed bots from glTF clips and lacked
//! flip and absolute translation, along with its hand tables `anims.rs` and `root_y.rs`.
