# Autolevel: Complex-style levels from a seed

The level generator: a seed makes a whole Complex-style arena (rooms scattered, joined by
corridors, furnished, lit, walked) with no model in the loop and no Blender, and an editor
lets a person shape one. It is three crates here, `autolevel`, `autolevel_build` and
`autolevel_edit`, moved in on 2026-11-16 from the Blender MCP repo
(`D:\Claude Code Projects\Blender MCP for level generation`, "the Blender repo"), where it
was ported from that repo's Python (`addon/ge_pd_tools/autolevel`). This file is the plan and
the status board for it; MILESTONES.md's M21 points here. The ideas behind it are in the
Blender repo's `docs/levels/autolevel/IDEAS.md`. **What is left** (the user's decisions, the known
rough edges, the bigger directions) is in [AUTOLEVEL_BACKLOG.md](AUTOLEVEL_BACKLOG.md).

**What stayed in the Blender repo**, and is still used: the frozen Python (the answer key's
source), `scripts/autolevel_key.py` (writes the keys into this repo's
`crates/autolevel/tests/key/` and `crates/autolevel_build/tests/key/`), the compare scripts
(`scripts/autolevel_compare.py`, `autolevel_compare_glb.py`), the Blender reference builds
(`work/autolevel/ref/`) and pd-walk (`viewer/`). Paths below that start `scripts/`, `viewer/`
or `addon/` are that repo's.

## Decisions (the user's, 2026-11-16)

1. **Every seed keeps its level.** The Rust port reproduces Python's random generator
   exactly, so seed 40 is still seed 40.
2. **The Python is frozen.** It is the answer key's source. New rules go into the Rust
   only; the user has not yet said when the Python is retired. A rule changed on purpose
   means the keys are re-written (from the Rust's own output) and committed with it.
3. **The editor is built to merge into `pd_edit`** (egui through `engine`, a headless
   document with snapshot undo). See "Track B".
4. **No "Blender-free in Python first" step.** The Blender jobs were replaced in Rust.
5. **Moved here** (2026-11-16): the three crates are members of this workspace, a level made
   by `autolevel build` is imported as an arena (`autolevel import`), and walks run in this
   process on `pd_sim` (`pd_import::autowalk`). See "In the clone".

## Status

| Stage | Python source | Rust | Key file | State |
|---|---|---|---|---|
| Random generator | `random.Random` | `pyrand.rs`, `sha512.rs` | `rng.json` | done, exact |
| Python arithmetic helpers | (language) | `py.rs` | unit tests | done |
| Scatter | `scatter.py` | `scatter.rs` | `lots.json` | done, 98 cases exact |
| Settle, compact, kinds and site | `level.py` (`settle`, `compact`, `kinds_and_site`) | `place.rs` | `lots.json` | done, exact |
| Spiral towers | `autolevel/spiral.py` | `spiral.rs` | `spiral.json` | done, 200 cases exact |
| Spaces (pools, door storeys, pods' ways in) | `route.py` (`Space`, `spaces`, `pool_rect`, `shell_rect`, `door_pad`), `furnish.pod_access`, `level.stage_spaces` | `space.rs`, `pods.rs`, `level.rs` | `spaces.json` | done, 20 levels exact |
| Corridors, stubs, vents, the network | `route.py` (`Grid`, `connect`, `components`), `scatter.hops`, `level.stage_network` | `route.rs`, `network.rs`, `level.rs` | `routes.json` | done, 20 levels exact |
| Room interiors, with the drops and grates | `furnish.py`, `level.py` (`_openings`, `_dress`, `_layout`, `_Build.refresh`, `stage_interiors`), `plan.py` (`box`, `cells`, `subtract`, `Ramp`) | `furnish/` (`mod.rs`, `decks.rs`, `ramps.rs`, `nav.rs`, `cover.rs`, `rails.rs`, `plans.rs`, `pod.rs`), `build.rs`, `plan.rs`, `level.rs` | `interiors.json` | done, 20 levels exact |
| Dead ends | `deadends.py`, `level._Build` (`ends`, `resolve`, `join`, `shorten`, `_respiral`) | `deadends.rs`, `build.rs`, `level.rs` (`stage_dead_ends`) | `deadends.json` (20 levels, 94 fixes) | done, 20 levels exact |
| The level: windows, plan, walks, views, report, score | `level.py` (`Level`, `generate`, `_windows`, `_plan`, `_walks`, `_views`, `report`, `score`), `plan.py` (`Room`, `Link`, `Plan`, `check`, `graph`) | `level.rs` (`Level`, `generate`, `stage_level`), `windows.rs`, `planner.rs`, `walks.rs`, `report.rs`, `plan.rs` | `level.json` | done, 20 levels exact |
| Polygons | `geometry.py`, `csg.py`, `sunbeam.py`, `styles/complex.py` (what the generator calls) | `autolevel_build`: `csg.rs`, `sunbeam.rs`, `complex.rs`, `geometry.rs` | `autolevel_build/tests/key/polys.json`, `lights.json` | done, 20 levels exact |
| Room meshes | `build.emit` | `autolevel_build/mesh.rs` | rooms and triangles equal to the Blender build | done |
| GE64 OBJ | `blender_io.export_level`, `formats/ge64_obj.py`, `formats/level_files.py` | `autolevel_build/obj.rs` | `scripts/autolevel_compare.py` against a Blender build; pd-walk | done: seeds 40, 1, 7 agree and walk clean |
| GLB | `gltf_export.py` | `autolevel_build/glb.rs` | `scripts/autolevel_compare_glb.py` against the Blender file; the Khronos validator | done: the same for seeds 40, 1, 7; valid |
| Lighting bake | `build.bake_lighting`, `cx.bake`, `add_beam`, `glow` (Blender) | `autolevel_build/bake.rs`, `bake/trace.rs` | `autolevel_compare.py --light`: tolerance, not exact | done: within 1 of 255 everywhere, in 55 ms |
| Command line | `scripts/autolevel.py`, `scripts/plan_walks.py`, `plan.write_svg` | `autolevel_build`: `cli.rs`, `svg.rs`, `walk.rs`, `textures.rs`; `autolevel/deadlist.rs`; the binary is `pd_import`'s (`src/bin/autolevel.rs`) | the Python's files and output; walks pass | done: the binary `autolevel` |
| Walking in this process | pd-walk (`viewer/`) | `pd_import::autowalk` (`SimWalk`, on `pd_sim`'s player and the importer's own tiles) | every route of six levels, the same outcome as pd-walk | done (2026-11-16) |
| As an arena | new | `autolevel import`: `pd_import::autolevel` (a source of its own: the level made again from its recipe, its rooms and openings as PD's rooms and portals, Complex's materials on the pool's textures) and `pd_import::autonav` (the generator's ways put into the waypoints) | tests in `pd_import` (rooms, portals, textures, tiles, sight, the repair); `pd_snapshot stage autolevel40`; `autolevel shots` | done (2026-11-17); nobody has played one |
| Kits (what a level is dressed in) | new | `autolevel_build/kit.rs` (`Kit`: palette, schemes, solid tags, tints, brightness targets, the arena whose material rows the import uses); Complex is `complex::KIT`, passed to `geometry`, `mesh`, `bake`, `obj`, `glb`, `Built` and `pd_import::autolevel` | the keys; seed 40's build byte for byte the same as before | Complex only (2026-11-17). A level's shape (storeys, doors, ramps, the module) is still the planner's and Complex's; `built::make` always takes `complex::KIT` until a recipe names its kit |
| Felicity-style levels | new (not a port) | `autolevel/felicity.rs` (the planner), `autolevel_build/felicity.rs` (the kit and its polygons), `pd_import::autolevel::doors` | `felicity` unit tests (40 seeds: plan check, the same twice, one piece, door shares, stairs, types; 60 seeds: the circulation targets); `autolevel build N --style felicity` walks every route of seeds 1-40 | second planner (2026-11-17): corridors first, rooms as offshoots; nobody has played one. See "Felicity" |
| Recipe (seed + options + edits) | new | `autolevel/recipe.rs` | unit tests | format 1; all fourteen edits apply |
| Editor | new | `autolevel_edit`, `autolevel/candidates.rs`, `boxes.rs`, `choices.rs` | `--shot`, tests | milestones 1 to 5 done (2026-11-17); nobody has looked at a window |

**Checking a change.** `cargo test --release -p autolevel -p autolevel_build -p autolevel_edit
-p pd_import` (always `--release` here: a dev build of this workspace is unusably slow). The
layout's tests take about twenty seconds (the key's 20 levels at every stage, side by side).
Then `cargo clippy --release -p autolevel -p autolevel_build -p autolevel_edit --all-targets`
and `cargo fmt -p autolevel -p autolevel_build -p autolevel_edit` (each of the three has its
own `rustfmt.toml`, width 160; the rest of this workspace is not run through rustfmt), and
`python tools/check_boundaries.py`. The keys themselves are written in the Blender repo:
`python scripts/autolevel_key.py [stage]` there must leave them byte for byte as they are
(it needs Python 3.12 or later). What cannot be exact is compared with a Blender build of the
same level, also there: `python scripts/autolevel_compare.py REF.obj NEW.obj --light` and
`python scripts/autolevel_compare_glb.py REF.glb NEW.glb --colours`, with `REF` in its
`work/autolevel/ref/`. One level takes about 0.4 s to plan (`cargo run --release -p autolevel
--example generate -- 40`); the Python took about 4 s.

## Step 0: groundwork in Python

- **Random numbers: done.** `pyrand.rs` is the Mersenne Twister seeded as CPython seeds it
  from an int, or from a string (the string's bytes plus their SHA-512, as one big number).
  The six calls the generator uses (`random`, `randint`, `randrange`, `choice`, `shuffle`,
  `uniform`) draw what Python's draw. SHA-512 is written out in `sha512.rs` with its
  constants computed, so there is no hash crate to add to the clone.
- **Set order: done for the layout stages.** I read `scatter`, `route`, `spiral`,
  `furnish`, `level` and `deadends` for places where a result depends on the order Python
  walks a set. Two did, and now break ties by place: which piece of deck gets its ramp
  first (`furnish._solve`), and a grate's view (`level._views`). Checked on seeds 1-40:
  only seed 34's camera views changed; no level did. `plan` was read with the level stage
  (`check` and `graph` walk sets of two room keys, and only count with them).
  **The polygon modules: read too, and nothing found** (`csg`, `sunbeam`, `styles/complex`,
  `geometry`, `plan.to_carver`; 2026-11-16). One set is walked for its order:
  `csg.fix_t_junctions` files every vertex into a grid of buckets from a set. The vertices
  found on an edge are then sorted as `(t, vertex)`, a total order, so the bucket order
  cannot show. The rest are sets only asked what they hold (`occ`, `used`, `keys`, `low`,
  `small`, the lit ids) or sorted before use (`_grid`'s lines, `hull`). Checked by running
  it: seeds 1-40 give the same polygons, in the same order, with that set walked as Python
  walks it, sorted, and sorted backwards. So no Python changed and no key was re-written.
  Dict order does matter there and is part of the port: the Carver's `air` cells (a cell a
  solid takes out and a later carve puts back goes to the end) and the face groups in
  `polygons`. Also found: every vertex of a generated level is a whole number, and no room
  takes two beams (the lit polygons are kept by `id()`, which a second beam in one room
  could confuse).
  Seeds 23 and 40 give the same plan, walks and polygons (in the same order) under
  `PYTHONHASHSEED` 0, 1 and 12345, so nothing in any stage depends on how strings hash.
- **Stages on their own: started.** `level.generate` now runs `stage_spaces`,
  `stage_network` and `stage_interiors`, which the key script calls. The later stages (the
  dead-end pass, `_plan`, `_walks`) need the same when their turn comes.
- **The answer key: started.** `python scripts/autolevel_key.py [stage ...]` writes
  `crates/autolevel/tests/key/<stage>.json`. Sets of cells are written as sorted rows
  `[y, x0, x1)`. Floats that are not whole or half numbers are written as their 64 bits,
  because serde_json's default reader can be one unit off in the last place.
- **The recipe: not built.** See below.

## Track A: the port

### How a module is ported

1. Add its stage to `scripts/autolevel_key.py` and write the key. Keep keys small: a few
   seeds, sets as rows.
2. Port the Python as it is written, function for function, with the same names where
   Rust allows. Do not improve it on the way: a better algorithm that gives another level
   is a failure here.
3. Add a test in `tests/key.rs` that compares the Rust output with the key.
4. It is done when the test passes for every case, `cargo clippy --all-targets` is clean
   and `cargo fmt` has been run.

### Where Python and Rust differ (all of these bite)

- `round()` rounds halves to even: `py::round`. `int()` truncates.
- `//` and `%` floor: `div_euclid`, `rem_euclid`.
- `max()` and `min()` return the first of equal items. Rust's `max_by_key` returns the
  last. Use `py::first_max` and `py::first_min`.
- `sorted()` is stable, and `sorted(reverse=True)` keeps equal items in their original
  order: sort with the comparison reversed, never sort then reverse.
- Dicts keep insertion order, and the Python relies on it (`sp`, `Grid.doors`,
  `interiors`). Use a `Vec` of pairs or keep a separate order list. Not `HashMap`.
- Sets have no order: use `cells::Cells` (a `BTreeSet`). If the Rust result differs from
  the key at a place that walks a set, the Python has an order dependence that was missed:
  fix the Python with an explicit tie-break, check the fingerprints of seeds 1-40, and
  re-write the key.
- `heapq` with tuples is a total order on the whole tuple, so any correct min-heap pops
  the same sequence. Costs are floats in halves: compare them exactly.
- String seeds: `random.Random(f"{seed}/{key}/{attempt}")` is `PyRandom::named(...)`. The
  order of the draws is the order Python evaluates them in, arguments left to right.
- Module globals the Python changes at run time (`furnish.set_walkway` sets `BAND` in
  three modules) become parameters. `spiral::layout` takes `band`.

### Order of work

1. **Spaces**: done.
2. **Routes** (`routes.json`): done. `route.rs` is `Grid` (the owner arrays and their window
   sums: numpy in Python, plain arrays in Rust), `direct`, `route` and `_route` (A\*, here
   `route_with`), `_assemble`, `vent`, `commit`, `remove`, `drop`; `network.rs` is `Network`,
   `connect` and `components`. Keyword arguments became structs with the Python defaults
   (`Terms`, `VentTerms`, `Connect`), and the catwalk depth (`route.BAND`) is the grid's
   `band`. `la`, `lb`, `search`, `storeyed`, `remove` and `drop` are for the dead-end pass:
   the key does not reach them, so they have unit tests. The Python knows a route and its
   doors by identity; the Rust knows a route by its first piece's key and a door by its
   fields. A piece's `cells` keep the order the storeys were reached in (the openings pass
   walks them in that order).
3. **Furnish** (`interiors.json`): done. `furnish/` is `Interior` and everything under it, a
   file for each of the Python's sections; `build.rs` is `openings`, `dress`, `layout` and
   `Build` (`refresh`, `furnish`, `sided`, with `caps` and the cache of insides already there
   for the dead-end pass). `Interior`'s keyword arguments are `Setup`, and `BAND`/`WIDE_H` are
   its `band`. The build's rooms are the network's (`net.spaces()`), as the Python's `sp` is
   one dict the build and the grid share, so `stage_interiors` takes the network and no `sp`.
   An inside is an `Arc<Interior>`, shared by the cache and the level. Besides the key, 1,500
   rooms made by hand at random (pods, pools, holes, spirals, both walkway widths, every
   layout and cover) were furnished by both and compared field by field, with `path`,
   `waypoints` and `stand`: none differed, and the 445 that can't be furnished gave the same
   reason. That run reached what the key's levels do not: ramps taken back and tried again,
   the budget of tries spent, a storey claimed by a pod's roof. Two things are not as the
   Python: a pool too narrow for its bridge is an error here (the Python stops with a
   TypeError), and `components` returns its pieces by their least cell (no caller depends on
   the order: `_solve` breaks its ties by place). Never reached by either run, so unchecked:
   cover taken away again because it cut a door off (`place` returning false).
4. **Dead ends** (`deadends.json`): done. `deadends.rs` is the finder (`Place`, `Way`, `Graph`,
   `DeadEnd`, `Nav`, `graph`, `find`, `joins`); the pass is the rest of `Build` in `build.rs`
   (`ends`, `resolve`, `join`, `shorten`, `respiral`, and the constants), and
   `level::stage_dead_ends` runs it. `find`'s keyword arguments are `Limits`; `joins` takes its
   three (`REACH`, `STOREYS`, `EACH` are the Python's defaults). A fix is the enum `Fix`
   (`Joined`, `Cut`, `Dropped`), and `DeadEnd::cut` is a list of (room, storey) in the order
   the rooms were met, which is the order `shorten` cuts them in. The Python knows places by
   their keys; `find` numbers them once and works on the numbers, and every order that is by
   key still is (the blind ways' sort, the heap's ties, the places nearest first, the middle's
   tie on its least key). Each inside's standing blocks are kept by where its `Arc` lives (the
   Python: by `id`), which holds because the build keeps every inside it ever made. `resolve`,
   `join` and `shorten` return `Result`: a refresh that fails outside a join's own trial ends
   the level, as `Failed` does. Nothing in it walks a set in an order that matters: `_reach`'s
   sets are only asked what they hold, and `_rooms` sorts.
   Besides the key, 65 more levels were dumped from the Python the same way and compared, every
   field (seeds 1-60 less the key's and the four that make no level, and others with
   `standard`, `size=2`, `walkway=2`, no vents, more drops and grates, and the four dead-end
   options set): 211 joined, 45 cut, 6 left out, none differed. The key reaches a join taken
   back because it did not help, and one because a room could not be furnished with the new
   door. Not reached by either: a pass that fails (leaving a room out cuts the level in two,
   or a room can't be furnished after a cut).
   **One thing is no longer as the Python** (fixed in the Rust on 2026-11-16, the user's
   decision; the frozen Python still has it). `_measure` took each place to be reached through
   one that comes before it in (distance, key) order. A drop costs nothing, so a floor dropped
   onto from the very block the room above was entered by is no further off than that room,
   and if its key sorts first it was handled before the place it was reached through. Where it
   was not a room to be in, that was a lookup of a place not yet measured (the Python: a
   KeyError; the Rust: a panic). Where it was, nothing failed, but the places before the one
   above it were never marked as leading to a room, so they were counted as a climb to nothing.
   `measure` now walks the places in the order the search settled them (each after the one it
   was reached through, whatever their keys), and still gives everything out in (distance,
   key) order. Without such a pair the two orders are the same, so no result moved: the key
   passes unchanged, and 720 runs (seeds 1-80 under nine sets of options: 651 levels, 416
   drops, 2,615 fixes) came out byte for byte the same before and after. No level in them has
   such a pair (one has a drop inside a measured dead end at all), so the keys were not
   re-written. Two unit tests hold the two cases
   (`a_place_is_measured_after_the_one_it_was_reached_through_whatever_their_keys`,
   `a_room_dropped_into_at_no_cost_counts_for_every_deck_on_the_way_to_it`).
5. **The level** (`level.json`): done. `level::generate(seed, &Options)` is the whole layout, and
   `stage_level` its last stage (the build after the dead-end pass, as a `Level`). Its parts:
   `windows.rs` (`_windows`, `_clear_inside`), `planner.rs` (`_plan`, `_describe`), `walks.rs`
   (`_walks`, `_views`, `_same`) and `report.rs` (`report`, `_is_ring`, `score`). `plan.rs` is now
   `Room`, `Link`, `Contact`, `box_contact` and `Plan` (`room`, `link`, `contacts`, `check`,
   `graph`); `Spot`, `footprint`, `to_carver`, `write_portals` and `write_svg` wait for the stages
   that build and draw. The level's dicts (the plan's rooms, `walks`, `views`, `pools`, `moods`,
   `ceilings`) are lists of pairs in the order the Python fills them, and the test holds them to
   the order the key's file has them in (it reads that itself: serde_json sorts an object's
   names). A walk's point is a `Step` (flags: 1 pass through, 2 squat, 3 both); the report is a
   struct with the dict's keys. `py::round_to` is Python's `round(x, n)` for any float.
   Besides the key, 81 more cases were dumped from the Python the same way and compared, every
   field (seeds 1-60 less the key's, and others with `standard`, `size` 2 and 4, `walkway=2`,
   more windows, drops and grates, dead ends left in, each cover, more risers and vents,
   tighter dead-end limits, no shift): 76 levels and 5 that fail, none differed. To do it again:
   write a key to another folder and run the test with `AUTOLEVEL_KEY_DIR` set to it.
   Worth knowing:
   - A room is named from its kind's list while names last and by its key after ("small11"). The
     Python's `s.key.title()` is never reached: the list it falls back on is never empty.
   - The report's `cover` is the number of cover pieces. The Python's dict has that key twice,
     and the count replaces the option's name in the first one's place.
   - `inside` (in `_walks`) finds a route's door in the room's inside by equality, `to` included.
     An inside kept from before a join was taken back could hold a door at the same place with
     another `to`: the Python stops with a ValueError there, the Rust gives Err. Not seen.
   - `Plan::check` gives its problems in the Python's order, and a failed check's message writes
     the first three as Python writes a list (`py::repr_list`). No level has failed it, so the
     message is only unit-tested. A self link is counted by `graph`, where the Python stops.
   - The report's `centre_offset` uses `powf(0.5)` for Python's `** 0.5` (as `place.rs` does). The
     compiler may make that a square root, which is the same number or its neighbour, and the
     result is rounded to two places.
   - Not as the Python, with the same result: a room's nav graph is worked out once for its
     door-to-door walk, where the Python works it out again for each pair of doors.
   - The walks have not been walked from the Rust level yet: pd-walk needs the polygons.
