# Perfect Dark weapon mechanics — where the authored data actually lives

Companion to [DESIGN_PD_SIMULANT_AI.md](DESIGN_PD_SIMULANT_AI.md), which covers the bot brain.
This one covers everything **between the decision to shoot and the damage landing**: where the
gun sits in the hand, when in the animation it fires, where the muzzle flash goes, and how much
damage it does.

The short version: **every one of these is authored data, not code constants, and almost all of
it is already sitting in the assets we extracted.** None of it has to be guessed.

Line references are `reference/pd-decomp/src/` (gitignored — see `reference/README.md`).

---

## 1. The answer table

| Mechanic | Where it lives | Already extracted? |
|---|---|---|
| Gun position in the hand | `POSITIONHELD` node inside the **chr-gun** model + the chr's `RIGHTHAND`/`LEFTHAND` part | **Yes** |
| Which bone the gun hangs off | `MODELPART_CHR_RIGHTHAND` (3) / `LEFTHAND` (5) in the chr's parts array | **Yes** |
| When in the animation it fires | `attackanimconfig.shootstartframe` / `shootendframe` | No — it's a **code table**, `chraction.c:912+` |
| When the arm can start tracking | `attackanimconfig.aimstartframe` / `aimendframe` | No — same table |
| Recoil frames | `attackanimconfig.recoilstartframe` / `recoilendframe` | No — same table |
| Aim cone limits + lean | `attackanimconfig.maxup/maxdown/maxleft/maxright` | No — same table |
| Free-arm follow | `attackanimconfig.freearmfracup` / `freearmfracdown` | No — same table |
| Muzzle-flash position, size, texture | `CHRGUNFIRE` node (part `0x0000`) in the **chr-gun** model | **Yes** |
| Barrel position for the shot ray | Same `CHRGUNFIRE` node — `chr_get_gun_pos`, `chraction.c:9640` | **Yes** |
| Damage / spread / penetration | `funcdef_shoot*` in `invitems.c` | No — code table |
| Rate of fire | `funcdef_shootauto.initialrpm` / `maxrpm`, plus `weapon_get_num_ticks_per_shot` | No — code table |
| Recoil kick (viewmodel) | `funcdef_shoot.recoildist` / `recoilangle` / `slidemax` | No — code table |
| Clip size + reload | `struct ammodef` | No — code table |
| Player viewmodel muzzle / grip | `MODELPART_GUN_MUZZLEPOS` (0x32) / `HOLDPOS` (0x37) | **Yes** |
| Player viewmodel animation | `struct guncmd` scripts in `invitems.c` | No — code table |

"Code table" is not a problem — it means transcribing a few hundred literals out of
`invitems.c` and `chraction.c`, which is mechanical and diffable. The asset-side items are the
ones that would have been genuinely hard, and those are the ones we can already read.

## 2. Gun placement — there is no offset to tune

This is the one that most directly replaces hand-tuning, and it is simpler than expected.

`propobj.c:17400`:

```c
weapon->base.model->attachedtomodel = chr->model;
weapon->base.model->attachedtonode = model_get_part(chr->model->definition,
                                                    MODELPART_CHR_RIGHTHAND);
```

and then at render time, `chr.c:1983`:

```c
Mtxf *mtx0 = model_find_node_mtx(model->attachedtomodel, model->attachedtonode, 0);
...
renderdata.rendermtx = mtx0;     // that's it — the hand matrix IS the gun's root
```

So the gun's root transform is *literally* the hand bone's matrix. **No per-weapon offset, no
per-character offset, no tuning.** Any fine positioning is baked into the weapon model itself,
via its `POSITIONHELD` node (`model_update_position_held_node_mtx`, `lib/model.c:1191`), which
applies a translation between the hand matrix and the gun's geometry.

Exactly three special cases exist in the whole game:

