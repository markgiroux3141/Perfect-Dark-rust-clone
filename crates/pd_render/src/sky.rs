//! The sky (`sky_render`, `sky.c:206`): a cloud plane `clouds_scale` cm up,
//! textured with `g_TexSkyWaterConfigs[clouds_type]` (every arena's is
//! `TEX_ENV_00`, `TEXTURE_0013`), drifting with the wind
//! (`World::sky_cloud_offset`), fading into the sky colour at the horizon.
//!
//! PD shoots the four screen corners' rays at the plane (and, where the
//! horizon crosses a screen edge, the point on it), keeps the corners that
//! see sky (`CORNERSTATE_*`), and draws that polygon's triangles itself
//! (`sky_render_tri`, `sky_render_full`: RDP triangles with perspective-correct
//! `s, t` and screen-linear shade). This builds the same vertices
//! (`sky_convert_vertex`, their clamps to the screen) and lets the GPU draw
//! the triangles with the RDP's interpolation: the shade and `s/w, t/w, 1/w`
//! linear on screen. The combiner is `(SHADE - ENV) × TEXEL0 + ENV`, ENV the
//! sky colour. Below the horizon the sky colour shows (the clear colour: no
//! arena has water).
//!
//! Not ported: water (no arena's is enabled), suns and lens flares (no
//! arena has suns), lightning.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use pd_core::assets::AssetDir;
use pd_sim::player::camera::Camera;
use wgpu::util::DeviceExt;

/// `TEXTURE_0013` (`g_TcSkyWaterConfigs[TEX_ENV_00]`): 64 × 64 IA8, wrapped.
pub const TEX_CLOUDS: u16 = 0x0013;

/// `struct nofogenvironment`'s sky fields (`bg.json` `env`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SkyEnv {
    pub sky: [u8; 3],
    pub clouds_enabled: bool,
    pub clouds: [u8; 3],
    pub clouds_scale: f32,
    pub clouds_height: f32,
}

/// A sky vertex on screen: NDC, the RDP's `s/w, t/w, 1/w` (texels), and the
/// shade (0..1).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SkyVert {
    pub ndc: [f32; 2],
    pub stw: [f32; 3],
    pub col: [f32; 4],
}

/// `struct skyvtx2d` as far as drawing needs: screen position (pixels, PD's
/// screen space), `s, t`, clip `w`, colour.
#[derive(Clone, Copy, Debug)]
struct Vtx2d {
    x: f32,
    y: f32,
    s: f32,
    t: f32,
    w: f32,
    col: [f32; 4],
}

fn sky_clamp(v: f32, min: f32, max: f32) -> f32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

/// A point the sky polygon uses: where a ray meets the plane, and its fade.
#[derive(Clone, Copy, Debug, Default)]
struct SkyPoint {
    pos: Vec3,
    frac: f32,
}

/// `sky_is_screen_corner_in_sky` (`sky.c:64`): where the ray `dir` from the
/// camera meets the cloud plane (at most 3 km out), and `1 − 2·dir.y/|dir.xz|`
/// (clamped), the fade towards the horizon. False: it points down.
fn sky_is_screen_corner_in_sky(env: &SkyEnv, campos: Vec3, dir: Vec3) -> (bool, SkyPoint) {
    let mut f12 = 2.0 * dir.y / (dir.x * dir.x + dir.z * dir.z + 0.0001).sqrt();
    if f12 > 1.0 {
        f12 = 1.0;
    }
    let frac = 1.0 - f12;
    let sp24 = if dir.y == 0.0 { 0.01 } else { dir.y };
    if sp24 > 0.0 {
        let mut sp2c = (env.clouds_scale - campos.y) / sp24;
        let f12_2 = (dir.x * dir.x + dir.z * dir.z).sqrt() * sp2c;
        if f12_2 > 300000.0 {
            sp2c *= 300000.0 / f12_2;
        }
        let pos = Vec3::new(campos.x + sp2c * dir.x, campos.y + sp2c * sp24, campos.z + sp2c * dir.z);
        return (true, SkyPoint { pos, frac });
    }
    (false, SkyPoint { pos: Vec3::ZERO, frac })
}

