//! Guns: the weapon table and gun scripts ([`gset`], from `assets/data/weapons.json`,
//! the one table for players and simulants), the hand state machine (`bondgun.c`:
//! fire, burst and automatic fire, reload, dry fire, weapon switching with the
//! gun-memory load latency, gun functions, dual wield), the on-screen pose (sway,
//! crosshair swivel, recoil, slide, zoom), shots ([`shot`]:
//! `hand_tick_attack`, `shot_calculate_hits`, penetration, hit parts), and the
//! gun HUD's timers ([`hud`]).
//!
//! PD reaches everything through `g_Vars.currentplayer`. Here a player's gun
//! state is a [`Bgun`] (the hands, `gunctrl`, and the gun fields of `struct
//! player`), and the ported functions run on a [`GunCtx`]: that `Bgun` plus the
//! world's RNG and timing, the player's camera and the player fields the gun
//! reads ([`GunIn`]). Side effects PD performs in place (sounds, beams,
//! casings, smoke) are queued as [`GunEvent`]s for the world.
//!
//! NTSC final throughout. Line numbers cite `reference/pd-decomp/src/game/bondgun.c`.
//!
//! Sources: the old repo's `pd_guns/gset.rs`, `bgun.rs`, `bgun_state.rs`,
//! `bgun_pose.rs`, `hud.rs` (the timers) and the shot half of `sim.rs`.
//! Replaces `pd_spike/weapons.rs` (8 hand-copied guns).

mod bgun;
pub mod gset;
pub mod hud;
mod pose;
pub mod shot;
mod state;
#[cfg(test)]
mod tests;

use glam::{Mat4, Vec3};
use pd_core::anim::{Anim, AnimBank};
use pd_core::ids::*;
use pd_core::lv::Lv;
use pd_core::model::{Model, ModelStore};
use pd_core::rng::Rng;

use crate::fx::Beam;
use crate::player::camera::Camera;
pub use gset::Gset;
use gset::{CmdPtr, FuncDef, WeaponDef};
pub use hud::HudState;

/// `MAX_PITCH` (`bondgun.c:70`): how far a gun tips down when lowered.
pub(crate) fn max_pitch() -> f32 {
    pd_core::math::baddtor(50.0)
}

/// `g_Vars.normmplayerisrunning`: always, in the Combat Simulator (the shorter
/// raise and lower times).
const NORMMPLAYERISRUNNING: bool = true;

/// Sound refs the gun code starts directly.
pub const SFXMAP_804F_RELOAD_DEFAULT: u16 = 0x804f;
pub const SFXMAP_8052_FIREEMPTY: u16 = 0x8052;
pub const SFXNUM_00E8_PICKUP_GUN: u16 = 0x00e8;

/// `g_AmmoTypes[].capacity` (`bondgun.c:9316`), by `AMMOTYPE_*`.
pub const AMMO_CAPACITY: [i32; 33] = [0, 800, 800, 69, 400, 100, 100, 12, 3, 10, 200, 40, 10, 10, 10, 800, 15, 50, 10, 200, 18000, 4, 200, 2, 10, 10, 10, 1000, 10, 50, 1, 200, 10];

/// Hand models by `g_HeadsAndBodies[].handfilenum` (`modeldata/robot.c`). The
/// first is Joanna's combat suit (BODY_DARK_COMBAT → FILE_GCOMBATHANDSLOD), the
/// Combat Simulator's default Joanna.
pub const HAND_MODELS: [&str; 9] = ["combathandslod", "hand_jofrock", "hand_jotrench", "hand_jopilot", "hand_jowetsuit", "hand_josnow", "hand_joaf1", "hand_mrblonde", "hand_carrington"];

