//! The guns' objects, headless in the firing range: the old repo's
//! `pd_guns/tests.rs` M5 tests (thrown and fired projectiles, mines, the N-Bomb,
//! the Laptop sentry, the Slayer, the Farsight's x-ray, the boost and the cloak)
//! on a `World`, plus what the port adds: shots and blasts setting explosives
//! off, and a sentry targeting players the way a Combat Simulator one does.

use glam::Vec3;
use pd_core::events::Event;
use pd_core::ids::*;

use super::{func0f06e9cc, Obj};
use pd_core::math;
use crate::player::PlayerInput;
use crate::testutil::*;
use crate::world::World;

fn objs(w: &World, weaponnum: u8) -> Vec<&Obj> {
    w.props.objs.iter().filter(|o| o.weaponnum == weaponnum && !o.is_deleting()).collect()
}

fn exploding(w: &World, ty: usize) -> bool {
    w.explosions.slots.iter().flatten().any(|e| e.ty == ty)
}

fn handle_sounds(events: &[Event]) -> Vec<(u32, u16)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::HandleSound { handle, sound, .. } => Some((*handle, *sound)),
            _ => None,
        })
        .collect()
}

/// `bgun_create_thrown_projectile` → an airborne sticky grenade with the fuse
/// less the wind-up (`primetimer60`), bouncing, a forced first hop,
/// `projectile_settle`, then `prop_explode` at 0: a rocket-sized blast with a
/// scorch on the floor.
#[test]
fn grenade_is_thrown_bounces_settles_and_explodes_on_its_fuse() {
    let mut w = settled(WEAPON_GRENADE);
    throw_once(&mut w);
    let (mut made, mut max_bounces, mut settled_, mut exploded) = (None, 0, false, None);
    for f in 0..400 {
        idle(&mut w, 1);
        if let Some(o) = objs(&w, WEAPON_GRENADE).first() {
            if made.is_none() {
                made = Some((f, o.timer240));
            }
            match &o.projectile {
                Some(p) => max_bounces = max_bounces.max(p.bouncecount),
                None => settled_ = true,
            }
        }
        if exploded.is_none() && exploding(&w, EXPLOSIONTYPE_ROCKET) {
            exploded = Some(f);
        }
    }
    let (made_at, fuse) = made.expect("no grenade thrown");
    assert!(fuse > 0 && fuse < 960, "the 4 s fuse less the wind-up: {fuse}");
    assert!(max_bounces >= 1, "it should bounce");
    assert!(settled_, "it should come to rest");
    let exploded = exploded.expect("never exploded");
    let expect = made_at + fuse / 4 + 1;
    assert!((exploded - expect).abs() <= 2, "the blast at {exploded}, the fuse said ~{expect}");
    assert!(objs(&w, WEAPON_GRENADE).is_empty(), "the grenade is gone");
    assert!(w.fx.wallhits.iter().any(|h| h.texnum == WALLHITTEX_SCORCH), "no scorch on the floor");
}

/// Holding the trigger past the 4 s fuse: the grenade leaves the hand at 0
/// (`HANDSTATEMINOR_ATTACK_THROW_GRENADEWAIT`) and blows up on the player,
/// who dies; after the fades, A starts a new life with the hand back.
#[test]
fn cooked_grenade_blows_up_in_the_hand() {
    let mut w = settled(WEAPON_GRENADE);
    let fire = PlayerInput { fire: true, ..Default::default() };
    let mut saw_wait = false;
    for _ in 0..300 {
        run(&mut w, &fire, 1);
        saw_wait |= w.players[0].gun.hands[HAND_RIGHT].stateminor == HANDSTATEMINOR_ATTACK_THROW_GRENADEWAIT;
    }
    assert!(saw_wait, "never entered GRENADEWAIT");
    assert!(w.players[0].isdead, "the blast should kill the player: {}", w.players[0].bondhealth);
    let press = PlayerInput { a_held: true, ..Default::default() };
    let mut respawned = false;
    for _ in 0..600 {
        run(&mut w, &press, 1);
        if !w.players[0].isdead {
            respawned = true;
            break;
        }
    }
    assert!(respawned, "A never started a new life");
    let mut idle_at = None;
    for f in 0..600 {
        idle(&mut w, 1);
        if idle_at.is_none() && w.players[0].gun.hands[HAND_RIGHT].state == HANDSTATE_IDLE {
            idle_at = Some(f);
        }
    }
    assert!(idle_at.is_some(), "the hand never recovered");
}

