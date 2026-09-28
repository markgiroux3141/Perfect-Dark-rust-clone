//! (feature `gpu`) The N64's 3D path on the GPU: the RSP vertex stage and the
//! RDP combiner and blender ([`Combiner`], `combiner.wgsl`), for triangles
//! described exactly as the CPU rasteriser takes them: an [`rdp::DrawState`]
//! per material and an [`rdp::MipTex`] per texture. The CPU rasteriser is the
//! oracle; this is the same state, drawn by wgpu.
//!
//! What a draw needs:
//! - a [`Vertex`] buffer (position in the model's space, the matrix slot the
//!   display list loaded, texels, colour or normal, lit/texgen flags);
//! - a [`FrameSlot`]: the projection, lights and render-context colours
//!   ([`FrameUniform`]), and the matrix palette (`joints[mtx]`: model → eye);
//! - a [`Material`] from [`Combiner::material`], and the pipeline for its
//!   [`PipeKey`] (blend, z mode, cull), made with [`Combiner::prepare`] before
//!   the render pass.
//!
//! Rendering targets are not sRGB: the combiner writes raw display-space values
//! and the blender mixes them, as the RDP does.
//!
//! Later (M5): the VI + CRT chain (RGBA5551 + Bayer store, VI dither filter,
//! divot, composite/S-Video encode) from the old repo's `pd_guns/n64video.rs`.
//!
//! Source: the old repo's `pd_guns/render.rs` (`PdRenderer`'s gun pipeline) and
//! `pdgun.wgsl`.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use crate::rdp::{self, AlphaTest, Cull, DrawState, MipTex};

/// The combiner's WGSL (validated by the tests).
pub const COMBINER_WGSL: &str = include_str!("combiner.wgsl");

/// How many matrix slots one draw's palette holds.
pub const MAX_JOINTS: usize = 128;

/// One vertex as the display list loaded it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    /// Texels (or, with texgen, the texture scale / 32).
    pub uv: [f32; 2],
    /// A colour 0..255, or when lit a signed normal (bytes) and alpha.
    pub col: [f32; 4],
    /// The matrix slot (`joints[mtx]`).
    pub mtx: u32,
    /// 1 = lit (`G_LIGHTING`), 2 = texgen (`G_TEXTURE_GEN`).
    pub flags: u32,
}

/// The per-draw uniform: projection, the RSP lights, the render context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FrameUniform {
    /// Eye space → clip space.
    pub proj: [[f32; 4]; 4],
    /// Ambient and directional light colour, 0..255.
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    /// The light's direction in eye space, as the display list's bytes / 127.
    pub light_dir: [f32; 4],
    /// The LookAt vectors texgen reads, in eye space.
    pub lookat_x: [f32; 4],
    pub lookat_y: [f32; 4],
    /// The render context's env colour (0..1), for materials that take it.
    pub envcol: [f32; 4],
    /// A flat colour override (alpha 0 = off).
    pub flat: [f32; 4],
    /// x: env-alpha override (0 = off); y: 1 = the RDP's 3-point filter.
    pub misc: [f32; 4],
}

impl FrameUniform {
    /// An unlit draw through `proj`: white light, no overrides.
    pub fn new(proj: Mat4) -> FrameUniform {
        FrameUniform {
            proj: proj.to_cols_array_2d(),
            ambient: [255.0, 255.0, 255.0, 0.0],
            diffuse: [0.0; 4],
            light_dir: [0.0, 0.0, 1.0, 0.0],
            lookat_x: [1.0, 0.0, 0.0, 0.0],
            lookat_y: [0.0, 1.0, 0.0, 0.0],
            envcol: [1.0; 4],
            flat: [0.0; 4],
            misc: [0.0; 4],
        }
    }

    /// The RSP's LookAt vectors from the camera's look and up (eye space is the
    /// camera's, so these are the view's x and y axes as seen from world space).
    pub fn with_lookat(mut self, look: Vec3, up: Vec3) -> FrameUniform {
        let lx = look.cross(up).normalize_or_zero();
        let ly = (-look).cross(lx).normalize_or_zero();
        self.lookat_x = lx.extend(0.0).to_array();
        self.lookat_y = ly.extend(0.0).to_array();
        self
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    cc0: [u32; 4],
    ac0: [u32; 4],
    cc1: [u32; 4],
    ac1: [u32; 4],
    prim: [f32; 4],
    env: [f32; 4],
    fog: [f32; 4],
    tex: [f32; 4],
    shift: [f32; 4],
    flags: [u32; 4],
    flags2: [u32; 4],
}

/// What a material needs beyond the [`DrawState`] the CPU rasteriser takes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Extras {
    /// Take the env colour from the frame ([`FrameUniform::envcol`]) rather than
    /// the draw state (a material that never set one).
    pub env_from_frame: bool,
    /// Cycle 1 of a two-cycle blender is `G_RM_FOG_PRIM_A`: the fog colour tints.
    pub fog_tint: bool,
    /// The fog colour, or `None` for the frame's env colour.
    pub fog: Option<[f32; 4]>,
    /// `G_TEXTURE_GEN_LINEAR`.
    pub texgen_linear: bool,
}

