//! `bondwalk.c`: the walk. Move data → speeds, the head bob's root motion → a
//! position delta, collide-and-slide against the tiles and other chrs, then the
//! vertical: ladders, the ground, stepping up, falling, the landing dip, crouch.

use glam::Vec3;
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::math::{badrtod4, baddtor2};
use pd_core::rng::Rng;

use super::bondmove::bmove_dampen_shotspeed;
use super::{CdGlobals, Player, WalkEnv, HEADANIM_MOVING};
use crate::world::WorldRes;
use crate::stage::{CdResult, Edge};

/// Add `rotateamount` radians to `vv_theta` (degrees) the way PD does:
/// `BADRTOD4`, then wrapped into [0, 360).
fn add_theta(theta: f32, rotateamount: f32) -> f32 {
    let mut degrees = theta + badrtod4(rotateamount);
    while degrees < 0.0 {
        degrees += 360.0;
    }
    while degrees >= 360.0 {
        degrees -= 360.0;
    }
    degrees
}

impl Player {
    /// `bwalk_try_move_upwards` (`bondwalk.c:189`).
    fn bwalk_try_move_upwards(&mut self, env: &WalkEnv, amount: f32) -> CdResult {
        let newpos = self.pos + Vec3::Y * amount;
        let (radius, ymax, ymin) = self.player_get_bbox();
        let ymin = ymin - 0.1;
        let result = env.level.cd_test_volume_simple(newpos, radius, true, ymax - self.pos.y, ymin - self.pos.y, env.cyls);
        if result == CdResult::NoCollision {
            self.pos.y = newpos.y;
        }
        result
    }

    /// `bwalk_try_delta_nopush` (`bondwalk.c:236`): try moving by `delta`; on
    /// success (and `apply`) take the move and the turn.
    fn bwalk_try_delta_nopush(&mut self, env: &WalkEnv, delta: Vec3, rotateamount: f32, apply: bool, extrawidth: f32) -> CdResult {
        let mut result = CdResult::NoCollision;
        let mut dstpos = self.pos;
        if delta != Vec3::ZERO {
            dstpos += delta;
            let (radius, ymax, ymin) = self.player_get_bbox();
            let radius = radius + extrawidth;
            let (xdiff, zdiff) = (dstpos.x - self.pos.x, dstpos.z - self.pos.z);
            let halfradius = radius * 0.5;
            let (rymax, rymin) = (ymax - self.pos.y, ymin - self.pos.y);
            let level = env.level;
            if xdiff > halfradius || zdiff > halfradius || xdiff < -halfradius || zdiff < -halfradius {
                let (r, o) = level.cd_test_cylmove_oobfail_findclosest_finddist(self.pos, dstpos, radius, rymax, rymin, env.cyls);
                result = r;
                if r == CdResult::Error {
                    self.cd = CdGlobals::default();
                }
                self.cd.set(o);
                if result == CdResult::NoCollision {
                    let (r, o) = level.cd_test_volume_fromdir(self.pos, dstpos, radius, true, rymax, rymin, env.cyls);
                    result = r;
                    self.cd.set(o);
                }
            } else {
                let (r, o) = level.cd_test_volume_fromdir(self.pos, dstpos, radius, true, rymax, rymin, env.cyls);
                result = r;
                self.cd.set(o);
            }
        }
        if result == CdResult::NoCollision && apply {
            self.theta = add_theta(self.theta, rotateamount);
            self.pos = dstpos;
        }
        result
    }

    /// `bwalk_try_delta` (`bondwalk.c:378`). Its obstacle reactions are doors
    /// (none on Complex), pushable chrs (only teammates in multiplayer) and
    /// pushable objects, so in a free-for-all it is the nopush test.
    fn bwalk_try_delta(&mut self, env: &WalkEnv, delta: Vec3, rotateamount: f32, apply: bool, extrawidth: f32) -> CdResult {
        self.bwalk_try_delta_nopush(env, delta, rotateamount, apply, extrawidth)
    }