/// A timed mine thrown at the west wall sticks standing on its normal
/// (`obj_stick_default`), goes off on its timer and scorches the wall it was on
/// (`prop_explode`'s attached branch). Landing, it sounds like a mine
/// (`SFXMAP_80AA`), not a ricochet.
#[test]
fn timed_mine_sticks_to_the_wall_and_scorches_it() {
    let mut w = settled(WEAPON_TIMEDMINE);
    w.players[0].theta = 90.0; // facing -x, the west wall 6 m away
    idle(&mut w, 2);
    w.take_events();
    throw_once(&mut w);
    let mut stuck = None;
    let mut heard = Vec::new();
    for f in 0..300 {
        idle(&mut w, 1);
        heard.extend(sounds(&w.take_events()));
        if stuck.is_none() {
            if let Some(o) = objs(&w, WEAPON_TIMEDMINE).first() {
                if o.has(OBJHFLAG_ATTACHED) {
                    stuck = Some((f, o.pos, o.realrot.y_axis.normalize()));
                }
            }
        }
    }
    let (_, pos, up) = stuck.expect("the mine never stuck");
    assert!(pos.x < -560.0, "stuck on the west wall: {pos}");
    assert!(up.x > 0.95, "stood on the wall's normal (+x): {up}");
    assert!(heard.contains(&0x80aa), "the mine landing: {heard:x?}");
    assert!(!heard.iter().any(|s| (0x13..=0x2a).contains(s)), "no ricochet");
    assert!(objs(&w, WEAPON_TIMEDMINE).is_empty(), "the timer should have fired");
    let scorch = w.fx.wallhits.iter().find(|h| h.texnum == WALLHITTEX_SCORCH).expect("no scorch");
    assert!(scorch.corners.iter().all(|c| (c.x + 600.0).abs() < 1.0), "the scorch lies on the wall: {:?}", scorch.corners);
}

/// A remote mine waits; B + fire presses the detonator (`HANDATTACKTYPE_DETONATE`
/// → `g_PlayersDetonatingMines`) with its click, and it goes up.
#[test]
fn remote_mine_waits_for_the_detonator() {
    let mut w = settled(WEAPON_REMOTEMINE);
    w.players[0].theta = 90.0;
    idle(&mut w, 2);
    throw_once(&mut w);
    idle(&mut w, 400);
    assert_eq!(objs(&w, WEAPON_REMOTEMINE).len(), 1, "a remote mine does not go off by itself");
    assert!(objs(&w, WEAPON_REMOTEMINE)[0].has(OBJHFLAG_ATTACHED));
    let hold = PlayerInput { use_held: true, ..Default::default() };
    run(&mut w, &hold, 30);
    w.take_events();
    let mut clicked = false;
    for _ in 0..4 {
        run(&mut w, &PlayerInput { use_held: true, fire: true, ..Default::default() }, 1);
        clicked |= sounds(&w.take_events()).contains(&0x80ab);
    }
    run(&mut w, &hold, 4);
    assert!(clicked, "no detonator click");
    assert!(objs(&w, WEAPON_REMOTEMINE).is_empty(), "the detonator should have set it off");
    assert!(w.explosions.live() > 0);
}

/// A proximity mine arms after 4 s, then a player within 2.5 m sets it off,
/// its thrower included, as in PD.
#[test]
fn proximity_mine_arms_then_takes_the_player_who_walks_up() {
    let mut w = settled(WEAPON_PROXIMITYMINE);
    w.players[0].verta = -45.0;
    throw_once(&mut w);
    idle(&mut w, 300);
    let o = *objs(&w, WEAPON_PROXIMITYMINE).first().expect("no mine");
    assert_eq!(o.timer240, 1, "armed after 4 s");
    let at = o.pos;
    assert!((at - w.players[0].pos).length() > 250.0, "landed out of reach: {at}");
    w.players[0].pos.x = at.x;
    w.players[0].pos.z = at.z - 150.0;
    idle(&mut w, 3);
    assert!(objs(&w, WEAPON_PROXIMITYMINE).is_empty(), "it should go off");
    assert!(w.players[0].bondhealth < 1.0);
}

/// The throwing knife (secondary) flies true and sticks in the first board,
/// which counts it (PD scores it in the firing range).
#[test]
fn thrown_knife_sticks_in_the_board() {
    let mut w = settled(WEAPON_COMBATKNIFE);
    secondary(&mut w);
    assert_eq!(w.players[0].gun.hands[0].weaponfunc, FUNC_SECONDARY, "B held should switch to the throw");
    w.players[0].verta = -3.0;
    throw_once(&mut w);
    idle(&mut w, 60);
    let knife = *objs(&w, WEAPON_COMBATKNIFE).first().expect("no knife");
    assert_eq!(knife.embedded, Some(super::Embed::Board(0)), "stuck in the first board: {}", knife.pos);
    assert!(w.boards[0].hits >= 1);
}

