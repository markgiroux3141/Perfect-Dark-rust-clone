//! The active menu on the pillar arena: one player (on a pad) and, for the
//! order screens, simulants on its team.

use pd_core::events::Event;
use pd_core::ids::*;

use super::*;
use crate::bot::MyAction;
use crate::harness;
use crate::player::PlayerInput;
use crate::testutil::res;
use crate::world::World;

/// One player with the default weapon set and `bots` simulants; with
/// `teams`, the first `mates` on the player's team (0), the rest on team 1.
fn world(bots: usize, teams: bool, mates: usize) -> World {
    let mut setup = harness::with_weapons(harness::setup(1, bots, BOTDIFF_NORMAL), &harness::DEFAULT_SET);
    if teams {
        setup.options |= MPOPTION_TEAMSENABLED;
        setup.players[0].chr.team = 0;
        for (k, s) in setup.simulants.iter_mut().enumerate() {
            s.chr.team = if k < mates { 0 } else { 1 };
        }
    }
    let mut w = harness::arena(res(), setup, harness::SPIKE_SEED).unwrap();
    w.bot_brains = false;
    for _ in 0..150 {
        harness::step_idle(&mut w);
    }
    w
}

fn pad() -> PlayerInput {
    PlayerInput { pad: true, ..PlayerInput::default() }
}

fn step(w: &mut World, input: &PlayerInput, n: usize) {
    for _ in 0..n {
        w.step(4, std::slice::from_ref(input));
    }
}

/// Hold A until the menu opens.
fn open(w: &mut World) {
    step(w, &PlayerInput { a_held: true, ..pad() }, 20);
    assert_eq!(w.players[0].activemenumode, AMMODE_VIEW, "open after 15 ticks of A");
}

/// Z (tapped) with A held: the next screen from the middle slot.
fn next_screen(w: &mut World) {
    step(w, &PlayerInput { a_held: true, fire: true, ..pad() }, 1);
    step(w, &PlayerInput { a_held: true, ..pad() }, 1);
}

/// Z until screen `n` (a player with nothing in hand has no function
/// screen: PD skips it).
fn to_screen(w: &mut World, n: i32) {
    for _ in 0..4 {
        if w.players[0].am.screenindex == n {
            return;
        }
        next_screen(w);
    }
    assert_eq!(w.players[0].am.screenindex, n);
}

#[test]
fn holding_a_opens_the_menu_and_takes_the_control() {
    let mut w = world(0, false, 0);
    step(&mut w, &PlayerInput { a_held: true, ..pad() }, 10);
    assert_eq!(w.players[0].activemenumode, AMMODE_CLOSED, "not yet");
    step(&mut w, &PlayerInput { a_held: true, ..pad() }, 10);
    assert_eq!(w.players[0].activemenumode, AMMODE_VIEW);
    assert!(!w.mp.players[0].withcontrol);
    // The stick walks nowhere while it is open.
    let pos = w.players[0].pos;
    step(&mut w, &PlayerInput { a_held: true, look_y: 80, ..pad() }, 30);
    assert!(w.players[0].pos.distance(pos) < 1.0, "the player stood still");
    // Letting go closes it and gives the control back.
    step(&mut w, &pad(), 1);
    assert_eq!(w.players[0].activemenumode, AMMODE_CLOSED);
    assert!(w.mp.players[0].withcontrol);
}

