//! What the editor draws into the view besides the stage, as 3D geometry
//! ([`crate::draw3d`]): each item as a pad on its floor (a square, a nub
//! along its facing, a post to see it from afar) in its kind's colour, a
//! ring round each hill, lines from crates to their weapon, the waypoint
//! graph (nodes and links, one-way arrows, the unreachable parts in red), the
//! selection's outline. Also each thing's pick box and where its label goes.

use glam::Vec3;

use crate::doc::{Doc, Item, Kind, Sel, PAD_HEIGHT};
use crate::draw3d::{rgba, Mesh3d};
use crate::scene::Scene;

/// What to show.
#[derive(Clone, Copy, Debug)]
pub struct Show {
    pub items: bool,
    pub cover: bool,
    pub waypoints: bool,
    /// What walls hide drawn faintly.
    pub through_walls: bool,
    pub labels: bool,
}

impl Default for Show {
    fn default() -> Self {
        Show { items: true, cover: false, waypoints: false, through_walls: true, labels: true }
    }
}

pub const SPAWN: [u8; 4] = [90, 230, 110, 255];
pub const WEAPON: [u8; 4] = [255, 150, 30, 255];
pub const AMMO: [u8; 4] = [240, 220, 60, 255];
pub const HILL: [u8; 4] = [235, 90, 235, 255];
pub const COVER: [u8; 4] = [175, 175, 200, 255];
pub const DOOR: [u8; 4] = [120, 190, 255, 255];
/// Capture the Case's four teams.
pub const TEAMS: [[u8; 4]; 4] = [[235, 60, 60, 255], [70, 130, 255, 255], [240, 220, 60, 255], [80, 220, 120, 255]];
pub const NODE: [u8; 4] = [70, 210, 235, 255];
pub const STRANDED: [u8; 4] = [255, 70, 70, 255];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const LINK_FROM: [f32; 4] = [1.0, 0.9, 0.2, 1.0];

/// Nothing past this (cm) is drawn but the selection.
const FAR: f32 = 15000.0;

/// A waypoint node's half size (cm) and its pick box's.
const NODE_HALF: f32 = 9.0;
const NODE_PICK: f32 = 22.0;

pub fn colour(doc: &Doc, it: Item) -> [u8; 4] {
    match it.kind {
        Kind::Spawn => SPAWN,
        Kind::Weapon => WEAPON,
        Kind::Ammo => AMMO,
        Kind::Hill => HILL,
        Kind::Base | Kind::Respawn => TEAMS[doc.team(it).unwrap_or(0) as usize % 4],
        Kind::Cover => COVER,
        Kind::Door => DOOR,
    }
}

/// A kind's pad: (half the square's side, the post's height (0: none)).
fn pad_size(kind: Kind) -> (f32, f32) {
    match kind {
        Kind::Spawn => (30.0, 150.0),
        Kind::Weapon => (40.0, 90.0),
        Kind::Ammo => (32.0, 0.0),
        Kind::Hill => (16.0, 0.0),
        Kind::Base => (45.0, 190.0),
        Kind::Respawn => (22.0, 0.0),
        Kind::Cover => (12.0, 0.0),
        Kind::Door => (0.0, 0.0),
    }
}

/// Something that can be clicked, and its box.
#[derive(Clone, Copy, Debug)]
pub struct Pick {
    pub sel: Sel,
    pub lo: Vec3,
    pub hi: Vec3,
}

/// A label's anchor and text.
pub struct Label {
    pub at: Vec3,
    pub text: String,
    pub col: [u8; 4],
}

#[derive(Default)]
pub struct Built {
    pub mesh: Mesh3d,
    pub picks: Vec<Pick>,
    pub labels: Vec<Label>,
}

/// What to highlight.
#[derive(Clone, Copy, Debug, Default)]
pub struct Highlight {
    pub selected: Option<Sel>,
    pub hovered: Option<Sel>,
    /// The Link tool's first waypoint.
    pub link_from: Option<usize>,
}

