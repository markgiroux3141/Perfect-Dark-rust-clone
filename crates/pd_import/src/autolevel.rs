//! A level made by the level generator (`autolevel`, `autolevel_build`:
//! docs/AUTOLEVEL.md), as a source of its own. The generator knows what a
//! glTF of its level no longer says: which room every polygon is in, where
//! every doorway, vent, window, drop and grate opens between two rooms, that
//! its materials are Complex's own on the game's own textures. So this source
//! reads no file of the level: it **makes the level again** from the
//! generator's recipe (a seed, options and a person's edits: a level is the
//! same every time it is made, and takes half a second), and hands over
//!
//! - **rooms**: one of PD's for each of the generator's (a hall, a pod, a
//!   corridor's piece, a vent), numbered as the generator numbers them; every
//!   drawn triangle and tile in the room whose air it faces, nothing cut;
//! - **portals**: one for each opening between two rooms
//!   (`autolevel_build::openings`: every face two rooms' air shares), the
//!   rectangle of the opening itself, wound and ordered as PD's
//!   `bg_init_portal` takes them ([`crate::rooms::portal_between`]);
//! - **materials**: the kit's arena's own rows (Complex's: `assets/stages/ref/bg.json`) on the
//!   pool's textures, so the level carries no image, is drawn through PD's
//!   combiner (mip-mapped), clamps as Complex's walls do, and sounds as
//!   Complex does under shots and feet. The generator's palette is Complex's
//!   material list; a row is found by its texture, its culling (`CullBoth`)
//!   and its layer (`Transparent`);
//! - **lighting**: the bake's raw 0-255 corner colours, as they are;
//! - **collision**: every drawn triangle a tile by PD's arenas' rules
//!   ([`crate::source::tile_of_triangle`]) and the three these levels are
//!   built on, always on here (the glTF source's `grates`, `low_ceilings` and
//!   `ramp_walls`): a translucent triangle facing up is a floor, a ceiling
//!   under a standing head is a wall, a wall with a sloped top or bottom is a
//!   ramp wall. A vent's floor is flagged `GEOFLAG_AIBOTCROUCH`, as the floor
//!   of Complex's crawl space is, so simulants crouch through it.
//!
//! The simulants' waypoints are then made as any custom level's and put
//! right with what the generator knows of its own ways ([`crate::autonav`]).
//!
//! What PD does with the rooms and portals, each now as on its own arenas
//! (with a split of the footprint every box neighboured every box beside
//! it): the portal pass draws what a player's doorways show
//! (`bg_tick_portals`, at most 60 rooms a view; a view here takes under 40);
//! a room's brightness settles with the light its portals let in (`dlights`:
//! 210 of 255 in a big hall, up to 255 in a small room off it, as Complex's
//! rooms are), and a shot's flash lights the room and those across its
//! portals; nobody respawns in a room an opponent is in, and avoids its
//! neighbours (`player_choose_spawn_location`); a hill is a room; a simulant
//! faces a target in its room or a neighbouring one (`bot_is_about_to_attack`);
//! chrs, objects and pads are in the room of the floor under them. PD's
//! limits: a room's vertex colours are indexed in 16 bits (65,536 a room; the
//! biggest hall of seeds 1-60 has 18,528), a batch's vertices too.
//!
//! Coordinates: the generator's are Blender's (x east, y north, z up, in PD's
//! units); PD's are (x, z, -y), a turn, so windings stand.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use autolevel::plan::Point3;
use autolevel_build::built::{self, Built};
use autolevel_build::kit::Kit;

use crate::layout::{Door, Motion, Side};
use glam::{Vec2, Vec3};
use pd_core::ids::*;
use serde_json::Value;

use crate::recipe::{AutolevelSource, Recipe};
use crate::rooms::portal_between;
use crate::source::*;

