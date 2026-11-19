//! The Complex (Perfect Dark multiplayer) style kit, the level's measured grammar as code: a port
//! of `ge_pd_tools/styles/complex.py` as far as the generator uses it.
//!
//! Ported: the units and heights, the palette, the wall schemes, `surface`, `style`, `open_ramp`,
//! `ramp_tube`, `glass_rail`, `railing`, `plan_polys` (a plan's geometry) and `plan_lights` with
//! `lamps` (the lights a bake is given). Not ported: the Blender side (`ensure_materials`, `bake`)
//! and the authoring helpers no generated level calls (`wall_surface`, `lifted_style`,
//! `stack_style`, `carver`, `room`, `corridor`, `vent`, `catwalk`, `ramp_corridor`, `grate_deck`).
//!
//! Not as the Python, with the same result: a material is known by its role in the palette (the
//! Python's `mats` maps a role to a Blender material's name, and the key is written with each
//! role mapped to itself), so nothing takes `mats`; and the scheme table, which
//! `autolevel.geometry` swaps into this module while it runs, is a parameter.
//!
//! `KIT` is Complex as a [`Kit`]: the palette, the level's schemes (`geometry::schemes`), the
//! solid tags, tints and brightness targets. The functions here take the kit whose palette they
//! draw roles from; they are Complex's rules, and any kit given them must have the roles they use.

use crate::csg::{self, f3, oriented, Carver, Face, Lattice, Poly, Surface, P3};
use crate::kit::{scheme, strip, Kit, Paint, Scheme, Schemes};
use autolevel::plan::{Plan, Ramp, Room};
use autolevel::py;
use std::collections::HashMap;

// ------------------------------------------------------------------ units
/// Complex was authored in metres: 1 m = 39.37 units.
pub const METRE: f64 = 39.37;
/// 3 m: corridor width, wall strip height, room-size step.
pub const MODULE: f64 = 118.11;

/// Metres to units.
pub fn m(metres: f64) -> i32 {
    py::round(metres * METRE) as i32
}

/// The position of grid line n (3 m cells). Rounded from 118.11, like Complex, so long runs
/// stay on the metre lattice instead of drifting.
pub fn cells(n: f64, origin: f64) -> i32 {
    py::round(origin + n * MODULE) as i32
}

// ---------------------------------------------------------------- heights
/// Floor to ceiling, 6 m.
pub const STOREY: i32 = 236;
/// Ceiling to the floor above, 1 m.
pub const SLAB: i32 = 40;
/// Floor to floor, 7 m.
pub const LEVEL: i32 = 276;
pub const FLOORS: [(&str, i32); 5] = [("basement", -276), ("pit", -118), ("ground", 0), ("upper", 276), ("top", 551)];
/// Multi-storey rooms (12, 13, 16, 19 m).
pub const TALL_CEILINGS: [i32; 4] = [473, 512, 630, 748];
pub const BAND_LINES: [i32; 14] = [-276, -157, -118, -39, 0, 118, 236, 276, 394, 512, 551, 630, 748, 787];

// --------------------------------------------------------------- features
/// The standard opening: full storey height, no lintel.
pub const DOOR: (i32, i32) = (118, 236);
/// The other common opening.
pub const DOOR_WIDE: (i32, i32) = (167, 236);
/// A square crawl duct, floor at a storey floor.
pub const VENT: i32 = 118;
/// A vent's sloped run is this tall: a low ceiling is a wall to PD's collision (tested by its
/// bounding box), so a sloping one 118 up blocks the crawl. At 172 or more it is none.
pub const VENT_SLOPE: i32 = 177;
/// A ramp of one storey, 30.3 degrees: its rise, run and width.
pub const RAMP: (i32, i32, i32) = (276, 472, 118);
/// The railing on catwalk edges (276..362).
pub const RAIL_HEIGHT: i32 = 86;
/// Units along a railing to one repeat of its texture (Complex's own railings: 0x190's 32 texels
/// to about 78 units; the glass strips' 88, the same texture, is used for both).
pub const RAIL_REPEAT: f64 = 88.0;
/// The edge strip under open ramps and catwalks.
pub const GRATE_EDGE: i32 = 16;
/// The glass strip's height (0..94 above a floor).
pub const WINDOW: i32 = 94;
/// Room_18's catwalk balustrade: window glass, 276..358.
pub const GLASS_RAIL: i32 = 82;

