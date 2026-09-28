//! Chrs, player and simulant alike: the `chrdata` subset a Combat Simulator
//! chr uses, and what PD does to one each tick.
//!
//! * [`body`]: the body and head (`pd_core::model`), posed in the sim with
//!   `chr_handle_joint_positioned`, the held guns hung on the hands, the gun
//!   positions a shot starts from (`chr_get_gun_pos`), the on-screen test
//!   (`pos_is_onscreen`) and the part-box hit test (`chr_test_hit`);
//! * [`thirdperson`]: `player_choose_third_person_animation`, the only thing
//!   that animates a simulant's body;
//! * `position`: `chr_update_position` (the model's `unk70` callback),
//!   `chr_calculate_push_pos`, falls, ladders, the duck/squat heights;
//! * `gopos`: `chr_go_to_room_pos`, `chr_tick_gopos`, `chr_run_from_pos`;
//! * `damage`: `chr_damage` (both the simulant's and the player's branch, with
//!   hit parts), flinches, grunts, `chr_die`, the death and the fade;
//! * `shoot`: `chr_shoot` for a simulant, its fire slots (the shot sound and
//!   tracer), punches;
//! * [`aim`]: the angles, the trigger cone, sight (`chr_has_los_to_chr`) and
//!   the vertical aim (`chr_calculate_aimend`);
//! * `tick`: `chr_tick` and the footsteps.
//!
//! The world keeps every chr in one list in PD's `g_MpAllChrPtrs` order: the
//! human players' chrs first (their `Chr` is kept in step with their `Player`,
//! as PD's player prop is), then the simulants in the order they were
//! allocated (`botmgr_allocate_bot`, `setup.c:1974`). A chr refers to another by
//! its index in that list.
//!
//! Sources: the old repo's `pd_spike/chr.rs`, `chraction.rs`, `thirdperson.rs`,
//! `gunpos.rs`, `view.rs`, and `pd_complex/fight.rs`.

mod alloc;
pub mod aim;
pub mod body;
mod damage;
mod gopos;
mod position;
mod shoot;
mod spawn;
pub mod thirdperson;
mod tick;

pub use alloc::{botmgr_allocate_bots, mp_chr_body_head, player_chr};
pub use body::Held;
pub use damage::{DamageFrom, GruntNext, HITPART_GENERAL};
pub use gopos::{pos_is_arriving_at_pos, pos_is_arriving_laterally_at_pos, NavStats};
pub use position::projectile_update_fall;
pub use spawn::*;

use glam::Vec3;
use pd_core::anim::Anim;
use pd_core::model::Model;

use crate::bot::Aibot;
use crate::fx::beam::Beam;
use crate::stage::PerimCyl;

pub use pd_core::ids::{HAND_LEFT, HAND_RIGHT};

/// `chr->actiontype`, the values a Combat Simulator chr passes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// `ACT_STAND`.
    Stand,
    /// `ACT_GOPOS`.
    GoPos,
    /// `ACT_DIE`: the death animation is playing.
    Die,
    /// `ACT_DEAD`: lying still, fading out.
    Dead,
    /// `ACT_BONDMULTI`: a human player's chr (moved by the player code).
    BondMulti,
}

/// `chr->act_gopos`.
#[derive(Clone, Debug, Default)]
pub struct GoPos {
    pub endpos: Vec3,
    /// `act_gopos.waypoints[MAX_CHRWAYPOINTS]` up to its NULL: the loaded part of
    /// the route (at most 5). `chr_gopos_advance_waypoint` reloads it from the
    /// current waypoint once `curindex` passes 3.
    pub waypoints: Vec<usize>,
    pub curindex: usize,
    /// `act_gopos.target`: the route's last waypoint.
    pub target: Option<usize>,
    /// `GOPOSFLAG_INIT`.
    pub init: bool,
    /// `GOPOSFLAG_CROUCH` / `GOPOSFLAG_DUCK`, set on the way to a pad flagged
    /// `PADFLAG_AICROUCH` / `AIDUCK` and kept for the rest of the go-to.
    pub crouch: bool,
    pub duck: bool,
    /// `act_gopos.waydata.age`.
    pub age: i32,
    /// `act_gopos.restartttl`: when the current leg is given up and re-routed.
    pub restartttl: u16,
}

