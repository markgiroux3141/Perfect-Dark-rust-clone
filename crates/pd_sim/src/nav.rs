//! Navigation in PD's own format (waypoints + waygroups): the verbatim
//! `padhalllv.c` port (`nav_find_route`, `waypoint_find_closest_to_pos`, the seeded
//! tie-breaks). Also our generator, which builds a graph from geometry alone (see
//! the Complex spike's decision), and the static checks S1-S4.
//!
//! Sources: `pd_spike/pd_nav.rs`, `navgen.rs`, `navcheck.rs`.