/// The z near of PD's arenas (`g_NoFogEnvironments`), and their usual z far;
/// a bigger level gets its diagonal and a margin.
const ZNEAR: f32 = 15.0;
const ZFAR: f32 = 10000.0;
/// Complex's sky (`g_NoFogEnvironments`' row for `STAGE_MP_COMPLEX`): what
/// shows where nothing is drawn.
const SKY: [u8; 3] = [2, 0, 0];

/// The generator's recipe as text: `src` (a `--src` path) if given, else what
/// the level's recipe says. Either may be the recipe's file or a build folder
/// (its `recipe.json`); the level's recipe may also hold the generator's
/// recipe itself.
pub fn recipe_text(s: &AutolevelSource, src: Option<&Path>) -> Result<String, String> {
    let read = |p: &Path| {
        let file = if p.is_dir() { p.join("recipe.json") } else { p.to_path_buf() };
        std::fs::read_to_string(&file).map_err(|e| format!("{}: {e} (the generator's recipe: autolevel build SEED writes one into its folder)", file.display()))
    };
    match (src, &s.recipe) {
        (Some(p), _) => read(p),
        (None, Value::String(p)) => read(&PathBuf::from(p)),
        (None, v @ Value::Object(_)) => Ok(v.to_string()),
        (None, other) => Err(format!("an autolevel source's recipe is the generator's recipe, or the path of one, not {other}")),
    }
}

/// The level last made here, by its recipe's text: an import makes a level
/// for its geometry and asks for it again for its waypoints, and a recipe's
/// level is the same every time.
static LAST: Mutex<Option<(String, Arc<Built>)>> = Mutex::new(None);

/// The level the source names, made: its layout, room meshes and bake.
pub fn make(s: &AutolevelSource, src: Option<&Path>) -> Result<Arc<Built>, String> {
    let text = recipe_text(s, src)?;
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, b)) = last.as_ref().filter(|(t, _)| *t == text) {
        return Ok(b.clone());
    }
    let recipe = autolevel::recipe::load(&text).map_err(|e| format!("the generator's recipe: {e}"))?;
    let b = Arc::new(built::make(&recipe, true)?);
    *last = Some((text, b.clone()));
    Ok(b)
}

/// A point of the generator's as PD's, times the recipe's scale.
pub fn pd_point(p: Point3, scale: f32) -> Vec3 {
    Vec3::new(p.0 as f32, p.2 as f32, -p.1 as f32) * scale
}