/// N-Bomb: an impact detonation → `nbomb_create_storm`; a storm reaches 500 cm
/// at 80 ticks, darkens the room, hums, and is gone after 370 ticks.
#[test]
fn nbomb_storm_grows_darkens_hums_and_fades() {
    let mut w = settled(WEAPON_NBOMB);
    throw_once(&mut w);
    let mut born = false;
    for _ in 0..200 {
        idle(&mut w, 1);
        born |= w.props.nbombs.any();
    }
    assert!(born, "no storm");
    let mut w = settled(WEAPON_FALCON2);
    w.take_events();
    let pos = w.players[0].pos + Vec3::new(0.0, 0.0, 400.0);
    w.nbomb_create_storm(pos, Some(0));
    idle(&mut w, 80);
    let n = *w.props.nbombs.bombs.iter().find(|n| n.age240 >= 0).unwrap();
    assert!((n.radius - 500.0).abs() < 30.0, "the radius at 80 ticks: {}", n.radius);
    let room = w.level.floor_room(pos, 1.0).unwrap();
    assert!(w.lights.room(room).br_flash < -100, "the storm darkens the room: {}", w.lights.room(room).br_flash);
    let hs = handle_sounds(&w.take_events());
    assert!(hs.iter().any(|&(h, s)| h == super::nbomb::NBOMB_HUM_HANDLE && s == 0x810c), "the hum: {hs:x?}");
    assert!(hs.iter().filter(|&&(_, s)| s == 0x0001).count() == 2, "two roars: {hs:x?}");
    idle(&mut w, 300);
    assert!(!w.props.nbombs.any(), "gone after 370 ticks");
    assert!(w.take_events().iter().any(|e| matches!(e, Event::StopSound { handle } if *handle == super::nbomb::NBOMB_HUM_HANDLE)), "the hum stops");
    assert!(w.players[0].bondhealth < 1.0, "standing inside, the player was hurt");
}

/// The launcher shows its rocket (`bgun_create_held_rocket`, at the muzzle);
/// firing turns that same object into a powered projectile ×2.1 with a smoke
/// trail that blows up on what it hits, and the next rocket loads.
#[test]
fn rocket_launcher_holds_its_rocket_fires_it_and_it_blows_on_impact() {
    let mut w = settled(WEAPON_ROCKETLAUNCHER);
    let held = w.players[0].gun.hands[0].rocket.expect("no rocket in the launcher");
    let o = w.props.get(held).unwrap();
    assert!(o.flags & OBJFLAG_HELDROCKET != 0 && o.flags2 & OBJFLAG2_THROWTHROUGH != 0);
    assert!((o.pos - w.players[0].gun.hands[0].muzzlepos).length() < 0.01, "it sits at the muzzle");
    let modelscale = o.scale;
    fire_once(&mut w);
    idle(&mut w, 2);
    let o = w.props.get(held).expect("the held rocket becomes the projectile");
    assert!(o.flags & OBJFLAG_HELDROCKET == 0);
    let p = o.projectile.as_ref().expect("flying");
    assert!(p.flags & PROJECTILEFLAG_POWERED != 0, "no gravity");
    assert!((o.scale - modelscale * 2.1).abs() < 1e-4, "×2.1 in flight");
    let (mut trail, mut boom) = (false, false);
    for _ in 0..200 {
        idle(&mut w, 1);
        trail |= w.fx.smokes.slots.iter().flatten().any(|s| s.ty == SMOKETYPE_ROCKETTAIL);
        boom |= exploding(&w, EXPLOSIONTYPE_ROCKET);
    }
    assert!(trail, "no rocket trail");
    assert!(boom, "no blast on impact");
    assert!(w.props.get(held).is_none(), "the rocket is gone");
    assert!(w.players[0].gun.hands[0].rocket.is_some(), "the next rocket is loaded");
}

/// Switching away from the launcher frees its loaded rocket for good
/// (`bgun_free_weapon`, `bondgun.c:5262`), so it isn't drawn on the next gun.
#[test]
fn switching_off_the_launcher_leaves_no_rocket_on_the_next_gun() {
    let mut w = settled(WEAPON_ROCKETLAUNCHER);
    assert!(w.players[0].gun.hands[0].rocket.is_some());
    run(&mut w, &PlayerInput { select: Some((WEAPON_GRENADE, false)), ..Default::default() }, 1);
    idle(&mut w, 200);
    assert_eq!(w.players[0].gun.ctrl.weaponnum, WEAPON_GRENADE);
    assert!(w.props.objs.is_empty() && w.players[0].gun.hands[0].rocket.is_none());
}

/// The Devastator's grenade round arcs and goes off when it lands.
#[test]
fn devastator_round_goes_off_when_it_lands() {
    let mut w = settled(WEAPON_DEVASTATOR);
    w.players[0].verta = -10.0;
    fire_once(&mut w);
    idle(&mut w, 2);
    assert!(objs(&w, WEAPON_GRENADEROUND).iter().any(|o| o.projectile.is_some()), "no round in flight");
    let mut boom = None;
    for f in 0..200 {
        idle(&mut w, 1);
        if boom.is_none() && exploding(&w, EXPLOSIONTYPE_ROCKET) {
            boom = Some(f);
        }
    }
    let f = boom.expect("the round never went off");
    assert!(f < 60, "on landing, not on its 20 s timer: {f}");
}

