//! The openings between a level's rooms: every doorway, vent mouth, window, balcony, drop and
//! grate, as a rectangle and the room on either side of it.
//!
//! A plan's rooms are boxes of air, and air in different rooms joins with no wall when it is
//! built, so every face two rooms' boxes share is an opening and there are no others
//! (`Plan::contacts`; `Plan::check` holds them to the links declared). That is exactly what a
//! game that draws by rooms and portals needs: a portal for each. Here the contacts are given
//! their sides (which room is below the face along its axis, which above) and the pieces of one
//! opening are put back together: a hall's air is several boxes where a pod's shell is cut out
//! of it, and a doorway two of them share a wall with comes as two contacts.

use autolevel::plan::{Box3, Plan, Point3, Room};

/// An opening between two rooms: a rectangle square to `axis`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    /// The room whose air is under the rectangle along `axis` (the lower coordinates).
    pub low: String,
    /// The room on the other side.
    pub high: String,
    /// The axis the rectangle is square to: 0 x, 1 y, 2 z (the plan's: x east, y north, z up).
    pub axis: usize,
    /// Its low and high corners, in units (equal along `axis`).
    pub lo: Point3,
    pub hi: Point3,
}

fn at(p: Point3, i: usize) -> i32 {
    [p.0, p.1, p.2][i]
}

fn with(p: Point3, i: usize, v: i32) -> Point3 {
    let mut a = [p.0, p.1, p.2];
    a[i] = v;
    (a[0], a[1], a[2])
}

impl Opening {
    /// The rectangle's corners, in order round it: counter-clockwise seen from the high side
    /// (from `high`, looking back along the axis into `low`).
    pub fn corners(&self) -> [Point3; 4] {
        let (u, v) = ((self.axis + 1) % 3, (self.axis + 2) % 3);
        let c = |pu: Point3, pv: Point3| with(with(self.lo, u, at(pu, u)), v, at(pv, v));
        [c(self.lo, self.lo), c(self.hi, self.lo), c(self.hi, self.hi), c(self.lo, self.hi)]
    }

    /// The rectangle's two sides.
    pub fn size(&self) -> (i32, i32) {
        let (u, v) = ((self.axis + 1) % 3, (self.axis + 2) % 3);
        (at(self.hi, u) - at(self.lo, u), at(self.hi, v) - at(self.lo, v))
    }
}

/// Does `room` have air against the rectangle on this side of it (`below`: the lower coordinates)?
fn against(room: &Room, axis: usize, lo: Point3, hi: Point3, below: bool) -> bool {
    let face = at(lo, axis);
    room.boxes.iter().any(|b: &Box3| {
        let edge = if below { at(b.1, axis) } else { at(b.0, axis) };
        edge == face && (0..3).filter(|&i| i != axis).all(|i| at(b.0, i) < at(hi, i) && at(b.1, i) > at(lo, i))
    })
}

/// Every opening of the plan, each once, in the order `Plan::contacts` first meets them. Two
/// rectangles in one plane between the same rooms that share a whole side are one opening.
pub fn openings(plan: &Plan) -> Vec<Opening> {
    let (contacts, _) = plan.contacts();
    let mut out: Vec<Opening> = Vec::new();
    for c in &contacts {
        let (Some(a), Some(b)) = (plan.get(&c.a), plan.get(&c.b)) else { continue };
        // which of the two is under the face. (Air on both sides of both would be two rooms
        // wrapped round each other at one rectangle, which boxes that do not overlap cannot do.)
        let (low, high) = if against(a, c.axis, c.lo, c.hi, true) && against(b, c.axis, c.lo, c.hi, false) { (a, b) } else { (b, a) };
        out.push(Opening { low: low.key.clone(), high: high.key.clone(), axis: c.axis, lo: c.lo, hi: c.hi });
    }
    // put the pieces of one opening together, until no two fit
    loop {
        let mut joined = None;
        'find: for i in 0..out.len() {
            for j in i + 1..out.len() {
                if let Some(o) = join(&out[i], &out[j]) {
                    joined = Some((i, j, o));
                    break 'find;
                }
            }
        }
        match joined {
            Some((i, j, o)) => {
                out[i] = o;
                out.remove(j);
            }
            None => return out,
        }
    }
}

