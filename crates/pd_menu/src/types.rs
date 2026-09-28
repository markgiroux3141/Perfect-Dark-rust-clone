//! The shapes of PD's menu data (`types.h:3252-3430`, `constants.h:1622-1810`):
//! `struct menuitem`, `struct menudialogdef`, `union handlerdata`, the menu
//! item data blocks, and the MP tables the generated module fills in.
//!
//! PD's `menuitem.param2` / `param3` are `intptr_t`s that hold, depending on the
//! item, a text id (`< 0x5a00`), a text *function* pointer, or a plain number
//! (a list's width, a checkbox's option bit). [`P`] keeps those three apart. An
//! item's `handler` is either a handler function or, for
//! `MENUITEMFLAG_SELECTABLE_OPENSDIALOG`, the dialog to open ([`H`]).

use pd_core::lang::Tx;
use super::MenuSystem;

// ---- constants.h:1622 ----
pub const MENUBG_BLUR: u8 = 1;
pub const MENUBG_BLACK: u8 = 2;
pub const MENUBG_FAILURE: u8 = 3;
pub const MENUBG_CONEALPHA: u8 = 4;
pub const MENUBG_GRADIENT: u8 = 5;
pub const MENUBG_6: u8 = 6;
pub const MENUBG_SUCCESS: u8 = 7;
pub const MENUBG_8: u8 = 8;
pub const MENUBG_CONEOPAQUE: u8 = 9;

pub const MENUDIALOGFLAG_CLOSEONSELECT: u32 = 0x0001;
pub const MENUDIALOGFLAG_ALLOW_MODELS: u32 = 0x0002;
pub const MENUDIALOGFLAG_STARTSELECTS: u32 = 0x0004;
pub const MENUDIALOGFLAG_DISABLEITEMSCROLL: u32 = 0x0008;
pub const MENUDIALOGFLAG_MPLOCKABLE: u32 = 0x0010;
pub const MENUDIALOGFLAG_IGNOREBACK: u32 = 0x0020;
pub const MENUDIALOGFLAG_SMOOTHSCROLLABLE: u32 = 0x0040;
pub const MENUDIALOGFLAG_DISABLEBANNER: u32 = 0x0080;
pub const MENUDIALOGFLAG_DISABLETITLEBAR: u32 = 0x0100;
pub const MENUDIALOGFLAG_DISABLERESIZE: u32 = 0x0200;
pub const MENUDIALOGFLAG_NOVERTICALBORDERS: u32 = 0x0400;
pub const MENUDIALOGFLAG_DROPOUTONCLOSE: u32 = 0x0800;
pub const MENUDIALOGFLAG_LESSHEIGHT: u32 = 0x1000;

pub const MENUDIALOGSTATE_PREOPEN: u8 = 0;
pub const MENUDIALOGSTATE_OPENING: u8 = 1;
pub const MENUDIALOGSTATE_POPULATING: u8 = 2;
pub const MENUDIALOGSTATE_POPULATED: u8 = 3;

pub const MENUDIALOGTYPE_DEFAULT: u8 = 1;
pub const MENUDIALOGTYPE_DANGER: u8 = 2;
pub const MENUDIALOGTYPE_SUCCESS: u8 = 3;
pub const MENUDIALOGTYPE_4: u8 = 4;
pub const MENUDIALOGTYPE_WHITE: u8 = 5;

