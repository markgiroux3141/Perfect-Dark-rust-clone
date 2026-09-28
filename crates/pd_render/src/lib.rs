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
//! 1. the world pass: the BG (in x-ray, the eraser's triangles on black), the
//!    world's objects, then the world's effects depth-tested against them
//!    (bullet holes, smoke, explosions and N-Bomb domes back to front, the
//!    sentries' tracers and flashes, sparks);
//! 2. the gun pass: the z-buffer cleared and the gun's own projection (near 1.5,
//!    far 1000, `vi0000aca4`), this player's tracers, each hand's loaded rocket,
//!    gun then hand model, the casings, then the N-Bomb's overlay; none of it in
//!    x-ray, nor riding a Slayer rocket;
//! 3. the 2D layer: the sight and the gun HUD, drawn on the CPU in PD pixels;
//! 4. `lv_render`'s framebuffer effects over all of it ([`post`]).

pub mod bg;
pub mod fx;
pub mod hud;
pub mod models;
pub mod post;
pub mod view;
pub mod xray;

use engine::gpu::RenderTarget;
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
    pub post: post::PostRenderer,
    assets: Option<AssetDir>,
    fonts: Option<Fonts>,
    text: TextState,
    /// The last frame's 2D layer (kept for `pd_snapshot`, and for the N64
    /// video chain, which composites it itself).
    pub hud_gfx: Gfx,
    /// The N64 video chain's switches (`n64::gpu::video`): the RDP's 3-point
    /// texture filter; a texture the world's depth is copied into before the
    /// gun pass clears it; and whether the 2D layer is drawn into the frame
    /// (off: the chain lays it on).
    pub three_point: bool,
    pub world_depth_copy: Option<wgpu::Texture>,
    pub hud_in_frame: bool,
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
            post: post::PostRenderer::new(device, color_format),
            assets: None,
            fonts: None,
            text: TextState::default(),
            hud_gfx: hud::layer(view::VIEW_W as usize, view::VIEW_H as usize),
            three_point: false,
            world_depth_copy: None,
            hud_in_frame: true,
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

    /// The loaded stage's z range (`env->near`, `env->far`).
    pub fn z_range(&self) -> (f32, f32) {
        // SUBST: every view PD draws has a stage / a test stage (the firing
        // range) has none, so it takes the arenas' (15, 10000) from
        // `g_NoFogEnvironments`: a 1 m near plane would clip a thrown grenade.
        self.bg.as_ref().map_or((15.0, 10000.0), |b| (b.env.near, b.env.far))
    }

    /// The clear colour: the stage's sky, or the old gun tool's grey for a test
    /// stage (its (0.12, 0.13, 0.15) went through an sRGB target).
    fn sky(&self) -> [f64; 3] {
        self.bg.as_ref().map_or([0.38, 0.40, 0.43], |b| b.sky())
    }

    /// The BG alone into `color` + `depth` (same size), cleared first.
    pub fn render(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, view: &View) {
        if let Some(bg) = &self.bg {
            bg.prepare(queue, view, self.three_point);
        }
        let mut rp = world_pass(encoder, color, depth, self.sky());
        if let Some(bg) = &self.bg {
            bg.draw(&mut rp, &self.combiner);
        }
    }

    /// Player `pi`'s whole frame into `target` (its colour texture is copied
    /// for the framebuffer effects, so it needs `COPY_SRC`, and its depth).
    #[allow(clippy::too_many_arguments)]
    pub fn render_player(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &RenderTarget, world: &World, pi: usize) {
        let Some(p) = world.players.get(pi) else { return };
        let Some(assets) = self.assets.clone() else {
            log::warn!("render_player before load_stage/load_assets");
            return;
        };
        let Some((_, depth)) = &target.depth else {
            log::warn!("render_player: the target has no depth");
            return;
        };
        let color = &target.view;
        let (znear, zfar) = self.z_range();
        // SUBST: vi_shake moves the displayed picture, 2D layer included / the
        // 3D passes move and the HUD stays put.
        let view = View::for_player(p, znear, zfar).with_shake(world.vi.offset);
        let w2e = view.world_to_eye();
        let world_proj = view.projection();
        let gun_proj = view.gun_projection();
        let brightness = world.lights.brightness(p.floorroom);
        let gun = &p.gun;
        let shadecol = gun.p.gunshadecol;
        let env = [shadecol[0] as f32 / 255.0, shadecol[1] as f32 / 255.0, shadecol[2] as f32 / 255.0, shadecol[3] as f32 / 255.0];
        // VISIONMODE_XRAY: the eraser's colours, and bgun_render returns at
        // once (`bondgun.c:8191`). CAMERAMODE_THIRDPERSON (riding a Slayer
        // rocket): player_render_hud draws no gun, overlay or HUD (`player.c:4448`).
        let xray = (p.visionmode == VISIONMODE_XRAY).then_some(&p.eraser);
        let riding = p.cameramode == CAMERAMODE_THIRDPERSON;
        let draw_gun = xray.is_none() && !riding;

        // The effects.
        let cam = fx::FxCam { pos: p.cam.pos(), look: p.look, fovy: view.fovy, projection: p.cam.projection, world_to_screen: p.cam.world_to_screen, brightness };
        let mut world_fx = fx::world_fx(world, &cam, xray);
        if self.bg.is_none() && xray.is_none() {
            // SUBST: a stage's BG is its textured display lists / a test stage
            // (the firing range) has none, so its polygons are drawn flat.
            world_fx.splice(0..0, fx::fixture_geometry(&world.stage.geom));
        }
        let gun_fx = fx::gun_fx(p, cam.pos);
        // nbomb_render_overlay (`player.c:4475`), after the gun.
        let overlay_fx: Vec<fx::FxBatch> = if riding { Vec::new() } else { fx::nbomb_overlay(&world.props.nbombs, cam.pos, world.frac20).into_iter().collect() };
        self.fx.prepare(device, queue, &assets, &world_fx, &gun_fx, &overlay_fx, world_proj * w2e, gun_proj * w2e, env);

        let res = &world.res;
        // The world's objects (`obj_render`, `propobj.c:12690`), in the world pass.
        // SUBST: PD shades an object by its room's light and `obj->shadecol`
        // (`colour[3] -= obj_get_brightness`) / we light it as the gun is, from
        // the player's room, until the stage lighting port (M6) gives objects
        // their rooms.
        let obj_lights = gun_lights(brightness, false);
        let held: Vec<u32> = gun.hands.iter().filter_map(|h| h.rocket).collect();
        let mut objdraws: Vec<Draw> = Vec::new();
        for o in &world.props.objs {
            // A loaded rocket is drawn with the gun (`bgun_render`); a Slayer
            // rider doesn't see their own rocket (`propobj.c:12715`).
            if o.flags & OBJFLAG_HELDROCKET != 0 || held.contains(&o.id) || o.flags2 & OBJFLAG2_INVISIBLE != 0 {
                continue;
            }
            if p.visionmode == VISIONMODE_SLAYERROCKET && p.slayerrocket == Some(o.id) {
                continue;
            }
            let mut frame = lit_frame(world_proj, p.look, p.up, obj_lights, env);
            let mut xlu = false;
            if let Some(e) = xray {
                // In x-ray: the flat eraser colour through the fog at full
                // weight, alpha 0..128 as the env alpha (BONDGUN_OBJ_XLU).
                let Some(c) = xray::obj_colour(e, o.pos) else { continue };
                frame.flat = [c[0], c[1], c[2], 1.0];
                frame.misc[0] = c[3];
                xlu = true;
            }
            let joints = o.init_matrices().iter().map(|m| w2e * *m).collect();
            objdraws.push((o.def.clone(), Vec::new(), joints, frame, None, xlu));
        }

        // The guns, the hands, the loaded rockets and the casings (`bgun_render`,
        // `casings_render`).
        let mut defs: Vec<Draw> = Vec::new();
        // bgun_render (`bondgun.c:8305`): a cloaked player's gun goes
        // see-through, env alpha 65 + 0.745 × the cloak's alpha.
        let cloak_alpha = p.cloak.alpha();
        let cloak = (cloak_alpha < 255).then(|| (65.0 + (cloak_alpha as f32 * 0.745_098_05).trunc()) / 255.0);
        for (h, hand) in gun.hands.iter().enumerate() {
            if !hand.visible || !draw_gun {
                continue;
            }
            let Some(gm) = &hand.gunmodel else { continue };
            let weaponnum = gun.bgun_get_weapon_num(h);
            let lights = gun_lights(brightness, res.gset.has_flag(hand.weaponnum, WEAPONFLAG_BRIGHTER));
            let mut colour = env;
            let mut envcol = env;
            if hand.weaponnum == WEAPON_MAULER {
                let e = rgba(colour_blend(0xff00007f, u32::from_be_bytes(shadecol), (hand.matmot1 * 50.0) as u32));
                envcol = e;
            }
            let mut frame = lit_frame(gun_proj, p.look, p.up, lights, envcol);
            if let Some(a) = cloak {
                // fogcolour = envcolour; envcolour = 65 + alpha; colour = envcolour.
                frame.misc[0] = a;
                colour = envcol;
            }
            let cull = res.gset.has_flag(weaponnum, WEAPONFLAG_DUALFLIP).then_some(if h == HAND_RIGHT { Cull::Back } else { Cull::Front });
            // The launcher's rocket, from the muzzle (`bondgun.c:8321`).
            if let Some(o) = hand.rocket.and_then(|id| world.props.get(id)) {
                let inv = o.root_matrix().inverse();
                let joints = o.init_matrices().iter().map(|m| hand.muzzlemat * inv * *m).collect();
                defs.push((o.def.clone(), Vec::new(), joints, frame, None, cloak.is_some()));
            }
            defs.push((gm.def.clone(), gm.vis.clone(), gm.matrices.clone(), frame, cull, cloak.is_some()));
            if let Some(hm) = &hand.handmodel {
                let mut hframe = lit_frame(gun_proj, p.look, p.up, lights, colour);
                hframe.misc = frame.misc;
                defs.push((hm.def.clone(), hm.vis.clone(), gm.matrices.clone(), hframe, cull, cloak.is_some()));
            }
        }
        let casing_lights = gun_lights(brightness, false);
        for c in world.fx.casings.iter().filter(|_| draw_gun) {
            let Ok(def) = res.models.get(pd_sim::fx::casing::CART_MODELS[c.model]) else { continue };
            defs.push((def, Vec::new(), vec![w2e * c.world_matrix()], lit_frame(gun_proj, p.look, p.up, casing_lights, env), None, false));
        }
        for (def, ..) in objdraws.iter().chain(&defs) {
            if let Err(e) = self.models.load(device, queue, &mut self.combiner, &assets, def) {
                log::warn!("model {}: {e}", def.stem);
            }
        }
        for hand in gun.hands.iter().filter(|_| draw_gun) {
            if let Some(gm) = hand.gunmodel.as_ref().filter(|_| hand.visible) {
                if world.players.len() == 1 {
                    self.models.slide_laser_liquid(queue, &gm.def.stem, world.lv.lvupdate240);
                }
                self.models.jitter_star(queue, &gm.def.stem);
            }
        }
        if self.three_point {
            for d in objdraws.iter_mut().chain(defs.iter_mut()) {
                d.3.misc[1] = 1.0;
            }
        }
        self.models.begin_frame();
        let obj_cmds = self.models.prepare(device, queue, &mut self.combiner, &to_instances(&objdraws));
        let cmds = self.models.prepare(device, queue, &mut self.combiner, &to_instances(&defs));

        // The 2D layer.
        self.hud_gfx = hud::layer(p.cam.c_screenwidth as usize, p.cam.c_screenheight as usize);
        if let Some(fonts) = self.fonts.as_ref().filter(|_| !riding) {
            let hin = hud::HudIn {
                gun,
                gset: &res.gset,
                view: [p.cam.c_screenleft as i32, p.cam.c_screentop as i32, p.cam.c_screenwidth as i32, p.cam.c_screenheight as i32],
                playercount: world.players.len(),
                isdead: false,
                sighton: p.insightaimmode,
                hasprop: world.lookingatprop.get(pi).copied().flatten().is_some(),
                speedpilltime: world.speedpill.time,
                options: world.setup.players.get(pi).map_or(pd_core::mp::MatchPlayer::DEFAULT_OPTIONS, |m| m.options),
                zoominfovy: p.zoominfovy,
            };
            let mut t = TextCtx { gfx: &mut self.hud_gfx, ts: &mut self.text, fonts, frac20: world.frac20 };
            hud::draw(&mut t, &hin);
        }
        self.overlay.prepare(device, queue, &self.hud_gfx);

        if let Some(bg) = self.bg.as_ref().filter(|_| xray.is_none()) {
            bg.prepare(queue, &view, self.three_point);
        }
        {
            // sky_render: the fill colour, black in x-ray (`sky.c:267`).
            let sky = if xray.is_some() { [0.0; 3] } else { self.sky() };
            let mut rp = world_pass(encoder, color, depth, sky);
            if let Some(bg) = self.bg.as_ref().filter(|_| xray.is_none()) {
                bg.draw(&mut rp, &self.combiner);
            }
            self.fx.draw_some(&mut rp, fx::FxPass::World, |k| k.before_objects());
            self.models.draw(&mut rp, &self.combiner, &obj_cmds);
            self.fx.draw_some(&mut rp, fx::FxPass::World, |k| !k.before_objects());
        }
        if let Some(t) = &self.world_depth_copy {
            let (dt, _) = target.depth.as_ref().unwrap();
            encoder.copy_texture_to_texture(dt.as_image_copy(), t.as_image_copy(), wgpu::Extent3d { width: target.width, height: target.height, depth_or_array_layers: 1 });
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
            self.fx.draw(&mut rp, fx::FxPass::Overlay);
        }
        if self.hud_in_frame {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pd-hud"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: color, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.overlay.draw(&mut rp);
        }
        // lv_render's framebuffer effects, over the HUD (`lv.c:1439`).
        self.post.draw(device, queue, encoder, target, &p.viewfx, p.cam.c_screenheight, p.cam.c_screenwidth, world.frac20);
    }
}

/// A model to draw: (model, visibility, joints (eye space), frame, cull, xlu).
type Draw = (std::sync::Arc<pd_core::model::ModelDef>, Vec<bool>, Vec<Mat4>, FrameUniform, Option<Cull>, bool);

fn to_instances(d: &[Draw]) -> Vec<models::Instance<'_>> {
    d.iter()
        .map(|(def, vis, joints, frame, cull, xlu)| models::Instance { def, vis: (!vis.is_empty()).then_some(vis.as_slice()), joints, frame: *frame, cull: *cull, xlu: *xlu })
        .collect()
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
        r.render_player(&gpu.device, &gpu.queue, &mut enc, &t, &w, 0);
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
