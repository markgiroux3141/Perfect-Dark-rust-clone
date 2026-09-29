//! PD's rooms and portals (`g_Rooms`, `g_BgPortals`), as `bg_build_tables`
//! builds them from the BG file (`bg.c:1638`), and the queries the game asks of
//! them: which rooms a position is in (`bg_find_rooms_by_pos`,
//! `bg_test_pos_in_room`), which rooms a line passes into (`portal_find_rooms`,
//! `lib/portal.c`), which rooms a box reaches (`bg_find_entered_rooms`), and a
//! room's neighbours (`bg_room_get_neighbours`).
//!
//! Rooms are numbered as PD numbers them: 1..roomcount, with room 0 a dummy
//! (`g_Rooms[0]`'s box is zero, `bg.c:1869`). A portal's open/closed state
//! changes during a match (doors, glass), so it is the world's
//! ([`PortalFlags`]); everything here is the stage's.
//!
//! The data is `bg.json`'s `rooms` (`g_BgRooms` pos, section 3's boxes) and
//! `portals` (`tools/pd-assets/pd_stage.py`).

use glam::Vec3;
use serde::Deserialize;

use pd_core::assets::AssetDir;

/// `PORTALFLAG_*` (`constants.h:3450`).
pub const PORTALFLAG_CLOSED: u8 = 0x01;
pub const PORTALFLAG_USEROOMBOX: u8 = 0x02;
pub const PORTALFLAG_FORCEOPEN: u8 = 0x04;
pub const PORTALFLAG_SKIP: u8 = 0x08;

/// `PORTALINTERSECTION_*` (`constants.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortalIntersection {
    None,
    BehindToFront,
    FrontToBehind,
}

/// `struct portalmetric`: the portal's unit normal (towards its front, where
/// `room2` is) and the range of `normal · v` over its vertices.
#[derive(Clone, Copy, Debug)]
pub struct PortalMetric {
    pub normal: Vec3,
    pub min: f32,
    pub max: f32,
}

/// `struct bgportal` + its `struct portalvertices`.
#[derive(Clone, Debug)]
pub struct BgPortal {
    /// `roomnum1`, `roomnum2`, after `bg_init_portal`'s swap.
    pub room1: u16,
    pub room2: u16,
    /// The flags the file gives it (`bg_build_tables` then clears
    /// `PORTALFLAG_CLOSED`, `bg.c:2008`).
    pub flags: u8,
    pub verts: Vec<Vec3>,
    pub metric: PortalMetric,
}

/// One `struct room`'s static half.
#[derive(Clone, Debug, Default)]
pub struct BgRoom {
    /// `g_BgRooms[r].pos`: the room's display lists are relative to it.
    pub pos: Vec3,
    /// `bbmin`/`bbmax`: section 3's box, grown to take in the room's portals
    /// (`bg_expand_room_to_portals`, `bg.c:1969`).
    pub bbmin: Vec3,
    pub bbmax: Vec3,
    /// From section 3's box before it was grown (`bg.c:1925-1935`).
    pub centre: Vec3,
    pub radius: f32,
    pub br_light_min: u8,
    pub br_light_max: u8,
    pub numlights: usize,
    /// Into [`BgRooms::lights`]; `None` with no lights (`bg.c:1956`).
    pub lightindex: Option<usize>,
    /// `g_RoomPortals[roomportallistoffset..][..numportals]`: the portals that
    /// touch this room, sorted by neighbour (with PD's sort, `bg.c:1768`).
    pub portals: Vec<usize>,
    /// `ROOMFLAG_COMPLICATEDPORTALS` (`bg_init_room`, `bg.c:2091`).
    pub complicated: bool,
}

/// `struct light` (`types.h`): one entry of the lights file.
#[derive(Clone, Debug, Deserialize)]
pub struct BgLight {
    pub roomnum: u16,
    /// 4/4/4/4 bits.
    pub colour: u16,
    pub brightness: u8,
    pub sparkable: u8,
    pub healthy: u8,
    pub on: u8,
    pub sparking: u8,
    pub vulnerable: u8,
    pub brightnessmult: u8,
    pub dir: [i8; 3],
    pub bbox: [[i16; 3]; 4],
}

