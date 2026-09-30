//! The move / rotate gizmo, after the old repo's prop gizmo
//! (`world/tools/prop_gizmo.rs`): Move is three arrows (x red, y green, z
//! blue) and a drag slides the selection along the arrow grabbed, following
//! the mouse ray; Rotate is a yellow ring round the vertical and a drag turns
//! the facing. Drawn on top of everything, sized by its distance so it stays
//! the same size on screen. Ctrl snaps (25 cm, 15°).

use glam::Vec3;

use crate::draw3d::Mesh3d;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Move,
    Rotate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// Along x (0), y (1) or z (2).
    Axis(usize),
    Ring,
}

pub const MOVE_SNAP: f32 = 25.0;
pub const ROTATE_SNAP: f32 = 15.0;

const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
const COLOURS: [[f32; 4]; 3] = [[0.93, 0.2, 0.2, 1.0], [0.25, 0.9, 0.25, 1.0], [0.25, 0.4, 1.0, 1.0]];
const YELLOW: [f32; 4] = [0.95, 0.85, 0.2, 1.0];

/// The gizmo's size (arrow length) at `origin` seen from `eye`.
pub fn size(eye: Vec3, origin: Vec3) -> f32 {
    (eye.distance(origin) * 0.2).max(15.0)
}

/// Each arrow's box (shaft and head together, for picking).
fn arrow_box(origin: Vec3, axis: usize, s: f32) -> (Vec3, Vec3) {
    let h = Vec3::splat(s * 0.06);
    let tip = origin + AXES[axis] * s;
    ((origin - h).min(tip - h), (origin + h).max(tip + h))
}

