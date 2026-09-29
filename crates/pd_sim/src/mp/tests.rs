//! The match: counters by chr slot, the kill feed, the limits, the pause, the
//! awards, teams and the options.

use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::mp::{MatchPlayer, MatchSetup};
use pd_core::rng::Rng;

use super::*;
use crate::bot::tests::{complex_world, duel, step};
use crate::harness;
use crate::player::PlayerInput;
use crate::testutil::{idle, range, res, run};
use crate::world::World;

/// Tap the trigger (6 ticks down, 6 up) until chr 1 dies, at most 12 s.
fn shoot_until_dead(w: &mut World) -> bool {
    for t in 0..60 * 12 {
        let fire = t > 60 && (t / 6) % 2 == 0;
        step(w, &PlayerInput { fire, ..PlayerInput::default() });
        if w.chr_is_dead(1) {
            return true;
        }
    }
    false
}

fn texts(w: &World, pi: usize) -> Vec<String> {
    let mut m: Vec<&HudMessage> = w.mp.hudmsgs.msgs.iter().filter(|m| m.state != HUDMSGSTATE_FREE && m.playernum == pi).collect();
    m.sort_by_key(|m| m.id);
    m.iter().map(|m| m.text.clone()).collect()
}

/// The player's Falcon kills the simulant: slot 0 killed slot 4, the
/// simulant's death counts, the player reads "Killed Sim 1" and "Kill count:
/// 1", the simulant remembers its killer, and the shots count by region.
#[test]
fn a_kill_is_counted_by_chr_slot_and_told_to_the_killer() {
    let (mut w, _, _) = duel(None);
    assert!(shoot_until_dead(&mut w), "the simulant survived");
    assert_eq!(w.mp.chrs[0].killcounts[4], 1);
    assert_eq!(w.mp.chrs[4].numdeaths, 1);
    assert_eq!((w.mp_chr_kills(0), w.mp_chr_deaths(1)), (1, 1));
    assert_eq!(w.chrs[1].aibot.as_ref().unwrap().lastkilledbyplayernum, 0);
    let t = texts(&w, 0);
    assert!(t.iter().any(|s| s == "Killed Sim 1\n"), "{t:?}");
    assert!(t.iter().any(|s| s == "Kill count: 1\n"), "{t:?}");
    let s = &w.mp.playerstats[0];
    assert!(s.shotcount[SHOTREGION_TOTAL] >= 2, "{:?}", s.shotcount);
    let hits: i32 = s.shotcount[1..].iter().sum();
    assert!(hits >= 1 && hits <= s.shotcount[SHOTREGION_TOTAL], "{:?}", s.shotcount);
    assert!(s.damtransmitted > 0.0);
    assert_eq!(w.mp.playerstats[0].maxsimulkills, 1);
    // The ranking: the player first with one point.
    let r = w.mp_get_player_rankings();
    assert_eq!((r.rankings[0].mpchr, r.rankings[0].score), (Some(0), 1));
    assert_eq!(w.mp.chrs[0].placement, 0);
}

/// A fall kills the player by its own hand: `lastshooter` is never set in NTSC
/// final, so even a player just shot is a suicide (killcounts of its own slot).
#[test]
fn a_fall_is_a_suicide_as_pd_never_sets_lastshooter() {
    let mut w = range();
    idle(&mut w, 10);
    w.player_die(0);
    assert!(w.players[0].isdead);
    assert_eq!(w.mp.chrs[0].killcounts[0], 1);
    assert_eq!(w.mp.chrs[0].numdeaths, 1);
    let r = w.mp_get_player_rankings();
    assert_eq!(r.rankings[0].score, -1);
    // The "Died once" message is dropped with the player dead? It is not
    // HUDMSGFLAG_ONLYIFALIVE; the suicide count is.
    let t = texts(&w, 0);
    assert!(t.iter().any(|s| s == "Suicide count: 1\n"), "{t:?}");
}