// ---------------------------------------------------------------- palette
const fn paint(role: &'static str, preset: &'static str, flags: &'static [&'static str], name: &'static str, scale: f64) -> Paint {
    Paint { role, preset, flags, name, scale: (scale, scale) }
}

const SEE_THROUGH: &[&str] = &["Transparent", "CullBoth", "ClampT"];

/// The kit's materials, in the Python's order, and the Rust's after them.
///
/// "rail" was the Python's 0x748, a blue perforated mesh that Complex hangs on one catwalk only;
/// the user (2026-11-19): a railing is Complex's metal one, 0x190 (the glass strips' texture, which
/// Complex runs along nearly every catwalk and ledge). The mesh is kept for the walkable floor
/// grates ("mesh"), which are floors, not railings.
pub const PALETTE: [Paint; 20] = [
    paint("floor", "073E", &[], "m0", 39.37),
    paint("grate", "073E", &["CullBoth"], "m14CullBoth", 39.37),
    paint("base_pipe", "0740", &["ClampT"], "m8ClampT", 118.11),
    paint("plain", "0746", &["ClampT"], "m3ClampT", 118.11),
    paint("pipes", "0742", &["ClampT"], "m7ClampT", 118.11),
    paint("panel", "0741", &["ClampT"], "m1ClampT", 118.11),
    paint("panel_stripe", "0743", &["ClampT"], "m2ClampT", 118.11),
    paint("panel_pipes", "0744", &["ClampT"], "m5ClampT", 118.11),
    paint("door_panel", "073F", &["ClampT"], "m6ClampT", 118.11),
    paint("grey_panel", "073C", &["ClampT"], "m9ClampT", 118.11),
    paint("grey_pipe", "073D", &["ClampT"], "m10ClampT", 118.11),
    paint("grille", "0745", &[], "m16", 118.11),
    paint("ceiling_tile", "0749", &[], "m4", 59.06),
    paint("ceiling_dark", "073B", &[], "m15", 59.06),
    paint("vent", "0747", &[], "m11", 59.06),
    paint("water", "090F", &[], "m12", 157.0),
    paint("water_surface", "090F", &["Transparent"], "m17Transparent", 197.0),
    Paint { role: "window", preset: "0190", flags: SEE_THROUGH, name: "m13TransparentCullBothClampT", scale: (88.0, 94.0) },
    Paint { role: "rail", preset: "0190", flags: SEE_THROUGH, name: "m18TransparentCullBothClampT", scale: (RAIL_REPEAT, 86.0) },
    Paint { role: "mesh", preset: "0748", flags: SEE_THROUGH, name: "m19TransparentCullBothClampT", scale: (43.0, 86.0) },
];

/// Complex, as the generator dresses its levels.
pub static KIT: Kit = Kit {
    name: "complex",
    tag: "cx",
    stage: "ref",
    palette: &PALETTE,
    schemes: crate::geometry::schemes,
    polys: crate::geometry::polys,
    tags: &TAGS,
    tints: &TINTS,
    target: &TARGET,
};

/// The kit's schemes: Complex's most common vertical pairings (`SCHEMES`).
pub fn schemes() -> Schemes {
    let scheme = |strips: [&'static str; 9]| -> Scheme {
        let below = [(-276, "plain"), (-157, "pipes"), (-118, "pipes"), (-39, "pipes")];
        below.into_iter().chain([0, 118, 236, 276, 394, 512, 551, 630, 748].into_iter().zip(strips)).collect()
    };
    vec![
        ("standard", scheme(["base_pipe", "panel", "plain", "panel", "panel_stripe", "plain", "plain", "grille", "plain"])),
        ("striped", scheme(["base_pipe", "panel_stripe", "plain", "panel", "panel_pipes", "panel", "panel", "grille", "plain"])),
        ("plain", scheme(["plain", "panel", "plain", "plain", "panel", "plain", "plain", "panel", "plain"])),
        ("pipework", scheme(["base_pipe", "pipes", "plain", "panel", "panel_pipes", "plain", "plain", "grille", "plain"])),
    ]
}

