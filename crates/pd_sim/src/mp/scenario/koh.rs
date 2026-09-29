//! King of the Hill (`scenarios/kingofthehill.inc`): one room is the hill,
//! lit green. A team alone in it for the hill time (20 s by default) scores
//! a point for each of its chrs there; while another team shares it the clock
//! stops. With Mobile Hill the hill then moves: the old room fades back to its
//! own light and another of the setup's hills takes over.
//!
//! The simulants find a spot in the hill with [`World::botroom_find_pos`]
//! (`botroom.c`), over the room's cover and waypoints.

use glam::Vec3;
use pd_core::ids::*;
use pd_core::math::atan2f;
use pd_core::mp::MAX_TEAMS;

use super::{l, ScenarioHud};
use crate::lights::{LIGHTOP_HIGHLIGHT, LIGHTOP_NONE};
use crate::mp::{radar_get_team_index, G_TEAM_COLOURS};
use crate::world::World;

/// `SFXNUM_05B9_MP_HILLENTERED`.
pub const SFXNUM_05B9_MP_HILLENTERED: u16 = 0x05b9;

/// `scenariodata_koh` (`types.h:4167`).
#[derive(Clone, Debug)]
pub struct Koh {
    /// A team index (`radar_get_team_index`), -1 none.
    pub occupiedteam: i16,
    pub elapsed240: i16,
    pub movehill: bool,
    pub hillindex: i16,
    pub hillcount: i16,
    /// `hillrooms[0]`, the hill's room (`hillrooms[1]` is always -1).
    pub hillroom: Option<u16>,
    pub hillpads: [i16; 9],
    pub hillpos: Vec3,
    pub colourfrac: [f32; 3],
    /// `botroom_find_pos`'s run-time pad and cover flags
    /// (`PADFLAG_AIBOTINUSE`, `g_CoverFlags`), and each cover's room
    /// (`g_CoverRooms`, `setup_prepare_cover`).
    pub pad_aibotinuse: Vec<bool>,
    pub coverflags: Vec<u16>,
    pub coverrooms: Vec<Option<u16>>,
    /// `g_Rooms[room].firstwaypoint` / `numwaypoints` over
    /// `g_Vars.waypointnums` (`setup_prepare_waypoints`).
    pub waypointnums: Vec<usize>,
    pub roomwaypoints: Vec<(usize, usize)>,
}

impl Default for Koh {
    /// `koh_init` (`kingofthehill.inc:160`).
    fn default() -> Self {
        Koh {
            occupiedteam: -1,
            elapsed240: 0,
            movehill: false,
            hillindex: -1,
            hillcount: 0,
            hillroom: None,
            hillpads: [-1; 9],
            hillpos: Vec3::ZERO,
            colourfrac: [0.25, 1.0, 0.25],
            pad_aibotinuse: Vec::new(),
            coverflags: Vec::new(),
            coverrooms: Vec::new(),
            waypointnums: Vec::new(),
            roomwaypoints: Vec::new(),
        }
    }
}

impl Koh {
    /// `koh_add_hill` (`kingofthehill.inc:653`).
    pub fn koh_add_hill(&mut self, pad: i16) {
        if (self.hillcount as usize) < self.hillpads.len() {
            self.hillpads[self.hillcount as usize] = pad;
            self.hillcount += 1;
        }
    }
}

impl World {
    /// The hill on pad `padnum`: its room, and the floor under the pad
    /// (`pad_unpack` + `cd_find_ground_at_pos_ct`).
    fn koh_place_hill(&mut self, padnum: i16) {
        let (pos, room) = self.stage.pads.get(padnum.max(0) as usize).map_or((Vec3::ZERO, None), |p| (p.pos, p.room));
        let rooms: Vec<u16> = room.into_iter().collect();
        let y = self.level.cd_find_ground_at_pos_ct(pos, &rooms);
        let d = &mut self.mp.scenariodata.koh;
        d.hillroom = room;
        d.hillpos = Vec3::new(pos.x, y, pos.z);
    }

