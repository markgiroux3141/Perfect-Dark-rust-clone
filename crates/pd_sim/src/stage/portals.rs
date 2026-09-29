//! Which rooms a player can see this frame: `bg_tick_portals` (`bg.c:5698`)
//! and its x-ray variant (`bg_tick_portals_xray`, `bg.c:5223`).
//!
//! Starting in the camera's room with the whole view as its box, every portal
//! of a room on screen that faces the camera and is open clips the box to its
//! own screen box; a room whose clipped box is not empty is on screen, with the
//! box to scissor it to, and its portals are followed in turn (a queue, a room
//! at most `roomportalrecursionlimit` times). Each on-screen room gets a draw
//! slot with its depth in the traversal (`draworder`), which is the order
//! `bg_render_scene` draws in. The rooms next to an on-screen room are on
//! standby (`bg_choose_rooms_to_load`).
//!
//! PD keeps this on `g_Rooms` and redoes it for each player's pass; the
//! result, [`PortalView`], lives on the player, and the renderer reads it.
//! The per-frame caches (`g_PortalCameraCache`, `g_BgDrawSlotsByRoom`) are
//! kept for one call, which is what PD's frame counter makes them.

use glam::{Mat4, Vec3};

use super::rooms::{portal_is_closed, BgRooms, PORTALFLAG_SKIP, PORTALFLAG_USEROOMBOX};

/// `ROOMFLAG_ONSCREEN` / `ROOMFLAG_STANDBY` (`constants.h:3614`).
pub const ROOMFLAG_ONSCREEN: u16 = 0x0004;
pub const ROOMFLAG_STANDBY: u16 = 0x0008;

/// `struct screenbox`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenBox {
    pub xmin: i16,
    pub ymin: i16,
    pub xmax: i16,
    pub ymax: i16,
}

impl ScreenBox {
    /// `bg_get_box_intersection` (`bg.c:2473`), in place.
    fn intersect(&mut self, b: &ScreenBox) -> bool {
        self.xmin = self.xmin.max(b.xmin);
        self.ymin = self.ymin.max(b.ymin);
        self.xmax = if b.xmax > self.xmax { self.xmax } else { b.xmax };
        self.ymax = if b.ymax > self.ymax { self.ymax } else { b.ymax };
        if self.xmin >= self.xmax {
            self.xmin = self.xmax;
            return false;
        }
        if self.ymax <= self.ymin {
            self.ymin = self.ymax;
            return false;
        }
        true
    }

    /// `bg_expand_box` (`bg.c:2493`).
    fn expand(&mut self, b: &ScreenBox) {
        self.xmin = self.xmin.min(b.xmin);
        self.ymin = self.ymin.min(b.ymin);
        self.xmax = self.xmax.max(b.xmax);
        self.ymax = self.ymax.max(b.ymax);
    }
}

/// `struct drawslot`.
#[derive(Clone, Copy, Debug)]
pub struct DrawSlot {
    pub roomnum: u16,
    pub draworder: u8,
    /// The scissor the room is drawn with.
    pub rect: ScreenBox,
}

/// The camera as the portal code reads it (`struct player`'s `c_*` fields,
/// `worldtoscreenmtx`, the view rectangle).
#[derive(Clone, Copy, Debug)]
pub struct PortalCam {
    /// `cam_get_world_to_screen_mtxf()`: world → camera space (−z ahead).
    pub world_to_screen: Mat4,
    pub cam_pos: Vec3,
    pub c_screenleft: f32,
    pub c_screentop: f32,
    pub c_halfwidth: f32,
    pub c_halfheight: f32,
    pub c_recipscalex: f32,
    pub c_recipscaley: f32,
    /// `bg_calculate_screen_properties` (`bg.c:5920`): the view rectangle
    /// clamped to the framebuffer (`screenxminf` ... `screenymaxf`).
    pub view: [f32; 4],
    /// `g_BgQueue.zrange.far` (`vi_get_z_range` / `scale_bg2gfx`).
    pub zfar: f32,
}

impl PortalCam {
    /// `bg_calculate_screen_properties`: `view` from the viewport and the
    /// framebuffer's size.
    pub fn screen_properties(left: f32, top: f32, width: f32, height: f32, fbwidth: f32, fbheight: f32) -> [f32; 4] {
        let c = |v: f32, max: f32| v.max(0.0).min(max);
        [c(left, fbwidth), c(top, fbheight), c(left + width, fbwidth), c(top + height, fbheight)]
    }

