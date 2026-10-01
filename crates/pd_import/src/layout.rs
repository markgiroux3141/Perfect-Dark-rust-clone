//! A level's **layout** (`crates/pd_import/levels/<code>.layout.json`): the
//! gameplay data placed by hand (in `pd_edit`), committed beside the recipe.
//! Placement ([`crate::place`]) takes it as it takes a source's own setup:
//! what the layout holds is placed exactly, the rest is generated.
//!
//! Each kind is either absent (generated from the recipe's counts, or the
//! source's own rows kept, as without a layout) or present (exactly these,
//! even none). The editor writes every kind once the user saves, so moving
//! one generated weapon never reshuffles the others.
//!
//! Positions are world centimetres (a pad's position: [`crate::place`]'s
//! `PAD_HEIGHT` over its floor), not pad numbers, so a re-exported level keeps
//! its layout. A facing is in degrees, PD's `atan2f(look.x, look.z)` (0 faces
//! +z, 90 faces +x).

use std::path::{Path, PathBuf};

use glam::Vec3;
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "pd-layout/1";

/// Somewhere to stand, facing somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub pos: [f32; 3],
    #[serde(default)]
    pub facing: f32,
}

/// A weapon pad: MP location `location` (`WEAPON_MPLOCATION00 + location`),
/// whose gun the weapon set picks (`mp_get_mp_weapon_by_location`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weapon {
    pub pos: [f32; 3],
    pub location: u8,
}

/// An ammo crate (`ammocratemulti`). It holds the ammo of the weapon row
/// before it in the setup (`setup.c`: `cur_mp_location`), so it names its
/// weapon (an index into [`Layout::weapons`]) and is written after it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ammo {
    pub pos: [f32; 3],
    pub weapon: usize,
}

/// A Capture the Case base (`case`) or respawn pad (`case_respawn`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TeamSpot {
    pub pos: [f32; 3],
    #[serde(default)]
    pub facing: f32,
    pub team: u8,
}

/// How a door moves: PD's `DOORTYPE_*`, and for a vertical door which way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    /// `DOORTYPE_SLIDING`: across the doorway, towards [`Door::side`].
    #[default]
    Slide,
    /// `DOORTYPE_FLEXI1..3`: GoldenEye's Bunker trislides, slid as sliding doors.
    Flexi1,
    Flexi2,
    Flexi3,
    /// `DOORTYPE_VERTICAL`, rising (the pad's look up) or sinking (down).
    Up,
    Down,
    /// `DOORTYPE_SWINGING`: about the hinge on [`Door::side`], [`Door::swing`] the way.
    Swing,
    /// `DOORTYPE_EYE`, `DOORTYPE_IRIS`: Caverns' eyelid and iris doors (their
    /// models' skeletons pose them; any other model stands still).
    Eyelid,
    Iris,
    /// `DOORTYPE_AZTECCHAIR`: turned about the world's z axis.
    Chair,
    /// `DOORTYPE_HULL`: turned in its own plane about its bottom corner on
    /// [`Door::side`] (the Attack Ship's windows).
    Hull,
}

impl Motion {
    pub const ALL: [Motion; 11] = [Motion::Slide, Motion::Up, Motion::Down, Motion::Swing, Motion::Hull, Motion::Eyelid, Motion::Iris, Motion::Chair, Motion::Flexi1, Motion::Flexi2, Motion::Flexi3];

    /// PD's door type.
    pub fn doortype(self) -> u16 {
        match self {
            Motion::Slide => 0,
            Motion::Flexi1 => 1,
            Motion::Flexi2 => 2,
            Motion::Flexi3 => 3,
            Motion::Up | Motion::Down => 4,
            Motion::Swing => 5,
            Motion::Eyelid => 6,
            Motion::Iris => 7,
            Motion::Chair => 9,
            Motion::Hull => 10,
        }
    }