/// The Devastator's wall hugger sticks to the wall, holds 480 quarter-ticks,
/// drops and goes off on the floor.
#[test]
fn wall_hugger_sticks_drops_and_blows() {
    let mut w = settled(WEAPON_DEVASTATOR);
    secondary(&mut w);
    assert_eq!(w.players[0].gun.hands[0].weaponfunc, FUNC_SECONDARY);
    w.players[0].theta = 270.0; // facing +x, the east wall 6 m away
    idle(&mut w, 2);
    fire_once(&mut w);
    let (mut stuck_at, mut dropped_at, mut blew_at) = (None, None, None);
    for f in 0..400 {
        idle(&mut w, 1);
        match objs(&w, WEAPON_GRENADEROUND).first() {
            Some(o) if o.has(OBJHFLAG_ATTACHED) && stuck_at.is_none() => stuck_at = Some(f),
            Some(o) if stuck_at.is_some() && !o.has(OBJHFLAG_ATTACHED) && dropped_at.is_none() => dropped_at = Some(f),
            None if stuck_at.is_some() && blew_at.is_none() => blew_at = Some(f),
            _ => {}
        }
    }
    let s = stuck_at.expect("never stuck");
    let d = dropped_at.expect("never dropped");
    let b = blew_at.expect("never blew");
    assert!((110..=130).contains(&(d - s)), "held ~480 quarter-ticks: {}", d - s);
    assert!(b > d, "it blows after it drops");
}

/// The crossbow's bolt sticks in the first board and quivers (timer240 13 → 1),
/// its trail following it in flight, then shrinking into it and gone.
#[test]
fn crossbow_bolt_sticks_and_quivers() {
    use crate::fx::boltbeam::BoltOwner;
    let mut w = settled(WEAPON_CROSSBOW);
    fire_once(&mut w);
    let mut n = 0;
    while objs(&w, WEAPON_BOLT).is_empty() && n < 10 {
        idle(&mut w, 1);
        n += 1;
    }
    let id = objs(&w, WEAPON_BOLT).first().expect("no bolt").id;
    let b = w.fx.boltbeams.find(BoltOwner::Prop(id)).expect("a trail");
    idle(&mut w, 2);
    let (bolt, beam) = (objs(&w, WEAPON_BOLT)[0].pos, w.fx.boltbeams.beams[b]);
    assert_eq!(beam.tailpos, bolt, "the tail follows the bolt");
    assert!((beam.tailpos - beam.headpos).length() > 10.0, "it stretches back to where the bolt left");
    let mut quiver = Vec::new();
    let mut shrinking = false;
    for _ in 0..60 {
        idle(&mut w, 1);
        if let Some(o) = objs(&w, WEAPON_BOLT).into_iter().find(|o| o.has(OBJHFLAG_ATTACHED)) {
            quiver.push(o.timer240);
            shrinking |= w.fx.boltbeams.beams[b].automatic;
        }
    }
    assert!(shrinking, "stuck, the trail lets go and shrinks");
    assert_eq!(w.fx.boltbeams.live().count(), 0, "and is gone within a second");
    let bolt = *objs(&w, WEAPON_BOLT).first().expect("no bolt");
    assert!(bolt.has(OBJHFLAG_ATTACHED), "stuck");
    assert!(w.boards[0].hits >= 1, "the board counts it");
    assert!(quiver.first().copied().unwrap_or(0) >= 12 && *quiver.last().unwrap() == 1, "the quiver ran 13 → 1: {quiver:?}");
}

/// The Slayer's fly-by-wire: the camera rides the rocket, the stick turns it,
/// Jo stands still, the hum loops, fire blows it, and the view comes back
/// through a frame of static.
#[test]
fn slayer_fly_by_wire_rides_steers_and_blows() {
    let mut w = settled(WEAPON_SLAYER);
    secondary(&mut w);
    w.take_events();
    fire_once(&mut w);
    idle(&mut w, 30);
    assert_eq!(w.players[0].visionmode, VISIONMODE_SLAYERROCKET);
    let hs = handle_sounds(&w.take_events());
    assert!(hs.iter().any(|&(_, s)| s == 0x8068), "the rocket hum: {hs:x?}");
    let id = w.players[0].slayerrocket.expect("no rocket to ride");
    let rocket = w.props.get(id).unwrap();
    let cam = w.players[0].cam.pos();
    assert!((cam - rocket.pos).length() < 20.0, "the camera is on the rocket: {cam} vs {}", rocket.pos);
    assert!(w.players[0].viewfx.slayer_interlace, "the rocket's interlace");
    let heading0 = rocket.projectile.as_ref().unwrap().speed.normalize();
    let jo = w.players[0].pos;
    run(&mut w, &PlayerInput { walk_x: 127, ..Default::default() }, 40);
    let rocket = w.props.get(id).unwrap();
    let heading1 = rocket.projectile.as_ref().unwrap().speed.normalize();
    assert!(heading0.dot(heading1) < 0.99, "the stick turns it: {heading0} → {heading1}");
    assert!((w.players[0].pos - jo).length() < 0.01, "Jo stands still while riding");
    fire_once(&mut w);
    let static_seen = w.players[0].viewfx.static_alpha == 255 || {
        idle(&mut w, 1);
        w.players[0].viewfx.static_alpha == 255
    };
    assert!(static_seen, "the signal is lost: static");
    idle(&mut w, 1);
    assert_eq!(w.players[0].visionmode, VISIONMODE_NORMAL);
    assert!(w.explosions.live() > 0);
}

