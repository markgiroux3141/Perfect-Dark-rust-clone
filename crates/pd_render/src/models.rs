//! Posed PD models on the GPU (guns, hands, casings, bodies, heads, held guns,
//! the guns' objects and the pickups), from the matrices `pd_core::model` computes, through the
//! one `n64::gpu` combiner path.
//!
//! A model draws the way `model_render` walks it: node by node in the file's
//! order, skipping what a hidden toggle or distance node prunes, each batch
//! with its material's cull, a node that never set the cull bits inheriting the
//! previous one's (`Node::cull_exit`). A `STARGUNFIRE` node's quads are
//! re-jittered every frame (`model_render_node_star_gunfire`, `model.c:3299`).
//!
//! Frame slots (projection, lights, the matrix palette) are handed out once per
//! instance per frame from a pool ([`ModelRenderer::begin_frame`]), because every
//! `queue.write_buffer` of a frame lands before the frame's passes run.
//!
//! Source: the old repo's `pd_guns/render.rs` `PdRenderer` (`build_model`,
//! `build_material`, the draw-list resolve, `jitter_star`).

use std::collections::HashMap;
use std::sync::Arc;

use glam::Mat4;
use n64::gpu::{Combiner, CullKey, Extras, FrameSlot, FrameUniform, Material, PipeKey, Vertex};
use n64::rdp::{rgba_f, Cull, MipTex};
use pd_core::assets::AssetDir;
use pd_core::ids::MODELPART_GUN_LASERLIQUID;
use pd_core::model::draw::draw_state;
use pd_core::model::{MatCull, ModelDef, NodeKind};
use pd_core::rng::Rng;
use wgpu::util::DeviceExt;

struct GpuBatch {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
    material: usize,
}

struct GpuModel {
    def: Arc<ModelDef>,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    batches: Vec<GpuBatch>,
    materials: Vec<Material>,
    /// The STARGUNFIRE batches' vertices as loaded, re-jittered each frame.
    star_verts: HashMap<usize, Vec<Vertex>>,
    /// MODELPART_GUN_LASERLIQUID's batches and their vertices' `t` (S10.5),
    /// which `bgun_render` scrolls in the model file itself.
    liquid: Vec<(usize, Vec<Vertex>, Vec<i32>)>,
}

/// One posed model to draw this frame.
pub struct Instance<'a> {
    pub def: &'a Arc<ModelDef>,
    /// Per-node toggle/distance visibility (`Model::vis`); `None` draws every
    /// toggle and the nearest level of detail.
    pub vis: Option<&'a [bool]>,
    /// The matrix palette: model → eye space.
    pub joints: &'a [Mat4],
    pub frame: FrameUniform,
    /// A cull that overrides every batch's (the DUALFLIP mirror's).
    pub cull: Option<Cull>,
    /// `MODELRENDERCONTEXT_BONDGUN_OBJ_XLU`: every batch blended, z-tested,
    /// writing no z (a cloaked gun, an object in x-ray).
    pub xlu: bool,
}

/// A resolved batch draw.
pub struct Cmd {
    model: String,
    batch: usize,
    slot: usize,
    key: PipeKey,
}

/// Every model file uploaded so far, its textures, and the per-frame slots.
pub struct ModelRenderer {
    models: HashMap<String, GpuModel>,
    textures: HashMap<String, n64::gpu::Texture>,
    slots: Vec<FrameSlot>,
    next_slot: usize,
    /// SUBST: `model_render_node_star_gunfire` jitters with `random()`, PD's one
    /// stream / a renderer-local stream, so drawing never changes the world.
    star_rng: Rng,
}

impl Default for ModelRenderer {
    fn default() -> Self {
        ModelRenderer { models: HashMap::new(), textures: HashMap::new(), slots: Vec::new(), next_slot: 0, star_rng: Rng::new(99) }
    }
}

fn is_below(def: &ModelDef, anc: usize, node: usize) -> bool {
    let mut cur = Some(node);
    while let Some(i) = cur {
        if i == anc {
            return true;
        }
        cur = def.nodes[i].parent;
    }
    false
}

/// Whether a node draws: every toggle and distance node at or above it is on.
fn node_visible(def: &ModelDef, vis: Option<&[bool]>, node: usize) -> bool {
    match vis {
        Some(v) => {
            let mut cur = Some(node);
            while let Some(i) = cur {
                if matches!(def.nodes[i].kind, NodeKind::Toggle | NodeKind::Distance { .. }) && !v.get(i).copied().unwrap_or(true) {
                    return false;
                }
                cur = def.nodes[i].parent;
            }
            true
        }
        // No instance state: every toggle, and only the nearest LOD.
        None => !def.nodes.iter().enumerate().any(|(i, n)| matches!(n.kind, NodeKind::Distance { near, .. } if near > 0.0) && is_below(def, i, node)),
    }
}