```c
if (embedded)                     rendermtx = mtx0 * embedment->matrix;
else if (race == RACE_SKEDAR)     rendermtx = mtx0 * rotY(75.6°) * rotZ(90°);  // odd hand rig
else if (hand == HAND_LEFT)       rendermtx = mtx0 * rotZ(180°);               // flip the model
else                              rendermtx = mtx0;
```

Verified against our extraction — every character exposes the attach parts, and the
weapons carry their grip offsets:

```
a51guard.bin    parts: 0, 1, 2, 3(RIGHTHAND), 5(LEFTHAND), 6(HEAD/HAT)
dd_guard.bin    parts: 0, 1, 2, 3(RIGHTHAND), 4, 5(LEFTHAND), 6(HEAD/HAT)
ak47.bin        POSITIONHELD pos = (-23.89, 51.58, 142.62), mtx 3   → (-0.025, 0.055, 0.151) scaled
```

## 3. Two separate weapon models per gun

Worth knowing before ripping anything, because it explains why some models look wrong for a
given job:

- **`files/guns/*.bin`** — the **player's first-person** model. Rich part set:
  `MUZZLEPOS`, `HOLDPOS`, `SLIDE`, `LASERSIGHT`, `CARTEJECTPOS`, `MUZZLEFLASH1..3`, cartridge
  flaps. There are also `hand_<character>.bin` models — per-character first-person hands.
- **`files/props/chr*.bin`** (61 of them) — the **third-person** model an enemy holds. Much
  smaller part set, and it carries the thing we need: part `0x0000` = the `CHRGUNFIRE` node.
- **`*lod.bin`** — reduced-detail variants.

Measured from our extraction:

```
guns/falcon2.bin      0x32=MUZZLEPOS, 0x33=SLIDE, 0x34=LASERSIGHT, 0x37=HOLDPOS,
                      0x3c=CARTEJECTPOS, 0x5a=MUZZLEFLASH1, ...
props/chrar34.bin     0x00=GUNFIRE, 0x01=POSITIONHELD, 0x02=TOGGLE
```

## 4. Muzzle flash — position, size and texture are authored

`struct modelrodata_chrgunfire` (`types.h:502`) is `{ pos, dim, texture, ... }`: a world
placement, a size in three axes, and the texture to draw. Read straight off the chr-gun model,
e.g.:

```
chrar34.bin        flash pos = (-414.5, 0.0, 6.5)   dim = (306.5, 148.0, 151.0)
chravenger.bin     flash pos = (-406.5, 1.0, 33.0)  dim = (179.5, 138.0, 142.0)
chrautogun.bin     flash pos = (-190.5, -2.0, 23.0) dim = (262.5, 126.0, 122.0)
```

The same node doubles as the **barrel origin for the shot ray** (`chr_get_gun_pos`,
`chraction.c:9640`) — flash and bullet come from the identical authored point, which is why
they always agree on screen. A handful of weapons use `STARGUNFIRE` (0x16) instead — the
star-shaped flash on the AR34, Cyclone, Dragon, Avenger.

`FUNCFLAG_NOMUZZLEFLASH` on a `funcdef` suppresses it (silenced Falcon 2, pistol whip).

## 5. When it fires — `attackanimconfig`

The whole per-animation timing model, with the original author's comment
(`types.h:333`):

```
start <= aimstart <= shootstart <= recoilstart <= recoilend <= shootend <= aimend <= end
```

| Field | Meaning |
|---|---|
| `animnum` | Which animation this row configures |
| `startframe` / `endframe` | Trim — start later or finish earlier than the raw clip (`endframe` may be −1) |
| `aimstartframe` / `aimendframe` | When the chr may swivel its aim. **Earlier than shooting** — it can track while still raising the arm |
| `shootstartframe` / `shootendframe` | It fires during these frames, given line of sight |
| `recoilstartframe` / `recoilendframe` | Recoil frames, for single-shot pistols; −1 if the clip has none |
| `maxup` / `maxdown` | Beyond these aim angles the body leans back / forward |
| `maxleft` / `maxright` | Horizontal aim limits (right is negative) |
| `freearmfracup` / `freearmfracdown` | The gunless arm moves by this fraction of the gun arm |
| `turnangleperframe`, `angleoffset` | Turn rate, and how far the "zero" X angle sits off the chr's facing |

