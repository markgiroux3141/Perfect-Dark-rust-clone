//! Shots (`prop.c`): `hands_tick_attack`, `shot_create`, `shot_calculate_hits`
//! and what a hit does (`chr_hit` / `obj_hit`, the BG hit: bullet hole, hit
//! sounds, sparks, the one-player bullet-hole flame), melee, and turning the
//! gun's queued events into world effects (tracers, casings, smoke, sounds).
//!
//! A shot leaves the eye (`cam_get_projection_mtxf` × the camera-space spread
//! direction), finds the first BG triangle ([`crate::stage::BgHitMesh`]), then
//! the props in front of it; the first `penetration` props that slow it take
//! the hit, and the BG takes it if nothing stopped the round.
//!
//! What a shot can hit: the BG, the firing range's boards, the objects the
//! guns put in the world (`obj_test_hit`: a shot mine or grenade goes off, a
//! shot sentry breaks; the pickups on the pads, sparks and a thump) and the
//! simulants, by part box (`chr_test_hit`, `chr_hit`).
//!
//! Another player's body is a chr like a simulant (posed by
//! `player_tick_third_person`); a player's own never meets its shots.
//!
//! Source: the shot half of the old repo's `pd_guns/sim.rs`.

use glam::Vec3;
use pd_core::events::Event;
use pd_core::ids::*;

use super::GunEvent;
use crate::fx::beam::BEAM_LASERSTREAM;
use crate::fx::{casing, wallhit};
use crate::propsnd::DEFAULT_DISTS;
use crate::stage::bghit::{surface_type, SURFACETYPE_DEEPWATER, SURFACETYPE_SHALLOWWATER};
use crate::world::World;

/// A sound handle a hand keeps (`hand->audiohandle`), unique per player.
pub fn hand_sound_handle(player: usize, hand: usize) -> u32 {
    (player * 2 + hand) as u32
}

/// What a round met besides the BG.
#[derive(Clone, Copy, Debug)]
enum ShotTarget {
    Board(usize),
    /// One of the guns' objects, by id.
    Obj(u32),
    /// A chr, by index, and where on it.
    Chr(usize, crate::chr::body::ChrHit),
}

/// `player->lookingatprop.prop`: what the crosshair is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AimedAt {
    Board(usize),
    Chr(usize),
    /// An object, by id (the Threat Detector's explosives and sentries).
    Obj(u32),
}

/// A shot's round meeting a prop, `t` along the ray.
#[derive(Clone, Copy, Debug)]
struct PropHit {
    target: ShotTarget,
    t: f32,
    pos: Vec3,
    normal: Vec3,
}

/// Slab test: the entry distance along `o + d·t` into the box, and the entry
/// face's normal. `None` from inside or when it misses.
pub(crate) fn ray_box(min: Vec3, max: Vec3, o: Vec3, d: Vec3, tmax: f32) -> Option<(f32, Vec3)> {
    let (mut t0, mut t1) = (0.0f32, tmax);
    let mut n = Vec3::ZERO;
    for a in 0..3 {
        let (oa, da, lo, hi) = (o[a], d[a], min[a], max[a]);
        if da.abs() < 1e-9 {
            if oa < lo || oa > hi {
                return None;
            }
            continue;
        }
        let (mut ta, mut tb) = ((lo - oa) / da, (hi - oa) / da);
        let mut na = Vec3::ZERO;
        na[a] = -1.0;
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
            na = -na;
        }
        if ta > t0 {
            t0 = ta;
            n = na;
        }
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    (n != Vec3::ZERO).then_some((t0, n))
}

impl World {
    /// `hands_tick_attack` (`prop.c:1392`).
    pub(crate) fn hands_tick_attack(&mut self, pi: usize) {
        if self.lv.lvupdate240 > 0 {
            self.hand_tick_attack(pi, HAND_RIGHT);
            self.hand_tick_attack(pi, HAND_LEFT);
        }
    }

