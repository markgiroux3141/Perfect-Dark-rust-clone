//! How a plan's rooms hang off its circulation: the corridors, halls and stair rooms players move
//! through (rooms of kind "corridor", "hub" or "stair"). Not a port.
//!
//! ```text
//! let c = circulation::of(&lv.plan);
//! c.depth                 // rooms by how many doorways they are from the nearest circulation
//! c.misses(&TARGETS)      // what falls short of the targets, in words (empty: none)
//! ```
//!
//! The measures are the ones taken of PD's arenas on 2026-11-17 (docs/AUTOLEVEL.md, "Felicity";
//! rooms joined by wide openings merged into spaces first, as a plan's rooms already are):
//!
//! | | Felicity | Complex |
//! |---|---|---|
//! | spaces, of them circulation | 20, 3 | 33, 14 |
//! | rooms right off circulation | 65% | 84% |
//! | the deepest room | 4 | 2 |
//! | rooms with one way in | 12% | 16% |
//! | loops (links - spaces + 1) | 6 | 13 |
//! | circulation joined to circulation | 0 times | 10 times |
//!
//! `TARGETS` are what the user asked for (2026-11-17): Felicity's look, but a level of
//! corridors with rooms as offshoots, some rooms off rooms but none nested deep: Complex's
//! circulation more than Felicity's.

use crate::plan::Plan;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The kinds of room that are circulation.
pub const KINDS: [&str; 3] = ["corridor", "hub", "stair"];

/// What `of` measures.
#[derive(Clone, Debug, PartialEq)]
pub struct Circulation {
    /// Every room of the plan, and those that are circulation.
    pub spaces: usize,
    pub circulation: usize,
    /// The other rooms by their depth: `depth[d]` rooms are d doorways from the nearest
    /// circulation (d from 1); rooms no circulation reaches are `unreached`.
    pub depth: Vec<usize>,
    pub unreached: usize,
    /// Rooms (not circulation) with one way in.
    pub one_way: usize,
    /// Links (pairs of rooms joined) less rooms plus one.
    pub loops: i64,
    /// Pairs of circulation rooms joined directly, and whether the circulation is one piece
    /// on its own (anywhere to anywhere without passing through a room).
    pub joins: usize,
    pub connected: bool,
    /// Rooms that join two or more circulation rooms (short cuts through a room).
    pub shortcuts: usize,
    /// Circulation's share of the plan's floor (each room's boxes' footprints).
    pub floor_share: f64,
}

impl Circulation {
    pub fn rooms(&self) -> usize {
        self.spaces - self.circulation
    }

    /// The share of rooms right off circulation.
    pub fn off_circulation(&self) -> f64 {
        self.depth.get(1).copied().unwrap_or(0) as f64 / self.rooms().max(1) as f64
    }

    /// The deepest room's depth.
    pub fn deepest(&self) -> usize {
        self.depth.len().saturating_sub(1)
    }

    /// One line, for a table of seeds.
    pub fn row(&self) -> String {
        let depth: Vec<String> = self.depth.iter().enumerate().skip(1).map(|(d, n)| format!("{d}:{n}")).collect();
        format!(
            "{} spaces ({} circulation{}, {} joins), rooms by depth {} ({:.0}% at 1), {} one way in, {} loops, {} shortcuts, circulation {:.0}% of the floor",
            self.spaces,
            self.circulation,
            if self.connected { ", one piece" } else { ", in pieces" },
            self.joins,
            depth.join(" "),
            100.0 * self.off_circulation(),
            self.one_way,
            self.loops,
            self.shortcuts,
            100.0 * self.floor_share
        )
    }

    /// What falls short of `t`, in words; empty if nothing does.
    pub fn misses(&self, t: &Targets) -> Vec<String> {
        let mut out = Vec::new();
        let rooms = self.rooms().max(1) as f64;
        if !(t.spaces.0..=t.spaces.1).contains(&self.spaces) {
            out.push(format!("{} spaces (want {} to {})", self.spaces, t.spaces.0, t.spaces.1));
        }
        if self.circulation < t.circulation {
            out.push(format!("{} circulation rooms (want {} or more)", self.circulation, t.circulation));
        }
        if t.connected && !self.connected {
            out.push("the circulation is in pieces (want one: corridors, halls and stairs reach each other without passing through a room)".into());
        }
        if self.unreached > 0 {
            out.push(format!("{} rooms no circulation reaches", self.unreached));
        }
        if self.off_circulation() < t.off_circulation {
            out.push(format!("{:.0}% of rooms right off circulation (want {:.0}% or more)", 100.0 * self.off_circulation(), 100.0 * t.off_circulation));
        }
        if self.deepest() > t.deepest {
            out.push(format!("a room {} doorways deep (want {} at most)", self.deepest(), t.deepest));
        }
        if self.one_way as f64 / rooms > t.one_way {
            out.push(format!("{} of {} rooms with one way in (want {:.0}% at most)", self.one_way, self.rooms(), 100.0 * t.one_way));
        }
        if (self.loops as f64) < t.loops * self.spaces as f64 {
            out.push(format!("{} loops (want {:.0} or more: {} a space)", self.loops, (t.loops * self.spaces as f64).ceil(), t.loops));
        }
        out
    }
}

