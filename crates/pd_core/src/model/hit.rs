//! `model_test_for_hit` (`model.c:3785`) and `model_test_bbox_node_for_hit`
//! (`model.c:3593`): a ray against a posed model's BBOX part boxes.
//!
//! The walk is the node tree's depth-first order from the root's first child
//! (the root itself is never tested). The FIRST box the ray passes through wins,
//! not the nearest; a missed box's children are skipped; a hidden toggle or
//! distance hides its subtree; a head's boxes are tested in place of the body's
//! headspot, in the head joint's space. The result carries a cursor:
//! `chr_test_hit` (`chr.c:4546`) calls again from the hit box to try the next
//! one, which continues into that box's children.
//!
//! Ray position and direction are in the space the model was posed in (PD
//! passes `gunpos2d`/`gundir2d`, eye space).
//!
//! Source: the old repo's `pd_guns/range.rs` `test_part_boxes`, which tested the
//! boxes of an exported hitbox list; this walks the model itself.

use glam::{Mat4, Vec3};

use super::pose::Model;
use super::NodeKind;

/// Where the walk stopped: a body node or a node of the attached head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitNode {
    Body(usize),
    Head(usize),
}

/// `var8005efc0`: how far every box is grown before the test (`chr.c:4534`
/// grows a shielded chr's by `10 / model->scale`).
#[derive(Clone, Copy, Debug, Default)]
pub struct HitPad(pub f32);

/// `model_test_bbox_node_for_hit`: does the ray `pos + t·dir` (t unbounded
/// forward) pass through `bbox` (`[xmin, xmax, ymin, ymax, zmin, zmax]`) in the
/// space of `mtx`? A literal port: slab parameters are kept as
/// numerator/denominator pairs and compared by cross-multiplying.
pub fn bbox_hit(bbox: &[f32; 6], mtx: &Mat4, pos: Vec3, dir: Vec3, pad: HitPad) -> bool {
    let [mut xmin, mut xmax, mut ymin, mut ymax, mut zmin, mut zmax] = *bbox;
    if pad.0 != 0.0 {
        xmin -= pad.0;
        xmax += pad.0;
        ymin -= pad.0;
        ymax += pad.0;
        zmin -= pad.0;
        zmax += pad.0;
    }
    let t = mtx.w_axis.truncate();
    // One axis: (|c|², c·dir, c·(pos − T)) → (sum1, sum2, sum3) sign-normalised
    // and ordered, or None when the slab is entirely behind the ray.
    let slab = |c: Vec3, lo: f32, hi: f32| -> Option<(f32, f32, f32)> {
        let thing = c.x * c.x + c.y * c.y + c.z * c.z;
        let mut sum1 = c.x * dir.x + c.y * dir.y + c.z * dir.z;
        let d = c.x * (pos.x - t.x) + c.y * (pos.y - t.y) + c.z * (pos.z - t.z);
        let f0 = -thing * hi;
        let mut sum3 = -(d + f0);
        let f0 = -thing * lo;
        let mut sum2 = -(d + f0);
        if sum1 < 0.0 {
            sum1 = -sum1;
            sum2 = -sum2;
            sum3 = -sum3;
        }
        if sum2 < 0.0 && sum3 < 0.0 {
            return None;
        }
        if sum3 < sum2 {
            std::mem::swap(&mut sum2, &mut sum3);
        }
        Some((sum1, sum2, sum3))
    };
    let Some((xsum1, xsum2, xsum3)) = slab(mtx.x_axis.truncate(), xmin, xmax) else { return false };
    let Some((ysum1, ysum2, ysum3)) = slab(mtx.y_axis.truncate(), ymin, ymax) else { return false };
    let mult1 = ysum2 * xsum1;
    let mult2 = xsum2 * ysum1;
    let mult3 = xsum3 * ysum1;
    let mult4 = ysum3 * xsum1;
    let (bestsum2, bestsum1) = if mult1 < mult2 {
        if mult4 < mult2 {
            return false;
        }
        (xsum2, xsum1)
    } else {
        if mult3 < mult1 {
            return false;
        }
        (ysum2, ysum1)
    };
    let (anotherbestsum3, anotherbestsum1) = if mult3 < mult4 { (xsum3, xsum1) } else { (ysum3, ysum1) };
    let Some((zsum1, zsum2, zsum3)) = slab(mtx.z_axis.truncate(), zmin, zmax) else { return false };
    if bestsum2 * zsum1 < zsum2 * bestsum1 {
        if anotherbestsum3 * zsum1 < zsum2 * anotherbestsum1 {
            return false;
        }
    } else if zsum3 * bestsum1 < bestsum2 * zsum1 {
        return false;
    }
    true
}

impl Model {
    /// `model_test_for_hit`: the first part box the ray passes through after
    /// `from` (`None`: from the start), as `(HITPART_*, where)`.
    pub fn test_for_hit(&self, pos: Vec3, dir: Vec3, from: Option<HitNode>, pad: HitPad) -> Option<(i32, HitNode)> {
        match from {
            None => self.hit_walk(false, 1, pos, dir, pad),
            Some(HitNode::Body(i)) => self.hit_walk(false, i + 1, pos, dir, pad),
            Some(HitNode::Head(i)) => {
                if let Some(r) = self.hit_walk(true, i + 1, pos, dir, pad) {
                    return Some(r);
                }
                // The head is the headspot's subtree; the body walk resumes after it.
                let spot = self.headspot()?;
                self.hit_walk(false, spot + 1, pos, dir, pad)
            }
        }
    }

    fn hit_walk(&self, head: bool, from: usize, pos: Vec3, dir: Vec3, pad: HitPad) -> Option<(i32, HitNode)> {
        let (def, vis) = if head {
            (self.head.as_deref()?, &self.head_vis)
        } else {
            (&*self.def, &self.vis)
        };
        let mut i = from;
        while i < def.nodes.len() {
            let n = &def.nodes[i];
            match &n.kind {
                NodeKind::BBox { hitpart, bbox } => {
                    let mtx = if head {
                        def.find_node_mtx_index(i, 0).map(|m| self.matrices[m]).or_else(|| self.head_matrix())
                    } else {
                        def.find_node_mtx_index(i, 0).map(|m| self.matrices[m])
                    };
                    if mtx.is_some_and(|m| bbox_hit(bbox, &m, pos, dir, pad)) {
                        return Some((*hitpart, if head { HitNode::Head(i) } else { HitNode::Body(i) }));
                    }
                    i = n.subtree_end;
                    continue;
                }
                NodeKind::Toggle | NodeKind::Distance { .. } if !vis[i] => {
                    i = n.subtree_end;
                    continue;
                }
                NodeKind::HeadSpot if !head && self.head.is_some() => {
                    if let Some(r) = self.hit_walk(true, 0, pos, dir, pad) {
                        return Some(r);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }
}
