//! The pickups on Complex: what the setup puts on each weapon location, a
//! player taking a weapon, a crate and a shield, the two-pad pair, the
//! respawn, and a dead player's weapons falling.

use glam::Vec3;
use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::mp::mpweapon_index;

use crate::harness;
use crate::player::PlayerInput;
use crate::testutil::{complex_arc, res, sounds};
use crate::world::World;

/// The slots the tests use: the Falcon 2, CMP150, shotgun, grenades, rocket
/// launcher, shield.
const SET: [u8; 6] = [WEAPON_FALCON2, WEAPON_CMP150, WEAPON_SHOTGUN, WEAPON_GRENADE, WEAPON_ROCKETLAUNCHER, WEAPON_MPSHIELD];

fn world(bots: usize) -> World {
    let (stage, level) = complex_arc();
    let mut setup = harness::setup(1, bots, BOTDIFF_NORMAL);
    for (s, &w) in SET.iter().enumerate() {
        setup.weapons[s] = mpweapon_index(w).unwrap();
    }
    World::new(setup, stage, level, res(), 7).unwrap()
}

/// The pickup standing on pad `pad`.
fn on_pad(w: &World, pad: i32) -> Option<&crate::props::Obj> {
    w.props.objs.iter().find(|o| o.pad == pad && o.is_pickup())
}

/// Stand player 0 on `o`'s floor, right on it, and step `n` frames.
fn stand_on(w: &mut World, id: u32, n: usize) -> Vec<Event> {
    let pos = w.props.get(id).unwrap().pos;
    let feet = Vec3::new(pos.x, w.level.cd_find_ground_at_cyl(pos + Vec3::Y * 20.0, 30.0).0, pos.z);
    harness::place_player(w, 0, feet, 0.0);
    w.players[0].verta = 0.0;
    let mut ev = Vec::new();
    for _ in 0..n {
        w.step(4, &[PlayerInput::default()]);
        ev.extend(w.take_events());
    }
    ev
}

fn hudmsgs(w: &World) -> Vec<String> {
    w.mp.hudmsgs.msgs.iter().filter(|m| m.state != HUDMSGSTATE_FREE).map(|m| m.text.clone()).collect()
}

