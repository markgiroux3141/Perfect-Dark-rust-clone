//! `text.c`: PD's ROM fonts (Handel Gothic xs/sm/md/lg and the numeric font)
//! and the two text renderers, with every blend the menus switch on. The menus
//! and the HUD both draw through it into an [`n64::rdp::Gfx`].
//!
//! * `text_render_v2` (text.c:1741): glyph cores in the text colour, per
//!   character through `text_get_colour_at_pos` (the blends); when a focused
//!   item turns the shadow on, `text_render_v1` first draws the glowing halo
//!   (NTSC: the v1 renderer in the focus colour, `text.c:1795`).
//! * The blends (`struct blendsettings`, text.c:24): the diagonal "redraw"
//!   sweep a dialog's text is revealed by (three modes), the menu fade, the
//!   wave shimmer, and the horizontal fade the marquee uses.
//! * The hologram ray a redraw sweep casts from each character it passes
//!   (`g_TextHoloRayEnabled`, text.c:1614) is queued on [`TextState::holorays`];
//!   PD records those into a display list that the RDP runs *before* the
//!   dialogs (`text_enable_holo_ray`), so the menu composites them underneath.
//!
//! Glyphs are CI4, 16 texels a row, `height + 2` rows with the halo border,
//! looked up through the IA16 TLUTs `var8007fb3c` / `var8007fb5c`.
//!
//! Source: the old repo's `pd_menu/text.rs` (the superset). Replaces the gun
//! spike's `pd_guns/font.rs`, whose HUD text moves here in M4.

use n64::rdp::{rgba, Blend, Gfx};

use crate::assets::AssetDir;

pub const SPACE_WIDTH: i32 = 5;

const BLENDTYPE_DIAGONAL: u8 = 0x01;
const BLENDTYPE_VERTICAL: u8 = 0x02;
const BLENDTYPE_WAVE: u8 = 0x04;
const BLENDTYPE_MENU: u8 = 0x08;
const BLENDTYPE_HORIZONTAL: u8 = 0x10;

/// `DIAGMODE_*` (constants.h): 0 redraw, 1 fade in, 2 fade out.
pub const DIAGMODE_REDRAW: u8 = 0;
pub const DIAGMODE_FADEIN: u8 = 1;
pub const DIAGMODE_FADEOUT: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontId {
    Xs,
    Sm,
    Md,
    Lg,
    /// `g_FontNumeric`: the HUD's ammo digits.
    Numeric,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FontChar {
    pub index: u8,
    pub baseline: i32,
    pub height: i32,
    pub width: i32,
    pub kerningindex: i32,
    pixeldata: usize,
}

pub struct Font {
    pub kerning: Vec<i32>,
    pub chars: Vec<FontChar>,
    data: Vec<u8>,
}

/// `var8007fb3c`: the v2 TLUT's alpha bytes (intensity is always 0xff).
const TLUT_V2: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0x24, 0x48, 0x6c, 0x90, 0xb4, 0xd8, 0xff];
/// `var8007fb5c`: palette 0 (TEXEL0, the halo mask) and 1 (TEXEL1, the core).
const TLUT_V1_0: [u8; 16] = [0, 0x58, 0x74, 0x90, 0xac, 0xc8, 0xe4, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
const TLUT_V1_1: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0x18, 0x30, 0x5c, 0x88, 0xb4, 0xd8, 0xff];

impl Font {
    /// `text_load_font` (text.c:184): 13×13 kerning then 94 chars.
    pub fn load(assets: &AssetDir, name: &str) -> Result<Font, String> {
        let path = assets.font(name);
        let data = assets.read(&path)?;
        if data.len() < 676 + 94 * 12 {
            return Err(format!("{}: too short", path.display()));
        }
        let be32 = |o: usize| i32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let kerning = (0..169).map(|i| be32(i * 4)).collect();
        let chars = (0..94)
            .map(|i| {
                let o = 676 + i * 12;
                FontChar {
                    index: data[o],
                    baseline: data[o + 1] as i8 as i32,
                    height: data[o + 2] as i32,
                    width: data[o + 3] as i32,
                    kerningindex: be32(o + 4),
                    pixeldata: be32(o + 8) as u32 as usize,
                }
            })
            .collect();
        Ok(Font { kerning, chars, data })
    }

    /// `&chars[c - 0x21]`, with `text_parse_char`'s range clamp to '!'.
    pub fn ch(&self, c: u8) -> &FontChar {
        let c = if (0x21..=0x7e).contains(&c) { c } else { b'!' };
        &self.chars[(c - 0x21) as usize]
    }

    fn texel(&self, fc: &FontChar, s: i32, t: i32) -> usize {
        if !(0..16).contains(&s) || t < 0 || t >= fc.height + 2 {
            return 0;
        }
        let o = fc.pixeldata + t as usize * 8 + s as usize / 2;
        let Some(&b) = self.data.get(o) else { return 0 };
        (if s % 2 == 0 { b >> 4 } else { b & 15 }) as usize
    }

