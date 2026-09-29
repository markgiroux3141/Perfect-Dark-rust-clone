//! The menu handlers and text functions the generated tables name, ported
//! from `mplayer/setup.c`, `mplayer/scenarios.c` (+ the scenario `.inc`
//! files), `mainmenu.c` and `mplayer/ingame.c`, one function each under PD's
//! name. The file manager's (`filemgr.c`) are [`super::filemgr`]'s, re-exported
//! here for the tables.

#![allow(clippy::too_many_arguments)]

pub use super::filemgr::*;
use super::stubs;
use super::generated::*;
use pd_core::ids::{FILEOP_LOAD_MPPLAYER, FILEOP_LOAD_MPSETUP, FILEOP_SAVE_MPPLAYER, FILEOP_SAVE_MPSETUP, FILETYPE_MPPLAYER, FILETYPE_MPSETUP, MODFILE_GAME, MODFILE_MPSETUP};
use pd_core::savebuffer::{cstr_to_string, savebuffer_bitstring_to_cstring, FileGuid};
use pd_core::lang::tx;
use super::types::*;
use super::MenuSystem;

type R = HRet;

fn ok() -> R {
    HRet::I(0)
}

fn cur_player(pd: &MenuSystem) -> usize {
    pd.mpplayernum
}

fn slot(pd: &MenuSystem) -> usize {
    pd.mr().mpsetup.slotindex.clamp(0, 7) as usize
}

// ---------------------------------------------------------------------------
// setup.c:41-120
// ---------------------------------------------------------------------------

pub fn menuhandler_mp_drop_out(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
        pd.menu_pop_dialog();
    }
    ok()
}

pub fn mp_get_current_player_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    pd.mp.players[cur_player(pd)].base.name.clone()
}

pub fn menuhandler_mp_teams_label(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_DISABLED && pd.mp.setup.options & MPOPTION_TEAMSENABLED as u32 == 0 {
        return HRet::I(1);
    }
    ok()
}

/// `mp_arena_menu_handler` (setup.c:157).
///
/// `// SUBST:` PD lists `g_MpArenas` in three groups (Dark, Classic,
/// Random) / the custom levels (not PD: `pd_import`'s conversions) follow in a
/// fourth group, "Custom", when there are any.
pub fn mp_arena_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let groups: [(i32, u16); 3] = [(0, 116), (13, 117), (16, 118)];
    let unlocked: Vec<usize> = (0..MP_ARENAS.len()).filter(|&i| pd.challenge_is_feature_unlocked(MP_ARENAS[i].requirefeature)).collect();
    let no_classic = !pd.challenge_is_feature_unlocked(MPFEATURE_STAGE_COMPLEX) && !pd.challenge_is_feature_unlocked(MPFEATURE_STAGE_TEMPLE) && !pd.challenge_is_feature_unlocked(MPFEATURE_STAGE_FELICITY);
    // The custom group follows PD's (index `pdgroups`), its rows after PD's.
    let pdgroups = if no_classic { 2 } else { 3 };
    let custom = !pd.custom_arenas.is_empty();
    let is_custom_group = custom && data.value == pdgroups;
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = (unlocked.len() + pd.custom_arenas.len()) as i32,
        MENUOP_GET_OPTION_TEXT => {
            let k = data.value.max(0) as usize;
            if let Some(&i) = unlocked.get(k) {
                return pd.lang(MP_ARENAS[i].name).into();
            }
            if let Some(c) = pd.custom_arenas.get(k - unlocked.len()) {
                return c.name.clone().into();
            }
        }
        MENUOP_CONFIRM => {
            // PD walks to index `data.value` among the unlocked arenas (or past the
            // end, which is the "Random" row's index 16 when all are unlocked).
            let k = data.value.max(0) as usize;
            if let Some(c) = k.checked_sub(unlocked.len()).and_then(|j| pd.custom_arenas.get(j)) {
                pd.mp.setup.stagenum = c.stagenum;
                return ok();
            }
            let i = unlocked.get(k).copied().unwrap_or(MP_ARENAS.len() - 1);
            pd.mp.setup.stagenum = MP_ARENAS[i].stagenum as u8;
        }
        MENUOP_GET_SELECTED_INDEX => {
            let mut count = 0;
            for a in MP_ARENAS.iter() {
                if pd.mp.setup.stagenum as i32 == a.stagenum {
                    data.value = count;
                }
                if pd.challenge_is_feature_unlocked(a.requirefeature) {
                    count += 1;
                }
            }
            if let Some(j) = pd.custom_arenas.iter().position(|c| c.stagenum == pd.mp.setup.stagenum) {
                data.value = (unlocked.len() + j) as i32;
            }
        }
        MENUOP_GET_OPTGROUP_COUNT => {
            data.value = 3;
            if no_classic {
                data.value -= 1;
            }
            if custom {
                data.value += 1;
            }
        }
        MENUOP_GET_OPTGROUP_TEXT if is_custom_group => return "Custom\n".to_string().into(),
        MENUOP_GET_OPTGROUP_START_INDEX if is_custom_group => data.groupstartindex = unlocked.len() as i32,
        MENUOP_GET_OPTGROUP_TEXT => {
            let mut count = data.value;
            if no_classic && count > 0 {
                count += 1;
            }
            return pd.lang(tx(B_MPMENU, groups[count.clamp(0, 2) as usize].1)).into();
        }
        MENUOP_GET_OPTGROUP_START_INDEX => {
            let mut groupindex = data.value;
            if no_classic && groupindex == 1 {
                groupindex += 1;
            }
            let off = groups[groupindex.clamp(0, 2) as usize].0;
            data.groupstartindex = (0..off as usize).filter(|&i| pd.challenge_is_feature_unlocked(MP_ARENAS[i].requirefeature)).count() as i32;
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_control_style` (setup.c:255).
pub fn menuhandler_mp_control_style(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let labels = [239u16, 240, 241, 242];
    let p = cur_player(pd);
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = 4,
        MENUOP_GET_OPTION_TEXT => return pd.lang(tx(B_OPTIONS, labels[data.value.clamp(0, 3) as usize])).into(),
        MENUOP_CONFIRM => pd.mp.players[p].controlmode = data.value as u8,
        MENUOP_GET_SELECTED_INDEX => data.value = pd.mp.players[p].controlmode as i32,
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_weapon_slot` (setup.c:281).
pub fn menuhandler_mp_weapon_slot(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let s = item.param3.num().clamp(0, 5) as usize;
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = pd.mp_get_num_weapon_options(),
        MENUOP_GET_OPTION_TEXT => return pd.mp_get_weapon_label(data.value).into(),
        MENUOP_CONFIRM => pd.mp_set_weapon_slot(s, data.value),
        MENUOP_GET_SELECTED_INDEX => data.value = pd.mp_get_weapon_slot(s),
        _ => {}
    }
    ok()
}

pub fn mp_menu_text_weapon_name_for_slot(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let s = pd.mp_get_weapon_slot(item.param.clamp(0, 5) as usize);
    pd.mp_get_weapon_label(s)
}

