//! A shield's glow (`chr.c`'s `chr_render_shield` → `shieldhit_render` →
//! `shieldhit_render_component`) and the first-person shield flash
//! (`player_render_shield`, `player.c:4349`), from the sim's state
//! (`pd_sim::fx::shieldhit`, `Player::shieldflash`).
//!
//! Each of a chr's part boxes, grown by 10 cm, draws one of: the box a round
//! hit (a fan from the hit point over the face it entered, the four sides back
//! to the far face, the far face; bright at the hit, fading over 80 ticks);
//! the whole shield of a hit with no box (every face); a box the glow has
//! spread to (fans dimmer with each bone travelled); or, during the crawl, the
//! last four bones it lit. The shimmer texture scrolls with
//! `thisframestart240`.
//!
//! `// SUBST:` a chr fading into or out of a cloak draws its boxes with the
//! framebuffer copied under them (`chr_render_cloak`'s refraction, side -7) /
//! not drawn: the cloaked chr is drawn see-through at its cloak alpha.

use glam::{Mat4, Vec3};
use n64::rdp::{Gfx, Texture};
use pd_core::model::hit::HitNode;
use pd_core::model::NodeKind;
use pd_sim::fx::shieldhit::{shield_box_corners, shieldhit_health_to_rgb, ShieldHit};
use pd_sim::player::health::ShieldFlash;
use pd_sim::world::World;

use crate::fx::{FxBatch, FxKind, FxVert};

/// `sp104` (`chr.c:5485`): each face's corners, as indexes into the box's
/// eight (`shield_box_corners`): the low and high x, y and z faces.
const FACES: [[usize; 4]; 6] = [[0, 1, 3, 2], [7, 5, 4, 6], [5, 1, 0, 4], [2, 3, 7, 6], [0, 2, 6, 4], [7, 3, 1, 5]];

/// A vertex in the box's space (PD's `s16` coordinates), its `s, t` (10.5)
/// and its colour's index into the component's colour list.
#[derive(Clone, Copy)]
struct V {
    p: [i32; 3],
    st: [i32; 2],
    col: usize,
}

fn v(p: Vec3, s: i32, t: i32, col: usize) -> V {
    V { p: [p.x as i32, p.y as i32, p.z as i32], st: [s, t], col }
}

/// `(a + b + c + d) >> 2` per axis, as PD averages its `s16`s.
fn centre(q: &[V; 4]) -> [i32; 3] {
    std::array::from_fn(|k| (q[0].p[k] + q[1].p[k] + q[2].p[k] + q[3].p[k]) >> 2)
}

