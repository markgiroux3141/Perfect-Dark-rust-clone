//! The generator's rooms in the 3D view: a click picks the room under the
//! pointer ([`pick_room`]), and the room selected gets a box gizmo
//! (docs/AUTOLEVEL.md, "Editing generated levels", item 3): six face handles
//! (a side pulled is a `Rect`, the top or the bottom a `Height`) and three
//! move arrows at its low corner (along the plan's x and y by cells, a
//! `Move`; up by storeys, a `Pin`).
//!
//! A drag is the side view's (`autolevel_edit::sideview::drag`) with the
//! pointer's ray read along the handle's line, so it snaps as the side view
//! does: whole cells across, the nearest storey for a floor, the nearest
//! ceiling line for a top, never under a storey tall. What it would come to is
//! tried with `boxes::trial` alone (a millisecond: nothing routed or
//! furnished) and drawn as a wireframe that follows the pointer, red with the
//! refusal; the change is made when it is let go (`space::Space::gizmo`).
//!
//! Units: the plan's cells (`preview::CELL` each way) and the generator's
//! heights, which are PD's world y; `sideview::UNITS` is what the side view
//! counts a height in, so a drag up is handed over in those.

use autolevel::boxes::Change;
use autolevel::level::Level;
use autolevel::recipe::Edit;
use autolevel::scatter::Lot;
use autolevel_edit::sideview::{self, Axis, Section, SidePart, UNITS};
use glam::Vec3;

use crate::draw3d::Mesh3d;
use crate::gizmo::{closest_t_on_axis, ray_aabb};
use crate::preview::{cell, world, Preview, RoomBox, CELL};

/// One of a box's faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// The plan's x0 side (world -x).
    West,
    /// x1 (world +x).
    East,
    /// y0 (world +z: the plan's y is the world's -z).
    South,
    /// y1 (world -z).
    North,
    Top,
    Bottom,
}

/// What of the gizmo a drag has hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Face(Face),
    /// A move arrow: along the plan's x (0), its y (1), or up (2).
    Arrow(usize),
}

impl Handle {
    pub const ALL: [Handle; 9] = [
        Handle::Face(Face::West),
        Handle::Face(Face::East),
        Handle::Face(Face::South),
        Handle::Face(Face::North),
        Handle::Face(Face::Top),
        Handle::Face(Face::Bottom),
        Handle::Arrow(0),
        Handle::Arrow(1),
        Handle::Arrow(2),
    ];

    /// Its name on the command line (`--drag top:1`).
    pub fn name(&self) -> &'static str {
        match self {
            Handle::Face(Face::West) => "west",
            Handle::Face(Face::East) => "east",
            Handle::Face(Face::South) => "south",
            Handle::Face(Face::North) => "north",
            Handle::Face(Face::Top) => "top",
            Handle::Face(Face::Bottom) => "bottom",
            Handle::Arrow(0) => "x",
            Handle::Arrow(1) => "y",
            Handle::Arrow(_) => "z",
        }
    }

    pub fn named(name: &str) -> Option<Handle> {
        Handle::ALL.into_iter().find(|h| h.name() == name)
    }

    /// The way a drag of it goes, in the world.
    pub fn dir(&self) -> Vec3 {
        match self {
            Handle::Face(Face::West | Face::East) | Handle::Arrow(0) => Vec3::X,
            Handle::Face(Face::South | Face::North) | Handle::Arrow(1) => Vec3::NEG_Z,
            _ => Vec3::Y,
        }
    }

    /// Does it go up and down (its steps storeys, not cells)?
    pub fn vertical(&self) -> bool {
        self.dir() == Vec3::Y
    }

    /// What a drag of it does to `key`, in words.
    pub fn does(&self, key: &str) -> String {
        match self {
            Handle::Face(Face::Top) => format!("drag: {key}'s top up or down a ceiling line"),
            Handle::Face(Face::Bottom) => format!("drag: {key}'s floor up or down a storey, its top kept"),
            Handle::Face(f) => format!("drag: pull {key}'s {} side", format!("{f:?}").to_lowercase()),
            Handle::Arrow(2) => format!("drag: {key} up or down a storey"),
            Handle::Arrow(k) => format!("drag: move {key} along {}", if *k == 0 { "x" } else { "y" }),
        }
    }

    /// The colour it is drawn in: x red, y green, up blue (as the item gizmo's).
    fn colour(&self) -> [f32; 4] {
        match self.dir() {
            d if d == Vec3::X => [0.93, 0.25, 0.22, 1.0],
            d if d == Vec3::NEG_Z => [0.28, 0.88, 0.3, 1.0],
            _ => [0.3, 0.48, 1.0, 1.0],
        }
    }
}

