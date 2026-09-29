//! A match and the menus over it, coupled the way PD couples them each frame
//! (`lv_tick` runs `menu_tick`, and the menus read and write the match's
//! globals): a player's open menu takes its controls, START opens its pause
//! menu, a dying player's menu closes, the end of the match opens the end
//! screens, and the menus' answers (pause, End Game, the inventory, back to
//! the Combat Simulator) reach the match. The game (`perfect_dark`) and the
//! snapshots (`pd_snapshot flow`) both step a match through [`step`].

use pd_core::events::Event;
use pd_core::lv::Lv;
use pd_menu::{InvRow, MatchView, MatchViewPlayer, MenuSystem, Outcome};
use pd_sim::mp::ModalText;
use pd_sim::player::PlayerInput;
use pd_sim::world::World;

/// What a frame of the match and its menus left for the caller.
pub struct Stepped {
    /// The world's events (sounds, kills, ...).
    pub events: Vec<Event>,
    /// The menus' (their sounds).
    pub menu_events: Vec<Event>,
    /// The end screens have all closed (`g_MpReturningFromMatch`): leave the
    /// match and `return_from_match`.
    pub over: bool,
    /// The match ended this frame (`mp_end_match`): the end screen's blurred
    /// backdrop is this frame's picture, drawn without the menus.
    pub ended: bool,
}

/// One frame in PD's order (set the menus' pads first): `lv_tick` decides
/// the frame from the menus as they are (a player's open menu pauses a
/// one-player match and takes its controls, `g_PlayersWithControl`), runs
/// `menu_tick`, then the players' controls (START opens a pause menu) and the
/// rest of the world (`diffframe240` quarter-ticks), and last the menus are
/// drawn. What the menus asked of the match applies after the world's frame.
pub fn step(world: &mut World, menu: &mut MenuSystem, lv: &Lv, diffframe240: i32, inputs: &[PlayerInput]) -> Stepped {
    for i in 0..world.players.len() {
        let slot = world.setup.players[i].slot as usize;
        world.set_menu_open(i, menu.menus[slot].curdialog.is_some());
    }
    menu.tick(lv);
    let mut outcomes = Vec::new();
    while let Some(o) = menu.take_outcome() {
        outcomes.push(o);
    }
    world.step(diffframe240, inputs);
    let events = world.take_events();
    let mut ended = false;
    for e in &events {
        match *e {
            Event::MpPushPauseDialog { player } => menu.mp_push_pause_dialog(player as usize),
            Event::AmOpenPickTarget { player, ref targets } => menu.am_open_pick_target(player as usize, targets.clone()),
            Event::MpCloseMenus { player } => menu.close_player_menus(player as usize),
            Event::MpEndMatch => {
                menu.set_match_view(match_view(world));
                menu.mp_end_match(&world.mp.results);
                ended = true;
            }
            _ => {}
        }
    }
    menu.set_match_view(match_view(world));
    menu.render();
    // mp_render_modal_text's "Press START" keeps the pause menu shut
    // (openinhibit, mplayer.c:1291).
    for i in 0..world.players.len() {
        if matches!(world.mp_modal_text(i), ModalText::PressStart { .. }) {
            menu.menus[world.setup.players[i].slot as usize].openinhibit = 10;
        }
    }
    let menu_events = menu.take_events();
    let mut over = false;
    for o in outcomes {
        match o {
            Outcome::SetPaused(mode) => world.mp_set_paused(mode),
            Outcome::EndGame { playernum } => world.mp_end_game(playernum),
            Outcome::Equip { playernum, index } => world.mp_equip_inventory(playernum, index),
            Outcome::PickTarget { playernum, chrnum } => world.am_pick_target(playernum, chrnum),
            Outcome::ReturnFromMatch => over = true,
            Outcome::StartMatch(_) => {}
        }
    }
    Stepped { events, menu_events, over, ended }
}

/// The menus draw something (a dialog, or a background fading): their frame
/// goes over the match's HUD.
pub fn menu_showing(menu: &MenuSystem) -> bool {
    let md = &menu.menudata;
    md.count > 0 || md.bg != 0 || md.nextbg != 255
}

