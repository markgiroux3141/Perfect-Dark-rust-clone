//! The lighting bake: every corner's colour ray-traced from the plan's lamps, each room
//! rescaled to its target brightness, then the beams and the red glows. It is what
//! `scripts/autolevel_build.py`'s `light()` does in Blender (`styles/complex.py`'s `bake`, and
//! `build.py`'s `bake_lighting`, `add_beam` and `glow`), with no Blender.
//!
//! A corner's colour is `min(255, (ambient + bounce on what faces down + the room's gain x its
//! lamps) x the room's tint)`, where a lamp gives its colour x Lambert x falloff x how much of it
//! the corner sees (seven rays to a lamp with a size, one to a point), the same for every corner
//! of a room that shares a vertex and a normal. All the rays are cast once (`trace`), on threads,
//! and kept as what each lamp gives each corner; the passes that find a room's gain are then
//! sums. The Python casts them again for every pass, which is where Blender's twelve seconds go.
//!
//! Everything that blocks light is one tree (`trace::Bvh`): every triangle of the level but the
//! Transparent ones (glass, rails and floor grates let light through), hit from either side. The
//! rays are cast as Blender casts them, in single precision with its own tests, so the corners
//! where a ray grazes an edge come out as they do there. See `trace`.
//!
//! Blender holds these colours as bytes, and the Python reads them back through sRGB and gets a
//! float a hair off the byte (`Reader`): above it for most bytes, below it for a few. That hair
//! decides whether a room exactly 4 from its target is calibrated again, so it is kept. What is
//! not kept: Blender turns a float into a byte with a fast approximate power, which moves the
//! half-way points by up to 0.02 of a level, differently on different processors. Here a value
//! becomes the nearest byte.

#[cfg(test)]
mod tests;
mod trace;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use autolevel::level::Level;
use autolevel::py;

use crate::complex::{self as cx, Light};
use crate::csg;
use crate::geometry::{Lit, BEAM_STRENGTH};
use crate::kit::Kit;
use crate::mesh::Room;
use crate::sunbeam::{pysum, Beam};
use trace::{Bvh, F3};

/// How the level is lit: `scripts/autolevel_build.py`'s `LIGHTING`, and the rest of what its
/// `light()` passes on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub ambient: f64,
    pub bounce: f64,
    pub spot: f64,
    /// The floor median of rooms in the "low" scheme: dim, as Complex's 2A is.
    pub low_floor: f64,
    /// And of vents: Complex's.
    pub vent_floor: f64,
    /// The colour of the lights at corridor bends (Complex's red), and how far each reaches.
    pub glow: [f64; 3],
    pub glow_radius: f64,
    /// Threads to cast the rays on (0: as many as the machine has). The result is the same for any number.
    pub threads: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { ambient: 52.0, bounce: 33.0, spot: 0.4, low_floor: 126.0, vent_floor: 137.0, glow: [141.0, 0.0, 0.0], glow_radius: 330.0, threads: 0 }
    }
}

/// `bake_lighting`'s samples over a lamp with a size: its middle, then a step along each axis either way.
const SAMPLES: [F3; 7] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
/// How far a ray starts off its surface, and again along itself: a corner lies in the plane of
/// the wall next to it, which a ray starting on it would hit at once.
const BIAS: f64 = 1.5;
/// How many times `bake` adjusts the gains.
const PASSES: usize = 4;
/// A room is calibrated until its floor median is within this of its target.
const CLOSE: f64 = 4.0;

