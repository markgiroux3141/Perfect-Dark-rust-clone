//! The save files through PD's own code: each file saved to the Game Pak,
//! read back by a menu system booted from the EEPROM image, and saved again
//! bit for bit; and the agent select's New Agent, as a player makes one.

use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_core::savebuffer::{cstr, cstr_to_string, FileGuid};

use super::mpstate::Profile;
use super::MenuSystem;

fn assets() -> AssetDir {
    AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))
}

fn frame(pd: &mut MenuSystem, buttons: u16) {
    pd.pads[0].next_frame(buttons, 0, 0);
    let mut lv = pd.lv.clone();
    lv.frametime_apply(1, 4);
    pd.frame(&lv);
}

fn tap(pd: &mut MenuSystem, bit: u16) {
    frame(pd, bit);
    for _ in 0..12 {
        frame(pd, 0);
    }
}

fn cur(pd: &MenuSystem) -> &'static str {
    pd.menus[0].curdialog.map(|d| pd.menus[0].dialogs[d].def().name).unwrap_or("-")
}

/// A free file of `filetype` on the Game Pak (`filelist_update`'s
/// `deviceguids`).
fn free_file(pd: &mut MenuSystem, filetype: i32) -> FileGuid {
    pd.filelist_create(3, filetype as u8);
    pd.filelists_tick();
    let g = pd.filelists.lists[3].as_ref().unwrap().deviceguids[SAVEDEVICE_GAMEPAK as usize];
    pd.filelists.lists[3] = None;
    g
}

/// A file's body as the Game Pak holds it.
fn body(pd: &mut MenuSystem, guid: FileGuid) -> Vec<u8> {
    let mut b = vec![0u8; 220];
    assert_eq!(pd.paks.pak_read_body_at_guid(SAVEDEVICE_GAMEPAK, guid.fileid, Some(&mut b), 0), 0);
    b
}

