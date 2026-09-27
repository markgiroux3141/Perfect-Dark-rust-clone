//! The runner: owns the winit event loop, the window, the frame clock, input
//! collection, audio and the debug UI, and drives a `Game`:
//!
//! - `init(ctx)` once the GPU exists;
//! - `tick(ctx)` at the fixed tick rate (as many times as the clock says per frame);
//! - `frame(ctx, alpha)` once per rendered frame, for per-frame work such as mouse look;
//! - `render(ctx, frame)` to record GPU work into the frame's encoder;
//! - `debug_ui(ctx, egui)` for the developer panel (F1).
//!
//! Replaces the four hand-rolled `ApplicationHandler`s of the old repo
//! (`app.rs`, `pd_guns/app.rs`, `pd_menu/app.rs`, `pd_spike/viewer.rs`).
//! Headless tools never touch this module.
