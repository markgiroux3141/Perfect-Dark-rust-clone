//! PD's maths, bugs included: `M_BADPI`/`M_BADTAU` angle constants and the
//! `BADDTOR` family, the `pdmtx` matrix helpers, `atan2f` and friends. Using true pi
//! where PD uses `M_BADPI` changes behaviour (the bots' trigger cone).
//!
//! Sources: `pd_spike/pdmath.rs`, `pd_guns/pdmtx.rs`, `pd_guns/props.rs` `pd_atan2f`.
