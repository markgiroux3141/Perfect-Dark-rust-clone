//! Doors (`struct doorobj`, `propobj.c`): made from the setup's `door()`
//! (`setup_create_door`, `setup.c:944`), each ticked once a frame
//! (`door_tick`, `propobj.c:7665`): the auto-close timer, siblings, the
//! opening fraction with its acceleration (`apply_speed`), held back while
//! something stands in the door's way (`doors_calc_frac`), the door's position
//! and collision block at that fraction (`door_update_tiles`), its sounds and
//! the portal it closes while shut.
//!
//! Opened by a player's use button (`current_player_interact` →
//! `door_test_for_interact`, `propdoor_interact`), by walking at an automatic
//! one (`doors_check_automatic`), and by simulants in their way
//! (`chr_open_door`, `chraction.c:12261`).
//!
//! The arenas' doors slide (`DOORTYPE_SLIDING`, `_VERTICAL`) or swing
//! (`DOORTYPE_SWINGING`: Felicity's). A custom level's doors (`pd_edit`, any
//! of PD's and GoldenEye's door models) may also be the types PD kept from
//! GoldenEye: the flexi doors (slid as sliding ones), the Attack Ship's hull
//! doors and the Aztec chair (turned about their pad's normal or the world's
//! z axis), and Caverns' eyelid and iris doors (posed by `door_init_matrices`).
//! Fall-away doors (a hatch that drops once opened, as a projectile) and lasers
//! are not ported.

use glam::{Mat3, Mat4, Vec2, Vec3};
use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::math::{self, baddtor, M_BADPI};

use super::autogun::apply_speed;
use super::setup::obj_populate_geoblock_vertices_from_bbox_and_mtx;
use super::{Bbox, Obj};
use crate::stage::rooms::PORTALFLAG_CLOSED;
use crate::stage::{Pad, PropGeo};
use crate::world::World;

/// `DOORFLAG_*` (`constants.h:807`).
pub const DOORFLAG_EXTENDEDY: u16 = 0x0001;
pub const DOORFLAG_WINDOWED: u16 = 0x0002;
pub const DOORFLAG_0004: u16 = 0x0004;
pub const DOORFLAG_FLIP: u16 = 0x0008;
pub const DOORFLAG_AUTOMATIC: u16 = 0x0010;
pub const DOORFLAG_REUSEGEO: u16 = 0x0020;
pub const DOORFLAG_ROTATEDPAD: u16 = 0x0040;
pub const DOORFLAG_TRANSLATION: u16 = 0x0080;
pub const DOORFLAG_0100: u16 = 0x0100;
pub const DOORFLAG_LONGRANGE: u16 = 0x0200;
pub const DOORFLAG_UNBLOCKABLEOPEN: u16 = 0x0800;

/// `DOORMODE_*` (`constants.h:821`).
pub const DOORMODE_IDLE: u8 = 0;
pub const DOORMODE_OPENING: u8 = 1;
pub const DOORMODE_CLOSING: u8 = 2;
pub const DOORMODE_WAITING: u8 = 3;

/// `DOORTYPE_*` (`constants.h:832`).
pub const DOORTYPE_SLIDING: u16 = 0;
pub const DOORTYPE_FLEXI1: u16 = 1;
pub const DOORTYPE_FLEXI2: u16 = 2;
pub const DOORTYPE_FLEXI3: u16 = 3;
pub const DOORTYPE_VERTICAL: u16 = 4;
pub const DOORTYPE_SWINGING: u16 = 5;
pub const DOORTYPE_EYE: u16 = 6;
pub const DOORTYPE_IRIS: u16 = 7;
pub const DOORTYPE_FALLAWAY: u16 = 8;
pub const DOORTYPE_AZTECCHAIR: u16 = 9;
pub const DOORTYPE_HULL: u16 = 10;
pub const DOORTYPE_LASER: u16 = 11;

/// The handles a door's sounds play on: four per door (`PSTYPE_DOOR`), the
/// three `door_play` restarts and the iris door's blades' one.
const DOOR_HANDLE_BASE: u32 = 0x4000_0000;

/// `struct doorobj`'s own fields.
#[derive(Clone, Debug)]
pub struct Door {
    /// The door's pad, as `pad_unpack` gives it (rotated for a
    /// `DOORFLAG_ROTATEDPAD` door, `pad_rotate_for_door`).
    pub pad: Pad,
    pub maxfrac: f32,
    pub perimfrac: f32,
    pub accel: f32,
    pub decel: f32,
    pub maxspeed: f32,
    pub doorflags: u16,
    pub doortype: u16,
    pub keyflags: u32,
    pub autoclosetime: i32,
    /// 0 closed .. `maxfrac` open (a swinging door's is in degrees).
    pub frac: f32,
    pub fracspeed: f32,
    pub mode: u8,
    pub xludist: i16,
    pub opadist: i16,
    pub startpos: Vec3,
    pub slidedist: Vec3,
    /// A rotating door's rotation when closed.
    pub rotmtx: Mat3,
    /// The next door of its sibling ring, by object id.
    pub sibling: Option<u32>,
    /// The setup's relative command number of the sibling (resolved after
    /// every door exists).
    pub siblingcmd: i32,
    pub lastopen60: i32,
    pub portalnum: Option<usize>,
    pub soundtype: u8,
    pub fadetime60: i32,
    /// A `DOORFLAG_WINDOWED` door's glass opacity from the last player pass
    /// (`door_update_portal_if_windowed`), 0..255.
    pub fadealpha: i32,
    pub lastcalc60: i32,
    /// `lastcalc60` doubles as the frac before a blocked move (`doors_calc_frac`).
    savedfrac: f32,
    /// The block last worked out (`door->base.geoblock`, kept when the door
    /// opens past `perimfrac` and when `DOORFLAG_REUSEGEO`).
    pub geo: PropGeo,
    /// `prop->rooms`: the pad's room and those the door's box reaches through
    /// the portals (`obj_detect_rooms`, once, closed).
    pub rooms: Vec<u16>,
}

/// `pos_is_within_padbbox` (`propobj.c:702`): within the pad's box grown by
/// `padding` on each of its axes.
pub fn pos_is_within_padding_of_padvol(pos: Vec3, padding: Vec3, pad: &Pad) -> bool {
    let d = pos - pad.pos;
    let b = &pad.bbox;
    let f0 = d.dot(pad.look);
    if f0 > padding.z + b[5] || f0 < b[4] - padding.z {
        return false;
    }
    let f0 = d.dot(pad.up);
    if f0 > padding.y + b[3] || f0 < b[2] - padding.y {
        return false;
    }
    let f0 = d.dot(pad.normal());
    if f0 > padding.x + b[1] || f0 < b[0] - padding.x {
        return false;
    }
    true
}

impl Door {
    /// `door_get_bbox` (`propobj.c:18157`): a `DOORFLAG_0004` door's box shrinks
    /// as it opens (the part that slid into the frame).
    fn door_get_bbox(&self, b: &Bbox) -> Bbox {
        let mut d = *b;
        if self.doorflags & DOORFLAG_0004 != 0 {
            if self.doortype == DOORTYPE_VERTICAL {
                d.ymax = b.ymax + (b.ymin - b.ymax) * self.frac;
            } else {
                d.xmin = b.xmin + (b.xmax - b.xmin) * self.frac;
            }
        }
        d
    }

    /// `pos_is_in_front_of_door` (`propobj.c:19710`).
    pub fn pos_is_in_front_of_door(&self, pos: Vec3) -> bool {
        let mut value = (pos - self.pad.pos).dot(self.pad.normal());
        if self.doorflags & DOORFLAG_FLIP != 0 {
            value = -value;
        }
        value >= 0.0
    }

    /// `vector_is_in_front_of_door` (`propobj.c:18034`).
    fn vector_is_in_front_of_door(&self, v: Vec3) -> bool {
        let r = v.dot(self.pad.normal()) >= 0.0;
        if self.doorflags & DOORFLAG_FLIP != 0 {
            !r
        } else {
            r
        }
    }

    /// `door_is_pos_in_range` (`propobj.c:17970`): 2 m (4 m long range) out
    /// along the pad's normal.
    pub fn door_is_pos_in_range(&self, pos: Vec3, distance: f32) -> bool {
        let distance = distance + if self.doorflags & DOORFLAG_LONGRANGE != 0 { 400.0 } else { 200.0 };
        matches!(self.doortype, DOORTYPE_VERTICAL | DOORTYPE_SLIDING | DOORTYPE_SWINGING) && pos_is_within_padding_of_padvol(pos, Vec3::new(distance, 0.0, 0.0), &self.pad)
    }

    /// `door_is_closed` / `door_is_open` (`propobj.c:18905`).
    pub fn is_closed(&self) -> bool {
        matches!(self.mode, DOORMODE_IDLE | DOORMODE_WAITING) && self.frac <= 0.0
    }

    pub fn is_open(&self) -> bool {
        matches!(self.mode, DOORMODE_IDLE | DOORMODE_WAITING) && self.frac >= self.maxfrac
    }

    /// `prop_door_get_cd_types` (`prop.c:2768`).
    pub fn cd_types(&self, flags2: u32) -> u32 {
        let mut t = if self.frac <= 0.0 {
            CDTYPE_CLOSEDDOORS
        } else if self.frac >= self.maxfrac {
            CDTYPE_OPENDOORS
        } else {
            CDTYPE_AJARDOORS
        };
        if flags2 & OBJFLAG2_AICANNOTUSE != 0 {
            t |= CDTYPE_DOORSLOCKEDTOAI;
        }
        t
    }
}

/// A door's display-list batches, each with its vertices' positions and UVs.
pub type DoorVerts = Vec<(usize, Vec<([f32; 3], [f32; 2])>)>;