/// Which pipeline a draw uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PipeKey {
    /// The blender's `M = CLR_MEM, B = 1MA`.
    pub alpha: bool,
    pub zwrite: bool,
    pub ztest: bool,
    /// Z compare `ZMODE_DEC`: drawn over coplanar geometry.
    pub decal: bool,
    /// `Cull::Both` draws nothing; callers skip it.
    pub cull: CullKey,
}

/// [`Cull`] as a hashable key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CullKey {
    None,
    Back,
    Front,
}

impl CullKey {
    /// `None` for `Cull::Both`, which the RSP culls entirely.
    pub fn from_cull(c: Cull) -> Option<CullKey> {
        match c {
            Cull::None => Some(CullKey::None),
            Cull::Back => Some(CullKey::Back),
            Cull::Front => Some(CullKey::Front),
            Cull::Both => None,
        }
    }
}

/// A texture on the GPU: every level of an [`rdp::MipTex`].
#[derive(Clone)]
pub struct Texture {
    pub view: wgpu::TextureView,
    pub w: u32,
    pub h: u32,
}

/// A material's bind group and the part of its pipeline key the state fixes.
pub struct Material {
    pub bind: wgpu::BindGroup,
    pub alpha: bool,
    pub zwrite: bool,
    pub ztest: bool,
    pub decal: bool,
}

impl Material {
    pub fn key(&self, cull: CullKey) -> PipeKey {
        PipeKey { alpha: self.alpha, zwrite: self.zwrite, ztest: self.ztest, decal: self.decal, cull }
    }
}

/// One draw's frame uniform and matrix palette.
pub struct FrameSlot {
    pub frame_buf: wgpu::Buffer,
    pub joint_buf: wgpu::Buffer,
    pub bind: wgpu::BindGroup,
}

impl FrameSlot {
    pub fn write(&self, queue: &wgpu::Queue, frame: &FrameUniform, joints: &[Mat4]) {
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(frame));
        let n = joints.len().min(MAX_JOINTS);
        let arr: Vec<[[f32; 4]; 4]> = joints[..n].iter().map(|m| m.to_cols_array_2d()).collect();
        if !arr.is_empty() {
            queue.write_buffer(&self.joint_buf, 0, bytemuck::cast_slice(&arr));
        }
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }
}

/// The combiner pipeline family: layouts, the shader, pipelines by [`PipeKey`],
/// samplers by tile mode.
pub struct Combiner {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    frame_bgl: wgpu::BindGroupLayout,
    mat_bgl: wgpu::BindGroupLayout,
    pipes: HashMap<PipeKey, wgpu::RenderPipeline>,
    samplers: HashMap<(u8, u8, bool, bool), wgpu::Sampler>,
    white: Texture,
    pub color_format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
}

fn addr_code(a: rdp::Addr) -> u8 {
    match a {
        rdp::Addr::Wrap => 0,
        rdp::Addr::Clamp => 1,
        rdp::Addr::Mirror => 2,
    }
}