    /// The motion of a door of type `doortype` whose pad looks down (`down`).
    pub fn of(doortype: u16, down: bool) -> Option<Motion> {
        Some(match doortype {
            0 => Motion::Slide,
            1 => Motion::Flexi1,
            2 => Motion::Flexi2,
            3 => Motion::Flexi3,
            4 if down => Motion::Down,
            4 => Motion::Up,
            5 => Motion::Swing,
            6 => Motion::Eyelid,
            7 => Motion::Iris,
            9 => Motion::Chair,
            10 => Motion::Hull,
            _ => return None,
        })
    }

    /// Whether it turns (its `maxfrac` is in degrees) rather than slides (a
    /// fraction of its size).
    pub fn turns(self) -> bool {
        matches!(self, Motion::Swing | Motion::Eyelid | Motion::Iris | Motion::Chair | Motion::Hull)
    }

    pub fn name(self) -> &'static str {
        match self {
            Motion::Slide => "slides across",
            Motion::Flexi1 => "trislide 1",
            Motion::Flexi2 => "trislide 2",
            Motion::Flexi3 => "trislide 3",
            Motion::Up => "rises",
            Motion::Down => "sinks",
            Motion::Swing => "swings",
            Motion::Eyelid => "eyelid",
            Motion::Iris => "iris",
            Motion::Chair => "Aztec chair",
            Motion::Hull => "hull window",
        }
    }
}

/// A door's hinge, or the edge it slides to, as seen from its front.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// Which way a swinging door opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Swing {
    /// Away from the front (into the room behind it).
    #[default]
    Back,
    /// Towards the front.
    Front,
    /// Either way, away from whoever opens it (`OBJFLAG_DOOR_TWOWAY`).
    Both,
}

/// A door: a model stretched to a box standing in a doorway (as PD's
/// `setup_create_door` stretches it to its pad's box), and its setup row.
/// The row's numbers are the setup's own integers (`include/props.h`
/// `door()`), taken from a catalogue template (`pd_doors.py`,
/// `ge_doors.py`) and then edited, so a door is PD's to the bit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Door {
    /// The middle of its bottom edge, shut.
    pub pos: [f32; 3],
    /// The way its front faces (degrees, as the other facings).
    pub facing: f32,
    /// Width (across the doorway), height and thickness (cm).
    pub size: [f32; 3],
    /// Its model's stem (in `models/index.json` or `custom/models/index.json`).
    pub model: String,
    pub motion: Motion,
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub swing: Swing,
    /// How far it opens (16.16): a fraction of its width or height for one
    /// that slides, degrees for one that turns.
    pub maxfrac: i32,
    /// Its collision goes once open this far (16.16, as `maxfrac`).
    pub perimfrac: i32,
    /// `accel`, `decel` (/ 65536000 a frame²) and `maxspeed` (/ 65536 a frame).
    pub accel: i32,
    pub decel: i32,
    pub maxspeed: i32,
    /// Frames (60ths) it stays open before closing itself.
    pub autoclosetime: i32,
    /// `door_play_opening_sound`'s type (0: silent).
    #[serde(default)]
    pub soundtype: u8,
    /// `DOORFLAG_*` (never `ROTATEDPAD`: the pad is made upright).
    #[serde(default)]
    pub doorflags: u16,
    /// `obj->flags` bits besides the portal, the swing and two-way (placement
    /// sets those), e.g. `OBJFLAG_DOOR_KEEPOPEN`.
    #[serde(default)]
    pub flags: u32,
    #[serde(default)]
    pub flags2: u32,
    /// `xludist << 16 | opadist`: a windowed door's glass fade (cm).
    #[serde(default)]
    pub unk88: u32,
    /// The next door of its sibling ring (an index into the layout's doors):
    /// they open and close together, as a double door's two leaves do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sibling: Option<usize>,
}

impl Door {
    /// The door's front, as a unit vector.
    pub fn front(&self) -> Vec3 {
        look(self.facing)
    }

    /// Its pad's up: across the doorway, from the hinge (or the edge it slides
    /// to) towards the other side. PD's hinge is the pad's `up * ymin` edge,
    /// and its front the pad's normal (`up × look`); a door hinged on the
    /// right is PD's turned round, its front the pad's back.
    pub fn up(&self) -> Vec3 {
        let across = Vec3::Y.cross(self.front());
        match self.side {
            Side::Left => across,
            Side::Right => -across,
        }
    }

