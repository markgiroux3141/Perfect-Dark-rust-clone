//! `game/music.c`'s match half: the arena's tune at `lv_reset`, the death
//! tune and its five seconds, multiple-tune switching, and `mp_choose_track`
//! (`mplayer.c:2882`), which draws from the world's RNG. The world publishes
//! what PD queues as [`MusicCall`]s; the game's one music player
//! (`pd_core::music::Music`) runs them, and `music_tick_events` at
//! `lv_tick`'s `music_tick` ([`MusicCall::Tick`]).
//!
//! Not here, because a Combat Simulator match never reaches them: the NRG
//! tune (its reasons are set by solo AI commands), ambient tracks
//! (`g_StageTracks` lists no MP arena, so `stage_get_ambient_track` is -1),
//! the solo death tune, cutscenes and temporary tracks.

use pd_core::events::Event;
use pd_core::ids::*;
use pd_core::mp::MatchMusic;
use pd_core::music::{MusicCall, MusicVolume};
use pd_core::rng::Rng;

use crate::world::World;

/// The music globals a match keeps.
#[derive(Clone, Debug)]
pub struct MpMusic {
    /// `g_MpEnableMusicSwitching` (`mplayer.c:264`): two or more of the
    /// multiple tunes enabled.
    pub switching: bool,
    /// `g_MusicAge60`, `g_MusicLife60`, `g_MusicSilenceTimer60`.
    pub age60: i32,
    pub life60: i32,
    pub silence60: i32,
    /// `g_MusicDeathTimer240`: the death tune's five seconds.
    pub death_timer240: i32,
    /// `g_MusicMpDeathIsPlaying`.
    pub mpdeath_playing: bool,
    /// `g_MpLockInfo.unk04`: the last tune chosen (a `g_MpTracks` index).
    pub lasttrack: i32,
}

impl MpMusic {
    /// A match's start: `mp_start_match`'s `g_MpEnableMusicSwitching` and
    /// `g_MpLockInfo.unk04 = -1` (`mplayer.c:264-283`), `music_reset`'s
    /// death timer, and the globals' first values (`game/music.c:44`).
    ///
    /// SUBST: `g_MusicAge60` is a global PD never resets, so a match with
    /// switching carries the last one's age / each match starts at 0.
    pub fn new(m: &MatchMusic) -> MpMusic {
        let mut switching = false;
        if m.usingmultipletunes {
            let count = m.tracks.iter().filter(|t| t.enabled).take(2).count();
            switching = count >= 2;
        }
        MpMusic { switching, age60: 0, life60: 120, silence60: 0, death_timer240: 0, mpdeath_playing: false, lasttrack: -1 }
    }

    /// `mp_choose_track` (`mplayer.c:2882`): the tune a match plays next,
    /// setting its life. With no tracks (the harness), none: -1.
    pub fn mp_choose_track(&mut self, m: &MatchMusic, rng: &mut Rng) -> i32 {
        let numunlocked = m.tracks.len() as u32;
        if numunlocked == 0 {
            return -1;
        }
        // PD loops until it draws a different tune than the last; with one
        // unlocked tune it would loop forever (there are always seven).
        let different = |rng: &mut Rng, last: i32| {
            let mut t = m.tracks[(rng.random() % numunlocked) as usize];
            for _ in 0..1000 {
                if t.tracknum != last {
                    break;
                }
                t = m.tracks[(rng.random() % numunlocked) as usize];
            }
            t
        };
        let t = if m.usingmultipletunes {
            let numselected = m.tracks.iter().filter(|t| t.enabled).count() as u32;
            if numselected == 0 {
                different(rng, self.lasttrack)
            } else {
                let mut t;
                let mut guard = 0;
                loop {
                    let selectionindex = rng.random() % numselected;
                    let slot = m.tracks.iter().enumerate().filter(|(_, t)| t.enabled).nth(selectionindex as usize).map(|(i, _)| i);
                    // `tracknum == -1` returns g_MpTracks[0]: slot 0 is always it.
                    t = m.tracks[slot.unwrap_or(0)];
                    guard += 1;
                    if !(numselected > 1 && t.tracknum == self.lasttrack) || guard > 1000 {
                        break;
                    }
                }
                t
            }
        } else if m.slot < 0 {
            different(rng, self.lasttrack)
        } else {
            m.tracks[(m.slot as usize).min(m.tracks.len() - 1)]
        };
        self.lasttrack = t.tracknum;
        self.life60 = t.duration * 60;
        t.musicnum
    }