/// `door_find_dl_node` (`propobj.c:1243`): the model's first display-list node.
fn door_find_dl_node(def: &pd_core::model::ModelDef) -> Option<usize> {
    def.nodes.iter().position(|n| matches!(n.kind, pd_core::model::NodeKind::Dl { .. }))
}

/// `door_calc_texturemap` (`propobj.c:18299`), as `door_calc_vertices_*` apply
/// it to a sliding `DOORFLAG_0004` door each frame: in the display list's
/// vertex table, taken four at a time, a vertex past the door's shrunk box
/// (`door_get_bbox`: above its top for a vertical door, left of its left
/// side otherwise) is moved onto the box's edge, its `s, t` slid along the
/// quad's edge to the vertex that differs only on that axis, so the door looks
/// to retract into its frame rather than slide through the wall. Returns each
/// of that node's batches with its vertices' positions and UVs (`s × scale >> 16`,
/// in texels / 32), or nothing for any other door.
pub fn door_calc_texturemap(o: &Obj, d: &Door) -> DoorVerts {
    if d.doorflags & (DOORFLAG_0004 | DOORFLAG_TRANSLATION) != (DOORFLAG_0004 | DOORFLAG_TRANSLATION) {
        return Vec::new();
    }
    let Some(node) = door_find_dl_node(&o.def) else { return Vec::new() };
    let batches: Vec<usize> = o.def.nodes[node].batches.iter().copied().filter(|&b| !o.def.batches[b].vsrc.is_empty()).collect();
    // The node's vertex table: (x, y, z, s, t) by index.
    let n = batches.iter().flat_map(|&b| o.def.batches[b].vsrc.iter()).map(|&v| v as usize + 1).max().unwrap_or(0);
    let mut src = vec![[0i32; 5]; n];
    for &b in &batches {
        let bt = &o.def.batches[b];
        for (k, &v) in bt.vsrc.iter().enumerate() {
            let p = bt.verts[k].pos;
            src[v as usize] = [p.x as i32, p.y as i32, p.z as i32, bt.st[k][0] as i32, bt.st[k][1] as i32];
        }
    }
    let bbox = d.door_get_bbox(&o.bbox);
    let vertical = d.doortype == DOORTYPE_VERTICAL;
    let reference = if vertical { bbox.ymax.ceil() as i32 } else { bbox.xmin.floor() as i32 };
    let mut dst = src.clone();
    // (the axis clipped, the other two)
    let (a, o1, o2) = if vertical { (1, 0, 2) } else { (0, 1, 2) };
    for q in 0..n / 4 {
        let ps = &src[q * 4..q * 4 + 4];
        for j in 0..4 {
            let past = if vertical { ps[j][a] >= reference } else { ps[j][a] <= reference };
            if !past {
                continue;
            }
            let pd = &mut dst[q * 4 + j];
            for k in 1..4 {
                let nx = ps[(j + k) % 4];
                if nx[o1] == ps[j][o1] && nx[o2] == ps[j][o2] && nx[a] != ps[j][a] {
                    // (y − ref) / (y − next.y) vertically, (ref − x) / (next.x − x) across.
                    let (num, den) = if vertical { (ps[j][a] - reference, ps[j][a] - nx[a]) } else { (reference - ps[j][a], nx[a] - ps[j][a]) };
                    pd[3] = (ps[j][3] + num * (nx[3] - ps[j][3]) / den) as i16 as i32;
                    pd[4] = (ps[j][4] + num * (nx[4] - ps[j][4]) / den) as i16 as i32;
                    break;
                }
            }
            pd[a] = reference;
        }
    }
    batches
        .iter()
        .map(|&b| {
            let bt = &o.def.batches[b];
            let verts = bt
                .vsrc
                .iter()
                .enumerate()
                .map(|(k, &v)| {
                    let t = dst[v as usize];
                    // fast3d's U = s × scale >> 16, wrapped to s16 (as the exporter).
                    let uv = |st: i32, scale: u16| (((st * scale as i32) >> 16) as i16) as f32 / 32.0;
                    let [su, sv] = bt.stscale[k];
                    let texgen = bt.verts[k].flags & 2 != 0;
                    let uvs = if texgen { bt.verts[k].uv } else { [uv(t[3], su), uv(t[4], sv)] };
                    ([t[0] as f32, t[1] as f32, t[2] as f32], uvs)
                })
                .collect();
            (b, verts)
        })
        .collect()
}

/// How many frames (60ths) a shut door takes to open, its fraction moved by
/// `apply_speed` a frame at a time as `door_calc_intended_frac` moves it
/// (the setup's integers: `maxfrac` 16.16, `accel`/`decel` / 65536000,
/// `maxspeed` / 65536). The level editor's readout, not PD's; `None` if it
/// never gets there in a minute.
pub fn door_open_frames(maxfrac: i32, accel: i32, decel: i32, maxspeed: i32) -> Option<u32> {
    let mut lv = pd_core::lv::Lv::new();
    lv.lvupdate60 = 1;
    let (end, accel, decel, maxspeed) = (maxfrac as f32 / 65536.0, accel as f32 / 65_536_000.0, decel as f32 / 65_536_000.0, maxspeed as f32 / 65536.0);
    let (mut frac, mut speed) = (0.0f32, 0.0f32);
    (1..=3600).find(|_| {
        apply_speed(&lv, &mut frac, end, &mut speed, accel, decel, maxspeed);
        frac >= end
    })
}

/// `door_get_mtx` (`propobj.c:18147`): the rotation and position, the z axis
/// mirrored for a `DOORFLAG_FLIP` door.
pub fn door_get_mtx(o: &Obj, d: &Door) -> Mat4 {
    let mut m = Mat4::from_cols(o.realrot.x_axis.extend(0.0), o.realrot.y_axis.extend(0.0), o.realrot.z_axis.extend(0.0), o.pos.extend(1.0));
    if d.doorflags & DOORFLAG_FLIP != 0 {
        m.z_axis = -m.z_axis;
    }
    m
}

/// `door_init_matrices` (`propobj.c:7843`) past matrix 0 (`door_get_mtx`,
/// world space here: the renderer puts the camera in front). An eyelid door
/// (`g_Skel11`, GoldenEye's Caverns) turns its two lids, parts 1 and 2, about
/// x in opposite ways by the fraction (in degrees); an iris door (`g_Skel13`)
/// turns six blades, each an outer part (1, 3, .. 11) that follows once the
/// door is 0.3 open and an inner one (2, 4, .. 12) turned by the fraction
/// within it. Each matrix is its part's number, as PD indexes them.
pub fn door_init_matrices(def: &pd_core::model::ModelDef, d: &Door, mats: &mut [Mat4]) {
    let pos = |part: i32| -> Option<Vec3> {
        match def.nodes[def.get_part(part)?].kind {
            pd_core::model::NodeKind::Position { pos, .. } => Some(pos),
            _ => None,
        }
    };
    let set = |mats: &mut [Mat4], i: usize, parent: usize, mut m: Mat4, at: Vec3| {
        math::set_translation(&mut m, at);
        if i < mats.len() && parent < mats.len() {
            mats[i] = math::mul(&mats[parent], &m);
        }
    };
    if def.skel == SKEL_11 {
        let xrot = baddtor(360.0) - math::baddtor2(d.frac);
        if let Some(p) = pos(MODELPART_0001) {
            set(mats, 1, 0, math::mtx4_load_x_rotation(xrot), p);
        }
        if let Some(p) = pos(MODELPART_0002) {
            set(mats, 2, 0, math::mtx4_load_x_rotation(baddtor(360.0) - xrot), p);
        }
    } else if def.skel == SKEL_13 {
        let zrot2 = math::baddtor2(d.frac);
        let limit = d.maxfrac * 0.3;
        let zrot1 = if d.frac > limit { math::baddtor2((d.maxfrac * (d.frac - limit)) / (d.maxfrac - limit)) } else { 0.0 };
        for i in 0..6 {
            let (i1, i2) = ((i << 1) + 1, (i << 1) + 2);
            if let Some(p) = pos(i1 as i32) {
                set(mats, i1, 0, math::mtx4_load_z_rotation(zrot1), p);
            }
            if let Some(p) = pos(i2 as i32) {
                set(mats, i2, i1, math::mtx4_load_z_rotation(zrot2), p);
            }
        }
    }
}