/// The stage's rooms, portals and lights.
#[derive(Clone, Debug, Default)]
pub struct BgRooms {
    /// Indexed by room number; `rooms[0]` is PD's dummy room 0.
    pub rooms: Vec<BgRoom>,
    pub portals: Vec<BgPortal>,
    pub lights: Vec<BgLight>,
    /// `g_Vars.roomportalrecursionlimit`: the most portals any room has.
    pub roomportalrecursionlimit: usize,
    /// The stage's z range (`g_Env` near and far, `env.c`), what
    /// `vi_get_z_range` gives the portal code.
    pub zrange: (f32, f32),
}

/// The world's copy of every portal's flags (`g_BgPortals[].flags`).
pub type PortalFlags = Vec<u8>;

/// `PORTAL_IS_CLOSED` (`constants.h:128`).
pub fn portal_is_closed(flags: u8) -> bool {
    flags & PORTALFLAG_CLOSED != 0 && flags & PORTALFLAG_FORCEOPEN == 0
}

// ─── the file ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RoomRow {
    room: u16,
    pos: [f32; 3],
    bbmin: [f32; 3],
    bbmax: [f32; 3],
    br_light_min: u8,
    br_light_max: u8,
    numlights: usize,
    lightindex: i32,
}

#[derive(Deserialize)]
struct PortalRow {
    rooms: [i16; 2],
    flags: u8,
    verts: Vec<[f32; 3]>,
}

#[derive(Deserialize)]
struct NoFogEnv {
    near: f32,
    far: f32,
}

#[derive(Deserialize)]
struct EnvRow {
    nofogenvironment: NoFogEnv,
}

#[derive(Deserialize)]
struct BgHeader {
    env: EnvRow,
    rooms: Vec<RoomRow>,
    portals: Vec<PortalRow>,
    bgcmds: Vec<[i32; 3]>,
    lights: Vec<BgLight>,
}

/// `BGCMD_END` (`bg.c:47`).
const BGCMD_END: i32 = 0x00;

impl BgRooms {
    /// Load `stages/<code>/bg.json`'s room and portal tables.
    pub fn load(assets: &AssetDir, code: &str) -> Result<BgRooms, String> {
        let dir = assets.stage(code);
        let h: BgHeader = assets.read_json(&dir.join("bg.json"))?;
        // `bg_cmd_execute` (`bg.c:5203`) runs the stage's portal commands every
        // frame; no Combat Simulator arena has any (their list is just END).
        if h.bgcmds.iter().any(|c| c[0] != BGCMD_END) {
            return Err(format!("stage {code}: bg commands are not ported"));
        }
        let mut rooms = vec![BgRoom::default(); h.rooms.len() + 1];
        for r in &h.rooms {
            let i = r.room as usize;
            if i == 0 || i >= rooms.len() {
                return Err(format!("stage {code}: room {i} out of range"));
            }
            rooms[i] = BgRoom {
                pos: r.pos.into(),
                bbmin: r.bbmin.into(),
                bbmax: r.bbmax.into(),
                br_light_min: r.br_light_min,
                br_light_max: r.br_light_max,
                numlights: r.numlights,
                lightindex: (r.lightindex >= 0).then_some(r.lightindex as usize),
                ..Default::default()
            };
        }
        let mut portals = Vec::with_capacity(h.portals.len());
        for (i, p) in h.portals.iter().enumerate() {
            let ok = |r: i16| r >= 0 && (r as usize) < rooms.len();
            if !ok(p.rooms[0]) || !ok(p.rooms[1]) || p.verts.len() < 3 {
                return Err(format!("stage {code}: portal {i} is malformed"));
            }
            let verts: Vec<Vec3> = p.verts.iter().map(|&v| Vec3::from(v)).collect();
            portals.push(BgPortal { room1: p.rooms[0] as u16, room2: p.rooms[1] as u16, flags: p.flags, metric: portal_metric(&verts), verts });
        }
        let mut b = BgRooms::build(rooms, portals, h.lights);
        b.zrange = (h.env.nofogenvironment.near, h.env.nofogenvironment.far);
        Ok(b)
    }

