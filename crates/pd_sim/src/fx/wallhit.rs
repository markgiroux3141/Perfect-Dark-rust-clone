//! Bullet holes and scorches: `wallhit_create` and
//! `wallhit_create_with_20_args` (`wallhit.c`), a textured quad on the surface
//! with a colour per corner.
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
    for c in cols.iter_mut() {
        let (g, a) = match ty {
            WallhitType::Bullet => (255 - rng.random() % 40, if alpha != 0 { alpha } else { 255 }),
            WallhitType::Soft => {
                let g = rng.random() % 70;
                (g, if alpha != 0 { alpha } else { 255 - rng.random() % 50 })
            }
            WallhitType::Scorch => {
                let g = rng.random() % 50;
                (g, if alpha != 0 { alpha } else { 255 - rng.random() % 80 })
            }
            WallhitType::Paint | WallhitType::Blood => (255, 255),
        };
        let g = ((g as f32 * frac) as u32 & 0xff) as f32 / 255.0;
        *c = [g, g, g, a as f32 / 255.0];
    }
    Wallhit { corners, texnum, cols }
}