/// `door_update_tiles` (`propobj.c:18172`): the door's place at its fraction
/// (slid along `slidedist`, or a rotating door turned about its hinge: a
/// swinging door about the vertical, the Aztec chair about the world's z, a
/// hull door about its pad's normal) and its collision block, gone once it is
/// open past `perimfrac`.
pub fn door_update_tiles(o: &mut Obj) {
    let Some(d) = o.door.as_mut() else { return };
    if d.doorflags & DOORFLAG_TRANSLATION != 0 {
        o.pos = d.slidedist * d.frac + d.startpos;
    } else if matches!(d.doortype, DOORTYPE_SWINGING | DOORTYPE_AZTECCHAIR | DOORTYPE_HULL) {
        let pad = &d.pad;
        let n = pad.normal();
        let tofront = o.flags & OBJFLAG_DOOR_OPENTOFRONT != 0;
        let mut sp8c = pad.pos + pad.up * pad.bbox[2];
        sp8c += match d.doortype {
            DOORTYPE_AZTECCHAIR => n * pad.bbox[1],
            // `// SUBST:` PD reads the pad's look without unpacking it (its
            // @bug, an uninitialised stack value) / the pad's look.
            DOORTYPE_HULL => pad.look * pad.bbox[4],
            _ => n * if tofront { pad.bbox[1] } else { pad.bbox[0] },
        };
        let sp80 = d.startpos - sp8c;
        let mut spdc = Mat4::from_mat3(d.rotmtx);
        spdc = Mat4::from_translation(sp80) * spdc;
        let angle = if tofront { baddtor(360.0) - d.frac * baddtor(1.0) } else { d.frac * baddtor(1.0) };
        let rot = match d.doortype {
            DOORTYPE_AZTECCHAIR => math::mtx4_load_z_rotation(angle),
            DOORTYPE_HULL => math::gu_rotate_f(if tofront { 360.0 - d.frac } else { d.frac }, n.x, n.y, n.z),
            _ => math::mtx4_load_y_rotation(angle),
        };
        spdc = rot * spdc;
        spdc = Mat4::from_translation(sp8c) * spdc;
        o.realrot = Mat3::from_mat4(spdc);
        o.pos = spdc.w_axis.truncate();
    }
    let d = o.door.as_ref().unwrap();
    let bbox = d.door_get_bbox(&o.bbox);
    if d.frac >= d.perimfrac {
        o.hidden |= OBJHFLAG_DOORPERIMDISABLED;
        o.geos.clear();
        return;
    }
    o.hidden &= !OBJHFLAG_DOORPERIMDISABLED;
    let mut geo = d.geo;
    if d.doorflags & DOORFLAG_REUSEGEO == 0 {
        // obj_populate_geoblock_from_bbox_and_mtx (propobj.c:643).
        let m = door_get_mtx(o, d);
        let verts: Vec<Vec2> = obj_populate_geoblock_vertices_from_bbox_and_mtx(&bbox, &m);
        let (ymin, ymax) = (m.w_axis.y + rotated_local_min(&bbox, m.x_axis.y, m.y_axis.y, m.z_axis.y), m.w_axis.y + rotated_local_max(&bbox, m.x_axis.y, m.y_axis.y, m.z_axis.y));
        geo = PropGeo::block(&verts, ymin, ymax);
    }
    let d = o.door.as_mut().unwrap();
    if d.doorflags & DOORFLAG_REUSEGEO == 0 && d.doortype == DOORTYPE_VERTICAL {
        d.doorflags |= DOORFLAG_REUSEGEO;
    }
    if d.doortype == DOORTYPE_VERTICAL {
        geo.ymin = d.startpos.y + bbox.rotated_y_min(&o.realrot);
    } else if d.doortype == DOORTYPE_FALLAWAY {
        geo.ymin = o.pos.y - 10000.0;
    } else if d.doorflags & DOORFLAG_EXTENDEDY != 0 {
        geo.ymin -= 1000.0;
    }
    // An eyelid or iris door past 0.4 open keeps only a 50 cm sill.
    if matches!(d.doortype, DOORTYPE_EYE | DOORTYPE_IRIS) && d.frac > 0.4 * d.maxfrac {
        geo.ymax = geo.ymin + 50.0;
    } else if d.doortype == DOORTYPE_FALLAWAY {
        geo.ymax = o.pos.y + 1000.0;
    } else if d.doorflags & DOORFLAG_EXTENDEDY != 0 {
        geo.ymax += 1000.0;
    }
    d.geo = geo;
    o.geos = vec![geo];
}

fn rotated_local_min(b: &Bbox, a1: f32, a2: f32, a3: f32) -> f32 {
    (if a1 >= 0.0 { b.xmin * a1 } else { b.xmax * a1 }) + (if a2 >= 0.0 { b.ymin * a2 } else { b.ymax * a2 }) + (if a3 >= 0.0 { b.zmin * a3 } else { b.zmax * a3 })
}

fn rotated_local_max(b: &Bbox, a1: f32, a2: f32, a3: f32) -> f32 {
    (if a1 <= 0.0 { b.xmin * a1 } else { b.xmax * a1 }) + (if a2 <= 0.0 { b.ymin * a2 } else { b.ymax * a2 }) + (if a3 <= 0.0 { b.zmin * a3 } else { b.zmax * a3 })
}

/// The `SFXMAP_*` a door plays as it starts opening, starts closing, ends
/// open and ends closed (`door_play_opening_sound` .. `_closed_sound`,
/// `propobj.c:18485-18730`), for the sound types the arenas use (their
/// setups' `soundtype`: 1, 3, 4, 8, 10, 16, 18, and Felicity's lift's 22) and
/// the rest of PD's table up to 18.
pub(crate) fn door_sounds(soundtype: u8) -> ([u16; 3], [u16; 3], u16, u16) {
    match soundtype {
        28 => ([0x8007, 0, 0], [0x8007, 0, 0], 0x801a, 0x801a),
        1 => ([0x801a, 0x801b, 0], [0x801a, 0x801b, 0], 0x801a, 0x801a),
        29 => ([0x8015, 0x801d, 0], [0x8015, 0x801d, 0], 0x8015, 0x8015),
        2 => ([0x801a, 0x801c, 0], [0x801a, 0x801c, 0], 0x801a, 0x801a),
        3 => ([0x8014, 0x8016, 0], [0x8014, 0x8016, 0], 0x8015, 0x8015),
        4 => ([0x801e, 0x8020, 0], [0x801e, 0x8020, 0], 0x801f, 0x801f),
        5 => ([0x8001, 0, 0], [0x8001, 0, 0], 0x8002, 0x8002),
        6 => ([0x8004, 0, 0], [0, 0, 0], 0, 0x8003),
        7 => ([0x8005, 0, 0], [0, 0, 0], 0, 0x8006),
        8 => ([0x800a, 0x8008, 0], [0x800a, 0x8008, 0], 0x801a, 0x801a),
        9 => ([0x8004, 0x800b, 0], [0x8004, 0x800b, 0], 0x8003, 0x8003),
        10 => ([0x800c, 0, 0], [0x800c, 0, 0], 0x800d, 0x800d),
        11 => ([0x800e, 0, 0], [0, 0, 0], 0, 0x800f),
        12 => ([0x8010, 0, 0], [0, 0, 0], 0, 0x8011),
        13 => ([0x8012, 0, 0], [0, 0, 0], 0, 0x8013),
        14 => ([0x8017, 0x8019, 0], [0x8017, 0x8019, 0], 0x816d, 0x8018),
        15 => ([0x8022, 0, 0], [0x8022, 0, 0], 0x8021, 0x8021),
        16 => ([0x8026, 0, 0], [0x8026, 0, 0], 0x8027, 0x8027),
        17 => ([0x801e, 0, 0], [0x801e, 0, 0], 0x801f, 0x801f),
        18 => ([0x81b0, 0x8014, 0x8016], [0x81b0, 0x8014, 0x8016], 0x8015, 0x8015),
        22 => ([0x81ae, 0x81b5, 0], [0; 3], 0x81af, 0),
        _ => ([0; 3], [0; 3], 0, 0),
    }
}