/// A HUD message queues, then (the player alive, the space clear) boxes in,
/// fades in with the swish over (√(w²+h²)+132)/7 ticks, stays 80, fades out
/// over (√(w²+h²)+92)/7 and is free.
#[test]
fn a_hud_message_fades_in_stays_and_fades_out() {
    let mut w = range();
    idle(&mut w, 10);
    // The match's first message (frame 5) is up where the next will go: a
    // new one waits, then boots it out as it starts fading.
    assert_eq!(texts(&w, 0), ["Combat\n"]);
    w.hudmsg_create(0, "Died once\n", HUDMSGTYPE_DEFAULT);
    idle(&mut w, 5);
    let waiting = w.mp.hudmsgs.msgs.iter().find(|m| m.text == "Died once\n").unwrap().state;
    assert_eq!(waiting, HUDMSGSTATE_QUEUED);
    for _ in 0..200 {
        idle(&mut w, 1);
        if w.mp.hudmsgs.msgs.iter().any(|m| m.text == "Died once\n" && m.state != HUDMSGSTATE_QUEUED) {
            break;
        }
    }
    assert_eq!(texts(&w, 0), ["Died once\n"], "the fading one was freed");
    for m in w.mp.hudmsgs.msgs.iter_mut() {
        m.state = HUDMSGSTATE_FREE;
    }
    w.take_events();
    w.hudmsg_create(0, "Kill count: 1\n", HUDMSGTYPE_DEFAULT);
    let i = w.mp.hudmsgs.msgs.iter().position(|m| m.state == HUDMSGSTATE_QUEUED).unwrap();
    let m = &w.mp.hudmsgs.msgs[i];
    assert!(m.width > 20 && m.height > 5, "{}×{}", m.width, m.height);
    // Bottom left of the 320 × 220 view: x = 24 + 3, y = 220 - h - 14.
    assert_eq!((m.x, m.y), (27, 220 - m.height - 14));
    let (fadein, fadeout) = (hudmsg::hudmsg_fadein_time(m) as i32, hudmsg::hudmsg_fadeout_time(m) as i32);
    let mut states = vec![];
    let mut swish = false;
    for _ in 0..(2 + fadein + 80 + fadeout + 5) {
        idle(&mut w, 1);
        swish |= crate::testutil::sounds(&w.take_events()).contains(&0x003e);
        let s = w.mp.hudmsgs.msgs[i].state;
        if states.last() != Some(&s) {
            states.push(s);
        }
    }
    assert_eq!(states, [HUDMSGSTATE_CHOOSETRANSITION, HUDMSGSTATE_FADINGIN, HUDMSGSTATE_ONSCREEN, HUDMSGSTATE_FADINGOUT, HUDMSGSTATE_FREE]);
    assert!(swish);
    // The same text twice is one message.
    w.hudmsg_create(0, "Died once\n", HUDMSGTYPE_DEFAULT);
    w.hudmsg_create(0, "Died once\n", HUDMSGTYPE_DEFAULT);
    assert_eq!(texts(&w, 0).len(), 1);
}

/// With one player, its open menu pauses the match: nothing moves, the clock
/// stops; START opens the pause menu; the START that closed it is ignored
/// until let go.
#[test]
fn an_open_menu_pauses_a_one_player_match() {
    let mut w = complex_world(1, 2, BOTDIFF_NORMAL, 7);
    for _ in 0..200 {
        harness::step_idle(&mut w);
    }
    // START opens the pause menu (the menus push it).
    let start = PlayerInput { start: true, ..PlayerInput::default() };
    step(&mut w, &start);
    assert!(w.take_events().iter().any(|e| matches!(e, Event::MpPushPauseDialog { player: 0 })));
    w.set_menu_open(0, true);
    assert!(w.mp_is_paused());
    let (t, pos, frame) = (w.mp.stagetime60, w.chrs[1].pos, w.lv.lvframenum);
    for _ in 0..120 {
        step(&mut w, &PlayerInput { walk_y: 127, fire: true, ..PlayerInput::default() });
    }
    // A paused chr_update_position still recomputes manground from sumground
    // (PD's too), which can move a chr by float noise.
    assert_eq!((w.mp.stagetime60, w.lv.lvframenum), (t, frame), "the match moved while paused");
    assert!(w.chrs[1].pos.distance(pos) < 1e-3, "the simulant moved while paused: {pos} to {}", w.chrs[1].pos);
    // Closed with START still held: no new pause menu until START is let go.
    w.set_menu_open(0, false);
    w.take_events();
    step(&mut w, &start);
    step(&mut w, &start);
    assert!(!w.take_events().iter().any(|e| matches!(e, Event::MpPushPauseDialog { .. })));
    step(&mut w, &PlayerInput::default());
    step(&mut w, &start);
    assert!(w.take_events().iter().any(|e| matches!(e, Event::MpPushPauseDialog { .. })));
    assert!(w.mp.stagetime60 > t);
}

