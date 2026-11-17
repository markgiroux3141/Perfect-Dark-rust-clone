//! Axis-aligned CSG for level blockouts: a port of `ge_pd_tools/csg.py`, held to the answer key
//! (`tests/key/polys.json`) through `geometry::polys`.
//!
//! Space starts solid. `Carver::carve` hollows out boxes of air and `solid` fills boxes back in.
//! Ops apply in order, so later ones win. `polygons` returns the boundary between air and solid
//! as polygons facing into the air: coplanar cell faces with the same look are greedily merged
//! into rectangles, those are re-split on a lattice (vertex lighting needs vertices), and
//! T-junctions are repaired (`fix_t_junctions`), so the mesh has no cracks. Air cells in different
//! zones join without a wall, which is how rooms connect. `portal` rectangles are left open onto
//! geometry outside the CSG. How each face looks comes from a `style(&Face) -> Surface` callback.
//!
//! `duct`, `plate_ramp` and `rail` make the pieces that are not axis-aligned in the same `Poly`
//! form. Vertices are whole units (every vertex of a generated level is), so `oriented`'s
//! `ndigits` (free-form geometry off the axes) is not ported. Where the Python takes whole
//! numbers or floats alike (points, widths, heights), this takes `f64`: whole numbers that size
//! are exact in it, and every expression is written in the Python's order.
//!
//! The order polygons come out in is the Python's, which rests on its dicts keeping the order
//! things were first put in: the air cells (`Air`) and the groups of faces with one look.

use autolevel::py;
use std::array::from_fn;
use std::collections::{BTreeSet, HashMap};

/// A vertex, in whole units.
pub type V3 = [i32; 3];
/// A point or a direction before it is rounded.
pub type P3 = [f64; 3];
/// A texture coordinate.
pub type Uv = (f64, f64);
/// A lattice's spacing along each axis (None: split evenly, only when longer than `max_span`).
pub type Lattice = [Option<f64>; 3];

/// Polygons are split every this many units along a ramp or a rail (the Python's `step=118`).
pub const STEP: f64 = 118.0;
/// `fix_t_junctions`: how far off an edge's line a vertex may be and still be put into it.
pub const TOL: f64 = 0.51;
/// `fix_t_junctions`: the size of the cells its vertices are indexed by.
pub const CELL: i64 = 64;

/// A plan's point as floats.
pub fn f3(p: (i32, i32, i32)) -> P3 {
    [p.0 as f64, p.1 as f64, p.2 as f64]
}

/// What a polygon looks like. `scale` is world units per texture repeat (u, v). For walls, `band`
/// puts v=0 at that height, which is how GE's ClampT wall strips work; None means v follows world
/// height and wraps. `slope` tilts the band: its bottom rises `slope` units per unit along the
/// wall, so a strip can run parallel to a ramp. `offset` shifts where repeats start on floors and
/// ceilings. Two surfaces are the same look when every field is equal (0.0 and -0.0 are).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    /// The material, by its role in the kit's palette (`complex::PALETTE`).
    pub material: &'static str,
    pub scale: (f64, f64),
    pub band: Option<f64>,
    pub slope: f64,
    pub offset: (f64, f64),
}

impl Surface {
    /// A surface with no slope and no offset.
    pub fn new(material: &'static str, scale: (f64, f64), band: Option<f64>) -> Surface {
        Surface { material, scale, band, slope: 0.0, offset: (0.0, 0.0) }
    }

    /// The surface as something to hash: equal surfaces give equal keys.
    fn key(&self) -> (&'static str, [u64; 5], Option<u64>) {
        let b = |x: f64| (x + 0.0).to_bits();
        (self.material, [b(self.scale.0), b(self.scale.1), b(self.slope), b(self.offset.0), b(self.offset.1)], self.band.map(b))
    }
}

/// One boundary cell face, as passed to the style callback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face<'a> {
    /// The normal is `sign` along this axis.
    pub axis: usize,
    pub sign: i32,
    pub lo: V3,
    pub hi: V3,
    /// The zone of the air side.
    pub zone: &'a str,
    /// The tag of the solid side ("void" outside every solid op).
    pub tag: &'a str,
}

impl Face<'_> {
    /// "wall", "floor" or "ceiling".
    pub fn kind(&self) -> &'static str {
        if self.axis != 2 {
            "wall"
        } else if self.sign > 0 {
            "floor"
        } else {
            "ceiling"
        }
    }
}

