//! A shaped room: an L or a U, a rectangle (or more) cut from its lot (`Space::cuts`,
//! `level::shapes`). New in the Rust (docs/AUTOLEVEL.md, "Shaped rooms"), a first try at rooms
//! that are not rectangles.
//!
//! The cut is solid on every storey: not the room's (`Interior::cut`; `inside` and `holds` say
//! no to it, so ramps, cover, platforms, angled catwalks and rails keep out of it and take its
//! walls for walls), nobody's in the router (no door or corridor reaches it), and out of the
//! room's air in the plan (`boxes`). The decks are laid along the rectangle's walls as the
//! Python lays them, then shaped (`shape_decks`): what is in the cut goes, and wherever a deck ran
//! into it a catwalk's depth of deck runs round the cut's inner walls instead, so a ring follows
//! the room round its inner corners.

use super::{grow, Interior};
use crate::cells::{rect_cells, Cells};

impl Interior {
    /// Decks less the cut, and round its inner walls where they ran into it (see the module).
    pub(super) fn shape_decks(&mut self) {
        if self.cut.is_empty() {
            return;
        }
        let band = self.band;
        let collar: Cells = self.cuts.iter().flat_map(|&r| rect_cells(grow(r, band))).filter(|c| self.room.contains(c)).collect();
        for z in self.upper.clone() {
            let shell: Cells = self.shell[&z].union(&self.roof[&z]).copied().collect();
            let deck = self.deck.get_mut(&z).expect("a storey of the room");
            // (the deck along the rectangle's walls is laid inside the room only, so a catwalk
            // that ran into the cut stops at its edge: it touches the cut)
            let touches = deck.iter().any(|c| self.cut.contains(c) || crate::space::DIRS.iter().any(|(dx, dy)| self.cut.contains(&(c.0 + dx, c.1 + dy))));
            if !touches {
                continue;
            }
            deck.retain(|c| !self.cut.contains(c));
            deck.extend(collar.iter().filter(|c| !shell.contains(c)).copied());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cells::{rect_cells, Cells};
    use crate::level::{generate, Options};
    use crate::place::Scale;

    #[test]
    fn a_shaped_room_is_an_l_or_a_u_and_nothing_stands_in_its_cut() {
        let (mut levels, mut shaped, mut ls, mut us, mut flat) = (0, 0, 0, 0, 0);
        for (scale, seeds) in [(Scale::Grand, 1..=10), (Scale::Towering, 1..=6)] {
            for seed in seeds {
                let mut o = Options { platforms: 1.0, angled: 1.0, flooded: 0.0, ..Options::default() };
                o.siting.scale = scale;
                let plain = generate(seed, &o);
                o.shaped = 1.0;
                let lv = generate(seed, &o);
                assert_eq!(
                    plain.is_ok(),
                    lv.is_ok(),
                    "seed {seed}: a shape changes whether a level is made ({:?} / {:?})",
                    plain.as_ref().err(),
                    lv.as_ref().err()
                );
                let Ok(lv) = lv else { continue };
                levels += 1;
                for (key, it) in &lv.interiors {
                    // (a hall its shape can't furnish is furnished as its rectangle: `it.s` has no cut)
                    let s = &it.s;
                    if s.cuts.is_empty() {
                        assert!(it.cut.is_empty());
                        continue;
                    }
                    shaped += 1;
                    flat += usize::from(it.upper.is_empty());
                    let at = format!("seed {seed}: {key}");
                    let cut = &it.cut;
                    let corner = s.cuts.iter().all(|c| (c.0 == s.x0 || c.2 == s.x1) && (c.1 == s.y0 || c.3 == s.y1));
                    if corner {
                        ls += 1;
                    } else {
                        us += 1;
                    }
                    assert!(cut.is_disjoint(&it.room), "{at}: the cut is the room's");
                    // nothing of the room in it, on any storey
                    for &z in &it.levels {
                        assert!(it.walk(z).is_disjoint(cut), "{at}@{z}: floor or deck in the cut");
                    }
                    for r in &it.ramps {
                        let all: Cells = r.cells().union(&r.top).chain(r.foot.iter()).copied().collect();
                        assert!(all.is_disjoint(cut), "{at}: a ramp {:?} in the cut", r.rect);
                    }
                    for c in &it.cover {
                        assert!(rect_cells((c.0, c.1, c.2, c.3)).is_disjoint(cut), "{at}: cover in the cut");
                    }
                    for a in &it.angled {
                        assert!(a.footprint.is_disjoint(cut), "{at}: an angled catwalk over the cut");
                    }
                    // no door opens onto it, and every door still reaches every other
                    for (pad, z) in it.targets() {
                        assert!(pad.is_disjoint(cut), "{at}@{z}: a door onto the cut");
                    }
                    assert!(it.connected(), "{at}: doors don't join");
                    // its air is the shape: no box of the plan's room in the cut
                    let room = lv.plan.get(key).expect("a room in the plan");
                    for &(lo, hi) in &room.boxes {
                        for c in &s.cuts {
                            let (cx0, cy0, cx1, cy1) = (
                                crate::plan::cells(c.0 as f64),
                                crate::plan::cells(c.1 as f64),
                                crate::plan::cells(c.2 as f64),
                                crate::plan::cells(c.3 as f64),
                            );
                            let apart = hi.0 <= cx0 || cx1 <= lo.0 || hi.1 <= cy0 || cy1 <= lo.1;
                            assert!(apart, "{at}: air {lo:?}..{hi:?} in the cut {c:?}");
                        }
                    }
                }
            }
        }
        assert!(
            levels >= 12 && shaped >= 15 && ls >= 4 && us >= 4, // (a room of one storey only from `level::LOW_LEAST` across: few are)
            "{levels} levels, {shaped} shaped rooms ({ls} L, {us} U, {flat} of one storey)"
        );
        // none when told none
        assert!(generate(40, &Options { shaped: 0.0, ..Options::default() }).unwrap().interiors.iter().all(|(_, it)| it.cut.is_empty()));
    }
}
