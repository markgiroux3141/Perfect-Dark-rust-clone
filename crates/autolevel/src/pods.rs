//! Where a pod opens into its hall. From `autolevel/furnish.py` (`Access`, `pod_access`): it
//! needs no doors, so the level can leave out pods that have no way in before anything is
//! routed. The rest of a pod's inside comes with the furnisher.

use crate::cells::{rect_cells, Cells, Rect};
use crate::space::{shell_rect, Space, DIRS, SIDES};

/// Where a pod opens into its hall.
#[derive(Clone, Debug, PartialEq)]
pub struct Access {
    /// Which of the pod's walls the door is in.
    pub side: char,
    /// The hole through the shell, half cells.
    pub door: Rect,
    /// Pad and gangway in the hall: half cells on the pod's storey.
    pub deck: Cells,
    /// The part of it just outside the door.
    pub pad: Cells,
    /// Gangway length, half cells.
    pub length: i32,
    /// The pod stands clear of the hall's walls.
    pub island: bool,
}

/// The shortest way from a pod's door to one of the hall's walls: a pad outside the door (on
/// the wall the pod pokes through, where it can be), or a gangway straight to the nearest
/// wall. `others` are the hall's pods; `band` is a catwalk's depth in half cells. None if
/// nothing fits.
pub fn pod_access(host: &Space, pod: &Space, others: &[&Space], band: i32) -> Option<Access> {
    let (hx0, hy0, hx1, hy1) = host.rect_h();
    let (sx0, sy0, sx1, sy1) = shell_rect(pod, host);
    let hall = rect_cells((hx0, hy0, hx1, hy1));
    let mut blocked = rect_cells((sx0, sy0, sx1, sy1));
    for o in others {
        if o.key != pod.key && o.floor == pod.floor {
            blocked.extend(rect_cells(shell_rect(o, host)));
        }
    }
    let island = sx0 > hx0 && sy0 > hy0 && sx1 < hx1 && sy1 < hy1;
    let inside = |cells: &Cells| cells.is_subset(&hall) && cells.is_disjoint(&blocked);
    let mut best: Option<(i32, Access)> = None;
    for (&side, &(dx, dy)) in SIDES.iter().zip(DIRS.iter()) {
        let along_x = dx != 0; // the door faces along x: it is in an east or west wall
        let (face, u0, u1) = if along_x {
            let face = if dx > 0 { sx1 } else { sx0 };
            if !(hx0 < face && face < hx1) {
                continue;
            }
            (face, (2 * pod.y0).max(hy0), (2 * pod.y1).min(hy1))
        } else {
            let face = if dy > 0 { sy1 } else { sy0 };
            if !(hy0 < face && face < hy1) {
                continue;
            }
            (face, (2 * pod.x0).max(hx0), (2 * pod.x1).min(hx1))
        };
        if u1 - u0 < 2 {
            continue;
        }
        let w = 2; // a pod's door is one cell wide, as every doorway is
        let (lo_wall, hi_wall) = if along_x { (hy0, hy1) } else { (hx0, hx1) };
        let out = if along_x { dx > 0 } else { dy > 0 }; // the door faces the high side
        let mut starts = vec![u0, u1 - w, (u0 + u1 - w).div_euclid(2)];
        starts.sort();
        starts.dedup();
        for u in starts {
            let near = if out { face } else { face - band }; // the pad's low edge across the wall
            let (pad, hole) = if along_x {
                (rect_cells((near, u, near + band, u + w)), if out { (face - 1, u, face, u + w) } else { (face, u, face + 1, u + w) })
            } else {
                (rect_cells((u, near, u + w, near + band)), if out { (u, face - 1, u + w, face) } else { (u, face, u + w, face + 1) })
            };
            if !inside(&pad) {
                continue;
            }
            let mut options: Vec<(i32, Cells)> = Vec::new();
            if u == lo_wall || u + w == hi_wall {
                options.push((0, Cells::new()));
            }
            let far = if along_x {
                if out {
                    hx1
                } else {
                    hx0
                }
            } else if out {
                hy1
            } else {
                hy0
            };
            // straight on to the wall ahead, or sideways along the pod's face to either wall
            let (a, b) = if out { (face + band, far) } else { (far, face - band) };
            if along_x {
                options.push((b - a, rect_cells((a, u, b, u + w))));
                options.push((u - lo_wall, rect_cells((near, lo_wall, near + band, u))));
                options.push((hi_wall - u - w, rect_cells((near, u + w, near + band, hi_wall))));
            } else {
                options.push((b - a, rect_cells((u, a, u + w, b))));
                options.push((u - lo_wall, rect_cells((lo_wall, near, u, near + band))));
                options.push((hi_wall - u - w, rect_cells((u + w, near, hi_wall, near + band))));
            }
            for (length, gang) in options {
                if length < 0 || !inside(&gang) {
                    continue;
                }
                if best.as_ref().is_none_or(|(len, _)| length < *len) {
                    let deck: Cells = pad.union(&gang).copied().collect();
                    best = Some((length, Access { side, door: hole, deck, pad: pad.clone(), length, island }));
                }
            }
        }
    }
    best.map(|(_, access)| access)
}
