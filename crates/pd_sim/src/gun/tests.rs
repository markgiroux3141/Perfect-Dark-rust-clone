//! The guns, headless, in the firing range (`stage::fixtures::firing_range`) and
//! on Complex. From the old repo's `pd_guns/tests.rs` (the hitscan, melee, HUD
//! and pad tests; the projectile, explosive and special ones are M5's), on a
//! `World` instead of the range `Sim`.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use pd_core::anim::{Anim, AnimCtx};
use pd_core::ids::*;
use pd_core::model::{Model, NodeKind, PoseParams};
use pd_core::mp::{MatchPlayer, MatchSetup};

use crate::player::PlayerInput;
use crate::stage::{fixtures, Stage, TileLevel};
use crate::testutil::*;
use crate::world::World;

/// Every POSITION node of a hand model matches the gun's joint with the same
/// anim part: the same rest offset and matrix slot (`bondgun.c:8394` draws the
/// hands with the gun's matrices).
#[test]
fn falcon_and_hands_share_joints_0_to_32() {
    let r = res();
    let gun = r.models.get("falcon2").unwrap();
    let hand = r.models.get("hand_joaf1").unwrap();
    let mut checked = 0;
    for hn in &hand.nodes {
        let NodeKind::Position { pos: hpos, animpart, mtx: hmtx, .. } = hn.kind else { continue };
        let (gpos, gmtx) = gun
            .nodes
            .iter()
            .find_map(|n| match n.kind {
                NodeKind::Position { pos, animpart: a, mtx, .. } if a == animpart => Some((pos, mtx)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the gun lacks joint {animpart}"));
        assert_eq!(hmtx[0], gmtx[0], "joint {animpart}'s slot");
        assert!((hpos - gpos).length() < 0.3, "joint {animpart}'s offset differs by {}", hpos - gpos);
        checked += 1;
    }
    assert_eq!(checked, 33);
}

/// The Falcon's reload brings the left wrist to the gun and back, and clamps on
/// its last frame.
#[test]
fn falcon_reload_moves_the_left_hand_in_and_back() {
    let r = res();
    let mut m = Model::new(r.models.get("falcon2").unwrap());
    let reload = r.bank.by_name("ANIM_GUN_FALCON2_RELOAD").unwrap();
    let mut anim = Anim::default();
    let mut ctx = AnimCtx { bank: &r.bank, skel: 0, scale: 1.0, chrinfo: None, merging_enabled: true };
    anim.set_animation(&mut ctx, reload, false, 0.0, 1.0, 0.0);
    let wrist = |m: &Model| m.matrices[18].w_axis.truncate();
    let gunpos = |m: &Model| m.matrices[33].w_axis.truncate();
    let params = PoseParams::new(Mat4::IDENTITY, &r.bank);
    m.set_matrices_with_anim(&params, Some(&anim), None);
    let start_gap = (wrist(&m) - gunpos(&m)).length();
    let mut closest = f32::MAX;
    for _ in 0..91 {
        anim.tick(&mut ctx, 1, true);
        m.set_matrices_with_anim(&params, Some(&anim), None);
        closest = closest.min((wrist(&m) - gunpos(&m)).length());
    }
    assert!(closest < start_gap * 0.6, "the left hand never came to the gun: {closest} vs {start_gap}");
    assert_eq!(anim.frame as i32, 91, "clamped on the last frame (not looping)");
}

/// The whole frame, headless: the Falcon comes up, the trigger fires down the
/// range, and PD's chain runs: the state machine, the shot from the eye, the
/// hit position, the tracer from the muzzle, a bullet hole and the shot sound.
#[test]
fn falcon_fires_down_the_range_and_hits_the_board() {
    let mut w = range();
    let mut raised_at = None;
    for f in 0..240 {
        idle(&mut w, 1);
        let h = &w.players[0].gun.hands[HAND_RIGHT];
        if raised_at.is_none() && h.visible && h.state == HANDSTATE_IDLE {
            raised_at = Some(f);
        }
    }
    assert!(raised_at.is_some(), "the Falcon never became visible and idle");
    assert_eq!(w.players[0].gun.bgun_get_weapon_num(HAND_RIGHT), WEAPON_FALCON2);
    // The gun sits in front of and below the eye (camera space looks down -z).
    let root = w.players[0].gun.hands[HAND_RIGHT].gunmodel.as_ref().unwrap().matrices[0].w_axis.truncate();
    assert!(root.z < 0.0 && root.y < 0.0, "the gun should be in front of and below the eye: {root}");
    w.take_events();
    let mut heard = Vec::new();
    for _ in 0..30 {
        fire_once(&mut w);
        heard.extend(sounds(&w.take_events()));
    }
    assert!(w.shots_fired[0] > 0, "no shot left the gun");
    assert!(w.boards[0].hits >= 1, "the first board took no hits: hitpos {}", w.players[0].gun.hands[0].hitpos);
    assert!(!w.fx.wallhits.is_empty(), "no bullet hole");
    assert!(heard.contains(&0x804d), "no Falcon shot sound: {heard:x?}");
}

/// The Farsight shoots through walls (`prop.c:688`, `:724`): with a wall
/// between Jo and the first board, a Falcon round stops at the wall but the
/// Farsight's scores, and sparks the wall on its way. M5: the same through the
/// x-ray view, where the wall does not spark.
#[test]
fn farsight_rounds_go_through_the_wall_to_the_board() {
    let walled = || {
        let mut g = fixtures::firing_range();
        fixtures::add_box(&mut g, Vec3::new(-150.0, 0.0, 150.0), Vec3::new(150.0, 400.0, 170.0));
        let mut w = range_with(g);
        idle(&mut w, 1);
        w
    };
    let mut falcon = walled();
    idle(&mut falcon, 200);
    fire_once(&mut falcon);
    idle(&mut falcon, 30);
    assert_eq!(falcon.boards[0].hits, 0, "the wall stops a Falcon round");

    let mut w = walled();
    run(&mut w, &PlayerInput { select: Some((WEAPON_FARSIGHT, false)), ..Default::default() }, 1);
    idle(&mut w, 200);
    assert_eq!(w.players[0].gun.bgun_get_weapon_num(HAND_RIGHT), WEAPON_FARSIGHT);
    let sparks0 = w.fx.sparks.live();
    let mut sparked = false;
    run(&mut w, &PlayerInput { fire: true, ..Default::default() }, 1);
    for _ in 0..60 {
        idle(&mut w, 1);
        sparked |= w.fx.sparks.live() > sparks0;
    }
    assert!(w.boards[0].hits >= 1, "through the wall to the board");
    assert!(sparked, "the wall sparks");
}

/// Holding the trigger empties the CMP150 and then dry-fires
/// (`HANDSTATE_ATTACKEMPTY`, `bondgun.c:1185`); PD reloads only on release, and
/// the clip comes back full.
#[test]
fn cmp150_empties_and_reloads_itself() {
    let mut w = settled(WEAPON_CMP150);
    assert_eq!(w.players[0].gun.bgun_get_weapon_num(HAND_RIGHT), WEAPON_CMP150);
    let full = w.players[0].gun.hands[HAND_RIGHT].loadedammo[0];
    assert!(full > 0, "the CMP150's clip is empty after the equip");
    let fire = PlayerInput { fire: true, ..Default::default() };
    let (mut min_loaded, mut saw_empty) = (full, false);
    for _ in 0..300 {
        run(&mut w, &fire, 1);
        let h = &w.players[0].gun.hands[HAND_RIGHT];
        min_loaded = min_loaded.min(h.loadedammo[0]);
        saw_empty |= h.state == HANDSTATE_ATTACKEMPTY;
        assert_ne!(h.state, HANDSTATE_RELOAD, "PD doesn't reload with the trigger held");
    }
    assert_eq!(min_loaded, 0, "never emptied the clip");
    assert!(saw_empty, "no dry fire while the trigger stayed down");
    let mut saw_reload = false;
    for _ in 0..200 {
        idle(&mut w, 1);
        saw_reload |= w.players[0].gun.hands[HAND_RIGHT].state == HANDSTATE_RELOAD;
    }
    assert!(saw_reload, "never reloaded after the release");
    assert_eq!(w.players[0].gun.hands[HAND_RIGHT].loadedammo[0], full, "the clip wasn't refilled");
}

/// Four quick Falcon rounds push `gunsmokepoint` past 0.66; once the hand is
/// idle, `smoke_create_for_hand` makes SMOKETYPE_MUZZLE_PISTOL, whose parts rise
/// from the muzzle, and the smoke frees itself once they have faded.
#[test]
fn falcon_rapid_fire_leaves_muzzle_smoke_that_rises_and_clears() {
    let mut w = settled(WEAPON_FALCON2);
    let fire = PlayerInput { fire: true, ..Default::default() };
    let still = PlayerInput::default();
    for f in 0..90 {
        run(&mut w, if f % 6 == 0 && f < 30 { &fire } else { &still }, 1);
    }
    let muzzle = w.players[0].gun.hands[0].muzzlepos;
    let smoke = w.fx.smokes.slots.iter().flatten().find(|s| s.ty == SMOKETYPE_MUZZLE_PISTOL).expect("no muzzle smoke");
    let parts: Vec<_> = smoke.parts.iter().filter(|p| p.size > 0.0).collect();
    assert!(parts.len() >= 5, "only {} parts", parts.len());
    for p in &parts {
        assert!((p.pos.x - muzzle.x).abs() < 1.0 && (p.pos.z - muzzle.z).abs() < 1.0, "a part off the muzzle's column: {} vs {muzzle}", p.pos);
        assert!(p.pos.y >= muzzle.y - 0.01, "the parts only rise");
    }
    assert!(!w.players[0].gun.hands[0].createsmoke, "createsmoke clears once a smoke is made");
    idle(&mut w, 900);
    assert!(!w.fx.smokes.slots.iter().flatten().any(|s| s.ty == SMOKETYPE_MUZZLE_PISTOL), "the muzzle smoke never freed");
}

/// One player, a bullet hole within 4 m: `explosion_create_simple(BULLETHOLE)`,
/// a one-part flame that plays out and frees, and (half the time) a
/// SMOKETYPE_BULLETIMPACT puff. Past 4 m the flame is skipped.
#[test]
fn close_bullet_holes_get_a_flame_and_puff_far_ones_dont() {
    let mut w = settled(WEAPON_FALCON2);
    w.players[0].verta = -40.0; // the floor ~2 m ahead
    let fire = PlayerInput { fire: true, ..Default::default() };
    let still = PlayerInput::default();
    let (mut saw_flame, mut saw_puff) = (false, false);
    for f in 0..120 {
        run(&mut w, if f % 12 == 0 { &fire } else { &still }, 1);
        saw_flame |= w.explosions.slots.iter().flatten().any(|e| e.ty == EXPLOSIONTYPE_BULLETHOLE);
        saw_puff |= w.fx.smokes.slots.iter().flatten().any(|s| s.ty == SMOKETYPE_BULLETIMPACT);
    }
    let hp = w.players[0].gun.hands[0].hitpos;
    assert!((hp - w.players[0].pos).length() < 400.0, "the shots should land close: {hp}");
    assert!(saw_flame, "no bullet-hole flame at close range");
    assert!(saw_puff, "no bullet-impact puff in ten close hits");
    idle(&mut w, 60);
    assert_eq!(w.explosions.live(), 0, "bullet-hole flames last 30 + 16 ticks");

    // Level again, and far: the first board is ~6.5 m away.
    w.players[0].verta = 0.0;
    for f in 0..60 {
        run(&mut w, if f % 12 == 0 { &fire } else { &still }, 1);
        assert!(!w.explosions.slots.iter().flatten().any(|e| e.ty == EXPLOSIONTYPE_BULLETHOLE), "a flame beyond 4 m");
    }
}

/// A rocket-sized blast next to a board: object damage on the first frame, the
/// room flash (+rangeh, decaying 2 per quarter-tick), the vi shake, SMOKETYPE_LARGE
/// 20 ticks before the end, freed after duration + 16 × flarespeed.
#[test]
fn rocket_explosion_hurts_the_board_flashes_the_room_shakes_and_smokes() {
    let mut w = settled(WEAPON_FALCON2);
    let board = (w.boards[0].min + w.boards[0].max) * 0.5;
    let at = board - Vec3::new(0.0, 0.0, 60.0);
    let room = w.level.floor_room(at, 1.0).unwrap();
    let settled_light = w.lights.brightness(Some(room));
    assert!(w.explosion_create_simple(0, at, EXPLOSIONTYPE_ROCKET));
    assert!(w.lights.room(room).br_flash >= 80, "rangeh 80 flash: {}", w.lights.room(room).br_flash);
    idle(&mut w, 1);
    assert!(w.boards[0].damage > 1.0, "first-frame object damage: {}", w.boards[0].damage);
    assert!(w.vi.intensity > 0.0, "no shake");
    assert!(w.lights.brightness(Some(room)) > settled_light);
    let (mut smoked_at, mut freed_at) = (None, None);
    for f in 1..300 {
        idle(&mut w, 1);
        if smoked_at.is_none() && w.fx.smokes.slots.iter().flatten().any(|s| s.ty == SMOKETYPE_LARGE) {
            smoked_at = Some(f);
        }
        if freed_at.is_none() && w.explosions.live() == 0 {
            freed_at = Some(f);
        }
    }
    let smoked_at = smoked_at.expect("no smoke");
    assert!((68..=72).contains(&smoked_at), "the smoke at maxage − 20 = 70: {smoked_at}");
    let freed_at = freed_at.expect("never freed");
    assert!((168..=172).contains(&freed_at), "freed at 90 + 16 × 5 = 170: {freed_at}");
    assert_eq!(w.lights.room(room).br_flash, 0, "the flash decays away");
    assert_eq!(w.vi.intensity, 0.0, "the shake stops");
}

/// The Phoenix's secondary is FUNCFLAG_EXPLOSIVESHELLS: the round that stops in
/// the board blows up there (EXPLOSIONTYPE_PHOENIX).
#[test]
fn phoenix_explosive_shells_blow_on_the_board() {
    let mut w = settled(WEAPON_PHOENIX);
    secondary(&mut w);
    assert_eq!(w.players[0].gun.hands[0].weaponfunc, FUNC_SECONDARY);
    fire_once(&mut w);
    let mut boom = false;
    for _ in 0..20 {
        idle(&mut w, 1);
        boom |= w.explosions.slots.iter().flatten().any(|e| e.ty == EXPLOSIONTYPE_PHOENIX);
    }
    assert!(boom, "no Phoenix blast");
}

/// The fists: a punch into the wall finds no chr, so on the next tick it tests
/// the BG in reach (`hand->unk0d0f_02`), which thuds.
#[test]
fn a_punch_at_the_wall_thuds() {
    let mut w = settled(WEAPON_UNARMED);
    // Face the west wall from 30 cm away.
    w.players[0].theta = 90.0;
    w.players[0].pos.x = -560.0;
    idle(&mut w, 2);
    w.take_events();
    let mut heard = Vec::new();
    for _ in 0..40 {
        fire_once(&mut w);
        heard.extend(sounds(&w.take_events()));
    }
    assert!(heard.iter().any(|&s| s == 0x8094 || s == 0x808f), "no punch thud: {heard:x?}");
}

/// A shot at Complex's floor: the BG it hits is the textured one, and the
/// floor's surface picks the hole and the hit sound (`g_SurfaceTypes`).
#[test]
fn a_shot_at_complexs_floor_sounds_like_its_surface() {
    let r = res();
    let stage = Arc::new(Stage::load(&assets(), "ref").unwrap());
    let level = Arc::new(TileLevel::new(stage.geom.clone()));
    let setup = MatchSetup { stagenum: STAGE_MP_COMPLEX, players: vec![MatchPlayer { slot: 0, handicap: 128, ..Default::default() }], ..Default::default() };
    let mut w = World::new(setup, stage.clone(), level, r, 7).unwrap();
    idle(&mut w, 200);
    w.players[0].verta = -60.0;
    idle(&mut w, 2);
    w.take_events();
    fire_once(&mut w);
    idle(&mut w, 2);
    let heard = sounds(&w.take_events());
    let hit = stage.bghit.bg_test_hit(w.players[0].pos, w.players[0].pos - Vec3::Y * 1000.0).unwrap();
    let surface = crate::stage::bghit::surface_type(hit.surface.unwrap().soundsurfacetype);
    assert!(heard.iter().any(|s| surface.sounds.contains(s)), "none of the floor's hit sounds {:x?} in {heard:x?}", surface.sounds);
    assert!(!w.fx.wallhits.is_empty(), "a bullet hole in the floor");
}

/// The magazine gauge's `abmag` follows the clip: settled full, a shot starts a
/// fade (change −1) that settles one unit lower; the 800-round reserve uses the
/// merged bar (36 px).
#[test]
fn hud_gauges_follow_the_clip_and_the_reserve() {
    let mut w = settled(WEAPON_FALCON2);
    let hud = &w.players[0].gun.hud;
    assert_eq!(hud.abmag[0].ref_, 8, "a full clip");
    assert_eq!(hud.ctrl_abmag.ref_, 36, "a full reserve bar");
    fire_once(&mut w);
    idle(&mut w, 3);
    assert_eq!(w.players[0].gun.hud.abmag[0].change, -1, "the spent round fading");
    idle(&mut w, 60);
    let a = w.players[0].gun.hud.abmag[0];
    assert_eq!((a.ref_, a.change), (7, 0));
}

/// The function square's fader runs to 255 on the secondary, and the function
/// name follows it.
#[test]
fn hud_function_square_turns_to_the_secondary() {
    let mut w = settled(WEAPON_CMP150);
    assert_eq!(w.players[0].gun.hud.fnfader, 0);
    secondary(&mut w);
    assert_eq!(w.players[0].gun.hud.fnfader, 255);
    let func = w.res.gset.func(WEAPON_CMP150, FUNC_SECONDARY).unwrap();
    assert_eq!(w.players[0].gun.hud.curfnstr.as_deref(), Some(func.name.as_str()));
}

/// Every explosion sound is in the sound pool, except 0x80a5 (an MP3 in PD).
#[test]
fn every_explosion_sound_is_in_the_pool() {
    let m: serde_json::Value = assets().read_json(&assets().sfx_manifest()).unwrap();
    let have: std::collections::HashSet<u16> = m
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|k| match k.strip_prefix("SFXMAP_") {
            Some(r) => u16::from_str_radix(&r[..4], 16).ok(),
            None => u16::from_str_radix(k, 16).ok(),
        })
        .collect();
    for (i, t) in crate::props::explosions::EXPLOSION_TYPES.iter().enumerate() {
        if t.sound != 0 && t.sound != 0x80a5 {
            assert!(have.contains(&t.sound), "explosion type {i}: sound {:#06x} missing", t.sound);
        }
    }
    for id in [0x8079u16, 0x8087, 0x8088, 0x8051, 0x804d, 0x8052] {
        assert!(have.contains(&id), "{id:#06x} missing");
    }
}

/// The remote mine's gunviscmds put the mine in the right hand and the
/// detonator in the left (`gunviscmds_remotemine`, GUNVISOP_SETVISIBILITY).
#[test]
fn remote_mine_hands_hold_the_mine_and_the_detonator() {
    let w = settled(WEAPON_REMOTEMINE);
    let cmds = &w.res.gset.weapon(WEAPON_REMOTEMINE).unwrap().gunviscmds;
    assert!(cmds.iter().all(|c| c.op == GUNVISOP_SETVISIBILITY), "SETVISIBILITY decoded");
    let vis = |h: usize, part: i32| w.players[0].gun.hands[h].gunmodel.as_ref().unwrap().part_visible(part);
    assert!(vis(0, MODELPART_REMOTEMINE_MINE) && !vis(0, MODELPART_REMOTEMINE_DETONATOR), "right: the mine");
    assert!(vis(1, MODELPART_REMOTEMINE_DETONATOR) && !vis(1, MODELPART_REMOTEMINE_MINE), "left: the detonator");
}

fn pad(f: impl FnOnce(&mut PlayerInput)) -> PlayerInput {
    let mut i = PlayerInput { pad: true, ..Default::default() };
    f(&mut i);
    i
}

/// Stick up walks forward, stick right turns the way the mouse does, C-up/down
/// look up/down, C-left strafes the way A does (`bondmove.c:1166`).
#[test]
fn pad_stick_walks_and_turns_and_c_buttons_strafe_and_look() {
    let mut w = settled(WEAPON_FALCON2);
    let p0 = w.players[0].pos;
    run(&mut w, &pad(|i| i.look_y = 80), 60);
    let d = w.players[0].pos - p0;
    // (A few cm of sideways drift is the walk's head bob.)
    assert!(d.z > 50.0 && d.x.abs() < 10.0, "forward (+z at theta 0): {d}");

    let t0 = w.players[0].theta;
    run(&mut w, &pad(|i| i.look_x = 80), 20);
    let mut kb = settled(WEAPON_FALCON2);
    let kt0 = kb.players[0].theta;
    run(&mut kb, &PlayerInput { mouse_dx: 20.0, ..Default::default() }, 20);
    let turned = |a: f32, b: f32| ((b - a + 540.0) % 360.0) - 180.0;
    assert!(turned(t0, w.players[0].theta).abs() > 5.0, "the stick turns");
    assert_eq!(turned(t0, w.players[0].theta).signum(), turned(kt0, kb.players[0].theta).signum(), "right is right");

    let v0 = w.players[0].verta;
    run(&mut w, &pad(|i| i.c_up = true), 20);
    assert!(w.players[0].verta > v0 + 5.0, "C-up looks up: {v0} → {}", w.players[0].verta);
    run(&mut w, &pad(|i| i.c_down = true), 40);
    assert!(w.players[0].verta < v0, "C-down looks down");

    let mut a = settled(WEAPON_FALCON2);
    let mut b = settled(WEAPON_FALCON2);
    let (pa, pb) = (a.players[0].pos, b.players[0].pos);
    run(&mut a, &pad(|i| i.c_left = true), 40);
    run(&mut b, &PlayerInput { walk_x: -127, ..Default::default() }, 40);
    let (da, db) = (a.players[0].pos - pa, b.players[0].pos - pb);
    assert!(da.length() > 30.0 && da.normalize().dot(db.normalize()) > 0.9, "C-left strafes like A: {da} vs {db}");
}

/// Aimed with the stick held up past 60: the crosshair goes down and the view
/// pitches down with it (PD's `invertpitch`, the MP default: the crosshair
/// and the edge look both take the stick's y as it is, `bondmove.c:1207`,
/// `:1807`).
#[test]
fn pad_aiming_up_moves_the_crosshair_and_the_view_down_together() {
    let mut w = settled(WEAPON_FALCON2);
    let c = w.players[0].cam.c_screenheight * 0.5;
    let v0 = w.players[0].verta;
    run(
        &mut w,
        &pad(|i| {
            i.aim = true;
            i.look_y = 80;
        }),
        40,
    );
    let cy = w.players[0].gun.p.crosspos[1];
    assert!(cy > c + 20.0, "the crosshair goes down: {cy} from {c}");
    assert!(w.players[0].verta < v0 - 2.0, "the view looks down with it: {v0} → {}", w.players[0].verta);
}

/// Aimed (R): the stick moves the crosshair (pushing up moves it down, as PD
/// passes the stick's y straight through, `bondmove.c:1807`), C-down crouches, a
/// short R tap stands back up (AIMCONTROL_HOLD), and on the sniper rifle C-up
/// zooms instead of crouching.
#[test]
fn pad_aiming_moves_the_crosshair_crouches_and_zooms() {
    let mut w = settled(WEAPON_FALCON2);
    let c = [w.players[0].cam.c_screenwidth * 0.5, w.players[0].cam.c_screenheight * 0.5];
    run(
        &mut w,
        &pad(|i| {
            i.aim = true;
            i.look_x = 50;
            i.look_y = 50;
        }),
        20,
    );
    let cp = w.players[0].gun.p.crosspos;
    assert!(cp[0] > c[0] + 10.0 && cp[1] > c[1] + 5.0, "the crosshair right and down: {cp:?} from {c:?}");

    let stand = w.players[0].crouchpos;
    run(&mut w, &pad(|i| i.aim = true), 1);
    run(
        &mut w,
        &pad(|i| {
            i.aim = true;
            i.c_down = true;
        }),
        1,
    );
    run(&mut w, &pad(|i| i.aim = true), 30);
    assert_eq!(w.players[0].crouchpos, stand - 1, "C-down while aiming crouches");
    run(&mut w, &pad(|_| {}), 5);
    run(&mut w, &pad(|i| i.aim = true), 3);
    run(&mut w, &pad(|_| {}), 1);
    assert_eq!(w.players[0].crouchpos, stand, "an R tap uncrouches");

    let mut sniper = settled(WEAPON_SNIPERRIFLE);
    let f0 = sniper.players[0].gun.p.gunzoomfovs[0];
    run(
        &mut sniper,
        &pad(|i| {
            i.aim = true;
            i.c_up = true;
        }),
        30,
    );
    assert!(sniper.players[0].gun.p.gunzoomfovs[0] < f0 * 0.8, "C-up zooms the sniper rifle");
    assert_eq!(sniper.players[0].crouchpos, stand, "and doesn't crouch");
}

/// A tap = the next gun, A + Z = the previous one (Z doesn't fire while A is
/// held); a B tap reloads, a B hold toggles the function.
#[test]
fn pad_a_cycles_guns_and_b_taps_reload_or_holds_the_function() {
    let mut w = settled(WEAPON_FALCON2);
    let held = |w: &World| (w.players[0].gun.ctrl.weaponnum, w.players[0].gun.hands[1].inuse);
    let w0 = held(&w);
    run(&mut w, &pad(|i| i.a_held = true), 3);
    idle(&mut w, 200);
    // The next entry after one Falcon is two (bgun_cycle_forward).
    assert_eq!(held(&w), (w0.0, true), "A tap: the next entry, the dual Falcons");
    let shots = w.shots_fired[0];
    run(&mut w, &pad(|i| i.a_held = true), 3);
    run(
        &mut w,
        &pad(|i| {
            i.a_held = true;
            i.fire = true;
        }),
        3,
    );
    run(&mut w, &pad(|_| {}), 1);
    idle(&mut w, 200);
    assert_eq!(held(&w), w0, "A + Z: back to one Falcon");
    assert_eq!(w.shots_fired[0], shots, "Z with A held doesn't fire");

    for _ in 0..3 {
        run(&mut w, &pad(|i| i.fire = true), 1);
        idle(&mut w, 10);
    }
    let clip = w.players[0].gun.hands[0].clipsizes[0];
    assert!(w.players[0].gun.hands[0].loadedammo[0] < clip);
    run(&mut w, &pad(|i| i.use_held = true), 1);
    run(&mut w, &pad(|_| {}), 1);
    idle(&mut w, 150);
    assert_eq!(w.players[0].gun.hands[0].loadedammo[0], clip, "the B tap reloaded");

    let mut cmp = settled(WEAPON_CMP150);
    run(&mut cmp, &pad(|i| i.use_held = true), 30);
    idle(&mut cmp, 60);
    assert_eq!(cmp.players[0].gun.hands[0].weaponfunc, FUNC_SECONDARY, "the B hold: the secondary");
    assert_eq!(cmp.players[0].gun.hands[0].loadedammo[0], cmp.players[0].gun.hands[0].clipsizes[0], "a hold is not a reload tap");
}

/// A seeded match with guns is reproducible bit for bit: two worlds fed the
/// same inputs agree on every hand and effect.
#[test]
fn a_seeded_fight_is_reproducible() {
    let script = |f: usize| PlayerInput { fire: f % 17 < 6, walk_y: if f % 50 < 25 { 127 } else { 0 }, mouse_dx: ((f % 13) as f32 - 6.0) * 3.0, aim: f % 90 > 70, ..Default::default() };
    let mut a = range();
    let mut b = range();
    for f in 0..400 {
        a.step(4, &[script(f)]);
        b.step(4, &[script(f)]);
    }
    let key = |w: &World| {
        let h = &w.players[0].gun.hands[0];
        (w.players[0].pos, h.muzzlepos, h.hitpos, h.loadedammo, w.fx.wallhits.len(), w.fx.sparks.live(), w.rng.clone().random())
    };
    assert_eq!(format!("{:?}", key(&a)), format!("{:?}", key(&b)));
    assert!(a.shots_fired[0] > 10);
}
