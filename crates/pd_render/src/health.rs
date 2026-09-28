//! The health bar (`player_render_health_bar`, `player.c:2683`, and
//! `healthbar_draw`, `healthbar.c:133`) and the screen fade
//! (`player_draw_stored_fade`, `player.c:2287`) on the HUD layer.
//!
//! PD's curved bar is a small mesh in the y = 0 plane, looked down on from
//! (0, 370, 0) with −z up the screen through the scene's perspective; here it
//! is projected on the CPU and drawn with the layer's shaded triangles
//! (`G_CC_SHADE`, `G_RM_XLU_SURF`, no culling). The timers that open and close
//! it and the fade's colour are the sim's (`pd_sim::player::health`).
//!
//! Source: the old repo's `pd_complex/health.rs` (`healthbar_strips`,
//! `draw_health_bar`).

use glam::{Mat4, Vec3, Vec4};
use n64::rdp::{rgba, Blend, Cc, Filter, Gfx, TriState, SV};

/// `struct marker` (`healthbar.c:13`).
#[derive(Clone, Copy, Default)]
struct Marker {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    frac: f32,
}

/// `healthbar_maybe_insert_marker` (`healthbar.c:21`).
fn insert_marker(markers: &mut [Marker], indexes: &mut [i32], fillfrac: f32) -> usize {
    let fillfrac = fillfrac.clamp(0.0, 1.0);
    let mut len = 0i32;
    for &i in indexes.iter() {
        len = len.max(i);
    }
    let len = (len + 1) as usize;
    for i in 0..len {
        let (i1, i2) = (indexes[i], indexes[i + 1]);
        if i1 < 0 || i2 < 0 {
            continue;
        }
        let (a, b) = (markers[i1 as usize], markers[i2 as usize]);
        if a.frac < fillfrac && b.frac > fillfrac {
            let t = (fillfrac - a.frac) / (b.frac - a.frac);
            markers[len] = Marker { x1: a.x1 + t * (b.x1 - a.x1), y1: a.y1 + t * (b.y1 - a.y1), x2: a.x2 + t * (b.x2 - a.x2), y2: a.y2 + t * (b.y2 - a.y2), frac: fillfrac };
            for j in (i + 1..len).rev() {
                indexes[j + 1] = indexes[j];
            }
            indexes[i + 1] = len as i32;
            return 1;
        }
    }
    0
}

/// `healthbar_choose_colour` (`healthbar.c:80`).
fn choose_colour(fillcol: u32, bgcol: u32, exc: f32, inc: f32, frac: f32) -> u32 {
    if frac >= inc {
        return bgcol;
    }
    if frac <= exc {
        return fillcol;
    }
    let mult = (frac - exc) / (inc - exc);
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as i32;
    let mix = |s: u32| (ch(fillcol, s) + ((ch(bgcol, s) - ch(fillcol, s)) as f32 * mult) as i32) as u32 & 0xff;
    mix(24) << 24 | mix(16) << 16 | mix(8) << 8 | mix(0)
}

/// A marker list as strip vertices `(x, z, colour)`: each marker's pair, in order.
fn strip(markers: &[Marker], idx: &[i32], offx: f32, offy: f32, colour: impl Fn(f32) -> u32) -> Vec<(f32, f32, u32)> {
    let mut v = Vec::new();
    for &i in idx {
        let mk = markers[i as usize];
        let c = colour(mk.frac);
        v.push(((mk.x1 as i32) as f32 + offx, (mk.y1 as i32) as f32 + offy, c));
        v.push(((mk.x2 as i32) as f32 + offx, (mk.y2 as i32) as f32 + offy, c));
    }
    v
}

