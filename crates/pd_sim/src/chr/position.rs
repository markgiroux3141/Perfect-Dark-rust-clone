//! Moving a chr (`chr.c`): `chr_update_position` (the body model's `unk70`
//! callback, run by `model_update_chr_info` after the animation ticks),
//! `chr_calculate_push_pos` (slide along what it hits), `chr_ascend`, the fall
//! (`projectile_update_fall`), the ground and ladders, the duck/squat heights,
//! and `chr_prop_can_move_to_pos_without_nav` (the go-to's skip-ahead).
//!
//! A simulant's horizontal travel is its velocity (`bot_update_lateral`), not
//! its animation's root motion; only the root's height survives, as
//! `prop->pos.y − manground`.
//!
//! `// SUBST:` PD's NTSC guard that holds an off-screen simulant about to
//! fall out of the world at a low frame rate (`forceslowupdates`,
//! `lvupdate60 >= 5`) never triggers at 60/30/20 Hz, so it is left out.
//!
//! Source: the old repo's `pd_spike/chraction.rs` (position), checked against
//! `reference/pd_bot_port_sheet.md` §11.

use pd_core::ids::*;
use glam::{Vec2, Vec3};
use pd_core::ids::FLOORTYPE_METAL;

use super::Act;
use crate::stage::{CdResult, PropFloor, PropGeo, TileFlag};
use crate::world::World;

/// `g_HeadAnims[HEADANIM_MOVING].translateperframe`: `bhead_reset`
/// (`bondheadreset.c:90`) from `ANIM_0029`'s root z motion over frames 7..16,
/// `1790 × 0.1 / 9.5`. The unit of the shove.
pub const TRANSLATE_PER_FRAME_MOVING: f32 = 18.842_107;

/// `projectile_update_fall` (`projectile.c:34`): gravity 0.2778 cm/tick².
pub fn projectile_update_fall(yincrement: &mut f32, speed: &mut f32, lvupdate60: f32) {
    let s = *speed - lvupdate60 * 0.277_777_79;
    *yincrement += lvupdate60 * (*speed + s) * 0.5;
    *speed = s;
}

/// `prop_is_of_cd_type` (`prop.c:2788`) for an object or a door with
/// geometry (a gone or deleting one has none). A door: not a see-through one
/// for `CDTYPE_AIOPAQUE`, and unless `CDTYPE_DOORS` asks for every door, one
/// in the state the types name (`prop_door_get_cd_types`). An object: not a
/// see-through one for `CDTYPE_AIOPAQUE`, a mortal one for the
/// `CDTYPE_OBJSIMMUNETO*` types, and a path blocker only for
/// `CDTYPE_PATHBLOCKER`, anything else only for `CDTYPE_OBJS`.
pub(crate) fn obj_is_of_cd_type(o: &crate::props::Obj, types: u32) -> bool {
    if o.geos.is_empty() || o.is_gone() || o.is_deleting() {
        return false;
    }
    if types & CDTYPE_AIOPAQUE != 0 && o.flags & OBJFLAG_AISEETHROUGH != 0 {
        return false;
    }
    if let Some(d) = &o.door {
        if types & CDTYPE_DOORSWITHOUTFLAG != 0 && o.flags3 & OBJFLAG3_80000000 != 0 {
            return false;
        }
        return types & CDTYPE_DOORS != 0 || d.cd_types(o.flags2) & types != 0;
    }
    let invincible = o.flags & OBJFLAG_INVINCIBLE != 0;
    if types & CDTYPE_OBJSIMMUNETOGUNFIRE != 0 && !invincible && o.flags2 & OBJFLAG2_IMMUNETOGUNFIRE == 0 {
        return false;
    }
    if types & CDTYPE_OBJSIMMUNETOEXPLOSIONS != 0 && !invincible && o.flags2 & OBJFLAG2_IMMUNETOEXPLOSIONS == 0 {
        return false;
    }
    let class = if o.flags & OBJFLAG_PATHBLOCKER != 0 { CDTYPE_PATHBLOCKER } else { CDTYPE_OBJS };
    types & class != 0
}

