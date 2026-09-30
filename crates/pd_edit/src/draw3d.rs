//! The editor's own 3D overlay: flat-coloured triangles and lines (markers,
//! the waypoint graph, the gizmo) drawn into the view after `pd_render` has
//! drawn the stage, against its depth buffer. Three depth modes: in the scene
//! (hidden behind walls), faint where hidden (to find things through walls),
//! and on top of everything (the gizmo).

use engine::gpu::RenderTarget;
use engine::wgpu;
use engine::wgpu::util::DeviceExt;
use glam::{Mat4, Vec3};

/// A vertex: world position, RGBA (0..1, display space).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct V {
    pub pos: [f32; 3],
    pub col: [f32; 4],
}

/// What to draw this frame.
#[derive(Default)]
pub struct Mesh3d {
    /// In the scene: depth-tested and written.
    pub tris: Vec<V>,
    pub lines: Vec<V>,
    /// Over everything (the gizmo).
    pub top_tris: Vec<V>,
    pub top_lines: Vec<V>,
}

pub fn rgba(c: [u8; 4]) -> [f32; 4] {
    c.map(|x| x as f32 / 255.0)
}

impl Mesh3d {
    pub fn tri(&mut self, top: bool, a: Vec3, b: Vec3, c: Vec3, col: [f32; 4]) {
        let t = if top { &mut self.top_tris } else { &mut self.tris };
        t.extend([a, b, c].map(|p| V { pos: p.to_array(), col }));
    }

    pub fn line(&mut self, top: bool, a: Vec3, b: Vec3, col: [f32; 4]) {
        let l = if top { &mut self.top_lines } else { &mut self.lines };
        l.extend([a, b].map(|p| V { pos: p.to_array(), col }));
    }

    pub fn quad(&mut self, top: bool, a: Vec3, b: Vec3, c: Vec3, d: Vec3, col: [f32; 4]) {
        self.tri(top, a, b, c, col);
        self.tri(top, a, c, d, col);
    }

    /// A solid box, its sides shaded a little so it reads as a box.
    pub fn cube(&mut self, top: bool, lo: Vec3, hi: Vec3, col: [f32; 4]) {
        let p = |x: bool, y: bool, z: bool| Vec3::new(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z });
        let shade = |k: f32| [col[0] * k, col[1] * k, col[2] * k, col[3]];
        self.quad(top, p(false, true, false), p(true, true, false), p(true, true, true), p(false, true, true), col);
        self.quad(top, p(false, false, false), p(false, false, true), p(true, false, true), p(true, false, false), shade(0.5));
        self.quad(top, p(false, false, false), p(true, false, false), p(true, true, false), p(false, true, false), shade(0.8));
        self.quad(top, p(false, false, true), p(false, true, true), p(true, true, true), p(true, false, true), shade(0.8));
        self.quad(top, p(false, false, false), p(false, true, false), p(false, true, true), p(false, false, true), shade(0.65));
        self.quad(top, p(true, false, false), p(true, false, true), p(true, true, true), p(true, true, false), shade(0.65));
    }

    /// A box's twelve edges.
    pub fn wire_box(&mut self, top: bool, lo: Vec3, hi: Vec3, col: [f32; 4]) {
        let p = |x: bool, y: bool, z: bool| Vec3::new(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z });
        for (a, b) in [
            (p(false, false, false), p(true, false, false)),
            (p(false, false, true), p(true, false, true)),
            (p(false, true, false), p(true, true, false)),
            (p(false, true, true), p(true, true, true)),
            (p(false, false, false), p(false, false, true)),
            (p(true, false, false), p(true, false, true)),
            (p(false, true, false), p(false, true, true)),
            (p(true, true, false), p(true, true, true)),
            (p(false, false, false), p(false, true, false)),
            (p(true, false, false), p(true, true, false)),
            (p(false, false, true), p(false, true, true)),
            (p(true, false, true), p(true, true, true)),
        ] {
            self.line(top, a, b, col);
        }
    }

    /// A flat ring round `c` (in the plane y = c.y) from radius `r0` to `r1`.
    pub fn ring(&mut self, top: bool, c: Vec3, r0: f32, r1: f32, col: [f32; 4]) {
        const SEGS: usize = 48;
        let at = |k: usize, r: f32| {
            let a = k as f32 / SEGS as f32 * std::f32::consts::TAU;
            c + Vec3::new(a.sin() * r, 0.0, a.cos() * r)
        };
        for k in 0..SEGS {
            self.quad(top, at(k, r0), at(k + 1, r0), at(k + 1, r1), at(k, r1), col);
            self.quad(top, at(k, r1), at(k + 1, r1), at(k + 1, r0), at(k, r0), col);
        }
    }
}

