//! The drawing half of `hudmsg.c` (`hudmsgs_render`, `hudmsg_render_box`) and
//! `mp_render_modal_text` (`mplayer.c:1196`): a player's HUD messages, boxed
//! and fading in and out along the diagonal, and "Paused" / "Press START".
//! The queue and its timers are the sim's (`pd_sim::mp::hudmsg`).

use pd_core::ids::*;
use pd_core::lang::{tx, Lang, LANGBANK_MPWEAPONS};
use pd_core::menugfx::menugfx_draw_filled_rect;
use pd_core::text::{measure, FontId, TextCtx, DIAGMODE_FADEIN, DIAGMODE_FADEOUT};
use pd_sim::mp::hudmsg::{hudmsg_fadein_time, hudmsg_fadeout_time};
use pd_sim::mp::{HudMessage, ModalText};

/// `hudmsg_render_box` (`hudmsg.c:305`): the 1 px border, and the dark box
/// behind the text at `textopacity` (half black at 1). `bgopacity` opens
/// the box from its middle (always 1 in a match: fully open).
#[allow(clippy::too_many_arguments)]
fn hudmsg_render_box(t: &mut TextCtx, x1: i32, y1: i32, x2: i32, y2: i32, bgopacity: f32, bordercolour: u32, textopacity: f32) {
    let f0 = (90.0 * bgopacity * std::f32::consts::PI / 180.0).sin();
    let mut f22 = (x2 - x1) as f32 * 0.5;
    let mut f20 = (y2 - y1) as f32 * 0.5;
    if f0 < 0.5 {
        f20 = 0.0;
        f22 *= f0 + f0;
    } else {
        f20 *= (f0 - 0.5) + (f0 - 0.5);
    }
    let frac20 = t.frac20;
    menugfx_draw_filled_rect(t.gfx, t.ts, frac20, x1, y1, x2, y1 + 1, bordercolour, bordercolour);
    menugfx_draw_filled_rect(t.gfx, t.ts, frac20, x1, y2, x2, y2 + 1, bordercolour, bordercolour);
    menugfx_draw_filled_rect(t.gfx, t.ts, frac20, x1, y1 + 1, x1 + 1, y2, bordercolour, bordercolour);
    menugfx_draw_filled_rect(t.gfx, t.ts, frac20, x2, y1, x2 + 1, y2 + 1, bordercolour, bordercolour);
    if textopacity > 0.0 {
        let width = (x1 + x2) as f32 * 0.5;
        let height = (y1 + y2) as f32 * 0.5;
        // text_draw_box(x1, y1, x2, y2, 128 * textopacity): black at that alpha.
        let alpha = (128.0 * textopacity) as u32 & 0xff;
        t.gfx.fill_rect(((width - f22) + 1.0) as i32, ((height - f20) + 1.0) as i32, (width + f22) as i32, (height + f20) as i32, alpha);
    }
}

/// `hudmsgs_render` (`hudmsg.c:1355`): every showing message of this player,
/// in slot order. Returns `timerthing`: false once a bottom-aligned message
/// was drawn (the zoom range then stays off).
pub fn hudmsgs_render(t: &mut TextCtx, msgs: &[&HudMessage]) -> bool {
    let (vw, vh) = (t.gfx.w as i32, t.gfx.h as i32);
    let mut timerthing = true;
    for msg in msgs {
        if msg.opacity == 0 || msg.state == HUDMSGSTATE_FREE || msg.state == HUDMSGSTATE_QUEUED {
            continue;
        }
        let (mut textcolour, mut glowcolour) = if msg.flash {
            let sin = ((msg.timer as f32 * std::f32::consts::PI) / 60.0).sin().abs();
            let alpha = (192.0 * sin) as u32;
            ((msg.textcolour & 0xffffff00).wrapping_add(alpha), msg.glowcolour)
        } else {
            (msg.textcolour | 0xa0, msg.glowcolour)
        };
        let opacity = msg.opacity as u32;
        let fade = |c: u32| if opacity != 255 { (c & 0xffffff00) + (((opacity * (c & 0xff)) / 255) & 0xff) } else { c };
        textcolour = fade(textcolour);
        glowcolour = fade(glowcolour);
        let bordercolour = fade(msg.textcolour | 0x40);
        let (mut x, mut y) = (msg.x, msg.y);
        let draw = |t: &mut TextCtx, x: &mut i32, y: &mut i32, boxopacity: f32| {
            if msg.boxed {
                hudmsg_render_box(t, *x - 3, *y - 3, *x + msg.width + 2, *y + msg.height + 2, 1.0, bordercolour, boxopacity);
                t.render_v2(x, y, &msg.text, msg.font, textcolour, vw, vh, 0, 0);
            } else {
                t.gfx.fill_rect(*x, *y, *x + msg.width, *y + msg.height, 0);
                t.render_v1(x, y, &msg.text, msg.font, textcolour, glowcolour, vw, vh, 0, 0);
            }
        };
        if matches!(msg.state, HUDMSGSTATE_FADINGIN | HUDMSGSTATE_ONSCREEN | HUDMSGSTATE_FADINGOUT) && msg.alignv == HUDMSGALIGN_BOTTOM {
            timerthing = false;
        }
        match msg.state {
            HUDMSGSTATE_FADINGIN => {
                let spc0 = (msg.timer as f32 / hudmsg_fadein_time(msg).min(30.0)).clamp(0.0, 1.0);
                t.ts.set_diagonal_blend(x, y, msg.timer as f32 * 7.0, DIAGMODE_FADEIN);
                draw(t, &mut x, &mut y, spc0);
                t.ts.reset_blends();
            }
            HUDMSGSTATE_ONSCREEN => draw(t, &mut x, &mut y, 1.0),
            HUDMSGSTATE_FADINGOUT => {
                let spa8 = hudmsg_fadeout_time(msg);
                t.ts.set_diagonal_blend(x + msg.width, y + msg.height, (spa8 - msg.timer as f32) * 7.0, DIAGMODE_FADEOUT);
                let frac = (msg.timer as f32 / spa8.min(30.0)).min(1.0);
                draw(t, &mut x, &mut y, 1.0 - frac);
                t.ts.reset_blends();
            }
            _ => {}
        }
    }
    timerthing
}