/// The weapon set's favourites (unarmed, Falcon, CMP150, Shotgun, grenade,
/// rocket launcher; the shield has none) in `g_AmMapping`'s places, and the
/// inventory in them.
#[test]
fn the_weapon_screen_places_the_set_and_equips_a_pick() {
    let mut w = world(0, false, 0);
    let fav = w.players[0].am.favourites;
    assert_eq!(fav, [WEAPON_UNARMED, WEAPON_FALCON2, 0xff, WEAPON_SHOTGUN, WEAPON_GRENADE, 0xff, WEAPON_CMP150, WEAPON_ROCKETLAUNCHER]);
    let inv = &mut w.players[0].gun.p.inventory;
    inv.inv_give_single_weapon(WEAPON_CMP150);
    inv.inv_give_single_weapon(WEAPON_FALCON2);
    open(&mut w);
    let (_, top) = w.am_get_slot_details(0, 1);
    let (_, bottom) = w.am_get_slot_details(0, 7);
    let (_, middle) = w.am_get_slot_details(0, 4);
    println!("top {top:?}, bottom {bottom:?}, middle {middle:?}");
    assert!(top.starts_with("Falcon"), "{top:?}");
    assert!(bottom.starts_with("CMP"), "{bottom:?}");
    assert!(middle.starts_with("Weapon"), "{middle:?}");
    // Stick down onto the CMP150, let go of A: it comes up.
    step(&mut w, &PlayerInput { a_held: true, look_y: -80, ..pad() }, 2);
    assert_eq!(w.players[0].am.slotnum, 7);
    step(&mut w, &pad(), 1);
    step(&mut w, &pad(), 60);
    assert_eq!(w.players[0].gun.bgun_get_weapon_num(HAND_RIGHT), WEAPON_CMP150);
}

#[test]
fn the_function_screen_switches_to_the_secondary() {
    let mut w = world(0, false, 0);
    w.players[0].gun.p.inventory.inv_give_single_weapon(WEAPON_CMP150);
    w.players[0].gun.select_weapon(WEAPON_CMP150, false);
    step(&mut w, &pad(), 60);
    assert!(!w.players[0].gun.funcissec());
    open(&mut w);
    next_screen(&mut w);
    assert_eq!(w.players[0].am.screenindex, 1);
    let (flags, label) = w.am_get_slot_details(0, 1);
    assert!(flags & AMSLOTFLAG_CURRENT != 0, "the primary is current ({label:?})");
    step(&mut w, &PlayerInput { a_held: true, look_y: -80, ..pad() }, 2);
    step(&mut w, &pad(), 2);
    assert!(w.players[0].gun.funcissec(), "on the secondary");
}

/// Screen 2 orders the one simulant teammate; the ordered simulant is the
/// commanding one (it pulses in its team colour).
#[test]
fn an_order_screen_orders_a_teammate() {
    let mut w = world(2, true, 1);
    let mate = w.players[0].aibuddynums.clone();
    assert_eq!(mate.len(), 1);
    let mate = mate[0];
    open(&mut w);
    to_screen(&mut w, 2);
    assert_eq!(w.players[0].commandingaibot, Some(mate));
    assert!(w.scenario_highlight_chr(0, mate).is_some_and(|c| c[3] <= 128), "the pulse");
    let (_, label) = w.am_get_slot_details(0, 3);
    assert!(label.starts_with("Defend"), "{label:?}");
    // Stick left onto Defend, let go.
    step(&mut w, &PlayerInput { a_held: true, look_x: -80, ..pad() }, 2);
    step(&mut w, &pad(), 1);
    let a = w.chrs[mate].aibot.as_ref().unwrap();
    assert_eq!(a.command, AIBOTCMD_DEFEND);
    assert!(a.defendholdpos.distance(w.chrs[0].pos) < 1.0);
    assert_eq!(w.players[0].commandingaibot, None, "closed");
}

/// R on an order screen orders every simulant teammate.
#[test]
fn r_orders_all_simulants() {
    let mut w = world(3, true, 2);
    let mates = w.players[0].aibuddynums.clone();
    assert_eq!(mates.len(), 2);
    open(&mut w);
    to_screen(&mut w, 2);
    // Hold R, stick to the top left (Follow).
    step(&mut w, &PlayerInput { a_held: true, aim: true, look_x: -80, look_y: 80, ..pad() }, 2);
    assert!(w.players[0].am.allbots);
    assert_eq!(w.players[0].commandingaibot, None, "no single simulant");
    step(&mut w, &PlayerInput { aim: true, ..pad() }, 1);
    for m in mates {
        let a = w.chrs[m].aibot.as_ref().unwrap();
        assert_eq!((a.command, a.followprotectpropnum), (AIBOTCMD_FOLLOW, Some(0)));
    }
}