impl World {
    /// What a chr moving with `CDTYPE_ALL` walks into besides the BG: every
    /// other chr's perimeter (`chr_set_perim_enabled(chr, false)` leaves out its
    /// own), players included, then the objects' geometry.
    pub(crate) fn chr_perims_except(&self, i: usize) -> Vec<PropGeo> {
        let mut v: Vec<PropGeo> = self.chrs.iter().enumerate().filter(|&(j, _)| j != i).filter_map(|(_, c)| c.perim()).collect();
        v.extend(self.obj_geos(CDTYPE_ALL));
        v
    }

    /// Every prop's floor (`GEOTYPE_TILE_F` with `GEOFLAG_FLOOR1 | FLOOR2`: the
    /// lifts' and the objects' floor quads) for the ground search
    /// (`cd_volume_collect`'s props, `CDTYPE_ALL`).
    pub(crate) fn prop_floors(&self) -> Vec<PropFloor> {
        self.props.objs.iter().filter(|o| !o.floors.is_empty() && !o.is_gone() && !o.is_deleting()).flat_map(|o| o.floors.iter().copied()).collect()
    }

    /// The walls of the objects of `types` (`prop_is_of_cd_type`,
    /// `prop.c:2788`): see [`obj_is_of_cd_type`].
    pub(crate) fn obj_geos(&self, types: u32) -> Vec<PropGeo> {
        self.props.objs.iter().filter(|o| obj_is_of_cd_type(o, types)).flat_map(|o| o.geos.iter().copied()).collect()
    }

    /// [`Self::obj_geos`] with each block's object id.
    pub(crate) fn obj_geos_with_ids(&self, types: u32) -> Vec<(u32, PropGeo)> {
        self.props.objs.iter().filter(|o| obj_is_of_cd_type(o, types)).flat_map(|o| o.geos.iter().map(move |g| (o.id, *g))).collect()
    }