/// A polygon of the level.
#[derive(Clone, Debug, PartialEq)]
pub struct Poly {
    /// Counter-clockwise seen from the side the normal points to.
    pub verts: Vec<V3>,
    pub normal: P3,
    pub surface: Surface,
    pub zone: String,
    /// Explicit UVs, one for each vertex (rails, grates); None: the world projection.
    pub uvs: Option<Vec<Uv>>,
    /// The beam that lights it: an index into the beams `geometry::polys` returns. Set on the
    /// pieces `sunbeam::split` cuts out as lit; a polygon a later beam cuts again is new.
    pub lit: Option<usize>,
    /// Made by the `Carver`. The Python holds such a polygon's normal as whole numbers, whose
    /// zeros have no sign: here they are +0.0, and `project_uvs` negates them as whole numbers.
    pub carved: bool,
}

/// What an op puts in its box: air of a zone, or a solid with a tag (numbers into the carver's lists).
#[derive(Clone, Copy, Debug)]
enum Fill {
    Air(usize),
    Solid(usize),
}

/// A cell of the grid: its number along each axis.
type Idx = [i32; 3];

/// The air cells, in the order a Python dict would hold them: a cell carved twice keeps its
/// place, and one a solid took out and a later carve put back goes to the end.
#[derive(Default)]
struct Air {
    at: HashMap<Idx, usize>,
    order: Vec<Option<(Idx, usize)>>,
}

impl Air {
    fn set(&mut self, c: Idx, zone: usize) {
        match self.at.get(&c) {
            Some(&i) => self.order[i] = Some((c, zone)),
            None => {
                self.at.insert(c, self.order.len());
                self.order.push(Some((c, zone)));
            }
        }
    }

    fn pop(&mut self, c: Idx) {
        if let Some(i) = self.at.remove(&c) {
            self.order[i] = None;
        }
    }
}

/// The carver's grid: every box's sides as lines, the air cells and the solid cells' tags.
struct Grid {
    lines: [Vec<i32>; 3],
    air: Air,
    tag: HashMap<Idx, usize>,
}

/// Faces of one look in one plane: (axis, value, sign, zone, surface, the cells' rectangles).
type Group<'a> = (usize, i32, i32, &'a str, Surface, Vec<[i32; 4]>);

/// The CSG: boxes of air and solid in order, and the rectangles left open.
#[derive(Clone, Debug)]
pub struct Carver {
    ops: Vec<((V3, V3), Fill)>,
    portals: Vec<(V3, V3)>,
    extra: [Vec<i32>; 3],
    zones: Vec<String>,
    tags: Vec<String>,
    /// The spacing merged faces are split on again.
    pub lattice: Lattice,
    /// A point on the lattice.
    pub origin: P3,
    /// With no lattice along an axis, faces longer than this are split evenly (0: never).
    pub max_span: i32,
    /// Zones with a lattice of their own: (zone, (lattice, origin)). Finer vertices in narrow
    /// vents, where lights only reach the edges at grazing angles.
    pub zone_lattice: Vec<(String, (Lattice, P3))>,
}

impl Default for Carver {
    /// The Python's defaults: a 118 lattice in x and y, and spans of 118.
    fn default() -> Self {
        Carver::new([Some(118.0), Some(118.0), None], 118)
    }
}

/// The number of `s` in `list`, added if it is new.
fn intern(list: &mut Vec<String>, s: &str) -> usize {
    list.iter().position(|q| q == s).unwrap_or_else(|| {
        list.push(s.to_string());
        list.len() - 1
    })
}

/// A box from two corners, rounded to whole units, low corner first (`_box`).
fn bx(lo: P3, hi: P3) -> (V3, V3) {
    let (lo, hi) = (lo.map(|c| py::round(c) as i32), hi.map(|c| py::round(c) as i32));
    (from_fn(|i| lo[i].min(hi[i])), from_fn(|i| lo[i].max(hi[i])))
}

impl Carver {
    pub fn new(lattice: Lattice, max_span: i32) -> Carver {
        Carver {
            ops: Vec::new(),
            portals: Vec::new(),
            extra: [Vec::new(), Vec::new(), Vec::new()],
            zones: Vec::new(),
            tags: Vec::new(),
            lattice,
            origin: [0.0; 3],
            max_span,
            zone_lattice: Vec::new(),
        }
    }

    /// Hollow out a box of air.
    pub fn carve(&mut self, lo: P3, hi: P3, zone: &str) -> &mut Self {
        let z = intern(&mut self.zones, zone);
        self.ops.push((bx(lo, hi), Fill::Air(z)));
        self
    }

    /// Fill a box back in.
    pub fn solid(&mut self, lo: P3, hi: P3, tag: &str) -> &mut Self {
        let t = intern(&mut self.tags, tag);
        self.ops.push((bx(lo, hi), Fill::Solid(t)));
        self
    }

