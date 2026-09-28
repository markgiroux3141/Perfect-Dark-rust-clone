//! The `menugfx.c` primitives the menus and the HUD both draw with: the shaded
//! quad (`menugfx_draw_tri2`), the line that takes the text's diagonal fade
//! (`menugfx_draw_projected_line`), the white comet travelling along a line
//! (`menugfx_draw_shimmer`) and the two together (`menugfx_draw_filled_rect`,
//! the dialog and HUD message borders). The rest of `menugfx.c` is
//! `pd_menu::gfx`'s.
//!
//! UI vertices are pixel × 10 through the ortho matrix, as PD builds them.
//!
//! Source: the old repo's `pd_menu/menugfx.rs` (moved from `pd_menu::gfx` so
//! the HUD messages use the same code).

use n64::rdp::{rgba, Blend, Cc, Filter, Gfx, TriState, SV};

use crate::text::{colour_blend, TextState};

fn st_shade() -> TriState<'static> {
    TriState { cc: Cc::Shade, tex: None, filter: Filter::Bilerp, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false }
}

/// A UI vertex `(x·10, y·10, −10)` with shade `c`.
fn uv(gfx: &Gfx, x10: i32, y10: i32, c: u32) -> SV {
    let (x, y) = gfx.ortho(x10 as f32, y10 as f32);
    SV { x, y, z: 0.0, inv_w: 1.0, s: 0.0, t: 0.0, c: rgba(c) }
}

/// `menugfx_draw_tri2` (menugfx.c:739): a quad, left→right (`arg7` false)
/// or top→bottom (`arg7` true) gradient.
#[allow(clippy::too_many_arguments)]
pub fn menugfx_draw_tri2(gfx: &mut Gfx, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32, vertical: bool) {
    // Vertex colours: !arg7 → 0, 4, 4, 0; arg7 → 0, 0, 4, 4.
    let v = [
        uv(gfx, x1 * 10, y1 * 10, colour1),
        uv(gfx, x2 * 10, y1 * 10, if vertical { colour1 } else { colour2 }),
        uv(gfx, x2 * 10, y2 * 10, colour2),
        uv(gfx, x1 * 10, y2 * 10, if vertical { colour2 } else { colour1 }),
    ];
    gfx.quad(v, &st_shade());
}

/// `menugfx_draw_projected_line` (menugfx.c:793): with a diagonal blend on,
/// in 15 px blocks, each end's colour through the blend.
#[allow(clippy::too_many_arguments)]
pub fn menugfx_draw_projected_line(gfx: &mut Gfx, ts: &TextState, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32) {
    if ts.has_diagonal_blend() {
        if x2 - x1 < y2 - y1 {
            // Portrait
            let numfullblocks = (y2 - y1) / 15;
            let mut parttop = y1;
            let mut partcolourtop = ts.apply_projection_colour(x1, y1, colour1);
            for i in 0..numfullblocks {
                let mut partbottom = y1 + i * 15;
                let partcolourbottom;
                if y2 - partbottom < 3 {
                    partbottom = y2;
                    partcolourbottom = ts.apply_projection_colour(x2, partbottom, colour2);
                } else {
                    let c = colour_blend(colour2, colour1, ((partbottom - y1) * 255 / (y2 - y1)) as u32);
                    // @bug: y1 should be x1
                    partcolourbottom = ts.apply_projection_colour(y1, partbottom, c);
                }
                menugfx_draw_tri2(gfx, x1, parttop, x2, partbottom, partcolourtop, partcolourbottom, false);
                parttop = partbottom;
                partcolourtop = partcolourbottom;
            }
            let partcolourbottom = ts.apply_projection_colour(x2, y2, colour2);
            menugfx_draw_tri2(gfx, x1, parttop, x2, y2, partcolourtop, partcolourbottom, false);
        } else {
            // Landscape
            let numfullblocks = (x2 - x1) / 15;
            let mut partleft = x1;
            let mut partcolourleft = ts.apply_projection_colour(x1, y1, colour1);
            for i in 0..numfullblocks {
                let mut partright = x1 + i * 15;
                let partcolourright;
                if x2 - partright < 3 {
                    partright = x2;
                    partcolourright = ts.apply_projection_colour(x2, y2, colour2);
                } else {
                    let c = colour_blend(colour2, colour1, ((partright - x1) * 255 / (x2 - x1)) as u32);
                    partcolourright = ts.apply_projection_colour(partright, y1, c);
                }
                menugfx_draw_tri2(gfx, partleft, y1, partright, y2, partcolourleft, partcolourright, false);
                partleft = partright;
                partcolourleft = partcolourright;
            }
            let partcolourright = ts.apply_projection_colour(x2, y2, colour2);
            menugfx_draw_tri2(gfx, partleft, y1, x2, y2, partcolourleft, partcolourright, false);
        }
    } else {
        menugfx_draw_tri2(gfx, x1, y1, x2, y2, colour1, colour2, false);
    }
}