pub const MENUITEMFLAG_NEWCOLUMN: u32 = 0x00000001;
pub const MENUITEMFLAG_00000002: u32 = 0x00000002;
pub const MENUITEMFLAG_SELECTABLE_OPENSDIALOG: u32 = 0x00000004;
pub const MENUITEMFLAG_SELECTABLE_CLOSESDIALOG: u32 = 0x00000008;
pub const MENUITEMFLAG_LESSLEFTPADDING: u32 = 0x00000010;
pub const MENUITEMFLAG_SELECTABLE_CENTRE: u32 = 0x00000020;
pub const MENUITEMFLAG_LIST_WIDE: u32 = 0x00000040;
pub const MENUITEMFLAG_DROPDOWN_BELOW: u32 = 0x00000080;
pub const MENUITEMFLAG_LABEL_ALTCOLOUR: u32 = 0x00000100;
pub const MENUITEMFLAG_SMALLFONT: u32 = 0x00000200;
pub const MENUITEMFLAG_ALWAYSDISABLED: u32 = 0x00000400;
pub const MENUITEMFLAG_MARQUEE_FADEBOTHSIDES: u32 = 0x00000800;
pub const MENUITEMFLAG_SLIDER_FAST: u32 = 0x00000800;
pub const MENUITEMFLAG_ADJUSTWIDTH: u32 = 0x00001000;
pub const MENUITEMFLAG_SLIDER_HIDEVALUE: u32 = 0x00002000;
pub const MENUITEMFLAG_DARKERBG: u32 = 0x00004000;
pub const MENUITEMFLAG_LABEL_HASRIGHTTEXT: u32 = 0x00008000;
pub const MENUITEMFLAG_DISABLESCROLL: u32 = 0x00010000;
pub const MENUITEMFLAG_LOCKABLEMINOR: u32 = 0x00020000;
pub const MENUITEMFLAG_LOCKABLEMAJOR: u32 = 0x00040000;
pub const MENUITEMFLAG_MPWEAPONSLOT: u32 = 0x00080000;
pub const MENUITEMFLAG_SLIDER_ALTSIZE: u32 = 0x00100000;
pub const MENUITEMFLAG_LIST_CUSTOMRENDER: u32 = 0x00200000;
pub const MENUITEMFLAG_BIGFONT: u32 = 0x00400000;
pub const MENUITEMFLAG_LIST_AUTOWIDTH: u32 = 0x00800000;
pub const MENUITEMFLAG_LABEL_CUSTOMCOLOUR: u32 = 0x01000000;
pub const MENUITEMFLAG_LESSHEIGHT: u32 = 0x02000000;
pub const MENUITEMFLAG_CAROUSEL_SCROLLWITHOUTFOCUS: u32 = 0x04000000;

pub const MENUITEMTYPE_LABEL: u8 = 0x01;
pub const MENUITEMTYPE_LIST: u8 = 0x02;
pub const MENUITEMTYPE_03: u8 = 0x03;
pub const MENUITEMTYPE_SELECTABLE: u8 = 0x04;
pub const MENUITEMTYPE_SCROLLABLE: u8 = 0x05;
pub const MENUITEMTYPE_OBJECTIVES: u8 = 0x06;
pub const MENUITEMTYPE_07: u8 = 0x07;
pub const MENUITEMTYPE_SLIDER: u8 = 0x08;
pub const MENUITEMTYPE_CHECKBOX: u8 = 0x09;
pub const MENUITEMTYPE_0A: u8 = 0x0a;
pub const MENUITEMTYPE_SEPARATOR: u8 = 0x0b;
pub const MENUITEMTYPE_DROPDOWN: u8 = 0x0c;
pub const MENUITEMTYPE_KEYBOARD: u8 = 0x0d;
pub const MENUITEMTYPE_RANKING: u8 = 0x0e;
pub const MENUITEMTYPE_PLAYERSTATS: u8 = 0x0f;
pub const MENUITEMTYPE_10: u8 = 0x10;
pub const MENUITEMTYPE_CAROUSEL: u8 = 0x11;
pub const MENUITEMTYPE_MODEL: u8 = 0x12;
pub const MENUITEMTYPE_13: u8 = 0x13;
pub const MENUITEMTYPE_14: u8 = 0x14;
pub const MENUITEMTYPE_METER: u8 = 0x15;
pub const MENUITEMTYPE_16: u8 = 0x16;
pub const MENUITEMTYPE_MARQUEE: u8 = 0x17;
pub const MENUITEMTYPE_18: u8 = 0x18;
pub const MENUITEMTYPE_CONTROLLER: u8 = 0x19;
pub const MENUITEMTYPE_END: u8 = 0x1a;

pub const MENUMODELFLAG_HASSCALE: u8 = 0x01;
pub const MENUMODELFLAG_HASPOSITION: u8 = 0x02;
pub const MENUMODELFLAG_HASROTATION: u8 = 0x04;

pub const MENUMODELTYPE_DEFAULT: i32 = 0;
pub const MENUMODELTYPE_HUDPIECE: i32 = 1;
pub const MENUMODELTYPE_2: i32 = 2;
pub const MENUMODELTYPE_3: i32 = 3;

