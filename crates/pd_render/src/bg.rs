//! The stage's textured BG (`stages/<code>/bg.json` + `bg.bin`, one model in
//! the one model format: a DL node per room and layer, world-space vertices,
//! baked vertex colours), drawn through the N64 combiner the way
//! `bg_render_scene` (`bg.c:987`) draws it:
//!
//! - only the rooms the player's portals found on screen (`pd_sim`'s
//!   `bg_tick_portals`), sorted by their draw order, each clipped to its
//!   portal box (`bg_scissor_within_viewport_f`); every opaque layer first,
//!   the translucent layers after, in reverse order (`bg.c:1133` vs `:1204`);
//! - each room's vertex colours rescaled by its brightness and flash
//!   (`room_highlight`, `dlights.c:1626`), rewritten when they change, as PD
//!   rewrites the room's colour table;
//! - a translucent layer's blocks ordered by the camera's side of each parent
//!   block's plane (`bg_render_room_pass`, `bg.c:3190`);
//! - the animated textures' s,t moved every frame (`dyntex_tick_room`,
//!   `dyntex.c:150`: Complex's ocean, Sewers' river).
//!
//! Without a portal view (the stage snapshot's odd aspects, a test) every
//! room is drawn in node order, unclipped.
//!
//! Substitutions:
//! - `// SUBST:` PD draws each room's props between its BG passes, under the
//!   room's scissor (`props_render`, `bg.c:1143-1178`) / the props are drawn
//!   after every opaque room and before the translucent ones, unclipped; the
//!   z-buffer makes the picture the same but for props seen through a portal's
//!   edge.
//!
//! Source: the old repo's `pd_complex/bg.rs` and the BG half of
//! `pd_guns/render.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec3;
use n64::gpu::{Combiner, CullKey, Extras, FrameSlot, FrameUniform, Material, PipeKey, Vertex};
use n64::rdp::{rgba_f, Cull, MipTex};
use pd_core::assets::AssetDir;
use pd_core::model::draw::draw_state;
use pd_core::model::{MatCull, ModelDef};
use pd_sim::lights::{Lights, ROOMFLAG_BRIGHTNESS_CALCED, ROOMFLAG_LIGHTSOFF};
use pd_sim::stage::portals::{DrawSlot, PortalView};
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
    pub clouds_enabled: u8,
    pub clouds_r: u8,
    pub clouds_g: u8,
    pub clouds_b: u8,
    pub clouds_scale: f32,
    pub clouds_height: f32,
}

#[derive(Deserialize)]
struct EnvHeader {
    fog: bool,
    nofogenvironment: NoFogEnv,
}

/// A layer's block tree (`pd_bg.bsp_tree`): a leaf, or a parent's plane and
/// its two children.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
enum TreeItem {
    Leaf { leaf: usize },
    Plane { plane: [f32; 6], a: Box<Option<TreeItem>>, b: Box<Option<TreeItem>> },
}

#[derive(Deserialize)]
struct NodeHeader {
    room: Option<u16>,
    layer: Option<String>,
    tree: Option<Vec<Option<TreeItem>>>,
}

#[derive(Deserialize)]
struct BatchHeader {
    #[serde(default)]
    leaf: usize,
    #[serde(default)]
    cidx: Vec<u16>,
    dyntex: Option<String>,
    #[serde(default)]
    st: Vec<[i16; 2]>,
    #[serde(default)]
    stscale: Vec<[u32; 2]>,
}

#[derive(Deserialize)]
struct RoomHeader {
    room: u16,
    numcolours: usize,
    colour_alpha_only: Vec<usize>,
}

#[derive(Deserialize)]
struct Header {
    env: EnvHeader,
    nodes: Vec<NodeHeader>,
    batches: Vec<BatchHeader>,
    rooms: Vec<RoomHeader>,
}

struct Draw {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
    material: usize,
    key: PipeKey,
    /// The block of its layer (`leaf` ordinal), for the BSP order.
    leaf: usize,
}

/// `DYNTEXTYPE_*` the arenas use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DyntexKind {
    River,
    Monitor,
    Ocean,
    Arrows,
}

