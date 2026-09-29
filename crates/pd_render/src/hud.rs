//! The in-match 2D layer, drawn on the CPU into an [`n64::rdp::Gfx`] of the
//! player's PD screen pixels and laid over the 3D view by [`HudOverlay`]:
//!
//! * the sight (`bgun_draw_sight` → `sight_draw`, `sight.c:1414`): the default
//!   aimer box and its long cross, the zoom range's corners, the Maian sight,
//!   and the small target cross;
//! * the gun HUD (`bgun_draw_hud`, `bondgun.c:9930`): the function square, the
//!   weapon and function names sliding in under the wave blend, the magazine
//!   and reserve gauges (`bgun_draw_hud_gauge`, `:9693`) and their counts, and
//!   the Combat Boost timer.
//!
//! PD ticks the HUD's timers while it draws; `pd_sim::gun::hud` runs those
//! updates in the sim, and this draws from the result without changing it.
//!
//! `// SUBST:` the classic, type-2 and Skedar sights (`sight_draw_classic`,
//! `_type2`, `_skedar`) / drawn as the default sight until they are ported.
//!
//! Source: the old repo's `pd_guns/hud.rs` (the drawing half) and `app.rs`
//! (`canvas_sight_aimer`, `canvas_sight_maian`).

use n64::rdp::{Blend, Cc, Filter, Gfx, TriState, SV};
use pd_core::ids::*;
use pd_core::text::{self, colour_blend, FontId, TextCtx};
use pd_sim::gun::hud::{Abmag, BARWIDTH, BOTTOM_MARGIN, CLIPHEIGHT, RESERVEHEIGHT};
use pd_sim::gun::{Bgun, Gset, AMMO_CAPACITY};

/// `HUDHALIGN_*` / `HUDVALIGN_*` (`constants.h:1423`, `:1466`).
const HUDHALIGN_RIGHT: bool = false;
const HUDHALIGN_LEFT: bool = true;
const HUDVALIGN_BOTTOM: i32 = 0;

/// What the 2D layer reads from the sim for one player.
pub struct HudIn<'a> {
    pub gun: &'a Bgun,
    pub gset: &'a Gset,
    /// The view: left, top, width, height in PD pixels.
    pub view: [i32; 4],
    pub playercount: usize,
    pub isdead: bool,
    /// `gunsightoff == 0`: aiming with R, and no damage flash
    /// (`GUNSIGHTREASON_DAMAGE`).
    pub sighton: bool,
    /// `lookingatprop.prop != NULL`.
    pub hasprop: bool,
    /// `g_Vars.speedpilltime`: the Combat Boost's time left.
    pub speedpilltime: i32,
    /// The player's `OPTION_*` (`g_PlayerConfigsArray[].options`).
    pub options: u16,
    /// `player->zoominfovy`.
    pub zoominfovy: f32,
    /// The health bar: `apparenthealth` and its height (`player_is_health_visible`).
    pub health: Option<(f32, f32, f32)>,
    /// The perspective's field of view the bar is seen through (degrees).
    pub fovy: f32,
    /// `colourscreen*`: the fade over the view.
    pub fade: ([i32; 3], f32),
    /// The player's HUD messages (`g_HudMessages` of this player, by slot).
    pub hudmsgs: Vec<&'a pd_sim::mp::HudMessage>,
    /// The scenario's part (`scenario_render_hud`).
    pub scenario: pd_sim::mp::scenario::ScenarioHud,
    /// `g_ScreenSplit`.
    pub screensplit: u8,
    /// The tracked props' boxes (`sight_draw_default`), and whether what the
    /// crosshair is on is a teammate (`sight_is_prop_friendly(NULL)`).
    pub targetboxes: Vec<pd_sim::gun::sight::TargetBox>,
    pub lookingat_friendly: bool,
    /// `radar_render`'s dots (none: no radar), and its ring's texture.
    pub radar: Option<pd_sim::mp::radar::RadarIn>,
    pub radartex: Option<&'a n64::rdp::Texture>,
    /// Display Team's line: the player's team colour as a 5551 fill word.
    pub displayteam: Option<u32>,
    /// The first-person shield flash (`player_render_shield`), and its texture.
    pub shieldflash: Option<pd_sim::player::health::ShieldFlash>,
    pub shieldtex: Option<&'a n64::rdp::Texture>,
    /// The player's number, and its active menu (open, and the ordered
    /// simulant's bars).
    pub playernum: usize,
    pub activemenu: (Option<pd_sim::player::activemenu::AmRender>, Option<pd_sim::player::activemenu::AmBars>),
}

impl HudIn<'_> {
    fn option(&self, o: u16) -> bool {
        self.options & o != 0
    }

    /// `options_get_screen_split() == SCREENSPLIT_VERTICAL`.
    fn vsplit(&self) -> bool {
        self.screensplit == SCREENSPLIT_VERTICAL
    }
}

/// `gDPHudRectangle` (`gbiex.h:101`) at `g_UiScaleX` 1: both corners inclusive,
/// in the box mode's prim colour.
fn hud_rect(gfx: &mut Gfx, x1: i32, y1: i32, x2: i32, y2: i32, colour: u32) {
    gfx.fill_rect(x1, y1, x2 + 1, y2 + 1, colour);
}

// ── sights (sight.c) ────────────────────────────────────────────────────────

/// `sight_draw_aimer` (`sight.c:428`).
fn sight_draw_aimer(gfx: &mut Gfx, h: &HudIn, x: i32, y: i32, radius: i32, cornergap: i32, colour: u32) {
    let [viewleft, viewtop, viewwidth, viewheight] = h.view;
    let viewright = viewleft + viewwidth - 1;
    let viewbottom = viewtop + viewheight - 1;
    let line = 0x00ff0028;
    // The lines that span most of the view.
    if h.playercount == 1 {
        hud_rect(gfx, viewleft + 48, y, x - radius + 2, y, line);
        hud_rect(gfx, x + radius - 2, y, viewright - 49, y, line);
        hud_rect(gfx, x, viewtop + 10, x, y - radius + 2, line);
        hud_rect(gfx, x, y + radius - 2, x, viewbottom - 10, line);
    } else {
        hud_rect(gfx, viewleft, y, x - radius + 2, y, line);
        hud_rect(gfx, x + radius - 2, y, viewright, y, line);
        hud_rect(gfx, x, viewtop, x, y - radius + 2, line);
        hud_rect(gfx, x, y + radius - 2, x, viewbottom, line);
    }
    let (r, g) = (radius, cornergap);
    for (x1, y1, x2, y2) in [
        // The box.
        (x - r, y - r, x - r, y + r),
        (x + r, y - r, x + r, y + r),
        (x - r, y - r, x + r, y - r),
        (x - r, y + r, x + r, y + r),
        // The corners a second time.
        (x - r, y - r, x - r, y - g),
        (x - r, y + g, x - r, y + r),
        (x + r, y - r, x + r, y - g),
        (x + r, y + g, x + r, y + r),
        (x - r, y - r, x - g, y - r),
        (x + g, y - r, x + r, y - r),
        (x - r, y + r, x - g, y + r),
        (x + g, y + r, x + r, y + r),
    ] {
        hud_rect(gfx, x1, y1, x2, y2, colour);
    }
}