6. **Polygons** (`autolevel_build/tests/key/polys.json`, `lights.json`): done, in the second crate,
   `autolevel_build`. `csg.rs` is the Carver and the free-form pieces, `sunbeam.rs` the beams,
   `complex.rs` the kit (palette, schemes, `style`, `plan_polys`, `plan_lights`, `lamps`),
   `geometry.rs` the level's own (`restyle`, `beams`, `planned`, `finish`, `polys`).
   `geometry::polys(&level)` is every polygon in the Python's order. The key holds, for the 20
   level cases, each room's polygons as a SHA-512 of their rows at two points (after
   `cx.plan_polys`, and at the end), the first case row by row, and the lamps; every float is
   written as its bits. `python scripts/autolevel_key.py --polys SEED out.json` writes any
   level's rows. Besides the key, 58 more levels (458,785 polygons, 90 beams) were compared the
   same way, and `project_uvs`, `intensity`, `split` and the free-form pieces on random input:
   none differed. Worth knowing:
   - A material is its role in the palette (the Python maps roles to Blender materials, one to
     one). The scheme table is a parameter: the Python swaps `geometry.SCHEMES` into the kit for
     the call.
   - A lit polygon is `Poly::lit = Some(beam)`, where the Python keeps sets of `id()`. If two
     beams ever lit one room the Python's ids could be reused and the two would differ; no
     level does it.
   - `Poly::carved` marks the Carver's polygons, whose normals the Python holds as whole
     numbers: their zeros have no sign, and `project_uvs` negates them as whole numbers.
   - **The key depends on Python 3.12 or later**: `sum()` adds floats with compensation from
     3.12 on, and `sunbeam._area`, `Beam.unit` and `intensity` go through it (`sunbeam::pysum`
     is that sum).
   - `hypot` is exact when one side is 0, which every use here is (walls and rails run along
     an axis). CPython's is correctly rounded and `f64::hypot` need not be, so a rail off the
     axes could differ in the last bit.
   - Faults in the Python, ported as they are or never reached: `split(snap=False)` does
     nothing (`oriented` rounds anyway); `split` gives cut pieces no explicit UVs; `style`
     stops on an unknown scheme where `restyle` and `ramp_tube` fall back.