/// The agent, a player, a setup and the boss file saved, the Game Pak's
/// image booted by a second menu system that reads them all back (the
/// statistics capped to their bits, a name's break restored), and each saved
/// again from there to the same bytes.
#[test]
fn every_file_is_written_and_read_back_bit_for_bit() {
    let mut pd = MenuSystem::new(&assets(), Profile::Files).unwrap();
    assert_eq!(pd.load_agent_without_select(), "Dark");
    // The agent.
    pd.gamefile.name = cstr::<11>("Joanna");
    pd.gamefile.totaltime = 123_456;
    pd.gamefile.besttimes[SOLOSTAGEINDEX_INFILTRATION as usize][1] = 0x345;
    pd.fr_set_weapon_found(WEAPON_LAPTOPGUN as i32);
    pd.challenge_set_completed_by_any_player_with_num_players(0, 2, true);
    pd.vars.screensplit = SCREENSPLIT_VERTICAL;
    pd.options_set_music_volume(0x3000);
    let agent = pd.gamefileguid;
    assert_eq!(pd.gamefile_save(SAVEDEVICE_GAMEPAK, agent.fileid, agent.deviceserial), 0);
    assert_eq!(pd.gamefileguid, agent, "a save keeps the file's id");
    // A player.
    pd.mp.players[0].base.name = "Velvet\n".into();
    pd.mp.players[0].base.mpbodynum = MPBODY_CASSANDRA;
    pd.mp.players[0].career.kills = 4321;
    pd.mp.players[0].career.accuracy = 5000;
    pd.mp.players[0].career.time = 3 * 3600 + 5 * 60;
    pd.mp.players[0].gunfuncs = [0x81, 0x42, 0x24, 0x18, 0x05, 0];
    pd.challenge_set_completed_by_player_with_num_players(0, 3, 1, true);
    let pguid = free_file(&mut pd, FILETYPE_MPPLAYER);
    assert_eq!(pd.mpplayerfile_save(0, SAVEDEVICE_GAMEPAK, pguid.fileid, pguid.deviceserial), 0);
    assert_eq!(pd.mp.players[0].career.accuracy, 1023, "capped to its 10 bits");
    // A setup.
    pd.mp.setup.name = "Hill".into();
    pd.mp.setup.scenario = MPSCENARIO_KINGOFTHEHILL;
    pd.vars.mphilltime = 25;
    pd.mp.setup.stagenum = STAGE_MP_COMPLEX;
    pd.mp_create_bot_from_profile(0, 2);
    let sguid = free_file(&mut pd, FILETYPE_MPSETUP);
    assert_eq!(pd.mpsetupfile_save(SAVEDEVICE_GAMEPAK, sguid.fileid, sguid.deviceserial), 0);
    // The boss file.
    pd.mp.bossfile.teamnames[2] = "Carrington\n".into();
    pd.mp.bossfile.tracknum = 7;
    pd.bossfile_save();
    let bodies = [body(&mut pd, agent), body(&mut pd, pguid), body(&mut pd, sguid)];

    let mut q = MenuSystem::new_with_eeprom(&assets(), Profile::Files, Some(pd.paks.eeprom.clone())).unwrap();
    assert_eq!(q.mp.bossfile.teamnames[2], "Carrington\n");
    assert_eq!(q.mp.bossfile.tracknum, 7);
    assert_eq!(q.load_agent_without_select(), "Joanna", "the agent the boss file names");
    assert_eq!(q.gamefile, pd.gamefile);
    assert!(q.challenge_is_completed_by_any_player_with_num_players(0, 2));
    assert!(q.fr_is_weapon_found(WEAPON_LAPTOPGUN as i32));
    assert_eq!(q.vars.screensplit, SCREENSPLIT_VERTICAL);
    assert_eq!(q.options.musicvolume, 0x3000);
    assert_eq!(q.mpplayerfile_load(1, SAVEDEVICE_GAMEPAK, pguid.fileid, pguid.deviceserial), 0);
    let (a, b) = (&pd.mp.players[0], &q.mp.players[1]);
    assert_eq!(b.base.name, "Velvet\n");
    assert_eq!((b.base.mpbodynum, b.career, b.fileguid), (a.base.mpbodynum, a.career, pguid));
    assert_eq!(&b.gunfuncs[..5], &a.gunfuncs[..5], "35 bits of gun functions");
    assert!(q.challenge_is_completed_by_player_with_num_players(1, 3, 1));
    assert_eq!(q.mpsetupfile_load(SAVEDEVICE_GAMEPAK, sguid.fileid, sguid.deviceserial), 0);
    assert_eq!((q.mp.setup.name.as_str(), q.mp.setup.scenario, q.vars.mphilltime), ("Hill", MPSCENARIO_KINGOFTHEHILL, 25));
    assert_eq!(q.mp.setup.chrslots & 0x10, 0x10, "the simulant came back");

    // Saved again from what was read: the same bytes.
    let a = q.gamefileguid;
    assert_eq!(q.gamefile_save(SAVEDEVICE_GAMEPAK, a.fileid, a.deviceserial), 0);
    assert_eq!(q.mpplayerfile_save(1, SAVEDEVICE_GAMEPAK, pguid.fileid, pguid.deviceserial), 0);
    assert_eq!(q.mpsetupfile_save(SAVEDEVICE_GAMEPAK, sguid.fileid, sguid.deviceserial), 0);
    let again = [body(&mut q, a), body(&mut q, pguid), body(&mut q, sguid)];
    for (i, (x, y)) in bodies.iter().zip(&again).enumerate() {
        assert_eq!(x, y, "file {i}");
    }
}

