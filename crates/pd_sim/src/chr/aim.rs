//! Where a chr faces and aims (`chraction.c`): `chr_get_theta`, the angles to
//! a position and the field-of-view test the trigger uses, the vertical aim
//! (`chr_calculate_aimend`, `chr_tween_aim`) and the shot direction a
//! simulant's aim makes (`chr_get_aimx_angle` / `chr_get_aimy_angle`).
//!
//! A simulant's horizontal aim *is* its facing: `holdturn` is false for bots,
//! so `aimendsideback` stays 0 and the zeroing error (`bot_update_zero_angle`)
//! is the whole accuracy model (SPIKE_PD_SIMULANTS.md, "what the decomp
//! overturned").
//!
//! Source: the old repo's `pd_spike/chraction.rs` (angles, sight, aim).

use glam::Vec3;
use pd_core::math::{atan2f, baddtor, dtor, turn, M_BADPI};

use super::thirdperson::AttackAnimConfig;
use super::Chr;

/// `D256TOR(degrees256)`: 256ths of a turn to (PD) radians.
pub fn d256tor(degrees256: u8) -> f32 {
    degrees256 as f32 * (360.0 * M_BADPI / 180.0 / 256.0)
}

impl Chr {
    /// `chr_get_theta` (`chraction.c:8883`): a simulant's `aibot->lookangle`, a
    /// player's `BADDTOR2(360 − vv_theta)` (kept in step by the world).
    pub fn theta(&self) -> f32 {
        match &self.aibot {
            Some(a) => a.lookangle,
            None => self.playertheta,
        }
    }

    /// `chr_get_rot_y` for a simulant: `aibot->roty`, the travel heading.
    pub fn roty(&self) -> f32 {
        self.aibot.as_ref().map_or(self.playertheta, |a| a.roty)
    }

    /// `chr_get_angle_to_pos` (`chraction.c:13787`): the bearing to `pos`
    /// relative to `theta`, in `[0, BADDTOR(360))`.
    pub fn chr_get_angle_to_pos(&self, pos: Vec3) -> f32 {
        let mut a = atan2f(pos.x - self.pos.x, pos.z - self.pos.z) - self.theta();
        if a < 0.0 {
            a += turn();
        }
        a
    }

    /// `chr_is_target_in_fov(chr, degrees256, false)` (`chraction.c:13979`).
    pub fn chr_is_pos_in_fov(&self, target_pos: Vec3, degrees256: u8) -> bool {
        let d256 = d256tor(degrees256);
        let angle = self.chr_get_angle_to_pos(target_pos);
        (angle < d256 && angle < dtor(180.0)) || (angle > baddtor(360.0) - d256 && angle > dtor(180.0))
    }

    /// `chr_get_aimx_angle`: for a simulant, `theta + aimsideback`.
    pub fn chr_get_aimx_angle(&self) -> f32 {
        let mut a = self.theta() + self.aimsideback;
        if a >= turn() {
            a -= turn();
        } else if a < 0.0 {
            a += turn();
        }
        a
    }

    /// `chr_get_aimy_angle`.
    pub fn chr_get_aimy_angle(&self) -> f32 {
        let mut s = self.aimuprshoulder + self.aimupback;
        if s < 0.0 {
            s += turn();
        }
        s
    }

    /// The un-spread shot direction (`chr_shoot`, `chraction.c:10035`).
    pub fn chr_shot_dir(&self) -> Vec3 {
        let roty = self.chr_get_aimx_angle();
        let rotx = self.chr_get_aimy_angle();
        Vec3::new(rotx.cos() * roty.sin(), rotx.sin(), rotx.cos() * roty.cos())
    }

    /// `chr_calculate_aimend` (`chraction.c:9071`), the aibot path: `holdturn` is
    /// false, so there is no horizontal correction (`aimendsideback = 0`). Only
    /// the vertical aim, from the chr's root to `target_pos`. A player target's
    /// `prop->pos` is its eye, so a simulant aims `eyeheight × 0.4` below it
    /// (`chraction.c:9123`: `relaimy -= eyeheight * (0.4 + 0.05 * RANDOMFRAC() *
    /// arg4)` with `arg4 = 0` from `bot_tick`; the caller draws the RANDOMFRAC).
    pub fn chr_calculate_aimend(&mut self, target_pos: Vec3, animcfg: Option<&'static AttackAnimConfig>, player_eyeheight: Option<(f32, f32)>) {
        let from = self.pos;
        let mut rel = target_pos - from;
        if let Some((eyeheight, randomfrac)) = player_eyeheight {
            let arg4 = 0.0;
            rel.y -= eyeheight * (0.4 + 0.05 * randomfrac * arg4);
        }
        let mut shootroty = rel.y.atan2((rel.x * rel.x + rel.z * rel.z).sqrt());
        if shootroty >= dtor(180.0) {
            shootroty -= turn();
        }
        let (left, right) = (self.has_weapon_in(super::HAND_LEFT), self.has_weapon_in(super::HAND_RIGHT));
        self.chr_calculate_aimend_vertical(animcfg, left, right, shootroty);
        self.aimendsideback = 0.0;
        self.aimendcount = 10;
    }