/// `sky_calculate_edge_vertex` (`sky.c:146`): the direction between `base`
/// and `refd` where y is 0.
fn sky_calculate_edge_vertex(base: Vec3, refd: Vec3) -> Vec3 {
    let mult = base.y / (base.y - refd.y);
    Vec3::new((refd.x - base.x) * mult + base.x, 0.0, (refd.z - base.z) * mult + base.z)
}

/// `sky_choose_cloud_vtx_colour` (`sky.c:173`).
fn sky_choose_cloud_vtx_colour(env: &SkyEnv, frac: f32) -> [f32; 4] {
    let scale = 1.0 - frac;
    let c = |sky: u8, cloud: u8| {
        let r = sky as f32;
        // Stored in a u8 field (`skyvtx3d.r`).
        ((r + cloud as f32 * (1.0 - r * (1.0 / 255.0)) * scale) as i32).clamp(0, 255) as f32 / 255.0
    };
    [c(env.sky[0], env.clouds[0]), c(env.sky[1], env.clouds[1]), c(env.sky[2], env.clouds[2]), 1.0]
}

/// `sky_convert_vertex` (`sky.c:1303`) with `clip = world → clip`: the point
/// on screen (clamped as PD clamps, then to the screen), its `s, t` and `w`.
fn sky_convert_vertex(cam: &Camera, env: &SkyEnv, clipmtx: &Mat4, p: SkyPoint, cloudoffset: f32) -> Vtx2d {
    let mult = 130.0 / 65536.0;
    let c = *clipmtx * p.pos.extend(1.0);
    let f22 = if c.w == 0.0 { 32767.0 } else { 1.0 / (c.w * mult) };
    let f0 = if f22 < 0.0 { 32767.0 } else { f22 };
    let (nx, ny) = (c.x * f0 * mult, c.y * f0 * mult);
    let (w, h) = (cam.c_screenwidth, cam.c_screenheight);
    // In quarter pixels, as PD.
    let x4 = sky_clamp(nx * (w + w) + (w + w + cam.c_screenleft * 4.0), -4090.0, 4090.0);
    let y4 = sky_clamp(-ny * (h + h) + (h + h + cam.c_screentop * 4.0), -4090.0, 4090.0) - env.clouds_height * 4.0;
    let x4 = sky_clamp(x4, cam.c_screenleft * 4.0, (cam.c_screenleft + w) * 4.0 - 1.0);
    let y4 = sky_clamp(y4, cam.c_screentop * 4.0, (cam.c_screentop + h) * 4.0 - 1.0);
    let s = p.pos.x * 0.1 * (65535.0 / 65536.0);
    let t = (p.pos.z * 0.1 + cloudoffset) * (65535.0 / 65536.0);
    Vtx2d { x: x4, y: y4, s, t, w: c.w, col: sky_choose_cloud_vtx_colour(env, p.frac) }
}

/// `sky_vertices_are_same` (`sky.c:1381`): within a quarter pixel.
fn same(a: &Vtx2d, b: &Vtx2d) -> bool {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() < 1.0
}

