//! The shape tool: a 2D profile swept along a path, mitred where the path bends, with explicit
//! UVs, and the clearing step that takes out the carver's faces a piece covers.
//!
//! A path is a start point and legs, each to its next point. Each leg carries its own profile
//! (the same number of points on every leg) in its own two axes square to it: `side`, the
//! profile's first coordinate, and `depth`, its second. Each profile edge (point `i` to `i + 1`)
//! is a face along the leg, or nothing (`None`: where the piece is against a wall or a ceiling).
//! An edge faces `(-Δdepth, Δside)` in the profile, so the points run with the solid on the left
//! of that: list them the way round that faces each edge out of the piece.
//!
//! Where two legs meet, each profile point's line along one leg is met with its line along the
//! next (where they cross: the mitre). The profiles may differ there (a pilaster 13 deep up a wall
//! into a beam 24 deep along the ceiling: the mitre runs from where the pilaster's front meets
//! the beam's underside to the corner of wall and ceiling, as Felicity's braces do). An open
//! path's ends are cut square to their legs; a closed one's last leg comes back to the start and
//! is mitred into the first.
//!
//! UVs: along the path (the distance along the centre line, each vertex's own place along its
//! leg) and round the profile (the distance along the profile from the first point of the run of
//! edges with the same look). An edge says which of u and v runs along the path: `along_v` for
//! a texture whose length runs along (Felicity's frame trim 00EF, u across the bevel), else u.

use crate::csg::{self, Poly, Surface, Uv, P3, V3};

/// How one edge of a profile looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub surface: Surface,
    /// v runs along the path and u round the profile (else the other way).
    pub along_v: bool,
}

/// One leg of a path: from the point before to `to`.
#[derive(Clone, Debug, PartialEq)]
pub struct Leg {
    pub to: P3,
    /// The profile's axes, unit vectors square to the leg.
    pub side: P3,
    pub depth: P3,
    /// (side, depth) points, an open polyline.
    pub profile: Vec<(f64, f64)>,
    /// Each edge's look (one fewer than the points); None: no face.
    pub edges: Vec<Option<Edge>>,
    /// The distance along the path at this leg's start, where it starts again (None: carried on).
    pub along: Option<f64>,
}

/// A profile swept along a path.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep<'a> {
    pub from: P3,
    pub legs: Vec<Leg>,
    /// The last leg ends at `from` and is mitred into the first.
    pub closed: bool,
    pub zone: &'a str,
}

fn add(p: P3, q: P3) -> P3 {
    [p[0] + q[0], p[1] + q[1], p[2] + q[2]]
}

fn sub(p: P3, q: P3) -> P3 {
    [p[0] - q[0], p[1] - q[1], p[2] - q[2]]
}

fn mul(p: P3, k: f64) -> P3 {
    p.map(|c| c * k)
}

fn dot(p: P3, q: P3) -> f64 {
    p[0] * q[0] + p[1] * q[1] + p[2] * q[2]
}

fn unit(p: P3) -> P3 {
    let d = dot(p, p).sqrt();
    if d == 0.0 {
        p
    } else {
        mul(p, 1.0 / d)
    }
}

/// Where the lines `a + s u` and `b + t v` meet (the middle of their closest points; for
/// parallel lines, the middle of `a` and `b`).
fn meet(a: P3, u: P3, b: P3, v: P3) -> P3 {
    let w = sub(a, b);
    let (uu, uv, vv, uw, vw) = (dot(u, u), dot(u, v), dot(v, v), dot(u, w), dot(v, w));
    let den = uu * vv - uv * uv;
    if den.abs() < 1e-9 * uu * vv {
        return mul(add(a, b), 0.5);
    }
    let (s, t) = ((uv * vw - vv * uw) / den, (uu * vw - uv * uw) / den);
    mul(add(add(a, mul(u, s)), add(b, mul(v, t))), 0.5)
}

