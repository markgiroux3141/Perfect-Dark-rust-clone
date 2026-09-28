//! The model format, the merged walker (against the frozen spike walkers), the
//! part-box hit test and the CPU draw, on the exported assets.

use std::sync::{Arc, OnceLock};

use glam::{Mat4, Vec3};

use super::draw::{draw_model, DrawOpts, TextureCache};
use super::hit::{HitNode, HitPad};
use super::*;
use crate::anim::tests::{assets, bank};
use crate::anim::{update_chr_info, Anim, AnimCtx};

fn store() -> &'static ModelStore {
    static STORE: OnceLock<ModelStore> = OnceLock::new();
    STORE.get_or_init(|| ModelStore::load(&assets()).expect("assets/models: run tools/pd-assets/build_assets.py"))
}

fn bodies() -> &'static Vec<HeadOrBody> {
    static B: OnceLock<Vec<HeadOrBody>> = OnceLock::new();
    B.get_or_init(|| load_heads_and_bodies(&assets()).unwrap())
}

fn body_row(stem: &str) -> &'static HeadOrBody {
    bodies().iter().find(|r| r.stem.as_deref() == Some(stem)).unwrap_or_else(|| panic!("no g_HeadsAndBodies row for {stem}"))
}

fn stems(kinds: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = store().index.iter().filter(|(_, e)| kinds.contains(&e.kind.as_str())).map(|(s, _)| s.clone()).collect();
    v.sort();
    v
}

fn anim_at(num: u16, frame: f32, speed: f32) -> Anim {
    let mut a = Anim::default();
    let mut ctx = AnimCtx { bank: bank(), scale: 1.0, chrinfo: None, merging_enabled: true };
    a.set_animation(&mut ctx, num, false, frame, speed, 0.0);
    a
}

// ─── the format ──────────────────────────────────────────────────────────────

#[test]
fn every_model_loads_in_preorder_with_its_parts_and_textures() {
    let s = store();
    assert_eq!(s.index.len(), 225);
    for stem in s.index.keys() {
        let d = s.get(stem).unwrap_or_else(|e| panic!("{e}"));
        for (i, n) in d.nodes.iter().enumerate() {
            if let Some(p) = n.parent {
                assert!(p < i, "{stem}: node {i}'s parent {p} comes after it");
                assert!(d.nodes[p].subtree_end >= n.subtree_end, "{stem}: node {i} leaves its parent's subtree");
            }
            assert!(n.subtree_end > i && n.subtree_end <= d.nodes.len());
        }
        for (&part, &node) in &d.parts {
            assert!(node < d.nodes.len(), "{stem}: part {part} -> node {node}");
        }
        for b in &d.batches {
            assert!(b.idx.iter().all(|&i| (i as usize) < b.verts.len()), "{stem}: index out of range");
            assert!(b.verts.iter().all(|v| (v.mtx as usize) < d.nummatrices.max(64)), "{stem}: matrix slot out of range");
        }
        for m in &d.materials {
            if let Some(t) = &m.texture {
                // Two casings' second textures run past the end of their files
                // (the old export dropped them too); everything else resolves.
                if !(stem.starts_with("cart") && t.id == 0x10001) {
                    let info = d.textures.get(&t.id).unwrap_or_else(|| panic!("{stem}: texture {:#x} missing", t.id));
                    assert!(assets().path(&info.file).exists(), "{stem}: {}", info.file);
                }
            }
        }
    }
    let falcon = s.get("falcon2").unwrap();
    assert_eq!(s.by_filenum(falcon.filenum).unwrap().stem, "falcon2");
}

#[test]
fn chr_bodies_carry_helpers_heads_carry_part_boxes() {
    let body = store().get("dark_combat").unwrap();
    assert_eq!(body.skel, SKEL_CHR);
    let helpers = body.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Position { flags, .. } if flags & (MODELNODETYPE_0100 | MODELNODETYPE_0200) != 0)).count();
    assert!(helpers >= 4, "elbows and knees: {helpers}");
    assert!(matches!(body.nodes[0].kind, NodeKind::ChrInfo { .. }));
    assert!(body.get_part(MODELPART_CHR_HEADSPOT).is_some());
    let head = store().get("headdark_combat").unwrap();
    assert_eq!(head.skel, SKEL_HEAD);
    assert!(matches!(head.nodes[0].kind, NodeKind::BBox { hitpart: 8, .. }), "a head is rooted at its HITPART_HEAD box");
    assert!(head.nodes.iter().all(|n| !matches!(n.kind, NodeKind::Position { .. })), "a head has no joints of its own");
}

// ─── the spike tests that travel with the walker ─────────────────────────────

