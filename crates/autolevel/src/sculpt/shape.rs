//! One room sculpted from a box: the box's faces are split into a lattice (2 x 2 at the first
//! level, 4 x 4 at the second, ...), and at each level some patches of it are extruded, out
//! (an alcove, a pit, a high bay) or in (a buttress, a platform, a soffit; a patch high on a
//! wall pushed in is a ledge to stand on). Each column of cells under a patch moves its surface
//! along the face's normal, as a mesh face extruded would.
//!
//! ```text
//! let s = Spec::draw(&mut rng, Kind::Wide);
//! let room = sculpt(&mut rng, &s);   // a Vox, zone 1 the room's air, a solid cell all round
//! ```
//!
//! Depths come in a few sizes so the result reads as built, not as noise: a platform 2, 4, 6
//! or 8 cells up (118 to 472), a pit 2 or 4 down, alcoves 2 to 6 cells deep, and each level
//! smaller than the last. No patch is smaller than 8 cells across or 4 up (`MIN_ACROSS`,
//! `MIN_UP`), and a patch moves as one flat face. A room is often mirrored across one axis (each
//! extrusion made twice).

use super::vox::{Ix, Vox};
use crate::pyrand::PyRandom;

/// Levels of subdivision.
pub const LEVELS: u32 = 3;
/// Air a standing player needs over a floor, cells (236).
pub const HEAD: i32 = 4;
/// The least width of air across a room a wall may be pushed in to, cells.
pub const KEEP_ACROSS: i32 = 4;
/// The smallest patch a face is extruded in: cells across (944) and up (236, a storey). The
/// user's rule (2026-11-18): eight times the smallest feature (a cell), or it looks blown up.
pub const MIN_ACROSS: i32 = 8;
pub const MIN_UP: i32 = 4;

/// The kinds of room, by their proportions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An ordinary room.
    Plain,
    /// Very large across, a storey or so tall.
    Wide,
    /// Small across and very tall.
    Tall,
    /// Large across and tall.
    Both,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Plain => "plain",
            Kind::Wide => "wide",
            Kind::Tall => "tall",
            Kind::Both => "both",
        }
    }
}

/// A room's box before it is sculpted: cells across (x, y) and up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub kind: Kind,
    pub w: i32,
    pub d: i32,
    pub h: i32,
    /// Extrusions are made twice, mirrored across this axis (0 x, 1 y).
    pub mirror: Option<usize>,
}

fn int(rng: &mut PyRandom, a: i32, b: i32) -> i32 {
    rng.randint(a as i64, b as i64) as i32
}

impl Spec {
    pub fn draw(rng: &mut PyRandom, kind: Kind) -> Spec {
        let (w, d, h) = match kind {
            Kind::Plain => (int(rng, 14, 20), int(rng, 12, 18), int(rng, 6, 9)),
            Kind::Wide => (int(rng, 28, 36), int(rng, 22, 30), int(rng, 7, 10)),
            Kind::Tall => (int(rng, 14, 18), int(rng, 14, 18), int(rng, 20, 28)),
            Kind::Both => (int(rng, 24, 32), int(rng, 20, 28), int(rng, 14, 20)),
        };
        let mirror = match rng.random() {
            r if r < 0.3 => Some(0),
            r if r < 0.55 => Some(1),
            _ => None,
        };
        Spec { kind, w, d, h, mirror }
    }

    fn size(&self, axis: usize) -> i32 {
        [self.w, self.d, self.h][axis]
    }
}

/// A face of the box: the axis it is square to, and which end (-1 the low end, 1 the high).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Side {
    axis: usize,
    dir: i32,
}

/// One extrusion: a side, a patch of it (two ranges of cells along its two other axes), a depth
/// in cells (out of the room when positive).
#[derive(Clone, Copy, Debug)]
struct Op {
    side: Side,
    u: (i32, i32),
    v: (i32, i32),
    depth: i32,
}

fn get(p: Ix, axis: usize) -> i32 {
    [p.0, p.1, p.2][axis]
}

fn with(mut p: Ix, axis: usize, value: i32) -> Ix {
    match axis {
        0 => p.0 = value,
        1 => p.1 = value,
        _ => p.2 = value,
    }
    p
}

