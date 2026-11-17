//! Hard-edged light beams, the way Complex paints them (basement Room_2A): a port of
//! `ge_pd_tools/sunbeam.py`.
//!
//! Light falls at a slant through an opening above, and the surfaces it lands on are cut along
//! the beam's outline so lit and unlit pieces can carry different vertex colours. The N64 lights
//! per vertex, so a sharp patch needs its own edges. `split` cuts the polygons a beam reaches and
//! says which pieces it lights; after the bake, `intensity(p, beam)` times a strength is added to
//! the lit pieces' corners.
//!
//! Two things here are Python's and not what Rust would do by itself. Its `sum()` (3.12 on)
//! adds floats with Neumaier's compensation and whole numbers without (`pysum`), and a polygon's
//! area is such a sum; so a 2D point remembers whether the Python holds it as whole numbers
//! (`P2::whole`: a corner of the face being cut) or as floats (a projected or a cut point).
//! And the Python knows a lit polygon by `id()`: here `split` returns the lit pieces' places in
//! the new list, and the caller marks them (`Poly::lit`). `split`'s `snap` is not ported:
//! `oriented` rounds every vertex to whole units whichever way it is set.

use crate::csg::{oriented, Poly, P3};
use autolevel::py;

pub const EPS: f64 = 1e-6;

/// A beam of light.
#[derive(Clone, Debug, PartialEq)]
pub struct Beam {
    /// The way the light travels (downward), any length.
    pub direction: P3,
    /// Convex planar polygons the light must pass through, all of them.
    pub apertures: Vec<Vec<P3>>,
    /// Convex solids as point lists; their shadows are cut out of the patch.
    pub occluders: Vec<Vec<P3>>,
}

impl Beam {
    /// The direction, of length 1.
    pub fn unit(&self) -> P3 {
        let d = self.direction;
        let ln = pysum(d.iter().map(|c| (c * c, false))).sqrt();
        d.map(|c| c / ln)
    }
}

/// Python's `sum()` of numbers that are each a whole number (true) or a float: whole numbers add
/// exactly until the first float, floats after the first add with Neumaier's compensation, and a
/// whole number among them adds without.
pub fn pysum(items: impl IntoIterator<Item = (f64, bool)>) -> f64 {
    let mut items = items.into_iter();
    let mut f = 0.0;
    loop {
        match items.next() {
            None => return f,
            Some((x, true)) => f += x,
            Some((x, false)) => {
                f += x;
                break;
            }
        }
    }
    let mut c = 0.0;
    for (x, whole) in items {
        if whole {
            f += x;
            continue;
        }
        let t = f + x;
        if f.abs() >= x.abs() {
            c += (f - t) + x;
        } else {
            c += (x - t) + f;
        }
        f = t;
    }
    if c != 0.0 && c.is_finite() {
        f += c;
    }
    f
}

/// Lambert factor of the beam on p (0 when p faces away).
pub fn intensity(p: &Poly, beam: &Beam) -> f64 {
    let d = beam.unit();
    let x = -pysum((0..3).map(|k| (p.normal[k] * d[k], false)));
    if x > 0.0 {
        x
    } else {
        0.0
    }
}

// ------------------------------------------------------------ 2D convex ops

/// A point in a face's plane. `whole`: the Python holds it as whole numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
    pub whole: bool,
}

impl P2 {
    /// A point the Python holds as floats.
    pub fn new(x: f64, y: f64) -> P2 {
        P2 { x, y, whole: false }
    }
}

fn area(poly: &[P2]) -> f64 {
    let n = poly.len();
    0.5 * pysum((0..n).map(|i| {
        let (p, q) = (poly[(i + n - 1) % n], poly[i]);
        (p.x * q.y - q.x * p.y, p.whole && q.whole)
    }))
}

fn ccw(poly: Vec<P2>) -> Vec<P2> {
    if area(&poly) >= 0.0 {
        poly
    } else {
        poly.into_iter().rev().collect()
    }
}

