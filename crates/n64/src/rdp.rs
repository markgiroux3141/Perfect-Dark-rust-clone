//! The RDP in software: the reference rasteriser PD's menus draw with, and the
//! oracle the GPU combiner is tested against.
//!
//! Two front ends share one framebuffer ([`Gfx`]):
//!
//! * **2D** (the menus' own drawing): the scissor, `gDPFillRectangle`, shaded and
//!   textured triangles through PD's ortho and holoray matrices, texture
//!   rectangles, the menu combiner modes ([`Cc`]) and the `G_RM_XLU_SURF` blend
//!   (`src·a + dst·(1 − a)`).
//! * **3D** (models): [`raster_clipped`] takes triangles already through the
//!   vertex stage ([`crate::rsp`]), clips them at the near plane and runs the
//!   two-cycle colour combiner literally from its 16 mux ids ([`combine_cycle`]),
//!   TRILERP between mip levels ([`MipTex`]), the 3-point filter, the tile's
//!   shift / clamp / mirror / `uls` offset, the texture-edge and threshold alpha
//!   compares, z test / update / decal and the XLU blender.
//!
//! Colour maths is gamma-free: textures are raw RGBA in N64 display space.
//!
//! Coordinates: PD's ortho modelview (`ortho_configure_mtx`, savebuffer.c:48)
//! is `x·0.1 + (0.5 − w)/2`, `−y·0.1 + (0.5 + h)/2` against a 10-unit near
//! plane, so a UI vertex `(10x, 10y, −10)` lands at pixel `(x + ¼, y − ¼)`.
//! [`Gfx::ortho`] and [`Gfx::holoray`] are those two matrices.
//!
//! **Substitutions** (the hardware does it differently; the menus still match PD
//! to the pixel at 320×220):
//! * the RDP's edge walker, coverage and anti-aliasing are not emulated.
//!   Triangles are sampled at pixel centres with a top-left rule, colour is kept
//!   in f32, and `G_RM_AA_*` modes blend like their non-AA twins. The
//!   framebuffer can be quantised to RGBA5551 on output ([`Gfx::rgba8`]);
//! * the TRILERP level of detail is computed once per triangle from its texel /
//!   pixel area ratio, not per 2×2 pixel block;
//! * mip levels below the base are box-filtered from it ([`MipTex::from_rgba8`]),
//!   where PD's `tex_shrink_*` builds them at load (or the ROM stores them).
//!
//! Sources: the old repo's `pd_menu/gfx.rs` (2D) and `pd_menu/pdmodel.rs`
//! (`MipTex`, `raster`, `combine`: the CPU copy of `pdgun.wgsl`), merged. The gun
//! spike's HUD canvas (`pd_guns/font.rs` `Canvas`) and `app.rs` `shade_tri` move
//! onto [`Gfx`] when the HUD migrates (M4).

use glam::{Mat4, Vec4};

// ─── textures ────────────────────────────────────────────────────────────────

/// Texture addressing, PD's `TXMODE_*` / the tile's `cms`/`cmt`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Addr {
    Wrap,
    Clamp,
    Mirror,
}

impl Addr {
    /// PD's `TXMODE_*` numbering (0 wrap, 1 clamp, 2 mirror), as the model
    /// exporter records a tile.
    pub fn from_txmode(m: u8) -> Addr {
        match m {
            1 => Addr::Clamp,
            2 => Addr::Mirror,
            _ => Addr::Wrap,
        }
    }

    fn index(self, i: i32, n: usize) -> usize {
        let n = n as i32;
        match self {
            Addr::Clamp => i.clamp(0, n - 1) as usize,
            Addr::Wrap => i.rem_euclid(n) as usize,
            Addr::Mirror => {
                let m = i.rem_euclid(2 * n);
                (if m >= n { 2 * n - 1 - m } else { m }) as usize
            }
        }
    }
}

/// RGBA8 texels as the RDP's 0..1 values.
pub fn texels_from_rgba8(rgba: &[u8]) -> Vec<[f32; 4]> {
    rgba.chunks_exact(4).map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]).collect()
}

/// One texture for the 2D front end: RGBA 0..1, with a tile's addressing.
#[derive(Clone)]
pub struct Texture {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
    pub wrap_s: Addr,
    pub wrap_t: Addr,
}

impl Texture {
    pub fn from_rgba(w: usize, h: usize, px: Vec<[f32; 4]>, wrap_s: Addr, wrap_t: Addr) -> Texture {
        Texture { w, h, px, wrap_s, wrap_t }
    }

    pub fn from_rgba8(w: usize, h: usize, rgba: &[u8], wrap_s: Addr, wrap_t: Addr) -> Texture {
        Texture { w, h, px: texels_from_rgba8(rgba), wrap_s, wrap_t }
    }