/// `menuhandler_mp_weapon_set_dropdown` (setup.c:304).
pub fn menuhandler_mp_weapon_set_dropdown(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = pd.mp_get_num_weaponset_slots(item.param != 0),
        MENUOP_GET_OPTION_TEXT => return pd.mp_get_weaponset_name_by_slotnum(data.value).into(),
        MENUOP_CONFIRM => pd.mp_set_weaponset_slotnum(data.value),
        MENUOP_GET_SELECTED_INDEX => data.value = pd.mp_get_weaponset_slotnum(),
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_control_checkbox` (setup.c:323).
pub fn menuhandler_mp_control_checkbox(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = cur_player(pd);
    let bit = item.param3.num() as u32;
    match op {
        MENUOP_IS_CHECKED => {
            let set = pd.mp.players[p].options & bit != 0;
            if bit == OPTION_FORWARDPITCH as u32 {
                return (!set).into();
            }
            return set.into();
        }
        MENUOP_CONFIRM => {
            let val = OPTION_FORWARDPITCH as u32;
            if bit == val {
                data.value = if data.value == 0 { val as i32 } else { 0 };
            }
            pd.mp.players[p].options &= !bit;
            if data.value != 0 {
                pd.mp.players[p].options |= bit;
            }
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_aim_control` (setup.c:360).
pub fn menuhandler_mp_aim_control(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let labels = [213u16, 214];
    let p = cur_player(pd);
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = 2,
        MENUOP_GET_OPTION_TEXT => return pd.lang(tx(B_MPMENU, labels[data.value.clamp(0, 1) as usize])).into(),
        MENUOP_CONFIRM => pd.mp.players[p].aimcontrol = data.value as u8,
        MENUOP_GET_SELECTED_INDEX => data.value = pd.mp.players[p].aimcontrol as i32,
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_checkbox_option` (setup.c:389).
pub fn menuhandler_mp_checkbox_option(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let bit = item.param3.num() as u32;
    match op {
        MENUOP_IS_CHECKED => return (pd.mp.setup.options & bit != 0).into(),
        MENUOP_CONFIRM => {
            pd.mp.setup.options &= !bit;
            if data.value != 0 {
                pd.mp.setup.options |= bit;
            }
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_teams_enabled` (setup.c:407).
pub fn menuhandler_mp_teams_enabled(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    if op == MENUOP_IS_DISABLED {
        let s = pd.mp.setup.scenario as i32;
        return (s == MPSCENARIO_CAPTURETHECASE || s == MPSCENARIO_KINGOFTHEHILL).into();
    }
    menuhandler_mp_checkbox_option(pd, op, item, data)
}

/// `menuhandler_mp_display_option_checkbox` (setup.c:421).
pub fn menuhandler_mp_display_option_checkbox(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = cur_player(pd);
    let bit = item.param3.num() as u32 & 0xff;
    match op {
        MENUOP_IS_CHECKED => return (pd.mp.players[p].base.displayoptions & bit != 0).into(),
        MENUOP_CONFIRM => {
            pd.mp.players[p].base.displayoptions &= !bit;
            if data.value != 0 {
                pd.mp.players[p].base.displayoptions |= bit;
            }
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_confirm_save_chr` (setup.c:441): where to save a new
/// player file.
pub fn menuhandler_mp_confirm_save_chr(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
        pd.filemgr_push_select_location_dialog(6, FILETYPE_MPPLAYER);
    }
    ok()
}

fn kb_from(s: &str) -> [u8; 11] {
    let mut out = [0u8; 11];
    for (i, c) in s.bytes().take_while(|&c| c != b'\n' && c != 0).take(11).enumerate() {
        out[i] = c;
    }
    out
}

fn kb_to(s: &[u8; 11]) -> String {
    let end = s.iter().position(|&c| c == 0).unwrap_or(11);
    s[..end].iter().map(|&b| b as char).collect()
}

/// `menuhandler_mp_setup_name` (setup.c:451).
pub fn menuhandler_mp_setup_name(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_KEYBOARD_STRING => data.string = kb_from(&pd.mp.setup.name),
        MENUOP_SET_KEYBOARD_STRING => pd.mp.setup.name = kb_to(&data.string),
        MENUOP_CONFIRM => pd.filemgr_push_select_location_dialog(7, FILETYPE_MPSETUP),
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_save_setup_overwrite` (setup.c:467).
pub fn menuhandler_mp_save_setup_overwrite(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
        let guid = pd.mp.setup.fileguid;
        pd.filemgr_save_or_load(guid, FILEOP_SAVE_MPSETUP, 0);
    }
    ok()
}

pub fn menuhandler_mp_save_setup_copy(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_pop_dialog();
        pd.menu_push_dialog(&G_MP_SAVE_SETUP_NAME_MENU_DIALOG);
    }
    ok()
}

pub fn mp_menu_text_setup_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    pd.mp.setup.name.clone()
}

// ---------------------------------------------------------------------------
// Character select (setup.c:568-690, :1881-1980)
// ---------------------------------------------------------------------------

/// `mp_character_body_menu_handler` (setup.c:568).
fn mp_character_body_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData, mpbodynum: i32, mpheadnum: i32, _isplayer: bool) -> R {
    let diff60 = pd.lv.diffframe60;
    let diff60f = pd.lv.diffframe60f;
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = pd.mp_get_num_bodies(),
        MENUOP_ON_CAROUSEL_TICK => {
            let mm = &mut pd.m().menumodel;
            mm.newanimnum = ANIM_01FC;
            mm.newparams = 0xffff | ((mpheadnum as u32 & 0xff) << 16) | ((mpbodynum as u32 & 0xff) << 24);
            mm.zoomtimer60 += diff60;
            if mm.zoomtimer60 > 480 {
                mm.zoomtimer60 -= 480;
            }
            if mm.rottimer60 > 0 {
                mm.rottimer60 -= diff60;
            } else {
                let v = mm.curroty + 0.01 * diff60f;
                mm.newroty = v;
                mm.curroty = v;
            }
            mm.hideheadparts = false;
            mm.zoom = 30.0;
        }
        MENUOP_IS_CAROUSEL_OPTION_HIDDEN => {
            let f = pd.mp_get_body_required_feature(data.value.max(0) as usize);
            if !pd.challenge_is_feature_unlocked(f) {
                return HRet::I(1);
            }
        }
        MENUOP_ON_FOCUS => pd.m().menumodel.loaddelay = 3,
        MENUOP_GET_SELECTED_INDEX => data.value = mpbodynum,
        MENUOP_CONFIRM | MENUOP_IS_PREFOCUSED => {
            pd.m().menumodel.removingpiece = false;
            pd.menu_configure_model(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, MENUMODELFLAG_HASSCALE);
            let mm = &mut pd.m().menumodel;
            mm.curposx = 8.2;
            mm.newposx = 8.2;
            mm.curposy = -4.1;
            mm.newposy = -4.1;
            mm.curscale = 0.002;
            mm.curroty = -0.2;
            mm.newroty = -0.2;
            mm.rottimer60 = 60;
            mm.zoomtimer60 = 120;
            mm.loaddelay = 8;
            if op == MENUOP_IS_PREFOCUSED {
                mm.loaddelay = 16;
            }
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_character_body` (setup.c:644).
pub fn menuhandler_mp_character_body(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = cur_player(pd);
    match op {
        MENUOP_CONFIRM => {
            if (pd.mp.players[p].base.mpheadnum as i32) < pd.mp_get_num_heads() && data.unk04 == 0 {
                pd.mp.players[p].base.mpheadnum = pd.mp_get_mpheadnum_by_mpbodynum(data.value) as u8;
            }
            pd.mp.players[p].base.mpbodynum = data.value as u8;
            mp_restart_character_body_label_timer(pd);
        }
        MENUOP_IS_PREFOCUSED => {
            let (b, h) = (pd.mp.players[p].base.mpbodynum as i32, pd.mp.players[p].base.mpheadnum as i32);
            mp_character_body_menu_handler(pd, op, item, data, b, h, true);
            return HRet::I(1);
        }
        _ => {}
    }
    let (b, h) = (pd.mp.players[p].base.mpbodynum as i32, pd.mp.players[p].base.mpheadnum as i32);
    mp_character_body_menu_handler(pd, op, item, data, b, h, true)
}

/// `menudialog_mp_human_character` (setup.c:675).
pub fn menudialog_mp_human_character(pd: &mut MenuSystem, op: i32, def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_TICK {
        let m = pd.mr();
        if let Some(cd) = m.curdialog {
            let d = m.dialogs[cd];
            if std::ptr::eq(d.def(), def) && d.focuseditem != Some(1) && d.focuseditem != Some(2) {
                let mut hd = HandlerData::default();
                menuhandler_mp_character_body(pd, MENUOP_ON_CAROUSEL_TICK, &def.items[2], &mut hd);
            }
        }
    }
    0
}

/// `mp_challenges_list_handler` (setup.c:692): the completed-challenges list.
pub fn mp_challenges_list_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = pd.challenge_get_auto_focused_index(cur_player(pd)),
        MENUOP_RENDER => {
            let Some(rd) = data.render else { return ok() };
            let ci = data.unk04.max(0) as usize;
            let name = pd.challenge_get_name(ci.min(29));
            let (vw, vh) = (pd.draw.gfx.w as i32, pd.draw.gfx.h as i32);
            let (mut x, mut y) = (rd.x + 10, rd.y + 1);
            pd.tc().render_v2(&mut x, &mut y, &name, pd_core::text::FontId::Sm, rd.colour, vw, vh, 0, 0);
            let mut loopx = 10;
            let size = 11;
            for i in 0..4 {
                let done = pd.challenge_is_completed_by_player_with_num_players(cur_player(pd), ci.min(29), i + 1);
                let env = if done { 0xb2efff00 | (rd.colour & 0xff) * 255 / 256 } else { 0x30407000 | (rd.colour & 0xff) * 255 / 256 };
                pd.draw_star(rd.x + loopx, rd.y + size, size, env, true);
                loopx += 13;
            }
        }
        MENUOP_GET_OPTION_HEIGHT => data.value = 26,
        _ => {}
    }
    ok()
}

// ---------------------------------------------------------------------------
// Statistics (setup.c:787-1030)
// ---------------------------------------------------------------------------

macro_rules! stat_text {
    ($name:ident, $field:ident) => {
        pub fn $name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
            format!("{}\n", pd.mp.players[cur_player(pd)].career.$field)
        }
    };
}
stat_text!(mp_menu_text_kills, kills);
stat_text!(mp_menu_text_deaths, deaths);
stat_text!(mp_menu_text_games_played, gamesplayed);
stat_text!(mp_menu_text_games_won, gameswon);
stat_text!(mp_menu_text_games_lost, gameslost);
stat_text!(mp_menu_text_head_shots, headshots);
stat_text!(mp_menu_text_medal_accuracy, accuracymedals);
stat_text!(mp_menu_text_medal_head_shot, headshotmedals);
stat_text!(mp_menu_text_medal_kill_master, killmastermedals);
stat_text!(mp_menu_text_medal_survivor, survivormedals);

pub fn mp_menu_text_ammo_used(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let mut value = pd.mp.players[cur_player(pd)].career.ammoused;
    if value > 100000 {
        value /= 1000;
        if value > 100000 {
            value /= 1000;
            return format!("{value}M\n");
        }
        return format!("{value}K\n");
    }
    format!("{value}\n")
}

pub fn mp_menu_text_distance(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    format!("{:.1}km\n", pd.mp.players[cur_player(pd)].career.distance as f32 / 10.0)
}

pub fn mp_menu_text_time(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let raw = pd.mp.players[cur_player(pd)].career.time;
    if raw == 0 {
        return "--:--\n".into();
    }
    if raw >= 0x7fffffff {
        return "==:==\n".into();
    }
    let secs = raw % 60;
    let mins = raw / 60;
    let hours = mins / 60;
    let days = hours / 24;
    if days == 0 {
        format!("{}:{:02}.{:02}", hours % 24, mins % 60, secs)
    } else {
        format!("{}:{:02}:{:02}", days, hours % 24, mins % 60)
    }
}

pub fn mp_menu_text_accuracy(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    format!("{:.1}%", pd.mp.players[cur_player(pd)].career.accuracy as f32 / 10.0)
}

/// `mp_format_damage_value` (setup.c:913, NTSC 1.0+).
fn mp_format_damage_value(damage: f32) -> String {
    if damage < 1000.0 {
        format!("{damage:.1}")
    } else if damage < 10000.0 {
        format!("{damage:.0}")
    } else if damage < 100000.0 {
        format!("{:.1}K", damage / 1000.0)
    } else if damage < 1000000.0 {
        format!("{:.0}K", damage / 1000.0)
    } else if damage < 10000000.0 {
        format!("{:.1}M", damage / 1000000.0)
    } else {
        format!("{:.0}M", damage / 1000000.0)
    }
}

pub fn mp_menu_text_pain_received(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    mp_format_damage_value(pd.mp.players[cur_player(pd)].career.painreceived as f32 / 10.0)
}

pub fn mp_menu_text_damage_dealt(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    mp_format_damage_value(pd.mp.players[cur_player(pd)].career.damagedealt as f32 / 10.0)
}

/// `mp_medal_menu_handler` (setup.c:957): the medal star after each count.
pub fn mp_medal_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    if op == MENUOP_RENDER {
        let Some(rd) = data.render else { return ok() };
        let mut colour: u32 = match item.param {
            0 => 0xff7f7fff,
            1 => 0xbfbf00ff,
            2 => 0x00ff00ff,
            _ => 0x00bfbfff,
        };
        colour = (colour & 0xffffff00) | ((colour & 0xff) * (rd.colour & 0xff)) >> 8;
        pd.draw_star(rd.x + 9, rd.y, 11, colour, true);
    }
    ok()
}

pub fn title_mp_menu_title_stats_for_player_name(pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    let fmt = pd.lang(tx(B_MPMENU, 145));
    let name = pd.mp.players[cur_player(pd)].base.name.clone();
    fmt.replacen("%s", name.trim_end_matches('\n'), 1)
}

pub fn menuhandler_mp_username_password(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && pd.mp.players[cur_player(pd)].title as i32 != MPPLAYERTITLE_PERFECT {
        return HRet::I(1);
    }
    ok()
}

/// `mp_menu_text_username_password` (setup.c:1575): PD de-obfuscates these at runtime.
pub fn mp_menu_text_username_password(_pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    if item.param == 0 {
        "EnTROpIcDeCAy\n".into()
    } else {
        "ZeRo-Tau\n".into()
    }
}

/// `mp_menu_text_player_title` (ingame.c:718).
pub fn mp_menu_text_player_title(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let t = pd.mp.players[cur_player(pd)].title as i32;
    pd.lang(tx(B_MISC, 185).add(t))
}

// ---------------------------------------------------------------------------
// Head select, name, load/save (setup.c:1881-2215)
// ---------------------------------------------------------------------------

/// `mp_character_head_menu_handler` (setup.c:1881).
fn mp_character_head_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData, mpheadnum: i32, _arg4: bool) -> R {
    let diff60f = pd.lv.diffframe60f;
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = pd.mp_get_num_heads(),
        MENUOP_ON_CAROUSEL_TICK => {
            let headnum = pd.mp_get_head_id(mpheadnum.max(0) as usize);
            let mm = &mut pd.m().menumodel;
            let v = mm.curroty + 0.01 * diff60f;
            mm.newroty = v;
            mm.curroty = v;
            // MENUMODELPARAMS_SET_FILENUM(g_HeadsAndBodies[headnum].filenum)
            mm.newparams = HEADS_AND_BODIES.get(headnum as usize).map(|h| h.filenum as u32).unwrap_or(0);
            mm.headnum = headnum;
            mm.isperfecthead = false;
            mm.zoomtimer60 = 0;
            mm.hideheadparts = true;
            mm.zoom = 30.0;
        }
        MENUOP_IS_CAROUSEL_OPTION_HIDDEN => {
            let f = MP_HEADS.get(data.value.max(0) as usize).map(|h| h.requirefeature).unwrap_or(0);
            if !pd.challenge_is_feature_unlocked(f) {
                return HRet::I(1);
            }
        }
        MENUOP_GET_SELECTED_INDEX => data.value = mpheadnum,
        MENUOP_CONFIRM | MENUOP_ON_FOCUS => {
            pd.m().menumodel.loaddelay = 3;
            pd.menu_configure_model(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, MENUMODELFLAG_HASSCALE);
            let mm = &mut pd.m().menumodel;
            mm.curposx = 0.0;
            mm.curposy = 0.0;
            mm.newposx = 0.0;
            mm.newposy = -3.0;
            mm.curscale = 0.01;
            mm.curroty = -0.3;
            mm.newroty = -0.3;
            mm.newscale = 1.0;
            mm.zoom = 30.0;
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_character_head` (setup.c:1961).
pub fn menuhandler_mp_character_head(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = cur_player(pd);
    if op == MENUOP_CONFIRM {
        pd.mp.players[p].base.mpheadnum = data.value as u8;
    }
    let h = pd.mp.players[p].base.mpheadnum as i32;
    mp_character_head_menu_handler(pd, op, item, data, h, true)
}

pub fn mp_menu_text_body_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let b = pd.mp.players[cur_player(pd)].base.mpbodynum as usize;
    pd.mp_get_body_name(b)
}

/// `mp_restart_character_body_label_timer` (setup.c:1975).
fn mp_restart_character_body_label_timer(pd: &mut MenuSystem) {
    pd.menu_set_item_redraw_timer(&G_MP_CHARACTER_MENU_ITEMS[0], -0.4);
}

/// `mp_player_name_menu_handler` (setup.c:1980).
pub fn mp_player_name_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = cur_player(pd);
    match op {
        MENUOP_GET_KEYBOARD_STRING => data.string = kb_from(&pd.mp.players[p].base.name),
        MENUOP_SET_KEYBOARD_STRING => pd.mp.players[p].base.name = format!("{}\n", kb_to(&data.string)),
        _ => {}
    }
    ok()
}

/// `mp_load_settings_menu_handler` (setup.c:2022): the presets, then the
/// setup files on each device (`g_FileLists[1]`).
pub fn mp_load_settings_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let presets = pd.mp_get_num_unlocked_presets();
    let list = pd.filelists.lists[1].clone();
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = presets + list.as_ref().map_or(0, |l| l.numfiles()),
        MENUOP_GET_OPTION_TEXT => {
            if data.value < presets {
                return pd.mp_get_preset_name_by_slot(data.value).into();
            }
            if let Some(f) = list.as_ref().and_then(|l| l.files.get((data.value - presets) as usize)) {
                let mut c = [0u8; 12];
                savebuffer_bitstring_to_cstring(&f.name, &mut c, false);
                return cstr_to_string(&c).into();
            }
        }
        MENUOP_CONFIRM => {
            mp_close_dialogs_for_new_setup(pd);
            if data.value < presets {
                pd.mp_load_preset_by_slotnum(data.value);
            } else if let Some(f) = list.as_ref().and_then(|l| l.files.get((data.value - presets) as usize)) {
                let guid = FileGuid { fileid: f.fileid, deviceserial: f.deviceserial };
                pd.filemgr_save_or_load(guid, FILEOP_LOAD_MPSETUP, 0);
            }
            if item.param == 1 {
                pd.menu_save_and_push_root_dialog(Some(&G_MP_QUICK_GO_MENU_DIALOG), MENUROOT_MPSETUP);
            }
        }
        MENUOP_GET_SELECTED_INDEX => data.value = 0xfffff,
        MENUOP_GET_OPTGROUP_COUNT => data.value = 1 + list.as_ref().map_or(0, |l| l.numdevices as i32),
        MENUOP_GET_OPTGROUP_TEXT => {
            if data.value == 0 {
                return pd.lang(tx(B_MPMENU, 141)).into();
            }
            if list.is_some() {
                return pd.filemgr_group_text(1, data.value - 1);
            }
        }
        MENUOP_GET_OPTGROUP_START_INDEX => {
            data.groupstartindex = if data.value == 0 {
                0
            } else {
                presets + if list.is_some() { pd.filemgr_group_start(1, data.value - 1) } else { 0 }
            };
        }
        MENUOP_ON_OPTION_FOCUS => pd.m().mpsetup.slotindex = if data.value < presets { 0xffff } else { data.value - presets },
        _ => {}
    }
    ok()
}