/// What the gun code asks the world to do.
#[derive(Clone, Debug)]
pub enum GunEvent {
    /// `snd_start` of a sound ref at `speed`. `handle`: the hand whose
    /// `audiohandle` keeps it (the Reaper spin, the Mauler charge), so a later
    /// [`GunEvent::StopSound`] can stop it.
    Sound { id: u16, speed: f32, handle: Option<usize> },
    /// Stop a hand's `audiohandle` sound.
    StopSound { hand: usize },
    /// `beam_create_for_hand`: a tracer from the muzzle to the shot's hit point.
    Beam { hand: usize },
    /// `casing_create_for_hand`: `mtx` is the eject node's world matrix.
    Casing { hand: usize, mtx: Mat4, casing: i32 },
    /// `smoke_create_for_hand`.
    Smoke { hand: usize, pos: Vec3, ty: usize },
    /// `bgun_free_held_rocket` (`:4552`). M5: the launcher's rocket.
    FreeHeldRocket { hand: usize },
    /// `chr_uncloak_temporarily` for a thrown or fired projectile (`:7273`).
    UncloakTemporarily,
    /// `bgun_update_rocket_launcher` (`:6997`). M5: the world owns the rocket.
    UpdateRocketLauncher { hand: usize },
}

/// The player fields the gun code reads (the rest of `struct player` is the
/// walk's), copied in for each call.
#[derive(Clone, Copy, Debug)]
pub struct GunIn {
    pub crouchpos: i32,
    pub bondbreathing: f32,
    /// The crouch as the gun's pull-in, 0..1.
    pub guncloseroffset: f32,
    /// R held.
    pub insightaimmode: bool,
    pub isdead: bool,
    /// `PLAYERCOUNT()`: the models' posing reads it.
    pub playercount: usize,
}

impl Default for GunIn {
    fn default() -> Self {
        GunIn { crouchpos: CROUCHPOS_STAND, bondbreathing: 0.0, guncloseroffset: 0.0, insightaimmode: false, isdead: false, playercount: 1 }
    }
}

// ─── the hand ────────────────────────────────────────────────────────────────

/// `struct hand` (`types.h:2086`): the fields the ported functions use.
#[derive(Clone)]
pub struct Hand {
    // gset
    pub weaponnum: u8,
    pub weaponfunc: usize,
    pub upgradewant: u8,

    pub firing: bool,
    pub flashon: bool,
    pub visible: bool,
    pub inuse: bool,
    pub triggeron: bool,
    pub triggerprev: bool,
    pub triggerreleased: bool,
    pub count: i32,
    pub count60: i32,
    pub mode: u32,
    pub modenext: u32,
    pub numfires: u32,
    pub pausetime60: i32,
    pub pausechange: u32,
    pub posstart: Vec3,
    pub rotxstart: f32,
    pub posend: Vec3,
    pub rotxend: f32,
    pub posoffset: Vec3,
    pub rotxoffset: f32,
    pub posrotmtx: Mat4,
    pub useposrot: bool,
    pub damppos: Vec3,
    pub damplook: Vec3,
    pub dampup: Vec3,
    pub damppossum: Vec3,
    pub damplooksum: Vec3,
    pub dampupsum: Vec3,
    pub blendpos: [Vec3; 4],
    pub blendlook: [Vec3; 4],
    pub blendup: [Vec3; 4],
    pub curblendpos: i32,
    pub dampt: f32,
    pub blendscale: f32,
    pub blendscale1: f32,
    pub sideflag: i32,
    pub adjustdamp: Vec3,
    pub adjustpos: Vec3,
    pub xshift: f32,
    pub aimpos: Vec3,
    pub allowshootframe: i32,
    pub lastshootframe60: i32,
    pub noiseradius: f32,
    pub slidetrans: f32,
    pub slideinc: bool,
    pub loadedammo: [i32; 2],
    pub clipsizes: [i32; 2],
    /// The `matmot1/2/3` union: `mm_maulercharge`, `mm_reaperrot`/`speedaim`/
    /// `speedcur`, `mm_shotgunfrac`...
    pub matmot1: f32,
    pub matmot2: f32,
    pub matmot3: f32,
    pub loadslide: f32,
    pub upgrademult: [f32; 2],
    pub finalmult: [f32; 2],
    pub cammtx: Mat4,
    pub posmtx: Mat4,
    pub prevmtx: Mat4,
    pub muzzlepos: Vec3,
    pub muzzlez: f32,
    pub muzzlemat: Mat4,
    pub burstbullets: i32,
    pub hitpos: Vec3,
    pub lastdirvalid: bool,
    pub shotstotake: i32,
    pub shotremainder: f32,
    pub state: i32,
    pub stateminor: i32,
    pub stateflags: u32,
    pub stateframes: i32,
    pub statecycles: i32,
    pub statelastframe: i32,
    pub statevar1: i32,
    /// `gs_float1`, aka `gs_barrelspeedfrac`.
    pub gs_barrelspeedfrac: f32,
    pub animload: i32,
    pub animframeinc: i32,
    pub animmode: i32,
    pub unk0cc8_01: bool,
    pub unk0cc8_02: bool,
    pub incrementalreloading: bool,
    pub ejectcount: i32,
    pub unk0cc8_07: bool,
    pub unk0cc8_08: bool,
    pub animloopcount: i32,
    pub crosspos: [f32; 2],
    pub guncrosspossum: [f32; 2],
    pub attacktype: i32,
    pub animcmd: Option<CmdPtr>,
    pub animcmd2: Option<CmdPtr>,
    pub gangstarot: f32,
    pub primetimer60: i32,
    pub ejectstate: i32,
    pub ejecttype: i32,
    pub unk0d0e_07: bool,
    pub createsmoke: bool,
    pub forcecreatesmoke: bool,
    pub unk0d0f_02: bool,
    pub activatesecondary: bool,
    pub gunroundsspent: [u16; 4],
    /// `ispare1`: the gangsta delay timer.
    pub ispare1: i32,
    pub gunsmokepoint: f32,
    pub fspare1: f32,
    pub fspare2: f32,
    pub lastrotangx: f32,
    pub lastrotangy: f32,
    /// `hand->audiohandle` is playing (the Reaper spin, the Mauler charge).
    pub audiohandle: bool,
    /// `gs_int1` / `gs_int2`, used by the reload.
    pub gs_int1: i32,
    pub gs_int2: i32,

