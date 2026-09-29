//! Headless checks on the menu port: the generated tables are PD's, the
//! dialogs open and navigate, and random input never panics.

use pd_core::assets::AssetDir;

use super::generated as gd;
use super::mpstate::Profile;
use super::types::*;
use super::MenuSystem;

fn pd() -> MenuSystem {
    MenuSystem::new(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR")), Profile::Complete).expect("assets/: run tools/pd-assets/build_assets.py")
}

/// One 60 Hz frame with controller 1 holding `buttons` and its stick at `stick_x`.
fn frame_with(pd: &mut MenuSystem, buttons: u16, stick_x: i8) {
    pd.pads[0].next_frame(buttons, stick_x, 0);
    let mut lv = pd.lv.clone();
    lv.frametime_apply(1, 4);
    pd.frame(&lv);
}

trait Frame {
    fn frame1(&mut self);
}

impl Frame for MenuSystem {
    fn frame1(&mut self) {
        frame_with(self, 0, 0);
    }
}

fn tap(pd: &mut MenuSystem, bit: u16) {
    frame_with(pd, bit, 0);
    for _ in 0..8 {
        pd.frame1();
    }
}

fn cur(pd: &MenuSystem) -> &'static str {
    pd.menus[0].curdialog.map(|d| pd.menus[0].dialogs[d].def().name).unwrap_or("-")
}

#[test]
fn tables_match_the_decomp() {
    // setup.c:5813: four big-font selectables then END.
    assert_eq!(gd::G_COMBAT_SIMULATOR_MENU_ITEMS.len(), 5);
    assert!(gd::G_COMBAT_SIMULATOR_MENU_ITEMS[..4].iter().all(|i| i.ty == MENUITEMTYPE_SELECTABLE && i.flags & MENUITEMFLAG_BIGFONT != 0));
    assert_eq!(gd::MP_ARENAS.len(), 17);
    assert_eq!(gd::MP_BODIES.len(), 61);
    assert_eq!(gd::MP_CHALLENGES.len(), 30);
    assert_eq!(gd::BOT_PROFILES.len(), 18);
    // ROM mpconfigs: 44 configs, and challenge 1's description is text.
    let pd = pd();
    assert_eq!(pd.draw.res.mpconfigs.len(), 44);
    let c1 = &pd.draw.res.mpconfigs[gd::MP_CHALLENGES[0].confignum as usize];
    assert!(c1.description.len() > 20, "{:?}", c1.description);
}

#[test]
fn strings_resolve() {
    let pd = pd();
    assert_eq!(pd.lang(pd_core::lang::tx(gd::B_MISC, 445)).trim(), "Combat Simulator");
    assert_eq!(pd.lang(pd_core::lang::tx(gd::B_MPMENU, 17)).trim(), "Game Setup");
}

#[test]
fn perfect_menu_to_advanced_setup() {
    let mut pd = pd();
    pd.open_main_menu();
    for _ in 0..40 {
        pd.frame1();
    }
    assert_eq!(cur(&pd), "g_CiMenuViaPcMenuDialog");
    // Carrington Institute is focused first; Combat Simulator is two down.
    tap(&mut pd, D_JPAD);
    tap(&mut pd, D_JPAD);
    tap(&mut pd, A_BUTTON);
    for _ in 0..40 {
        pd.frame1();
    }
    assert_eq!(cur(&pd), "g_CombatSimulatorMenuDialog");
    assert_eq!(pd.menudata.root, MENUROOT_MPSETUP);
    // Advanced Setup is the fourth item.
    for _ in 0..3 {
        tap(&mut pd, D_JPAD);
    }
    tap(&mut pd, A_BUTTON);
    for _ in 0..40 {
        pd.frame1();
    }
    assert_eq!(cur(&pd), "g_MpAdvancedSetupMenuDialog");
    assert_eq!(pd.vars.mpsetupmenu, gd::MPSETUPMENU_ADVSETUP);
    // Its sibling layer is Player Setup, Stuff, Challenges.
    let depth = pd.menus[0].depth;
    assert_eq!(pd.menus[0].layers[depth - 1].numsiblings, 4);
}

#[test]
fn add_a_simulant() {
    let mut pd = pd();
    pd.open_combat_simulator();
    for _ in 0..30 {
        pd.frame1();
    }
    for _ in 0..3 {
        tap(&mut pd, D_JPAD);
    }
    tap(&mut pd, A_BUTTON);
    for _ in 0..30 {
        pd.frame1();
    }
    // Simulants is the 7th entry of Game Setup.
    for _ in 0..6 {
        tap(&mut pd, D_JPAD);
    }
    tap(&mut pd, A_BUTTON);
    assert_eq!(cur(&pd), "g_MpSimulantsMenuDialog");
    tap(&mut pd, A_BUTTON); // Add Simulant...
    assert_eq!(cur(&pd), "g_MpAddSimulantMenuDialog");
    tap(&mut pd, A_BUTTON); // MeatSim
    assert_eq!(cur(&pd), "g_MpSimulantsMenuDialog");
    assert!(pd.mp.setup.chrslots & 0x10 != 0);
    assert!(pd.mp.bots[0].base.name.starts_with("MeatSim"), "{:?}", pd.mp.bots[0].base.name);
}

