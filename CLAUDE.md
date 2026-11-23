# Perfect Dark: Combat Simulator in Rust

A faithful recreation of Perfect Dark's Combat Simulator (NTSC final): the menus, the MP arenas, the scenarios, simulants and the full MP arsenal. There is no solo campaign. Custom levels (other games' levels, any glTF) are imported offline and edited in `pd_edit` (items, waypoints, doors; no modelling).

**Read first:** [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (crates, boundaries, asset layout, where old code goes), then [docs/MILESTONES.md](docs/MILESTONES.md) (status and the current milestone's plan). The spikes' measured findings are in [docs/spike-notes/](docs/spike-notes/). Check there before assuming how PD behaves.

## Build, test, run

```
cargo build --release                       # the game: target/release/perfect_dark.exe (--combat, --fresh, --unlock-all,
                                            #   --play <code>: a match on that arena at once, alone, the default setup;
                                            #   the Game Pak's EEPROM is save/perfect_dark.eep, or $PD_SAVE)
cargo test --workspace --release            # all tests (incl. the menu goldens, crates/pd_menu/tests/golden/)
cargo test -p pd_sim --release -- --ignored --nocapture   # long probes
cargo run --release -p pd_tools --bin pd_snapshot -- <outdir> <what> ...   # offscreen PNGs (e.g. `out model dark_combat --gun chrfalcon2`,
                                            #   `out menu --combat w40 down down down a w50 shot:setup` (script format in pd_menu::script;
                                            #   `--fresh --boot` starts at power on's agent select on a blank Game Pak),
                                            #   `out stage ref` (the 8 spawn-pad views, on the GPU; `--full` adds objects, sky, HUD;
                                            #   `--at x,y,z,theta` one view from the F1 panel's "P1 feet (...) θ"),
                                            #   `out match --duel`, `out lab` (a match with simulants; the pd_lab map), `out pickups [--bots]`,
                                            #   `out scenario htb` (htm, pac, koh, ctc: a scenario's props, HUD, highlights, room tints),
                                            #   `out flow start w30 shot:pause kill wend shot:end` (the pause menu and
                                            #   end screens over a match; steps in pd_tools::snapshot::flow),
                                            #   `out flow --teams --mates 2 hold:a:50 shot:am` (the active menu; `hold:a+z:1` next screen),
                                            #   `out flow --players 4 [--vsplit] [--shield] w60 shot:split` (split screen),
                                            #   `out flow --challenge 1 w40 win wend shot:verdict` (a challenge, won),
                                            #   `out match --duel --lock --dist 900` (the rocket launcher's lock),
                                            #   `out flow --stage kokiri --mates 3 start w300 shot:m` (a match on any arena, custom too))
cargo run --release -p pd_import --bin pd_import -- kokiri   # convert a custom level (crates/pd_import/levels/<code>.json) into custom/
                                            #   (gitignored); `--check <code>`, `--explain <code> x z`, `--probe <code> x0 z0 x1 z1`;
                                            #   `facility` is GoldenEye's, from the GE ROM via tools/ge-extract (Python, ~1 min)
                                            #   `verycomplex` is Complex rebuilt in Blender (a glTF from the Blender MCP repo, ~11 s)
                                            #   `cifelicity` is CI Training + Felicity fused in Blender (CI from the decomp; ~3 min, the waypoints)
                                            #   `--ge-doors`: every GoldenEye door model + the editor's catalogue of them (custom/ge/doors.json)
                                            #   `catacombs` is a Jedi Academy map (M20): the .bsp + the game's base/ (pk3s), in extra maps/ (gitignored)
                                            #   `--ge64 <code> <dir>`: a level as the GoldenEye Setup Editor's level files (OBJ rooms, portals, clipping)
                                            #   `--jka <dir> [--clone]`: every Jedi Academy MP map in extra maps/base's pk3s: Setup Editor folders (+ arenas)
cargo run --release -p pd_import --bin pd_import -- --place <code>   # place a level again with its layout
                                            #   (levels/<code>.layout.json; the cached waypoints); `--adopt <code>` writes one
target/release/pd_edit.exe [<code>]         # the level editor (a window: for the user; "Generate…" hosts the level generator's editor,
                                            #   with "Plan | 3D" over it: the level in 3D as it is shaped, and "Walk" to walk it;
                                            #   "Make it an arena" imports what it makes); `--shot-generated <out.png> [--seed N | --recipe FILE]
                                            #   [--view NAME | --room KEY | --at x,y,z,yaw,pitch] [--walk NAME [--forward TICKS] [--crouch]] [--plan] [--list]`
                                            #   is that 3D view with no window (a preview in a scratch folder, no arena; ~1.7 s); `--shot <code> <out.png>
                                            #   [--at x,y,z,yaw,pitch | --item weapon 3 | --item door 3] [--waypoints] [--open]`
                                            #   is its view as a PNG (`--open`: every door opened two seconds before)
                                            #   `--shot-ui <out.png> [<code>] [--step home|layout|items|test] [--select weapon 2] [--generate | --seed N]
                                            #   [--room KEY] [--side] [--run-test] [--import FILE.glb] [--tool KIND]`: the whole window (panels and view) with no window: how to
                                            #   look at the editor's screens (`--room vast`: a generated room selected on Layout; `--run-test`: the simulant test's numbers)
cargo build --release -p pd_import          # also builds target/release/autolevel.exe, the level generator (docs/AUTOLEVEL.md):
                                            #   `autolevel build 40` plans, builds, bakes and walks seed 40 into out/autolevel/seed40
                                            #   (`--style felicity`: a Felicity-style level instead, with doors; docs/AUTOLEVEL.md, "Felicity")
                                            #   (GLB, GE64 OBJ, plan.svg, report.json; ~0.6 s, the walks in-process on pd_sim);
                                            #   `autolevel import out/autolevel/seed40` makes it the arena `autolevel40`:
                                            #   the generator's rooms and doorways as PD's rooms and portals, on the pool's textures,
                                            #   its recipe kept in levels/autolevel40.json (`--gltf`: as any glTF instead);
                                            #   `autolevel shots out/autolevel/seed40` then draws its views with the game (DIR/shots/*.png);
                                            #   `autolevel trek 1`: an atrium 15 storeys tall whose base is reached only
                                            #   through a helix of rooms round it; `--scale towering` (any command): halls up
                                            #   to 15 storeys whose high bases are reached only from outside (docs/AUTOLEVEL.md,
                                            #   "Atriums"; `cargo run --release -p autolevel --example climbs -- 1 40` lists the climbs);
                                            #   `--platforms N`: big platforms off the catwalks into a hall's empty air;
                                            #   `--galleries N`: high catwalks of their own in towering halls, climbed to
                                            #   from outside; `--holes P`: drops through platforms (holes, unrailed spans)
                                            #   and between rooms; `--cellars P`: a room under a hall, a hatch down, a
                                            #   ladder up; `--angled P`: angled catwalks, an island reached only by them
                                            #   and shortcuts across a hall's air; `--flooded P` (0.1 by default): a room's
                                            #   floor water under railed walkways; `--shaped P` (0.1): rooms an L or a U;
                                            #   `--sunken P` (0.1): a pit a cell deep in a hall's floor, a ramp down into it;
                                            #   bases (on by default; `--no-bases`): a base through a hall's wall, floor or
                                            #   ceiling, slit windows into it at eye height, entered from outside (by hand: an
                                            #   `add` of kind `base`); big pods (on; `--no-big-pods`): pods grown to 5-10 x 4-8
                                            #   (docs/AUTOLEVEL.md, "Platforms", "Galleries", "Drops", "Cellars",
                                            #   "Angled catwalks", "Flooded rooms", "Shaped rooms", "Sunken floors", "Bases")
                                            #   a room no ramps can join is rescued with ladders and bridges, so every seed makes a
                                            #   level (`--no-rescue`: it fails as it did; docs/AUTOLEVEL.md, "Rescue")
                                            #   also `sweep`, `plan`, `batch`, `walk`, `deadends`, `--recipe FILE`; `autolevel --help`
cargo run --release -p autolevel_edit -- --seed 40 --shot out.png [--empty] [--storey Z] [--pick ROOM@STOREY [--join N]] [--remove KEY] [--side x@Y|y@ROOM]
                                            #   the generator's editor: its plan view as a PNG with no window (how to look at a
                                            #   level's plan), the dead-end list, the joins that could be made; without --shot a window (the user's).
                                            #   `--empty`: no rooms but those --add makes (a blank level; docs/AUTOLEVEL.md, "Blank levels").
                                            #   Room boxes: `--rooms --select ROOM --lots --move ROOM@X,Y --rect ROOM@X0,Y0,X1,Y1 --pin ROOM@FLOOR
                                            #   --delete ROOM --add KIND@X0,Y0,X1,Y1@FLOOR@TOP --height ROOM@FLOOR@TOP --try` (a refusal is drawn and
                                            #   exits 1). `--side x@Y | y@X | x@ROOM`: the snapshot is the side view (a section, height up). One room's
                                            #   choices: `--layout --cover --inside --brightness --ceiling --own ROOM@VALUE`, `--feature ROOM@NAME@on|off|auto`
                                            #   (shaped flooded sunken cellar platforms holes angled), `--choices ROOM`
cargo test --release -p autolevel -p autolevel_build -p autolevel_edit   # the generator against its answer keys (~60 s); these three
                                            #   crates alone are run through rustfmt (`cargo fmt -p ...`, their own rustfmt.toml)
cargo run --release -p pd_tools --bin pd_music -- <out.wav> <tune|--flow> [secs]   # offline music renders (MUSIC_* name or number; --list)
cargo run --release -p pd_tools --bin pd_lab # the simulant/route viewer (a window: for the user, not for Claude to drive)
python tools/check_boundaries.py            # headless crates stay headless
python tools/pd-assets/build_assets.py      # regenerate assets/ (and crates/pd_core/src/ids.rs) from the decomp
python tools/pd-assets/check_against_spikes.py   # assets/ against the old repo's exports
```

- The first build is slow (~2.5 min). Incremental builds are seconds. The linker is `rust-lld` (`.cargo/config.toml`).
- If a release build says "Access is denied", the game window is still running and locking the exe. Ask the user to close it.
- Hand off playtests with `cargo build --release`, plus a short brief of what changed and what to look at.

## Rules

- **The decomp is the spec.** Port PD functions under PD's names and units, and cite `file.c:line` from `reference/pd-decomp` (NTSC final; every `#if VERSION >= VERSION_PAL_BETA` takes the `#else`). Keep PD's bugs (`M_BADPI`, off-by-ones) unless the user says otherwise.
- **Mark every departure** from PD with `// SUBST: <what PD does> / <what we do and why>` at the call site.
- **Respect the crate boundaries** in ARCHITECTURE.md. `n64` (CPU), `pd_core`, `pd_sim` and `pd_menu` never depend on wgpu, winit, kira, gilrs, egui or `engine`. `pd_render` never writes to the world. `engine` never mentions PD or the N64.
- **One of everything.** Before adding a helper (angle maths, RNG, timing, text, model posing), look in `pd_core`/`n64`. Duplicates are what this repo exists to remove.
- **Never drive the game window.** Check visuals with `pd_snapshot` PNGs (read them), behaviour with tests and probes. The user playtests.
- **Assets are generated.** Don't hand-edit anything under `assets/` or a generated `.rs` file (they say so in their header). Change the exporter in `tools/pd-assets/` and rerun it. Nothing reads the decomp or the ROM at run time.
- **Tests travel with ported code.** When migrating a spike module, bring its tests. If a pinned value changes, the commit message says why.
- **Milestone discipline:** work inside the current milestone. At the end of a context, update the Status table and the milestone's notes in MILESTONES.md so the next context can continue cold.

## Where things are

- `crates/`: the thirteen crates (see ARCHITECTURE.md § Crates); `pd_import` is the custom level importer (offline, M15), `pd_edit` the level editor (M18).
- `crates/autolevel`, `autolevel_build`, `autolevel_edit`: the level generator (Complex-style arenas from a seed) and its editor, moved here from the Blender MCP repo (M21). **Read [docs/AUTOLEVEL.md](docs/AUTOLEVEL.md) before touching them**: a seed's level must never change by accident (the answer keys in `tests/key/` are written by that repo's `scripts/autolevel_key.py` from its frozen Python), and they are not PD ports, so the decomp rules don't apply to them.
- `custom/`: converted levels (gitignored; another game's data), laid over `assets/` at run time (ARCHITECTURE.md § Custom levels).
- `crates/pd_import/sets/`: the arena sets (committed): which custom arenas the game lists. One is active per machine (`custom/arena_set.json`); `custom/levels.json` is written from it (`pd_import::sets`), and a finished import joins it. Pick, make, rename sets and delete arenas on `pd_edit`'s Home.
- `assets/`: generated, committed; layout in ARCHITECTURE.md § Assets.
- `tools/pd-assets/`: Python exporters (stdlib only; `pd_bg_preview.py` needs numpy + Pillow).
- `tools/ge-extract/`: the GoldenEye level extractor (stdlib only, reuses `tools/pd-assets`): a GE level from the ROM into PD's stage formats, run by `pd_import` (M16). **Read [docs/GOLDENEYE.md](docs/GOLDENEYE.md) before porting another GE level**: the format mapping and every lesson Facility cost.
- `reference/`: gitignored. `pd-decomp` and `pd-pcport` are junctions to the old repo's clones, and `pd_bot_port_sheet.md` holds the verbatim C for the bot port. See [reference/README.md](reference/README.md).
- Old repo (the migration source): `D:\Claude Code Projects\Hide and Seek Level Builder`, branch `main` (it has merged every spike: `spike/pd-combat-sim-menu` and the Complex fight), code under `native/crates/game/src/pd_{menu,guns,spike,complex}/`. Read it; don't modify it. To run one of its snapshot tools, build it with `CARGO_TARGET_DIR` in your scratchpad (see MILESTONES.md, M3 notes).
- ROM (only for the decomp's one-time `tools/extract`): `D:\GoldenPerfectModding\Perfect Dark (U) (V1.1) USE THIS ONE\`. The `[!]` in the file name breaks PowerShell wildcards, so use `-LiteralPath`.
