//! What a player sees besides the scene: the Farsight's x-ray
//! (`VISIONMODE_XRAY`: `bgun_tick_gameplay2`'s vision half, `bondgun.c:7989`,
//! and the eraser sphere `bg_tick_portals_xray` places, `bg.c:5222`), and the
//! framebuffer effects `lv_render` draws over the view each frame (`lv.c:1439`
//! -`:1520`): the Slayer rocket's interlace and static, the x-ray's smear, and
//! the Combat Boost's wipe. The sim works them out; `pd_render::post` draws them.
//!
//! Source: the old repo's `pd_guns/xray.rs` (the eraser; its colours are
//! `pd_render::xray`'s) and `pd_guns/sim.rs` (`xray_tick`, `lv_render_boost`).

use glam::Vec3;
use pd_core::ids::*;

use crate::world::World;

/// The player's eraser: `eraserpos`, `eraserdepth`, `eraserpropdist`,
/// `eraserbgdist`, `erasertime`, and the colour shifts `ecol_1..3` / `epcol_0..2`.
#[derive(Clone, Copy, Debug)]
pub struct Eraser {
    pub pos: Vec3,
    pub depth: f32,
    pub propdist: f32,
    pub bgdist: f32,
    /// Quarter-ticks since x-ray came on.
    pub time: i32,
    /// Bit shifts into an RGBA8888 word (24 red, 16 green, 8 blue).
    pub ecol: [u32; 3],
    /// Channel indexes into an object's colour.
    pub epcol: [usize; 3],
}

impl Default for Eraser {
    fn default() -> Eraser {
        Eraser { pos: Vec3::ZERO, depth: -500.0, propdist: 400.0, bgdist: 400.0, time: 0, ecol: [16, 24, 8], epcol: [0, 1, 2] }
    }
}

/// One frame's framebuffer effects for a player's view, in `lv_render`'s order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewFx {
    /// `bview_draw_slayer_rocket_interlace`: the rocket camera's scanlines.
    pub slayer_interlace: bool,
    /// `bview_draw_static(0x4fffffff, alpha)`: 0 draws none.
    pub static_alpha: u32,
    /// `bview_draw_zoom_blur(0xffffffff, alpha, sx, sy)` calls: the x-ray's
    /// smear, then the boost's wipe.
    pub zoom_blurs: Vec<(u32, f32, f32)>,
    /// `player_draw_fade(r, g, b, frac)`: the boost's white.
    pub fade: Option<([u8; 3], f32)>,
    /// `bview_set_motion_blur(bluramount)`: the dizziness' smear, 0 for none,
    /// else 100..230.
    pub motion_blur: i32,
}

impl World {
    /// The vision half of `bgun_tick_gameplay2` (`bondgun.c:7989`): aiming the
    /// Farsight (`gunsightoff == 0`) turns x-ray on with its colours; anything
    /// else turns it off, unless riding a Slayer rocket. (No x-ray scanner
    /// device in the Combat Simulator.)
    pub(crate) fn bgun_tick_vision(&mut self, pi: usize) {
        let lv240 = self.lv.lvupdate240;
        let p = &mut self.players[pi];
        let gunsighton = p.insightaimmode;
        if gunsighton && p.gun.hands[HAND_RIGHT].weaponnum == WEAPON_FARSIGHT {
            if p.visionmode != VISIONMODE_XRAY {
                p.eraser.time = 0;
            } else {
                p.eraser.time += lv240;
            }
            p.visionmode = VISIONMODE_XRAY;
            p.eraser.ecol = [16, 24, 8];
            p.eraser.epcol = [0, 1, 2];
        } else if p.visionmode != VISIONMODE_SLAYERROCKET {
            p.visionmode = VISIONMODE_NORMAL;
        }
    }

    /// `bg_tick_portals_xray`'s eraser (`bg.c:5253`): 5 m in front of the eye,
    /// pushed out by the zoom while the Farsight is aimed; the stage's reach.
    pub(crate) fn bg_tick_eraser(&mut self, pi: usize) {
        let (propdist, extra) = (self.stage.eraserpropdist, self.stage.eraserbgextra);
        let p = &mut self.players[pi];
        let aiming = p.gun.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_FARSIGHT && p.insightaimmode;
        p.eraser.depth = if aiming { -500.0 / p.cam.c_lodscalez } else { -500.0 };
        p.eraser.pos = p.cam.projection.transform_point3(Vec3::new(0.0, 0.0, p.eraser.depth));
        p.eraser.propdist = propdist;
        p.eraser.bgdist = propdist + extra;
    }

    /// `lv_render`'s framebuffer effects for player `pi` (`lv.c:1439`): the
    /// Slayer's interlace and its static (out of bounds, or once when the signal
    /// is lost), the x-ray's zoom blur, and the boost's wipe, which only the
    /// first player's pass advances.
    /// `lv_render`'s blur (`lv.c:1110`), at the start of each player's pass:
    /// the player chr's `blurdrugamount` sets the motion blur and wears off.
    pub(crate) fn lv_render_blur(&mut self, pi: usize) -> i32 {
        let lv60 = self.lv.lvupdate60;
        let c = &mut self.chrs[pi];
        let mut bluramount = 0;
        if c.blurdrugamount > 0 {
            bluramount = (c.blurdrugamount * 130 / 5000 + 100).min(230);
            c.blurdrugamount = c.blurdrugamount.min(5000);
            c.blurdrugamount -= lv60 * (c.blurnumtimesdied + 1);
            if c.blurdrugamount < 1 {
                c.blurdrugamount = 0;
                c.blurnumtimesdied = 0;
            }
        }
        bluramount
    }

    pub(crate) fn lv_render_fx(&mut self, pi: usize, motion_blur: i32) {
        let mut fx = ViewFx { motion_blur, ..ViewFx::default() };
        {
            let p = &mut self.players[pi];
            if p.visionmode == VISIONMODE_SLAYERROCKET {
                fx.slayer_interlace = true;
                if p.badrockettime > 0 {
                    fx.static_alpha = ((p.badrockettime * 255 / 90) as u32).min(255);
                }
            }
            if p.visionmode == VISIONMODE_SLAYERROCKETSTATIC {
                fx.static_alpha = 255;
                p.visionmode = VISIONMODE_NORMAL;
            }
            if p.visionmode == VISIONMODE_XRAY {
                let xraything = if p.eraser.time < 200 { 249 - ((p.eraser.time * 3) >> 2) } else { 99 };
                fx.zoom_blurs.push((xraything as u32, 1.05, 1.05));
            }
        }
        // Combat boosts.
        let sp = &mut self.speedpill;
        let mut groan = None;
        if (sp.change > 0 && sp.change < 30) || (sp.want && !sp.on) || (!sp.want && sp.on) {
            if sp.change == 30 && !sp.want {
                groan = Some(if self.setup.slowmotion() != pd_core::lv::SlowMotion::Off { 0x05c9 } else { 0x02ad });
            }
            let k = if sp.change < 15 { sp.change } else { 30 - sp.change };
            fx.zoom_blurs.push(((k * 180 / 15) as u32, k as f32 * 0.020_000_001 + 1.1, k as f32 * 0.020_000_001 + 1.1));
            fx.fade = Some(([0xff, 0xff, 0xff], k as f32 * 0.006_666_667));
            if pi == 0 {
                if sp.want {
                    sp.change += 1;
                } else {
                    sp.change -= 1;
                }
            }
            sp.change = sp.change.clamp(0, 30);
        }
        sp.on = sp.change > 15;
        if let Some(sound) = groan {
            self.sound(sound, 1.0);
        }
        self.players[pi].viewfx = fx;
    }
}
