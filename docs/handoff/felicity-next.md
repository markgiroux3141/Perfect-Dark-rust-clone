# Handoff: Felicity levels, everything still planned

Written 2026-11-17 at the end of a session; the next one starts cold. Branch `autolevel-kits`.
Read `CLAUDE.md` (loaded), then `docs/AUTOLEVEL.md`'s "Felicity" section, all of it (families,
room types, stairs, walkways, upstairs areas, arches and the hall's bays, circulation targets,
stalls, ledge falls, "Not yet"), then the files below. The previous brief and what came of it:
`docs/handoff/felicity-variation.md` (all four items done).

## Where things are

- `crates/autolevel/src/felicity.rs`: the planner. `generate` → `layout` (draft: `sprawl` or
  `block`; turn; doorways `gap`; stairs `stair_ways`/`stair_fits`; second ways; types; the hall's
  `screens`; `balconies`) → `level` (doors from `<seed>/felicity/doors`, arches, plan, walks, views).
  Streams: `<seed>/felicity` (layout), `/doors`, `/balconies`. `upper()` is the only rule that joins
  two storeys (a landing onto a tall room's walkway); `could()` never finds such pairs.
- `crates/autolevel/src/balcony.rs` (walkways: `fit`, `HEAD`), `stairs.rs` (flights; `Frame`'s
  `point`/`local`/`way`/`span` are the shared frame maths), `level.rs` (`Level`, `Doorway`, `Arch`
  and its `curve`), `circulation.rs` (the targets).
- `crates/autolevel_build/src/felicity.rs`: the kit (roles, `TYPES`, the style over Complex's
  carver, `Zone::base` for strips above walkways, `clear_under` + `arch` for arches).
- `crates/pd_import/src/autonav.rs`: `doorways` (waypoints out of doorways' mouths, two on each
  axis) then `repair`. `crates/pd_import/src/autolevel/tests.rs`: `probe_stalls` (stalls and falls,
  `PD_PROBE_SECS`, works on PD's arenas too).
- Showcase arena: **seed 15, code `felicity15`, stage 127** (`crates/pd_import/levels/felicity15.json`,
  built in `out/autolevel/felicity15`). `felicity10` is still on disk on the same stage number, so
  it's out of the menu; the user hasn't asked for it to be deleted.

## The work, in this order (the user: "take these all on in whatever order is best")

1. **Layout gaps.** Each a planner change with walks and a test:
   - Upper rooms beside a tall room open onto its walkway as a second way in (today only a
     landing's doorway does). The walkway's wall is then fixed by its doorways: `balconies`
     already refuses ways round that don't hold every doorway a storey up, and fails the layout
     (it redraws) if none fits; keep that rule, mind the redraw rate.
   - Walkways in tall plain rooms (`Kind::Room`, 15% of big rooms off circulation, two storeys).
     Their door-to-door walks are straight lines, which a flight on the floor would block: route
     those walks round it, or keep the flight against a wall clear of every line between doorways.
   - The block family: a second upstairs area (a landing off the hall's walkway at least; the
     block has no open rooms).
   - The hall not always closing one end of the building (both families put it at an end).
2. **Felicity's doorway trims**: its doorways have small 45-degree chamfered frames (13 units, the
   00EF faces round openings: `PD_PROBE_STAGE=mp11 PD_PROBE_OUT=tris.json cargo test -p pd_import
   --release probe_stage_triangles -- --ignored`, rooms 24 and 29 have them; a script that groups
   slanted faces by room and normal is the quickest look). A new kit piece like `arch`: faces
   added round an opening, the carver's left alone or trimmed.
3. **Windows and light strips** (in "Not yet" since the first version). Look at Felicity's own
   first (its tinted glass: 9 `tinted_glass` props; its lights).
4. **An outdoor part** (Felicity has one; ours are all indoors). Ask the user how much before
   building: it touches the sky, the bake and the rooms/portals.
5. **The editor for Felicity levels** (a Felicity recipe takes no edits; `pd_edit --shot-generated`
   draws Complex's pipeline only). The largest; read AUTOLEVEL.md's "Track B: the editor" first.

Done this session, not to redo: walkways, second gallery and landings, arches and the hall's bays,
doorway waypoints; the remaining stalls at doors are PD's own door logic (the user: leave it);
"ledge falls" are simulants running down stairs, as in PD's Felicity (nothing to fix).

## Rules that must hold (the acceptance for every step)

- **Complex unchanged**: `cargo test --release -p autolevel -p autolevel_build -p autolevel_edit
  -p pd_import` passes, and `target/release/autolevel.exe build 40` is byte for byte what it was:
  build it to a scratch folder before you start, `diff -r` after every step.
- **Felicity**: `cargo test -p autolevel --release` (includes the circulation targets on seeds
  1-60); every walk passes on seeds 1-40 (`cargo build --release -p pd_import`, then for each seed
  `target/release/autolevel.exe build N --style felicity --out <dir> --no-bake`, last line "X of X
  walks as expected"; a loop over 1-40 takes about 30 s). New features get walks and tests.
- The three random streams stay separate; the doorway contract (a box through a 40 wall on a grid
  line, a half each side, 106 or 212 wide for doors, 212 tall, one doorway per pair of rooms);
  `Plan::check` passing.
- rustfmt on `autolevel`, `autolevel_build`, `autolevel_edit` only. `--release` always.
- **Never run `pd_edit` or the game without shot arguments.** Look with `target/release/pd_edit.exe
  --shot <code> out.png --at X,Y,Z,yaw,pitch --solid` (PD axes: x, height, -plan y; yaw 0 faces
  plan south, 90 east; from above: Y 5000, pitch -89). Simulants: `PD_PROBE_SECS=300
  PD_PROBE_STAGE=<code> cargo test -p pd_import --release probe_stalls -- --ignored --nocapture`.
- Importing: `target/release/autolevel.exe import <dir> --code felicity15 --name "Felicity 15"
  --stagenum 127` (reuse that code and number). Seed 15's layout moves whenever the layout stream's
  draws change; that's fine, re-import it.
- Commit per step on `autolevel-kits` (end the message with the attribution line the system gives
  you); update the "Felicity" section of `docs/AUTOLEVEL.md` as you go. Ask the user for design
  decisions instead of guessing (they answer quickly; last session they chose every recommended
  option). Hand off playtests with `cargo build --release` and what to look at.
- Tooling: the Bash tool mangles heredocs whose text holds apostrophes: write edit scripts with the
  Write tool into the scratchpad and run them (`open(p, newline='')` to keep LF).
