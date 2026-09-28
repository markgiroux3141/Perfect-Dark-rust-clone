//! PD's effects as triangles: tracers (`beam_render`), impact sparks
//! (`sparks_render`), bullet holes (`wallhits_render`), smoke (`smoke_render`),
//! explosion flares (`explosion_render`), the N-Bomb's dome and its overlay
//! (`nbomb_render`, `nbomb_render_overlay`), a sentry's muzzle flash
//! (`model_render_node_chr_gunfire`), and in x-ray the BG and the props in
//! their eraser colours ([`crate::xray`]); from the state `pd_sim::fx` and
//! `pd_sim::props` tick, and the target boards. Each batch names its texture
//! and combiner ([`FxKind`]); `fx.wgsl` draws them.
//!
//! Positions are world centimetres. The world pass draws them depth-tested
//! against the BG; the beams go in the gun pass (`bgun_render` draws them first,
//! `bondgun.c:8235`).
//!
//! Source: the old repo's `pd_guns/fx.rs`, `smoke.rs`, `explosions.rs` (their
//! `geometry` functions), `sim.rs` `world_fx`/`gun_fx`, and `render.rs`'s effect
//! pipelines.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_sim::fx::{Beam, Smokes, Sparks, Wallhit};
use pd_sim::player::vision::Eraser;
use pd_sim::props::explosions::{etype, texture_pair, Explosion, ExplosionPart, Explosions};
use pd_sim::props::nbomb::{nbomb_calculate_alpha, Nbombs};
use pd_sim::world::Board;

use crate::xray;
use wgpu::util::DeviceExt;

/// The effect shader's WGSL (validated by the tests).
pub const FX_WGSL: &str = include_str!("fx.wgsl");

/// A textured, vertex-coloured vertex in world centimetres.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct FxVert {
    pub pos: [f32; 3],
    /// Texels (PD's `s,t / 32`), divided by the texture's size in the shader.
    pub st: [f32; 2],
    /// 0..1.
    pub col: [f32; 4],
}

fn fv(pos: Vec3, st: [f32; 2], col: [f32; 4]) -> FxVert {
    FxVert { pos: pos.to_array(), st, col }
}

/// Which texture and combiner a batch uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxKind {
    /// `G_CC_BLENDIA` (beams): colour = lerp(shade, env, texel), alpha = texel·shade.
    Beam(u16),
    /// `G_CC_CUSTOM_04` (sparks): colour = shade, alpha = texel·shade.
    Spark,
    /// Bullet holes: IA texel × shade, decal z.
    Wallhit(u16),
    /// Untextured vertex colour, opaque (the target boards).
    Flat,
    /// `g_TcGdl1` smoke: IA texel 0x002a × shade (`G_CC_MODULATEIA`), wrapped.
    Smoke,
    /// `g_TcGdl2` explosion flare frame `i`: flame × colour map
    /// (`G_CC_INTERFERENCE`) × shade (`G_CC_MODULATEIA2`), clamped.
    Explosion(u8),
    /// The N-Bomb's dome and overlay: `TEX_GENERAL_NBOMBDOME` (IA8, wrapped) ×
    /// shade (`G_CC_MODULATEIA`), `G_RM_ZB_XLU_SURF`, no cull.
    Nbomb,
    /// A `CHRGUNFIRE` billboard's texture × shade, clamped.
    GunFire(u16),
    /// The x-ray's BG: `G_CC_SHADE`, `G_RM_AA_XLU_SURF`, no z, no cull.
    XrayBg,
    /// A prop in x-ray drawn as flat colour (the boards), no z.
    Xray,
}

/// `g_TcGeneralConfigs[TEX_GENERAL_NBOMBDOME]` (TEXTURE_063B).
pub const TEX_NBOMBDOME: u16 = 0x063b;

impl FxKind {
    /// The shader's combiner mode.
    fn mode(self) -> u32 {
        match self {
            FxKind::Beam(_) => 0,
            FxKind::Spark => 1,
            FxKind::Wallhit(_) | FxKind::Smoke | FxKind::Nbomb | FxKind::GunFire(_) => 2,
            FxKind::Flat | FxKind::XrayBg | FxKind::Xray => 3,
            FxKind::Explosion(_) => 4,
        }
    }

    /// (texture, second texture, clamp).
    fn textures(self) -> (Option<u16>, Option<u16>, bool) {
        match self {
            FxKind::Beam(t) => (Some(t), None, false),
            FxKind::Spark => (Some(TEX_SPARK), None, true),
            FxKind::Wallhit(t) => (Some(t), None, true),
            FxKind::Flat | FxKind::XrayBg | FxKind::Xray => (None, None, false),
            FxKind::Smoke => (Some(TEX_SMOKE), None, false),
            FxKind::Nbomb => (Some(TEX_NBOMBDOME), None, false),
            FxKind::GunFire(t) => (Some(t), None, true),
            FxKind::Explosion(i) => {
                let (a, b) = texture_pair(i as usize);
                (Some(a), Some(b), true)
            }
        }
    }

    fn pipe(self) -> FxPipe {
        match self {
            FxKind::Flat => FxPipe::Opaque,
            FxKind::Wallhit(_) => FxPipe::Decal,
            FxKind::XrayBg | FxKind::Xray => FxPipe::NoZ,
            _ => FxPipe::Xlu,
        }
    }

    /// Drawn before the world's objects (the opaque boards; the x-ray's BG).
    pub fn before_objects(self) -> bool {
        matches!(self, FxKind::Flat | FxKind::XrayBg)
    }
}

pub struct FxBatch {
    pub kind: FxKind,
    pub verts: Vec<FxVert>,
}

// ── beams (gunfx.c) ─────────────────────────────────────────────────────────

/// `g_TexBeamConfigs` and `g_TexLaserConfigs` texture numbers.
pub const TEX_BEAM: [u16; 5] = [0x0006, 0x0007, 0x0008, 0x0859, 0x085a];
const TEX_BEAM_ORANGE: usize = 0;
const TEX_BEAM_BLUE: usize = 1;
const TEX_BEAM_YELLOW: usize = 3;
const TEX_BEAM_GREEN: usize = 4;
pub const TEX_LASER: u16 = 0x0009;
/// `g_TexSparkConfigs[0]`, `g_TexGeneralConfigs[1]` (smoke).
pub const TEX_SPARK: u16 = 0x001a;
pub const TEX_SMOKE: u16 = 0x002a;