    pub fn texel(&self, s: i32, t: i32) -> [f32; 4] {
        let x = self.wrap_s.index(s, self.w);
        let y = self.wrap_t.index(t, self.h);
        self.px[y * self.w + x]
    }

    /// `G_TF_POINT`.
    pub fn point(&self, s: f32, t: f32) -> [f32; 4] {
        self.texel(s.floor() as i32, t.floor() as i32)
    }

    /// `G_TF_BILERP`: the RDP's three-point filter (the triangle of the three
    /// texels nearest the sample, not a four-texel bilinear).
    pub fn bilerp(&self, s: f32, t: f32) -> [f32; 4] {
        three_point(|x, y| self.texel(x, y), s, t)
    }
}

/// The RDP's three-point filter over `texel(s, t)`, sample point `(s, t)` in texels.
fn three_point(texel: impl Fn(i32, i32) -> [f32; 4], s: f32, t: f32) -> [f32; 4] {
    let (s, t) = (s - 0.5, t - 0.5);
    let (s0, t0) = (s.floor(), t.floor());
    let (fs, ft) = (s - s0, t - t0);
    let (s0, t0) = (s0 as i32, t0 as i32);
    let mut out = [0.0; 4];
    if fs + ft < 1.0 {
        let (a, b, c) = (texel(s0, t0), texel(s0 + 1, t0), texel(s0, t0 + 1));
        for i in 0..4 {
            out[i] = a[i] + fs * (b[i] - a[i]) + ft * (c[i] - a[i]);
        }
    } else {
        let (d, b, c) = (texel(s0 + 1, t0 + 1), texel(s0 + 1, t0), texel(s0, t0 + 1));
        for i in 0..4 {
            out[i] = d[i] + (1.0 - fs) * (c[i] - d[i]) + (1.0 - ft) * (b[i] - d[i]);
        }
    }
    out
}

/// A texture with its mip chain, for the 3D front end's TRILERP.
pub struct MipTex {
    /// Level 0 first; each level `(w, h, RGBA 0..1)`.
    pub levels: Vec<(usize, usize, Vec<[f32; 4]>)>,
}

impl MipTex {
    /// Level 0 from RGBA8, then box-filtered levels down to at least six (or
    /// `levels`, PD's texconfig count, if more), stopping at 1×1.
    pub fn from_rgba8(w: usize, h: usize, rgba: &[u8], levels: Option<u32>) -> MipTex {
        // SUBST: PD builds the lower levels with tex_shrink_* at load (or the ROM
        // stores them, hasloddata) / we box-filter from level 0, as the menu
        // spike did; its snapshots are pinned to this.
        let mut out = vec![(w, h, texels_from_rgba8(rgba))];
        let want = levels.unwrap_or(1).clamp(1, 8) as usize;
        while out.len() < want.max(6) {
            let (pw, ph, prev) = out.last().unwrap();
            let (pw, ph) = (*pw, *ph);
            if pw <= 1 && ph <= 1 {
                break;
            }
            let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
            let mut next = vec![[0.0f32; 4]; nw * nh];
            for y in 0..nh {
                for x in 0..nw {
                    let mut acc = [0.0f32; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(pw - 1);
                        let sy = (y * 2 + dy).min(ph - 1);
                        let p = prev[sy * pw + sx];
                        for i in 0..4 {
                            acc[i] += p[i] * 0.25;
                        }
                    }
                    next[y * nw + x] = acc;
                }
            }
            out.push((nw, nh, next));
        }
        MipTex { levels: out }
    }

    /// One level, `s, t` in that level's texels.
    pub fn sample(&self, level: usize, s: f32, t: f32, cms: Addr, cmt: Addr, bilerp: bool) -> [f32; 4] {
        let (w, h, px) = &self.levels[level.min(self.levels.len() - 1)];
        let texel = |x: i32, y: i32| px[cmt.index(y, *h) * w + cms.index(x, *w)];
        if !bilerp {
            return texel(s.floor() as i32, t.floor() as i32);
        }
        three_point(texel, s, t)
    }
}

// ─── the framebuffer and the 2D front end ────────────────────────────────────

/// A colour word `0xRRGGBBAA` → RGBA 0..1.
pub fn rgba(c: u32) -> [f32; 4] {
    [((c >> 24) & 0xff) as f32 / 255.0, ((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0]
}

/// RGBA bytes → RGBA 0..1.
pub fn rgba_f(c: [u8; 4]) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0]
}

/// The 2D front end's colour combiner modes (what PD's menus set).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cc {
    /// `G_CC_SHADE`.
    Shade,
    /// `G_CC_MODULATEI`: rgb = TEXEL0 × SHADE, a = SHADE.
    ModulateI,
    /// `G_CC_MODULATEIA`: rgba = TEXEL0 × SHADE.
    ModulateIA,
    /// `TEXEL0 × ENVIRONMENT` for rgb and alpha.
    TexEnv,
    /// `G_CC_DECALRGBA`: TEXEL0.
    Decal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    Point,
    Bilerp,
}