/// The SuperDragon's secondary: its grenade round (gunfunc FUNC_2) goes off as
/// EXPLOSIONTYPE_SDGRENADE.
#[test]
fn superdragon_grenade_uses_the_sdgrenade_blast() {
    let mut w = settled(WEAPON_SUPERDRAGON);
    secondary(&mut w);
    w.players[0].verta = -12.0;
    fire_once(&mut w);
    let mut boom = false;
    for _ in 0..200 {
        idle(&mut w, 1);
        boom |= exploding(&w, EXPLOSIONTYPE_SDGRENADE);
    }
    assert!(boom, "no SDGRENADE blast");
}

/// A thrown grenade is heard leaving the hand (`SFXMAP_80A9_THROW`) and going
/// off at full volume a few metres away (its audio config's 25 m).
#[test]
fn grenade_throw_and_blast_are_audible() {
    let mut w = settled(WEAPON_GRENADE);
    w.take_events();
    let mut ev = Vec::new();
    throw_once(&mut w);
    ev.extend(w.take_events());
    for _ in 0..300 {
        idle(&mut w, 1);
        ev.extend(w.take_events());
    }
    let vol = |id: u16| ev.iter().find_map(|e| match e {
        Event::Sound { sound, volume, .. } if *sound == id => Some(*volume),
        _ => None,
    });
    assert!(vol(0x80a9).expect("no throw sound") > 0.99);
    assert!(vol(0x809f).expect("no blast sound") > 0.99);
}

/// Shooting a proximity mine sets it off (`obj_hit` → `obj_damage_by_gunfire`
/// cuts the fuse).
#[test]
fn a_shot_mine_goes_off() {
    let mut w = settled(WEAPON_PROXIMITYMINE);
    w.players[0].verta = -45.0;
    throw_once(&mut w);
    idle(&mut w, 300);
    let at = objs(&w, WEAPON_PROXIMITYMINE).first().expect("no mine").pos;
    run(&mut w, &PlayerInput { select: Some((WEAPON_FALCON2, false)), ..Default::default() }, 1);
    idle(&mut w, 200);
    // Aim the eye at the mine.
    let d = at - w.players[0].pos;
    w.players[0].theta = (-d.x).atan2(d.z).to_degrees().rem_euclid(360.0);
    w.players[0].verta = (d.y / (d.x * d.x + d.z * d.z).sqrt()).atan().to_degrees();
    idle(&mut w, 2);
    for _ in 0..10 {
        if objs(&w, WEAPON_PROXIMITYMINE).is_empty() {
            break;
        }
        fire_once(&mut w);
        idle(&mut w, 8);
    }
    assert!(objs(&w, WEAPON_PROXIMITYMINE).is_empty(), "the shot mine should go off");
    assert!(exploding(&w, EXPLOSIONTYPE_ROCKET) || w.fx.wallhits.iter().any(|h| h.texnum == WALLHITTEX_SCORCH));
}

/// A blast sets off a remote mine beside it (`explosion_inflict_damage` →
/// `obj_damage_by_explosion(WEAPON_REMOTEMINE)`).
#[test]
fn a_blast_sets_off_a_mine_beside_it() {
    let mut w = settled(WEAPON_REMOTEMINE);
    w.players[0].theta = 90.0;
    idle(&mut w, 2);
    throw_once(&mut w);
    idle(&mut w, 200);
    let at = objs(&w, WEAPON_REMOTEMINE).first().expect("no mine").pos;
    assert!(w.explosion_create_simple(0, at + Vec3::new(80.0, 0.0, 0.0), EXPLOSIONTYPE_ROCKET));
    idle(&mut w, 10);
    assert!(objs(&w, WEAPON_REMOTEMINE).is_empty(), "the blast should set the mine off");
}