fn side(a: P2, b: P2, p: P2) -> f64 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// Split a convex polygon by the line a->b: (the left part, the right part).
fn cut(poly: &[P2], a: P2, b: P2) -> (Vec<P2>, Vec<P2>) {
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let (sp, sq) = (side(a, b, p), side(a, b, q));
        if sp >= -EPS {
            left.push(p);
        }
        if sp <= EPS {
            right.push(p);
        }
        if (sp > EPS && sq < -EPS) || (sp < -EPS && sq > EPS) {
            let t = sp / (sp - sq);
            let x = P2::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t);
            left.push(x);
            right.push(x);
        }
    }
    (left, right)
}

fn clean(poly: &[P2], min_area: f64) -> Option<Vec<P2>> {
    let mut out: Vec<P2> = Vec::new();
    for p in poly {
        if out.last().is_none_or(|l| (p.x - l.x).abs() > 1e-4 || (p.y - l.y).abs() > 1e-4) {
            out.push(*p);
        }
    }
    if out.len() > 1 && (out[0].x - out[out.len() - 1].x).abs() <= 1e-4 && (out[0].y - out[out.len() - 1].y).abs() <= 1e-4 {
        out.pop();
    }
    (out.len() >= 3 && area(&out).abs() >= min_area).then_some(out)
}

/// `poly` and `convex` (both convex, counter-clockwise) -> the convex polygon they share, or None.
pub fn intersect(poly: &[P2], convex: &[P2]) -> Option<Vec<P2>> {
    let mut poly = poly.to_vec();
    for i in 0..convex.len() {
        // (an empty polygon is nothing, as the Python's `if poly` has it)
        poly = if poly.is_empty() { None } else { clean(&cut(&poly, convex[i], convex[(i + 1) % convex.len()]).0, 0.0) }?;
    }
    clean(&poly, 1.0)
}

/// `poly` less `convex` -> (the pieces outside, as convex polygons; the piece inside, or None).
pub fn subtract(poly: &[P2], convex: &[P2]) -> (Vec<Vec<P2>>, Option<Vec<P2>>) {
    let (mut outside, mut rest) = (Vec::new(), Some(poly.to_vec()));
    for i in 0..convex.len() {
        let Some(r) = &rest else { break };
        let (inside, out) = cut(r, convex[i], convex[(i + 1) % convex.len()]);
        if let Some(out) = clean(&out, 1.0) {
            outside.push(out);
        }
        rest = clean(&inside, 0.0);
    }
    (outside, rest.filter(|r| !r.is_empty()).and_then(|r| clean(&r, 1.0)))
}

