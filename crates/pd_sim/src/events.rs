//! What the world tells the outside each step: the one [`Event`] type, which
//! lives in `pd_core::events` so the menus can use it without depending on the
//! world. The world's own variants (footstep, grunt, hit, kill, respawn, screen
//! fade and shake, ...) are added there as they are ported.

pub use pd_core::events::Event;