/// Blender: `G_RM_XLU_SURF` (and its AA twin) or opaque.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    Xlu,
    Opaque,
}

/// A transformed 2D vertex: screen position, `1/w`, texture coords in texels, shade.
#[derive(Clone, Copy, Debug, Default)]
pub struct SV {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub inv_w: f32,
    pub s: f32,
    pub t: f32,
    pub c: [f32; 4],
}

pub struct TriState<'a> {
    pub cc: Cc,
    pub tex: Option<&'a Texture>,
    pub filter: Filter,
    pub blend: Blend,
    pub env: [f32; 4],
    /// Perspective-correct texture coordinates (`G_TP_PERSP`).
    pub persp: bool,
    /// Z-buffered (`G_ZBUFFER` + a `ZB_` render mode): test and update.
    pub zbuf: bool,
    /// `G_CULL_BACK`.
    pub cull_back: bool,
}

/// The framebuffer, its z-buffer and the scissor.
pub struct Gfx {
    pub w: usize,
    pub h: usize,
    /// Premultiplied RGBA. The frame itself is opaque (alpha 1); a layer
    /// ([`Gfx::push_layer`]) starts transparent and is composited back with
    /// "over", which is exactly the sequential XLU blend it stands in for.
    pub fb: Vec<[f32; 4]>,
    pub zb: Vec<f32>,
    /// `gDPSetScissor`: x1, y1, x2, y2 (exclusive), in framebuffer pixels.
    pub scissor: [i32; 4],
    /// `g_UiScaleX`: 2 in hi-res.
    pub uiscale: i32,
    layers: Vec<Vec<[f32; 4]>>,
    pending: Option<Vec<[f32; 4]>>,
}

impl Gfx {
    pub fn new(w: usize, h: usize) -> Gfx {
        Gfx { w, h, fb: vec![[0.0, 0.0, 0.0, 1.0]; w * h], zb: vec![f32::INFINITY; w * h], scissor: [0, 0, w as i32, h as i32], uiscale: 1, layers: Vec::new(), pending: None }
    }

    pub fn clear(&mut self, c: [f32; 3]) {
        self.fb.fill([c[0], c[1], c[2], 1.0]);
        self.zb.fill(f32::INFINITY);
    }

    /// Clear to transparent black: a frame another picture shows through
    /// (premultiplied, so composited with "over").
    pub fn clear_transparent(&mut self) {
        self.fb.fill([0.0; 4]);
        self.zb.fill(f32::INFINITY);
    }

    /// Draw into a fresh transparent layer until [`Gfx::swap_to_base`].
    pub fn push_layer(&mut self) {
        let fresh = vec![[0.0; 4]; self.w * self.h];
        let base = std::mem::replace(&mut self.fb, fresh);
        self.layers.push(base);
    }

    /// Back to the frame underneath, keeping the layer aside: draw what PD's
    /// display list orders *before* the layer's contents, then
    /// [`Gfx::composite_pending`].
    pub fn swap_to_base(&mut self) {
        let Some(base) = self.layers.pop() else { return };
        let layer = std::mem::replace(&mut self.fb, base);
        self.pending = Some(layer);
    }

    /// Composite the layer set aside by [`Gfx::swap_to_base`] over the frame.
    pub fn composite_pending(&mut self) {
        let Some(layer) = self.pending.take() else { return };
        for (d, s) in self.fb.iter_mut().zip(layer.iter()) {
            let ia = 1.0 - s[3];
            for i in 0..4 {
                d[i] = s[i] + d[i] * ia;
            }
        }
    }

    pub fn clear_z(&mut self) {
        self.zb.fill(f32::INFINITY);
    }

    pub fn set_scissor(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let (w, h) = (self.w as i32, self.h as i32);
        self.scissor = [x1.clamp(0, w), y1.clamp(0, h), x2.clamp(0, w), y2.clamp(0, h)];
    }

    pub fn full_scissor(&mut self) {
        self.scissor = [0, 0, self.w as i32, self.h as i32];
    }

