//! The framebuffer effects `lv_render` draws over a player's finished view,
//! HUD included (`lv.c:1439`-`:1520`, after `player_render_hud` at `:1317`):
//! the Slayer rocket's interlace and static, the x-ray's zoom blur, the
//! Combat Boost's wipe (a zoom blur and a white fade), and the dizziness'
//! motion blur, as `pd_sim` lists them in [`ViewFx`]. `post.wgsl` draws them.
//!
//! The interlace reads this frame (PD's back buffer) and the zoom blur the last
//! one (the front buffer), so the target's colour texture is copied first and
//! kept after. `bview_draw_slayer_rocket_interlace` and `bview_draw_zoom_blur`
//! share a counter (`var8007f840`) that `bview_set_motion_blur` resets once per
//! player's view (`lv.c:1136`): the first of them draws, the rest don't.
//!
//! Source: the old repo's `pd_guns/render.rs` (`post`) and `pdpost.wgsl`, less
//! their sRGB conversions (our target holds display-space values, as the N64's
//! framebuffer does) and with PD's one-effect counter where the spike allowed two.

use bytemuck::{Pod, Zeroable};
use engine::gpu::RenderTarget;
use pd_sim::player::vision::ViewFx;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostU {
    params: [f32; 4],
    zoom: [f32; 4],
    col: [f32; 4],
}

/// Which frame a pass samples.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Src {
    None,
    This,
    Last,
}

struct Copy {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

/// The effect pipelines and the frame copies.
pub struct PostRenderer {
    /// Opaque (the interlace replaces the view) and alpha-blended.
    pipes: [wgpu::RenderPipeline; 2],
    bgl: wgpu::BindGroupLayout,
    buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    this: Option<Copy>,
    last: Option<Copy>,
    blank: wgpu::TextureView,
    /// The static's rows (PD's `random()` picks them; the renderer keeps its
    /// own count so drawing never touches the world's stream).
    seed: u32,
}

/// The most passes one view draws: interlace or blur, two statics, a fade.
const MAX_PASSES: u64 = 8;

impl PostRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> PostRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pd-post"), source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pd-post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pd-post"), bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let pipe = |blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("pd-post"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipes = [pipe(None), pipe(Some(wgpu::BlendState::ALPHA_BLENDING))];
        // One 256-byte slot per pass (uniform offsets are 256-aligned).
        let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("pd-post"), size: 256 * MAX_PASSES, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        // bview_prepare_static_*: G_TF_POINT.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pd-post"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let blank = copy_texture(device, format, (1, 1)).view;
        PostRenderer { pipes, bgl, buf, sampler, format, this: None, last: None, blank, seed: 0 }
    }

    /// Draw `fx` over `target` (the view is `lines` PD lines by `width` PD
    /// pixels; `frac20` is `g_20SecIntervalFrac`), then keep the result as the
    /// next frame's front buffer.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &RenderTarget, fx: &ViewFx, lines: f32, width: f32, frac20: f32) {
        self.seed = self.seed.wrapping_add(1);
        let size = (target.width, target.height);
        let last_ok = self.last.as_ref().is_some_and(|c| c.size == size);
        let mut passes: Vec<(PostU, bool, Src)> = Vec::new();
        let mut budget = 1;
        let u = |mode: f32, z: f32, alpha: f32, zoom: [f32; 2], col: [f32; 3]| PostU { params: [mode, lines, z, alpha], zoom: [zoom[0], zoom[1], width, 0.0], col: [col[0], col[1], col[2], 0.0] };
        if fx.slayer_interlace {
            budget -= 1;
            let offset = ((frac20 * 600.0) as i32 % 12) as f32;
            passes.push((u(0.0, offset, 1.0, [1.0; 2], [0.0; 3]), false, Src::This));
        }
        if fx.static_alpha > 0 {
            // bview_prepare_static_i8: env alpha `alpha & 0xff`.
            let alpha = (fx.static_alpha & 0xff) as f32 / 255.0;
            passes.push((u(1.0, (self.seed % 997) as f32, alpha, [1.0; 2], [0.0; 3]), true, Src::None));
        }
        // bview_set_motion_blur (`bondview.c:2526`): the dizziness adds 2/3 of
        // its blur to the first zoom blur drawn (`:274`, capped at 230).
        let mut extra = (fx.motion_blur.max(0) as u32 * 2) / 3;
        for &(alpha, sx, sy) in &fx.zoom_blurs {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let alpha = ((alpha & 0xff) + extra).min(230);
            extra = 0;
            // SUBST: PD's first frame blurs whatever the front buffer held / we
            // have no last frame then, and skip it.
            if last_ok {
                passes.push((u(2.0, 0.0, alpha as f32 / 255.0, [sx, sy], [0.0; 3]), true, Src::Last));
            }
        }
        // bview_draw_motion_blur(0xffffffff, bluramount) (`lv.c:1522`): the last
        // frame over this one 1:1, unless a zoom blur already drew.
        if fx.motion_blur > 0 && budget > 0 && last_ok {
            let alpha = (fx.motion_blur as u32).min(230);
            passes.push((u(2.0, 0.0, alpha as f32 / 255.0, [1.0, 1.0], [0.0; 3]), true, Src::Last));
        }
        if let Some((rgb, frac)) = fx.fade.filter(|f| f.1 > 0.0) {
            // gDPSetPrimColor(.., (s32)(frac * 255))
            passes.push((u(3.0, 0.0, (frac * 255.0) as i32 as f32 / 255.0, [1.0; 2], rgb.map(|c| c as f32 / 255.0)), true, Src::None));
        }

        if passes.iter().any(|p| p.2 == Src::This) {
            let c = self.copy_slot(device, true, size);
            encoder.copy_texture_to_texture(target.color.as_image_copy(), c.tex.as_image_copy(), extent(size));
        }
        for (i, (pu, _, _)) in passes.iter().enumerate().take(MAX_PASSES as usize) {
            queue.write_buffer(&self.buf, i as u64 * 256, bytemuck::bytes_of(pu));
        }
        for (i, (_, blended, src)) in passes.iter().enumerate().take(MAX_PASSES as usize) {
            let view = match src {
                Src::This => &self.this.as_ref().unwrap().view,
                Src::Last => &self.last.as_ref().unwrap().view,
                Src::None => &self.blank,
            };
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("pd-post"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.buf, offset: i as u64 * 256, size: std::num::NonZeroU64::new(std::mem::size_of::<PostU>() as u64) }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pd-post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &target.view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.pipes[*blended as usize]);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        // The finished frame is the next one's front buffer.
        let c = self.copy_slot(device, false, size);
        encoder.copy_texture_to_texture(target.color.as_image_copy(), c.tex.as_image_copy(), extent(size));
    }

    fn copy_slot(&mut self, device: &wgpu::Device, this: bool, size: (u32, u32)) -> &Copy {
        let format = self.format;
        let slot = if this { &mut self.this } else { &mut self.last };
        if slot.as_ref().is_none_or(|c| c.size != size) {
            *slot = Some(copy_texture(device, format, size));
        }
        slot.as_ref().unwrap()
    }
}

fn extent(size: (u32, u32)) -> wgpu::Extent3d {
    wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 }
}

fn copy_texture(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Copy {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pd-post-copy"),
        size: extent(size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    Copy { tex, view, size }
}
