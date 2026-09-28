//! The stage's textured BG (`stages/<code>/bg.json` + `bg.bin`, one model in
//! the one model format: a DL node per room and layer, world-space vertices,
//! baked vertex colours), drawn through the N64 combiner the way
//! `bg_render_scene` draws it: every room's opaque layer, then every room's
//! translucent layer (`bg.c:1133` vs `:1204`).
//!
//! Substitutions, until M9's room work:
//! - `// SUBST:` PD draws only the rooms its portals find on screen
//!   (`bg_tick_portals`) / every room is drawn every frame. The z-buffer makes
//!   the picture the same.
//! - `// SUBST:` PD rescales each room's vertex colours by its brightness
//!   (`room_highlight`, `dlights.c:1627`) / the base colours are drawn, which is
//!   brightness 255.
//! - `// SUBST:` a parent block orders its two translucent children by the
//!   camera's side of its plane (`bg_render_room_pass`, `bg.c:3190`) / they draw
//!   in the exported (`child` first) order.
//! - `// SUBST:` animated textures (`dyntex`, Complex's ocean) do not move.
//! - `// SUBST:` `sky_render` draws the stage's sky and clouds behind the BG /
//!   the view is cleared to the environment's sky colour.
//!
//! Source: the old repo's `pd_complex/bg.rs` and the BG half of
//! `pd_guns/render.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use n64::gpu::{Combiner, CullKey, Extras, FrameSlot, FrameUniform, Material, PipeKey, Vertex};
use n64::rdp::{rgba_f, Cull, MipTex};
use pd_core::assets::AssetDir;
use pd_core::model::draw::draw_state;
use pd_core::model::{MatCull, ModelDef};
use serde::Deserialize;
use wgpu::util::DeviceExt;

use crate::view::View;

/// The render context's env colour for a BG material that never set one.
const BG_ENV: [u8; 4] = [255, 255, 255, 255];

/// `struct nofogenvironment`'s fields the view needs (`bg.json` `env`).
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct NoFogEnv {
    pub near: f32,
    pub far: f32,
    pub sky_r: u8,
    pub sky_g: u8,
    pub sky_b: u8,
}

#[derive(Deserialize)]
struct EnvHeader {
    fog: bool,
    nofogenvironment: NoFogEnv,
}

#[derive(Deserialize)]
struct Header {
    env: EnvHeader,
}

struct Draw {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
    material: usize,
    key: PipeKey,
}

pub struct StageBg {
    pub def: Arc<ModelDef>,
    /// The stage's environment row: z range and sky.
    pub env: NoFogEnv,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    draws: Vec<Draw>,
    materials: Vec<Material>,
    slot: FrameSlot,
}

impl StageBg {
    /// Load `stages/<code>/bg.*` and upload it with its textures.
    pub fn load(device: &wgpu::Device, queue: &wgpu::Queue, combiner: &mut Combiner, assets: &AssetDir, code: &str) -> Result<StageBg, String> {
        let dir = assets.stage(code);
        let def = Arc::new(ModelDef::load_file(assets, &dir.join("bg.json"), &dir.join("bg.bin"))?);
        let header: Header = assets.read_json(&dir.join("bg.json"))?;
        if header.env.fog {
            return Err(format!("stage {code} runs with fog, which the BG does not draw yet"));
        }

        // Textures, once per file.
        let mut textures: HashMap<u32, n64::gpu::Texture> = HashMap::new();
        let mut by_file: HashMap<String, n64::gpu::Texture> = HashMap::new();
        for (&id, info) in &def.textures {
            let t = match by_file.get(&info.file) {
                Some(t) => t.clone(),
                None => {
                    let (w, h, rgba) = assets.read_png(&assets.path(&info.file))?;
                    let t = combiner.texture(device, queue, &MipTex::from_rgba8(w, h, &rgba, info.levels));
                    by_file.insert(info.file.clone(), t.clone());
                    t
                }
            };
            textures.insert(id, t);
        }

        // Materials, with the cull each is drawn with.
        let materials: Vec<Material> = def
            .materials
            .iter()
            .map(|m| {
                let st = draw_state(m, None, BG_ENV, Cull::Back);
                let tex = m.texture.as_ref().and_then(|t| textures.get(&t.id));
                let extras = Extras { env_from_frame: false, fog_tint: m.fog_tint, fog: m.fog.map(rgba_f), texgen_linear: m.texgen_linear };
                combiner.material(device, &st, tex, &extras)
            })
            .collect();

        // One vertex and index buffer; draws in node order, which is PD's (the
        // exporter lists every opaque room node before every translucent one).
        let mut verts: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut draws = Vec::new();
        let mut cull = Cull::Back;
        for node in &def.nodes {
            for &bi in &node.batches {
                let b = &def.batches[bi];
                let m = &def.materials[b.material];
                let c = match m.cull {
                    MatCull::Inherit => cull,
                    MatCull::None => Cull::None,
                    MatCull::Back => Cull::Back,
                    MatCull::Front => Cull::Front,
                    MatCull::Both => Cull::Both,
                };
                cull = c;
                let base = verts.len() as i32;
                verts.extend(b.verts.iter().map(|v| Vertex {
                    pos: v.pos.to_array(),
                    uv: v.uv,
                    col: v.c.map(|x| x as f32),
                    mtx: v.mtx as u32,
                    flags: v.flags as u32,
                }));
                let first = indices.len() as u32;
                indices.extend(b.idx.iter().map(|&i| i as u32));
                // G_CULL_BOTH: the RSP culls every triangle.
                if let Some(ck) = CullKey::from_cull(c) {
                    let key = materials[b.material].key(ck);
                    combiner.prepare(device, key);
                    draws.push(Draw { first_index: first, index_count: b.idx.len() as u32, base_vertex: base, material: b.material, key });
                }
            }
            if let Some(c) = node.cull_exit {
                cull = c;
            }
        }
        if verts.is_empty() {
            return Err(format!("stage {code}: the BG has no triangles"));
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("bg-vb"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("bg-ib"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX });
        let slot = combiner.frame_slot(device);
        Ok(StageBg { def, env: header.env.nofogenvironment, vbuf, ibuf, draws, materials, slot })
    }

    /// The sky colour the view is cleared to (0..1).
    pub fn sky(&self) -> [f64; 3] {
        let e = &self.env;
        [e.sky_r as f64 / 255.0, e.sky_g as f64 / 255.0, e.sky_b as f64 / 255.0]
    }

    /// Set this frame's camera: the BG's one matrix is world → eye.
    pub fn prepare(&self, queue: &wgpu::Queue, view: &View) {
        let frame = FrameUniform::new(view.projection()).with_lookat(view.look, view.up);
        self.slot.write(queue, &frame, &[view.world_to_eye()]);
    }

    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, combiner: &'a Combiner) {
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.set_bind_group(0, &self.slot.bind, &[]);
        for d in &self.draws {
            let Some(pipe) = combiner.pipeline(d.key) else { continue };
            rp.set_pipeline(pipe);
            rp.set_bind_group(1, &self.materials[d.material].bind, &[]);
            rp.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use pd_core::assets::AssetDir;
    use pd_core::model::ModelDef;
    use pd_sim::stage::Stage;

    fn assets() -> AssetDir {
        AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))
    }