/// `beam_render` (`gunfx.c:298`): a quad from the beam's head back `mindist`,
/// facing the camera at `campos`.
pub fn beam_geometry(beam: &Beam, campos: Vec3) -> Option<FxBatch> {
    if beam.age < 0 {
        return None;
    }
    let w = beam.weaponnum;
    let is = |x: u8| w == x as i32;
    let texidx = if is(WEAPON_CYCLONE) {
        TEX_BEAM_BLUE
    } else if is(WEAPON_TRANQUILIZER) {
        TEX_BEAM_YELLOW
    } else if is(WEAPON_MAULER) || is(WEAPON_PHOENIX) || is(WEAPON_CALLISTO) || is(WEAPON_REAPER) || is(WEAPON_FARSIGHT) || w <= -3 {
        TEX_BEAM_GREEN
    } else {
        TEX_BEAM_ORANGE
    };
    let alpha = if w == pd_sim::fx::beam::BEAM_STATIC || is(WEAPON_CYCLONE) { 127.0 / 255.0 } else { 1.0 };
    let mut halfwidth = if is(WEAPON_LASER) {
        50.0
    } else if w == pd_sim::fx::beam::BEAM_LASERSTREAM {
        10.0
    } else {
        30.0
    };
    if w <= -3 {
        halfwidth *= (w + 3) as f32 * 2.0 + 1.0;
    }
    let tex = if is(WEAPON_LASER) || w == pd_sim::fx::beam::BEAM_LASERSTREAM { TEX_LASER } else { TEX_BEAM[texidx] };
    let mut len = beam.mindist;
    let mut dist = beam.dist;
    let mut head = beam.from;
    if dist > 0.0 {
        head += beam.dir * dist;
    } else {
        len += dist;
        dist = 0.0;
    }
    if dist + len > beam.maxdist {
        len = beam.maxdist - dist;
    }
    if len <= 0.0 {
        return None;
    }
    let tail = head + beam.dir * len;
    let side = beam.dir.cross(campos - tail);
    let side = if side != Vec3::ZERO { side.normalize() * halfwidth } else { Vec3::new(0.0, halfwidth, 0.0) };
    // The quad's vertices are in 0.1-unit model space (mtx00015f04(0.1)).
    let s = side * 0.1;
    let (tw, th) = (16.0, 32.0);
    let c = [1.0, 1.0, 1.0, alpha];
    let v = [fv(head + s, [tw, 0.0], c), fv(head - s, [0.0, 0.0], c), fv(tail + s * 0.9, [tw, th], c), fv(tail - s * 0.9, [0.0, th], c)];
    // gSPTri2(0, 2, 3, 0, 3, 1)
    Some(FxBatch { kind: FxKind::Beam(tex), verts: vec![v[0], v[2], v[3], v[0], v[3], v[1]] })
}

/// `beam_render_generic` (`gunfx.c:168`) with `arg2` 1: a quad `halfwidth`
/// either side of head → tail, turned to the camera, `TEX_LASER_00` across it,
/// the head's colour at the head and the tail's at the tail. Nothing when
/// either end is over 100 m from the eye on any axis.
fn beam_render_generic(head: Vec3, headcol: u32, halfwidth: f32, tail: Vec3, tailcol: u32, cam: &FxCam) -> Option<FxBatch> {
    let d = tail - head;
    let length = d.length();
    if length < 0.00001 {
        return None;
    }
    let dir = d / length;
    for p in [head, tail] {
        let e = cam.world_to_screen.transform_point3(p);
        if e.abs().max_element() > 10000.0 {
            return None;
        }
    }
    let side = dir.cross(cam.pos - (head + dir * length));
    let side = if side != Vec3::ZERO { side.normalize() } else { Vec3::Y } * halfwidth;
    let (hc, tc) = (rgba(headcol), rgba(tailcol));
    let (tw, th) = (16.0, 32.0);
    let v = [fv(head + side, [0.0, 0.0], hc), fv(head - side, [tw, 0.0], hc), fv(tail - side, [tw, th], tc), fv(tail + side, [0.0, th], tc)];
    // gSPTri2(0, 1, 2, 2, 3, 0)
    Some(FxBatch { kind: FxKind::Beam(TEX_LASER), verts: vec![v[0], v[1], v[2], v[2], v[3], v[0]] })
}

/// `boltbeams_render` (`gunfx.c:977`): each crossbow bolt's trail, clear at
/// the head (where the bolt left) to half-alpha pale blue at the bolt.
pub fn boltbeam_geometry(beams: &pd_sim::fx::boltbeam::BoltBeams, cam: &FxCam) -> Vec<FxBatch> {
    beams.live().filter_map(|b| beam_render_generic(b.headpos, 0xafafff00, 2.0, b.tailpos, 0xafafff7f, cam)).collect()
}

// ── sparks (sparks.c) ───────────────────────────────────────────────────────

fn rgba(word: u32) -> [f32; 4] {
    [((word >> 24) & 0xff) as f32 / 255.0, ((word >> 16) & 0xff) as f32 / 255.0, ((word >> 8) & 0xff) as f32 / 255.0, (word & 0xff) as f32 / 255.0]
}