/// `healthbar_draw(gdl, NULL, 0, 0)` (`healthbar.c:133`) as triangle strips:
/// the shield ring (`apparentarmour`, filling the other way), then the armour
/// (green, the health above a quarter) and the trauma (red, below it).
fn healthbar_strips(apparenthealth: f32, apparentarmour: f32, heightfrac: f32) -> Vec<Vec<(f32, f32, u32)>> {
    let (radmax, radmed, radmin) = (30.0f32, 18.0f32, 12.0f32);
    let (len1, len2, len3) = (170.0f32, 47.0f32, 40.0f32);
    let (shieldcol, armourcol, traumacol, bgcol) = (0x10500090u32, 0x00c00060u32, 0xff000060u32, 0x00000080u32);
    let (offx, offy) = (-85.0f32, -185.0f32);
    let (shieldfade, armourfade, traumafade) = (100.0f32, 100.0f32, 200.0f32);
    let hf = heightfrac;
    let shieldfrac = apparentarmour;
    let armourfrac = ((apparenthealth - 0.25) / 0.75).max(0.0);
    let traumafrac = ((0.25 - apparenthealth) * 4.0).max(0.0);
    let m = |x1: f32, y1: f32, x2: f32, y2: f32, frac: f32| Marker { x1, y1, x2, y2, frac };

    let mut shield = [Marker::default(); 12];
    shield[..10].copy_from_slice(&[
        m(len1 + radmax * 1.08, 0.0, len1 + radmed, 0.0, 0.0),
        m(len1 + radmax * 0.924 * 1.04, hf * radmax * 0.383, len1 + radmed * 0.924, hf * radmed * 0.383, 0.05),
        m(len1 + radmax * 0.707 * 1.02, hf * radmax * 0.707, len1 + radmed * 0.707, hf * radmed * 0.707, 0.1),
        m(len1 + radmax * 0.383, hf * radmax * 0.924, len1 + radmed * 0.383, hf * radmed * 0.924, 0.15),
        m(len1, hf * radmax, len1, hf * radmed, 0.2),
        m(0.0, hf * radmax, 0.0, hf * radmed, 0.8),
        m(-radmax * 0.383, hf * radmax * 0.924, -radmed * 0.383, hf * radmed * 0.924, 0.85),
        m(-radmax * 0.707 * 1.02, hf * radmax * 0.707, -radmed * 0.707, hf * radmed * 0.707, 0.9),
        m(-radmax * 0.924 * 1.04, hf * radmax * 0.383, -radmed * 0.924, hf * radmed * 0.383, 0.95),
        m(-radmax * 1.08, 0.0, -radmed, 0.0, 1.0),
    ]);
    let mut armour = [Marker::default(); 8];
    armour[..6].copy_from_slice(&[
        m(len2, hf * radmin, len2, -hf * radmin, 0.0),
        m(len1, hf * radmin, len1, -hf * radmin, 0.9),
        m(len1 + radmin * 0.342, hf * radmin * 0.94, len1 + radmin * 0.342, -hf * radmin * 0.94, 0.94),
        m(len1 + radmin * 0.643, hf * radmin * 0.766, len1 + radmin * 0.643, -hf * radmin * 0.766, 0.97),
        m(len1 + radmin * 0.866, hf * radmin * 0.5, len1 + radmin * 0.866, -hf * radmin * 0.5, 0.99),
        m(len1 + radmin * 0.985, hf * radmin * 0.174, len1 + radmin * 0.985, -hf * radmin * 0.174, 1.0),
    ]);
    let mut trauma = [Marker::default(); 8];
    trauma[..6].copy_from_slice(&[
        m(len3, hf * radmin, len3, -hf * radmin, 0.0),
        m(0.0, hf * radmin, 0.0, -hf * radmin, 0.8),
        m(-radmin * 0.383, hf * radmin * 0.924, -radmin * 0.383, -hf * radmin * 0.924, 0.85),
        m(-radmin * 0.707, hf * radmin * 0.7070, -radmin * 0.707, -hf * radmin * 0.7070, 0.9),
        m(-radmin * 0.924, hf * radmin * 0.383, -radmin * 0.924, -hf * radmin * 0.383, 0.95),
        m(-radmin, 0.0, -radmin, 0.0, 1.0),
    ]);

    let mut out = Vec::new();
    // The shield fills the other way (shielddir = 1).
    let sfrac = 1.0 - shieldfrac;
    let inc = (1.0 + shieldfade * 0.001) * sfrac;
    let exc = inc - shieldfade * 0.001;
    let mut sidx: [i32; 12] = std::array::from_fn(|i| if i < 10 { i as i32 } else { -1 });
    let mut n = 10;
    n += insert_marker(&mut shield, &mut sidx, exc);
    n += insert_marker(&mut shield, &mut sidx, inc);
    out.push(strip(&shield, &sidx[..n], offx, offy, |f| choose_colour(bgcol, shieldcol, exc, inc, f)));
    for (markers, frac, fade, col) in [(&mut armour, armourfrac, armourfade, armourcol), (&mut trauma, traumafrac, traumafade, traumacol)] {
        let inc = (1.0 + fade * 0.001) * frac;
        let exc = inc - fade * 0.001;
        let mut idx: [i32; 8] = std::array::from_fn(|i| if i < 6 { i as i32 } else { -1 });
        let mut n = 6;
        n += insert_marker(markers, &mut idx, exc);
        n += insert_marker(markers, &mut idx, inc);
        out.push(strip(markers, &idx[..n], offx, offy, |f| choose_colour(col, bgcol, exc, inc, f)));
    }
    out
}