Real rows (`chraction.c:980`), fields in animation frames:

```c
// animnum,  unk, turn, angoff, start, end, shootstart, shootend, recoilstart, recoilend, aimstart, aimend, maxup, maxdown, maxleft, maxright, freeup, freedown
{ ANIM_0041, 26, 0, DTOR(0), 12, 140, 58, 92, 60, 79, 20, 120, BADDTOR(50), BADDTOR(-40), BADDTOR(40), BADDTOR(-40), 0, 0 },
{ ANIM_0044, 0,  0, DTOR(0), 17, 100, 25, 87, 30, 55, 20,  93, BADDTOR(50), BADDTOR(-40), BADDTOR(40), BADDTOR(-60), 0, 0 },
{ ANIM_0046, 22, 0, DTOR(0), 4,   69, 22, 49, 22, 33,  8,  58, BADDTOR(50), BADDTOR(-40), BADDTOR(25), BADDTOR(-45), 0, 0 },
```

Read the first row: the clip is trimmed to frames 12–140, the chr starts tracking its aim at
frame 20, fires between 58 and 92, recoils 60–79, and keeps tracking until 120.

Configs are grouped (`attackanimgroup`) and selected by **race × situation** — e.g.
`g_StandHeavyAttackAnims[RACE_HUMAN | RACE_SKEDAR][32]`. So the timing is per animation, per
race, per stance. This is the direct, authoritative replacement for our hand-set `FIRE_TIMING`
windows in `combat/config.rs`.

`chr_calculate_aimend` (`chraction.c:9071`) consumes this each frame to drive the shoulder /
back / lean offsets — the same job our `AimOffsetLayer` does.

## 6. Rate of fire, and the tick floor

`chr_shoot` (`chraction.c:9916`) has a detail worth copying, in its own comment:

> Most guns can fire at most once every few ticks — even automatics.

`weapon_get_num_ticks_per_shot(weaponnum, weaponfunc)` returns a floor; `chr->firecount[hand]`
accumulates and a shot goes out when it crosses. Notably `makebeam` alternates via
`chr->unk32c_12 ^= 1 << handnum` — **only every other shot draws a tracer**.

## 7. Damage and the rest of the weapon stats

`struct funcdef_shoot` (`types.h:2919`) with the data in `invitems.c` (6,294 lines, one block
per weapon). A complete example — the Falcon 2 (`invitems.c:485`):

```c
struct funcdef_shootsingle invfunc_falcon2_singleshot = {
    INVENTORYFUNCTYPE_SHOOT_SINGLE,
    L_GUN_085,                    // name
    0, 0,                         // unused, ammoindex
    &invnoisesettings_default,
    invanim_falcon2_shoot,        // fire animation script
    0,                            // flags
    &invrecoilsettings_default,
    16,                           // recoverytime60
    1,                            // damage
    1,                            // spread
    3, 5, 2, 0,                   // recoil animation speed bytes
    10,                           // recoildist
    15,                           // recoilangle
    59.999996,                    // slidemax
    0,                            // impactforce
    0,                            // duration60
    SFXMAP_804D,                  // shootsound
    1,                            // penetration
};
```

Automatics add `initialrpm` / `maxrpm` (a spin-up), projectiles add speed / decel / travel
distance / lifetime. Melee and throw have their own `funcdef` variants.

## 8. The player viewmodel is a scripted animation

`struct guncmd` (`types.h:5267`) is a small bytecode, one script per weapon action —
`fire_animation`, `reload_animation`, `equip_animation`, `unequip_animation`,
`pritosec_animation`. Commands (`include/gunscript.h`):