/// The scheme of this name, where the Python would stop with a KeyError without it.
fn scheme_must<'a>(schemes: &'a Schemes, name: &str) -> Result<&'a Scheme, String> {
    scheme(schemes, name).ok_or_else(|| format!("no wall scheme {}", py::repr(name)))
}

/// The strip base at or below `z` (`strip_base`).
pub fn strip_base(z: i32) -> Option<i32> {
    BAND_LINES[..BAND_LINES.len() - 1].iter().copied().filter(|&b| b <= z).max()
}

// ------------------------------------------------------------------ style

/// A csg style callback in Complex's grammar.
///
/// `zones`: (zone, scheme name | "vent" | "water") overrides per zone (the last of a zone wins).
/// `tags`: (solid tag, role) for special solids. Built-in tags: "trim" (slab edges, catwalk
/// edges: plain trim strip), "grate" (catwalk decks), "column" (door_panel/grille alternating).
/// Floors are diamond plate (water at the pit level in "water" zones), ceilings at storey heights
/// (236, 512) are plain panels, atrium ceilings (630+) are tiles, and undersides of trim/grate
/// solids are dark ceiling panels. `lines`: the strip base heights, where a level has its own
/// (None: `BAND_LINES` less its last). Roles are `kit`'s.
pub fn style<'a>(
    kit: &'a Kit,
    scheme: &'a str,
    zones: &[(&'a str, &'a str)],
    tags: &'a [(&'a str, &'a str)],
    lines: Option<&[i32]>,
    schemes: &'a Schemes,
) -> impl Fn(&Face) -> Result<Surface, String> + 'a {
    let zones: HashMap<&str, &str> = zones.iter().copied().collect();
    let mut bases: Vec<i32> = lines.map_or_else(|| BAND_LINES[..BAND_LINES.len() - 1].to_vec(), |l| l.to_vec());
    bases.sort();
    let pit = FLOORS[1].1;
    move |f| {
        let strip_base = |z: i32| -> Result<i32, String> {
            let first = *bases.first().ok_or("a style with no strip lines")?;
            Ok(bases.iter().copied().filter(|&b| b <= z).max().unwrap_or(first))
        };
        let band = |z: i32| Some(z as f64);
        let mode = zones.get(f.zone).copied().unwrap_or(scheme);
        if mode == "vent" {
            return kit.surface("vent", None, None);
        }
        if f.kind() == "floor" {
            if mode == "water" && f.lo[2] <= pit {
                return kit.surface("water", None, None);
            }
            return kit.surface(if f.tag == "grate" { "grate" } else { "floor" }, None, None);
        }
        if f.kind() == "ceiling" {
            if f.tag == "trim" || f.tag == "grate" {
                return kit.surface("ceiling_dark", None, None);
            }
            let tile = f.lo[2] >= 630; // atria; storey ceilings (236, 512) are plain in Complex
            return kit.surface(if tile { "ceiling_tile" } else { "plain" }, None, if tile { None } else { Some((118.11, 236.22)) });
        }
        let z0 = f.lo[2];
        if f.tag == "trim" || f.tag == "grate" {
            return kit.surface("plain", band(z0), None);
        }
        if let Some((_, role)) = tags.iter().find(|(t, _)| *t == f.tag) {
            return kit.surface(role, band(strip_base(z0)?), None);
        }
        let b = strip_base(z0)?;
        if f.tag == "column" {
            return kit.surface(if b.div_euclid(118).rem_euclid(2) != 0 { "grille" } else { "door_panel" }, band(b), None);
        }
        let table = scheme_must(schemes, if mode == "water" { "standard" } else { mode })?;
        kit.surface(strip(table, b).unwrap_or("plain"), band(b), None)
    }
}

// ---------------------------------------------------------------- building