/// A lot's box in the world (low and high corners).
pub fn world_box(lot: &Lot) -> (Vec3, Vec3) {
    corners((lot.x0, lot.y0, lot.x1, lot.y1), lot.floor, lot.top)
}

/// A room of the preview's box in the world.
pub fn room_box(r: &RoomBox) -> (Vec3, Vec3) {
    corners(r.rect, r.floor, r.top)
}

fn corners((x0, y0, x1, y1): (i32, i32, i32, i32), floor: i32, top: i32) -> (Vec3, Vec3) {
    let a = world((x0 as f32, y0 as f32), floor as f32);
    let b = world((x1 as f32, y1 as f32), top as f32);
    (a.min(b), a.max(b))
}

/// The move arrows' length for the box `lo..hi` seen from `eye`: as the
/// item gizmo's, by the distance to where they start (so the same on
/// screen), never more than the box is across.
pub fn size(eye: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    let across = (hi.x - lo.x).min(hi.z - lo.z).max(CELL);
    let corner = Vec3::new(lo.x, lo.y, hi.z);
    crate::gizmo::size(eye, corner).min(across * 0.9)
}

/// Where the move arrows start: just outside the box's south-west corner,
/// on its floor.
pub fn origin(lo: Vec3, hi: Vec3, s: f32) -> Vec3 {
    Vec3::new(lo.x - s * 0.15, lo.y, hi.z + s * 0.15)
}

/// Where a handle is: a face's middle, or an arrow's start.
pub fn spot(h: Handle, lo: Vec3, hi: Vec3, s: f32) -> Vec3 {
    let c = (lo + hi) * 0.5;
    match h {
        Handle::Face(Face::West) => Vec3::new(lo.x, c.y, c.z),
        Handle::Face(Face::East) => Vec3::new(hi.x, c.y, c.z),
        Handle::Face(Face::South) => Vec3::new(c.x, c.y, hi.z),
        Handle::Face(Face::North) => Vec3::new(c.x, c.y, lo.z),
        Handle::Face(Face::Top) => Vec3::new(c.x, hi.y, c.z),
        Handle::Face(Face::Bottom) => Vec3::new(c.x, lo.y, c.z),
        Handle::Arrow(_) => origin(lo, hi, s),
    }
}

/// A face handle's half size where it is seen from `eye`: the same on screen
/// near or far (about 1.5% of the view's height).
fn face_half(eye: Vec3, at: Vec3) -> f32 {
    (eye.distance(at) * 0.012).max(6.0)
}

/// A handle's box, for picking and drawing: a face's a cube on its middle,
/// an arrow's its shaft and head.
fn handle_box(h: Handle, lo: Vec3, hi: Vec3, s: f32, eye: Vec3) -> (Vec3, Vec3) {
    let at = spot(h, lo, hi, s);
    match h {
        Handle::Face(_) => (at - Vec3::splat(face_half(eye, at)), at + Vec3::splat(face_half(eye, at))),
        Handle::Arrow(_) => {
            let (w, tip) = (Vec3::splat(s * 0.06), at + h.dir() * s);
            ((at - w).min(tip - w), (at + w).max(tip + w))
        }
    }
}

