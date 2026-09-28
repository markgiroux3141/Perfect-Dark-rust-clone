//! GPU plumbing, with no game pipelines of its own:
//! - [`Gpu`]: instance, adapter, device, queue, surface and format; backend,
//!   present mode and sRGB come from [`GpuConfig`], not environment variables;
//! - [`RenderTarget`]: an offscreen colour texture at any size (e.g. 320×220),
//!   with an optional depth buffer, that the CPU can upload into;
//! - [`Frame`]: the swapchain image, its command encoder, and the part of the
//!   window the debug UI left free;
//! - [`Presenter`]: scale a low-resolution target to the window (nearest or
//!   linear) inside a letterboxed canvas. Low-resolution rendering is first-class,
//!   not a hook.
//!
//! Also [`validate_wgsl`], the naga check the shader tests in every crate use.
//!
//! Source: the old engine's `Renderer::new` / `resize` / frame acquisition and
//! its backend and present-mode choice (`pick_backends`, `pick_present_mode`),
//! without the renderer.

use std::sync::Arc;

use winit::window::Window;

/// Which graphics API to ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Backend {
    /// wgpu's choice for the platform.
    #[default]
    Auto,
    Vulkan,
    Dx12,
    Gl,
}

/// Presentation, lowest latency first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PresentPref {
    /// Mailbox where available, else vsync.
    #[default]
    LowLatency,
    Vsync,
    Immediate,
}

#[derive(Clone, Copy, Debug)]
pub struct GpuConfig {
    pub backend: Backend,
    pub present: PresentPref,
    /// Prefer an sRGB swapchain (the hardware encodes linear shader output).
    /// A game whose colours are already display-space values wants `false`.
    pub srgb: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        GpuConfig { backend: Backend::Auto, present: PresentPref::LowLatency, srgb: false }
    }
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    pub fn new(window: Arc<Window>, cfg: &GpuConfig) -> Result<Gpu, String> {
        let backends = match cfg.backend {
            Backend::Auto => wgpu::Backends::PRIMARY,
            Backend::Vulkan => wgpu::Backends::VULKAN,
            Backend::Dx12 => wgpu::Backends::DX12,
            Backend::Gl => wgpu::Backends::GL,
        };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends, ..Default::default() });
        let surface = instance.create_surface(window.clone()).map_err(|e| format!("create surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or("no GPU adapter for this window")?;
        log::info!("gpu: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor { label: Some("engine-device"), required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), memory_hints: wgpu::MemoryHints::default() },
            None,
        ))
        .map_err(|e| format!("request device: {e}"))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| f.is_srgb() == cfg.srgb).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: pick_present_mode(cfg.present, &caps.present_modes),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            // 1: the lowest input-to-photon latency (the GPU may not queue ahead).
            desired_maximum_frame_latency: 1,
        };
        log::info!("gpu: surface {:?}, {:?}", config.format, config.present_mode);
        surface.configure(&device, &config);
        Ok(Gpu { instance, adapter, device, queue, surface, config })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// The next swapchain image, or `None` this frame (the surface is then
    /// reconfigured; a minimised window has no image).
    pub fn begin_frame(&mut self, viewport: [f32; 4]) -> Option<Frame> {
        let surface_tex = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Timeout) => return None,
            Err(e) => {
                log::debug!("gpu: {e}; reconfiguring the surface");
                self.surface.configure(&self.device, &self.config);
                return None;
            }
        };
        let view = surface_tex.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        Some(Frame { surface_tex, view, encoder, size: self.size(), viewport })
    }

    pub fn end_frame(&self, frame: Frame) {
        self.queue.submit(std::iter::once(frame.encoder.finish()));
        frame.surface_tex.present();
    }
}

fn pick_present_mode(pref: PresentPref, available: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    use wgpu::PresentMode::*;
    let order: &[wgpu::PresentMode] = match pref {
        PresentPref::LowLatency => &[Mailbox, Fifo],
        PresentPref::Vsync => &[Fifo],
        PresentPref::Immediate => &[Immediate, Mailbox, Fifo],
    };
    order.iter().copied().find(|p| available.contains(p)).unwrap_or(Fifo)
}

/// One rendered frame.
pub struct Frame {
    surface_tex: wgpu::SurfaceTexture,
    /// The swapchain image.
    pub view: wgpu::TextureView,
    pub encoder: wgpu::CommandEncoder,
    /// The window's size in pixels.
    pub size: (u32, u32),
    /// `[x, y, w, h]` in pixels: the part of the window the debug UI left free.
    pub viewport: [f32; 4],
}

/// An offscreen colour target (and optional depth) at its own resolution.
pub struct RenderTarget {
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub color: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub depth: Option<(wgpu::Texture, wgpu::TextureView)>,
}

impl RenderTarget {
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    pub fn new(gpu: &Gpu, width: u32, height: u32, format: wgpu::TextureFormat, with_depth: bool) -> RenderTarget {
        Self::on_device(&gpu.device, width, height, format, with_depth)
    }