    /// `chr_update_position` (`chr.c:521`), the non-absolute-animation path of a
    /// simulant: `arg1` is the root before (`rwdata->chrinfo.pos`), `arg2` the
    /// animation's proposed root with its y relative to the ground, `ground`
    /// `rwdata->chrinfo.ground`, set to the chr's `manground`. Returns whether
    /// `CHRCFLAG_FORCETOGROUND` was applied (the caller copies the root motion's
    /// next height into the current one, `chr.c:880`).
    pub(crate) fn chr_update_position(&mut self, i: usize, arg1: Vec3, arg2: &mut Vec3, ground_out: &mut f32) -> bool {
        let lv = self.lv.clone();
        let freal = lv.lvupdate60freal;
        let cyls = self.chr_perims_except(i);
        let floors = self.prop_floors();
        let level = self.level.clone();
        // A player's body (`PROPTYPE_PLAYER`): its ground and floor are the
        // player's (`chr.c:823`: `vv_manground`, `floortype`).
        let player = self.chrs[i].player.map(|p| (self.players[p].manground, self.players[p].floortype));
        let c = &mut self.chrs[i];
        let manground = c.manground;
        let mut yincrement = 0.0f32;
        arg2.y += manground;
        if c.aibot.is_some() {
            let mv = if lv.lvupdate240 > 0 { crate::bot::bot_update_lateral(c, lv.lvframe60, lv.lvupdate240, freal) } else { Vec2::ZERO };
            arg2.x = arg1.x + mv.x;
            arg2.z = arg1.z + mv.y;
        }
        // chr.c:618: a chr on a go-to touching a ladder climbs it.
        c.onladder = c.actiontype == Act::GoPos && level.cd_find_ladder(c.pos, c.radius * 2.5, c.manground + c.height - c.pos.y, c.manground + 1.0 - c.pos.y).is_some();
        if c.aibot.is_some() {
            // chr.c:628: the height, from the go-to's pad flags and the tiles.
            c.height = 185.0;
            if c.actiontype == Act::GoPos && c.act_gopos.duck {
                c.height = 135.0;
            } else if c.actiontype == Act::GoPos && c.act_gopos.crouch {
                c.height = 90.0;
            } else if level.is_cyl_touching_tile_with_flags(TileFlag::Duck, c.pos, c.radius * 1.1, c.manground + 185.0 - c.pos.y, c.manground - 10.0 - c.pos.y) {
                c.height = 135.0;
            } else if level.is_cyl_touching_tile_with_flags(TileFlag::Crouch, c.pos, c.radius * 1.1, c.manground + 135.0 - c.pos.y, c.manground - 10.0 - c.pos.y) {
                c.height = 90.0;
            }
            let a = c.aibot.as_mut().unwrap();
            crate::player::bmove_dampen_shotspeed(&mut a.shotspeed, freal);
            arg2.x += a.shotspeed.x * TRANSLATE_PER_FRAME_MOVING * freal * 0.5;
            arg2.z += a.shotspeed.z * TRANSLATE_PER_FRAME_MOVING * freal * 0.5;
        }
        // An unarmed punch's push (`chr_damage`: timeextra, extraspeed).
        if c.timeextra > 0.0 {
            let speed = c.anim.playspeed * lv.lvupdate60f * (c.timeextra - c.elapseextra) / c.timeextra;
            arg2.x += c.extraspeed.x * speed;
            arg2.z += c.extraspeed.z * speed;
            yincrement += c.extraspeed.y * speed;
            c.elapseextra += lv.lvupdate60f * c.anim.playspeed;
            if c.elapseextra > c.timeextra {
                c.timeextra = 0.0;
            }
        }
        arg2.x += c.fallspeed.x * freal;
        arg2.z += c.fallspeed.z * freal;
        if c.onladder {
            // chr.c:753: on a ladder the whole lateral move becomes climb (≤ 100 cm).
            let (xdiff, zdiff) = (arg2.x - arg1.x, arg2.z - arg1.z);
            arg2.x = arg1.x;
            arg2.z = arg1.z;
            yincrement += (xdiff * xdiff + zdiff * zdiff).sqrt().min(100.0);
            c.floortype = FLOORTYPE_METAL;
        }
        // chr.c:745: a player's body stays at the player's position.
        let playerbody = c.player.is_some() && c.actiontype == Act::BondMulti;
        if playerbody {
            arg2.x = c.pos.x;
            arg2.z = c.pos.z;
            c.invalidmove = 0;
            c.lastmoveok60 = lv.lvframe60;
        } else {
            self.chr_calculate_push_pos(i, arg2, &cyls);
        }

        let mut forced = false;
        let mut die = false;
        let c = &mut self.chrs[i];
        if c.onladder {
            // chr.c:806: climb if the cylinder fits there.
            if Self::chr_ascend(&level, c, *arg2, yincrement, &cyls) {
                c.manground += yincrement;
                arg2.y += yincrement;
            }
            c.sumground = c.manground * 9.999_998;
            c.ground = c.manground;
            arg2.y -= c.manground;
        } else {
            // chr.c:830: probe from 69 above manground, the step height.
            let (ground, floorflags) = if let Some((vv_manground, floortype)) = player {
                c.floortype = floortype;
                (vv_manground, 0)
            } else {
                let probe = if arg2.y - manground < 69.0 { Vec3::new(arg2.x, manground + 69.0, arg2.z) } else { *arg2 };
                let g = level.cd_find_ground_at_cyl_ctfril(probe, c.radius, &floors);
                c.floorroom = g.room;
                c.floortype = g.floortype;
                c.lift = g.lift();
                c.inlift = c.lift.is_some();
                (g.y.max(-100_000.0), g.flags)
            };
            c.ground = ground;
            if c.forcetoground {
                arg2.y += yincrement + c.ground - manground;
                c.forcetoground = false;
                c.manground = c.ground;
                c.sumground = c.ground * 9.999_998;
                forced = true;
            } else {
                if c.fallspeed.y != 0.0 || c.ground < c.manground {
                    if c.player.is_none() && c.manground <= -30_000.0 {
                        die = true;
                    }
                    let mut fallspeed = c.fallspeed.y;
                    projectile_update_fall(&mut yincrement, &mut fallspeed, freal);
                    if Self::chr_ascend(&level, c, *arg2, yincrement, &cyls) {
                        c.manground += yincrement;
                        c.fallspeed.y = fallspeed;
                    }
                    if c.manground <= c.ground {
                        c.manground = c.ground;
                        c.sumground = c.ground * 9.999_998;
                        c.fallspeed.y = 0.0;
                        // Landing on a GEOFLAG_DIE tile kills.
                        if floorflags & GEOFLAG_DIE != 0 {
                            die = true;
                        }
                    }
                } else if c.manground <= c.ground {
                    for _ in 0..lv.lvupdate60 {
                        c.sumground = c.sumground * 0.9 + c.ground;
                        c.fallspeed.x *= 0.9;
                        c.fallspeed.z *= 0.9;
                    }
                    c.manground = c.sumground * 0.100_000_024;
                    if c.manground < c.ground - 30.0 {
                        c.manground = c.ground - 30.0;
                        c.sumground = (c.ground - 30.0) * 9.999_998;
                    }
                    if c.fallspeed.x < 0.1 && c.fallspeed.x > -0.1 && c.fallspeed.z < 0.1 && c.fallspeed.z > -0.1 {
                        c.fallspeed.z = 0.0;
                        c.fallspeed.x = 0.0;
                    }
                }
                if manground != c.manground {
                    arg2.y += c.manground - manground;
                }
            }
            arg2.y -= c.manground;
        }
        *ground_out = c.manground;
        let oldpos = c.pos;
        c.pos = Vec3::new(arg2.x, arg2.y + c.manground, arg2.z);
        if playerbody {
            // The body keeps the player's rooms (`rooms_copy(prop->rooms, spfc)`).
            return forced;
        }
        // prop->rooms: the rooms the move ends in (`los_find_final_room_exhaustive`),
        // cut to the floor's room when that is one of them (`chr.c:1004`), then
        // those the chr's box enters (`chr_detect_rooms`, `chr.c:1934`: ±50 cm
        // across, ±110 cm up and down, through open portals).
        let (mut rooms, _) = self.stage.rooms.portal_find_rooms(oldpos, self.chrs[i].pos, &self.chrs[i].rooms);
        let c = &self.chrs[i];
        // (A chr with no rooms yet takes its floor's.)
        if let Some(fr) = c.floorroom.filter(|fr| rooms.contains(fr) || rooms.is_empty()) {
            rooms = vec![fr];
        }
        let pos = c.pos;
        self.stage.rooms.bg_find_entered_rooms(pos - Vec3::new(50.0, 110.0, 50.0), pos + Vec3::new(50.0, 110.0, 50.0), &mut rooms, 7, true, &self.portalflags);
        let c = &mut self.chrs[i];
        c.rooms = rooms;
        if die && c.aibot.is_some() {
            let shooter = if c.lastshooter.is_some() && c.timeshooter > 0 { c.lastshooter } else { Some(i) };
            self.chr_die(i, shooter);
        }
        forced
    }

