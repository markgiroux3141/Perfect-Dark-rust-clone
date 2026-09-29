//! `menu.c` (+ `menu_tick` from `menutick.c`): the dialog stack and everything
//! that isn't a specific item type.
//!
//! PD's globals map onto [`MenuSystem`]: `g_Menus[4]` → `pd.menus`, `g_MenuData` →
//! `pd.menudata`, `g_MpPlayerNum` → `pd.mpplayernum`. Pointers become indices:
//! `curdialog` / `layer->siblings[]` index `menu.dialogs`, `focuseditem` is an
//! index into its dialog's `items`, and a row's `blockindex` indexes
//! `menu.blocks` (one [`ItemData`] per item that needs one, rather than PD's
//! 1-5 words — the capacity check is the only thing that changes).

use super::gfx::{mixcolour, sin_osc, wave1, wave2, Pal};
use pd_core::text::{colour_blend, measure, FontId, DIAGMODE_FADEIN, DIAGMODE_REDRAW};
use super::types::*;
use super::{generated as gd, MenuSystem};

pub const NUM_DIALOGS: usize = 10;
pub const NUM_ROWS: usize = 88;
pub const NUM_COLS: usize = 12;
pub const NUM_BLOCKS: usize = 80;

/// `struct menudialog` (types.h:3669).
#[derive(Clone, Copy, Default)]
pub struct MenuDialog {
    pub definition: Option<&'static MenuDialogDef>,
    pub colstart: u8,
    pub numcols: u8,
    pub blockstart: u16,
    pub focuseditem: Option<usize>,
    pub dimmed: bool,
    pub unk10: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub contentwidth: i32,
    pub contentheight: i32,
    pub dstx: i32,
    pub dsty: i32,
    pub dstwidth: i32,
    pub dstheight: i32,
    pub ty: u8,
    pub type2: u8,
    pub transitionfrac: f32,
    pub colourweight: u32,
    pub redrawtimer: f32,
    pub unk4c: f32,
    pub statefrac: f32,
    pub unk54: i32,
    pub unk58: u32,
    pub unk5c: i32,
    pub state: u8,
    pub scroll: i32,
    pub dstscroll: i32,
    pub swipedir: i8,
    pub unk6e: u8,
}

impl MenuDialog {
    pub fn def(&self) -> &'static MenuDialogDef {
        self.definition.expect("dialog without definition")
    }
}

#[derive(Clone, Copy, Default)]
pub struct MenuLayer {
    pub siblings: [usize; 5],
    pub numsiblings: i8,
    pub cursibling: i8,
}

#[derive(Clone, Copy, Default)]
pub struct MenuRow {
    pub height: i16,
    pub itemindex: u8,
    pub blockindex: i8,
}

#[derive(Clone, Copy, Default)]
pub struct MenuColumn {
    pub width: i16,
    pub height: i16,
    pub rowstart: u16,
    pub numrows: u8,
}

/// `struct menumodel` (types.h:3827) — the fields the menus read and write.
#[derive(Clone, Copy, Default)]
pub struct MenuModel {
    pub loaddelay: u8,
    pub headnum: i32,
    pub newparams: u32,
    pub curparams: u32,
    pub newanimnum: i32,
    pub curanimnum: i32,
    pub curposx: f32,
    pub curposy: f32,
    pub curposz: f32,
    pub curscale: f32,
    pub currotx: f32,
    pub curroty: f32,
    pub currotz: f32,
    pub displacex: f32,
    pub displacey: f32,
    pub displacez: f32,
    pub newposx: f32,
    pub newposy: f32,
    pub newposz: f32,
    pub newscale: f32,
    pub newrotx: f32,
    pub newroty: f32,
    pub newrotz: f32,
    pub zoom: f32,
    pub configurefrac: f32,
    pub flags: u8,
    pub bodynum: i32,
    pub zoomtimer60: i32,
    pub rottimer60: i32,
    pub removingpiece: bool,
    pub perfectheadnum: u8,
    pub isperfecthead: bool,
    pub reverseanim: bool,
    pub configuring: bool,
    pub drawbehinddialog: bool,
    /// `partvisibility`: the head carousel hides sunglasses / closed eyes / the
    /// hudpiece (setup.c:1885).
    pub hideheadparts: bool,
    /// The body animation's current frame (`bodyanim`), advanced at PD's
    /// quarter speed (`model_tick_anim_quarter_speed`).
    pub anim_frame: f32,
}

#[derive(Clone, Copy, Default)]
pub struct MenuItemRedrawInfo {
    pub item: Option<*const MenuItem>,
    pub timer60: f32,
}

/// `menudata_mpsetup` / the other members of `menu`'s trailing union that the
/// Combat Simulator uses.
#[derive(Clone, Copy, Default)]
pub struct MpSetupMenuData {
    pub slotindex: i32,
    pub slotcount: i32,
    pub botprofileindex: i32,
}

/// `struct menu` (types.h:3908).
#[derive(Clone)]
pub struct Menu {
    pub dialogs: [MenuDialog; NUM_DIALOGS],
    pub numdialogs: usize,
    pub layers: [MenuLayer; 6],
    pub depth: usize,
    pub curdialog: Option<usize>,
    pub rows: [MenuRow; NUM_ROWS],
    pub rowend: usize,
    pub cols: [MenuColumn; NUM_COLS],
    pub colend: usize,
    pub blocks: [ItemData; NUM_BLOCKS],
    pub blockend: usize,
    pub xrepeattimer60: i32,
    pub xrepeatcount: i16,
    pub xrepeatdir: i16,
    pub xrepeatmode: i16,
    pub yrepeattimer60: i32,
    pub yrepeatcount: i16,
    pub yrepeatdir: i16,
    pub yrepeatmode: i16,
    pub playernum: usize,
    pub openinhibit: u8,
    pub menumodel: MenuModel,
    pub bannernum: i8,
    pub itemredrawinfo: [MenuItemRedrawInfo; 4],
    pub mpsetup: MpSetupMenuData,
    /// `fm.unke40_00`: swallow this frame's input (set when a dialog opens).
    pub inhibit_input: bool,
    /// `training.unke1c` / `training.mpconfig`: the challenge a confirm dialog shows.
    pub training_slot: i32,
    pub training_config: Option<usize>,
    /// `mppause.weaponnum`: the inventory row in focus (its description scrolls
    /// in the marquee).
    pub mppause_weaponnum: i32,
    /// `fm`: the file manager's state (`menudata_filemgr`).
    pub fm: super::filemgr::FilemgrMenuData,
}

impl Default for Menu {
    fn default() -> Self {
        Menu {
            dialogs: [MenuDialog::default(); NUM_DIALOGS],
            numdialogs: 0,
            layers: [MenuLayer::default(); 6],
            depth: 0,
            curdialog: None,
            rows: [MenuRow::default(); NUM_ROWS],
            rowend: 0,
            cols: [MenuColumn::default(); NUM_COLS],
            colend: 0,
            blocks: [ItemData::default(); NUM_BLOCKS],
            blockend: 0,
            xrepeattimer60: 0,
            xrepeatcount: 0,
            xrepeatdir: 0,
            xrepeatmode: 0,
            yrepeattimer60: 0,
            yrepeatcount: 0,
            yrepeatdir: 0,
            yrepeatmode: 0,
            playernum: 0,
            openinhibit: 0,
            menumodel: MenuModel::default(),
            bannernum: -1,
            itemredrawinfo: [MenuItemRedrawInfo::default(); 4],
            mpsetup: MpSetupMenuData::default(),
            inhibit_input: false,
            training_slot: 0,
            training_config: None,
            mppause_weaponnum: 0,
            fm: Default::default(),
        }
    }
}

/// `struct menudata` (types.h:4722) — the parts the Combat Simulator uses.
#[derive(Clone)]
pub struct MenuData {
    pub count: i32,
    pub root: i32,
    pub nextroot: i32,
    pub nextdialog: Option<&'static MenuDialogDef>,
    pub bgopacityfrac: f32,
    pub bg: u8,
    pub nextbg: u8,
    pub screenshottimer: u8,
    pub playerjoinalpha: [u8; 4],
    pub bannernum: i8,
    pub hudpiece: MenuModel,
    pub hudpieceactive: bool,
    pub triggerhudpiece: bool,
    pub usezbuf: bool,
    pub checkroots: bool,
    /// The saves waiting for the dialogs to settle (`menu_queue_save`): 0-3 a
    /// player's file, 4 the agent's; 0xff unused.
    pub pendingsaves: [u8; 5],
    pub numpendingsaves: i32,
    pub savetimer: i32,
    /// `var8009dfc0`: a menu background has faded in (and not out again).
    pub bgsettled: bool,
}

impl Default for MenuData {
    fn default() -> Self {
        MenuData {
            count: 0,
            root: 0,
            nextroot: -1,
            nextdialog: None,
            bgopacityfrac: 0.0,
            bg: 0,
            nextbg: 255,
            screenshottimer: 0,
            playerjoinalpha: [0; 4],
            bannernum: -1,
            hudpiece: MenuModel::default(),
            hudpieceactive: false,
            triggerhudpiece: false,
            usezbuf: false,
            checkroots: false,
            pendingsaves: [0xff; 5],
            numpendingsaves: 0,
            savetimer: 0,
            bgsettled: false,
        }
    }
}

/// `struct menurendercontext` (types.h:5216).
#[derive(Clone, Copy)]
pub struct Ctx {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub item: &'static MenuItem,
    /// 0 unfocused, 1 focused, 3 focused while the dialog is dimmed.
    pub focused: i32,
    pub dialog: usize,
    pub data: Option<usize>,
    pub unk18: bool,
}

/// `inputs->leftright = inputs->updown = ... = 0` (menu.c:3940).
pub fn zero_inputs(i: &mut MenuInputs) {
    i.leftright = 0;
    i.updown = 0;
    i.select = 0;
    i.back = 0;
    i.xaxis = 0;
    i.yaxis = 0;
    i.shoulder = 0;
    i.back2 = 0;
    i.unk14 = 0;
}

/// `menu_get_item_blocks_required` (menu.c:498): whether the item keeps a
/// data block.
fn needs_block(ty: u8) -> bool {
    matches!(
        ty,
        MENUITEMTYPE_SLIDER
            | MENUITEMTYPE_CHECKBOX
            | MENUITEMTYPE_RANKING
            | MENUITEMTYPE_14
            | MENUITEMTYPE_18
            | MENUITEMTYPE_SCROLLABLE
            | MENUITEMTYPE_MARQUEE
            | MENUITEMTYPE_CONTROLLER
            | MENUITEMTYPE_LIST
            | MENUITEMTYPE_DROPDOWN
            | MENUITEMTYPE_PLAYERSTATS
            | MENUITEMTYPE_KEYBOARD
            | MENUITEMTYPE_10
            | MENUITEMTYPE_16
    )
}

