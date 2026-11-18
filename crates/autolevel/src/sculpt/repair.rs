//! Making a sculpted room walkable: every doorway and every floor of some size (a platform, a
//! pit, a ledge) must be reachable from every other and back (one strongly connected component
//! of the room's regions; `nav`). What is left out is joined on with the cheapest of:
//!
//! - a **ramp**: a sheet two cells wide rising a cell (59) for each cell (118) it runs, from a
//!   floor up to the edge of a higher one, over whatever lies under it (open underneath, railed);
//! - a **switchback**: two such ramps side by side, rising the other way from a landing built
//!   half-way (for a climb too long for one straight run);
//! - a **catwalk**: a slab two cells wide straight across a drop between two floors at one
//!   height, railed where it is open.
//!
//! Each is tried and kept only if the floors it joins are then mutually reachable and nothing
//! that was reachable is lost (a ramp's sheet takes the floor under it; a catwalk may cut a
//! way). A floor nothing joins is given up and reported.

use super::nav::{standable, Nav, Sheet, STAND};
use super::vox::{add, up, Ix, Vox};
use std::collections::{BTreeSet, HashSet};

/// The smallest floor that must be reached, cells.
pub const MIN_AREA: usize = 6;
/// The longest catwalk, cells.
pub const MAX_SPAN: i32 = 24;
/// The highest ramp, cells (a storey and a half).
pub const MAX_RISE: i32 = 16;
/// Joins tried for one floor before it is given up.
const TRIES: usize = 16;

const ACROSS: [Ix; 4] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0)];

/// A rail: two ends on grid lines and the way it faces.
pub type Rail = ((i32, i32, i32), (i32, i32, i32), Ix);

/// A catwalk: its slab cells (the cells its floor sits in), its two lanes' outer sides as rails.
#[derive(Clone, Debug, PartialEq)]
pub struct Catwalk {
    pub zone: u16,
    pub slab: Vec<Ix>,
    /// Rails: two ends on grid lines (x, y faces; z the cell whose floor they stand on) and the
    /// way they face.
    pub rails: Vec<Rail>,
}

/// What the repair did.
#[derive(Clone, Debug, Default)]
pub struct Repaired {
    pub ramps: usize,
    pub catwalks: usize,
    /// Floors (a cell of each) no join was found for.
    pub given_up: Vec<Ix>,
}

/// One way to join a floor on.
#[derive(Clone, Debug)]
enum Join {
    Ramp(Sheet),
    /// Two flights and the landing's slab cells.
    Switch(Vec<Sheet>, Vec<Ix>),
    Catwalk(Catwalk),
}

fn perp(d: Ix) -> Ix {
    if d.0 != 0 {
        (0, 1, 0)
    } else {
        (1, 0, 0)
    }
}

fn sub(a: Ix, b: Ix) -> Ix {
    (a.0 - b.0, a.1 - b.1, a.2 - b.2)
}

fn times(d: Ix, n: i32) -> Ix {
    (d.0 * n, d.1 * n, d.2 * n)
}

/// The grid line between cell `a` and the cell after it along `d`, on `d`'s axis.
fn face(a: Ix, d: Ix) -> i32 {
    if d.0 > 0 {
        a.0 + 1
    } else if d.0 < 0 {
        a.0
    } else if d.1 > 0 {
        a.1 + 1
    } else {
        a.1
    }
}

/// A point on grid lines: `along` on `d`'s axis, `across` on the other, at height `k`.
fn point(d: Ix, along: i32, across: i32, k: i32) -> (i32, i32, i32) {
    if d.0 != 0 {
        (along, across, k)
    } else {
        (across, along, k)
    }
}

fn across_of(c: Ix, d: Ix) -> i32 {
    if d.0 != 0 {
        c.1
    } else {
        c.0
    }
}

/// The room's regions the repair must join: those with a doorway, and those of `MIN_AREA` or
/// more. (region, has a doorway)
fn targets(nav: &Nav, doors: &[Ix]) -> Vec<(usize, bool)> {
    let with_door: BTreeSet<usize> = doors.iter().filter_map(|d| nav.region_of.get(d).copied()).collect();
    (0..nav.regions.len()).filter(|r| with_door.contains(r) || nav.regions[*r].cells.len() >= MIN_AREA).map(|r| (r, with_door.contains(&r))).collect()
}

