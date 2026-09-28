//! Ejected cartridge cases: `casing_create_for_hand` (`gunfx.c:662`) and
//! `casing_tick` (`casingtick.c:13`).

use glam::{Mat3, Mat4, Vec3};
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::math::{self, baddtor};
use pd_core::rng::Rng;

/// `g_CartFileNums` by `casingeject`: the model stems.
pub const CART_MODELS: [&str; 4] = ["cartridge", "cartrifle", "cartblue", "cartshell"];

#[derive(Clone, Debug)]
pub struct Casing {
    /// Index into [`CART_MODELS`].
    pub model: usize,
    pub pos: Vec3,
    pub speed: Vec3,
    pub rot: Mat3,
    pub rotspeed: Mat3,
    /// The floor it falls to (`casing->ground`): the thrower's feet.
    pub ground: f32,
}

/// `casing_create_for_hand` (`gunfx.c:662`). `mtx` is the eject node's world
/// matrix; `handvel` the hand's world displacement this frame per 60 Hz tick.
pub fn casing_create_for_hand(rng: &mut Rng, weaponnum: u8, casingtype: i32, ground: f32, mtx: &Mat4, handvel: Vec3) -> Option<Casing> {
    if casingtype < 0 || casingtype as usize >= CART_MODELS.len() {
        return None;
    }
    let pos = mtx.w_axis.truncate();
    let rot = Mat3::from_mat4(*mtx);
    let rand_rot = |rng: &mut Rng, div: f32, off: f32| -> Mat3 {
        let a = Vec3::new(
            2.0 * rng.randomfrac() * baddtor(360.0) * div - off,
            2.0 * rng.randomfrac() * baddtor(360.0) * div - off,
            2.0 * rng.randomfrac() * baddtor(360.0) * div - off,
        );
        Mat3::from_mat4(math::load_rotation(a))
    };
    let mut speed;
    let rotspeed;
    if matches!(weaponnum, WEAPON_PP9I | WEAPON_CC13 | WEAPON_FALCON2 | WEAPON_MAGSEC4) {
        speed = Vec3::new(-(rng.randomfrac() * 0.533_333_3 * (1.0 / 16.0) + 0.533_333_3), rng.randomfrac() * 2.5 * (1.0 / 16.0) + 2.5, 0.0);
        speed = math::rotate(mtx, speed);
        rotspeed = rand_rot(rng, 1.0 / 16.0, baddtor(22.5));
    } else {
        speed = if weaponnum == WEAPON_REAPER {
            Vec3::new(-(rng.randomfrac() * 0.416_666_66 * 0.125 + 0.416_666_66), rng.randomfrac() * 3.333_333_3 * 0.125 + 3.333_333_3, 0.0)
        } else {
            Vec3::new(-(rng.randomfrac() * 1.416_666_6 * 0.125 + 1.416_666_6), rng.randomfrac() * 1.666_666_6 * 0.125 + 1.666_666_6, 0.0)
        };
        if weaponnum == WEAPON_DY357MAGNUM || weaponnum == WEAPON_DY357LX {
            speed = Vec3::new(0.0, 0.0, -1.0);
        }
        speed = math::rotate(mtx, speed);
        if weaponnum == WEAPON_REAPER {
            let r = rand_rot(rng, 1.0 / 64.0, 0.098_159_14);
            speed = r * speed;
        }
        rotspeed = rand_rot(rng, 1.0 / 64.0, 0.098_159_14);
    }
    // The sub-frame head start: f0 is a random fraction of a frame, in frames.
    let magic: u32 = 0x15aca6;
    let sp5c = (((rng.random() >> 24).wrapping_mul(magic) as i32) >> 10) as u32 + magic;
    let f0 = (rng.random() % sp5c) as f32 / (46_875_000.0 / 60.0);
    let newyspeed = speed.y - f0 * 0.277_777_8;
    let mut pos = pos;
    pos.y += f0 * (speed.y + newyspeed) * 0.5;
    pos.x += f0 * speed.x;
    pos.z += f0 * speed.z;
    speed.y = newyspeed;
    speed += handvel;
    Some(Casing { model: casingtype as usize, pos, speed, rot, rotspeed, ground })
}

impl Casing {
    /// `casing_tick` (`casingtick.c:13`). True when it reaches the ground,
    /// where it is removed (the caller plays `SFXMAP_8051`).
    pub fn tick(&mut self, lv: &Lv) -> bool {
        let l = lv.lvupdate60freal;
        let tmp = self.speed.y - l * (1.0 / 3.6);
        self.pos.y += l * 0.5 * (self.speed.y + tmp);
        if self.pos.y < self.ground {
            return true;
        }
        self.speed.y = tmp;
        self.pos.x += l * self.speed.x;
        self.pos.z += l * self.speed.z;
        for _ in 0..lv.lvupdate240 {
            self.rot = self.rotspeed * self.rot;
        }
        false
    }

    /// `casing_render`'s model matrix, world space.
    pub fn world_matrix(&self) -> Mat4 {
        let mut m = Mat4::from_mat3(self.rot);
        math::scale3(&mut m, 0.1);
        math::set_translation(&mut m, self.pos);
        m
    }
}
