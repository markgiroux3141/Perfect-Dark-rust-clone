# SPIKE: you against Perfect Dark's simulants, on Complex

Branch `spike/pd-complex` (off main 479899c). Code: `native/crates/game/src/pd_complex/`. Binaries: `pd_complex` (the window) and `pd_complex_snapshot` (offscreen PNGs).

This joins the two earlier spikes into one match. The guns spike (`SPIKE_PD_GUNS.md`) supplies the player: Joanna's guns, movement, HUD and N64/CRT video. The simulant spike (`SPIKE_PD_SIMULANTS.md`, `SPIKE_PD_COMPLEX.md`) supplies the bots. The level is Perfect Dark's Complex, drawn with its own textured geometry.

## Why a third spike

Merging one spike into the other would have tied a large renderer and input stack to a headless bot lab (or the reverse). Instead, both stay runnable on their own (`pd_range`, `pd_arena`), and `pd_complex` owns only the glue a match needs:

- **One world.** Complex's collision tiles drive both the bots' movement and the player's, plus the guns' shots, projectiles and explosions.
- **The player is a chr in the bot match.** Bots see, target, collide with and shoot it.
- **Hits go both ways.** Each side's half of PD's `chr_damage` is applied.
- **Death and respawn** for the player.

The range window became generic over a small `Host` trait (`pd_guns::app::Host`). `pd_range` is the bare `Sim` host; `pd_complex` is the second host. One window implementation serves both: input, N64 pad, audio, N64 video, CRT and panel.

## Run it

```powershell
cd native
cargo build --release
./target/release/pd_complex.exe
```

The environment variables are the simulant spike's. In PowerShell, set each one first, e.g. `$env:PD_BOTS = "6"`.

