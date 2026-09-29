//! `am_render` (`activemenu.c:1243`), NTSC final at `g_UiScaleX` 1: the dark
//! diamond joining the edge slots, the eight slots (a translucent red box with
//! an orange border and text; black for the current one, white-bordered;
//! orange text with no border for no ammo; an active device's text pulsing),
//! the title in the middle, the pulsing selection box, an order screen's
//! heading (the simulant's name and weapon, or "All Simulants"), and below,
//! the ordered simulant's health and shield bars. The state is the sim's
//! (`pd_sim::player::activemenu`); this only draws it.

use n64::rdp::{rgba, Blend, Cc, Filter, Gfx, TriState, SV};
use pd_core::text::{self, colour_blend, FontId, TextCtx};
use pd_sim::player::activemenu::*;

fn st_shade() -> TriState<'static> {
    TriState { cc: Cc::Shade, tex: None, filter: Filter::Bilerp, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false }
}

/// A UI vertex at pixel `(x, y)` (PD's ×10 through the ortho matrix).
fn uv(gfx: &Gfx, x: i32, y: i32, c: u32) -> SV {
    let (x, y) = gfx.ortho((x * 10) as f32, (y * 10) as f32);
    SV { x, y, z: 0.0, inv_w: 1.0, s: 0.0, t: 0.0, c: rgba(c) }
}

fn font(playercount: usize) -> FontId {
    if playercount >= 2 {
        FontId::Xs
    } else {
        FontId::Sm
    }
}

/// `am_render_text` (`activemenu.c:954`): centred on `x`, 4 above `y`.
fn am_render_text(t: &mut TextCtx, text: &str, colour: u32, x: i16, y: i16, f: FontId) {
    let (_, textwidth) = text::measure(t.fonts.get(f), text, 0);
    let (mut x, mut y) = (x as i32 - textwidth / 2, y as i32 - 4);
    t.render_v2(&mut x, &mut y, text, f, colour, 320, 240, 0, 0);
}

/// The alpha byte scaled by the fade-in.
fn faded(colour: u32, alphafrac: f32) -> u32 {
    ((alphafrac * (colour & 0xff) as f32) as u32) | (colour & 0xffff_ff00)
}

/// `am_render_slot` (`activemenu.c:1106`).
fn am_render_slot(t: &mut TextCtx, r: &AmRender, slot: &AmSlot, x: i16, y: i16) {
    let (text, mode, flags) = (slot.label.as_str(), slot.mode, slot.flags);
    const OBCOL: u32 = 0xff00004f;
    const IBCOL: u32 = 0x3f00008f;
    const DEFCOL: u32 = 0xff4f00ff;
    if text.is_empty() {
        return;
    }
    let am = &r.am;
    let (paddingtop, paddingbottom) = if r.view.playercount >= 2 { (5, 3) } else { (6, 6) };
    let (x, y, half) = (x as i32, y as i32, am.slotwidth as i32 / 2);
    // The background.
    let mut colour = faded(IBCOL, am.alphafrac);
    if mode == AMSLOTMODE_FOCUSED || mode == AMSLOTMODE_CURRENT || flags & (AMSLOTFLAG_CURRENT | AMSLOTFLAG_NOAMMO) != 0 {
        colour &= 0xff;
    }
    t.gfx.fill_rect_scaled(x - half + 1, y - paddingtop + 1, x + half, y + paddingbottom, colour);
    // The border.
    let mut colour = OBCOL;
    if flags & AMSLOTFLAG_NOAMMO != 0 {
        colour &= 0xff;
    }
    if mode == AMSLOTMODE_CURRENT || flags & AMSLOTFLAG_CURRENT != 0 {
        colour = 0xffffff8f;
    }
    let colour = faded(colour, am.alphafrac);
    t.gfx.fill_rect_scaled(x - half, y - paddingtop, x + half + 1, y - paddingtop + 1, colour);
    t.gfx.fill_rect_scaled(x - half, y + paddingbottom, x + half + 1, y + paddingbottom + 1, colour);
    t.gfx.fill_rect_scaled(x - half, y - paddingtop + 1, x - half + 1, y + paddingbottom, colour);
    t.gfx.fill_rect_scaled(x + half, y - paddingtop + 1, x + half + 1, y + paddingbottom, colour);
    // The text.
    let mut colour = DEFCOL;
    if mode == AMSLOTMODE_CURRENT || flags & AMSLOTFLAG_CURRENT != 0 {
        colour = 0xffffffff;
    }
    if flags & AMSLOTFLAG_ACTIVE != 0 {
        colour = colour_blend(0xffaf8fff, colour, (pd_core::menugfx::cos_osc(t.frac20, 10.0) * 255.0) as u32);
    }
    am_render_text(t, text, faded(colour, am.alphafrac), x as i16, y as i16, font(r.view.playercount));
}

