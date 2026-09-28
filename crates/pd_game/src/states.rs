//! Game states. **Menus**: the `pd_menu` framebuffer presented full screen.
//! **Match**: for now a stand-in screen that shows the `MatchSetup` the menus
//! handed over, until M3's world exists; later the `pd_sim` world and
//! `pd_render` views with the pause menu composited over. **Results** (PD's
//! end-of-match dialogs, which are menus too) arrive with M7.

use n64::rdp::Gfx;
use pd_core::mp::MatchSetup;
use pd_core::text::{FontId, Fonts, TextCtx, TextState};

pub enum Screen {
    Menus,
    /// SUBST: PD loads the arena and starts the match here / there is no match
    /// until M3, so the game shows what would start and goes back to the menus
    /// on START or A, as PD does after a match (menutick.c:217).
    Match { setup: MatchSetup, lines: Vec<String> },
}

/// The match stand-in, drawn with PD's own text into a 320×220 frame.
pub fn draw_match_stand_in(gfx: &mut Gfx, fonts: &Fonts, lines: &[String]) {
    gfx.clear([0.02, 0.03, 0.08]);
    gfx.full_scissor();
    let (w, h) = (gfx.w as i32, gfx.h as i32);
    let mut ts = TextState::default();
    let mut ctx = TextCtx { gfx, ts: &mut ts, fonts, frac20: 0.0 };
    let (mut x, mut y) = (16, 14);
    ctx.render_v2(&mut x, &mut y, "Match Setup\n", FontId::Md, 0xffff00ff, w, h, 0, 0);
    let (mut x, mut y) = (16, 40);
    ctx.render_v2(&mut x, &mut y, &(lines.join("\n") + "\n"), FontId::Xs, 0xc0e0ffff, w, h, 0, 0);
    let (mut x, mut y) = (16, h - 22);
    ctx.render_v2(&mut x, &mut y, "No match yet (M3). START: back to the menus\n", FontId::Xs, 0x8090a0ff, w, h, 0, 0);
}
