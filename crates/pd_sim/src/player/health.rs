//! The player's health on screen, the sim half (`player.c`): the colour the
//! screen is tinted (`colourscreen*`: `player_set_fade_colour`,
//! `player_adjust_fade`, `player_update_colour_screen_properties`), the red
//! flash when hurt (`player_display_damage`, `g_DamageTypes`), the health bar's
//! timers (`player_display_health`, `player_tick_damage_and_health`,
//! `g_HealthDamageTypes`) and the death's fades. `pd_render::hud` draws the
//! bar and the tint from these.
//!
//! Source: the old repo's `pd_complex/health.rs`, split along PD's own
//! functions (it had folded the fades into one), checked against `player.c`.

/// `struct damagetype` (`g_DamageTypes`, `player.c:2390`): the flash's frames,
/// its strongest alpha and its colour.
#[derive(Clone, Copy, Debug)]
struct DamageType {
    flashstartframe: f32,
    flashfullframe: f32,
    flashendframe: f32,
    maxalpha: f32,
    rgb: [i32; 3],
}

const fn dt(end: f32, maxalpha: f32) -> DamageType {
    DamageType { flashstartframe: 0.0, flashfullframe: 5.0, flashendframe: end, maxalpha, rgb: [0x96, 0, 0] }
}

const G_DAMAGE_TYPES: [DamageType; 8] = [dt(40.0, 0.7), dt(40.0, 0.7), dt(30.0, 0.65), dt(25.0, 0.6), dt(22.0, 0.55), dt(19.0, 0.5), dt(17.0, 0.45), dt(15.0, 0.4)];

/// `g_HealthDamageTypes` (`player.c:2409`): openend, updatestart, updateend,
/// closestart, closeend.
const G_HEALTH_DAMAGE_TYPES: [[f32; 5]; 8] = [
    [20.0, 34.0, 46.0, 270.0, 285.0],
    [20.0, 37.0, 52.0, 250.0, 265.0],
    [20.0, 40.0, 58.0, 230.0, 245.0],
    [20.0, 43.0, 64.0, 210.0, 225.0],
    [20.0, 46.0, 70.0, 190.0, 205.0],
    [20.0, 49.0, 76.0, 170.0, 185.0],
    [20.0, 52.0, 82.0, 150.0, 165.0],
    [20.0, 55.0, 88.0, 130.0, 145.0],
];

/// `HEALTHSHOWMODE_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthShowMode {
    Hidden,
    Opening,
    Previous,
    Updating,
    Current,
    Closing,
}

/// The player fields these functions keep.
#[derive(Clone, Debug)]
pub struct Health {
    pub damageshowtime: f32,
    damagetype: usize,
    pub healthshowmode: HealthShowMode,
    pub healthshowtime: f32,
    healthdamagetype: usize,
    pub oldhealth: f32,
    pub apparenthealth: f32,
    /// The shield's share of the bar: `oldarmour`, `apparentarmour`.
    pub oldarmour: f32,
    pub apparentarmour: f32,
    /// `colourscreenred/green/blue/frac`: the tint over the view.
    pub colourscreen: [i32; 3],
    pub colourscreenfrac: f32,
    pub(super) colourfadetime60: f32,
    colourfadetimemax60: f32,
    colourfadeold: ([i32; 3], f32),
    colourfadenew: ([i32; 3], f32),
    /// `GUNSIGHTREASON_DAMAGE`: the sight is hidden while the flash plays.
    pub sightoff_damage: bool,
}

impl Default for Health {
    /// `player_load_defaults` (`player.c:681`).
    fn default() -> Self {
        Health {
            damageshowtime: -1.0,
            damagetype: 7,
            healthshowmode: HealthShowMode::Hidden,
            healthshowtime: -1.0,
            healthdamagetype: 7,
            oldhealth: 1.0,
            apparenthealth: 1.0,
            oldarmour: 0.0,
            apparentarmour: 0.0,
            colourscreen: [0xff, 0xff, 0xff],
            colourscreenfrac: 0.0,
            colourfadetime60: 0.0,
            colourfadetimemax60: -1.0,
            colourfadeold: ([0xff, 0xff, 0xff], 0.0),
            colourfadenew: ([0xff, 0xff, 0xff], 0.0),
            sightoff_damage: false,
        }
    }
}

impl Health {
    /// `player_set_fade_colour` (`player.c:2296`).
    pub fn player_set_fade_colour(&mut self, rgb: [i32; 3], frac: f32) {
        self.colourscreen = rgb;
        self.colourscreenfrac = frac;
    }