impl World {
    /// `setup_create_door` (`setup.c:944`) for the setup's `door()` at
    /// `props[cmdindex]`: the door placed and sized on its pad (the model's
    /// box stretched to the pad's box: x by its height, y by its depth, z by
    /// its width), its portal found and closed while it is shut. Returns the
    /// object's id.
    pub(crate) fn setup_create_door(&mut self, p: &serde_json::Value) -> Option<u32> {
        let int = |k: &str| p.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let modelnum = int("model") as i32;
        let padnum = int("pad") as i32;
        let mut o = self.setup_obj(modelnum, OBJTYPE_DOOR, int("flags") as u32, int("flags2") as u32, int("flags3") as u32, int("maxdamage") as i32)?;
        let mut pad = self.stage.pads.get(usize::try_from(padnum).ok()?)?.clone();
        let doorflags = (int("doorflags") & 0xffff) as u16;
        if doorflags & DOORFLAG_ROTATEDPAD != 0 {
            // pad_rotate_for_door (pad.c:186).
            pad.up.y = 0.0;
            let s = 1.0 / (pad.up.x * pad.up.x + pad.up.z * pad.up.z).sqrt();
            pad.up.x *= s;
            pad.up.z *= s;
            pad.look = Vec3::Y;
        }
        let portalnum = if o.flags & OBJFLAG_DOOR_HASPORTAL != 0 { self.setup_get_portal_by_door_pad(padnum) } else { None };
        pad.room?;
        o.pad = padnum;
        let bbox = o.bbox;
        let sp110 = math::look_at_basis(Vec3::ZERO, -pad.look, pad.up);
        let finalmtx = sp110 * (math::mtx4_load_z_rotation(baddtor(90.0)) * math::mtx4_load_x_rotation(baddtor(90.0)));
        let centre = pad.centre();
        let b = &pad.bbox;
        let (mut xscale, mut yscale, mut zscale) = ((b[3] - b[2]) / (bbox.xmax - bbox.xmin), (b[5] - b[4]) / (bbox.ymax - bbox.ymin), (b[1] - b[0]) / (bbox.zmax - bbox.zmin));
        if xscale <= 0.000_001 || yscale <= 0.000_001 || zscale <= 0.000_001 {
            (xscale, yscale, zscale) = (1.0, 1.0, 1.0);
        }
        let mut finalmtx = finalmtx;
        finalmtx.x_axis *= xscale;
        finalmtx.y_axis *= yscale;
        finalmtx.z_axis *= zscale;
        let doortype = ((int("doortype") as u32) & 0xffff) as u16;
        let slidedist = if matches!(doortype, DOORTYPE_VERTICAL | DOORTYPE_FALLAWAY) { pad.look * (b[5] - b[4]) } else { pad.up * (b[2] - b[3]) };
        let unkc4 = int("unkc4") as u32;
        let unk88 = int("unk88") as u32;
        let mut d = Door {
            pad: pad.clone(),
            maxfrac: int("maxfrac") as i32 as f32 / 65536.0,
            perimfrac: int("perimfrac") as i32 as f32 / 65536.0,
            accel: int("accel") as i32 as f32 / 65_536_000.0,
            decel: int("decel") as i32 as f32 / 65_536_000.0,
            maxspeed: int("maxspeed") as i32 as f32 / 65536.0,
            doorflags,
            doortype,
            keyflags: int("keyflags") as u32,
            autoclosetime: int("autoclosetime") as i32,
            frac: 0.0,
            fracspeed: 0.0,
            mode: DOORMODE_IDLE,
            xludist: (unk88 >> 16) as i16,
            opadist: (unk88 & 0xffff) as i16,
            startpos: centre,
            slidedist: Vec3::ZERO,
            rotmtx: Mat3::IDENTITY,
            sibling: None,
            siblingcmd: int("sibling") as i32,
            lastopen60: 0,
            portalnum: None,
            soundtype: ((unkc4 >> 8) & 0xff) as u8,
            fadetime60: (unkc4 & 0xff) as i8 as i32,
            fadealpha: 255,
            lastcalc60: 0xff00_0000u32 as i32,
            savedfrac: 0.0,
            geo: PropGeo::default(),
            rooms: Vec::new(),
        };
        // door_init (propobj.c:18406).
        o.flags |= OBJFLAG_CORE_GEO_INUSE;
        if matches!(d.doortype, DOORTYPE_SLIDING | DOORTYPE_FLEXI1 | DOORTYPE_FLEXI2 | DOORTYPE_FLEXI3 | DOORTYPE_VERTICAL | DOORTYPE_FALLAWAY | DOORTYPE_LASER) {
            d.doorflags |= DOORFLAG_TRANSLATION;
        }
        let mut rotmtx = finalmtx;
        math::scale3(&mut rotmtx, o.scale);
        o.realrot = Mat3::from_mat4(rotmtx);
        d.frac = if o.flags & OBJFLAG_DOOR_KEEPOPEN != 0 { d.maxfrac } else { 0.0 };
        if d.doorflags & DOORFLAG_TRANSLATION != 0 {
            d.slidedist = slidedist;
        } else {
            d.rotmtx = o.realrot;
        }
        o.pos = centre;
        o.door = Some(Box::new(d));
        door_update_tiles(&mut o);
        let d = o.door.as_mut().unwrap();
        if o.flags & OBJFLAG_DOOR_HASPORTAL != 0 {
            d.portalnum = portalnum;
        }
        o.scale *= xscale.max(yscale).max(zscale);
        // obj_detect_rooms (propobj.c:2850): the rooms the door's box enters.
        // `// SUBST:` PD keeps them up to date as the door moves / worked out
        // once, shut.
        let g = d.geo;
        let lo = g.verts().iter().fold(Vec2::splat(f32::MAX), |a, v| a.min(*v));
        let hi = g.verts().iter().fold(Vec2::splat(f32::MIN), |a, v| a.max(*v));
        let mut rooms = vec![pad.room.unwrap()];
        self.stage.rooms.bg_find_entered_rooms(Vec3::new(lo.x, g.ymin, lo.y), Vec3::new(hi.x, g.ymax, hi.y), &mut rooms, 7, false, &self.portalflags);
        d.rooms = rooms;
        let (closeportal, pn) = (d.portalnum.is_some() && d.frac == 0.0, d.portalnum);
        let id = o.id;
        self.props.objs.push(o);
        if closeportal {
            self.bg_set_portal_open_state(pn.unwrap(), false);
        }
        Some(id)
    }

    /// `door_update_portal_if_windowed` (`propobj.c:7803`) for player `pi`'s
    /// pass: a windowed door's glass opacity from the player's camera
    /// (`glass_calculate_opacity`, clear within `xludist`, opaque past
    /// `opadist`), and its portal open while the glass can be seen through,
    /// the door is open, its glass is gone, or more than one player plays;
    /// shut otherwise.
    pub(crate) fn doors_update_portals_if_windowed(&mut self, pi: usize) {
        let campos = self.players[pi].cam.pos();
        let multi = self.players.len() >= 2;
        for i in 0..self.props.objs.len() {
            let o = &mut self.props.objs[i];
            let Some(d) = o.door.as_mut() else { continue };
            if d.doorflags & DOORFLAG_WINDOWED == 0 {
                continue;
            }
            d.fadealpha = super::glass::glass_calculate_opacity(o.pos, campos, d.xludist as f32, d.opadist as f32, 0.0);
            // The glass's toggle (MODELPART_WINDOWEDDOOR_0001) is never hidden:
            // no arena's glass breaks (its shot handling isn't ported).
            let canhide = d.fadealpha == 255 && d.frac <= 0.0 && !multi;
            if let Some(p) = d.portalnum {
                self.bg_set_portal_open_state(p, !canhide);
            }
        }
    }

    /// Every door opened (or closed) at once, as if used
    /// (`doors_request_mode`): the level editor's preview, not PD's.
    pub fn doors_preview(&mut self, open: bool) {
        for i in 0..self.props.objs.len() {
            if self.props.objs[i].door.is_some() {
                self.doors_request_mode(i, if open { DOORMODE_OPENING } else { DOORMODE_CLOSING });
            }
        }
    }

    /// `bg_set_portal_open_state` (`bg.c:6144`).
    pub(crate) fn bg_set_portal_open_state(&mut self, portal: usize, open: bool) {
        if let Some(f) = self.portalflags.get_mut(portal) {
            *f = (*f | PORTALFLAG_CLOSED) ^ (open as u8);
        }
    }