    /// `hand->anim`, shared by `gunmodel` and `handmodel`.
    pub anim: Anim,
    /// `hand->gunmodel`, with its toggle state.
    pub gunmodel: Option<Model>,
    /// `hand->handmodel`: drawn with the gun model's matrices.
    pub handmodel: Option<Model>,
    /// The muzzle flash toggle nodes (parts 0x5a..0x5c) found this frame.
    pub flash_toggles: Vec<usize>,
    /// The left hand of a `WEAPONFLAG_DUALFLIP` weapon is drawn mirrored.
    pub dualflip: bool,
    /// `hand->rocket`: the rocket sitting in the launcher (M5).
    pub rocket: Option<u32>,
    /// `hand->beam`: this hand's tracer.
    pub beam: Beam,
}

impl Hand {
    /// `bgun_reset`'s positional initialiser (`bondgunreset.c:17`).
    fn new() -> Self {
        Hand {
            weaponnum: WEAPON_NONE,
            weaponfunc: FUNC_PRIMARY,
            upgradewant: 0,
            firing: false,
            flashon: false,
            visible: false,
            inuse: false,
            triggeron: false,
            triggerprev: false,
            triggerreleased: false,
            count: 0,
            count60: 0,
            mode: HANDMODE_NONE,
            modenext: HANDMODE_NONE,
            numfires: 0,
            pausetime60: 0,
            pausechange: 0,
            posstart: Vec3::ZERO,
            rotxstart: 0.0,
            posend: Vec3::ZERO,
            rotxend: 0.0,
            posoffset: Vec3::ZERO,
            rotxoffset: 0.0,
            posrotmtx: Mat4::IDENTITY,
            useposrot: false,
            damppos: Vec3::ZERO,
            damplook: Vec3::new(0.0, 0.0, -1.0),
            dampup: Vec3::new(0.0, 1.0, 0.0),
            damppossum: Vec3::ZERO,
            damplooksum: Vec3::new(0.0, 0.0, -19.999_996),
            dampupsum: Vec3::new(0.0, 19.999_996, 0.0),
            blendpos: [Vec3::ZERO; 4],
            blendlook: [Vec3::new(0.0, 0.0, -1.0); 4],
            blendup: [Vec3::new(0.0, 1.0, 0.0); 4],
            curblendpos: 0,
            dampt: 0.0,
            blendscale: 1.0,
            blendscale1: 1.0,
            sideflag: 0,
            adjustdamp: Vec3::ZERO,
            adjustpos: Vec3::ZERO,
            xshift: 0.0,
            aimpos: Vec3::new(0.0, 0.0, 1000.0),
            allowshootframe: 0,
            lastshootframe60: 0,
            noiseradius: 0.0,
            slidetrans: 0.0,
            slideinc: false,
            loadedammo: [0; 2],
            clipsizes: [0; 2],
            matmot1: 0.0,
            matmot2: 0.0,
            matmot3: 0.0,
            loadslide: 0.0,
            upgrademult: [1.0; 2],
            finalmult: [1.0; 2],
            cammtx: Mat4::IDENTITY,
            posmtx: Mat4::IDENTITY,
            prevmtx: Mat4::IDENTITY,
            muzzlepos: Vec3::ZERO,
            muzzlez: 0.0,
            muzzlemat: Mat4::IDENTITY,
            burstbullets: 0,
            hitpos: Vec3::ZERO,
            lastdirvalid: false,
            shotstotake: 0,
            shotremainder: 0.0,
            state: HANDSTATE_IDLE,
            stateminor: 0,
            stateflags: 0,
            stateframes: 0,
            statecycles: 0,
            statelastframe: 0,
            statevar1: 0,
            gs_barrelspeedfrac: 0.0,
            animload: -1,
            animframeinc: 0,
            animmode: HANDANIMMODE_IDLE,
            unk0cc8_01: false,
            unk0cc8_02: false,
            incrementalreloading: false,
            ejectcount: 0,
            unk0cc8_07: false,
            unk0cc8_08: false,
            animloopcount: 0,
            crosspos: [0.0; 2],
            guncrosspossum: [0.0; 2],
            attacktype: 0,
            animcmd: None,
            animcmd2: None,
            gangstarot: 0.0,
            primetimer60: 0,
            ejectstate: EJECTSTATE_INACTIVE,
            ejecttype: EJECTTYPE_GUN,
            unk0d0e_07: false,
            createsmoke: false,
            forcecreatesmoke: false,
            unk0d0f_02: false,
            activatesecondary: false,
            gunroundsspent: [0; 4],
            ispare1: 0,
            gunsmokepoint: 0.0,
            fspare1: 0.0,
            fspare2: 0.0,
            lastrotangx: 0.0,
            lastrotangy: 0.0,
            audiohandle: false,
            gs_int1: 0,
            gs_int2: 0,
            anim: Anim::default(),
            gunmodel: None,
            handmodel: None,
            flash_toggles: Vec::new(),
            dualflip: false,
            rocket: None,
            beam: Beam::default(),
        }
    }
}

