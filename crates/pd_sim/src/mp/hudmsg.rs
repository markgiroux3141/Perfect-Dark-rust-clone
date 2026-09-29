//! HUD messages (`hudmsg.c`): the lines that slide in at the bottom left of a
//! player's view ("Killed Sim 1", "Kill count: 3", "One minute left."), their
//! queue, placement, timers and the swish. Only the in-match types are used;
//! the drawing is `pd_render::hud`'s (`hudmsgs_render`).
//!
//! The sim measures and wraps the text with the fonts' metrics because the
//! size decides the placement, the fade times and whether a queued message
//! may show yet.

use pd_core::ids::*;
use pd_core::text::{measure, wrap, FontId};

use crate::world::World;

/// `struct hudmsgtype` (`hudmsg.c:54`): `unk00` boxed, `unk01` allowfadein,
/// `unk02` flash, the font, `colour` (text) and `unk10` (glow), the
/// alignments with their margins (`unk16`, `unk18`), the duration (ticks).
#[derive(Clone, Copy, Debug)]
pub struct HudMsgType {
    pub boxed: bool,
    pub allowfadein: bool,
    pub flash: bool,
    pub font: FontId,
    pub colour: u32,
    pub glow: u32,
    pub alignh: u8,
    pub xmargin: i32,
    pub alignv: u8,
    pub ymargin: i32,
    pub duration: i32,
}

const fn ty(boxed: bool, allowfadein: bool, flash: bool, font: FontId, colour: u32, glow: u32, alignh: u8, alignv: u8, duration: i32) -> HudMsgType {
    HudMsgType { boxed, allowfadein, flash, font, colour, glow, alignh, xmargin: 0, alignv, ymargin: 0, duration }
}

/// `g_HudmsgTypes` (`hudmsg.c:54`, NTSC).
pub const G_HUDMSG_TYPES: [HudMsgType; 12] = [
    ty(true, true, false, FontId::Sm, 0x00ff0000, 0x000000a0, HUDMSGALIGN_LEFT, HUDMSGALIGN_BOTTOM, 80),
    ty(false, true, false, FontId::Md, 0x00ff0000, 0x000000a0, HUDMSGALIGN_XMIDDLE, HUDMSGALIGN_YMIDDLE, 120),
    ty(false, false, true, FontId::Md, 0xff000000, 0xffffffa0, HUDMSGALIGN_XMIDDLE, HUDMSGALIGN_YMIDDLE, 120),
    ty(false, true, false, FontId::Md, 0x00ff0000, 0x000000a0, HUDMSGALIGN_LEFT, HUDMSGALIGN_BOTTOM, 120),
    ty(true, true, false, FontId::Sm, 0x00ffc000, 0x000000a0, HUDMSGALIGN_LEFT, HUDMSGALIGN_BOTTOM, 40),
    ty(false, false, false, FontId::Md, 0x00ff0000, 0x000000a0, HUDMSGALIGN_LEFT, HUDMSGALIGN_TOP, 120),
    ty(true, false, false, FontId::Sm, 0x00ff0000, 0x000000a0, HUDMSGALIGN_XMIDDLE, HUDMSGALIGN_TOP, 120),
    ty(true, true, false, FontId::Sm, 0x00ff0000, 0x000000a0, HUDMSGALIGN_XMIDDLE, HUDMSGALIGN_TOP, -1),
    ty(true, true, false, FontId::Sm, 0x00ffc000, 0x000000a0, HUDMSGALIGN_XMIDDLE, HUDMSGALIGN_BOTTOM, 500),
    ty(true, true, false, FontId::Xs, 0x00ff0000, 0x000000a0, HUDMSGALIGN_LEFT, HUDMSGALIGN_BOTTOM, 120),
    ty(true, true, false, FontId::Sm, 0x00ff0000, 0x000000a0, HUDMSGALIGN_LEFT, HUDMSGALIGN_BOTTOM, 240),
    ty(false, false, false, FontId::Sm, 0x00ff0000, 0x000000a0, HUDMSGALIGN_XMIDDLE, HUDMSGALIGN_BELOWVIEWPORT, 120),
];

