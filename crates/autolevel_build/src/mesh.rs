//! Room meshes: the level's polygons gathered into one mesh a room, as `build.emit` does in
//! Blender, and cut into triangles for the writers.
//!
//! The rules are `emit`'s: a polygon goes to the room its zone names, one that rounding
//! collapsed (fewer than three different vertices) is left out, a room's vertices are shared
//! by position in the order they are first met, and its faces keep the polygons' order. Each
//! face carries its UVs (`csg::project_uvs`), its material (a place in its kit's palette), the
//! beam that lights it and a colour for each corner: the baked lighting, white until `bake`
//! has run.
//!
//! Blender cut the faces into triangles on export, its own way. Here `triangulate` clips ears,
//! shortest diagonal first, and never takes one whose diagonal runs through another vertex: the
//! polygons are convex but carry vertices along their edges (the T-junction repair puts them
//! there), and a careless fan would make triangles of no area. The count is the same either way,
//! two fewer than the corners.

use autolevel::level::Level;

use crate::csg::{self, Poly, Uv, P3, V3};
use crate::kit::Kit;

/// One polygon of a room.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Its corners, as places in the room's `verts`, counter-clockwise from the side it faces.
    pub verts: Vec<u32>,
    /// A UV for each corner (v up, as OBJ has it).
    pub uvs: Vec<Uv>,
    pub normal: P3,
    /// Its place in the kit's palette.
    pub material: usize,
    /// The beam that lights it (a place in the level's `Lit` list).
    pub lit: Option<usize>,
    /// The baked lighting, raw 0-255, for each corner. White until the bake.
    pub colors: Vec<[u8; 3]>,
    /// Its triangles, as places in its own corner lists.
    pub tris: Vec<[u32; 3]>,
}

/// One room: an object `Room_NN` in the Python's Blender scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    /// The plan's key for it.
    pub key: String,
    /// `Room_NN`, in hex.
    pub name: String,
    pub number: usize,
    pub verts: Vec<V3>,
    pub faces: Vec<Face>,
}

impl Room {
    pub fn triangles(&self) -> usize {
        self.faces.iter().map(|f| f.tris.len()).sum()
    }

    /// A face's corners as points.
    pub fn points(&self, f: &Face) -> Vec<V3> {
        f.verts.iter().map(|&i| self.verts[i as usize]).collect()
    }
}

/// The level's polygons as room meshes, in the order the rooms are first met (the writers put
/// them in order of their numbers), each face's material its role's place in `kit`'s palette. An
/// error if a polygon's zone is no room of the plan, or its role none of the kit's.
pub fn emit(lv: &Level, polys: &[Poly], kit: &Kit) -> Result<Vec<Room>, String> {
    let mut rooms: Vec<Room> = Vec::new();
    let mut index: Vec<std::collections::HashMap<V3, u32>> = Vec::new();
    let mut place: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for p in polys {
        if distinct(&p.verts) < 3 {
            continue;
        }
        let at = match place.get(p.zone.as_str()) {
            Some(&at) => at,
            None => {
                let r = lv.plan.get(&p.zone).ok_or_else(|| format!("a polygon in {:?}, which is no room of the plan", p.zone))?;
                rooms.push(Room { key: r.key.clone(), name: r.object_name(), number: r.room_id, verts: Vec::new(), faces: Vec::new() });
                index.push(std::collections::HashMap::new());
                place.insert(p.zone.as_str(), rooms.len() - 1);
                rooms.len() - 1
            }
        };
        let (room, seen) = (&mut rooms[at], &mut index[at]);
        let mut verts = Vec::with_capacity(p.verts.len());
        for v in &p.verts {
            let n = room.verts.len() as u32;
            let i = *seen.entry(*v).or_insert(n);
            if i == n {
                room.verts.push(*v);
            }
            verts.push(i);
        }
        let material = kit.index(p.surface.material).map_err(|_| format!("no material {:?} in the palette", p.surface.material))?;
        let tris = triangulate(&p.verts);
        room.faces.push(Face { colors: vec![[255; 3]; verts.len()], verts, uvs: csg::project_uvs(p), normal: p.normal, material, lit: p.lit, tris });
    }
    Ok(rooms)
}

fn distinct(verts: &[V3]) -> usize {
    let mut seen: Vec<V3> = Vec::with_capacity(verts.len());
    for v in verts {
        if !seen.contains(v) {
            seen.push(*v);
        }
    }
    seen.len()
}

type W3 = [i128; 3];

fn sub(a: V3, b: V3) -> W3 {
    [(a[0] - b[0]) as i128, (a[1] - b[1]) as i128, (a[2] - b[2]) as i128]
}