    /// `player_adjust_fade` (`player.c:2304`): fade to `rgb`/`frac` over `maxfadetime` ticks.
    pub fn player_adjust_fade(&mut self, maxfadetime: f32, rgb: [i32; 3], frac: f32) {
        self.colourfadetime60 = 0.0;
        self.colourfadetimemax60 = maxfadetime;
        self.colourfadeold = (self.colourscreen, self.colourscreenfrac);
        self.colourfadenew = (rgb, frac);
    }

    /// `player_is_fade_complete`.
    pub fn player_is_fade_complete(&self) -> bool {
        self.colourfadetimemax60 < 0.0
    }

    /// `player_update_colour_screen_properties` (`player.c:2336`).
    pub fn player_update_colour_screen_properties(&mut self, lvupdate60freal: f32) {
        if self.colourfadetimemax60 >= 0.0 {
            self.colourfadetime60 += lvupdate60freal;
            if self.colourfadetime60 < self.colourfadetimemax60 {
                let mult = self.colourfadetime60 / self.colourfadetimemax60;
                let (orgb, ofrac) = self.colourfadeold;
                let (nrgb, nfrac) = self.colourfadenew;
                self.colourscreenfrac = ofrac + (nfrac - ofrac) * mult;
                for k in 0..3 {
                    self.colourscreen[k] = orgb[k] + ((nrgb[k] - orgb[k]) as f32 * mult) as i32;
                }
                return;
            }
            self.colourscreen = self.colourfadenew.0;
            self.colourscreenfrac = self.colourfadenew.1;
            self.colourfadetimemax60 = -1.0;
        }
    }

    /// `player_display_health` (`player.c:2430`), called before the health (or
    /// the shield, `shieldfrac` = `player_get_shield_frac`) changes.
    pub fn player_display_health(&mut self, bondhealth: f32, shieldfrac: f32) {
        use HealthShowMode::*;
        match self.healthshowmode {
            Hidden | Closing => {
                self.oldhealth = bondhealth;
                self.oldarmour = shieldfrac;
            }
            Updating | Current => {
                self.oldhealth = self.apparenthealth;
                self.oldarmour = self.apparentarmour;
            }
            Opening | Previous => {}
        }
        match self.healthshowmode {
            Hidden => {
                self.healthshowtime = 0.0;
                self.healthshowmode = Opening;
            }
            Opening | Previous => {}
            Updating | Current => {
                self.healthshowtime = G_HEALTH_DAMAGE_TYPES[self.healthdamagetype][1];
                self.healthshowmode = Updating;
            }
            Closing => {
                self.healthshowtime = G_HEALTH_DAMAGE_TYPES[self.healthdamagetype][0] * self.player_get_health_bar_height_frac();
                self.healthshowmode = Opening;
            }
        }
    }

    /// `player_display_damage` (`player.c:2666`), with PD's bug: it indexes
    /// `g_DamageTypes` by `healthdamagetype`.
    pub fn player_display_damage(&mut self) {
        let full = G_DAMAGE_TYPES[self.healthdamagetype].flashfullframe;
        if self.damageshowtime >= full {
            self.damageshowtime = full;
            return;
        }
        if self.damageshowtime < 0.0 {
            self.damageshowtime = 0.0;
        }
    }