/// The Laptop Gun's B + fire: FUNCFLAG_DISCARDWEAPON takes it out of the
/// inventory, it is thrown as a sentry (`laptop_deploy`) and lands; a Combat
/// Simulator sentry tries the other players round-robin and shoots the one it
/// sees until its rounds are gone. A second deploy blows up the first.
#[test]
fn laptop_deploys_as_a_sentry_and_shoots_the_other_player() {
    let mut w = range_players(2);
    // Player 1 stands 6 m down the hall.
    w.players[1].pos = w.players[0].pos + Vec3::new(0.0, 0.0, 600.0);
    let setup = |w: &mut World, i: PlayerInput| run_all(w, &[i, PlayerInput::default()], 1);
    setup(&mut w, PlayerInput { select: Some((WEAPON_LAPTOPGUN, false)), ..Default::default() });
    for _ in 0..200 {
        setup(&mut w, PlayerInput::default());
    }
    w.players[0].verta = -30.0;
    for _ in 0..30 {
        setup(&mut w, PlayerInput { use_held: true, ..Default::default() });
    }
    for _ in 0..4 {
        setup(&mut w, PlayerInput { use_held: true, fire: true, ..Default::default() });
    }
    for _ in 0..200 {
        setup(&mut w, PlayerInput::default());
    }
    assert!(!w.players[0].gun.p.inventory.iter().any(|(wn, _)| *wn == WEAPON_LAPTOPGUN), "the Laptop left the inventory");
    let sentry = w.props.objs.iter().find(|o| o.ty == OBJTYPE_AUTOGUN).expect("no sentry");
    assert_eq!(sentry.owner(), 0);
    assert!(sentry.projectile.is_none(), "it landed");
    assert!(w.players[1].bondhealth < 1.0, "it shot player 1: {}", w.players[1].bondhealth);
    assert_eq!(w.players[0].bondhealth, 1.0, "never its owner");
    for _ in 0..600 {
        setup(&mut w, PlayerInput::default());
    }
    // A dead chr is no target: it stops with rounds left.
    assert!(w.players[1].isdead, "player 1 died");
    let a = w.props.objs.iter().find(|o| o.ty == OBJTYPE_AUTOGUN).unwrap().autogun.as_ref().unwrap();
    assert!(a.ammoquantity > 0, "it stopped once its target died");
    // A second deploy (M8: the pickup; here the loadout again).
    w.give_loadout(0);
    setup(&mut w, PlayerInput { select: Some((WEAPON_LAPTOPGUN, false)), ..Default::default() });
    for _ in 0..200 {
        setup(&mut w, PlayerInput::default());
    }
    for _ in 0..30 {
        setup(&mut w, PlayerInput { use_held: true, ..Default::default() });
    }
    let mut blew = false;
    for f in 0..200 {
        let fire = f < 4;
        setup(&mut w, PlayerInput { use_held: fire, fire, ..Default::default() });
        blew |= exploding(&w, EXPLOSIONTYPE_LAPTOP);
    }
    assert!(blew, "the old sentry should blow up");
    assert_eq!(w.props.objs.iter().filter(|o| o.ty == OBJTYPE_AUTOGUN).count(), 1);
}

/// Aiming the Farsight turns on x-ray (`bondgun.c:8007`): the eraser time counts
/// from 0, the smear starts at 249/255 and settles at 99/255 after 200 ticks
/// (`lv.c:1462`), the eraser sits 5 m ahead (÷ `c_lodscalez` while aimed) and
/// zooming pushes it out; letting go turns it off.
#[test]
fn farsight_aim_turns_on_xray_with_the_smear_and_the_eraser_follows_the_zoom() {
    let mut w = settled(WEAPON_FARSIGHT);
    assert_eq!(w.players[0].visionmode, VISIONMODE_NORMAL);
    let aim = PlayerInput { aim: true, ..Default::default() };
    run(&mut w, &aim, 1);
    assert_eq!(w.players[0].visionmode, VISIONMODE_XRAY);
    assert_eq!(w.players[0].eraser.time, 0);
    assert_eq!(w.players[0].viewfx.zoom_blurs.first(), Some(&(249, 1.05, 1.05)));
    run(&mut w, &aim, 1);
    let p = &w.players[0];
    let ahead = p.eraser.pos - p.cam.pos();
    let want = 500.0 / p.cam.c_lodscalez;
    assert!((ahead.length() - want).abs() < 1.0, "{want} ahead: {ahead}");
    run(&mut w, &aim, 60);
    assert_eq!(w.players[0].eraser.time, 244);
    assert_eq!(w.players[0].viewfx.zoom_blurs.first(), Some(&(99, 1.05, 1.05)));
    let before = (w.players[0].eraser.pos - w.players[0].cam.pos()).length();
    run(&mut w, &PlayerInput { aim: true, zoom_in: true, ..Default::default() }, 120);
    let far = (w.players[0].eraser.pos - w.players[0].cam.pos()).length();
    assert!(far > before * 2.0, "the eraser rides the zoom: {before} → {far}");
    idle(&mut w, 1);
    assert_eq!(w.players[0].visionmode, VISIONMODE_NORMAL);
    assert!(w.players[0].viewfx.zoom_blurs.is_empty());
}