/// One room's animated vertices of one kind (`struct dyntextype`).
struct DyntexGroup {
    kind: DyntexKind,
    /// Into the vertex buffer, with the base s,t and the texture scale.
    verts: Vec<(usize, [i32; 2], [u32; 2])>,
}

/// One room's draws and colours.
#[derive(Default)]
struct RoomDraws {
    opa: Vec<usize>,
    xlu: Vec<usize>,
    xlu_tree: Option<Vec<Option<TreeItem>>>,
    /// The room's colour table as the file has it (`gfxdata->vertices` + ...)
    /// and which entries only scale their alpha (PD's bug: tested by the
    /// vertex's flags at a colour's index, `dlights.c:1668`).
    base: Vec<[u8; 4]>,
    alpha_only: Vec<bool>,
    /// Each of the room's vertices: its place in the buffer and its colour index.
    verts: Vec<(usize, u16)>,
    dyntex: Vec<DyntexGroup>,
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
    /// By room number.
    rooms: Vec<RoomDraws>,
    /// The draws in node order (every opaque room, then every translucent one).
    all: Vec<usize>,
    /// The CPU copy of the vertex buffer, rewritten per room.
    verts: std::cell::RefCell<Vec<Vertex>>,
    /// Per room: the brightness its vertices were last written with, and which
    /// of its animated kinds have initialised.
    rooms_state: std::cell::RefCell<Vec<RoomState>>,
}

/// A room's last written brightness (`br`, flash, black flashes) and which of
/// its animated kinds have initialised.
type RoomState = (Option<(u8, i32, bool)>, Vec<bool>);

