//! A shield's glow on a chr (`chr.c:5083-6650`): where a round meets the
//! shield (`shieldhit_create`), the part box it hit lights up from the face it
//! entered, fades over 80 ticks and spreads bone by bone to the neighbouring
//! boxes over 30 (`shieldhits_tick`); and every 300 frames a shielded chr's
//! shield crawls over four bones for 10 (`chr_render_shield`'s `cmnum` walk).
//! The drawing is `pd_render::shield`, from this state and the posed body.
//!
//! A "cmnum" is a matrix index of the chr's model.
//!
//! `// SUBST:` PD numbers the matrices of the chr's attached props (held
//! guns, a stuck knife or mine) after its own, so the glow can climb onto them
//! / only the body's bones take part; the head's boxes glow with the bone the
//! head hangs from (`headspot`).

use glam::Vec3;
use pd_core::model::hit::HitNode;

use crate::world::World;

/// `MAX_SHIELDHITS` (`constants.h:30`).
pub const MAX_SHIELDHITS: usize = 20;

/// Where a round met the shield: the part box, the face it entered
/// (`hitthing.unk28 / 2`) and the point, in the box's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShieldHitAt {
    pub node: HitNode,
    pub side: i32,
    pub hitpos: [i16; 3],
}

/// `struct shieldhit`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShieldHit {
    pub chr: usize,
    /// The box hit (`node`, `model`) and its face; none: the whole shield.
    pub node: Option<HitNode>,
    pub side: i32,
    pub lvframe60: i32,
    /// `unk018[cmnum]`: ticks since the glow reached the bone (-1 never, -2
    /// done, -3 reached this tick); `unk038`: how many bones it travelled.
    pub unk018: [i32; 32],
    pub unk038: [i32; 32],
    /// `unk011`: how far it spreads, 2..7 bones.
    pub unk011: i32,
    /// The shield's strength it shows, easing to the chr's.
    pub shield: f32,
    /// `hitposx..z`: the hit in the box's space; none (`0x7fff`): the face's middle.
    pub hitpos: Option<[i16; 3]>,
}

/// `g_ShieldHits`, `g_ShieldHitActive`.
#[derive(Clone, Debug)]
pub struct ShieldHits {
    pub hits: Vec<Option<ShieldHit>>,
    pub active: bool,
}

impl Default for ShieldHits {
    fn default() -> ShieldHits {
        ShieldHits { hits: vec![None; MAX_SHIELDHITS], active: false }
    }
}

/// `shieldhit_health_to_rgb` (`chr.c:5402`): the glow's colour by the shield
/// left (0..8).
pub fn shieldhit_health_to_rgb(health: f32) -> [i32; 3] {
    if health < 1.5 {
        [57 - ((1.5 - health) * 28.0) as i32, 75 - ((1.5 - health) * 20.0) as i32, 0]
    } else if health < 3.0 {
        [102 - ((3.0 - health) * 30.0) as i32, 90 - ((3.0 - health) * 10.0) as i32, 0]
    } else if health < 4.5 {
        [174 - ((4.5 - health) * 48.0) as i32, 129 - ((4.5 - health) * 26.0) as i32, 0]
    } else if health < 6.0 {
        [162 - ((6.0 - health) * -8.0) as i32, 54 - ((6.0 - health) * -50.0) as i32, 0]
    } else {
        [162, 54, 0]
    }
}

impl World {
    /// The cmnum of a box of chr `ci`'s model (`shieldhit_node_to_cmnum`).
    pub fn shieldhit_node_to_cmnum(&self, ci: usize, node: HitNode) -> Option<usize> {
        let model = &self.chrs[ci].model;
        match node {
            HitNode::Body(n) => model.def.find_node_mtx_index(n, 0),
            HitNode::Head(_) => model.headspot().and_then(|h| model.def.find_node_mtx_index(h, 0)),
        }
    }

    /// `shieldhit_find_parentnode_cmnum` (`chr.c:5305`).
    fn shieldhit_find_parentnode_cmnum(&self, ci: usize, cmnum: usize) -> Option<usize> {
        let def = &self.chrs[ci].model.def;
        let node = def.find_node_by_mtx_index(cmnum)?;
        def.find_parent_mtx_node(node).and_then(|p| def.find_node_mtx_index(p, 0))
    }

