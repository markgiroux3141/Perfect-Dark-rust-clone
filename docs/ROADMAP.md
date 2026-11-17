# Roadmap: Perfect Dark complete, then GoldenEye

The long-term plan beyond the Combat Simulator. Nothing here is started; [MILESTONES.md](MILESTONES.md) is the live plan. Read this when the current milestones run out, or before any work on single player or on a GoldenEye game mode.

Decided with the user on 2026-10-11:

1. **Perfect Dark becomes feature complete:** single player, the Carrington Institute, Co-op and Counter-op, cheats and the credits, on top of the Combat Simulator.
2. **Then a faithful GoldenEye 007,** played by GoldenEye's own rules, not GoldenEye's levels on PD's rules.
3. **The repo becomes a PD / GoldenEye clone.** The player chooses which game to play. GoldenEye reuses what it can from PD; GoldenEye-specific code is fine where GoldenEye behaves differently.

The estimates below were made on 2026-10-11, when the Combat Simulator (M0 to M14) had taken about 5 days. That figure flatters the pace: the old repo's spikes had already proven the menus, the guns and the Complex fight, so those milestones were mostly migration. Neither phase below has spikes. In both phases the user's playtests set the pace more than the code does, because headless tests can't tell whether a mission plays right.

---

## Phase 1: Perfect Dark single player

**Estimate:** 3 to 5 weeks.

### What carries over

The BG rooms, portals, collision, sky, fog and lighting. The guns, projectiles and explosions. Doors, lifts, glass and autoguns. Chr bodies, animation, damage and aim. The menu system, music and the save files. The asset pipeline: solo stages share the arenas' BG format, so their geometry is mostly an exporter run.

### What's new

Sizes are lines in `reference/pd-decomp/src/game/` (NTSC final).

| Area | Decomp | Notes |
|---|---|---|
| AI list interpreter | `chraicommands.c` (9.9k), `gailists.c` (6.2k) | The heart of single player. Guards, cutscene triggers and objectives are AI-list bytecode |
| Per-stage scripts | `src/setups/` (~83k with the MP setups) | Data, not ported by hand: the exporter compiles the setup C into bytecode under `assets/`. Each stage will still exercise commands nothing else has |
| Guard behaviour | `chraction.c` (16k; about 80 citations at M20, from the simulants) | Cover (`setupcover.c`), alerting, hearing, surrender, grenades, fleeing, dodging |
| Solo props | `propobj.c` (20k; partly ported) | Keys and items, security cameras, alarms, consoles, the hoverbike and other vehicles |
| Gadgets | `invitems.c` (6.3k), `bondeyespy.c` | CamSpy, the data uplink, ECM, night vision, the x-ray scanner, the door decoder, disguises |
| Missions | `objectives.c`, `hudmsg.c`, `bondcutscene.c`, `camera.c` | Objectives per difficulty, in-engine cutscenes, subtitles, the intro and outro cameras |
| Set pieces | per stage | Air Force One, the Skedar finale and the other bosses: each a small job of its own |
| Front end and hub | `mainmenu.c` (the solo half), `training.c` + `trainingmenus.c` (6.7k), `cheats.c`, `credits.c`, `title.c` | Mission select, difficulty, the Carrington Institute (firing range, device and holo training) |
| Co-op and Counter-op | | On M12's split screen |
| Audio | | Guard speech is a large new sample set. The sfx pool held 223 sounds at M20 |

### Suggested order

1. **A vertical slice: dataDyne Central, Defection.** It forces the interpreter, guards, objectives, an intro cutscene, mission select and the end screen. How long it takes calibrates the rest of the estimate.
2. **Foundations, about 5 to 8 days:** the interpreter, guard behaviour, objectives, cutscenes and gadgets, finished as the slice needs them.
3. **The missions, about 10 to 15 days:** one a day at first, faster once most commands are ported. Air Force One and the Skedar finale take longer.
4. **The hub, training, Co-op and Counter-op, cheats, credits and polish, about 3 to 5 days.**

### Rules that change when Phase 1 starts

- CLAUDE.md says "There is no solo campaign". Rewrite that line, and the project description, at the first Phase 1 milestone.
- Everything else stands: the decomp is the spec, and every departure gets a `// SUBST:`.

---

## Phase 2: a faithful GoldenEye 007

**Estimate:** 5 to 8 weeks after Phase 1. Most of the guard, script and mission machinery GoldenEye needs will exist by then in PD's form.

### The decomp

**n64decomp/007**, already cloned at `reference/ge-decomp` and cited by the Facility import (see [GOLDENEYE.md](GOLDENEYE.md) § Sources). About 140k lines of C under `src/game/`; one file there still holds inline assembly. It covers NTSC-U, the version of the user's ROM. The setups are generated C (`assets/obseg/setup/`), as PD's are.

