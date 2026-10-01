//! The document: a level's layout being edited ([`pd_import::layout`]), its
//! undo history, and the edits the panel and the view make. Every kind is
//! present once a level is open (the stage's placement is adopted), so what
//! the user sees is exactly what is saved.

use glam::Vec3;
use pd_import::layout::{Ammo, Door, Layout, Motion, Side, Spot, TeamSpot, Weapon};

/// A kind of item the layout holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Spawn,
    Weapon,
    Ammo,
    Hill,
    Base,
    Respawn,
    Cover,
    /// A door ([`pd_import::layout::Door`]): its position is its bottom edge's
    /// middle, on the floor (the others' are pads, `PAD_HEIGHT` over it).
    Door,
}

impl Kind {
    pub const ALL: [Kind; 8] = [Kind::Spawn, Kind::Weapon, Kind::Ammo, Kind::Hill, Kind::Base, Kind::Respawn, Kind::Cover, Kind::Door];
    /// The Items tab's kinds (doors have their own).
    pub const ITEMS: [Kind; 7] = [Kind::Spawn, Kind::Weapon, Kind::Ammo, Kind::Hill, Kind::Base, Kind::Respawn, Kind::Cover];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Spawn => "Spawn",
            Kind::Weapon => "Weapon",
            Kind::Ammo => "Ammo crate",
            Kind::Hill => "Hill",
            Kind::Base => "CTC base",
            Kind::Respawn => "CTC respawn",
            Kind::Cover => "Cover",
            Kind::Door => "Door",
        }
    }

    /// Whether it has a facing worth turning.
    pub fn faces(self) -> bool {
        matches!(self, Kind::Spawn | Kind::Base | Kind::Respawn | Kind::Cover | Kind::Door)
    }

    /// How high over its floor its position is.
    pub fn height(self) -> f32 {
        if self == Kind::Door {
            0.0
        } else {
            PAD_HEIGHT
        }
    }

    /// Whether it belongs to a Capture the Case team.
    pub fn team(self) -> bool {
        matches!(self, Kind::Base | Kind::Respawn)
    }
}

/// One item: its kind and index in its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Item {
    pub kind: Kind,
    pub index: usize,
}

/// What's selected: an item of the layout, or a waypoint of the graph (by
/// its index in the scene's graph).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sel {
    Item(Item),
    Node(usize),
}

/// The pad height PD's items stand at over their floor (`place.rs`).
pub const PAD_HEIGHT: f32 = 53.0;

/// A layout and its history.
pub struct Doc {
    pub layout: Layout,
    undo: Vec<Layout>,
    redo: Vec<Layout>,
    /// The layout as last saved.
    saved: Layout,
    pub selected: Option<Item>,
}

fn v(p: [f32; 3]) -> Vec3 {
    Vec3::from(p)
}

fn a(p: Vec3) -> [f32; 3] {
    [p.x, p.y, p.z].map(pd_import::layout::round1)
}

impl Doc {
    /// Editing `layout`; `saved` says whether it is what the file holds.
    pub fn new(layout: Layout, saved: bool) -> Doc {
        let mut l = layout;
        // Every kind present: an absent one would be generated anew on each
        // placement, and could never be edited.
        l.spawns.get_or_insert_with(Vec::new);
        l.weapons.get_or_insert_with(Vec::new);
        l.ammo.get_or_insert_with(Vec::new);
        l.hills.get_or_insert_with(Vec::new);
        l.bases.get_or_insert_with(Vec::new);
        l.respawns.get_or_insert_with(Vec::new);
        l.cover.get_or_insert_with(Vec::new);
        let saved_as = if saved { l.clone() } else { Layout::new() };
        Doc { layout: l, undo: Vec::new(), redo: Vec::new(), saved: saved_as, selected: None }
    }

    pub fn dirty(&self) -> bool {
        self.layout != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.layout.clone();
    }

