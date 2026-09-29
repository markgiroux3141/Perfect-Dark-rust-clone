//! Riding a Slayer rocket (`VISIONMODE_SLAYERROCKET`, `player_tick`'s branch,
//! `player.c:3358`): Jo stands still (`bmove_tick(0, 0, 0, 1)`), the camera sits
//! on the rocket looking along its nose, and the controls fly it: the stick turns
//! it (pitch about its level right axis, yaw about world up), A, B, L or R slow
//! it to 1 cm a tick (else it runs up to 12), and Z blows it. Out of every room
//! for two seconds, the signal is lost.
//!
//! Keyboard and mouse follow the PC port (`pd-pcport/src/game/player.c:3620`):
//! the mouse turns it too, and the keyboard's movement keys are the stick.
//!
//! Source: the old repo's `pd_guns/sim.rs` (`slayer_control`, `slayer_camera`).

use glam::{Mat3, Vec3};
use pd_core::ids::*;
use pd_core::math::{self, Quatf};


use super::PlayerInput;
use crate::world::World;

impl World {
    /// The rider's rocket, if it still exists (`slayerrocket && rocket->base.prop`).
    fn slayer_rocket_index(&self, pi: usize) -> Option<usize> {
        let id = self.players[pi].slayerrocket?;
        self.props.objs.iter().position(|o| o.id == id && !o.is_deleting())
    }

    /// `player_tick`'s Slayer branch for player `pi`, after `bmove_tick(0, 0, 0,
    /// 1)`: fly the rocket with `input` and put the camera on it.
    pub(crate) fn player_tick_slayer(&mut self, pi: usize, input: &PlayerInput) {
        let lv = self.lv.clone();
        let lv60 = lv.lvupdate60freal;
        // player_set_camera_mode(CAMERAMODE_THIRDPERSON).
        self.players[pi].cameramode = CAMERAMODE_THIRDPERSON;
        let fire_pressed = input.fire && !self.players[pi].slayer_prevfire;
        self.players[pi].slayer_prevfire = input.fire;
        let mouse_sens = self.players[pi].mouse_sens;
        let Some(i) = self.slayer_rocket_index(pi) else {
            let p = &mut self.players[pi];
            p.slayerrocket = None;
            p.visionmode = VISIONMODE_SLAYERROCKETSTATIC;
            return;
        };
        let o = &mut self.props.objs[i];
        let sp2a8 = o.realrot.x_axis.length();
        let mut sp2b8 = Mat3::from_cols(o.realrot.x_axis / sp2a8, o.realrot.y_axis / sp2a8, o.realrot.z_axis / sp2a8);
        let rocketpos = o.pos;
        // mtx00016208(sp2b8, ...): the camera's look and up, before this tick's turn.
        let (look, up) = (sp2b8 * Vec3::Z, sp2b8 * Vec3::Y);
        // bg_find_rooms_by_pos: no room holds the rocket, nor lies under it.
        let (inrooms, aboverooms, _) = self.stage.rooms.bg_find_rooms_by_pos(rocketpos, 20);
        let outofbounds = inrooms.is_empty() && aboverooms.is_empty();
        {
            let p = &mut self.players[pi];
            if outofbounds {
                p.badrockettime += lv.lvupdate60;
                if p.badrockettime > 120 {
                    p.visionmode = VISIONMODE_SLAYERROCKETSTATIC;
                }
            } else if p.badrockettime > 0 {
                p.badrockettime = (p.badrockettime - lv.lvupdate60).max(0);
            }
        }
        if let Some(proj) = o.projectile.as_mut() {
            // Control style 1.1: Z blows it, A/B/L/R slow it, the stick steers.
            // The keyboard: fire, aim or use; the movement keys as the stick.
            let (stickx, sticky) = if input.pad { (input.look_x, input.look_y) } else { (input.walk_x * 80 / 127, input.walk_y * 80 / 127) };
            let slow = input.a_held || input.use_held || input.aim;
            let explode = fire_pressed;
            let mut sp178 = sticky as f32 * lv60 * 0.00025;
            let mut sp174 = -stickx as f32 * lv60 * 0.00025;
            if !input.pad {
                // The PC port's mouse (`inputMouseGetScaledDelta` × 0.022, pitch
                // flipped unless "forward pitch" is set).
                let s = 0.022 / 3.5 * mouse_sens;
                let mdx = (input.mouse_dx * s * 0.022).clamp(-128.0, 127.0);
                let mdy = -(input.mouse_dy * s * 0.022).clamp(-128.0, 127.0);
                sp178 += mdy;
                sp174 -= mdx;
            }
            let right = Vec3::new(sp2b8.x_axis.x, 0.0, sp2b8.x_axis.z);
            let f20 = (right.x * right.x + right.z * right.z).sqrt();
            let right = right / f20;
            let s = sp178.sin();
            let sp14c: Quatf = [sp178.cos(), right.x * s, 0.0, right.z * s];
            let s = sp174.sin();
            let sp15c: Quatf = [sp174.cos(), 0.0, if sp2b8.y_axis.y >= 0.0 { s } else { -s }, 0.0];
            let sp13c = math::quaternion_mult_quaternion(sp15c, sp14c);
            let sp1fc = math::quaternion_to_mtx(sp13c);
            proj.speed = sp1fc.transform_vector3(proj.speed);
            proj.powerlimit240 = -1;
            proj.flags |= PROJECTILEFLAG_NOTIMELIMIT;
            proj.accel = Vec3::ZERO;
            if proj.flags & PROJECTILEFLAG_LAUNCHING == 0 {
                proj.ownerprop = None;
            }
            if explode {
                // rocket->team = TEAM_00: team and timer240 share their s16.
                o.timer240 = 0;
            }
            let prevspeed = proj.speed.length();
            let targetspeed = if slow { 1.0 } else { 12.0 };
            let mut newspeed = prevspeed;
            if prevspeed < targetspeed {
                newspeed = (prevspeed + 0.05 * lv60).min(targetspeed);
            } else if prevspeed > targetspeed {
                newspeed = (prevspeed - 0.05 * lv60).max(targetspeed);
            }
            proj.speed = proj.speed * newspeed / prevspeed;
            let sp12c = math::quaternion0f097044(&glam::Mat4::from_mat3(sp2b8));
            let sp11c = math::quaternion_mult_quaternion(sp13c, sp12c);
            sp2b8 = Mat3::from_mat4(math::quaternion_to_mtx(sp11c));
            o.realrot = sp2b8 * sp2a8;
        }
        let p = &mut self.players[pi];
        p.waitforzrelease = true;
        // player_move_camera_from_pos_rooms(rocketpos, up, look, rocket's pos, its
        // rooms) (player.c:3636). `// SUBST:` the rocket's prop rooms aren't
        // tracked / from its box and the floor under it (PD's route for a
        // rocket with no rooms).
        p.player_move_camera_from_pos_rooms(rocketpos, None, &self.stage.rooms, &self.level);
        p.cam.player_allocate_matrices(rocketpos, look, up);
    }
}