/// `player_render_health_bar` (`player.c:2683`) into the view `[x, y, w, h]`
/// of the layer: the bar's plane seen from (0, 370, 0) looking at the origin
/// with −z up the screen, through the view's perspective (`fovy` degrees).
pub fn draw_health_bar(gfx: &mut Gfx, view: [i32; 4], apparenthealth: f32, apparentarmour: f32, heightfrac: f32, fovy: f32) {
    let [vx, vy, vw, vh] = view.map(|v| v as f32);
    let eye = pd_core::math::view_matrix(Vec3::new(0.0, 370.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, -1.0));
    let proj = Mat4::perspective_rh(fovy.to_radians(), vw / vh, 10.0, 10000.0);
    let pv = proj * eye;
    let to_px = |x: f32, z: f32, c: u32| -> Option<SV> {
        let p = pv * Vec4::new(x, 0.0, z, 1.0);
        (p.w > 0.0).then(|| SV { x: vx + (p.x / p.w * 0.5 + 0.5) * vw, y: vy + (0.5 - p.y / p.w * 0.5) * vh, c: rgba(c), ..SV::default() })
    };
    let st = TriState { cc: Cc::Shade, tex: None, filter: Filter::Point, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false };
    for s in healthbar_strips(apparenthealth, apparentarmour, heightfrac) {
        for k in 0..s.len().saturating_sub(2) {
            let v: Option<Vec<SV>> = s[k..k + 3].iter().map(|&(x, z, c)| to_px(x, z, c)).collect();
            if let Some(v) = v {
                gfx.tri([v[0], v[1], v[2]], &st);
            }
        }
    }
}

/// `player_draw_fade` (`player.c:2261`): the view filled with `rgb` at `frac`.
pub fn draw_fade(gfx: &mut Gfx, view: [i32; 4], rgb: [i32; 3], frac: f32) {
    if frac > 0.0 {
        let [x, y, w, h] = view;
        let c = (rgb[0].clamp(0, 255) as u32) << 24 | (rgb[1].clamp(0, 255) as u32) << 16 | (rgb[2].clamp(0, 255) as u32) << 8 | (frac * 255.0) as u32 & 0xff;
        gfx.fill_rect(x, y, x + w, y + h, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(health: f32, armour: f32) -> (Gfx, usize, usize) {
        let (w, h) = (320usize, 220usize);
        let mut g = crate::hud::layer(w, h);
        draw_health_bar(&mut g, [0, 0, w as i32, h as i32], health, armour, 1.0, 60.0);
        (g, w, h)
    }

    /// The bar sits at the top of the view, green at full health, red at low.
    #[test]
    fn the_health_bar_fills_green_then_red_and_sits_at_the_top() {
        let colour_at = |health: f32| {
            let (g, w, h) = draw(health, 0.0);
            let (mut green, mut red, mut top, mut bottom) = (0, 0, h, 0);
            for y in 0..h {
                for x in 0..w {
                    let p = g.fb[y * w + x];
                    if p[3] > 0.0 {
                        top = top.min(y);
                        bottom = bottom.max(y);
                        if p[1] > p[0] * 1.5 {
                            green += 1;
                        }
                        if p[0] > p[1] * 1.5 {
                            red += 1;
                        }
                    }
                }
            }
            (green, red, top, bottom, h)
        };
        let (g1, r1, top, bottom, h) = colour_at(1.0);
        assert!(g1 > 200 && r1 == 0, "full health: green {g1} red {r1}");
        assert!(bottom < h / 3, "the bar is at the top: rows {top}..{bottom}");
        let (g2, r2, ..) = colour_at(0.1);
        assert!(r2 > 20 && g2 < g1 / 4, "low health: green {g2} red {r2}");
    }

    /// A shield fills the ring around the bar (`apparentarmour`).
    #[test]
    fn a_shield_fills_the_ring() {
        let (none, ..) = draw(1.0, 0.0);
        let (full, ..) = draw(1.0, 1.0);
        let (half, ..) = draw(1.0, 0.5);
        let differ = |a: &Gfx, b: &Gfx| a.fb.iter().zip(&b.fb).filter(|(p, q)| (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs() > 0.02).count();
        let (d_full, d_half) = (differ(&none, &full), differ(&none, &half));
        assert!(d_full > 150, "a full shield changes the ring: {d_full} px");
        assert!(d_half > 40 && d_half < d_full, "half a shield, part of it: {d_half} of {d_full}");
    }
}
