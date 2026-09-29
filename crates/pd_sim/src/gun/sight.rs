//! What the sight tracks: `sight.c`'s `sight_tick` and `lv.c`'s tracked props
//! (`lv_update_tracked_prop`, `lv_find_threats`), the player's
//! `trackedprops[4]` with their screen boxes and `targetset` timers (the boxes
//! close in over 80 ticks). By the weapon's `tracktype`: the default sight
//! keeps what the crosshair is on in slot 0; the rocket launcher locks the
//! first thing aimed at into slot 0 and keeps it (its homing rocket steers at
//! it, `bondgun.c:4752`); the CMP150's follow lock-on fills all four; the
//! Threat Detector (the K7 Avenger's and the mines' secondary) boxes the
//! explosives and sentries on screen. The boxes are drawn by `pd_render::hud`
//! (`sight_draw_target_box`).
//!
//! `// SUBST:` the CMP150's lock-on also turns the aim onto its targets
//! (`autoaim_tick`, `prop.c:2590`) / auto-aim isn't ported, so its boxes only
//! show what it tracks.

use glam::{Mat4, Vec3};
use pd_core::ids::*;
use pd_core::model::NodeKind;

use super::shot::AimedAt;
use crate::player::camera::Camera;
use crate::world::World;

/// `SFXNUM_0007`: the lock's beep (`snd_start_extra`, full volume, centred).
const SFXNUM_0007: u16 = 0x0007;

/// `struct trackedprop`: a prop and its box on screen (`x1..x2`, `y1..y2`,
/// PD pixels; `x1 = -1, x2 = -2` when it isn't on screen).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackedProp {
    pub prop: Option<AimedAt>,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Default for TrackedProp {
    fn default() -> Self {
        TrackedProp { prop: None, x1: -1.0, y1: -1.0, x2: -2.0, y2: -2.0 }
    }
}

/// The sight fields of `struct player`.
#[derive(Clone, Debug, Default)]
pub struct Sight {
    pub trackedprops: [TrackedProp; 4],
    /// `lookingatprop`'s box (the prop is `World::lookingatprop`).
    pub lookingat: TrackedProp,
    /// `targetset[4]`: ticks since each slot took its prop (to 512, and from
    /// 512 on to 1020 once its chr has died).
    pub targetset: [i32; 4],
    /// `sighttracktype`: `SIGHTTRACKTYPE_*`.
    pub sighttracktype: u8,
    pub lastsighton: bool,
    pub sighttimer240: i32,
}

/// `cam0f0b4eb8` (`camera.c:169`): an eye-space point to screen pixels through
/// the view's field of view and aspect.
fn cam0f0b4eb8(cam: &Camera, p: Vec3, fovy: f32, aspect: f32) -> [f32; 2] {
    let f12 = (fovy * 0.008_726_646).cos() * cam.c_halfheight / ((fovy * 0.008_726_646).sin() * p.z);
    let f14 = f12 * cam.c_halfwidth / (aspect * cam.c_halfheight);
    [cam.c_screenleft + cam.c_halfwidth - f14 * p.x, f12 * p.y + (cam.c_screentop + cam.c_halfheight)]
}

