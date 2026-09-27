//! Frame timing, as `lv.c` computes it: `lvupdate240` (quarter-ticks this frame) with
//! the remainder carry, `lvupdate60`, `lvupdate60f`/`freal`, `lvframe60`, the slow-motion
//! cap (Combat Boost, MP slow-mo option) and `diffframe60` for the menus.
//!
//! There is exactly one `Lv` per world. The spikes had three (`pd_guns::bgun::Lv`,
//! `pd_spike::sim::Globals`, `pd_menu` `Vars`), which is why Combat Boost slowed the
//! player but not the bots.