/// The sky's triangles for a view: none when the clouds are off.
/// `clipmtx` is world → clip (the world pass's projection).
pub fn sky_geometry(env: &SkyEnv, cam: &Camera, campos: Vec3, clipmtx: &Mat4, cloudoffset: f32) -> Vec<SkyVert> {
    if !env.clouds_enabled {
        return Vec::new();
    }
    let (left, top, width, height) = (cam.c_screenleft, cam.c_screentop, cam.c_screenwidth, cam.c_screenheight);
    // sky_get_world_pos_from_screen_pos (`sky.c:52`).
    let ray = |l: f32, t: f32| cam.projection.transform_vector3(cam.cam0f0b4c3c([l + left, t + top + env.clouds_height], 100.0));
    let tl3 = ray(0.0, 0.0);
    let tr3 = ray(width - 0.1, 0.0);
    let bl3 = ray(0.0, height - 0.1);
    let br3 = ray(width - 0.1, height - 0.1);
    let (tlsky, tl) = sky_is_screen_corner_in_sky(env, campos, tl3);
    let (trsky, tr) = sky_is_screen_corner_in_sky(env, campos, tr3);
    let (blsky, bl) = sky_is_screen_corner_in_sky(env, campos, bl3);
    let (brsky, br) = sky_is_screen_corner_in_sky(env, campos, br3);
    // Where the horizon crosses each edge that has sky at one end only.
    let edge = |a: Vec3, b: Vec3| sky_is_screen_corner_in_sky(env, campos, sky_calculate_edge_vertex(a, b)).1;
    let (mut lh, mut rh) = (0.0, 0.0);
    let (mut le, mut re, mut te, mut be) = (SkyPoint::default(), SkyPoint::default(), SkyPoint::default(), SkyPoint::default());
    if tlsky != blsky {
        lh = top + height * (tl3.y / (tl3.y - bl3.y));
        le = edge(tl3, bl3);
    }
    if trsky != brsky {
        rh = top + height * (tr3.y / (tr3.y - br3.y));
        re = edge(tr3, br3);
    }
    if tlsky != trsky {
        te = edge(tl3, tr3);
    }
    if blsky != brsky {
        be = edge(bl3, br3);
    }
    let state = (tlsky as u8) << 3 | (trsky as u8) << 2 | (blsky as u8) << 1 | brsky as u8;
    let pts: Vec<SkyPoint> = match state {
        0xf => vec![tl, tr, bl, br],
        0xc => vec![tl, tr, le, re],
        0x3 => vec![br, bl, re, le],
        0x5 => vec![tr, br, te, be],
        0xa => vec![bl, tl, be, te],
        0x1 => vec![br, be, re],
        0x2 => vec![bl, le, be],
        0x4 => vec![tr, re, te],
        0x8 => vec![tl, te, le],
        0xe => vec![bl, tl, tr, re, be],
        0xd => vec![tl, tr, br, be, le],
        0xb => vec![br, bl, tl, te, re],
        0x7 => vec![tr, br, bl, le, te],
        // CORNERSTATE_NONE and the diagonals draw no sky.
        _ => return Vec::new(),
    };
    let mut v: Vec<Vtx2d> = pts.iter().map(|&p| sky_convert_vertex(cam, env, clipmtx, p, cloudoffset)).collect();
    let (sl, st, sr) = (left * 4.0, top * 4.0, (left + width) * 4.0 - 1.0);
    let mut tris: Vec<[Vtx2d; 3]> = Vec::new();
    let mut tri = |a: Vtx2d, b: Vtx2d, c: Vtx2d| {
        if !(same(&a, &b) || same(&b, &c) || same(&c, &a)) {
            tris.push([a, b, c]);
        }
    };
    match v.len() {
        4 if state == 0xc => {
            // CORNERSTATE_TOP: the horizon crosses both sides; from the top of
            // the screen down to it in one quad when it is low enough.
            let full = |v: &mut Vec<Vtx2d>| {
                v[0].x = sl;
                v[0].y = st;
                v[1].x = sr;
                v[1].y = st;
                v[2].x = sl;
                v[3].x = sr;
            };
            if rh < lh {
                if v[3].y >= v[1].y + 4.0 {
                    full(&mut v);
                    sky_render_full(&mut tris, v[0], v[1], v[2], v[3]);
                } else {
                    tri(v[0], v[1], v[2]);
                }
            } else if v[2].y >= v[0].y + 4.0 {
                full(&mut v);
                sky_render_full(&mut tris, v[1], v[0], v[3], v[2]);
            } else {
                tri(v[1], v[0], v[3]);
            }
        }
        4 => {
            tri(v[0], v[1], v[3]);
            tri(v[3], v[2], v[0]);
        }
        5 => {
            tri(v[0], v[1], v[2]);
            tri(v[0], v[2], v[3]);
            tri(v[0], v[3], v[4]);
        }
        3 => tri(v[0], v[1], v[2]),
        _ => {}
    }
    let ndc = |a: &Vtx2d| {
        let px = a.x * 0.25;
        let py = a.y * 0.25;
        [(px - left) / width * 2.0 - 1.0, 1.0 - (py - top) / height * 2.0]
    };
    let mut out = Vec::with_capacity(tris.len() * 3);
    for t in &tris {
        // The RDP's S, T, W: s·k, t·k, 32767·k with k = min(w) / (2w); only
        // the ratios matter, so s/w, t/w, 1/w.
        for a in t {
            let iw = 1.0 / a.w;
            out.push(SkyVert { ndc: ndc(a), stw: [a.s * iw, a.t * iw, iw], col: a.col });
        }
    }
    out
}