/// The kit's arena's material for each palette entry `used` (places in
/// `kit.palette`), as [`Material`]s in that order.
fn materials(assets: &Path, kit: &Kit, used: &[usize], theme: Option<&crate::recipe::Theme>) -> Result<Vec<Material>, String> {
    let mut stages: std::collections::HashMap<String, (PathBuf, Value)> = std::collections::HashMap::new();
    let mut bg_of = |stage: &str| -> Result<(PathBuf, Value), String> {
        if let Some(v) = stages.get(stage) {
            return Ok(v.clone());
        }
        let path = assets.join("stages").join(stage).join("bg.json");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bg: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        stages.insert(stage.to_owned(), (path.clone(), bg.clone()));
        Ok((path, bg))
    };
    used.iter()
        .map(|&m| {
            let paint = &kit.palette[m];
            // (a theme's texture for the role, in its stage: else the kit's own)
            let (stage, preset, themed) = match theme.and_then(|t| t.roles.get(paint.role).map(|v| (t, v))) {
                Some((t, v)) => match v.split_once(':') {
                    Some((st, tex)) => (st.to_owned(), tex.to_owned(), true),
                    None => (t.stage.clone(), v.clone(), true),
                },
                None => (kit.stage.to_owned(), paint.preset.to_owned(), false),
            };
            let (path, bg) = bg_of(&stage)?;
            let rows = bg["materials"].as_array().ok_or_else(|| format!("{}: no materials", path.display()))?;
            let number = u32::from_str_radix(&preset, 16).map_err(|_| format!("the palette's {}: {preset} is no texture number", paint.role))?;
            let (both, mut clear) = (paint.flags.contains(&"CullBoth"), paint.flags.contains(&"Transparent"));
            // The same texture, culled the same way, in the same layer
            // (PD's translucent materials don't write depth). A theme's
            // texture with no row culled as the role is takes the layer's
            // row with its culling changed (a row's only difference from
            // its twin, as Complex's and G5's are). One its arena draws only
            // in the other layer is drawn in that layer, as the arena draws
            // it: G5's green grate (0x0192) is see-through, so a "grate" deck
            // or open ramp in it is too (a see-through floor is still walked
            // on: `grate_of_triangle`).
            let in_layer = |r: &Value, clear: bool| r["texture"]["id"].as_u64() == Some(number as u64) && (r["zwrite"] == false) == clear;
            let exact = rows.iter().find(|r| in_layer(r, clear) && (r["cull"] == "none") == both);
            let row = match (exact, themed) {
                (Some(r), _) => r.clone(),
                (None, true) => {
                    let mut r = match rows.iter().find(|r| in_layer(r, clear)) {
                        Some(r) => r.clone(),
                        None => {
                            let r = rows.iter().find(|r| in_layer(r, !clear)).ok_or_else(|| format!("{}: no material draws texture {number:#06x} (the theme's {})", path.display(), paint.role))?;
                            clear = !clear;
                            r.clone()
                        }
                    };
                    r["cull"] = Value::from(if both { "none" } else { "back" });
                    r
                }
                (None, false) => return Err(format!("{}: no material draws texture {number:#06x} as the palette's {} does ({})", path.display(), paint.role, paint.flags.join(" "))),
            };
            let row = &row;
            let texture_row = &bg["textures"][number.to_string()];
            let size = |k: &str| texture_row[k].as_u64().map(|v| v as u32).ok_or_else(|| format!("{}: texture {number:#06x} has no {k}", path.display()));
            let wrap = |k: &str| match row["texture"][k].as_u64() {
                Some(1) => Wrap::Clamp,
                Some(2) => Wrap::Mirror,
                _ => Wrap::Repeat,
            };
            Ok(Material {
                texture: None,
                wrap: [wrap("cms"), wrap("cmt")],
                blend: if clear { Blend::Translucent } else { Blend::Opaque },
                cull_back: !both,
                zwrite: !clear,
                decal: false,
                surface: pd_sim::stage::bghit::SURFACETYPE_DEFAULT,
                pd: Some(PdMaterial { texture: number, size: (size("w")?, size("h")?), row: row.clone(), texture_row: texture_row.clone() }),
            })
        })
        .collect()
}

/// What a palette entry's UVs (in repeats of its texture at the kit's size)
/// are multiplied by in a theme: the kit's units to a repeat over the
/// theme's, across and up (1, 1 for a role the theme gives no size).
fn uv_scale(kit: &Kit, entry: usize, theme: Option<&crate::recipe::Theme>) -> (f64, f64) {
    let paint = &kit.palette[entry];
    match theme.and_then(|t| t.scale.get(paint.role)) {
        Some(s) => {
            let (u, v) = s.pair();
            (paint.scale.0 / u, paint.scale.1 / v)
        }
        None => (1.0, 1.0),
    }
}