    /// Leave the boundary open inside this flat rectangle (lo and hi share one coordinate).
    pub fn portal(&mut self, lo: P3, hi: P3) -> Result<&mut Self, String> {
        let b = bx(lo, hi);
        if (0..3).filter(|&a| b.0[a] == b.1[a]).count() != 1 {
            return Err("portal must be flat along exactly one axis".to_string());
        }
        self.portals.push(b);
        Ok(self)
    }

    /// Force grid lines, e.g. the heights where wall texture bands change.
    pub fn add_lines(&mut self, axis: usize, values: &[i32]) -> &mut Self {
        self.extra[axis].extend(values);
        self
    }

    // ------------------------------------------------------------ internals

    fn grid(&self) -> Grid {
        let lines: [Vec<i32>; 3] = from_fn(|a| {
            let mut s: BTreeSet<i32> = self.extra[a].iter().copied().collect();
            for ((lo, hi), _) in &self.ops {
                s.extend([lo[a], hi[a]]);
            }
            for (lo, hi) in &self.portals {
                s.extend([lo[a], hi[a]]);
            }
            s.into_iter().collect()
        });
        let at = |a: usize, v: i32| lines[a].partition_point(|&q| q < v) as i32;
        let (mut air, mut tag) = (Air::default(), HashMap::new());
        for ((lo, hi), fill) in &self.ops {
            let r: [(i32, i32); 3] = from_fn(|a| (at(a, lo[a]), at(a, hi[a])));
            for i in r[0].0..r[0].1 {
                for j in r[1].0..r[1].1 {
                    for k in r[2].0..r[2].1 {
                        let c = [i, j, k];
                        match *fill {
                            Fill::Air(zone) => air.set(c, zone),
                            Fill::Solid(t) => {
                                air.pop(c);
                                tag.insert(c, t);
                            }
                        }
                    }
                }
            }
        }
        Grid { lines, air, tag }
    }

    fn in_portal(&self, axis: usize, lo: V3, hi: V3) -> bool {
        self.portals
            .iter()
            .any(|(plo, phi)| plo[axis] == phi[axis] && plo[axis] == lo[axis] && (0..3).all(|b| b == axis || (plo[b] <= lo[b] && hi[b] <= phi[b])))
    }

    /// Every air/solid boundary cell face, with its cell's index, in the Python's order: the air
    /// cells as first carved, and for each the axes in turn, the low side then the high.
    pub fn faces<'a>(&'a self, mut each: impl FnMut(&Face<'a>, [i32; 3]) -> Result<(), String>) -> Result<(), String> {
        let g = self.grid();
        for &(idx, zone) in g.air.order.iter().flatten() {
            for a in 0..3 {
                for s in [-1, 1] {
                    let mut nb = idx;
                    nb[a] += s;
                    if g.air.at.contains_key(&nb) {
                        continue;
                    }
                    let mut lo: V3 = from_fn(|b| g.lines[b][idx[b] as usize]);
                    let mut hi: V3 = from_fn(|b| g.lines[b][idx[b] as usize + 1]);
                    let v = if s > 0 { hi[a] } else { lo[a] };
                    lo[a] = v;
                    hi[a] = v;
                    if self.in_portal(a, lo, hi) {
                        continue;
                    }
                    let tag = g.tag.get(&nb).map_or("void", |&t| self.tags[t].as_str());
                    each(&Face { axis: a, sign: -s, lo, hi, zone: &self.zones[zone], tag }, idx)?;
                }
            }
        }
        Ok(())
    }

    /// Split points strictly inside (a, b) along `axis` (`_cuts`).
    fn cuts(&self, axis: usize, a: i32, b: i32, zone: &str) -> Vec<i32> {
        let (lattice, origin) = self.zone_lattice.iter().find(|(z, _)| z == zone).map_or((self.lattice, self.origin), |(_, v)| *v);
        if let Some(step) = lattice[axis].filter(|&s| s != 0.0) {
            let o = origin[axis];
            let k0 = ((a as f64 - o) / step).floor() as i64 + 1;
            let k1 = ((b as f64 - o) / step).ceil() as i64 - 1;
            return (k0..=k1).map(|k| py::round(o + k as f64 * step) as i32).filter(|&p| a < p && p < b).collect();
        }
        let n = if self.max_span != 0 { ((b - a) as f64 / self.max_span as f64).ceil() as i64 } else { 1 };
        (1..n).map(|i| py::round(a as f64 + ((b - a) as i64 * i) as f64 / n as f64) as i32).collect()
    }