/// Bake the rooms' corner colours, to `kit`'s targets and in its tints. Returns each room's
/// gain, by its name (`Room_NN`), in the plan's order.
pub fn light(lv: &Level, rooms: &mut [Room], lit: &[Lit], kit: &Kit, set: &Settings) -> Result<Vec<(String, f64)>, String> {
    // a room with lamps of its own (a Felicity room's light panels) has them in place of its grid
    let lights = if lv.lamps.is_empty() {
        cx::plan_lights(&lv.plan, None, set.spot)
    } else {
        let others: Vec<&str> = lv.plan.rooms.iter().map(|r| r.key.as_str()).filter(|k| !lv.lamps.iter().any(|(r, _)| r == k)).collect();
        let mut lights = cx::plan_lights(&lv.plan, Some(&others), set.spot);
        let how = cx::Lamps { spot: set.spot, ..cx::Lamps::default() };
        lights.extend(lv.lamps.iter().map(|(_, p)| Light { pos: csg::f3(*p), color: [how.strength; 3], radius: how.radius, size: how.size, spot: how.spot }));
        lights
    };
    let mut targets = Vec::with_capacity(rooms.len());
    let mut tints = Vec::with_capacity(rooms.len());
    for r in rooms.iter() {
        let p = lv.plan.get(&r.key).ok_or_else(|| format!("{} is no room of the plan", py::repr(&r.key)))?;
        // the level's moods, then the low road, then the vents: the later ones win
        let mut want = lv.moods.iter().find(|(k, _)| *k == r.key).map_or(kit.target("floor"), |(_, v)| *v as f64);
        if p.scheme == "low" {
            want = set.low_floor;
        }
        if p.kind == "vent" {
            want = set.vent_floor;
        }
        targets.push(want);
        tints.push(if p.tint.is_empty() { None } else { Some(kit.tint(p.tint)?) });
    }
    let scene = Scene::new(rooms, kit);
    let gains = scene.bake(rooms, &lights, &targets, &tints, set);
    let reader = Reader::new();
    // after baking: only the lit faces' corners change
    for (b, l) in lit.iter().enumerate() {
        if let Some(room) = rooms.iter_mut().find(|r| r.key == l.room) {
            add_beam(room, b, &l.beam, BEAM_STRENGTH, &reader);
        }
    }
    // Complex's red warning lights, at a bend
    for (key, (x, y)) in &lv.glows {
        if let Some(at) = rooms.iter().position(|r| r.key == *key) {
            scene.glow(&mut rooms[at], at, (*x as f64, *y as f64), set.glow, set.glow_radius, &reader);
        }
    }
    // a kit's light panels glow: full white (Felicity's braces, 254)
    if let Ok(m) = kit.index("lamp") {
        for f in rooms.iter_mut().flat_map(|r| r.faces.iter_mut()).filter(|f| f.material == m) {
            f.colors.fill([254; 3]);
        }
    }
    let mut out = Vec::with_capacity(rooms.len());
    for p in &lv.plan.rooms {
        if let Some(at) = rooms.iter().position(|r| r.key == p.key) {
            out.push((rooms[at].name.clone(), gains[at]));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ colours

/// What the Python reads back for a stored byte: Blender turns the byte into a linear float (a
/// table of single-precision powers), and `_linear_to_srgb` x 255 turns that into a number a
/// hair off the byte.
pub struct Reader([f64; 256]);

impl Reader {
    pub fn new() -> Reader {
        Reader(std::array::from_fn(|b| {
            let c = b as f32 * (1.0f32 / 255.0);
            let linear = if c < 0.04045 { c * (1.0f32 / 12.92) } else { ((c + 0.055) * (1.0f32 / 1.055)).powf(2.4) } as f64;
            (if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 }) * 255.0
        }))
    }

    pub fn read(&self, byte: u8) -> f64 {
        self.0[byte as usize]
    }
}

impl Default for Reader {
    fn default() -> Reader {
        Reader::new()
    }
}

/// A brightness as it is stored: clamped, and the nearest byte.
fn byte(v: f64) -> u8 {
    (v.clamp(0.0, 255.0) + 0.5).floor() as u8
}

// -------------------------------------------------------------------- scene

/// What the bake knows of a room's faces: Blender's normal for each (from its corners, in
/// single precision), and for floors and ramps their weight in the room's median.
struct Shape {
    normals: Vec<F3>,
    /// The face's area over its corners, for faces that count as floor (normal z over 0.8).
    floor: Vec<Option<f64>>,
}

impl Shape {
    fn new(room: &Room) -> Shape {
        let mut normals = Vec::with_capacity(room.faces.len());
        let mut floor = Vec::with_capacity(room.faces.len());
        for f in &room.faces {
            // Newell's sum, in whole numbers: its direction is the normal, half its length the area
            let mut n = [0i64; 3];
            for (i, &a) in f.verts.iter().enumerate() {
                let (p, q) = (room.verts[a as usize].map(i64::from), room.verts[f.verts[(i + 1) % f.verts.len()] as usize].map(i64::from));
                n[0] += (p[1] - q[1]) * (p[2] + q[2]);
                n[1] += (p[2] - q[2]) * (p[0] + q[0]);
                n[2] += (p[0] - q[0]) * (p[1] + q[1]);
            }
            let len = ((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) as f64).sqrt();
            // a face of no area faces up, as Blender has it
            let unit: F3 = if len > 0.0 { n.map(|c| (c as f64 / len) as f32) } else { [0.0, 0.0, 1.0] };
            let area = (len * 0.5) as f32;
            floor.push((unit[2] as f64 > 0.8).then(|| area as f64 / f.verts.len() as f64));
            normals.push(trace::normalized(unit));
        }
        Shape { normals, floor }
    }
}

/// What the lamps give one corner: `base` with no lamp, and `gain x colour x k` more for each
/// (lamp, k) of `terms`.
struct Corner {
    base: f64,
    terms: Vec<(u32, f64)>,
}

/// A room's corners as the Python's cache has them: one for each (vertex, normal to three places).
struct Traced {
    /// For each corner of each face, in order: its place in `corners`.
    slot: Vec<u32>,
    corners: Vec<Corner>,
}

/// The level as the bake sees it: what blocks light, and each room's faces.
pub struct Scene {
    bvh: Bvh,
    shapes: Vec<Shape>,
}

impl Scene {
    /// Every polygon of the rooms blocks light, except those of Transparent materials (`kit`'s).
    pub fn new(rooms: &[Room], kit: &Kit) -> Scene {
        let mut tris = Vec::new();
        for r in rooms {
            for f in &r.faces {
                if kit.palette[f.material].flags.contains(&"Transparent") {
                    continue;
                }
                let at = |i: u32| r.verts[f.verts[i as usize] as usize].map(|c| c as f32);
                tris.extend(f.tris.iter().map(|t| [at(t[0]), at(t[1]), at(t[2])]));
            }
        }
        Scene { bvh: Bvh::new(&tris), shapes: rooms.iter().map(Shape::new).collect() }
    }

    /// How many triangles block light.
    pub fn occluders(&self) -> usize {
        self.bvh.len()
    }

    /// `complex.bake`: write every corner's colour, each room's lamps rescaled until its floor
    /// median is within 4 of its target (`targets`, one for each room), then its tint (`tints`).
    /// Rooms that do not respond (no floor, or one no light reaches) get the median gain of the
    /// others. Returns the gains, to three places. `rooms` must be the rooms the scene was made of.
    pub fn bake(&self, rooms: &mut [Room], lights: &[Light], targets: &[f64], tints: &[Option<[f64; 3]>], set: &Settings) -> Vec<f64> {
        let traced = self.trace(rooms, lights, set);
        let reader = Reader::new();
        let n = rooms.len();
        let mut gains = vec![1.0f64; n];
        let run = |rooms: &mut [Room], gains: &[f64], which: &[bool]| {
            for i in (0..n).filter(|&i| which[i]) {
                paint(&mut rooms[i], &traced[i], lights, gains[i], [1.0; 3]);
            }
        };
        let mut todo = vec![true; n];
        for _ in 0..PASSES {
            run(rooms, &gains, &todo);
            todo = vec![false; n];
            for i in 0..n {
                if let Some(med) = self.floor_median(&rooms[i], i, &reader) {
                    if (med - targets[i]).abs() >= CLOSE {
                        let step = (targets[i] - set.ambient) / f64::max(1.0, med - set.ambient);
                        gains[i] *= step.clamp(1.0 / 3.0, 3.0);
                        todo[i] = true;
                    }
                }
            }
            if !todo.contains(&true) {
                break;
            }
        }
        run(rooms, &gains, &todo);
        let loose: Vec<bool> = (0..n).map(|i| self.floor_median(&rooms[i], i, &reader).is_none() || !(0.1..=10.0).contains(&gains[i])).collect();
        let mut good: Vec<f64> = (0..n).filter(|&i| !loose[i]).map(|i| gains[i]).collect();
        good.sort_by(|a, b| a.total_cmp(b));
        for i in (0..n).filter(|&i| loose[i]) {
            gains[i] = if good.is_empty() { 1.0 } else { good[good.len() / 2] };
        }
        run(rooms, &gains, &loose);
        // tint last, so calibration sees the untinted light
        for i in 0..n {
            if let Some(tint) = tints[i] {
                paint(&mut rooms[i], &traced[i], lights, gains[i], tint);
            }
        }
        gains.iter().map(|g| py::round_to(*g, 3)).collect()
    }

    /// Cast every ray: what each lamp gives each corner of each room, whatever the room's gain.
    fn trace(&self, rooms: &[Room], lights: &[Light], set: &Settings) -> Vec<Traced> {
        // the corners to work out, room after room, each the first face's that has its vertex and normal
        let mut jobs: Vec<(F3, F3)> = Vec::new();
        let mut slots: Vec<(usize, Vec<u32>)> = Vec::with_capacity(rooms.len());
        for (room, shape) in rooms.iter().zip(&self.shapes) {
            let first = jobs.len();
            let mut seen: HashMap<(u32, [i32; 3]), u32> = HashMap::new();
            let mut slot = Vec::new();
            for (f, n) in room.faces.iter().zip(&shape.normals) {
                let key = n.map(|c| (c as f64 * 1000.0).round_ties_even() as i32);
                for &v in &f.verts {
                    let next = (jobs.len() - first) as u32;
                    let at = *seen.entry((v, key)).or_insert(next);
                    if at == next {
                        jobs.push((room.verts[v as usize].map(|c| c as f32), *n));
                    }
                    slot.push(at);
                }
            }
            slots.push((first, slot));
        }
        let threads = if set.threads == 0 { std::thread::available_parallelism().map_or(1, |n| n.get()) } else { set.threads };
        let mut done = each(jobs.len(), threads, |i| self.corner(jobs[i].0, jobs[i].1, lights, set)).into_iter();
        let ends = slots.iter().skip(1).map(|s| s.0).chain([jobs.len()]).collect::<Vec<_>>();
        slots.into_iter().zip(ends).map(|((first, slot), end)| Traced { slot, corners: done.by_ref().take(end - first).collect() }).collect()
    }

    /// `bake_lighting`'s inner loop for one corner at `p` on a face whose normal is `n`.
    fn corner(&self, p: F3, n: F3, lights: &[Light], set: &Settings) -> Corner {
        let bias = BIAS as f32;
        let o = trace::add(p, trace::scale(n, bias));
        let base = set.ambient + set.bounce * f64::max(0.0, -(n[2] as f64));
        let mut terms = Vec::new();
        for (i, l) in lights.iter().enumerate() {
            let at = trace::f3(l.pos);
            let d = trace::sub(at, p);
            let dist = trace::length(d);
            if dist >= l.radius || dist < 1e-6 {
                continue;
            }
            let mut ndl = trace::dot(n, d) / dist;
            if ndl <= 0.0 {
                continue;
            }
            if l.spot > 0.0 {
                // d points up at the lamp
                let down = d[2] as f64 / dist;
                if down <= 0.0 {
                    continue;
                }
                ndl *= down.powf(l.spot);
            }
            let dirs = if l.size > 0.0 { &SAMPLES[..] } else { &SAMPLES[..1] };
            let mut seen = 0;
            for sd in dirs {
                let r = trace::sub(trace::add(at, trace::scale(*sd, l.size as f32)), o);
                let rl = trace::length(r);
                if rl <= 2.0 * BIAS {
                    seen += 1;
                    continue;
                }
                let rd = trace::over(r, rl);
                if !self.bvh.hit(trace::add(o, trace::scale(rd, bias)), rd, (rl - 2.0 * BIAS) as f32) {
                    seen += 1;
                }
            }
            if seen == 0 {
                continue;
            }
            let fall = 1.0 - dist / l.radius;
            terms.push((i as u32, ndl * (fall * fall) * (seen as f64 / dirs.len() as f64)));
        }
        Corner { base, terms }
    }

    /// `_floor_median`: the area-weighted median of the red the Python reads at the corners of the
    /// room's floors and ramps. None for a room with none.
    fn floor_median(&self, room: &Room, at: usize, reader: &Reader) -> Option<f64> {
        let mut vals: Vec<(f64, f64)> = Vec::new();
        for (f, w) in room.faces.iter().zip(&self.shapes[at].floor) {
            if let Some(w) = w {
                vals.extend(f.colors.iter().map(|c| (reader.read(c[0]), *w)));
            }
        }
        if vals.is_empty() {
            return None;
        }
        vals.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let half = pysum(vals.iter().map(|v| (v.1, false))) / 2.0;
        let mut acc = 0.0;
        for (v, w) in &vals {
            acc += w;
            if acc >= half {
                return Some(*v);
            }
        }
        None
    }

    /// `build.glow`: the corners of `room` within `radius` (seen from above) of the vertical line
    /// at `xy` are drawn towards `color`, wholly on the line and not at all at `radius`. Corners
    /// that can't see the line at their own height are left alone. Returns how many changed.
    fn glow(&self, room: &mut Room, at: usize, xy: (f64, f64), color: [f64; 3], radius: f64, reader: &Reader) -> usize {
        let mut n = 0;
        for (fi, nrm) in self.shapes[at].normals.iter().enumerate() {
            let f = &mut room.faces[fi];
            for (i, &v) in f.verts.iter().enumerate() {
                let p = room.verts[v as usize].map(|c| c as f32);
                let d = csg::hypot(p[0] as f64 - xy.0, p[1] as f64 - xy.1);
                if d >= radius {
                    continue;
                }
                if d > 2.0 {
                    let o = trace::add(p, trace::scale(*nrm, 1.5));
                    let r = trace::sub([xy.0 as f32, xy.1 as f32, p[2]], o);
                    if self.bvh.hit(o, trace::normalized(r), f64::max(0.0, trace::length(r) - 2.0) as f32) {
                        continue;
                    }
                }
                let w = 1.0 - d / radius;
                f.colors[i] = std::array::from_fn(|k| byte(reader.read(f.colors[i][k]) * (1.0 - w) + color[k] * w));
                n += 1;
            }
        }
        n
    }
}

/// Write a room's colours from what was traced, with its lamps at `gain`.
fn paint(room: &mut Room, traced: &Traced, lights: &[Light], gain: f64, tint: [f64; 3]) {
    let colors: Vec<[u8; 3]> = traced
        .corners
        .iter()
        .map(|c| {
            let mut sum = [c.base; 3];
            for &(l, k) in &c.terms {
                for (s, col) in sum.iter_mut().zip(lights[l as usize].color) {
                    *s += col * gain * k;
                }
            }
            std::array::from_fn(|k| byte(f64::min(255.0, sum[k] * tint[k])))
        })
        .collect();
    let mut slot = traced.slot.iter();
    for f in &mut room.faces {
        f.colors = f.verts.iter().map(|_| colors[*slot.next().expect("a slot for every corner") as usize]).collect();
    }
}

/// `build.add_beam`: brighten the faces of `room` that beam number `which` lights, by `strength` x
/// the beam's Lambert factor. Only those faces' corners change, so the patch keeps hard edges
/// where lit and unlit faces share a vertex. Returns the number of faces brightened.
fn add_beam(room: &mut Room, which: usize, beam: &Beam, strength: f64, reader: &Reader) -> usize {
    let d = beam.unit();
    let mut n = 0;
    for f in room.faces.iter_mut().filter(|f| f.lit == Some(which)) {
        // `sunbeam.intensity`, of the polygon's own normal
        let x = -pysum((0..3).map(|k| (f.normal[k] * d[k], false)));
        let add = strength * if x > 0.0 { x } else { 0.0 };
        for c in &mut f.colors {
            *c = c.map(|b| byte(reader.read(b) + add));
        }
        n += 1;
    }
    n
}

/// `job(0..n)` on `threads` threads, the answers in order whatever the threads do.
fn each<T: Send>(n: usize, threads: usize, job: impl Fn(usize) -> T + Sync) -> Vec<T> {
    const CHUNK: usize = 64;
    let threads = threads.clamp(1, n.div_ceil(CHUNK).max(1));
    if threads == 1 {
        return (0..n).map(job).collect();
    }
    let next = AtomicUsize::new(0);
    let mut parts: Vec<(usize, Vec<T>)> = std::thread::scope(|s| {
        let hands: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let at = next.fetch_add(CHUNK, Ordering::Relaxed);
                        if at >= n {
                            break mine;
                        }
                        mine.push((at, (at..(at + CHUNK).min(n)).map(&job).collect::<Vec<T>>()));
                    }
                })
            })
            .collect();
        hands.into_iter().flat_map(|h| h.join().expect("a bake thread panicked")).collect()
    });
    parts.sort_by_key(|p| p.0);
    parts.into_iter().flat_map(|p| p.1).collect()
}