/// `struct gunctrl` (the fields used).
#[derive(Clone, Debug)]
pub struct GunCtrl {
    pub weaponnum: u8,
    pub prevweaponnum: u8,
    /// PD's `switchtoweaponnum`, -1 as `None`.
    pub switchtoweaponnum: Option<u8>,
    pub dualwielding: bool,
    pub prevwasdualwielding: bool,
    pub invertgunfunc: bool,
    pub wantammo: bool,
    pub throwing: bool,
    pub gangsta: bool,
    pub ammotypes: [i32; 2],
    /// The gun memory (`bgun_tick_master_load`): what is loaded, what is loading
    /// (PD's -1 as `None`), and the load steps left.
    pub gunmemtype: u8,
    pub gunmemnew: Option<u8>,
    pub load_steps: i32,
    /// The hand model loaded (`handfilenum`), by stem.
    pub handfilenum: String,
}

/// The gun fields of `struct player`.
#[derive(Clone, Debug)]
pub struct GunPlayer {
    pub crosspos: [f32; 2],
    pub crosspossum: [f32; 2],
    pub oldcrosspos: [f32; 2],
    pub guncrossdamp: f32,
    pub crosspos2: [f32; 2],
    pub crosssum2: [f32; 2],
    pub gunaimdamp: f32,
    pub gunposamplitude: f32,
    pub gunxamplitude: f32,
    pub gunampsum: f32,
    pub cyclesum: f32,
    pub synccount: f32,
    pub syncchange: f32,
    pub gunsync: f32,
    pub syncoffset: i32,
    pub playertriggeron: bool,
    pub playertriggerprev: bool,
    pub playertrigtime240: i32,
    pub curguntofire: usize,
    pub doautoselect: bool,
    /// `gunshadecol`, RGBA.
    pub gunshadecol: [u8; 4],
    pub ammoheldarr: [i32; 40],
    pub gunzoomfovs: [f32; 3],
    /// `g_PlayerConfigsArray[].gunfuncs`: per-weapon "use the secondary" bits.
    pub gunfuncs: [u8; 8],
    /// Inventory (`inv.c`): the weapons held, and whether twice. M8's pickups
    /// fill it; until then the world hands out a loadout.
    pub inventory: Vec<(u8, bool)>,
    /// `CHEAT_UNLIMITEDAMMO`'s `bgun_give_max_ammo` every frame.
    pub unlimited_ammo: bool,
}

