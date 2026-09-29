//! `nbomb.c`: the N-Bomb storm (NTSC final).
//!
//! A black, textured geodesic dome that grows to 5 m in 80 ticks, breathes,
//! spins, darkens every room its box touches and makes whoever stands inside
//! dizzy, then fades from 310 ticks and is gone after 370. Standing inside it
//! paints the storm texture over the screen (`nbomb_render_overlay`, in
//! `pd_render`). A ship's hum plays while any storm is young.
//!
//! Source: the old repo's `pd_guns/nbomb.rs` (its geometry is `pd_render`'s now).

use glam::Vec3;
use pd_core::events::Event;

use crate::world::World;

/// `g_Nbombs[6]`.
pub const MAX_NBOMBS: usize = 6;

/// The hum's sound handle (`g_NbombAudioHandle`); a storm's two roars use the
/// next twelve.
pub const NBOMB_HUM_HANDLE: u32 = 0x100;

/// `struct nbomb`.
#[derive(Clone, Copy, Debug)]
pub struct Nbomb {
    pub age240: i32,
    pub pos: Vec3,
    pub radius: f32,
    /// The dome's spin, in 2048ths of a turn.
    pub unk14: i32,
    pub unk18: f32,
    /// `ownerprop`: the player who set it off (`chr_damage_by_dizziness`' attacker).
    pub owner: Option<usize>,
}

impl Default for Nbomb {
    fn default() -> Self {
        Nbomb { age240: -1, pos: Vec3::ZERO, radius: 0.0, unk14: 0, unk18: 0.0, owner: None }
    }
}

/// `g_Nbombs`, `g_NbombsActive` and the hum.
#[derive(Clone, Default)]
pub struct Nbombs {
    pub bombs: [Nbomb; MAX_NBOMBS],
    pub active: bool,
    /// `g_NbombAudioHandle` is playing.
    pub humming: bool,
}

/// `nbomb_calculate_alpha` (`nbomb.c:291`): 127, fading out from 310 to 350.
pub fn nbomb_calculate_alpha(n: &Nbomb) -> i32 {
    if n.age240 > 310 {
        if n.age240 < 350 {
            (350 * 127 - n.age240 * 127) / 40
        } else {
            0
        }
    } else {
        127
    }
}

impl Nbombs {
    pub fn any(&self) -> bool {
        self.bombs.iter().any(|b| b.age240 >= 0)
    }

    /// `nbomb_render_overlay`'s test (`nbomb.c:784`): the strongest storm the
    /// camera stands in, as its alpha (0..127).
    pub fn overlay_alpha(&self, campos: Vec3) -> Option<i32> {
        let mut out: Option<i32> = None;
        for n in &self.bombs {
            if n.age240 >= 0 && n.age240 <= 350 && (campos - n.pos).length() < n.radius {
                out = Some(out.unwrap_or(0).max(nbomb_calculate_alpha(n)));
            }
        }
        out
    }
}

impl World {
    /// `nbomb_create_storm` (`nbomb.c:663`): a free slot or the oldest storm's,
    /// and two rocket roars at 0.4 pitch.
    pub(crate) fn nbomb_create_storm(&mut self, pos: Vec3, owner: Option<usize>) {
        let n = &mut self.props.nbombs;
        let mut oldest240 = -1;
        let mut index = 0;
        n.active = true;
        for (i, b) in n.bombs.iter().enumerate() {
            if b.age240 == -1 {
                index = i;
                break;
            }
            if b.age240 > oldest240 {
                index = i;
                oldest240 = b.age240;
            }
        }
        n.bombs[index] = Nbomb { age240: 0, pos, owner, ..Nbomb::default() };
        for k in 0..2 {
            // snd_start(SFXNUM_0001_LAUNCH_ROCKET) with the pitch posted at 0.4.
            let handle = NBOMB_HUM_HANDLE + 1 + index as u32 * 2 + k;
            self.push_event(Event::HandleSound { handle, sound: 0x0001, pitch: 0.4, volume: 1.0, pan: 0.0 });
        }
    }

