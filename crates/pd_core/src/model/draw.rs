//! `model_render` on the CPU: a posed [`Model`] through the RSP vertex stage
//! ([`n64::rsp::vertex`]) and the RDP ([`n64::rdp::raster_clipped`]), batch by
//! batch in node order, the head's nodes drawn in place of the body's HEADSPOT.
//! The menus draw their models this way, and `pd_snapshot model` uses it to
//! check the format by eye.
//!
//! Cull state leaks from node to node as on hardware: a batch whose node never
//! touched the cull bits draws with whatever the previous node left
//! ([`super::MatCull::Inherit`], `Node::cull_exit`), starting from the chr pass's
//! `G_CULL_BACK`.
//!
//! The fog tint (`Material::fog_tint`) is not applied: the menu's model context
//! turns fog off. The game view's GPU path applies it (M3).
//!
//! Source: the old repo's `pd_menu/pdmodel.rs` `ModelStore::render`.

use std::collections::HashMap;
use std::sync::Arc;

use n64::rdp::{self, AlphaTest, Cull, DrawState, Gfx, MipTex, Tile, View, PV};
use n64::rsp::{self, Lights};

use super::pose::Model;
use super::{MatAlphaTest, MatBlend, MatCull, Material, ModelDef, NodeKind, SKEL_HUDPIECE};
use crate::assets::AssetDir;

/// Textures decoded for the rasteriser, by asset path, loaded on first use.
pub struct TextureCache {
    assets: AssetDir,
    map: HashMap<String, Option<Arc<MipTex>>>,
    /// The first texture that failed to load, for the caller to report.
    pub error: Option<String>,
}

impl TextureCache {
    pub fn new(assets: &AssetDir) -> TextureCache {
        TextureCache { assets: assets.clone(), map: HashMap::new(), error: None }
    }

    fn get(&mut self, def: &ModelDef, id: u32) -> Option<Arc<MipTex>> {
        let info = def.textures.get(&id)?;
        if let Some(t) = self.map.get(&info.file) {
            return t.clone();
        }
        let t = match self.assets.read_png(&self.assets.path(&info.file)) {
            Ok((w, h, rgba)) => Some(Arc::new(MipTex::from_rgba8(w, h, &rgba, info.levels))),
            Err(e) => {
                self.error.get_or_insert(e);
                None
            }
        };
        self.map.insert(info.file.clone(), t.clone());
        t
    }
}

/// What the render context supplies beyond the model.
#[derive(Clone, Copy, Debug)]
pub struct DrawOpts {
    /// The env colour of a material that does not set one (the context's).
    pub env: [u8; 4],
    /// The hudpiece's scrolling "liquid": added to every vertex `s` of
    /// MODELPART_HUDPIECE_0000, in S10.5 units (`menu.c:2254`).
    pub hud_s: i32,
}

impl Default for DrawOpts {
    fn default() -> Self {
        DrawOpts { env: [255; 4], hud_s: 0 }
    }
}

/// Draw a posed model into `gfx`.
pub fn draw_model(gfx: &mut Gfx, textures: &mut TextureCache, model: &Model, view: &View, lights: &Lights, opts: &DrawOpts) {
    let mut cull = Cull::Back;
    let body = model.def.clone();
    for i in 0..body.nodes.len() {
        if !Model::reaches(&body, &model.vis, i) {
            continue;
        }
        if matches!(body.nodes[i].kind, NodeKind::HeadSpot) {
            if let Some(head) = model.head.clone() {
                for j in 0..head.nodes.len() {
                    if Model::reaches(&head, &model.head_vis, j) {
                        draw_node(gfx, textures, &head, j, model, view, lights, opts, &mut cull);
                    }
                }
            }
            continue;
        }
        draw_node(gfx, textures, &body, i, model, view, lights, opts, &mut cull);
    }
}

/// A material and its texture as the RDP's draw state.
pub fn draw_state<'a>(m: &Material, tex: Option<&'a MipTex>, env: [u8; 4], cull: Cull) -> DrawState<'a> {
    let tile = match &m.texture {
        Some(t) => Tile {
            cms: rdp::Addr::from_txmode(t.cms),
            cmt: rdp::Addr::from_txmode(t.cmt),
            shift: [rdp::shift_scale(t.shifts), rdp::shift_scale(t.shiftt)],
            ul: [t.uls, t.ult],
            mipmap: t.mipmap,
            bilerp: t.linear,
        },
        None => Tile { cms: rdp::Addr::Wrap, cmt: rdp::Addr::Wrap, shift: [1.0, 1.0], ul: [0.0, 0.0], mipmap: false, bilerp: false },
    };
    DrawState {
        two_cycle: m.two_cycle,
        combine: m.combine,
        tex,
        tile,
        prim: rdp::rgba_f(m.prim),
        env: rdp::rgba_f(m.env.unwrap_or(env)),
        xlu: m.blend == MatBlend::Alpha,
        alpha_test: match m.alpha_test {
            MatAlphaTest::None => AlphaTest::None,
            MatAlphaTest::Edge => AlphaTest::Edge,
            MatAlphaTest::Threshold => AlphaTest::Threshold,
        },
        ztest: m.ztest,
        zwrite: m.zwrite,
        decal: m.decal,
        cull,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_node(gfx: &mut Gfx, textures: &mut TextureCache, def: &ModelDef, node: usize, model: &Model, view: &View, lights: &Lights, opts: &DrawOpts, cull: &mut Cull) {
    let n = &def.nodes[node];
    let hud_liquid = def.skel == SKEL_HUDPIECE && def.get_part(0) == Some(node);
    for &bi in &n.batches {
        let b = &def.batches[bi];
        let m = &def.materials[b.material];
        let bcull = match m.cull {
            MatCull::Inherit => *cull,
            MatCull::None => Cull::None,
            MatCull::Back => Cull::Back,
            MatCull::Front => Cull::Front,
            MatCull::Both => Cull::Both,
        };
        let tex = m.texture.as_ref().and_then(|t| textures.get(def, t.id));
        let st = draw_state(m, tex.as_deref(), opts.env, bcull);
        let pv: Vec<PV> = b
            .verts
            .iter()
            .map(|v| {
                let mtx = model.matrices.get(v.mtx as usize).copied().unwrap_or(glam::Mat4::IDENTITY);
                let mut uv = v.uv;
                if hud_liquid {
                    uv[0] += opts.hud_s as f32 / 32.0;
                }
                rsp::vertex(&mtx, &view.proj, v.pos, uv, v.c, v.flags & 1 != 0, v.flags & 2 != 0, m.texgen_linear, lights)
            })
            .collect();
        for tri in b.idx.chunks_exact(3) {
            rdp::raster_clipped(gfx, [pv[tri[0] as usize], pv[tri[1] as usize], pv[tri[2] as usize]], &st, view);
        }
    }
    if let Some(c) = n.cull_exit {
        *cull = c;
    }
}
