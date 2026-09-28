//! The dynamic checks D1–D4 (`SPIKE_PD_COMPLEX.md` in the old repo): matches
//! on the same seeds with PD's graph and with ours, measured the same way.
//!
//! Source: the old repo's `pd_spike/abtest.rs`, on a [`World`].

use std::collections::HashSet;

use glam::Vec2;

use crate::chr::Act;
use crate::stage::{FloorKind, TileLevel};
use crate::world::World;

/// Floor bands (cm) the coverage check counts separately: pit, ground, first, top.
pub const BANDS: [(f32, f32, &str); 4] = [(-400.0, -100.0, "pit"), (-100.0, 140.0, "ground"), (140.0, 400.0, "first"), (400.0, 800.0, "top")];

fn band(y: f32) -> Option<usize> {
    BANDS.iter().position(|&(lo, hi, _)| y >= lo && y < hi)
}

#[derive(Clone, Debug, Default)]
pub struct MatchMetrics {
    /// Seconds from each life's start to the first target in sight.
    pub first_contact: Vec<f32>,
    pub kills: u32,
    pub minutes: f32,
    pub bot_minutes: f32,
    pub alive_frames: u32,
    pub insight_frames: u32,
    pub gopos_frames: u32,
    pub repaths: u32,
    /// Go-to calls / failures: no start waypoint, no end waypoint, no route.
    pub gotos: [u32; 4],
    /// Frames in each `botcmd` distance mode (none, backup, ok, advance, goto).
    pub modes: [u32; 5],
    /// Sum of target distance over frames with the target in sight (cm).
    pub insight_dist: f64,
    /// Go-tos standing still for 3 s.
    pub stalls: u32,
    /// Landings more than 100 cm below where the fall began.
    pub ledge_falls: u32,
    /// Visited 1 m floor cells, per band.
    pub visited: [HashSet<(i32, i32)>; 4],
    /// Alive frames spent in each band.
    pub band_frames: [u32; 4],
    /// Rounds fired, and rounds that hit a chr.
    pub shots: u32,
    pub hits: u32,
}

/// Run `seconds` of the (headless) match `w` and measure its simulants.
pub fn run_match(mut w: World, seconds: u32) -> MatchMetrics {
    let first = w.players.len();
    let n = w.chrs.len();
    let mut m = MatchMetrics::default();
    let mut life_start: Vec<Option<i32>> = vec![Some(145); n];
    let mut was_dead = vec![false; n];
    let mut still_since = vec![0i32; n];
    let mut fall_top: Vec<Option<f32>> = vec![None; n];
    for _ in 0..(60 * seconds) {
        super::step_idle(&mut w);
        let t = w.lv.lvframe60;
        for k in first..n {
            let dead = w.chr_is_dead(k);
            let c = &w.chrs[k];
            if was_dead[k] && !dead {
                life_start[k] = Some(t);
            }
            was_dead[k] = dead;
            if dead {
                still_since[k] = t;
                fall_top[k] = None;
                continue;
            }
            let a = c.aibot.as_ref().unwrap();
            m.alive_frames += 1;
            m.modes[a.distmode.map_or(0, |d| d as usize)] += 1;
            if a.targetinsight {
                m.insight_frames += 1;
                m.insight_dist += a.last_dist as f64;
                if let Some(st) = life_start[k].take() {
                    if t >= st {
                        m.first_contact.push((t - st) as f32 / 60.0);
                    } else {
                        life_start[k] = Some(st);
                    }
                }
            }
            if c.actiontype == Act::GoPos {
                m.gopos_frames += 1;
            }
            let moved = Vec2::new(c.pos.x - c.prevpos.x, c.pos.z - c.prevpos.z).length() > 0.5;
            if c.actiontype != Act::GoPos || moved {
                still_since[k] = t;
            } else if t - still_since[k] == 180 {
                m.stalls += 1;
            }
            match (fall_top[k], c.fallspeed.y != 0.0) {
                // From the feet: the root (`prop->pos`) rides ~1 m above them.
                (None, true) => fall_top[k] = Some(c.manground),
                (Some(top), false) => {
                    if top - c.manground > 100.0 {
                        m.ledge_falls += 1;
                    }
                    fall_top[k] = None;
                }
                _ => {}
            }
            if let Some(b) = band(c.manground) {
                m.band_frames[b] += 1;
                m.visited[b].insert(((c.pos.x / 100.0).floor() as i32, (c.pos.z / 100.0).floor() as i32));
            }
        }
    }
    m.kills = (first..w.chrs.len()).map(|i| w.mp_chr_kills(i)).sum();
    m.repaths = w.navstats.repaths;
    m.gotos = [w.navstats.gotos, w.navstats.goto_no_start, w.navstats.goto_no_end, w.navstats.goto_no_route];
    m.shots = w.navstats.rounds;
    m.hits = w.navstats.round_hits;
    m.minutes = seconds as f32 / 60.0;
    m.bot_minutes = m.minutes * (n - first) as f32;
    m
}

