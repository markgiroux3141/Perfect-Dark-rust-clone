//! `menugfx.c` and the holoray half of `savebuffer.c`: every shape the menus
//! draw that isn't text — dialog backgrounds and borders, the gradient title
//! bars, the white "comets" (`menugfx_draw_shimmer`), list group headers,
//! dropdown backgrounds, sliders, chevrons, checkboxes, the blurred backdrop
//! and the Combat Simulator's two rotating cones.
//!
//! Each function builds the same vertices PD does (UI vertices are pixel × 10
//! through the ortho matrix, holoray vertices are pixels with a depth) and
//! hands them to [`n64::rdp::Gfx`]. Vertex `colour` bytes in PD are offsets
//! into the `gSPColor` array (0, 4, 8 → colours[0], [1], [2]).

use n64::rdp::{rgba, Blend, Cc, Filter, TriState, SV};
use super::menu::MenuDialog;
use pd_core::text::colour_blend;
use super::types::*;
use super::MenuSystem;

const MENU_COLOURS: [[u32; 15]; 6] = [
    [0x20202000, 0x20202000, 0x20202000, 0x4f4f4f00, 0x00000000, 0x00000000, 0x4f4f4f00, 0x4f4f4f00, 0x4f4f4f00, 0x4f4f4f00, 0x00000000, 0x00000000, 0x4f4f4f00, 0x00000000, 0x00000000],
    [0x0060bf7f, 0x0000507f, 0x00f0ff7f, 0xffffffff, 0x00002f7f, 0x00006f7f, 0x00ffffff, 0x007f7fff, 0xffffffff, 0x8fffffff, 0x000044ff, 0x000030ff, 0x7f7fffff, 0xffffffff, 0x6644ff7f],
    [0xbf00007f, 0x5000007f, 0xff00007f, 0xffff00ff, 0x2f00007f, 0x6f00007f, 0xff7050ff, 0x7f0000ff, 0xffff00ff, 0xff9070ff, 0x440000ff, 0x003000ff, 0xffff00ff, 0xffffffff, 0xff44447f],
    [0x00bf007f, 0x0050007f, 0x00ff007f, 0xffff00ff, 0x002f007f, 0x00ff0028, 0x55ff55ff, 0x006f00af, 0xffffffff, 0x00000000, 0x004400ff, 0x003000ff, 0xffff00ff, 0xffffffff, 0x44ff447f],
    [0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0x00000000, 0xffffff5f, 0xffffffff, 0xffffff7f, 0xffffffff],
    [0xaaaaaaff, 0xaaaaaa7f, 0xaaaaaaff, 0xffffffff, 0xffffff2f, 0xffffffff, 0xffffffff, 0xffffffff, 0xff8888ff, 0xffffffff, 0x00000000, 0xffffff5f, 0xffffffff, 0xffffff7f, 0xffffffff],
];

const MENU_WAVE1_COLOURS: [[u32; 15]; 6] = [
    [0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0x4f4f4f00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0x00000000],
    [0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0x006f6faf, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0x00000000],
    [0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0x006f6faf, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0x00000000],
    [0xffffff00, 0xffffff00, 0xffffff00, 0xff7f0000, 0xffffff00, 0xffffff00, 0x00ffff00, 0x006f6faf, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0xffffff00, 0x00000000],
    [0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffff5f, 0xffffffff, 0xffffff7f, 0xffffffff],
    [0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffff5f, 0xffffffff, 0xffffff7f, 0xffffffff],
];

const MENU_WAVE2_COLOURS: [[u32; 15]; 6] = [
    [0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x4f4f4f00, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x00000000],
    [0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x006f6faf, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x00000000],
    [0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x006f6faf, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x00000000],
    [0x44444400, 0x44444400, 0x44444400, 0x00ff0000, 0x44444400, 0x44444400, 0xffff0000, 0x006f6faf, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x44444400, 0x00000000],
    [0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffff5f, 0xffffffff, 0xffffff7f, 0xffffffff],
    [0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffff7f, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffff5f, 0xffffffff, 0xffffff7f, 0xffffffff],
];