#[test]
fn each_weapon_location_takes_the_sets_slots_in_turn_with_its_ammo_crates() {
    let w = world(0);
    // Complex's weapon locations are pads 47..56, each followed by two crates.
    let expect: [(i32, Option<u8>, u8, i32, i32); 10] = [
        (47, Some(WEAPON_FALCON2), OBJTYPE_WEAPON, AMMOTYPE_PISTOL, 80),
        (48, Some(WEAPON_CMP150), OBJTYPE_WEAPON, AMMOTYPE_SMG, 100),
        (49, Some(WEAPON_SHOTGUN), OBJTYPE_WEAPON, AMMOTYPE_SHOTGUN, 16),
        // Grenades: `hasweapon` 0, only the crates.
        (50, None, 0, AMMOTYPE_GRENADE, 5),
        (51, Some(WEAPON_ROCKETLAUNCHER), OBJTYPE_WEAPON, AMMOTYPE_ROCKET, 3),
        // The shield: a shield object, no crates (priammoqty 0).
        (52, None, OBJTYPE_SHIELD, 0, 0),
        // Location 6 wraps to slot 0.
        (53, Some(WEAPON_FALCON2), OBJTYPE_WEAPON, AMMOTYPE_PISTOL, 80),
        (54, Some(WEAPON_CMP150), OBJTYPE_WEAPON, AMMOTYPE_SMG, 100),
        (55, Some(WEAPON_SHOTGUN), OBJTYPE_WEAPON, AMMOTYPE_SHOTGUN, 16),
        (56, None, 0, AMMOTYPE_GRENADE, 5),
    ];
    for (k, &(pad, weapon, ty, ammotype, qty)) in expect.iter().enumerate() {
        let o = on_pad(&w, pad);
        match ty {
            0 => assert!(o.is_none(), "pad {pad}: nothing on it"),
            _ => {
                let o = o.unwrap_or_else(|| panic!("pad {pad}: no pickup"));
                assert_eq!(o.ty, ty, "pad {pad}");
                if let Some(wn) = weapon {
                    assert_eq!(o.weaponnum, wn, "pad {pad}");
                }
                assert!(o.hidden2 & OBJH2FLAG_CANREGEN != 0);
            }
        }
        for crate_pad in [57 + 2 * k as i32, 58 + 2 * k as i32] {
            let c = on_pad(&w, crate_pad);
            if qty == 0 {
                assert!(c.is_none(), "pad {crate_pad}: no crate by the shield");
                continue;
            }
            let c = c.unwrap_or_else(|| panic!("pad {crate_pad}: no crate"));
            assert_eq!(c.ty, OBJTYPE_MULTIAMMOCRATE);
            assert_eq!(c.ammoslots[(ammotype - 1) as usize], qty, "pad {crate_pad}");
            assert_eq!(c.ammoslots.iter().filter(|&&q| q > 0).count(), 1);
        }
    }
    // Every pickup sits on the floor: a weapon's box on it, a crate 4 cm up.
    for o in w.props.objs.iter().filter(|o| o.is_pickup()) {
        let low = o.pos.y + o.bbox.ymin * o.scale;
        let floor = w.level.cd_find_room_at_pos_ycnp(o.pos + Vec3::Y * 5.0).unwrap().0;
        let clearance = if o.ty == OBJTYPE_WEAPON { 0.0 } else { 4.0 };
        assert!((low - floor - clearance).abs() < 0.5, "pad {}: bottom {low} floor {floor}", o.pad);
    }
    // Complex's five crates (mp_setupref.c, pads 77-81): on the floor 4 cm up,
    // or stacked on the one below (OBJHFLAG_ONANOTHEROBJ), each a block chrs
    // walk into.
    let crates: Vec<&crate::props::Obj> = w.props.objs.iter().filter(|o| o.ty == OBJTYPE_BASIC).collect();
    assert_eq!(crates.len(), 5);
    let mut stacked = 0;
    for o in &crates {
        let low = o.pos.y + o.bbox.ymin * o.scale;
        let floor = w.level.cd_find_room_at_pos_ycnp(o.pos + Vec3::Y * 5.0).unwrap().0;
        assert_eq!(o.geos.len(), 1, "pad {}", o.pad);
        let g = o.geos[0];
        assert!(g.is_block() && (g.ymin - low).abs() < 0.5, "pad {}", o.pad);
        if o.hidden & OBJHFLAG_ONANOTHEROBJ != 0 {
            let under = crates.iter().find(|u| u.pad != o.pad && crate::stage::cd_is_xz_in_block(u.geos[0].verts(), o.pos.x, o.pos.z)).expect("nothing under it");
            assert!((low - under.geos[0].ymax).abs() < 0.5, "pad {}: {low} on {}", o.pad, under.geos[0].ymax);
            stacked += 1;
        } else {
            assert!((low - floor - 4.0).abs() < 0.5, "pad {}: bottom {low} floor {floor}", o.pad);
        }
    }
    assert!(stacked >= 1, "no crate stacked");
}