    /// `player_tick_damage_and_health` (`player.c:2474`). `shieldfrac` is
    /// `player_get_shield_frac`, `menuopen` `current_player_is_menu_open_in_solo_or_mp`.
    pub fn player_tick_damage_and_health(&mut self, bondhealth: f32, shieldfrac: f32, isdead: bool, menuopen: bool, lvupdate60freal: f32, diffframe60freal: f32) {
        if self.damageshowtime >= 0.0 {
            if self.damageshowtime == 0.0 {
                self.sightoff_damage = true;
                self.damagetype = ((bondhealth * 8.0) as i32).clamp(0, 7) as usize;
            }
            let d = G_DAMAGE_TYPES[self.damagetype];
            if !isdead && self.damageshowtime <= d.flashendframe {
                let inc = lvupdate60freal.min(5.0);
                self.damageshowtime += inc;
                if self.damageshowtime >= d.flashstartframe && self.damageshowtime <= d.flashendframe {
                    let done = self.damageshowtime - d.flashstartframe;
                    let total = d.flashendframe - d.flashstartframe;
                    let alpha = if done < d.flashfullframe { d.maxalpha * done / d.flashfullframe } else { d.maxalpha * (total - done) / (total - d.flashfullframe) };
                    self.player_set_fade_colour(d.rgb, alpha);
                }
            } else {
                self.damageshowtime = -1.0;
                self.player_set_fade_colour([0xff, 0xff, 0xff], 0.0);
                if !isdead {
                    self.sightoff_damage = false;
                }
            }
        }
        if self.healthshowmode != HealthShowMode::Hidden {
            use HealthShowMode::*;
            if self.healthshowmode == Opening {
                self.healthdamagetype = (((bondhealth + shieldfrac) * 8.0) as i32).clamp(0, 7) as usize;
            }
            let h = G_HEALTH_DAMAGE_TYPES[self.healthdamagetype];
            if !isdead {
                match self.healthshowmode {
                    Opening => {
                        self.apparenthealth = self.oldhealth;
                        self.apparentarmour = self.oldarmour;
                        self.healthshowtime += diffframe60freal;
                        if self.healthshowtime >= h[0] {
                            self.healthshowmode = Previous;
                        }
                    }
                    Previous => {
                        self.apparenthealth = self.oldhealth;
                        self.apparentarmour = self.oldarmour;
                        self.healthshowtime += diffframe60freal;
                        if menuopen {
                            self.healthshowmode = Current;
                        }
                        if self.healthshowtime >= h[1] {
                            self.healthshowmode = Updating;
                        }
                    }
                    Updating => {
                        self.healthshowtime += diffframe60freal;
                        let frac = ((self.healthshowtime - h[1]) / (h[2] - h[1])).clamp(0.0, 1.0);
                        if menuopen {
                            self.healthshowmode = Current;
                        }
                        let healthdiff = self.oldhealth - bondhealth;
                        let armourdiff = self.oldarmour - shieldfrac;
                        self.apparenthealth = self.oldhealth - frac * healthdiff;
                        self.apparentarmour = self.oldarmour - frac * armourdiff;
                        if self.healthshowtime >= h[2] {
                            self.healthshowmode = Current;
                        }
                    }
                    Current => {
                        self.apparenthealth = bondhealth;
                        self.apparentarmour = shieldfrac;
                        self.healthshowtime += diffframe60freal;
                        if menuopen {
                            self.healthshowtime = h[3];
                        }
                        if self.healthshowtime >= h[3] && !menuopen {
                            self.healthshowmode = Closing;
                            self.healthshowtime = h[3];
                        }
                    }
                    Closing => {
                        self.healthshowtime += diffframe60freal;
                        if self.healthshowtime >= h[4] {
                            self.healthshowtime = -1.0;
                            self.healthshowmode = Hidden;
                        }
                    }
                    Hidden => {}
                }
            } else {
                self.healthshowtime = -1.0;
                self.healthshowmode = Hidden;
            }
        }
    }

    /// `player_get_health_bar_height_frac` (`player.c:4885`).
    pub fn player_get_health_bar_height_frac(&self) -> f32 {
        let h = G_HEALTH_DAMAGE_TYPES[self.healthdamagetype];
        match self.healthshowmode {
            HealthShowMode::Hidden => 0.0,
            HealthShowMode::Opening => self.healthshowtime / h[0],
            HealthShowMode::Closing => 1.0 - (self.healthshowtime - h[3]) / (h[4] - h[3]),
            _ => 1.0,
        }
    }

    /// `player_is_health_visible`.
    pub fn player_is_health_visible(&self) -> bool {
        self.healthshowmode != HealthShowMode::Hidden
    }
}

impl super::Player {
    /// `player_start_chr_fade` (`player.c:2354`), from the chr's current alpha.
    pub fn player_start_chr_fade(&mut self, duration60: f32, targetfrac: f32) {
        self.bondfadetime60 = 0.0;
        self.bondfadetimemax60 = duration60;
        self.bondfadefracold = self.chrfadefrac;
        self.bondfadefracnew = targetfrac;
    }

    /// `player_tick_chr_fade` (`player.c:2366`). The world copies
    /// [`super::Player::chrfadefrac`] to the player's chr (`chr->fadealpha`).
    pub(super) fn player_tick_chr_fade(&mut self, lvupdate60freal: f32) {
        if self.bondfadetimemax60 >= 0.0 {
            self.bondfadetime60 += lvupdate60freal;
            let frac = if self.bondfadetime60 < self.bondfadetimemax60 {
                self.bondfadefracold + (self.bondfadefracnew - self.bondfadefracold) * self.bondfadetime60 / self.bondfadetimemax60
            } else {
                self.bondfadetimemax60 = -1.0;
                self.bondfadefracnew
            };
            // chr->fadealpha = (s8)(frac * 255): kept as the fraction.
            self.chrfadefrac = frac;
        }
    }