    /// `shieldhit_find_childnode_cmnum` (`chr.c:5326`).
    fn shieldhit_find_childnode_cmnum(&self, ci: usize, cmnum: usize) -> Option<usize> {
        let def = &self.chrs[ci].model.def;
        let node = def.find_node_by_mtx_index(cmnum)?;
        def.find_child_mtx_node(node).and_then(|c| def.find_node_mtx_index(c, 0))
    }

    /// `shieldhit_find_anynode_cmnum` (`chr.c:5360`): the next sibling bone.
    fn shieldhit_find_anynode_cmnum(&self, ci: usize, cmnum: usize) -> Option<usize> {
        let def = &self.chrs[ci].model.def;
        let node = def.find_node_by_mtx_index(cmnum)?;
        def.find_child_or_parent_mtx_node(node).and_then(|c| def.find_node_mtx_index(c, 0))
    }

    /// `shieldhit_create` (`chr.c:5166`): a free slot or the oldest, the hit
    /// box's bone lit at once (unless another of the chr's hits is still
    /// spreading), `CHRH2FLAG_SHIELDHIT`.
    pub(crate) fn shieldhit_create(&mut self, ci: usize, shield: f32, at: Option<ShieldHitAt>) {
        let lvframe60 = self.lv.lvframe60;
        let hits = &self.fx.shieldhits.hits;
        let slot = hits.iter().position(|h| h.is_none()).or_else(|| {
            let mut oldest = None;
            let mut oldestframe = lvframe60;
            for (i, h) in hits.iter().enumerate() {
                if let Some(h) = h {
                    if h.lvframe60 < oldestframe {
                        oldest = Some(i);
                        oldestframe = h.lvframe60;
                    }
                }
            }
            oldest
        });
        if let Some(slot) = slot {
            let unk011 = 2 + (self.rng.random() % 6) as i32;
            let mut hit = ShieldHit {
                chr: ci,
                node: at.map(|a| a.node),
                side: at.map_or(0, |a| a.side),
                lvframe60,
                unk018: [-1; 32],
                unk038: [0; 32],
                unk011,
                shield,
                hitpos: at.map(|a| a.hitpos),
            };
            if let Some(a) = at {
                // PD's @bug: it checks the new hit's own (fresh) table for every
                // other hit of the chr, so the bone is always lit.
                if let Some(index) = self.shieldhit_node_to_cmnum(ci, a.node).filter(|&i| i < 32) {
                    hit.unk018[index] = 0;
                    hit.unk038[index] = 0;
                }
            }
            self.fx.shieldhits.hits[slot] = Some(hit);
            self.chrs[ci].shieldhit = true;
        }
        self.fx.shieldhits.active = true;
    }

    /// `shieldhit_remove` (`chr.c:5255`).
    fn shieldhit_remove(&mut self, slot: usize) {
        let Some(hit) = self.fx.shieldhits.hits[slot].take() else { return };
        self.fx.shieldhits.active = self.fx.shieldhits.hits.iter().any(|h| h.is_some());
        if !self.fx.shieldhits.hits.iter().flatten().any(|h| h.chr == hit.chr) {
            self.chrs[hit.chr].shieldhit = false;
        }
    }

