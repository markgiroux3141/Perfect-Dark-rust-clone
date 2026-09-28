//! Test stages built from boxes, as PD polygons, so they run through the same
//! collision code as a real arena. Each is one room, room 1 (PD's rooms start
//! at 1; `nbomb_inflict_damage` and friends skip room 0):
//!
//! * [`arena`]: the simulant spike's room, 16 m square with a 3 m ceiling and
//!   four 1.5 m pillars in a pinwheel (no two on one line through the centre, so
//!   there is always an open diagonal and always a pillar to break sight). It is
//!   centred on the origin because `chr_run_from_pos` forgets to add the bot's
//!   position to its flee vector (`chraction.c:15690`), and at the origin that
//!   still reads as "back away".
//! * [`firing_range`]: the gun spike's range, a 12 × 36 m hall with crates and
//!   pillars, with its target boards ([`firing_range_boards`]).
//!
//! Sources: the old repo's `pd_spike/arena.rs` `Arena::geom` and
//! `pd_guns/range.rs` `Range::standard` / `Range::geom`.

use glam::{Vec2, Vec3};

use super::geom::{GeomPoly, LevelGeom};

/// The arena's inner half-width (cm).
pub const ARENA_HALF: f32 = 800.0;
/// The arena's ceiling (cm).
pub const ARENA_HEIGHT: f32 = 300.0;
/// The pillars' centres (x, z) and half-width.
pub const ARENA_PILLARS: [Vec2; 4] = [Vec2::new(-350.0, -150.0), Vec2::new(150.0, -350.0), Vec2::new(350.0, 150.0), Vec2::new(-150.0, 350.0)];
pub const ARENA_PILLAR_HALF: f32 = 75.0;

/// Four walls around an XZ rectangle, from `y0` to `y1`, facing out of it.
fn sides(min: Vec2, max: Vec2, y0: f32, y1: f32, out: &mut Vec<GeomPoly>) {
    let c = [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
    for i in 0..4 {
        let (p, q) = (c[i], c[(i + 1) % 4]);
        let v = vec![Vec3::new(p.x, y0, p.y), Vec3::new(q.x, y0, q.y), Vec3::new(q.x, y1, q.y), Vec3::new(p.x, y1, p.y)];
        out.push(GeomPoly::new(v, false, true, true, true, Some(1)));
    }
}

fn quad_y(min: Vec2, max: Vec2, y: f32) -> Vec<Vec3> {
    vec![Vec3::new(min.x, y, min.y), Vec3::new(max.x, y, min.y), Vec3::new(max.x, y, max.y), Vec3::new(min.x, y, max.y)]
}

/// The simulant spike's box arena.
pub fn arena() -> LevelGeom {
    let (min, max) = (Vec2::splat(-ARENA_HALF), Vec2::splat(ARENA_HALF));
    let mut polys = vec![
        GeomPoly::new(quad_y(min, max, 0.0), true, false, true, true, Some(1)),
        // The ceiling blocks sight and shots but is neither floor nor wall.
        GeomPoly::new(quad_y(min, max, ARENA_HEIGHT), false, false, true, true, Some(1)),
    ];
    sides(min, max, 0.0, ARENA_HEIGHT, &mut polys);
    for c in ARENA_PILLARS {
        sides(c - Vec2::splat(ARENA_PILLAR_HALF), c + Vec2::splat(ARENA_PILLAR_HALF), 0.0, ARENA_HEIGHT, &mut polys);
    }
    LevelGeom { polys, rooms: vec![1] }
}

/// The gun spike's firing range: its hall, crates and pillars.
pub fn firing_range() -> LevelGeom {
    let (lo, hi) = (Vec3::new(-600.0, 0.0, -300.0), Vec3::new(600.0, 400.0, 3300.0));
    let mut solids = Vec::new();
    // Crates: (x, z, half-width, height).
    for (x, z, s, h) in [(-250.0, 350.0, 50.0, 100.0), (220.0, 450.0, 60.0, 120.0), (-120.0, 1300.0, 45.0, 90.0), (330.0, 1500.0, 50.0, 100.0), (0.0, 2300.0, 70.0, 140.0)] {
        solids.push((Vec3::new(x - s, 0.0, z - s), Vec3::new(x + s, h, z + s)));
    }
    // Pillars.
    for (x, z) in [(-420.0, 900.0), (420.0, 900.0), (-420.0, 2000.0), (420.0, 2000.0)] {
        solids.push((Vec3::new(x - 60.0, 0.0, z - 60.0), Vec3::new(x + 60.0, 400.0, z + 60.0)));
    }
    let xz = |v: Vec3| Vec2::new(v.x, v.z);
    let mut polys = vec![GeomPoly::new(quad_y(xz(lo), xz(hi), lo.y), true, false, true, true, Some(1))];
    sides(xz(lo), xz(hi), lo.y, hi.y, &mut polys);
    for (mn, mx) in solids {
        sides(xz(mn), xz(mx), mn.y, mx.y, &mut polys);
        polys.push(GeomPoly::new(quad_y(xz(mn), xz(mx), mx.y), true, false, true, true, Some(1)));
    }
    LevelGeom { polys, rooms: vec![1] }
}

/// A solid box added to a test stage: four walls and a top you can stand on.
pub fn add_box(geom: &mut LevelGeom, min: Vec3, max: Vec3) {
    let xz = |v: Vec3| Vec2::new(v.x, v.z);
    sides(xz(min), xz(max), min.y, max.y, &mut geom.polys);
    geom.polys.push(GeomPoly::new(quad_y(xz(min), xz(max), max.y), true, false, true, true, Some(1)));
}

/// Where a player starts in the firing range: the south end, looking north (+z).
pub const FIRING_RANGE_SPAWN: (Vec3, Vec3) = (Vec3::new(0.0, 50.0, -150.0), Vec3::Z);

/// The range's target boards at 5, 10, 20 and 30 m, 80 × 120 cm, their faces
/// 110 cm up.
pub fn firing_range_boards() -> Vec<crate::world::Board> {
    let half = Vec2::new(40.0, 60.0);
    let centre_y = 110.0;
    [(0.0, 500.0), (-200.0, 1000.0), (200.0, 1000.0), (0.0, 2000.0), (-150.0, 3000.0), (150.0, 3000.0)]
        .into_iter()
        .map(|(x, z)| crate::world::Board {
            min: Vec3::new(x - half.x, centre_y - half.y, z - 4.0),
            max: Vec3::new(x + half.x, centre_y + half.y, z + 4.0),
            face: Vec3::new(x, centre_y, z - 4.0),
            half,
            hits: 0,
            damage: 0.0,
            flash: 0.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::TileLevel;

    #[test]
    fn a_crate_top_is_ground_and_its_sides_are_walls() {
        let l = TileLevel::new(firing_range());
        // On top of the first crate (100 cm high) and beside it.
        assert_eq!(l.cd_find_ground_at_cyl(Vec3::new(-250.0, 169.0, 350.0), 20.0).0, 100.0);
        assert_eq!(l.cd_find_ground_at_cyl(Vec3::new(-250.0, 69.0, 200.0), 20.0).0, 0.0);
        let hit = l.raycast_shoot(Vec3::new(-250.0, 50.0, 0.0), Vec3::Z, 1000.0).unwrap();
        assert!((hit.dist - 300.0).abs() < 0.5, "{}", hit.dist);
    }
}
