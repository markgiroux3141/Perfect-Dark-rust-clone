//! Perfect Dark on the GPU. It reads `pd_sim` views and never writes to the world.
//! Every model (the BG, guns, hands, simulant bodies and heads, props) goes through
//! the one `n64::gpu` combiner path, so simulants are lit and textured the way the
//! guns and the level are. In the spike match they were drawn flat by the old
//! engine's glTF path.
//!
//! One `View` per human player (viewport, camera, fov), so split-screen needs no
//! special case.
//!
//! A player's frame, in PD's order (`lv_render` → `player_render` →
//! `bgun_render` → the HUD):
//! 1. the world pass: the BG, then the world's effects depth-tested against it
//!    (boards, bullet holes, smoke and explosions back to front, sparks);
//! 2. the gun pass: the z-buffer cleared and the gun's own projection (near 1.5,
//!    far 1000, `vi0000aca4`), this player's tracers, each hand's gun then hand
//!    model, then the casings;
//! 3. the 2D layer: the sight and the gun HUD, drawn on the CPU in PD pixels.

pub mod bg;
pub mod fx;
pub mod hud;
pub mod models;
pub mod post;
pub mod view;
pub mod xray;

use glam::{Mat4, Vec3};
use n64::gpu::{Combiner, FrameUniform};
use n64::rdp::{rgba, Cull, Gfx};
use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_core::text::{colour_blend, Fonts, TextCtx, TextState};
use pd_sim::world::World;

pub use view::View;

/// The depth format every pass uses.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Draws a match: the combiner pipelines, the loaded stage, the models, the
/// effects and the 2D layer.
pub struct Renderer {
    pub combiner: Combiner,
    pub bg: Option<bg::StageBg>,
    pub models: models::ModelRenderer,
    pub fx: fx::FxRenderer,
    pub overlay: hud::HudOverlay,
    assets: Option<AssetDir>,
    fonts: Option<Fonts>,
    text: TextState,
    /// The last frame's 2D layer (kept for `pd_snapshot`).
    pub hud_gfx: Gfx,
}

/// `lights_set_for_room`'s light from a room's brightness (`dlights.c:303`),
/// or `var80070090` for a `WEAPONFLAG_BRIGHTER` weapon (`bondgun.c:162`):
/// (ambient, diffuse, direction in the display list's bytes).
fn gun_lights(brightness: f32, brighter: bool) -> (f32, f32, Vec3) {
    if brighter {
        (150.0, 255.0, Vec3::new(-78.0, 77.0, 46.0))
    } else {
        ((brightness * 0.588_235_3).floor(), brightness, Vec3::new(77.0, 77.0, 46.0))
    }
}

fn lit_frame(proj: Mat4, look: Vec3, up: Vec3, lights: (f32, f32, Vec3), envcol: [f32; 4]) -> FrameUniform {
    let (amb, dif, dir) = lights;
    let mut f = FrameUniform::new(proj).with_lookat(look, up);
    f.ambient = [amb, amb, amb, 0.0];
    f.diffuse = [dif, dif, dif, 0.0];
    f.light_dir = (dir / 127.0).extend(0.0).to_array();
    f.envcol = envcol;
    f
}