    /// The doors in a sibling ring, starting at `id`.
    fn door_ring(&self, id: u32) -> Vec<usize> {
        let mut out = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(i) = self.props.objs.iter().position(|o| o.id == c) else { break };
            if out.contains(&i) {
                break;
            }
            out.push(i);
            cur = self.props.objs[i].door.as_ref().and_then(|d| d.sibling);
        }
        out
    }

    fn door_mut(&mut self, i: usize) -> &mut Door {
        self.props.objs[i].door.as_mut().unwrap()
    }

    /// A door's sound on one of its three handles, at the door.
    fn door_sound(&mut self, i: usize, k: u32, sound: u16) {
        let o = &self.props.objs[i];
        let (id, pos) = (o.id, o.pos);
        let (volume, pan) = crate::propsnd::vol_pan(&self.res.audio, sound, pos, crate::propsnd::DEFAULT_DISTS, &self.listeners());
        self.push_event(Event::HandleSound { handle: DOOR_HANDLE_BASE + id * 4 + k, sound, pitch: 1.0, volume, pan });
    }

    /// `ps_stop_sound(prop, PSTYPE_DOOR, 0xffff)`.
    fn door_stop_sounds(&mut self, i: usize) {
        let id = self.props.objs[i].id;
        for k in 0..3 {
            self.push_event(Event::StopSound { handle: DOOR_HANDLE_BASE + id * 4 + k });
        }
    }

    pub(crate) fn door_play(&mut self, i: usize, sounds: [u16; 3]) {
        self.door_stop_sounds(i);
        for (k, s) in sounds.into_iter().enumerate() {
            if s != 0 {
                self.door_sound(i, k as u32, s);
            }
        }
    }

    /// `door_start_open` (`propobj.c:18730`).
    fn door_start_open(&mut self, i: usize) {
        let o = &mut self.props.objs[i];
        o.flags &= !OBJFLAG_DOOR_KEEPOPEN;
        o.hidden |= OBJHFLAG_DOOREVEROPENED;
        let d = o.door.as_mut().unwrap();
        let (open, _, _, _) = door_sounds(d.soundtype);
        let pn = d.portalnum;
        d.fadetime60 = 0;
        self.door_play(i, open);
        if let Some(p) = pn {
            self.bg_set_portal_open_state(p, true);
        }
    }

    /// `door_start_close` (`propobj.c:18760`).
    fn door_start_close(&mut self, i: usize) {
        let o = &mut self.props.objs[i];
        o.flags &= !OBJFLAG_DOOR_KEEPOPEN;
        let d = o.door.as_mut().unwrap();
        d.fadetime60 = 0;
        let (_, close, _, _) = door_sounds(d.soundtype);
        self.door_play(i, close);
    }

    /// `door_finish_open` (`propobj.c:18780`).
    fn door_finish_open(&mut self, i: usize) {
        let (_, _, opened, _) = door_sounds(self.door_mut(i).soundtype);
        self.door_play(i, [opened, 0, 0]);
    }

    /// `door_finish_close` (`propobj.c:18800`): the portal closes unless a
    /// sibling on the same portal is still open.
    fn door_finish_close(&mut self, i: usize) {
        let (_, _, _, closed) = door_sounds(self.door_mut(i).soundtype);
        self.door_play(i, [closed, 0, 0]);
        let id = self.props.objs[i].id;
        let pn = self.door_mut(i).portalnum;
        let pass = self.door_ring(id).iter().all(|&j| {
            let d = self.props.objs[j].door.as_ref().unwrap();
            !(d.frac > 0.0 && d.portalnum == pn)
        });
        if pass {
            if let Some(p) = pn {
                self.bg_set_portal_open_state(p, false);
            }
        }
    }

    /// `door_set_mode` (`propobj.c:18850`): one door, not its siblings.
    fn door_set_mode(&mut self, i: usize, newmode: u8) {
        let d = self.props.objs[i].door.as_ref().unwrap();
        let (mode, frac) = (d.mode, d.frac);
        match newmode {
            DOORMODE_OPENING => {
                if mode == DOORMODE_IDLE || mode == DOORMODE_WAITING {
                    self.door_start_open(i);
                }
                self.door_mut(i).mode = newmode;
            }
            DOORMODE_CLOSING => {
                if mode == DOORMODE_IDLE && frac > 0.0 {
                    self.door_start_close(i);
                }
                if (mode != DOORMODE_IDLE && mode != DOORMODE_WAITING) || frac > 0.0 {
                    self.door_mut(i).mode = newmode;
                } else if mode == DOORMODE_WAITING {
                    self.door_mut(i).mode = DOORMODE_IDLE;
                }
            }
            _ => self.door_mut(i).mode = newmode,
        }
    }

    /// `doors_request_mode` (`propobj.c:18881`): the door and its siblings (an
    /// airlock's siblings close while it waits for them).
    pub(crate) fn doors_request_mode(&mut self, i: usize, mut newmode: u8) {
        let mut siblingmode = newmode;
        if self.props.objs[i].flags2 & OBJFLAG2_AIRLOCKDOOR != 0 && newmode == DOORMODE_OPENING {
            siblingmode = DOORMODE_CLOSING;
            if self.door_mut(i).mode == DOORMODE_IDLE {
                newmode = DOORMODE_WAITING;
            }
        }
        self.door_set_mode(i, newmode);
        let id = self.props.objs[i].id;
        for j in self.door_ring(id).into_iter().skip(1) {
            self.door_set_mode(j, siblingmode);
        }
    }

    /// `doors_activate` (`propobj.c:19683`): an opening door closes, a closing
    /// one opens, one at rest goes the other way (by its fraction). A lift door
    /// calls its lift instead (`door_call_lift`, `props::lift`).
    pub(crate) fn doors_activate(&mut self, i: usize, allowliftclose: bool) {
        if !self.door_call_lift(i, allowliftclose) {
            let d = self.props.objs[i].door.as_ref().unwrap();
            let newmode = match d.mode {
                DOORMODE_OPENING | DOORMODE_WAITING => Some(DOORMODE_CLOSING),
                DOORMODE_CLOSING => Some(DOORMODE_OPENING),
                DOORMODE_IDLE => Some(if d.frac > 0.5 * d.maxfrac { DOORMODE_CLOSING } else { DOORMODE_OPENING }),
                _ => None,
            };
            if let Some(m) = newmode {
                self.doors_request_mode(i, m);
            }
        }
        self.props.objs[i].flags2 &= !OBJFLAG2_DOOR_PENDINGACTIVATION;
    }

    /// `doors_choose_swing_direction` (`propobj.c:19741`): a two-way door at
    /// rest swings away from `pos`, with its siblings.
    pub(crate) fn doors_choose_swing_direction(&mut self, pos: Vec3, i: usize) {
        let o = &self.props.objs[i];
        let d = o.door.as_ref().unwrap();
        if o.flags & OBJFLAG_DOOR_TWOWAY != 0 && d.mode == DOORMODE_IDLE && d.frac == 0.0 {
            let infront = d.pos_is_in_front_of_door(pos);
            let flip = d.doorflags & DOORFLAG_FLIP != 0;
            let wantflag = if infront == flip { OBJFLAG_DOOR_OPENTOFRONT } else { 0 };
            if (o.flags ^ wantflag) & OBJFLAG_DOOR_OPENTOFRONT != 0 {
                let id = o.id;
                for j in self.door_ring(id) {
                    self.props.objs[j].flags ^= OBJFLAG_DOOR_OPENTOFRONT;
                }
            }
        }
    }

    /// `door_is_range_empty` (`propobj.c:18053`): no chr within the door's
    /// opening range. `// SUBST:` PD asks the props in the door's rooms /
    /// every chr (the range is 2 m from the door, so the same ones).
    fn door_is_range_empty(&self, i: usize) -> bool {
        let d = self.props.objs[i].door.as_ref().unwrap();
        !self.chrs.iter().enumerate().any(|(ci, c)| {
            let pos = match c.player {
                Some(p) => self.players[p].pos,
                None => c.pos,
            };
            !c.is_dying_or_dead() && ci < self.chrs.len() && d.door_is_pos_in_range(pos, 0.0)
        })
    }

    /// `door_tick` (`propobj.c:7665`): once a frame, in the first player's
    /// `props_tick_player`.
    pub(crate) fn door_tick(&mut self, i: usize) {
        let lvframe60 = self.lv.lvframe60;
        let id = self.props.objs[i].id;
        let prevfrac = self.door_mut(i).frac;
        {
            let o = &self.props.objs[i];
            let d = o.door.as_ref().unwrap();
            if d.lastopen60 > 0 && d.mode == DOORMODE_IDLE && o.flags & OBJFLAG_DOOR_KEEPOPEN == 0 && d.lastopen60 < lvframe60 - d.autoclosetime {
                let ring = self.door_ring(id);
                let automatic = |j: usize| self.props.objs[j].door.as_ref().unwrap().doorflags & DOORFLAG_AUTOMATIC != 0;
                let pass = ring.iter().any(|&j| automatic(j));
                if !pass {
                    self.doors_request_mode(i, DOORMODE_CLOSING);
                } else if automatic(i) {
                    if ring.iter().any(|&j| !self.door_is_range_empty(j)) {
                        for j in ring {
                            self.door_mut(j).lastopen60 = lvframe60;
                        }
                    } else {
                        self.doors_request_mode(i, DOORMODE_CLOSING);
                    }
                }
            }
        }
        if self.door_mut(i).mode == DOORMODE_WAITING {
            let ring = self.door_ring(id);
            let shouldopen = ring.iter().skip(1).all(|&j| {
                let d = self.props.objs[j].door.as_ref().unwrap();
                d.mode == DOORMODE_IDLE && d.frac <= 0.0
            });
            if shouldopen {
                self.door_set_mode(i, DOORMODE_OPENING);
            }
        }
        let d = self.door_mut(i);
        if d.lastcalc60 < lvframe60 || self.lv.lvupdate240 == 0 {
            self.doors_calc_frac(i);
        }
        // An iris door's blades sound as the fraction passes 0.3 of open
        // (`propobj.c:7762`), on a handle of their own (`ps_create`).
        if self.props.objs[i].def.skel == SKEL_13 {
            let d = self.door_mut(i);
            let (frac, soundpoint) = (d.frac, d.maxfrac * 0.3);
            if frac > soundpoint && prevfrac <= soundpoint {
                self.door_sound(i, 3, 0x8014);
            } else if frac <= soundpoint && prevfrac > soundpoint {
                self.door_sound(i, 3, 0x8015);
            }
        }
    }

    /// `door_calc_intended_frac` (`propobj.c:19131`): towards open or shut.
    fn door_calc_intended_frac(&mut self, i: usize) -> bool {
        let lv = self.lv.clone();
        let d = self.door_mut(i);
        if d.mode == DOORMODE_OPENING || d.mode == DOORMODE_CLOSING {
            let end = if d.mode == DOORMODE_OPENING { d.maxfrac } else { 0.0 };
            // OBJFLAG3_DOOR_STICKY is Skedar Ruins' (solo).
            let (mut frac, mut speed) = (d.frac, d.fracspeed);
            apply_speed(&lv, &mut frac, end, &mut speed, d.accel, d.decel, d.maxspeed);
            d.frac = frac.clamp(0.0, d.maxfrac);
            d.fracspeed = speed;
            return true;
        }
        false
    }

    /// `doors_calc_frac` (`propobj.c:19221`): every door of the ring moves,
    /// unless one of their blocks would then meet a chr or an object (with the
    /// door's own block left out), when they all keep their fractions.
    fn doors_calc_frac(&mut self, i: usize) {
        let lvframe60 = self.lv.lvframe60;
        let id = self.props.objs[i].id;
        let ring = self.door_ring(id);
        let mut checkcollision = false;
        for &j in &ring {
            let d = self.door_mut(j);
            d.savedfrac = d.frac;
            if self.door_calc_intended_frac(j) {
                checkcollision = true;
            }
        }
        let mut blocked = false;
        if checkcollision {
            let unblockable = self.door_mut(i).doorflags & DOORFLAG_UNBLOCKABLEOPEN != 0;
            for &j in &ring {
                door_update_tiles(&mut self.props.objs[j]);
                let closing = self.door_mut(j).mode == DOORMODE_CLOSING;
                if !unblockable || closing {
                    let block = self.door_mut(j).geo;
                    let others = self.cd_blockvolume_obstacles(self.props.objs[j].id);
                    if cd_test_blockvolume(&block, &others) {
                        blocked = true;
                        break;
                    }
                }
            }
        }
        for &j in &ring {
            if checkcollision {
                if !blocked {
                    let d = self.door_mut(j);
                    if d.mode == DOORMODE_OPENING && d.frac >= d.maxfrac {
                        d.mode = DOORMODE_IDLE;
                        d.fracspeed = 0.0;
                        d.lastopen60 = lvframe60;
                        self.door_finish_open(j);
                    } else if d.mode == DOORMODE_CLOSING && d.frac <= 0.0 {
                        d.mode = DOORMODE_IDLE;
                        d.fracspeed = 0.0;
                        d.lastopen60 = 0;
                        self.door_finish_close(j);
                    }
                } else {
                    let d = self.door_mut(j);
                    d.fracspeed = 0.0;
                    d.frac = d.savedfrac;
                    door_update_tiles(&mut self.props.objs[j]);
                }
            }
            self.door_mut(j).lastcalc60 = lvframe60;
        }
        // portal_set_xlu_frac: the portal's openness for the acoustics, which
        // aren't ported.
    }

    /// What a door's block may not meet (`CDTYPE_OBJS | CDTYPE_PLAYERS |
    /// CDTYPE_CHRS | CDTYPE_PATHBLOCKER | CDTYPE_OBJSNOTSAFEORHELI`), without
    /// the door's own geometry.
    fn cd_blockvolume_obstacles(&self, doorid: u32) -> Vec<PropGeo> {
        let mut v: Vec<PropGeo> = self.chrs.iter().filter_map(|c| c.perim()).collect();
        for o in self.props.objs.iter().filter(|o| o.id != doorid && o.door.is_none()) {
            if o.geos.is_empty() || o.is_gone() || o.is_deleting() {
                continue;
            }
            v.extend(o.geos.iter().copied());
        }
        v
    }

    /// `doors_check_automatic` (`propobj.c:18085`), after player `pi`'s walk
    /// (`bwalk_tick`, `bondwalk.c:1834`): an automatic door the player is
    /// facing, in range of and unlocked opens (or stops closing).
    /// `// SUBST:` PD looks at the doors in the player's rooms / every door
    /// (the range test keeps it to the doors beside the player).
    pub(crate) fn doors_check_automatic(&mut self, pi: usize) {
        let (pos, facing) = (self.players[pi].pos, self.players[pi].theta_vec());
        for i in 0..self.props.objs.len() {
            let Some(d) = self.props.objs[i].door.as_ref() else { continue };
            if d.doorflags & DOORFLAG_AUTOMATIC == 0 || !(d.mode == DOORMODE_CLOSING || (d.mode == DOORMODE_IDLE && d.frac <= 0.0)) {
                continue;
            }
            let id = self.props.objs[i].id;
            let canopen = self.door_ring(id).into_iter().any(|j| {
                let s = self.props.objs[j].door.as_ref().unwrap();
                s.pos_is_in_front_of_door(pos) != s.vector_is_in_front_of_door(facing) && s.door_is_pos_in_range(pos, 0.0)
            });
            if canopen {
                self.doors_request_mode(i, DOORMODE_OPENING);
            }
        }
    }

    /// `current_player_interact` (`prop.c:1494`) for player `pi`'s use press,
    /// through `prop_find_for_interact` (`prop.c:1432`): the on-screen props
    /// near to far, an object `obj_test_for_interact` accepts becoming the
    /// choice and the walk going on, a door `door_test_for_interact` accepts
    /// ending it. A door is activated (`propdoor_interact`: MP doors have no
    /// locks); an object is used (`propobj_interact`: Hacker Central's
    /// terminal remembers who, `scenario_handle_activated_prop`). True:
    /// nothing was there to use (so the guns reload).
    ///
    /// `// SUBST:` PD's objects to use also count the alarms, a player's
    /// thrown Laptop (picked up again) and the lift doors / only the
    /// interactable ones (the terminal); a sentry isn't picked up by use yet.
    pub(crate) fn current_player_interact(&mut self, pi: usize) -> bool {
        let p = &self.players[pi];
        let (pos, theta) = (p.pos, p.theta);
        // prop_find_for_interact: the on-screen props, near to far.
        let mut cands: Vec<(f32, usize)> = self
            .props
            .objs
            .iter()
            .enumerate()
            // PROPFLAG_ONTHISSCREENTHISTICK. `// SUBST:` PD's is set by
            // the prop's on-screen test (its rooms on screen, its bounds in
            // the view) / one of its rooms on screen.
            .filter(|(_, o)| match o.door.as_ref() {
                Some(d) => d.rooms.iter().any(|&r| p.portalview.is_onscreen(r as usize)),
                None => o.flags3 & (OBJFLAG3_HTMTERMINAL | OBJFLAG3_INTERACTABLE) != 0 && !o.is_gone() && o.room.is_some_and(|r| p.portalview.is_onscreen(r as usize)),
            })
            .map(|(i, o)| (o.pos.distance_squared(pos), i))
            .collect();
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut interact = None;
        for (_, i) in cands {
            if self.props.objs[i].door.is_some() {
                if self.door_test_for_interact(i, pos, theta) {
                    interact = Some(i);
                    break;
                }
            } else if self.obj_test_for_interact(i, pos, theta) {
                interact = Some(i);
            }
        }
        let Some(i) = interact else { return true };
        if self.props.objs[i].door.is_some() {
            let pos = self.players[pi].pos;
            self.doors_choose_swing_direction(pos, i);
            self.doors_activate(i, true);
        } else {
            // propobj_interact (`propobj.c:15170`) → scenario_handle_activated_prop.
            let id = self.props.objs[i].id;
            self.scenario_handle_activated_prop(pi, id);
        }
        false
    }

    /// `obj_test_for_interact` (`propobj.c:15044`) for an interactable object:
    /// healthy, not `OBJFLAG_CANNOT_ACTIVATE`, within 2 m (1 m with
    /// `OBJFLAG3_INTERACTSHORTRANGE`) across and up or down, within 22.5° of
    /// the player's facing, and (with `OBJFLAG2_INTERACTCHECKLOS`) in sight.
    fn obj_test_for_interact(&self, i: usize, playerpos: Vec3, theta: f32) -> bool {
        let o = &self.props.objs[i];
        if o.flags & OBJFLAG_CANNOT_ACTIVATE != 0 || o.hidden2 & OBJH2FLAG_DESTROYED != 0 {
            return false;
        }
        let d = o.pos - playerpos;
        let range = if o.flags3 & OBJFLAG3_INTERACTSHORTRANGE != 0 { 100.0 } else { 200.0 };
        if !(d.x * d.x + d.z * d.z < range * range && d.y < range && d.y > -range) {
            return false;
        }
        let mut angle = pd_core::math::atan2f(d.x, d.z) - (360.0 - theta) * baddtor(360.0) / 360.0;
        if angle < 0.0 {
            angle += baddtor(360.0);
        }
        if angle > baddtor(180.0) {
            angle = baddtor(360.0) - angle;
        }
        angle <= baddtor(22.5) && (o.flags2 & OBJFLAG2_INTERACTCHECKLOS == 0 || self.level.los(playerpos, o.pos))
    }

    /// `door_test_for_interact` (`propobj.c:19623`): a usable door within 2 m
    /// of the player (or within 1.5 m of its pad's box), whose doorway or
    /// door lies ahead within 20° either side (`door_test_interact_angle`).
    fn door_test_for_interact(&self, i: usize, playerpos: Vec3, theta: f32) -> bool {
        let o = &self.props.objs[i];
        let d = o.door.as_ref().unwrap();
        if o.flags & OBJFLAG_CANNOT_ACTIVATE != 0 || d.maxfrac <= 0.0 {
            return false;
        }
        let diff = d.startpos - playerpos;
        let maybe = (diff.x * diff.x + diff.z * diff.z < 200.0 * 200.0 && diff.y < 200.0 && diff.y > -200.0)
            || pos_is_within_padding_of_padvol(playerpos, Vec3::splat(150.0), &d.pad);
        if !maybe {
            return false;
        }
        door_test_interact_angle(self, i, playerpos, theta, false) || (o.flags2 & OBJFLAG2_DOOR_ALTCOORDSYSTEM != 0 && door_test_interact_angle(self, i, playerpos, theta, true))
    }

    /// `chr_open_door` (`chraction.c:12261`): a door in chr `i`'s way to
    /// `rangepos` (the first closed or ajar door the move meets) within 2 m
    /// opens, swinging away from the chr. Returns the door.
    pub(crate) fn chr_open_door(&mut self, i: usize, rangepos: Vec3) -> Option<usize> {
        let c = &self.chrs[i];
        let from = c.pos;
        let mut doors: Vec<(usize, PropGeo)> = Vec::new();
        for (j, o) in self.props.objs.iter().enumerate() {
            if let Some(d) = &o.door {
                if d.cd_types(o.flags2) & (CDTYPE_CLOSEDDOORS | CDTYPE_AJARDOORS) != 0 {
                    doors.extend(o.geos.iter().map(|g| (j, *g)));
                }
            }
        }
        let geos: Vec<PropGeo> = doors.iter().map(|&(_, g)| g).collect();
        let hit = self.level.cd_test_cylmove_oobok_findclosest_obstacle(from, rangepos, true, 0.0, 0.0, &geos)?;
        let (j, _) = doors[hit];
        let o = &self.props.objs[j];
        let d = o.pos - from;
        if d.x * d.x + d.z * d.z < 200.0 * 200.0 {
            self.doors_choose_swing_direction(from, j);
            if !self.door_call_lift(j, false) {
                self.doors_request_mode(j, DOORMODE_OPENING);
            }
            Some(j)
        } else {
            None
        }
    }

    /// Resolve every door's sibling from the setup's command numbers.
    pub(crate) fn doors_link_siblings(&mut self, cmd_to_obj: &[Option<u32>], door_cmds: &[(usize, u32)]) {
        for &(cmd, id) in door_cmds {
            let Some(i) = self.props.objs.iter().position(|o| o.id == id) else { continue };
            let rel = self.door_mut(i).siblingcmd;
            if rel != 0 {
                let target = (cmd as i32 + rel) as usize;
                let sib = cmd_to_obj.get(target).copied().flatten();
                self.door_mut(i).sibling = sib;
            }
        }
    }
}

