//! (feature `gpu`) N64 video + CRT: a frame at PD's own resolution (320×220
//! NTSC) through the RDP's 16-bit store, the VI's filters and an NTSC tube.
//! See `video.wgsl` for what each pass models and where each number comes
//! from:
//!
//! - the RDP store: RGBA5551 with the Bayer colour dither, the HUD composited
//!   in display space, and a coverage estimate from depth (`fs_rdp`);
//! - the VI: the dither ("restore") filter on covered pixels and the
//!   anti-alias filter on edges (`fs_vi`), then the divot filter (`fs_divot`);
//! - the analogue signal per scanline: RGB, S-Video, or composite (YIQ on the
//!   3.58 MHz subcarrier, decoded as a basic TV does) (`fs_signal`);
//! - the tube: BT.1886 gamma, a gaussian beam per line that blooms with
//!   brightness, the phosphor mask, halation, curvature and overscan
//!   (`fs_glow`, `fs_tube`), or the bare 240-line raster (`fs_flat`);
//! - optionally a photo of a TV set around it, its green screen keyed out
//!   (`fs_frame`).
//!
//! The renderer draws the world into a scene target of
//! [`Resolution::size`], the guns and PD's framebuffer effects draw into it as
//! usual, and [`N64Video::run`] takes it from there to the present target.
//! [`N64Video::run_still`] runs only the CRT half on an image, e.g. an
//! emulator capture, so the two halves can be judged separately.
//!
//! The TV-set photos (`crt_screen*.png`) are compiled in and decoded with
//! `image` when a set is first chosen.
//!
//! Departures from the spike: the scene and present targets may each be sRGB
//! or raw (`Frame::color_srgb`, the present format; the spike assumed both
//! sRGB); the HUD arrives as premultiplied RGBA8 instead of the gun spike's
//! `Canvas`; the depth ranges come with the frame instead of being constants;
//! with no gun depth the coverage estimate is off (every pixel covered).
//!
//! Source: the old repo's `pd_guns/n64video.rs` and `n64video.wgsl`.

use bytemuck::{Pod, Zeroable};

/// The chain's WGSL (validated by the tests).
pub const VIDEO_WGSL: &str = include_str!("video.wgsl");

/// PD's NTSC low-res framebuffer (`FBALLOC_WIDTH_LO` × `FBALLOC_HEIGHT_LO`,
/// `constants.h:3655`).
pub const N64_W: u32 = 320;
pub const N64_H: u32 = 220;
/// Visible raster lines (240p); PD's 220 sit centred in them.
pub const LINES: u32 = 240;

/// The framebuffer the world renders into. The first two are PD's own modes
/// (`constants.h:3655`: hi-res doubles the width only); the third is sharper
/// than any N64 and goes out on a 480-line raster.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolution {
    Lo,
    Hi,
    Double,
}

impl Resolution {
    pub const ALL: [Resolution; 3] = [Resolution::Lo, Resolution::Hi, Resolution::Double];
    pub fn size(self) -> (u32, u32) {
        match self {
            Resolution::Lo => (N64_W, N64_H),
            Resolution::Hi => (N64_W * 2, N64_H),
            Resolution::Double => (N64_W * 2, N64_H * 2),
        }
    }
    /// Raster lines on the tube.
    pub fn lines(self) -> u32 {
        match self {
            Resolution::Double => LINES * 2,
            _ => LINES,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Resolution::Lo => "320×220 (PD)",
            Resolution::Hi => "640×220 (PD hi-res)",
            Resolution::Double => "640×440 (beyond N64)",
        }
    }
}

/// CRT settings in one go (the tube's knobs only, not the N64 stages).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    Clean,
    SVideo,
    Composite,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Clean, Preset::SVideo, Preset::Composite];
    pub fn label(self) -> &'static str {
        match self {
            Preset::Clean => "clean RGB",
            Preset::SVideo => "S-Video TV",
            Preset::Composite => "composite TV",
        }
    }
    pub fn apply(self, v: &mut VideoSettings) {
        let (signal, scan, mask, hal, curve, sharp) = match self {
            Preset::Clean => (Signal::Rgb, 0.45, 0.15, 0.03, 0.03, 1.4),
            Preset::SVideo => (Signal::SVideo, 0.9, 0.25, 0.05, 0.05, 1.0),
            Preset::Composite => (Signal::Composite, 0.75, 0.35, 0.06, 0.06, 1.0),
        };
        v.signal = signal;
        v.scanlines = scan;
        v.mask = Mask::ApertureGrille;
        v.mask_strength = mask;
        v.halation = hal;
        v.curvature = curve;
        v.sharpness = sharp;
    }
}