/// `sparks_render` (`sparks.c:273`): one stretched triangle per live spark; in
/// x-ray in the eraser's colours, none beyond its reach.
pub fn sparks_geometry(sparks: &Sparks, campos: Vec3, camlook: Vec3, fovy: f32, xray: Option<&Eraser>) -> Option<FxBatch> {
    let look = camlook.abs();
    let axis = if look.y > look.x {
        if look.z > look.y {
            2
        } else {
            1
        }
    } else if look.z > look.x {
        2
    } else {
        0
    };
    let mut verts = Vec::new();
    for grp in sparks.groups.iter().flatten() {
        let ty = &grp.ty;
        let dist = (campos - grp.pos).length();
        if dist > 20000.0 {
            continue;
        }
        let (mut c0, mut c1) = (rgba(ty.col0), rgba(ty.col1));
        if ty.unk12 < ty.maxage && (ty.unk12 as i32) < grp.age {
            let diff1 = (ty.maxage - ty.unk12) as f32;
            let diff2 = (grp.age - ty.unk12 as i32) as f32;
            let frac = (diff1 - diff2) / diff1;
            c0[3] *= frac;
            c1[3] *= frac;
        }
        if let Some(e) = xray {
            // Both colours from unk1c's alpha (PD's @bug).
            match xray::spark_colour(e, grp.pos, c0[3]) {
                Some(c) => {
                    c0 = c;
                    c1 = c;
                }
                None => continue,
            }
        }
        let sp120 = dist * 0.2 * (fovy / 60.0);
        let widen = ty.unk06 as f32 + grp.age as f32 * ty.unk0a as f32 + (sp120 as i32) as f32;
        let n = sparks.sparks.len();
        let mut idx = grp.startindex;
        for _ in 0..grp.numsparks {
            let s = &sparks.sparks[idx];
            idx = (idx + 1) % n;
            if s.ttl == 0 {
                continue;
            }
            let sl = s.speed.length();
            let f2 = (sp120 + (ty.unk04 as f32 + grp.age as f32 * ty.unk08 as f32)) / sl;
            let p0 = s.pos;
            let mut p1 = s.pos + s.speed * f2;
            let mut p2 = p1;
            let sp = s.speed.abs();
            let a = match axis {
                0 => {
                    if sp.z > sp.y {
                        1
                    } else {
                        2
                    }
                }
                1 => {
                    if sp.x > sp.z {
                        2
                    } else {
                        0
                    }
                }
                _ => {
                    if sp.x > sp.y {
                        1
                    } else {
                        0
                    }
                }
            };
            p1[a] -= widen;
            p2[a] += widen;
            // Spark-local units are scaled by 0.05 around the group (spd4).
            let w = |p: Vec3| grp.pos + p * 0.05;
            verts.push(fv(w(p0), [4.0, -8.0], c1));
            verts.push(fv(w(p1), [-1.0, 15.5], c0));
            verts.push(fv(w(p2), [9.0, 15.5], c0));
        }
    }
    (!verts.is_empty()).then_some(FxBatch { kind: FxKind::Spark, verts })
}

// ── wallhits (wallhit.c) ────────────────────────────────────────────────────

/// `g_TcWallhitConfigs[]` by `WALLHITTEX_*`: texture number and texel size.
pub const WALLHIT_TEX: [(u16, f32, f32); 18] = [
    (0x0003, 48.0, 48.0),
    (0x0c27, 64.0, 64.0),
    (0x0da5, 64.0, 48.0),
    (0x0003, 48.0, 48.0),
    (0x0003, 48.0, 48.0),
    (0x0003, 48.0, 48.0),
    (0x0004, 32.0, 32.0),
    (0x0005, 54.0, 54.0),
    (0x0c28, 64.0, 64.0),
    (0x0854, 48.0, 48.0),
    (0x0855, 48.0, 48.0),
    (0x0856, 48.0, 48.0),
    (0x08f0, 24.0, 24.0),
    (0x0b53, 64.0, 64.0),
    (0x0b53, 64.0, 64.0),
    (0x0b53, 64.0, 64.0),
    (0x0d74, 32.0, 24.0),
    (0x0d72, 32.0, 24.0),
];

/// `wallhit_render_bg_hits`' quad for one hole.
pub fn wallhit_tris(wh: &Wallhit, out: &mut Vec<FxVert>) {
    let (_, tw, th) = WALLHIT_TEX[wh.texnum];
    let st = [[0.0, th], [0.0, 0.0], [tw, 0.0], [tw, th]];
    let v = |i: usize| fv(wh.corners[i], st[i], wh.cols[i]);
    out.extend_from_slice(&[v(0), v(1), v(2), v(0), v(2), v(3)]);
}

// ── smoke (smoke.c) ─────────────────────────────────────────────────────────

/// `smoke_render` + `smoke_render_part` (`smoke.c:574`, `:66`): one batch per
/// smoke, with its prop position for sorting. `right`/`up` are the camera's
/// world axes (`cam_get_projection_mtxf()` columns 0 and 1); `brightness` is
/// `room_get_final_brightness_for_player` (0..255).
pub fn smoke_geometry(smokes: &Smokes, campos: Vec3, right: Vec3, up: Vec3, brightness: f32, xray: Option<&Eraser>) -> Vec<(Vec3, FxBatch)> {
    let mut out = Vec::new();
    for smoke in smokes.slots.iter().flatten() {
        let t = pd_sim::fx::smoke::smoke_type(smoke.ty);
        let mut verts = Vec::new();
        for part in smoke.parts.iter().filter(|p| p.size > 0.0) {
            let alpha: u8 = if t.fadespeed as f32 >= part.count as f32 { (part.alpha / t.fadespeed as f32 * part.count as f32) as u8 } else { part.alpha as u8 };
            let mut c78 = part.rot.cos() * part.size;
            let mut s74 = part.rot.sin() * part.size;
            let p = Vec3::new(part.pos.x + 7.0 * part.offset1.sin() * t.unk20, part.pos.y, part.pos.z + 7.0 * part.offset2.sin() * t.unk20);
            let d = p - campos;
            let distance = d.length();
            if distance > 30000.0 {
                continue;
            }
            // Pulled up to 1 m towards the camera (scaled to keep its apparent
            // size) so it doesn't cut into the wall it sits on.
            let range = (distance * 0.5).min(100.0);
            let mult = if distance == 0.0 { 0.0 } else { (distance - range) / distance };
            c78 *= mult;
            s74 *= mult;
            let c = campos + d * mult;
            let spa0 = right * c78;
            let sp94 = right * s74;
            let sp88 = up * c78;
            let sp7c = up * s74;
            // SMOKETYPE_PINBALL ignores the room light.
            let frac = if smoke.ty != SMOKETYPE_PINBALL { (brightness / 255.0).min(1.0) } else { 1.0 };
            let col = match xray {
                Some(e) => match xray::smoke_colour(e, part.pos, alpha as f32) {
                    Some(c) => c,
                    None => continue,
                },
                None => [
                    ((t.r as f32 * frac) as u32 & 0xff) as f32 / 255.0,
                    ((t.g as f32 * frac) as u32 & 0xff) as f32 / 255.0,
                    ((t.b as f32 * frac) as u32 & 0xff) as f32 / 255.0,
                    alpha as f32 / 255.0,
                ],
            };
            // s,t 1760 = 55 texels (the 56-texel tile's last texel edge).
            let v = [fv(c - spa0 - sp7c, [55.0, 0.0], col), fv(c + sp94 - sp88, [0.0, 0.0], col), fv(c + spa0 + sp7c, [0.0, 55.0], col), fv(c - sp94 + sp88, [55.0, 55.0], col)];
            // gSPTri2(0, 1, 2, 0, 2, 3)
            verts.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
        }
        if !verts.is_empty() {
            out.push((smoke.pos, FxBatch { kind: FxKind::Smoke, verts }));
        }
    }
    out
}

// ── explosions (explosions.c) ───────────────────────────────────────────────