    /// `hand_tick_attack` (`prop.c:1285`).
    fn hand_tick_attack(&mut self, pi: usize, h: usize) {
        if self.players[pi].gun.hands[h].unk0d0f_02 {
            // A punch that met no chr: next tick it tries the BG
            // (prop_find_aiming_at(handnum, true, FINDPROPCONTEXT_SHOOT)).
            let gun = &self.players[pi].gun;
            let doit = !(gun.bgun_get_weapon_num(h) == WEAPON_REAPER && gun.hands[h].burstbullets % 3 != 1);
            if doit {
                self.prop_find_aiming_at(pi, h, true, true);
            }
            self.players[pi].gun.hands[h].unk0d0f_02 = false;
        }
        if !self.players[pi].gun.hands[h].firing {
            return;
        }
        let weaponnum = self.players[pi].gun.bgun_get_weapon_num(h);
        self.players[pi].gun.hands[h].activatesecondary = false;
        let (gsetnum, gsetfunc) = (self.players[pi].gun.hands[h].weaponnum, self.players[pi].gun.hands[h].weaponfunc);
        match self.players[pi].gun.hands[h].attacktype {
            HANDATTACKTYPE_SHOOT => {
                // Always the right hand; the left only if the right isn't
                // firing this tick (no two guns on one tick).
                if h == HAND_RIGHT || !self.players[pi].gun.hands[HAND_RIGHT].firing {
                    self.chr_uncloak_temporarily(pi);
                    self.mpstats_increment_player_shotcount(pi, gsetnum, SHOTREGION_TOTAL);
                    if weaponnum == WEAPON_SHOTGUN {
                        for _ in 0..6 {
                            self.shot_create(pi, h, true, true, 1);
                        }
                    } else {
                        let n = self.players[pi].gun.hands[h].shotstotake;
                        self.shot_create(pi, h, true, true, n);
                    }
                    self.mpstats_end_shot();
                }
            }
            HANDATTACKTYPE_MELEE => {
                self.chr_uncloak_temporarily(pi);
                self.hand_inflict_melee_damage(pi, h, false);
            }
            HANDATTACKTYPE_MELEENOUNCLOAK => self.hand_inflict_melee_damage(pi, h, true),
            HANDATTACKTYPE_DETONATE => self.player_activate_remote_mine_detonator(pi),
            HANDATTACKTYPE_BOOST => self.bgun_apply_boost(),
            HANDATTACKTYPE_REVERTBOOST => self.bgun_revert_boost(),
            HANDATTACKTYPE_SHOOTPROJECTILE => self.bgun_create_fired_projectile(pi, h),
            HANDATTACKTYPE_CROUCH => {
                // bwalk_adjust_crouch_pos(±2): the sniper rifle's crouch.
                let p = &mut self.players[pi];
                let d = if p.crouchpos == CROUCHPOS_SQUAT { 2 } else { -2 };
                p.crouchpos = (p.crouchpos + d).clamp(CROUCHPOS_SQUAT, CROUCHPOS_STAND);
            }
            HANDATTACKTYPE_THROWPROJECTILE => self.bgun_create_thrown_projectile(pi, h, gsetnum, gsetfunc),
            HANDATTACKTYPE_RCP120CLOAK => {
                let p = &mut self.players[pi];
                if p.devicesactive & DEVICE_CLOAKRCP120 != 0 {
                    p.devicesactive &= !DEVICE_CLOAKRCP120;
                } else {
                    p.devicesactive = (p.devicesactive & !DEVICE_CLOAKDEVICE) | DEVICE_CLOAKRCP120;
                }
            }
            // HANDATTACKTYPE_UPLINK is solo only.
            _ => {}
        }
    }

    /// `hand_inflict_melee_damage` (`prop.c:1130`): every chr on screen
    /// nearer than 5 m, near to far, whose part boxes overlap a 73 x 55 cm
    /// window around the crosshair's line and reach within the weapon's range
    /// (`obj_is_any_node_in_range`), with a clear line from the eye, takes the
    /// blow in the torso (ducking: general; squatting: half). If none did (and
    /// not `arg2`, the no-uncloak attacks), the BG is tried on the next tick.
    /// A pane of glass on screen within 5 m (not for the Tranquilizer, nor `arg2`)
    /// in the same order takes `damage × 2.5` if the blow's line meets its model.
    /// `arg2`'s `CHRCFLAG_AVOIDING` has no reader in a match.
    fn hand_inflict_melee_damage(&mut self, pi: usize, h: usize, arg2: bool) {
        let mut skipthething = false;
        let (gsetnum, gsetfunc) = (self.players[pi].gun.hands[h].weaponnum, self.players[pi].gun.hands[h].weaponfunc);
        let func = self.res.gset.func(gsetnum, gsetfunc).cloned();
        let rangelimit = func.as_ref().filter(|f| f.kind() == INVENTORYFUNCTYPE_MELEE).map_or(60.0, |f| f.range);
        let w2s = self.players[pi].cam.world_to_screen;
        let ppos = self.players[pi].pos;
        // bgun_get_cross_pos, over the screen: -1..1.
        let cam = &self.players[pi].cam;
        let cross = self.players[pi].gun.p.crosspos;
        let spfc = [(cross[0] - cam.c_screenleft) / (cam.c_screenwidth * 0.5) - 1.0, (cross[1] - cam.c_screentop) / (cam.c_screenheight * 0.5) - 1.0];
        let spf4 = [cam.c_screenheight * 0.166_666_67, cam.c_screenheight * 0.125];
        let mut order: Vec<(f32, usize)> = (0..self.chrs.len())
            .filter(|&j| self.chrs[j].player != Some(pi))
            .filter(|&j| crate::chr::body::pos_is_onscreen(&self.players[pi].cam, self.chrs[j].pos, self.chrs[j].effective_scale()))
            .map(|j| (-w2s.transform_point3(self.chrs[j].pos).z, j))
            .filter(|&(z, _)| z < 500.0)
            .collect();
        // The glass on screen, in the same list (`g_Vars.onscreenprops`).
        const GLASS: usize = 1 << 30;
        if gsetnum != WEAPON_TRANQUILIZER && !arg2 {
            for (k, o) in self.props.objs.iter().enumerate() {
                if (o.ty == OBJTYPE_GLASS || o.ty == OBJTYPE_TINTEDGLASS) && !o.is_gone() && !o.is_deleting() && crate::chr::body::pos_is_onscreen(&self.players[pi].cam, o.pos, o.def.scale * o.scale) {
                    let z = -w2s.transform_point3(o.pos).z;
                    if z < 500.0 {
                        order.push((z, GLASS | k));
                    }
                }
            }
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, j) in order {
            if j & GLASS != 0 {
                let k = j & !GLASS;
                let o = &self.props.objs[k];
                let mut m = pd_core::model::Model::new(o.def.clone());
                m.scale = o.scale;
                m.matrices = o.init_matrices();
                let Some((distance, sp110)) = crate::chr::body::obj_is_any_node_in_range(&m, &w2s, spfc, spf4) else { continue };
                if !(sp110 <= 0.0 && distance >= -rangelimit) {
                    continue;
                }
                // cdtypes 0: nothing stands between. model_test_for_hit along the blow.
                let id = o.id;
                let (gunpos2d, gundir2d) = self.spread(pi, h, true);
                let lodscale = self.players[pi].cam.c_lodscalez;
                if self.props.objs[k].test_hit(&w2s, lodscale, gunpos2d, gundir2d.normalize_or_zero(), 4_294_836_224.0).is_some() {
                    skipthething = true;
                    // bgun_play_glass_hit_sound (`bondgun.c:8726`).
                    self.sound_at(0x8077, 1.0, ppos, [400.0, 2500.0, 3000.0]);
                    let damage = self.player_gset_damage(pi, h) * 2.5;
                    self.obj_damage_by_gunfire(id, damage, gsetnum, pi as i32);
                }
                continue;
            }
            let Some((distance, sp110)) = crate::chr::body::obj_is_any_node_in_range(&self.chrs[j].model, &w2s, spfc, spf4) else { continue };
            if !(sp110 <= 0.0 && distance >= -rangelimit) {
                continue;
            }
            if !self.level.los_autoflags(ppos, self.chrs[j].pos) {
                continue;
            }
            if arg2 {
                continue;
            }
            let (_, gundir2d) = self.spread(pi, h, true);
            skipthething = true;
            let gundir = self.players[pi].cam.projection.transform_vector3(gundir2d);
            let cpos = self.chrs[j].pos;
            self.bgun_play_prop_hit_sound_chr(gsetnum, gsetfunc, cpos);
            let hitpart = match self.players[pi].crouchpos {
                CROUCHPOS_DUCK => crate::chr::HITPART_GENERAL,
                CROUCHPOS_SQUAT => HITPART_GENERALHALF,
                _ => HITPART_TORSO,
            };
            let damage = self.player_gset_damage(pi, h);
            self.chr_damage_by_impact(j, damage, gundir, crate::chr::DamageFrom::new(Some(pi), gsetnum, gsetfunc), hitpart);
        }
        if !skipthething && !arg2 {
            self.players[pi].gun.hands[h].unk0d0f_02 = true;
        }
    }

