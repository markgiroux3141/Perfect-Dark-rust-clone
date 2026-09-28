//! Perfect Dark's animation decoder (`lib/anim.c`), over the raw animation bank
//! (`assets/anims/<num>.bin`, PD's own files copied verbatim, and `index.json`,
//! the `g_Anims` table entries).
//!
//! Shipping the raw bytes (typically 11–60 bytes per frame) rather than decoded
//! tables keeps this a port instead of a re-encoding: the bit reader, the per-part
//! header walk, the `u16` rotation wrap and the repeat-frame remap are PD's.
//!
//! Layout (anim.c:424): a header of per-part records — a flags byte followed by
//! `(base_hi, base_lo, bitlen)` triples for each channel present — then fixed-size
//! frames whose bits are the per-channel deltas, in part order. Parts are the
//! skeleton's *animation* part numbers (a `POSITION` node's `rodata.part`).

use std::collections::HashMap;

use glam::Vec3;
use serde::Deserialize;

use crate::assets::AssetDir;
use crate::math::baddtor;

pub const ANIMFLAG_LOOP: u32 = 0x01;
pub const ANIMFLAG_ABSOLUTETRANSLATION: u32 = 0x02;
pub const ANIMFLAG_HASREPEATFRAMES: u32 = 0x04;

const ANIMFIELD_S16_ROTATE: u8 = 0x01;
const ANIMFIELD_S16_TRANSLATE: u8 = 0x02;
const ANIMFIELD_08: u8 = 0x08;
const ANIMFIELD_F32_ROTATE: u8 = 0x10;
const ANIMFIELD_S32_TRANSLATE: u8 = 0x20;
const ANIMFIELD_CAMERA: u8 = 0x40;
const ANIMFIELD_F32_SCALE: u8 = 0x80;

/// One `g_Anims` entry (`anims/index.json`), from the decomp's animations.json.
#[derive(Deserialize, Debug, Clone)]
pub struct AnimMeta {
    /// `ANIM_*` name.
    pub id: String,
    pub numframes: u32,
    pub bytesperframe: u32,
    pub headerlen: u32,
    pub framelen: u32,
    /// `ANIMFLAG_*`.
    pub flags: u32,
}

/// One animation's bytes plus the `g_Anims` entry fields the decoder needs.
pub struct AnimData {
    pub animnum: u16,
    pub name: String,
    pub numframes: u32,
    pub bytesperframe: u32,
    pub headerlen: u32,
    /// `g_Anims[].framelen`: rotation channels are stored with `framelen` bits of
    /// precision and shifted up by `16 - framelen` (anim.c:522).
    pub framelen: u32,
    pub flags: u32,
    pub data: Vec<u8>,
}

/// `anim_read_bits` (anim.c:374): big-endian bit extraction.
fn read_bits(data: &[u8], numbits: u32, bitoffset: u32) -> u32 {
    let mut remaining = numbits;
    let mut pos = (bitoffset / 8) as usize;
    let off = bitoffset % 8;
    let mut numbitsthisbyte = 8 - off;
    let mut result: u32 = 0;
    let byte = |p: usize| -> u32 { data.get(p).copied().unwrap_or(0) as u32 };
    while remaining >= numbitsthisbyte {
        remaining -= numbitsthisbyte;
        let mask = if numbitsthisbyte >= 32 { u32::MAX } else { (1u32 << numbitsthisbyte) - 1 };
        result |= (byte(pos) & mask).wrapping_shl(remaining);
        pos += 1;
        numbitsthisbyte = 8;
    }
    if remaining > 0 {
        let mask = (1u32 << remaining) - 1;
        result |= (byte(pos) >> (numbitsthisbyte - remaining)) & mask;
    }
    result
}

/// `anim_read_signed_short` (anim.c:407): sign-extend a `readbitlen`-bit field to 16.
fn read_signed_short(data: &[u8], readbitlen: u32, bitoffset: u32) -> u16 {
    let mut result = read_bits(data, readbitlen, bitoffset) as u16;
    if readbitlen > 0 && readbitlen < 16 && (result & (1 << (readbitlen - 1))) != 0 {
        result |= (((1u32 << (16 - readbitlen)) - 1) << readbitlen) as u16;
    }
    result
}