const WGSL: &str = r#"
struct U { vp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct VIn { @location(0) pos: vec3<f32>, @location(1) col: vec4<f32> };
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) col: vec4<f32> };
@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.pos = u.vp * vec4<f32>(v.pos, 1.0);
    o.col = v.col;
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    return i.col;
}
"#;

#[derive(Clone, Copy)]
enum Depth {
    Scene,
    Hidden,
    Top,
}

pub struct Draw3d {
    /// (triangles, lines) for each depth mode.
    pipes: [(wgpu::RenderPipeline, wgpu::RenderPipeline); 3],
    layout: wgpu::BindGroupLayout,
}

impl Draw3d {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Draw3d {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pd_edit draw3d"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pd_edit draw3d"),
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pd_edit draw3d"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipe = |topology: wgpu::PrimitiveTopology, depth: Depth| {
            let (compare, write) = match depth {
                Depth::Scene => (wgpu::CompareFunction::LessEqual, true),
                Depth::Hidden => (wgpu::CompareFunction::Greater, false),
                Depth::Top => (wgpu::CompareFunction::Always, false),
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("pd_edit draw3d"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<V>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: RenderTarget::DEPTH_FORMAT, depth_write_enabled: write, depth_compare: compare, stencil: Default::default(), bias: Default::default() }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let both = |d: Depth| (pipe(wgpu::PrimitiveTopology::TriangleList, d), pipe(wgpu::PrimitiveTopology::LineList, d));
        Draw3d { pipes: [both(Depth::Scene), both(Depth::Hidden), both(Depth::Top)], layout }
    }

    /// Draw `mesh` over `target` (its colour and depth as the stage left
    /// them) with world → clip `vp`; `ghost`: what walls hide drawn faintly.
    pub fn draw(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, target: &RenderTarget, vp: Mat4, mesh: &Mesh3d, ghost: bool) {
        let Some((_, depth)) = &target.depth else { return };
        let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("pd_edit draw3d"), contents: bytemuck::cast_slice(&vp.to_cols_array()), usage: wgpu::BufferUsages::UNIFORM });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("pd_edit draw3d"), layout: &self.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }] });
        let faint = |v: &[V]| -> Vec<V> { v.iter().map(|x| V { pos: x.pos, col: [x.col[0], x.col[1], x.col[2], x.col[3] * 0.3] }).collect() };
        let (gt, gl) = if ghost { (faint(&mesh.tris), faint(&mesh.lines)) } else { (Vec::new(), Vec::new()) };
        // (vertices, mode index, lines?)
        let lists: [(&[V], usize, bool); 6] = [(&mesh.tris, 0, false), (&mesh.lines, 0, true), (&gt, 1, false), (&gl, 1, true), (&mesh.top_tris, 2, false), (&mesh.top_lines, 2, true)];
        let bufs: Vec<Option<wgpu::Buffer>> = lists
            .iter()
            .map(|(v, _, _)| (!v.is_empty()).then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("pd_edit draw3d"), contents: bytemuck::cast_slice(v), usage: wgpu::BufferUsages::VERTEX })))
            .collect();
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pd_edit draw3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &target.view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_bind_group(0, &bind, &[]);
        for ((v, mode, lines), buf) in lists.iter().zip(&bufs) {
            let Some(buf) = buf else { continue };
            let (tri, line) = &self.pipes[*mode];
            rp.set_pipeline(if *lines { line } else { tri });
            rp.set_vertex_buffer(0, buf.slice(..));
            rp.draw(0..v.len() as u32, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shader_validates() {
        engine::gpu::validate_wgsl(super::WGSL).unwrap();
    }
}
