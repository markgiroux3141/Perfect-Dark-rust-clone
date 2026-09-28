//! Per-player view: the camera, fov and aspect PD sets up each frame
//! (`player_tick`, `player.c:3181`: `vi_set_fov_aspect_and_size(60, aspect,
//! viewport width, height)`), the stage's z range (`vi_set_z_range(env->near,
//! env->far)`, `env.c:243`), and the world → eye → clip matrices.
//!
//! Later: the viewport of a split-screen quarter (M12), and the world-to-screen
//! maths the crosshair and HUD use (M4).

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
    /// Vertical field of view in degrees (`zoominfovy`).
    pub fovy: f32,
    /// Width / height as PD computes it: `player_get_aspect_ratio`
    /// (`player.c:2999`), viewport width / height × the VI mode's `yscale`.
    pub aspect: f32,
    /// The stage's z range, cm.
    pub znear: f32,
    pub zfar: f32,
}

impl View {
    /// A player's view of the stage, full screen, at PD's 320 × 220 aspect.
    pub fn for_player(p: &Player, znear: f32, zfar: f32) -> View {
        View { eye: p.pos, look: p.look, up: p.up, fovy: p.zoominfovy, aspect: VIEW_W as f32 / VIEW_H as f32, znear, zfar }
    }

    /// World → eye (`player_allocate_matrices`' `mtxf0064`, `mtx00016874`).
    pub fn world_to_eye(&self) -> Mat4 {
        math::view_matrix(self.eye, self.look, self.up)
    }

    /// Eye → clip: `guPerspective(fovy, aspect, near, far)` for wgpu's 0..1 depth.
    /// PD's eye space looks down −z (`mtx00016b58` puts the negated look in
    /// column 2), as a right-handed perspective does.
    pub fn projection(&self) -> Mat4 {
        Mat4::perspective_rh(self.fovy.to_radians(), self.aspect, self.znear, self.zfar)
    }
}
