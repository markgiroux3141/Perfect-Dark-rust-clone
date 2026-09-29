//! The human player: `bondmove` (look/turn/aim, including the PC port's mouse
//! aim), `bondwalk` (collide-and-slide, stepping, ramps, falls, landing dip,
//! ladders, crouch under ceilings), `bondhead` (the head-bob model whose root
//! motion is the walk), footsteps, and spawning (`player_choose_spawn_location`,
//! `player_start_new_life`). One [`Player`] per human; the world steps each with
//! its own [`PlayerInput`].
//!
//! PD's walk is animation-driven: `g_PlayerModeldef` plays the walk and run clips
//! (`g_HeadAnims`, `bondhead.c:14`) at a speed set by how hard the stick is
//! pushed, and **the damped root motion of those clips is both the head bob and
//! the distance travelled** (`bwalk_update_horizontal`, `bondwalk.c:1611`). That
//! is where PD's ease-in and ease-out come from; there is no acceleration constant.
//!
//! Input follows the PC port's `CONTROLMODE_PC` with `MOUSEAIM_CLASSIC` for
//! keyboard and mouse (`pd-pcport/src/game/bondmove.c:1243`, defaults from
//! `mplayer.c:129`), and control style 1.1 for an N64 pad (`bondmove.c:1166`),
//! except the gun-function toggle, which keeps the N64's hold-B behaviour.
//!
//! The player carries its guns ([`Player::gun`], `crate::gun`) and its camera
//! ([`camera`]), both part of PD's `struct player`: `bmove_process_input` drives
//! the trigger, the gun function, reloads, weapon cycling and the zoom, and the
//! walk feeds the gun's sway. The player fields the gun code reads
//! (`crouchpos`, `bondbreathing`, `insightaimmode`, `guncloseroffset`) are
//! copied into its [`GunCtx`] per call.
//!
//! Source: the old repo's `pd_guns/player.rs`.

mod bondhead;
mod bondmove;
mod bondwalk;
pub mod activemenu;
pub mod health;
pub mod camera;
pub mod rooms;
pub mod slayer;
mod spawn;
pub mod vision;
#[cfg(test)]
mod tests;

pub use bondmove::bmove_dampen_shotspeed;
pub use spawn::{player_choose_spawn_location, SpawnOther};

use std::sync::Arc;

use glam::{Mat4, Vec3};
use pd_core::anim::{Anim, AnimBank};
use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::math::{self, baddtor2};
use pd_core::model::{Model, ModelDef, NodeKind};
use pd_core::rng::Rng;

use crate::gun::{Bgun, GunCtx, GunIn, HAND_MODELS};
use crate::stage::{CdObstacle, Edge, PropFloor, PropGeo, TileLevel};
use crate::world::WorldRes;
use bondhead::HeadAnim;
use camera::{Camera, SCREEN_H, SCREEN_W};

/// `PLAYER_DEFAULT_FOV`.
pub const PLAYER_DEFAULT_FOV: f32 = 60.0;

/// What the walk collides with: the stage's collision and every other chr's
/// perimeter cylinder (`CDTYPE_ALL`; the player's own is left out, as
/// `prop_set_perim_enabled(prop, false)` does).
pub struct WalkEnv<'a> {
    pub level: &'a TileLevel,
    pub cyls: &'a [PropGeo],
    /// The props' floors (`GEOTYPE_TILE_F`: lifts, objects' floor quads).
    pub floors: &'a [PropFloor],
    /// `MPOPTION_FASTMOVEMENT`: the walk speed × 1.25 (`bondwalk.c:1478`).
    pub fastmovement: bool,
    /// `player_get_shield_frac`: the player's chr's shield / 8.
    pub shieldfrac: f32,
    /// `current_player_is_menu_open_in_solo_or_mp`.
    pub menuopen: bool,
    /// Carrying a case in Hold the Briefcase or Capture the Case
    /// (`bondwalk.c:1472`): the walk as if the eye were at -63.6 cm.
    pub briefcase: bool,
}

/// The collision results PD keeps in globals after a test that collided
/// (`cd_get_edge`, `cd_has_distance` / `cd_get_distance`). The obstacle prop
/// (`cd_get_obstacle_prop`) only matters for pushing, which a free-for-all never does.
#[derive(Clone, Copy, Debug, Default)]
struct CdGlobals {
    edge: Option<Edge>,
    dist: Option<f32>,
}

