//! Split screen: PD's viewports for two to four players, and the players'
//! cameras, portals and sights in their own quarters of the framebuffer.

use pd_core::ids::{SCREENSPLIT_HORIZONTAL, SCREENSPLIT_VERTICAL};
use pd_core::mp::{MatchPlayer, MatchSetup};

use super::camera::Viewport;
use super::PlayerInput;
use crate::testutil::{complex_arc, res};
use crate::world::World;

fn vp(left: i16, top: i16, width: i16, height: i16) -> Viewport {
    Viewport { left, top, width, height }
}

/// `player_get_viewport_*` for every player count and split: the views leave
/// one black line or column between them, taken from the top or left view.
#[test]
fn every_layouts_viewports() {
    let get = |pn, n, split| Viewport::player_get_viewport(pn, n, split);
    for split in [SCREENSPLIT_HORIZONTAL, SCREENSPLIT_VERTICAL] {
        assert_eq!(get(0, 1, split), vp(0, 0, 320, 220));
    }
    // Two players, one above the other.
    assert_eq!(get(0, 2, SCREENSPLIT_HORIZONTAL), vp(0, 0, 320, 109));
    assert_eq!(get(1, 2, SCREENSPLIT_HORIZONTAL), vp(0, 110, 320, 110));
    // Side by side.
    assert_eq!(get(0, 2, SCREENSPLIT_VERTICAL), vp(0, 0, 159, 220));
    assert_eq!(get(1, 2, SCREENSPLIT_VERTICAL), vp(160, 0, 160, 220));
    // Three and four: the quarters, whatever the split.
    for n in [3, 4] {
        for split in [SCREENSPLIT_HORIZONTAL, SCREENSPLIT_VERTICAL] {
            assert_eq!(get(0, n, split), vp(0, 0, 159, 109));
            assert_eq!(get(1, n, split), vp(160, 0, 160, 109));
            assert_eq!(get(2, n, split), vp(0, 110, 159, 110));
        }
    }
    assert_eq!(get(3, 4, SCREENSPLIT_HORIZONTAL), vp(160, 110, 160, 110));
    // The aspect PD's perspective takes: a quarter is about as wide as the
    // full screen, a horizontal half twice as wide.
    assert!((get(0, 2, SCREENSPLIT_HORIZONTAL).aspect() - 320.0 / 109.0).abs() < 1e-6);
    assert!((get(3, 4, SCREENSPLIT_HORIZONTAL).aspect() - 160.0 / 110.0).abs() < 1e-6);
}

/// `n` players on Complex, stepped `frames` times standing still.
pub(crate) fn complex_players(n: usize, split: u8, frames: usize) -> World {
    let (stage, level) = complex_arc();
    let players = (0..n).map(|i| MatchPlayer { slot: i as u8, handicap: 128, ..Default::default() }).collect();
    let setup = MatchSetup { stagenum: stage.stagenum, players, screensplit: split, ..Default::default() };
    let mut w = World::new(setup, stage, level, res(), 3).unwrap();
    let idle = vec![PlayerInput::default(); n];
    for _ in 0..frames {
        w.step(4, &idle);
    }
    w
}

/// Four players on Complex: each camera draws into its own quarter, its
/// portals' whole-view box is the quarter, its crosshair rests at the
/// quarter's centre, and the centre's ray is the camera's look.
#[test]
fn four_players_see_through_their_own_quarters() {
    let w = complex_players(4, SCREENSPLIT_HORIZONTAL, 30);
    for (i, p) in w.players.iter().enumerate() {
        let v = Viewport::player_get_viewport(i, 4, SCREENSPLIT_HORIZONTAL);
        let c = &p.cam;
        assert_eq!((c.c_screenleft, c.c_screentop, c.c_screenwidth, c.c_screenheight), (v.left as f32, v.top as f32, v.width as f32, v.height as f32), "player {i}");
        assert!((c.c_perspaspect - v.aspect()).abs() < 1e-6);
        let b = p.portalview.fullbox;
        assert_eq!((b.xmin, b.ymin, b.xmax, b.ymax), (v.left, v.top, v.left + v.width, v.top + v.height), "player {i}'s portal box");
        assert!(!p.portalview.drawslots.is_empty(), "player {i} sees a room");
        let [x, y] = p.gun.p.crosspos;
        let (cx, cy) = (v.left as f32 + v.width as f32 * 0.5, v.top as f32 + v.height as f32 * 0.5);
        assert!((x - cx).abs() < 2.0 && (y - cy).abs() < 2.0, "player {i}'s crosshair at ({x}, {y}), the view's centre ({cx}, {cy})");
        let dir = c.cam0f0b4c3c([cx, cy], 1.0);
        assert!(dir.x.abs() < 1e-3 && dir.y.abs() < 1e-3 && dir.z < -0.99, "player {i}'s centre ray {dir}");
    }
}