    /// `cam0f0b4d68` (`camera.c:135`): camera space to screen pixels.
    fn project(&self, p: Vec3) -> [f32; 2] {
        let value = if p.z == 0.0 { -100000000000000000000.0 } else { 1.0 / p.z };
        [(self.c_screenleft + self.c_halfwidth) - p.x * value * self.c_recipscalex, p.y * value * self.c_recipscaley + (self.c_screentop + self.c_halfheight)]
    }

    fn full_box(&self) -> ScreenBox {
        ScreenBox { xmin: self.view[0] as i16, ymin: self.view[1] as i16, xmax: self.view[2] as i16, ymax: self.view[3] as i16 }
    }
}

/// One player's rooms this frame.
#[derive(Clone, Debug, Default)]
pub struct PortalView {
    /// `g_BgDrawSlots[0..g_BgNumDrawSlots]`, in the order they were found.
    pub drawslots: Vec<DrawSlot>,
    /// `g_Rooms[].flags`' ONSCREEN / STANDBY bits, by room.
    pub roomflags: Vec<u16>,
    /// `g_BgDrawSlots[60]`: the whole view, for rooms without a slot.
    pub fullbox: ScreenBox,
}

impl PortalView {
    pub fn is_onscreen(&self, room: usize) -> bool {
        self.roomflags.get(room).is_some_and(|f| f & ROOMFLAG_ONSCREEN != 0)
    }

    pub fn is_standby(&self, room: usize) -> bool {
        self.roomflags.get(room).is_some_and(|f| f & ROOMFLAG_STANDBY != 0)
    }

    /// `bg_get_room_draw_slot` (`bg.c:279`): the room's scissor, or the whole view.
    pub fn room_box(&self, room: usize) -> ScreenBox {
        self.drawslots.iter().find(|s| s.roomnum as usize == room).map_or(self.fullbox, |s| s.rect)
    }

    /// The draw slots sorted by draw order, as `bg_render_scene` sorts them
    /// (`bg.c:1011`: a bubble sort, so equal orders keep their slot order).
    pub fn draw_order(&self) -> Vec<DrawSlot> {
        let mut v = self.drawslots.clone();
        // A stable sort is the bubble sort's result.
        v.sort_by_key(|s| s.draworder);
        v
    }
}

/// `struct bgqueueitem`.
#[derive(Clone, Copy, Default)]
struct QueueItem {
    roomnum: u16,
    fromroomnums: [i32; 5],
    depth: u8,
    screenbox: ScreenBox,
}

const QUEUE_LEN: usize = 250;

struct Tick<'a> {
    rooms: &'a BgRooms,
    flags: &'a [u8],
    cam: &'a PortalCam,
    view: PortalView,
    /// `g_Rooms[].portalrecursioncount`, `queuecount`.
    recursion: Vec<u8>,
    queuecount: Vec<u8>,
    /// `g_BgDrawSlotsByRoom`: this frame's slot of each room.
    slot_of: Vec<Option<usize>>,
    numattempted: usize,
    /// `g_PortalCameraCache`: the camera's side (0 front, 1 behind, 2 in the
    /// slab) and the screen box, each once per frame.
    side: Vec<Option<u8>>,
    bbox: Vec<Option<(bool, ScreenBox)>>,
    queue: Vec<QueueItem>,
    head: usize,
    tail: usize,
}

/// `bg_tick_portals` (`bg.c:5698`) for a camera in `cam_room`, with the
/// portals' current flags.
pub fn bg_tick_portals(rooms: &BgRooms, flags: &[u8], cam: &PortalCam, cam_room: usize) -> PortalView {
    let n = rooms.roomcount();
    let mut t = Tick {
        rooms,
        flags,
        cam,
        view: PortalView { drawslots: Vec::new(), roomflags: vec![0; n], fullbox: cam.full_box() },
        recursion: vec![0; n],
        queuecount: vec![0; n],
        slot_of: vec![None; n],
        numattempted: 0,
        side: vec![None; rooms.portals.len()],
        bbox: vec![None; rooms.portals.len()],
        queue: vec![QueueItem::default(); QUEUE_LEN],
        head: 0,
        tail: 0,
    };
    let full = t.view.fullbox;
    // bg_cmd_execute(g_BgCommands): the arenas have none (BgRooms::load).
    if rooms.portals.is_empty() {
        // Unreachable in PD ("all BGs have portals"); the test fixtures take it.
        for room in 1..n {
            if bg_room_intersects_screen_box(rooms, cam, room, &full) {
                t.bg_set_room_onscreen(room, 0, full);
            }
        }
    } else {
        t.bg_set_room_onscreen(cam_room, 0, full);
        t.bg_add_to_queue(cam_room as i32, cam_room, 1, &full);
        while t.head != t.tail {
            let item = t.queue[t.tail];
            t.bg_process_queue_item(&item);
            t.tail = (t.tail + 1) % QUEUE_LEN;
        }
    }
    bg_choose_rooms_to_load(rooms, flags, &mut t.view);
    t.view
}