/// `menugfx_draw_shimmer` (menugfx.c:885): the white comet travelling along a
/// line, positioned by `g_20SecIntervalFrac` (`frac20`).
#[allow(clippy::too_many_arguments)]
pub fn menugfx_draw_shimmer(gfx: &mut Gfx, frac20: f32, x1: i32, y1: i32, x2: i32, y2: i32, colour: u32, _arg6: bool, arg7: i32, reverse: bool) {
    let mut alpha;
    let mut minalpha = 0;
    let mut v0: i32 = if reverse { (6.0 * frac20 * 600.0) as i32 } else { ((1.0 - frac20) * 6.0 * 600.0) as i32 };
    if y2 - y1 < x2 - x1 {
        v0 = v0.wrapping_add((y1 + x1) as u32 as i32);
        v0 = v0.rem_euclid(600);
        let mut shimmerleft = x1 + v0 - arg7;
        let mut shimmerright = shimmerleft + arg7;
        alpha = 0;
        if shimmerleft < x1 {
            alpha = x1 - shimmerleft;
            shimmerleft = x1;
        }
        if shimmerright > x2 {
            minalpha = shimmerright - x2;
            shimmerright = x2;
        }
        if alpha < minalpha {
            alpha = minalpha;
        }
        alpha = (alpha * 255 / arg7).min(255);
        if x1 <= shimmerright && x2 >= shimmerleft {
            let tail = ((((colour & 0xff) * (0xff - alpha as u32)) / 255) & 0xff) | 0xffffff00;
            if reverse {
                menugfx_draw_tri2(gfx, shimmerleft, y1, shimmerright, y2, 0xffffff00, tail, false);
            } else {
                menugfx_draw_tri2(gfx, shimmerleft, y1, shimmerright, y2, tail, 0xffffff00, false);
            }
        }
    } else {
        v0 = v0.wrapping_add((y1 + x1) as u32 as i32);
        v0 = v0.rem_euclid(600);
        let mut shimmertop = y1 + v0 - arg7;
        let mut shimmerbottom = shimmertop + arg7;
        alpha = 0;
        if shimmertop < y1 {
            alpha = y1 - shimmertop;
            shimmertop = y1;
        }
        if shimmerbottom > y2 {
            minalpha = shimmerbottom - y2;
            shimmerbottom = y2;
        }
        if alpha < minalpha {
            alpha = minalpha;
        }
        alpha = (alpha * 255 / arg7).min(255);
        if y1 <= shimmerbottom && y2 >= shimmertop {
            let tail = ((((colour & 0xff) * (0xff - alpha as u32)) / 255) & 0xff) | 0xffffff00;
            if reverse {
                menugfx_draw_tri2(gfx, x1, shimmertop, x2, shimmerbottom, 0xffffff00, tail, true);
            } else {
                menugfx_draw_tri2(gfx, x1, shimmertop, x2, shimmerbottom, tail, 0xffffff00, true);
            }
        }
    }
}

/// `menugfx_draw_filled_rect` (menugfx.c:1002): a projected line and its comet.
#[allow(clippy::too_many_arguments)]
pub fn menugfx_draw_filled_rect(gfx: &mut Gfx, ts: &TextState, frac20: f32, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32) {
    menugfx_draw_projected_line(gfx, ts, x1, y1, x2, y2, colour1, colour2);
    menugfx_draw_shimmer(gfx, frac20, x1, y1, x2, y2, colour1, false, 10, false);
}
