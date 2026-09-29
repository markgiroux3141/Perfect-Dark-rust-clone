//! `propsnd.c`: how loud and where a sound at a world position is, for the
//! players: `ps_calculate_vol` (the nearest player's distance through the
//! sound's audio-config falloff) and `ps_calculate_pan` (by the angle from the
//! player's facing, one player only; with more, centre).
//!
//! An `SFXMAP_*` ref carries its audio config's three distances (full volume
//! to `dist1`, a square-root fade to 1000/32767 at `dist2`, linear to silence
//! at `dist3`), read from `sfx/manifest.json`; a bare sound takes the caller's.
//!
//! `// SUBST:` PD measures the distance through rooms and portals
//! (`lights_find_distance_through_rooms_with_limit`) / a straight line.

use std::collections::HashMap;

use glam::Vec3;
use pd_core::assets::AssetDir;
use pd_core::math::{atan2f, dtor};

/// `AL_VOL_FULL`.
const AL_VOL_FULL: f32 = 32767.0;
/// `AL_PAN_CENTER`.
const AL_PAN_CENTER: i32 = 64;

/// The default distances of `ps_create` and `ps_apply_vol_pan` callers.
pub const DEFAULT_DISTS: [f32; 3] = [400.0, 2500.0, 3000.0];

/// One `g_AudioConfigs` row, as a sound ref's mapping reaches it.
#[derive(Clone, Copy, Debug)]
pub struct AudioConfig {
    pub dist: [f32; 3],
    /// `AUDIOCONFIGFLAG_SPECIALPAN`: pan only fades in past `dist1`.
    pub specialpan: bool,
}

/// Every `SFXMAP_*` ref's audio config.
#[derive(Clone, Debug, Default)]
pub struct AudioConfigs {
    map: HashMap<u16, AudioConfig>,
}

impl AudioConfigs {
    /// From `sfx/manifest.json`'s `SFXMAP_*` entries.
    pub fn load(assets: &AssetDir) -> Result<AudioConfigs, String> {
        let m: serde_json::Value = assets.read_json(&assets.sfx_manifest())?;
        let mut map = HashMap::new();
        for (k, e) in m.as_object().ok_or("sfx/manifest.json: not an object")? {
            let Some(rest) = k.strip_prefix("SFXMAP_") else { continue };
            let Some(id) = rest.get(..4).and_then(|h| u16::from_str_radix(h, 16).ok()) else { continue };
            let Some(d) = e.pointer("/config/dist").and_then(|d| d.as_array()) else { continue };
            let v: Vec<f32> = d.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
            if v.len() == 3 {
                let specialpan = e.pointer("/config/flags").and_then(|f| f.as_str()).is_some_and(|f| f.contains("SPECIALPAN"));
                map.insert(id, AudioConfig { dist: [v[0], v[1], v[2]], specialpan });
            }
        }
        Ok(AudioConfigs { map })
    }

    pub fn get(&self, sound: u16) -> Option<&AudioConfig> {
        self.map.get(&sound)
    }
}

/// `ps_calculate_volume_from_distance` (`propsnd.c:80`), as a fraction of full.
pub fn ps_calculate_volume_from_distance(playerdist: f32, dist: [f32; 3]) -> f32 {
    let (d1, d2, d3) = (dist[0].min(5501.0), dist[1].min(5801.0), dist[2].min(6000.0));
    let mut result = 0.0;
    if playerdist < dist[2] {
        result = if playerdist < d1 {
            AL_VOL_FULL
        } else if playerdist < d2 {
            AL_VOL_FULL - ((((playerdist - d1) / (d2 - d1)).sqrt() * (AL_VOL_FULL - 1000.0)) as i32) as f32
        } else {
            (((d3 - playerdist) * 1000.0 / (d3 - d2)) as i32) as f32
        };
    }
    let result = result.min(AL_VOL_FULL);
    if result < 40.0 {
        0.0
    } else {
        result / AL_VOL_FULL
    }
}

/// A listener: a player's camera position and facing (`vv_theta`, degrees).
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    pub pos: Vec3,
    pub theta: f32,
}

/// `ps_calculate_pan3` (`propsnd.c:1195`), stereo: 0 in front and behind,
/// ±90 to the sides, as an `AL_PAN` 0..127.
fn ps_calculate_pan3(mut degrees: i32) -> i32 {
    while degrees >= 180 {
        degrees -= 360;
    }
    while degrees < -180 {
        degrees += 360;
    }
    let mut absdegrees = degrees.abs();
    if absdegrees > 90 {
        absdegrees = 180 - absdegrees;
    }
    let dir = if degrees > 0 { 1 } else { -1 };
    degrees = dir * absdegrees;
    if (-90..=90).contains(&degrees) {
        AL_PAN_CENTER + (degrees as f32 * 0.7) as i32
    } else {
        let absdegrees = degrees.abs();
        128 + (AL_PAN_CENTER as f32 + (180 - absdegrees) as f32 * dir as f32 * 0.7) as i32
    }
}