    /// `music_start_primary` (`game/music.c:272`): `PRIMARYTRACK()` is
    /// evaluated twice (the test, then the start), so a random tune draws
    /// twice and starts the second.
    pub fn music_start_primary(&mut self, m: &MatchMusic, rng: &mut Rng, secs: f32) -> Option<MusicCall> {
        if self.mp_choose_track(m, rng) >= 0 {
            let tracknum = self.mp_choose_track(m, rng);
            return Some(MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum, fadesecs: secs, volume: MusicVolume::Music });
        }
        None
    }
}

impl World {
    fn music(&mut self, c: MusicCall) {
        self.push_event(Event::Music(c));
    }

    fn music_start_primary(&mut self, secs: f32) {
        let m = self.setup.music.clone();
        if let Some(c) = self.mp.music.music_start_primary(&m, &mut self.rng, secs) {
            self.music(c);
        }
    }

    /// `lv_reset`'s `music_reset` (`lv.c:301`) and
    /// `music_set_stage_and_start_music` (`game/music.c:376`); the tune was
    /// drawn in `World::new`, at `lv_reset`'s place (before the setup's props
    /// and chrs), and is `first`.
    pub(crate) fn music_lv_reset(&mut self, first: Option<MusicCall>) {
        self.music(MusicCall::Reset);
        if let Some(c) = first {
            self.music(c);
        }
    }

    /// `music_start_mp_death` (`game/music.c:452`), for a player's death.
    pub(crate) fn music_start_mp_death(&mut self) {
        self.music(MusicCall::SaveInterval);
        self.music(MusicCall::Stop { tracktype: TRACKTYPE_MENU });
        self.music(MusicCall::Stop { tracktype: TRACKTYPE_DEATH });
        self.music(MusicCall::Stop { tracktype: TRACKTYPE_AMBIENT });
        // g_MusicNrgIsActive is never set in a match.
        self.music(MusicCall::Fade { tracktype: TRACKTYPE_PRIMARY, secs: 0.1, keep: true });
        // _music_start_mp_death saves and restores again (PD's nesting,
        // which loses the saved interval).
        self.music(MusicCall::SaveInterval);
        self.music(MusicCall::Start { tracktype: TRACKTYPE_DEATH, tracknum: MUSIC_DEATH_MP, fadesecs: 0.0, volume: MusicVolume::MaxSfxMusic });
        self.music(MusicCall::RestoreInterval);
        self.mp.music.death_timer240 = 1200;
        self.mp.music.mpdeath_playing = true;
        self.music(MusicCall::RestoreInterval);
    }

    /// `music_end_death` (`game/music.c:484`).
    fn music_end_death(&mut self) {
        self.music(MusicCall::Fade { tracktype: TRACKTYPE_DEATH, secs: 2.0, keep: false });
        self.music_start_primary(2.0);
        self.mp.music.mpdeath_playing = false;
    }

    /// `mp_end_match`'s `music_start_menu` (`mplayer.c:2433`):
    /// `menu_choose_music()` is `MUSIC_COMBATSIM_COMPLETE` for every root a
    /// match's menus can have (`menu.c:5559`, `:5567`).
    pub(crate) fn music_start_menu_at_end(&mut self) {
        self.music(MusicCall::StartTrackAsMenu(MUSIC_COMBATSIM_COMPLETE));
    }