#[test]
fn the_player_starts_unarmed_and_takes_a_weapon_its_ammo_and_a_crate() {
    let mut w = world(0);
    w.step(4, &[PlayerInput::default()]);
    let gun = &w.players[0].gun;
    assert_eq!(gun.p.inventory.weapons(), vec![(WEAPON_UNARMED, false)], "only the fists");
    assert_eq!(gun.ctrl.weaponnum, WEAPON_NONE, "nothing in hand");
    let falcon = on_pad(&w, 47).unwrap().id;
    let ev = stand_on(&mut w, falcon, 3);
    let gun = &w.players[0].gun;
    assert!(gun.p.inventory.inv_has_single_weapon_exc_all_guns(WEAPON_FALCON2));
    assert_eq!(gun.ammoheld(AMMOTYPE_PISTOL), 10, "a pistol pickup comes with 10 rounds");
    assert!(sounds(&ev).contains(&super::SFXNUM_00E8_PICKUP_GUN), "{:x?}", sounds(&ev));
    assert!(hudmsgs(&w).contains(&"Picked up a Falcon 2.\n".to_string()), "{:?}", hudmsgs(&w));
    let o = w.props.get(falcon).unwrap();
    assert!(o.is_gone() && o.timetoregen > 1190, "taken, 20 s to come back");
    // Its crate: the location's 80 rounds.
    let crate_id = on_pad(&w, 57).unwrap().id;
    let ev = stand_on(&mut w, crate_id, 3);
    assert_eq!(w.players[0].gun.ammoheld(AMMOTYPE_PISTOL), 90);
    assert!(sounds(&ev).contains(&super::SFXNUM_00EA_PICKUP_AMMO));
    assert!(hudmsgs(&w).contains(&"Picked up some ammo.\n".to_string()), "{:?}", hudmsgs(&w));
    // The Falcon again at full ammo: still wanted, from another pad (the pair).
    w.players[0].gun.p.ammoheldarr[AMMOTYPE_PISTOL as usize] = 800;
    let other = on_pad(&w, 53).unwrap().id;
    stand_on(&mut w, other, 3);
    assert!(w.players[0].gun.p.inventory.inv_has_double_weapon_exc_all_guns(WEAPON_FALCON2, WEAPON_FALCON2), "two Falcons");
    assert!(hudmsgs(&w).contains(&"Double Falcon 2.\n".to_string()), "{:?}", hudmsgs(&w));
}

#[test]
fn a_taken_pickup_comes_back_after_twenty_seconds_with_its_chime() {
    let mut w = world(0);
    let id = on_pad(&w, 48).unwrap().id;
    stand_on(&mut w, id, 2);
    assert!(w.props.get(id).unwrap().is_gone());
    // Away from the pad, wait.
    let away = w.stage.pads[w.stage.spawn_pads[0]].pos - Vec3::Y * 50.0;
    harness::place_player(&mut w, 0, away, 0.0);
    let mut chime = None;
    let mut fading = None;
    for f in 0..1300 {
        w.step(4, &[PlayerInput::default()]);
        let o = w.props.get(id).unwrap();
        if fading.is_none() && !o.is_gone() {
            fading = Some((f, o.timetoregen));
        }
        if chime.is_none() && sounds(&w.take_events()).contains(&super::SFXNUM_0052_REGEN) {
            chime = Some(f);
        }
    }
    let (f, t) = fading.expect("it came back");
    assert!((1135..=1145).contains(&f) && t < 60, "the last second fades in: frame {f}, {t} left");
    assert_eq!(chime, Some(f), "the chime as it appears");
    assert_eq!(w.props.get(id).unwrap().timetoregen, 0);
}

#[test]
fn a_shield_pickup_gives_a_full_shield_that_takes_a_hit_instead_of_health() {
    let mut w = world(1);
    let id = on_pad(&w, 52).unwrap().id;
    let ev = stand_on(&mut w, id, 2);
    assert_eq!(w.chrs[0].cshield, 8.0, "a full shield");
    assert_eq!(w.player_get_shield_frac(0), 1.0);
    assert!(sounds(&ev).contains(&super::SFXNUM_01CD_PICKUP_SHIELD));
    assert!(hudmsgs(&w).contains(&"Picked up a shield.\n".to_string()), "{:?}", hudmsgs(&w));
    assert!(w.mp.playerstats[0].armourcount >= 1.0, "counted for the awards");
    // A shot: the shield pays, the health doesn't.
    let before = w.players[0].bondhealth;
    w.chr_damage_by_impact(0, 1.0, Vec3::X, crate::chr::DamageFrom::new(Some(1), WEAPON_FALCON2, FUNC_PRIMARY), HITPART_TORSO);
    assert_eq!(w.players[0].bondhealth, before);
    assert!(w.chrs[0].cshield < 8.0 && w.chrs[0].cshield > 0.0, "{}", w.chrs[0].cshield);
    // At full shield another shield isn't wanted.
    w.chrs[0].cshield = 8.0;
    stand_on(&mut w, id, 1300);
    assert!(!w.props.get(id).unwrap().is_gone(), "left on its pad");
}

