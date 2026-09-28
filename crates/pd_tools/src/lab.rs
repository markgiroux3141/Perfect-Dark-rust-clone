//! `pd_lab`'s picture of a match, from above: the floors (by height band),
//! the route graph (PD's or ours), the pickups (dim while they respawn), and
//! every chr with its facing, its go-to's waypoints, the pickup it is
//! fetching, its target and its sight polls. It is a list of 2D
//! primitives in world x/z (cm), which the window draws with egui
//! (`bin/pd_lab.rs`) and `pd_snapshot lab` rasterises into a PNG.
//!
//! Source: the old repo's `pd_spike/viewer.rs` and `debug_draw.rs` (the
//! top-down debug view).

use glam::Vec2;
use pd_sim::chr::Act;
use pd_sim::harness::abtest::BANDS;
use pd_sim::nav::NavGraph;
use pd_sim::stage::{FloorKind, TileLevel};
use pd_sim::world::World;

/// One thing to draw, in world x/z; widths and radii in cm unless `px`.
#[derive(Clone, Debug)]
pub enum Prim {
    Poly { pts: Vec<Vec2>, fill: [u8; 4] },
    Line { a: Vec2, b: Vec2, col: [u8; 4], width_px: f32 },
    Circle { c: Vec2, r: f32, col: [u8; 4], filled: bool },
    Text { at: Vec2, text: String, col: [u8; 4] },
}

/// What to show.
#[derive(Clone, Copy, Debug)]
pub struct LabOpts {
    /// Only floors (and chrs) between these heights (cm).
    pub ymin: f32,
    pub ymax: f32,
    pub graph: bool,
    pub routes: bool,
    pub targets: bool,
    pub names: bool,
    pub selected: Option<usize>,
}

impl Default for LabOpts {
    fn default() -> Self {
        LabOpts { ymin: -1000.0, ymax: 1000.0, graph: true, routes: true, targets: true, names: true, selected: None }
    }
}

const BAND_FILL: [[u8; 4]; 4] = [[70, 40, 40, 255], [60, 64, 72, 255], [72, 84, 64, 255], [84, 72, 96, 255]];

fn band_fill(y: f32) -> [u8; 4] {
    BANDS.iter().position(|&(lo, hi, _)| y >= lo && y < hi).map_or([50, 50, 50, 255], |b| BAND_FILL[b])
}

/// The floor polygons between `ymin` and `ymax`, lowest first, shaded by band.
pub fn floors(level: &TileLevel, o: &LabOpts) -> Vec<Prim> {
    let mut polys: Vec<(f32, Prim)> = level
        .geom
        .polys
        .iter()
        .filter(|p| matches!(p.floor_kind(), Some(FloorKind::Flat | FloorKind::Ramp)))
        .filter_map(|p| {
            let y = p.verts.iter().map(|v| v.y).sum::<f32>() / p.verts.len() as f32;
            (y >= o.ymin && y <= o.ymax).then(|| (y, Prim::Poly { pts: p.verts.iter().map(|v| Vec2::new(v.x, v.z)).collect(), fill: band_fill(y) }))
        })
        .collect();
    polys.sort_by(|a, b| a.0.total_cmp(&b.0));
    polys.into_iter().map(|(_, p)| p).collect()
}

/// The route graph: links (one-way ones red), waypoints.
pub fn graph(nav: &NavGraph, o: &LabOpts) -> Vec<Prim> {
    let mut out = Vec::new();
    let at = |w: usize| {
        let p = nav.waypoint_pos(w);
        (Vec2::new(p.x, p.z), p.y)
    };
    for (a, b, one_way) in nav.links() {
        let ((pa, ya), (pb, yb)) = (at(a), at(b));
        if ya.max(yb) < o.ymin || ya.min(yb) > o.ymax {
            continue;
        }
        out.push(Prim::Line { a: pa, b: pb, col: if one_way { [255, 90, 90, 180] } else { [120, 170, 255, 150] }, width_px: 1.0 });
    }
    for w in 0..nav.waypoints.len() {
        let (p, y) = at(w);
        if y >= o.ymin && y <= o.ymax {
            out.push(Prim::Circle { c: p, r: 12.0, col: [150, 200, 255, 220], filled: true });
        }
    }
    out
}

