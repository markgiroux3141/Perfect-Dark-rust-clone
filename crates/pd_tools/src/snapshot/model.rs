//! `pd_snapshot <outdir> model <stem> [options]`: a CPU turntable of any model
//! file, a chr body wearing its head and holding a gun, to one PNG sheet. This is
//! how the model format is checked by eye: it poses through `pd_core::model` and
//! draws through `n64::rdp`, the same path as the menus.
//!
//! Options:
//! * `--head <stem>|none`: the head on a chr body (default: the body's MP head,
//!   `g_MpBodies`; for a body whose head PD picks at random, the first of
//!   `g_MpMaleHeads` / `g_MpFemaleHeads`);
//! * `--gun <stem>`: a held gun (a `P*` model) posed on `MODELPART_CHR_RIGHTHAND`,
//!   whose matrix is the gun's `rendermtx` (its POSITIONHELD root is the grip);
//! * `--anim <num|ANIM_name>` and `--frame <f>`: the pose (default: a chr stands
//!   in `ANIM_TWO_GUN_HOLD`, anything else is at rest);
//! * `--views <n>`: turntable views, evenly around the vertical (default 8);
//! * `--pitch <deg>`: the camera's elevation (default 8.5; 90 looks straight
//!   down, for a held gun, which is authored lying in its hand's space).
//!
//! Lighting is the menus' model light (`var80071468`, `menu.c:1714`). A
//! weapon's first-person model gets its gun's static part visibility: the
//! `sethidden` commands of its `gunviscmds` (`invitems.c`), which
//! `bgun_execute_model_cmd_list` applies every frame, and the muzzle flash off.
//! The rest of the gun's visibility logic (and the hands) is M4's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::{Mat4, Vec3};
use n64::rdp::{Gfx, View};
use n64::rsp::Lights;
use pd_core::anim::{update_chr_info, Anim, AnimBank};
use pd_core::assets::AssetDir;
use pd_core::model::draw::{draw_model, DrawOpts, TextureCache};
use pd_core::model::{Bodies, Model, ModelDef, ModelStore, NodeKind, PoseParams, SKEL_CHR};
use pd_core::text::{FontId, Fonts, TextCtx, TextState};

/// `MODELPART_CHR_RIGHTHAND` (`constants.h:2354`).
const MODELPART_CHR_RIGHTHAND: i32 = 3;
const CELL_W: usize = 200;
const CELL_H: usize = 250;
const COLS: usize = 4;

pub struct Opts {
    pub stem: String,
    /// `None`: the default; `Some(None)`: no head.
    pub head: Option<Option<String>>,
    pub gun: Option<String>,
    pub anim: Option<String>,
    pub frame: f32,
    pub views: usize,
    pub pitch: f32,
}

pub fn parse(args: &[String]) -> Result<Opts, String> {
    let mut it = args.iter();
    let stem = it.next().ok_or("model: which model? (a stem from assets/models/index.json)")?.clone();
    let mut o = Opts { stem, head: None, gun: None, anim: None, frame: 0.0, views: 8, pitch: 8.5 };
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--head" => {
                let h = val()?;
                o.head = Some((h != "none").then_some(h));
            }
            "--gun" => o.gun = Some(val()?),
            "--anim" => o.anim = Some(val()?),
            "--frame" => o.frame = val()?.parse().map_err(|e| format!("--frame: {e}"))?,
            "--views" => o.views = val()?.parse::<usize>().map_err(|e| format!("--views: {e}"))?.clamp(1, 16),
            "--pitch" => o.pitch = val()?.parse::<f32>().map_err(|e| format!("--pitch: {e}"))?.clamp(-89.9, 89.9),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn anim_num(bank: &AnimBank, s: &str) -> Result<u16, String> {
    let hex = s.trim_start_matches("0x");
    u16::from_str_radix(hex, 16).ok().filter(|n| bank.get(*n).is_some()).or_else(|| bank.by_name(s)).ok_or(format!("no animation {s}"))
}

struct Scene {
    body: Model,
    gun: Option<Model>,
    anim: Option<Anim>,
}

impl Scene {
    fn pose(&mut self, rendermtx: Mat4, bank: &AnimBank) {
        self.body.set_matrices_with_anim(&PoseParams::new(rendermtx, bank), self.anim.as_ref(), None);
        if let Some(gun) = &mut self.gun {
            let hand = self.body.def.get_part(MODELPART_CHR_RIGHTHAND).and_then(|n| self.body.def.find_node_mtx_index(n, 0));
            let hand = hand.map_or(rendermtx, |i| self.body.matrices[i]);
            gun.set_matrices_with_anim(&PoseParams::new(hand, bank), None, None);
        }
    }