pub const MENUOP_GET_OPTION_COUNT: i32 = 1;
pub const MENUOP_GET_OPTGROUP_COUNT: i32 = 2;
pub const MENUOP_GET_OPTION_TEXT: i32 = 3;
pub const MENUOP_GET_OPTGROUP_TEXT: i32 = 4;
pub const MENUOP_GET_OPTGROUP_START_INDEX: i32 = 5;
pub const MENUOP_CONFIRM: i32 = 6;
pub const MENUOP_GET_SELECTED_INDEX: i32 = 7;
pub const MENUOP_IS_CHECKED: i32 = 8;
pub const MENUOP_GET_SLIDER_VALUE: i32 = 9;
pub const MENUOP_GET_SLIDER_LABEL: i32 = 10;
pub const MENUOP_ON_CAROUSEL_TICK: i32 = 11;
pub const MENUOP_IS_DISABLED: i32 = 12;
pub const MENUOP_ON_FOCUS: i32 = 13;
pub const MENUOP_IS_OPTION_CHECKED: i32 = 14;
pub const MENUOP_IS_PREFOCUSED: i32 = 15;
pub const MENUOP_ON_OPTION_FOCUS: i32 = 16;
pub const MENUOP_GET_KEYBOARD_STRING: i32 = 17;
pub const MENUOP_SET_KEYBOARD_STRING: i32 = 18;
pub const MENUOP_RENDER: i32 = 19;
pub const MENUOP_GET_OPTION_HEIGHT: i32 = 20;
pub const MENUOP_IS_CAROUSEL_OPTION_HIDDEN: i32 = 21;
pub const MENUOP_GET_LABEL_COLOURS: i32 = 22;
pub const MENUOP_IS_HIDDEN: i32 = 24;
pub const MENUOP_GET_OPTION_INDEX2: i32 = 25;
pub const MENUOP_ON_OPEN: i32 = 100;
pub const MENUOP_ON_CLOSE: i32 = 101;
pub const MENUOP_ON_TICK: i32 = 102;

pub const MENUROOT_ENDSCREEN: i32 = 1;
pub const MENUROOT_MAINMENU: i32 = 2;
pub const MENUROOT_MPSETUP: i32 = 3;
pub const MENUROOT_MPPAUSE: i32 = 4;
pub const MENUROOT_MPENDSCREEN: i32 = 5;
pub const MENUROOT_FILEMGR: i32 = 6;
pub const MENUROOT_TRAINING: i32 = 13;
pub const MENUROOT_START_MP_MATCH: i32 = -5;

pub const MENUREPEATMODE_RELEASED: i16 = -1;
pub const MENUREPEATMODE_SLOW: i16 = 0;
pub const MENUREPEATMODE_FAST: i16 = 1;

pub const MENUSOUND_SWIPE: i32 = 0x00;
pub const MENUSOUND_OPENDIALOG: i32 = 0x01;
pub const MENUSOUND_FOCUS: i32 = 0x02;
pub const MENUSOUND_SELECT: i32 = 0x03;
pub const MENUSOUND_ERROR: i32 = 0x04;
pub const MENUSOUND_EXPLOSION: i32 = 0x05;
pub const MENUSOUND_TOGGLEON: i32 = 0x08;
pub const MENUSOUND_TOGGLEOFF: i32 = 0x09;
pub const MENUSOUND_SUBFOCUS: i32 = 0x0a;
pub const MENUSOUND_0B: i32 = 0x0b;
pub const MENUSOUND_KEYBOARDFOCUS: i32 = 0x0c;
pub const MENUSOUND_KEYBOARDCANCEL: i32 = 0x0d;
pub const MENUSOUND_SUCCESS: i32 = 0x0e;

pub const MENUTICKFLAG_DIALOGISCURRENT: u32 = 0x01;
pub const MENUTICKFLAG_ITEMISFOCUSED: u32 = 0x02;
pub const MENUTICKFLAG_DIALOGISDIMMED: u32 = 0x04;

pub const MENUPLANE_00: i32 = 0;
pub const MENUPLANE_01: i32 = 1;
pub const MENUPLANE_02: i32 = 2;
pub const MENUPLANE_03: i32 = 3;
pub const MENUPLANE_04: i32 = 4;
pub const MENUPLANE_05: i32 = 5;
pub const MENUPLANE_06: i32 = 6;
pub const MENUPLANE_07: i32 = 7;
pub const MENUPLANE_08: i32 = 8;
pub const MENUPLANE_09: i32 = 9;
pub const MENUPLANE_10: i32 = 10;
pub const MENUPLANE_11: i32 = 11;