/// Two players on Complex 4 m apart, facing each other, player 1 armed
/// (the harness loadout) and settled.
fn facing_pair() -> World {
    use crate::bot::tests::{open_spot, theta_towards};
    let (stage, level) = complex_arc();
    let mut w = complex_players(2, SCREENSPLIT_HORIZONTAL, 1);
    let (a, b) = open_spot(&level, &stage, 400.0);
    crate::harness::place_player(&mut w, 0, a, theta_towards(a, b));
    crate::harness::place_player(&mut w, 1, b, theta_towards(b, a));
    w.harness_give_loadout(vec![pd_core::ids::WEAPON_FALCON2]);
    let idle = vec![PlayerInput::default(); 2];
    for _ in 0..120 {
        w.step(4, &idle);
    }
    w
}

/// Each player's body is posed where the player stands, facing its way,
/// holding the gun in its hand, and never in its own view: player 1's Falcon
/// rounds hit player 2's body by its parts, and player 2 sees them come from
/// the body's gun (`chrmuzzlelastpos`), not from player 1's first-person one.
#[test]
fn another_players_body_takes_its_rounds_and_shows_its_tracers() {
    let mut w = facing_pair();
    for j in 0..2 {
        let c = &w.chrs[j];
        let root = c.model.matrices.first().map(|m| m.w_axis.truncate()).expect("a posed body");
        let feet = w.players[j].pos;
        assert!((root.x - feet.x).abs() < 20.0 && (root.z - feet.z).abs() < 20.0, "player {j}'s body at {root}, the player at {feet}");
        assert!(root.y > w.players[j].manground && root.y < w.players[j].manground + 150.0, "the body's hips at {} over the floor at {}", root.y, w.players[j].manground);
        assert!(c.held[pd_core::ids::HAND_RIGHT].as_ref().is_some_and(|h| h.weaponnum == pd_core::ids::WEAPON_FALCON2), "player {j}'s body holds the Falcon");
        let gun = c.chr_get_gun_pos(pd_core::ids::HAND_RIGHT).expect("the body's gun is posed");
        assert!(gun.distance(root) < 120.0, "the gun at {gun}, the hips at {root}");
    }
    let health = w.players[1].bondhealth;
    let mut hit = false;
    let mut tracer = false;
    for t in 0..60 * 4 {
        let fire = (t / 6) % 2 == 0;
        w.step(4, &[PlayerInput { fire, ..PlayerInput::default() }, PlayerInput::default()]);
        let beam = &w.chrs[0].fireslots[pd_core::ids::HAND_RIGHT].beam;
        if beam.age >= 0 {
            let from = w.players[0].chrmuzzlelastpos[pd_core::ids::HAND_RIGHT];
            assert!(beam.from.distance(from) < 1.0, "the tracer the other player sees starts at the body's gun");
            assert!(beam.from.distance(w.players[0].gun.hands[pd_core::ids::HAND_RIGHT].muzzlepos) > 1.0, "not at the first-person muzzle");
            tracer = true;
        }
        if w.players[1].bondhealth < health {
            hit = true;
            break;
        }
    }
    assert!(tracer, "no tracer from player 1's body");
    assert!(hit, "player 2's body took no round (health {})", w.players[1].bondhealth);
}

/// Killed, a player's body plays one of `g_DeathAnimations` for the others
/// (the first person sees its head's death animation), and fades out.
#[test]
fn a_dead_players_body_falls_for_the_others() {
    let mut w = facing_pair();
    let mut died = false;
    for t in 0..60 * 12 {
        let fire = (t / 6) % 2 == 0;
        w.step(4, &[PlayerInput { fire, ..PlayerInput::default() }, PlayerInput::default()]);
        if w.players[1].isdead {
            died = true;
            break;
        }
    }
    assert!(died, "player 2 survived (health {})", w.players[1].bondhealth);
    let idle = vec![PlayerInput::default(); 2];
    for _ in 0..40 {
        w.step(4, &idle);
    }
    let anim = w.chrs[1].anim.animnum;
    assert!(crate::chr::thirdperson::DEATH_ANIMS.contains(&anim), "the body plays {anim:#x}");
    assert!(w.chrs[1].held.iter().all(|h| h.is_none()), "the body dropped its guns");
}
