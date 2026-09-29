#!/usr/bin/env python3
"""Generate `crates/pd_core/src/ids.rs`: PD's enumerations under PD's names.

From `include/constants.h` (NTSC final): `WEAPON_*` (enum weaponnum and the
defines beside it), `STAGE_*`, `BODY_*`, `HEAD_*`, `MPBODY_*`, `MPHEAD_*`,
`MPSCENARIO_*`, `MPOPTION_*`, `MPFEATURE_*`, `BOTDIFF_*`, `BOTTYPE_*`,
`HITPART_*`, `GEOFLAG_*`, `PADFLAG_*`, `LIFTACTION_*`, `FLOORTYPE_*`, `CROUCHPOS_*`; and from `game/stagetable.c` each stage's code (its BG file,
`FILE_BG_REF_SEG` -> "ref"), which names `assets/stages/<code>/`.

`MUSIC_*` (a sequence's number in `g_SeqTable`) come from the extract's
sequences.json, whose order the decomp's build makes the enum from.

Sound ids (`sfx.h`) are keyed in `assets/sfx/manifest.json` and file numbers
(`files.h`) in `assets/models/index.json`, so neither is generated here.

Usage:
    python tools/pd-assets/pd_ids.py        # build_assets.py runs it too
"""

from __future__ import annotations

import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_weapons  # noqa: E402
from pd_paths import REPO, asset, src  # noqa: E402

OUT = os.path.join(REPO, "crates", "pd_core", "src", "ids.rs")