    fn room_set_light_op(&mut self, room: Option<u16>, op: u8) {
        if let Some(r) = room.and_then(|r| self.lights.rooms.get_mut(r as usize)) {
            r.lightop = op;
        }
    }

    /// `koh_init_props` (`kingofthehill.inc:184`): a random hill of the
    /// setup's, lit. PD's bug is kept: with a single hill the hill is pad 0's
    /// room.
    pub(crate) fn koh_init_props(&mut self) {
        self.koh_prepare_rooms();
        let d = &mut self.mp.scenariodata.koh;
        let mut pad_id = 0;
        if d.hillcount > 1 {
            let n = d.hillcount as u32;
            let k = (self.rng.random() % n) as i16;
            let d = &mut self.mp.scenariodata.koh;
            d.hillindex = k;
            pad_id = d.hillpads[k as usize];
        } else {
            d.hillindex = 0;
        }
        self.koh_place_hill(pad_id);
        self.mp.scenariodata.koh.movehill = false;
        let room = self.mp.scenariodata.koh.hillroom;
        self.room_set_light_op(room, LIGHTOP_HIGHLIGHT);
    }

    /// Chr `i`'s team index (`radar_get_team_index(chr->team)`).
    pub(crate) fn chr_team_index(&self, i: usize) -> usize {
        radar_get_team_index(self.chrs[i].team)
    }

    /// `prop->rooms[0]` of chr `i`.
    pub(crate) fn chr_room0(&self, i: usize) -> Option<u16> {
        self.chrs[i].rooms.first().copied()
    }