| var | default | meaning |
|---|---|---|
| `PD_BOTS` | 4 | 1–8 simulants |
| `PD_DIFF` | normal | meat / easy / normal / hard / perfect / dark |
| `PD_WEAPON` | mixed | the simulants' gun: `ar34`, `cmp150`, `falcon2`, `dragon`, `k7avenger`, `dy357magnum`, `magsec4`, `laptopgun` or `unarmed` |
| `PD_NAV` | pd | `pd` (PD's waypoint graph) or `ours` (the generated graph) |
| `PD_SEED` | PD's seed | the match's RNG seed |

The panel (F1) also sets the simulants' count, difficulty and gun, and has "restart match".

**Controls** are the range's:
- movement and view: WASD move, mouse look;
- shooting: LMB fire, RMB (hold) aim;
- R reload, E or MMB (hold) = B, which switches the gun function;
- weapons: wheel or Q cycle, 1–0 pick;
- Ctrl/C crouch down, Space crouch up;
- Esc frees the mouse.

A USB N64 pad plays PD's control style 1.1. After a death, fire respawns once the fade is done (or wait 6 s).

**Loadout:** the player has every gun with unlimited reserve ammo (no pickups yet). Each simulant gets its configured gun on every spawn, as in the simulant spike.

## What is ported (all against `reference/pd-decomp`, NTSC final)

| Area | PD source | Rust |
|---|---|---|
| Textured level: rooms, display lists, the N64 draw state, baked vertex lighting | `bg.c` `bg_reset` / `bg_load_room` / `bg_render_room_pass`, `gfxreplace.c`, `tex.c` | `tools/pd-assets/pd_bg.py` → `native/assets/levels/pd_bg/ref/`; `pd_complex::bg`; drawn by the guns' N64 renderer |
| Player walk: collide-and-slide, stepping, ramps, falling, landing dip, ladders, crouching under ceilings | `bondwalk.c` `bwalk_update_vertical`, `bwalk_resolve_posdelta` + try_delta / quarterdelta / slide_along_edge / slide_along_corner, `bwalk_try_move_upwards`, `bwalk_update_crouch_offset`; `player_get_bbox` | `pd_guns::player` |
| The collision those use | `cd_test_volume_fromdir`, `cd_test_cylmove_oobfail_findclosest_finddist`, `func0f1579cc` / `func0f1578c8` | `pd_spike::tile_level` |
| The player as a chr: eye `prop->pos`, `chr_get_theta`, perimeter | `player.c:5160` / `:5193`, `chraction.c:8883` | `pd_spike::chr` (`Chr::player`), `pd_complex::fight` |
| Bots aiming at a player (eye − 0.4 × eye height) | `chr_calculate_aimend`, `chraction.c:9123` | `pd_spike::chraction` |
| The player's shots on chrs: body-part boxes, first box in tree order, then hit part × damage | `chr_test_hit` (`chr.c:4502`, the multiplayer "cheap" path), `model_test_for_hit` / `model_test_bbox_node_for_hit`, `chr_damage` (`chraction.c:4706`) | `tools/pd-assets/pd_hitbox.py`, `pd_guns::range::test_part_boxes`, `chraction::chr_damage_hitpart` |
| Hit feedback on chrs: prop hit sound, blood and flesh sparks | `chr_hit`, `bgun_play_prop_hit_sound`, `chr_emit_sparks`, `sparks.c` rows 2–4 | `pd_guns::sim`, `pd_guns::fx` |
| The bots' shots, heard and seen: fireslot shot sound and duration, tracers, ricochets, sparks, blood | `chr_shoot` (the half the bot sim leaves out), `chr_update_fireslot` | `pd_complex::fight` |
| Damage to the player: health, shove, grunt, death | `chr_damage`'s player branch (`chraction.c:4752`) | `Fight::player_damage` |
| Damage flash, health bar, death fades | `player_tick_damage_and_health`, `player_display_health` / `_damage`, `player_render_health_bar`, `healthbar_draw`, `player.c:4546` | `pd_complex::health` |
| Respawn | `player_spawn` + `player_choose_spawn_location` (radius 30) | `Fight::spawn_player` |
| Laptop sentry vs chrs: multiplayer round-robin targeting and RCP45 rounds | `autogun_tick` (`propobj.c:8676`), `autogun_tick_shoot` | `pd_guns::autogun` |
| Farsight x-ray of the BG | `bg_render_scene_in_xray` over the BG's own triangles | `pd_guns::xray` |
| Footsteps: bots by animation footfall frames, player every 150 cm, by floor type | `footstep.c`, `bondmove.c:1933` | `pd_spike::chraction`, `pd_guns::sim` |
| RC-P120 cloak vs bot sight | `bot_is_target_invisible`, `canseecloaked`, `targetcloaktimer60`, `zerocloakspeed` | `pd_spike::bot` |
| Hurt grunts by head | `chr_grunt` | `Fight::bot_grunts` |

**Sound pack additions** (merged into `native/assets/audio/pd/sfx/`; no existing entries were changed):
- the hurt voices: male `0x86`–`0x9e`, Jo `0x2aa`–`0x2b3`, female `0x0d`–`0x0f`, Maian `0x5df`–`0x5e1`;
- the stone and metal footsteps, `SFXMAP_80C4`–`80CB` and `80D4`–`80DB`.

## Substitutions (each marked at its call site)

- **BG drawing:**
  - every room is drawn every frame (PD's portals only cull);
  - the base vertex colours are used, which equals room brightness 255 (`room_highlight` is not applied);
  - the ocean texture doesn't animate;
  - translucent BSP blocks draw in export order.
- **Collision:** every polygon is a candidate (PD tests only the rooms the player is in or moves through).
- **Shots against chrs:**
  - the box test is PD's multiplayer path. Single-player PD also refines the hit on the model's triangles;
  - bots' own shots still test the perimeter cylinder (`useperimshoot`), as in PD;
  - the coarse volume is the perimeter's box grown to hold every posed part.
- **Death:** the player has no third-person model, so "the death animation finished" is a fixed 90 ticks.
- **Explosions** are confined to the bounding box of the room at the blast. PD also adds the boxes of the portals it overlaps.
- **Objects** (grenades, mines) collide as a 10 cm cylinder against the wall tiles.

## Measured, and pinned in tests

`cargo test --release -p game --lib pd_complex` runs 10 tests, plus 5 ignored probes.

- **The player walks every link of PD's Complex waypoint graph with the ported walk:** 398 of 401 directed links. This includes the ladder, the crawl space (crouched), every ramp and every ledge drop. The 3 misses are the links the simulant spike found bad for bots too: two climb a sheer 276 cm pit wall that PD's data marks two-way, and one starts on a walkway-edge pad.
- **Ledge drops** fall with PD's gravity, land on the floor below and dip.
- **The Falcon kills a simulant.** A round to the head does 4.0, one to the chest 2.0 and one to the shin 1.0 (PD's hit-part multipliers).
- **A simulant hunts down a player who stands still and kills them.** The player respawns at full health, and the kill and death are scored.
- **A Laptop sentry** finds and shoots a simulant.
- **The RC-P120 cloak** hides the player from a simulant that isn't already tracking them.
- **Footsteps:** simulants and the player make metal footsteps on Complex's floors.
- **The textured BG** has the same bounding box as the collision tiles, to the centimetre, and every texture it uses is present.
- **PD's health bar** sits at the top of the view, green at full health and red at low health.

The whole library suite passes (`cargo test --release -p game --lib`: 844). The range's 58 guns tests pass on the new walk.

## Things the decomp settled that intuition got wrong

- **PD's player walk had to start from a clear spot.** The first run placed the player (radius 30, against a bot's 20) where it already touched two wall tiles, and every climb was refused. PD spawns through `chr_adjust_pos_for_spawn` at radius 30 (`player.c:527`). Both the harness and the match's spawn now do.
- **A standing player can't crouch into the crawl space from inside it.** `bwalk_update_crouch_offset` refuses any height change while the head is in a ceiling, so a start inside the 118 cm tunnel must already be squatting.
- **Bots aim at a player's chest, not the eye.** `prop->pos` is a player's eye, so the unported branch sent every round over the head: about 1,000 misses from 2.4 m.
- **The first box in tree order wins, not the nearest** (`model_test_for_hit`). An armed simulant holds its gun across its chest, and the arms come before the torso, so a chest-high round is an arm hit (×1). Unarmed, the same round is a torso hit (×2).
- **Being hit shoves a bot back ~60 cm** (the ported `shotspeed`). That was worth knowing before misreading hit positions in a probe.
- **Rifle bots walk silently.** The heavy-gun walk and run clips (`ANIM_0030` / `0031`) aren't in `g_FootstepAnims`.

## Visual checks without driving the window

`cargo run --release --bin pd_complex_snapshot -- <outdir>` renders the player's view offscreen through the real GPU path:
- 8 spawn pads;
- firing;
- the damage flash;
- the health bar;
- the Farsight x-ray.

The simulants are drawn by the engine's character path, which needs the window's surface, so they are not in these PNGs. The BG frames were checked against the exporter's independent CPU render: `tools/pd-assets/pd_bg_preview.py` agrees, down to the diagonal baked shadows.

## Known gaps

- **Missing effects:**
  - no room lighting (brightness, flashes on the BG);
  - no blood splats on walls behind a hit chr;
  - no shield/armour pickups.
- **The simulants' guns** are the simulant spike's 8 hitscan weapons; they don't use explosives.
- **Player feedback:** the player's death has no death camera, only the fades; there is no rumble.
- **The simulants are lit flat** (the engine's white ambient).
- **Remaining BG work:** the Complex water animation, and the portal/room culling that a larger stage would need.
