//! Going somewhere (`chraction.c`): `chr_go_to_room_pos`, `chr_tick_gopos`
//! (arrival, the skip-ahead, the stuck and restart timers),
//! `chr_gopos_advance_waypoint`, `chr_run_from_pos` (with PD's bug),
//! `chr_try_stop`, and the arrival tests.
//!
//! `// SUBST:` PD steers along a route with `chr_nav_tick_main`'s "expensive"
//! mode, which steps round obstacles, other chrs included / a straight line
//! at the current point, `roty` snapped to it, which is what `chr_turn_toward`
//! does for a simulant once it has an aim point (`chraction.c:11481`). The
//! Complex spike ported the rest and measured no stalls without it on PD's
//! graph (SPIKE_PD_COMPLEX.md, stage 3). The off-screen "magic" mode is off in
//! multiplayer (`normmplayerisrunning`).
//!
//! Lifts: a route's `PADFLAG_AIWAITLIFT` pad (in front of a lift) and
//! `PADFLAG_AIONLIFT` pad (in it) come in pairs. The chr waits at the first
//! for the lift to come (calling it through the stop's door), steps on, waits
//! in it for the other stop, and steps off (`chr_gopos_update_lift_action`).
//!
//! Source: the old repo's `pd_spike/chraction.rs` (go-to), checked against
//! `reference/pd_bot_port_sheet.md` §12, which added the crouch flags' reset
//! each tick, the restart timer and PD's handling of a failed re-route.

use glam::{Vec2, Vec3};
use pd_core::ids::{LIFTACTION_NOTUSINGLIFT, LIFTACTION_ONLIFT, LIFTACTION_WAITINGFORLIFT, LIFTACTION_WAITINGONLIFT};
use pd_core::math::{atan2f, baddtor, turn};

use super::{Act, Chr, GoPos};
use crate::nav::{chrnavseed, PadFlags, MAX_CHRWAYPOINTS};
use crate::world::World;

/// `pos_is_moving_towards_pos_or_stopped_in_range` (`chraction.c:11646`).
fn pos_is_moving_towards_pos_or_stopped_in_range(prev: Vec3, moved: Vec3, target: Vec3, range: f32) -> bool {
    let pd = Vec2::new(target.x - prev.x, target.z - prev.z);
    if moved.x == 0.0 && moved.z == 0.0 {
        return pd.length_squared() <= range * range;
    }
    let tmp = moved.x * pd.x + moved.z * pd.y;
    if tmp > 0.0 {
        let sqmoved = moved.x * moved.x + moved.z * moved.z;
        let sqprev = pd.length_squared();
        return (sqprev - range * range) * sqmoved <= tmp * tmp;
    }
    false
}

/// `pos_is_arriving_laterally_at_pos` (`chraction.c:11682`).
pub fn pos_is_arriving_laterally_at_pos(prev: Vec3, cur: Vec3, target: Vec3, range: f32) -> bool {
    if prev.x <= target.x - range && cur.x <= target.x - range {
        return false;
    }
    if prev.x >= target.x + range && cur.x >= target.x + range {
        return false;
    }
    if prev.z <= target.z - range && cur.z <= target.z - range {
        return false;
    }
    if prev.z >= target.z + range && cur.z >= target.z + range {
        return false;
    }
    pos_is_moving_towards_pos_or_stopped_in_range(prev, Vec3::new(cur.x - prev.x, 0.0, cur.z - prev.z), target, range)
}

/// `pos_is_arriving_at_pos` (`chraction.c:11716`): arriving laterally, and
/// within 150 cm vertically.
pub fn pos_is_arriving_at_pos(prev: Vec3, cur: Vec3, target: Vec3, range: f32) -> bool {
    if prev.y <= target.y - 150.0 && cur.y <= target.y - 150.0 {
        return false;
    }
    if prev.y >= target.y + 150.0 && cur.y >= target.y + 150.0 {
        return false;
    }
    pos_is_arriving_laterally_at_pos(prev, cur, target, range)
}

impl Chr {
    /// `chr_gopos_is_waiting` (`chraction.c:1438`) for a simulant: standing,
    /// or on a go-to waiting at or in a lift.
    pub fn chr_gopos_is_waiting(&self) -> bool {
        self.actiontype == Act::Stand || (self.actiontype == Act::GoPos && self.act_gopos.waiting)
    }