impl Renderer {
    /// `color_format` must not be sRGB: the combiner writes display-space values.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat) -> Renderer {
        Renderer {
            combiner: Combiner::new(device, queue, color_format, DEPTH_FORMAT),
            bg: None,
            models: models::ModelRenderer::default(),
            fx: fx::FxRenderer::new(device, queue, color_format, DEPTH_FORMAT),
            overlay: hud::HudOverlay::new(device, color_format),
            assets: None,
            fonts: None,
            text: TextState::default(),
            hud_gfx: hud::layer(view::VIEW_W as usize, view::VIEW_H as usize),
        }
    }

    /// Load the stage's BG, and the fonts and textures a match draws with.
    pub fn load_stage(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetDir, code: &str) -> Result<(), String> {
        self.bg = Some(bg::StageBg::load(device, queue, &mut self.combiner, assets, code)?);
        self.load_assets(assets)
    }

    /// The fonts and the asset directory the models and effects load from
    /// (for a view without a textured stage, like the firing range).
    pub fn load_assets(&mut self, assets: &AssetDir) -> Result<(), String> {
        if self.fonts.is_none() {
            self.fonts = Some(Fonts::load(assets)?);
        }
        self.assets = Some(assets.clone());
        Ok(())
    }

    /// The loaded stage's z range, or PD's title-screen default (100, 10000).
    pub fn z_range(&self) -> (f32, f32) {
        self.bg.as_ref().map_or((100.0, 10000.0), |b| (b.env.near, b.env.far))
    }

    /// The clear colour: the stage's sky, or the old gun tool's grey for a test
    /// stage (its (0.12, 0.13, 0.15) went through an sRGB target).
    fn sky(&self) -> [f64; 3] {
        self.bg.as_ref().map_or([0.38, 0.40, 0.43], |b| b.sky())
    }

    /// The BG alone into `color` + `depth` (same size), cleared first.
    pub fn render(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, view: &View) {
        if let Some(bg) = &self.bg {
            bg.prepare(queue, view);
        }
        let mut rp = world_pass(encoder, color, depth, self.sky());
        if let Some(bg) = &self.bg {
            bg.draw(&mut rp, &self.combiner);
        }
    }

    /// Player `pi`'s whole frame into `color` + `depth` (same size).
    #[allow(clippy::too_many_arguments)]
    pub fn render_player(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, world: &World, pi: usize) {
        let Some(p) = world.players.get(pi) else { return };
        let Some(assets) = self.assets.clone() else {
            log::warn!("render_player before load_stage/load_assets");
            return;
        };
        let (znear, zfar) = self.z_range();
        // SUBST: vi_shake moves the displayed picture, 2D layer included / the
        // 3D passes move and the HUD stays put.
        let view = View::for_player(p, znear, zfar).with_shake(world.vi.offset);
        let w2e = view.world_to_eye();
        let gun_proj = view.gun_projection();
        let brightness = world.lights.brightness(p.floorroom);
        let gun = &p.gun;
        let shadecol = gun.p.gunshadecol;
        let env = [shadecol[0] as f32 / 255.0, shadecol[1] as f32 / 255.0, shadecol[2] as f32 / 255.0, shadecol[3] as f32 / 255.0];

        // The effects.
        let cam = fx::FxCam { pos: p.cam.pos(), look: p.look, fovy: view.fovy, projection: p.cam.projection, world_to_screen: p.cam.world_to_screen, brightness };
        let mut world_fx = fx::world_fx(world, &cam);
        if self.bg.is_none() {
            // SUBST: a stage's BG is its textured display lists / a test stage
            // (the firing range) has none, so its polygons are drawn flat.
            world_fx.splice(0..0, fx::fixture_geometry(&world.stage.geom));
        }
        let gun_fx = fx::gun_fx(p, cam.pos);
        self.fx.prepare(device, queue, &assets, &world_fx, &gun_fx, view.projection() * w2e, gun_proj * w2e, env);

        // The guns, the hands and the casings (`bgun_render`, `casings_render`).
        let res = &world.res;
        let mut defs = Vec::new();
        for (h, hand) in gun.hands.iter().enumerate() {
            if !hand.visible {
                continue;
            }
            let Some(gm) = &hand.gunmodel else { continue };
            let weaponnum = gun.bgun_get_weapon_num(h);
            let lights = gun_lights(brightness, res.gset.has_flag(hand.weaponnum, WEAPONFLAG_BRIGHTER));
            let colour = env;
            let mut envcol = env;
            if hand.weaponnum == WEAPON_MAULER {
                let e = rgba(colour_blend(0xff00007f, u32::from_be_bytes(shadecol), (hand.matmot1 * 50.0) as u32));
                envcol = e;
            }
            // M5: a cloaked player's gun (MODELRENDERCONTEXT_BONDGUN_OBJ_XLU).
            let cull = res.gset.has_flag(weaponnum, WEAPONFLAG_DUALFLIP).then_some(if h == HAND_RIGHT { Cull::Back } else { Cull::Front });
            defs.push((gm.def.clone(), gm.vis.clone(), gm.matrices.clone(), lit_frame(gun_proj, p.look, p.up, lights, envcol), cull));
            if let Some(hm) = &hand.handmodel {
                defs.push((hm.def.clone(), hm.vis.clone(), gm.matrices.clone(), lit_frame(gun_proj, p.look, p.up, lights, colour), cull));
            }
        }
        let casing_lights = gun_lights(brightness, false);
        for c in &world.fx.casings {
            let Ok(def) = res.models.get(pd_sim::fx::casing::CART_MODELS[c.model]) else { continue };
            defs.push((def, Vec::new(), vec![w2e * c.world_matrix()], lit_frame(gun_proj, p.look, p.up, casing_lights, env), None));
        }
        for (def, ..) in &defs {
            if let Err(e) = self.models.load(device, queue, &mut self.combiner, &assets, def) {
                log::warn!("model {}: {e}", def.stem);
            }
        }
        for hand in &gun.hands {
            if let Some(gm) = hand.gunmodel.as_ref().filter(|_| hand.visible) {
                if world.players.len() == 1 {
                    self.models.slide_laser_liquid(queue, &gm.def.stem, world.lv.lvupdate240);
                }
                self.models.jitter_star(queue, &gm.def.stem);
            }
        }
        self.models.begin_frame();
        let instances: Vec<models::Instance> = defs
            .iter()
            .map(|(def, vis, joints, frame, cull)| models::Instance { def, vis: (!vis.is_empty()).then_some(vis.as_slice()), joints, frame: *frame, cull: *cull })
            .collect();
        let cmds = self.models.prepare(device, queue, &mut self.combiner, &instances);

        // The 2D layer.
        self.hud_gfx = hud::layer(p.cam.c_screenwidth as usize, p.cam.c_screenheight as usize);
        if let Some(fonts) = &self.fonts {
            let hin = hud::HudIn {
                gun,
                gset: &res.gset,
                view: [p.cam.c_screenleft as i32, p.cam.c_screentop as i32, p.cam.c_screenwidth as i32, p.cam.c_screenheight as i32],
                playercount: world.players.len(),
                isdead: false,
                sighton: p.insightaimmode,
                hasprop: world.lookingatprop.get(pi).copied().flatten().is_some(),
                speedpilltime: 0,
                options: world.setup.players.get(pi).map_or(pd_core::mp::MatchPlayer::DEFAULT_OPTIONS, |m| m.options),
                zoominfovy: p.zoominfovy,
            };
            let mut t = TextCtx { gfx: &mut self.hud_gfx, ts: &mut self.text, fonts, frac20: world.frac20 };
            hud::draw(&mut t, &hin);
        }
        self.overlay.prepare(device, queue, &self.hud_gfx);

        if let Some(bg) = &self.bg {
            bg.prepare(queue, &view);
        }
        {
            let mut rp = world_pass(encoder, color, depth, self.sky());
            if let Some(bg) = &self.bg {
                bg.draw(&mut rp, &self.combiner);
            }
            self.fx.draw(&mut rp, fx::FxPass::World);
        }
        {
            // bgun_render: the z-buffer cleared, the gun's projection.
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pd-gun"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.fx.draw(&mut rp, fx::FxPass::Gun);
            self.models.draw(&mut rp, &self.combiner, &cmds);
        }
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pd-hud"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.overlay.draw(&mut rp);
        }
    }
}

