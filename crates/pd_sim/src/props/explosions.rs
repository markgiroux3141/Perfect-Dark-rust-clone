//! `explosions.c`: PD's explosions (NTSC).
//!
//! An explosion is a prop with up to 40 flare parts. Each part plays a 15-frame
//! animation over `g_ExplosionTexturePairs`, holding each frame `flarespeed`
//! ticks. New parts appear inside a box that grows by `changerateh/v` a tick,
//! clamped to the room. The damage is PD's per-axis box falloff
//! (`explosion_inflict_damage`).
//!
//! Substitutions, until rooms and portals (M9):
//! - `// SUBST:` PD clamps the parts to the explosion room's bbox plus those of
//!   the portals the blast reaches (`explosion_create`) / only the room's own
//!   box ([`ExpWorld::room_bbox`]).
//! - `// SUBST:` `explosion_overlaps_prop` tests a victim against the rooms the
//!   blast reaches / every victim inside the damage cube is hit.
//! - `// SUBST:` breakable lights (`room_set_light_broken`) do not exist.
//!
//! Source: the old repo's `pd_guns/explosions.rs`; its damage came from the
//! old engine's `combat::explosives` and is ported from the decomp here.

use glam::Vec3;
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::math::baddtor;
use pd_core::rng::Rng;

use crate::fx::smoke::Smokes;
use crate::fx::wallhit::{wallhit_create_with_20_args, Wallhit};

/// `MAX_EXPLOSIONS` (`constants.h:21`).
pub const MAX_EXPLOSIONS: usize = 6;

/// `struct explosiontype`.
#[derive(Clone, Copy, Debug)]
pub struct ExplosionType {
    pub rangeh: f32,
    pub rangev: f32,
    pub changerateh: f32,
    pub changeratev: f32,
    pub innersize: f32,
    pub blastradius: f32,
    pub damageradius: f32,
    pub duration: i32,
    pub propagationrate: i32,
    pub flarespeed: f32,
    pub smoketype: usize,
    pub sound: u16,
    pub damage: f32,
}

#[allow(clippy::too_many_arguments)]
const fn et(rangeh: f32, rangev: f32, changerateh: f32, changeratev: f32, innersize: f32, blastradius: f32, damageradius: f32, duration: i32, propagationrate: i32, flarespeed: f32, smoketype: usize, sound: u16, damage: f32) -> ExplosionType {
    ExplosionType { rangeh, rangev, changerateh, changeratev, innersize, blastradius, damageradius, duration, propagationrate, flarespeed, smoketype, sound, damage }
}

use SMOKETYPE_BULLETIMPACT as BI;
use SMOKETYPE_ELECTRICAL as EL;
use SMOKETYPE_LARGE as LG;
use SMOKETYPE_MEDIUM as MD;
use SMOKETYPE_MINI as MI;
use SMOKETYPE_NONE as NO;
use SMOKETYPE_SMALL as SM;