/// `mp_menu_text_mpconfig_marquee` (setup.c:2105): a setup file's overview
/// ("Name:  Scenario: ...   Arena: ...    Simulants: n").
pub fn mp_menu_text_mpconfig_marquee(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let slot = pd.mr().mpsetup.slotindex;
    let Some(f) = pd.filelists.lists[1].as_ref().and_then(|l| l.files.get(slot.max(0) as usize)).copied() else { return String::new() };
    if slot >= 0xffff {
        return String::new();
    }
    let (filename, numsims, stagenum, scenarionum) = MenuSystem::mpsetupfile_get_overview(&f.name);
    // PD's arenanum search over g_MpArenas, and the custom levels (not PD).
    let arena = pd.arena_name(stagenum as u8);
    if let (true, Some(arena)) = (scenarionum <= 5 && !filename.is_empty() && numsims as i32 <= MAX_BOTS, arena) {
        let fmt = pd.lang(tx(B_MPMENU, 140));
        let scen = pd.lang(MP_SCENARIO_OVERVIEWS[scenarionum as usize].name);
        return fmt.replacen("%s", &filename, 1).replacen("%s", &scen, 1).replacen("%s", &arena, 1).replacen("%d", &numsims.to_string(), 1);
    }
    String::new()
}

/// `mp_load_player_menu_handler` (setup.c:2156): the player files on each
/// device (`g_FileLists[0]`); a file another joined player has loaded can't
/// be loaded twice.
pub fn mp_load_player_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let Some(list) = pd.filelists.lists[0].clone() else { return ok() };
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = list.numfiles(),
        MENUOP_GET_OPTION_TEXT => {
            if let Some(f) = list.files.get(data.value.max(0) as usize) {
                return pd.filemgr_get_select_name(f, FILETYPE_MPPLAYER).into();
            }
        }
        MENUOP_CONFIRM => {
            let Some(file) = list.files.get(data.value.max(0) as usize).copied() else { return ok() };
            let mut available = true;
            for i in 0..4 {
                if file.fileid == pd.mp.players[i].fileguid.fileid && file.deviceserial == pd.mp.players[i].fileguid.deviceserial {
                    if pd.mp.setup.chrslots & (1 << i) == 0 {
                        pd.mp_player_set_defaults(i, true);
                    } else {
                        available = false;
                    }
                }
            }
            if available {
                let guid = FileGuid { fileid: file.fileid, deviceserial: file.deviceserial };
                pd.menu_pop_dialog();
                let p = pd.mpplayernum as u32;
                pd.filemgr_save_or_load(guid, FILEOP_LOAD_MPPLAYER, p);
            } else {
                pd.filemgr_push_error_dialog(pd_core::ids::FILEERROR_ALREADYLOADED);
            }
        }
        MENUOP_GET_SELECTED_INDEX => data.value = 0xfffff,
        MENUOP_GET_OPTGROUP_COUNT => data.value = list.numdevices as i32,
        MENUOP_GET_OPTGROUP_TEXT => return pd.filemgr_group_text(0, data.value),
        MENUOP_GET_OPTGROUP_START_INDEX => {
            data.groupstartindex = pd.filemgr_group_start(0, data.value);
            return ok();
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_time_limit_slider` (setup.c:2216).
pub fn menuhandler_mp_time_limit_slider(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_SLIDER_VALUE => data.value = pd.mp.setup.timelimit as i32,
        MENUOP_CONFIRM => pd.mp.setup.timelimit = data.value as u8,
        MENUOP_GET_SLIDER_LABEL => {
            data.label = if data.value == 60 { pd.lang(tx(B_MPMENU, 112)) } else { pd.lang(tx(B_MPMENU, 114)).replacen("%d", &(data.value + 1).to_string(), 1) };
        }
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_score_limit_slider(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_SLIDER_VALUE => data.value = pd.mp.setup.scorelimit as i32,
        MENUOP_CONFIRM => pd.mp.setup.scorelimit = data.value as u8,
        MENUOP_GET_SLIDER_LABEL => {
            data.label = if data.value == 100 { pd.lang(tx(B_MPMENU, 112)) } else { pd.lang(tx(B_MPMENU, 113)).replacen("%d", &(data.value + 1).to_string(), 1) };
        }
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_team_score_limit_slider(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_SLIDER_VALUE => data.value = pd.mp_calculate_team_score_limit(),
        MENUOP_CONFIRM => pd.mp.setup.teamscorelimit = data.value as u16,
        MENUOP_GET_SLIDER_LABEL => {
            data.label = if data.value == 400 { pd.lang(tx(B_MPMENU, 112)) } else { pd.lang(tx(B_MPMENU, 113)).replacen("%d", &(data.value + 1).to_string(), 1) };
        }
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_restore_score_defaults(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.mp_init_limits();
    }
    ok()
}

/// `menuhandler_mp_handicap_player` (setup.c:2284).
pub fn menuhandler_mp_handicap_player(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = item.param.clamp(0, 3) as usize;
    match op {
        MENUOP_IS_HIDDEN => {
            if pd.mp.setup.chrslots & (1 << p) == 0 {
                return HRet::I(1);
            }
        }
        MENUOP_GET_SLIDER_VALUE => data.value = pd.mp.players[p].handicap as i32,
        MENUOP_CONFIRM => pd.mp.players[p].handicap = data.value as u16,
        MENUOP_GET_SLIDER_LABEL => {
            data.label = format!("{:.0}%\n", MenuSystem::mp_handicap_to_value(pd.mp.players[p].handicap as u8) * 100.0);
        }
        _ => {}
    }
    ok()
}

pub fn mp_menu_text_handicap_player_name(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let p = item.param.clamp(0, 3) as usize;
    if pd.mp.setup.chrslots & (1 << p) != 0 {
        pd.mp.players[p].base.name.clone()
    } else {
        String::new()
    }
}

pub fn menuhandler_mp_restore_handicap_defaults(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        for p in 0..4 {
            pd.mp.players[p].handicap = 0x80;
        }
    }
    ok()
}

/// `menudialog_mp_ready` (setup.c:2326): a player with a file saves it as
/// they get ready.
pub fn menudialog_mp_ready(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_OPEN {
        let g = pd.mp.players[cur_player(pd)].fileguid;
        if g.fileid != 0 && g.deviceserial != 0 {
            let p = cur_player(pd) as u32;
            pd.filemgr_save_or_load(g, FILEOP_SAVE_MPPLAYER, p);
        }
    }
    0
}

/// `menudialog_mp_simulant` (setup.c:2337).
pub fn menudialog_mp_simulant(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_TICK && pd.mp.bots[slot(pd)].base.name.is_empty() {
        pd.menu_pop_dialog();
    }
    0
}

// ---------------------------------------------------------------------------
// Simulants (setup.c:2667-3002)
// ---------------------------------------------------------------------------

fn unlocked_profiles(pd: &MenuSystem) -> Vec<usize> {
    (0..BOT_PROFILES.len()).filter(|&i| pd.challenge_is_feature_unlocked(BOT_PROFILES[i].requirefeature)).collect()
}

/// `mp_add_change_simulant_menu_handler` (setup.c:2667).
pub fn mp_add_change_simulant_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let groups: [(usize, u16); 2] = [(0, 103), (6, 104)];
    let unl = unlocked_profiles(pd);
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = unl.len() as i32,
        MENUOP_GET_OPTION_TEXT => {
            if let Some(&i) = unl.get(data.value.max(0) as usize) {
                return pd.lang(BOT_PROFILES[i].name).into();
            }
        }
        MENUOP_CONFIRM => {
            let mut botnum = pd.mr().mpsetup.slotindex;
            let mut creating = false;
            if botnum < 0 {
                botnum = pd.mp_get_slot_for_new_bot() as i32;
                creating = true;
            } else if pd.mp.setup.chrslots & (1 << (botnum + 4)) == 0 {
                creating = true;
            }
            let botnum = botnum.clamp(0, 7) as usize;
            let i = unl.get(data.value.max(0) as usize).copied().unwrap_or(BOT_PROFILES.len() - 1);
            if creating {
                pd.mp_create_bot_from_profile(botnum, i);
            } else {
                pd.mp.bots[botnum].ty = BOT_PROFILES[i].ty as u8;
                if pd.mp.bots[botnum].ty as i32 == BOTTYPE_GENERAL {
                    pd.mp_set_bot_difficulty(botnum, BOT_PROFILES[i].difficulty);
                }
            }
            pd.mp_generate_bot_names();
            pd.m().mpsetup.slotcount = data.value;
        }
        MENUOP_ON_OPTION_FOCUS | MENUOP_GET_SELECTED_INDEX => {
            if op == MENUOP_ON_OPTION_FOCUS {
                let i = unl.get(data.value.max(0) as usize).copied().unwrap_or(BOT_PROFILES.len());
                pd.m().mpsetup.botprofileindex = i as i32;
            }
            data.value = pd.mr().mpsetup.slotcount;
        }
        MENUOP_GET_OPTGROUP_COUNT => data.value = 2,
        MENUOP_GET_OPTGROUP_TEXT => return pd.lang(tx(B_MPMENU, groups[data.value.clamp(0, 1) as usize].1)).into(),
        MENUOP_GET_OPTGROUP_START_INDEX => {
            let off = groups[data.value.clamp(0, 1) as usize].0;
            data.groupstartindex = (0..off).filter(|&i| pd.challenge_is_feature_unlocked(BOT_PROFILES[i].requirefeature)).count() as i32;
        }
        _ => {}
    }
    ok()
}

pub fn mp_menu_text_simulant_description(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let i = pd.mr().mpsetup.botprofileindex;
    pd.lang(tx(B_MISC, 106).add(i))
}

/// `menuhandler_mp_simulant_head` (setup.c:2775).
pub fn menuhandler_mp_simulant_head(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let start = if item.param2.num() == 1 { pd.mp_get_num_heads() } else { 0 };
    let s = slot(pd);
    if op == MENUOP_CONFIRM {
        pd.mp.bots[s].base.mpheadnum = (start + data.value) as u8;
    }
    // PD's switch falls through from MENUOP_CONFIRM into MENUOP_ON_FOCUS, whose
    // body re-checks for ON_FOCUS (setup.c:2787): the redundant test is PD's.
    #[allow(clippy::overly_complex_bool_expr)]
    if (op == MENUOP_CONFIRM || op == MENUOP_ON_FOCUS) && op == MENUOP_ON_FOCUS && item.param2.num() == 1 && (pd.mp.bots[s].base.mpheadnum as i32) < start {
        pd.mp.bots[s].base.mpheadnum = start as u8;
    }
    let h = pd.mp.bots[s].base.mpheadnum as i32;
    mp_character_head_menu_handler(pd, op, item, data, h, false)
}

/// `menuhandler_mp_simulant_body` (setup.c:2804).
pub fn menuhandler_mp_simulant_body(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let s = slot(pd);
    if op == MENUOP_CONFIRM {
        pd.mp.bots[s].base.mpbodynum = data.value as u8;
    }
    let (b, h) = (pd.mp.bots[s].base.mpbodynum as i32, pd.mp.bots[s].base.mpheadnum as i32);
    mp_character_body_menu_handler(pd, op, item, data, b, h, false)
}

/// `menudialog_mp_bot_character` (setup.c:2814).
pub fn menudialog_mp_bot_character(pd: &mut MenuSystem, op: i32, def: &'static MenuDialogDef, data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_TICK {
        let m = pd.mr();
        if let Some(cd) = m.curdialog {
            let d = m.dialogs[cd];
            if std::ptr::eq(d.def(), def) && d.focuseditem != Some(0) && d.focuseditem != Some(1) {
                let mut hd = HandlerData::default();
                // PD calls the *player's* body handler here (setup.c:2823).
                menuhandler_mp_character_body(pd, MENUOP_ON_CAROUSEL_TICK, &def.items[1], &mut hd);
            }
        }
    }
    menudialog_mp_simulant(pd, op, def, data)
}

/// `mp_bot_difficulty_menu_handler` (setup.c:2831).
pub fn mp_bot_difficulty_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let s = slot(pd);
    let unl: Vec<usize> = (0..BOTDIFF_DISABLED as usize).filter(|&i| pd.challenge_is_feature_unlocked(BOT_PROFILES[i].requirefeature)).collect();
    match op {
        MENUOP_CONFIRM => {
            pd.mp_set_bot_difficulty(s, data.value);
            pd.mp_generate_bot_names();
        }
        MENUOP_GET_SELECTED_INDEX => {
            let d = pd.mp.bots[s].difficulty as i32;
            data.value = if (0..BOTDIFF_DISABLED).contains(&d) { d } else { 0 };
        }
        MENUOP_GET_OPTION_COUNT => data.value = unl.len() as i32,
        MENUOP_GET_OPTION_TEXT => {
            if let Some(&i) = unl.get(data.value.max(0) as usize) {
                return pd.lang(tx(B_MISC, 82).add(i as i32)).into();
            }
            return String::from("\n").into();
        }
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_delete_simulant(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let s = slot(pd);
        pd.mp_remove_simulant(s);
        pd.menu_pop_dialog();
    }
    ok()
}

pub fn title_mp_menu_title_edit_simulant(pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    pd.mp.bots[slot(pd)].base.name.clone()
}

/// `menuhandler_mp_change_simulant_type` (setup.c:2892).
pub fn menuhandler_mp_change_simulant_type(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let s = slot(pd);
        let b = pd.mp.bots[s].clone();
        let profilenum = MenuSystem::mp_find_bot_profile(b.ty as i32, b.difficulty as i32);
        let count = (0..profilenum.max(0) as usize).filter(|&i| pd.challenge_is_feature_unlocked(BOT_PROFILES[i].requirefeature)).count();
        pd.m().mpsetup.slotcount = count as i32;
        pd.menu_push_dialog(&G_MP_CHANGE_SIMULANT_MENU_DIALOG);
    }
    ok()
}

pub fn menuhandler_mp_clear_all_simulants(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        for i in 0..8 {
            pd.mp_remove_simulant(i);
        }
    }
    ok()
}

pub fn menuhandler_mp_add_simulant(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    match op {
        MENUOP_CONFIRM => {
            pd.m().mpsetup.slotindex = -1;
            pd.menu_push_dialog(&G_MP_ADD_SIMULANT_MENU_DIALOG);
        }
        MENUOP_IS_DISABLED => {
            if !pd.mp_has_unused_bot_slots() {
                return HRet::I(1);
            }
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_simulant_slot` (setup.c:2943).
pub fn menuhandler_mp_simulant_slot(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    let p = item.param.clamp(0, 7) as usize;
    match op {
        MENUOP_CONFIRM => {
            pd.m().mpsetup.slotindex = p as i32;
            if pd.mp.setup.chrslots & (1 << (p + 4)) == 0 {
                pd.menu_push_dialog(&G_MP_ADD_SIMULANT_MENU_DIALOG);
            } else {
                pd.menu_push_dialog(&G_MP_EDIT_SIMULANT_MENU_DIALOG);
            }
        }
        MENUOP_IS_HIDDEN => {
            if p >= 4 && !pd.challenge_is_feature_unlocked(MPFEATURE_8BOTS) {
                return HRet::I(1);
            }
        }
        MENUOP_IS_DISABLED => {
            if !pd.mp_is_sim_slot_enabled(p) {
                return HRet::I(1);
            }
        }
        _ => {}
    }
    ok()
}

pub fn mp_menu_text_simulant_name(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let i = item.param.clamp(0, 7) as usize;
    if pd.mp.bots[i].base.name.is_empty() || pd.mp.setup.chrslots & (1 << (i + 4)) == 0 {
        return String::new();
    }
    pd.mp.bots[i].base.name.clone()
}

/// `menudialog_mp_simulants` (setup.c:2995).
pub fn menudialog_mp_simulants(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_OPEN {
        pd.m().mpsetup.slotcount = 0;
    }
    0
}

// ---------------------------------------------------------------------------
// Teams (setup.c:3248-3480)
// ---------------------------------------------------------------------------

/// `menuhandler_mp_n_teams` (setup.c:3248).
fn menuhandler_mp_n_teams(pd: &mut MenuSystem, op: i32, numteams: i32) -> R {
    if op == MENUOP_CONFIRM {
        let numchrs = pd.mp_get_num_chrs();
        if numchrs == 0 {
            return ok();
        }
        let mut array = [0i32; 4];
        let somevalue = (numchrs + numteams - 1) / numteams;
        let mut teamsremaining = numteams;
        let mut chrsremaining = numchrs;
        let start = (pd.rng.random() % numchrs as u32) as i32;
        let mut i = (start + 1) % numchrs;
        loop {
            let chr = pd.mp_get_chr_index_by_slot(i);
            if teamsremaining >= chrsremaining {
                let mut teamnum = (pd.rng.random() % numteams as u32) as i32;
                loop {
                    if array[teamnum as usize] == 0 {
                        if let Some(c) = chr.and_then(|c| pd.mpchr_mut(c)) {
                            c.team = teamnum as u8;
                        }
                        array[teamnum as usize] += 1;
                        teamsremaining -= 1;
                        chrsremaining -= 1;
                        break;
                    }
                    teamnum = (teamnum + 1) % numteams;
                }
            } else {
                let mut teamnum = (pd.rng.random() % numteams as u32) as i32;
                loop {
                    if array[teamnum as usize] < somevalue {
                        if let Some(c) = chr.and_then(|c| pd.mpchr_mut(c)) {
                            c.team = teamnum as u8;
                        }
                        if array[teamnum as usize] == 0 {
                            teamsremaining -= 1;
                        }
                        array[teamnum as usize] += 1;
                        chrsremaining -= 1;
                        break;
                    }
                    teamnum = (teamnum + 1) % numteams;
                }
            }
            if i == start {
                break;
            }
            i = (i + 1) % numchrs;
        }
        pd.menu_pop_dialog();
    }
    ok()
}

pub fn menuhandler_mp_two_teams(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    menuhandler_mp_n_teams(pd, op, 2)
}
pub fn menuhandler_mp_three_teams(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    menuhandler_mp_n_teams(pd, op, 3)
}
pub fn menuhandler_mp_four_teams(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    menuhandler_mp_n_teams(pd, op, 4)
}

pub fn menuhandler_mp_maximum_teams(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let mut team = 0u8;
        let max = pd.scenario_get_max_teams() as u8;
        for i in 0..12 {
            if pd.mp.setup.chrslots & (1 << i) != 0 {
                if let Some(c) = pd.mpchr_mut(i) {
                    c.team = team;
                }
                team += 1;
                if team >= max {
                    team = 0;
                }
            }
        }
        pd.menu_pop_dialog();
    }
    ok()
}

