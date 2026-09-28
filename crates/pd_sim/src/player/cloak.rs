//! The RC-P120's cloak on a player (`chr.c:2043`-`:2262`, `bondgun.c:8050`):
//! `chr_cloak`, `chr_uncloak`, `chr_uncloak_temporarily` (a shot or a throw
//! drops it for two seconds), `chr_update_cloak`'s player branch and the fade
//! (`cloakfadefrac` → `chr_get_cloak_alpha`), and the RC-P120's ammo drain,
//! 0.4 rounds a tick once the fade has finished.
//!
//! Source: the old repo's `pd_guns/sim.rs` (`ChrCloak`, `chr_update_cloak`,
//! `rcp120_cloak_tick`).

use pd_core::ids::*;

use crate::world::World;

/// A chr's cloak state: `CHRHFLAG_CLOAKED`, `cloakfadefrac`,
/// `cloakfadefinished`, `cloakpause`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChrCloak {
    pub cloaked: bool,
    pub fadefrac: i32,
    pub fadefinished: bool,
    pub pause: i32,
}

impl ChrCloak {
    /// `chr_get_cloak_alpha` (`chr.c:2244`): 255 visible; fading, 255 down to 1;
    /// fully cloaked, a slow shimmer between 1 and 20.
    pub fn alpha(&self) -> i32 {
        let mut alpha = 255;
        if self.fadefrac > 0 || self.fadefinished {
            if !self.fadefinished {
                alpha = 255 - self.fadefrac * 2;
            } else {
                let f = ((self.fadefrac as f32 / 127.0 + self.fadefrac as f32 / 127.0) * pd_core::math::dtor(180.0)).cos();
                alpha = ((1.0 - f) * 20.0 * 0.5) as i32;
            }
            if alpha == 0 {
                alpha = 1;
            }
        }
        alpha
    }

    /// The cloakfade half of `chr_update_cloak` (`chr.c:2209`).
    fn update_fade(&mut self, lvupdate240: i32, lvupdate60: i32) {
        if self.cloaked {
            if !self.fadefinished {
                let fadefrac = self.fadefrac + (lvupdate240 * 5) / 8;
                if fadefrac >= 128 {
                    self.fadefinished = true;
                    self.fadefrac = 0;
                } else {
                    self.fadefrac = fadefrac;
                }
            } else {
                self.fadefrac = (self.fadefrac + lvupdate60) % 127;
            }
        } else {
            if self.fadefinished {
                self.fadefinished = false;
                let f = 1.0 - ((self.fadefrac as f32 / 127.0 + self.fadefrac as f32 / 127.0) * pd_core::math::dtor(180.0)).cos();
                self.fadefrac = (254 - (f * 20.0 * 0.5) as i32) / 2;
            }
            if self.fadefrac > 0 {
                self.fadefrac = (self.fadefrac - (lvupdate240 * 5) / 8).max(0);
            }
        }
    }
}

impl World {
    /// `chr_cloak(chr, true)` (`chr.c:2043`). M7: not while dead.
    fn chr_cloak(&mut self, pi: usize) {
        self.players[pi].cloak.cloaked = true;
        let pos = self.players[pi].pos;
        self.sound_at(0x005b, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
    }

    /// `chr_uncloak(chr, value)` (`chr.c:2054`): `value` plays the sound.
    pub(crate) fn chr_uncloak(&mut self, pi: usize, value: bool) {
        if self.players[pi].cloak.cloaked {
            self.players[pi].cloak.cloaked = false;
            if value {
                let pos = self.players[pi].pos;
                self.sound_at(0x005c, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
            }
        }
    }

    /// `chr_uncloak_temporarily` (`chr.c:2082`): off, and not back for 120 ticks.
    pub(crate) fn chr_uncloak_temporarily(&mut self, pi: usize) {
        self.chr_uncloak(pi, true);
        self.players[pi].cloak.pause = 120;
    }

    /// `chr_update_cloak` (`chr.c:2088`), the player branch. (The cloaking
    /// device's own toggle, `DEVICE_CLOAKDEVICE`, is not ported: it is a locked
    /// pickup, left for later.)
    pub(crate) fn chr_update_cloak(&mut self, pi: usize) {
        let (lv240, lv60) = (self.lv.lvupdate240, self.lv.lvupdate60);
        {
            let c = &mut self.players[pi].cloak;
            if c.pause > 0 {
                c.pause -= lv60;
                if c.pause < 1 {
                    c.pause = 0;
                }
            }
        }
        let p = &self.players[pi];
        let enabled = p.devicesactive & DEVICE_CLOAKDEVICE != 0 || (p.gun.ctrl.weaponnum == WEAPON_RCP120 && p.devicesactive & DEVICE_CLOAKRCP120 != 0);
        if enabled {
            if !p.cloak.cloaked && p.cloak.pause < 1 {
                self.chr_cloak(pi);
            }
        } else if p.devicesactive & DEVICE_CLOAKDEVICE == 0 && p.cloak.cloaked {
            self.chr_uncloak(pi, true);
        }
        self.players[pi].cloak.update_fade(lv240, lv60);
    }

    /// The RC-P120 half of `bgun_tick_gameplay2` (`bondgun.c:8050`): fully
    /// cloaked, the cloak eats 0.4 rounds a tick from the clip and turns off once
    /// the clip and the reserve can't pay; another gun turns it off; a remainder
    /// left when it went off is still paid.
    pub(crate) fn rcp120_cloak_tick(&mut self, pi: usize) {
        let lv60 = self.lv.lvupdate60freal;
        let p = &mut self.players[pi];
        let ammotype = p.gun.ctrl.ammotypes[0];
        if p.devicesactive & DEVICE_CLOAKRCP120 != 0 {
            if p.gun.ctrl.weaponnum == WEAPON_RCP120 {
                if p.cloak.cloaked && p.cloak.fadefinished {
                    let hand = &mut p.gun.hands[HAND_RIGHT];
                    hand.matmot1 += lv60 * 0.4;
                    if hand.matmot1 > 1.0 {
                        let usedqty = (hand.matmot1 as i32).min(hand.loadedammo[0]);
                        hand.matmot1 -= usedqty as f32;
                        hand.loadedammo[0] -= usedqty;
                        if hand.loadedammo[0] == 0 && hand.state != HANDSTATE_RELOAD {
                            let stilltogo = hand.matmot1 as i32;
                            if stilltogo > p.gun.ammoheld(ammotype) {
                                p.devicesactive &= !DEVICE_CLOAKRCP120;
                            }
                        }
                    }
                }
            } else {
                p.devicesactive &= !DEVICE_CLOAKRCP120;
            }
        } else if p.gun.ctrl.weaponnum == WEAPON_RCP120 {
            let hand = &mut p.gun.hands[HAND_RIGHT];
            if hand.matmot1 > 1.0 {
                let usedqty = (hand.matmot1 as i32).min(hand.loadedammo[0]);
                hand.matmot1 -= usedqty as f32;
                hand.loadedammo[0] -= usedqty;
                if hand.matmot1 > 1.0 {
                    let rest = hand.matmot1 as i32;
                    hand.matmot1 = 0.0;
                    let usedqty = rest.min(p.gun.ammoheld(ammotype));
                    if ammotype >= 0 {
                        p.gun.p.ammoheldarr[ammotype as usize] -= usedqty;
                    }
                }
            }
        }
    }
}
