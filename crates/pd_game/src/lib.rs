//! `pd_game`'s library half: the parts of the game the tools share.
//! [`session`] couples a match with the menus drawn over it, as PD does each
//! frame; the binary (`perfect_dark`) and `pd_snapshot flow` both use it.

pub mod session;
