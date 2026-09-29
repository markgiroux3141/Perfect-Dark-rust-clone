//! Hover props (`OBJTYPE_HOVERPROP`: Warehouse's floating crates): each floats
//! over the ground under it, bobbing up and down and rocking in pitch and roll
//! towards random targets, and tilted to the slope beneath (`hov_tick`,
//! `propobj.c:5241`), while on some player's screen.
//!
//! Hover bikes and beds, grabbing and mounting aren't in the Combat Simulator.

use glam::{Mat3, Vec3};
use pd_core::ids::*;
use pd_core::math::{self, baddtor, dtor};

use super::autogun::{apply_rotation, apply_speed};
use super::setup::obj_update_all_geo;
use super::Obj;
use crate::world::World;

/// `HOVFLAG_FIRSTTICK`.
const HOVFLAG_FIRSTTICK: u8 = 1;

/// `struct hovtype`: the bob's middle height, and for the height, the pitch
/// and the roll the least and random extra swing, acceleration and top speed.
#[derive(Clone, Copy, Debug)]
struct HovType {
    bobymid: f32,
    y: [f32; 4],
    pitch: [f32; 4],
    roll: [f32; 4],
}

/// `g_HovTypes` (`propobj.c:4873`), NTSC.
const HOVTYPES: [HovType; 5] = [
    // HOVTYPE_BED
    HovType { bobymid: 90.0, y: [1.0, 2.0, 0.0010, 1.0], pitch: [0.006_282_185_2, 0.006_282_185_2, 0.000_010_470_309, 0.000_314_109_26], roll: [0.006_282_185_2, 0.006_282_185_2, 0.000_010_470_309, 0.000_314_109_26] },
    // HOVTYPE_BIKE
    HovType { bobymid: 80.0, y: [1.0, 3.0, 0.0025, 0.1], pitch: [0.012_564_37, 0.018_846_555, 0.000_020_940_617, 0.000_628_218_5], roll: [0.012_564_37, 0.018_846_555, 0.000_020_940_617, 0.000_628_218_5] },
    // HOVTYPE_CRATE
    HovType { bobymid: 70.0, y: [2.0, 4.0, 0.0010, 1.0], pitch: [0.006_282_185_2, 0.012_564_37, 0.000_010_470_309, 0.000_314_109_26], roll: [0.006_282_185_2, 0.012_564_37, 0.000_010_470_309, 0.000_314_109_26] },
    // HOVTYPE_3
    HovType { bobymid: 170.0, y: [2.0, 2.0, 0.0010, 1.0], pitch: [0.003_141_092_6, 0.003_141_092_6, 0.000_005_235_154, 0.000_188_465_55], roll: [0.003_141_092_6, 0.003_141_092_6, 0.000_005_235_154, 0.000_188_465_55] },
    // HOVTYPE_4
    HovType { bobymid: 170.0, y: [2.0, 2.0, 0.0010, 1.0], pitch: [0.003_141_092_6, 0.003_141_092_6, 0.000_005_235_154, 0.000_188_465_55], roll: [0.003_141_092_6, 0.003_141_092_6, 0.000_005_235_154, 0.000_188_465_55] },
];

/// `struct hov`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hov {
    pub ty: u8,
    pub flags: u8,
    pub bobycur: f32,
    pub bobytarget: f32,
    pub bobyspeed: f32,
    pub yrot: f32,
    pub bobpitchcur: f32,
    pub bobpitchtarget: f32,
    pub bobpitchspeed: f32,
    pub bobrollcur: f32,
    pub bobrolltarget: f32,
    pub bobrollspeed: f32,
    pub groundpitch: f32,
    pub y: f32,
    pub ground: f32,
    pub prevframe60: i32,
    pub prevgroundframe60: i32,
    /// `PROPFLAG_ONANYSCREENTHISTICK` as the last tick left it (the next
    /// tick's `PROPFLAG_ONANYSCREENPREVTICK`).
    pub onanyscreen: bool,
}

/// `setup_create_hov` (`setup.c:1099`): at rest, facing the way it was placed.
pub fn setup_create_hov(o: &Obj, ty: u8) -> Hov {
    Hov { ty, flags: HOVFLAG_FIRSTTICK, yrot: math::atan2f(o.realrot.z_axis.x, o.realrot.z_axis.z), prevframe60: -1, prevgroundframe60: -1, ..Default::default() }
}