```
playanimation(anim, direction, speed)   showpart(keyframe, part)   hidepart(keyframe, part)
playsound(keyframe, sound)              allowfeature(keyframe, feature)
waitforzreleased(keyframe)              repeatuntilfull(keyframe, gotokeyframe)
random(probability, address)            setsoundspeed(keyframe, speed)   include / end
```

Each command is keyed to a **keyframe**, so sound, part visibility and "the player may act
again now" are all authored against the animation rather than timed in code. Example:

```c
struct guncmd invanim_falcon2_shoot[] = {
    gunscript_playanimation(ANIM_GUN_FALCON2_SHOOT, 0, 10000)
    gunscript_allowfeature(9, GUNFEATURE_CLICK)   // at keyframe 9, allow the next click
    gunscript_end
};
```

This is the system that replaces hand-positioning the player's weapon: the viewmodel pose comes
from a real animation, and everything else hangs off its keyframes.

## 9. The former gate: animations — now proven

This section previously called the animation layer the one real blocker, on the theory that
chr models could not be posed until a bind pose was found. **That was wrong, and it is
resolved.** There is no separate bind pose: the node tree is the skeleton, and the animation
supplies the rotations, so frame 0 of any clip is a valid rest pose.

Proven end to end with [tools/pd-assets/pd_anim.py](tools/pd-assets/pd_anim.py) and
[tools/pd-assets/pd_pose.py](tools/pd-assets/pd_pose.py) — see START_HERE.md §2 for the format
details and the units-per-metre calibration. Summary of the evidence:

- Every animation header parses to exactly the declared file size.
- 65 characters carry the shared **15-bone rig**, matching the 15 animated parts in the clips.
- Posed output is anatomically correct: head on top, symmetric shoulders, feet level in a
  standing clip and staggered mid-stride in a walk.
- Heights land where the body table says they should — 1.65 m for a male guard, 1.61 m for
  Joanna, 1.02 m for Maians.

`MODELPART_CHR_RIGHTHAND` also resolves cleanly to a real joint on every body checked, so the
weapon-attach chain (§2) is de-risked along with it.

## 10. Suggested order

1. ~~**Animation format + bind pose.**~~ **Done** — see §9.
2. **Chr model + skeleton import**, reusing the existing skinned-mesh path. This is now the
   next real step: the geometry, skeleton and animation all decode, so the work is converting
   them into whatever the engine's `SkinnedModel` / `AnimationClip` want.
3. **Weapon attach** — hand part + `POSITIONHELD`. Nearly free once 2 lands, and it deletes the
   hand-tuned offsets.
4. **Muzzle flash + barrel origin** from `CHRGUNFIRE`. Also nearly free, and it makes flash and
   bullet agree by construction.
5. ~~**`attackanimconfig` transcription**~~ **Done** — `game/src/combat/attack_anim.rs` carries
   the three rows our fire clips need (`ANIM_0032` / `ANIM_0041` / `ANIM_007A`), resolved per
   hunter at spawn into a `FireTiming`. PD hunters use it; GoldenEye hunters keep the
   `FIRE_TIMING` guess, so the two are an A/B on *identical animations*. What it changed:
   the pistol's shoot window went from a 3-frame sliver (frames 63–66) to the authored 58–92,
   turning a one-round burst into three; every row brought an **aim window wider than the
   shoot window**, so a hunter swivels onto you before it can fire and keeps tracking after
   the last round; and the authored `maxup`/`maxdown`/`maxleft`/`maxright` replaced our single
   80° cone with ±40° sideways and +50°/−40° vertical, so the body turns where the chest used
   to twist. Bursts are now fewer and longer (the row's `endframe` holds the action open),
   which is the cadence change to judge in playtest.
6. **Weapon stats** from `invitems.c` — damage, spread, RPM, penetration, clip sizes.
7. **Player viewmodel** — `guncmd` scripts + first-person models. Largest and most independent;
   sensible to do last. Note `g_HeadsAndBodies[].handfilenum` gives the per-character
   first-person hand model.

Steps 5 and 6 are transcription and could be done at any point if a worked-out weapon table is
wanted early.