impl CdGlobals {
    fn set(&mut self, o: Option<CdObstacle>) {
        if let Some(o) = o {
            *self = CdGlobals { edge: Some(o.edge), dist: o.dist };
        }
    }
}

/// One frame's controls for one player, already mapped from the devices
/// (`pd_game::controls`): the keyboard and mouse as the PC port reads them, or
/// an N64 pad (`pad`), whose control style is applied here.
#[derive(Clone, Debug, Default)]
pub struct PlayerInput {
    /// Mouse movement since the last frame, in pixels.
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    /// Z / left mouse.
    pub fire: bool,
    /// R / right mouse (hold to aim).
    pub aim: bool,
    /// The keyboard's movement "stick", -127..127 (x right, y forward).
    pub walk_x: i32,
    pub walk_y: i32,
    /// The N64 stick, about -80..80 (x right, y up). With a pad it walks and
    /// turns (style 1.1); with the keyboard it is unused.
    pub look_x: i32,
    pub look_y: i32,
    /// B / use: held.
    pub use_held: bool,
    pub reload: bool,
    pub cycle_next: bool,
    pub cycle_prev: bool,
    /// Pick a weapon (`WEAPON_*`, dual).
    pub select: Option<(u8, bool)>,
    /// Crouch key presses: go down one level / up one level.
    pub crouch_down: bool,
    pub crouch_up: bool,
    /// Manual zoom while aiming a sniper rifle or the Farsight: held.
    pub zoom_in: bool,
    pub zoom_out: bool,
    /// An N64 pad drove this frame: control style 1.1 (`bondmove.c:1166`).
    pub pad: bool,
    pub c_up: bool,
    pub c_down: bool,
    pub c_left: bool,
    pub c_right: bool,
    /// A held (`invbuttons`): tap = next gun, A+Z = previous gun, held = the
    /// active menu.
    pub a_held: bool,
    /// L, and the D-pad (the active menu reads them).
    pub l_trig: bool,
    pub d_up: bool,
    pub d_down: bool,
    pub d_left: bool,
    pub d_right: bool,
    /// START held: a press opens the pause menu (`bondmove.c:691`); a dead
    /// player holds it to respawn.
    pub start: bool,
}

const HEADANIM_RESTING: i32 = 0;
const HEADANIM_MOVING: i32 = 1;

/// `g_PlayerModeldef` (`modeldata/player.c:13`): a CHRINFO root and two
/// POSITION joints, no display lists. `bhead_update` reads matrix 0's
/// translation and axes as the head's offset and orientation.
pub fn player_modeldef() -> ModelDef {
    ModelDef::from_nodes(
        "g_PlayerModeldef",
        0x0b, // g_Skel0B
        3,
        vec![
            (NodeKind::ChrInfo { animpart: 0, mtx: 1 }, None, None),
            (NodeKind::Position { pos: Vec3::new(1.177_982, 41.144_371, 0.0), animpart: 1, mtx: [2, -1, -1], flags: 0 }, Some(0), Some(1)),
            (NodeKind::Position { pos: Vec3::new(-2.576_027, 480.429_02, 0.0), animpart: 2, mtx: [0, -1, -1], flags: 0 }, Some(1), Some(2)),
        ],
    )
}