    /// `bwalk_try_fulldelta` (`bondwalk.c:526`).
    fn bwalk_try_fulldelta(&mut self, env: &WalkEnv, delta: Vec3) -> (CdResult, Edge) {
        let result = self.bwalk_try_delta(env, delta, 0.0, true, 0.0);
        let edge = if result == CdResult::Collision { self.cd.edge.unwrap_or((self.pos, self.pos)) } else { (Vec3::ZERO, Vec3::ZERO) };
        (result, edge)
    }

    /// `bwalk_try_quarterdelta` (`bondwalk.c:541`): a quarter of the way to the
    /// obstacle the last test met. A collision with a *different* edge is
    /// reported (with that edge); anything else that fails is an error.
    fn bwalk_try_quarterdelta(&mut self, env: &WalkEnv, delta: Vec3, prevedge: Edge) -> (CdResult, Edge) {
        let mut edge = (Vec3::ZERO, Vec3::ZERO);
        if let Some(distance) = self.cd.dist {
            let quarter = delta * distance / 4.0;
            let result = self.bwalk_try_delta(env, quarter, 0.0, true, 0.0);
            if result == CdResult::NoCollision {
                return (CdResult::NoCollision, edge);
            }
            if result == CdResult::Collision {
                edge = self.cd.edge.unwrap_or(edge);
                if prevedge.0 != edge.0 || prevedge.1 != edge.1 {
                    return (CdResult::Collision, edge);
                }
            }
        }
        (CdResult::Error, edge)
    }

    /// `bwalk_try_slide_along_edge` (`bondwalk.c:588`): the move projected onto the edge.
    fn bwalk_try_slide_along_edge(&mut self, env: &WalkEnv, delta: Vec3, e: Edge) -> CdResult {
        let (v1, v2) = e;
        if v1.x != v2.x || v1.z != v2.z {
            let mut edgedir = Vec3::new(v2.x - v1.x, 0.0, v2.z - v1.z);
            let tmp = (edgedir.x * edgedir.x + edgedir.z * edgedir.z).sqrt();
            edgedir.x *= 1.0 / tmp;
            edgedir.z *= 1.0 / tmp;
            let tmp = delta.x * edgedir.x + delta.z * edgedir.z;
            let newdelta = Vec3::new(edgedir.x * tmp, 0.0, edgedir.z * tmp);
            return self.bwalk_try_delta(env, newdelta, 0.0, true, 0.0);
        }
        CdResult::Error
    }

    /// `bwalk_try_slide_along_corner` (`bondwalk.c:616`): if the destination is
    /// within a radius of one of the edge's ends, the move projected onto the
    /// perpendicular to (end − pos): rounding the corner.
    fn bwalk_try_slide_along_corner(&mut self, env: &WalkEnv, delta: Vec3, e: Edge) -> CdResult {
        let (radius, _, _) = self.player_get_bbox();
        let pos = self.pos;
        let round = |this: &mut Self, vtx: Vec3| -> Option<CdResult> {
            if vtx.x != pos.x || vtx.z != pos.z {
                let (mut x, mut z) = (-(vtx.z - pos.z), vtx.x - pos.x);
                let tmp = (x * x + z * z).sqrt();
                x *= 1.0 / tmp;
                z *= 1.0 / tmp;
                let tmp = delta.x * x + delta.z * z;
                let newdelta = Vec3::new(x * tmp, 0.0, z * tmp);
                if this.bwalk_try_delta(env, newdelta, 0.0, true, 0.0) == CdResult::NoCollision {
                    return Some(CdResult::NoCollision);
                }
            }
            None
        };
        let near = |vtx: Vec3| {
            let (x, z) = (vtx.x - (pos.x + delta.x), vtx.z - (pos.z + delta.z));
            x * x + z * z <= radius * radius
        };
        if near(e.0) {
            if let Some(r) = round(self, e.0) {
                return r;
            }
        } else if near(e.1) {
            if let Some(r) = round(self, e.1) {
                return r;
            }
        }
        CdResult::Collision
    }

