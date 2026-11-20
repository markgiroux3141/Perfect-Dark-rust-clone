//! A generated level's polygons, in Complex's grammar: a port of
//! `ge_pd_tools/autolevel/geometry.py`, held to the answer key (`tests/key/polys.json`).
//!
//! ```text
//! let (polys, lit) = geometry::polys(&lv, &complex::KIT)?;   // every polygon, and the beams that light some
//! polys[i].lit                                // Some(b): lit[b].beam lights this polygon
//! ```
//!
//! Everything the plan can't say for itself is here: the wall schemes over all the level's
//! storeys, what the kit's plan style doesn't know (`restyle`), the beams of light through floor
//! grates and drops, the grates, the glass and the ramps' rails. A material is its role in
//! the kit's palette (`kit::Kit`). `planned` is the level as `complex::plan_polys` leaves it, and `finish`
//! the rest, so the two can be told apart (the key has a digest at each).

use crate::complex::{self as cx, Detail};
use crate::csg::{self, f3, Face, Poly, Surface};
use crate::kit::{self, Kit, Scheme, Schemes};
use crate::sunbeam::{self, Beam};
use autolevel::furnish::WINDOW;
use autolevel::level::Level;
use autolevel::plan::cells as C;
use autolevel::py;
use std::collections::HashMap;

/// Storey lines: every floor of `storeys::LEVELS`, each with a strip line 118 up and its ceiling
/// 236 up. (The Python's stop at 1654; the storeys over it are an atrium's, and no wall of a
/// seed's own level reaches them, so they split and dress nothing there.)
pub const FLOORS: [i32; 17] = autolevel::storeys::LEVELS;

/// The heights walls are split on, and strips anchored on (`LINES`).
pub fn lines() -> Vec<i32> {
    [-276, -157, -39].into_iter().chain(FLOORS[1..].iter().flat_map(|z| [0, 118, 236].map(|k| z + k))).collect()
}

/// How finely the level's faces are split (`DETAIL`, with `LINES`).
pub fn detail() -> Detail {
    Detail { lattice: [Some(236.22), Some(236.22), None], max_span: 236, vent_step: 118.11, extras: true, lines: Some(lines()) }
}

/// The second strips of the storeys from 827 up, in turn, where a scheme has none of its own.
const UPPER: [&str; 4] = ["panel_stripe", "grille", "panel_pipes", "panel"];