/// Signal samples per line (4 per framebuffer pixel).
const SIG_W: u32 = 1280;
const GLOW_W: u32 = 160;
const GLOW_H: u32 = 120;

/// Photos of a TV set with a chroma-green screen, shown around the tube with
/// the green keyed out. Each photo is 4:3 and is drawn over the whole 4:3
/// tube rect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TvSet {
    Off,
    /// `crt_screen.png`: the set alone on grey.
    Plain,
    /// `crt_screen_with_background.png`: the set in a 90s bedroom.
    Bedroom,
}

impl TvSet {
    pub const ALL: [TvSet; 3] = [TvSet::Plain, TvSet::Bedroom, TvSet::Off];
    pub fn label(self) -> &'static str {
        match self {
            TvSet::Off => "off",
            TvSet::Plain => "TV set",
            TvSet::Bedroom => "TV set in a bedroom",
        }
    }
    /// The photo and its screen box in photo pixels `[x0, y0, x1, y1]`. The
    /// box is the green screen's extent (its largest connected green region:
    /// the bedroom has other green things, e.g. the soda can) plus 3 px, so
    /// the raster sits under the key's soft edge. Only pixels inside the box
    /// are keyed.
    fn photo(self) -> Option<(&'static [u8], [f32; 4])> {
        match self {
            TvSet::Off => None,
            // Green spans x 234..=1215, y 104..=809.
            TvSet::Plain => Some((include_bytes!("crt_screen.png"), [231.0, 101.0, 1219.0, 813.0])),
            // Green spans x 381..=1079, y 138..=615.
            TvSet::Bedroom => Some((include_bytes!("crt_screen_with_background.png"), [378.0, 135.0, 1083.0, 619.0])),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signal {
    Rgb,
    SVideo,
    Composite,
}

impl Signal {
    pub const ALL: [Signal; 3] = [Signal::Composite, Signal::SVideo, Signal::Rgb];
    pub fn label(self) -> &'static str {
        match self {
            Signal::Rgb => "RGB (modded)",
            Signal::SVideo => "S-Video",
            Signal::Composite => "composite",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mask {
    None,
    ApertureGrille,
    Slot,
    Shadow,
}

impl Mask {
    pub const ALL: [Mask; 4] = [Mask::ApertureGrille, Mask::Slot, Mask::Shadow, Mask::None];
    pub fn label(self) -> &'static str {
        match self {
            Mask::None => "none",
            Mask::ApertureGrille => "aperture grille",
            Mask::Slot => "slot mask",
            Mask::Shadow => "shadow mask",
        }
    }
}

/// Every stage can be switched off on its own, to see what it contributes.
///
/// The default is off (`n64: false`, so [`Self::active`] is false and the
/// caller presents its frame as usual). The stages below it default to the
/// spike's choices (every N64 stage on, the S-Video preset, the plain TV
/// set), so `VideoSettings { n64: true, ..Default::default() }` is the full
/// chain.
#[derive(Clone, Copy, Debug)]
pub struct VideoSettings {
    /// Master switch: render at `resolution` and run the passes below.
    pub n64: bool,
    pub resolution: Resolution,
    /// The RDP's 3-point texture filter (world and guns). Read by the
    /// renderers, not by this chain.
    pub three_point: bool,
    /// RGBA5551 framebuffer with the RDP's Bayer dither.
    pub fb16: bool,
    /// The VI's dither ("restore") filter.
    pub dither_filter: bool,
    /// The VI's edge anti-aliasing (coverage estimated from depth).
    pub aa: bool,
    /// The VI's divot filter.
    pub divot: bool,
    /// The tube; off = the raw 240-line raster, nearest, at 4:3.
    pub crt: bool,
    pub signal: Signal,
    /// 0 = lines merge, 1 = strong gaps between them.
    pub scanlines: f32,
    pub mask: Mask,
    pub mask_strength: f32,
    /// Show the tube inside a photo of a TV set, its green screen replaced by
    /// the picture.
    pub tv: TvSet,
    pub curvature: f32,
    pub halation: f32,
    pub overscan: f32,
    /// Analogue bandwidth scale (1 = the standard's): above 1 the signal is
    /// sharper than a real set, below 1 softer.
    pub sharpness: f32,
}

impl VideoSettings {
    /// Whether the chain runs at all (the master switch).
    pub fn active(&self) -> bool {
        self.n64
    }
}

impl Default for VideoSettings {
    fn default() -> Self {
        let mut v = VideoSettings {
            n64: false,
            resolution: Resolution::Hi,
            three_point: true,
            fb16: true,
            dither_filter: true,
            aa: true,
            divot: true,
            crt: true,
            signal: Signal::Composite,
            scanlines: 0.75,
            mask: Mask::ApertureGrille,
            mask_strength: 0.35,
            tv: TvSet::Plain,
            curvature: 0.06,
            halation: 0.06,
            overscan: 0.08,
            sharpness: 1.0,
        };
        Preset::SVideo.apply(&mut v);
        v
    }
}

/// One frame for [`N64Video::run`]: the scene target after the guns and PD's
/// post effects, and what the coverage estimate needs.
pub struct Frame<'a> {
    /// The scene target, `size` pixels ([`Resolution::size`]).
    pub color: &'a wgpu::TextureView,
    /// `color`'s format is sRGB (a load gives linear light). The repo's own
    /// targets are raw `Rgba8Unorm` (display values).
    pub color_srgb: bool,
    pub size: (u32, u32),
    /// The scene's depth as the gun pass left it (the gun pass clears z, so
    /// anything < 1 is gun). `None` turns the coverage estimate off: every
    /// pixel is fully covered, and the VI's AA filter has nothing to do.
    pub gun_depth: Option<&'a wgpu::TextureView>,
    /// Near and far of the world's projection and of the gun's, in any one
    /// unit per pair, for the edge finder's 1/z (the test is relative, so
    /// only their ratio matters). The world's depth before the gun pass must
    /// have been copied into [`N64Video::world_depth`].
    pub world_near_far: (f32, f32),
    pub gun_near_far: (f32, f32),
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VideoU {
    size: [f32; 4],
    flags: [u32; 4],
    depth: [f32; 4],
    crt: [f32; 4],
    crt2: [f32; 4],
    tube: [f32; 4],
    raster: [f32; 4],
    keybox: [f32; 4],
    encode: [f32; 4],
    ntsc: [[f32; 4]; 65],
}

/// Gaussian low-pass σ (µs) for a −3 dB bandwidth in MHz (as in the shader).
fn sigma_us(mhz: f32) -> f32 {
    0.1325 / mhz
}

/// The composite decoder's taps (see `fs_signal`) and the luma filter's gain
/// at the subcarrier. Taps sit 1/9 of a subcarrier period apart. Luma is a
/// gaussian (~3 MHz); I and Q are a 9-tap box (one period, so its response is
/// exactly zero at 3.58 and 7.16 MHz) convolved with a gaussian, ~1.3 and
/// ~0.6 MHz overall.
/// `sharpness` scales every bandwidth except the box, which must stay one
/// period long to null the subcarrier.
fn ntsc_taps(sharpness: f32) -> ([[f32; 4]; 65], f32) {
    const FSC: f32 = 3.579545;
    let dt = 1.0 / (FSC * 9.0);
    let g = |t: f32, s: f32| (-0.5 * (t / s) * (t / s)).exp();
    let k = sharpness.max(0.1);
    let (sy, si, sq) = (sigma_us(3.0 * k), sigma_us(2.5 * k), sigma_us(0.8 * k));
    let boxed = |k: i32, s: f32| (-4..=4).map(|j| g((k - j) as f32 * dt, s)).sum::<f32>();
    let mut taps = [[0.0f32; 4]; 65];
    for (i, t) in taps.iter_mut().enumerate() {
        let k = i as i32 - 32;
        *t = [g(k as f32 * dt, sy), boxed(k, si), boxed(k, sq), 0.0];
    }
    for c in 0..3 {
        let sum: f32 = taps.iter().map(|t| t[c]).sum();
        for t in taps.iter_mut() {
            t[c] /= sum;
        }
    }
    let omega = std::f32::consts::TAU * FSC * dt;
    let gain = taps.iter().enumerate().map(|(i, t)| t[0] * ((i as i32 - 32) as f32 * omega).cos()).sum();
    (taps, gain)
}

/// The largest 4:3 rect that fits the present area right of `left` (pixels),
/// centred: `[x0, y0, w, h]`.
pub fn tube_rect(present_w: u32, present_h: u32, left: f32) -> [f32; 4] {
    let left = left.clamp(0.0, present_w as f32 * 0.5);
    let aw = present_w as f32 - left;
    let ah = present_h as f32;
    let (w, h) = if aw / ah > 4.0 / 3.0 { (ah * 4.0 / 3.0, ah) } else { (aw, aw * 3.0 / 4.0) };
    [(left + (aw - w) * 0.5).floor(), ((ah - h) * 0.5).floor(), w.floor(), h.floor()]
}

struct Tex {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

fn tex(device: &wgpu::Device, label: &str, w: u32, h: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> Tex {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    Tex { tex, view }
}

const FB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A TV set's photo decoded to RGBA8, its size, and its screen box as a
/// fraction of the photo.
fn decode_photo(set: TvSet) -> Option<(Vec<u8>, u32, u32, [f32; 4])> {
    let (bytes, bx) = set.photo()?;
    let img = image::load_from_memory(bytes).expect("TV set photo").to_rgba8();
    let (w, h) = img.dimensions();
    let (fw, fh) = (w as f32, h as f32);
    Some((img.into_raw(), w, h, [bx[0] / fw, bx[1] / fh, bx[2] / fw, bx[3] / fh]))
}

/// A TV set's photo on the GPU (raw bytes: the key works on the photo's own
/// values) and its screen box as a fraction of the photo.
struct TvPhoto {
    set: TvSet,
    tex: Tex,
    keybox: [f32; 4],
}

fn tv_photo(device: &wgpu::Device, queue: &wgpu::Queue, set: TvSet) -> Option<TvPhoto> {
    let (rgba, w, h, keybox) = decode_photo(set)?;
    let t = tex(device, "n64-tv-photo", w, h, FB, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    queue.write_texture(
        t.tex.as_image_copy(),
        &rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    Some(TvPhoto { set, tex: t, keybox })
}

/// The tube inside a TV set drawn over `rect`: the 4:3 rect that covers the
/// photo's screen box, centred on it. The screen's glass is rarely exactly
/// 4:3, so the picture is cropped by the bezel (as a real tube's overscan is)
/// instead of stretched.
fn tv_screen_rect(rect: [f32; 4], keybox: [f32; 4]) -> [f32; 4] {
    let x0 = rect[0] + keybox[0] * rect[2];
    let y0 = rect[1] + keybox[1] * rect[3];
    let w = (keybox[2] - keybox[0]) * rect[2];
    let h = (keybox[3] - keybox[1]) * rect[3];
    let (cw, ch) = if w / h > 4.0 / 3.0 { (w, w * 3.0 / 4.0) } else { (h * 4.0 / 3.0, h) };
    let (cx, cy) = (x0 + w * 0.5, y0 + h * 0.5);
    [(cx - cw * 0.5).floor(), (cy - ch * 0.5).floor(), cw.ceil(), ch.ceil()]
}

const SIG: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The per-scene-size textures.
struct Targets {
    w: u32,
    h: u32,
    fb: Tex,
    vi: Tex,
    vid: Tex,
    world_depth: Tex,
}

/// The chain's pipelines and intermediate targets. One per present format.
pub struct N64Video {
    bgl: wgpu::BindGroupLayout,
    buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    rdp: wgpu::RenderPipeline,
    vi: wgpu::RenderPipeline,
    divot: wgpu::RenderPipeline,
    flat: wgpu::RenderPipeline,
    signal: wgpu::RenderPipeline,
    glow: wgpu::RenderPipeline,
    tube: wgpu::RenderPipeline,
    frame_pipe: wgpu::RenderPipeline,
    /// The present target is sRGB (it encodes the tube's linear light itself).
    present_srgb: bool,
    /// The selected TV set's photo, loaded when first chosen.
    tv_photo: Option<TvPhoto>,
    sig: Tex,
    glow_tex: Tex,
    targets: Option<Targets>,
    hud: Option<(Tex, u32, u32)>,
    /// 1×1 stand-ins for unbound slots.
    clear: Tex,
    depth1: Tex,
    still: Option<(Tex, u32, u32)>,
    frame: u32,
    ntsc_taps: [[f32; 4]; 65],
    ntsc_gain: f32,
    /// The sharpness the taps were built for.
    ntsc_sharpness: f32,
    /// Raster lines the signal texture is sized for.
    sig_lines: u32,
}

impl N64Video {
    /// The chain drawing into targets of `present_format` (sRGB or not).
    pub fn new(device: &wgpu::Device, _queue: &wgpu::Queue, present_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("n64-video"), source: wgpu::ShaderSource::Wgsl(VIDEO_WGSL.into()) });
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let depth_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("n64-video"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex_entry(1),
                tex_entry(2),
                depth_entry(3),
                depth_entry(4),
                wgpu::BindGroupLayoutEntry { binding: 5, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("n64-video"), bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let pipe_blend = |entry: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipe = |entry: &str, format: wgpu::TextureFormat| pipe_blend(entry, format, None);
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("n64-video-u"),
            size: std::mem::size_of::<VideoU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("n64-video"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let sampled = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let upload = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let (ntsc_taps, ntsc_gain) = ntsc_taps(1.0);
        N64Video {
            rdp: pipe("fs_rdp", FB),
            vi: pipe("fs_vi", FB),
            divot: pipe("fs_divot", FB),
            flat: pipe("fs_flat", present_format),
            signal: pipe("fs_signal", SIG),
            glow: pipe("fs_glow", SIG),
            tube: pipe("fs_tube", present_format),
            frame_pipe: pipe_blend("fs_frame", present_format, Some(wgpu::BlendState::ALPHA_BLENDING)),
            present_srgb: present_format.is_srgb(),
            tv_photo: None,
            sig: tex(device, "n64-signal", SIG_W, LINES, SIG, sampled),
            glow_tex: tex(device, "n64-glow", GLOW_W, GLOW_H, SIG, sampled),
            clear: tex(device, "n64-clear", 1, 1, FB, upload),
            depth1: tex(device, "n64-depth1", 1, 1, wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::TEXTURE_BINDING),
            bgl,
            buf,
            sampler,
            targets: None,
            hud: None,
            still: None,
            frame: 0,
            ntsc_taps,
            ntsc_gain,
            ntsc_sharpness: 1.0,
            sig_lines: LINES,
        }
    }

    /// The signal texture for `lines` raster lines, and the composite taps for
    /// `sharpness`.
    fn ensure_raster(&mut self, device: &wgpu::Device, lines: u32, sharpness: f32) {
        if self.sig_lines != lines {
            let sampled = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            self.sig = tex(device, "n64-signal", SIG_W, lines, SIG, sampled);
            self.sig_lines = lines;
        }
        if self.ntsc_sharpness != sharpness {
            (self.ntsc_taps, self.ntsc_gain) = ntsc_taps(sharpness);
            self.ntsc_sharpness = sharpness;
        }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| t.w == w && t.h == h) {
            return;
        }
        let sampled = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        self.targets = Some(Targets {
            w,
            h,
            fb: tex(device, "n64-fb", w, h, FB, sampled),
            vi: tex(device, "n64-vi", w, h, FB, sampled),
            vid: tex(device, "n64-vid", w, h, FB, sampled),
            world_depth: tex(device, "n64-world-depth", w, h, wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST),
        });
    }

    /// The texture the world's depth (Depth32Float, before the gun pass clears
    /// it) should be copied into this frame, for the coverage estimate.
    pub fn world_depth(&mut self, device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
        self.ensure_targets(device, w, h);
        self.targets.as_ref().unwrap().world_depth.tex.clone()
    }

    /// This frame's HUD: premultiplied RGBA8, `(pixels, w, h)` in PD pixels,
    /// composited in the RDP pass (scaled up to hi-res modes). `None` = no HUD.
    pub fn upload_hud(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hud: Option<(&[u8], u32, u32)>) {
        let Some((px, w, h)) = hud else {
            self.hud = None;
            return;
        };
        if !self.hud.as_ref().is_some_and(|(_, hw, hh)| *hw == w && *hh == h) {
            let t = tex(device, "n64-hud", w, h, FB, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            self.hud = Some((t, w, h));
        }
        let (t, _, _) = self.hud.as_ref().unwrap();
        queue.write_texture(
            t.tex.as_image_copy(),
            px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// The uniform for a frame of `src` pixels shown on `lines` raster lines.
    /// `depth` is (world near, far, gun near, gun far), or `None` for no
    /// coverage estimate; `src_srgb` says the source loads as linear light.
    fn uniform(&self, s: &VideoSettings, src: (u32, u32), lines: u32, rect: [f32; 4], depth: Option<[f32; 4]>, src_srgb: bool) -> VideoU {
        let signal = match s.signal {
            Signal::Rgb => 0.0,
            Signal::SVideo => 1.0,
            Signal::Composite => 2.0,
        };
        let mask = match s.mask {
            Mask::None => 0.0,
            Mask::ApertureGrille => 1.0,
            Mask::Slot => 2.0,
            Mask::Shadow => 3.0,
        };
        VideoU {
            size: [src.0 as f32, src.1 as f32, lines as f32, self.frame as f32],
            flags: [s.fb16 as u32, s.dither_filter as u32, (s.aa && depth.is_some()) as u32, s.divot as u32],
            depth: depth.unwrap_or([1.0, 2.0, 1.0, 2.0]),
            crt: [signal, s.scanlines, mask, s.mask_strength],
            crt2: [s.curvature, s.halation, self.overscan(s), self.ntsc_gain],
            tube: rect,
            raster: [self.sig_lines as f32, s.sharpness, self.framed(s) as u32 as f32, 0.0],
            keybox: self.tv_photo.as_ref().filter(|_| self.framed(s)).map_or([0.0; 4], |p| p.keybox),
            encode: [src_srgb as u32 as f32, self.present_srgb as u32 as f32, 0.0, 0.0],
            ntsc: self.ntsc_taps,
        }
    }

    fn bind(&self, device: &wgpu::Device, t0: &wgpu::TextureView, t1: &wgpu::TextureView, d0: &wgpu::TextureView, d1: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("n64-video"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(t0) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(t1) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(d0) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(d1) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    fn pass(encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, pipe: &wgpu::RenderPipeline, bind: &wgpu::BindGroup, viewport: Option<[f32; 4]>) {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("n64-video"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if let Some([x, y, w, h]) = viewport {
            rp.set_viewport(x, y, w.max(1.0), h.max(1.0), 0.0, 1.0);
        }
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Whether this frame draws a TV set around the tube.
    fn framed(&self, s: &VideoSettings) -> bool {
        s.crt && s.tv != TvSet::Off && self.tv_photo.as_ref().is_some_and(|p| p.set == s.tv)
    }

    /// Load the selected TV set's photo if it isn't the one on the GPU.
    fn ensure_tv(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, set: TvSet) {
        if set != TvSet::Off && !self.tv_photo.as_ref().is_some_and(|p| p.set == set) {
            self.tv_photo = tv_photo(device, queue, set);
        }
    }

    /// The overscan to apply. Inside a TV set the bezel already crops what the
    /// 4:3 raster has beyond the screen box (`tv_screen_rect`), and that crop
    /// counts towards the overscan instead of adding to it.
    fn overscan(&self, s: &VideoSettings) -> f32 {
        match self.tv_photo.as_ref().filter(|_| self.framed(s)) {
            Some(p) => {
                let (w, h) = ((p.keybox[2] - p.keybox[0]) * 4.0, (p.keybox[3] - p.keybox[1]) * 3.0);
                let crop = 1.0 - w.min(h) / w.max(h);
                (s.overscan - crop).max(0.0)
            }
            None => s.overscan,
        }
    }

    /// The tube's rect for `rect`: the whole of it, or the TV set's screen.
    fn tube_in(&self, rect: [f32; 4], s: &VideoSettings) -> [f32; 4] {
        match self.tv_photo.as_ref().filter(|_| self.framed(s)) {
            Some(p) => tv_screen_rect(rect, p.keybox),
            None => rect,
        }
    }

    /// The CRT half (or the flat raster) from a VI-output texture to `present`.
    fn present(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, vid: &wgpu::TextureView, present: &wgpu::TextureView, rect: [f32; 4], s: &VideoSettings) {
        let (d, c) = (&self.depth1.view, &self.clear.view);
        if !s.crt {
            Self::pass(encoder, present, &self.flat, &self.bind(device, vid, c, d, d), Some(rect));
            return;
        }
        Self::pass(encoder, &self.sig.view, &self.signal, &self.bind(device, vid, c, d, d), None);
        Self::pass(encoder, &self.glow_tex.view, &self.glow, &self.bind(device, &self.sig.view, c, d, d), None);
        let tube = self.tube_in(rect, s);
        Self::pass(encoder, present, &self.tube, &self.bind(device, &self.sig.view, &self.glow_tex.view, d, d), Some(tube));
        if let Some(p) = self.tv_photo.as_ref().filter(|_| self.framed(s)) {
            Self::pass(encoder, present, &self.frame_pipe, &self.bind(device, &p.tex.view, c, d, d), Some(rect));
        }
    }

    /// The whole chain: `frame` through the RDP store and the VI, then the CRT
    /// or the flat raster into `rect` (`[x0, y0, w, h]` present pixels, e.g.
    /// [`tube_rect`]) of `present`. Only `rect` is drawn: clear the rest
    /// first. Call after the guns, PD's post effects and [`Self::upload_hud`];
    /// the caller checks [`VideoSettings::active`]. One `run` per submitted
    /// encoder: the passes share one uniform buffer.
    #[allow(clippy::too_many_arguments)]
    pub fn run(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &Frame<'_>, present: &wgpu::TextureView, rect: [f32; 4], s: &VideoSettings) {
        let size = frame.size;
        self.frame = self.frame.wrapping_add(1);
        self.ensure_targets(device, size.0, size.1);
        let lines = s.resolution.lines();
        self.ensure_raster(device, lines, s.sharpness);
        self.ensure_tv(device, queue, s.tv);
        let depth = frame.gun_depth.map(|_| {
            let (w, g) = (frame.world_near_far, frame.gun_near_far);
            [w.0, w.1, g.0, g.1]
        });
        let u = self.uniform(s, size, size.1.min(lines), self.tube_in(rect, s), depth, frame.color_srgb);
        queue.write_buffer(&self.buf, 0, bytemuck::bytes_of(&u));
        let t = self.targets.as_ref().unwrap();
        let hud = self.hud.as_ref().map(|(t, _, _)| &t.view).unwrap_or(&self.clear.view);
        let d = &self.depth1.view;
        let c = &self.clear.view;
        let (dw, dg) = match frame.gun_depth {
            Some(g) => (&t.world_depth.view, g),
            None => (d, d),
        };
        Self::pass(encoder, &t.fb.view, &self.rdp, &self.bind(device, frame.color, hud, dw, dg), None);
        Self::pass(encoder, &t.vi.view, &self.vi, &self.bind(device, &t.fb.view, c, d, d), None);
        Self::pass(encoder, &t.vid.view, &self.divot, &self.bind(device, &t.vi.view, c, d, d), None);
        self.present(device, encoder, &t.vid.view, present, rect, s);
    }

    /// Only the CRT half, on an image (raw display values, RGBA8, e.g. an
    /// emulator capture at 320×240 or a line-doubled 640×480): its rows are
    /// shown on min(height, 240) lines.
    #[allow(clippy::too_many_arguments)]
    pub fn run_still(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, rgba: &[u8], size: (u32, u32), present: &wgpu::TextureView, rect: [f32; 4], s: &VideoSettings) {
        self.ensure_raster(device, LINES, s.sharpness);
        self.ensure_tv(device, queue, s.tv);
        if self.still.as_ref().is_none_or(|(_, w, h)| (*w, *h) != size) {
            let t = tex(device, "n64-still", size.0, size.1, FB, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            self.still = Some((t, size.0, size.1));
        }
        let (t, _, _) = self.still.as_ref().unwrap();
        queue.write_texture(
            t.tex.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * 4), rows_per_image: Some(size.1) },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        let lines = size.1.min(LINES);
        let u = self.uniform(s, size, lines, self.tube_in(rect, s), None, false);
        queue.write_buffer(&self.buf, 0, bytemuck::bytes_of(&u));
        let view = t.view.clone();
        self.present(device, encoder, &view, present, rect, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_shader_validates() {
        let module = naga::front::wgsl::parse_str(VIDEO_WGSL).unwrap_or_else(|e| panic!("{}", e.emit_to_string(VIDEO_WGSL)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
    }

    #[test]
    fn the_uniform_matches_the_shaders_layout() {
        // 9 vec4s, then the 65 composite taps.
        assert_eq!(std::mem::size_of::<VideoU>(), (9 + 65) * 16);
    }

    /// The composite chroma taps reject a steady subcarrier (so a flat colour
    /// decodes without stripes): demodulating Y = 1 at any phase leaves ~0.
    #[test]
    fn composite_chroma_taps_null_the_subcarrier() {
        let (taps, gain) = ntsc_taps(1.0);
        let omega = std::f32::consts::TAU * 3.579545 / (3.579545 * 9.0);
        for phase in [0.0f32, 0.7, 1.9, 3.0] {
            let (mut i, mut q) = (0.0, 0.0);
            for (k, t) in taps.iter().enumerate() {
                let phi = (k as i32 - 32) as f32 * omega + phase;
                i += t[1] * 2.0 * phi.cos();
                q += t[2] * 2.0 * phi.sin();
            }
            assert!(i.abs() < 1e-4 && q.abs() < 1e-4, "phase {phase}: leak I {i} Q {q}");
        }
        assert!(gain > 0.0 && gain < 1.0, "luma gain at fsc {gain}");
    }

    #[test]
    fn the_tube_is_the_largest_centred_4_3_rect() {
        assert_eq!(tube_rect(1920, 1080, 0.0), [240.0, 0.0, 1440.0, 1080.0]);
        assert_eq!(tube_rect(800, 1000, 0.0), [0.0, 200.0, 800.0, 600.0]);
        // A side panel narrows the area; the rect centres in what is left.
        assert_eq!(tube_rect(1920, 1080, 600.0), [600.0, 45.0, 1320.0, 990.0]);
    }

    /// Each photo decodes, is 4:3, and its screen box sits on the green screen:
    /// green at the box's centre and just inside each edge, not green a little
    /// outside the left and right edges (the bezel).
    #[test]
    fn the_key_boxes_sit_on_the_green_screens() {
        for set in [TvSet::Plain, TvSet::Bedroom] {
            let (rgba, w, h, kb) = decode_photo(set).unwrap();
            assert_eq!(w * 3, h * 4, "{set:?} is {w}x{h}");
            let green = |x: f32, y: f32| {
                let (x, y) = ((x * w as f32) as usize, (y * h as f32) as usize);
                let p = &rgba[(y * w as usize + x) * 4..][..3];
                p[1] as i32 - (p[0] as i32).max(p[2] as i32) > 60
            };
            let (cx, cy) = ((kb[0] + kb[2]) * 0.5, (kb[1] + kb[3]) * 0.5);
            let (dx, dy) = (12.0 / w as f32, 12.0 / h as f32);
            assert!(green(cx, cy), "{set:?}: centre");
            assert!(green(kb[0] + dx, cy) && green(kb[2] - dx, cy), "{set:?}: inside left/right");
            assert!(green(cx, kb[1] + dy) && green(cx, kb[3] - dy), "{set:?}: inside top/bottom");
            assert!(!green(kb[0] - dx, cy) && !green(kb[2] + dx, cy), "{set:?}: bezel left/right");
        }
    }

    #[test]
    fn the_default_is_off_and_the_presets_set_only_the_tube() {
        let v = VideoSettings::default();
        assert!(!v.active());
        assert!(VideoSettings { n64: true, ..v }.active());
        for p in Preset::ALL {
            let mut t = v;
            p.apply(&mut t);
            assert_eq!((t.n64, t.resolution, t.fb16, t.aa, t.divot, t.crt, t.tv), (v.n64, v.resolution, v.fb16, v.aa, v.divot, v.crt, v.tv));
        }
    }
}