    /// Its pad's look: up, or down for a door that sinks.
    pub fn look(&self) -> Vec3 {
        if self.motion == Motion::Down {
            -Vec3::Y
        } else {
            Vec3::Y
        }
    }

    /// Its box's centre, shut.
    pub fn centre(&self) -> Vec3 {
        Vec3::from(self.pos) + Vec3::Y * (self.size[1] * 0.5)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawns: Option<Vec<Spot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapons: Option<Vec<Weapon>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ammo: Option<Vec<Ammo>>,
    /// King of the Hill: the hill is the pad's room.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hills: Option<Vec<Spot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bases: Option<Vec<TeamSpot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub respawns: Option<Vec<TeamSpot>>,
    /// Where simulants take cover (`pads.json` `cover[]`), facing away from
    /// the wall they hide behind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<Vec<Spot>>,
    /// Hand edits to the generated waypoint graph ([`crate::waypoints`]),
    /// replayed on it each time the level is placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waypoints: Option<WaypointEdits>,
    /// The doors (replacing the source's own: a GoldenEye level's are adopted
    /// into the layout like its other rows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doors: Option<Vec<Door>>,
}

/// A waypoint moved from where the generator put it (`from`, a pad position)
/// to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Moved {
    pub from: [f32; 3],
    pub to: [f32; 3],
}

/// A link between the waypoints at `a` and `b` (positions after the moves
/// and additions); `one_way`: from `a` to `b` only.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub a: [f32; 3],
    pub b: [f32; 3],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub one_way: bool,
}

/// Edits to a generated waypoint graph, each keyed by position (a waypoint
/// is whichever lies within [`crate::waypoints::TOL`] of it), applied in
/// this order: removed, moved, added, unlinked, linked. A key that finds no
/// waypoint (the geometry changed under it) is reported and skipped.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WaypointEdits {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<Moved>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlinked: Vec<Link>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked: Vec<Link>,
}

impl WaypointEdits {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.moved.is_empty() && self.added.is_empty() && self.unlinked.is_empty() && self.linked.is_empty()
    }
}

/// A door's keys in the order a diff reads best: where, which, how.
const DOOR_KEYS: &[&str] = &["pos", "facing", "size", "model", "motion", "side", "swing", "sibling"];

/// `val` on one line, an object's keys in `order` first (the rest sorted).
fn one_line(val: &serde_json::Value, order: &[&str]) -> String {
    match val {
        serde_json::Value::Object(obj) if !order.is_empty() => {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort_by_key(|k| (order.iter().position(|o| o == k).unwrap_or(order.len()), k.as_str()));
            let parts: Vec<String> = keys.iter().map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), serde_json::to_string(&obj[k.as_str()]).unwrap())).collect();
            format!("{{{}}}", parts.join(","))
        }
        _ => serde_json::to_string(val).unwrap(),
    }
}

/// Write `val` as JSON with each array's items one per line at `indent`
/// (an item's keys in `items` first), objects' keys (in `order` first) one
/// per line.
fn pretty(out: &mut String, val: &serde_json::Value, indent: usize, order: &[&str], items: &[&str]) {
    let pad = "  ".repeat(indent);
    match val {
        serde_json::Value::Array(list) if !list.is_empty() => {
            out.push_str("[\n");
            for (j, it) in list.iter().enumerate() {
                out.push_str(&format!("{pad}  {}{}\n", one_line(it, items), if j + 1 < list.len() { "," } else { "" }));
            }
            out.push_str(&format!("{pad}]"));
        }
        serde_json::Value::Object(obj) if !obj.is_empty() => {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort_by_key(|k| order.iter().position(|o| o == k).unwrap_or(order.len()));
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                out.push_str(&format!("{pad}  {}: ", serde_json::to_string(k).unwrap()));
                let items = if k.as_str() == "doors" { DOOR_KEYS } else { &[] };
                pretty(out, &obj[k.as_str()], indent + 1, &["removed", "moved", "added", "unlinked", "linked"], items);
                out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{pad}}}"));
        }
        _ => out.push_str(&serde_json::to_string(val).unwrap()),
    }
}

