//! The level's storeys: a floor every 7 m (275.59 units, rounded) from -276, each 236 high
//! (the low road's ceiling is the slab under the ground floor). From `autolevel/route.py`, whose
//! table stops at 1654 (seven storeys over the low road).
//!
//! New in the Rust: the storeys above 1654, for an atrium fifteen storeys tall. Nothing the
//! Python scatters reaches them, so a seed's level is the same with them as without.

/// Floor levels, lowest first.
pub const LEVELS: [i32; 17] = [-276, 0, 276, 551, 827, 1102, 1378, 1654, 1929, 2205, 2480, 2756, 3031, 3307, 3583, 3858, 4134];
/// The ceiling over each floor level.
pub const CEILINGS: [i32; 17] = [-39, 236, 512, 787, 1063, 1338, 1614, 1890, 2165, 2441, 2716, 2992, 3267, 3543, 3819, 4094, 4370];

/// (floor, ceiling) of the storey whose floor is `z`.
pub fn storey(z: i32) -> (i32, i32) {
    (z, CEILINGS[index(z)])
}

/// Which storey has its floor at `z`.
pub fn index(z: i32) -> usize {
    LEVELS.iter().position(|&l| l == z).unwrap_or_else(|| panic!("{z} is not a floor level"))
}

/// The ceiling line nearest a scattered room's top (the lower of two as near).
pub fn snap_top(top: i32) -> i32 {
    crate::py::first_min(CEILINGS, |c| (c - top).abs()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tops_snap_to_ceiling_lines() {
        assert_eq!([748, 1024, 236, 1614, 1890, 100].map(snap_top), [787, 1063, 236, 1614, 1890, 236]);
        assert_eq!(storey(-276), (-276, -39));
        assert_eq!(storey(551), (551, 787));
    }
}
