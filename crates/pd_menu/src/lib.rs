//! Perfect Dark's menus, as PD runs them: the dialog stack and its open,
//! populate and redraw animations (`menu.c`), every item widget
//! (`menuitem.c`), the dialog chrome, shimmer comets and the rotating cone
//! background (`menugfx.c`), and the Combat Simulator's ~56 dialogs with their
//! handlers (`mplayer/setup.c`, `scenarios.c`, `mainmenu.c`), driven by PD's
//! input and key-repeat code. Text is `pd_core::text`, models are
//! `pd_core::model`, and everything draws through the CPU RDP (`n64::rdp`)
//! into PD's 320×220 framebuffer.
//!
//! The API is small. Each frame the caller sets up to four N64 controllers
//! ([`MenuSystem::pads`]) and calls [`MenuSystem::frame`] with the frame's
//! [`Lv`]; it then reads the framebuffer ([`Draw::gfx`]), takes the sounds
//! ([`MenuSystem::take_events`]) and takes an [`Outcome`] such as "start this
//! `MatchSetup`".
//!
//! Ground rules (the crate's, and the spike's before it):
//!
//! * **Port functions, not behaviours.** Each PD function is a Rust function
//!   with the same name and a `file:line` citation, in PD's units: 320×220
//!   screen pixels, 60 Hz frames (`diffframe60`), PD's colour words.
//! * **The data is PD's.** The menu and MP tables are generated from the decomp
//!   by `tools/pd-assets/pd_menu_gen.py` ([`generated`]); fonts, strings,
//!   textures, presets and challenges come from `assets/`.
//! * **Where we must substitute, say so at the call site** (`SUBST:`). There is
//!   no Controller Pak and no solo game file ([`mpstate::Profile`]). The music
//!   the menus ask for goes out as `Event::Music` (`pd_core::music`).
//!
//! Module map: [`types`] (the C structs), [`generated`] (the tables),
//! [`gfx`] (`menugfx.c`), [`menu`], [`item`] (`menuitem.c`), [`mpstate`]
//! (`mplayer.c`, `challenge.c`), [`handlers`], [`stubs`] (dialogs outside the
//! Combat Simulator), [`model`] (menu models), [`script`] (the scripted
//! controller the snapshots and goldens use).
//!
//! Source: the old repo's `pd_menu/`. Its `Pd` god object is split here into
//! the menu state ([`MenuSystem`]'s own fields), the MP state
//! ([`MenuSystem::mp`]) and the render state ([`MenuSystem::draw`]).

// Ported functions keep the C's shape (index loops, argument lists, nested ifs,
// branches the C spells out twice, its operator precedence, its NaN-aware
// negated float compares, its float constants digit for digit).
#![allow(
    clippy::if_same_then_else,
    clippy::precedence,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::excessive_precision,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::manual_range_contains,
    clippy::comparison_chain,
    clippy::field_reassign_with_default
)]

pub mod activemenu;
pub mod generated;
pub mod gfx;
pub mod handlers;
pub mod item;
pub mod menu;
pub mod model;
pub mod mpstate;
pub mod script;
pub mod stubs;
pub mod types;

#[cfg(test)]
mod tests;

use n64::pad::{Pad, MAX_PADS};
use n64::rdp::{Addr, Gfx, Texture};
use pd_core::anim::AnimBank;
use pd_core::assets::AssetDir;
use pd_core::events::Event;
use pd_core::lang::Lang;
use pd_core::lv::Lv;
use pd_core::model::draw::TextureCache;
use pd_core::model::ModelStore;
use pd_core::mp::{MatchChr, MatchPlayer, MatchSetup, MatchSimulant};
use pd_core::rng::Rng;
use pd_core::text::{Fonts, TextCtx, TextState};

use menu::{Menu, MenuData};
use model::MenuModelInst;
use mpstate::{MpConfig, MpState, Profile};
use types::*;

/// `FBALLOC_WIDTH_LO` × `FBALLOC_HEIGHT_LO` (constants.h:3654).
pub const FB_W: usize = 320;
pub const FB_H: usize = 220;

/// The `g_Vars` fields the menus touch (varsinit.c:57-71 for the defaults).
/// The frame timing is the caller's [`Lv`].
#[derive(Clone, Debug)]
pub struct Vars {
    pub mpsetupmenu: i32,
    pub mpquickteam: i32,
    pub usingadvsetup: bool,
    pub waitingtojoin: [bool; 4],
    pub mpquickteamnumsims: i32,
    pub mpsimdifficulty: i32,
    pub unk0004a0: i32,
    pub mpplayerteams: [u8; 4],
    pub mphilltime: u8,
    pub unk000498: i32,
    pub screenratio: u8,
    pub screensplit: u8,
}