/// `am_render_aibot_info` (`activemenu.c:970`): centred at the view's top.
fn am_render_aibot_info(t: &mut TextCtx, r: &AmRender, info: &AmInfo) {
    let v = &r.view;
    let f = font(v.playercount);
    let offset = if (v.playercount == 2 && v.vsplit) || v.playercount >= 3 {
        if v.playernum.is_multiple_of(2) {
            8
        } else {
            -8
        }
    } else {
        0
    };
    let centre = |t: &TextCtx, s: &str| {
        let (h, w) = text::measure(t.fonts.get(f), s, 0);
        (v.left + (v.width as f32 * 0.5) as i32 - (w as f32 * 0.5) as i32 + offset, h)
    };
    let (mut x, textheight) = centre(t, &info.title);
    let mut y = if v.playercount >= 2 { v.top + 5 } else { v.top + 10 };
    t.render_v2(&mut x, &mut y, &info.title, f, 0xffffffff, 320, 240, 0, 0);
    if let Some(weapon) = &info.weapon {
        y += if v.playercount >= 2 { 0 } else { (textheight as f32 * 1.1) as i32 };
        let (mut x, _) = centre(t, weapon);
        t.render_v1(&mut x, &mut y, weapon, f, 0xffffffff, 0x000000ff, 320, 240, 0, 0);
    }
}

