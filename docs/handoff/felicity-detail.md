# Handoff: Felicity levels, detail until it stops paying

Written 2026-11-17; the next session starts cold. Branch `autolevel-kits`. Read `CLAUDE.md`
(loaded), then `docs/AUTOLEVEL.md`'s "Felicity" section, all of it, then the files below. This brief
replaces `docs/handoff/felicity-windows.md` (its "Done last session" list still holds; read it for
that). The user dropped the outdoor part (the courtyard) and put the editor aside.

## The direction (the user, 2026-11-17)

"Keep adding detail until I think we're reaching diminishing returns, so anything you see in the
actual Felicity level that can add something, we should try and add it, even the toilets and
dividers in the washroom ... we can literally have a washroom option." So: work through what
Felicity (`assets/stages/mp11`) has that our levels lack, measure each from Felicity's own data,
build it as generator pieces, show the user, and carry on until they call it. What they pointed at
(screenshots of `cifelicity`, which joins CI Training and Felicity, so some may be CI's: say which
you copy from):

- **Ceiling braces with lights**: in the blue-floor rooms, pilasters up both walls, a beam across
  the ceiling (and a cross beam), glowing light panels set into the beams.
- **Deep angled frames** round openings and repeated along a corridor (sloped faces inside).
- **Windows** with sloped reveals and a sill between rooms.
- **Steps up to a doorway** (a raised floor a few risers up, not a storey).
- Smaller: pipes along wall bases, hazard-striped sills and blocks, a leaning strut.
- **The washrooms**: toilets, cubicle dividers, basins (a "washroom option": a room type the
  generator can turn on or off).

## What was measured (2026-11-17, `trims.py` / `grey.py`-style scripts over the triangle dump)

Felicity is 4,954 BG triangles, 908 of them slanted: room 12 (the grey room, 741 triangles, a third
slanted, greys 0714 and 0644), the hall's rooms 33-39 (arches, 0719 and band 0288), the frames
(00EF, rooms 3 and 23-39). Felicity 15 is already ~15,000 faces, because the carver splits faces
on a 212 lattice: adding detail should come with merging faces where it can (check what the game's
renderer and the importer cost first; nothing measured yet).

Dump: `PD_PROBE_STAGE=mp11 PD_PROBE_OUT=tris.json cargo test -p pd_import --release
probe_stage_triangles -- --ignored` gives `[room, texture, [w, h], [[x, y, z, u, v] x 3]]` per BG
triangle (PD axes, y up). Props (toilets, glass, lights may be props, not BG): `assets/stages/mp11/
setup.json`. Textures: `assets/textures/<preset>.png`. Look at Felicity itself with `cargo run
--release -p pd_tools --bin pd_snapshot -- <outdir> stage ref` style commands (see CLAUDE.md:
`out stage mp11 --at x,y,z,theta`), never a window.

## How to build it (the plan agreed with the user)

1. **One shape tool** in `autolevel_build`: a 2D profile swept along a path (round an opening, up a
   wall / across the ceiling / down, along a wall's base), mitred at corners, explicit UVs (u along
   the path, v round the profile), a kit role per profile edge. Rebuild `frame` (and the arch's
   soffit if it fits) on it first, and show they come out the same (the kit's test).
2. **One clearing step**: generalise `clear_frame`'s `cut` into "remove the carver's faces a piece
   covers in a plane"; pieces proud of a wall may leave the wall behind them.
3. **Placement** in the planner or the kit by rules (braces every few cells across a room's short
   span, pilasters clear of doorways, ...), on their own random stream (`<seed>/felicity/dress`)
   so dressing never moves a room. Proportions measured from Felicity, as the frames were.
4. **Gameplay**: overhead pieces are free; anything on the floor (pilasters, pipes, struts,
   toilets, dividers) is collision: the walks must still pass, the doorway mouths stay clear, and
   simulants shouldn't snag (`probe_stalls`).
5. **Lights**: glowing panels must light the bake (`complex::plan_lights`, Complex's `glows`).
6. **Windows** are a new link between rooms nobody walks: a portal in the importer (PD draws a room
   only through portals), reveals and sill from the shape tool, glass (Felicity's 9 `tinted_glass`
   props, or faces).
7. **Raised doorways**: a floor a few risers up changes the planner's "doorways sit on a storey"
   rule and the importer's door and waypoint assumptions: read `pd_import::autolevel` first.

Suggested order (each about a session; re-plan freely as the user reacts): overhead first (shape
tool, frames on it, braces with inset lights, corridor ribs), then floor-level pieces (pilasters,
pipes, struts, deep frames on grey doorways), the washroom fittings and option, windows, raised
doorways, then whatever else Felicity shows.

## Where things are

- `crates/autolevel/src/felicity.rs`: the planner (`generate` → `layout` → `level`); streams
  `<seed>/felicity`, `/doors`, `/balconies`. `crates/autolevel/src/level.rs`: `Level` (has `glass`,
  `windows`, `glows`, `ceilings`, used by Complex; Felicity leaves them empty), `Doorway`, `Arch`.
- `crates/autolevel_build/src/felicity.rs`: the kit (`PALETTE`, `TYPES`, `schemes`, `Zone::base`,
  `clear_under` + `arch`, `TRIM`, `cut`, `clear_frame` + `frame`), all before
  `csg::fix_t_junctions`. Complex's openings and glass: `autolevel_build::complex`, `openings.rs`.
- Import: `crates/pd_import/src/autolevel/` (doors, `autonav.rs` waypoints, `probe_stalls`).
- Showcase: **seed 15, `felicity15`, stage 127**.

## Rules that must hold (the acceptance for every step)

- **Complex unchanged**: `cargo test --release -p autolevel -p autolevel_build -p autolevel_edit
  -p pd_import` passes, and `target/release/autolevel.exe build 40` is byte for byte what it was
  (build it to a scratch folder before you start, `diff -r` after every step).
- **Felicity**: `cargo test -p autolevel --release` (circulation targets, seeds 1-60); every walk
  passes on seeds 1-40 (`cargo build --release -p pd_import`; per seed `target/release/autolevel.exe
  build N --style felicity --out <dir> --no-bake`, last line "X of X walks as expected"; ~30 s for
  1-40). Write one scratchpad script that does both checks. New pieces get tests.
- The three random streams stay separate (a new one for dressing); the doorway contract; `Plan::check`.
- rustfmt on `autolevel`, `autolevel_build`, `autolevel_edit` only. `--release` always.
- **Never run `pd_edit` or the game without shot arguments**: `target/release/pd_edit.exe --shot
  <code> out.png --at X,Y,Z,yaw,pitch --solid` (PD axes: x, height, -plan y; yaw 0 faces plan
  south; whole level from above: Y 7000, pitch -89; `views.json` in the build folder has each
  room's view in cells of 118.11) or `--item door N`.
- Showcase: `target/release/autolevel.exe build 15 --style felicity --out out/autolevel/felicity15`,
  then `target/release/autolevel.exe import out/autolevel/felicity15 --code felicity15 --name
  "Felicity 15" --stagenum 127`.
- Commit per step on `autolevel-kits` (attribution line as the system gives it); update the
  "Felicity" section of `docs/AUTOLEVEL.md`. Ask the user for design decisions; show them each new
  piece (re-import, say where to look) and ask whether to keep going.
- Tooling: the Bash tool mangles heredocs holding apostrophes: write edit scripts with the Write
  tool into the scratchpad (`open(p, newline='')` keeps LF). Test output needs `-- --nocapture`.
