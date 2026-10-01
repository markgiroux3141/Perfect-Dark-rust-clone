//! Doors: a layout's ([`Door`]) as the setup rows and pads PD's
//! `setup_create_door` (`setup.c:944`) reads, and a stage's own adopted back
//! into a layout; the catalogues of door models the level editor offers
//! (every door PD's setups place, `assets/data/doors.json` from
//! `tools/pd-assets/pd_doors.py`, and GoldenEye's, `custom/ge/doors.json`
//! from `tools/ge-extract --doors`); and whether a door closes a portal.
//!
//! A door's pad stands upright in its doorway: its look up (down for a door
//! that sinks), its up across the doorway from the hinge (PD's hinge, and
//! the edge a sliding door slides to, is the pad's `up * ymin` edge), its box
//! the door's thickness, width and height centred on the pad. PD's front is
//! the pad's normal (`up × look`); a door hinged on the right, seen from the
//! front the layout gives it, is PD's turned round.

use std::path::{Path, PathBuf};

use glam::Vec3;
use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_sim::stage::rooms::BgRooms;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::layout::{self, Door, Motion, Side, Swing};
use crate::write::PadRow;
use crate::Paths;

/// `DOORFLAG_ROTATEDPAD` (`constants.h:815`): `pad_rotate_for_door` stands
/// the pad up at run time; a layout's pads are made upright instead.
const DOORFLAG_ROTATEDPAD: u16 = 0x0040;
/// The object flags placement sets from the layout: the portal, two-way, and
/// the swing (but a two-way door's, which says how it starts: `flags` keeps it).
fn placed_flags(twoway: bool) -> u32 {
    OBJFLAG_DOOR_HASPORTAL | OBJFLAG_DOOR_TWOWAY | if twoway { 0 } else { OBJFLAG_DOOR_OPENTOFRONT }
}

/// A catalogue row: a template's setup numbers (`pd_doors.py` `ROW_KEYS`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct Row {
    pub doortype: u16,
    pub doorflags: u16,
    pub flags: u32,
    pub flags2: u32,
    pub maxfrac: i32,
    pub perimfrac: i32,
    pub accel: i32,
    pub decel: i32,
    pub maxspeed: i32,
    pub autoclosetime: i32,
    pub unk88: u32,
    pub soundtype: u8,
}

/// One way a model is placed: how often, in a box of what median size
/// (width, height, thickness), with what row.
#[derive(Clone, Debug, Deserialize)]
pub struct Template {
    pub count: u32,
    pub size: [f32; 3],
    pub row: Row,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    Pd,
    Ge,
}

/// A door model and the ways its game places it.
#[derive(Clone, Debug, Deserialize)]
pub struct CatDoor {
    pub stem: String,
    /// Its `MODEL_*` number (GoldenEye's: 0x1000 + its prop number).
    pub model: i32,
    /// `MODEL_*` (PD) or the prop's directory (GoldenEye).
    pub name: String,
    /// `SKEL_*`: `SKEL_11` and `SKEL_13` are the eyelid and iris doors.
    pub skel: i32,
    /// The stages (PD) or levels (GoldenEye) it is placed on.
    pub seen: Vec<String>,
    pub templates: Vec<Template>,
    #[serde(skip, default = "default_game")]
    pub game: Game,
}

fn default_game() -> Game {
    Game::Pd
}

#[derive(Deserialize)]
struct CatFile {
    format: String,
    doors: Vec<CatDoor>,
}

impl CatDoor {
    /// A readable name: "Dd Officedoor" for `MODEL_DD_OFFICEDOOR`.
    pub fn title(&self) -> String {
        let base = self.name.trim_start_matches("MODEL_");
        let words: Vec<String> = base
            .split('_')
            .filter(|w| !w.is_empty())
            .map(|w| {
                let lower = w.to_ascii_lowercase();
                let mut c = lower.chars();
                c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
            })
            .collect();
        words.join(" ")
    }

