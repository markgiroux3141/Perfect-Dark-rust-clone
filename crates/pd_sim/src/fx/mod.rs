//! The effects the world ticks: tracers ([`beam`], `gunfx.c`), impact sparks
//! ([`sparks`], `sparks.c` / `sparkstick.c`), bullet holes and scorches
//! ([`wallhit`], `wallhit.c`), glass shards ([`shards`], `shards.c`), ejected casings ([`casing`], `gunfx.c` /
//! `casingtick.c`), crossbow bolts' trails ([`boltbeam`], `gunfx.c`) and smoke
//! ([`smoke`], `smoke.c`). They are simulation
//! state because PD ticks them with `lvupdate240` and draws random numbers
//! from the one stream as it does; `pd_render::fx` turns them into triangles.
//!
//! Sources: the old repo's `pd_guns/fx.rs` and `smoke.rs`, split from their
//! geometry (now `pd_render::fx`).

pub mod beam;
pub mod boltbeam;
pub mod casing;
pub mod shards;
pub mod smoke;
pub mod sparks;
pub mod wallhit;

pub use beam::Beam;
pub use casing::Casing;
pub use smoke::Smokes;
pub use sparks::Sparks;
pub use wallhit::Wallhit;

/// Every effect in a world.
#[derive(Clone, Default)]
pub struct Fx {
    pub sparks: Sparks,
    /// Bullet holes and scorches, oldest first.
    pub wallhits: std::collections::VecDeque<Wallhit>,
    pub casings: Vec<Casing>,
    pub smokes: Smokes,
    /// `casing_tick`'s landing sound limiter: quarter-ticks until another may play.
    pub casing_cooldown240: i32,
    pub boltbeams: boltbeam::BoltBeams,
    pub shards: shards::Shards,
}

impl Fx {
    /// Keep at most [`wallhit::MAX_WALLHITS`] holes, dropping the oldest.
    pub fn push_wallhit(&mut self, wh: Wallhit) {
        if self.wallhits.len() >= wallhit::MAX_WALLHITS {
            self.wallhits.pop_front();
        }
        self.wallhits.push_back(wh);
    }
}