/// `bg_tick_portals_xray` (`bg.c:5223`): every room whose box meets the
/// eraser's cube (`eraserpos` ± `eraserbgdist`), in room order, ranked by the
/// distance of its centre in metres; the camera's room is always on screen.
pub fn bg_tick_portals_xray(rooms: &BgRooms, flags: &[u8], cam: &PortalCam, cam_room: usize, eraserpos: Vec3, eraserbgdist: f32) -> PortalView {
    let n = rooms.roomcount();
    let full = cam.full_box();
    let mut view = PortalView { drawslots: Vec::new(), roomflags: vec![0; n], fullbox: full };
    let vismax = eraserpos + Vec3::splat(eraserbgdist);
    let vismin = eraserpos - Vec3::splat(eraserbgdist);
    for i in 1..n {
        let r = &rooms.rooms[i];
        let meets = !(vismax.x < r.bbmin.x) && !(vismin.x > r.bbmax.x) && !(vismax.z < r.bbmin.z) && !(vismin.z > r.bbmax.z) && !(vismax.y < r.bbmin.y) && !(vismin.y > r.bbmax.y);
        if meets && view.drawslots.len() < 60 {
            view.roomflags[i] |= ROOMFLAG_ONSCREEN;
            let d = (r.bbmin + r.bbmax) / 2.0 - eraserpos;
            let draworder = ((d.x * d.x + d.y * d.y + d.z * d.z).sqrt() / 100.0) as u32 as u8;
            view.drawslots.push(DrawSlot { roomnum: i as u16, draworder, rect: full });
            view.roomflags[cam_room] |= ROOMFLAG_ONSCREEN;
        }
    }
    bg_choose_rooms_to_load(rooms, flags, &mut view);
    view
}