/// `explosion_render` (`explosions.c:1226`): per explosion, texture pairs 14 →
/// 0, each drawing the parts on that animation frame.
pub fn explosion_geometry(explosions: &Explosions, right: Vec3, up: Vec3, xray: Option<&Eraser>) -> Vec<(Vec3, Vec<FxBatch>)> {
    let mut out = Vec::new();
    for exp in explosions.slots.iter().flatten() {
        let t = etype(exp.ty);
        // In x-ray every part takes the explosion's colour, or none draws.
        let col = match xray {
            Some(e) => match xray::explosion_colour(e, exp.pos) {
                Some(c) => c,
                None => continue,
            },
            None => [1.0; 4],
        };
        let mut batches = Vec::new();
        for i in (0..15).rev() {
            let mut verts = Vec::new();
            for part in exp.parts.iter() {
                if part.frame > 0 && i == ((part.frame - 1) as f32 / t.flarespeed) as i32 {
                    explosion_render_part(exp, part, i, right, up, col, &mut verts);
                }
            }
            if !verts.is_empty() {
                batches.push(FxBatch { kind: FxKind::Explosion(i as u8), verts });
            }
        }
        if !batches.is_empty() {
            out.push((exp.pos, batches));
        }
    }
    out
}

/// `explosion_render_part` (`explosions.c:1319`): a camera-facing quad, kept
/// inside the explosion's bounding boxes.
fn explosion_render_part(exp: &Explosion, part: &ExplosionPart, arg4: i32, right: Vec3, up: Vec3, col: [f32; 4], out: &mut Vec<FxVert>) {
    let mut size = part.size;
    let mut pos = part.pos;
    if !exp.bbs.is_empty() {
        let mut bbnum = part.bb.min(exp.bbs.len() - 1);
        let mut max = 0.0;
        for (i, bb) in exp.bbs.iter().enumerate() {
            if pos.cmpge(bb.bbmin).all() && pos.cmple(bb.bbmax).all() {
                let mut min = 65536.0f32;
                for j in 0..3 {
                    min = min.min(pos[j] - bb.bbmin[j]).min(bb.bbmax[j] - pos[j]);
                }
                if min > max {
                    max = min;
                    bbnum = i;
                }
            }
        }
        let bb = exp.bbs[bbnum];
        let mut size2 = size * 0.7;
        for i in 0..3 {
            if (bb.bbmax[i] - bb.bbmin[i]) * 0.8 < size2 {
                size = (bb.bbmax[i] - bb.bbmin[i]) * 0.8 / 0.7;
                if part.size * 0.65 > size {
                    size = part.size * 0.65;
                }
                size2 = size * 0.7;
            }
        }
        size2 *= match arg4 {
            1 => 0.589_285_73,
            2 => 0.696_428_6,
            3 => 0.821_428_6,
            4 => 0.964_285_73,
            _ => 1.0,
        };
        for i in 0..3 {
            if bb.bbmax[i] - bb.bbmin[i] < size2 {
                pos[i] = (bb.bbmax[i] + bb.bbmin[i]) * 0.5;
            } else {
                let value = bb.bbmin[i] + size2 - pos[i];
                if value > 0.0 {
                    pos[i] += value;
                } else {
                    let value = pos[i] - (bb.bbmax[i] - size2);
                    if value > 0.0 {
                        pos[i] -= value;
                    }
                }
            }
        }
    }
    let cosine = part.rot.cos() * size;
    let sine = part.rot.sin() * size;
    let spbc = right * cosine;
    let spb0 = right * sine;
    let spa4 = up * cosine;
    let sp98 = up * sine;
    let v = [fv(pos - spbc - sp98, [55.0, 0.0], col), fv(pos + spb0 - spa4, [0.0, 0.0], col), fv(pos + spbc + sp98, [0.0, 55.0], col), fv(pos - spb0 + spa4, [55.0, 55.0], col)];
    out.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
}

// ── the N-Bomb (nbomb.c) ────────────────────────────────────────────────────

/// `nbomb_create_gdl` + `nbomb_render` (`nbomb.c:311`, `:364`): per storm, a
/// black geodesic dome (an octahedron split twice) of its radius, spun by
/// `unk14`, its texture scrolled by `g_20SecIntervalFrac`. `// SUBST:` PD
/// builds the display list before `nbomb_render` sets its 2000-unit scale, so
/// a storm's very first frame draws a tiny dome / every frame is full size.
pub fn nbomb_geometry(nbombs: &Nbombs, frac20: f32) -> Vec<(Vec3, FxBatch)> {
    let mut out = Vec::new();
    let cb00 = ((frac20 * 64.0 * 32.0 * 16.0) as i32 % 0x800) as f32;
    for n in nbombs.bombs.iter().filter(|n| n.age240 >= 0) {
        let alpha = nbomb_calculate_alpha(n) as f32 / 255.0;
        let mut mtx = pd_core::math::load_rotation(Vec3::new(0.0, n.unk14 as f32 / 2048.0 * pd_core::math::dtor(360.0), 0.0));
        pd_core::math::scale3(&mut mtx, n.radius / 2000.0);
        let mtx = Mat4::from_translation(n.pos) * mtx;
        let mut verts = Vec::new();
        geodesic_dome(2, 2000.0, cb00, &mut |v: Vec3, st: [f32; 2]| verts.push(fv(mtx.transform_point3(v), st, [0.0, 0.0, 0.0, alpha])));
        out.push((n.pos, FxBatch { kind: FxKind::Nbomb, verts }));
    }
    out
}

/// `func0f008558` + `func0f006c80` (`nbomb.c`): the octahedron's eight faces,
/// each split `depth` times into four through the edge midpoints pushed out to
/// the sphere. `MAKEVERTEX` gives each vertex s = y·256 and t = the angle
/// around·256 texels plus the scroll; the second half's seam vertex (t = 0)
/// moves to t = 256 (`var8009cb04`).
fn geodesic_dome(depth: i32, scale: f32, cb00: f32, emit: &mut dyn FnMut(Vec3, [f32; 2])) {
    let c = [Vec3::Z, Vec3::X, -Vec3::Z, -Vec3::X, Vec3::Y, -Vec3::Y];
    let faces_a = [(0, 4, 1), (1, 4, 2), (1, 5, 0), (2, 5, 1)];
    let faces_b = [(2, 4, 3), (3, 4, 0), (3, 5, 2), (0, 5, 3)];
    for (half, faces) in [(false, faces_a), (true, faces_b)] {
        let vert = |v: Vec3| -> (Vec3, [f32; 2]) {
            let s = (v.y * 256.0 * 32.0) as i16 as f32;
            let mut t = (pd_core::math::atan2f(v.x, v.z) / pd_core::math::dtor(360.0) * 256.0 * 32.0) as i16 as i32;
            if half && t == 0 {
                t = 256 * 32;
            }
            let t = (t + cb00 as i32) as i16 as f32;
            (v * scale, [s / 32.0, t / 32.0])
        };
        for (a, b, cc) in faces {
            dome_split(c[a], c[b], c[cc], depth, &vert, emit);
        }
    }
}

