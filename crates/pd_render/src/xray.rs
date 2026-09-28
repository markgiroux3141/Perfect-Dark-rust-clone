//! The Farsight's x-ray (`VISIONMODE_XRAY`) as PD draws it: the view cleared
//! black (`sky_render`), the BG only within the eraser sphere with PD's
//! per-vertex colours (`bg_render_scene_in_xray` / `bg_render_gdl_in_xray` /
//! `bg_choose_xray_vtx_colour`, `bg.c:891`, `:760`, `:451`), and every prop in
//! the colour its distance from the eraser gives it: objects (`propobj.c:12720`,
//! `:12842`), smoke (`smoke.c:180`), explosions (`explosions.c:1314`) and
//! sparks (`sparks.c:333`). No bullet holes (`wallhit.c:1332`), no gun.
//!
//! PD's arenas are tessellated finely enough for per-vertex colour to read as a
//! sphere (their `g_Stages[].unk2c` is -1, so `bg_process_xray_tri` never
//! subdivides); the BG's own triangles are drawn.
//!
//! Source: the old repo's `pd_guns/xray.rs`, whose range faces were cut into
//! 50 cm cells to stand in for a room's vertex density; a stage needs none.

use glam::Vec3;
use pd_sim::player::vision::Eraser;

use crate::fx::{FxBatch, FxKind, FxVert};

/// The fade every x-ray prop shares: `None` beyond `eraserpropdist`, else
/// (distance frac 0..1, alpha frac 1 → 0 over the last 150 cm).
pub fn prop_fade(e: &Eraser, pos: Vec3) -> Option<(f32, f32)> {
    let dist = (pos - e.pos).length();
    if dist > e.propdist {
        return None;
    }
    let fadedist = e.propdist - 150.0;
    let alpha = if dist > fadedist { 1.0 - (dist - fadedist) / 150.0 } else { 1.0 };
    Some(((dist / e.propdist).min(1.0), alpha))
}

/// An object's x-ray colour (`propobj.c:12720`, `:12842`): alpha 128 fading
/// out, `colour[epcol_0] = frac·255`, `colour[epcol_1] = (1 − frac)·255`,
/// `colour[epcol_2] = 0`, drawn flat through the fog at full weight.
pub fn obj_colour(e: &Eraser, pos: Vec3) -> Option<[f32; 4]> {
    let (frac, fade) = prop_fade(e, pos)?;
    let alpha = (fade * 128.0).floor();
    if alpha <= 0.0 {
        return None;
    }
    let mut c = [0.0f32; 4];
    c[e.epcol[0]] = (frac * 255.0).floor() / 255.0;
    c[e.epcol[1]] = ((1.0 - frac) * 255.0).floor() / 255.0;
    c[e.epcol[2]] = 0.0;
    c[3] = alpha / 255.0;
    Some(c)
}

/// Smoke in x-ray (`smoke.c:180`): red → green with distance, the part's
/// alpha × 0.5 fading out. `alpha` is 0..255.
pub fn smoke_colour(e: &Eraser, pos: Vec3, alpha: f32) -> Option<[f32; 4]> {
    let (frac, fade) = prop_fade(e, pos)?;
    let a = ((alpha * fade * 0.5) as u32 & 0xff) as f32;
    Some([(frac * 255.0).floor() / 255.0, ((1.0 - frac) * 255.0).floor() / 255.0, 0.0, a / 255.0])
}

/// An explosion in x-ray (`explosions.c:1314`): `red << 24 | green << 16 |
/// alpha | 0x80800000`, red and green 0..127 over half-bright bases.
pub fn explosion_colour(e: &Eraser, pos: Vec3) -> Option<[f32; 4]> {
    let (frac, fade) = prop_fade(e, pos)?;
    let alpha = (fade * 128.0) as u32;
    let red = (frac * 127.0) as u32;
    let green = ((1.0 - frac) * 127.0) as u32;
    Some([(0x80 | red) as f32 / 255.0, (0x80 | green) as f32 / 255.0, 0.0, alpha as f32 / 255.0])
}

/// A spark group in x-ray (`sparks.c:375`): red and green by distance, blue
/// 0x3f, alpha from `unk1c` scaled by the fade (PD's @bug: both colours read
/// `unk1c`).
pub fn spark_colour(e: &Eraser, pos: Vec3, unk1c_alpha: f32) -> Option<[f32; 4]> {
    let dist = (pos - e.pos).length();
    if dist > e.propdist {
        return None;
    }
    let f12 = e.propdist - 150.0;
    let sp138 = if f12 < dist { 1.0 - (dist - f12) / 150.0 } else { 1.0 };
    let frac = (dist / e.propdist).min(1.0);
    let a = (sp138 * unk1c_alpha * 255.0) as u32 as f32;
    Some([(frac * 255.0).floor() / 255.0, ((1.0 - frac) * 255.0).floor() / 255.0, 0x3f as f32 / 255.0, a / 255.0])
}

/// `struct xraydata` as `bg_render_gdl_in_xray` fills it (`bg.c:771`).
struct XrayData {
    centre: Vec3,
    radius: f32,
    radius_sq: f32,
    /// `unk014`: where the alpha starts fading.
    fadefrom: f32,
    /// `unk01c`: where the near colour band ends.
    near: f32,
}

impl XrayData {
    fn new(e: &Eraser) -> Self {
        let radius = e.bgdist;
        XrayData { centre: e.pos, radius, radius_sq: radius * radius, fadefrom: 0.25, near: (e.propdist / radius).min(0.7) }
    }
}