fn others(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// The room: `spec`'s box sculpted, as zone 1 of a grid with room all round for the
/// extrusions, cut down to the air and one solid cell round it.
pub fn sculpt(rng: &mut PyRandom, spec: &Spec) -> Vox {
    let pad = (spec.w.max(spec.d) / 3).max(spec.h / 3) + 4;
    let mut v = Vox::new(spec.w + 2 * pad, spec.d + 2 * pad, spec.h + 2 * pad);
    let origin = (pad, pad, pad);
    for x in 0..spec.w {
        for y in 0..spec.d {
            for z in 0..spec.h {
                v.set((pad + x, pad + y, pad + z), 1);
            }
        }
    }
    for level in 1..=LEVELS {
        let count = ops_at(rng, spec, level);
        for _ in 0..count {
            let Some(op) = draw_op(rng, spec, origin, level) else { continue };
            extrude(&mut v, &op);
            if let Some(axis) = spec.mirror {
                if let Some(m) = mirrored(&op, axis, spec, origin) {
                    extrude(&mut v, &m);
                }
            }
        }
    }
    v.keep_largest(1);
    crop(&v)
}

/// How many extrusions a level makes: more as the faces get finer, more in a bigger room.
fn ops_at(rng: &mut PyRandom, spec: &Spec, level: u32) -> i32 {
    let area = (spec.w * spec.d + 2 * spec.h * (spec.w + spec.d)) as f64;
    let scale = (area / 900.0).sqrt().clamp(0.8, 2.0);
    let (lo, hi) = match level {
        1 => (2, 4),
        2 => (2, 5),
        _ => (3, 6),
    };
    ((int(rng, lo, hi) as f64) * scale).round() as i32
}

/// The lattice of the box along `axis` at `level`: the cell indices its parts start at, and the
/// end. No part is smaller than `MIN_ACROSS` (or `MIN_UP`), so a small face stops dividing.
fn lattice(spec: &Spec, origin: Ix, axis: usize, level: u32) -> Vec<i32> {
    let size = spec.size(axis);
    let least = if axis == 2 { MIN_UP } else { MIN_ACROSS };
    let parts = (1i32 << level).min((size / least).max(1));
    (0..=parts).map(|i| get(origin, axis) + (i * size + parts / 2) / parts).collect()
}

fn draw_op(rng: &mut PyRandom, spec: &Spec, origin: Ix, level: u32) -> Option<Op> {
    let tall = matches!(spec.kind, Kind::Tall | Kind::Both);
    // floor, walls, ceiling
    let r = rng.random();
    // a tall room keeps most of its floor (raised in a narrow room, it leaves a strip under a
    // stack of ledges) and gets its levels from its walls
    let side = if r < if tall { 0.12 } else { 0.35 } {
        Side { axis: 2, dir: -1 }
    } else if r < 0.82 {
        Side { axis: rng.below(2) as usize, dir: if rng.random() < 0.5 { -1 } else { 1 } }
    } else {
        Side { axis: 2, dir: 1 }
    };
    let (ua, va) = others(side.axis);
    let (lu, lv) = (lattice(spec, origin, ua, level), lattice(spec, origin, va, level));
    if level > 1 && lu.len() == 2 && lv.len() == 2 {
        return None; // the face divides no finer: the whole of it was the first level's
    }
    // a patch: one part of the lattice, or a strip of two or three along one of its axes
    let pick = |l: &[i32], long: bool, rng: &mut PyRandom| -> (i32, i32) {
        let n = l.len() - 1;
        let len = if long { (int(rng, 1, 3) as usize).min(n) } else { 1 };
        let at = rng.below((n - len + 1) as u64) as usize;
        (l[at], l[at + len])
    };
    let along_u = rng.random() < 0.5;
    let strip = rng.random() < 0.45;
    let u = pick(&lu, strip && along_u, rng);
    let v = pick(&lv, strip && !along_u, rng);
    let shrink = |d: &[i32], level: u32| -> Vec<i32> {
        let keep = (d.len() as i32 - (level as i32 - 1)).max(1) as usize;
        d[..keep].to_vec()
    };
    let depth = match (side.axis, side.dir) {
        (2, -1) => {
            if rng.random() < 0.65 {
                -*rng.choice(&shrink(&[2, 4, 6, 8], level))
            } else {
                *rng.choice(&[2, 4])
            }
        }
        (2, _) => {
            if rng.random() < 0.65 {
                *rng.choice(&shrink(&[2, 4, 6], level))
            } else {
                -2
            }
        }
        _ => {
            if rng.random() < 0.5 {
                *rng.choice(&shrink(&[2, 4, 6], level))
            } else {
                -*rng.choice(&[2, 4])
            }
        }
    };
    Some(Op { side, u, v, depth })
}

/// The op mirrored across the room's middle along `axis`; None if that is itself.
fn mirrored(op: &Op, axis: usize, spec: &Spec, origin: Ix) -> Option<Op> {
    let (lo, hi) = (get(origin, axis), get(origin, axis) + spec.size(axis));
    let flip = |(a, b): (i32, i32)| (lo + hi - b, lo + hi - a);
    let mut m = *op;
    if op.side.axis == axis {
        m.side.dir = -op.side.dir;
    } else {
        let (ua, _) = others(op.side.axis);
        if ua == axis {
            m.u = flip(op.u);
        } else {
            m.v = flip(op.v);
        }
        if m.u == op.u && m.v == op.v {
            return None;
        }
    }
    Some(m)
}

/// The least depth an extrusion is made at: a shallower one is left out.
const MIN_DEPTH: i32 = 2;

/// Move the patch's face along the side's normal by the op's depth, as one flat face: out, from
/// its outermost cell (every column's air is carried out to the one plane); in, to a plane that
/// far inside its outermost cell (whatever lies beyond the plane is filled). Pushing in is made
/// shallower, for the whole patch, where the air left across would be too thin.
fn extrude(v: &mut Vox, op: &Op) {
    let (a, (ua, va)) = (op.side.axis, others(op.side.axis));
    let n = [v.nx, v.ny, v.nz][a];
    let keep = if a == 2 { HEAD } else { KEEP_ACROSS };
    let dir = op.side.dir;
    let at = |pu: i32, pv: i32, t: i32| with(with(with((0, 0, 0), ua, pu), va, pv), a, t);
    // each column: its outermost air on this side, and the run of air inward from it
    let mut cols: Vec<(i32, i32, i32, i32)> = Vec::new();
    for pu in op.u.0..op.u.1 {
        for pv in op.v.0..op.v.1 {
            let mut ts: Vec<i32> = (0..n).filter(|&t| v.get(at(pu, pv, t)) == 1).collect();
            if dir < 0 {
                ts.reverse();
            }
            let Some(&surface) = ts.last() else { continue };
            let mut run = 0;
            while v.get(at(pu, pv, surface - dir * run)) == 1 {
                run += 1;
            }
            cols.push((pu, pv, surface, run));
        }
    }
    let Some(outer) = (if dir > 0 { cols.iter().map(|c| c.2).max() } else { cols.iter().map(|c| c.2).min() }) else { return };
    if op.depth > 0 {
        let plane = (outer + dir * op.depth).clamp(1, n - 2);
        for &(pu, pv, surface, _) in &cols {
            let mut t = surface + dir;
            while (plane - t) * dir >= 0 {
                v.set(at(pu, pv, t), 1);
                t += dir;
            }
        }
    } else {
        // the deepest the plane may go: each column keeps `keep` cells of air inside it
        let mut d = -op.depth;
        for &(_, _, surface, run) in &cols {
            let reach = (outer - surface) * dir; // how far this column's face is inside the outermost
            d = d.min(run - keep + reach);
        }
        if d < MIN_DEPTH {
            return;
        }
        let plane = outer - dir * d;
        for &(pu, pv, surface, _) in &cols {
            let mut t = surface;
            while (t - plane) * dir > 0 {
                v.set(at(pu, pv, t), 0);
                t -= dir;
            }
        }
    }
}

/// The grid cut down to the air and one solid cell round it.
fn crop(v: &Vox) -> Vox {
    let Some((lo, hi)) = v.bounds(1) else { return Vox::new(1, 1, 1) };
    let mut out = Vox::new(hi.0 - lo.0 + 2, hi.1 - lo.1 + 2, hi.2 - lo.2 + 2);
    for p in v.each() {
        if v.get(p) == 1 {
            out.set((p.0 - lo.0 + 1, p.1 - lo.1 + 1, p.2 - lo.2 + 1), 1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_room_is_one_piece_and_the_same_twice() {
        for (seed, kind) in [(1, Kind::Plain), (2, Kind::Wide), (3, Kind::Tall), (4, Kind::Both)] {
            let make = || {
                let mut rng = PyRandom::from_int(seed);
                let s = Spec::draw(&mut rng, kind);
                (s, sculpt(&mut rng, &s))
            };
            let (s, a) = make();
            let (_, b) = make();
            assert_eq!(a.cells_of(1), b.cells_of(1));
            assert_eq!(a.parts(1).len(), 1, "{kind:?}");
            // the sculpting moved the box's faces
            assert_ne!(a.cells_of(1).len() as i32, s.w * s.d * s.h, "{kind:?}");
        }
    }
}
