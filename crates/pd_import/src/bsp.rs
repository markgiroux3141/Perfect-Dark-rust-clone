//! A translucent layer's block tree, as PD's arenas carry one (`bg.json`
//! node `tree`, `pd_bg.bsp_tree`): PD's translucent BG doesn't write depth
//! (`G_RM_AA_ZB_XLU_SURF`), so a room's railings, grilles and glass are drawn
//! back to front by walking the tree, each parent's two children in the order
//! of the camera's side of its plane (`bg_render_room_pass`, `bg.c:3190`).
//! A layer written as one block draws its triangles in the order they were
//! made, and a railing behind lands on the one in front of it.
//!
//! The tree is made the classic way: a node's plane is one of its triangles'
//! (the one that cuts the fewest of the others), the triangles across it are
//! cut in two, and a leaf is a set of triangles on one plane (which cannot be
//! in front of each other).

use glam::Vec3;
use serde_json::{json, Value};

use crate::rooms::clip_plane;
use crate::source::Vert;

/// A vertex this close to a plane is on it (cm).
const EPS: f32 = 0.5;
/// At most this many of a node's planes are weighed as its cut.
const CANDIDATES: usize = 32;

/// A triangle and what it carries (its material).
pub type Item<T> = (T, [Vert; 3]);

/// The tree's root (`{"leaf": i}` or `{"plane": [point, normal], "a": .., "b":
/// ..}`, `a` the side behind the normal, drawn first from in front of it) and
/// its leaves, numbered as the tree numbers them.
pub fn build<T: Copy>(items: Vec<Item<T>>) -> (Value, Vec<Vec<Item<T>>>) {
    let mut leaves = Vec::new();
    let root = node(items, &mut leaves);
    (root, leaves)
}

/// A triangle's plane: its unit normal and `n · p` on it; `None` with no area.
fn plane_of(t: &[Vert; 3]) -> Option<(Vec3, f32)> {
    let n = (t[1].pos - t[0].pos).cross(t[2].pos - t[0].pos).normalize_or_zero();
    (n != Vec3::ZERO).then(|| (n, n.dot(t[0].pos)))
}

/// Where a triangle is against a plane.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    On,
    Behind,
    Front,
    Across,
}

fn side(t: &[Vert; 3], n: Vec3, d: f32) -> Side {
    let (mut behind, mut front) = (false, false);
    for v in t {
        let s = n.dot(v.pos) - d;
        behind |= s < -EPS;
        front |= s > EPS;
    }
    match (behind, front) {
        (false, false) => Side::On,
        (true, false) => Side::Behind,
        (false, true) => Side::Front,
        (true, true) => Side::Across,
    }
}

/// The part of `t` with `n · p <= d`, as triangles.
fn clip<T: Copy>(tag: T, t: &[Vert; 3], n: Vec3, d: f32, out: &mut Vec<Item<T>>) {
    let poly = clip_plane(t, n, d);
    for k in 1..poly.len().saturating_sub(1) {
        out.push((tag, [poly[0], poly[k], poly[k + 1]]));
    }
}

