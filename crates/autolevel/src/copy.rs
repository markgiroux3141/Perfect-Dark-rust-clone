//! A room copied with its choices, pasted elsewhere (in this level or another seed's), mirrored,
//! and saved as a preset (the editing roadmap's item 8; new here, the Python has none). The plan
//! is in `docs/AUTOLEVEL.md`, "Lock, copy, paste, mirror and presets".
//!
//! A copy is what the room is relative to itself: its kind, size and height in storeys, the
//! streams it is drawn from (`draws`: a paste is dressed and furnished from the same numbers),
//! its choices (layout, cover, brightness, ceiling, re-rolls, features), the things put in it by
//! place (cells from its low corner), a hall's pods, and the axes it is mirrored on. A paste is
//! recipe edits: an `add`, a `draws`, the choices and puts renamed to the new room and moved to
//! where it is put, a `mirror` for each axis, and the same for each pod.
//!
//! ```text
//! let c = copy(&recipe, &level, "vast")?;
//! let (r, key) = paste(&recipe, &c.mirrored("x"), (12, 40), 0)?;   // "room1", vast flipped
//! let text = save_preset("Pump hall", &c);                           // a preset's file
//! ```

use crate::boxes::{self, Change};
use crate::cells::Rect;
use crate::level::Level;
use crate::lock;
use crate::recipe::{Edit, Recipe};
use crate::scatter::Lot;
use crate::storeys::{index, CEILINGS, LEVELS};
use serde::{Deserialize, Serialize};

/// A room as it is relative to itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    /// The room it was copied from (its edits below still name it).
    pub room: String,
    pub kind: String,
    /// Cells wide and deep.
    pub size: (i32, i32),
    /// How many storeys tall.
    pub storeys: usize,
    /// The floor it stood on (where a paste goes unless told otherwise).
    pub floor: i32,
    /// The name its streams are drawn under ("40/vast").
    pub draws: String,
    /// Its `layout`, `cover`, `brightness`, `ceiling`, `reroll` and `feature` edits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Edit>,
    /// Its `window`, `piece` and `drop` edits, their cells from its low corner, a window's storey
    /// a count of storeys up from its floor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub puts: Vec<Edit>,
    /// How many cells square each piece moved is, by where it was moved to (from the low corner):
    /// what mirroring it needs to know.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<((i32, i32), i32)>,
    /// A hall's pods.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pods: Vec<Pod>,
    /// The axes it is mirrored on ("x", "y"), each at most once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirror: Vec<String>,
}

/// A pod of a hall copied: its low corner from the hall's, its floor in storeys up from the hall's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pod {
    pub at: (i32, i32),
    pub up: usize,
    pub room: Copy,
}

/// The edits a copy carries as its choices.
fn is_choice(e: &Edit) -> bool {
    matches!(e, Edit::Layout { .. } | Edit::Cover { .. } | Edit::Brightness { .. } | Edit::Ceiling { .. } | Edit::Reroll { .. } | Edit::Feature { .. })
}

/// The edits a copy carries as things put by place.
fn is_put(e: &Edit) -> bool {
    matches!(e, Edit::Window { .. } | Edit::Piece { .. } | Edit::Drop { .. })
}

/// The piece whose low corner is on the cell `at` in a room of `lv`: how many cells square.
fn piece_size(lv: &Level, room: &str, at: (i32, i32)) -> Option<i32> {
    let it = lv.interiors.iter().find(|(k, _)| k == room)?;
    it.1.cover.iter().find(|c| c.5 != "wall" && (c.0, c.1) == (2 * at.0, 2 * at.1)).map(|c| ((c.2 - c.0) / 2).max(1))
}

/// `room` of the level `lv` (which `r` makes) as a copy. Err for a room not there, or a pod
/// (copy its hall: its pods come with it).
pub fn copy(r: &Recipe, lv: &Level, room: &str) -> Result<Copy, String> {
    let s = lv.spaces().iter().find(|s| s.key == room).ok_or_else(|| format!("copy: no room {room}"))?;
    if !s.host.is_empty() {
        return Err(format!("copy: {room} is a pod of {}: copy its hall", s.host));
    }
    let lots = r.lots_on(r.scattered()?)?.lots;
    copy_of(r, lv, &lots, room)
}