fn item_label(doc: &Doc, it: Item) -> String {
    match it.kind {
        Kind::Spawn => format!("Spawn {}", it.index),
        Kind::Weapon => format!("Weapon {} (loc {})", it.index, doc.location(it).unwrap_or(0)),
        Kind::Ammo => format!("Ammo for W{}", doc.crate_weapon(it).unwrap_or(0)),
        Kind::Hill => format!("Hill {}", it.index),
        Kind::Base => format!("Base, team {}", doc.team(it).unwrap_or(0)),
        Kind::Respawn => format!("Respawn, team {}", doc.team(it).unwrap_or(0)),
        Kind::Cover => format!("Cover {}", it.index),
        Kind::Door => {
            let d = doc.door(it.index);
            format!("Door {}: {} ({})", it.index, d.model, d.motion.name())
        }
    }
}

/// A door: its box's edges, a faint face, an arrow out of its front on the
/// floor, and how it opens: a post on its hinge and the arc it swings
/// through, an arrow the way it slides, rises or sinks, a ring for an
/// eyelid or iris. Returns its pick box and its label's anchor.
fn door(mesh: &mut Mesh3d, d: &pd_import::layout::Door, col: [f32; 4]) -> (Vec3, Vec3, Vec3) {
    use pd_import::layout::{Motion, Swing};
    let [w, h, t] = d.size;
    let (front, up, c) = (d.front(), d.up(), d.centre());
    let across = Vec3::Y.cross(front);
    let corner = |x: f32, y: f32, z: f32| c + across * (x * w * 0.5) + Vec3::Y * (y * h * 0.5) + front * (z * t * 0.5);
    let edges = [
        ((-1., -1., -1.), (1., -1., -1.)), ((-1., 1., -1.), (1., 1., -1.)), ((-1., -1., 1.), (1., -1., 1.)), ((-1., 1., 1.), (1., 1., 1.)),
        ((-1., -1., -1.), (-1., 1., -1.)), ((1., -1., -1.), (1., 1., -1.)), ((-1., -1., 1.), (-1., 1., 1.)), ((1., -1., 1.), (1., 1., 1.)),
        ((-1., -1., -1.), (-1., -1., 1.)), ((1., -1., -1.), (1., -1., 1.)), ((-1., 1., -1.), (-1., 1., 1.)), ((1., 1., -1.), (1., 1., 1.)),
    ];
    for (a, b) in edges {
        mesh.line(false, corner(a.0, a.1, a.2), corner(b.0, b.1, b.2), col);
    }
    let face = [col[0], col[1], col[2], col[3] * 0.18];
    mesh.quad(false, corner(-1., -1., 1.), corner(1., -1., 1.), corner(1., 1., 1.), corner(-1., 1., 1.), face);
    // The front: an arrow on the floor.
    let floor = Vec3::from(d.pos) + Vec3::Y * 2.0;
    let tip = floor + front * (t * 0.5 + 45.0);
    let arrow = |mesh: &mut Mesh3d, from: Vec3, to: Vec3, side: Vec3| {
        mesh.line(false, from, to, col);
        let back = (from - to).normalize_or_zero() * 14.0;
        mesh.line(false, to, to + back + side * 9.0, col);
        mesh.line(false, to, to + back - side * 9.0, col);
    };
    arrow(mesh, floor + front * (t * 0.5), tip, across);
    // The hinge (or the edge it slides to) is the pad's `up * ymin` edge.
    let hinge = floor - up * (w * 0.5);
    match d.motion {
        Motion::Swing | Motion::Hull | Motion::Chair => {
            mesh.cube(false, hinge - Vec3::new(3.0, 0.0, 3.0), hinge + Vec3::new(3.0, h, 3.0), col);
            // The arc a swinging door's free edge sweeps, on the floor.
            if d.motion == Motion::Swing {
                let open = (d.maxfrac as f32 / 65536.0).clamp(0.0, 180.0).to_radians();
                let ways: &[f32] = match d.swing {
                    Swing::Front => &[1.0],
                    Swing::Back => &[-1.0],
                    Swing::Both => &[1.0, -1.0],
                };
                for &way in ways {
                    const SEGS: usize = 16;
                    let at = |k: usize| {
                        let a = open * k as f32 / SEGS as f32;
                        hinge + (up * a.cos() + front * way * a.sin()) * w
                    };
                    for k in 0..SEGS {
                        mesh.line(false, at(k), at(k + 1), [col[0], col[1], col[2], col[3] * 0.6]);
                    }
                }
            }
        }
        Motion::Up | Motion::Down => {
            let (from, to) = if d.motion == Motion::Up { (c, c + Vec3::Y * (h * 0.5 + 40.0)) } else { (c, c - Vec3::Y * (h * 0.5 - 5.0)) };
            arrow(mesh, from + front * (t * 0.5 + 2.0), to + front * (t * 0.5 + 2.0), across);
        }
        Motion::Eyelid | Motion::Iris => {
            // A circle on its face.
            const SEGS: usize = 32;
            let r = w.min(h) * 0.45;
            let at = |k: usize| {
                let a = k as f32 / SEGS as f32 * std::f32::consts::TAU;
                c + front * (t * 0.5 + 2.0) + (across * a.cos() + Vec3::Y * a.sin()) * r
            };
            for k in 0..SEGS {
                mesh.line(false, at(k), at(k + 1), col);
            }
        }
        _ => {
            // Slides towards the hinge edge.
            let mid = c + front * (t * 0.5 + 2.0);
            arrow(mesh, mid, mid - up * (w * 0.5 + 30.0), Vec3::Y);
        }
    }
    let pts = edges.iter().flat_map(|(a, b)| [corner(a.0, a.1, a.2), corner(b.0, b.1, b.2)]);
    let (lo, hi) = pts.fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(p), hi.max(p)));
    (lo - Vec3::splat(4.0), hi + Vec3::splat(4.0), c + Vec3::Y * (h * 0.5 + 25.0))
}