/// The handle of the gizmo on `lo..hi` under the ray `o + t d` (the nearest
/// it meets), if any.
pub fn pick(lo: Vec3, hi: Vec3, eye: Vec3, o: Vec3, d: Vec3) -> Option<Handle> {
    let s = size(eye, lo, hi);
    Handle::ALL
        .into_iter()
        .filter_map(|h| {
            let (a, b) = handle_box(h, lo, hi, s, eye);
            let pad = (b - a) * 0.25;
            ray_aabb(o, d, a - pad, b + pad).map(|t| (h, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(h, _)| h)
}

/// The change a handle of `lot`'s box dragged `by` world units along its
/// line is ([`Handle::dir`]): the side view's drag with the same part, so it
/// snaps as that does. None where it changes nothing.
pub fn change_by(lot: &Lot, h: Handle, by: f32) -> Option<Change> {
    let (across, up) = ((by / CELL, 0.0), (0.0, by / UNITS));
    let (axis, part, to) = match h {
        Handle::Face(Face::West) => (Axis::X, SidePart::End(-1), across),
        Handle::Face(Face::East) => (Axis::X, SidePart::End(1), across),
        Handle::Face(Face::South) => (Axis::Y, SidePart::End(-1), across),
        Handle::Face(Face::North) => (Axis::Y, SidePart::End(1), across),
        Handle::Face(Face::Top) => (Axis::X, SidePart::Top, up),
        Handle::Face(Face::Bottom) => (Axis::X, SidePart::Bottom, up),
        Handle::Arrow(0) => (Axis::X, SidePart::Body, across),
        Handle::Arrow(1) => (Axis::Y, SidePart::Body, across),
        Handle::Arrow(_) => (Axis::X, SidePart::Body, up),
    };
    sideview::drag(lot, &Section { axis, at: 0 }, part, (0.0, 0.0), to)
}

/// A drag of the gizmo, from where it took hold.
#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    pub handle: Handle,
    /// The room's box as it was when the button went down.
    pub lot: Lot,
    /// The handle's line: through here, along [`Handle::dir`].
    at: Vec3,
    /// Where along it the pointer's ray passed when it took hold.
    t0: f32,
}

impl Grab {
    /// Take hold of `handle` of `lot`'s gizmo, seen from `eye`, under the ray.
    pub fn start(handle: Handle, lot: &Lot, eye: Vec3, o: Vec3, d: Vec3) -> Option<Grab> {
        let (lo, hi) = world_box(lot);
        let at = spot(handle, lo, hi, size(eye, lo, hi));
        let t0 = closest_t_on_axis(at, handle.dir(), o, d)?;
        Some(Grab { handle, lot: lot.clone(), at, t0 })
    }

    /// How far along its line the drag has gone for the ray now (world units).
    pub fn by(&self, o: Vec3, d: Vec3) -> Option<f32> {
        Some(closest_t_on_axis(self.at, self.handle.dir(), o, d)? - self.t0)
    }

    /// The change the drag is for the ray now. None where it changes nothing
    /// (or the ray runs along the line).
    pub fn change(&self, o: Vec3, d: Vec3) -> Option<Change> {
        change_by(&self.lot, self.handle, self.by(o, d)?)
    }
}

/// The room under the ray `o + t d` (unit `d`) in the preview: the one whose
/// box holds the first thing the ray meets that is drawn, a pod before the
/// hall it is in. A room's top is its ceiling unless the cutaway leaves
/// ceilings out; nothing over the cutaway's height is met (nor a room whose
/// floor is over it). The surface met is a floor or a wall of the level's
/// tiles, so a click through a doorway picks the room beyond. None where the
/// ray meets nothing, or a corridor (no room's box).
pub fn pick_room(pv: &Preview, o: Vec3, d: Vec3) -> Option<String> {
    let (start, t) = met(pv, o, d)?;
    let cut = pv.cut.height();
    let shown = |r: &&RoomBox| cut.is_none_or(|h| (r.floor as f32) < h);
    let holder = |p: Vec3| {
        pv.rooms.iter().filter(shown).filter(|r| within(r, p, 4.0)).min_by_key(|r| (r.host.is_empty(), (r.rect.2 - r.rect.0) * (r.rect.3 - r.rect.1))).map(|r| r.key.clone())
    };
    // (in the air the surface is seen from, else just past it: a wall seen from outside)
    holder(start + d * (t - 4.0)).or_else(|| holder(start + d * (t + 4.0)))
}

/// How near a wall's plane (cells) a click must meet the level to ask for a doorway in it.
pub const WALL_GRIP: f32 = 0.45;

/// The doorway a click asks for (with "Join" in hand): where the ray meets the level (as
/// [`pick_room`] reads it) within [`WALL_GRIP`] of a room's wall, between its floor and top
/// (pods never: they are joined inside their halls), the nearest such wall; with the storey
/// the point is on (the highest floor at or under it).
pub fn doorway_at(pv: &Preview, o: Vec3, d: Vec3) -> Option<(String, autolevel::recipe::Doorway, i32)> {
    let (start, t) = met(pv, o, d)?;
    let p = start + d * t;
    let at = cell(p);
    let cut = pv.cut.height();
    let rooms = pv.rooms.iter().filter(|r| r.host.is_empty() && cut.is_none_or(|h| (r.floor as f32) < h));
    let rooms = rooms.filter(|r| r.floor as f32 - 4.0 <= p.y && p.y <= r.top as f32 + 4.0);
    let (key, door) = rooms.filter_map(|r| autolevel_edit::planview::wall_of(r.rect, at, WALL_GRIP).map(|(off, d)| (off, r.key.clone(), d))).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, k, d)| (k, d))?;
    let storey = autolevel::storeys::LEVELS.into_iter().filter(|z| *z as f32 <= p.y + 4.0).max()?;
    Some((key, door, storey))
}