    /// Before an edit: remember the layout for undo.
    fn push(&mut self) {
        self.undo.push(self.layout.clone());
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Start a drag: one undo step for the whole move.
    pub fn begin_drag(&mut self) {
        self.push();
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(l) => {
                self.redo.push(std::mem::replace(&mut self.layout, l));
                self.fix_selection();
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(l) => {
                self.undo.push(std::mem::replace(&mut self.layout, l));
                self.fix_selection();
                true
            }
            None => false,
        }
    }

    fn fix_selection(&mut self) {
        if self.selected.is_some_and(|s| s.index >= self.count(s.kind)) {
            self.selected = None;
        }
    }

    pub fn count(&self, kind: Kind) -> usize {
        let l = &self.layout;
        match kind {
            Kind::Spawn => l.spawns.as_ref().map_or(0, Vec::len),
            Kind::Weapon => l.weapons.as_ref().map_or(0, Vec::len),
            Kind::Ammo => l.ammo.as_ref().map_or(0, Vec::len),
            Kind::Hill => l.hills.as_ref().map_or(0, Vec::len),
            Kind::Base => l.bases.as_ref().map_or(0, Vec::len),
            Kind::Respawn => l.respawns.as_ref().map_or(0, Vec::len),
            Kind::Cover => l.cover.as_ref().map_or(0, Vec::len),
            Kind::Door => l.doors.as_ref().map_or(0, Vec::len),
        }
    }

    /// Every item, kind by kind.
    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        Kind::ALL.into_iter().flat_map(move |kind| (0..self.count(kind)).map(move |index| Item { kind, index }))
    }

    /// An item's pad position.
    pub fn pos(&self, it: Item) -> Vec3 {
        let l = &self.layout;
        let i = it.index;
        match it.kind {
            Kind::Spawn => v(l.spawns.as_ref().unwrap()[i].pos),
            Kind::Weapon => v(l.weapons.as_ref().unwrap()[i].pos),
            Kind::Ammo => v(l.ammo.as_ref().unwrap()[i].pos),
            Kind::Hill => v(l.hills.as_ref().unwrap()[i].pos),
            Kind::Base => v(l.bases.as_ref().unwrap()[i].pos),
            Kind::Respawn => v(l.respawns.as_ref().unwrap()[i].pos),
            Kind::Cover => v(l.cover.as_ref().unwrap()[i].pos),
            Kind::Door => v(l.doors.as_ref().unwrap()[i].pos),
        }
    }

    /// Its facing in degrees, if it has one.
    pub fn facing(&self, it: Item) -> Option<f32> {
        let l = &self.layout;
        let i = it.index;
        match it.kind {
            Kind::Spawn => Some(l.spawns.as_ref().unwrap()[i].facing),
            Kind::Base => Some(l.bases.as_ref().unwrap()[i].facing),
            Kind::Respawn => Some(l.respawns.as_ref().unwrap()[i].facing),
            Kind::Cover => Some(l.cover.as_ref().unwrap()[i].facing),
            Kind::Door => Some(l.doors.as_ref().unwrap()[i].facing),
            _ => None,
        }
    }

    pub fn team(&self, it: Item) -> Option<u8> {
        let l = &self.layout;
        match it.kind {
            Kind::Base => Some(l.bases.as_ref().unwrap()[it.index].team),
            Kind::Respawn => Some(l.respawns.as_ref().unwrap()[it.index].team),
            _ => None,
        }
    }

    /// A weapon's MP location.
    pub fn location(&self, it: Item) -> Option<u8> {
        (it.kind == Kind::Weapon).then(|| self.layout.weapons.as_ref().unwrap()[it.index].location)
    }

    /// An ammo crate's weapon.
    pub fn crate_weapon(&self, it: Item) -> Option<usize> {
        (it.kind == Kind::Ammo).then(|| self.layout.ammo.as_ref().unwrap()[it.index].weapon)
    }