impl Tick<'_> {
    /// `bg_set_room_onscreen` (`bg.c:193`, NTSC 1.0+).
    fn bg_set_room_onscreen(&mut self, roomnum: usize, draworder: u8, rect: ScreenBox) {
        self.view.roomflags[roomnum] |= ROOMFLAG_ONSCREEN;
        if let Some(index) = self.slot_of[roomnum] {
            let s = &mut self.view.drawslots[index];
            if draworder > s.draworder {
                s.draworder = draworder;
            }
            s.rect.expand(&rect);
        } else {
            // Slot 59 is reused once 60 are taken (bg.c:231).
            let index = self.view.drawslots.len().min(59);
            let slot = DrawSlot { roomnum: roomnum as u16, draworder, rect };
            if index < self.view.drawslots.len() {
                self.view.drawslots[index] = slot;
            } else {
                self.view.drawslots.push(slot);
            }
            self.slot_of[roomnum] = Some(index);
            self.numattempted += 1;
            // `bg_unpause_props_in_room` and `bg_load_room`: props are never
            // paused here and every room is always loaded.
        }
    }

    /// `bg_add_to_queue` (`bg.c:5356`).
    fn bg_add_to_queue(&mut self, fromroomnum: i32, roomnum: usize, depth: u8, rect: &ScreenBox) {
        if depth >= 2 {
            self.recursion[roomnum] = self.recursion[roomnum].saturating_add(1);
            if self.recursion[roomnum] as usize > self.rooms.roomportalrecursionlimit {
                return;
            }
        }
        // `unk07` is always 1 here.
        if self.queuecount[roomnum] != 0 {
            let mut i = self.tail;
            while i != self.head {
                let item = &mut self.queue[i];
                if item.roomnum as usize == roomnum {
                    if let Some(j) = item.fromroomnums.iter().position(|&r| r == -1) {
                        item.screenbox.expand(rect);
                        item.fromroomnums[j] = fromroomnum;
                        return;
                    }
                }
                i = (i + 1) % QUEUE_LEN;
            }
        }
        self.queue[self.head] = QueueItem { roomnum: roomnum as u16, fromroomnums: [fromroomnum, -1, -1, -1, -1], depth, screenbox: *rect };
        self.queuecount[roomnum] += 1;
        self.head += 1;
        if self.head == QUEUE_LEN {
            self.head = 0;
        }
        if self.head == self.tail {
            // A full queue drops its newest item.
            self.head = if self.head == 0 { QUEUE_LEN - 1 } else { self.head - 1 };
        }
    }

    /// `bg_process_queue_item` (`bg.c:5437`).
    fn bg_process_queue_item(&mut self, item: &QueueItem) {
        let roomnum = item.roomnum as usize;
        self.queuecount[roomnum] = self.queuecount[roomnum].saturating_sub(1);
        let campos = self.cam.cam_pos;
        let mut prevvalidcount = 0;
        let mut prevfoundroom: i32 = -1;
        let mut prevbox = ScreenBox::default();
        let portals = self.rooms.rooms[roomnum].portals.clone();
        for portalnum in portals {
            let p = &self.rooms.portals[portalnum];
            let side = *self.side[portalnum].get_or_insert_with(|| {
                let m = &p.metric;
                let sum = m.normal.x * campos.x + m.normal.y * campos.y + m.normal.z * campos.z;
                if sum < m.min {
                    1
                } else if sum > m.max {
                    0
                } else {
                    2
                }
            });
            // Skip a portal whose other room is on the camera's side.
            let newfoundroom = if p.room1 as usize == roomnum {
                if side == 0 {
                    continue;
                }
                p.room2 as i32
            } else {
                if side == 1 {
                    continue;
                }
                p.room1 as i32
            };
            if prevfoundroom != newfoundroom {
                if prevvalidcount != 0 {
                    self.bg_set_room_onscreen(prevfoundroom as usize, item.depth, prevbox);
                    self.bg_add_to_queue(roomnum as i32, prevfoundroom as usize, item.depth + 1, &prevbox);
                }
                prevvalidcount = 0;
                prevfoundroom = newfoundroom;
            }
            if portal_is_closed(self.flags[portalnum]) {
                continue;
            }
            if item.fromroomnums.contains(&newfoundroom) {
                continue;
            }
            let (valid, mut newbox) = if self.flags[portalnum] & PORTALFLAG_USEROOMBOX != 0 {
                (true, item.screenbox)
            } else {
                self.bg_get_portal_screen_bbox(portalnum)
            };
            if valid {
                newbox.intersect(&item.screenbox);
                if newbox.xmin < newbox.xmax && newbox.ymin < newbox.ymax {
                    if prevvalidcount == 0 {
                        prevbox = newbox;
                    } else {
                        prevbox.expand(&newbox);
                    }
                    prevvalidcount += 1;
                }
            }
        }
        if prevvalidcount != 0 {
            self.bg_set_room_onscreen(prevfoundroom as usize, item.depth, prevbox);
            self.bg_add_to_queue(roomnum as i32, prevfoundroom as usize, item.depth + 1, &prevbox);
        }
    }

    /// `bg_get_portal_screen_bbox` (`bg.c:2353`) over
    /// `portal_convert_coordinates` (`portalconv_c.c:14`, the near-plane clip).
    fn bg_get_portal_screen_bbox(&mut self, portalnum: usize) -> (bool, ScreenBox) {
        if let Some(b) = self.bbox[portalnum] {
            return b;
        }
        let pts = portal_convert_coordinates(&self.rooms.portals[portalnum].verts, &self.cam.world_to_screen);
        let mut numvalid = 0;
        let mut lo = [0.0f32; 2];
        let mut hi = [0.0f32; 2];
        for c in pts {
            if c.z <= 0.0 {
                let s = self.cam.project(c);
                if numvalid == 0 {
                    lo = s;
                    hi = s;
                } else {
                    if s[0] < lo[0] {
                        lo[0] = s[0];
                    }
                    if hi[0] < s[0] {
                        hi[0] = s[0];
                    }
                    if s[1] < lo[1] {
                        lo[1] = s[1];
                    }
                    if hi[1] < s[1] {
                        hi[1] = s[1];
                    }
                }
                numvalid += 1;
            }
        }
        let rect = if numvalid == 0 {
            ScreenBox::default()
        } else if hi[0] < lo[0] || hi[1] < lo[1] {
            self.view.fullbox
        } else {
            let c = |v: f32| -> i16 {
                if v >= 0.0 {
                    if v > 32000.0 {
                        32000
                    } else {
                        v as i16
                    }
                } else if v < -32000.0 {
                    -32000
                } else {
                    v as i16
                }
            };
            ScreenBox { xmin: c(lo[0] - 0.5), ymin: c(lo[1] - 0.5), xmax: c(hi[0] + 0.5), ymax: c(hi[1] + 0.5) }
        };
        let out = (numvalid != 0, rect);
        self.bbox[portalnum] = Some(out);
        out
    }
}