/// `hudmsg_render_zoom_range` (`hudmsg.c:198`): the zoom (`curzoom`) and its
/// most (`maxzoom`) in the numeric font, "1.00X / 5.00X", centred over the
/// view's bottom on black boxes, at `hudmsgs_render`'s full alpha.
/// `view` is left, top, width, height; `placement` the player count, the
/// player and the vertical split (the bottom edge's shift).
pub fn hudmsg_render_zoom_range(t: &mut TextCtx, view: [i32; 4], placement: (usize, usize, bool), curzoom: f32, maxzoom: f32) {
    let [viewleft, viewtop, viewwidth, viewheight] = view;
    let (playercount, playernum, vsplit) = placement;
    let colour = (255 * 0xa0 / 255) | 0x00ff0000;
    let viewhalfwidth = viewwidth >> 1;
    let mut texty = viewheight + viewtop - 1 - 17;
    if playercount == 2 {
        texty += if !vsplit && playernum == 0 { 10 } else { 2 };
    } else if playercount >= 3 {
        texty += if playernum < 2 { 10 } else { 2 };
    }
    let (fw, fh) = (t.gfx.w as i32, t.gfx.h as i32);
    let piece = |t: &mut TextCtx, text: &str, x: i32| {
        let (th, tw) = measure(t.fonts.get(FontId::Numeric), text, 0);
        let (mut x, mut y) = (x, texty);
        // text_draw_black_uibox: black at alpha 0 behind the text.
        t.gfx.fill_rect(x, y, x + tw, y + th, 0x00000000);
        t.render_v1(&mut x, &mut y, text, FontId::Numeric, colour, 0x000000a0, fw, fh, 0, 0);
        tw
    };
    let cur = format!("{curzoom:4.2}X");
    let (_, tw) = measure(t.fonts.get(FontId::Numeric), &cur, 0);
    piece(t, &cur, viewleft + viewhalfwidth - tw - 5);
    let (_, sw) = measure(t.fonts.get(FontId::Numeric), "/", 0);
    piece(t, "/", viewleft + viewhalfwidth - (sw >> 1));
    piece(t, &format!("{maxzoom:4.2}X"), viewleft + viewhalfwidth + 5);
}

/// `mp_render_modal_text` (`mplayer.c:1196`) over a view (`[left, top, width,
/// height]`): "Paused" pulsing red to green in the middle (at the top while
/// the player's menu is open), or "Press START" in red with a challenge's
/// countdown under it.
pub fn mp_render_modal_text(t: &mut TextCtx, lang: &Lang, view: [i32; 4], modal: ModalText) {
    let [vl, vt, vw, vh] = view;
    let width = t.gfx.w as i32;
    match modal {
        ModalText::None => {}
        ModalText::Paused { menuopen } => {
            let red = (((1.0 - t.frac20) * 20.0 * 255.0) as i32).rem_euclid(255) as u32;
            let text = lang.get(tx(LANGBANK_MPWEAPONS, 40));
            let (_, tw) = measure(t.fonts.get(FontId::Md), text, 0);
            let mut x = vl + vw / 2 - tw / 2;
            let mut y = if menuopen { vt + 10 } else { vt + vh / 2 };
            // @bug kept: the crop box is vi_get_width() twice.
            t.render_v1(&mut x, &mut y, text, FontId::Md, (red << 24) | 0x00ff00ff, 0x000000ff, width, width, 0, 0);
        }
        ModalText::PressStart { deadtimer } => {
            let text = lang.get(tx(LANGBANK_MPWEAPONS, 39));
            let (th, tw) = measure(t.fonts.get(FontId::Sm), text, 0);
            let mut x = vl + vw / 2 - tw / 2;
            let mut y = vt + vh / 2;
            t.render_v1(&mut x, &mut y, text, FontId::Sm, 0xff0000ff, 0x000000ff, width, width, 0, 0);
            if deadtimer > 0 {
                let countdown = format!("{}\n", (deadtimer + 60 - 1) / 60);
                let (_, cw) = measure(t.fonts.get(FontId::Sm), &countdown, 0);
                let mut x = vl + vw / 2 - cw / 2;
                let mut y = vt + vh / 2 + th + 2;
                t.render_v1(&mut x, &mut y, &countdown, FontId::Sm, 0xff0000ff, 0x000000ff, width, width, 0, 0);
            }
        }
    }
}
