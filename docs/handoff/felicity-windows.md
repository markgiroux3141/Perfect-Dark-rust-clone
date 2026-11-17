# Handoff: Felicity levels, windows, a courtyard and the editor

**Superseded by `docs/handoff/felicity-detail.md`** (2026-11-17: the user dropped the courtyard and
put the editor aside for Felicity's detail). The "Done last session" list below still holds.

Written 2026-11-17 at the end of a session; the next one starts cold. Branch `autolevel-kits`.
Read `CLAUDE.md` (loaded), then `docs/AUTOLEVEL.md`'s "Felicity" section, all of it, then the files
below. The previous brief and what came of it: `docs/handoff/felicity-next.md` (items 1 and 2 done,
this session; 3, 4 and 5 are this brief's).

## Done last session (not to redo)

- **Item 1, the layout gaps** (four commits): upper rooms beside a tall room open onto its walkway
  (`onto_walkways`; `walkway` is the fitting `balconies` uses, tried with nothing drawn); landings
  leave a free end, a landing's stair room may open at its foot into the tall room (`Draft::stairs`'
  fourth field), the galleries' rooms are hung after the landings, rooms off upper corridors are
  shortened rather than dropped, rooms are planned along tall rooms (`ALONG`); walkways in tall
  plain rooms clear of the door-to-door lines (`LINE`, `crosses`); the block family's landing off
  the hall's walkway (`landing`, `upper_rooms` are functions taking a `Fits` rule and a reach;
  `BLOCK_LANDING`); built round the hall (`Lot::far`, `shared` refuses the far wall and the side
  walls beside the platform, `usable`, `ROUND`; the sprawl's round corridor, the block's
  `corridor3`). Layout redraws on seeds 1-60: 129 (79 before), every seed well inside `TRIES`.
- **Item 2, doorway frames** (`autolevel_build::felicity`: `TRIM`, `cut`, `clear_frame`, `frame`,
  role `trim` on 00EF): every doorway with no arch chamfered 13 at 45 degrees on both faces.
