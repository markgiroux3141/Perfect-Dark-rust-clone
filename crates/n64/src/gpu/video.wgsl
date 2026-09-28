// N64 video + CRT: what happens to a Perfect Dark frame between the RDP and
// the viewer's eye, as full-screen passes over the low-res scene target.
//
//   fs_rdp    scene (+ HUD) → 16-bit framebuffer: RGBA5551 store with the
//             RDP's Bayer colour dither, and a coverage estimate in alpha.
//   fs_vi     the VI's per-pixel filter: the dither ("restore") filter on
//             fully covered pixels, the anti-alias filter on edge pixels.
//   fs_divot  the VI's divot filter (median of 3 across AA edges).
//   fs_flat   N64 without the CRT: the 240-line raster, nearest, 4:3.
//   fs_signal the analogue signal, one row per scanline: RGB, S-Video or
//             composite (NTSC YIQ, QAM on the 3.58 MHz subcarrier, decoded).
//   fs_glow   halation: a coarse blur of the tube's light.
//   fs_tube   the tube: CRT gamma, per-line gaussian beam that widens with
//             brightness, phosphor mask, curvature, overscan.
//   fs_frame  the TV set: a photo of a CRT drawn over the tube, its green
//             screen keyed out.
//
// Sources. PD's video mode is from the decomp: 320×220 NTSC, 16-bit colour
// image (`vi.c:633`), `OS_VI_GAMMA_OFF | OS_VI_DITHER_FILTER_ON` (`vi.c:287`),
// divot on and VI mode LAN1 (anti-aliased) (`vitbl.c`), Bayer as the usual
// colour dither (`G_CD_BAYER`). The RDP dither rule, the Bayer matrix and the
// VI restore / AA / divot filters follow angrylion-rdp-plus (the accurate
// software RDP), reproduced from memory and NOT checked against its source.
// Coverage is the one thing we can't have: the RDP writes a 3-bit coverage per
// pixel and we render point-sampled, so `coverage()` estimates it from depth
// discontinuities (see there).
//
// Colour spaces: the shaders upstream (`combiner.wgsl`) work in display space
// and write raw 0..255 values. If the scene target is sRGB, a load gives linear
// light and `linear_to_srgb` recovers the raw value (`encode.x`). The
// framebuffer passes carry raw display values in Rgba8Unorm. The tube converts
// voltage to light with the CRT's own gamma (2.4, BT.1886), because PD runs
// with VI gamma off, i.e. it was graded for the tube's response. The light goes
// out through `display()`: as is to an sRGB present target (the hardware
// encodes it), sRGB-encoded here to a raw one (`encode.y`).
//
// Source: the old repo's `pd_guns/n64video.wgsl`, which assumed an sRGB scene
// and an sRGB swapchain; `encode` is what this port adds.

struct Video {
    size: vec4<f32>,   // source width, height, lines shown (≤ 240), frame
    flags: vec4<u32>,  // 16-bit colour + dither, dither filter, VI anti-alias, divot
    depth: vec4<f32>,  // world near, far, gun near, gun far
    crt: vec4<f32>,    // signal (0 RGB, 1 S-Video, 2 composite), scanlines 0..1, mask (0 none, 1 aperture grille, 2 slot, 3 shadow), mask strength
    crt2: vec4<f32>,   // curvature, halation, overscan, composite luma gain at the subcarrier
    tube: vec4<f32>,   // the tube's rect in present pixels: x0, y0, w, h
    raster: vec4<f32>, // raster lines (240, or 480 for the beyond-N64 mode), signal bandwidth scale, TV-set frame (1 = on), _
    keybox: vec4<f32>, // the TV photo's screen box (photo uv x0, y0, x1, y1): only pixels inside it are keyed
    encode: vec4<f32>, // scene target is sRGB (1) or raw (0), present target is sRGB (1) or raw (0), _, _
    // Composite decoder taps k = −32..32 at NTSC_DT: (luma, I, Q, _) weights,
    // each normalised to sum 1 (built by `video::ntsc_taps`).
    ntsc: array<vec4<f32>, 65>,
};