/// The score limit: at 1 point the match ends once the dying is over: the
/// awards are worked out, the match is over for good and the end screens are
/// asked for.
#[test]
fn reaching_the_score_limit_ends_the_match_after_the_death() {
    let (mut w, _, _) = duel(None);
    w.setup.scorelimit = 0;
    w.mp.scorelimit = 1;
    assert!(shoot_until_dead(&mut w));
    // The simulant is still dying: the match waits.
    assert_eq!(w.mp.numreasonstoend, 0, "counted before the frame's check");
    let mut ended = None;
    for f in 0..60 * 5 {
        step(&mut w, &PlayerInput::default());
        if w.take_events().contains(&Event::MpEndMatch) {
            ended = Some(f);
            break;
        }
    }
    let f = ended.expect("the match never ended");
    assert!(f > 5, "ended while the simulant was dying ({f})");
    assert!(w.mp.endscreen);
    assert_eq!(w.mp.paused, MPPAUSEMODE_GAMEOVER);
    // One player: only the one-player awards; this one killed with a head
    // or body shot, so "Most Deadly" / "Most Professional" are candidates.
    let v = &w.mp.players[0];
    let allowed = [4u8, 5, 6, 9, 10, 14, 15, 16];
    for a in [v.award1, v.award2].into_iter().flatten() {
        assert!(allowed.contains(&a), "award {a}");
    }
    assert!(v.award1.is_some());
    let r = &w.mp.results[0];
    assert_eq!((r.slot, r.career.kills, r.career.gamesplayed, r.career.gameswon), (0, 1, 1, 1));
    assert_eq!(r.medals & MEDAL_KILLMASTER, MEDAL_KILLMASTER, "{:#x}", r.medals);
    // Nothing more moves; START doesn't reopen anything.
    let t = w.mp.stagetime60;
    step(&mut w, &PlayerInput { start: true, ..PlayerInput::default() });
    assert_eq!(w.mp.stagetime60, t);
}

/// A one-minute match: the alarm starts ten seconds before the end and the
/// match ends on the minute; a two-minute one says "One minute left." first.
#[test]
fn the_time_limit_warns_sounds_the_alarm_and_ends_the_match() {
    let mut w = range();
    w.setup.timelimit = 1;
    w.mp.timelimit60 = 2 * 3600;
    let mut warned = None;
    let mut alarm = None;
    let mut ended = None;
    for f in 1..=2 * 3600 + 2 {
        idle(&mut w, 1);
        if warned.is_none() && texts(&w, 0).iter().any(|s| s == "One minute left.\n") {
            warned = Some(f);
        }
        for e in w.take_events() {
            match e {
                Event::HandleSound { handle: MISC_AUDIO_HANDLE, sound: 0x00a3, .. } => alarm = Some(f),
                Event::MpEndMatch => ended = Some(f),
                _ => {}
            }
        }
    }
    // Frame f's lv_tick sees stagetime60 = f - 1 and f.
    assert_eq!(warned, Some(3600));
    assert_eq!(alarm, Some(2 * 3600 - 600));
    assert_eq!(ended, Some(2 * 3600));
    assert!(w.mp.endscreen);
}