/// A wall scheme over the level's storeys: each has a strip from its floor, a second strip 118
/// up, and the plain slab trim above (Outworks' schemes). `upper` is the second strip of the
/// storeys from 827 up, in turn.
fn stack(first: &'static str, second: &'static str, third: &'static str, fourth: &'static str, upper: &[&'static str]) -> Scheme {
    let mut out: Scheme = vec![
        (-276, "plain"),
        (-157, "pipes"),
        (-39, "plain"),
        (0, first),
        (118, second),
        (236, "plain"),
        (276, "panel"),
        (394, third),
        (512, "plain"),
        (551, "panel"),
        (669, fourth),
        (787, "plain"),
    ];
    for (i, z) in FLOORS[4..].iter().enumerate() {
        out.extend([(*z, "panel"), (z + 118, upper[i % upper.len()]), (z + 236, "plain")]);
    }
    out
}

/// A scheme with some strips changed.
fn with(mut scheme: Scheme, strips: &[(i32, &'static str)]) -> Scheme {
    for (z, role) in strips {
        match scheme.iter_mut().find(|(b, _)| b == z) {
            Some(s) => s.1 = role,
            None => scheme.push((*z, role)),
        }
    }
    scheme
}

/// The level's own wall schemes (`SCHEMES`).
pub fn own_schemes() -> Schemes {
    vec![
        ("standard", stack("base_pipe", "panel", "panel_stripe", "grille", &UPPER)),
        ("striped", stack("base_pipe", "panel_stripe", "panel_pipes", "grille", &UPPER)),
        ("plain", stack("plain", "panel", "panel", "grille", &UPPER)),
        ("pipework", stack("base_pipe", "pipes", "panel_pipes", "grille", &UPPER)),
        ("chimney", stack("base_pipe", "panel_stripe", "panel_pipes", "panel_stripe", &["grille", "panel_stripe"])),
        ("flue", stack("base_pipe", "pipes", "panel_pipes", "grille", &["panel_pipes", "grille", "pipes"])),
        ("shaft", with(stack("base_pipe", "panel", "panel_stripe", "grille", &UPPER), &[(-276, "base_pipe"), (-157, "panel")])),
        ("reservoir", with(stack("base_pipe", "panel", "panel_pipes", "grille", &UPPER), &[(-157, "pipes")])),
        ("low", stack("plain", "panel", "panel", "grille", &UPPER)), // Complex's basement, 2A: plain under pipes
    ]
}

/// The scheme table a level is built with: the kit's, with the level's own put over it (what the
/// Python swaps into `cx.SCHEMES` while `polys` runs).
pub fn schemes() -> Schemes {
    let mut out = cx::schemes();
    for (name, scheme) in own_schemes() {
        match out.iter_mut().find(|(k, _)| *k == name) {
            Some(s) => s.1 = scheme,
            None => out.push((name, scheme)),
        }
    }
    out
}

/// Light through a floor grate or a drop: Complex's 2A.
pub const BEAM_DIR: [f64; 3] = [0.3, -0.25, -1.0];
/// Added to the lit faces' corners after the bake (times `sunbeam::intensity`).
pub const BEAM_STRENGTH: f64 = 220.0;

/// What `restyle` knows of a room.
struct Zone {
    low: bool,
    small: bool,
    vent: bool,
    scheme: &'static str,
    /// The top of its air.
    top: i32,
    /// For a tall room that is no pod: its floor.
    tall: Option<i32>,
    /// The height of its water.
    pool: Option<i32>,
    /// Its ceiling's role, where not the kit's own choice.
    ceiling: Option<&'static str>,
}

/// What the kit's plan style doesn't know: pools are water, tanks have panel tops, the low road
/// has a dark ceiling, rooms have ceilings of their own (`lv.ceilings`), and a room or corridor
/// on an upper storey starts its wall strips from its own floor (a base strip, the scheme's
/// second strip, the slab trim), as one on the ground does. Roles and schemes are `kit`'s.
pub fn restyle<'a>(lv: &'a Level, kit: &'a Kit) -> impl Fn(&Face, &cx::Base) -> Result<Surface, String> + 'a {
    let rooms: HashMap<&str, Zone> = lv
        .plan
        .rooms
        .iter()
        .map(|r| {
            let key = r.key.as_str();
            let zone = Zone {
                low: r.scheme == "low",
                small: ["corridor", "ramp", "perch"].contains(&r.kind),
                vent: r.kind == "vent",
                scheme: r.scheme,
                top: r.hi().2,
                tall: lv.spaces().iter().find(|s| s.key == key && s.tall() && s.host.is_empty()).map(|s| s.floor),
                pool: lv.pools.iter().find(|(k, _)| k == key).map(|(_, z)| *z),
                ceiling: lv.ceilings.iter().find(|(k, _)| k == key).map(|(_, role)| *role),
            };
            (key, zone)
        })
        .collect();
    // (a pool, a ceiling or a tall room the plan has no room for would be found by the Python
    // too, but every zone carved is a room of the plan)
    let (table, lines) = ((kit.schemes)(), lines());
    let ceiling = move |role: &str| kit.surface(role, None, if role == "plain" { Some((118.11, 236.22)) } else { None });
    move |f, base| {
        let z = f.lo[2];
        let room = rooms.get(f.zone);
        let kind = f.kind();
        let pool = room.and_then(|r| r.pool);
        if kind == "floor" && pool.is_some_and(|water| z <= water) && f.tag != "tank" {
            return kit.surface("water", None, None);
        }
        if kind == "floor" && f.tag == "tank" {
            return kit.surface("grey_panel", None, None);
        }
        if kind == "ceiling" && f.tag == "void" {
            if let Some(r) = room {
                if r.low {
                    return kit.surface("ceiling_dark", None, None);
                }
                if let Some(role) = r.ceiling.filter(|_| z >= r.top - 1) {
                    return ceiling(role);
                }
                if r.small {
                    return ceiling("plain");
                }
            }
        }
        if kind == "wall" && ["void", "wall", "pier"].contains(&f.tag) && z >= 276 {
            if let Some(r) = room.filter(|r| !r.vent) {
                let floor_under = |z: i32| FLOORS.iter().copied().filter(|&q| q <= z).max().ok_or("a wall under every storey");
                let rf = match r.tall {
                    Some(rf) if z >= rf => rf,
                    _ => floor_under(z)?, // the storey the strip is on
                };
                if rf > 0 {
                    let b = lines.iter().copied().filter(|&q| q <= z).max().ok_or("a wall under every line")?;
                    let standard = kit::scheme(&table, "standard").ok_or("no wall scheme 'standard'")?;
                    let strips = kit::scheme(&table, r.scheme).unwrap_or(standard);
                    let rel = py::first_min([0, 118, 236, 276, 394, 512, 551, 669, 787], |q| (q - (b - rf)).abs()).expect("nine strips");
                    let mut role = kit::strip(strips, rel).ok_or_else(|| format!("the wall scheme has no strip at {rel}"))?;
                    if rel == 0 && role == "panel" {
                        role = "base_pipe";
                    }
                    return kit.surface(role, Some(b as f64), None);
                }
            }
        }
        base(f)
    }
}

/// A beam with the room it lights, and how many polygons its split made lit (the Python's set
/// of ids; a polygon knows its beam by `Poly::lit`).
#[derive(Clone, Debug, PartialEq)]
pub struct Lit {
    pub room: String,
    pub beam: Beam,
    pub count: usize,
}

/// (room lit, beam): one through each floor grate and drop, with what stands in the room below
/// as its occluders.
pub fn beams(lv: &Level) -> Result<Vec<(String, Beam)>, String> {
    let mut out = Vec::new();
    for (low, (x0, y0, x1, y1), z_top, z_bottom) in &lv.beams {
        let (x0, y0, x1, y1) = (C(*x0 as f64) as f64, C(*y0 as f64) as f64, C(*x1 as f64) as f64, C(*y1 as f64) as f64);
        let rect = |z: i32| vec![[x0, y0, z as f64], [x1, y0, z as f64], [x1, y1, z as f64], [x0, y1, z as f64]];
        let room = lv.plan.get(low).ok_or_else(|| format!("a beam into {low}, which the plan has no room for"))?;
        let mut solid = Vec::new();
        for (lo, hi, tag) in &room.solids {
            if !["trim", "grate"].contains(tag) {
                let (lo, hi) = (f3(*lo), f3(*hi));
                solid.push([lo[0], hi[0]].into_iter().flat_map(|x| [lo[1], hi[1]].into_iter().flat_map(move |y| [lo[2], hi[2]].map(|z| [x, y, z]))).collect());
            }
        }
        out.push((low.clone(), Beam { direction: BEAM_DIR, apertures: vec![rect(*z_top), rect(*z_bottom)], occluders: solid }));
    }
    Ok(out)
}

/// The level as `complex::plan_polys` leaves it: the plan in Complex's grammar, dressed in `kit`,
/// before the beams.
pub fn planned(lv: &Level, kit: &Kit) -> Result<Vec<Poly>, String> {
    cx::plan_polys(&lv.plan, kit, None, &detail(), &(kit.schemes)(), Some(&restyle(lv, kit)))
}

/// The rest of `polys`, from `planned`'s polygons: the lit rooms cut along their beams, the floor
/// grates, the glass and the ramps' rails.
pub fn finish(lv: &Level, kit: &Kit, planned: Vec<Poly>) -> Result<(Vec<Poly>, Vec<Lit>), String> {
    let mut out = planned;
    let mut lit = Vec::new();
    for (low, beam) in beams(lv)? {
        let (cut, ids) = sunbeam::split(out, &beam, Some(&[low.as_str()]), Some(lv.space(&low).top as f64));
        out = cut;
        for &i in &ids {
            out[i].lit = Some(lit.len());
        }
        lit.push(Lit { room: low, beam, count: ids.len() });
    }
    if !lit.is_empty() {
        csg::fix_t_junctions(&mut out);
    }
    let grate = kit.surface("mesh", None, None)?; // a walkable mesh square to each cell, flush with the floor
    for (piece, _, (x0, y0, x1, y1), z) in &lv.grates {
        for i in *x0..*x1 {
            for j in *y0..*y1 {
                let (xa, ya, xb, yb, z) = (C(i as f64) as f64, C(j as f64) as f64, C((i + 1) as f64) as f64, C((j + 1) as f64) as f64, *z as f64);
                out.push(csg::oriented(
                    &[[xa, ya, z], [xb, ya, z], [xb, yb, z], [xa, yb, z]],
                    [0.0, 0.0, 1.0],
                    grate,
                    piece,
                    Some(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)]),
                ));
            }
        }
    }
    for (room, a, b, z, facing) in &lv.glass {
        let at = |p: &(f64, f64)| [C(p.0) as f64, C(p.1) as f64, *z as f64];
        out.extend(cx::glass_rail(kit, at(a), at(b), room, f3(*facing), WINDOW as f64)?);
    }
    let height = cx::RAIL_HEIGHT as f64;
    for (room, a, b, facing) in &lv.ramp_rails {
        // the railing, down the open sides of the ramps
        let surf = kit.surface("rail", Some(a.2.min(b.2) as f64), Some((cx::RAIL_REPEAT, height)))?;
        out.extend(csg::rail(f3(*a), f3(*b), height, room, surf, f3(*facing), Some(cx::RAIL_REPEAT)));
    }
    // a cellar's ladder (`Space::cellar`), where its hatch is
    for (up, low, _, side) in &lv.drops {
        if let (Some(_), Some(c)) = (side, lv.space(low).cellar.as_ref()) {
            out.extend(ladder(kit, c, lv.space(low), lv.space(up))?);
        }
    }
    // the ladders up the pillars (`furnish::Ladder`)
    for (key, it) in &lv.interiors {
        for l in &it.ladders {
            out.extend(pillar_ladder(kit, it, l, key)?);
        }
    }
    // the rescue's ladders up the edges of decks (`furnish::Chute`)
    for (key, it) in &lv.interiors {
        for c in &it.chutes {
            out.extend(chute_ladder(kit, c, key)?);
        }
    }
    // the angled catwalks (`furnish::angled`), which the plan can't hold
    let mut any = false;
    for (key, it) in &lv.interiors {
        for s in &it.angled {
            out.extend(angled(kit, s, key)?);
            any = true;
        }
    }
    if any {
        csg::fix_t_junctions(&mut out); // (its corners lie along the decks' edges)
    }
    Ok((out, lit))
}

/// A deck's slab, units: its top at the storey's floor, this far down (`furnish::plans`).
pub const SLAB: f64 = 40.0;

/// An angled catwalk: a slab a catwalk wide from deck to deck, its top the decks' floor (the
/// world's projection, so its plates run on from theirs), its underside and its two sides the
/// decks' own, a railing along each side. Its ends are open: each is cut along its deck's edge
/// (`furnish::AngledEnd`), its corners on that edge, so it meets the deck's top and underside edge
/// to edge. Split every `csg::STEP` along it, as the carver splits the decks.
pub fn angled(kit: &Kit, s: &autolevel::furnish::Angled, zone: &str) -> Result<Vec<Poly>, String> {
    let (top, under) = (kit.surface("floor", None, None)?, kit.surface("ceiling_dark", None, None)?);
    let (z, z0) = (s.z as f64, s.z as f64 - SLAB);
    let side = kit.surface("plain", Some(z0), None)?;
    let p = |c: (i32, i32)| [c.0 as f64, c.1 as f64];
    let ([l0, r0], [l1, r1]) = (s.ends[0].corners.map(p), s.ends[1].corners.map(p));
    let (ux, uy) = s.way();
    let left = [-uy, ux, 0.0]; // out of its left side
    let right = [uy, -ux, 0.0];
    let length = ((l1[0] - l0[0]).hypot(l1[1] - l0[1])).max((r1[0] - r0[0]).hypot(r1[1] - r0[1]));
    let n = ((length / csg::STEP).ceil() as usize).max(1);
    let lerp = |a: [f64; 2], b: [f64; 2], t: f64, h: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, h];
    let mut out = Vec::new();
    for i in 0..n {
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        let (la, lb, ra, rb) = (|h| lerp(l0, l1, t0, h), |h| lerp(l0, l1, t1, h), |h| lerp(r0, r1, t0, h), |h| lerp(r0, r1, t1, h));
        out.push(csg::oriented(&[la(z), ra(z), rb(z), lb(z)], [0.0, 0.0, 1.0], top, zone, None));
        out.push(csg::oriented(&[la(z0), ra(z0), rb(z0), lb(z0)], [0.0, 0.0, -1.0], under, zone, None));
        out.push(csg::oriented(&[la(z0), lb(z0), lb(z), la(z)], left, side, zone, None));
        out.push(csg::oriented(&[ra(z0), rb(z0), rb(z), ra(z)], right, side, zone, None));
    }
    // (as the decks' own rails: `complex::railing`)
    out.extend(cx::railing(kit, [l0[0], l0[1], z], [l1[0], l1[1], z], zone, left, None)?);
    out.extend(cx::railing(kit, [r0[0], r0[1], z], [r1[0], r1[1], z], zone, right, None)?);
    Ok(out)
}