fn dome_split(a: Vec3, b: Vec3, c: Vec3, depth: i32, vert: &dyn Fn(Vec3) -> (Vec3, [f32; 2]), emit: &mut dyn FnMut(Vec3, [f32; 2])) {
    let ab = (a + b).normalize();
    let bc = (b + c).normalize();
    let ca = (c + a).normalize();
    if depth == 0 {
        // gSPTri4(a, ab, ca,  b, bc, ab,  c, ca, bc,  ab, bc, ca)
        for (p, q, r) in [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)] {
            for v in [p, q, r] {
                let (pos, st) = vert(v);
                emit(pos, st);
            }
        }
    } else {
        dome_split(a, ab, ca, depth - 1, vert, emit);
        dome_split(b, bc, ab, depth - 1, vert, emit);
        dome_split(c, ca, bc, depth - 1, vert, emit);
        dome_split(ab, bc, ca, depth - 1, vert, emit);
    }
}

/// `nbomb_render_overlay` (`nbomb.c:784`): standing inside a storm, its texture
/// over the whole view in clip space, black at the strongest storm's alpha; s
/// spans 5 texels across the view, t 30 down, both scrolling.
pub fn nbomb_overlay(nbombs: &Nbombs, campos: Vec3, frac20: f32) -> Option<FxBatch> {
    let finalalpha = nbombs.overlay_alpha(campos)?;
    let s = ((8.0 * frac20 * 128.0 * 32.0) as i32 % 2048) as f32 / 32.0;
    let t = (((campos.y * 8.0) as i32 % 2048) as i16 as i32 + (2.0 * frac20 * 128.0 * 32.0) as i16 as i32) as f32 / 32.0;
    let col = [0.0, 0.0, 0.0, finalalpha as f32 / 255.0];
    let v = [fv(Vec3::new(-1.0, 1.0, 0.5), [s, t], col), fv(Vec3::new(1.0, 1.0, 0.5), [s + 5.0, t], col), fv(Vec3::new(1.0, -1.0, 0.5), [s + 5.0, t + 30.0], col), fv(Vec3::new(-1.0, -1.0, 0.5), [s, t + 30.0], col)];
    // gSPTri2(0, 1, 2, 2, 3, 0)
    Some(FxBatch { kind: FxKind::Nbomb, verts: vec![v[0], v[1], v[2], v[2], v[3], v[0]] })
}

// ── a sentry's flash (model.c) ──────────────────────────────────────────────

/// `model_render_node_chr_gunfire` (`model.c:3368`): the flash billboard turned
/// to face the camera, 0.75..1.25 × `dim`, its texture square spun at random.
/// `// SUBST:` PD's scale and spin come from `random()`, the one stream / from
/// a hash of `seed`, so drawing never draws from the world's stream.
pub fn gunfire_geometry(def: &pd_core::model::ModelDef, mats: &[Mat4], node: usize, campos: Vec3, seed: u32) -> Option<FxBatch> {
    let pd_core::model::NodeKind::ChrGunfire { pos, dim, texture, texture_size } = def.nodes.get(node)?.kind else { return None };
    let mi = def.find_node_mtx_index(def.nodes[node].parent?, 0)?;
    let w = mats.get(mi)?;
    let p = w.transform_point3(pos);
    let scale_m = w.x_axis.truncate().length();
    let mut e = campos - p;
    let distance = e.length();
    e = if distance > 0.0 { e / (scale_m * distance) } else { Vec3::new(0.0, 0.0, 1.0 / scale_m) };
    let col = |i: usize| w.col(i).truncate();
    let spec = e.dot(col(1)).clamp(-1.0, 1.0).acos();
    let mut spf0 = (-(e.dot(col(2))) / spec.sin()).clamp(-1.0, 1.0).acos();
    if -(e.dot(col(0))) < 0.0 {
        spf0 = pd_core::math::baddtor(360.0) - spf0;
    }
    let (spdc, spd8) = (spf0.cos(), spf0.sin());
    let (rot2, spd0) = (spec.cos(), spec.sin());
    let h = |k: u32| {
        let x = seed.wrapping_mul(0x9e37_79b9).wrapping_add(k.wrapping_mul(0x85eb_ca6b));
        (x ^ (x >> 15)).wrapping_mul(0x2c1b_3c6d) ^ (x >> 13)
    };
    let scale = 0.75 + (h(1) % 128) as f32 / 256.0;
    let d = dim * scale;
    let spcc = d.x * spdc * 0.5;
    let spc8 = d.z * spd8 * 0.5;
    let spc4 = d.y * spd0 * 0.5;
    let spc0 = d.x * rot2 * spd8 * 0.5;
    let spbc = d.z * rot2 * spdc * 0.5;
    let sp90 = Vec3::new(pos.x - d.x * 0.5, pos.y, pos.z);
    let v = [
        Vec3::new(sp90.x - spcc - spc0, sp90.y - spc4, sp90.z + spc8 - spbc),
        Vec3::new(sp90.x - spcc + spc0, sp90.y + spc4, sp90.z + spc8 + spbc),
        Vec3::new(sp90.x + spcc + spc0, sp90.y + spc4, sp90.z - spc8 + spbc),
        Vec3::new(sp90.x + spcc - spc0, sp90.y - spc4, sp90.z - spc8 - spbc),
    ];
    // The texture square turned by a random angle; its half-diagonal
    // (0.707 · width texels) keeps the square inside the quad.
    let ang = ((h(2) as u16) as f32) / 65536.0 * std::f32::consts::TAU;
    let r = texture_size[0] * 181.0 / 256.0;
    let (c, sn) = (ang.cos() * r, ang.sin() * r);
    let centre = texture_size[0] * 0.5;
    let st = [[centre - c, centre - sn], [centre + sn, centre - c], [centre + c, centre + sn], [centre - sn, centre + c]];
    let fv4 = |i: usize| fv(w.transform_point3(v[i]), st[i], [1.0; 4]);
    // gSPTri2(0, 1, 2, 2, 3, 0)
    Some(FxBatch { kind: FxKind::GunFire(texture? as u16), verts: vec![fv4(0), fv4(1), fv4(2), fv4(2), fv4(3), fv4(0)] })
}

