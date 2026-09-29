//! Which rooms the player and its camera are in, and which rooms the camera
//! sees: `bmove_update_rooms` (`bondmove.c:1834`), `player_move_camera_from_pos_rooms`
//! (`player.c:4946`) and `bg_tick` (`bg.c:2123`).

use glam::Vec3;

use super::Player;
use crate::stage::portals::{bg_tick_portals, bg_tick_portals_xray, PortalCam, ROOMFLAG_ONSCREEN, ROOMFLAG_STANDBY};
use crate::stage::{BgRooms, TileLevel};
use crate::world::World;
use pd_core::ids::VISIONMODE_XRAY;

impl Player {
    /// The end of `bwalk_tick` (`bondwalk.c:1803`) and `bmove_update_rooms`:
    /// the player's rooms are the floor's, grown by every room whose portal
    /// the player's box (±50 cm, the feet − 10 to the head + 10) overlaps
    /// (`bmove_find_entered_rooms_by_pos`, `bondmove.c:1811`).
    ///
    /// `// SUBST:` PD carries `prop->rooms` along each move through the portals
    /// it crosses (`los_find_final_room_exhaustive`) and narrows them to the
    /// floor's room when that is among them / the floor's room is taken as the
    /// start: the walk tests every tile, not the rooms'.
    pub fn bmove_update_rooms(&mut self, rooms: &BgRooms, portalflags: &[u8]) {
        let mid = self.pos;
        let bbmin = Vec3::new(mid.x - 50.0, mid.y - self.crouchheight - self.eyeheight - 10.0, mid.z - 50.0);
        let bbmax = Vec3::new(mid.x + 50.0, mid.y - self.crouchheight - self.eyeheight + self.headheight + 10.0, mid.z + 50.0);
        let mut r: Vec<u16> = self.floorroom.into_iter().collect();
        if r.is_empty() {
            // Off every floor (falling, a ladder): keep what it had.
            r = self.rooms.iter().copied().take(1).collect();
        }
        if !r.is_empty() {
            rooms.bg_find_entered_rooms(bbmin, bbmax, &mut r, 7, false, portalflags);
        }
        self.rooms = r;
    }

    /// `player_move_camera_from_pos_rooms` (`player.c:4946`): the room the
    /// camera at `pos` is in, looked for from a known position and its rooms
    /// through the portals between, else by the rooms' boxes and the floor under
    /// it. A room found inside the level becomes the memory (`memcampos`,
    /// `memcamroom`).
    pub fn player_move_camera_from_pos_rooms(&mut self, pos: Vec3, prevgood: Option<(Vec3, &[u16])>, rooms: &BgRooms, level: &TileLevel) {
        if let Some((prevgoodpos, prevgoodrooms)) = prevgood.filter(|(_, r)| !r.is_empty()) {
            let (mut found, _) = rooms.portal_find_rooms(prevgoodpos, pos, prevgoodrooms);
            found.retain(|&r| rooms.bg_room_contains_coord(pos, r as usize));
            // In most cases there is one room containing the given pos.
            if found.len() == 1 {
                return self.player_set_cam_properties_in_bounds(pos, found[0] as usize);
            }
            for complicated in [false, true] {
                if let Some(&r) = found.iter().find(|&&r| rooms.rooms[r as usize].complicated == complicated && rooms.bg_test_pos_in_room(pos, r as usize)) {
                    return self.player_set_cam_properties_in_bounds(pos, r as usize);
                }
            }
        }
        let (inrooms, aboverooms, bestroom) = rooms.bg_find_rooms_by_pos(pos, 20);
        if let Some(&first) = inrooms.first() {
            let room = level.cd_find_room_at_pos(pos, &inrooms).filter(|&r| r > 0).unwrap_or(first);
            self.player_set_cam_properties_in_bounds(pos, room as usize);
        } else if let Some(&first) = aboverooms.first() {
            let room = level.cd_find_room_at_pos(pos, &aboverooms).filter(|&r| r > 0).unwrap_or(first);
            self.player_set_cam_properties_out_of_bounds(room as usize);
        } else {
            self.player_set_cam_properties_out_of_bounds(bestroom.map_or(1, |r| r as usize));
        }
    }

    /// `player_set_cam_properties_in_bounds` (`player.c:5063`).
    fn player_set_cam_properties_in_bounds(&mut self, pos: Vec3, room: usize) {
        self.memcampos = pos;
        self.memcamroom = Some(room as u16);
        self.cam_room = room;
    }

    /// `player_set_cam_properties_out_of_bounds` (`player.c:5073`).
    fn player_set_cam_properties_out_of_bounds(&mut self, room: usize) {
        self.memcamroom = None;
        self.cam_room = room;
    }

    /// The portal code's view of this player's camera.
    pub fn portal_cam(&self, zfar: f32) -> PortalCam {
        let c = &self.cam;
        PortalCam {
            world_to_screen: c.world_to_screen,
            cam_pos: c.pos(),
            c_screenleft: c.c_screenleft,
            c_screentop: c.c_screentop,
            c_halfwidth: c.c_halfwidth,
            c_halfheight: c.c_halfheight,
            c_recipscalex: 1.0 / c.c_scalex,
            c_recipscaley: 1.0 / c.c_scaley,
            // vi_get_width/height: one player's framebuffer is its view.
            view: PortalCam::screen_properties(c.c_screenleft, c.c_screentop, c.c_screenwidth, c.c_screenheight, super::camera::SCREEN_W, super::camera::SCREEN_H),
            zfar,
        }
    }
}