    fn kern(&self, prev: &FontChar, cur: &FontChar) -> i32 {
        self.kerning.get((prev.kerningindex * 13 + cur.kerningindex) as usize).copied().unwrap_or(0)
    }

    /// `chars['['].height + chars['['].baseline` — PD indexes the array directly
    /// with the character, so this is char 0x7c's metrics.
    pub fn lineheight(&self) -> i32 {
        let c = &self.chars[b'[' as usize];
        c.height + c.baseline
    }
}

pub struct Fonts {
    pub xs: Font,
    pub sm: Font,
    pub md: Font,
    pub lg: Font,
    pub numeric: Font,
}

impl Fonts {
    pub fn load(assets: &AssetDir) -> Result<Fonts, String> {
        Ok(Fonts {
            xs: Font::load(assets, "handelgothicxs")?,
            sm: Font::load(assets, "handelgothicsm")?,
            md: Font::load(assets, "handelgothicmd")?,
            lg: Font::load(assets, "handelgothiclg")?,
            numeric: Font::load(assets, "numeric")?,
        })
    }

    pub fn get(&self, f: FontId) -> &Font {
        match f {
            FontId::Xs => &self.xs,
            FontId::Sm => &self.sm,
            FontId::Md => &self.md,
            FontId::Lg => &self.lg,
            FontId::Numeric => &self.numeric,
        }
    }
}

/// `colour_blend` (game_006900.c:33).
pub fn colour_blend(a: u32, b: u32, aweight: u32) -> u32 {
    let aweight = aweight & 0xff;
    let bweight = 0xff - aweight;
    let ch = |s: u32| (((aweight * ((a >> s) & 0xff) + bweight * ((b >> s) & 0xff)) >> 8) & 0xff) << s;
    ch(24) | ch(16) | ch(8) | ch(0)
}

/// `struct blendsettings` (text.c:24).
#[derive(Clone, Copy, Debug, Default)]
pub struct BlendSettings {
    pub types: u8,
    pub colour04: u32,
    pub colour08: u32,
    pub diagrefx: i32,
    pub diagrefy: i32,
    pub diagtimer: f32,
    pub diagmode: u8,
    pub backupdiagrefx: i32,
    pub backupdiagrefy: i32,
    pub backupdiagtimer: f32,
    pub backupdiagmode: u8,
    pub backupdiagtypes: u8,
    pub backuptypes: u8,
    pub vertrefy1: i32,
    pub vertrefy2: i32,
    pub vert34: i32,
    pub horizrefx1: i32,
    pub horizrefx2: i32,
    pub horiz40: i32,
    pub colour44: u32,
    pub colour48: u32,
    pub wave4c: i32,
    pub wave50: i32,
    pub wave54: i32,
    pub wavecolour1: u32,
    pub wavecolour2: u32,
    pub menuweight: u32,
}

/// A queued `ortho_draw_holoray` onto `g_TextHoloRayGdl`.
#[derive(Clone, Copy, Debug)]
pub struct HolorayReq {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub colour1: u32,
    pub colour2: u32,
    pub plane: i32,
    pub miny: i32,
    pub maxy: i32,
    pub fromx: i32,
    pub fromy: i32,
}

/// The text renderer's globals (text.c:68-140).
pub struct TextState {
    pub blend: BlendSettings,
    pub holoray_enabled: bool,
    pub blend_distance: i32,
    pub last_blend_y: i32,
    pub shadow_enabled: bool,
    /// `var800a463c`.
    pub shadow_colour: u32,
    pub rotated90: bool,
    pub lalpha: u32,
    pub lfade: u32,
    pub llimbo: u32,
    pub subtlety: u32,
    pub subtletx: u32,
    /// `g_TextHoloRayGdl`.
    pub holorays: Vec<HolorayReq>,
    /// `g_HolorayMinY` / `MaxY` / `ProjectFromX` / `Y` (savebuffer.c:22).
    pub holoray_miny: i32,
    pub holoray_maxy: i32,
    pub holoray_fromx: i32,
    pub holoray_fromy: i32,
}

impl Default for TextState {
    fn default() -> Self {
        TextState {
            blend: BlendSettings::default(),
            holoray_enabled: false,
            blend_distance: 0,
            last_blend_y: -1,
            shadow_enabled: false,
            shadow_colour: 0,
            rotated90: false,
            lalpha: 1,
            lfade: 100,
            llimbo: 44,
            subtlety: 128,
            subtletx: 60,
            holorays: Vec::new(),
            holoray_miny: -1000,
            holoray_maxy: 1000,
            holoray_fromx: 0,
            holoray_fromy: 0,
        }
    }
}