- `pd_import`'s `a_doorways_mouth_is_cleared_and_its_axis_given_waypoints` now checks the mouths of
  the doorways the pass handles (a stair's foot is left as it was by design).
- Showcase re-imported: **seed 15, `felicity15`, stage 127** (two landings, `landing1`'s stair room
  opening into the hall, a second gallery, rooms past the hall's far wall, the frames).

## Where things are

- `crates/autolevel/src/felicity.rs`: the planner (`generate` → `layout` → `level`). Streams
  `<seed>/felicity` (layout), `/doors`, `/balconies`. `Kind::Room` lots may be tall (`levels`).
- `crates/autolevel_build/src/felicity.rs`: the kit (`PALETTE`, `TYPES`, `schemes`, `Zone::base`,
  `clear_under` + `arch`, `clear_frame` + `frame`), all before `csg::fix_t_junctions`.
- `crates/autolevel/src/level.rs`: `Level` (it already has `glass`, `windows`, `glows`, `ceilings`
  fields, used by Complex; Felicity leaves them empty), `Doorway`, `Arch`.
- The bake: `autolevel_build::bake` / `complex::plan_lights` (lamps under walkways, as under
  Complex's catwalks). Complex's windows and glass: `level.windows`, `level.glass` and how
  `autolevel_build::complex` and `openings.rs` draw them: read them before item 3.
- Import: `crates/pd_import/src/autolevel/` (doors, waypoints `autonav.rs`, `probe_stalls`).

## The work, in this order

3. **Windows and light strips** ("Not yet" since the first version). Look at Felicity's own first:
   its tinted glass (9 `tinted_glass` props in `assets/stages/mp11/setup.json`: where, how big,
   between which rooms) and its lights (which textures glow, where: the triangles probe,
   `PD_PROBE_STAGE=mp11 PD_PROBE_OUT=tris.json cargo test -p pd_import --release
   probe_stage_triangles -- --ignored`, gives every BG triangle's room, texture and UVs; a script
   grouping by texture and room is the quickest look, as `trims.py` did for 00EF:
   `[room, texture, [w, h], [[x, y, z, u, v] x 3]]`, PD axes, y up). Then decide with the user if
   it isn't plain (windows between which rooms; glass as props or as faces).
4. **A courtyard** (the user's answer, 2026-11-17: "a courtyard", over a yard along the outside or
   both): one open-air yard inside the building, the building's walls round it, sky above, entered
   from corridors (and later seen from upper rooms' windows). One room with no ceiling, the sky
   drawn, the bake lit from above. Find out first how the importer and the game draw a sky for a
   custom stage (Complex-style levels are indoors too; GoldenEye's Facility import or `kokiri` may
   have one: `docs/GOLDENEYE.md`, `crates/pd_import/levels/*.json`) and how the bake treats a room
   with no ceiling. Ask the user anything that isn't plain (its size, a fountain or cover in it).
5. **The editor for Felicity levels** (a Felicity recipe takes no edits; `pd_edit --shot-generated`
   draws Complex's pipeline only). The largest; read AUTOLEVEL.md's "Track B: the editor" first,
   and propose a scope to the user before building.

## Rules that must hold (the acceptance for every step)

- **Complex unchanged**: `cargo test --release -p autolevel -p autolevel_build -p autolevel_edit
  -p pd_import` passes, and `target/release/autolevel.exe build 40` is byte for byte what it was:
  build it to a scratch folder before you start, `diff -r` after every step.
- **Felicity**: `cargo test -p autolevel --release` (the circulation targets on seeds 1-60); every
  walk passes on seeds 1-40 (`cargo build --release -p pd_import`, then for each seed
  `target/release/autolevel.exe build N --style felicity --out <dir> --no-bake`, last line "X of X
  walks as expected"; about 30 s for 1-40). New features get walks and tests.
- A script that does both (write it into the scratchpad; last session's, `check.sh`):
  build `pd_import`; `autolevel.exe build 40 --out $S/now40` and `diff -r $S/base40 $S/now40`; then
  the loop over seeds 1-40 printing any seed whose last line's two numbers differ.
- The three random streams stay separate; the doorway contract (a box through a 40 wall on a grid
  line, a half each side, 106 or 212 wide for doors, 212 tall, one doorway per pair of rooms);
  `Plan::check` passing.
- rustfmt on `autolevel`, `autolevel_build`, `autolevel_edit` only. `--release` always.
- **Never run `pd_edit` or the game without shot arguments.** Look with `target/release/pd_edit.exe
  --shot <code> out.png --at X,Y,Z,yaw,pitch --solid` (PD axes: x, height, -plan y; yaw 0 faces
  plan south, 90 east; from above: pitch -89, Y 7000 for a whole level; `views.json` in the build
  folder gives each room's view in cells of 118.11) or `--item door N`. Simulants:
  `PD_PROBE_SECS=300 PD_PROBE_STAGE=<code> cargo test -p pd_import --release probe_stalls --
  --ignored --nocapture`.
- Importing: `target/release/autolevel.exe build 15 --style felicity --out out/autolevel/felicity15`
  then `target/release/autolevel.exe import out/autolevel/felicity15 --code felicity15 --name
  "Felicity 15" --stagenum 127`. Seed 15's layout moves whenever the layout stream's draws change.
- Commit per step on `autolevel-kits` (end the message with the attribution line the system gives
  you); update the "Felicity" section of `docs/AUTOLEVEL.md` as you go. Ask the user for design
  decisions (they answer quickly and have chosen the recommended option every time but once: they
  took "more upper rooms by tall rooms" over a walkway turning the hall's corner). Hand off
  playtests with `cargo build --release` and what to look at.
- Tooling: the Bash tool mangles heredocs whose text holds apostrophes: write edit scripts with the
  Write tool into the scratchpad and run them (`open(p, newline='')` to keep LF). Test output is
  captured unless `-- --nocapture`. To find why layouts are redrawn, print the error in
  `generate`'s `Err(e)` arm and the misses after it, temporarily.
