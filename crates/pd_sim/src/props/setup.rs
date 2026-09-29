//! The stage's MP setup made into props: `setup_create_props` (`setup.c:1420`)
//! over `props[]` in file order — the weapon locations and their ammo crates
//! (`props::pickup`), the setup's scenery (`stdobject`), glass and tinted glass
//! — each placed on its pad by `setup_create_object` (`setup.c:289`), scaled
//! to the pad's box when the object asks, and given the collision geometry
//! PD builds for it (`obj_update_all_geo`, `propobj.c:1869`).
//!
//! Doors and lifts are `props::door` and `props::lift`.

use glam::{Mat3, Mat4, Vec2, Vec3};
use pd_core::ids::*;
use pd_core::math;
use pd_core::model::{ModelDef, NodeKind, SKEL_BASIC};
use pd_core::mp::mp_get_mp_weapon_by_location;

use super::{Bbox, Obj};
use crate::stage::{PropFloor, PropGeo};
use crate::world::World;

/// `DIFF_A` (`lv.c:108`): the difficulty a Combat Simulator match runs at.
const DIFF_A: u32 = 0;

impl World {
    /// `setup_create_props` (`setup.c:1420`) for a Combat Simulator stage.
    /// An object is left out when its `flags2` exclude the difficulty (always
    /// Agent here) or the number of players (`setup.c:1426`).
    pub(crate) fn setup_create_props(&mut self) {
        let props = self.stage.props.clone();
        let weapons = self.setup.weapons;
        let mut diffflag = 1u32 << (DIFF_A + 4);
        diffflag |= match self.players.len() {
            2 => OBJFLAG2_EXCLUDE_2P,
            3 => OBJFLAG2_EXCLUDE_3P,
            4 => OBJFLAG2_EXCLUDE_4P,
            _ => 0,
        };
        let mut cur_mp_location: i32 = -1;
        // Each command's object (setup_get_cmd_by_index), for the doors' siblings.
        let mut cmd_to_obj: Vec<Option<u32>> = vec![None; props.len()];
        let mut door_cmds: Vec<(usize, u32)> = Vec::new();
        let mut lift_cmds: Vec<(usize, u32)> = Vec::new();
        let mut liftdoor_cmds: Vec<usize> = Vec::new();
        // tag(tagid, value): the object `value` commands on (`obj_find_by_tag_id`).
        let mut tags: Vec<(i32, usize)> = Vec::new();
        for (cmd, p) in props.iter().enumerate() {
            let before = self.props.objs.len();
            let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let int = |k: &str| p.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
            let flags2 = int("flags2") as u32;
            match ty {
                "weapon" => {
                    // The macro's `chr` parameter is the pad unless OBJFLAG_ASSIGNEDTOCHR.
                    let flags = int("flags") as u32;
                    if flags & OBJFLAG_ASSIGNEDTOCHR != 0 {
                        continue;
                    }
                    let pad = int("chr") as i32;
                    let mut weaponnum = int("weapon") as i32;
                    let mut extrascale = int("scale") as i32;
                    let mut createweapon = true;
                    cur_mp_location = -1;
                    let loc = weaponnum - WEAPON_MPLOCATION00 as i32;
                    if (0..16).contains(&loc) {
                        let mpweapon = mp_get_mp_weapon_by_location(&weapons, loc);
                        cur_mp_location = loc;
                        weaponnum = mpweapon.weaponnum;
                        extrascale = mpweapon.extrascale;
                        createweapon = mpweapon.hasweapon != 0;
                        if mpweapon.weaponnum == WEAPON_MPSHIELD as i32 {
                            // setup.c:645: a shield object, invincible, at full.
                            if let Some(mut o) = self.pickup_obj("chrshield", extrascale, OBJTYPE_SHIELD, 0, flags | OBJFLAG_01000000 | OBJFLAG_INVINCIBLE, pad) {
                                o.flags2 |= flags2 | OBJFLAG2_IMMUNETOEXPLOSIONS | OBJFLAG2_IMMUNETOGUNFIRE;
                                o.flags3 |= int("flags3") as u32;
                                o.shieldinitialamount = 1.0;
                                o.shieldamount = 1.0;
                                self.setup_create_object(o, pad, extrascale);
                            }
                            createweapon = false;
                        }
                    }
                    if weaponnum != WEAPON_NONE as i32 && createweapon {
                        let stem = self.res.gset.weapon(weaponnum as u8).and_then(|w| w.tp_model.clone());
                        if let Some(stem) = stem {
                            if let Some(mut o) = self.pickup_obj(&stem, extrascale, OBJTYPE_WEAPON, weaponnum as u8, flags, pad) {
                                o.flags2 |= flags2;
                                o.flags3 |= int("flags3") as u32;
                                self.setup_create_object(o, pad, extrascale);
                            }
                        }
                    }
                }
                "ammocratemulti" => {
                    let mut slots = [0i32; 19];
                    let mut ammoqty = 1;
                    if cur_mp_location >= 0 {
                        let mpweapon = mp_get_mp_weapon_by_location(&weapons, cur_mp_location);
                        ammoqty = mpweapon.priammoqty;
                        if mpweapon.priammotype > 0 && mpweapon.priammotype < 20 {
                            slots[(mpweapon.priammotype - 1) as usize] = ammoqty;
                        }
                        if mpweapon.secammotype > 0 && mpweapon.secammotype < 20 {
                            slots[(mpweapon.secammotype - 1) as usize] = mpweapon.secammoqty;
                        }
                    }
                    if ammoqty > 0 {
                        let pad = int("pad") as i32;
                        if let Some(mut o) = self.pickup_obj("multi_ammo_crate", int("scale") as i32, OBJTYPE_MULTIAMMOCRATE, 0, int("flags") as u32, pad) {
                            o.flags2 |= flags2;
                            o.flags3 |= int("flags3") as u32;
                            o.ammoslots = slots;
                            self.setup_create_object(o, pad, int("scale") as i32);
                        }
                    }
                }
                "stdobject" | "glass" | "tinted_glass" if flags2 & diffflag == 0 => {
                    let objtype = match ty {
                        "glass" => OBJTYPE_GLASS,
                        "tinted_glass" => OBJTYPE_TINTEDGLASS,
                        _ => OBJTYPE_BASIC,
                    };
                    let pad = int("pad") as i32;
                    let Some(mut o) = self.setup_obj(int("model") as i32, objtype, int("flags") as u32, flags2, int("flags3") as u32, int("maxdamage") as i32) else { continue };
                    if objtype != OBJTYPE_BASIC && o.flags & OBJFLAG_GLASS_HASPORTAL != 0 {
                        // setup_get_portal_by_pad (setup.c:1531, :1643).
                        o.portalnum = self.setup_get_portal_by_pad(pad);
                    }
                    if objtype == OBJTYPE_TINTEDGLASS {
                        // The macro's unk5c is `s16 xludist, opadist`; `unk64`
                        // is 0 either way (0 / 65536 with a portal, `setup.c:1536`).
                        let unk5c = int("unk5c") as u32;
                        o.tinted = Some(super::glass::TintedGlass { xludist: (unk5c >> 16) as i16 as f32, opadist: (unk5c & 0xffff) as i16 as f32, unk64: 0.0, opacity: 0 });
                    }
                    self.setup_create_object(o, pad, int("scale") as i32);
                }
                "door" if flags2 & diffflag == 0 => {
                    if let Some(id) = self.setup_create_door(p) {
                        door_cmds.push((cmd, id));
                    }
                }
                "lift" if flags2 & diffflag == 0 => {
                    if let Some(id) = self.setup_create_lift(p) {
                        lift_cmds.push((cmd, id));
                    }
                }
                "lift_door" => liftdoor_cmds.push(cmd),
                "hover_prop" if flags2 & diffflag == 0 => self.setup_create_hoverprop(p),
                "tag" => tags.push((int("id") as i32, cmd + int("value") as usize)),
                _ => {}
            }
            if self.props.objs.len() > before {
                cmd_to_obj[cmd] = self.props.objs.last().map(|o| o.id);
            }
        }
        // setup.c:1997: after the setup's objects and the simulants.
        self.scenario_init_props();
        self.doors_link_siblings(&cmd_to_obj, &door_cmds);
        self.lifts_link_doors(&props, &cmd_to_obj, &lift_cmds, &liftdoor_cmds);
        // SUBST: PD runs the setup's background AI lists on BG chrs (their
        // first tick) / their `activate_lift`s and `set_wind_speed`s run here,
        // with the props; the lists do nothing else a match needs.
        let bgai = self.stage.bgai.clone();
        for c in &bgai {
            if c["type"] == "set_wind_speed" {
                // ai_set_wind_speed (`chraicommands.c:9030`).
                self.sky_wind_speed = 0.1 * c["speed"].as_i64().unwrap_or(10) as i32 as f32;
            }
            if c["type"] == "activate_lift" {
                let tag = c["object"].as_i64().unwrap_or(-1) as i32;
                let obj = tags.iter().find(|t| t.0 == tag).and_then(|t| cmd_to_obj.get(t.1).copied().flatten());
                if let Some(id) = obj {
                    self.lift_activate(id, c["liftid"].as_i64().unwrap_or(0) as u8);
                }
            }
        }
    }