    /// Move an item (no undo step: see [`Doc::begin_drag`]). A door takes the
    /// rest of its sibling ring with it.
    pub fn set_pos(&mut self, it: Item, p: Vec3) {
        if it.kind == Kind::Door {
            let delta = p - self.pos(it);
            for j in self.ring(it.index) {
                let d = &mut self.layout.doors.as_mut().unwrap()[j];
                d.pos = a(v(d.pos) + delta);
            }
            return;
        }
        let l = &mut self.layout;
        let i = it.index;
        let p = a(p);
        match it.kind {
            Kind::Spawn => l.spawns.as_mut().unwrap()[i].pos = p,
            Kind::Weapon => l.weapons.as_mut().unwrap()[i].pos = p,
            Kind::Ammo => l.ammo.as_mut().unwrap()[i].pos = p,
            Kind::Hill => l.hills.as_mut().unwrap()[i].pos = p,
            Kind::Base => l.bases.as_mut().unwrap()[i].pos = p,
            Kind::Respawn => l.respawns.as_mut().unwrap()[i].pos = p,
            Kind::Cover => l.cover.as_mut().unwrap()[i].pos = p,
            Kind::Door => {}
        }
    }

    /// Turn an item to `deg` (0..360), as one undo step.
    pub fn set_facing(&mut self, it: Item, deg: f32) {
        self.push();
        self.set_facing_live(it, deg);
    }

    /// Turn an item without an undo step (a drag: see [`Doc::begin_drag`]). A
    /// door turns its sibling ring with it, about itself.
    pub fn set_facing_live(&mut self, it: Item, deg: f32) {
        let f = pd_import::layout::round1(deg.rem_euclid(360.0)) % 360.0;
        if it.kind == Kind::Door {
            let (about, was) = (self.pos(it), self.facing(it).unwrap_or(0.0));
            let turn = glam::Quat::from_rotation_y((f - was).to_radians());
            for j in self.ring(it.index) {
                let d = &mut self.layout.doors.as_mut().unwrap()[j];
                d.pos = a(about + turn * (v(d.pos) - about));
                d.facing = pd_import::layout::round1((d.facing + f - was).rem_euclid(360.0)) % 360.0;
            }
            return;
        }
        let l = &mut self.layout;
        match it.kind {
            Kind::Spawn => l.spawns.as_mut().unwrap()[it.index].facing = f,
            Kind::Base => l.bases.as_mut().unwrap()[it.index].facing = f,
            Kind::Respawn => l.respawns.as_mut().unwrap()[it.index].facing = f,
            Kind::Cover => l.cover.as_mut().unwrap()[it.index].facing = f,
            _ => {}
        }
    }

    pub fn set_team(&mut self, it: Item, team: u8) {
        self.push();
        let l = &mut self.layout;
        match it.kind {
            Kind::Base => l.bases.as_mut().unwrap()[it.index].team = team,
            Kind::Respawn => l.respawns.as_mut().unwrap()[it.index].team = team,
            _ => {}
        }
    }

    pub fn set_location(&mut self, it: Item, location: u8) {
        if it.kind == Kind::Weapon {
            self.push();
            self.layout.weapons.as_mut().unwrap()[it.index].location = location.min(15);
        }
    }

    /// Tie a crate to another weapon.
    pub fn set_crate_weapon(&mut self, it: Item, weapon: usize) {
        if it.kind == Kind::Ammo && weapon < self.count(Kind::Weapon) {
            self.push();
            self.layout.ammo.as_mut().unwrap()[it.index].weapon = weapon;
        }
    }

    /// The weapon nearest `p` (a new crate's).
    pub fn nearest_weapon(&self, p: Vec3) -> Option<usize> {
        let w = self.layout.weapons.as_ref()?;
        (0..w.len()).min_by(|&x, &y| v(w[x].pos).distance(p).total_cmp(&v(w[y].pos).distance(p)))
    }