/// `sight_draw_default` (`sight.c:615`): the aimer while R is held, tighter
/// over a prop, red (blue over a teammate, `sight_is_prop_friendly`). The
/// tracked props' boxes that come first are [`sight_draw_target_boxes`].
fn sight_draw_default(gfx: &mut Gfx, h: &HudIn, sighton: bool) {
    if !sighton {
        return;
    }
    let [x, y] = crosspos(h);
    let over = if h.lookingat_friendly { 0x0000ff60 } else { 0xff000060 };
    let (colour, radius, cornergap) = if h.hasprop { (over, 6, 3) } else { (0x00ff0028, 8, 5) };
    sight_draw_aimer(gfx, h, x, y, radius, cornergap, colour);
}

/// `sight_calculate_box_bound` (`sight.c:317`): a box edge closing in from
/// the view's edge over `timeend` ticks.
fn sight_calculate_box_bound(targetx: i32, viewleft: i32, timeelapsed: i32, timeend: i32) -> i32 {
    let t = timeelapsed.min(timeend);
    viewleft + (targetx - viewleft) * t / timeend
}

/// `sight_draw_target_box` (`sight.c:344`): a red (blue: a teammate) box round
/// a tracked prop, its edges sliding in from the view's over 80 ticks, and its
/// label at its top right while the right and top edges show.
fn sight_draw_target_box(t: &mut TextCtx, view: [i32; 4], b: &pd_sim::gun::sight::TargetBox) {
    let [viewleft, viewtop, viewwidth, viewheight] = view;
    let viewright = viewleft + viewwidth - 1;
    let viewbottom = viewtop + viewheight - 1;
    let time = b.time.min(512);
    let tp = &b.tp;
    let boxleft = sight_calculate_box_bound(tp.x1 as i32, viewleft, time, 80);
    let boxtop = sight_calculate_box_bound(tp.y1 as i32, viewtop, time, 80);
    let boxright = sight_calculate_box_bound(tp.x2 as i32, viewright, time, 80);
    let boxbottom = sight_calculate_box_bound(tp.y2 as i32, viewbottom, time, 80);
    let colour = if b.friendly { 0x0000ff60 } else { 0xff000060 };
    let mut textonscreen = true;
    let gfx = &mut *t.gfx;
    if boxleft >= viewleft && boxleft <= viewright && boxtop <= viewbottom && boxbottom >= viewtop {
        hud_rect(gfx, boxleft, boxtop.max(viewtop), boxleft, boxbottom.min(viewbottom), colour);
    }
    if boxright >= viewleft && boxright <= viewright && boxtop <= viewbottom && boxbottom >= viewtop {
        hud_rect(gfx, boxright, boxtop.max(viewtop), boxright, boxbottom.min(viewbottom), colour);
    } else {
        textonscreen = false;
    }
    if boxtop >= viewtop && boxtop <= viewbottom && boxleft <= viewright && boxright >= viewleft {
        hud_rect(gfx, boxleft.max(viewleft), boxtop, boxright.min(viewright), boxtop, colour);
    } else {
        textonscreen = false;
    }
    if boxbottom >= viewtop && boxbottom <= viewbottom && boxleft <= viewright && boxright >= viewleft {
        hud_rect(gfx, boxleft.max(viewleft), boxbottom, boxright.min(viewright), boxbottom, colour);
    }
    let Some(label) = b.label.as_ref().filter(|_| textonscreen) else { return };
    let (mut x, mut y) = (boxright + 3, boxtop + 3);
    let (w, hgt) = (t.gfx.w as i32, t.gfx.h as i32);
    match label {
        pd_sim::gun::sight::TargetLabel::Digit(c) => {
            let s = format!("{c}\n");
            t.render_v1(&mut x, &mut y, &s, FontId::Numeric, 0x00ff00a0, 0x000000a0, w, hgt, 0, 0);
        }
        pd_sim::gun::sight::TargetLabel::Text(s) => t.render_v1(&mut x, &mut y, s, FontId::Xs, 0x00ff00a0, 0x000000a0, w, hgt, 0, 0),
    }
}

/// The tracked props' boxes of `sight_draw_default` (the rocket launcher's
/// lock, the CMP150's, the Threat Detector's), before its aimer.
pub fn sight_draw_target_boxes(t: &mut TextCtx, h: &HudIn) {
    for b in &h.targetboxes {
        sight_draw_target_box(t, h.view, b);
    }
}

