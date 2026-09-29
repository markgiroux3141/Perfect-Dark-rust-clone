//! Room lighting (`dlights.c`): each room's settled brightness, the light it
//! passes to its neighbours through the portals, and the flashes that
//! explosions, gunfire and sparks add on top.
//!
//! - At load, `lights_reset_1` gives every room its base (`room_init_lights`:
//!   the BG's brightness range, a quarter of the minimum, plus 4/5 of the range
//!   when the room has no lights) and `lights_reset_2` builds
//!   `g_LightTransferLookup`: how much of each room's light reaches every other
//!   room, by the rooms' volumes and surface areas and the portals' areas
//!   (`l2_build_transfer_table`).
//! - Every frame `lighting_tick` (`lv_tick`) recomputes a dirty room's local
//!   brightness from its lights, decays the flashes, and for rooms on screen or
//!   on standby sums their neighbours' local brightness through the transfer
//!   table into `br_settled_regional`.
//! - `room_flash_lighting` spreads a flash over the rooms the source room
//!   reaches, each only while it is on screen.
//!
//! What a room's brightness does: `room_highlight` rescales its vertex colours
//! (drawn by `pd_render::bg`), and `lights_set_for_room` lights the props in it.

use glam::Vec3;
use pd_core::rng::Rng;

use crate::stage::portals::{ROOMFLAG_ONSCREEN, ROOMFLAG_STANDBY};
use crate::stage::BgRooms;

/// `LIGHTOP_*` (`constants.h:1519`).
pub const LIGHTOP_NONE: u8 = 0;
pub const LIGHTOP_SET: u8 = 1;
pub const LIGHTOP_SETRANDOM: u8 = 2;
pub const LIGHTOP_TRANSITION: u8 = 3;
pub const LIGHTOP_SINELOOP: u8 = 4;
pub const LIGHTOP_HIGHLIGHT: u8 = 5;

/// The `ROOMFLAG_*` bits the lighting keeps (`constants.h:3612`).
pub const ROOMFLAG_BRIGHTNESS_CALCED: u16 = 0x0040;
pub const ROOMFLAG_RENDERALWAYS: u16 = 0x0080;
pub const ROOMFLAG_LIGHTS_DIRTY: u16 = 0x0100;
pub const ROOMFLAG_BRIGHTNESS_DIRTY_PERM: u16 = 0x0200;
pub const ROOMFLAG_BRIGHTNESS_DIRTY_TEMP: u16 = 0x0400;
pub const ROOMFLAG_NEEDRESHADE: u16 = 0x1000;
pub const ROOMFLAG_LIGHTSOFF: u16 = 0x2000;
pub const ROOMFLAG_OUTDOORS: u16 = 0x8000;

/// `struct room`'s lighting half.
#[derive(Clone, Debug, Default)]
pub struct RoomLighting {
    pub flags: u16,
    pub br_light_min: u8,
    pub br_light_max: u8,
    pub br_light_each: u8,
    pub br_settled_regional: u8,
    pub br_base: u8,
    pub lightop: u8,
    pub br_settled_local: i16,
    pub br_flash: i16,
    pub lightop_timer240: i16,
    pub lightop_cur_frac: f32,
    pub lightop_to_frac: f32,
    pub lightop_from_frac: f32,
    pub lightop_duration240: f32,
    /// In metres³ (+1, at most 60) and cm² (`lights_calculate_room_dimensions`).
    pub volume: f32,
    pub surfacearea: f32,
    pub highlightfrac: [f32; 3],
}

/// A light's state (`struct light`'s run-time bits).
#[derive(Clone, Copy, Debug, Default)]
pub struct LightState {
    pub brightness: u8,
    pub sparkable: bool,
    pub healthy: bool,
    pub on: bool,
    pub sparking: bool,
    pub vulnerable: bool,
}