/// (a room's box and its pods are its lot's and theirs: a pod grown by the generator is grown
/// again where it is pasted, from the same draws)
fn copy_of(r: &Recipe, lv: &Level, lots: &[Lot], room: &str) -> Result<Copy, String> {
    let s = lots.iter().find(|a| a.key == room).ok_or_else(|| format!("copy: no room {room}"))?;
    let o = r.effective()?;
    let (x0, y0) = (s.x0, s.y0);
    let base = index(s.floor) as i32;
    let mut puts = Vec::new();
    let mut sizes = Vec::new();
    for e in r.edits.iter().filter(|e| is_put(e) && e.rooms() == [room]) {
        let mut e = e.clone();
        match &mut e {
            Edit::Window { storey, at, .. } => {
                *storey = index(*storey) as i32 - base;
                *at = (at.0 - x0, at.1 - y0);
            }
            Edit::Piece { at, what, size, to, .. } => {
                if what.as_deref().is_some_and(|w| w != "none") && size.is_none() {
                    *size = piece_size(lv, room, *at);
                }
                if let Some(t) = to {
                    sizes.push(((t.0 - x0, t.1 - y0), piece_size(lv, room, *t).unwrap_or(1)));
                    *t = (t.0 - x0, t.1 - y0);
                }
                *at = (at.0 - x0, at.1 - y0);
            }
            Edit::Drop { at: Some(at), .. } => *at = (at.0 - x0, at.1 - y0),
            _ => {}
        }
        puts.push(e);
    }
    let (mx, my) = o.mirror(room);
    let mirror = [(mx, "x"), (my, "y")].into_iter().filter(|(on, _)| *on).map(|(_, a)| a.to_string()).collect();
    let mut pods = Vec::new();
    // (pods the level left out, with no way in, are not copied)
    for p in lots.iter().filter(|p| p.host == room && lv.spaces().iter().any(|q| q.key == p.key)) {
        let up = (index(p.floor) as i32 - base).max(0) as usize;
        pods.push(Pod { at: (p.x0 - x0, p.y0 - y0), up, room: copy_of(r, lv, lots, &p.key)? });
    }
    Ok(Copy {
        room: room.to_string(),
        kind: s.kind.to_string(),
        size: (s.x1 - s.x0, s.y1 - s.y0),
        // (as `Space::storeys` counts them)
        storeys: LEVELS.iter().filter(|&&z| z >= s.floor && crate::storeys::storey(z).1 <= s.top).count().max(1),
        floor: s.floor,
        draws: o.named(r.seed, room),
        choices: r.edits.iter().filter(|e| is_choice(e) && e.rooms() == [room]).cloned().collect(),
        puts,
        sizes,
        pods,
        mirror,
    })
}

/// A cell flipped across a room `w` cells wide (from its low corner), a thing `n` cells across.
fn flip(x: i32, w: i32, n: i32) -> i32 {
    w - n - x
}

impl Copy {
    /// The copy mirrored on `axis` ("x": east for west, "y": north for south): its pods' boxes and
    /// its puts' cells flipped, and the axis added (or taken off: the same axis twice is none).
    /// Any other axis: as it is.
    pub fn mirrored(&self, axis: &str) -> Copy {
        let on_x = match axis {
            "x" => true,
            "y" => false,
            _ => return self.clone(),
        };
        let w = if on_x { self.size.0 } else { self.size.1 };
        let f = |c: (i32, i32), n: i32| if on_x { (flip(c.0, w, n), c.1) } else { (c.0, flip(c.1, w, n)) };
        let mut out = self.clone();
        for e in &mut out.puts {
            match e {
                Edit::Window { at, .. } => *at = f(*at, 1),
                Edit::Piece { at, size, to, what, .. } => {
                    let stood = what.as_deref().is_some_and(|w| w != "none");
                    if let Some(t) = to {
                        let n = self.sizes.iter().find(|(c, _)| c == t).map_or(1, |(_, n)| *n);
                        *t = f(*t, n);
                    }
                    *at = f(*at, if stood { size.unwrap_or(1) } else { 1 });
                }
                Edit::Drop { at: Some(at), .. } => *at = f(*at, 2),
                _ => {}
            }
        }
        out.sizes = self.sizes.iter().map(|(c, n)| (f(*c, *n), *n)).collect();
        for p in &mut out.pods {
            let n = if on_x { p.room.size.0 } else { p.room.size.1 };
            p.at = f(p.at, n);
            p.room = p.room.mirrored(axis);
        }
        match out.mirror.iter().position(|a| a == axis) {
            Some(i) => drop(out.mirror.remove(i)),
            None => out.mirror.push(axis.to_string()),
        }
        out
    }