    /// Every drawn vertex, posed.
    fn points(&self) -> Vec<Vec3> {
        let mut pts = Vec::new();
        let mut add = |m: &Model, def: &ModelDef, vis: &[bool]| {
            for b in &def.batches {
                if !Model::reaches(def, vis, b.node) {
                    continue;
                }
                for v in &b.verts {
                    if let Some(mtx) = m.matrices.get(v.mtx as usize) {
                        pts.push(mtx.transform_point3(v.pos));
                    }
                }
            }
        };
        add(&self.body, &self.body.def, &self.body.vis);
        if let Some(h) = &self.body.head {
            add(&self.body, h, &self.body.head_vis);
        }
        if let Some(g) = &self.gun {
            add(g, &g.def, &g.vis);
        }
        pts
    }
}

fn build(assets: &AssetDir, store: &ModelStore, bank: &AnimBank, o: &Opts) -> Result<Scene, String> {
    let bodies = Bodies::load(assets)?;
    let def: Arc<ModelDef> = store.get(&o.stem)?;
    let is_chr = def.skel == SKEL_CHR && matches!(def.nodes.first().map(|n| &n.kind), Some(NodeKind::ChrInfo { .. }));
    let row = bodies.by_stem(&o.stem).cloned();
    let head = match &o.head {
        Some(Some(h)) => Some(store.get(h)?),
        Some(None) => None,
        None => match row.as_ref().and_then(|r| bodies.default_head(r.num, 0)).and_then(|h| h.stem.clone()) {
            Some(h) => Some(store.get(&h)?),
            None => None,
        },
    };
    let mut body = Model::with_head(def, head);
    let anim = match (&o.anim, is_chr) {
        (Some(a), _) => Some(anim_num(bank, a)?),
        (None, true) => Some(1), // ANIM_TWO_GUN_HOLD
        (None, false) => None,
    };
    let anim = anim.map(|num| {
        let mut a = Anim { animscale: row.as_ref().map_or(1.0, |r| r.animscale), ..Anim::default() };
        if is_chr {
            body.scale = row.as_ref().map_or(0.1, |r| r.model_scale());
            // One tick into the frame, so the CHRINFO root reads its height.
            let mut ctx = body.anim_ctx(bank);
            a.set_animation(&mut ctx, num, false, (o.frame - 1.0).max(0.0), 1.0, 0.0);
            a.tick(&mut ctx, 1, true);
            update_chr_info(&a, &mut body.chrinfo);
        } else {
            let mut ctx = body.anim_ctx(bank);
            a.set_animation(&mut ctx, num, false, o.frame, 1.0, 0.0);
        }
        a
    });
    hide_gun_parts(assets, &mut body)?;
    let gun = match &o.gun {
        Some(g) => Some(Model::new(store.get(g)?)),
        None => None,
    };
    Ok(Scene { body, gun, anim })
}

/// The `sethidden` gunviscmds of the weapon whose first-person model this is.
fn hide_gun_parts(assets: &AssetDir, m: &mut Model) -> Result<(), String> {
    let w: serde_json::Value = assets.read_json(&assets.data("weapons.json"))?;
    let fp = format!("guns/{}.bin", m.def.stem);
    let Some(weapon) = w["weapons"].as_array().into_iter().flatten().find(|x| x["assets"]["fp_model"].as_str() == Some(fp.as_str())) else {
        return Ok(());
    };
    let cmds = weapon["gunviscmds_symbol"].as_str().and_then(|s| w["gunviscmds"][s].as_array());
    for c in cmds.into_iter().flatten() {
        if c["op"] == "sethidden" {
            if let Some(part) = c["args"][0].as_i64() {
                m.set_part_visible(part as i32, false);
            }
        }
    }
    // pd_weapons.py's part_visibility adds the muzzle flash, hidden between shots.
    for p in weapon["part_visibility"].as_array().into_iter().flatten() {
        if p["visible"] == false {
            if let Some(part) = p["part"].as_i64() {
                m.set_part_visible(part as i32, false);
            }
        }
    }
    Ok(())
}