    /// The motion a template's row gives (an eyelid or iris model moves as its
    /// skeleton does, whatever its row says).
    pub fn motion(&self, t: &Template) -> Motion {
        match self.skel {
            SKEL_11 => Motion::Eyelid,
            SKEL_13 => Motion::Iris,
            _ => Motion::of(t.row.doortype, false).unwrap_or(Motion::Slide),
        }
    }

    /// A door of this model as template `t` places it, standing at `pos`
    /// facing `facing` (degrees), hinged on the left, swinging back.
    pub fn door(&self, t: usize, pos: Vec3, facing: f32) -> Door {
        let t = &self.templates[t.min(self.templates.len().saturating_sub(1))];
        let r = t.row;
        Door {
            pos: [pos.x, pos.y, pos.z].map(layout::round1),
            facing: layout::round1(facing.rem_euclid(360.0)) % 360.0,
            size: t.size,
            model: self.stem.clone(),
            motion: self.motion(t),
            side: Side::Left,
            swing: if r.flags & OBJFLAG_DOOR_TWOWAY != 0 { Swing::Both } else if r.flags & OBJFLAG_DOOR_OPENTOFRONT != 0 { Swing::Front } else { Swing::Back },
            maxfrac: r.maxfrac,
            perimfrac: r.perimfrac,
            accel: r.accel,
            decel: r.decel,
            maxspeed: r.maxspeed,
            autoclosetime: r.autoclosetime,
            soundtype: r.soundtype,
            doorflags: r.doorflags & !DOORFLAG_ROTATEDPAD,
            flags: r.flags & !placed_flags(r.flags & OBJFLAG_DOOR_TWOWAY != 0),
            flags2: r.flags2,
            unk88: r.unk88,
            sibling: None,
        }
    }
}

/// Every door model the editor can place.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    pub doors: Vec<CatDoor>,
    /// What couldn't be read (GoldenEye's catalogue not extracted yet, ...).
    pub notes: Vec<String>,
}

/// `custom/ge/doors.json`.
pub fn ge_catalogue_path(custom: &Path) -> PathBuf {
    custom.join("ge").join("doors.json")
}

impl Catalogue {
    /// PD's catalogue (in `assets/`) and GoldenEye's (in the custom tree, once
    /// extracted: [`extract_ge`]).
    pub fn load(assets: &AssetDir) -> Catalogue {
        let mut c = Catalogue::default();
        let mut read = |path: PathBuf, game: Game| match assets.read_json::<CatFile>(&path) {
            Ok(f) if f.format == "pd-doors/1" => c.doors.extend(f.doors.into_iter().map(|d| CatDoor { game, ..d })),
            Ok(f) => c.notes.push(format!("{}: format {:?}, expected pd-doors/1", path.display(), f.format)),
            Err(e) => c.notes.push(e),
        };
        read(assets.data("doors.json"), Game::Pd);
        match assets.custom_dir().map(ge_catalogue_path).filter(|p| p.exists()) {
            Some(p) => read(p, Game::Ge),
            None => c.notes.push("GoldenEye's doors aren't extracted (they need the GoldenEye ROM a GE level's recipe names)".into()),
        }
        c
    }

    pub fn get(&self, stem: &str) -> Option<&CatDoor> {
        self.doors.iter().find(|d| d.stem == stem)
    }
}

/// `MODEL_*` by stem: `models/index.json` and the custom tree's.
pub fn model_numbers(assets: &AssetDir) -> Result<std::collections::HashMap<String, i32>, String> {
    let mut out = std::collections::HashMap::new();
    for path in std::iter::once(assets.model_index()).chain(assets.custom_model_index()) {
        let v: Value = assets.read_json(&path)?;
        for (stem, row) in v.as_object().into_iter().flatten() {
            if let Some(n) = row["modelnum"].as_i64() {
                out.insert(stem.clone(), n as i32);
            }
        }
    }
    Ok(out)
}