/// Teams: two simulants on one team and the player on the other: they never
/// target each other, only the player.
#[test]
fn simulants_leave_their_teammates_alone() {
    let (stage, level) = crate::testutil::complex_arc();
    let mut setup = harness::setup(1, 2, BOTDIFF_NORMAL);
    setup.options |= MPOPTION_TEAMSENABLED;
    setup.players[0].chr.team = 0;
    for s in setup.simulants.iter_mut() {
        s.chr.team = 1;
    }
    let mut w = harness::world(stage, level, res(), setup, harness::NavChoice::Pd, 11, true).unwrap();
    let mut targeted_player = false;
    for _ in 0..60 * 30 {
        harness::step_idle(&mut w);
        for i in 1..3 {
            if let Some(t) = w.chrs[i].target {
                assert_eq!(t, 0, "simulant {i} targets its teammate {t}");
                targeted_player = true;
            }
        }
    }
    assert!(targeted_player);
    assert!(w.chr_compare_teams(1, 2, Compare::Friends) && w.chr_compare_teams(0, 1, Compare::Enemies));
}

/// One-hit kills: a single Falcon round kills a simulant.
#[test]
fn one_hit_kills_take_one_round() {
    let (mut w, _, _) = duel(None);
    w.setup.options |= MPOPTION_ONEHITKILLS;
    for t in 0..60 * 4 {
        let fire = t > 60 && t < 64;
        step(&mut w, &PlayerInput { fire, ..PlayerInput::default() });
        if w.chr_is_dead(1) {
            break;
        }
    }
    assert!(w.chr_is_dead(1), "damage {:.2}", w.chrs[1].damage);
    assert!(w.shots_fired(0) <= 2, "{} shots", w.shots_fired(0));
}

/// Fast movement: the same second of running goes 25% further.
#[test]
fn fast_movement_runs_a_quarter_faster() {
    let dist = |fast: bool| {
        let mut w = range();
        if fast {
            w.setup.options |= MPOPTION_FASTMOVEMENT;
        }
        idle(&mut w, 30);
        let a = w.players[0].pos;
        run(&mut w, &PlayerInput { walk_y: 127, ..PlayerInput::default() }, 40);
        (w.players[0].pos - a).length()
    };
    let (slow, fast) = (dist(false), dist(true));
    assert!(fast > slow * 1.2 && fast < slow * 1.3, "{slow:.1} → {fast:.1}");
}

/// `mp_find_max_float`'s truncated best: 2.9 beats 2.5 as the second value,
/// but a third of 2.5 then compares with 2 and wins.
#[test]
fn the_float_search_keeps_pds_truncated_best() {
    let mut rng = Rng::new(1);
    assert_eq!(mp_find_max_float(&mut rng, 2, [2.5, 2.9, 0.0, 0.0]), 1);
    assert_eq!(mp_find_max_float(&mut rng, 3, [2.5, 2.9, 2.5, 0.0]), 2);
    assert_eq!(mp_find_max_int(&mut rng, 3, [1, 5, 3, 0]), 1);
    assert_eq!(mp_find_min_int(&mut rng, 4, [4, 5, 3, 9]), 2);
}

/// The weapon of choice is the pair held longest.
#[test]
fn the_weapon_of_choice_is_the_one_held_longest() {
    let mut w = crate::testutil::settled(WEAPON_CMP150);
    idle(&mut w, 300);
    assert_eq!(w.inv_get_weapon_of_choice(0).0, WEAPON_CMP150);
    assert_eq!(w.mp_player_get_weapon_of_choice_name(0), "CMP150");
}

/// The match setup's limits: 10 minutes, 10 points, none for teams.
#[test]
fn the_limits_come_from_the_setup() {
    let setup = MatchSetup { players: vec![MatchPlayer::default()], teamscorelimit: 400, ..MatchSetup::default() };
    let m = MpMatch::new(&setup);
    assert_eq!((m.timelimit60, m.scorelimit, m.teamscorelimit), (10 * 3600, 10, 0));
}