impl AnimData {
    pub fn load(assets: &AssetDir, animnum: u16, meta: &AnimMeta) -> Result<Self, String> {
        let data = assets.read(&assets.anim(animnum))?;
        Ok(AnimData {
            animnum,
            name: meta.id.clone(),
            numframes: meta.numframes,
            bytesperframe: meta.bytesperframe,
            headerlen: meta.headerlen,
            framelen: meta.framelen,
            flags: meta.flags,
            data,
        })
    }

    pub fn looped(&self) -> bool {
        self.flags & ANIMFLAG_LOOP != 0
    }

    fn header(&self) -> &[u8] {
        let n = (self.headerlen as usize).min(self.data.len());
        &self.data[..n]
    }

    /// `anim_remap_frame_for_load` (anim.c:202). Repeated frames are not stored;
    /// the header's tail lists `(repeatfrom, repeatto)` spans to skip.
    fn remap_frame_for_load(&self, apparentframe: i32) -> i32 {
        let h = self.header();
        if h.len() < 2 {
            return apparentframe;
        }
        let mut p = h.len() as isize - 2;
        let mut result = apparentframe;
        loop {
            if p < 0 {
                break;
            }
            let repeatfrom = i16::from_be_bytes([h[p as usize], h[p as usize + 1]]) as i32;
            if repeatfrom < 0 {
                break;
            }
            if p < 2 {
                break;
            }
            let repeatto = i16::from_be_bytes([h[p as usize - 2], h[p as usize - 1]]) as i32;
            p -= 4;
            if repeatfrom <= apparentframe {
                if repeatto < apparentframe {
                    result = result - repeatto + repeatfrom - 1;
                } else {
                    result = result - apparentframe + repeatfrom;
                    break;
                }
            }
        }
        result
    }

    /// `anim_load_frame` (anim.c:280): the bytes of `framenum`, or empty when the
    /// animation stores no per-frame data (`bytesperframe == 0`).
    fn frame_bytes(&self, framenum: i32) -> &[u8] {
        if self.bytesperframe == 0 {
            return &[];
        }
        let mut f = framenum;
        if self.flags & ANIMFLAG_HASREPEATFRAMES != 0 {
            f = self.remap_frame_for_load(framenum);
        }
        let f = f.max(0) as usize;
        let start = self.headerlen as usize + self.bytesperframe as usize * f;
        let end = (start + self.bytesperframe as usize).min(self.data.len());
        if start >= end {
            return &[];
        }
        &self.data[start..end]
    }

    /// The header walk shared by both readers: returns `(bitoffset, header index of
    /// the part's flags byte)` for `part`, or `None` past the end of the header.
    fn seek_part(&self, part: usize) -> Option<(u32, usize)> {
        let h = self.header();
        let mut p = 0usize;
        let mut bitoffset = 0u32;
        let at = |i: usize| -> u32 { h.get(i).copied().unwrap_or(0) as u32 };
        for _ in 0..part {
            if p >= h.len() {
                return None;
            }
            let flags = h[p];
            p += 1;
            if flags & ANIMFIELD_08 != 0 {
                bitoffset += at(p + 2) + at(p + 5) + at(p + 8) + at(p + 11);
                p += 12;
            } else if flags & ANIMFIELD_S16_TRANSLATE != 0 {
                bitoffset += at(p + 2) + at(p + 5) + at(p + 8);
                p += 9;
            } else if flags & ANIMFIELD_S32_TRANSLATE != 0 {
                bitoffset += at(p) + at(p + 5) + at(p + 10);
                p += 15;
            }
            if flags & ANIMFIELD_S16_ROTATE != 0 {
                bitoffset += at(p + 2) + at(p + 5) + at(p + 8);
                p += 9;
            } else if flags & ANIMFIELD_F32_ROTATE != 0 {
                bitoffset += 96;
            }
            if flags & ANIMFIELD_CAMERA != 0 {
                bitoffset += at(p);
                p += 5;
            }
            if flags & ANIMFIELD_F32_SCALE != 0 {
                bitoffset += 0x60;
            }
        }
        if p < h.len() {
            Some((bitoffset, p))
        } else {
            None
        }
    }

    /// `anim_get_rot_translate_scale` (anim.c:424) unflipped. Rotations are PD
    /// euler angles in `[0, BADDTOR(360))`, applied as `Rz · Ry · Rx` by
    /// `mtx4_load_rotation`.
    pub fn rot_translate_scale(&self, part: usize, framenum: i32) -> (Vec3, Vec3, Vec3) {
        self.rot_translate_scale_flip(part, framenum, None)
    }