    /// A target on any device (a window's or a [`HeadlessGpu`]).
    pub fn on_device(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat, with_depth: bool) -> RenderTarget {
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render-target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = with_depth.then(|| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("render-target-depth"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Self::DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            (t, v)
        });
        RenderTarget { width, height, format, color, view, depth }
    }

    /// Replace the whole colour texture with tightly packed 4-byte pixels.
    pub fn upload(&self, gpu: &Gpu, pixels: &[u8]) {
        assert_eq!(pixels.len(), (self.width * self.height * 4) as usize, "RenderTarget::upload: size");
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.width * 4), rows_per_image: Some(self.height) },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
    }
}

impl RenderTarget {
    /// Read the colour texture back as tightly packed 4-byte pixels (blocking).
    pub fn read_rgba8(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let (w, h) = (self.width, self.height);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("render-target-read"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("render-target-read") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            out.extend_from_slice(&data[y * row as usize..y * row as usize + (w * 4) as usize]);
        }
        out
    }
}

/// A device and queue without a window, for offscreen rendering (tools, tests).
pub struct HeadlessGpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl HeadlessGpu {
    pub fn new() -> Result<HeadlessGpu, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::PRIMARY, ..Default::default() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: None, force_fallback_adapter: false }))
            .ok_or("no GPU adapter")?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("headless-device"), ..Default::default() }, None)).map_err(|e| format!("request device: {e}"))?;
        Ok(HeadlessGpu { device, queue })
    }
}

/// How [`Presenter::present`] scales.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Linear,
}

/// Where a `src`-sized image goes in a `viewport`: the `canvas` (≥ `src`,
/// square pixels) scaled to fit with its aspect kept, the image centred in it.
/// Returns `[x, y, w, h]` in window pixels.
pub fn letterbox(viewport: [f32; 4], canvas: (u32, u32), src: (u32, u32)) -> [f32; 4] {
    let [vx, vy, vw, vh] = viewport;
    let (cw, ch) = (canvas.0.max(src.0) as f32, canvas.1.max(src.1) as f32);
    let scale = (vw / cw).min(vh / ch).max(0.0);
    let (w, h) = (src.0 as f32 * scale, src.1 as f32 * scale);
    [vx + (vw - w) * 0.5, vy + (vh - h) * 0.5, w, h]
}

const PRESENT_WGSL: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the viewport.
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn fs(v: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(tex, samp, v.uv).rgb, 1.0);
}
"#;

/// Draws a texture onto the swapchain, letterboxed, over a black clear.
pub struct Presenter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
}

impl Presenter {
    pub fn new(gpu: &Gpu) -> Presenter {
        let d = &gpu.device;
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present"), source: wgpu::ShaderSource::Wgsl(PRESENT_WGSL.into()) });
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("present"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format: gpu.format(), blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = |f: wgpu::FilterMode| {
            d.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("present"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: f,
                min_filter: f,
                ..Default::default()
            })
        };
        Presenter { pipeline, layout, nearest: sampler(wgpu::FilterMode::Nearest), linear: sampler(wgpu::FilterMode::Linear) }
    }

    /// Clear the window black and draw `src` into the frame's free viewport:
    /// `canvas` is the picture the image sits in (e.g. 220 lines inside a
    /// 240-line frame), scaled to fit with its aspect kept.
    pub fn present(&self, gpu: &Gpu, frame: &mut Frame, src: &RenderTarget, canvas: (u32, u32), filter: Filter) {
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(if filter == Filter::Nearest { &self.nearest } else { &self.linear }) },
            ],
        });
        let [x, y, w, h] = letterbox(frame.viewport, canvas, (src.width, src.height));
        let mut rp = frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if w >= 1.0 && h >= 1.0 {
            let (fw, fh) = (frame.size.0 as f32, frame.size.1 as f32);
            let (x0, y0) = (x.clamp(0.0, fw), y.clamp(0.0, fh));
            rp.set_viewport(x0, y0, w.min(fw - x0), h.min(fh - y0), 0.0, 1.0);
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

/// Parse and validate WGSL with naga (the front end wgpu runs), so a shader typo
/// fails a test instead of the game's first frame.
pub fn validate_wgsl(src: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(src).map_err(|e| e.emit_to_string(src))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_present_shader_validates() {
        validate_wgsl(PRESENT_WGSL).unwrap();
    }

    #[test]
    fn letterbox_keeps_the_canvas_aspect_and_centres_the_image() {
        // 320×220 inside a 320×240 picture, on a 1600×900 window: the picture
        // is 1200×900, the image 1200×825, centred.
        let r = letterbox([0.0, 0.0, 1600.0, 900.0], (320, 240), (320, 220));
        assert_eq!(r, [200.0, 37.5, 1200.0, 825.0]);
        // A viewport left of a 300-pixel debug panel.
        let r = letterbox([300.0, 0.0, 1300.0, 975.0], (320, 240), (320, 240));
        assert_eq!(r, [300.0, 0.0, 1300.0, 975.0]);
    }
}
