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

pub mod activemenu;
pub mod bg;
pub mod fx;
pub mod health;
pub mod hud;
pub mod hudmsg;
pub mod models;
pub mod post;
pub mod radar;
pub mod shield;
pub mod sky;
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
    /// The last view's 2D layer, the view's size (kept for `pd_snapshot`).
    pub hud_gfx: Gfx,
    /// Every view's 2D layer in PD's framebuffer (`render_views`): the N64
    /// video chain composites it itself.
    pub hud_frame: Gfx,
    /// A 2D layer in framebuffer pixels, drawn into and cut to each view.
    hud_scratch: Gfx,
    /// Each view's own target in split screen (`render_views`).
    views: Vec<RenderTarget>,
    /// The N64 video chain's switches (`n64::gpu::video`): the RDP's 3-point
    /// texture filter; a texture the world's depth is copied into before the
    /// gun pass clears it; and whether the 2D layer is drawn into the frame
    /// (off: the chain lays it on).
    pub three_point: bool,
    pub world_depth_copy: Option<wgpu::Texture>,
    pub hud_in_frame: bool,
    /// The menus' frame to lay over the HUD (premultiplied, PD's 320 × 220
    /// framebuffer with every player's dialogs in its view; empty for none):
    /// `lv_render` draws `menu_render` after the HUD, then the modal text over it.
    pub menu_layer: Vec<[f32; 4]>,
    /// A copy of its model per `DOORFLAG_0004` door (by object id), whose
    /// display list's vertices are rewritten every frame.
    door_models: std::collections::HashMap<u32, std::sync::Arc<pd_core::model::ModelDef>>,
    /// The sky pass (made on the first match view).
    sky: Option<sky::SkyRenderer>,
    /// The radar's ring (`TEXTURE_003C`) and the shield's shimmer
    /// (`TEXTURE_000D`), loaded with the fonts.
    radar_ring: Option<n64::rdp::Texture>,
    shield_tex: Option<n64::rdp::Texture>,
    color_format: wgpu::TextureFormat,
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
    f.fogcol = envcol;
    f
}