impl TextState {
    pub fn set_diagonal_blend(&mut self, x: i32, y: i32, timer: f32, mode: u8) {
        self.blend.types |= BLENDTYPE_DIAGONAL;
        self.blend.diagrefx = x;
        self.blend.diagrefy = y;
        self.blend.diagtimer = timer;
        self.blend.diagmode = mode;
    }
    pub fn backup_diagonal_blend_settings(&mut self) {
        let b = &mut self.blend;
        b.backupdiagrefx = b.diagrefx;
        b.backupdiagrefy = b.diagrefy;
        b.backupdiagtimer = b.diagtimer;
        b.backupdiagmode = b.diagmode;
        b.backupdiagtypes = b.types & BLENDTYPE_DIAGONAL;
    }
    pub fn restore_diagonal_blend_settings(&mut self) {
        let b = &mut self.blend;
        b.diagrefx = b.backupdiagrefx;
        b.diagrefy = b.backupdiagrefy;
        b.diagtimer = b.backupdiagtimer;
        b.diagmode = b.backupdiagmode;
        b.types |= b.backupdiagtypes;
    }
    pub fn set_horizontal_blend(&mut self, x1: i32, x2: i32, arg2: i32) {
        self.blend.types |= BLENDTYPE_HORIZONTAL;
        self.blend.horizrefx1 = x1;
        self.blend.horizrefx2 = x2;
        self.blend.horiz40 = arg2;
    }
    pub fn backup_and_reset_blends(&mut self) {
        self.blend.backuptypes = self.blend.types;
        self.blend.types = 0;
    }
    pub fn restore_blends(&mut self) {
        self.blend.types = self.blend.backuptypes;
    }
    /// `text_set_wave_blend` (text.c:590).
    pub fn set_wave_blend(&mut self, a: i32, b: i32, cthresh: i32) {
        self.blend.types |= BLENDTYPE_WAVE;
        self.blend.wave4c = a;
        self.blend.wave50 = b;
        self.blend.wave54 = cthresh;
        self.blend.wavecolour1 = 0x44444400;
        self.blend.wavecolour2 = 0xffffff00;
    }
    /// `text_set_menu_blend` (text.c:600).
    pub fn set_menu_blend(&mut self, f: f32) {
        self.blend.types |= BLENDTYPE_MENU;
        self.blend.menuweight = (f * f * 110.0) as u32;
    }
    pub fn set_wave_colours(&mut self, c1: u32, c2: u32) {
        self.blend.wavecolour1 = c1;
        self.blend.wavecolour2 = c2;
    }
    pub fn reset_blends(&mut self) {
        self.blend.types = 0;
    }
    /// `text_has_diagonal_blend` (text.c:617).
    pub fn has_diagonal_blend(&self) -> bool {
        self.blend.types & BLENDTYPE_DIAGONAL != 0 && (self.blend.diagmode == DIAGMODE_FADEIN || self.blend.diagmode == DIAGMODE_FADEOUT)
    }

    fn diag_dist(&self, x: i32, y: i32) -> f32 {
        let (dx, dy) = (x - self.blend.diagrefx, y - self.blend.diagrefy);
        if dx > -3000 && dx < 3000 && dy > -3000 && dy < 3000 {
            ((dx * dx + dy * dy) as f32).sqrt()
        } else {
            3000.0
        }
    }

    /// `text_apply_projection_colour` (text.c:623).
    pub fn apply_projection_colour(&self, x: i32, y: i32, colour: u32) -> u32 {
        let mut result = colour;
        if self.blend.types & BLENDTYPE_DIAGONAL != 0 {
            let f12 = self.diag_dist(x, y);
            let f14 = self.lalpha as f32;
            let f18 = self.lfade as f32;
            let mut f16 = self.llimbo as f32;
            let t = self.blend.diagtimer;
            if self.blend.diagmode == 0 {
                if t < f12 {
                    result = 0;
                } else if t - f14 < f12 {
                    let weightf = (f12 - (t - f14)) / f14 * 255.0;
                    let intensity = 255u32.wrapping_sub(weightf as u32) & 0xff;
                    result = intensity << 8 | intensity | intensity << 16 | intensity << 24;
                } else if t - (f14 + f16) < f12 {
                    result = (((colour & 0xff) + 0xff) >> 1) | (colour & 0xffffff00);
                } else if t - (f14 + f18 + f16) < f12 {
                    let colour2 = (((colour & 0xff) + 0xff) / 2) | (colour & 0xffffff00);
                    let weightf = (f12 - (t - (f14 + f18 + f16))) / f18 * 255.0;
                    result = colour_blend(colour, colour2, 0xff - weightf as u32);
                }
            } else if self.blend.diagmode == 2 {
                f16 = 0.0;
                if t < f12 {
                    result = 0;
                } else if t - f14 < f12 {
                    let weightf = (f12 - (t - f14)) / f14 * 255.0;
                    result = colour_blend(0, colour & 0xff, weightf as u32);
                } else if t - (f14 + f16) < f12 {
                    result = colour & 0xff;
                } else if t - (f14 + f18 + f16) < f12 {
                    let weightf = (f12 - (t - (f14 + f18 + f16))) / f18 * 255.0;
                    result = colour_blend(colour & 0xff, colour, weightf as u32);
                }
            }
        }
        result
    }

