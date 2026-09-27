//! PD's HUD, rasterised on the CPU into a PD-pixel canvas and composited: gun HUD
//! (ammo gauges, function square, banners), health bar, damage flash, sights, fades,
//! and later the radar and MP messages. Timers live in the sim; this only draws.
//! Sources: `pd_guns/hud.rs` (draw half), `pd_complex/health.rs` (draw half),
//! `pd_guns/app.rs` sights.