/// A fire slot (`struct fireslot`, `g_Fireslots`): when the gun's shot sound
/// may play again, and the tracer. PD allocates one per hand from a pool of
/// 20 on the first shot; every chr keeps its two here.
#[derive(Clone, Debug)]
pub struct Fireslot {
    /// `endlvframe`: the looping shot sound's end (`duration60` after it started).
    pub endlvframe: i32,
    pub beam: Beam,
}

impl Default for Fireslot {
    fn default() -> Self {
        Fireslot { endlvframe: -1, beam: Beam::default() }
    }
}

/// `struct chrdata` (the Combat Simulator subset).
#[derive(Clone)]
pub struct Chr {
    /// `chr->chrnum`: a simulant's allocation order (`botmgr_allocate_bot`); a
    /// player's its player number. It seeds the chr's routes (`CHRNAVSEED`).
    pub chrnum: usize,
    /// `PROPTYPE_PLAYER`: the human player this chr belongs to.
    pub player: Option<usize>,
    pub name: String,
    /// `BODY_*` / `HEAD_*` (`g_HeadsAndBodies` rows).
    pub bodynum: usize,
    pub headnum: Option<usize>,
    pub ismale: bool,
    /// `chr->voicebox`: which set of grunts (`VOICEBOX_*`; female bodies 3).
    pub voicebox: u8,
    /// `g_HeadsAndBodies[bodynum].height` (a simulant's speed, `bot_calculate_max_speed`).
    pub bodyheight: f32,
    /// The body with its head (`chr->model`) and its `struct anim`.
    pub model: Model,
    pub anim: Anim,
    /// `weapons_held[HAND_RIGHT]`, `[HAND_LEFT]`: the guns in the hands.
    pub held: [Option<Held>; 2],

    /// `prop->pos`: the model's root, `manground` + the animation's root height.
    pub pos: Vec3,
    /// `chr->prevpos`: the root before this tick's animation (`chr_update_anim`).
    pub prevpos: Vec3,
    /// `prop->rooms`, cut to the floor's room when the chr stands in it.
    pub rooms: Vec<u16>,
    /// `chr->manground`: the smoothed height the feet are at.
    pub manground: f32,
    /// `chr->ground`: the floor found under the cylinder this tick.
    pub ground: f32,
    /// `chr->sumground`: the low-pass accumulator behind `manground` (×10).
    pub sumground: f32,
    pub fallspeed: Vec3,
    pub radius: f32,
    /// 185 standing, 135 ducking, 90 squatting (`chr_update_position`).
    pub height: f32,
    pub floorroom: Option<u16>,
    /// `FLOORTYPE_*`.
    pub floortype: u8,
    pub onladder: bool,
    /// 0 the last move was clear, 2 it slid along an obstacle, 1 it was refused.
    pub invalidmove: u8,
    pub lastmoveok60: i32,
    /// `CHRCFLAG_FORCETOGROUND`: stand straight on the floor next position update.
    pub forcetoground: bool,

    pub actiontype: Act,
    pub act_gopos: GoPos,
    /// `act_dead.fadetimer60` (-1 for none).
    pub fadetimer60: i32,
    /// `chr->fadealpha`: -1, or 255 → 0 while a corpse fades.
    pub fadealpha: f32,

    pub damage: f32,
    pub maxdamage: f32,
    /// `chr->flinchcnt` (-1 idle) and `(hidden2 >> 13) & 7`, and
    /// `CHRH2FLAG_HEADSHOTTED`.
    pub flinchcnt: i32,
    pub flinchtype: u8,
    pub headshotted: bool,
    /// `CHRH2FLAG_AUTOANIM`.
    pub autoanim: bool,

    pub aimendlshoulder: f32,
    pub aimendrshoulder: f32,
    pub aimendback: f32,
    pub aimendsideback: f32,
    pub aimuplshoulder: f32,
    pub aimuprshoulder: f32,
    pub aimupback: f32,
    pub aimsideback: f32,
    pub aimendcount: i32,