/// `g_ExplosionTypes[]` (`explosions.c:41`). Sounds: SFXNUM_0000 = 0, the
/// SFXMAP_8099/809A/809C/809E/809F/80A0/80A4/80A5 as listed.
pub const EXPLOSION_TYPES: [ExplosionType; 26] = [
    /*00*/ et(0.1, 0.1, 0.0, 0.0, 0.1, 0.0, 0.0, 1, 1, 1.0, NO, 0x0000, 0.0),
    /*01*/ et(1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 30, 1, 1.0, BI, 0x0000, 0.0),
    /*02*/ et(20.0, 20.0, 0.0, 0.0, 30.0, 50.0, 50.0, 40, 1, 3.0, MI, 0x8099, 0.125),
    /*03*/ et(50.0, 50.0, 0.0, 0.0, 50.0, 100.0, 100.0, 45, 1, 4.0, MI, 0x809a, 0.5),
    /*04*/ et(60.0, 80.0, 2.0, 0.6, 100.0, 130.0, 240.0, 60, 2, 5.0, EL, 0x809e, 1.0),
    /*05*/ et(60.0, 120.0, 2.0, 0.6, 150.0, 160.0, 280.0, 60, 2, 5.0, EL, 0x809e, 2.0),
    /*06*/ et(20.0, 20.0, 0.0, 0.0, 22.0, 40.0, 40.0, 60, 1, 3.0, MI, 0x8099, 0.5),
    /*07*/ et(35.0, 40.0, 0.0, 0.0, 35.0, 70.0, 70.0, 60, 1, 4.0, MI, 0x809a, 1.0),
    /*08*/ et(50.0, 80.0, 2.0, 0.6, 50.0, 100.0, 160.0, 60, 2, 5.0, EL, 0x809e, 2.0),
    /*09*/ et(60.0, 120.0, 2.0, 0.6, 50.0, 130.0, 180.0, 60, 2, 5.0, EL, 0x809e, 2.0),
    /*10*/ et(40.0, 40.0, 0.8, 0.5, 70.0, 80.0, 160.0, 80, 4, 5.0, SM, 0x80a0, 1.0),
    /*11*/ et(50.0, 50.0, 1.2, 0.8, 100.0, 100.0, 200.0, 90, 1, 4.0, SM, 0x809e, 2.0),
    /*12*/ et(70.0, 60.0, 2.0, 1.2, 150.0, 140.0, 280.0, 90, 2, 5.0, MD, 0x809e, 4.0),
    /*13*/ et(80.0, 60.0, 4.0, 1.4, 200.0, 200.0, 400.0, 90, 2, 5.0, LG, 0x809f, 4.0),
    /*14*/ et(50.0, 50.0, 0.0, 0.0, 120.0, 150.0, 300.0, 150, 4, 4.0, SM, 0x809f, 4.0),
    /*15*/ et(1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1, 1, 1.0, BI, 0x809c, 0.0),
    /*16*/ et(1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1, 1, 1.0, BI, 0x809c, 0.0),
    /*17*/ et(80.0, 60.0, 10.0, 5.0, 1500.0, 2200.0, 3600.0, 500, 1, 2.0, NO, 0x80a5, 4.0),
    /*18*/ et(80.0, 60.0, 3.0, 1.0, 300.0, 450.0, 640.0, 60, 1, 2.0, NO, 0x809f, 4.0),
    /*19*/ et(90.0, 75.0, 2.5, 0.87, 250.0, 375.0, 600.0, 180, 2, 5.0, LG, 0x809f, 4.0),
    /*20*/ et(160.0, 120.0, 6.0, 2.0, 600.0, 450.0, 640.0, 60, 1, 2.0, NO, 0x809f, 4.0),
    /*21*/ et(40.0, 30.0, 2.0, 0.7, 100.0, 140.0, 270.0, 45, 2, 5.0, SM, 0x809f, 3.5),
    /*22*/ et(20.0, 20.0, 0.0, 0.0, 30.0, 100.0, 200.0, 40, 1, 3.0, MI, 0x8099, 0.25),
    /*23*/ et(100.0, 80.0, 4.0, 1.4, 210.0, 220.0, 500.0, 90, 2, 5.0, LG, 0x809f, 4.0),
    /*24*/ et(80.0, 60.0, 4.0, 1.4, 500.0, 200.0, 400.0, 90, 2, 5.0, LG, 0x809f, 4.0),
    /*25*/ et(640.0, 480.0, 32.0, 11.2, 1600.0, 1000.0, 1000.0, 180, 2, 5.0, NO, 0x80a4, 4.0),
];

/// `g_TcExplosionTexturePairs` (`textureconfig.c:55`): the flame (IA8) and its
/// colour map (RGBA16) of animation frame `i`.
pub fn texture_pair(i: usize) -> (u16, u16) {
    let a = 0x001e + 2 * i as u16;
    (a, a + 1)
}

/// `struct explosionpart`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExplosionPart {
    pub pos: Vec3,
    pub size: f32,
    pub rot: f32,
    pub frame: i32,
    pub bb: usize,
}

/// `struct explosionbb`.
#[derive(Clone, Copy, Debug)]
pub struct ExplosionBb {
    pub bbmin: Vec3,
    pub bbmax: Vec3,
}