#[test]
fn random_input_never_panics() {
    let mut pd = pd();
    pd.open_main_menu();
    let mut rng = pd_core::rng::Rng::new(99);
    let bits = [A_BUTTON, B_BUTTON, U_JPAD, D_JPAD, L_JPAD, R_JPAD, START_BUTTON, Z_TRIG, R_TRIG];
    for _ in 0..6000 {
        let b = bits[(rng.random() % bits.len() as u32) as usize];
        let buttons = if rng.random().is_multiple_of(3) { b } else { 0 };
        let stick_x = ((rng.random() % 161) as i32 - 80) as i8 * rng.random().is_multiple_of(4) as i8;
        frame_with(&mut pd, buttons, stick_x);
        if pd.take_outcome().is_some() {
            pd.return_from_match();
        }
    }
}

/// Every MP body (with its default head), every MP head on its own, and the
/// hudpiece load and draw pixels through `menu_render_model`.
#[test]
fn every_menu_model_draws() {
    let mut pd = pd();
    let draw = |pd: &mut MenuSystem, params: u32, modeltype: i32| -> usize {
        let mut mm = super::menu::MenuModel { newparams: params, loaddelay: 1, zoom: 30.0, curscale: 1.0, newscale: 1.0, newanimnum: gd::ANIM_01FC, headnum: -1, bodynum: -1, ..Default::default() };
        if modeltype == MENUMODELTYPE_HUDPIECE {
            mm = super::menu::MenuModel { curposx: -205.5, newposx: -205.5, curposy: 244.7, newposy: 244.7, curposz: 68.3, newposz: 68.3, curscale: 0.12209, newscale: 0.12209, newroty: -std::f32::consts::PI, zoom: -1.0, newanimnum: gd::ANIM_040D, ..mm };
        }
        pd.scissor_menu = [100, 70, 220, 150];
        pd.draw.gfx.clear([0.0; 3]);
        for _ in 0..3 {
            pd.menu_render_model(&mut mm, modeltype);
        }
        assert!(pd.draw.model_inst[if modeltype == MENUMODELTYPE_HUDPIECE { 4 } else { 0 }].is_some(), "params {params:#x} did not load: {:?}", pd.draw.error);
        pd.draw.gfx.fb.iter().filter(|p| p[0] + p[1] + p[2] > 0.0).count()
    };
    for b in 0..gd::MP_BODIES.len() {
        let head = gd::MP_BODIES[b].headnum;
        let mphead = gd::MP_HEADS.iter().position(|h| h.headnum == head).unwrap_or(0);
        let n = draw(&mut pd, 0xffff | (mphead as u32) << 16 | (b as u32) << 24, MENUMODELTYPE_DEFAULT);
        assert!(n > 50, "body {b} drew {n} pixels");
    }
    for h in 0..gd::MP_HEADS.len() {
        let filenum = gd::HEADS_AND_BODIES[gd::MP_HEADS[h].headnum as usize].filenum as u32;
        let n = draw(&mut pd, filenum, MENUMODELTYPE_DEFAULT);
        assert!(n > 50, "head {h} drew {n} pixels");
    }
    let n = draw(&mut pd, gd::FILE_GHUDPIECE as u32, MENUMODELTYPE_HUDPIECE);
    assert!(n > 50, "hudpiece drew {n} pixels");
    // The eye feeds the holoray origin (menu.c:2287) only on the menu roots.
    let _ = pd.draw.text.holoray_fromx;
}

/// Choosing King of the Hill in the Scenario list turns teams on
/// (`koh_init`); Capture the Case too, and folds every chr's team into its four
/// (`ctc_init`). The Hill Options' time goes into the match.
#[test]
fn choosing_a_team_scenario_turns_teams_on() {
    let mut pd = pd();
    pd.mp.setup.options &= !(gd::MPOPTION_TEAMSENABLED as u32);
    pd.mp.setup.chrslots = 0x31;
    pd.mp.bots[1].base.team = 6;
    let item = &gd::G_MP_SCENARIO_MENU_ITEMS[0];
    // Everything unlocked, not a quick team game: the list is the six in order.
    let mut data = HandlerData { value: gd::MPSCENARIO_KINGOFTHEHILL, ..HandlerData::default() };
    super::handlers::scenario_scenario_menu_handler(&mut pd, MENUOP_CONFIRM, item, &mut data);
    assert_eq!(pd.mp.setup.scenario as i32, gd::MPSCENARIO_KINGOFTHEHILL);
    assert!(pd.mp.setup.options & gd::MPOPTION_TEAMSENABLED as u32 != 0);
    assert_eq!(pd.mp.bots[1].base.team, 6);
    let mut data = HandlerData { value: gd::MPSCENARIO_CAPTURETHECASE, ..HandlerData::default() };
    super::handlers::scenario_scenario_menu_handler(&mut pd, MENUOP_CONFIRM, item, &mut data);
    assert_eq!(pd.mp.bots[1].base.team, 2);
    pd.vars.mphilltime = 50;
    assert_eq!(pd.match_setup(pd_core::ids::STAGE_MP_COMPLEX).mphilltime, 50);
}