/// `LINEHEIGHT` (constants.h:65, non-JPN).
pub const LINEHEIGHT: i32 = 11;

// ---- joy.h button masks: the one copy is n64::pad ----
pub use n64::pad::{
    A_BUTTON, B_BUTTON, D_CBUTTONS, D_JPAD, L_CBUTTONS, L_JPAD, L_TRIG, R_CBUTTONS, R_JPAD, R_TRIG, START_BUTTON, U_CBUTTONS, U_JPAD, Z_TRIG,
};

// ---- The data ----

pub type ItemTextFn = fn(&mut MenuSystem, &'static MenuItem) -> String;
pub type DialogTextFn = fn(&mut MenuSystem, &'static MenuDialogDef) -> String;
pub type ItemHandler = fn(&mut MenuSystem, i32, &'static MenuItem, &mut HandlerData) -> HRet;
pub type DialogHandler = fn(&mut MenuSystem, i32, &'static MenuDialogDef, &mut HandlerData) -> i32;

/// A `param2` / `param3` / dialog `title` (`uintptr_t`).
#[derive(Clone, Copy)]
pub enum P {
    Num(i32),
    Text(Tx),
    Fn(ItemTextFn),
    DFn(DialogTextFn),
}

impl P {
    pub fn num(&self) -> i32 {
        match self {
            P::Num(n) => *n,
            _ => 0,
        }
    }
}

/// `menuitem.handler`: a handler, or (`SELECTABLE_OPENSDIALOG`) the dialog.
#[derive(Clone, Copy)]
pub enum H {
    None,
    Fn(ItemHandler),
    Dialog(&'static MenuDialogDef),
}

/// `struct menuitem` (types.h:3400).
pub struct MenuItem {
    pub ty: u8,
    pub param: i32,
    pub flags: u32,
    pub param2: P,
    pub param3: P,
    pub handler: H,
}

impl MenuItem {
    pub const END: MenuItem = MenuItem { ty: MENUITEMTYPE_END, param: 0, flags: 0, param2: P::Num(0), param3: P::Num(0), handler: H::None };

    /// PD guards `handler(...)` with `(flags & SELECTABLE_OPENSDIALOG) == 0`
    /// because the field then holds a dialog; this is that check.
    pub fn fn_handler(&self) -> Option<ItemHandler> {
        match self.handler {
            H::Fn(f) if self.flags & MENUITEMFLAG_SELECTABLE_OPENSDIALOG == 0 => Some(f),
            _ => None,
        }
    }
}

/// `struct menudialogdef` (types.h:3413). `name` is the C symbol, for logs.
pub struct MenuDialogDef {
    pub name: &'static str,
    pub ty: u8,
    pub title: P,
    pub items: &'static [MenuItem],
    pub handler: Option<DialogHandler>,
    pub flags: u32,
    pub nextsibling: Option<&'static MenuDialogDef>,
}

impl PartialEq for MenuDialogDef {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// A handler's return (`MenuItemHandlerResult`): PD returns ints, and casts a
/// `char *` to `s32` for the `GET_*_TEXT` ops; here those are [`HRet::S`].
pub enum HRet {
    I(i32),
    S(String),
}

impl HRet {
    pub fn int(&self) -> i32 {
        match self {
            HRet::I(v) => *v,
            HRet::S(_) => 1,
        }
    }
    pub fn text(self) -> String {
        match self {
            HRet::S(s) => s,
            HRet::I(_) => String::new(),
        }
    }
}

impl From<i32> for HRet {
    fn from(v: i32) -> Self {
        HRet::I(v)
    }
}
impl From<bool> for HRet {
    fn from(v: bool) -> Self {
        HRet::I(v as i32)
    }
}
impl From<String> for HRet {
    fn from(s: String) -> Self {
        HRet::S(s)
    }
}

/// `struct menuitemrenderdata` (types.h:3360).
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderData {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub colour: u32,
    pub unk10: bool,
}

/// `union handlerdata` (types.h:3386). Every union member overlays the same
/// four words, so the fields here are the words: `value` is `list.value` =
/// `dropdown.value` = `carousel.value` = `checkbox.value` = `slider.value` =
/// `dialog1.preventclose`; `unk04` is `list.unk04` = `dropdown.unk04` =
/// `carousel.unk04` = `type19.unk04`. The pointer members get their own slots:
/// `slider.label` → `label`, `keyboard.string` → `string`,
/// `type19.renderdata2` → `render`, `label.colour1/colour2` → `colour1/2`,
/// `dialog2.inputs` → `inputs`.
#[derive(Clone, Debug, Default)]
pub struct HandlerData {
    pub value: i32,
    pub unk04: i32,
    pub groupstartindex: i32,
    pub unk0c: i32,
    pub label: String,
    pub string: [u8; 11],
    pub render: Option<RenderData>,
    pub colour1: u32,
    pub colour2: u32,
    pub inputs: Option<MenuInputs>,
}

/// `struct menuinputs` (types.h:4903).
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuInputs {
    pub leftright: i8,
    pub updown: i8,
    pub select: u8,
    pub back: u8,
    pub xaxis: i8,
    pub yaxis: i8,
    pub shoulder: u8,
    pub back2: u8,
    pub leftrightheld: i8,
    pub updownheld: i8,
    pub start: bool,
    pub unk0c: i32,
    pub unk10: i32,
    pub unk14: u8,
}

/// `union menuitemdata` (types.h:3308), flattened: `dropdown.list` is `list`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemData {
    // menuitemdata_list / dropdown.list
    pub curoffsety: i16,
    pub index: i16,
    pub targetoffsety: i16,
    pub viewheight: i16,
    // dropdown
    pub scrolloffset: i16,
    pub unk0e: u16,
    // keyboard
    pub string: [u8; 11],
    pub col: i8,
    pub row: i8,
    pub capslock: bool,
    pub capseffective: bool,
    // marquee
    pub totalmoved: u16,
    pub texthash: u16,
    pub viewwidth: u16,
    // scrollable (scrolloffset shared)
    pub maxscrolloffset: i16,
    pub dialogheight: i16,
    // slider
    pub multiplier: i16,
}

// ---- MP tables (types.h) ----

#[derive(Clone, Copy)]
pub struct MpArena {
    pub stagenum: i32,
    pub requirefeature: i32,
    pub name: Tx,
}

#[derive(Clone, Copy)]
pub struct MpHead {
    pub headnum: i32,
    pub requirefeature: i32,
}

#[derive(Clone, Copy)]
pub struct BotProfile {
    pub ty: i32,
    pub difficulty: i32,
    pub name: Tx,
    pub body: i32,
    pub requirefeature: i32,
}

#[derive(Clone, Copy)]
pub struct MpBody {
    pub bodynum: i32,
    pub name: Tx,
    pub headnum: i32,
    pub requirefeature: i32,
}

#[derive(Clone, Copy)]
pub struct MpWeapon {
    pub weaponnum: i32,
    pub priammotype: i32,
    pub priammoqty: i32,
    pub secammotype: i32,
    pub secammoqty: i32,
    pub hasweapon: i32,
    pub unlockfeature: i32,
    pub model: i32,
    pub extrascale: i32,
}

#[derive(Clone, Copy)]
pub struct MpWeaponSet {
    pub name: Tx,
    pub slots: [i32; 6],
    pub requirefeatures: [i32; 4],
    pub slotsiflocked: [i32; 6],
}

#[derive(Clone, Copy)]
pub struct MpTrack {
    pub musicnum: i32,
    pub duration: i32,
    pub name: Tx,
    pub unlockstage: i32,
}

#[derive(Clone, Copy)]
pub struct MpPreset {
    pub name: Tx,
    pub confignum: i32,
}

#[derive(Clone, Copy)]
pub struct ChallengeDef {
    pub name: Tx,
    pub confignum: i32,
}

#[derive(Clone, Copy)]
pub struct MpScenarioOverview {
    pub name: Tx,
    pub shortname: Tx,
    pub requirefeature: i32,
    pub teamonly: bool,
}

/// `struct headorbody` (types.h:3062); `file` is the `FILE_*` name.
#[derive(Clone, Copy)]
pub struct HeadOrBody {
    pub ismale: bool,
    pub unk00_01: bool,
    pub canvaryheight: bool,
    /// `HEADBODYTYPE_*` (the head-offset table in `body_calculate_head_offset`).
    pub ty: i32,
    pub height: i32,
    pub filenum: i32,
    pub file: &'static str,
    pub scale: f32,
    pub animscale: f32,
}
