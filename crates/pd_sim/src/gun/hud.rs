//! The gun HUD's state (`bgun_draw_hud`, `bondgun.c:9930`): the function
//! square's fade, the weapon and function names' reveal timers, and the
//! magazine and reserve gauges' `abmag` trackers (`bgun0f0a9da8`).
//!
//! PD ticks these while it draws. Here [`GunCtx::bgun_tick_hud`] runs the same
//! updates under the same conditions each frame, and `pd_render::hud` draws
//! from the result without changing it.
//!
//! Source: the old repo's `pd_guns/hud.rs`, split into its timers (here) and its
//! drawing (`pd_render::hud`).

use pd_core::ids::*;

use super::{GunCtx, AMMO_CAPACITY};

/// The gauges' width, px.
pub const BARWIDTH: i32 = 9;
/// The reserve gauge's and the magazine gauge's heights, px.
pub const RESERVEHEIGHT: i32 = 36;
pub const CLIPHEIGHT: i32 = 57;
/// The gauges' bottom, above the view's bottom edge.
pub const BOTTOM_MARGIN: i32 = 13;

/// `struct abmag`: a gauge's animated view of its clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Abmag {
    pub loadedammo: i32,
    pub change: i32,
    pub ref_: i32,
    pub timer60: i32,
}

impl Abmag {
    /// `bgun0f0a9da8` (`bondgun.c:9606`).
    pub fn tick(&mut self, mut remaining: i32, mut capacity: i32, height: i32, lvupdate60: i32) {
        if capacity > 20 {
            let mut newremaining = height * remaining / capacity;
            if remaining > 0 && newremaining < 1 {
                newremaining = 1;
            }
            capacity = height;
            if newremaining == self.ref_ && self.loadedammo > remaining {
                self.ref_ += 1;
            }
            self.loadedammo = remaining;
            remaining = newremaining;
        }
        let mut newchange = remaining - self.ref_;
        if (self.change < 0 && newchange > 0) || (self.change > 0 && newchange < 0) {
            self.ref_ += self.change;
            self.change = 0;
            self.timer60 = 0;
            newchange = remaining - self.ref_;
        }
        if self.change < 0 && self.change > newchange && self.timer60 > -self.change * 64 {
            self.timer60 = -self.change * 64;
        }
        self.change = newchange;
        let speed = if self.change > 0 {
            capacity.max(6)
        } else {
            let mut h = 8;
            if self.change < -3 {
                h += -self.change * 2;
            }
            h
        };
        if self.change != 0 {
            self.timer60 += lvupdate60 * speed;
            if self.timer60 > 255 {
                if self.change > 0 {
                    while self.timer60 > 255 && self.change > 0 {
                        self.change -= 1;
                        self.ref_ += 1;
                        self.timer60 -= 64;
                    }
                } else {
                    while self.timer60 > 255 && self.change < 0 {
                        self.change += 1;
                        self.ref_ -= 1;
                        self.timer60 -= 64;
                    }
                }
            }
        } else {
            self.timer60 = 0;
        }
    }
}

/// The `gunctrl` fields `bgun_draw_hud` keeps between frames.
#[derive(Clone, Debug, Default)]
pub struct HudState {
    /// The magazines, by hand.
    pub abmag: [Abmag; 2],
    /// The reserve.
    pub ctrl_abmag: Abmag,
    /// The function square's red → yellow fade, 0..255.
    pub fnfader: i32,
    pub guntypetimer: i32,
    pub curgunstr: u8,
    pub fnstrtimer: i32,
    /// The function name being shown.
    pub curfnstr: Option<String>,
    pub lastmag: i32,
    /// The function shown this frame (the one in use, or the other if it is
    /// out of ammo).
    pub funcnum: usize,
    /// The HUD is drawn this frame (`lvframenum >= 5`).
    pub shown: bool,
    /// The weapon and function names are drawn this frame (their timers were
    /// still counting).
    pub gunstr_shown: bool,
    pub fnstr_shown: bool,
}