impl World {
    /// After player `pi`'s `bmove_tick`: its rooms, then its camera's room from
    /// its position and rooms (`player.c:3669`, normal movement).
    pub(crate) fn player_update_rooms(&mut self, pi: usize) {
        let p = &mut self.players[pi];
        p.bmove_update_rooms(&self.stage.rooms, &self.portalflags);
        let pos = p.pos;
        let rooms = p.rooms.clone();
        p.player_move_camera_from_pos_rooms(pos, Some((pos, &rooms)), &self.stage.rooms, &self.level);
    }

    /// `bg_tick` (`bg.c:2123`) for player `pi`'s pass: the rooms its camera sees
    /// (`bg_tick_portals`, or in x-ray those near the eraser), and
    /// `g_MpRoomVisibility`'s bits for the player (`bg.c:5680`).
    pub(crate) fn bg_tick(&mut self, pi: usize) {
        let rooms = &self.stage.rooms;
        let p = &self.players[pi];
        let cam = p.portal_cam(rooms.zrange.1);
        let view = if p.visionmode == VISIONMODE_XRAY {
            bg_tick_portals_xray(rooms, &self.portalflags, &cam, p.cam_room, p.eraser.pos, p.eraser.bgdist)
        } else {
            bg_tick_portals(rooms, &self.portalflags, &cam, p.cam_room)
        };
        let flag1 = 0x01u8 << pi;
        let flag2 = 0x10u8 << pi;
        for (r, f) in view.roomflags.iter().enumerate() {
            let v = &mut self.mp_room_visibility[r];
            if f & ROOMFLAG_ONSCREEN != 0 {
                *v |= flag1;
            } else {
                *v &= !flag1;
            }
            if f & ROOMFLAG_STANDBY != 0 {
                *v |= flag2;
            } else {
                *v &= !flag2;
            }
        }
        self.roomflags.clone_from(&view.roomflags);
        self.players[pi].portalview = view;
    }

    /// `bg_room_is_onscreen` (`bg.c:2509`): on any player's screen.
    pub fn bg_room_is_onscreen(&self, room: usize) -> bool {
        self.mp_room_visibility.get(room).is_some_and(|v| v & 0x0f != 0)
    }

    /// `bg_room_is_standby` (`bg.c:2518`).
    pub fn bg_room_is_standby(&self, room: usize) -> bool {
        self.mp_room_visibility.get(room).is_some_and(|v| v & 0xf0 != 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::player::PlayerInput;
    use crate::testutil;
    use crate::world::World;
    use pd_core::mp::{MatchPlayer, MatchSetup};

    /// A player walking and turning around Complex: the camera's room holds
    /// the camera (in its box while in bounds), is always on screen, and the
    /// walk passes through several rooms.
    #[test]
    fn the_camera_room_follows_the_player() {
        let (stage, level) = testutil::complex_arc();
        let setup = MatchSetup { stagenum: pd_core::ids::STAGE_MP_COMPLEX, players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
        let mut w = World::new(setup, stage, level, testutil::res(), 7).unwrap();
        let mut rooms_seen = std::collections::BTreeSet::new();
        for f in 0..1200 {
            let input = PlayerInput { walk_y: 127, mouse_dx: if f % 200 < 40 { 6.0 } else { 0.0 }, ..Default::default() };
            w.step(4, std::slice::from_ref(&input));
            let p = &w.players[0];
            let r = p.cam_room;
            rooms_seen.insert(r);
            assert!(p.portalview.is_onscreen(r), "frame {f}: room {r} not on screen");
            assert!(w.bg_room_is_onscreen(r));
            if p.memcamroom.is_some() {
                assert!(w.stage.rooms.bg_room_contains_coord(p.cam.pos(), r), "frame {f}: camera {} outside room {r}", p.cam.pos());
            }
            assert!(!p.rooms.is_empty() || p.floorroom.is_none());
        }
        assert!(rooms_seen.len() >= 2, "walked through {rooms_seen:?}");
    }

    /// On every arena, a camera at head height over each waypoint's pad finds a
    /// room whose box holds it and which is on the inside of its portals, from
    /// nothing (`bg_find_rooms_by_pos` and the floor) and again from its own
    /// room (the portal route).
    #[test]
    fn every_waypoint_is_in_a_room_on_every_arena() {
        let a = testutil::assets();
        let mut rng = pd_core::rng::Rng::new(1);
        let mut p = crate::player::Player::new(&testutil::res(), glam::Vec3::ZERO, 0.0, 1, &mut rng).unwrap();
        for code in crate::stage::ARENAS {
            let stage = crate::stage::Stage::load(&a, code).unwrap();
            let level = crate::stage::TileLevel::for_stage(&stage);
            let rooms = &stage.rooms;
            let mut inside = 0;
            for w in &stage.waypoints {
                let pos = stage.pads[w.padnum].pos + glam::Vec3::Y * 60.0;
                p.player_move_camera_from_pos_rooms(pos, None, rooms, &level);
                let r = p.cam_room;
                assert!(r >= 1 && r < rooms.roomcount(), "{code}: room {r}");
                if p.memcamroom.is_some() {
                    assert!(rooms.bg_room_contains_coord(pos, r), "{code}: {pos} outside room {r}");
                    inside += 1;
                    let from = [r as u16];
                    p.player_move_camera_from_pos_rooms(pos, Some((pos, &from)), rooms, &level);
                    assert_eq!(p.cam_room, r, "{code}: {pos}");
                }
            }
            assert!(inside * 10 >= stage.waypoints.len() * 9, "{code}: {inside} of {} waypoints in bounds", stage.waypoints.len());
        }
    }
}