/// One player's guns: both hands, `gunctrl`, the gun fields of `struct player`,
/// the HUD's timers, and the events the last tick queued.
#[derive(Clone)]
pub struct Bgun {
    pub hands: [Hand; 2],
    pub ctrl: GunCtrl,
    pub p: GunPlayer,
    pub hud: HudState,
    pub events: Vec<GunEvent>,
    /// The player's hand model stem (`g_HeadsAndBodies[].handfilenum`).
    pub hand_model: String,
    /// `var8009d140`: the Reaper barrel angle the joint callback turns by.
    pub reaper_rot: f32,
    /// `speedtheta * 0.3 + gunextraaimx`, `-speedverta * 0.1 + gunextraaimy`:
    /// the fists' swivel target (`bgun_swivel`, `:4912`), set by bmove.
    pub swivel_extra: [f32; 2],
}

impl Bgun {
    /// `bgun_reset` (`bondgunreset.c`) for a player with `hand_model` hands.
    /// The three blend keys per hand draw from `rng`.
    pub fn new(hand_model: &str, gset: &Gset, rng: &mut Rng) -> Bgun {
        let mut b = Bgun {
            hands: [Hand::new(), Hand::new()],
            ctrl: GunCtrl {
                weaponnum: WEAPON_NONE,
                prevweaponnum: WEAPON_UNARMED,
                switchtoweaponnum: None,
                dualwielding: false,
                prevwasdualwielding: false,
                invertgunfunc: false,
                wantammo: false,
                throwing: false,
                gangsta: false,
                ammotypes: [-1; 2],
                gunmemtype: WEAPON_NONE,
                gunmemnew: None,
                load_steps: 0,
                handfilenum: String::new(),
            },
            p: GunPlayer {
                crosspos: [0.0; 2],
                crosspossum: [0.0; 2],
                oldcrosspos: [0.0; 2],
                guncrossdamp: 0.9,
                crosspos2: [0.0; 2],
                crosssum2: [0.0; 2],
                gunaimdamp: 0.9,
                gunposamplitude: 1.0,
                gunxamplitude: 1.0,
                gunampsum: 0.0,
                cyclesum: 0.0,
                synccount: 0.0,
                syncchange: 0.0,
                gunsync: 0.0,
                syncoffset: 0,
                playertriggeron: false,
                playertriggerprev: false,
                playertrigtime240: 0,
                curguntofire: 0,
                doautoselect: false,
                gunshadecol: [0xff, 0xff, 0xff, 0],
                ammoheldarr: [0; 40],
                gunzoomfovs: [15.0, 60.0, 30.0],
                gunfuncs: [0; 8],
                inventory: Vec::new(),
                unlimited_ammo: false,
            },
            hud: HudState::default(),
            events: Vec::new(),
            hand_model: hand_model.to_owned(),
            reaper_rot: 0.0,
            swivel_extra: [0.0; 2],
        };
        // bgun_reset: bgun_calculate_blend × 3 per hand.
        for h in [HAND_RIGHT, HAND_LEFT] {
            for _ in 0..3 {
                pose::bgun_calculate_blend(&mut b, gset, rng, h);
            }
        }
        b
    }