impl World {
    /// The hover prop from the setup's `hover_prop()` (`setup.c:1600`).
    pub(crate) fn setup_create_hoverprop(&mut self, p: &serde_json::Value) {
        let int = |k: &str| p.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let Some(o) = self.setup_obj(int("model") as i32, OBJTYPE_HOVERPROP, int("flags") as u32, int("flags2") as u32, int("flags3") as u32, int("maxdamage") as i32) else { return };
        let n = self.props.objs.len();
        self.setup_create_object(o, int("pad") as i32, int("scale") as i32);
        if let Some(o) = self.props.objs.get_mut(n) {
            o.hov = Some(setup_create_hov(o, int("hovtype") as u8));
        }
    }

    /// `hoverprop_tick` (`propobj.c:10785`): not while grabbed, and only for a
    /// prop on some player's screen last tick (or one that moved). Then this
    /// tick's on-screen state for the next.
    pub(crate) fn hoverprop_tick(&mut self, i: usize) {
        let o = &self.props.objs[i];
        let h = o.hov.as_ref().unwrap();
        if o.hidden & OBJHFLAG_GRABBED == 0 && (h.onanyscreen || o.flags & OBJFLAG_CHOPPER_INACTIVE != 0) {
            self.hov_tick(i);
        }
        let o = &self.props.objs[i];
        let scale = o.def.scale * o.scale;
        let pos = o.pos;
        let on = self.players.iter().any(|p| crate::chr::body::pos_is_onscreen(&p.cam, pos, scale));
        self.props.objs[i].hov.as_mut().unwrap().onanyscreen = on;
    }

    /// The ground a hover prop sees at `pos`: `cd_find_ground_at_cyl_ct` with a
    /// 5 cm cylinder over the tiles and the other props' floors.
    fn hov_ground_at(&self, i: usize, pos: Vec3) -> f32 {
        let id = self.props.objs[i].id;
        let floors: Vec<_> = self.prop_floors().into_iter().filter(|f| f.prop != id).collect();
        self.level.cd_find_ground_at_cyl_ctfril(pos, 5.0, &floors).y
    }