/// A door's pad: at its box's centre, upright, the box centred on it.
pub fn pad_of(d: &Door) -> PadRow {
    let [w, h, t] = d.size;
    PadRow { pos: d.centre(), look: d.look(), up: d.up(), flags: PADFLAG_HASBBOXDATA, bbox: [-t * 0.5, t * 0.5, -w * 0.5, w * 0.5, -h * 0.5, h * 0.5] }
}

/// PD's front (the pad's normal, `up × look`) against the layout's.
fn pd_front_is_front(d: &Door) -> bool {
    d.up().cross(d.look()).dot(d.front()) > 0.0
}

/// A door's `setup.json` row on pad `pad`: model `modelnum`, closing its
/// doorway's portal if `portal`, the next of its sibling ring `sibling` rows
/// on (0: none; PD's relative command number, `setup.c:1063`).
pub fn row_of(d: &Door, modelnum: i32, pad: usize, portal: bool, sibling: i32) -> Value {
    let mut flags = d.flags & !placed_flags(d.swing == Swing::Both);
    if portal {
        flags |= OBJFLAG_DOOR_HASPORTAL;
    }
    match d.swing {
        Swing::Both => flags |= OBJFLAG_DOOR_TWOWAY,
        // OPENTOFRONT swings towards the pad's normal (`door_update_tiles`).
        s => {
            if (s == Swing::Front) == pd_front_is_front(d) {
                flags |= OBJFLAG_DOOR_OPENTOFRONT;
            }
        }
    }
    json!({
        "type": "door", "scale": 256, "model": modelnum, "pad": pad,
        "flags": flags, "flags2": d.flags2, "flags3": 0, "maxdamage": 1000,
        "maxfrac": d.maxfrac, "perimfrac": d.perimfrac, "accel": d.accel, "decel": d.decel, "maxspeed": d.maxspeed,
        "doorflags": d.doorflags & !DOORFLAG_ROTATEDPAD, "doortype": d.motion.doortype(), "keyflags": 0,
        "autoclosetime": d.autoclosetime, "unk88": d.unk88, "sibling": sibling, "unkc4": (d.soundtype as u32) << 8,
    })
}

/// The portal a door closes: PD's (`setup_get_portal_by_door_pad`,
/// `setup.c:918`: the one the line along the pad's normal through its centre
/// crosses), if the doorway is all it covers: every corner within the door's
/// box (the union of its sibling ring's, `ring`) grown by `margin` cm. A
/// portal bigger than the doorway (a room split elsewhere, a k-d cut)
/// would hide the rooms behind it while the door is shut.
pub fn portal_for(rooms: &BgRooms, d: &Door, ring: &[&Door], margin: f32) -> Option<usize> {
    let pad = pad_of(d);
    let n = pad.up.cross(pad.look);
    let mult = d.size[2] * 0.5 + 10.0;
    let p = rooms.bg_find_portal_between_positions(pad.pos - n * mult, pad.pos + n * mult)?;
    let inside = |v: Vec3| {
        ring.iter().any(|o| {
            let (c, up) = (o.centre(), o.up());
            let dv = v - c;
            dv.dot(up).abs() <= o.size[0] * 0.5 + margin && dv.y.abs() <= o.size[1] * 0.5 + margin && dv.dot(o.front()).abs() <= o.size[2] * 0.5 + margin
        })
    };
    rooms.portals[p].verts.iter().all(|&v| inside(v)).then_some(p)
}

