//! A pod's own inside: its air, its door through the shell onto the hall's deck, and its
//! windows. From `furnish.py` (`PodPlan`, `pod_interior`, `_minus`).

use crate::cells::{rect_cells, Cells};
use crate::plan::{box3, Box3};
use crate::pods::Access;
use crate::space::{shell_rect, Space, DIRS, SIDES};
use crate::storeys::storey;

/// A pod's door. Like `space::Door`, but its place along the wall and its width may be half
/// cells: a pod's shell is half a cell thick, so its door need not sit on the cell grid.
#[derive(Clone, Debug, PartialEq)]
pub struct PodDoor {
    pub room: String,
    /// E, N, W or S: the wall it is in.
    pub side: char,
    /// Its low end along the wall, cells.
    pub a: f64,
    /// The floor it is on.
    pub level: i32,
    pub width: f64,
    /// What is through it: the hall.
    pub to: String,
}

/// A window's glass: its two ends (x, y) in cells, the floor it stands on, and the way it faces.
pub type Glass = ((f64, f64), (f64, f64), i32, (i32, i32, i32));

/// A pod's inside.
#[derive(Clone, Debug, PartialEq)]
pub struct PodPlan {
    /// Its air: its own box, the door through the shell, and the window openings.
    pub boxes: Vec<Box3>,
    pub glass: Vec<Glass>,
    pub door: Option<PodDoor>,
}

/// A pod's air: its own box, its door through the shell onto the hall's deck, and a window
/// through every other wall that looks into the hall (glass 94 high across each). No window
/// where another pod is hard against that wall. `others` are the hall's pods.
pub fn pod_interior(host: &Space, pod: &Space, acc: Option<&Access>, others: &[&Space]) -> PodPlan {
    let (hx0, hy0, hx1, hy1) = host.rect_h();
    let (lo, hi) = storey(pod.floor);
    let half = |v: i32| v as f64 / 2.0;
    let mut out = PodPlan { boxes: vec![box3(pod.x0 as f64, pod.y0 as f64, pod.x1 as f64, pod.y1 as f64, lo, hi)], glass: Vec::new(), door: None };
    let (sx0, sy0, sx1, sy1) = shell_rect(pod, host);
    let mut near = Cells::new();
    for o in others {
        if o.key != pod.key && o.floor == pod.floor {
            near.extend(rect_cells(shell_rect(o, host)));
        }
    }
    if let Some(acc) = acc {
        let (x0, y0, x1, y1) = acc.door;
        out.boxes.push(box3(half(x0), half(y0), half(x1), half(y1), lo, hi));
        let along_y = acc.side == 'E' || acc.side == 'W';
        let a = half(if along_y { y0 } else { x0 });
        out.door = Some(PodDoor {
            room: pod.key.clone(),
            side: acc.side,
            a,
            level: pod.floor,
            width: half(if along_y { y1 - y0 } else { x1 - x0 }),
            to: host.key.clone(),
        });
    }
    for (&side, &(dx, dy)) in SIDES.iter().zip(DIRS.iter()) {
        let door = acc.filter(|acc| acc.side == side).map(|acc| acc.door);
        if dx != 0 {
            let face = if dx > 0 { sx1 } else { sx0 };
            if !(hx0 + 2 <= face && face <= hx1 - 2) {
                continue; // no hall air past this wall
            }
            let (u0, u1) = ((2 * pod.y0).max(hy0 + 1), (2 * pod.y1).min(hy1 - 1));
            for (a, b) in minus((u0, u1), door.map(|d| (d.1, d.3))) {
                if b - a < 2 || !near.is_disjoint(&rect_cells((face - 2, a - 1, face + 2, b + 1))) {
                    continue;
                }
                let (wx0, wx1) = if dx > 0 { (face - 1, face) } else { (face, face + 1) };
                out.boxes.push(box3(half(wx0), half(a), half(wx1), half(b), lo, hi));
                out.glass.push(((half(face), half(a)), (half(face), half(b)), pod.floor, (dx, 0, 0)));
            }
        } else {
            let face = if dy > 0 { sy1 } else { sy0 };
            if !(hy0 + 2 <= face && face <= hy1 - 2) {
                continue;
            }
            let (u0, u1) = ((2 * pod.x0).max(hx0 + 1), (2 * pod.x1).min(hx1 - 1));
            for (a, b) in minus((u0, u1), door.map(|d| (d.0, d.2))) {
                if b - a < 2 || !near.is_disjoint(&rect_cells((a - 1, face - 2, b + 1, face + 2))) {
                    continue;
                }
                let (wy0, wy1) = if dy > 0 { (face - 1, face) } else { (face, face + 1) };
                out.boxes.push(box3(half(a), half(wy0), half(b), half(wy1), lo, hi));
                out.glass.push(((half(a), half(face)), (half(b), half(face)), pod.floor, (0, dy, 0)));
            }
        }
    }
    out
}

/// A span with a hole (and a half cell of wall each side of it) taken out.
fn minus(span: (i32, i32), hole: Option<(i32, i32)>) -> Vec<(i32, i32)> {
    match hole {
        Some(hole) if hole.1 > span.0 && hole.0 < span.1 => vec![(span.0, hole.0 - 1), (hole.1 + 1, span.1)],
        _ => vec![span],
    }
}