@group(0) @binding(0) var<uniform> v: Video;
@group(0) @binding(1) var t0: texture_2d<f32>;
@group(0) @binding(2) var t1: texture_2d<f32>;
@group(0) @binding(3) var d_world: texture_depth_2d;
@group(0) @binding(4) var d_gun: texture_depth_2d;
@group(0) @binding(5) var s_lin: sampler;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
// NTSC active line time (µs) and colour subcarrier (MHz).
const ACTIVE_US: f32 = 52.66;
const FSC: f32 = 3.579545;
// Composite tap spacing: exactly 1/9 of a subcarrier period.
const NTSC_DT: f32 = 1.0 / (3.579545 * 9.0);

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    var o: VOut;
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Linear light out to the present target (see "Colour spaces" above).
fn display(light: vec3<f32>) -> vec3<f32> {
    if (v.encode.y != 0.0) {
        return light;
    }
    return linear_to_srgb(clamp(light, vec3<f32>(0.0), vec3<f32>(1.0)));
}

fn clamp_px(p: vec2<i32>, t: texture_2d<f32>) -> vec2<i32> {
    let d = vec2<i32>(textureDimensions(t, 0));
    return clamp(p, vec2<i32>(0), d - vec2<i32>(1));
}

fn ld(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(t0, clamp_px(p, t0), 0);
}

// ─── RDP: the 16-bit store ──────────────────────────────────────────────────

// 1/z in view space, which is affine in window depth, so a plane has a zero
// second difference in it.
fn inv_z(d: f32, n: f32, f: f32) -> f32 {
    return (f - d * (f - n)) / (n * f);
}

fn depth_at(t: texture_depth_2d, p: vec2<i32>) -> f32 {
    let dim = vec2<i32>(textureDimensions(t, 0));
    return textureLoad(t, clamp(p, vec2<i32>(0), dim - vec2<i32>(1)), 0);
}

// Coverage estimate, 0..7 like the RDP's. The real value is the fraction of a
// pixel a polygon covers; we render one sample per pixel, so instead: a pixel
// on the NEAR side of a depth discontinuity is the edge of whatever covers it
// (cvg 4 ≈ half), everything else is full (7). The gun is found by its own
// depth (the gun pass clears depth, so anything < 1 there is gun); the world
// by a second-difference test on 1/z, which is zero across any plane at any
// slant, so grazing floors don't read as edges but creases can.
fn coverage(p: vec2<i32>) -> f32 {
    if (v.flags.z == 0u) {
        return 7.0;
    }
    var offs = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    let gun_c = depth_at(d_gun, p) < 1.0;
    var any_gun = false;
    var any_world = false;
    for (var i = 0; i < 4; i++) {
        let g = depth_at(d_gun, p + offs[i]) < 1.0;
        any_gun = any_gun || g;
        any_world = any_world || !g;
    }
    if (gun_c && any_world) {
        return 4.0; // the gun's silhouette
    }
    if (!gun_c && any_gun) {
        return 7.0; // the world behind the gun's edge is fully covered
    }
    var n = v.depth.x;
    var f = v.depth.y;
    if (gun_c) {
        n = v.depth.z;
        f = v.depth.w;
    }
    for (var axis = 0; axis < 2; axis++) {
        let o = offs[axis * 2 + 1];
        var wa: f32;
        var wc: f32;
        var wb: f32;
        if (gun_c) {
            wa = inv_z(depth_at(d_gun, p - o), n, f);
            wc = inv_z(depth_at(d_gun, p), n, f);
            wb = inv_z(depth_at(d_gun, p + o), n, f);
        } else {
            wa = inv_z(depth_at(d_world, p - o), n, f);
            wc = inv_z(depth_at(d_world, p), n, f);
            wb = inv_z(depth_at(d_world, p + o), n, f);
        }
        if (abs(wa + wb - 2.0 * wc) > 0.08 * wc && wc > min(wa, wb)) {
            return 4.0;
        }
    }
    return 7.0;
}

