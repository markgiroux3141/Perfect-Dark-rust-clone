//! A human player's body as the other players see it: `player_tick_third_person`
//! (`player.c:5275`), the tick of a player's prop in another player's pass.
//! The body plays `player_choose_third_person_animation`'s rows from the
//! player's movement (its death animation once dead), aims up and down along
//! the player's crosshair (`shootrotx`), shows its guns' flashes while the
//! hands' do, faces the player's way less the legs' `angleoffset`, and is
//! posed by `chr_tick`; its guns' muzzles are kept for the tracers the others
//! see (`chrmuzzlelastpos`). In the player's own pass nothing of this runs:
//! PD draws no body in the first person.
//!
//! `// SUBST:` PD ticks the body again in each later pass that isn't its
//! player's (only the first advances its animation, `CHRHFLAG_00000800`) and
//! poses it in that pass's eye space / once per frame, in world space, as the
//! simulants are (`World::chrs_tick`).

use pd_core::ids::*;
use pd_core::math::{baddtor, turn};

use crate::chr::{thirdperson, Act};
use crate::world::World;

impl World {
    /// `player_tick_third_person`'s other-player branch for player `j`'s body
    /// (chr `j`).
    pub(crate) fn player_tick_third_person(&mut self, j: usize) {
        let res = self.res.clone();
        let dead = self.chr_is_dead(j);
        let prevanimnum = self.chrs[j].anim.animnum;
        let wm = crate::bot::wieldmode(&res.gset, &self.chrs[j]);
        let (hasleft, hasright) = (self.chrs[j].held[HAND_LEFT].is_some(), self.chrs[j].held[HAND_RIGHT].is_some());
        let p = &self.players[j];
        let (crouchpos, speedsideways, speedforwards, speedtheta) = (p.crouchpos, p.speedsideways, p.speedforwards, p.speedtheta);
        let soft = p.headanim == super::HEADANIM_RESTING;
        let mut angleoffset = p.angleoffset;
        let (shootrotx, shootroty) = if dead { (0.0, 0.0) } else { (p.shootrotx, p.shootroty) };
        let flashon = [p.gun.hands[HAND_RIGHT].flashon, p.gun.hands[HAND_LEFT].flashon];
        let freal = self.lv.lvupdate60freal;
        let c = &mut self.chrs[j];
        c.actiontype = Act::BondMulti;
        let mut ctx = c.model.anim_ctx(&res.bank);
        let animcfg = if dead {
            thirdperson::choose_death(&mut c.anim, &mut ctx, &mut self.rng);
            None
        } else {
            thirdperson::choose(&mut c.anim, &mut ctx, crouchpos, wm, speedsideways, speedforwards, speedtheta, &mut angleoffset, freal, soft).0
        };
        if c.anim.animnum == prevanimnum {
            if let Some(cfg) = animcfg {
                c.autoanim = false;
                c.chr_calculate_aimend_vertical(Some(cfg), hasleft, hasright, shootrotx);
            } else {
                c.autoanim = true;
                c.aimendback = shootrotx;
                c.aimendrshoulder = 0.0;
                c.aimendlshoulder = 0.0;
            }
        }
        c.aimendsideback = shootroty;
        c.aimendcount = 10;
        // chr_set_firing: the guns' flashes, which light the body's room.
        let mut flash = false;
        for (h, on) in [(HAND_RIGHT, flashon[0]), (HAND_LEFT, flashon[1])] {
            if let Some(held) = c.held[h].as_mut() {
                flash |= held.weapon_set_gunfire_visible(on);
            }
        }
        // The root on the player's spot, facing the player's way less the legs'.
        let mut angle = (360.0 - self.players[j].theta) * baddtor(1.0) - angleoffset;
        if angle >= turn() {
            angle -= turn();
        } else if angle < 0.0 {
            angle += turn();
        }
        let c = &mut self.chrs[j];
        c.playerangleoffset = angleoffset;
        c.model.chrinfo.set_chr_rot_y(angle);
        c.playertheta = angle;
        let sp80 = c.pos;
        c.model.chrinfo.pos.x = c.pos.x;
        c.model.chrinfo.pos.z = c.pos.z;
        self.players[j].angleoffset = angleoffset;
        if flash {
            if let Some(room) = self.chrs[j].rooms.first().copied() {
                self.room_flash_lighting(room, 48, 128);
            }
        }
        self.chr_tick(j, true);
        self.chrs[j].pos = sp80;
        // The guns' muzzles, for the tracers the others see; a gun not posed
        // for more than a frame falls back on the first-person muzzle.
        let frame = self.lv.lvframenum;
        for h in 0..2 {
            if let Some(pos) = self.chrs[j].chr_get_gun_pos(h) {
                self.players[j].chrmuzzlelastpos[h] = pos;
                self.players[j].chrmuzzlelast[h] = frame;
            } else if self.players[j].chrmuzzlelast[h] < frame - 1 {
                self.players[j].chrmuzzlelastpos[h] = self.players[j].gun.hands[h].muzzlepos;
            }
        }
    }

    /// `player_tick_third_person` for every body seen in player `current`'s
    /// pass: the other players' bodies not yet ticked this frame.
    pub(crate) fn players_tick_bodies(&mut self, current: usize, ticked: &mut [bool]) {
        for j in 0..self.players.len() {
            if j != current && !ticked[j] {
                ticked[j] = true;
                self.player_tick_third_person(j);
            }
        }
    }
}
