//! Blood on the level (`splat.c`): a round through a chr sprays up to two
//! splats behind it (`splats_create_for_chr_hit`); a wounded chr drips drops
//! where it walks and a dead one leaves a pool (`splat_tick_chr`). Each is a
//! ray from the hit (or down from the chr) to the first BG surface within
//! 1.8 m, where a blood wallhit in the chr's colour grows in
//! (`splat_create_wallhit`). A Skedar's (or Mr Blonde's) blood smokes.
//!
//! `// SUBST:` PD's spray also lands on the objects on screen (`obj_test_hit`)
//! and walks the portals' rooms from the chr (`portal_find_rooms`,
//! `bg_test_hit_in_room`) / blood only lands on the BG, tested in every room
//! (as the shots' BG test is).

use glam::Vec3;
use pd_core::ids::*;
use pd_core::math::{dtor, load_rotation};

use super::wallhit::{wallhit_create_with_extra, WallhitExtra};
use crate::world::World;

/// `SPLATTYPE_*` (`splat.c:17`): a round's spray, a dead chr's pool, a
/// wounded one's drop.
pub const SPLATTYPE_SHOT: i32 = 0;
pub const SPLATTYPE_PUDDLE: i32 = 1;
pub const SPLATTYPE_DROP: i32 = 2;

/// `g_SplatDistanceScale`, `g_SplatMaxAngleInDegrees`, `g_SplatMaxDistance`,
/// `g_SplatMinDiameter`, `g_SplatMaxDiameter` (`splat.c:42`).
const SPLAT_DISTANCE_SCALE: f32 = 0.15;
const SPLAT_MAX_ANGLE: f32 = 12.0;
const SPLAT_MAX_DISTANCE: f32 = 180.0;
const SPLAT_MIN_DIAMETER: f32 = 5.0;
const SPLAT_MAX_DIAMETER: f32 = 50.0;

/// A chr's splat bookkeeping (`splat_reset_chr`'s fields).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplatState {
    pub bulletstaken: i32,
    pub tickssincesplat: i32,
    pub stdsplatsadded: i32,
    pub woundedsplatsadded: i32,
    pub deaddropsplatsadded: i32,
    pub splatsdroppedhere: i32,
    pub lastdroppos: Vec3,
    /// `lastattacker`: the chr whose round it last took.
    pub lastattacker: Option<usize>,
}

/// `chr_get_blood_colour`'s first colour (`chr.c:3297`) by body.
pub fn chr_get_blood_colour(bodynum: usize) -> [u8; 3] {
    match bodynum as i32 {
        BODY_ELVIS1 | BODY_THEKING | BODY_ELVISWAISTCOAT => [0x0a, 0x40, 0x0a],
        BODY_DRCAROLL | BODY_EYESPY | BODY_CHICROB => [0x0a, 0x0a, 0x0a],
        BODY_MRBLONDE | BODY_SKEDAR | BODY_MINISKEDAR | BODY_SKEDARKING => [0x40, 0x19, 0x0a],
        _ => [0x40, 0x0a, 0x0a],
    }
}

/// A splat's ray: where it starts and which way.
#[derive(Clone, Copy)]
struct SplatRay {
    from: Vec3,
    dir: Vec3,
}

impl World {
    /// Whether chr `i` bleeds (`chr_hit`'s test, `chr.c:4759`): MP bodies are
    /// all human or Skedar (no Dr Caroll, robots or eyespies).
    fn chr_isskedar(&self, i: usize) -> bool {
        let b = self.chrs[i].bodynum as i32;
        b == BODY_MRBLONDE || b == BODY_SKEDAR || b == BODY_MINISKEDAR || b == BODY_SKEDARKING
    }

    /// `splats_create_for_chr_hit` (`splat.c:131`): chr `i` took a round
    /// fired from `gunpos` that met it at `hitpos` going `dir`; 0-2 splats
    /// spray on behind it (`attacker` the shooter's chr).
    pub(crate) fn splats_create_for_chr_hit(&mut self, i: usize, gunpos: Vec3, hitpos: Vec3, dir: Vec3, attacker: Option<usize>) {
        let s = &mut self.chrs[i].splat;
        if s.bulletstaken < 7 {
            s.bulletstaken += 1;
        }
        s.lastattacker = attacker;
        let qty = self.rng.random() % 3;
        if qty != 0 {
            let isskedar = self.chr_isskedar(i);
            let dist = gunpos.distance(hitpos);
            let n = self.splats_create(qty, 0.8, i, Some(SplatRay { from: hitpos, dir }), dist, isskedar, SPLATTYPE_SHOT, 50, attacker, 0);
            self.chrs[i].splat.stdsplatsadded += n;
        }
    }