fn node<T: Copy>(items: Vec<Item<T>>, leaves: &mut Vec<Vec<Item<T>>>) -> Value {
    // The distinct planes, in the order the triangles bring them.
    let mut planes: Vec<(Vec3, f32)> = Vec::new();
    for (_, t) in &items {
        if let Some((n, d)) = plane_of(t) {
            if !planes.iter().any(|&(pn, pd)| side(t, pn, pd) == Side::On && pn.dot(n).abs() > 0.999) {
                planes.push((n, d));
            }
        }
    }
    if planes.len() <= 1 {
        leaves.push(items);
        return json!({ "leaf": leaves.len() - 1 });
    }
    // The cut: the fewest triangles cut, then the most even split.
    let step = planes.len().div_ceil(CANDIDATES);
    let (n, d) = planes
        .iter()
        .step_by(step)
        .copied()
        .min_by_key(|&(n, d)| {
            let (mut across, mut behind, mut front) = (0i64, 0i64, 0i64);
            for (_, t) in &items {
                match side(t, n, d) {
                    Side::Across => across += 1,
                    Side::Behind => behind += 1,
                    Side::Front => front += 1,
                    Side::On => {}
                }
            }
            across * 8 + (behind - front).abs()
        })
        .expect("two planes or more");
    let (mut on, mut behind, mut front) = (Vec::new(), Vec::new(), Vec::new());
    for (tag, t) in items {
        match side(&t, n, d) {
            Side::On => on.push((tag, t)),
            Side::Behind => behind.push((tag, t)),
            Side::Front => front.push((tag, t)),
            Side::Across => {
                clip(tag, &t, n, d, &mut behind);
                clip(tag, &t, -n, -d, &mut front);
            }
        }
    }
    // The plane's own triangles may be drawn with either side (nothing is
    // between them and the plane): in front, unless nothing is behind, so
    // that both sides hold some and the tree ends.
    if behind.is_empty() {
        behind = on;
    } else {
        front.extend(on);
    }
    // A cut that parts nothing (every triangle on it within `EPS`, as a sliver
    // is on planes it does not lie in) would be made again below it for ever:
    // the triangles are as good as one plane, a leaf.
    if behind.is_empty() || front.is_empty() {
        behind.extend(front);
        leaves.push(behind);
        return json!({ "leaf": leaves.len() - 1 });
    }
    let p = n * d;
    let a = node(behind, leaves);
    let b = node(front, leaves);
    json!({ "plane": [p.x, p.y, p.z, n.x, n.y, n.z], "a": a, "b": b })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    fn quad(x: f32) -> Vec<Item<usize>> {
        // An upright panel across z at `x`, both triangles.
        let v = |y: f32, z: f32| Vert { pos: Vec3::new(x, y, z), uv: Vec2::ZERO, col: [255; 4] };
        vec![(0, [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0)]), (0, [v(0.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)])]
    }

    /// The leaves in the order `pd_render::bg` walks them (`bg.c:3190`).
    fn order(tree: &Value, cam: Vec3, out: &mut Vec<usize>) {
        if let Some(l) = tree["leaf"].as_u64() {
            out.push(l as usize);
            return;
        }
        let f: Vec<f32> = tree["plane"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
        let (p, n) = (Vec3::new(f[0], f[1], f[2]), Vec3::new(f[3], f[4], f[5]));
        let (first, second) = if n.dot(p - cam) < 0.0 { ("a", "b") } else { ("b", "a") };
        order(&tree[first], cam, out);
        order(&tree[second], cam, out);
    }

    #[test]
    fn panels_are_drawn_far_to_near_from_either_end() {
        let items: Vec<_> = [300.0, 0.0, 200.0, 100.0].into_iter().flat_map(quad).collect();
        let (tree, leaves) = build(items);
        assert_eq!(leaves.len(), 4);
        assert!(leaves.iter().all(|l| l.len() == 2), "a panel's two triangles share a leaf, uncut");
        let x = |leaf: usize| leaves[leaf][0].1[0].pos.x;
        for cam in [Vec3::new(-500.0, 50.0, 50.0), Vec3::new(800.0, 50.0, 50.0)] {
            let mut o = Vec::new();
            order(&tree, cam, &mut o);
            let d: Vec<f32> = o.iter().map(|&l| (x(l) - cam.x).abs()).collect();
            assert!(d.windows(2).all(|w| w[0] > w[1]), "from {cam}: {d:?} is not far to near");
        }
    }

    #[test]
    fn a_panel_across_another_is_cut() {
        // One panel at x = 50 across z, one at z = 50 across x: each crosses the other.
        let v = |x: f32, y: f32, z: f32| Vert { pos: Vec3::new(x, y, z), uv: Vec2::ZERO, col: [255; 4] };
        let mut items = quad(50.0);
        items.push((1, [v(0.0, 0.0, 50.0), v(100.0, 100.0, 50.0), v(100.0, 0.0, 50.0)]));
        items.push((1, [v(0.0, 0.0, 50.0), v(0.0, 100.0, 50.0), v(100.0, 100.0, 50.0)]));
        let (_, leaves) = build(items);
        assert_eq!(leaves.len(), 3, "the cut panel's halves on either side, the cutting one in its own");
        let area = |t: &[Vert; 3]| (t[1].pos - t[0].pos).cross(t[2].pos - t[0].pos).length() * 0.5;
        let total: f32 = leaves.iter().flatten().map(|(_, t)| area(t)).sum();
        assert!((total - 20000.0).abs() < 1.0, "nothing lost or doubled: {total}");
    }
}