    #[inline]
    pub fn blend_px(&mut self, x: i32, y: i32, c: [f32; 4], mode: Blend) {
        let [sx1, sy1, sx2, sy2] = self.scissor;
        if x < sx1 || y < sy1 || x >= sx2 || y >= sy2 {
            return;
        }
        let p = &mut self.fb[y as usize * self.w + x as usize];
        match mode {
            Blend::Opaque => {
                *p = [c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), 1.0];
            }
            Blend::Xlu => {
                let a = c[3].clamp(0.0, 1.0);
                if a <= 0.0 {
                    return;
                }
                for i in 0..3 {
                    p[i] = c[i].clamp(0.0, 1.0) * a + p[i] * (1.0 - a);
                }
                p[3] = a + p[3] * (1.0 - a);
            }
        }
    }

    /// `gDPFillRectangle` in 1-cycle mode with `G_CC_PRIMITIVE` + XLU
    /// (`text_begin_boxmode`, text.c:414): lower-right exclusive.
    pub fn fill_rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: u32) {
        let c = rgba(colour);
        for y in y1..y2 {
            for x in x1..x2 {
                self.blend_px(x, y, c, Blend::Xlu);
            }
        }
    }

    /// `gDPFillRectangleScaled` (gbiex.h): x multiplied by `g_UiScaleX`.
    pub fn fill_rect_scaled(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: u32) {
        let s = self.uiscale;
        self.fill_rect(x1 * s, y1, x2 * s, y2, colour);
    }

    /// A UI vertex through the ortho matrix (`ortho_begin`): PD passes pixel × 10.
    pub fn ortho(&self, x10: f32, y10: f32) -> (f32, f32) {
        (x10 * 0.1 * self.uiscale as f32 + 0.25, y10 * 0.1 - 0.25)
    }

    /// A vertex through `ortho_configure_full_mtx` + the ortho frustum (near 10):
    /// the holoray planes, whose back edge sits at `z = −10 − a1`. Returns
    /// (screen x, screen y, 1/w).
    pub fn holoray(&self, x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        let (w, h) = (self.w as f32, self.h as f32);
        let xe = x * self.uiscale as f32 + (0.5 - w) / 2.0;
        let ye = -y + (0.5 + h) / 2.0;
        let inv = 10.0 / -z;
        (w / 2.0 + xe * inv, h / 2.0 - ye * inv, 1.0 / -z)
    }

    /// Rasterise one 2D triangle.
    pub fn tri(&mut self, v: [SV; 3], st: &TriState) {
        let area = (v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y);
        if area.abs() < 1e-9 {
            return;
        }
        if st.cull_back && area > 0.0 {
            // Screen y points down, so a front face (CCW in PD's y-up clip
            // space) has negative area here.
            return;
        }
        let [sx1, sy1, sx2, sy2] = self.scissor;
        let minx = v.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).floor().max(sx1 as f32) as i32;
        let maxx = v.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).ceil().min(sx2 as f32) as i32;
        let miny = v.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).floor().max(sy1 as f32) as i32;
        let maxy = v.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).ceil().min(sy2 as f32) as i32;
        if minx >= maxx || miny >= maxy {
            return;
        }
        // Edge functions with a top-left fill rule.
        let edge = |a: &SV, b: &SV, px: f32, py: f32| (b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x);
        let sign = area.signum();
        let is_tl = |a: &SV, b: &SV| {
            let (dx, dy) = ((b.x - a.x) * sign, (b.y - a.y) * sign);
            (dy == 0.0 && dx > 0.0) || dy < 0.0
        };
        let tl = [is_tl(&v[1], &v[2]), is_tl(&v[2], &v[0]), is_tl(&v[0], &v[1])];
        let inv_area = 1.0 / area;
        for py in miny..maxy {
            let fy = py as f32 + 0.5;
            for px in minx..maxx {
                let fx = px as f32 + 0.5;
                let w0 = edge(&v[1], &v[2], fx, fy) * inv_area;
                let w1 = edge(&v[2], &v[0], fx, fy) * inv_area;
                let w2 = edge(&v[0], &v[1], fx, fy) * inv_area;
                let inside = |w: f32, tl: bool| w > 0.0 || (w == 0.0 && tl);
                if !(inside(w0, tl[0]) && inside(w1, tl[1]) && inside(w2, tl[2])) {
                    continue;
                }
                let idx = py as usize * self.w + px as usize;
                if st.zbuf {
                    let z = w0 * v[0].z + w1 * v[1].z + w2 * v[2].z;
                    if z > self.zb[idx] {
                        continue;
                    }
                    self.zb[idx] = z;
                }
                let mut shade = [0.0f32; 4];
                for i in 0..4 {
                    shade[i] = w0 * v[0].c[i] + w1 * v[1].c[i] + w2 * v[2].c[i];
                }
                let tex = st.tex.map(|t| {
                    let (s, tt) = if st.persp {
                        let iw = w0 * v[0].inv_w + w1 * v[1].inv_w + w2 * v[2].inv_w;
                        let s = (w0 * v[0].s * v[0].inv_w + w1 * v[1].s * v[1].inv_w + w2 * v[2].s * v[2].inv_w) / iw;
                        let tt = (w0 * v[0].t * v[0].inv_w + w1 * v[1].t * v[1].inv_w + w2 * v[2].t * v[2].inv_w) / iw;
                        (s, tt)
                    } else {
                        (w0 * v[0].s + w1 * v[1].s + w2 * v[2].s, w0 * v[0].t + w1 * v[1].t + w2 * v[2].t)
                    };
                    match st.filter {
                        Filter::Point => t.point(s, tt),
                        Filter::Bilerp => t.bilerp(s, tt),
                    }
                });
                let c = combine(st.cc, tex.unwrap_or([1.0; 4]), shade, st.env);
                self.blend_px(px, py, c, st.blend);
            }
        }
    }

    /// Two triangles `0,1,2` and `2,3,0` (`gSPTri2(0, 1, 2, 2, 3, 0)`).
    pub fn quad(&mut self, v: [SV; 4], st: &TriState) {
        self.tri([v[0], v[1], v[2]], st);
        self.tri([v[2], v[3], v[0]], st);
    }

    /// `gSPTextureRectangle` in 1-cycle mode, coordinates in pixels (not
    /// 10.2), `s0/t0` in texels, `dsdx/dtdy` in texels per pixel.
    #[allow(clippy::too_many_arguments)]
    pub fn tex_rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, tex: &Texture, s0: f32, t0: f32, dsdx: f32, dtdy: f32, cc: Cc, env: [f32; 4], filter: Filter) {
        for y in y1..y2 {
            for x in x1..x2 {
                let s = s0 + (x - x1) as f32 * dsdx;
                let t = t0 + (y - y1) as f32 * dtdy;
                let texel = match filter {
                    Filter::Point => tex.point(s, t),
                    Filter::Bilerp => tex.bilerp(s + 0.5, t + 0.5),
                };
                let c = combine(cc, texel, [1.0; 4], env);
                self.blend_px(x, y, c, Blend::Xlu);
            }
        }
    }

    /// The framebuffer as opaque RGBA8, optionally quantised to RGBA5551 like the N64's.
    pub fn rgba8(&self, n64: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for p in &self.fb {
            for c in &p[..3] {
                let mut v = (c.clamp(0.0, 1.0) * 255.0).round() as u32;
                if n64 {
                    v = (v >> 3) << 3;
                    v |= v >> 5;
                }
                out.push(v as u8);
            }
            out.push(255);
        }
        out
    }
}