/// `bg_choose_xray_vtx_colour` (`bg.c:451`): `None` out of range (PD's
/// `0x0000ff00`, invisible), else the vertex's RGBA word decoded.
fn bg_choose_xray_vtx_colour(v: Vec3, xd: &XrayData, ecol: [u32; 3]) -> Option<[f32; 4]> {
    let d = v - xd.centre;
    let (dx, dy, dz) = (d.x * d.x, d.y * d.y, d.z * d.z);
    if dx >= xd.radius_sq || dz >= xd.radius_sq || dy >= xd.radius_sq {
        return None;
    }
    let dist = (dx + dy + dz).sqrt();
    if dist >= xd.radius {
        return None;
    }
    let f12 = dist / xd.radius;
    let alphafrac = if xd.fadefrom < f12 { 1.0 - (f12 - xd.fadefrom) / (1.0 - xd.fadefrom) } else { 1.0 };
    let word: u32 = if f12 < xd.near {
        let anglefrac = f12 / xd.near;
        let colfrac = ((1.0 - anglefrac) * std::f32::consts::FRAC_PI_2).sin();
        ((colfrac * 255.0) as u32) << ecol[0] | (((1.0 - colfrac) * 255.0) as u32) << ecol[1] | (alphafrac * 128.0) as u32
    } else {
        let anglefrac = (f12 - xd.near) / (1.0 - xd.near);
        let anglefrac = 0.65 * anglefrac + 0.35;
        let colfrac = (anglefrac * std::f32::consts::FRAC_PI_2).sin();
        ((colfrac * 255.0) as u32) << ecol[2] | 0xff << ecol[1] | (alphafrac * 128.0) as u32
    };
    Some(unpack(word))
}

fn unpack(w: u32) -> [f32; 4] {
    [(w >> 24 & 0xff) as f32 / 255.0, (w >> 16 & 0xff) as f32 / 255.0, (w >> 8 & 0xff) as f32 / 255.0, (w & 0xff) as f32 / 255.0]
}

/// The BG in x-ray: each triangle with any vertex in range (`bg_process_xray_tri`
/// with no subdivision → `bg_add_xray_tri`), out-of-range vertices transparent
/// blue. Drawn with `G_CC_SHADE` / `G_RM_AA_XLU_SURF`: no z, no cull
/// ([`FxKind::XrayBg`]).
pub fn bg_geometry(bghit: &pd_sim::stage::BgHitMesh, e: &Eraser) -> FxBatch {
    let xd = XrayData::new(e);
    let reach = Vec3::splat(xd.radius);
    let mut verts = Vec::new();
    for tri in bghit.tris_in(e.pos - reach, e.pos + reach) {
        let cols = tri.map(|p| bg_choose_xray_vtx_colour(p, &xd, e.ecol));
        if cols.iter().all(|c| c.is_none()) {
            continue;
        }
        for (p, c) in tri.iter().zip(cols) {
            verts.push(FxVert { pos: p.to_array(), st: [0.0, 0.0], col: c.unwrap_or([0.0, 0.0, 1.0, 0.0]) });
        }
    }
    FxBatch { kind: FxKind::XrayBg, verts }
}

/// A box (a firing-range board) in x-ray: its camera-facing faces in the object
/// colour (`G_RM_AA_ZB_XLU_SURF` writes no z, so only the front faces show).
pub fn box_geometry(min: Vec3, max: Vec3, campos: Vec3, col: [f32; 4], out: &mut Vec<FxVert>) {
    let c = (min + max) * 0.5;
    for axis in 0..3 {
        for side in [-1.0f32, 1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = side;
            let mut centre = c;
            centre[axis] = if side < 0.0 { min[axis] } else { max[axis] };
            if n.dot(campos - centre) <= 0.0 {
                continue;
            }
            let (a1, a2) = ((axis + 1) % 3, (axis + 2) % 3);
            let corner = |s1: f32, s2: f32| {
                let mut p = centre;
                p[a1] = if s1 < 0.0 { min[a1] } else { max[a1] };
                p[a2] = if s2 < 0.0 { min[a2] } else { max[a2] };
                FxVert { pos: p.to_array(), st: [0.0, 0.0], col }
            };
            let q = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
            out.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Farsight's BG colours: full green at the eraser, red plus blue past
    /// the near band, nothing past the reach.
    #[test]
    fn farsight_vertex_colours_run_green_to_red_to_blue_and_fade() {
        let e = Eraser { pos: Vec3::ZERO, propdist: 400.0, bgdist: 400.0, ..Eraser::default() };
        let xd = XrayData::new(&e);
        assert_eq!(bg_choose_xray_vtx_colour(Vec3::ZERO, &xd, e.ecol).unwrap(), [0.0, 1.0, 0.0, 128.0 / 255.0]);
        let c = bg_choose_xray_vtx_colour(Vec3::new(300.0, 0.0, 0.0), &xd, e.ecol).unwrap();
        assert_eq!(c[0], 1.0);
        assert!(c[2] > 0.5 && c[3] < 0.5, "{c:?}");
        assert!(bg_choose_xray_vtx_colour(Vec3::new(401.0, 0.0, 0.0), &xd, e.ecol).is_none());
        assert!(obj_colour(&e, Vec3::new(390.0, 0.0, 0.0)).is_some() && obj_colour(&e, Vec3::new(401.0, 0.0, 0.0)).is_none());
    }
}
