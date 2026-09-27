//! GPU plumbing, with no pipelines of its own:
//! - `Gpu`: instance, adapter, device, queue, surface and formats; backend and present
//!   mode come from a config struct, not environment variables;
//! - `RenderTarget`: offscreen colour + depth at any size, e.g. PD's 320x220;
//! - `Frame`: the swapchain image and command encoder for one rendered frame;
//! - `present`: scale a low-resolution target to the window (nearest or linear, with
//!   aspect letterboxing). Low-resolution rendering is first-class, not a hook.
//!
//! Also holds the naga validation helper the shader tests in every crate use.