    /// A setup object before `setup_create_object`: `MODEL_*` `modelnum` at its
    /// `g_ModelStates` scale (`obj_init`, `propobj.c:2098`).
    pub(crate) fn setup_obj(&mut self, modelnum: i32, ty: u8, flags: u32, flags2: u32, flags3: u32, maxdamage: i32) -> Option<Obj> {
        let stem = self.res.models.stem_of_modelnum(modelnum)?.to_owned();
        let def = self.res.models.get(&stem).ok()?;
        let scale = self.res.models.modelstate_scale(&stem);
        let id = self.props.alloc_id();
        let mut o = Obj::weapon(id, def, scale, 0, FUNC_PRIMARY, 0);
        o.ty = ty;
        o.hidden = 0;
        o.flags = flags;
        o.flags2 = flags2;
        o.flags3 = flags3;
        o.timer240 = 0;
        o.modelnum = modelnum;
        o.maxdamage = maxdamage as f32;
        o.vis.clear();
        Some(o)
    }

    /// `setup_get_portal_by_pad` (`setup.c:894`): the portal the line through
    /// the pad's centre along its up axis (half its height and 10 cm each way)
    /// crosses.
    fn setup_get_portal_by_pad(&self, pad: i32) -> Option<usize> {
        let p = self.stage.pads.get(usize::try_from(pad).ok()?)?;
        let centre = p.centre();
        let mult = (p.bbox[3] - p.bbox[2]) * 0.5 + 10.0;
        self.stage.rooms.bg_find_portal_between_positions(centre - p.up * mult, centre + p.up * mult)
    }