/// In x-ray the Farsight's round doesn't test the BG at all (`prop.c:688`): the
/// wall between Jo and the board takes no sparks, and the board still scores.
#[test]
fn farsight_in_xray_goes_through_the_wall_without_sparks() {
    let mut g = crate::stage::fixtures::firing_range();
    crate::stage::fixtures::add_box(&mut g, Vec3::new(-150.0, 0.0, 150.0), Vec3::new(150.0, 400.0, 170.0));
    let mut w = range_with(g);
    run(&mut w, &PlayerInput { select: Some((WEAPON_FARSIGHT, false)), ..Default::default() }, 1);
    idle(&mut w, 200);
    let aim = PlayerInput { aim: true, ..Default::default() };
    run(&mut w, &aim, 20);
    assert_eq!(w.players[0].visionmode, VISIONMODE_XRAY);
    let sparks0 = w.fx.sparks.live();
    run(&mut w, &PlayerInput { fire: true, ..aim.clone() }, 1);
    let mut sparked = false;
    for _ in 0..60 {
        run(&mut w, &aim, 1);
        sparked |= w.fx.sparks.live() > sparks0;
    }
    assert!(w.boards[0].hits >= 1, "through the wall to the board");
    assert!(!sparked, "no sparks off a wall the round never met");
}

/// Combat Boost (`bgun_apply_boost` → `bgun_add_boost`, `lv.c:1478`,
/// `lv.c:2061`): a pill buys 10 s; the wipe's white fade peaks at step 15 where
/// `speedpillon` flips; while on, a 20 Hz frame is capped to 4 quarter-ticks for
/// the whole world and the heartbeat loops; when it runs out it wipes back off
/// with Jo's groan and full speed returns.
#[test]
fn combat_boost_slows_the_whole_world_with_a_wipe_and_wears_off() {
    let mut w = settled(WEAPON_COMBATBOOST);
    w.take_events();
    let loaded = w.players[0].gun.hands[0].loadedammo[0];
    w.step(12, &[PlayerInput { fire: true, ..Default::default() }]);
    let mut peak = 0.0f32;
    let mut ev = Vec::new();
    for _ in 0..40 {
        w.step(12, &[PlayerInput::default()]);
        peak = peak.max(w.players[0].viewfx.fade.map_or(0.0, |f| f.1));
        ev.extend(w.take_events());
    }
    assert!(w.speedpill.on && w.speedpill.change == 30, "{:?}", w.speedpill);
    assert_eq!(w.players[0].gun.hands[0].loadedammo[0], loaded - 1, "one pill used");
    assert!((peak - 15.0 * 0.006_666_667).abs() < 1e-4, "the white fade peaks at 0.1: {peak}");
    assert_eq!(w.lv.lvupdate240, 4, "20 Hz frames run at a third speed");
    assert!(sounds(&ev).contains(&0x05c9), "Jo: boost activate");
    assert!(handle_sounds(&ev).contains(&(crate::gun::boost::MISC_SFX_HANDLE + MISCSFX_BOOSTHEARTBEAT as u32, 0x05c8)), "the heartbeat loop");
    let t0 = w.speedpill.time;
    w.step(12, &[PlayerInput::default()]);
    assert_eq!(t0 - w.speedpill.time, 1, "the boost clock runs in game ticks");
    w.speedpill.time = 10;
    let mut ev = Vec::new();
    for _ in 0..60 {
        w.step(12, &[PlayerInput::default()]);
        ev.extend(w.take_events());
    }
    assert!(!w.speedpill.want && !w.speedpill.on && w.speedpill.change == 0, "{:?}", w.speedpill);
    assert!(sounds(&ev).contains(&0x02ad), "Jo groans as it wears off");
    assert!(ev.iter().any(|e| matches!(e, Event::StopSound { handle } if *handle == crate::gun::boost::MISC_SFX_HANDLE)), "the heartbeat stops");
    assert_eq!(w.lv.lvupdate240, 12, "full speed again");
}

/// The RC-P120's cloak (hold B + fire, `prop.c:1367`): Jo fades out over about
/// a second (`chr_update_cloak`); once fully in, it eats 0.4 rounds a tick
/// (`bondgun.c:8055`); firing drops it for 2 s (`chr_uncloak_temporarily`),
/// then it comes back; another gun turns it off.
#[test]
fn rcp120_cloak_fades_eats_ammo_breaks_on_firing_and_comes_back() {
    let mut w = settled(WEAPON_RCP120);
    w.take_events();
    run(&mut w, &PlayerInput { use_held: true, ..Default::default() }, 30);
    let mut ev = Vec::new();
    for _ in 0..4 {
        run(&mut w, &PlayerInput { use_held: true, fire: true, ..Default::default() }, 1);
        ev.extend(w.take_events());
    }
    assert!(w.players[0].devicesactive & DEVICE_CLOAKRCP120 != 0 && w.players[0].cloak.cloaked);
    assert!(sounds(&ev).contains(&0x005b), "the cloak on");
    idle(&mut w, 70);
    let c = w.players[0].cloak;
    assert!(c.fadefinished && c.alpha() <= 20, "{c:?} alpha {}", c.alpha());
    let a0 = w.players[0].gun.hands[0].loadedammo[0];
    idle(&mut w, 50);
    let used = a0 - w.players[0].gun.hands[0].loadedammo[0];
    assert!((19..=21).contains(&used), "0.4 rounds a tick: {used}");
    idle(&mut w, 2);
    w.take_events();
    fire_once(&mut w);
    idle(&mut w, 2);
    let c = w.players[0].cloak;
    assert!(!c.cloaked && c.pause > 100, "{c:?}");
    assert!(sounds(&w.take_events()).contains(&0x005c), "the cloak off");
    assert!(w.players[0].devicesactive & DEVICE_CLOAKRCP120 != 0, "the device stays on");
    idle(&mut w, 125);
    assert!(w.players[0].cloak.cloaked, "back on after the pause");
    run(&mut w, &PlayerInput { select: Some((WEAPON_FALCON2, false)), ..Default::default() }, 1);
    idle(&mut w, 120);
    assert!(w.players[0].devicesactive & DEVICE_CLOAKRCP120 == 0 && !w.players[0].cloak.cloaked);
    assert_eq!(w.players[0].cloak.alpha(), 255);
}