/// `model_get_screen_coords2` (`propobj.c:869`) over a model's part boxes
/// (`bboxes`: each box and its node's world matrix), seen by `cam`: the box
/// on screen of every part whose node is in front of the eye, as
/// (x1, y1, x2, y2); none if no part is.
pub fn model_get_screen_coords(cam: &Camera, bboxes: impl IntoIterator<Item = ([f32; 6], Mat4)>) -> Option<[f32; 4]> {
    let mut out: Option<[f32; 4]> = None;
    for (bbox, m) in bboxes {
        let m = cam.world_to_screen * m;
        let origin = m.w_axis.truncate();
        if origin.z >= 0.0 {
            continue;
        }
        // obj_get_rotated_local_[xy]_{min,max}_by_mtx4 + the translation.
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for k in 0..8 {
            let c = Vec3::new(bbox[k & 1], bbox[2 + ((k >> 1) & 1)], bbox[4 + ((k >> 2) & 1)]);
            let p = m.transform_point3(c);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        // obj_get_screeninfo (`propobj.c:949`).
        let (fovy, aspect) = (cam.c_perspfovy, cam.c_perspaspect);
        let left = cam0f0b4eb8(cam, Vec3::new(lo.x, origin.y, origin.z), fovy, aspect)[0];
        let right = cam0f0b4eb8(cam, Vec3::new(hi.x, origin.y, origin.z), fovy, aspect)[0];
        let top = cam0f0b4eb8(cam, Vec3::new(origin.x, hi.y, origin.z), fovy, aspect)[1];
        let bottom = cam0f0b4eb8(cam, Vec3::new(origin.x, lo.y, origin.z), fovy, aspect)[1];
        out = Some(match out {
            None => [left, top, right, bottom],
            Some([x1, y1, x2, y2]) => [x1.min(left), y1.min(top), x2.max(right), y2.max(bottom)],
        });
    }
    out
}

/// A model's part boxes with their nodes' matrices (a chr's head's too).
fn part_boxes(model: &pd_core::model::Model) -> Vec<([f32; 6], Mat4)> {
    let mut out = Vec::new();
    for (n, node) in model.def.nodes.iter().enumerate() {
        if let NodeKind::BBox { bbox, .. } = &node.kind {
            if let Some(m) = model.def.find_node_mtx_index(n, 0).and_then(|i| model.matrices.get(i).copied()) {
                out.push((*bbox, m));
            }
        }
    }
    if let (Some(head), Some(hm)) = (model.head.as_deref(), model.head_matrix()) {
        for (n, node) in head.nodes.iter().enumerate() {
            if let NodeKind::BBox { bbox, .. } = &node.kind {
                let m = head.find_node_mtx_index(n, 0).and_then(|i| model.matrices.get(i).copied()).unwrap_or(hm);
                out.push((*bbox, m));
            }
        }
    }
    out
}

impl World {
    /// Whether prop `p` is on player `pi`'s screen (`PROPFLAG_ONTHISSCREENTHISTICK`).
    fn sight_prop_onscreen(&self, pi: usize, p: AimedAt) -> bool {
        let cam = &self.players[pi].cam;
        match p {
            AimedAt::Chr(j) => crate::chr::body::pos_is_onscreen(cam, self.chrs[j].pos, self.chrs[j].effective_scale()),
            AimedAt::Board(b) => self.boards.get(b).is_some_and(|b| crate::chr::body::pos_is_onscreen(cam, (b.min + b.max) * 0.5, 1.0)),
            AimedAt::Obj(id) => self.props.get(id).is_some_and(|o| !o.is_gone() && crate::chr::body::pos_is_onscreen(cam, o.pos, o.def.scale * o.scale)),
        }
    }

    /// `model_get_screen_coords` for a tracked prop as player `pi` sees it.
    fn sight_prop_screen_coords(&self, pi: usize, p: AimedAt) -> Option<[f32; 4]> {
        let cam = &self.players[pi].cam;
        match p {
            AimedAt::Chr(j) => model_get_screen_coords(cam, part_boxes(&self.chrs[j].model)),
            AimedAt::Board(b) => {
                let b = self.boards.get(b)?;
                model_get_screen_coords(cam, [([b.min.x, b.max.x, b.min.y, b.max.y, b.min.z, b.max.z], Mat4::IDENTITY)])
            }
            AimedAt::Obj(id) => {
                let o = self.props.get(id)?;
                let mats = o.init_matrices();
                let boxes = o.def.nodes.iter().enumerate().filter_map(|(n, node)| match &node.kind {
                    NodeKind::BBox { bbox, .. } => o.def.find_node_mtx_index(n, 0).and_then(|i| mats.get(i).copied()).map(|m| (*bbox, m)),
                    _ => None,
                });
                model_get_screen_coords(cam, boxes.collect::<Vec<_>>())
            }
        }
    }

    /// `lv_update_tracked_prop` (`lv.c:580`): the tracked prop's box this
    /// frame (`index` its slot, -1 for `lookingatprop`). False if it's off
    /// screen, the player itself, or dead: `lookingatprop` lets a dead chr go
    /// at once; a slot keeps it until its `targetset` passes 175.
    fn lv_update_tracked_prop(&mut self, pi: usize, tp: &mut TrackedProp, index: i32) -> bool {
        let Some(p) = tp.prop else { return true };
        if let AimedAt::Chr(j) = p {
            if self.chrs[j].player == Some(pi) {
                return false;
            }
            if self.chr_is_dead(j) {
                if index >= 0 {
                    let ts = &mut self.players[pi].sight.targetset[index as usize];
                    if *ts < 129 {
                        *ts = 129;
                    }
                    if *ts >= 175 {
                        tp.prop = None;
                        return false;
                    }
                } else {
                    tp.prop = None;
                    return false;
                }
            }
        }
        if !self.sight_prop_onscreen(pi, p) {
            return false;
        }
        let Some([x1, y1, x2, y2]) = self.sight_prop_screen_coords(pi, p) else { return false };
        tp.x1 = x1 - 2.0;
        tp.x2 = x2 + 2.0;
        tp.y1 = y1 - 2.0;
        tp.y2 = y2 + 2.0;
        true
    }

    /// `lv_render`'s tracking after `lookingatprop` (`lv.c:1238`): the Threat
    /// Detector's search, or for an aim-tracking weapon the boxes of what the
    /// crosshair is on and of the tracked props.
    pub(crate) fn lv_tick_tracked_props(&mut self, pi: usize) {
        let right = &self.players[pi].gun.hands[HAND_RIGHT];
        let funcflags = self.res.gset.func(right.weaponnum, right.weaponfunc).map_or(0, |f| f.flags);
        let weaponnum = self.players[pi].gun.bgun_get_weapon_num(HAND_RIGHT);
        if funcflags & FUNCFLAG_THREATDETECTOR != 0 {
            self.lv_find_threats(pi);
        } else if self.res.gset.has_flag(weaponnum, WEAPONFLAG_AIMTRACK) {
            let mut look = TrackedProp { prop: self.lookingatprop[pi], ..self.players[pi].sight.lookingat };
            if !self.lv_update_tracked_prop(pi, &mut look, -1) {
                look.prop = None;
            }
            self.lookingatprop[pi] = look.prop;
            self.players[pi].sight.lookingat = look;
            for j in 0..4 {
                let mut tp = self.players[pi].sight.trackedprops[j];
                if !self.lv_update_tracked_prop(pi, &mut tp, j as i32) {
                    tp.x1 = -1.0;
                    tp.x2 = -2.0;
                }
                self.players[pi].sight.trackedprops[j] = tp;
            }
        }
    }

    /// `lv_find_threats` (`lv.c:914`): the slots keep their props still on
    /// screen (their boxes updated); the rest empty; then the explosives and
    /// sentries on screen, nearest first, take the free slots, or the
    /// farthest slot's place if nearer.
    fn lv_find_threats(&mut self, pi: usize) {
        let campos = self.players[pi].cam.pos();
        let w2s = self.players[pi].cam.world_to_screen;
        let mut distances = [0.0f32; 4];
        let mut activeslots = [false; 4];
        // The objects on screen, nearest first (`onscreenprops` walked back).
        let mut onscreen: Vec<(f32, u32)> = self
            .props
            .objs
            .iter()
            .filter(|o| !o.is_deleting() && !o.is_gone() && crate::chr::body::pos_is_onscreen(&self.players[pi].cam, o.pos, o.def.scale * o.scale))
            .map(|o| (-w2s.transform_point3(o.pos).z, o.id))
            .collect();
        onscreen.sort_by(|a, b| a.0.total_cmp(&b.0));
        // func0f168f24: the slots' props still on screen.
        for i in 0..4 {
            let Some(AimedAt::Obj(id)) = self.players[pi].sight.trackedprops[i].prop else { continue };
            if !onscreen.iter().any(|&(_, o)| o == id) {
                continue;
            }
            if let Some([x1, y1, x2, y2]) = self.sight_prop_screen_coords(pi, AimedAt::Obj(id)) {
                activeslots[i] = true;
                let tp = &mut self.players[pi].sight.trackedprops[i];
                (tp.x1, tp.x2, tp.y1, tp.y2) = (x1 - 2.0, x2 + 2.0, y1 - 2.0, y2 + 2.0);
                distances[i] = self.props.get(id).map_or(0.0, |o| o.pos.distance_squared(campos));
            }
        }
        for i in 0..4 {
            if !activeslots[i] {
                self.players[pi].sight.trackedprops[i] = TrackedProp::default();
            }
        }
        // lv_find_threats_for_prop.
        for &(z, id) in &onscreen {
            if z < 0.0 {
                continue;
            }
            let Some(o) = self.props.get(id) else { continue };
            let mut pass = o.ty == OBJTYPE_AUTOGUN && o.flags2 & (OBJFLAG2_AUTOGUN_MALFUNCTIONING | OBJFLAG2_AUTOGUN_WINDMILL) == 0;
            if o.ty == OBJTYPE_WEAPON {
                match o.weaponnum {
                    WEAPON_GRENADE | WEAPON_NBOMB | WEAPON_TIMEDMINE | WEAPON_PROXIMITYMINE | WEAPON_REMOTEMINE => pass = true,
                    WEAPON_DRAGON if o.gunfunc == FUNC_SECONDARY => pass = true,
                    _ => {}
                }
            }
            if !pass || self.players[pi].sight.trackedprops.iter().any(|t| t.prop == Some(AimedAt::Obj(id))) {
                continue;
            }
            let Some([x1, y1, x2, y2]) = self.sight_prop_screen_coords(pi, AimedAt::Obj(id)) else { continue };
            let sqdist = o.pos.distance_squared(campos);
            let mut index = (0..4).rev().find(|&i| !activeslots[i]).map_or(-1, |i| i as i32);
            if index == -1 {
                let mut furthest = 0.0;
                for (i, &d) in distances.iter().enumerate() {
                    if d > furthest {
                        furthest = d;
                        index = i as i32;
                    }
                }
                if sqdist >= furthest {
                    index = -1;
                }
            }
            if index >= 0 {
                let i = index as usize;
                self.players[pi].sight.trackedprops[i] = TrackedProp { prop: Some(AimedAt::Obj(id)), x1: x1 - 2.0, x2: x2 + 2.0, y1: y1 - 2.0, y2: y2 + 2.0 };
                self.players[pi].sight.targetset[i] = 0;
                activeslots[i] = true;
                distances[i] = sqdist;
            }
        }
    }

    /// `sight_can_target_prop` (`sight.c:70`): not already in the first `max`
    /// slots, and a chr (a player's too), or anything for the rocket launcher.
    fn sight_can_target_prop(&self, pi: usize, p: AimedAt, max: usize) -> bool {
        let s = &self.players[pi].sight;
        if s.trackedprops[..max].iter().any(|t| t.prop == Some(p)) {
            return false;
        }
        matches!(p, AimedAt::Chr(_)) || self.players[pi].gun.bgun_get_weapon_num(HAND_RIGHT) == WEAPON_ROCKETLAUNCHER
    }

    /// `sight_is_prop_friendly` (`sight.c:26`): the chr is on the player's
    /// team (blue boxes and aimer). `None`: what the crosshair is on.
    pub fn sight_is_prop_friendly(&self, pi: usize, p: Option<AimedAt>) -> bool {
        match p.or(self.lookingatprop.get(pi).copied().flatten()) {
            Some(AimedAt::Chr(j)) => self.chr_compare_teams(pi, j, crate::mp::Compare::Friends),
            _ => false,
        }
    }

    /// `sight_tick` (`sight.c:163`), from `sight_draw` in player `pi`'s
    /// `player_render_hud` (not while the active menu is open). `sighton`:
    /// aiming, the sight not hidden, the player's menu shut.
    pub(crate) fn sight_tick(&mut self, pi: usize, sighton: bool) {
        let lv240 = self.lv.lvupdate240;
        let right = &self.players[pi].gun.hands[HAND_RIGHT];
        let (weaponnum, weaponfunc) = (right.weaponnum, right.weaponfunc);
        let func = self.res.gset.func(weaponnum, weaponfunc);
        let mut newtracktype = self.res.gset.weapon(weaponnum).map_or(SIGHTTRACKTYPE_DEFAULT, |w| w.aim.tracktype);
        if func.is_some_and(|f| f.flags & FUNCFLAG_THREATDETECTOR != 0) {
            newtracktype = SIGHTTRACKTYPE_THREATDETECTOR;
        }
        if func.is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_MELEE) {
            newtracktype = SIGHTTRACKTYPE_NONE;
        }
        let lookingat = self.lookingatprop[pi];
        let s = &mut self.players[pi].sight;
        s.sighttimer240 += lv240;
        // The boxes' timers: to 512 (516 on NTSC), and past 512 to 1024.
        for ts in s.targetset.iter_mut() {
            if *ts > 512 {
                *ts = if *ts < 1024 - lv240 { *ts + lv240 } else { 1020 };
            } else {
                *ts = if *ts < 516 - lv240 { *ts + lv240 } else { 512 };
            }
        }
        if newtracktype != s.sighttracktype {
            if newtracktype == SIGHTTRACKTYPE_THREATDETECTOR {
                s.trackedprops.iter_mut().for_each(|t| t.prop = None);
            }
            s.sighttracktype = newtracktype;
        }
        if sighton && !s.lastsighton && newtracktype != SIGHTTRACKTYPE_THREATDETECTOR {
            s.trackedprops.iter_mut().for_each(|t| t.prop = None);
        }
        // sight_is_reactive_to_prop is true for every prop a match tracks.
        let lookbox = s.lookingat;
        let take = |w: &mut World, index: usize| {
            w.sound(SFXNUM_0007, 1.0);
            let s = &mut w.players[pi].sight;
            s.trackedprops[index] = TrackedProp { prop: lookingat, ..lookbox };
            s.targetset[index] = 0;
        };
        match self.players[pi].sight.sighttracktype {
            SIGHTTRACKTYPE_DEFAULT | SIGHTTRACKTYPE_BETASCANNER => {
                if sighton {
                    if lookingat.is_some() {
                        if lookingat != self.players[pi].sight.trackedprops[0].prop {
                            take(self, 0);
                        }
                    } else {
                        self.players[pi].sight.trackedprops[0].prop = None;
                    }
                }
            }
            SIGHTTRACKTYPE_ROCKETLAUNCHER | SIGHTTRACKTYPE_FOLLOWLOCKON => {
                let max = if self.players[pi].sight.sighttracktype == SIGHTTRACKTYPE_ROCKETLAUNCHER { 1 } else { 4 };
                if lookingat.is_some_and(|p| sighton && self.sight_can_target_prop(pi, p, max)) {
                    if let Some(index) = (0..max).find(|&i| self.players[pi].sight.trackedprops[i].prop.is_none()) {
                        take(self, index);
                    }
                }
            }
            _ => {}
        }
        self.players[pi].sight.lastsighton = sighton;
    }
}