// ── the target boards ───────────────────────────────────────────────────────

fn quad(out: &mut Vec<FxVert>, p: [Vec3; 4], col: [f32; 4]) {
    let v = |i: usize| fv(p[i], [0.0, 0.0], col);
    out.extend_from_slice(&[v(0), v(1), v(2), v(0), v(2), v(3)]);
}

fn disc(out: &mut Vec<FxVert>, c: Vec3, rx: f32, ry: f32, col: [f32; 4]) {
    let n = 20;
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
        let p0 = c + Vec3::new(a0.cos() * rx, a0.sin() * ry, 0.0);
        let p1 = c + Vec3::new(a1.cos() * rx, a1.sin() * ry, 0.0);
        out.extend_from_slice(&[fv(c, [0.0; 2], col), fv(p1, [0.0; 2], col), fv(p0, [0.0; 2], col)]);
    }
}

fn box_tris(out: &mut Vec<FxVert>, mn: Vec3, mx: Vec3, col: [f32; 4]) {
    let c = Vec3::new;
    let faces = [
        [c(mn.x, mn.y, mn.z), c(mn.x, mx.y, mn.z), c(mx.x, mx.y, mn.z), c(mx.x, mn.y, mn.z)],
        [c(mn.x, mn.y, mx.z), c(mx.x, mn.y, mx.z), c(mx.x, mx.y, mx.z), c(mn.x, mx.y, mx.z)],
        [c(mn.x, mn.y, mn.z), c(mn.x, mn.y, mx.z), c(mn.x, mx.y, mx.z), c(mn.x, mx.y, mn.z)],
        [c(mx.x, mn.y, mn.z), c(mx.x, mx.y, mn.z), c(mx.x, mx.y, mx.z), c(mx.x, mn.y, mx.z)],
        [c(mn.x, mx.y, mn.z), c(mn.x, mx.y, mx.z), c(mx.x, mx.y, mx.z), c(mx.x, mx.y, mn.z)],
        [c(mn.x, mn.y, mn.z), c(mx.x, mn.y, mn.z), c(mx.x, mn.y, mx.z), c(mn.x, mn.y, mx.z)],
    ];
    for f in faces {
        quad(out, f, col);
    }
}

/// The firing range's boards: a white face with a bullseye, flashing red on a
/// hit, on a dark frame. They are not PD objects (props arrive with M8).
pub fn board_geometry(boards: &[Board]) -> Option<FxBatch> {
    let mut flat = Vec::new();
    for t in boards {
        let f = t.flash;
        let face = [1.0, 1.0 - 0.6 * f, 1.0 - 0.6 * f, 1.0];
        box_tris(&mut flat, t.min, t.max, [0.25, 0.22, 0.2, 1.0]);
        let c = t.face - Vec3::Z * 0.2;
        let (hx, hy) = (t.half.x * 0.85, t.half.y * 0.85);
        quad(&mut flat, [c + Vec3::new(-hx, -hy, 0.0), c + Vec3::new(hx, -hy, 0.0), c + Vec3::new(hx, hy, 0.0), c + Vec3::new(-hx, hy, 0.0)], face);
        for (r, col) in [(0.55, [0.1, 0.1, 0.1, 1.0]), (0.35, face), (0.18, [0.8, 0.1, 0.1, 1.0])] {
            let rr = hx * r * 1.4;
            disc(&mut flat, c - Vec3::Z * 0.1 * (2.0 - r), rr.min(hx), rr.min(hy), col);
        }
    }
    (!flat.is_empty()).then_some(FxBatch { kind: FxKind::Flat, verts: flat })
}

/// A test stage's polygons (`pd_sim::stage::fixtures`), which have no textured
/// BG: floors mid grey, other faces shaded by which way they face.
pub fn fixture_geometry(geom: &pd_sim::stage::LevelGeom) -> Option<FxBatch> {
    let mut out = Vec::new();
    for p in &geom.polys {
        let n = p.normal.abs();
        let v = if p.floor { 0.34 } else if n.y > 0.7 { 0.26 } else { 0.22 + 0.16 * n.x + 0.08 * n.z };
        let col = [v, v * 0.97, v * 0.92, 1.0];
        for i in 1..p.verts.len().saturating_sub(1) {
            out.extend_from_slice(&[fv(p.verts[0], [0.0; 2], col), fv(p.verts[i], [0.0; 2], col), fv(p.verts[i + 1], [0.0; 2], col)]);
        }
    }
    (!out.is_empty()).then_some(FxBatch { kind: FxKind::Flat, verts: out })
}

// ── one frame's effects ─────────────────────────────────────────────────────

/// What a view's effects need from the camera.
#[derive(Clone, Copy, Debug)]
pub struct FxCam {
    pub pos: Vec3,
    pub look: Vec3,
    pub fovy: f32,
    /// `cam_get_projection_mtxf()`: camera space → world.
    pub projection: Mat4,
    /// `cam_get_world_to_screen_mtxf()`.
    pub world_to_screen: Mat4,
    /// `room_get_final_brightness_for_player`.
    pub brightness: f32,
}