/// `portal_convert_coordinates` (`portalconv_c.c:14`): the portal in camera
/// space, clipped to the part in front of the camera (z < 0) when some of it
/// is behind.
fn portal_convert_coordinates(verts: &[Vec3], mtx: &Mat4) -> Vec<Vec3> {
    // PD walks the vertices backwards from things[39]; `right[k]` is vertex
    // (count - 1 - k) then, and right[count] repeats vertex 0 (things[39]).
    let n = verts.len();
    let conv: Vec<(Vec3, bool)> = verts
        .iter()
        .map(|&v| {
            let c = mtx.x_axis.truncate() * v.x + mtx.y_axis.truncate() * v.y + mtx.z_axis.truncate() * v.z + mtx.w_axis.truncate();
            (c, c.z >= 0.0)
        })
        .collect();
    if !conv.iter().any(|&(_, behind)| behind) {
        return conv.into_iter().map(|(c, _)| c).collect();
    }
    // things[39 - i] = vertex i; right starts at things[39 - count] = a copy of
    // vertex 0, and steps up: right[0] is the copy, right[1] vertex count-1, ...
    let mut ring: Vec<(Vec3, bool)> = Vec::with_capacity(n + 1);
    ring.push(conv[0]);
    for i in (0..n).rev() {
        ring.push(conv[i]);
    }
    let mut out = Vec::new();
    for i in 0..n {
        let (r0, b0) = ring[i];
        let (r1, b1) = ring[i + 1];
        let value = (b1 as u8) * 2 + b0 as u8;
        let cut = || {
            let mult = -r0.z / (r1.z - r0.z);
            Vec3::new((r1.x - r0.x) * mult + r0.x, (r1.y - r0.y) * mult + r0.y, 0.0)
        };
        match value {
            0 => out.push(r1),
            1 => {
                out.push(r1);
                out.push(cut());
            }
            2 => out.push(cut()),
            _ => {}
        }
    }
    out
}

/// `bg_room_intersects_screen_box` (`bg.c:2249`): some corner of the room's box
/// is on the screen box's side of every edge, not all behind or past far.
fn bg_room_intersects_screen_box(rooms: &BgRooms, cam: &PortalCam, room: usize, screen: &ScreenBox) -> bool {
    let r = &rooms.rooms[room];
    let (mut numbehind, mut numfar, mut numleft, mut numright, mut numbelow, mut numabove) = (0, 0, 0, 0, 0, 0);
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 != 0 { r.bbmin.x } else { r.bbmax.x },
            if i & 2 != 0 { r.bbmin.y } else { r.bbmax.y },
            if i & 4 != 0 { r.bbmin.z } else { r.bbmax.z },
        );
        // bg_3d_pos_to_2d_pos (bg.c:2335).
        let c = cam.world_to_screen.transform_point3(corner);
        let s = cam.project(c);
        let (x, y) = (s[0], s[1]);
        if cam.zfar <= -c.z {
            numfar += 1;
        }
        if c.z > 0.0 {
            if x > screen.xmin as f32 {
                numleft += 1;
            }
            if x < screen.xmax as f32 {
                numright += 1;
            }
            if y > screen.ymin as f32 {
                numbelow += 1;
            }
            if y < screen.ymax as f32 {
                numabove += 1;
            }
            numbehind += 1;
        } else {
            if x < screen.xmin as f32 {
                numleft += 1;
            } else if x > screen.xmax as f32 {
                numright += 1;
            }
            if y < screen.ymin as f32 {
                numbelow += 1;
            } else if y > screen.ymax as f32 {
                numabove += 1;
            }
        }
    }
    !(numbehind == 8 || numfar == 8 || numleft == 8 || numright == 8 || numbelow == 8 || numabove == 8)
}