/// The generator's doors (its doorways that have one) as a layout's: each
/// a copy of the kit's arena's own door of its kind (`doors::adopt` of that
/// stage's setup: Felicity's swinging and sliding doors, rows and all),
/// standing on its doorway's middle plane (the portal) and stretched to the
/// opening; a double doorway's two leaves are a sibling ring, hinged (or
/// sliding) at their outer edges. The front is the doorway's `b` side.
pub fn doors(b: &Built, scale: f32, assets: &Path) -> Result<Vec<Door>, String> {
    let ways: Vec<_> = b.level.doorways.iter().filter_map(|d| d.door.map(|k| (d, k))).collect();
    if ways.is_empty() {
        return Ok(Vec::new());
    }
    let dir = assets.join("stages").join(b.kit.stage);
    let read = |name: &str| -> Result<Value, String> {
        let path = dir.join(name);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    };
    let (own, _) = crate::doors::adopt(&read("pads.json")?, &read("setup.json")?);
    let of = |m: Motion| own.iter().find(|d| d.motion == m).ok_or_else(|| format!("{}: the arena has no {m:?} door to copy", b.kit.stage));
    let mut out: Vec<Door> = Vec::new();
    for (w, k) in ways {
        let t = of(if k.swing { Motion::Swing } else { Motion::Slide })?;
        let across = 1 - w.axis;
        let (a0, a1) = (autolevel::plan::at(w.lo, across) as f32, autolevel::plan::at(w.hi, across) as f32);
        let leaves = k.leaves.max(1) as usize;
        let leaf = (a1 - a0) / leaves as f32;
        // the plan's +x is PD's +x, its +y PD's -z
        let front = if w.axis == 0 { Vec3::X } else { Vec3::NEG_Z };
        let at = |along: f32| {
            let (x, y) = if w.axis == 0 { (w.plane() as f32, along) } else { (along, w.plane() as f32) };
            Vec3::new(x, w.lo.2 as f32, -y) * scale
        };
        let centre = at((a0 + a1) / 2.0);
        let first = out.len();
        for j in 0..leaves {
            let pos = at(a0 + leaf * (j as f32 + 0.5));
            let mut d = t.clone();
            d.pos = [pos.x, pos.y, pos.z].map(crate::layout::round1);
            d.facing = crate::layout::facing(front);
            d.size = [leaf * scale, (w.hi.2 - w.lo.2) as f32 * scale, t.size[2]].map(crate::layout::round1);
            if leaves == 2 {
                // a leaf's hinge is at its pad's -up edge, and a Left door's up
                // is Y x front: the leaf on that side of the middle is Left
                let side = (pos - centre).dot(Vec3::Y.cross(front));
                d.side = if side < 0.0 { Side::Left } else { Side::Right };
                d.sibling = Some(if j == 0 { first + 1 } else { first });
            }
            out.push(d);
        }
    }
    Ok(out)
}