fn world_pass<'a>(encoder: &'a mut wgpu::CommandEncoder, color: &'a wgpu::TextureView, depth: &'a wgpu::TextureView, sky: [f64; 3]) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("pd-world"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: sky[0], g: sky[1], b: sky[2], a: 1.0 }), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pd_core::assets::AssetDir;
    use pd_core::mp::{MatchPlayer, MatchSetup};
    use pd_sim::player::PlayerInput;
    use pd_sim::stage::{fixtures, Stage, TileLevel};
    use pd_sim::world::{World, WorldRes};

    /// A player's whole frame on a headless GPU, in the firing range with the
    /// Falcon 2 up: the gun and hand cover the lower right, the HUD's gauges
    /// are green at the right edge, and the boards stand in the middle.
    /// Skipped when the machine has no GPU adapter.
    #[test]
    fn the_falcon_and_its_hud_render_in_the_firing_range() {
        let Ok(gpu) = engine::gpu::HeadlessGpu::new() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let a = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
        let stage = Stage::fixture("range", fixtures::firing_range(), &[fixtures::FIRING_RANGE_SPAWN]);
        let level = TileLevel::new(stage.geom.clone());
        let setup = MatchSetup { players: vec![MatchPlayer::default()], ..Default::default() };
        let mut w = World::new(setup, Arc::new(stage), Arc::new(level), Arc::new(WorldRes::load(&a).unwrap()), 1).unwrap();
        w.boards = fixtures::firing_range_boards();
        for _ in 0..200 {
            w.step(4, &[PlayerInput::default()]);
        }
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = crate::Renderer::new(&gpu.device, &gpu.queue, format);
        r.load_assets(&a).unwrap();
        let (tw, th) = (320, 220);
        let t = engine::gpu::RenderTarget::on_device(&gpu.device, tw, th, format, true);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.render_player(&gpu.device, &gpu.queue, &mut enc, &t.view, &t.depth.as_ref().unwrap().1, &w, 0);
        gpu.queue.submit(Some(enc.finish()));
        let px = t.read_rgba8(&gpu.device, &gpu.queue);
        let at = |x: u32, y: u32| &px[((y * tw + x) * 4) as usize..][..3];
        let bg = [97u8, 102, 110];
        let differs = |x0: u32, x1: u32, y0: u32, y1: u32| (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| at(x, y) != bg).count();
        // The gun and the hand, lower right of centre.
        let gun = (y_range(th, 0.6, 1.0)).map(|y| (160..260).filter(|&x| is_gun(at(x, y))).count()).sum::<usize>();
        assert!(gun > 400, "gun pixels: {gun}");
        // The magazine gauge: green blocks near the right edge.
        let green = (60..160).map(|y| (280..300).filter(|&x| {
            let p = at(x, y);
            p[1] > p[0] + 40 && p[1] > p[2]
        }).count()).sum::<usize>();
        assert!(green > 100, "gauge pixels: {green}");
        assert!(differs(0, tw, 0, th) > 0);
    }

    fn y_range(h: u32, a: f32, b: f32) -> std::ops::Range<u32> {
        (h as f32 * a) as u32..(h as f32 * b) as u32
    }

    /// Not the range's flat greys: the gun's blue steel or the glove.
    fn is_gun(p: &[u8]) -> bool {
        let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
        (b - r).abs() > 12 || (r - g).abs() > 12 || r.max(g).max(b) < 40
    }
}