// The RDP's store. `rgb_dither` (angrylion): a channel whose low 3 bits exceed
// the Bayer threshold rounds up to the next 5-bit step, then the store keeps
// the top 5 bits; the VI reads them back as `v << 3` (so white is 248). The
// HUD (`t1`, premultiplied, in PD pixels) is composited here, in display
// space like the N64's blender; PD's text runs with G_CD_DISABLE, so HUD
// pixels truncate instead of dithering, and HUD pixels are never AA edges.
@fragment
fn fs_rdp(in: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.pos.xy));
    var scene = clamp(textureLoad(t0, p, 0).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if (v.encode.x != 0.0) {
        scene = linear_to_srgb(scene);
    }
    // The HUD is in PD pixels (320 wide); hi-res modes show it scaled up.
    let hp = vec2<i32>(floor(vec2<f32>(p) * vec2<f32>(textureDimensions(t1, 0)) / v.size.xy));
    let hud = textureLoad(t1, clamp_px(hp, t1), 0);
    let c = hud.rgb + scene * (1.0 - hud.a);
    var c8 = floor(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + 0.5);
    if (v.flags.x != 0u) {
        if (hud.a < 0.5) {
            var bayer = array<f32, 16>(0.0, 4.0, 1.0, 5.0, 4.0, 0.0, 5.0, 1.0, 3.0, 7.0, 2.0, 6.0, 7.0, 3.0, 6.0, 2.0);
            let d = bayer[((p.y & 3) << 2) | (p.x & 3)];
            let low = c8 - floor(c8 / 8.0) * 8.0;
            let up = select(c8 - low + 8.0, vec3<f32>(255.0), c8 > vec3<f32>(247.0));
            c8 = select(c8, up, low > vec3<f32>(d));
        }
        c8 = floor(c8 / 8.0) * 8.0;
    }
    var cvg = coverage(p);
    if (hud.a > 0.0) {
        cvg = 7.0;
    }
    return vec4<f32>(c8 / 255.0, cvg / 7.0);
}

// ─── VI: the per-pixel filters ──────────────────────────────────────────────

fn cvg_at(p: vec2<i32>) -> f32 {
    return floor(ld(p).a * 7.0 + 0.5);
}

// `restore_filter16`: each of the 8 neighbours nudges the pixel by one 8-bit
// step towards itself (+1 if its 5-bit value is higher, −1 if lower), which
// averages the dither pattern back into the 3 bits the store dropped.
fn restore(p: vec2<i32>, c8: vec3<f32>) -> vec3<f32> {
    let c5 = floor(c8 / 8.0);
    var acc = c8;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            if (i == 0 && j == 0) {
                continue;
            }
            let n5 = floor(ld(p + vec2<i32>(i, j)).rgb * 255.0 / 8.0 + 0.01);
            acc += sign(n5 - c5);
        }
    }
    return clamp(acc, vec3<f32>(0.0), vec3<f32>(255.0));
}

// `video_filter16`: an edge pixel is blended towards an estimated background
// by its missing coverage. The candidates are the pixel itself and the fully
// covered ones of six neighbours (up-left, up-right, two left, two right,
// down-left, down-right); the background is second-max + second-min − self.
fn anti_alias(p: vec2<i32>, c8: vec3<f32>, cvg: f32) -> vec3<f32> {
    var offs = array<vec2<i32>, 6>(vec2<i32>(-1, -1), vec2<i32>(1, -1), vec2<i32>(-2, 0), vec2<i32>(2, 0), vec2<i32>(-1, 1), vec2<i32>(1, 1));
    var mx1 = c8;
    var mx2 = c8;
    var mn1 = c8;
    var mn2 = c8;
    var count = 1;
    for (var i = 0; i < 6; i++) {
        let n = ld(p + offs[i]);
        if (floor(n.a * 7.0 + 0.5) < 7.0) {
            continue;
        }
        let c = n.rgb * 255.0;
        if (count == 1) {
            // Two candidates: the second max is the smaller, the second min the larger.
            mx2 = min(mx1, c);
            mx1 = max(mx1, c);
            mn2 = max(mn1, c);
            mn1 = min(mn1, c);
        } else {
            mx2 = max(mx2, min(mx1, c));
            mx1 = max(mx1, c);
            mn2 = min(mn2, max(mn1, c));
            mn1 = min(mn1, c);
        }
        count++;
    }
    let coeff = 7.0 - cvg;
    let back = mx2 + mn2 - 2.0 * c8;
    return clamp(c8 + floor((back * coeff + 4.0) / 8.0), vec3<f32>(0.0), vec3<f32>(255.0));
}

@fragment
fn fs_vi(in: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.pos.xy));
    let px = ld(p);
    let c8 = floor(px.rgb * 255.0 + 0.5);
    let cvg = floor(px.a * 7.0 + 0.5);
    var out = c8;
    if (cvg >= 7.0) {
        if (v.flags.y != 0u && v.flags.x != 0u) {
            out = restore(p, c8);
        }
    } else if (v.flags.z != 0u) {
        out = anti_alias(p, c8, cvg);
    }
    return vec4<f32>(out / 255.0, px.a);
}