/// `struct player`, the walk-mode subset.
pub struct Player {
    /// `prop->pos`: the eye, `vv_manground` + the eye height.
    pub pos: Vec3,
    /// `vv_theta`, degrees; forward = (-sin, 0, cos).
    pub theta: f32,
    /// `vv_verta`, degrees, positive up.
    pub verta: f32,
    /// `vv_eyeheight`.
    pub eyeheight: f32,
    /// `vv_headheight`: the top of the collision cylinder above `manground`
    /// (the eye height in multiplayer, `player.c:1495`).
    pub headheight: f32,
    /// `bond2.radius`.
    pub radius: f32,
    /// `vv_manground`: the smoothed height the feet are at.
    pub manground: f32,
    /// `vv_ground`: the floor under the cylinder this frame.
    pub ground: f32,
    /// `sumground`: the low-pass behind `manground` going up steps and ramps.
    pub sumground: f32,
    /// `bdeltapos.y`: the fall speed (cm per tick, negative down).
    pub fallspeed: f32,
    pub isfalling: bool,
    pub fallstart: i32,
    pub onladder: bool,
    pub ladderupdown: f32,
    laddernormal: Vec3,
    /// The floor polygon, its room (`floorroom`) and type (`floortype`).
    pub floorpoly: Option<usize>,
    pub floorroom: Option<u16>,
    pub floortype: u8,
    /// `floorflags`: the floor's `GEOFLAG_*` (kept while no floor is found).
    pub floorflags: u32,
    /// `inlift`, `lift`: standing on a lift's floor, and which lift (by object
    /// id; `None` also while falling onto one); `liftground` its height.
    pub inlift: bool,
    pub lift: Option<u32>,
    pub liftground: f32,
    /// Landing: `crouchtime240`, `crouchfall`, `sumcrouch` (the dip after a fall).
    crouchtime240: i32,
    crouchfall: f32,
    sumcrouch: f32,
    /// `bondshotspeed`: the shove from being shot (`chr_damage`), per tick.
    pub shotspeed: Vec3,
    /// `bondprevpos`.
    pub prevpos: Vec3,
    /// `player_die(true)` was asked for this frame: fell for 4 s, or out of the world.
    pub die_request: bool,
    /// `bondhealth`: 1 full, dead at 0 or below.
    pub bondhealth: f32,
    /// `isdead` (PD's 1 on the frame of death, 2 once `player_render_hud` has seen it).
    pub isdead: bool,
    isdead2: bool,
    /// The death sequence (`player_render_hud`, `player.c:4546`): the red
    /// fade has had its frame, the head's death animation has ended, the
    /// player may press to respawn.
    pub redbloodfinished: bool,
    pub deathanimfinished: bool,
    pub startnewbonddie: bool,
    pub dostartnewlife: bool,
    /// The health bar, the damage flash and the screen's fades.
    pub health: health::Health,
    /// `bondfade*` (`player_start_chr_fade`): the player's chr's alpha as
    /// others see it, 0..1.
    bondfadetime60: f32,
    bondfadetimemax60: f32,
    bondfadefracold: f32,
    bondfadefracnew: f32,
    pub chrfadefrac: f32,
    /// The fall speed of a landing this frame.
    pub landed: Option<f32>,
    cd: CdGlobals,

    pub speedtheta: f32,
    pub speedthetacontrol: f32,
    pub speedverta: f32,
    pub speedforwards: f32,
    pub speedsideways: f32,
    pub speedstrafe: f32,
    pub speedgo: f32,
    pub speedboost: f32,
    pub speedmaxtime60: i32,
    pub gunspeed: f32,

    /// `crouchpos`: `CROUCHPOS_SQUAT`/`DUCK`/`STAND`.
    pub crouchpos: i32,
    pub crouchoffset: f32,
    pub crouchspeed: f32,
    pub crouchoffsetreal: f32,
    pub crouchoffsetrealsmall: f32,
    pub crouchheight: f32,
    /// `guncloseroffset`: the crouch as the gun's pull-in (0..1).
    pub guncloseroffset: f32,
    /// `bondbreathing`: 0..1, rises while running.
    pub bondbreathing: f32,
    /// `insightaimmode`: R held.
    pub insightaimmode: bool,

    pub swaytarget: f32,
    pub swayoffset0: f32,
    pub swayoffset2: f32,

    // The head-bob model (bondhead.c).
    head: Model,
    head_anim: Anim,
    headanims: [HeadAnim; 2],
    pub headanim: i32,
    headdamp: f32,
    headamplitude: f32,
    sideamplitude: f32,
    headwalkingtime60: i32,
    pub headpos: Vec3,
    pub headlook: Vec3,
    pub headup: Vec3,
    headpossum: Vec3,
    headlooksum: Vec3,
    headupsum: Vec3,
    resetheadpos: bool,
    resetheadrot: bool,
    standheight: f32,
    standfrac: f32,
    standlook: [Vec3; 2],
    standup: [Vec3; 2],
    standcnt: usize,