    /// `gset_get_damage` for player `pi`'s hand `h`: the chrs' one, with the
    /// Mauler's charge (`mm_maulercharge`).
    fn player_gset_damage(&self, pi: usize, h: usize) -> f32 {
        let hand = &self.players[pi].gun.hands[h];
        self.chr_gset_damage(hand.weaponnum, hand.weaponfunc, hand.matmot1)
    }

    /// `chr_hit` (`chr.c:4602`): the hit position, the prop hit sound, the
    /// blood and `chr_damage_by_impact` with the shooter's gun.
    /// And the blood behind it (`splats_create_for_chr_hit`).
    fn chr_hit(&mut self, pi: usize, h: usize, j: usize, hit: &crate::chr::body::ChrHit, gundir3d: Vec3) {
        let (weaponnum, func) = (self.players[pi].gun.hands[h].weaponnum, self.players[pi].gun.hands[h].weaponfunc);
        self.players[pi].gun.bgun_set_hit_pos(hit.pos);
        self.bgun_play_prop_hit_sound_chr(weaponnum, func, hit.pos);
        self.chr_emit_sparks(j, hit.hitpart, hit.pos, gundir3d);
        // chr_hit's blood (`chr.c:4759`): not the Tranquilizer's dart (a round
        // here is never a melee blow).
        // SUBST: PD also bruises the body (`chr_bruise`: the nearest vertex's
        // colour alpha set to 20-70, which PD's chr shading reads as darker) /
        // no bruise: our chrs are lit by a room light instead of PD's shade
        // pipeline, which is what reads that alpha.
        if weaponnum != WEAPON_TRANQUILIZER {
            let gunpos = self.players[pi].cam.pos();
            self.splats_create_for_chr_hit(j, gunpos, hit.pos, gundir3d, Some(pi));
        }
        let damage = self.player_gset_damage(pi, h);
        // hit->bboxnode, hitthing.unk28 / 2 and hitthing.pos as s16s: where a
        // shield glows from (`chr.c:4640`).
        let at = hit.face.map(|(side, p)| crate::fx::shieldhit::ShieldHitAt { node: hit.node, side, hitpos: [p.x as i16, p.y as i16, p.z as i16] });
        self.chr_damage_by_impact(j, damage, gundir3d, crate::chr::DamageFrom::new(Some(pi), weaponnum, func).at(at), hit.hitpart);
    }

    /// `prop_find_aiming_at` (`prop.c:979`): a shot, or with `isshooting`
    /// false a query for the nearest prop under the crosshair.
    pub(crate) fn prop_find_aiming_at(&mut self, pi: usize, h: usize, isshooting: bool, shootcontext: bool) -> Option<AimedAt> {
        let (gunpos2d, dir2d) = self.spread(pi, h, shootcontext);
        let mut gunpos2d = gunpos2d;
        if shootcontext && self.players[pi].gun.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_REAPER {
            gunpos2d.y -= 15.0 * self.rng.randomfrac();
        }
        let cheap = self.players.len() >= 2;
        self.shot_calculate_hits(pi, h, isshooting, gunpos2d, dir2d, cheap)
    }