/// `am_render` for one player.
pub fn am_render(t: &mut TextCtx, r: Option<&AmRender>, bars: Option<AmBars>, view: [i32; 4], playercount: usize, playernum: usize) {
    if let Some(r) = r {
        let am = &r.am;
        let v = &r.view;
        // The diamond: the four edge slots, and an inner diamond an eighth in.
        let pos = |c: i16, row: i16| am.am_calculate_slot_position(c, row, v);
        let (top, right, bottom, left) = (pos(1, 0), pos(2, 1), pos(1, 2), pos(0, 1));
        let o = [top, right, bottom, left].map(|(x, y)| (x as i32, y as i32));
        let tmp2 = (o[1].0 * 10 - o[3].0 * 10) / 8;
        let tmp1 = (o[2].1 * 10 - o[0].1 * 10) / 8;
        let inner = [(o[0].0 * 10, o[0].1 * 10 + tmp1), (o[1].0 * 10 - tmp2, o[1].1 * 10), (o[2].0 * 10, o[2].1 * 10 - tmp1), (o[3].0 * 10 + tmp2, o[3].1 * 10)];
        let (outc, inc) = (0x22222200, 0x0000004f);
        let ov: Vec<SV> = o.iter().map(|&(x, y)| uv(t.gfx, x, y, outc)).collect();
        let iv: Vec<SV> = inner
            .iter()
            .map(|&(x10, y10)| {
                let (x, y) = t.gfx.ortho(x10 as f32, y10 as f32);
                SV { x, y, z: 0.0, inv_w: 1.0, s: 0.0, t: 0.0, c: rgba(inc) }
            })
            .collect();
        // gSPTri2(4,5,6, 6,7,4); gSPTri4(0,4,7, 7,3,0, 0,1,5, 5,4,0); gSPTri4(1,2,6, 6,5,1, 6,2,3, 3,7,6).
        let all: Vec<SV> = ov.iter().chain(iv.iter()).copied().collect();
        let st = st_shade();
        for [a, b, c] in [[4, 5, 6], [6, 7, 4], [0, 4, 7], [7, 3, 0], [0, 1, 5], [5, 4, 0], [1, 2, 6], [6, 5, 1], [6, 2, 3], [3, 7, 6]] {
            t.gfx.tri([all[a], all[b], all[c]], &st);
        }
        // The slots.
        let cramped = am.am_is_cramped(v);
        for column in 0..3i16 {
            for row in 0..3i16 {
                let (x, y) = pos(column, row);
                let slot = &r.slots[(column + row * 3) as usize];
                if column == 1 && row == 1 {
                    if !cramped {
                        am_render_text(t, &slot.label, 0xffffffff, x, y, font(v.playercount));
                    }
                } else {
                    am_render_slot(t, r, slot, x, y);
                }
            }
        }
        if let Some(info) = &r.info {
            am_render_aibot_info(t, r, info);
        }
        // The selection, pulsing between red and white.
        let (mut above, mut below) = if v.playercount >= 2 { (5, 3) } else { (6, 6) };
        let c = ((am.selpulse.sin() + 1.0) * 127.0) as u32;
        let colour = 0xff0000ff | c << 8 | c << 16;
        let mut halfwidth = am.slotwidth as i32 / 2;
        if am.slotnum == 4 {
            if cramped {
                halfwidth = 1;
                above = 2;
                below = 0;
            } else if v.playercount >= 2 {
                let (_, w) = text::measure(t.fonts.get(font(v.playercount)), &r.slots[4].label, 0);
                halfwidth = w / 2 + 2;
            }
        }
        let (sx, sy) = (am.selx as i32, am.sely as i32);
        t.gfx.fill_rect_scaled(sx - halfwidth, sy - above, sx + halfwidth + 1, sy - above + 1, colour);
        t.gfx.fill_rect_scaled(sx - halfwidth, sy + below, sx + halfwidth + 1, sy + below + 1, colour);
        t.gfx.fill_rect_scaled(sx - halfwidth, sy - above + 1, sx - halfwidth + 1, sy + below, colour);
        t.gfx.fill_rect_scaled(sx + halfwidth, sy - above + 1, sx + halfwidth + 1, sy + below, colour);
    }
    // The ordered simulant's health (green, red under a quarter) and shield.
    if let Some(b) = bars {
        let [vleft, vtop, vwidth, vheight] = view;
        let mut healthfrac = b.healthfrac;
        let redhealth = healthfrac < 0.25;
        let (barwidth, mut barheight) = if playercount >= 2 { (48, 7) } else { (64, 11) };
        let xoffset = if playercount >= 3 { if playernum & 1 == 0 { 8 } else { -8 } } else { 0 };
        let part1left = (vwidth as f32 * 0.5) as i32 + vleft - (barwidth as f32 * 0.5) as i32 + xoffset;
        let part1width = (barwidth as f32 * 0.25) as i32 - 1;
        let part2left = part1left + part1width + 2;
        if healthfrac < 0.0 {
            healthfrac = 0.0;
        }
        let mut y = vtop + vheight - if playercount >= 2 { 19 } else { 34 };
        if redhealth {
            let a2 = part1left + part1width - (part1width as f32 * (0.25 - healthfrac) * 4.0) as i32;
            t.gfx.fill_rect_scaled(a2, y, part1left + part1width, y + barheight, 0xff000060);
            t.gfx.fill_rect_scaled(part1left, y, a2, y + barheight, 0x00000080);
            t.gfx.fill_rect_scaled(part2left, y, part1left + barwidth, y + barheight, 0x00000080);
        } else {
            t.gfx.fill_rect_scaled(part1left, y, part1left + part1width, y + barheight, 0x00c00060);
            let a2 = part1left + (barwidth as f32 * healthfrac) as i32;
            t.gfx.fill_rect_scaled(part2left, y, a2, y + barheight, 0x00c00060);
            t.gfx.fill_rect_scaled(a2, y, part1left + barwidth, y + barheight, 0x00000080);
        }
        y += barheight + 2;
        barheight = (barheight as f32 * 0.75) as i32;
        let a2 = part1left + (barwidth as f32 * b.shieldfrac) as i32;
        t.gfx.fill_rect_scaled(part1left, y, a2, y + barheight, 0x00c00060);
        t.gfx.fill_rect_scaled(a2, y, part1left + barwidth, y + barheight, 0x00000080);
    }
}
