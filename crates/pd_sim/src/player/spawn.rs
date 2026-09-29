//! Where a player (re)enters the match: `player_choose_spawn_location`
//! (`player.c:225`) over the stage's spawn pads, then `player_start_new_life`'s
//! placement (`player.c:527`).

use glam::Vec3;
use pd_core::math::{badrtod4, baddtor};
use pd_core::rng::Rng;

use super::Player;
use crate::chr::chr_adjust_pos_for_spawn;
use crate::stage::{PropFloor, PropGeo, Stage, TileLevel};

/// Another chr as the spawn choice sees it: its `prop->pos` and `prop->rooms`.
#[derive(Clone, Debug)]
pub struct SpawnOther {
    pub pos: Vec3,
    pub rooms: Vec<u16>,
    /// A human (`g_Vars.players[i]`): judged by what its screen shows, not
    /// its rooms.
    pub player: Option<usize>,
}

/// `player_choose_spawn_location(chrradius, ..., pads, numpads)` (`player.c:225`)
/// over `spawnpads` (`g_SpawnPoints`, or Capture the Case's team pads,
/// `ctc_choose_spawn_location`). Each spawn pad is scored by the nearest other chr and
/// marked "very bad" (a chr in the pad's room) or "bad" (in a neighbouring
/// room). A 4-slot shortlist fills in three passes: pads over 10 m from
/// everyone and not bad, walking circularly from a random pad; the same
/// allowing bad; then what is left, furthest first, until the best left is
/// within 2 m. Every candidate must pass `chr_adjust_pos_for_spawn`. One
/// shortlisted spot is picked at random; with none, a random pad.
///
/// Returns the spot (at pad height, not on the floor) and
/// `atan2f(pad.look.x, pad.look.z)`. `cyls` are the other chrs' perimeters.
#[allow(clippy::too_many_arguments)]
pub fn player_choose_spawn_location(level: &TileLevel, stage: &Stage, spawnpads: &[usize], chrradius: f32, others: &[SpawnOther], cyls: &[PropGeo], mp_room_visibility: &[u8], rng: &mut Rng) -> (Vec3, f32) {
    let pads: Vec<Vec3> = spawnpads.iter().map(|&p| stage.pads[p].pos).collect();
    let angles: Vec<f32> = spawnpads.iter().map(|&p| stage.pads[p].look_angle()).collect();
    let numpads = pads.len();
    let mut padsqdists = vec![u32::MAX as f32; numpads];
    let mut verybad = vec![false; numpads];
    let mut bad = vec![false; numpads];
    for p in 0..numpads {
        let padroom = stage.pads[spawnpads[p]].room;
        let neighbours = padroom.map(|r| stage.rooms.bg_room_get_neighbours(r as usize, 20)).unwrap_or_default();
        for o in others {
            let sq = o.pos.distance_squared(pads[p]);
            if sq < padsqdists[p] {
                padsqdists[p] = sq;
            }
            if let Some(pi) = o.player {
                // bg_room_is_on_player_screen / _standby (player.c:275).
                let vis = padroom.and_then(|r| mp_room_visibility.get(r as usize)).copied().unwrap_or(0);
                if vis & (1 << pi) != 0 {
                    verybad[p] = true;
                }
                if verybad[p] || vis & (0x10 << pi) != 0 {
                    bad[p] = true;
                }
                continue;
            }
            // A simulant: in the pad's room, or a neighbour (player.c:290).
            if padroom.is_some_and(|r| o.rooms.contains(&r)) {
                verybad[p] = true;
            }
            if verybad[p] || o.rooms.iter().any(|r| neighbours.contains(r)) {
                bad[p] = true;
            }
        }
    }
    let mut shortlist: Vec<(usize, Vec3)> = Vec::new();
    let adjust = |p: usize| chr_adjust_pos_for_spawn(level, chrradius, pads[p], angles[p], false, cyls);
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
    /// under `pos` among the tiles and the props' `floors`
    /// (`cd_find_ground_at_cyl_ctfril`), eye above it.
    pub fn start_new_life(&mut self, level: &TileLevel, floors: &[PropFloor], pos: Vec3, angle: f32) {
        self.reset_life();
        let angle = baddtor(360.0) - angle;
        let g = level.cd_find_ground_at_cyl_ctfril(pos, 30.0, floors);
        self.place(Vec3::new(pos.x, g.y, pos.z), badrtod4(angle));
        self.floorpoly = g.poly;
        self.floorroom = g.room;
        self.floortype = g.floortype;
        if g.poly.is_some() || g.floor.is_some() {
            self.floorflags = g.flags;
        }
    }
}