    /// `bgun_calculate_player_shot_spread`: the camera-space origin (the eye)
    /// and direction.
    fn spread(&mut self, pi: usize, h: usize, dorandom: bool) -> (Vec3, Vec3) {
        let res = self.res.clone();
        let p = &mut self.players[pi];
        let mut g = p.gun_ctx(&res, &mut self.rng, &self.lv);
        (Vec3::ZERO, g.bgun_calculate_player_shot_spread(h, dorandom))
    }

    /// `shot_create` (`prop.c:998`).
    fn shot_create(&mut self, pi: usize, h: usize, isshooting: bool, dorandom: bool, numshots: i32) {
        let (gunpos2d, dir2d) = self.spread(pi, h, dorandom);
        if numshots > 0 {
            // cheap = g_Vars.mplayerisrunning.
            self.shot_calculate_hits(pi, h, isshooting, gunpos2d, dir2d, true);
        }
    }

    /// `shot_calculate_hits` (`prop.c:570`). A query (`isshooting` false, not
    /// melee) returns the nearest board hit.
    fn shot_calculate_hits(&mut self, pi: usize, h: usize, isshooting: bool, gunpos2d: Vec3, gundir2d: Vec3, cheap: bool) -> Option<AimedAt> {
        let proj = self.players[pi].cam.projection;
        let w2s = self.players[pi].cam.world_to_screen;
        // bgun0f0a9494(FINDPROPCONTEXT_QUERY): the dot is found again.
        for hand in self.players[pi].gun.hands.iter_mut() {
            hand.hasdotinfo = false;
        }
        let gunpos3d = proj.transform_point3(gunpos2d);
        let gundir3d = proj.transform_vector3(gundir2d);
        let gun = &self.players[pi].gun;
        let weaponnum = gun.bgun_get_weapon_num(h);
        let weaponfunc = gun.hands[h].weaponfunc;
        let func = self.res.gset.func(gun.hands[h].weaponnum, weaponfunc).cloned();
        let mut isshooting = isshooting;
        let mut explosiveshells = false;
        let mut ismelee = false;
        if let Some(f) = &func {
            if isshooting && f.flags & FUNCFLAG_EXPLOSIVESHELLS != 0 {
                explosiveshells = true;
            }
            if f.kind() == INVENTORYFUNCTYPE_MELEE && isshooting {
                ismelee = true;
                isshooting = false;
            }
        }
        let laserstream = weaponnum == WEAPON_LASER && weaponfunc == FUNC_SECONDARY;
        let penetration = if isshooting { func.as_ref().and_then(|f| f.shoot.as_ref()).map_or(0, |s| s.penetration) } else { 1 };
        let mut range = 200.0;
        let mut hitpos = if laserstream {
            gunpos3d + gundir3d * 300.0
        } else if ismelee {
            if let Some(f) = func.as_ref().filter(|f| f.kind() == INVENTORYFUNCTYPE_MELEE) {
                range = f.range;
            }
            gunpos3d + gundir3d * range
        } else {
            gunpos3d + gundir3d * 65536.0
        };

        // The BG: every room's display-list triangles, unless the Farsight
        // looks through it in x-ray (`prop.c:688`).
        let xray = weaponnum == WEAPON_FARSIGHT && self.players[pi].visionmode == VISIONMODE_XRAY;
        let bg = if xray { None } else { self.stage.bghit.bg_test_hit(gunpos3d, hitpos) };
        if let Some(b) = &bg {
            hitpos = b.pos;
        }
        // The props stop at the BG hit's depth (the Farsight's go through).
        let mut distance = 4_294_836_224.0f32;
        if let Some(b) = &bg {
            if weaponnum != WEAPON_FARSIGHT {
                let z = -w2s.transform_point3(b.pos).z;
                distance = distance.min(z);
            }
        }

        // obj_test_hit on the boards and the guns' objects, nearest first. M6:
        // chr_test_hit.
        let dirn = gundir3d.normalize_or_zero();
        let mut hits: Vec<PropHit> = Vec::new();
        for (i, b) in self.boards.iter().enumerate() {
            if let Some((t, n)) = ray_box(b.min, b.max, gunpos3d, dirn, 65536.0) {
                let pos = gunpos3d + dirn * t;
                if -w2s.transform_point3(pos).z <= distance {
                    hits.push(PropHit { target: ShotTarget::Board(i), t, pos, normal: n });
                }
            }
        }
        // chr_test_hit on the chrs this player sees (not a melee attack's).
        if !ismelee {
            for (j, c) in self.chrs.iter().enumerate() {
                if c.player == Some(pi) || !crate::chr::body::pos_is_onscreen(&self.players[pi].cam, c.pos, c.effective_scale()) {
                    continue;
                }
                if -w2s.transform_point3(c.pos).z - c.chr_get_hit_radius() >= distance {
                    continue;
                }
                if let Some(ch) = c.chr_test_hit(gunpos3d, dirn, cheap) {
                    if -w2s.transform_point3(ch.pos).z < distance {
                        let t = (ch.pos - gunpos3d).dot(dirn).max(0.0);
                        hits.push(PropHit { target: ShotTarget::Chr(j, ch), t, pos: ch.pos, normal: ch.normal });
                    }
                }
            }
        }
        let lodscale = self.players[pi].cam.c_lodscalez;
        let dir2n = gundir2d.normalize_or_zero();
        // A taken pickup is disabled (`prop_disable`) until it fades back in.
        for o in self.props.objs.iter().filter(|o| !o.is_deleting() && !o.is_gone() && o.flags & OBJFLAG_HELDROCKET == 0) {
            // The shooter's own held rocket and a THROWTHROUGH object are skipped
            // by the object test's own flags (OBJFLAG2_SHOOTTHROUGH is unset).
            if let Some((depth, p, n)) = o.test_hit(&w2s, lodscale, gunpos2d, dir2n, distance) {
                let pos = proj.transform_point3(p);
                let normal = proj.transform_vector3(n).normalize_or_zero();
                let t = (pos - gunpos3d).dot(dirn).max(0.0);
                let _ = depth;
                hits.push(PropHit { target: ShotTarget::Obj(o.id), t, pos, normal });
            }
        }
        hits.sort_by(|a, b| a.t.total_cmp(&b.t));
        // bgun0f0a94d0: the dot is the nearest prop hit, else the BG's.
        let dot = hits.first().map(|x| (x.pos, x.normal)).or(bg.as_ref().map(|b| (b.pos, b.normal)));
        if let Some((pos, rot)) = dot.filter(|(p, _)| p.abs().max_element() < 100_000.0) {
            for hand in self.players[pi].gun.hands.iter_mut() {
                hand.hasdotinfo = true;
                hand.dotpos = pos;
                hand.dotrot = rot;
            }
        }

        if isshooting {
            let mut blockedbyprop = false;
            let mut s1 = 0;
            for hit in &hits {
                if laserstream && hit.t > 300.0 {
                    continue;
                }
                match hit.target {
                    ShotTarget::Board(b) => self.board_hit(pi, b, hit, gunpos3d),
                    ShotTarget::Obj(id) => self.obj_hit(pi, id, hit, &func),
                    ShotTarget::Chr(j, ch) => self.chr_hit(pi, h, j, &ch, gundir3d),
                }
                // A board slows the bullet.
                s1 += 1;
                if s1 >= penetration {
                    blockedbyprop = true;
                    hitpos = hit.pos;
                    if explosiveshells {
                        self.explosion_create_simple(pi, hit.pos, EXPLOSIONTYPE_PHOENIX);
                    }
                    break;
                }
            }
            match bg {
                Some(b) if !blockedbyprop => self.bg_hit(pi, weaponnum, weaponfunc, &func, gunpos3d, gundir3d, &b, explosiveshells),
                _ => self.players[pi].gun.bgun_set_hit_pos(hitpos),
            }
        } else if ismelee {
            let hitaprop = hits.iter().find(|x| x.t < range);
            if hitaprop.is_some() || bg.is_some() {
                self.weapon_play_melee_hit_sound(pi, weaponnum);
                if weaponnum != WEAPON_UNARMED && weaponnum != WEAPON_TRANQUILIZER {
                    let (pos, normal) = match (hitaprop, &bg) {
                        (Some(x), _) => (x.pos, x.normal),
                        (None, Some(b)) => (b.pos, b.normal),
                        _ => unreachable!(),
                    };
                    self.fx.sparks.create(&mut self.rng, pos, gundir3d, normal, SPARKTYPE_DEFAULT);
                }
            } else {
                self.weapon_play_melee_miss_sound(pi, weaponnum);
            }
        } else {
            // The query (`prop.c:942`): the closest object, unless a laser
            // stream's is out of its reach.
            // Only the boards stand for objects the sight reacts to
            // (OBJFLAG3_REACTTOSIGHT, MODEL_TARGET); the guns' objects don't.
            return hits.first().filter(|x| !(laserstream && x.t > 300.0)).and_then(|x| match x.target {
                ShotTarget::Board(b) => Some(AimedAt::Board(b)),
                ShotTarget::Chr(j, _) => Some(AimedAt::Chr(j)),
                ShotTarget::Obj(_) => None,
            });
        }
        None
    }