/// A chr's colour by what it is doing.
fn chr_colour(w: &World, i: usize) -> [u8; 4] {
    let c = &w.chrs[i];
    if c.player.is_some() {
        return [255, 255, 255, 255];
    }
    if w.chr_is_dead(i) {
        return [110, 110, 110, 255];
    }
    match c.actiontype {
        Act::GoPos => [255, 200, 60, 255],
        _ if c.aibot.as_ref().is_some_and(|a| a.targetinsight) => [255, 80, 60, 255],
        _ => [90, 230, 120, 255],
    }
}

/// Every chr: its perimeter, facing, route, target and name.
pub fn chrs(w: &World, o: &LabOpts) -> Vec<Prim> {
    let mut out = Vec::new();
    for (i, c) in w.chrs.iter().enumerate() {
        if c.manground < o.ymin - 200.0 || c.manground > o.ymax + 200.0 {
            continue;
        }
        let p = Vec2::new(c.pos.x, c.pos.z);
        let col = chr_colour(w, i);
        let sel = o.selected == Some(i);
        if o.routes && c.actiontype == Act::GoPos {
            let mut prev = p;
            for &wp in c.act_gopos.waypoints.iter().skip(c.act_gopos.curindex) {
                let q = w.nav.waypoint_pos(wp);
                let q = Vec2::new(q.x, q.z);
                out.push(Prim::Line { a: prev, b: q, col: [255, 200, 60, if sel { 255 } else { 120 }], width_px: if sel { 2.5 } else { 1.5 } });
                prev = q;
            }
            let e = c.act_gopos.endpos;
            out.push(Prim::Line { a: prev, b: Vec2::new(e.x, e.z), col: [255, 200, 60, 80], width_px: 1.0 });
        }
        if o.targets {
            if let (Some(t), Some(a)) = (c.target, c.aibot.as_ref()) {
                let tp = w.chrs[t].pos;
                let col = if a.targetinsight { [255, 80, 60, 200] } else { [255, 80, 60, 60] };
                out.push(Prim::Line { a: p, b: Vec2::new(tp.x, tp.z), col, width_px: 1.0 });
            }
        }
        out.push(Prim::Circle { c: p, r: c.radius, col, filled: !w.chr_is_dead(i) });
        if sel {
            out.push(Prim::Circle { c: p, r: c.radius + 25.0, col: [255, 255, 255, 255], filled: false });
        }
        // Facing: chr_get_theta, forward = (sin, cos) in x/z.
        let t = c.theta();
        out.push(Prim::Line { a: p, b: p + Vec2::new(t.sin(), t.cos()) * 70.0, col: [255, 255, 255, 230], width_px: 2.0 });
        if o.names {
            let a = c.aibot.as_ref();
            let mode = a.and_then(|a| a.distmode).map_or("", |d| d.label());
            out.push(Prim::Text { at: p + Vec2::new(30.0, -30.0), text: format!("{} K{}D{} {mode}", c.name, w.mp_chr_kills(i), w.mp_chr_deaths(i)), col });
        }
    }
    out
}