/// How many pieces the targets are in, and how many target cells are walkable at all.
fn pieces(nav: &Nav, doors: &[Ix]) -> (usize, usize) {
    let comp = nav.components();
    let ts = targets(nav, doors);
    let set: BTreeSet<usize> = ts.iter().map(|(r, _)| comp[*r]).collect();
    (set.len(), ts.iter().map(|(r, _)| nav.regions[*r].cells.len()).sum())
}

/// Join every doorway and floor of `zone` (inside the box `lo..hi`) to the rest. `doors` are
/// cells of doorways (which must be reached); `keep` columns nothing may be built over (in front
/// of doorways).
///
/// Greedy: of every join between two regions in different components (strongly connected:
/// `Nav::components`), the cheapest that leaves the targets in fewer pieces is made, and again,
/// until they are one piece or no join helps. A join that would join a piece only to a region
/// no target is in (a stepping stone) is made only when nothing better is left.
#[allow(clippy::too_many_arguments)]
pub fn repair(v: &mut Vox, sheets: &mut Vec<Sheet>, walks: &mut Vec<Catwalk>, zone: u16, lo: Ix, hi: Ix, doors: &[Ix], keep: &HashSet<(i32, i32)>) -> Repaired {
    let mut out = Repaired::default();
    for _ in 0..48 {
        let nav = Nav::build(v, sheets, Some((zone, lo, hi)));
        let comp = nav.components();
        let ts = targets(&nav, doors);
        let (before, area) = pieces(&nav, doors);
        if before <= 1 {
            break;
        }
        let has_target: BTreeSet<usize> = ts.iter().map(|(r, _)| comp[*r]).collect();
        let mut joins = candidates(v, &nav, sheets, zone, &comp, keep);
        // joins between two pieces with targets first, the cheapest first
        joins.sort_by(|a, b| {
            let rank = |j: &(f64, usize, usize, Join)| (!(has_target.contains(&comp[j.1]) && has_target.contains(&comp[j.2])), j.0);
            let (ra, rb) = (rank(a), rank(b));
            ra.0.cmp(&rb.0).then(ra.1.total_cmp(&rb.1))
        });
        let mut done = false;
        for (_, _, _, join) in joins.into_iter().take(TRIES * 3) {
            let undo = apply(v, sheets, &join);
            let after = Nav::build(v, sheets, Some((zone, lo, hi)));
            let (now, area2) = pieces(&after, doors);
            let stepping = now == before && after.regions.len() < nav.regions.len();
            if (now < before || stepping) && area2 + 8 >= area {
                match join {
                    Join::Ramp(_) => out.ramps += 1,
                    Join::Switch(..) => out.ramps += 2,
                    Join::Catwalk(c) => {
                        out.catwalks += 1;
                        walks.push(c);
                    }
                }
                done = true;
                break;
            }
            revert(v, sheets, undo);
        }
        if !done {
            break;
        }
    }
    let nav = Nav::build(v, sheets, Some((zone, lo, hi)));
    let comp = nav.components();
    let ts = targets(&nav, doors);
    let main = ts.iter().max_by_key(|(r, door)| (*door, nav.regions[*r].cells.len())).map(|(r, _)| comp[*r]);
    let mut weight: std::collections::BTreeMap<usize, usize> = Default::default();
    for &(r, door) in &ts {
        *weight.entry(comp[r]).or_default() += if door { 100_000 } else { nav.regions[r].cells.len() };
    }
    let main = weight.into_iter().max_by_key(|&(c, w)| (w, std::cmp::Reverse(c))).map(|(c, _)| c).or(main);
    for &(r, _) in &ts {
        if Some(comp[r]) != main {
            out.given_up.push(nav.regions[r].cells[0]);
            if std::env::var_os("SCULPT_DEBUG").is_some() {
                let reg = &nav.regions[r];
                eprintln!("zone {zone}: left out floor k {} ({} cells, at {:?})", reg.k, reg.cells.len(), reg.cells[0]);
            }
        }
    }
    out
}

