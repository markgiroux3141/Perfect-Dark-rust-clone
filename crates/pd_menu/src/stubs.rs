//! Dialogs the menus reach that this recreation leaves out: the Controller
//! Pak's game notes (Game Files' "Delete Game Notes...", `g_PakChoosePakMenuDialog`)
//! and the solo, co-op and counter-op menus. Each is a one-screen danger
//! dialog in PD's own dialog machinery, so the flow around it (open → B /
//! "OK" → back) behaves like a real dialog.
//!
//! SUBST: PD opens the real dialogs / no Controller Pak is ever plugged in
//! and there is no solo game here (CLAUDE.md), so these say so.

use super::types::*;
use super::MenuSystem;

fn pak_text(_pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    "There is no Controller Pak.\n".into()
}

fn not_here_text(_pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    "Only the Combat Simulator is here.\n".into()
}

fn ok_text(_pd: &mut MenuSystem, _item: &'static MenuItem) -> String {
    "OK\n".into()
}

fn pak_title(_pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    "Controller Pak\n".into()
}

fn pd_title(_pd: &mut MenuSystem, _def: &'static MenuDialogDef) -> String {
    "Perfect Dark\n".into()
}

pub static STUB_PAK_ITEMS: [MenuItem; 3] = [
    MenuItem { ty: MENUITEMTYPE_LABEL, param: 0, flags: MENUITEMFLAG_LESSLEFTPADDING, param2: P::Fn(pak_text), param3: P::Num(0), handler: H::None },
    MenuItem { ty: MENUITEMTYPE_SELECTABLE, param: 0, flags: MENUITEMFLAG_SELECTABLE_CLOSESDIALOG, param2: P::Fn(ok_text), param3: P::Num(0), handler: H::None },
    MenuItem::END,
];

pub static STUB_PAK_DIALOG: MenuDialogDef = MenuDialogDef {
    name: "stub_pak",
    ty: MENUDIALOGTYPE_DANGER,
    title: P::DFn(pak_title),
    items: &STUB_PAK_ITEMS,
    handler: None,
    flags: 0,
    nextsibling: None,
};

pub static STUB_NOT_IN_SPIKE_ITEMS: [MenuItem; 3] = [
    MenuItem { ty: MENUITEMTYPE_LABEL, param: 0, flags: MENUITEMFLAG_LESSLEFTPADDING, param2: P::Fn(not_here_text), param3: P::Num(0), handler: H::None },
    MenuItem { ty: MENUITEMTYPE_SELECTABLE, param: 0, flags: MENUITEMFLAG_SELECTABLE_CLOSESDIALOG, param2: P::Fn(ok_text), param3: P::Num(0), handler: H::None },
    MenuItem::END,
];

pub static STUB_NOT_IN_SPIKE_DIALOG: MenuDialogDef = MenuDialogDef {
    name: "stub_not_in_spike",
    ty: MENUDIALOGTYPE_DANGER,
    title: P::DFn(pd_title),
    items: &STUB_NOT_IN_SPIKE_ITEMS,
    handler: None,
    flags: 0,
    nextsibling: None,
};
