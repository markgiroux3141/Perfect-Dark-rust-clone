//! Shards (`shards.c`, `shardstick.c`): the triangles a broken pane of glass
//! (or a bottle, or wood) bursts into. `shards_create` lays a grid of them
//! over the object's box face, each flung out with a random speed and spin;
//! `shards_tick` moves them under gravity for 2.5 s. `pd_render::fx` draws them.

use glam::Vec3;
use pd_core::ids::SHARDTYPE_WOOD;
use pd_core::math::baddtor;
use pd_core::rng::Rng;

/// `struct shard`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shard {
    pub room: Option<u16>,
    /// 0: free; counts up from 1.
    pub age60: i32,
    pub pos: Vec3,
    /// Euler angles (`mtx4_load_rotation_and_translation`).
    pub rot: Vec3,
    pub vel: Vec3,
    pub rotspeed: Vec3,
    /// The triangle's corners in the shard's own x/y plane.
    pub verts: [[f32; 2]; 3],
    /// RGBA per corner.
    pub colours: [[u8; 4]; 3],
    pub ty: u8,
}

/// `g_Shards`: a ring of `g_MaxShards`.
#[derive(Clone, Debug, Default)]
pub struct Shards {
    pub shards: Vec<Shard>,
    next: usize,
    /// `g_ShardsActive`.
    pub active: bool,
}

impl Shards {
    /// `shards_reset` (`shardsreset.c:8`): 200 shared by the human players.
    pub fn new(playercount: usize) -> Shards {
        let max = 200 / playercount.max(1);
        Shards { shards: vec![Shard::default(); max], next: 0, active: false }
    }

    /// `shards_create` (`shards.c:24`) for a box face: `rot`'s x and y
    /// columns span it (their lengths scale the box's x and y extents), its z
    /// column sets the shards' tilt. The caller plays the breaking sound.
    #[allow(clippy::too_many_arguments)]
    pub fn shards_create(&mut self, rng: &mut Rng, pos: Vec3, rotx: Vec3, roty: Vec3, rotz: Vec3, mut relxmin: f32, mut relxmax: f32, mut relymin: f32, mut relymax: f32, ty: u8, room: Option<u16>) {
        if self.shards.is_empty() {
            return;
        }
        let f0 = rotx.length();
        let spcc = rotx * (1.0 / f0);
        relxmin *= f0;
        relxmax *= f0;
        let f0 = roty.length();
        let spc0 = roty * (1.0 / f0);
        relymin *= f0;
        relymax *= f0;
        let f30 = pd_core::math::atan2f(rotz.x, rotz.z);
        let f20 = relxmax - relxmin;
        let spac = relymax - relymin;
        let spec = (f20 * spac / (self.shards.len() / 2) as f32).sqrt();
        let speci = spec as i32;
        let speci2 = speci;
        if speci <= 0 {
            return;
        }
        let half = (speci >> 1) as f32;
        let basepos = pos + (relxmin + half) * spcc + spc0 * (relymin + half);
        let xmax = (f20 / speci as f32) as i32;
        let ymax = (spac / speci2 as f32) as i32;
        for y in 0..ymax {
            let f20 = y as f32 * speci2 as f32;
            for x in 0..xmax {
                let thispos = basepos + x as f32 * speci as f32 * spcc + spc0 * f20;
                let size = (rng.randomfrac() * 0.7 + 0.1) * spec;
                self.shard_create(rng, room, thispos, f30, size, ty);
            }
        }
    }