/// How far a ladder stands off its wall, units (it is drawn over it, and is the face a player
/// climbs: `GEOFLAG_LADDER` on the importer's side).
pub const LADDER_OFF: f64 = 2.0;

/// A cellar's ladder: the grille, a cell wide, on the wall under the hatch's open side, from the
/// cellar's floor through the shaft to the hall's (the part in the shaft is the hall's: the
/// planner gives the hall the shaft's air). Complex's own ladder is the grille on a wall a cell
/// wide and a storey tall (`ref`'s tiles 0x44 in room 24).
pub fn ladder(kit: &Kit, c: &autolevel::space::Cellar, cellar: &autolevel::space::Space, hall: &autolevel::space::Space) -> Result<Vec<Poly>, String> {
    let ((ax, ay), (bx, by), (dx, dy)) = c.ladder();
    let at = |x: f64, y: f64, z: i32| [C(x) as f64 + dx as f64 * LADDER_OFF, C(y) as f64 + dy as f64 * LADDER_OFF, z as f64];
    let surf = kit.surface("ladder", None, None)?;
    let mut out = Vec::new();
    for (z0, z1, zone) in [(cellar.floor, cellar.top, &cellar.key), (cellar.top, hall.floor, &hall.key)] {
        let (v0, v1) = (z0 as f64 / 118.11, z1 as f64 / 118.11);
        let quad = [at(ax, ay, z0), at(bx, by, z0), at(bx, by, z1), at(ax, ay, z1)];
        out.push(csg::oriented(&quad, [dx as f64, dy as f64, 0.0], surf, zone, Some(&[(0.0, v0), (1.0, v0), (1.0, v1), (0.0, v1)])));
    }
    Ok(out)
}

