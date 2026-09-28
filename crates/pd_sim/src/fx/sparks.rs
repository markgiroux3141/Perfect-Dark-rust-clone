//! Impact sparks: `g_SparkTypes`, `sparks_create` (`sparks.c`) and
//! `sparks_tick` (`sparkstick.c`). 100 sparks in 10 groups, recycled in order.

use glam::Vec3;
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::rng::Rng;

/// `struct sparktype`.
#[derive(Clone, Copy, Debug)]
pub struct SparkType {
    pub unk00: u16,
    pub unk02: i16,
    pub unk04: u16,
    pub unk06: u16,
    pub unk08: u16,
    pub unk0a: u16,
    pub weight: f32,
    pub maxage: u16,
    pub unk12: u16,
    pub numsparks: u16,
    pub unk18: u32,
    pub col0: u32,
    pub col1: u32,
    pub decel: f32,
}

/// The `g_SparkTypes[]` rows the guns use (`sparks.c:38`).
pub fn spark_type(t: usize) -> SparkType {
    let base = SparkType {
        unk00: 100,
        unk02: 28,
        unk04: 100,
        unk06: 1,
        unk08: 0,
        unk0a: 0,
        weight: 2.0,
        maxage: 60,
        unk12: 60,
        numsparks: 15,
        unk18: 1,
        col0: 0xffff80ff,
        col1: 0xffffffff,
        decel: 0.02,
    };
    match t {
        SPARKTYPE_ELECTRICAL => SparkType { col0: 0x80ffffff, ..base },
        // A human body's blood colour (`chr_get_blood_colour`) is the table's own.
        SPARKTYPE_BLOOD => SparkType { unk00: 40, unk02: -1, unk04: 30, unk06: 30, weight: 2.0, maxage: 35, unk12: 35, numsparks: 5, col0: 0x301010ff, col1: 0x401010ff, ..base },
        SPARKTYPE_FLESH => SparkType { unk00: 40, unk02: -1, unk04: 300, unk06: 200, weight: 0.15, maxage: 5, unk12: 5, numsparks: 4, col0: 0xffffff40, col1: 0x560011a0, ..base },
        SPARKTYPE_FLESH_LARGE => SparkType { unk00: 10, unk02: 1, unk04: 1200, unk06: 400, weight: 0.15, maxage: 5, unk12: 5, numsparks: 5, col0: 0xa0a0e000, col1: 0xffffffff, ..base },
        SPARKTYPE_PROJECTILE => SparkType { unk00: 50, weight: 1.0, unk12: 30, numsparks: 10, ..base },
        SPARKTYPE_BGHIT_ORANGE => SparkType { maxage: 120, unk12: 120, numsparks: 30, col0: 0xff8080ff, col1: 0xffff80ff, ..base },
        SPARKTYPE_BGHIT_GREEN => SparkType { col0: 0x4fff4fff, ..base },
        SPARKTYPE_BGHIT_TRANQULIZER => SparkType { col0: 0xffff7f7f, ..base },
        _ => base,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Spark {
    pub pos: Vec3,
    pub speed: Vec3,
    pub ttl: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct SparkGroup {
    pub ty: SparkType,
    pub numsparks: usize,
    pub age: i32,
    pub startindex: usize,
    pub pos: Vec3,
}

#[derive(Clone)]
pub struct Sparks {
    pub sparks: Vec<Spark>,
    next: usize,
    pub groups: Vec<Option<SparkGroup>>,
    next_group: usize,
}

impl Default for Sparks {
    fn default() -> Self {
        Sparks { sparks: vec![Spark::default(); 100], next: 0, groups: vec![None; 10], next_group: 0 }
    }
}

impl Sparks {
    /// Live spark groups.
    pub fn live(&self) -> usize {
        self.groups.iter().flatten().count()
    }

    /// `spark_create` (`sparks.c:67`).
    fn spark_create(&mut self, rng: &mut Rng, pos: Vec3, ty: &SparkType) {
        let i = self.next;
        self.next = (self.next + 1) % self.sparks.len();
        let r = ty.unk00 as u32 * 2 + 1;
        let mut speed = Vec3::new(
            (rng.random() % r) as i32 as f32 - ty.unk00 as f32,
            (rng.random() % r) as i32 as f32 - ty.unk00 as f32,
            (rng.random() % r) as i32 as f32 - ty.unk00 as f32,
        );
        if speed.y == 0.0 {
            speed.y = -0.0001;
        }
        let maxspeed = speed.abs().max_element();
        let len = speed.length();
        if len > 0.0 {
            speed *= maxspeed / len;
        }
        speed.y += (ty.unk00 / 2) as f32;
        speed += pos;
        if speed.y == 0.0 {
            speed.y = -0.0001;
        }
        let ttl = if ty.unk18 % 2 == 1 { (rng.random() % ty.maxage as u32) as i32 } else { ty.maxage as i32 };
        self.sparks[i] = Spark { pos: Vec3::ZERO, speed, ttl };
    }

    /// `sparks_create` (`sparks.c:148`): `dir` is the shot's direction,
    /// `normal` the surface's (zero for none); the group sprays along the
    /// reflection.
    pub fn create(&mut self, rng: &mut Rng, pos: Vec3, dir: Vec3, normal: Vec3, typenum: usize) {
        let ty = spark_type(typenum);
        let gi = self.next_group;
        self.next_group = (self.next_group + 1) % self.groups.len();
        let grouppos = if normal != Vec3::ZERO {
            let n = normal.normalize_or_zero();
            let refl = dir + n * (-2.0 * dir.dot(n));
            let l = refl.length();
            refl * (ty.unk02 as f32 / if l == 0.0 { 1.0 } else { l })
        } else if (-1..2).contains(&ty.unk02) {
            // No surface (`chr_emit_sparks` passes NULL): along the shot.
            dir * 10.0 * ty.unk02 as f32
        } else {
            Vec3::ZERO
        };
        let start = self.next;
        for _ in 0..ty.numsparks {
            // sparkgroup_ensure_free_spark_slot
            for (k, g) in self.groups.iter_mut().enumerate() {
                if k == gi {
                    continue;
                }
                if let Some(grp) = g {
                    if grp.startindex == self.next {
                        grp.startindex = (grp.startindex + 1) % 100;
                        grp.numsparks -= 1;
                        if grp.numsparks == 0 {
                            *g = None;
                        }
                    }
                }
            }
            self.spark_create(rng, grouppos, &ty);
        }
        self.groups[gi] = Some(SparkGroup { ty, numsparks: ty.numsparks as usize, age: 1, startindex: start, pos });
    }

    /// `sparks_tick` (`sparkstick.c:7`).
    pub fn tick(&mut self, lv: &Lv) {
        for g in self.groups.iter_mut() {
            let Some(grp) = g else { continue };
            if grp.age >= grp.ty.maxage as i32 {
                *g = None;
                continue;
            }
            for _ in 0..lv.lvupdate60 {
                grp.age += 1;
                let mut idx = grp.startindex;
                for _ in 0..grp.numsparks {
                    let s = &mut self.sparks[idx];
                    if s.ttl != 0 {
                        s.speed.x -= s.speed.x * grp.ty.decel;
                        s.speed.y = (s.speed.y - s.speed.y * grp.ty.decel) - grp.ty.weight;
                        s.speed.z -= s.speed.z * grp.ty.decel;
                        if s.speed.y == 0.0 {
                            s.speed.y = -0.0001;
                        }
                        s.pos += s.speed;
                        s.ttl -= 1;
                    }
                    idx = (idx + 1) % 100;
                }
            }
        }
    }
}