/// The two as one rectangle, if they are in one plane between the same rooms and share a whole side.
fn join(p: &Opening, q: &Opening) -> Option<Opening> {
    if p.axis != q.axis || p.low != q.low || p.high != q.high || at(p.lo, p.axis) != at(q.lo, q.axis) {
        return None;
    }
    let (u, v) = ((p.axis + 1) % 3, (p.axis + 2) % 3);
    for (along, across) in [(u, v), (v, u)] {
        let same = at(p.lo, across) == at(q.lo, across) && at(p.hi, across) == at(q.hi, across);
        let meet = at(p.hi, along) == at(q.lo, along) || at(q.hi, along) == at(p.lo, along);
        if same && meet {
            let lo = with(p.lo, along, at(p.lo, along).min(at(q.lo, along)));
            let hi = with(p.hi, along, at(p.hi, along).max(at(q.hi, along)));
            return Some(Opening { lo, hi, ..p.clone() });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::level::{generate, Options};
    use autolevel::plan::box3;

    fn plan(rooms: &[(&str, Vec<Box3>)]) -> Plan {
        let mut p = Plan::new("test");
        for (key, boxes) in rooms {
            p.room(Room::new(key, key, 0, boxes.clone())).unwrap();
        }
        p
    }

    #[test]
    fn a_doorway_has_its_rooms_on_their_sides() {
        // a room, and a corridor off its east wall and another off its south wall
        let p = plan(&[
            ("room", vec![box3(0.0, 0.0, 4.0, 4.0, 0, 236)]),
            ("east", vec![box3(4.0, 1.0, 8.0, 2.0, 0, 236)]),
            ("south", vec![box3(1.0, -3.0, 2.0, 0.0, 0, 236)]),
        ]);
        let o = openings(&p);
        assert_eq!(o.len(), 2);
        assert_eq!((o[0].low.as_str(), o[0].high.as_str(), o[0].axis, o[0].size()), ("room", "east", 0, (118, 236)));
        assert_eq!((o[1].low.as_str(), o[1].high.as_str(), o[1].axis, o[1].size()), ("south", "room", 1, (236, 118)));
        // the corners go round the rectangle, counter-clockwise from the high side
        let c = o[0].corners();
        assert_eq!(c, [(472, 118, 0), (472, 236, 0), (472, 236, 236), (472, 118, 236)]);
    }

    #[test]
    fn the_pieces_of_one_opening_are_joined() {
        // a hall in two boxes side by side, and a room on top whose floor is open over both
        let p = plan(&[("hall", vec![box3(0.0, 0.0, 2.0, 4.0, 0, 236), box3(2.0, 0.0, 4.0, 4.0, 0, 236)]), ("over", vec![box3(1.0, 1.0, 3.0, 2.0, 236, 472)])]);
        let o = openings(&p);
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!((o[0].low.as_str(), o[0].high.as_str(), o[0].axis, o[0].lo, o[0].hi), ("hall", "over", 2, (118, 118, 236), (354, 236, 236)));
        // two doorways in one wall, apart, stay two
        let p = plan(&[("a", vec![box3(0.0, 0.0, 4.0, 4.0, 0, 236)]), ("b", vec![box3(4.0, 0.0, 6.0, 1.0, 0, 236), box3(4.0, 3.0, 6.0, 4.0, 0, 236)])]);
        assert_eq!(openings(&p).len(), 2);
    }

    #[test]
    fn a_levels_openings_cover_its_links() {
        let lv = generate(40, &Options::default()).unwrap();
        let o = openings(&lv.plan);
        // every pair of rooms the plan links has an opening, and every opening is a link
        let pair = |a: &str, b: &str| if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) };
        for l in &lv.plan.links {
            assert!(o.iter().any(|o| pair(&o.low, &o.high) == pair(&l.a, &l.b)), "no opening for the link {} - {}", l.a, l.b);
        }
        for q in &o {
            assert!(lv.plan.links.iter().any(|l| pair(&l.a, &l.b) == pair(&q.low, &q.high)), "an opening no link declares: {q:?}");
            assert!(q.size().0 > 0 && q.size().1 > 0 && at(q.lo, q.axis) == at(q.hi, q.axis), "{q:?}");
        }
        // no two left that would join, and none twice
        for (i, p) in o.iter().enumerate() {
            for q in &o[i + 1..] {
                assert!(join(p, q).is_none() && p != q);
            }
        }
    }
}