/// `struct hudmessage` (`types.h:4617`).
#[derive(Clone, Debug)]
pub struct HudMessage {
    pub state: u8,
    pub boxed: bool,
    pub allowfadein: bool,
    pub flash: bool,
    pub opacity: u8,
    pub timer: i32,
    pub font: FontId,
    pub textcolour: u32,
    pub glowcolour: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub text: String,
    /// The audio channel a subtitle follows (-1: none, the in-match types).
    pub channelnum: i32,
    pub ty: usize,
    pub id: i32,
    pub showduration: i32,
    pub playernum: usize,
    pub flags: u32,
    pub alignh: u8,
    pub alignv: u8,
    pub xmarginextra: i32,
    pub xmargin: i32,
    pub ymargin: i32,
    pub hash: i32,
}

/// `g_HudMessages`: 20 in a match (`hudmsgs_reset`, `hudmsg.c:419`).
#[derive(Clone, Debug)]
pub struct HudMsgs {
    pub msgs: Vec<HudMessage>,
    /// `g_NextHudMessageId`.
    pub next_id: i32,
}

impl Default for HudMessage {
    fn default() -> Self {
        HudMessage {
            state: HUDMSGSTATE_FREE,
            boxed: false,
            allowfadein: false,
            flash: false,
            opacity: 0,
            timer: 0,
            font: FontId::Sm,
            textcolour: 0,
            glowcolour: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            text: String::new(),
            channelnum: -1,
            ty: 0,
            id: 0,
            showduration: 0,
            playernum: 0,
            flags: 0,
            alignh: 0,
            alignv: 0,
            xmarginextra: 0,
            xmargin: 0,
            ymargin: 0,
            hash: 0,
        }
    }
}

impl Default for HudMsgs {
    fn default() -> Self {
        HudMsgs { msgs: vec![HudMessage::default(); 20], next_id: 0 }
    }
}

impl HudMsgs {
    /// `hudmsg_get_next` (`hudmsg.c:446`): the message with the smallest id
    /// above `refid`.
    fn hudmsg_get_next(&self, refid: i32) -> Option<usize> {
        let mut best: Option<(i32, usize)> = None;
        for (i, m) in self.msgs.iter().enumerate() {
            if m.state != HUDMSGSTATE_FREE && m.id > refid && best.is_none_or(|(id, _)| m.id < id) {
                best = Some((m.id, i));
            }
        }
        best.map(|(_, i)| i)
    }
}

/// The fade in's length (`hudmsgs_tick`, `hudmsg.c:1223`) and the fade out's
/// (`:1263`), ticks.
pub fn hudmsg_fadein_time(m: &HudMessage) -> f32 {
    (((m.width * m.width + m.height * m.height) as f32).sqrt() + 132.0) / 7.0
}

pub fn hudmsg_fadeout_time(m: &HudMessage) -> f32 {
    (((m.width * m.width + m.height * m.height) as f32).sqrt() + 92.0) / 7.0
}

impl World {
    /// Player `pi`'s view: left, top, width, height (`viewleft`, ...).
    fn hudmsg_view(&self, pi: usize) -> (i32, i32, i32, i32) {
        self.players.get(pi).map_or((0, 0, 320, 220), |p| (p.cam.c_screenleft as i32, p.cam.c_screentop as i32, p.viewwidth as i32, p.viewheight as i32))
    }