    // Look / aim.
    /// `bond2.look` / `bond2.up`: the camera basis.
    pub look: Vec3,
    pub up: Vec3,
    pub swivelpos: [f32; 2],
    pub usedowntime: i32,
    /// `bondactivateorreload`: a use tap this frame (a door, else a reload).
    pub bondactivateorreload: bool,
    /// `invdowntime`: A held for this many ticks (-1 = consumed).
    pub invdowntime: i32,
    /// `aimtaptime`: R held this long (a short tap uncrouches), -1 = used.
    pub aimtaptime: i32,
    /// Last frame's C-up / C-down, for the crouch presses.
    prev_c_updown: [bool; 2],
    prev_fire: bool,
    pub waitforzrelease: bool,
    pub zoominfovy: f32,
    zoominfovyold: f32,
    zoominfovynew: f32,
    zoomintime: f32,
    zoomintimemax: f32,
    pub headroll: bool,

    /// Footsteps (`bondmove.c:1933`): `footstepdist`, `foot`, the chr's `lastfootsample`.
    footstepdist: f32,
    foot: i32,
    lastfootsample: i32,

    pub mouse_sens: f32,
    pub mouseaimspeed: f32,
    pub crosshairsway: f32,
    pub crosshairedgeboundary: f32,
    /// The view: `player_get_viewport_width/height` and `player_get_aspect_ratio`
    /// (320 × 220 at 320/220 for one player).
    pub viewwidth: f32,
    pub viewheight: f32,
    pub aspect: f32,

    /// The hands and `gunctrl` (`bondgun.c`).
    pub gun: Bgun,
    /// `camera.c`'s state.
    pub cam: Camera,

    /// `visionmode`: `VISIONMODE_*`.
    pub visionmode: i32,
    /// `cameramode`: `CAMERAMODE_*`; third person while riding a Slayer rocket,
    /// when `player_render_hud` leaves the gun and the HUD out.
    pub cameramode: i32,
    /// `slayerrocket`: the Slayer rocket the camera rides, by object id.
    pub slayerrocket: Option<u32>,
    /// `badrockettime`: ticks the ridden rocket has been out of bounds.
    pub badrockettime: i32,
    /// Z held last frame, for the rocket's "pressed this frame".
    pub slayer_prevfire: bool,
    /// `devicesactive`: `DEVICE_*` (the RC-P120's cloak here).
    pub devicesactive: u32,
    /// The player chr's cloak (`CHRHFLAG_CLOAKED`, `cloakfadefrac`, ...).
    pub cloak: crate::chr::cloak::ChrCloak,
    /// The active menu (`g_AmMenus[playernum]`, `activemenumode`), the
    /// buttons it last read, and A held long enough to open it this tick.
    pub am: activemenu::ActiveMenu,
    pub activemenumode: u8,
    pub am_prevbuttons: u16,
    pub am_open_request: bool,
    /// `aibuddynums`: the simulants on the player's team (chr indexes), and
    /// the one the active menu is ordering (`commandingaibot`).
    pub aibuddynums: Vec<usize>,
    pub commandingaibot: Option<usize>,
    /// The x-ray eraser (`eraserpos`, `erasertime`, the colour shifts).
    pub eraser: vision::Eraser,
    /// This frame's framebuffer effects (`lv_render`'s `bview_*` calls).
    pub viewfx: vision::ViewFx,

    /// `prop->rooms`: the rooms the player's box is in (`bmove_update_rooms`).
    pub rooms: Vec<u16>,
    /// `cam_room`: the room the camera is in (`player_set_cam_properties`).
    pub cam_room: usize,
    /// `memcampos` / `memcamroom`: the last camera position that was in bounds.
    pub memcampos: Vec3,
    pub memcamroom: Option<u16>,
    /// This frame's rooms on screen (`bg_tick_portals`).
    pub portalview: crate::stage::portals::PortalView,

    bank: Arc<AnimBank>,
    /// `PLAYERCOUNT()`.
    playercount: usize,
}