7. **Meshes and the OBJ**: done. `mesh.rs` is `build.emit`'s rules (a room's vertices shared by
   position in the order met, faces in the polygons' order, collapsed polygons left out) and
   the triangles: ears clipped, shortest diagonal first, never one whose diagonal runs through
   another corner, in whole numbers. Blender cut its own way, so the files can't match byte
   for byte; `python scripts/autolevel_compare.py REF.obj NEW.obj [--light]` compares what must
   agree (groups and their order, each group's triangle count, vertices and area per material,
   material names, the `.mtl` and presets; with `--light`, room medians and shared places).
   `obj.rs` is `export_level` and the dialect's writer. For seeds 40, 1 and 7 the Rust OBJ
   agrees with the Blender build and every walk passes in pd-walk (71, 82, 61), with no
   triangle of no area.
8. **GLB, bake, command line**: see the next section.

Sizes, to judge the work: layout about 4,100 lines of Python, geometry about 1,850, the
Blender side about 900. The layout and the geometry are all in Rust, so a seed makes the same
`Level` and the same polygons as in Python.

## Dropping Blender

Blender did three jobs. Each is now done in Rust, in the second headless crate
(`autolevel_build`), so a game that only wants the layout need not carry the rest.

| Job | Done by | Checked by | Result (seeds 40, 1, 7) |
|---|---|---|---|
| Polygons to room meshes | `mesh.rs`: `build.emit`'s rules, and its own triangles | rooms and triangles equal to the Blender build | equal: 61, 66 and 58 rooms; 15,303, 16,523 and 17,018 triangles; none of no area |
| GE64 OBJ | `obj.rs` | `scripts/autolevel_compare.py`; every walk in pd-walk | agrees; 71, 82 and 61 walks pass |
| GLB | `glb.rs` | `scripts/autolevel_compare_glb.py`; the Khronos validator | "the same"; 0 errors, 0 warnings |
| Lighting bake | `bake.rs`, `bake/trace.rs` | `autolevel_compare.py --light`; pd-walk frames side by side | the same colour at 99.5% of places, within 1 of 255 at all of them; frames can't be told apart |