    /// One line: "vast (vast, 20 x 16, 3 storeys, 2 pods, 4 choices, mirrored x)".
    pub fn describe(&self) -> String {
        let mut parts = vec![
            self.kind.clone(),
            format!("{} x {}", self.size.0, self.size.1),
            format!("{} storey{}", self.storeys, if self.storeys == 1 { "" } else { "s" }),
        ];
        let n = |n: usize, what: &str| (n > 0).then(|| format!("{n} {what}{}", if n == 1 { "" } else { "s" }));
        parts.extend(n(self.pods.len(), "pod"));
        parts.extend(n(self.choices.len(), "choice"));
        parts.extend(n(self.puts.len(), "thing"));
        if !self.mirror.is_empty() {
            parts.push(format!("mirrored {}", self.mirror.join(" and ")));
        }
        format!("{} ({})", self.room, parts.join(", "))
    }
}

/// An edit of the copy's renamed to `key` and moved by `at` (cells) and `up` (storeys, a window's
/// count from `floor`).
fn placed(e: &Edit, key: &str, at: (i32, i32), floor: usize) -> Edit {
    let mut e = e.clone();
    let mv = |c: &mut (i32, i32)| *c = (c.0 + at.0, c.1 + at.1);
    match &mut e {
        Edit::Layout { room, .. }
        | Edit::Cover { room, .. }
        | Edit::Brightness { room, .. }
        | Edit::Ceiling { room, .. }
        | Edit::Reroll { room, .. }
        | Edit::Feature { room, .. }
        | Edit::Mirror { room, .. } => *room = key.to_string(),
        Edit::Window { room, storey, at, .. } => {
            *room = key.to_string();
            *storey = LEVELS[(floor as i32 + *storey).clamp(0, LEVELS.len() as i32 - 1) as usize];
            mv(at);
        }
        Edit::Piece { room, at, to, .. } => {
            *room = key.to_string();
            mv(at);
            if let Some(t) = to {
                mv(t);
            }
        }
        Edit::Drop { room, at } => {
            *room = key.to_string();
            if let Some(a) = at {
                mv(a);
            }
        }
        _ => {}
    }
    e
}

/// `r` with the copy pasted: its low corner on the cell `at`, its floor on the storey `floor`, as
/// many storeys tall as it was; then its pods, choices, puts and mirrors, each renamed to the new
/// room. Returns the recipe and the new room's key ("room1"). Err for a floor that is no
/// storey's, a height over the highest ceiling, or the lots then breaking the scatter's rule (as
/// an `add`'s are: `Recipe::rooms`).
pub fn paste(r: &Recipe, c: &Copy, at: (i32, i32), floor: i32) -> Result<(Recipe, String), String> {
    let mut out = r.clone();
    let mut p = r.lots_on(r.scattered()?)?;
    let key = add(&mut out, &mut p, c, at, floor)?;
    out.rooms()?;
    Ok((out, key))
}

/// The copy's edits added to `r` (`p`: the lots so far, for new keys), the room first.
fn add(r: &mut Recipe, p: &mut crate::place::Placed, c: &Copy, at: (i32, i32), floor: i32) -> Result<String, String> {
    let i = LEVELS.iter().position(|&z| z == floor).ok_or_else(|| format!("paste: {floor} is no storey's floor (one of {})", list(&LEVELS)))?;
    let top = *CEILINGS.get(i + c.storeys - 1).ok_or_else(|| format!("paste: {} storeys from {floor} go over the highest ceiling", c.storeys))?;
    let key = boxes::new_key(p);
    let rect: Rect = (at.0, at.1, at.0 + c.size.0, at.1 + c.size.1);
    r.edits.push(Edit::Add { key: key.clone(), kind: c.kind.clone(), rect, floor, top });
    r.edits.push(Edit::Draws { room: key.clone(), from: c.draws.clone() });
    p.add(&key, &c.kind, rect, floor, top)?;
    for pod in &c.pods {
        let z = *LEVELS.get(i + pod.up).ok_or_else(|| format!("paste: {}'s pod would be over the highest storey", c.room))?;
        add(r, p, &pod.room, (at.0 + pod.at.0, at.1 + pod.at.1), z)?;
    }
    for e in c.choices.iter().chain(&c.puts) {
        r.edits.push(placed(e, &key, at, i));
    }
    for axis in &c.mirror {
        r.edits.push(Edit::Mirror { room: key.clone(), axis: axis.clone() });
    }
    Ok(key)
}

fn list(zs: &[i32]) -> String {
    zs.iter().map(|z| z.to_string()).collect::<Vec<_>>().join(", ")
}