/// What a frame of the BG needs from the world: the rooms on screen and their
/// lighting, and the 80-second clock the animated textures run on.
pub struct BgFrame<'a> {
    pub portals: &'a PortalView,
    pub lights: &'a Lights,
    pub frac80: f32,
    /// PD pixels (the player's view, 320 × 220 for one) to target pixels.
    pub scale: [f32; 2],
    /// The target's size, for clamping the scissors.
    pub target: [u32; 2],
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
        if header.nodes.len() != def.nodes.len() || header.batches.len() != def.batches.len() {
            return Err(format!("stage {code}: bg.json's nodes/batches don't match its model"));
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

        let nrooms = header.rooms.iter().map(|r| r.room as usize + 1).max().unwrap_or(1);
        let mut rooms: Vec<RoomDraws> = (0..nrooms).map(|_| RoomDraws::default()).collect();
        for r in &header.rooms {
            let room = &mut rooms[r.room as usize];
            room.base = vec![[0, 0, 0, 255]; r.numcolours];
            room.alpha_only = vec![false; r.numcolours];
            for &i in &r.colour_alpha_only {
                if i < r.numcolours {
                    room.alpha_only[i] = true;
                }
            }
        }

        // One vertex and index buffer; draws in node order, which is PD's (the
        // exporter lists every opaque room node before every translucent one).
        let mut verts: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut draws = Vec::new();
        let mut all = Vec::new();
        let mut cull = Cull::Back;
        for (ni, node) in def.nodes.iter().enumerate() {
            let nh = &header.nodes[ni];
            let room = nh.room.map(|r| r as usize).filter(|&r| r < rooms.len());
            let xlu = nh.layer.as_deref() == Some("xlu");
            if let (Some(r), true) = (room, xlu) {
                rooms[r].xlu_tree = nh.tree.clone();
            }
            for &bi in &node.batches {
                let b = &def.batches[bi];
                let bh = &header.batches[bi];
                let m = &def.materials[b.material];
                let c = match m.cull {
                    MatCull::Inherit => cull,
                    MatCull::None => Cull::None,
                    MatCull::Back => Cull::Back,
                    MatCull::Front => Cull::Front,
                    MatCull::Both => Cull::Both,
                };
                cull = c;
                let base = verts.len();
                verts.extend(b.verts.iter().map(|v| Vertex { pos: v.pos.to_array(), uv: v.uv, col: v.c.map(|x| x as f32), mtx: v.mtx as u32, flags: v.flags as u32 }));
                if let Some(r) = room {
                    let rd = &mut rooms[r];
                    for (k, v) in b.verts.iter().enumerate() {
                        let ci = bh.cidx.get(k).copied().unwrap_or(0);
                        rd.verts.push((base + k, ci));
                        if let Some(slot) = rd.base.get_mut(ci as usize) {
                            *slot = v.c;
                        }
                    }
                    if let Some(kind) = bh.dyntex.as_deref().and_then(dyntex_kind) {
                        // dyntex_set_current_type (dyntex.c:345): Villa's shallow
                        // water doesn't move; nothing else on an arena is special.
                        let group = match rd.dyntex.iter_mut().position(|g| g.kind == kind) {
                            Some(i) => &mut rd.dyntex[i],
                            None => {
                                rd.dyntex.push(DyntexGroup { kind, verts: Vec::new() });
                                rd.dyntex.last_mut().unwrap()
                            }
                        };
                        for k in 0..b.verts.len() {
                            let st = bh.st.get(k).copied().unwrap_or([0, 0]);
                            let sc = bh.stscale.get(k).copied().unwrap_or([0xffff, 0xffff]);
                            group.verts.push((base + k, [st[0] as i32, st[1] as i32], sc));
                        }
                    }
                }
                let first = indices.len() as u32;
                indices.extend(b.idx.iter().map(|&i| i as u32));
                // G_CULL_BOTH: the RSP culls every triangle.
                if let Some(ck) = CullKey::from_cull(c) {
                    let key = materials[b.material].key(ck);
                    combiner.prepare(device, key);
                    let di = draws.len();
                    draws.push(Draw { first_index: first, index_count: b.idx.len() as u32, base_vertex: base as i32, material: b.material, key, leaf: bh.leaf });
                    all.push(di);
                    if let Some(r) = room {
                        if xlu {
                            rooms[r].xlu.push(di);
                        } else {
                            rooms[r].opa.push(di);
                        }
                    }
                }
            }
            if let Some(c) = node.cull_exit {
                cull = c;
            }
        }
        if verts.is_empty() {
            return Err(format!("stage {code}: the BG has no triangles"));
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bg-vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("bg-ib"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX });
        let slot = combiner.frame_slot(device);
        let n = rooms.len();
        Ok(StageBg {
            def,
            env: header.env.nofogenvironment,
            vbuf,
            ibuf,
            draws,
            materials,
            slot,
            rooms,
            all,
            verts: std::cell::RefCell::new(verts),
            rooms_state: std::cell::RefCell::new((0..n).map(|_| (None, Vec::new())).collect()),
        })
    }

    /// The sky colour the view is cleared to (0..1).
    /// The sky's fields of the environment (`sky_render`).
    pub fn sky_env(&self) -> crate::sky::SkyEnv {
        let e = &self.env;
        crate::sky::SkyEnv {
            sky: [e.sky_r, e.sky_g, e.sky_b],
            clouds_enabled: e.clouds_enabled != 0,
            clouds: [e.clouds_r, e.clouds_g, e.clouds_b],
            clouds_scale: e.clouds_scale,
            clouds_height: e.clouds_height,
        }
    }

    pub fn sky(&self) -> [f64; 3] {
        let e = &self.env;
        [e.sky_r as f64 / 255.0, e.sky_g as f64 / 255.0, e.sky_b as f64 / 255.0]
    }

    /// Set this frame's camera: the BG's one matrix is world → eye.
    /// `three_point`: the RDP's 3-point texture filter. With `frame`, the rooms
    /// on screen get their brightness (`room_highlight`) and animated textures.
    pub fn prepare(&self, queue: &wgpu::Queue, view: &View, three_point: bool, frame: Option<&BgFrame>) {
        let mut f = FrameUniform::new(view.projection()).with_lookat(view.look, view.up);
        f.misc[1] = three_point as u32 as f32;
        self.slot.write(queue, &f, &[view.world_to_eye()]);
        let Some(frame) = frame else { return };
        let mut verts = self.verts.borrow_mut();
        let mut dirty: Vec<(usize, usize)> = Vec::new();
        let mut state = self.rooms_state.borrow_mut();
        for s in &frame.portals.drawslots {
            let r = s.roomnum as usize;
            let Some(rd) = self.rooms.get(r) else { continue };
            let Some(rl) = frame.lights.rooms.get(r) else { continue };
            // room_get_settled_regional_brightness_for_player; the flash; and
            // whether a black colour takes the flash too (dlights.c:1717).
            let regional = if rl.flags & ROOMFLAG_BRIGHTNESS_CALCED != 0 { rl.br_settled_regional } else { 255 };
            let key = (regional, rl.br_flash as i32, rl.lightop_cur_frac == 0.0 || rl.flags & ROOMFLAG_LIGHTSOFF != 0);
            if state[r].0 != Some(key) {
                state[r].0 = Some(key);
                let table = room_highlight(&rd.base, &rd.alpha_only, key.0, key.1, key.2);
                for &(vi, ci) in &rd.verts {
                    if let Some(c) = table.get(ci as usize) {
                        verts[vi].col = c.map(|x| x as f32);
                    }
                }
                push_ranges(&mut dirty, rd.verts.iter().map(|&(vi, _)| vi));
            }
            if !rd.dyntex.is_empty() {
                let init = &mut state[r].1;
                init.resize(rd.dyntex.len(), false);
                push_ranges(&mut dirty, dyntex_tick_room(rd, init, &mut verts, frame.frac80).into_iter());
            }
        }
        for (a, b) in dirty {
            queue.write_buffer(&self.vbuf, (a * std::mem::size_of::<Vertex>()) as u64, bytemuck::cast_slice(&verts[a..b]));
        }
    }

    /// Every opaque room (the portal slots' rooms in draw order, each in its
    /// box; with none, every room in node order).
    pub fn draw_opaque<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, combiner: &'a Combiner, frame: Option<&BgFrame>) {
        self.bind(rp);
        match frame {
            None => {
                for &d in &self.all {
                    if !self.is_xlu(d) {
                        self.draw_one(rp, combiner, d);
                    }
                }
            }
            Some(f) => {
                for s in f.portals.draw_order() {
                    if let Some(rd) = self.rooms.get(s.roomnum as usize) {
                        set_scissor(rp, &s, f);
                        for &d in &rd.opa {
                            self.draw_one(rp, combiner, d);
                        }
                    }
                }
                rp.set_scissor_rect(0, 0, f.target[0], f.target[1]);
            }
        }
    }

    /// Every translucent room, the slots in reverse draw order, each layer's
    /// blocks by the camera's side of their planes.
    pub fn draw_xlu<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, combiner: &'a Combiner, frame: Option<&BgFrame>, campos: Vec3) {
        self.bind(rp);
        match frame {
            None => {
                for &d in &self.all {
                    if self.is_xlu(d) {
                        self.draw_one(rp, combiner, d);
                    }
                }
            }
            Some(f) => {
                for s in f.portals.draw_order().iter().rev() {
                    if let Some(rd) = self.rooms.get(s.roomnum as usize) {
                        set_scissor(rp, s, f);
                        for d in xlu_order(rd, &self.draws, campos) {
                            self.draw_one(rp, combiner, d);
                        }
                    }
                }
                rp.set_scissor_rect(0, 0, f.target[0], f.target[1]);
            }
        }
    }

    /// Both passes, for a view with nothing between them (the stage snapshot).
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, combiner: &'a Combiner, frame: Option<&BgFrame>, campos: Vec3) {
        self.draw_opaque(rp, combiner, frame);
        self.draw_xlu(rp, combiner, frame, campos);
    }

    fn is_xlu(&self, d: usize) -> bool {
        self.rooms.iter().any(|r| r.xlu.contains(&d))
    }

    fn bind<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.set_bind_group(0, &self.slot.bind, &[]);
    }

    fn draw_one<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, combiner: &'a Combiner, d: usize) {
        let d = &self.draws[d];
        let Some(pipe) = combiner.pipeline(d.key) else { return };
        rp.set_pipeline(pipe);
        rp.set_bind_group(1, &self.materials[d.material].bind, &[]);
        rp.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, 0..1);
    }
}