**The GLB** holds what the Blender export held: a node and a mesh for each room (`ge_room`,
`ge_plan_key` in extras), a primitive for each material, GE coordinates, `1 - v`, COLOR_0 as
linear 16-bit, `KHR_materials_unlit`, CullBoth double-sided, Transparent `BLEND`, the flags and
preset in extras, the textures as PNGs inside. Not as Blender's, and no matter to a reader: nodes
go by room number (Blender's sort starts at `Room_0A`), a texture is shared by materials on the
same image, PNGs carry no metadata. The two Transparent textures (0190, 0748) are 32-bit BMPs
whose fourth byte is the cut-out; the `image` crate drops it, so `glb.rs` reads 24- and 32-bit
BMPs itself, as pd-walk does. The validator has no command of its own on npm: it was run from a
ten-line Node script (`npm i gltf-validator`, then `validator.validateBytes`).
Not yet done: a baked level's GLB opened in the clone.

**The bake** casts each ray once: a corner's colour is `min(255, (ambient + bounce + gain x S) x
tint)` with S the lamps' sum, so the calibration passes (each room's lamps rescaled until its
floor median is within 4 of its target) are sums. It takes 55 ms on 16 threads (370 ms on one;
the result is the same byte for byte), where Blender took 12 s. To come out as Blender's it
does three things Blender's way: the ray caster follows `BVHTree` in single precision (its box
test, and a triangle test that counts its edges: a ray along a floor is stopped by a wall
standing on it, a hit at the ray's very end is none), face normals and floor areas are taken
from the vertices, and a stored byte is read back a hair off as Blender reads it (which decides
whether a floor exactly 4 from its target gets another pass; a test pins the 34 bytes Blender
5.2 reads low, so another maths library fails the test and does not drift). The one difference
left is how a float becomes a byte: Blender uses an approximate power that moves the half-way
points by up to 0.02 of a level, the Rust rounds to the nearest. That is the 0.5% of places
one level apart (with Blender's thresholds put in, all three seeds came out identical).
It keeps light through floor grates, the hard-edged beams (added after the bake, to the lit
faces' corners only), tinted corridors, the red glows at bends and the dim low road.
Checked on default options and three seeds only.

`autolevel_compare.py --light` compares place by place (a room, a vertex, a material and the
way the face points), so the two builds' different triangles do not show. Its first version
weighted by triangle corners and reported gaps of 50 levels that were not there.

Textures are ROM data. The Rust refers to them by preset number and reads them from a
folder it is given (`--textures`, by default `levels/perfectdark/complex`), as the Python
does. Nothing ROM-derived goes into a crate.

### The command line

`target/release/autolevel` (`cargo build --release -p pd_import`: the command line is
`autolevel_build::cli`, the binary `crates/pd_import/src/bin/autolevel.rs`, because walking in
this process and importing need `pd_import`) is `scripts/autolevel.py` with no Python and no
Blender:

```
autolevel sweep --seeds 1-300 --top 8 [--out DIR]
autolevel plan SEED [--out DIR]                 plan.svg, report.json, walks.json, views.json, options.json, recipe.json
autolevel build SEED [--out DIR] [--no-bake]    ... then polygons, bake, LevelIndices.obj, autolevel_SEED.glb, and walk it
autolevel batch --seeds 1-60 --top 4 [--out DIR] [--no-bake] [--shots]
autolevel walk DIR [names...] [-v]              a route named !x must fail
autolevel shots DIR [names...]                  pd-walk frames into DIR/shots
autolevel deadends SEED
autolevel import DIR [--code C] [--name N] [--stagenum N]   a built level as an arena in custom/ (see "In the clone")
```

with the Python's level options as flags, and `--recipe FILE` in place of a seed, `--root` (this
repo: found above the current folder by its `crates/` and `assets/`), `--out` (default
`out/autolevel/seed<N>`), `--textures` (default `assets/textures`), `--pd-walk` or `$PD_WALK`
(walk with pd-walk in place of this process), `--jobs`.
Checked against the Python: `plan 40` writes the same `report.json`, `walks.json`, `views.json`
and `options.json` value for value and a `plan.svg` the same byte for byte; `deadends 40` and
`7` print the same text; `sweep --seeds 1-40 --top 5` prints the same table (seeds 23, 20, 7,
40, 6) in 2.5 s where the Python takes 155 s.
`build 40` takes 0.6 s (plan 0.37, polygons and meshes 0.05, bake 0.05, files 0.09) and then
walks its 71 routes in 1 to 5 s on 16 threads; Blender took about 20 s before the walks.
Built both ways and compared (`autolevel_compare.py --light`, `autolevel_compare_glb.py
--colours`), all agreeing and all walks passing: seeds 40, 1 and 7 at the defaults, seed 1
`--scale standard` (58 walks), seed 9 `--cover pillars --clutter 1.0` (71), seed 1 `--size 2`
(118 rooms, 33,211 triangles, 146 walks).

Not as the Python's: seeds are scored and routes walked on threads (results in seed and route
order); if pd-walk cannot load the level every route fails, the `!` ones too; a batch carries
on past a level that fails; no `plan.png` and no `.blend`; `report.json` writes `4.0` where
the Python wrote `4`.
These figures were measured in the Blender repo, walking with pd-walk as a program (0.13 s a
route to start and load). Here the walks are in this process: see "In the clone".

### What is left of Blender and the Python

Nothing a level needs. The Blender repo's `scripts/autolevel.py` and
`scripts/autolevel_build.py` still work and are what the reference builds in its
`work/autolevel/ref/` were made with. The Python generator stays there as the answer key's
source until the user decides to retire it: a new rule goes into the Rust, and the keys become
a record of what the levels were (re-written from the Rust's own output when a rule changes on
purpose; there is no tool for that yet, because nothing has needed it).

## Track B: the editor

Milestones 1 to 5 are built (1 and 2 on 2026-11-16, 3 to 5 on 2026-11-17; the fifth in `pd_edit`): crate `autolevel_edit`,
on `engine`. See the "As built" sections below; the design it follows comes first.

### What `pd_edit` is (surveyed 2026-11-16)

- **UI:** egui 0.31, reached through `engine::egui`. A left `SidePanel` and a central
  viewport. Tabs and tools are `selectable_value`s.
- **Split:** `lib.rs` holds everything but the window (camera, document, overlay, gizmo,
  jobs, shot). `main.rs` holds the app state, the egui code and `impl engine::app::Game`.
- **Document:** `Doc` holds the data, an undo stack and a redo stack of whole snapshots
  (capped at 500), and a `saved` copy that `dirty()` compares with. Edits are methods on
  `Doc`, not a command type. A drag pushes one snapshot (`begin_drag`) and then changes the
  data live.
- **Saving:** pretty JSON with a `format` field, floats rounded, and the format belongs to
  the crate that reads it (`pd_import`), not to the editor.
- **Rebuilding:** after an edit, a job on a thread re-runs the importer 0.35 s later.
- **Without a window:** `pd_edit --shot` renders to a PNG through the same code. The
  clone's rule is that Claude never drives the window: every visual feature has a
  snapshot path and every behaviour a test.
- **No plan view.** `pd_edit` is a 3D fly-through only. The clone's top-down map is in
  `crates/pd_tools/src/lab.rs`: lists of `Prim` (polygon, line, circle, text) that egui
  draws and a rasteriser turns into a PNG.

### So the new editor is built like this

- A crate `autolevel_edit`: a lib with no window in it, and a thin `main.rs`.
- **The document** is the recipe (below) plus the generated level. `Doc` has `push`,
  `undo`, `redo`, `begin_drag`, `dirty`, `mark_saved`, and one method per edit, as
  `pd_edit`'s does.
- **The recipe format belongs to `autolevel`**, the crate that reads it, with a `format`
  number and versioned `load` and `save`.
- **The plan view is lists of primitives**, as in `pd_tools::lab`, so egui draws them and
  a `--shot` path rasterises the same lists to a PNG. That is also how Claude checks it.
- **Regeneration runs on a thread** after an edit, from the first stage the edit touches.
- **The window is an `engine::app::Game`** (`init`, `tick`, `frame`, `debug_ui`, `render`),
  as `pd_edit`'s is. In the Blender repo it had a small host of its own shaped like that
  trait; that went with the move. The UI is its own module that takes `&mut Ui`, so it can
  become another `Tab` in `pd_edit`.
- **3D:** `pd_render` and `Draw3d` (milestone 5).

### As built (milestones 1 and 2)

- **Lib, with no window and no egui in it** (as `pd_tools::lab` keeps egui out): `doc` (`Doc`:
  the recipe and its level, snapshot undo capped at 500, `dirty`, `mark_saved`, `begin_drag`,
  a method for each edit, `regenerate` here and now and `accept(rev, ..)` for a result from a
  thread; the pick, its candidates, `choose(n)`, `pick_route`, `remove_route`), `jobs` (a level,
  or a pick's candidates, made on a thread), `planview` (the lists of `Prim`, the `View` with
  its storey filter and what is picked, `hit` for what a click lands on, the legend, every
  colour in `planview::colour`), `routes` (a level's routes as rows of text), `headless` (all
  the command line does short of writing files), `raster` (an RGBA image, shapes drawn at twice
  the size and halved, the PNG through `image`), `font` (a built-in 5 x 7 face), `shot` (title,
  plan and legend as one image), `cli`. The dead-end list is `autolevel::deadlist`, and the
  joins that could be made `autolevel::candidates` (both headless, in the layout crate).
- **Bin:** `main.rs` (thin) and `ui.rs` (`Editor::panel` and `Editor::canvas` take `&mut
  egui::Ui`; `Window` is the `engine::app::Game`). The window is the feature `window` (on by
  default, `dep:engine`), so the lib and `--shot` also build with `--no-default-features`,
  with no wgpu, winit or egui in the tree.
- **Command line:** `autolevel_edit --seed 40 [--name value for each option] --shot out.png
  [--storey Z] [--width W] [--list] [--routes]`, `--recipe file.json`, `--save-recipe file.json`,
  `--pick ROOM[@STOREY]` (the joins that could be made from there, drawn and listed; `--all`
  for every storey of every room, not each room's cheapest), `--join N` (make the pick's
  candidate N), `--remove KEY` (take a route out; may be repeated), and `--frames N` (the
  window closes itself after N frames, so it can be smoke-tested). `--shot` needs no window
  and no GPU and takes about a second; a pick about 2.4 s.
- **The plan view shows**, by storey colour with the lowest underneath: rooms with name, key
  and heights; pods and their doors; corridors and stubs with their route keys ("sw": a
  switchback); vents; enclosed and open ramps with uphill arrows; decks, catwalks and pods'
  roofs; pools and bridges; cover; drops, grates, windows and balconies; doors; and the dead
  ends left in the level, the allowed ones ringed green and numbered, the others boxed in red.
  One storey at a time it also draws rails and where drops land. It leaves out glass inside
  pods, the red glows and tints, the walks and the views.
- **The dead-end list** is line for line what `python scripts/autolevel.py deadends SEED`
  prints (seeds 40 and 7 are test fixtures), plus which number on the plan is which dead end.
- **Joining (milestone 2).** A pick is a room and a storey. Its candidates
  (`candidates::candidates(recipe, room, storey)`) are every room within 16 cells and 4
  storeys, on each storey it takes doors, joined by the recipe's own `join` on a copy of the
  network as it stands after routing and the recipe's earlier edits; each is then made in a
  whole level, and only those whose route comes out in place and the same are offered (the
  rest are listed as refused, with why). So **what is previewed is what is made**: choosing
  one appends `{"edit": "join", "room", "storey", "to", "on": [z]}`, the same call on the same
  network (tested on seeds 40, 7, 23 and a vent on 13). They are drawn pale over the plan with
  numbered discs and listed in the same order. In the window: click a room to pick it, hover a
  row to light its corridor, a "join" button on each row.
- **Taking a corridor out (milestone 2).** Click a corridor (or `--remove KEY`). One that is
  the only way to a room is refused, naming the room. One the router made gets a `remove`
  edit. One the dead-end pass added cannot be named by `remove` (it is not there yet when
  `remove` applies), so its dead end is kept instead (`keep`), and the level then shows that
  dead end as too long. A join made by hand is taken out with a `remove` too, which leaves
  later route keys as they were; deleting the join itself is the edit list's "x".
- **Rough:** nobody has looked at the window (it opens, draws and closes under `--frames`, and
  three tests run the panels through egui with no window, with pointer moves, presses and
  releases). With every storey shown, labels overlap where rooms stack: the storey filter is
  the cure. A candidate found before the dead-end pass can lie where the pass now adds a
  corridor of its own (the list says so). A vent cannot be asked for where a corridor fits.
  Each pick routes again and makes up to about 25 whole levels on threads; nothing is kept
  between picks. The window does not draw a taken-out corridor's ghost (the snapshot does).
  Saving writes to the `--recipe` path with no file dialog. Primitives are `(f32, f32)`, not
  glam's `Vec2`. Sample pictures: `out/autolevel/edit/` (scratch; the commands are in M21).

### As built (milestone 3: room boxes by hand, 2026-11-17)

The rules are `autolevel::boxes` (see "What the editor's room tools ask of the recipe"); the
document has `Doc::select`, `lots` (the lots as the recipe places them, held to no rule),
`try_change` (what a change would come to, nothing changed: what a drag shows as it goes),
`change` (one step to undo, or `Err` with the refusal and nothing changed) and `boxes` (what
the plan draws). A refusal is always the generator's own words, e.g. "medium1 and small3
overlap: rooms keep a cell between them, unless one is over the other with a 40-unit slab
between (medium1 is at 48, 41 to 55, 46, from 0 to 512; small3 is at ...)".

- **On the plan** (`View::boxes`, `planview`): the lots over the level, lowest first and a pod
  after its hall, each a faint tint of its storey's colour with a lighter edge, thick at the
  corners. The level underneath may be older than the lots, so a lot the level does not have
  there (`planview::differs`) is filled in, edged in white and says why ("moved", "resized",
  "new", "floor was 276", or "left out" where the generator dropped it), where the level still
  draws it is dimmed with a dashed line to the box, and a room with no lot
  (`planview::deleted`) is crossed out in red ("vast deleted", its pods with it). The selected
  box has a thick edge in the pick colour, eight grips (`GRIP_PX`: 11-pixel squares) and a
  dark panel beside it with its key, size and heights; `Boxes::refused` turns it the dead
  ends' red and puts the reason on the panel, wrapped at 46 letters. With one storey shown, a
  box whose floor is there is drawn as usual, one that is only air there dashed, the rest left
  out (`hit_box` goes by the same rule). `planview::bare` is the boxes on the grid alone, for
  a recipe that makes no level (`shot::render_bare`), so it can be put right by moving them.
- **On the command line** (`cli`, done by `headless::run` through the same `Doc` calls):
  `--rooms` (the snapshot draws the boxes; any flag here does), `--select ROOM`, `--lots` (each
  lot's key, kind, box, heights and pods; needs no level), `--move ROOM@X,Y`,
  `--rect ROOM@X0,Y0,X1,Y1`, `--pin ROOM@FLOOR`, `--delete ROOM`,
  `--add KIND@X0,Y0,X1,Y1@FLOOR@TOP` (its key is the next free `roomN`), and `--try` (the last
  change is tried, not made). Changes are made in the order given, before `--remove` and
  `--pick`, and the level made once after them; each prints `room: ` and the edits it added in
  the edit list's words (`lots::said`). **A refused change** prints `refused: <why>`, stops
  what follows, still writes the snapshot (the box in red with the reason), writes no recipe
  and exits 1 (`Done::refused`). A recipe whose rooms make no level prints `no level: <why>`,
  writes the bare snapshot and exits 1 (`Done::failed`); a lot the generator leaves out is
  named.
- **In the window** (`ui.rs`): a tool switch, "Join" (milestone 2 as it was) and "Rooms"
  (`--rooms` or `--select ROOM` opens with it in hand). Click a box to select it; drag it to
  move it by whole cells; drag a side or corner of the selected one to pull it (never under 2
  cells). While the button is down each new whole-cell position is tried, the box drawn where
  it would lie and a refusal shown on the plan and in red in the panel; letting go makes the
  change (one edit, one undo step) or leaves everything as it was and says why. Escape drops a
  drag; Delete or Backspace deletes the selected room; Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z; a drag
  from bare ground pans. The "Rooms" section: the selected room's key, kind and pods, its box
  as four numbers (x and y move it with its pods, wide and deep resize it), a button per
  storey for its floor (one it cannot go to is greyed, the refusal as its hover text), its
  height, "Delete room", "New room" (kind, floor, top, and "Draw it on the plan": the next
  drag rubber-bands the room), and the list of rooms with pods under their halls. The status
  line says what a change did: the edits gained (or taken out again, where it undid earlier
  ones), what a delete took with it, the warning about route keys. Controls' places are kept
  in `Editor::spots` for tests to aim at.
- **Checked:** 17 tests on the snapshot side, 12 on the window's (egui pointer and key events
  with no window), 9 in `boxes`, 3 in `doc`/`planview`; the pictures in `out/autolevel/edit/`
  (`rooms`, `selected`, `moved`, `try`, `refused`, `added`, `deleted`, `pinned`, `storey276`,
  `bare`, `left_out`) read. A trial takes 2 to 9 ms a room with its eight pin trials.
- **Worth knowing:** seed 25 makes no level ("colossal: no set of ramps joins its storeys")
  and `--delete colossal` cures it: the tests' no-level case. A trial that passes does not
  promise the room survives: moved far off, a room can be left out by the dead-end pass
  (`--move small7@74,54` on seed 40), which the text and the plan then say. Bad options
  (`--walkway 3`) refuse every room change with the option's words.
- **Rough:** nobody has looked at the window. The site's edge is not drawn (`Boxes` has no
  site), so "off the site" has nothing to point at. With every storey shown the highest box
  wins a click, so a hall under a loft is selected from the list or with the storey filter.
  The box's panel is sized for the snapshot's built-in face; egui's lettering may not fit it.
  No edit changes a room's height, and nothing locks a room or re-rolls the others (the old
  plan's "lock, re-roll the rest": not asked for, not built).

### As built (milestone 4: one room's choices, 2026-11-17)

The rules are `autolevel::choices`; the document has `Doc::choices(room)` (what the room has
in the last level made, what the recipe chose, what it could take), `set_choice(edit)` (one
step to undo; the room keeps one edit of a kind however often it is changed; `Err` in plain
words where it can be refused before the level is made) and `clear_choice(room, what)` (the
seed's own again). `Doc::look` hands the plan `View::chosen`: each room with a choice, and
the words ("layout ring, cover none, inside 2, brightness 90, ceiling grille").

- **On the plan:** each such room's words by its name in small violet letters
  (`colour::CHOSEN`), wrapped to the room's width where they lie on no other word and none of
  its pods; where they will not go whole, the one word "chosen". A selected box's panel adds
  `chosen: ...`, and the snapshot's title says for how many rooms something was chosen. A
  layout, a cover or another inside shows in the room itself (decks, ramps, pillars: tested
  that the picture differs with no words drawn). **Brightness and ceiling are for the built
  level** and show on the plan only as words.
- **On the command line:** `--layout ROOM@ring|galleries|mezzanine|none`,
  `--cover ROOM@mixed|pillars|crates|none` (with no `ROOM@` it is still the level's option),
  `--inside ROOM@N` (the `reroll` edit; 0 is the seed's own), `--brightness ROOM@0..255`,
  `--ceiling ROOM@plain|ceiling_tile|ceiling_dark|grille`,
  `--own ROOM@layout|cover|inside|brightness|ceiling|all` (the seed's own again) and
  `--choices ROOM` (`chosen::text`: what the room has, whether the seed or the recipe chose
  it, each layout it could take or why not). Choices are made in the order given, after the
  room changes and before `--remove` and `--pick`, the level made again after each; each
  prints `choice: ` and the edit in the edit list's words. One refused beforehand prints
  `refused: <why>`; one that passes but makes no level prints `no level: <why>` and is taken
  back. Either way nothing after it is done, the snapshot is still written (that room's box
  in red, the choice and the reason on its panel), no recipe is written, and the exit is 1.
- **In the window:** with the Rooms tool and a room selected, "This room's inside" has a row
  for each: layout, cover, inside ("another" for the next re-roll, "back"), brightness (a
  slider that makes its one edit when it is let go, and "the usual") and ceiling. The choice
  in the recipe is lit, what the room has in the level on show is said at the row's end
  ("has: galleries", "has: lanes, 2 pieces"), a layout the room cannot take is greyed with the
  reason as hover text, and a pod's cover and inside rows are greyed with the reason shown
  once. Where a choice makes no level the panel has "undo that" under the red line.
- **Checked:** 10 tests on the snapshot side and 7 on the window's, 3 in `choices` and one in
  `doc`; pictures in `out/autolevel/edit/` (`choice_layout_ring`, `_galleries`, `_mezzanine`,
  `choice_cover`, `choice_inside`, `choice_chosen`, `choice_refused`, `choice_no_level`,
  `choice_selected`) read.
- **Worth knowing:** the real case of a choice only making the level refuses is
  `--seed 40 --layout basin@ring` ("basin cannot be furnished with a ring (basin: no set of
  ramps joins its storeys)"; seed 1's basin too, with a ring and with galleries). Every other
  layout a room's size allows, on every room of seeds 40, 7 and 1, made a level. So "could
  take" means by its size alone. `Choices::cover` is the furnisher's word, which can be
  "lanes" (the pillars that break a long lane), not one of the four a choice may be, and can
  name a cover with no pieces. The level does not record which inside a room was furnished
  from: the number shown is the recipe's. A refusal for `--inside` is worded with the edit's
  name, `reroll`. Galleries on seed 40's colossal has a catwalk along the south short wall
  as well as both long ones (the generator's own doing).
- **Rough:** nobody has looked at the window. The first choice of a kind adds a row to the
  Edits list and shifts the panel one row under the pointer. With a room selected the list
  of rooms is below the fold of a 950-high window. Arrow keys on the focused slider make one
  edit a press. "has:" speaks of the level on show, which is the old one until the new is made.

### As built (the editor in `pd_edit`, 2026-11-17)

The window's panels moved into the lib (`autolevel_edit::ui`, behind the `window` feature;
`--no-default-features` still builds the lib and `--shot` with no engine), and `pd_edit` hosts
them: **"Generate…"** beside "Open…" and "New from glTF…" puts the generator's `Editor` in
place of the fly-through view (its panel under a short form: the arena's name, code and stage
number; its plan in the middle), and **"Make it an arena"** writes the level's recipe with the
generator's in it (`pd_edit::generator::Form::write`: what `autolevel import` writes) and
imports it; the level then opens in `pd_edit` as any other, for its items, waypoints and
doors, and "Play". A generated level has **"Generator…"** beside its tabs, which reads the
generator's recipe back out of the level's (`generator::of`) to shape it again. The name and
code follow the seed until one is typed; a code that is another kind of level's is refused;
a level made again keeps its stage number and its layout file (said in the panel: items
placed by hand may then be out of place). Nothing else is written: no build folder, no glTF.
Checked: two tests of `pd_edit::generator` (written as the importer reads it and read back;
each problem's words), and by hand on the command line the same path end to end (a recipe
with a room moved, a ring chosen and a cover taken away: built, 70 of 70 walks, imported,
and a view of it drawn by the game with the ring in it). **Nobody has looked at the window**,
and `pd_edit`'s own window code has no test that reaches the hosted panels. It is a mode of
the window more than a tab: the level is shaped here (on the plan and, since milestone 5, in
3D), and its items are placed once it has been made an arena.

### As built (milestone 5: the level in 3D, and walked in place, 2026-11-17)

In `pd_edit`'s generator mode a strip over the middle of the window switches it: **Plan | 3D**.
"3D" is the level the recipe makes, drawn by the game's renderer, with `pd_edit`'s fly camera;
"Walk" puts PD's player in it.

- **The preview** (`pd_edit::preview::Preview`): the geometry step of the import alone
  (`pd_import::autowalk::geometry_only`, which `SimWalk` now also uses: the generator's rooms as
  PD's, its openings as portals, Complex's materials on the pool's textures, the bake, the tiles
  by the importer's rules), with no waypoints, placement, check match or layout, loaded as the
  game loads a stage. It is no arena: it is written under the system's temporary directory
  (`pd_edit_preview/<pid>-<n>/stages/autolevelpreview/`), laid over `assets/` as `custom/` is,
  and deleted when the preview is dropped; `custom/` and the levels folder are never written
  (tested). Seed 40 takes 0.6 s on a thread (the level made again and baked 0.45 s, written and
  loaded 0.12 s) and 0.02 s to load onto the GPU; `--size 4` (251 rooms) 2.9 s.
- **Following the edits** (`pd_edit::space::Space::follow`): a preview is made whenever the
  recipe is not the one on show, started with the generator's own remake, so the view is current
  about 0.6 s after an edit. The camera stays where it was. While it is made the last level
  stays ("bringing the 3D view up to the plan... (the last level stays)"); where the recipe
  makes no level it stays too, with the generator's reason. The window keeps a second renderer
  for it, so the open level's BG is left alone.
- **Walking** (`preview::Walker`: `pd_sim::player::Player` alone on the preview's tiles, as
  `autowalk`'s): "Walk" or G drops it where the camera is, onto the floor beneath; WASD or the
  arrows (left and right turn), the right button looks at the game's mouse rate, C or Ctrl
  crouches a level down and Space up (squatting fits a vent), Escape flies again from its eyes.
  A level made again under it keeps it where the floor is as it was, stands it on a floor that
  moved, puts it on the nearest floor within 15 m where its own is gone (and says how far), or
  ends the walk and says so.
- **Plan and 3D together:** the camera (or the walker) is a pink arrowhead on the plan; a right
  click on the plan sends the camera there and shows the 3D view; "Go to <room>" puts it over
  the room in hand in the generator's panel.
- **Without a window:** `pd_edit --shot-generated OUT.png [--seed N | --recipe FILE]
  [--at x,y,z,yaw,pitch | --view NAME | --room KEY] [--walk x,y,z,theta | --walk NAME
  [--forward TICKS] [--crouch]] [--plan] [--list]`: the 3D view from a camera (a view by the
  name `autolevel shots` gives it), a walker's view after walking, or the plan with the camera
  marked. Pictures: `out/autolevel/edit3d/` (scratch).
- **Checked:** 13 tests in `pd_edit`'s lib with no GPU (the preview and what it leaves on disk;
  a walker on a floor, down a corridor to its wall, held by a catwalk's rail, stopped at a
  vent's mouth standing and through it squatting; a preview after an edit and the walker put
  back; the view's bar and keys run through egui; the mark and the right click on the plan; the
  command line), and the pictures read against `autolevel shots`' of the imported arena.
- **Rough:** nobody has looked at the window, and `main.rs`'s part (keys and mouse into the
  camera and the walker, the cursor capture, the BG loaded into the second renderer, the draw)
  has no test. The view draws every room at full baked brightness with no portal pass, as
  `pd_edit`'s fly-through does, so halls are brighter than in the game; no objects, sky or HUD.
  The level is made twice an edit (the editor's and the preview's). The mouse turns a walker at
  PD's rate, a quarter of the fly camera's (`Walker.p.mouse_sens`). A walker put back is not
  checked for standing room. The generator's own keys (Delete on a selected room) still act
  with the 3D view on show. There is no key for Plan | 3D.

### Editor milestones

1. Plan view of a generated level, with `--shot`. The dead-end list beside it. **Done.**
2. Join two places: pick a room and a storey, see the generated corridors that could join
   them, choose one. Remove a corridor. **Done.**
3. Room boxes by hand: move, resize, add, delete, pin a floor, with the overlap rule's
   refusals in plain words. **Done** (2026-11-17).
4. Per-room choices (layout, cover, brightness, ceiling) and re-rolling one room's inside.
   **Done** (2026-11-17).
5. The 3D view and walking the level in place. **Done** (2026-11-17), in `pd_edit`.

## The recipe

A level is recorded as what made it, not as its geometry:

```
{ "format": 1, "seed": 40, "options": { ... }, "edits": [ ... ] }
```

Edits name things the way the generator already does (room keys, storeys, route keys) and
each belongs to the stage it is applied after:

| Edit | Applied after |
|---|---|
| move, resize, add or delete a room; pin a room's floor | placing the lots |
| join a room on a storey to another; remove a route | routing |
| a room's layout, cover, brightness, ceiling; re-roll a room's inside | furnishing |
| keep or cut a dead end by hand | the dead-end pass |

Re-running a recipe gives the same level. The dead-end pass already records what it did in
this shape (`lv.fixes`: joined, cut, dropped), which is the model for the edit list.

**As built** (`autolevel/src/recipe.rs`, format 1): `recipe::load(text)`, `recipe::save(&Recipe)`
(pretty JSON, options that are the default left out, so a fresh recipe is five lines) and
`Recipe::generate()`, which is `furnished()` (the stages through furnishing, with their edits)
then `finish()`; the editor stops between the two to list the dead ends as the rooms fall.
Options go by the Python's names (`recipe::OPTIONS`, `set_option`, `option`). An unknown format,
field, option or edit is an error that says which. With no edits a recipe gives exactly
`level::generate(seed, &options)`.

```
{ "format": 1, "seed": 40, "options": { "cover": "pillars", "dead_corridor": 6 },
  "edits": [ { "edit": "join", "room": "tower", "storey": 551, "to": "colossal" },
             { "edit": "keep", "room": "spire1" },
             { "edit": "brightness", "room": "vast", "value": 128 } ] }