/// The storey of the place the ray first meets (the highest floor at or under
/// it): where a click in 3D picks a room to join from.
pub fn storey_at(pv: &Preview, o: Vec3, d: Vec3) -> Option<i32> {
    let (start, t) = met(pv, o, d)?;
    let y = (start + d * t).y;
    autolevel::storeys::LEVELS.into_iter().filter(|z| *z as f32 <= y + 4.0).max()
}

/// The cell and storey a click with the things tool names (`puts::click`): where the ray first
/// meets the level, a hair past the surface (so a pillar's side is the pillar's own cell, a wall
/// the cell behind its face), and the highest floor at or under the point met.
pub fn place_at(pv: &Preview, o: Vec3, d: Vec3) -> Option<((i32, i32), i32)> {
    let (start, t) = met(pv, o, d)?;
    let (x, y) = cell(start + d * (t + 2.0));
    let z = autolevel::storeys::LEVELS.into_iter().filter(|z| *z as f32 <= (start + d * t).y + 4.0).max()?;
    Some(((x.floor() as i32, y.floor() as i32), z))
}

/// Where a thing put by place is in the world (low corner and high), to frame it: a window's
/// wall cells on its storey, a pillar or crate where it stands (else the cell it was put on), a
/// drop's hole in its room's floor. None for a drop taken out.
pub fn thing_box(lv: &Level, e: &Edit, took: bool) -> Option<(Vec3, Vec3)> {
    let floor = |room: &str| lv.spaces().iter().find(|s| s.key == room).map_or(0, |s| s.floor) as f32;
    let (r, lo, hi) = match e {
        Edit::Window { room, storey, at, .. } => {
            let wall = autolevel::windows::places(lv).into_iter().find(|p| p.hall == *room && p.z == *storey && p.wall.contains(at)).map_or(vec![*at], |p| p.wall);
            let (x0, y0) = (wall.iter().map(|c| c.0).min()?, wall.iter().map(|c| c.1).min()?);
            let (x1, y1) = (wall.iter().map(|c| c.0).max()? + 1, wall.iter().map(|c| c.1).max()? + 1);
            ((x0 as f32, y0 as f32, x1 as f32, y1 as f32), *storey as f32, (*storey + 236) as f32)
        }
        Edit::Piece { room, at, to, .. } => {
            let end = to.unwrap_or(*at);
            let it = lv.interiors.iter().find(|(k, _)| k == room).map(|(_, it)| it);
            let c = it.and_then(|it| it.piece_at(end).map(|i| it.cover[i])).filter(|_| took);
            let r = c.map_or((end.0 as f32, end.1 as f32, end.0 as f32 + 1.0, end.1 as f32 + 1.0), |c| (c.0 as f32 / 2.0, c.1 as f32 / 2.0, c.2 as f32 / 2.0, c.3 as f32 / 2.0));
            (r, floor(room), floor(room) + c.map_or(118, |c| c.4) as f32)
        }
        Edit::Drop { room, at: Some(at) } => ((at.0 as f32, at.1 as f32, at.0 as f32 + 2.0, at.1 as f32 + 2.0), floor(room) - 30.0, floor(room) + 30.0),
        _ => return None,
    };
    let (a, b) = (world((r.0, r.1), lo), world((r.2, r.3), hi));
    Some((a.min(b), a.max(b)))
}