impl Default for Vars {
    fn default() -> Self {
        Vars {
            mpsetupmenu: 0,
            mpquickteam: generated::MPQUICKTEAM_NONE,
            usingadvsetup: false,
            waitingtojoin: [false; 4],
            mpquickteamnumsims: 1,
            mpsimdifficulty: generated::BOTDIFF_NORMAL,
            unk0004a0: 1,
            mpplayerteams: [0, 1, 2, 3],
            mphilltime: 10,
            unk000498: 0,
            screenratio: 0,
            screensplit: 0,
        }
    }
}

/// What the menus read from `assets/`.
pub struct Resources {
    pub fonts: Fonts,
    pub lang: Lang,
    /// `TEX_GENERAL_MENURAY0` (TEXTURE_01E5, 64×64 IA8, wrap).
    pub menuray0: Texture,
    /// `TEX_GENERAL_ENVSTAR` (TEXTURE_084E, 11×11 IA8, clamp).
    pub envstar: Texture,
    /// `g_BlurBuffer`: 40×30, made by `menugfx_create_blur` from the frame
    /// behind the menu. See [`Resources::blur_from_image`].
    pub blur: Option<Texture>,
    pub mpconfigs: Vec<MpConfig>,
}

fn pool_texture(assets: &AssetDir, num: u16, s: Addr, t: Addr) -> Result<Texture, String> {
    let (w, h, rgba) = assets.read_png(&assets.texture(num))?;
    Ok(Texture::from_rgba8(w, h, &rgba, s, t))
}

impl Resources {
    pub fn load(assets: &AssetDir) -> Result<Resources, String> {
        Ok(Resources {
            fonts: Fonts::load(assets)?,
            lang: Lang::load(assets, "en")?,
            menuray0: pool_texture(assets, 0x01e5, Addr::Wrap, Addr::Wrap)?,
            envstar: pool_texture(assets, 0x084e, Addr::Clamp, Addr::Clamp)?,
            blur: None,
            mpconfigs: mpstate::load_mpconfigs(assets)?,
        })
    }

    /// `menugfx_create_blur` (menugfx.c:45) over a picture of "the game behind
    /// the menu" (`w`×`h` RGBA8): the image is resampled to the 320×220
    /// framebuffer, each 8×8 block averaged in RGB555, into a 40×30 RGBA5551
    /// texture.
    pub fn blur_from_image(&mut self, img: Option<(usize, usize, &[u8])>) {
        let (w, h) = (FB_W, FB_H);
        let sample = |x: usize, y: usize| -> [u32; 3] {
            match img {
                Some((iw, ih, px)) => {
                    let sx = (x * iw / w).min(iw - 1);
                    let sy = (y * ih / h).min(ih - 1);
                    let p = &px[(sy * iw + sx) * 4..];
                    [p[0] as u32 >> 3, p[1] as u32 >> 3, p[2] as u32 >> 3]
                }
                None => {
                    // SUBST: PD blurs the Carrington Institute it is running
                    // (STAGE_CITRAINING) / with no stage behind the menus we blur
                    // a dim gradient, as the spike did (its goldens pin it).
                    let t = y as f32 / h as f32;
                    [(4.0 + 6.0 * t) as u32, (5.0 + 4.0 * t) as u32, (9.0 - 3.0 * t) as u32]
                }
            }
        };
        let mut px = vec![[0.0f32; 4]; 40 * 30];
        for dy in 0..30 {
            for dx in 0..40 {
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for sx in 0..8 {
                    for sy in 0..8 {
                        let (x, y) = (dx * 8 + sx, dy * 8 + sy);
                        // Rows past the 220-line framebuffer read the next buffer
                        // in PD; clamp here.
                        let c = sample(x.min(w - 1), y.min(h - 1));
                        r += c[0];
                        g += c[1];
                        b += c[2];
                    }
                }
                let (r, g, b) = (r / 64, g / 64, b / 64);
                let f = |v: u32| ((v << 3) | (v >> 2)) as f32 / 255.0;
                px[dy * 40 + dx] = [f(r), f(g), f(b), 1.0];
            }
        }
        self.blur = Some(Texture::from_rgba(40, 30, px, Addr::Clamp, Addr::Clamp));
    }
}

/// The render state: the framebuffer, the text renderer's state, what the
/// menus draw with, and each menumodel's live model.
pub struct Draw {
    pub gfx: Gfx,
    pub text: TextState,
    pub res: Resources,
    /// Model files, loaded on first use.
    pub models: ModelStore,
    pub bank: AnimBank,
    pub textures: TextureCache,
    /// Each menumodel's `bodymodel` (players 0-3, then the hudpiece).
    pub model_inst: [Option<MenuModelInst>; 5],
    /// The first model or texture that failed to load.
    pub error: Option<String>,
}