```

| Edit | Applied | What it does, and when it is refused |
|---|---|---|
| `move` (a room's low corner to a cell) | after placing the lots | Its size kept; a hall's pods go with it. |
| `resize` (cells wide and deep, from its low corner) | lots | A hall's pods stay where they are; a spire's spiral is laid out again with the same start wall and turn. |
| `add` (a key, a kind, a rect, a floor and a top) | lots | A lot the scatter did not make, after the scattered ones. The kind (one of the scale's) decides its dress and name only. The key is new, a lower-case word, and not shaped like a route's (`c07`, `v02`). |
| `delete` | lots | The lot goes, and its pods with it. Any later edit that names it is refused. |
| `pin` (a room's floor on a storey) | lots | Its height kept; a hall's pods go the same number of storeys. |
| `join` (a room on a storey to another, optionally `"on": [storeys]`) | after routing | On the dead-end pass's terms: a corridor, or a vent if none fits. The editor's candidates are this edit with `on` given. |
| `remove` (a route, by any of its pieces' keys) | routing | Refused if the level would fall in two. It cannot name a corridor the dead-end pass adds later: `keep` stops one. Keys shift when lots change. |
| `cover` (one of `level::COVER`) | furnishing | In place of the dress's own cover; the room's shape and pillar stay. Refused for a pod. |
| `layout` ("ring", "galleries", "mezzanine", "none") | furnishing | A hall's first upper storey, on every attempt. Refused for a room of one storey, a tower or spire, one too small for it (a ring 8 by 8 cells, galleries 7 by 8, a mezzanine 5 by 10), and when no attempt can be furnished with it: it never falls back to another. "none" keeps a balcony the room would have had anyway. |
| `reroll` (n) | furnishing | The room is dressed and furnished from other streams of random numbers (`{seed}/{key}/reroll{n}/dress`, `.../{attempt}`); 0 is the seed's own. A spire's spiral is not re-rolled (it is laid before routing). |
| `keep` (the dead end a room is in) | the dead-end pass | The pass leaves it alone (`Build::keep`). |
| `cut` (a room's own decks stop at a storey) | after the pass | Refused if the room has a door higher up (`Build::cut`). |
| `brightness`, `ceiling` | the finished level | They replace the planner's own choice. |

**Which lots may overlap** (`place::Placed::check`, held to after all the lot edits, not after
each). Every lot is on the site, at least 2 by 2 cells, with its floor on a storey
(`storeys::LEVELS`), a storey or more tall and its top at or under 1890. A spire must take a
spiral (both walls at least 8 cells, two storeys or more). Every pair that is not a hall and
its pod has a cell between them along x or along y, or one is over the other with a 40-unit
slab between (`scatter::clear`, `apart`). A pod is one storey tall, on one of its hall's first
two upper storeys, reaches at least 2 cells into the hall and takes at most a fifth of its
floor (`scatter::pod_misfit`). These are the scatter's own rules: its `place()` now calls the
same functions, and the seeds' own lots pass the check on 116 levels. A refusal names the
lots and the rule in plain words, for the editor to show.

What the plain reading left open, and what was chosen:
- **Whose pod a lot is** is worked out again from where the lots are after every edit: a small
  or tiny lot that overlaps a hall and is not clear of it in height is that hall's pod. So a
  pod moved or pinned clear of its hall becomes a room of its own, and a small room moved onto
  a hall becomes its pod.
- **No floor changes without a `pin`.** A lot that lands in a hall on a storey that takes no
  pod is refused, naming the storeys that do; it is not lifted there as the scatter would.
- **A pod with no way in** is left out silently by the generator. In an edited recipe that is
  a refusal if a lot edit named the pod or the seed's own level keeps it.
- Nothing is settled or compacted again: the lots stay where the person put them.

**How a choice reaches the furnisher.** `Build::choices` (room -> `Choice {cover, layout,
reroll}`), set through `Build::choose` before the first `refresh`, and part of the key of the
cache of insides, so the dead-end pass's furnishing again keeps it. With no choice a room is
furnished exactly as before (the key).

What changed in the layout code for the recipe, with no seed's level changed (the key still
passes): `Options::check()`; in `build.rs` `Build::keep`, `kept()`, `cut()`, `choices`,
`choose()` and `takes()`; `place::Placed`; `level::stage_rooms` (`stage_spaces` is `lots` then
that); the scatter's test as functions (`clear`, `beside`, `hosts`, `pod_levels`,
`pod_misfit`); `Setup::fixed` (a hand-laid first upper storey is not given the hall's own
ring); `Recipe::routed()` (the first two stages, where the editor's candidates stop). `serde`
and `serde_json` are dependencies of `autolevel`.

Checked (2026-11-16): each edit on real seeds with each refusal's message, 105 edits over
seeds 40, 7 and 1 (90 made a level, 15 were refused: 12 overlaps, a shallow pod, galleries a
basin could not be furnished with, a layout for a one-storey room; none hit a generator
failure), and those 90 levels with twelve named ones built and walked with pd-walk in the
Blender repo: 7,231 walks, all as expected. Here, a recipe with all eight new edits builds and
walks 71 of 71 in this process.

Worth knowing: the scatter's slab is 40 and the gaps from 512 to 551 and from 1063 to 1102 are
39, so by the rule a room cannot go straight over one whose top is 512 or 1063 (kept as the
scatter has it; relaxing it would be a rule change). The check is at the end, so the message
names the lots, not the edit that did it. A spire that cannot take a spiral is refused, not
furnished as a plain tall room.

### What the editor's room tools ask of the recipe (2026-11-17)

Two modules of `autolevel`, new, with nothing in the Python behind them, and no seed's level
changed by them (the keys pass, and `scripts/autolevel_key.py` leaves them byte for byte).

**The first stage alone.** `Recipe::rooms()` is the lots as placed and the rooms they make,
with every refusal `routed()` gives before it routes (the overlap rule, the site, a spire's
walls, a pod's place and its way in, a later edit that names a deleted room), and no corridor
looked for, so it is quick enough to ask on every move of the pointer (nine trials, each with
its scatter, in 0.01 s). `routed()` is now that, then the network. Its parts, for a caller
that tries one change after another: `scattered()` (the seed's own lots, once for a seed and
its siting), `lots_on(base)` (the lot edits made on them, **held to no rule**, so lots that
break one can still be drawn where they are) and `rooms_on(base)`.

**Room boxes by hand: `boxes.rs`** (editor milestone 3). A `Change` is what a person does on
the plan: `Move` (a hall's pods go with it), `Rect` (the box becomes this rectangle; a hall's
pods stay), `Pin`, `Add`, `Delete`. `trial(recipe, change, base)` says what it would come to
without making it: `Trial { recipe, lots, refused, dropped, warn }`: the recipe with the
change in it as the recipe's own lot edits, the lots as they would lie (whether or not they
keep the rules), and the refusal in plain words. `drag(lot, part, by)` is the change a drag
of a box's body, side or corner (`Part`) is; `new_key` ("room1", "room2", ...), `kinds` and
`tops` are what a "new room" form offers. What it settles that the edits leave open:
- **The recipe stays short.** A change that undoes an earlier one takes that edit out: a room
  moved twice has one `move`, one dragged back to where the seed put it has none, and a room
  made by hand and then moved, sized or pinned has only its `add`, with where it now is. This
  is done by trying, never by rule: an edit is left out only if the lots come out exactly the
  same without it.
- **A side pulled leaves a hall's pods where they are.** The recipe's `move` takes them along,
  so pulling a hall's west or south side is a `move`, a `resize` and a `move` back for each pod.
- **A deleted room takes with it every later edit that names it** or one of its pods (a join
  to it, its cover), listed in `Trial::dropped`; a room made by hand goes as if never added.
- **`Trial::warn`**: the recipe has a `remove` (a corridor named by its key), and keys are
  dealt out again when rooms change. Nothing is done about it but the warning.
- No edit changes an existing room's height (only `add` gives one): delete it and add another.

**One room's choices: `choices.rs`** (editor milestone 4). `of(recipe, level, room)` reads what
a room has (its layout, its cover and how many pieces, its brightness and ceiling where set),
what the recipe chose for it, and each layout it could take with the reason it could not.
`refusal(level, edit)` is what can be refused before the level is made (a pod's cover or
inside, a layout the room can't take, a value out of range), in the words making the level
would use (tested: the two agree). `set(recipe, edit)` makes a choice the room's one edit of
its kind, in the earlier one's place, so the list does not grow as a choice is changed again
(the generator goes by the last of a kind anyway); a `reroll` of 0 is no edit; `clear` gives
the seed's own back. A layout no attempt can be furnished with is only found by making the
level, and comes back as that level's error.

## In the clone

Moved on 2026-11-16. What changed with the move, and what is here that was not there:

- **The workspace.** The three crates are members (not default members: build with `-p`).
  `autolevel` and `autolevel_build` are headless and in `tools/check_boundaries.py`'s list;
  `pd_import` depends on `autolevel_build`, and `autolevel_edit` on `engine`. Versions were
  the clone's already. Conventions as the clone's: every file opens with a `//!` paragraph,
  errors are `Result<_, String>`, names are short.
