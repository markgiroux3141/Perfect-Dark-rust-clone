//! Frame timing, as PD computes it: `frametime_apply` (`timing.c:14`) turns the
//! time since the last frame into `diffframe60`/`diffframe240`, then `lv_tick`
//! (`lv.c:1993`) makes this frame's `lvupdate240` (quarter-ticks the world
//! advances) with the pause, the slow-motion option and Combat Boost, carries the
//! remainder into `lvupdate60`, and counts `lvframe60`/`lvframe240`/`lvframenum`.
//! The menus run on `diffframe60`; the world on `lvupdate240`.
//!
//! There is exactly one `Lv` per world. The spikes had three (`pd_guns::bgun::Lv`,
//! `pd_spike::sim::Globals`, `pd_menu` `Vars`), which is why Combat Boost slowed
//! the player but not the bots.
//!
//! NTSC: `PALUPF(x)` is `x`, so every `*freal` equals its `*f`.

/// `SLOWMOTION_*` (`constants.h:3805`), from `MPOPTION_SLOWMOTION_ON`/`_SMART`
/// (`lv_get_slow_motion_type`, `lv.c:1945`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SlowMotion {
    #[default]
    Off,
    On,
    Smart,
}

/// What `lv_tick` reads from the rest of the game to decide this frame's step.
#[derive(Clone, Copy, Debug, Default)]
pub struct LvTickIn {
    /// `lv_is_paused() || mp_is_paused()`.
    pub paused: bool,
    pub slowmo: SlowMotion,
    /// `g_Vars.speedpillon`: a Combat Boost is running.
    pub speedpillon: bool,
    /// Smart slow motion: a living player's room is on another living player's
    /// screen (`bg_room_is_on_player_screen`, `lv.c:2066`). The world computes it.
    pub enemy_on_screen: bool,
}

/// `g_Vars`' timing fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Lv {
    pub diffframe60: i32,
    pub diffframe60f: f32,
    pub diffframe60freal: f32,
    pub diffframe240: i32,
    pub diffframe240f: f32,
    pub diffframe240freal: f32,
    pub thisframestart240: i32,
    pub prevframestart240: i32,

    pub lvupdate240: i32,
    pub lvupdate240rem: i32,
    pub lvupdate60: i32,
    pub lvupdate60f: f32,
    pub lvupdate60freal: f32,
    pub lvupdate60frealprev: f32,
    pub lvframe60: i32,
    pub lvframe240: i32,
    pub lvframenum: i32,
}

impl Default for Lv {
    fn default() -> Self {
        Lv::new()
    }
}

impl Lv {
    /// `vars_init` (`varsinit.c:13`) then `lv_reset`'s timing (`lv.c:264`).
    pub fn new() -> Lv {
        Lv {
            diffframe60: 1,
            diffframe60f: 1.0,
            diffframe60freal: 1.0,
            diffframe240: 4,
            diffframe240f: 4.0,
            diffframe240freal: 4.0,
            thisframestart240: 0,
            prevframestart240: -1,
            lvupdate240: 4,
            // varsinit.c:31. lv_reset leaves it alone, so the carry starts at a
            // half-step and lvupdate60 rounds to nearest.
            lvupdate240rem: 2,
            lvupdate60: 0,
            lvupdate60f: 1.0,
            lvupdate60freal: 1.0,
            lvupdate60frealprev: 1.0,
            lvframe60: 0,
            lvframe240: 0,
            lvframenum: 0,
        }
    }

    /// `frametime_apply(diffframe60, diffframe240, ...)` (`timing.c:14`). The
    /// engine's fixed clock supplies whole 60 Hz and 240 Hz ticks, which
    /// `frametime_calculate` would have rounded from the CPU counter.
    pub fn frametime_apply(&mut self, diffframe60: i32, diffframe240: i32) {
        self.diffframe60 = diffframe60;
        self.diffframe60f = diffframe60 as f32;
        self.diffframe60freal = self.diffframe60f;
        self.prevframestart240 = self.thisframestart240;
        self.thisframestart240 += diffframe240;
        self.diffframe240 = diffframe240;
        self.diffframe240f = diffframe240 as f32;
        self.diffframe240freal = self.diffframe240f;
    }