/// What the menus hand back to the game.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// `MENUROOT_START_MP_MATCH` (menutick.c:521): `mp_start_match` has run and
    /// the menus have closed; the game starts this match, then calls
    /// [`MenuSystem::return_from_match`].
    StartMatch(MatchSetup),
    /// `mp_set_paused(mode)` (the pause menu's Pause / Unpause,
    /// `menuhandler_mp_pause`, ingame.c:157): `MPPAUSEMODE_*`.
    SetPaused(u8),
    /// `menuhandler_mp_end_game` (ingame.c:101): player `playernum` aborts
    /// (`aborted = true`) and the match ends (`main_end_stage`).
    EndGame { playernum: usize },
    /// The pause menu's inventory (`menuhandler_inventory_list`'s confirm,
    /// mainmenu.c:4176): player `playernum` equips inventory row `index`.
    Equip { playernum: usize, index: usize },
    /// "Pick Target" chose chr `chrnum` for player `playernum`'s simulants to
    /// attack (`am_pick_target_menu_list`, `bot_apply_attack`).
    PickTarget { playernum: usize, chrnum: usize },
    /// Every end-of-match dialog has closed (menutick.c:615,
    /// `g_MpReturningFromMatch`): the game leaves the match and calls
    /// [`MenuSystem::return_from_match`].
    ReturnFromMatch,
}

/// One row of the pause menu's inventory (`inv_get_name_by_index`), with the
/// weapon's description for the marquee.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InvRow {
    pub weaponnum: u8,
    pub name: String,
    pub description: String,
}

/// A human player in the match, as the pause and end-of-match dialogs read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchViewPlayer {
    /// The player's chr slot (`playerstats.mpindex`, the menu it owns).
    pub slot: usize,
    /// The player's view: left, top, width, height (`viewleft`, ...).
    pub view: [i32; 4],
    pub inventory: Vec<InvRow>,
    /// `inv_get_current_index`.
    pub invcur: i32,
    /// `mp_player_get_weapon_of_choice_name` (title.c:108).
    pub weapon_of_choice: String,
    /// `player->award1` / `award2`, as `g_AwardNames` indexes.
    pub award1: Option<u8>,
    pub award2: Option<u8>,
    /// `player->aborted`: the player chose End Game.
    pub aborted: bool,
}

/// The running match, as the menus read it. PD's pause and end-of-match
/// dialogs read the globals the match writes (`g_MpSetup.paused`, the
/// mpchrconfigs' counters, `g_Vars.players[]`); the game hands them over every
/// frame ([`MenuSystem::set_match_view`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchView {
    /// `g_MpSetup.paused` (`MPPAUSEMODE_*`).
    pub paused: u8,
    /// `g_MainIsEndscreen`.
    pub endscreen: bool,
    /// `lv_get_stage_time60()`.
    pub stagetime60: i32,
    /// The mpchrconfigs' counters, by chr slot.
    pub chrs: [pd_core::mp::MpChrStats; pd_core::mp::MAX_MPCHRS],
    /// The scenario's own counters (`g_ScenarioData`), for the scores.
    pub scenario: pd_core::mp::ScenarioScores,
    /// By player number (`g_Vars.players[i]`).
    pub players: Vec<MatchViewPlayer>,
}

/// The whole of PD's menu system: `g_Menus`, `g_MenuData`, the menu fields of
/// `g_Vars`, the MP state and the render state.
pub struct MenuSystem {
    pub assets: AssetDir,
    /// The frame's timing; the menus read `diffframe60` and `diffframe240`.
    pub lv: Lv,
    /// `g_20SecIntervalFrac`.
    pub frac20: f32,
    pub vars: Vars,
    pub menus: [Menu; 4],
    pub menudata: MenuData,
    pub mpplayernum: usize,
    /// `g_MpNumJoined`.
    pub mp_num_joined: i32,
    pub mp: MpState,
    pub draw: Draw,
    /// `random()`.
    pub rng: Rng,
    /// The four N64 controllers, as the caller read them this frame.
    pub pads: [Pad; MAX_PADS],
    /// The name keyboard's delete (PD's `inputs.back2`), per controller.
    pub back2: [bool; MAX_PADS],
    /// `g_LineHeight` (menuitem.c:37).
    pub line_height: i32,
    /// `g_MenuCThresh` (menu.c:3581).
    pub menu_cthresh: i32,
    /// `g_MenuScissorX1..Y2`.
    pub scissor_menu: [i32; 4],
    /// `g_MpSelectedPlayersForStats`.
    pub mp_selected_for_stats: [usize; 4],
    /// Sounds (and later other events) queued this frame.
    pub events: Vec<Event>,
    pub outcomes: std::collections::VecDeque<Outcome>,
    /// A match is running (`g_Vars.normmplayerisrunning`): the menus are the
    /// pause and end-of-match menus, drawn over the game.
    pub in_match: bool,
    /// The running match, as its dialogs read it.
    pub matchview: MatchView,
    /// The Pick Target rows the match listed, by player number.
    pub picktargets: Vec<Vec<(u8, u8)>>,
}