- **Textures** come from the game's pool by default (`assets/textures/<nnnn>.png`).
  `autolevel_build::textures::dumps` writes the ones a level draws beside it as the Setup
  Editor's dumps (`tempImgEd<NNNN>.bmp`, 32-bit; the pool's image is the dump's upside down,
  and within 7 of 255 of it, 15 on 0745), which is what the OBJ names and the GLB embeds. A
  folder of dumps given with `--textures` is used as it is. Against the Blender build of seed
  40 the GLB differs only in two images being RGBA where the dumps of 073C and 0745 are RGB.
- **Walking in this process.** `walk::Walker` is now opened on one built level and then given
  routes from any thread. `pd_import::autowalk::SimWalk::open(dir, assets, res)` runs the
  importer's geometry step on the level's GLB (the same recipe `autolevel import` writes) into
  a scratch folder under `dir`, loads the `Stage` and `TileLevel` the game would, and deletes
  the folder; `walk` is pd-walk's `walk_route` rule for rule on `pd_sim::player::Player`.
  So a walk passes on the very tiles and rooms the game gets. Opening seed 40 takes 0.21 s
  and its 71 routes under 0.05 s on 16 threads (about 0.1 s on one); pd-walk took 1.0 s (9.2 s
  on one). `--pd-walk PATH` or `$PD_WALK` walks with pd-walk instead, and `shots` needs it
  (this walker has no renderer: its error names `pd_snapshot ... stage CODE --at x,y,z,theta`).
  Checked: seeds 40, 1 and 7, `1 --scale standard`, `9 --cover pillars --clutter 1.0` and
  `1 --size 2` (71, 82, 61, 58, 71 and 146 routes) each walk clean, with the same outcome on
  every route as pd-walk.
- **What the importer lacked** (found by walking; each is a `GltfSource` option, off unless a
  recipe names it, so every other glTF level imports as before):

  | Option | Rule | Without it | Seed 40 |
  |---|---|---|---|
  | `grates` | a translucent triangle facing up is a floor, seen and shot through | the player falls through a corridor's grate (a route fails in five of the six levels) | 4 triangles |
  | `low_ceilings` | a ceiling under 172 over the floor beneath it is a wall, as Complex's crawl space's | a player stands up through a vent's roof | 38 of 2,222 ceiling triangles |
  | `ramp_walls` | an upright wall triangle whose top or bottom slopes is `GEOFLAG_RAMPWALL` | the wall under an open ramp blocks over its whole box | 724 triangles |

  No route fails without the last two: unit tests and a stand-up check in a vent cover them.
  The player here ducks to 114 and fits a 118 vent ducked; pd-walk's head is 172 and only a
  squat fits. Standing is refused by both.
- **The ground probe** (`pd_sim::stage::collision`, `cd_find_ground_at_cyl_ctfril`): PD keeps
  the first 20 floor tiles; of more than 20 this now keeps the 20 nearest below the probe
  (`// SUBST`, with a test). No route failed for want of it, but a sweep of every floor tile's
  middle and corners found the ground wrong at 2 to 136 spots a level (seed 1: standing on a
  deck at 1654, the ground was 0), and none after. **It also changes Car Park**: 60 of 684
  probes there now find the floor under the player where the first 20 gave none, or one a
  storey down (the candidates are every room's tiles, which PD's own room list would not have
  held). The user has been told and has not yet said whether to keep it for PD's arenas.