/// `struct explosion` and its prop.
#[derive(Clone, Debug)]
pub struct Explosion {
    pub pos: Vec3,
    pub ty: usize,
    pub age: i32,
    /// The prop that exploded (`exp->source`), spared by the blast.
    pub source: Option<u32>,
    pub makescorch: bool,
    /// `unk3d0` / `unk3dc`: where the scorch goes and its surface's normal.
    pub scorchpos: Vec3,
    pub scorchnormal: Vec3,
    /// The player who set it off (`exp->owner`), -1 for nobody.
    pub owner: i32,
    pub bbs: Vec<ExplosionBb>,
    pub parts: [ExplosionPart; 40],
}

/// What the explosions need from the world (PD: the rooms and `cd_*`).
pub trait ExpWorld {
    /// The bbox of the room at `pos` (`g_Rooms[exproom].bbmin/bbmax`).
    fn room_bbox(&self, pos: Vec3) -> (Vec3, Vec3);
    /// `cd_find_room_at_pos_ycnp`: the floor's height and normal under `pos`,
    /// `None` outside the stage. The bool: the floor is a prop.
    fn floor_below(&self, pos: Vec3) -> Option<(f32, Vec3, bool)>;
}

/// Something `explosion_inflict_damage` can hurt.
#[derive(Clone, Copy, Debug)]
pub struct Victim {
    pub id: VictimId,
    pub pos: Vec3,
    /// A chr's `prop_get_bbox` (radius, ymax, ymin); `None` for an object.
    pub chrbox: Option<(f32, f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VictimId {
    /// A chr (a player's or a simulant's), by index in the world's list.
    Chr(usize),
    /// A firing-range target board, by index.
    Board(usize),
    Prop(u32),
}

/// One explosion tick's side effects, for the caller to apply.
#[derive(Default, Debug)]
pub struct ExpOut {
    /// `ps_create(type->sound)` at a position.
    pub sounds: Vec<(u16, Vec3)>,
    /// Scorch marks (`wallhit_create_with_20_args(WALLHITTEX_SCORCH)`).
    pub wallhits: Vec<Wallhit>,
    /// (victim, damage, knockback direction, first frame, owner).
    pub damage: Vec<(VictimId, f32, Vec3, bool, i32)>,
    /// `room_flash_lighting(room, start, 255)` requests (the room at the blast).
    pub flashes: Vec<(Vec3, f32)>,
}

#[derive(Clone)]
pub struct Explosions {
    pub slots: Vec<Option<Explosion>>,
    /// `g_ExplosionShakeTotalTimer` / `g_ExplosionShakeIntensityTimer`.
    pub shake_total_timer: i32,
    pub shake_intensity_timer: i32,
}

impl Default for Explosions {
    fn default() -> Self {
        Explosions { slots: vec![None; MAX_EXPLOSIONS], shake_total_timer: 0, shake_intensity_timer: 0 }
    }
}

pub fn etype(t: usize) -> &'static ExplosionType {
    &EXPLOSION_TYPES[t.min(EXPLOSION_TYPES.len() - 1)]
}

impl Explosion {
    /// `explosion_get_horizontal_range_at_frame` (`explosions.c:113`).
    pub fn hrange_at(&self, frame: i32) -> f32 {
        let t = etype(self.ty);
        if self.ty == EXPLOSIONTYPE_GASBARREL && frame > 32 {
            (frame as f32 * 3.0 + 40.0).min(300.0)
        } else {
            t.rangeh + t.changerateh * frame as f32
        }
    }

    /// `explosion_get_vertical_range_at_frame` (`explosions.c:131`).
    pub fn vrange_at(&self, frame: i32) -> f32 {
        let t = etype(self.ty);
        if self.ty == EXPLOSIONTYPE_GASBARREL && frame > 32 {
            20.0
        } else {
            t.rangev + t.changeratev * frame as f32
        }
    }
}

/// The camera an explosion is created in front of: its position and
/// `c_lodscalez` (a bullet hole's flame is skipped beyond 4 m of it).
#[derive(Clone, Copy, Debug)]
pub struct ExpCam {
    pub pos: Vec3,
    pub lodscalez: f32,
}

impl Explosions {
    /// `explosion_create_simple` (`explosions.c:73`).
    #[allow(clippy::too_many_arguments)]
    pub fn create_simple(&mut self, rng: &mut Rng, smokes: &mut Smokes, world: &dyn ExpWorld, source: Option<u32>, pos: Vec3, ty: usize, owner: i32, cam: ExpCam, out: &mut ExpOut) -> bool {
        self.create(rng, smokes, world, source, pos, ty, owner, false, Vec3::ZERO, Vec3::Y, cam, out)
    }