    /// `text_get_colour_at_pos` (text.c:681), with `frac20 = g_20SecIntervalFrac`.
    pub fn get_colour_at_pos(&self, x: i32, y: i32, colourarg: u32) -> u32 {
        let b = &self.blend;
        let mut colour = colourarg;
        if b.types & BLENDTYPE_MENU != 0 {
            colour = (colour_blend(0, colour, b.menuweight) & 0xffffff00) | (colour & 0xff);
        }
        if b.types & BLENDTYPE_VERTICAL != 0 {
            let mut v0 = (y - b.vertrefy1).abs();
            let v1 = (y - b.vertrefy2).abs();
            if v1 < v0 {
                v0 = v1;
            }
            if b.vert34 >= v0 {
                colour = colour_blend(colour, 0, (v0 * 255 / b.vert34.max(1)) as u32);
            }
        }
        if b.types & BLENDTYPE_HORIZONTAL != 0 {
            let mut v0 = x - b.horizrefx1;
            let mut v1 = x - b.horizrefx2;
            if v0 < 0 {
                v0 = 0;
            }
            if v1 < 0 {
                v1 = -v1;
            }
            if v1 < v0 {
                v0 = v1;
            }
            if b.horiz40 >= v0 {
                colour = colour_blend(colour, 0, (v0 * 255 / b.horiz40.max(1)) as u32);
            }
        }
        if b.types & BLENDTYPE_DIAGONAL != 0 {
            let f12 = self.diag_dist(x, y);
            let mut f14 = self.lalpha as f32;
            let mut f18 = self.lfade as f32;
            let mut f16 = self.llimbo as f32;
            let t = b.diagtimer;
            if b.diagmode == 0 {
                if t < f12 {
                    colour = 0;
                } else if t - f14 < f12 {
                    let weightf = (f12 - (t - f14)) / f14 * 255.0;
                    let intensity = 255u32.wrapping_sub(weightf as u32) & 0xff;
                    colour = intensity << 8 | intensity | intensity << 16 | intensity << 24;
                } else if t - (f14 + f16) < f12 {
                    colour = 0xffffffff;
                } else if t - (f14 + f18 + f16) < f12 {
                    let weightf = (f12 - (t - (f14 + f18 + f16))) / f18 * 255.0;
                    let add = (weightf as u32) * 255;
                    let mult = 255 - weightf as u32;
                    let ch = |s: u32| ((((colour >> s) & 0xff) * mult + add) >> 8) & 0xff;
                    colour = ch(24) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0);
                }
            } else if b.diagmode == 2 {
                f14 = 0.0;
                f18 = 66.0;
                f16 = 0.0;
                if t < f12 {
                    colour = 0;
                } else if t - f14 < f12 {
                    let weightf = (f12 - (t - f14)) / f14 * 255.0;
                    colour = colour_blend(0, colour & 0xff, weightf as u32);
                } else if t - (f14 + f16) < f12 {
                    colour &= 0xff;
                } else if t - (f14 + f18 + f16) < f12 {
                    let weightf = (f12 - (t - (f14 + f18 + f16))) / f18 * 255.0;
                    colour = colour_blend(0, colour, weightf as u32);
                }
            } else {
                let alpha0 = colour & 0xff;
                f18 = 50.0;
                f16 = 22.0;
                let burncol: u32 = 0xffffff00;
                if t < f12 {
                    colour = colour_blend(alpha0, colour, 110);
                } else if t - f14 < f12 {
                    let weightf = (f12 - (t - f14)) / f14 * 255.0;
                    colour = colour_blend(colour_blend(burncol | (colour & 0xff), colour, 0xc0), colour_blend(alpha0, colour, 110), 255u32.wrapping_sub(weightf as u32));
                } else if t - (f14 + f16) < f12 {
                    colour = colour_blend(burncol | (colour & 0xff), colour, 0xc0);
                } else if t - (f14 + f18 + f16) < f12 {
                    let weightf = (f12 - (t - (f14 + f18 + f16))) / f18 * 255.0;
                    colour = colour_blend(colour, colour_blend(burncol | (colour & 0xff), colour, 0xc0), 255u32.wrapping_sub(weightf as u32));
                }
            }
        }
        if b.types & BLENDTYPE_WAVE != 0 {
            let mut f0 = (b.wave4c - x + b.wave50 - y + 800) as f32;
            f0 = 4.0 * f0 / b.wave54 as f32;
            f0 -= ((f0 * 0.25) as i32) as f32 * 4.0;
            f0 -= 1.0;
            if f0 > 1.0 {
                f0 = 2.0 - f0;
            }
            if f0 < 0.0 {
                let weight = (self.subtletx as f32 * -f0) as u32;
                colour = colour_blend(b.wavecolour1 | (colour & 0xff), colour, weight);
            } else {
                let weight = (self.subtlety as f32 * f0) as u32;
                colour = colour_blend(b.wavecolour2 | (colour & 0xff), colour, weight);
            }
        }
        colour
    }

    /// `text_calculate_blend_distance` (text.c:511).
    fn calculate_blend_distance(&mut self, y: i32) {
        if y != self.last_blend_y {
            let d = (y - self.blend.diagrefy) as f32;
            let sqdist = self.blend.diagtimer * self.blend.diagtimer - d * d;
            self.blend_distance = if sqdist > 0.0 { self.blend.diagrefx + sqdist.sqrt() as i32 } else { 0 };
            self.last_blend_y = y;
        }
    }

    fn queue_holoray(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, c1: u32, c2: u32, plane: i32) {
        let r = HolorayReq {
            x1,
            y1,
            x2,
            y2,
            colour1: c1,
            colour2: c2,
            plane,
            miny: self.holoray_miny,
            maxy: self.holoray_maxy,
            fromx: self.holoray_fromx,
            fromy: self.holoray_fromy,
        };
        self.holorays.push(r);
    }

    /// `ortho_draw_holoray` onto `g_TextHoloRayGdl` (queued; see the module doc).
    pub fn text_holoray(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, c1: u32, c2: u32, plane: i32) {
        self.queue_holoray(x1, y1, x2, y2, c1, c2, plane);
    }
}