    /// `splat_tick_chr` (`splat.c:60`), each frame a chr is ticked: a dead
    /// chr past its death animation's thud drops up to six pools a half
    /// second apart; a wounded one, drops the more often the more rounds it
    /// took (fewer where it already bled), at most 40 kept.
    pub(crate) fn splat_tick_chr(&mut self, i: usize) {
        let s = self.chrs[i].splat;
        if s.bulletstaken == 0 {
            return;
        }
        let isskedar = self.chr_isskedar(i);
        let attacker = s.lastattacker;
        // `ACT_DEAD || ACT_DIE`: a dead player's body (ACT_BONDMULTI) goes on
        // dripping as a wounded one.
        let dead = self.chrs[i].is_dying_or_dead();
        if dead {
            // A simulant's thud frames aren't kept: its fall counts as done.
            if s.tickssincesplat > 30 && s.deaddropsplatsadded < 6 {
                let speed = self.rng.random() & 8;
                let n = self.splats_create(1, 1.1, i, None, 0.7, isskedar, SPLATTYPE_PUDDLE, 150, attacker, speed);
                self.chrs[i].splat.deaddropsplatsadded += n;
            }
        } else {
            let value = s.bulletstaken * s.tickssincesplat;
            if value > 240 {
                let pos = self.chrs[i].pos;
                let dist = s.lastdroppos.distance(pos);
                let mut addmore = false;
                if dist > 40.0 {
                    addmore = true;
                    self.chrs[i].splat.splatsdroppedhere = 0;
                } else if s.splatsdroppedhere < 8 {
                    addmore = true;
                    self.chrs[i].splat.splatsdroppedhere += 1;
                }
                if addmore {
                    let n = self.splats_create(1, 0.3, i, None, 0.7, isskedar, SPLATTYPE_DROP, 80, attacker, 0);
                    self.chrs[i].splat.woundedsplatsadded += n;
                }
            }
            if self.chrs[i].splat.woundedsplatsadded >= 40 {
                self.wallhit_remove_oldest_wounded_splat_by_chr(i);
                self.chrs[i].splat.woundedsplatsadded -= 1;
            }
            self.chrs[i].splat.deaddropsplatsadded = 0;
        }
        self.chrs[i].splat.tickssincesplat += self.lv.lvupdate60;
    }

    /// `wallhit_remove_oldest_wounded_splat_by_chr` (`wallhit.c:1500`).
    fn wallhit_remove_oldest_wounded_splat_by_chr(&mut self, i: usize) {
        if let Some(w) = self.fx.wallhits.iter_mut().filter(|w| w.chr == Some(i) && !w.fading && w.is_blood_drop()).min_by_key(|w| w.createdframe) {
            w.wallhit_fade(120);
        }
    }

    /// `splats_create` (`splat.c:150`): `qty` rays each turned up to 12° off
    /// the spray (a shot's, from its hit; else straight down from 50 cm above
    /// a chr), and a splat where each lands. Returns how many landed.
    #[allow(clippy::too_many_arguments)]
    fn splats_create(&mut self, qty: u32, scale: f32, i: usize, shot: Option<SplatRay>, dist: f32, isskedar: bool, splattype: i32, timermax: u32, attacker: Option<usize>, timerspeed: u32) -> i32 {
        let ray = shot.unwrap_or_else(|| {
            // PROPTYPE_CHR: 50 cm up; a player's prop: none.
            let extraheight = if self.chrs[i].player.is_none() { 50.0 } else { 0.0 };
            SplatRay { from: self.chrs[i].pos + Vec3::Y * extraheight, dir: Vec3::NEG_Y }
        });
        let mut numdropped = 0;
        for _ in 0..qty {
            let a: [f32; 3] = std::array::from_fn(|_| dtor(self.rng.randomfrac() * SPLAT_MAX_ANGLE * 2.0 - SPLAT_MAX_ANGLE));
            let m = load_rotation(Vec3::from(a));
            let dir = m.transform_vector3(ray.dir).normalize_or_zero();
            if self.splat_create_one(scale, i, SplatRay { from: ray.from, dir }, dist, isskedar, splattype, timermax, attacker, timerspeed) {
                numdropped += 1;
            }
        }
        if numdropped > 0 {
            let pos = self.chrs[i].pos;
            let s = &mut self.chrs[i].splat;
            s.tickssincesplat = 0;
            s.lastdroppos = pos;
        }
        numdropped
    }

    /// `splat_create_one` (`splat.c:246`): the first BG surface on the ray
    /// within `g_SplatMaxDistance`.
    #[allow(clippy::too_many_arguments)]
    fn splat_create_one(&mut self, scale: f32, i: usize, ray: SplatRay, _dist: f32, isskedar: bool, splattype: i32, timermax: u32, attacker: Option<usize>, timerspeed: u32) -> bool {
        let end = ray.from + ray.dir * SPLAT_MAX_DISTANCE;
        let Some(hit) = self.stage.bghit.bg_test_hit(ray.from, end) else { return false };
        if hit.pos == ray.from || ray.from.distance(hit.pos) >= SPLAT_MAX_DISTANCE {
            return false;
        }
        self.splat_create_wallhit(i, ray.from, hit.pos, hit.normal, hit.room, scale, splattype, timermax, attacker, timerspeed, isskedar);
        true
    }