/// `sight_draw_maian` (`sight.c:1278`): four shaded triangles from the middle of
/// each view edge to an 8-pixel box round the crosshair, then the box's border.
fn sight_draw_maian(gfx: &mut Gfx, h: &HudIn, sighton: bool) {
    if !sighton {
        return;
    }
    let [viewleft, viewtop, viewwidth, viewheight] = h.view;
    let viewright = viewleft + viewwidth - 1;
    let viewbottom = viewtop + viewheight - 1;
    let [x, y] = crosspos(h);
    // M6: sight_is_prop_friendly.
    let colour = 0xff000060;
    let outer = n64::rdp::rgba(0x00ff000f);
    let inner = n64::rdp::rgba(if h.hasprop { colour } else { 0x00ff0044 });
    // Vertices in ortho units (pixels × 10), through `ortho_begin`'s matrix.
    let v = |px: i32, py: i32, c: [f32; 4]| {
        let (sx, sy) = gfx.ortho((px * 10) as f32, (py * 10) as f32);
        SV { x: sx, y: sy, z: 0.0, inv_w: 1.0, s: 0.0, t: 0.0, c }
    };
    let verts = [
        v(viewleft + (viewwidth >> 1), viewtop + 10, outer),
        v(viewleft + (viewwidth >> 1), viewbottom - 10, outer),
        v(viewleft + 48, viewtop + (viewheight >> 1), outer),
        v(viewright - 49, viewtop + (viewheight >> 1), outer),
        v(x - 4, y - 4, inner),
        v(x + 4, y - 4, inner),
        v(x + 4, y + 4, inner),
        v(x - 4, y + 4, inner),
    ];
    // G_CC_SHADE, G_RM_AA_XLU_SURF, no cull.
    let st = TriState { cc: Cc::Shade, tex: None, filter: Filter::Bilerp, blend: Blend::Xlu, env: [1.0; 4], persp: false, zbuf: false, cull_back: false };
    // gSPTri4(0, 4, 5, 5, 3, 6, 7, 6, 1, 4, 7, 2)
    for t in [[0, 4, 5], [5, 3, 6], [7, 6, 1], [4, 7, 2]] {
        gfx.tri([verts[t[0]], verts[t[1]], verts[t[2]]], &st);
    }
    let b = 0x00ff0028;
    hud_rect(gfx, x - 4, y - 4, x - 4, y + 4, b);
    hud_rect(gfx, x + 4, y - 4, x + 4, y + 4, b);
    hud_rect(gfx, x - 4, y - 4, x + 4, y - 4, b);
    hud_rect(gfx, x - 4, y + 4, x + 4, y + 4, b);
}

/// `sight_draw_zoom` (`sight.c:1140`): the zoom range's four corners, closing in
/// as the view zooms, then the default sight.
fn sight_draw_zoom(gfx: &mut Gfx, h: &HudIn, sighton: bool) {
    let [viewleft, viewtop, viewwidth, viewheight] = h.view;
    let viewhalfwidth = viewwidth >> 1;
    let viewhalfheight = viewheight >> 1;
    let viewright = viewleft + viewhalfwidth * 2 - 1;
    let viewbottom = viewtop + viewhalfheight * 2 - 1;
    // Padding: zoomed right in, the left corner is 48 px from the view's edge.
    let availableleft = (viewhalfwidth - 48) as f32;
    let availableright = (viewhalfwidth - 49) as f32;
    let availableabove = (viewhalfheight - 10) as f32;
    let availablebelow = (viewhalfheight - 10) as f32;
    let mut frac = 1.0f32;
    let weaponnum = h.gun.hands[HAND_RIGHT].weaponnum;
    let mut cornerwidth = (viewhalfwidth >> 1) - 60;
    let mut cornerheight = (viewhalfheight >> 1) - 22;
    let mut showzoomrange = h.option(OPTION_SHOWZOOMRANGE) && h.option(OPTION_SIGHTONSCREEN);
    let maxfovy = h.gun.gset_get_gun_zoom_fov(h.gset);
    if maxfovy == 0.0 || maxfovy == 60.0 {
        if weaponnum != WEAPON_SNIPERRIFLE {
            showzoomrange = false;
        }
    } else {
        frac = maxfovy / h.zoominfovy;
    }
    if showzoomrange {
        let c = 0x00ff0028;
        let f = frac.max(0.2);
        cornerwidth = (cornerwidth as f32 * f) as i32;
        cornerheight = (cornerheight as f32 * f) as i32;
        if h.playercount >= 2 {
            cornerheight *= 2;
        }
        cornerwidth = cornerwidth.max(5);
        cornerheight = cornerheight.max(5);
        // The margins from the view's edges to the box.
        let marginleft = viewhalfwidth as f32 - availableleft * frac;
        let marginright = viewhalfwidth as f32 - availableright * frac;
        let marginbottom = viewhalfheight as f32 - availablebelow * frac;
        let margintop = viewhalfheight as f32 - availableabove * frac;
        // gDPHudRectangle takes s32s: the float corners truncate at the call.
        let boxleft = viewleft as f32 + marginleft;
        let boxright = viewright as f32 - marginright;
        let boxbottom = viewbottom as f32 - marginbottom;
        let boxtop = viewtop as f32 + margintop;
        if cornerwidth as f32 > boxright - boxleft {
            cornerwidth = (boxright - boxleft) as i32;
        }
        if cornerheight as f32 > boxbottom - boxtop {
            cornerheight = (boxbottom - boxtop) as i32;
        }
        let r = |gfx: &mut Gfx, x1: f32, y1: f32, x2: f32, y2: f32| hud_rect(gfx, x1 as i32, y1 as i32, x2 as i32, y2 as i32, c);
        let (cw, ch) = (cornerwidth as f32, cornerheight as f32);
        r(gfx, boxleft + 1.0, boxtop, boxleft + cw - 1.0, boxtop);
        r(gfx, boxleft, boxtop, boxleft, boxtop + ch - 1.0);
        r(gfx, boxright - cw + 2.0, boxtop, boxright - 1.0, boxtop);
        r(gfx, boxright, boxtop, boxright, boxtop + ch - 1.0);
        r(gfx, boxleft + 1.0, boxbottom, boxleft + cw - 1.0, boxbottom);
        r(gfx, boxleft, boxbottom - ch + 1.0, boxleft, boxbottom);
        r(gfx, boxright - cw + 2.0, boxbottom, boxright - 1.0, boxbottom);
        r(gfx, boxright, boxbottom - ch + 1.0, boxright, boxbottom);
        // The corners again, half as long.
        let (cw, ch) = ((cornerwidth >> 1) as f32, (cornerheight >> 1) as f32);
        r(gfx, boxleft, boxtop, boxleft + cw, boxtop);
        r(gfx, boxleft, boxtop, boxleft, boxtop + ch);
        r(gfx, boxright - cw, boxtop, boxright, boxtop);
        r(gfx, boxright, boxtop, boxright, boxtop + ch);
        r(gfx, boxleft, boxbottom, boxleft + cw, boxbottom);
        r(gfx, boxleft, boxbottom - ch, boxleft, boxbottom);
        r(gfx, boxright - cw, boxbottom, boxright, boxbottom);
        r(gfx, boxright, boxbottom - ch, boxright, boxbottom);
    }
    sight_draw_default(gfx, h, sighton);
}

