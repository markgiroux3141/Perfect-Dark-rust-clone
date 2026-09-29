//! Cloaks (`chr.c:2043`-`:2262`, `bondgun.c:8050`): `chr_cloak`,
//! `chr_uncloak`, `chr_uncloak_temporarily` (a shot or a throw drops it for two
//! seconds), `chr_update_cloak` (a simulant's cloaking device or RC-P120, a
//! player's device or RC-P120, then the fade, `cloakfadefrac` →
//! `chr_get_cloak_alpha`), and the player's RC-P120 drain, 0.4 rounds a tick
//! once the fade has finished. A player's state is [`crate::player::Player`]'s
//! (copied to its chr each tick), a simulant's its chr's.
//!
//! Source: the old repo's `pd_guns/sim.rs` (`ChrCloak`, `chr_update_cloak`,
//! `rcp120_cloak_tick`), for the player's branch.

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

impl ChrCloak {
    /// `chr_cloak(chr, value)` (`chr.c:2043`) on the state: not while dead.
    /// True if it went on.
    fn cloak(&mut self, dead: bool) -> bool {
        !dead && !std::mem::replace(&mut self.cloaked, true)
    }

    /// `chr_uncloak` (`chr.c:2054`) on the state: true if it went off.
    fn uncloak(&mut self) -> bool {
        std::mem::replace(&mut self.cloaked, false)
    }

    /// The pause countdown that opens `chr_update_cloak` (`chr.c:2096`).
    fn tick_pause(&mut self, lvupdate60: i32) {
        if self.pause > 0 {
            self.pause -= lvupdate60;
            if self.pause < 1 {
                self.pause = 0;
            }
        }
    }
}