    /// `splat_create_wallhit` (`splat.c:413`): a blood hole (BLOOD1-3, a
    /// drop's BLOOD4) sized by how far it sprayed (× 1.5, 5 or 3, 5-50 cm),
    /// its alpha 0xc0-0xff, turned at random, growing in over `timermax`.
    #[allow(clippy::too_many_arguments)]
    fn splat_create_wallhit(&mut self, i: usize, gunpos: Vec3, pos: Vec3, normal: Vec3, room: u16, splatscale: f32, splattype: i32, timermax: u32, attacker: Option<usize>, timerspeed: u32, isskedar: bool) {
        let mut texnum = WALLHITTEX_BLOOD1 + (self.rng.random() % 3) as usize;
        match splattype {
            SPLATTYPE_PUDDLE => texnum = WALLHITTEX_BLOOD1 + (self.rng.random() % 3) as usize,
            SPLATTYPE_DROP => {
                // PD's `random() % 1`: a draw, always 0.
                let _ = self.rng.random();
                texnum = WALLHITTEX_BLOOD4;
            }
            _ => {}
        }
        let scale = match self.rng.random() % 6 {
            0..=2 => 1.5,
            3 | 4 => 5.0,
            _ => 3.0,
        };
        let distance = gunpos.distance(pos);
        let diameter = (SPLAT_DISTANCE_SCALE * distance * scale).clamp(SPLAT_MIN_DIAMETER, SPLAT_MAX_DIAMETER);
        let radius = (0.5 * diameter).max(1.0);
        let width = (self.rng.randomfrac() * radius * 2.0 - radius + diameter).min(SPLAT_MAX_DIAMETER) * splatscale;
        let height = (self.rng.randomfrac() * radius * 2.0 - radius + diameter).min(SPLAT_MAX_DIAMETER) * splatscale;
        // wallhit_choose_blood_colour(splat->chrprop): the bleeding chr's.
        let blood = chr_get_blood_colour(self.chrs[i].bodynum);
        let rotdeg = self.rng.random() % 360;
        let brightness = self.lights.brightness(Some(room));
        let source = attacker.map(|a| self.chrs[a].pos);
        let extra = WallhitExtra { blood, timermax, timerspeed, chr: Some(i), frame: self.lv.lvframenum };
        let wh = wallhit_create_with_extra(&mut self.rng, pos, normal, source, texnum, width, height, 0xc0, 0xff, rotdeg, brightness, extra);
        self.fx.push_wallhit(wh);
        // A Skedar's blood smokes (`splat.c:510`; PD tests the same flag twice,
        // so it is always SMOKETYPE_SKCORPSE).
        if isskedar {
            self.fx.smokes.smoke_create_simple(pos, SMOKETYPE_SKCORPSE);
        }
    }

    /// `wallhits_tick` (`wallhit.c:417`), in `lv_tick`: the splats' timers.
    pub(crate) fn wallhits_tick(&mut self) {
        let lv240 = self.lv.lvupdate240;
        self.fx.wallhits.retain_mut(|w| w.tick(lv240));
    }
}

#[cfg(test)]
mod tests {
    use crate::player::PlayerInput;
    use pd_core::ids::*;

    /// Shooting a simulant leaves its blood: splats in its colour, growing in
    /// from nothing to their full alpha, owned by it; killed, it pools.
    #[test]
    fn a_shot_simulant_bleeds_on_the_floor() {
        let (mut w, _, _) = crate::bot::tests::duel(None);
        w.harness_give_loadout(vec![WEAPON_FALCON2]);
        // Aim low, at the legs, so the spray meets the floor.
        w.players[0].verta = -12.0;
        crate::testutil::run(&mut w, &PlayerInput::default(), 120);
        let blood = |w: &crate::world::World| w.fx.wallhits.iter().filter(|h| h.chr == Some(1) && (WALLHITTEX_BLOOD1..=WALLHITTEX_BLOOD4).contains(&h.texnum)).count();
        let mut first = None;
        for t in 0..60 * 12 {
            crate::testutil::run(&mut w, &PlayerInput { fire: t % 12 < 6, ..Default::default() }, 1);
            if first.is_none() && blood(&w) > 0 {
                first = w.fx.wallhits.iter().find(|h| h.chr == Some(1)).cloned();
            }
            if w.chr_is_dead(1) {
                break;
            }
        }
        let h = first.expect("no blood");
        assert!(h.expanding && h.timermax > 0 && h.cols[0][3] < 0.2, "it grows in: {:?}", (h.expanding, h.timermax, h.cols[0]));
        assert!(h.cols[0][0] > h.cols[0][1], "red: {:?}", h.cols[0]);
        assert!(w.chrs[1].splat.bulletstaken > 0);
        let before = blood(&w);
        crate::testutil::run(&mut w, &PlayerInput::default(), 150);
        assert!(blood(&w) > before, "the corpse pools ({before} splats before)");
        assert!(w.fx.wallhits.iter().filter(|h| h.chr == Some(1)).any(|h| h.timermax == 0 && h.cols[0][3] > 0.7), "a splat grown in full");
    }
}