    /// `setup_get_portal_by_door_pad` (`setup.c:918`): the same along the pad's
    /// normal (its x extent).
    pub(crate) fn setup_get_portal_by_door_pad(&self, pad: i32) -> Option<usize> {
        let p = self.stage.pads.get(usize::try_from(pad).ok()?)?;
        let centre = p.centre();
        let n = p.normal();
        let mult = (p.bbox[1] - p.bbox[0]) * 0.5 + 10.0;
        self.stage.rooms.bg_find_portal_between_positions(centre - n * mult, centre + n * mult)
    }

    /// `setup_create_object` (`setup.c:289`) on pad `pad`, with the setup's
    /// `extrascale` (256 = 1): the pad's basis (`mtx00016d58(-look, up)`), with a
    /// pad box the object sized to it on the axes its `*TOPADBOUNDS` flags name,
    /// then placed flat (`obj_place_2d`, `OBJFLAG_00000002`: glass, screens)
    /// or on the floor (`obj_place_3d`), and its geometry built.
    pub(crate) fn setup_create_object(&mut self, mut o: Obj, pad: i32, extrascale: i32) {
        let scale = extrascale as f32 * (1.0 / 256.0);
        if o.hidden2 & OBJH2FLAG_CANREGEN == 0 && o.is_pickup() {
            o.hidden2 |= OBJH2FLAG_CANREGEN;
        }
        // Every setup object can regenerate in a match (setup.c:305).
        o.hidden2 |= OBJH2FLAG_CANREGEN;
        let Some(p) = usize::try_from(pad).ok().and_then(|i| self.stage.pads.get(i)).cloned() else { return };
        if p.room.is_none() {
            return;
        }
        o.pad = pad;
        let mut mtx = math::look_at_basis(Vec3::ZERO, -p.look, p.up);
        let centre = if p.has_bbox_data() {
            let c = p.centre();
            c + (p.bbox[2] - p.bbox[3]) * 0.5 * p.up
        } else {
            p.pos
        };
        if p.has_bbox_data() {
            let b = o.bbox;
            let flag2 = o.flags & OBJFLAG_00000002 != 0;
            let (mut xscale, mut yscale, mut zscale) = (1.0f32, 1.0f32, 1.0f32);
            let pw = [p.bbox[1] - p.bbox[0], p.bbox[3] - p.bbox[2], p.bbox[5] - p.bbox[4]];
            if o.flags & OBJFLAG_XTOPADBOUNDS != 0 && b.xmin < b.xmax {
                xscale = pw[0] / ((b.xmax - b.xmin) * o.scale);
            }
            if o.flags & OBJFLAG_YTOPADBOUNDS != 0 && b.ymin < b.ymax {
                if flag2 {
                    zscale = pw[2] / ((b.ymax - b.ymin) * o.scale);
                } else {
                    yscale = pw[1] / ((b.ymax - b.ymin) * o.scale);
                }
            }
            if o.flags & OBJFLAG_ZTOPADBOUNDS != 0 && b.zmin < b.zmax {
                if flag2 {
                    yscale = pw[1] / ((b.zmax - b.zmin) * o.scale);
                } else {
                    zscale = pw[2] / ((b.zmax - b.zmin) * o.scale);
                }
            }
            let maxscale = xscale.max(yscale).max(zscale);
            if o.flags & OBJFLAG_XTOPADBOUNDS == 0 && b.xmax == b.xmin {
                xscale = maxscale;
            }
            if o.flags & OBJFLAG_YTOPADBOUNDS == 0 && b.ymax == b.ymin {
                if flag2 {
                    zscale = maxscale;
                } else {
                    yscale = maxscale;
                }
            }
            if o.flags & OBJFLAG_ZTOPADBOUNDS == 0 && b.zmax == b.zmin {
                if flag2 {
                    yscale = maxscale;
                } else {
                    zscale = maxscale;
                }
            }
            xscale /= maxscale;
            yscale /= maxscale;
            zscale /= maxscale;
            if xscale <= 0.000_001 || yscale <= 0.000_001 || zscale <= 0.000_001 {
                xscale = 1.0;
                yscale = 1.0;
                zscale = 1.0;
            }
            // mtx00015e24 / 15e80 / 15edc: the basis columns.
            mtx.x_axis *= xscale;
            mtx.y_axis *= yscale;
            mtx.z_axis *= zscale;
            o.scale *= maxscale;
        }
        o.scale *= scale;
        math::scale3(&mut mtx, o.scale);
        let (pos, rot) = if o.flags & OBJFLAG_00000002 != 0 {
            obj_place_2d(&o, &mtx, centre)
        } else {
            let others = self.props.objs.iter().filter(|x| !x.geos.is_empty()).map(|x| (x.pos, x.geos[0])).collect::<Vec<_>>();
            obj_place_3d(&self.level, &mut o, &mtx, centre, &others)
        };
        o.realrot = rot;
        o.pos = pos;
        let (inrooms, _, best) = self.stage.rooms.bg_find_rooms_by_pos(pos, 8);
        o.room = best.or(inrooms.first().copied()).or(p.room);
        obj_update_all_geo(&mut o);
        self.props.objs.push(o);
    }
}