    /// The lowest MP location no weapon has yet (else the least used).
    pub fn free_location(&self) -> u8 {
        let w = self.layout.weapons.as_deref().unwrap_or(&[]);
        (0..16u8).min_by_key(|&loc| (w.iter().filter(|x| x.location == loc).count(), loc)).unwrap_or(0)
    }

    /// Add an item of `kind` at pad position `p`: a weapon on the next free
    /// location, a crate for `weapon` (else the nearest weapon), a team item
    /// on `team`. Returns it (none: a crate with no weapon to belong to).
    pub fn add(&mut self, kind: Kind, p: Vec3, facing: f32, team: u8, weapon: Option<usize>) -> Option<Item> {
        if kind == Kind::Door {
            return None; // a door has a model and a row: `add_door`.
        }
        let crate_weapon = if kind == Kind::Ammo { Some(weapon.filter(|&w| w < self.count(Kind::Weapon)).or_else(|| self.nearest_weapon(p))?) } else { None };
        let location = self.free_location();
        self.push();
        let pos = a(p);
        let l = &mut self.layout;
        let index = match kind {
            Kind::Spawn => push(l.spawns.as_mut().unwrap(), Spot { pos, facing }),
            Kind::Weapon => push(l.weapons.as_mut().unwrap(), Weapon { pos, location }),
            Kind::Ammo => push(l.ammo.as_mut().unwrap(), Ammo { pos, weapon: crate_weapon.unwrap() }),
            Kind::Hill => push(l.hills.as_mut().unwrap(), Spot { pos, facing: 0.0 }),
            Kind::Base => push(l.bases.as_mut().unwrap(), TeamSpot { pos, facing, team }),
            Kind::Respawn => push(l.respawns.as_mut().unwrap(), TeamSpot { pos, facing, team }),
            Kind::Cover => push(l.cover.as_mut().unwrap(), Spot { pos, facing }),
            Kind::Door => unreachable!(),
        };
        let it = Item { kind, index };
        self.selected = Some(it);
        Some(it)
    }

    /// Delete an item. A weapon takes its crates with it (a crate holds its
    /// weapon's ammo, and has none without one); later crates' weapons are
    /// renumbered.
    pub fn delete(&mut self, it: Item) {
        if it.index >= self.count(it.kind) {
            return;
        }
        self.push();
        let l = &mut self.layout;
        let i = it.index;
        match it.kind {
            Kind::Spawn => drop(l.spawns.as_mut().unwrap().remove(i)),
            Kind::Weapon => {
                l.weapons.as_mut().unwrap().remove(i);
                let ammo = l.ammo.as_mut().unwrap();
                ammo.retain(|c| c.weapon != i);
                for c in ammo.iter_mut().filter(|c| c.weapon > i) {
                    c.weapon -= 1;
                }
            }
            Kind::Ammo => drop(l.ammo.as_mut().unwrap().remove(i)),
            Kind::Hill => drop(l.hills.as_mut().unwrap().remove(i)),
            Kind::Base => drop(l.bases.as_mut().unwrap().remove(i)),
            Kind::Respawn => drop(l.respawns.as_mut().unwrap().remove(i)),
            Kind::Cover => drop(l.cover.as_mut().unwrap().remove(i)),
            Kind::Door => {
                let doors = l.doors.as_mut().unwrap();
                doors.remove(i);
                // A ring broken open is closed again past it.
                for d in doors.iter_mut() {
                    d.sibling = match d.sibling {
                        Some(s) if s == i => None,
                        Some(s) if s > i => Some(s - 1),
                        s => s,
                    };
                }
                fix_rings(doors);
            }
        }
        self.selected = None;
    }
}