impl Player {
    /// A player standing at `feet` facing `theta` degrees, with `bgun_reset` and
    /// `bhead_reset` run, holding Joanna's combat hands. `playercount` is
    /// `PLAYERCOUNT()`, which the models' posing reads.
    pub fn new(res: &WorldRes, feet: Vec3, theta: f32, playercount: usize, rng: &mut Rng) -> Result<Player, String> {
        let bank = res.bank.clone();
        let gun = Bgun::new(HAND_MODELS[0], &res.gset, rng);
        let anim = |name: &str| bank.by_name(name).ok_or_else(|| format!("no {name} in the animation bank"));
        let (walk, run, hold) = (anim("ANIM_002B")?, anim("ANIM_0029")?, anim("ANIM_TWO_GUN_HOLD")?);
        let mut p = Player {
            pos: feet + Vec3::Y * 159.0,
            theta,
            verta: 0.0,
            eyeheight: 159.0,
            headheight: 159.0,
            radius: 30.0,
            manground: feet.y,
            ground: feet.y,
            sumground: feet.y / 0.045_499_98,
            // bwalk_init: bdeltapos.y = -0.0001.
            fallspeed: -0.0001,
            isfalling: false,
            fallstart: 0,
            onladder: false,
            ladderupdown: 0.0,
            laddernormal: Vec3::ZERO,
            floorpoly: None,
            floorroom: None,
            floortype: FLOORTYPE_DEFAULT,
            floorflags: 0,
            inlift: false,
            lift: None,
            liftground: 0.0,
            crouchtime240: 0,
            crouchfall: 0.0,
            sumcrouch: 0.0,
            shotspeed: Vec3::ZERO,
            prevpos: feet,
            die_request: false,
            bondhealth: 1.0,
            isdead: false,
            isdead2: false,
            redbloodfinished: false,
            deathanimfinished: false,
            startnewbonddie: true,
            dostartnewlife: false,
            health: health::Health::default(),
            bondfadetime60: 0.0,
            bondfadetimemax60: -1.0,
            bondfadefracold: 1.0,
            bondfadefracnew: 1.0,
            chrfadefrac: 1.0,
            landed: None,
            cd: CdGlobals::default(),
            speedtheta: 0.0,
            speedthetacontrol: 0.0,
            speedverta: 0.0,
            speedforwards: 0.0,
            speedsideways: 0.0,
            speedstrafe: 0.0,
            speedgo: 0.0,
            speedboost: 1.0,
            speedmaxtime60: 0,
            gunspeed: 0.0,
            crouchpos: CROUCHPOS_STAND,
            crouchoffset: 0.0,
            crouchspeed: 0.0,
            crouchoffsetreal: 0.0,
            crouchoffsetrealsmall: 0.0,
            crouchheight: 0.0,
            guncloseroffset: 0.0,
            bondbreathing: 0.0,
            insightaimmode: false,
            swaytarget: 0.0,
            swayoffset0: 0.0,
            swayoffset2: 0.0,
            head: Model::new(Arc::new(player_modeldef())),
            head_anim: Anim::default(),
            // g_HeadAnims (bondhead.c:14)
            headanims: [
                HeadAnim { animnum: walk, loopframe: 9.5, endframe: 27.0, translateperframe: 0.0, maxspeed: 1.5 },
                HeadAnim { animnum: run, loopframe: 7.5, endframe: 17.0, translateperframe: 0.0, maxspeed: 100.0 },
            ],
            headanim: HEADANIM_RESTING,
            headdamp: 0.93,
            headamplitude: 1.0,
            sideamplitude: 1.0,
            headwalkingtime60: 0,
            headpos: Vec3::ZERO,
            headlook: Vec3::ZERO,
            headup: Vec3::ZERO,
            headpossum: Vec3::ZERO,
            headlooksum: Vec3::new(0.0, 0.0, 14.285_716),
            headupsum: Vec3::new(0.0, 14.285_716, 0.0),
            resetheadpos: true,
            resetheadrot: true,
            standheight: 0.0,
            standfrac: 0.0,
            standlook: [Vec3::Z; 2],
            standup: [Vec3::Y; 2],
            standcnt: 0,
            look: Vec3::Z,
            up: Vec3::Y,
            swivelpos: [0.0; 2],
            usedowntime: 0,
            bondactivateorreload: false,
            invdowntime: 0,
            aimtaptime: 0,
            prev_c_updown: [false; 2],
            prev_fire: false,
            waitforzrelease: false,
            zoominfovy: PLAYER_DEFAULT_FOV,
            zoominfovyold: PLAYER_DEFAULT_FOV,
            zoominfovynew: PLAYER_DEFAULT_FOV,
            zoomintime: 0.0,
            zoomintimemax: 0.0,
            headroll: true,
            footstepdist: 0.0,
            foot: 0,
            lastfootsample: -1,
            mouse_sens: 2.5,
            mouseaimspeed: 0.7,
            crosshairsway: 1.0,
            crosshairedgeboundary: 0.7,
            viewwidth: SCREEN_W,
            viewheight: SCREEN_H,
            aspect: SCREEN_W / SCREEN_H,
            gun,
            cam: Camera::default(),
            visionmode: VISIONMODE_NORMAL,
            cameramode: CAMERAMODE_DEFAULT,
            rooms: Vec::new(),
            // playermgr_allocate_player (playermgr.c:229).
            cam_room: 1,
            memcampos: Vec3::ZERO,
            memcamroom: None,
            portalview: Default::default(),
            slayerrocket: None,
            badrockettime: 0,
            slayer_prevfire: false,
            devicesactive: 0,
            cloak: crate::chr::cloak::ChrCloak::default(),
            am: activemenu::ActiveMenu::default(),
            activemenumode: activemenu::AMMODE_CLOSED,
            am_prevbuttons: 0,
            am_open_request: false,
            aibuddynums: Vec::new(),
            commandingaibot: None,
            eraser: vision::Eraser::default(),
            viewfx: vision::ViewFx::default(),
            bank,
            playercount,
        };
        p.bhead_reset(hold, rng);
        p.reset_look();
        Ok(p)
    }