/// `obj_place_3d` (`propobj.c:2232`): upside down (`OBJFLAG_UPSIDEDOWN`) it
/// hangs from `centre` by its top; with `OBJFLAG_00000008` it stands by its
/// model's y extent at `centre`; otherwise the basis vector most nearly
/// vertical picks the box's axis, and that axis's low end (its high end if
/// the vector points down) goes on the floor under the centre (plus the
/// ground clearance), or on top of an object's block standing there
/// (`OBJHFLAG_ONANOTHEROBJ`). Returns the position and the rotation.
pub fn obj_place_3d(level: &crate::stage::TileLevel, o: &mut Obj, mtx: &Mat4, centre: Vec3, others: &[(Vec3, PropGeo)]) -> (Vec3, Mat3) {
    let b: Bbox = o.bbox;
    let (mut min, mut max) = (b.ymin, b.ymax);
    let mut sp70 = *mtx;
    let cols = |m: &Mat4| [m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate()];
    if o.flags & OBJFLAG_UPSIDEDOWN != 0 {
        // mtx4_load_z_rotation(BADDTOR(180)), then mtx × it.
        let rz = math::mtx4_load_z_rotation(math::baddtor(180.0));
        sp70 = *mtx * rz;
        let c = cols(&sp70);
        return (centre - c[1] * max, Mat3::from_mat4(sp70));
    }
    if o.flags & OBJFLAG_00000008 != 0 {
        let c = cols(&sp70);
        return (centre - c[1] * min, Mat3::from_mat4(sp70));
    }
    let c = cols(&sp70);
    let mut row = 0;
    let mut maxval = c[0].y.abs();
    let mut isnegative = c[0].y < 0.0;
    for (r, col) in c.iter().enumerate().skip(1) {
        if col.y.abs() > maxval {
            row = r;
            isnegative = col.y < 0.0;
            maxval = col.y.abs();
        }
    }
    if row == 0 {
        min = b.xmin;
        max = b.xmax;
    } else if row == 2 {
        min = b.zmin;
        max = b.zmax;
    }
    if isnegative {
        std::mem::swap(&mut min, &mut max);
    }
    let mut pos2 = centre - c[row] * min;
    // SUBST: PD finds the floor among the rooms the line from the pad to
    // here reaches (`los_find_final_room_exhaustive`) / among every tile.
    if let Some((y, _)) = level.cd_find_room_at_pos_ycnp(pos2) {
        // obj_find_by_pos (propobj.c:1015): an object whose block holds the point.
        let under = others.iter().find(|(_, g)| g.is_block() && crate::stage::cd_is_xz_in_block(g.verts(), pos2.x, pos2.z)).map(|(_, g)| *g);
        let clearance = super::projectile::obj_get_ground_clearance(o);
        match under {
            Some(block) if block.ymax > y && block.ymin < y + (max - min) * c[row].y + clearance => {
                pos2.y = block.ymax - c[row].y * min;
                o.hidden |= OBJHFLAG_ONANOTHEROBJ;
            }
            _ => pos2.y = y - min * c[row].y + clearance,
        }
    }
    (pos2, Mat3::from_mat4(sp70))
}