    /// `obj_hit` (`propobj.c:14765`) on one of the guns' objects: sparks, the
    /// prop hit sound and `obj_damage_by_gunfire`, which sets an explosive off
    /// or breaks a sentry. `// SUBST:` PD also leaves a bullet hole on the
    /// object's model / none (no wallhit rides a prop).
    fn obj_hit(&mut self, pi: usize, id: u32, hit: &PropHit, func: &Option<super::gset::FuncDef>) {
        let ismelee = func.as_ref().is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_MELEE);
        self.players[pi].gun.bgun_set_hit_pos(hit.pos);
        if !ismelee {
            self.fx.sparks.create(&mut self.rng, hit.pos, Vec3::ZERO, Vec3::ZERO, SPARKTYPE_DEFAULT);
            // bgun_play_prop_hit_sound: an object's (g_SurfaceTypeMetalObj).
            let sound = if self.rng.random().is_multiple_of(2) { 0x8089 } else { 0x808a };
            self.sound_at(sound, 1.0, hit.pos, DEFAULT_DISTS);
        }
        let damage = func.as_ref().and_then(|f| f.shoot.as_ref()).map_or(0.0, |s| s.damage);
        let weaponnum = self.players[pi].gun.bgun_get_weapon_num(HAND_RIGHT);
        let destroyed = self.obj_damage(id, damage, weaponnum, pi as i32);
        if destroyed {
            if let Some(pos) = self.props.get(id).map(|o| o.pos) {
                self.autogun_destroyed(pos, pi as i32, pi);
            }
        }
    }

    /// `obj_hit` on a board: it counts the hit, takes a bullet hole and its
    /// object hit sound (`g_SurfaceTypeMetalObj`).
    fn board_hit(&mut self, pi: usize, board: usize, hit: &PropHit, gunpos3d: Vec3) {
        let gun = &self.players[pi].gun;
        let damage = self.res.gset.func(gun.hands[HAND_RIGHT].weaponnum, gun.hands[HAND_RIGHT].weaponfunc).and_then(|f| f.shoot.as_ref()).map_or(0.0, |s| s.damage);
        let b = &mut self.boards[board];
        b.hits += 1;
        b.damage += damage;
        b.flash = 1.0;
        let tex = if self.rng.random().is_multiple_of(2) { WALLHITTEX_BULLET1 } else { WALLHITTEX_BULLET2 };
        let brightness = self.lights.brightness(self.level.floor_room(hit.pos, 1.0));
        // The board's face art sits a few mm proud of its box; the hole goes on top.
        let wh = wallhit::wallhit_create(&mut self.rng, hit.pos + hit.normal * 0.6, hit.normal, gunpos3d, tex, brightness);
        self.fx.push_wallhit(wh);
        let id = if self.rng.random().is_multiple_of(2) { 0x8089 } else { 0x808a };
        self.sound_at(id, 1.0, hit.pos, DEFAULT_DISTS);
    }

    /// The `hitbg && !blockedbyprop` branch of `shot_calculate_hits`
    /// (`prop.c:816`): the hit position, a bullet hole from the surface's
    /// textures, the hit sounds, the Phoenix blast or the bullet-hole flame, and
    /// the sparks.
    #[allow(clippy::too_many_arguments)]
    fn bg_hit(&mut self, pi: usize, weaponnum: u8, weaponfunc: usize, func: &Option<super::gset::FuncDef>, gunpos3d: Vec3, gundir3d: Vec3, b: &crate::stage::BgHit, explosiveshells: bool) {
        let playercount = self.players.len();
        // lights_handle_hit (`prop.c:821`): a light in the room it hits breaks,
        // with the glass sound at the light's first corner. PD's, kept: that
        // corner is relative to the room, and goes to ps_create as a world
        // position (`dlights.c:497`).
        if let Some(corner) = self.lights.lights_handle_hit(&self.stage.rooms, gunpos3d, b.pos, b.room as usize) {
            self.sound_at(0x8077, 1.0, corner, crate::propsnd::DEFAULT_DISTS);
        }
        let mut texnum = 0;
        self.players[pi].gun.bgun_set_hit_pos(b.pos);
        // surfacetype: g_Textures[texturenum], or the default for none.
        let surface = surface_type(b.surface.map_or(0, |s| s.surfacetype));
        if !surface.wallhittexes.is_empty() && func.as_ref().is_none_or(|f| f.kind() != INVENTORYFUNCTYPE_MELEE) {
            if !matches!(weaponnum, WEAPON_UNARMED | WEAPON_LASER | WEAPON_TRANQUILIZER | WEAPON_FARSIGHT) {
                let i = (self.rng.random() % surface.wallhittexes.len() as u32) as usize;
                texnum = surface.wallhittexes[i];
                if (WALLHITTEX_GLASS1..=WALLHITTEX_GLASS3).contains(&texnum) {
                    // The bulletproof-glass holes instead.
                    texnum += 10;
                }
                if texnum != 0 {
                    let brightness = self.lights.brightness(Some(b.room));
                    let wh = wallhit::wallhit_create(&mut self.rng, b.pos, b.normal, gunpos3d, texnum, brightness);
                    self.fx.push_wallhit(wh);
                }
            }
            self.bgun_play_bg_hit_sound(pi, weaponnum, weaponfunc, b.pos, b.surface.map(|s| s.soundsurfacetype));
            if explosiveshells {
                self.explosion_create_simple(pi, b.pos, EXPLOSIONTYPE_PHOENIX);
            } else {
                // chr_is_using_paintball: a cheat, solo only.
                if playercount >= 2 {
                    if self.rng.random().is_multiple_of(8) {
                        self.fx.smokes.smoke_create_simple(b.pos, SMOKETYPE_BULLETIMPACT);
                    }
                } else if texnum != 0 {
                    self.explosion_create_simple(pi, b.pos, EXPLOSIONTYPE_BULLETHOLE);
                }
                if (playercount <= 2 || self.lv.lvupdate240 <= 8 || self.rng.random().is_multiple_of(4)) && b.pos.abs().max_element() < 32000.0 {
                    let mut sparktype = match weaponnum {
                        WEAPON_FARSIGHT => SPARKTYPE_BGHIT_ORANGE,
                        WEAPON_CYCLONE => SPARKTYPE_ELECTRICAL,
                        WEAPON_MAULER | WEAPON_PHOENIX | WEAPON_CALLISTO | WEAPON_REAPER => SPARKTYPE_BGHIT_GREEN,
                        WEAPON_TRANQUILIZER => SPARKTYPE_BGHIT_TRANQULIZER,
                        _ => SPARKTYPE_DEFAULT,
                    };
                    let st = b.surface.map_or(0, |s| s.surfacetype);
                    if st == SURFACETYPE_SHALLOWWATER || st == SURFACETYPE_DEEPWATER {
                        sparktype = SPARKTYPE_SHALLOWWATER;
                    }
                    self.fx.sparks.create(&mut self.rng, b.pos, gundir3d, b.normal, sparktype);
                }
            }
        }
    }

    /// `bgun_play_bg_hit_sound` (`bondgun.c:8741`, NTSC 1.0+): a ricochet from
    /// the shared table, then the surface's own hit sound. `soundsurface` is the
    /// hit texture's `soundsurfacetype` (`None`: no texture, no surface sound).
    pub(crate) fn bgun_play_bg_hit_sound(&mut self, pi: usize, weaponnum: u8, weaponfunc: usize, pos: Vec3, soundsurface: Option<u8>) {
        const RICOCHETS: [u16; 36] = [
            0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x17, 0x18, 0x19, 0x1a, 0x17, 0x18, 0x19, 0x1a, 0x1f, 0x20, 0x20, 0x21, 0x1f, 0x20, 0x20, 0x21, 0x1f, 0x20, 0x20, 0x21, 0x23, 0x24, 0x25,
            0x26, 0x27, 0x28, 0x29, 0x2a,
        ];
        let rand1 = self.rng.random();
        let rand2 = self.rng.random();
        if self.lv.lvupdate240 <= 0 {
            return;
        }
        if let Some(s) = soundsurface {
            if surface_type(s).sounds.is_empty() {
                return;
            }
        }
        let mut playdefault = true;
        if weaponnum == WEAPON_LASER {
            playdefault = false;
            // gset->lasershots: the hand's burstbullets & 0xff (`gset.c:422`); the
            // stream plays on every fourth, half the time.
            let lasershots = self.players.get(pi).map_or(0, |p| p.gun.hands[HAND_RIGHT].burstbullets & 0xff);
            if weaponfunc == FUNC_PRIMARY || (lasershots % 4 == 0 && !self.rng.random().is_multiple_of(2)) {
                let id = if rand1.is_multiple_of(2) { 0x5b } else { 0x5c };
                self.sound_at(id, 1.0, pos, DEFAULT_DISTS);
                return;
            }
        } else if weaponnum == WEAPON_COMBATKNIFE || weaponnum == WEAPON_BOLT {
            // Knives and bolts make a metal sound.
            self.sound_at(0x8079, 1.0, pos, DEFAULT_DISTS);
            return;
        } else if matches!(weaponnum, WEAPON_REMOTEMINE | WEAPON_PROXIMITYMINE | WEAPON_TIMEDMINE | WEAPON_COMMSRIDER | WEAPON_TRACERBUG | WEAPON_TARGETAMPLIFIER | WEAPON_ECMMINE) {
            // A mine landing.
            self.sound_at(0x80aa, 1.0, pos, DEFAULT_DISTS);
            return;
        } else {
            let id = RICOCHETS[(rand1 % RICOCHETS.len() as u32) as usize];
            self.sound_at(id, 1.0, pos, DEFAULT_DISTS);
        }
        if playdefault {
            if let Some(s) = soundsurface {
                let t = surface_type(s);
                if !t.sounds.is_empty() {
                    let id = t.sounds[(rand2 % t.sounds.len() as u32) as usize];
                    self.sound_at(id, 1.0, pos, DEFAULT_DISTS);
                }
            }
        }
    }

    /// `weapon_play_melee_hit_sound` (`prop.c:504`).
    fn weapon_play_melee_hit_sound(&mut self, pi: usize, weaponnum: u8) {
        let (id, speed) = match weaponnum {
            WEAPON_UNARMED => {
                let id = if self.rng.random() % 2 == 1 { 0x8094 } else { 0x808f };
                (id, 1.0 - self.rng.randomfrac() * 0.1)
            }
            WEAPON_TRANQUILIZER => (0x04fb, 2.78),
            _ => (0x8079, 1.0 - self.rng.randomfrac() * 0.1),
        };
        let pos = self.players[pi].pos;
        self.sound_at(id, speed, pos, DEFAULT_DISTS);
    }

    /// `weapon_play_melee_miss_sound` (`prop.c:453`) at player `pi`.
    fn weapon_play_melee_miss_sound(&mut self, pi: usize, weaponnum: u8) {
        let pos = self.players[pi].pos;
        self.weapon_play_melee_miss_sound_at(weaponnum, pos);
    }

    /// `weapon_play_melee_miss_sound` at a prop's position.
    pub(crate) fn weapon_play_melee_miss_sound_at(&mut self, weaponnum: u8, pos: Vec3) {
        let (id, speed) = match weaponnum {
            WEAPON_TRANQUILIZER => (0x04fb, 2.78),
            WEAPON_REAPER => return,
            WEAPON_COMBATKNIFE => {
                let id = if self.rng.random() % 2 == 1 { 0x8060 } else { 0x8061 };
                (id, 1.05 - self.rng.randomfrac() * 0.2)
            }
            _ => (0x0069, 1.0 - self.rng.randomfrac() * 0.2),
        };
        self.sound_at(id, speed, pos, DEFAULT_DISTS);
    }

    /// Turn player `pi`'s queued gun events into world effects and sounds.
    pub(crate) fn process_gun_events(&mut self, pi: usize) {
        let events = std::mem::take(&mut self.players[pi].gun.events);
        let lv60 = self.lv.lvupdate60freal;
        let lv240 = self.lv.lvupdate240;
        for e in events {
            match e {
                GunEvent::Sound { id, speed, handle } => match handle {
                    Some(hand) => self.push_event(Event::HandleSound { handle: hand_sound_handle(pi, hand), sound: id, pitch: speed, volume: 1.0, pan: 0.0 }),
                    None => self.sound(id, speed),
                },
                GunEvent::StopSound { hand } => self.push_event(Event::StopSound { handle: hand_sound_handle(pi, hand) }),
                GunEvent::Beam { hand } => self.beam_create_for_hand(pi, hand),
                GunEvent::Casing { hand, mtx, casing } => {
                    let weaponnum = self.players[pi].gun.bgun_get_weapon_num(hand);
                    let hd = &self.players[pi].gun.hands[hand];
                    let handvel = if lv240 > 0 { (hd.posmtx.w_axis - hd.prevmtx.w_axis).truncate() / lv60 } else { Vec3::ZERO };
                    let ground = self.players[pi].manground;
                    if let Some(c) = casing::casing_create_for_hand(&mut self.rng, weaponnum, casing, ground, &mtx, handvel) {
                        // SUBST: PD keeps g_MaxCasings in a ring / the oldest
                        // of 20 goes.
                        if self.fx.casings.len() >= 20 {
                            self.fx.casings.remove(0);
                        }
                        self.fx.casings.push(c);
                    }
                }
                GunEvent::Smoke { hand, pos, ty } => {
                    // smoke_create_for_hand (`bondgun.c:6631`): `createsmoke`
                    // stays set until a smoke is actually made.
                    if self.fx.smokes.smoke_create_for_hand(pos, ty, hand) {
                        self.players[pi].gun.hands[hand].createsmoke = false;
                    }
                }
                GunEvent::FreeHeldRocket { hand } => self.bgun_free_held_rocket(pi, hand),
                GunEvent::CreateHeldWeapon { hand } => self.playermgr_create_weapon(pi, hand),
                GunEvent::DeleteHeldWeapon { hand } => self.chrs[pi].held[hand] = None,
                GunEvent::Rumble => self.players[pi].rumble.pak_rumble(0.2, 2, 4),
                GunEvent::UpdateRocketLauncher { hand } => self.bgun_update_rocket_launcher(pi, hand),
                GunEvent::UncloakTemporarily => self.chr_uncloak_temporarily(pi),
            }
        }
    }

    /// `beam_create_for_hand` (`gunfx.c:104`).
    fn beam_create_for_hand(&mut self, pi: usize, h: usize) {
        let p = &mut self.players[pi];
        let hand = &p.gun.hands[h];
        let v = p.cam.world_to_screen.transform_point3(hand.hitpos);
        if -v.z < hand.muzzlez {
            return;
        }
        let mut weaponnum = p.gun.bgun_get_weapon_num(h) as i32;
        if weaponnum == WEAPON_LASER as i32 && hand.weaponfunc == FUNC_SECONDARY {
            weaponnum = BEAM_LASERSTREAM;
        }
        let (from, to) = (hand.muzzlepos, hand.hitpos);
        let lasertype = hand.matmot1 as i32;
        let beam = &mut p.gun.hands[h].beam;
        beam.create(&mut self.rng, weaponnum, from, to);
        if beam.weaponnum == WEAPON_MAULER as i32 {
            // mm_lasertype: the charge.
            beam.weaponnum = -3 - lasertype.clamp(0, 5);
        }
        // The tracer the other players see, from the body's gun (`gunfx.c:131`),
        // unless it points more than 5° off the first-person one.
        if self.players.len() >= 2 {
            let last = self.players[pi].chrmuzzlelastpos[h];
            let a = (to - last).normalize_or_zero();
            let b = (to - from).normalize_or_zero();
            let radians = a.dot(b).clamp(-1.0, 1.0).acos();
            if !(radians > pd_core::math::baddtor(5.0)) || weaponnum == BEAM_LASERSTREAM {
                let beam = &mut self.chrs[pi].fireslots[h].beam;
                beam.create(&mut self.rng, weaponnum, last, to);
                if beam.weaponnum == WEAPON_MAULER as i32 {
                    beam.weaponnum = -3 - lasertype.clamp(0, 5);
                }
            }
        }
    }

    /// `playermgr_create_weapon` (`playermgr.c:776`): the hand's weapon in
    /// the player's body's hand, if it has none yet (the left hand's remote
    /// mine has no model).
    pub(crate) fn playermgr_create_weapon(&mut self, pi: usize, hand: usize) {
        if self.chrs[pi].held[hand].is_some() {
            return;
        }
        let weaponnum = self.players[pi].gun.bgun_get_weapon_num(hand);
        if hand == HAND_LEFT && weaponnum == WEAPON_REMOTEMINE {
            return;
        }
        self.chr_give_weapon(pi, weaponnum, hand);
    }
}