    /// The timing half of `lv_tick` (`lv.c:2037-2120`). The Combat Simulator has
    /// no cutscenes, so `g_Vars.in_cutscene` is false throughout.
    pub fn tick(&mut self, input: LvTickIn) {
        if input.paused {
            self.lvupdate240 = 0;
        } else {
            self.lvupdate240 = self.diffframe240;
            let cap = |lv: &mut Lv, max: i32| {
                if lv.lvupdate240 > max {
                    lv.lvupdate240 = max;
                }
            };
            match input.slowmo {
                SlowMotion::On => {
                    if !input.speedpillon {
                        cap(self, 4);
                    }
                }
                SlowMotion::Smart => {
                    // mplayerisrunning is always true here.
                    if !input.speedpillon {
                        cap(self, if input.enemy_on_screen { 4 } else { 8 });
                    }
                }
                SlowMotion::Off => {
                    if input.speedpillon {
                        cap(self, 4);
                    }
                }
            }
        }
        self.lvupdate60 = self.lvupdate240 + self.lvupdate240rem;
        self.lvupdate240rem = self.lvupdate60 & 3;
        self.lvupdate60 >>= 2;
        if self.lvupdate240 > 0 {
            self.lvframenum += 1;
        }
        self.lvupdate60f = self.lvupdate240 as f32 * 0.25;
        self.lvframe60 += self.lvupdate60;
        self.lvframe240 += self.lvupdate240;
        self.lvupdate60frealprev = self.lvupdate60freal;
        self.lvupdate60freal = self.lvupdate60f;
    }

    /// One frame: `frametime_apply` for a frame `diffframe240` quarter-ticks
    /// long, then `lv_tick`.
    pub fn frame(&mut self, diffframe240: i32, input: LvTickIn) {
        self.frametime_apply((diffframe240 + 2) / 4, diffframe240);
        self.tick(input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_60_30_and_20_hz_every_frame_advances_1_2_and_3_ticks() {
        for (d240, d60) in [(4, 1), (8, 2), (12, 3)] {
            let mut lv = Lv::new();
            for n in 1..=10 {
                lv.frame(d240, LvTickIn::default());
                assert_eq!((lv.lvupdate240, lv.lvupdate60, lv.lvupdate240rem), (d240, d60, 2));
                assert_eq!(lv.lvframe60, n * d60);
                assert_eq!(lv.diffframe60, d60);
            }
            assert_eq!(lv.lvframenum, 10);
        }
    }

    #[test]
    fn the_remainder_carries_odd_quarter_ticks() {
        let mut lv = Lv::new();
        let mut total60 = 0;
        for _ in 0..8 {
            lv.frame(5, LvTickIn::default());
            total60 += lv.lvupdate60;
        }
        // 40 quarter-ticks + the initial half-step carry of 2 = 10 whole ticks, 2 left.
        assert_eq!((total60, lv.lvupdate240rem), (10, 2));
        assert_eq!(lv.lvframe240, 40);
    }

    #[test]
    fn combat_boost_caps_the_frame_only_with_slow_motion_off() {
        let boost = LvTickIn { speedpillon: true, ..Default::default() };
        let mut lv = Lv::new();
        lv.frame(12, boost);
        assert_eq!(lv.lvupdate240, 4);
        lv.frame(12, LvTickIn { slowmo: SlowMotion::On, ..boost });
        assert_eq!(lv.lvupdate240, 12, "a boost lifts the slow-motion cap");
        lv.frame(12, LvTickIn { slowmo: SlowMotion::On, ..Default::default() });
        assert_eq!(lv.lvupdate240, 4);
        lv.frame(12, LvTickIn { slowmo: SlowMotion::Smart, ..Default::default() });
        assert_eq!(lv.lvupdate240, 8);
        lv.frame(12, LvTickIn { slowmo: SlowMotion::Smart, enemy_on_screen: true, ..Default::default() });
        assert_eq!(lv.lvupdate240, 4);
    }

    #[test]
    fn a_paused_frame_does_not_advance_the_world() {
        let mut lv = Lv::new();
        lv.frame(4, LvTickIn::default());
        lv.frame(4, LvTickIn { paused: true, ..Default::default() });
        assert_eq!((lv.lvupdate240, lv.lvupdate60, lv.lvframenum, lv.lvframe60), (0, 0, 1, 1));
        assert_eq!(lv.diffframe60, 1, "the menus still run");
    }
}