    /// `chr_choose_stand_animation` (`chraction.c:1670`) for a simulant: a
    /// go-to starts waiting (a simulant has no stand animation to play).
    fn chr_choose_stand_animation(&mut self) {
        if self.actiontype == Act::GoPos {
            self.act_gopos.waiting = true;
        }
    }

    /// `chr_gopos_choose_animation` (`chraction.c:5862`) for a simulant: the
    /// go-to stops waiting.
    fn chr_gopos_choose_animation(&mut self) {
        if self.actiontype == Act::GoPos {
            self.act_gopos.waiting = false;
        }
    }
}

/// Go-to bookkeeping the A/B harness reads (not PD).
#[derive(Clone, Debug, Default)]
pub struct NavStats {
    /// `chr_tick_gopos`'s stuck-for-a-second re-routes.
    pub repaths: u32,
    /// `chr_go_to_room_pos` calls, and why the failed ones failed.
    pub gotos: u32,
    pub goto_no_start: u32,
    pub goto_no_end: u32,
    pub goto_no_route: u32,
    /// Where each stuck re-route happened: (the chr, the point it was heading for).
    pub repath_at: Vec<(Vec3, Vec3)>,
    /// Every request: the chr's position and rooms, and the destination.
    pub goto_log: Vec<(Vec3, Vec<u16>, Vec3)>,
    /// Simulants' rounds fired, and those that met a chr.
    pub rounds: u32,
    pub round_hits: u32,
}

impl World {
    /// `chr_go_to_room_pos` (`chraction.c:6124`), not magic: the waypoint nearest
    /// the chr and the one nearest `pos` (`waypoint_find_closest_to_pos`), then
    /// `nav_find_route` under the chr's nav seed into the 6-slot array. PD
    /// starts the go-to when the count is > 1, and that count includes the NULL
    /// terminator (`padhalllv.c:668`): **at least one** waypoint.
    pub(crate) fn chr_go_to_room_pos(&mut self, i: usize, pos: Vec3, endrooms: &[u16]) -> bool {
        if self.chr_is_dead(i) {
            return false;
        }
        let endrooms = endrooms.to_vec();
        let prop = self.chrs[i].pos;
        let rooms = self.chrs[i].rooms.clone();
        if self.record_gotos {
            self.navstats.goto_log.push((prop, rooms.clone(), pos));
        }
        let next = self.nav.waypoint_find_closest_to_pos(&self.level, prop, &rooms);
        let last = self.nav.waypoint_find_closest_to_pos(&self.level, pos, &endrooms);
        let endrooms_kept = endrooms.clone();
        self.navstats.gotos += 1;
        if next.is_none() {
            self.navstats.goto_no_start += 1;
        } else if last.is_none() {
            self.navstats.goto_no_end += 1;
        }
        let (route, numwaypoints) = match (next, last) {
            (Some(a), Some(b)) => {
                let seed = chrnavseed(self.lv.lvframe60, self.chrs[i].chrnum);
                self.nav.nav_find_route(a, b, MAX_CHRWAYPOINTS, seed, &mut self.rng)
            }
            _ => (Vec::new(), 0),
        };
        if next.is_some() && last.is_some() && numwaypoints <= 1 {
            self.navstats.goto_no_route += 1;
        }
        if numwaypoints > 1 {
            let age = (self.rng.random() % 100) as i32;
            let c = &mut self.chrs[i];
            c.actiontype = Act::GoPos;
            c.act_gopos = GoPos { endpos: pos, endrooms: endrooms_kept, waypoints: route, curindex: 0, target: last, init: true, crouch: false, duck: false, age, restartttl: 0, waiting: false };
            c.liftaction = LIFTACTION_NOTUSINGLIFT;
            // chr_gopos_init_expensive: the restart timer starts again.
            c.sleep = 0;
            return true;
        }
        false
    }