/// `door_get_activation_angle` (`propobj.c:19409`): the direction of (x, z)
/// relative to the player's facing, in −180°..180° (BADPI).
fn door_get_activation_angle(x: f32, z: f32, theta: f32) -> f32 {
    let mut angle = math::atan2f(x, z);
    angle -= (360.0 - theta) * (M_BADPI * 2.0) / 360.0;
    if angle < 0.0 {
        angle += baddtor(360.0);
    }
    if angle > baddtor(180.0) {
        angle -= baddtor(360.0);
    }
    angle
}

/// `door_get_activation_angles` (`propobj.c:19439`): the doorway's edges and,
/// if asked, the door's own, as angles from the player's facing.
fn door_get_activation_angles(d: &Door, flags: u32, playerpos: Vec3, theta: f32, withdoor: bool, altcoordsystem: bool) -> ((f32, f32), Option<(f32, f32)>) {
    let pad = &d.pad;
    let (ymin, ymax, upx, upz) = if altcoordsystem {
        (pad.bbox[0], pad.bbox[1], pad.up.y * pad.look.z - pad.look.y * pad.up.z, pad.up.x * pad.look.y - pad.look.x * pad.up.y)
    } else {
        (pad.bbox[2], pad.bbox[3], pad.up.x, pad.up.z)
    };
    let x1 = pad.pos.x + upx * ymin - playerpos.x;
    let z1 = pad.pos.z + upz * ymin - playerpos.z;
    let value1 = door_get_activation_angle(x1, z1, theta);
    let x2 = pad.pos.x + upx * ymax - playerpos.x;
    let z2 = pad.pos.z + upz * ymax - playerpos.z;
    let value2 = door_get_activation_angle(x2, z2, theta);
    let home = if value1 < value2 { (value1, value2) } else { (value2, value1) };
    if !withdoor {
        return (home, None);
    }
    let (value3, value4) = if d.doortype == DOORTYPE_SWINGING {
        let mut angle = d.frac * (M_BADPI / 180.0);
        if flags & OBJFLAG_DOOR_OPENTOFRONT != 0 {
            angle = baddtor(360.0) - angle;
        }
        let (c, s) = (angle.cos(), angle.sin());
        let x = pad.pos.x + upx * ymin - playerpos.x + (ymax - ymin) * (upx * c + upz * s);
        let z = pad.pos.z + upz * ymin - playerpos.z + (ymax - ymin) * (-upx * s + upz * c);
        (value1, door_get_activation_angle(x, z, theta))
    } else if matches!(d.doortype, DOORTYPE_SLIDING | DOORTYPE_FLEXI1 | DOORTYPE_FLEXI2 | DOORTYPE_FLEXI3) {
        let (xf, zf) = (d.slidedist.x * d.frac, d.slidedist.z * d.frac);
        (door_get_activation_angle(x1 + xf, z1 + zf, theta), door_get_activation_angle(x2 + xf, z2 + zf, theta))
    } else {
        (value1, value2)
    };
    (home, Some(if value3 < value4 { (value3, value4) } else { (value4, value3) }))
}