/// Byte → the ASCII the fonts cover (`text_latin1_to_ascii` for the few
/// accented characters in English text is a no-op: none appear).
fn bytes(text: &str) -> Vec<u8> {
    text.chars().map(|c| if (c as u32) < 256 { c as u32 as u8 } else { b'?' }).collect()
}

/// `text_measure` (text.c:2256, NTSC): (height, width).
pub fn measure(f: &Font, text: &str, lineheight: i32) -> (i32, i32) {
    let lineheight = if lineheight == 0 { f.lineheight() } else { lineheight };
    let (mut h, mut w, mut longest) = (0, 0, 0);
    let mut prev = b'H';
    let b = bytes(text);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            0 => break,
            b' ' => {
                if b.get(i + 1) != Some(&b'\n') {
                    w += SPACE_WIDTH;
                }
                prev = b'H';
            }
            b'\n' => {
                longest = longest.max(w);
                w = 0;
                h += lineheight;
            }
            c => {
                let tmp = f.kern(f.ch(prev), f.ch(c)) - 1;
                w += f.ch(c).width - tmp;
                prev = c;
            }
        }
        i += 1;
    }
    (h, w.max(longest))
}

/// `TIMEPRECISION_*` (`constants.h:4395`).
pub const TIMEPRECISION_DAYS: usize = 0;
pub const TIMEPRECISION_HOURS: usize = 1;
pub const TIMEPRECISION_MINUTES: usize = 2;
pub const TIMEPRECISION_SECONDS: usize = 3;
pub const TIMEPRECISION_HUNDREDTHS: usize = 4;

/// `format_time` (`savebuffer.c:610`): `time60` as days:hours:minutes:seconds
/// :hundredths down to `precision`, leading zero parts left out (minutes on
/// are always shown): `"4:07"`, `"1:02:03"`.
pub fn format_time(time60: i32, precision: usize) -> String {
    let mut parts = [0i32; 5];
    parts[4] = time60 % 60 * 100 / 60;
    parts[3] = time60 / 60;
    parts[2] = parts[3] / 60;
    parts[1] = parts[2] / 60;
    parts[0] = parts[1] / 24;
    parts[3] %= 60;
    parts[2] %= 60;
    parts[1] %= 24;
    let mut out = String::new();
    let mut donefirst = false;
    for (i, &p) in parts.iter().enumerate().take(precision + 1) {
        if donefirst {
            out.push_str(&format!(":{p:02}"));
        } else if p != 0 || i >= TIMEPRECISION_MINUTES {
            out.push_str(&p.to_string());
            donefirst = true;
        }
    }
    out
}

/// `text_wrap` (text.c:2397, NTSC).
pub fn wrap(wrapwidth: i32, src: &str, f: &Font) -> String {
    let src = bytes(src);
    let mut dst = Vec::new();
    let mut curlinewidth = 0;
    let mut p = 0usize;
    let at = |p: usize| src.get(p).copied().unwrap_or(0);
    loop {
        let mut word = Vec::new();
        while at(p) > b' ' {
            word.push(at(p));
            p += 1;
        }
        let wordstr: String = word.iter().map(|&b| b as char).collect();
        let (_, wordwidth) = measure(f, &wordstr, 0);
        curlinewidth += wordwidth;
        let itfits = curlinewidth <= wrapwidth;
        match at(p) {
            b'\n' => {
                if !itfits {
                    dst.push(b'\n');
                }
                curlinewidth = 0;
                dst.extend_from_slice(&word);
                dst.push(b'\n');
            }
            b' ' => {
                if !itfits {
                    dst.push(b'\n');
                    curlinewidth = wordwidth;
                }
                curlinewidth += SPACE_WIDTH;
                dst.extend_from_slice(&word);
                dst.push(b' ');
            }
            0 => {
                if !itfits {
                    dst.push(b'\n');
                }
                dst.extend_from_slice(&word);
                break;
            }
            _ => {
                // Control characters other than '\n' end the word too.
                dst.extend_from_slice(&word);
            }
        }
        p += 1;
    }
    dst.iter().map(|&b| b as char).collect()
}