impl ModelRenderer {
    /// Upload `def` (once; later calls are free).
    pub fn load(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, combiner: &mut Combiner, assets: &AssetDir, def: &Arc<ModelDef>) -> Result<(), String> {
        if self.models.contains_key(&def.stem) {
            return Ok(());
        }
        let mut textures: HashMap<u32, n64::gpu::Texture> = HashMap::new();
        for (&id, info) in &def.textures {
            let t = match self.textures.get(&info.file) {
                Some(t) => t.clone(),
                None => {
                    let (w, h, rgba) = assets.read_png(&assets.path(&info.file))?;
                    let t = combiner.texture(device, queue, &MipTex::from_rgba8(w, h, &rgba, info.levels));
                    self.textures.insert(info.file.clone(), t.clone());
                    t
                }
            };
            textures.insert(id, t);
        }
        let materials: Vec<Material> = def
            .materials
            .iter()
            .map(|m| {
                let st = draw_state(m, None, [255; 4], Cull::Back);
                let tex = m.texture.as_ref().and_then(|t| textures.get(&t.id));
                let extras = Extras { env_from_frame: m.env.is_none(), fog_tint: m.fog_tint, fog: m.fog.map(rgba_f), texgen_linear: m.texgen_linear };
                combiner.material(device, &st, tex, &extras)
            })
            .collect();
        let mut verts: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut batches = Vec::new();
        let mut star_verts = HashMap::new();
        let liquid_node = def.get_part(MODELPART_GUN_LASERLIQUID);
        let mut liquid = Vec::new();
        for (bi, b) in def.batches.iter().enumerate() {
            let base = verts.len();
            let bv: Vec<Vertex> = b.verts.iter().map(|v| Vertex { pos: v.pos.to_array(), uv: v.uv, col: v.c.map(|x| x as f32), mtx: v.mtx as u32, flags: v.flags as u32 }).collect();
            if matches!(def.nodes.get(b.node).map(|n| &n.kind), Some(NodeKind::StarGunfire { .. })) {
                star_verts.insert(bi, bv.clone());
            }
            if liquid_node == Some(b.node) {
                let tc = bv.iter().map(|v| (v.uv[1] * 32.0).round() as i32).collect();
                liquid.push((bi, bv.clone(), tc));
            }
            verts.extend_from_slice(&bv);
            let first = indices.len() as u32;
            indices.extend(b.idx.iter().map(|&i| i as u32));
            batches.push(GpuBatch { first_index: first, index_count: b.idx.len() as u32, base_vertex: base as i32, material: b.material });
        }
        if verts.is_empty() {
            verts.push(Vertex::default());
        }
        if indices.is_empty() {
            indices.extend_from_slice(&[0, 0, 0]);
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("model-vb"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("model-ib"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX });
        self.models.insert(def.stem.clone(), GpuModel { def: def.clone(), vbuf, ibuf, batches, materials, star_verts, liquid });
        Ok(())
    }

    pub fn is_loaded(&self, stem: &str) -> bool {
        self.models.contains_key(stem)
    }

    /// Start a frame: every slot is free again.
    pub fn begin_frame(&mut self) {
        self.next_slot = 0;
    }

    /// Resolve `instances` into batch draws in `model_render`'s order, writing
    /// each instance's frame slot and making the pipelines it needs. Call
    /// before the render pass; models not [`ModelRenderer::load`]ed are skipped.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, combiner: &mut Combiner, instances: &[Instance]) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        for inst in instances {
            let Some(gm) = self.models.get(&inst.def.stem) else { continue };
            while self.slots.len() <= self.next_slot {
                self.slots.push(combiner.frame_slot(device));
            }
            let slot = self.next_slot;
            self.next_slot += 1;
            self.slots[slot].write(queue, &inst.frame, inst.joints);
            let def = &gm.def;
            let mut cull = Cull::Back;
            for (ni, node) in def.nodes.iter().enumerate() {
                if node.batches.is_empty() || !node_visible(def, inst.vis, ni) {
                    continue;
                }
                for &bi in &node.batches {
                    let batch = &gm.batches[bi];
                    let m = &def.materials[batch.material];
                    let c = match m.cull {
                        MatCull::Inherit => cull,
                        MatCull::None => Cull::None,
                        MatCull::Back => Cull::Back,
                        MatCull::Front => Cull::Front,
                        MatCull::Both => Cull::Both,
                    };
                    cull = c;
                    let c = inst.cull.unwrap_or(c);
                    // G_CULL_BOTH: the RSP culls every triangle.
                    let Some(ck) = CullKey::from_cull(c) else { continue };
                    let mut key = gm.materials[batch.material].key(ck);
                    if inst.xlu {
                        key.alpha = true;
                        key.zwrite = false;
                    }
                    combiner.prepare(device, key);
                    cmds.push(Cmd { model: def.stem.clone(), batch: bi, slot, key });
                }
                if let Some(c) = node.cull_exit {
                    cull = c;
                }
            }
        }
        cmds
    }

    /// `model_render_node_star_gunfire` (`model.c:3299`) for a model about to
    /// be drawn: each flash quad takes a random turn of its UV square, a random
    /// starting corner and a 0.75..1 scale.
    pub fn jitter_star(&mut self, queue: &wgpu::Queue, stem: &str) {
        let Some(gm) = self.models.get(stem) else { return };
        for (&bi, base) in &gm.star_verts {
            let batch = &gm.batches[bi];
            let mut out = base.clone();
            for q in 0..(base.len() / 4) {
                let src = &base[q * 4..q * 4 + 4];
                let rand1 = ((self.star_rng.random() << 10) & 0xffff) as f32;
                let ang = rand1 / 65536.0 * std::f32::consts::TAU;
                let s4 = ang.cos() * 724.0;
                let s3 = ang.sin() * 724.0;
                let s1 = (self.star_rng.random() >> 31) as usize;
                let mult = (0x10000 - (self.star_rng.random() & 0x3fff)) as f32 / 65536.0;
                let (c1, c2, c3, c4) = (512.0 + s3, 512.0 - s3, 512.0 - s4, 512.0 + s4);
                let st = [[c3, c2], [c1, c3], [c4, c1], [c2, c4]];
                for k in 0..4 {
                    let from = src[(s1 + k) % 4];
                    let d = &mut out[q * 4 + k];
                    d.pos = [from.pos[0] * mult, from.pos[1] * mult, from.pos[2] * mult];
                    d.uv = [st[k][0] / 32.0, st[k][1] / 32.0];
                }
            }
            let offset = batch.base_vertex as u64 * std::mem::size_of::<Vertex>() as u64;
            queue.write_buffer(&gm.vbuf, offset, bytemuck::cast_slice(&out));
        }
    }

    /// Rewrite batch `bi` of the model loaded as `stem` with new positions and
    /// UVs (a `DOORFLAG_0004` door's `door_calc_texturemap`), the rest of each
    /// vertex as loaded.
    pub fn rewrite_verts(&mut self, queue: &wgpu::Queue, stem: &str, bi: usize, pos_uv: &[([f32; 3], [f32; 2])]) {
        let Some(gm) = self.models.get(stem) else { return };
        let Some(b) = gm.def.batches.get(bi) else { return };
        if b.verts.len() != pos_uv.len() {
            return;
        }
        let out: Vec<Vertex> =
            b.verts.iter().zip(pos_uv).map(|(v, (p, uv))| Vertex { pos: *p, uv: *uv, col: v.c.map(|x| x as f32), mtx: v.mtx as u32, flags: v.flags as u32 }).collect();
        let offset = gm.batches[bi].base_vertex as u64 * std::mem::size_of::<Vertex>() as u64;
        queue.write_buffer(&gm.vbuf, offset, bytemuck::cast_slice(&out));
    }

    /// `bgun_render`'s laser liquid slide (`bondgun.c:8362`, one player only):
    /// every vertex's `t` falls 25 per 240th of a second, and when one passes
    /// -0x6000 they all go back 0x2000.
    pub fn slide_laser_liquid(&mut self, queue: &wgpu::Queue, stem: &str, lvupdate240: i32) {
        let Some(gm) = self.models.get_mut(stem) else { return };
        for (bi, verts, tcs) in gm.liquid.iter_mut() {
            for i in 0..tcs.len() {
                tcs[i] = (tcs[i] - lvupdate240 * 25) as i16 as i32;
                if tcs[i] < -0x6000 {
                    for t in tcs.iter_mut() {
                        *t = (*t + 0x2000) as i16 as i32;
                    }
                }
            }
            for (v, &t) in verts.iter_mut().zip(tcs.iter()) {
                v.uv[1] = t as f32 / 32.0;
            }
            let offset = gm.batches[*bi].base_vertex as u64 * std::mem::size_of::<Vertex>() as u64;
            queue.write_buffer(&gm.vbuf, offset, bytemuck::cast_slice(verts));
        }
    }

    /// Draw resolved commands.
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, combiner: &'a Combiner, cmds: &'a [Cmd]) {
        let mut cur: Option<&str> = None;
        for c in cmds {
            let Some(gm) = self.models.get(&c.model) else { continue };
            if cur != Some(c.model.as_str()) {
                rp.set_vertex_buffer(0, gm.vbuf.slice(..));
                rp.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                cur = Some(c.model.as_str());
            }
            let Some(pipe) = combiner.pipeline(c.key) else { continue };
            let batch = &gm.batches[c.batch];
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &self.slots[c.slot].bind, &[]);
            rp.set_bind_group(1, &gm.materials[batch.material].bind, &[]);
            rp.draw_indexed(batch.first_index..batch.first_index + batch.index_count, batch.base_vertex, 0..1);
        }
    }
}