pub fn menuhandler_mp_humans_vs_simulants(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        for i in 0..12 {
            if pd.mp.setup.chrslots & (1 << i) != 0 {
                if let Some(c) = pd.mpchr_mut(i) {
                    c.team = if i < 4 { 0 } else { 1 };
                }
            }
        }
        pd.menu_pop_dialog();
    }
    ok()
}

pub fn menuhandler_mp_human_simulant_pairs(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let team_ids = [0u8, 1, 2, 3];
        let mut playerindex = 0;
        let mut simindex = 0;
        for i in 0..12 {
            if pd.mp.setup.chrslots & (1 << i) != 0 {
                let t = if i < 4 {
                    let t = team_ids[playerindex.min(3)];
                    playerindex += 1;
                    t
                } else {
                    let t = team_ids[simindex.min(3)];
                    simindex += 1;
                    if simindex >= playerindex {
                        simindex = 0;
                    }
                    t
                };
                if let Some(c) = pd.mpchr_mut(i) {
                    c.team = t;
                }
            }
        }
        pd.menu_pop_dialog();
    }
    ok()
}

pub fn mp_menu_text_chr_name_for_team_setup(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    pd.mp_get_chr_index_by_slot(item.param).and_then(|c| pd.mpchr(c)).map(|c| c.name).unwrap_or_default()
}