/// `ps_calculate_pan2` (`propsnd.c:1270`).
fn ps_calculate_pan2(pos: Vec3, arg2: f32, listeners: &[Listener]) -> i32 {
    if listeners.len() >= 2 {
        return AL_PAN_CENTER;
    }
    let Some(l) = listeners.first() else { return AL_PAN_CENTER };
    let f2 = -(atan2f(pos.x - l.pos.x, pos.z - l.pos.z) * 180.0 / dtor(180.0) + l.theta);
    let degrees = if arg2 >= 0.0 {
        let sp3c = (0.017_453_292 * f2).sin() * arg2;
        let sp38 = (0.017_453_292 * f2).cos();
        let mut f = sp3c.abs().atan2(sp38.abs());
        if sp3c >= 0.0 && sp38 >= 0.0 {
        } else if sp3c >= 0.0 {
            f = dtor(180.0) - f;
        } else if sp38 >= 0.0 {
            f = -f;
        } else {
            f = -(dtor(180.0) - f);
        }
        (f * 180.0 / dtor(180.0)) as i32
    } else {
        f2 as i32
    };
    ps_calculate_pan3(degrees)
}

/// `ps_get_theoretical_vol_pan` (`propsnd.c:1380`): the volume (0..1) and pan
/// (−1 left .. 1 right) of `sound` at `pos` for these listeners.
pub fn vol_pan(configs: &AudioConfigs, sound: u16, pos: Vec3, dists: [f32; 3], listeners: &[Listener]) -> (f32, f32) {
    let (dist, specialpan) = match configs.get(sound) {
        Some(c) => (c.dist, c.specialpan),
        None => (dists, false),
    };
    // ps_calculate_vol: the nearest player's distance.
    let mut playerdist = dist[2] + 10.0;
    if configs.get(sound).is_some() {
        playerdist = playerdist.min(dist[2]);
    }
    for l in listeners {
        playerdist = playerdist.min((pos - l.pos).length());
    }
    let vol = ps_calculate_volume_from_distance(playerdist, dist);
    // ps_calculate_pan.
    let mut pan = AL_PAN_CENTER;
    if playerdist > 0.0 && playerdist < dist[2] {
        let (d1, d2) = (dist[0].min(5501.0), dist[1].min(5801.0));
        pan = if specialpan {
            if playerdist < d1 {
                AL_PAN_CENTER
            } else if playerdist < d2 {
                ps_calculate_pan2(pos, ((playerdist - d1) / (d2 - d1)).clamp(0.0, 1.0), listeners)
            } else {
                ps_calculate_pan2(pos, -1.0, listeners)
            }
        } else {
            ps_calculate_pan2(pos, -1.0, listeners)
        };
    }
    (vol, ((pan - AL_PAN_CENTER) as f32 / 64.0).clamp(-1.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full to dist1, a square-root fade to 1000/32767 at dist2, then linear.
    #[test]
    fn the_falloff_is_pds() {
        let d = [2500.0, 4900.0, 5000.0];
        assert_eq!(ps_calculate_volume_from_distance(2000.0, d), 1.0);
        assert!((ps_calculate_volume_from_distance(4899.0, d) - 1000.0 / 32767.0).abs() < 0.01);
        assert_eq!(ps_calculate_volume_from_distance(5000.0, d), 0.0);
    }

    /// One player: a sound to the right pans right; two players: centre.
    #[test]
    fn one_player_hears_the_side_two_hear_the_middle() {
        let c = AudioConfigs::default();
        // Facing +z (theta 0); +x is to the player's left in PD (forward is (-sin, 0, cos)).
        let me = Listener { pos: Vec3::ZERO, theta: 0.0 };
        let (_, left) = vol_pan(&c, 1, Vec3::new(300.0, 0.0, 0.0), DEFAULT_DISTS, &[me]);
        let (_, right) = vol_pan(&c, 1, Vec3::new(-300.0, 0.0, 0.0), DEFAULT_DISTS, &[me]);
        assert!(left < -0.5 && right > 0.5, "{left} {right}");
        let (_, mid) = vol_pan(&c, 1, Vec3::new(300.0, 0.0, 0.0), DEFAULT_DISTS, &[me, me]);
        assert_eq!(mid, 0.0);
    }
}
