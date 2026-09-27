# Milestones

Each milestone is sized to fit one Claude context and ends in something checkable: tests, snapshots or a playtest. Start a new context by reading [../CLAUDE.md](../CLAUDE.md), [ARCHITECTURE.md](ARCHITECTURE.md), then this file's **Status** and the milestone's own section. End a context by updating **Status**, ticking what was done, and noting anything the next context must know under the milestone.

"Old repo" = `D:\Claude Code Projects\Hide and Seek Level Builder`, branch `spike/pd-combat-sim-menu` (its working tree also holds the untracked `pd_complex` spike). Old code paths are under `native/crates/game/src/`.

## Status

| # | Milestone | State |
|---|---|---|
| M0 | Architecture + skeleton | **done** (2026-09-27) |
| M1 | Foundations: `pd_core` + `n64` CPU + asset pipeline | next |
| M2 | Engine runner + the menus boot | — |
| M3 | Stages + walking Complex | — |
| M4 | Guns (hitscan, HUD, effects) | — |
| M5 | Guns (projectiles, explosives, specials, N64 video, TV audio) | — |
| M6 | Simulants: a Combat match on Complex | — |
| M7 | Match flow: limits, pause, results, teams, options | — |
| M8 | Pickups, weapon sets, simulants seeking weapons | — |
| M9 | All arenas | — |
| M10 | The other five scenarios | — |
| M11 | Simulant types, difficulties, commands | — |
| M12 | Split-screen, radar, music, challenges, profile | — |

The first playable path is M2 → M3 → M4 → M6: menus, walking, shooting, then a real match. M5 moves ahead of M6 if the user wants the full arsenal before simulants.

---

## M0: Architecture + skeleton (done)

- [x] Survey the spikes, the old engine and the asset pipeline
- [x] `docs/ARCHITECTURE.md`: crates, boundaries, unifications, asset layout, source map, decisions
- [x] Cargo workspace: `engine`, `n64`, `pd_core`, `pd_sim`, `pd_menu`, `pd_render`, `pd_game`, `pd_tools`; module files carry their contract and their spike sources; the workspace builds
- [x] `CLAUDE.md` conventions
- [x] Spike notes copied to `docs/spike-notes/`; `reference/pd_bot_port_sheet.md` copied (gitignored: it quotes the decomp)
- [x] `reference/pd-decomp` and `reference/pd-pcport` are directory junctions to the old repo's clones (no 2.3 GB copy)
- [x] Asset exporters copied to `tools/pd-assets/` verbatim. **They still write to the old layout** (`<repo>/native/assets`); M1 repoints them.

---

## M1: Foundations

**Goal:** everything that is shared lands once, with its tests, and `assets/` is generated in the new layout.

**Asset pipeline**
1. `tools/pd-assets/pd_paths.py`: one module for the decomp root (`PD_DECOMP_DIR`, else `reference/pd-decomp`) and the output root (`assets/`). Repoint every exporter at it.
2. **Texture pool:** export every texture the game uses to `assets/textures/<num>.png` + `index.json`, replacing the per-feature copies (`pd_menu/textures`, `pd_menu/models/tex`, `weapons/pd_fp/textures`, `weapons/pd_fp/fx`, `levels/pd_bg/ref/textures`). Keep mip metadata.
3. **One model format.** Read both writers (`pd_fpgun.py` `export_model` → JSON; `pd_menu_models.py` → `.pdm`) and both readers (`pd_guns/data.rs` + `model.rs`; `pd_menu/pdmodel.rs`). Define a single `models/<file>.json + .bin` that carries the node tree (every node type, including `0x0100`/`0x0200` helpers and BBOX part boxes, so `pd_hitbox.py` merges in), draws with N64 draw state, and materials by texture number. Export guns (G*), held guns and props (P*), the six+ chr bodies and heads, and the hudpiece. Record the format in `pd_core::model` docs.
4. **Animation bank:** `assets/anims/<num>.bin` + `index.json` for every animation the game references (gun scripts, menu `ANIM_01FC`/`040D`, the bot rows including squat/duck `0280`-`0287`, deaths, hits). Bots no longer need the `bot_anims/*.glb` clips.
5. Fonts (one copy), `lang/`, `data/weapons.json`, `data/mpconfigs.bin`, and `sfx/` (merge `sfx` and `menu_sfx`, keyed by sfx id; include the 56 WAVs untracked in the old repo).
6. `build_assets.py` runs all of the above and writes `MANIFEST.json`. Diff the new textures, animations and sfx against the old repo's files (the same bytes, moved) as the regression check.