fn menuhandler_mp_team_slot2(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_OPTION_COUNT => {
            data.value = pd.scenario_get_max_teams();
            return ok();
        }
        MENUOP_GET_OPTION_TEXT => {
            if pd.mp.setup.options & MPOPTION_TEAMSENABLED as u32 == 0 {
                return String::from("\n").into();
            }
            return pd.mp.bossfile.teamnames[data.value.clamp(0, 7) as usize].clone().into();
        }
        _ => {}
    }
    menuhandler_mp_teams_label(pd, op, item, data)
}

/// `menuhandler_mp_team_slot` (setup.c:3442).
pub fn menuhandler_mp_team_slot(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let chr = pd.mp_get_chr_index_by_slot(item.param);
    match op {
        MENUOP_CONFIRM => {
            if let Some(c) = chr.and_then(|c| pd.mpchr_mut(c)) {
                c.team = data.value as u8;
            }
            return ok();
        }
        MENUOP_GET_SELECTED_INDEX => {
            data.value = chr.and_then(|c| pd.mpchr(c)).map(|c| c.team as i32).unwrap_or(0xff);
            return ok();
        }
        MENUOP_IS_DISABLED => {
            if chr.is_none() {
                return HRet::I(1);
            }
            return menuhandler_mp_teams_label(pd, op, item, data);
        }
        _ => {}
    }
    menuhandler_mp_team_slot2(pd, op, item, data)
}

pub fn mp_menu_text_select_tune_or_tunes(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    title_mp_menu_text_select_tune_or_tunes(pd, &G_MP_SELECT_TUNES_MENU_DIALOG)
}

pub fn title_mp_menu_text_select_tune_or_tunes(pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    if pd.mp.bossfile.usingmultipletunes {
        pd.lang(tx(B_MPMENU, 69))
    } else {
        pd.lang(tx(B_MPMENU, 68))
    }
}

// ---------------------------------------------------------------------------
// Soundtrack, team names, challenges (setup.c:3841-4700)
// ---------------------------------------------------------------------------

/// `mp_select_tune_list_handler` (setup.c:3841): the focused tune plays
/// (`music_start_track_as_menu`).
pub fn mp_select_tune_list_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let special = [166u16, 167, 168, 169];
    let numtracks = pd.mp_get_num_unlocked_tracks();
    let multi = pd.mp.bossfile.usingmultipletunes;
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = numtracks + if multi { 3 } else { 1 },
        MENUOP_GET_OPTION_TEXT => {
            if data.value < numtracks {
                return pd.mp_get_track_name(data.value).into();
            }
            let i = if multi { 1 + data.value - numtracks } else { data.value - numtracks };
            return pd.lang(tx(B_MISC, special[i.clamp(0, 3) as usize])).into();
        }
        MENUOP_CONFIRM => {
            if data.value < numtracks {
                if data.unk04 == 0 {
                    pd.mp_set_track_slot_enabled(data.value);
                }
                pd.vars.modifiedfiles |= MODFILE_MPSETUP;
            } else if multi {
                match data.value - numtracks {
                    0 => {
                        pd.mp_enable_all_multi_tracks();
                        pd.vars.modifiedfiles |= MODFILE_MPSETUP;
                    }
                    1 => {
                        pd.mp_disable_all_multi_tracks();
                        pd.vars.modifiedfiles |= MODFILE_MPSETUP;
                    }
                    2 => {
                        pd.mp_randomise_multi_tracks();
                        pd.vars.modifiedfiles |= MODFILE_MPSETUP;
                    }
                    _ => {}
                }
            } else {
                pd.mp.bossfile.tracknum = -1;
                pd.vars.modifiedfiles |= MODFILE_MPSETUP;
            }
        }
        MENUOP_GET_SELECTED_INDEX => {
            if multi {
                data.value = 0x000fffff;
            } else {
                let s = pd.mp_get_current_track_slot_num();
                data.value = if s < 0 { numtracks } else { s };
            }
        }
        MENUOP_ON_OPTION_FOCUS => {
            if data.value < numtracks {
                let t = pd.mp_get_track_num_at_slot_index(data.value);
                pd.music(pd_core::music::MusicCall::StartTrackAsMenu(MP_TRACKS[t].musicnum));
            }
        }
        MENUOP_IS_OPTION_CHECKED => {
            if multi && data.value < numtracks {
                data.unk04 = pd.mp_is_multi_track_slot_enabled(data.value) as i32;
            }
        }
        _ => {}
    }
    ok()
}

/// `menudialog_mp_select_tune` (setup.c:3930): the queue runs every 80
/// quarter-ticks while the dialog is open (so scrolling doesn't restart the
/// tune at every row), 15 again after.
pub fn menudialog_mp_select_tune(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_OPEN {
        pd.music(pd_core::music::MusicCall::SetInterval(80));
    }
    if op == MENUOP_ON_CLOSE {
        pd.music(pd_core::music::MusicCall::SetInterval(15));
    }
    0
}

pub fn mp_menu_text_current_track(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    if pd.mp.bossfile.usingmultipletunes {
        return pd.lang(tx(B_MPMENU, 66));
    }
    let s = pd.mp_get_current_track_slot_num();
    if s >= 0 {
        return pd.mp_get_track_name(s);
    }
    pd.lang(tx(B_MPMENU, 67))
}

pub fn menuhandler_mp_multiple_tunes(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_IS_CHECKED => return pd.mp.bossfile.usingmultipletunes.into(),
        MENUOP_CONFIRM => {
            pd.mp.bossfile.usingmultipletunes = data.value != 0;
            pd.vars.modifiedfiles |= MODFILE_MPSETUP;
        }
        _ => {}
    }
    ok()
}

/// `mp_team_name_menu_handler` (setup.c:3981).
pub fn mp_team_name_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let s = pd.mr().mpsetup.slotindex.clamp(0, 7) as usize;
    match op {
        MENUOP_GET_KEYBOARD_STRING => data.string = kb_from(&pd.mp.bossfile.teamnames[s]),
        MENUOP_SET_KEYBOARD_STRING => {
            pd.mp.bossfile.teamnames[s] = format!("{}\n", kb_to(&data.string));
            pd.vars.modifiedfiles |= MODFILE_MPSETUP;
        }
        _ => {}
    }
    ok()
}

/// `mp_menu_text_team_name` (setup.c:4030): param2 is the team colour's text id.
pub fn mp_menu_text_team_name(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let index = match item.param2 {
        P::Text(t) => t.index as i32 - 8,
        _ => 0,
    };
    pd.mp.bossfile.teamnames[index.clamp(0, 7) as usize].clone()
}

pub fn menuhandler_mp_team_name_slot(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let index = match item.param2 {
            P::Text(t) => t.index as i32 - 8,
            _ => 0,
        };
        pd.m().mpsetup.slotindex = index;
        pd.menu_push_dialog(&G_MP_CHANGE_TEAM_NAME_MENU_DIALOG);
    }
    ok()
}

pub fn title_menutext_mp_challenge_name(pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    let fmt = pd.lang(tx(B_MPMENU, 56));
    let name = pd.challenge_get_name_by_slot(pd.mr().mpsetup.slotindex);
    fmt.replacen("%s", name.trim_end_matches('\n'), 1)
}

pub fn menuhandler_mp_accept_challenge(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.challenge_unset_current();
        pd.menu_pop_dialog();
        let s = pd.mr().mpsetup.slotindex;
        pd.challenge_set_current_by_slot(s);
    }
    ok()
}

/// `menudialog_mp_confirm_challenge` (setup.c:4074).
pub fn menudialog_mp_confirm_challenge(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    match op {
        MENUOP_ON_OPEN => {
            pd.m().menumodel.curparams = 0;
            let s = pd.mr().mpsetup.slotindex;
            pd.m().training_slot = s;
            let c = pd.challenge_config_by_slot(s);
            pd.m().training_config = c;
        }
        MENUOP_ON_TICK => {
            if pd.mp.bossfile.locktype as i32 == MPLOCKTYPE_CHALLENGE {
                pd.menu_pop_dialog();
            }
        }
        _ => {}
    }
    0
}

/// `mp_challenges_list_menu_handler` (setup.c:4479).
pub fn mp_challenges_list_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_IS_HIDDEN => {
            if pd.mp.bossfile.locktype as i32 == MPLOCKTYPE_CHALLENGE {
                return HRet::I(1);
            }
        }
        MENUOP_GET_OPTION_COUNT => data.value = pd.challenge_get_num_available(),
        MENUOP_CONFIRM => {
            if data.unk04 != 0 {
                data.unk04 = 2;
            }
            pd.m().mpsetup.slotindex = data.value;
            if item.param == 0 {
                pd.menu_push_dialog(&G_MP_CONFIRM_CHALLENGE_VIA_LIST_OR_DETAILS_MENU_DIALOG);
            } else {
                pd.menu_push_dialog(&G_MP_CONFIRM_CHALLENGE_MENU_DIALOG);
            }
        }
        MENUOP_GET_SELECTED_INDEX => data.value = 0xfffff,
        MENUOP_GET_OPTGROUP_COUNT => data.value = 0,
        MENUOP_GET_OPTGROUP_TEXT => return String::new().into(),
        MENUOP_GET_OPTGROUP_START_INDEX => data.groupstartindex = 0,
        MENUOP_RENDER => {
            let Some(rd) = data.render else { return ok() };
            let (vw, vh) = (pd.draw.gfx.w as i32, pd.draw.gfx.h as i32);
            let name = pd.challenge_get_name_by_slot(data.unk04);
            let (mut x, mut y) = (rd.x + 10, rd.y + 1);
            pd.tc().render_v2(&mut x, &mut y, &name, pd_core::text::FontId::Sm, rd.colour, vw, vh, 0, 0);
            let mut marginleft = 10;
            for i in 0..4 {
                let done = pd.challenge_is_completed_by_any_chr_with_num_players_by_slot(data.unk04, i + 1);
                let env = if done { ((rd.colour & 0xff) * 0xff >> 8) | 0xffe56500 } else { ((rd.colour & 0xff) * 0xff >> 8) | 0x43430000 };
                pd.draw_star(rd.x + marginleft, rd.y + 11, 11, env, true);
                marginleft += 13;
            }
        }
        MENUOP_GET_OPTION_HEIGHT => data.value = 26,
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_challenge_description_and_separator(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && pd.mp.bossfile.locktype as i32 != MPLOCKTYPE_CHALLENGE {
        return HRet::I(1);
    }
    ok()
}

pub fn menuhandler_mp_abort_challenge(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && pd.mp.bossfile.locktype as i32 != MPLOCKTYPE_CHALLENGE {
        return HRet::I(1);
    }
    if op == MENUOP_CONFIRM {
        pd.challenge_remove_player_lock();
    }
    ok()
}

pub fn menuhandler_mp_start_challenge(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && pd.mp.bossfile.locktype as i32 != MPLOCKTYPE_CHALLENGE {
        return HRet::I(1);
    }
    if op == MENUOP_CONFIRM {
        pd.menu_push_dialog(&G_MP_READY_MENU_DIALOG);
    }
    ok()
}

pub fn title_mp_menu_text_challenge_name(pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    if pd.mp.bossfile.locktype as i32 != MPLOCKTYPE_CHALLENGE {
        return pd.lang(tx(B_MPMENU, 50));
    }
    format!("{}:\n", pd.challenge_get_name(pd.mp.challenge_index).trim_end_matches('\n'))
}

