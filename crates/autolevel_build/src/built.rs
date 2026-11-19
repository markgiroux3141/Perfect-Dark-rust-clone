//! A level as it is built, in memory: what a game that makes the level itself reads, in place of
//! the files `autolevel build` writes.
//!
//! ```text
//! let b = built::make(&recipe, true)?;   // the layout, its polygons, its room meshes, baked
//! b.level                 // the layout: plan, routes, walks
//! b.rooms                 // a mesh for each room of the plan, by its key and number
//! b.openings()            // every face two rooms' air shares: the doorways, windows, drops
//! ```
//!
//! A recipe's level is the same every time, so a reader need not go through a file: the
//! importer's source for generated levels (`pd_import::autolevel`) makes the level again from
//! the recipe the build folder keeps, and gets rooms, colours and openings exactly, with no
//! format between.

use autolevel::level::Level;
use autolevel::recipe::Recipe;

use crate::bake;
use crate::geometry::Lit;
use crate::kit::{self, Kit};
use crate::mesh::{self, Room};
use crate::openings::{self, Opening};

/// A level made: its layout and its room meshes.
#[derive(Clone, Debug)]
pub struct Built {
    pub level: Level,
    /// What it is dressed in: a face's material is a place in this kit's palette.
    pub kit: &'static Kit,
    /// A mesh for each room of the plan that has polygons, in the order they are first met.
    pub rooms: Vec<Room>,
    /// The beams that light some of the polygons (`Face::lit`).
    pub lit: Vec<Lit>,
    /// Whether the rooms' colours are the baked lighting (else white).
    pub baked: bool,
}

/// The level a recipe makes, its polygons as room meshes, lit by the bake's usual settings if
/// `bake`, in the kit of its style. Err as `Recipe::generate`, or if a stage after it fails.
pub fn make(recipe: &Recipe, bake: bool) -> Result<Built, String> {
    of(recipe.generate()?, kit::named(recipe.options.style)?, bake)
}

/// The rest of `make`, from a layout, dressed in `kit`.
pub fn of(level: Level, kit: &'static Kit, bake: bool) -> Result<Built, String> {
    let (polys, lit) = (kit.polys)(&level, kit)?;
    let mut rooms = mesh::emit(&level, &polys, kit)?;
    if bake {
        bake::light(&level, &mut rooms, &lit, kit, &bake::Settings::default())?;
    }
    Ok(Built { level, kit, rooms, lit, baked: bake })
}

impl Built {
    /// Every opening between two rooms (`openings::openings` of the plan).
    pub fn openings(&self) -> Vec<Opening> {
        openings::openings(&self.level.plan)
    }

    /// The texture presets the level draws, in the order its rooms first use them.
    pub fn presets(&self) -> Vec<&'static str> {
        self.kit.presets(self.rooms.iter().flat_map(|r| r.faces.iter().map(|f| f.material)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recipe_is_built_with_its_rooms_and_openings() {
        let b = make(&Recipe::python(40), true).unwrap();
        // as the Blender build of seed 40: 61 rooms, 15,303 triangles
        assert_eq!((b.rooms.len(), b.rooms.iter().map(Room::triangles).sum::<usize>()), (61, 15303));
        assert!(b.baked && b.rooms.iter().any(|r| r.faces.iter().any(|f| f.colors.iter().any(|c| *c != [255; 3]))), "baked colours");
        assert_eq!(b.presets().len(), 16);
        // every opening joins two rooms that have meshes
        let openings = b.openings();
        assert!(!openings.is_empty());
        for o in &openings {
            assert!(b.rooms.iter().any(|r| r.key == o.low) && b.rooms.iter().any(|r| r.key == o.high), "{o:?}");
        }
        // and unbaked, the colours are white
        let plain = make(&Recipe::python(40), false).unwrap();
        assert!(plain.rooms.iter().all(|r| r.faces.iter().all(|f| f.colors.iter().all(|c| *c == [255; 3]))));
    }
}