/// Power on a blank Game Pak: the agent select offers only New Agent; its
/// name keyboard (START: "Dark") asks where, the Game Pak shows its four
/// free spaces, and the agent is saved there. Back on the list, the new
/// agent is focused; loading it opens the Perfect Menu, and the next power on
/// lists it.
#[test]
fn a_new_agent_is_made_on_the_game_pak() {
    use n64::pad::*;
    let mut pd = MenuSystem::new(&assets(), Profile::Files).unwrap();
    pd.open_file_select();
    for _ in 0..40 {
        frame(&mut pd, 0);
    }
    assert_eq!(cur(&pd), "g_FilemgrFileSelectMenuDialog");
    assert_eq!(pd.filelists.lists[0].as_ref().map(|l| l.numfiles()), Some(0));
    tap(&mut pd, A_BUTTON);
    assert_eq!(cur(&pd), "g_FilemgrEnterNameMenuDialog");
    tap(&mut pd, START_BUTTON);
    assert_eq!(cur(&pd), "g_FilemgrSelectLocationMenuDialog");
    let l = pd.filelists.lists[pd.menus[0].fm.listnum as usize].as_ref().unwrap();
    assert_eq!(l.spacesfree[SAVEDEVICE_GAMEPAK as usize], 4, "five agent files, one the swap");
    assert!(l.spacesfree[..4].iter().all(|&s| s == -1), "no Controller Paks");
    tap(&mut pd, A_BUTTON);
    for _ in 0..20 {
        frame(&mut pd, 0);
    }
    assert_eq!(cur(&pd), "g_FilemgrFileSelectMenuDialog");
    assert_eq!(pd.filelists.lists[0].as_ref().map(|l| l.numfiles()), Some(1));
    let saved = pd.gamefileguid;
    assert_ne!(saved.fileid, 0);
    tap(&mut pd, A_BUTTON);
    for _ in 0..40 {
        frame(&mut pd, 0);
    }
    assert_eq!(cur(&pd), "g_CiMenuViaPcMenuDialog");
    assert_eq!((pd.vars.bossfileid, pd.vars.bossdeviceserial), (saved.fileid, saved.deviceserial));
    assert_eq!(cstr_to_string(&pd.gamefile.name), "Dark");

    let mut q = MenuSystem::new_with_eeprom(&assets(), Profile::Files, Some(pd.paks.eeprom.clone())).unwrap();
    assert_eq!(q.vars.bossfileid, saved.fileid, "the boss file names the last agent");
    q.open_file_select();
    for _ in 0..10 {
        frame(&mut q, 0);
    }
    let l = q.filelists.lists[0].as_ref().unwrap();
    assert_eq!(l.numfiles(), 1);
    assert_eq!(MenuSystem::gamefile_get_overview(&l.files[0].name).0, "Dark");
}

/// A saved player is a Combat Simulator Player File that the Combat
/// Simulator's player list (Load Player's) shows, named with its play time.
#[test]
fn the_load_player_list_shows_saved_players() {
    let mut pd = MenuSystem::new(&assets(), Profile::Files).unwrap();
    pd.load_agent_without_select();
    pd.mp.players[0].base.name = "Joanna\n".into();
    pd.mp.players[0].career.time = 2 * 3600 + 7 * 60 + 30;
    let g = free_file(&mut pd, FILETYPE_MPPLAYER);
    assert_eq!(pd.mpplayerfile_save(0, SAVEDEVICE_GAMEPAK, g.fileid, g.deviceserial), 0);
    pd.open_combat_simulator();
    for _ in 0..5 {
        frame(&mut pd, 0);
    }
    let list = pd.filelists.lists[0].clone().expect("the Combat Simulator's player list");
    assert_eq!(list.numfiles(), 1);
    assert_eq!(pd.filemgr_get_select_name(&list.files[0], FILETYPE_MPPLAYER), "Joanna-2:07\n");
}

/// Not PD: the Tester agent. On a blank Game Pak it is made once (a second
/// call finds it); the menus' own agent stays the defaults; loaded, it has
/// every arena, weapon and challenge PD's unlock rules open, and a second boot
/// from the image still lists it.
#[test]
fn the_tester_agent_has_everything_unlocked() {
    let mut pd = MenuSystem::new(&assets(), Profile::Files).unwrap();
    let locked = crate::generated::MP_ARENAS.iter().filter(|a| !pd.challenge_is_feature_unlocked(a.requirefeature)).count();
    assert!(locked > 0, "a new agent has arenas to unlock");
    assert!(pd.make_tester_agent());
    assert!(!pd.make_tester_agent(), "made once");
    assert_eq!(cstr_to_string(&pd.gamefile.name), "Dark", "the menus' agent is the default still");
    let mut q = MenuSystem::new_with_eeprom(&assets(), Profile::Files, Some(pd.paks.eeprom.clone())).unwrap();
    assert!(!q.make_tester_agent(), "found on the next boot");
    assert_eq!(q.load_agent_without_select(), super::filemgr::TESTER_AGENT);
    assert!(crate::generated::MP_ARENAS.iter().all(|a| q.challenge_is_feature_unlocked(a.requirefeature)));
    // Entering the Combat Simulator works the unlocks out again (the load did it before the weapons found).
    q.open_combat_simulator();
    let locked: Vec<i32> = crate::generated::MP_WEAPONS.iter().filter(|w| !q.challenge_is_feature_unlocked(w.unlockfeature)).map(|w| w.weaponnum).collect();
    assert!(locked.is_empty(), "locked weapons: {locked:?}");
    assert!((0..q.mp.challenges.len()).all(|i| q.challenge_is_completed_by_any_player_with_num_players(i, 1)));
}