/// Every room's lighting and every light's state.
#[derive(Clone, Debug, Default)]
pub struct Lights {
    /// By room number (room 0 is PD's dummy).
    pub rooms: Vec<RoomLighting>,
    /// The lights file's entries, in file order (a room's are
    /// `lightindex..lightindex + numlights`).
    pub lights: Vec<LightState>,
    /// `g_LightTransferLookup`: `transfer[i][j]`, how much of room `i`'s light
    /// reaches room `j` (0-255). A row is the `horizontal` list, a column the
    /// `vertical` one; PD stores both run-length packed (`func0f177a54`), which
    /// with fewer than 255 rooms loses nothing. `None`: no transfer table (PD
    /// ticks no lighting then).
    transfer: Option<Vec<Vec<u8>>>,
    /// Each room's lights as (first, count) in `lights`.
    ranges: Vec<(usize, usize)>,
}

impl Lights {
    /// `lights_reset_1` + `lights_reset_2` (`dlights.c:957`, `:573`) for a
    /// stage: every portal open (`bg_build_tables` has just cleared them).
    pub fn new(rooms: &BgRooms) -> Lights {
        let n = rooms.roomcount();
        let ranges = rooms.rooms.iter().map(|r| (r.lightindex.unwrap_or(0), r.numlights)).collect();
        let mut l = Lights { rooms: vec![RoomLighting::default(); n], lights: vec![LightState::default(); rooms.lights.len()], transfer: None, ranges };
        for i in 1..n {
            room_set_defaults(&mut l.rooms[i]);
            l.room_init_lights(rooms, i);
        }
        l.lights_calculate_room_dimensions(rooms);
        // SUBST: with no portals PD builds no transfer table, and a stage
        // without one has no lighting at all (`lights_reset_2`, `:604`) / a test
        // fixture has no portals, so it gets its rooms' own transfer (each
        // room lights itself), which keeps the flashes working there.
        l.transfer = Some(l.l2_build_transfer_table(rooms));
        let mut flags = vec![0u16; n];
        for f in flags.iter_mut().skip(1) {
            *f |= ROOMFLAG_ONSCREEN;
        }
        let mut rng = Rng::new(0);
        l.lighting_tick(&flags, 0, &mut rng, &[]);
        l
    }

    /// `room_init_lights` (`dlights.c:348`).
    fn room_init_lights(&mut self, rooms: &BgRooms, roomnum: usize) {
        let b = &rooms.rooms[roomnum];
        let room = &mut self.rooms[roomnum];
        room.br_light_min = b.br_light_min / 4;
        room.br_light_max = b.br_light_max;
        room.br_light_each = if b.numlights != 0 { ((room.br_light_max - room.br_light_min) as f32 / b.numlights as f32) as u8 } else { 0 };
        room.br_base = room.br_light_min;
        if b.numlights == 0 {
            room.br_base = room.br_base.wrapping_add(((room.br_light_max as i32 - room.br_light_min as i32) * 4 / 5) as u8);
        }
        // ROOMFLAG_RENDERALWAYS is for solo stages' sky rooms.
        room.flags &= !ROOMFLAG_RENDERALWAYS;
        room.flags |= ROOMFLAG_LIGHTS_DIRTY;
        if let Some(first) = b.lightindex {
            let each = room.br_light_each;
            for light in &mut self.lights[first..first + b.numlights] {
                *light = LightState { brightness: each, sparkable: true, healthy: true, on: true, sparking: false, vulnerable: true };
            }
        }
    }

    /// `lights_calculate_room_dimensions` (`dlights.c:802`).
    fn lights_calculate_room_dimensions(&mut self, rooms: &BgRooms) {
        for (i, room) in self.rooms.iter_mut().enumerate() {
            let b = &rooms.rooms[i];
            let mut valid = true;
            room.volume = 1.0;
            room.surfacearea = 1.0;
            for j in 0..3 {
                let diff = b.bbmax[j] - b.bbmin[j];
                if diff > 0.0 {
                    room.volume *= (b.bbmax[j] - b.bbmin[j]) / 100.0;
                } else {
                    valid = false;
                }
            }
            room.volume += 1.0;
            if room.volume > 60.0 {
                room.volume = 60.0;
            }
            room.surfacearea = if valid {
                let d = (b.bbmax - b.bbmin).abs();
                2.0 * (d.x * d.y + d.x * d.z + d.y * d.z)
            } else {
                20000000.0
            };
        }
    }