/// `obj_place_2d` (`propobj.c:2359`): a flat object (glass, a screen) turned
/// by X 270° then Y 180° into the pad's frame, standing its z extent's low
/// end back from `centre`.
pub fn obj_place_2d(o: &Obj, mtx: &Mat4, centre: Vec3) -> (Vec3, Mat3) {
    let mult = o.bbox.zmin;
    let rx = math::mtx4_load_x_rotation(math::baddtor(270.0));
    let ry = math::mtx4_load_y_rotation(math::baddtor(180.0));
    // mtx4_mult_mtx4_in_place(&sp1c, &sp5c): sp5c = sp1c × sp5c; then arg2 × that.
    let sp5c = *mtx * (ry * rx);
    let z = sp5c.z_axis.truncate();
    (centre - z * mult, Mat3::from_mat4(sp5c))
}

/// `obj_update_all_geo` (`propobj.c:1869`): the object's core block from its
/// box (or, on a basic skeleton, its `MODELPART_BASIC_0064` quad) through its
/// rotation and position (`obj_update_core_geo`), and its floor and wall
/// quads (`obj_update_extra_geo`).
pub fn obj_update_all_geo(o: &mut Obj) {
    o.geos.clear();
    o.floors.clear();
    let mtx = Mat4::from_cols(o.realrot.x_axis.extend(0.0), o.realrot.y_axis.extend(0.0), o.realrot.z_axis.extend(0.0), o.pos.extend(1.0));
    if o.flags & OBJFLAG_CORE_GEO_INUSE != 0 {
        let b = o.bbox;
        let geo = (o.def.skel == SKEL_BASIC).then(|| geo_part(&o.def, MODELPART_BASIC_0064)).flatten();
        let (ymin, ymax) = (o.pos.y + b.rotated_y_min(&o.realrot), o.pos.y + b.rotated_y_max(&o.realrot));
        let verts = match geo {
            // obj_populate_geoblock_from_modeldef (propobj.c:652).
            Some(v) => v.iter().map(|p| {
                let w = mtx.transform_point3(*p);
                Vec2::new(w.x, w.z)
            }).collect(),
            None => obj_populate_geoblock_vertices_from_bbox_and_mtx(&b, &mtx),
        };
        o.geos.push(PropGeo::block(&verts, ymin, ymax));
    }
    // obj_update_extra_geo (propobj.c:1831): the floor, then the walls.
    if let Some(v) = geo_part(&o.def, MODELPART_BASIC_FLOORGEO) {
        let mut flags = GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2;
        if o.ty == OBJTYPE_ESCASTEP {
            flags |= GEOFLAG_LIFTFLOOR;
        }
        o.floors.push(PropFloor { verts: obj_get_vertices_from_georodata(&v, &o.realrot, o.pos), flags, prop: o.id, room: o.room });
    }
    if let Some(v) = geo_part(&o.def, MODELPART_BASIC_WALLGEO) {
        let q = obj_get_vertices_from_georodata(&v, &o.realrot, o.pos);
        let (lo, hi) = q.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        let xz: Vec<Vec2> = q.iter().map(|p| Vec2::new(p.x, p.z)).collect();
        o.geos.push(PropGeo::block(&xz, lo, hi));
    }
}