    /// Stand the player on the floor at `feet` (`player_start_new_life` →
    /// `player_reset_bond` → `bwalk_init`): feet there, the eye above, nothing
    /// carried over.
    pub fn place(&mut self, feet: Vec3, theta: f32) {
        self.manground = feet.y;
        self.ground = feet.y;
        self.sumground = feet.y / 0.045_499_98;
        self.pos = Vec3::new(feet.x, feet.y + self.eyeheight, feet.z);
        self.prevpos = self.pos;
        self.theta = theta;
        self.verta = 0.0;
        self.fallspeed = -0.0001;
        self.isfalling = false;
        self.onladder = false;
        self.shotspeed = Vec3::ZERO;
        self.speedforwards = 0.0;
        self.speedsideways = 0.0;
        self.speedstrafe = 0.0;
        self.speedgo = 0.0;
        self.speedtheta = 0.0;
        self.speedthetacontrol = 0.0;
        self.speedverta = 0.0;
        self.crouchtime240 = 0;
        self.crouchfall = 0.0;
        self.sumcrouch = 0.0;
        self.crouchheight = 0.0;
        self.crouchpos = CROUCHPOS_STAND;
        self.die_request = false;
        self.resetheadpos = true;
        self.resetheadrot = true;
        self.reset_look();
    }

    /// The view before the first tick: level, along `theta` (`player_start_new_life`
    /// sets `bond2.theta`; `bmove_update_head_with_mtx` builds the full basis on
    /// the next frame).
    fn reset_look(&mut self) {
        self.look = self.theta_vec();
        self.up = Vec3::Y;
    }

    /// Put the crouch straight at `offset` (−90 squat, −45 duck, 0 stand), as if
    /// `bwalk_update_crouch_offset` had finished getting there (a harness and
    /// spawn helper; PD tweens it).
    pub fn set_crouch_offset(&mut self, offset: f32) {
        self.crouchoffset = offset;
        self.crouchspeed = 0.0;
        self.update_crouch_offset_real();
    }

    /// `player_tick`'s part for a living player in the first person
    /// (`player.c:3140`): the view (`vi_set_fov_aspect_and_size(60, ...)`), then
    /// `bmove_tick` in walk mode (`bondmove.c:1880`: the controls with the guns'
    /// `bgun_tick_gameplay`, walking with `bwalk_tick`, `bondwalk.c:1791`, the
    /// footsteps), then the camera's matrices for this frame
    /// (`player_allocate_matrices`). Sounds go to `events`.
    pub fn tick(&mut self, input: &PlayerInput, lv: &Lv, env: &WalkEnv, res: &WorldRes, rng: &mut Rng, events: &mut Vec<Event>) {
        self.die_request = false;
        self.landed = None;
        self.cam.vi_set_fov_aspect_and_size(PLAYER_DEFAULT_FOV, self.aspect, self.viewwidth, self.viewheight);
        self.cam.cam_set_screen_position(0.0, 0.0);
        self.health.player_update_colour_screen_properties(lv.lvupdate60freal);
        self.player_tick_chr_fade(lv.lvupdate60freal);
        self.health.player_tick_damage_and_health(self.bondhealth, env.shieldfrac, self.isdead, env.menuopen, lv.lvupdate60freal, lv.diffframe60freal);
        // bmove_process_input reads no controls while dead (`bondmove.c:717`).
        let idle = PlayerInput::default();
        let input = if self.isdead { &idle } else { input };
        self.bmove_process_input(input, lv, res, rng);
        // bwalk_update_prev_pos
        self.prevpos = self.pos;
        self.bwalk_update_theta(lv);
        self.bmove_update_look();
        self.bwalk_update_horizontal(lv, env, res, rng);
        self.bwalk_update_vertical(lv, env);
        self.bmove_footsteps(rng, events);
        self.cam.player_allocate_matrices(self.pos, self.look, self.up);
    }