impl GunCtx<'_> {
    /// The state half of `bgun_draw_hud` (`bondgun.c:9930`), full-screen, one
    /// player: what the draw would tick, in the order it would.
    pub fn bgun_tick_hud(&mut self) {
        let st_shown = self.lv.lvframenum >= 5;
        self.b.hud.shown = st_shown;
        self.b.hud.gunstr_shown = false;
        self.b.hud.fnstr_shown = false;
        if !st_shown {
            return;
        }
        let lv240 = self.lv.lvupdate240;
        let lv60 = self.lv.lvupdate60;
        let hand = self.b.hands[HAND_RIGHT].clone();
        let lefthand_inuse = self.b.hands[HAND_LEFT].inuse;
        let (lclip, lloaded, lweapon) = {
            let l = &self.b.hands[HAND_LEFT];
            (l.clipsizes, l.loadedammo, l.weaponnum)
        };
        let weaponnum = self.b.ctrl.weaponnum;

        let mut funcnum = hand.weaponfunc;
        let fnfaderinc = lv240 * 2;
        let tmpfuncnum = self.bgun_is_using_secondary_function() as usize;
        if self.bgun_get_ammo_state(tmpfuncnum, HAND_RIGHT) > GUNAMMOSTATE_DEPLETED {
            funcnum = tmpfuncnum;
        }
        let func = self.gset.func(hand.weaponnum, funcnum).cloned();
        let weapon = self.gset.weapon(weaponnum).cloned();
        let ammoheld = |b: &super::Bgun, t: i32| b.ammoheld(t);
        let st = &mut self.b.hud;
        st.funcnum = funcnum;

        // The function square.
        if funcnum == FUNC_SECONDARY && st.fnfader < 255 {
            st.fnfader = st.fnfader.max(128);
            st.fnfader = (st.fnfader + fnfaderinc).min(255);
        }
        if funcnum == FUNC_PRIMARY && st.fnfader > 0 {
            st.fnfader = (st.fnfader - fnfaderinc).max(0);
        }

        // The weapon name, then the function name (options_get_show_gun_function on).
        if st.curgunstr != hand.weaponnum {
            st.guntypetimer = 0;
            st.curgunstr = hand.weaponnum;
        }
        if st.guntypetimer < 255 {
            st.guntypetimer = (st.guntypetimer + lv60).min(255);
            st.gunstr_shown = true;
        }
        if let Some(func) = &func {
            if (st.curfnstr.as_deref() != Some(func.name.as_str()) && st.fnfader > 128) || st.curfnstr.is_none() {
                st.fnstrtimer = 0;
                st.curfnstr = Some(func.name.clone());
            }
            if st.fnstrtimer < 255 {
                st.fnstrtimer = (st.fnstrtimer + lv60).min(255);
                st.fnstr_shown = true;
            }
        }

        let Some(weapon) = weapon else { return };
        let mut ammoindex = weapon.functions[hand.weaponfunc].as_ref().map_or(0, |f| f.ammoindex);
        if ammoindex == -1 {
            ammoindex = weapon.functions[1 - hand.weaponfunc].as_ref().map_or(-1, |f| f.ammoindex);
            if ammoindex == -1 {
                return;
            }
        }
        let st = &mut self.b.hud;
        if ammoindex != st.lastmag {
            st.abmag = [Abmag::default(); 2];
            st.ctrl_abmag = Abmag::default();
            st.lastmag = ammoindex;
        }
        let ai = ammoindex as usize;

        // The left hand's magazine.
        if lefthand_inuse && weapon.ammos[ai].is_some() && lweapon != WEAPON_REMOTEMINE {
            let a = weapon.ammos[ai].as_ref().unwrap();
            if lclip[ai] > 0 && a.flags & AMMOFLAG_EQUIPPEDISRESERVE == 0 {
                st.abmag[HAND_LEFT].tick(lloaded[ai], lclip[ai], CLIPHEIGHT, lv60);
            }
        }

        // The right hand's magazine and the reserve.
        let ammotype = self.b.ctrl.ammotypes[ai];
        if hand.inuse && ammotype >= 0 {
            let held = ammoheld(self.b, ammotype);
            let st = &mut self.b.hud;
            let a = weapon.ammos[ai].as_ref();
            if hand.clipsizes[ai] > 0 && a.is_some_and(|a| a.flags & AMMOFLAG_EQUIPPEDISRESERVE == 0) {
                st.abmag[HAND_RIGHT].tick(hand.loadedammo[ai], hand.clipsizes[ai], CLIPHEIGHT, lv60);
            }
            let capacity = AMMO_CAPACITY.get(ammotype as usize).copied().unwrap_or(0);
            if let Some(a) = a {
                if capacity > 0 && a.flags & AMMOFLAG_NORESERVE == 0 {
                    let mut ammototal = held;
                    if a.flags & AMMOFLAG_EQUIPPEDISRESERVE != 0 {
                        if hand.clipsizes[ai] > 0 {
                            ammototal += hand.loadedammo[ai];
                        }
                        if lclip[ai] > 0 {
                            ammototal += lloaded[ai];
                        }
                    }
                    st.ctrl_abmag.tick(ammototal, capacity, RESERVEHEIGHT, lv60);
                }
            }
        }
    }
}