    /// `chr_calculate_push_pos` (`chr.c:204`): try the move; if something is in
    /// the way, slide along the edge that was hit (method 1), else round that
    /// edge's nearer end (method 2), else stay put. Moves larger than half the
    /// radius on either axis are swept first.
    fn chr_calculate_push_pos(&mut self, i: usize, dst: &mut Vec3, cyls: &[PropGeo]) {
        let lvframe60 = self.lv.lvframe60;
        let level = &self.level;
        let c = &mut self.chrs[i];
        let prop = c.pos;
        let radius = c.radius;
        let halfradius = radius * 0.5;
        let (ymax, ymin) = c.bbox_rel();
        let big = |to: Vec3| {
            let (mx, mz) = (to.x - prop.x, to.z - prop.z);
            mx > halfradius || mz > halfradius || mx < -halfradius || mz < -halfradius
        };
        let (cdresult, edge) = if big(*dst) {
            match level.cd_test_cylmove_oobfail_findclosest(prop, *dst, radius, ymax, ymin, cyls) {
                (CdResult::NoCollision, _) => level.cd_test_volume_closestedge(prop, *dst, radius, ymax, ymin, cyls),
                other => other,
            }
        } else {
            level.cd_test_volume_closestedge(prop, *dst, radius, ymax, ymin, cyls)
        };
        // The re-test each candidate gets (`chr.c:354-380`).
        let clear = |sp44: Vec3| -> bool {
            let r = if big(sp44) {
                match level.cd_test_cylmove_oobfail(prop, sp44, radius, ymax, ymin, cyls) {
                    CdResult::NoCollision => level.cd_test_volume_simple(sp44, radius, true, ymax, ymin, cyls),
                    r => r,
                }
            } else {
                level.cd_test_volume_simple(sp44, radius, true, ymax, ymin, cyls)
            };
            r == CdResult::NoCollision
        };
        let mut moveok = false;
        match cdresult {
            CdResult::Error => {}
            CdResult::NoCollision => {
                c.invalidmove = 0;
                c.lastmoveok60 = lvframe60;
                moveok = true;
            }
            CdResult::Collision => {
                let (sp78, sp6c) = edge.unwrap_or((prop, prop));
                let sp60 = Vec2::new(dst.x - prop.x, dst.z - prop.z);
                // Method 1: project the move onto the edge.
                if sp78.x != sp6c.x || sp78.z != sp6c.z {
                    let mut sp54 = Vec2::new(sp6c.x - sp78.x, sp6c.z - sp78.z);
                    sp54 *= 1.0 / (sp54.x * sp54.x + sp54.y * sp54.y).sqrt();
                    let value = sp60.x * sp54.x + sp60.y * sp54.y;
                    let sp44 = Vec3::new(sp54.x * value + prop.x, dst.y, sp54.y * value + prop.z);
                    if clear(sp44) {
                        dst.x = sp44.x;
                        dst.z = sp44.z;
                        c.invalidmove = 2;
                        moveok = true;
                    }
                }
                // Method 2: the destination within a radius of one of the edge's
                // ends: project the move onto the perpendicular to (end − pos).
                if !moveok {
                    let corner = |vtx: Vec3| -> Option<Vec3> {
                        if vtx.x == prop.x && vtx.z == prop.z {
                            return None;
                        }
                        let mut sp54 = Vec2::new(-(vtx.z - prop.z), vtx.x - prop.x);
                        sp54 *= 1.0 / (sp54.x * sp54.x + sp54.y * sp54.y).sqrt();
                        let value = sp60.x * sp54.x + sp60.y * sp54.y;
                        Some(Vec3::new(sp54.x * value + prop.x, dst.y, sp54.y * value + prop.z))
                    };
                    let near = |vtx: Vec3| {
                        let (x, z) = (vtx.x - dst.x, vtx.z - dst.z);
                        x * x + z * z <= radius * radius
                    };
                    let cand = if near(sp78) {
                        corner(sp78)
                    } else if near(sp6c) {
                        corner(sp6c)
                    } else {
                        None
                    };
                    if let Some(sp44) = cand {
                        if clear(sp44) {
                            dst.x = sp44.x;
                            dst.z = sp44.z;
                            c.invalidmove = 2;
                            moveok = true;
                        }
                    }
                }
            }
        }
        if !moveok {
            dst.x = prop.x;
            dst.z = prop.z;
            c.invalidmove = 1;
        }
    }