/// `bg_choose_rooms_to_load` (`bg.c:5602`)'s standby half: an off-screen room
/// across a portal from an on-screen one is on standby. (Loading is moot:
/// every room is always loaded.)
fn bg_choose_rooms_to_load(rooms: &BgRooms, flags: &[u8], view: &mut PortalView) {
    for (i, p) in rooms.portals.iter().enumerate() {
        if flags[i] & PORTALFLAG_SKIP != 0 {
            continue;
        }
        let (r1, r2) = (p.room1 as usize, p.room2 as usize);
        let on = |r: usize| view.roomflags[r] & ROOMFLAG_ONSCREEN != 0;
        if on(r1) && !on(r2) {
            view.roomflags[r2] |= ROOMFLAG_STANDBY;
        } else if on(r2) && !on(r1) {
            view.roomflags[r1] |= ROOMFLAG_STANDBY;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pd_core::assets::AssetDir;

    fn cam_at(eye: Vec3, look: Vec3) -> PortalCam {
        let mut c = crate::player::camera::Camera::default();
        c.player_allocate_matrices(eye, look, Vec3::Y);
        PortalCam {
            world_to_screen: c.world_to_screen,
            cam_pos: eye,
            c_screenleft: c.c_screenleft,
            c_screentop: c.c_screentop,
            c_halfwidth: c.c_halfwidth,
            c_halfheight: c.c_halfheight,
            c_recipscalex: 1.0 / c.c_scalex,
            c_recipscaley: 1.0 / c.c_scaley,
            view: PortalCam::screen_properties(0.0, 0.0, 320.0, 220.0, 320.0, 220.0),
            zfar: 10000.0,
        }
    }

    /// On Complex, from every spawn pad looking along it: the camera's room is
    /// on screen first with the whole view, every slot's box lies inside the
    /// view, and closing every portal leaves only the camera's room.
    #[test]
    fn complex_spawns_see_through_their_portals() {
        let a = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
        let stage = crate::stage::Stage::load(&a, "ref").unwrap();
        let rooms = &stage.rooms;
        let open = rooms.initial_portal_flags();
        let closed: Vec<u8> = open.iter().map(|f| f | super::super::rooms::PORTALFLAG_CLOSED).collect();
        let mut most = 0;
        for &pad in &stage.spawn_pads {
            let p = &stage.pads[pad];
            let eye = p.pos + Vec3::Y * 106.0;
            let (inrooms, _, _) = rooms.bg_find_rooms_by_pos(eye, 20);
            let room = inrooms.iter().copied().find(|&r| rooms.bg_test_pos_in_room(eye, r as usize)).expect("a room") as usize;
            let cam = cam_at(eye, Vec3::new(p.look.x, 0.0, p.look.z).normalize());
            let v = bg_tick_portals(rooms, &open, &cam, room);
            assert_eq!(v.drawslots[0].roomnum as usize, room);
            assert_eq!(v.drawslots[0].rect, ScreenBox { xmin: 0, ymin: 0, xmax: 320, ymax: 220 });
            for s in &v.drawslots {
                assert!(s.rect.xmin >= 0 && s.rect.xmax <= 320 && s.rect.ymin >= 0 && s.rect.ymax <= 220 && s.rect.xmin < s.rect.xmax, "{s:?}");
                assert!(v.is_onscreen(s.roomnum as usize));
            }
            most = most.max(v.drawslots.len());
            let shut = bg_tick_portals(rooms, &closed, &cam, room);
            assert_eq!(shut.drawslots.len(), 1);
            assert!(shut.roomflags.iter().enumerate().any(|(r, f)| r != room && f & ROOMFLAG_STANDBY != 0));
        }
        assert!(most >= 5, "the best spawn sees {most} rooms");
    }

    /// A portal wholly in front is unchanged; one that straddles the camera
    /// plane is cut at z = 0.
    #[test]
    fn a_portal_behind_the_camera_is_clipped_at_the_near_plane() {
        let m = Mat4::IDENTITY;
        let front = [Vec3::new(-1.0, -1.0, -5.0), Vec3::new(1.0, -1.0, -5.0), Vec3::new(1.0, 1.0, -5.0), Vec3::new(-1.0, 1.0, -5.0)];
        assert_eq!(portal_convert_coordinates(&front, &m).len(), 4);
        let straddle = [Vec3::new(-1.0, 0.0, -5.0), Vec3::new(1.0, 0.0, -5.0), Vec3::new(1.0, 0.0, 5.0), Vec3::new(-1.0, 0.0, 5.0)];
        let c = portal_convert_coordinates(&straddle, &m);
        assert_eq!(c.len(), 4);
        assert!(c.iter().all(|p| p.z <= 0.0));
        assert_eq!(c.iter().filter(|p| p.z == 0.0).count(), 2);
    }
}

