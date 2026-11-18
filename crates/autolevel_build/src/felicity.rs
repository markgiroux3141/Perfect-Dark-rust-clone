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
//! | `industrial` | grey 0271 | grey panels 071B to 141, 071D (blue lines) to 259, rock 0281 above | rock 0281 | the grey bays 26-32 |
//! | `washroom` | white tile 070F | black tile 0274 to 141, marble 0872 above | 070F | 12 (the toilets) |
//!
//! (Until 2026-11-17 these two were the wrong way round: the washroom's tiles were on the
//! industrial rooms. `--at` views of `mp11` in `pd_snapshot` take the dump's coordinates as they
//! are, and show which room is which.)
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
//!
//! **Arches** (`autolevel::level::Arch`, the carver's boxes can't make one): over each, the
//! carver's faces under its curve come out (the lintel's underside, the jambs above the
//! springing) and `arch` puts in the soffit along the curve and, on each face of the wall, the
//! corners above it filled in, each piece in the wall's own strip there. A doorway's soffit is
//! jamb; a screen's is the hall's band 0288, as Felicity's own arches are.
//!
//! **Doorway frames** (measured 2026-11-17 from Felicity's rooms 24 and 29: the 00EF faces round
//! their openings): every doorway with no arch is chamfered at 45 degrees on each face of its
//! wall, `TRIM` (13) deep and wide, jambs and lintel mitred at the corners, so at the wall's face
//! the opening is 26 wider and 13 taller. `clear_frame` cuts the carver's faces to suit (the wall's
//! faces lose the strip round the opening, the jambs and the lintel's underside start 13 into the
//! wall) and `frame` puts in the chamfers (00EF, role `trim`) and a floor triangle at each jamb's
//! foot. A door (13 thick, on the doorway's middle plane) stands in the 14 left between them.

use std::collections::HashMap;

use autolevel::dress::{
    self, Arm, Crossing, Pipe, Portal, Surround, BEAM_D, BEAM_W, BEVEL, FLARE, JAMB, LEAN, LIGHT, LINTEL, PILASTER, PIPE, PIPE_TIP, PORTAL_D, POST, SPLAY,
    SURROUND,
};
use autolevel::felicity::{DOOR_H, PLATFORM, ROOM_H, STOREY};
use autolevel::level::{Arch, Doorway, Level};

use crate::complex::{self as cx, Detail};
use crate::csg::{self, f3, Face, Poly, Surface, P3};
use crate::geometry::Lit;
use crate::kit::{strip, Kit, Paint, Schemes};
use crate::sweep::{self, Cover, Edge, Leg, Sweep};

const fn paint(role: &'static str, preset: &'static str, flags: &'static [&'static str], name: &'static str, scale: (f64, f64)) -> Paint {
    Paint { role, preset, flags, name, scale }
}

/// The kit's materials (names: Felicity's Setup Editor export's, where it has the texture).
const PALETTE: [Paint; 34] = [
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
    // a doorway's chamfered frame (Felicity's 00EF: 16 x 64 texels, about 18 across the bevel and
    // 212 up it)
    paint("trim", "00EF", &[], "trim", (18.4, 212.0)),
    // a ceiling brace's underside (Felicity's 0262: four tiles, the third a light panel; its u
    // repeat is set for each arm, so it holds whole repeats)
    paint("lamp", "0262", &[], "lamp", (180.0, 40.0)),
    // a pipe along a wall's base (Felicity's 012A, the shading its pipes have round them: dark
    // below, light on top, about 127 a repeat round the profile)
    paint("pipe", "012A", &[], "pipe", (128.0, 127.0)),
    // a hazard surround's stripes (Felicity's 071E round its grey bays' doorways: one repeat across
    // the flare, about 35 along it) and the grating under them (01B2: 64 across the depth, 66.5
    // along the wall)
    paint("hazard", "071E", &[], "hazard", (41.4, 35.0)),
    paint("grating", "01B2", &[], "grating", (64.0, 66.5)),
    // a portal across an industrial corridor (Felicity's grey bays): its passage 027D (79 a repeat
    // along its depth, 47.5 up), its lintel's light strip 071F (once across, once along: baked full
    // white)
    paint("portal", "027D", &[], "portal", (79.0, 47.5)),
    paint("glow", "071F", &[], "glow", (211.0, 79.0)),
    // the stairwells' railings: Felicity's see-through rail
    paint("rail", "0710", &["Transparent", "CullBoth", "MirrorS", "ClampT"], "m14TransparentCullBothMirrorSClampT", (49.0, 94.0)),
    // industrial: Felicity's grey concrete bays (rooms 26-32)
    paint("bay_floor", "0271", &["MirrorS", "MirrorT"], "m12MirrorSMirrorT", (184.2, 141.1)),
    paint("bay_lower", "071B", &[], "m28", (150.0, 140.1)),
    paint("bay_upper", "071D", &[], "m27", (127.2, 129.9)),
    paint("bay_plain", "0281", &[], "m22", (255.1, 506.9)),
    paint("bay_ceiling", "0281", &[], "m22", (255.1, 506.9)),
    // washroom: Felicity's washroom (room 12: black tile, marble above, white floor tile)
    paint("wc_floor", "070F", &[], "m15", (64.2, 63.9)),
    paint("wc_lower", "0274", &[], "m18", (94.6, 92.2)),
    paint("wc_upper", "0872", &[], "m10", (162.9, 142.1)),
    paint("wc_ceiling", "070F", &[], "m15", (64.2, 63.9)),
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
    ("industrial", "bay_floor", "bay_ceiling"),
    ("washroom", "wc_floor", "wc_ceiling"),
    ("hall", "hall_floor", "hall_upper"),
    ("hallside", "side_floor", "bay_ceiling"),
];

/// Each room type's wall strips above a storey's floor, bottom-up (above the last, it goes on).
pub fn schemes() -> Schemes {
    vec![
        ("office", vec![(0, "lower"), (141, "upper"), (259, "plain")]),
        ("marble", vec![(0, "marble"), (141, "upper"), (259, "plain")]),
        ("industrial", vec![(0, "bay_lower"), (141, "bay_upper"), (259, "bay_plain")]),
        ("washroom", vec![(0, "wc_lower"), (141, "wc_upper")]),
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

/// Take out the carver's faces under an arch's curve: every face across the opening (not on a
/// face of the wall) that lies wholly between its springing and its top goes, and a jamb that
/// rises from below the springing is cut down to it.
fn clear_under(polys: &mut Vec<Poly>, a: &Arch) {
    let o = 1 - a.axis;
    let within = |v: &[i32; 3]| a.t.0 <= v[a.axis] && v[a.axis] <= a.t.1 && a.s.0 <= v[o] && v[o] <= a.s.1 && v[2] <= a.top;
    polys.retain_mut(|p| {
        if p.normal[a.axis].abs() > 0.5 || !p.verts.iter().all(within) {
            return true;
        }
        let low = p.verts.iter().map(|v| v[2]).min().unwrap_or(a.top);
        if low >= a.spring {
            return false;
        }
        if p.normal[2].abs() < 0.5 {
            for v in p.verts.iter_mut() {
                v[2] = v[2].min(a.spring);
            }
            p.verts.dedup();
            if p.verts.len() > 1 && p.verts.first() == p.verts.last() {
                p.verts.pop();
            }
        }
        p.verts.len() >= 3
    });
}

/// A convex polygon in (along, height) cut by the line `height = z`: the part below, the part above.
#[allow(clippy::type_complexity)]
fn slice(poly: &[(f64, f64)], z: f64) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let (mut below, mut above) = (Vec::new(), Vec::new());
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        if p.1 <= z {
            below.push(p);
        }
        if p.1 >= z {
            above.push(p);
        }
        if (p.1 < z && q.1 > z) || (p.1 > z && q.1 < z) {
            let x = p.0 + (q.0 - p.0) * (z - p.1) / (q.1 - p.1);
            below.push((x, z));
            above.push((x, z));
        }
    }
    (below, above)
}