/// What `apply` changed: the cells made solid (with what they were), and how many sheets added.
struct Undo {
    slab: Vec<(Ix, u16)>,
    sheets: usize,
}

fn apply(v: &mut Vox, sheets: &mut Vec<Sheet>, join: &Join) -> Undo {
    let (add_sheets, slab): (&[Sheet], &[Ix]) = match join {
        Join::Ramp(s) => (std::slice::from_ref(s), &[]),
        Join::Switch(two, slab) => (two, slab),
        Join::Catwalk(c) => (&[], &c.slab),
    };
    sheets.extend(add_sheets.iter().cloned());
    let was: Vec<(Ix, u16)> = slab.iter().map(|&p| (p, v.get(p))).collect();
    for &p in slab {
        v.set(p, 0);
    }
    Undo { slab: was, sheets: add_sheets.len() }
}

fn revert(v: &mut Vox, sheets: &mut Vec<Sheet>, undo: Undo) {
    sheets.truncate(sheets.len() - undo.sheets);
    for (p, z) in undo.slab {
        v.set(p, z);
    }
}

/// Every ramp and catwalk between two regions in different components (two cells wide or
/// more): (cost, one region, the other, the join).
fn candidates(v: &Vox, nav: &Nav, sheets: &[Sheet], zone: u16, comp: &[usize], keep: &HashSet<(i32, i32)>) -> Vec<(f64, usize, usize, Join)> {
    let covered: HashSet<(i32, i32)> = sheets.iter().flat_map(|s| s.cover.iter().map(|&(x, y, _)| (x, y))).collect();
    let free = |c: Ix| !keep.contains(&(c.0, c.1)) && !covered.contains(&(c.0, c.1));
    let mut out = Vec::new();
    let live: Vec<usize> = (0..nav.regions.len()).filter(|&r| nav.regions[r].cells.len() >= 2).collect();
    for (i, &a) in live.iter().enumerate() {
        for &b in &live[i + 1..] {
            if comp[a] == comp[b] {
                continue;
            }
            let (ka, kb) = (nav.regions[a].k, nav.regions[b].k);
            if ka == kb {
                out.extend(catwalks(v, nav, zone, a, b, &free).into_iter().map(|(c, j)| (c, a, b, Join::Catwalk(j))));
                out.extend(catwalks(v, nav, zone, b, a, &free).into_iter().map(|(c, j)| (c, a, b, Join::Catwalk(j))));
            } else if (ka - kb).abs() <= MAX_RISE {
                let (low, high) = if ka < kb { (a, b) } else { (b, a) };
                out.extend(ramps(v, nav, zone, low, high, &free).into_iter().map(|(c, s)| (c, a, b, Join::Ramp(s))));
                if (ka - kb).abs() >= 4 {
                    out.extend(switchbacks(v, nav, zone, low, high, &free).into_iter().map(|(c, j)| (c, a, b, j)));
                }
            }
        }
    }
    out
}

/// Ramps from region `low` up to the edge of region `high`.
fn ramps(v: &Vox, nav: &Nav, zone: u16, low: usize, high: usize, free: &impl Fn(Ix) -> bool) -> Vec<(f64, Sheet)> {
    let (kl, ku) = (nav.regions[low].k, nav.regions[high].k);
    let n = ku - kl;
    let is = |c: Ix, reg: usize| nav.region_of.get(&c) == Some(&reg);
    let mut out = Vec::new();
    for &e in &nav.regions[high].cells {
        for d in ACROSS {
            if is(sub(e, d), high) {
                continue; // not an edge on this side
            }
            let t = perp(d);
            if !is(add(e, t), high) {
                continue;
            }
            // the run: cells e - d*n .. e - d, then the foot before it
            let run: Vec<Ix> = (0..n).map(|i| sub(e, times(d, n - i))).map(|c| (c.0, c.1, kl)).collect();
            let foot = (e.0 - d.0 * (n + 1), e.1 - d.1 * (n + 1), kl);
            if !is(foot, low) || !is(add(foot, t), low) {
                continue;
            }
            let clear = run.iter().enumerate().all(|(i, &c)| {
                // clear from the sheet's underside up: under it may be anything lower
                [c, add(c, t)].into_iter().all(|q| free(q) && (i as i32..=i as i32 + STAND).all(|k| v.get(up(q, k)) == zone))
            });
            if !clear {
                continue;
            }
            let cover = run.iter().enumerate().flat_map(|(i, &c)| [c, add(c, t)].map(|q| (q.0, q.1, kl + i as i32 + 1))).collect();
            let across = across_of(e, d).max(across_of(add(e, t), d));
            let start = point(d, face(foot, d), across, kl);
            let end = point(d, face(*run.last().expect("a ramp rises"), d), across, ku);
            let sheet = Sheet { zone, foot: [foot, add(foot, t)], top: [e, add(e, t)], cover, start, end, open: true };
            out.push((n as f64, sheet));
        }
    }
    out
}