/// A ladder up a pillar's side (`furnish::Ladder`): the cellar's grille, a cell wide, from the
/// floor to the pillar's top, `LADDER_OFF` out from its face.
pub fn pillar_ladder(kit: &Kit, it: &autolevel::furnish::Interior, l: &autolevel::furnish::Ladder, zone: &str) -> Result<Vec<Poly>, String> {
    let ((ax, ay), (bx, by), (dx, dy)) = it.ladder_line(l);
    let ((_, _, z0), (_, _, z1)) = it.ladder_ends(l);
    let at = |x: f64, y: f64, z: i32| [C(x) as f64 + dx as f64 * LADDER_OFF, C(y) as f64 + dy as f64 * LADDER_OFF, z as f64];
    let surf = kit.surface("ladder", None, None)?;
    let (v0, v1) = (z0 as f64 / 118.11, z1 as f64 / 118.11);
    let quad = [at(ax, ay, z0), at(bx, by, z0), at(bx, by, z1), at(ax, ay, z1)];
    // (no stiles: the user's call, 2026-11-20, they were not needed)
    Ok(vec![csg::oriented(&quad, [dx as f64, dy as f64, 0.0], surf, zone, Some(&[(0.0, v0), (1.0, v0), (1.0, v1), (0.0, v1)]))])
}