/// Convex hull, counter-clockwise (monotone chain). Fewer than three points come back as they are.
pub fn hull(points: &[(f64, f64)]) -> Vec<P2> {
    // sorted(set(...)): the first of equal points is the one kept (0.0 and -0.0 are equal)
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for &(x, y) in points {
        let p = (py::round_to(x, 6), py::round_to(y, 6));
        if !pts.contains(&p) {
            pts.push(p);
        }
    }
    pts.sort_by(|a, b| py::cmp_f64(a.0, b.0).then(py::cmp_f64(a.1, b.1)));
    let pts: Vec<P2> = pts.into_iter().map(|(x, y)| P2::new(x, y)).collect();
    if pts.len() < 3 {
        return pts;
    }
    let (mut lower, mut upper): (Vec<P2>, Vec<P2>) = (Vec::new(), Vec::new());
    for &p in &pts {
        while lower.len() >= 2 && side(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && side(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

// ------------------------------------------------------------ projection

/// The axis to drop for an axis-aligned plane, and the two kept axes.
fn frame(normal: P3) -> (usize, [usize; 2]) {
    let axis = py::first_max(0..3, |&k| normal[k].abs()).expect("three axes");
    let kept: Vec<usize> = (0..3).filter(|&k| k != axis).collect();
    (axis, [kept[0], kept[1]])
}

/// Slide points along d onto the plane coord[axis] = value. Points already past the plane
/// (behind it, as seen by the light) are dropped.
fn project(points: &[P3], d: P3, axis: usize, value: f64) -> Vec<P3> {
    let mut out = Vec::new();
    for q in points {
        if d[axis].abs() < EPS {
            return Vec::new();
        }
        let t = (value - q[axis]) / d[axis];
        if t < -EPS {
            continue;
        }
        out.push([q[0] + d[0] * t, q[1] + d[1] * t, q[2] + d[2] * t]);
    }
    out
}

/// Cut the axis-aligned polygons that face the beam along its outline. `zones`: only polygons in
/// these zones (None: all). `below`: only polygons whose top is at or below this z (keeps the cut
/// off surfaces the occluders touch). Returns the new list and where in it the lit pieces are.
/// Polygons the beam does not cut are passed on as they are; the pieces of one it cuts are new.
pub fn split(polys: Vec<Poly>, beam: &Beam, zones: Option<&[&str]>, below: Option<f64>) -> (Vec<Poly>, Vec<usize>) {
    let d = beam.unit();
    let (mut out, mut lit) = (Vec::new(), Vec::new());
    for p in polys {
        if zones.is_some_and(|z| !z.contains(&p.zone.as_str()))
            || intensity(&p, beam) <= EPS
            || below.is_some_and(|b| p.verts.iter().map(|v| v[2]).max().is_some_and(|top| top as f64 > b + 0.5))
        {
            out.push(p);
            continue;
        }
        let (axis, [u, v]) = frame(p.normal);
        if (p.normal[axis].abs() - 1.0).abs() > 1e-3 {
            out.push(p); // not axis-aligned: leave it
            continue;
        }
        let value = p.verts[0][axis] as f64;
        let to2 = |q: &P3| (q[u], q[v]);
        let whole = |verts: &[[i32; 3]]| -> Vec<P2> { verts.iter().map(|q| P2 { x: q[u] as f64, y: q[v] as f64, whole: true }).collect() };
        let face = ccw(whole(&p.verts));
        let mut region = Some(face.clone());
        for ap in &beam.apertures {
            let proj = project(ap, d, axis, value);
            region = match region {
                Some(r) if !r.is_empty() && proj.len() >= 3 => intersect(&r, &ccw(hull(&proj.iter().map(to2).collect::<Vec<_>>()))),
                _ => None,
            };
        }
        let Some(region) = region else {
            out.push(p);
            continue;
        };
        let mut pieces: Vec<(Vec<P2>, bool)> = Vec::new(); // (2D polygon, lit)
        let (dark, _) = subtract(&face, &region);
        pieces.extend(dark.into_iter().map(|q| (q, false)));
        let mut bright = vec![region];
        for occ in &beam.occluders {
            let proj = project(occ, d, axis, value);
            if proj.len() < 3 {
                continue;
            }
            let h = ccw(hull(&proj.iter().map(to2).collect::<Vec<_>>()));
            if h.len() < 3 {
                continue;
            }
            let mut nxt = Vec::new();
            for b in &bright {
                let (outside, inside) = subtract(b, &h);
                nxt.extend(outside);
                if let Some(inside) = inside.filter(|i| !i.is_empty()) {
                    pieces.push((inside, false));
                }
            }
            bright = nxt;
        }
        pieces.extend(bright.into_iter().map(|q| (q, true)));
        for (q2, is_lit) in pieces {
            let verts: Vec<P3> = q2
                .iter()
                .map(|q| {
                    let mut pt = [0.0; 3];
                    pt[axis] = value;
                    pt[u] = q.x;
                    pt[v] = q.y;
                    pt.map(|c| py::round(c) as f64)
                })
                .collect();
            let np = oriented(&verts, p.normal, p.surface, &p.zone, None);
            if np.verts.len() < 3 || area(&whole(&np.verts)).abs() < 0.5 {
                continue; // a sliver that rounding collapsed
            }
            if is_lit {
                lit.push(out.len());
            }
            out.push(np);
        }
    }
    (out, lit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csg::Surface;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<P2> {
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|&(x, y)| P2::new(x, y)).collect()
    }

    #[test]
    fn sums_are_pythons() {
        // sum([0.1] * 10) is 1.0 in Python 3.12, and 0.9999999999999999 added one by one
        assert_eq!(pysum([(0.1, false); 10]), 1.0);
        assert_eq!(pysum([(1e16, false), (1.0, false), (1.0, false)]), 1.0000000000000002e16);
        // whole numbers among floats add without compensation: sum([1e16, 1, 1]) == 1e16
        assert_eq!(pysum([(1e16, false), (1.0, true), (1.0, true)]), 1e16);
        assert_eq!(pysum([(3.0, true), (4.0, true)]), 7.0);
        assert_eq!(pysum([(-0.0, false)]).to_bits(), 0.0f64.to_bits());
        assert_eq!(pysum(Vec::new()), 0.0);
    }

    #[test]
    fn convex_polygons_meet_and_part() {
        let (a, b) = (square(0.0, 0.0, 10.0, 10.0), square(5.0, 5.0, 20.0, 20.0));
        let both = intersect(&a, &b).unwrap();
        assert_eq!(area(&both), 25.0);
        let (outside, inside) = subtract(&a, &b);
        assert_eq!(area(&inside.unwrap()), 25.0);
        assert_eq!(outside.iter().map(|q| area(q)).sum::<f64>(), 75.0);
        assert!(intersect(&a, &square(30.0, 30.0, 40.0, 40.0)).is_none());
        let h = hull(&[(0.0, 0.0), (4.0, 0.0), (2.0, 1.0), (4.0, 4.0), (0.0, 4.0), (4.0, 0.0)]);
        assert_eq!(h.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>(), [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]);
    }

    #[test]
    fn a_beam_cuts_a_lit_patch_out_of_the_floor_under_its_opening() {
        let s = Surface::new("floor", (39.37, 39.37), None);
        let floor = oriented(&[[0.0, 0.0, 0.0], [400.0, 0.0, 0.0], [400.0, 400.0, 0.0], [0.0, 400.0, 0.0]], [0.0, 0.0, 1.0], s, "hall", None);
        let other = oriented(&[[0.0, 0.0, 0.0], [400.0, 0.0, 0.0], [400.0, 400.0, 0.0], [0.0, 400.0, 0.0]], [0.0, 0.0, 1.0], s, "next", None);
        let hole = |z: f64| vec![[100.0, 100.0, z], [200.0, 100.0, z], [200.0, 200.0, z], [100.0, 200.0, z]];
        let beam = Beam { direction: [0.0, 0.0, -1.0], apertures: vec![hole(300.0), hole(260.0)], occluders: Vec::new() };
        let (out, lit) = split(vec![floor, other.clone()], &beam, Some(&["hall"]), Some(236.0));
        assert_eq!(lit.len(), 1);
        let patch = &out[lit[0]];
        let mut corners = patch.verts.clone();
        corners.sort();
        assert_eq!(corners, [[100, 100, 0], [100, 200, 0], [200, 100, 0], [200, 200, 0]]);
        assert_eq!(intensity(patch, &beam), 1.0);
        // the other zone's floor is passed on untouched, and the cut floor's pieces cover it
        assert_eq!(out.last(), Some(&other));
        let covered: f64 =
            out[..out.len() - 1].iter().map(|p| area(&p.verts.iter().map(|v| P2::new(v[0] as f64, v[1] as f64)).collect::<Vec<_>>()).abs()).sum();
        assert_eq!(covered, 160000.0);
    }
}