/// Complex's open ramp: a double-sided grate sheet with an edge strip on each open side (-1 / 1:
/// the low / high coordinate side). `start` and `end` are centre-line points sharing x or y.
/// Keep it at about 30 degrees.
pub fn open_ramp(kit: &Kit, start: P3, end: P3, zone: &str, width: f64, open_sides: &[i32]) -> Result<Vec<Poly>, String> {
    Ok(csg::plate_ramp(start, end, width, zone, kit.surface("grate", None, None)?, GRATE_EDGE as f64, open_sides))
}

/// An enclosed ramp corridor the way Complex builds them (rooms 0C, 11, 2B): a sloped tube with
/// no hidden faces. Floor and ceiling are one sloped quad each, one storey apart, and each wall
/// is two 118-high ClampT strips that run parallel to the slope (the scheme's 0 and 118 strips;
/// the "plain" scheme's if there is none of that name). The ends are open: leave matching
/// portals in the rooms they join (`plan_polys` does).
pub fn ramp_tube(kit: &Kit, start: P3, end: P3, zone: &str, scheme_name: &str, width: f64, schemes: &Schemes) -> Result<Vec<Poly>, String> {
    let run = if start[0] != end[0] { 0 } else { 1 };
    let side = 1 - run;
    let hw = width / 2.0;
    let at = |p: P3, off: f64, up: f64| -> P3 {
        let mut q = p;
        q[side] += off;
        q[2] += up;
        q
    };
    let plain = scheme_must(schemes, "plain")?;
    let table = scheme(schemes, scheme_name).unwrap_or(plain);
    let storey = STOREY as f64;
    let mut out = vec![
        oriented(
            &[at(start, -hw, 0.0), at(start, hw, 0.0), at(end, hw, 0.0), at(end, -hw, 0.0)],
            [0.0, 0.0, 1.0],
            kit.surface("floor", None, None)?,
            zone,
            None,
        ),
        oriented(
            &[at(start, -hw, storey), at(start, hw, storey), at(end, hw, storey), at(end, -hw, storey)],
            [0.0, 0.0, -1.0],
            kit.surface("plain", None, Some((MODULE, MODULE)))?,
            zone,
            None,
        ),
    ];
    let role = |z: i32| strip(table, z).ok_or_else(|| format!("the wall scheme has no strip at {z}"));
    let strips = [(0, role(0)?), (1, role(118)?)];
    for off in [-hw, hw] {
        let mut n = [0.0; 3];
        n[side] = if off < 0.0 { 1.0 } else { -1.0 }; // walls face into the tube
        let (rx, ry) = (-n[1], n[0]);
        let along = |p: P3| p[0] * rx + p[1] * ry;
        let d = along(end) - along(start);
        let slope = (end[2] - start[2]) / if d != 0.0 { d } else { 1.0 };
        for (k, role) in strips {
            let band = start[2] + k as f64 * MODULE - slope * along(at(start, off, 0.0));
            let p = kit.paint(role)?;
            let surf = Surface { material: p.role, scale: p.scale, band: Some(py::round_to(band, 3)), slope, offset: (0.0, 0.0) };
            let (z0, z1) = ((k * 118) as f64, ((k + 1) * 118) as f64);
            out.push(oriented(&[at(start, off, z0), at(end, off, z0), at(end, off, z1), at(start, off, z1)], n, surf, zone, None));
        }
    }
    Ok(out)
}

/// Room_18's railing: a double-sided strip of window glass along a catwalk or ramp edge. p0 and
/// p1 carry their own z, so it can follow a slope (explicit UVs keep the texture parallel to it).
pub fn glass_rail(kit: &Kit, p0: P3, p1: P3, zone: &str, facing: P3, height: f64) -> Result<Vec<Poly>, String> {
    let low = if p1[2] < p0[2] { p1[2] } else { p0[2] };
    Ok(csg::rail(p0, p1, height, zone, kit.surface("window", Some(low), Some((88.0, height)))?, facing, Some(88.0)))
}