    /// `music_tick` (`lib/music.c:411`) from `lv_tick`: the death tune's
    /// timer, multiple-tune switching, then `music_tick_events`.
    pub(crate) fn music_tick(&mut self) {
        let (lvupdate240, diffframe60, diffframe240) = (self.lv.lvupdate240, self.lv.diffframe60, self.lv.diffframe240);
        let switch_due = |m: &MpMusic| m.switching && m.life60 < m.age60;
        if self.mp.music.death_timer240 > 0 {
            self.mp.music.silence60 = 0;
            self.mp.music.death_timer240 -= lvupdate240;
            if self.mp.music.death_timer240 <= 0 {
                self.music_end_death();
                if switch_due(&self.mp.music) {
                    self.mp.music.age60 = 0;
                    self.music_next_track();
                }
            }
        } else if switch_due(&self.mp.music) {
            // Fade the old tune out, then two seconds of silence.
            self.mp.music.age60 = 0;
            self.music(MusicCall::Fade { tracktype: TRACKTYPE_PRIMARY, secs: 2.0, keep: true });
            self.mp.music.silence60 = 120;
        }
        if self.mp.music.switching {
            self.mp.music.age60 += diffframe60;
            if self.mp.music.silence60 > 0 {
                self.mp.music.silence60 -= diffframe60;
                if self.mp.music.silence60 <= 0 {
                    self.music_next_track();
                }
            }
        }
        self.music(MusicCall::Tick { diffframe240 });
    }

    /// Stop the menu, death and primary tunes and start the next primary
    /// (`lib/music.c:431`, `:452`).
    fn music_next_track(&mut self) {
        self.music(MusicCall::Stop { tracktype: TRACKTYPE_MENU });
        self.music(MusicCall::Stop { tracktype: TRACKTYPE_DEATH });
        self.music(MusicCall::Stop { tracktype: TRACKTYPE_PRIMARY });
        let m = self.setup.music.clone();
        let tracknum = self.mp.music.mp_choose_track(&m, &mut self.rng);
        self.music(MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum, fadesecs: 0.0, volume: MusicVolume::Music });
    }
}

#[cfg(test)]
mod tests {
    use pd_core::mp::MatchTrack;

    use super::*;

    fn tracks(n: usize) -> Vec<MatchTrack> {
        (0..n).map(|i| MatchTrack { tracknum: i as i32, musicnum: 58 + i as i32, duration: 120 + i as i32, enabled: false }).collect()
    }

    #[test]
    fn a_chosen_tune_draws_nothing_and_a_random_one_twice() {
        let fixed = MatchMusic { slot: 2, usingmultipletunes: false, tracks: tracks(7) };
        let mut m = MpMusic::new(&fixed);
        let mut rng = Rng::new(1);
        let before = rng.clone();
        let c = m.music_start_primary(&fixed, &mut rng, 0.0).unwrap();
        assert_eq!(c, MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum: 60, fadesecs: 0.0, volume: MusicVolume::Music });
        assert_eq!(rng.clone().random(), before.clone().random(), "no draws");
        assert_eq!((m.lasttrack, m.life60), (2, 122 * 60));