impl Combiner {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat, depth_format: wgpu::TextureFormat) -> Combiner {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("n64-combiner"), source: wgpu::ShaderSource::Wgsl(COMBINER_WGSL.into()) });
        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("n64-frame"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let mat_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("n64-material"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("n64-combiner"), bind_group_layouts: &[&frame_bgl, &mat_bgl], push_constant_ranges: &[] });
        let white = upload_levels(device, queue, &[(1, 1, vec![255, 255, 255, 255])], "n64-white");
        Combiner { shader, layout, frame_bgl, mat_bgl, pipes: HashMap::new(), samplers: HashMap::new(), white, color_format, depth_format }
    }

    /// Upload every level of `tex`, as the CPU rasteriser samples them.
    pub fn texture(&self, device: &wgpu::Device, queue: &wgpu::Queue, tex: &MipTex) -> Texture {
        let levels: Vec<(u32, u32, Vec<u8>)> = tex
            .levels
            .iter()
            .map(|(w, h, px)| (*w as u32, *h as u32, px.iter().flat_map(|c| c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)).collect()))
            .collect();
        upload_levels(device, queue, &levels, "n64-texture")
    }

    fn sampler(&mut self, device: &wgpu::Device, cms: u8, cmt: u8, linear: bool, mip: bool) -> wgpu::Sampler {
        let key = (cms, cmt, linear, mip);
        if let Some(s) = self.samplers.get(&key) {
            return s.clone();
        }
        let mode = |m: u8| match m {
            1 => wgpu::AddressMode::ClampToEdge,
            2 => wgpu::AddressMode::MirrorRepeat,
            _ => wgpu::AddressMode::Repeat,
        };
        let f = if linear { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
        let s = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("n64-sampler"),
            address_mode_u: mode(cms),
            address_mode_v: mode(cmt),
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: f,
            min_filter: f,
            // TRILERP: the sampler's trilinear filter stands for TEXEL1 at the
            // next level and LOD_FRACTION (see the shader).
            mipmap_filter: if mip { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest },
            lod_max_clamp: if mip { 32.0 } else { 0.0 },
            ..Default::default()
        });
        self.samplers.insert(key, s.clone());
        s
    }

    /// A material for `st` (its `tex` is ignored: pass the GPU copy as `tex`).
    pub fn material(&mut self, device: &wgpu::Device, st: &DrawState<'_>, tex: Option<&Texture>, extras: &Extras) -> Material {
        let t = &st.tile;
        let (view, size, has_tex) = match tex {
            Some(t) => (t.view.clone(), [t.w as f32, t.h as f32], 1.0),
            None => (self.white.view.clone(), [1.0, 1.0], 0.0),
        };
        let sampler = self.sampler(device, addr_code(t.cms), addr_code(t.cmt), t.bilerp, t.mipmap && tex.is_some());
        let c = st.combine;
        let alpha_test = match st.alpha_test {
            AlphaTest::None => 0,
            AlphaTest::Edge => 1,
            AlphaTest::Threshold => 2,
        };
        let u = MaterialUniform {
            cc0: [c[0] as u32, c[1] as u32, c[2] as u32, c[3] as u32],
            ac0: [c[4] as u32, c[5] as u32, c[6] as u32, c[7] as u32],
            cc1: [c[8] as u32, c[9] as u32, c[10] as u32, c[11] as u32],
            ac1: [c[12] as u32, c[13] as u32, c[14] as u32, c[15] as u32],
            prim: st.prim,
            env: st.env,
            fog: extras.fog.unwrap_or([1.0, 1.0, 1.0, 0.0]),
            tex: [size[0], size[1], t.ul[0], t.ul[1]],
            shift: [t.shift[0], t.shift[1], has_tex, if st.two_cycle { 1.0 } else { 0.0 }],
            flags: [alpha_test, extras.fog_tint as u32, extras.env_from_frame as u32, extras.fog.is_none() as u32],
            flags2: [extras.texgen_linear as u32, 0, st.xlu as u32, (has_tex > 0.0 && t.bilerp) as u32],
        };
        let buf = wgpu::util::DeviceExt::create_buffer_init(device, &wgpu::util::BufferInitDescriptor { label: Some("n64-material"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("n64-material"),
            layout: &self.mat_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        Material { bind, alpha: st.xlu, zwrite: st.zwrite, ztest: st.ztest, decal: st.decal }
    }

    /// A frame uniform and a [`MAX_JOINTS`] palette for one draw.
    pub fn frame_slot(&self, device: &wgpu::Device) -> FrameSlot {
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("n64-frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let joint_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("n64-joints"),
            size: (MAX_JOINTS * 64) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("n64-frame"),
            layout: &self.frame_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: joint_buf.as_entire_binding() }],
        });
        FrameSlot { frame_buf, joint_buf, bind }
    }

    /// Make the pipeline for `key` if it does not exist yet (before a pass).
    pub fn prepare(&mut self, device: &wgpu::Device, key: PipeKey) {
        if self.pipes.contains_key(&key) {
            return;
        }
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Uint32, 4 => Uint32];
        let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("n64-combiner"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.color_format,
                    blend: if key.alpha { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None },
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: match key.cull {
                    CullKey::None => None,
                    CullKey::Back => Some(wgpu::Face::Back),
                    CullKey::Front => Some(wgpu::Face::Front),
                },
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: self.depth_format,
                depth_write_enabled: key.zwrite,
                depth_compare: if !key.ztest {
                    wgpu::CompareFunction::Always
                } else if key.decal {
                    wgpu::CompareFunction::LessEqual
                } else {
                    wgpu::CompareFunction::Less
                },
                stencil: Default::default(),
                bias: if key.decal { wgpu::DepthBiasState { constant: -2, slope_scale: -1.0, clamp: 0.0 } } else { Default::default() },
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        self.pipes.insert(key, p);
    }

    /// The pipeline [`Self::prepare`] made for `key`.
    pub fn pipeline(&self, key: PipeKey) -> Option<&wgpu::RenderPipeline> {
        self.pipes.get(&key)
    }
}

/// Upload RGBA8 levels (level 0 first) as one mipmapped texture.
fn upload_levels(device: &wgpu::Device, queue: &wgpu::Queue, levels: &[(u32, u32, Vec<u8>)], label: &str) -> Texture {
    let (w, h) = (levels[0].0, levels[0].1);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Raw values: the N64 has no gamma.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (i, (lw, lh, px)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: i as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(*lh) },
            wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
        );
    }
    Texture { view: tex.create_view(&wgpu::TextureViewDescriptor::default()), w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_combiner_shader_validates() {
        let module = naga::front::wgsl::parse_str(COMBINER_WGSL).unwrap_or_else(|e| panic!("{}", e.emit_to_string(COMBINER_WGSL)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
    }

    #[test]
    fn the_uniforms_match_the_shaders_layout() {
        // 4x4 matrix + 8 vec4s; 11 vec4s.
        assert_eq!(std::mem::size_of::<FrameUniform>(), 64 + 8 * 16);
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 11 * 16);
        assert_eq!(std::mem::size_of::<Vertex>(), 44);
    }
}