/// Attack hands over to the menus' Pick Target with everyone but the
/// simulant; the pick sends it after that chr.
#[test]
fn attack_asks_for_a_target() {
    let mut w = world(2, true, 1);
    let mate = w.players[0].aibuddynums[0];
    let enemy = (1..w.chrs.len()).find(|&i| i != mate).unwrap();
    open(&mut w);
    to_screen(&mut w, 2);
    w.take_events();
    // Top middle is Attack; Z applies it.
    step(&mut w, &PlayerInput { a_held: true, look_y: 80, ..pad() }, 2);
    step(&mut w, &PlayerInput { a_held: true, look_y: 80, fire: true, ..pad() }, 1);
    let targets = w.take_events().into_iter().find_map(|e| match e {
        Event::AmOpenPickTarget { player: 0, targets } => Some(targets),
        _ => None,
    });
    let targets = targets.expect("Pick Target asked for");
    let listed: Vec<usize> = targets.iter().map(|&(c, _)| c as usize).collect();
    assert_eq!(listed, (0..w.chrs.len()).filter(|&i| i != mate).collect::<Vec<_>>());
    assert_eq!(w.players[0].activemenumode, AMMODE_CLOSED);
    w.am_pick_target(0, enemy);
    w.bot_tick_mainloop(mate);
    let a = w.chrs[mate].aibot.as_ref().unwrap();
    assert_eq!((a.command, a.myaction, a.attackingplayernum), (AIBOTCMD_ATTACK, MyAction::Attack, Some(enemy)));
}

/// `am_calculate_slot_position` for one player on the 320 × 220 screen.
#[test]
fn the_slots_sit_around_the_middle_of_the_screen() {
    let am = ActiveMenu { xradius: 60, slotwidth: 55, ..ActiveMenu::default() };
    let v = AmView { left: 0, top: 0, width: 320, height: 220, playercount: 1, playernum: 0, vsplit: false };
    assert_eq!(am.am_calculate_slot_position(1, 1, &v), (160, 110));
    assert_eq!(am.am_calculate_slot_position(1, 0, &v), (160, 60));
    assert_eq!(am.am_calculate_slot_position(0, 1, &v), (100, 110));
    assert_eq!(am.am_calculate_slot_position(2, 2, &v), (190, 135));
}

/// Devices switch rather than equip: the pause menu's inventory row turns the
/// x-ray scanner on (x-ray vision) and off; the active menu's slot turns the
/// cloaking device on (its label "Cloak N", the seconds left).
#[test]
fn devices_switch_on_and_off() {
    let mut w = world(0, false, 0);
    w.players[0].gun.p.unlimited_ammo = false;
    let inv = &mut w.players[0].gun.p.inventory;
    inv.inv_give_single_weapon(WEAPON_XRAYSCANNER);
    inv.inv_give_single_weapon(WEAPON_CLOAKINGDEVICE);
    w.players[0].gun.p.ammoheldarr[AMMOTYPE_CLOAK as usize] = 600;
    let row = |w: &World, wn: u8| (0..w.players[0].gun.p.inventory.inv_get_count()).find(|&i| w.players[0].gun.p.inventory.inv_get_weapon_num_by_index(i) == wn).unwrap() as usize;
    let xray = row(&w, WEAPON_XRAYSCANNER);
    w.mp_equip_inventory(0, xray);
    assert!(w.players[0].devicesactive & DEVICE_XRAYSCANNER != 0);
    step(&mut w, &pad(), 2);
    assert_eq!(w.players[0].visionmode, VISIONMODE_XRAY);
    w.mp_equip_inventory(0, xray);
    step(&mut w, &pad(), 2);
    assert_eq!(w.players[0].visionmode, VISIONMODE_NORMAL);
    // The cloaking device's slot in the active menu.
    open(&mut w);
    let place = w.players[0].am.invindexes.iter().position(|&i| i as usize == row(&w, WEAPON_CLOAKINGDEVICE)).expect("placed") as u8;
    let slot = if place >= 4 { place + 1 } else { place };
    let (_, label) = w.am_get_slot_details(0, slot);
    assert_eq!(label.trim_end(), "cloak 10", "{label:?}");
    w.am_apply(0, slot);
    assert!(w.players[0].devicesactive & DEVICE_CLOAKDEVICE != 0);
    let (flags, _) = w.am_get_slot_details(0, slot);
    assert!(flags & AMSLOTFLAG_ACTIVE != 0, "the slot pulses while it's on");
}