/// How deep and how wide a doorway's chamfered frame is (Felicity's: 13, at 45 degrees).
pub const TRIM: i32 = 13;

/// Cut a doorway's frame out of the carver's faces: on each face of the wall that is `chamfered`
/// (room `a`'s, room `b`'s), its faces lose the strip `TRIM` wide round the opening (beside the
/// jambs and over the lintel), and the faces inside the opening (the jambs, the lintel's
/// underside; not its floor) start `TRIM` into the wall from it. (A face with a hazard surround
/// instead clears its own wall: `surround`.)
fn clear_frame(polys: &mut Vec<Poly>, d: &Doorway, chamfered: [bool; 2]) {
    let (a, o) = (d.axis, 1 - d.axis);
    let (lo, hi) = ([d.lo.0, d.lo.1, d.lo.2], [d.hi.0, d.hi.1, d.hi.2]);
    // the wall's face round the opening, on each side
    let (rlo, rhi) = ([lo[o] - TRIM, lo[2]], [hi[o] + TRIM, hi[2] + TRIM]);
    let faces = [Cover { axis: a, at: lo[a], sign: -1, lo: rlo, hi: rhi }, Cover { axis: a, at: hi[a], sign: 1, lo: rlo, hi: rhi }];
    let covers: Vec<Cover> = faces.into_iter().zip(chamfered).filter(|(_, c)| *c).map(|(f, _)| f).collect();
    sweep::clear(polys, &covers);
    let (t0, t1) = (lo[a] + if chamfered[0] { TRIM } else { 0 }, hi[a] - if chamfered[1] { TRIM } else { 0 });
    polys.retain_mut(|p| {
        let inside = p.verts.iter().all(|v| lo[a] <= v[a] && v[a] <= hi[a] && lo[o] <= v[o] && v[o] <= hi[o] && lo[2] <= v[2] && v[2] <= hi[2]);
        if inside && p.normal[a].abs() < 0.5 && p.normal[2] < 0.5 {
            // a jamb or the lintel's underside: from `TRIM` into the wall to `TRIM` short of its far face
            for v in p.verts.iter_mut() {
                v[a] = v[a].clamp(t0, t1);
            }
            p.verts.dedup();
            if p.verts.len() > 1 && p.verts.first() == p.verts.last() {
                p.verts.pop();
            }
            return p.verts.len() >= 3;
        }
        true
    });
}

/// A doorway's chamfered frame: on each face of the wall that is `chamfered` (room `a`'s on the low
/// side, `b`'s on the high), the two jambs and the lintel bevelled at 45 degrees, `TRIM` deep,
/// mitred at the top corners, and a floor triangle at each jamb's foot in that room's floor.
fn frame(d: &Doorway, trim: Surface, floors: [Surface; 2], chamfered: [bool; 2]) -> Vec<Poly> {
    let (a, o) = (d.axis, 1 - d.axis);
    let (lo, hi) = ([d.lo.0, d.lo.1, d.lo.2], [d.hi.0, d.hi.1, d.hi.2]);
    let (s0, s1, z0, zt) = (lo[o] as f64, hi[o] as f64, lo[2] as f64, hi[2] as f64);
    let tr = TRIM as f64;
    let at = |t: f64, s: f64, z: f64| -> P3 {
        let mut p = [0.0; 3];
        p[a] = t;
        p[o] = s;
        p[2] = z;
        p
    };
    let axis = |k: usize, sign: f64| -> P3 {
        let mut p = [0.0; 3];
        p[k] = sign;
        p
    };
    // the bevel, from the opening's edge `TRIM` into the wall to the wall's face `TRIM` outside it
    // (u across it from the opening, as Felicity's 00EF is laid)
    let edge = Some(Edge { surface: trim, along_v: true });
    let mut out = Vec::new();
    let faces = [(d.a.as_str(), lo[a] as f64, 1.0, floors[0]), (d.b.as_str(), hi[a] as f64, -1.0, floors[1])];
    for (zone, t, into, floor) in faces.into_iter().zip(chamfered).filter(|(_, c)| *c).map(|(f, _)| f) {
        let depth = axis(a, into);
        // up the near jamb, across the lintel, down the far jamb: `side` towards the opening
        let leg = |to: P3, side: P3| Leg { to, side, depth, profile: vec![(0.0, tr), (-tr, 0.0)], edges: vec![edge], along: None };
        let path = Sweep {
            from: at(t, s0, z0),
            legs: vec![leg(at(t, s0, zt), axis(o, 1.0)), leg(at(t, s1, zt), axis(2, -1.0)), leg(at(t, s1, z0), axis(o, -1.0))],
            closed: false,
            caps: false,
            zone,
        };
        out.extend(sweep::sweep(&path));
        let w = t + into * tr;
        for (edge, beyond) in [(s0, s0 - tr), (s1, s1 + tr)] {
            out.push(csg::oriented(&[at(t, beyond, z0), at(t, edge, z0), at(w, edge, z0)], [0.0, 0.0, 1.0], floor, zone, None));
        }
    }
    out
}

/// The looks of a hazard surround: its stripes, the wall's tile on its splay, the grating under
/// the stripes, and the room's floor.
#[derive(Clone, Copy)]
struct SurroundLook {
    hazard: Surface,
    splay: Surface,
    grating: Surface,
    floor: Surface,
}