**Code**
7. `pd_core::math` + `rng` (from `pd_spike/pdmath.rs`, `pd_guns/pdmtx.rs`, `pd_atan2f`); `lv` (merge the three timing structs: remainder carry from `pd_spike`, slow-motion cap from `pd_guns`, `diffframe60` from `pd_menu`).
8. `pd_core::anim` from `pd_guns/anim.rs` + `animdata.rs`, with their tests.
9. `pd_core::model`: the loader for the new format and the walker from `pd_menu/pdmodel.rs` (helper joints, head-on-body matrix segment), plus `model_test_for_hit` from `pd_guns/range.rs` `test_part_boxes`. Tests: posing is identical to the old walkers on guns (no helpers) and bodies (helpers). Port the spike's "feet on the floor within ±4 cm" check onto the new path.
10. `n64::rdp`: merge `pd_menu/gfx.rs` + `pdmodel.rs` `raster`/`combine`. `n64::pad`: `Joy` + raw codes. `pd_core::text` from `pd_menu/text.rs` (the fonts through `n64::rdp`).
11. `pd_core::ids`, `lang`, `assets` (the layout builder), `mp::MatchSetup`.
12. `pd_snapshot model <name>`: a CPU turntable of any model (body + head + held gun) to PNG, for checking the format by eye.

**Done when:** `cargo test --workspace` is green with the ported anim/model/text tests; `build_assets.py` regenerates `assets/` reproducibly; `pd_snapshot model` renders Joanna, a simulant body with head, and a Falcon correctly.

**Watch out for** (from the spike notes):
- a head has no matrices of its own; it loads matrix 0 from the body's segment;
- every chr body uses the elbow/knee helpers, and the gun walker skipped them;
- the menu model context turns fog off, so ignore the exporter's `fog_tint` there;
- colour maths is gamma-free: textures are raw `Rgba8Unorm` in N64 display space.

---

## M2: Engine runner + the menus boot

**Goal:** `cargo run --release` opens a window on PD's Perfect Menu, with the Combat Simulator dialogs, sound and N64 pad, and "Start" hands back a real `MatchSetup`.

- `engine`: `app` runner + `Game` trait, `clock` (with alpha), `input`, `gpu` (context, offscreen target, present with letterbox), `audio` (voices, DSP tracks), `assets` (run-time root), `debug_ui`. Sources: the old engine's `FrameClock`, `Gamepads`, `AudioManager`, and the `Renderer` setup code only.
- `pd_menu`: migrate `pd_menu/` (split `Pd`), pointed at the new assets and `pd_core`/`n64`. `start_match` returns `StartMatch(MatchSetup)`.
- `pd_game`: the Menus state, controls (keyboard per the spike's key table, N64 pad raw codes), audio routing.
- `pd_snapshot menu <script>`: the old `pd_combat_sim_snapshot` script format.
- **Golden images:** before migrating, run the old repo's `pd_combat_sim_snapshot` over a fixed set of scripts (main menu, combat sim, char select, arena list, weapons, simulants, name keyboard) into `crates/pd_menu/tests/golden/`. The new snapshot must match pixel for pixel. Any intentional difference gets a written reason.

**Done when:** goldens match, and the user playtests the menus in the window.

---

## M3: Stages + walking Complex

**Goal:** from the menus, "Start" on Complex drops the player into the textured level, walking with PD's movement. No guns yet.

- Stage exporter: `pd_bg.py` + a new tiles/pads/setup exporter → `assets/stages/ref/`. Trim the tiles JSON to what `LevelGeom` needs; parse `mp_setupref.c` spawns and weapon pads into `setup.json`.
- `pd_sim::stage`: `level_geom`, `tile_level`, and the stage loader (was `pd_tiles`); the arena and range fixtures.
- `pd_sim::player`: `bondmove`/`bondwalk`/`bondhead` from `pd_guns/player.rs`, with `PlayerInput` per player and control style 1.1.
- `n64::gpu` combiner pipeline + `pd_render::{bg, view}`; the Match state in `pd_game`.
- `pd_sim::world` skeleton: one `Lv`, one `Rng`, players, the event queue, PD frame order.
- Tests: the player walks 398/401 of PD's Complex waypoint links (the spike's pinned result, same 3 known bad links). Snapshots: 8 spawn-pad views, checked against the old `pd_complex_snapshot` frames.