    /// `shieldhits_tick` (`chr.c:6558`), in `props_tick`: each hit's shown
    /// strength eases to the chr's for 80 ticks, and each lit bone passes the
    /// glow to its parent, child and siblings on its first tick, burning for 30.
    pub(crate) fn shieldhits_tick(&mut self) {
        if !self.fx.shieldhits.active {
            return;
        }
        let (lv60, lv60f, lvframe60) = (self.lv.lvupdate60, self.lv.lvupdate60f, self.lv.lvframe60);
        for slot in 0..MAX_SHIELDHITS {
            let Some(mut hit) = self.fx.shieldhits.hits[slot].clone() else { continue };
            let mut changed = false;
            if hit.lvframe60 >= lvframe60 - 80 {
                changed = true;
                let health = self.chr_get_shield(hit.chr);
                hit.shield += (health - hit.shield) * lv60f * 0.0125;
            }
            for j in 0..32 {
                if hit.unk018[j] < 0 {
                    continue;
                }
                changed = true;
                let time60 = hit.unk018[j] + lv60;
                if hit.unk018[j] < 1 && time60 > 0 {
                    let pass = |hit: &mut ShieldHit, index: Option<usize>| {
                        if let Some(i) = index.filter(|&i| i < 32) {
                            if hit.unk018[i] == -1 {
                                hit.unk018[i] = -3;
                                hit.unk038[i] = hit.unk038[j] + 1;
                            }
                        }
                    };
                    let chr = hit.chr;
                    let parent = self.shieldhit_find_parentnode_cmnum(chr, j);
                    pass(&mut hit, parent);
                    let mut index = self.shieldhit_find_childnode_cmnum(chr, j);
                    let mut guard = 0;
                    while let Some(i) = index {
                        pass(&mut hit, Some(i));
                        index = self.shieldhit_find_anynode_cmnum(chr, i);
                        guard += 1;
                        if guard > 64 {
                            break;
                        }
                    }
                }
                hit.unk018[j] = if time60 < 30 { time60 } else { -2 };
            }
            for v in hit.unk018.iter_mut() {
                if *v == -3 {
                    *v = 0;
                }
            }
            self.fx.shieldhits.hits[slot] = Some(hit);
            if !changed {
                self.shieldhit_remove(slot);
            }
        }
    }

    /// `chr_render_shield`'s bookkeeping (`chr.c:6469`) for chr `ci`, once a
    /// frame while it is on a screen: the 300-frame cycle, and during its first
    /// 10 frames the crawl: 1-4 steps from the last bone to its parent, then
    /// child, then siblings, shifting the last four.
    ///
    /// `// SUBST:` PD runs this in `chr_render`, in every view that draws the
    /// chr (the cycle runs faster in split screen) / once a frame.
    pub(crate) fn chr_shield_crawl_tick(&mut self, ci: usize) {
        let shield = self.chr_get_shield(ci);
        if shield > 0.0 && self.lv.lvupdate240 > 0 {
            let c = &mut self.chrs[ci];
            c.cmcount += 1;
            if c.cmcount > 300 {
                c.cmcount = 0;
            }
        }
        let c = &self.chrs[ci];
        let cloaking = c.cloak.fadefrac > 0 && !c.cloak.fadefinished;
        let shows = c.shieldhit || (shield > 0.0 && c.cmcount < 10) || cloaking;
        if !shows || shield <= 0.0 || self.lv.lvupdate240 <= 0 {
            return;
        }
        let numiterations = (self.rng.random() % 4) as i32 + 1;
        let [cm, cm2, _, _] = self.chrs[ci].cmnum;
        let mut newcmnum = cm2;
        let mut candidate: i32 = -1;
        let mut operation = 0;
        let mut again = true;
        let mut i = 0;
        let mut guard = 0;
        while i <= numiterations {
            guard += 1;
            if guard > 64 {
                break;
            }
            match operation {
                0 => {
                    candidate = self.shieldhit_find_parentnode_cmnum(ci, cm.max(0) as usize).map_or(-1, |c| c as i32);
                    operation = 1;
                    if candidate >= 0 {
                        if candidate != cm2 {
                            newcmnum = candidate;
                        }
                        i += 1;
                    } else {
                        again = false;
                    }
                }
                1 => {
                    candidate = self.shieldhit_find_childnode_cmnum(ci, cm.max(0) as usize).map_or(-1, |c| c as i32);
                    if candidate >= 0 {
                        operation = 2;
                        if candidate != cm2 {
                            newcmnum = candidate;
                        }
                        i += 1;
                    } else if again {
                        operation = 0;
                    } else {
                        break;
                    }
                }
                _ => {
                    candidate = self.shieldhit_find_anynode_cmnum(ci, candidate.max(0) as usize).map_or(-1, |c| c as i32);
                    if candidate >= 0 {
                        if candidate != cm2 {
                            newcmnum = candidate;
                        }
                        i += 1;
                    } else {
                        operation = 0;
                    }
                }
            }
        }
        let c = &mut self.chrs[ci];
        c.cmnum = [newcmnum, c.cmnum[0], c.cmnum[1], c.cmnum[2]];
    }
}