/// Render the sheet; returns the PNG's path.
pub fn run(outdir: &Path, args: &[String]) -> Result<PathBuf, String> {
    let o = parse(args)?;
    let assets = crate::assets();
    let store = ModelStore::load(&assets)?;
    let bank = AnimBank::load(&assets)?;
    let fonts = Fonts::load(&assets)?;
    let mut scene = build(&assets, &store, &bank, &o)?;

    // Frame it from the rest-of-world pose.
    scene.pose(Mat4::IDENTITY, &bank);
    let pts = scene.points();
    if pts.is_empty() {
        return Err(format!("{}: nothing to draw", o.stem));
    }
    let (lo, hi) = pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let centre = (lo + hi) * 0.5;
    let radius = ((hi - lo) * 0.5).length().max(1e-3);
    let fovy = 40f32.to_radians();
    let aspect = CELL_W as f32 / CELL_H as f32;
    let fovx = 2.0 * ((fovy * 0.5).tan() * aspect).atan();
    let dist = radius / (fovx.min(fovy) * 0.5).sin() * 1.02;
    let near = (dist - radius * 1.2).max(radius * 0.01);
    let view = View { proj: Mat4::perspective_rh_gl(fovy, aspect, near, dist + radius * 2.0), vp: [0.0, 0.0, CELL_W as f32, CELL_H as f32], near };
    let (sp, cp) = o.pitch.to_radians().sin_cos();
    let cam = Mat4::look_at_rh(centre + Vec3::new(0.0, sp, cp) * dist, centre, Vec3::Y);
    // var80071468 (menu.c:1714): gdSPDefLights1(0x96,0x96,0x96, 0xff,0xff,0xff, 0xb2,0x4d,0x2e).
    let sb = |b: u8| b as i8 as f32 / 127.0;
    let lights = Lights { ambient: 150.0, diffuse: 255.0, dir: Vec3::new(sb(0xb2), sb(0x4d), sb(0x2e)), lookat_x: Vec3::X, lookat_y: Vec3::Y };

    let rows = o.views.div_ceil(COLS);
    let cols = o.views.min(COLS);
    let (sw, sh) = (cols * CELL_W, rows * CELL_H);
    let mut sheet = vec![0u8; sw * sh * 4];
    let mut textures = TextureCache::new(&assets);
    let label = format!(
        "{}{}{}",
        o.stem,
        scene.body.head.as_ref().map_or(String::new(), |h| format!(" + {}", h.stem)),
        scene.gun.as_ref().map_or(String::new(), |g| format!(" + {}", g.def.stem))
    );
    for k in 0..o.views {
        let yaw = k as f32 * std::f32::consts::TAU / o.views as f32;
        let turn = Mat4::from_translation(centre) * Mat4::from_rotation_y(yaw) * Mat4::from_translation(-centre);
        scene.pose(cam * turn, &bank);
        let mut gfx = Gfx::new(CELL_W, CELL_H);
        gfx.clear([0.11, 0.12, 0.15]);
        draw_model(&mut gfx, &mut textures, &scene.body, &view, &lights, &DrawOpts::default());
        if let Some(g) = &scene.gun {
            draw_model(&mut gfx, &mut textures, g, &view, &lights, &DrawOpts::default());
        }
        let mut ts = TextState::default();
        let mut ctx = TextCtx { gfx: &mut gfx, ts: &mut ts, fonts: &fonts, frac20: 0.0 };
        let caption = if k == 0 { format!("{}\n{:.0} deg", label.replace(" + ", "\n+ "), yaw.to_degrees()) } else { format!("{:.0} deg", yaw.to_degrees()) };
        let (mut x, mut y) = (4, 4);
        ctx.render_v2(&mut x, &mut y, &caption, FontId::Xs, 0xc0e0ffff, CELL_W as i32, CELL_H as i32, 0, 0);
        let rgba = gfx.rgba8(false);
        let (cx, cy) = ((k % COLS) * CELL_W, (k / COLS) * CELL_H);
        for row in 0..CELL_H {
            let src = &rgba[row * CELL_W * 4..(row + 1) * CELL_W * 4];
            let dst = ((cy + row) * sw + cx) * 4;
            sheet[dst..dst + CELL_W * 4].copy_from_slice(src);
        }
    }
    if let Some(e) = &textures.error {
        eprintln!("pd_snapshot model: {e}");
    }
    let mut name = format!("model_{}", o.stem);
    if let Some(h) = &scene.body.head {
        name += &format!("_{}", h.stem);
    }
    if let Some(g) = &scene.gun {
        name += &format!("_{}", g.def.stem);
    }
    let path = outdir.join(format!("{name}.png"));
    crate::write_png(&path, sw, sh, &sheet)?;
    Ok(path)
}