#: (prefix, Rust type, hex?, doc)
GROUPS = [
    ("WEAPON_", "u8", False, "`enum weaponnum` (`constants.h:4561`) and the `WEAPON_*` defines."),
    ("STAGE_", "u8", True, "Stage numbers (`constants.h`)."),
    ("BODY_", "i32", True, "`g_HeadsAndBodies` rows that are bodies (`constants.h`)."),
    ("HEAD_", "i32", True, "`g_HeadsAndBodies` rows that are heads (`constants.h`)."),
    ("MPBODY_", "u8", True, "`g_MpBodies` indexes."),
    ("MPHEAD_", "u8", True, "`g_MpHeads` indexes."),
    ("MPSCENARIO_", "u8", False, "Scenarios."),
    ("MPOPTION_", "u32", True, "`g_MpSetup.options` flags."),
    ("OPTION_", "u16", True, "`g_PlayerConfigsArray[].options`: a player's own options (`options.c`)."),
    ("MPFEATURE_", "u8", True, "Unlockable features (`g_MpFeaturesUnlocked`)."),
    ("BOTDIFF_", "u8", False, "Simulant difficulties."),
    ("BOTTYPE_", "u8", False, "Simulant personalities."),
    ("HITPART_", "i32", False, "Body parts a shot can hit (`chr_damage`'s multipliers)."),
    ("GEOFLAG_", "u32", True, "Collision tile flags (`constants.h:1189`)."),
    ("PADFLAG_", "u32", True, "Pad flags (`constants.h:3321`)."),
    ("LIFTACTION_", "u8", False, "`chr->liftaction`: a go-to's progress through a lift (`constants.h:1505`)."),
    ("FLOORTYPE_", "u8", False, "Floor materials, for footsteps (`constants.h:961`)."),
    ("CROUCHPOS_", "i32", False, "The player's crouch levels (`currentplayer->crouchpos`)."),
    # The guns (bondgun.c, gset.c, invitems.c) and their effects.
    ("HAND_", "usize", False, "`hands[]` indexes."),
    ("FUNC_", "usize", False, "Weapon functions (`hand->gset.weaponfunc`)."),
    ("FUNCFLAG_", "u32", True, "`funcdef.flags`."),
    ("WEAPONFLAG_", "u32", True, "`weapondef.flags`."),
    ("INVENTORYFUNCTYPE_", "u32", True, "`funcdef.type`: the low byte is the kind, the high byte the shoot subtype."),
    ("AMMOFLAG_", "u32", True, "`inventory_ammo.flags`."),
    ("INVAIMFLAG_", "u32", True, "`invaimsettings.flags`."),
    ("AMMOTYPE_", "i32", False, "`ammotype` (`g_AmmoTypes` rows)."),
    ("GUNFEATURE_", "i32", False, "What a gun script's `ALLOWFEATURE` permits."),
    ("GUNVISOP_", "i32", False, "`gunviscmd.op`."),
    ("GUNAMMOSTATE_", "i32", False, "`bgun_get_ammo_state`'s answers."),
    ("HANDSTATE_", "i32", False, "`hand->state`."),
    ("HANDSTATEMINOR_", "i32", False, "`hand->stateminor`, per state."),
    ("HANDSTATEFLAG_", "u32", True, "`hand->stateflags`."),
    ("HANDMODE_", "u32", False, "`hand->mode`."),
    ("HANDANIMMODE_", "i32", False, "`hand->animmode`."),
    ("HANDATTACKTYPE_", "i32", False, "`hand->attacktype`."),
    ("EJECTSTATE_", "i32", False, "`hand->ejectstate`."),
    ("EJECTTYPE_", "i32", False, "`hand->ejecttype`."),
    ("USETIMER_", "i32", False, "`bgun_consider_toggle_gun_function`'s answers."),
    ("SIGHT_", "i32", False, "`gset_get_sight`'s sights."),
    ("MODELPART_", "i32", True, "Model part numbers (`model_get_part`)."),
    ("SPARKTYPE_", "usize", True, "`g_SparkTypes` rows."),
    ("SMOKETYPE_", "usize", False, "`g_SmokeTypes` rows."),
    ("SHARDTYPE_", "u8", False, "`shard->type` (`shards_create`)."),
    ("EXPLOSIONTYPE_", "usize", False, "`g_ExplosionTypes` rows."),
    ("WALLHITTEX_", "usize", True, "`g_WallhitTexes` rows."),
    # The objects the guns put in the world (propobj.c, projectile.c).
    ("PROJECTILEFLAG_", "u32", True, "`projectile->flags`."),
    ("OBJHFLAG_", "u32", True, "`obj->hidden`."),
    ("OBJFLAG_", "u32", True, "`obj->flags`."),
    ("OBJFLAG2_", "u32", True, "`obj->flags2`."),
    ("OBJFLAG3_", "u32", True, "`obj->flags3`."),
    ("OBJTYPE_", "u8", True, "`obj->type`."),
    ("CDTYPE_", "u32", True, "What a collision test considers."),
    ("DEVICE_", "u32", True, "`player->devicesactive`."),
    ("VISIONMODE_", "i32", False, "`player->visionmode`."),
    ("CAMERAMODE_", "i32", False, "`player->cameramode`."),
    ("MISCSFX_", "usize", False, "`g_MiscSfxSounds` rows (`lv.c:175`)."),
    # The match (mplayer.c, mpstats.c, hudmsg.c).
    ("MPPAUSEMODE_", "u8", False, "`g_MpSetup.paused`."),
    ("SHOTREGION_", "usize", False, "`playerstats.shotcount[]` indexes (`mpstats.c`)."),
    ("AWARD_", "u32", True, "`mp_calculate_awards`' award bits (`g_AwardNames` order)."),
    ("MEDAL_", "u8", True, "`mpplayerconfig.medals`."),
    ("MPPLAYERTITLE_", "u8", False, "`mpplayerconfig.title`."),
    ("MPDISPLAYOPTION_", "u8", True, "`mpchrconfig.displayoptions`."),
    ("HUDMSGTYPE_", "usize", False, "`g_HudmsgTypes` rows."),
    ("HUDMSGFLAG_", "u32", True, "`hudmessage.flags`."),
    ("HUDMSGSTATE_", "u8", False, "`hudmessage.state`."),
    ("HUDMSGALIGN_", "u8", False, "`hudmessage.alignh` / `alignv`."),
    # Pickups and inventories (inv.c, botinv.c, propobj.c, prop.c).
    ("OBJH2FLAG_", "u32", True, "`obj->hidden2`."),
    ("INVITEMTYPE_", "i32", False, "`invitem.type`."),
    ("NUM_CYCLEABLE_WEAPONS", "u8", False, "The weapons `inv_choose_cycle_*_weapon` can land on."),
    ("TICKOP_", "i32", False, "What a prop's tick asks `prop_execute_tick_operation` to do."),
    ("DROPTYPE_", "u8", False, "`projectile->droptype`: how `obj_drop` throws a dropped object."),
    ("BOTFLAG_", "u32", True, "`aibot->flags`."),
    ("MA_AIBOT", "i32", False, "A simulant's `chr->myaction`."),
    # The scenarios (mplayer/scenarios.c, bot.c, botroom.c).
    ("AIBOTCMD_", "u8", False, "`aibot->command`: a simulant's orders (a human's, or the scenario's own)."),
    ("COVERFLAG_", "u16", True, "`cover.flags`."),
    ("MODEL_", "i32", True, "`g_ModelStates` rows (`models/index.json` maps each to its file)."),
    # Presentation (player.c's viewports, sight.c).
    ("SCREENSPLIT_", "u8", False, "`g_ScreenSplit`: two players' viewports (`options_get_screen_split`)."),
    ("SIGHTTRACKTYPE_", "u8", False, "`invaimsettings.tracktype`: what a sight tracks (`sight_tick`)."),
    # Music (lib/music.c, game/music.c, lib/snd.c).
    ("TRACKTYPE_", "i32", False, "`g_SeqChannels[].tracktype`: what a sequence player is playing."),
    ("MUSICEVENTTYPE_", "i32", False, "`g_MusicEventQueue[].eventtype`."),
    ("SOUNDMODE_", "i32", False, "`g_SoundMode`: the Sound option (`snd_set_sound_mode`)."),
    ("SPEAKERMODE_", "i32", False, "`alSurround_OutputType`'s modes."),
]