/// A box's face corners in PD's `sp180` order (`chr.c:5528`): x min/max ×
/// y min/max × z min/max, `pad` out.
pub fn shield_box_corners(bbox: &[f32; 6], pad: f32) -> [Vec3; 8] {
    let (x0, x1, y0, y1, z0, z1) = ((bbox[0] - pad) as i32 as f32, (bbox[1] + pad) as i32 as f32, (bbox[2] - pad) as i32 as f32, (bbox[3] + pad) as i32 as f32, (bbox[4] - pad) as i32 as f32, (bbox[5] + pad) as i32 as f32);
    [
        Vec3::new(x0, y0, z0),
        Vec3::new(x0, y0, z1),
        Vec3::new(x0, y1, z0),
        Vec3::new(x0, y1, z1),
        Vec3::new(x1, y0, z0),
        Vec3::new(x1, y0, z1),
        Vec3::new(x1, y1, z0),
        Vec3::new(x1, y1, z1),
    ]
}

#[cfg(test)]
mod tests {
    use crate::player::PlayerInput;
    use pd_core::ids::*;

    /// A Falcon round on a shielded simulant: a shield hit on the box it met,
    /// from the face it entered, its bone lit; the glow reaches the
    /// neighbouring bones on the next ticks, and the hit is gone once it has
    /// faded (80 ticks) and spread (30 a bone).
    #[test]
    fn a_shield_hit_glows_spreads_and_fades() {
        let (mut w, _, _) = crate::bot::tests::duel(None);
        w.harness_give_loadout(vec![WEAPON_FALCON2]);
        crate::harness::give_shield(&mut w, 1, 8.0);
        crate::testutil::run(&mut w, &PlayerInput::default(), 120);
        let mut hit = None;
        for t in 0..60 {
            crate::testutil::run(&mut w, &PlayerInput { fire: t % 12 < 6, ..Default::default() }, 1);
            if let Some(h) = w.fx.shieldhits.hits.iter().flatten().find(|h| h.chr == 1) {
                hit = Some(h.clone());
                break;
            }
        }
        let h = hit.expect("no shield hit");
        assert!(w.chrs[1].shieldhit, "CHRH2FLAG_SHIELDHIT");
        assert!(w.chrs[1].cshield < 8.0 && w.chrs[1].damage == 0.0, "the shield took it");
        let node = h.node.expect("the round's part box");
        assert!((0..6).contains(&h.side) && h.hitpos.is_some());
        let lit = w.shieldhit_node_to_cmnum(1, node).unwrap();
        assert!(h.unk018[lit] >= 0, "its bone lit");
        crate::testutil::run(&mut w, &PlayerInput::default(), 3);
        let h2 = w.fx.shieldhits.hits.iter().flatten().find(|x| x.chr == 1).unwrap();
        let spread = h2.unk018.iter().enumerate().filter(|&(i, &v)| i != lit && (v >= 0 || v == -2)).count();
        assert!(spread >= 1, "the glow reached another bone: {:?}", h2.unk018);
        crate::testutil::run(&mut w, &PlayerInput::default(), 400);
        assert!(w.fx.shieldhits.hits.iter().flatten().all(|x| x.chr != 1), "the hit is gone");
        assert!(!w.chrs[1].shieldhit);
    }

    /// The first-person flash: a second long, bright at first, then gone.
    #[test]
    fn the_shield_flash_lasts_a_second() {
        let mut s = crate::player::health::ShieldShow::default();
        let mut rng = pd_core::rng::Rng::new(5);
        assert!(s.player_render_shield([0.0, 0.0, 320.0, 220.0], 8.0, 1.0).is_none());
        s.player_display_shield(&mut rng, 1234);
        let first = s.player_render_shield([0.0, 0.0, 320.0, 220.0], 8.0, 1.0).unwrap();
        assert_eq!(first.primalpha, 175);
        assert!(first.half[0] >= 320.0 && first.half[1] >= 220.0, "it covers the view");
        let mut frames = 1;
        while s.player_render_shield([0.0, 0.0, 320.0, 220.0], 8.0, 1.0).is_some() {
            frames += 1;
            assert!(frames < 100);
        }
        assert_eq!(frames, 61);
    }
}