    /// `player_die_by_shooter`'s player half (`player.c:4807`): dead, the guns
    /// thrown (`bgun_handle_player_dead`). The world scores it and uncloaks.
    pub(crate) fn player_set_dead(&mut self) {
        self.isdead = true;
        self.isdead2 = false;
    }

    /// `player_render_hud`'s dead branch (`player.c:4546`), normal
    /// multiplayer: the red wash while the head falls, then a fade to black
    /// and the body fading out, then any of A, Z or START (held, `0xb000`)
    /// starts a new life, while the match isn't paused or ending
    /// (`canrestart`: `!mp_is_paused() && g_NumReasonsToEndMpMatch == 0`).
    /// True on the frame the death begins: the world starts the death tune
    /// (`music_start_mp_death`).
    pub fn player_tick_death(&mut self, input: &super::PlayerInput, canrestart: bool) -> bool {
        if !self.isdead {
            return false;
        }
        let mut started = false;
        if !self.deathanimfinished {
            if !self.isdead2 {
                // pak_disable_rumble_for_player, then music_start_mp_death.
                self.rumble.disable();
                self.isdead2 = true;
                started = true;
            } else if self.redbloodfinished {
                self.health.player_set_fade_colour([0x96, 0, 0], 0.705_882_37);
            } else {
                self.redbloodfinished = true;
            }
        }
        if self.head_anim_done() && self.redbloodfinished {
            if !self.deathanimfinished {
                self.deathanimfinished = true;
                self.health.player_adjust_fade(60.0, [0, 0, 0], 1.0);
                self.player_start_chr_fade(120.0, 0.0);
            }
            if self.health.player_is_fade_complete() && (input.a_held || input.fire || input.start) && canrestart {
                self.dostartnewlife = true;
            }
        }
        started
    }