/// A ladder the rescue put up the edge of a deck (`furnish::Chute`): the cellar's grille, a cell
/// wide, from the storey below to the deck's top, on the edge itself. (Not `LADDER_OFF` out, as a
/// pillar's is: a player stepping off the deck to climb down must come out in front of it, where a
/// move away from it is a climb down, `bondwalk.c:1614`; 2 units out, they are behind it and hang
/// there. The walks `chutedown_*` go down each.)
pub fn chute_ladder(kit: &Kit, c: &autolevel::furnish::Chute, zone: &str) -> Result<Vec<Poly>, String> {
    let ((ax, ay), (bx, by), (dx, dy)) = c.line();
    let ((_, _, z0), (_, _, z1)) = c.ends();
    let at = |x: f64, y: f64, z: i32| [C(x) as f64, C(y) as f64, z as f64];
    let surf = kit.surface("ladder", None, None)?;
    let (v0, v1) = (z0 as f64 / 118.11, z1 as f64 / 118.11);
    let quad = [at(ax, ay, z0), at(bx, by, z0), at(bx, by, z1), at(ax, ay, z1)];
    Ok(vec![csg::oriented(&quad, [dx as f64, dy as f64, 0.0], surf, zone, Some(&[(0.0, v0), (1.0, v0), (1.0, v1), (0.0, v1)]))])
}