    /// `hudmsg0f0ddb1c` (`hudmsg.c:350`, NTSC): the wrap width, and the left
    /// margin a message gets.
    fn hudmsg_wrap_width(&self, pi: usize, xmargin: i32) -> (i32, i32) {
        let (_, _, viewwidth, _) = self.hudmsg_view(pi);
        let mut extra = 24;
        let mut result = 0;
        let playercount = self.players.len();
        if playercount == 2 && self.setup_screensplit_vertical() {
            result -= extra * 2 / 3;
            extra = if pi == 0 { extra / 3 } else { extra / 6 };
        }
        result = result + viewwidth - extra - xmargin - 11;
        if playercount == 1 {
            result -= 16;
        }
        (result, extra)
    }

    /// Two players side by side (`options_get_screen_split`).
    fn setup_screensplit_vertical(&self) -> bool {
        self.setup.screensplit == pd_core::ids::SCREENSPLIT_VERTICAL
    }

    /// `hudmsg_calculate_position` (`hudmsg.c:828`, NTSC 1.0+).
    fn hudmsg_calculate_position(&self, m: &mut HudMessage) {
        let (mut viewleft, viewtop, mut viewwidth, viewheight) = self.hudmsg_view(m.playernum);
        let playercount = self.players.len();
        let offset = if m.alignh == HUDMSGALIGN_XMIDDLE { 10 } else { 0 };
        if playercount >= 3 {
            viewwidth -= offset;
            if m.playernum == 0 || m.playernum == 2 {
                viewleft += offset;
            }
        }
        let vertical = self.setup_screensplit_vertical();
        if playercount == 2 && vertical {
            viewwidth -= offset;
            if m.playernum == 0 {
                viewleft += offset;
            }
        }
        let x = match m.alignh {
            HUDMSGALIGN_SCREENLEFT => m.xmargin,
            HUDMSGALIGN_LEFT => {
                // (No cutscenes in a match: the margin is xmarginextra.)
                let mut x = viewleft + m.xmarginextra + m.xmargin + 3;
                if playercount == 2 && vertical {
                    x += if m.playernum == 0 { 15 } else { 4 };
                } else if playercount >= 3 {
                    x += if m.playernum.is_multiple_of(2) { -1 } else { -16 };
                }
                x
            }
            HUDMSGALIGN_RIGHT => viewleft + viewwidth - m.width - m.xmargin - 57,
            HUDMSGALIGN_XMIDDLE => (viewwidth - m.width) / 2 + viewleft + m.xmargin,
            _ => m.xmargin,
        };
        let y = match m.alignv {
            HUDMSGALIGN_SCREENTOP => m.ymargin,
            HUDMSGALIGN_TOP => viewtop + m.ymargin + 13,
            HUDMSGALIGN_BOTTOM => {
                let mut y = viewtop + viewheight - m.height - m.ymargin - 14;
                if playercount == 2 {
                    y += if !vertical && m.playernum == 0 { 8 } else { 3 };
                } else if playercount >= 3 {
                    y += if m.playernum <= 1 { 8 } else { 3 };
                }
                // (options_get_effective_screen_size is SCREENSIZE_FULL.)
                y
            }
            HUDMSGALIGN_YMIDDLE => (viewheight - m.height) / 2 + viewtop + m.ymargin,
            HUDMSGALIGN_BELOWVIEWPORT => viewtop + viewheight - m.height / 2 + 18,
            _ => m.ymargin,
        };
        m.x = x;
        m.y = y;
    }

    /// `hudmsg_create` (`hudmsg.c:465`) for player `pi`.
    pub(crate) fn hudmsg_create(&mut self, pi: usize, text: &str, ty: usize) {
        self.hudmsg_create_with_flags(pi, text, ty, 0);
    }