/// `mp_combat_challenges_menu_dialog` (setup.c:4648).
pub fn mp_combat_challenges_menu_dialog(pd: &mut MenuSystem, op: i32, def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_TICK && pd.mp.bossfile.locktype as i32 == MPLOCKTYPE_CHALLENGE && pd.cur_def().map(|d| std::ptr::eq(d, def)).unwrap_or(false) && !pd.challenge_is_loaded() {
        pd.m().menumodel.curparams = 0x4fac5ace;
        pd.challenge_load_and_store_current();
    }
    if op == MENUOP_ON_CLOSE && pd.mr().menumodel.curparams == 0x4fac5ace {
        pd.challenge_unset_current();
    }
    0
}

pub fn menuhandler_mp_accept_challenge2(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let s = pd.mr().mpsetup.slotindex;
        pd.challenge_set_current_by_slot(s);
        pd.menu_save_and_push_root_dialog(Some(&G_MP_QUICK_GO_MENU_DIALOG), MENUROOT_MPSETUP);
    }
    ok()
}

/// `menuhandler_mp_lock` (setup.c:4700).
pub fn menuhandler_mp_lock(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let labels = [45u16, 46, 47, 48];
    let challenge = pd.mp_get_lock_type() == MPLOCKTYPE_CHALLENGE;
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = if challenge { 1 } else { 5 },
        MENUOP_GET_OPTION_TEXT => {
            if challenge {
                return pd.lang(tx(B_MPMENU, 49)).into();
            }
            if data.value <= 3 {
                return pd.lang(tx(B_MPMENU, labels[data.value.max(0) as usize])).into();
            }
            if pd.mp_get_lock_type() == MPLOCKTYPE_PLAYER {
                let l = pd.mp.lockinfo.lockedplayernum.clamp(0, 3) as usize;
                return pd.mp.players[l].base.name.clone().into();
            }
            return mp_get_current_player_name(pd, item).into();
        }
        MENUOP_CONFIRM => {
            if !challenge {
                let p = pd.mpplayernum as i32;
                pd.mp_set_lock(data.value, p);
            }
            pd.vars.modifiedfiles |= MODFILE_MPSETUP;
        }
        MENUOP_GET_SELECTED_INDEX => data.value = if challenge { 0 } else { pd.mp_get_lock_type() },
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_save_player` (setup.c:4738).
pub fn menuhandler_mp_save_player(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        if pd.mp.players[cur_player(pd)].fileguid.fileid == 0 {
            pd.filemgr_push_select_location_dialog(6, FILETYPE_MPPLAYER);
        } else {
            pd.menu_push_dialog(&G_MP_SAVE_PLAYER_MENU_DIALOG);
        }
    }
    ok()
}

pub fn mp_menu_text_save_player_or_copy(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    if pd.mp.players[cur_player(pd)].fileguid.fileid == 0 {
        pd.lang(tx(B_MPMENU, 38))
    } else {
        pd.lang(tx(B_MPMENU, 39))
    }
}

/// `menuhandler_mp_abort_setup` (setup.c:4760): back to the Perfect Menu.
pub fn menuhandler_mp_abort_setup(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_save_and_push_root_dialog(Some(&G_CI_MENU_VIA_PC_MENU_DIALOG), MENUROOT_MAINMENU);
    }
    ok()
}

/// `menuhandler_mp_save_settings` (setup.c:4777).
pub fn menuhandler_mp_save_settings(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        if pd.mp.setup.fileguid.fileid == 0 {
            pd.menu_push_dialog(&G_MP_SAVE_SETUP_NAME_MENU_DIALOG);
        } else {
            pd.menu_push_dialog(&G_MP_SAVE_SETUP_EXISTS_MENU_DIALOG);
        }
    }
    ok()
}

pub fn mp_menu_text_arena_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    pd.arena_name(pd.mp.setup.stagenum).unwrap_or_else(|| "\n".into())
}

pub fn mp_menu_text_weapon_set_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let s = pd.mp_get_weaponset_slotnum();
    pd.mp_get_weaponset_name_by_slotnum(s)
}

pub fn menudialog_mp_game_setup(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_OPEN {
        pd.vars.mpsetupmenu = MPSETUPMENU_ADVSETUP;
        pd.vars.usingadvsetup = true;
    }
    0
}

pub fn menudialog_mp_quick_go(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_OPEN {
        pd.vars.mpsetupmenu = MPSETUPMENU_QUICKGO;
    }
    0
}

/// `mp_configure_quick_team_players` (setup.c:4831).
fn mp_configure_quick_team_players(pd: &mut MenuSystem) {
    let qt = pd.vars.mpquickteam;
    if qt == MPQUICKTEAM_NONE {
        return;
    }
    for i in 0..8 {
        pd.mp_remove_simulant(i);
    }
    let te = MPOPTION_TEAMSENABLED as u32;
    match qt {
        MPQUICKTEAM_PLAYERSONLY | MPQUICKTEAM_PLAYERSANDSIMS => pd.mp.setup.options &= !te,
        MPQUICKTEAM_PLAYERSTEAMS => {
            pd.mp.setup.options |= te;
            for i in 0..4 {
                pd.mp.players[i].base.team = pd.vars.mpplayerteams[i];
            }
        }
        MPQUICKTEAM_PLAYERSVSSIMS => {
            pd.mp.setup.options |= te;
            for i in 0..4 {
                pd.mp.players[i].base.team = 0;
            }
        }
        MPQUICKTEAM_PLAYERSIMTEAMS => {
            pd.mp.setup.options |= te;
            for i in 0..4 {
                pd.mp.players[i].base.team = i as u8;
            }
        }
        _ => {}
    }
}

/// `mp_configure_quick_team_simulants` (setup.c:4875), run by `mp_start_match`.
pub fn mp_configure_quick_team_simulants(pd: &mut MenuSystem) {
    let qt = pd.vars.mpquickteam;
    let diff = pd.vars.mpsimdifficulty.max(0) as usize;
    match qt {
        MPQUICKTEAM_PLAYERSANDSIMS | MPQUICKTEAM_PLAYERSVSSIMS => {
            for _ in 0..pd.vars.mpquickteamnumsims {
                let b = pd.mp_get_slot_for_new_bot();
                pd.mp_create_bot_from_profile(b, diff);
            }
            pd.mp_generate_bot_names();
            if qt == MPQUICKTEAM_PLAYERSVSSIMS {
                for b in pd.mp.bots.iter_mut() {
                    b.base.team = 1;
                }
            }
        }
        MPQUICKTEAM_PLAYERSIMTEAMS => {
            for i in (0..pd.mp_get_num_chrs()).rev() {
                let team = pd.mp_get_chr_index_by_slot(i).and_then(|c| pd.mpchr(c)).map(|c| c.team).unwrap_or(0);
                for _ in 0..pd.vars.unk0004a0 {
                    let b = pd.mp_get_slot_for_new_bot();
                    pd.mp_create_bot_from_profile(b, diff);
                    pd.mp.bots[b].base.team = team;
                }
            }
            pd.mp_generate_bot_names();
        }
        _ => {}
    }
}

impl MenuSystem {
    /// `mp_apply_quickstart` (setup.c:4935).
    pub fn mp_apply_quickstart(&mut self) {
        mp_configure_quick_team_players(self);
        self.menu_save_and_push_root_dialog(Some(&G_MP_QUICK_GO_MENU_DIALOG), MENUROOT_MPSETUP);
    }

    /// `mp_open_advanced_setup` (setup.c:5858).
    pub fn mp_open_advanced_setup(&mut self, silent: bool) {
        let p = self.mpplayernum;
        self.menus[p].playernum = p;
        if self.mp.bossfile.locktype as i32 == MPLOCKTYPE_CHALLENGE {
            self.menu_push_root_dialog(&G_MP_CHALLENGE_LIST_OR_DETAILS_VIA_ADV_CHALLENGE_MENU_DIALOG, MENUROOT_MPSETUP);
        } else {
            self.menu_push_root_dialog(&G_MP_ADVANCED_SETUP_MENU_DIALOG, MENUROOT_MPSETUP);
        }
        self.menu_hide_pressstart_labels();
        if !silent {
            self.play_sound(SFXMAP_809A_EXPLOSION, 1.0, 1.0);
        }
    }
}

pub fn menuhandler_mp_finished_setup(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_PREFOCUSED {
        return HRet::I(1);
    }
    if op == MENUOP_CONFIRM {
        pd.mp_apply_quickstart();
    }
    ok()
}

pub fn menuhandler_quick_team_separator(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && pd.vars.mpquickteam == MPQUICKTEAM_PLAYERSONLY {
        return HRet::I(1);
    }
    ok()
}

/// `menuhandler_player_team` (setup.c:4972).
pub fn menuhandler_player_team(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = item.param.clamp(0, 3) as usize;
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = MAX_TEAMS,
        MENUOP_GET_OPTION_TEXT => return pd.mp.bossfile.teamnames[data.value.clamp(0, 7) as usize].clone().into(),
        MENUOP_CONFIRM => pd.vars.mpplayerteams[p] = data.value as u8,
        MENUOP_GET_SELECTED_INDEX => data.value = pd.vars.mpplayerteams[p] as i32,
        MENUOP_IS_HIDDEN => {
            if pd.vars.mpquickteam != MPQUICKTEAM_PLAYERSTEAMS {
                return HRet::I(1);
            }
        }
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_number_of_simulants(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = if pd.challenge_is_feature_unlocked(MPFEATURE_8BOTS) { MAX_BOTS } else { 4 },
        MENUOP_GET_OPTION_TEXT => return format!("{}\n", data.value + 1).into(),
        MENUOP_CONFIRM => pd.vars.mpquickteamnumsims = data.value + 1,
        MENUOP_GET_SELECTED_INDEX => data.value = pd.vars.mpquickteamnumsims - 1,
        MENUOP_IS_HIDDEN => {
            if pd.vars.mpquickteam != MPQUICKTEAM_PLAYERSANDSIMS && pd.vars.mpquickteam != MPQUICKTEAM_PLAYERSVSSIMS {
                return HRet::I(1);
            }
        }
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_simulants_per_team(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = 2,
        MENUOP_GET_OPTION_TEXT => return format!("{}\n", data.value + 1).into(),
        MENUOP_CONFIRM => pd.vars.unk0004a0 = data.value + 1,
        MENUOP_GET_SELECTED_INDEX => data.value = pd.vars.unk0004a0 - 1,
        MENUOP_IS_HIDDEN => {
            if pd.vars.mpquickteam != MPQUICKTEAM_PLAYERSIMTEAMS {
                return HRet::I(1);
            }
        }
        _ => {}
    }
    ok()
}

pub fn mp_quick_team_simulant_difficulty_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let unl: Vec<usize> = (0..NUM_BOTDIFFS as usize).filter(|&i| pd.challenge_is_feature_unlocked(BOT_PROFILES[i].requirefeature)).collect();
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = unl.len() as i32,
        MENUOP_GET_OPTION_TEXT => {
            if let Some(&i) = unl.get(data.value.max(0) as usize) {
                return pd.lang(tx(B_MISC, 82).add(i as i32)).into();
            }
        }
        MENUOP_CONFIRM => pd.vars.mpsimdifficulty = data.value,
        MENUOP_GET_SELECTED_INDEX => data.value = pd.vars.mpsimdifficulty,
        MENUOP_IS_HIDDEN => {
            let q = pd.vars.mpquickteam;
            if q != MPQUICKTEAM_PLAYERSANDSIMS && q != MPQUICKTEAM_PLAYERSVSSIMS && q != MPQUICKTEAM_PLAYERSIMTEAMS {
                return HRet::I(1);
            }
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_quick_team_option` (setup.c:5099).
pub fn menuhandler_mp_quick_team_option(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.vars.mpquickteam = item.param;
        if pd.mp_get_weaponset_slotnum() >= pd.mp_get_num_weaponset_slots(false) {
            pd.mp_set_weaponset_slotnum(0);
        }
        if pd.vars.mpquickteam == MPQUICKTEAM_PLAYERSONLY || pd.vars.mpquickteam == MPQUICKTEAM_PLAYERSANDSIMS {
            let s = pd.mp.setup.scenario as i32;
            if s == MPSCENARIO_KINGOFTHEHILL || s == MPSCENARIO_CAPTURETHECASE {
                pd.mp.setup.scenario = MPSCENARIO_COMBAT as u8;
            }
        }
        pd.menu_push_dialog(&G_MP_QUICK_TEAM_GAME_SETUP_MENU_DIALOG);
    }
    ok()
}

