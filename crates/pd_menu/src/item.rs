//! `menuitem.c`: each item type's render / tick / init / overlay.
//!
//! Item data blocks (`union menuitemdata *data`) are indices into the current
//! player's `menu.blocks`; the item's dialog is an index into `menu.dialogs`.
//! Ranking, controller and objectives items are not ported (the Combat
//! Simulator setup never shows them).

use pd_core::lang::tx;
use super::menu::Ctx;
use super::gfx::{mixcolour, sin_osc, wave1, wave2, Pal};
use pd_core::text::{colour_blend, measure, wrap, FontId};
use super::types::*;
use super::{generated as gd, MenuSystem};

/// `g_KeyboardKeys` (menuitem.c:40).
const KEYBOARD_KEYS: [[u8; 10]; 5] = [
    *b"0123456789",
    *b"ABCDEFGHIJ",
    *b"KLMNOPQRST",
    *b"UVWXYZ ?!.",
    *b"1212123123",
];

fn dim(colour: u32, dimmed: bool) -> u32 {
    if dimmed {
        (colour_blend(colour, 0, 127) & 0xffffff00) | (colour & 0xff)
    } else {
        colour
    }
}

impl MenuSystem {
    fn mix(&self, di: usize, p: Pal) -> u32 {
        mixcolour(&self.mr().dialogs[di], p)
    }
    fn dty(&self, di: usize) -> u8 {
        self.mr().dialogs[di].ty
    }
    fn dimmed(&self, di: usize) -> bool {
        self.mr().dialogs[di].dimmed
    }
    fn waves(&mut self, di: usize, p: Pal) {
        let t = self.dty(di);
        self.draw.text.set_wave_colours(wave2(t, p), wave1(t, p));
    }
    fn measure_f(&self, text: &str, f: FontId) -> (i32, i32) {
        measure(self.draw.res.fonts.get(f), text, 0)
    }
    fn blk(&mut self, b: usize) -> &mut ItemData {
        &mut self.m().blocks[b]
    }

    // ---- list (menuitem.c:62-793) ----

    /// `menuitem0f0e5d2c` (menuitem.c:62): the first option on screen for a scroll offset.
    fn menuitem_list_first_option(&mut self, arg0: i32, item: &'static MenuItem) -> i32 {
        let Some(h) = item.fn_handler() else { return 0 };
        let lh = self.line_height;
        let mut hd = HandlerData::default();
        h(self, MENUOP_GET_OPTGROUP_COUNT, item, &mut hd);
        let mut s1;
        if hd.value == 0 {
            s1 = arg0 / lh;
        } else {
            let numgroups = hd.value;
            let mut s0 = arg0;
            s1 = 0;
            hd.value = 0;
            hd.unk04 = 0;
            loop {
                let a0 = if hd.value < numgroups {
                    h(self, MENUOP_GET_OPTGROUP_START_INDEX, item, &mut hd);
                    hd.groupstartindex
                } else {
                    9999
                };
                hd.value += 1;
                if s1 + s0 / lh >= a0 {
                    s0 = s0 - (a0 - s1) * lh - LINEHEIGHT;
                    s1 += a0 - s1;
                } else {
                    s1 += s0 / lh;
                    break;
                }
            }
        }
        s1.max(0)
    }

    /// `menuitem_list_get_offset_y` (menuitem.c:117).
    fn menuitem_list_get_offset_y(&mut self, optionindex: i32, item: &'static MenuItem) -> i32 {
        let Some(h) = item.fn_handler() else { return 0 };
        let lh = self.line_height;
        let optionindex = optionindex.max(0);
        let mut hd = HandlerData::default();
        h(self, MENUOP_GET_OPTGROUP_COUNT, item, &mut hd);
        if hd.value == 0 {
            return optionindex * lh;
        }
        let numgroups = hd.value;
        hd.unk04 = 0;
        let mut numlines = 0;
        hd.value = 0;
        while hd.value < numgroups {
            h(self, MENUOP_GET_OPTGROUP_START_INDEX, item, &mut hd);
            if optionindex >= hd.groupstartindex {
                numlines += 1;
            } else {
                break;
            }
            hd.value += 1;
        }
        optionindex * lh + numlines * LINEHEIGHT
    }

    /// `menuitem_list_render_header` (menuitem.c:159).
    #[allow(clippy::too_many_arguments)]
    fn menuitem_list_render_header(&mut self, x1: i32, y1: i32, width: i32, arg4: i32, height: i32, text: &str, di: usize) {
        let dimmed = self.dimmed(di);
        let mut colour = self.mix(di, Pal::ListgroupHeaderbg);
        if dimmed {
            colour = (colour_blend(colour, 0, 0x2c) & 0xffffff00) | (colour & 0xff);
        }
        self.menugfx_draw_list_group_header(x1, y1, x1 + width, y1 + height, x1 + arg4, (colour & 0xff) as u8);
        let (mut x, mut y) = (x1 + 3, y1 + 2);
        let mut colour = self.mix(di, Pal::ListgroupHeaderfg);
        if dimmed {
            colour = (colour_blend(colour, 0, 0x2c) & 0xffffff00) | (colour & 0xff);
        }
        self.waves(di, Pal::ListgroupHeaderfg);
        self.tc().render_v2(&mut x, &mut y, text, FontId::Sm, colour, width, height, 0, 0);
    }

    fn set_scissor_clamped(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let us = self.draw.gfx.uiscale;
        let (x1, x2) = (x1 * us, x2 * us);
        let x2 = x2.max(x1);
        let y2 = y2.max(y1);
        self.draw.gfx.set_scissor(x1.max(0), y1.max(0), x2.max(0), y2.max(0));
    }