    /// `l2_build_transfer_table` (`dlights.c:749`).
    fn l2_build_transfer_table(&mut self, rooms: &BgRooms) -> Vec<Vec<u8>> {
        let n = rooms.roomcount();
        let area: Vec<f32> = (0..rooms.portals.len()).map(|p| bg_calculate_portal_surface_area(&rooms.portals[p].verts)).collect();
        let mut table = vec![vec![0u8; n]; n];
        for i in 1..n {
            let mut tmp = self.l2_build_transfer_for_room(rooms, &area, i);
            for j in 1..n {
                if tmp[i] < tmp[j] {
                    tmp[j] = tmp[i];
                }
                table[i][j] = tmp[j] as u8;
            }
        }
        table
    }

    /// `l2_build_transfer_for_room` (`dlights.c:861`): room `roomnum`'s light
    /// (√volume × 255) passed on through open portals until the amounts fall
    /// to 50, then each room's share scaled by 3 / √its volume.
    fn l2_build_transfer_for_room(&mut self, rooms: &BgRooms, area: &[f32], roomnum: usize) -> Vec<f32> {
        let n = rooms.roomcount();
        let mut tmp = vec![0.0f32; n];
        tmp[roomnum] = self.rooms[roomnum].volume.sqrt() * 255.0;
        if !rooms.rooms[roomnum].portals.is_empty() {
            let start = tmp[roomnum];
            self.l2_build_transfer_through_portal(rooms, area, &mut tmp, roomnum, start, 0, None);
        }
        let portalsurfacearea: f32 = rooms.rooms[roomnum].portals.iter().map(|&p| area[p]).sum();
        let r = &self.rooms[roomnum];
        let wallsurfaceareafrac = (r.surfacearea - portalsurfacearea) / r.surfacearea;
        for i in 1..n {
            tmp[i] *= 3.0 / self.rooms[i].volume.sqrt();
        }
        if tmp[roomnum] > 255.0 {
            tmp[roomnum] = 255.0;
        }
        if wallsurfaceareafrac < 0.1 {
            let r = &mut self.rooms[roomnum];
            r.br_light_min = r.br_light_max >> 1;
            tmp[roomnum] = r.br_light_max as f32;
        }
        tmp
    }

    /// `l2_build_transfer_through_portal` (`dlights.c:913`), with
    /// `l2_calculate_room_transfer` (`:789`) as the handler.
    #[allow(clippy::too_many_arguments)]
    fn l2_build_transfer_through_portal(&self, rooms: &BgRooms, area: &[f32], tmp: &mut [f32], roomnum: usize, mult: f32, recursioncount: i32, portalnum: Option<usize>) {
        let other = |p: usize, r: usize| if r == rooms.portals[p].room1 as usize { rooms.portals[p].room2 as usize } else { rooms.portals[p].room1 as usize };
        let otherroomnum = portalnum.map(|p| other(p, roomnum));
        for &iterportalnum in &rooms.rooms[roomnum].portals {
            // g_PortalsOpenTmp: every portal is open when the table is built.
            let iterroomnum = other(iterportalnum, roomnum);
            if Some(iterroomnum) != otherroomnum {
                let portal1surfacearea = portalnum.map_or(0.0, |p| area[p]);
                let amount = (area[iterportalnum] / (self.rooms[roomnum].surfacearea - portal1surfacearea)) * mult;
                if amount > 50.0 && recursioncount < 20 {
                    tmp[roomnum] -= amount;
                    tmp[iterroomnum] += amount;
                    if tmp[roomnum] < 0.0 {
                        tmp[roomnum] = 0.0;
                    }
                    self.l2_build_transfer_through_portal(rooms, area, tmp, iterroomnum, amount, recursioncount + 1, Some(iterportalnum));
                }
            }
        }
    }