def stage_codes(values: dict[str, int]) -> list[tuple[str, int, str]]:
    """(STAGE_ name, number, code) for every `g_Stages` row (`stagetable.c`)."""
    text = open(src("game", "stagetable.c"), encoding="utf-8").read()
    out = []
    for m in re.finditer(r"/\*0x[0-9a-f]+\*/\s*(STAGE_\w+)\s*,[^,]*,[^,]*,[^,]*,[^,]*,[^,]*,\s*FILE_BG_(\w+?)_SEG", text):
        name, code = m.group(1), m.group(2).lower()
        out.append((name, values[name], code))
    return out


def main() -> int:
    pd_weapons.require_decomp()
    header = src("include", "constants.h")
    values = pd_weapons.scrape_defines(header, tuple(p for p, *_ in GROUPS))
    values.update(pd_weapons.scrape_enums(header, ("weaponnum",)))
    lines = [
        "//! GENERATED by tools/pd-assets/pd_ids.py from reference/pd-decomp (NTSC final). Do not edit:",
        "//! change the generator and rerun it.",
        "//!",
        "//! PD's enumerations under PD's names, so a value can be looked up in the decomp",
        "//! directly. Sound ids are keyed in `assets/sfx/manifest.json` and FILE_* numbers in",
        "//! `assets/models/index.json`.",
        "#![allow(dead_code)]",
        "",
    ]
    total = 0
    for prefix, ty, hexa, doc in GROUPS:
        names = sorted((n for n in values if n.startswith(prefix)), key=lambda n: (values[n], n))
        lines.append(f"// {doc}")
        for n in names:
            v = values[n]
            if ty.startswith("u") and v < 0:
                continue
            lit = (f"{v:#x}" if v >= 0 else f"-{-v:#x}") if hexa else str(v)
            lines.append(f"pub const {n}: {ty} = {lit};")
            total += 1
        lines.append("")
    seqs = json.load(open(asset("sequences.json"), encoding="utf-8"))
    lines.append("// `MUSIC_*`: sequence numbers (`g_SeqTable` order, the extract's sequences.json).")
    for num, e in enumerate(seqs):
        lines.append(f"pub const {e['id']}: i32 = {num};")
        total += 1
    lines.append("")
    stages = stage_codes(values)
    lines.append("/// Each `g_Stages` row's stage and code (its BG file, `stagetable.c`): the")
    lines.append("/// name of `assets/stages/<code>/`.")
    lines.append("pub const STAGE_CODES: &[(u8, &str)] = &[")
    for name, v, code in stages:
        lines.append(f"    ({name}, \"{code}\"),")
    lines.append("];")
    lines.append("")
    lines.append("/// A stage's code (`STAGE_MP_COMPLEX` -> `\"ref\"`).")
    lines.append("pub fn stage_code(stage: u8) -> Option<&'static str> {")
    lines.append("    STAGE_CODES.iter().find(|(s, _)| *s == stage).map(|(_, c)| *c)")
    lines.append("}")
    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(lines) + "\n")
    print(f"pd_ids: {total} constants, {len(stages)} stage codes -> {os.path.relpath(OUT, REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
