//! The player's camera (`camera.c`), whose state PD keeps on `struct player`:
//! the screen rectangle, the perspective (`c_perspfovy`, `c_perspaspect`),
//! `cam_set_scale`'s pixel scales, and the matrices `player_allocate_matrices`
//! makes each frame. The gun code reads all of it: the crosshair, shot spread
//! and the gun's aim point are screen positions turned into camera-space
//! directions ([`Camera::cam0f0b4c3c`]).
//!
//! The `vi_set_*` setters (`vi.c`) that change the camera call
//! [`Camera::cam_set_scale`] the way PD's do.

use glam::{Mat4, Vec3};
use pd_core::math::{self, dtor};

/// `SCREEN_WIDTH_LO` × `SCREEN_HEIGHT_LO` (NTSC): one full-screen player.
pub const SCREEN_W: f32 = 320.0;
pub const SCREEN_H: f32 = 220.0;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub c_screenwidth: f32,
    pub c_screenheight: f32,
    pub c_screenleft: f32,
    pub c_screentop: f32,
    pub c_halfwidth: f32,
    pub c_halfheight: f32,
    /// `c_perspfovy` (degrees) and `c_perspaspect`.
    pub c_perspfovy: f32,
    pub c_perspaspect: f32,
    pub c_scalex: f32,
    pub c_scaley: f32,
    /// This view's scale against a 60°, 240-line one: distance LODs and the
    /// Farsight's eraser use it.
    pub c_lodscalez: f32,
    /// `cam_get_projection_mtxf()` (`mtxf0068`): camera space → world.
    pub projection: Mat4,
    /// `cam_get_world_to_screen_mtxf()` (`mtxf0064`): world → camera space.
    pub world_to_screen: Mat4,
}

impl Default for Camera {
    fn default() -> Camera {
        let mut c = Camera {
            c_screenwidth: 0.0,
            c_screenheight: 0.0,
            c_screenleft: 0.0,
            c_screentop: 0.0,
            c_halfwidth: 0.0,
            c_halfheight: 0.0,
            c_perspfovy: 60.0,
            c_perspaspect: SCREEN_W / SCREEN_H,
            c_scalex: 1.0,
            c_scaley: 1.0,
            c_lodscalez: 1.0,
            projection: Mat4::IDENTITY,
            world_to_screen: Mat4::IDENTITY,
        };
        c.vi_set_fov_aspect_and_size(60.0, SCREEN_W / SCREEN_H, SCREEN_W, SCREEN_H);
        c
    }
}

impl Camera {
    /// `vi_set_fov_aspect_and_size` (`vi.c:861`).
    pub fn vi_set_fov_aspect_and_size(&mut self, fovy: f32, aspect: f32, width: f32, height: f32) {
        self.cam_set_screen_size(width, height);
        self.c_perspfovy = fovy;
        self.c_perspaspect = aspect;
        self.cam_set_scale();
    }

    /// `vi_set_fov_y` (`vi.c:847`).
    pub fn vi_set_fov_y(&mut self, fovy: f32) {
        self.c_perspfovy = fovy;
        self.cam_set_scale();
    }

    /// `cam_set_screen_size` (`camera.c:30`).
    pub fn cam_set_screen_size(&mut self, width: f32, height: f32) {
        self.c_screenwidth = width;
        self.c_screenheight = height;
        self.c_halfwidth = width * 0.5;
        self.c_halfheight = height * 0.5;
    }

    /// `cam_set_screen_position` (`camera.c:40`).
    pub fn cam_set_screen_position(&mut self, left: f32, top: f32) {
        self.c_screenleft = left;
        self.c_screentop = top;
    }

    /// `cam_set_scale` (`camera.c:69`).
    pub fn cam_set_scale(&mut self) {
        let a = self.c_perspfovy * (dtor(180.0) / 360.0);
        self.c_scaley = a.sin() / (a.cos() * self.c_halfheight);
        self.c_scalex = (self.c_scaley * self.c_perspaspect * self.c_halfheight) / self.c_halfwidth;
        let lod60 = dtor(30.0).sin() / (dtor(30.0).cos() * 120.0);
        self.c_lodscalez = self.c_scaley / lod60;
    }

    /// `cam0f0b4c3c` (`camera.c:108`): a screen position to a camera-space
    /// direction of length `len`.
    pub fn cam0f0b4c3c(&self, pos2d: [f32; 2], len: f32) -> Vec3 {
        let sp1c = (self.c_halfheight - (pos2d[1] - self.c_screentop)) * self.c_scaley;
        let sp20 = (pos2d[0] - self.c_screenleft - self.c_halfwidth) * self.c_scalex;
        let sp18 = -1.0;
        let f2 = len / (sp20 * sp20 + sp1c * sp1c + sp18 * sp18).sqrt();
        Vec3::new(sp20 * f2, sp1c * f2, sp18 * f2)
    }

    /// `player_allocate_matrices` (`player.c:4211`), the two matrices the game
    /// reads back (`mtxf0064`, `mtxf0068`).
    pub fn player_allocate_matrices(&mut self, pos: Vec3, look: Vec3, up: Vec3) {
        self.world_to_screen = math::view_matrix(pos, look, up);
        self.projection = math::look_basis(pos, look, up);
    }

    /// The eye (`cam_get_pos`).
    pub fn pos(&self) -> Vec3 {
        self.projection.w_axis.truncate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 60° view: the screen's top edge is 30° up, its centre straight ahead,
    /// and a 320 × 240 view is the reference LOD scale.
    #[test]
    fn screen_positions_turn_into_camera_directions() {
        let mut c = Camera::default();
        c.vi_set_fov_aspect_and_size(60.0, 320.0 / 240.0, 320.0, 240.0);
        assert!((c.c_lodscalez - 1.0).abs() < 1e-6);
        let centre = c.cam0f0b4c3c([160.0, 120.0], 1.0);
        assert!((centre - Vec3::NEG_Z).length() < 1e-6);
        let top = c.cam0f0b4c3c([160.0, 0.0], 1.0);
        assert!((top.y.atan2(-top.z).to_degrees() - 30.0).abs() < 1e-3, "{top}");
        // PD's own 220-line view.
        c.vi_set_fov_aspect_and_size(60.0, SCREEN_W / SCREEN_H, SCREEN_W, SCREEN_H);
        assert!((c.c_lodscalez - 240.0 / 220.0).abs() < 1e-5);
    }
}
