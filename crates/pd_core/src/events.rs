//! What the headless crates tell the outside: one `Event` type for the menus
//! and the world. `pd_menu` queues menu sounds with it now; `pd_sim` re-exports
//! it and adds its variants (footsteps, grunts, hits, kills, respawns, screen
//! fades) as they are ported. The game turns sounds into engine voices
//! (`pd_game::audio`).
//!
//! It lives here, not in `pd_sim`, because `pd_menu` must not depend on
//! `pd_sim` (docs/ARCHITECTURE.md § Crates).
//!
//! Replaces the spikes' three sound-queue formats (`pd_guns::SoundReq`,
//! `pd_spike`'s footsteps and grunts, `pd_menu`'s `(id, pitch, volume)` tuples).

/// One thing that happened this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `snd_start(sound)`: a PD sound ref as the game passes it, an `SFXNUM_*`
    /// (bank sound number) or an `SFXMAP_*` (bit 15 set, an index into
    /// `g_AudioRussMappings`, snd.c:2111), played at `pitch` (1 is as
    /// recorded), `volume` (linear, 1 is the sound's own) and `pan` (−1 left,
    /// 1 right).
    Sound { sound: u16, pitch: f32, volume: f32, pan: f32 },
    /// A sound PD keeps a handle to so it can stop it later (a hand's
    /// `audiohandle`: the Reaper's spin, the Mauler's charge). `handle` is
    /// unique among the sounds playing; a new one on the same handle replaces
    /// the old.
    HandleSound { handle: u32, sound: u16, pitch: f32, volume: f32, pan: f32 },
    /// Stop the sound on `handle`, if it still plays.
    StopSound { handle: u32 },
}
