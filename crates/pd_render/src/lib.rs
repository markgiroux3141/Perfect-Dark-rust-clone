//! Perfect Dark on the GPU. It reads `pd_sim` views and never writes to the world.
//! Every model (the BG, guns, hands, simulant bodies and heads, props) goes through
//! the one `n64::gpu` combiner path, so simulants are lit and textured the way the
//! guns and the level are. In the spike match they were drawn flat by the old
//! engine's glTF path.
//!
//! One `View` per human player (viewport, camera, fov), so split-screen needs no
//! special case.

pub mod bg;
pub mod fx;
pub mod hud;
pub mod models;
pub mod post;
pub mod view;
pub mod xray;

use n64::gpu::Combiner;
use pd_core::assets::AssetDir;

pub use view::View;

/// The depth format every pass uses.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Draws a match: the combiner pipelines and the loaded stage. M3 draws the BG;
/// the chrs, guns, effects and HUD join it (M4-M6).
pub struct Renderer {
    pub combiner: Combiner,
    pub bg: Option<bg::StageBg>,
}

impl Renderer {
    /// `color_format` must not be sRGB: the combiner writes display-space values.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat) -> Renderer {
        Renderer { combiner: Combiner::new(device, queue, color_format, DEPTH_FORMAT), bg: None }
    }

    pub fn load_stage(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetDir, code: &str) -> Result<(), String> {
        self.bg = Some(bg::StageBg::load(device, queue, &mut self.combiner, assets, code)?);
        Ok(())
    }

    /// The loaded stage's z range, or PD's title-screen default (100, 10000).
    pub fn z_range(&self) -> (f32, f32) {
        self.bg.as_ref().map_or((100.0, 10000.0), |b| (b.env.near, b.env.far))
    }

    /// One view of the world into `color` + `depth` (same size), cleared first.
    pub fn render(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, view: &View) {
        let sky = self.bg.as_ref().map_or([0.0; 3], |b| b.sky());
        if let Some(bg) = &self.bg {
            bg.prepare(queue, view);
        }
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pd-world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: sky[0], g: sky[1], b: sky[2], a: 1.0 }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if let Some(bg) = &self.bg {
            bg.draw(&mut rp, &self.combiner);
        }
    }
}