impl Layout {
    pub fn new() -> Layout {
        Layout { format: FORMAT.into(), ..Default::default() }
    }

    /// `levels/<code>.layout.json` beside a recipe at `recipe`.
    pub fn path_for(recipe: &Path) -> PathBuf {
        let stem = recipe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        recipe.with_file_name(format!("{stem}.layout.json"))
    }

    /// The layout at `path`, or none if there is no file.
    pub fn load(path: &Path) -> Result<Option<Layout>, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let l: Layout = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if l.format != FORMAT {
            return Err(format!("{}: format {:?}, expected {FORMAT}", path.display(), l.format));
        }
        l.check().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(l))
    }

    /// Written pretty, one item per line, so a diff shows what moved.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.check()?;
        let mut v = serde_json::to_value(self).map_err(|e| e.to_string())?;
        // f32s as the tenths they are, not their f64 expansions.
        fn tidy(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Number(n) if n.is_f64() => *v = serde_json::json!(((n.as_f64().unwrap() * 10.0).round() / 10.0)),
                serde_json::Value::Array(a) => a.iter_mut().for_each(tidy),
                serde_json::Value::Object(o) => o.values_mut().for_each(tidy),
                _ => {}
            }
        }
        tidy(&mut v);
        let mut out = String::new();
        pretty(&mut out, &v, 0, &["format", "spawns", "weapons", "ammo", "hills", "bases", "respawns", "cover", "doors", "waypoints"], &[]);
        out.push('\n');
        std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The rules a layout keeps: crates name a weapon of the layout's own.
    pub fn check(&self) -> Result<(), String> {
        if let Some(ammo) = &self.ammo {
            let Some(weapons) = &self.weapons else { return Err("ammo crates without the weapons they belong to (a layout with ammo has weapons)".into()) };
            if let Some((i, a)) = ammo.iter().enumerate().find(|(_, a)| a.weapon >= weapons.len()) {
                return Err(format!("ammo crate {i} names weapon {} of {}", a.weapon, weapons.len()));
            }
        }
        if let Some((i, w)) = self.weapons.iter().flatten().enumerate().find(|(_, w)| w.location >= 16) {
            return Err(format!("weapon {i}: location {} (there are 16, 0-15)", w.location));
        }
        let doors = self.doors.as_deref().unwrap_or(&[]);
        for (i, d) in doors.iter().enumerate() {
            if d.model.is_empty() {
                return Err(format!("door {i} has no model"));
            }
            if d.size.iter().any(|&s| s.is_nan() || s <= 0.0) {
                return Err(format!("door {i}: size {:?} (each must be over 0)", d.size));
            }
            if let Some(s) = d.sibling.filter(|&s| s >= doors.len() || s == i) {
                return Err(format!("door {i}: its sibling is door {s} of {}", doors.len()));
            }
        }
        Ok(())
    }

    /// Every kind as the stage in `dir` has it now (its `pads.json` and
    /// `setup.json`): a level's generated placement, and the source's doors,
    /// taken over by hand. Returns the layout and what couldn't be taken (a
    /// weapon pad naming a gun rather than an MP location: none of PD's
    /// arenas has one; a door whose pad doesn't stand upright).
    pub fn adopt(dir: &Path) -> Result<(Layout, Vec<String>), String> {
        let read = |f: &str| -> Result<serde_json::Value, String> {
            let p = dir.join(f);
            serde_json::from_str(&std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))
        };
        let pads = read("pads.json")?;
        let setup = read("setup.json")?;
        let v3 = |v: &serde_json::Value| Vec3::new(v[0].as_f64().unwrap_or(0.0) as f32, v[1].as_f64().unwrap_or(0.0) as f32, v[2].as_f64().unwrap_or(0.0) as f32);
        let pad = |row: &serde_json::Value, key: &str| -> Option<(Vec3, f32)> {
            let p = &pads["pads"][row[key].as_u64()? as usize];
            p.is_object().then(|| (v3(&p["pos"]), facing_tenths(v3(&p["look"]))))
        };
        let arr = |p: Vec3| [round1(p.x), round1(p.y), round1(p.z)];
        let mut l = Layout::new();
        let (mut spawns, mut hills, mut bases, mut respawns) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for row in setup["intro"].as_array().into_iter().flatten() {
            let Some((pos, f)) = pad(row, "pad") else { continue };
            let team = row["id"].as_u64().unwrap_or(0) as u8;
            match row["type"].as_str() {
                Some("spawn") => spawns.push(Spot { pos: arr(pos), facing: f }),
                Some("hill") => hills.push(Spot { pos: arr(pos), facing: 0.0 }),
                Some("case") => bases.push(TeamSpot { pos: arr(pos), facing: f, team }),
                Some("case_respawn") => respawns.push(TeamSpot { pos: arr(pos), facing: f, team }),
                _ => {}
            }
        }
        let (mut weapons, mut ammo, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
        let mpl = pd_core::ids::WEAPON_MPLOCATION00 as i64;
        for row in setup["props"].as_array().into_iter().flatten() {
            match row["type"].as_str() {
                Some("weapon") => {
                    let w = row["weapon"].as_i64().unwrap_or(0);
                    match pad(row, "chr") {
                        Some((pos, _)) if (mpl..mpl + 16).contains(&w) => weapons.push(Weapon { pos: arr(pos), location: (w - mpl) as u8 }),
                        _ => skipped.push(format!("a weapon pad for weapon {w} (not an MP location)")),
                    }
                }
                Some("ammocratemulti") => match (pad(row, "pad"), weapons.len()) {
                    (Some((pos, _)), n) if n > 0 => ammo.push(Ammo { pos: arr(pos), weapon: n - 1 }),
                    _ => skipped.push("an ammo crate before any weapon".into()),
                },
                _ => {}
            }
        }
        let cover = pads["cover"].as_array().into_iter().flatten().map(|c| Spot { pos: arr(v3(&c["pos"])), facing: facing_tenths(v3(&c["look"])) }).collect();
        let (mut doors, left) = crate::doors::adopt(&pads, &setup);
        skipped.extend(left);
        // The models by stem: PD's index, and the custom tree's (the stage
        // directory's grandparent) for a GoldenEye level's own.
        let mut assets = pd_core::assets::AssetDir::new(crate::ge::repo().join("assets"));
        if let Some(custom) = dir.parent().and_then(Path::parent) {
            assets = assets.with_custom_dir(custom);
        }
        crate::doors::name_models(&mut doors, &crate::doors::model_numbers(&assets)?);
        l.doors = Some(doors);
        l.spawns = Some(spawns);
        l.weapons = Some(weapons);
        l.ammo = Some(ammo);
        l.hills = Some(hills);
        l.bases = Some(bases);
        l.respawns = Some(respawns);
        l.cover = Some(cover);
        l.check()?;
        Ok((l, skipped))
    }

    /// Does the layout place anything (else placement is as without one)?
    pub fn is_empty(&self) -> bool {
        self.spawns.is_none() && self.weapons.is_none() && self.ammo.is_none() && self.hills.is_none() && self.bases.is_none() && self.respawns.is_none() && self.cover.is_none() && self.doors.is_none() && self.waypoints.as_ref().is_none_or(WaypointEdits::is_empty)
    }
}

/// A facing in degrees as a look vector.
pub fn look(facing_deg: f32) -> Vec3 {
    let a = facing_deg.to_radians();
    Vec3::new(a.sin(), 0.0, a.cos())
}

/// A look vector as a facing in degrees (0..360).
pub fn facing(look: Vec3) -> f32 {
    let d = look.x.atan2(look.z).to_degrees();
    if d < 0.0 {
        d + 360.0
    } else {
        d
    }
}

/// To a tenth (of a centimetre, of a degree): enough, and a readable diff.
pub fn round1(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}

/// A look vector as a facing in tenths of a degree, 0..360.
pub(crate) fn facing_tenths(look: Vec3) -> f32 {
    let f = round1(facing(look));
    if f >= 360.0 {
        f - 360.0
    } else {
        f
    }
}