    /// One room holding everything, with no portals: a test fixture's.
    pub fn single(bbmin: Vec3, bbmax: Vec3) -> BgRooms {
        let room = BgRoom { bbmin, bbmax, br_light_min: 128, br_light_max: 255, ..Default::default() };
        // The arenas' z range (the firing range's SUBST, M5).
        let mut b = BgRooms::build(vec![BgRoom::default(), room], Vec::new(), Vec::new());
        b.zrange = (15.0, 10000.0);
        b
    }

    /// `bg_build_tables`' portal and room half (`bg.c:1703-2010`), from the
    /// file's rooms (with section 3's boxes) and portals.
    fn build(mut rooms: Vec<BgRoom>, mut portals: Vec<BgPortal>, lights: Vec<BgLight>) -> BgRooms {
        // g_RoomPortals, by room ascending (room 0 included, bg.c:1739).
        let mut roomportalrecursionlimit = 0;
        for (i, room) in rooms.iter_mut().enumerate() {
            room.portals.clear();
            for (j, p) in portals.iter().enumerate() {
                if i == p.room1 as usize {
                    room.portals.push(j);
                }
                if i == p.room2 as usize {
                    room.portals.push(j);
                }
            }
            roomportalrecursionlimit = roomportalrecursionlimit.max(room.portals.len());
        }
        // The sort by neighbour room, with its bug: `k` doesn't restart after a
        // swap (bg.c:1768-1807).
        for (i, room) in rooms.iter_mut().enumerate() {
            let other = |pn: usize| if portals[pn].room1 as usize == i { portals[pn].room2 } else { portals[pn].room1 };
            let n = room.portals.len();
            for j in 0..n {
                let mut thisneighbournum = other(room.portals[j]);
                for k in j..n {
                    if other(room.portals[k]) < thisneighbournum {
                        room.portals.swap(j, k);
                        thisneighbournum = other(room.portals[j]);
                    }
                }
            }
        }
        // Centres and radii from section 3's boxes (bg.c:1925-1935); room 0's
        // box is zero (bg.c:1869).
        rooms[0].bbmin = Vec3::ZERO;
        rooms[0].bbmax = Vec3::ZERO;
        for room in rooms.iter_mut().skip(1) {
            room.centre = (room.bbmin + room.bbmax) / 2.0;
            let d = room.bbmin - room.bbmax;
            room.radius = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt() / 2.0;
        }
        for p in portals.iter_mut() {
            bg_init_portal(p, &rooms);
        }
        for r in 1..rooms.len() {
            rooms[r].complicated = bg_init_room(r, &rooms[r], &portals);
        }
        for r in 1..rooms.len() {
            bg_expand_room_to_portals(r, &mut rooms, &portals);
        }
        for p in portals.iter_mut() {
            p.flags &= !PORTALFLAG_CLOSED;
        }
        BgRooms { rooms, portals, lights, roomportalrecursionlimit, zrange: (15.0, 10000.0) }
    }

    /// `g_Vars.roomcount` (room 0 counts).
    pub fn roomcount(&self) -> usize {
        self.rooms.len()
    }

    /// The flags every portal starts a match with.
    pub fn initial_portal_flags(&self) -> PortalFlags {
        self.portals.iter().map(|p| p.flags).collect()
    }