    /// `chr_go_to_pos` (`chraction.c:7279`): to a position, in the rooms it is
    /// in (or above, `bg_find_rooms_by_pos`).
    pub(crate) fn chr_go_to_pos(&mut self, i: usize, pos: Vec3) -> bool {
        let (inrooms, aboverooms, _) = self.stage.rooms.bg_find_rooms_by_pos(pos, 20);
        let rooms = if !inrooms.is_empty() { inrooms } else { aboverooms };
        if rooms.is_empty() {
            return false;
        }
        self.chr_go_to_room_pos(i, pos, &rooms)
    }

    /// `chr_gopos_advance_waypoint` (`chraction.c:5557`): the next loaded
    /// waypoint, or, once past the third slot, the route reloaded from the
    /// current waypoint to `target` under the chr's nav seed, from slot 1.
    fn chr_gopos_advance_waypoint(&mut self, i: usize) {
        let gp = &self.chrs[i].act_gopos;
        if gp.curindex < 3 {
            self.chrs[i].act_gopos.curindex += 1;
        } else {
            let from = gp.waypoints.get(gp.curindex).copied();
            let target = gp.target;
            self.chrs[i].act_gopos.curindex = 1;
            if let (Some(from), Some(target)) = (from, target) {
                let seed = chrnavseed(self.lv.lvframe60, self.chrs[i].chrnum);
                let (route, _) = self.nav.nav_find_route(from, target, MAX_CHRWAYPOINTS, seed, &mut self.rng);
                self.chrs[i].act_gopos.waypoints = route;
            }
        }
        // chr_gopos_init_expensive → chr_gopos_clear_restart_ttl.
        self.chrs[i].act_gopos.restartttl = 0;
    }

    /// `chr_run_from_pos` (`chraction.c:15690`), **with PD's bug**: the flee
    /// vector is never added back to the chr's position, so the destination is
    /// the point `away × rundist` in world coordinates, cut at the first wall
    /// on the way there.
    pub(crate) fn chr_run_from_pos(&mut self, i: usize, rundist: f32, frompos: Vec3) -> bool {
        if self.chr_is_dead(i) {
            return false;
        }
        let pp = self.chrs[i].pos;
        let mut delta = Vec3::new(pp.x - frompos.x, pp.y, pp.z - frompos.z);
        if delta.x == 0.0 || delta.z == 0.0 {
            return false;
        }
        let cur = (delta.x * delta.x + delta.z * delta.z).sqrt();
        delta.x *= rundist / cur;
        delta.z *= rundist / cur;
        // cd_test_los_oobok_findclosest(pos, delta, CDTYPE_ALL, GEOFLAG_WALL).
        let dir = delta - pp;
        if let Some(hit) = self.level.raycast_walls(pp, dir, dir.length()) {
            delta = hit.point;
        }
        self.chr_go_to_pos(i, delta)
    }

    /// `chr_try_stop` → `chr_stop` → `chr_stand_immediate(chr, 16)`: back to
    /// `ACT_STAND`, sleeping 16 ticks (the action tick waits; the brain runs).
    /// `wallcount = random() % 120 + 180` is drawn and never read by a simulant.
    pub(crate) fn chr_try_stop(&mut self, i: usize) -> bool {
        if self.chr_is_dead(i) {
            return false;
        }
        let _wallcount = self.rng.random() % 120 + 180;
        let c = &mut self.chrs[i];
        c.actiontype = Act::Stand;
        let mut fsleep = 16.0f32;
        if c.anim.playspeed != 1.0 {
            fsleep *= 1.0 / c.anim.playspeed;
        }
        c.sleep = fsleep.min(127.0) as i32;
        true
    }

    /// `chr_stand` (`chraction.c:1745`) for a standing human: `chr_stand_immediate(chr, 16)`,
    /// as [`Self::chr_try_stop`] does.
    pub(crate) fn chr_stand(&mut self, i: usize) -> bool {
        self.chr_try_stop(i)
    }

    /// The pad of loaded waypoint `k`, and its `PADFLAG_AI*` bits.
    fn gopos_pad(&self, i: usize, k: usize) -> Option<(Vec3, PadFlags)> {
        self.chrs[i].act_gopos.waypoints.get(k).map(|&w| (self.nav.waypoint_pos(w), self.nav.waypoint_flags(w)))
    }