/// A sibling chain left open (a door deleted from a ring of three or more)
/// closed again, so every ring still comes round; a door its own sibling
/// has none.
fn fix_rings(doors: &mut [Door]) {
    for start in 0..doors.len() {
        if doors.iter().any(|d| d.sibling == Some(start)) || doors[start].sibling.is_none() {
            continue;
        }
        // `start` heads a chain: point its end back at it.
        let mut cur = start;
        let mut seen = vec![start];
        while let Some(n) = doors[cur].sibling.filter(|n| !seen.contains(n)) {
            seen.push(n);
            cur = n;
        }
        if cur != start && doors[cur].sibling.is_none() {
            doors[cur].sibling = Some(start);
        }
    }
    for (i, d) in doors.iter_mut().enumerate() {
        if d.sibling == Some(i) {
            d.sibling = None;
        }
    }
}

/// Doors.
impl Doc {
    pub fn door(&self, i: usize) -> &Door {
        &self.layout.doors.as_ref().unwrap()[i]
    }

    /// Door `i`'s sibling ring, `i` first.
    pub fn ring(&self, i: usize) -> Vec<usize> {
        let doors = self.layout.doors.as_deref().unwrap_or(&[]);
        let mut out = vec![i];
        let mut cur = doors.get(i).and_then(|d| d.sibling);
        while let Some(c) = cur.filter(|c| !out.contains(c) && *c < doors.len()) {
            out.push(c);
            cur = doors[c].sibling;
        }
        out
    }

    /// The stage's own doors, when the layout file has none (a GoldenEye
    /// level's, before its layout names them): taken as if saved, since
    /// placement keeps the same doors without a layout's.
    pub fn adopt_doors(&mut self, doors: Vec<Door>) {
        if self.layout.doors.is_none() {
            self.layout.doors = Some(doors.clone());
            if self.saved.doors.is_none() {
                self.saved.doors = Some(doors);
            }
        }
    }

    /// Add `door`; returns it.
    pub fn add_door(&mut self, door: Door) -> Item {
        self.push();
        let doors = self.layout.doors.get_or_insert_with(Vec::new);
        doors.push(door);
        let it = Item { kind: Kind::Door, index: doors.len() - 1 };
        self.selected = Some(it);
        it
    }

    /// Change door `i` as one undo step.
    pub fn edit_door(&mut self, i: usize, f: impl FnOnce(&mut Door)) {
        self.push();
        self.edit_door_live(i, f);
    }

    /// Change door `i` without an undo step (a drag: see [`Doc::begin_drag`]).
    pub fn edit_door_live(&mut self, i: usize, f: impl FnOnce(&mut Door)) {
        if let Some(d) = self.layout.doors.as_mut().and_then(|d| d.get_mut(i)) {
            f(d);
            d.pos = a(v(d.pos));
            d.size = d.size.map(|x| pd_import::layout::round1(x.max(1.0)));
        }
    }

    /// Split door `i` into a double door: two leaves of half its width
    /// (half its height for one that rises or sinks: a top leaf rising and a
    /// bottom one sinking, as Area 51's), hinged or sliding at the outer
    /// edges, opening together. Returns the new leaf.
    pub fn make_double(&mut self, i: usize) -> Option<usize> {
        if self.layout.doors.as_ref().is_none_or(|d| i >= d.len() || d[i].sibling.is_some()) {
            return None;
        }
        self.push();
        let doors = self.layout.doors.as_mut().unwrap();
        let mut left = doors[i].clone();
        let mut right = left.clone();
        let across = Vec3::Y.cross(left.front());
        if matches!(left.motion, Motion::Up | Motion::Down) {
            let h = left.size[1] * 0.5;
            left.size[1] = h;
            right.size[1] = h;
            left.motion = Motion::Down;
            right.motion = Motion::Up;
            right.pos = a(v(right.pos) + Vec3::Y * h);
        } else {
            let w = left.size[0] * 0.5;
            left.size[0] = w;
            right.size[0] = w;
            left.pos = a(v(left.pos) - across * (w * 0.5));
            right.pos = a(v(right.pos) + across * (w * 0.5));
            left.side = Side::Left;
            right.side = Side::Right;
        }
        let j = doors.len();
        left.sibling = Some(j);
        right.sibling = Some(i);
        doors[i] = left;
        doors.push(right);
        Some(j)
    }