    /// `menuitem_list_render` (menuitem.c:201).
    fn menuitem_list_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let Some(b) = ctx.data else { return };
        let Some(h) = item.fn_handler() else { return };
        if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
            let mut hd = HandlerData::default();
            h(self, MENUOP_GET_OPTION_HEIGHT, item, &mut hd);
            self.line_height = hd.value;
        } else {
            self.line_height = LINEHEIGHT;
        }
        let lh = self.line_height;
        let mut width = self.mr().dialogs[di].width;
        if item.flags & MENUITEMFLAG_LIST_AUTOWIDTH != 0 {
            width = ctx.width;
        }
        self.set_scissor_clamped(ctx.x, ctx.y, ctx.x + width, ctx.y + ctx.height);
        let mut halfheight = ctx.height / 2;
        halfheight /= lh;
        halfheight *= lh;
        self.blk(b).viewheight = ctx.height as i16;
        if item.ty == MENUITEMTYPE_DROPDOWN || item.ty == MENUITEMTYPE_PLAYERSTATS {
            self.menugfx_draw_dropdown_background(ctx.x, ctx.y, ctx.x + ctx.width, ctx.y + ctx.height);
            self.menugfx_draw_shimmer(ctx.x, ctx.y, ctx.x + 1, ctx.y + ctx.height, 0x7f, true, 15, true);
            self.menugfx_draw_shimmer(ctx.x + ctx.width, ctx.y, ctx.x + ctx.width + 1, ctx.y + ctx.height, 0x7f, false, 15, true);
            self.menugfx_draw_shimmer(ctx.x, ctx.y, ctx.x + ctx.width, ctx.y + 1, 0x7f, false, 15, true);
            self.menugfx_draw_shimmer(ctx.x, ctx.y + ctx.height, ctx.x + ctx.width, ctx.y + ctx.height + 1, 0x7f, false, 15, false);
        }
        let left = ctx.x + 2;
        let mut sp15c = HandlerData::default();
        h(self, MENUOP_GET_SELECTED_INDEX, item, &mut sp15c);
        let mut selectedindex = sp15c.value as u32 as i64;
        if selectedindex >= 0x10000 {
            selectedindex = -1;
        }
        let mut sp104 = ctx.y + 1;
        h(self, MENUOP_GET_OPTION_COUNT, item, &mut sp15c);
        let numoptions = sp15c.value;
        let curoffsety = self.blk(b).curoffsety as i32;
        let firstonscreen = self.menuitem_list_first_option(curoffsety - halfheight, item);
        let mut optionindex = firstonscreen;
        sp15c.unk04 = 0;
        let mut s4 = self.menuitem_list_get_offset_y(optionindex, item) + halfheight - curoffsety;
        let mut sp14c = HandlerData::default();
        h(self, MENUOP_GET_OPTGROUP_COUNT, item, &mut sp14c);
        let numgroups = sp14c.value;
        let mut nextgroupstartindex = 9999;
        let mut donestickyheader = false;
        let mut sp13c = HandlerData::default();
        let vw = self.draw.gfx.w as i32;
        let vh = self.draw.gfx.h as i32;
        let dimmed = self.dimmed(di);
        if numoptions > 0 {
            if numgroups != 0 {
                let mut spc8 = 0;
                nextgroupstartindex = 0;
                sp14c.value = 0;
                sp14c.unk04 = 0;
                while sp14c.value < numgroups {
                    h(self, MENUOP_GET_OPTGROUP_START_INDEX, item, &mut sp14c);
                    let tmp = sp14c.groupstartindex;
                    if tmp <= firstonscreen {
                        spc8 = sp14c.value;
                        nextgroupstartindex = tmp;
                        sp14c.value += 1;
                    } else {
                        break;
                    }
                }
                sp13c.value = spc8;
                sp13c.unk04 = 0;
                sp13c.unk0c = sp14c.unk0c;
                if nextgroupstartindex < firstonscreen || s4 < LINEHEIGHT {
                    let text = h(self, MENUOP_GET_OPTGROUP_TEXT, item, &mut sp13c).text();
                    if s4 + lh > 0 {
                        self.menuitem_list_render_header(ctx.x, ctx.y, ctx.width, width, LINEHEIGHT, &text, di);
                        donestickyheader = true;
                    }
                    sp104 += LINEHEIGHT;
                    sp13c.value += 1;
                    if sp14c.value < numgroups {
                        h(self, MENUOP_GET_OPTGROUP_START_INDEX, item, &mut sp14c);
                        nextgroupstartindex = sp14c.groupstartindex;
                        sp14c.value += 1;
                    } else {
                        nextgroupstartindex = 9999;
                    }
                }
            }
            if firstonscreen == nextgroupstartindex {
                s4 -= LINEHEIGHT;
            }
            let mut done2 = false;
            while !done2 {
                let mut colour = self.mix(di, Pal::ItemUnfocused);
                if dimmed {
                    colour = (colour_blend(colour, 0, 127) & 0xffffff00) | (colour & 0xff);
                }
                self.waves(di, Pal::ItemUnfocused);
                if optionindex == nextgroupstartindex {
                    if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
                        self.menu_apply_scissor();
                    }
                    let title = h(self, MENUOP_GET_OPTGROUP_TEXT, item, &mut sp13c).text();
                    sp13c.value += 1;
                    let height = (ctx.height - s4).min(LINEHEIGHT);
                    self.menuitem_list_render_header(ctx.x, ctx.y + s4, ctx.width, width, height, &title, di);
                    if sp14c.value < numgroups {
                        h(self, MENUOP_GET_OPTGROUP_START_INDEX, item, &mut sp14c);
                        nextgroupstartindex = sp14c.groupstartindex;
                        sp14c.value += 1;
                    } else {
                        nextgroupstartindex = 9999;
                    }
                    s4 += LINEHEIGHT;
                } else {
                    if optionindex < numoptions {
                        let mut spb4 = false;
                        if selectedindex as i32 == optionindex && selectedindex >= 0 {
                            colour |= 0xffffff00;
                        }
                        let index = self.blk(b).index as i32;
                        if optionindex == index && ctx.focused != 0 {
                            let weight = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
                            let spb0 = self.mix(di, Pal::ItemFocusedInner);
                            colour = colour_blend(colour, colour & 0xff, 127);
                            colour = colour_blend(colour, spb0, weight);
                            let d = self.mr().dialogs[di];
                            if (!(d.transitionfrac >= 0.0) || d.type2 != 0) && (!(d.transitionfrac < 0.0) || d.ty != 0) {
                                self.draw.text.shadow_enabled = true;
                                spb4 = true;
                            }
                        }
                        if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
                            let rd = RenderData { x: ctx.x, y: ctx.y + s4, width: ctx.width, colour, unk10: optionindex == index };
                            let mut top = rd.y;
                            let mut bottom = rd.y + lh;
                            if top < ctx.y {
                                top = ctx.y;
                            }
                            if bottom > ctx.y + ctx.height - 1 {
                                bottom = ctx.y + ctx.height - 1;
                            }
                            if donestickyheader && top < ctx.y + LINEHEIGHT {
                                top = ctx.y + LINEHEIGHT;
                            }
                            let l = rd.x.max(0);
                            let r = (rd.x + rd.width).max(0);
                            self.set_scissor_clamped(l, top, r, bottom);
                            let mut spb8 = HandlerData { unk04: optionindex, render: Some(rd), unk0c: sp15c.unk04, ..HandlerData::default() };
                            h(self, MENUOP_RENDER, item, &mut spb8);
                            sp15c.unk04 = spb8.unk0c;
                        } else {
                            sp15c.value = optionindex;
                            let text2 = h(self, MENUOP_GET_OPTION_TEXT, item, &mut sp15c).text();
                            let mut sp128 = 0;
                            let mut y = ctx.y + s4 + 1;
                            let mut x = if item.ty == MENUITEMTYPE_DROPDOWN || item.ty == MENUITEMTYPE_PLAYERSTATS { left } else { left + 8 };
                            if y < sp104 {
                                sp128 = y - sp104;
                                y = sp104;
                            }
                            let height = (ctx.y + ctx.height - y).max(0);
                            self.tc().render_v2(&mut x, &mut y, &text2, FontId::Sm, colour, ctx.width - left + ctx.x, height, sp128, 0);
                            let mut spb8 = HandlerData { value: optionindex, unk04: 255, ..HandlerData::default() };
                            h(self, MENUOP_IS_OPTION_CHECKED, item, &mut spb8);
                            if spb8.unk04 != 255 {
                                self.menugfx_draw_checkbox(left, ctx.y + s4 + 1, 6, spb8.unk04 != 0, colour, 0xff00007f);
                            }
                        }
                        if spb4 {
                            self.draw.text.shadow_enabled = false;
                        }
                    }
                    optionindex += 1;
                    s4 += lh;
                    if optionindex >= numoptions {
                        done2 = true;
                    }
                }
                if ctx.height < s4 {
                    done2 = true;
                }
            }
            self.menu_apply_scissor();
        } else {
            let colour = dim(self.mix(di, Pal::ItemUnfocused), dimmed);
            let (mut x, mut y) = (left + 8, ctx.y + ctx.height / 2);
            let empty = self.lang(tx(gd::B_OPTIONS, 313));
            self.tc().render_v2(&mut x, &mut y, &empty, FontId::Sm, colour, ctx.width - left + ctx.x, vh, 0, 0);
        }
        let _ = vw;
    }

    /// `menuitem_list_tick` (menuitem.c:661).
    fn menuitem_list_tick(&mut self, item: &'static MenuItem, inputs: &mut MenuInputs, tickflags: u32, b: usize) -> bool {
        let Some(h) = item.fn_handler() else { return true };
        if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
            let mut hd = HandlerData::default();
            h(self, MENUOP_GET_OPTION_HEIGHT, item, &mut hd);
            self.line_height = hd.value;
        } else {
            self.line_height = LINEHEIGHT;
        }
        let lh = self.line_height;
        let mut hd = HandlerData::default();
        if item.ty == MENUITEMTYPE_DROPDOWN || item.ty == MENUITEMTYPE_PLAYERSTATS {
            let mut min = (self.blk(b).viewheight / 2) as i32;
            min /= lh;
            min *= lh;
            let idx = self.blk(b).index as i32;
            let mut target = self.menuitem_list_get_offset_y(idx, item);
            if target < min {
                target = min;
            }
            h(self, MENUOP_GET_OPTION_COUNT, item, &mut hd);
            let max = hd.value * lh - self.blk(b).viewheight as i32 + min;
            if target > max {
                target = max;
            }
            self.blk(b).targetoffsety = target as i16;
        }
        let diffframe60 = self.lv.diffframe60;
        {
            let d = self.blk(b);
            if d.curoffsety != d.targetoffsety {
                let mut f0 = d.curoffsety as f32;
                let prev = d.curoffsety;
                for _ in 0..diffframe60 {
                    f0 = d.targetoffsety as f32 * 0.35 + 0.65 * f0;
                }
                d.curoffsety = f0 as i16;
                if d.curoffsety != d.targetoffsety && prev == d.curoffsety {
                    if d.curoffsety < d.targetoffsety {
                        d.curoffsety += 1;
                    } else {
                        d.curoffsety -= 1;
                    }
                }
            }
        }
        if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 {
            h(self, MENUOP_GET_OPTION_COUNT, item, &mut hd);
            if hd.value != 0 {
                let last = hd.value - 1;
                let count = hd.value;
                if self.blk(b).index as i32 > last {
                    self.blk(b).index = last as i16;
                    let t = self.menuitem_list_get_offset_y(last, item);
                    self.blk(b).targetoffsety = t as i16;
                }
                if inputs.updown != 0 {
                    let prev2 = self.blk(b).index;
                    let mut idx = self.blk(b).index as i32 + inputs.updown as i32;
                    if idx < 0 {
                        idx = count - 1;
                    }
                    if idx >= count {
                        idx = 0;
                    }
                    self.blk(b).index = idx as i16;
                    let t = self.menuitem_list_get_offset_y(idx, item);
                    self.blk(b).targetoffsety = t as i16;
                    if prev2 != idx as i16 {
                        let mut hd2 = HandlerData { value: idx, ..HandlerData::default() };
                        h(self, MENUOP_ON_OPTION_FOCUS, item, &mut hd2);
                        self.menu_play_sound(MENUSOUND_SUBFOCUS);
                    }
                }
                if inputs.select != 0 {
                    let mut hd2 = HandlerData { value: self.blk(b).index as i32, unk04: inputs.start as i32, ..HandlerData::default() };
                    h(self, MENUOP_CONFIRM, item, &mut hd2);
                    self.menu_play_sound(MENUSOUND_SELECT);
                    if hd2.unk04 == 2 {
                        inputs.start = false;
                    }
                }
                inputs.updown = 0;
            }
        }
        // The confirm above may have closed this item's dialog.
        if self.blk(b).index < 0 {
            return true;
        }
        let tmp = self.blk(b).index as i32;
        let mut hd3 = HandlerData { value: tmp, unk04: 1, unk0c: tmp, groupstartindex: (tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0) as i32, ..HandlerData::default() };
        h(self, MENUOP_GET_OPTION_INDEX2, item, &mut hd3);
        if hd3.unk0c != hd3.value {
            self.blk(b).index = hd3.value as i16;
            let t = self.menuitem_list_get_offset_y(hd3.value, item);
            self.blk(b).targetoffsety = t as i16;
        }
        true
    }

    /// `menuitem_dropdown_init` (menuitem.c:794).
    fn menuitem_dropdown_init(&mut self, item: &'static MenuItem, b: usize) {
        {
            let d = self.blk(b);
            d.curoffsety = 0;
            d.index = 0;
        }
        let Some(h) = item.fn_handler() else { return };
        if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
            let mut hd = HandlerData::default();
            h(self, MENUOP_GET_OPTION_HEIGHT, item, &mut hd);
            self.line_height = hd.value;
        } else {
            self.line_height = LINEHEIGHT;
        }
        let mut hd = HandlerData::default();
        h(self, MENUOP_GET_SELECTED_INDEX, item, &mut hd);
        if (hd.value as u32) < 0xffff {
            self.blk(b).index = hd.value as u16 as i16;
        } else {
            hd.value = 0;
            hd.unk04 = 0;
            h(self, MENUOP_GET_OPTION_INDEX2, item, &mut hd);
            self.blk(b).index = hd.value as i16;
        }
        let idx = self.blk(b).index as i32;
        let t = self.menuitem_list_get_offset_y(idx, item);
        self.blk(b).targetoffsety = t as i16;
        h(self, MENUOP_ON_OPTION_FOCUS, item, &mut hd);
    }

    /// `menuitem_dropdown_render` (menuitem.c:833).
    fn menuitem_dropdown_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let text = self.menu_resolve_param2_text(item).unwrap_or_default();
        let dimmed = self.dimmed(di);
        let mut colour = dim(self.mix(di, Pal::ItemUnfocused), dimmed);
        if ctx.focused != 0 {
            let freq = if ctx.focused & 2 != 0 { 20.0 } else { 40.0 };
            let weight = (sin_osc(self.frac20, freq) * 255.0) as u32;
            let tmp = self.mix(di, Pal::ItemFocusedInner);
            colour = colour_blend(colour, colour & 0xff, 0x7f);
            colour = colour_blend(colour, tmp, weight);
            self.waves(di, Pal::ItemFocusedInner);
        } else {
            self.waves(di, Pal::ItemUnfocused);
        }
        if self.menu_is_item_disabled(item, di) {
            colour = dim(self.mix(di, Pal::ItemDisabled), dimmed);
            self.waves(di, Pal::ItemDisabled);
        }
        let (mut x, mut y) = (ctx.x + 10, ctx.y + 2);
        self.tc().render_v2(&mut x, &mut y, &text, FontId::Sm, colour, ctx.width, ctx.height, 0, 0);
        let mut y = ctx.y + 2;
        if let Some(h) = item.fn_handler() {
            let mut hd = HandlerData::default();
            h(self, MENUOP_GET_SELECTED_INDEX, item, &mut hd);
            hd.unk04 = 0;
            let text = h(self, MENUOP_GET_OPTION_TEXT, item, &mut hd).text();
            let tw = self.measure_f(&text, FontId::Sm).1;
            let mut x = ctx.x + ctx.width - tw - 10;
            self.tc().render_v2(&mut x, &mut y, &text, FontId::Sm, colour, ctx.width, ctx.height, 0, 0);
        }
    }

    /// `menuitem_dropdown_tick` (menuitem.c:922).
    fn menuitem_dropdown_tick(&mut self, item: &'static MenuItem, di: usize, inputs: &mut MenuInputs, tickflags: u32, b: usize) -> bool {
        if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 && item.fn_handler().is_some() {
            let d = self.mr().dialogs[di];
            let focused = d.focuseditem.map(|f| std::ptr::eq(&d.def().items[f], item)).unwrap_or(false);
            if d.dimmed && focused {
                self.menuitem_list_tick(item, inputs, tickflags, b);
                if self.mp_is_player_locked_out(self.mpplayernum as i32) && (item.flags & MENUITEMFLAG_LOCKABLEMAJOR != 0 || d.def().flags & MENUDIALOGFLAG_MPLOCKABLE != 0) {
                    self.dlg(di).dimmed = false;
                }
            }
            if inputs.back != 0 && self.dimmed(di) {
                self.dlg(di).dimmed = false;
                inputs.back = 0;
                self.menu_play_sound(MENUSOUND_TOGGLEOFF);
            }
            if inputs.select != 0 {
                if self.dimmed(di) {
                    self.dlg(di).dimmed = false;
                } else {
                    self.dlg(di).dimmed = true;
                    self.menuitem_dropdown_init(item, b);
                    let mut hd = HandlerData::default();
                    item.fn_handler().unwrap()(self, MENUOP_GET_SELECTED_INDEX, item, &mut hd);
                    let lh = self.line_height;
                    self.blk(b).unk0e = (hd.value as u32).wrapping_mul(lh as u32) as u16;
                    self.menu_play_sound(MENUSOUND_TOGGLEOFF);
                }
            }
        }
        true
    }

    /// `menuitem_dropdown_overlay` (menuitem.c:963).
    #[allow(clippy::too_many_arguments)]
    fn menuitem_dropdown_overlay(&mut self, x: i32, y: i32, x2: i32, _y2: i32, item: &'static MenuItem, di: usize, data: Option<usize>) {
        let d = self.mr().dialogs[di];
        let mut cx = if d.unk6e != 0 {
            x + 78
        } else if x2 < 0 {
            x + 2
        } else {
            x + x2 - 62
        };
        if item.flags & MENUITEMFLAG_DROPDOWN_BELOW != 0 {
            cx = x + 30;
        }
        let mut cy = y + 1;
        let focused = d.focuseditem.map(|f| std::ptr::eq(&d.def().items[f], item)).unwrap_or(false);
        let Some(h) = item.fn_handler() else { return };
        if d.dimmed && focused {
            if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
                let mut hd = HandlerData::default();
                h(self, MENUOP_GET_OPTION_HEIGHT, item, &mut hd);
                self.line_height = hd.value;
            } else {
                self.line_height = LINEHEIGHT;
            }
            let lh = self.line_height;
            let mut hd = HandlerData::default();
            h(self, MENUOP_GET_OPTION_COUNT, item, &mut hd);
            let numoptions = hd.value;
            let mut width = 0;
            let mut height = lh * numoptions;
            for i in 0..numoptions {
                hd.value = i;
                let text = h(self, MENUOP_GET_OPTION_TEXT, item, &mut hd).text();
                let tw = self.measure_f(&text, FontId::Sm).1 + 6;
                if tw > width {
                    width = tw;
                }
            }
            if x2 > 0 {
                cx = x + x2 - width - 7;
            }
            if cy + height > d.y + d.height + 2 {
                if height > d.height {
                    let mut i = d.height;
                    i /= lh;
                    i *= lh;
                    height = i;
                }
                cy = d.y + d.height - height + 2;
            }
            let ctx = Ctx { x: cx, y: cy, width, height, item, focused: 1, dialog: di, data, unk18: false };
            self.menuitem_list_render(&ctx);
        }
    }

    // ---- keyboard (menuitem.c:1053-1570) ----

    fn kb_string_empty_or_spaces(s: &[u8; 11]) -> bool {
        let end = s.iter().position(|&c| c == 0).unwrap_or(11);
        s[..end].iter().all(|&c| c == b' ')
    }

    fn kb_str(s: &[u8; 11]) -> String {
        let end = s.iter().position(|&c| c == 0).unwrap_or(11);
        s[..end].iter().map(|&b| b as char).collect()
    }

    /// `menuitem_keyboard_render` (menuitem.c:1088).
    fn menuitem_keyboard_render(&mut self, ctx: &Ctx) {
        let di = ctx.dialog;
        let Some(b) = ctx.data else { return };
        let data = *self.blk(b);
        let d = self.mr().dialogs[di];
        self.waves(di, Pal::ItemUnfocused);
        if ctx.item.param3.num() == 0 {
            self.draw.gfx.fill_rect_scaled(ctx.x + 4, ctx.y + 1, ctx.x + 63, ctx.y + 10, 0x0000ff7f);
        } else {
            self.draw.gfx.fill_rect_scaled(ctx.x + 4, ctx.y + 1, ctx.x + 125, ctx.y + 10, 0x0000ff7f);
        }
        let (mut x, mut y) = (ctx.x + 4, ctx.y + 2);
        let s = Self::kb_str(&data.string);
        self.tc().render_v2(&mut x, &mut y, &s, FontId::Sm, 0xffffffff, ctx.width, ctx.height, 0, 0);
        // Cursor
        let alpha = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
        let cursorcolour = colour_blend(colour_blend(0x0000ffff, 0x000000ff, 127), self.mix(di, Pal::ItemFocusedInner), alpha);
        self.draw.gfx.fill_rect_scaled(x + 1, ctx.y + 2, x + 3, ctx.y + 9, cursorcolour);
        // Grid lines
        for row in 0..6 {
            self.menugfx_draw_filled_rect(ctx.x + 4, ctx.y + row * 11 + 13, ctx.x + 124, ctx.y + row * 11 + 14, 0x00ffff7f, 0x00ffff7f);
        }
        for col in 0..11 {
            let rowspan = if matches!(col, 1 | 3 | 4 | 6 | 7 | 9) { 4 } else { 5 };
            self.menugfx_draw_filled_rect(ctx.x + col * 12 + 4, ctx.y + 13, ctx.x + col * 12 + 5, ctx.y + rowspan * 11 + 14, 0x00ffff7f, 0x00ffff7f);
        }
        let labels = [314u16, 315, 316, 317];
        for col in 0..10i32 {
            for row in 0..5i32 {
                let mut textcolour = dim(self.mix(di, Pal::ItemUnfocused), d.dimmed);
                if data.capseffective && col == 2 && row == 4 {
                    textcolour = (textcolour & 0xff) | 0xffff0000;
                }
                if col == data.col as i32 && row == data.row as i32 {
                    let alpha = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
                    let tmp = self.mix(di, Pal::ItemFocusedInner);
                    textcolour = colour_blend(colour_blend(textcolour, textcolour & 0xff, 127), tmp, alpha);
                }
                let mut x = ctx.x + col * 12 + 4;
                let mut y = ctx.y + row * 11 + 15;
                if row == 4 {
                    let index = match col {
                        0 => Some(0),
                        2 => Some(1),
                        5 => Some(2),
                        8 => Some(3),
                        _ => None,
                    };
                    if let Some(index) = index {
                        let buttonwidth = if index == 1 || index == 2 { 36 } else { 24 };
                        y += 1;
                        let label = self.lang(tx(gd::B_OPTIONS, labels[index]));
                        let tw = self.measure_f(&label, FontId::Xs).1;
                        x += (buttonwidth - tw) / 2;
                        let okdim = index == 3 && Self::kb_string_empty_or_spaces(&data.string);
                        if okdim {
                            textcolour = dim(self.mix(di, Pal::ItemDisabled), d.dimmed);
                            self.waves(di, Pal::ItemDisabled);
                        }
                        self.tc().render_v2(&mut x, &mut y, &label, FontId::Xs, textcolour, ctx.width, ctx.height, 0, 0);
                        if okdim {
                            self.waves(di, Pal::ItemUnfocused);
                        }
                    }
                } else {
                    let mut c = KEYBOARD_KEYS[row as usize][col as usize];
                    if !data.capseffective && c.is_ascii_uppercase() {
                        c += 32;
                    }
                    let label = format!("{}\n", c as char);
                    let tw = self.measure_f(&label, FontId::Sm).1;
                    x += (12 - tw) / 2;
                    self.tc().render_v2(&mut x, &mut y, &label, FontId::Sm, textcolour, ctx.width, ctx.height, 0, 0);
                }
            }
        }
        // Highlight border of focused button
        let (col, row) = (data.col as i32, data.row as i32);
        let x1 = ctx.x + col * 12 + 4;
        let mut x2 = ctx.x + col * 12 + 16;
        let y1 = ctx.y + row * 11 + 13;
        let y2 = ctx.y + row * 11 + 24;
        if row == 4 {
            x2 += match col {
                8 | 0 => 12,
                5 | 2 => 24,
                _ => 0,
            };
        }
        self.menugfx_draw_line(x1, y1, x2, y1 + 1, 0xffffffff, 0xffffffff);
        self.menugfx_draw_line(x2, y1, x2 + 1, y2 + 1, 0xffffffff, 0xffffffff);
        self.menugfx_draw_line(x1, y2, x2, y2 + 1, 0xffffffff, 0xffffffff);
        self.menugfx_draw_line(x1, y1, x1 + 1, y2 + 1, 0xffffffff, 0xffffffff);
    }

    /// `menuitem_keyboard_tick` (menuitem.c:1350).
    fn menuitem_keyboard_tick(&mut self, item: &'static MenuItem, inputs: &mut MenuInputs, tickflags: u32, b: usize) -> bool {
        let mut delete = false;
        if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 {
            let (prevcol, prevrow) = (self.blk(b).col, self.blk(b).row);
            {
                let kb = self.blk(b);
                if inputs.leftright != 0 {
                    loop {
                        kb.col += inputs.leftright;
                        if kb.col < 0 {
                            kb.col = 9;
                        }
                        if kb.col > 9 {
                            kb.col = 0;
                        }
                        if !(kb.row == 4 && kb.col != 0 && kb.col != 2 && kb.col != 5 && kb.col != 8) {
                            break;
                        }
                    }
                }
                if inputs.updown != 0 {
                    kb.row += inputs.updown;
                    if kb.row < 0 {
                        kb.row = 4;
                    }
                    if kb.row > 4 {
                        kb.row = 0;
                    }
                    if kb.row == 4 {
                        kb.col = match kb.col {
                            9 => 8,
                            7 | 6 => 5,
                            3 | 4 => 2,
                            1 => 0,
                            c => c,
                        };
                    }
                }
            }
            if prevcol != self.blk(b).col || prevrow != self.blk(b).row {
                self.menu_play_sound(MENUSOUND_KEYBOARDFOCUS);
            }
            if inputs.back2 != 0 {
                delete = true;
            }
            let h = item.fn_handler();
            if inputs.start {
                let s = self.blk(b).string;
                if let (Some(h), false) = (h, Self::kb_string_empty_or_spaces(&s)) {
                    self.menu_play_sound(MENUSOUND_SELECT);
                    let mut hd = HandlerData { string: s, ..HandlerData::default() };
                    h(self, MENUOP_SET_KEYBOARD_STRING, item, &mut hd);
                    self.menu_pop_dialog();
                    h(self, MENUOP_CONFIRM, item, &mut hd);
                    inputs.start = false;
                    return true;
                }
                inputs.start = false;
            }
            if inputs.select != 0 {
                let (row, col) = (self.blk(b).row, self.blk(b).col);
                if row == 4 {
                    if col == 0 {
                        delete = true;
                    }
                    if col == 2 {
                        let kb = self.blk(b);
                        kb.capslock = !kb.capslock;
                    }
                    let s = self.blk(b).string;
                    let mut hd = HandlerData { string: s, ..HandlerData::default() };
                    if col == 8 {
                        if let (Some(h), false) = (h, Self::kb_string_empty_or_spaces(&s)) {
                            h(self, MENUOP_SET_KEYBOARD_STRING, item, &mut hd);
                        }
                    }
                    if col == 8 || col == 5 {
                        let ok = col == 8;
                        if col == 5 || !Self::kb_string_empty_or_spaces(&s) {
                            self.menu_pop_dialog();
                            if ok {
                                if let Some(h) = h {
                                    h(self, MENUOP_CONFIRM, item, &mut hd);
                                }
                                self.menu_play_sound(MENUSOUND_SELECT);
                            } else {
                                self.menu_play_sound(MENUSOUND_KEYBOARDCANCEL);
                            }
                            inputs.select = 0;
                            return true;
                        }
                    }
                } else {
                    let kb = *self.blk(b);
                    if kb.string[9] == 0 {
                        let mut key = KEYBOARD_KEYS[row as usize][col as usize];
                        if !kb.capseffective && key.is_ascii_uppercase() {
                            key += 32;
                        }
                        let i = kb.string.iter().position(|&c| c == 0).unwrap_or(10);
                        self.blk(b).string[i] = key;
                        let s = Self::kb_str(&self.blk(b).string);
                        let tw = self.measure_f(&s, FontId::Sm).1;
                        if item.param3.num() == 0 && tw > 58 {
                            delete = true;
                        }
                        if !delete {
                            self.menu_play_sound(MENUSOUND_FOCUS);
                        }
                    }
                }
            }
            if delete && self.blk(b).string[0] != 0 {
                self.menu_play_sound(MENUSOUND_FOCUS);
                let kb = self.blk(b);
                let mut i = gd::MAX_USERSTRING_LEN as usize;
                loop {
                    if kb.string[i] != 0 {
                        kb.string[i] = 0;
                        break;
                    }
                    if i == 0 {
                        break;
                    }
                    i -= 1;
                }
            }
            // Update caps
            let prev = self.blk(b).capseffective;
            let mut eff = self.blk(b).capslock;
            if inputs.shoulder != 0 {
                eff = !eff;
            }
            self.blk(b).capseffective = eff;
            if eff != prev {
                self.menu_play_sound(if eff { MENUSOUND_TOGGLEON } else { MENUSOUND_TOGGLEOFF });
            }
        }
        true
    }

    /// `menuitem_keyboard_init` (menuitem.c:1550).
    fn menuitem_keyboard_init(&mut self, item: &'static MenuItem, b: usize) {
        self.blk(b).string = [0; 11];
        if let Some(h) = item.fn_handler() {
            let mut hd = HandlerData::default();
            h(self, MENUOP_GET_KEYBOARD_STRING, item, &mut hd);
            self.blk(b).string = hd.string;
        }
        let kb = self.blk(b);
        kb.col = 0;
        kb.row = 4;
        kb.capseffective = false;
        kb.capslock = false;
    }

    // ---- separator, label, meter, selectable ----

    /// `menuitem_separator_render` (menuitem.c:1571).
    fn menuitem_separator_render(&mut self, ctx: &Ctx) {
        let colour = (self.mix(ctx.dialog, Pal::ItemUnfocused) & 0xffffff00) | 0x3f;
        self.menugfx_draw_filled_rect(ctx.x, ctx.y + 2, ctx.x + ctx.width, ctx.y + 3, colour, colour);
    }

    /// `menuitem_label_render` (menuitem.c:1859).
    fn menuitem_label_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let saved = self.draw.text.holoray_enabled;
        let mut font = FontId::Sm;
        let mut x = ctx.x + 10;
        let mut y = ctx.y + 2;
        if item.flags & MENUITEMFLAG_LESSLEFTPADDING != 0 {
            x -= 6;
        }
        let Some(text) = self.menu_resolve_param2_text(item) else { return };
        if item.flags & MENUITEMFLAG_SMALLFONT != 0 {
            font = FontId::Xs;
            y -= 2;
        }
        if item.flags & MENUITEMFLAG_SELECTABLE_CENTRE != 0 {
            let tw = self.measure_f(&text, font).1;
            x = ctx.x + (ctx.width - tw) / 2;
        }
        let dimmed = self.dimmed(di);
        let mut colour1;
        if item.flags & MENUITEMFLAG_LABEL_ALTCOLOUR != 0 {
            colour1 = dim(self.mix(di, Pal::CheckboxCheckedUnfocused), dimmed);
            self.waves(di, Pal::CheckboxCheckedUnfocused);
        } else {
            colour1 = dim(self.mix(di, Pal::ItemUnfocused), dimmed);
            self.waves(di, Pal::ItemUnfocused);
        }
        if self.menu_is_item_disabled(item, di) {
            colour1 = dim(self.mix(di, Pal::ItemDisabled), dimmed);
            self.waves(di, Pal::ItemDisabled);
        }
        let redraw = self.menu_find_item_redraw_info(Some(item as *const MenuItem));
        if let Some(ri) = redraw {
            let t = self.mr().itemredrawinfo[ri].timer60;
            if t < 0.0 {
                return;
            }
            self.draw.text.backup_diagonal_blend_settings();
            self.draw.text.set_diagonal_blend(x, y, t * 300.0, 0);
            self.draw.text.holoray_enabled = true;
        }
        let mut colour2 = colour1;
        if item.flags & MENUITEMFLAG_LABEL_CUSTOMCOLOUR != 0 {
            if let Some(h) = item.fn_handler() {
                let mut hd = HandlerData { colour1, colour2, ..HandlerData::default() };
                h(self, MENUOP_GET_LABEL_COLOURS, item, &mut hd);
                colour1 = hd.colour1;
                colour2 = hd.colour2;
            }
        }
        self.tc().render_v2(&mut x, &mut y, &text, font, colour1, ctx.width, ctx.height, 0, 0);
        if item.flags & MENUITEMFLAG_LABEL_HASRIGHTTEXT == 0 {
            if let Some(t3) = self.menu_resolve_text(item.param3, item) {
                let mut y = ctx.y + 2;
                if item.flags & MENUITEMFLAG_SMALLFONT != 0 {
                    y -= 2;
                }
                let tw = self.measure_f(&t3, font).1;
                let mut x = ctx.x + ctx.width - tw - 10;
                if item.flags & MENUITEMFLAG_LESSLEFTPADDING != 0 {
                    x += 6;
                }
                self.tc().render_v2(&mut x, &mut y, &t3, font, colour2, ctx.width, ctx.height, 0, 0);
            }
        }
        if let Some(ri) = redraw {
            let t = self.mr().itemredrawinfo[ri].timer60;
            let dr = self.mr().dialogs[di].redrawtimer;
            if ((ctx.width + 200) as f32) < t * 300.0 && dr < 0.0 {
                self.menu_remove_item_redraw_info(item);
            }
            self.draw.text.holoray_enabled = saved;
            self.draw.text.restore_diagonal_blend_settings();
        }
        if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
            if let Some(h) = item.fn_handler() {
                let rd = RenderData { x: ctx.x, y: ctx.y, width: ctx.width, colour: colour1, unk10: false };
                let mut hd = HandlerData { render: Some(rd), ..HandlerData::default() };
                h(self, MENUOP_RENDER, item, &mut hd);
            }
        }
    }

    /// `menuitem_selectable_render` (menuitem.c:2108).
    fn menuitem_selectable_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let mut x = ctx.x + 10;
        let mut y = ctx.y + 2;
        let mut font = FontId::Sm;
        if item.flags & MENUITEMFLAG_LESSLEFTPADDING != 0 {
            x -= 6;
        }
        if item.flags & MENUITEMFLAG_BIGFONT != 0 {
            font = FontId::Md;
        }
        let text = self.menu_resolve_param2_text(item).unwrap_or_default();
        let dimmed = self.dimmed(di);
        let mut leftcolour = dim(self.mix(di, Pal::ItemUnfocused), dimmed);
        let mut rightcolour = leftcolour;
        if ctx.focused != 0 {
            let weight = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
            let c2 = self.mix(di, Pal::ItemFocusedInner);
            leftcolour = colour_blend(colour_blend(leftcolour, leftcolour & 0xff, 127), c2, weight);
            self.waves(di, Pal::ItemFocusedInner);
        } else {
            self.waves(di, Pal::ItemUnfocused);
        }
        if self.menu_is_item_disabled(item, di) {
            leftcolour = dim(self.mix(di, Pal::ItemDisabled), dimmed);
            rightcolour = leftcolour;
            self.waves(di, Pal::ItemDisabled);
        }
        if item.flags & MENUITEMFLAG_SELECTABLE_CENTRE != 0 {
            let tw = self.measure_f(&text, font).1;
            x = ctx.x + (ctx.width - tw) / 2;
        }
        if item.flags & MENUITEMFLAG_BIGFONT != 0 {
            x += 35;
            y += 6;
        }
        self.tc().render_v2(&mut x, &mut y, &text, font, leftcolour, ctx.width, ctx.height, 0, 0);
        if item.flags & (MENUITEMFLAG_LABEL_HASRIGHTTEXT | MENUITEMFLAG_BIGFONT) == 0 {
            if let Some(t3) = self.menu_resolve_text(item.param3, item) {
                let mut y = ctx.y + 2;
                let tw = self.measure_f(&t3, font).1;
                let mut x = ctx.x + ctx.width - tw - 10;
                self.tc().render_v2(&mut x, &mut y, &t3, font, rightcolour, ctx.width, ctx.height, 0, 0);
            }
        }
    }

    /// `menuitem_selectable_tick` (menuitem.c:2230).
    fn menuitem_selectable_tick(&mut self, item: &'static MenuItem, inputs: &mut MenuInputs, tickflags: u32) -> bool {
        if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 && inputs.select != 0 {
            self.menu_play_sound(MENUSOUND_SELECT);
            if item.flags & MENUITEMFLAG_SELECTABLE_CLOSESDIALOG != 0 {
                self.menu_pop_dialog();
            }
            if item.flags & MENUITEMFLAG_SELECTABLE_OPENSDIALOG != 0 {
                if let H::Dialog(d) = item.handler {
                    self.menu_push_dialog(d);
                }
            } else if let H::Fn(h) = item.handler {
                let mut hd = HandlerData::default();
                h(self, MENUOP_CONFIRM, item, &mut hd);
            }
        }
        true
    }

    // ---- slider (menuitem.c:2250) ----

    fn menuitem_slider_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let mut slidervalue = 0;
        if let Some(h) = item.fn_handler() {
            let mut hd = HandlerData::default();
            h(self, MENUOP_GET_SLIDER_VALUE, item, &mut hd);
            slidervalue = hd.value as i16 as i32;
        }
        let d = self.mr().dialogs[di];
        let extray = if d.unk6e != 0 || item.flags & MENUITEMFLAG_SLIDER_ALTSIZE != 0 { 10 } else { 0 };
        let mut x = ctx.x + 10;
        let mut y = ctx.y + 2;
        if item.flags & MENUITEMFLAG_LESSLEFTPADDING != 0 {
            x -= 6;
        }
        let label = self.menu_resolve_param2_text(item).unwrap_or_default();
        let p3 = item.param3.num().max(1);
        let markerx = ctx.x + ctx.width + slidervalue * 75 / p3 - 82;
        let mut colour = dim(self.mix(di, Pal::ItemUnfocused), d.dimmed);
        if ctx.focused != 0 {
            if ctx.focused & 2 != 0 {
                let weight = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
                let tmp = self.mix(di, Pal::ItemFocusedInner);
                colour = colour_blend(colour, colour & 0xff, 0x7f);
                colour = colour_blend(colour, tmp, weight) | 0xff;
            }
            self.waves(di, Pal::ItemFocusedInner);
        } else {
            self.waves(di, Pal::ItemUnfocused);
        }
        self.menugfx_render_slider(ctx.x + ctx.width - 82, ctx.y + extray + 5, ctx.x + ctx.width - 7, ctx.y + extray + 11, markerx, colour);
        let mut colour = dim(self.mix(di, Pal::ItemUnfocused), d.dimmed);
        if ctx.focused != 0 {
            let freq = if ctx.focused & 2 != 0 { 20.0 } else { 40.0 };
            let weight = (sin_osc(self.frac20, freq) * 255.0) as u32;
            let tmp = self.mix(di, Pal::ItemFocusedInner);
            colour = colour_blend(colour, colour & 0xff, 0x7f);
            colour = colour_blend(colour, tmp, weight);
            self.waves(di, Pal::ItemFocusedInner);
        } else {
            self.waves(di, Pal::ItemUnfocused);
        }
        self.tc().render_v2(&mut x, &mut y, &label, FontId::Sm, colour, ctx.width, ctx.height, 0, 0);
        if item.flags & MENUITEMFLAG_SLIDER_HIDEVALUE == 0 {
            let mut buffer = format!("{slidervalue}\n");
            if let Some(h) = item.fn_handler() {
                let mut hd = HandlerData { value: slidervalue, label: buffer.clone(), ..HandlerData::default() };
                h(self, MENUOP_GET_SLIDER_LABEL, item, &mut hd);
                buffer = hd.label;
            }
            let tw = self.measure_f(&buffer, FontId::Sm).1;
            let mut x = ctx.x + ctx.width - tw - 7;
            let mut y = ctx.y + 2;
            let colour = dim(self.mix(di, Pal::ItemUnfocused), d.dimmed);
            self.waves(di, Pal::ItemUnfocused);
            let colour = (colour & 0xffffff00) | ((colour & 0xff) >> 1);
            self.tc().render_v2(&mut x, &mut y, &buffer, FontId::Sm, colour, ctx.width, ctx.height, 0, 0);
        }
    }

    /// `menuitem_slider_tick` (menuitem.c:2379).
    fn menuitem_slider_tick(&mut self, item: &'static MenuItem, di: usize, inputs: &mut MenuInputs, tickflags: u32, b: usize) -> bool {
        if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 {
            if tickflags & MENUTICKFLAG_DIALOGISDIMMED != 0 {
                let mut index = 0;
                if let Some(h) = item.fn_handler() {
                    let mut hd = HandlerData::default();
                    h(self, MENUOP_GET_SLIDER_VALUE, item, &mut hd);
                    index = hd.value as i16 as i32;
                }
                let p3 = item.param3.num().max(1) as f32;
                let slow = self.mr().xrepeatmode == MENUREPEATMODE_SLOW;
                let diffframe60 = self.lv.diffframe60 as f32;
                if item.flags & MENUITEMFLAG_SLIDER_FAST == 0 && slow {
                    index += inputs.leftright as i32;
                } else {
                    let mut f0 = self.blk(b).multiplier as f32 / 1000.0;
                    f0 = f0 * 100.0 / p3;
                    f0 += inputs.leftrightheld as f32 * diffframe60;
                    f0 = p3 * f0 / 100.0;
                    let tmp = f0 as i32;
                    f0 -= tmp as f32;
                    index += tmp;
                    self.blk(b).multiplier = (f0 * 1000.0) as i16;
                }
                let f14 = inputs.xaxis as f32;
                let f2 = f14.abs();
                if item.flags & MENUITEMFLAG_SLIDER_FAST == 0 && f2 < 40.0 {
                    if !slow {
                        index += inputs.leftright as i32;
                    }
                } else {
                    let mut f0 = self.blk(b).multiplier as f32 / 1000.0;
                    f0 = f0 * 100.0 / p3;
                    let mut f2 = f2;
                    if f2 > 20.0 {
                        f2 = (f2 - 20.0) / 16.0;
                        f2 *= self.lv.diffframe60f;
                        if inputs.xaxis < 0 {
                            f0 -= f2;
                        } else {
                            f0 += f2;
                        }
                    }
                    f0 = p3 * f0 / 100.0;
                    let tmp = f0 as i32;
                    f0 -= tmp as f32;
                    index += tmp;
                    self.blk(b).multiplier = (f0 * 1000.0) as i16;
                }
                index = index.clamp(0, item.param3.num());
                inputs.leftright = 0;
                if let Some(h) = item.fn_handler() {
                    let mut hd = HandlerData { value: index, ..HandlerData::default() };
                    h(self, MENUOP_CONFIRM, item, &mut hd);
                }
                if inputs.select != 0 {
                    self.dlg(di).dimmed = false;
                }
            } else if inputs.select != 0 {
                self.dlg(di).dimmed = true;
            }
        }
        true
    }

    // ---- carousel (menuitem.c:2494) ----

    fn menuitem_carousel_render(&mut self, ctx: &Ctx) {
        let mut colour = 0xff0000ff;
        if ctx.focused != 0 {
            let weight = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
            let c1 = self.mix(ctx.dialog, Pal::ItemFocusedInner);
            colour = colour_blend(colour_blend(colour, 0x000000ff, 127), c1, weight);
        }
        self.menugfx_draw_carousel_chevron(ctx.x, ctx.y + ctx.height / 2, 8, 1, 0xffffffff, colour);
        self.menugfx_draw_carousel_chevron(ctx.x + ctx.width, ctx.y + ctx.height / 2, 8, 3, 0xffffffff, colour);
    }

    /// `menuitem_carousel_tick` (menuitem.c:2542).
    fn menuitem_carousel_tick(&mut self, item: &'static MenuItem, inputs: &mut MenuInputs, tickflags: u32) -> bool {
        let Some(h) = item.fn_handler() else { return true };
        if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 || item.flags & MENUITEMFLAG_CAROUSEL_SCROLLWITHOUTFOCUS != 0 {
            let mut hd = HandlerData::default();
            if inputs.leftright != 0 && (!self.mp_is_player_locked_out(self.mpplayernum as i32) || item.flags & MENUITEMFLAG_LOCKABLEMINOR == 0) {
                h(self, MENUOP_GET_OPTION_COUNT, item, &mut hd);
                let numoptions = hd.value;
                h(self, MENUOP_GET_SELECTED_INDEX, item, &mut hd);
                let mut index = hd.value;
                let mut guard = 0;
                loop {
                    index += inputs.leftright as i32;
                    if index >= numoptions {
                        index = 0;
                    }
                    if index < 0 {
                        index = numoptions - 1;
                    }
                    hd.value = index;
                    guard += 1;
                    if h(self, MENUOP_IS_CAROUSEL_OPTION_HIDDEN, item, &mut hd).int() == 0 || guard > 512 {
                        break;
                    }
                }
                hd.value = index;
                hd.unk04 = inputs.shoulder as i32;
                h(self, MENUOP_CONFIRM, item, &mut hd);
            }
            h(self, MENUOP_ON_CAROUSEL_TICK, item, &mut hd);
        }
        true
    }

    // ---- checkbox (menuitem.c:2597) ----

    fn menuitem_checkbox_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let font = if item.flags & MENUITEMFLAG_SMALLFONT != 0 { FontId::Xs } else { FontId::Sm };
        let text = self.menu_resolve_param2_text(item).unwrap_or_default();
        let dimmed = self.dimmed(di);
        let mut checked = false;
        let mut fillcolour = 0xff002faf;
        let mut maincolour;
        let is_checked = item.fn_handler().map(|h| {
            let mut hd = HandlerData::default();
            h(self, MENUOP_IS_CHECKED, item, &mut hd).int() == 1
        });
        if is_checked == Some(true) {
            checked = true;
            maincolour = dim(self.mix(di, Pal::CheckboxCheckedUnfocused), dimmed);
            self.waves(di, Pal::CheckboxCheckedUnfocused);
        } else {
            maincolour = dim(self.mix(di, Pal::ItemUnfocused), dimmed);
            self.waves(di, Pal::ItemUnfocused);
        }
        if ctx.focused != 0 {
            let weight = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
            let fc = self.mix(di, Pal::ItemFocusedInner);
            maincolour = colour_blend(colour_blend(maincolour, maincolour & 0xff, 127), fc, weight);
            self.waves(di, Pal::ItemFocusedInner);
        }
        if self.menu_is_item_disabled(item, di) {
            maincolour = dim(self.mix(di, Pal::ItemDisabled), dimmed);
            self.waves(di, Pal::ItemDisabled);
            fillcolour = 0x7f002faf;
        }
        self.menugfx_draw_checkbox(ctx.x + ctx.width - 16, ctx.y + 2, 6, checked, maincolour, fillcolour);
        let (mut x, mut y) = (ctx.x + 10, ctx.y + 2);
        self.tc().render_v2(&mut x, &mut y, &text, font, maincolour, ctx.width, ctx.height, 0, 0);
    }

    /// `menuitem_checkbox_tick` (menuitem.c:2717).
    fn menuitem_checkbox_tick(&mut self, item: &'static MenuItem, inputs: &mut MenuInputs, tickflags: u32) -> bool {
        if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 && inputs.select != 0 {
            let mut hd = HandlerData::default();
            let checked = item.fn_handler().map(|h| h(self, MENUOP_IS_CHECKED, item, &mut hd).int() == 1).unwrap_or(false);
            if checked {
                hd.value = 0;
                self.menu_play_sound(MENUSOUND_TOGGLEOFF);
            } else {
                hd.value = 1;
                self.menu_play_sound(MENUSOUND_TOGGLEON);
            }
            if let Some(h) = item.fn_handler() {
                h(self, MENUOP_CONFIRM, item, &mut hd);
            }
        }
        true
    }

    // ---- scrollable (menuitem.c:2738) ----

    fn menuitem_scrollable_get_text(&mut self, ty: i32) -> String {
        match ty {
            gd::DESCRIPTION_MPCONFIG => {
                let cfg = self.mr().training_config;
                cfg.map(|c| self.draw.res.mpconfigs[c].description.clone()).unwrap_or_default()
            }
            gd::DESCRIPTION_MPCHALLENGE => {
                if !self.challenge_is_loaded() {
                    self.m().menumodel.curparams = 0x4fac5ace;
                    self.challenge_load_and_store_current();
                }
                self.challenge_get_current_description()
            }
            _ => String::new(),
        }
    }

    /// `menuitem_scrollable_render` (menuitem.c:2777).
    fn menuitem_scrollable_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let Some(b) = ctx.data else { return };
        let paddingright = if self.menu_is_scrollable_unscrollable(item) { 10 } else { 24 };
        let raw = self.menuitem_scrollable_get_text(item.param);
        let alltext = wrap(ctx.width - paddingright, &raw, &self.draw.res.fonts.sm);
        let (mut heading, mut body) = (String::new(), String::new());
        let mut inheading = false;
        let mut prevwaslinebreak = false;
        for c in alltext.chars() {
            if c == '|' {
                inheading = true;
            } else if c == '\n' {
                body.push('\n');
                heading.push('\n');
                if prevwaslinebreak {
                    inheading = false;
                }
                prevwaslinebreak = true;
            } else {
                prevwaslinebreak = false;
                if inheading {
                    heading.push(c);
                } else {
                    body.push(c);
                }
            }
        }
        let colour = dim(self.mix(di, Pal::ItemUnfocused), self.dimmed(di));
        self.waves(di, Pal::ItemUnfocused);
        let so = self.blk(b).scrolloffset as i32;
        let (mut x, mut y) = (ctx.x + 3, ctx.y + 3);
        self.tc().render_v2(&mut x, &mut y, &heading, FontId::Sm, 0x000000ff, ctx.width - 4, ctx.height - 4, -so, 0);
        let (mut x, mut y) = (ctx.x + 2, ctx.y + 2);
        self.tc().render_v2(&mut x, &mut y, &heading, FontId::Sm, 0xff4444ff, ctx.width - 4, ctx.height - 4, -so, 0);
        let mut x = if self.menu_is_scrollable_unscrollable(item) { ctx.x + 5 } else { ctx.x + 12 };
        let mut y = ctx.y + 2;
        self.tc().render_v2(&mut x, &mut y, &body, FontId::Sm, colour, ctx.width - 4, ctx.height - 1, -so, 0);
    }

    /// `menuitem_scrollable_tick` (menuitem.c:2885).
    fn menuitem_scrollable_tick(&mut self, item: &'static MenuItem, di: usize, inputs: &mut MenuInputs, tickflags: u32, b: usize) -> bool {
        let dh = self.mr().dialogs[di].height as i16;
        if dh != self.blk(b).dialogheight {
            let focus = Some(self.mr().dialogs[di].def().items.iter().position(|i| std::ptr::eq(i, item)).unwrap_or(0));
            let (_, rowindex, colindex) = self.dialog_find_item(di, focus);
            let colwidth = self.mr().cols[colindex].width as i32;
            let rowheight = self.mr().rows[rowindex].height as i32;
            let width = if self.menu_is_scrollable_unscrollable(item) { colwidth - 10 } else { colwidth - 24 };
            let raw = self.menuitem_scrollable_get_text(item.param);
            let wrapped = wrap(width, &raw, &self.draw.res.fonts.sm);
            let (height, _) = measure(&self.draw.res.fonts.sm, &wrapped, 0);
            let d = self.blk(b);
            d.maxscrolloffset = (height - rowheight + 5).max(-10) as i16;
            d.dialogheight = dh;
        }
        if self.menu_is_scrollable_unscrollable(item) {
            self.blk(b).scrolloffset = 0;
        } else if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 {
            let mut intval = 0;
            let mut f = (inputs.yaxis as f32).abs();
            if f > 20.0 {
                f = (f - 20.0) / 5.0;
                f *= self.lv.diffframe60f;
                intval = if inputs.yaxis < 0 { f as i32 } else { -(f as i32) };
            }
            intval += inputs.updownheld as i32 * 2 * self.lv.diffframe60;
            let d = self.blk(b);
            d.scrolloffset = (d.scrolloffset as i32 + intval).clamp(-10, (d.maxscrolloffset as i32).max(-10)) as i16;
        }
        true
    }

    // ---- marquee (menuitem.c:2987) ----

    fn menuitem_marquee_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        let di = ctx.dialog;
        let Some(b) = ctx.data else { return };
        let Some(text) = self.menu_resolve_param2_text(item) else { return };
        let font = if item.flags & MENUITEMFLAG_SMALLFONT != 0 { FontId::Xs } else { FontId::Sm };
        let colour = dim(self.mix(di, Pal::ItemUnfocused), self.dimmed(di));
        let totalmoved = self.blk(b).totalmoved as i32;
        let mut x = ctx.x + ctx.width - totalmoved;
        let mut y = ctx.y + 2;
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let s = chars[i].to_string();
            let tw = self.measure_f(&s, font).1;
            if x + tw > ctx.x {
                break;
            }
            x += tw;
            i += 1;
        }
        let rest: String = chars[i..].iter().collect();
        self.set_scissor_clamped(ctx.x, ctx.y, ctx.x + ctx.width, ctx.y + ctx.height - 1);
        self.draw.text.backup_and_reset_blends();
        if item.flags & MENUITEMFLAG_MARQUEE_FADEBOTHSIDES != 0 {
            self.draw.text.set_horizontal_blend(ctx.x, ctx.x + ctx.width, 14);
        } else {
            self.draw.text.set_horizontal_blend(ctx.x, ctx.x, 14);
        }
        let w = ctx.width + ctx.x - x;
        self.tc().render_v2(&mut x, &mut y, &rest, font, colour, w, ctx.height, 0, 0);
        self.menu_apply_scissor();
        self.draw.text.restore_blends();
        self.blk(b).viewwidth = ctx.width as u16;
    }

    /// `menuitem_marquee_tick` (menuitem.c:3166).
    fn menuitem_marquee_tick(&mut self, item: &'static MenuItem, b: usize) -> bool {
        let font = if item.flags & MENUITEMFLAG_SMALLFONT != 0 { FontId::Xs } else { FontId::Sm };
        let Some(text) = self.menu_resolve_param2_text(item) else { return true };
        let hash = text.bytes().fold(0u16, |a, c| a.wrapping_add(c as u16));
        if self.blk(b).texthash != hash {
            self.blk(b).totalmoved = 0;
            self.blk(b).texthash = hash;
        }
        let tw = self.measure_f(&text, font).1;
        let limit = self.blk(b).viewwidth as i32 + tw;
        let increment = (self.lv.diffframe60 / 2).max(1);
        let d = self.blk(b);
        d.totalmoved = d.totalmoved.wrapping_add(increment as u16);
        if d.totalmoved as i32 > limit {
            d.totalmoved = 0;
        }
        true
    }

    // ---- player stats (menuitem.c:3505) ----

    fn menuitem_player_stats_render(&mut self, ctx: &Ctx) {
        let di = ctx.dialog;
        let Some(b) = ctx.data else { return };
        let playernum = self.mp_selected_for_stats[self.mpplayernum];
        let Some(mpchr) = self.mpchr(playernum) else { return };
        let weight = (sin_osc(self.frac20, 40.0) * 255.0) as u32;
        let sel = colour_blend(colour_blend(0xffffffff, 0x000000ff, 127), self.mix(di, Pal::ItemFocusedInner), weight);
        let (mut x, mut y) = (ctx.x + 2, ctx.y + 1);
        self.tc().render_v2(&mut x, &mut y, &mpchr.name, FontId::Sm, sel, ctx.width, ctx.height, 0, 0);
        let main = dim(self.mix(di, Pal::ItemUnfocused), self.dimmed(di));
        let suicides = self.lang(tx(gd::B_MPMENU, 281));
        let tw = self.measure_f(&suicides, FontId::Xs).1;
        let (mut x, mut y) = (ctx.x - tw + 121, ctx.y + 1);
        self.tc().render_v2(&mut x, &mut y, &suicides, FontId::Xs, main, ctx.width, ctx.height, 0, 0);
        let buffer = format!("{}\n", mpchr.killcounts[playernum]);
        let tw2 = self.measure_f(&buffer, FontId::Sm).1;
        let (mut x, mut y) = (ctx.x - tw + 119 - tw2, ctx.y + 1);
        self.tc().render_v2(&mut x, &mut y, &buffer, FontId::Sm, 0xffff00ff, ctx.width, ctx.height, 0, 0);
        let mut ypos = 12;
        if self.mp_get_num_chrs() >= 2 {
            let deaths = self.lang(tx(gd::B_MPMENU, 282));
            let tw = self.measure_f(&deaths, FontId::Xs).1;
            let (mut x, mut y) = (ctx.x - tw + 120, ctx.y + ypos);
            self.tc().render_v2(&mut x, &mut y, &deaths, FontId::Xs, main, ctx.width, ctx.height, 0, 0);
            let kills = self.lang(tx(gd::B_MPMENU, 283));
            let tw = self.measure_f(&kills, FontId::Xs).1;
            let (mut x, mut y) = (ctx.x - tw + 25, ctx.y + ypos);
            self.tc().render_v2(&mut x, &mut y, &kills, FontId::Xs, main, ctx.width, ctx.height, 0, 0);
            ypos += 7;
            let numchrs = self.mp_get_num_chrs();
            let gap = (numchrs * (LINEHEIGHT - 1) - ctx.height + ypos - 10).max(0);
            if self.blk(b).scrolloffset as i32 > gap {
                self.blk(b).scrolloffset = gap as i16;
            }
            self.set_scissor_clamped(ctx.x, ctx.y + ypos, ctx.x + ctx.width, ctx.y + ctx.height);
            ypos -= self.blk(b).scrolloffset as i32;
            for i in 0..gd::MAX_MPCHRS as usize {
                if self.mp.setup.chrslots & (1 << i) != 0 && i != playernum {
                    let Some(other) = self.mpchr(i) else { continue };
                    let (mut x, mut y) = (ctx.x + 29, ctx.y + ypos);
                    self.tc().render_v2(&mut x, &mut y, &other.name, FontId::Sm, 0x00ffffff, ctx.width, ctx.height, 0, 0);
                    let buffer = format!("{}\n", other.killcounts[playernum]);
                    let tw = self.measure_f(&buffer, FontId::Sm).1;
                    let (mut x, mut y) = (ctx.x - tw + 120, ctx.y + ypos);
                    self.tc().render_v2(&mut x, &mut y, &buffer, FontId::Sm, 0xff4040ff, ctx.width, ctx.height, 0, 0);
                    let buffer = format!("{}\n", mpchr.killcounts[i]);
                    let tw = self.measure_f(&buffer, FontId::Sm).1;
                    let (mut x, mut y) = (ctx.x - tw + 25, ctx.y + ypos);
                    self.tc().render_v2(&mut x, &mut y, &buffer, FontId::Sm, 0x00ff00ff, ctx.width, ctx.height, 0, 0);
                    ypos += 10;
                }
            }
            self.menu_apply_scissor();
        }
    }

    // ---- model (menuitem.c:1824) ----

    fn menuitem_model_render(&mut self, ctx: &Ctx) {
        let item = ctx.item;
        if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
            if let Some(h) = item.fn_handler() {
                let mut colour = self.mix(ctx.dialog, Pal::ItemFocusedInner);
                colour = dim(colour, self.dimmed(ctx.dialog));
                let rd = RenderData { x: ctx.x, y: ctx.y, width: ctx.width, colour, unk10: true };
                let mut hd = HandlerData { render: Some(rd), ..HandlerData::default() };
                h(self, MENUOP_RENDER, item, &mut hd);
            }
        }
    }

    // ---- dispatch (menuitem.c:4307) ----

    pub fn menuitem_render(&mut self, ctx: &Ctx) {
        match ctx.item.ty {
            MENUITEMTYPE_LIST => self.menuitem_list_render(ctx),
            MENUITEMTYPE_SELECTABLE => self.menuitem_selectable_render(ctx),
            MENUITEMTYPE_SLIDER => self.menuitem_slider_render(ctx),
            MENUITEMTYPE_CHECKBOX => self.menuitem_checkbox_render(ctx),
            MENUITEMTYPE_SCROLLABLE => self.menuitem_scrollable_render(ctx),
            MENUITEMTYPE_MARQUEE => self.menuitem_marquee_render(ctx),
            MENUITEMTYPE_LABEL => self.menuitem_label_render(ctx),
            MENUITEMTYPE_SEPARATOR => self.menuitem_separator_render(ctx),
            MENUITEMTYPE_DROPDOWN => self.menuitem_dropdown_render(ctx),
            MENUITEMTYPE_KEYBOARD => self.menuitem_keyboard_render(ctx),
            MENUITEMTYPE_PLAYERSTATS => self.menuitem_player_stats_render(ctx),
            MENUITEMTYPE_CAROUSEL => self.menuitem_carousel_render(ctx),
            MENUITEMTYPE_MODEL => self.menuitem_model_render(ctx),
            _ => {}
        }
    }

    /// `menuitem_tick` (menuitem.c:4336): true = use the default navigation.
    pub fn menuitem_tick(&mut self, item: &'static MenuItem, di: usize, inputs: &mut MenuInputs, tickflags: u32, data: Option<usize>) -> bool {
        match (item.ty, data) {
            (MENUITEMTYPE_LIST, Some(b)) => self.menuitem_list_tick(item, inputs, tickflags, b),
            (MENUITEMTYPE_SELECTABLE, _) => self.menuitem_selectable_tick(item, inputs, tickflags),
            (MENUITEMTYPE_SLIDER, Some(b)) => self.menuitem_slider_tick(item, di, inputs, tickflags, b),
            (MENUITEMTYPE_CHECKBOX, _) => self.menuitem_checkbox_tick(item, inputs, tickflags),
            (MENUITEMTYPE_SCROLLABLE, Some(b)) => self.menuitem_scrollable_tick(item, di, inputs, tickflags, b),
            (MENUITEMTYPE_MARQUEE, Some(b)) => self.menuitem_marquee_tick(item, b),
            (MENUITEMTYPE_DROPDOWN, Some(b)) => self.menuitem_dropdown_tick(item, di, inputs, tickflags, b),
            (MENUITEMTYPE_KEYBOARD, Some(b)) => self.menuitem_keyboard_tick(item, inputs, tickflags, b),
            (MENUITEMTYPE_CAROUSEL, _) => self.menuitem_carousel_tick(item, inputs, tickflags),
            (MENUITEMTYPE_PLAYERSTATS, Some(b)) => {
                if tickflags & MENUTICKFLAG_ITEMISFOCUSED != 0 && !self.dimmed(di) {
                    let mut intval = 0;
                    let mut f = (inputs.yaxis as f32).abs();
                    if f > 20.0 {
                        f = (f - 20.0) / 5.0 * self.lv.diffframe60f;
                        intval = if inputs.yaxis < 0 { f as i32 } else { -(f as i32) };
                    }
                    intval += inputs.updownheld as i32 * 2 * self.lv.diffframe60;
                    let d = self.blk(b);
                    d.scrolloffset = (d.scrolloffset as i32 + intval).max(0) as i16;
                }
                self.menuitem_dropdown_tick(item, di, inputs, tickflags, b)
            }
            _ => true,
        }
    }

    /// `menuitem_init` (menuitem.c:4355).
    pub fn menuitem_init(&mut self, item: &'static MenuItem, data: Option<usize>) {
        let Some(b) = data else { return };
        match item.ty {
            MENUITEMTYPE_LIST | MENUITEMTYPE_DROPDOWN => self.menuitem_dropdown_init(item, b),
            MENUITEMTYPE_SCROLLABLE => {
                let d = self.blk(b);
                d.dialogheight = -1;
                d.scrolloffset = -10;
            }
            MENUITEMTYPE_MARQUEE => {
                let d = self.blk(b);
                d.totalmoved = 0;
                d.viewwidth = 50;
            }
            MENUITEMTYPE_SLIDER => self.blk(b).multiplier = 0,
            MENUITEMTYPE_PLAYERSTATS => {
                self.blk(b).scrolloffset = 0;
                let p = self.mpplayernum;
                self.mp_selected_for_stats[p] = p;
                self.menuitem_dropdown_init(item, b);
            }
            MENUITEMTYPE_KEYBOARD => self.menuitem_keyboard_init(item, b),
            _ => {}
        }
    }

    /// `menuitem_overlay` (menuitem.c:4386).
    #[allow(clippy::too_many_arguments)]
    pub fn menuitem_overlay(&mut self, x: i32, y: i32, x2: i32, y2: i32, item: &'static MenuItem, di: usize, data: Option<usize>) {
        match item.ty {
            MENUITEMTYPE_DROPDOWN => self.menuitem_dropdown_overlay(x, y, x2, y2, item, di, data),
            MENUITEMTYPE_PLAYERSTATS => self.menuitem_dropdown_overlay(x + 1, y, -1, y2, item, di, data),
            _ => {}
        }
    }
}