        let random = MatchMusic { slot: -1, ..fixed };
        let mut m = MpMusic::new(&random);
        let mut rng = Rng::new(1);
        let Some(MusicCall::Start { tracknum, .. }) = m.music_start_primary(&random, &mut rng, 0.0) else { panic!() };
        // The second draw (it can't repeat the first) is the one started.
        let mut r2 = Rng::new(1);
        let first = r2.random() % 7;
        let mut second = r2.random() % 7;
        while second == first {
            second = r2.random() % 7;
        }
        assert_eq!(tracknum, 58 + second as i32);
        assert!(MpMusic::new(&MatchMusic::default()).mp_choose_track(&MatchMusic::default(), &mut rng) < 0);
    }

    #[test]
    fn switching_needs_two_enabled_multiple_tunes() {
        let mut t = tracks(7);
        t[3].enabled = true;
        let one = MatchMusic { slot: -1, usingmultipletunes: true, tracks: t.clone() };
        assert!(!MpMusic::new(&one).switching);
        t[5].enabled = true;
        let two = MatchMusic { tracks: t, ..one };
        let mut m = MpMusic::new(&two);
        assert!(m.switching);
        let mut rng = Rng::new(3);
        for _ in 0..20 {
            let a = m.mp_choose_track(&two, &mut rng);
            assert!(a == 58 + 3 || a == 58 + 5, "only the enabled ones");
            let b = m.mp_choose_track(&two, &mut rng);
            assert_ne!(a, b, "never the same twice running");
        }
    }

    fn music_calls(events: &[Event]) -> Vec<MusicCall> {
        events.iter().filter_map(|e| if let Event::Music(c) = e { Some(*c) } else { None }).filter(|c| !matches!(c, MusicCall::Tick { .. })).collect()
    }

    fn world(music: MatchMusic) -> World {
        let (stage, level) = crate::testutil::complex_arc();
        let setup = pd_core::mp::MatchSetup { music, ..crate::harness::setup(1, 0, BOTDIFF_NORMAL) };
        crate::harness::world(stage, level, crate::testutil::res(), setup, crate::harness::NavChoice::Pd, 7, true).unwrap()
    }

    /// The match starts its tune at lv_reset (after music_reset); a death
    /// pauses it for the death tune; 1200 quarter-ticks later the death tune
    /// fades and the primary resumes; every frame asks for the queue's tick.
    #[test]
    fn a_death_plays_the_death_tune_for_five_seconds() {
        let mut t = tracks(7);
        t[0].musicnum = MUSIC_DARK_COMBAT;
        let mut w = world(MatchMusic { slot: 0, usingmultipletunes: false, tracks: t });
        w.step(4, &[]);
        let ev = w.take_events();
        let calls = music_calls(&ev);
        assert_eq!(calls[..2], [MusicCall::Reset, MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum: MUSIC_DARK_COMBAT, fadesecs: 0.0, volume: MusicVolume::Music }]);
        assert!(ev.contains(&Event::Music(MusicCall::Tick { diffframe240: 4 })));

        w.player_die(0);
        let mut calls = Vec::new();
        for _ in 0..3 {
            w.step(4, &[]);
            calls.extend(music_calls(&w.take_events()));
        }
        assert!(calls.contains(&MusicCall::Start { tracktype: TRACKTYPE_DEATH, tracknum: MUSIC_DEATH_MP, fadesecs: 0.0, volume: MusicVolume::MaxSfxMusic }), "{calls:?}");
        assert!(calls.contains(&MusicCall::Fade { tracktype: TRACKTYPE_PRIMARY, secs: 0.1, keep: true }));
        assert_eq!(w.mp.music.death_timer240, 1200 - 2 * 4);
        let mut frames = 0;
        let end = loop {
            w.step(4, &[]);
            frames += 1;
            let c = music_calls(&w.take_events());
            if !c.is_empty() {
                break c;
            }
            assert!(frames < 400);
        };
        assert_eq!(frames, 1200 / 4 - 2);
        assert_eq!(end, [MusicCall::Fade { tracktype: TRACKTYPE_DEATH, secs: 2.0, keep: false }, MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum: MUSIC_DARK_COMBAT, fadesecs: 2.0, volume: MusicVolume::Music }]);
    }

    /// With two tunes enabled, a tune ends after its duration: a 2 s fade,
    /// 2 s of silence, then the other one.
    #[test]
    fn multiple_tunes_switch_after_a_tunes_duration() {
        let mut t = tracks(7);
        t[1].enabled = true;
        t[4].enabled = true;
        t[1].duration = 3;
        t[4].duration = 3;
        let mut w = world(MatchMusic { slot: -1, usingmultipletunes: true, tracks: t });
        assert!(w.mp.music.switching);
        let mut log = Vec::new();
        for f in 0..60 * 12 {
            w.step(4, &[]);
            for c in music_calls(&w.take_events()) {
                log.push((f, c));
            }
        }
        let starts: Vec<(i32, i32)> = log.iter().filter_map(|&(f, c)| if let MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum, .. } = c { Some((f, tracknum)) } else { None }).collect();
        assert!(starts.len() >= 3, "{log:?}");
        for w2 in starts.windows(2) {
            assert_ne!(w2[0].1, w2[1].1, "the next tune is the other one");
            assert!([59, 62].contains(&w2[1].1));
        }
        let fade = log.iter().find(|(_, c)| matches!(c, MusicCall::Fade { tracktype: TRACKTYPE_PRIMARY, secs, keep: true } if *secs == 2.0)).expect("a fade");
        // The age passes the 3 s life (180) at frame 181; the silence (120)
        // counts down from that same frame, so the next starts 119 later.
        assert_eq!(fade.0, 181);
        assert_eq!(starts[1].0, 181 + 119);
    }
}