    /// The boundary as polygons. `style(&Face)` gives a face's surface, or None to leave it out.
    /// `repair`: fix T-junctions before returning.
    pub fn polygons(&self, mut style: impl FnMut(&Face) -> Result<Option<Surface>, String>, repair: bool) -> Result<Vec<Poly>, String> {
        type Key<'a> = (usize, i32, i32, &'a str, (&'static str, [u64; 5], Option<u64>));
        let mut groups: Vec<Group> = Vec::new();
        let mut number: HashMap<Key, usize> = HashMap::new();
        self.faces(|f, _| {
            let Some(surf) = style(f)? else { return Ok(()) };
            let a = f.axis;
            let (b, c) = ((a + 1) % 3, (a + 2) % 3);
            let g = *number.entry((a, f.lo[a], f.sign, f.zone, surf.key())).or_insert_with(|| {
                groups.push((a, f.lo[a], f.sign, f.zone, surf, Vec::new()));
                groups.len() - 1
            });
            groups[g].5.push([f.lo[b], f.lo[c], f.hi[b], f.hi[c]]);
            Ok(())
        })?;

        let mut polys = Vec::new();
        for (a, val, sign, zone, surf, cells) in groups {
            let (b, c) = ((a + 1) % 3, (a + 2) % 3);
            let bs: Vec<i32> = cells.iter().flat_map(|k| [k[0], k[2]]).collect::<BTreeSet<_>>().into_iter().collect();
            let cs: Vec<i32> = cells.iter().flat_map(|k| [k[1], k[3]]).collect::<BTreeSet<_>>().into_iter().collect();
            let (nb, nc) = (bs.len(), cs.len());
            let at = |i: usize, j: usize| i * nc + j;
            let (mut occ, mut used) = (vec![false; nb * nc], vec![false; nb * nc]);
            for k in &cells {
                occ[at(bs.partition_point(|&q| q < k[0]), cs.partition_point(|&q| q < k[1]))] = true;
            }
            let mut n = [0.0; 3];
            n[a] = sign as f64;
            for i0 in 0..nb {
                for j0 in 0..nc {
                    if !occ[at(i0, j0)] || used[at(i0, j0)] {
                        continue;
                    }
                    let free = |used: &[bool], i: usize, j: usize| i < nb && j < nc && occ[at(i, j)] && !used[at(i, j)];
                    let mut i1 = i0;
                    while free(&used, i1 + 1, j0) {
                        i1 += 1;
                    }
                    let mut j1 = j0;
                    while (i0..=i1).all(|x| free(&used, x, j1 + 1)) {
                        j1 += 1;
                    }
                    for x in i0..=i1 {
                        for y in j0..=j1 {
                            used[at(x, y)] = true;
                        }
                    }
                    let (b0, b1, c0, c1) = (bs[i0], bs[i1 + 1], cs[j0], cs[j1 + 1]);
                    let ends = |lo: i32, mid: Vec<i32>, hi: i32| [vec![lo], mid, vec![hi]].concat();
                    let bcut = ends(b0, self.cuts(b, b0, b1, zone), b1);
                    let ccut = ends(c0, self.cuts(c, c0, c1, zone), c1);
                    for u in bcut.windows(2) {
                        for w in ccut.windows(2) {
                            let mut q: Vec<V3> =
                                [(u[0], w[0]), (u[1], w[0]), (u[1], w[1]), (u[0], w[1])].iter().map(|&(u, w)| pt(a, val, b, u, c, w)).collect();
                            if sign < 0 {
                                q.reverse();
                            }
                            polys.push(Poly { verts: q, normal: n, surface: surf, zone: zone.to_string(), uvs: None, lit: None, carved: true });
                        }
                    }
                }
            }
        }
        if repair {
            fix_t_junctions(&mut polys);
        }
        Ok(polys)
    }
}

fn pt(a: usize, av: i32, b: usize, bv: i32, c: usize, cv: i32) -> V3 {
    let mut p = [0; 3];
    p[a] = av;
    p[b] = bv;
    p[c] = cv;
    p
}

/// `math.hypot`. CPython's is correctly rounded and the C library's need not be, so where one
/// side is nothing (a wall or a rail along an axis, as all the generator's are) the answer is
/// the other side, which is exact.
pub fn hypot(x: f64, y: f64) -> f64 {
    if x == 0.0 {
        y.abs()
    } else if y == 0.0 {
        x.abs()
    } else {
        x.hypot(y)
    }
}

/// World-space projection per Surface. Each polygon is shifted by whole repeats to keep UVs
/// small (N64 texture coordinates are s10.5). Polygons with explicit `uvs` keep them.
pub fn project_uvs(p: &Poly) -> Vec<Uv> {
    if let Some(uvs) = p.uvs.as_ref().filter(|u| !u.is_empty()) {
        return uvs.clone();
    }
    let (n, s) = (p.normal, &p.surface);
    let wall = n[2].abs() <= 0.7;
    let uv: Vec<Uv> = if !wall {
        p.verts.iter().map(|v| ((v[0] as f64 - s.offset.0) / s.scale.0, (v[1] as f64 - s.offset.1) / s.scale.1)).collect()
    } else {
        let d = hypot(n[0], n[1]);
        let h = if d != 0.0 { d } else { 1.0 };
        // a carved face's normal is whole numbers in the Python, and a whole 0 negated has no sign
        let neg = |x: f64| if p.carved && x == 0.0 { 0.0 } else { -x };
        let (rx, ry) = (neg(n[1]) / h, n[0] / h); // "right" when looking at the wall
        let base = s.band.unwrap_or(0.0);
        let along = |v: &V3| v[0] as f64 * rx + v[1] as f64 * ry;
        p.verts.iter().map(|v| (along(v) / s.scale.0, (v[2] as f64 - base - s.slope * along(v)) / s.scale.1)).collect()
    };
    let least = |f: fn(&Uv) -> f64| uv.iter().map(f).fold(f64::INFINITY, |a, b| if b < a { b } else { a });
    let du = (least(|t| t.0) + 1e-6).floor();
    let dv = if s.band.is_some() && wall { 0.0 } else { (least(|t| t.1) + 1e-6).floor() };
    uv.iter().map(|&(u, v)| (u - du, v - dv)).collect()
}

// ----------------------------------------------------------- free-form pieces

fn dot(p: P3, q: P3) -> f64 {
    p[0] * q[0] + p[1] * q[1] + p[2] * q[2]
}

/// A polygon's normal by Newell's method, of length 1 (all zeros for a polygon of no area).
pub fn newell(verts: &[V3]) -> P3 {
    let mut n = [0.0f64; 3];
    for (i, p) in verts.iter().enumerate() {
        let (p, q) = (p.map(i64::from), verts[(i + 1) % verts.len()].map(i64::from));
        n[0] += ((p[1] - q[1]) * (p[2] + q[2])) as f64;
        n[1] += ((p[2] - q[2]) * (p[0] + q[0])) as f64;
        n[2] += ((p[0] - q[0]) * (p[1] + q[1])) as f64;
    }
    let d = dot(n, n).sqrt();
    let ln = if d != 0.0 { d } else { 1.0 };
    n.map(|x| x / ln)
}

/// A polygon from `verts`, flipped if need be so its normal agrees with `want`. Explicit `uvs`
/// (one for each vertex) follow the vertices. Coordinates are rounded to whole units, and a
/// vertex equal to the one before it (the first: to the last) is dropped.
pub fn oriented(verts: &[P3], want: P3, surface: Surface, zone: &str, uvs: Option<&[Uv]>) -> Poly {
    let all: Vec<V3> = verts.iter().map(|v| v.map(|c| py::round(c) as i32)).collect();
    let keep: Vec<usize> = (0..all.len()).filter(|&i| all[i] != all[if i == 0 { all.len() - 1 } else { i - 1 }]).collect();
    let mut verts: Vec<V3> = keep.iter().map(|&i| all[i]).collect();
    let mut uvs: Option<Vec<Uv>> = uvs.filter(|u| !u.is_empty()).map(|u| keep.iter().map(|&i| u[i]).collect());
    let mut n = newell(&verts);
    if dot(n, want) < 0.0 {
        verts.reverse();
        if let Some(u) = uvs.as_mut() {
            u.reverse();
        }
        n = n.map(|x| -x);
    }
    Poly { verts, normal: n, surface, zone: zone.to_string(), uvs, lit: None, carved: false }
}

/// Which axis a ramp runs along: x if its ends differ in x, or else y.
fn run_axis(start: P3, end: P3) -> usize {
    if start[0] != end[0] {
        0
    } else {
        1
    }
}

/// How many pieces a length is split into: `max(1, math.ceil(length / step))`.
fn pieces(length: f64, step: f64) -> i64 {
    ((length / step).ceil() as i64).max(1)
}

/// A rectangular tube between floor-centre points `start` and `end`. They may differ in z (a
/// ramp) but must share x or y. The ends are left open (make them CSG portals or wall holes).
/// Faces point inwards. `surfaces`: the floor's, the ceiling's and the walls'.
pub fn duct(start: P3, end: P3, width: f64, height: f64, zone: &str, surfaces: [Surface; 3], step: f64) -> Vec<Poly> {
    let [floor, ceiling, wall] = surfaces;
    let run = run_axis(start, end);
    let side = 1 - run;
    let length = (end[run] - start[run]).abs();
    let n = pieces(length, step);
    let hw = width / 2.0;
    let mut out = Vec::new();
    let at = |t: f64, off: f64, up: f64| -> P3 {
        let mut p: P3 = from_fn(|i| start[i] + (end[i] - start[i]) * t);
        p[side] += off;
        p[2] += up;
        p
    };
    let (mw, mh) = (pieces(width, step), pieces(height, step));
    let mut e = [0.0; 3];
    e[side] = 1.0;
    for i in 0..n {
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        for j in 0..mw {
            let (s0, s1) = (-hw + width * j as f64 / mw as f64, -hw + width * (j + 1) as f64 / mw as f64);
            out.push(oriented(&[at(t0, s0, 0.0), at(t0, s1, 0.0), at(t1, s1, 0.0), at(t1, s0, 0.0)], [0.0, 0.0, 1.0], floor, zone, None));
            out.push(oriented(&[at(t0, s0, height), at(t1, s0, height), at(t1, s1, height), at(t0, s1, height)], [0.0, 0.0, -1.0], ceiling, zone, None));
        }
        for j in 0..mh {
            let (h0, h1) = (height * j as f64 / mh as f64, height * (j + 1) as f64 / mh as f64);
            out.push(oriented(&[at(t0, -hw, h0), at(t1, -hw, h0), at(t1, -hw, h1), at(t0, -hw, h1)], e, wall, zone, None));
            out.push(oriented(&[at(t0, hw, h0), at(t0, hw, h1), at(t1, hw, h1), at(t1, hw, h0)], e.map(|x| -x), wall, zone, None));
        }
    }
    out
}

/// An open ramp in the style of GE/PD's big rooms (Complex rooms 18 and 27): a thin sloped sheet
/// between centre-line points `start` and `end` (sharing x or y), with open space underneath.
/// Use a double-sided (CullBoth) material. `edges` picks which sides (-1: the low-coordinate
/// side, 1: the high one) get a strip hanging `edge` units below the sheet. It tapers out where
/// it meets the floor. Split every `STEP`.
pub fn plate_ramp(start: P3, end: P3, width: f64, zone: &str, surface: Surface, edge: f64, edges: &[i32]) -> Vec<Poly> {
    let run = run_axis(start, end);
    let side = 1 - run;
    let n = pieces((end[run] - start[run]).abs(), STEP);
    let hw = width / 2.0;
    let floor_z = if end[2] < start[2] { end[2] } else { start[2] };
    let at = |t: f64, off: f64| -> P3 {
        let mut p: P3 = from_fn(|i| start[i] + (end[i] - start[i]) * t);
        p[side] += off;
        p[2] += 0.0;
        p
    };
    let m = pieces(width, STEP);
    let mut out = Vec::new();
    for i in 0..n {
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        for j in 0..m {
            let (s0, s1) = (-hw + width * j as f64 / m as f64, -hw + width * (j + 1) as f64 / m as f64);
            out.push(oriented(&[at(t0, s0), at(t0, s1), at(t1, s1), at(t1, s0)], [0.0, 0.0, 1.0], surface, zone, None));
        }
        for &s in edges {
            let mut e = [0.0; 3];
            e[side] = s as f64;
            let (top0, top1) = (at(t0, s as f64 * hw), at(t1, s as f64 * hw));
            let under = |top: P3| [top[0], top[1], if floor_z > top[2] - edge { floor_z } else { top[2] - edge }];
            let (b0, b1) = (under(top0), under(top1));
            if b0[2] < top0[2] || b1[2] < top1[2] {
                out.push(oriented(&[top0, top1, b1, b0], e, surface, zone, None));
            }
        }
    }
    out
}

/// A vertical strip from p0 to p1 (their z may differ, for sloped rails), `height` tall and split
/// every `STEP` units. `uv_repeat`: units per texture repeat along the rail; gives explicit UVs
/// (v 0 at the bottom edge, 1 at the top), so a sloped rail's texture follows the slope instead
/// of repeating up it.
pub fn rail(p0: P3, p1: P3, height: f64, zone: &str, surface: Surface, facing: P3, uv_repeat: Option<f64>) -> Vec<Poly> {
    let d: P3 = from_fn(|i| p1[i] - p0[i]);
    let run = hypot(d[0], d[1]);
    let n = pieces(run, STEP);
    let mut out = Vec::new();
    for i in 0..n {
        let a: P3 = from_fn(|k| p0[k] + d[k] * i as f64 / n as f64);
        let b: P3 = from_fn(|k| p0[k] + d[k] * (i + 1) as f64 / n as f64);
        let uvs = uv_repeat.filter(|&r| r != 0.0).map(|r| {
            let (ua, ub) = (run * i as f64 / n as f64 / r, run * (i + 1) as f64 / n as f64 / r);
            [(ua, 0.0), (ub, 0.0), (ub, 1.0), (ua, 1.0)]
        });
        out.push(oriented(&[a, b, [b[0], b[1], b[2] + height], [a[0], a[1], a[2] + height]], facing, surface, zone, uvs.as_ref().map(|u| &u[..])));
    }
    out
}

/// Insert every vertex that lies on another polygon's edge into that edge (within `TOL`).
pub fn fix_t_junctions(polys: &mut [Poly]) {
    let cell = |v: [i64; 3]| v.map(|c| c.div_euclid(CELL));
    let wide = |v: &V3| v.map(i64::from);
    let verts: BTreeSet<V3> = polys.iter().flat_map(|p| p.verts.iter().copied()).collect();
    let mut index: HashMap<[i64; 3], Vec<V3>> = HashMap::new();
    for v in &verts {
        index.entry(cell(wide(v))).or_default().push(*v);
    }
    let sub = |p: [i64; 3], q: [i64; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let dot = |p: [i64; 3], q: [i64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
    let cross = |p: [i64; 3], q: [i64; 3]| [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
    for p in polys.iter_mut() {
        let mut out: Vec<V3> = Vec::new();
        let has_uv = p.uvs.as_ref().is_some_and(|u| !u.is_empty());
        let mut out_uv: Vec<Uv> = Vec::new();
        let m = p.verts.len();
        for i in 0..m {
            let (va, vb) = (p.verts[i], p.verts[(i + 1) % m]);
            let (a, b) = (wide(&va), wide(&vb));
            out.push(va);
            if has_uv {
                out_uv.push(p.uvs.as_ref().expect("has UVs")[i]);
            }
            let ab = sub(b, a);
            let l2 = dot(ab, ab);
            if l2 == 0 {
                continue;
            }
            // int() cuts towards zero, and the // that follows floors
            let lo = cell(from_fn(|k| (a[k].min(b[k]) as f64 - TOL) as i64));
            let hi = cell(from_fn(|k| (a[k].max(b[k]) as f64 + TOL) as i64));
            let mut found: Vec<(f64, V3)> = Vec::new();
            for cx in lo[0]..=hi[0] {
                for cy in lo[1]..=hi[1] {
                    for cz in lo[2]..=hi[2] {
                        for v in index.get(&[cx, cy, cz]).map_or(&[][..], |l| &l[..]) {
                            if *v == va || *v == vb {
                                continue;
                            }
                            let av = sub(wide(v), a);
                            let t = dot(av, ab) as f64 / l2 as f64;
                            if !(0.0 < t && t < 1.0) {
                                continue;
                            }
                            let cr = cross(av, ab);
                            if dot(cr, cr) as f64 <= TOL * TOL * l2 as f64 {
                                found.push((t, *v));
                            }
                        }
                    }
                }
            }
            found.sort_by(|x, y| py::cmp_f64(x.0, y.0).then(x.1.cmp(&y.1)));
            out.extend(found.iter().map(|f| f.1));
            if has_uv {
                let uvs = p.uvs.as_ref().expect("has UVs");
                let (ua, ub) = (uvs[i], uvs[(i + 1) % m]);
                out_uv.extend(found.iter().map(|&(t, _)| (ua.0 + (ub.0 - ua.0) * t, ua.1 + (ub.1 - ua.1) * t)));
            }
        }
        // A vertex a unit from a sharp corner (the foot of a ramp's tapered edge strip) lies
        // within TOL of both edges that meet there. Keep it once: twice is an invalid polygon.
        let mut seen = BTreeSet::new();
        let keep: Vec<usize> = (0..out.len()).filter(|&i| seen.insert(out[i])).collect();
        p.verts = keep.iter().map(|&i| out[i]).collect();
        if has_uv {
            p.uvs = Some(keep.iter().map(|&i| out_uv[i]).collect());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(f: &Face) -> Result<Option<Surface>, String> {
        Ok(Some(Surface::new(if f.kind() == "wall" { "plain" } else { "floor" }, (118.0, 118.0), None)))
    }

    #[test]
    fn a_carved_box_is_six_faces_split_on_the_lattice() {
        let mut c = Carver::default();
        c.carve([0.0, 0.0, 0.0], [236.0, 118.0, 100.0], "room");
        let polys = c.polygons(plain, true).unwrap();
        // floor and ceiling in two, the long walls in two, the short ones whole
        assert_eq!(polys.len(), 2 + 2 + 2 + 2 + 1 + 1);
        for p in &polys {
            assert!(p.carved && p.verts.len() == 4);
            let n = newell(&p.verts);
            assert_eq!(n.map(|x| x + 0.0), p.normal, "{p:?}");
        }
        let floors: Vec<&Poly> = polys.iter().filter(|p| p.normal == [0.0, 0.0, 1.0]).collect();
        assert_eq!(floors.len(), 2);
        assert!(floors.iter().all(|p| p.verts.iter().all(|v| v[2] == 0)));
    }

    #[test]
    fn air_keeps_a_dicts_order_and_zones_join_with_no_wall() {
        let mut air = Air::default();
        for (c, z) in [([0, 0, 0], 0), ([1, 0, 0], 0), ([2, 0, 0], 0)] {
            air.set(c, z);
        }
        air.set([0, 0, 0], 1); // carved again: keeps its place
        air.pop([1, 0, 0]);
        air.set([1, 0, 0], 2); // taken out and put back: goes to the end
        let order: Vec<(Idx, usize)> = air.order.iter().flatten().copied().collect();
        assert_eq!(order, [([0, 0, 0], 1), ([2, 0, 0], 0), ([1, 0, 0], 2)]);

        let mut c = Carver::new([None; 3], 0);
        c.carve([0.0, 0.0, 0.0], [100.0, 100.0, 100.0], "a").carve([100.0, 0.0, 0.0], [200.0, 100.0, 100.0], "b");
        c.solid([40.0, 40.0, 0.0], [60.0, 60.0, 50.0], "pier");
        let mut walls = Vec::new();
        c.faces(|f, _| {
            if f.axis == 0 && f.lo[0] == 100 {
                walls.push(f.zone.to_string());
            }
            Ok(())
        })
        .unwrap();
        assert!(walls.is_empty(), "a wall between the zones: {walls:?}");
        let mut tags = BTreeSet::new();
        c.faces(|f, _| {
            tags.insert(f.tag.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(tags.into_iter().collect::<Vec<_>>(), ["pier", "void"]);
        assert!(c.portal([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).is_err());
    }

    #[test]
    fn a_repeated_vertex_is_dropped_and_the_normal_follows_what_is_wanted() {
        let s = Surface::new("floor", (39.37, 39.37), None);
        let quad = [[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 10.0, 0.0], [10.0, 10.2, 0.0]];
        let p = oriented(&quad, [0.0, 0.0, 1.0], s, "z", Some(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 2.0)]));
        assert_eq!(p.verts, [[10, 10, 0], [0, 10, 0], [0, 0, 0]]);
        assert_eq!(p.uvs.as_deref(), Some(&[(1.0, 1.0), (0.0, 1.0), (0.0, 0.0)][..]));
        assert_eq!(p.normal, [0.0, 0.0, 1.0]);
        // the first vertex is held against the last
        let p = oriented(&[[5.0, 5.0, 0.0], [9.0, 5.0, 0.0], [9.0, 9.0, 0.0], [5.0, 5.4, 0.0]], [0.0, 0.0, 1.0], s, "z", None);
        assert_eq!(p.verts, [[9, 5, 0], [9, 9, 0], [5, 5, 0]]);
    }

    #[test]
    fn a_vertex_on_an_edge_is_put_into_it_once() {
        let s = Surface::new("floor", (39.37, 39.37), None);
        let mut polys = vec![
            oriented(
                &[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 100.0, 0.0], [0.0, 100.0, 0.0]],
                [0.0, 0.0, 1.0],
                s,
                "z",
                Some(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            ),
            oriented(&[[100.0, 25.0, 0.0], [200.0, 25.0, 0.0], [200.0, 75.0, 0.0], [100.0, 75.0, 0.0]], [0.0, 0.0, 1.0], s, "z", None),
        ];
        fix_t_junctions(&mut polys);
        assert_eq!(polys[0].verts, [[0, 0, 0], [100, 0, 0], [100, 25, 0], [100, 75, 0], [100, 100, 0], [0, 100, 0]]);
        assert_eq!(polys[0].uvs.as_deref(), Some(&[(0.0, 0.0), (1.0, 0.0), (1.0, 0.25), (1.0, 0.75), (1.0, 1.0), (0.0, 1.0)][..]));
        assert_eq!(polys[1].verts.len(), 4);
    }

    #[test]
    fn walls_project_along_themselves_and_bands_pin_v() {
        let s = Surface::new("panel", (118.11, 118.11), Some(118.0));
        let mut c = Carver::new([None; 3], 0);
        c.carve([0.0, 0.0, 0.0], [236.0, 118.0, 236.0], "room");
        let polys = c.polygons(|_| Ok(Some(s)), false).unwrap();
        let wall = polys.iter().find(|p| p.normal == [0.0, 1.0, 0.0]).unwrap();
        let uv = project_uvs(wall);
        assert!(uv.iter().all(|t| t.0.is_sign_positive() || t.0 != 0.0), "{uv:?}");
        let vs: Vec<f64> = uv.iter().map(|t| t.1).collect();
        assert!(vs.contains(&(-118.0 / 118.11)) && vs.contains(&(118.0 / 118.11)), "{vs:?}");
    }
}