    /// `bwalk_resolve_posdelta` (`bondwalk.c:1257`): the full move; else a
    /// quarter of the way to the obstacle, then slide along its edge or round its
    /// corner (only when not leaning); against a second, nearer obstacle, the
    /// same with both edges.
    fn bwalk_resolve_posdelta(&mut self, env: &WalkEnv, delta: Vec3, notrleaning: bool) {
        let (r, edge_a) = self.bwalk_try_fulldelta(env, delta);
        if r != CdResult::Collision {
            return;
        }
        let (result, edge_b) = self.bwalk_try_quarterdelta(env, delta, edge_a);
        if result != CdResult::Collision {
            if notrleaning && self.bwalk_try_slide_along_edge(env, delta, edge_a) != CdResult::NoCollision {
                self.bwalk_try_slide_along_corner(env, delta, edge_a);
            }
        } else {
            let (_, _edge_c) = self.bwalk_try_quarterdelta(env, delta, edge_b);
            if notrleaning
                && self.bwalk_try_slide_along_edge(env, delta, edge_b) != CdResult::NoCollision
                && self.bwalk_try_slide_along_edge(env, delta, edge_a) != CdResult::NoCollision
                && self.bwalk_try_slide_along_corner(env, delta, edge_b) != CdResult::NoCollision
            {
                self.bwalk_try_slide_along_corner(env, delta, edge_a);
            }
        }
    }

