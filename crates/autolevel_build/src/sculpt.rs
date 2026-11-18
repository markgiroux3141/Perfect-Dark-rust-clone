//! A sculpted level's kit (`autolevel::sculpt`, `--style sculpt`): Complex's, under its own name.
//! The planner's rooms are boxes of air like Complex's, so Complex's polygon stage
//! (`geometry::polys`) draws them as they are.

use crate::complex::{PALETTE, TAGS, TARGET, TINTS};
use crate::kit::Kit;

/// Complex's palette, schemes and polygons, as `sculpt`.
pub static KIT: Kit = Kit {
    name: "sculpt",
    tag: "cx",
    stage: "ref",
    palette: &PALETTE,
    schemes: crate::geometry::schemes,
    polys: crate::geometry::polys,
    tags: &TAGS,
    tints: &TINTS,
    target: &TARGET,
};