    /// `bgun_get_weapon_num` (`:5430`).
    pub fn bgun_get_weapon_num(&self, h: usize) -> u8 {
        if !self.hands[h].inuse {
            WEAPON_NONE
        } else {
            self.ctrl.weaponnum
        }
    }

    /// `FUNCISSEC()` (`constants.h:84`).
    pub fn funcissec(&self) -> bool {
        let w = self.ctrl.weaponnum;
        if !(WEAPON_UNARMED..=WEAPON_COMBATBOOST).contains(&w) {
            return false;
        }
        self.p.gunfuncs[((w - 1) >> 3) as usize] & (1 << ((w - 1) & 7)) != 0
    }

    /// `bgun_is_using_secondary_function` (`:9043`).
    pub fn bgun_is_using_secondary_function(&self) -> bool {
        self.funcissec() != self.ctrl.invertgunfunc
    }

    /// `ammoheldarr[ammotype]`.
    pub fn ammoheld(&self, ammotype: i32) -> i32 {
        if ammotype < 0 {
            return 0;
        }
        self.p.ammoheldarr.get(ammotype as usize).copied().unwrap_or(0)
    }

    /// `bgun_get_ammo_count` (`:9415`): the reserve plus both loaded clips.
    pub fn bgun_get_ammo_count(&self, ammotype: i32) -> i32 {
        let mut total = self.ammoheld(ammotype);
        for h in 0..2 {
            for i in 0..2 {
                if self.ctrl.ammotypes[i] == ammotype && self.hands[h].inuse {
                    total += self.hands[h].loadedammo[i];
                }
            }
        }
        total
    }

    /// `gset_get_gun_zoom_fov` (`gset.c:187`).
    pub fn gset_get_gun_zoom_fov(&self, gset: &Gset) -> f32 {
        match self.bgun_get_weapon_num(HAND_RIGHT) {
            WEAPON_SNIPERRIFLE => self.p.gunzoomfovs[0],
            WEAPON_FARSIGHT => self.p.gunzoomfovs[1],
            w => gset.weapon(w).map_or(0.0, |w| w.aim.zoomfov),
        }
    }

    /// `inv_give_single_weapon` / `inv_give_double_weapon` plus the MP starting
    /// ammo (`g_MpWeapons`), capped at each type's capacity.
    pub fn give_weapon(&mut self, gset: &Gset, weaponnum: u8, double: bool) {
        if let Some(e) = self.p.inventory.iter_mut().find(|(w, _)| *w == weaponnum) {
            e.1 |= double;
        } else {
            self.p.inventory.push((weaponnum, double));
        }
        if let Some(w) = gset.weapon(weaponnum) {
            for (t, q) in w.mp_ammo {
                if t > 0 && (t as usize) < self.p.ammoheldarr.len() {
                    let cap = AMMO_CAPACITY.get(t as usize).copied().unwrap_or(999);
                    self.p.ammoheldarr[t as usize] = (self.p.ammoheldarr[t as usize] + q).min(cap);
                }
            }
        }
    }

    /// `bgun_equip_weapon` (`:5418`).
    pub fn bgun_equip_weapon(&mut self, weaponnum: u8) {
        if self.ctrl.weaponnum == weaponnum && self.ctrl.switchtoweaponnum.is_none() {
            return;
        }
        self.ctrl.switchtoweaponnum = Some(weaponnum);
        self.ctrl.wantammo = false;
    }

    pub(crate) fn inv_has_single(&self, weaponnum: u8) -> bool {
        weaponnum == WEAPON_UNARMED || self.p.inventory.iter().any(|(w, _)| *w == weaponnum)
    }

    pub(crate) fn inv_has_double(&self, weaponnum: u8) -> bool {
        self.p.inventory.iter().any(|(w, d)| *w == weaponnum && *d)
    }