    /// `hudmsg_create_with_flags` (`hudmsg.c:482`) → `hudmsg_create_from_args`
    /// (`hudmsg.c:935`) with the type's settings: unless it duplicates a
    /// message the player already has, into the first free slot, wrapped to
    /// the view, queued.
    pub(crate) fn hudmsg_create_with_flags(&mut self, pi: usize, text: &str, ty: usize, flags: u32) {
        let conf = G_HUDMSG_TYPES[ty];
        let hash: i32 = text.bytes().map(|b| b as i32).sum();
        if flags & HUDMSGFLAG_ONLYIFALIVE != 0 && self.players.get(pi).is_some_and(|p| p.isdead) {
            return;
        }
        if flags & HUDMSGFLAG_ALLOWDUPES == 0 && self.mp.hudmsgs.msgs.iter().any(|m| m.state != HUDMSGSTATE_FREE && m.state != HUDMSGSTATE_FADINGOUT && m.playernum == pi && m.hash == hash) {
            return;
        }
        // (Only the objective and subtitle types may push out a queued one.)
        let Some(index) = self.mp.hudmsgs.msgs.iter().position(|m| m.state == HUDMSGSTATE_FREE) else { return };
        let (wrapwidth, xmarginextra) = self.hudmsg_wrap_width(pi, conf.xmargin);
        let font = self.res.fonts.get(conf.font);
        let (mut textheight, mut textwidth) = measure(font, text, 0);
        let msgtext = if textwidth > wrapwidth {
            let mut stacktext: String = text.chars().take(400).filter(|&c| c != '\n').collect();
            stacktext.push('\n');
            let wrapped = wrap(wrapwidth, &stacktext, font);
            (textheight, textwidth) = measure(font, &wrapped, 0);
            wrapped
        } else {
            text.chars().take(399).collect()
        };
        let id = self.mp.hudmsgs.next_id;
        self.mp.hudmsgs.next_id += 1;
        let mut m = HudMessage {
            state: HUDMSGSTATE_QUEUED,
            boxed: conf.boxed,
            allowfadein: conf.allowfadein,
            flash: conf.flash,
            opacity: 0,
            timer: 0,
            font: conf.font,
            textcolour: conf.colour,
            glowcolour: conf.glow,
            x: 0,
            y: 0,
            width: textwidth,
            height: textheight,
            text: msgtext,
            // hudmsg_create passes -1: no channel.
            channelnum: -1,
            ty,
            id,
            showduration: conf.duration,
            playernum: pi,
            flags,
            alignh: conf.alignh,
            alignv: conf.alignv,
            xmarginextra,
            xmargin: conf.xmargin,
            ymargin: conf.ymargin,
            hash,
        };
        self.hudmsg_calculate_position(&mut m);
        self.mp.hudmsgs.msgs[index] = m;
    }

    /// `hudmsgs_remove_for_dead_player` (`hudmsg.c:1341`).
    pub(crate) fn hudmsgs_remove_for_dead_player(&mut self, pi: usize) {
        for m in self.mp.hudmsgs.msgs.iter_mut() {
            if m.state != HUDMSGSTATE_FREE && m.playernum == pi && m.flags & HUDMSGFLAG_ONLYIFALIVE != 0 {
                m.state = HUDMSGSTATE_FREE;
                m.timer = 0;
            }
        }
    }

