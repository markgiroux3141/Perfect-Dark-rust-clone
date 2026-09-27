//! Framebuffer effects (`bondview.c`, `player_draw_fade`): Slayer interlace, static,
//! zoom blur (reads last frame), Combat Boost wipe, fades. Then the N64 video / CRT
//! chain from `n64::gpu` when it is enabled. Source: `pd_guns/render.rs`, `pdpost.wgsl`.