/// A hazard surround (`autolevel::dress::Surround`, Felicity's grey bays' doorways) on one face of
/// a doorway: from the opening's edge at the wall's face its stripes flare out `SURROUND` into the
/// room, `FLARE` wider beside the opening and taller over it, and the wall's tile splays back from
/// there to the wall, `SPLAY` beside and above the opening. Two sweeps, each from a jamb's foot up
/// and along the lintel to its middle (the stripes meet there in a chevron), the grating under the
/// stripes and the room's floor in front of each splay. Returns its polygons and what it covers of
/// the carver's: the wall behind it and the floor under it.
fn surround(d: &Doorway, s: &Surround, look: SurroundLook) -> (Vec<Poly>, Vec<Cover>) {
    let (a, o) = (d.axis, 1 - d.axis);
    let (lo, hi) = ([d.lo.0, d.lo.1, d.lo.2], [d.hi.0, d.hi.1, d.hi.2]);
    // the wall's face, and out of it into the room
    let (t, out) = if s.low() { (lo[a], -1) } else { (hi[a], 1) };
    let (s0, s1, z0, zt) = (lo[o], hi[o], lo[2], hi[2]);
    let at = |t: f64, s: f64, z: f64| -> P3 {
        let mut p = [0.0; 3];
        (p[a], p[o], p[2]) = (t, s, z);
        p
    };
    let (tf, depth) = (t as f64, unit_along(a, out as f64));
    let (dz, f) = (SURROUND as f64, (FLARE.0 as f64, FLARE.1 as f64));
    let profile = |flare: f64, splay: f64| vec![(0.0, 0.0), (flare, dz), (splay, 0.0)];
    let mid = (s0 + s1) as f64 / 2.0;
    let mut polys = Vec::new();
    for (edge, outward) in [(s0 as f64, -1.0), (s1 as f64, 1.0)] {
        let hazard = Some(Edge { surface: look.hazard, along_v: true });
        let path = Sweep {
            from: at(tf, edge, z0 as f64),
            legs: vec![
                // up the jamb: the splay's v as the wall's upper strip's (from 141 up)
                Leg {
                    to: at(tf, edge, zt as f64),
                    side: unit_along(o, outward),
                    depth,
                    profile: profile(f.0, SPLAY.0 as f64),
                    edges: vec![hazard, Some(Edge { surface: look.splay, along_v: true })],
                    along: Some(-141.0),
                },
                // along the lintel to its middle
                Leg {
                    to: at(tf, mid, zt as f64),
                    side: [0.0, 0.0, 1.0],
                    depth,
                    profile: profile(f.1, SPLAY.1 as f64),
                    edges: vec![hazard, Some(Edge { surface: look.splay, along_v: false })],
                    along: None,
                },
            ],
            closed: false,
            caps: false,
            zone: &s.room,
        };
        polys.extend(sweep::sweep(&path));
    }
    // the floor: the grating between the stripes' feet, the room's in front of each splay
    let fl = |t: i32, s: i32| at(t as f64, s as f64, z0 as f64);
    let front = t + out * SURROUND;
    let gr = [fl(t, s0), fl(t, s1), fl(front, s1 + FLARE.0), fl(front, s0 - FLARE.0)];
    let uvs: Vec<(f64, f64)> = gr.iter().map(|p| ((p[a] - tf).abs() / look.grating.scale.0, (p[o] - s0 as f64) / look.grating.scale.1)).collect();
    polys.push(csg::oriented(&gr, [0.0, 0.0, 1.0], look.grating, &s.room, Some(&uvs)));
    for (edge, w) in [(s0, -1), (s1, 1)] {
        let tri = [fl(t, edge + w * SPLAY.0), fl(front, edge + w * SPLAY.0), fl(front, edge + w * FLARE.0)];
        polys.push(csg::oriented(&tri, [0.0, 0.0, 1.0], look.floor, &s.room, None));
    }
    let (w0, w1) = (s0 - SPLAY.0, s1 + SPLAY.0);
    let wall = Cover { axis: a, at: t, sign: out, lo: [w0, z0], hi: [w1, zt + SPLAY.1] };
    let (f0, f1) = (t.min(front), t.max(front));
    let (flo, fhi) = if a == 0 { ([f0, w0], [f1, w1]) } else { ([w0, f0], [w1, f1]) };
    (polys, vec![wall, Cover { axis: 2, at: z0, sign: 1, lo: flo, hi: fhi }])
}

/// The looks of a portal: its bevels (the wall's upper tile, banded as the wall), its passage, the
/// light strip under its lintel, and the corridor's floor.
#[derive(Clone, Copy)]
struct PortalLook {
    bevel: Surface,
    passage: Surface,
    glow: Surface,
    floor: Surface,
}

/// A portal across a corridor (`autolevel::dress::Portal`, the openings between Felicity's grey
/// bays): on each face, a bevel `BEVEL` deep from the corridor's outline (wall, ceiling, wall) in to
/// the opening, `POST` in from each wall at the floor, straight up `JAMB`, leaning in `LEAN` to the
/// lintel at `LINTEL`; through it the passage (its sides `passage`, its lintel's underside the light
/// strip). Returns its polygons and what it covers of the carver's: the walls and the ceiling it
/// stands against, and the floor under its posts (the floor in front of each bevel's foot is put
/// back).
fn portal(p: &Portal, look: PortalLook) -> (Vec<Poly>, Vec<Cover>) {
    let (k, o) = (p.axis, 1 - p.axis);
    let pt = |t: i32, (s, h): (i32, i32)| -> P3 {
        let mut v = [0.0; 3];
        (v[k], v[o], v[2]) = (t as f64, s as f64, (p.floor + h) as f64);
        v
    };
    let (w0, w1, high) = (p.across.0, p.across.1, p.top - p.floor);
    let hole = [(w0 + POST, 0), (w0 + POST, JAMB), (w0 + POST + LEAN, LINTEL), (w1 - POST - LEAN, LINTEL), (w1 - POST, JAMB), (w1 - POST, 0)];
    let (a, b, c, d) = ((w0, 0), (w0, high), (w1, high), (w1, 0));
    let (t0, t1) = (p.at - PORTAL_D / 2, p.at + PORTAL_D / 2);
    let (i0, i1) = (t0 + BEVEL, t1 - BEVEL);
    let mut polys = Vec::new();
    // the bevels, the corridor's outline to the opening, as triangles
    for (face, inner, sign) in [(t0, i0, -1.0), (t1, i1, 1.0)] {
        let [a, b, c, d] = [a, b, c, d].map(|q| pt(face, q));
        let h = hole.map(|q| pt(inner, q));
        let tris = [[a, h[0], h[1]], [a, h[1], b], [b, h[1], h[2]], [b, h[2], h[3]], [b, h[3], c], [c, h[3], h[4]], [c, h[4], d], [d, h[4], h[5]]];
        for tri in tris {
            polys.push(csg::oriented(&tri, unit_along(k, sign), look.bevel, &p.room, None));
        }
    }
    // the passage: each edge of the opening from one bevel to the other, facing in; v up the
    // outline from the floor on each side, the lintel's strip once across and once along
    let rise = (LEAN as f64).hypot((LINTEL - JAMB) as f64);
    let up = [0.0, JAMB as f64, JAMB as f64 + rise, JAMB as f64 + rise, JAMB as f64, 0.0];
    let depth = (i1 - i0) as f64;
    for e in 0..5 {
        let (h, n) = (hole[e], hole[e + 1]);
        let mut normal = [0.0; 3];
        (normal[o], normal[2]) = ((n.1 - h.1) as f64, -(n.0 - h.0) as f64);
        let quad = [pt(i0, h), pt(i1, h), pt(i1, n), pt(i0, n)];
        if e == 2 {
            let uvs = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
            polys.push(csg::oriented(&quad, normal, look.glow, &p.room, Some(&uvs)));
        } else {
            let (sc, (vh, vn)) = (look.passage.scale, (up[e], up[e + 1]));
            let uvs = [(0.0, vh / sc.1), (depth / sc.0, vh / sc.1), (depth / sc.0, vn / sc.1), (0.0, vn / sc.1)];
            polys.push(csg::oriented(&quad, normal, look.passage, &p.room, Some(&uvs)));
        }
    }
    // the floor in front of each bevel's foot, beside each post
    for (face, inner) in [(t0, i0), (t1, i1)] {
        for (wall, post) in [(w0, w0 + POST), (w1, w1 - POST)] {
            let tri = [pt(face, (wall, 0)), pt(face, (post, 0)), pt(inner, (post, 0))];
            polys.push(csg::oriented(&tri, [0.0, 0.0, 1.0], look.floor, &p.room, None));
        }
    }
    let flat = |s0: i32, s1: i32| if k == 0 { ([t0, s0], [t1, s1]) } else { ([s0, t0], [s1, t1]) };
    let mut covers =
        vec![Cover { axis: o, at: w0, sign: 1, lo: [t0, p.floor], hi: [t1, p.top] }, Cover { axis: o, at: w1, sign: -1, lo: [t0, p.floor], hi: [t1, p.top] }];
    let (lo, hi) = flat(w0, w1);
    covers.push(Cover { axis: 2, at: p.top, sign: -1, lo, hi });
    for (s0, s1) in [(w0, w0 + POST), (w1 - POST, w1)] {
        let (lo, hi) = flat(s0, s1);
        covers.push(Cover { axis: 2, at: p.floor, sign: 1, lo, hi });
    }
    (polys, covers)
}