/// `prop_calculate_shade_colour`'s `scenario_highlight_room(prop->rooms[0])`
/// (`propobj.c:1647`): a prop in King of the Hill's hill or a Capture the Case
/// base takes the room's tint on its light.
fn tint_frame(f: &mut FrameUniform, tint: Option<[f32; 3]>) {
    if let Some(t) = tint {
        for (k, &m) in t.iter().enumerate() {
            f.ambient[k] *= m;
            f.diffuse[k] *= m;
        }
    }
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
            hud_frame: hud::layer(view::VIEW_W as usize, view::VIEW_H as usize),
            hud_scratch: hud::layer(view::VIEW_W as usize, view::VIEW_H as usize),
            views: Vec::new(),
            three_point: false,
            world_depth_copy: None,
            hud_in_frame: true,
            menu_layer: Vec::new(),
            door_models: std::collections::HashMap::new(),
            sky: None,
            radar_ring: None,
            shield_tex: None,
            color_format,
        }
    }

    /// Load the stage's BG, and the fonts and textures a match draws with.
    pub fn load_stage(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetDir, code: &str) -> Result<(), String> {
        self.bg = Some(bg::StageBg::load(device, queue, &mut self.combiner, assets, code)?);
        self.door_models.clear();
        self.load_assets(assets)
    }

    /// The fonts and the asset directory the models and effects load from
    /// (for a view without a textured stage, like the firing range).
    pub fn load_assets(&mut self, assets: &AssetDir) -> Result<(), String> {
        if self.fonts.is_none() {
            self.fonts = Some(Fonts::load(assets)?);
        }
        if self.radar_ring.is_none() {
            let (w, h, px) = assets.read_png(&assets.texture(0x003c))?;
            self.radar_ring = Some(n64::rdp::Texture::from_rgba8(w, h, &px, n64::rdp::Addr::Clamp, n64::rdp::Addr::Clamp));
        }
        if self.shield_tex.is_none() {
            let (w, h, px) = assets.read_png(&assets.texture(fx::TEX_SHIELD))?;
            self.shield_tex = Some(n64::rdp::Texture::from_rgba8(w, h, &px, n64::rdp::Addr::Wrap, n64::rdp::Addr::Wrap));
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

    /// The BG alone into `color` + `depth` (same size), cleared first; with
    /// `frame`, only the rooms its portals found, lit.
    pub fn render(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, color: &wgpu::TextureView, depth: &wgpu::TextureView, view: &View, frame: Option<&bg::BgFrame>) {
        if let Some(bg) = &self.bg {
            bg.prepare(queue, view, self.three_point, frame);
        }
        let mut rp = world_pass(encoder, color, depth, self.sky());
        if let Some(bg) = &self.bg {
            bg.draw(&mut rp, &self.combiner, frame, view.eye);
        }
    }

    /// The BG frame for player `pi` of `world` into a `w` × `h` target.
    pub fn bg_frame<'a>(world: &'a World, pi: usize, w: u32, h: u32) -> Option<bg::BgFrame<'a>> {
        let p = world.players.get(pi)?;
        Some(bg::BgFrame {
            portals: &p.portalview,
            lights: &world.lights,
            frac80: world.frac80,
            tints: world.scenario_highlighted_rooms(),
            scale: [w as f32 / p.cam.c_screenwidth, h as f32 / p.cam.c_screenheight],
            origin: [p.cam.c_screenleft, p.cam.c_screentop],
            target: [w, h],
        })
    }

    /// Every player's view laid into `frame`, a target of PD's framebuffer
    /// (320 × 220) at any scale: with one player the view is the frame; in
    /// split screen each view is drawn into its own target and copied to its
    /// rectangle (`player_get_viewport_*`), black between them. Each view is
    /// submitted before the next is prepared, since they share the uniform
    /// buffers (`encoder` is replaced by a fresh one each time). Leaves every
    /// view's 2D layer in [`Renderer::hud_frame`].
    pub fn render_views(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &RenderTarget, world: &World) {
        self.hud_frame = hud::layer(view::VIEW_W as usize, view::VIEW_H as usize);
        let n = world.players.len();
        if n <= 1 {
            self.render_player(device, queue, encoder, frame, world, 0);
            return;
        }
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pd-frame-clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let (sx, sy) = (frame.width as f32 / view::VIEW_W as f32, frame.height as f32 / view::VIEW_H as f32);
        let mut views = std::mem::take(&mut self.views);
        views.truncate(n);
        for (pi, p) in world.players.iter().enumerate() {
            let c = &p.cam;
            let x0 = ((c.c_screenleft * sx).round() as u32).min(frame.width);
            let y0 = ((c.c_screentop * sy).round() as u32).min(frame.height);
            let x1 = (((c.c_screenleft + c.c_screenwidth) * sx).round() as u32).min(frame.width);
            let y1 = (((c.c_screentop + c.c_screenheight) * sy).round() as u32).min(frame.height);
            let (w, h) = ((x1 - x0).max(1), (y1 - y0).max(1));
            if views.get(pi).is_none_or(|v| v.width != w || v.height != h || v.format != frame.format) {
                let v = RenderTarget::on_device(device, w, h, frame.format, true);
                if pi < views.len() {
                    views[pi] = v;
                } else {
                    views.push(v);
                }
            }
            self.render_player(device, queue, encoder, &views[pi], world, pi);
            encoder.copy_texture_to_texture(
                views[pi].color.as_image_copy(),
                wgpu::TexelCopyTextureInfo { texture: &frame.color, mip_level: 0, origin: wgpu::Origin3d { x: x0, y: y0, z: 0 }, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: w.min(frame.width - x0), height: h.min(frame.height - y0), depth_or_array_layers: 1 },
            );
            let done = std::mem::replace(encoder, device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pd-view") }));
            queue.submit(Some(done.finish()));
        }
        self.views = views;
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
        // lights_set_for_room(prop->rooms[0]): the player's room lights the gun.
        let brightness = world.lights.brightness(p.rooms.first().copied().or(p.floorroom));
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
        let cam = fx::FxCam { pos: p.cam.pos(), look: p.look, fovy: view.fovy, projection: p.cam.projection, world_to_screen: p.cam.world_to_screen, brightness, viewer: pi };
        let mut world_fx = fx::world_fx(world, &cam, xray);
        if self.bg.is_none() && xray.is_none() {
            // SUBST: a stage's BG is its textured display lists / a test stage
            // (the firing range) has none, so its polygons are drawn flat.
            world_fx.splice(0..0, fx::fixture_geometry(&world.stage.geom));
        }
        // chr_render_shield after each chr drawn: its shield's glow.
        if xray.is_none() {
            for (ci, c) in world.chrs.iter().enumerate().filter(|(_, c)| c.player != Some(pi) && c.onanyscreen) {
                let alpha = chr_render_alpha(c);
                if alpha > 0.0 {
                    world_fx.extend(shield::chr_shield_batch(world, ci, alpha));
                }
            }
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
        let mut door_verts: Vec<(String, pd_sim::props::door::DoorVerts)> = Vec::new();
        for o in &world.props.objs {
            // A loaded rocket is drawn with the gun (`bgun_render`); a Slayer
            // rider doesn't see their own rocket (`propobj.c:12715`).
            if o.flags & OBJFLAG_HELDROCKET != 0 || held.contains(&o.id) || o.flags2 & OBJFLAG2_INVISIBLE != 0 {
                continue;
            }
            if p.visionmode == VISIONMODE_SLAYERROCKET && p.slayerrocket == Some(o.id) {
                continue;
            }
            // A taken pickup is disabled until its last second (`obj_tick`).
            if o.is_gone() {
                continue;
            }
            // obj_render (`propobj.c:12692`): lost in a fog stage's fog, not drawn.
            let shade = obj_shade_mode(self.bg.as_ref(), w2e, o.pos, xray.is_some());
            if shade == ShadeMode::Xlu {
                continue;
            }
            let mut frame = lit_frame(world_proj, p.look, p.up, obj_lights, env);
            tint_frame(&mut frame, o.room.and_then(|r| world.scenario_highlight_room(r)));
            let mut xlu = false;
            // obj_render (`propobj.c:12701`): the last second of a respawn fades in.
            if o.timetoregen > 0 && o.timetoregen < 60 {
                frame.misc[0] = (60 - o.timetoregen) as f32 * 0.016_666_668;
                xlu = true;
            }
            // A tinted pane's env alpha is its opacity from this camera
            // (`propobj.c:12804`, `glass_update_portal`).
            if let Some(t) = &o.tinted {
                let opacity = pd_sim::props::glass::glass_calculate_opacity(o.pos, p.cam.pos(), t.xludist, t.opadist, t.unk64);
                frame.misc[0] = opacity as f32 / 255.0;
                xlu = true;
            }
            // The fog colour: the pickup highlight (scenario_highlight_prop,
            // propobj.c:12839).
            let highlight = world.scenario_highlight_obj(pi, o);
            if let Some(h) = highlight {
                frame.fogcol = h.map(|v| v as f32 / 255.0);
            }
            if let (ShadeMode::Frac(a), Some(bg)) = (shade, self.bg.as_ref()) {
                // PD merges `obj->shadecol` toward the sky: the colour of the floor
                // under the object reduced to its tint, the lowest component 0,
                // the highest the spread (`prop_calculate_shade_colour`,
                // `propobj.c:1623-1690`), so a grey or white floor's is black.
                // SUBST: the rest of `shadecol` (the room's light, the brightness
                // in its alpha, the halving it eases in by) waits for the objects'
                // lighting port (above) / the floor's tint with the frame's alpha.
                if highlight.is_none() {
                    if let (_, Some(poly)) = world.level.cd_find_ground_at_cyl(o.pos, 1.0) {
                        let c = world.level.geom.polys[poly].floorcol;
                        let mut n = [8, 4, 0].map(|s| (((c >> s) & 0xf) * 17) as i32);
                        let (mut max, mut min) = if n[1] > n[0] { (1, 0) } else { (0, 1) };
                        let med;
                        if n[2] > n[max] {
                            med = max;
                            max = 2;
                        } else if n[2] > n[min] {
                            med = 2;
                        } else {
                            med = min;
                            min = 2;
                        }
                        if n[max] > 0 {
                            let (hi, lo) = (n[max], n[min]);
                            n[med] = n[med] * (hi - lo) / hi;
                            n[min] = 0;
                            n[max] = hi - lo;
                        }
                        frame.fogcol = [n[0] as f32 / 255.0, n[1] as f32 / 255.0, n[2] as f32 / 255.0, frame.fogcol[3]];
                    }
                }
                frame.fogcol = bg::obj_merge_colour_fracs(frame.fogcol, sky_f32(bg), a);
            }
            if let Some(e) = xray {
                // In x-ray: the flat eraser colour through the fog at full
                // weight, alpha 0..128 as the env alpha (BONDGUN_OBJ_XLU).
                let Some(c) = xray::obj_colour(e, o.pos) else { continue };
                frame.flat = [c[0], c[1], c[2], 1.0];
                frame.misc[0] = c[3];
                xlu = true;
            }
            let joints = o.init_matrices().iter().map(|m| w2e * *m).collect();
            // A DOORFLAG_0004 door draws its own copy of the model, its display
            // list clipped to the frame (`door_calc_vertices_*`).
            let def = match o.door.as_ref().map(|d| pd_sim::props::door::door_calc_texturemap(o, d)) {
                Some(verts) if !verts.is_empty() => {
                    let def = self
                        .door_models
                        .entry(o.id)
                        .or_insert_with(|| std::sync::Arc::new(pd_core::model::ModelDef { stem: format!("{}#door{}", o.def.stem, o.id), ..(*o.def).clone() }))
                        .clone();
                    door_verts.push((def.stem.clone(), verts));
                    def
                }
                _ => o.def.clone(),
            };
            objdraws.push((def, o.vis.clone(), joints, frame, None, xlu));
        }

        // The simulants and the other players' bodies (`chr_render`,
        // `chr.c:3378`; `player_render`): the body, its head and the held guns,
        // posed by the sim in world space. A dying chr's corpse fades
        // (`fadealpha`), a new life fades in over 2 s (`aibot->fadeintimer60`),
        // and a cloak thins it to its shimmer (`chr_get_cloak_alpha`), all drawn
        // see-through. No player sees its own body.
        // SUBST: PD lights a chr by its room (`chr_render`'s shade colour) /
        // lit as the objects are, from the chr's floor room's brightness.
        for (ci, c) in world.chrs.iter().enumerate().filter(|(_, c)| c.player != Some(pi) && c.onanyscreen) {
            let alpha = chr_render_alpha(c);
            // chr_render (`chr.c:3446`): not drawn when lost in the fog.
            let shade = obj_shade_mode(self.bg.as_ref(), w2e, c.pos, xray.is_some());
            if alpha <= 0.0 || shade == ShadeMode::Xlu {
                continue;
            }
            let cloak = c.cloak.alpha() as f32 / 255.0;
            let lights = gun_lights(world.lights.brightness(c.floorroom), false);
            let mut frame = lit_frame(world_proj, p.look, p.up, lights, env);
            tint_frame(&mut frame, c.rooms.first().and_then(|&r| world.scenario_highlight_room(r)));
            // The fog colour: the highlight (scenario_highlight_prop,
            // chr.c:3482) in place of the shade colour.
            if let Some(h) = world.scenario_highlight_chr(pi, ci) {
                frame.fogcol = h.map(|v| v as f32 / 255.0);
            }
            if let (ShadeMode::Frac(a), Some(bg)) = (shade, self.bg.as_ref()) {
                frame.fogcol = bg::obj_merge_colour_fracs(frame.fogcol, sky_f32(bg), a);
            }
            let mut xlu = alpha < 255.0;
            if xlu {
                frame.misc[0] = alpha / 255.0;
            }
            if let Some(e) = xray {
                let Some(col) = xray::obj_colour(e, c.pos) else { continue };
                frame.flat = [col[0], col[1], col[2], 1.0];
                frame.misc[0] = col[3] * cloak;
                xlu = true;
            }
            let joints: Vec<Mat4> = c.model.matrices.iter().map(|m| w2e * *m).collect();
            objdraws.push((c.model.def.clone(), c.model.vis.clone(), joints, frame, None, xlu));
            if let (Some(head), Some(hm)) = (c.model.head.clone(), c.model.head_matrix()) {
                objdraws.push((head, c.model.head_vis.clone(), vec![w2e * hm], frame, None, xlu));
            }
            for held in c.held.iter().flatten() {
                let joints = held.model.matrices.iter().map(|m| w2e * *m).collect();
                objdraws.push((held.model.def.clone(), held.model.vis.clone(), joints, frame, None, xlu));
            }
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
        for (stem, batches) in &door_verts {
            for (bi, verts) in batches {
                self.models.rewrite_verts(queue, stem, *bi, verts);
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

        // The 2D layer, in PD's framebuffer pixels (every element is placed
        // from the view's rectangle), then cut to the view.
        let rect = [p.cam.c_screenleft as i32, p.cam.c_screentop as i32, p.cam.c_screenwidth as i32, p.cam.c_screenheight as i32];
        let (fbw, fbh) = ((rect[0] + rect[2]).max(view::VIEW_W as i32) as usize, (rect[1] + rect[3]).max(view::VIEW_H as i32) as usize);
        if self.hud_scratch.w != fbw || self.hud_scratch.h != fbh {
            self.hud_scratch = hud::layer(fbw, fbh);
        } else {
            self.hud_scratch.fb.fill([0.0; 4]);
            self.hud_scratch.full_scissor();
        }
        if let Some(fonts) = self.fonts.as_ref().filter(|_| !riding) {
            let hin = hud::HudIn {
                gun,
                gset: &res.gset,
                view: rect,
                playercount: world.players.len(),
                isdead: p.isdead,
                sighton: p.insightaimmode && !p.health.sightoff_damage,
                hasprop: world.lookingatprop.get(pi).copied().flatten().is_some(),
                speedpilltime: world.speedpill.time,
                options: world.setup.players.get(pi).map_or(pd_core::mp::MatchPlayer::DEFAULT_OPTIONS, |m| m.options),
                zoominfovy: p.zoominfovy,
                health: p.health.player_is_health_visible().then(|| (p.health.apparenthealth, p.health.apparentarmour, p.health.player_get_health_bar_height_frac())),
                fovy: view.fovy,
                fade: (p.health.colourscreen, p.health.colourscreenfrac),
                hudmsgs: world.mp.hudmsgs.msgs.iter().filter(|m| m.playernum == pi).collect(),
                scenario: world.scenario_hud(pi),
                playernum: pi,
                screensplit: world.setup.screensplit,
                targetboxes: world.sight_target_boxes(pi),
                lookingat_friendly: world.sight_is_prop_friendly(pi, None),
                radar: world.radar_render_in(pi),
                radartex: self.radar_ring.as_ref(),
                displayteam: world.scenario_display_team(pi),
                shieldflash: p.shieldflash,
                shieldtex: self.shield_tex.as_ref(),
                activemenu: world.am_render_in(pi),
            };
            let mut t = TextCtx { gfx: &mut self.hud_scratch, ts: &mut self.text, fonts, frac20: world.frac20 };
            hud::draw(&mut t, &hin);
        }
        // lv_render: menu_render (the pause and end-of-match menus), then
        // mp_render_modal_text (`lv.c:1643`).
        hud::composite_over_rect(&mut self.hud_scratch, &self.menu_layer, view::VIEW_W as usize, rect);
        if let Some(fonts) = self.fonts.as_ref() {
            let mut t = TextCtx { gfx: &mut self.hud_scratch, ts: &mut self.text, fonts, frac20: world.frac20 };
            hudmsg::mp_render_modal_text(&mut t, &world.res.lang, rect, world.mp_modal_text(pi));
        }
        self.hud_gfx = hud::crop(&self.hud_scratch, rect);
        hud::paste(&mut self.hud_frame, &self.hud_gfx, rect);
        self.overlay.prepare(device, queue, &self.hud_gfx);

        let bgframe = Self::bg_frame(world, pi, target.width, target.height);
        if let Some(bg) = self.bg.as_ref().filter(|_| xray.is_none()) {
            bg.prepare(queue, &view, self.three_point, bgframe.as_ref());
        }
        // sky_render (`sky.c:206`): the cloud plane, not in x-ray.
        if let Some(bg) = self.bg.as_ref() {
            let env = bg.sky_env();
            let verts = if xray.is_none() { sky::sky_geometry(&env, &p.cam, p.cam.pos(), &(world_proj * w2e), world.sky_cloud_offset) } else { Vec::new() };
            let sky = self.sky.get_or_insert_with(|| sky::SkyRenderer::new(device, queue, &assets, self.color_format, DEPTH_FORMAT));
            sky.prepare(device, queue, &env, &verts);
        }
        {
            // sky_render: the fill colour, black in x-ray (`sky.c:267`).
            let sky = if xray.is_some() { [0.0; 3] } else { self.sky() };
            let mut rp = world_pass(encoder, color, depth, sky);
            if let Some(s) = self.sky.as_ref().filter(|_| self.bg.is_some()) {
                s.draw(&mut rp);
            }
            // bg_render_scene (bg.c:987): the rooms' opaque layers, the wall
            // hits, then the translucent layers with the props' (see bg.rs).
            if let Some(bg) = self.bg.as_ref().filter(|_| xray.is_none()) {
                bg.draw_opaque(&mut rp, &self.combiner, bgframe.as_ref());
            }
            self.fx.draw_some(&mut rp, fx::FxPass::World, |k| k.before_objects());
            self.models.draw(&mut rp, &self.combiner, &obj_cmds);
            if let Some(bg) = self.bg.as_ref().filter(|_| xray.is_none()) {
                bg.draw_xlu(&mut rp, &self.combiner, bgframe.as_ref(), view.eye);
            }
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

/// `chr_render`'s alpha (0..255): the corpse's fade (`fadealpha`), a new
/// life's 2-second fade-in (`aibot->fadeintimer60`), the cloak's
/// (`chr_get_cloak_alpha`, `chr.c:3424`; no IR scanner in a match).
fn chr_render_alpha(c: &pd_sim::chr::Chr) -> f32 {
    let mut alpha = if c.fadealpha < 0.0 { 255.0 } else { c.fadealpha };
    if let Some(a) = c.aibot.as_ref().filter(|a| a.fadeintimer60 > 0) {
        alpha = alpha * (120 - a.fadeintimer60) as f32 * (1.0 / 120.0);
    }
    (alpha * (c.cloak.alpha() as f32 / 255.0)).trunc()
}

/// `SHADEMODE_*` (`constants.h:3690`): how the fog takes a prop.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ShadeMode {
    Opa,
    Frac(f32),
    Xlu,
}

/// `env_get_obj_shade_mode` (`env.c:445`) for a prop at world `pos`: only on
/// a fog stage, not in x-ray; `prop->z` is its depth ahead of the camera.
fn obj_shade_mode(bg: Option<&bg::StageBg>, w2e: Mat4, pos: Vec3, xray: bool) -> ShadeMode {
    let Some(bg) = bg.filter(|b| b.fog && !xray) else { return ShadeMode::Opa };
    let z = -w2e.transform_point3(pos).z;
    match bg::env_get_obj_shade_frac(&bg.env, z) {
        None => ShadeMode::Opa,
        Some(a) if a > 1.0 => ShadeMode::Xlu,
        Some(a) => ShadeMode::Frac(a),
    }
}

/// The sky colour (0..1), which fog stages fade to (`g_Env.skyredfrac`, ...).
fn sky_f32(bg: &bg::StageBg) -> [f32; 3] {
    bg.sky().map(|v| v as f32)
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
        let level = TileLevel::for_stage(&stage);
        let setup = MatchSetup { players: vec![MatchPlayer::default()], ..Default::default() };
        let mut w = World::new(setup, Arc::new(stage), Arc::new(level), Arc::new(WorldRes::load(&a).unwrap()), 1).unwrap();
        w.boards = fixtures::firing_range_boards();
        // Not PD: armed as the gun tests are (a match starts unarmed).
        w.harness_give_loadout(w.res.gset.order.clone());
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