/// The match as its pause and end-of-match dialogs read it (PD's read the
/// globals: `g_MpSetup.paused`, the mpchrconfigs, `g_Vars.players[]`).
pub fn match_view(w: &World) -> MatchView {
    let gset = &w.res.gset;
    MatchView {
        paused: w.mp.paused,
        endscreen: w.mp.endscreen,
        stagetime60: w.mp.stagetime60,
        chrs: w.mp.chrs,
        scenario: w.scenario_scores(),
        players: (0..w.players.len())
            .map(|i| {
                let p = &w.players[i];
                let v = &w.mp.players[i];
                MatchViewPlayer {
                    slot: w.setup.players[i].slot as usize,
                    view: [p.cam.c_screenleft as i32, p.cam.c_screentop as i32, p.cam.c_screenwidth as i32, p.cam.c_screenheight as i32],
                    inventory: p
                        .gun
                        .p
                        .inventory
                        .weapons()
                        .into_iter()
                        .map(|(wn, _)| {
                            let def = gset.weapon(wn);
                            // lang_get's strings end in a line break.
                            InvRow { weaponnum: wn, name: def.map_or(String::new(), |d| format!("{}\n", d.name.trim_end())), description: def.map_or(String::new(), |d| d.description.clone()) }
                        })
                        .collect(),
                    invcur: w.inv_get_current_index(i),
                    weapon_of_choice: format!("{}\n", w.mp_player_get_weapon_of_choice_name(i).trim_end()),
                    award1: v.award1,
                    award2: v.award2,
                    aborted: v.aborted,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use n64::pad::*;
    use pd_core::assets::AssetDir;
    use pd_core::ids::*;
    use pd_menu::mpstate::Profile;
    use pd_menu::types::{MENUROOT_MPENDSCREEN, MENUROOT_MPPAUSE};
    use pd_sim::stage::{Stage, TileLevel};
    use pd_sim::world::WorldRes;

    use super::*;

    struct T {
        world: World,
        menu: MenuSystem,
        lv: Lv,
        over: bool,
        ended: bool,
    }

    impl T {
        fn frame(&mut self, held: u16) {
            self.frame_stick(held, 0);
        }

        fn frame_stick(&mut self, held: u16, sticky: i8) {
            self.menu.pads[0].next_frame(held, 0, sticky);
            self.menu.pads[0].connected = true;
            self.lv.frametime_apply(1, 4);
            let input = PlayerInput { pad: true, a_held: held & A_BUTTON != 0, start: held & START_BUTTON != 0, fire: held & Z_TRIG != 0, look_y: sticky as i32, ..PlayerInput::default() };
            let out = step(&mut self.world, &mut self.menu, &self.lv, 4, &[input]);
            self.over |= out.over;
            self.ended |= out.ended;
        }

        fn tap(&mut self, button: u16) {
            self.frame(button);
            for _ in 0..8 {
                self.frame(0);
            }
        }

        fn dialog(&self) -> Option<&'static str> {
            self.menu.menus[0].curdialog.map(|d| self.menu.menus[0].dialogs[d].def().name)
        }
    }

    fn start() -> T {
        start_with(|_| {})
    }

    fn start_with(edit: impl FnOnce(&mut MenuSystem)) -> T {
        let assets = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
        let mut menu = MenuSystem::new(&assets, Profile::Complete).unwrap();
        menu.open_combat_simulator();
        menu.mp.setup.stagenum = STAGE_MP_COMPLEX;
        menu.mp.setup.chrslots = 0b1_0001;
        menu.mp.bots[0].base.name = "Sim 1\n".into();
        menu.mp.bots[0].difficulty = BOTDIFF_NORMAL;
        edit(&mut menu);
        menu.start_match();
        let Some(Outcome::StartMatch(setup)) = menu.take_outcome() else { panic!("no match") };
        let stage = Stage::load(&assets, "ref").unwrap();
        let level = TileLevel::for_stage(&stage);
        let world = World::new(setup, Arc::new(stage), Arc::new(level), Arc::new(WorldRes::load(&assets).unwrap()), 5).unwrap();
        T { world, menu, lv: Lv::new(), over: false, ended: false }
    }

    /// START opens the pause menu over the match and pauses it; its Control
    /// dialog's End Game, confirmed, ends the match; the end screens open
    /// (Game Over, after the offer to save the player), and when they close
    /// the match is over and the menus come back.
    #[test]
    fn the_pause_menu_ends_the_match_and_the_end_screens_return_to_the_menus() {
        let mut t = start();
        for _ in 0..60 {
            t.frame(0);
        }
        t.tap(START_BUTTON);
        assert_eq!(t.dialog(), Some("g_MpPausePlayerRankingMenuDialog"));
        assert_eq!(t.menu.menudata.root, MENUROOT_MPPAUSE);
        assert!(t.world.mp_is_paused());
        let time = t.world.mp.stagetime60;
        for _ in 0..3 {
            t.tap(R_JPAD);
        }
        assert_eq!(t.dialog(), Some("g_MpPauseControlMenuDialog"));
        assert_eq!(t.world.mp.stagetime60, time, "paused");
        // End Game → "Are you sure?" (Cancel focused) → End Game.
        t.tap(A_BUTTON);
        assert_eq!(t.dialog(), Some("g_MpEndGameMenuDialog"));
        t.tap(D_JPAD);
        t.tap(A_BUTTON);
        assert!(t.ended && t.world.mp.endscreen && t.world.mp.players[0].aborted);
        for _ in 0..30 {
            t.frame(0);
        }
        assert_eq!(t.menu.menudata.root, MENUROOT_MPENDSCREEN);
        assert_eq!(t.dialog(), Some("g_MpEndscreenSavePlayerMenuDialog"));
        t.tap(B_BUTTON);
        assert_eq!(t.dialog(), Some("g_MpEndscreenIndGameOverMenuDialog"));
        assert!(!t.over);
        t.tap(START_BUTTON);
        assert!(t.over, "the end screens closed but the match went on");
        t.menu.return_from_match();
        for _ in 0..10 {
            t.menu.frame(&t.lv);
        }
        assert_eq!(t.dialog(), Some("g_CombatSimulatorMenuDialog"));
    }

    /// The active menu's Attack order (teams on, a simulant teammate):
    /// holding A opens it, Z moves on to the simulant's orders, Z on Attack
    /// hands over to the menus' Pick Target, and picking the enemy there sends
    /// the teammate after it.
    #[test]
    fn attack_picks_its_target_in_the_menus() {
        let mut t = start_with(|m| {
            m.mp.setup.options |= MPOPTION_TEAMSENABLED;
            m.mp.players[0].base.team = 0;
            m.mp.bots[0].base.team = 1;
            m.mp.setup.chrslots |= 1 << 5;
            m.mp.bots[1].base.name = "Sim 2\n".into();
            m.mp.bots[1].difficulty = BOTDIFF_NORMAL;
            m.mp.bots[1].base.team = 0;
        });
        for _ in 0..60 {
            t.frame(0);
        }
        let mate = t.world.players[0].aibuddynums[0];
        let enemy = (1..t.world.chrs.len()).find(|&i| i != mate).unwrap();
        for _ in 0..20 {
            t.frame(A_BUTTON);
        }
        for _ in 0..4 {
            if t.world.players[0].am.screenindex == 2 {
                break;
            }
            t.frame(A_BUTTON | Z_TRIG);
            t.frame(A_BUTTON);
        }
        assert_eq!(t.world.players[0].am.screenindex, 2);
        for _ in 0..3 {
            t.frame_stick(A_BUTTON, 80);
        }
        t.frame_stick(A_BUTTON | Z_TRIG, 80);
        for _ in 0..10 {
            t.frame(0);
        }
        assert_eq!(t.dialog(), Some("g_AmPickTargetMenuDialog"));
        // Down to the enemy's row, then A.
        let row = t.menu.picktargets[0].iter().position(|&(c, _)| c as usize == enemy).unwrap();
        for _ in 0..row {
            t.tap(D_JPAD);
        }
        t.tap(A_BUTTON);
        assert_eq!(t.dialog(), None);
        let a = t.world.chrs[mate].aibot.as_ref().unwrap();
        assert_eq!((a.command, a.attackpropnum), (AIBOTCMD_ATTACK, Some(enemy)));
    }
}