/// `sight_draw_target` (`sight.c:1375`): the small cross at the crosshair.
fn sight_draw_target(gfx: &mut Gfx, h: &HudIn) {
    let [x, y] = crosspos(h);
    let c = 0x00ff0028;
    hud_rect(gfx, x + 2, y, x + 6, y, c);
    hud_rect(gfx, x + 2, y, x + 4, y, c);
    hud_rect(gfx, x - 6, y, x - 2, y, c);
    hud_rect(gfx, x - 4, y, x - 2, y, c);
    hud_rect(gfx, x, y + 2, x, y + 6, c);
    hud_rect(gfx, x, y + 2, x, y + 4, c);
    hud_rect(gfx, x, y - 6, x, y - 2, c);
    hud_rect(gfx, x, y - 4, x, y - 2, c);
}

fn crosspos(h: &HudIn) -> [i32; 2] {
    [h.gun.p.crosspos[0] as i32, h.gun.p.crosspos[1] as i32]
}

/// `sight_has_target_while_aiming` (`sight.c:1402`).
fn sight_has_target_while_aiming(sight: i32) -> bool {
    sight == SIGHT_DEFAULT || sight == SIGHT_ZOOM
}

/// `bgun_draw_sight` (`bondgun.c:10401`) → `sight_draw` (`sight.c:1414`).
pub fn sight_draw(gfx: &mut Gfx, h: &HudIn) {
    let right = &h.gun.hands[HAND_RIGHT];
    let mut sight = h.gset.gset_get_sight(h.gun.bgun_get_weapon_num(HAND_RIGHT), right.weaponfunc);
    // No co-op or counter-op in the Combat Simulator.
    if h.playercount >= 2 {
        sight = SIGHT_DEFAULT;
    }
    let sightonscreen = h.option(OPTION_SIGHTONSCREEN);
    let alwaysshowtarget = h.option(OPTION_ALWAYSSHOWTARGET);
    let sighton = h.sighton && sightonscreen;
    match sight {
        SIGHT_NONE => {}
        SIGHT_ZOOM => sight_draw_zoom(gfx, h, sighton),
        SIGHT_MAIAN => sight_draw_maian(gfx, h, sighton),
        // SUBST: see the module docs.
        _ => sight_draw_default(gfx, h, sighton),
    }
    if sight != SIGHT_NONE && sightonscreen && ((alwaysshowtarget && !h.sighton) || (h.sighton && sight_has_target_while_aiming(sight))) {
        sight_draw_target(gfx, h);
    }
}

// ── the gun HUD (bondgun.c) ─────────────────────────────────────────────────

/// `bgun_draw_hud_string` (`bondgun.c:9542`): aligned, an invisible black box,
/// then `text_render_v1` in the numeric font with a 0x000000a0 glow.
fn bgun_draw_hud_string(t: &mut TextCtx, text: &str, x: i32, halign: bool, y: i32, valign: i32, colour: u32) {
    let (textheight, textwidth) = text::measure(t.fonts.get(FontId::Numeric), text, 0);
    let mut x1 = if halign == HUDHALIGN_LEFT { x } else { x - textwidth };
    let mut y1 = if valign == HUDVALIGN_BOTTOM { y - textheight } else { y };
    let (x2, y2) = (x1 + textwidth, y1 + textheight);
    t.gfx.fill_rect(x1, y1, x2, y2, 0x00000000);
    let (w, h) = (t.gfx.w as i32, t.gfx.h as i32);
    t.render_v1(&mut x1, &mut y1, text, FontId::Numeric, colour, 0x000000a0, w, h, 0, 0);
}

/// `bgun_draw_hud_integer` (`bondgun.c:9588`).
fn bgun_draw_hud_integer(t: &mut TextCtx, value: i32, x: i32, halign: bool, y: i32, valign: i32, colour: u32) {
    bgun_draw_hud_string(t, &format!("{value}\n"), x, halign, y, valign, colour);
}

/// `bgun_draw_hud_gauge` (`bondgun.c:9693`) after its `bgun0f0a9da8` tick (the
/// sim's): a block per round up to 20, a split bar above that; newly loaded
/// rounds flash white then settle, spent ones fade. `flip` draws it top-down.
#[allow(clippy::too_many_arguments)]
fn bgun_draw_hud_gauge(gfx: &mut Gfx, x1: i32, y1: i32, x2: i32, y2: i32, abmag: &Abmag, capacity: i32, emptycolour: u32, filledcolour: u32, flip: bool) {
    let mut gaugeheight = y2 - y1;
    let mut numunits = capacity;
    let refv = abmag.ref_;
    let (unitheight, gaugetop);
    if numunits > 20 {
        unitheight = 1;
        numunits = gaugeheight;
        gaugetop = y2 - gaugeheight;
    } else {
        let mut uh = gaugeheight / numunits;
        let r1 = (uh * numunits - gaugeheight).abs();
        let r2 = ((uh + 1) * numunits - gaugeheight).abs();
        if r2 < r1 {
            uh += 1;
        }
        unitheight = uh;
        let mut gt = y2 - unitheight * capacity + 1;
        if unitheight <= 2 {
            gt -= 1;
        }
        gaugetop = gt;
    }
    let rect = |gfx: &mut Gfx, top: i32, bottom: i32, colour: u32| {
        if flip {
            gfx.fill_rect_scaled(x1, y2 - bottom + y1, x2, y2 - top + y1, colour);
        } else {
            gfx.fill_rect_scaled(x1, top, x2, bottom, colour);
        }
    };
    if unitheight == 0 {
        // Unreachable in PD (`bondgun.c:9738`).
        gaugeheight = y2 - gaugetop;
        let partitiony = y2 - gaugeheight * refv / numunits;
        if partitiony > gaugetop {
            rect(gfx, gaugetop, partitiony, emptycolour);
        }
        rect(gfx, partitiony, y2, filledcolour);
        return;
    }
    // The RDP prim colour: text_begin_boxmode(emptycolour), then set only when a
    // unit's state changes; a merged gauge flushes its previous run in the
    // colour that was current before the change.
    let mut prim = emptycolour;
    let mut colour = emptycolour;
    let mut unittop = gaugetop;
    let mut unitbottom = -1;
    for i in 0..numunits {
        let mut newstate = false;
        if abmag.change > 0 {
            // Loading or reloading.
            if i >= numunits - refv - abmag.change && i < numunits - refv {
                let fadeamount = abmag.timer60 - (numunits - refv - i - 1) * 64;
                if fadeamount >= 0 {
                    if fadeamount >= 64 {
                        let weight = (((fadeamount * 4 - 252) / 3) as u32).min(255);
                        colour = colour_blend(filledcolour, 0xffffffbf, weight);
                    } else {
                        colour = colour_blend(0xffffffbf, emptycolour, (fadeamount * 4) as u32);
                    }
                    newstate = true;
                }
            }
        } else if abmag.change < 0 && i < numunits - refv - abmag.change && i >= numunits - refv {
            // Firing.
            let fadeamount = abmag.timer60 - (i - numunits + refv) * 64;
            if fadeamount >= 0 {
                let weight = fadeamount as u32;
                colour = if weight > 255 { emptycolour } else { colour_blend(emptycolour, filledcolour | 0xff, weight) };
                newstate = true;
            }
        }
        if abmag.change < 0 {
            if i == numunits - refv - abmag.change {
                colour = filledcolour;
                newstate = true;
            }
        } else if i == numunits - refv {
            colour = filledcolour;
            newstate = true;
        }
        if unitheight <= 2 {
            if newstate {
                if unitbottom >= 0 {
                    rect(gfx, unittop, unitbottom, prim);
                }
                unittop = gaugetop + i * unitheight;
            }
            unitbottom = gaugetop + i * unitheight + unitheight;
        } else {
            unittop = gaugetop + i * unitheight;
            unitbottom = gaugetop + i * unitheight + unitheight - 1;
        }
        if newstate {
            prim = colour;
        }
        if unitbottom >= y2 - 1 && unitheight >= 2 {
            unitbottom = y2;
        }
        if unitheight >= 3 {
            rect(gfx, unittop, unitbottom, prim);
        }
    }
    if unitheight <= 2 {
        rect(gfx, unittop, unitbottom, prim);
    }
}