/// What a level should meet (`Circulation::misses`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Targets {
    pub spaces: (usize, usize),
    pub circulation: usize,
    pub connected: bool,
    pub off_circulation: f64,
    pub deepest: usize,
    /// The most rooms with one way in, as a share of rooms.
    pub one_way: f64,
    /// The least loops, a share of the spaces.
    pub loops: f64,
}

/// A Felicity level's targets: corridors with rooms as offshoots (see the module).
pub const TARGETS: Targets = Targets { spaces: (18, 34), circulation: 4, connected: true, off_circulation: 0.75, deepest: 2, one_way: 0.4, loops: 0.2 };

/// Measure a plan.
pub fn of(plan: &Plan) -> Circulation {
    let keys: Vec<&str> = plan.rooms.iter().map(|r| r.key.as_str()).collect();
    let circ: BTreeSet<&str> = plan.rooms.iter().filter(|r| KINDS.contains(&r.kind)).map(|r| r.key.as_str()).collect();
    let mut adj: BTreeMap<&str, BTreeSet<&str>> = keys.iter().map(|k| (*k, BTreeSet::new())).collect();
    for l in &plan.links {
        if l.a != l.b {
            adj.get_mut(l.a.as_str()).map(|s| s.insert(l.b.as_str()));
            adj.get_mut(l.b.as_str()).map(|s| s.insert(l.a.as_str()));
        }
    }
    let pairs: usize = adj.values().map(|s| s.len()).sum::<usize>() / 2;
    // depth: breadth first from every circulation room at once
    let mut depth: BTreeMap<&str, usize> = circ.iter().map(|k| (*k, 0)).collect();
    let mut q: VecDeque<&str> = circ.iter().copied().collect();
    while let Some(k) = q.pop_front() {
        let d = depth[k];
        for n in &adj[k] {
            if !depth.contains_key(n) {
                depth.insert(n, d + 1);
                q.push_back(n);
            }
        }
    }
    let rooms: Vec<&str> = keys.iter().copied().filter(|k| !circ.contains(k)).collect();
    let mut by = vec![0usize];
    let mut unreached = 0;
    for k in &rooms {
        match depth.get(k) {
            Some(&d) => {
                if by.len() <= d {
                    by.resize(d + 1, 0);
                }
                by[d] += 1;
            }
            None => unreached += 1,
        }
    }
    // the circulation on its own
    let joins = circ.iter().map(|k| adj[k].iter().filter(|n| circ.contains(*n)).count()).sum::<usize>() / 2;
    let connected = match circ.iter().next() {
        None => false,
        Some(first) => {
            let mut seen = BTreeSet::from([*first]);
            let mut q = vec![*first];
            while let Some(k) = q.pop() {
                for n in adj[k].iter().filter(|n| circ.contains(*n)) {
                    if seen.insert(n) {
                        q.push(n);
                    }
                }
            }
            seen.len() == circ.len()
        }
    };
    let shortcuts = rooms.iter().filter(|k| adj[*k].iter().filter(|n| circ.contains(*n)).count() >= 2).count();
    let area = |key: &str| -> f64 {
        let r = plan.get(key).expect("a room of the plan");
        r.boxes.iter().map(|(lo, hi)| ((hi.0 - lo.0) as f64 * (hi.1 - lo.1) as f64).max(0.0)).sum()
    };
    let total: f64 = keys.iter().map(|k| area(k)).sum();
    let circ_area: f64 = circ.iter().map(|k| area(k)).sum();
    Circulation {
        spaces: keys.len(),
        circulation: circ.len(),
        depth: by,
        unreached,
        one_way: rooms.iter().filter(|k| adj[*k].len() == 1).count(),
        loops: pairs as i64 - keys.len() as i64 + 1,
        joins,
        connected,
        shortcuts,
        floor_share: if total > 0.0 { circ_area / total } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Room;

    #[test]
    fn rooms_are_measured_from_the_nearest_circulation() {
        // a corridor, a room off it, a room off that, and a hall joined to the corridor
        let mut p = Plan::new("t");
        let b = |x: i32| vec![((x, 0, 0), (x + 100, 100, 100))];
        for (k, kind, x) in [("c", "corridor", 0), ("r1", "room", 100), ("r2", "room", 200), ("h", "hub", -100)] {
            let mut r = Room::new(k, k, 0, b(x));
            r.kind = kind;
            p.room(r).unwrap();
        }
        for (a, z) in [("c", "r1"), ("r1", "r2"), ("c", "h")] {
            p.link(a, z, "door", "").unwrap();
        }
        let c = of(&p);
        assert_eq!((c.spaces, c.circulation, c.depth.clone(), c.unreached), (4, 2, vec![0, 1, 1], 0));
        assert_eq!((c.one_way, c.loops, c.joins, c.connected, c.shortcuts), (1, 0, 1, true, 0));
        assert!((c.floor_share - 0.5).abs() < 1e-9);
        assert_eq!(c.deepest(), 2);
        let t = Targets { loops: 0.0, spaces: (1, 10), ..TARGETS };
        assert_eq!(
            c.misses(&t),
            vec![
                "2 circulation rooms (want 4 or more)",
                "50% of rooms right off circulation (want 75% or more)",
                "1 of 2 rooms with one way in (want 40% at most)"
            ]
        );
    }
}
