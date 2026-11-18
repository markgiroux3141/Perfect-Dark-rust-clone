# Handoff: Felicity levels, detail until it stops paying (part 3)

Written 2026-11-17; the next session starts cold. Branch `autolevel-kits`. Read `CLAUDE.md`
(loaded), then **`docs/handoff/felicity-detail.md`** (the direction, the plan, the rules: all still
hold), then `docs/AUTOLEVEL.md`'s "Felicity" section to its end (this session's paragraphs: the
room types' note, hazard surrounds, portals, washroom fittings), then `crates/autolevel/src/dress.rs`
and `crates/autolevel_build/src/felicity.rs` (every dressing piece is in those two files).
`docs/handoff/felicity-detail-2.md` is the session before (the shape tool, braces, ribs, pipes).

## Done this session (each committed, documented, the acceptance passing)

1. `12dc875` **Industrial and washroom looks swapped.** Looking at Felicity itself showed the kit
   had them the wrong way round: Felicity's washroom (room 12, the toilets) is black tile 0274 /
   marble 0872 / white floor 070F, its grey concrete bays (rooms 26-32) are 071B / 071D / 0271 /
   rock 0281. Roles are now `bay_*` (on `industrial`, and the hall-side rooms' ceiling) and `wc_*`
   (on `washroom`). The user chose the swap.
2. `62dcc33` **Hazard surrounds** (Felicity's 071E stripes round its grey bays' doorways, over a
   01B2 grating, the wall tile splaying back): every doorway face in a one-storey industrial room
   or corridor, standing out of the wall (ours are 40 thick, Felicity's 92). `dress::surrounds`,
   `felicity::surround`; that face loses its chamfer; pipes stop beyond the splay.
3. `66281f2` **Portals** across industrial corridors (the openings between Felicity's bays: a
   bevelled frame 105 deep, leaning jambs, the lintel's underside a light strip 071F baked 254 and a
   lamp under it). These are the "struts" the user saw: Felicity has no separate strut piece.
   Narrower than Felicity's (posts 13, lean 40) so the walks pass (see AUTOLEVEL.md).
4. `639e204` **Washroom fittings**, the user's option: `Options::washroom` (on by default; recipe
   `"washroom": false`; `--no-washroom`). Stalls (tiled partitions), toilets (pedestal, flaring
   bowl, rim, water), basins on the wall, kept off doorway mouths and walk lines. Off changes
   nothing else (tested).

**Showcases** (all re-imported this session): **Felicity 15, stage 127** (portals in corridor1 and
corridor2, surrounds on their doorways); **Felicity 5, stage 123** (surrounds round corridor1 and
rooms 1-4, portals in corridor1, pipes); **Felicity 36, stage 122** (new: the only showcase with
washrooms: upper2-4 off the gallery). The user has not seen any of this session's pieces in game.

## Next (re-plan as the user reacts)

Measure each from Felicity first and say whether it's Felicity's or CI Training's.

- **Windows** between rooms (a portal in the importer: PD draws a room only through portals;
  reveals and sill from the sweep; glass from Felicity's 9 `tinted_glass` props or faces). Read
  `pd_import::autolevel` first. Not measured yet in Felicity: find its windows in the dump
  (`tinted_glass` in `assets/stages/mp11/setup.json`, and BG openings between rooms that no floor
  crosses).
- **Raised doorways**: Felicity has one in its bay room 30's wall z 736, x 2017..2176: the floor
  rises in two treads of grating 01B2 (23 and 70 up), riser 0717, jambs 0712, a 00EF frame, lintel
  at 283-294 (`python roomtex.py tris.json 30 -763` style scripts, below). Changes the planner's
  "doorways sit on a storey" rule and the importer's door and waypoint assumptions.
- Smaller things Felicity has that we don't (each measured only roughly): the bays' **leaning
  walls and pitched ceilings** (rooms 29-32: wall straight to 94 over the hazard base 0276, then
  leaning in 53 to 283; the ceiling pitched from 283 to 330 along the middle) — would suit the
  industrial corridors with the portals; the washroom's **vanity block** (only fits a bigger
  washroom: the planner makes them 3-4 x 3 cells) and taps; vents 01B2 in the hall's rooms (24-39);
  024D (tan with a light panel, rooms 2, 5, 10, 18).

## How to work (what this session learned)

- **The triangle dump's coordinates are the game's.** `pd_snapshot <dir> stage mp11 --at
  X,Y,Z,THETA` takes the dump's x, y, z unchanged (the part-2 brief said x was negated: wrong).
  Feet on a real floor: under a raised floor the view is of somewhere else (that is how room 12
  was taken for a grey room). Theta 0 faces +z, 90 faces -x, 270 faces +x. `--views file.json`
  (a list of `{"name","x","y","z","theta"}`) renders many in one run; `--full` names its file
  `<code>_spawn0_full.png` even with `--at`.
- **Measuring scripts** (scratchpad, recreate as needed): `rooms.py` (each room's bounds and floor
  heights), `roomtex.py tris.json ROOM FLOOR` (a room's faces grouped by texture and normal, with
  extents and heights above the floor), `faces.py tris.json ROOM TEXHEX` (one texture's triangles
  with UVs), a texture contact sheet with Pillow (`assets/textures/<id>.png`, lower case).
- **Acceptance script**: build `autolevel.exe build 40 --out <base40>` before changing anything;
  after each step `diff -r` a fresh build; seeds 1-40 `--style felicity --no-bake`, last line "X of
  X walks as expected"; `cargo test --release -p autolevel -p autolevel_build -p autolevel_edit -p
  pd_import`; `PD_PROBE_STAGE=<code> cargo test -p pd_import --release probe_stalls -- --ignored
  --nocapture` per showcase. A failing walk's name says which room (`room_<key>_i_j`: door to door
  in that room, lines `INSIDE` 100 in from each doorway's middle plane, so 80 off the wall's face;
  the walker is radius 30, 185 tall). Anything standing in a room must keep 40 off those lines.
- **To print what a seed placed**: a throwaway file `crates/autolevel/tests/zz_print.rs` (a
  `#[test]` calling `generate` and printing), `cargo test --release -p autolevel --test zz_print --
  --nocapture`, then delete it. Simpler than editing `dress.rs`.
- **Edit scripts** with apostrophes: write them with the Write tool into the scratchpad and run
  with Python (`open(p, newline='')`); bash heredocs break on them. rustfmt rewraps long `use`
  lines, so match on short fragments.
- `pd_edit --shot <code> out.png --at X,Y,Z,yaw,pitch --solid` (PD axes: x, height, -plan y; yaw
  90 looks along +x, 0 along -plan y, 180 along +plan y). Small rooms only allow close views.
  **Never run `pd_edit` or the game without shot arguments.**
- Stage numbers in use: 100-120 (Jedi Academy imports), 122 (felicity36), 123 (felicity5), 124-126
  (autolevel40/1/7), 127 (felicity15). 121 is free.
- Every dressing piece: placement in `autolevel::dress` (pure, tested over seeds 1-40 for
  clearances), drawing in `autolevel_build::felicity` (the sweep, `sweep::clear` for what it
  covers), a kit test, the acceptance, a commit, AUTOLEVEL.md, re-import, tell the user.