/// Switchbacks from region `low` up to the edge of region `high`: the upper flight runs up to
/// the edge, the lower beside it the other way, a landing four cells across joining the two.
fn switchbacks(v: &Vox, nav: &Nav, zone: u16, low: usize, high: usize, free: &impl Fn(Ix) -> bool) -> Vec<(f64, Join)> {
    let (kl, ku) = (nav.regions[low].k, nav.regions[high].k);
    let n = ku - kl;
    let (n1, n2) = (n / 2, n - n / 2);
    let km = kl + n1;
    let is = |c: Ix, reg: usize| nav.region_of.get(&c) == Some(&reg);
    let at = |c: Ix, k: i32| (c.0, c.1, k);
    let clear_from = |q: Ix, k0: i32, k1: i32| free(q) && (k0..=k1).all(|k| v.get(at(q, k)) == zone);
    let mut out = Vec::new();
    for &e in &nav.regions[high].cells {
        for d in ACROSS {
            if is(sub(e, d), high) {
                continue;
            }
            let t = perp(d);
            if !is(add(e, t), high) {
                continue;
            }
            let landing_row = sub(e, times(d, n2 + 1));
            for off in [2, -2] {
                // the upper flight: lanes 0 and 1, from the landing up to the edge
                let upper: Vec<Ix> = (0..n2).map(|i| at(sub(e, times(d, n2 - i)), km)).collect();
                // the landing: four lanes across
                let lanes: [i32; 4] = if off > 0 { [0, 1, 2, 3] } else { [-2, -1, 0, 1] };
                let landing: Vec<Ix> = lanes.iter().map(|&j| at(add(landing_row, times(t, j)), km)).collect();
                // the lower flight: lanes `off` and `off + 1`, from its foot up to the landing
                let a0 = at(add(landing_row, times(t, off)), km);
                let lower: Vec<Ix> = (0..n1).map(|i| at(add(a0, times(d, n1 - i)), kl)).collect();
                let foot = at(add(a0, times(d, n1 + 1)), kl);
                if !is(foot, low) || !is(add(foot, t), low) {
                    continue;
                }
                let flight_clear = |run: &[Ix], k0: i32| {
                    run.iter().enumerate().all(|(i, &c)| [c, add(c, t)].into_iter().all(|q| clear_from(q, k0 + i as i32, k0 + i as i32 + STAND)))
                };
                let ok = flight_clear(&upper, km) && landing.iter().all(|&q| clear_from(q, km - 1, km + STAND - 1)) && flight_clear(&lower, kl);
                if !ok {
                    continue;
                }
                let back = times(d, -1);
                let across = |c: Ix| across_of(c, d).max(across_of(add(c, t), d));
                let cover = |run: &[Ix], k0: i32| -> Vec<(i32, i32, i32)> {
                    run.iter().enumerate().flat_map(|(i, &c)| [c, add(c, t)].map(|q| (q.0, q.1, k0 + i as i32 + 1))).collect()
                };
                let lower_sheet = Sheet {
                    zone,
                    foot: [foot, add(foot, t)],
                    top: [a0, add(a0, t)],
                    cover: cover(&lower, kl),
                    start: point(back, face(foot, back), across(a0), kl),
                    end: point(back, face(*lower.last().expect("a flight"), back), across(a0), km),
                    open: true,
                };
                let row = at(landing_row, km);
                let upper_sheet = Sheet {
                    zone,
                    foot: [row, add(row, t)],
                    top: [e, add(e, t)],
                    cover: cover(&upper, km),
                    start: point(d, face(row, d), across(e), km),
                    end: point(d, face(*upper.last().expect("a flight"), d), across(e), ku),
                    open: true,
                };
                let slab = landing.iter().map(|&q| up(q, -1)).collect();
                out.push((n as f64 + 3.0, Join::Switch(vec![lower_sheet, upper_sheet], slab)));
            }
        }
    }
    out
}

