//! A kit: what a generated level is dressed in. Its materials (each known by its role: "floor",
//! "panel", "rail"), its wall schemes (which role each strip of a wall takes, bottom-up), the
//! roles of special solids, the room tints and the brightness the bake aims for, and the arena
//! whose own material rows the importer draws the kit's textures with.
//!
//! ```text
//! let kit = kit::named("complex")?;   // or &complex::KIT
//! kit.surface("panel", Some(118.0), None)?   // a role's surface, at its scale
//! kit.palette[face.material]          // a mesh face's material is a place in its kit's palette
//! ```
//!
//! A kit is data; the rules that put its roles on faces (`complex::style`, `geometry::restyle`)
//! are Complex's and name the roles they want, so a kit gives every role they use. Complex is
//! the first kit (`complex::KIT`); its rows are the answer key's, so nothing here may change what
//! it makes. Felicity's (`felicity::KIT`) has rules of its own (`polys`). How a level is shaped
//! (storeys, doors, ramps, the module) is the planner's (`autolevel`), not a kit's.

use crate::csg::{Poly, Surface};
use crate::geometry::Lit;
use crate::{complex, felicity};
use autolevel::level::Level;
use autolevel::py;

/// A material of a kit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    /// What it is for: the name surfaces know it by.
    pub role: &'static str,
    /// The ROM texture preset.
    pub preset: &'static str,
    /// GE material flags.
    pub flags: &'static [&'static str],
    /// Its name in the source level's Setup Editor export.
    pub name: &'static str,
    /// Units per texture repeat.
    pub scale: (f64, f64),
}

/// Wall strips, bottom-up: (strip base z, role).
pub type Scheme = Vec<(i32, &'static str)>;
/// The wall schemes by name.
pub type Schemes = Vec<(&'static str, Scheme)>;

/// The scheme of this name.
pub fn scheme<'a>(schemes: &'a Schemes, name: &str) -> Option<&'a Scheme> {
    schemes.iter().find(|(k, _)| *k == name).map(|(_, s)| s)
}

/// The role of the strip whose base is at `z`.
pub fn strip(scheme: &Scheme, z: i32) -> Option<&'static str> {
    scheme.iter().find(|(b, _)| *b == z).map(|(_, r)| *r)
}

/// A kit (see the module).
#[derive(Debug)]
pub struct Kit {
    /// Its name, as a recipe names it.
    pub name: &'static str,
    /// The prefix of its materials' names in a GLB (`cx_floor`).
    pub tag: &'static str,
    /// The arena whose material rows draw the kit's textures in the game
    /// (`assets/stages/<stage>/bg.json`): found by texture, culling and layer.
    pub stage: &'static str,
    /// Its materials. A mesh face's material is a place in this list.
    pub palette: &'static [Paint],
    /// The wall schemes a level is built with.
    pub schemes: fn() -> Schemes,
    /// Every polygon of a level in this kit, and the beams that light some (`geometry::polys`).
    pub polys: fn(&Level, &Kit) -> Result<(Vec<Poly>, Vec<Lit>), String>,
    /// Solid tags used by level plans, beyond the built-in ones: (tag, role).
    pub tags: &'static [(&'static str, &'static str)],
    /// Whole-room tints by name, as multipliers.
    pub tints: &'static [(&'static str, [f64; 3])],
    /// The median brightness the bake aims a room's faces at, by kind (floor, wall, ceiling).
    pub target: &'static [(&'static str, i32)],
}

impl Kit {
    /// The palette's place for a role. Err if the kit has no such role.
    pub fn index(&self, role: &str) -> Result<usize, String> {
        self.palette.iter().position(|p| p.role == role).ok_or_else(|| format!("the palette has no {}", py::repr(role)))
    }

    /// The palette's entry for a role. Err if the kit has no such role.
    pub fn paint(&self, role: &str) -> Result<&'static Paint, String> {
        let palette: &'static [Paint] = self.palette;
        Ok(&palette[self.index(role)?])
    }

    /// A role's surface: its scale from the palette unless one is given.
    pub fn surface(&self, role: &str, band: Option<f64>, scale: Option<(f64, f64)>) -> Result<Surface, String> {
        let p = self.paint(role)?;
        Ok(Surface::new(p.role, scale.unwrap_or(p.scale), band))
    }

    /// The tint of this name.
    pub fn tint(&self, name: &str) -> Result<[f64; 3], String> {
        self.tints.iter().find(|(k, _)| *k == name).map(|(_, t)| *t).ok_or_else(|| format!("no tint {}", py::repr(name)))
    }

    /// The brightness a face of this kind is baked to (a floor's for a kind it has none for).
    pub fn target(&self, kind: &str) -> f64 {
        self.target.iter().find(|(k, _)| *k == kind).or_else(|| self.target.iter().find(|(k, _)| *k == "floor")).map_or(167.0, |(_, v)| *v as f64)
    }

    /// The presets of the palette's materials `used` (places in it), each once, in that order.
    pub fn presets(&self, used: impl IntoIterator<Item = usize>) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for m in used {
            let preset = self.palette[m].preset;
            if !out.contains(&preset) {
                out.push(preset);
            }
        }
        out
    }
}

/// Every kit, by the style it dresses (`Options::style`).
pub const KITS: [&Kit; 3] = [&complex::KIT, &felicity::KIT, &crate::sculpt::KIT];

/// The kit of this name.
pub fn named(name: &str) -> Result<&'static Kit, String> {
    KITS.into_iter().find(|k| k.name == name).ok_or_else(|| {
        let names: Vec<&str> = KITS.iter().map(|k| k.name).collect();
        format!("no kit {} (there are {})", py::repr(name), names.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kits_by_name_and_roles_by_place() {
        let cx = named("complex").unwrap();
        assert_eq!((cx.tag, cx.stage, cx.palette.len()), ("cx", "ref", 20));
        assert_eq!(named("moss").unwrap_err(), "no kit 'moss' (there are complex, felicity, sculpt)");
        assert_eq!(named("felicity").unwrap().stage, "mp11");
        assert_eq!((cx.index("floor"), cx.index("rail")), (Ok(0), Ok(18)));
        assert_eq!(cx.paint("moss").unwrap_err(), "the palette has no 'moss'");
        assert_eq!(cx.tint("cyan"), Ok([0.5, 1.0, 1.0]));
        assert_eq!((cx.target("wall"), cx.target("pier")), (131.0, 167.0));
        assert_eq!(cx.presets([0, 1, 3, 0]), ["073E", "0746"]);
    }
}