    /// `bg_room_get_neighbours` (`bg.c:5869`): the rooms across this room's
    /// portals, each once, at most `len`.
    pub fn bg_room_get_neighbours(&self, roomnum: usize, len: usize) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        for &pn in &self.rooms[roomnum].portals {
            let p = &self.portals[pn];
            let neighbournum = if p.room1 as usize == roomnum { p.room2 } else { p.room1 };
            if out.contains(&neighbournum) {
                continue;
            }
            out.push(neighbournum);
            if out.len() >= len {
                break;
            }
        }
        out
    }

    /// `bg_rooms_are_neighbours` (`bg.c:5905`).
    pub fn bg_rooms_are_neighbours(&self, roomnum1: usize, roomnum2: usize) -> bool {
        self.rooms[roomnum1].portals.iter().any(|&pn| {
            let p = &self.portals[pn];
            p.room1 as usize == roomnum2 || p.room2 as usize == roomnum2
        })
    }

    /// `bg_room_contains_coord` (`bg.c:4643`): inside the room's box.
    pub fn bg_room_contains_coord(&self, pos: Vec3, roomnum: usize) -> bool {
        let r = &self.rooms[roomnum];
        pos.x >= r.bbmin.x && pos.x <= r.bbmax.x && pos.z >= r.bbmin.z && pos.z <= r.bbmax.z && pos.y >= r.bbmin.y && pos.y <= r.bbmax.y
    }

    /// `bg_test_pos_in_room_cheap` (`bg.c:4670`): on the inside of every portal.
    pub fn bg_test_pos_in_room_cheap(&self, pos: Vec3, roomnum: usize) -> bool {
        for &pn in &self.rooms[roomnum].portals {
            let p = &self.portals[pn];
            let value = p.metric.normal.dot(pos);
            if value < p.metric.min {
                if roomnum != p.room1 as usize {
                    return false;
                }
            } else if value > p.metric.max && roomnum != p.room2 as usize {
                return false;
            }
        }
        true
    }

    /// `bg_test_pos_in_room_expensive` (`bg.c:4696`): only the portals the line
    /// from the room's centre to `pos` passes through count.
    pub fn bg_test_pos_in_room_expensive(&self, pos: Vec3, roomnum: usize) -> bool {
        let sp74 = self.rooms[roomnum].centre;
        for &pn in &self.rooms[roomnum].portals {
            let p = &self.portals[pn];
            let m = &p.metric;
            let f0 = pos.dot(m.normal);
            let f18 = sp74.dot(m.normal);
            if f0 < m.min {
                if f18 < m.min {
                    continue;
                }
            } else if f0 > m.max && f18 > m.max {
                continue;
            }
            let sp68 = sp74 - pos;
            let mut t4 = 0;
            let mut t5 = true;
            let n = p.verts.len();
            for j in 0..n {
                let cur = p.verts[j];
                let next = p.verts[if j + 1 == n { 0 } else { j + 1 }];
                let sp5c = next - cur;
                let sp4c = Vec3::new(sp5c.y * sp68.z - sp5c.z * sp68.y, sp5c.z * sp68.x - sp5c.x * sp68.z, sp5c.x * sp68.y - sp5c.y * sp68.x);
                let sum = sp4c.x * sp4c.x + sp4c.y * sp4c.y + sp4c.z * sp4c.z;
                if sum == 0.0 {
                    t5 = false;
                    break;
                }
                let sp58 = sp4c.x * cur.x + sp4c.y * cur.y + sp4c.z * cur.z;
                let sum = sp4c.x * pos.x + sp4c.y * pos.y + sp4c.z * pos.z;
                if sum < sp58 {
                    if t4 == 2 {
                        t5 = false;
                        break;
                    }
                    t4 = 1;
                } else if t4 == 1 {
                    t5 = false;
                    break;
                } else {
                    t4 = 2;
                }
            }
            if t5 {
                if f0 < m.min {
                    if roomnum == p.room2 as usize {
                        return false;
                    }
                } else if f0 > m.max && roomnum == p.room1 as usize {
                    return false;
                }
            }
        }
        true
    }

    /// `bg_test_pos_in_room` (`bg.c:4804`).
    pub fn bg_test_pos_in_room(&self, pos: Vec3, roomnum: usize) -> bool {
        if self.rooms[roomnum].complicated {
            self.bg_test_pos_in_room_expensive(pos, roomnum)
        } else {
            self.bg_test_pos_in_room_cheap(pos, roomnum)
        }
    }

    /// `bg_find_rooms_by_pos` (`bg.c:4833`): the rooms whose box holds `pos`
    /// and those it is above (rooms with portals first, else the others), at
    /// most `max` each; and with neither, the nearest room by box distance.
    pub fn bg_find_rooms_by_pos(&self, pos: Vec3, max: usize) -> (Vec<u16>, Vec<u16>, Option<u16>) {
        let mut inrooms = Vec::new();
        let mut aboverooms = Vec::new();
        for pass in [true, false] {
            // Rooms without portals only when those with found nothing.
            if !(pass || inrooms.is_empty() && aboverooms.is_empty()) {
                break;
            }
            for (i, r) in self.rooms.iter().enumerate().skip(1) {
                if (r.portals.is_empty() != pass)
                    && pos.x >= r.bbmin.x
                    && pos.x <= r.bbmax.x
                    && pos.z >= r.bbmin.z
                    && pos.z <= r.bbmax.z
                    && pos.y >= r.bbmin.y
                {
                    if pos.y <= r.bbmax.y {
                        if inrooms.len() < max {
                            inrooms.push(i as u16);
                        }
                    } else if aboverooms.len() < max {
                        aboverooms.push(i as u16);
                    }
                }
            }
        }
        let mut bestroom = None;
        if inrooms.is_empty() && aboverooms.is_empty() {
            let mut closestdist = 0.0;
            for (i, r) in self.rooms.iter().enumerate().skip(1) {
                let mut dist = 0.0;
                for j in 0..3 {
                    if pos[j] < r.bbmin[j] || pos[j] > r.bbmax[j] {
                        let dist1 = (pos[j] - r.bbmin[j]).abs();
                        let dist2 = (pos[j] - r.bbmax[j]).abs();
                        dist += dist1.min(dist2);
                    }
                }
                if dist > 0.0 && (bestroom.is_none() || dist < closestdist) {
                    bestroom = Some(i as u16);
                    closestdist = dist;
                }
            }
        }
        (inrooms, aboverooms, bestroom)
    }

    /// `portal_calculate_intersection` (`lib/portal.c:89`): does the line
    /// `pos1 → pos2` pass through the portal, and which way. Also returns
    /// `var8007fcb4` (how far past `min` the line's middle is), which
    /// `bg_find_portal_between_positions` ranks by.
    pub fn portal_calculate_intersection(&self, portalnum: usize, pos1: Vec3, pos2: Vec3) -> (PortalIntersection, Option<f32>) {
        let p = &self.portals[portalnum];
        let m = &p.metric;
        let value1 = pos1.dot(m.normal);
        let value2 = pos2.dot(m.normal);
        if value1 < m.min {
            if value2 < m.min {
                return (PortalIntersection::None, None);
            }
        } else if m.max < value1 && m.max < value2 {
            return (PortalIntersection::None, None);
        }
        let sp60 = pos2 - pos1;
        let fcb4 = (value1 + value2) * 0.5 - m.min;
        let mut lastside = 0u8;
        let n = p.verts.len();
        for i in 0..n {
            let curr = p.verts[i];
            let next = p.verts[if i + 1 == n { 0 } else { i + 1 }];
            let sp48 = next - curr;
            let sp34 = Vec3::new(sp48.y * sp60.z - sp48.z * sp60.y, sp48.z * sp60.x - sp48.x * sp60.z, sp48.x * sp60.y - sp48.y * sp60.x);
            let tmp = sp34.x * sp34.x + sp34.y * sp34.y + sp34.z * sp34.z;
            if tmp == 0.0 {
                return (PortalIntersection::None, Some(fcb4));
            }
            let sp40 = sp34.x * curr.x + sp34.y * curr.y + sp34.z * curr.z;
            let tmp = sp34.x * pos1.x + sp34.y * pos1.y + sp34.z * pos1.z;
            if tmp < sp40 {
                if lastside == 2 {
                    return (PortalIntersection::None, Some(fcb4));
                }
                lastside = 1;
            } else {
                if lastside == 1 {
                    return (PortalIntersection::None, Some(fcb4));
                }
                lastside = 2;
            }
        }
        let dir = if value1 < m.min { PortalIntersection::BehindToFront } else { PortalIntersection::FrontToBehind };
        (dir, Some(fcb4))
    }

    /// `portal_find_rooms` (`lib/portal.c:185`): starting in `fromrooms` at
    /// `frompos`, the rooms `topos` ends up in by crossing portals (at most 7),
    /// and every room passed through (`intersecting`, at most 15).
    pub fn portal_find_rooms(&self, frompos: Vec3, topos: Vec3, fromrooms: &[u16]) -> (Vec<u16>, Vec<u16>) {
        let mut srcrooms: Vec<u16> = fromrooms.iter().copied().take(8).collect();
        let mut allrooms = srcrooms.clone();
        // var8009a4e0: each portal's intersection, worked out once per call and
        // spent when crossed.
        let mut cache: Vec<Option<PortalIntersection>> = vec![None; self.portals.len()];
        loop {
            let mut foundrooms: Vec<u16> = Vec::new();
            for &roomnum in srcrooms.iter().take(16) {
                for &pn in &self.rooms[roomnum as usize].portals {
                    let s1 = cache[pn].get_or_insert_with(|| self.portal_calculate_intersection(pn, frompos, topos).0);
                    let p = &self.portals[pn];
                    if *s1 == PortalIntersection::BehindToFront && roomnum == p.room1 {
                        portal_append_room(&mut foundrooms, p.room2);
                        portal_append_room(&mut allrooms, p.room2);
                        *s1 = PortalIntersection::None;
                    }
                    if *s1 == PortalIntersection::FrontToBehind && roomnum == p.room2 {
                        portal_append_room(&mut foundrooms, p.room1);
                        portal_append_room(&mut allrooms, p.room1);
                        *s1 = PortalIntersection::None;
                    }
                }
            }
            if foundrooms.is_empty() {
                break;
            }
            srcrooms = foundrooms;
        }
        srcrooms.truncate(7);
        (srcrooms, allrooms)
    }

    /// `bg_calculate_portal_bbox` (`bg.c:6203`), from MAXFLOAT / MINFLOAT.
    pub fn bg_calculate_portal_bbox(&self, portalnum: usize) -> (Vec3, Vec3) {
        let v = &self.portals[portalnum].verts;
        let lo = v.iter().copied().fold(Vec3::splat(f32::MAX), Vec3::min);
        let hi = v.iter().copied().fold(Vec3::splat(f32::MIN), Vec3::max);
        (lo, hi)
    }

    /// `bg_find_entered_rooms` (`bg.c:6234`): `rooms` grown by every room whose
    /// portal the box overlaps, repeatedly, up to `maxlen` (skipping closed
    /// portals when `skipclosed`).
    pub fn bg_find_entered_rooms(&self, bbmin: Vec3, bbmax: Vec3, rooms: &mut Vec<u16>, maxlen: usize, skipclosed: bool, flags: &[u8]) {
        let mut i = 0;
        loop {
            let origlen = rooms.len();
            while i < origlen {
                let room = rooms[i] as usize;
                for &pn in &self.rooms[room].portals {
                    if skipclosed && portal_is_closed(flags[pn]) {
                        continue;
                    }
                    let (plo, phi) = self.bg_calculate_portal_bbox(pn);
                    if bg_is_bbox_overlapping(plo, phi, bbmin, bbmax) {
                        let p = &self.portals[pn];
                        let otherroom = if room == p.room1 as usize { p.room2 } else { p.room1 };
                        if !rooms.contains(&otherroom) {
                            if rooms.len() < maxlen {
                                rooms.push(otherroom);
                            }
                            if rooms.len() >= maxlen {
                                return;
                            }
                        }
                    }
                }
                i += 1;
            }
            if rooms.len() == origlen {
                return;
            }
        }
    }

    /// `bg_find_portal_between_positions` (`bg.c:6161`): the portal the line
    /// crosses whose middle is nearest its plane.
    pub fn bg_find_portal_between_positions(&self, pos1: Vec3, pos2: Vec3) -> Option<usize> {
        let mut best = None;
        let mut bestthing = f32::MAX;
        for i in 0..self.portals.len() {
            if let (dir, Some(fcb4)) = self.portal_calculate_intersection(i, pos1, pos2) {
                if dir != PortalIntersection::None && fcb4.abs() < bestthing {
                    best = Some(i);
                    bestthing = fcb4.abs();
                }
            }
        }
        best
    }

    /// `portal_get_centre` (`lib/portal.c:25`).
    pub fn portal_get_centre(&self, portalnum: usize) -> Vec3 {
        let v = &self.portals[portalnum].verts;
        let f0 = 1.0 / v.len() as f32;
        let mut avg = v[0];
        for w in &v[1..] {
            avg += *w;
        }
        avg * f0
    }
}