/// Every polygon of the level: the plan in Complex's grammar, the lit rooms cut along their
/// beams, the floor grates, the glass and the ramps' rails. Returns the polygons and the beams
/// (a polygon's `lit` is its beam's number in that list). Dressed in `kit`.
pub fn polys(lv: &Level, kit: &Kit) -> Result<(Vec<Poly>, Vec<Lit>), String> {
    finish(lv, kit, planned(lv, kit)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_lines_and_schemes() {
        let l = lines();
        assert_eq!((l.len(), &l[..6], l[l.len() - 1]), (51, &[-276, -157, -39, 0, 118, 236][..], 4370));
        let table = schemes();
        assert_eq!(
            table.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            ["standard", "striped", "plain", "pipework", "chimney", "flue", "shaft", "reservoir", "low"]
        );
        let strip = |name: &str, z: i32| kit::strip(kit::scheme(&table, name).unwrap(), z);
        assert_eq!((strip("standard", 669), strip("standard", 945), strip("standard", 1772)), (Some("grille"), Some("panel_stripe"), Some("panel")));
        assert_eq!((strip("chimney", 669), strip("chimney", 1220), strip("flue", 1496)), (Some("panel_stripe"), Some("panel_stripe"), Some("pipes")));
        assert_eq!(
            (strip("shaft", -276), strip("shaft", -157), strip("reservoir", -157), strip("low", -157)),
            (Some("base_pipe"), Some("panel"), Some("pipes"), Some("pipes"))
        );
        // the kit's pit line is not one of the level's
        assert_eq!(strip("standard", -118), None);
    }
}