    /// `chr_tick_gopos` (`chraction.c:12800`), not magic (magic is off in MP).
    /// Arrival is tested from `prop->pos` against the chr's position before
    /// this tick's animation (`chr->prevpos`).
    pub(crate) fn chr_tick_gopos(&mut self, i: usize) {
        let lvframe60 = self.lv.lvframe60;
        {
            let gp = &mut self.chrs[i].act_gopos;
            gp.crouch = false;
            gp.duck = false;
            gp.age += 1;
        }

        // Stuck for a second: route to the same destination again. A failed
        // route leaves the go-to as it was (PD does not stop the chr).
        if self.chrs[i].lastmoveok60 < lvframe60 - 60 {
            // SUBST: PD re-routes every tick until a move is clear, which its
            // obstacle stepping (`chr_nav_tick_main`'s expensive mode) makes rare
            // / our straight-line steering slides along walls far more (and a
            // slide doesn't count as a clear move), so a re-route restarts the
            // one-second clock, as in the spike.
            self.chrs[i].lastmoveok60 = lvframe60;
            let c = &self.chrs[i];
            let gp = &c.act_gopos;
            let aim = gp.waypoints.get(gp.curindex).map_or(gp.endpos, |&w| self.nav.waypoint_pos(w));
            self.navstats.repaths += 1;
            if self.record_gotos {
                self.navstats.repath_at.push((c.pos, aim));
            }
            let (end, endrooms) = (gp.endpos, gp.endrooms.clone());
            self.chr_go_to_room_pos(i, end, &endrooms);
        }
        self.chr_gopos_consider_restart(i);
        if self.chrs[i].actiontype != Act::GoPos {
            return;
        }

        let c = &self.chrs[i];
        let (prev, prop, inlift) = (c.prevpos, c.pos, c.inlift);
        let curindex = c.act_gopos.curindex;
        let mut advance = false;
        if let Some((padpos, flags)) = self.gopos_pad(i, curindex) {
            let arrivingxyz = pos_is_arriving_at_pos(prev, prop, padpos, 30.0);
            let arrivingxz = pos_is_arriving_laterally_at_pos(prev, prop, padpos, 30.0);
            let gp = &mut self.chrs[i].act_gopos;
            if flags.crouch {
                gp.crouch = true;
            } else if flags.duck {
                gp.duck = true;
            }
            if flags.lift() {
                let w = self.chrs[i].act_gopos.waypoints[curindex];
                let next = self.chrs[i].act_gopos.waypoints.get(curindex + 1).map(|&n| self.nav.waypoint_padnum(n));
                advance = self.chr_gopos_update_lift_action(i, flags, arrivingxz, arrivingxyz, self.nav.waypoint_padnum(w), next);
            } else if arrivingxyz || (arrivingxz && (inlift || flags.ignorey)) {
                advance = true;
            }
        } else {
            // No more waypoints: arriving at the end point finishes the go-to.
            let end = self.chrs[i].act_gopos.endpos;
            if pos_is_arriving_at_pos(prev, prop, end, 30.0) || (inlift && pos_is_arriving_laterally_at_pos(prev, prop, end, 30.0)) {
                self.chr_try_stop(i);
                return;
            }
        }
        if advance {
            self.chr_gopos_advance_waypoint(i);
        }

        let radius = self.chrs[i].radius;
        let point = |w: &World, k: usize| w.gopos_pad(i, k).map_or(w.chrs[i].act_gopos.endpos, |p| p.0);

        // Every 10 ticks: skip two waypoints if neither is PADFLAG_AIWALKDIRECT
        // and the one after them (or the end) can be run to straight.
        let (age, init) = (self.chrs[i].act_gopos.age, self.chrs[i].act_gopos.init);
        if age % 10 == 5 || init {
            let cur = self.chrs[i].act_gopos.curindex;
            if let (Some((_, f0)), Some((_, f1))) = (self.gopos_pad(i, cur), self.gopos_pad(i, cur + 1)) {
                if !f0.walkdirect && !f1.walkdirect {
                    let to = point(self, cur + 2);
                    if self.chr_prop_can_move_to_pos_without_nav(i, to, radius * 1.2) {
                        self.chr_gopos_advance_waypoint(i);
                        self.chr_gopos_advance_waypoint(i);
                    }
                }
            }
        }
        // Every 10 ticks: skip the current waypoint if the next (or the end) can
        // be run to straight. A PADFLAG_AIWALKDIRECT waypoint is skipped only on
        // the first tick, and only within 45° of the line through it.
        if age % 10 == 0 || init {
            let cur = self.chrs[i].act_gopos.curindex;
            if let Some((padpos, flags)) = self.gopos_pad(i, cur) {
                // Two lift pads in a row are never skipped.
                let nextlift = self.gopos_pad(i, cur + 1).is_some_and(|(_, f)| f.lift());
                let candosomething = init && !(flags.lift() && nextlift);
                if !flags.walkdirect || candosomething {
                    let nextpos = point(self, cur + 1);
                    if flags.walkdirect && candosomething {
                        let p = self.chrs[i].pos;
                        let (sp180, sp176, sp172, sp168) = (p.x - padpos.x, p.z - padpos.z, nextpos.x - padpos.x, nextpos.z - padpos.z);
                        let sp156 = ((sp180 * sp180 + sp176 * sp176) * (sp172 * sp172 + sp168 * sp168)).sqrt();
                        if sp156 > 0.0 {
                            let sp160 = ((sp180 * sp172 + sp176 * sp168) / sp156).clamp(-1.0, 1.0).acos();
                            if (sp160 < baddtor(45.0) || sp160 > baddtor(315.0)) && self.chr_prop_can_move_to_pos_without_nav(i, nextpos, radius * 1.2) {
                                self.chr_gopos_advance_waypoint(i);
                            }
                        }
                    } else if self.chr_prop_can_move_to_pos_without_nav(i, nextpos, radius * 1.2) {
                        self.chr_gopos_advance_waypoint(i);
                    }
                }
            }
            self.chrs[i].act_gopos.init = false;
        }

        // chr_nav_tick_main (SUBST, see the module): face the current point,
        // and every 10 ticks open a door in the way (chraction.c:12586).
        let cur = self.chrs[i].act_gopos.curindex;
        let target = point(self, cur);
        if age % 10 == 0 {
            self.chr_open_door(i, target);
        }
        let c = &mut self.chrs[i];
        let d = Vec2::new(target.x - c.pos.x, target.z - c.pos.z);
        if d.length_squared() > 1e-6 {
            let mut a = atan2f(d.x, d.y);
            if a >= turn() {
                a -= turn();
            }
            if let Some(ab) = c.aibot.as_mut() {
                ab.roty = a;
            }
        }
    }