/// Start Game on the Stuff layer, then A on the Ready dialog: the menus close
/// and hand back PD's `g_MpSetup` as a `MatchSetup`, and come back after it.
#[test]
fn start_game_hands_back_the_match_setup() {
    let script = super::script::Script::parse(&"--combat w40 down down down a w50 right w30 right w30 down down down down down a w40 a w120".split_whitespace().collect::<Vec<_>>()).unwrap();
    let mut pd = script.start(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))).unwrap();
    script.run(&mut pd, |_, _| Ok(())).unwrap();
    let Some(super::Outcome::StartMatch(m)) = pd.take_outcome() else { panic!("no match started; dialog {}", cur(&pd)) };
    assert!(pd.in_match && pd.menus.iter().all(|m| m.curdialog.is_none()));
    // mp_init's defaults: Combat on Skedar, one player, no simulants.
    assert_eq!((m.stagenum, m.scenario), (pd_core::ids::STAGE_MP_SKEDAR, pd_core::ids::MPSCENARIO_COMBAT));
    assert_eq!(m.players.len(), 1);
    assert_eq!(m.players[0].slot, 0);
    assert!(m.simulants.is_empty());
    // mp_init's weapon set is Pistols: slot 1 is the Falcon 2.
    assert_eq!(gd::MP_WEAPONS[m.weapons[0] as usize].weaponnum, pd_core::ids::WEAPON_FALCON2 as i32);
    let lines = pd.describe_match(&m);
    assert_eq!(lines[1], "Arena: Skedar");
    assert!(lines[2].starts_with("Weapons: Falcon 2, "), "{}", lines[2]);
    pd.return_from_match();
    for _ in 0..40 {
        pd.frame1();
    }
    assert!(!pd.in_match);
    assert_eq!(cur(&pd), "g_MpAdvancedSetupMenuDialog");
}

/// On a new save some weapons are locked, so a slot's option index is not its
/// `g_MpWeapons` index: the match summary must still name what the Weapons
/// dialog shows.
#[test]
fn the_match_summary_names_the_weapons_the_dialog_shows() {
    let mut pd = MenuSystem::new(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR")), Profile::Files).unwrap();
    pd.challenge_determine_unlocked_features();
    let shown: Vec<String> = (0..6).map(|s| pd.mp_get_weapon_label(pd.mp_get_weapon_slot(s)).trim().to_string()).collect();
    let m = pd.match_setup(pd.mp.setup.stagenum);
    assert_eq!(pd.describe_match(&m)[2], format!("Weapons: {}", shown.join(", ")));
    assert!(shown.iter().all(|w| !w.is_empty()), "{shown:?}");
}

/// The menus ask for PD's tunes (after power on's volumes): the Perfect Menu `MUSIC_MAINMENU`, the
/// Combat Simulator `MUSIC_COMBATSIM_MENU`; the Soundtrack dialog slows the
/// queue to 80 and previews each tune the cursor rests on (it opens on
/// Random, the default; down wraps to Dark Combat, then Skedar Mystery),
/// and puts the interval back when it closes.
#[test]
fn the_menus_ask_for_their_music() {
    use pd_core::events::Event;
    use pd_core::ids::*;
    use pd_core::music::MusicCall;
    let music = |pd: &mut MenuSystem| -> Vec<MusicCall> { pd.take_events().into_iter().filter_map(|e| if let Event::Music(c) = e { Some(c) } else { None }).collect() };
    let mut pd = pd();
    // Power on: gamefile_load_defaults' volumes.
    assert_eq!(music(&mut pd), [MusicCall::SetSfxVolume(0x5000), MusicCall::SetVolume(0x5000)]);
    pd.open_main_menu();
    assert_eq!(music(&mut pd), [MusicCall::StartTrackAsMenu(MUSIC_MAINMENU)]);
    pd.open_combat_simulator();
    assert_eq!(music(&mut pd), [MusicCall::StartTrackAsMenu(MUSIC_COMBATSIM_MENU)]);
    pd.frame1();
    pd.menu_push_dialog(&gd::G_MP_SELECT_TUNES_MENU_DIALOG);
    pd.frame1();
    let opened = music(&mut pd);
    assert_eq!(opened.first(), Some(&MusicCall::SetInterval(80)), "{opened:?}");
    tap(&mut pd, n64::pad::D_JPAD);
    let moved = music(&mut pd);
    assert!(moved.contains(&MusicCall::StartTrackAsMenu(MUSIC_DARK_COMBAT)), "{moved:?}");
    tap(&mut pd, n64::pad::D_JPAD);
    let moved = music(&mut pd);
    assert!(moved.contains(&MusicCall::StartTrackAsMenu(MUSIC_SKEDAR_MYSTERY)), "{moved:?}");
    tap(&mut pd, n64::pad::B_BUTTON);
    for _ in 0..20 {
        pd.frame1();
    }
    assert!(music(&mut pd).contains(&MusicCall::SetInterval(15)));
}