/// `bgun_draw_hud` (`bondgun.c:9930`) from the state the sim ticked.
pub fn bgun_draw_hud(t: &mut TextCtx, h: &HudIn) {
    let st = &h.gun.hud;
    if h.isdead || !st.shown {
        return;
    }
    let [viewleft, viewtop, viewwidth, viewheight] = h.view;
    let (playercount, playernum, vsplit) = (h.playercount, h.playernum, h.vsplit());
    let mut bottom = viewtop + viewheight - BOTTOM_MARGIN;
    let (mut barwidth, mut reserveheight, mut clipheight) = (BARWIDTH, RESERVEHEIGHT, CLIPHEIGHT);
    if playercount >= 2 {
        barwidth = 5;
        reserveheight = 26;
        clipheight = 47;
        // The views that meet another's bottom edge sit lower.
        if playercount == 2 {
            bottom += if !vsplit && playernum == 0 { 10 } else { 2 };
        } else {
            bottom += if playernum < 2 { 10 } else { 2 };
        }
    }
    // The right edge's gauges move out for a view with another to its right,
    // the left hand's in for one with another to its left.
    let rightshift = if (playercount == 2 && vsplit && playernum == 0) || (playercount >= 3 && playernum % 2 == 0) { 15 } else { 0 };
    let leftshift = if (playercount == 2 && vsplit && playernum == 1) || (playercount >= 3 && playernum % 2 == 1) { -14 } else { 0 };
    let ctrl = &h.gun.ctrl;
    let hand = &h.gun.hands[HAND_RIGHT];
    let lefthand = &h.gun.hands[HAND_LEFT];
    let weapon = h.gset.weapon(ctrl.weaponnum);
    let funcnum = st.funcnum;
    let mut xpos = viewleft + viewwidth - barwidth - 24 + rightshift;

    // The function square.
    let mut fncolour: u32 = 0xff000040;
    if st.fnfader > 128 {
        fncolour = (((st.fnfader * 2) - 256) as u32) << 16 | 0xff000040;
    }
    t.gfx.fill_rect_scaled(xpos - 13, bottom - 11, xpos - 2, bottom, fncolour);

    // The weapon name and the function name.
    let wave = (t.frac20 * 50.0) as i32;
    let func = h.gset.func(hand.weaponnum, funcnum);
    let showfunc = h.option(OPTION_SHOWGUNFUNCTION);
    if showfunc && st.gunstr_shown {
        let mut colour: u32 = 0x55ffffff;
        let name = weapon.map_or("", |w| w.name.as_str());
        let (textheight, textwidth) = text::measure(t.fonts.get(FontId::Xs), name, 0);
        let textwidth = (textwidth + 2).min(st.guntypetimer * 3);
        let mut x = if h.playercount >= 2 { xpos - textwidth - 13 } else { xpos - textwidth - 2 };
        let mut y = bottom - textheight - 15;
        if st.guntypetimer > 192 {
            let alpha = 255 - (st.guntypetimer - 192) as u32 * 255 / 63;
            colour = (colour & 0xffffff00) | alpha;
        }
        t.gfx.fill_rect_scaled(x - 1, y - 1, xpos - 11, bottom, 0);
        t.ts.set_wave_blend(wave, 0, 50);
        t.ts.set_wave_colours(0xffffffff, 0xffffffff);
        t.render_v2(&mut x, &mut y, name, FontId::Xs, colour, textwidth, 1000, 0, 0);
        t.ts.reset_blends();
    }
    if let (true, Some(func), Some(curfnstr)) = (showfunc, func, st.curfnstr.as_deref()) {
        if st.fnstr_shown {
            let mut colour: u32 = 0xff5555ff;
            if funcnum == FUNC_SECONDARY && func.name == curfnstr {
                colour |= 0x00ff0000;
            }
            if funcnum == FUNC_PRIMARY && func.name != curfnstr {
                colour |= 0x00ff0000;
            }
            let (textheight, textwidth) = text::measure(t.fonts.get(FontId::Xs), curfnstr, 0);
            let textwidth = (textwidth + 2).min(st.fnstrtimer * 3);
            let mut x = xpos - textwidth - 13;
            let mut y = bottom - textheight - 1;
            if st.fnstrtimer > 192 {
                let alpha = 255 - (st.fnstrtimer - 192) as u32 * 255 / 63;
                colour = (colour & 0xffffff00) | alpha;
            }
            t.gfx.fill_rect_scaled(x - 1, y - 1, xpos - 11, bottom + 3, 0);
            t.ts.set_wave_blend(wave, 0, 50);
            t.ts.set_wave_colours(0xffffffff, 0xffffffff);
            t.render_v2(&mut x, &mut y, curfnstr, FontId::Xs, colour, textwidth, 1000, 0, 0);
            t.ts.reset_blends();
        }
    }

    let Some(weapon) = weapon else { return };
    let mut ammoindex = weapon.functions[hand.weaponfunc].as_ref().map_or(0, |f| f.ammoindex);
    if ammoindex == -1 {
        ammoindex = weapon.functions[1 - hand.weaponfunc].as_ref().map_or(-1, |f| f.ammoindex);
        if ammoindex == -1 {
            return;
        }
    }
    let ai = ammoindex as usize;

    // The left hand's magazine.
    if lefthand.inuse && weapon.ammos[ai].is_some() && lefthand.weaponnum != WEAPON_REMOTEMINE {
        let lx = viewleft + 24 + leftshift;
        let a = weapon.ammos[ai].as_ref().unwrap();
        if lefthand.clipsizes[ai] > 0 && a.flags & AMMOFLAG_EQUIPPEDISRESERVE == 0 {
            bgun_draw_hud_gauge(t.gfx, lx, bottom - reserveheight - clipheight - 3, lx + barwidth, bottom - reserveheight - 3, &st.abmag[HAND_LEFT], lefthand.clipsizes[ai], 0x00300080, 0x00ff0040, false);
            bgun_draw_hud_integer(t, lefthand.loadedammo[ai], lx + barwidth + 2, HUDHALIGN_LEFT, bottom - reserveheight - 8, HUDVALIGN_BOTTOM, 0x00ff00a0);
        }
    }

    // The right hand's magazine, the reserve and the Combat Boost timer.
    let ammotype = ctrl.ammotypes[ai];
    if hand.inuse && ammotype >= 0 {
        xpos = viewleft + viewwidth - barwidth - 24 + rightshift;
        let ammoheld = h.gun.ammoheld(ammotype);
        let a = weapon.ammos[ai].as_ref();
        if hand.clipsizes[ai] > 0 && a.is_some_and(|a| a.flags & AMMOFLAG_EQUIPPEDISRESERVE == 0) {
            bgun_draw_hud_gauge(t.gfx, xpos, bottom - reserveheight - clipheight - 3, xpos + barwidth, bottom - reserveheight - 3, &st.abmag[HAND_RIGHT], hand.clipsizes[ai], 0x00300080, 0x00ff0040, false);
            bgun_draw_hud_integer(t, hand.loadedammo[ai], xpos - 2, HUDHALIGN_RIGHT, bottom - reserveheight - 8, HUDVALIGN_BOTTOM, 0x00ff00a0);
        }
        let capacity = AMMO_CAPACITY.get(ammotype as usize).copied().unwrap_or(0);
        if let Some(a) = a {
            if capacity > 0 && a.flags & AMMOFLAG_NORESERVE == 0 {
                let mut ammototal = ammoheld;
                if a.flags & AMMOFLAG_EQUIPPEDISRESERVE != 0 {
                    if hand.clipsizes[ai] > 0 {
                        ammototal += hand.loadedammo[ai];
                    }
                    if lefthand.clipsizes[ai] > 0 {
                        ammototal += lefthand.loadedammo[ai];
                    }
                }
                bgun_draw_hud_gauge(t.gfx, xpos, bottom - reserveheight, xpos + barwidth, bottom, &st.ctrl_abmag, capacity, 0x00403080, 0x00ffc040, true);
                bgun_draw_hud_integer(t, ammototal, xpos - 2, HUDHALIGN_RIGHT, bottom - reserveheight + 1, HUDVALIGN_BOTTOM, 0x00ffc0a0);
            }
        }
        if hand.weaponnum == WEAPON_COMBATBOOST {
            let t60 = h.speedpilltime;
            let mins = t60 / 3600;
            let secs60 = t60 - mins * 3600;
            let text = if mins >= 1 {
                format!("{:02}:{:02}:{:02}\n", mins, secs60 / 60, (secs60 - (secs60 / 60) * 60) * 100 / 60)
            } else {
                format!("{:02}:{:02}\n", secs60 / 60, (secs60 - (secs60 / 60) * 60) * 100 / 60)
            };
            bgun_draw_hud_string(t, &text, xpos + barwidth - 2, HUDHALIGN_RIGHT, bottom - reserveheight + 1, HUDVALIGN_BOTTOM, 0x00ffc0a0);
        }
    }
}