    /// `shard_create` (`shards.c:114`).
    fn shard_create(&mut self, rng: &mut Rng, room: Option<u16>, pos: Vec3, rotx: f32, size: f32, ty: u8) {
        let velx = rng.randomfrac() * 2.0 - 1.0;
        let vely = rng.randomfrac() * 1.12 - 0.12;
        let velz = rng.randomfrac() * 2.0 - 1.0;
        let mut s = Shard { ty, room, age60: 1, pos, vel: Vec3::new(velx * 1.5, vely * 3.0, velz * 1.5), ..Default::default() };
        let mut corner = |sx: f32, sy: f32| [(rng.randomfrac() * 0.5 + 1.0) * size * sx, (rng.randomfrac() * 0.5 + 1.0) * size * sy];
        let v0 = corner(1.0, 1.0);
        let v1 = corner(1.0, -1.0);
        let v2 = corner(-1.0, -1.0);
        s.verts = [v0, v1, v2];
        if ty == SHARDTYPE_WOOD {
            let r = rng.random() % 100;
            let word: [u32; 3] = if r < 20 {
                [0xbbbb_bbf0, 0xaaaa_aaf0, 0x7777_77f0]
            } else if r < 40 {
                [0x0000_00f0; 3]
            } else if r < 60 {
                [0x5533_11f0; 3]
            } else {
                [0xddaa_88f0; 3]
            };
            for (c, w) in s.colours.iter_mut().zip(word) {
                *c = w.to_be_bytes();
            }
        } else {
            // The template (05 05 7e / 05 fb 7e / fb fb 7e) is overwritten
            // by random bytes before it is used.
            for c in s.colours.iter_mut() {
                for b in c.iter_mut().take(3) {
                    *b = (rng.random() % 0xff) as u8;
                }
                c[3] = 0xff;
            }
        }
        s.rot = Vec3::new(rotx, 0.0, rng.randomfrac() * baddtor(360.0));
        s.rotspeed = Vec3::new(rng.randomfrac() * 0.1, rng.randomfrac() * 0.1, rng.randomfrac() * 0.1);
        self.shards[self.next] = s;
        self.next += 1;
        if self.next >= self.shards.len() {
            self.next = 0;
        }
        self.active = true;
    }

    /// `shards_tick` (`shardstick.c:7`): at most 15 ticks' movement a frame,
    /// gone after 150 ticks or far out of the world.
    pub fn shards_tick(&mut self, lvupdate60: i32) {
        if !self.active {
            return;
        }
        let lvupdate = lvupdate60.min(15) as f32;
        for s in self.shards.iter_mut().filter(|s| s.age60 > 0) {
            s.age60 += lvupdate as i32;
            s.rot += s.rotspeed * lvupdate;
            s.pos.x += s.vel.x * lvupdate;
            s.pos.z += s.vel.z * lvupdate;
            for _ in 0..lvupdate as i32 {
                s.pos.y += s.vel.y;
                s.vel.y -= 0.1;
            }
            if s.age60 >= 150 || s.pos.y < -30000.0 || s.pos.y > 30000.0 {
                s.age60 = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pd_core::ids::SHARDTYPE_GLASS;

    /// A 2 × 2 m pane bursts into a grid of shards that fly up, fall, and are
    /// gone after 150 ticks.
    #[test]
    fn a_pane_bursts_into_falling_shards() {
        let mut s = Shards::new(1);
        let mut rng = Rng::new(7);
        s.shards_create(&mut rng, Vec3::new(0.0, 100.0, 0.0), Vec3::X, Vec3::Y, Vec3::Z, -100.0, 100.0, -100.0, 100.0, SHARDTYPE_GLASS, Some(1));
        let live = s.shards.iter().filter(|x| x.age60 > 0).count();
        // 200 shards between the halves: spec = √(40000 / 100) = 20 cm, a 10 × 10 grid.
        assert_eq!(live, 100);
        let y0: f32 = s.shards.iter().filter(|x| x.age60 > 0).map(|x| x.pos.y).sum();
        // Thrown up at 1.3 cm a tick on average, they come down within a second.
        for _ in 0..60 {
            s.shards_tick(1);
        }
        let y1: f32 = s.shards.iter().filter(|x| x.age60 > 0).map(|x| x.pos.y).sum();
        assert!(y1 < y0, "the shards rose");
        for _ in 0..90 {
            s.shards_tick(1);
        }
        assert_eq!(s.shards.iter().filter(|x| x.age60 > 0).count(), 0);
    }
}
