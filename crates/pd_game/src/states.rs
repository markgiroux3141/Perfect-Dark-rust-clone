//! Game states. **Menus**: the `pd_menu` framebuffer presented full screen.
//! **Match**: a `pd_sim` world on the chosen arena, drawn by `pd_render`, with
//! the menus' frame laid over its HUD: the pause menu, and at the end PD's
//! end-of-match dialogs (they are menus too) until they close.

use std::collections::HashMap;
use std::sync::Arc;

use n64::rdp::Gfx;
use pd_core::assets::AssetDir;
use pd_core::mp::MatchSetup;
use pd_core::text::{FontId, Fonts, TextCtx, TextState};
use pd_sim::stage::{Stage, TileLevel};
use pd_sim::world::{World, WorldRes};

pub enum Screen {
    Menus,
    /// A match being played.
    Match(Box<World>),
    /// SUBST: PD loads any arena here / only the stages the exporter has
    /// written can be played (Complex until M9), so for the others the game
    /// shows what would start and goes back to the menus on START or A, as PD
    /// does after a match (menutick.c:217).
    StandIn { setup: MatchSetup, lines: Vec<String> },
}

/// Loaded stages and the world's resources (animations, guns, models, sound
/// configs), kept between matches.
pub struct MatchAssets {
    assets: AssetDir,
    stages: HashMap<String, (Arc<Stage>, Arc<TileLevel>)>,
    res: Option<Arc<WorldRes>>,
}

impl MatchAssets {
    pub fn new(assets: &AssetDir) -> MatchAssets {
        MatchAssets { assets: assets.clone(), stages: HashMap::new(), res: None }
    }

    /// Whether `stages/<code>/` has been exported.
    pub fn has_stage(&self, code: &str) -> bool {
        self.assets.stage(code).join("bg.json").exists()
    }

    /// A world for `setup`, on its stage.
    pub fn start(&mut self, setup: MatchSetup, code: &str, seed: u64) -> Result<World, String> {
        if !self.stages.contains_key(code) {
            let stage = Stage::load(&self.assets, code)?;
            let level = TileLevel::new(stage.geom.clone());
            self.stages.insert(code.to_owned(), (Arc::new(stage), Arc::new(level)));
        }
        if self.res.is_none() {
            self.res = Some(Arc::new(WorldRes::load(&self.assets)?));
        }
        let (stage, level) = self.stages[code].clone();
        World::new(setup, stage, level, self.res.clone().unwrap(), seed)
    }
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
    ctx.render_v2(&mut x, &mut y, "This arena is not exported yet (M9). START: back to the menus\n", FontId::Xs, 0x8090a0ff, w, h, 0, 0);
}
