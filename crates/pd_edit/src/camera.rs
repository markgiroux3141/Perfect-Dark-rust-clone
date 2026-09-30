//! The fly-through camera: a position and a yaw/pitch, moved with WASD
//! (Q/E down and up, Shift faster) and turned with the mouse while the right
//! button is held. Yaw 0 looks down +z, 90° down +x (PD's facing).

use glam::{Mat4, Vec3};
use pd_render::View;

#[derive(Clone, Copy, Debug)]
pub struct FlyCam {
    pub pos: Vec3,
    /// Radians about +y (0 faces +z).
    pub yaw: f32,
    /// Radians up from level.
    pub pitch: f32,
    /// Centimetres per second.
    pub speed: f32,
}

/// What moves the camera this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Motion {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub fast: bool,
    /// Mouse travel in pixels while looking.
    pub look: (f32, f32),
}

/// Radians of turn per pixel of mouse travel.
const LOOK_RATE: f32 = 0.0035;

impl FlyCam {
    pub fn new(pos: Vec3, yaw: f32, pitch: f32) -> FlyCam {
        FlyCam { pos, yaw, pitch, speed: 800.0 }
    }

    pub fn look(&self) -> Vec3 {
        Vec3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos())
    }

    /// Right of the look, level (look × up).
    pub fn right(&self) -> Vec3 {
        Vec3::new(-self.yaw.cos(), 0.0, self.yaw.sin())
    }

    pub fn update(&mut self, m: &Motion, dt: f32) {
        self.yaw -= m.look.0 * LOOK_RATE;
        self.pitch = (self.pitch - m.look.1 * LOOK_RATE).clamp(-1.55, 1.55);
        let dir = self.look() * m.forward + self.right() * m.right + Vec3::Y * m.up;
        let speed = self.speed * if m.fast { 4.0 } else { 1.0 };
        self.pos += dir.normalize_or_zero() * speed * dt;
    }

    /// PD's view from here (its 60° field of view) over `aspect`, clipping at
    /// `znear`..`zfar`.
    pub fn view(&self, aspect: f32, znear: f32, zfar: f32) -> View {
        View { eye: self.pos, look: self.look(), up: Vec3::Y, fovy: 60.0, aspect, znear, zfar, shake: 0.0 }
    }

    /// Turn to look at `target` from `dist` back along the current look.
    pub fn frame(&mut self, target: Vec3, dist: f32) {
        self.pos = target - self.look() * dist;
    }
}

/// World → clip for `view`.
pub fn world_to_clip(view: &View) -> Mat4 {
    view.projection() * view.world_to_eye()
}

/// A world point in pixels of a `w` × `h` view (origin top left), or none
/// behind the eye.
pub fn project(clip: &Mat4, p: Vec3, w: f32, h: f32) -> Option<(f32, f32, f32)> {
    let c = *clip * p.extend(1.0);
    if c.w <= 1.0 {
        return None;
    }
    let (x, y) = (c.x / c.w, c.y / c.w);
    Some(((x * 0.5 + 0.5) * w, (0.5 - y * 0.5) * h, c.w))
}

/// The ray through pixel (`x`, `y`) of a `w` × `h` view: its origin (the
/// eye) and unit direction.
pub fn ray(view: &View, x: f32, y: f32, w: f32, h: f32) -> (Vec3, Vec3) {
    let inv = world_to_clip(view).inverse();
    let (nx, ny) = (x / w * 2.0 - 1.0, 1.0 - y / h * 2.0);
    let far = inv.project_point3(Vec3::new(nx, ny, 1.0));
    (view.eye, (far - view.eye).normalize_or_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_is_pds_facing_and_a_centre_ray_is_the_look() {
        let c = FlyCam::new(Vec3::ZERO, std::f32::consts::FRAC_PI_2, 0.0);
        assert!((c.look() - Vec3::X).length() < 1e-5, "yaw 90° faces +x");
        assert!((c.right() - Vec3::Z).length() < 1e-5, "looking down +x, +z is on the right");
        let v = c.view(1.5, 15.0, 10000.0);
        let (o, d) = ray(&v, 300.0, 200.0, 600.0, 400.0);
        assert_eq!(o, Vec3::ZERO);
        assert!((d - Vec3::X).length() < 1e-4, "{d}");
        // A point ahead projects to the centre; one to the right, right of it.
        let clip = world_to_clip(&v);
        let (x, y, _) = project(&clip, Vec3::new(500.0, 0.0, 0.0), 600.0, 400.0).unwrap();
        assert!((x - 300.0).abs() < 1e-2 && (y - 200.0).abs() < 1e-2);
        let (x, _, _) = project(&clip, Vec3::new(500.0, 0.0, 100.0), 600.0, 400.0).unwrap();
        assert!(x > 300.0);
        assert!(project(&clip, Vec3::new(-500.0, 0.0, 0.0), 600.0, 400.0).is_none());
    }
}