/// `bg_scissor_within_viewport_f` (`bg.c:2208`): the slot's box, scaled to
/// the target.
///
/// `// SUBST:` PD scissors in its own pixels / at a render scale above 1 the
/// right and bottom edges reach one PD pixel further, less one target pixel:
/// the portal box pads its edges by half a PD pixel, which at a higher
/// resolution no longer covers the columns a neighbouring wall starts short of,
/// and the sky showed through. At 1× the box is PD's.
fn set_scissor(rp: &mut wgpu::RenderPass<'_>, s: &DrawSlot, f: &BgFrame) {
    let x0 = ((s.rect.xmin.max(0) as f32) * f.scale[0]).floor() as u32;
    let y0 = ((s.rect.ymin.max(0) as f32) * f.scale[1]).floor() as u32;
    let x1 = (((s.rect.xmax.max(0) as f32 + 1.0) * f.scale[0]).ceil() as u32).saturating_sub(1);
    let y1 = (((s.rect.ymax.max(0) as f32 + 1.0) * f.scale[1]).ceil() as u32).saturating_sub(1);
    let (x0, y0) = (x0.min(f.target[0]), y0.min(f.target[1]));
    let (x1, y1) = (x1.min(f.target[0]), y1.min(f.target[1]));
    rp.set_scissor_rect(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0));
}