/// A railing along a catwalk edge, `RAIL_HEIGHT` tall, at p0's height or at `level`.
pub fn railing(kit: &Kit, p0: P3, p1: P3, zone: &str, facing: P3, level: Option<f64>) -> Result<Vec<Poly>, String> {
    let z = level.unwrap_or(p0[2]);
    let height = RAIL_HEIGHT as f64;
    Ok(csg::rail([p0[0], p0[1], z], [p1[0], p1[1], z], height, zone, kit.surface("rail", Some(z), Some((RAIL_REPEAT, height)))?, facing, None))
}

// ------------------------------------------------------------- from a plan
/// Solid tags used by level plans, beyond the built-in ones: (tag, role).
pub const TAGS: [(&str, &str); 4] = [("core", "door_panel"), ("housing", "grey_panel"), ("tank", "pipes"), ("ledge", "pipes")];

/// How finely `plan_polys` splits faces, and what it leaves out (the Python's keyword arguments).
#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    /// The lattice faces are split on (fewer splits = fewer triangles, softer lighting).
    pub lattice: Lattice,
    pub max_span: i32,
    /// Vents' own lattice, so their lighting has vertices.
    pub vent_step: f64,
    /// false leaves out the open sheets (grate and plate ramps, railings), so the rest can be
    /// checked for watertightness.
    pub extras: bool,
    /// The heights walls are split on, and strips anchored on (None: `BAND_LINES`). A level with
    /// rooms open from -276 up through 0 leaves out -118, the pit line, and may add storeys.
    pub lines: Option<Vec<i32>>,
}

impl Default for Detail {
    fn default() -> Self {
        Detail { lattice: [None; 3], max_span: 236, vent_step: 59.06, extras: true, lines: None }
    }
}

/// The plan's own style, as `Restyle` is given it.
pub type Base<'a> = dyn Fn(&Face) -> Result<Surface, String> + 'a;
/// `restyle(face, base) -> Surface` overrides the look of chosen faces; `base` is the plan's own
/// style (call it for the rest).
pub type Restyle<'a> = dyn Fn(&Face, &Base) -> Result<Surface, String> + 'a;

