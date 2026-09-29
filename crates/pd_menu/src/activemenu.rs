//! The active menu's "Pick Target" dialog (`activemenu.c:64-239`): ordering
//! a simulant (or all of them) to Attack, the match hands the menus the chrs
//! to choose from ([`MenuSystem::am_open_pick_target`]); the names are drawn
//! in their team colours, and a pick goes back as [`crate::Outcome::PickTarget`].
//! The rest of the active menu is the match's (`pd_sim::player::activemenu`).

use pd_core::lang::tx;

use super::generated::*;
use super::types::*;
use super::MenuSystem;

pub static G_AM_PICK_TARGET_MENU_ITEMS: [MenuItem; 2] = [
    MenuItem { ty: MENUITEMTYPE_LIST, param: 0, flags: MENUITEMFLAG_LIST_CUSTOMRENDER, param2: P::Num(0x5a), param3: P::Num(0), handler: H::Fn(am_pick_target_menu_list) },
    MenuItem::END,
];

/// `g_AmPickTargetMenuDialog`. Its handler (`am_pick_target_menu_dialog`)
/// only keeps the player's control off while it is open, which the game does
/// from the menu's open state.
pub static G_AM_PICK_TARGET_MENU_DIALOG: MenuDialogDef = MenuDialogDef {
    name: "g_AmPickTargetMenuDialog",
    ty: MENUDIALOGTYPE_DANGER,
    title: P::Text(tx(B_OPTIONS, 492)), // "Pick Target"
    items: &G_AM_PICK_TARGET_MENU_ITEMS,
    handler: None,
    flags: 0,
    nextsibling: None,
};

/// The list's team colours (`am_pick_target_menu_list`'s `teamcolours`).
const TEAMCOLOURS: [u32; 8] = [0xff666600, 0xffff0000, 0x4444ff00, 0xff00ff00, 0x00ffff00, 0xff885500, 0x8800ff00, 0x88445500];

/// `am_pick_target_menu_list` (`activemenu.c:77`): the rows the match listed
/// for the menu's player.
pub fn am_pick_target_menu_list(pd: &mut MenuSystem, op: i32, _item: &'static MenuItem, data: &mut HandlerData) -> HRet {
    let playernum = pd.mr().playernum;
    let rows = pd.picktargets.get(playernum).cloned().unwrap_or_default();
    match op {
        MENUOP_GET_OPTION_COUNT => data.value = rows.len() as i32,
        MENUOP_CONFIRM => {
            if let Some(&(chrnum, _)) = rows.get(data.value.max(0) as usize) {
                pd.outcomes.push_back(super::Outcome::PickTarget { playernum, chrnum: chrnum as usize });
            }
            pd.menu_pop_dialog();
        }
        MENUOP_GET_SELECTED_INDEX => data.value = 0xfffff,
        MENUOP_RENDER => {
            let (Some(rd), Some(&(_, slot))) = (data.render, rows.get(data.unk04.max(0) as usize)) else { return HRet::I(0) };
            let Some(chr) = pd.mpchr(slot as usize) else { return HRet::I(0) };
            let mut colour = TEAMCOLOURS[chr.team as usize & 7] | (rd.colour & 0xff);
            if rd.unk10 {
                let weight = (super::gfx::sin_osc(pd.frac20, 40.0) * 255.0) as u32;
                colour = pd_core::text::colour_blend(rd.colour | 0xffffff00, pd_core::text::colour_blend(colour, colour & 0xff, 0x7f), weight);
            }
            let (vw, vh) = (pd.draw.gfx.w as i32, pd.draw.gfx.h as i32);
            let (mut x, mut y) = (rd.x + 10, rd.y + 1);
            pd.tc().render_v2(&mut x, &mut y, &chr.name, pd_core::text::FontId::Sm, colour, vw, vh, 0, 0);
        }
        MENUOP_GET_OPTION_HEIGHT => data.value = 11, // LINEHEIGHT
        _ => {}
    }
    HRet::I(0)
}

impl MenuSystem {
    /// `am_open_pick_target` (`activemenu.c:64`)'s push: player `playernum`'s
    /// menu opens "Pick Target" over the match, listing `targets` (chr index,
    /// chr slot).
    pub fn am_open_pick_target(&mut self, playernum: usize, targets: Vec<(u8, u8)>) {
        let Some(slot) = self.matchview.players.get(playernum).map(|p| p.slot) else { return };
        if self.picktargets.len() <= playernum {
            self.picktargets.resize(playernum + 1, Vec::new());
        }
        self.picktargets[playernum] = targets;
        let prev = self.mpplayernum;
        self.mpplayernum = slot;
        self.menus[slot].playernum = playernum;
        self.menu_push_root_dialog(&G_AM_PICK_TARGET_MENU_DIALOG, MENUROOT_PICKTARGET);
        self.mpplayernum = prev;
    }
}
