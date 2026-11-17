//! The bake's ray caster: a bounding-volume tree over the level's occluding triangles, and rays
//! that ask only whether anything is in the way.
//!
//! It stands in for Blender's `mathutils.bvhtree.BVHTree`, which the Python bake casts against,
//! and it is made to answer as that does: in single precision, with Blender's own tests. A ray
//! is cut short by a triangle when it passes the triangle's box (`fast_ray_nearest_hit`, with
//! the box grown by FLT_EPSILON) and the triangle itself (`isect_ray_tri_watertight_v3`, which
//! counts a hit on an edge and from either side, and none in the triangle's own plane) nearer
//! than the ray is long. That matters where the level's grid puts a ray exactly along a floor or
//! through a pillar's edge: a level has thousands of such rays, and another rule would light
//! those corners differently. Blender's tree and this one group the triangles their own ways,
//! which changes nothing: a box that holds a triangle's box never turns away a ray it lets in.
//!
//! The vectors here are `mathutils`' too (`f32`, with lengths and dot products summed in `f64`
//! from the last component to the first), so a ray starts and points where the Python's does.

pub type F3 = [f32; 3];

pub fn add(a: F3, b: F3) -> F3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: F3, b: F3) -> F3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: F3, s: f32) -> F3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `Vector.dot`: the products are single, their sum double, the last component first.
pub fn dot(a: F3, b: F3) -> f64 {
    let mut d = 0.0f64;
    for i in (0..3).rev() {
        d += (a[i] * b[i]) as f64;
    }
    d
}

/// `Vector.length`.
pub fn length(a: F3) -> f64 {
    dot(a, a).sqrt()
}

/// `Vector / float`: times the single-precision reciprocal.
pub fn over(a: F3, s: f64) -> F3 {
    scale(a, 1.0f32 / s as f32)
}

/// `Vector.normalized()`: the squares are summed in double, the root taken, and the vector
/// multiplied by its single-precision reciprocal. A vector of no length stays zero.
pub fn normalized(a: F3) -> F3 {
    let mut d = 0.0f64;
    for i in (0..3).rev() {
        d += (a[i] as f64) * (a[i] as f64);
    }
    if d > 1.0e-35 {
        scale(a, 1.0f32 / d.sqrt() as f32)
    } else {
        [0.0; 3]
    }
}

/// A point of the level (whole numbers, exact in `f32`) or any other, as a `mathutils` vector.
pub fn f3(p: [f64; 3]) -> F3 {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

#[derive(Clone, Copy)]
struct Tri {
    v: [F3; 3],
    lo: F3,
    hi: F3,
}

#[derive(Clone, Copy)]
struct Node {
    lo: F3,
    hi: F3,
    /// A leaf: its triangles are `tris[at..at + count]`. Otherwise its children are `at` and `at + 1`.
    at: u32,
    count: u32,
}

/// The tree. Empty, nothing is ever in the way.
pub struct Bvh {
    nodes: Vec<Node>,
    tris: Vec<Tri>,
}

const LEAF: usize = 4;

impl Bvh {
    /// A tree over triangles, in any order: the same triangles in the same order make the same tree.
    pub fn new(triangles: &[[F3; 3]]) -> Bvh {
        let mut tris: Vec<Tri> = triangles
            .iter()
            .map(|v| {
                // Blender's leaf box: the triangle's, grown by FLT_EPSILON (lost on any coordinate past 2)
                let lo = std::array::from_fn(|k| v[0][k].min(v[1][k]).min(v[2][k]) - f32::EPSILON);
                let hi = std::array::from_fn(|k| v[0][k].max(v[1][k]).max(v[2][k]) + f32::EPSILON);
                Tri { v: *v, lo, hi }
            })
            .collect();
        let mut nodes = Vec::with_capacity(tris.len());
        if !tris.is_empty() {
            nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], at: 0, count: 0 });
            let n = tris.len();
            split(&mut nodes, &mut tris, 0, 0, n);
        }
        Bvh { nodes, tris }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// `BVHTree.ray_cast(origin, direction, distance)[0] is not None`: is a triangle met within
    /// `max` of `origin`, going along `direction` (any length)?
    pub fn hit(&self, origin: F3, direction: F3, max: f32) -> bool {
        if self.is_empty() {
            return false;
        }
        let ray = Ray::new(origin, direction);
        let mut stack = [0u32; 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let node = &self.nodes[stack[top] as usize];
            if !ray.enters(&node.lo, &node.hi, max) {
                continue;
            }
            if node.count > 0 {
                for t in &self.tris[node.at as usize..(node.at + node.count) as usize] {
                    if ray.enters(&t.lo, &t.hi, max) {
                        let d = ray.triangle(&t.v);
                        if d >= 0.0 && d < max {
                            return true;
                        }
                    }
                }
            } else {
                stack[top] = node.at;
                stack[top + 1] = node.at + 1;
                top += 2;
            }
        }
        false
    }
}