/// The doors of a stage's `setup.json` (with its `pads.json`) as a layout's.
/// Returns them and what couldn't be taken (a door on a pad that isn't
/// upright, of a type no layout door has).
pub fn adopt(pads: &Value, setup: &Value) -> (Vec<Door>, Vec<String>) {
    let f3 = |x: &Value| Vec3::new(x[0].as_f64().unwrap_or(0.0) as f32, x[1].as_f64().unwrap_or(0.0) as f32, x[2].as_f64().unwrap_or(0.0) as f32);
    let int = |r: &Value, k: &str| r[k].as_i64().unwrap_or(0);
    let props = setup["props"].as_array().cloned().unwrap_or_default();
    let (mut doors, mut skipped) = (Vec::new(), Vec::new());
    // props index -> door index, and each door's sibling as a props index.
    let mut door_of = std::collections::HashMap::new();
    let mut sib_cmd = Vec::new();
    for (ci, r) in props.iter().enumerate() {
        if r["type"] != "door" {
            continue;
        }
        let p = &pads["pads"][int(r, "pad") as usize];
        let doorflags = (int(r, "doorflags") & 0xffff) as u16;
        let (mut up, mut look) = (f3(&p["up"]), f3(&p["look"]));
        if doorflags & DOORFLAG_ROTATEDPAD != 0 {
            // pad_rotate_for_door (pad.c:186).
            up.y = 0.0;
            up = up.normalize_or_zero();
            look = Vec3::Y;
        }
        let b: [f32; 6] = std::array::from_fn(|k| p["bbox"][k].as_f64().unwrap_or(0.0) as f32);
        let doortype = (int(r, "doortype") & 0xffff) as u16;
        let upright = look.y.abs() > 0.999 && up.y.abs() < 0.001;
        let Some(motion) = Motion::of(doortype, look.y < 0.0).filter(|_| upright) else {
            skipped.push(format!("door row {ci} (model {:#x}, type {doortype}): its pad isn't upright, or no layout door moves so", int(r, "model")));
            continue;
        };
        let n = up.cross(look);
        let pos = f3(&p["pos"]);
        let centre = pos + ((b[0] + b[1]) * n + (b[2] + b[3]) * up + (b[4] + b[5]) * look) * 0.5;
        let size = [b[3] - b[2], b[5] - b[4], b[1] - b[0]];
        // Left-hinged, the front the pad's up × Y: then `Door::up` is the pad's.
        let front = up.cross(Vec3::Y);
        let flags = int(r, "flags") as u32;
        let mut d = Door {
            pos: [centre.x, centre.y - size[1] * 0.5, centre.z].map(layout::round1),
            facing: layout::round1(layout::facing(front)) % 360.0,
            size: size.map(layout::round1),
            model: int(r, "model").to_string(),
            motion,
            side: Side::Left,
            swing: Swing::Back,
            maxfrac: int(r, "maxfrac") as i32,
            perimfrac: int(r, "perimfrac") as i32,
            accel: int(r, "accel") as i32,
            decel: int(r, "decel") as i32,
            maxspeed: int(r, "maxspeed") as i32,
            autoclosetime: int(r, "autoclosetime") as i32,
            soundtype: ((int(r, "unkc4") >> 8) & 0xff) as u8,
            doorflags: doorflags & !DOORFLAG_ROTATEDPAD,
            flags: flags & !placed_flags(flags & OBJFLAG_DOOR_TWOWAY != 0),
            flags2: int(r, "flags2") as u32,
            unk88: int(r, "unk88") as u32,
            sibling: None,
        };
        d.swing = if flags & OBJFLAG_DOOR_TWOWAY != 0 {
            Swing::Both
        } else if (flags & OBJFLAG_DOOR_OPENTOFRONT != 0) == pd_front_is_front(&d) {
            Swing::Front
        } else {
            Swing::Back
        };
        door_of.insert(ci, doors.len());
        let rel = int(r, "sibling") as i32;
        sib_cmd.push((rel != 0).then_some(ci as i32 + rel));
        doors.push(d);
    }
    for (i, cmd) in sib_cmd.into_iter().enumerate() {
        doors[i].sibling = cmd.and_then(|c| door_of.get(&(c as usize)).copied()).filter(|&s| s != i);
    }
    (doors, skipped)
}