    /// `explosion_create_complex` (`explosions.c:78`): as simple, plus a scorch
    /// on the floor below if the blast reaches it.
    #[allow(clippy::too_many_arguments)]
    pub fn create_complex(&mut self, rng: &mut Rng, smokes: &mut Smokes, world: &dyn ExpWorld, source: Option<u32>, pos: Vec3, ty: usize, owner: i32, cam: ExpCam, out: &mut ExpOut) -> bool {
        if ty == EXPLOSIONTYPE_NONE {
            return false;
        }
        let t = etype(ty);
        let (makescorch, sp100, sp88) = match world.floor_below(pos) {
            Some((y, n, collisionprop)) => {
                let reach = pos.y - y <= (t.rangev + t.changeratev * t.duration as f32 + t.innersize) * 0.5 || pos.y - y <= 75.0;
                (!collisionprop && reach, Vec3::new(pos.x, y, pos.z), n)
            }
            None => (false, pos, Vec3::Y),
        };
        self.create(rng, smokes, world, source, pos, ty, owner, makescorch, sp100, sp88, cam, out)
    }

    /// `explosion_create` (`explosions.c:234`).
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &mut self,
        rng: &mut Rng,
        smokes: &mut Smokes,
        world: &dyn ExpWorld,
        source: Option<u32>,
        pos: Vec3,
        ty: usize,
        owner: i32,
        makescorch: bool,
        scorchpos: Vec3,
        scorchnormal: Vec3,
        cam: ExpCam,
        out: &mut ExpOut,
    ) -> bool {
        if ty == EXPLOSIONTYPE_NONE {
            return false;
        }
        let t = etype(ty);
        // Bullet holes: the flame only within 4 metres.
        if ty == EXPLOSIONTYPE_BULLETHOLE {
            let sum = (pos - cam.pos).length_squared();
            if sum * cam.lodscalez * cam.lodscalez > 400.0 * 400.0 {
                if rng.random().is_multiple_of(2) {
                    smokes.smoke_create_simple(pos, t.smoketype);
                }
                return true;
            }
        }
        let mut slot = self.slots.iter().position(|s| s.is_none());
        if slot.is_none() {
            // Replace the oldest bullet-hole flame.
            let mut maxage = -1;
            for (i, s) in self.slots.iter().enumerate() {
                if let Some(e) = s {
                    if e.ty == EXPLOSIONTYPE_BULLETHOLE && e.age > maxage {
                        maxage = e.age;
                        slot = Some(i);
                    }
                }
            }
        }
        let Some(i) = slot else { return false };
        if ty != EXPLOSIONTYPE_16 && ty != EXPLOSIONTYPE_BULLETHOLE {
            self.shake_total_timer = 6;
        }
        out.flashes.push((pos, t.rangeh));
        let (rmin, rmax) = world.room_bbox(pos);
        let mut exp = Explosion { pos, ty, age: 0, source, makescorch, scorchpos, scorchnormal, owner, bbs: Vec::new(), parts: [ExplosionPart::default(); 40] };
        if ty != EXPLOSIONTYPE_HUGE25 {
            // SUBST: bbs[0] is the room; PD appends the bboxes of the portals
            // the blast overlaps (see the module notes).
            exp.bbs.push(ExplosionBb { bbmin: rmin, bbmax: rmax });
        }
        if !makescorch {
            exp.scorchpos.x = 999_999.875;
        }
        exp.parts[0] = ExplosionPart { pos, size: t.innersize * (rng.randomfrac() * 0.5 + 1.0), rot: rng.randomfrac() * baddtor(360.0), frame: 1, bb: 0 };
        self.slots[i] = Some(exp);
        true
    }