/// Make `nodes[at]` the node over `tris[from..to]`, and its children after it.
fn split(nodes: &mut Vec<Node>, tris: &mut [Tri], at: usize, from: usize, to: usize) {
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    let (mut clo, mut chi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for t in &tris[from..to] {
        for k in 0..3 {
            lo[k] = lo[k].min(t.lo[k]);
            hi[k] = hi[k].max(t.hi[k]);
            let c = t.lo[k] + t.hi[k];
            clo[k] = clo[k].min(c);
            chi[k] = chi[k].max(c);
        }
    }
    if to - from <= LEAF {
        nodes[at] = Node { lo, hi, at: from as u32, count: (to - from) as u32 };
        return;
    }
    // halve along the axis the triangles' middles are spread furthest on (the first of equals)
    let mut axis = 0;
    for k in 1..3 {
        if chi[k] - clo[k] > chi[axis] - clo[axis] {
            axis = k;
        }
    }
    tris[from..to].sort_by(|a, b| (a.lo[axis] + a.hi[axis]).total_cmp(&(b.lo[axis] + b.hi[axis])));
    let mid = from + (to - from) / 2;
    let kids = nodes.len();
    nodes.push(Node { lo, hi, at: 0, count: 0 });
    nodes.push(Node { lo, hi, at: 0, count: 0 });
    nodes[at] = Node { lo, hi, at: kids as u32, count: 0 };
    split(nodes, tris, kids, from, mid);
    split(nodes, tris, kids + 1, mid, to);
}

/// A ray as Blender's tree holds one: `bvhtree_ray_cast_data_precalc` and
/// `isect_ray_tri_watertight_v3_precalc`.
struct Ray {
    origin: F3,
    /// One over the direction, each way; FLT_MAX where the ray does not move that way.
    inverse: F3,
    /// The axis the ray moves along most, and the two others, in the order that keeps a triangle's winding.
    k: [usize; 3],
    /// The shear that lays the ray along that axis.
    s: F3,
}

impl Ray {
    fn new(origin: F3, direction: F3) -> Ray {
        // `normalize_v3`, all in single precision
        let d = direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2];
        let dir = if d > 1.0e-35 { scale(direction, 1.0 / d.sqrt()) } else { [0.0; 3] };
        let inverse = dir.map(|c| if c.abs() < f32::EPSILON { f32::MAX } else { 1.0 / c });
        let (x, y, z) = (dir[0].abs(), dir[1].abs(), dir[2].abs());
        let kz = if x > y {
            if x > z {
                0
            } else {
                2
            }
        } else if y > z {
            1
        } else {
            2
        };
        let mut kx = if kz != 2 { kz + 1 } else { 0 };
        let mut ky = if kx != 2 { kx + 1 } else { 0 };
        if dir[kz] < 0.0 {
            std::mem::swap(&mut kx, &mut ky);
        }
        let inv = 1.0 / dir[kz];
        Ray { origin, inverse, k: [kx, ky, kz], s: [dir[kx] * inv, dir[ky] * inv, inv] }
    }

    /// `fast_ray_nearest_hit`, and the test `dfs_raycast` makes of its answer: can the ray meet
    /// something in this box nearer than `max`?
    fn enters(&self, lo: &F3, hi: &F3, max: f32) -> bool {
        let mut t1 = [0.0f32; 3];
        let mut t2 = [0.0f32; 3];
        for i in 0..3 {
            let (near, far) = if self.inverse[i] < 0.0 { (hi[i], lo[i]) } else { (lo[i], hi[i]) };
            t1[i] = (near - self.origin[i]) * self.inverse[i];
            t2[i] = (far - self.origin[i]) * self.inverse[i];
        }
        let out = (t1[0] > t2[1] || t2[0] < t1[1] || t1[0] > t2[2] || t2[0] < t1[2] || t1[1] > t2[2] || t2[1] < t1[2])
            || (t2[0] < 0.0 || t2[1] < 0.0 || t2[2] < 0.0)
            || (t1[0] > max || t1[1] > max || t1[2] > max);
        // max_fff, as C's: the first unless a later one is greater
        let m = if t1[0] < t1[1] { t1[1] } else { t1[0] };
        let m = if m < t1[2] { t1[2] } else { m };
        // a miss is FLT_MAX, which no ray is longer than
        !out && m < max
    }

    /// `isect_ray_tri_watertight_v3`: how far along the ray the triangle is, or FLT_MAX if the
    /// ray's line misses it or it lies behind.
    fn triangle(&self, v: &[F3; 3]) -> f32 {
        let [kx, ky, kz] = self.k;
        let [sx, sy, sz] = self.s;
        let a = sub(v[0], self.origin);
        let b = sub(v[1], self.origin);
        let c = sub(v[2], self.origin);
        let (ax, ay) = (a[kx] - sx * a[kz], a[ky] - sy * a[kz]);
        let (bx, by) = (b[kx] - sx * b[kz], b[ky] - sy * b[kz]);
        let (cx, cy) = (c[kx] - sx * c[kz], c[ky] - sy * c[kz]);
        let u = cx * by - cy * bx;
        let v = ax * cy - ay * cx;
        let w = bx * ay - by * ax;
        if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
            return f32::MAX;
        }
        let det = u + v + w;
        if det == 0.0 || !det.is_finite() {
            return f32::MAX;
        }
        let t = (u * a[kz] + v * b[kz] + w * c[kz]) * sz;
        // t with the determinant's sign: behind the ray if that is negative
        let signed = if det.is_sign_negative() { -t } else { t };
        if signed < 0.0 {
            return f32::MAX;
        }
        t * (1.0 / det)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slab(z: f32) -> Vec<[F3; 3]> {
        vec![[[0.0, 0.0, z], [100.0, 0.0, z], [100.0, 100.0, z]], [[0.0, 0.0, z], [100.0, 100.0, z], [0.0, 100.0, z]]]
    }

    #[test]
    fn a_ray_is_stopped_from_either_side_and_only_within_its_length() {
        let t = Bvh::new(&slab(50.0));
        assert!(t.hit([30.0, 60.0, 0.0], [0.0, 0.0, 1.0], 80.0));
        assert!(t.hit([30.0, 60.0, 90.0], [0.0, 0.0, -3.0], 80.0), "from above, and the direction need not be a unit");
        assert!(!t.hit([30.0, 60.0, 0.0], [0.0, 0.0, 1.0], 49.0), "too short to reach");
        assert!(!t.hit([30.0, 60.0, 0.0], [0.0, 0.0, 1.0], 50.0), "a hit at the ray's very end is none");
        assert!(!t.hit([30.0, 60.0, 60.0], [0.0, 0.0, 1.0], 80.0), "behind it");
        assert!(!t.hit([130.0, 60.0, 0.0], [0.0, 0.0, 1.0], 80.0), "beside it");
        assert!(t.hit([50.0, 50.0, 0.0], [0.0, 0.0, 1.0], 80.0), "on the diagonal the two triangles share");
        assert!(!Bvh::new(&[]).hit([0.0; 3], [0.0, 0.0, 1.0], 1e9));
    }

    #[test]
    fn a_ray_along_a_floor_meets_the_wall_that_stands_on_it_and_not_the_floor() {
        // an upright rectangle at x = 50, from y = 0 to 100 and z0 to z1
        let wall = |z0: f32, z1: f32| [[[50.0, 0.0, z0], [50.0, 100.0, z0], [50.0, 100.0, z1]], [[50.0, 0.0, z0], [50.0, 100.0, z1], [50.0, 0.0, z1]]];
        // a floor at 100 and a wall standing on it
        let mut tris = slab(100.0);
        tris.extend(wall(100.0, 180.0));
        let t = Bvh::new(&tris);
        assert!(t.hit([10.0, 40.0, 100.0], [1.0, 0.0, 0.0], 80.0), "the wall's foot is in the way");
        assert!(!t.hit([10.0, 40.0, 100.0], [1.0, 0.0, 0.0], 39.0), "the floor the ray lies in is not");
        assert!(!t.hit([10.0, 140.0, 100.0], [1.0, 0.0, 0.0], 80.0), "past the wall's end");
        // Blender's answers, asked of its own BVHTree: a wall that hangs down to the ray's height is
        // passed under (its box turns the ray away), except at height 0, where FLT_EPSILON grows the box
        assert!(!Bvh::new(&wall(20.0, 100.0)).hit([10.0, 40.0, 100.0], [1.0, 0.0, 0.0], 80.0));
        assert!(Bvh::new(&wall(0.0, 80.0)).hit([10.0, 40.0, 0.0], [1.0, 0.0, 0.0], 80.0));
        assert!(Bvh::new(&wall(-80.0, 0.0)).hit([10.0, 40.0, 0.0], [1.0, 0.0, 0.0], 80.0));
    }

    #[test]
    fn many_triangles_answer_as_each_would_alone() {
        // a field of small tilted triangles, and rays through it: the tree agrees with trying every one
        let mut tris = Vec::new();
        let mut s = 12345u32;
        let mut next = |span: f32| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 8) as f32 / (1u32 << 24) as f32 * span
        };
        for _ in 0..400 {
            let p = [next(1000.0), next(1000.0), next(400.0)];
            tris.push([p, [p[0] + next(60.0), p[1] + next(60.0), p[2] + next(60.0)], [p[0] + next(60.0), p[1] - next(60.0), p[2] + next(60.0)]]);
        }
        let t = Bvh::new(&tris);
        assert_eq!(t.len(), 400);
        let (mut hits, mut tried) = (0, 0);
        for _ in 0..2000 {
            let o = [next(1000.0), next(1000.0), next(400.0)];
            let d = [next(2.0) - 1.0, next(2.0) - 1.0, next(2.0) - 1.0];
            let max = next(900.0);
            let ray = Ray::new(o, d);
            let slow = tris.iter().any(|v| {
                let x = ray.triangle(v);
                x >= 0.0 && x < max
            });
            // the boxes can only turn away what the triangle test would too, bar rays that graze a box's face
            let fast = t.hit(o, d, max);
            assert!(!fast || slow, "the tree found a hit no triangle gives");
            tried += slow as usize;
            hits += fast as usize;
        }
        assert!(tried > 100 && tried - hits <= 2, "{hits} of {tried}");
    }
}