/// `sky_render_full` (`sky.c:1896`): the quad `v0 v1 v3 v2` filled with the
/// attribute planes of triangle `v0 v1 v2` (two GPU triangles, `v3` given the
/// plane's values at its position).
fn sky_render_full(tris: &mut Vec<[Vtx2d; 3]>, v0: Vtx2d, v1: Vtx2d, v2: Vtx2d, v3: Vtx2d) {
    if same(&v0, &v1) || same(&v1, &v2) || same(&v2, &v0) || same(&v3, &v0) || same(&v3, &v1) || same(&v3, &v2) {
        return;
    }
    // Screen-linear attributes: shade, s/w, t/w, 1/w.
    let attrs = |a: &Vtx2d| [a.col[0], a.col[1], a.col[2], a.col[3], a.s / a.w, a.t / a.w, 1.0 / a.w];
    let (a0, a1, a2) = (attrs(&v0), attrs(&v1), attrs(&v2));
    let (dx1, dy1, dx2, dy2) = (v1.x - v0.x, v1.y - v0.y, v2.x - v0.x, v2.y - v0.y);
    let det = dx1 * dy2 - dx2 * dy1;
    if det == 0.0 {
        return;
    }
    let (px, py) = (v3.x - v0.x, v3.y - v0.y);
    // v3 = v0 + a·(v1 − v0) + b·(v2 − v0).
    let a = (px * dy2 - dx2 * py) / det;
    let b = (dx1 * py - px * dy1) / det;
    let mut e = [0.0f32; 7];
    for k in 0..7 {
        e[k] = a0[k] + a * (a1[k] - a0[k]) + b * (a2[k] - a0[k]);
    }
    let iw = e[6];
    let v3p = Vtx2d { x: v3.x, y: v3.y, s: e[4] / iw, t: e[5] / iw, w: 1.0 / iw, col: [e[0], e[1], e[2], e[3]] };
    tris.push([v0, v1, v2]);
    tris.push([v1, v3p, v2]);
}

/// The sky pass: the cloud texture, a pipeline and a vertex buffer rewritten
/// each view.
pub struct SkyRenderer {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    vbuf: Option<(wgpu::Buffer, u32)>,
}

const SKY_WGSL: &str = r#"
struct U { env: vec4<f32>, texsize: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(linear) stw: vec3<f32>,
    @location(1) @interpolate(linear) col: vec4<f32>,
};

@vertex
fn vs_main(@location(0) ndc: vec2<f32>, @location(1) stw: vec3<f32>, @location(2) col: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(ndc, 0.0, 1.0);
    o.stw = stw;
    o.col = col;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    // The RDP's perspective divide; s, t are S10.5 texels.
    let st = i.stw.xy / i.stw.z / 32.0 / u.texsize.xy;
    let texel = textureSample(tex, samp, st);
    // (SHADE - ENV) * TEXEL0 + ENV; alpha SHADE. An IA texel's colour is its intensity.
    let c = (i.col.rgb - u.env.rgb) * texel.rgb + u.env.rgb;
    return vec4<f32>(c, 1.0);
}
"#;