#[test]
fn falcon_and_hands_share_joints_0_to_32() {
    let gun = store().get("falcon2").unwrap();
    let hand = store().get("hand_joaf1").unwrap();
    // Every POSITION node of the hand model must match the gun's joint with the
    // same anim part: same rest offset, same matrix slot (bondgun.c:8394 draws the
    // hand with the gun's matrices).
    let mut checked = 0;
    for hn in &hand.nodes {
        let NodeKind::Position { pos: hp, animpart, mtx: hm, .. } = hn.kind else { continue };
        let (gp, gm) = gun
            .nodes
            .iter()
            .find_map(|n| match n.kind {
                NodeKind::Position { pos, animpart: a, mtx, .. } if a == animpart => Some((pos, mtx)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("gun lacks joint {animpart}"));
        assert_eq!(hm[0], gm[0], "joint {animpart} slot");
        let d = hp - gp;
        assert!(d.length() < 0.3, "joint {animpart} offset differs by {d:?}");
        checked += 1;
    }
    assert_eq!(checked, 33);
}

#[test]
fn falcon_reload_moves_the_left_hand_in_and_back() {
    let bank = bank();
    let mut m = Model::new(store().get("falcon2").unwrap());
    let reload = bank.by_name("ANIM_GUN_FALCON2_RELOAD").unwrap();
    let mut anim = Anim::default();
    let mut ctx = AnimCtx { bank, scale: 1.0, chrinfo: None, merging_enabled: true };
    anim.set_animation(&mut ctx, reload, false, 0.0, 1.0, 0.0);

    let wrist = |m: &Model| m.matrices[18].w_axis.truncate(); // left wrist
    let gun = |m: &Model| m.matrices[33].w_axis.truncate();
    let p = PoseParams::new(Mat4::IDENTITY, bank);

    m.set_matrices_with_anim(&p, Some(&anim), None);
    let (w0, g0) = (wrist(&m), gun(&m));
    let mut closest = f32::MAX;
    for _ in 0..91 {
        anim.tick(&mut ctx, 1, true);
        m.set_matrices_with_anim(&p, Some(&anim), None);
        closest = closest.min((wrist(&m) - gun(&m)).length());
    }
    let start_gap = (w0 - g0).length();
    assert!(closest < start_gap * 0.6, "the left hand never came to the gun: {start_gap:.1} -> {closest:.1}");
    assert_eq!(anim.frame as i32, 91, "clamped on the last frame (non-looping)");
}

// ─── chrs on the floor ───────────────────────────────────────────────────────

/// The spike's check on the new path: posed with PD's model scale and animscale
/// and nothing else (`body.c:170`), every body's lowest vertex stands on the
/// floor, standing and mid-stride. The spike measured the GLB bodies; this poses
/// PD's own model files through CHRINFO.
#[test]
fn every_body_stands_on_the_floor_with_pds_animscale() {
    let bank = bank();
    for stem in ["dark_frock", "area51guard", "cassandra", "mrblonde", "ddshock", "elvis1"] {
        let row = body_row(stem);
        let def = store().get(stem).unwrap();
        for (num, frames) in [(0x0002u16, 35..41), (0x0031, 1..21), (0x0055, 1..23)] {
            let mut lo = f32::INFINITY;
            for f in frames {
                let mut m = Model::new(def.clone());
                m.scale = row.model_scale();
                let mut a = Anim { animscale: row.animscale, ..Anim::default() };
                {
                    // One tick into frame `f`, so the CHRINFO has read the frame's
                    // root height (model_set_anim_frame2_with_chr_stuff).
                    let mut ctx = m.anim_ctx(bank);
                    a.set_animation(&mut ctx, num, false, f as f32 - 1.0, 1.0, 0.0);
                    a.tick(&mut ctx, 1, true);
                }
                update_chr_info(&a, &mut m.chrinfo);
                m.set_matrices_with_anim(&PoseParams::new(Mat4::IDENTITY, bank), Some(&a), None);
                for b in &def.batches {
                    for v in &b.verts {
                        lo = lo.min(m.matrices[v.mtx as usize].transform_point3(v.pos).y);
                    }
                }
            }
            assert!(lo.abs() < 4.0, "{stem} ANIM_{num:04X}: lowest point {lo:+.1} cm");
        }
    }
}

// ─── hits ────────────────────────────────────────────────────────────────────

fn joanna() -> Model {
    let body = body_row("dark_combat");
    let mut m = Model::with_head(store().get("dark_combat").unwrap(), Some(store().get("headdark_combat").unwrap()));
    m.scale = body.model_scale();
    // One tick into the idle, so the CHRINFO root stands at the clip's height.
    let mut a = Anim { animscale: body.animscale, ..Anim::default() };
    {
        let mut ctx = m.anim_ctx(bank());
        a.set_animation(&mut ctx, 1, false, 0.0, 1.0, 0.0);
        a.tick(&mut ctx, 1, true);
    }
    update_chr_info(&a, &mut m.chrinfo);
    m.set_matrices_with_anim(&PoseParams::new(Mat4::IDENTITY, bank()), Some(&a), None);
    m
}

#[test]
fn a_head_takes_its_matrix_from_the_body_and_stands_on_the_neck() {
    let m = joanna();
    let spot = m.headspot().unwrap();
    let joint = m.def.find_node_mtx_index(spot, 0).unwrap();
    assert_eq!(m.head_matrix(), Some(m.matrices[joint]));
    let head = m.head.clone().unwrap();
    let ys: Vec<f32> = head.batches.iter().flat_map(|b| b.verts.iter()).map(|v| m.matrices[v.mtx as usize].transform_point3(v.pos).y).collect();
    let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
    assert!(lo > 130.0 && hi < 175.0, "the head spans {lo:.0}..{hi:.0} cm");
}

#[test]
fn a_shot_hits_the_first_part_box_in_tree_order() {
    let m = joanna();
    let through = |y: f32| m.test_for_hit(Vec3::new(0.0, y, 300.0), Vec3::NEG_Z, None, HitPad::default());
    let (head, at) = through(150.0).expect("a shot at head height hits");
    assert_eq!(head, 8, "HITPART_HEAD");
    assert!(matches!(at, HitNode::Head(_) | HitNode::Body(_)));
    // Every box the ray passes, as chr_test_hit's loop finds them (each call
    // resumes from the last hit).
    let seq = |y: f32| {
        let mut from = None;
        let mut seq = vec![];
        while let Some((hp, at)) = m.test_for_hit(Vec3::new(0.0, y, 300.0), Vec3::NEG_Z, from, HitPad::default()) {
            seq.push(hp);
            from = Some(at);
            assert!(seq.len() < 40);
        }
        seq
    };
    assert_eq!(seq(130.0), [15], "HITPART_TORSO alone at the chest");
    // At the waist the ray crosses the right hand (holding the gun) before the
    // torso and pelvis: the first box in TREE order wins, not the nearest.
    assert_eq!(seq(115.0), [12, 13, 15, 7], "RHAND, RFOREARM, TORSO, PELVIS");
    assert_eq!(seq(90.0), [6, 7], "RTHIGH before PELVIS");
    assert!(seq(60.0).is_empty(), "between the knees");
    assert!(through(400.0).is_none(), "over her head");
    let feet = [8.0, -8.0].iter().find_map(|&x| m.test_for_hit(Vec3::new(x, 4.0, 300.0), Vec3::NEG_Z, None, HitPad::default()));
    assert!(matches!(feet.map(|h| h.0), Some(1 | 2 | 4 | 5)), "a foot or shin: {feet:?}");
    // A shield grows every box (chr.c:4534).
    assert!(m.test_for_hit(Vec3::new(0.0, 188.0, 300.0), Vec3::NEG_Z, None, HitPad(40.0 / m.scale)).is_some());
}

// ─── drawing ─────────────────────────────────────────────────────────────────

/// Every MP body with its head, and every head alone, draws pixels.
#[test]
fn every_chr_draws_through_the_cpu_rdp() {
    let bank = bank();
    let mut tex = TextureCache::new(&assets());
    let view = n64::rdp::View { proj: Mat4::perspective_rh_gl(0.9, 64.0 / 96.0, 10.0, 1000.0), vp: [0.0, 0.0, 64.0, 96.0], near: 10.0 };
    let lights = n64::rsp::Lights { ambient: 150.0, diffuse: 255.0, dir: Vec3::new(-0.6, 0.6, 0.36), lookat_x: Vec3::X, lookat_y: Vec3::Y };
    let cam = Mat4::look_at_rh(Vec3::new(0.0, 100.0, 330.0), Vec3::new(0.0, 90.0, 0.0), Vec3::Y);
    let draw = |m: &Model, tex: &mut TextureCache| -> usize {
        let mut g = n64::rdp::Gfx::new(64, 96);
        g.clear([0.0; 3]);
        draw_model(&mut g, tex, m, &view, &lights, &DrawOpts::default());
        g.fb.iter().filter(|p| p[0] + p[1] + p[2] > 0.0).count()
    };
    let mut n = 0;
    for stem in stems(&["chr"]) {
        let def: Arc<ModelDef> = store().get(&stem).unwrap();
        let mut m = Model::new(def.clone());
        if def.skel == SKEL_CHR {
            m.scale = 0.1;
            let a = anim_at(1, 0.0, 1.0);
            m.set_matrices_with_anim(&PoseParams::new(cam, bank), Some(&a), None);
        } else {
            // A head alone: posing it only sets its distance relations (it has
            // no joints), then it hangs at the origin, scaled to the frame.
            m.set_matrices_with_anim(&PoseParams::new(cam, bank), None, None);
            m.matrices = vec![cam * Mat4::from_translation(Vec3::new(0.0, 90.0, 0.0)) * Mat4::from_scale(Vec3::splat(0.4)); def.nummatrices.max(1)];
        }
        let px = draw(&m, &mut tex);
        assert!(px > 50, "{stem} drew {px} pixels");
        n += 1;
    }
    assert!(tex.error.is_none(), "{:?}", tex.error);
    assert!(n >= 130, "{n}");
}