- **As an arena: a source of its own** (2026-11-17). `autolevel import out/autolevel/seed40`
  writes `crates/pd_import/levels/autolevel40.json` with the generator's recipe in it
  (`"source": {"game": "autolevel", "recipe": {"format": 1, "seed": 40, ...}}`; group
  "Autolevel", stage 124 unless `--stagenum`: 124 to 127 are the free ones, and a second level
  on a number drops the first from the menu) and imports it. `pd_import::autolevel` makes the
  level again from that recipe in this process (`autolevel_build::built::make`: a level is the
  same every time, half a second) and reads no file of it, so that one JSON remakes the arena
  on any machine (`pd_import autolevel40`):
  - **rooms**: one of PD's for each of the plan's (seed 40: 61), every triangle and tile in the
    room whose air it faces, nothing cut. A room's vertex colours are indexed in 16 bits (65,536
    a room; the biggest hall of seeds 1-60 has 18,528), so no room is divided; `write_bg`
    refuses one that would not fit.
  - **portals**: one for each opening (`autolevel_build::openings`: every face two rooms' air
    shares; seed 40: 91), wound and ordered as `bg_init_portal` keeps them. PD cannot take a
    portal whose two rooms both have their boxes' middles on its far side; the room behind
    then gets a wider box (no seed of 1-60 needs it: unit-tested only).
  - **materials**: Complex's own rows (`assets/stages/ref/bg.json`), found by texture, culling
    and layer, on the pool's textures: no image is written, PD's mip-mapped combiner draws it,
    ClampT clamps, and shots and feet sound as on Complex.
  - **tiles**: the three rules always on, and a vent's floor flagged `GEOFLAG_AIBOTCROUCH`.

  What PD does with rooms is now what it does on its own arenas: the portal pass (under 40
  rooms a view, of PD's 60), brightness by room, no respawn in a room an opponent is in, a
  hill is a room, simulants gather waypoints by room. Seed 40: 15,303 BG triangles (23,440
  after the k-d cut), 13,119 tiles (16,244), import 8.5 s. `autolevel import DIR --gltf` is
  the old route, kept; a test walks seed 40 both ways with the same outcome on every route.
  Walked clean on the new source: seeds 40, 1, 7 (71, 82, 61 routes), `1 --size 4` (251 rooms,
  270 routes), `9 --cover pillars --clutter 1.0`, `1 --scale standard`.
  **Two things a player will meet, undecided** (the user has been told):
  - *The halls are about 18% dimmer than the glTF import drew them.* With real portals PD's
    lighting settles a hall at 210 of 255 and a small room off it at up to 255, exactly as
    Complex's own rooms do; the glTF import had 255 everywhere. Left PD's way, which means a
    step in brightness at some doorways. The other way is to scale each room's colours by 255
    over its settled brightness.
  - *Simulants meet less often.* Over ten 5-minute matches on seed 40, kills fell from 69
    (glTF) to 28 and target-in-sight from about 70% to about 45%. The same drop shows without
    the waypoint repair, so it comes from the rooms: most likely PD's respawn rule now that a
    hall is one room (read in the code, not isolated by experiment). Six simulants are thin
    on a level 82 by 63 m.
- **Simulants** (`pd_import::autonav`). **The "ledge falls" of the check match were never
  falls**: they are simulants running down ramps (in the air from a ramp's top to half-way
  down, twice a ramp; Complex on PD's own waypoints has 18 a minute), and the check line now
  says how many are hops down ramps. What was wrong was the waypoints. The maker
  (`pd_sim::nav::gen`) leaves out the foot of an open ramp over a floor (where the ramp is
  under 30 cm over the floor it takes the two for one surface and keeps no sample), finds a
  drop through a hole only by luck, and puts nothing in a vent. Vents are cured by the crouch
  flag. The rest is repaired from what the generator knows: every ramp end to end and every
  leg of every walk is tried with the maker's own probe and linked where the graph does not
  join its ends (one way for a drop); then the waypoints outside the largest part (a pool's
  water, a tank's top, a pod's roof no deck reaches) are left out. Seeds 40, 1, 7: 880 of
  975, 990 of 1,011 and 971 of 1,021 in the largest part before; 959, 1,012 and 1,004 after,
  each all one part (3, 1 and 2 ways linked; 34, 15 and 32 left out). In four 5-minute
  matches on seed 40 simulants went through the vents 105 times and the low road 33.
- **Views drawn by the game** (2026-11-17). `autolevel shots DIR [names...] [--code C]` writes
  the level's views (`views.json`: a door and a deck of each big room, the drops, grates,
  vents, windows) as a list and has `pd_snapshot stage CODE --views FILE.json` (new: many
  named views on one load of the stage; `pd_tools`) draw them into `DIR/shots/<name>.png`.
  The level must be imported first, and the pictures are of the arena as last imported. Seed
  40: 39 views in 0.8 s. With `--pd-walk` it is pd-walk's frames of the OBJ, as before.
- **Not done:** the sampler's own fault in `nav::gen` (the higher of two floors within 30 cm
  should be the one kept: left, since it would move other levels' graphs and pinned tests), a
  BSP order for a room's translucent layer (a hall's rails and glass are one layer; no
  artefact seen in 47 views), `cli::build` handing its level to the walker (which makes it
  again: 0.6 s to open where the glTF took 0.2), `--size 4` imported (it was walked); pd-walk (`viewer/`) stays in the Blender repo as the
  cross-check.

## Felicity

`--style felicity` (the option `style`, written to a recipe only when it is not `complex`) makes
a level of Felicity's kind. It is new code beside Complex's: its own planner
(`autolevel::felicity`, two random streams `<seed>/felicity` and `<seed>/felicity/doors`), its own
kit (`autolevel_build::felicity::KIT`, arena `mp11`), and the doors the importer stands in its
doorways. Complex's seeds are untouched (seed 40's whole build is byte for byte the same).

```
autolevel build 7 --style felicity --out out/autolevel/felicity7     # plan, polygons, bake, walks
autolevel import out/autolevel/felicity7 --code felicity7 --stagenum 127   # the arena, with its doors
```

**What Felicity is (measured 2026-11-17 from `assets/stages/mp11`, Complex `ref` and G5 `cryp`
beside it; rooms joined by wide openings merged into "spaces"):** 20 spaces, one of them a
corridor (Complex: 33, ten corridors). A building cut into rooms off a wide hallway and off each
other, beside a big three-bay hall 7.8 m tall with a raised platform. Storey 283 (rooms 259, slab
24), stairs of 12 risers (23.6) 40 deep, no ramps. Doorways 106 x 212 (212 wide for a double),
with a lintel, in walls 40 thick. 19 doors, all one model (`dd_dr_nonref`, 106 x 212 x 13): 12
swinging, 7 sliding; 15 of 29 doorways have one. Where: 9 of 11 doorways from the hallway or the
hall into a room, 4 of 12 between two rooms, none into a tiny room or the corridor.

**Two families** (2026-11-17, the user after playing version 2: "very uniform and compact ...
more sprawling and random, different sized rooms, branching, not so much of a grid like an
office building ... big open rooms and very tall rooms"). A seed picks its family once (`SPRAWL`,
0.7) and keeps it through its redraws; seeds 1-40: 26 sprawl, 14 block.

- **Sprawl** (`felicity::sprawl`): grown from the hall, piece by piece: a corridor (3 to 5
  cells wide, 6 to 18 long) or a **big open room** (kind `Open`, a hub: 7-14 x 7-12 cells, one to
  three storeys tall, up to 8.3 m; four pillars when 9 cells or more each way; one at least) off a
  wall of a piece already there, at its end (a run or a bend) or its side (a branch), the newest
  likeliest so arms reach out. The first two corridors come off the hall's own walls. Pieces
  that end up wall to wall are joined (0.8: loops), a corridor's end opening all but a cell each
  side (an archway up to three cells: doors only in doorways up to two). The gallery goes over
  a long corridor with its stair rooms beside it; then rooms of every size along every wall of
  the circulation (3-8 cells, now and then 9-13 by 7-11; 15% of the big ones two storeys tall),
  with stretches of bare wall left, so the outline is ragged; a washroom cluster once; up to
  three rooms behind rooms. Nothing past the hall's far wall or beside its platform. Seeds 1-40:
  1 to 4 open rooms, 26 to 32 lots; the first draw meets every circulation target, and fails
  outright (no room for the stairs beside the gallery) on about a third of seeds, which redraw.
  Open rooms take doors as the hall does (0.5) and get second ways in as corridors do.
- **Block**: version 2, below.

**What the planner makes** (version 2, 2026-11-17: circulation first; version 1 cut a building
into rooms off one hallway and off each other, and played as a maze). On a 106 grid, drawn one
way round and then turned and mirrored by the seed (eight ways):

- **Two long corridors** side by side (3 or 4 cells wide, 15 to 30 long, 6 to 9 cells apart),
  both ending on **the hall** (12-16 x 14-19 cells, 780 tall; a platform 70 up along its far
  wall, three steps up its middle third; six pillars until the screens, below) by a doorway each (a double one where the
  corridor is 4 wide). **Cross corridors** join the two, drawn one in three each: one at their
  far ends (a ring), one 5 to 9 cells along (an H; one long corridor may then start at it, a
  bend), or one at the far ends and one in the middle (a ladder). A corridor meets another
  through a doorway in its end wall, with no door. Corridor 1 is `office`, corridor 2 `marble`,
  the last cross corridor `industrial`: three looks to tell where you are.
- **The gallery**: an upper corridor 14 to 20 cells long over corridor 2, and a **stair room**
  towards each end of it. A stair room is sized for its stairs, the two of a level different
  shapes: beside the gallery on its outer side (a straight flight 9-10 x 3-4 cells, both
  doorways in the one wall; an L 6-7 square; a U 4 x 7-8), in the band between the long
  corridors (the same, or a flight straight across: its foot on corridor 1, its top on the
  gallery), or, on an H where corridor 2 is 4 wide, a U in line with corridor 2 where it stops
  (`END`). The stairs are fitted by the first of the eight frames, in a seeded order, that
  leaves the foot a doorway onto a ground corridor and the top one onto the gallery; one of
  each, with no doors.
- **Rooms** in a row along the outer side of each long corridor (4-8 cells wide, 5-7 deep), in
  the band between them (as deep as the band; behind a stair room that doesn't fill it, a room
  off corridor 1), along both sides of the gallery (5-9 wide), and often one or two off each
  side wall of the hall, clear of its platform. One room 6 or more wide becomes **the
  washrooms**: two or three rooms 3-4 x 3 together. Up to three **rooms behind rooms** (3-6 x
  3-4, two doorways deep; nothing is deeper). The footprint is not a rectangle: nothing is
  built where no room is.
- **Doorways**: every room off the corridor (or hall) it was cut along, one in five double.
  Then second ways in: onto any other corridor (0.6) or the hall (0.7) a room is beside, so the
  rooms between the corridors are mostly short cuts; then, until at most 20-36% of rooms (a
  seeded share) have one way in and the level has its loops, from a room with one way in onto
  circulation or into a neighbour (a pair of such rooms first; washrooms only into each other).
- **Types** by where a room is: `washroom` the cluster, `hallside` a room sharing three cells
  of wall with the hall, `industrial` a room beside the industrial cross corridor, `marble` a
  room of 40 cells or more, `office` the rest; a room behind a room is its front room's; a
  stair room is the corridor's at its foot. Seeds 1-40: 357 office, 142 marble, 105 hallside,
  83 washroom, 57 industrial.
- **Doors** by Felicity's shares: 0.8 from circulation into a room, 0.33 between rooms, never
  into a room under 12 cells, none between corridors or onto stairs, 0.5 on the hall's own
  doorways; two leaves in a double doorway; swinging 0.63. Seeds 1-40: 655 of 790, 80 of 232,
  0 of 86, 0 of 278, 39 of 80.

19 to 32 rooms a seed (seeds 1-60), about 26, 7 or 8 of them circulation. A layout that misses
`circulation::TARGETS` (or has no room for its stairs) is drawn again from the same stream, up
to 40 times: of seeds 1-60 one is (seed 5, its stairs), none for a target. Walks: every doorway
crossed, every room and corridor door to door (every pair), each flight from the doorway at its
foot to the one off its landing, the platform; 121 to 228 a seed, all passing on seeds 1-40.

**The kit:** the procgen theme's roles (`themes.py` `_fel`), each found by its texture in
Felicity's Setup Editor export: floor 0287 (blue marble), lower band 024F to 141, upper band
0256 to 259, plain 024B above (a tall room's walls), the marble wainscot 0872 (a quarter of the
rooms, the hallway and the hall), ceiling 024B, treads 0872 and risers 0256, jambs 0264 (every
face inside a doorway). Brightness targets 137 / 85 / 50. The polygons are Complex's carver
(`complex::plan_polys`) with Felicity's style over every face.

**The doors** (`pd_import::autolevel::doors`): where a level's layout names no doors, each
doorway's door is a copy of Felicity's own (`doors::adopt` of `mp11`'s setup: its swinging and
sliding rows, speeds and close times), on the doorway's middle plane (the portal) and stretched
to the opening; a double doorway's leaves are a sibling ring, hinged at their outer edges. A
layout's doors (the editor's) replace them. Seed 7: 18 doors, all closing their portals.