/// Catwalks from region `a` straight across to region `b`, at their one height.
fn catwalks(v: &Vox, nav: &Nav, zone: u16, a: usize, b: usize, free: &impl Fn(Ix) -> bool) -> Vec<(f64, Catwalk)> {
    let k = nav.regions[a].k;
    let is = |c: Ix, reg: usize| nav.region_of.get(&c) == Some(&reg);
    let mut out = Vec::new();
    for &e in &nav.regions[a].cells {
        for d in ACROSS {
            let t = perp(d);
            if is(add(e, d), a) || !is(add(e, t), a) {
                continue;
            }
            let mut span = Vec::new();
            let mut far = None;
            for j in 1..=MAX_SPAN {
                let s = add(e, times(d, j));
                if is(s, b) && is(add(s, t), b) {
                    far = Some(s);
                    break;
                }
                let ok = [s, add(s, t)].into_iter().all(|q| free(q) && !nav.region_of.contains_key(&q) && (-1..STAND).all(|kk| v.get(up(q, kk)) == zone));
                if !ok {
                    break;
                }
                span.push(s);
            }
            let Some(far) = far else { continue };
            if span.is_empty() {
                continue;
            }
            let slab: Vec<Ix> = span.iter().flat_map(|&s| [up(s, -1), up(add(s, t), -1)]).collect();
            let _ = far;
            // the two outer sides, railed only where they are open to a drop (a rail beside a
            // floor at the catwalk's height would stand across a way on)
            let neg = times(t, -1);
            let mut rails = Vec::new();
            for (lane, out_dir, line) in [(e, neg, across_of(e, d)), (add(e, t), t, across_of(add(e, t), d) + 1)] {
                let open = |s: Ix| {
                    let beside = add(add(s, sub(lane, e)), out_dir);
                    !standable(v, beside)
                };
                let mut run: Option<(Ix, Ix)> = None;
                for &s in span.iter().chain(std::iter::once(&(0, 0, i32::MIN))) {
                    if s.2 != i32::MIN && open(s) {
                        run = Some(run.map_or((s, s), |(a, _)| (a, s)));
                        continue;
                    }
                    if let Some((a, b)) = run.take() {
                        rails.push((point(d, face(sub(a, d), d), line, k), point(d, face(b, d), line, k), out_dir));
                    }
                }
            }
            out.push((span.len() as f64 * 1.2, Catwalk { zone, slab, rails }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room 14 x 6, 12 tall: a floor, a platform 4 up over its east part and a pit 2 down in
    /// the middle of the west part.
    fn room() -> Vox {
        let mut v = Vox::new(16, 8, 14);
        for p in v.each().collect::<Vec<_>>() {
            let inside = (1..15).contains(&p.0) && (1..7).contains(&p.1) && (3..13).contains(&p.2);
            let platform = p.0 >= 10 && p.2 < 7;
            let pit = (2..5).contains(&p.0) && (1..7).contains(&p.1) && (1..3).contains(&p.2);
            if (inside && !platform) || pit {
                v.set(p, 1);
            }
        }
        v
    }

    #[test]
    fn ramps_join_a_platform_and_a_pit() {
        let mut v = room();
        let (mut sheets, mut walks) = (Vec::new(), Vec::new());
        let doors = [(1, 3, 3), (14, 3, 7)];
        let r = repair(&mut v, &mut sheets, &mut walks, 1, (0, 0, 0), (16, 8, 14), &doors, &HashSet::new());
        assert!(r.given_up.is_empty(), "{r:?}");
        assert!(r.ramps >= 2, "{r:?}");
        let nav = Nav::build(&v, &sheets, None);
        for a in doors {
            for b in doors {
                assert!(nav.path(a, b).is_some(), "{a:?} -> {b:?}");
            }
        }
    }
}