    /// `chr_calculate_aimend_vertical` (`chraction.c:9345`).
    pub fn chr_calculate_aimend_vertical(&mut self, animcfg: Option<&'static AttackAnimConfig>, hasleftgun: bool, hasrightgun: bool, shootroty: f32) {
        let mut freearmangle = 0.0;
        let mut backangle = 0.0;
        let mut gunarmangle = shootroty;
        if let Some(cfg) = animcfg {
            if shootroty > cfg.maxup_rad() {
                backangle = shootroty - cfg.maxup_rad();
                gunarmangle = cfg.maxup_rad();
            } else if shootroty < cfg.maxdown_rad() {
                backangle = shootroty - cfg.maxdown_rad();
                gunarmangle = cfg.maxdown_rad();
            }
            freearmangle = if gunarmangle > 0.0 { cfg.freearmfracup * gunarmangle } else { cfg.freearmfracdown * gunarmangle };
        }
        if hasrightgun {
            self.aimendrshoulder = gunarmangle;
            self.aimendlshoulder = if hasleftgun { gunarmangle } else { freearmangle };
        } else {
            self.aimendrshoulder = freearmangle;
            self.aimendlshoulder = gunarmangle;
        }
        self.aimendback = backangle;
    }

    /// `chr_reset_aimend` (`chraction.c:9383`).
    pub fn chr_reset_aimend(&mut self) {
        self.aimendcount = 10;
        self.aimendrshoulder = 0.0;
        self.aimendlshoulder = 0.0;
        self.aimendback = 0.0;
        self.aimendsideback = 0.0;
    }

    /// `chr_tween_aim` (`chr.c:1509`).
    pub fn chr_tween_aim(&mut self, lvupdate60f: f32, lvupdate60: i32) {
        if self.aimendcount >= 2 {
            let mult = (lvupdate60f / self.aimendcount as f32).min(1.0);
            self.aimuplshoulder += (self.aimendlshoulder - self.aimuplshoulder) * mult;
            self.aimuprshoulder += (self.aimendrshoulder - self.aimuprshoulder) * mult;
            self.aimupback += (self.aimendback - self.aimupback) * mult;
            self.aimsideback += (self.aimendsideback - self.aimsideback) * mult;
            self.aimendcount -= lvupdate60;
        } else {
            self.aimuplshoulder = self.aimendlshoulder;
            self.aimuprshoulder = self.aimendrshoulder;
            self.aimupback = self.aimendback;
            self.aimsideback = self.aimendsideback;
        }
    }
}

impl crate::world::World {
    /// `chr_has_los_to_chr` (`chraction.c:6513`): unless the target is
    /// invisible to it, a clear sight line from the chr's eye (`ground +
    /// height − 20`) to the target's `prop->pos` through the sight-blocking BG,
    /// doors, objects and path blockers that aren't see-through
    /// (`CDTYPE_OBJS | DOORS | PATHBLOCKER | BG | AIOPAQUE`, `GEOFLAG_BLOCK_SIGHT`;
    /// the two perimeters are off, and no chr blocks sight).
    pub(crate) fn chr_has_los_to_chr(&self, i: usize, target: usize) -> bool {
        if self.bot_is_target_invisible(i, target) {
            return false;
        }
        let c = &self.chrs[i];
        let eye = glam::Vec3::new(c.pos.x, c.ground + c.height - 20.0, c.pos.z);
        let to = self.chrs[target].pos;
        if !self.level.los(eye, to) {
            return false;
        }
        let types = pd_core::ids::CDTYPE_OBJS | pd_core::ids::CDTYPE_DOORS | pd_core::ids::CDTYPE_PATHBLOCKER | pd_core::ids::CDTYPE_AIOPAQUE;
        crate::stage::TileLevel::cd_los_props(eye, to, &self.obj_geos(types), &self.prop_floors(), pd_core::ids::GEOFLAG_BLOCK_SIGHT).is_none()
    }
}

#[cfg(test)]
mod tests {
    /// The trigger cone is 45/256 of a turn either side, ±63.3°, not 45°
    /// (SPIKE_PD_SIMULANTS.md).
    #[test]
    fn the_trigger_cone_is_45_256ths_of_a_turn() {
        assert!((super::d256tor(45).to_degrees() - 63.27).abs() < 0.02, "{}", super::d256tor(45).to_degrees());
    }
}