/// A layout's `model` as adopted is the row's `MODEL_*` number; turn each into
/// its stem (by `models`, stem -> number).
pub fn name_models(doors: &mut [Door], models: &std::collections::HashMap<String, i32>) {
    for d in doors {
        if let Ok(n) = d.model.parse::<i32>() {
            if let Some((stem, _)) = models.iter().filter(|(_, &m)| m == n).min_by_key(|(s, _)| s.len()) {
                d.model = stem.clone();
            }
        }
    }
}

/// The recipes' GoldenEye ROM and decomp (the first GE recipe's), or
/// `$PD_GE_ROM` with `reference/ge-decomp`.
fn ge_rom(levels: &Path) -> Option<(PathBuf, PathBuf)> {
    let decomp = crate::ge::repo().join("reference").join("ge-decomp");
    if let Some(rom) = std::env::var_os("PD_GE_ROM") {
        return Some((PathBuf::from(rom), decomp));
    }
    let mut recipes: Vec<PathBuf> = std::fs::read_dir(levels).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json") && !p.to_string_lossy().ends_with(".layout.json")).collect();
    recipes.sort();
    recipes.into_iter().find_map(|p| match crate::recipe::Recipe::load(&p).ok()?.source {
        crate::recipe::Source::Ge(g) => Some((PathBuf::from(&g.rom), crate::ge::repo().join(&g.decomp))),
        _ => None,
    })
}