/// The 2D front end's combiner.
pub fn combine(cc: Cc, t: [f32; 4], shade: [f32; 4], env: [f32; 4]) -> [f32; 4] {
    match cc {
        Cc::Shade => shade,
        Cc::ModulateI => [t[0] * shade[0], t[1] * shade[1], t[2] * shade[2], shade[3]],
        Cc::ModulateIA => [t[0] * shade[0], t[1] * shade[1], t[2] * shade[2], t[3] * shade[3]],
        Cc::TexEnv => [t[0] * env[0], t[1] * env[1], t[2] * env[2], t[3] * env[3]],
        Cc::Decal => t,
    }
}

// ─── the 3D front end ────────────────────────────────────────────────────────

/// Face culling (`G_CULL_BACK` / `G_CULL_FRONT` in the geometry mode).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cull {
    None,
    Back,
    Front,
    Both,
}

/// The alpha compare that decides whether a pixel is written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlphaTest {
    None,
    /// `CVG_X_ALPHA` + `ALPHA_CVG_SEL` (texture edge): keep above 0.19, then opaque.
    Edge,
    /// `G_AC_THRESHOLD` against a zero blend alpha: drop below 8/256.
    Threshold,
}

/// Tile 0 as the texture load left it.
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub cms: Addr,
    pub cmt: Addr,
    /// `2^-shift` per axis (see [`shift_scale`]).
    pub shift: [f32; 2],
    /// `uls`, `ult` in texels.
    pub ul: [f32; 2],
    /// TRILERP between levels (`G_TL_LOD` with a mipmapped load).
    pub mipmap: bool,
    /// `G_TF_BILERP` (else point sampled).
    pub bilerp: bool,
}

/// A tile's shift as a coordinate multiplier: 1..10 shift right, 11..15 left.
pub fn shift_scale(s: u8) -> f32 {
    match s {
        0 => 1.0,
        1..=10 => 1.0 / (1u32 << s) as f32,
        _ => (1u32 << (16 - s as u32)) as f32,
    }
}

/// Everything the RDP needs to draw one batch of triangles.
pub struct DrawState<'a> {
    pub two_cycle: bool,
    /// fast3d's mux order: `[a0,b0,c0,d0, Aa0,Ab0,Ac0,Ad0, a1,b1,c1,d1, Aa1,Ab1,Ac1,Ad1]`.
    pub combine: [u8; 16],
    pub tex: Option<&'a MipTex>,
    pub tile: Tile,
    pub prim: [f32; 4],
    pub env: [f32; 4],
    /// The blender's `M = CLR_MEM, B = 1MA`: alpha blending.
    pub xlu: bool,
    pub alpha_test: AlphaTest,
    pub ztest: bool,
    pub zwrite: bool,
    pub decal: bool,
    pub cull: Cull,
}