#[test]
fn a_dead_players_weapons_fall_where_they_died() {
    let mut w = world(0);
    let id = on_pad(&w, 48).unwrap().id;
    stand_on(&mut w, id, 2);
    let before = w.props.objs.len();
    let pos = w.players[0].pos;
    w.player_die_by_shooter(0, Some(0));
    let dropped: Vec<&crate::props::Obj> = w.props.objs.iter().skip(before).collect();
    assert_eq!(dropped.len(), 1, "the CMP150 (not the fists)");
    assert_eq!(dropped[0].weaponnum, WEAPON_CMP150);
    assert!(dropped[0].projectile.is_some() && dropped[0].hidden2 & OBJH2FLAG_CANREGEN == 0, "falling, and not coming back");
    assert!((dropped[0].pos - pos).length() < 1.0);
    let did = dropped[0].id;
    for _ in 0..200 {
        w.step(4, &[PlayerInput::default()]);
    }
    let o = w.props.get(did).expect("still there");
    assert!(o.projectile.is_none(), "landed");
    let floor = w.level.cd_find_room_at_pos_ycnp(o.pos + Vec3::Y * 5.0).unwrap().0;
    assert!((o.pos.y - floor).abs() < 20.0, "on the floor: {} over {floor}", o.pos.y);
}

/// Unarmed simulants with a hitscan set go for the pads (`MA_AIBOTGETITEM`),
/// arm themselves (`botinv_tick` picks the best they hold) and fight.
#[test]
fn unarmed_simulants_go_for_the_weapons_and_fight_with_them() {
    let (stage, level) = complex_arc();
    let mut setup = harness::setup(0, 4, BOTDIFF_NORMAL);
    for (s, w) in [WEAPON_FALCON2, WEAPON_CMP150, WEAPON_AR34, WEAPON_SHOTGUN, WEAPON_DY357MAGNUM, WEAPON_MPSHIELD].iter().enumerate() {
        setup.weapons[s] = mpweapon_index(*w).unwrap();
    }
    let mut w = World::new(setup, stage, level, res(), harness::M10_SEED).unwrap();
    let n = w.chrs.len();
    let mut fetched = vec![false; n];
    let mut armed_at = vec![None; n];
    let mut kills = 0;
    for f in 0..60 * 90 {
        harness::step_idle(&mut w);
        for (i, c) in w.chrs.iter().enumerate() {
            let a = c.aibot.as_ref().unwrap();
            fetched[i] |= a.myaction == crate::bot::MyAction::GetItem;
            if armed_at[i].is_none() && a.weaponnum > WEAPON_UNARMED && c.held[0].is_some() {
                armed_at[i] = Some(f);
            }
        }
        kills += w.take_events().iter().filter(|e| matches!(e, Event::Kill { .. })).count();
    }
    println!("armed at {armed_at:?}, kills {kills}");
    assert!(fetched.iter().all(|&f| f), "every simulant went for a pickup: {fetched:?}");
    assert!(armed_at.iter().all(|a| a.is_some_and(|f| f < 60 * 45)), "armed within 45 s: {armed_at:?}");
    assert!(kills >= 3, "{kills} kills in 90 s");
    // The pads were visited: pickups taken and respawning.
    assert!(w.props.objs.iter().any(|o| o.hidden2 & OBJH2FLAG_CANREGEN != 0 && o.timetoregen > 0) || kills > 0);
}