impl SkyRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetDir, color_format: wgpu::TextureFormat, depth_format: wgpu::TextureFormat) -> SkyRenderer {
        let (w, h, rgba) = assets.read_png(&assets.path(&format!("textures/{TEX_CLOUDS:04x}.png"))).unwrap_or((1, 1, vec![255; 4]));
        let view = crate::fx::upload(device, queue, w as u32, h as u32, &rgba);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pdsky"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("pdsky-u"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pdsky"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pdsky"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pdsky"), source: wgpu::ShaderSource::Wgsl(SKY_WGSL.into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pdsky"), bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pdsky"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<SkyVert>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: depth_format, depth_write_enabled: false, depth_compare: wgpu::CompareFunction::Always, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let size = [w as f32, h as f32];
        queue.write_buffer(&uniform, 16, bytemuck::cast_slice(&[size[0], size[1], 0.0, 0.0]));
        SkyRenderer { pipeline, bind, uniform, vbuf: None }
    }

    /// Upload this view's triangles and the sky colour (ENV).
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, env: &SkyEnv, verts: &[SkyVert]) {
        let e = [env.sky[0] as f32 / 255.0, env.sky[1] as f32 / 255.0, env.sky[2] as f32 / 255.0, 1.0];
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&e));
        self.vbuf = (!verts.is_empty()).then(|| (device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("pdsky-vb"), contents: bytemuck::cast_slice(verts), usage: wgpu::BufferUsages::VERTEX }), verts.len() as u32));
    }

    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if let Some((vb, n)) = &self.vbuf {
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.bind, &[]);
            rp.set_vertex_buffer(0, vb.slice(..));
            rp.draw(0..*n, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> SkyEnv {
        SkyEnv { sky: [0, 0, 8], clouds_enabled: true, clouds: [70, 199, 186], clouds_scale: 4500.0, clouds_height: 0.0 }
    }

    fn camera(look: Vec3) -> Camera {
        let mut cam = Camera::default();
        let pos = Vec3::new(0.0, 100.0, 0.0);
        cam.projection = pd_core::math::look_basis(pos, look, Vec3::Y);
        cam.world_to_screen = cam.projection.inverse();
        cam
    }

    fn clip(cam: &Camera) -> Mat4 {
        Mat4::perspective_rh(60f32.to_radians(), cam.c_perspaspect, 10.0, 10000.0) * cam.world_to_screen
    }

    /// Looking straight up, the whole screen is sky: two triangles over the
    /// full view, bright clouds (the fade is 0 for a steep ray).
    #[test]
    fn looking_up_is_all_sky() {
        let cam = camera(Vec3::new(0.0, 1.0, -0.001).normalize());
        let v = sky_geometry(&env(), &cam, Vec3::new(0.0, 100.0, 0.0), &clip(&cam), 0.0);
        assert_eq!(v.len(), 6);
        for p in &v {
            assert!(p.ndc[0].abs() <= 1.0 && p.ndc[1].abs() <= 1.0);
            assert!((p.col[1] - 199.0 / 255.0).abs() < 0.01, "{:?}", p.col);
        }
    }

    /// Looking at the horizon, the sky is the top half: from the screen's top
    /// corners down to the horizon line, fading to the sky colour there.
    #[test]
    fn at_the_horizon_the_sky_is_the_top_half() {
        let cam = camera(Vec3::new(0.0, 0.0, -1.0));
        let v = sky_geometry(&env(), &cam, Vec3::new(0.0, 100.0, 0.0), &clip(&cam), 0.0);
        assert!(!v.is_empty());
        let lowest = v.iter().map(|p| p.ndc[1]).fold(f32::MAX, f32::min);
        assert!(lowest.abs() < 0.05, "the sky reaches {lowest}");
        let at_horizon = v.iter().filter(|p| p.ndc[1].abs() < 0.05).map(|p| p.col[1]).fold(0.0f32, f32::max);
        assert!(at_horizon < 0.05, "the horizon is {at_horizon}, not the sky colour");
        // Looking down there is none.
        let cam = camera(Vec3::new(0.0, -1.0, -0.001).normalize());
        assert!(sky_geometry(&env(), &cam, Vec3::new(0.0, 100.0, 0.0), &clip(&cam), 0.0).is_empty());
    }
}