/// A vertex after the RSP: eye-space and clip-space position, texture
/// coordinates in texels (before the tile), shade RGBA 0..1.
#[derive(Clone, Copy, Default, Debug)]
pub struct PV {
    pub eye: Vec4,
    pub clip: Vec4,
    pub st: [f32; 2],
    pub shade: [f32; 4],
}

impl PV {
    fn lerp(a: &PV, b: &PV, t: f32) -> PV {
        let l = |x: f32, y: f32| x + (y - x) * t;
        PV {
            eye: a.eye + (b.eye - a.eye) * t,
            clip: a.clip + (b.clip - a.clip) * t,
            st: [l(a.st[0], b.st[0]), l(a.st[1], b.st[1])],
            shade: [l(a.shade[0], b.shade[0]), l(a.shade[1], b.shade[1]), l(a.shade[2], b.shade[2]), l(a.shade[3], b.shade[3])],
        }
    }
}

/// Where the projected triangles land: the projection and the viewport in
/// framebuffer pixels, plus the near plane distance clipped against.
pub struct View {
    pub proj: Mat4,
    pub vp: [f32; 4],
    pub near: f32,
}

/// Clip against the near plane (eye `z = −near`), then rasterise.
pub fn raster_clipped(gfx: &mut Gfx, v: [PV; 3], st: &DrawState, view: &View) {
    let inside = |p: &PV| p.eye.z <= -view.near;
    if v.iter().all(inside) {
        raster(gfx, v, st, view);
        return;
    }
    if !v.iter().any(inside) {
        return;
    }
    let mut poly: Vec<PV> = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, b) = (&v[i], &v[(i + 1) % 3]);
        if inside(a) {
            poly.push(*a);
        }
        if inside(a) != inside(b) {
            let t = (-view.near - a.eye.z) / (b.eye.z - a.eye.z);
            poly.push(PV::lerp(a, b, t));
        }
    }
    for k in 1..poly.len().saturating_sub(1) {
        raster(gfx, [poly[0], poly[k], poly[k + 1]], st, view);
    }
}

struct SP {
    x: f32,
    y: f32,
    z: f32,
    iw: f32,
    s: f32,
    t: f32,
    c: [f32; 4],
}

