//! Glass (`OBJTYPE_GLASS`, `OBJTYPE_TINTEDGLASS`): Grid's windows and glass
//! floors, Area 52's tinted windows. A pane shot to its `maxdamage` breaks
//! (`glass_destroy`, `propobj.c:14192`) into shards with the shatter sound,
//! and, as every setup object in a match, comes back 20 s later
//! (`prop_execute_tick_operation`'s `TICKOP_FREE` with `OBJH2FLAG_CANREGEN`).
//! A tinted pane's portal opens for good once it is gone.

use pd_core::ids::*;

use crate::propsnd::DEFAULT_DISTS;
use crate::stage::rooms::{portal_is_closed, PORTALFLAG_FORCEOPEN};
use crate::world::World;

/// `SFXMAP_8078_GLASS_SHATTER`.
const SFX_GLASS_SHATTER: u16 = 0x8078;

/// `struct tintedglassobj`'s own fields.
#[derive(Clone, Copy, Debug)]
pub struct TintedGlass {
    /// Clear within `xludist`, opaque beyond `opadist` (cm).
    pub xludist: f32,
    pub opadist: f32,
    /// The opacity when clear (0..1).
    pub unk64: f32,
    /// 0..255, from the last player pass.
    pub opacity: i32,
}

/// `glass_calculate_opacity` (`propobj.c:4850`): 255 beyond `opadist` of the
/// camera, `arg3 × 255` within `xludist`, a ramp between.
pub fn glass_calculate_opacity(pos: glam::Vec3, campos: glam::Vec3, xludist: f32, opadist: f32, arg3: f32) -> i32 {
    let distance = (pos - campos).length();
    if distance > opadist {
        255
    } else if distance < xludist {
        (arg3 * 255.0) as i32
    } else {
        ((((distance - xludist) * (1.0 - arg3)) / (opadist - xludist) + arg3) * 255.0) as i32
    }
}

impl World {
    /// `glass_update_portal` (`propobj.c:10830`) for player `pi`'s pass: the
    /// pane's opacity from the player's camera, and with one player its portal
    /// shut while it is opaque (`g_TintedGlassEnabled`, which would hold it
    /// opaque, is set only by solo AI lists).
    pub(crate) fn tinted_glass_update_portals(&mut self, pi: usize) {
        let campos = self.players[pi].cam.pos();
        let single = self.players.len() == 1;
        for i in 0..self.props.objs.len() {
            let o = &mut self.props.objs[i];
            let Some(t) = o.tinted.as_mut() else { continue };
            t.opacity = glass_calculate_opacity(o.pos, campos, t.xludist, t.opadist, t.unk64);
            let (opacity, portal) = (t.opacity, o.portalnum);
            if let (Some(p), true) = (portal, single) {
                self.bg_set_portal_open_state(p, opacity != 255);
            }
        }
    }

    /// `glass_destroy` (`propobj.c:14192`): shards over the pane's box face
    /// (`shards_create`, with its sound), then the pane is deleted, destroyed.
    /// (Villa's bottles, which shatter as `SHARDTYPE_BOTTLE`, aren't on an arena.)
    pub(crate) fn glass_destroy(&mut self, id: u32) {
        let Some(o) = self.props.get(id) else { return };
        let (pos, rot, b) = (o.pos, o.realrot, o.bbox);
        let (_, _, best) = self.stage.rooms.bg_find_rooms_by_pos(pos, 8);
        self.sound_at(SFX_GLASS_SHATTER, 1.0, pos, DEFAULT_DISTS);
        self.fx.shards.shards_create(&mut self.rng, pos, rot.x_axis, rot.y_axis, rot.z_axis, b.xmin, b.xmax, b.ymin, b.ymax, SHARDTYPE_GLASS, best);
        let o = self.props.get_mut(id).unwrap();
        o.damage = 0.0;
        o.hidden |= OBJHFLAG_DELETING;
        o.hidden2 |= OBJH2FLAG_DESTROYED;
    }

    /// `obj_tick_player`'s deleting object (`propobj.c:11078`) that can
    /// regenerate: a tinted pane first forces its portal open (and waits a
    /// frame if it was shut); then `obj_free` (a tinted pane's portal open for
    /// good) and `TICKOP_FREE`: gone for 20 s, undamaged. False: not yet.
    pub(crate) fn obj_free_to_regen(&mut self, o: &mut super::Obj) -> bool {
        if o.ty == OBJTYPE_TINTEDGLASS {
            if let Some(p) = o.portalnum {
                let pass = self.portalflags.get(p).is_some_and(|&f| portal_is_closed(f));
                if let Some(f) = self.portalflags.get_mut(p) {
                    *f |= PORTALFLAG_FORCEOPEN;
                }
                if pass {
                    return false;
                }
                // obj_free: bg_set_portal_open_state(portal, true), FORCEOPEN.
                self.bg_set_portal_open_state(p, true);
            }
        }
        o.timetoregen = super::pickup::REGEN_TIME60;
        o.damage = 0.0;
        o.hidden |= OBJHFLAG_GONE;
        o.hidden &= !OBJHFLAG_DELETING;
        o.hidden2 &= !OBJH2FLAG_DESTROYED;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::props::lift::tests_support::floor_centre;
    use crate::testutil;
    use glam::Vec3;
    use pd_core::ids::*;

    /// A player stands on one of Grid's glass floors; four Falcon rounds'
    /// damage breaks it into shards, the player falls through, and 20 s later
    /// the pane is back, whole.
    #[test]
    fn a_glass_floor_breaks_and_comes_back() {
        let mut w = testutil::arena("mp15");
        let id = w.props.objs.iter().find(|o| o.ty == OBJTYPE_GLASS && !o.floors.is_empty()).expect("a glass floor").id;
        let c = floor_centre(w.props.get(id).unwrap());
        let (floors, level) = (w.prop_floors(), w.level.clone());
        w.players[0].start_new_life(&level, &floors, c + Vec3::Y * 100.0, 0.0);
        assert!((w.players[0].manground - c.y).abs() < 1.0, "not on the glass: {} for {}", w.players[0].manground, c.y);
        let idle = crate::player::PlayerInput::default();
        w.step(4, std::slice::from_ref(&idle));
        for k in 0..4 {
            assert!(!w.props.get(id).unwrap().is_deleting(), "broke after {k} rounds");
            w.obj_damage_by_gunfire(id, 1.0, WEAPON_FALCON2, 0);
        }
        assert!(w.props.get(id).unwrap().is_deleting());
        assert!(w.fx.shards.shards.iter().any(|s| s.age60 > 0), "no shards");
        for _ in 0..60 {
            w.step(4, std::slice::from_ref(&idle));
        }
        assert!(w.props.get(id).unwrap().is_gone());
        assert!(w.players[0].manground < c.y - 100.0 || w.players[0].isdead, "the player stayed up at {}", w.players[0].manground);
        for _ in 0..60 * 21 {
            w.step(4, std::slice::from_ref(&idle));
        }
        let o = w.props.get(id).unwrap();
        assert!(!o.is_gone() && !o.is_deleting() && o.damage == 0.0, "the pane didn't come back");
        assert!(!w.prop_floors().is_empty());
    }
}