    /// Door `i`'s ring taken apart: each door opens alone.
    pub fn separate(&mut self, i: usize) {
        let ring = self.ring(i);
        if ring.len() < 2 {
            return;
        }
        self.push();
        let doors = self.layout.doors.as_mut().unwrap();
        for j in ring {
            doors[j].sibling = None;
        }
    }
}

/// Two waypoint positions the edits treat as the same (cm).
const SAME: f32 = 5.0;

fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    v(a).distance(v(b)) < SAME
}

/// Waypoint edits, kept in the layout (`pd_import::waypoints` replays them on
/// the generated graph). Each keeps the edit list short: a waypoint moved
/// twice is one move, one added then moved is added where it ends, and so on.
impl Doc {
    fn wp(&mut self) -> &mut pd_import::layout::WaypointEdits {
        self.layout.waypoints.get_or_insert_with(Default::default)
    }

    /// Every link edit's end at `from` now at `to`.
    fn wp_rekey(&mut self, from: [f32; 3], to: [f32; 3]) {
        let e = self.wp();
        for l in e.linked.iter_mut().chain(e.unlinked.iter_mut()) {
            if same(l.a, from) {
                l.a = to;
            }
            if same(l.b, from) {
                l.b = to;
            }
        }
    }

    /// Move the waypoint at `from` to `to`.
    pub fn wp_move(&mut self, from: Vec3, to: Vec3) {
        let (from, to) = (a(from), a(to));
        if same(from, to) {
            return;
        }
        self.push();
        self.wp_rekey(from, to);
        let e = self.wp();
        if let Some(p) = e.added.iter_mut().find(|p| same(**p, from)) {
            *p = to;
        } else if let Some(i) = e.moved.iter().position(|m| same(m.to, from)) {
            e.moved[i].to = to;
            if same(e.moved[i].from, to) {
                e.moved.remove(i);
            }
        } else {
            e.moved.push(pd_import::layout::Moved { from, to });
        }
    }

    /// Add a waypoint at `p`.
    pub fn wp_add(&mut self, p: Vec3) {
        self.push();
        let p = a(p);
        self.wp().added.push(p);
    }

    /// Delete the waypoint at `p` (and the link edits touching it).
    pub fn wp_remove(&mut self, p: Vec3) {
        self.push();
        let p = a(p);
        let e = self.wp();
        e.linked.retain(|l| !same(l.a, p) && !same(l.b, p));
        e.unlinked.retain(|l| !same(l.a, p) && !same(l.b, p));
        if let Some(i) = e.added.iter().position(|q| same(*q, p)) {
            e.added.remove(i);
        } else if let Some(i) = e.moved.iter().position(|m| same(m.to, p)) {
            let from = e.moved.remove(i).from;
            e.removed.push(from);
        } else {
            e.removed.push(p);
        }
    }

    /// Link the waypoints at `x` and `y` (only x → y if `one_way`).
    pub fn wp_link(&mut self, x: Vec3, y: Vec3, one_way: bool) {
        self.push();
        let (x, y) = (a(x), a(y));
        let e = self.wp();
        let pair = |l: &pd_import::layout::Link| (same(l.a, x) && same(l.b, y)) || (same(l.a, y) && same(l.b, x));
        e.unlinked.retain(|l| !pair(l));
        e.linked.retain(|l| !pair(l));
        e.linked.push(pd_import::layout::Link { a: x, b: y, one_way });
    }

    /// Cut every link between the waypoints at `x` and `y`.
    pub fn wp_unlink(&mut self, x: Vec3, y: Vec3) {
        self.push();
        let (x, y) = (a(x), a(y));
        let e = self.wp();
        let pair = |l: &pd_import::layout::Link| (same(l.a, x) && same(l.b, y)) || (same(l.a, y) && same(l.b, x));
        e.linked.retain(|l| !pair(l));
        e.unlinked.retain(|l| !pair(l));
        e.unlinked.push(pd_import::layout::Link { a: x, b: y, one_way: false });
    }