    /// The parts of `player_start_new_life` (`player.c:495`),
    /// `player_load_defaults` (`:673`) and `player_spawn` (`:934`) the death
    /// undoes: health, fades, the death flags.
    pub(super) fn reset_life(&mut self) {
        self.dostartnewlife = false;
        self.isdead = false;
        self.isdead2 = false;
        self.bondhealth = 1.0;
        self.health = Health::default();
        self.shieldshow.time = -1.0;
        // player_start_new_life's pak_enable_rumble_for_player (`player.c:500`).
        self.rumble.enable();
        self.health.colourfadetime60 = -1.0;
        self.bondfadetime60 = -1.0;
        self.bondfadetimemax60 = -1.0;
        self.bondfadefracold = 0.0;
        self.bondfadefracnew = 0.0;
        self.deathanimfinished = false;
        self.redbloodfinished = false;
        self.startnewbonddie = true;
        self.bondbreathing = 0.0;
        self.insightaimmode = false;
        self.inlift = false;
        self.lift = None;
        // player_start_new_life: normal multiplayer fades the chr in.
        self.player_start_chr_fade(120.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hit_flashes_red_and_the_bar_opens_slides_and_closes() {
        let mut h = Health::default();
        let mut health = 1.0;
        h.player_display_health(health, 0.0);
        health -= 0.25;
        h.player_display_damage();
        let mut reddest = 0.0f32;
        let mut seen = vec![];
        for _ in 0..400 {
            h.player_tick_damage_and_health(health, 0.0, false, false, 1.0, 1.0);
            reddest = reddest.max(h.colourscreenfrac);
            if seen.last() != Some(&h.healthshowmode) {
                seen.push(h.healthshowmode);
            }
        }
        // Full health hit: damage type 6 (0.75 × 8), a 0.45 flash at most.
        assert!((reddest - 0.45).abs() < 0.02, "{reddest}");
        assert_eq!(h.colourscreenfrac, 0.0, "the flash ended");
        use HealthShowMode::*;
        assert_eq!(seen, vec![Opening, Previous, Updating, Current, Closing, Hidden]);
        assert!((h.apparenthealth - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_fade_reaches_its_colour_and_reports_done() {
        let mut h = Health::default();
        h.player_set_fade_colour([0x96, 0, 0], 0.705_882_4);
        h.player_adjust_fade(60.0, [0, 0, 0], 1.0);
        assert!(!h.player_is_fade_complete());
        for _ in 0..61 {
            h.player_update_colour_screen_properties(1.0);
        }
        assert!(h.player_is_fade_complete());
        assert_eq!((h.colourscreen, h.colourscreenfrac), ([0, 0, 0], 1.0));
    }
}

/// The first-person shield flash (`shieldshowtime`, `shieldshowrnd`,
/// `shieldshowrot`): a hit the shield took washes the view with its shimmer
/// for a second (`player_display_shield`, `player_render_shield`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShieldShow {
    /// Ticks into the flash; -1: none.
    pub time: f32,
    pub rnd: u32,
    pub rot: f32,
}

impl Default for ShieldShow {
    fn default() -> Self {
        // player_reset_...: shieldshowtime = -1 (`player.c:726`).
        ShieldShow { time: -1.0, rnd: 0, rot: 0.0 }
    }
}

/// What `player_render_shield` draws this frame, in the view's pixels: the
/// shield texture stretched over a box centred at `centre` with half-sizes
/// `half`, flipped and mirrored by `rnd`'s low bits, `TEXEL0 × ENV` over the
/// view with the prim alpha as the floor of the coverage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShieldFlash {
    pub centre: [f32; 2],
    pub half: [f32; 2],
    pub flip: bool,
    pub mirror_s: bool,
    pub mirror_t: bool,
    /// ENV rgb, ENV alpha, PRIM alpha.
    pub env: [i32; 4],
    pub primalpha: i32,
}

impl ShieldShow {
    /// `player_display_shield` (`player.c:4334`): (re)start the flash; a new
    /// one draws a new pattern.
    pub fn player_display_shield(&mut self, rng: &mut pd_core::rng::Rng, thisframestart240: i32) {
        if self.time < 0.0 {
            let r = ((self.rnd >> 16) % 200) * 4 + 800;
            self.rnd = rng.random();
            self.rot = (thisframestart240.rem_euclid(r as i32)) as f32;
        }
        self.time = 0.0;
    }

    /// `player_render_shield` (`player.c:4349`) for a view `[left, top,
    /// width, height]` with the shield at `shield` (0..8): the frame's flash,
    /// and the flash moved on by `lvupdate60freal`.
    pub fn player_render_shield(&mut self, view: [f32; 4], shield: f32, lvupdate60freal: f32) -> Option<ShieldFlash> {
        if self.time < 0.0 {
            return None;
        }
        let [left, top, width, height] = view;
        let rnd = self.rnd;
        let maxrotf = (((rnd >> 16) % 200) * 4 + 800) as f32;
        let mut f20 = (60.0 - self.time) * (1.0 / 60.0);
        self.rot += lvupdate60freal * (0.8 + 2.0 * f20 * f20);
        if self.rot >= maxrotf {
            self.rot -= maxrotf;
        }
        let tau = pd_core::math::baddtor(360.0);
        f20 = ((self.rot * (tau / maxrotf)).sin() + 1.0) * 0.5;
        let cx = left + width * f20;
        f20 = ((self.rot * (tau / maxrotf)).cos() + 1.0) * 0.5;
        let cy = top + height * f20;
        let t = self.time;
        let hx = width * (1.0 + 0.002 * ((rnd >> 20) % 100) as f32 + (t * (0.2 + 0.002 * (rnd % 100) as f32) * (1.0 / 60.0)));
        let hy = height * (1.0 + 0.002 * ((rnd >> 24) % 100) as f32 + (t * (0.2 + 0.002 * ((rnd >> 8) % 100) as f32) * (1.0 / 60.0)));
        let [mut r, mut g, mut b] = crate::fx::shieldhit::shieldhit_health_to_rgb(shield);
        // PD leaves f20 as the pattern's y when the time is 60 or more.
        if t < 30.0 {
            let f = 1.0 - t * (1.0 / 120.0);
            f20 = 50.0 * f * f * f;
        } else if t < 60.0 {
            f20 = (t - (1.0 / 120.0)) * (1.0 / 120.0);
            f20 *= -30.0;
        }
        let add = f20 as i32;
        r = (r + add).clamp(0, 255);
        g = (g + add).clamp(0, 255);
        b = (b + add).clamp(0, 255);
        let fade = 1.0 - t * (1.0 / 60.0);
        let flash = ShieldFlash {
            centre: [cx, cy],
            half: [hx, hy],
            flip: rnd & 1 != 0,
            mirror_s: rnd & 2 != 0,
            mirror_t: rnd & 4 != 0,
            env: [r, g, b, (200.0 * fade) as i32],
            primalpha: (175.0 * fade * fade) as i32,
        };
        self.time += lvupdate60freal;
        if self.time > 60.0 {
            self.time = -1.0;
        }
        Some(flash)
    }
}