/// An arch's polygons: the soffit along its curve across the wall (each room's half in it), and
/// on each face of the wall the two corners above the curve filled in, cut at the `cuts` (the
/// wall's strip lines) so that each piece takes `face`'s surface for the wall there.
fn arch(a: &Arch, soffit: Surface, cuts: &[i32], face: &dyn Fn(&str, &Face) -> Result<Surface, String>) -> Result<Vec<Poly>, String> {
    let o = 1 - a.axis;
    let at = |t: f64, (s, z): (f64, f64)| -> P3 {
        let mut p = [0.0; 3];
        p[a.axis] = t;
        p[o] = s;
        p[2] = z;
        p
    };
    // pieces about every 40 along the curve, 6 to 16, an even number (the crown is a point)
    let rough = a.curve(16);
    let length: f64 = rough.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
    let n = (((length / 40.0).round() as usize).clamp(6, 16) + 1) & !1;
    let curve = a.curve(n);
    let sides = [(a.a.as_str(), a.t.0 as f64, a.mid as f64, -1.0), (a.b.as_str(), a.mid as f64, a.t.1 as f64, 1.0)];
    let mut out = Vec::new();
    for w in curve.windows(2) {
        let ((s0, z0), (s1, z1)) = (w[0], w[1]);
        if (s1 - s0).abs() + (z1 - z0).abs() < 1.0 {
            continue;
        }
        // facing in under the curve
        let mut normal = [0.0; 3];
        normal[o] = z1 - z0;
        normal[2] = -(s1 - s0);
        for &(zone, t0, t1, _) in &sides {
            if t1 > t0 {
                out.push(csg::oriented(&[at(t0, w[0]), at(t1, w[0]), at(t1, w[1]), at(t0, w[1])], normal, soffit, zone, None));
            }
        }
    }
    let (top, half) = (a.top as f64, n / 2);
    let lines: Vec<f64> = cuts.iter().filter(|&&z| a.spring < z && z < a.top).map(|&z| z as f64).collect();
    for (corner, part) in [((a.s.0 as f64, top), &curve[..=half]), ((a.s.1 as f64, top), &curve[half..])] {
        for w in part.windows(2) {
            let mut rest = vec![corner, w[0], w[1]];
            let mut pieces = Vec::new();
            for &z in &lines {
                let (below, above) = slice(&rest, z);
                pieces.push(below);
                rest = above;
            }
            pieces.push(rest);
            for piece in pieces {
                let mut round: Vec<(i32, i32)> = piece.iter().map(|&(s, z)| (s.round() as i32, z.round() as i32)).collect();
                round.dedup();
                if round.len() > 1 && round.first() == round.last() {
                    round.pop();
                }
                if round.len() < 3 {
                    continue;
                }
                let (lo_s, hi_s) = (round.iter().map(|p| p.0).min().unwrap_or(0), round.iter().map(|p| p.0).max().unwrap_or(0));
                let (lo_z, hi_z) = (round.iter().map(|p| p.1).min().unwrap_or(0), round.iter().map(|p| p.1).max().unwrap_or(0));
                for &(zone, _, _, sign) in &sides {
                    let t = if sign < 0.0 { a.t.0 } else { a.t.1 };
                    let (mut lo, mut hi) = ([0; 3], [0; 3]);
                    (lo[a.axis], hi[a.axis], lo[o], hi[o], lo[2], hi[2]) = (t, t, lo_s, hi_s, lo_z, hi_z);
                    let f = Face { axis: a.axis, sign: sign as i32, lo, hi, zone, tag: "void" };
                    let mut normal = [0.0; 3];
                    normal[a.axis] = sign;
                    let verts: Vec<P3> = piece.iter().map(|&p| at(t as f64, p)).collect();
                    out.push(csg::oriented(&verts, normal, face(zone, &f)?, zone, None));
                }
            }
        }
    }
    Ok(out)
}

/// A unit vector along `axis`, `sign` long.
fn unit_along(axis: usize, sign: f64) -> P3 {
    let mut p = [0.0; 3];
    p[axis] = sign;
    p
}

/// What a brace's arm covers of the carver's faces: the ceiling over it, and at each wall end the
/// wall behind its pilaster and the floor under it.
fn arm_covers(a: &Arm) -> Vec<Cover> {
    let (k, h) = (a.axis, BEAM_W / 2);
    // a rectangle in the floor or ceiling, `t0..t1` along the arm and `at +- h` across it
    let flat = |t0: i32, t1: i32| if k == 0 { ([t0, a.at - h], [t1, a.at + h]) } else { ([a.at - h, t0], [a.at + h, t1]) };
    let (lo, hi) = flat(a.t.0, a.t.1);
    let mut out = vec![Cover { axis: 2, at: a.top, sign: -1, lo, hi }];
    for (wall, t, into) in [(a.walls.0, a.t.0, 1), (a.walls.1, a.t.1, -1)] {
        if wall {
            out.push(Cover { axis: k, at: t, sign: into, lo: [a.at - h, a.floor], hi: [a.at + h, a.top] });
            let (lo, hi) = flat(t.min(t + into * PILASTER), t.max(t + into * PILASTER));
            out.push(Cover { axis: 2, at: a.floor, sign: 1, lo, hi });
        }
    }
    out
}