/// The whole 2D layer for one player: `bgun_draw_sight`, then `bgun_draw_hud`
/// if "ammo on screen" is on (`player.c:4731`), into a transparent `gfx`.
/// `player_render_hud`'s 2D part (`player.c:4536`): the health bar, the
/// sight, the ammo, the radar, the HUD messages (`hudmsgs_render`) and the
/// zoom range, then the stored fade over it all.
pub fn draw(t: &mut TextCtx, h: &HudIn) {
    // player_render_shield, after the gun and before the health bar.
    if let (Some(f), Some(tex)) = (h.shieldflash.as_ref(), h.shieldtex) {
        crate::shield::player_render_shield(t.gfx, tex, f);
    }
    if let Some((apparent, armour, heightfrac)) = h.health {
        crate::health::draw_health_bar(t.gfx, h.view, apparent, armour, heightfrac, h.fovy);
    }
    // No sight while the active menu is open (sight.c:1418). The default
    // sight (and the zoom's, which ends with it) draws its boxes first.
    if h.activemenu.0.is_none() {
        let right = &h.gun.hands[HAND_RIGHT];
        let sight = if h.playercount >= 2 { SIGHT_DEFAULT } else { h.gset.gset_get_sight(h.gun.bgun_get_weapon_num(HAND_RIGHT), right.weaponfunc) };
        if sight != SIGHT_NONE && sight != SIGHT_MAIAN {
            sight_draw_target_boxes(t, h);
        }
        sight_draw(t.gfx, h);
    }
    if h.option(OPTION_AMMOONSCREEN) {
        bgun_draw_hud(t, h);
    }
    // radar_render, then hudmsgs_render (NTSC 1.0+, `player.c:4742`).
    if let Some(r) = &h.radar {
        crate::radar::radar_render(t.gfx, h.view, (h.playercount, h.playernum, h.vsplit()), h.radartex, r);
    }
    let timerthing = crate::hudmsg::hudmsgs_render(t, &h.hudmsgs);
    if timerthing {
        if let Some((cur, max)) = zoom_range(h) {
            crate::hudmsg::hudmsg_render_zoom_range(t, h.view, (h.playercount, h.playernum, h.vsplit()), cur, max);
        }
    }
    crate::health::draw_fade(t.gfx, h.view, h.fade.0, h.fade.1);
    // lv_render (`lv.c:1595`): the scenario's HUD after the player's, then
    // (after the sky's overexposure) the active menu (`lv.c:1639`).
    scenario_render_hud(t, h.view, &h.scenario);
    if let Some(colour) = h.displayteam {
        scenario_render_display_team(t.gfx, h.view, h.playercount, h.playernum, colour);
    }
    crate::activemenu::am_render(t, h.activemenu.0.as_ref(), h.activemenu.1, h.view, h.playercount, h.playernum);
}