/// `struct menucolourpalette` field (types.h:5228), as an index into the rows above.
#[derive(Clone, Copy)]
pub enum Pal {
    DialogBorder1 = 0,
    DialogTitlebg = 1,
    DialogBorder2 = 2,
    DialogTitlefg = 3,
    DialogBodybg = 4,
    Unused14 = 5,
    ItemUnfocused = 6,
    ItemDisabled = 7,
    ItemFocusedInner = 8,
    CheckboxCheckedUnfocused = 9,
    ItemFocusedOuter = 10,
    ListgroupHeaderbg = 11,
    ListgroupHeaderfg = 12,
}

pub fn menu_colour(ty: u8, p: Pal) -> u32 {
    MENU_COLOURS[ty as usize][p as usize]
}
pub fn wave1(ty: u8, p: Pal) -> u32 {
    MENU_WAVE1_COLOURS[ty as usize][p as usize]
}
pub fn wave2(ty: u8, p: Pal) -> u32 {
    MENU_WAVE2_COLOURS[ty as usize][p as usize]
}

/// `MIXCOLOUR(dialog, field)` (menu.h): the palette entry, blended from the
/// old dialog type to the new one while `transitionfrac >= 0`.
pub fn mixcolour(d: &MenuDialog, p: Pal) -> u32 {
    if d.transitionfrac < 0.0 {
        menu_colour(d.ty, p)
    } else {
        colour_blend(menu_colour(d.type2, p), menu_colour(d.ty, p), d.colourweight)
    }
}

pub use pd_core::menugfx::{cos_osc, sin_osc};
/// `menu_get_linear_osc_pause_frac` (game_006900.c:139).
pub fn linear_osc_pause_frac(frac: f32) -> f32 {
    let ival = (frac * 4.0) as i32;
    let fval = frac * 4.0 - ((ival / 4) as f32) * 4.0;
    if fval < 1.0 {
        fval
    } else if fval < 2.0 {
        1.0
    } else if fval < 3.0 {
        1.0 - (fval - 2.0)
    } else {
        0.0
    }
}

fn st_shade() -> TriState<'static> {
    TriState { cc: Cc::Shade, tex: None, filter: Filter::Bilerp, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false }
}

impl MenuSystem {
    /// A UI vertex `(x·10, y·10, −10)` with shade `c`.
    fn uv(&self, x10: i32, y10: i32, c: u32) -> SV {
        let (x, y) = self.draw.gfx.ortho(x10 as f32, y10 as f32);
        SV { x, y, z: 0.0, inv_w: 1.0, s: 0.0, t: 0.0, c: rgba(c) }
    }

    /// `menugfx_draw_tri2` (menugfx.c:739): `pd_core::menugfx`'s.
    pub fn menugfx_draw_tri2(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32, vertical: bool) {
        pd_core::menugfx::menugfx_draw_tri2(&mut self.draw.gfx, x1, y1, x2, y2, colour1, colour2, vertical);
    }

