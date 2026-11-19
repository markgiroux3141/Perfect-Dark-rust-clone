//! The climbs of a level: for every pod on a towering hall's high storey (`Space::high`), the way
//! up to it from the hall's own floor. Such a pod is never reached from below inside its hall: its
//! storey is a place of its own, joined only by doors from outside, so its climb goes out of the
//! hall and back in through other rooms. How long that way is says how hard the pod is to get to,
//! which is what a towering level is for. New in the Rust (docs/AUTOLEVEL.md, "Atriums").

use crate::level::Level;
use std::collections::{BTreeMap, VecDeque};

/// The way up to one pod on a high storey, or to a gallery (`Space::galleries`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Climb {
    /// The pod's key, or the gallery's place (`hall@storey`).
    pub pod: String,
    /// It is a gallery: the way ends in the hall, on that storey.
    pub gallery: bool,
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
        self.way.len().saturating_sub(if self.gallery { 2 } else { 3 })
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
    // (target place, hall, storey, a gallery?)
    let mut ends: Vec<(String, String, i32, bool)> = lv
        .spaces()
        .iter()
        .filter(|s| !s.host.is_empty() && lv.space(&s.host).high.contains(&s.floor))
        .map(|p| (p.key.clone(), p.host.clone(), p.floor, false))
        .collect();
    for s in lv.spaces() {
        ends.extend(s.galleries.iter().map(|&z| (place(lv, &s.key, z), s.key.clone(), z, true)));
    }
    for (target, hall, storey, gallery) in &ends {
        let mut from: BTreeMap<&str, &str> = BTreeMap::from([(hall.as_str(), "")]);
        let mut todo = VecDeque::from([hall.as_str()]);
        while let Some(at) = todo.pop_front() {
            if at == target {
                let mut way = vec![at];
                while let Some(&back) = from.get(way[way.len() - 1]).filter(|b| !b.is_empty()) {
                    way.push(back);
                }
                way.reverse();
                let way = way.into_iter().map(|p| p.split('@').next().unwrap_or(p).to_string()).collect();
                out.push(Climb { pod: target.clone(), gallery: *gallery, hall: hall.clone(), storey: *storey, way });
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
                    assert!(!pods.is_empty() || s.galleries.contains(z), "seed {seed}: {}@{z} holds no pod and is no gallery", s.key);
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

    #[test]
    fn a_gallery_is_a_high_storey_of_catwalk_climbed_to_from_outside() {
        use crate::level::GALLERY_GAP;
        use crate::storeys::index;
        let (mut levels, mut galleries, mut through) = (0, 0, 0);
        for seed in 1..=8 {
            let mut o = Options::default();
            o.siting.scale = Scale::Towering;
            let plain = generate(seed, &o);
            o.galleries = 2;
            let lv = generate(seed, &o);
            assert_eq!(plain.is_ok(), lv.is_ok(), "seed {seed}: galleries change whether a level is made");
            let Ok(lv) = lv else { continue };
            levels += 1;
            let cs = climbs(&lv);
            for s in lv.spaces().iter().filter(|s| !s.galleries.is_empty()) {
                assert!(s.towers() && s.galleries.len() <= 2);
                let it = lv.interior(&s.key);
                for &z in &s.galleries {
                    galleries += 1;
                    assert!(s.high.contains(&z), "seed {seed}: {}@{z} is a gallery and no high storey", s.key);
                    assert!(lv.spaces().iter().all(|p| !(p.host == s.key && p.floor == z)), "seed {seed}: {}@{z} holds a pod", s.key);
                    for &h in s.high.iter().filter(|&&h| h != z) {
                        assert!(index(h).abs_diff(index(z)) >= GALLERY_GAP, "seed {seed}: {}@{z} is next to {h}", s.key);
                    }
                    let doors = lv.net.doors(&s.key).iter().filter(|d| d.level == z).count();
                    assert!(doors >= 1 && !it.deck[&z].is_empty(), "seed {seed}: {}@{z}: {doors} doors and no catwalk", s.key);
                    through += usize::from(doors >= 2);
                    let c = cs.iter().find(|c| c.gallery && c.hall == s.key && c.storey == z);
                    let c = c.unwrap_or_else(|| panic!("seed {seed}: no way up to {}@{z}", s.key));
                    assert!(c.way.len() >= 3 && c.way[0] == s.key && c.way[c.way.len() - 1] == s.key);
                }
            }
        }
        assert!(levels >= 6 && galleries >= 12 && through * 2 >= galleries, "{levels} levels, {galleries} galleries, {through} with two doors or more");
        // none unless asked for
        let mut o = Options::default();
        o.siting.scale = Scale::Towering;
        assert!(generate(9, &o).unwrap().spaces().iter().all(|s| s.galleries.is_empty()));
    }
}
