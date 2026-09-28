//! egui integration for developer panels only: egui-winit event translation plus
//! the egui-wgpu painter, drawn last at window resolution. Nothing the player sees
//! in the game or its menus goes through egui; those are the game's own drawing.
//!
//! Each frame the runner calls [`DebugUi::run`] with the game's panel code, then
//! [`DebugUi::paint`] after the game has rendered. The part of the window the
//! panels leave free ([`DebugUi::free_rect`]) is where the game draws.
//!
//! Source: the old game app's egui `Context` + `egui_winit::State` and the old
//! engine's `EguiFrame` paint pass (`Renderer::render_with_hook`).

use winit::event::WindowEvent;
use winit::window::Window;

use crate::gpu::{Frame, Gpu};

pub struct DebugUi {
    pub ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pending: Option<egui::FullOutput>,
}

impl DebugUi {
    pub fn new(window: &Window, gpu: &Gpu) -> DebugUi {
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, window, None, None, None);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, gpu.format(), None, 1, false);
        DebugUi { ctx, state, renderer, pending: None }
    }

    /// Feed a window event to egui; true if egui used it.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).consumed
    }

    /// A text field (or other widget) has the keyboard.
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    pub fn wants_pointer(&self) -> bool {
        self.ctx.wants_pointer_input()
    }

    /// Run one egui pass of the panels.
    pub fn run(&mut self, window: &Window, panels: impl FnMut(&egui::Context)) {
        let raw = self.state.take_egui_input(window);
        let out = self.ctx.run(raw, panels);
        self.state.handle_platform_output(window, out.platform_output.clone());
        self.pending = Some(out);
    }

    /// `[x, y, w, h]` in window pixels: what the last pass's panels left free.
    pub fn free_rect(&self) -> [f32; 4] {
        let r = self.ctx.available_rect();
        let s = self.ctx.pixels_per_point();
        [r.min.x * s, r.min.y * s, r.width() * s, r.height() * s]
    }

    /// Paint the last pass over the frame.
    pub fn paint(&mut self, gpu: &Gpu, frame: &mut Frame) {
        let Some(out) = self.pending.take() else { return };
        let jobs = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [frame.size.0, frame.size.1], pixels_per_point: out.pixels_per_point };
        for (id, delta) in &out.textures_delta.set {
            self.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
        }
        self.renderer.update_buffers(&gpu.device, &gpu.queue, &mut frame.encoder, &jobs, &screen);
        {
            let mut rp = frame
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("debug-ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &frame.view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut rp, &jobs, &screen);
        }
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