    /// `hov_tick` (`propobj.c:5241`) for a hover prop.
    fn hov_tick(&mut self, i: usize) {
        let lvframe60 = self.lv.lvframe60;
        let lv = self.lv.clone();
        let mut h = *self.props.objs[i].hov.as_ref().unwrap();
        if lvframe60 <= h.prevframe60 {
            return;
        }
        let (pos, flags) = (self.props.objs[i].pos, self.props.objs[i].flags);
        let ty = HOVTYPES[h.ty as usize % HOVTYPES.len()];
        let mut moved = false;
        if lvframe60 > h.prevgroundframe60 {
            // hov_update_ground (`propobj.c:5216`).
            if lvframe60 > h.prevframe60 {
                let ground = self.hov_ground_at(i, pos);
                h.ground = if ground < -30000.0 { h.ground } else { ground };
                h.prevgroundframe60 = lvframe60;
            }
        }
        h.prevframe60 = lvframe60;

        // The ground's angle under the prop, front to back.
        let groundangle = if flags & OBJFLAG_DEACTIVATED != 0 {
            0.0
        } else {
            let o = &self.props.objs[i];
            let (sp1d0, sp1cc) = if o.flags3 & OBJFLAG3_GEOCYL != 0 {
                let r = o.bbox.xmax.max(o.bbox.zmax) * o.scale * 0.9;
                (-r, r)
            } else {
                (o.bbox.zmin * 0.9 * o.scale, o.bbox.zmax * 0.9 * o.scale)
            };
            let (spbc, spb8) = (h.yrot.cos(), h.yrot.sin());
            let back = Vec3::new(pos.x + sp1d0 * spb8, pos.y, pos.z + sp1d0 * spbc);
            let front = Vec3::new(pos.x + sp1cc * spb8, pos.y, pos.z + sp1cc * spbc);
            let ground1 = self.hov_ground_at(i, back);
            let ground2 = self.hov_ground_at(i, front);
            let wrap = |a: f32| if a >= dtor(180.0) { a - baddtor(360.0) } else { a };
            if ground1 >= -30000.0 && ground2 >= -30000.0 {
                wrap(math::atan2f(ground1 - ground2, sp1cc - sp1d0))
            } else if ground1 >= -30000.0 {
                wrap(math::atan2f(ground1 - h.ground, -sp1d0))
            } else if ground2 >= -30000.0 {
                wrap(math::atan2f(h.ground - ground2, sp1cc))
            } else {
                0.0
            }
        };
        let ground = h.ground;
        if h.flags & HOVFLAG_FIRSTTICK != 0 {
            moved = true;
            h.bobycur = ty.bobymid;
            h.bobytarget = ty.bobymid;
            h.y = ground;
            h.flags &= !HOVFLAG_FIRSTTICK;
        }

        // The height's bob.
        apply_speed(&lv, &mut h.bobycur, h.bobytarget, &mut h.bobyspeed, ty.y[2], ty.y[2], ty.y[3]);
        if h.bobytarget >= ty.bobymid && h.bobycur >= h.bobytarget {
            h.bobyspeed = 0.0;
            h.bobytarget = ty.bobymid - ty.y[0] - self.rng.randomfrac() * ty.y[1];
        } else if h.bobytarget < ty.bobymid && h.bobycur <= h.bobytarget {
            h.bobyspeed = 0.0;
            h.bobytarget = ty.bobymid + ty.y[0] + self.rng.randomfrac() * ty.y[1];
        }
        // The pitch's and the roll's.
        for (cur, target, speed, b) in [(&mut h.bobpitchcur, &mut h.bobpitchtarget, &mut h.bobpitchspeed, ty.pitch), (&mut h.bobrollcur, &mut h.bobrolltarget, &mut h.bobrollspeed, ty.roll)] {
            apply_rotation(&lv, cur, *target, speed, b[2], b[2], b[3]);
            if *cur == *target && *speed <= 2.0 * b[2] && *speed >= 2.0 * -b[2] {
                *speed = 0.0;
                *target = if *target < dtor(180.0) { baddtor(360.0) - b[0] - self.rng.randomfrac() * b[1] } else { b[0] + self.rng.randomfrac() * b[1] };
            }
        }

        for _ in 0..lv.lvupdate60 {
            h.groundpitch += (groundangle - h.groundpitch) * 0.075;
            let mut f0 = ground - h.y;
            let mut f12 = 0.085f32;
            if h.y < h.ground {
                let f2 = f0.abs();
                if f2 > 10.0 {
                    f12 *= 1.0 + (f2 - 10.0) * 0.2;
                }
                if f12 > 0.5 {
                    f12 = 0.5;
                }
                f0 *= f12;
            } else {
                f0 = (f0 * f12).clamp(-5.0, 5.0);
            }
            h.y += f0;
            if !(-1.0..=1.0).contains(&f0) {
                moved = true;
            }
        }

        let o = &mut self.props.objs[i];
        // obj_onmoved before the new height and rotation, as PD does.
        if moved {
            obj_update_all_geo(o);
        }
        if h.y < h.ground - 5.0 || h.y > h.ground + 5.0 {
            o.flags |= OBJFLAG_HOVERCAR_ISHOVERBOT;
        } else {
            o.flags &= !OBJFLAG_HOVERCAR_ISHOVERBOT;
        }
        // obj_get_hov_bob_offset_y: a hover type's is the bob.
        o.pos.y = h.bobycur + h.y;
        let mut xrot = h.groundpitch + h.bobpitchcur;
        if xrot >= baddtor(360.0) {
            xrot -= baddtor(360.0);
        } else if xrot < 0.0 {
            xrot += baddtor(360.0);
        }
        // mtx00015be0 (b = a × b) twice: Ry × Rx × Rz, scaled.
        let m = math::mtx4_load_y_rotation(h.yrot) * math::mtx4_load_x_rotation(xrot) * math::mtx4_load_z_rotation(h.bobrollcur);
        o.realrot = Mat3::from_mat4(m) * o.scale;
        o.hov = Some(h);
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil;
    use pd_core::ids::*;

    /// Warehouse's three hover crates float 70 cm over the floor, give or
    /// take their bob, once someone looks at them.
    #[test]
    fn warehouses_crates_hover() {
        let mut w = testutil::arena("mp4");
        let ids: Vec<u32> = w.props.objs.iter().filter(|o| o.ty == OBJTYPE_HOVERPROP).map(|o| o.id).collect();
        assert_eq!(ids.len(), 3);
        for id in ids {
            // Stand 4 m off, facing the crate.
            let c = w.props.get(id).unwrap().pos;
            let level = w.level.clone();
            let spot = (0..16)
                .map(|k| {
                    let a = k as f32 * std::f32::consts::TAU / 16.0;
                    glam::Vec3::new(c.x + a.sin() * 400.0, c.y + 50.0, c.z + a.cos() * 400.0)
                })
                .find(|p| level.cd_find_ground_at_cyl(*p, 30.0).1.is_some())
                .unwrap();
            let d = c - spot;
            w.players[0].start_new_life(&level, &[], spot, pd_core::math::atan2f(d.x, d.z));
            let idle = crate::player::PlayerInput::default();
            for _ in 0..240 {
                w.step(4, std::slice::from_ref(&idle));
            }
            let o = w.props.get(id).unwrap();
            let h = o.hov.as_ref().unwrap();
            let above = o.pos.y - h.ground;
            assert!((64.0..=76.0).contains(&above), "crate {id} floats {above} cm up");
            assert!((h.y - h.ground).abs() < 5.0);
        }
    }
}
