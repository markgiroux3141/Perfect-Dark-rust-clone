//! `radar.c`'s drawing (`radar_render`, `radar_draw_dot`), from the dots the
//! sim worked out ([`pd_sim::mp::radar::RadarIn`]): the green ring
//! (`TEXTURE_003C`, 64 × 64 drawn at 32 × 32, `TEXEL0 × ENV` with env green
//! at alpha 40) at the view's top right, then each dot turned by the
//! player's facing, a pixel per 2.5 m out to 40 m and on the rim at half
//! alpha beyond; the player's own mark is a box, and a mark more than 2.5 m
//! above or below is a triangle pointing that way.
//!
//! `radar_render_background` first fills the ring's box with black at alpha 0
//! (`G_RM_XLU_SURF`, so nothing shows); that fill is left out.

use n64::rdp::{Cc, Filter, Gfx, Texture};
use pd_sim::mp::radar::{RadarDot, RadarIn};

/// `g_RadarX`, `g_RadarY` (`radar.c:268`): the ring's centre for player
/// `playernum` of `playercount` (`vsplit`: two side by side), `view` its
/// left, top, width, height.
pub fn radar_position(view: [i32; 4], playercount: usize, playernum: usize, vsplit: bool) -> (i32, i32) {
    let [left, top, width, _] = view;
    let mut x = left + width - 41;
    if playercount == 2 {
        if vsplit {
            if playernum == 0 {
                x += 16;
            }
        } else {
            x -= 7;
        }
    } else if playercount >= 3 {
        x += if playernum & 1 == 0 { 7 } else { -7 };
    }
    let mut y = top + 26;
    if playercount == 2 {
        if !vsplit && playernum == 1 {
            y -= 8;
        }
    } else if playercount >= 3 {
        y -= if playernum >= 2 { 8 } else { 2 };
    }
    (x, y)
}

/// `radar_render` (`radar.c:244`) for one player.
pub fn radar_render(gfx: &mut Gfx, view: [i32; 4], placement: (usize, usize, bool), ring: Option<&Texture>, r: &RadarIn) {
    let (playercount, playernum, vsplit) = placement;
    let (rx, ry) = radar_position(view, playercount, playernum, vsplit);
    if let Some(tex) = ring {
        // func0f0b278c(centre, half-size 16): 64 texels over 32 pixels.
        let half = 16;
        let dsdx = tex.w as f32 / (2.0 * half as f32);
        let dtdy = tex.h as f32 / (2.0 * half as f32);
        gfx.tex_rect(rx - half, ry - half, rx + half, ry + half, tex, 0.0, 0.0, dsdx, dtdy, Cc::TexEnv, [0.0, 1.0, 0.0, 40.0 / 255.0], Filter::Point);
    }
    for d in &r.dots {
        radar_draw_dot(gfx, (rx, ry), r.theta, r.yindicators, d);
    }
}

/// `radar_draw_dot` (`radar.c:122`).
pub fn radar_draw_dot(gfx: &mut Gfx, centre: (i32, i32), theta: f32, yindicators: bool, d: &RadarDot) {
    let spcc = (pd_core::math::atan2f(d.dist.x, d.dist.z) * 180.0) / std::f32::consts::PI + theta + 180.0;
    let mut sqdist = (d.dist.z * d.dist.z + d.dist.x * d.dist.x).sqrt() * (1.0 / 250.0);
    let shiftamount = if sqdist < 16.0 {
        0
    } else {
        sqdist = 16.0;
        1
    };
    let x = centre.0 + ((spcc * 0.017_453_292).sin() * sqdist) as i32;
    let y = centre.1 + ((spcc * 0.017_453_292).cos() * sqdist) as i32;
    let alpha = 0xff >> shiftamount;
    // Which colour outlines and which fills: the ordinary dots outline in
    // colour1 and fill in colour2 (black); `swapcolours` the other way round.
    let (outer, inner) = if d.swapcolours { (d.colour1, d.colour2) } else { (d.colour2, d.colour1) };
    let (outer, inner) = (outer + alpha, inner + alpha);
    let mut r = |x1: i32, y1: i32, x2: i32, y2: i32, c: u32| gfx.fill_rect_scaled(x1, y1, x2, y2, c);
    if d.isself {
        // The box.
        r(x - 2, y + 2, x + 1, y + 3, outer);
        r(x - 3, y - 1, x + 2, y + 2, outer);
        r(x - 2, y - 2, x + 1, y - 1, outer);
        r(x - 1, y + 1, x, y + 2, inner);
        r(x - 2, y, x + 1, y + 1, inner);
        r(x - 1, y - 1, x, y, inner);
    } else if yindicators && d.dist.y > 250.0 {
        // Up.
        r(x - 3, y - 1, x + 2, y + 2, outer);
        r(x - 2, y - 2, x + 1, y - 1, outer);
        r(x - 2, y, x + 1, y + 1, inner);
        r(x - 1, y - 1, x, y, inner);
    } else if yindicators && d.dist.y < -250.0 {
        // Down.
        r(x - 3, y - 2, x + 2, y + 1, outer);
        r(x - 2, y + 1, x + 1, y + 2, outer);
        r(x - 2, y - 1, x + 1, y, inner);
        r(x - 1, y, x, y + 1, inner);
    } else {
        r(x - 2, y - 2, x + 2, y + 2, outer);
        r(x - 1, y - 1, x + 1, y + 1, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PD's ring positions: the top right of each view, pulled in towards
    /// the middle of the screen in split screen.
    #[test]
    fn the_ring_sits_at_each_views_top_right() {
        assert_eq!(radar_position([0, 0, 320, 220], 1, 0, false), (279, 26));
        assert_eq!(radar_position([0, 0, 320, 109], 2, 0, false), (272, 26));
        assert_eq!(radar_position([0, 110, 320, 110], 2, 1, false), (272, 128));
        assert_eq!(radar_position([0, 0, 159, 220], 2, 0, true), (134, 26));
        assert_eq!(radar_position([160, 0, 160, 220], 2, 1, true), (279, 26));
        assert_eq!(radar_position([0, 0, 159, 109], 4, 0, false), (125, 24));
        assert_eq!(radar_position([160, 110, 160, 110], 4, 3, false), (272, 128));
    }

    /// A dot 10 m straight ahead of a player facing +z (`vv_theta` 0) lands 4
    /// pixels above the centre; 60 m away, on the rim at 16 at half alpha.
    #[test]
    fn a_dot_ahead_is_above_the_centre() {
        let mut gfx = crate::hud::layer(64, 64);
        let d = RadarDot { dist: glam::Vec3::new(0.0, 0.0, 1000.0), colour1: 0x00ff0000, colour2: 0, swapcolours: false, isself: false };
        radar_draw_dot(&mut gfx, (32, 32), 0.0, true, &d);
        let lit = |g: &Gfx| (0..64 * 64).filter(|&i| g.fb[i][1] > 0.1).map(|i| ((i % 64) as i32, (i / 64) as i32)).collect::<Vec<_>>();
        let px = lit(&gfx);
        assert!(!px.is_empty());
        let cy = px.iter().map(|p| p.1).sum::<i32>() as f32 / px.len() as f32;
        assert!((cy - 28.0).abs() <= 1.0, "the dot's rows centre on {cy}");
        let mut far = crate::hud::layer(64, 64);
        radar_draw_dot(&mut far, (32, 32), 0.0, true, &RadarDot { dist: glam::Vec3::new(0.0, 0.0, 6000.0), ..d });
        let px = lit(&far);
        assert!(px.iter().all(|p| p.1 <= 18), "on the rim: {px:?}");
    }
}