/// The pickups: weapons yellow, crates brown, shields cyan, dim while gone;
/// and a line from each simulant fetching one (`MA_AIBOTGETITEM`) to it.
pub fn pickups(w: &World, o: &LabOpts) -> Vec<Prim> {
    let mut out = Vec::new();
    for ob in w.props.objs.iter().filter(|ob| ob.is_pickup() && ob.pos.y >= o.ymin && ob.pos.y <= o.ymax) {
        let mut col = match ob.ty {
            pd_core::ids::OBJTYPE_WEAPON => [255, 230, 90, 255],
            pd_core::ids::OBJTYPE_SHIELD => [90, 230, 255, 255],
            _ => [190, 130, 70, 255],
        };
        if ob.is_gone() || ob.timetoregen > 0 {
            col[3] = 70;
        }
        let c = Vec2::new(ob.pos.x, ob.pos.z);
        let h = 22.0;
        out.push(Prim::Poly { pts: vec![c + Vec2::new(-h, -h), c + Vec2::new(h, -h), c + Vec2::new(h, h), c + Vec2::new(-h, h)], fill: col });
    }
    for c in &w.chrs {
        let Some(a) = c.aibot.as_ref().filter(|a| a.myaction == pd_sim::bot::MyAction::GetItem) else { continue };
        if let Some(ob) = a.gotoprop.and_then(|id| w.props.get(id)) {
            out.push(Prim::Line { a: Vec2::new(c.pos.x, c.pos.z), b: Vec2::new(ob.pos.x, ob.pos.z), col: [90, 230, 255, 170], width_px: 1.5 });
        }
    }
    out
}

/// The whole picture, back to front.
pub fn picture(w: &World, o: &LabOpts) -> Vec<Prim> {
    let mut out = floors(&w.level, o);
    if o.graph {
        out.extend(graph(&w.nav, o));
    }
    out.extend(pickups(w, o));
    out.extend(chrs(w, o));
    out
}

/// The x/z bounds of the level's floors.
pub fn bounds(level: &TileLevel) -> (Vec2, Vec2) {
    let (lo, hi) = level.geom.bounds();
    (Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z))
}

/// `prims` rasterised onto an RGBA8 image `w` × `h` framing `bounds`
/// (`pd_snapshot lab`); text is left out.
pub fn rasterise(prims: &[Prim], bounds: (Vec2, Vec2), w: usize, h: usize) -> Vec<u8> {
    use n64::rdp::{Blend, Cc, Filter, Gfx, TriState, SV};
    let mut gfx = Gfx::new(w, h);
    gfx.clear([0.08, 0.09, 0.11]);
    let (lo, hi) = bounds;
    let scale = ((w as f32 - 20.0) / (hi.x - lo.x)).min((h as f32 - 20.0) / (hi.y - lo.y));
    let to = |p: Vec2| Vec2::new(10.0 + (p.x - lo.x) * scale, 10.0 + (p.y - lo.y) * scale);
    let st = TriState { cc: Cc::Shade, tex: None, filter: Filter::Point, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false };
    let sv = |p: Vec2, c: [u8; 4]| SV { x: p.x, y: p.y, c: n64::rdp::rgba_f(c), ..SV::default() };
    let quad = |gfx: &mut Gfx, a: Vec2, b: Vec2, half: f32, c: [u8; 4]| {
        let d = (b - a).normalize_or_zero();
        let n = Vec2::new(-d.y, d.x) * half;
        gfx.quad([sv(a + n, c), sv(b + n, c), sv(b - n, c), sv(a - n, c)], &st);
    };
    for p in prims {
        match p {
            Prim::Poly { pts, fill } => {
                for k in 1..pts.len().saturating_sub(1) {
                    gfx.tri([sv(to(pts[0]), *fill), sv(to(pts[k]), *fill), sv(to(pts[k + 1]), *fill)], &st);
                }
            }
            Prim::Line { a, b, col, width_px } => quad(&mut gfx, to(*a), to(*b), width_px * 0.5, *col),
            Prim::Circle { c, r, col, filled } => {
                let (cc, rr) = (to(*c), (r * scale).max(2.0));
                let n = 16;
                for k in 0..n {
                    let (a0, a1) = (k as f32 * std::f32::consts::TAU / n as f32, (k + 1) as f32 * std::f32::consts::TAU / n as f32);
                    let (p0, p1) = (cc + Vec2::new(a0.cos(), a0.sin()) * rr, cc + Vec2::new(a1.cos(), a1.sin()) * rr);
                    if *filled {
                        gfx.tri([sv(cc, *col), sv(p0, *col), sv(p1, *col)], &st);
                    } else {
                        quad(&mut gfx, p0, p1, 0.75, *col);
                    }
                }
            }
            Prim::Text { .. } => {}
        }
    }
    gfx.rgba8(false)
}