/// `menudialog_combat_simulator` (setup.c:5123).
pub fn menudialog_combat_simulator(pd: &mut MenuSystem, op: i32, def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_OPEN {
        pd.vars.waitingtojoin = [false; 4];
    }
    if op == MENUOP_ON_TICK && pd.cur_def().map(|d| std::ptr::eq(d, def)).unwrap_or(false) {
        pd.vars.mpsetupmenu = MPSETUPMENU_GENERAL;
        pd.vars.mpquickteam = MPQUICKTEAM_NONE;
        pd.vars.usingadvsetup = false;
        pd.challenge_unset_current();
        pd.challenge_remove_player_lock();
    }
    0
}

pub fn menuhandler_mp_advanced_setup(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_save_and_push_root_dialog(Some(&G_MP_ADVANCED_SETUP_MENU_DIALOG), MENUROOT_MPSETUP);
    }
    ok()
}

/// `mp_close_dialogs_for_new_setup` (setup.c:5157).
fn mp_close_dialogs_for_new_setup(pd: &mut MenuSystem) {
    let prev = pd.mpplayernum;
    let closeme: [&'static MenuDialogDef; 11] = [
        &G_MP_SAVE_SETUP_NAME_MENU_DIALOG,
        &G_MP_SAVE_SETUP_EXISTS_MENU_DIALOG,
        &G_MP_ADD_SIMULANT_MENU_DIALOG,
        &G_MP_CHANGE_SIMULANT_MENU_DIALOG,
        &G_MP_EDIT_SIMULANT_MENU_DIALOG,
        &G_MP_COMBAT_OPTIONS_MENU_DIALOG,
        &G_HTB_OPTIONS_MENU_DIALOG,
        &G_CTC_OPTIONS_MENU_DIALOG,
        &G_KOH_OPTIONS_MENU_DIALOG,
        &G_HTM_OPTIONS_MENU_DIALOG,
        &G_PAC_OPTIONS_MENU_DIALOG,
    ];
    for i in 0..4 {
        pd.mpplayernum = i;
        if pd.menus[i].curdialog.is_none() {
            continue;
        }
        loop {
            let m = pd.mr();
            let mut found = false;
            for j in 0..m.depth {
                for k in 0..m.layers[j].numsiblings as usize {
                    let d = m.dialogs[m.layers[j].siblings[k]].def();
                    if closeme.iter().any(|c| std::ptr::eq(*c, d)) {
                        found = true;
                    }
                }
            }
            if !found {
                break;
            }
            pd.menu_pop_dialog();
        }
    }
    pd.mpplayernum = prev;
}

// ---------------------------------------------------------------------------
// scenarios.c
// ---------------------------------------------------------------------------

pub fn menuhandler_mp_display_team(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    if op == MENUOP_IS_DISABLED {
        return (pd.mp.setup.options & MPOPTION_TEAMSENABLED as u32 == 0).into();
    }
    menuhandler_mp_checkbox_option(pd, op, item, data)
}

pub fn menuhandler_mp_one_hit_kills(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    if op == MENUOP_IS_DISABLED || op == MENUOP_IS_HIDDEN {
        return (!pd.challenge_is_feature_unlocked(MPFEATURE_ONEHITKILLS)).into();
    }
    menuhandler_mp_checkbox_option(pd, op, item, data)
}