    /// `explosions_update_shake` (`explosions.c:563`): this frame's `vi_shake`
    /// intensity (the camera offset it also computes is discarded by
    /// `player_update_shake`, `player.c:3012`).
    pub fn update_shake(&mut self, playerpos: Vec3) -> f32 {
        if self.shake_total_timer == 0 {
            return 0.0;
        }
        let mut intensity = 0.0;
        for e in self.slots.iter().flatten() {
            let mut dist = (e.pos - playerpos).length();
            if dist == 0.0 {
                dist = 0.0001;
            }
            intensity += etype(e.ty).innersize / dist * 15.0;
        }
        if self.shake_intensity_timer > 0 {
            self.shake_intensity_timer -= 1;
            intensity += 1.0;
        }
        self.shake_total_timer -= 1;
        (self.shake_total_timer as f32 * intensity).clamp(0.0, 14.0)
    }

    /// `explosion_tick` (`explosions.c:1014`) for every live explosion.
    /// `brightness` is the room light scorches are coloured by.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(&mut self, rng: &mut Rng, lv: &Lv, smokes: &mut Smokes, victims: &[Victim], brightness: f32, source_pos: &dyn Fn(u32) -> Option<Vec3>, out: &mut ExpOut) {
        if lv.lvupdate60 == 0 {
            return;
        }
        for slot in self.slots.iter_mut() {
            let Some(exp) = slot else { continue };
            let t = *etype(exp.ty);
            let maxage = t.duration;
            let lvupdate = lv.lvupdate60.min(15);
            if exp.age >= 8 && exp.age < maxage {
                let hrange = exp.hrange_at(exp.age);
                let vrange = exp.vrange_at(exp.age);
                let sp11c = exp.pos - Vec3::new(hrange, vrange, hrange) * 0.5;
                let sp110 = exp.pos + Vec3::new(hrange, vrange, hrange) * 0.5;
                if exp.ty == EXPLOSIONTYPE_GASBARREL && exp.age < 32 {
                    exp.pos.y += 10.0 * lvupdate as f32;
                }
                let numpartstocreate = (t.propagationrate as f32 * exp.age as f32 / maxage as f32) as i32 + 1;
                for _ in 0..numpartstocreate {
                    let Some(j) = exp.parts.iter().position(|p| p.frame == 0) else { break };
                    let (mut spfc, mut spf0, mut bb);
                    if exp.bbs.is_empty() || exp.ty == EXPLOSIONTYPE_HUGE25 {
                        spfc = sp11c;
                        spf0 = sp110;
                        bb = 0;
                    } else {
                        bb = j % exp.bbs.len();
                        spfc = exp.bbs[bb].bbmin.max(sp11c);
                        spf0 = exp.bbs[bb].bbmax.min(sp110);
                        if spf0.x <= spfc.x || spf0.y <= spfc.y || spf0.z <= spfc.z {
                            bb = 0;
                            spfc = exp.bbs[bb].bbmin.max(sp11c);
                            spf0 = exp.bbs[bb].bbmax.min(sp110);
                        }
                    }
                    let px = spfc.x + rng.randomfrac() * (spf0.x - spfc.x);
                    let py = spfc.y + rng.randomfrac() * (spf0.y - spfc.y);
                    let pz = spfc.z + rng.randomfrac() * (spf0.z - spfc.z);
                    let size = (1.0 + rng.randomfrac() * 0.5) * t.innersize;
                    let rot = rng.randomfrac() * baddtor(360.0);
                    exp.parts[j] = ExplosionPart { pos: Vec3::new(px, py, pz), size, rot, frame: 1, bb };
                }
            }

            explosion_inflict_damage(exp, rng, lv, victims, out);

            if exp.age == 0 {
                out.sounds.push((t.sound, exp.pos));
            }

            for _ in 0..lvupdate {
                exp.age += 1;
                for p in exp.parts.iter_mut() {
                    if p.frame > 0 {
                        p.frame += 1;
                    }
                }
                let smoketime = (exp.age == 15 && exp.ty == EXPLOSIONTYPE_GASBARREL) || (exp.age == maxage - 20 && exp.ty != EXPLOSIONTYPE_GASBARREL);
                if smoketime && (exp.ty != EXPLOSIONTYPE_BULLETHOLE || rng.random().is_multiple_of(2)) {
                    let at = exp.source.and_then(source_pos).unwrap_or(exp.pos);
                    smokes.smoke_create_simple(at, t.smoketype);
                }
                // The scorch, at half the duration.
                if exp.age == (maxage >> 1) && exp.makescorch {
                    let mut scorchsize = (2.0 * t.innersize).min(100.0);
                    scorchsize *= 0.8 + 0.2 * rng.randomfrac();
                    let wh = wallhit_create_with_20_args(rng, exp.scorchpos, exp.scorchnormal, Some(exp.pos), WALLHITTEX_SCORCH, scorchsize, scorchsize, 0xff, 0xff, 0, brightness);
                    out.wallhits.push(wh);
                }
            }

            // explosion_render's reset of finished parts (`explosions.c:1304`),
            // done here so the renderer stays read-only.
            let tmp = (t.flarespeed * 15.0) as i32;
            for p in exp.parts.iter_mut() {
                if p.frame > tmp {
                    p.frame = 0;
                }
            }

            if exp.age >= maxage + (16.0 * t.flarespeed) as i32 {
                *slot = None;
            }
        }
    }

    pub fn live(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

/// `explosion_inflict_damage` (`explosions.c:663`): objects take a linear
/// falloff floored at 30 %, chrs a squared one ×8; after the first frame both
/// take 5 % a tick. The room light flickers.
fn explosion_inflict_damage(exp: &Explosion, rng: &mut Rng, lv: &Lv, victims: &[Victim], out: &mut ExpOut) {
    if lv.lvupdate60 <= 0 {
        return;
    }
    let t = etype(exp.ty);
    if t.damage <= 0.0 {
        return;
    }
    let isfirstframe = exp.age <= 0;
    let damageradius = if isfirstframe { t.damageradius } else { (t.blastradius + (t.damageradius - t.blastradius) * exp.age as f32 / t.duration as f32).min(t.damageradius) };
    if exp.age > (t.duration as f32 + 7.0 * t.flarespeed) as i32 {
        return;
    }
    // Flicker the room lighting (the explosion's own room).
    if rng.random() % 2048 <= 240 {
        out.flashes.push((exp.pos, t.rangeh));
    }
    for v in victims {
        if let VictimId::Prop(id) = v.id {
            if Some(id) == exp.source {
                continue;
            }
        }
        let d = v.pos - exp.pos;
        if d.x.abs() > damageradius || d.y.abs() > damageradius || d.z.abs() > damageradius {
            continue;
        }
        let frac = |a: f32| 1.0 - (a / damageradius).abs();
        let minfrac = frac(d.x).min(frac(d.y)).min(frac(d.z));
        match v.chrbox {
            Some(_) => {
                // SUBST: explosion_overlaps_prop (the chr's box against the
                // rooms the blast reached) is always true.
                let mut dmg = minfrac * minfrac * t.damage * 8.0;
                let mut dir = Vec3::ZERO;
                if isfirstframe {
                    if d.x != 0.0 || d.z != 0.0 {
                        let dist = (d.x * d.x + d.z * d.z).sqrt();
                        if dist > 0.0 {
                            dir = Vec3::new(d.x / dist, 0.0, d.z / dist);
                        }
                    }
                } else {
                    dmg *= 0.05 * lv.lvupdate60freal;
                }
                out.damage.push((v.id, dmg, dir, isfirstframe, exp.owner));
            }
            None => {
                let minfrac = (minfrac * 0.7 + 0.3) * t.damage;
                let dmg = if isfirstframe { (rng.randomfrac() * 0.5 + 1.0) * minfrac } else { (rng.randomfrac() * 0.5 + 1.0) * minfrac * 0.05 * lv.lvupdate60freal };
                out.damage.push((v.id, dmg, Vec3::ZERO, isfirstframe, exp.owner));
            }
        }
    }
}