/// The world pass's effects in PD's order: the boards, the bullet holes, the
/// translucent props (smoke, explosions, the N-Bomb's domes) back to front by
/// `prop->z`, the sentries' tracers and flashes, the sparks, then the crossbow
/// bolts' trails. In x-ray
/// (`xray`): the BG's eraser triangles first, the props in their x-ray colours,
/// and no bullet holes.
pub fn world_fx(world: &pd_sim::world::World, cam: &FxCam, xray: Option<&Eraser>) -> Vec<FxBatch> {
    let mut out = Vec::new();
    if let Some(e) = xray {
        out.push(xray::bg_geometry(&world.stage.bghit, e));
        let mut boards = Vec::new();
        for b in &world.boards {
            if let Some(col) = xray::obj_colour(e, (b.min + b.max) * 0.5) {
                xray::box_geometry(b.min, b.max, cam.pos, col, &mut boards);
            }
        }
        out.push(FxBatch { kind: FxKind::Xray, verts: boards });
    } else {
        out.extend(board_geometry(&world.boards));
    }
    let mut by_tex: Vec<(u16, Vec<FxVert>)> = Vec::new();
    for wh in world.fx.wallhits.iter().filter(|_| xray.is_none()) {
        let tex = WALLHIT_TEX[wh.texnum].0;
        let i = match by_tex.iter().position(|(t, _)| *t == tex) {
            Some(i) => i,
            None => {
                by_tex.push((tex, Vec::new()));
                by_tex.len() - 1
            }
        };
        wallhit_tris(wh, &mut by_tex[i].1);
    }
    out.extend(by_tex.into_iter().map(|(tex, verts)| FxBatch { kind: FxKind::Wallhit(tex), verts }));
    // smoke_tick_player / explosion_tick_player's prop->z.
    let (right, up) = (cam.projection.x_axis.truncate(), cam.projection.y_axis.truncate());
    let propz = |pos: Vec3| {
        let z = -cam.world_to_screen.transform_point3(pos).z;
        if z < 100.0 {
            z * 0.5
        } else {
            z - 100.0
        }
    };
    let mut xlu: Vec<(f32, Vec<FxBatch>)> = Vec::new();
    for (pos, b) in smoke_geometry(&world.fx.smokes, cam.pos, right, up, cam.brightness, xray) {
        xlu.push((propz(pos), vec![b]));
    }
    for (pos, bs) in explosion_geometry(&world.explosions, right, up, xray) {
        xlu.push((propz(pos), bs));
    }
    xlu.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, bs) in xlu {
        out.extend(bs);
    }
    // props_render_beams (`propobj.c:11450`): the simulants' tracers
    // (`chr->fireslots[]`) and the sentries'. SUBST: another human's tracers
    // are their chr's fireslot beams in PD / theirs are drawn only in their
    // own gun pass until M6 poses a player's body.
    for c in world.chrs.iter().filter(|c| c.player.is_none()) {
        for slot in &c.fireslots {
            out.extend(beam_geometry(&slot.beam, cam.pos));
        }
    }
    for o in &world.props.objs {
        if let Some(a) = &o.autogun {
            out.extend(beam_geometry(&a.beam, cam.pos));
        }
    }
    // A simulant's muzzle flash: its held gun's CHRGUNFIRE node
    // (`model_render_node_chr_gunfire`), while `weapon_set_gunfire_visible`.
    if xray.is_none() {
        for (k, c) in world.chrs.iter().enumerate().filter(|(_, c)| c.player.is_none() && c.onanyscreen) {
            for (h, held) in c.held.iter().enumerate() {
                let Some(held) = held.as_ref().filter(|g| g.gunfire) else { continue };
                if let Some(node) = held.model.def.get_part(pd_core::ids::MODELPART_0000) {
                    let seed = (world.lv.lvframenum as u32) ^ (k as u32).wrapping_mul(7919) ^ h as u32;
                    out.extend(gunfire_geometry(&held.model.def, &held.model.matrices, node, cam.pos, seed));
                }
            }
        }
    }
    // A firing sentry's flash, part of its model (not in x-ray, where the model
    // is flat colour).
    if xray.is_none() {
        for o in &world.props.objs {
            let Some(a) = &o.autogun else { continue };
            if !(a.fireleft || a.fireright) {
                continue;
            }
            let mats = o.init_matrices();
            for (part, on) in [(MODELPART_AUTOGUN_FLASHLEFT, a.fireleft), (MODELPART_AUTOGUN_FLASHRIGHT, a.fireright)] {
                if let Some(node) = o.def.get_part(part).filter(|_| on) {
                    let seed = (world.lv.lvframenum as u32) ^ o.id.wrapping_mul(7919) ^ part as u32;
                    out.extend(gunfire_geometry(&o.def, &mats, node, cam.pos, seed));
                }
            }
        }
    }
    // nbombs_render (`lv.c:1313`), after the sparks in PD's order.
    out.extend(sparks_geometry(&world.fx.sparks, cam.pos, cam.look, cam.fovy, xray));
    for (_, b) in nbomb_geometry(&world.props.nbombs, world.frac20) {
        out.push(b);
    }
    // player_render_hud's boltbeams_render (`player.c:4466`), before the gun,
    // still in the world's projection and z.
    out.extend(boltbeam_geometry(&world.fx.boltbeams, cam));
    out
}

/// The gun pass's effects: this player's tracers (none in x-ray or riding a
/// rocket: `bgun_render` isn't reached).
pub fn gun_fx(player: &pd_sim::player::Player, campos: Vec3) -> Vec<FxBatch> {
    if player.visionmode == VISIONMODE_XRAY || player.cameramode == CAMERAMODE_THIRDPERSON {
        return Vec::new();
    }
    player.gun.hands.iter().filter(|h| h.visible).filter_map(|h| beam_geometry(&h.beam, campos)).collect()
}

// ── the GPU side ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FxPipe {
    /// Opaque, z write.
    Opaque,
    /// Translucent, z test, no write, pulled towards the camera.
    Decal,
    /// Translucent, z test, no write.
    Xlu,
    /// Translucent, no z at all (the x-ray).
    NoZ,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PassU {
    view_proj: [[f32; 4]; 4],
    env: [f32; 4],
}

/// Which pass a range belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxPass {
    World,
    Gun,
    /// Over the gun, in clip space (the N-Bomb's overlay).
    Overlay,
}

pub struct FxRange {
    pass: FxPass,
    kind: FxKind,
    start: u32,
    count: u32,
}

/// The effect pipelines, textures and this frame's vertices.
pub struct FxRenderer {
    layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    draw_bgl: wgpu::BindGroupLayout,
    pipes: HashMap<FxPipe, wgpu::RenderPipeline>,
    passes: [(wgpu::Buffer, wgpu::BindGroup); 3],
    binds: HashMap<FxKind, wgpu::BindGroup>,
    textures: HashMap<u16, (wgpu::TextureView, u32, u32)>,
    white: wgpu::TextureView,
    vbuf: Option<(wgpu::Buffer, u64)>,
    ranges: Vec<FxRange>,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, rgba: &[u8]) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pdfx-tex"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Raw values: the N64 has no gamma.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    tex.create_view(&Default::default())
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}