/// Geometry for a plan in Complex's grammar: every room carved into one `Carver` (zone = room
/// key), enclosed ramps as `ramp_tube`s with portals where they join, open and plate ramps, and
/// railings. A vent's "duct" ramps are sloped runs (`csg::duct`, `VENT_SLOPE` high) between two
/// of its flat boxes, with `VENT`-high portals and a cap over each.
///
/// `rooms`: keys to build (None: all). Doors to rooms left out stay walled, so a level can be
/// built one stage at a time. Roles are `kit`'s, and so are the solid tags.
pub fn plan_polys(plan: &Plan, kit: &Kit, rooms: Option<&[&str]>, how: &Detail, schemes: &Schemes, restyle: Option<&Restyle>) -> Result<Vec<Poly>, String> {
    let chosen: Vec<&Room> = plan.rooms.iter().filter(|r| rooms.is_none_or(|keys| keys.contains(&r.key.as_str()))).collect();
    let lines = how.lines.as_deref();
    let mut c = Carver::new(how.lattice, how.max_span);
    c.zone_lattice = chosen.iter().filter(|r| r.kind == "vent").map(|r| (r.key.clone(), ([Some(how.vent_step); 3], [0.0; 3]))).collect();
    c.add_lines(2, lines.unwrap_or(&BAND_LINES));
    let mut tubes: Vec<(&Room, &Ramp)> = Vec::new();
    for r in &chosen {
        let mut boxes = &r.boxes[..];
        if let Some(rp) = r.ramps.iter().find(|rp| rp.kind == "enclosed") {
            boxes = &boxes[1..]; // by convention the ramp's box comes first
            tubes.push((r, rp));
        }
        for (lo, hi) in boxes {
            c.carve(f3(*lo), f3(*hi), &r.key);
        }
    }
    for r in &chosen {
        for (lo, hi, tag) in &r.solids {
            c.solid(f3(*lo), f3(*hi), tag);
        }
    }
    let ducts: Vec<(&Room, &Ramp)> = chosen.iter().flat_map(|r| r.ramps.iter().filter(|rp| rp.kind == "duct").map(move |rp| (*r, rp))).collect();
    for ((_, rp), height) in tubes.iter().map(|t| (t, STOREY)).chain(ducts.iter().map(|t| (t, VENT))) {
        let (start, end) = (f3(rp.start), f3(rp.end));
        let run = if start[0] != end[0] { 0 } else { 1 };
        let side = 1 - run;
        for p in [start, end] {
            let (mut lo, mut hi) = (p, p);
            lo[side] = p[side] - rp.width as f64 / 2.0;
            hi[side] = p[side] + rp.width as f64 / 2.0;
            hi[2] = p[2] + height as f64;
            c.portal(lo, hi)?;
        }
    }
    let zones: Vec<(&str, &str)> = chosen.iter().map(|r| (r.key.as_str(), if r.kind == "vent" { "vent" } else { r.scheme })).collect();
    let base = style(kit, "standard", &zones, kit.tags, lines, schemes);
    let mut polys = match restyle {
        None => c.polygons(|f| base(f).map(Some), false)?,
        Some(restyle) => c.polygons(|f| restyle(f, &base).map(Some), false)?,
    };
    for (r, rp) in &tubes {
        polys.extend(ramp_tube(kit, f3(rp.start), f3(rp.end), &r.key, r.scheme, rp.width as f64, schemes)?);
    }
    for (r, rp) in &ducts {
        let v = kit.surface("vent", None, None)?;
        let (start, end, width) = (f3(rp.start), f3(rp.end), rp.width as f64);
        polys.extend(csg::duct(start, end, width, VENT_SLOPE as f64, &r.key, [v; 3], how.vent_step));
        let run = if start[0] != end[0] { 0 } else { 1 };
        for (p, q) in [(start, end), (end, start)] {
            // over each mouth, up to the run's ceiling
            let mut n = [0.0; 3];
            n[run] = if q[run] > p[run] { 1.0 } else { -1.0 };
            let (mut a, mut b) = (p, p);
            a[1 - run] -= width / 2.0;
            b[1 - run] += width / 2.0;
            let (z0, z1) = (p[2] + VENT as f64, p[2] + VENT_SLOPE as f64);
            polys.push(oriented(&[[a[0], a[1], z0], [b[0], b[1], z0], [b[0], b[1], z1], [a[0], a[1], z1]], n, v, &r.key, None));
        }
    }
    if how.extras {
        for r in &chosen {
            for rp in &r.ramps {
                if rp.kind == "open" {
                    polys.extend(open_ramp(kit, f3(rp.start), f3(rp.end), &r.key, rp.width as f64, &rp.open_sides)?);
                } else if rp.kind == "plate" {
                    polys.extend(csg::plate_ramp(
                        f3(rp.start),
                        f3(rp.end),
                        rp.width as f64,
                        &r.key,
                        kit.surface("floor", None, None)?,
                        GRATE_EDGE as f64,
                        &rp.open_sides,
                    ));
                }
            }
            for (p0, p1, facing) in &r.rails {
                polys.extend(railing(kit, f3(*p0), f3(*p1), &r.key, f3(*facing), None)?);
            }
        }
    }
    csg::fix_t_junctions(&mut polys);
    Ok(polys)
}

/// A light for the bake (the arguments of the Python's `build.Light`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub pos: P3,
    /// Raw 0-255 colour.
    pub color: [f64; 3],
    pub radius: f64,
    /// The lamp's size, for soft shadows (0: a point).
    pub size: f64,
    /// How far it is a downward spot (0: it shines all round).
    pub spot: f64,
}