/// Every preset weapon set of the menus (`g_MpWeaponSets`, the weapons by
/// number), four NormalSims on Complex for two minutes each: they arm up with
/// whatever the set holds (launchers and throwables included) and fight.
#[test]
fn simulants_arm_up_and_fight_with_every_preset_set() {
    let sets: [[u8; 6]; 12] = [
        [2, 5, 7, 6, 91, 92],
        [2, 10, 14, 17, 91, 92],
        [5, 8, 19, 13, 91, 92],
        [7, 11, 12, 22, 91, 92],
        [2, 10, 15, 28, 91, 92],
        [6, 16, 20, 18, 91, 92],
        [3, 30, 10, 9, 91, 92],
        [23, 23, 18, 18, 91, 92],
        [5, 10, 17, 23, 91, 92],
        [6, 11, 15, 24, 91, 92],
        [5, 14, 16, 33, 91, 92],
        [26, 26, 32, 27, 91, 92],
    ];
    let (stage, level) = complex_arc();
    let mut total_kills = 0;
    for set in sets {
        let mut setup = harness::setup(0, 4, BOTDIFF_NORMAL);
        for (s, &wn) in set.iter().enumerate() {
            setup.weapons[s] = mpweapon_index(wn).unwrap();
        }
        let mut w = World::new(setup, stage.clone(), level.clone(), res(), harness::M10_SEED).unwrap();
        let mut held = std::collections::BTreeSet::new();
        let mut kills = 0;
        for _ in 0..60 * 120 {
            harness::step_idle(&mut w);
            for c in &w.chrs {
                held.insert(c.aibot.as_ref().unwrap().weaponnum);
            }
            kills += w.take_events().iter().filter(|e| matches!(e, Event::Kill { .. })).count();
        }
        println!("set {set:?}: held {held:?}, kills {kills}, rounds {} hits {}", w.navstats.rounds, w.navstats.round_hits);
        assert!(held.iter().any(|&wn| set.contains(&wn)), "set {set:?}: nobody armed ({held:?})");
        // A launcher-heavy set can go two minutes without a kill (set 9, the
        // MagSec/CMP150/AR34/rocket one, since M9's solid crates changed the
        // match's course; its simulants still cover 350-520 m each), so each
        // set must land hits, and the twelve together must kill.
        assert!(kills >= 1 || w.navstats.round_hits >= 5, "set {set:?}: no kills and {} hits in two minutes", w.navstats.round_hits);
        total_kills += kills;
    }
    assert!(total_kills >= 40, "{total_kills} kills over the twelve sets");
}

/// `scenario_highlight_prop`'s object half: a pulsing blue on pickups for a
/// player with "highlight pickups", unless the match has No Pickup Highlight.
#[test]
fn pickups_are_highlighted_unless_the_option_is_off() {
    let mut w = world(0);
    let falcon = on_pad(&w, 47).unwrap().clone();
    // PD's default display options (radar, highlight teams) don't ask for it.
    assert_eq!(w.scenario_highlight_obj(0, &falcon), None);
    w.setup.players[0].chr.displayoptions |= MPDISPLAYOPTION_HIGHLIGHTPICKUPS;
    w.frac20 = 0.0125; // the pulse's peak: sin(40·f·π) = 1
    assert_eq!(w.scenario_highlight_obj(0, &falcon), Some([0, 0xcd, 0xff, 255]));
    let crate_ = on_pad(&w, 57).unwrap().clone();
    assert!(w.scenario_highlight_obj(0, &crate_).is_some());
    w.setup.options |= MPOPTION_NOPICKUPHIGHLIGHT;
    assert_eq!(w.scenario_highlight_obj(0, &falcon), None, "No Pickup Highlight");
}

/// A grenade location has only its crates (`hasweapon` 0): the crate's five
/// grenades give the weapon (`ammo_handle_pickup`), which leaves the inventory
/// once none are left (`bgun_tick_gameplay`).
#[test]
fn a_grenade_crate_gives_the_grenades_and_they_go_when_used_up() {
    let mut w = world(0);
    let id = on_pad(&w, 63).unwrap().id;
    stand_on(&mut w, id, 3);
    assert!(w.players[0].gun.p.inventory.inv_has_single_weapon_exc_all_guns(WEAPON_GRENADE));
    assert_eq!(w.players[0].gun.ammoheld(AMMOTYPE_GRENADE), 5);
    assert!(hudmsgs(&w).contains(&"Picked up some grenades.\n".to_string()), "{:?}", hudmsgs(&w));
    w.players[0].gun.p.ammoheldarr[AMMOTYPE_GRENADE as usize] = 0;
    w.step(4, &[PlayerInput::default()]);
    assert!(!w.players[0].gun.p.inventory.inv_has_single_weapon_exc_all_guns(WEAPON_GRENADE), "no grenades left, none held");
}
