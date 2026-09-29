//! Bullet holes, scorches and blood: `wallhit_create` and
//! `wallhit_create_with_20_args` (`wallhit.c`), a textured quad on the surface
//! with a colour per corner; a splat grows into place over its `timermax`
//! and a fading one thins away (`wallhits_tick`).
//!
//! `// SUBST:` PD keeps up to `g_MaxBgWallhitsPerRoom` holes per room (and
//! `g_MaxPropWallhits` on props), fading the oldest when a room fills
//! (`wallhit.c:770`, `wallhits_tick`) / one world-wide list of
//! [`MAX_WALLHITS`] where the oldest disappears at once.

use glam::Vec3;
use pd_core::ids::*;
use pd_core::rng::Rng;

/// The world-wide cap standing in for PD's per-room ones.
pub const MAX_WALLHITS: usize = 80;

/// `g_WallhitTexes[].width` (`wallhit.c:49`), cm.
const WALLHIT_SIZE: [f32; 18] = [10.0, 6.0, 8.0, 6.0, 8.0, 12.0, 6.0, 100.0, 24.0, 20.0, 20.0, 20.0, 20.0, 6.0, 8.0, 12.0, 4.0, 6.0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum WallhitType {
    Bullet,
    Soft,
    Scorch,
    Paint,
    Blood,
}

/// `g_WallhitTexes[].type`.
const WALLHIT_TYPE: [WallhitType; 18] = {
    use WallhitType::*;
    [Bullet, Bullet, Soft, Bullet, Bullet, Bullet, Bullet, Scorch, Paint, Blood, Blood, Blood, Blood, Bullet, Bullet, Bullet, Bullet, Bullet]
};

#[derive(Clone, Debug)]
pub struct Wallhit {
    pub corners: [Vec3; 4],
    /// `WALLHITTEX_*`.
    pub texnum: usize,
    /// `finalcolours[4]`: RGBA 0..1, one per corner.
    pub cols: [[f32; 4]; 4],
    /// `basecolours[].a` (0..1): the alpha the timers scale.
    pub basealpha: [f32; 4],
    /// `timermax`, `timercur`, `timerspeed`, `expanding`, `fading`: a splat
    /// grows in; a faded one shrinks its alpha to nothing.
    pub timermax: u32,
    pub timercur: u32,
    pub timerspeed: u32,
    pub expanding: bool,
    pub fading: bool,
    /// `vertices2`: the corners while it grows (drawn instead of `corners`).
    pub growing: Option<[Vec3; 4]>,
    /// `chrprop`: the chr whose blood it is; `createdframe`.
    pub chr: Option<usize>,
    pub createdframe: i32,
}

impl Wallhit {
    /// The quad as drawn this frame.
    pub fn draw_corners(&self) -> [Vec3; 4] {
        self.growing.unwrap_or(self.corners)
    }

    /// `IS_BLOOD_DROP` (`wallhit.c`).
    pub fn is_blood_drop(&self) -> bool {
        self.texnum == WALLHITTEX_BLOOD4
    }

    /// `wallhit_fade` (`wallhit.c:245`): fade out over `time` ticks.
    pub fn wallhit_fade(&mut self, time: u32) {
        if !self.fading {
            if self.timermax == 0 {
                self.timermax = time;
                self.timercur = time;
            }
            self.fading = true;
            self.expanding = false;
        }
    }

    /// `wallhits_tick`'s part for one wallhit (`wallhit.c:463`); false once
    /// it has faded away (`wallhit_free`).
    pub fn tick(&mut self, lvupdate240: i32) -> bool {
        let mut f0 = (lvupdate240 as f32 + 2.0) * 0.25;
        if self.timerspeed != 8 {
            f0 *= 0.6 * ((self.timerspeed as f32 - 8.0) * 0.125);
        }
        if self.timermax == 0 {
            self.growing = None;
            for (c, &a) in self.cols.iter_mut().zip(&self.basealpha) {
                c[3] = a;
            }
            return true;
        }
        let amount = (f0 + 0.5) as u32;
        if self.expanding {
            if self.timercur > self.timermax {
                self.timermax = 0;
                self.timercur = 0;
            }
            self.timercur += amount;
        } else if amount < self.timercur {
            self.timercur -= amount;
        } else {
            return false;
        }
        if self.timermax == 0 {
            return true;
        }
        let mut f24 = (self.timercur as f32 / self.timermax as f32).min(1.0);
        if self.expanding {
            let f30 = 0.8 * (std::f32::consts::FRAC_PI_2 * f24).sin();
            let mid = (self.corners[0] + self.corners[1] + self.corners[2] + self.corners[3]) * 0.25;
            self.growing = Some(self.corners.map(|c| mid + (c - mid) * (0.2 + f30)));
            f24 = (f24 * 2.0).min(1.0);
        }
        for (c, &a) in self.cols.iter_mut().zip(&self.basealpha) {
            c[3] = ((a * 255.0 * f24) as u32).min(255) as f32 / 255.0;
        }
        true
    }
}

/// The options `wallhit_create_with_20_args` takes beyond a hole's: the
/// blood's colour (`g_WallhitBloodColour`), `timermax`/`timerspeed` (a splat
/// grows in), whose blood it is and the frame.
#[derive(Clone, Copy, Debug)]
pub struct WallhitExtra {
    pub blood: [u8; 3],
    pub timermax: u32,
    pub timerspeed: u32,
    pub chr: Option<usize>,
    pub frame: i32,
}

impl Default for WallhitExtra {
    fn default() -> Self {
        // wallhit_choose_blood_colour with no chr (`wallhit.c:232`).
        WallhitExtra { blood: [0x40, 0x0a, 0x0a], timermax: 0, timerspeed: 0, chr: None, frame: 0 }
    }
}

/// `wallhit_create` (`wallhit.c:637`): 0.6–0.7 of the texture's size.
pub fn wallhit_create(rng: &mut Rng, pos: Vec3, normal: Vec3, gunpos: Vec3, texnum: usize, brightness: f32) -> Wallhit {
    let scale = rng.randomfrac() * 0.1 + 0.6;
    let width = WALLHIT_SIZE[texnum] * scale;
    let height = WALLHIT_SIZE[texnum] * scale;
    wallhit_create_with_20_args(rng, pos, normal, Some(gunpos), texnum, width, height, 0xff, 0xff, 0, brightness)
}

/// `wallhit_create_with_20_args` (`wallhit.c:652`) for a BG hit: a quad on the
/// surface oriented from the normal, flipped to face `arg2` (the gun or the
/// blast), with per-corner colours by type × `room_get_final_brightness_for_player`.
#[allow(clippy::too_many_arguments)]
pub fn wallhit_create_with_20_args(rng: &mut Rng, pos: Vec3, normal: Vec3, arg2: Option<Vec3>, texnum: usize, width: f32, height: f32, minalpha: u8, maxalpha: u8, rotdeg: u32, brightness: f32) -> Wallhit {
    wallhit_create_with_extra(rng, pos, normal, arg2, texnum, width, height, minalpha, maxalpha, rotdeg, brightness, WallhitExtra::default())
}

/// [`wallhit_create_with_20_args`] with its blood colour, timers and owner.
#[allow(clippy::too_many_arguments)]
pub fn wallhit_create_with_extra(rng: &mut Rng, pos: Vec3, normal: Vec3, arg2: Option<Vec3>, texnum: usize, width: f32, height: f32, minalpha: u8, maxalpha: u8, rotdeg: u32, brightness: f32, extra: WallhitExtra) -> Wallhit {
    let n = normal.normalize_or_zero();
    // NTSC 1.0+: BULLET2, blood, BP glass and METAL keep the given rotdeg; the
    // rest spin at random.
    let rotdeg = match texnum {
        WALLHITTEX_BULLET2 | WALLHITTEX_BLOOD1..=WALLHITTEX_BPGLASS3 | WALLHITTEX_METAL => rotdeg,
        _ => rng.random() % 360,
    };
    let eps = 1e-6;
    let (xz, yz, zz) = (normal.x.abs() < eps, normal.y.abs() < eps, normal.z.abs() < eps);
    let (mut u, mut v);
    if xz && zz {
        u = Vec3::new(-1.0, 0.0, 0.0);
        v = Vec3::new(0.0, 0.0, if normal.y >= 0.0 { -1.0 } else { 1.0 });
    } else if xz && yz {
        u = Vec3::new(if normal.z >= 0.0 { 1.0 } else { -1.0 }, 0.0, 0.0);
        v = Vec3::new(0.0, -1.0, 0.0);
    } else if yz && zz {
        u = Vec3::new(0.0, if normal.x >= 0.0 { -1.0 } else { 1.0 }, 0.0);
        v = Vec3::new(0.0, 0.0, 1.0);
    } else {
        let f0 = (n.x * n.x + n.z * n.z).sqrt();
        let (xv, zv) = (n.x / f0, n.z / f0);
        u = Vec3::new(zv, 0.0, -xv);
        v = Vec3::new(n.y * xv, -f0, n.y * zv);
    }
    if rotdeg != 0 {
        let (s, c) = (rotdeg as f32 * 0.017_453_292).sin_cos();
        let (u0, v0) = (u, v);
        u = u0 * c + v0 * s;
        v = u0 * -s + v0 * c;
    }
    // The source on the far side of the plane flips v (the "sum < 0" test).
    if let Some(src) = arg2 {
        if (src - pos).dot(n) < 0.0 {
            v = -v;
        }
    }
    let u = u * width;
    let v = v * height;
    let c0 = u + v;
    let c1 = u - v;
    let corners = [pos + c0, pos + c1, pos - c0, pos + (v - u)];
    let frac = brightness / 255.0;
    let range = maxalpha as u32 - minalpha as u32;
    let alpha = if range != 0 { minalpha as u32 + rng.random() % range } else { 0 };
    let ty = WALLHIT_TYPE[texnum];
    let mut cols = [[0.0f32; 4]; 4];
    let mut basealpha = [0.0f32; 4];
    for (c, ba) in cols.iter_mut().zip(basealpha.iter_mut()) {
        let or255 = |a: u32| if a != 0 { a } else { 255 };
        let (rgb, a) = match ty {
            WallhitType::Bullet => {
                let g = 255 - rng.random() % 40;
                ([g; 3], or255(alpha))
            }
            WallhitType::Soft => {
                let g = rng.random() % 70;
                ([g; 3], if alpha != 0 { alpha } else { 255 - rng.random() % 50 })
            }
            WallhitType::Scorch => {
                let g = rng.random() % 50;
                ([g; 3], if alpha != 0 { alpha } else { 255 - rng.random() % 80 })
            }
            WallhitType::Blood => (extra.blood.map(|v| v as u32), or255(alpha)),
            // Paintball (an option the Combat Simulator never sets).
            WallhitType::Paint => ([255; 3], or255(alpha)),
        };
        let f = |v: u32| ((v as f32 * frac) as u32 & 0xff) as f32 / 255.0;
        *ba = a as f32 / 255.0;
        *c = [f(rgb[0]), f(rgb[1]), f(rgb[2]), if extra.timermax != 0 { 0.0 } else { *ba }];
    }
    Wallhit {
        corners,
        texnum,
        cols,
        basealpha,
        timermax: extra.timermax,
        timercur: 0,
        timerspeed: if extra.timerspeed != 0 { extra.timerspeed } else { 8 },
        expanding: extra.timermax != 0,
        fading: false,
        growing: None,
        chr: extra.chr,
        createdframe: extra.frame,
    }
}