    /// `hudmsgs_tick` (`hudmsg.c:1099`, from `lv_tick`): every message placed
    /// again, then in id order: a queued one shows once its player lives and
    /// its box is clear (a boxed one of its type fading out there makes way);
    /// the fade in (with the swish, one player only), the stay, the fade out.
    pub(crate) fn hudmsgs_tick(&mut self) {
        let mut msgs = std::mem::take(&mut self.mp.hudmsgs.msgs);
        for m in msgs.iter_mut().filter(|m| m.state != HUDMSGSTATE_FREE) {
            self.hudmsg_calculate_position(m);
        }
        self.mp.hudmsgs.msgs = msgs;
        let lv60 = self.lv.lvupdate60;
        let paused = self.mp_is_paused();
        let mut previd = -1;
        while let Some(index) = self.mp.hudmsgs.hudmsg_get_next(previd) {
            previd = self.mp.hudmsgs.msgs[index].id;
            // No audio channel: fully opaque.
            self.mp.hudmsgs.msgs[index].opacity = 0xff;
            match self.mp.hudmsgs.msgs[index].state {
                HUDMSGSTATE_QUEUED => {
                    let m = &self.mp.hudmsgs.msgs[index];
                    if m.flags & HUDMSGFLAG_DELAY != 0 {
                        let m = &mut self.mp.hudmsgs.msgs[index];
                        m.timer += 1;
                        if m.timer > 3 {
                            m.flags &= !HUDMSGFLAG_DELAY;
                        }
                        continue;
                    }
                    let mut show = !self.players.get(m.playernum).is_some_and(|p| p.isdead);
                    if show {
                        // Check if any other message is occupying our space
                        let (mx, my, mw, mh, mty, mboxed) = (m.x, m.y, m.width, m.height, m.ty, m.boxed);
                        for i in 0..self.mp.hudmsgs.msgs.len() {
                            let o = &self.mp.hudmsgs.msgs[i];
                            if o.state != HUDMSGSTATE_FREE && o.state != HUDMSGSTATE_QUEUED && o.x + o.width >= mx && o.x <= mx + mw && o.y + o.height >= my && o.y <= my + mh {
                                show = false;
                                // Consider booting the previous message out earlier
                                if o.ty == mty && mboxed && o.boxed && o.state == HUDMSGSTATE_FADINGOUT {
                                    let o = &mut self.mp.hudmsgs.msgs[i];
                                    o.state = HUDMSGSTATE_FREE;
                                    o.timer = 0;
                                    let m = &mut self.mp.hudmsgs.msgs[index];
                                    m.state = HUDMSGSTATE_FADINGIN;
                                    m.timer = 0;
                                }
                                break;
                            }
                        }
                    }
                    if show {
                        let m = &mut self.mp.hudmsgs.msgs[index];
                        m.state = if m.boxed {
                            HUDMSGSTATE_CHOOSETRANSITION
                        } else if m.allowfadein {
                            HUDMSGSTATE_FADINGIN
                        } else {
                            HUDMSGSTATE_ONSCREEN
                        };
                        m.timer = 0;
                    }
                }
                HUDMSGSTATE_CHOOSETRANSITION => {
                    let m = &mut self.mp.hudmsgs.msgs[index];
                    m.state = if m.boxed && m.allowfadein { HUDMSGSTATE_FADINGIN } else { HUDMSGSTATE_ONSCREEN };
                    m.timer = 0;
                }
                HUDMSGSTATE_FADINGIN => {
                    // Most HUD messages play a swish sound effect
                    if self.mp.hudmsgs.msgs[index].timer == 0 && !paused && self.players.len() == 1 {
                        self.sound(0x003e, 1.0);
                    }
                    let m = &mut self.mp.hudmsgs.msgs[index];
                    let fadeintime = hudmsg_fadein_time(m);
                    m.timer += lv60;
                    if m.timer >= fadeintime as i32 {
                        m.state = HUDMSGSTATE_ONSCREEN;
                        m.timer = 0;
                    }
                }
                HUDMSGSTATE_ONSCREEN => {
                    let m = &mut self.mp.hudmsgs.msgs[index];
                    m.timer += lv60;
                    if m.timer >= m.showduration && m.showduration != -1 {
                        m.state = if m.boxed { HUDMSGSTATE_FADINGOUT } else { HUDMSGSTATE_FREE };
                        m.timer = 0;
                    }
                }
                HUDMSGSTATE_FADINGOUT => {
                    let m = &mut self.mp.hudmsgs.msgs[index];
                    let fadeouttime = hudmsg_fadeout_time(m);
                    m.timer += lv60;
                    if m.timer >= fadeouttime as i32 {
                        m.state = HUDMSGSTATE_FREE;
                        m.timer = 0;
                    }
                }
                _ => {}
            }
        }
    }
}