    /// The rooms (by index, ascending) that room `i`'s light reaches, with the
    /// amount: the `vertical` list (`func0f177c8c` over it).
    fn vertical(&self, i: usize) -> impl Iterator<Item = (usize, u8)> + '_ {
        self.transfer.iter().flat_map(move |t| t.iter().enumerate().filter_map(move |(j, row)| (row[i] != 0).then_some((j, row[i]))))
    }

    /// The rooms that light room `i`: the `horizontal` list.
    fn horizontal(&self, i: usize) -> impl Iterator<Item = (usize, u8)> + '_ {
        self.transfer.iter().flat_map(move |t| t[i].iter().enumerate().filter_map(|(j, &v)| (v != 0).then_some((j, v))))
    }

    /// `lighting_tick` / `rooms_tick_lighting` (`dlights.c:1166`, `:1248`),
    /// with `roomflags` the latest portal tick's ONSCREEN/STANDBY bits and
    /// `highlight` the scenario's tints of its `LIGHTOP_HIGHLIGHT` rooms
    /// (`scenario_highlight_room`: King of the Hill's hill, Capture the Case's
    /// bases).
    pub fn lighting_tick(&mut self, roomflags: &[u16], lvupdate240: i32, rng: &mut Rng, highlight: &[(u16, [f32; 3])]) {
        if self.transfer.is_none() {
            return;
        }
        let n = self.rooms.len();
        let mut wasdirty = false;
        for r in self.rooms.iter_mut().skip(1) {
            r.flags &= !ROOMFLAG_BRIGHTNESS_DIRTY_TEMP;
        }
        let light_ranges = self.light_ranges();
        for i in 1..n {
            {
                let r = &mut self.rooms[i];
                r.lightop_timer240 = r.lightop_timer240.wrapping_sub(lvupdate240 as i16);
                match r.lightop {
                    LIGHTOP_SET => {
                        r.lightop_cur_frac = r.lightop_to_frac.max(0.0);
                        r.flags |= ROOMFLAG_LIGHTS_DIRTY;
                        r.lightop = LIGHTOP_NONE;
                    }
                    LIGHTOP_SETRANDOM => {
                        if r.lightop_timer240 < 0 {
                            if rng.randomfrac() * 100.0 < r.lightop_to_frac {
                                r.lightop_cur_frac = 1.0;
                            } else {
                                r.lightop_cur_frac = r.lightop_from_frac.max(0.0);
                            }
                            r.lightop_timer240 = r.lightop_duration240 as i16;
                            r.flags |= ROOMFLAG_LIGHTS_DIRTY;
                        }
                    }
                    LIGHTOP_TRANSITION => {
                        if r.lightop_timer240 > 0 {
                            r.lightop_cur_frac = (r.lightop_from_frac + r.lightop_timer240 as f32 / r.lightop_duration240 * (r.lightop_to_frac - r.lightop_from_frac)).max(0.0);
                        } else {
                            r.lightop = LIGHTOP_NONE;
                        }
                        r.flags |= ROOMFLAG_LIGHTS_DIRTY;
                    }
                    LIGHTOP_SINELOOP => {
                        let timer240 = (r.lightop_timer240 as i32).abs();
                        let angle = (timer240 % r.lightop_duration240 as i32) as f32 * pd_core::math::dtor(360.0) / r.lightop_duration240;
                        let average = (r.lightop_to_frac + r.lightop_from_frac) * 0.5;
                        r.lightop_cur_frac = (r.lightop_to_frac + (angle.cos() + 1.0) * average).max(0.0);
                        r.flags |= ROOMFLAG_LIGHTS_DIRTY;
                    }
                    LIGHTOP_HIGHLIGHT => r.flags |= ROOMFLAG_LIGHTS_DIRTY,
                    _ => {}
                }
            }
            if self.rooms[i].flags & ROOMFLAG_LIGHTS_DIRTY != 0 {
                let (first, count) = light_ranges[i];
                let lights = &self.lights[first..first + count];
                let r = &mut self.rooms[i];
                let mut amount = if r.flags & ROOMFLAG_LIGHTSOFF != 0 {
                    2.0
                } else if count != 0 && !lights.iter().any(|l| l.on) {
                    r.br_base as f32 / 2.0
                } else {
                    r.br_base as f32
                };
                amount *= r.lightop_cur_frac;
                r.br_settled_local = amount as i16;
                for l in lights.iter().filter(|l| l.on) {
                    r.br_settled_local += (r.lightop_cur_frac * l.brightness as f32) as i32 as i16;
                }
                if r.br_settled_local > 255 {
                    r.br_settled_local = 255;
                }
                r.flags &= !ROOMFLAG_LIGHTS_DIRTY;
                wasdirty = true;
            }
            if self.rooms[i].br_flash != 0 {
                let reached: Vec<usize> = self.vertical(i).map(|(j, _)| j).collect();
                for j in reached {
                    self.rooms[j].flags |= ROOMFLAG_BRIGHTNESS_DIRTY_TEMP;
                }
                let r = &mut self.rooms[i];
                let mut increment = (lvupdate240 * 2) as i16;
                if r.br_flash > 0 {
                    if increment > r.br_flash {
                        increment = r.br_flash;
                    }
                    r.br_flash -= increment;
                } else {
                    // PD's @bug, kept: br_flash is <= 0 here.
                    if increment < r.br_flash {
                        increment = r.br_flash;
                    }
                    r.br_flash += increment;
                }
                r.flags |= ROOMFLAG_BRIGHTNESS_DIRTY_TEMP;
            }
            self.rooms[i].flags &= !ROOMFLAG_LIGHTS_DIRTY;
        }
        if wasdirty {
            for r in self.rooms.iter_mut().skip(1) {
                r.flags |= ROOMFLAG_BRIGHTNESS_DIRTY_PERM;
            }
        }
        for i in 1..n {
            let visible = roomflags.get(i).is_some_and(|f| f & (ROOMFLAG_ONSCREEN | ROOMFLAG_STANDBY) != 0);
            let r = &self.rooms[i];
            if (r.flags & ROOMFLAG_RENDERALWAYS != 0 || visible) && r.flags & (ROOMFLAG_BRIGHTNESS_DIRTY_PERM | ROOMFLAG_BRIGHTNESS_DIRTY_TEMP) != 0 {
                let mut sum: i32 = 0;
                for (j, ret) in self.horizontal(i) {
                    if j != 0 {
                        sum += ((1.0f32 / 255.0) * ret as f32 * self.rooms[j].br_settled_local as f32) as i32;
                    }
                }
                let r = &mut self.rooms[i];
                r.br_settled_regional = sum.min(255) as u8;
                r.flags |= ROOMFLAG_BRIGHTNESS_CALCED | ROOMFLAG_NEEDRESHADE;
                r.flags &= !(ROOMFLAG_BRIGHTNESS_DIRTY_PERM | ROOMFLAG_BRIGHTNESS_DIRTY_TEMP);
                let b = self.room_get_final_brightness_for_player(i) as i32;
                let tint = if self.rooms[i].lightop == LIGHTOP_HIGHLIGHT { highlight.iter().find(|h| h.0 as usize == i).map(|h| h.1) } else { None };
                self.rooms[i].highlightfrac = match tint {
                    Some(t) => t.map(|t| ((b as f32 * t) as i32) as f32 * (1.0 / 255.0)),
                    None => [b as f32 * (1.0 / 255.0); 3],
                };
            }
        }
    }

    /// Each room's lights as (first, count) in [`Self::lights`].
    fn light_ranges(&self) -> Vec<(usize, usize)> {
        self.ranges.clone()
    }

    /// `room_flash_lighting` (`dlights.c:1567`): a flash of `start` in every
    /// room `roomnum`'s light reaches, scaled by how much reaches it, up to
    /// `limit`; only rooms on screen take it (`room_flash_local_lighting`).
    pub fn room_flash_lighting(&mut self, roomflags: &[u16], roomnum: usize, start: i32, limit: i32) {
        if roomnum >= self.rooms.len() || self.transfer.is_none() || self.rooms[roomnum].flags & ROOMFLAG_OUTDOORS != 0 {
            return;
        }
        let reached: Vec<(usize, u8)> = self.vertical(roomnum).collect();
        for (neighbournum, value) in reached {
            let mut increment = value as f32 * (1.0 / 255.0) * start as f32 * 5.0;
            if start > 0 {
                if increment > start as f32 {
                    increment = start as f32;
                }
            } else if increment < start as f32 {
                increment = start as f32;
            }
            // PD's @bug, kept: it tests roomnum's flags, not the neighbour's.
            if self.rooms[roomnum].flags & ROOMFLAG_OUTDOORS == 0 {
                self.room_flash_local_lighting(roomflags, neighbournum, increment as i32, limit);
            }
        }
    }

    /// `room_flash_local_lighting` (`dlights.c:1599`).
    pub fn room_flash_local_lighting(&mut self, roomflags: &[u16], roomnum: usize, increment: i32, limit: i32) {
        if roomnum == 0 || roomflags.get(roomnum).is_none_or(|f| f & ROOMFLAG_ONSCREEN == 0) {
            return;
        }
        let r = &mut self.rooms[roomnum];
        let flash = r.br_flash as i32;
        if increment > 0 {
            if flash < limit {
                r.br_flash = (flash + increment).min(limit) as i16;
            }
        } else if flash > limit {
            r.br_flash = (flash + increment).max(limit) as i16;
        }
    }

    /// `room_get_final_brightness_for_player` (`dlights.c:106`): the settled
    /// light plus the flash, 0-255.
    pub fn room_get_final_brightness_for_player(&self, roomnum: usize) -> u8 {
        let r = &self.rooms[roomnum];
        (r.br_flash as i32 + r.br_settled_regional as i32).clamp(0, 255) as u8
    }

    /// `room_get_settled_regional_brightness_for_player` (`dlights.c:132`).
    pub fn room_get_settled_regional_brightness_for_player(&self, roomnum: usize) -> u8 {
        let r = &self.rooms[roomnum];
        if r.flags & ROOMFLAG_BRIGHTNESS_CALCED != 0 {
            r.br_settled_regional
        } else {
            255
        }
    }

    /// The final brightness of the room a prop is in, as `lights_set_for_room`
    /// lights it (`dlights.c:304`); a prop in no room gets the most there is
    /// (`room_get_max_possible_brightness`).
    pub fn brightness(&self, room: Option<u16>) -> f32 {
        match room {
            Some(r) if (r as usize) < self.rooms.len() && r != 0 => self.room_get_final_brightness_for_player(r as usize) as f32,
            _ => 255.0,
        }
    }

    /// `lights_handle_hit` (`dlights.c:452`): a shot from `gunpos` hitting at
    /// `hitpos` in `roomnum` breaks the first healthy, vulnerable light whose
    /// quad it passes through within 2 cm past the hit. Returns the broken
    /// light's first corner (world) for the glass sound.
    pub fn lights_handle_hit(&mut self, rooms: &BgRooms, gunpos: Vec3, hitpos: Vec3, roomnum: usize) -> Option<Vec3> {
        if roomnum == 0 || roomnum >= rooms.roomcount() || rooms.rooms[roomnum].numlights == 0 {
            return None;
        }
        let b = &rooms.rooms[roomnum];
        let spa4 = gunpos - b.pos;
        let mut sp98 = hitpos - b.pos;
        let mut sp8c = sp98 - spa4;
        let f2 = 2.0 / (sp8c.x * sp8c.x + sp8c.y * sp8c.y + sp8c.z * sp8c.z).sqrt();
        sp8c *= f2;
        sp98 += sp8c;
        let first = b.lightindex?;
        for i in 0..b.numlights {
            let st = self.lights[first + i];
            if !(st.healthy && st.vulnerable) {
                continue;
            }
            let bb = rooms.lights[first + i].bbox.map(|v| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32));
            // func0002f490 is func0002f560 on the s16 corners (lib_2f490_c.c:193).
            let hit = |a: Vec3, b2: Vec3, c: Vec3| pd_core::math::func0002f560(a, b2, c, spa4, sp98, sp8c).is_some();
            if hit(bb[0], bb[1], bb[3]) || hit(bb[1], bb[2], bb[3]) {
                let l = &mut self.lights[first + i];
                l.healthy = false;
                l.on = false;
                self.rooms[roomnum].flags |= ROOMFLAG_LIGHTS_DIRTY;
                return Some(bb[0]);
            }
        }
        None
    }
}