impl FxRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat, depth_format: wgpu::TextureFormat) -> FxRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pdfx"), source: wgpu::ShaderSource::Wgsl(FX_WGSL.into()) });
        let pass_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("pdfx-pass"), entries: &[uniform_entry(0)] });
        let draw_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pdfx-draw"),
            entries: &[
                uniform_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                tex_entry(3),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pdfx"), bind_group_layouts: &[&pass_bgl, &draw_bgl], push_constant_ranges: &[] });
        let pass = || {
            let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("pdfx-pass"), size: std::mem::size_of::<PassU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("pdfx-pass"), layout: &pass_bgl, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }] });
            (buf, bind)
        };
        let passes = [pass(), pass(), pass()];
        let white = upload(device, queue, 1, 1, &[255; 4]);
        FxRenderer {
            layout,
            shader,
            draw_bgl,
            pipes: HashMap::new(),
            passes,
            binds: HashMap::new(),
            textures: HashMap::new(),
            white,
            vbuf: None,
            ranges: Vec::new(),
            color_format,
            depth_format,
        }
    }

    fn pipe(&mut self, device: &wgpu::Device, key: FxPipe) {
        if self.pipes.contains_key(&key) {
            return;
        }
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];
        let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pdfx"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<FxVert>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.color_format,
                    blend: if key == FxPipe::Opaque { None } else { Some(wgpu::BlendState::ALPHA_BLENDING) },
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: self.depth_format,
                depth_write_enabled: key == FxPipe::Opaque,
                depth_compare: if key == FxPipe::NoZ { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::LessEqual },
                stencil: Default::default(),
                bias: if key == FxPipe::Decal { wgpu::DepthBiasState { constant: -8, slope_scale: -2.0, clamp: 0.0 } } else { Default::default() },
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        self.pipes.insert(key, p);
    }

    fn texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetDir, num: u16) -> (wgpu::TextureView, u32, u32) {
        if let Some(t) = self.textures.get(&num) {
            return t.clone();
        }
        let t = match assets.read_png(&assets.path(&format!("textures/{num:04x}.png"))) {
            Ok((w, h, rgba)) => (upload(device, queue, w as u32, h as u32, &rgba), w as u32, h as u32),
            Err(e) => {
                log::warn!("effect texture {num:04x}: {e}");
                (self.white.clone(), 1, 1)
            }
        };
        self.textures.insert(num, t.clone());
        t
    }

    fn bind(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetDir, kind: FxKind) {
        if self.binds.contains_key(&kind) {
            return;
        }
        let (t0, t1, clamp) = kind.textures();
        let (view, w, h) = match t0 {
            Some(t) => self.texture(device, queue, assets, t),
            None => (self.white.clone(), 1, 1),
        };
        let view2 = match t1 {
            Some(t) => self.texture(device, queue, assets, t).0,
            None => self.white.clone(),
        };
        let mode = if clamp { wgpu::AddressMode::ClampToEdge } else { wgpu::AddressMode::Repeat };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pdfx"),
            address_mode_u: mode,
            address_mode_v: mode,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let u: [f32; 4] = [w as f32, h as f32, kind.mode() as f32, 0.0];
        let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("pdfx-draw"), contents: bytemuck::cast_slice(&u), usage: wgpu::BufferUsages::UNIFORM });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pdfx-draw"),
            layout: &self.draw_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view2) },
            ],
        });
        self.binds.insert(kind, bind);
    }

    /// Upload this frame's batches. `world_vp`/`gun_vp` take world cm to clip
    /// for each pass; `env` is the beams' env colour (the gun shade colour).
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetDir, world: &[FxBatch], gun: &[FxBatch], overlay: &[FxBatch], world_vp: Mat4, gun_vp: Mat4, env: [f32; 4]) {
        let mut verts: Vec<FxVert> = Vec::new();
        self.ranges.clear();
        for (pass, batches) in [(FxPass::World, world), (FxPass::Gun, gun), (FxPass::Overlay, overlay)] {
            for b in batches {
                if b.verts.is_empty() {
                    continue;
                }
                let start = verts.len() as u32;
                verts.extend_from_slice(&b.verts);
                self.ranges.push(FxRange { pass, kind: b.kind, start, count: b.verts.len() as u32 });
                self.bind(device, queue, assets, b.kind);
                self.pipe(device, b.kind.pipe());
            }
        }
        let need = (verts.len().max(1) * std::mem::size_of::<FxVert>()) as u64;
        if self.vbuf.as_ref().is_none_or(|(_, cap)| *cap < need) {
            let cap = need.next_power_of_two();
            let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("pdfx-vb"), size: cap, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.vbuf = Some((buf, cap));
        }
        if !verts.is_empty() {
            queue.write_buffer(&self.vbuf.as_ref().unwrap().0, 0, bytemuck::cast_slice(&verts));
        }
        for (i, vp) in [world_vp, gun_vp, Mat4::IDENTITY].into_iter().enumerate() {
            queue.write_buffer(&self.passes[i].0, 0, bytemuck::bytes_of(&PassU { view_proj: vp.to_cols_array_2d(), env }));
        }
    }

    /// Draw one pass's batches, in order.
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, pass: FxPass) {
        self.draw_some(rp, pass, |_| true);
    }

    /// Draw the batches of one pass `which` keeps, in order.
    pub fn draw_some<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, pass: FxPass, which: impl Fn(FxKind) -> bool) {
        let Some((vb, _)) = &self.vbuf else { return };
        rp.set_vertex_buffer(0, vb.slice(..));
        rp.set_bind_group(0, &self.passes[pass as usize].1, &[]);
        for r in self.ranges.iter().filter(|r| r.pass == pass && which(r.kind)) {
            let (Some(pipe), Some(bind)) = (self.pipes.get(&r.kind.pipe()), self.binds.get(&r.kind)) else { continue };
            rp.set_pipeline(pipe);
            rp.set_bind_group(1, bind, &[]);
            rp.draw(r.start..r.start + r.count, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fx_shader_validates() {
        let module = naga::front::wgsl::parse_str(FX_WGSL).unwrap_or_else(|e| panic!("{}", e.emit_to_string(FX_WGSL)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
    }

    #[test]
    fn a_fresh_beam_draws_a_quad_along_the_shot() {
        let mut rng = pd_core::rng::Rng::new(1);
        let mut b = Beam::default();
        b.create(&mut rng, WEAPON_FALCON2 as i32, Vec3::ZERO, Vec3::new(0.0, 0.0, 2000.0));
        // Keep drawing until the head is past the muzzle.
        b.dist = 100.0;
        let g = beam_geometry(&b, Vec3::new(0.0, 50.0, -100.0)).expect("a live beam draws");
        assert_eq!(g.kind, FxKind::Beam(TEX_BEAM[TEX_BEAM_ORANGE]));
        assert_eq!(g.verts.len(), 6);
        assert!(g.verts.iter().all(|v| v.pos[2] >= 99.0 && v.pos[2] <= 2000.0));
    }
}