    /// `anim_get_rot_translate_scale` (anim.c:424). With `flip` (the model's
    /// skeleton, [`flip_part`]) the part is read through the skeleton's mirror
    /// table and its y and z rotations are mirrored; the translation is not.
    pub fn rot_translate_scale_flip(&self, part: usize, framenum: i32, flip: Option<i32>) -> (Vec3, Vec3, Vec3) {
        let zero = (Vec3::ZERO, Vec3::ZERO, Vec3::ONE);
        let part = flip.map_or(part, |skel| flip_part(skel, part));
        let Some((mut bitoffset, mut p)) = self.seek_part(part) else {
            return zero;
        };
        let h = self.header();
        let fb = self.frame_bytes(framenum);
        let at = |i: usize| -> u32 { h.get(i).copied().unwrap_or(0) as u32 };
        let flags = h[p];
        p += 1;

        let mut translate = Vec3::ZERO;
        if flags & ANIMFIELD_S16_TRANSLATE != 0 {
            let mut t = [0.0f32; 3];
            for (k, tk) in t.iter_mut().enumerate() {
                let q = p + k * 3;
                let bits = at(q + 2);
                let v = read_signed_short(fb, bits, bitoffset) as u32 + (at(q) << 8) + at(q + 1);
                *tk = (v as u16 as i16) as f32;
                bitoffset += bits;
            }
            translate = Vec3::from(t);
            p += 9;
        } else if flags & ANIMFIELD_S32_TRANSLATE != 0 {
            let mut t = [0.0f32; 3];
            for (k, tk) in t.iter_mut().enumerate() {
                let q = p + k * 5;
                let bits = at(q);
                let base = (at(q + 1) << 24) | (at(q + 2) << 16) | (at(q + 3) << 8) | at(q + 4);
                let v = read_bits(fb, bits, bitoffset).wrapping_add(base) as i32;
                *tk = v as f32 * 0.001;
                bitoffset += bits;
            }
            translate = Vec3::from(t);
            p += 15;
        } else if flags & ANIMFIELD_08 != 0 {
            bitoffset += at(p + 2) + at(p + 5) + at(p + 8) + at(p + 11);
            p += 12;
        }

        let mut rot = Vec3::ZERO;
        if flags & ANIMFIELD_S16_ROTATE != 0 {
            let shift = 16 - self.framelen.min(16);
            let mut r = [0.0f32; 3];
            for (k, rk) in r.iter_mut().enumerate() {
                let q = p + k * 3;
                let bits = at(q + 2);
                let mut introt = (read_bits(fb, bits, bitoffset) as u16).wrapping_add(((at(q) << 8) + at(q + 1)) as u16);
                introt = introt.wrapping_shl(shift);
                if flip.is_some() && k > 0 {
                    *rk = if introt != 0 { (0x10000 - introt as u32) as f32 * baddtor(360.0) / 65536.0 } else { 0.0 };
                } else {
                    *rk = introt as f32 * baddtor(360.0) / 65536.0;
                }
                bitoffset += bits;
            }
            rot = Vec3::from(r);
        } else if flags & ANIMFIELD_F32_ROTATE != 0 {
            let mut r = [0.0f32; 3];
            for rk in r.iter_mut() {
                *rk = f32::from_bits(read_bits(fb, 32, bitoffset));
                bitoffset += 32;
            }
            if flip.is_some() {
                for rk in &mut r[1..] {
                    if *rk != 0.0 {
                        *rk = baddtor(360.0) - *rk;
                    }
                }
            }
            rot = Vec3::from(r);
        }

        let mut scale = Vec3::ONE;
        if flags & ANIMFIELD_F32_SCALE != 0 {
            let mut s = [1.0f32; 3];
            for sk in s.iter_mut() {
                *sk = f32::from_bits(read_bits(fb, 32, bitoffset));
                bitoffset += 32;
            }
            scale = Vec3::from(s);
        }
        (rot, translate, scale)
    }

    /// `anim_get_pos_angle_as_int` (anim.c:615), `use_cache = false`, unflipped:
    /// the `ANIMFIELD_08` root-motion channels as raw ints plus a 16-bit turn.
    pub fn pos_angle_as_int(&self, part: usize, framenum: i32) -> ([i16; 3], u16) {
        self.pos_angle_as_int_flip(part, framenum, None)
    }

