# Architecture

A faithful recreation of Perfect Dark's **Combat Simulator** in Rust. It covers the Perfect Menu, every Combat Simulator setup dialog, the 16 MP arenas, the six scenarios, simulants, and the full MP arsenal, all behaving the way the NTSC-final ROM does. There is no solo campaign, co-op or counter-op. Levels from other games, or any glTF, can be converted into arenas offline (`pd_import`, [D10](#decisions)), and their items, waypoints and doors placed by hand in `pd_edit` ([D11](#decisions)); modelling is done in other tools.

The work so far lives as five spikes in `D:\Claude Code Projects\Hide and Seek Level Builder` (the "old repo"). Each is a function-by-function port of the decomp that has been verified by tests and playtests. This repo does not re-port what those spikes already got right. It **moves that code into a structure it can grow in**, merges the duplicates the spikes accumulated, and cuts the ties to the hide-and-seek game. Their measured findings are kept in [spike-notes/](spike-notes/).

## Principles

1. **PD is the spec.** Code ports decomp functions under PD's names, in PD's units (cm, `lvupdate240` quarter-ticks, BADPI angles), and cites `file.c:line` from `reference/pd-decomp` (NTSC final). If our behaviour and the decomp disagree, the decomp wins, including its bugs.
2. **Substitutions are marked.** Anything that is not PD's behaviour gets a `// SUBST:` comment at the call site saying what PD does and why we don't. `rg "SUBST:"` is the complete list of places where we differ from PD.
3. **The simulation is headless and deterministic.** `pd_core`, `pd_sim` and `pd_menu` never link wgpu, winit, kira or egui. A match is a pure function of its `MatchSetup`, its seed and the per-tick inputs. This is what makes it testable, replayable and debuggable.
4. **One of everything.** One RNG stream, one `Lv`, one `struct anim`, one model walker, one `text.c`, one weapon table, one event type. The spikes had two to four of most of these ([§ Unifications](#unifications)).
5. **The engine knows nothing about the game.** The engine provides the window, the loop, input, GPU plumbing, audio voices and asset discovery. It has no renderer: pipelines and draw order belong to `pd_render`.
6. **Verify without a window.** Every visual feature gets a headless snapshot target and every behaviour gets a test or a probe. Claude never drives the game window; the user playtests.

## Crates

```
pd_game ──► pd_render ──► engine
   │            │  └────► n64 (feature "gpu")
   │            ▼
   ├──────► pd_sim ──┐
   └──────► pd_menu ─┴──► pd_core ──► n64 (CPU only)

pd_tools ──► everything (headless snapshots, probes, debug viewers)
pd_import ──► pd_sim, pd_core (offline: another game's level into stage files)
pd_edit ──► pd_import, pd_render, engine, ... (the level editor, a window)
```

Arrows point at what a crate may use. Nothing points back up.

| Crate | Owns | Must not |
|---|---|---|
| **`engine`** | winit runner and `Game` trait (`init`/`tick`/`frame`/`render`/`debug_ui`), `FrameClock` (fixed tick + alpha + pacing), raw input snapshot (keys, mouse, gilrs pads), GPU context + offscreen targets + low-res present, kira voices + DSP tracks, run-time asset root, egui for dev panels; it re-exports `wgpu` and `egui` so the game uses its versions | know about the N64 or PD; depend on workspace crates; use compile-time asset paths |
| **`n64`** | the console: N64 controller state, RDP (combiner, blender, texture formats, TLUTs, 3-point filter, TRILERP, fill rule, RGBA5551 + dither) as a **CPU reference rasteriser**, RSP semantics (lighting, texgen, matrices), the audio output + TV speaker DSP (pure), the sequenced-audio library as PD links it (`naudio`: Rare's `n_` libultra synth, reverb, compact-MIDI sequence players and the RSP audio ABI they drive, headless), and behind `gpu`, the WGSL ports of the combiner and the VI/CRT chain. Both halves take the same `rdp::DrawState` + `rdp::MipTex` | know about PD; depend on `engine` (the `gpu` half uses wgpu directly) |
| **`pd_core`** | PD maths (BADPI, pdmtx), `random()`, `Lv` timing, PD ids, the animation bank + `struct anim`, the model format + walker + hit test, `text.c` + fonts, language banks, the asset layout, `MatchSetup` (the menu → match handoff), the one `Event` type the menus and the world publish, the save files' bit packing and PD's file system over the Game Pak's EEPROM image (`savebuffer`, `pak`), and the music player (`music`: the tunes, `lib/music.c`'s queue and `snd.c`'s sequence calls over `n64::naudio`; the menus and the world reach it through `Event::Music`) | render on the GPU, do I/O beyond reading the asset files it is pointed at |
| **`pd_sim`** | the world: stage collision (`TileLevel`), pads, nav (PD's routing + our generator), chrs, the player (`bondmove`/`bondwalk`), guns (`gset`, `bondgun`, shots), simulants (`bot`, `botcmd`), props (projectiles, mines, explosions, sentry, N-Bomb, pickups), effect state, match rules (and the match's music calls: its tune, the death tune, switching), events | draw, play sound, read devices |
| **`pd_menu`** | `menu.c`, `menuitem.c`, every Combat Simulator dialog and handler, MP state (presets, locks, challenges), the save files (agent, boss, MP player and setup files, `files`) and the file manager (`filemgr.c`: the agent select, saving and loading), menu graphics and 3D models via the CPU RDP. Output: a framebuffer, sound events, and outcomes such as `StartMatch(MatchSetup)` | depend on `pd_sim` |
| **`pd_render`** | PD on the GPU: BG, every model through the one combiner path, effects, HUD canvas (radar, sights, shields), x-ray, framebuffer post, a `View` per player laid into the frame (`Renderer::render_views`: split screen) | write to the world |
| **`pd_game`** | the `perfect_dark` binary: state machine (Menus → Match → Results), device → N64 controller mapping, event → voice routing, the music's frames onto an engine stream, the Game Pak's EEPROM kept in a file (`save`), presentation settings; its library half, `session`, couples a match with the menus over it (pause, end screens) for the binary and `pd_snapshot` | contain game rules |
| **`pd_tools`** | `pd_snapshot` (offscreen PNGs of menus, guns, stages, matches, pickups), probes, offline audio renders (`pd_music`: a tune, or a menu → match → death → end flow, to a WAV), the bot/nav debug viewer | ship in the game |
| **`pd_edit`** | the level editor: a custom level's layout (items, waypoint edits, doors from the catalogues of PD's and GoldenEye's door models) edited over the stage as the game loads it, in a fly-through view (`pd_render::Renderer::render_free`) with an egui panel; it runs `pd_import` to place the layout and to import a new glTF; `pd_edit --shot` renders its view to a PNG | ship in the game; hold level data of its own (the layout file is `pd_import`'s) |
| **`pd_import`** | the custom level importer: a recipe (`levels/<code>.json`), the source importers (`oot`: Ocarina of Time scenes from the OoT Clone repo's extractor; `ge`: GoldenEye 007 levels, which `tools/ge-extract` converts from the ROM straight into PD's stage formats, rooms, portals, tiles, doors and door models included; `pd`: one of PD's arenas rebuilt in Blender, a glTF laid over the arena room by room, its unchanged rooms' data kept, tiles and portals made for the rest, its setup kept; or several of PD's stages fused, each turned and moved into place) into one `LevelSource`, the collision preprocessing (step risers, ledges as ladders, water), rooms and portals by a k-d split, the four stage files, the gameplay data (`nav::gen`'s waypoints baked in, spawns, weapons and ammo, hills, bases, cover), a check match; headless | ship in the game; be needed at run time |

### Why these boundaries

- **`n64` is separate from `pd_core`** because the RDP, VI and controller are hardware behaviour, not game behaviour. The CPU rasteriser is the oracle that the GPU shaders are tested against.
- **`MatchSetup` lives in `pd_core`** so that the menus and the simulation don't depend on each other. `pd_menu` produces it, `pd_sim` consumes it, and a test can build one by hand.
- **The menus render on the CPU.** PD's menus are 2D plus a few small models at 320×220, and the spike reproduces them pixel-exactly this way. The menu framebuffer has alpha, so the pause menu composites over the game view.
- **The HUD is also CPU-rasterised.** PD's HUD code draws with the same text and RDP primitives as the menus. Its timers tick in the sim; only the drawing lives in `pd_render`.

## A frame

The engine ticks at the PD frame rate chosen in the settings: 60, 30 or 20 Hz. The Combat Simulator's own frame-rate option maps onto this too.

```
engine tick ─► pd_game::controls: devices ─► n64::pad state (+ mouse aim) per player
            ─► state.tick():
                 Menus: pd_menu.frame(lv) with its pads ─► framebuffer, sound events, outcome
                 Match: world.step(inputs)  ─► one Lv (lvupdate240), PD frame order:
                        players: input + bgun_tick_gameplay ─► camera ─► world ticks
                        ─► hands_tick_attack (hitpos) ─► bgun_tick_gameplay2 ─► chrs/bots
                        ─► props ─► match rules ─► events
            ─► pd_game::audio: events ─► engine voices (PD pitch/pan; optional TV chain)
            ─► pd_game::music: Event::Music ─► pd_core::music (the queue) ─► n64::naudio
                 (3 sequence players, 30 voices, reverb) ─► 736-sample frames at 22018 Hz
                 ─► an engine stream kept ~130 ms ahead of the device
            ─► pd_game::save: the menus wrote the EEPROM (paks.take_written) ─► its file
engine frame ─► pd_render: for each player View: BG, chrs, props, fx, gun + hands, HUD,
               post ─► (n64 video/CRT) ─► present; pause menu framebuffer over it
```

Poses are computed in the sim: `pd_core::model` walks each chr's body every tick. The gun position a simulant fires from (`chr_get_gun_pos`) therefore never depends on whether a renderer drew it. In the spike match it did, because the renderer wrote `gunpos_rendered` back into the sim.

## Unifications

What the spikes carry today, and where each piece ends up.

| Concern | In the spikes | Here |
|---|---|---|
| RNG | `pd_spike::pdmath::Rng`, plus a second stream in the gun sim and `pdsim`'s xorshift | `pd_core::rng`, **one stream per world** |
| Frame timing | `pd_guns::bgun::Lv`, `pd_spike::sim::Globals`, `pd_menu` `diffframe60`: three accumulators and three rate enums | `pd_core::lv::Lv` + `engine::clock`. This fixes Combat Boost slowing only the player |
| `struct anim` | `pd_guns::anim` (complete) and `pd_spike::model::Anim` (glTF-driven, no flip or absolute translation, hand tables `anims.rs` + `root_y.rs`) | `pd_core::anim`, from `pd_guns` |
| Model walker | `pd_guns::model` (no helper joints), `pd_menu::pdmodel` (helper joints, own `.pdm` format), engine glTF skinning for bots | `pd_core::model`, from `pdmodel`, **one format** for guns, hands, bodies, heads, props and the hudpiece |
| Simulant bodies | GLB bodies + GLB clips via the old engine, lit flat | PD model files + PD animation bank, drawn by the combiner path like everything else |
| Text | `pd_guns::font` and `pd_menu::text` (with two copies of the fonts) | `pd_core::text`, from `pd_menu` |
| Software raster | `pd_menu::gfx`, `pd_menu::pdmodel::raster`, `pd_guns::font::Canvas`, `app::shade_tri` | `n64::rdp` |
| Weapon table | `pd_guns::gset` (JSON), `pd_spike::weapons` (8 hand-copied), `combat::pd_weapons`, `pd_menu` `MP_WEAPONS` | `pd_sim::gun` gset, from `assets/data/weapons.json`. The menus keep only the setup slot table, keyed by `WEAPON_*` |
| Difficulty / bot type | `pd_spike::bot::Difficulty`, `pdsim` (seconds/radians), menu ints | `pd_core::ids` + `pd_sim::bot` (tick-exact); `pdsim` is dropped |
| Events | `pd_guns::SoundReq`, `pd_spike` footsteps/grunts, `pd_menu` tuples | `pd_core::events::Event`, queued by `pd_menu` and re-exported by `pd_sim::events` (in `pd_core` because `pd_menu` may not depend on `pd_sim`) |
| N64 pad raw codes, SFX manifest loader | copied in `pd_guns/app.rs` and `pd_menu/app.rs` | `pd_game::controls`, `pd_game::audio` |
| Event loops | four `ApplicationHandler`s | `engine::app` |
| Stand-in worlds | `pd_guns::range` boxes, `pd_spike::arena` box room | test fixtures in `pd_sim::stage` (both were already converted to PD polygons) |

## Assets

### Layout

Assets live in `assets/` at the repo root, found at run time (`PD_ASSETS`, then walking up from the executable). The layout is keyed by **PD's own ids**, so any asset can be traced back to the decomp:

```
assets/
  MANIFEST.json              provenance: decomp commit, ROM md5, exporter sha256s, counts, a digest per directory
  textures/<num>.png         the global texture pool, by PD texture number (4 hex digits); BG, guns, chrs, menus and fx share it
  textures/index.json        num -> w, h, format, codec, numcolours, numlods, hasloddata
  models/<stem>.json + .bin  one format for every model file (guns G*, props/held guns P*, chr bodies and heads, hudpiece);
                             the format is documented in pd_core::model
  models/tex/<stem>_<i>.png  textures stored inside a model file (a51guard, dd_shock, elvis, the casings)
  models/index.json          stem -> FILE_* number and name, kind, source, MODEL_* number and g_ModelStates scale (modelnum, statescale)
  anims/<num>.bin + index.json  the whole animation bank, as PD's bit-packed data, by ANIM_* number
  fonts/<name>.bin           handelgothic xs/sm/md/lg, numeric (one copy)
  lang/en.json               every bank, keyed by name with its LANGBANK_* number; lang/mpstringsE.bin
  sfx/<num>.wav + manifest.json   game and menu sounds in one pool, by bank sound number, keyed also by
                             SFXNUM_/SFXMAP_ name (rate, pitch, volume, loop, envelope, chains, provenance)
  data/weapons.json          gset: weapons, funcdefs, gun scripts, aim/recoil/noise, gunviscmds
  data/doors.json            pd_doors.py: every door model PD's setups place (solo stages too; their
                             models are in models/), each with the setup rows it is placed with and
                             their boxes' sizes: the level editor's catalogue
  data/bodies.json           g_HeadsAndBodies (scale, animscale, height, hands), g_MpBodies, g_MpHeads, male/female heads
  data/mpconfigs.bin         challenge and preset configs
  stages/<code>/             per arena, by PD's stage code (Complex = ref), from pd_stage.py:
                             bg.json + bg.bin   the textured BG in the one model format: a DL node per
                                                room and layer, world cm, pool textures, the rooms and
                                                the environment row (z range, sky)
                             tiles.json         collision tiles: room, GEOFLAG bits, floor type, outline
                             pads.json          pads (PADFLAG bits), waypoints, waygroups, cover
                             setup.json         the MP setup's intro[] (spawns, ...) and props[]
                                                (weapon and ammo pads, objects), by macro parameter
  music/                     pd_music.py: bank.json (seq.ctl's instruments, sounds, envelopes, keymaps,
                             ADPCM wavetables), seq.tbl (the sample data, raw), seq/<num>.seq (each
                             compact-MIDI sequence by MUSIC_* number), index.json (names, g_SeqVolumes,
                             the Python reader's counts and decodes as the Rust tests' cross-checks)
```

Generated Rust sits beside the code that uses it: `crates/pd_core/src/ids.rs` (`pd_ids.py`: `WEAPON_*`, `STAGE_*` with stage codes, `BODY_*`/`HEAD_*`, `MP*`, `BOT*`, `HITPART_*`) `crates/pd_menu/src/generated.rs` (`pd_menu_gen.py` `write_rust`: the menu dialogs and item arrays, the MP tables) and `crates/pd_core/src/mpweapons.rs` (`pd_menu_gen.py`: `g_MpWeapons`, which the sim's pickups and the menus share). `build_assets.py` regenerates both.

Nothing reads the decomp or the ROM at run time. In the spikes, the Complex match read `tiles/ref.json`, `pads/ref.json` and `mp_setupref.c` from `reference/pd-decomp` on every launch; the stage exporter (`pd_stage.py`) owns that now.

### Custom levels

Levels converted from other games (`crates/pd_import`, run by hand: `pd_import <code>`) live in `custom/` at the repo root, **gitignored** (the output is the source game's data, made from its owner's copy), laid over `assets/` at run time (`AssetDir::with_custom_levels`: `$PD_CUSTOM`, else `custom/` beside the asset root; a file there wins over `assets/`' at the same path):

```
custom/
  levels.json                the custom arenas: code, stage number (0x60-0x7f, CUSTOM_STAGENUMS), menu name
  stages/<code>/             the four stage files, in pd_stage.py's formats, plus
                             tex/<n>.png  the level's own textures (ids 0x10000 | material in bg.json)
                             report.txt   the import's report
                             ge.json      a GoldenEye level's hand-off from tools/ge-extract to pd_import
  models/<stem>.json + .bin  a level's own models (GoldenEye's doors: all 48 of them once
                             `pd_import --ge-doors` has run), in the one model format
  models/index.json          their rows, beside assets/' index (MODEL_* from CUSTOM_MODELNUMS, 0x1000-0x1fff)
  ge/tex/<nnnn>.png          GoldenEye images by image number (ids 0x10000 | n), shared by GE levels and models
  ge/doors.json              GoldenEye's door catalogue (tools/ge-extract --doors), in data/doors.json's format
  textures/<nnnn>.png        PD pool textures a level draws that assets/ lacks (CI's, for CI Felicity), at assets/' path
  cache/nav/                 the waypoint graphs, by a hash of each stage's geometry (pd_import --place)
  cache/pd/                  PD stages that aren't arenas, exported from the decomp to build a level over
                             (pd_stage.py --base: stages/<code>/ bg + tiles, textures/ + index.json)
```

A custom stage is an arena to everything that loads one: the menus list it in a "Custom" group, `AssetDir::stage_code` resolves its number, `Stage::load` and `StageBg::load` read it like any other, and `ModelStore` takes the custom models' rows beside PD's (never replacing one of PD's).

**GoldenEye levels** (`tools/ge-extract`, run by `pd_import`) need no made-up structure: GoldenEye is PD's ancestor in every part PD's game runs on (rooms and portals, floor tiles, pads, doors, the display-list dialect, the image format), so a GE level is converted through PD's own exporters (`pd_tex`, `pd_fpgun.Interp`, `pd_models.write_model`) and only what an arena needs besides (spawns, weapons, waypoints) is generated. The differences are made up for there, in data: the walls GE implies at unlinked tile edges, the portals moved onto their doors' planes, the blender's `FORCE_BL` rule (`ge_gbi`), PD's own fog rewrite of the rooms' combiners. The porting guide, with every lesson learned, is [GOLDENEYE.md](GOLDENEYE.md).

**PD arenas rebuilt in Blender** (`pd_import`'s `pd` source: the Blender MCP repo's levels, e.g. Very Complex from Complex) are laid over the arena they came from: a room whose triangles are the arena's keeps its batches, tiles and portals; a changed or added room is drawn from the glTF on the arena's own materials, its tiles made from its triangles by the arena's rules and its openings to other rooms found as portals from the open edges; the arena's setup is kept on its pads and only the waypoints are generated. Several of PD's stages can be **fused** the same way (CI Felicity: CI Training and Felicity joined by a hall): each stage is turned and moved to where the glTF has it and numbered after the one before (the recipe's `bases`), the stages are merged into one arena to lay the glTF over (materials, textures, batches, rooms, portals, lights, tiles), and the gameplay data is generated, preferring the stages' MP spots. A stage that isn't an arena is exported from the decomp into `custom/cache/pd/` on the first import, and the textures `assets/` lacks go into `custom/textures/`, their surface types into the stage's own texture entries.
### Pipeline

`tools/pd-assets/` holds the Python exporters from the spikes: stdlib only, except the numpy/Pillow preview tool. They read `reference/pd-decomp` (the decomp's own `tools/extract` must have run once against the ROM; see [reference/README.md](../reference/README.md)). One driver, `build_assets.py`, runs them all into `assets/` and writes `MANIFEST.json`; every path comes from `pd_paths.py`. Individual exporters stay runnable on their own. `check_against_spikes.py` compares `assets/` with the old repo's per-feature exports and names every intended difference.

Generated Rust, such as the menu dialog tables, is written into the crate it belongs to with a "generated by …, do not edit" header, and the generator is rerun to change it.

### What is committed

`assets/` is committed: 28 MB after M1, more as arenas arrive. A clone then builds and runs without the 1.2 GB decomp or the ROM, and every context can run the tests. The exporters must reproduce it bit for bit. If an exporter changes, it is rerun and the diff reviewed. This follows the old repo's practice; see [open questions](#open-questions).

## Verification

| Layer | How |
|---|---|
| Ported functions | Unit tests beside the code, with expected values taken from the decomp or measured in the spikes (their tests move over with the code) |
| Behaviour over time | Deterministic headless probes: seeded matches, walk-every-link, A/B of nav graphs. Long ones are `#[ignore]` and run with `-- --ignored --nocapture` |
| Visuals | `pd_snapshot` renders through the real code paths offscreen and writes PNGs to read. The menus, which are CPU-rendered, get **golden images** captured from the old spikes, so the migration can be pinned pixel for pixel |
| Shaders | naga validation of every `.wgsl` in tests; the CPU RDP as the oracle for the GPU combiner on test scenes |
| Feel | The user's playtests, with a short brief of what changed and what to look at |

## Source map

Where each spike file goes. Paths on the left are under `native/crates/game/src/` in the old repo.

| Old | New |
|---|---|
| `pd_spike/pdmath.rs` | `pd_core::math`, `pd_core::rng` |
| `pd_spike/anims.rs`, `root_y.rs`, `model.rs` | replaced by `pd_core::anim` + `pd_core::model` (bank data instead of hand tables) |
| `pd_spike/level_geom.rs`, `tile_level.rs`, `pd_tiles.rs` | `pd_sim::stage` (`pd_tiles` becomes the stage exporter + a loader for `assets/stages/`) |
| `pd_spike/pd_nav.rs`, `navgen.rs`, `navcheck.rs` | `pd_sim::nav` |
| `pd_spike/chr.rs`, `chraction.rs`, `thirdperson.rs`, `gunpos.rs` | `pd_sim::chr` |
| `pd_spike/bot.rs`, `botcmd.rs` | `pd_sim::bot` |
| `pd_spike/weapons.rs` | deleted; bots use `pd_sim::gun` gset |
| `pd_spike/sim.rs` | `pd_sim::world` |
| `pd_spike/arena.rs`, `waypoints.rs` | `pd_sim::stage` test fixture |
| `pd_spike/walk.rs`, `abtest.rs`, `tests.rs` | `pd_sim::harness` (headless matches, the A/B metrics, not PD) + `pd_sim` tests and probes (`bot::tests`) |
| `pd_spike/viewer.rs`, `camera.rs`, `view.rs`, `debug_draw.rs`, `greybox.rs` | `pd_tools` debug viewer (`pd_lab`: a top-down egui map, `pd_tools::lab`; `pd_snapshot lab` renders the same map) |
| `pd_guns/anim.rs`, `animdata.rs` | `pd_core::anim` |
| `pd_guns/model.rs`, `data.rs` | `pd_core::model`, `pd_core::assets` |
| `pd_guns/pdmtx.rs` | `pd_core::math` |
| `pd_guns/font.rs` | merged into `pd_core::text` + `n64::rdp` |
| `pd_guns/gset.rs`, `bgun.rs`, `bgun_state.rs`, `bgun_pose.rs` | `pd_sim::gun` |
| `pd_guns/player.rs` | `pd_sim::player` |
| `pd_guns/sim.rs` | split: `pd_sim::world` (frame order), `pd_sim::gun` (shots), `pd_sim::fx` (effect state), `pd_render::fx` (effect geometry) |
| `pd_guns/props.rs`, `throw.rs`, `explosions.rs`, `autogun.rs`, `nbomb.rs` | `pd_sim::props` |
| `pd_guns/fx.rs`, `smoke.rs` | `pd_sim::fx` + `pd_render::fx` |
| `pd_guns/xray.rs` | eraser state in `pd_sim`, drawing in `pd_render::xray` |
| `pd_guns/hud.rs` | timers in `pd_sim::gun`, drawing in `pd_render::hud` |
| `pd_guns/render.rs`, `*.wgsl` | `pd_render` + `n64::gpu` |
| `pd_guns/n64video.rs`, `n64video.wgsl`, `crt_screen*.png` | `n64::gpu` |
| `pd_guns/tvaudio.rs` | `n64::audio` |
| `pd_guns/range.rs` | `pd_sim::stage` fixture (the firing range) |
| `pd_guns/app.rs` | `engine::app` + `pd_game::controls`/`audio` + `pd_render::hud` sights |
| `pd_guns/snapshot.rs` | `pd_tools` |
| `pd_menu/menu.rs`, `types.rs`, `handlers.rs`, `generated.rs`, `model.rs` | `pd_menu` (same names); `menuitem.rs` → `pd_menu::item`, `menugfx.rs` → `pd_menu::gfx`, `defs.rs` → `pd_menu::stubs` |
| `pd_menu/mp.rs` | `pd_menu::mpstate` + `pd_core::mp` |
| `pd_menu/text.rs`, `lang.rs` | `pd_core::text`, `pd_core::lang` |
| `pd_menu/gfx.rs` | `n64::rdp` |
| `pd_menu/pdmodel.rs` | `pd_core::model` (walker, format) + `n64::rdp` (raster, combine) |
| `pd_menu/mod.rs` (`Pd`) | split across `pd_menu` (menu, MP and render state) |
| `pd_menu/app.rs`, `snapshot.rs` | `pd_game` (states, controls, audio); `pd_menu::script` (the scripted controller) + `pd_tools` `pd_snapshot menu` |
| `pd_complex/fight.rs` | `pd_sim::world` |
| `pd_complex/health.rs` | `pd_sim::player::health` (the timers, fades, death sequence) + `pd_render::health` (the bar, the fade) |
| `pd_complex/bg.rs` | `pd_render::bg` |
| `pd_complex/app.rs`, `snapshot.rs`, `tests.rs` | `pd_game` match state, `pd_tools`, `pd_sim` tests |
| `pdsim/` | **not carried over** (the hide-and-seek hunter's model, superseded by `pd_spike`) |
| `combat::config::Explosion`, `combat::explosives::pd_falloff_*` | `pd_sim::props` (explosions) |
| `combat::load_gun`/`load_flash`, `weapons/pd/*.glb` | deleted; held guns are PD `P*` model files |
| engine `csg_runtime`, `Region`, `Brush` (range/arena boxes) | deleted; fixtures are PD polygons already |

## Decisions

| # | Decision | Why |
|---|---|---|
| D1 | Migrate the spikes rather than re-port from scratch | They are recent, decomp-cited and verified. The rewrite is structural: boundaries, one-of-everything, headless sim |
| D2 | No renderer in the engine; `pd_render` owns pipelines | The old `Renderer` forced the spikes through `render_with_hook`; PD's look needs the N64 combiner everywhere |
| D3 | Simulants drawn and posed through PD's model files, not glTF | One walker, one lighting model, poses available headlessly; retires `root_y.rs`, `anims.rs` and the GLB exports |
| D4 | CPU RDP is the reference; GPU combiner is tested against it | The menus already need it; it gives the GPU path an oracle |
| D5 | `MatchSetup` in `pd_core` | Menus and sim stay independent and separately testable |
| D6 | Asset exporters stay Python, output committed | They are verified against the decomp's own Python tooling; committing keeps clones runnable without a ROM |
| D7 | The world supports four human players from the start | Retrofitting players into single-player state (`PlayerGun` holds screen/projection) is the expensive way round |
| D8 | PD arenas route on PD's own waypoint graph; our generator (`pd_sim::nav`) is kept, tested and selectable | This is a faithful recreation, so PD's graph is the behaviour. The generator (which passed the Complex A/B) stays because it takes generic geometry and serves any level PD has no graph for |
| D9 | Saves go through PD's own file system (`pak.c`) on the Game Pak's 2 KB EEPROM, kept as a file | Everything around a save stays PD's: the swap files, four of each type, the file manager's dialogs and free spaces, the GUIDs. No Controller Pak is plugged in, a state PD handles. The file is the chip's bytes, so an emulator's 16 Kbit `.eep` loads |
| D10 | Levels from other games are converted **offline** into PD's own stage format (`pd_import`); the run time gains nothing but a second asset root, a menu group and PD's fog | The game stays one path: a custom arena is four stage files like an arena's. Everything such a level lacks (baked lighting, rooms and portals, PD-style collision, waypoints, spawns, pickups, scenario pads) is made ahead of time, and checked by loading the stage as the game does and playing a match on it |
| D11 | A level editor (`pd_edit`) for placing things in imported levels (items, waypoints, doors), not for modelling. Its edits are a committed layout file per level that `pd_import` reads; the editor runs the importer, it is not a second pipeline | Generated placement is a starting point the user wants to fix by hand, and a glTF from any tool needs the gameplay data PD's arenas carry. Keeping the edits beside the recipe means re-exporting the geometry keeps them, and the run time still sees only four stage files |

## Open questions

- **Commit the assets or regenerate them?** The default is commit ([D6](#decisions)). The alternative is to gitignore `assets/` and require the decomp + ROM extraction on each machine.
- **Music's RSP half:** the audio microcode isn't in the decomp; `n64::naudio::abi` follows the PC port's CPU version of its ops (`port/src/mixer.c`). Two of Rare's ops (the per-voice low-pass, "noop") have no documented behaviour; nothing in the Combat Simulator reaches them, so they aren't emulated.