fn median3(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> vec3<f32> {
    return max(min(a, b), min(max(a, b), c));
}

// The divot filter: where any of three horizontal neighbours is an AA edge,
// each channel takes the median of the three, which removes the one-pixel
// notches the AA filter leaves on crossing edges.
@fragment
fn fs_divot(in: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.pos.xy));
    let c = ld(p);
    if (v.flags.w == 0u) {
        return vec4<f32>(c.rgb, 1.0);
    }
    let l = ld(p - vec2<i32>(1, 0));
    let r = ld(p + vec2<i32>(1, 0));
    let edge = min(min(l.a, c.a), r.a) < 0.99;
    if (!edge) {
        return vec4<f32>(c.rgb, 1.0);
    }
    return vec4<f32>(median3(l.rgb, c.rgb, r.rgb), 1.0);
}

// Which source row a raster line shows (−1 = the black above/below PD's
// 220-line picture, which the VI centres in the 240).
fn source_row(line: f32) -> f32 {
    let lines = v.size.z;
    let top = floor((v.raster.x - lines) * 0.5);
    let y = line - top;
    return select(-1.0, y, y >= 0.0 && y < lines);
}

@fragment
fn fs_flat(in: VOut) -> @location(0) vec4<f32> {
    let y = source_row(floor(in.uv.y * v.raster.x));
    if (y < 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let x = floor(in.uv.x * v.size.x);
    let row = floor((y + 0.5) / v.size.z * v.size.y);
    let c = ld(vec2<i32>(i32(x), i32(row))).rgb;
    return vec4<f32>(display(srgb_to_linear(c)), 1.0);
}

// ─── the analogue signal ────────────────────────────────────────────────────

const RGB_TO_YIQ = mat3x3<f32>(
    vec3<f32>(0.299, 0.595716, 0.211456),
    vec3<f32>(0.587, -0.274453, -0.522591),
    vec3<f32>(0.114, -0.321263, 0.311135),
);
const YIQ_TO_RGB = mat3x3<f32>(
    vec3<f32>(1.0, 1.0, 1.0),
    vec3<f32>(0.9563, -0.2721, -1.1070),
    vec3<f32>(0.6210, -0.6474, 1.7046),
);

fn gauss(x: f32, s: f32) -> f32 {
    let q = x / s;
    return exp(-0.5 * q * q);
}

// The VI's output along the line at time `t` (µs into the active line):
// linear between framebuffer pixels (the VI resamples 320 → 640), blanking
// outside the active line.
fn line_rgb(t: f32, vv: f32) -> vec3<f32> {
    let u = t / ACTIVE_US;
    if (u < 0.0 || u > 1.0) {
        return vec3<f32>(0.0);
    }
    return textureSampleLevel(t0, s_lin, vec2<f32>(u, vv), 0.0).rgb;
}

// Gaussian low-pass σ (µs) for a −3 dB bandwidth in MHz.
fn sigma_us(mhz: f32) -> f32 {
    return 0.1325 / (mhz * max(v.raster.y, 0.1));
}

@fragment
fn fs_signal(in: VOut) -> @location(0) vec4<f32> {
    let line = floor(in.pos.y);
    let y = source_row(line);
    if (y < 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let vv = (y + 0.5) / v.size.z;
    let t = in.uv.x * ACTIVE_US;
    let mode = u32(v.crt.x);
    if (mode == 0u) {
        // RGB: three clean channels, a little analogue softening.
        let s = sigma_us(6.0);
        var acc = vec3<f32>(0.0);
        var ws = 0.0;
        for (var k = -6; k <= 6; k++) {
            let dt = f32(k) * 0.01;
            let w = gauss(dt, s);
            acc += w * line_rgb(t + dt, vv);
            ws += w;
        }
        return vec4<f32>(acc / ws, 1.0);
    }
    if (mode == 1u) {
        // S-Video: luma and chroma on separate wires. Y at ~5 MHz, I/Q at
        // ~1.3 MHz each: the colour smears, the detail doesn't.
        let sy = sigma_us(5.0);
        let sc = sigma_us(1.3);
        var yv = 0.0;
        var wy = 0.0;
        var iq = vec2<f32>(0.0);
        var wc = 0.0;
        for (var k = -16; k <= 16; k++) {
            let dt = f32(k) * 0.02;
            let c = RGB_TO_YIQ * line_rgb(t + dt, vv);
            let a = gauss(dt, sy);
            let b = gauss(dt, sc);
            yv += a * c.x;
            wy += a;
            iq += b * c.yz;
            wc += b;
        }
        let rgb = YIQ_TO_RGB * vec3<f32>(yv / wy, iq / wc);
        return vec4<f32>(max(rgb, vec3<f32>(0.0)), 1.0);
    }
    // Composite: one wire. Encode Y + I·cos φ + Q·sin φ on the subcarrier
    // (phase flips 180° a line, 227.5 cycles), then decode the way a basic TV
    // does: I and Q by synchronous demodulation and a low-pass, luma by
    // low-passing the signal and subtracting the decoded chroma. The chroma
    // low-pass includes a box exactly one subcarrier period long, which nulls
    // 3.58 MHz, so a flat colour decodes clean; where the decode is wrong
    // (colour edges, luma detail near 3.58 MHz) you get composite's
    // artefacts: dots along colour edges, rainbowing on fine stripes.
    let ph = PI * line;
    var ys = 0.0;
    var isum = 0.0;
    var qsum = 0.0;
    for (var k = -32; k <= 32; k++) {
        let tk = t + f32(k) * NTSC_DT;
        let c = RGB_TO_YIQ * line_rgb(tk, vv);
        let phi = TAU * FSC * tk + ph;
        let cs = cos(phi);
        let sn = sin(phi);
        let s = c.x + c.y * cs + c.z * sn;
        let w = v.ntsc[k + 32];
        ys += w.x * s;
        isum += w.y * 2.0 * s * cs;
        qsum += w.z * 2.0 * s * sn;
    }
    let phi0 = TAU * FSC * t + ph;
    let y_d = ys - v.crt2.w * (isum * cos(phi0) + qsum * sin(phi0));
    let rgb = YIQ_TO_RGB * vec3<f32>(y_d, isum, qsum);
    return vec4<f32>(max(rgb, vec3<f32>(0.0)), 1.0);
}

// ─── the tube ───────────────────────────────────────────────────────────────

// Signal voltage → phosphor light: BT.1886 with zero black level.
fn tube_light(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.4));
}