    /// `chr_gopos_update_lift_action` (`chraction.c:12652`), NTSC final in
    /// multiplayer: at a `PADFLAG_AIWAITLIFT` pad before an `AIONLIFT` one, wait
    /// (calling the lift through the door towards the next pad) until the lift
    /// is at most 40 cm above the chr's feet and its door, if any, half open;
    /// at an `AIONLIFT` pad before an `AIWAITLIFT` one, wait in it until it is
    /// at most 30 cm below the next pad's floor (and its door half open).
    /// True: go on to the next waypoint. `arg2` is arriving at the pad
    /// laterally, `arrivingatlift` arriving at it.
    fn chr_gopos_update_lift_action(&mut self, i: usize, curpadflags: PadFlags, arg2: bool, arrivingatlift: bool, curpadnum: usize, nextpadnum: Option<usize>) -> bool {
        let Some(liftid) = self.lift_find_by_pad(curpadnum) else { return false };
        let Some(li) = self.props.objs.iter().position(|o| o.id == liftid && o.lift.is_some()) else { return false };
        let lifty = crate::props::lift::lift_get_y(&self.props.objs[li]);
        let l = self.props.objs[li].lift.as_ref().unwrap();
        // The door at the lift's current stop is at least halfway open.
        let doorblocks = l.doors[l.levelcur].and_then(|d| self.props.get(d)).and_then(|o| o.door.as_ref()).is_some_and(|d| d.frac < 0.5);
        let nextpad = nextpadnum.map(|n| self.stage.pads[n].clone());
        let nextflags = nextpad.as_ref().map_or(0, |p| p.flags);
        let mut advance = false;
        let c = &self.chrs[i];
        let (liftaction, manground) = (c.liftaction, c.manground);
        if curpadflags.waitlift {
            if nextflags & pd_core::ids::PADFLAG_AIONLIFT != 0 {
                if arrivingatlift || liftaction == LIFTACTION_WAITINGFORLIFT {
                    // Begin entering the lift if it is under 40 cm above this
                    // level (the solo check that it is over 1 m under it is
                    // skipped: MP simulants may drop onto lifts).
                    advance = lifty <= manground + 40.0 && !doorblocks;
                }
                if !advance {
                    if arrivingatlift && liftaction != LIFTACTION_WAITINGFORLIFT {
                        // Just arrived at the lift: wait, and call it.
                        self.chrs[i].liftaction = LIFTACTION_WAITINGFORLIFT;
                        self.chrs[i].chr_choose_stand_animation();
                        if let Some(p) = &nextpad {
                            self.chr_open_door(i, p.pos);
                        }
                    }
                } else {
                    // Enter the lift.
                    let c = &mut self.chrs[i];
                    c.liftaction = LIFTACTION_NOTUSINGLIFT;
                    if c.chr_gopos_is_waiting() {
                        c.chr_gopos_choose_animation();
                    }
                }
            } else if arrivingatlift {
                // Running past the lift without using it.
                advance = true;
                self.chrs[i].liftaction = LIFTACTION_NOTUSINGLIFT;
            }
        } else if curpadflags.onlift {
            if nextflags & pd_core::ids::PADFLAG_AIWAITLIFT != 0 {
                // Waiting for the door to close or the lift to arrive.
                if arg2 || liftaction == LIFTACTION_WAITINGONLIFT {
                    let p = nextpad.as_ref().unwrap();
                    let rooms: Vec<u16> = p.room.into_iter().collect();
                    let nextground = self.level.cd_find_ground_at_pos_ct(p.pos, &rooms);
                    // Begin leaving once the lift is at most 30 cm under the
                    // destination (no solo upper limit).
                    advance = lifty >= nextground - 30.0 && !doorblocks;
                }
                if !advance {
                    if arg2 && liftaction != LIFTACTION_WAITINGONLIFT {
                        // Just arrived inside the lift.
                        self.chrs[i].liftaction = LIFTACTION_WAITINGONLIFT;
                        self.chrs[i].chr_choose_stand_animation();
                    }
                } else {
                    // Start disembarking.
                    let c = &mut self.chrs[i];
                    c.liftaction = LIFTACTION_ONLIFT;
                    if c.chr_gopos_is_waiting() {
                        c.chr_gopos_choose_animation();
                    }
                }
            } else if arg2 {
                advance = true;
                self.chrs[i].liftaction = LIFTACTION_ONLIFT;
            }
        }
        advance
    }

