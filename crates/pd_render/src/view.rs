//! Per-player view: the camera, fov and aspect PD sets up each frame
//! (`player_tick`, `player.c:3181`: `vi_set_fov_aspect_and_size(60, aspect,
//! viewport width, height)`, then the zoom's `vi_set_fov_y`), the stage's z
//! range (`vi_set_z_range(env->near, env->far)`, `env.c:243`), and the world →
//! eye → clip matrices.
//!
//! Later: the viewport of a split-screen quarter (M12).

use glam::{Mat4, Vec3};
use pd_core::math;
use pd_sim::player::Player;

/// PD's framebuffer for one player: `SCREEN_WIDTH_LO` × `SCREEN_HEIGHT_LO` (NTSC).
pub const VIEW_W: u32 = 320;
pub const VIEW_H: u32 = 220;

#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The eye (`prop->pos`), and the camera basis (`bond2.look`, `bond2.up`).
    pub eye: Vec3,
    pub look: Vec3,
    pub up: Vec3,
    /// Vertical field of view in degrees (`c_perspfovy`).
    pub fovy: f32,
    /// Width / height as PD computes it: `player_get_aspect_ratio`
    /// (`player.c:2999`), viewport width / height × the VI mode's `yscale`.
    pub aspect: f32,
    /// The stage's z range, cm.
    pub znear: f32,
    pub zfar: f32,
    /// `vi_shake`'s vertical offset of the whole picture, in clip units.
    pub shake: f32,
}

impl View {
    /// A player's view of the stage, from the camera `player_tick` set up.
    pub fn for_player(p: &Player, znear: f32, zfar: f32) -> View {
        View { eye: p.pos, look: p.look, up: p.up, fovy: p.cam.c_perspfovy, aspect: p.cam.c_perspaspect, znear, zfar, shake: 0.0 }
    }

    /// With `vi_shake`'s offset (half-lines of the 240-line picture).
    pub fn with_shake(mut self, offset: f32) -> View {
        self.shake = -offset / 240.0;
        self
    }

    /// World → eye (`player_allocate_matrices`' `mtxf0064`, `mtx00016874`).
    pub fn world_to_eye(&self) -> Mat4 {
        math::view_matrix(self.eye, self.look, self.up)
    }

    fn shaken(&self, proj: Mat4) -> Mat4 {
        Mat4::from_translation(Vec3::new(0.0, self.shake, 0.0)) * proj
    }

    /// Eye → clip: `guPerspective(fovy, aspect, near, far)` for wgpu's 0..1 depth.
    /// PD's eye space looks down −z (`mtx00016b58` puts the negated look in
    /// column 2), as a right-handed perspective does.
    pub fn projection(&self) -> Mat4 {
        self.shaken(Mat4::perspective_rh(self.fovy.to_radians(), self.aspect, self.znear, self.zfar))
    }

    /// The gun's eye → clip: `bgun_render`'s `vi0000aca4(1.5, 1000)`, the same
    /// field of view over its own z range.
    pub fn gun_projection(&self) -> Mat4 {
        self.shaken(Mat4::perspective_rh(self.fovy.to_radians(), self.aspect, 1.5, 1000.0))
    }
}
