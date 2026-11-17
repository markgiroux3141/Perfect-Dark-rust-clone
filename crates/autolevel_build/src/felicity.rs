//! The Felicity kit: a Felicity-style level (`autolevel::felicity`) dressed in Felicity's own
//! materials (`assets/stages/mp11`), and its polygons.
//!
//! **Room types** (a room's `scheme`), measured 2026-11-17 from Felicity's own triangles
//! (`pd_import`'s `probe_stage_triangles`: which texture on floors, walls and ceilings, at what
//! height above the room's floor, and its units per repeat from each triangle's UV gradient,
//! which gives 128.3 x 134.8 for the blue marble floor where the procgen theme measured 128.3 x
//! 134.9). Felicity's rooms fall into these families by their textures:
//!
//! | type | floor | walls, bottom-up | ceiling | Felicity's rooms |
//! |---|---|---|---|---|
//! | `office` | blue marble 0287 | tan band 024F to 141, tile 0256 to 259, panel 024B | timber beams 01EC | most of the building, the hallway |
//! | `marble` | blue marble 0287 | pink marble 0872 to 141, tile 0256, panel 024B | panel 024B | the marble rooms |
//! | `industrial` | grey plate 070F | grey 0274 to 141, 0872 above | grey plate 070F | 12, 13, 15 |
//! | `washroom` | mirrored tile 0271 | tile 071B to 141, tile 071D to 259, 0281 above | 0281 | 26-28 (the toilets) |
//! | `hall` | mirrored tile 0271 | base 0276 to 118, 027D to 236, band 0288 to 310, 0281 above | 0281 | the three-bay hall |
//! | `hallside` | 0288 | 0273 full height, 0281 above | 0281 | 23-25, off the hall |
//!
//! The beam ceiling clamps in v (ClampT): its v repeat is 424, twice the lattice ceilings are
//! split on (212), so every piece spans half a repeat from a whole one and never reaches the
//! clamp (Felicity's own is 409).
//!
//! The geometry is Complex's carver (`complex::plan_polys`: rooms carved, solids filled back)
//! with a style of Felicity's own over it: a wall takes its strip by its height above its own
//! storey's floor; a face inside a doorway is jamb; steps, landings and the platform are treads
//! on top and risers on their sides.

use std::collections::HashMap;

use autolevel::felicity::{DOOR_H, PLATFORM, ROOM_H, STOREY};
use autolevel::level::Level;

use crate::complex::{self as cx, Detail};
use crate::csg::{self, f3, Face, Poly, Surface};
use crate::geometry::Lit;
use crate::kit::{strip, Kit, Paint, Schemes};

const fn paint(role: &'static str, preset: &'static str, flags: &'static [&'static str], name: &'static str, scale: (f64, f64)) -> Paint {
    Paint { role, preset, flags, name, scale }
}

/// The kit's materials (names: Felicity's Setup Editor export's, where it has the texture).
const PALETTE: [Paint; 27] = [
    // office and marble
    paint("floor", "0287", &[], "m2", (128.3, 134.8)),
    paint("lower", "024F", &[], "m0", (89.9, 94.5)),
    paint("upper", "0256", &[], "m1", (87.8, 86.8)),
    paint("plain", "024B", &[], "m9", (176.2, 124.0)),
    paint("marble", "0872", &[], "m10", (137.7, 129.3)),
    paint("ceiling", "024B", &[], "m9", (176.2, 124.0)),
    paint("beams", "01EC", &["ClampT"], "m3ClampT", (291.3, 424.0)),
    // every type's
    paint("tread", "0872", &[], "m10", (134.8, 127.6)),
    paint("riser", "0256", &[], "m1", (87.8, 86.8)),
    paint("jamb", "0264", &["ClampT"], "m4ClampT", (40.1, 212.1)),
    // the stairwells' railings: Felicity's see-through rail
    paint("rail", "0710", &["Transparent", "CullBoth", "MirrorS", "ClampT"], "m14TransparentCullBothMirrorSClampT", (49.0, 94.0)),
    // industrial
    paint("ind_floor", "070F", &[], "m15", (64.2, 63.9)),
    paint("ind_lower", "0274", &[], "m18", (94.6, 92.2)),
    paint("ind_upper", "0872", &[], "m10", (162.9, 142.1)),
    paint("ind_ceiling", "070F", &[], "m15", (64.2, 63.9)),
    // washroom
    paint("wc_floor", "0271", &["MirrorS", "MirrorT"], "m12MirrorSMirrorT", (184.2, 141.1)),
    paint("wc_lower", "071B", &[], "m28", (150.0, 140.1)),
    paint("wc_upper", "071D", &[], "m27", (127.2, 129.9)),
    paint("wc_plain", "0281", &[], "m22", (255.1, 506.9)),
    paint("wc_ceiling", "0281", &[], "m22", (255.1, 506.9)),
    // the hall, and the rooms off it
    paint("hall_floor", "0271", &["MirrorS", "MirrorT"], "m12MirrorSMirrorT", (243.5, 173.5)),
    paint("hall_base", "0276", &["ClampT"], "m29ClampT", (92.6, 143.3)),
    paint("hall_mid", "027D", &[], "m7", (79.1, 221.7)),
    paint("hall_band", "0288", &[], "m25", (305.5, 197.6)),
    paint("hall_upper", "0281", &[], "m22", (296.0, 401.4)),
    paint("side_wall", "0273", &["ClampT"], "m24ClampT", (137.0, 259.2)),
    paint("side_floor", "0288", &[], "m25", (205.1, 230.5)),
];