@fragment
fn fs_glow(in: VOut) -> @location(0) vec4<f32> {
    let dim = vec2<f32>(textureDimensions(t0, 0));
    let step = vec2<f32>(8.0, 2.0) / dim;
    var acc = vec3<f32>(0.0);
    var ws = 0.0;
    for (var j = -2; j <= 2; j++) {
        for (var i = -2; i <= 2; i++) {
            let o = vec2<f32>(f32(i), f32(j));
            let w = gauss(length(o), 1.3);
            acc += w * tube_light(textureSampleLevel(t0, s_lin, in.uv + o * step, 0.0).rgb);
            ws += w;
        }
    }
    return vec4<f32>(acc / ws, 1.0);
}

// The phosphor pattern at a tube pixel, averaging 1 at full strength after
// `mask_gain`. Triads are 3 px wide, scaled up on big tubes so a 4K window
// shows about the same triad count as a 1080p one.
fn mask(p: vec2<f32>) -> vec3<f32> {
    let kind = u32(v.crt.z);
    let dark = 0.2;
    let scale = max(1.0, floor(v.tube.w / 720.0));
    let q = floor(p / scale);
    var m = vec3<f32>(1.0);
    if (kind == 1u || kind == 2u) {
        // Aperture grille (Trinitron): vertical R, G, B stripes.
        let i = u32(q.x) % 3u;
        m = vec3<f32>(dark);
        m[i] = 1.0;
        if (kind == 2u) {
            // Slot mask: the stripes broken every 4 rows, staggered per triad.
            let row = (u32(q.y) + (u32(q.x) / 3u % 2u) * 2u) % 4u;
            if (row == 0u) {
                m = m * dark;
            }
        }
    } else if (kind == 3u) {
        // Shadow (dot) mask: triads offset each row.
        let x = q.x + q.y * 3.0;
        let f = fract(x / 6.0);
        m = vec3<f32>(dark);
        if (f < 1.0 / 3.0) {
            m.r = 1.0;
        } else if (f < 2.0 / 3.0) {
            m.g = 1.0;
        } else {
            m.b = 1.0;
        }
    }
    return m;
}