impl MenuSystem {
    pub fn new(assets: &AssetDir, profile: Profile) -> Result<MenuSystem, String> {
        let mut res = Resources::load(assets)?;
        res.blur_from_image(None);
        let draw = Draw {
            gfx: Gfx::new(FB_W, FB_H),
            text: TextState::default(),
            res,
            models: ModelStore::load(assets)?,
            bank: AnimBank::load(assets)?,
            textures: TextureCache::new(assets),
            model_inst: Default::default(),
            error: None,
        };
        let mut pd = MenuSystem {
            assets: assets.clone(),
            lv: Lv::new(),
            frac20: 0.0,
            vars: Vars::default(),
            menus: std::array::from_fn(|_| Menu::default()),
            menudata: MenuData::default(),
            mpplayernum: 0,
            mp_num_joined: 1,
            mp: MpState { profile, ..MpState::default() },
            draw,
            rng: Rng::new(0x1234_5678),
            pads: [Pad::default(); MAX_PADS],
            back2: [false; MAX_PADS],
            line_height: LINEHEIGHT,
            menu_cthresh: 120,
            scissor_menu: [0, 0, FB_W as i32, FB_H as i32],
            mp_selected_for_stats: [0, 1, 2, 3],
            events: Vec::new(),
            outcomes: std::collections::VecDeque::new(),
            in_match: false,
            matchview: MatchView::default(),
            picktargets: Vec::new(),
        };
        pd.pads[0].connected = true;
        for m in pd.menus.iter_mut() {
            m.menumodel.zoom = -1.0;
        }
        // menu_reset (menu.c:3801): the hudpiece's resting place.
        let hp = &mut pd.menudata.hudpiece;
        hp.newparams = generated::FILE_GHUDPIECE as u32;
        hp.curroty = -std::f32::consts::PI;
        hp.newroty = hp.curroty;
        hp.curposx = -205.5;
        hp.newposx = -205.5;
        hp.curposy = 244.7;
        hp.newposy = 244.7;
        hp.curposz = 68.3;
        hp.newposz = 68.3;
        hp.curscale = 0.12209;
        hp.newscale = 0.12209;
        hp.zoom = -1.0;
        hp.headnum = -1;
        hp.bodynum = -1;
        pd.mp_init();
        Ok(pd)
    }

    /// Open the Perfect Menu (the CI main menu) as PD does after file select.
    pub fn open_main_menu(&mut self) {
        self.mpplayernum = 0;
        // menu_push_root_dialog_and_pause starts the music for this dialog (menu.c:3571).
        self.music_start_menu();
        self.menu_push_root_dialog(&generated::G_CI_MENU_VIA_PC_MENU_DIALOG, MENUROOT_MAINMENU);
    }

    /// Straight into the Combat Simulator (what "Combat Simulator" on the
    /// Perfect Menu does, via `menu_save_and_push_root_dialog`).
    pub fn open_combat_simulator(&mut self) {
        self.mpplayernum = 0;
        self.challenge_determine_unlocked_features();
        self.vars.mpsetupmenu = generated::MPSETUPMENU_GENERAL;
        self.menu_push_root_dialog(&generated::G_COMBAT_SIMULATOR_MENU_DIALOG, MENUROOT_MPSETUP);
        self.play_sound(generated::SFXMAP_8098_EXPLOSION, 1.0, 1.0);
        self.music_start_menu();
    }

