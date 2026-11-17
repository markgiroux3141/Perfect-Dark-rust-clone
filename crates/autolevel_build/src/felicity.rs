//! The Felicity kit: a Felicity-style level (`autolevel::felicity`) dressed in Felicity's own
//! materials (`assets/stages/mp11`), and its polygons.
//!
//! The roles and their units per repeat are the ones the Blender repo's procgen measured from
//! Felicity (`addon/ge_pd_tools/procgen/themes.py`, `_fel`), each material found by its texture in
//! Felicity's Setup Editor export (`levels/perfectdark/felicity/LevelIndices.mtl`): the blue
//! marble floor, the tan lower band to 141 and the tiled upper band to the ceiling at 259, plain
//! panels above, the pink marble wainscot, the jambs in the doorways, treads and risers.
//!
//! The geometry is Complex's carver (`complex::plan_polys`: rooms carved, solids filled back)
//! with a style of Felicity's own over it: a wall takes its strip by its height above its own
//! storey's floor; a face inside a doorway is jamb; steps, landings and the platform are treads
//! on top and risers on their sides.

use std::collections::HashMap;

use autolevel::felicity::{DOOR_H, PLATFORM, ROOM_H, STOREY};
use autolevel::level::Level;

use crate::complex::{self as cx, Detail};
use crate::csg::{Face, Poly, Surface};
use crate::geometry::Lit;
use crate::kit::{strip, Kit, Paint, Schemes};

const fn paint(role: &'static str, preset: &'static str, flags: &'static [&'static str], name: &'static str, scale: (f64, f64)) -> Paint {
    Paint { role, preset, flags, name, scale }
}

/// The kit's materials.
const PALETTE: [Paint; 9] = [
    paint("floor", "0287", &[], "m2", (128.3, 134.9)),
    paint("lower", "024F", &[], "m0", (97.2, 94.6)),
    paint("upper", "0256", &[], "m1", (86.8, 104.2)),
    paint("plain", "024B", &[], "m9", (243.6, 133.8)),
    paint("marble", "0872", &[], "m10", (184.4, 141.0)),
    paint("ceiling", "024B", &[], "m9", (316.0, 230.0)),
    paint("tread", "0872", &[], "m10", (134.8, 127.6)),
    paint("riser", "0256", &[], "m1", (86.8, 104.2)),
    paint("jamb", "0264", &["ClampT"], "m4ClampT", (40.0, 211.5)),
];

/// Wall strips above a storey's floor: the lower band, the upper band, plain panel above the
/// ceiling line (a tall room's walls).
pub fn schemes() -> Schemes {
    vec![("default", vec![(0, "lower"), (141, "upper"), (259, "plain")]), ("marble", vec![(0, "marble"), (141, "upper"), (259, "plain")])]
}

/// Felicity, as the generator dresses its levels.
pub static KIT: Kit = Kit {
    name: "felicity",
    tag: "fel",
    stage: "mp11",
    palette: &PALETTE,
    schemes,
    polys,
    tags: &[],
    tints: &[],
    // the procgen theme's, measured from Felicity's vertex colours
    target: &[("floor", 137), ("wall", 85), ("ceiling", 50)],
};

/// The heights walls are split on: each storey's floor, its band line, its doorways' tops and
/// its ceiling, and the platform's top.
fn lines(lv: &Level) -> Vec<i32> {
    let top = lv.plan.rooms.iter().map(|r| r.hi().2).max().unwrap_or(ROOM_H);
    let mut out = vec![PLATFORM];
    let mut z = 0;
    while z < top {
        out.extend([z, z + 141, z + DOOR_H, z + ROOM_H]);
        z += STOREY;
    }
    out.sort();
    out.dedup();
    out
}

/// What the style knows of a room.
struct Zone {
    floor: i32,
    /// Its storeys repeat (false: a hall, one tall storey).
    storeys: bool,
    scheme: &'static str,
}