    /// `chr_ascend` (`chr.c:486`): may the chr's cylinder move `amount`
    /// vertically from `pos` without meeting a wall?
    fn chr_ascend(level: &crate::stage::TileLevel, c: &super::Chr, pos: Vec3, amount: f32, cyls: &[PropGeo]) -> bool {
        let (ymax, ymin) = c.bbox_rel();
        level.cd_test_volume_simple(pos + Vec3::Y * amount, c.radius, true, ymax, ymin, cyls) == CdResult::NoCollision
    }

    /// `chr_prop_can_move_to_pos_without_nav(chr, &prop->pos, prop->rooms, topos,
    /// torooms, NULL, turndist, CDTYPE_PATHBLOCKER | CDTYPE_BG)` (`chraction.c:5173`):
    /// a clear swept line to `topos` (`oobfail`: the go-to passes the rooms),
    /// and two more offset `turndist` to either side (room to turn). Walls only.
    pub(crate) fn chr_prop_can_move_to_pos_without_nav(&self, i: usize, topos: Vec3, turndist: f32) -> bool {
        let c = &self.chrs[i];
        let frompos = c.pos;
        let (ymax, ymin) = c.bbox_rel();
        let level = &self.level;
        if level.cd_test_cylmove_oobfail(frompos, topos, c.radius, ymax, ymin, &[]) == CdResult::Collision {
            return false;
        }
        let d = Vec2::new(topos.x - frompos.x, topos.z - frompos.z);
        if d.x == 0.0 && d.y == 0.0 {
            return true;
        }
        let d = d * (1.0 / (d.x * d.x + d.y * d.y).sqrt());
        let (tx, tz) = (d.x * turndist, d.y * turndist);
        for side in [1.0f32, -1.0] {
            let nf = Vec3::new(frompos.x + tz * side, frompos.y, frompos.z - tx * side);
            let nt = Vec3::new(topos.x + tz * side, topos.y, topos.z - tx * side);
            if level.cd_test_cylmove_oobok(frompos, nf, ymax, ymin, &[]) == CdResult::Collision || level.cd_test_cylmove_oobok(nf, nt, ymax, ymin, &[]) == CdResult::Collision {
                return false;
            }
        }
        true
    }
}