impl World {
    /// `chr_cloak(chr, true)` (`chr.c:2043`) for player `pi`.
    fn chr_cloak(&mut self, pi: usize) {
        let dead = self.players[pi].isdead;
        if self.players[pi].cloak.cloak(dead) {
            let pos = self.players[pi].pos;
            self.sound_at(0x005b, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
        }
    }

    /// `chr_uncloak(chr, value)` (`chr.c:2054`) for player `pi`: `value` plays
    /// the sound.
    pub(crate) fn chr_uncloak(&mut self, pi: usize, value: bool) {
        if self.players[pi].cloak.uncloak() && value {
            let pos = self.players[pi].pos;
            self.sound_at(0x005c, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
        }
    }

    /// `chr_uncloak_temporarily` (`chr.c:2082`) for player `pi`: off, and not
    /// back for 120 ticks.
    pub(crate) fn chr_uncloak_temporarily(&mut self, pi: usize) {
        self.chr_uncloak(pi, true);
        self.players[pi].cloak.pause = 120;
    }

    /// `chr_uncloak(chr, value)` (`chr.c:2054`) for chr `i`.
    pub(crate) fn chr_uncloak_chr(&mut self, i: usize, value: bool) {
        if let Some(p) = self.chrs[i].player {
            self.chr_uncloak(p, value);
            self.chrs[i].cloak = self.players[p].cloak;
            return;
        }
        if self.chrs[i].cloak.uncloak() && value {
            let pos = self.chrs[i].pos;
            self.sound_at(0x005c, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
        }
    }

    /// `chr_uncloak_temporarily` (`chr.c:2082`) for chr `i`.
    pub(crate) fn chr_uncloak_temporarily_chr(&mut self, i: usize) {
        if let Some(p) = self.chrs[i].player {
            self.chr_uncloak_temporarily(p);
            self.chrs[i].cloak = self.players[p].cloak;
            return;
        }
        self.chr_uncloak_chr(i, true);
        self.chrs[i].cloak.pause = 120;
    }

    /// `chr_update_cloak` (`chr.c:2088`), the player branch: the cloaking
    /// device eats its `AMMOTYPE_CLOAK` reserve a tick at a time while it
    /// hides the player, and turns off when it runs out.
    pub(crate) fn chr_update_cloak(&mut self, pi: usize) {
        let (lv240, lv60) = (self.lv.lvupdate240, self.lv.lvupdate60);
        self.players[pi].cloak.tick_pause(lv60);
        let p = &mut self.players[pi];
        if p.devicesactive & DEVICE_CLOAKDEVICE != 0 {
            let mut qty = p.gun.ammoheld(AMMOTYPE_CLOAK);
            if qty > 0 {
                if p.cloak.cloaked {
                    qty = (qty - lv60).max(0);
                    // bgun_set_ammo_quantity.
                    p.gun.p.ammoheldarr[AMMOTYPE_CLOAK as usize] = qty;
                }
            } else {
                p.devicesactive &= !DEVICE_CLOAKDEVICE;
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

    /// `chr_update_cloak` (`chr.c:2088`), the simulant branch: the cloaking
    /// device eats `ammoheld[AMMOTYPE_CLOAK]` while it hides the simulant; the
    /// RC-P120's cloak 0.4 rounds a tick (the clip, then the reserve), while
    /// the RC-P120 is in hand with ammo. (No fade gate here, unlike the
    /// player's drain.)
    pub(crate) fn chr_update_cloak_bot(&mut self, i: usize) {
        let (lv240, lv60, freal) = (self.lv.lvupdate240, self.lv.lvupdate60, self.lv.lvupdate60freal);
        let dead = self.chr_is_dead(i);
        let gset = self.res.gset.clone();
        let c = &mut self.chrs[i];
        c.cloak.tick_pause(lv60);
        let cloaked = c.cloak.cloaked;
        let a = c.aibot.as_mut().expect("a simulant");
        if a.cloakdeviceenabled {
            let qty = a.ammoheld[AMMOTYPE_CLOAK as usize];
            if qty > 0 && !dead {
                if cloaked {
                    a.ammoheld[AMMOTYPE_CLOAK as usize] = (qty - lv60).max(0);
                }
            } else {
                a.cloakdeviceenabled = false;
            }
        } else if a.rcp120cloakenabled {
            if a.weaponnum == WEAPON_RCP120 && !dead && a.botact_get_ammo_quantity_by_weapon(&gset, WEAPON_RCP120, FUNC_PRIMARY, true) > 0 {
                if cloaked {
                    a.rcpcloaktimer60 += freal * 0.4;
                    if a.rcpcloaktimer60 >= 1.0 {
                        let qty = a.rcpcloaktimer60 as i32;
                        a.rcpcloaktimer60 -= qty as f32;
                        if a.loadedammo[0] > 0 {
                            a.loadedammo[0] = (a.loadedammo[0] - qty).max(0);
                        } else {
                            let t = crate::bot::botinv::botact_get_ammo_type_by_function(&gset, WEAPON_RCP120, FUNC_PRIMARY);
                            if t >= 0 && a.ammoheld[t as usize] > 0 {
                                a.ammoheld[t as usize] = (a.ammoheld[t as usize] - qty).max(0);
                            }
                        }
                    }
                }
            } else {
                a.rcp120cloakenabled = false;
            }
        }
        let wanted = a.cloakdeviceenabled || a.rcp120cloakenabled;
        if wanted {
            if !c.cloak.cloaked && c.cloak.pause < 1 && c.cloak.cloak(dead) {
                let pos = c.pos;
                self.sound_at(0x005b, 1.0, pos, crate::propsnd::DEFAULT_DISTS);
            }
        } else if c.cloak.cloaked {
            self.chr_uncloak_chr(i, true);
        }
        self.chrs[i].cloak.update_fade(lv240, lv60);
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

#[cfg(test)]
mod tests {
    use pd_core::ids::*;

    use crate::harness;
    use crate::testutil::res;

    /// A simulant with 30 s of cloaking device (more than the 20-40 s it
    /// wants) cloaks with PD's sound, fades in, eats a tick of it per tick
    /// while hidden, and uncloaks when it runs out.
    #[test]
    fn a_simulant_cloaks_with_the_device_until_it_runs_out() {
        let mut w = harness::arena(res(), harness::setup(0, 1, BOTDIFF_NORMAL), harness::SPIKE_SEED).unwrap();
        for _ in 0..150 {
            harness::step_idle(&mut w);
        }
        w.ab_mut(0).ammoheld[AMMOTYPE_CLOAK as usize] = 2500;
        harness::step_idle(&mut w);
        harness::step_idle(&mut w);
        assert!(w.ab(0).cloakdeviceenabled && w.chrs[0].cloak.cloaked, "cloaked");
        let left = w.ab(0).ammoheld[AMMOTYPE_CLOAK as usize];
        for _ in 0..240 {
            harness::step_idle(&mut w);
        }
        assert_eq!(w.ab(0).ammoheld[AMMOTYPE_CLOAK as usize], left - 240, "a tick a tick");
        let alpha = w.chrs[0].cloak.alpha();
        assert!(w.chrs[0].cloak.fadefinished && alpha <= 20, "faded to the shimmer ({alpha})");
        // Below its turn-off point (0-20 s by random1) it switches off.
        w.ab_mut(0).ammoheld[AMMOTYPE_CLOAK as usize] = 1;
        for _ in 0..3 {
            harness::step_idle(&mut w);
        }
        assert!(!w.chrs[0].cloak.cloaked, "uncloaked");
    }

    /// A simulant holding an RC-P120 with plenty of rounds (over 300-800)
    /// cloaks with it, and the cloak eats 0.4 rounds a tick from the clip.
    #[test]
    fn a_simulant_cloaks_with_the_rcp120() {
        let mut w = harness::arena(res(), harness::setup(0, 1, BOTDIFF_NORMAL), harness::SPIKE_SEED).unwrap();
        w.bot_loadout = vec![Some((WEAPON_RCP120, false))];
        for _ in 0..150 {
            harness::step_idle(&mut w);
        }
        assert_eq!(w.ab(0).weaponnum, WEAPON_RCP120);
        harness::step_idle(&mut w);
        assert!(w.ab(0).rcp120cloakenabled && w.chrs[0].cloak.cloaked, "cloaked");
        let before = w.ab(0).loadedammo[0];
        for _ in 0..50 {
            harness::step_idle(&mut w);
        }
        assert_eq!(before - w.ab(0).loadedammo[0], 20, "0.4 rounds a tick");
    }

    /// The player's cloaking device (switched on from the active menu) eats
    /// its reserve while it hides the player and switches itself off at 0.
    #[test]
    fn the_players_cloaking_device_runs_down() {
        let mut w = crate::testutil::range();
        w.players[0].gun.p.unlimited_ammo = false;
        w.players[0].gun.p.ammoheldarr[AMMOTYPE_CLOAK as usize] = 100;
        w.players[0].devicesactive |= DEVICE_CLOAKDEVICE;
        crate::testutil::idle(&mut w, 2);
        assert!(w.players[0].cloak.cloaked, "cloaked");
        crate::testutil::idle(&mut w, 120);
        assert_eq!(w.players[0].devicesactive & DEVICE_CLOAKDEVICE, 0, "the device went off");
        assert!(!w.players[0].cloak.cloaked);
    }
}