/// A text call's borrow of the renderer's pieces.
pub struct TextCtx<'a> {
    pub gfx: &'a mut Gfx,
    pub ts: &'a mut TextState,
    pub fonts: &'a Fonts,
    /// `g_20SecIntervalFrac` (for the shadow's oscillation).
    pub frac20: f32,
}

impl<'a> TextCtx<'a> {
    /// `menu_get_sin_osc_frac` (game_006900.c:94).
    fn sin_osc(&self, freq: f32) -> f32 {
        ((freq * self.frac20 + freq * self.frac20) * std::f32::consts::PI).sin() / 2.0 + 0.5
    }

    /// `text_render_v2` (text.c:1741, NTSC): renders `text` at (*x, *y), leaving
    /// them where PD does (after the last glyph / line).
    pub fn render_v2(&mut self, x: &mut i32, y: &mut i32, text: &str, font: FontId, textcolour: u32, width: i32, height: i32, shadowoffset: i32, lineheight: i32) {
        let uiscale = self.gfx.uiscale;
        let scalex = 1;
        if self.ts.rotated90 {
            *y *= uiscale;
        } else {
            *x *= uiscale;
        }
        if self.ts.shadow_enabled {
            let alpha = (1.0 - self.sin_osc(40.0)) * 100.0 + 150.0;
            let mut shadowx = *x / uiscale;
            let mut shadowy = *y;
            let shadowcolour = self.ts.shadow_colour;
            let textcolourforshadow = (textcolour & 0xffffff00) | (alpha as u32 & 0xff);
            self.render_v1(&mut shadowx, &mut shadowy, text, font, textcolourforshadow, shadowcolour, width, height, shadowoffset, lineheight);
        }
        let f = self.fonts.get(font);
        let savedx = *x;
        let savedy = *y;
        let mut prev = b'H';
        let lineheight = if lineheight == 0 { f.lineheight() } else { lineheight };
        self.ts.blend.colour04 = textcolour;
        self.ts.blend.colour44 = textcolour;
        for c in bytes(text) {
            match c {
                0 => break,
                b' ' => {
                    prev = b'H';
                    *x += scalex * SPACE_WIDTH;
                }
                b'\n' => {
                    prev = b'H';
                    *y += lineheight;
                    *x = savedx;
                }
                c => {
                    let cur = *f.ch(c);
                    let prevc = *f.ch(prev);
                    self.render_char_v2(x, y, &cur, &prevc, f, savedx, savedy, width, height, shadowoffset);
                    prev = c;
                }
            }
        }
        if self.ts.rotated90 {
            *y /= uiscale;
        } else {
            *x /= uiscale;
        }
    }

    /// `text_render_char_v2` (text.c:1453, NTSC).
    #[allow(clippy::too_many_arguments)]
    fn render_char_v2(&mut self, x: &mut i32, y: &mut i32, cur: &FontChar, prev: &FontChar, f: &Font, savedx: i32, savedy: i32, width: i32, height: i32, shadowoffset: i32) {
        let xscale = 1;
        let uiscale = self.gfx.uiscale;
        let sp90 = *y + shadowoffset;
        let tmp = f.kern(prev, cur);
        *x -= (tmp - 1) * xscale;
        let width = width * xscale;
        let (vw, vh) = (self.gfx.w as i32, self.gfx.h as i32);
        if self.ts.rotated90 || (*x > 0 && *x <= vw && sp90 + cur.baseline <= vh) {
            if savedx + width >= *x && savedy + height >= cur.baseline + sp90 && *x >= savedx && cur.baseline + sp90 + cur.height >= savedy {
                let mut colour = self.ts.blend.colour04;
                if self.ts.blend.types != 0 {
                    colour = self.ts.get_colour_at_pos(*x / uiscale, *y + shadowoffset, self.ts.blend.colour04);
                    self.ts.blend.colour44 = colour;
                }
                if *x + xscale * cur.width <= savedx + width {
                    if savedy <= cur.baseline + sp90 {
                        if cur.baseline + sp90 + cur.height <= savedy + height {
                            if self.ts.rotated90 {
                                // gSPTextureRectangleFlip: the glyph turned 90°
                                // (x ← y, y ← x), drawn up the screen.
                                self.glyph_rot90(f, cur, sp90 - cur.baseline - cur.height, *x, colour);
                            } else {
                                self.glyph(f, cur, *x, sp90 + cur.baseline, 0, cur.height, colour);
                                if self.ts.holoray_enabled {
                                    self.ts.calculate_blend_distance(*y + shadowoffset);
                                    let bd = self.ts.blend_distance;
                                    let c04 = self.ts.blend.colour04;
                                    if bd >= *x / uiscale && *x / uiscale + cur.width >= bd {
                                        self.ts.text_holoray(bd, cur.baseline + sp90, bd, cur.baseline + sp90 + cur.height, c04, c04, 0);
                                    }
                                    if bd - 3 >= *x / uiscale && *x / uiscale + cur.width >= bd - 3 {
                                        self.ts.text_holoray(bd, cur.baseline + sp90, bd, cur.baseline + sp90 + cur.height, c04, c04, 0);
                                    }
                                }
                            }
                        } else if savedy + height >= cur.baseline + sp90 {
                            // Clipped at the bottom.
                            let rows = savedy + height - (sp90 + cur.baseline);
                            self.glyph(f, cur, *x, sp90 + cur.baseline, 0, rows, colour);
                        }
                    } else if cur.baseline + sp90 + cur.height >= savedy {
                        // Clipped at the top: start at savedy, t offset.
                        let skip = savedy - sp90 - cur.baseline;
                        self.glyph(f, cur, *x, savedy, skip, cur.baseline + sp90 + cur.height - savedy, colour);
                    }
                }
            }
        }
        *x += cur.width * xscale;
    }