    /// The player fields the gun code reads.
    pub fn gun_in(&self) -> GunIn {
        GunIn {
            crouchpos: self.crouchpos,
            bondbreathing: self.bondbreathing,
            guncloseroffset: self.guncloseroffset,
            insightaimmode: self.insightaimmode,
            isdead: self.isdead,
            playercount: self.playercount,
        }
    }

    /// This player as `g_Vars.currentplayer` for the gun code.
    pub fn gun_ctx<'a>(&'a mut self, res: &'a WorldRes, rng: &'a mut Rng, lv: &'a Lv) -> GunCtx<'a> {
        let pl = self.gun_in();
        GunCtx { b: &mut self.gun, gset: &res.gset, bank: &res.bank, models: &res.models, rng, lv, cam: &self.cam, pl }
    }

    /// `player_get_bbox` (`player.c:5193`), walk mode: radius, then the absolute
    /// top and bottom of the collision cylinder. The bottom is 30 cm above the feet,
    /// so anything lower is stepped onto rather than walked into.
    pub fn player_get_bbox(&self) -> (f32, f32, f32) {
        let ymin = self.manground + 30.0;
        let ymax = (self.manground + self.headheight + self.crouchoffsetrealsmall).max(self.manground + 80.0);
        (self.radius, ymax, ymin)
    }

    /// `player_update_perim_info` (`player.c:5160`): the cylinder other chrs
    /// collide with and shoot at.
    pub fn perim(&self) -> PropGeo {
        let ymax = (self.manground + self.headheight + self.crouchoffsetrealsmall).max(self.manground + 80.0);
        PropGeo::cyl(self.pos.x, self.pos.z, self.radius, self.manground, ymax)
    }

    /// `bond2.theta`: the facing unit vector.
    pub fn theta_vec(&self) -> Vec3 {
        let t = baddtor2(self.theta);
        Vec3::new(-t.sin(), 0.0, t.cos())
    }

    /// The camera for this frame: `player_allocate_matrices`' `mtxf0068`
    /// (camera → world, `cam_get_projection_mtxf`) and `mtxf0064` (world → camera).
    pub fn camera(&self) -> (Mat4, Mat4) {
        (math::look_basis(self.pos, self.look, self.up), math::view_matrix(self.pos, self.look, self.up))
    }

    /// Which head clip is running, its frame and speed (debug).
    pub fn head_debug(&self) -> (i32, f32, f32) {
        (self.headanim, self.head_anim.frame, self.head_anim.speed)
    }

    /// `bmove_tick`'s footsteps (`bondmove.c:1933`): every 150 cm walked, a
    /// sample for the floor under the player. PD plays none in a multiplayer
    /// match with more than one human (`PLAYERCOUNT() == 1`).
    fn bmove_footsteps(&mut self, rng: &mut Rng, events: &mut Vec<Event>) {
        if (self.speedforwards == 0.0 && self.speedsideways == 0.0) || self.playercount != 1 {
            return;
        }
        if self.fallspeed < -6.0 {
            return;
        }
        let distance = (self.prevpos - self.pos).length();
        self.footstepdist += distance;
        if self.footstepdist < 150.0 {
            return;
        }
        self.footstepdist = 0.0;
        self.foot = 1 - self.foot;
        // chr->floortype = currentplayer->floortype (the ground probe's).
        let sound = crate::chr::footstep_choose_sound(rng, self.floortype, &mut self.lastfootsample, distance > 10.0);
        if sound != 0 {
            // snd_start_extra(NULL, false, AL_VOL_FULL, AL_PAN_CENTER, sound, 1, 1, -1, true)
            events.push(Event::Sound { sound, pitch: 1.0, volume: 1.0, pan: 0.0 });
        }
    }
}
