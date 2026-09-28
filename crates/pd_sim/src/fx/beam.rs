//! Bullet tracers: `struct beam`, `beam_create` and `beam_tick` (`gunfx.c`).

use glam::Vec3;
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::rng::Rng;

/// The laser's stream (`weaponnum == -2`): a beam that lives one tick.
pub const BEAM_LASERSTREAM: i32 = -2;
/// `beam_create`'s -1: a fire slot's beam that doesn't travel.
pub const BEAM_STATIC: i32 = -1;

/// `struct beam`. `weaponnum` is PD's signed byte: a `WEAPON_*`, or -1 / -2,
/// or `-3 - lasertype` for a charged Mauler round.
#[derive(Clone, Debug)]
pub struct Beam {
    pub age: i32,
    pub weaponnum: i32,
    pub from: Vec3,
    pub dir: Vec3,
    pub maxdist: f32,
    pub speed: f32,
    pub mindist: f32,
    pub dist: f32,
}

impl Default for Beam {
    fn default() -> Self {
        Beam { age: -1, weaponnum: 0, from: Vec3::ZERO, dir: Vec3::Z, maxdist: 0.0, speed: 0.0, mindist: 0.0, dist: 0.0 }
    }
}

impl Beam {
    /// `beam_create` (`gunfx.c:27`).
    pub fn create(&mut self, rng: &mut Rng, weaponnum: i32, from: Vec3, to: Vec3) {
        self.from = from;
        let d = to - from;
        let mut distance = d.length();
        self.dir = if distance > 0.0 { d / distance } else { d };
        if distance > 10000.0 {
            distance = 10000.0;
        }
        self.age = 0;
        self.weaponnum = weaponnum;
        self.maxdist = distance;
        if distance < 500.0 {
            distance = 500.0;
        }
        if weaponnum == BEAM_STATIC || weaponnum == BEAM_LASERSTREAM {
            self.speed = 0.0;
            self.mindist = distance.min(3000.0);
            self.dist = 0.0;
        } else if weaponnum == WEAPON_LASER as i32 {
            self.speed = 0.25 * distance;
            self.mindist = (0.6 * distance).min(3000.0);
            self.dist = (-0.1 - rng.randomfrac() * 0.3) * distance;
        } else {
            self.speed = 0.2 * distance;
            self.mindist = (0.2 * distance).min(3000.0);
            let tmp = rng.randomfrac();
            self.dist = (tmp + tmp - 1.0) * self.speed;
        }
        if self.dist >= self.maxdist {
            self.age = -1;
        }
    }

    /// `beam_tick` (`gunfx.c:600`).
    pub fn tick(&mut self, rng: &mut Rng, lv: &Lv) {
        if self.age < 0 {
            return;
        }
        if self.weaponnum == BEAM_LASERSTREAM {
            self.age += 1;
            if self.age > 1 {
                self.age = -1;
            }
        } else {
            if lv.lvupdate240 <= 8 {
                self.dist += self.speed * lv.lvupdate60freal;
            } else {
                self.dist += self.speed * (2.0 + rng.randomfrac() * 0.5);
            }
            if self.dist >= self.maxdist {
                self.age = -1;
            }
        }
    }

    pub fn live(&self) -> bool {
        self.age >= 0
    }
}