/// `room_set_defaults` (`dlights.c:286`).
fn room_set_defaults(r: &mut RoomLighting) {
    r.br_light_min = 0;
    r.br_light_max = 255;
    r.br_light_each = 0;
    r.br_settled_local = 128;
    r.br_flash = 0;
    r.br_settled_regional = 0;
    r.lightop = LIGHTOP_NONE;
    r.flags &= !(ROOMFLAG_BRIGHTNESS_DIRTY_PERM | ROOMFLAG_LIGHTS_DIRTY | ROOMFLAG_RENDERALWAYS | ROOMFLAG_BRIGHTNESS_CALCED);
    r.volume = 1.0;
    r.surfacearea = 1.0;
    r.lightop_cur_frac = 1.0;
    r.lightop_to_frac = 0.0;
    r.lightop_from_frac = 0.0;
    r.lightop_duration240 = 0.0;
}

/// `bg_calculate_portal_surface_area` (`bg.c:1285`): the fan's triangles.
pub fn bg_calculate_portal_surface_area(verts: &[Vec3]) -> f32 {
    let mut sum = 0.0;
    for i in 2..verts.len() {
        let sp84 = verts[i - 1] - verts[0];
        let sp78 = verts[i] - verts[i - 1];
        let sp90 = [sp84.y * sp78.z - sp84.z * sp78.y, -sp84.x * sp78.z + sp84.z * sp78.x, sp84.x * sp78.y - sp84.y * sp78.x];
        sum += (sp90[0] * sp90[0] + sp90[1] * sp90[1] + sp90[2] * sp90[2]).sqrt() * 0.5;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::Stage;
    use crate::testutil;

    fn stage(code: &str) -> Stage {
        Stage::load(&testutil::assets(), code).unwrap()
    }

    /// Complex has no lights: each room's base is 128 / 4 + 4/5 of the rest
    /// (210), and once lit through its neighbours every room sits between that
    /// and full brightness.
    #[test]
    fn complex_rooms_settle_from_their_base_and_neighbours() {
        let s = stage("ref");
        let l = Lights::new(&s.rooms);
        for (i, r) in l.rooms.iter().enumerate().skip(1) {
            assert_eq!(r.br_base, 210, "room {i}");
            assert_eq!(r.br_settled_local, 210, "room {i}");
            assert!(r.flags & ROOMFLAG_BRIGHTNESS_CALCED != 0, "room {i}");
            assert!((210..=255).contains(&r.br_settled_regional), "room {i}: {}", r.br_settled_regional);
        }
        // A room passes all its own light to itself, and some to a neighbour.
        let t = l.transfer.as_ref().unwrap();
        assert!(t[1][1] > 0);
        let n = s.rooms.bg_room_get_neighbours(1, 8)[0] as usize;
        assert!(t[1][n] > 0 && t[1][n] <= t[1][1]);
    }

    /// A flash in a room on screen brightens it by `start` up to `limit`, and
    /// the rooms whose light reaches it by their share (the transfer isn't
    /// symmetric: a big room's light may not reach a small neighbour); off
    /// screen it does nothing; it decays by 2 per quarter-tick.
    #[test]
    fn a_flash_spreads_to_the_rooms_on_screen_and_decays() {
        let s = stage("ref");
        let mut l = Lights::new(&s.rooms);
        let mut flags = vec![0u16; s.rooms.roomcount()];
        l.room_flash_lighting(&flags, 1, 64, 80);
        assert_eq!(l.rooms[1].br_flash, 0, "off screen");
        for f in flags.iter_mut() {
            *f = ROOMFLAG_ONSCREEN;
        }
        l.room_flash_lighting(&flags, 1, 64, 80);
        assert_eq!(l.rooms[1].br_flash, 64);
        l.room_flash_lighting(&flags, 1, 64, 80);
        assert_eq!(l.rooms[1].br_flash, 80, "the limit");
        // Every room whose light reaches the room (its `vertical` list) takes a
        // share. Most of Complex's rooms keep their light to themselves.
        let t = l.transfer.as_ref().unwrap();
        let cross = (1..t.len()).map(|i| (1..t.len()).filter(|&j| j != i && t[j][i] > 0).count()).collect::<Vec<_>>();
        let r = (1..t.len()).max_by_key(|&i| cross[i - 1]).unwrap();
        l.room_flash_lighting(&flags, r, 64, 80);
        let reached: Vec<(usize, u8)> = l.vertical(r).collect();
        assert!(reached.len() >= 2, "{reached:?}");
        for (k, _) in reached {
            assert!(l.rooms[k].br_flash > 0, "room {k}");
        }
        assert_eq!(l.room_get_final_brightness_for_player(1), 255);
        let mut rng = Rng::new(1);
        l.lighting_tick(&flags, 4, &mut rng, &[]);
        assert_eq!(l.rooms[1].br_flash, 72);
    }

    /// Ruins has the arenas' only lights. A shot through one breaks it (once),
    /// and its room's local brightness drops by the light's share.
    #[test]
    fn a_shot_breaks_a_ruins_light() {
        let s = stage("mp9");
        let mut l = Lights::new(&s.rooms);
        let (room, b) = s.rooms.rooms.iter().enumerate().find(|(_, r)| r.numlights > 0).unwrap();
        let first = b.lightindex.unwrap();
        let bb = s.rooms.lights[first].bbox.map(|v| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32));
        let centre = (bb[0] + bb[1] + bb[2] + bb[3]) * 0.25 + b.pos;
        let n = (bb[1] - bb[0]).cross(bb[3] - bb[0]).normalize();
        let before = l.rooms[room].br_settled_local;
        assert!(l.lights_handle_hit(&s.rooms, centre + n * 100.0, centre, room).is_some() || l.lights_handle_hit(&s.rooms, centre - n * 100.0, centre, room).is_some(), "the light didn't break");
        assert!(!l.lights[first].healthy && !l.lights[first].on);
        assert!(l.lights_handle_hit(&s.rooms, centre + n * 100.0, centre, room).is_none() && l.lights_handle_hit(&s.rooms, centre - n * 100.0, centre, room).is_none(), "broke twice");
        let flags = vec![ROOMFLAG_ONSCREEN; s.rooms.roomcount()];
        l.lighting_tick(&flags, 4, &mut Rng::new(1), &[]);
        assert!(l.rooms[room].br_settled_local < before, "{} -> {}", before, l.rooms[room].br_settled_local);
    }
}