/// The sweep's polygons: one face per leg and drawn profile edge.
pub fn sweep(s: &Sweep) -> Vec<Poly> {
    let n = s.legs.len();
    if n == 0 {
        return Vec::new();
    }
    let points: Vec<P3> = std::iter::once(s.from).chain(s.legs.iter().map(|l| l.to)).collect();
    let dirs: Vec<P3> = (0..n).map(|i| unit(sub(points[i + 1], points[i]))).collect();
    let offset = |i: usize, j: usize| -> P3 {
        let (w, d) = s.legs[i].profile[j];
        add(mul(s.legs[i].side, w), mul(s.legs[i].depth, d))
    };
    let count = s.legs[0].profile.len();
    // the section at each point of the path: square at an open end, else the mitre
    let section = |k: usize| -> Vec<P3> {
        let at = points[k];
        (0..count)
            .map(|j| {
                let (before, after) = (
                    if k > 0 {
                        Some(k - 1)
                    } else if s.closed {
                        Some(n - 1)
                    } else {
                        None
                    },
                    if k < n {
                        Some(k)
                    } else if s.closed {
                        Some(0)
                    } else {
                        None
                    },
                );
                match (before, after) {
                    (Some(b), Some(a)) => meet(add(at, offset(b, j)), dirs[b], add(at, offset(a, j)), dirs[a]),
                    (Some(b), None) => add(at, offset(b, j)),
                    (None, Some(a)) => add(at, offset(a, j)),
                    (None, None) => at,
                }
            })
            .collect()
    };
    let sections: Vec<Vec<P3>> = (0..=n).map(section).collect();
    let mut out = Vec::new();
    let mut run = 0.0;
    for (i, leg) in s.legs.iter().enumerate() {
        if let Some(a) = leg.along {
            run = a;
        }
        let (start, end) = (&sections[i], if s.closed && i == n - 1 { &sections[0] } else { &sections[i + 1] });
        let along = |v: P3| run + dot(sub(v, points[i]), dirs[i]);
        // round the profile: from the first point of each run of edges that look the same
        let mut round = 0.0;
        for j in 0..count - 1 {
            let (p, q) = (leg.profile[j], leg.profile[j + 1]);
            let length = (q.0 - p.0).hypot(q.1 - p.1);
            if j > 0 && leg.edges[j].map(|e| e.surface) != leg.edges[j - 1].map(|e| e.surface) {
                round = 0.0;
            }
            if let Some(e) = leg.edges[j] {
                let quad = [start[j], end[j], end[j + 1], start[j + 1]];
                let rounds = [round, round, round + length, round + length];
                let normal = add(mul(leg.side, -(q.1 - p.1)), mul(leg.depth, q.0 - p.0));
                let sc = e.surface.scale;
                let mut uvs: Vec<Uv> =
                    quad.iter().zip(rounds).map(|(&v, r)| if e.along_v { (r / sc.0, along(v) / sc.1) } else { (along(v) / sc.0, r / sc.1) }).collect();
                // whole repeats off along the path, to keep the numbers small
                let least = uvs.iter().map(|t| if e.along_v { t.1 } else { t.0 }).fold(f64::INFINITY, f64::min);
                let shift = (least + 1e-6).floor();
                for t in uvs.iter_mut() {
                    if e.along_v {
                        t.1 -= shift;
                    } else {
                        t.0 -= shift;
                    }
                }
                let p = csg::oriented(&quad, unit(normal), e.surface, s.zone, Some(&uvs));
                if p.verts.len() >= 3 {
                    out.push(p);
                }
            }
            round += length;
        }
        run += dot(sub(points[i + 1], points[i]), dirs[i]);
    }
    out
}

/// A convex polygon cut by the plane `v[k] = at`: the part below, the part above (either empty).
pub fn cut(poly: &[V3], k: usize, at: i32) -> (Vec<V3>, Vec<V3>) {
    let (mut below, mut above) = (Vec::new(), Vec::new());
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        if p[k] <= at {
            below.push(p);
        }
        if p[k] >= at {
            above.push(p);
        }
        if (p[k] < at && q[k] > at) || (p[k] > at && q[k] < at) {
            let f = (at - p[k]) as f64 / (q[k] - p[k]) as f64;
            let mut x = [0; 3];
            for c in 0..3 {
                x[c] = if c == k { at } else { (p[c] as f64 + (q[c] - p[c]) as f64 * f).round() as i32 };
            }
            below.push(x);
            above.push(x);
        }
    }
    // a piece wholly on the line (the edge of a face the line runs along) is no piece
    let tidy = |mut v: Vec<V3>| {
        v.dedup();
        if v.len() > 1 && v.first() == v.last() {
            v.pop();
        }
        if v.len() < 3 || v.iter().all(|p| p[k] == at) {
            v.clear();
        }
        v
    };
    (tidy(below), tidy(above))
}