/// A brace's arm (`autolevel::dress`): from a wall end, a pilaster up the wall mitred into the
/// beam along the ceiling (and down the far wall's pilaster if both ends are walls), as one sweep.
/// The beam's sides are `side`, its underside `lamp` (its strip from the wall in, whole repeats),
/// the pilasters `pilaster`.
fn arm(a: &Arm, side: Surface, lamp: Surface, pilaster: Surface) -> Vec<Poly> {
    let (k, o) = (a.axis, 1 - a.axis);
    let pt = |t: i32, z: i32| -> P3 {
        let mut p = [0.0; 3];
        (p[k], p[o], p[2]) = (t as f64, a.at as f64, z as f64);
        p
    };
    // the path runs from the end the strip starts at
    let (start, end, walls, dir) = if a.from_high() { (a.t.1, a.t.0, (a.walls.1, a.walls.0), -1.0) } else { (a.t.0, a.t.1, a.walls, 1.0) };
    let each = (a.t.1 - a.t.0) as f64 / a.repeats() as f64;
    let lamp = Surface { scale: (each, BEAM_W as f64), ..lamp };
    let h = (BEAM_W / 2) as f64;
    let box_of = |deep: i32| vec![(-h, 0.0), (-h, deep as f64), (h, deep as f64), (h, 0.0)];
    let up = |e: Surface| Some(Edge { surface: e, along_v: true });
    let along = |e: Surface| Some(Edge { surface: e, along_v: false });
    let across = unit_along(o, 1.0);
    let post = |to: P3, into: f64| Leg {
        to,
        side: across,
        depth: unit_along(k, into),
        profile: box_of(PILASTER),
        edges: vec![up(pilaster), up(pilaster), up(pilaster)],
        along: None,
    };
    let mut legs = Vec::new();
    if walls.0 {
        legs.push(post(pt(start, a.top), dir));
    }
    legs.push(Leg {
        to: pt(end, a.top),
        side: across,
        depth: [0.0, 0.0, -1.0],
        profile: box_of(BEAM_D),
        edges: vec![along(side), along(lamp), along(side)],
        along: Some(0.0),
    });
    if walls.1 {
        legs.push(post(pt(end, a.floor), -dir));
    }
    let from = if walls.0 { pt(start, a.floor) } else { pt(start, a.top) };
    sweep::sweep(&Sweep { from, legs, closed: false, caps: false, zone: &a.room })
}

/// Where two braces cross: the square of underside under both, a plain tile of the strip.
fn crossing(c: &Crossing, lamp: Surface) -> Poly {
    let (h, z) = ((BEAM_W / 2) as f64, (c.top - BEAM_D) as f64);
    let (x, y) = (c.x as f64, c.y as f64);
    let corners = [[x - h, y - h, z], [x + h, y - h, z], [x + h, y + h, z], [x - h, y + h, z]];
    let uvs = [(0.0, 0.0), (LIGHT.0 / 2.0, 0.0), (LIGHT.0 / 2.0, 1.0), (0.0, 1.0)];
    csg::oriented(&corners, [0.0, 0.0, -1.0], lamp, &c.room, Some(&uvs))
}

/// A pipe along the base of a room's walls (`autolevel::dress::Pipe`): Felicity's hexagon lying on
/// the floor, swept along the walls and mitred round the corners, its ends capped. Returns its
/// polygons and what it covers of the floor (under its flat bottom, which is not drawn).
fn pipe(p: &Pipe, surface: Surface) -> (Vec<Poly>, Vec<Cover>) {
    let (w, tip) = (PIPE as f64, PIPE_TIP as f64);
    // (height, out from the wall), the wall's tip first, round under and over
    let profile = vec![(w / 2.0, 0.0), (0.0, tip), (0.0, w - tip), (w / 2.0, w), (w, w - tip), (w, tip), (w / 2.0, 0.0)];
    let e = Some(Edge { surface, along_v: false });
    let edges = vec![e, None, e, e, e, e];
    let z = p.floor as f64;
    let at = |(x, y): (i32, i32)| [x as f64, y as f64, z];
    let mut legs = Vec::new();
    let mut covers = Vec::new();
    for pair in p.path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let d = [(b.0 - a.0).signum(), (b.1 - a.1).signum()];
        // the room is on the left
        let left = [-d[1], d[0]];
        legs.push(Leg {
            to: at(b),
            side: [0.0, 0.0, 1.0],
            depth: [left[0] as f64, left[1] as f64, 0.0],
            profile: profile.clone(),
            edges: edges.clone(),
            along: None,
        });
        let (i0, i1) = (PIPE_TIP, PIPE - PIPE_TIP);
        let xs = [a.0, b.0, a.0 + left[0] * i0, a.0 + left[0] * i1];
        let ys = [a.1, b.1, a.1 + left[1] * i0, a.1 + left[1] * i1];
        let (lo, hi) = if d[0] != 0 {
            ([a.0.min(b.0), ys[2].min(ys[3])], [a.0.max(b.0), ys[2].max(ys[3])])
        } else {
            ([xs[2].min(xs[3]), a.1.min(b.1)], [xs[2].max(xs[3]), a.1.max(b.1)])
        };
        covers.push(Cover { axis: 2, at: p.floor, sign: 1, lo, hi });
    }
    let polys = sweep::sweep(&Sweep { from: at(p.path[0]), legs, closed: false, caps: true, zone: &p.room });
    (polys, covers)
}

/// What the style knows of a room.
struct Zone {
    floor: i32,
    /// Its storeys repeat (false: a hall, one tall storey).
    storeys: bool,
    scheme: &'static str,
    /// Its walkways (`autolevel::balcony`): a wall above one, behind it, starts its strips again
    /// at the walkway's top, as a storey's would.
    decks: Vec<([i32; 3], [i32; 3])>,
}

impl Zone {
    /// The floor a wall face's strips stand on: its storey's (a hall's own floor: one tall
    /// storey), or the top of the walkway it is behind.
    fn base(&self, f: &Face) -> i32 {
        let z = f.lo[2];
        let own = if self.storeys { self.floor + (z - self.floor).div_euclid(STOREY) * STOREY } else { self.floor };
        let o = 1 - f.axis;
        let behind =
            |(lo, hi): &&([i32; 3], [i32; 3])| hi[2] <= z && (lo[f.axis] == f.lo[f.axis] || hi[f.axis] == f.lo[f.axis]) && lo[o] <= f.lo[o] && f.hi[o] <= hi[o];
        self.decks.iter().filter(behind).map(|d| d.1[2]).max().map_or(own, |top| top.max(own))
    }
}

/// A room type's floor and ceiling roles (an unknown type is an office).
fn type_of(scheme: &str) -> (&'static str, &'static str) {
    TYPES.iter().find(|t| t.0 == scheme).or(TYPES.first()).map(|t| (t.1, t.2)).expect("a type")
}