fn raster(gfx: &mut Gfx, v: [PV; 3], st: &DrawState, view: &View) {
    let [vx, vy, vw, vh] = view.vp;
    let sp: Vec<SP> = v
        .iter()
        .map(|p| {
            let iw = 1.0 / p.clip.w;
            let (nx, ny, nz) = (p.clip.x * iw, p.clip.y * iw, p.clip.z * iw);
            SP { x: vx + (nx + 1.0) * 0.5 * vw, y: vy + (1.0 - ny) * 0.5 * vh, z: nz, iw, s: p.st[0], t: p.st[1], c: p.shade }
        })
        .collect();
    let area = (sp[1].x - sp[0].x) * (sp[2].y - sp[0].y) - (sp[2].x - sp[0].x) * (sp[1].y - sp[0].y);
    if area.abs() < 1e-9 {
        return;
    }
    // Screen y points down: a front face (CCW, y up) has negative area here.
    let front = area < 0.0;
    match st.cull {
        Cull::Back if !front => return,
        Cull::Front if front => return,
        Cull::Both => return,
        _ => {}
    }
    let [sx1, sy1, sx2, sy2] = gfx.scissor;
    let minx = sp.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).floor().max(sx1 as f32) as i32;
    let maxx = sp.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).ceil().min(sx2 as f32) as i32;
    let miny = sp.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).floor().max(sy1 as f32) as i32;
    let maxy = sp.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).ceil().min(sy2 as f32) as i32;
    if minx >= maxx || miny >= maxy {
        return;
    }

    // Texture: tile coordinates are texels × 2^-shift − (uls, ult).
    let tex = st.tex;
    let tile = |s: f32, t: f32| (s * st.tile.shift[0] - st.tile.ul[0], t * st.tile.shift[1] - st.tile.ul[1]);
    // TRILERP LOD from the triangle's texel / pixel area ratio.
    let (lod_level, lod_frac) = match tex {
        Some(tx) if st.tile.mipmap && tx.levels.len() > 1 => {
            let stc: Vec<(f32, f32)> = sp.iter().map(|p| tile(p.s, p.t)).collect();
            let tarea = ((stc[1].0 - stc[0].0) * (stc[2].1 - stc[0].1) - (stc[2].0 - stc[0].0) * (stc[1].1 - stc[0].1)).abs();
            let lod = (0.5 * (tarea / area.abs()).max(1e-9).log2()).clamp(0.0, (tx.levels.len() - 1) as f32);
            (lod.floor() as usize, lod.fract())
        }
        _ => (0, 0.0),
    };
    let (cms, cmt, bilerp) = (st.tile.cms, st.tile.cmt, st.tile.bilerp);
    let cc = st.combine;

    let edge = |a: &SP, b: &SP, px: f32, py: f32| (b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x);
    let sign = area.signum();
    let is_tl = |a: &SP, b: &SP| {
        let (dx, dy) = ((b.x - a.x) * sign, (b.y - a.y) * sign);
        (dy == 0.0 && dx > 0.0) || dy < 0.0
    };
    let tl = [is_tl(&sp[1], &sp[2]), is_tl(&sp[2], &sp[0]), is_tl(&sp[0], &sp[1])];
    let inv_area = 1.0 / area;
    let w = gfx.w;
    for py in miny..maxy {
        let fy = py as f32 + 0.5;
        for px in minx..maxx {
            let fx = px as f32 + 0.5;
            let w0 = edge(&sp[1], &sp[2], fx, fy) * inv_area;
            let w1 = edge(&sp[2], &sp[0], fx, fy) * inv_area;
            let w2 = edge(&sp[0], &sp[1], fx, fy) * inv_area;
            let ins = |w: f32, tl: bool| w > 0.0 || (w == 0.0 && tl);
            if !(ins(w0, tl[0]) && ins(w1, tl[1]) && ins(w2, tl[2])) {
                continue;
            }
            let idx = py as usize * w + px as usize;
            let z = w0 * sp[0].z + w1 * sp[1].z + w2 * sp[2].z;
            if st.ztest {
                let limit = gfx.zb[idx] + if st.decal { 1e-4 } else { 0.0 };
                if z > limit {
                    continue;
                }
            }
            let mut shade = [0.0f32; 4];
            for i in 0..4 {
                shade[i] = (w0 * sp[0].c[i] + w1 * sp[1].c[i] + w2 * sp[2].c[i]).clamp(0.0, 1.0);
            }
            let (mut t0, mut t1) = ([1.0f32; 4], [1.0f32; 4]);
            if let Some(tx) = tex {
                let iw = w0 * sp[0].iw + w1 * sp[1].iw + w2 * sp[2].iw;
                let s = (w0 * sp[0].s * sp[0].iw + w1 * sp[1].s * sp[1].iw + w2 * sp[2].s * sp[2].iw) / iw;
                let t = (w0 * sp[0].t * sp[0].iw + w1 * sp[1].t * sp[1].iw + w2 * sp[2].t * sp[2].iw) / iw;
                let (s, t) = tile(s, t);
                let (w0l, h0l, _) = &tx.levels[0];
                let at = |lv: usize| {
                    let (lw, lh, _) = &tx.levels[lv.min(tx.levels.len() - 1)];
                    tx.sample(lv, s * *lw as f32 / *w0l as f32, t * *lh as f32 / *h0l as f32, cms, cmt, bilerp)
                };
                t0 = at(lod_level);
                t1 = if lod_frac > 0.0 { at(lod_level + 1) } else { t0 };
            }
            let inp = CombineInputs { combined: [0.0; 4], t0, t1, shade, prim: st.prim, env: st.env, lod: lod_frac };
            let mut c = combine_cycle(&cc[0..8], &inp);
            if st.two_cycle {
                let inp2 = CombineInputs { combined: c, ..inp };
                c = combine_cycle(&cc[8..16], &inp2);
            }
            match st.alpha_test {
                AlphaTest::Edge => {
                    if c[3] > 0.19 {
                        c[3] = 1.0;
                    } else {
                        continue;
                    }
                }
                AlphaTest::Threshold => {
                    if c[3] < 8.0 / 256.0 {
                        continue;
                    }
                }
                AlphaTest::None => {}
            }
            if st.zwrite {
                gfx.zb[idx] = z;
            }
            gfx.blend_px(px, py, c, if st.xlu { Blend::Xlu } else { Blend::Opaque });
        }
    }
}

/// The combiner's inputs at one pixel.
#[derive(Clone, Copy, Debug)]
pub struct CombineInputs {
    pub combined: [f32; 4],
    pub t0: [f32; 4],
    pub t1: [f32; 4],
    pub shade: [f32; 4],
    pub prim: [f32; 4],
    pub env: [f32; 4],
    /// `LOD_FRACTION`.
    pub lod: f32,
}