/// The ray `o + t d`'s entry into the box, if it meets it.
pub fn ray_aabb(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-9 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

/// Where along the line `p0 + t a` (unit `a`) the ray `o + s d` passes
/// closest. None when they are parallel.
pub fn closest_t_on_axis(p0: Vec3, a: Vec3, o: Vec3, d: Vec3) -> Option<f32> {
    let w0 = p0 - o;
    let b = a.dot(d);
    let c = d.dot(d);
    let denom = c - b * b;
    (denom.abs() > 1e-6).then(|| (b * d.dot(w0) - c * a.dot(w0)) / denom)
}

/// Where the ray meets the level plane through `y`, ahead of its origin.
pub fn ray_plane_y(y: f32, o: Vec3, d: Vec3) -> Option<Vec3> {
    if d.y.abs() < 1e-6 {
        return None;
    }
    let t = (y - o.y) / d.y;
    (t >= 0.0).then(|| o + d * t)
}

/// The handle under the ray: the nearest arrow in Move, the ring in Rotate.
pub fn pick(mode: Mode, origin: Vec3, s: f32, o: Vec3, d: Vec3) -> Option<Handle> {
    match mode {
        Mode::Move => (0..3)
            .filter_map(|k| {
                let (lo, hi) = arrow_box(origin, k, s);
                let pad = Vec3::splat(s * 0.04);
                ray_aabb(o, d, lo - pad, hi + pad).map(|t| (k, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| Handle::Axis(k)),
        Mode::Rotate => {
            let p = ray_plane_y(origin.y, o, d)?;
            let r = (p - origin).with_y(0.0).length();
            ((r - s * 0.9).abs() <= s * 0.22).then_some(Handle::Ring)
        }
    }
}

/// A facing in degrees (PD's `atan2(x, z)`) toward `p` from `origin`.
pub fn facing_to(origin: Vec3, p: Vec3) -> f32 {
    let d = p - origin;
    d.x.atan2(d.z).to_degrees()
}

/// A drag in progress: what it started from.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pub handle: Handle,
    pub origin: Vec3,
    /// The axis parameter, or the facing toward the grab point, at the start.
    pub grab: f32,
    pub start_facing: f32,
}

impl Drag {
    /// Start dragging `handle` of a gizmo at `origin` under the ray.
    pub fn start(handle: Handle, origin: Vec3, facing: f32, o: Vec3, d: Vec3) -> Option<Drag> {
        let grab = match handle {
            Handle::Axis(k) => closest_t_on_axis(origin, AXES[k], o, d)?,
            Handle::Ring => facing_to(origin, ray_plane_y(origin.y, o, d)?),
        };
        Some(Drag { handle, origin, grab, start_facing: facing })
    }

    /// Where a Move drag puts the selection for the ray now (`snap`: that
    /// axis's coordinate to the grid).
    pub fn moved(&self, o: Vec3, d: Vec3, snap: bool) -> Option<Vec3> {
        let Handle::Axis(k) = self.handle else { return None };
        let t = closest_t_on_axis(self.origin, AXES[k], o, d)?;
        let mut p = self.origin + AXES[k] * (t - self.grab);
        if snap {
            p[k] = (p[k] / MOVE_SNAP).round() * MOVE_SNAP;
        }
        Some(p)
    }

    /// The facing a Rotate drag gives for the ray now (degrees, 0..360).
    pub fn turned(&self, o: Vec3, d: Vec3, snap: bool) -> Option<f32> {
        let p = ray_plane_y(self.origin.y, o, d)?;
        let mut f = self.start_facing + facing_to(self.origin, p) - self.grab;
        if snap {
            f = (f / ROTATE_SNAP).round() * ROTATE_SNAP;
        }
        Some(f.rem_euclid(360.0))
    }
}

/// Draw the gizmo (on top) at `origin`: `hot` is the handle hovered or
/// dragged; in Rotate, `facing` (degrees) is drawn as a spoke.
pub fn draw(mesh: &mut Mesh3d, mode: Mode, origin: Vec3, s: f32, hot: Option<Handle>, facing: Option<f32>) {
    let bright = |c: [f32; 4]| [(c[0] * 1.4 + 0.15).min(1.0), (c[1] * 1.4 + 0.15).min(1.0), (c[2] * 1.4 + 0.15).min(1.0), 1.0];
    match mode {
        Mode::Move => {
            for k in 0..3 {
                let col = if hot == Some(Handle::Axis(k)) { bright(COLOURS[k]) } else { COLOURS[k] };
                let a = AXES[k];
                let (w, hw) = (Vec3::splat(s * 0.025), Vec3::splat(s * 0.07));
                let shaft_end = origin + a * s * 0.78;
                mesh.cube(true, (origin - w).min(shaft_end - w), (origin + w).max(shaft_end + w), col);
                let (h0, h1) = (origin + a * s * 0.78, origin + a * s);
                mesh.cube(true, (h0 - hw).min(h1 - hw), (h0 + hw).max(h1 + hw), col);
            }
            let c = Vec3::splat(s * 0.05);
            mesh.cube(true, origin - c, origin + c, [0.9, 0.9, 0.9, 1.0]);
        }
        Mode::Rotate => {
            let col = if hot == Some(Handle::Ring) { bright(YELLOW) } else { YELLOW };
            mesh.ring(true, origin, s * 0.84, s * 0.96, col);
            if let Some(f) = facing {
                let dir = pd_import::layout::look(f);
                mesh.line(true, origin, origin + dir * s * 0.9, col);
                let tip = origin + dir * s * 0.9;
                let c = Vec3::splat(s * 0.05);
                mesh.cube(true, tip - c, tip + c, col);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_drag_follows_the_ray_along_its_axis() {
        let origin = Vec3::new(0.0, 100.0, 0.0);
        let eye = Vec3::new(0.0, 300.0, -600.0);
        let s = size(eye, origin);
        // Aim at the x arrow's middle: its handle.
        let aim = |p: Vec3| (eye, (p - eye).normalize());
        let (o, d) = aim(origin + Vec3::X * s * 0.5);
        assert_eq!(pick(Mode::Move, origin, s, o, d), Some(Handle::Axis(0)));
        let drag = Drag::start(Handle::Axis(0), origin, 0.0, o, d).unwrap();
        // Move the mouse to aim 100 cm further along x: it moves 100 cm in x only.
        let (o, d) = aim(origin + Vec3::X * (s * 0.5 + 100.0));
        let p = drag.moved(o, d, false).unwrap();
        assert!((p - (origin + Vec3::X * 100.0)).length() < 0.5, "{p}");
        assert_eq!(drag.moved(o, d, true).unwrap().x, 100.0, "snapped to 25 cm");
    }

    #[test]
    fn a_rotate_drag_turns_by_the_angle_swept() {
        let origin = Vec3::ZERO;
        let eye = Vec3::new(0.0, 500.0, 0.0);
        let s = 100.0;
        let down = |x: f32, z: f32| (eye, (Vec3::new(x, 0.0, z) - eye).normalize());
        // Grab the ring at +z (facing 0), sweep to +x (facing 90).
        let (o, d) = down(0.0, 90.0);
        assert_eq!(pick(Mode::Rotate, origin, s, o, d), Some(Handle::Ring));
        assert_eq!(pick(Mode::Rotate, origin, s, down(0.0, 20.0).0, down(0.0, 20.0).1), None, "inside the ring");
        let drag = Drag::start(Handle::Ring, origin, 30.0, o, d).unwrap();
        let (o, d) = down(90.0, 0.0);
        assert!((drag.turned(o, d, false).unwrap() - 120.0).abs() < 0.01);
    }
}