/// Where the ray `o + t d` first meets what is drawn of the preview (a tile, or with no
/// ceilings drawn a room's top from under it), cut where the cutaway cuts: the ray's start
/// (where it crosses the cut, from over it) and `t` along it from there.
fn met(pv: &Preview, o: Vec3, d: Vec3) -> Option<(Vec3, f32)> {
    let cut = pv.cut.height();
    let start = match cut {
        Some(h) if o.y > h => {
            if d.y >= -1e-6 {
                return None;
            }
            o + d * ((h - o.y) / d.y)
        }
        _ => o,
    };
    let shown = |r: &&RoomBox| cut.is_none_or(|h| (r.floor as f32) < h);
    const REACH: f32 = 30000.0;
    let mut t = pv.level.first_tile_hit(start, start + d * REACH).map_or(f32::INFINITY, |hit| hit.dist);
    if let Some(h) = cut.filter(|_| d.y > 1e-6) {
        // (the ray comes up out of the cut: nothing further is drawn)
        t = t.min((h - start.y) / d.y);
    }
    if !pv.cut.ceilings && d.y > 1e-6 {
        for r in pv.rooms.iter().filter(shown).filter(|r| cut.is_none_or(|h| r.top as f32 <= h)) {
            let tc = (r.top as f32 - start.y) / d.y;
            if tc > 0.0 && tc < t && within(r, start + d * tc, 0.0) {
                t = tc;
            }
        }
    }
    t.is_finite().then_some((start, t))
}

/// Is `p` in the room's box, within `pad` units of its floor and top?
fn within(r: &RoomBox, p: Vec3, pad: f32) -> bool {
    let (x, y) = cell(p);
    let e = 0.02;
    r.rect.0 as f32 - e <= x && x <= r.rect.2 as f32 + e && r.rect.1 as f32 - e <= y && y <= r.rect.3 as f32 + e && r.floor as f32 - pad <= p.y && p.y <= r.top as f32 + pad
}

/// The colours: the room's box as it stands, the box a drag would leave
/// (amber), and refused (red).
pub const BOX: [f32; 4] = [0.95, 0.95, 0.95, 0.9];
pub const TRIED: [f32; 4] = [1.0, 0.75, 0.25, 1.0];
pub const REFUSED: [f32; 4] = [1.0, 0.2, 0.2, 1.0];

/// What the gizmo draws: the selected room's box and its handles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    /// The selected room's box as it stands.
    pub lot: Option<Lot>,
    /// While a drag is tried: the boxes it would leave (the room's, a hall's
    /// pods with it), and whether it is refused.
    pub tried: Vec<Lot>,
    pub refused: bool,
    /// The handle under the pointer or held.
    pub hot: Option<Handle>,
    /// The doorways asked for of a join (with "Join" in hand): each a panel on
    /// its wall, low corner and high, drawn on top.
    pub doors: Vec<(Vec3, Vec3)>,
    /// The things put by place (`thing_box`), each with whether the level took
    /// it: framed on top, magenta, or red where it did not.
    pub marks: Vec<(Vec3, Vec3, bool)>,
}

/// The colour of a doorway asked for (the plan's `colour::ASKED`).
pub const ASKED: [f32; 4] = [1.0, 0.47, 1.0, 1.0];

/// A doorway on a room's wall (the rectangle `rect` in cells), on the storey
/// whose floor is `z`, as a panel in the world: a cell wide, a door tall.
pub fn doorway_box(rect: (i32, i32, i32, i32), d: autolevel::recipe::Doorway, z: i32) -> (Vec3, Vec3) {
    let (x0, y0, x1, y1) = rect;
    let a = d.at as f32;
    let (p, q) = match d.side {
        'E' => ((x1 as f32, a), (x1 as f32, a + 1.0)),
        'W' => ((x0 as f32, a), (x0 as f32, a + 1.0)),
        'N' => ((a, y1 as f32), (a + 1.0, y1 as f32)),
        _ => ((a, y0 as f32), (a + 1.0, y0 as f32)),
    };
    let (lo, hi) = (world(p, z as f32), world(q, (z + autolevel::felicity::DOOR_H) as f32));
    (lo.min(hi), lo.max(hi))
}