    /// `bwalk_apply_move_data` (`bondwalk.c:1335`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bwalk_apply_move_data(
        &mut self,
        analogstrafe: i32,
        analogwalk: i32,
        digitalstep: i32,
        unk14: bool,
        canlookahead: bool,
        rleanleft: bool,
        rleanright: bool,
        crouchdown: i32,
        crouchup: i32,
        lv: &Lv,
    ) {
        let lv60 = lv.lvupdate60freal;
        // Sideways: digital (C-left/right) then analog (bondwalk.c:1339).
        if digitalstep < 0 {
            self.update_speed_sideways(-1.0, 0.2, lv.lvupdate60.max(1));
        } else if digitalstep > 0 {
            self.update_speed_sideways(1.0, 0.2, lv.lvupdate60.max(1));
        } else if !unk14 {
            self.update_speed_sideways(0.0, 0.2, lv.lvupdate60);
        }
        if unk14 {
            self.update_speed_sideways(analogstrafe as f32 * 0.014_285_714, 0.2, lv.lvupdate60);
        }
        // Forwards and back.
        if !canlookahead {
            self.update_speed_forwards(0.0, 1.0, lv60);
        } else {
            self.update_speed_forwards(analogwalk as f32 * 0.014_285_714, 1.0, lv60);
            if analogwalk > 60 {
                self.speedmaxtime60 += lv.lvupdate60;
            } else {
                self.speedmaxtime60 = 0;
            }
        }
        self.speedforwards = self.speedforwards.clamp(-1.0, 1.0);
        self.speedsideways = self.speedsideways.clamp(-1.0, 1.0);
        self.speedforwards *= 1.08;
        self.speedforwards *= self.speedboost;
        if !canlookahead || self.crouchpos != CROUCHPOS_STAND {
            self.speedmaxtime60 = 0;
        }
        // bwalk_set_sway_target: the lean.
        self.swaytarget = if rleanleft {
            -75.0
        } else if rleanright {
            75.0
        } else {
            0.0
        };
        for _ in 0..crouchdown {
            self.crouchpos = (self.crouchpos - 1).max(CROUCHPOS_SQUAT);
        }
        for _ in 0..crouchup {
            self.crouchpos = (self.crouchpos + 1).min(CROUCHPOS_STAND);
        }
    }

    /// `bwalk_update_speed_sideways` (`bondwalk.c:693`).
    fn update_speed_sideways(&mut self, targetspeed: f32, accelspeed: f32, mult: i32) {
        let step = accelspeed * mult as f32;
        if self.speedstrafe > targetspeed {
            self.speedstrafe = (self.speedstrafe - step).max(targetspeed);
        } else if self.speedstrafe < targetspeed {
            self.speedstrafe = (self.speedstrafe + step).min(targetspeed);
        }
        self.speedsideways = self.speedstrafe;
    }

    /// `bwalk_update_speed_forwards` (`bondwalk.c:716`).
    fn update_speed_forwards(&mut self, targetspeed: f32, accelspeed: f32, lv60: f32) {
        if self.speedgo < targetspeed {
            self.speedgo = (self.speedgo + accelspeed * lv60).min(targetspeed);
        } else if self.speedgo > targetspeed {
            self.speedgo = (self.speedgo - accelspeed * lv60).max(targetspeed);
        }
        self.speedforwards = self.speedgo;
    }

    /// `bwalk_update_theta` (`bondwalk.c:1239`).
    pub(super) fn bwalk_update_theta(&mut self, lv: &Lv) {
        let mult = 159.0 / self.eyeheight;
        let rotateamount = self.speedtheta * mult * lv.lvupdate60freal * 0.017_450_513 * 3.5;
        self.theta = add_theta(self.theta, rotateamount);
    }

    /// `bwalk_update_crouch_offset_real` (`bondwalk.c:1180`).
    pub(super) fn update_crouch_offset_real(&mut self) {
        let e = self.eyeheight;
        if e + -90.0 * e * (1.0 / 159.0) < 69.0 {
            self.crouchoffsetreal = self.crouchoffset * ((69.0 - e) / -90.0);
        } else {
            self.crouchoffsetreal = self.crouchoffset * e * (1.0 / 159.0);
        }
        self.crouchoffsetrealsmall = self.crouchoffsetreal;
    }

    /// `bwalk_update_crouch_offset` (`bondwalk.c:1197`) with `apply_speed`.
    /// Rising into a ceiling is refused: the offset stays and the crouch goes
    /// down a level (`bwalk_adjust_crouch_pos(-1)`).
    fn update_crouch_offset(&mut self, lv: &Lv, env: &WalkEnv) {
        let target = match self.crouchpos {
            CROUCHPOS_SQUAT => -90.0,
            CROUCHPOS_DUCK => -45.0,
            _ => 0.0,
        };
        if target != self.crouchoffset {
            let prev = (self.crouchoffset, self.crouchoffsetreal, self.crouchoffsetrealsmall);
            // apply_speed(&crouchoffset, target, &crouchspeed, 0.5, 0.5, 5.0)
            let (accel, decel, maxspeed) = (0.5f32, 0.5f32, 5.0f32);
            let mut speed = self.crouchspeed;
            for _ in 0..lv.lvupdate60 {
                let limit = speed * speed * 0.5 / decel;
                let rem = target - self.crouchoffset;
                if rem > 0.0 {
                    if speed > 0.0 && rem <= limit {
                        speed = (speed - decel).max(decel);
                    } else if speed < maxspeed {
                        speed += if speed < 0.0 { decel } else { accel };
                        speed = speed.min(maxspeed);
                    }
                    if speed >= rem {
                        self.crouchoffset = target;
                        break;
                    }
                    self.crouchoffset += speed;
                } else {
                    if speed < 0.0 && -rem <= limit {
                        speed = (speed + decel).min(-decel);
                    } else if speed > -maxspeed {
                        speed -= if speed > 0.0 { decel } else { accel };
                        speed = speed.max(-maxspeed);
                    }
                    if speed <= rem {
                        self.crouchoffset = target;
                        break;
                    }
                    self.crouchoffset += speed;
                }
            }
            self.crouchspeed = speed;
            self.update_crouch_offset_real();
            if self.bwalk_try_move_upwards(env, 0.0) == CdResult::Collision {
                (self.crouchoffset, self.crouchoffsetreal, self.crouchoffsetrealsmall) = prev;
                self.crouchspeed = 0.0;
                self.crouchpos = (self.crouchpos - 1).clamp(CROUCHPOS_SQUAT, CROUCHPOS_STAND);
            }
        }
        if target == self.crouchoffset {
            self.crouchspeed = 0.0;
        }
        self.guncloseroffset = self.crouchoffset / -90.0;
    }

    /// `bwalk_update_horizontal` (`bondwalk.c:1425`).
    pub(super) fn bwalk_update_horizontal(&mut self, lv: &Lv, env: &WalkEnv, res: &WorldRes, rng: &mut Rng) {
        let lv60 = lv.lvupdate60freal;
        let spc0 = (self.eyeheight - 159.0) / 353.333_3 + 1.0;
        // bwalk_apply_crouch_speed
        match self.crouchpos {
            CROUCHPOS_DUCK => {
                self.speedforwards *= 0.5;
                self.speedsideways *= 0.5;
            }
            CROUCHPOS_SQUAT => {
                self.speedforwards *= 0.35;
                self.speedsideways *= 0.35;
            }
            _ => {}
        }
        self.update_crouch_offset(lv, env);

        // bmove_shotspeed_to_lateral (bondmove.c:1877): being shot shoves.
        let (sintheta, costheta) = baddtor2(self.theta).sin_cos();
        let (shotforwards, shotsideways) = if self.shotspeed.x != 0.0 || self.shotspeed.z != 0.0 {
            bmove_dampen_shotspeed(&mut self.shotspeed, lv60);
            (self.shotspeed.z * costheta + -self.shotspeed.x * sintheta, -self.shotspeed.x * costheta - self.shotspeed.z * sintheta)
        } else {
            (0.0, 0.0)
        };

        // The lean.
        let th = self.theta_vec();
        let mut tmp1 = -self.swaytarget * th.z * spc0;
        let mut tmp2 = self.swaytarget * th.x * spc0;
        if self.crouchoffset < -45.0 {
            tmp1 *= 0.35;
            tmp2 *= 0.35;
        } else if self.crouchoffset < 0.0 {
            tmp1 *= 0.5;
            tmp2 *= 0.5;
        }
        let mut spb4 = tmp1 - self.swayoffset0;
        let mut spb0 = tmp2 - self.swayoffset2;
        let dist = (spb4 * spb4 + spb0 * spb0).sqrt();
        let (lv60f, lv240) = if lv60 > 4.0 { (4.0, 4) } else { (lv60, lv.lvupdate60) };
        let mut spa8 = 0.0f32;
        for _ in 0..lv240 {
            spa8 += (dist - spa8) * 0.1;
        }
        spa8 += 3.75 * lv60f;
        if self.crouchoffset < -45.0 {
            spa8 *= 0.35;
        } else if self.crouchoffset < 0.0 {
            spa8 *= 0.5;
        }
        if spa8 < dist {
            spa8 /= dist;
            spb4 *= spa8;
            spb0 *= spa8;
        }

        // Breathing.
        let speedsideways = ((self.speedsideways + shotsideways) * 0.8).abs();
        let speedforwards = (self.speedforwards + shotforwards).abs();
        let speedtheta = (self.speedtheta * 0.8).abs();
        let mut heartrate = speedforwards.max(speedsideways).max(speedtheta);
        if dist >= 0.1 && heartrate < 0.8 {
            heartrate = 0.8;
        }
        if heartrate >= 0.75 {
            self.bondbreathing += (heartrate - 0.75) * lv60 / 900.0;
        } else {
            self.bondbreathing -= (0.75 - heartrate) * lv60 / 2700.0;
        }
        self.bondbreathing = self.bondbreathing.clamp(0.0, 1.0);

        let mult = self.headanims[HEADANIM_MOVING as usize].translateperframe * 0.5 * lv60;
        let spe0 = (self.speedsideways * spc0 + shotsideways) * mult;
        // bmove_update_head (bondmove.c:2071)
        let fwd = self.speedforwards * spc0 + shotforwards;
        self.bhead_adjust_animation(heartrate);
        let newspeedforwards = if heartrate != 0.0 { fwd / heartrate } else { 0.0 };
        self.bhead_update(newspeedforwards, spe0, lv, rng);
        self.update_camera_basis();

        // The head's root motion is the step.
        self.gunspeed = heartrate;
        let spdc = self.headpos.x;
        let spd8 = self.headpos.z;
        let mut spcc = Vec3::ZERO;
        spcc.x += (spd8 * th.x - spdc * th.z) * lv60;
        spcc.z += (spd8 * th.z + spdc * th.x) * lv60;
        spcc.x += spb4;
        spcc.z += spb0;

        // Ladders (bondwalk.c:1614): the part of the move into the ladder becomes
        // climbing (ladderupdown, applied by bwalk_update_vertical) and the rest
        // is slowed to 30 %.
        if self.onladder {
            let n = self.laddernormal.normalize_or_zero();
            self.laddernormal = n;
            let sp74 = -(spcc.x * n.x + spcc.z * n.z);
            if -4.0 * lv60 < sp74 {
                if sp74 < 0.0 {
                    spcc.x += sp74 * n.x;
                    spcc.z += sp74 * n.z;
                    self.ladderupdown = sp74 * 0.3;
                } else {
                    let (radius, ymax, _) = self.player_get_bbox();
                    if env.level.cd_find_ladder(self.pos, radius * 1.1, ymax - self.pos.y, self.manground - self.pos.y + 1.0).is_none() {
                        self.ladderupdown = 0.0;
                    } else {
                        spcc.x += sp74 * n.x;
                        spcc.z += sp74 * n.z;
                        self.ladderupdown = sp74 * 0.3;
                    }
                }
                spcc.x *= 0.3;
                spcc.z *= 0.3;
            } else {
                self.ladderupdown = 0.0;
            }
        }

        let before = self.pos;
        self.bwalk_resolve_posdelta(env, spcc, self.swaytarget == 0.0);
        // Deltas from bondprevpos (bwalk_tick's start; the turn doesn't move).
        let xdelta = self.pos.x - self.prevpos.x;
        let zdelta = self.pos.z - self.prevpos.z;
        // A blocked shove loses the blocked part (bondwalk.c:1700).
        for (k, (d, want)) in [(xdelta, spcc.x), (zdelta, spcc.z)].into_iter().enumerate() {
            let a = if k == 0 { 0 } else { 2 };
            let s = &mut self.shotspeed[a];
            if d >= 0.0 {
                if *s > 0.0 {
                    if want >= 0.0 && d < want {
                        *s *= d / want;
                    }
                } else if want < 0.0 {
                    *s = 0.0;
                }
            } else if *s < 0.0 {
                if want <= 0.0 && want < d {
                    *s *= d / want;
                }
            } else if want > 0.0 {
                *s = 0.0;
            }
        }
        let (xdelta, zdelta) = (self.pos.x - before.x, self.pos.z - before.z);
        // Blocked movement bleeds speed (bondwalk.c:1736).
        let sp54 = -xdelta * th.z + zdelta * th.x;
        let sp50 = xdelta * th.x + zdelta * th.z;
        let sp4c = -spcc.x * th.z + spcc.z * th.x;
        let sp48 = spcc.x * th.x + spcc.z * th.z;
        if sp4c != 0.0 && self.speedstrafe * sp4c > 0.0 {
            let r = sp54 / sp4c;
            if r <= 0.0 {
                self.speedstrafe = 0.0;
            } else if r < 1.0 {
                self.speedstrafe *= r;
            }
        }
        if sp48 != 0.0 && self.speedgo * sp48 > 0.0 {
            let r = sp50 / sp48;
            if r <= 0.0 {
                self.speedgo = 0.0;
            } else if r < 1.0 {
                self.speedgo *= r;
            }
        }
        let f0 = spcc.x * spcc.x + spcc.z * spcc.z;
        let f0 = if f0 != 0.0 { ((xdelta * xdelta + zdelta * zdelta) / f0).sqrt() } else { 0.0 };
        self.swayoffset0 += f0 * spb4;
        self.swayoffset2 += f0 * spb0;

        // The gun's sway inputs (bondwalk.c:1771).
        let sp44 = self.speedtheta;
        let sp40 = (self.speedverta / 0.7 + self.crouchspeed / 5.0).clamp(-1.0, 1.0);
        let sp3c = self.gunspeed;
        let mut breathing = self.bhead_get_breathing_value();
        if self.headanim == HEADANIM_MOVING {
            breathing *= 1.2;
        }
        self.gun_ctx(res, rng, lv).bgun_update_sway(breathing, sp3c, sp40, sp44, 0.0);
        let mut v360 = self.verta;
        if v360 < 0.0 {
            v360 += 360.0;
        }
        self.gun.bgun_set_adjust_pos(v360 * 0.017_450_513);
    }

    /// `bwalk_update_vertical` (`bondwalk.c:739`), NTSC final: ladders, the
    /// ground under the cylinder, stepping up (a low-pass on `manground`, at most
    /// 50 cm below the ground, refused by a ceiling), falling (PD's gravity,
    /// landing exactly on the ground), the landing dip, and the eye height.
    ///
    /// Not ported (M9): lifts and escalators, `GEOFLAG_DIE` floors, landing on a
    /// chr in a lift. The counter-op radius fix and turbo mode are not in the
    /// Combat Simulator.
    pub(super) fn bwalk_update_vertical(&mut self, lv: &Lv, env: &WalkEnv) {
        let level = env.level;
        let (radius, ymax, _ymin) = self.player_get_bbox();
        let lv60 = lv.lvupdate60freal;

        // On a ladder? A second, lower probe keeps the player "near" one without
        // taking the ground from under it (onladder2).
        let mut onladder = level.cd_find_ladder(self.pos, radius * 1.2, ymax - self.pos.y, self.manground - self.pos.y + 1.0);
        let mut onladder2 = false;
        if onladder.is_none() {
            if let Some(n) = level.cd_find_ladder(self.pos, radius * 1.1, ymax - self.pos.y, self.manground - self.pos.y - 10.0) {
                onladder2 = true;
                self.laddernormal = n;
            }
        }
        if let Some(n) = onladder {
            self.laddernormal = n;
        }

        // cd_find_ground_at_cyl_ctfril from the eye.
        let (mut ground, floorpoly) = level.cd_find_ground_at_cyl(self.pos, self.radius);
        self.floorpoly = floorpoly;
        self.floorroom = floorpoly.and_then(|p| level.geom.polys[p].room);
        if let Some(p) = floorpoly {
            self.floortype = level.geom.polys[p].floortype;
        }
        if ground < -30000.0 {
            ground = -30000.0;
        }

        // Ladders.
        if self.onladder {
            if self.ladderupdown >= 0.0 || (ground <= self.manground && ground <= self.manground + self.ladderupdown) {
                // Still on the ladder.
                if self.bwalk_try_move_upwards(env, self.ladderupdown) == CdResult::NoCollision {
                    self.manground += self.ladderupdown;
                }
            } else if self.bwalk_try_move_upwards(env, ground - self.manground) == CdResult::NoCollision {
                self.manground = ground;
                onladder = None;
            }
        }
        self.onladder = onladder.is_some();
        if self.onladder {
            self.ground = self.manground;
        } else if !onladder2 {
            self.ground = ground;
        }

        // Standing on flat ground, or going up stairs, ledges or ramps.
        if self.fallspeed >= 0.0 || self.ground > self.manground {
            self.sumground = self.manground / 0.045_499_98;
            for _ in 0..lv.lvupdate240 {
                self.sumground = self.sumground * 0.9545 + self.ground;
            }
            if self.manground < self.ground {
                // The feet are lower than the ground.
                let sumground = (self.sumground * 0.045_499_98).max(self.ground - 50.0);
                if self.bwalk_try_move_upwards(env, sumground - self.manground) == CdResult::NoCollision {
                    self.manground = sumground;
                }
            }
        }

        if self.manground > self.ground {
            // Not standing on the ground: falling.
            let fallspeed = self.fallspeed;
            let mut newmanground = self.manground;
            let multiplier = 0.277_777_777;
            let mut newfallspeed = fallspeed - lv60 * multiplier;
            newmanground += lv60 * (fallspeed + newfallspeed) * 0.5;
            let mut fallspeed = newfallspeed;
            if newmanground < self.ground {
                newfallspeed = self.manground - self.ground;
                newmanground = self.ground;
                fallspeed = -(self.fallspeed * self.fallspeed + (((newfallspeed + newfallspeed) * 0.277_777_777) / 60.0) * 60.0).sqrt();
            }
            if self.bwalk_try_move_upwards(env, newmanground - self.manground) == CdResult::NoCollision {
                self.manground = newmanground;
                self.fallspeed = fallspeed;
                if !self.isfalling {
                    self.isfalling = true;
                    self.fallstart = lv.lvframe60;
                } else if lv.lvframe60 - self.fallstart > 240 {
                    // Falling for 4 seconds.
                    self.die_request = true;
                }
            } else {
                // Not falling (something is in the way).
                self.fallspeed = 0.0;
                self.isfalling = false;
                if self.manground <= -30000.0 {
                    self.die_request = true;
                }
            }
        } else {
            self.isfalling = false;
            if self.manground <= -30000.0 {
                self.die_request = true;
            }
        }

        if self.fallspeed < 0.0 && self.manground <= self.ground {
            // Landing after a fall: the harder, the deeper the dip.
            self.isfalling = false;
            if self.fallspeed < -13.333_333 {
                self.crouchtime240 = 60;
                self.crouchfall = -90.0;
            } else if self.fallspeed < -5.0 {
                self.crouchtime240 = 60;
                self.crouchfall = (-5.0 - self.fallspeed) * -90.0 / 8.333_333;
            }
            if self.fallspeed < -6.0 {
                self.landed = Some(self.fallspeed);
            }
            self.fallspeed = 0.0;
        }

        // The dip: hold crouchfall for crouchtime240, then let it back to 0.
        for _ in 0..lv.lvupdate240 {
            if self.crouchtime240 > 0 {
                self.sumcrouch = self.sumcrouch * 0.9456 + self.crouchfall;
                self.crouchtime240 -= 1;
            } else {
                if self.crouchfall < 0.0 {
                    self.crouchfall -= -1.125;
                    if self.crouchfall >= 0.0 {
                        self.crouchfall = 0.0;
                    }
                }
                self.sumcrouch = self.sumcrouch * 0.9456 + self.crouchfall;
            }
        }

        self.crouchheight = self.sumcrouch * 0.054_400_027;
        let vv_height = (self.headpos.y / self.standheight) * self.eyeheight;
        let mut eyeheight = vv_height + self.crouchoffsetrealsmall + self.crouchheight * self.eyeheight * 0.006_289_308;
        if eyeheight < 30.0 {
            eyeheight = 30.0;
        }
        self.pos.y = self.manground + eyeheight;
        if self.pos.y < self.ground + 10.0 {
            self.pos.y = self.ground + 10.0;
        }
    }
}