/// `menuhandler_mp_slow_motion` (scenarios.c:124).
pub fn menuhandler_mp_slow_motion(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let labels = [240u16, 241, 242];
    let (on, smart) = (MPOPTION_SLOWMOTION_ON as u32, MPOPTION_SLOWMOTION_SMART as u32);
    match op {
        MENUOP_IS_DISABLED | MENUOP_IS_HIDDEN => return (!pd.challenge_is_feature_unlocked(MPFEATURE_SLOWMOTION)).into(),
        MENUOP_GET_OPTION_COUNT => data.value = 3,
        MENUOP_GET_OPTION_TEXT => return pd.lang(tx(B_MPMENU, labels[data.value.clamp(0, 2) as usize])).into(),
        MENUOP_CONFIRM => {
            pd.mp.setup.options &= !(on | smart);
            if data.value == SLOWMOTION_ON {
                pd.mp.setup.options |= on;
            } else if data.value == SLOWMOTION_SMART {
                pd.mp.setup.options |= smart;
            }
        }
        MENUOP_GET_SELECTED_INDEX => {
            data.value = if pd.mp.setup.options & smart != 0 {
                SLOWMOTION_SMART
            } else if pd.mp.setup.options & on != 0 {
                SLOWMOTION_ON
            } else {
                SLOWMOTION_OFF
            };
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_hill_time` (kingofthehill.inc).
pub fn menuhandler_mp_hill_time(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    match op {
        MENUOP_GET_SLIDER_VALUE => data.value = pd.vars.mphilltime as i32,
        MENUOP_CONFIRM => pd.vars.mphilltime = data.value as u8,
        MENUOP_GET_SLIDER_LABEL => data.label = pd.lang(tx(B_MPWEAPONS, 23)).replacen("%d", &(data.value + 10).to_string(), 1),
        _ => {}
    }
    ok()
}

fn scenario_options_dialog(s: u8) -> &'static MenuDialogDef {
    match s as i32 {
        MPSCENARIO_HOLDTHEBRIEFCASE => &G_HTB_OPTIONS_MENU_DIALOG,
        MPSCENARIO_HACKERCENTRAL => &G_HTM_OPTIONS_MENU_DIALOG,
        MPSCENARIO_POPACAP => &G_PAC_OPTIONS_MENU_DIALOG,
        MPSCENARIO_KINGOFTHEHILL => &G_KOH_OPTIONS_MENU_DIALOG,
        MPSCENARIO_CAPTURETHECASE => &G_CTC_OPTIONS_MENU_DIALOG,
        _ => &G_MP_COMBAT_OPTIONS_MENU_DIALOG,
    }
}

/// `mp_options_menu_dialog` (scenarios.c:267).
pub fn mp_options_menu_dialog(pd: &mut MenuSystem, op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    if op == MENUOP_ON_TICK {
        let want = scenario_options_dialog(pd.mp.setup.scenario);
        if let Some(cur) = pd.cur_def() {
            if !std::ptr::eq(cur, want) {
                let all = (0..6).map(|s| scenario_options_dialog(s as u8));
                if all.into_iter().any(|d| std::ptr::eq(d, cur)) {
                    pd.menu_pop_dialog();
                    pd.menu_push_dialog(want);
                }
            }
        }
    }
    0
}

pub fn mp_menu_text_scenario_short_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let s = pd.mp.setup.scenario as usize;
    format!("{}\n", pd.lang(MP_SCENARIO_OVERVIEWS[s.min(5)].shortname).trim_end_matches('\n'))
}

/// `scenario_scenario_menu_handler` (scenarios.c:307).
pub fn scenario_scenario_menu_handler(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    let groups: [(usize, u16); 2] = [(0, 244), (4, 245)];
    let mut teamgame = true;
    if item.param != 0 && (pd.vars.mpquickteam == MPQUICKTEAM_PLAYERSONLY || pd.vars.mpquickteam == MPQUICKTEAM_PLAYERSANDSIMS) {
        teamgame = false;
    }
    let avail = |pd: &MenuSystem, i: usize| pd.challenge_is_feature_unlocked(MP_SCENARIO_OVERVIEWS[i].requirefeature) && (teamgame || !MP_SCENARIO_OVERVIEWS[i].teamonly);
    let list: Vec<usize> = (0..MP_SCENARIO_OVERVIEWS.len()).filter(|&i| avail(pd, i)).collect();
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = list.len() as i32,
        MENUOP_GET_OPTION_TEXT => {
            if let Some(&i) = list.get(data.value.max(0) as usize) {
                return pd.lang(MP_SCENARIO_OVERVIEWS[i].name).into();
            }
        }
        MENUOP_CONFIRM => {
            if let Some(&i) = list.get(data.value.max(0) as usize) {
                pd.mp.setup.scenario = i as u8;
            }
            pd.scenario_init();
        }
        MENUOP_GET_SELECTED_INDEX => {
            if let Some(p) = list.iter().position(|&i| i == pd.mp.setup.scenario as usize) {
                data.value = p as i32;
            }
        }
        MENUOP_GET_OPTGROUP_COUNT => {
            data.value = 2;
            if !teamgame || (!pd.challenge_is_feature_unlocked(MPFEATURE_SCENARIO_KOH) && !pd.challenge_is_feature_unlocked(MPFEATURE_SCENARIO_CTC)) {
                data.value -= 1;
            }
        }
        MENUOP_GET_OPTGROUP_TEXT => return pd.lang(tx(B_MPMENU, groups[data.value.clamp(0, 1) as usize].1)).into(),
        MENUOP_GET_OPTGROUP_START_INDEX => {
            let off = groups[data.value.clamp(0, 1) as usize].0;
            data.groupstartindex = (0..off).filter(|&i| avail(pd, i)).count() as i32;
        }
        _ => {}
    }
    ok()
}

pub fn menuhandler_mp_open_options(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let d = scenario_options_dialog(pd.mp.setup.scenario);
        pd.menu_push_dialog(d);
    }
    ok()
}

// ---------------------------------------------------------------------------
// mainmenu.c
// ---------------------------------------------------------------------------

pub fn menuhandler_screen_ratio(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let options = [223u16, 224];
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = 2,
        MENUOP_GET_OPTION_TEXT => return pd.lang(tx(B_OPTIONS, options[data.value.clamp(0, 1) as usize])).into(),
        MENUOP_CONFIRM => {
            pd.vars.screenratio = data.value as u8;
            pd.vars.modifiedfiles |= MODFILE_GAME;
        }
        MENUOP_GET_SELECTED_INDEX => data.value = pd.vars.screenratio as i32,
        _ => {}
    }
    ok()
}

pub fn menuhandler_screen_split(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let options = [225u16, 226];
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = 2,
        MENUOP_GET_OPTION_TEXT => return pd.lang(tx(B_OPTIONS, options[data.value.clamp(0, 1) as usize])).into(),
        MENUOP_CONFIRM => {
            // (PD closes both players' menus when two are in a game: the
            // menus here run one player at CI.)
            if data.value as u8 != pd.vars.screensplit {
                pd.vars.screensplit = data.value as u8;
                pd.vars.modifiedfiles |= MODFILE_GAME;
            }
        }
        MENUOP_GET_SELECTED_INDEX => data.value = pd.vars.screensplit as i32,
        _ => {}
    }
    ok()
}

pub fn menudialog_main_menu(_pd: &mut MenuSystem, _op: i32, _def: &'static MenuDialogDef, _data: &mut HandlerData) -> i32 {
    0
}

pub fn main_menu_text_label(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let nocheats = [117u16, 118, 119, 120];
    pd.lang(tx(B_OPTIONS, nocheats[item.param.clamp(0, 3) as usize]))
}

/// Solo missions / co-op / counter-op are not part of this recreation.
pub fn menuhandler_main_menu_solo_missions(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.menu_push_dialog(&stubs::STUB_NOT_IN_SPIKE_DIALOG);
    }
    ok()
}

/// `menuhandler_main_menu_combat_simulator` (mainmenu.c:4709).
pub fn menuhandler_main_menu_combat_simulator(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        pd.challenge_determine_unlocked_features();
        pd.vars.mpsetupmenu = MPSETUPMENU_GENERAL;
        pd.menu_save_and_push_root_dialog(Some(&G_COMBAT_SIMULATOR_MENU_DIALOG), MENUROOT_MPSETUP);
        pd.menu_hide_pressstart_labels();
    }
    ok()
}

pub fn menuhandler_main_menu_cooperative(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    menuhandler_main_menu_solo_missions(pd, op, item, data)
}

/// `menuhandler_main_menu_counter_operative` (mainmenu.c:4734): disabled
/// without a second controller.
pub fn menuhandler_main_menu_counter_operative(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, data: &mut HandlerData) -> R {
    if op == MENUOP_IS_DISABLED && pd.connected_pads() & 2 == 0 {
        return HRet::I(1);
    }
    menuhandler_main_menu_solo_missions(pd, op, item, data)
}

// ---------------------------------------------------------------------------
// ingame.c: the pause menu and the end of a match
// ---------------------------------------------------------------------------

/// The chr slots taking part, in slot order.
fn match_slots(pd: &MenuSystem) -> Vec<usize> {
    (0..pd_core::mp::MAX_MPCHRS).filter(|&i| pd.mp.setup.chrslots & (1 << i) != 0).collect()
}

/// The player (`g_Vars.players[]`) whose menu this is: `g_Menus[g_MpPlayerNum].playernum`.
fn menu_player(pd: &MenuSystem) -> Option<&super::MatchViewPlayer> {
    pd.matchview.players.get(pd.mr().playernum)
}

/// `mp_stats_for_player_dropdown_handler` (ingame.c:31): the chrs taking part.
pub fn mp_stats_for_player_dropdown_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let slots = match_slots(pd);
    let p = cur_player(pd);
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = slots.len() as i32,
        MENUOP_GET_OPTION_TEXT => {
            return slots.get(data.value.max(0) as usize).and_then(|&i| pd.mpchr(i)).map_or(String::new(), |c| c.name).into();
        }
        MENUOP_CONFIRM => {
            if let Some(&i) = slots.get(data.value.max(0) as usize) {
                pd.mp_selected_for_stats[p] = i;
            }
        }
        MENUOP_GET_SELECTED_INDEX => {
            if let Some(k) = slots.iter().position(|&i| i == pd.mp_selected_for_stats[p]) {
                data.value = k as i32;
            }
        }
        _ => {}
    }
    ok()
}

/// `menuhandler_mp_end_game` (ingame.c:101): the player aborts and the match
/// ends. (PD marks `g_Vars.currentplayer`; here the menu's own player, the
/// same one in a one-player match.)
pub fn menuhandler_mp_end_game(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let playernum = pd.mr().playernum;
        pd.outcomes.push_back(super::Outcome::EndGame { playernum });
    }
    ok()
}

/// `menuhandler00178018` (ingame.c:114): the challenge's name, shown in a
/// challenge only.
pub fn menuhandler00178018(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN && pd.mp.bossfile.locktype as i32 != MPLOCKTYPE_CHALLENGE {
        return HRet::I(1);
    }
    ok()
}

/// `mp_menu_text_challenge_name` (setup.c:4633).
pub fn mp_menu_text_challenge_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    if pd.mp.bossfile.locktype as i32 != MPLOCKTYPE_CHALLENGE {
        return pd.lang(tx(B_MPMENU, 50));
    }
    format!("{}:\n", pd.challenge_get_name(pd.mp.challenge_index).trim_end_matches('\n'))
}

/// `mp_menu_text_scenario_name` (scenarios.c:296).
pub fn mp_menu_text_scenario_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let s = pd.mp.setup.scenario as usize;
    format!("{}\n", pd.lang(MP_SCENARIO_OVERVIEWS[s.min(5)].name))
}

/// `mp_menu_text_in_game_limit` (ingame.c:125): "10 Min", "10", or the team
/// limit as a challenge scales it.
pub fn mp_menu_text_in_game_limit(pd: &mut MenuSystem, item: &'static MenuItem) -> String {
    let (fmt, value) = match item.param {
        0 => (114, pd.mp.setup.timelimit as i32 + 1),
        1 => (113, pd.mp.setup.scorelimit as i32 + 1),
        2 => (113, pd.mp_calculate_team_score_limit() + 1),
        _ => return String::new(),
    };
    pd.lang(tx(B_MPMENU, fmt)).replacen("%d", &value.to_string(), 1)
}

/// `menuhandler_mp_in_game_limit_label` (ingame.c:144): a limit set to "No
/// Limit" is left out.
pub fn menuhandler_mp_in_game_limit_label(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_IS_HIDDEN {
        let s = &pd.mp.setup;
        let hidden = match item.param {
            0 => s.timelimit == 60,
            1 => s.scorelimit == 100,
            2 => s.teamscorelimit == 400,
            _ => false,
        };
        if hidden {
            return HRet::I(1);
        }
    }
    ok()
}

/// `menuhandler_mp_pause` (ingame.c:157): with two or more players, pause or
/// unpause the match.
pub fn menuhandler_mp_pause(pd: &mut MenuSystem, op: i32, item: &'static MenuItem, _data: &mut HandlerData) -> R {
    if op == MENUOP_CONFIRM {
        let mode = if pd.mp_is_paused() { pd_core::ids::MPPAUSEMODE_UNPAUSED } else { pd_core::ids::MPPAUSEMODE_PAUSED };
        pd.matchview.paused = mode;
        pd.outcomes.push_back(super::Outcome::SetPaused(mode));
    }
    if op == MENUOP_IS_HIDDEN && pd.matchview.players.len() == 1 {
        return HRet::I(1);
    }
    if op == MENUOP_IS_PREFOCUSED && item.param == 1 {
        return HRet::I(1);
    }
    ok()
}

/// `menutext_pause_or_unpause` (ingame.c:182).
pub fn menutext_pause_or_unpause(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    pd.lang(tx(B_MPMENU, if pd.mp_is_paused() { 289 } else { 288 }))
}

/// `menutext_match_time` (ingame.c:191): the time played.
pub fn menutext_match_time(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    pd_core::text::format_time(pd.matchview.stagetime60, pd_core::text::TIMEPRECISION_SECONDS)
}

/// `menuhandler_inventory_list` (mainmenu.c:4176): the player's weapons; a
/// pick equips one (the game does, `bgun_equip_weapon2`), a focus shows its
/// description.
pub fn menuhandler_inventory_list(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let Some(p) = menu_player(pd).cloned() else { return ok() };
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = p.inventory.len() as i32,
        MENUOP_GET_OPTION_TEXT => return p.inventory.get(data.value.max(0) as usize).map_or(String::new(), |r| r.name.clone()).into(),
        MENUOP_CONFIRM => {
            // The game equips the row's weapon, or switches its device.
            let playernum = pd.mr().playernum;
            pd.outcomes.push_back(super::Outcome::Equip { playernum, index: data.value.max(0) as usize });
        }
        MENUOP_GET_SELECTED_INDEX => data.value = p.invcur,
        MENUOP_ON_OPTION_FOCUS => {
            pd.m().mppause_weaponnum = p.inventory.get(data.value.max(0) as usize).map_or(0, |r| r.weaponnum as i32);
        }
        _ => {}
    }
    ok()
}

/// `mp_menu_text_weapon_description` (ingame.c:426).
pub fn mp_menu_text_weapon_description(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let w = pd.mr().mppause_weaponnum;
    let Some(row) = menu_player(pd).and_then(|p| p.inventory.iter().find(|r| r.weaponnum as i32 == w)) else { return "\n".into() };
    // lang_get's strings end in a line break.
    let mut d = row.description.clone();
    if !d.ends_with('\n') {
        d.push('\n');
    }
    d
}

/// `mp_menu_title_stats_for` (ingame.c:438): "Stats for <name>".
pub fn title_mp_menu_title_stats_for(pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    let slot = pd.mp_selected_for_stats[cur_player(pd)];
    let name = pd.mpchr(slot).map(|c| c.name).unwrap_or_default();
    pd.lang(tx(B_MPMENU, 280)).replacen("%s", &name, 1)
}

/// `mp_menu_text_weapon_of_choice_name` (ingame.c:454).
pub fn mp_menu_text_weapon_of_choice_name(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    menu_player(pd).map(|p| p.weapon_of_choice.clone()).unwrap_or_default()
}

/// An award's name: `g_AwardNames` is `L_MPMENU_000` .. `L_MPMENU_016`, in
/// `AWARD_*` bit order.
fn award_text(pd: &MenuSystem, award: Option<u8>) -> String {
    award.map_or(String::new(), |a| pd.lang(tx(B_MPMENU, a as u16)))
}

/// `mp_menu_text_award1` (ingame.c:459).
pub fn mp_menu_text_award1(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let a = menu_player(pd).and_then(|p| p.award1);
    award_text(pd, a)
}

/// `mp_menu_text_award2` (ingame.c:464).
pub fn mp_menu_text_award2(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let a = menu_player(pd).and_then(|p| p.award2);
    award_text(pd, a)
}

/// `mp_menu_text_placement_with_suffix` (ingame.c:606): "1st" .. "12th".
pub fn mp_menu_text_placement_with_suffix(pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    let placement = pd.mp.players[cur_player(pd)].base.stats.placement.min(11);
    pd.lang(tx(B_MPMENU, 264 + placement as u16))
}

/// `mp_placement_menu_handler` (ingame.c:626): the winner's place pulses yellow.
pub fn mp_placement_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    if op == MENUOP_GET_LABEL_COLOURS && pd.mp.players[cur_player(pd)].base.stats.placement == 0 {
        data.colour2 = pd_core::text::colour_blend(data.colour2, 0xffff00ff, (super::gfx::sin_osc(pd.frac20, 40.0) * 255.0) as u32);
    }
    ok()
}

/// `mp_awards_menu_handler` (ingame.c:637): a star for each medal won, right
/// to left: KillMaster red, Headshot yellow, Accuracy green, Survivor blue.
pub fn mp_awards_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    if op == MENUOP_RENDER {
        let Some(rd) = data.render else { return ok() };
        let medals = pd.mp.players[cur_player(pd)].medals;
        let mut x = rd.x + rd.width - 15;
        for i in 0..4 {
            if medals & (1 << i) != 0 {
                let colour = [0xff7f7fff, 0xbfbf00ff, 0x00ff00ff, 0x00bfbfff][i];
                // gSPTextureRectangle(x, y - 2, x + 11, y + 9, s 0x0010, t 0x0150, dtdy -1024).
                pd.draw_envstar_rect(x, rd.y - 2, x + 11, rd.y + 9, colour, 0.5, 10.5, -1.0);
                x -= 14;
            }
        }
    }
    ok()
}

/// `mp_player_title_menu_handler` (ingame.c:707): a title that changed this
/// match pulses yellow.
pub fn mp_player_title_menu_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let pl = &pd.mp.players[cur_player(pd)];
    if op == MENUOP_GET_LABEL_COLOURS && pl.title != pl.newtitle {
        data.colour2 = pd_core::text::colour_blend(data.colour2, 0xffff00ff, (super::gfx::sin_osc(pd.frac20, 40.0) * 255.0) as u32);
    }
    ok()
}

/// `mp_confirm_player_name_handler` (ingame.c:724): the new player's name,
/// then where to save it.
pub fn mp_confirm_player_name_handler(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> R {
    let p = cur_player(pd);
    match op {
        MENUOP_GET_KEYBOARD_STRING => data.string = kb_from(&pd.mp.players[p].base.name),
        MENUOP_SET_KEYBOARD_STRING => pd.mp.players[p].base.name = format!("{}\n", kb_to(&data.string)),
        MENUOP_CONFIRM => pd.filemgr_push_select_location_dialog(6, FILETYPE_MPPLAYER),
        _ => {}
    }
    ok()
}