@fragment
fn fs_tube(in: VOut) -> @location(0) vec4<f32> {
    // Curvature: each axis bows with the square of the other. Inside the TV
    // set's photo (`fs_frame`) the photo's own glass gives the shape, so the
    // raster stays flat and fills the screen box.
    let framed = v.raster.z != 0.0;
    var cc = in.uv * 2.0 - 1.0;
    let k = select(v.crt2.x, 0.0, framed);
    cc = cc * (1.0 + k * vec2<f32>(cc.y * cc.y, cc.x * cc.x));
    // Rounded corners and the black outside the glass.
    let r = 0.05;
    let qd = abs(cc) - vec2<f32>(1.0 - r);
    let corner = length(max(qd, vec2<f32>(0.0))) - r;
    let glass = select(1.0 - smoothstep(-0.004, 0.004, corner), 1.0, framed);
    if (glass <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // Overscan: the bezel hides the raster's outer edge.
    let uv = 0.5 + (cc * 0.5) * (1.0 - v.crt2.z);
    // The beam: sum the gaussian spot of each nearby line, its width growing
    // with the line's brightness (the beam blooms), each normalised so a flat
    // field keeps its brightness.
    let s = v.crt.y;
    // σ in lines. The gap between lines only shows below σ ≈ 0.3 (a gaussian
    // comb's ripple is exp(−2π²σ²)), so the range runs from merged (0.5) to
    // a dark gap on dim lines (0.12) with bright lines blooming to 0.28.
    let sd = mix(0.5, 0.12, s);
    let sb = mix(0.5, 0.28, s);
    let lines_n = v.raster.x;
    let ly = uv.y * lines_n;
    let l0 = floor(ly - 0.5);
    var light = vec3<f32>(0.0);
    for (var j = -1; j <= 2; j++) {
        let line = l0 + f32(j);
        if (line < 0.0 || line >= lines_n) {
            continue;
        }
        let c = tube_light(textureSampleLevel(t0, s_lin, vec2<f32>(uv.x, (line + 0.5) / lines_n), 0.0).rgb);
        let sig = mix(vec3<f32>(sd), vec3<f32>(sb), sqrt(min(c, vec3<f32>(1.0))));
        let d = ly - (line + 0.5);
        light += c * exp(-0.5 * (d * d) / (sig * sig)) / (sig * 2.5066283);
    }
    // Phosphor mask, with the brightness it costs given back.
    let strength = select(0.0, v.crt.w, u32(v.crt.z) != 0u);
    let m = mix(vec3<f32>(1.0), mask(in.pos.xy - v.tube.xy), strength);
    let mean = mix(1.0, (1.0 + 2.0 * 0.2) / 3.0, strength);
    light = light * m / mix(1.0, mean, 0.7);
    // Halation: light scattered in the glass.
    light += v.crt2.y * textureSampleLevel(t1, s_lin, uv, 0.0).rgb;
    // A little falloff towards the corners.
    let vig = 1.0 - 0.12 * dot(cc * cc, cc * cc);
    return vec4<f32>(display(light * vig * glass), 1.0);
}

// The TV set: a photo of a CRT with a chroma-green screen, drawn over the tube
// with the green keyed out. The key is how far green exceeds the other two
// channels, softened so the screen's anti-aliased edge blends, and it only
// applies inside the screen box, so green things elsewhere in the photo (the
// bedroom's soda can) stay. The despill clamps green to the larger of red and
// blue, which leaves the grey and black plastic alone; it runs a little past
// the box too, because the bezel's inner lip carries the green screen's
// reflection in the photo. On a raw present target the key's soft edge blends
// in display space rather than in linear light (a slightly darker fringe).
@fragment
fn fs_frame(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(t0, s_lin, in.uv, 0.0).rgb;
    let kb = v.keybox;
    let pad = vec2<f32>(0.035, 0.045);
    let near = all(in.uv >= kb.xy - pad) && all(in.uv <= kb.zw + pad);
    if (!near) {
        return vec4<f32>(display(srgb_to_linear(c)), 1.0);
    }
    let rgb = vec3<f32>(c.r, min(c.g, max(c.r, c.b)), c.b);
    let inside = all(in.uv >= kb.xy) && all(in.uv <= kb.zw);
    var a = 1.0;
    if (inside) {
        a = 1.0 - smoothstep(0.12, 0.40, c.g - max(c.r, c.b));
    }
    return vec4<f32>(display(srgb_to_linear(rgb)), a);
}