/// `door_test_interact_angle` (`propobj.c:19544`): the door (for a rotating
/// door, or a sliding one within 30 cm of its box) or the doorway lies within
/// 20° of the facing, or straddles it; a doorway made of siblings counts as
/// one.
fn door_test_interact_angle(w: &World, i: usize, playerpos: Vec3, theta: f32, altcoordsystem: bool) -> bool {
    let o = &w.props.objs[i];
    let d = o.door.as_ref().unwrap();
    let limit = baddtor(20.0);
    let includedoor = (d.doorflags & (DOORFLAG_TRANSLATION | DOORFLAG_0100)) != DOORFLAG_TRANSLATION || pos_is_within_padding_of_padvol(playerpos, Vec3::splat(30.0), &d.pad);
    let ((mut homemin, mut homemax), door) = door_get_activation_angles(d, o.flags, playerpos, theta, includedoor, altcoordsystem);
    let within = |a: f32| a >= -limit && a <= limit;
    if let Some((dmin, dmax)) = door {
        if (within(dmin) && within(dmax)) || (dmax - dmin < baddtor(180.0) && dmin < 0.0 && dmax > 0.0) {
            return true;
        }
    }
    if within(homemin) && within(homemax) {
        return true;
    }
    let mut sib = d.sibling;
    let mut seen = vec![o.id];
    while let Some(s) = sib.filter(|s| !seen.contains(s)) {
        if !(homemin >= 0.0 || homemax < 0.0) {
            break;
        }
        seen.push(s);
        let Some(so) = w.props.objs.iter().find(|x| x.id == s) else { break };
        let Some(sd) = so.door.as_ref() else { break };
        let ((smin, smax), _) = door_get_activation_angles(sd, so.flags, playerpos, theta, false, altcoordsystem);
        if homemin >= 0.0 && homemin > smin {
            homemin = smin;
        }
        if homemax <= 0.0 && homemax < smax {
            homemax = smax;
        }
        sib = sd.sibling;
    }
    homemax - homemin < baddtor(180.0) && homemin < 0.0 && homemax > 0.0
}

/// `cd_test_blockvolume_from_bytes` (`collision.c:3869`) against props: a
/// block overlapping another block (vertices inside either, or no separating
/// edge, `cd_block_collides_with_block_laterally`), or a cylinder.
pub fn cd_test_blockvolume(block: &PropGeo, others: &[PropGeo]) -> bool {
    for g in others {
        if g.ymax < block.ymin || g.ymin > block.ymax {
            continue;
        }
        if g.is_block() {
            if block.verts().iter().any(|v| crate::stage::cd_is_xz_in_block(g.verts(), v.x, v.y)) || g.verts().iter().any(|v| crate::stage::cd_is_xz_in_block(block.verts(), v.x, v.y)) {
                return true;
            }
            if !cd_block_collides_with_block_laterally(block.verts(), g.verts()) && !cd_block_collides_with_block_laterally(g.verts(), block.verts()) {
                return true;
            }
        } else if block_collides_with_cyl(block, g.x, g.z, g.radius) {
            return true;
        }
    }
    false
}

/// `cd_block_collides_with_cyl_laterally` (`collision.c:1118`).
fn block_collides_with_cyl(block: &PropGeo, x: f32, z: f32, radius: f32) -> bool {
    let v = block.verts();
    if crate::stage::cd_is_xz_in_block(v, x, z) {
        return true;
    }
    for i in 0..v.len() {
        let next = (i + 1) % v.len();
        let value = crate::stage::cd_pos_get_dist_to_line(v[i].x, v[i].y, v[next].x, v[next].y, x, z).abs();
        if value <= radius
            && (crate::stage::cd_pos_get_dist_to_vtx(v[i].x, v[i].y, x, z) <= radius
                || crate::stage::cd_pos_get_dist_to_vtx(v[next].x, v[next].y, x, z) <= radius
                || crate::stage::cd_pos_get_side(v[i].x, v[i].y, v[next].x, v[next].y, x, z))
        {
            return true;
        }
    }
    false
}