/// A room's translucent draws in `bg_render_room_pass`'s order.
fn xlu_order(rd: &RoomDraws, draws: &[Draw], campos: Vec3) -> Vec<usize> {
    let Some(tree) = &rd.xlu_tree else { return rd.xlu.clone() };
    let mut leaves = Vec::new();
    fn walk(item: &Option<TreeItem>, campos: Vec3, out: &mut Vec<usize>) {
        match item {
            None => {}
            Some(TreeItem::Leaf { leaf }) => out.push(*leaf),
            Some(TreeItem::Plane { plane, a, b }) => {
                // sum = normal · (point − campos); < 0: child first (bg.c:3190).
                let p = Vec3::new(plane[0], plane[1], plane[2]);
                let n = Vec3::new(plane[3], plane[4], plane[5]);
                let d = p - campos;
                let sum = n.x * d.x + n.y * d.y + n.z * d.z;
                if sum < 0.0 {
                    walk(a, campos, out);
                    walk(b, campos, out);
                } else {
                    walk(b, campos, out);
                    walk(a, campos, out);
                }
            }
        }
    }
    for item in tree {
        walk(item, campos, &mut leaves);
    }
    let mut out = Vec::with_capacity(rd.xlu.len());
    for leaf in leaves {
        out.extend(rd.xlu.iter().copied().filter(|&d| draws[d].leaf == leaf));
    }
    out
}

/// Merge vertex indices into contiguous ranges to upload.
fn push_ranges(out: &mut Vec<(usize, usize)>, idx: impl Iterator<Item = usize>) {
    for i in idx {
        match out.last_mut() {
            Some((_, b)) if *b == i => *b = i + 1,
            _ => out.push((i, i + 1)),
        }
    }
}