    /// The textured BG and the collision tiles are the same building: the same
    /// bounding box to the centimetre, and every texture it samples in the pool.
    #[test]
    fn the_textured_bg_matches_the_collision_tiles() {
        let a = assets();
        let dir = a.stage("ref");
        let def = ModelDef::load_file(&a, &dir.join("bg.json"), &dir.join("bg.bin")).unwrap();
        let stage = Stage::load(&a, "ref").unwrap();
        let (lo, hi) = stage.geom.bounds();
        let mut bmin = Vec3::splat(f32::INFINITY);
        let mut bmax = Vec3::splat(f32::NEG_INFINITY);
        let mut tris = 0;
        for b in &def.batches {
            tris += b.idx.len() / 3;
            for v in &b.verts {
                bmin = bmin.min(v.pos);
                bmax = bmax.max(v.pos);
            }
        }
        assert_eq!(tris, 2559);
        assert!((bmin - lo).abs().max_element() < 1.0 && (bmax - hi).abs().max_element() < 1.0, "BG {bmin} {bmax} vs tiles {lo} {hi}");
        for m in &def.materials {
            if let Some(t) = &m.texture {
                let info = def.textures.get(&t.id).unwrap_or_else(|| panic!("texture {:#x} not listed", t.id));
                assert!(a.path(&info.file).exists(), "{} missing", info.file);
            }
        }
    }

    /// The BG through the combiner on a headless GPU, from Complex's first spawn
    /// pad: most of the picture is BG, not the sky it is cleared to. Skipped
    /// when the machine has no GPU adapter.
    #[test]
    fn complex_renders_from_a_spawn_pad() {
        let Ok(gpu) = engine::gpu::HeadlessGpu::new() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let a = assets();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = crate::Renderer::new(&gpu.device, &gpu.queue, format);
        r.load_stage(&gpu.device, &gpu.queue, &a, "ref").unwrap();
        assert_eq!(r.z_range(), (15.0, 10000.0), "Complex's g_NoFogEnvironments row");
        let stage = Stage::load(&a, "ref").unwrap();
        let pad = &stage.pads[stage.spawn_pads[1]];
        let eye = pad.pos + Vec3::Y * 106.0;
        let look = Vec3::new(pad.look.x, 0.0, pad.look.z).normalize();
        let view = crate::View { eye, look, up: Vec3::Y, fovy: 60.0, aspect: 320.0 / 220.0, znear: 15.0, zfar: 10000.0, shake: 0.0 };
        let t = engine::gpu::RenderTarget::on_device(&gpu.device, 320, 220, format, true);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.render(&gpu.queue, &mut enc, &t.view, &t.depth.as_ref().unwrap().1, &view);
        gpu.queue.submit(Some(enc.finish()));
        let px = t.read_rgba8(&gpu.device, &gpu.queue);
        let sky = [2u8, 0, 0];
        let bg = px.chunks(4).filter(|p| p[..3] != sky).count();
        assert!(bg > 320 * 220 * 3 / 4, "only {bg} BG pixels");
    }
}
