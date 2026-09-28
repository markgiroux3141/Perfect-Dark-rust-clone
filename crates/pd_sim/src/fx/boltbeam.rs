//! A crossbow bolt's trail (`g_BoltBeams`, `gunfx.c:897`-`:1010`): a beam from
//! where the bolt left (the head) to the bolt (the tail). While the bolt flies
//! the tail follows it and the head stays within 30 m; once it sticks or goes
//! the beam lets go of it and shrinks into the tail at a fixed speed.
//! `pd_render::fx` draws them (`boltbeams_render`).

use glam::Vec3;

/// `ARRAYCOUNT(g_BoltBeams)`.
pub const MAX_BOLTBEAMS: usize = 8;

/// `boltbeam->unk00`: -1 free, a prop, or 0 once `boltbeam_set_automatic`
/// has let go of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoltOwner {
    #[default]
    Free,
    Prop(u32),
    Detached,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BoltBeam {
    pub owner: BoltOwner,
    /// `BOLTBEAMTICKMODE_AUTOMATIC` (else `_MANUAL`).
    pub automatic: bool,
    pub headpos: Vec3,
    pub tailpos: Vec3,
    /// cm per second the beam shrinks by, automatic.
    pub speed: f32,
}

#[derive(Clone, Debug)]
pub struct BoltBeams {
    pub beams: [BoltBeam; MAX_BOLTBEAMS],
}

impl Default for BoltBeams {
    /// `boltbeams_reset` (`gunfxreset.c:15`).
    fn default() -> Self {
        BoltBeams { beams: [BoltBeam::default(); MAX_BOLTBEAMS] }
    }
}

impl BoltBeams {
    /// `boltbeam_find_by_prop` (`gunfx.c:897`).
    pub fn find(&self, owner: BoltOwner) -> Option<usize> {
        self.beams.iter().position(|b| b.owner == owner)
    }

    /// `boltbeam_create` (`:911`): a free slot, manual, for `id`.
    pub fn create(&mut self, id: u32) -> Option<usize> {
        let i = self.find(BoltOwner::Free)?;
        self.beams[i].automatic = false;
        self.beams[i].owner = BoltOwner::Prop(id);
        Some(i)
    }

    /// `boltbeam_increment_head_pos` (`:946`): pull the head to within `len`
    /// of the tail.
    pub fn increment_head_pos(&mut self, i: usize, len: f32, arg2: bool) {
        let b = &mut self.beams[i];
        let dist = (b.tailpos - b.headpos).length();
        if dist > len && !arg2 {
            let dir = (b.headpos - b.tailpos) / dist;
            b.headpos = b.tailpos + dir * len;
        }
    }

    /// `boltbeam_set_automatic` (`:970`): let go of the bolt and shrink.
    pub fn set_automatic(&mut self, i: usize, speed: f32) {
        let b = &mut self.beams[i];
        b.automatic = true;
        b.owner = BoltOwner::Detached;
        b.speed = speed;
    }

    /// `boltbeams_tick` (`:990`, from `lv_tick`).
    pub fn tick(&mut self, lvupdate60freal: f32) {
        for i in 0..MAX_BOLTBEAMS {
            let b = self.beams[i];
            if b.owner != BoltOwner::Free && b.automatic {
                let length = (b.tailpos - b.headpos).length() - b.speed * lvupdate60freal / 60.0;
                if length < 0.0 {
                    self.beams[i].owner = BoltOwner::Free;
                } else {
                    self.increment_head_pos(i, length, false);
                }
            }
        }
    }

    /// The beams `boltbeams_render` draws.
    pub fn live(&self) -> impl Iterator<Item = &BoltBeam> {
        self.beams.iter().filter(|b| b.owner != BoltOwner::Free)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bolt's beam follows it, then shrinks 1400 cm/s once let go and frees
    /// itself when gone.
    #[test]
    fn a_boltbeam_trails_then_shrinks_away() {
        let mut bb = BoltBeams::default();
        let i = bb.create(7).unwrap();
        bb.beams[i].headpos = Vec3::ZERO;
        bb.beams[i].tailpos = Vec3::new(0.0, 0.0, 4000.0);
        bb.increment_head_pos(i, 3000.0, false);
        assert_eq!(bb.beams[i].headpos, Vec3::new(0.0, 0.0, 1000.0));
        bb.set_automatic(i, 1400.0);
        assert_eq!(bb.find(BoltOwner::Prop(7)), None);
        bb.tick(60.0);
        assert!((bb.beams[i].headpos.z - 2400.0).abs() < 1e-2);
        bb.tick(60.0);
        bb.tick(60.0);
        assert_eq!(bb.live().count(), 0);
    }
}