/// Draw it into `mesh`, seen from `eye`: the box's edges in the scene (faint
/// where walls hide them), the handles on top; a drag's boxes on top; the
/// doorways asked for on top.
pub fn draw(mesh: &mut Mesh3d, shown: &Shown, eye: Vec3) {
    for &(lo, hi, took) in &shown.marks {
        let col = if took { ASKED } else { REFUSED };
        mesh.wire_box(true, lo - Vec3::splat(3.0), hi + Vec3::splat(3.0), col);
        mesh.wire_box(true, lo - Vec3::splat(6.0), hi + Vec3::splat(6.0), col);
    }
    for &(lo, hi) in &shown.doors {
        // (a panel standing in the wall's plane, its four edges doubled)
        let (a, b, c, d) = if lo.x == hi.x {
            (lo, Vec3::new(lo.x, lo.y, hi.z), hi, Vec3::new(lo.x, hi.y, lo.z))
        } else {
            (lo, Vec3::new(hi.x, lo.y, lo.z), hi, Vec3::new(lo.x, hi.y, hi.z))
        };
        mesh.quad(true, a, b, c, d, [ASKED[0], ASKED[1], ASKED[2], 0.45]);
        mesh.wire_box(true, lo - Vec3::splat(2.0), hi + Vec3::splat(2.0), ASKED);
    }
    let Some(lot) = &shown.lot else { return };
    let (lo, hi) = world_box(lot);
    let dragging = !shown.tried.is_empty();
    mesh.wire_box(false, lo, hi, BOX);
    // (the floor's outline, on top, so the room reads from anywhere)
    let lift = Vec3::Y * 3.0;
    let floor = [Vec3::new(lo.x, lo.y, lo.z), Vec3::new(hi.x, lo.y, lo.z), Vec3::new(hi.x, lo.y, hi.z), Vec3::new(lo.x, lo.y, hi.z)];
    for k in 0..4 {
        mesh.line(true, floor[k] + lift, floor[(k + 1) % 4] + lift, [BOX[0], BOX[1], BOX[2], 0.5]);
    }
    let col = if shown.refused { REFUSED } else { TRIED };
    for b in &shown.tried {
        let (a, z) = world_box(b);
        mesh.wire_box(true, a, z, col);
        // (doubled a hair outside, so it reads thicker than the box it came from)
        mesh.wire_box(true, a - Vec3::splat(2.0), z + Vec3::splat(2.0), col);
    }
    let s = size(eye, lo, hi);
    for h in Handle::ALL {
        let held = shown.hot == Some(h);
        if dragging && !held {
            continue;
        }
        let c = h.colour();
        let c = if held { [(c[0] * 1.3 + 0.2).min(1.0), (c[1] * 1.3 + 0.2).min(1.0), (c[2] * 1.3 + 0.2).min(1.0), 1.0] } else { c };
        let at = spot(h, lo, hi, s);
        match h {
            Handle::Face(_) => {
                let r = Vec3::splat(face_half(eye, at) * if held { 1.35 } else { 1.0 });
                mesh.cube(true, at - r, at + r, c);
            }
            Handle::Arrow(_) => {
                let a = h.dir();
                let (w, hw) = (Vec3::splat(s * 0.022), Vec3::splat(s * 0.065));
                let shaft = at + a * s * 0.78;
                mesh.cube(true, (at - w).min(shaft - w), (at + w).max(shaft + w), c);
                let tip = at + a * s;
                mesh.cube(true, (shaft - hw).min(tip - hw), (shaft + hw).max(tip + hw), c);
            }
        }
    }
    if !dragging {
        let c = Vec3::splat(s * 0.045);
        let o = origin(lo, hi, s);
        mesh.cube(true, o - c, o + c, [0.9, 0.9, 0.9, 1.0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::storeys::LEVELS;

    fn lot() -> Lot {
        Lot { key: "hall".into(), kind: "medium", x0: 10, y0: 20, x1: 18, y1: 26, floor: 0, top: 512, host: String::new() }
    }

    #[test]
    fn a_lots_box_is_where_the_preview_puts_its_cells() {
        let (lo, hi) = world_box(&lot());
        assert_eq!(lo, Vec3::new(10.0 * CELL, 0.0, -26.0 * CELL));
        assert_eq!(hi, Vec3::new(18.0 * CELL, 512.0, -20.0 * CELL));
        // the south face is the plan's y0 side: the world's high z
        assert_eq!(spot(Handle::Face(Face::South), lo, hi, 100.0).z, hi.z);
        assert_eq!(spot(Handle::Face(Face::North), lo, hi, 100.0).z, lo.z);
    }

    #[test]
    fn each_handle_dragged_is_the_change_the_side_view_would_make() {
        let a = lot();
        let pull = |h: &str, by: f32| change_by(&a, Handle::named(h).unwrap(), by);
        // a side two cells out
        assert_eq!(pull("east", 2.0 * CELL), Some(Change::Rect { room: "hall".into(), rect: (10, 20, 20, 26) }));
        assert_eq!(pull("west", 1.0 * CELL), Some(Change::Rect { room: "hall".into(), rect: (11, 20, 18, 26) }));
        assert_eq!(pull("north", 3.0 * CELL), Some(Change::Rect { room: "hall".into(), rect: (10, 20, 18, 29) }));
        assert_eq!(pull("south", -2.0 * CELL), Some(Change::Rect { room: "hall".into(), rect: (10, 18, 18, 26) }));
        // under half a cell is nothing
        assert_eq!(pull("east", 0.4 * CELL), None);
        // the top up a ceiling line, the floor up a storey with the top kept
        assert_eq!(pull("top", 276.0), Some(Change::Height { room: "hall".into(), floor: 0, top: 787 }));
        assert_eq!(pull("bottom", 276.0), Some(Change::Height { room: "hall".into(), floor: 276, top: 512 }));
        // never under a storey tall
        assert_eq!(pull("top", -600.0), Some(Change::Height { room: "hall".into(), floor: 0, top: 236 }));
        // the arrows: moved along x and y by cells, up by storeys
        assert_eq!(pull("x", -3.0 * CELL), Some(Change::Move { room: "hall".into(), to: (7, 20) }));
        assert_eq!(pull("y", 1.0 * CELL), Some(Change::Move { room: "hall".into(), to: (10, 21) }));
        assert_eq!(pull("z", 2.0 * 276.0), Some(Change::Pin { room: "hall".into(), floor: LEVELS[3] }));
        assert_eq!(pull("z", 100.0), None);
        assert!(Handle::ALL.iter().all(|h| Handle::named(h.name()) == Some(*h)));
    }

    #[test]
    fn a_handle_is_picked_and_a_drag_follows_the_pointer_along_its_line() {
        let a = lot();
        let (lo, hi) = world_box(&a);
        let eye = Vec3::new((lo.x + hi.x) * 0.5 + 300.0, 1500.0, hi.z + 1800.0);
        let s = size(eye, lo, hi);
        let aim = |p: Vec3| (eye, (p - eye).normalize());
        // at the east face's handle
        let east = spot(Handle::Face(Face::East), lo, hi, s);
        let (o, d) = aim(east);
        assert_eq!(pick(lo, hi, eye, o, d), Some(Handle::Face(Face::East)));
        // at the top's, and at the up arrow's shaft
        let (o2, d2) = aim(spot(Handle::Face(Face::Top), lo, hi, s));
        assert_eq!(pick(lo, hi, eye, o2, d2), Some(Handle::Face(Face::Top)));
        let (o3, d3) = aim(origin(lo, hi, s) + Vec3::Y * s * 0.5);
        assert_eq!(pick(lo, hi, eye, o3, d3), Some(Handle::Arrow(2)));
        // the middle of the floor is the bottom's handle; a quarter of the way across, none
        let (o4, d4) = aim(Vec3::new((lo.x + hi.x) * 0.5, 0.0, (lo.z + hi.z) * 0.5));
        assert_eq!(pick(lo, hi, eye, o4, d4), Some(Handle::Face(Face::Bottom)));
        let (o4, d4) = aim(Vec3::new(lo.x * 0.75 + hi.x * 0.25, 0.0, lo.z * 0.75 + hi.z * 0.25));
        assert_eq!(pick(lo, hi, eye, o4, d4), None);
        // the east face taken and the pointer moved to aim 2.2 cells further east: pulled 2
        let g = Grab::start(Handle::Face(Face::East), &a, eye, o, d).unwrap();
        let (o, d) = aim(east + Vec3::X * 2.2 * CELL);
        assert!((g.by(o, d).unwrap() - 2.2 * CELL).abs() < 1.0);
        assert_eq!(g.change(o, d), Some(Change::Rect { room: "hall".into(), rect: (10, 20, 20, 26) }));
    }
}