/// One combiner cycle, `(a − b)·c + d` for colour and alpha, from fast3d's mux
/// ids in the order `[a, b, c, d, Aa, Ab, Ac, Ad]`.
pub fn combine_cycle(m: &[u8], i: &CombineInputs) -> [f32; 4] {
    let rgb_src = |s: u8, allow_one: bool| -> [f32; 3] {
        let v = match s {
            0 => i.combined,
            1 => i.t0,
            2 => i.t1,
            3 => i.prim,
            4 => i.shade,
            5 => i.env,
            6 if allow_one => [1.0; 4],
            _ => [0.0; 4],
        };
        [v[0], v[1], v[2]]
    };
    let c_src = |s: u8| -> [f32; 3] {
        match s {
            0..=5 => rgb_src(s, false),
            7 => [i.combined[3]; 3],
            8 => [i.t0[3]; 3],
            9 => [i.t1[3]; 3],
            10 => [i.prim[3]; 3],
            11 => [i.shade[3]; 3],
            12 => [i.env[3]; 3],
            13 => [i.lod; 3],
            _ => [0.0; 3],
        }
    };
    let a_abd = |s: u8| -> f32 {
        match s {
            0 => i.combined[3],
            1 => i.t0[3],
            2 => i.t1[3],
            3 => i.prim[3],
            4 => i.shade[3],
            5 => i.env[3],
            6 => 1.0,
            _ => 0.0,
        }
    };
    let a_c = |s: u8| -> f32 {
        match s {
            0 => i.lod,
            1 => i.t0[3],
            2 => i.t1[3],
            3 => i.prim[3],
            4 => i.shade[3],
            5 => i.env[3],
            _ => 0.0,
        }
    };
    let (a, b, c, d) = (rgb_src(m[0], true), rgb_src(m[1], false), c_src(m[2]), rgb_src(m[3], true));
    let mut out = [0.0f32; 4];
    for k in 0..3 {
        out[k] = ((a[k] - b[k]) * c[k] + d[k]).clamp(0.0, 1.0);
    }
    out[3] = ((a_abd(m[4]) - a_abd(m[5])) * a_c(m[6]) + a_abd(m[7])).clamp(0.0, 1.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `G_CC_TRILERP, G_CC_MODULATEIA2` as fast3d mux ids (pd_fpgun.py).
    const TRILERP_MODULATEIA2: [u8; 16] = [2, 1, 13, 1, 2, 1, 0, 1, 0, 15, 4, 7, 0, 7, 4, 7];

    #[test]
    fn trilerp_then_modulate_is_lerped_texel_times_shade() {
        let inp = CombineInputs { combined: [0.0; 4], t0: [0.2, 0.4, 0.6, 1.0], t1: [0.6, 0.8, 1.0, 0.5], shade: [0.5, 0.5, 1.0, 1.0], prim: [1.0; 4], env: [1.0; 4], lod: 0.25 };
        let c1 = combine_cycle(&TRILERP_MODULATEIA2[0..8], &inp);
        let c2 = combine_cycle(&TRILERP_MODULATEIA2[8..16], &CombineInputs { combined: c1, ..inp });
        let lerp = |a: f32, b: f32| a + (b - a) * 0.25;
        let want = [lerp(0.2, 0.6) * 0.5, lerp(0.4, 0.8) * 0.5, lerp(0.6, 1.0), lerp(1.0, 0.5)];
        for k in 0..4 {
            assert!((c2[k] - want[k]).abs() < 1e-6, "{c2:?} vs {want:?}");
        }
    }

    #[test]
    fn three_point_filter_hits_texel_centres() {
        let px: Vec<[f32; 4]> = (0..4).map(|i| [i as f32 / 3.0, 0.0, 0.0, 1.0]).collect();
        let t = Texture::from_rgba(2, 2, px, Addr::Clamp, Addr::Clamp);
        assert_eq!(t.bilerp(0.5, 0.5), t.texel(0, 0));
        assert_eq!(t.bilerp(1.5, 1.5), t.texel(1, 1));
        let mid = t.bilerp(1.0, 0.5);
        assert!((mid[0] - (t.texel(0, 0)[0] + t.texel(1, 0)[0]) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn fill_rule_covers_a_square_exactly_once() {
        let mut g = Gfx::new(8, 8);
        g.clear([0.0; 3]);
        let v = |x: f32, y: f32| SV { x, y, z: 0.0, inv_w: 1.0, s: 0.0, t: 0.0, c: [1.0, 1.0, 1.0, 0.5] };
        let st = TriState { cc: Cc::Shade, tex: None, filter: Filter::Point, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false };
        // Two triangles sharing a diagonal: a double hit would read 0.75, not 0.5.
        g.quad([v(2.0, 2.0), v(6.0, 2.0), v(6.0, 6.0), v(2.0, 6.0)], &st);
        let covered: Vec<f32> = g.fb.iter().map(|p| p[0]).filter(|&r| r > 0.0).collect();
        assert_eq!(covered.len(), 16);
        assert!(covered.iter().all(|&r| (r - 0.5).abs() < 1e-6), "{covered:?}");
    }

    #[test]
    fn mirror_addressing_reflects() {
        assert_eq!(Addr::Mirror.index(4, 4), 3);
        assert_eq!(Addr::Mirror.index(-1, 4), 0);
        assert_eq!(Addr::Wrap.index(-1, 4), 3);
        assert_eq!(Addr::Clamp.index(9, 4), 3);
    }
}