    /// [`Self::pos_angle_as_int`]; with `flip` (the skeleton) the part is read
    /// through its mirror table, and x and the turn are negated.
    pub fn pos_angle_as_int_flip(&self, part: usize, framenum: i32, flip: Option<i32>) -> ([i16; 3], u16) {
        let part = flip.map_or(part, |skel| flip_part(skel, part));
        let Some((mut bitoffset, p)) = self.seek_part(part) else {
            return ([0; 3], 0);
        };
        let h = self.header();
        let fb = self.frame_bytes(framenum);
        let at = |i: usize| -> u32 { h.get(i).copied().unwrap_or(0) as u32 };
        let mut out = [0i16; 3];
        for (k, ok) in out.iter_mut().enumerate() {
            let q = p + k * 3;
            let bits = at(q + 3);
            let v = (read_signed_short(fb, bits, bitoffset) as u32).wrapping_add(at(q + 1) * 256 + at(q + 2));
            *ok = v as u16 as i16;
            bitoffset += bits;
        }
        let bits = at(p + 12);
        let mut angle = (read_signed_short(fb, bits, bitoffset) as u32).wrapping_add(at(p + 10) * 256 + at(p + 11)) as u16;
        if flip.is_some() {
            out[0] = out[0].wrapping_neg();
            if angle != 0 {
                angle = angle.wrapping_neg();
            }
        }
        (out, angle)
    }

    /// `anim_get_translate_angle` (anim.c:702), unflipped.
    pub fn translate_angle(&self, part: usize, framenum: i32) -> (Vec3, f32) {
        self.translate_angle_flip(part, framenum, None)
    }

    /// `anim_get_translate_angle` (anim.c:702).
    pub fn translate_angle_flip(&self, part: usize, framenum: i32, flip: Option<i32>) -> (Vec3, f32) {
        let (t, a) = self.pos_angle_as_int_flip(part, framenum, flip);
        (
            Vec3::new(t[0] as f32, t[1] as f32, t[2] as f32),
            a as f32 * baddtor(360.0) / 65536.0,
        )
    }
}

/// `g_Skel0BJoints` (`modeldata/skel0b.c:6`): the head-bob model's
/// (`g_PlayerModeldef`) mirror table, `things[part][1]`.
const SKEL0B_FLIP: [u8; 15] = [0, 1, 2, 4, 3, 6, 5, 8, 7, 10, 9, 12, 11, 14, 13];

/// `skel->things[part][1]` (`anim.c:437`): the part a flipped animation reads
/// for `part`. The only model that flips in the Combat Simulator is the
/// head-bob model (`g_Skel0B`: `bhead_flip_animation` and the first-person
/// death, `bondmove.c:2090`); chrs and guns set `flip = false` on every
/// animation, so other skeletons read their own parts.
pub fn flip_part(skel: i32, part: usize) -> usize {
    match skel {
        0x0b => SKEL0B_FLIP.get(part).map_or(part, |&p| p as usize),
        _ => part,
    }
}

/// The animation bank, by PD animation number (`ANIM_*`).
#[derive(Default)]
pub struct AnimBank {
    pub anims: HashMap<u16, AnimData>,
}

impl AnimBank {
    /// The whole bank.
    pub fn load(assets: &AssetDir) -> Result<Self, String> {
        let metas: HashMap<String, AnimMeta> = assets.read_json(&assets.anim_index())?;
        let mut anims = HashMap::with_capacity(metas.len());
        for (k, meta) in &metas {
            let num = u16::from_str_radix(k, 16).map_err(|_| format!("anims/index.json: bad key {k}"))?;
            anims.insert(num, AnimData::load(assets, num, meta)?);
        }
        Ok(AnimBank { anims })
    }

    /// The animation named `ANIM_*` (e.g. `"ANIM_TWO_GUN_HOLD"`).
    pub fn by_name(&self, id: &str) -> Option<u16> {
        self.anims.iter().find(|(_, a)| a.name == id).map(|(&n, _)| n)
    }

    pub fn get(&self, animnum: u16) -> Option<&AnimData> {
        self.anims.get(&animnum)
    }

    /// `anim_get_num_frames`.
    pub fn num_frames(&self, animnum: u16) -> i32 {
        self.anims.get(&animnum).map_or(0, |a| a.numframes as i32)
    }

    pub fn flags(&self, animnum: u16) -> u32 {
        self.anims.get(&animnum).map_or(0, |a| a.flags)
    }
}