/// `r` with `room` (of the level `lv`, which `r` makes) mirrored in place on `axis`: its pods
/// moved to the other side (`move` edits, kept short as the plan's are: `boxes::trial`), the
/// cells of the things put in it flipped, and a `mirror` edit set (taken out where the room was
/// mirrored on that axis already). Mirrored twice, the recipe is as it was. Err for a pod, a room
/// not there, a locked room, an axis that is neither, or the pods' moves refused.
pub fn mirror(r: &Recipe, lv: &Level, room: &str, axis: &str) -> Result<Recipe, String> {
    let on_x = match axis {
        "x" => true,
        "y" => false,
        _ => return Err(format!("mirror: axis \"x\" or \"y\", not {axis:?}")),
    };
    let s = lv.spaces().iter().find(|s| s.key == room).ok_or_else(|| format!("mirror: no room {room}"))?;
    if !s.host.is_empty() {
        return Err(format!("mirror: {room} is a pod of {}: mirror its hall", s.host));
    }
    let edit = Edit::Mirror { room: room.to_string(), axis: axis.to_string() };
    if let Some(why) = lock::refusal(r, &edit) {
        return Err(why);
    }
    let base = r.scattered()?;
    let lots = r.lots_on(base.clone())?.lots;
    let s = lots.iter().find(|a| a.key == room).ok_or_else(|| format!("mirror: no room {room}"))?;
    let (lo, hi) = if on_x { (s.x0, s.x1) } else { (s.y0, s.y1) };
    let f = |c: (i32, i32), n: i32| if on_x { (lo + hi - n - c.0, c.1) } else { (c.0, lo + hi - n - c.1) };
    let mut out = r.clone();
    // the pods to the other side (their lots: a grown pod is grown again where it goes)
    for p in lots.iter().filter(|p| p.host == room) {
        let n = if on_x { p.x1 - p.x0 } else { p.y1 - p.y0 };
        let t = boxes::trial(&out, &Change::Move { room: p.key.clone(), to: f((p.x0, p.y0), n) }, &base);
        if let Some(why) = t.refused {
            return Err(format!("mirror: {why}"));
        }
        out = t.recipe;
    }
    // the things put, where they are on the other side
    for e in out.edits.iter_mut().filter(|e| is_put(e) && e.rooms() == [room]) {
        match e {
            Edit::Window { at, .. } => *at = f(*at, 1),
            Edit::Piece { at, size, to, what, .. } => {
                let stood = what.as_deref().is_some_and(|w| w != "none");
                if let Some(t) = to {
                    *t = f(*t, piece_size(lv, room, *t).unwrap_or(1));
                }
                let n = if stood { size.or_else(|| piece_size(lv, room, *at)).unwrap_or(1) } else { 1 };
                *at = f(*at, n);
            }
            Edit::Drop { at: Some(at), .. } => *at = f(*at, 2),
            _ => {}
        }
    }
    // the mirror edit: the same axis again is none
    match out.edits.iter().rposition(|e| *e == edit) {
        Some(i) => drop(out.edits.remove(i)),
        None => out.edits.push(edit),
    }
    Ok(out)
}

/// The format a preset's file is in.
pub const PRESET_FORMAT: u32 = 1;

/// A room saved to use again: a copy with a name, and where it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub preset: u32,
    pub name: String,
    /// Where it was copied from, in words ("seed 40, vast").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    pub room: Copy,
}

/// A preset's file: pretty JSON.
pub fn save_preset(name: &str, from: &str, c: &Copy) -> String {
    let p = Preset { preset: PRESET_FORMAT, name: name.to_string(), from: from.to_string(), room: c.clone() };
    serde_json::to_string_pretty(&p).expect("a preset is plain data") + "\n"
}

/// A preset from its file's text. Err, saying what, for text that is not one.
pub fn load_preset(text: &str) -> Result<Preset, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("preset: {e}"))?;
    match v.get("preset") {
        Some(f) if f.as_u64() == Some(PRESET_FORMAT as u64) => {}
        Some(f) => return Err(format!("preset: format {f} is not one this build reads (format {PRESET_FORMAT})")),
        None => return Err(format!("preset: no \"preset\" (this build reads format {PRESET_FORMAT})")),
    }
    serde_json::from_value(v).map_err(|e| format!("preset: {e}"))
}

/// A file name for a preset's name: "Pump hall!" is "pump_hall".
pub fn file_name(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    let s = s.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_");
    if s.is_empty() {
        "preset".to_string()
    } else {
        s
    }
}