fn cross(a: W3, b: W3) -> W3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: W3, b: W3) -> i128 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A flat polygon as triangles, each three places in `verts`, wound as the polygon is.
/// Ears are clipped, the one with the shortest diagonal first (the first of equals), and an ear
/// is only one if no other corner still to do lies in or on it: so a corner along an edge never
/// ends up on a triangle's long side, where it would leave a triangle of no area behind. All in
/// whole numbers, so the result is the same on any machine.
pub fn triangulate(verts: &[V3]) -> Vec<[u32; 3]> {
    let n = verts.len();
    if n < 3 {
        return Vec::new();
    }
    // the polygon's normal, twice its area long (Newell, as a sum of crosses about the first corner)
    let mut nrm: W3 = [0; 3];
    for i in 1..n - 1 {
        let c = cross(sub(verts[i], verts[0]), sub(verts[i + 1], verts[0]));
        nrm = [nrm[0] + c[0], nrm[1] + c[1], nrm[2] + c[2]];
    }
    let mut ring: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    while ring.len() > 3 {
        let m = ring.len();
        let area = |k: usize| {
            let (p, i, q) = (verts[ring[(k + m - 1) % m]], verts[ring[k]], verts[ring[(k + 1) % m]]);
            dot(cross(sub(i, p), sub(q, i)), nrm)
        };
        let ear = |k: usize| {
            if area(k) <= 0 {
                return false;
            }
            let (p, i, q) = (verts[ring[(k + m - 1) % m]], verts[ring[k]], verts[ring[(k + 1) % m]]);
            (0..m).filter(|&j| j != k && j != (k + m - 1) % m && j != (k + 1) % m).all(|j| {
                let r = verts[ring[j]];
                // outside if it is strictly beyond any of the triangle's three sides
                dot(cross(sub(i, p), sub(r, p)), nrm) < 0 || dot(cross(sub(q, i), sub(r, i)), nrm) < 0 || dot(cross(sub(p, q), sub(r, q)), nrm) < 0
            })
        };
        let diagonal = |k: usize| {
            let d = sub(verts[ring[(k + 1) % m]], verts[ring[(k + m - 1) % m]]);
            dot(d, d)
        };
        let mut best: Option<usize> = None;
        for k in 0..m {
            if ear(k) && best.is_none_or(|b| diagonal(k) < diagonal(b)) {
                best = Some(k);
            }
        }
        // no ear: a polygon of no area, or one that crosses itself. Take the widest corner and go on.
        let k = best.unwrap_or_else(|| (0..m).fold(0, |b, k| if area(k) > area(b) { k } else { b }));
        out.push([ring[(k + m - 1) % m] as u32, ring[k] as u32, ring[(k + 1) % m] as u32]);
        ring.remove(k);
    }
    out.push([ring[0] as u32, ring[1] as u32, ring[2] as u32]);
    out
}

/// Twice a triangle's area, squared (0: its corners are in a line).
pub fn area2(a: V3, b: V3, c: V3) -> i128 {
    let x = cross(sub(b, a), sub(c, a));
    dot(x, x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry;
    use autolevel::level::{generate, Options};

    #[test]
    fn a_quad_is_two_triangles_on_its_shorter_diagonal() {
        let q = [[0, 0, 0], [10, 0, 0], [12, 4, 0], [0, 4, 0]];
        let t = triangulate(&q);
        assert_eq!(t.len(), 2);
        // diagonals: 0-2 is 12, 4 (160); 1-3 is 10, 4 (116): the ear at 0 or 2 is clipped, across 1-3
        assert!(t.iter().all(|t| t.contains(&1) && t.contains(&3)), "{t:?}");
    }

    #[test]
    fn corners_along_an_edge_leave_no_flat_triangle() {
        // a triangle with a corner along one side, and a rectangle with three along its top
        let shapes: [&[V3]; 3] = [
            &[[0, 0, 0], [8, 0, 0], [0, 8, 0], [0, 4, 0]],
            &[[0, 0, 0], [8, 0, 0], [8, 0, 6], [6, 0, 6], [4, 0, 6], [2, 0, 6], [0, 0, 6]],
            &[[0, 0, 0], [4, 0, 0], [8, 0, 0], [8, 8, 0], [4, 8, 0], [0, 8, 0], [0, 4, 0]],
        ];
        for s in shapes {
            let t = triangulate(s);
            assert_eq!(t.len(), s.len() - 2);
            let total: i128 = t.iter().map(|t| (area2(s[t[0] as usize], s[t[1] as usize], s[t[2] as usize]) as f64).sqrt() as i128).sum();
            let whole: i128 = (1..s.len() - 1).map(|i| (area2(s[0], s[i], s[i + 1]) as f64).sqrt() as i128).sum();
            assert_eq!(total, whole, "the triangles cover the polygon once: {t:?}");
            assert!(t.iter().all(|t| area2(s[t[0] as usize], s[t[1] as usize], s[t[2] as usize]) > 0), "a flat triangle in {t:?}");
        }
    }

    #[test]
    fn a_level_becomes_its_rooms() {
        let lv = generate(40, &Options::default()).unwrap();
        let (polys, _) = geometry::polys(&lv, &crate::complex::KIT).unwrap();
        let rooms = emit(&lv, &polys, &crate::complex::KIT).unwrap();
        // as the Blender build of seed 40 reports: 61 rooms, 15,303 triangles
        assert_eq!(rooms.len(), 61);
        assert_eq!(rooms.iter().map(Room::triangles).sum::<usize>(), 15303);
        let mut flat = 0;
        for r in &rooms {
            assert_eq!(r.name, format!("Room_{:02X}", r.number));
            for f in &r.faces {
                assert_eq!(f.verts.len(), f.uvs.len());
                assert_eq!(f.verts.len(), f.colors.len());
                assert_eq!(f.tris.len(), f.verts.len() - 2);
                let p = r.points(f);
                flat += f.tris.iter().filter(|t| area2(p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]) == 0).count();
            }
        }
        assert_eq!(flat, 0, "triangles of no area");
    }
}