/// Display Team's line (`scenario_render_hud`, `scenarios.c:589`): one pixel
/// of the team's colour (`gDPFillRectangle` in fill mode, both corners
/// inclusive) along the view's bottom for a view above another, else its top.
/// PD's @bug kept: a vertical split draws it at the top or bottom too.
pub fn scenario_render_display_team(gfx: &mut Gfx, view: [i32; 4], playercount: usize, playernum: usize, fill: u32) {
    let [viewleft, viewtop, viewwidth, viewheight] = view;
    let viewright = viewleft + viewwidth;
    let y = if (playercount >= 3 && playernum <= 1) || (playercount == 2 && playernum == 0) { viewheight + viewtop - 1 } else { viewtop };
    // The fill word's first RGBA5551 pixel, opaque.
    let c = fill >> 16;
    let ch = |v: u32| (v << 3) | (v >> 2);
    let rgba = ch((c >> 11) & 0x1f) << 24 | ch((c >> 6) & 0x1f) << 16 | ch((c >> 1) & 0x1f) << 8 | 0xff;
    gfx.fill_rect(viewleft, y, viewright, y + 1, rgba);
}

/// `hudmsg_is_zoom_range_visible` (`hudmsg.c:91`) and the zoom levels
/// `hudmsg_render_zoom_range` shows: one player, "show zoom range" on, a zoom
/// sight; the Sniper Rifle unzoomed reads 1.00X / 1.00X.
fn zoom_range(h: &HudIn) -> Option<(f32, f32)> {
    let right = &h.gun.hands[HAND_RIGHT];
    let weaponnum = h.gun.bgun_get_weapon_num(HAND_RIGHT);
    if !h.option(OPTION_SHOWZOOMRANGE) || h.playercount != 1 || h.gset.gset_get_sight(weaponnum, right.weaponfunc) != SIGHT_ZOOM {
        return None;
    }
    let zoomfov = h.gun.gset_get_gun_zoom_fov(h.gset);
    if zoomfov == 0.0 || zoomfov == 60.0 {
        return (right.weaponnum == WEAPON_SNIPERRIFLE).then_some((1.0, 1.0));
    }
    let maxzoom = 60.0 / zoomfov;
    Some((maxzoom - 1.0 / (zoomfov / h.zoominfovy) + 1.0, maxzoom))
}

/// `scenario_render_hud`'s scenario part (`scenarios.c:557`): a countdown
/// (`htb_render_hud`, `pac_render_hud`, `koh_render_hud`) centred at the
/// view's top + 10 — measured in HandelGothic XS but drawn in the numeric font,
/// as PD does, over a black box of alpha 0 (drawn, invisible) — or Hacker
/// Central's bar (`htm_render_hud`): a third of the view wide at its top, rows
/// 8..16, dark red with 1-pixel strips every 2 for the downloaded part.
pub fn scenario_render_hud(t: &mut TextCtx, view: [i32; 4], s: &pd_sim::mp::scenario::ScenarioHud) {
    use pd_sim::mp::scenario::ScenarioHud;
    let [viewleft, viewtop, viewwidth, _] = view;
    match s {
        ScenarioHud::None => {}
        ScenarioHud::Countdown { text } => {
            let mut x = viewleft + viewwidth / 2;
            let mut y = viewtop + 10;
            let (textheight, textwidth) = text::measure(t.fonts.get(FontId::Xs), text, 0);
            x -= textwidth / 2;
            // PD's @bug: the box's "width" is its right edge, so it reaches
            // past the text; black at alpha 0 either way.
            let (right, bottom) = (textwidth + x, textheight + y);
            t.gfx.fill_rect(x - 1, y - 1, right + x + 1, bottom + y + 1, 0x00000000);
            let (w, hgt) = (t.gfx.w as i32, t.gfx.h as i32);
            t.render_v1(&mut x, &mut y, text, FontId::Numeric, 0x00ff00a0, 0x000000a0, w, hgt, 0, 0);
        }
        ScenarioHud::DownloadBar { frac } => {
            let t6 = (viewleft + viewleft + viewwidth) / 2;
            let a1 = viewwidth / 3;
            let barleft = t6 - a1 / 2;
            let barright = t6 + a1 / 2;
            let s1 = barleft + (a1 as f32 * frac) as i32;
            t.gfx.fill_rect(barleft, viewtop + 8, barright, viewtop + 16, 0x60000060);
            let (mut a0, mut v1) = (barleft, barleft + 1);
            while v1 < s1 {
                t.gfx.fill_rect(a0, viewtop + 8, v1, viewtop + 16, 0xc00000d0);
                v1 += 2;
                a0 += 2;
            }
        }
    }
}

