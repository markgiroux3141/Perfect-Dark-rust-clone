//! Windows: where a corridor runs along a hall's wall, a cell away, the wall between is opened.
//! From `autolevel/level.py` (`_windows`, `_clear_inside`).
//!
//! Two to four cells of wall, glazed 94 high, or railed (a balcony) where the corridor is above
//! the hall's floor. Only into rooms of 30 cells or more, and only where the room has nothing
//! against that wall on that storey (no deck, ramp, pod, pillar or water). Those from above come
//! first. They are chosen once every room is furnished.

use crate::cells::{rect_cells, Cell, Cells};
use crate::furnish::{meets, runs, Interior};
use crate::level::{Level, Window};
use crate::pyrand::PyRandom;
use crate::space::DIRS;
use crate::storeys::storey;
use std::collections::BTreeSet;

/// A place a window could go: (on the hall's floor, minus its width, piece, hall, storey, the
/// wall cells, direction). Sorted, the ones from above come first, then the widest.
type Found = (bool, i32, String, String, i32, Vec<Cell>, usize);

/// The level's windows: at most `options.windows`, one to a corridor piece and two to a hall.
pub fn windows(lv: &Level) -> Vec<Window> {
    let (sp, grid) = (lv.spaces(), &lv.net.grid);
    let mut rng = PyRandom::named(&format!("{}/windows", lv.seed));
    let mut found: Vec<Found> = Vec::new();
    for r in lv.net.routes() {
        for pc in r.pieces.iter().filter(|pc| matches!(pc.kind, "corridor" | "ramp")) {
            for (z, cells) in &pc.cells {
                let z = *z;
                for hall in sp {
                    if !hall.host.is_empty() || !hall.cuts.is_empty() || hall.area() < 30 || !(hall.floor <= z && storey(z).1 <= hall.top) {
                        continue;
                    }
                    let it = lv.interior(&hall.key);
                    for (d, &(dx, dy)) in DIRS.iter().enumerate() {
                        // the corridor's cells with one cell of wall between them and the hall
                        // (all in one line, so sorting them along it is a whole order)
                        let mut line: Vec<Cell> = cells
                            .iter()
                            .copied()
                            .filter(|c| {
                                hall.x0 <= c.0 + 2 * dx
                                    && c.0 + 2 * dx < hall.x1
                                    && hall.y0 <= c.1 + 2 * dy
                                    && c.1 + 2 * dy < hall.y1
                                    && grid.owner_at(c.0 + dx, c.1 + dy, z) == 0
                            })
                            .collect();
                        line.sort_by_key(|c| if dx != 0 { c.1 } else { c.0 });
                        for run in runs(line.iter().map(|c| if dx != 0 { c.1 } else { c.0 }), None) {
                            let (ax, ay) = if dx != 0 { (0, 1) } else { (1, 0) };
                            let mut wall: Vec<Cell> = run.iter().map(|&t| if dx != 0 { (line[0].0 + dx, t) } else { (t, line[0].1 + dy) }).collect();
                            // the wall cells past each end stay wall
                            while !wall.is_empty() && grid.owner_at(wall[0].0 - ax, wall[0].1 - ay, z) != 0 {
                                wall.remove(0);
                            }
                            while let Some(&last) = wall.last() {
                                if grid.owner_at(last.0 + ax, last.1 + ay, z) == 0 {
                                    break;
                                }
                                wall.pop();
                            }
                            while wall.len() > 4 {
                                if wall.len() % 2 == 1 {
                                    wall.remove(0);
                                } else {
                                    wall.pop();
                                }
                            }
                            if wall.len() >= 2 && clear_inside(it, &wall, (dx, dy), z) {
                                found.push((z == hall.floor, -(wall.len() as i32), pc.key.clone(), hall.key.clone(), z, wall, d));
                            }
                        }
                    }
                }
            }
        }
    }
    found.sort();
    let mut out: Vec<Window> = Vec::new();
    let mut pieces: BTreeSet<String> = BTreeSet::new();
    for (level, _, piece, hall, z, wall, d) in found {
        if out.len() >= lv.options.windows {
            break;
        }
        if pieces.contains(&piece) || out.iter().filter(|w| w.hall == hall).count() >= 2 {
            continue;
        }
        pieces.insert(piece.clone());
        // (an empty start: the window's own stream, by its hall and where it is in the wall)
        let mut own = None;
        let rng = lv.options.stream(&mut rng, &mut own, || format!("{}/{hall}/window/{z}/{},{}", lv.seed, wall[0].0, wall[0].1));
        let kind = if !level && rng.random() < 0.3 { "rail" } else { "glass" };
        out.push(Window { piece, hall, z, wall, d, kind });
    }
    out
}

/// Is a hall clear behind these wall cells on storey `z`, two cells deep?
fn clear_inside(it: &Interior, wall: &[Cell], d: (i32, i32), z: i32) -> bool {
    let mut front = Cells::new();
    for &(wx, wy) in wall {
        for k in [1, 2] {
            let (x, y) = (wx + d.0 * k, wy + d.1 * k);
            front.extend(rect_cells((2 * x, 2 * y, 2 * x + 2, 2 * y + 2)));
        }
    }
    if [&it.deck, &it.shell, &it.roof].iter().any(|by| by.get(&z).is_some_and(|cells| meets(&front, cells))) {
        return false;
    }
    if z == it.s.floor && (meets(&front, &it.pool) || meets(&front, &it.hole)) {
        return false;
    }
    if it.ramps.iter().any(|r| r.lo == z && meets(&r.cells(), &front)) {
        return false;
    }
    for &(x0, y0, x1, y1, h, _) in &it.cover {
        if it.s.floor + h > z && meets(&rect_cells((x0, y0, x1, y1)), &front) {
            return false;
        }
    }
    !it.piers.iter().any(|&(x0, y0, x1, y1, _, b)| b > z && meets(&rect_cells((x0, y0, x1, y1)), &front))
}
