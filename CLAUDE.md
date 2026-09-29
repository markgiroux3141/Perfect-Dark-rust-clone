# Perfect Dark: Combat Simulator in Rust

A faithful recreation of Perfect Dark's Combat Simulator (NTSC final): the menus, the MP arenas, the scenarios, simulants and the full MP arsenal. There is no solo campaign and no level editor.

**Read first:** [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (crates, boundaries, asset layout, where old code goes), then [docs/MILESTONES.md](docs/MILESTONES.md) (status and the current milestone's plan). The spikes' measured findings are in [docs/spike-notes/](docs/spike-notes/). Check there before assuming how PD behaves.

## Build, test, run

```
cargo build --release                       # the game: target/release/perfect_dark.exe (--combat, --fresh, --unlock-all;
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

- `crates/`: the nine crates (see ARCHITECTURE.md § Crates); `pd_import` is the custom level importer (offline, M15).
- `custom/`: converted levels (gitignored; another game's data), laid over `assets/` at run time (ARCHITECTURE.md § Custom levels).
- `assets/`: generated, committed; layout in ARCHITECTURE.md § Assets.
- `tools/pd-assets/`: Python exporters (stdlib only; `pd_bg_preview.py` needs numpy + Pillow).
- `tools/ge-extract/`: the GoldenEye level extractor (stdlib only, reuses `tools/pd-assets`): a GE level from the ROM into PD's stage formats, run by `pd_import` (M16). **Read [docs/GOLDENEYE.md](docs/GOLDENEYE.md) before porting another GE level**: the format mapping and every lesson Facility cost.
- `reference/`: gitignored. `pd-decomp` and `pd-pcport` are junctions to the old repo's clones, and `pd_bot_port_sheet.md` holds the verbatim C for the bot port. See [reference/README.md](reference/README.md).
- Old repo (the migration source): `D:\Claude Code Projects\Hide and Seek Level Builder`, branch `main` (it has merged every spike: `spike/pd-combat-sim-menu` and the Complex fight), code under `native/crates/game/src/pd_{menu,guns,spike,complex}/`. Read it; don't modify it. To run one of its snapshot tools, build it with `CARGO_TARGET_DIR` in your scratchpad (see MILESTONES.md, M3 notes).
- ROM (only for the decomp's one-time `tools/extract`): `D:\GoldenPerfectModding\Perfect Dark (U) (V1.1) USE THIS ONE\`. The `[!]` in the file name breaks PowerShell wildcards, so use `-LiteralPath`.