    /// `CHRHFLAG_FIRINGRIGHT` / `LEFT`: the trigger, consumed by `chr_tick_shots`.
    pub hand_firing: [bool; 2],
    pub firecount: [i32; 2],
    pub unk32c_12: u8,
    pub fireslots: [Fireslot; 2],
    /// `CHRH2FLAG_FIRESOUNDDONE`: one shot sound per chr per tick.
    pub firesounddone: bool,

    /// `chr->target` (a chr index).
    pub target: Option<usize>,
    /// `CHRHFLAG_CLOAKED`.
    pub cloaked: bool,
    /// `chr->oldframe`, `chr->lastfootsample`: the footsteps' memory.
    pub oldframe: f32,
    pub lastfootsample: i32,
    /// `PROPFLAG_ONTHISSCREENTHISTICK` for the pass that fully ticked it
    /// (player 0's): `chr_get_gun_pos` needs a drawn gun.
    pub onscreen: bool,
    /// `chr->lastshooter` / `timeshooter`: who gets a fall death.
    pub lastshooter: Option<usize>,
    pub timeshooter: i32,
    /// `chr->aibot`: a simulant's brain.
    pub aibot: Option<Box<Aibot>>,
    /// A player chr's `chr_get_theta` (`BADDTOR2(360 − vv_theta)`) and eye
    /// height, kept in step with its player by the world.
    pub playertheta: f32,
    pub eyeheight: f32,
    /// `chr->blurdrugamount`, `blurnumtimesdied`: the dizziness a punch or a
    /// tranquilizer leaves (a simulant's aim wobbles, a player's view blurs).
    pub blurdrugamount: i32,
    pub blurnumtimesdied: i32,
    /// `chr->sleep`: ticks until the action next runs (`chra_tick`).
    pub sleep: i32,
    /// A punch's shove (`chr->timeextra`, `elapseextra`, `extraspeed`):
    /// `chr_update_position` adds `extraspeed` per tick for `timeextra` ticks.
    pub timeextra: f32,
    pub elapseextra: f32,
    pub extraspeed: Vec3,
    /// The footsteps (`footstep.c`): which foot landed this tick (0 none), and
    /// the last walk's rhythm kept while asleep (`magicanim` indexes
    /// `g_FootstepAnims`).
    pub footstep: u8,
    pub magicanim: Option<usize>,
    pub magicframe: f32,
    pub magicspeed: f32,
    /// `PROPFLAG_ONANYSCREENTHISTICK` / `ONANYSCREENPREVTICK`.
    pub onanyscreen: bool,
    pub onanyscreenprev: bool,

    /// `chr->team`: `1 << mpchrconfig.team`.
    pub team: u8,
    /// `mpstats` (M7 keeps them properly): kills, deaths and suicides.
    pub kills: u32,
    pub deaths: u32,
    pub suicides: u32,
}

impl Chr {
    /// `chr_is_dead` (`chraction.c:9588`) without the player half: a player
    /// chr is dead when its player is (the world asks [`crate::world::World::chr_is_dead`]).
    pub fn is_dying_or_dead(&self) -> bool {
        matches!(self.actiontype, Act::Die | Act::Dead)
    }

    pub fn has_weapon_in(&self, hand: usize) -> bool {
        self.held[hand].is_some()
    }

    /// `aibot->angleoffset`: how far the body's animation turns it from its
    /// facing (a player chr's is 0).
    pub fn angleoffset(&self) -> f32 {
        self.aibot.as_ref().map_or(0.0, |a| a.angleoffset)
    }

    /// `chr_get_bbox` (`chr.c:4995`) relative to `prop->pos.y`, the way every
    /// caller passes it: `(ymax - prop->pos.y, ymin - prop->pos.y)`. The cylinder
    /// starts 20 cm above `manground`, so anything lower is stepped over.
    pub fn bbox_rel(&self) -> (f32, f32) {
        (self.manground + self.height - self.pos.y, self.manground + 20.0 - self.pos.y)
    }

    /// `chr_get_geometry` (`chr.c:4949`): the cylinder other chrs collide with.
    /// A dying chr's blocks shots only, and a dead one's nothing.
    pub fn perim(&self) -> Option<PerimCyl> {
        (!self.is_dying_or_dead()).then_some(PerimCyl { x: self.pos.x, z: self.pos.z, radius: self.radius, ymin: self.manground, ymax: self.manground + self.height })
    }
}
