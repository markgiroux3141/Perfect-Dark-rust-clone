//! The runner: owns the winit event loop, the window, the frame clock, input
//! collection, audio and the debug UI, and drives a [`Game`]:
//!
//! - `init(ctx)` once the GPU exists;
//! - `tick(ctx)` at the fixed tick rate (as many times as the clock says per frame);
//! - `frame(ctx, alpha)` once per rendered frame, for per-frame work such as mouse look;
//! - `debug_ui(ctx, egui)` for the developer panels;
//! - `render(ctx, frame)` to record GPU work into the frame's encoder, inside the
//!   part of the window the panels left free (`frame.viewport`).
//!
//! Replaces the four hand-rolled `ApplicationHandler`s of the old repo (the
//! main game's and three spikes').
//! Headless tools never touch this module.

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use crate::audio::Audio;
use crate::clock::FrameClock;
use crate::debug_ui::DebugUi;
use crate::gpu::{Frame, Gpu, GpuConfig};
use crate::input::Input;

pub struct AppConfig {
    pub title: String,
    /// The window's inner size in logical pixels.
    pub size: (u32, u32),
    pub tick_hz: f64,
    /// The most ticks one rendered frame may run.
    pub max_substeps: u32,
    pub max_fps: u32,
    /// Draw only the frames that ran a tick: for a game whose picture (and
    /// whatever its render step advances) changes only on its ticks, so a
    /// display faster than the tick rate shows each tick once instead of
    /// redrawing it. A frame with no tick then skips `frame`, `debug_ui`
    /// and `render`, and the window keeps the last image presented.
    pub render_on_tick_only: bool,
    pub gpu: GpuConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig { title: "engine".into(), size: (1280, 960), tick_hz: 60.0, max_substeps: 8, max_fps: 240, render_on_tick_only: false, gpu: GpuConfig::default() }
    }
}

/// What a game can reach from inside a callback.
pub struct Ctx<'a> {
    pub gpu: &'a mut Gpu,
    pub window: &'a Window,
    pub input: &'a mut Input,
    pub audio: Option<&'a mut Audio>,
    pub clock: &'a mut FrameClock,
    exit: &'a mut bool,
}

impl Ctx<'_> {
    /// Close the window and end the loop after this frame.
    pub fn exit(&mut self) {
        *self.exit = true;
    }

    /// Capture the mouse for relative motion (locked, else confined, and hidden)
    /// or release it. Returns whether it is captured.
    pub fn set_cursor_captured(&self, on: bool) -> bool {
        use winit::window::CursorGrabMode;
        if on {
            let ok = self.window.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined)).is_ok();
            self.window.set_cursor_visible(!ok);
            ok
        } else {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
            false
        }
    }
}

pub trait Game: 'static {
    fn init(&mut self, _ctx: &mut Ctx) {}
    fn tick(&mut self, ctx: &mut Ctx);
    fn frame(&mut self, _ctx: &mut Ctx, _alpha: f32) {}
    fn debug_ui(&mut self, _ctx: &mut Ctx, _egui: &egui::Context) {}
    fn render(&mut self, ctx: &mut Ctx, frame: &mut Frame);
}

struct Live {
    window: Arc<Window>,
    gpu: Gpu,
    debug: DebugUi,
}

struct Runner<G: Game> {
    config: AppConfig,
    game: G,
    live: Option<Live>,
    input: Input,
    audio: Option<Audio>,
    clock: FrameClock,
    exit: bool,
    error: Option<String>,
}

/// Open the window and run `game` until it exits or the window closes.
pub fn run<G: Game>(config: AppConfig, game: G) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("event loop: {e}"))?;
    let clock = FrameClock::new(config.tick_hz, config.max_substeps, config.max_fps);
    let mut runner = Runner { config, game, live: None, input: Input::new(), audio: Audio::new(), clock, exit: false, error: None };
    event_loop.run_app(&mut runner).map_err(|e| format!("event loop: {e}"))?;
    match runner.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

macro_rules! ctx {
    ($r:expr, $live:expr) => {
        Ctx { gpu: &mut $live.gpu, window: &$live.window, input: &mut $r.input, audio: $r.audio.as_mut(), clock: &mut $r.clock, exit: &mut $r.exit }
    };
}

impl<G: Game> Runner<G> {
    fn frame(&mut self) {
        let Some(live) = self.live.as_mut() else { return };
        self.clock.begin_frame(Instant::now());
        let ticks = self.clock.take_ticks();
        for _ in 0..ticks {
            self.input.poll_pads();
            self.game.tick(&mut ctx!(self, live));
            self.input.end_tick();
        }
        if ticks == 0 && self.config.render_on_tick_only {
            return;
        }
        let alpha = self.clock.alpha();
        self.game.frame(&mut ctx!(self, live), alpha);
        let window = live.window.clone();
        {
            let Runner { game, input, audio, clock, exit, .. } = self;
            let Live { gpu, debug, .. } = live;
            debug.run(&window, |egui| {
                let mut ctx = Ctx { gpu, window: &window, input, audio: audio.as_mut(), clock, exit };
                game.debug_ui(&mut ctx, egui);
            });
        }
        let viewport = live.debug.free_rect();
        if let Some(mut frame) = live.gpu.begin_frame(viewport) {
            self.game.render(&mut ctx!(self, live), &mut frame);
            live.debug.paint(&live.gpu, &mut frame);
            live.gpu.end_frame(frame);
        }
    }
}

impl<G: Game> ApplicationHandler for Runner<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title(self.config.title.clone()).with_inner_size(winit::dpi::LogicalSize::new(self.config.size.0, self.config.size.1));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(format!("create window: {e}"));
                event_loop.exit();
                return;
            }
        };
        let gpu = match Gpu::new(window.clone(), &self.config.gpu) {
            Ok(g) => g,
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
                return;
            }
        };
        let debug = DebugUi::new(&window, &gpu);
        let mut live = Live { window, gpu, debug };
        self.game.init(&mut ctx!(self, live));
        self.live = Some(live);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(live) = self.live.as_mut() else { return };
        let consumed = live.debug.on_window_event(&live.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => live.gpu.resize(size.width, size.height),
            WindowEvent::Focused(false) => self.input.clear(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                let down = event.state == ElementState::Pressed;
                // A focused text field keeps its keys; releases always go through.
                if !(down && consumed && live.debug.wants_keyboard()) {
                    self.input.key_event(code, down);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                if !(down && consumed && live.debug.wants_pointer()) {
                    self.input.mouse_button_event(button, down);
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                if self.exit {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.input.mouse_motion(delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(live) = self.live.as_ref() else { return };
        let (redraw, next) = self.clock.pace(Instant::now());
        if redraw {
            live.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(next));
    }
}