/// A rectangle a piece covers in a plane: the plane `v[axis] = at`, faces in it facing `sign`
/// along `axis`, and the rectangle's two other coordinates (in axis order) from `lo` to `hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cover {
    pub axis: usize,
    pub at: i32,
    pub sign: i32,
    pub lo: [i32; 2],
    pub hi: [i32; 2],
}

impl Cover {
    /// The two axes the rectangle spans, in order.
    pub fn others(&self) -> [usize; 2] {
        match self.axis {
            0 => [1, 2],
            1 => [0, 2],
            _ => [0, 1],
        }
    }

    /// Whether a face lies in the plane, facing the way.
    fn holds(&self, p: &Poly) -> bool {
        p.normal[self.axis] * self.sign as f64 > 0.5 && p.verts.iter().all(|v| v[self.axis] == self.at)
    }
}

/// Take a piece's covers out of the carver's faces: each face in a cover's plane that overlaps it
/// is cut on the rectangle's edges, and the pieces inside it go.
pub fn clear(polys: &mut Vec<Poly>, covers: &[Cover]) {
    for c in covers {
        let [x, y] = c.others();
        let mut out = Vec::with_capacity(polys.len());
        for p in polys.drain(..) {
            let overlaps = || {
                let span = |k: usize| (p.verts.iter().map(|v| v[k]).min().unwrap_or(0), p.verts.iter().map(|v| v[k]).max().unwrap_or(0));
                let ((x0, x1), (y0, y1)) = (span(x), span(y));
                x0 < c.hi[0] && c.lo[0] < x1 && y0 < c.hi[1] && c.lo[1] < y1
            };
            if !c.holds(&p) || !overlaps() {
                out.push(p);
                continue;
            }
            let mut pieces = vec![p.verts.clone()];
            for (k, at) in [(x, c.lo[0]), (x, c.hi[0]), (y, c.lo[1]), (y, c.hi[1])] {
                pieces = pieces
                    .into_iter()
                    .flat_map(|v| {
                        let (b, a) = cut(&v, k, at);
                        [b, a]
                    })
                    .filter(|v| !v.is_empty())
                    .collect();
            }
            for v in pieces {
                let m = v.len() as f64;
                let (mx, my) = (v.iter().map(|q| q[x] as f64).sum::<f64>() / m, v.iter().map(|q| q[y] as f64).sum::<f64>() / m);
                if (c.lo[0] as f64) < mx && mx < c.hi[0] as f64 && (c.lo[1] as f64) < my && my < c.hi[1] as f64 {
                    continue;
                }
                out.push(Poly { verts: v, ..p.clone() });
            }
        }
        *polys = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface() -> Surface {
        Surface::new("trim", (10.0, 100.0), None)
    }

    /// A beam's profile, 40 wide and `deep`, its top against the surface it hangs from.
    fn beam(deep: f64) -> (Vec<(f64, f64)>, Vec<Option<Edge>>) {
        let e = Some(Edge { surface: surface(), along_v: false });
        (vec![(-20.0, 0.0), (-20.0, deep), (20.0, deep), (20.0, 0.0)], vec![e, e, e])
    }

    #[test]
    fn a_pilaster_mitres_into_a_deeper_beam_as_felicitys_do() {
        // Felicity's room 4 (PD axes turned to the plan's: z up): a pilaster 13 deep up the wall
        // at x = 0, into a beam 24 deep under the ceiling at 259
        let (p13, e) = beam(13.0);
        let (p24, _) = beam(24.0);
        let s = Sweep {
            from: [0.0, 0.0, 0.0],
            legs: vec![
                Leg { to: [0.0, 0.0, 259.0], side: [0.0, 1.0, 0.0], depth: [1.0, 0.0, 0.0], profile: p13, edges: e.clone(), along: None },
                Leg { to: [400.0, 0.0, 259.0], side: [0.0, 1.0, 0.0], depth: [0.0, 0.0, -1.0], profile: p24, edges: e, along: None },
            ],
            closed: false,
            zone: "r",
        };
        let polys = sweep(&s);
        assert_eq!(polys.len(), 6);
        // the pilaster's front: 13 out, from the floor to the beam's underside 24 down
        let front = polys.iter().find(|p| p.normal[0] > 0.99).unwrap();
        let mut got: Vec<V3> = front.verts.clone();
        got.sort();
        assert_eq!(got, vec![[13, -20, 0], [13, -20, 235], [13, 20, 0], [13, 20, 235]]);
        // the beam's side runs from the mitre (13 out, 24 down) to the corner of wall and ceiling
        let side = polys.iter().find(|p| p.normal[1] < -0.99 && p.verts.iter().any(|v| v[0] == 400)).unwrap();
        let mut got: Vec<V3> = side.verts.clone();
        got.sort();
        assert_eq!(got, vec![[0, -20, 259], [13, -20, 235], [400, -20, 235], [400, -20, 259]]);
        // every face faces out of the piece; UVs along the path
        for p in &polys {
            assert_eq!(p.uvs.as_ref().map(|u| u.len()), Some(p.verts.len()));
        }
        let under = polys.iter().find(|p| p.normal[2] < -0.99).unwrap();
        let us: Vec<f64> = under.uvs.as_ref().unwrap().iter().map(|t| t.0).collect();
        let span = us.iter().cloned().fold(f64::MIN, f64::max) - us.iter().cloned().fold(f64::MAX, f64::min);
        assert!((span - 387.0 / 10.0).abs() < 0.01, "{span}");
    }

    #[test]
    fn a_closed_path_is_mitred_all_round() {
        let e = Some(Edge { surface: surface(), along_v: true });
        let leg = |to: P3, side: P3| Leg { to, side, depth: [0.0, 1.0, 0.0], profile: vec![(0.0, 10.0), (-10.0, 0.0)], edges: vec![e], along: None };
        // round a 100 x 100 opening in the plane y = 0, the profile facing in and out of the wall
        let s = Sweep {
            from: [0.0, 0.0, 0.0],
            legs: vec![
                leg([0.0, 0.0, 100.0], [1.0, 0.0, 0.0]),
                leg([100.0, 0.0, 100.0], [0.0, 0.0, -1.0]),
                leg([100.0, 0.0, 0.0], [-1.0, 0.0, 0.0]),
                leg([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ],
            closed: true,
            zone: "r",
        };
        let polys = sweep(&s);
        assert_eq!(polys.len(), 4);
        for p in &polys {
            assert_eq!(p.verts.len(), 4);
            // each face is a trapezoid from the opening's edge (10 into the wall) to 10 outside it
            assert!(p.verts.iter().all(|v| (v[1] == 10 && (0..=100).contains(&v[0]) && (0..=100).contains(&v[2])) || v[1] == 0));
            assert!(p.verts.iter().filter(|v| v[1] == 0).all(|v| v[0] == -10 || v[0] == 110 || v[2] == -10 || v[2] == 110));
        }
    }

    #[test]
    fn a_cover_cuts_a_face_and_drops_what_it_covers() {
        let wall = csg::oriented(&[[0.0, 0.0, 0.0], [212.0, 0.0, 0.0], [212.0, 0.0, 212.0], [0.0, 0.0, 212.0]], [0.0, 1.0, 0.0], surface(), "r", None);
        let mut polys = vec![wall];
        clear(&mut polys, &[Cover { axis: 1, at: 0, sign: 1, lo: [80, 0], hi: [120, 212] }]);
        // two pieces left, either side of the strip
        assert_eq!(polys.len(), 2);
        assert!(polys.iter().all(|p| p.verts.iter().all(|v| v[0] <= 80) || p.verts.iter().all(|v| v[0] >= 120)));
        // a face facing the other way is left alone
        let back = csg::oriented(&[[0.0, 0.0, 0.0], [0.0, 0.0, 212.0], [212.0, 0.0, 212.0], [212.0, 0.0, 0.0]], [0.0, -1.0, 0.0], surface(), "r", None);
        let mut polys = vec![back];
        clear(&mut polys, &[Cover { axis: 1, at: 0, sign: 1, lo: [80, 0], hi: [120, 212] }]);
        assert_eq!(polys.len(), 1);
    }
}
