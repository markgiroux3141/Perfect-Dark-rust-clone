//! The climbs of a level: for every pod on a towering hall's high storey (`Space::high`), the way
//! up to it from the hall's own floor. Such a pod is never reached from below inside its hall: its
//! storey is a place of its own, joined only by doors from outside, so its climb goes out of the
//! hall and back in through other rooms. How long that way is says how hard the pod is to get to,
//! which is what a towering level is for. New in the Rust (docs/AUTOLEVEL.md, "Atriums").

use crate::level::Level;
use std::collections::{BTreeMap, VecDeque};

/// The way up to one pod on a high storey.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Climb {
    pub pod: String,
    /// Its hall, and the storey it is on.
    pub hall: String,
    pub storey: i32,
    /// The rooms passed through, in order, from the hall (its floor) to the pod: the hall comes
    /// again where the way goes back into it, on the pod's storey.
    pub way: Vec<String>,
}

impl Climb {
    /// The rooms the way goes through between leaving the hall and coming back into it.
    pub fn rooms(&self) -> usize {
        self.way.len().saturating_sub(3)
    }
}

/// A place: a room, or a towering hall's high storey (`key@floor`).
fn place(lv: &Level, room: &str, level: i32) -> String {
    if lv.space(room).high.contains(&level) {
        format!("{room}@{level}")
    } else {
        room.to_string()
    }
}

/// Every pod on a high storey and the way up to it by the fewest rooms, over the routes
/// (corridors, stubs, vents) and the pods' doors. Drops are left out: they only go down. A pod
/// with no way up from its hall's floor has no climb.
pub fn climbs(lv: &Level) -> Vec<Climb> {
    let mut next: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut join = |a: String, b: String| {
        next.entry(a.clone()).or_default().push(b.clone());
        next.entry(b).or_default().push(a);
    };
    for r in lv.net.routes() {
        join(place(lv, &r.a.room, r.a.level), place(lv, &r.b.room, r.b.level));
    }
    for s in lv.spaces().iter().filter(|s| !s.host.is_empty()) {
        join(s.key.clone(), place(lv, &s.host, s.floor));
    }
    let mut out = Vec::new();
    for pod in lv.spaces().iter().filter(|s| !s.host.is_empty() && lv.space(&s.host).high.contains(&s.floor)) {
        let mut from: BTreeMap<&str, &str> = BTreeMap::from([(pod.host.as_str(), "")]);
        let mut todo = VecDeque::from([pod.host.as_str()]);
        while let Some(at) = todo.pop_front() {
            if at == pod.key {
                let mut way = vec![at];
                while let Some(&back) = from.get(way[way.len() - 1]).filter(|b| !b.is_empty()) {
                    way.push(back);
                }
                way.reverse();
                let way = way.into_iter().map(|p| p.split('@').next().unwrap_or(p).to_string()).collect();
                out.push(Climb { pod: pod.key.clone(), hall: pod.host.clone(), storey: pod.floor, way });
                break;
            }
            for n in next.get(at).into_iter().flatten() {
                if !from.contains_key(n.as_str()) {
                    from.insert(n, at);
                    todo.push_back(n);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{generate, Options};
    use crate::place::Scale;

    #[test]
    fn every_high_pod_of_a_towering_level_is_climbed_to_from_outside() {
        let mut o = Options::default();
        o.siting.scale = Scale::Towering;
        let (mut levels, mut long) = (0, 0);
        for seed in 1..=10 {
            let Ok(lv) = generate(seed, &o) else { continue };
            levels += 1;
            let cs = climbs(&lv);
            for s in lv.spaces().iter().filter(|s| !s.high.is_empty()) {
                assert!(s.towers(), "seed {seed}: {} has high storeys and does not tower", s.key);
                for z in &s.high {
                    // every high storey holds a pod, and every one is climbed to
                    let pods: Vec<&str> = lv.spaces().iter().filter(|p| p.host == s.key && p.floor == *z).map(|p| p.key.as_str()).collect();
                    assert!(!pods.is_empty(), "seed {seed}: {}@{z} holds no pod", s.key);
                    for p in pods {
                        let c = cs.iter().find(|c| c.pod == p).unwrap_or_else(|| panic!("seed {seed}: no way up to {p}"));
                        assert_eq!(c.way[0], s.key);
                        long += usize::from(c.rooms() >= 3);
                    }
                }
            }
        }
        assert!(levels >= 8 && long >= 5, "{levels} levels, {long} climbs through three rooms or more");
        // a level of another scale has no high storeys
        assert!(generate(40, &Options::default()).unwrap().spaces().iter().all(|s| s.high.is_empty()));
    }
}
