# Handoff: Felicity levels, detail until it stops paying (part 2)

Written 2026-11-17; the next session starts cold. Branch `autolevel-kits`. Read `CLAUDE.md`
(loaded), then **`docs/handoff/felicity-detail.md`** (the direction, the plan, the rules: all still
hold), then `docs/AUTOLEVEL.md`'s "Felicity" section to its end (the last four paragraphs are this
session's: the shape tool, ceiling braces, corridor ribs, pipes), then the files below.

## Done this session (each committed, documented, the acceptance passing)

1. `46c53fb` **The shape tool** `autolevel_build::sweep`: a profile swept along a path of legs
   (each leg its own profile in its own `side`/`depth` axes, the same number of points), mitred
   where legs meet (profiles of different depths too), explicit UVs (`Edge::along_v`), `Leg::along`
   to restart the distance along, `Sweep::caps` for open ends, `closed` paths. `sweep::clear` +
   `Cover`: take what a piece covers out of the carver's faces (cut on a rectangle in a plane,
   drop the pieces inside; slivers on a cut line go too). Doorway frames rebuilt on it (same
   polygons, test `the_swept_frames_are_the_frames_built_by_hand`), trim UVs now Felicity's.
2. `db6675f` **Ceiling braces with light panels** (Felicity's rooms 4 and 17): `autolevel::dress`
   (new: the dressing, its own streams `<seed>/felicity/dress/<piece>`, run after the plan check),
   `Level::braces` / `dress::arms` / `Level::lamps`; the kit's `arm`, `crossing`, role `lamp`
   (0262); `bake::light` lights a room with lamps of its own from them in place of its grid, and
   every `lamp` face bakes 254.
3. `afb0170` **Corridor ribs** (the user: "try it and see"; 70% for room braces confirmed): the
   brace across every one-storey office/marble corridor every 3 cells. Not Felicity's idea.
4. `7d7d002` **Pipes** along the base of industrial rooms' and corridors' walls (Felicity's
   hexagon from room 39, 012A shading), broken at doorways and kept out of their mouths.

Showcase: **Felicity 15, stage 127** (braces in upper1/6/7, ribs in corridor3 and the galleries;
no pipes on this seed). **Felicity 5, stage 123** (imported this session to show pipes: room2,
room4, corridor1). The user has not seen ribs or pipes in the game yet.

## Next, in the brief's order (re-plan as the user reacts)

Measure each from Felicity first and say whether it is Felicity's or CI Training's.

- **Struts** (a leaning strut) and **hazard stripes**: not found in Felicity yet. 012A (thought to
  be stripes) is the pipe shading. Look at `cifelicity`'s CI half (`custom/stages/cifelicity`,
  `PD_PROBE_STAGE=cifelicity`) and say they're CI's if so.
- **Deep angled frames on grey doorways**: Felicity's grey room (12, dump x -1404..-823, z
  -982..260, floor -480) has rusted steel frames round its openings (seen in a snapshot, not yet
  measured: find their texture and depth in the dump).
- **Room 12's floor**: raised 54 on a 0714 grating, chamfered kerbs 59 wide along both long walls,
  short slanted pieces at intervals; and **room 35's floor ridge** (012A, 41 wide, 10 tall, 202
  long, `faces.py tris.json 35 12A`). Candidates for industrial rooms.
- **Washroom fittings** (the user wants a "washroom option" the generator can turn on or off):
  Felicity's washrooms are its rooms 26-28 (dump x 610..1344; a snapshot at `--at
  -1350,-480,200,180` showed toilets and cubicle dividers). Check `assets/stages/mp11/setup.json`
  first: the toilets may be props, not BG. The option is a recipe/`Options` field: ask the user
  how it should appear (on by default?) before adding one.
- **Windows** between rooms (a portal in the importer, reveals and sill on the sweep, glass) and
  **raised doorways** (the planner's doorway rule, the importer's doors and waypoints): each about
  a session; read `pd_import::autolevel` first.

## How to work (what this session learned)

- **Acceptance script** (scratchpad, not durable; recreate it): build `autolevel.exe build 40 --out
  <base40>` once before changing anything, then after each step `diff -r` a fresh build of 40
  against it, and for seeds 1-40 `autolevel.exe build N --style felicity --out <dir> --no-bake`,
  whose last line must be "X of X walks as expected". Plus `cargo test --release -p autolevel -p
  autolevel_build -p autolevel_edit -p pd_import`, and `PD_PROBE_STAGE=<code> cargo test -p
  pd_import --release probe_stalls -- --ignored --nocapture` on the showcase.
- **Measuring Felicity**: `PD_PROBE_STAGE=mp11 PD_PROBE_OUT=tris.json cargo test -p pd_import
  --release probe_stage_triangles -- --ignored` now gives `[room, tex, [w, h], [[x, y, z, u, v, r,
  g, b] x 3]]` (corner colours added). Textures print as decimal ids (647 = 0x287). Small Python
  over it: group a room's triangles by (texture, normal) with their bounding boxes and heights
  above the room's floor; list slanted faces by room; print one texture's triangles with UVs.
- **Looking at Felicity**: `cargo run --release -p pd_tools --bin pd_snapshot -- <dir> stage mp11
  --at X,Y,Z,THETA --full`, where **X is the negative of the dump's x** (Y height, Z the dump's z).
- **Looking at our levels**: `target/release/pd_edit.exe --shot <code> out.png --at
  X,Y,Z,yaw,pitch --solid` only (PD axes: x, height, -plan y; yaw 90 looks along +x, 180 along
  +plan y). **Never pass `--help` or anything else to `pd_edit`**: it opens the window (it did
  once this session; stopped at once).
- To find what a seed placed (braces, pipes) without adding code: append a throwaway `#[test]`
  that prints it to `dress.rs`, run it with `-- --nocapture`, then restore the file from a copy.
- Edit scripts with apostrophes: write them with the Write tool into the scratchpad, run with
  Python (`open(p, newline='')`); bash heredocs break on them.
- Every dressing piece: placement in `autolevel::dress` (pure, its own stream, tested over seeds
  1-40 for clearances), drawing in `autolevel_build::felicity` on the sweep, `sweep::clear` for
  what it covers, a kit test, the acceptance, a commit, AUTOLEVEL.md, re-import, tell the user.
