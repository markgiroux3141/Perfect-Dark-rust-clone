//! Chrs, player and simulant alike: the `chrdata` subset, movement
//! (`chr_calculate_push_pos`, the ground half of `chr_update_position`, falls,
//! ladders, duck/squat), go-to (`chr_go_to_room_pos`, `chr_tick_gopos`), damage and
//! hit parts (`chr_damage`), death, spawn (`player_choose_spawn_location`,
//! `chr_adjust_pos_for_spawn`), footsteps, and the body pose, computed here with
//! `pd_core::model` so gun positions come from the sim, not from the renderer.
//!
//! M3 has what the player needs: [`chr_adjust_pos_for_spawn`] and
//! [`footstep_choose_sound`]. The simulants' half arrives with M6.
//!
//! Sources: `pd_spike/chr.rs`, `chraction.rs`, `thirdperson.rs`, `gunpos.rs`.

use glam::Vec3;
use pd_core::ids::FLOORTYPE_SNOW;
use pd_core::math::baddtor;
use pd_core::rng::Rng;

use crate::stage::{CdResult, PerimCyl, TileLevel};

/// `chr_adjust_pos_for_spawn(chrradius, pos, rooms, angle, allowonscreen = true,
/// force = false, onlysurrounding = false)` (`chraction.c:15018`): `pos` if a
/// cylinder there, reaching 200 cm up and down to the ground, touches nothing;
/// else the first of 8 points 60 cm around it (from `angle`, 45° apart) the pad
/// can see and that is clear the same way. `pos` is the pad's own position, not
/// the ground; the caller finds the floor under the result.
///
/// Every caller in the Combat Simulator passes `allowonscreen = true`, so
/// `chr_is_pos_offscreen` is never asked.
pub fn chr_adjust_pos_for_spawn(level: &TileLevel, chrradius: f32, pos: Vec3, angle: f32, cyls: &[PerimCyl]) -> Option<Vec3> {
    let ymax = 200.0;
    let ymin_at = |p: Vec3| {
        let ground = level.cd_find_ground_at_cyl(p, chrradius).0;
        if ground > -100_000.0 && ground - p.y < -200.0 {
            ground - p.y
        } else {
            -200.0
        }
    };
    if level.cd_test_volume_simple(pos, chrradius, ymax, ymin_at(pos), cyls) != CdResult::Collision {
        return Some(pos);
    }
    let mut curangle = angle;
    for _ in 0..8 {
        let testpos = Vec3::new(pos.x + curangle.sin() * 60.0, pos.y, pos.z + curangle.cos() * 60.0);
        // `cd_test_los_oobok_getfinalroom_autoflags(..., CDTYPE_BG)`: the BG only.
        if level.los_autoflags(pos, testpos) && level.cd_test_volume_simple(testpos, chrradius, ymax, ymin_at(testpos), cyls) != CdResult::Collision {
            return Some(testpos);
        }
        curangle += baddtor(45.0);
        if curangle >= baddtor(360.0) {
            curangle -= baddtor(360.0);
        }
    }
    None
}

/// `g_FootstepSounds` (`footstep.c:14`): per `FLOORTYPE_*`, walking (0, 1, 4, 5)
/// and running (2, 3, 6, 7) samples; the "none" row is PD's −1, 0 here.
pub const FOOTSTEP_SOUNDS: [u16; 72] = [
    0, 0, 0, 0, 0, 0, 0, 0, //
    0x80dc, 0x80dd, 0x80e0, 0x80e1, 0x80de, 0x80df, 0x80e2, 0x80e3, //
    0x80c4, 0x80c5, 0x80c8, 0x80c9, 0x80c6, 0x80c7, 0x80ca, 0x80cb, //
    0x80e6, 0x80e7, 0x80ea, 0x80eb, 0x80e8, 0x80e9, 0x80ec, 0x80ed, //
    0x80d4, 0x80d5, 0x80d8, 0x80d9, 0x80d6, 0x80d7, 0x80da, 0x80db, //
    0x80ee, 0x80ef, 0x80f2, 0x80f3, 0x80f0, 0x80f1, 0x80f4, 0x80f5, //
    0x80e4, 0x80e5, 0x80e4, 0x80e5, 0x80e4, 0x80e5, 0x80e4, 0x80e5, //
    0x80cc, 0x80cd, 0x80d0, 0x80d1, 0x80ce, 0x80cf, 0x80d2, 0x80d3, //
    0x8187, 0x8188, 0x818b, 0x818c, 0x8189, 0x818a, 0x818d, 0x818e,
];

/// `footstep_choose_sound` (`footstep.c:100`), non-Skedar: a random sample of
/// the floor's walking or running set, never the same one twice in a row.
/// `running` is `footstep_is_running(g_FootstepAnims[footstepindex].animnum)`;
/// the player passes `footstepindex = distance > 10` (`bondmove.c:1966`), and
/// entries 0 and 1 are ANIM_002B (walk) and ANIM_0029 (run), so it is exactly
/// "moved more than 10 cm". Returns 0 for none (PD's −1 row and its `footstep == 0`).
pub fn footstep_choose_sound(rng: &mut Rng, floortype: u8, lastfootsample: &mut i32, running: bool) -> u16 {
    let floortype = if floortype <= FLOORTYPE_SNOW { floortype as i32 } else { 0 };
    let mut index;
    loop {
        let rand = (rng.random() % 8) as i32;
        index = (if running { 2 } else { 0 }) + (rand & 5) + floortype * 8;
        if index != *lastfootsample {
            break;
        }
    }
    *lastfootsample = index;
    FOOTSTEP_SOUNDS[index as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::{fixtures, TileLevel};

    #[test]
    fn a_blocked_spawn_moves_60cm_to_a_clear_spot() {
        let l = TileLevel::new(fixtures::arena());
        let c = fixtures::ARENA_PILLARS[0];
        // Clear floor: the pad itself.
        let open = Vec3::new(0.0, 60.0, 0.0);
        assert_eq!(chr_adjust_pos_for_spawn(&l, 30.0, open, 0.0, &[]), Some(open));
        // Touching the pillar's west face: moved 60 cm, away from it.
        let near = Vec3::new(c.x - fixtures::ARENA_PILLAR_HALF - 20.0, 60.0, c.y);
        let got = chr_adjust_pos_for_spawn(&l, 30.0, near, 0.0, &[]).unwrap();
        assert!((got.distance(near) - 60.0).abs() < 1e-3, "{got}");
        assert_eq!(l.cd_test_volume_simple(got, 30.0, 200.0, -60.0, &[]), CdResult::NoCollision);
    }

    #[test]
    fn footsteps_never_repeat_a_sample_and_the_default_floor_is_silent() {
        let mut rng = Rng::new(1);
        let mut last = -1;
        let metal = &FOOTSTEP_SOUNDS[4 * 8..5 * 8];
        let mut prev = 0;
        for i in 0..50 {
            let s = footstep_choose_sound(&mut rng, pd_core::ids::FLOORTYPE_METAL, &mut last, i % 2 == 0);
            assert!(metal.contains(&s));
            assert_ne!(s, prev);
            prev = s;
        }
        assert_eq!(footstep_choose_sound(&mut rng, pd_core::ids::FLOORTYPE_DEFAULT, &mut last, false), 0);
    }
}
