//! The animation bank and `struct anim` against the exported bank.

use std::sync::OnceLock;

use super::*;
use crate::assets::AssetDir;

pub(crate) fn assets() -> AssetDir {
    AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn bank() -> &'static AnimBank {
    static BANK: OnceLock<AnimBank> = OnceLock::new();
    BANK.get_or_init(|| AnimBank::load(&assets()).expect("assets/anims: run tools/pd-assets/build_assets.py"))
}

#[test]
fn the_whole_bank_loads_under_pds_numbers() {
    let bank = bank();
    assert_eq!(bank.anims.len(), 1207);
    assert_eq!(bank.by_name("ANIM_TWO_GUN_HOLD"), Some(1));
    assert_eq!(bank.num_frames(1), 163);
    assert_eq!(bank.num_frames(0), 0, "ANIM_IDLE is an empty slot");
    let reload = bank.by_name("ANIM_GUN_FALCON2_RELOAD").unwrap();
    assert_eq!(bank.flags(reload) & ANIMFLAG_LOOP, 0);
}

#[test]
fn frames_clamp_or_wrap_by_the_loop_flag() {
    let bank = bank();
    let looped = (1u16..1207).find(|&n| bank.flags(n) & ANIMFLAG_LOOP != 0 && bank.num_frames(n) > 10).unwrap();
    let once = bank.by_name("ANIM_GUN_FALCON2_RELOAD").unwrap();
    let (nl, no) = (bank.num_frames(looped), bank.num_frames(once));
    assert_eq!(constrain_or_wrap(bank, nl + 3, looped, -1.0), 3);
    assert_eq!(constrain_or_wrap(bank, -2, looped, -1.0), nl - 2);
    assert_eq!(constrain_or_wrap(bank, no + 3, once, -1.0), no - 1);
    assert_eq!(constrain_or_wrap(bank, -2, once, -1.0), 0);
    assert_eq!(constrain_or_wrap(bank, 30, once, 20.5), 21, "endframe clamps, rounded up");
}

#[test]
fn a_non_looping_animation_stops_on_its_last_frame() {
    let bank = bank();
    let reload = bank.by_name("ANIM_GUN_FALCON2_RELOAD").unwrap();
    let mut a = Anim::default();
    let mut ctx = AnimCtx { bank, scale: 1.0, chrinfo: None, merging_enabled: true };
    a.set_animation(&mut ctx, reload, false, 0.0, 1.0, 0.0);
    for _ in 0..200 {
        a.tick(&mut ctx, 1, true);
    }
    assert_eq!(a.frame as i32, bank.num_frames(reload) - 1);
    assert_eq!(a.frac, 0.0);
}

#[test]
fn quarter_ticks_advance_a_quarter_of_a_frame_each() {
    let bank = bank();
    let mut a = Anim::default();
    let mut ctx = AnimCtx { bank, scale: 1.0, chrinfo: None, merging_enabled: true };
    a.set_animation(&mut ctx, 1, false, 0.0, 1.0, 0.0);
    a.tick_quarter(&mut ctx, 6, false);
    assert_eq!((a.framea, a.frameb), (1, 2));
    assert!((a.frac - 0.5).abs() < 1e-6);
}

#[test]
fn a_merge_blends_from_the_old_animation_over_its_time() {
    let bank = bank();
    let mut a = Anim::default();
    let mut ctx = AnimCtx { bank, scale: 1.0, chrinfo: None, merging_enabled: true };
    a.set_animation(&mut ctx, 1, false, 10.0, 1.0, 0.0);
    a.set_animation(&mut ctx, 0x29, false, 0.0, 1.0, 16.0);
    assert_eq!((a.animnum2, a.fracmerge), (1, 1.0));
    a.tick(&mut ctx, 4, false);
    assert!((a.fracmerge - 0.75).abs() < 1e-6, "{}", a.fracmerge);
    a.tick(&mut ctx, 12, false);
    assert_eq!((a.animnum2, a.fracmerge), (0, 0.0), "the merge ends after 16 ticks");
}

#[test]
fn a_chrinfo_root_walks_with_the_clips_root_motion() {
    let bank = bank();
    // ANIM_0029: g_HeadAnims' walk (bondhead.c:14), whose ANIMFIELD_08 root
    // motion is the walk displacement.
    let mut a = Anim::default();
    let mut ci = ChrInfo::default();
    let mut ctx = AnimCtx { bank, scale: 1.0, chrinfo: Some((&mut ci, 0)), merging_enabled: true };
    a.set_animation(&mut ctx, 0x29, false, 0.0, 1.0, 0.0);
    a.set_looping(0.0, 0.0);
    let start = {
        let (c, _) = ctx.chrinfo.as_mut().unwrap();
        update_chr_info(&a, c);
        c.pos
    };
    for _ in 0..60 {
        a.tick(&mut ctx, 1, true);
    }
    let (c, _) = ctx.chrinfo.as_mut().unwrap();
    update_chr_info(&a, c);
    let moved = (c.pos - start).length();
    assert!(moved > 10.0, "a second of walking moved the root {moved:.1} units");
}

#[test]
fn an_absolute_translation_clip_places_the_root_at_stage_scale() {
    let bank = bank();
    let roll = bank.by_name("ANIM_SPECIALDIE_ROLL1").unwrap();
    let ad = bank.get(roll).unwrap();
    assert_ne!(ad.flags & ANIMFLAG_ABSOLUTETRANSLATION, 0);
    let mut a = Anim::default();
    let mut ci = ChrInfo::default();
    let mut ctx = AnimCtx { bank, scale: 1.0, chrinfo: Some((&mut ci, 0)), merging_enabled: true };
    a.set_animation(&mut ctx, roll, false, 5.0, 1.0, 0.0);
    let want = ad.rot_translate_scale(0, 5).1 * STAGE_TRANSLATION;
    let (c, _) = ctx.chrinfo.as_mut().unwrap();
    assert_eq!(c.unk34, want);
    assert!(!c.unk01, "frame 5.0 has no fraction to tween");
    // Ticking walks the absolute positions, never accumulating.
    a.tick(&mut ctx, 3, true);
    let (c, _) = ctx.chrinfo.as_mut().unwrap();
    assert_eq!(c.unk34, ad.rot_translate_scale(0, 8).1 * STAGE_TRANSLATION);
    assert_eq!(c.unk24, ad.rot_translate_scale(0, 9).1 * STAGE_TRANSLATION);
}