    /// One glyph core as `gSPTextureRectangle` with `G_CC` (PRIM, TEXEL0·PRIM),
    /// s/t from 1 (the halo border is skipped). `t0` = first glyph row.
    fn glyph(&mut self, f: &Font, fc: &FontChar, x: i32, y: i32, t0: i32, rows: i32, colour: u32) {
        let c = rgba(colour);
        for t in 0..rows.max(0) {
            for s in 0..fc.width {
                let a = TLUT_V2[f.texel(fc, s + 1, t + t0 + 1)] as f32 / 255.0;
                if a > 0.0 {
                    self.gfx.blend_px(x + s, y + t, [c[0], c[1], c[2], a * c[3]], Blend::Xlu);
                }
            }
        }
    }

    /// `gSPTextureRectangleFlip` for the rotated sibling-dialog titles
    /// (dialog_render): screen x runs along glyph rows, screen y along columns.
    fn glyph_rot90(&mut self, f: &Font, fc: &FontChar, sx: i32, sy: i32, colour: u32) {
        let c = rgba(colour);
        for t in 0..fc.height {
            for s in 0..fc.width {
                let a = TLUT_V2[f.texel(fc, s + 1, (fc.height - 1 - t) + 1)] as f32 / 255.0;
                if a > 0.0 {
                    self.gfx.blend_px(sx + t, sy + s, [c[0], c[1], c[2], a * c[3]], Blend::Xlu);
                }
            }
        }
    }

    /// `text_render_v1` (text.c:2150, NTSC): each glyph's (w+2)×(h+2) block,
    /// colour = lerp(glow, text, TEXEL1), alpha = TEXEL0 × text alpha.
    #[allow(clippy::too_many_arguments)]
    pub fn render_v1(&mut self, x: &mut i32, y: &mut i32, text: &str, font: FontId, textcolour: u32, glowcolour: u32, width: i32, height: i32, shadowoffset: i32, lineheight: i32) {
        let uiscale = self.gfx.uiscale;
        let f = self.fonts.get(font);
        *x *= uiscale;
        let savedx = *x;
        let savedy = *y;
        let mut prev = b'H';
        let lineheight = if lineheight == 0 { f.lineheight() } else { lineheight };
        self.ts.blend.colour08 = glowcolour;
        self.ts.blend.colour48 = glowcolour;
        self.ts.blend.colour04 = textcolour;
        self.ts.blend.colour44 = textcolour;
        let (vw, vh) = (self.gfx.w as i32, self.gfx.h as i32);
        for c in bytes(text) {
            match c {
                0 => break,
                b' ' => {
                    *x += SPACE_WIDTH;
                    prev = b'H';
                }
                b'\n' => {
                    *x = savedx;
                    *y += lineheight;
                    prev = b'H';
                }
                c => {
                    let cur = *f.ch(c);
                    let prevc = *f.ch(prev);
                    // text_render_char_v1 (text.c:1988)
                    let sp38 = *y + shadowoffset;
                    *x -= f.kern(&prevc, &cur) - 1;
                    if *x > 0
                        && *x <= vw
                        && sp38 + cur.baseline <= vh
                        && *x <= savedx + width
                        && cur.baseline + sp38 <= savedy + height
                        && *x >= savedx
                        && sp38 + cur.baseline + cur.height >= savedy
                    {
                        let (mut tc, mut gc) = (self.ts.blend.colour04, self.ts.blend.colour08);
                        if self.ts.blend.types != 0 {
                            // text_configure_colours_v1 (text.c:1967)
                            tc = self.ts.get_colour_at_pos(*x / uiscale, *y + shadowoffset, self.ts.blend.colour04);
                            gc = (self.ts.blend.colour08 & 0xffffff00) | (self.ts.get_colour_at_pos(*x / uiscale, *y + shadowoffset, self.ts.blend.colour08) & 0xff);
                        }
                        self.glyph_v1(f, &cur, *x - 1, sp38 - 1, savedx, savedy - 1, width, height, tc, gc);
                    }
                    *x += cur.width;
                    prev = c;
                }
            }
        }
        *x /= uiscale;
    }