/// Which component a box draws (`shieldhit_render`'s choice).
enum Component<'a> {
    /// `s0`: the hit's own box, its face.
    Hit(&'a ShieldHit),
    /// `s1`: a hit with no box: the whole shield (side -1).
    Whole(&'a ShieldHit),
    /// `s2`: the glow spread here (side -2): ticks since, bones travelled.
    Spread(&'a ShieldHit, i32, i32),
    /// The crawl's `cmnum`..`cmnum4` (sides -3..-6).
    Crawl(usize),
}

/// The triangles `shieldhit_render` draws for chr `ci` at `alpha` (its
/// render alpha, 0..255), in world space; none when nothing shows.
pub fn chr_shield_batch(world: &World, ci: usize, alpha: f32) -> Option<FxBatch> {
    let c = &world.chrs[ci];
    let shield = world.chr_get_shield(ci);
    let cloaking = c.cloak.fadefrac > 0 && !c.cloak.fadefinished;
    if !(c.shieldhit || (shield > 0.0 && c.cmcount < 10) || cloaking) {
        return None;
    }
    let model = &c.model;
    let gap = 10.0 / model.scale;
    let lvframe60 = world.lv.lvframe60;
    // st1..st4 (`chr.c:5645`): the shimmer's scroll.
    let ph = (world.lv.thisframestart240 % 1200) as f32 * 0.005_235_154;
    let st1 = ((ph.sin() + 1.0) * 0.5 * 32.0 * 32.0) as i16 as i32;
    let st2 = ((ph.cos() + 1.0) * 0.5 * 32.0 * 32.0) as i16 as i32;
    let hits: Vec<&ShieldHit> = world.fx.shieldhits.hits.iter().flatten().filter(|h| h.chr == ci).collect();
    let mut verts = Vec::new();
    // Every part box, the head's by the bone it hangs from.
    let mut boxes: Vec<(HitNode, [f32; 6], Mat4)> = Vec::new();
    for (n, node) in model.def.nodes.iter().enumerate() {
        if let NodeKind::BBox { bbox, .. } = &node.kind {
            if let Some(m) = model.def.find_node_mtx_index(n, 0).and_then(|i| model.matrices.get(i).copied()) {
                boxes.push((HitNode::Body(n), *bbox, m));
            }
        }
    }
    if let (Some(head), Some(hm)) = (model.head.as_deref(), model.head_matrix()) {
        for (n, node) in head.nodes.iter().enumerate() {
            if let NodeKind::BBox { bbox, .. } = &node.kind {
                boxes.push((HitNode::Head(n), *bbox, hm));
            }
        }
    }
    for (node, bbox, m) in boxes {
        let index = world.shieldhit_node_to_cmnum(ci, node);
        let latest = |f: &dyn Fn(&&ShieldHit) -> bool| hits.iter().copied().filter(|h| f(h)).max_by_key(|h| h.lvframe60);
        let s0 = latest(&|h| h.node == Some(node));
        let s2 = latest(&|h| h.node.is_some() && h.node != Some(node) && index.is_some_and(|i| i < 32 && h.unk018[i] >= 0));
        let s1 = latest(&|h| h.node.is_none());
        let comp = if let Some(h) = s0 {
            Component::Hit(h)
        } else if let Some(h) = s1 {
            Component::Whole(h)
        } else if let (Some(h), Some(i)) = (s2, index) {
            Component::Spread(h, h.unk018[i], h.unk038[i])
        } else if cloaking {
            continue;
        } else if let Some(k) = index.and_then(|i| c.cmnum.iter().position(|&n| n == i as i32)) {
            Component::Crawl(k)
        } else {
            continue;
        };
        component(&mut verts, &comp, &bbox, gap, &m, shield, c.cmcount, lvframe60, alpha, [st1, st2]);
    }
    (!verts.is_empty()).then_some(FxBatch { kind: FxKind::Shield, verts })
}

/// `shieldhit_render_component` (`chr.c:5452`) for one box.
#[allow(clippy::too_many_arguments)]
fn component(out: &mut Vec<FxVert>, comp: &Component, bbox: &[f32; 6], gap: f32, m: &Mat4, shieldamount: f32, cmcount: u16, lvframe60: i32, alpha: f32, st: [i32; 2]) {
    let corners = shield_box_corners(bbox, gap);
    let (xmin, xmax, ymin, ymax, zmin, zmax) = (corners[0].x, corners[7].x, corners[0].y, corners[7].y, corners[0].z, corners[7].z);
    let (st1, st2) = (st[0], st[1]);
    let (st3, st4) = (st1 + 512, st2 + 512);
    let shield = match comp {
        Component::Hit(h) => h.shield,
        Component::Spread(h, _, arg7) => {
            if h.unk011 < *arg7 {
                0.0
            } else {
                shieldamount * ((4.0 * h.unk011 as f32) - *arg7 as f32 + 1.0) / (4.0 * h.unk011 as f32)
            }
        }
        _ => shieldamount,
    };
    let [r1, g1, b1] = shieldhit_health_to_rgb(shield);
    let c2 = [(r1 - 20).max(0), (g1 - 20).max(0), (b1 - 20).max(0)];
    let c3 = [(r1 - 60).max(0), (g1 - 60).max(0), (b1 - 60).max(0)];
    let rgba = |c: [i32; 3], a: f32| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, (a.clamp(0.0, 255.0) as i32 & 0xff) as f32 / 255.0];
    let fade = |h: &ShieldHit| if lvframe60 - h.lvframe60 <= 80 { ((h.lvframe60 - lvframe60 + 80) as f32 * 3.1875 * alpha * (1.0 / 255.0)) as i32 as f32 } else { 0.0 };
    let mut emit = |tris: &[[V; 3]], cols: &[[f32; 4]]| {
        for t in tris {
            for vv in t {
                let p = m.transform_point3(Vec3::new(vv.p[0] as f32, vv.p[1] as f32, vv.p[2] as f32));
                out.push(FxVert { pos: p.to_array(), st: [vv.st[0] as f32 / 32.0, vv.st[1] as f32 / 32.0], col: cols[vv.col] });
            }
        }
    };
    // A face as a quad with PD's st (s1,t1)-(s3,t1)-(s3,t4)-(s1,t4).
    let quad = |f: [usize; 4], col: usize, st1: i32, st2: i32, st3: i32, st4: i32| -> [V; 4] {
        let c = |k: usize, s: i32, t: i32| v(corners[f[k]], s, t, col);
        [c(0, st1, st2), c(1, st3, st2), c(2, st3, st4), c(3, st1, st4)]
    };
    let fan = |q: &[V; 4], mid: V| -> [[V; 3]; 4] { [[q[0], q[1], mid], [q[1], q[2], mid], [q[2], q[3], mid], [q[3], q[0], mid]] };
    match comp {
        Component::Crawl(k) => {
            // Sides -3..-6: one colour, the scroll parked.
            let a = if cmcount < 10 { alpha * [0.588_235_3, 0.313_725_5, 0.235_294_12, 0.156_862_75][*k] } else { 0.0 };
            let col = if *k == 0 { rgba(c2, a) } else { rgba(c3, a) };
            let (s1, s2, s3, s4) = (0, 0, 512, 512);
            let mut tris = Vec::new();
            for f in FACES {
                let q = quad(f, 0, s1, s2, s3, s4);
                tris.push([q[0], q[1], q[2]]);
                tris.push([q[0], q[2], q[3]]);
            }
            emit(&tris, &[col]);
        }
        Component::Spread(h, arg6, arg7) => {
            // Side -2: each face a fan, its corners in the dim colour, its middle clear.
            let a3 = if h.unk011 < *arg7 { 0.0 } else { ((30.0 - *arg6 as f32) * 5.333_333_5 + 40.0) * ((h.unk011 as f32 - *arg7 as f32 + 1.0) / h.unk011 as f32) * alpha * (1.0 / 255.0) };
            let cols = [rgba(c3, a3), rgba(c3, 0.0)];
            let mut tris = Vec::new();
            for f in FACES {
                let q = quad(f, 0, st1, st2, st3, st4);
                let mid = V { p: centre(&q), st: [(st1 + st3) >> 1, (st2 + st4) >> 1], col: 1 };
                tris.extend(fan(&q, mid));
            }
            emit(&tris, &cols);
        }
        Component::Whole(h) => {
            // Side -1: every face at the hit's fade.
            let col = rgba([r1, g1, b1], fade(h));
            let c = |k: usize, s: i32, t: i32| v(corners[k], s, t, 0);
            let vs = [
                c(0, st1, st2),
                c(1, st3, st2),
                c(2, st3, st2),
                c(3, st1, st2),
                c(4, st1, st4),
                c(5, st3, st4),
                c(6, st3, st4),
                c(7, st1, st4),
                c(2, st1, st4),
                c(3, st3, st4),
                c(6, st1, st2),
                c(7, st3, st2),
            ];
            let mut tris = vec![[vs[0], vs[1], vs[9]], [vs[0], vs[9], vs[8]], [vs[11], vs[5], vs[4]], [vs[11], vs[4], vs[10]]];
            for f in &FACES[2..6] {
                tris.push([vs[f[0]], vs[f[1]], vs[f[2]]]);
                tris.push([vs[f[0]], vs[f[2]], vs[f[3]]]);
            }
            emit(&tris, &[col]);
        }
        Component::Hit(h) => {
            let side = h.side.clamp(0, 5) as usize;
            let a1 = fade(h);
            let cols = [
                rgba([r1, g1, b1], a1),
                rgba(c2, a1),
                rgba(c2, a1),
                rgba([(r1 + 100).min(255), (g1 + 100).min(255), (b1 + 100).min(255)], a1),
                rgba([(c2[0] + 70).min(255), (c2[1] + 70).min(255), (c2[2] + 70).min(255)], a1),
            ];
            // The far face's value on the hit face's axis.
            let far = |mut p: [i32; 3]| {
                match side {
                    0 => p[0] = xmax as i32,
                    1 => p[0] = xmin as i32,
                    2 => p[1] = ymax as i32,
                    3 => p[1] = ymin as i32,
                    4 => p[2] = zmax as i32,
                    _ => p[2] = zmin as i32,
                }
                p
            };
            let q = quad(FACES[side], 0, st1, st2, st3, st4);
            let mid = V { p: h.hitpos.map_or(centre(&q), |hp| [hp[0] as i32, hp[1] as i32, hp[2] as i32]), st: [(st1 + st3) >> 1, (st2 + st4) >> 1], col: 3 };
            let mut tris: Vec<[V; 3]> = fan(&q, mid).to_vec();
            for j in 0..4 {
                let next = (j + 1) % 4;
                let (a, b) = (q[next].p, q[j].p);
                let sq = [
                    V { p: a, st: [st1, st2], col: 1 },
                    V { p: b, st: [st3, st2], col: 1 },
                    V { p: far(b), st: [st3, st4], col: 2 },
                    V { p: far(a), st: [st1, st4], col: 2 },
                ];
                let mid = V { p: centre(&sq), st: [(st1 + st3) >> 1, (st2 + st4) >> 1], col: 4 };
                tris.extend(fan(&sq, mid));
            }
            let back: [V; 4] = std::array::from_fn(|j| V { p: far(q[3 - j].p), st: [[st1, st3, st3, st1][j], [st2, st2, st4, st4][j]], col: 2 });
            let mid = V { p: centre(&back), st: [(st1 + st3) >> 1, (st2 + st4) >> 1], col: 4 };
            tris.extend(fan(&back, mid));
            emit(&tris, &cols);
        }
    }
}

/// `player_render_shield`'s draw (`player.c:4418`): the shield texture over
/// the flash's box, clipped to the framebuffer, 2-cycle `TEXEL0 × ENV` then
/// the prim alpha as the coverage's floor (`G_CC_CUSTOM_01`), blended over
/// the view (`G_RM_CLD_SURF2`), bilinear.
pub fn player_render_shield(gfx: &mut Gfx, tex: &Texture, f: &ShieldFlash) {
    let [cx, cy] = f.centre;
    let [hx, hy] = f.half;
    if hx <= 0.0 || hy <= 0.0 {
        return;
    }
    let (w, h) = (tex.w as f32, tex.h as f32);
    let (x0, x1) = ((cx - hx).max(0.0), (cx + hx).min(gfx.w as f32));
    let (y0, y1) = ((cy - hy).max(0.0), (cy + hy).min(gfx.h as f32));
    let env = [f.env[0] as f32 / 255.0, f.env[1] as f32 / 255.0, f.env[2] as f32 / 255.0, f.env[3] as f32 / 255.0];
    let prim = f.primalpha as f32 / 255.0;
    for y in y0 as i32..y1 as i32 {
        for x in x0 as i32..x1 as i32 {
            // The rectangle's texels: flip swaps s and t, the mirrors run them back.
            let (mut u, mut t) = ((x as f32 + 0.5 - (cx - hx)) / (2.0 * hx), (y as f32 + 0.5 - (cy - hy)) / (2.0 * hy));
            if f.flip {
                std::mem::swap(&mut u, &mut t);
            }
            if f.mirror_s {
                u = 1.0 - u;
            }
            if f.mirror_t {
                t = 1.0 - t;
            }
            let tx = tex.bilerp(u * w, t * h);
            let a1 = tx[3] * env[3];
            let a = (1.0 - a1) * prim + a1;
            let c = [tx[0] * env[0], tx[1] * env[1], tx[2] * env[2], a];
            gfx.blend_px(x, y, c, n64::rdp::Blend::Xlu);
        }
    }
}