/// The room types: (type, its floor's role, its ceiling's). Its walls are its scheme's.
pub const TYPES: [(&str, &str, &str); 6] = [
    ("office", "floor", "beams"),
    ("marble", "floor", "ceiling"),
    ("industrial", "ind_floor", "ind_ceiling"),
    ("washroom", "wc_floor", "wc_ceiling"),
    ("hall", "hall_floor", "hall_upper"),
    ("hallside", "side_floor", "wc_ceiling"),
];

/// Each room type's wall strips above a storey's floor, bottom-up (above the last, it goes on).
pub fn schemes() -> Schemes {
    vec![
        ("office", vec![(0, "lower"), (141, "upper"), (259, "plain")]),
        ("marble", vec![(0, "marble"), (141, "upper"), (259, "plain")]),
        ("industrial", vec![(0, "ind_lower"), (141, "ind_upper")]),
        ("washroom", vec![(0, "wc_lower"), (141, "wc_upper"), (259, "wc_plain")]),
        ("hall", vec![(0, "hall_base"), (118, "hall_mid"), (236, "hall_band"), (310, "hall_upper")]),
        ("hallside", vec![(0, "side_wall"), (259, "hall_upper")]),
    ]
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

/// The heights walls are split on: on each storey, every type's strip lines, its doorways' tops
/// and its ceiling; and the platform's top.
fn lines(lv: &Level, schemes: &Schemes) -> Vec<i32> {
    let top = lv.plan.rooms.iter().map(|r| r.hi().2).max().unwrap_or(ROOM_H);
    let mut out = vec![PLATFORM];
    let mut z = 0;
    while z < top {
        out.extend([z, z + DOOR_H, z + ROOM_H]);
        out.extend(schemes.iter().flat_map(|(_, s)| s.iter().map(|(b, _)| z + b)));
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

/// A room type's floor and ceiling roles (an unknown type is an office).
fn type_of(scheme: &str) -> (&'static str, &'static str) {
    TYPES.iter().find(|t| t.0 == scheme).or(TYPES.first()).map(|t| (t.1, t.2)).expect("a type")
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
        let (floor, ceiling) = type_of(room.scheme);
        match f.kind() {
            "floor" => kit.surface(if matches!(f.tag, "step" | "landing" | "platform") { "tread" } else { floor }, None, None),
            "ceiling" if door.is_some() => kit.surface("jamb", None, None),
            "ceiling" => kit.surface(ceiling, None, None),
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
                let table = crate::kit::scheme(&schemes, room.scheme).or_else(|| crate::kit::scheme(&schemes, "office")).ok_or("no wall scheme 'office'")?;
                let at = table.iter().map(|(b, _)| *b).filter(|&b| b <= z - base).max().unwrap_or(0);
                kit.surface(strip(table, at).unwrap_or("plain"), Some((base + at) as f64), None)
            }
        }
    };
    let how = Detail { lattice: [Some(212.0), Some(212.0), None], max_span: 212, vent_step: 106.0, extras: true, lines: Some(lines(lv, &schemes)) };
    let mut polys = cx::plan_polys(&lv.plan, kit, None, &how, &schemes, Some(&style))?;
    // the railings down the flights' open sides (the landings' flat ones are the plan's own)
    let height = cx::RAIL_HEIGHT as f64;
    for (room, a, b, facing) in &lv.ramp_rails {
        let surf = kit.surface("rail", Some(a.2.min(b.2) as f64), Some((49.0, height)))?;
        polys.extend(csg::rail(f3(*a), f3(*b), height, room, surf, f3(*facing), Some(49.0)));
    }
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
        for role in ["floor", "lower", "upper", "beams", "tread", "riser", "jamb", "hall_floor", "hall_band"] {
            assert!(count(role) > 0, "no {role}");
        }
        // every type's walls, floor and ceiling are the kit's own roles
        let table = schemes();
        for (name, floor, ceiling) in TYPES {
            let strips = crate::kit::scheme(&table, name).unwrap_or_else(|| panic!("no scheme {name}"));
            for role in strips.iter().map(|s| s.1).chain([floor, ceiling]) {
                assert!(KIT.index(role).is_ok(), "{name}: {role}");
            }
        }
        // every role drawn is the kit's
        assert!(polys.iter().all(|p| KIT.index(p.surface.material).is_ok()));
        // the 24 treads, the hall's 3 steps and the platform: treads; the doorways' faces: jambs
        assert!(count("tread") >= 28);
        assert!(count("jamb") >= 3 * lv.doorways.len());
    }
}