/// Every polygon of a Felicity level, dressed in `kit`. No beams light a Felicity level.
pub fn polys(lv: &Level, kit: &Kit) -> Result<(Vec<Poly>, Vec<Lit>), String> {
    let schemes = (kit.schemes)();
    let rooms: HashMap<&str, Zone> = lv
        .plan
        .rooms
        .iter()
        .map(|r| {
            let decks = r.solids.iter().filter(|s| s.2 == "walkway").map(|(lo, hi, _)| ([lo.0, lo.1, lo.2], [hi.0, hi.1, hi.2])).collect();
            (r.key.as_str(), Zone { floor: r.floor, storeys: r.kind != "hub", scheme: r.scheme, decks })
        })
        .collect();
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
    // a wall's strip: its storey's, or its walkway's (a hall's walls are one storey, plain above
    // its bands, but above a walkway, behind it)
    let table = crate::kit::scheme(&schemes, "office").ok_or("no wall scheme 'office'")?;
    let wall = |room: &Zone, f: &Face| -> Result<Surface, String> {
        let base = room.base(f);
        let table = crate::kit::scheme(&schemes, room.scheme).unwrap_or(table);
        let at = table.iter().map(|(b, _)| *b).filter(|&b| b <= f.lo[2] - base).max().unwrap_or(0);
        kit.surface(strip(table, at).unwrap_or("plain"), Some((base + at) as f64), None)
    };
    let style = |f: &Face, _base: &cx::Base| -> Result<Surface, String> {
        let room = rooms.get(f.zone).ok_or_else(|| format!("a face in {}, which is no room of the plan", f.zone))?;
        let within = |(lo, hi): &([i32; 3], [i32; 3])| (0..3).all(|k| lo[k] <= f.lo[k] && f.hi[k] <= hi[k]);
        let door = doorways.get(f.zone).unwrap_or(&none).iter().find(|b| within(b));
        let z = f.lo[2];
        let (floor, ceiling) = type_of(room.scheme);
        match f.kind() {
            "floor" => kit.surface(if matches!(f.tag, "step" | "landing" | "platform" | "walkway") { "tread" } else { floor }, None, None),
            "ceiling" if door.is_some() => kit.surface("jamb", None, None),
            // the underside of a walkway, or of a flight standing on nothing
            "ceiling" if matches!(f.tag, "walkway" | "step") => kit.surface("ceiling", None, None),
            "ceiling" => kit.surface(ceiling, None, None),
            _ => {
                if let Some((lo, _)) = door {
                    return kit.surface("jamb", Some(lo[2] as f64), None);
                }
                match f.tag {
                    "step" | "landing" | "walkway" => return kit.surface("riser", Some(z as f64), None),
                    "platform" => return kit.surface("lower", Some(0.0), None),
                    _ => {}
                }
                wall(room, f)
            }
        }
    };
    let cuts = lines(lv, &schemes);
    let how = Detail { lattice: [Some(212.0), Some(212.0), None], max_span: 212, vent_step: 106.0, extras: true, lines: Some(cuts.clone()) };
    let mut polys = cx::plan_polys(&lv.plan, kit, None, &how, &schemes, Some(&style))?;
    // the arches: the carver's faces under each curve out, the arch's own in
    for a in &lv.arches {
        clear_under(&mut polys, a);
        let soffit = if a.kind == "screen" { kit.surface("hall_band", None, None)? } else { kit.surface("jamb", Some(a.floor as f64), None)? };
        let face =
            |zone: &str, f: &Face| -> Result<Surface, String> { wall(rooms.get(zone).ok_or_else(|| format!("an arch in {zone}, which is no room"))?, f) };
        polys.extend(arch(a, soffit, &cuts, &face)?);
    }
    // the doorways' chamfered frames, round every doorway with no arch
    let trim = kit.surface("trim", None, None)?;
    for d in lv.doorways.iter().filter(|d| !lv.arches.iter().any(|a| a.kind == "doorway" && a.a == d.a && a.b == d.b)) {
        let floor = |zone: &str| -> Result<Surface, String> {
            let room = rooms.get(zone).ok_or_else(|| format!("a doorway into {zone}, which is no room"))?;
            kit.surface(type_of(room.scheme).0, None, None)
        };
        // a face with a hazard surround has it in place of the chamfer
        let mine: Vec<&Surround> = lv.surrounds.iter().filter(|s| s.a == d.a && s.b == d.b).collect();
        let chamfered = [!mine.iter().any(|s| s.low()), !mine.iter().any(|s| !s.low())];
        clear_frame(&mut polys, d, chamfered);
        polys.extend(frame(d, trim, [floor(&d.a)?, floor(&d.b)?], chamfered));
        for s in mine {
            let look = SurroundLook {
                hazard: kit.surface("hazard", None, None)?,
                splay: kit.surface("bay_upper", None, None)?,
                grating: kit.surface("grating", None, None)?,
                floor: floor(&s.room)?,
            };
            let (faces, covers) = surround(d, s, look);
            sweep::clear(&mut polys, &covers);
            polys.extend(faces);
        }
    }
    // the ceiling braces: each arm's faces in, and what they cover of the carver's out
    let (arms, crossings) = dress::arms(&lv.braces);
    let (side, lamp, pilaster) = (kit.surface("plain", None, None)?, kit.surface("lamp", None, None)?, kit.surface("upper", None, None)?);
    for a in &arms {
        sweep::clear(&mut polys, &arm_covers(a));
        polys.extend(arm(a, side, lamp, pilaster));
    }
    for c in &crossings {
        let h = BEAM_W / 2;
        sweep::clear(&mut polys, &[Cover { axis: 2, at: c.top, sign: -1, lo: [c.x - h, c.y - h], hi: [c.x + h, c.y + h] }]);
        polys.push(crossing(c, lamp));
    }
    // the portals across the industrial corridors
    for p in &lv.portals {
        let look = PortalLook {
            bevel: kit.surface("bay_upper", Some((p.floor + 141) as f64), None)?,
            passage: kit.surface("portal", None, None)?,
            glow: kit.surface("glow", None, None)?,
            floor: kit.surface("bay_floor", None, None)?,
        };
        let (faces, covers) = portal(p, look);
        sweep::clear(&mut polys, &covers);
        polys.extend(faces);
    }
    // the pipes along the industrial rooms' walls
    let pipe_look = kit.surface("pipe", None, None)?;
    for p in &lv.pipes {
        let (faces, covers) = pipe(p, pipe_look);
        sweep::clear(&mut polys, &covers);
        polys.extend(faces);
    }
    csg::fix_t_junctions(&mut polys);
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
    use crate::csg::V3;
    use autolevel::level::{generate, Options};

    /// The frame as it was built before the shape tool: six quads by hand, a reference for `frame`.
    fn frame_by_hand(d: &Doorway) -> Vec<Vec<V3>> {
        let (a, o) = (d.axis, 1 - d.axis);
        let (lo, hi) = ([d.lo.0, d.lo.1, d.lo.2], [d.hi.0, d.hi.1, d.hi.2]);
        let (s0, s1, z0, zt) = (lo[o], hi[o], lo[2], hi[2]);
        let at = |t: i32, s: i32, z: i32| -> V3 {
            let mut p = [0; 3];
            (p[a], p[o], p[2]) = (t, s, z);
            p
        };
        let mut out = Vec::new();
        for (t, into) in [(lo[a], 1), (hi[a], -1)] {
            let (u, w) = (t, t + into * TRIM);
            out.push(vec![at(u, s0 - TRIM, z0), at(w, s0, z0), at(w, s0, zt), at(u, s0 - TRIM, zt + TRIM)]);
            out.push(vec![at(u, s1 + TRIM, z0), at(u, s1 + TRIM, zt + TRIM), at(w, s1, zt), at(w, s1, z0)]);
            out.push(vec![at(u, s0 - TRIM, zt + TRIM), at(u, s1 + TRIM, zt + TRIM), at(w, s1, zt), at(w, s0, zt)]);
        }
        out
    }

    /// A polygon's vertices as a cycle from its least vertex, either way round.
    fn cycle(v: &[V3]) -> Vec<V3> {
        let k = (0..v.len()).min_by_key(|&i| v[i]).unwrap_or(0);
        let fwd: Vec<V3> = (0..v.len()).map(|i| v[(k + i) % v.len()]).collect();
        let back: Vec<V3> = (0..v.len()).map(|i| v[(k + v.len() - i) % v.len()]).collect();
        fwd.min(back)
    }

    #[test]
    fn braces_are_swept_and_nothing_of_the_carvers_is_left_behind_them() {
        let lv = (1..=10).map(|seed| generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap()).find(|lv| !lv.braces.is_empty()).unwrap();
        let (polys, _) = polys(&lv, &KIT).unwrap();
        let (arms, crossings) = dress::arms(&lv.braces);
        let lamps = polys.iter().filter(|p| p.surface.material == "lamp").count();
        assert_eq!(lamps, arms.len() + crossings.len());
        for a in &arms {
            let (k, o, h) = (a.axis, 1 - a.axis, BEAM_W / 2);
            // the underside: the arm's length, 24 down, its strip whole repeats long
            let under = polys
                .iter()
                .find(|p| {
                    p.surface.material == "lamp"
                        && p.zone == a.room
                        && p.verts.iter().all(|v| v[o] >= a.at - h && v[o] <= a.at + h && v[k] >= a.t.0 && v[k] <= a.t.1)
                })
                .unwrap_or_else(|| panic!("{a:?}: no underside"));
            assert!(under.verts.iter().all(|v| v[2] == a.top - BEAM_D));
            for p in polys.iter().filter(|p| p.zone == a.room && p.surface.material != "lamp") {
                let mean = |c: usize| p.verts.iter().map(|v| v[c] as f64).sum::<f64>() / p.verts.len() as f64;
                let over = (a.at - h) as f64 <= mean(o) && mean(o) <= (a.at + h) as f64 && (a.t.0 as f64) < mean(k) && mean(k) < a.t.1 as f64;
                // no ceiling over the beam
                assert!(!(over && p.normal[2] < -0.99 && p.verts.iter().all(|v| v[2] == a.top)), "{a:?}: a ceiling face over it");
                // no wall behind a pilaster
                for (wall, t) in [(a.walls.0, a.t.0), (a.walls.1, a.t.1)] {
                    let behind =
                        wall && p.normal[k].abs() > 0.99 && p.verts.iter().all(|v| v[k] == t) && (a.at - h) as f64 <= mean(o) && mean(o) <= (a.at + h) as f64;
                    assert!(!behind, "{a:?}: a wall face behind its pilaster");
                }
            }
        }
    }

    #[test]
    fn pipes_lie_along_their_walls_capped_and_the_floor_under_them_cleared() {
        let lv = (1..=10).map(|seed| generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap()).find(|lv| !lv.pipes.is_empty()).unwrap();
        let (polys, _) = polys(&lv, &KIT).unwrap();
        for p in &lv.pipes {
            let mine: Vec<&Poly> = polys.iter().filter(|q| q.zone == p.room && q.surface.material == "pipe").collect();
            // five faces a leg, two caps, for every run in the room
            let want: usize = lv.pipes.iter().filter(|q| q.room == p.room).map(|q| 5 * (q.path.len() - 1) + 2).sum();
            assert_eq!(mine.len(), want, "{p:?}");
            assert!(mine.iter().all(|q| q.verts.iter().all(|v| v[2] >= p.floor && v[2] <= p.floor + PIPE)));
            // no floor left under the middle of a leg
            for w in p.path.windows(2) {
                let d = [(w[1].0 - w[0].0).signum(), (w[1].1 - w[0].1).signum()];
                let mid = ((w[0].0 + w[1].0) / 2 - d[1] * PIPE / 2, (w[0].1 + w[1].1) / 2 + d[0] * PIPE / 2);
                let under = polys.iter().any(|q| {
                    q.zone == p.room && q.normal[2] > 0.99 && q.surface.material != "pipe" && q.verts.iter().all(|v| v[2] == p.floor) && {
                        let (x0, x1) = (q.verts.iter().map(|v| v[0]).min().unwrap(), q.verts.iter().map(|v| v[0]).max().unwrap());
                        let (y0, y1) = (q.verts.iter().map(|v| v[1]).min().unwrap(), q.verts.iter().map(|v| v[1]).max().unwrap());
                        x0 < mid.0 && mid.0 < x1 && y0 < mid.1 && mid.1 < y1
                    }
                });
                assert!(!under, "{p:?}: floor under it at {mid:?}");
            }
        }
    }

    #[test]
    fn hazard_surrounds_stand_out_of_their_walls_with_nothing_of_the_carvers_behind_them() {
        let lv =
            (1..=10).map(|seed| generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap()).find(|lv| !lv.surrounds.is_empty()).unwrap();
        let (polys, _) = polys(&lv, &KIT).unwrap();
        for s in &lv.surrounds {
            let d = lv.doorways.iter().find(|d| d.a == s.a && d.b == s.b).unwrap();
            let (a, o) = (d.axis, 1 - d.axis);
            let (lo, hi) = ([d.lo.0, d.lo.1, d.lo.2], [d.hi.0, d.hi.1, d.hi.2]);
            let (t, out) = if s.low() { (lo[a], -1) } else { (hi[a], 1) };
            let (w0, w1, top) = (lo[o] - SPLAY.0, hi[o] + SPLAY.0, hi[2] + SPLAY.1);
            let within = |p: &Poly| {
                p.zone == s.room
                    && p.verts.iter().all(|v| w0 <= v[o] && v[o] <= w1 && lo[2] <= v[2] && v[2] <= top && (v[a] - t) * out >= 0 && (v[a] - t) * out <= SURROUND)
            };
            // the stripes: two jambs and two halves of the lintel, facing the opening; the splay
            let stripes = polys.iter().filter(|p| p.surface.material == "hazard" && within(p)).count();
            assert_eq!(stripes, 4, "{s:?}");
            assert!(polys.iter().filter(|p| p.surface.material == "bay_upper" && within(p) && p.normal[a] * out as f64 > 0.5).count() >= 4, "{s:?}: no splay");
            assert_eq!(polys.iter().filter(|p| p.surface.material == "grating" && within(p)).count(), 1, "{s:?}");
            // nothing of the wall left behind it, and no chamfer on this face
            for p in polys.iter().filter(|p| p.zone == s.room && p.normal[a] * out as f64 > 0.99 && p.verts.iter().all(|v| v[a] == t)) {
                let m = |k: usize| p.verts.iter().map(|v| v[k] as f64).sum::<f64>() / p.verts.len() as f64;
                let behind = (w0 as f64) < m(o) && m(o) < w1 as f64 && (lo[2] as f64) < m(2) && m(2) < top as f64;
                assert!(!behind, "{s:?}: a wall face behind it");
            }
            assert!(!polys.iter().any(|p| p.surface.material == "trim" && within(p)), "{s:?}: a chamfer under it");
            // the jambs reach the wall's face on this side
            let jamb_reaches = polys.iter().any(|p| p.surface.material == "jamb" && p.verts.iter().any(|v| v[a] == t) && p.normal[o].abs() > 0.99);
            assert!(jamb_reaches, "{s:?}: the jambs stop short of the face");
        }
    }

    #[test]
    fn portals_stand_across_their_corridors_with_nothing_of_the_carvers_inside_them() {
        let lv = (1..=10).map(|seed| generate(seed, &Options { style: "felicity", ..Options::default() }).unwrap()).find(|lv| !lv.portals.is_empty()).unwrap();
        let (polys, _) = polys(&lv, &KIT).unwrap();
        for p in &lv.portals {
            let (k, o) = (p.axis, 1 - p.axis);
            let (t0, t1) = (p.at - PORTAL_D / 2, p.at + PORTAL_D / 2);
            let inside = |q: &Poly| q.zone == p.room && q.verts.iter().all(|v| t0 <= v[k] && v[k] <= t1);
            let count = |role: &str| polys.iter().filter(|q| q.surface.material == role && inside(q)).count();
            // its light strip, the passage's four sides, sixteen bevel triangles
            assert_eq!(count("glow"), 1, "{p:?}");
            assert_eq!(count("portal"), 4, "{p:?}");
            assert!(polys.iter().filter(|q| inside(q) && q.surface.material == "bay_upper" && q.normal[k].abs() > 0.6).count() >= 16, "{p:?}");
            // the glow is the lintel's underside
            let glow = polys.iter().find(|q| q.surface.material == "glow" && inside(q)).unwrap();
            assert!(glow.verts.iter().all(|v| v[2] == p.floor + LINTEL) && glow.normal[2] < -0.99);
            // nothing of the carver's ceiling over it, nor walls behind it, strictly within its depth
            for q in polys.iter().filter(|q| inside(q) && !matches!(q.surface.material, "glow" | "portal" | "bay_upper" | "bay_floor")) {
                let m = |c: usize| q.verts.iter().map(|v| v[c] as f64).sum::<f64>() / q.verts.len() as f64;
                let within = (t0 as f64) < m(k) && m(k) < t1 as f64;
                assert!(!(within && q.normal[2] < -0.99 && q.verts.iter().all(|v| v[2] == p.top)), "{p:?}: a ceiling face in it");
                assert!(
                    !(within && q.normal[o].abs() > 0.99 && q.verts.iter().all(|v| v[o] == p.across.0 || v[o] == p.across.1)),
                    "{p:?}: a wall face behind it"
                );
            }
            // the floor between the posts is still there
            let mid = (p.across.0 + p.across.1) / 2;
            let floored = polys.iter().any(|q| {
                q.zone == p.room && q.normal[2] > 0.99 && q.verts.iter().all(|v| v[2] == p.floor) && {
                    let span = |c: usize| (q.verts.iter().map(|v| v[c]).min().unwrap(), q.verts.iter().map(|v| v[c]).max().unwrap());
                    let ((a0, a1), (b0, b1)) = (span(k), span(o));
                    a0 <= p.at && p.at <= a1 && b0 <= mid && mid <= b1
                }
            });
            assert!(floored, "{p:?}: no floor through it");
        }
    }

    #[test]
    fn the_swept_frames_are_the_frames_built_by_hand() {
        let lv = generate(3, &Options { style: "felicity", ..Options::default() }).unwrap();
        let trim = KIT.surface("trim", None, None).unwrap();
        let floor = KIT.surface("floor", None, None).unwrap();
        for d in &lv.doorways {
            let swept: Vec<Poly> = frame(d, trim, [floor, floor], [true, true]).into_iter().filter(|p| p.surface.material == "trim").collect();
            let mut got: Vec<Vec<V3>> = swept.iter().map(|p| cycle(&p.verts)).collect();
            let mut want: Vec<Vec<V3>> = frame_by_hand(d).iter().map(|v| cycle(v)).collect();
            got.sort();
            want.sort();
            assert_eq!(got, want, "{} {}", d.a, d.b);
            for p in &swept {
                // facing out of the wall and in towards the opening, u across the bevel (0 to 1)
                assert!(p.normal[d.axis].abs() > 0.7 && p.normal[d.axis].abs() < 0.71);
                let us: Vec<f64> = p.uvs.as_ref().unwrap().iter().map(|t| t.0).collect();
                assert!(us.iter().all(|&u| u.abs() < 1e-9 || (u - 13.0 * 2f64.sqrt() / 18.4).abs() < 1e-9), "{us:?}");
            }
        }
    }

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
        // every arch: a soffit under its curve, slanted, and nothing of the carver's left across
        // the opening wholly above its springing
        assert!(!lv.arches.is_empty());
        for a in &lv.arches {
            let o = 1 - a.axis;
            let within = |p: &Poly| {
                p.verts.iter().all(|v| a.t.0 <= v[a.axis] && v[a.axis] <= a.t.1 && a.s.0 <= v[o] && v[o] <= a.s.1 && a.spring <= v[2] && v[2] <= a.top)
            };
            let soffit = polys.iter().filter(|p| within(p) && p.normal[o].abs() > 0.01 && p.normal[2].abs() > 0.01).count();
            assert!(soffit >= 6, "{} {}: {soffit} soffit pieces", a.a, a.b);
            let flat = polys.iter().filter(|p| within(p) && p.normal[a.axis].abs() < 0.5 && (p.normal[o].abs() < 0.01 || p.normal[2].abs() < 0.01)).count();
            // (the crown's own piece is flat on top: one each side of the middle)
            assert!(flat <= 2, "{} {}: {flat} of the carver's faces left", a.a, a.b);
        }
        // every doorway with no arch: framed on both faces, nothing of the carver's left in the
        // strip round the opening, and its jambs from `TRIM` into the wall
        let framed: Vec<&Doorway> = lv.doorways.iter().filter(|d| !lv.arches.iter().any(|a| a.kind == "doorway" && a.a == d.a && a.b == d.b)).collect();
        assert!(!framed.is_empty());
        for d in framed {
            let (a, o) = (d.axis, 1 - d.axis);
            let (lo, hi) = ([d.lo.0, d.lo.1, d.lo.2], [d.hi.0, d.hi.1, d.hi.2]);
            let near = |p: &Poly| {
                p.verts.iter().all(|v| lo[a] <= v[a] && v[a] <= hi[a] && lo[o] - TRIM <= v[o] && v[o] <= hi[o] + TRIM && lo[2] <= v[2] && v[2] <= hi[2] + TRIM)
            };
            let chamfers = polys.iter().filter(|p| p.surface.material == "trim" && near(p)).count();
            assert_eq!(chamfers, 6, "{} {}", d.a, d.b);
            for p in polys.iter().filter(|p| near(p) && p.surface.material != "trim" && p.normal[a].abs() > 0.99) {
                assert!(
                    p.verts.iter().all(|v| v[o] <= lo[o] - TRIM || v[o] >= hi[o] + TRIM || v[2] >= hi[2] + TRIM),
                    "{} {}: a wall face in the frame",
                    d.a,
                    d.b
                );
            }
            for p in polys.iter().filter(|p| near(p) && p.surface.material == "jamb") {
                assert!(p.verts.iter().all(|v| lo[a] + TRIM <= v[a] && v[a] <= hi[a] - TRIM), "{} {}: a jamb in the frame", d.a, d.b);
            }
        }
    }
}