    /// `text_render_char_v1_part2` (text.c:2029): the 2-cycle combine
    /// `lerp(PRIM=glow, ENV=text, TEXEL1_ALPHA)`, alpha `TEXEL0 × ENV`.
    #[allow(clippy::too_many_arguments)]
    fn glyph_v1(&mut self, f: &Font, fc: &FontChar, x: i32, y: i32, left: i32, top: i32, width: i32, height: i32, textcolour: u32, glowcolour: u32) {
        if left + width < fc.width + x + 2 {
            return;
        }
        let (t0, t1) = if y + fc.baseline >= top {
            if top + height >= y + fc.baseline + fc.height + 2 {
                (0, fc.height + 2)
            } else if top + height >= y + fc.baseline {
                (0, top + height - (y + fc.baseline))
            } else {
                return;
            }
        } else if y + fc.baseline + fc.height + 2 >= top {
            (top - fc.baseline - y, fc.height + 2)
        } else {
            return;
        };
        let (trgb, ta) = (rgba(textcolour), rgba(textcolour)[3]);
        let grgb = rgba(glowcolour);
        for t in t0.max(0)..t1 {
            for s in 0..fc.width + 2 {
                let idx = f.texel(fc, s, t);
                let a0 = TLUT_V1_0[idx] as f32 / 255.0;
                if a0 <= 0.0 {
                    continue;
                }
                let a1 = TLUT_V1_1[idx] as f32 / 255.0;
                let rgb = [0, 1, 2].map(|k| (trgb[k] - grgb[k]) * a1 + grgb[k]);
                self.gfx.blend_px(x + s, y + fc.baseline + t, [rgb[0], rgb[1], rgb[2], a0 * ta], Blend::Xlu);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts::load(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))).expect("assets/fonts: run tools/pd-assets/build_assets.py")
    }

    /// From the gun spike: the ROM fonts decode (`text_load_font`), and the
    /// numeric '8' is 3×5, drawn by `text_render_v1` as a green core in a dark halo.
    #[test]
    fn hud_fonts_decode_and_draw_a_haloed_digit() {
        let fonts = fonts();
        let eight = fonts.numeric.ch(b'8');
        assert_eq!((eight.width, eight.height), (3, 5));
        let mut gfx = Gfx::new(32, 16);
        gfx.push_layer();
        let mut ts = TextState::default();
        let mut ctx = TextCtx { gfx: &mut gfx, ts: &mut ts, fonts: &fonts, frac20: 0.0 };
        let (mut x, mut y) = (4, 4);
        let lh = fonts.numeric.lineheight();
        ctx.render_v1(&mut x, &mut y, "8", FontId::Numeric, 0x00ff00a0, 0x000000a0, 32, 16, 0, lh);
        let lit: Vec<_> = gfx.fb.iter().filter(|p| p[3] > 0.0).collect();
        // The layer is premultiplied: a green core has green over alpha near 1.
        assert!(lit.iter().any(|p| p[1] / p[3] > 0.5 && p[0] < 0.1), "green core");
        assert!(lit.iter().any(|p| p[1] / p[3] < 0.05 && p[3] > 0.3), "dark halo");
        assert!(x > 4, "the pen moved past the glyph");
    }

    #[test]
    fn format_time_leaves_out_leading_zero_parts() {
        assert_eq!(format_time(0, TIMEPRECISION_SECONDS), "0:00");
        assert_eq!(format_time((4 * 60 + 7) * 60 + 30, TIMEPRECISION_SECONDS), "4:07");
        assert_eq!(format_time((62 * 60 + 3) * 60, TIMEPRECISION_SECONDS), "1:02:03");
        assert_eq!(format_time(90, TIMEPRECISION_HUNDREDTHS), "0:01:50");
    }

    #[test]
    fn measure_and_wrap_follow_the_fonts_metrics() {
        let fonts = fonts();
        let xs = &fonts.xs;
        let (h, w) = measure(xs, "Falcon 2\n", 0);
        assert!(w > 20 && h == xs.lineheight(), "{w}×{h}");
        assert_eq!(measure(xs, "", 0), (0, 0));
        let (_, one) = measure(xs, "Combat Simulator", 0);
        let wrapped = wrap(one / 2, "Combat Simulator", xs);
        assert!(wrapped.contains('\n'), "{wrapped:?}");
        // PD keeps the space before the newline it inserts ("Combat \nSimulator").
        assert_eq!(wrapped.split_whitespace().collect::<Vec<_>>(), ["Combat", "Simulator"]);
    }
}