    /// Select a weapon (the keyboard's number keys, a pickup), two of them if
    /// held twice and `dual`.
    pub fn select_weapon(&mut self, weaponnum: u8, dual: bool) {
        self.ctrl.dualwielding = dual && self.inv_has_double(weaponnum);
        self.bgun_equip_weapon(weaponnum);
    }

    /// `inv_remove_item_by_num` (`inv.c`).
    pub fn inv_remove_item_by_num(&mut self, weaponnum: u8) {
        self.p.inventory.retain(|(w, _)| *w != weaponnum);
    }

    /// `bgun_reload_if_possible` (`:5850`).
    pub fn bgun_reload_if_possible(&mut self, gset: &Gset, h: usize) {
        let w = self.bgun_get_weapon_num(h);
        let has_ammo = gset.weapon(w).is_some_and(|w| w.ammos[0].is_some());
        if has_ammo && self.hands[h].modenext == HANDMODE_NONE {
            self.hands[h].modenext = HANDMODE_RELOAD;
        }
    }

    /// `bgun0f0a8c50` (`:9036`): releasing B clears a temporary invert.
    pub fn bgun_release_use(&mut self) {
        if !self.hands[HAND_RIGHT].activatesecondary {
            self.ctrl.invertgunfunc = false;
        }
    }

    /// `bgun_set_adjust_pos` (`:5860`).
    pub fn bgun_set_adjust_pos(&mut self, angle: f32) {
        let z = (1.0 - angle.cos()) * 5.0;
        self.hands[0].adjustpos.z = z;
        self.hands[1].adjustpos.z = z;
    }

    /// `bgun_set_hit_pos` (`:9259`): both hands share it.
    pub fn bgun_set_hit_pos(&mut self, pos: Vec3) {
        self.hands[0].hitpos = pos;
        self.hands[1].hitpos = pos;
    }
}

/// `g_Vars.currentplayer` for the gun code: one player's [`Bgun`], with the
/// shared tables, the world's RNG and timing, the player's camera and the
/// player fields the gun reads.
pub struct GunCtx<'a> {
    pub b: &'a mut Bgun,
    pub gset: &'a Gset,
    pub bank: &'a AnimBank,
    pub models: &'a ModelStore,
    pub rng: &'a mut Rng,
    pub lv: &'a Lv,
    pub cam: &'a Camera,
    pub pl: GunIn,
}

impl GunCtx<'_> {
    pub(crate) fn randomfrac(&mut self) -> f32 {
        self.rng.randomfrac()
    }

    pub(crate) fn weapon(&self, weaponnum: u8) -> Option<&WeaponDef> {
        self.gset.weapon(weaponnum)
    }

    pub(crate) fn func_of(&self, h: usize) -> Option<FuncDef> {
        let hand = &self.b.hands[h];
        self.gset.func(hand.weaponnum, hand.weaponfunc).cloned()
    }

    pub(crate) fn func_by(&self, h: usize, which: usize) -> Option<FuncDef> {
        self.gset.func(self.b.hands[h].weaponnum, which).cloned()
    }

    pub(crate) fn sound(&mut self, id: u16, speed: f32) {
        self.b.events.push(GunEvent::Sound { id, speed, handle: None });
    }

    /// `bgun_get_weapon_num` (`:5430`).
    pub fn bgun_get_weapon_num(&self, h: usize) -> u8 {
        self.b.bgun_get_weapon_num(h)
    }

    pub(crate) fn bgun_is_using_secondary_function(&self) -> bool {
        self.b.bgun_is_using_secondary_function()
    }

    pub(crate) fn funcissec(&self) -> bool {
        self.b.funcissec()
    }

    pub(crate) fn ammoheld(&self, ammotype: i32) -> i32 {
        self.b.ammoheld(ammotype)
    }

    pub(crate) fn bgun_get_ammo_count(&self, ammotype: i32) -> i32 {
        self.b.bgun_get_ammo_count(ammotype)
    }

    /// `gset_get_gun_zoom_fov` (`gset.c:187`).
    pub fn gset_get_gun_zoom_fov(&self) -> f32 {
        self.b.gset_get_gun_zoom_fov(self.gset)
    }
}