/// `cd_block_collides_with_block_laterally` (`collision.c:3808`): true when
/// one of `block1`'s edges separates it from `block2` (so they don't touch).
fn cd_block_collides_with_block_laterally(block1: &[Vec2], block2: &[Vec2]) -> bool {
    let n0 = block1.len();
    for i in 0..n0 {
        let next = (i + 1) % n0;
        let diff1 = block1[next].y as f64 - block1[i].y as f64;
        let diff2 = block1[i].x as f64 - block1[next].x as f64;
        if diff1 == 0.0 && diff2 == 0.0 {
            if crate::stage::cd_is_xz_in_block(block2, block1[i].x, block1[i].y) {
                return false;
            }
        } else {
            let sum1 = block1[i].x as f64 * diff1 + block1[i].y as f64 * diff2;
            let mut sum2 = sum1;
            let mut j = (next + 1) % n0;
            while j != i {
                sum2 = block1[j].x as f64 * diff1 + block1[j].y as f64 * diff2;
                if sum2 != sum1 {
                    break;
                }
                j = (j + 1) % n0;
            }
            let mut k = 0;
            while k < block2.len() {
                let sum3 = block2[k].x as f64 * diff1 + block2[k].y as f64 * diff2;
                if sum2 == sum1 {
                    sum2 = sum1 - sum3 + sum1;
                }
                if (sum3 < sum1 && sum2 < sum1) || (sum3 > sum1 && sum2 > sum1) {
                    break;
                }
                k += 1;
            }
            if k == block2.len() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::PlayerInput;
    use crate::testutil;

    fn world(code: &str) -> World {
        testutil::arena(code)
    }

    fn doors(w: &World) -> Vec<usize> {
        (0..w.props.objs.len()).filter(|&i| w.props.objs[i].door.is_some()).collect()
    }

    /// Every arena's setup doors are made, closed, each with its block, and a
    /// door that fills a portal closes it.
    #[test]
    fn every_arenas_doors_are_made_shut() {
        let expect = [("jun", 6), ("mp1", 5), ("mp3", 20), ("mp5", 12), ("mp9", 8), ("mp11", 19), ("mp12", 12), ("mp15", 10), ("ref", 0)];
        for (code, n) in expect {
            let w = world(code);
            let ds = doors(&w);
            assert_eq!(ds.len(), n, "{code}");
            // PD's lookup (setup_get_portal_by_door_pad) finds no portal for
            // Temple's and Ruins' doors: their pads' normals run along the doorway.
            let portals_expected = n > 0 && !matches!(code, "jun" | "mp9" | "mp15");
            let mut withportal = 0;
            for &i in &ds {
                let o = &w.props.objs[i];
                let d = o.door.as_ref().unwrap();
                assert!(d.is_closed(), "{code}: door {i} starts open");
                assert_eq!(o.geos.len(), 1, "{code}: door {i} has no block");
                assert!(d.maxfrac > 0.0 && d.maxspeed > 0.0, "{code}: door {i}");
                if let Some(p) = d.portalnum {
                    withportal += 1;
                    assert!(crate::stage::rooms::portal_is_closed(w.portalflags[p]), "{code}: door {i}'s portal {p} open");
                }
            }
            if portals_expected {
                assert!(withportal > 0, "{code}: no door has a portal");
            }
        }
    }

    /// Standing at a Felicity door and tapping use: it opens to `maxfrac`,
    /// opens its portal, loses its block past `perimfrac`, then closes itself
    /// after `autoclosetime` and the portal shuts again.
    #[test]
    fn a_used_door_opens_and_closes_itself() {
        let mut w = world("mp11");
        let i = doors(&w).into_iter().find(|&i| {
            let d = w.props.objs[i].door.as_ref().unwrap();
            d.doortype == DOORTYPE_SLIDING && d.portalnum.is_some() && d.sibling.is_none()
        });
        let i = i.expect("a lone sliding door with a portal");
        let (pad, portal, autoclose) = {
            let d = w.props.objs[i].door.as_ref().unwrap();
            (d.pad.clone(), d.portalnum.unwrap(), d.autoclosetime)
        };
        // Stand 1.2 m out along the pad's normal, facing the door.
        let n = pad.normal();
        let centre = pad.centre();
        let level = w.level.clone();
        let side = [1.0f32, -1.0].into_iter().find(|s| level.cd_find_room_at_pos_ycnp(centre + n * 120.0 * *s).is_some()).unwrap();
        let stand = centre + n * 120.0 * side;
        let face = -n * side;
        let floors = w.prop_floors();
        w.players[0].start_new_life(&level, &floors, stand, math::atan2f(face.x, face.z));
        for _ in 0..30 {
            w.step(4, &[PlayerInput::default()]);
        }
        // A use tap: B down for a few frames, then up.
        for f in 0..4 {
            w.step(4, &[PlayerInput { use_held: f < 3, ..Default::default() }]);
        }
        let mut opened_at = None;
        for f in 0..(300 + autoclose + 600) {
            w.step(4, &[PlayerInput::default()]);
            let d = w.props.objs[i].door.as_ref().unwrap();
            if opened_at.is_none() && d.is_open() {
                opened_at = Some(f);
                // Felicity's setup (accel 0x2aaa, maxspeed 0x2c5, maxfrac
                // 0xf333) through apply_speed: 148 ticks, about 2.5 s.
                assert!((144..=150).contains(&f), "open {f} ticks after the use tap");
                assert!(!crate::stage::rooms::portal_is_closed(w.portalflags[portal]), "the portal stays shut");
                // Past perimfrac the block goes; short of it (Felicity's doors
                // stop at 0.95 of 1.0) it has slid out of the doorway.
                let g = w.props.objs[i].geos.first().copied();
                assert!(g.is_none_or(|g| !crate::stage::cd_is_xz_in_block(g.verts(), centre.x, centre.z)), "the doorway is still blocked");
            }
            if let Some(o) = opened_at {
                if f > o + autoclose + 10 && d.is_closed() {
                    assert!(crate::stage::rooms::portal_is_closed(w.portalflags[portal]), "the portal stays open when shut");
                    return;
                }
            }
        }
        panic!("the door never opened (or never closed): opened at {opened_at:?}");
    }

    /// A hull door turns about its pad's normal, a swinging one about the
    /// vertical, the Aztec chair about the world's z; each by its fraction in
    /// degrees, about a hinge on its pad's `up * ymin` edge.
    #[test]
    fn rotating_doors_turn_about_their_axes() {
        let mut w = world("mp11");
        let i = doors(&w).into_iter().find(|&i| w.props.objs[i].door.as_ref().unwrap().doortype == DOORTYPE_SWINGING).unwrap();
        for (ty, axis) in [(DOORTYPE_SWINGING, None), (DOORTYPE_HULL, Some(true)), (DOORTYPE_AZTECCHAIR, Some(false))] {
            let o = &mut w.props.objs[i];
            let d = o.door.as_mut().unwrap();
            d.doortype = ty;
            d.frac = 0.0;
            door_update_tiles(o);
            let shut = (o.realrot, o.pos);
            o.door.as_mut().unwrap().frac = 30.0;
            door_update_tiles(o);
            let d = o.door.as_ref().unwrap();
            // The turn from shut to open, and the axis it leaves alone.
            let turn = o.realrot * shut.0.inverse();
            let want = match axis {
                None => Vec3::Y,
                Some(true) => d.pad.normal(),
                Some(false) => Vec3::Z,
            };
            assert!((turn * want).distance(want) < 1e-3, "type {ty}: the axis moved to {}", turn * want);
            let other = if want.y.abs() > 0.5 { d.pad.up } else { want.cross(Vec3::Y).normalize() };
            let a = (turn * other).dot(other).clamp(-1.0, 1.0).acos().to_degrees();
            assert!((a - 30.0).abs() < 0.5, "type {ty}: turned {a}°");
            assert!(o.pos.distance(shut.1) > 1.0, "type {ty}: it didn't move off its hinge");
        }
    }

    /// `door_init_matrices`: shut, an iris door's blades sit where the model
    /// puts them (each part translated from its parent); open, the inner ones
    /// turn by the fraction, the outer only past 0.3 of `maxfrac`.
    #[test]
    fn iris_blades_turn_as_the_door_opens() {
        use pd_core::model::{ModelDef, NodeKind};
        let pos = |p: Vec3, part: i32| (NodeKind::Position { pos: p, animpart: part as u16, mtx: [part as i16, -1, -1], flags: 0 }, None, Some(part));
        let mut nodes = vec![pos(Vec3::ZERO, 0)];
        for b in 0..6 {
            nodes.push((pos(Vec3::new(100.0 + b as f32, 0.0, 0.0), 2 * b + 1).0, Some(0), Some(2 * b + 1)));
            let parent = nodes.len() - 1;
            nodes.push((pos(Vec3::new(0.0, 50.0, 0.0), 2 * b + 2).0, Some(parent), Some(2 * b + 2)));
        }
        let def = ModelDef::from_nodes("iris", SKEL_13, 13, nodes);
        let w = world("mp11");
        let mut d = (**w.props.objs[doors(&w)[0]].door.as_ref().unwrap()).clone();
        d.maxfrac = 40.0;
        let at = |d: &Door| {
            let mut m = vec![Mat4::IDENTITY; 13];
            door_init_matrices(&def, d, &mut m);
            m
        };
        d.frac = 0.0;
        let m = at(&d);
        assert!(m[1].w_axis.truncate().distance(Vec3::new(100.0, 0.0, 0.0)) < 1e-4 && m[2].w_axis.truncate().distance(Vec3::new(100.0, 50.0, 0.0)) < 1e-4);
        d.frac = 10.0; // under 0.3 of 40: only the inner blades turn
        let m = at(&d);
        assert!(m[1].x_axis.truncate().distance(Vec3::X) < 1e-5, "an outer blade turned early");
        assert!((m[2].x_axis.truncate().angle_between(Vec3::X).to_degrees() - 10.0 * M_BADPI / std::f32::consts::PI).abs() < 0.1);
        d.frac = 40.0;
        let m = at(&d);
        assert!(m[1].x_axis.truncate().angle_between(Vec3::X) > 0.5, "the outer blades don't follow");
    }

    /// The editor's open-time readout, by `apply_speed`: Felicity's sliding
    /// doors take about 2.5 s (the use test's 148 ticks less the tap's).
    #[test]
    fn felicitys_doors_open_in_two_and_a_half_seconds() {
        let f = door_open_frames(0xf333, 0x2aaa, 0x2aaa, 0x2c5).unwrap();
        assert!((140..=150).contains(&f), "{f} frames");
        assert_eq!(door_open_frames(0xf333, 0, 0, 0), None);
    }

    /// A half-open `DOORFLAG_0004` door (Grid's sliding ones, Temple's vertical
    /// ones) draws within its shrunk box: nothing left of its left side (below
    /// its top), its texture cut with it; shut, it draws as loaded.
    #[test]
    fn a_retracting_door_stays_in_its_frame() {
        for code in ["mp15", "jun"] {
            let mut w = world(code);
            let i = doors(&w).into_iter().find(|&i| w.props.objs[i].door.as_ref().unwrap().doorflags & DOORFLAG_0004 != 0).unwrap();
            let shut = door_calc_texturemap(&w.props.objs[i], w.props.objs[i].door.as_ref().unwrap());
            assert!(!shut.is_empty(), "{code}: no display list to clip");
            let o = &w.props.objs[i];
            for (b, verts) in &shut {
                for (k, (p, _)) in verts.iter().enumerate() {
                    assert_eq!(glam::Vec3::from(*p), o.def.batches[*b].verts[k].pos, "{code}: a shut door moved a vertex");
                }
            }
            let d = w.props.objs[i].door.as_mut().unwrap();
            d.frac = d.maxfrac * 0.5;
            let (o, d) = (&w.props.objs[i], w.props.objs[i].door.as_ref().unwrap());
            let b = d.door_get_bbox(&o.bbox);
            let half = door_calc_texturemap(o, d);
            let (mut clipped, mut retextured) = (0, 0);
            for (bi, verts) in &half {
                for (k, (p, uv)) in verts.iter().enumerate() {
                    let v0 = o.def.batches[*bi].verts[k];
                    if d.doortype == DOORTYPE_VERTICAL {
                        assert!(p[1] <= b.ymax.ceil(), "{code}: a vertex above the top");
                    } else {
                        assert!(p[0] >= b.xmin.floor(), "{code}: a vertex left of the side");
                    }
                    if glam::Vec3::from(*p) != v0.pos {
                        clipped += 1;
                        // (An edge face's neighbour can share its `s`, `t`.)
                        if *uv != v0.uv {
                            retextured += 1;
                        }
                    }
                }
            }
            assert!(clipped > 0 && retextured > 0, "{code}: {clipped} clipped, {retextured} retextured half open");
        }
    }
}