**Room types** (2026-11-17, a room's `scheme`; `autolevel_build::felicity::TYPES`): measured from
Felicity's own triangles (`PD_PROBE_STAGE=mp11 PD_PROBE_OUT=tris.json cargo test -p pd_import
--release probe_stage_triangles -- --ignored`: every BG triangle with its room, texture and UVs;
units per repeat from each triangle's UV gradient, which agrees with the procgen theme to 0.1).
`office` (tan bands, timber-beam ceiling 01EC, its v repeat 424 so the 212 lattice never reaches
the clamp), `marble`, `industrial` (grey 0274 / 0872, plate floor and ceiling 070F), `washroom`
(tiles 071B / 071D, mirrored tile floor 0271), `hall` (0276 / 027D / 0288 / 0281 bands) and
`hallside` (0273). The planner gives them by where a room is (above); the hall is `hall`.

**Stairs** (`autolevel::stairs`): `fit(shape, air, floor, rise, frame)` lays a straight flight
(132 wide against a wall, a gallery at the top), an L (two half flights round a corner landing)
or a U (a switchback filling a room 240 to 400 wide) in a stair room, by one of eight frames,
and gives the steps and landings as solids, railings on every edge that drops into the well
(Felicity's see-through rail 0710; sloped ones down the flights go in `Level::ramp_rails`), the
stretches of wall a doorway may open in at the foot and at the top, and the path up. The planner
sizes each stair room for its shape and takes the first frame, in a seeded order, whose foot
opens on a ground corridor and top on the gallery (above). Seeds 1-40: 29 L, 28 U, 23 straight;
every walk passes.

**Walkways** (`autolevel::balcony`, 2026-11-17, the handoff brief's first item): every tall
room (an open room two or three storeys tall, and the hall) gets a balcony a storey up (283)
along one wall: a slab 24 thick and 160 out from the wall (tag `walkway`, its top a floor, its
underside open), railed along its edge (Felicity's rail) but where a flight meets it, and across
its end where something standing in the room stops it short (a pillar, the hall's platform). It is
reached by a straight flight of twelve risers square to it, rising towards its wall and arriving
at its edge, solid to the floor as the stairs are: against the wall at the walkway's end if that
leaves every ground doorway's mouth (120 in) and every solid clear, else out in the room (railed
both sides), half a cell at a time along the walkway from a place drawn. In a room three storeys
tall a second walkway runs along the opposite wall two storeys up (566), wall to wall, reached by a
flight up the far end wall from the first walkway's end that stands on nothing (each step a block
48 deep, so its underside is stepped), with a landing along that wall where the flight's top falls
short. The way round, the flight's place: their own stream, `<seed>/felicity/balconies`, so no
room moves. A walkway passes over ground doorways (its underside at 259, door tops at 212). The
kit dresses its top as treads, its edge as risers, its underside (and a floating flight's) as the
plain ceiling panel 024B, and starts the wall strips again at a walkway's top on the wall behind it
(a hall's walls are otherwise one tall storey); the bake hangs lamps under it (`plan_lights`, as
under Complex's catwalks). The walk `walkway_<room>` starts at the flight's foot, climbs, goes to
both ends of the first walkway and, with a second, climbs on and walks it end to end. Seeds 1-40:
60 of 74 tall rooms had one, 11 of them two, while the hall stood on pillars (a short hall's pillar
rows left no 132-wide gap for the flight); with the screens (below) all 68 do, 10 of them two. `Balcony::upper` names the walls behind the walkways,
where a doorway on an upper storey opens onto one (a landing's, below). Tall plain rooms (15% of
the big rooms off the circulation) get none: their door-to-door walks are straight lines.

**Upstairs areas** (2026-11-17, the brief's second item: "more than one upstairs area, with
stairs into the tall rooms ... each reached two ways"). Sprawling layouts only (the block keeps its
one gallery):

- **A second gallery** (0.6, drawn once the first is placed, and only with lots to spare): over
  another long corridor, laid as the first is (its own two stair rooms near its ends, rooms along
  both sides), `gallery2`. A gallery's stair rooms now open at the top onto their own gallery
  (`Draft::stairs` names it; `stair_ways` filters), so neither borrows the other's.
- **Landings** (0.7 a tall room, two at most): an upper corridor `landingN`, 5 to 12 cells along
  a wall of a tall room (an open room two or three storeys tall, or the hall's side walls) and 3
  or 4 out from it, a storey up over whatever stands on the ground there. Its two ways: a doorway
  onto the tall room's walkway (`upper`: the only doorways joining two storeys, never found by
  `could`, only planned), and a stair room off its far side or an end down to a corridor on the
  ground, checked to fit before it is kept (`stair_fits`: some way round puts its foot on the
  ground corridor and its top on the landing). Rooms hang off its other walls. The tall room must
  be 8 cells deep away from that wall, and `balconies` then takes only the ways round that put the
  walkway along it and over the doorway; a room that gets none fails the layout, which is drawn
  again (`TRIES`).

Seeds 1-40: 9 second galleries and 8 landings (seeds 5, 8, 15, 17, 32, 34 and 40; seed 5 has two;
seed 15 a second gallery and a landing); every upper corridor has two ways or more (test
`every_upstairs_area_is_reached_two_ways`), and every walk passes. Seed 10 has neither.

**Arches and the hall's bays** (2026-11-17, the brief's third item; the user chose each option:
cross walls, circulation only, round then segmental). Felicity measured first (its triangles,
`probe_stage_triangles`): its hall's arches are round-headed, about 580 wide and 640 tall, a
semicircle springing about 360 up on straight jambs, soffits in the hall's band 0288, in deep walls
between bays 778 tall; its doorways are not arched (106 x 212 under a lintel, with small 45-degree
chamfered trims). So:

- **The hall stands in three bays**: two screens across it, 80 thick (`felicity::screens`), the
  second just short of the platform's steps, the first halfway to it, so the last bay is the
  platform's. Each screen is three round-headed arches on piers a cell wide: one beside each side
  wall, 400 wide and springing at 400 (a walkway along the side wall passes under it, its rail top
  at 369), and a wide one between on a semicircle as high as the hall allows (springing at 400, or
  lower so its crown stays 60 under the ceiling); a beam over each. A screen moves (20 at a time,
  200 at most) to keep 40 clear of any doorway a storey up in a side wall (a landing's), or is left
  out. The six pillars are gone. A walkway's flight keeps out of a screen's footprint, and
  `balcony::fit` ignores what stands wholly higher than `balcony::HEAD` (200) over a walkway (the
  beams). Seeds 1-40: every hall has both screens.
- **Arched doorways**: every doorless doorway between two rooms of the circulation (corridors, the
  hall, open rooms, stair rooms; 420 on seeds 1-40) gets an arch inside its own 212 box, so the
  doorway contract and the portals are unchanged: a semicircle on a one-cell doorway (springing at
  159), a segment of a circle rising a quarter of the width on wider ones (159 on two cells, 133 on
  three). Rooms keep Felicity's rectangular doorways, door or none.
- **The piece** (`level::Arch`, its `curve`; drawn by `autolevel_build::felicity`'s `arch`): the
  carver still cuts the opening as a box; then every face of its across the opening wholly
  between the springing and the top comes out (the lintel's underside, the jambs above the
  springing; a jamb rising from below is cut down to the springing: no new split lines), and the
  arch puts in the soffit along the curve (6 to 16 pieces, about 40 each, half in each room) and,
  on each face of the wall, the two corners above the curve, cut at the wall's strip lines so each
  piece takes the wall's strip there. A doorway's soffit is jamb (0264), a screen's the hall's band
  (0288). Then `fix_t_junctions` again.

**Circulation targets** (`autolevel::circulation`, the user's call 2026-11-17: "more like
corridors with rooms as offshoots, not a complex web of rooms nested deep"): the corridors,
halls and stair rooms (kinds `corridor`, `hub`, `stair`) and how the other rooms hang off them.
`TARGETS`: 18-34 spaces, 4 or more circulation rooms that reach each other without passing
through a room, 75% of rooms right off circulation, none deeper than 2, at most 40% with one
way in, loops at least 0.2 a space. Felicity itself measures 65% / 4 deep, Complex 84% / 2. The
test `felicity_meets_the_circulation_targets` prints every seed's measures (`-- --nocapture`).
The first planner missed on all 60 seeds (64% right off circulation on average, rooms up to 5
deep). The second meets them on all 60 (the brief it was built to: docs/handoff/felicity-planner.md):
rooms right off circulation 92% on average (least 81%, seed 41), none deeper than 2, rooms with
one way in 25% (most 33%, seed 22), loops 0.45 a space (least 0.32, seed 12), 7 or 8 circulation
rooms in one piece with 8 or 10 joins between them, circulation 52% of the floor (the hall is
most of that).

**Simulant stalls** (`PD_PROBE_STAGE=<code> [PD_PROBE_SECS=300] cargo test -p pd_import
--release probe_stalls -- --ignored --nocapture`: a match of six Normal simulants, 60 s unless
told, each 3-s stall with where it is, the waypoint made for, and the nearest door's state; it
reads `out/autolevel/<code>/recipe.json`, so build there what is imported). Found (the brief's
fourth item, 2026-11-17): the waypoint maker samples floors on a 50 cm lattice, so at a doorway it
leaves a waypoint anywhere in the mouth, a few cm off the wall's face beside the jamb (legal: 34
clear of the jamb's corner, 23 needed); a go-to skips ahead, so a chr coming along the wall makes
for it straight and meets the jamb. **The fix** (`pd_import::autonav::doorways`, run before
`repair` on a generated level with doorways; Complex's have none and are given back unchanged,
custom imports and PD's arenas never come here): every waypoint of the maker's in a doorway's mouth
(within its wall or 70 of either face, across its opening) comes out, and the doorway gets two on
its axis, never skipped (`walkdirect`), 150 out each side (100 or 70 where the floor stops sooner),
linked through it; the removed waypoints' neighbours are linked to the one on their side and each
new one is tied to the graph, every link by the maker's probe. None stands in the opening: a first
try put one there and on 100-out points, and chrs made to reach a door's plane, or waiting inside a
swinging leaf's sweep, held doors shut (Felicity 9: a door 12% open for minutes). A doorway the
probe can't walk straight through both ways (a stair's foot) is left as it was. Seeds 1-12, five
minutes each: 161 stalls before, 48 after (seed 10: 31 to 2; stalls away from doors 86 to 14).
**Left**: 34 of the 48 are two doors (seeds 9 and 11) held part open by chrs pressing on them from
both sides. PD's own door logic: an opening door stops dead at a chr in its sweep unless it has
`DOORFLAG_UNBLOCKABLEOPEN`, which Felicity's doors (copied into ours) don't. Not changed: it is the
doors, not the waypoints, and a departure from PD for the user to decide. **The user (2026-11-17):
leave it, simulants get stuck in PD's own levels too.**

**Ledge falls** (the import check's count of drops over 100; `probe_stalls` now prints each, from
where to where, and runs on PD's arenas too: `PD_PROBE_STAGE=mp11`, rooms by number): not ledges.
Simulants running down stairs leave the treads and land half a flight on (141 or 142, about 236
along; 118 on a walkway's flight). Felicity 15, five minutes: 54, every one on a flight. PD's own
Felicity (`mp11`, PD's graph), five minutes: 33, the same half-flight drops in its stair rooms (and
a few true drops of 577 from its room 14 into 16). It is PD's movement on stairs; nothing to fix.
(Felicity's risers are vertical floor tiles flagged `GEOFLAG_STEP`; ours are wall tiles. Not
tried: whether flagging ours so changes anything; PD's own drops show it doesn't stop these.)

**Not yet:** windows, light strips; no outdoor part; the editor can't make or
edit a Felicity level (a Felicity recipe takes no edits). Of the layout: the hall always closes
one end, the block family's upper storey is one gallery, a walkway's only doorways are a
landing's, and nobody has played one.

## Risks

- **A missed order dependence** shows up as a key mismatch deep in furnish. The cure is in
  "Where Python and Rust differ".
- **Float differences.** The layout uses floats only in costs (halves), sort keys and
  `uniform` draws that are rounded straight away. The polygon stage uses more, and may
  need the key compared with a tiny tolerance, or the Python changed to whole units.
- **The bake's look.** There is no exact key. Agree the tolerance and look at frames.
- **Two generators.** The Rust now does everything the Python did, and one thing better (the
  dead-end measure, above). Do not let both drift: new rules go into the Rust only, and the
  user decides when the Python is retired.
- **Two repos.** The keys are written by a script in the Blender repo into this one. A change
  here that moves a key must be seen as a diff under `tests/key/` and never be made by hand.
- **The bake follows Blender's arithmetic** (single-precision rays, a byte read back through
  `powf`). On another machine's maths library its test of the read-back fails; that is the
  signal, not a fault to tune away.
- **Simulants on these levels** route everywhere a player walks (all waypoints one part), but
  nobody has watched a match: six are thin on 82 x 63 m, and PD does not respawn into a hall an
  opponent is in.