/// Lay the part of a premultiplied layer `layer_w` pixels wide that falls in
/// `rect` (left, top, width, height) over the same pixels of `gfx`.
pub fn composite_over_rect(gfx: &mut n64::rdp::Gfx, layer: &[[f32; 4]], layer_w: usize, rect: [i32; 4]) {
    if layer.is_empty() || layer_w == 0 {
        return;
    }
    let layer_h = layer.len() / layer_w;
    let [left, top, w, h] = rect;
    for y in top.max(0)..(top + h).min(layer_h as i32).min(gfx.h as i32) {
        for x in left.max(0)..(left + w).min(layer_w as i32).min(gfx.w as i32) {
            let s = layer[y as usize * layer_w + x as usize];
            let d = &mut gfx.fb[y as usize * gfx.w + x as usize];
            let ia = 1.0 - s[3];
            for i in 0..4 {
                d[i] = s[i] + d[i] * ia;
            }
        }
    }
}

/// The `rect` (left, top, width, height) of a layer as a layer of its own.
pub fn crop(gfx: &Gfx, rect: [i32; 4]) -> Gfx {
    let [left, top, w, h] = rect;
    let mut out = layer(w.max(0) as usize, h.max(0) as usize);
    for y in 0..out.h {
        let sy = y as i32 + top;
        if sy < 0 || sy >= gfx.h as i32 {
            continue;
        }
        for x in 0..out.w {
            let sx = x as i32 + left;
            if sx >= 0 && sx < gfx.w as i32 {
                out.fb[y * out.w + x] = gfx.fb[sy as usize * gfx.w + sx as usize];
            }
        }
    }
    out
}

/// Copy a view's layer into the framebuffer's at `rect`'s top left.
pub fn paste(frame: &mut Gfx, view: &Gfx, rect: [i32; 4]) {
    for y in 0..view.h {
        let dy = y as i32 + rect[1];
        if dy < 0 || dy >= frame.h as i32 {
            continue;
        }
        for x in 0..view.w {
            let dx = x as i32 + rect[0];
            if dx >= 0 && dx < frame.w as i32 {
                frame.fb[dy as usize * frame.w + dx as usize] = view.fb[y * view.w + x];
            }
        }
    }
}

// ── the GPU overlay ─────────────────────────────────────────────────────────

const OVERLAY_WGSL: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.clip = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
"#;

/// Lays a [`Gfx`] layer (premultiplied RGBA) over the view, pixel for pixel
/// scaled to the target, as the RDP draws the HUD into the frame.
pub struct HudOverlay {
    pipe: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    tex: Option<(wgpu::Texture, wgpu::BindGroup, u32, u32)>,
}

impl HudOverlay {
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> HudOverlay {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pd-hud"), source: wgpu::ShaderSource::Wgsl(OVERLAY_WGSL.into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pd-hud"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pd-hud"), bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pd-hud"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("pd-hud"), mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, ..Default::default() });
        HudOverlay { pipe, bgl, sampler, tex: None }
    }

    /// Upload this frame's layer.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, gfx: &Gfx) {
        let (w, h) = (gfx.w as u32, gfx.h as u32);
        if self.tex.as_ref().is_none_or(|t| t.2 != w || t.3 != h) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pd-hud"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("pd-hud"),
                layout: &self.bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
            });
            self.tex = Some((tex, bind, w, h));
        }
        let px = premultiplied_rgba8(gfx);
        let (tex, _, _, _) = self.tex.as_ref().unwrap();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Draw the layer over the whole of the pass's target.
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        let Some((_, bind, _, _)) = &self.tex else { return };
        rp.set_pipeline(&self.pipe);
        rp.set_bind_group(0, bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

/// A layer's premultiplied pixels as RGBA8.
pub fn premultiplied_rgba8(gfx: &Gfx) -> Vec<u8> {
    gfx.fb.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect()
}

/// A fresh transparent layer of `w` × `h` PD pixels.
pub fn layer(w: usize, h: usize) -> Gfx {
    let mut gfx = Gfx::new(w, h);
    gfx.fb.fill([0.0; 4]);
    gfx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_shader_validates() {
        let module = naga::front::wgsl::parse_str(OVERLAY_WGSL).unwrap_or_else(|e| panic!("{}", e.emit_to_string(OVERLAY_WGSL)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
    }

    /// The ROM fonts decode (`text_load_font`): the numeric '8' is 3×5, and
    /// `bgun_draw_hud_integer` draws it as a green core in its dark halo;
    /// `text_measure` gives "Falcon 2" in HandelGothic XS a width and the XS
    /// line height.
    #[test]
    fn the_hud_draws_a_haloed_digit() {
        let fonts = pd_core::text::Fonts::load(&pd_core::assets::AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let eight = fonts.numeric.ch(b'8');
        assert_eq!((eight.width, eight.height), (3, 5));
        let mut gfx = layer(32, 16);
        let mut ts = pd_core::text::TextState::default();
        let mut t = TextCtx { gfx: &mut gfx, ts: &mut ts, fonts: &fonts, frac20: 0.0 };
        bgun_draw_hud_integer(&mut t, 8, 4, HUDHALIGN_LEFT, 12, HUDVALIGN_BOTTOM, 0x00ff00a0);
        let lit: Vec<[f32; 4]> = gfx.fb.iter().copied().filter(|p| p[3] > 0.0).collect();
        assert!(lit.iter().any(|p| p[1] > 0.3 && p[0] < 0.1), "green core");
        assert!(lit.iter().any(|p| p[1] < 0.05 && p[3] > 0.3), "dark halo");
        let (h, w) = text::measure(fonts.get(FontId::Xs), "Falcon 2
", 0);
        assert!(w > 20 && h > 0, "{w}x{h}");
    }

    /// A 20-round gauge, full: 20 blocks of 3 px (57 / 20 rounds to 3) with
    /// 1 px gaps, all in the filled colour.
    #[test]
    fn a_full_magazine_draws_one_block_per_round() {
        let mut gfx = layer(64, 80);
        let abmag = Abmag { loadedammo: 0, change: 0, ref_: 20, timer60: 0 };
        bgun_draw_hud_gauge(&mut gfx, 10, 5, 19, 62, &abmag, 20, 0x00300080, 0x00ff0040, false);
        let lit: Vec<bool> = (0..80).map(|y| gfx.fb[y * 64 + 12][3] > 0.0).collect();
        let runs = lit.windows(2).filter(|w| !w[0] && w[1]).count() + lit[0] as usize;
        assert_eq!(runs, 20);
        let p = gfx.fb[61 * 64 + 12];
        assert!(p[1] > p[0] && p[1] > p[2], "filled green: {p:?}");
    }
}