/// `room_highlight` (`dlights.c:1626`): a room's colour table at brightness
/// `br_settled_regional` plus the flash `extra`. A colour brighter than the
/// room is scaled down to it; the flash adds to every channel, capped so the
/// brightest reaches 285 (PD's `extra` stays capped for the colours after, a
/// quirk kept); a black colour takes the flash only when `black_flashes` (the
/// light op at 0 or the lights off).
pub fn room_highlight(base: &[[u8; 4]], alpha_only: &[bool], br_settled_regional: u8, flash: i32, black_flashes: bool) -> Vec<[u8; 4]> {
    let br = br_settled_regional as i32;
    let mut extra = flash;
    base.iter()
        .enumerate()
        .map(|(i, src)| {
            if alpha_only.get(i).copied().unwrap_or(false) {
                let a = (src[3] as f32 * (1.0 / 255.0 * br as f32)) as i32;
                return [src[0], src[1], src[2], a as u8];
            }
            let (tmpr, tmpg, tmpb) = (src[0] as i32, src[1] as i32, src[2] as i32);
            let mut max = tmpr.max(tmpg).max(tmpb);
            let mult = if max > br { br as f32 / max as f32 } else { 1.0 };
            let mut red = (tmpr as f32 * mult) as i32;
            let mut green = (tmpg as f32 * mult) as i32;
            let mut blue = (tmpb as f32 * mult) as i32;
            max = (max as f32 * mult) as i32;
            if extra + max > 285 {
                extra = 285 - max;
            }
            if red + green + blue != 0 || black_flashes {
                red += extra;
                green += extra;
                blue += extra;
            }
            [red.clamp(0, 255) as u8, green.clamp(0, 255) as u8, blue.clamp(0, 255) as u8, src[3]]
        })
        .collect()
}

fn dyntex_kind(name: &str) -> Option<DyntexKind> {
    // dyntex_set_current_type: the power juice and rings run as rivers off
    // the Attack Ship; the teleportal needs Deep Sea's flags (never set here).
    match name {
        "river" | "powerjuice" | "powerring" => Some(DyntexKind::River),
        "monitor" => Some(DyntexKind::Monitor),
        "ocean" => Some(DyntexKind::Ocean),
        "arrows" => Some(DyntexKind::Arrows),
        _ => None,
    }
}

/// `dyntex_tick_room` (`dyntex.c:150`): each kind's vertices' s,t from their
/// base and the 80-second clock, as texels (`U = s × scale >> 16`, S10.5).
/// Returns the vertices it moved.
///
/// A kind is initialised on its first tick: a base reaching past ±0x5d00 is
/// shifted by 0x2000 back towards 0. PD's loop bug, kept in effect: the
/// initialisation reuses the outer loop's counter, so a tick that initialises
/// a kind moves nothing more (`dyntex.c:171`).
fn dyntex_tick_room(rd: &RoomDraws, initialised: &mut [bool], verts: &mut [Vertex], frac80: f32) -> Vec<usize> {
    let mut moved = Vec::new();
    for (gi, g) in rd.dyntex.iter().enumerate() {
        if !initialised[gi] {
            initialised[gi] = true;
            break;
        }
        let (adds, addt) = dyntex_base_shift(g);
        for &(vi, st, sc) in &g.verts {
            let (s0, t0) = ((st[0] + adds) as i16, (st[1] + addt) as i16);
            let (s, t) = match g.kind {
                DyntexKind::River => {
                    let tmp = ((frac80 * 10.0 * 4096.0) as i32 % 4096) as i16;
                    (s0, t0.wrapping_add(tmp))
                }
                DyntexKind::Monitor => {
                    let tmp = ((frac80 * 4.0 * 4096.0) as i32 % 4096) as i16;
                    (s0, t0.wrapping_sub(tmp))
                }
                DyntexKind::Ocean => {
                    // ripsize 65, modula 22 (dyntex.c:103).
                    let f24 = frac80 * 5.0;
                    let a = (((t0 as i32 % 22) as f32 / 22.0 + f24) * pd_core::math::baddtor(360.0)).sin();
                    let t = t0.wrapping_add((a * 65.0) as i16);
                    let b = ((((s0 as i32 + 22) % 22) as f32 / 22.0 + f24) * pd_core::math::baddtor(360.0)).cos();
                    (s0.wrapping_add((b * 65.0) as i16), t)
                }
                DyntexKind::Arrows => {
                    let tmp = (((1.0 - frac80) * 60.0 * 8.0) as i32 % 8) * 256;
                    (s0.wrapping_add(tmp as i16), t0)
                }
            };
            // fast3d's U = s * scale >> 16, kept as a short (as the exporter does).
            let u = |v: i16, scale: u32| ((v as i32 * scale as i32) >> 16) as i16 as f32 / 32.0;
            verts[vi].uv = [u(s, sc[0]), u(t, sc[1])];
            moved.push(vi);
        }
    }
    moved
}