    /// `nbombs_tick` (`nbomb.c:567`) with `nbomb_tick` and `nbomb_inflict_damage`.
    pub(crate) fn nbombs_tick(&mut self) {
        let lv = self.lv.clone();
        if lv.lvupdate240 != 0 {
            self.props.nbombs.active = false;
        }
        let mut youngest240 = 20000;
        for i in 0..MAX_NBOMBS {
            if lv.lvupdate240 != 0 && self.props.nbombs.bombs[i].age240 >= 0 {
                self.nbomb_tick(i);
                let b = &self.props.nbombs.bombs[i];
                if b.age240 < youngest240 {
                    youngest240 = b.age240;
                }
                self.props.nbombs.active = true;
            }
        }
        if youngest240 < 350 {
            if lv.lvupdate240 != 0 {
                if !self.props.nbombs.humming {
                    self.props.nbombs.humming = true;
                    self.push_event(Event::HandleSound { handle: NBOMB_HUM_HANDLE, sound: 0x810c, pitch: 0.4, volume: 1.0, pan: 0.0 });
                }
                // sndp_post_event: full, fading from 300 to 350; the pitch wobbles
                // with menu_get_sin_osc_frac(20).
                let mut volume = 1.0;
                if youngest240 > 300 {
                    volume = 1.0 - (youngest240 - 300) as f32 / 50.0;
                }
                let osc = (((20.0 * self.frac20) + 20.0 * self.frac20) * pd_core::math::dtor(180.0)).sin() / 2.0 + 0.5;
                let speed = osc * 0.02 + 0.4;
                self.push_event(Event::SoundParams { handle: NBOMB_HUM_HANDLE, pitch: speed, volume });
            } else if self.props.nbombs.humming {
                self.props.nbombs.humming = false;
                self.push_event(Event::StopSound { handle: NBOMB_HUM_HANDLE });
            }
        } else if self.props.nbombs.humming {
            self.props.nbombs.humming = false;
            self.push_event(Event::StopSound { handle: NBOMB_HUM_HANDLE });
        }
        if lv.lvupdate240 == 0 {
            // Paused: the roars stop.
            for i in 0..MAX_NBOMBS {
                if self.props.nbombs.bombs[i].age240 >= 0 {
                    for k in 0..2 {
                        self.push_event(Event::StopSound { handle: NBOMB_HUM_HANDLE + 1 + i as u32 * 2 + k });
                    }
                }
            }
        }
    }

    /// `nbomb_tick` (`nbomb.c:522`).
    fn nbomb_tick(&mut self, i: usize) {
        let increment = (self.lv.lvupdate240 + 2) >> 2;
        {
            let n = &mut self.props.nbombs.bombs[i];
            n.age240 += increment;
            if n.age240 < 80 {
                n.radius = (n.age240 as f32 / 80.0).sqrt().sqrt();
                n.unk18 = 0.0;
            } else {
                n.radius = ((n.age240 - 80) as f32 * 0.052_333_336).sin() * 0.05 + 1.0;
                n.unk18 = (n.age240 - 80) as f32 / 270.0 * 3.0;
            }
            n.radius *= 500.0;
        }
        self.nbomb_inflict_damage(i);
        let n = &mut self.props.nbombs.bombs[i];
        let age60 = (n.age240 / 4).min(40);
        n.unk14 = (n.unk14 + increment * age60) % 0x800;
        if n.age240 > 370 {
            n.age240 = -1;
        }
    }

    /// `nbomb_inflict_damage` (`nbomb.c:431`): every room the dome's box touches
    /// darkens (`room_flash_lighting(room, -38, -180)`), and every chr inside the
    /// dome takes `chr_damage_by_dizziness(0.01 × lvupdate60freal)` and
    /// uncloaks.
    fn nbomb_inflict_damage(&mut self, i: usize) {
        let n = self.props.nbombs.bombs[i];
        if self.lv.lvupdate240 <= 0 || n.age240 > 350 {
            return;
        }
        let (bbmin, bbmax) = (n.pos - Vec3::splat(n.radius), n.pos + Vec3::splat(n.radius));
        let level = self.level.clone();
        let mut index = 0;
        for &room in level.geom.rooms.iter().filter(|&&r| r >= 1) {
            let Some((lo, hi)) = level.room_bbox(room) else { continue };
            let apart = bbmax.x < lo.x || bbmin.x > hi.x || bbmax.y < lo.y || bbmin.y > hi.y || bbmax.z < lo.z || bbmin.z > hi.z;
            if !apart && index < 52 {
                index += 1;
                self.room_flash_lighting(room, -38, -180);
            }
        }
        let damage = 0.01 * self.lv.lvupdate60freal;
        for c in 0..self.chrs.len() {
            if (self.chrs[c].pos - n.pos).length() < n.radius {
                // chr_damage_by_dizziness(chr, damage, {0, 0, 0}, WEAPON_NBOMB, ownerprop).
                self.chr_damage(c, damage, Vec3::ZERO, crate::chr::DamageFrom::new(n.owner, pd_core::ids::WEAPON_NBOMB, pd_core::ids::FUNC_PRIMARY), crate::chr::HITPART_GENERAL, false, false);
                self.chr_uncloak_chr(c, true);
            }
        }
    }
}