/// Neutral lights for a plan: downward lamps on a 3-module grid under every room ceiling (and
/// under catwalk decks), plain lights along corridors and ramps, and dimmer ones in vents. The
/// bake then rescales each room to Complex's floor brightness. `rooms`: keys to light (None: all).
pub fn plan_lights(plan: &Plan, rooms: Option<&[&str]>, spot: f64) -> Vec<Light> {
    let mut out = Vec::new();
    let plain = |pos: P3, color: f64, radius: f64| Light { pos, color: [color; 3], radius, size: 0.0, spot: 0.0 };
    let along_box = |out: &mut Vec<Light>, lo: P3, hi: P3, z: f64, step: f64, color: f64, radius: f64| {
        let (dx, dy) = (hi[0] - lo[0], hi[1] - lo[1]);
        let n = py::round((if dy > dx { dy } else { dx }) / step).max(1);
        for i in 0..n {
            let t = (i as f64 + 0.5) / n as f64;
            let x = lo[0] + dx * if dx >= dy { t } else { 0.5 };
            let y = lo[1] + dy * if dy > dx { t } else { 0.5 };
            out.push(plain([x, y, z], color, radius));
        }
    };
    for r in plan.rooms.iter().filter(|r| rooms.is_none_or(|keys| keys.contains(&r.key.as_str()))) {
        if r.kind == "vent" {
            for (lo, hi) in &r.boxes {
                along_box(&mut out, f3(*lo), f3(*hi), (lo.2 + 59) as f64, MODULE * 2.0, 170.0, MODULE * 3.0);
            }
            for rp in r.ramps.iter().filter(|rp| rp.kind == "duct") {
                let (s, e) = (f3(rp.start), f3(rp.end));
                let mid: P3 = std::array::from_fn(|k| (s[k] + e[k]) / 2.0);
                out.push(plain([mid[0], mid[1], mid[2] + 59.0], 170.0, MODULE * 3.0));
            }
            continue;
        }
        let enclosed: Vec<&Ramp> = r.ramps.iter().filter(|rp| rp.kind == "enclosed").collect();
        for rp in &enclosed {
            let (s, e) = (f3(rp.start), f3(rp.end));
            let run = if s[0] != e[0] { 0 } else { 1 };
            let n = py::round((e[run] - s[run]).abs() / (MODULE * 2.0)).max(1);
            for i in 0..n {
                let t = (i as f64 + 0.5) / n as f64;
                let p: P3 = std::array::from_fn(|k| s[k] + (e[k] - s[k]) * t);
                out.push(plain([p[0], p[1], p[2] + m(5.0) as f64], 200.0, MODULE * 4.0));
            }
        }
        for (lo, hi) in &r.boxes[if enclosed.is_empty() { 0 } else { 1 }..] {
            let (lo, hi) = (f3(*lo), f3(*hi));
            let (dx, dy, h) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
            if dx * dy < 1.5 * MODULE * MODULE {
                continue; // door stubs: lit by their rooms
            }
            if r.kind == "corridor" || (if dy < dx { dy } else { dx }) < MODULE * 1.5 {
                along_box(&mut out, lo, hi, lo[2] + m(5.0) as f64, MODULE * 2.5, 200.0, MODULE * 4.0);
            } else {
                let reach = h + MODULE * 3.0;
                let radius = if reach > MODULE * 7.0 { reach } else { MODULE * 7.0 };
                out.extend(lamps(lo, hi, hi[2] - METRE, &Lamps { radius, spot, ..Lamps::default() }));
            }
        }
        for (lo, hi, tag) in &r.solids {
            // (a Felicity level's walkways: `autolevel::balcony`)
            if (*tag == "trim" || *tag == "walkway") && lo.2 > r.floor + 100 {
                // under catwalks and balconies
                out.extend(lamps(f3(*lo), f3(*hi), lo.2 as f64 - METRE, &Lamps { strength: 150.0, radius: MODULE * 4.0, spot, ..Lamps::default() }));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- lighting
/// Complex is lit almost entirely in neutral grey. Floors are brightest (median 167), walls 131,
/// ceilings 86, with strong pools: (kind, the median brightness to bake to).
pub const TARGET: [(&str, i32); 3] = [("floor", 167), ("wall", 131), ("ceiling", 86)];
/// Whole-room tints (cyan and yellow are Complex's corridors 0E and 11; red is not in Complex).
pub const TINTS: [(&str, [f64; 3]); 3] = [("cyan", [0.5, 1.0, 1.0]), ("yellow", [1.0, 1.0, 0.7]), ("red", [1.0, 0.5, 0.5])];

/// How `lamps` are spaced and what they are (the Python's keyword arguments).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamps {
    pub spacing: f64,
    pub strength: f64,
    pub radius: f64,
    pub size: f64,
    pub spot: f64,
}

impl Default for Lamps {
    fn default() -> Self {
        Lamps { spacing: MODULE * 3.0, strength: 230.0, radius: MODULE * 7.0, size: 20.0, spot: 1.0 }
    }
}

/// Neutral recessed ceiling lamps on a regular grid over the box lo..hi (x, y), `z` just under
/// the ceiling. They shine downward (spot), which keeps ceilings dark and floors in pools, as in
/// Complex.
pub fn lamps(lo: P3, hi: P3, z: f64, how: &Lamps) -> Vec<Light> {
    let mut out = Vec::new();
    let nx = py::round((hi[0] - lo[0]) / how.spacing).max(1);
    let ny = py::round((hi[1] - lo[1]) / how.spacing).max(1);
    for i in 0..nx {
        for j in 0..ny {
            let pos = [lo[0] + (i as f64 + 0.5) * (hi[0] - lo[0]) / nx as f64, lo[1] + (j as f64 + 0.5) * (hi[1] - lo[1]) / ny as f64, z];
            out.push(Light { pos, color: [how.strength; 3], radius: how.radius, size: how.size, spot: how.spot });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_the_palette() {
        assert_eq!((m(5.0), cells(4.0, 0.0), cells(2.5, 0.0)), (197, 472, 295));
        assert_eq!((KIT.paint("rail").unwrap().preset, KIT.paint("rail").unwrap().scale), ("0190", (88.0, 86.0)));
        assert_eq!(KIT.paint("mesh").unwrap().preset, "0748");
        assert_eq!(KIT.surface("plain", Some(118.0), None).unwrap(), Surface::new("plain", (118.11, 118.11), Some(118.0)));
        assert_eq!(strip(scheme(&schemes(), "striped").unwrap(), 394), Some("panel_pipes"));
        assert_eq!((strip_base(120), strip_base(787), strip_base(-300)), (Some(118), Some(748), None));
    }

    #[test]
    fn a_wall_takes_its_schemes_strip_and_special_solids_their_own() {
        let table = schemes();
        let fn_ = style(&KIT, "standard", &[("duct", "vent"), ("pool", "water")], &TAGS, None, &table);
        let surface = |role, band, scale| KIT.surface(role, band, scale);
        let face = |axis, sign, z, zone, tag| Face { axis, sign, lo: [0, 0, z], hi: [10, 0, z + 10], zone, tag };
        assert_eq!(fn_(&face(0, 1, 130, "hall", "void")).unwrap(), surface("panel", Some(118.0), None).unwrap());
        assert_eq!(fn_(&face(0, 1, 130, "hall", "trim")).unwrap(), surface("plain", Some(130.0), None).unwrap());
        assert_eq!(fn_(&face(0, 1, 130, "hall", "tank")).unwrap(), surface("pipes", Some(118.0), None).unwrap());
        assert_eq!(fn_(&face(0, 1, 130, "hall", "column")).unwrap(), surface("grille", Some(118.0), None).unwrap());
        assert_eq!(fn_(&face(0, 1, 250, "hall", "column")).unwrap(), surface("door_panel", Some(236.0), None).unwrap());
        // -118 // 118 is -1, and -1 % 2 is 1
        assert_eq!(fn_(&face(0, 1, -100, "hall", "column")).unwrap(), surface("grille", Some(-118.0), None).unwrap());
        assert_eq!(fn_(&face(0, 1, 130, "duct", "void")).unwrap().material, "vent");
        assert_eq!(fn_(&face(2, 1, -118, "pool", "void")).unwrap().material, "water");
        assert_eq!(fn_(&face(2, -1, 236, "hall", "void")).unwrap(), surface("plain", None, Some((118.11, 236.22))).unwrap());
        assert_eq!(fn_(&face(2, -1, 630, "hall", "void")).unwrap().material, "ceiling_tile");
        let other = style(&KIT, "mossy", &[], &TAGS, None, &table);
        assert_eq!(other(&face(0, 1, 0, "hall", "void")).unwrap_err(), "no wall scheme 'mossy'");
    }
}