    /// Forget every waypoint edit: the generated graph as it is.
    pub fn wp_clear(&mut self) {
        if self.layout.waypoints.as_ref().is_some_and(|e| !e.is_empty()) {
            self.push();
            self.layout.waypoints = None;
        }
    }

    pub fn wp_edit_count(&self) -> usize {
        self.layout.waypoints.as_ref().map_or(0, |e| e.removed.len() + e.moved.len() + e.added.len() + e.linked.len() + e.unlinked.len())
    }
}

fn push<T>(v: &mut Vec<T>, x: T) -> usize {
    v.push(x);
    v.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Doc {
        let mut l = Layout::new();
        l.weapons = Some(vec![Weapon { pos: [0.0, 53.0, 0.0], location: 0 }, Weapon { pos: [1000.0, 53.0, 0.0], location: 1 }]);
        l.ammo = Some(vec![Ammo { pos: [100.0, 53.0, 0.0], weapon: 0 }, Ammo { pos: [1100.0, 53.0, 0.0], weapon: 1 }, Ammo { pos: [1200.0, 53.0, 0.0], weapon: 1 }]);
        Doc::new(l, true)
    }

    #[test]
    fn crates_follow_their_weapon() {
        let mut d = doc();
        assert!(!d.dirty());
        // A crate goes to the nearest weapon; a new weapon takes a free location.
        let c = d.add(Kind::Ammo, Vec3::new(900.0, 53.0, 0.0), 0.0, 0, None).unwrap();
        assert_eq!(d.crate_weapon(c), Some(1));
        let w = d.add(Kind::Weapon, Vec3::new(0.0, 53.0, 900.0), 0.0, 0, None).unwrap();
        assert_eq!(d.location(w), Some(2));
        // Deleting weapon 0 takes its crate and renumbers the rest.
        d.delete(Item { kind: Kind::Weapon, index: 0 });
        let ammo = d.layout.ammo.clone().unwrap();
        assert_eq!(ammo.iter().map(|c| c.weapon).collect::<Vec<_>>(), vec![0, 0, 0]);
        assert!(d.layout.check().is_ok());
        assert!(d.dirty());
        // Undo it all, step by step.
        assert!(d.undo() && d.undo() && d.undo());
        assert!(!d.dirty() && !d.undo());
        assert!(d.redo());
        assert_eq!(d.count(Kind::Ammo), 4);
    }

    #[test]
    fn waypoint_edits_stay_short() {
        let mut d = Doc::new(Layout::new(), true);
        let (p, q, r) = (Vec3::new(0.0, 53.0, 0.0), Vec3::new(100.0, 53.0, 0.0), Vec3::new(200.0, 53.0, 0.0));
        // Moved twice: one move; moved back: none.
        d.wp_move(p, q);
        d.wp_move(q, r);
        assert_eq!(d.layout.waypoints.as_ref().unwrap().moved.len(), 1);
        d.wp_move(r, p);
        assert!(d.layout.waypoints.as_ref().unwrap().moved.is_empty());
        // Added then moved: added where it ends; a link to it follows it.
        let n = Vec3::new(0.0, 53.0, 500.0);
        d.wp_add(n);
        d.wp_link(p, n, false);
        d.wp_move(n, n + Vec3::X * 50.0);
        let e = d.layout.waypoints.clone().unwrap();
        assert_eq!(e.added, vec![[50.0, 53.0, 500.0]]);
        assert_eq!(e.linked[0].b, [50.0, 53.0, 500.0]);
        // Deleting it takes it and its link out of the edits altogether.
        d.wp_remove(n + Vec3::X * 50.0);
        let e = d.layout.waypoints.clone().unwrap();
        assert!(e.added.is_empty() && e.linked.is_empty() && e.removed.is_empty());
        // A generated one moved then deleted: removed where it was generated.
        d.wp_move(p, q);
        d.wp_remove(q);
        assert_eq!(d.layout.waypoints.clone().unwrap().removed, vec![[0.0, 53.0, 0.0]]);
        // Unlinking what was linked cancels out.
        d.wp_link(q, r, true);
        d.wp_unlink(r, q);
        let e = d.layout.waypoints.clone().unwrap();
        assert!(e.linked.is_empty() && e.unlinked.len() == 1);
    }

    fn a_door(x: f32) -> Door {
        Door {
            pos: [x, 0.0, 0.0],
            facing: 0.0,
            size: [200.0, 220.0, 10.0],
            model: "dd_officedoor".into(),
            motion: Motion::Slide,
            side: Side::Left,
            swing: pd_import::layout::Swing::Back,
            maxfrac: 62259,
            perimfrac: 65536,
            accel: 10922,
            decel: 10922,
            maxspeed: 218,
            autoclosetime: 900,
            soundtype: 3,
            doorflags: 0,
            flags: 0,
            flags2: 0,
            unk88: 0,
            sibling: None,
        }
    }

    /// A double door is two half-width leaves sliding apart, opening together;
    /// they move and turn as one; deleting a door keeps every ring whole.
    #[test]
    fn doors_move_with_their_ring() {
        let mut d = Doc::new(Layout::new(), true);
        d.adopt_doors(vec![a_door(0.0), a_door(1000.0)]);
        assert!(!d.dirty(), "the stage's own doors are what the file means");
        let j = d.make_double(0).unwrap();
        let (l, r) = (d.door(0).clone(), d.door(j).clone());
        assert_eq!((l.size[0], r.size[0], l.side, r.side, l.sibling, r.sibling), (100.0, 100.0, Side::Left, Side::Right, Some(j), Some(0)));
        // Facing +z, the left leaf is on -x (seen from the front).
        assert_eq!((l.pos[0], r.pos[0]), (-50.0, 50.0));
        d.begin_drag();
        d.set_pos(Item { kind: Kind::Door, index: j }, Vec3::new(50.0, 0.0, 100.0));
        assert_eq!((d.door(0).pos[2], d.door(j).pos[2]), (100.0, 100.0), "the pair moves together");
        d.set_facing(Item { kind: Kind::Door, index: 0 }, 90.0);
        assert_eq!((d.door(0).facing, d.door(j).facing), (90.0, 90.0));
        assert_eq!(d.door(j).pos, [-50.0, 0.0, 0.0], "turned about the leaf turned");
        // A ring of three loses its middle door: the other two still pair.
        let k = d.add_door(a_door(2000.0)).index;
        d.edit_door(j, |x| x.sibling = Some(k));
        d.edit_door(k, |x| x.sibling = Some(0));
        assert_eq!(d.ring(0), vec![0, j, k]);
        d.delete(Item { kind: Kind::Door, index: j });
        assert_eq!(d.ring(0), vec![0, k - 1]);
        assert!(d.layout.check().is_ok());
        d.separate(0);
        assert_eq!(d.ring(0), vec![0]);
    }

    #[test]
    fn a_crate_needs_a_weapon() {
        let mut d = Doc::new(Layout::new(), false);
        assert!(d.add(Kind::Ammo, Vec3::ZERO, 0.0, 0, None).is_none());
        assert_eq!(d.count(Kind::Ammo), 0);
        let s = d.add(Kind::Spawn, Vec3::new(1.23, 53.0, 4.56), 90.0, 0, None).unwrap();
        assert_eq!(d.pos(s), Vec3::new(1.2, 53.0, 4.6), "positions to a tenth");
        d.set_facing(s, -90.0);
        assert_eq!(d.facing(s), Some(270.0));
    }
}