impl MenuSystem {
    pub fn m(&mut self) -> &mut super::menu::Menu {
        let p = self.mpplayernum;
        &mut self.menus[p]
    }
    pub fn mr(&self) -> &super::menu::Menu {
        &self.menus[self.mpplayernum]
    }
    pub fn dlg(&mut self, di: usize) -> &mut MenuDialog {
        let p = self.mpplayernum;
        &mut self.menus[p].dialogs[di]
    }
    pub fn cur_def(&self) -> Option<&'static MenuDialogDef> {
        let m = self.mr();
        m.curdialog.map(|d| m.dialogs[d].def())
    }

    /// `menu_play_sound` (menu.c:137).
    pub fn menu_play_sound(&mut self, menusound: i32) {
        let (sound, pitch) = match menusound {
            MENUSOUND_SWIPE => (gd::SFXNUM_05BB_MENU_SWIPE, 1.0),
            MENUSOUND_OPENDIALOG => (gd::SFXNUM_05BC_MENU_OPENDIALOG, 1.0),
            MENUSOUND_FOCUS => (gd::SFXNUM_0441_MENU_FOCUS, 1.0),
            MENUSOUND_SELECT => (gd::SFXNUM_05DD_MENU_SELECT, 1.0),
            MENUSOUND_ERROR => (gd::SFXMAP_8040_MENU_ERROR, 0.4),
            MENUSOUND_EXPLOSION => (gd::SFXMAP_8098_EXPLOSION, 1.0),
            MENUSOUND_TOGGLEON => (gd::SFXNUM_05DD_MENU_SELECT, 1.0),
            MENUSOUND_TOGGLEOFF => (gd::SFXNUM_043E_MENU_SUBFOCUS, 1.0),
            MENUSOUND_SUBFOCUS => (gd::SFXNUM_043E_MENU_SUBFOCUS, 1.0),
            MENUSOUND_KEYBOARDFOCUS => (gd::SFXNUM_00EA_PICKUP_AMMO, 3.5),
            MENUSOUND_KEYBOARDCANCEL => (gd::SFXNUM_002B_MENU_CANCEL, 0.41904801130295),
            _ => return,
        };
        // AL_SNDP_VOL_EVT 0x4000 halves the volume of the keyboard focus sound.
        let vol = if menusound == MENUSOUND_KEYBOARDFOCUS { 0.5 } else { 1.0 };
        self.play_sound(sound, pitch, vol);
    }

    // ---- item redraw timers (menu.c:396) ----

    pub fn menu_find_item_redraw_info(&mut self, item: Option<*const MenuItem>) -> Option<usize> {
        self.m().itemredrawinfo.iter().position(|r| r.item == item)
    }
    pub fn menu_set_item_redraw_timer(&mut self, item: &'static MenuItem, timer60: f32) {
        let key = Some(item as *const MenuItem);
        if let Some(i) = self.menu_find_item_redraw_info(key) {
            self.m().itemredrawinfo[i].timer60 = timer60;
            return;
        }
        if let Some(i) = self.menu_find_item_redraw_info(None) {
            self.m().itemredrawinfo[i] = MenuItemRedrawInfo { item: key, timer60 };
        }
    }
    pub fn menu_remove_item_redraw_info(&mut self, item: &'static MenuItem) {
        if let Some(i) = self.menu_find_item_redraw_info(Some(item as *const MenuItem)) {
            self.m().itemredrawinfo[i].item = None;
        }
    }
    fn menu_increment_item_redraw_timers(&mut self) {
        let d = self.lv.diffframe60f / 60.0;
        for r in self.m().itemredrawinfo.iter_mut() {
            if r.item.is_some() {
                r.timer60 += d;
            }
        }
    }
    fn menu_remove_all_item_redraw_info(&mut self) {
        for r in self.m().itemredrawinfo.iter_mut() {
            r.item = None;
        }
    }

    // ---- text resolution (menu.c:459) ----

    pub fn menu_resolve_text(&mut self, p: P, item: &'static MenuItem) -> Option<String> {
        match p {
            P::Num(0) => None,
            P::Num(_) => None,
            P::Text(t) => Some(self.lang(t)),
            P::Fn(f) => Some(f(self, item)),
            P::DFn(_) => None,
        }
    }
    pub fn menu_resolve_param2_text(&mut self, item: &'static MenuItem) -> Option<String> {
        self.menu_resolve_text(item.param2, item)
    }
    pub fn menu_resolve_dialog_title(&mut self, def: &'static MenuDialogDef) -> String {
        match def.title {
            P::Text(t) => self.lang(t),
            P::DFn(f) => f(self, def),
            _ => String::new(),
        }
    }

    fn measure(&self, text: &str, font: FontId) -> (i32, i32) {
        measure(self.draw.res.fonts.get(font), text, 0)
    }

    /// `menu_calculate_item_size` (menu.c:535).
    pub fn menu_calculate_item_size(&mut self, item: &'static MenuItem, dialog: Option<usize>) -> (i16, i16) {
        if let Some(h) = item.fn_handler() {
            let mut hd = HandlerData::default();
            if h(self, MENUOP_IS_HIDDEN, item, &mut hd).int() != 0 {
                return (0, 0);
            }
        }
        let mut font = FontId::Sm;
        let (mut w, mut h): (i32, i32);
        match item.ty {
            MENUITEMTYPE_CONTROLLER => {
                h = 150;
                w = 230;
            }
            MENUITEMTYPE_18 => {
                h = if item.param2.num() == 1 { 170 } else { 126 };
                w = 210;
            }
            MENUITEMTYPE_14 => {
                w = 90;
                h = 54;
            }
            MENUITEMTYPE_METER => {
                w = 24;
                h = 6;
            }
            MENUITEMTYPE_KEYBOARD => {
                w = 130;
                h = 73;
            }
            MENUITEMTYPE_LIST => {
                if item.param2.num() > 0 {
                    w = item.param2.num();
                } else {
                    w = 80;
                    if item.flags & MENUITEMFLAG_LIST_WIDE != 0 {
                        w = 180;
                    }
                }
                h = if item.param3.num() > 0 { item.param3.num() } else { 121 };
            }
            MENUITEMTYPE_DROPDOWN => {
                let text = self.menu_resolve_param2_text(item);
                if text.as_deref() == Some("") {
                    w = 0;
                    h = 0;
                } else {
                    let mut textwidth = 0;
                    if let Some(t) = &text {
                        textwidth = self.measure(t, FontId::Sm).1;
                    }
                    w = textwidth + 20;
                    h = 12;
                    if let Some(hf) = item.fn_handler() {
                        let mut hd = HandlerData::default();
                        hf(self, MENUOP_GET_SELECTED_INDEX, item, &mut hd);
                        hd.unk04 = 0;
                        let text2 = hf(self, MENUOP_GET_OPTION_TEXT, item, &mut hd).text();
                        let tw = self.measure(&text2, FontId::Sm).1;
                        w += tw + 10;
                    }
                }
            }
            MENUITEMTYPE_13 => {
                w = 70;
                h = 50;
            }
            MENUITEMTYPE_SLIDER => {
                if let Some(d) = dialog {
                    if self.mr().dialogs[d].unk6e != 0 {
                        return (120, 22);
                    }
                }
                w = 150;
                h = 12;
                if item.flags & MENUITEMFLAG_SLIDER_ALTSIZE != 0 {
                    h = 22;
                    w = 120;
                }
            }
            MENUITEMTYPE_CHECKBOX => {
                if item.flags & MENUITEMFLAG_SMALLFONT != 0 {
                    font = FontId::Xs;
                }
                let text = self.menu_resolve_param2_text(item);
                match text {
                    None => {
                        w = 120;
                        h = 12;
                    }
                    Some(t) if t.is_empty() => {
                        w = 0;
                        h = 0;
                    }
                    Some(t) => {
                        w = self.measure(&t, font).1 + 34;
                        h = 12;
                    }
                }
                h = 12.max(if w == 0 { 12 } else { h });
                if w == 0 {
                    h = 12;
                }
            }
            MENUITEMTYPE_MODEL => {
                w = item.param2.num();
                h = item.param3.num();
            }
            MENUITEMTYPE_SEPARATOR => {
                w = 1;
                if item.param2.num() != 0 {
                    w = item.param2.num();
                }
                h = 5;
            }
            MENUITEMTYPE_MARQUEE => {
                w = 1;
                h = if item.flags & MENUITEMFLAG_SMALLFONT != 0 { LINEHEIGHT } else { LINEHEIGHT + 2 };
            }
            MENUITEMTYPE_LABEL | MENUITEMTYPE_SELECTABLE => {
                let Some(text) = self.menu_resolve_param2_text(item) else {
                    return (0, 0);
                };
                if item.flags & MENUITEMFLAG_SMALLFONT != 0 {
                    font = FontId::Xs;
                }
                if item.flags & MENUITEMFLAG_BIGFONT != 0 {
                    font = FontId::Md;
                }
                if text.is_empty() {
                    w = 0;
                    h = 0;
                } else {
                    let (th, tw) = self.measure(&text, font);
                    w = tw + 8;
                    if item.flags & (MENUITEMFLAG_LESSLEFTPADDING | MENUITEMFLAG_ADJUSTWIDTH) == 0 {
                        w += 20;
                    }
                    h = th + 3;
                    if item.flags & MENUITEMFLAG_SMALLFONT != 0 {
                        h -= 2;
                    }
                    if item.flags & (MENUITEMFLAG_LABEL_HASRIGHTTEXT | MENUITEMFLAG_BIGFONT) == 0 {
                        // @bug (menu.c:676): PD compares the pointer against "",
                        // so any resolved text, even empty, is measured.
                        if let Some(t3) = self.menu_resolve_text(item.param3, item) {
                            let tw3 = self.measure(&t3, font).1;
                            w += tw3 + 5;
                            if item.flags & MENUITEMFLAG_ADJUSTWIDTH != 0 {
                                w -= 6;
                            }
                        }
                    }
                }
                if item.flags & MENUITEMFLAG_BIGFONT != 0 {
                    h = 28;
                    w += 36;
                }
                if item.flags & MENUITEMFLAG_LESSHEIGHT != 0 {
                    h -= 1;
                }
            }
            MENUITEMTYPE_SCROLLABLE => {
                w = if item.param2.num() > 0 { item.param2.num() } else { 240 };
                h = if item.param3.num() > 0 { item.param3.num() } else { 150 };
            }
            MENUITEMTYPE_07 => {
                w = 120;
                h = 120;
            }
            MENUITEMTYPE_PLAYERSTATS => {
                w = 125;
                h = 68;
            }
            MENUITEMTYPE_RANKING => {
                w = 125;
                h = 58;
            }
            MENUITEMTYPE_10 => {
                w = if item.param2.num() != 0 { item.param2.num() + 2 } else { 66 };
                h = w;
            }
            MENUITEMTYPE_16 => {
                w = 66;
                h = 66;
            }
            MENUITEMTYPE_CAROUSEL => {
                w = 130;
                h = item.param3.num();
            }
            _ => {
                w = 80;
                h = 12;
            }
        }
        (w as i16, h as i16)
    }

    /// `dialog_init_blocks` (menu.c:860).
    fn dialog_init_blocks(&mut self, def: &'static MenuDialogDef, di: usize) {
        let m = self.m();
        let mut colindex = m.colend as isize - 1;
        let mut rowindex = m.rowend;
        let mut blockindex = m.blockend;
        m.dialogs[di].numcols = 0;
        m.dialogs[di].colstart = (colindex + 1) as u8;
        m.dialogs[di].blockstart = blockindex as u16;
        let mut newcolumn = true;
        for (itemindex, item) in def.items.iter().enumerate() {
            if item.ty == MENUITEMTYPE_END {
                break;
            }
            if item.flags & MENUITEMFLAG_NEWCOLUMN != 0 {
                newcolumn = true;
            }
            if newcolumn {
                m.dialogs[di].numcols += 1;
                colindex += 1;
                let c = &mut m.cols[colindex as usize];
                c.width = 0;
                c.height = 0;
                c.numrows = 0;
                c.rowstart = rowindex as u16;
                newcolumn = false;
            }
            if needs_block(item.ty) && blockindex < NUM_BLOCKS {
                m.rows[rowindex].blockindex = blockindex as i8;
                m.blocks[blockindex] = ItemData::default();
                blockindex += 1;
            } else {
                m.rows[rowindex].blockindex = -1;
            }
            m.rows[rowindex].itemindex = itemindex as u8;
            m.cols[colindex as usize].numrows += 1;
            rowindex += 1;
        }
        m.colend = (colindex + 1) as usize;
        m.rowend = rowindex;
        m.blockend = blockindex;
    }

    /// `dialog_tick_height` (menu.c:917).
    fn dialog_tick_height(&mut self, di: usize) {
        let d = self.mr().dialogs[di];
        let def = d.def();
        let bodyheight = d.height - LINEHEIGHT - 1;
        if def.flags & MENUDIALOGFLAG_SMOOTHSCROLLABLE == 0 && self.menudata.root != MENUROOT_TRAINING && bodyheight < d.contentheight {
            for i in 0..d.numcols as usize {
                let colindex = d.colstart as usize + i;
                let m = self.m();
                let mut remaining = m.cols[colindex].height as i32 - bodyheight;
                if remaining > 0 {
                    for j in 0..m.cols[colindex].numrows as usize {
                        if remaining > 0 {
                            let rowindex = m.cols[colindex].rowstart as usize + j;
                            let item = &def.items[m.rows[rowindex].itemindex as usize];
                            let mut itemheight = 0;
                            match item.ty {
                                MENUITEMTYPE_LIST => {
                                    if item.flags & MENUITEMFLAG_LIST_CUSTOMRENDER != 0 {
                                        itemheight = remaining;
                                        if m.rows[rowindex].height as i32 - itemheight < 30 {
                                            itemheight = m.rows[rowindex].height as i32 - 30;
                                        }
                                    } else {
                                        itemheight = (remaining + 10) / 11 * 11;
                                    }
                                }
                                MENUITEMTYPE_SCROLLABLE | MENUITEMTYPE_MODEL => {
                                    itemheight = remaining;
                                    if m.rows[rowindex].height as i32 - remaining < 50 {
                                        itemheight = m.rows[rowindex].height as i32 - 50;
                                    }
                                }
                                _ => {}
                            }
                            if itemheight > 0 {
                                m.rows[rowindex].height -= itemheight as i16;
                                remaining -= itemheight;
                            }
                        }
                    }
                }
            }
        }
    }

    /// `dialog_calculate_content_size` (menu.c:970).
    fn dialog_calculate_content_size(&mut self, def: &'static MenuDialogDef, di: usize) {
        let colstart = self.mr().dialogs[di].colstart as usize;
        let numcols = self.mr().dialogs[di].numcols as usize;
        let mut colindex = colstart as isize - 1;
        let mut rowindex = 0usize;
        let mut newcolumn = true;
        for item in def.items.iter() {
            if item.ty == MENUITEMTYPE_END {
                break;
            }
            if item.flags & MENUITEMFLAG_NEWCOLUMN != 0 {
                newcolumn = true;
            }
            if newcolumn {
                colindex += 1;
                let m = self.m();
                m.cols[colindex as usize].width = 0;
                m.cols[colindex as usize].height = 0;
                newcolumn = false;
                rowindex = m.cols[colindex as usize].rowstart as usize;
            }
            let (w, h) = self.menu_calculate_item_size(item, Some(di));
            let m = self.m();
            if w > m.cols[colindex as usize].width {
                m.cols[colindex as usize].width = w;
            }
            m.rows[rowindex].height = h;
            m.cols[colindex as usize].height += h;
            rowindex += 1;
        }
        let m = self.mr();
        let mut contentheight = 0;
        let mut contentwidth = 0;
        for i in 0..numcols {
            let c = &m.cols[colstart + i];
            contentwidth += c.width as i32;
            contentheight = contentheight.max(c.height as i32);
        }
        contentheight += 12;
        let title = self.menu_resolve_dialog_title(def);
        let textwidth = self.measure(&title, FontId::Sm).1;
        let titleextra = match self.menudata.root {
            MENUROOT_MPSETUP | MENUROOT_MPPAUSE | MENUROOT_MPENDSCREEN => 17,
            _ => 8,
        };
        if textwidth + titleextra > contentwidth {
            contentwidth = textwidth + titleextra;
        }
        let d = self.dlg(di);
        d.contentwidth = contentwidth;
        d.contentheight = contentheight;
    }

    /// `dialog_find_item` (menu.c:1067): (y, rowindex, colindex).
    pub fn dialog_find_item(&self, di: usize, item: Option<usize>) -> (i32, usize, usize) {
        let m = self.mr();
        let d = &m.dialogs[di];
        for colindex in d.colstart as usize..(d.colstart + d.numcols) as usize {
            let mut y = 0;
            let rs = m.cols[colindex].rowstart as usize;
            for rowindex in rs..rs + m.cols[colindex].numrows as usize {
                if Some(m.rows[rowindex].itemindex as usize) == item {
                    return (y, rowindex, colindex);
                }
                y += m.rows[rowindex].height as i32;
            }
        }
        let colindex = d.colstart as usize;
        (0, m.cols[colindex].rowstart as usize, colindex)
    }

    /// `menu_is_scrollable_unscrollable` (menu.c:1095).
    pub fn menu_is_scrollable_unscrollable(&self, item: &MenuItem) -> bool {
        item.ty == MENUITEMTYPE_SCROLLABLE && (item.param == gd::DESCRIPTION_MPCONFIG || item.param == gd::DESCRIPTION_MPCHALLENGE)
    }

    /// `menu_is_item_disabled` (menu.c:1110).
    pub fn menu_is_item_disabled(&mut self, item: &'static MenuItem, di: usize) -> bool {
        if item.flags & MENUITEMFLAG_ALWAYSDISABLED != 0 {
            return true;
        }
        if self.mp_is_player_locked_out(self.mpplayernum as i32) && item.flags & MENUITEMFLAG_LOCKABLEMAJOR != 0 {
            return true;
        }
        if self.menu_is_scrollable_unscrollable(item) {
            return true;
        }
        if let Some(h) = item.fn_handler() {
            let mut hd = HandlerData::default();
            if h(self, MENUOP_IS_DISABLED, item, &mut hd).int() != 0 {
                return true;
            }
        }
        let (_, h) = self.menu_calculate_item_size(item, Some(di));
        h == 0
    }

    /// `menu_is_item_focusable` (menu.c:1144).
    pub fn menu_is_item_focusable(&mut self, item: &'static MenuItem, di: usize) -> bool {
        match item.ty {
            MENUITEMTYPE_LABEL
            | MENUITEMTYPE_OBJECTIVES
            | MENUITEMTYPE_07
            | MENUITEMTYPE_SEPARATOR
            | MENUITEMTYPE_MODEL
            | MENUITEMTYPE_13
            | MENUITEMTYPE_METER
            | MENUITEMTYPE_MARQUEE
            | MENUITEMTYPE_CONTROLLER => return false,
            _ => {}
        }
        !self.menu_is_item_disabled(item, di)
    }

    /// `dialog_find_item_at_col_y` (menu.c:1174): (item index, row index).
    fn dialog_find_item_at_col_y(&mut self, targety: i32, colindex: usize, di: usize) -> (Option<usize>, usize) {
        let def = self.mr().dialogs[di].def();
        let mut result = None;
        let mut rowres = 0;
        let mut y = 0;
        let rs = self.mr().cols[colindex].rowstart as usize;
        let n = self.mr().cols[colindex].numrows as usize;
        for i in 0..n {
            let rowindex = rs + i;
            let ii = self.mr().rows[rowindex].itemindex as usize;
            if self.menu_is_item_focusable(&def.items[ii], di) {
                result = Some(ii);
                rowres = rowindex;
                if y >= targety {
                    break;
                }
            }
            y += self.mr().rows[rowindex].height as i32;
        }
        (result, rowres)
    }

    /// `dialog_find_first_item` (menu.c:1201).
    pub fn dialog_find_first_item(&mut self, di: usize) -> Option<usize> {
        let d = self.mr().dialogs[di];
        let mut colindex = d.colstart as usize;
        for _ in 0..d.numcols {
            if let (Some(i), _) = self.dialog_find_item_at_col_y(0, colindex, di) {
                return Some(i);
            }
            colindex += 1;
        }
        Some(0)
    }

    /// `dialog_find_first_item_right` (menu.c:1222).
    pub fn dialog_find_first_item_right(&mut self, di: usize) -> Option<usize> {
        let d = self.mr().dialogs[di];
        let mut colindex = d.colstart as isize + d.numcols as isize - 1;
        for _ in 0..d.numcols {
            if let (Some(i), _) = self.dialog_find_item_at_col_y(0, colindex as usize, di) {
                return Some(i);
            }
            colindex -= 1;
        }
        Some(0)
    }

    /// `dialog_change_item_focus_vertically` (menu.c:1243).
    fn dialog_change_item_focus_vertically(&mut self, di: usize, updown: i32) {
        let focused = self.mr().dialogs[di].focuseditem;
        let (_, mut rowindex, colindex) = self.dialog_find_item(di, focused);
        let startrowindex = rowindex;
        let def = self.mr().dialogs[di].def();
        let mut item;
        loop {
            let start = self.mr().cols[colindex].rowstart as i32;
            let end = self.mr().cols[colindex].numrows as i32 + start;
            let mut r = rowindex as i32 + updown;
            if r >= end {
                r = start;
            }
            if r < start {
                r = end - 1;
            }
            rowindex = r as usize;
            item = self.mr().rows[rowindex].itemindex as usize;
            if self.menu_is_item_focusable(&def.items[item], di) {
                break;
            }
            if rowindex == startrowindex {
                break;
            }
        }
        self.dlg(di).focuseditem = Some(item);
    }

    /// `dialog_change_item_focus_horizontally` (menu.c:1284).
    fn dialog_change_item_focus_horizontally(&mut self, di: usize, leftright: i32) -> i32 {
        let focused = self.mr().dialogs[di].focuseditem;
        let (y, _, colindex) = self.dialog_find_item(di, focused);
        let d = self.mr().dialogs[di];
        let startcolindex = colindex as i32;
        let mut colindex = colindex as i32;
        let mut swipedir = 0;
        let mut item;
        loop {
            colindex += leftright;
            if colindex >= d.colstart as i32 + d.numcols as i32 {
                swipedir = 1;
                colindex = d.colstart as i32;
            }
            if colindex < d.colstart as i32 {
                swipedir = -1;
                colindex = d.colstart as i32 + d.numcols as i32 - 1;
            }
            item = self.dialog_find_item_at_col_y(y, colindex as usize, di).0;
            if item.is_some() || colindex == startcolindex {
                break;
            }
        }
        if item.is_some() {
            self.dlg(di).focuseditem = item;
        }
        swipedir
    }

    /// `dialog_change_item_focus` (menu.c:1325).
    fn dialog_change_item_focus(&mut self, di: usize, leftright: i32, updown: i32) -> i32 {
        if leftright == 0 && updown == 0 {
            return 0;
        }
        if updown != 0 {
            self.dialog_change_item_focus_vertically(di, updown);
        }
        let mut swipedir = 0;
        if leftright != 0 {
            swipedir = self.dialog_change_item_focus_horizontally(di, leftright);
        }
        self.run_focus_handler(di);
        swipedir
    }

    fn run_focus_handler(&mut self, di: usize) {
        let d = self.mr().dialogs[di];
        if let Some(fi) = d.focuseditem {
            let item = &d.def().items[fi];
            if let Some(h) = item.fn_handler() {
                let mut hd = HandlerData::default();
                h(self, MENUOP_ON_FOCUS, item, &mut hd);
            }
        }
    }

    /// `menu_open_dialog` (menu.c:1353).
    fn menu_open_dialog(&mut self, def: &'static MenuDialogDef, di: usize) {
        let unk6e = match self.menudata.root {
            MENUROOT_MPSETUP => 1,
            _ => 0,
        };
        {
            let d = self.dlg(di);
            *d = MenuDialog { swipedir: d.swipedir, ..MenuDialog::default() };
            d.definition = Some(def);
            d.unk6e = unk6e;
        }
        self.dialog_init_blocks(def, di);
        self.dialog_init_items(di);
        let r = self.rng.randomfrac();
        {
            let d = self.dlg(di);
            d.ty = def.ty;
            d.transitionfrac = -1.0;
            d.redrawtimer = 0.0;
            d.unk4c = 2.0 * std::f32::consts::PI * r;
        }
        if let Some(cd) = self.mr().curdialog {
            let c = self.dlg(cd);
            c.state = MENUDIALOGSTATE_PREOPEN;
            c.statefrac = 0.0;
        }
        {
            let d = self.dlg(di);
            d.unk54 = 0;
            d.unk58 = 0;
            d.unk5c = 0;
        }
        let first = self.dialog_find_first_item(di);
        self.dlg(di).focuseditem = first;
        // Check if any items should be focused automatically
        for (i, item) in def.items.iter().enumerate() {
            if item.ty == MENUITEMTYPE_END {
                break;
            }
            if let Some(h) = item.fn_handler() {
                let mut hd = HandlerData::default();
                if h(self, MENUOP_IS_PREFOCUSED, item, &mut hd).int() != 0 {
                    self.dlg(di).focuseditem = Some(i);
                }
            }
        }
        self.run_focus_handler(di);
        {
            let d = self.dlg(di);
            d.dimmed = false;
            d.scroll = 0;
            d.dstscroll = 0;
        }
        if let Some(h) = def.handler {
            let mut hd = HandlerData::default();
            h(self, MENUOP_ON_OPEN, def, &mut hd);
        }
        self.dialog_calculate_content_size(def, di);
        self.dialog_calculate_position(di);
        let d = self.dlg(di);
        d.x = d.dstx;
        d.y = d.dsty;
        d.width = d.dstwidth;
        d.height = d.dstheight;
    }

    /// `menu_push_dialog` (menu.c:1428).
    pub fn menu_push_dialog(&mut self, def: &'static MenuDialogDef) {
        self.menu_unset_model_current();
        if self.mr().depth < 6 && self.mr().numdialogs < NUM_DIALOGS {
            let depth = self.mr().depth;
            {
                let m = self.m();
                m.depth += 1;
                m.layers[depth].numsiblings = 1;
                m.layers[depth].cursibling = 0;
            }
            let di = self.mr().numdialogs;
            {
                let m = self.m();
                m.numdialogs += 1;
                m.layers[depth].siblings[0] = di;
                m.curdialog = Some(di);
                m.dialogs[di].swipedir = 0;
            }
            self.menu_open_dialog(def, di);
            let (w, h) = (self.draw.gfx.w as i32, self.draw.gfx.h as i32);
            {
                let d = self.dlg(di);
                d.dstx = (w - d.width) / 2;
                d.dsty = (h - d.height) / 2;
            }
            self.m().inhibit_input = true;
            let mut sibling = def.nextsibling;
            while let Some(s) = sibling {
                if self.mr().layers[depth].numsiblings >= 5 || self.mr().numdialogs >= NUM_DIALOGS {
                    break;
                }
                let di2 = self.mr().numdialogs;
                {
                    let m = self.m();
                    m.numdialogs += 1;
                    let n = m.layers[depth].numsiblings as usize;
                    m.layers[depth].siblings[n] = di2;
                    m.layers[depth].numsiblings += 1;
                    m.dialogs[di2].swipedir = -1;
                }
                self.menu_open_dialog(s, di2);
                let d = self.dlg(di2);
                d.dstx = -320;
                d.x = -320;
                d.dsty = (h - d.height) / 2;
                d.y = d.dsty;
                d.ty = 0;
                sibling = s.nextsibling;
            }
            self.menu_play_sound(MENUSOUND_OPENDIALOG);
            if def.ty == MENUDIALOGTYPE_DANGER {
                self.menu_play_sound(MENUSOUND_ERROR);
            }
            if def.ty == MENUDIALOGTYPE_SUCCESS {
                self.menu_play_sound(MENUSOUND_SUCCESS);
            }
        }
    }

    /// `menu_close_dialog` (menu.c:1571).
    pub fn menu_close_dialog(&mut self) {
        let depth = self.mr().depth;
        if depth > 0 {
            let layer = self.mr().layers[depth - 1];
            for i in 0..layer.numsiblings as usize {
                let def = self.mr().dialogs[layer.siblings[i]].def();
                let mut hd = HandlerData::default();
                if let Some(h) = def.handler {
                    h(self, MENUOP_ON_CLOSE, def, &mut hd);
                }
                if hd.value == 1 {
                    return;
                }
            }
            let m = self.m();
            m.numdialogs -= layer.numsiblings as usize;
            let colstart = m.dialogs[layer.siblings[0]].colstart as usize;
            m.rowend = m.cols[colstart].rowstart as usize;
            m.colend = colstart;
            m.blockend = m.dialogs[layer.siblings[0]].blockstart as usize;
            m.depth -= 1;
            self.menu_play_sound(MENUSOUND_0B);
        }
        if self.menudata.numpendingsaves > 0 && self.mr().depth == 0 {
            let mut value = self.menudata.numpendingsaves;
            while value >= 0 {
                self.menu_save_file(value);
                value -= 1;
            }
        }
        let m = self.m();
        if m.depth == 0 {
            m.curdialog = None;
        } else {
            let layer = m.layers[m.depth - 1];
            m.curdialog = Some(layer.siblings[layer.cursibling as usize]);
        }
    }

    /// `menu_update_cur_frame` (menu.c:1625).
    pub fn menu_update_cur_frame(&mut self) {
        let depth = self.mr().depth;
        if depth == 0 {
            self.menu_close();
            self.m().curdialog = None;
        } else {
            let m = self.m();
            let layer = m.layers[depth - 1];
            m.curdialog = Some(layer.siblings[layer.cursibling as usize]);
        }
    }

    /// `menu_pop_dialog` (menu.c:1641).
    pub fn menu_pop_dialog(&mut self) {
        self.menu_close_dialog();
        self.menu_update_cur_frame();
    }

    /// `menu_replace_current_dialog` (menu.c:1647).
    pub fn menu_replace_current_dialog(&mut self, def: &'static MenuDialogDef) {
        self.menu_close_dialog();
        self.menu_push_dialog(def);
    }

    /// `menu_configure_model` (menu.c:1653).
    #[allow(clippy::too_many_arguments)]
    pub fn menu_configure_model(&mut self, x: f32, y: f32, z: f32, rotx: f32, roty: f32, rotz: f32, scale: f32, flags: u8) {
        let mm = &mut self.m().menumodel;
        mm.configuring = true;
        if flags & MENUMODELFLAG_HASPOSITION != 0 {
            mm.newposx = x;
            mm.newposy = y;
            mm.newposz = z;
        }
        if flags & MENUMODELFLAG_HASROTATION != 0 {
            mm.newrotx = rotx;
            mm.newroty = roty;
            mm.newrotz = rotz;
        }
        if flags & MENUMODELFLAG_HASSCALE != 0 {
            mm.newscale = scale;
        }
        mm.flags = flags;
        mm.configurefrac = 0.0;
    }

    /// `menu_unset_model` (menu.c:1677) on `g_Menus[g_MpPlayerNum].menumodel`.
    pub fn menu_unset_model_current(&mut self) {
        let mm = &mut self.m().menumodel;
        *mm = MenuModel { zoom: -1.0, curscale: 1.0, newscale: 1.0, headnum: -1, bodynum: -1, ..MenuModel::default() };
    }

    /// `menu_find_available_size` (menu.c:3165): left, top, right, bottom.
    pub fn menu_find_available_size(&self) -> (i32, i32, i32, i32) {
        let us = self.draw.gfx.uiscale;
        let (vw, vh) = (self.draw.gfx.w as i32, self.draw.gfx.h as i32);
        let left = 20;
        let mut top = 4;
        let right = vw / us - 20;
        let mut bottom = vh - 4;
        match self.menudata.root {
            MENUROOT_MPSETUP => {
                let playernum = self.mr().playernum;
                if self.menudata.playerjoinalpha[0] > 0 || self.menudata.playerjoinalpha[1] > 0 {
                    top += 10;
                }
                if self.menudata.playerjoinalpha[2] > 0 || self.menudata.playerjoinalpha[3] > 0 {
                    bottom -= 10;
                }
                match self.mp_num_joined {
                    2 => {
                        if playernum == 0 {
                            (left, top, (left + right) / 2, bottom)
                        } else {
                            ((left + right) / 2, top, right, bottom)
                        }
                    }
                    3 => {
                        if playernum == 0 || playernum == 1 {
                            let (l, r) = if playernum == 0 { (left, (left + right) / 2) } else { ((left + right) / 2, right) };
                            (l, top, r, (top + bottom) / 2)
                        } else {
                            (left, (top + bottom) / 2, right, bottom)
                        }
                    }
                    4 => {
                        let (l, r) = if playernum == 0 || playernum == 2 { (left, (left + right) / 2) } else { ((left + right) / 2, right) };
                        let (t, b) = if playernum == 0 || playernum == 1 { (top, (top + bottom) / 2) } else { ((top + bottom) / 2, bottom) };
                        (l, t, r, b)
                    }
                    _ => (left, top, right, bottom),
                }
            }
            MENUROOT_MPPAUSE | MENUROOT_MPENDSCREEN | MENUROOT_PICKTARGET => {
                // The player's view (menu.c:3287). With one player it is the
                // whole 320 x 220 frame.
                let playernum = self.mr().playernum;
                let [vl, vt, vw2, vh2] = self.matchview.players.get(playernum).map_or([0, 0, vw, vh], |p| p.view);
                let (mut l, t, mut r, b) = (vl / us, vt, (vl + vw2) / us, vt + vh2);
                let playercount = self.matchview.players.len();
                if playercount > 2 {
                    if playernum == 0 || playernum == 2 {
                        l += 22;
                    } else {
                        r -= 22;
                    }
                }
                if playercount == 2 && self.vars.screensplit == SCREENSPLIT_VERTICAL {
                    if playernum == 0 {
                        l += 22;
                    } else {
                        r -= 22;
                    }
                }
                (l, t, r, b)
            }
            _ => (left, top, right, bottom),
        }
    }

    /// `menu_calculate_swipe_direction` (menu.c:3089): (vdir, hdir).
    fn menu_calculate_swipe_direction(&self, arg0: i32) -> (i32, i32) {
        if self.menudata.root == MENUROOT_MPSETUP {
            let playernum = self.mr().playernum;
            let (mut vdir, mut hdir) = (0, 0);
            match self.mp_num_joined {
                1 => hdir = arg0,
                2 => {
                    if playernum == 0 {
                        if arg0 < 0 {
                            hdir = -1;
                        } else {
                            vdir = -1;
                        }
                    } else if arg0 > 0 {
                        hdir = 1;
                    } else {
                        vdir = 1;
                    }
                }
                3 => {
                    if playernum == 2 {
                        hdir = arg0;
                    } else if playernum == 0 {
                        if arg0 < 0 {
                            hdir = -1;
                        } else {
                            vdir = -1;
                        }
                    } else if arg0 > 0 {
                        hdir = 1;
                    } else {
                        vdir = -1;
                    }
                }
                4 => {
                    if playernum == 0 || playernum == 2 {
                        if arg0 < 0 {
                            hdir = -1;
                        } else {
                            vdir = if playernum == 0 { -1 } else { 1 };
                        }
                    } else if arg0 > 0 {
                        hdir = 1;
                    } else {
                        vdir = if playernum == 1 { -1 } else { 1 };
                    }
                }
                _ => {}
            }
            (vdir, hdir)
        } else {
            (0, arg0)
        }
    }

    /// `dialog_calculate_position` (menu.c:3326).
    pub fn dialog_calculate_position(&mut self, di: usize) {
        let (xmin, ymin, xmax, ymax) = self.menu_find_available_size();
        let d = self.mr().dialogs[di];
        let mut height = ymax - ymin - 6;
        let mut width = xmax - xmin - 6;
        if width > d.contentwidth {
            width = d.contentwidth;
        }
        if height > d.contentheight {
            height = d.contentheight;
        }
        let (mut dstx, mut dsty) = ((xmax + xmin - width) / 2, (ymin + ymax - height) / 2);
        if d.swipedir != 0 {
            let (vdir, hdir) = self.menu_calculate_swipe_direction(d.swipedir as i32);
            let (vw, vh) = (self.draw.gfx.w as i32 / self.draw.gfx.uiscale, self.draw.gfx.h as i32);
            if hdir < 0 {
                dstx = -4 - width;
            }
            if hdir > 0 {
                dstx = vw + 4;
            }
            if vdir < 0 {
                dsty = -4 - height;
            }
            if vdir > 0 {
                dsty = vh + 4;
            }
        }
        let d = self.dlg(di);
        d.dstx = dstx;
        d.dsty = dsty;
        d.dstwidth = width;
        d.dstheight = height;
    }

    /// `menu_close` (menu.c:3380).
    pub fn menu_close(&mut self) {
        let m = self.m();
        m.depth = 0;
        m.numdialogs = 0;
        m.rowend = 0;
        m.colend = 0;
        m.blockend = 0;
        m.curdialog = None;
        m.openinhibit = 10;
        self.menudata.count -= 1;
    }

    /// `menu_save_and_close_all` (menu.c:3405): the queued saves, then every
    /// dialog closed (unless a save's error dialog opened).
    pub fn menu_save_and_close_all(&mut self) {
        let prev = self.mr().curdialog.map(|d| (d, self.mr().dialogs[d].def() as *const MenuDialogDef));
        self.menu_save_all_pending();
        let now = self.mr().curdialog.map(|d| (d, self.mr().dialogs[d].def() as *const MenuDialogDef));
        if now == prev {
            while self.mr().depth > 0 {
                self.menu_pop_dialog();
            }
        }
    }

    /// `menu_save_and_push_root_dialog` (menu.c:3433).
    pub fn menu_save_and_push_root_dialog(&mut self, def: Option<&'static MenuDialogDef>, root: i32) {
        let prev = self.mpplayernum;
        for i in 0..4 {
            if self.menus[i].curdialog.is_some() {
                self.mpplayernum = i;
                self.menu_save_and_close_all();
            }
        }
        self.mpplayernum = prev;
        self.menudata.nextroot = root;
        self.menudata.nextdialog = def;
    }

    /// `menu_set_background` (menu.c:3451).
    pub fn menu_set_background(&mut self, bg: u8) {
        let mut screenshot = self.menudata.bg == 0;
        if self.menudata.nextbg == MENUBG_BLUR || self.menudata.nextbg == MENUBG_CONEALPHA {
            screenshot = false;
        }
        if self.menudata.bg != bg {
            self.menudata.nextbg = bg;
        }
        if screenshot && self.menudata.bg == 0 {
            self.menudata.screenshottimer = 1;
        }
    }

    /// `menu_hide_pressstart_labels` (menu.c:3472).
    pub fn menu_hide_pressstart_labels(&mut self) {
        if self.menudata.count == 0 {
            self.menudata.playerjoinalpha = [0; 4];
        }
    }

    /// `menu_push_root_dialog` (menu.c:3483).
    pub fn menu_push_root_dialog(&mut self, def: &'static MenuDialogDef, root: i32) {
        {
            let m = self.m();
            m.numdialogs = 0;
            m.depth = 0;
        }
        self.menu_remove_all_item_redraw_info();
        self.menudata.count += 1;
        if matches!(root, MENUROOT_ENDSCREEN | MENUROOT_MAINMENU | MENUROOT_FILEMGR | MENUROOT_TRAINING) {
            self.menudata.count = 1;
        }
        self.menudata.root = root;
        self.menudata.nextroot = -1;
        if matches!(root, MENUROOT_MAINMENU | MENUROOT_MPSETUP | MENUROOT_TRAINING | MENUROOT_FILEMGR) && (!self.menudata.hudpieceactive || self.menudata.hudpiece.reverseanim) {
            self.menudata.triggerhudpiece = true;
        }
        self.menu_push_dialog(def);
        match root {
            MENUROOT_MPSETUP => self.menu_set_background(MENUBG_CONEALPHA),
            MENUROOT_MAINMENU | MENUROOT_MPENDSCREEN | MENUROOT_FILEMGR | MENUROOT_TRAINING => self.menu_set_background(MENUBG_BLUR),
            _ => {}
        }
    }

    /// `menu_swipe` (menu.c:3850).
    fn menu_swipe(&mut self, direction: i32) {
        let depth = self.mr().depth;
        let layer = self.mr().layers[depth - 1];
        if layer.numsiblings < 2 {
            return;
        }
        let cd = self.mr().curdialog.unwrap();
        self.dlg(cd).swipedir = -direction as i8;
        let mut cs = layer.cursibling as i32 + direction;
        if cs < 0 {
            cs = layer.numsiblings as i32 - 1;
        }
        if cs >= layer.numsiblings as i32 {
            cs = 0;
        }
        self.m().layers[depth - 1].cursibling = cs as i8;
        let nd = layer.siblings[cs as usize];
        self.m().curdialog = Some(nd);
        let f = if direction == 1 { self.dialog_find_first_item(nd) } else { self.dialog_find_first_item_right(nd) };
        self.dlg(nd).focuseditem = f;
        let def = self.mr().dialogs[nd].def();
        for (i, item) in def.items.iter().enumerate() {
            if item.ty == MENUITEMTYPE_END {
                break;
            }
            if let Some(h) = item.fn_handler() {
                let mut hd = HandlerData::default();
                if h(self, MENUOP_IS_PREFOCUSED, item, &mut hd).int() != 0 {
                    self.dlg(nd).focuseditem = Some(i);
                }
            }
        }
        self.run_focus_handler(nd);
        self.dlg(nd).swipedir = direction as i8;
        self.dialog_calculate_position(nd);
        {
            let d = self.dlg(nd);
            d.x = d.dstx;
            d.y = d.dsty;
            d.swipedir = 0;
            d.state = MENUDIALOGSTATE_PREOPEN;
            d.statefrac = 0.0;
        }
        self.menu_unset_model_current();
        self.menu_play_sound(MENUSOUND_SWIPE);
    }

    /// `dialog_init_items` (menu.c:4468).
    fn dialog_init_items(&mut self, di: usize) {
        let d = self.mr().dialogs[di];
        let def = d.def();
        for i in 0..d.numcols as usize {
            let colindex = d.colstart as usize + i;
            for j in 0..self.mr().cols[colindex].numrows as usize {
                let rowindex = self.mr().cols[colindex].rowstart as usize + j;
                let row = self.mr().rows[rowindex];
                let item = &def.items[row.itemindex as usize];
                let data = if row.blockindex >= 0 { Some(row.blockindex as usize) } else { None };
                self.menuitem_init(item, data);
            }
        }
    }

    /// `dialog_tick` (menu.c:3914).
    fn dialog_tick(&mut self, di: usize, inputs: &mut MenuInputs, tickflags: u32) {
        self.dialog_tick_inner(di, inputs, tickflags);
        // menu.c:4459: a dialog pushed during the tick swallows this frame's input.
        if self.mr().inhibit_input {
            zero_inputs(inputs);
            self.m().inhibit_input = false;
        }
    }

    fn dialog_tick_inner(&mut self, di: usize, inputs: &mut MenuInputs, tickflags: u32) {
        let def = self.mr().dialogs[di].def();
        let mut usedefaultbehaviour = false;
        if self.mr().inhibit_input {
            zero_inputs(inputs);
        }
        self.m().inhibit_input = false;
        let spd8 = MenuInputs {
            select: 0,
            back: inputs.back,
            leftright: inputs.leftright,
            updown: inputs.updown,
            xaxis: inputs.xaxis,
            yaxis: inputs.yaxis,
            leftrightheld: inputs.leftrightheld,
            updownheld: inputs.updownheld,
            start: false,
            unk0c: inputs.unk0c,
            unk10: inputs.unk10,
            ..MenuInputs::default()
        };
        let diffframe60 = self.lv.diffframe60;
        let diffframe60f = self.lv.diffframe60f;
        let cthresh = self.menu_cthresh;
        {
            let d = self.dlg(di);
            d.unk54 += 1;
            d.unk5c += diffframe60;
            d.unk54 += d.unk5c / 9;
            d.unk5c %= 9;
            d.unk54 %= cthresh;
        }
        // For endscreens, handle transitioning of background and dialog type
        let is_cur = self.mr().curdialog == Some(di);
        let locked = self.mp_is_player_locked_out(self.mpplayernum as i32);
        {
            let d = self.dlg(di);
            if d.transitionfrac < 0.0 {
                if is_cur {
                    let mut transitiontotype = def.ty;
                    if locked && def.flags & MENUDIALOGFLAG_MPLOCKABLE != 0 {
                        transitiontotype = MENUDIALOGTYPE_DANGER;
                    }
                    if d.ty != transitiontotype {
                        d.type2 = transitiontotype;
                        d.colourweight = 0;
                        d.transitionfrac = 0.0;
                    }
                } else if d.ty != 0 {
                    d.type2 = 0;
                    d.colourweight = 0;
                    d.transitionfrac = 0.0;
                }
            } else {
                d.transitionfrac += diffframe60f * 0.042;
                if d.transitionfrac > 1.0 {
                    d.transitionfrac = -1.0;
                    d.ty = d.type2;
                }
                d.colourweight = (d.transitionfrac * 255.0) as i32 as u32;
            }
        }
        let nextbg = self.menudata.nextbg;
        let bg = self.menudata.bg;
        {
            let d = self.dlg(di);
            // The redraw loop: 2 s steady, then a redraw sweep.
            if d.state == MENUDIALOGSTATE_POPULATED && nextbg != MENUBG_CONEALPHA {
                if d.redrawtimer < 0.0 {
                    d.statefrac += diffframe60f / 120.0;
                    if d.statefrac > 1.0 {
                        d.redrawtimer = 0.0;
                    }
                } else {
                    d.statefrac = 0.0;
                }
            }
            if d.state == MENUDIALOGSTATE_POPULATING {
                d.statefrac -= 0.05 * diffframe60f;
                if d.statefrac < 0.0 {
                    d.statefrac = 0.0;
                    if d.redrawtimer < 0.0 {
                        d.state = MENUDIALOGSTATE_POPULATED;
                    }
                }
            }
            if d.state == MENUDIALOGSTATE_OPENING {
                let oldfracint = d.statefrac as i32;
                if d.statefrac != d.height as f32 {
                    for _ in 0..diffframe60 {
                        d.statefrac = d.height as f32 * 0.2 + 0.8 * d.statefrac;
                    }
                }
                if d.statefrac as i32 == oldfracint {
                    d.statefrac = oldfracint as f32 + 1.0;
                }
                if d.statefrac > d.height as f32 - 1.0 && d.statefrac < d.height as f32 + 1.0 {
                    d.state = MENUDIALOGSTATE_POPULATING;
                    d.statefrac = 1.0;
                }
            }
            if d.state == MENUDIALOGSTATE_PREOPEN {
                if std::ptr::eq(def, &gd::G_MP_READY_MENU_DIALOG) {
                    if d.statefrac < 0.1 {
                        d.statefrac += 0.04;
                    } else {
                        d.state = MENUDIALOGSTATE_OPENING;
                        d.redrawtimer = 0.0;
                        d.statefrac = 0.5;
                    }
                } else if nextbg == 255 || bg != 0 {
                    d.state = MENUDIALOGSTATE_OPENING;
                    d.redrawtimer = 0.0;
                    d.statefrac = 0.5;
                }
            }
            if d.redrawtimer >= 0.0 {
                if d.state == MENUDIALOGSTATE_POPULATED {
                    d.redrawtimer += 2.0 * diffframe60 as f32;
                } else {
                    d.redrawtimer += 5.0 * diffframe60 as f32;
                }
                if d.redrawtimer > 600.0 {
                    d.redrawtimer = -1.0;
                }
            }
        }
        if def.flags & MENUDIALOGFLAG_DISABLERESIZE == 0 {
            self.dialog_calculate_content_size(def, di);
        }
        self.dialog_calculate_position(di);
        self.dialog_tick_height(di);
        {
            let root = self.menudata.root;
            let d = self.dlg(di);
            let tween = |cur: &mut i32, dst: i32, k: f32| {
                if *cur != dst {
                    let old = *cur;
                    let mut f = *cur as f32;
                    for _ in 0..diffframe60 {
                        f = dst as f32 * k + (1.0 - k) * f;
                    }
                    *cur = f as i32;
                    if *cur != dst && *cur == old {
                        if *cur < dst {
                            *cur += 1;
                        } else {
                            *cur -= 1;
                        }
                    }
                }
            };
            if matches!(root, MENUROOT_MPPAUSE | MENUROOT_PICKTARGET | MENUROOT_MPENDSCREEN) {
                // Don't slide
                d.x = d.dstx;
                d.y = d.dsty;
            } else {
                tween(&mut d.x, d.dstx, 0.3);
                tween(&mut d.y, d.dsty, 0.3);
            }
            tween(&mut d.width, d.dstwidth, 0.3);
            tween(&mut d.height, d.dstheight, 0.3);
        }
        // Call the dialog's tick handler, if any
        if let Some(h) = def.handler {
            let mut hd = HandlerData { inputs: Some(*inputs), ..HandlerData::default() };
            h(self, MENUOP_ON_TICK, def, &mut hd);
        }
        if self.mr().numdialogs <= di || self.mr().dialogs[di].definition.map(|d| !std::ptr::eq(d, def)).unwrap_or(true) {
            // The tick handler closed this dialog.
            return;
        }
        {
            let d = self.dlg(di);
            if d.dimmed {
                d.unk10 += diffframe60 as u32;
            } else {
                d.unk10 = 0;
            }
        }
        // Tick each item in the dialog
        let d = self.mr().dialogs[di];
        'cols: for col in 0..d.numcols as usize {
            let colindex = d.colstart as usize + col;
            let numrows = self.mr().cols[colindex].numrows as usize;
            for j in 0..numrows {
                if self.mr().numdialogs <= di || self.mr().dialogs[di].definition.map(|dd| !std::ptr::eq(dd, def)).unwrap_or(true) {
                    break 'cols;
                }
                let rowindex = self.mr().cols[colindex].rowstart as usize + j;
                let row = self.mr().rows[rowindex];
                let item = &def.items[row.itemindex as usize];
                let data = if row.blockindex >= 0 { Some(row.blockindex as usize) } else { None };
                let mut local = spd8;
                let use_local = (locked && item.flags & MENUITEMFLAG_LOCKABLEMINOR != 0)
                    || (item.flags & MENUITEMFLAG_MPWEAPONSLOT != 0 && self.mp_get_weaponset_slotnum() != self.mp_get_custom_weaponset_slot());
                if self.mr().inhibit_input {
                    continue;
                }
                let focused_now = self.mr().dialogs[di].focuseditem == Some(row.itemindex as usize);
                let dimmed = self.mr().dialogs[di].dimmed;
                let inp: &mut MenuInputs = if use_local { &mut local } else { &mut *inputs };
                if tickflags & MENUTICKFLAG_DIALOGISCURRENT != 0 && focused_now {
                    let mut itemtickflags = tickflags | MENUTICKFLAG_ITEMISFOCUSED;
                    if dimmed {
                        itemtickflags |= MENUTICKFLAG_DIALOGISDIMMED;
                    }
                    usedefaultbehaviour = self.menuitem_tick(item, di, inp, itemtickflags, data);
                } else {
                    self.menuitem_tick(item, di, inp, tickflags, data);
                }
            }
        }
        if self.mr().numdialogs <= di || self.mr().dialogs[di].definition.map(|dd| !std::ptr::eq(dd, def)).unwrap_or(true) {
            return;
        }
        // If the focused item is disabled somehow, automatically jump to the next
        if let Some(fi) = self.mr().dialogs[di].focuseditem {
            if tickflags & MENUTICKFLAG_DIALOGISCURRENT != 0 && self.menu_is_item_disabled(&def.items[fi], di) {
                usedefaultbehaviour = true;
                inputs.updown = 1;
                self.dlg(di).dimmed = false;
            }
        }
        // Apply default navigational behaviour if requested
        if usedefaultbehaviour && tickflags & MENUTICKFLAG_DIALOGISCURRENT != 0 && !self.mr().dialogs[di].dimmed {
            let depth = self.mr().depth;
            let layer = self.mr().layers[depth - 1];
            let prev = self.mr().dialogs[di].focuseditem;
            if layer.numsiblings <= 1 {
                self.dialog_change_item_focus(di, inputs.leftright as i32, inputs.updown as i32);
                if self.mr().dialogs[di].focuseditem != prev {
                    self.menu_play_sound(MENUSOUND_FOCUS);
                }
            } else {
                let swipedir = self.dialog_change_item_focus(di, inputs.leftright as i32, inputs.updown as i32);
                if swipedir != 0 {
                    self.menu_swipe(swipedir);
                } else if prev != self.mr().dialogs[di].focuseditem {
                    self.menu_play_sound(MENUSOUND_FOCUS);
                }
            }
            let state = self.mr().dialogs.get(di).map(|d| d.state).unwrap_or(0);
            if inputs.back != 0 {
                if def.flags & MENUDIALOGFLAG_DROPOUTONCLOSE != 0 && self.vars.unk000498 != 0 {
                    self.menu_push_dialog(&gd::G_MP_DROP_OUT_MENU_DIALOG);
                } else if def.flags & MENUDIALOGFLAG_IGNOREBACK == 0 {
                    self.menu_pop_dialog();
                }
            } else if def.flags & MENUDIALOGFLAG_CLOSEONSELECT != 0 && state > MENUDIALOGSTATE_PREOPEN && (inputs.select & 1 == 1 || inputs.back & 1 == 1) {
                self.menu_pop_dialog();
            }
        }
        if self.mr().numdialogs <= di || self.mr().dialogs[di].definition.map(|dd| !std::ptr::eq(dd, def)).unwrap_or(true) {
            return;
        }
        // Scrolling related (when the dialog is too big vertically)
        let d = self.mr().dialogs[di];
        if let (Some(fi), true) = (d.focuseditem, def.flags & MENUDIALOGFLAG_DISABLEITEMSCROLL == 0) {
            let (y, rowindex, _) = self.dialog_find_item(di, Some(fi));
            if def.items[fi].flags & MENUITEMFLAG_DISABLESCROLL == 0 {
                let itemy = y + self.mr().rows[rowindex].height as i32 / 2;
                let mut dstscroll = (d.height - LINEHEIGHT - 1) / 2 - itemy;
                if dstscroll > 0 {
                    dstscroll = 0;
                }
                if dstscroll < d.height - d.contentheight {
                    dstscroll = d.height - d.contentheight;
                }
                self.dlg(di).dstscroll = dstscroll;
            } else {
                self.dlg(di).dstscroll = 0;
            }
        } else if def.flags & MENUDIALOGFLAG_SMOOTHSCROLLABLE != 0 {
            let adjustment = inputs.yaxis as i32 * diffframe60 / 20 - inputs.updownheld as i32 * diffframe60;
            let dd = self.dlg(di);
            dd.dstscroll += adjustment;
            if dd.dstscroll > 0 {
                dd.dstscroll = 0;
            }
            if dd.dstscroll < dd.height - dd.contentheight {
                dd.dstscroll = dd.height - dd.contentheight;
            }
            dd.scroll = dd.dstscroll;
        }
        let dd = self.dlg(di);
        if dd.scroll != dd.dstscroll {
            let old = dd.scroll;
            let mut f = dd.scroll as f32;
            for _ in 0..diffframe60 {
                f = dd.dstscroll as f32 * 0.2 + 0.8 * f;
            }
            dd.scroll = f as i32;
            if dd.scroll != dd.dstscroll && dd.scroll == old {
                if dd.scroll < dd.dstscroll {
                    dd.scroll += 1;
                } else {
                    dd.scroll -= 1;
                }
            }
        }
    }

    /// `menu_process_input` (menu.c:4504) for the current `g_MpPlayerNum`.
    /// PD reads `menu_get_cont_pads`: in MP setup, player N's menu is pad N.
    pub fn menu_process_input(&mut self) {
        self.menu_increment_item_redraw_timers();
        let Some(_) = self.mr().curdialog else { return };
        let pad = self.mpplayernum;
        let joy = self.pads[pad];
        let buttons = joy.buttons;
        let buttonsnow = joy.buttons & !joy.prev;
        let mut inputs = MenuInputs::default();
        let (stickx, sticky) = (joy.stick_x as i32, joy.stick_y as i32);
        let mut starttap = false;
        let (mut yhelddir, mut xhelddir, mut ytapdir, mut xtapdir) = (0i32, 0i32, 0i32, 0i32);
        if buttonsnow & A_BUTTON != 0 {
            inputs.select = 1;
        }
        if buttonsnow & B_BUTTON != 0 {
            inputs.back = 1;
        }
        if buttonsnow & Z_TRIG != 0 {
            inputs.select = 1;
        }
        if buttonsnow & START_BUTTON != 0 {
            starttap = true;
        }
        if buttons & (R_TRIG | L_TRIG) != 0 {
            inputs.shoulder = 1;
        }
        for (held, tap, dir, is_y) in [
            (U_CBUTTONS, U_CBUTTONS, -1, true),
            (D_CBUTTONS, D_CBUTTONS, 1, true),
            (L_CBUTTONS, L_CBUTTONS, -1, false),
            (R_CBUTTONS, R_CBUTTONS, 1, false),
            (U_JPAD, U_JPAD, -1, true),
            (D_JPAD, D_JPAD, 1, true),
            (L_JPAD, L_JPAD, -1, false),
            (R_JPAD, R_JPAD, 1, false),
        ] {
            if is_y {
                if buttons & held != 0 {
                    yhelddir = dir;
                }
                if buttonsnow & tap != 0 {
                    ytapdir = dir;
                }
            } else {
                if buttons & held != 0 {
                    xhelddir = dir;
                }
                if buttonsnow & tap != 0 {
                    xtapdir = dir;
                }
            }
        }
        if inputs.select != 0 {
            inputs.back = 0;
        }
        if ytapdir != 0 {
            yhelddir = ytapdir;
        }
        if xtapdir != 0 {
            xhelddir = xtapdir;
        }
        // Choose repeat rate settings
        let mut digitalrepeatinterval = 10;
        let mut xdeadzone = 20;
        xdeadzone += 10;
        let ydeadzone = 20;
        let mut stickintervalbase = 60;
        let mut xstickintervalmult = 33;
        let mut ystickintervalmult = 44;
        let mut allowdiagonal = false;
        if let Some(cd) = self.mr().curdialog {
            let d = self.mr().dialogs[cd];
            if let Some(fi) = d.focuseditem {
                let item = &d.def().items[fi];
                if (item.ty == MENUITEMTYPE_SLIDER || item.ty == MENUITEMTYPE_10) && d.dimmed {
                    digitalrepeatinterval = 5;
                    xdeadzone = 20;
                    stickintervalbase = 30;
                    xstickintervalmult = 10;
                }
                if item.ty == MENUITEMTYPE_KEYBOARD {
                    allowdiagonal = true;
                    digitalrepeatinterval = 5;
                    xdeadzone = 20;
                    xstickintervalmult = 10;
                    ystickintervalmult = 10;
                }
            }
        }
        let diffframe60 = self.lv.diffframe60;
        // Left/right repeat
        {
            let m = self.m();
            let mut apply = false;
            if xhelddir == 0 {
                m.xrepeatmode = MENUREPEATMODE_RELEASED;
            }
            if xtapdir != 0 {
                m.xrepeatmode = MENUREPEATMODE_SLOW;
                m.xrepeattimer60 = 0;
                m.xrepeatdir = xtapdir as i16;
                apply = true;
            } else if xhelddir != 0 {
                xhelddir = m.xrepeatdir as i32;
            }
            if m.xrepeattimer60 > 60 {
                m.xrepeatmode = MENUREPEATMODE_FAST;
            }
            let mut oldslot = m.xrepeattimer60 / digitalrepeatinterval;
            let mut newslot = (m.xrepeattimer60 + diffframe60) / digitalrepeatinterval;
            if m.xrepeatmode == MENUREPEATMODE_SLOW {
                oldslot /= 2;
                newslot /= 2;
            }
            inputs.leftrightheld = xhelddir as i8;
            let mut absstickx = stickx.abs();
            let abssticky = sticky.abs();
            if absstickx >= xdeadzone && (absstickx > abssticky || allowdiagonal) {
                if stickx < 0 && m.xrepeatcount > 0 {
                    m.xrepeatcount = 0;
                }
                if stickx > 0 && m.xrepeatcount < 0 {
                    m.xrepeatcount = 0;
                }
                if m.xrepeatcount == 0 {
                    m.xrepeattimer60 = 0;
                }
                if absstickx > 70 {
                    absstickx = 70;
                }
                absstickx -= xdeadzone;
                let mut interval = stickintervalbase - xstickintervalmult * absstickx / (70 - xdeadzone);
                if m.xrepeatcount >= 3 || m.xrepeatcount <= -3 {
                    interval /= 2;
                }
                if interval > 0 {
                    oldslot = m.xrepeattimer60 / interval;
                    newslot = (m.xrepeattimer60 + diffframe60) / interval;
                    xhelddir = if stickx < 0 { -1 } else { 1 };
                    if oldslot != newslot {
                        apply = true;
                    }
                    if m.xrepeatcount == 0 {
                        apply = true;
                    }
                    if apply {
                        m.xrepeatcount += xhelddir as i16;
                    }
                }
            } else {
                m.xrepeatcount = 0;
            }
            if oldslot != newslot {
                apply = true;
            }
            if !apply {
                xhelddir = 0;
            }
        }
        // Up/down repeat
        {
            let m = self.m();
            let mut apply = false;
            if ytapdir != 0 {
                apply = true;
                m.yrepeatmode = MENUREPEATMODE_SLOW;
                m.yrepeattimer60 = 0;
                m.yrepeatdir = ytapdir as i16;
            } else if yhelddir != 0 {
                yhelddir = m.yrepeatdir as i32;
            }
            if m.yrepeattimer60 > 60 {
                m.yrepeatmode = MENUREPEATMODE_FAST;
            }
            let mut oldslot = m.yrepeattimer60 / digitalrepeatinterval;
            let mut newslot = (m.yrepeattimer60 + diffframe60) / digitalrepeatinterval;
            if m.yrepeatmode == MENUREPEATMODE_SLOW {
                oldslot /= 2;
                newslot /= 2;
            }
            inputs.updownheld = yhelddir as i8;
            let mut abssticky = sticky.abs();
            let absstickx = stickx.abs();
            if abssticky >= ydeadzone && (abssticky > absstickx || allowdiagonal) {
                if sticky < 0 && m.yrepeatcount < 0 {
                    m.yrepeatcount = 0;
                }
                if sticky > 0 && m.yrepeatcount > 0 {
                    m.yrepeatcount = 0;
                }
                if m.yrepeatcount == 0 {
                    m.yrepeattimer60 = 0;
                }
                if abssticky > 70 {
                    abssticky = 70;
                }
                abssticky -= ydeadzone;
                let mut interval = stickintervalbase - ystickintervalmult * abssticky / 50;
                if m.yrepeatcount >= 3 || m.yrepeatcount <= -3 {
                    interval /= 3;
                }
                if interval > 0 {
                    oldslot = m.yrepeattimer60 / interval;
                    newslot = (m.yrepeattimer60 + diffframe60) / interval;
                    yhelddir = if sticky > 0 { -1 } else { 1 };
                    if oldslot != newslot {
                        apply = true;
                    }
                    if m.yrepeatcount == 0 {
                        apply = true;
                    }
                    if apply {
                        m.yrepeatcount += yhelddir as i16;
                    }
                }
            } else {
                m.yrepeatcount = 0;
            }
            if oldslot != newslot {
                apply = true;
            }
            if !apply {
                yhelddir = 0;
            }
            m.xrepeattimer60 += diffframe60;
            m.yrepeattimer60 += diffframe60;
        }
        inputs.leftright = xhelddir as i8;
        inputs.updown = yhelddir as i8;
        inputs.xaxis = stickx as i8;
        inputs.yaxis = sticky as i8;
        inputs.unk14 = 0;
        inputs.start = starttap;
        // Keyboard delete: PD's `back2` is fed from the B button inside the
        // keyboard item (menuitem_keyboard_tick reads it); here Backspace.
        inputs.back2 = self.back2[pad] as u8;
        let mut starttoselect = false;
        if let Some(cd) = self.mr().curdialog {
            if starttap {
                let d = self.mr().dialogs[cd];
                if d.def().flags & MENUDIALOGFLAG_STARTSELECTS != 0 {
                    inputs.select = 1;
                    starttoselect = true;
                }
                if let Some(fi) = d.focuseditem {
                    if d.def().items[fi].ty == MENUITEMTYPE_LIST {
                        inputs.select = 1;
                    }
                }
            }
        }
        // Iterate all dialogs and give them the input for processing
        let mut foundcurrent = false;
        let mut i = 0;
        while i < self.mr().depth {
            let layer = self.mr().layers[i];
            for j in 0..layer.numsiblings as usize {
                let mut tickflags = 0;
                if i == self.mr().depth.saturating_sub(1) && j == layer.cursibling as usize && !foundcurrent {
                    tickflags |= MENUTICKFLAG_DIALOGISCURRENT;
                    foundcurrent = true;
                }
                if i >= self.mr().depth {
                    break;
                }
                let di = self.mr().layers[i].siblings[j];
                self.dialog_tick(di, &mut inputs, tickflags);
            }
            i += 1;
        }
        // MP setup: START jumps to the Ready dialog, or applies quick start.
        if self.menudata.root == MENUROOT_MPSETUP && inputs.start && !starttoselect {
            if let Some(cd) = self.mr().curdialog {
                let d = self.mr().dialogs[cd];
                if !d.dimmed {
                    if self.vars.mpsetupmenu != gd::MPSETUPMENU_GENERAL && !std::ptr::eq(d.def(), &gd::G_MP_READY_MENU_DIALOG) {
                        self.menu_push_dialog(&gd::G_MP_READY_MENU_DIALOG);
                    } else if std::ptr::eq(d.def(), &gd::G_MP_QUICK_TEAM_GAME_SETUP_MENU_DIALOG) {
                        self.mp_apply_quickstart();
                    }
                }
            }
        }
        if self.menudata.root == MENUROOT_MPPAUSE {
            // (No cutscenes in a match.)
            self.m().openinhibit = 10;
        }
        if matches!(self.menudata.root, MENUROOT_ENDSCREEN | MENUROOT_MAINMENU | MENUROOT_MPPAUSE | MENUROOT_MPENDSCREEN | MENUROOT_TRAINING) && inputs.start && !starttoselect {
            if let Some(cd) = self.mr().curdialog {
                if self.mr().dialogs[cd].def().flags & MENUDIALOGFLAG_IGNOREBACK == 0 {
                    self.menu_save_and_close_all();
                }
            }
        }
    }

    // ---- rendering ----

    /// `menu_get_team_titlebar_colours` (menu.c:2362).
    fn menu_get_team_titlebar_colours(&self, top: &mut u32, middle: &mut u32, bottom: &mut u32) {
        const COLOURS: [[u32; 3]; 8] = [
            [0xbf000000, 0x50000000, 0xff000000],
            [0xbfbf0000, 0x50500000, 0xffff0000],
            [0x0000bf00, 0x00005000, 0x0000ff00],
            [0xbf00bf00, 0x50005000, 0xff00ff00],
            [0x00bfbf00, 0x00505000, 0x00ffff00],
            [0xff885500, 0x7f482000, 0xff885500],
            [0xff888800, 0x7f484800, 0xff888800],
            [0x88445500, 0x48242000, 0x88445500],
        ];
        let team = self.mp.players[self.mpplayernum].base.team as usize & 7;
        *top = COLOURS[team][0] | (*top & 0xff);
        *middle = COLOURS[team][1] | (*middle & 0xff);
        *bottom = COLOURS[team][2] | (*bottom & 0xff);
    }

    /// `menu_apply_scissor` (menu.c:2381).
    pub fn menu_apply_scissor(&mut self) {
        let us = self.draw.gfx.uiscale;
        let s = self.scissor_menu;
        self.draw.gfx.set_scissor(s[0] * us, s[1], s[2] * us, s[3]);
    }

    /// `dialog_render` (menu.c:2448).
    fn dialog_render(&mut self, di: usize, lightweight: bool) {
        let d = self.mr().dialogs[di];
        let def = d.def();
        let (mut bgx1, mut bgy1, mut bgx2, mut bgy2) = (d.x, d.y, d.x + d.width, d.y + d.height);
        let _ = (&mut bgx1, &mut bgx2);
        self.draw.text.shadow_colour = mixcolour(&d, Pal::ItemFocusedOuter);
        self.draw.text.holoray_enabled = false;
        let dialogwidth = d.width;
        let mut dialogheight = d.height;
        if d.state == MENUDIALOGSTATE_PREOPEN {
            if std::ptr::eq(def, &gd::G_MP_READY_MENU_DIALOG) {
                return;
            }
            let mut sp170 = 1.0 - self.menudata.bgopacityfrac;
            sp170 = 1.0 - sp170 * sp170;
            dialogheight = (dialogheight as f32 * sp170) as i32;
            bgy2 = d.y + dialogheight;
        }
        let dialogleft = d.x;
        let dialogtop = d.y;
        let dialogright = dialogleft + dialogwidth;
        let mut dialogbottom = dialogtop + dialogheight;
        let title = self.menu_resolve_dialog_title(def);
        let mut colour1 = mixcolour(&d, Pal::DialogBorder1);
        let mut colour2 = mixcolour(&d, Pal::DialogTitlebg);
        let mut colour3 = mixcolour(&d, Pal::DialogBorder2);
        let mut colour4 = colour1;
        let mut colour5 = colour3;
        if colour4 & 0xff > 0x3f {
            colour4 = (colour4 & 0xffffff00) | 0x3f;
        }
        if colour5 & 0xff > 0x3f {
            colour5 = (colour5 & 0xffffff00) | 0x3f;
        }
        self.draw.text.holoray_miny = -1000;
        self.draw.text.holoray_maxy = 1000;
        if def.flags & MENUDIALOGFLAG_DISABLETITLEBAR != 0 {
            bgy1 += LINEHEIGHT;
        }
        // The walls/floor/ceiling coming from the projection source (not in MP setup).
        if self.menudata.root != MENUROOT_MPSETUP && (self.menudata.root != MENUROOT_MPPAUSE || self.in_match) {
            let t = &mut self.draw.text;
            t.text_holoray(bgx1, bgy1, bgx2, bgy1, colour4, colour5, MENUPLANE_00);
            t.text_holoray(bgx2, bgy1, bgx2, bgy2, colour5, colour4, MENUPLANE_00);
            t.text_holoray(bgx2, bgy2, bgx1, bgy2, colour4, colour5, MENUPLANE_00);
            t.text_holoray(bgx1, bgy2, bgx1, bgy1, colour5, colour4, MENUPLANE_00);
            t.text_holoray(bgx1, bgy1, bgx2, bgy1, colour5, colour4, MENUPLANE_01);
            t.text_holoray(bgx2, bgy1, bgx2, bgy2, colour4, colour5, MENUPLANE_01);
            t.text_holoray(bgx2, bgy2, bgx1, bgy2, colour5, colour4, MENUPLANE_01);
            t.text_holoray(bgx1, bgy2, bgx1, bgy1, colour4, colour5, MENUPLANE_01);
        }
        // Title bar
        if def.flags & MENUDIALOGFLAG_DISABLETITLEBAR == 0 {
            if self.menudata.root == MENUROOT_MPSETUP && self.mp.setup.options & gd::MPOPTION_TEAMSENABLED as u32 != 0 && self.vars.mpsetupmenu != gd::MPSETUPMENU_GENERAL {
                self.menu_get_team_titlebar_colours(&mut colour1, &mut colour2, &mut colour3);
            }
            self.menugfx_render_gradient(dialogleft - 2, dialogtop, dialogright + 2, dialogtop + LINEHEIGHT, colour1, colour2, colour3);
            self.menugfx_draw_shimmer(dialogleft - 2, dialogtop, dialogright + 2, dialogtop + 1, (colour1 & 0xff) >> 1, true, 40, false);
            self.menugfx_draw_shimmer(dialogleft - 2, dialogtop + 10, dialogright + 2, dialogtop + LINEHEIGHT, (colour1 & 0xff) >> 1, false, 40, true);
            let c = mixcolour(&d, Pal::DialogTitlefg);
            self.draw.text.set_wave_colours(wave2(d.ty, Pal::DialogTitlefg), wave1(d.ty, Pal::DialogTitlefg));
            let vh = self.draw.gfx.h as i32;
            let (mut x, mut y) = (dialogleft + 3, dialogtop + 3);
            self.tc().render_v2(&mut x, &mut y, &title, FontId::Sm, c & 0xff, dialogwidth, vh, 0, 0);
            let (mut x, mut y) = (dialogleft + 2, dialogtop + 2);
            self.tc().render_v2(&mut x, &mut y, &title, FontId::Sm, c, dialogwidth, vh, 0, 0);
            if matches!(self.menudata.root, MENUROOT_MPSETUP | MENUROOT_MPPAUSE | MENUROOT_MPENDSCREEN) {
                let (mut x, mut y) = (dialogright - 9, dialogtop + 2);
                let num = ["1\n", "2\n", "3\n", "4\n"][self.mpplayernum];
                self.tc().render_v2(&mut x, &mut y, num, FontId::Sm, c, dialogwidth, vh, 0, 0);
            }
        }
        // Configure things for the redraw effect
        if d.redrawtimer >= 0.0 {
            if self.menudata.root != MENUROOT_MPPAUSE {
                if d.state >= MENUDIALOGSTATE_POPULATED {
                    self.draw.text.set_diagonal_blend(d.x, d.y, d.redrawtimer, DIAGMODE_REDRAW);
                } else {
                    self.draw.text.set_diagonal_blend(d.x, d.y, d.redrawtimer, DIAGMODE_FADEIN);
                }
                self.draw.text.holoray_enabled = true;
            }
        } else if d.state == MENUDIALOGSTATE_POPULATED {
            self.draw.text.set_menu_blend(d.statefrac);
        }
        if dialogbottom < dialogtop + LINEHEIGHT {
            dialogbottom = dialogtop + LINEHEIGHT;
        }
        let mut bodybg = mixcolour(&d, Pal::DialogBodybg);
        if d.dimmed {
            bodybg = (colour_blend(bodybg, 0, 44) & 0xffffff00) | (bodybg & 0xff);
        }
        let c2 = mixcolour(&d, Pal::Unused14);
        if !lightweight {
            let frac = match d.state {
                MENUDIALOGSTATE_OPENING => 1.0,
                MENUDIALOGSTATE_POPULATING => d.statefrac,
                _ => -1.0,
            };
            self.menugfx_render_dialog_background(dialogleft + 1, dialogtop + LINEHEIGHT, dialogright - 1, dialogbottom, &d, bodybg, c2, frac);
        }
        if d.state == MENUDIALOGSTATE_PREOPEN {
            return;
        }
        let us = self.draw.gfx.uiscale;
        let (viewleft, viewtop, viewright, viewbottom) = (0, 0, self.draw.gfx.w as i32 / us, self.draw.gfx.h as i32);
        let mut s = [dialogleft + 2, dialogtop + LINEHEIGHT, dialogright - 2, dialogbottom - 1];
        if s[2] < viewleft || s[3] < viewtop || s[0] > viewright || s[1] > viewbottom {
            return;
        }
        if s[2] > viewright {
            s[2] = viewright;
        }
        if s[3] > viewbottom {
            s[3] = viewbottom;
        }
        if s[0] < viewleft {
            s[0] = viewleft;
        }
        self.scissor_menu = s;
        self.draw.text.holoray_miny = s[1];
        self.draw.text.holoray_maxy = s[3];
        self.menu_apply_scissor();
        // Render models (character select)
        if self.mr().curdialog == Some(di) && def.flags & MENUDIALOGFLAG_ALLOW_MODELS != 0 && !lightweight && !self.mr().menumodel.drawbehinddialog {
            self.menu_render_model_current();
        }
        // Render menu items
        let d = self.mr().dialogs[di];
        if d.ty != 0 || d.transitionfrac >= 0.0 {
            let mut sumwidth = 0;
            let mut curx = dialogleft;
            for i in 0..d.numcols as usize {
                let mut cury = dialogtop + LINEHEIGHT + 1 + d.scroll;
                let mut prevwaslist = false;
                let sp120 = (mixcolour(&d, Pal::ItemUnfocused) & 0xffffff00) | 0x3f;
                let colindex = d.colstart as usize + i;
                if i != 0 && def.flags & MENUDIALOGFLAG_NOVERTICALBORDERS == 0 {
                    self.menugfx_draw_filled_rect(curx - 1, dialogtop + LINEHEIGHT + 1, curx, dialogbottom, sp120, sp120);
                }
                let mut colwidth = self.mr().cols[colindex].width as i32;
                sumwidth += colwidth;
                if i == d.numcols as usize - 1 {
                    let v0 = (dialogright - dialogleft) - 2;
                    if sumwidth < v0 {
                        colwidth = colwidth + v0 - sumwidth;
                    }
                }
                for j in 0..self.mr().cols[colindex].numrows as usize {
                    let rowindex = self.mr().cols[colindex].rowstart as usize + j;
                    let row = self.mr().rows[rowindex];
                    let item = &def.items[row.itemindex as usize];
                    let d = self.mr().dialogs[di];
                    let mut focused = 0;
                    if d.focuseditem == Some(row.itemindex as usize) {
                        focused = if d.dimmed { 3 } else { 1 };
                    }
                    let data = if row.blockindex >= 0 { Some(row.blockindex as usize) } else { None };
                    let ctx = Ctx { x: curx, y: cury, width: colwidth, height: row.height as i32, item, focused, dialog: di, data, unk18: lightweight };
                    let offscreen = ctx.y + ctx.height < dialogtop + LINEHEIGHT + 1 || ctx.y > dialogbottom || ctx.height == 0;
                    if !offscreen {
                        if prevwaslist {
                            self.menugfx_draw_filled_rect(ctx.x, ctx.y - 1, ctx.x + ctx.width, ctx.y, sp120, sp120);
                            prevwaslist = false;
                        }
                        if item.flags & MENUITEMFLAG_DARKERBG != 0 && !lightweight {
                            let c2 = mixcolour(&d, Pal::ItemFocusedOuter);
                            let colour = colour_blend(c2, c2 & 0xffffff00, 127);
                            self.draw.gfx.fill_rect_scaled(ctx.x, ctx.y, ctx.x + ctx.width, ctx.y + ctx.height, colour);
                        }
                        if focused != 0 {
                            if matches!(item.ty, MENUITEMTYPE_03 | MENUITEMTYPE_SELECTABLE | MENUITEMTYPE_CHECKBOX | MENUITEMTYPE_0A | MENUITEMTYPE_SLIDER | MENUITEMTYPE_DROPDOWN)
                                && !(d.transitionfrac >= 0.0 && d.type2 == 0)
                                && !(d.transitionfrac < 0.0 && d.ty == 0)
                            {
                                self.draw.text.shadow_enabled = true;
                            }
                            // The horizontal line behind the focused item
                            if matches!(item.ty, MENUITEMTYPE_SELECTABLE | MENUITEMTYPE_CHECKBOX | MENUITEMTYPE_0A | MENUITEMTYPE_DROPDOWN) {
                                let liney = ctx.y + ctx.height / 2 - 1;
                                let x1 = ctx.x;
                                let x3 = ctx.x + 8;
                                let x4 = ctx.x + ctx.width / 3;
                                let colour = (sp120 & 0xffffff00) | 0x2f;
                                self.menugfx_draw_tri2(x1, liney - 1, x3 - 3, liney, sp120, sp120, false);
                                self.menugfx_draw_tri2(x3 - 3, liney - 1, x3, liney, sp120, 0xffffffff, false);
                                self.menugfx_draw_tri2(x1, liney + 1, x3 - 3, liney + 2, sp120, sp120, false);
                                self.menugfx_draw_tri2(x3 - 3, liney + 1, x3, liney + 2, sp120, 0xffffffff, false);
                                self.menugfx_draw_tri2(x3 - 2, liney, x4, liney + 1, colour, sp120 & 0xffffff00, false);
                                if item.flags & MENUITEMFLAG_SELECTABLE_CENTRE != 0 {
                                    let x1 = ctx.x + ctx.width;
                                    let x3 = ctx.x + ctx.width - 8;
                                    let x4 = ctx.x + ctx.width - ctx.width / 3;
                                    self.menugfx_draw_tri2(x1 - 5, liney - 1, x1, liney, sp120, sp120, false);
                                    self.menugfx_draw_tri2(x3, liney - 1, x3 + 3, liney, 0xffffffff, sp120, false);
                                    self.menugfx_draw_tri2(x3 + 3, liney + 1, x1, liney + 2, sp120, sp120, false);
                                    self.menugfx_draw_tri2(x3, liney + 1, x3 + 3, liney + 2, 0xffffffff, sp120, false);
                                    self.menugfx_draw_tri2(x4, liney, x3 + 2, liney + 1, sp120 & 0xffffff00, colour, false);
                                }
                            }
                        }
                        self.menuitem_render(&ctx);
                        if item.ty == MENUITEMTYPE_LIST {
                            prevwaslist = true;
                        }
                        if focused != 0 {
                            self.draw.text.shadow_enabled = false;
                        }
                    }
                    cury += row.height as i32;
                }
                curx += self.mr().cols[colindex].width as i32;
            }
            // Overlays, such as dropdown menus
            if !lightweight {
                let mut curx = dialogleft;
                for i in 0..d.numcols as usize {
                    let mut cury = dialogtop + LINEHEIGHT + 1 + d.scroll;
                    let colindex = d.colstart as usize + i;
                    for j in 0..self.mr().cols[colindex].numrows as usize {
                        let rowindex = self.mr().cols[colindex].rowstart as usize + j;
                        let row = self.mr().rows[rowindex];
                        let item = &def.items[row.itemindex as usize];
                        let data = if row.blockindex >= 0 { Some(row.blockindex as usize) } else { None };
                        let cw = self.mr().cols[colindex].width as i32;
                        self.menuitem_overlay(curx, cury, cw, row.height as i32, item, di, data);
                        cury += row.height as i32;
                    }
                    curx += self.mr().cols[colindex].width as i32;
                }
            }
        }
        self.draw.gfx.full_scissor();
        // Left/right chevrons (and the sibling titles outside MP setup)
        let depth = self.mr().depth;
        let layer = self.mr().layers[depth - 1];
        let d = self.mr().dialogs[di];
        if (d.ty != 0 || d.transitionfrac >= 0.0) && layer.siblings[layer.cursibling as usize] == di && layer.numsiblings >= 2 {
            let weight = (sin_osc(self.frac20, 10.0) * 255.0) as u32;
            let c1 = mixcolour(&d, Pal::DialogBorder1);
            let colour = colour_blend(0xffffffff, c1, weight);
            let f = sin_osc(self.frac20, 20.0);
            self.menugfx_draw_dialog_chevron(dialogleft - 5, (dialogtop + dialogbottom) / 2, 9, 1, colour, colour, f);
            self.menugfx_draw_dialog_chevron(dialogright + 5, (dialogtop + dialogbottom) / 2, 9, 3, colour, colour, f);
            if self.menudata.root == MENUROOT_MAINMENU {
                self.draw.text.reset_blends();
                self.draw.text.rotated90 = true;
                let vh = self.draw.gfx.h as i32;
                let mut previndex = layer.cursibling as i32 - 1;
                if previndex < 0 {
                    previndex = layer.numsiblings as i32 - 1;
                }
                let pdef = self.mr().dialogs[layer.siblings[previndex as usize]].def();
                let title = self.menu_resolve_dialog_title(pdef);
                let tw = self.measure(&title, FontId::Xs).1;
                let mut x = dialogleft - 1;
                let mut y = (dialogtop + dialogbottom) / 2 - tw - 3;
                if y < dialogtop {
                    y = (dialogtop + dialogbottom - tw) / 2;
                    x -= 3;
                }
                self.tc().render_v2(&mut y, &mut x, &title, FontId::Xs, 0xffffffff, dialogwidth, vh, 0, 0);
                let mut nextindex = layer.cursibling as i32 + 1;
                if nextindex >= layer.numsiblings as i32 {
                    nextindex = 0;
                }
                let ndef = self.mr().dialogs[layer.siblings[nextindex as usize]].def();
                let title = self.menu_resolve_dialog_title(ndef);
                let tw = self.measure(&title, FontId::Xs).1;
                let mut x = dialogright + 7;
                let mut y = (dialogtop + dialogbottom) / 2 + 3;
                if y + tw > dialogbottom {
                    y = (dialogtop + dialogbottom - tw) / 2;
                    x += 3;
                }
                self.tc().render_v2(&mut y, &mut x, &title, FontId::Xs, 0xffffffff, dialogwidth, vh, 0, 0);
                self.draw.text.rotated90 = false;
            }
        }
    }

    /// `menu_render_dialog` (menu.c:3583).
    fn menu_render_dialog(&mut self, di: usize) {
        let d = self.mr().dialogs[di];
        let cthresh = self.menu_cthresh;
        self.draw.text.set_wave_blend(d.unk54, d.unk58 as i32, cthresh);
        self.dialog_render(di, false);
        self.draw.text.reset_blends();
    }

    /// `menu_render_dialogs` (menu.c:3609, NTSC 1.0+): one "other" on-screen
    /// dialog, then the current one on top.
    fn menu_render_dialogs(&mut self) {
        let Some(cd) = self.mr().curdialog else { return };
        if matches!(self.menudata.root, MENUROOT_MPPAUSE | MENUROOT_PICKTARGET | MENUROOT_MPENDSCREEN) {
            let d = self.mr().dialogs[cd];
            let (vw, vh) = (self.draw.gfx.w as i32 / self.draw.gfx.uiscale, self.draw.gfx.h as i32);
            self.draw.text.holoray_fromx = d.x + d.width / 2 - vw / 2;
            self.draw.text.holoray_fromy = d.y + d.height / 2 - vh / 2;
            self.menu_render_dialog(cd);
            return;
        }
        let mut other = None;
        for i in 0..self.mr().depth {
            let layer = self.mr().layers[i];
            for j in 0..layer.numsiblings as usize {
                let s = layer.siblings[j];
                if s != cd {
                    let sd = self.mr().dialogs[s];
                    if sd.ty != 0 || sd.transitionfrac >= 0.0 {
                        other = Some(s);
                    }
                }
            }
        }
        if let Some(o) = other {
            self.menu_render_dialog(o);
        }
        self.menu_render_dialog(cd);
    }

    /// `menu_render_background_layer1` (menu.c:5050).
    fn menu_render_background_layer1(&mut self, bg: u8, frac: f32) {
        let (w, h) = (self.draw.gfx.w as i32, self.draw.gfx.h as i32);
        match bg {
            MENUBG_BLUR => {
                let alpha = (255.0 * frac) as u32;
                self.menugfx_render_bg_blur(0xffffff00 | alpha, 0, 0);
                self.menugfx_render_bg_blur(0xffffff00 | (alpha >> 1), -30, -30);
                self.menugfx_render_bg_blur(0xffffff00 | (alpha >> 1), 30, 30);
            }
            MENUBG_BLACK | MENUBG_8 => {
                let colour = (255.0 * frac) as u32;
                self.draw.gfx.fill_rect(0, 0, w, h, colour);
            }
            MENUBG_CONEALPHA => {
                if self.menudata.screenshottimer != 0 {
                    return;
                }
                self.menugfx_render_bg_blur(0xffffffff, 0, 0);
                if frac < 1.0 {
                    let alpha = ((1.0 - frac) * 255.0) as u32;
                    self.draw.gfx.fill_rect(0, 0, w, h, 0xff000000 | alpha);
                }
            }
            MENUBG_GRADIENT => {
                self.menugfx_render_gradient(0, 0, w, h, 0x00007f7f, 0x000000ff, 0x8f0000ff);
            }
            MENUBG_CONEOPAQUE => {
                self.menugfx_render_gradient(0, 0, w, h, 0x3f3f00ff, 0x7f0000ff, 0x3f3f00ff);
            }
            _ => {}
        }
    }

    /// `menu_render_background_layer2` (menu.c:5154).
    fn menu_render_background_layer2(&mut self, bg: u8, _frac: f32) {
        if (bg == MENUBG_CONEALPHA || bg == MENUBG_CONEOPAQUE) && (self.menudata.nextbg == MENUBG_CONEALPHA || self.menudata.nextbg == 0 || self.menudata.nextbg == 255) {
            self.menugfx_render_bg_cone();
        }
    }

    /// `menu_render` (menu.c:5168).
    pub fn menu_render(&mut self) {
        self.mpplayernum = 0;
        self.draw.gfx.full_scissor();
        // The frame behind the menus: PD draws the stage (CI) first. With no
        // background the spike shows black (the "screenshot" the blur comes from
        // is `res.blur`, see `Resources::blur_from_image`).
        if self.in_match {
            // The match is behind the menus (lv_render draws menu_render last):
            // the frame is a layer the game lays over its view.
            self.draw.gfx.clear_transparent();
        } else {
            self.draw.gfx.clear([0.0, 0.0, 0.0]);
        }
        let (bg, nextbg, frac) = (self.menudata.bg, self.menudata.nextbg, self.menudata.bgopacityfrac);
        if nextbg != 255 {
            if nextbg == 0 {
                self.menu_render_background_layer1(bg, 1.0 - frac);
            } else {
                self.menu_render_background_layer1(bg, 1.0);
                self.menu_render_background_layer1(nextbg, frac);
            }
        } else {
            self.menu_render_background_layer1(bg, 1.0);
        }
        // The hudpiece
        if self.menudata.triggerhudpiece {
            let hp = &mut self.menudata.hudpiece;
            hp.curanimnum = 0;
            hp.newanimnum = gd::ANIM_040D;
            hp.removingpiece = false;
            hp.reverseanim = false;
            self.menudata.hudpieceactive = true;
            self.menudata.triggerhudpiece = false;
        }
        self.draw.text.holoray_fromx = 0;
        self.draw.text.holoray_fromy = 0;
        if self.menudata.hudpieceactive {
            let mut removepiece = false;
            if self.rng.randomfrac() < 0.01 {
                let (a, b) = (self.rng.randomfrac(), self.rng.randomfrac());
                self.menudata.hudpiece.newposx = a * 80.0 + -205.5 - 40.0;
                self.menudata.hudpiece.newposy = b * 80.0 + 244.7 - 40.0;
            }
            if self.menudata.root == MENUROOT_MPSETUP && self.menudata.count <= 0 {
                removepiece = true;
            }
            if !matches!(self.menudata.root, MENUROOT_MAINMENU | MENUROOT_MPSETUP | MENUROOT_FILEMGR | MENUROOT_TRAINING) {
                removepiece = true;
            }
            if self.menus[0].curdialog.is_none() && self.menudata.root != MENUROOT_MPSETUP {
                removepiece = true;
            }
            if removepiece {
                let hp = &mut self.menudata.hudpiece;
                if !hp.removingpiece {
                    hp.reverseanim = true;
                    hp.curanimnum = 0;
                    hp.newanimnum = gd::ANIM_040D;
                    hp.removingpiece = true;
                } else if hp.curanimnum == 0 {
                    hp.removingpiece = false;
                    self.menudata.hudpieceactive = false;
                }
            }
            // main_override_variable("usePiece") (menu.c:5257).
            if std::env::var_os("PD_MENU_NO_PIECE").is_none() {
                self.menu_render_hudpiece();
            }
        }
        if nextbg != 255 {
            if nextbg == 0 {
                self.menu_render_background_layer2(bg, 1.0 - frac);
            } else {
                self.menu_render_background_layer2(bg, 1.0);
                self.menu_render_background_layer2(nextbg, frac);
            }
        } else {
            self.menu_render_background_layer2(bg, 1.0);
        }
        if self.menudata.count > 0 {
            // text_enable_holo_ray: the rays PD records go under the dialogs.
            self.draw.text.holorays.clear();
            self.draw.gfx.push_layer();
            if matches!(self.menudata.root, MENUROOT_MPPAUSE | MENUROOT_MPENDSCREEN) {
                // Each player's view draws that player's menus (the game draws
                // every view into the one frame).
                let slots: Vec<usize> = self.matchview.players.iter().map(|p| p.slot).collect();
                for i in slots {
                    self.mpplayernum = i;
                    self.menu_render_dialogs();
                }
            } else {
                for i in 0..4 {
                    self.mpplayernum = i;
                    self.menu_render_dialogs();
                }
            }
            self.mpplayernum = 0;
            self.draw.text.holoray_enabled = false;
            // Corner texts in the combat simulator
            if self.menudata.root == MENUROOT_MPSETUP {
                self.menu_render_corner_texts();
            }
            let reqs = std::mem::take(&mut self.draw.text.holorays);
            // Composite: the holoray list first, then the dialogs over it.
            let layer_done = self.gfx_pop_with_holorays(reqs);
            let _ = layer_done;
        }
        self.draw.gfx.full_scissor();
    }

    fn gfx_pop_with_holorays(&mut self, reqs: Vec<pd_core::text::HolorayReq>) -> bool {
        self.draw.gfx.swap_to_base();
        self.draw.gfx.full_scissor();
        for r in reqs {
            self.ortho_draw_holoray(r.x1, r.y1, r.x2, r.y2, r.colour1, r.colour2, r.plane, r.miny, r.maxy, r.fromx, r.fromy);
        }
        self.draw.gfx.composite_pending();
        true
    }

    /// The "Player N: Press START!" / "Ready!" corner labels (menu.c:5270).
    fn menu_render_corner_texts(&mut self) {
        let us = self.draw.gfx.uiscale;
        let viewleft = 20;
        let viewtop = 4;
        let viewright = self.draw.gfx.w as i32 / us - 20;
        let viewbottom = self.draw.gfx.h as i32 - 4;
        let (vw, vh) = (self.draw.gfx.w as i32, self.draw.gfx.h as i32);
        let fmt_player = self.lang(pd_core::lang::tx(gd::B_MPMENU, 482));
        for i in 0..4usize {
            let waiting = self.vars.mpsetupmenu == gd::MPSETUPMENU_GENERAL && self.vars.waitingtojoin[i];
            let text = if waiting {
                format!("{}{}", fmt_player, self.lang(pd_core::lang::tx(gd::B_MISC, 461)))
            } else {
                format!("{}{}", fmt_player, self.lang(pd_core::lang::tx(gd::B_MPMENU, 483)))
            };
            let textwidth = self.measure(&text, FontId::Sm).1;
            let connected = self.connected_pads();
            let diff = self.lv.diffframe60;
            let pja = &mut self.menudata.playerjoinalpha[i];
            if ((self.mp.setup.chrslots as u32 | !connected) & (1 << i)) == 0 {
                let tmp1 = diff * 3;
                if *pja < 255 {
                    if 255 - (*pja as i32) > tmp1 {
                        *pja += tmp1 as u8;
                    } else {
                        *pja = 255;
                    }
                }
            } else {
                let tmp2 = diff * 9;
                if *pja > 0 {
                    if *pja as i32 > tmp2 {
                        *pja -= tmp2 as u8;
                    } else {
                        *pja = 0;
                    }
                }
            }
            let alpha = *pja;
            if alpha > 0 {
                let weight = (sin_osc(self.frac20, 20.0) * 255.0) as u32;
                let t = fmt_player.replacen("%d", &(i + 1).to_string(), 1);
                let mut y = if i < 2 { viewtop + 2 } else { viewbottom - 9 };
                let mut x = if i == 1 || i == 3 { viewright - textwidth - 2 } else { viewleft + 2 };
                self.tc().render_v2(&mut x, &mut y, &t, FontId::Sm, alpha as u32 | 0x5070ff00, vw, vh, 0, 0);
                let (t2, colour) = if waiting {
                    (self.lang(pd_core::lang::tx(gd::B_MISC, 461)), alpha as u32 | 0xd00020ff)
                } else {
                    (self.lang(pd_core::lang::tx(gd::B_MPMENU, 483)), colour_blend(0x00ffff00, 0xffffff00, weight) | alpha as u32)
                };
                self.tc().render_v2(&mut x, &mut y, &t2, FontId::Sm, colour, vw, vh, 0, 0);
            }
        }
    }

    /// `menu_tick` (menutick.c:56), the parts the Combat Simulator exercises.
    pub fn menu_tick(&mut self) {
        // menu_tick_timers (game_006900.c:44)
        self.frac20 += self.lv.diffframe240f / 4800.0;
        if self.frac20 > 1.0 {
            self.frac20 -= 1.0;
        }
        self.menu_count_dialogs();
        let mut anyopen = false;
        for i in 0..4 {
            if self.menus[i].openinhibit > 0 {
                self.menus[i].openinhibit -= 1;
            }
            if self.menus[i].curdialog.is_some() {
                anyopen = true;
            }
        }
        if !anyopen && self.menudata.bg != 0 && self.menudata.nextbg == 255 {
            self.menudata.nextbg = 0;
        }
        self.menu_tick_pending_saves(anyopen);
        if self.menudata.nextbg != 255 {
            if self.menudata.nextbg == self.menudata.bg {
                self.menudata.nextbg = 255;
            } else {
                let mut mult = 0.02f32;
                if self.menudata.bg == 0 {
                    mult += mult;
                }
                if self.menudata.nextbg == 0 {
                    mult += mult;
                }
                if self.menudata.nextbg == 0 {
                    self.menudata.bgsettled = false;
                }
                if self.menudata.screenshottimer == 0 || self.menudata.bg != 0 {
                    let diffframe = self.lv.diffframe60f.min(4.0);
                    self.menudata.bgopacityfrac += mult * diffframe;
                }
                if self.menudata.bgopacityfrac > 1.0 {
                    if self.menudata.nextbg != 0 {
                        self.menudata.bgsettled = true;
                    }
                    self.menudata.bgopacityfrac = 0.0;
                    self.menudata.bg = self.menudata.nextbg;
                    self.menudata.nextbg = 255;
                }
            }
        } else {
            self.menudata.bgopacityfrac = 0.0;
        }
        // The screenshot for the blur is taken a frame after it's requested.
        if self.menudata.screenshottimer > 0 {
            self.menudata.screenshottimer -= 1;
        }
        self.vars.unk000498 = 0;
        if self.menudata.count > 0 {
            if self.menudata.root == MENUROOT_MPSETUP {
                if self.menudata.nextroot == -1 {
                    self.mp.setup.chrslots &= 0xfff0;
                }
                self.mp_num_joined = 0;
                for i in 0..4 {
                    if self.menus[i].curdialog.is_some() {
                        self.menus[i].playernum = self.mp_num_joined as usize;
                        self.mp_num_joined += 1;
                        if self.menudata.nextroot == -1 {
                            self.mp.setup.chrslots |= 1 << i;
                        }
                    }
                }
                self.mp_calculate_lock_if_last_winner_or_loser();
                self.challenge_perform_sanity_checks();
            }
            let mut allready = true;
            for i in 0..4 {
                self.mpplayernum = i;
                if let Some(def) = self.cur_def() {
                    if std::ptr::eq(def, &gd::G_MP_READY_MENU_DIALOG) {
                        self.vars.unk000498 = 1;
                    } else {
                        allready = false;
                    }
                }
            }
            for i in 0..4 {
                self.mpplayernum = i;
                if self.menus[i].curdialog.is_some() {
                    self.menu_process_input();
                } else if self.menudata.root == MENUROOT_MPSETUP {
                    // Check if player is joining the game
                    let joy = self.pads[i];
                    let buttons = joy.buttons & !joy.prev;
                    if self.mp.bossfile.locktype == gd::MPLOCKTYPE_CHALLENGE as u8 {
                        self.mp.players[i].base.team = 0;
                    }
                    if buttons & START_BUTTON != 0 {
                        self.mp.players[i].handicap = 128;
                        if self.vars.mpsetupmenu == gd::MPSETUPMENU_GENERAL {
                            if !self.vars.waitingtojoin[i] {
                                self.play_sound(gd::SFXMAP_809A_EXPLOSION, 1.0, 1.0);
                            }
                            self.vars.waitingtojoin[i] = true;
                        } else if self.vars.mpsetupmenu == gd::MPSETUPMENU_QUICKGO {
                            self.mp_num_joined += 1;
                            self.menu_push_root_dialog(&gd::G_MP_QUICK_GO_MENU_DIALOG, MENUROOT_MPSETUP);
                        } else {
                            self.mp_num_joined += 1;
                            self.mp_open_advanced_setup(false);
                        }
                    }
                    if buttons & START_BUTTON == 0 {
                        if buttons & B_BUTTON != 0 {
                            if self.vars.mpsetupmenu == gd::MPSETUPMENU_GENERAL {
                                self.vars.waitingtojoin[i] = false;
                            }
                        } else if self.vars.waitingtojoin[i] {
                            if self.vars.mpsetupmenu == gd::MPSETUPMENU_QUICKGO {
                                self.vars.waitingtojoin[i] = false;
                                self.mp_num_joined += 1;
                                self.menu_push_root_dialog(&gd::G_MP_QUICK_GO_MENU_DIALOG, MENUROOT_MPSETUP);
                            } else if self.vars.mpsetupmenu == gd::MPSETUPMENU_ADVSETUP {
                                self.vars.waitingtojoin[i] = false;
                                self.mp_num_joined += 1;
                                self.mp_open_advanced_setup(false);
                            }
                        }
                    }
                } else {
                    self.vars.waitingtojoin[i] = false;
                }
            }
            if allready && self.menudata.root == MENUROOT_MPSETUP {
                self.menu_save_and_push_root_dialog(None, MENUROOT_START_MP_MATCH);
            }
            self.menus_were_open = true;
        } else {
            self.menus_were_open = false;
        }
        if self.menus_were_open {
            if !self.mpsetup_filelists_made && self.menudata.root == MENUROOT_MPSETUP {
                self.mpsetup_filelists_made = true;
                self.filelist_create(0, pd_core::ids::FILETYPE_MPPLAYER as u8);
                self.filelist_create(1, pd_core::ids::FILETYPE_MPSETUP as u8);
            }
            if self.filelists_active {
                self.filelists_tick();
            }
        } else if self.filelists_active {
            self.menu_stop();
        }
        self.mpplayernum = 0;
        let anyopen2 = self.menus.iter().any(|m| m.curdialog.is_some());
        let mut anyopen2 = anyopen2;
        if (self.menudata.checkroots || self.menudata.nextroot != -1) && !anyopen2 {
            if self.menudata.root == MENUROOT_MPSETUP && self.menudata.nextroot == -1 {
                if self.vars.mpsetupmenu == gd::MPSETUPMENU_GENERAL {
                    self.menudata.nextroot = MENUROOT_MAINMENU;
                    self.menudata.nextdialog = Some(&gd::G_CI_MENU_VIA_PC_MENU_DIALOG);
                } else {
                    self.menudata.nextroot = MENUROOT_MPSETUP;
                    self.menudata.nextdialog = Some(&gd::G_COMBAT_SIMULATOR_MENU_DIALOG);
                }
            }
            if self.menudata.nextroot != -1 {
                if self.menudata.nextroot == MENUROOT_START_MP_MATCH {
                    // Match is beginning: mp_start_match + menu_stop, which hand
                    // the setup to the game (`MenuSystem::start_match`), and the
                    // boss file saved if the setup changed it.
                    self.start_match();
                } else if self.menudata.nextroot == MENUROOT_END_MP_MATCH {
                    // Match is ending: the agent's save queued, then each
                    // player's end screen by chr slot, and the save of each
                    // player who has a file.
                    self.menu_queue_save(4);
                    let mut playernum = 0;
                    for i in 0..4 {
                        if self.mp.setup.chrslots & (1 << i) != 0 {
                            self.mp_push_endscreen_dialog(playernum, i);
                            anyopen2 = true;
                            let g = self.mp.players[i].fileguid;
                            if g.fileid != 0 && g.deviceserial != 0 {
                                self.menu_queue_save(i as u8);
                            }
                            playernum += 1;
                        }
                    }
                } else if self.menudata.nextroot == MENUROOT_CHANGE_AGENT {
                    self.change_agent();
                    anyopen2 = true;
                } else if let Some(def) = self.menudata.nextdialog {
                    let root = self.menudata.nextroot;
                    self.menu_push_root_dialog(def, root);
                    anyopen2 = true;
                    let mut startmusic = false;
                    if self.menudata.root == MENUROOT_MPSETUP {
                        startmusic = true;
                        self.play_sound(gd::SFXMAP_8098_EXPLOSION, 1.0, 1.0);
                    }
                    // The Perfect Menu at CI's PC (not in a training room).
                    if matches!(self.menudata.root, MENUROOT_MAINMENU | MENUROOT_TRAINING) {
                        startmusic = true;
                    }
                    if startmusic {
                        self.music_start_menu();
                    }
                }
                self.menudata.nextdialog = None;
                self.menudata.nextroot = -1;
            } else if self.menudata.root == MENUROOT_MPENDSCREEN {
                // Every end screen is closed: g_MpReturningFromMatch, the match
                // unpaused and over, a challenge's lock released, and the CI
                // stage loads (the game leaves the match).
                if self.mp.bossfile.locktype == gd::MPLOCKTYPE_CHALLENGE as u8 {
                    self.mp.bossfile.locktype = gd::MPLOCKTYPE_NONE as u8;
                }
                self.outcomes.push_back(super::Outcome::ReturnFromMatch);
            }
        }
        self.menu_count_dialogs();
        if self.menudata.count == 0 {
            if self.menudata.nextbg != 255 {
                if self.menudata.nextbg != 0 {
                    self.menudata.bg = self.menudata.nextbg;
                    self.menudata.nextbg = 0;
                    self.menudata.bgopacityfrac = 1.0 - self.menudata.bgopacityfrac;
                }
            } else if self.menudata.bg != 0 {
                self.menudata.nextbg = 0;
            }
        }
        self.menudata.checkroots = anyopen2;
    }

    fn menu_count_dialogs(&mut self) {
        self.menudata.count = self.menus.iter().filter(|m| m.curdialog.is_some()).count() as i32;
    }

    /// `menu_render_model` for the current player's dialog model (character
    /// select). See `super::model`.
    fn menu_render_model_current(&mut self) {
        let p = self.mpplayernum;
        let mut mm = self.menus[p].menumodel;
        self.menu_render_model(&mut mm, MENUMODELTYPE_DEFAULT);
        self.menus[p].menumodel = mm;
    }

    fn menu_render_hudpiece(&mut self) {
        let mut hp = self.menudata.hudpiece;
        self.menu_render_model(&mut hp, MENUMODELTYPE_HUDPIECE);
        self.menudata.hudpiece = hp;
    }
}

/// Keep `DIAGMODE_FADEIN` referenced for readers grepping the modes.
pub const _DIAG_FADEIN: u8 = DIAGMODE_FADEIN;