/// The 1 m floor cells of a level, per band (the coverage denominators).
pub fn floor_cells(level: &TileLevel) -> [HashSet<(i32, i32)>; 4] {
    let mut out: [HashSet<(i32, i32)>; 4] = Default::default();
    for p in &level.geom.polys {
        if !matches!(p.floor_kind(), Some(FloorKind::Flat | FloorKind::Ramp)) {
            continue;
        }
        let lo = p.verts.iter().fold(Vec2::splat(f32::INFINITY), |a, v| a.min(Vec2::new(v.x, v.z)));
        let hi = p.verts.iter().fold(Vec2::splat(f32::NEG_INFINITY), |a, v| a.max(Vec2::new(v.x, v.z)));
        let (x0, x1) = ((lo.x / 100.0).floor() as i32, (hi.x / 100.0).floor() as i32);
        let (z0, z1) = ((lo.y / 100.0).floor() as i32, (hi.y / 100.0).floor() as i32);
        for ix in x0..=x1 {
            for iz in z0..=z1 {
                let (x, z) = ((ix as f32 + 0.5) * 100.0, (iz as f32 + 0.5) * 100.0);
                if p.xz_in_convex(x, z) {
                    if let Some(b) = band(p.find_y(x, z)) {
                        out[b].insert((ix, iz));
                    }
                }
            }
        }
    }
    out
}

/// Several matches pooled.
#[derive(Clone, Debug, Default)]
pub struct Pooled {
    pub first_contact: Vec<f32>,
    pub kills: u32,
    pub minutes: f32,
    pub bot_minutes: f32,
    pub alive_frames: u32,
    pub insight_frames: u32,
    pub gopos_frames: u32,
    pub repaths: u32,
    pub gotos: [u32; 4],
    pub modes: [u32; 5],
    pub insight_dist: f64,
    pub stalls: u32,
    pub ledge_falls: u32,
    pub shots: u32,
    pub hits: u32,
    /// Kills per match, in order.
    pub kills_each: Vec<u32>,
    /// Mean per-match coverage per band (0..1).
    pub coverage: [f32; 4],
    pub matches: u32,
}

impl Pooled {
    pub fn add(&mut self, m: &MatchMetrics, cells: &[HashSet<(i32, i32)>; 4]) {
        self.first_contact.extend(&m.first_contact);
        self.kills += m.kills;
        self.minutes += m.minutes;
        self.bot_minutes += m.bot_minutes;
        self.alive_frames += m.alive_frames;
        self.insight_frames += m.insight_frames;
        self.gopos_frames += m.gopos_frames;
        self.repaths += m.repaths;
        for k in 0..4 {
            self.gotos[k] += m.gotos[k];
        }
        for k in 0..5 {
            self.modes[k] += m.modes[k];
        }
        self.insight_dist += m.insight_dist;
        self.shots += m.shots;
        self.hits += m.hits;
        self.kills_each.push(m.kills);
        self.stalls += m.stalls;
        self.ledge_falls += m.ledge_falls;
        for b in 0..4 {
            let visited = m.visited[b].intersection(&cells[b]).count() as f32;
            self.coverage[b] += visited / cells[b].len().max(1) as f32;
        }
        self.matches += 1;
    }

    pub fn median_first_contact(&self) -> f32 {
        let mut v = self.first_contact.clone();
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
    }
    pub fn kills_per_min(&self) -> f32 {
        self.kills as f32 / self.minutes
    }
    pub fn insight_share(&self) -> f32 {
        self.insight_frames as f32 / self.alive_frames.max(1) as f32
    }
    pub fn gopos_share(&self) -> f32 {
        self.gopos_frames as f32 / self.alive_frames.max(1) as f32
    }
    pub fn per_bot_minute(&self, x: u32) -> f32 {
        x as f32 / self.bot_minutes
    }
    pub fn coverage(&self, b: usize) -> f32 {
        self.coverage[b] / self.matches.max(1) as f32
    }
}