    pub fn tc(&mut self) -> TextCtx<'_> {
        TextCtx { gfx: &mut self.draw.gfx, ts: &mut self.draw.text, fonts: &self.draw.res.fonts, frac20: self.frac20 }
    }

    /// `joy_get_connected_controllers` as a bit mask.
    pub fn connected_pads(&self) -> u32 {
        self.pads.iter().enumerate().filter(|(_, p)| p.connected).fold(0, |m, (i, _)| m | 1 << i)
    }

    /// A call into PD's music, for the game's music player.
    pub fn music(&mut self, call: pd_core::music::MusicCall) {
        self.events.push(Event::Music(call));
    }

    /// `menu_choose_music` (`menu.c:5514`) for the Combat Simulator: the menus
    /// outside a match run on CI (`STAGE_CITRAINING`), so the Perfect Menu is
    /// `MUSIC_MAINMENU`. The solo, co-op and counter-op end screens aren't here.
    pub fn menu_choose_music(&self) -> i32 {
        use pd_core::ids::*;
        match self.menudata.root {
            MENUROOT_FILEMGR => MUSIC_MAINMENU,
            MENUROOT_MPSETUP => MUSIC_COMBATSIM_MENU,
            MENUROOT_MPPAUSE => MUSIC_COMBATSIM_COMPLETE,
            _ if self.in_match => MUSIC_COMBATSIM_COMPLETE,
            _ => MUSIC_MAINMENU,
        }
    }

    /// `music_start_menu` (`game/music.c:424`).
    pub fn music_start_menu(&mut self) {
        let t = self.menu_choose_music();
        self.music(pd_core::music::MusicCall::StartTrackAsMenu(t));
    }

    /// Queue a sound: `snd_start(sound)` with PD's pitch and volume, centred.
    pub fn play_sound(&mut self, sound: i32, pitch: f32, volume: f32) {
        self.events.push(Event::Sound { sound: sound as u16, pitch, volume, pan: 0.0 });
    }

    /// One PD frame: `menu_tick` then `menu_render`, at `lv.diffframe60`. Set
    /// [`MenuSystem::pads`] (and [`MenuSystem::back2`]) first.
    pub fn frame(&mut self, lv: &Lv) {
        self.tick(lv);
        self.render();
    }

    /// The input half of [`MenuSystem::frame`]: `menu_tick`. In a match PD
    /// runs it inside `lv_tick`, before the players' controls
    /// (`pd_game::session`), and draws the menus later in the frame.
    pub fn tick(&mut self, lv: &Lv) {
        self.lv = lv.clone();
        self.menu_tick();
    }

    /// The drawing half of [`MenuSystem::frame`]: `menu_render`.
    pub fn render(&mut self) {
        self.menu_render();
        self.back2 = [false; MAX_PADS];
    }

    /// Everything queued since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// The next thing the menus asked of the game, oldest first.
    pub fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcomes.pop_front()
    }

    /// The match as it stands this frame (see [`MatchView`]): the counters go
    /// into the mpchrconfigs, which the dialogs read as PD's do.
    pub fn set_match_view(&mut self, view: MatchView) {
        for (i, stats) in view.chrs.iter().enumerate() {
            if let Some(c) = self.mpchr_mut(i) {
                c.stats = *stats;
            }
        }
        self.matchview = view;
    }

    /// `mp_is_paused` (mplayer.c:1167), as the menus ask it.
    pub fn mp_is_paused(&self) -> bool {
        if self.matchview.players.len() == 1 && self.menus[self.matchview.players[0].slot].curdialog.is_some() {
            return true;
        }
        self.matchview.paused != pd_core::ids::MPPAUSEMODE_UNPAUSED
    }

    /// `mp_push_pause_dialog` (ingame.c:770) for player `playernum`: its
    /// rankings, as a new `MENUROOT_MPPAUSE` root, unless the match is over or
    /// its menu was closed a moment ago (`openinhibit`).
    pub fn mp_push_pause_dialog(&mut self, playernum: usize) {
        if self.matchview.paused == pd_core::ids::MPPAUSEMODE_GAMEOVER || self.matchview.endscreen {
            return;
        }
        let Some(slot) = self.matchview.players.get(playernum).map(|p| p.slot) else { return };
        let prev = self.mpplayernum;
        self.mpplayernum = slot;
        if self.menus[slot].openinhibit == 0 {
            self.menus[slot].playernum = playernum;
            // (g_Vars.normmplayerisrunning: no two-player missions here.)
            if self.mp.setup.options & generated::MPOPTION_TEAMSENABLED as u32 != 0 {
                self.menu_push_root_dialog(&generated::G_MP_PAUSE_TEAM_RANKINGS_MENU_DIALOG, MENUROOT_MPPAUSE);
            } else {
                self.menu_push_root_dialog(&generated::G_MP_PAUSE_PLAYER_RANKING_MENU_DIALOG, MENUROOT_MPPAUSE);
            }
        }
        self.mpplayernum = prev;
    }

    /// `menu_save_and_close_all` for one player's menu (`player_die_by_shooter`
    /// closes a dying player's pause menu, player.c:4817).
    pub fn close_player_menus(&mut self, playernum: usize) {
        let Some(slot) = self.matchview.players.get(playernum).map(|p| p.slot) else { return };
        let prev = self.mpplayernum;
        self.mpplayernum = slot;
        self.menu_save_and_close_all();
        self.mpplayernum = prev;
    }

    /// The menus' half of `mp_end_match` (mplayer.c:2426): the players' files
    /// take `mp_calculate_awards`' results (statistics, medals, title), then
    /// `menu_save_and_push_root_dialog(NULL, MENUROOT_END_MP_MATCH)` opens every
    /// player's end screen on the next tick.
    pub fn mp_end_match(&mut self, results: &[pd_core::mp::MpPlayerResult]) {
        for r in results {
            if let Some(p) = self.mp.players.get_mut(r.slot) {
                p.career = r.career;
                p.medals = r.medals;
                p.title = r.title;
            }
        }
        self.menu_save_and_push_root_dialog(None, MENUROOT_END_MP_MATCH);
    }

    /// `mp_push_endscreen_dialog` (ingame.c:802): player `playernum`'s (chr
    /// slot `slot`'s) Game Over, or a challenge's verdict, then the offer to
    /// save a new player once per player.
    pub fn mp_push_endscreen_dialog(&mut self, playernum: usize, slot: usize) {
        let prev = self.mpplayernum;
        self.mpplayernum = slot;
        self.menus[slot].playernum = playernum;
        if self.mp.setup.options & generated::MPOPTION_TEAMSENABLED as u32 != 0 {
            if self.mp.bossfile.locktype == generated::MPLOCKTYPE_CHALLENGE as u8 {
                // (No cheats.)
                if self.challenge_is_complete_for_endscreen() {
                    self.menu_push_root_dialog(&generated::G_MP_ENDSCREEN_CHALLENGE_COMPLETED_MENU_DIALOG, MENUROOT_MPENDSCREEN);
                } else {
                    self.menu_push_root_dialog(&generated::G_MP_ENDSCREEN_CHALLENGE_FAILED_MENU_DIALOG, MENUROOT_MPENDSCREEN);
                }
            } else {
                self.menu_push_root_dialog(&generated::G_MP_ENDSCREEN_TEAM_GAME_OVER_MENU_DIALOG, MENUROOT_MPENDSCREEN);
            }
        } else {
            self.menu_push_root_dialog(&generated::G_MP_ENDSCREEN_IND_GAME_OVER_MENU_DIALOG, MENUROOT_MPENDSCREEN);
        }
        // SUBST: PD asks once whether to save a new player (it has no file
        // yet, `fileguid` 0) / there are no player files here, so the offer
        // always comes once per player per run; saving shows the pak stub.
        let pl = &mut self.mp.players[slot];
        if pl.options & pd_core::ids::OPTION_ASKEDSAVEPLAYER as u32 == 0 && pl.fileid == 0 {
            pl.options |= pd_core::ids::OPTION_ASKEDSAVEPLAYER as u32;
            self.menu_push_dialog(&generated::G_MP_ENDSCREEN_SAVE_PLAYER_MENU_DIALOG);
        }
        self.mpplayernum = prev;
    }

    /// The menu half of `menu_reset` (menu.c:3734), which a stage load runs: no
    /// dialogs, no root, no background, no hudpiece.
    pub fn menu_reset(&mut self) {
        self.menudata.hudpieceactive = false;
        self.menudata.triggerhudpiece = false;
        for m in self.menus.iter_mut() {
            m.curdialog = None;
            m.depth = 0;
            m.numdialogs = 0;
            m.rowend = 0;
            m.blockend = 0;
            m.colend = 0;
        }
        self.menudata.nextdialog = None;
        self.menudata.nextroot = -1;
        self.menudata.count = 0;
        self.menudata.root = 0;
        self.menudata.bgopacityfrac = 0.0;
        self.menudata.bg = 0;
        self.menudata.checkroots = false;
        self.menudata.nextbg = 255;
    }

    /// `MENUROOT_START_MP_MATCH` (menutick.c:521): `mp_start_match`
    /// (mplayer.c:141) and `menu_stop`. The match itself is the caller's: the
    /// setup goes out as [`Outcome::StartMatch`].
    pub fn start_match(&mut self) {
        handlers::mp_configure_quick_team_simulants(self);
        if !self.challenge_is_feature_unlocked(generated::MPFEATURE_ONEHITKILLS) {
            self.mp.setup.options &= !(generated::MPOPTION_ONEHITKILLS as u32);
        }
        if !self.challenge_is_feature_unlocked(generated::MPFEATURE_SLOWMOTION) {
            self.mp.setup.options &= !((generated::MPOPTION_SLOWMOTION_ON | generated::MPOPTION_SLOWMOTION_SMART) as u32);
        }
        let mut stagenum = self.mp.setup.stagenum as i32;
        if stagenum == pd_core::ids::STAGE_MP_RANDOM as i32 {
            stagenum = self.mp_choose_random_stage();
        }
        let setup = self.match_setup(stagenum as u8);
        log::info!("pd_menu: starting a match: {}", self.describe_match(&setup).join(" | "));
        self.outcomes.push_back(Outcome::StartMatch(setup));
        for i in 0..4 {
            self.mpplayernum = i;
            self.menu_save_and_close_all();
        }
        self.mpplayernum = 0;
        // The arena loads (menu_reset); the match's mpchrconfig counters start
        // at zero (mp_reset_mpchrconfig_for_match, mp_reset).
        self.menu_reset();
        for i in 0..pd_core::mp::MAX_MPCHRS {
            if let Some(c) = self.mpchr_mut(i) {
                c.stats = pd_core::mp::MpChrStats::default();
            }
        }
        self.matchview = MatchView::default();
        self.in_match = true;
    }

    /// `mp_choose_random_stage` (setup.c:130): one of the first 16 arenas that
    /// is unlocked, by `random()`.
    pub fn mp_choose_random_stage(&mut self) -> i32 {
        let arenas = &generated::MP_ARENAS[..16];
        let n = arenas.iter().filter(|a| self.challenge_is_feature_unlocked(a.requirefeature)).count() as u32;
        let mut index = self.rng.random() % n.max(1);
        for a in arenas {
            if self.challenge_is_feature_unlocked(a.requirefeature) {
                if index == 0 {
                    return a.stagenum;
                }
                index -= 1;
            }
        }
        pd_core::ids::STAGE_MP_SKEDAR as i32
    }

    /// The match's weapon set as `WEAPON_*` (`g_MpWeapons[slot].weaponnum` of
    /// each of the six slots; nothing and the shield, which isn't a gun, left out).
    pub fn weapon_set_weaponnums(weapons: &[u8; 6]) -> Vec<u8> {
        weapons
            .iter()
            .filter_map(|&w| generated::MP_WEAPONS.get(w as usize))
            .map(|w| w.weaponnum)
            .filter(|&n| n > 0 && n != pd_core::ids::WEAPON_MPSHIELD as i32)
            .map(|n| n as u8)
            .collect()
    }

    /// `g_MpSetup` and the chrs in its slots, as a [`MatchSetup`].
    pub fn match_setup(&self, stagenum: u8) -> MatchSetup {
        let s = &self.mp.setup;
        let chr = |c: &mpstate::MpChrConfig| MatchChr { name: c.name.trim_end().to_string(), mpbodynum: c.mpbodynum, mpheadnum: c.mpheadnum, team: c.team, displayoptions: c.displayoptions as u8 };
        let players = (0..4)
            .filter(|&i| s.chrslots & (1 << i) != 0)
            .map(|i| {
                let p = &self.mp.players[i];
                MatchPlayer { slot: i as u8, chr: chr(&p.base), controlmode: p.controlmode, options: p.options as u16, handicap: p.handicap as u8, career: p.career }
            })
            .collect();
        let simulants = (0..8)
            .filter(|&i| s.chrslots & (1 << (i + 4)) != 0)
            .map(|i| {
                let b = &self.mp.bots[i];
                MatchSimulant { slot: (i + 4) as u8, chr: chr(&b.base), bottype: b.ty, difficulty: b.difficulty }
            })
            .collect();
        MatchSetup {
            stagenum,
            scenario: s.scenario,
            options: s.options,
            timelimit: s.timelimit,
            scorelimit: s.scorelimit,
            teamscorelimit: s.teamscorelimit,
            weapons: s.weapons,
            players,
            simulants,
            teamnames: self.mp.bossfile.teamnames.iter().map(|t| t.trim_end().to_string()).collect(),
            challenge: self.mp.bossfile.locktype == generated::MPLOCKTYPE_CHALLENGE as u8,
            mphilltime: self.vars.mphilltime,
            screensplit: self.vars.screensplit,
            music: self.match_music(),
        }
    }

    /// The Soundtrack settings, over the unlocked tracks in slot order.
    pub fn match_music(&self) -> pd_core::mp::MatchMusic {
        let tracks = (0..self.mp_get_num_unlocked_tracks())
            .map(|slot| {
                let t = self.mp_get_track_num_at_slot_index(slot);
                let row = &generated::MP_TRACKS[t];
                pd_core::mp::MatchTrack { tracknum: t as i32, musicnum: row.musicnum, duration: row.duration, enabled: self.mp_is_multi_track_slot_enabled(slot) }
            })
            .collect();
        pd_core::mp::MatchMusic { slot: self.mp_get_current_track_slot_num(), usingmultipletunes: self.mp.bossfile.usingmultipletunes, tracks }
    }

    /// A match as text lines, in the menus' own words (the log, and the game's
    /// stand-in screen until the match exists).
    pub fn describe_match(&self, m: &MatchSetup) -> Vec<String> {
        let mut lines = Vec::new();
        let scen = self.lang(generated::MP_SCENARIO_OVERVIEWS[m.scenario as usize % 6].name);
        let arena = generated::MP_ARENAS.iter().find(|a| a.stagenum == m.stagenum as i32).map(|a| self.lang(a.name)).unwrap_or_default();
        lines.push(format!("Scenario: {}", scen.trim()));
        lines.push(format!("Arena: {}", arena.trim()));
        // A slot holds a g_MpWeapons index; the label takes the option index
        // among the unlocked weapons (mp_get_weapon_slot, mplayer.c:928).
        let option = |w: u8| generated::MP_WEAPONS[..(w as usize).min(generated::MP_WEAPONS.len())].iter().filter(|x| self.challenge_is_feature_unlocked(x.unlockfeature)).count() as i32;
        let weps: Vec<String> = m.weapons.iter().map(|&w| self.mp_get_weapon_label(option(w)).trim().to_string()).collect();
        lines.push(format!("Weapons: {}", weps.join(", ")));
        let time = m.time_limit_minutes().map_or("No Limit".into(), |t| format!("{t} min"));
        let score = m.score_limit().map_or("No Limit".into(), |s| s.to_string());
        lines.push(format!("Time: {time}   Score: {score}"));
        let team = |c: &MatchChr| if m.teams_enabled() { format!(" [{}]", m.teamnames.get(c.team as usize & 7).map_or("", |t| t.as_str())) } else { String::new() };
        for p in &m.players {
            lines.push(format!("Player {}: {} - {}{}", p.slot + 1, p.chr.name, self.mp_get_body_name(p.chr.mpbodynum as usize).trim(), team(&p.chr)));
        }
        for s in &m.simulants {
            let d = if (s.difficulty as i32) < generated::BOTDIFF_DISABLED { self.lang(pd_core::lang::tx(generated::B_MISC, 82).add(s.difficulty as i32)) } else { String::new() };
            lines.push(format!("Sim ({}): {} - {}{}", d.trim(), s.chr.name, self.mp_get_body_name(s.chr.mpbodynum as usize).trim(), team(&s.chr)));
        }
        lines
    }

    /// Back from the match (menutick.c:217, `g_MpReturningFromMatch`).
    pub fn return_from_match(&mut self) {
        // The CI stage loads (menu_reset), then g_MpReturningFromMatch reopens
        // the Combat Simulator (the blur behind its menus is CI's again).
        self.menu_reset();
        self.draw.res.blur_from_image(None);
        self.in_match = false;
        self.mp_num_joined = 0;
        self.vars.mpsetupmenu = if self.vars.usingadvsetup { generated::MPSETUPMENU_ADVSETUP } else { generated::MPSETUPMENU_GENERAL };
        // mp_start_match turned quick-team sims into real ones; PD reloads the
        // setup at match end, and the quick team rebuilds them next time.
        if self.vars.mpquickteam != generated::MPQUICKTEAM_NONE {
            for i in 0..8 {
                self.mp_remove_simulant(i);
            }
        }
        for i in 0..4 {
            self.vars.waitingtojoin[i] = false;
            if self.mp.setup.chrslots & (1 << i) != 0 {
                self.mpplayernum = i;
                if self.vars.mpsetupmenu == generated::MPSETUPMENU_ADVSETUP {
                    self.mp_num_joined += 1;
                    self.mp_open_advanced_setup(true);
                } else if self.mp_num_joined == 0 {
                    self.mp_num_joined += 1;
                    self.menu_push_root_dialog(&generated::G_COMBAT_SIMULATOR_MENU_DIALOG, MENUROOT_MPSETUP);
                } else {
                    self.vars.waitingtojoin[i] = true;
                }
            }
        }
        self.mpplayernum = 0;
        if self.menus.iter().all(|m| m.curdialog.is_none()) {
            self.open_combat_simulator();
        }
        self.play_sound(generated::SFXMAP_8098_EXPLOSION, 1.0, 1.0);
        // player_pause(MENUROOT_MPSETUP): the setup opens, paused, and its
        // music starts (player.c:2208).
        self.music_start_menu();
    }

    /// Switch the pretend save file and re-derive the unlocks.
    pub fn set_profile(&mut self, profile: Profile) {
        self.mp.profile = profile;
        self.challenges_init();
    }
}