/// A tracked prop's box as `sight_draw_default` draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetBox {
    pub tp: TrackedProp,
    /// `textid`: none, a digit (the CMP150's slots, "1" to "4"), or a word
    /// (the Threat Detector's "PROXY", "TIMED", ...).
    pub label: Option<TargetLabel>,
    /// `targetset[slot]`: how far the box has closed in.
    pub time: i32,
    /// `sight_is_prop_friendly`: blue rather than red.
    pub friendly: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TargetLabel {
    Digit(char),
    Text(String),
}

impl World {
    /// The boxes `sight_draw_default` (`sight.c:615`) draws for player `pi`:
    /// the rocket launcher's lock, the CMP150's four locks numbered, the
    /// Threat Detector's threats named.
    pub fn sight_target_boxes(&self, pi: usize) -> Vec<TargetBox> {
        let s = &self.players[pi].sight;
        let n = match s.sighttracktype {
            SIGHTTRACKTYPE_ROCKETLAUNCHER => 1,
            SIGHTTRACKTYPE_FOLLOWLOCKON | SIGHTTRACKTYPE_THREATDETECTOR => 4,
            _ => 0,
        };
        let text = |i: u16| TargetLabel::Text(self.res.lang.get(pd_core::lang::tx(pd_core::lang::LANGBANK_GUN, i)).to_string());
        let mut out = Vec::new();
        for i in 0..n {
            let tp = s.trackedprops[i];
            let Some(p) = tp.prop else { continue };
            let label = match s.sighttracktype {
                SIGHTTRACKTYPE_THREATDETECTOR => match p {
                    AimedAt::Obj(id) => self.props.get(id).and_then(|o| {
                        if o.ty == OBJTYPE_AUTOGUN && o.flags2 & (OBJFLAG2_AUTOGUN_MALFUNCTIONING | OBJFLAG2_AUTOGUN_WINDMILL) == 0 {
                            return Some(text(215));
                        }
                        if o.ty != OBJTYPE_WEAPON {
                            return None;
                        }
                        match o.weaponnum {
                            WEAPON_GRENADE => Some(text(if o.gunfunc == FUNC_SECONDARY { 212 } else { 213 })),
                            WEAPON_NBOMB => Some(text(if o.gunfunc == FUNC_SECONDARY { 212 } else { 216 })),
                            WEAPON_TIMEDMINE => Some(text(213)),
                            WEAPON_PROXIMITYMINE => Some(text(212)),
                            WEAPON_REMOTEMINE => Some(text(214)),
                            WEAPON_DRAGON if o.gunfunc == FUNC_SECONDARY => Some(text(212)),
                            _ => None,
                        }
                    }),
                    _ => None,
                },
                // textid i + 2 draws the digit '0' + i + 1.
                SIGHTTRACKTYPE_FOLLOWLOCKON => Some(TargetLabel::Digit((b'1' + i as u8) as char)),
                _ => None,
            };
            out.push(TargetBox { tp, label, time: s.targetset[i], friendly: self.sight_is_prop_friendly(pi, Some(p)) });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::PlayerInput;
    use crate::testutil::{run, secondary};

    /// Aiming the rocket launcher at a simulant locks it into slot 0 (its box
    /// on screen round the body); the homing rocket fired then carries it as
    /// its target and turns after the simulant when it steps aside.
    #[test]
    fn the_launchers_lock_steers_the_homing_rocket() {
        let (mut w, _, b) = crate::bot::tests::duel(None);
        w.harness_give_loadout(vec![WEAPON_ROCKETLAUNCHER]);
        run(&mut w, &PlayerInput::default(), 120);
        assert!(w.players[0].sight.trackedprops[0].prop.is_none(), "no lock without aiming");
        secondary(&mut w);
        assert_eq!(w.players[0].gun.hands[HAND_RIGHT].weaponfunc, FUNC_SECONDARY, "the homing rocket");
        run(&mut w, &PlayerInput { aim: true, ..Default::default() }, 40);
        let tp = w.players[0].sight.trackedprops[0];
        assert_eq!(tp.prop, Some(AimedAt::Chr(1)), "the simulant is locked");
        let cam = &w.players[0].cam;
        assert!(tp.x1 < tp.x2 && tp.y1 < tp.y2, "a box: {tp:?}");
        assert!(tp.x1 > cam.c_screenleft && tp.x2 < cam.c_screenleft + cam.c_screenwidth, "on screen: {tp:?}");
        // Fire, then the target steps two metres aside.
        run(&mut w, &PlayerInput { aim: true, fire: true, ..Default::default() }, 3);
        let rocket = w.props.objs.iter().find(|o| o.weaponnum == WEAPON_HOMINGROCKET).expect("a homing rocket");
        assert_eq!(rocket.projectile.as_ref().unwrap().targetprop, Some(1));
        let side = (b - w.players[0].pos).cross(Vec3::Y).normalize_or_zero();
        let angle = w.chrs[1].theta();
        crate::harness::place(&mut w, 1, b + side * 200.0, angle);
        let mut hit = false;
        for _ in 0..240 {
            run(&mut w, &PlayerInput::default(), 1);
            if w.chrs[1].damage > 0.0 || w.chr_is_dead(1) {
                hit = true;
                break;
            }
        }
        assert!(hit, "the rocket never reached the simulant");
    }
}