/// `func0f06e9cc` stands local +y on the normal it is given, to within
/// PD's `BADDTOR(90)` against `atan2f`'s true radians (2.5e-4 rad).
#[test]
fn func0f06e9cc_stands_y_on_the_normal() {
    for n in [Vec3::X, -Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(0.3, 0.8, -0.5).normalize()] {
        let m = func0f06e9cc(n);
        let y = m.y_axis.truncate();
        assert!((y - n).length() < 1e-3, "{n} → {y}");
    }
}

/// `mtx4_get_rotation` inverts `mtx4_load_rotation` (`mtx.c:187`, `:223`).
#[test]
fn get_rotation_inverts_load_rotation() {
    let rot = Vec3::new(0.3, -0.7, 2.1);
    let m = math::load_rotation(rot);
    let back = math::load_rotation(math::mtx4_get_rotation(&m));
    assert!(m.abs_diff_eq(back, 1e-5), "{m} vs {back}");
}


/// On Complex, a grenade thrown at the floor lands on it, settles and goes off
/// on its fuse, and a timed mine thrown at a wall sticks to it: PD's
/// collision on a real arena (the BG's triangles for sticky flight, the tiles
/// for floors).
#[test]
fn on_complex_a_grenade_settles_on_the_floor_and_a_mine_sticks_to_a_wall() {
    use std::sync::Arc;
    let stage = Arc::new(crate::stage::Stage::load(&assets(), "ref").unwrap());
    let level = Arc::new(crate::stage::TileLevel::new(stage.geom.clone()));
    let setup = pd_core::mp::MatchSetup { stagenum: STAGE_MP_COMPLEX, players: vec![pd_core::mp::MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
    let mut w = World::new(setup.clone(), stage.clone(), level.clone(), res(), 7).unwrap();
    run(&mut w, &PlayerInput { select: Some((WEAPON_GRENADE, false)), ..Default::default() }, 1);
    idle(&mut w, 200);
    w.players[0].verta = -30.0;
    throw_once(&mut w);
    let mut rest = None;
    for _ in 0..300 {
        idle(&mut w, 1);
        if let Some(o) = objs(&w, WEAPON_GRENADE).first() {
            if o.projectile.is_none() && rest.is_none() {
                rest = Some(o.pos);
            }
        }
    }
    let rest = rest.expect("the grenade never came to rest");
    let floor = level.cd_find_room_at_pos_ycnp(rest + Vec3::Y * 5.0).expect("a floor under it").0;
    assert!((rest.y - floor).abs() < 20.0, "resting on the floor: {rest} over {floor}");
    assert!(objs(&w, WEAPON_GRENADE).is_empty() && w.fx.wallhits.iter().any(|h| h.texnum == WALLHITTEX_SCORCH), "it went off with a scorch");

    // A timed mine at the nearest wall straight ahead.
    let mut w = World::new(setup, stage.clone(), level, res(), 7).unwrap();
    run(&mut w, &PlayerInput { select: Some((WEAPON_TIMEDMINE, false)), ..Default::default() }, 1);
    idle(&mut w, 200);
    let eye = w.players[0].pos;
    let ahead = w.players[0].look;
    let wall = stage.bghit.bg_test_hit(eye, eye + ahead * 3000.0).expect("a wall ahead");
    throw_once(&mut w);
    let mut stuck = None;
    for _ in 0..200 {
        idle(&mut w, 1);
        if let Some(o) = objs(&w, WEAPON_TIMEDMINE).first() {
            if o.has(OBJHFLAG_ATTACHED) && stuck.is_none() {
                stuck = Some(o.pos);
            }
        }
    }
    let stuck = stuck.expect("the mine never stuck");
    assert!((stuck - wall.pos).length() < 300.0, "stuck near the wall ahead: {stuck} vs {}", wall.pos);
}