/// A model's GEO part (`model_get_part_rodata`), padded to four vertices.
pub(crate) fn geo_part(def: &ModelDef, part: i32) -> Option<Vec<Vec3>> {
    let n = def.get_part(part)?;
    match &def.nodes[n].kind {
        NodeKind::Geo { verts } if !verts.is_empty() => Some(verts.clone()),
        _ => None,
    }
}

/// `obj_get_vertices_from_georodata` (`propobj.c:4911`): the quad's four
/// vertices through the object's rotation, at its position.
pub(crate) fn obj_get_vertices_from_georodata(v: &[Vec3], rot: &Mat3, pos: Vec3) -> [Vec3; 4] {
    let mut out = [Vec3::ZERO; 4];
    for (i, o) in out.iter_mut().enumerate() {
        let p = v.get(i).copied().unwrap_or(v[v.len() - 1]);
        *o = *rot * p + pos;
    }
    out
}

/// `obj_populate_geoblock_vertices_from_bbox_and_mtx` (`propobj.c:454`): the
/// box's eight corners projected to x/z (in f64), duplicates within 0.001
/// merged, then the convex outline walked from the four extreme points
/// (leftmost, bottom, rightmost, top) with at most one extra corner between
/// each pair; translated to the matrix's position.
pub fn obj_populate_geoblock_vertices_from_bbox_and_mtx(b: &Bbox, mtx: &Mat4) -> Vec<Vec2> {
    let (m00, m02) = (mtx.x_axis.x as f64, mtx.x_axis.z as f64);
    let (m10, m12) = (mtx.y_axis.x as f64, mtx.y_axis.z as f64);
    let (m20, m22) = (mtx.z_axis.x as f64, mtx.z_axis.z as f64);
    let (x0, x1) = (b.xmin as f64, b.xmax as f64);
    let (y0, y1) = (b.ymin as f64, b.ymax as f64);
    let (z0, z1) = (b.zmin as f64, b.zmax as f64);
    let (a0, a2, b0, b2, c0, c2) = (m00 * x0, m02 * x0, m10 * y0, m12 * y0, m20 * z0, m22 * z0);
    let (a1, a3, b1, b3, c1, c3) = (m00 * x1, m02 * x1, m10 * y1, m12 * y1, m20 * z1, m22 * z1);
    let sp270 = [
        [a0 + b0 + c0, a2 + b2 + c2],
        [a0 + b0 + c1, a2 + b2 + c3],
        [a0 + b1 + c0, a2 + b3 + c2],
        [a0 + b1 + c1, a2 + b3 + c3],
        [a1 + b0 + c0, a3 + b2 + c2],
        [a1 + b0 + c1, a3 + b2 + c3],
        [a1 + b1 + c0, a3 + b3 + c2],
        [a1 + b1 + c1, a3 + b3 + c3],
    ];
    let f0 = 0.001f32 as f64;
    let mut sp1f0: Vec<[f64; 2]> = Vec::new();
    for p in sp270 {
        if !sp1f0.iter().any(|q| {
            let (a, bb) = (p[0] - q[0], p[1] - q[1]);
            a < f0 && a > -f0 && bb < f0 && bb > -f0
        }) {
            sp1f0.push(p);
        }
    }
    let len = sp1f0.len();
    let (mut t3, mut t2, mut t1, mut t0) = (0, 0, 0, 0);
    for i in 1..len {
        let (p, q) = (sp1f0[i], sp1f0[t3]);
        if p[0] < q[0] || (p[0] == q[0] && p[1] < q[1]) {
            t3 = i;
        }
    }
    for i in 1..len {
        let (p, q) = (sp1f0[i], sp1f0[t2]);
        if q[1] < p[1] || (p[1] == q[1] && p[0] < q[0]) {
            t2 = i;
        }
    }
    for i in 1..len {
        let (p, q) = (sp1f0[i], sp1f0[t1]);
        if q[0] < p[0] || (p[0] == q[0] && q[1] < p[1]) {
            t1 = i;
        }
    }
    for i in 1..len {
        let (p, q) = (sp1f0[i], sp1f0[t0]);
        if p[1] < q[1] || (p[1] == q[1] && q[0] < p[0]) {
            t0 = i;
        }
    }
    let indexes: Vec<usize> = (0..len).filter(|&i| i != t3 && i != t1 && i != t2 && i != t0).collect();
    let v = &sp1f0;
    // Is `index` outside the edge a → b (the side the outline bulges to)?
    let beyond = |index: usize, a: usize, b: usize| (v[index][0] - v[b][0]) * (v[a][1] - v[b][1]) < (v[a][0] - v[b][0]) * (v[index][1] - v[b][1]);
    let mut out: Vec<[f64; 2]> = vec![v[t3]];
    if t0 != t3 {
        if let Some(&i) = indexes.iter().find(|&&i| beyond(i, t3, t0)) {
            out.push(v[i]);
        }
        out.push(v[t0]);
    }
    if t1 != t0 {
        if let Some(&i) = indexes.iter().find(|&&i| beyond(i, t0, t1)) {
            out.push(v[i]);
        }
        out.push(v[t1]);
    }
    if t2 != t1 {
        if let Some(&i) = indexes.iter().find(|&&i| beyond(i, t1, t2)) {
            out.push(v[i]);
        }
    }
    if t2 != t1 && t3 != t2 {
        out.push(v[t2]);
    }
    if t3 != t2 {
        if let Some(&i) = indexes.iter().find(|&&i| beyond(i, t2, t3)) {
            out.push(v[i]);
        }
    }
    let (tx, tz) = (mtx.w_axis.x, mtx.w_axis.z);
    out.iter().map(|p| Vec2::new(p[0] as f32 + tx, p[1] as f32 + tz)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::PlayerInput;
    use crate::testutil;
    use pd_core::mp::{MatchPlayer, MatchSetup};

    fn complex_world() -> World {
        let (stage, level) = testutil::complex_arc();
        let setup = MatchSetup { stagenum: STAGE_MP_COMPLEX, players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
        World::new(setup, stage, level, testutil::res(), 5).unwrap()
    }

    /// The player walking straight at one of Complex's crates stops at its
    /// side: never inside the block, and within a step of the player's radius
    /// from it.
    #[test]
    fn the_player_walks_into_a_crate_and_stops() {
        let mut w = complex_world();
        let c = w.props.objs.iter().find(|o| o.ty == OBJTYPE_BASIC && o.hidden & OBJHFLAG_ONANOTHEROBJ == 0).unwrap();
        let block = c.geos[0];
        let centre = block.verts().iter().fold(Vec2::ZERO, |a, v| a + *v) / block.numvertices as f32;
        // Stand 3 m off, on the side with open floor, facing it.
        let level = w.level.clone();
        let start = [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y]
            .iter()
            .map(|d| centre + *d * 300.0)
            .find(|p| level.cd_find_room_at_pos_ycnp(Vec3::new(p.x, c.pos.y + 50.0, p.y)).is_some_and(|(y, _)| (y - (c.pos.y + c.bbox.ymin * c.scale - 4.0)).abs() < 5.0))
            .expect("no open floor beside the crate");
        let d = centre - start;
        let angle = pd_core::math::atan2f(d.x, d.y);
        let floors = w.prop_floors();
        w.players[0].start_new_life(&level, &floors, Vec3::new(start.x, c.pos.y + 50.0, start.y), angle);
        let fwd = PlayerInput { walk_y: 127, ..Default::default() };
        let mut closest = f32::MAX;
        for _ in 0..240 {
            w.step(4, std::slice::from_ref(&fwd));
            let p = w.players[0].pos;
            assert!(!crate::stage::cd_is_xz_in_block(block.verts(), p.x, p.z), "the player walked into the crate at {p}");
            closest = closest.min((Vec2::new(p.x, p.z) - centre).length());
        }
        // The crate is a 1 m cube (±50 cm); the player's radius is 30 cm.
        assert!(closest < 110.0, "never reached the crate: {closest}");
    }

    /// An unrotated 2 × 4 box gives its rectangle, anticlockwise from the
    /// bottom-left corner, at the matrix's position.
    #[test]
    fn a_box_projects_to_its_outline() {
        let b = Bbox { xmin: -1.0, xmax: 1.0, ymin: 0.0, ymax: 3.0, zmin: -2.0, zmax: 2.0 };
        let m = Mat4::from_translation(Vec3::new(10.0, 0.0, 20.0));
        let v = obj_populate_geoblock_vertices_from_bbox_and_mtx(&b, &m);
        assert_eq!(v, vec![Vec2::new(9.0, 18.0), Vec2::new(11.0, 18.0), Vec2::new(11.0, 22.0), Vec2::new(9.0, 22.0)]);
        // Rotated 45°, the outline is a diamond of 4 corners.
        let r = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4);
        let d = obj_populate_geoblock_vertices_from_bbox_and_mtx(&b, &r);
        assert_eq!(d.len(), 4);
        assert!(crate::stage::cd_is_xz_in_block(&d, 0.0, 0.0));
        assert!(!crate::stage::cd_is_xz_in_block(&d, 3.0, 0.0));
    }
}