    /// `menugfx_draw_line` (menugfx.c:785).
    pub fn menugfx_draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32) {
        self.menugfx_draw_tri2(x1, y1, x2, y2, colour1, colour2, false);
    }

    /// `menugfx_draw_projected_line` (menugfx.c:793): `pd_core::menugfx`'s.
    pub fn menugfx_draw_projected_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32) {
        pd_core::menugfx::menugfx_draw_projected_line(&mut self.draw.gfx, &self.draw.text, x1, y1, x2, y2, colour1, colour2);
    }

    /// `menugfx_draw_shimmer` (menugfx.c:885): `pd_core::menugfx`'s.
    #[allow(clippy::too_many_arguments)]
    pub fn menugfx_draw_shimmer(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: u32, arg6: bool, arg7: i32, reverse: bool) {
        pd_core::menugfx::menugfx_draw_shimmer(&mut self.draw.gfx, self.frac20, x1, y1, x2, y2, colour, arg6, arg7, reverse);
    }

    /// `menugfx_draw_dialog_border_line` (menugfx.c:994).
    pub fn menugfx_draw_dialog_border_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32) {
        self.menugfx_draw_line(x1, y1, x2, y2, colour1, colour2);
        self.menugfx_draw_shimmer(x1, y1, x2, y2, colour1, false, 10, false);
    }

    /// `menugfx_draw_filled_rect` (menugfx.c:1002): `pd_core::menugfx`'s.
    pub fn menugfx_draw_filled_rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32) {
        pd_core::menugfx::menugfx_draw_filled_rect(&mut self.draw.gfx, &self.draw.text, self.frac20, x1, y1, x2, y2, colour1, colour2);
    }

    /// `menugfx_render_gradient` (menugfx.c:555).
    pub fn menugfx_render_gradient(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colourstart: u32, colourmid: u32, colourend: u32) {
        let ymid = (y1 + y2) / 2;
        let v = [
            self.uv(x1 * 10, y1 * 10, colourstart),
            self.uv(x2 * 10, y1 * 10, colourstart),
            self.uv(x2 * 10, y2 * 10, colourend),
            self.uv(x1 * 10, y2 * 10, colourend),
            self.uv(x1 * 10, ymid * 10, colourmid),
            self.uv(x2 * 10, ymid * 10, colourmid),
        ];
        let st = st_shade();
        // gSPTri4(0, 1, 5, 5, 4, 0, 2, 3, 4, 4, 5, 2)
        self.draw.gfx.tri([v[0], v[1], v[5]], &st);
        self.draw.gfx.tri([v[5], v[4], v[0]], &st);
        self.draw.gfx.tri([v[2], v[3], v[4]], &st);
        self.draw.gfx.tri([v[4], v[5], v[2]], &st);
    }

    /// `menugfx_render_dialog_background` (menugfx.c:241).
    #[allow(clippy::too_many_arguments)]
    pub fn menugfx_render_dialog_background(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, dialog: &MenuDialog, colour1: u32, _colour2: u32, _arg8: f32) {
        self.draw.gfx.fill_rect_scaled(x1, y1, x2, y2, colour1);
        let leftcolour = if dialog.transitionfrac < 0.0 {
            menu_colour(dialog.ty, Pal::DialogBorder1)
        } else {
            colour_blend(menu_colour(dialog.type2, Pal::DialogBorder1), menu_colour(dialog.ty, Pal::DialogBorder1), dialog.colourweight)
        };
        let rightcolour = if dialog.transitionfrac < 0.0 {
            menu_colour(dialog.ty, Pal::DialogBorder2)
        } else {
            colour_blend(menu_colour(dialog.type2, Pal::DialogBorder2), menu_colour(dialog.ty, Pal::DialogBorder2), dialog.colourweight)
        };
        // Right, left, bottom border
        self.menugfx_draw_dialog_border_line(x2 - 1, y1, x2, y2, rightcolour, rightcolour);
        self.menugfx_draw_dialog_border_line(x1, y1, x1 + 1, y2, leftcolour, leftcolour);
        self.menugfx_draw_dialog_border_line(x1, y2 - 1, x2, y2, leftcolour, rightcolour);
    }

    /// `menugfx_draw_dropdown_background` (menugfx.c:397).
    pub fn menugfx_draw_dropdown_background(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let ymid = (y1 + y2) / 2;
        let xmid = (x1 + x2) / 2;
        let colour1 = self.draw.text.get_colour_at_pos(xmid, ymid, 0xffffffff) & 0xff;
        let colour2 = (self.draw.text.get_colour_at_pos(xmid, ymid, 0xffffff7f) & 0xff) | 0x00006f00;
        let c = [colour1 | 0x00006f00, colour2, colour1 | 0x00003f00];
        let v = [
            self.uv(x1 * 10, y1 * 10, c[0]),
            self.uv(x2 * 10, y1 * 10, c[0]),
            self.uv(x1 * 10, ymid * 10, c[1]),
            self.uv(x2 * 10, ymid * 10, c[1]),
            self.uv(x1 * 10, y2 * 10, c[2]),
            self.uv(x2 * 10, y2 * 10, c[2]),
        ];
        let st = st_shade();
        // gSPTri4(0, 1, 3, 3, 2, 0, 2, 3, 4, 4, 3, 5)
        self.draw.gfx.tri([v[0], v[1], v[3]], &st);
        self.draw.gfx.tri([v[3], v[2], v[0]], &st);
        self.draw.gfx.tri([v[2], v[3], v[4]], &st);
        self.draw.gfx.tri([v[4], v[3], v[5]], &st);
    }

    /// `menugfx_draw_list_group_header` (menugfx.c:462, NTSC 1.0+).
    pub fn menugfx_draw_list_group_header(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, alpha: u8) {
        let alpha1 = alpha as u32;
        let alpha2 = alpha as u32;
        let ymid = (y1 + y2) / 2;
        let colours = [
            0x00006f00 | alpha1,
            0x00006f00 | alpha2,
            0x00003f00 | alpha2,
            0xffffff00,
            (0x00006f00 | alpha2) & 0xffffff00,
            (0x00003f00 | alpha1) & 0xffffff00,
            0x6f6f6f00 | alpha1,
        ];
        let v = [
            self.uv(x1 * 10, y1 * 10, colours[0]),
            self.uv(x2 * 10, y1 * 10, colours[6]),
            self.uv(x1 * 10, ymid * 10, colours[1]),
            self.uv(x2 * 10, ymid * 10, colours[1]),
            self.uv(x1 * 10, y2 * 10, colours[2]),
            self.uv(x2 * 10, y2 * 10, colours[2]),
            self.uv(x3 * 10, y1 * 10, colours[3]),
            self.uv(x3 * 10, ymid * 10, colours[4]),
            self.uv(x3 * 10, y2 * 10, colours[5]),
        ];
        let st = st_shade();
        for [a, b, c] in [[0, 1, 3], [3, 2, 0], [2, 3, 4], [4, 3, 5], [1, 6, 7], [7, 3, 1], [3, 7, 8], [8, 5, 3]] {
            self.draw.gfx.tri([v[a], v[b], v[c]], &st);
        }
        self.menugfx_draw_shimmer(x1, y1, x2, y1 + 1, (alpha1 & 0xff) >> 2, true, 0x28, false);
        self.menugfx_draw_shimmer(x1, y2, x2, y2 + 1, (alpha1 & 0xff) >> 2, false, 0x28, true);
    }

    /// `menugfx_render_slider` (menugfx.c:630).
    pub fn menugfx_render_slider(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, markerx: i32, colour: u32) {
        let colours = [(colour & 0xffffff00) | 0x4f, 0xffffffff, 0x0000ff4f];
        let st = st_shade();
        // Background triangle
        let b = [self.uv(x1 * 10, y2 * 10, colours[2]), self.uv(x2 * 10 - 40, y1 * 10, colours[2]), self.uv(x2 * 10, y2 * 10, colours[2])];
        self.draw.gfx.tri(b, &st);
        // Marker triangle
        let m = [self.uv(markerx * 10 - 40, y2 * 10 - 80, colours[0]), self.uv(markerx * 10 + 40, y2 * 10 - 80, colours[0]), self.uv(markerx * 10, y2 * 10 + 10, colours[1])];
        self.draw.gfx.tri(m, &st);
        // Line to the left of the marker: blue -> white gradient
        self.menugfx_draw_line(x1, y2, markerx, y2 + 1, 0x0000ffff, 0xffffffff);
        // Line to the right of the marker: solid blue
        self.menugfx_draw_line(markerx, y2, x2, y2 + 1, 0x0000ffff, 0x0000ffff);
    }

    /// `menugfx_draw_carousel_chevron` (menugfx.c:1019).
    pub fn menugfx_draw_carousel_chevron(&mut self, x: i32, y: i32, size: i32, direction: i32, colour1: u32, colour2: u32) {
        let size = size * 10;
        let (mut relx, mut rely, mut halfwidth, mut halfheight) = (0, 0, 0, 0);
        match direction {
            0 => {
                rely = -size;
                halfwidth = -size / 2;
            }
            1 => {
                relx = size;
                halfheight = -size / 2;
            }
            2 => {
                rely = size;
                halfwidth = size / 2;
            }
            _ => {
                relx = -size;
                halfheight = size / 2;
            }
        }
        let v = [
            self.uv(x * 10, y * 10, colour1),
            self.uv(x * 10 + relx + halfwidth, y * 10 + rely + halfheight, colour2),
            self.uv(x * 10 + relx - halfwidth, y * 10 + rely - halfheight, colour2),
        ];
        self.draw.gfx.tri(v, &st_shade());
    }

    /// `menugfx_draw_dialog_chevron` (menugfx.c:1102).
    #[allow(clippy::too_many_arguments)]
    pub fn menugfx_draw_dialog_chevron(&mut self, x: i32, y: i32, size: i32, direction: i32, colour1: u32, colour2: u32, arg7: f32) {
        let size = size * 10;
        let (mut relx, mut rely, mut halfwidth, mut halfheight) = (0i32, 0i32, 0i32, 0i32);
        let grow = ((size as f32 * arg7 * 0.5) as i32 + size) / 2;
        match direction {
            0 => {
                rely = -size;
                halfwidth = -grow;
            }
            1 => {
                relx = size;
                halfheight = -grow;
            }
            2 => {
                rely = size;
                halfwidth = grow;
            }
            _ => {
                relx = -size;
                halfheight = grow;
            }
        }
        let v0 = self.uv(x * 10, y * 10, colour1);
        let v1 = self.uv(x * 10 + relx + halfwidth, y * 10 + rely + halfheight, colour2);
        let v2 = self.uv(x * 10 + relx - halfwidth, y * 10 + rely - halfheight, colour2);
        let relx2 = (relx as f32 / 3.0 + (relx as f32 * 1.5 * (1.0 - arg7)) / 3.0) as i16 as i32;
        let rely2 = (rely as f32 / 3.0 + (rely as f32 * 1.5 * (1.0 - arg7)) / 3.0) as i16 as i32;
        let v3 = self.uv(x * 10 + relx2, y * 10 + rely2, colour2);
        let st = st_shade();
        self.draw.gfx.tri([v0, v1, v3], &st);
        self.draw.gfx.tri([v3, v2, v0], &st);
    }

    /// `menugfx_draw_checkbox` (menugfx.c:1187).
    pub fn menugfx_draw_checkbox(&mut self, x: i32, y: i32, size: i32, fill: bool, bordercolour: u32, fillcolour: u32) {
        if fill {
            self.draw.gfx.fill_rect_scaled(x, y, x + size, y + size, fillcolour);
        }
        self.draw.gfx.fill_rect_scaled(x, y, x + size + 1, y + 1, bordercolour);
        self.draw.gfx.fill_rect_scaled(x, y + size, x + size + 1, y + size + 1, bordercolour);
        self.draw.gfx.fill_rect_scaled(x, y + 1, x + 1, y + size, bordercolour);
        self.draw.gfx.fill_rect_scaled(x + size, y + 1, x + size + 1, y + size, bordercolour);
    }

    /// `menugfx_render_bg_blur` (menugfx.c:118): the 40×30 blurred screenshot
    /// stretched over the screen, `colour` its shade (white + alpha).
    pub fn menugfx_render_bg_blur(&mut self, colour: u32, arg2: i16, arg3: i16) {
        let Some(tex) = self.draw.res.blur.as_ref() else { return };
        let c = rgba(colour);
        let mk = |g: &n64::rdp::Gfx, x10: i32, y10: i32, s: f32, t: f32| {
            let (x, y) = g.ortho(x10 as f32, y10 as f32);
            SV { x, y, z: 0.0, inv_w: 1.0, s, t, c }
        };
        let (a2, a3) = (arg2 as i32, arg3 as i32);
        let v = [
            mk(&self.draw.gfx, a2, a3, 0.0, 0.0),
            mk(&self.draw.gfx, a2 + 320 * 10 + 40, a3, 1280.0 / 32.0, 0.0),
            mk(&self.draw.gfx, a2 + 320 * 10 + 40, a3 + 240 * 10 + 50, 1280.0 / 32.0, 960.0 / 32.0),
            mk(&self.draw.gfx, a2, a3 + 240 * 10 + 50, 0.0, 960.0 / 32.0),
        ];
        let st = TriState { cc: Cc::ModulateI, tex: Some(tex), filter: Filter::Bilerp, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false };
        self.draw.gfx.quad(v, &st);
    }

    /// `ortho_draw_holoray` (savebuffer.c:189) drawn now (onto `gdl`, not the
    /// text holoray list): a plane from the edge (x1,y1)-(x2,y2) back into the
    /// screen, textured with `TEX_GENERAL_MENURAY0`, perspective-correct.
    #[allow(clippy::too_many_arguments)]
    pub fn ortho_draw_holoray(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour1: u32, colour2: u32, ty: i32, miny: i32, maxy: i32, fromx: i32, fromy: i32) {
        let (mut y1, mut y2) = (y1, y2);
        if y1 < miny && y2 < miny {
            return;
        }
        if y1 > maxy && y2 > maxy {
            return;
        }
        y1 = y1.clamp(miny, maxy.max(miny));
        y2 = y2.clamp(miny, maxy.max(miny));
        let txmul = 20;
        let mut sp34 = 1.0f32;
        let mut sp30 = 1.0f32;
        let mut sp2e = ((x1 + y1) * txmul) as i16 as i32;
        let mut sp2c = ((x2 + y2) * txmul) as i16 as i32;
        let mut sp2a = 0;
        let mut sp28 = 16384;
        let scale = 10.0f32;
        if ty == MENUPLANE_01 {
            sp30 = 2.0;
        }
        let mut a1: i32 = 200;
        if ty == MENUPLANE_02 || ty == MENUPLANE_03 {
            if ty == MENUPLANE_02 {
                sp2e = 0;
                sp2c = 1024;
            } else {
                sp2e = 1024;
                sp2c = 2048;
            }
            sp34 = 4.0;
            sp30 = 4.0;
            a1 = 6000;
        }
        if ty == MENUPLANE_08 || ty == MENUPLANE_09 || ty == MENUPLANE_11 {
            sp2e = 0;
            sp2c = 2048;
            a1 = 2000;
            sp34 = 4.0;
            sp30 = 4.0;
            if ty == MENUPLANE_09 {
                sp30 = 2.0;
            }
        }
        if ty == MENUPLANE_04 {
            a1 = 2000;
            sp34 = 1.0;
            sp30 = 1.0;
        }
        if ty == MENUPLANE_05 || ty == MENUPLANE_06 || ty == MENUPLANE_10 {
            a1 = 1000;
            sp2e = 0;
            sp2c = 4096;
            sp30 = 4.0;
            if ty == MENUPLANE_06 || ty == MENUPLANE_10 {
                sp2e = 384;
                sp2c = 4480;
                sp30 = 8.0;
            } else {
                sp34 = 2.0;
            }
        }
        if ty == MENUPLANE_07 {
            a1 = -5;
            sp30 = 8.0;
            sp2a = 256;
            sp28 = 0;
        }
        let tmp1 = (fromx as f32 * a1 as f32 / scale) as i16 as i32;
        let tmp2 = (fromy as f32 * a1 as f32 / scale) as i16 as i32;
        let f = self.frac20;
        let t1 = if ty == MENUPLANE_10 { (f * sp34 * 64.0 * 32.0) as i16 as i32 } else { ((f - 0.5) * sp34 * 64.0 * 32.0) as i16 as i32 };
        let a1_2 = if ty == MENUPLANE_10 { ((f - 0.5) * sp30 * 64.0 * 32.0) as i16 as i32 } else { (f * sp30 * 64.0 * 32.0) as i16 as i32 };
        let pos = [
            (x1, y1, -10),
            (x2, y2, -10),
            (x1 + tmp1, y1 + tmp2, -10 - a1),
            (x2 + tmp1, y2 + tmp2, -10 - a1),
        ];
        // s/t are s10.5 and wrap as 16-bit.
        let st = [
            ((sp2e + t1) as i16, (sp2a + a1_2) as i16),
            ((sp2c + t1) as i16, (sp2a + a1_2) as i16),
            ((sp2e + t1) as i16, (sp28 + a1_2) as i16),
            ((sp2c + t1) as i16, (sp28 + a1_2) as i16),
        ];
        let cols = if ty == MENUPLANE_07 { [colour1, colour1, colour2, colour2] } else { [colour1, colour2, colour1, colour2] };
        let mut v = [SV::default(); 4];
        for i in 0..4 {
            let (x, y, z) = pos[i];
            let (sx, sy, iw) = self.draw.gfx.holoray(x as f32, y as f32, z as f32);
            v[i] = SV { x: sx, y: sy, z: 0.0, inv_w: iw, s: st[i].0 as f32 / 32.0, t: st[i].1 as f32 / 32.0, c: rgba(cols[i]) };
        }
        let tex = &self.draw.res.menuray0;
        let ts = TriState { cc: Cc::ModulateIA, tex: Some(tex), filter: Filter::Bilerp, blend: Blend::Xlu, env: [1.0; 4], persp: true, zbuf: false, cull_back: false };
        // gSPTri2(0, 1, 3, 3, 2, 0)
        self.draw.gfx.tri([v[0], v[1], v[3]], &ts);
        self.draw.gfx.tri([v[3], v[2], v[0]], &ts);
    }

    /// `menugfx_render_bg_cone` (menugfx.c:1318): two cones of eight rays
    /// rotating in opposite directions, their green channel pulsing.
    pub fn menugfx_render_bg_cone(&mut self) {
        let deg = std::f32::consts::PI / 180.0;
        // g_HolorayProjectFrom*: the hudpiece's eye when it's up (menu_render).
        let (fromx, fromy) = (self.draw.text.holoray_fromx, self.draw.text.holoray_fromy);
        let baseangle = 360.0 * deg * self.frac20 * 2.0;
        let colourupper = ((sin_osc(self.frac20, 1.0) * 255.0) as u32) << 16;
        for i in 0..8 {
            let angle = baseangle + i as f32 * 2.0 * 180.0 * deg * 0.125;
            let x1 = (600.0 * angle.sin()) as i32 + 160;
            let y1 = (600.0 * angle.cos()) as i32 + 120;
            let x2 = (600.0 * (angle + 45.0 * deg).sin()) as i32 + 160;
            let y2 = (600.0 * (angle + 45.0 * deg).cos()) as i32 + 120;
            let colour = colourupper | 0xff00007f;
            self.ortho_draw_holoray(x1, y1, x2, y2, colour, colour, MENUPLANE_08, -100000, 100000, fromx, fromy);
        }
        let colourupper = ((255.0 - cos_osc(self.frac20, 1.0) * 255.0) as u32) << 16;
        let baseangle = 360.0 * deg * self.frac20;
        for i in 0..8 {
            let angle = -baseangle + 2.0 * i as f32 * 180.0 * deg * 0.125;
            let x1 = (600.0 * angle.sin()) as i32 + 160;
            let y1 = (600.0 * angle.cos()) as i32 + 120;
            let x2 = (600.0 * (angle + 45.0 * deg).sin()) as i32 + 160;
            let y2 = (600.0 * (angle + 45.0 * deg).cos()) as i32 + 120;
            let colour = colourupper | 0xff00007f;
            self.ortho_draw_holoray(x1, y1, x2, y2, colour, colour, MENUPLANE_09, -100000, 100000, fromx, fromy);
        }
    }

    /// Run the text-holoray list (`g_TextHoloRayGdl`) now.
    pub fn flush_text_holorays(&mut self) {
        let reqs = std::mem::take(&mut self.draw.text.holorays);
        for r in reqs {
            self.ortho_draw_holoray(r.x1, r.y1, r.x2, r.y2, r.colour1, r.colour2, r.plane, r.miny, r.maxy, r.fromx, r.fromy);
        }
    }
}
