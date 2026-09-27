//! The N64 controller as PD reads it: a button bitmask (A, B, Z, START, L, R,
//! D-pad, C-buttons) and a signed 8-bit stick per controller, for up to four
//! controllers, with PD's pressed/held edge semantics.
//!
//! Source: the `Joy` state in `pd_menu/mod.rs` and the raw-code map for USB N64
//! adapters duplicated in `pd_guns/app.rs` and `pd_menu/app.rs`.