/// The pad of an item at pad position `p`: its floor square, facing nub and
/// post. Returns its pick box.
fn pad(mesh: &mut Mesh3d, kind: Kind, p: Vec3, facing: Option<f32>, col: [f32; 4]) -> (Vec3, Vec3) {
    let floor = p.y - PAD_HEIGHT + 2.0;
    let (h, post) = pad_size(kind);
    let c = Vec3::new(p.x, floor, p.z);
    if matches!(kind, Kind::Weapon | Kind::Ammo) {
        // A frame, so the gun or crate the world draws there shows inside it.
        let t = 4.0;
        for (lo, hi) in [(Vec3::new(-h, 0.0, -h), Vec3::new(h, 3.0, -h + t)), (Vec3::new(-h, 0.0, h - t), Vec3::new(h, 3.0, h)), (Vec3::new(-h, 0.0, -h), Vec3::new(-h + t, 3.0, h)), (Vec3::new(h - t, 0.0, -h), Vec3::new(h, 3.0, h))] {
            mesh.cube(false, c + lo, c + hi, col);
        }
    } else {
        mesh.cube(false, c - Vec3::new(h, 0.0, h), c + Vec3::new(h, 3.0, h), col);
    }
    if let Some(f) = facing {
        let dir = pd_import::layout::look(f);
        let n = c + dir * (h + 14.0);
        let s = (h * 0.35).max(6.0);
        mesh.cube(false, n - Vec3::new(s, 0.0, s), n + Vec3::new(s, 6.0, s), col);
    }
    if post > 0.0 {
        let w = 2.5;
        mesh.cube(false, c + Vec3::new(-w, 0.0, -w), c + Vec3::new(w, post, w), col);
        let t = 9.0;
        mesh.cube(false, c + Vec3::new(-t, post, -t), c + Vec3::new(t, post + 2.0 * t, t), col);
    }
    if kind == Kind::Hill {
        mesh.ring(false, c + Vec3::Y * 2.0, 110.0, 125.0, col);
    }
    let reach = if kind == Kind::Hill { 125.0 } else { h };
    (c - Vec3::new(reach, 5.0, reach), c + Vec3::new(reach, post.max(50.0) + 20.0, reach))
}

