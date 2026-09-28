//! Where a player (re)enters the match: `player_choose_spawn_location`
//! (`player.c:225`) over the stage's spawn pads, then `player_start_new_life`'s
//! placement (`player.c:527`).

use glam::Vec3;
use pd_core::math::{badrtod4, baddtor};
use pd_core::rng::Rng;

use super::Player;
use crate::chr::chr_adjust_pos_for_spawn;
use crate::stage::{PerimCyl, Stage, TileLevel};

/// Another chr as the spawn choice sees it: its `prop->pos` and `prop->rooms`.
#[derive(Clone, Debug)]
pub struct SpawnOther {
    pub pos: Vec3,
    pub rooms: Vec<u16>,
}

/// `player_choose_spawn_location(chrradius, ..., pads = g_SpawnPoints)`
/// (`player.c:225`). Each spawn pad is scored by the nearest other chr and
/// marked "very bad" (a chr in the pad's room) or "bad" (in a neighbouring
/// room). A 4-slot shortlist fills in three passes: pads over 10 m from
/// everyone and not bad, walking circularly from a random pad; the same
/// allowing bad; then what is left, furthest first, until the best left is
/// within 2 m. Every candidate must pass `chr_adjust_pos_for_spawn`. One
/// shortlisted spot is picked at random; with none, a random pad.
///
/// Returns the spot (at pad height, not on the floor) and
/// `atan2f(pad.look.x, pad.look.z)`. `cyls` are the other chrs' perimeters.
pub fn player_choose_spawn_location(level: &TileLevel, stage: &Stage, chrradius: f32, others: &[SpawnOther], cyls: &[PerimCyl], rng: &mut Rng) -> (Vec3, f32) {
    let pads: Vec<Vec3> = stage.spawn_pads.iter().map(|&p| stage.pads[p].pos).collect();
    let angles: Vec<f32> = stage.spawn_pads.iter().map(|&p| stage.pads[p].look_angle()).collect();
    let numpads = pads.len();
    let mut padsqdists = vec![u32::MAX as f32; numpads];
    let mut verybad = vec![false; numpads];
    let mut bad = vec![false; numpads];
    for p in 0..numpads {
        // SUBST: PD reads `pad.room` from the pad file / the room of the floor
        // under the pad (the tiles' rooms stand in for the BSP's until M9).
        let padroom = level.floor_room(pads[p], 20.0);
        let neighbours = padroom.map(|r| level.room_neighbours(r)).unwrap_or_default();
        for o in others {
            let sq = o.pos.distance_squared(pads[p]);
            if sq < padsqdists[p] {
                padsqdists[p] = sq;
            }
            // SUBST: for a human, PD asks whether the pad's room is on that
            // player's screen or standby list (`bg_room_is_on_player_screen`,
            // portals, M9) / every other chr is judged the simulants' way, by
            // its rooms against the pad's room and its neighbours.
            if padroom.is_some_and(|r| o.rooms.contains(&r)) {
                verybad[p] = true;
            }
            if verybad[p] || o.rooms.iter().any(|r| neighbours.contains(r)) {
                bad[p] = true;
            }
        }
    }
    let mut shortlist: Vec<(usize, Vec3)> = Vec::new();
    let adjust = |p: usize| chr_adjust_pos_for_spawn(level, chrradius, pads[p], angles[p], cyls);
    // Passes 1 and 2: circular from a random pad, over 10 m, not bad / not very bad.
    for pass in 0..2 {
        let start = (rng.random() as usize) % numpads;
        let mut p = start;
        while shortlist.len() < 4 {
            let excluded = if pass == 0 { bad[p] } else { verybad[p] };
            if padsqdists[p] > 1000.0 * 1000.0 && !excluded {
                if let Some(spot) = adjust(p) {
                    shortlist.push((p, spot));
                }
                padsqdists[p] = -1.0;
            }
            p = (p + 1) % numpads;
            if p == start {
                break;
            }
        }
    }
    // Pass 3: whatever is left, furthest first, until the best is within 2 m.
    while shortlist.len() < 4 {
        let mut best: Option<(usize, f32)> = None;
        for (p, &d) in padsqdists.iter().enumerate() {
            if d > best.map_or(-1.0, |(_, bd)| bd) {
                best = Some((p, d));
            }
        }
        let Some((p, d)) = best else { break };
        if !(d > 200.0 * 200.0) && !shortlist.is_empty() {
            break;
        }
        if let Some(spot) = adjust(p) {
            shortlist.push((p, spot));
        }
        padsqdists[p] = -1.0;
    }
    if shortlist.is_empty() {
        let p = (rng.random() as usize) % numpads;
        (pads[p], angles[p])
    } else {
        let (p, spot) = shortlist[(rng.random() as usize) % shortlist.len()];
        (spot, angles[p])
    }
}

impl Player {
    /// `player_start_new_life`'s placement (`player.c:527`): face
    /// `BADDTOR(360) − angle`, stand on the ground a 30 cm cylinder finds
    /// under `pos` (`cd_find_ground_at_cyl_ctfril`), eye above it.
    pub fn start_new_life(&mut self, level: &TileLevel, pos: Vec3, angle: f32) {
        let angle = baddtor(360.0) - angle;
        let (groundy, floorpoly) = level.cd_find_ground_at_cyl(pos, 30.0);
        self.place(Vec3::new(pos.x, groundy, pos.z), badrtod4(angle));
        self.floorpoly = floorpoly;
        self.floorroom = floorpoly.and_then(|p| level.geom.polys[p].room);
        if let Some(p) = floorpoly {
            self.floortype = level.geom.polys[p].floortype;
        }
    }
}