/// The shift `dyntex_tick_room`'s initialisation gives a kind's bases.
fn dyntex_base_shift(g: &DyntexGroup) -> (i32, i32) {
    let (mut mins, mut maxs, mut mint, mut maxt) = (32767, -32766, 32767, -32766);
    for &(_, st, _) in &g.verts {
        mins = mins.min(st[0]);
        mint = mint.min(st[1]);
        maxs = maxs.max(st[0]);
        maxt = maxt.max(st[1]);
    }
    let (mut adds, mut addt) = (0, 0);
    if mins < -0x5d00 {
        adds = 0x2000;
    }
    if mint < -0x5d00 {
        addt = 0x2000;
    }
    if maxs > 0x5d00 {
        adds = -0x2000;
    }
    if maxt > 0x5d00 {
        addt = -0x2000;
    }
    (adds, addt)
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

    /// The textured BG and the collision tiles are the same building: the BG
    /// covers every floor tile, and every texture it samples is in the pool.
    #[test]
    fn the_textured_bg_matches_the_collision_tiles() {
        let a = assets();
        for code in pd_sim::stage::ARENAS {
            let dir = a.stage(code);
            let def = ModelDef::load_file(&a, &dir.join("bg.json"), &dir.join("bg.bin")).unwrap();
            let stage = Stage::load(&a, code).unwrap();
            // The floors: Warehouse has one invisible wall past its BG (room 47).
            let (lo, hi) = stage.geom.polys.iter().filter(|p| p.floor).flat_map(|p| p.verts.iter()).fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
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
            if code == "ref" {
                assert_eq!(tris, 2559);
            }
            // The BG may reach past the tiles (scenery nobody walks on), never short of them.
            assert!(bmin.cmple(lo + 1.0).all() && bmax.cmpge(hi - 1.0).all(), "{code}: BG {bmin} {bmax} vs tiles {lo} {hi}");
            for m in &def.materials {
                if let Some(t) = &m.texture {
                    let info = def.textures.get(&t.id).unwrap_or_else(|| panic!("{code}: texture {:#x} not listed", t.id));
                    assert!(a.path(&info.file).exists(), "{} missing", info.file);
                }
            }
        }
    }

    /// `room_highlight`: at full brightness nothing changes; a room at 128
    /// scales its bright colours down to 128; a flash adds to every channel of
    /// a lit colour, black only with the lights at 0; alpha-only colours scale
    /// their alpha.
    #[test]
    fn room_highlight_scales_and_flashes() {
        let base = [[255u8, 128, 0, 255], [0, 0, 0, 255], [60, 60, 60, 200]];
        let alpha = [false, false, true];
        let full = super::room_highlight(&base, &alpha, 255, 0, false);
        assert_eq!(full[0], base[0]);
        assert_eq!(full[2], [60, 60, 60, 200]);
        let dim = super::room_highlight(&base, &alpha, 128, 0, false);
        assert_eq!(dim[0], [128, 64, 0, 255]);
        assert_eq!(dim[2], [60, 60, 60, 100]);
        let flash = super::room_highlight(&base, &alpha, 128, 50, false);
        assert_eq!(flash[0], [178, 114, 50, 255]);
        assert_eq!(flash[1], [0, 0, 0, 255], "black takes no flash");
        let off = super::room_highlight(&base, &alpha, 128, 50, true);
        assert_eq!(off[1], [50, 50, 50, 255]);
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
        r.render(&gpu.queue, &mut enc, &t.view, &t.depth.as_ref().unwrap().1, &view, None);
        gpu.queue.submit(Some(enc.finish()));
        let px = t.read_rgba8(&gpu.device, &gpu.queue);
        let sky = [2u8, 0, 0];
        let bg = px.chunks(4).filter(|p| p[..3] != sky).count();
        assert!(bg > 320 * 220 * 3 / 4, "only {bg} BG pixels");
    }
}