/// Build the overlay from `eye`.
pub fn build(doc: &Doc, scene: &Scene, eye: Vec3, show: &Show, hl: &Highlight) -> Built {
    let mut b = Built::default();
    let near = |p: Vec3| p.distance(eye) < FAR;

    if show.waypoints {
        let g = &scene.graph;
        let node_col = |i: usize| rgba(if g.part.get(i).is_some_and(|&p| p != 0) { STRANDED } else { NODE });
        for &(x, y, one_way) in &g.links {
            let (px, py) = (g.pos[x], g.pos[y]);
            if !near(px) && !near(py) {
                continue;
            }
            let bad = g.part[x] != 0 || g.part[y] != 0;
            let mut col = rgba(if bad { STRANDED } else { NODE });
            col[3] = 0.75;
            b.mesh.line(false, px, py, col);
            if one_way {
                // An arrowhead two thirds of the way, flat.
                let m = px.lerp(py, 0.66);
                let d = (py - px).with_y(0.0).normalize_or_zero() * 22.0;
                let n = Vec3::new(-d.z, 0.0, d.x) * 0.6;
                b.mesh.line(false, m, m - d + n, col);
                b.mesh.line(false, m, m - d - n, col);
            }
        }
        for (i, &p) in g.pos.iter().enumerate() {
            let sel = hl.selected == Some(Sel::Node(i));
            if !near(p) && !sel {
                continue;
            }
            let (half, col) = if sel {
                (NODE_HALF * 1.6, WHITE)
            } else if hl.link_from == Some(i) {
                (NODE_HALF * 1.6, LINK_FROM)
            } else {
                (NODE_HALF, node_col(i))
            };
            b.mesh.cube(false, p - Vec3::splat(half), p + Vec3::splat(half), col);
            if hl.hovered == Some(Sel::Node(i)) && !sel {
                b.mesh.wire_box(true, p - Vec3::splat(NODE_PICK), p + Vec3::splat(NODE_PICK), WHITE);
            }
            b.picks.push(Pick { sel: Sel::Node(i), lo: p - Vec3::splat(NODE_PICK), hi: p + Vec3::splat(NODE_PICK) });
        }
    }

    if !show.items {
        return b;
    }
    // Lines from each crate to its weapon.
    for i in 0..doc.count(Kind::Ammo) {
        let it = Item { kind: Kind::Ammo, index: i };
        let Some(wi) = doc.crate_weapon(it).filter(|&w| w < doc.count(Kind::Weapon)) else { continue };
        let (p, w) = (doc.pos(it), doc.pos(Item { kind: Kind::Weapon, index: wi }));
        if !near(p) {
            continue;
        }
        let lift = Vec3::Y * (20.0 - PAD_HEIGHT);
        let hot = hl.selected == Some(Sel::Item(it)) || hl.selected == Some(Sel::Item(Item { kind: Kind::Weapon, index: wi }));
        let mut col = rgba(AMMO);
        col[3] = if hot { 1.0 } else { 0.55 };
        b.mesh.line(false, p + lift, w + lift, col);
    }
    for it in doc.items() {
        if it.kind == Kind::Cover && !show.cover {
            continue;
        }
        let p = doc.pos(it);
        let sel = hl.selected == Some(Sel::Item(it));
        if !near(p) && !sel {
            continue;
        }
        let col = rgba(colour(doc, it));
        if it.kind == Kind::Door {
            let (lo, hi, at) = door(&mut b.mesh, doc.door(it.index), if sel { WHITE } else { col });
            if sel || hl.hovered == Some(Sel::Item(it)) {
                b.mesh.wire_box(true, lo, hi, if sel { WHITE } else { [1.0, 1.0, 1.0, 0.6] });
            }
            b.picks.push(Pick { sel: Sel::Item(it), lo, hi });
            if show.labels && (sel || p.distance(eye) < 2500.0) {
                b.labels.push(Label { at, text: item_label(doc, it), col: colour(doc, it) });
            }
            continue;
        }
        let (lo, hi) = pad(&mut b.mesh, it.kind, p, doc.facing(it), col);
        if sel || hl.hovered == Some(Sel::Item(it)) {
            b.mesh.wire_box(true, lo - Vec3::splat(4.0), hi + Vec3::splat(4.0), if sel { WHITE } else { [1.0, 1.0, 1.0, 0.6] });
        }
        b.picks.push(Pick { sel: Sel::Item(it), lo, hi });
        if show.labels && (sel || p.distance(eye) < 2500.0) && it.kind != Kind::Cover {
            let (_, post) = pad_size(it.kind);
            b.labels.push(Label { at: Vec3::new(p.x, p.y - PAD_HEIGHT + post.max(20.0) + 30.0, p.z), text: item_label(doc, it), col: colour(doc, it) });
        }
    }
    b
}

/// The nearest pick box the ray `o + t d` enters.
pub fn pick(picks: &[Pick], o: Vec3, d: Vec3) -> Option<Sel> {
    picks.iter().filter_map(|p| crate::gizmo::ray_aabb(o, d, p.lo, p.hi).map(|t| (p.sel, t))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s)
}