    /// `chr_gopos_consider_restart` (`chraction.c:5504`): a leg that takes twice
    /// its running time plus 5 s restarts (`bot_check_fetch`: a simulant
    /// fetching gives up, else route again).
    fn chr_gopos_consider_restart(&mut self, i: usize) {
        let lv60 = self.lv.lvupdate60 as u16;
        let c = &self.chrs[i];
        if c.liftaction == LIFTACTION_WAITINGONLIFT || c.liftaction == LIFTACTION_WAITINGFORLIFT {
            return;
        }
        if c.act_gopos.restartttl == 0 {
            // chr_gopos_calculate_base_ttl (`chraction.c:5464`).
            let pos = self.gopos_pad(i, c.act_gopos.curindex).map_or(c.act_gopos.endpos, |p| p.0);
            let (xdiff, zdiff) = ((pos.x - c.pos.x).abs(), (pos.z - c.pos.z).abs());
            let speed = crate::bot::bot_calculate_max_speed(c).max(0.001);
            let base = ((xdiff + zdiff) / speed) as i32;
            let value = (base * 2 + 300).min(0xffff);
            self.chrs[i].act_gopos.restartttl = value as u16;
        } else if c.act_gopos.restartttl <= lv60 {
            if c.aibot.is_some() {
                self.bot_check_fetch(i);
            } else {
                let (end, endrooms) = (c.act_gopos.endpos, c.act_gopos.endrooms.clone());
                self.chr_go_to_room_pos(i, end, &endrooms);
            }
        } else {
            self.chrs[i].act_gopos.restartttl -= lv60;
        }
    }
}