/// The built level as a [`LevelSource`] with its own rooms and portals.
/// `assets` is the game's `assets/` (Complex's materials are read there).
pub fn level_source(r: &Recipe, s: &AutolevelSource, b: &Built, assets: &Path, theme: Option<&crate::recipe::Theme>, report: &mut Vec<String>) -> Result<LevelSource, String> {
    let scale = r.scale;
    let plan = &b.level.plan;
    // PD's rooms are the plan's, by their numbers: 1..=n, each once.
    let nrooms = plan.rooms.len();
    let mut key_of: Vec<Option<&str>> = vec![None; nrooms];
    for room in &plan.rooms {
        match key_of.get_mut(room.room_id.wrapping_sub(1)) {
            Some(slot @ None) => *slot = Some(&room.key),
            _ => return Err(format!("the plan's room {} has number {}, and PD's rooms are 1..={nrooms}, each once", room.key, room.room_id)),
        }
    }
    let number = |key: &str| plan.get(key).map(|room| room.room_id as u16).ok_or_else(|| format!("{key} is no room of the plan"));

    // The palette entries drawn, in the order the rooms first use them.
    let mut used: Vec<usize> = Vec::new();
    for f in b.rooms.iter().flat_map(|room| &room.faces) {
        if !used.contains(&f.material) {
            used.push(f.material);
        }
    }
    let materials = materials(assets, b.kit, &used, theme)?;

    // Every room's air, then everything drawn in it (a ramp's tube is not
    // one of the plan's boxes).
    let mut boxes: Vec<(Vec3, Vec3)> = vec![(Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)); nrooms];
    let mut grow = |room: usize, p: Vec3| boxes[room - 1] = (boxes[room - 1].0.min(p), boxes[room - 1].1.max(p));
    for room in &plan.rooms {
        for (lo, hi) in &room.boxes {
            grow(room.room_id, pd_point(*lo, scale));
            grow(room.room_id, pd_point(*hi, scale));
        }
    }

    let mut tris: Vec<Tri> = Vec::new();
    let mut collision: Vec<ColPoly> = Vec::new();
    let mut ceilings: Vec<(u16, [Vec3; 3])> = Vec::new();
    let mut grates = 0usize;
    for room in &b.rooms {
        let n = number(&room.key)?;
        // A vent's floor is where simulants crouch (`GEOFLAG_AIBOTCROUCH`,
        // as the floor of Complex's crawl space is): a chr touching such a
        // tile is 90 cm tall, fits under the vent's roof, and the waypoint
        // maker then takes vents for ways through.
        let crawl = if plan.get(&room.key).is_some_and(|r| r.kind == "vent") { GEOFLAG_AIBOTCROUCH } else { 0 };
        let pos: Vec<Vec3> = room.verts.iter().map(|v| pd_point((v[0], v[1], v[2]), scale)).collect();
        for &p in &pos {
            grow(n as usize, p);
        }
        for f in &room.faces {
            let material = used.iter().position(|&m| m == f.material).expect("a face's material was gathered");
            let xlu = materials[material].blend == Blend::Translucent;
            let (su, sv) = uv_scale(b.kit, f.material, theme);
            for t in &f.tris {
                let v: [Vert; 3] = t.map(|c| {
                    let (c, col) = (c as usize, f.colors[c as usize]);
                    Vert { pos: pos[f.verts[c] as usize], uv: Vec2::new((f.uvs[c].0 * su) as f32, (f.uvs[c].1 * sv) as f32), col: [col[0], col[1], col[2], 255] }
                });
                tris.push(Tri { v, material, xlu, room: n });
                // The tile it is: PD's arenas' rules, then these levels' own.
                let (p, cols) = (v.map(|x| x.pos), v.map(|x| x.col));
                let tile = match tile_of_triangle(p, cols, xlu) {
                    None if xlu => grate_of_triangle(p, cols).inspect(|_| grates += 1),
                    tile => tile,
                };
                match tile {
                    Some((mut flags, floorcol)) => {
                        if flags & GEOFLAG_FLOOR1 != 0 {
                            flags |= crawl;
                        }
                        // a cellar's ladder (the kit's "ladder", Complex's grille on a wall): climbed,
                        // by players and simulants, as Complex's own is (`GEOFLAG_LADDER`)
                        if b.kit.palette[f.material].role == "ladder" {
                            flags |= GEOFLAG_LADDER;
                        }
                        collision.push(ColPoly { verts: p.to_vec(), flags, floortype: FLOORTYPE_DEFAULT, grab: false, floorcol, room: n })
                    }
                    None if is_ceiling(p) => ceilings.push((n, p)),
                    None => {}
                }
            }
        }
    }
    if tris.is_empty() {
        return Err("the level has no triangles".to_string());
    }
    // PD's boxes are a unit wider than what they hold (Complex's: a room's
    // box against its drawn one).
    let boxes: Vec<(Vec3, Vec3)> = boxes.into_iter().map(|(lo, hi)| if lo.x <= hi.x { (lo - 1.0, hi + 1.0) } else { (Vec3::ZERO, Vec3::ZERO) }).collect();

    // A portal for every opening: the low room behind it, the high in front.
    // PD cannot take one whose rooms' boxes both have their middles on the
    // far side of it (two corridors hooked round each other); the room behind
    // then gets a box wide enough behind the opening to hold its own middle,
    // which PD's boxes may be (none of seeds 1-60 needs it).
    let openings = b.openings();
    let mut boxes = boxes;
    let mut portals = Vec::with_capacity(openings.len());
    let mut widened: Vec<&str> = Vec::new();
    for pass in 0..4 {
        portals.clear();
        let mut again = false;
        for o in &openings {
            let rect = o.corners().map(|p| pd_point(p, scale));
            let (low, high) = (number(&o.low)?, number(&o.high)?);
            match portal_between(low, high, rect, &boxes) {
                Ok(p) => portals.push(p),
                Err(_) if pass < 3 => {
                    widen(&mut boxes[low as usize - 1], rect);
                    widened.push(&o.low);
                    again = true;
                }
                Err(e) => return Err(format!("{} - {}: {e}", o.low, o.high)),
            }
        }
        if !again {
            break;
        }
    }
    if !widened.is_empty() {
        report.push(format!("rooms: the boxes of {} widened behind an opening, so PD takes its portal the right way round", widened.join(", ")));
    }

    let (lo, hi) = tris.iter().flat_map(|t| t.v.iter().map(|v| v.pos)).fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), p| (a.min(p), b.max(p)));
    // (a theme: whose textures the roles it names are drawn with)
    let themed = match theme {
        Some(t) if t.name.is_empty() => format!(", themed from arena {} ({} of the kit's roles)", t.stage, t.roles.len()),
        Some(t) => format!(", in the theme {} (arena {}, {} of the kit's roles)", t.name, t.stage, t.roles.len()),
        None => String::new(),
    };
    report.push(format!(
        "autolevel: seed {}, {} rooms ({} with nothing drawn), {} openings between them, {} materials of the {} kit's (arena {}) on {} pool textures{themed}, {}",
        b.level.seed,
        nrooms,
        nrooms - b.rooms.len(),
        portals.len(),
        materials.len(),
        b.kit.name,
        b.kit.stage,
        b.presets().len(),
        if b.baked { "baked" } else { "not baked" }
    ));
    let mut level = LevelSource {
        textures: Vec::new(),
        materials,
        tris,
        collision,
        markers: Vec::new(),
        env: Env { sky: s.sky.unwrap_or(SKY), near: ZNEAR, far: ZFAR.max((hi - lo).length() * 1.2), fog: None },
        rooms: Some(OwnRooms { boxes, portals }),
    };
    let low = level.wall_low_ceilings(&ceilings);
    let ramps = level.mark_ramp_walls();
    report.push(format!(
        "tiles: {grates} translucent triangles facing up made floors (grates), {low} of {} ceiling triangles under {LOW_CEILING} cm made walls, {ramps} wall triangles flagged as ramp walls",
        ceilings.len()
    ));
    Ok(level)
}