---

## M4: Guns (hitscan, HUD, effects)

`pd_sim::gun` (gset, bgun hand state, pose, shots), `pd_sim::fx`, `pd_render::{models, fx, hud}`: every hitscan and melee weapon, single and dual, secondary functions, reload, zoom and the sniper scope, tracers, sparks, bullet holes, casings, smoke, the gun HUD, and the default and Maian sights. The firing-range fixture comes back as a test stage. Source: `pd_guns`. Tests: the spike's 37 gun tests. Snapshots: `pd_snapshot guns --all` against the old `pd_gun_snapshot --all` frames.

## M5: Guns (projectiles, explosives, specials)

`pd_sim::props`: throwables, mines, rockets, the Slayer, Phoenix, crossbow, N-Bomb, the Laptop sentry and explosions; the Farsight x-ray, Combat Boost (now slowing the whole world) and the cloak. Post effects, `n64::gpu` video/CRT, and `n64::audio` TV speaker as presentation options. Snapshots: `--seq all`.

## M6: Simulants, a Combat match on Complex

`pd_sim::{chr, bot, nav}` from `pd_spike` + `pd_complex/fight.rs`. Bodies and held guns go through the PD model path (M1), posed in the sim. Damage both ways with hit parts, death and respawn, health bar and damage flash, footsteps and grunts. The menu's simulants (count, difficulty, body) and weapon set drive the match. `pd_tools` gets the bot/nav debug viewer (`pd_lab`). Tests: the spike's bot tests, the Complex A/B baseline (kills/min, first contact, zero stalls), and a seeded match that is reproducible bit for bit.

## M7: Match flow

Time and score limits, the pause menu over the game, end of match with PD's results and stats dialogs, back to the menus; teams and team scores; MP options (one-hit kills, slow motion, fast movement, no radar yet, and so on). `pd_sim::mp`.

## M8: Pickups and weapon sets

Weapon and ammo pads from the setup, respawn timers, shields, weapon sets from the menu, simulants spawning unarmed and routing to pickups (`botinv`).

## M9: All arenas

Generalise the stage exporter; portal and room culling, room lighting and flashes, animated textures, doors, lifts and escalators, `GEOFLAG_DIE`/`STEP`/`SLOPE`/`RAMPWALL`. The 16 MP arenas: Skedar, Pipes, Ravine, G5 Building, Sewers, Warehouse, Grid, Ruins, Area 52, Base, Fortress, Villa, Car Park, Temple, Complex and Felicity.

## M10: Scenarios

Hold the Briefcase, Hacker Central, Pop a Cap, King of the Hill and Capture the Case, with their options, HUD and simulant behaviour.

## M11: Simulant types, difficulties, commands

The `BOTTYPE_*` personalities (Peace, Shield, Rocket, Kaze, Fist, Prey, Coward, Judge, Feud, Speed, Turtle, Venge), difficulties through DarkSim, and simulant commands from the pause menu.

## M12: Presentation

Split-screen for 2–4 players (one `View` each), the radar, music (PD's sequencer + soundbank), challenges, the save profile, the death camera, and rumble.