/// Every polygon of a Felicity level, dressed in `kit`. No beams light a Felicity level.
pub fn polys(lv: &Level, kit: &Kit) -> Result<(Vec<Poly>, Vec<Lit>), String> {
    let schemes = (kit.schemes)();
    let rooms: HashMap<&str, Zone> =
        lv.plan.rooms.iter().map(|r| (r.key.as_str(), Zone { floor: r.floor, storeys: r.kind != "hub", scheme: r.scheme })).collect();
    // each doorway's two halves, by the room they are in
    let mut doorways: HashMap<&str, Vec<([i32; 3], [i32; 3])>> = HashMap::new();
    for d in &lv.doorways {
        let (lo, hi) = ([d.lo.0, d.lo.1, d.lo.2], [d.hi.0, d.hi.1, d.hi.2]);
        let (mut a_hi, mut b_lo) = (hi, lo);
        (a_hi[d.axis], b_lo[d.axis]) = (d.plane(), d.plane());
        doorways.entry(d.a.as_str()).or_default().push((lo, a_hi));
        doorways.entry(d.b.as_str()).or_default().push((b_lo, hi));
    }
    let none = Vec::new();
    let style = |f: &Face, _base: &cx::Base| -> Result<Surface, String> {
        let room = rooms.get(f.zone).ok_or_else(|| format!("a face in {}, which is no room of the plan", f.zone))?;
        let within = |(lo, hi): &([i32; 3], [i32; 3])| (0..3).all(|k| lo[k] <= f.lo[k] && f.hi[k] <= hi[k]);
        let door = doorways.get(f.zone).unwrap_or(&none).iter().find(|b| within(b));
        let z = f.lo[2];
        match f.kind() {
            "floor" => kit.surface(if matches!(f.tag, "step" | "landing" | "platform") { "tread" } else { "floor" }, None, None),
            "ceiling" if door.is_some() => kit.surface("jamb", None, None),
            "ceiling" => kit.surface("ceiling", None, None),
            _ => {
                if let Some((lo, _)) = door {
                    return kit.surface("jamb", Some(lo[2] as f64), None);
                }
                match f.tag {
                    "step" | "landing" => return kit.surface("riser", Some(z as f64), None),
                    "platform" => return kit.surface("lower", Some(0.0), None),
                    _ => {}
                }
                // the strip of its storey (a hall's walls are one storey, plain above its bands)
                let base = if room.storeys { room.floor + (z - room.floor).div_euclid(STOREY) * STOREY } else { room.floor };
                let table = crate::kit::scheme(&schemes, room.scheme).or_else(|| crate::kit::scheme(&schemes, "default")).ok_or("no wall scheme 'default'")?;
                let at = [0, 141, 259].into_iter().filter(|&b| b <= z - base).max().unwrap_or(0);
                kit.surface(strip(table, at).unwrap_or("plain"), Some((base + at) as f64), None)
            }
        }
    };
    let how = Detail { lattice: [Some(212.0), Some(212.0), None], max_span: 212, vent_step: 106.0, extras: true, lines: Some(lines(lv)) };
    let polys = cx::plan_polys(&lv.plan, kit, None, &how, &schemes, Some(&style))?;
    Ok((polys, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::level::{generate, Options};

    #[test]
    fn a_felicity_level_is_dressed_in_felicitys_materials() {
        let lv = generate(3, &Options { style: "felicity", ..Options::default() }).unwrap();
        let (polys, lit) = polys(&lv, &KIT).unwrap();
        assert!(lit.is_empty() && polys.len() > 1000, "{}", polys.len());
        let count = |role: &str| polys.iter().filter(|p| p.surface.material == role).count();
        for role in ["floor", "lower", "upper", "ceiling", "tread", "riser", "jamb"] {
            assert!(count(role) > 0, "no {role}");
        }
        // every role drawn is the kit's
        assert!(polys.iter().all(|p| KIT.index(p.surface.material).is_ok()));
        // the 24 treads, the hall's 3 steps and the platform: treads; the doorways' faces: jambs
        assert!(count("tread") >= 28);
        assert!(count("jamb") >= 3 * lv.doorways.len());
    }
}