    /// `koh_tick` (`kingofthehill.inc:218`).
    pub(crate) fn koh_tick(&mut self) {
        if self.mp.scenariodata.koh.hillindex == -1 {
            return;
        }
        let mut dualoccupancy = false;
        let target: [f32; 3];
        if self.mp.scenariodata.koh.movehill {
            // Back to the room's own light first, over several frames.
            let d = &mut self.mp.scenariodata.koh;
            d.occupiedteam = -1;
            d.elapsed240 = 0;
            target = [1.0; 3];
            if d.colourfrac.iter().all(|&f| f >= 0.95) {
                let room = d.hillroom;
                self.room_set_light_op(room, LIGHTOP_NONE);
                let mut padnum = 0;
                let d = &self.mp.scenariodata.koh;
                if d.hillcount >= 2 {
                    let previndex = d.hillindex;
                    let n = d.hillcount as u32;
                    loop {
                        let k = (self.rng.random() % n) as i16;
                        self.mp.scenariodata.koh.hillindex = k;
                        if k != previndex {
                            break;
                        }
                    }
                    let d = &self.mp.scenariodata.koh;
                    padnum = d.hillpads[d.hillindex as usize];
                } else {
                    self.mp.scenariodata.koh.hillindex = 0;
                }
                self.koh_place_hill(padnum);
                let room = self.mp.scenariodata.koh.hillroom;
                self.room_set_light_op(room, LIGHTOP_HIGHLIGHT);
                let d = &mut self.mp.scenariodata.koh;
                d.occupiedteam = -1;
                d.elapsed240 = 0;
                d.movehill = false;
            }
        } else {
            let hillroom = self.mp.scenariodata.koh.hillroom;
            // The chrs in the hill, alive.
            let inhill: Vec<usize> = (0..self.chrs.len()).rev().filter(|&i| hillroom.is_some() && self.chr_room0(i) == hillroom && !self.chr_is_dead(i)).collect();
            let mut teamsinhill = [0i32; MAX_TEAMS];
            let mut numteamsinhill = 0;
            for &c in &inhill {
                let t = self.chr_team_index(c);
                if teamsinhill[t] == 0 {
                    numteamsinhill += 1;
                    teamsinhill[t] = 1;
                }
            }
            if numteamsinhill == 0 {
                let d = &mut self.mp.scenariodata.koh;
                d.occupiedteam = -1;
                d.elapsed240 = 0;
            } else {
                let mut hillteam: i32;
                if numteamsinhill == 1 {
                    hillteam = teamsinhill.iter().position(|&t| t != 0).unwrap_or(MAX_TEAMS) as i32;
                } else {
                    // PD's "most chrs" filter over flags that are all 1: the
                    // teams in the hill stay, all counted again.
                    let mostchrs = teamsinhill.iter().copied().max().unwrap_or(0);
                    for t in teamsinhill.iter_mut() {
                        if *t != mostchrs {
                            *t = 0;
                        }
                    }
                    dualoccupancy = teamsinhill.iter().filter(|&&t| t != 0).count() >= 2;
                    let occupied = self.mp.scenariodata.koh.occupiedteam as i32;
                    hillteam = (0..MAX_TEAMS as i32).find(|&t| teamsinhill[t as usize] != 0 && t == occupied).unwrap_or(MAX_TEAMS as i32);
                    if hillteam == MAX_TEAMS as i32 {
                        // The holders left; the hill is green until one team
                        // has it alone.
                        self.mp.scenariodata.koh.occupiedteam = -1;
                        hillteam = -1;
                    }
                }
                if hillteam != self.mp.scenariodata.koh.occupiedteam as i32 {
                    self.sound(SFXNUM_05B9_MP_HILLENTERED, 1.0);
                    let d = &mut self.mp.scenariodata.koh;
                    d.occupiedteam = hillteam as i16;
                    d.elapsed240 = 0;
                    // "%shas captured\nthe Hill!\n"
                    let teamname = format!("{}\n", self.setup.teamnames.get(hillteam.max(0) as usize).map_or("", |t| t.as_str()));
                    let text = self.res.lang.get(l(22)).replacen("%s", &teamname, 1);
                    for p in 0..self.players.len() {
                        if self.chr_team_index(p) as i32 == hillteam {
                            // "We have\nthe Hill!\n"
                            let ours = self.res.lang.get(l(21)).to_string();
                            self.hudmsg_create_with_flags(p, &ours, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                        } else {
                            self.hudmsg_create_with_flags(p, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                        }
                    }
                } else if !dualoccupancy {
                    let lv240 = self.lv.lvupdate240;
                    let limit = self.setup.mphilltime as i32 * 240 + 2400;
                    let d = &mut self.mp.scenariodata.koh;
                    d.elapsed240 = d.elapsed240.wrapping_add(lv240 as i16);
                    if d.elapsed240 as i32 >= limit {
                        self.sound(crate::mp::scenario::htb::SFXNUM_05B8_MP_SCOREPOINT, 1.0);
                        let occupied = self.mp.scenariodata.koh.occupiedteam as i32;
                        // PD's bug, kept: the dead of the team in the hill score too.
                        for c in 0..self.chrs.len() {
                            if self.chr_team_index(c) as i32 == occupied && self.chr_room0(c) == hillroom {
                                let slot = self.chrs[c].mpslot;
                                self.mp.chrs[slot].numpoints += 1;
                            }
                        }
                        for c in 0..self.chrs.len() {
                            if self.chrs[c].aibot.is_none() && self.chr_team_index(c) as i32 == occupied {
                                if let Some(p) = self.chrs[c].player {
                                    // "King of\nthe Hill!\n"
                                    let text = self.res.lang.get(l(20)).to_string();
                                    self.hudmsg_create_with_flags(p, &text, HUDMSGTYPE_MPSCENARIO, HUDMSGFLAG_ONLYIFALIVE);
                                }
                            }
                        }
                        let d = &mut self.mp.scenariodata.koh;
                        d.occupiedteam = -1;
                        d.elapsed240 = 0;
                        if self.setup.options & MPOPTION_KOH_MOBILEHILL != 0 {
                            d.movehill = true;
                        }
                    }
                }
            }
            let occupied = self.mp.scenariodata.koh.occupiedteam;
            target = if occupied == -1 {
                [0.25, 1.0, 0.25]
            } else {
                let c = G_TEAM_COLOURS[occupied as usize];
                [((c >> 24 & 0xff) as i32 + 0xff) as f32 * (1.0 / 512.0), ((c >> 16 & 0xff) as i32 + 0xff) as f32 * (1.0 / 512.0), ((c >> 8 & 0xff) as i32 + 0xff) as f32 * (1.0 / 512.0)]
            };
        }
        // The tween runs on diffframe60, which counts while paused (PD's bug:
        // a hill scored just before a pause fades and moves in the pause).
        let frames = self.lv.diffframe60;
        let d = &mut self.mp.scenariodata.koh;
        for k in 0..3 {
            if d.colourfrac[k] != target[k] {
                for _ in 0..frames {
                    d.colourfrac[k] = 0.05 * target[k] + 0.95 * d.colourfrac[k];
                }
            }
        }
    }

    /// `koh_render_hud` (`kingofthehill.inc:552`): the holding team's time to
    /// the point, "%02d" (or "%d:%02d" for a hill time of a minute or more).
    pub(crate) fn koh_render_hud(&self, pi: usize) -> ScenarioHud {
        let d = &self.mp.scenariodata.koh;
        if self.chr_team_index(pi) as i32 != d.occupiedteam as i32 || d.movehill {
            return ScenarioHud::None;
        }
        let mphilltime = self.setup.mphilltime as i32;
        let mut time240 = mphilltime * 240 - d.elapsed240 as i32;
        time240 += 2400;
        let mins = time240 / (60 * 240);
        time240 -= 60 * 240 * mins;
        let secs = (time240 + (240 - 1)) / 240;
        let text = if (mphilltime * 60 + 600) / 3600 != 0 { format!("{mins}:{secs:02}") } else { format!("{secs:02}") };
        ScenarioHud::Countdown { text }
    }

    /// `koh_highlight_room` (`kingofthehill.inc:666`): the hill's tint.
    pub(crate) fn koh_highlight_room(&self, room: u16) -> Option<[f32; 3]> {
        let d = &self.mp.scenariodata.koh;
        (d.hillroom == Some(room)).then_some(d.colourfrac)
    }

    /// The run-time tables `botroom_find_pos` reads: `setup_prepare_cover`'s
    /// (`setupcover.c:24`) cover rooms and flags, and `setup_prepare_waypoints`'
    /// (`setupwaypoints.c:14`) waypoints by room (room ascending, then pad,
    /// an AI-drop waypoint going after the others of its room).
    fn koh_prepare_rooms(&mut self) {
        let stage = self.stage.clone();
        let covers = &stage.cover;
        let mut coverflags = vec![0u16; covers.len()];
        let mut coverrooms = vec![None; covers.len()];
        for i in 0..covers.len() {
            // cover_unpack's @bug, kept: the definition is covers[(u8)i].
            let def = &covers[(i as u8) as usize % covers.len()];
            coverflags[i] |= def.special as u16;
            if def.look == Vec3::ZERO {
                continue;
            }
            let (inrooms, aboverooms, _) = stage.rooms.bg_find_rooms_by_pos(def.pos, 20);
            let roomsptr = if !inrooms.is_empty() { inrooms } else { aboverooms };
            if !roomsptr.is_empty() {
                coverrooms[i] = match self.level.cd_find_room_at_pos(def.pos, &roomsptr) {
                    Some(r) if r > 0 => Some(r),
                    _ => roomsptr.first().copied(),
                };
            }
        }
        let n = stage.waypoints.len();
        let room_of = |w: usize| stage.pads.get(stage.waypoints[w].padnum).and_then(|p| p.room).map_or(-1, |r| r as i32);
        let aidrop = |w: usize| stage.pads.get(stage.waypoints[w].padnum).is_some_and(|p| p.flags & PADFLAG_AIDROP != 0);
        let mut nums: Vec<usize> = Vec::with_capacity(n);
        for i in 0..n {
            let (room, pad) = (room_of(i), stage.waypoints[i].padnum);
            let j = nums.iter().position(|&w| room < room_of(w) || (room == room_of(w) && (aidrop(w) || pad < stage.waypoints[w].padnum))).unwrap_or(nums.len());
            nums.insert(j, i);
        }
        let nrooms = self.lights.rooms.len().max(stage.rooms.roomcount());
        let mut roomwaypoints = vec![(0usize, 0usize); nrooms];
        let mut current = -1;
        for (k, &w) in nums.iter().enumerate() {
            let room = room_of(w);
            if room != current {
                current = room;
                if room >= 0 && (room as usize) < nrooms {
                    roomwaypoints[room as usize].0 = k;
                }
            }
            if !aidrop(w) && current >= 0 && (current as usize) < nrooms {
                roomwaypoints[current as usize].1 += 1;
            }
        }
        let d = &mut self.mp.scenariodata.koh;
        d.pad_aibotinuse = vec![false; stage.pads.len()];
        d.coverflags = coverflags;
        d.coverrooms = coverrooms;
        d.waypointnums = nums;
        d.roomwaypoints = roomwaypoints;
    }

    /// `botroom_find_pos` (`botroom.c:49`): a random spot in `room` among its
    /// ordinary cover and its waypoints' pads not marked in use by a simulant;
    /// if every one is, the marks are cleared and it tries again. The spot,
    /// its facing, and its pad or cover.
    pub(crate) fn botroom_find_pos(&mut self, room: u16) -> Option<(Vec3, f32, i32, i32)> {
        const SPECIAL: u16 = COVERFLAG_SPECIAL1 | COVERFLAG_SPECIAL2 | COVERFLAG_SPECIAL3;
        let stage = self.stage.clone();
        let mut covernums: Vec<usize> = Vec::new();
        let mut padnums: Vec<usize> = Vec::new();
        let mut totalcount = 0;
        let mut clearinuse = false;
        loop {
            let mut anyinuse = false;
            let d = &mut self.mp.scenariodata.koh;
            for i in 0..d.coverflags.len() {
                if d.coverflags[i] & SPECIAL == 0 && d.coverrooms[i] == Some(room) {
                    if clearinuse && d.coverflags[i] & COVERFLAG_AIBOTINUSE != 0 {
                        d.coverflags[i] &= !COVERFLAG_AIBOTINUSE;
                        covernums.push(i);
                        totalcount += 1;
                    } else if d.coverflags[i] & COVERFLAG_AIBOTINUSE == 0 {
                        covernums.push(i);
                        totalcount += 1;
                    } else {
                        anyinuse = true;
                    }
                    if covernums.len() >= 40 {
                        break;
                    }
                }
            }
            if let Some(&(first, count)) = d.roomwaypoints.get(room as usize) {
                for k in first..first + count {
                    let Some(&w) = d.waypointnums.get(k) else { break };
                    let pad = stage.waypoints[w].padnum;
                    let inuse = d.pad_aibotinuse.get(pad).copied().unwrap_or(false);
                    if clearinuse && inuse {
                        d.pad_aibotinuse[pad] = false;
                        padnums.push(pad);
                        totalcount += 1;
                    } else if !inuse {
                        padnums.push(pad);
                        totalcount += 1;
                    } else {
                        anyinuse = true;
                    }
                    if padnums.len() >= 40 {
                        break;
                    }
                }
            }
            clearinuse = anyinuse;
            if !(anyinuse && totalcount == 0) {
                break;
            }
        }
        if totalcount == 0 {
            return None;
        }
        let i = (self.rng.random() % totalcount as u32) as usize;
        if i < covernums.len() {
            let c = covernums[i];
            let def = &stage.cover[(c as u8) as usize % stage.cover.len()];
            Some((def.pos, atan2f(def.look.z, def.look.x), -1, c as i32))
        } else {
            let p = padnums[i - covernums.len()];
            let pad = &stage.pads[p];
            Some((pad.pos, atan2f(pad.look.z, pad.look.x), p as i32, -1))
        }
    }
}