PD's engine grew out of GoldenEye's. GE's `chrai.c` (4.5k), `chraction.c` (10.9k), `propobj.c` (14.4k), `bg.c` and `prop.c` are the ancestors of PD's files of the same names, and GOLDENEYE.md records how much survived unchanged: GE's door skeletons are PD's, its sound bank is the same n_audio format, its display lists use the same command words, and PD's door sound table is GE's with one row inserted. Expect the same when porting GE's code: a GE function is usually its PD descendant with fewer features. Diff the two before porting.

### What already exists (M16)

- `tools/ge-extract`: the ROM reader, GE's rzip, textures, BG and clipping into PD's stage formats, fog, prop models (headers scraped from the decomp), every door model, pads and props from the setups (`ge_setup.py`). Facility is imported whole and the user confirmed it in a playtest.
- PD's Classic weapons are GoldenEye's guns (PP9i, CC13, KF7 Special, ZZT, DMC and the rest), already in PD's engine.
- GE's music is n_audio sequences, so `pd_core::music` over `n64::naudio` should play it. Not yet tried.
- `n64` (the RDP, the VI chain, audio, the controller) is console behaviour and serves both games unchanged.

### What's GoldenEye-specific

| Area | GE decomp | Notes |
|---|---|---|
| The player | `bondview2.c` (10.8k), `bondview.c`, `bondwalk2.c` | GE's movement, its aiming (the manual-aim reticle, auto-aim), health and armour |
| Guns | `gun.c`, `gunfire.c` (6.5k) | GE's weapon behaviour and stats. The Classic guns' models carry over, not their rules |
| Guards | `chrai.c`, `chraction.c`, `chr.c`, `chrprop.c` | GE's AI command set and reactions (hit animations by body part, GE's alert rules) |
| Props | `propobj.c` (14.4k) | Including the tank (St. Petersburg), the train, and escorts (Natalya) |
| Missions | `objective*.c` | Objectives per difficulty, 00 Agent, 007 mode's sliders |
| Front end | `front.c` (8.7k), `title*.c`, `options.c` | The folder select, briefings, the watch menu (inventory, objectives, options) |
| Cheats | `cheat.c`, `initcheattext.c` | Including the unlock times |
| Multiplayer | `mpmenu.c`, `mp_weapon.c`, `mp_music.c` | GE's MP modes and its weapon sets |
| Levels | `src/game/bg.c:184` (the level table) | The other 19 solo levels plus Aztec and Egyptian, through `tools/ge-extract`. GE's `stan.c` collision is already converted to PD's tiles at import |
| Chrs and animations | | GE's character models and animation bank, through the model and anim formats PD inherited |

Out of scope: `spectrum.c` (the hidden ZX Spectrum emulator) and the debug menus.

### Architecture (to settle at Phase 2's first milestone)

The direction agreed on 2026-10-11, not yet designed:

- **A game select at boot** chooses PD or GoldenEye. `pd_game` grows a GoldenEye path, or a sibling binary shares a launcher.
- **GoldenEye's rules live in their own crates**, beside PD's and never inside them. Likely a `ge_sim` (the world under GE's rules) and a `ge_menu` (the front end and the watch menu), with the same boundaries as `pd_sim` and `pd_menu`. `pd_sim` stays PD's.
- **What both games share moves down.** Anything genuinely game-neutral in `pd_core` (maths, the model and anim formats, timing, text) may need to become a shared core that `ge_*` can use without pulling in PD's ids and rules. "One of everything" still holds: a GE function that only differs from PD's in data takes the data as a parameter rather than being copied.
- **`pd_render` draws both** if GE's world can be handed to it in the same shape. That is likely, since GE's stages already import into PD's stage formats.
- **GOLDENEYE.md's rule** ("preprocess the level into PD's shape; don't add GE code paths to the game") still governs the custom-level importer. GoldenEye levels played under PD's rules stay as they are (Facility, `pd_edit`'s door catalogue). The faithful mode is the separate, sanctioned place for GE code.
- **Cite GE as `file.c:line`** from `reference/ge-decomp` (NTSC-U, `ge007.u`). Mark departures from GE with `// SUBST:` exactly as for PD.

### Assets

GoldenEye's data comes from the user's ROM at build time and **is never committed**, as `custom/` is today. Whether the GE mode gets its own generated tree (a gitignored `ge_assets/` written by a `tools/ge-extract` build step), and how the game reports a missing one, is decided at Phase 2's first milestone.

### Suggested order

1. **Architecture milestone:** the crate split, the game select, the shared core. No gameplay.
2. **A vertical slice: Dam.** It forces GE's player, guns, guards, objectives, the watch menu, the folder select and the briefing.
3. **The remaining levels,** each with its set pieces (the tank, the train, the escorts).
4. **00 Agent and 007 mode, the cheats and their unlock times, multiplayer.**

---

## Open questions

- Phase 1: compile the AI lists into bytecode under `assets/`, or into Rust tables? The decomp writes them as C macros over bytes, so bytecode is the natural fit.
- Phase 2: one binary with a game select, or two binaries and a launcher?
- Phase 2: where the shared core is split out of `pd_core`, and what it's called.
- Phase 2: GE's save data. GE has its own EEPROM layout, so is it a second Game Pak file?