/// Widen a room's box behind the opening `rect` (its corners counter-clockwise
/// seen from in front) until the box's middle is behind it.
fn widen(bx: &mut (Vec3, Vec3), rect: [Vec3; 4]) {
    let n = (rect[1] - rect[0]).cross(rect[2] - rect[0]).normalize_or_zero();
    let over = n.dot((bx.0 + bx.1) * 0.5) - n.dot(rect[0]);
    if over < 0.0 {
        return;
    }
    // The box's face on the side behind the opening moves back by twice
    // what the middle is over, and a unit.
    let k = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap_or(0);
    let by = (2.0 * over + 2.0) / n[k].abs().max(1e-3);
    if n[k] > 0.0 {
        bx.0[k] -= by;
    } else {
        bx.1[k] += by;
    }
}

/// Make the level the source names and convert it, in the theme it names
/// (a name is a file of `themes`: [`crate::themes`]).
pub fn load(r: &Recipe, s: &AutolevelSource, src: Option<&Path>, assets: &Path, themes: &Path, report: &mut Vec<String>) -> Result<LevelSource, String> {
    let theme = s.theme.as_ref().map(|t| crate::themes::resolve(themes, t)).transpose()?;
    let b = make(s, src)?;
    level_source(r, s, &b, assets, theme.as_ref(), report)
}

#[cfg(test)]
mod tests;