/// `portal_append_room` (`lib/portal.c:59`): add a room once, keeping one of
/// the 16 slots for the terminator.
fn portal_append_room(rooms: &mut Vec<u16>, roomnum: u16) {
    if rooms.iter().take(16).any(|&r| r == roomnum) {
        return;
    }
    if rooms.len() < 15 {
        rooms.push(roomnum);
    }
}

/// `bg_is_bbox_overlapping` (`bg.c:6190`).
pub fn bg_is_bbox_overlapping(portalbbmin: Vec3, portalbbmax: Vec3, propbbmin: Vec3, propbbmax: Vec3) -> bool {
    (0..3).all(|i| !(propbbmin[i] > portalbbmax[i] || propbbmax[i] < portalbbmin[i]))
}

/// `g_PortalMetrics[i]` (`bg.c:1818-1857`): the normal from the vertices'
/// winding (clockwise faces the viewer), negated and normalised, and the range
/// of the vertices along it.
fn portal_metric(verts: &[Vec3]) -> PortalMetric {
    let mut n = Vec3::ZERO;
    for j in 0..verts.len() {
        let a = verts[j];
        let b = verts[(j + 1) % verts.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    let divisor = -(n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
    let normal = n / divisor;
    // MAXFLOAT / MINFLOAT (constants.h:49).
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for v in verts {
        let value = v.x * normal.x + v.y * normal.y + v.z * normal.z;
        if value < min {
            min = value;
        }
        if value > max {
            max = value;
        }
    }
    PortalMetric { normal, min, max }
}

/// `bg_init_portal` (`bg.c:6020`): make `room2` the room on the front side, by
/// the rooms' centres.
fn bg_init_portal(p: &mut BgPortal, rooms: &[BgRoom]) {
    let c1 = rooms[p.room1 as usize].centre;
    let c2 = rooms[p.room2 as usize].centre;
    let mut m = p.metric;
    let tmp1 = m.normal.dot(c1);
    let mut swapped = false;
    if tmp1 > m.max {
        swapped = true;
        std::mem::swap(&mut p.room1, &mut p.room2);
        m.normal = -m.normal;
        let tmp = m.min;
        m.min = -m.max;
        m.max = -tmp;
    }
    let tmp2 = m.normal.dot(c2);
    if tmp2 <= m.min && swapped {
        std::mem::swap(&mut p.room1, &mut p.room2);
    }
}

/// `bg_init_room` (`bg.c:6091`): a room has complicated portals when another of
/// its portals lies (partly) outside one portal's plane.
fn bg_init_room(roomnum: usize, room: &BgRoom, portals: &[BgPortal]) -> bool {
    for &pn in &room.portals {
        let mut m = portals[pn].metric;
        if roomnum == portals[pn].room1 as usize {
            m.normal = -m.normal;
            let tmp = m.min;
            m.min = -m.max;
            m.max = -tmp;
        }
        for &pn2 in &room.portals {
            if pn2 == pn {
                continue;
            }
            if portals[pn2].verts.iter().any(|v| m.normal.dot(*v) < m.min) {
                return true;
            }
        }
    }
    false
}

/// `bg_expand_room_to_portals` (`bg.c:1969`).
fn bg_expand_room_to_portals(roomnum: usize, rooms: &mut [BgRoom], portals: &[BgPortal]) {
    let list = rooms[roomnum].portals.clone();
    let r = &mut rooms[roomnum];
    for pn in list {
        for v in &portals[pn].verts {
            r.bbmin = r.bbmin.min(*v);
            r.bbmax = r.bbmax.max(*v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rooms(code: &str) -> BgRooms {
        BgRooms::load(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR")), code).unwrap()
    }

    /// Every arena's portals join two real rooms, lie flat (every vertex on the
    /// plane), and put `room2` on their front; each room's box holds its
    /// centre.
    #[test]
    fn every_arena_has_sane_rooms_and_portals() {
        for code in crate::stage::ARENAS {
            let b = rooms(code);
            assert!(b.rooms.len() > 20 && !b.portals.is_empty(), "{code}");
            for (i, p) in b.portals.iter().enumerate() {
                assert!(p.room1 != 0 && p.room2 != 0 && p.room1 != p.room2, "{code} portal {i}: rooms {} {}", p.room1, p.room2);
                assert!((p.metric.normal.length() - 1.0).abs() < 1e-4);
                assert!(p.metric.max - p.metric.min < 1.0, "{code} portal {i} is not flat ({} .. {})", p.metric.min, p.metric.max);
            }
            for r in b.rooms.iter().skip(1) {
                assert!(r.bbmin.cmple(r.centre).all() && r.centre.cmple(r.bbmax).all());
            }
        }
    }

    /// A room's neighbours are symmetric, and a point just past a portal on each
    /// side lands in the room that side belongs to.
    #[test]
    fn a_point_either_side_of_a_portal_is_in_that_sides_room() {
        let b = rooms("ref");
        let mut checked = 0;
        for (i, p) in b.portals.iter().enumerate() {
            let (r1, r2) = (p.room1 as usize, p.room2 as usize);
            assert!(b.bg_room_get_neighbours(r1, 20).contains(&p.room2), "portal {i}");
            assert!(b.bg_rooms_are_neighbours(r2, r1));
            let c = b.portal_get_centre(i);
            let front = c + p.metric.normal * 5.0;
            let back = c - p.metric.normal * 5.0;
            if !b.rooms[r1].complicated && !b.rooms[r2].complicated {
                assert!(b.bg_test_pos_in_room(front, r2) && !b.bg_test_pos_in_room(front, r1), "portal {i} front");
                assert!(b.bg_test_pos_in_room(back, r1) && !b.bg_test_pos_in_room(back, r2), "portal {i} back");
                checked += 1;
            }
            // A line through the portal crosses into the other room.
            let (to, all) = b.portal_find_rooms(back, front, &[p.room1]);
            assert_eq!(to, vec![p.room2], "portal {i}");
            assert_eq!(all, vec![p.room1, p.room2]);
        }
        // 32 of Complex's 60 portals join two rooms without complicated portals.
        assert!(checked >= 30, "{checked}");
    }

    /// Every spawn pad on every arena has a room and a floor under it (within
    /// 1.5 m: Felicity's pad 28 is 136 cm up, and a spawn drops to the floor)
    /// that is in that room or one next to it.
    #[test]
    fn every_arenas_spawn_pads_stand_over_a_floor() {
        let a = pd_core::assets::AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
        for code in crate::stage::ARENAS {
            let stage = crate::stage::Stage::load(&a, code).unwrap();
            let level = crate::stage::TileLevel::for_stage(&stage);
            assert!(!stage.spawn_pads.is_empty(), "{code}: no spawn pads");
            for &p in &stage.spawn_pads {
                let pad = &stage.pads[p];
                let room = pad.room.unwrap_or_else(|| panic!("{code}: spawn pad {p} has no room"));
                let (y, poly) = level.cd_find_ground_at_cyl(pad.pos, 30.0);
                assert!(poly.is_some() && pad.pos.y - y < 150.0 && pad.pos.y >= y, "{code}: spawn pad {p} at {} over a floor at {y}", pad.pos);
                let froom = level.geom.polys[poly.unwrap()].room.unwrap();
                assert!(froom == room || level.rooms_are_neighbours(room, froom), "{code}: spawn pad {p} in room {room} over room {froom}'s floor");
            }
        }
    }
}