/// Run `tools/ge-extract --doors`: every GoldenEye door model into
/// `custom/models/` and their catalogue into `custom/ge/doors.json`. The
/// ROM is a GE recipe's in `levels` (or `$PD_GE_ROM`). Returns its report.
pub fn extract_ge(paths: &Paths, levels: &Path) -> Result<Vec<String>, String> {
    let (rom, decomp) = ge_rom(levels).ok_or("no GoldenEye ROM: no recipe in the levels directory is a GoldenEye level's, and $PD_GE_ROM isn't set")?;
    let python = std::env::var_os("PYTHON").unwrap_or_else(|| "python".into());
    let out = std::process::Command::new(&python)
        .arg(crate::ge::repo().join("tools").join("ge-extract").join("ge_extract.py"))
        .arg("--rom")
        .arg(&rom)
        .arg("--decomp")
        .arg(&decomp)
        .arg("--custom")
        .arg(&paths.custom)
        .arg("--doors")
        .output()
        .map_err(|e| format!("{}: {e} (the GoldenEye extractor needs Python; set PYTHON)", python.to_string_lossy()))?;
    if !out.status.success() {
        return Err(format!("tools/ge-extract --doors failed:\n{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door(motion: Motion, side: Side, swing: Swing, facing: f32) -> Door {
        Door {
            pos: [100.0, -20.0, 300.0],
            facing,
            size: [120.0, 230.0, 10.0],
            model: "1234".into(),
            motion,
            side,
            swing,
            maxfrac: if motion.turns() { 90 << 16 } else { 62259 },
            perimfrac: 65536,
            accel: 10922,
            decel: 10922,
            maxspeed: 218,
            autoclosetime: 900,
            soundtype: 3,
            doorflags: 0x0004,
            flags: 0,
            flags2: OBJFLAG2_AICANNOTUSE,
            unk88: 0,
            sibling: None,
        }
    }

    /// A layout's doors written as PD's rows and pads, then adopted back,
    /// come back as they were: every motion, both sides, every swing, any
    /// facing, a ring of two.
    #[test]
    fn a_door_is_its_row_and_back() {
        let mut doors = Vec::new();
        for (k, m) in Motion::ALL.into_iter().enumerate() {
            for side in [Side::Left, Side::Right] {
                for swing in [Swing::Back, Swing::Front, Swing::Both] {
                    if swing != Swing::Back && !matches!(m, Motion::Swing | Motion::Hull | Motion::Chair) {
                        continue;
                    }
                    doors.push(door(m, side, swing, (k as f32 * 37.0 + 15.0) % 360.0));
                }
            }
        }
        doors[0].sibling = Some(1);
        doors[1].sibling = Some(0);
        doors[2].flags = OBJFLAG_DOOR_KEEPOPEN;
        let (mut pads, mut props) = (Vec::new(), Vec::new());
        for (i, d) in doors.iter().enumerate() {
            let p = pad_of(d);
            pads.push(json!({"pos": [p.pos.x, p.pos.y, p.pos.z], "look": [p.look.x, p.look.y, p.look.z], "up": [p.up.x, p.up.y, p.up.z], "flags": p.flags, "bbox": p.bbox}));
            props.push(row_of(d, 1234, i, i % 3 == 0, d.sibling.map_or(0, |s| s as i32 - i as i32)));
        }
        // A weapon before them: siblings are relative command numbers.
        props.insert(0, json!({"type": "weapon", "chr": 0}));
        for r in props.iter_mut().skip(1) {
            r["pad"] = json!(r["pad"].as_u64().unwrap());
        }
        let (back, skipped) = adopt(&json!({"pads": pads}), &json!({"props": props}));
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(back.len(), doors.len());
        for (i, (a, b)) in doors.iter().zip(&back).enumerate() {
            // Adopted doors are hinged on the left: a right-hinged one comes back
            // turned round, the same door.
            let (f, side) = if a.side == Side::Right { ((a.facing + 180.0) % 360.0, Side::Left) } else { (a.facing, Side::Left) };
            let swing = match (a.side, a.swing) {
                (Side::Right, Swing::Front) => Swing::Back,
                (Side::Right, Swing::Back) => Swing::Front,
                (_, s) => s,
            };
            let turned = matches!(a.motion, Motion::Swing | Motion::Hull | Motion::Chair);
            assert!((b.facing - f).abs() < 0.11 || (b.facing - f).abs() > 359.8, "door {i} ({:?} {:?}): facing {} for {f}", a.motion, a.side, b.facing);
            assert_eq!((b.side, b.motion, b.sibling), (side, a.motion, a.sibling), "door {i}");
            if turned {
                assert_eq!(b.swing, swing, "door {i} ({:?} {:?} {:?})", a.motion, a.side, a.swing);
            }
            assert!(Vec3::from(b.pos).distance(Vec3::from(a.pos)) < 0.1 && b.size == a.size, "door {i}: {:?} {:?}", b.pos, b.size);
            assert_eq!((b.maxfrac, b.perimfrac, b.accel, b.decel, b.maxspeed, b.autoclosetime, b.soundtype, b.doorflags, b.flags, b.flags2), (a.maxfrac, a.perimfrac, a.accel, a.decel, a.maxspeed, a.autoclosetime, a.soundtype, a.doorflags, a.flags, a.flags2), "door {i}");
            // The same pad either way: the door PD makes is the same.
            let (p, q) = (pad_of(a), pad_of(b));
            assert!(p.pos.distance(q.pos) < 0.1 && p.up.distance(q.up) < 1e-4 && p.look == q.look, "door {i}: pad {p:?} / {q:?}");
        }
    }

    /// PD's hinge is the pad's `up * ymin` edge and its front the pad's normal:
    /// a left-hinged door's hinge is on the left seen from its front.
    #[test]
    fn the_hinge_is_on_its_side() {
        for side in [Side::Left, Side::Right] {
            let d = door(Motion::Swing, side, Swing::Back, 0.0);
            let p = pad_of(&d);
            let hinge = p.pos + p.up * p.bbox[2];
            // Facing +z, seen from the front (at +z looking at -z) the viewer's
            // left is -x.
            let left = hinge.x < p.pos.x;
            assert_eq!(left, side == Side::Left, "{side:?}");
        }
    }
}
