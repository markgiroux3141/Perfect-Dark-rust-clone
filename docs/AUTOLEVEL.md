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
| As an arena | new | `autolevel import`: `pd_import::autolevel` (a source of its own: the level made again from its recipe, its rooms and openings as PD's rooms and portals, Complex's materials on the pool's textures) and `pd_import::autonav` (the generator's ways put into the waypoints) and `pd_import::autoplace` (spawns, weapons and ammo by the generator's floors, 2026-11-20) | tests in `pd_import` (rooms, portals, textures, tiles, sight, the repair); `pd_snapshot stage autolevel40`; `autolevel shots` | done (2026-11-17); nobody has played one |
| Kits (what a level is dressed in) | new | `autolevel_build/kit.rs` (`Kit`: palette, schemes, solid tags, tints, brightness targets, the arena whose material rows the import uses); Complex is `complex::KIT`, passed to `geometry`, `mesh`, `bake`, `obj`, `glb`, `Built` and `pd_import::autolevel` | the keys; seed 40's build byte for byte the same as before | Complex only (2026-11-17). A level's shape (storeys, doors, ramps, the module) is still the planner's and Complex's; `built::make` always takes `complex::KIT` until a recipe names its kit |
| Felicity-style levels | new (not a port) | `autolevel/felicity.rs` (the planner), `autolevel_build/felicity.rs` (the kit and its polygons), `pd_import::autolevel::doors` | `felicity` unit tests (40 seeds: plan check, the same twice, one piece, door shares, stairs, types; 60 seeds: the circulation targets); `autolevel build N --style felicity` walks every route of seeds 1-40 | second planner (2026-11-17): corridors first, rooms as offshoots; nobody has played one. See "Felicity" |
| Sculpted levels | new (not a port) | `autolevel/sculpt/` (`vox`, `shape`, `layout`, `nav`, `repair`, `mod`), `autolevel_build/sculpt.rs` (Complex's kit under its own name) | `sculpt` unit tests (the grid, a room the same twice and one piece, a platform dropped from and not climbed, ramps and a switchback joining floors, a corridor climbing between two rooms, seeds 1-4 plan-checked); `autolevel build N --style sculpt` walks | first version (2026-11-18): rooms sculpted from boxes, repaired with ramps and catwalks; seeds 1 and 2 imported (`sculpt1`, `sculpt2`, stages 101 and 102); nobody has played one. See "Sculpt" |
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
a record of what the levels were, re-written from the Rust's own output when a rule changes on
purpose: `AUTOLEVEL_KEY_REWRITE=1 cargo test --release -p autolevel -p autolevel_build --test
key` (`crates/autolevel/tests/keyfile/`, which both key tests share). It puts in only the values
the Rust no longer gives, in the file's own text, so every other byte and the order of every
object's names stay as the Python wrote them; run the tests again without it to see them pass,
and look at the diff under `tests/key/`. Since a key was re-written, `autolevel_key.py` no
longer writes it byte for byte; it still writes every case a later rule did not move.

**Rules of the Rust's own** (each moved the keys when it came in, but floods and shapes):

- **Shaped rooms are on by default** (2026-11-19, the user's call): `Options::shaped` is 0.1
  (`level::SHAPED`), in rooms big enough (see "Shaped rooms"). As for floods, the keys did not move:
  both key tests read their cases with `shaped: 0`, and every test that pins a seed's level uses
  `Options::python`.

- **Pillars stop at the first storey, and a tall room takes fewer** (2026-11-19, the user's call,
  after `shaped1`'s hall stood "a forest" of grey columns floor to ceiling through four storeys).
  Every pillar (the colonnade's, the lane breakers') stands to its storey's ceiling (236 over the
  floor), or to the first storey's deck over it, never higher (`cover::reach`: only that deck can
  stand in its way). And in a room of more than one storey the colonnade lays one row for each 14
  cells of width (7 before) and its pillars half as far again apart (`cover::colonnade`). Rooms of
  one storey are as they were. It moved 19 of the 20 levels of `interiors.json`, `level.json` and
  `polys.json`, 17 of `deadends.json` and one of `lights.json` (re-written; the level key's dicts
  can now be re-written whole, in their own order, `keyfile::take_text`), and seed 40's pinned
  counts: 15,303 triangles to 14,995, 13,119 tiles to 12,850 (13,507 to 13,238 with a ring), the
  plan's 371 rectangles to 363. Seeds 1-20 at the defaults: the same levels, every walk passes.

- **A pillar with a clear top has a ladder up it** (2026-11-19, the user's call). A pillar that
  stops at the first storey's ceiling with 200 units or more clear over it (no deck, pod's roof
  or block, or angled catwalk) gets a ladder up one side (`cover::lay_ladders`, `furnish::Ladder`):
  a side drawn from the room's own stream `{seed}/{key}/ladders` among those with standing room
  a cell deep in front. It is the cellar's ladder (the kit's "ladder", Complex's grille, flagged
  `GEOFLAG_LADDER` by the importer; a glTF material whose `extras.pd` is "ladder" is too), a cell
  wide from the floor to the pillar's top (`geometry::pillar_ladder`; at first between two stiles
  of "plain", which the user found not needed, 2026-11-20). Players
  climb it, and so do simulants: the importer's waypoints link a ladder's foot to its top
  (`pd_sim::nav::gen`), so a top has waypoints of its own, dropped down from. Each has a walk and a view (`pillar_<room>_<n>`),
  and the report counts them (`ladders`). Nothing else in a level moves; seed 40 (by
  `Options::python`) has 5: 10 triangles and 10 tiles more (15,005, 12,860; 13,248 with a ring),
  5 walks (76) and views (44) more, and a 19th material. Seeds 1-20, grand and towering: 2 to 17
  a level (none in towering 17), every walk passes, and every one fails from the pillar's far side.

- **Sunken floors are on by default** (2026-11-19, the user's call): `Options::sunken` is 0.1 (a
  room's floor a cell lower in a rectangle, a floor ramp down into it, no railing; "Sunken
  floors"). `Options::python` leaves them out, so no key moved.

- **Flooded rooms are on by default** (2026-11-19, the user's call): `Options::flooded` is 0.1
  (`level::FLOODED`), so a seed's level now has a flooded room where one falls (see "Flooded
  rooms"). The keys did not move: they are the Python's levels, which have none, and both key tests
  read their cases with `flooded: 0`.

- **Railings are Complex's metal railing** (2026-11-19, the user's call). The Python hung 0x748,
  a blue perforated mesh, on every catwalk edge and down every open ramp side; Complex has that
  mesh on one catwalk only, and its railings elsewhere are 0x190 (the frame with posts the
  generator already used for its glass strips, "window"). The kit's `rail` is now 0x190, one
  repeat to `complex::RAIL_REPEAT` (88) units along, as the glass strips; the walkable floor grates
  keep the mesh as a role of their own, `mesh` (the palette's 20th). Moved `polys.json` only (a
  railing's and a grate's material, a railing's scale and UVs); `verycomplex` keeps the original's
  own mesh catwalk. Every Complex-kit arena was imported again.
- **No deck under a ramp** (2026-11-18, the user's call). A big hall's open ramp could lie
  across the catwalk on the storey it rises from: `ramp_options` keeps a ramp off the deck it
  climbs to, but only scores against the deck it starts on. The plan knew nobody could stand
  there (`stand` takes a ramp's cells out of its own storey), but the Python built the deck on
  underneath, so a catwalk ran into a ramp's underside, or lay under its whole run with a wedge
  of no headroom below its high end (`autolevel1x4`, seed 1 `--size 4`: colossal3's catwalk
  under its ramp 1). `Interior::clear_under_ramps` (`furnish/mend.rs`) cuts that deck away once
  the ramps are placed, so where they go and every random draw before are as they were. A cut
  can leave a catwalk ending at the ramp (a strip a catwalk wide with nothing beside it; a ring
  or landing the ramp only clips keeps its rail), and each such dead end is mended, in order:
  **run round the ramp** (`way_round`: the fewest new cells, a catwalk wide, through cells free
  for deck, to any other deck within two ramp runs: usually its foot landing, so the catwalk
  leads onto the ramp, or the deck past its far side or end); else **taken back** (`take_back`:
  a straight spur, up to two ramp runs long, back to where it joins other deck or to whatever
  needs it, a door's or pod's way in or a ramp's landing); else left with its rail. Across
  seeds 1-200 at the defaults, `--size 2` (1-40), `--size 4` (1-10), `--walkway 2` and
  `--scale standard` (1-60), no finished level has a catwalk dead-ending at a ramp, and the
  same seeds fail as before. And `Plan::check` (so every level, Complex and Felicity) refuses a
  walkable top that an open or floor ramp passes over or under with less than `HEADROOM` (200)
  between them. It changed 45 of the 182 levels seeds 1-200 make at the defaults (21 of 38 at
  `--size 2`, 7 of 9 at `--size 4`), only in their decks, rails and what follows (lamps,
  polygons, `catwalk_cells`); 9 of the key's 20 levels, re-written; `autolevel7` and
  `autolevel1x4` imported again. Seeds 1 and 40 are the same as before.

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
- **Worth knowing:** seed 25 with `--no-rescue` makes no level ("colossal: no set of ramps
  joins its storeys") and `--delete colossal` cures it: the tests' no-level case. (By default the
  rescue makes it a level: "Rescue", below.) A trial that passes does not
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

### As built (the options' controls, 2026-11-17)

The Options section offered a text field per option. Each now has a control
(`autolevel_edit::controls`: headless, its lists the generator's own `STYLES`, `COVER` and
scales): a list for `style`, `scale`, `cover` and `walkway`; a switch for `washroom`, `crates`,
`shapes` and `dead_ends`; a slider over a range worth trying for the numbers (the ranges are the
editor's alone: a recipe's value outside one is shown at the end, not changed); and for `site`
"the scale's own: W x H" or, ticked off, two sliders (made one's own at the size it had, so the
level stays as it was). A slider makes one edit when it is let go, as the brightness slider
does; "reset" beside an option not at its default puts it back; a label's hover text says what
it does. A Felicity level reads only `washroom`, so the rest are greyed (`controls::read_by`)
and a Complex level greys `washroom`. Checked: 5 tests in `controls` (every word offered is
taken, every default lies in its range, a slider's float steps come back as the number) and 2
in the window's (a slider held is no edit and let go is one, reset, a switch, a word from a
list, Felicity's greyed sliders; the site made one's own and back). Nobody has looked at it.

### Editor milestones

1. Plan view of a generated level, with `--shot`. The dead-end list beside it. **Done.**
2. Join two places: pick a room and a storey, see the generated corridors that could join
   them, choose one. Remove a corridor. **Done.**
3. Room boxes by hand: move, resize, add, delete, pin a floor, with the overlap rule's
   refusals in plain words. **Done** (2026-11-17).
4. Per-room choices (layout, cover, brightness, ceiling) and re-rolling one room's inside.
   **Done** (2026-11-17).
5. The 3D view and walking the level in place. **Done** (2026-11-17), in `pd_edit`.
6. Blank levels: an empty start, contacts in place of refusals, alternatives, the cascade shown,
   and a side view for heights. **Designed** (2026-11-18); its step 1, the empty start, **done**
   the same day; step 2, the side view and the `height` edit, **done** 2026-11-23: see "Blank
   levels: your edit wins".
7. The rest of the editing roadmap (per-room features, the 3D gizmo, per-part re-rolls, single
   elements): see "Editing generated levels: the roadmap" (2026-11-23).

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
| `height` (a room's floor and its top) | lots | New (2026-11-23): the side view's edit, and the only one that changes a seeded room's height. The top is a ceiling line, as `add`'s is; a hall's pods go as many storeys as its floor. Refused under a storey tall, over the highest ceiling, for a pod left with no storey, or by the overlap rule. |
| `join` (a room on a storey to another, optionally `"on": [storeys]`) | after routing | On the dead-end pass's terms: a corridor, or a vent if none fits. The editor's candidates are this edit with `on` given. New (2026-11-23): `"door"` and `"to_door"` (`{"side": "E", "at": 26}`: the one doorway each room may take, the cell along its wall of the door's low end) and `"via"` (`"corridor"` or `"vent"`); refused where nothing fits through them, or for a doorway off the wall. |
| `remove` (a route, by any of its pieces' keys) | routing | Refused if the level would fall in two. It cannot name a corridor the dead-end pass adds later: `keep` stops one. Keys shift when lots change. |
| `cover` (one of `level::COVER`) | furnishing | In place of the dress's own cover; the room's shape and pillar stay. Refused for a pod. |
| `layout` ("ring", "galleries", "mezzanine", "none") | furnishing | A hall's first upper storey, on every attempt. Refused for a room of one storey, a tower or spire, one too small for it (a ring 8 by 8 cells, galleries 7 by 8, a mezzanine 5 by 10), and when no attempt can be furnished with it: it never falls back to another. "none" keeps a balcony the room would have had anyway. |
| `reroll` (n) | furnishing | The room is dressed and furnished from other streams of random numbers (`{seed}/{key}/reroll{n}/dress`, `.../{attempt}`); 0 is the seed's own. A spire's spiral is not re-rolled (it is laid before routing). New (2026-11-23): `"part"` (one of `level::PARTS`: catwalks, ramps, cover, windows): only that part from a stream of its own, the rest as it was ("Re-rolls side by side", below); one of each part a room, left out of the file when not given. |
| `keep` (the dead end a room is in) | the dead-end pass | The pass leaves it alone (`Build::keep`). |
| `cut` (a room's own decks stop at a storey) | after the pass | Refused if the room has a door higher up (`Build::cut`). |
| `brightness`, `ceiling` | the finished level | They replace the planner's own choice. |
| `window` (a room, a storey, a cell of the window's wall, `as` glass, rail or none) | the finished level (once the windows are chosen) | New (2026-11-23). The window there is made glass or rail, or taken out; a place the level left without one is given one, past the level's limits. Left out with why (`Level::unmet`) where no window fits, or for a rail on the room's own floor; refused only for an `as` that is none of them. |
| `piece` (a room, a cell, `as` pillar, crate or none, or `to` a cell) | furnishing (once the room's cover is laid) | New (2026-11-23). A pillar or crate stood with its low corner on the cell (`size` cells square; none: a pillar the room's colonnade's width, a crate a cell), in place of what stood there; the one over the cell taken out, or moved by its low corner to `to`. Left out with why where it does not fit (off the floor, by a door, a ramp's foot, water or a drop, on another piece, under a deck's edge, cutting a door off). Refused without one of `as` and `to`. |
| `drop` (a room, a cell or none) | furnishing (as the openings are chosen) | New (2026-11-23). The room's drop (two cells square, its low corner on the cell, into the room under it) in place of the generator's, or none. Left out with why on the generator's own terms (a cell in from both rooms' walls, clear of doors, nothing between, at most four storeys, a floor to land on). A cellar's hatch stays. |
| `feature` (a room, one of `level::FEATURES`, on or off) | the rooms (shaped, flooded, sunken, cellar) or furnishing (platforms, holes, angled) | New (2026-11-23). In place of the level's chance for that room alone: on is a chance of 1, off of 0, drawn from the room's own stream as ever. A room the feature does not fit is left without it (the room's choices say so); refused only for an unknown feature. |

**Which lots may overlap** (`place::Placed::check`, held to after all the lot edits, not after
each). Every lot of the seed's own is on the site (a room drawn by hand, an `add`, may be
anywhere, past any edge and at negative cells: the user's call, 2026-11-24, so a level drawn by
hand is as big as it is drawn; `Placed::drawn`. The router sizes its grid to the rooms, and the
import and the 3D preview take negative cells: checked on an empty start from -16 to 88 cells),
every lot at least 2 by 2 cells, with its floor on a storey
(`storeys::LEVELS`), a storey or more tall and its top at or under 4370 (1890 before the atrium's storeys). A spire must take a
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
  "Autolevel"; since M22 a custom level has no stage number of its own: the game numbers them
  as it starts, in `custom/levels.json`'s order, so there is no `--stagenum`) and imports it. `pd_import::autolevel` makes the
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
the clamp), `marble`, `industrial` (Felicity's grey concrete bays, its rooms 26-32: panels 071B /
071D with blue lines, floor 0271, rock ceiling 0281), `washroom` (Felicity's washroom, room 12:
black tile 0274 to 141, marble 0872 above, white tile floor and ceiling 070F), `hall` (0276 / 027D
/ 0288 / 0281 bands) and `hallside` (0273). The planner gives them by where a room is (above); the
hall is `hall`. **Until 2026-11-17 `industrial` and `washroom` wore each other's materials** (the
washroom's tiles were on the industrial rooms): found by looking at Felicity itself, since swapped
(the user's call). Looking at Felicity: `pd_snapshot <dir> stage mp11 --at X,Y,Z,THETA` (or
`--views file.json`, many at once) takes the triangle dump's coordinates as they are (an earlier
brief said x was negated: it isn't), feet on a real floor (under a raised floor the view is of
somewhere else), theta 0 facing +z, 90 facing -x.

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
where a doorway on an upper storey opens onto one (a landing's, below). **Tall plain rooms** (15% of
the big rooms off the circulation, two storeys) get one too (2026-11-17, the brief's item 1): their
door-to-door walks are straight lines between points `INSIDE` in from each doorway, so the flight's
footprint keeps `LINE` (60) off every such line as well as off the doorways' mouths. Seeds 1-40: 6
of 16 tall plain rooms (the rest are under 8 cells deep, or the lines leave no place); test
`tall_plain_rooms_have_walkways_clear_of_their_walks`.
`walkway` is the fitting for one room (`balconies` runs it with the frames shuffled by its stream;
`onto_walkways` with none drawn, to ask whether one fits).

**Upstairs areas** (2026-11-17, the brief's second item: "more than one upstairs area, with
stairs into the tall rooms ... each reached two ways"). Second galleries in sprawling layouts only;
landings in both (the block's since the next brief's item 1, below):

- **A second gallery** (0.6, drawn once the first is placed, and only with lots to spare): over
  another long corridor, laid as the first is (its own two stair rooms near its ends, rooms along
  both sides), `gallery2`. A gallery's stair rooms now open at the top onto their own gallery
  (`Draft::stairs` names it; `stair_ways` filters), so neither borrows the other's.
- **Landings** (0.7 a tall room, two at most): an upper corridor `landingN`, 5 to 12 cells along
  a wall of a tall room (an open room two or three storeys tall, or the hall's side walls) and 3
  or 4 out from it, a storey up over whatever stands on the ground there, and short enough to leave
  three cells of that wall free at one end where the wall is 8 or more (a room there stands along
  the tall room too). Its two ways: a doorway onto the tall room's walkway (`upper`: the only
  doorways joining two storeys, never found by `could`, only planned), and a stair room off its far
  side or an end down to a corridor on the ground **or into the tall room itself** (2026-11-17: most
  stair rooms off a landing stand beside the tall room and nothing else; `Draft::stairs` names the
  room the foot may open on), checked to fit before it is kept (`stair_fits`: some way round puts
  its foot on the ground and its top on the landing). Rooms hang off its other walls. The tall room
  must be 8 cells deep away from that wall, and `balconies` then takes only the ways round that put
  the walkway along it and over the doorway; a room that gets none fails the layout, which is drawn
  again (`TRIES`).
- **The block's landing** (`BLOCK_LANDING` 0.9; the hall is its only tall room): `landing` (the
  sprawl's code, as a function taking the family's "may build here" rule and a reach along the
  wall) on one of the hall's side walls, everything beside the hall (x from its near wall), the
  landing three cells short of the stretch clear of the platform. Its stair room, off the landing's
  far end and flush with the hall's wall (`landing` tries that place as well as the one drawn), is
  mostly a U: foot and top at one end, so the foot opens into the hall right by the landing and
  the room runs on beside the platform with no doorway there. `stair_ways` never puts a foot
  doorway into the hall where its mouth meets the platform or its steps (`fixtures`, the hall's
  far wall known: east in the draft, `far` after the turn). The hall's side rooms keep off the
  stair room. Seeds 1-40: 7 of the 14 block layouts have one.
- **The galleries' rooms are hung last** of the upper storey, after the landings and the rooms
  along tall rooms (below): the lots (`MOST`) used to run out before a landing could take its rooms.

Seeds 1-40: 8 second galleries and 28 landings (8 before the stair foot could open into the tall
room, 7 of them the block's); every upper corridor has two ways or more (test `every_upstairs_area_is_reached_two_ways`),
and every walk passes.

**Upper rooms onto walkways** (2026-11-17, the handoff brief's item 1; the user chose "more upper
rooms by tall rooms" over a walkway turning the hall's corner): an upper room beside a tall room
opens onto its walkway as a second way in (`onto_walkways`, `ONTO` 0.8, after the "one way in"
doorways so every ground doorway of the tall room is known). All of a tall room's are on one wall,
the walkway's: a landing's wall if it has one, else one drawn; the hall's side walls only (on its
near wall a flight would cross the first screen). Each is kept only if a walkway still fits holding
every doorway onto it: `walkway` (the fitting `balconies` uses, tried every way round with nothing
drawn), so the layout never fails for one. To make such rooms: a room off an upper corridor that
meets something is shortened to fit (3 cells at least) instead of dropped, and each tall room
(`ALONG` 0.8) takes a room planned along its wall a storey up that shares three cells with an upper
corridor (a landing's free end, mostly). Seeds 1-40: 8 upper rooms open onto a walkway, 2 of them
onto the hall's (2 before; 14 before the block's landings moved the draws); test `upper_rooms_beside_a_tall_room_open_onto_its_walkway`. Layouts
drawn again on seeds 1-60: 94 (79 before), most for too many rooms with one way in.

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

**Built round the hall** (2026-11-17, the handoff brief's item 1; the user chose "build round
it"): the hall need not close an end of the building.

- **The rule**: the hall's lot knows its far wall (`Lot::far`, east in the draft, turned with the
  lots), and `shared` gives no wall to share there, nor along the side walls' last five cells (the
  platform's four and one). So nothing anywhere (`could`, `gap`, `stair_ways`) opens a doorway
  through the far wall or onto the platform; `usable` is how much of a lot's wall a doorway may
  take, for the growth and the rows of rooms.
- **Sprawl** (`ROUND` 0.5): the zones kept clear past the far wall and beside the platform are
  lifted, and before the growth a corridor is laid along one side wall of the hall from five
  cells or so before the platform's stretch to 4-9 cells past the far wall, a doorway onto the
  hall where the wall is free, with arms to grow from at its far end and its outer side (the two
  pieces off the hall's own walls still come first, off its near wall and its other side).
- **Block** (`ROUND` 0.5): `corridor3` along a side wall of the hall from its near wall to 6-10
  cells past its far wall, 3-4 wide, a doorway onto the hall; two rooms at most off its outer side
  and a row of rooms past the far wall, behind the hall. The hall's side rooms skip that side; a
  front that meets something already there (a landing's stair room) is left out.
- The platform walk starts at a corridor's doorway in the near wall (the round corridor's is in a
  side wall, where the walkway's flight may stand).

Seeds 1-40: 15 layouts go on past the hall's far wall, 3 of them blocks (test
`the_building_often_goes_on_past_the_hall`, which also checks no doorway is in the far wall).
Layouts drawn again on seeds 1-60: 129 (79 before this brief's item 1), most for too many rooms
with one way in (the round corridor's rooms, the landings' rooms); every seed finds one well within
`TRIES`.

**Doorway frames** (2026-11-17, the handoff brief's item 2). Felicity measured first (its
triangles, `probe_stage_triangles`; rooms 24 and 29, texture 00EF): each face of a doorway's wall
is chamfered at 45 degrees, 13 deep and 13 wide, round both jambs and the lintel, mitred at the
top corners, so at the face the opening is 132 x 225 and 13 in it is 106 x 212. So every doorway
with no arch (rooms' doorways, door or none, and the walkway doorways a storey up) gets the same
(`autolevel_build::felicity`, `TRIM`): `clear_frame` cuts the carver's faces (the wall's faces on
each side are cut on the frame's edges and the pieces inside it dropped; the jambs and the lintel's
underside are drawn in to start 13 into the wall) and `frame` puts in six chamfers (role `trim`,
00EF) and a floor triangle at each jamb's foot in that room's floor; then `fix_t_junctions` as
ever. A door (13 thick on the middle plane) stands in the 14 left between the bevels. Test: the
kit's `a_felicity_level_is_dressed_in_felicitys_materials` (six chamfers a framed doorway, no wall
face in the strip, the jambs from 13 in). Felicity 15, seen with `pd_edit --shot felicity15 out.png
--item door 3 --solid`: the frame reads as Felicity's.

**The shape tool** (`autolevel_build::sweep`, 2026-11-17, the detail brief's step 1): a 2D profile
swept along a path, for every piece of trim. A path is a start and legs; each leg has its own
profile (the same number of points on each) in two axes square to it (`side`, `depth`), and each
profile edge is a face (a kit surface) or nothing (against a wall or the ceiling). Legs meet in a
mitre: each profile point's line along one leg met with its line along the next, so profiles of
different depths meet properly (a pilaster 13 deep into a beam 24 deep: the mitre runs from where
the pilaster's front meets the beam's underside to the wall-ceiling corner, as in Felicity's room
4). Open ends are cut square; a closed path is mitred all round. UVs are explicit: one axis along
the path, the other round the profile, and each edge says which (`along_v`). The clearing step
beside it, `sweep::clear`, takes a piece's `Cover`s (a rectangle in a plane, facing one way) out of
the carver's faces: each face there that overlaps is cut on the rectangle's edges and the pieces
inside go (a piece lying wholly on a cut line, a sliver of no area, goes too). The doorway frames
are now one sweep per wall face (up a jamb, across the lintel, down the other jamb), and
`clear_frame`'s wall faces are a `clear`. The output is the same polygons (test
`the_swept_frames_are_the_frames_built_by_hand`; seeds 3 and 15: every vertex and every other
triangle the same), except that the trim's UVs are now Felicity's (u across the bevel from the
opening, v along it at 212 a repeat), where the lintel's used to be projected from above.

**Ceiling braces** (2026-11-17, the detail brief's step 1; **from Felicity**, not CI Training).
Measured first (`probe_stage_triangles`, which now gives each corner's colour too): Felicity's two
biggest blue-floor rooms (4 and 17, about 8 x 6 cells) each have a cross of beams under the
ceiling, 40 wide and 24 deep (the underside 235 up a 259 room), on a pilaster 13 deep and 40 wide at
each end, floor to ceiling. The pilasters are tile 0256, the beams' sides the plain panel 024B,
their undersides 0262 (a strip of four tiles, the third a light panel), about two repeats an arm
from the wall in. Felicity bakes the undersides full white (254), the pilasters' fronts 137 at the
floor to 254 at the top, and a 254 pool on the floor under the cross; the ceiling stays at 44.

- **Where** (`autolevel::dress`, stream `<seed>/felicity/dress/braces`, after the plan check so no
  room moves): a one-storey `office` or `marble` room 5 cells or more each way, `BRACED` (0.7) of
  them, gets beams both ways, wall to wall: `round(span / 424) - 1` (one at least) along each span,
  evenly placed, so an 8 x 6 room gets Felicity's cross and a 13 x 11 one a grid of four. A pilaster
  keeps `MARGIN` (40) off every doorway's opening in its wall and 60 off the corners; a beam slides
  up to 160 (steps of 20) to clear them, or is left out. `Level::braces`; `dress::arms` cuts each
  beam at the beams crossing it (the kit and the lamps both use it). Seeds 1-40: 159 of the 234
  rooms that could be are braced, 312 beams (test
  `braced_rooms_keep_their_pilasters_clear_of_every_doorway`).
- **Drawn** (`autolevel_build::felicity`'s `arm`, `crossing`): each arm is one sweep from its wall
  end (the pilaster up the wall mitred into the beam, and down the far pilaster when a beam crosses
  nothing), the crossings a square of plain strip. The strip is whole repeats an arm (`PANEL` 180:
  Felicity's arms of 278-396 hold two each), u from the wall in. `sweep::clear` takes out the
  ceiling over each beam, the wall behind each pilaster and the floor under it (test
  `braces_are_swept_and_nothing_of_the_carvers_is_left_behind_them`). Role `lamp` (0262).
- **Light**: a lamp under each panel (`Level::lamps`, `dress::lamps`) in place of the room's lamp
  grid (`bake::light`: `plan_lights` for the other rooms); after the bake every `lamp` face is 254,
  as Felicity's are. Complex has neither, and is unchanged.
- Gameplay: the pilasters are 13 deep; every walk passes on seeds 1-40, and Felicity 15 shows no
  stalls in a minute of six simulants. On seed 15 only upper rooms are braced (upper1, upper6,
  upper7); its ground rooms are too small, or hallside or industrial.
- **Corridor ribs** (2026-11-17, the user: "try corridor ribs and see how it looks"; **not
  Felicity's**: its hallway has none; the rhythm is CI Training's corridors', the piece Felicity's
  brace): every one-storey `office` or `marble` corridor (ground corridors, galleries, landings)
  gets beams across its short span only, `RIB` (318, three cells) apart along it
  (`round(length / 318) - 1`), each where its pilasters clear the corridor's doorways as a room's
  do. No draw (the rooms' stream is untouched: the same 159 rooms are braced). Seeds 1-40: 768 ribs
  in 235 corridors. Felicity 15: corridor3 (marble, ground) has 4, the galleries 3 each, landing1
  2; a minute of six simulants stalls 4 times, all at one door held part open from both sides
  (PD's doors, as before), none by a pilaster.

**Pipes: removed** (2026-11-17, the user's call: "strange and don't really work"). `7d7d002` ran
Felicity's hexagonal pipe (room 39: 53 across, tips 13 deep, shaded by 012A) along the base of
the walls of 60% of the one-storey `industrial` rooms and corridors; on the grey bays they read as
brown and out of place. `dress::pipes`, `Level::pipes`, `felicity::pipe`, the `pipe` role and
their tests are gone (the commit that removed them has them, should they come back in another
form). Nothing else moved: they had their own stream (`<seed>/felicity/dress/pipes`), drawn last.
`Sweep::caps` (capped open sweeps), added for them, stays in the shape tool.

**Hazard surrounds** (2026-11-17, the detail brief's "hazard stripes" and "deep angled frames on
the grey room's doorways"; **from Felicity**, not CI Training: its grey bays, rooms 29 and 32).
Measured first: no strut or stripe piece of its own exists in Felicity. Its stripes are 071E (32 x 32,
yellow and black), the reveal of the doorways into its grey bays: past a jamb 13 deep the opening
flares 39 deep from 105 x 212 to 133 x 224, over a grating floor (01B2, 64 a repeat across the depth,
66.5 along the wall), and the bay's wall tile (071D) splays back from there to the wall face, which
is 92 from the opening's plane (its walls are that thick). Its "leaning struts" are the jambs of the
portals between its bays (next, below). The stripes along its bays' wall base are the hall's base
0276, already ours.

- **Where** (`dress::surrounds`, no draw): every face of a doorway with no arch in a one-storey
  `industrial` room or corridor, where it fits: its splay `SURROUND_GAP` (6) clear of the room's
  corners and room for a surround beside every other doorway in that wall. Seeds 1-40: all 245 such
  faces.
- **Drawn** (`felicity::surround`): our walls are 40 thick, so the surround stands out of the wall
  into the room instead of into it: from the opening's edge at the wall's face the stripes (role
  `hazard`, u across the flare, v along) flare `SURROUND` (39) out, `FLARE` (14, 12) wider and
  taller, and the splay (the wall's upper tile `bay_upper`, its v as the wall's so the blue lines
  meet) runs back to the wall `SPLAY` (54, 35) beside and above the opening. Two sweeps a face, each
  from a jamb's foot up and along the lintel to its middle (the stripes meet there in a chevron);
  the grating (role `grating`) between the stripes' feet, the room's floor in front of each splay;
  `sweep::clear` takes out the wall behind and the floor under it. That face gets no chamfer
  (`clear_frame` and `frame` take which faces are chamfered) and its jambs run to the wall's face.
  Test `hazard_surrounds_stand_out_of_their_walls_with_nothing_of_the_carvers_behind_them`.
- Felicity 5 (stage 123): every doorway of corridor1 and rooms 1-4 (room2's double doorway at plan
  x 1146). Felicity 15 (stage 127): corridor1 (onto the hall and room7), corridor2 and room1. A
  minute of six simulants on each: no stalls.

**Portals** (2026-11-17, the detail brief's "leaning strut" and "deep angled frames repeated along
a corridor"; **from Felicity**: the openings between its grey bays, rooms 29-32, x 1806-1912).
Measured first: each bay's end is bevelled 13 deep (071D) from the bay's own outline in to an
opening 79 deep: straight jambs to 47, then leaning in 53 to a lintel at 235 (317 wide at the floor,
211 under the lintel, in a bay 529 wide), its sides 027D (79 a repeat along the depth, 47.5 up), the
lintel's underside the light strip 071F (once across, once along) baked full white. The "strut"
seen in `cifelicity` is a leaning jamb of one of these.

- **Where** (`dress::portals`, no draw): across every one-storey `industrial` corridor, one every
  `BAY` (424) or so along it (`round(length / BAY) - 1`, evenly), each slid up to 160 to keep
  `PORTAL_MARGIN` (40) off the frames of the doorways in its walls (a surround's splay included) and
  `PORTAL_END` (106) off the corridor's ends, or left out. A lamp under each
  lintel (`Portal::lamp`, in `Level::lamps`), so a corridor with portals is lit by them in place of
  its grid. Seeds 1-40: 88 portals in 51 corridors (test
  `portals_stand_across_industrial_corridors_clear_of_their_doorways`).
- **Drawn** (`felicity::portal`): `PORTAL_D` (105) deep along the corridor; on each face eight bevel
  triangles (the wall's upper tile, banded as the wall) from the corridor's outline (wall, ceiling,
  wall) to the opening 13 in; through it the passage's four sides (role `portal`, 027D) and the
  lintel's light strip (role `glow`, 071F; the bake makes `glow` faces 254 as it does `lamp`).
  `sweep::clear` takes out the walls and ceiling it stands against and the floor under its posts;
  the floor in front of each bevel's foot is put back. Test
  `portals_stand_across_their_corridors_with_nothing_of_the_carvers_inside_them`.
- **Narrower than Felicity's**: posts `POST` 13 in at the floor (Felicity's 106 in a 529 bay; ours
  are 278-384 wide), leaning `LEAN` 40 (Felicity's 53). Felicity's lean broke 17 door-to-door walks
  on 10 seeds: those lines run 80 off the wall's face (`INSIDE` 100 from the doorway's middle plane),
  and a walker is radius 30 and 185 tall, so a portal may reach at most 50 in below 185 (now 42).
- Felicity 15 (stage 127): corridor1 (two, looking along it from its west end) and corridor2.
  Felicity 5 (stage 123): corridor1. A minute of six simulants on each: no stalls.

**Washroom fittings** (2026-11-17, the detail brief's washroom option; **from Felicity**: its
washroom, room 12, x -1404..-823, z -982..260, floor -480; all BG, no props). Measured first: a row
of open-fronted stalls at each end of the room, partitions 14 thick, 212 tall and 238 deep, 105
apart (one cell), in its black wall tile 0274; in each a toilet (0714): a pedestal 47 x 30 rising
25, the bowl flaring to a rim 71 x 77 at 54 (29 beyond the pedestal at the front, 18 at the back,
12 at the sides), the rim 14 off the wall, white water (0001) inside; in the middle of the room a
block floor to ceiling with a sink alcove each side, a basin (0644) in each, 122 wide, hanging 48
to 63 up and 53 out, a tap to 112. The "raised grating floor" an earlier brief gave room 12 is
these toilets' rims and sills (0714 at 25-54), not a floor.

- **The option** (the user's call: on by default, a flag turns it off): `Options::washroom`,
  `washroom` in a recipe (written only when off: `"washroom": false`), `--no-washroom` (or
  `--washroom false`) on the command line. Off, a level has no fittings and nothing else changes
  (test: the plan, doorways, braces, portals and lamps are the same either way).
- **Where** (`dress::washrooms`, no draw): in each one-storey `washroom` room, along the longest
  wall with no doorway that takes them, a row of stalls: the whole wall if it fits, else fewer from
  a corner (a partition closing the open end), the most stalls first, each as deep as fits from
  `STALL_D` (238) down to `STALL_LEAST` (110) by 20; a toilet in each. Then on the longest other
  wall with no doorway, as many basins as fit, `BASIN_EVERY` (150) apart at least. Everything keeps
  off each doorway's mouth (`MOUTH` 120 in front of the opening and its frame) and `FITTING_PAD`
  (40: a walker's radius and 10) off every walk line (doorway to doorway through points `INSIDE`
  in, as the walks go). Our washrooms are small (mostly 278 x 278 with a centred doorway on two
  neighbouring walls), so a row is mostly one stall: seeds 1-40, 52 of 52 washrooms fitted, 58
  stalls, 6 basin rows. `Fitting` is one shape for all three: its middle on the wall's face, facing
  out, its width along the wall and depth out of it.
- **Drawn** (`felicity::fitting`): a partition is a box of the wall's tile (`wc_lower`, banded from
  the floor); a toilet a closed sweep round its pedestal (each side its own profile: pedestal, the
  bowl's underside, the rim, the bowl's inside) and its water (role `water`); a basin a U-shaped
  sweep off the wall with its bowl (white) and underside. Roles `toilet` (0714), `water` (0001),
  `basin` (0644). The floor under partitions and pedestals is cleared (no waypoint inside them).
  Test `washroom_fittings_are_drawn_and_the_floor_under_them_cleared`. No tap yet; no vanity block
  (no washroom is big enough to stand one in).
- To see them: **Felicity 36, stage 122** (seed 15 and 5 have no washroom), upstairs off the
  gallery: upper2, upper3 and upper4 (a stall on its south wall, a basin on its north wall; plan
  1610..1888 x 868..1146, floor 283). A minute of six simulants: no stalls.

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

**Not yet:** windows, raised doorways; no outdoor part; the editor can't make or
edit a Felicity level (a Felicity recipe takes no edits). Nobody has played the layouts of the
handoff brief's item 1 (upper rooms onto walkways, walkways in tall plain rooms, the block's
landing, building round the hall).

## Sculpt

`--style sculpt` (2026-11-18, the user's idea; not a port of anything). Each room starts as a box;
its faces are split into a lattice (2 x 2, then 4 x 4, then 8 x 8) and at each level some patches
are extruded out (an alcove, a pit, a high bay) or in (a buttress, a platform, a soffit; pushed in
high on a wall, a ledge). The rooms are placed beside each other, an overlapping one cut back a
wall's thickness, and joined by doorways through shared walls or corridors routed through the
solid (with ramps where they climb). Then every doorway and every floor of 6 cells or more in a
room is joined to the rest, both ways, by ramps, switchbacks and catwalks: **sculpt, then repair**.

```
autolevel build 2 --style sculpt --out out/autolevel/sculpt2           # plan, polygons, bake, walks
autolevel import out/autolevel/sculpt2 --code sculpt2 --stagenum 102    # the arena
SCULPT_DEBUG=1 autolevel plan 2 --style sculpt --out DIR                # what the repair joined and left out, by room
```

**The user's decisions (2026-11-18):** rooms of every proportion (very wide, very tall, both); drops
are fine (one way); sculpt and repair (not a walkable skeleton first); Complex's look. Later: when a
catwalk, ramp or pillar is in the way of a new join, a rule to divert round it or to consolidate.
After seeing version 1 in `pd_edit` ("like a creeper blew up here"): **no patch smaller than eight
times the smallest feature**, so shapes read as built. Now 8 cells across (944) and 4 up (236, a
storey: 8 up would leave a plain room's walls whole), each patch moved as one flat face.

**How it is built** (`autolevel/src/sculpt/`):

- `vox`: one grid for the level, cells 118 across and **59 up** (a quarter of 236), each solid or the
  air of a zone (a room or a corridor). The plan's rooms are their zones' air cut into boxes
  (columns first, so a box reaches its ceiling); whatever is left solid inside a room (a platform, a
  ledge, a catwalk's slab) is not carved, so Complex's polygon stage draws it as it is. Six rooms
  (`KINDS`: wide, tall, both, three plain) and `LOOPS` 0.35.
- `shape`: one room. The lattice stops dividing at `MIN_ACROSS` 8 cells and `MIN_UP` 4; a patch is
  one lattice part or a strip of up to three, and moves as **one flat face** (out: every column's air
  carried to one plane past the outermost; in: everything beyond a plane that far inside it filled;
  version 1 moved each column on its own, which over earlier steps made the jagged 1-cell blocks).
  Depths: platforms 2, 4, 6 or 8 cells up, pits 2 or 4 down, alcoves 2 to 6 deep, buttresses 2 or 4,
  none under 2; pushing in stops at 4 cells of air up or 4 across. Rooms: plain 14-20 x 12-18 x 6-9,
  wide 28-36 x 22-30 x 7-10, tall 14-18 square x 20-28, both 24-32 x 20-28 x 14-20. 55% are mirrored
  across one axis; a tall room raises its floor less (12% of extrusions against 35%).
- `layout`: placing (a gap of up to 9 cells or an overlap of up to 3 beside a room already placed; no
  more than a fifth of either overlapping), stamping (a later room's air within a cell of an earlier
  room's stays solid; only its largest part is kept), a spanning tree plus loops over the rooms'
  boxes, doorways (2 cells wide, 236 tall, through a one-cell wall with floor at one height both
  sides) and corridors (2 x 4 cells, weighted A*: moves 1 or 2 cells flat, or 2 cells and 1 or 2 up
  or down in a straight line; a ramp ends flat before it turns; nothing but its own two rooms within
  a cell of it).
- `nav`: walkable = air, solid under it, **4 cells (236) of air over it** (with 3, 177, the game's
  player stopped short of places a 172 player fits), no ramp sheet over it. Regions are one zone's
  walkable cells at one height; ways are walking across, **drops** (one way; not onto a sheet) and
  **sheets** (ramps, both ways). Components are strongly connected (Tarjan's).
- `repair`: greedy. Of every join between two regions in different components (a straight ramp, a
  switchback for 4 cells or more, a catwalk across a drop at one height), the cheapest that leaves
  the targets in fewer pieces is made, and again; joins between two pieces with targets first. A
  ramp needs air from its sheet up, not from its foot's floor (it may pass over lower clutter); a
  catwalk is railed only where its side is open to a drop (a rail beside a floor at its height stood
  across the way on: room 4 of seed 1).
- `mod`: the level. Links are every pair of zones whose air touches (the plan check passes on every
  seed tried). Walks: each room doorway to doorway and doorway to each floor, each corridor end to
  end; a target with no path is written as a straight two-point walk **on purpose**, so it fails in
  the walker. A drop's landing is a stop (passed through, the walker counted the cell past an edge
  as reached while still on the edge) unless it is beside a ramp's end, where a standing player is
  on the slope and can't be held to a floor's height.

**Where it stands (seeds 1-8, 2026-11-18, version 2: the coarse patches):** each plans in 0.4 to 1.6
s. 286 of 378 walks pass (version 1: 247 of 373); of the failures all but 7 are the two-point walks
of floors the repair left out (122 over 48 rooms). The 7 others fail part-way on paths the nav
believes. Seeds 1 and 2 are imported (`sculpt1`, `sculpt2`); six simulants for a minute: 0 and 1
kills (version 1: 3 and 4; the rooms are bigger, and some now read as plain halls).

**Next:**
- More of each room reached: an L-shaped flight; ramps up onto a catwalk; rails at the open sides of
  landings; a floor no join reaches made part of a neighbour (lowered, or its ledge removed) rather
  than left out.
- The user's diverting rule, for a join with an existing ramp, catwalk or pillar in its way.
- The 4 part-way failures (seed 1 r3, seed 8 r6: read them with the walker's trace).
- L and U rooms (footprint first: a corner, an edge or the middle cut from the box); angled walls.
- The views: some stand against a wall (a doorway's cell, facing the room's middle).

## Blank levels: your edit wins

**Designed 2026-11-18; step 1 (the empty start) built the same day** (see "As built" at the
end of this section). The user's idea: start from nothing, draw rooms, and have the
generator work out the rest around each edit. A move is top priority, and everything around it is
made again to fit it. Where the edit can be met in more than one way, the editor shows the ways
and the person chooses. The cascade is the point: change something and see what follows from it.
The user's test: start blank, add a room, add a second beside it, add a third that pokes into one
of them. The generator does the rest, and offers alternatives where there are several.

**The user's decisions (2026-11-18):**
1. **The newest edit wins.** Where two of the person's own rooms conflict, the one touched last
   keeps its box and the other gives way. (Priority is by the last edit that names a room, so
   moving an old room makes it the winner again.)
2. **The generator may add rooms of its own** (a junction hall, a stair room) where that is the
   best way to join the person's rooms.
3. **A room poking into another at a different height is offered as alternatives**, like any
   other overlap: it is not always a pod.
4. **Heights need a proper tool: a side view** (below).

### What the seed is for here

From reading the code: routing (`route::connect`) and the dead-end pass draw no random numbers.
For the same rooms and options the router gives the same corridors whatever the seed (tested).
**The dead-end pass does not**: it measures walks over the rooms' decks and ramps, which the seed
furnishes, so a seed whose deck leaves a dead end gets a corridor another does not (two rooms of
an empty start: seed 40 adds a ramp corridor, seed 41 none). The seed's streams are: the scatter (`PyRandom::from_int(seed)`: which rooms exist, and where), each spire's
spiral (`{seed}/{key}/spiral`), each room's dress and furnishing (`{seed}/{key}/dress`,
`/{attempt}`, `/reroll{n}/...`), drops and grates (`{seed}/openings`), windows (`{seed}/windows`)
and the plan (`{seed}/plan`: names, moods, ceilings, tints, glows). With no scatter, **the seed is
a decoration number**: changing it re-dresses the rooms, and can add or cut a dead-end corridor
through them, but never moves one the router made.

### What has to change

The generator already cascades: an edit makes the level again from the first stage it touches.
What it lacks:
- **Contact is refused.** Rooms keep a cell apart unless one is a pod or stacked above the other.
- **Noise in the cascade.** Openings, windows and names come from level-wide streams, and route
  keys are dealt out again, so an edit changes things it has nothing to do with.
- **One answer.** The router makes the cheapest network and nothing else.
- **Nothing shows what changed.** The new level simply replaces the old one.

### The model

- **What the person places is fixed** (their rooms' boxes, and any choice or "keep").
  **What the generator makes can change** (corridors, vents, ramps, catwalks, decks, windows,
  cover, dead-end fixes, and rooms it adds itself). An edit makes the changeable things again
  around the fixed ones.
- **Stability.** Of the ways to meet an edit, prefer the one closest to the level before it: the
  generator's own cost plus a cost for each thing changed. Without this, one moved room can
  rewire the whole network over a small difference in cost.
- **The recipe still replays.** The way the person chose is recorded as an edit in plain words
  (`{"edit": "contact", "rooms": ["room3", "room2"], "as": "carve"}`). The level is never stored.
- **Rooms the generator adds** are its own: a later cascade may move or remove them. The person
  can adopt one (it becomes theirs and fixed).

### The pieces

**1. An empty start.** A recipe field (`"start": "empty"`): no scatter, rooms keyed `room1`,
`room2` and so on. In this mode the level-wide streams are keyed by room and by route
(`{seed}/{key}/openings`, `{seed}/{route}/window`, a room's name from `{seed}/{key}/name`), so adding
a room re-draws only what touches it. Seeded recipes keep the streams they have: **no seed's
level changes, and the answer keys still pass** (a test says so).

**2. Contacts in place of refusals.** How two of the person's rooms meet:

| Contact | What the generator does |
|---|---|
| Apart | Corridors, as now |
| Beside (walls touching, storeys in common) | A shared wall with a doorway: no corridor between |
| Poking in (footprints overlap, storeys in common) | Alternatives (below) |
| Above (no storey in common) | A slab between, as now, or joined by a ramp or a drop |

Built on what is there. Rooms are rectangles (`space::Space`); a room's L-shape is only a notch the
furnisher puts inside it (`build::dress`, `shape`). But `Space::host` is already "the room it pokes
into" for a pod, the hall's furnisher already keeps clear of its pods, and `pods::pod_access`
already gives a pod a way in from its hall with no corridor. So:
- **Poking in** is a host relation, generalised from pods to any size and height.
- **Beside** is a pod's way in, generalised to two rooms sharing a wall.

Sculpt's voxel grid handles any contact (`sculpt::layout` cuts an overlapping room back and joins
rooms through shared walls), but sculpt is shelved, and Complex's rooms, router and furnisher are
the ones that walk clean. Blank levels are built on Complex's model.

**3. Alternatives for an overlap.** When room C pokes into room B (C touched last, so B gives way):
- **C carves B.** C's box is solid in B from B's floor to its top, apart from C's own air: an
  intrusion, like a full-height pod. The wall C shows inside B gets windows or a railed opening,
  so each room sees into the other.
- **C hangs in B.** C is a glazed box in B's air, and B's floor runs on under it. This is a pod,
  of any size, at any height that leaves room under it.
- **Merged.** One room, with a cross wall where they met (the furnisher's `partition`).
- **B pushed away.** Not by the first decision when B is the person's own. Offered only where B
  is a room the generator added.

Each alternative is made as a whole level on a thread, as the Join tool's candidates are
(`candidates.rs`: "what is previewed is what is made"), and the ones that make no level are
listed as refused, with the reason. The rest come ranked (cost plus change), the first one made at
once, the others a click away. With a handful of rooms a level takes a small part of the
0.5 s a seed does, so the alternatives arrive in well under a second.

**4. Alternatives in the network.** The router gives one network. For a choice of corridors,
the next-cheapest networks within a margin are offered too: k-best, or the cheapest with one of
its links forbidden in turn. So is a network with a room the generator adds (decision 2), where it
costs less than the corridors it saves.

**5. The cascade, shown.** After any edit, the plan compares the new level with the one before
it: what is new is lit, what went is a ghost, what moved has an arrow. A list says it in words
("c03 re-routed round room3, +6 cells; two windows in room2's east wall; room2's ring is now a
U"). Each alternative carries its own list. A `diff` of two `Level`s, by room key and by route
ends (not route keys, which are dealt out again).

**6. Keeping what the generator made.** "Keep" on a corridor, a window or a catwalk makes it
fixed (an edit), so later edits route round it. A kept thing that a newer edit cannot be met
with is given up, by the first decision, and the list says so.

**7. The side view.** A switch on the plan, **Top | Side**: a section through the level, height
upwards.
- **The section** is a line on the top view, along x or along y (a toggle), dragged to where it
  should cut. By default it cuts through the middle of the room in hand. Rooms the line crosses
  are drawn solid, those behind it faded, with the nearest in front.
- **The grid is the storeys:** floors at -276, 0, 276, ..., 1654 (`storeys::LEVELS`) and tops at
  -39, 236, ..., 1890 (`storeys::CEILINGS`), labelled up the side.
- **It shows** the rooms' boxes and slabs, decks and catwalks, ramps as slopes, corridors at
  their heights, pods hanging, drops and where they land, and the cascade's ghosts and arrows.
- **Editing:** drag a box up or down to change its floor (its height kept), drag its top or
  bottom edge to change its height (snapped to the storeys), drag it sideways to move it along
  the section's axis. Each goes through the same trial as the top view: tried at every whole
  step while the button is down, and made when it is let go.
- **Heights need an edit.** No edit changes a seeded room's height now (the room boxes' "Rough").
  In an empty start every room is an `add`, so the side view rewrites the add's floor and top.
  For seeded rooms a `height` edit is new: floor and top, a hall's pods kept at the same storeys
  above its floor.
- **Without a window:** `autolevel_edit --side x@Y | y@X --shot`, as the top view's `--shot`.

### Order of work

1. Empty start, with streams keyed by room and route; `--empty` on the command line; a test that
   the seeds' keys are unchanged.
2. The side view, showing first and then editing; the `height` edit.
3. Beside: shared walls and doorways.
4. Poking in: carve, hang and merge, the newest edit winning, the alternatives made in parallel,
   and the `contact` edit.
5. The cascade shown: the `diff`, drawn and listed.
6. Stability (the change cost), "keep" on generated things, and network alternatives.
7. Rooms the generator adds.

**The acceptance test**, as a test and a set of shots at each step: empty start; add `room1`; add
`room2` touching it (one doorway, no corridor); add `room3` overlapping `room2` (at least carve and
hang offered, both walk clean); choose carve (windows in the shared wall; `room2`'s catwalk, if
`room3` blocks it, routed round); then the same with `room3` raised two storeys in the side view.

### As built (step 1: the empty start, 2026-11-18)

- **The recipe:** `"start": "empty"` beside `"seed"` (left out for a seed's own; still format 1).
  It is `level::Start` (`Seed`, `Empty`), held on `Options::start` so every stage sees it, though
  it is not one of `recipe::OPTIONS`. `Recipe::empty(seed)`. `scattered()` is `Placed::empty`
  (no lots; `Placed::blank`, so the last room may be deleted too). With no room, every stage
  refuses with "no room yet: an empty start (draw one)". `level::generate` refuses an empty
  start (its rooms are the recipe's), and `Options::check` refuses one for another style.
- **Streams of their own** (`Options::stream`: a seed's level gets the shared stream back,
  untouched, so the keys pass): a room's name (`{seed}/{key}/name`: its own shuffle of its kind's
  names, the first one no earlier room has, so a room added later never renames one before it),
  its scheme, brightness and ceiling (`{seed}/{key}/plan`), each corridor piece's tint, scheme and
  glow (`{seed}/{a}/{b}/{i}/plan`: by its route's rooms, not its key), a window's glass or rail
  (`{seed}/{hall}/window/{z}/{x},{y}`: by where it is), and a drop
  (`{seed}/{up}/{low}/drop`: its place in the order, then a spot over the two rooms' overlap;
  the drop takes the free place **nearest** that spot, so a door added to the room moves it only
  if the door is where it was. Picking by index, as the seed's level does, moved it.) Grates
  draw no random numbers. A room's dress and furnishing and a spire's spiral were keyed by room
  already. The level-wide limits still hold (`drops`, `windows`, three glows), so a new
  candidate can take a place another had: a real consequence of the edit, not noise.
- **The editor:** `--empty` on the command line (the room boxes always drawn); in the window an
  "empty" switch in the seed row: the edits are set aside (Undo brings them back), the Rooms tool
  is put in hand, and the plan is framed on the site. The bare plan (no level: an empty start,
  or a seed like 25 that makes none) is framed on the site and draws its edge dashed, with a
  legend line (`Boxes::site`); `Doc::base` is cached by the start as well as the seed and siting.
- **Checked:** 5 tests in `tests/recipe.rs` (no room until one is added, the file both ways, the
  last room deleted, the router the same on two seeds while the decoration differs on some,
  names, brightness and ceilings unchanged by a room added elsewhere, a drop unchanged by a room
  added elsewhere), one on the command line and one in the window (the switch, the site framed,
  two rooms drawn, a level of them joined, undo back to the seed with its edit). All 123 layout
  tests and the keys pass unchanged.
- **Rough:** a room the dead-end pass joins with a corridor of its own is still named by a route
  key that is dealt out again. Two rooms "beside" each other are still refused (a cell between):
  that is step 3.

### As built (step 2: the side view and the `height` edit, 2026-11-23)

- **The edit:** `Edit::Height { room, floor, top }` (`place::Placed::height`; the top parsed as
  `add` parses it, `top_line`; the pods shifted by storeys as `pin` shifts them, `raise`).
  `boxes::Change::Height` is what the side view asks for: made a `pin` where the room keeps its
  height, else a `height`, and folded into a made room's `add` like the other lot edits. The
  answer keys pass unchanged (no seed has the edit).
- **The picture** (`autolevel_edit::sideview`): a `Section` (`Axis::X` through the row
  `y = at`, or `Axis::Y` through the column `x = at`; `Section::through` a room's middle). Cells
  across, `z / 118` up (`UNITS`: true proportions). Floor lines labelled ("ground 0", "floor 2
  551") and ceiling lines; rooms the cut crosses solid in their floor's colour, the rest faint
  outlines; decks and catwalks on the cut row (half cells) as thick lines; a room's ramps as
  slopes (a ramp crossing the cut as its box with a diagonal); corridors and vents as bands at
  their heights (a corridor 236 high, a vent 59); a corridor's ramp and a vent's ducts as slopes
  where their ends are within a cell of the cut. With the room tool the boxes are drawn over it
  (`Boxes`), the selected one with four grips and its heights beside it, red with the refusal.
  The same `Prim`s as the plan: the window's `paint` and the snapshot's `rasterise` draw it.
- **Taking hold:** `sideview::hit` (the selected box's top, bottom or ends within the grip, else
  the smallest box under the pointer: a pod over its hall). `sideview::drag`: the body goes the
  way it has gone furthest (across: a `Move` along the section; up or down: a `Pin` to the
  nearest storey), the top a `Height` to the nearest ceiling line (never under a storey), the
  bottom a `Height` to the nearest floor with the top kept, an end a `Rect`.
- **In the window** (`ui::Editor`): `side: Option<Section>`; `canvas` draws `side_canvas` in
  its place, with its own pan and zoom (`side_look`; framed on the selected room, its storeys
  70% of the height), the drag tried at each step and made on letting go as on the plan.
  `side_controls` (the generator's panel, under the tool row): Top | Side, "along x/y", `<` `>`
  to move the cut a cell, "through the selected room". `set_side`, `turn_side`, `nudge_side`,
  `side_through_selected` are public. **On the Layout screen** (`pd_edit::app::shape`): "From
  above | From the side" under "Change the plan", "Turn the slice", the two arrows, "Through
  the room"; the room panel has **Taller / Shorter** beside Down/Up a floor (a `Height` a
  ceiling line either way).
- **Without a window:** `autolevel_edit --side x@Y|y@X|x@ROOM|y@ROOM --shot OUT.png` (with the
  room flags, the boxes over it; `--height ROOM@FLOOR@TOP` the edit);
  `pd_edit --shot-ui OUT.png --seed 40 --room vast --side` (the Layout screen from the side).
  Pictures: `out/autolevel/side/` (scratch).
- **Checked:** one in `boxes` (a height is one edit, pods go with the floor, back is none,
  refusals, folded into an `add`), 4 in `sideview` (sections, hits, drags, seed 40's picture the
  same twice), one in `headless` (`--height` and `--side`), one in the window's (egui events:
  the Side switch, a top dragged up a storey is one `height`, the body up a floor is a `pin`,
  Top again). The pictures read: seed 40 through vast along x, medium5 made four storeys tall,
  the Layout screen from the side.
- **Rough:** nobody has looked at the window. The cut is not drawn on the plan. Zoomed in, the
  floor labels at the left margin go off the view (the line under the view says the height).
  A drag up of the body always keeps the height (a `pin`); moving across and up at once is two
  drags. Corridors are drawn 236 high whatever their own ceiling; cover (pillars, crates) and
  windows are not drawn.

**Not decided yet:** how many alternatives to show at most, and whether they appear as a strip of
small plans or one at a time with numbers (as the Join tool's are); what a room the generator adds
looks like (its kind, and so its dress); whether a seeded level can be switched to "your edit
wins" (its rooms all made the person's) or only an empty one.

## Atriums: a base fifteen storeys up, reached through other rooms

**The user's idea (2026-11-18).** A hall that goes up fifteen storeys: from its floor you see a
walkway at the very top that goes out to a base, and you have to work out how to get up there.
Not a zigzag of ramps inside the hall: the top walkway is a door into another room on that
storey, from which a corridor goes down a storey into another room, then two more, and so on,
until a sequence of rooms comes back into the hall at its floor. The hall is connected through
the level, not inside itself. Later: more bases scattered through a huge hall, and catwalks at
any angle (a feature of its own: everything inside a room is on the half-cell grid; the user
was told it is a few sessions' work and is not sure it is worth it).

**The approach agreed:** goal-first. The Complex generator scatters and repairs, and the
router takes the shortest join, so a long trek never happens by chance. Instead a small
planner lays **only the trek**, backwards from the goal (the base and its door on the top
storey), a room at a time, each 1 to 3 storeys lower, checking at every step that it can still
get home (storeys left over what a room may descend, and the distance back to the atrium's
floor door), and backtracking where nothing fits. The trek is written as recipe edits (`add`,
`keep`; `join ... on` where the router would not join by itself), and the existing pipeline
fills the rest around it, as the blank-levels work does round a person's rooms. First as a
fixed helix (rooms stacked in a ring round the atrium, each 2 to 3 storeys, entered high at one
end and left low at the other), later searched. Order: (1) storeys and atrium rules, (2) the
trek by hand as a recipe, (3) a helix trek planner that writes those edits from a seed, with the
trek's length measured, (4) a searched trek, a second way down (a drop from the gangway).

### As built (steps 1 and 2, 2026-11-19)

- **Storeys** (`storeys::LEVELS`, `CEILINGS`): 17 floors, -276 to 4134 (7 m apart, rounded as
  the Python's are), the highest ceiling 4370. The ones over 1654 are new; nothing the Python
  scatters reaches them, and **every key passes unchanged** (the router's grid is one layer a
  storey: the extra layers change no seed). The editor's storey colours (`planview::colour::
  STOREY`) gained nine paler ones; a room may be added or pinned up to 4370.
- **The atrium kind** (`place::ATRIUM`): one of `Placed`'s kinds (so `add` takes it at any scale),
  never one of `kinds_and_site`'s (the scatter's list is the Python's: the lots key checks it).
  Its rules: doors on its floor and its top storey (`Space::door_levels`; at first its two lowest
  storeys too, but a door on its first upper storey let the router climb from there into the middle
  of the trek, so step 3 took it away); pods on its
  first two upper storeys and its top one (`scatter::pod_levels`); in the furnisher each storey
  over the two lowest is a band of its own (`Interior::band_of`): `place_ramps` never ramps a
  band down, and `connected` asks that each band's doors reach one another, not every door every
  other. Its dress never takes pillars (they run floor to ceiling: fifteen storeys of them hid
  everything); its names are `level::ATRIUM_NAMES`, drawn from a stream of the room's own (not
  `NAMES`, which a seed's level shuffles kind by kind); a hub; schemes as the colossal's.
- **Walls** (`autolevel_build::geometry::FLOORS` is now `storeys::LEVELS`): the strip lines and
  the schemes' second strips go on up every storey. Before, walls over 1890 were one stretched
  strip. The polygons key passes.
- **The prototype** (test `an_atriums_top_is_reached_through_other_rooms_and_never_ramped_down`
  in `tests/recipe.rs`, `atrium_trek()`): an empty start, seed 1; the atrium 18 x 14 cells from 0
  to 4094; seven `tower` rooms in a helix round it, each two storeys lower than the last (3307 to
  4094 on the south-west, down to 0 to 787 on the west); a `small` base, 4 x 4 cells, in the
  middle of the atrium's top storey; `keep` on the top room. **The router made the whole helix
  by itself** (one join a step, at the storey the two rooms share) and opened the top door
  (room2 to the atrium on 3858) with no `join`. The atrium's top storey is a catwalk from that
  door round two walls and a gangway out to the base, which stands on a pier from the floor (a
  base on a fourteen-storey column: it looks right). The dead end listed is the trek: room6 to
  the base through rooms 5 to 2, 164 cells' walk, 10 storeys. `autolevel build --recipe`: 14
  rooms, 9104 triangles, 17 of 17 walks. Views: `pd_edit --shot-generated out.png --recipe R
  --at 3600,160,-4130,45,58` (up the shaft from the floor), `--view room1_deck3858` (the top).
- **Checked:** all the layout, build, editor and import tests and every key, clippy, fmt.

**Rough, and next:**
- **A shortcut:** the router joined the atrium's first upper storey (276) to room6 (1102). It
  makes a loop, not a spoiler (five rooms still to climb), but the trek planner should decide
  which of the atrium's low storeys take doors.
- **`keep` stands in for a goal.** Without it the dead-end pass joins the trek's rooms to each
  other on other storeys (four corridors), cutting the climb short. The planner wants a "this is
  the goal" mark the pass leaves alone, or a second way out (a drop from the gangway).
- **The walks** go door to door in each room, so nothing walks the atrium's top catwalk out to
  the base (each piece of the trek is walked; the whole trek is not one walk).
- **Messages** still say a pod is on one of "its hall's first two upper storeys", an atrium's top
  one aside. The editor's floor buttons are now 17.
- **Not imported as an arena** yet, and nobody has played one. (The stage numbers were the limit
  then, 124 to 127; since M22 the game numbers custom levels itself, 160 of them.)
- Next is step 3, the helix trek planner (above).

### As built (step 3: the trek planner, 2026-11-19)

- **`autolevel trek SEED [--storeys N] [--out DIR] [--no-bake]`** (`autolevel::trek`, `lay`): an
  atrium N storeys tall (15 by default; 4 to 16), 16-22 by 12-16 cells near the site's middle,
  with a base (a `small` pod, 3-5 by 3-4 cells, three cells and more from the walls) on its top
  storey; then the trek from the top down along a ring round the atrium, a cell clear of it, from
  a seeded side and either way round: each room 6-10 cells along the ring and 6-9 out (8 at least
  one way, so it takes doors on its upper storeys), dropping two storeys (a `tower`, 70%) or one
  (a `medium`), its top storey the one the room before stands on, until one stands on the
  atrium's floor. When a ring is full a second goes round outside it, 10 cells further out.
  Written as a recipe on an empty start (`trek<SEED>/recipe.json`, every room an `add`) with the
  trek's options (`trek::OPTIONS`: no loops, risers or vents, each a way round the climb, and the
  dead-end pass off, since the trek is the level). Drops stay: a hole into the room below is a quick
  way down. Then built and walked as `build` does.
- **Checked by the way up** (`trek::climb`: the fewest rooms from the atrium's floor to the base
  over the routes and pods' doors, the atrium's floor and top two places, drops left out). A
  layout is kept only if that way goes through every room of the trek; else (or if the lots
  break the placing rule) the next stream is tried (`{seed}/trek/{attempt}`, 40 at most).
  **Seeds 1-40: all 40 laid** (0 to 7 tries), 7 to 10 rooms each, every walk as expected
  (15 to 21 a level), 0.2 s a level. Seed 24 needed the second ring.
- **The router knows an atrium is two places** (`network::connect`): its top storey is a node of
  the spanning tree's own, and a join to an atrium is tried for its floor and for its top, the
  route held to those storeys (`Terms::la`, `lb`). Before, the tree thought the trek's two ends
  joined through the atrium and left out a link in the middle (seed 1: trek6 to trek7). A level
  with no atrium makes the same joins in the same order: every key passes.
- **Tests:** `trek::tests` (rooms round the atrium clear of it; seeds 1-8 climb through every
  room in order to the base, the atrium joined on its floor and top only; the same seed the same
  trek), the command line's.
- **Arenas:** `atrium1` (stage 103, the hand-laid prototype) and `trek1` (stage 104, `autolevel
  trek 1`). The custom stage numbers run from 96 to 127; 103 to 121 were free.

**Rough, and next:**
- A trek is the whole level: the atrium and its climb, nothing round them. Joining it to a seed's
  scattered rooms needs the scatter to start from fixed lots (or the blank-levels remake).
- The outer ring's rooms are joined by long corridors (seed 24: three of 15 cells and more).
- Nothing yet chooses a second way down (a drop from the gangway into the atrium, a vent).
- The rooms are all `tower` or `medium`, so a trek's names run out (then a room is named by its key).

### As built (the towering scale: high bases in the main generator, 2026-11-19)

**The user's direction (2026-11-19):** the trek is "a big room, a high base and one way down,
with uniform rooms whose only purpose is the path"; the goal was never that, but a system in
which such constructions fall out of the procedural code with the variety the Complex
generator already has. So the atrium's rules were made general and given to the scatter.

- **Towering halls** (`space::towers`, `TOWERING` = 1890): an atrium, or any room but a spire
  whose ceiling is over 1890 (nothing the Python scatters is). Only its lowest storeys are ramped
  inside it (`space::ramped`: 4 for a hall of 300 cells or more, else 3; an atrium 2) and take
  doors as before. **`Space::high`**: the storeys over those that hold a pod (worked out in
  `spaces()` from the lots, and again after pods without a way in are left out). Each is a place
  of its own: a door on it from outside (`door_levels`), a band of its own in the furnisher (no
  ramp down; its doors and pod joined by deck on that storey), a node of its own in the router's
  spanning tree. The atrium kind is now one case of this: its high storey is its base's.
- **Pods** (`scatter::pod_levels`): a towering hall takes them on its first two upper storeys
  (as every hall) and on every high storey, so the scatter lifts them up its whole height.
- **The scale** (`--scale towering`, `place::TOWERING_KINDS`, site 88 x 66): grand's kinds
  reaching higher. A colossal hall 10, 12 or 15 storeys, a vast one 5 to 10, three spires up to
  12 storeys, towers to 8, the long gallery also at 1654, mediums also at 1102 to 2756, eight
  lofts from 551 to 3307, ten smalls and six tinies (the pods). Seeds 1-60: 54 make a level (grand
  makes 56 of the same seeds).
- **Climbs** (`autolevel::climbs`, `climbs(lv)`): for every pod on a high storey, the way up from
  its hall's floor by the fewest rooms, over routes and pods' doors (drops left out). The trek's
  `climb` is now this. `cargo run --release -p autolevel --example climbs -- 1 40` lists them.
  **Seeds 1-40: 38 levels, 88 high pods, every one climbed to**, through 1 room out (28), 2 (12),
  3 (29), 4 (11), 5 to 7 (7). Seed 9: two pods at 1654 and 1929 in the colossal hall, each six
  rooms out (spire, the vast hall, the basin, another spire, a loft, the long gallery). Some pods
  have a back door of their own and are climbed to without going back into the hall.
- **What it took** (each only where a towering hall is, so every key passes):
  - a high storey's catwalk is one piece: where a pod blocks the short way round, it goes the long
    way (`Interior::arc`, `whole`). Before, 22 of 60 seeds failed with "no set of ramps".
  - the dead-end pass never takes a high pod away: it is joined if it can be, else kept
    (`Build::holds_high_pod`).
  - a high storey no route reached (nothing stands that high near enough) loses its pods and the
    level is routed again (`level::unreached`).
  - a towering hall's ramps keep clear of piers on every storey they cross, not only from the floor.
  - a room's walk is made for each of its bands (`room_<key>` and `room_<key>_<storey>`). It strung
    every door together and jumped between bands. Seed 9: 101 of 101 walks.
  - no pillars or lanes in a towering hall (they run floor to ceiling).
- **Tests:** `climbs::tests` (towering seeds 1-10: every high storey holds a pod, every pod is
  climbed to from its hall, at least five climbs through three rooms or more; a grand level has no
  high storey), with all the trek's and the atrium's.
- **Arena:** `towering9` (stage 105, `autolevel build 9 --scale towering`): 89 rooms, 1445
  waypoints all reachable.

**Rough, and next:**
- Nothing scores the climbs yet: the sweep ranks towering seeds by the old score. A climb's
  length (rooms out, storeys) belongs in the report and the score, so `sweep` and `batch` pick
  the levels where good ones fell out.
- Most climbs are short (one room out: a spire beside the hall). Longer ones need fewer rooms at
  the high storeys near the hall, or a preference for joining a high storey from further away.
- The six failures in 60 seeds are the generator's old kinds (a basin's ramps, a colossal hall's,
  rooms cut off): made since by the rescue ("Rescue", below).
- In the import's one-minute check six simulants on `towering9` met almost never (0 kills): 89
  rooms over fifteen storeys is a lot of level for six.

## Platforms: filling a hall's empty air (2026-11-19)

**The user's ask (2026-11-19):** find the empty space in the big halls and fill it, with large
platforms on the catwalks (as the landings at a catwalk's turns, but larger), where not much is
happening. Both are new in the Rust, and off by default: every seed's level is as it was.

- **Empty air** (`furnish::platforms`, `Interior::void`): a tall room's half cells over which
  nothing is built on any storey above its floor (no deck, pod, pod roof, ramp or landing, pier,
  and nothing a drop or a grate's beam falls through): columns of air floor to ceiling. The
  furnisher's per-storey cell sets give it at once; nothing new is modelled. In the report
  (after the Python's 54 keys, `cli::RUSTS_OWN`): `empty_air`, its share of the tall rooms' cells,
  and `largest_void`, the biggest rectangle of it in one room, in cells.
- **Platforms** (`--platforms N`, the recipe's `platforms`, the editor's slider; `Setup::platforms`):
  one for each 100 cells of empty air, times N (in a tall hall, the air each storey sees within
  `WINDOW`, shared among the storeys that see it: a hall 15 storeys tall takes several times a
  short one's). Laid after the ramps (and the cut under
  them), before the cover and the rails, so the rails run round a platform's open edges and a
  pillar under one stops at its underside. A platform is a rectangle of deck 4 to 8 cells a side
  (`SIZES`, reaching no more than a third of the way across), a catwalk's depth in from every wall,
  touching its storey's deck along 2 cells or more (`CONTACT`), with nothing built over or under
  it within two storeys (`WINDOW`: a tall hall's column may hold a platform low down and another
  high up; what a drop or a beam falls through is kept clear on every storey), a cell clear of
  anything hard, with 60% of the air within 3 cells of it empty (`EMPTY`). The best
  by area times that emptiness, each next one kept 2 cells clear of the last and counting for
  less within three storeys of another (`SPREAD`). No random numbers: nothing else in the room
  moves. Rooms under 10 cells across and spirals get none.
- Each platform has a view, `platform_<room>_<storey>` (`_2` for a second on that storey), from the
  far end of its storey's catwalk.
- **Seeds 1-20** (with `--platforms 1`): towering, the same 18 levels, 117 platforms, empty air
  0.64 to 0.51; grand, the same 20 levels, 64 platforms, empty air 0.66 to 0.56. Every walk passes
  in both. Seed 9 towering: 8 platforms (the colossal hall three at 276, three at 1654, one at
  1929; the vast hall one at 276), empty air 0.62 to 0.45, largest void 136 to 68 cells. (With the
  whole column kept clear, as at first, it was 4 and 67.)
- **Density saturates**: seed 9 has 8 at `--platforms` 1, 2 or 3. What limits it is catwalk to
  hang from: a towering hall's storeys over its ramped ones have catwalk only where a high pod or
  a door is (seed 9's colossal hall: 276, 1654, 1929), so the walls over them stay bare.
- **Test:** `platforms::tests` (a platform is deck, joined to its catwalk, over a column nothing
  else is in, clear of ramps, with its view; the same seeds make levels with and without).
- **Arena:** `towering9p` (stage 106): `towering9` with `platforms: 1`.

**Rough, and next:**
- A platform hangs off catwalk that is already there (above): galleries (below) give the high
  storeys some.
- Nothing stands under a platform but what the cover puts there; it floats as the catwalks do.
- Platforms are a risky place for location 5, after islands and pillar tops ("Placement", below);
  nothing else is placed on them by design.
### Galleries: high catwalks of their own (2026-11-19)

**The user's idea (2026-11-19):** a high catwalk and its platforms can be a place of their own, as
a high base is, with its own way up and down through other rooms. So a high storey no longer
needs a pod.

- **`--galleries N`** (the recipe's `galleries`, the editor's; `level::galleries`, run at the end
  of `stage_rooms`): in each towering hall, up to N of its storeys over the ramped ones that hold
  no pod, `GALLERY_GAP` (2) storeys or more from each other, its pods' storeys and the top ramped
  one, at a height another room has a door on. Drawn from the hall's own stream
  ("{seed}/{key}/galleries"): nothing else moves, and none unless asked for. Each is put in
  `Space::high` and listed in **`Space::galleries`**, so everything a high pod's storey has comes
  with it: a door from outside, a band of its own (never ramped down), a node of the router's
  tree, a walk, and `unreached` leaving it out (and routing again) if no corridor got there.
- **Its catwalk** (`lay_decks`): the length of every wall it has a door in, joined in one piece
  (`arc`, `whole`); platforms then hang off it.
- **The dead-end pass** joins a gallery with one way in to another room if it can, else keeps it
  (`Build::holds_gallery`): it is a climb's end.
- **Climbs** (`climbs::climbs`) take galleries too (`Climb::gallery`; the way ends in the hall on
  that storey); `--example climbs -- 1 40 2` lists them with their doors. In the report:
  `galleries` (after the Python's keys). Views: `gallery_<hall>_<storey>`, from its door.
- **Seeds 1-20, towering, `--galleries 2 --platforms 1`:** the same 18 levels, 56 galleries, 167
  platforms (117 without galleries), empty air 0.47, every walk passes. Seeds 1-12: most galleries
  have two to four doors; the longest climbs go 6 and 7 rooms out (seed 9's colossal hall at 2756:
  spire, vast hall, basin, spire, loft, vast hall, loft). Seed 9: 5 galleries, 12 platforms.
- **Test:** `climbs::tests::a_gallery_is_a_high_storey_of_catwalk_climbed_to_from_outside`.
- **Arena:** `towering9g` (stage 107): seed 9 towering, `platforms: 1`, `galleries: 2`.

**Rough, and next:**
- Galleries with one door stay as camps; a second way down (a drop to a catwalk below) is not made.
- The gallery's storey must be one another room has a door on, so a hall's highest storeys get
  one only where a spire or a loft reaches them.

### Drops: holes through platforms and floors (2026-11-19)

**The user's ask (2026-11-19):** drop-downs to what is below, as regular rectangular holes and as
holes spanned by catwalks with no rails, on floors and platforms, made with some chance wherever
there is something to fall onto a reasonable distance below.

What there was: the Python's drop (`build::openings`, `--drops N`, 2): a hole two cells square in
a room's floor into a single-storey room at most one empty storey below, railed on three sides.
Seeds 1-20: 13 in 20 grand levels, 3 in 18 towering ones.

- **`--holes P`** (the recipe's `holes`, the editor's slider; 0 by default: every seed as it was).
- **Pits** (`furnish::pits`, laid after the platforms, before the cover and the rails): each
  platform, with chance P (its own stream, "{seed}/{key}/pits/pit/{z}/{x}/{y}"), gets a **hole**
  (2 by 2 cells, 2 by 3 in a big platform; railed on three sides, open towards the catwalk it hangs
  off) or a **span** (the platform's middle open a cell in from its edges, crossed by one or two
  unrailed catwalks a cell wide, `planks`), where everything under the opening is standing room
  within `MAX_FALL` (4) storeys with nothing between (a catwalk, a platform, the hall's floor; no
  other opening). The rails leave the open side and the planks' sides out (`Interior::unrailed`);
  nothing stands where it lands (`keep`). Each has a walk (`pit_<room>_<storey>`: from a door of its
  band to the edge, off it, to the landing), so the importer links it one way for simulants
  (`towering9h`: 3 linked, 2 already joined), a view of the same name, and a one-way way in the
  dead-end graph. Report: `holes`, `spans`.
- **More drops between rooms** (`build::more_drops`, after the Python's are chosen): any pair of
  rooms overlapping by four cells or more, one over the other with no room between and no more
  than `MAX_FALL` storeys from floor to floor, the lower no spiral or basin and **tall or not**,
  takes a drop with chance P (stream "{seed}/{up}/{low}/hole"), 2 or 3 cells each way. A drop into a
  tall room must land clear (`lands_clear`: nothing it built in the column, its floor standing
  room there); `Build::refresh` takes one that does not away and furnishes again until none is
  left (it used to re-furnish one room by hand, which left an `Arc` the nav cache, keyed by
  address, could see again: a panic in `Places::of`).
- **Seeds 1-20 with `--holes 1`:** drops between rooms, grand 13 to 18, towering 3 to 10. With
  `--platforms 1 --galleries 2 --holes 0.5`: towering 18 levels, 39 pits (15 spans), 5 drops between
  rooms; grand 20 levels, 26 pits (7 spans), 15 drops. Every walk passes. (Towering seeds 4 and 14
  make no level with or without any of this: "no set of ramps".)
- **Tests:** `pits::tests`, `build::tests::holes_add_drops_into_rooms_a_storey_or_more_below_and_land_them_clear`.
- **Arena:** `towering9h` (stage 108): seed 9 towering, platforms 1, galleries 2, holes 1: 5 pits
  (one a span).

**Rough, and next:**
- Drops between rooms stay rare: the scatter seldom puts one room over another (grand seeds 1-10:
  three pairs overlap at all). More would need the scatter to stack rooms on purpose.
- A span in a room's floor (between rooms) is not made: the floor hole is one rectangle all through
  (`Setup::hole`, the plan's shaft, `Sided`). Spans are only in platforms.
- A drop off a catwalk's open edge (an unrailed stretch) is not made.

### Towers: unreached rings, and a shape per storey (2026-11-19)

**The user's report (2026-11-19):** in `towering9h` a tower's top ring and the third from the top
could not be reached; and a ring at every storey, all alike, is dull (the opposite of the sparse
hall).

- **Unreached rings** (a galleries bug): `tower2` of seed 9 (8 storeys, a ring tower, galleries at
  1102 and 1654) had a ring on every storey; its own storeys' ramps can't join a gallery's band,
  so the rings at 1378 and 1929 were reached by nothing. Now a storey over one of the hall's high
  storeys gets no deck of the room's own unless it is high itself (`lay_decks`: `past_high`). Seeds
  1-20 with galleries: 10 such storeys in 6 of 12 seeds before, none after. `--example room --
  RECIPE (KEY | X,Y | list | orphans)` prints a room storey by storey, or every storey whose
  catwalk no door reaches (grand seeds 1-20: none; what seemed so were pods' roofs beside a deck).
  Towering seed 20's colossal hall has a 3-block pocket at 276 with or without any of this.
- **Why the stack:** a `tower` (towering: 4, 5 or 8 storeys), the furnishing's coin for "ring" over
  "ends" (`Setup::tower`, each attempt), and a ring tower six cells across or more takes a ring on
  every storey it may deck, one ramp between each: eight alike. (Vast halls and basins: a 30% ring
  on any storey with deck.)
- **A shape per storey** (`decks::SHAPES`, towering towers only, their first 4 attempts): each
  storey's own deck is a ring, a corner (two walls), a side (a long wall) or an end (a short wall),
  never the shape of the storey under it, the wall drawn too, from "{seed}/{key}/pits/shape/{attempt}/{z}".
  Later attempts lay rings or ends as before. `Interior::shapes` lists them. Towering seeds 1-60:
  54 levels with and without galleries (as before: 4, 14, 43, 46, 49, 55 fail either way); seeds
  1-20 with platforms, galleries and holes: every walk passes.
- **Test:** `climbs::tests::a_towering_towers_storeys_take_turns_and_every_catwalk_is_reached`.

### Cellars: a hatch down, a ladder up (2026-11-19)

**The user's ask (2026-11-19):** rooms rarely lie under rooms (the scatter keeps a lot only clear
of the rest, and nothing aims for one under another: grand seeds 1-10 have three overlapping
pairs), so few drops; put a room under a room on purpose, its way out a ramp if it fits, else a
climbable grate (Complex has one) against a wall, which fits nearly anywhere. Floor thickness was
already there: a storey is 236 of room and a 40 slab, the plan is air, and a drop's shaft is an air
box from the lower room's ceiling to the upper's floor (`planner`), its sides drawn.

- **`--cellars P`** (the recipe's `cellars`, the editor's slider; 0 by default: every seed as it
  was). **`level::cellars`**, in `stage_rooms` before `spaces()`: a colossal, vast or tower lot on
  the ground floor takes, with chance P (its own stream "{seed}/{key}/cellar"), a `medium` lot
  `cellar<n>` on the storey under it (-276 to -39), 4-9 by 4-8 cells, `CELLAR_INSET` (3) cells in
  from its walls, clear of every other lot by the scatter's rule; twenty tries. **`Space::cellar`**
  (`Cellar { hall, hatch, side }`): the hatch two cells square against one of the cellar's walls,
  a cell from its corners, clear of the hall's pods. Neither room's doors open onto it
  (`Space::clear`).
- **The router** joins a cellar to its hall's floor like a pod (`Network::hatches`, in `edges` and
  the spanning tree): it needs no corridor (one may come as a loop; seed 40 gets three).
- **The hatch** is a drop (`build::openings`, before the Python's, always): the hall's floor hole,
  its open side forced to the ladder's (`Setup::hole_side`, `Interior::forced_side`; the floor
  there kept clear), railed on the other three.
- **The ladder** (`geometry::ladder`): the kit's `ladder` (the palette's 21st: Complex's grille
  0x745, as its own ladder in room 24 is, a cell wide, a storey tall, tiles 0x44) a cell wide on
  the cellar's wall under the open side, `LADDER_OFF` (2) in front of it, from the cellar's floor
  through the shaft to the hall's floor (the shaft part in the hall's zone). The importer flags its
  tiles `GEOFLAG_LADDER` (`pd_import::autolevel`), which `bondwalk` climbs (players) and the
  waypoint maker links one way up (simulants: they climb, never descend; down is the drop).
- **Walks**: `drop_<hall>_<cellar>` (in through the hatch) and `ladder_<cellar>_<hall>` (from the
  ladder's foot, `Cellar::foot`, to the floor beside the hatch: walked on `pd_sim`, it climbs);
  views `drop_<hall>_hole`, `ladder_<cellar>`. The dead-end graph has the ladder as a one-way way
  up; the dead-end pass joins a cellar if it can and never takes it away (`holds_cellar`). Report:
  `cellars`.
- **Seeds 1-20 with `--cellars 1 --holes 0.5`:** grand 20 levels, 40 cellars; towering 18 levels
  (4 and 14 fail with or without), 36 cellars; every walk passes, the ladders' too.
- **Test:** `build::tests::a_cellar_lies_under_its_hall_with_a_hatch_down_and_a_ladder_up`.
- **Arena:** `cellars40` (stage 109): seed 40 grand, `cellars: 1`, `holes: 0.5`.

**Rough, and next:**
- No open ramp up through the hall's floor yet (a slot about 1 by 6 cells): the ladder only.
- Only colossal, vast and tower halls take cellars; a grand tower is seldom wide enough (it needs
  10 cells each way).
- Simulants get up by the waypoint maker's ladder link; the importer's report counts the ladder walk
  as joined.

- The user's other questions with this (2026-11-19): a large base kind (the pod kinds are 3-5 by
  3-4 cells and 2-3 by 2; the scatter's rule allows a fifth of a hall's floor), and a base shared
  by two halls a cell apart (a pod has one host now). Not started.

### Angled catwalks: islands and shortcuts (2026-11-19)

**The user's idea (2026-11-19):** straight catwalks at any angle, not on the grid, as Complex has
(its railing runs at 45 degrees from (-997, -1601) to (-761, -1365)). Spokes to a central island
were the first example, not a target: angled catwalks spanning a hall's air on their own, or joining
platforms of any kind, are as welcome.

- **An overlay, not a grid change** (`furnish::angled`). An angled catwalk (`Angled`) is a slab a
  catwalk wide on one storey, from one deck edge to another, each end cut along its deck's edge
  (`End`: the edge's axis and line, the way out, the crossing point, the two corners in units, the
  nav block just inside). It is not deck and not in the plan: `geometry::angled` draws it in
  `finish` (top `floor`, the world's projection, so the plates run on from the decks'; underside
  `ceiling_dark`; sides `plain`; `complex::railing` down both sides), and `fix_t_junctions` runs
  once more when a level has any, since its corners lie along the decks' edges.
- **Its footprint** (the half cells it crosses and a half cell round, by a convex-polygon test,
  `cover`) is `Interior::spanned`: a platform's `built` counts it hard, a pit lands on nothing and
  falls past nothing there (`lands_on`), no pillar stands under it (`lay_cover`), a drop between
  rooms does not fall through it (`build::lands_clear`), and no pit is cut where one ends
  (`joints`). When it is laid it keeps clear of what is at its height (`blocked`: the storey's deck,
  pods and roofs, ramps from or to the storey and their landings, piers through it, every column a
  drop or a beam falls down, other angled catwalks); a storey over or under it has a deck's headroom.
- **Its way**: `nav_over` joins its two end blocks; `waypoints` steps onto its middle line a half
  cell inside each deck and walks that line (the end blocks are beside it: walking block to block
  caught the railing on a long one); the dead-end graph has a way of kind `angled`; the room's
  rails are cut where it crosses an edge (`cut_rails`).
- **`--angled P`** (the recipe's `angled`, the editor's slider; 0 by default: every seed as it was).
  In a hall 10 cells across or more that is no spiral, from its own stream ("{seed}/{key}/pits/angled",
  every draw made first): with chance P an **island** (`lay_island`): a rectangle of deck 3 to 6
  catwalk widths a side (`ISLANDS`) with nothing over or under it within two storeys, 3 cells
  (`CLEAR`) from every deck at its height, the most clear air round it first (to `REACH`, 12 cells),
  taking 3 or 4 spokes (`spokes`: the shortest first, 60 degrees apart round it, clear of one another);
  and with chance P each catwalk storey a **shortcut** (`lay_shortcut`): of every angled catwalk from
  that storey's deck edges at 20 to 60 degrees off straight out (`ANGLES`), the one that saves the
  most walking on the room's nav, 10 cells or more (one between decks nothing else joins counts 40).
  Lengths 2 to 24 cells. A shortcut often reaches a platform (`--platforms`), and joins two.
- **Walks and views**: `angled_<room>_<storey>_<n>`: from a door of its band to one end by any way
  but that catwalk, across it, and onto the island's middle for a spoke; the view from its end
  along it. Report (`RUSTS_OWN`): `islands`, `angled`.
- **Seeds 1-20 with `--angled 1`:** grand, the same 20 levels, 11 islands, 64 angled catwalks;
  towering with `--platforms 1 --galleries 2 --holes 0.5`, the same 18 levels, 4 islands, 103 (most
  of them shortcuts, many to platforms: platforms take the air islands want). Every walk passes;
  `--example room -- RECIPE orphans` lists nothing. Without platforms, 13 islands in 46 big halls of
  seeds 1-10, spokes 3 to 13 cells long.
- **Found on the way:** the importer's translucent block tree (`pd_import::bsp`) recursed for ever
  when a cut parted no triangles (slivers within `EPS` of planes they do not lie in: the angled
  railings' were the first); such a set is now a leaf.
- **Test:** `angled::tests::angled_catwalks_join_decks_and_an_island_is_reached_only_by_its_spokes`
  (grand seeds 1-10 bare, towering 1-10 with platforms and holes: the same seeds make levels with and
  without; the ends on standing room and on their edges, no rail across a joint, nothing in a
  footprint, each in the dead-end graph with a walk and a view; an island cut off on the grid alone
  and reached from a door through its spokes).
- **Arena:** `angled9` (stage 110): seed 9 towering, platforms 1, galleries 2, holes 0.5, angled 1:
  an island at 1654 with four spokes and seven shortcuts.

**Rough, and next:**
- Flat only: an angled catwalk joins two decks on one storey. An angled ramp between storeys is not
  made (the grid's ramps are straight along x or y).
- Islands are rare where platforms are on (the platforms fill the air first); a platform could be
  left floating on purpose and reached by angled catwalks only.
- Its ends cut along one edge each: none meets a deck at a corner, and none crosses another.
- An island is the riskiest place there is: location 5 goes on one when the level has it ("Placement").

### Flooded rooms: water under railed walkways (2026-11-19)

**The user's idea (2026-11-19):** a room whose floor is water, a good place for platforms and
catwalks: walkways and platforms at floor level joining its doors, the water recessed a little
and never walked on, at a low chance. The user's calls: railed all round, the water half a cell
down, and **on by default** (`level::FLOODED`, 0.1).

- **Which rooms** (`level::floods`, in `stage_rooms`): each room, with chance `--flooded P` (the
  recipe's `flooded`, the editor's slider) from its own stream ("{seed}/{key}/flooded"), unless it
  is a pod, a spire, a basin, an atrium, a cellar or a cellar's hall, under 5 cells across, or has a
  room under it where the pit would be. `Space::flood` is the water's height, `FLOOD_DEPTH` (59) under
  the floor. A flooded room keeps its upper doors (a basin's `pool` would allow its floor's only);
  the router keeps the storey under it clear (`route`), and no drop or grate goes into or out of it.
- **Furnished dry, then flooded** (`furnish::flood`, `lay_flood`, after the ramps and the cut under
  them, before the platforms): so its decks and ramps are as they would be. Then **walkways** a
  catwalk wide join every floor target (a door's pad, a pod's way in, a ramp's foot), each the
  shortest way to those laid that turns least (`walkway`: `TURN`), never under a ramp from the floor;
  **rafts** (`lay_rafts`), platforms in the water off a walkway along one side, jutting into the
  widest water (one for each 150 cells of it, at most 3); an **island** with chance 1/2 and the best
  **shortcut**, both angled catwalks at floor level (`angled`, over `decked`: the floor's walkways),
  from the room's own stream ("{seed}/{key}/pits/flood"), whatever `--angled` says; then **tanks** in
  the water (`lay_tanks`, as a basin's, after the angled catwalks). The water is the rest
  (`Interior::pool`, `reflood`); the walkways are the basin's `bridge` (floor slabs over the pit,
  `plans`), the pit is the room's box from the water to its floor, less its pods' blocks, which
  stand on its ground; its pods' piers stand in the water.
- **Never walked on**: the rails run round every walkway's edge over the water (`lay_rails`, as round
  a pool); a ramp from the floor is railed down every open side over the water, even alongside the
  deck it climbs to (where a dry room's is not: the deck has its own rail, and the floor under it is
  only a step off), from its foot to where its rail's top meets the deck's (`ramp_rails`; the user
  found the water reached off such a side in `flooded13`, 2026-11-19); the walk `!flood_<room>` steps over one into the widest water and must fail; the importer
  leaves the water's waypoints out (nobody gets back from it). The room's walks cross its walkways.
- **The keys** pin the Python's levels, which have none: both key tests pass `flooded: 0` (`options`),
  so they still check every stage exactly. The levels the Rust makes by default change where a room
  floods.
- **Seeds 1-20 by default (0.1):** grand, the same 20 levels, 11 flooded rooms in 8 of them;
  towering, the same 18 levels, 14 in 10; every walk passes. With `--flooded 1 --angled 1` (every room
  that can be): grand 20 levels, 158 flooded rooms, towering 18 levels, 231; every walk passes. Of
  the arenas, `angled9`, `atrium1`, `autolevel1x4`, `towering9`, `towering9g`, `towering9h`,
  `towering9p` and `trek1` now have a flooded room and were imported again.
- **Test:** `flood::tests::a_flooded_floor_is_water_under_railed_walkways_that_join_its_doors`
  (grand and towering seeds 1-6 with every room flooded: the same seeds make levels; walkway or water,
  never both; every floor target on the walkways, none under a ramp; the doors join; rails on the
  floor; rafts are walkway, tanks in the water, piers on it; no drop into it).
- **Arena:** `flooded13` (stage 111): seed 13 grand, flooded 0.4, angled 1: the colossal hall, the vast
  hall, the tower and a medium room flooded, 4 rafts, an island.

**Rough, and next:**
- The water is a floor 59 down, kept off by rails: nothing happens to a player who gets into it (no
  player can), and it is drawn as the basins' water.
- The walkways are a tree: a second way round (a loop over the water) comes only from the shortcut.
- A flooded room's floor has no cover but its tanks.

### Shaped rooms: a first try (2026-11-19)

**The user's question (2026-11-19):** rooms that are not rectangles (an L, a U, any shape with
square corners), carefully, as a test of whether it is worth it. The design talked through: the
shape lives inside the lot (subtractive), so the scatter, the router and the Python's deck code
are as they were; a union of lots in the scatter would change everything from seed 1 on.

- **`--shaped P`** (the recipe's `shaped`, the editor's slider; **0.1 by default**, `level::SHAPED`,
  the user's call: low, and only in rooms big enough): **each room where a cut fits**, with chance P
  (its own stream "{seed}/{key}/shape"), has a rectangle cut from it (`level::shapes`,
  `Space::cuts`): a corner (an L) or the middle of a side (a U), 30 to 45% of the room each way, its
  arms `ARM` (5) cells across or more in a hall, `ARM_LOW` (3) in a room of one storey, two cells
  clear of its pods' blocks and their ways in. Not a pod, spire, basin, atrium, cellar or cellar's
  hall, nor a hall under 10 cells across or a room of one storey under `LOW_LEAST` (8), nor any room
  of an empty start (drawn by hand: each is as it was drawn). (At first only the biggest hall
  was shaped; the user liked `shaped1` and asked for it wherever it fits, 2026-11-19.) A shaped room
  of one storey takes no notch or partition (`build::dress`).
- **The cut is solid on every storey**: nobody's in the router (`route`: an id of its own, so no
  door, stub, vent or corridor reaches it), not the room's (`Interior::cut`, `inside`, `holds`,
  `built`, the ramps' `no_ramp`, `no_top`, `no_foot`), and out of the room's air in the plan
  (`plans::boxes` subtracts it as a pod's block is: the walls round it are the carver's, the lamps
  are per box). No window, drop or grate goes into or out of a shaped hall, and none is flooded.
- **Decks** (`furnish::shape`, `shape_decks`, after `lay_decks`): laid along the rectangle's walls as
  the Python lays them (inside the room only), then wherever a storey's deck touches the cut a
  catwalk's depth of deck runs round the cut's inner walls, so a ring follows the L or the U round
  its inner corners. Everything else works on the room's cells as it is. No grate's beam falls into
  a shaped room (`build::openings`).
- **A shape that can't be furnished** (towering seed 5: no set of ramps joined the colossal hall's
  storeys round its cut) is furnished as its rectangle instead (`Build::furnish`'s caller): the cut
  was nobody's, so no door or corridor minds. Report: `shaped_rooms`.
- **Seeds 1-20 with `--shaped 1 --platforms 1 --angled 1`** (every room that fits, before the 8-cell
  least for a room of one storey): grand, the same 20 levels, 91 shaped rooms; towering the same 18,
  83; every walk passes. **By default (0.1):** grand 20 levels, 3 shaped rooms; towering 18, 3; every
  walk passes.
- **The tests pin `Options::python`** (`Recipe::python`): a seed's level with the Rust's own rooms
  that are on by default (flooded, shaped) left out, so such a feature moves no pinned count. The
  key tests, the Python's dead-end list, seed 40's counts in `autolevel_build`, `pd_import` and
  `pd_edit`, the editor's fixture and its command-line tests (`--flooded 0 --shaped 0`) all use it.
- **Test:** `shape::tests::a_shaped_room_is_an_l_or_a_u_and_nothing_stands_in_its_cut`.
- **Arena:** `shaped1` (stage 112): seed 1 grand, shaped 1 (its colossal hall a U, its vast hall and
  medium4 Ls), platforms 1, angled 1, holes 0.5.

**Rough, and next:** the cut is dead solid (corridors and small rooms could use it, with doors on its
inner walls); one cut a room (a T, a cross, a Z would take two); the layouts that assume a rectangle
(galleries, a mezzanine) are clipped, not reshaped; the editor's plan view draws the lot's whole
rectangle (the cut shows as air) and its room tool draws rectangles only.

### Sunken floors: a pit with a ramp down (2026-11-19)

**The user's ask (2026-11-19):** where a room's floor is flat and open, a pit with a ramp into it:
not deep, a recessed area with no railing round it, and no railing on its ramp either. The user's
calls: a cell deep (118), and on by default at a low chance.

- **`--sunken P`** (the recipe's `sunken`, the editor's slider; **0.1 by default**, `level::SUNKEN`):
  each room with that chance (its own stream "{seed}/{key}/sunken") has a rectangle of its floor
  `SUNK_DEPTH` (118) lower (`level::sinks`, `Space::sunk`): its sides `SUNK_SIDE` (4 to 7) cells,
  `SUNK_INSET` (3) cells clear of the room's walls (the floor in front of its doors, the catwalks'
  ramps along the walls) and two cells clear of its pods' blocks and their ways in, with nothing
  under it where its pit would be (as a flooded room's). Ten tries, then none. Not a pod, tower,
  spire, basin, atrium, cellar or cellar's hall, nor a flooded or shaped room, nor any room of an
  empty start; a room is 10 cells across or more, so nearly every one is a hall's floor.
- **Its pit is the room's** in the router (`route`: marked on the storey under it, as a pool's
  pit), and in the plan it is a box of the room's air under its floor (`plans::boxes`): its walls
  are the carver's, and nothing goes round its edge.
- **The ramp** (`furnish::sunken`, `lay_sunk`, after the decks and before the ramps): from the
  middle of one of the pit's sides, its top on the edge, `RUN` (4 h, two cells: 26.6 degrees)
  down into the pit, a catwalk's width (`band`). The side is drawn from the room's own stream
  ("{seed}/{key}/sunken") among those with standing room in front of the ramp's top. It is a
  plan ramp of kind "plate" (`csg::plate_ramp`), as the sculpted levels' floor ramps: a sheet of
  diamond plate with a lip (`GRATE_EDGE`, 16) down each open side, no railing. (At first its sides
  hung all the way to the pit's floor, a solid wedge; the user found them ugly, 2026-11-19.)
- **The pit is not the floor's** (`Interior::sunk_cells`): not walkable to the furnisher
  (`walk`), so nothing stands in it or a cell round it (`sunk_clear`: cover, the ramps' feet and
  bodies keep out, and its ramp's way in), no drop lands in it, no drop or grate goes into or out
  of the room, and the room's nav goes round it. The importer's waypoints are the game's
  (`pd_sim::nav::gen`): simulants go down into it and back up.
- **Walks:** `sunk_<room>` from a door to the ramp's top, down it (a stop at its foot: running
  down it, PD's feet lag the slope by more than a pass-through point allows), across the pit,
  and back up; `!sunk_<room>` from the pit's far side straight at the floor past its wall, which
  must fail. **View:** `sunk_<room>`, from two cells past the far side, looking at the ramp.
  Report: `sunken`.
- **Seeds 1-20, grand and towering:** with `--sunken 1`, 46 sunken floors; by default, 3; every
  walk passes either way (towering 4 and 14 make no level either way).
- **The tests pin `Options::python`**, which leaves sunken floors out too (with flooded and
  shaped rooms): no key or pinned count moved.
- **Test:** `sunken::tests::a_sunken_floor_is_a_pit_a_cell_deep_with_a_ramp_down_and_nothing_round_it`.
- **Arena:** `sunken1` (stage 113): seed 1 grand, sunken 1 (its colossal and vast halls).

**Rough, and next:** one a room, rectangles only, and only rooms 10 cells across (a room of one
storey would take one with a smaller inset);
nothing is put in a pit (cover, a weapon's spot is the editor's).

## Rescue: every seed makes a level (2026-11-20)

**The user's ask (2026-11-20):** some seeds never build (towering 4 and 14 among them): why, and
could a last step join what is left apart, with a climbable chute or the like, so that no seed
fails? Seeds 1-100, grand and towering: 17 of 200 failed; seeds 1-300 at all three scales, 73 of
900. Three kinds, none of them parts of the level that can't be joined to each other:

- **"no set of ramps joins its storeys"** (nearly all): one hall can't join its own storeys. A
  piece of deck upstairs with nowhere below for a ramp to land (a basin's pool and pods under it;
  towering 4's basin), a door's or pod's landing alone on a high storey a pod cuts off from its
  catwalk (towering 14), a door onto a pod's roof no deck touches (grand 25), a ramp across a
  landing, a pier through one, a tall tower's ramps out of tries. Every attempt fails alike, and
  the level is given up.
- **"rooms cut off"** (towering 43, 49): a loft alone far over everything else (1378, 3307) that
  no corridor reaches.
- **"was furnished without its door"** (towering 46): a room kept from before the dead-end pass
  (its inside is cached by its doors' places, `Sig`) knew a door by the key of the corridor that
  had been there.

**What was done** (all new in the Rust, and none of it runs where the level could be made
without it: **seeds 1-300 at three scales, the 827 levels made before are byte for byte as they
were** (`report.json` aside, which has `chutes: 0`, `crossings: 0`, `cut_off: []` now; walks,
views, plan the same), the 73 others
are made):

- **`Options::rescue`** (on by default; the recipe's `rescue`, `--no-rescue`, the editor's
  switch). A room that every attempt fails to furnish is furnished once more, attempt by attempt,
  with `Setup::rescue` (`Build::furnish`; the error, if that fails too, is the first pass's):
  - a door onto a lone pod's roof makes that roof a floor (`stitch::land_roofs`), and an island
    pod's pier through a landing is left out (the pod floats);
  - the ramps are laid as before, but a piece no ramp can reach (or the tries run out) is let be
    (`ramps::strand`), so long as every door still has somewhere to stand;
  - then **`stitch`** joins the pieces: while some door does not reach the others of its band, the
    cheapest join from the first door's piece: a **chute**, a ladder a cell wide up the edge of a
    deck from a storey below (`furnish::Chute`, `CHUTE_COST` 24 a storey), or a **bridge**, deck a
    cell wide over one storey's air (or a basin's water), as straight as it can be (`TURN` 6 a
    turn, up to `BRIDGE_MAX` 64 steps), round what stands in the way. A high storey is its own
    band, so it is bridged; only where a pod as wide as the storey cuts its catwalk in two
    (towering 124) is it climbed to from the hall. Then any piece of deck no door reaches is
    joined too if it can be.
- **A chute** stands clear of ramps and door landings on every storey it passes, with a second
  block of floor out from its foot to step off onto; its foot is kept clear (`keep`), nobody
  walks through it on its foot's storey (`stand`), a flooded floor's walkways reach it, its top's
  rail is open, and the nav joins its ends both ways. It is built **on the deck's edge**, not
  `LADDER_OFF` out as a pillar's (a player stepping off the deck must come out in front of it to
  climb down: `bondwalk.c:1614`, a move away from a ladder is a climb down; 2 units out, behind
  it, they hang there). Walks: `chute_<room>_<n>` up, `chutedown_<room>_<n>` down, and every other
  walk down one stops at its lip first (run off the deck at it, PD's player moves away too fast
  to climb, and hangs). View `chute_<room>_<n>`. Report: `chutes`. The editor's plan draws it (the
  ladder on the edge, an arrow up it).
- **No rescue in a join's trial** (`Build::trial`): the dead-end pass takes back a join a room
  can't be furnished with; were the rescue to furnish it, the join would stay and a level made
  before would change (grand 237 did, before this). The cache's key says whether the rescue was
  allowed.
- **A bridge** is deck like any other (the floor's `bridge` on the ground), and is kept in
  `Interior::crossings` too: report `crossings`, and the editor's plan outlines it in the
  ladder's cyan.
- **Rooms cut off** (`level::stage_network`): the rooms outside the level's largest piece are
  left out (`Network::drop`, with their pods), if that piece is most of the level. They are kept
  in `Network::cut_off`: report `cut_off`, and the editor's room boxes say "left out" by them.
- **A kept room's door** is found by where it is when its key is another's (`walks::inside`).
- **Seeds 1-300, grand, standard and towering:** 900 of 900 make a level; 48 have chutes.
  The 73 made now, built and walked: 5200 of 5200 walks pass (the chutes up and down too).
- **Tests:** `build::tests::a_room_no_ramp_can_join_is_rescued_with_a_ladder`,
  `a_room_that_cannot_be_furnished_fails_the_level` (a room too small even for a ladder); the
  recipe's, editor's and `pd_edit`'s no-level cases now say `--no-rescue` (`rescue: false`), and
  the recipe's check that a rescued room keeps a layout chosen by hand.
- **To look at, one of each** (imported arenas, `crates/pd_import/levels/rescue*.json`, stages
  114-117): `rescueladder4` (towering 4: a chute up the basin's west catwalk at 276),
  `rescuebridge140` (standard 140: a bridge in the vast hall at 276, by small15),
  `rescueladder124` (towering 124: tower1's chutes, one from the floor to its high band at 1378),
  `rescuecutoff221` (towering 221: loft6, alone at 3307, left out). The door found by its place:
  towering 46, nothing to see.

**Rough, and next:**
- A rescued room spends its time failing first: every attempt's ramp search runs out before the
  rescue starts, and again each time the dead-end pass furnishes it anew (grand 46: 12 s to plan,
  0.1 s of it the rescue). Only rescued seeds pay it.
- A bridge across a hall at a high storey can be long (towering 14: twenty cells, straight across
  the hall at 1378); some rescued levels keep dead ends the pass can't fix (its joins are tried
  without the rescue).
- Simulants: the importer's waypoints are repaired from the walks with a chr's probe, as the
  cellars' ladders are; nobody has watched one use a chute.

## Placement: what the generator knows of its floors (2026-11-20)

**The user (2026-11-20):** weapons and ammo stood on slopes, and a weapon's ammo could be far from
it; now that the levels are ours, placement should make sense. Agreed order: (1) the generator
labels its floors and nothing stands off flat floor; (2) distances by walking, ammo in its
weapon's room; (3) rules per item (against walls, one weapon per room first, a pickup in a pod off
the path, hills in halls, bases in rooms with two ways in, cover behind the generator's own cover)
and fairness numbers in the report; (4) waygroups from the generator's rooms; (5) sparse waypoints
from the plan. **Location 5 (the set's sixth slot, the power weapon) goes somewhere risky, and
spawns stay out of corridors** (the user's answers). Steps 1 and 2 are done (committed
2026-11-20, `3d6eadf`), and step 3 (below, "Step 3").

**Before** (seed 40, the plain placement, judged by the same audit): 6 of 46 items off flat
floor, 14 on a ramp, in a doorway or by a hole, 3 of 16 spawns in corridors, ammo a 19.7 m walk
from its weapon on average (68 m at most; 8 of 20 crates in another room), location 5 anywhere.
**After:** 0, 0, 0, ammo 5.2 m on average (15 m at most; 2 of 20 in another room), location 5 on a
pillar top. Seeds 1, 7 and 1 at size 4: 0, 0, 0, 4.4 to 5.3 m. Seed 2 with `angled 1, cellars 1`:
location 5 on its island.

- **The hints** (`pd_import::autoplace::hints`, from the built level, read after it is made: no
  draw of the generator's, so no key moves):
  - each PD room's kind by its number (the plan's `room_id`): `hub`, `room`, `corridor`, `ramp`,
    `vent`, `perch`;
  - **clear zones**, where nothing stands: every ramp of the plan (`Room::ramps`, the furnished
    rooms', spirals', risers', sunken floors', ducts') from half a cell before its foot to half a
    cell past its top and 30 units out of its sides; every opening of `Built::openings`, a
    doorway's mouth 150 units either side of its plane and 30 past its jambs, a hole 100 round its
    edge; every pit through a platform (`Interior::pits`) and a half cell round it;
  - **risky zones**, riskiest first (`autoplace::RISKY`): islands, pillar tops with a ladder
    (`Interior::ladders`: the pillar's top), platforms, galleries (the deck of `Space::galleries`'
    storeys), perches (rooms of kind `perch`), cellars, sunken floors.
- **The guided placement** (`autoplace::Guide`, used by `place::place` when the source is a
  generated level and the layout doesn't place that kind):
  - every usable waypoint (the largest strongly connected part, no crouch or duck spot) is
    **settled** on flat floor: its floor, or a point up to 50 cm off in sight of it at knee and pad
    height, whose ground and that of every point of a ring 45 cm round it is the same floor within
    3 cm (`autoplace::flat`: a 4 degree slope, a ledge, a step, a hole or a wall in the ring fails
    it). Of the rest, those in a clear zone are dropped. Seed 40: 693 of 941 spots kept, 77 off flat
    floor, 135 by a doorway, 36 by a ramp;
  - **location 5** first: the riskiest kind of place the level has, its zone nearest the level's
    middle, the spot nearest that zone's middle;
  - **spawns** by farthest-point sampling on walking distance (`pd_sim::nav::gen::dijkstra`, now
    public, over the graph's links), 5 m apart, 10 m from location 5, out of corridors, ramps,
    vents and the risky places; corridors let in only if the rest has no room (the report says how
    many). Facing their most open way, as before;
  - **weapons** the same way, 6 m apart and 5 m from the spawns, out of ramps and vents, corridors
    only if the rooms are full; location 5 swapped into the sixth slot;
  - **ammo**: two crates per weapon (PD fills a crate for the MP location set just before it,
    `setup.c:1710`, so each crate is that weapon's ammo), in the weapon's PD room 2 to 5 m walking
    from it, else 1.5 to 9 m in its room, else anywhere 1.5 to 15 m; 1.5 m apart, 2 m from anything
    placed.
  - Hills, Capture the Case and cover: step 3, below.
- **The audit** (`autoplace::audit`, a `placement audit:` line in the report) judges whatever was
  placed, the plain placement too: items off flat floor (a 30 cm ring), items in a clear zone,
  spawns in corridors, the walk from each crate to its weapon, crates outside their weapon's room,
  what location 5 stands on.
- **`plain_placement`** in a recipe's `source` (`"plain_placement": true`) places as on any custom
  level and only audits. A layout (the editor's) still wins over both.
- **Tests:** `autoplace::tests` (the hints of seed 40 and of seed 2 with islands; seed 40 imported,
  its report's lines). `probe_placement` (ignored): any recipe's placement lines,
  `PD_RECIPE='{"format": 1, "seed": 2, "options": {"angled": 1}}' cargo test -p pd_import --release
  probe_placement -- --ignored --nocapture`.

### Step 3: a rule for each kind of thing (2026-11-20)

- **Weapons before spawns.** With the spawns first, farthest-point sampling sent them to the
  level's ends (the dead ends) and the weapons, kept 5 m off them, went elsewhere: a spawn's walk to
  its nearest weapon was 52 to 133 m at most. Now location 5, then the weapons, then the spawns
  (`place::place` computes the guided weapons before the spawns; a layout's spawns still keep
  them off).
- **Weapons** (`Guide::weapons`), each pass after the last until there are enough:
  1. against a wall (a wall within 1 m at knee height in one of eight ways: `Spot::wall`; a
     catwalk's railing counts) in a **dead end** (a room linked with one other room, by the
     waypoints' links either way: `Walker::ways`), with 4 spots or more, up to a third of them;
  2. against a wall in any room (not a corridor, ramp or vent) with 8 spots or more (room for the
     crates: a 1-spot room on seed 7 left a weapon with none), as many to a room as it takes (one,
     and one more for each 150 spots: the big hall takes more);
  3. anywhere in those rooms, as many as each takes; 4. anywhere in them; 5. in any room but a
     ramp or a vent; 6. corridors too.
- **Spawns** (`Guide::spawns`): 5 m from every weapon, 10 m from location 5, and within a 25 m walk
  **to** a weapon (the reversed links, `Walker::walk_to`: a drop is quick one way only; walked from
  the weapon, seed 2 had a spawn 77 m from one), else 50 m, else anywhere; out of corridors as
  before.
- **Hills** (`Guide::hills`): the middle of a hall or room (`hub`, `room`) with 8 spots or more,
  spread by walking: the open spot (no wall beside it, not a risky place) nearest the middle of its
  spots on its lowest floor (`Guide::middles`).
- **Capture the Case** (`Guide::bases`, `Guide::respawns`): bases in the middles of halls and rooms
  with two ways in or more and 24 spots (a smaller room on seed 1 at size 4 had no room for six
  respawn pads), as far apart as walking allows; each base's six respawn pads nearer it (walking)
  than any other base, within 15 m of it (else 30 m), out of corridors and the risky places.
- **Cover** (`place::cover` as before) on the guided spots only: none on a ramp or in a doorway.
- **The fairness line** (`placement fairness:`): a spawn's walk to its nearest weapon (least,
  most, mean), the dead ends with a weapon, the most weapons in a room, hills not in a hall or a
  room, bases with one way in.

| seed 40 | plain | steps 1-2 | step 3 |
|---|---|---|---|
| items off flat floor / on a ramp, doorway, hole | 6 / 14 | 0 / 0 | 0 / 0 |
| spawns in corridors | 3 of 16 | 0 | 0 |
| ammo walk from its weapon, mean (most) | 19.7 m (68) | 5.2 m (15) | 4.9 m (9) |
| crates out of their weapon's room | 8 of 20 | 2 | 0 |
| a spawn's walk to its nearest weapon, most (mean) | | 89 m (34) | 25 m (17) |
| dead ends with a weapon | | 1 of 7 | 2 of 7 |

Seeds 1, 7, 1 at size 4, 2 (`angled 1, cellars 1`: location 5 on its island) and 5 (towering,
`platforms 1, galleries 2`): every spawn within 25 m of a weapon, every crate in its weapon's room,
hills all in halls and rooms, bases all with two ways in or more.

### Step 4: waygroups from rooms, measured and not done (2026-11-20)

PD's router (`padhalllv.c:630`, `NavGraph::nav_find_route`) finds the fewest **group** hops first
(a breadth-first search over the groups), then leaves each group by whichever of its links into the
next group it meets last (or where the coin stops: not the one nearest the chr), and walks inside the
group by fewest waypoint hops; a simulant carries five waypoints (`MAX_CHRWAYPOINTS`) and routes
again. Nothing else reads the groups (the decomp's only uses are in `padhalllv.c`). So a group's size
is a trade: tiny groups make the group search a fewest-hops search over the waypoints (on an even
lattice, near the shortest walk); big ones make a big hall one hop, the same as a pod.

`autonav::tests::probe_waygroups` (ignored; `PD_STAGE`, `autolevel40` by default, read from
`custom/`) regroups the imported graph (`pd_sim::nav::gen::regroup`: the maker's grouping, within a
radius of each group's seed, and with `by_room` inside one room) and walks 399 random pairs as a
simulant does, against the shortest walk along the links:

| groups (seed 40, 956 waypoints) | groups | waypoints each | median | 90% | worst | a route |
|---|---|---|---|---|---|---|
| as imported (the maker's 2 m) | 790 | 1.2 | 1.02 | 1.07 | 1.41 | 12 us |
| 4 m, inside a room | 274 | 3.5 | 1.06 | 1.17 | 1.67 | 5 us |
| 6 m, inside a room | 180 | 5.3 | 1.05 | 1.20 | 1.58 | 3 us |
| 8 m, inside a room | 126 | 7.6 | 1.05 | 1.27 | 1.86 | 2 us |
| 12 m, inside a room | 86 | 11.1 | 1.06 | 1.41 | 2.04 | 2 us |
| a room each | 66 | 14.5 | 1.09 | 2.04 | 5.10 | 3 us |
| 8 m, rooms ignored | 98 | 9.8 | 1.06 | 1.27 | 1.78 | 2 us |

(PD's arenas have 2.5 to 32 waypoints a group, mostly 4 to 8, over 100 to 370 waypoints.) Every
coarser grouping walks further, a room each worst of all (the 10% worst routes twice the shortest);
the only gain is speed, 12 us a route to 2 or 3, nothing at six simulants. **The waygroups stay as
the maker makes them.** `graph_from_links_grouped` (`by_room`) and `regroup` stay for the probe.

**Rough, and next:**
- Most dead ends stay empty (seed 40: 5 of 7; seed 1 at size 4: 16 of 19): too small, too cramped by
  doorways, or the third used up.
- Every default level's riskiest place is a pillar top (the mixed cover's pillars take ladders),
  ahead of platforms and galleries; the zone nearest the middle wins, so location 5 is often in the
  big hall.
- Waygroups stay as they are (step 4, above: coarser groups make simulants walk further), and
  step 5 (sparse waypoints from the plan) is dropped (the user, 2026-11-20: the goal was where the
  weapons, ammo and spawns go; the waypoints are fine as they are).
- Nobody has played it.

## Bases: rooms that cut into a hall (2026-11-22)

**The user's ask (2026-11-22, with a sketch):** a pod is a room wholly inside a bigger one, and
they are all small. Let a small room cut into a big one instead: half in and half out of its
wall, up through its floor, or down through its ceiling, with windows to see and shoot into the
hall (all four sides, one for cover, any mix), and wherever a window is, something to stand on at
eye level with it. And rooms in rooms of more varied sizes. The user's open questions were
answered by default: the windows are **open slits** (no glass), and a base is entered **only from
outside** (a fortified base); both are easy to change.

**The model** (`autolevel::bases`): a base is a lot of kind `base` (`place::BASE`), never
scattered. It crosses one hall (`bases::hall_of`: the first lot that is no base or pod, overlaps
it in plan and shares a height with it).
- **One room a storey** (`bases::split`, in `level::stage_rooms` before the cellars): each storey
  is its own `Space` with a whole floor, so whoever stands in it is at eye level with its slits,
  at any height the base reaches. The lowest keeps the lot's key; the rest are `{key}_2`,
  `{key}_3` upwards (the editor draws them as the lot, `planview::differs`, `deleted`).
- **Storeys are joined as a cellar is to its hall** (`Space::cellar`): a hatch two cells square
  in the upper room's floor and a ladder up the lower room's wall. All of the cellars' machinery
  serves them unchanged (the router's union, the drop, the ladder's geometry and `GEOFLAG_LADDER`,
  the walks `drop_*` and `ladder_*`, the dead-end graph, `holds_cellar`). **The lower room of
  each pair is a cell narrower on the ladder's side**, so the climb comes out on the upper room's
  floor (a ladder up a wall that runs on up would come out against it). That side is one that
  does not look into the hall where one exists.
- **`Space::base`** (`bases::Storey`): the column, the hall, `inside` (wholly in the hall: no door
  is possible, the hatches are its ways in), the walls with slits (`DIRS` indices) and how they
  are cut ("band": one slit along the wall; "posts": a cell of slit, a cell of wall). Drawn from
  the base's own stream, "{seed}/{key}/base": all the walls that look into the hall (0.4), the
  one facing its middle (0.25), or two or three.
- **The hall's side** (`Space::blocks`: each base storey on one of its storeys): the block is
  the room and a half cell of wall round it, run on to the hall's wall where it would leave a slot
  under two cells (as a pod's shell), cut back to the hall (`bases::block`), from the ceiling under
  it to the floor over it (`bases::block_z`). The furnisher adds it to `shell` (that storey) and
  `roof` (the one over it, unless the base goes on up there), so everything that keeps clear of a
  pod keeps clear of a base; `plans::boxes` subtracts it from the hall's air. **On the hall's
  floor storey** a shell is new (no pod is ever there), so `walk` and the cover's floor now leave
  it out (no seed's floor changes: it is empty there).
- **The router** marks base storeys after every other room and before pods (each overwrites the
  hall it crosses), and the block on the hall's storey as a pod's shell. A door needs empty cells
  outside it, so no base door can open into its hall: a base through a wall gets its door on the
  outside part, one through the floor on the storey under the hall, one through the ceiling on the
  storey over it. `door_levels` is a one-storey room's.
- **Slits** (`bases::slits`, `planner`): air of the base storey's own through its block's wall,
  from `SILL` (100) to `LINTEL` (190) over its floor: a standing player's eye (159) is in it, a
  crouching one's under the sill (the wall below is cover). Each is an opening the plan checks
  (an "overlook" link) and so a portal in the import. A wall looks into the hall when the block's
  face is a cell or more from the hall's walls; the span keeps a half cell at each end
  (`bases::faces`).
- **What a base storey is not given:** cover or a cross wall (`build::furnish`: nothing between
  its slits and whoever stands at them), drops or grates (`build::openings`, `more_drops`),
  corridor windows (`windows`), a shape, water or a sunken floor (`level::shapes`, `floods`,
  `sinks`, which also skip a hall with a base in it).
- **Names** (`bases::NAMES`, the column's own stream "{seed}/{column}/name"): "The Pillbox", its
  other storeys "The Pillbox, storey 2".
- **Views** (`walks::views`): `base_<room>` from the far side of the room out through its first
  slit, `base_<room>_out` from four cells out in the hall back at it.

**The rules** (`bases::misfit`, in `Placed::check` for a base and its hall; every other pair by
the scatter's rule): not a spire, basin or atrium; at least `DEEP` (2) cells into the hall each
way, and not right across it; four storeys at most (`MOST_STOREYS`); a whole storey shared with
the hall; not wholly inside it (that is a pod's place); through a wall, at least two cells out
(for its door); and each storey under another a cell narrower, so a base of n storeys is at least
2 + n cells across. Each refusal says which in words.

**By hand:** an `add` of kind `base` over a hall, as any room (the editor's room tool offers the
kind). **By the generator:** the recipe's `bases`, a switch, **on by default** (the user's call,
2026-11-22: "a toggleable option ... it will default to on"; `--no-bases` on the command lines,
"Bases" among the Generate screen's features in `pd_edit`, a switch in "Advanced";
`bases::random`): each hall (no spire, basin or atrium; 8 cells across and 80 of floor or more)
with the chance `bases::CHANCE` (0.75), from its own stream "{seed}/{key}/bases", takes one: through one of its
walls (a storey or two of the hall's, 3 to 10 cells along it, 2 to 5 in and 2 to 4 out), up
through its floor from the storey under it, or down through its ceiling from the storey over it
(a storey or two into the hall, 3 to 8 cells a side, `INSET` (3) cells from its walls), clear of
every other lot. Twenty-five tries, then none. Not in an empty start.

**Bigger pods:** the recipe's `big_pods`, a switch, **on by default** as bases are
(`--no-big-pods`; "Big pods" on the Generate screen; `level::grow_pods`): each pod with the chance
`BIG_POD_CHANCE` (0.5; "{seed}/{key}/grow"), not in an empty start, grown about its middle to 5 to 10 cells by 4 to 8
(`BIG_POD`), in its hall a cell from its walls, a fifth of its floor at most, clear of every
other room, and only if it and every other pod of its hall still have a way in
(`pods::pod_access`).

**On by default, and what that took (2026-11-22):**
- **Every seed that made a level still makes one.** A seed whose generated bases or grown pods
  leave it with no level (a hall no set of ramps can join round a block) is made without them,
  as the rescue makes every seed a level: `Recipe::plain` (the recipe with both off, for a seed's
  level only), tried by `Recipe::generate`, `furnished` and `finish` and by `level::generate` when
  the level as drawn fails. The recipe still says `bases: true`; the fallback is the same every
  time. Probe: `bases::tests::seeds_with_bases_make_levels_as_they_did` (ignored; `SEEDS=1-60`):
  seeds 1-60 at grand, standard and towering, 180 levels with and without, none lost; 294 bases;
  one fallback (grand 42), and 13 levels with no base (no hall drew one, or none had room: most at
  the standard scale, whose halls are small).
- **A slit only where the hall is clear a cell in front of it** (`bases::slits`, which now takes the
  level's rooms): a pod's shell or another base's block there left a slit opening onto nothing,
  and the plan check refused the level (standard 58, towering 18 and 52).
- **Bases take some halls from the other features:** a hall with a base is never flooded, shaped
  or sunken, so those are fewer now (their own tests switch bases off to count them).
- **The arenas already imported keep their levels:** every seeded Complex recipe in
  `crates/pd_import/levels/` (and `autolevel51`) now says `"bases": false, "big_pods": false`, so
  importing or placing one again makes the level it has (its items' places included); `bases7`
  says `true` and was imported again.
- **Tests that pin a seed's level by the defaults** pin it by `Options::before_bases()` /
  `Recipe::before_bases(seed)` (the defaults with only these two off), the key tests and the
  importer's by naming them off, the editor's command-line tests by `--no-bases --no-big-pods`
  (`tests/cli.rs`, `PINNED`); `Options::python()` leaves both out too.
- **The Generate screen** (`pd_edit::app::shape::FEATURES`): two more chips, whose on and off
  values are `true` and `false` (`off_at`; a test turns every chip on and off).

**Tests:** `bases::tests` (the sketch's three in one hall: one room a storey, doors only
outside, the hatches and the narrower lower rooms, nothing of the hall's in a block, each slit an
overlook with its view; the refusals in words; `--bases 1` on seeds 1-12 makes levels exactly
where the seed does, with all three kinds and no inside storey with a door; big pods grown with
their doors). The recipe and editor tests pin the kind list with `base` in it.

**Checked (2026-11-22):** the keys and every test of the three crates and `pd_import` pass (no
seed's level changed). `--bases 1` on grand seeds 1-12: 12 levels, 26 bases, every walk passes.
With everything on at once (`--bases 1 --big-pods 1 --platforms 1 --angled 1 --holes 0.5
--cellars 1 --flooded 0.3`), grand 1-12: 12 levels, 26 bases, 1,152 walks, all pass; towering 1-3,
5-11 (`... --galleries 2`): 10 levels, 23 bases, 1,227 walks, all pass. Seen in the game's renderer
(`pd_snapshot stage bases1 --at`): the hall through a wall base's three slits, by their portals.
**Found on the way:** a shell on a hall's floor storey (no pod is ever there) was still walked on
(`walk`); and in a column of three, two hatches centred on opposite walls of a room five cells deep
met in its middle, so the drop from above landed on the lower hatch's rail: consecutive hatches now
take opposite walls and opposite ends of them.

**Arenas** (imported 2026-11-22, the Autolevel group): `bases1` (an empty start: the sketch's three
in one hall, `bases::tests::three`), `bases2` (an empty start: a base two storeys up through a
wall, slits on both; one rising two storeys through the floor; one hanging two storeys through the
ceiling; a big one, 9 x 8, at catwalk height; and a pod beside them), `bases7` (seed 7, grand,
`bases: 1, big_pods: 1, platforms: 1`: three bases, a pod grown to 10 x 8). Recipes in
`crates/pd_import/levels/`. Nobody has played them.

**Rough, and next:** the slits are open (no glass; one line in `bases::slits` and the planner to
glaze them); a base's storeys take their names in order ("..., storey 2"), not by what they are;
the editor's room tool draws a base as its lot (the column's narrower storeys are not shown apart);
the ladder of a column goes up a wall that may also have slits (the band runs across it); a base
in a basin, spire or atrium is refused, not fitted; nothing is placed at a slit on purpose (the
placement rules know nothing of bases yet: a sniper's spot at a slit is the editor's).

## Editing generated levels: the roadmap (2026-11-23)

The user's ask (2026-11-23): make generated Complex-style levels much more customisable: move
things vertically, edit in 3D with a gizmo, add corridors between rooms, cycle the options for
catwalks and other connections, add and take away pillars, windows in pods and the like, worked
on over several sessions. The rule that shapes all of it: **every tool makes a recipe edit** (a
level is its recipe, never its geometry), so a new kind of edit in `autolevel` is the real work,
and a new way of making one (side view, gizmo, a cycle button) is cheap once the edit exists. No
seed's level may change (the answer keys), so every new edit is an override that a recipe
without it never takes.

Order of work, and where each stands:

1. **The side view and `height`.** Done (2026-11-23): "Blank levels", step 2, as built.
2. **Per-room features.** Done (2026-11-23): "Per-room features", below. The features that were level-wide dials (`platforms`,
   `galleries`, `holes`, `cellars`, `angled`, `flooded`, `shaped`, `sunken`, `bases`,
   `big_pods`) chosen for one room: forced on, forced off, or the seed's roll. One edit kind,
   applied where the roll is made, so it replaces the roll rather than adding geometry code.
   Shown on the room panel as Auto / On / Off rows, greyed with the reason where a room can't
   take it (as layouts are). Watch: some rolls draw from level-wide streams in a seeded level,
   so forcing one room can shift another's draw; an empty start keys them by room already.
3. **3D picking and a box gizmo.** Done (2026-11-23): "The box gizmo", below. (In `pd_edit`'s 3D view): a click picks the room under the
   pointer (PD's rooms are the generator's: the preview's room index gives the key); the
   selected room gets six face handles (the sides a `Rect`, top and bottom a `Height`) and move
   arrows (x, y by cells, z by storeys), drawn as a wireframe that follows the drag using
   `boxes::trial` alone (a millisecond), red with the refusal; the level is made again when it
   is let go, as on the plan. Plus a cutaway (ceilings hidden, storeys above N hidden) to see
   into tall halls. `pd_edit::gizmo` exists for items: look there first.
4. **Corridors drawn by hand, and their style.** Done (2026-11-23): step 1 the edit ("Joins
   through a doorway", below), step 2 the editor's part ("Picking the doorway", below): a
   doorway picked on each room's wall on the plan, in the side view and in 3D, the way cycled
   (auto / corridor / vent, and "straight"). L, stairs and a forced ramp are noted there and
   not built (they need new search state in the router).
5. **Per-part re-rolls and a variants strip.** Done (2026-11-23): "Re-rolls side by side",
   below. `reroll { room, n, part }` with a stream of its own per part (catwalks, ramps, cover,
   windows), so one part changes and the rest stays; five small pictures of re-rolls made in
   parallel, one click to keep.
6. **Single elements by place.** Done (2026-11-23): "Single things by place", below. `window { room, storey, at, as: glass|rail|none }`, `piece { room,
   at, as: pillar|crate|none [, size] }` or `piece { room, at, to }`, `drop { room [, at] }`,
   named by where they are, never by an index (indices shift with every remake); put after the
   generator's own, so no draw moves; one that does not fit is left out with why. On the plan,
   the side view, `pd_edit`'s Layout screen ("Put things") and its 3D view.
7. **The cascade shown.** Done (2026-11-23): "The cascade shown", below. ("Blank levels", piece
   5): after any change, what is new lit, what went a ghost, what moved an arrow, and a list in
   words, on the plan, the side view, the variants strip, the panel, `pd_edit`'s Layout screen and
   3D view, and the command line.
8. Lock one room and re-roll the rest; copy, paste and mirror a room with its choices; a room
   saved as a preset. Done (2026-11-23): "Lock, copy, paste, mirror and presets", below, and
   its "As built": `lock`, `rest`, `draws`, `mirror` edits, `autolevel::lock` and `copy`, the
   plan's padlock and paste ghost, the panel's rows (shared with `pd_edit`'s Layout screen), the
   command line.

### Per-room features (roadmap item 2, 2026-11-23)

- **The edit:** `Edit::Feature { room, feature, on }`, `feature` one of `level::FEATURES`:
  `shaped`, `flooded`, `sunken`, `cellar` (rolled with the rooms: `Stage::Lots`) and
  `platforms`, `holes`, `angled` (rolled by the room's furnisher: `Stage::Interiors`).
  `galleries`, `bases` and `big_pods` are not in it yet (they add rooms or storeys, not a roll
  per room), nor the drops and grates between rooms.
- **How it reaches the rolls:** `Recipe::effective()` is the recipe's options with its feature
  edits as `Options::forced` (not saved, as `start` is not: the edits are). `rooms_on`, `routed`
  and `furnished_as_drawn` make their stages with it. Each roll asks `o.chance(room, feature,
  level)` in place of the level's chance (on: `level.max(1.0)`, off: 0), and a stage that returns
  early at a chance of nothing goes on if `o.forces_on(feature)`. Every one of these rolls draws
  from the room's own stream (`{seed}/{key}/shape`, `/flooded`, `/sunken`, `/cellar`; the
  furnisher's per room), so **no other room's draw moves** (tested). A recipe with no feature
  edit borrows its own options: no seed's level changes, the keys pass. In an empty start the
  shape and the pit are still none unless chosen on.
- **What a room has:** `choices::Choices::features` (`Feature { name, has, chosen, cannot }`):
  `has` read from the level (cuts, flood, sunk, a cellar whose hall it is, platforms, pits,
  angled catwalks or islands); `cannot` from the room alone (a pod; a cellar only under a
  colossal, vast or tower on the ground; platforms, holes and angled only in a room of more than
  one storey; a spire, basin or atrium keeps its shape and floor). Each feature is a choice kind
  of its own (`choices::kind`), so `set` keeps one edit a room a feature and `clear` takes it out.
- **On the Layout screen** (`pd_edit::app::shape`, the room panel): a **Features** section, a
  row each in plain words (L or U shape, Flooded floor, Sunken pit, Cellar, Platforms, Platform
  drops, Angled walkways) with Auto / On / Off at the right, a dot where the room has it, On
  greyed with the reason where it can't, and a line naming any turned on that didn't fit. Reset
  room clears them too. **In the generator's panel:** "features (this room alone)", folded
  unless one is chosen, a row each with "level's chance / on / off" and what the room has.
- **On the command line:** `--feature ROOM@FEATURE@on|off|auto`; `--own ROOM@FEATURE`;
  `--choices ROOM` lists a line for each feature (has it or none, whose choice, why it can't).
- **Checked:** `tests/recipe.rs` (on: medium1 flooded, colossal's platforms, a cellar under vast,
  every other room's rolls as they were; off: one room kept dry among rooms flooded at a chance
  of 1; off where it wouldn't have been: the same level; an unknown feature; the file both
  ways); a window test (On is one edit, the level has it, "has: yes", the level's chance again
  takes it out); the choices text and lists. Seed 40 with six forced on: four took (vast's shape
  didn't, as it then has a cellar and a cellar's hall keeps its rectangle; colossal's pit didn't
  fit). Pictures read: the side view with the cellar under vast and colossal's platform, the
  Layout screen's Features rows.
- **Rough:** nobody has looked at the window. Why a feature turned on didn't fit is not said
  (only that it didn't): the rolls' own tests would have to give their reasons. Forcing a cellar
  renames the cellars after it (`cellar1`, `cellar2` go by order), so edits naming a later
  cellar can lose it. Platforms "on" is the level's count or one for each 100 cells, whichever is
  more; there is no "more" or "fewer" yet.

### The box gizmo and the cutaway (roadmap item 3, 2026-11-23)

- **Picking** (`pd_edit::boxgizmo::pick_room`): the ray from the pointer meets the level's tiles
  (floors and walls: PD's rooms are the generator's, numbered as its plan numbers them) or a
  room's top from under it (ceilings are no tiles where a head does not reach them); the room is
  the one whose box holds the point just before what it met, else just past it (a wall seen from
  outside), a pod before its hall. A corridor is no room's box: nothing is picked. With the
  cutaway, a ray from over the cut starts where it crosses it, rooms over it are not met, and
  with no ceilings a room's top is not met either. A click on a room selects it in the
  generator's panel (the Layout screen's room panel then has it); a click on nothing selects
  nothing.
- **The gizmo** (`boxgizmo`): the selected lot's box (`world_box`) as white edges, faint where
  walls hide them, its floor outlined on top; six face handles (a cube on each face's middle,
  1.2% of their distance across, so the same on screen), the sides red (x) and green (y), top
  and bottom blue; three arrows from just outside the box's south-west corner (x, y, up),
  their length as the item gizmo's, never more than the box is across. `pick` is the nearest
  handle the ray meets. A drag (`Grab`: the ray read along the handle's line,
  `gizmo::closest_t_on_axis`) is `change_by`: the side view's `sideview::drag` with the same
  part (a side an `End`, the top `Top`, the bottom `Bottom`, the arrows the `Body` along x, y or
  up), so it snaps as the side view does: whole cells, the nearest storey for a floor, the
  nearest ceiling line for a top, never under a storey tall. Sides are `Rect`, top and bottom
  `Height`, x and y `Move`, up `Pin`.
- **In the window** (`space::Space::gizmo`, after `over_view`): the hovered handle lit and the
  line over the view saying what a drag of it does; while held, the change is tried at each
  step with `Doc::try_change` (`boxes::trial` alone, a millisecond) and the boxes it would leave
  (the room's, and a hall's pods, or a room that would become its pod) drawn on top in amber,
  red with the refusal written over the view; let go, `Editor::change` makes it (the plan's
  status line says what, the preview is made again a moment later). `Space::overlay` is the
  mesh; the window draws it with `Draw3d` over the preview's BG (pd_render untouched).
- **The cutaway** (`preview::Cutaway`): "No ceilings" leaves out every drawn triangle that faces
  down (ceilings, catwalks' and decks' undersides); "Cut lower" / "Cut higher" draws only the
  storeys up to one (from under the highest ceiling line down to the lowest floor; past the top,
  every storey), triangles over its ceiling line dropped and those across it clipped
  (`Vert::lerp`). Only the drawing: `pd_import::geometry_drawn` / `autowalk::geometry_only_drawn`
  put the BG's triangles through it after the collision is made, so the tiles, a walk and a pick
  are the same. Changing it makes the preview again (0.7 s; "the cutaway is being made...").
- **Without a window:** `pd_edit --shot-generated OUT.png --select ROOM [--drag HANDLE:N]
  [--no-ceilings] [--up-to FLOOR] [--pick PX,PY]` (HANDLE one of west east south north top
  bottom x y z, N cells, or storeys for top, bottom and z; it prints what the drag comes to and
  any refusal; the camera over the room unless asked otherwise); `pd_edit --shot-ui OUT.png
  --seed 40 --room vast --3d [--no-ceilings]` (the window with the 3D view, the camera over the
  room from outside: `Space::overlook`). Pictures: `out/autolevel/gizmo/` (scratch).
- **Checked:** 3 in `boxgizmo` (the box against the preview's cells, each handle's change against
  the side view's, a pick and a drag along its line), one in `preview` (ceilings dropped, a wall
  clipped with its colour and texture place along its edge), two in the window's (egui events:
  a click picks vast, the east face pulled a cell is one `resize` made on letting go and nothing
  while held, the top up two storeys red with the refusal and nothing made, the x arrow three
  cells west refused (small4 would land in it), a click on nothing selects nothing; "No
  ceilings" makes a preview with the same tiles, "Cut lower / higher" step from the highest
  storey to the lowest and back). Pictures read: vast's gizmo from inside and from outside with
  no ceilings, a refused top drag in red, seed 40 cut to floor 1 with no ceilings from above,
  the whole Layout screen in 3D.
- **Rough:** nobody has looked at the window. Seed 40's rooms are packed close, so most drags
  of its halls are refused: try an empty start or a smaller room. A drag that is let go where
  it is refused does nothing but say why. A side and the top at once, or moving and lifting at
  once, are two drags. The gizmo's arrows sit at the south-west corner, which can be behind the
  camera from inside a room (they are drawn on top, so they are seen through walls). With the
  walker walking there is no gizmo. A cutaway change makes the whole preview again; the cut is
  not shown on the plan or the side view.

### Joins through a doorway (roadmap item 4, step 1, 2026-11-23)

- **The edit:** `Edit::Join` gains `door`, `to_door` (`recipe::Doorway { side, at }`: the
  wall, E N W or S, and the cell along it of the door's low end, x on a north or south wall, y
  on an east or west one) and `via` (one of `recipe::VIAS`: "corridor", "vent"; none: a
  corridor, failing that a vent, as ever). All three are left out of the file when not given,
  so every recipe there was reads and writes as it did.
- **How it reaches the router:** `route::Terms` and `VentTerms` gain `da` and `db` (None by
  default: the answer keys pass unchanged): `terminals` (a corridor's ends), `direct_at` (a stub
  through the wall between rooms a cell or two apart; `direct` is it with none) and the vent's
  ends keep only the doorway given. `recipe::join` checks the doorway is on its wall (else
  "medium3 has no doorway N 90 (its walls run 61 to 67)") and the way; "vent" goes straight to
  a vent (made even where the level's `vents` is 0), "corridor" never falls back to one; none
  fitting is "no corridor fits from medium3 on 0 to medium1 through the doorways asked for".
  `describe` words them ("join medium3 on 0 to medium1, from its east wall at 26, to its west
  wall at 27, by a vent").
- **Checked:** `tests/recipe.rs` (`a_join_takes_the_doorway_asked_for_or_says_none_fits_there`:
  every doorway on medium3's walls tried, each one made has its door exactly there, the rest
  refused with the message, the router's own door among those made; a doorway off the wall; a
  way that is none; a vent asked for is a vent; a corridor asked for where one is made anyway is
  the same level; the file both ways). The answer keys pass.
- **Not yet:** the editor's part (step 2, below); styles beyond corridor and vent (below).

### Picking the doorway (roadmap item 4, step 2, 2026-11-23)

- **What is asked:** `candidates::Asked { door, to, to_door, via }`: a doorway on the picked
  room's wall, the room at the other end (only joins to it are looked for, near or not) and a
  doorway on its wall, and the way. `candidates_asked(recipe, room, storey, &asked)` (and
  `candidates_within`, `routes`, which take it too) pass them to the recipe's own `join` on a
  copy, as ever, so what is previewed is what is made; each `Candidate` carries them, and its
  `edit()` is the `join` with `door`, `to_door` and `via` set. What can't be asked is an error
  up front (`Doorway::off`, shared with `join`: "medium3 has no doorway N 90 (its walls run 62
  to 68)"; "no room attic"; "by a tunnel?"). Nothing found is `Candidates::none_fits()`, the
  recipe's words: "no corridor fits from medium3 on 0 to medium1 through the doorways asked
  for". `Asked::default()` is the plain search, exactly as before.
- **The editor** (`autolevel_edit`): the pick is `(room, storey)` plus `Doc::asked` (`ask`,
  `pick_asked`; a search is for the pick and what was asked together, `doc::Pick`, so one
  started before a change of either is not taken). Another room picked forgets what was asked;
  the same room on another storey keeps it. `Editor::doorway(room, door, storey)` is the click
  on a wall, wherever it is made: on the picked room's wall, its doorway (on that storey, if a
  height was clicked and it takes doors there); the same one again lets it go; on another
  room's wall, only joins to that room through that doorway (the same again: every room near);
  with nothing picked, the room is picked with that doorway. `set_via`, `cycle_way(n)` (a row's
  way button: only that row's room, the next way round) and `any_room` are the panel's.
  - **The plan:** with a room picked, a click within a grip of a wall (the room tool's, at most
    0.45 of a cell, inside or out) is a doorway (`planview::wall_hit`, the picked room and the
    room asked for first); a cell's middle is still the room. The picked room's walls get a tick
    a cell outside (where cells are 8 px or more), the doorways asked for a thick magenta cell of
    wall with "door W:26" inside the room, the room asked for a frame and "to medium1 alone", and
    nothing found the refusal in red on a panel under the pick. All of it is drawn over the room
    boxes. The line under the plan says what a click on a wall does.
  - **The side view:** with "Join" in hand a click on a room's end wall is that wall's doorway at
    the section's row or column, on the storey the height is on (`sideview::wall_at`); a click on
    a room picks it on that storey (`Editor::pick_join`). The picked room is ringed, its ends
    ticked at the door's height, the doorways asked for drawn on their walls where the section
    cuts them (the other room's on each storey it takes doors on).
  - **The panel:** under the storeys, "doorway: its west wall at 26 [any]", "only to medium1,
    its east wall at 30 [every room]", "way: auto | corridor | vent"; each row of the list a way
    button; nothing found, the refusal in red.
  - **pd_edit's 3D view:** with "Join" in hand a click on a room selects it and picks it to join
    from, on the storey the click met (`boxgizmo::storey_at`); with a room picked, a click that
    meets the level within 0.45 of a cell of a room's wall (`boxgizmo::doorway_at`, pods never,
    from inside or out) is that doorway, on the storey of the height met (so a click on the
    ground floor's wall moves a pick on a catwalk's storey down to it). The doorways asked for
    are magenta panels on their walls, a cell wide and a door tall (`boxgizmo::doorway_box`,
    `Shown::doors`), drawn on top; the line over the view says what a click on a wall does.
- **Command line:** `autolevel_edit --pick ROOM@Z [--door SIDE:AT] [--to ROOM | --to-door
  ROOM@SIDE:AT] [--via corridor|vent] [--join N]` (each needs `--pick`); with `--shot` the plan
  or `--side` view draws them. `pd_edit --shot-generated OUT --door ROOM@SIDE:AT@FLOOR` draws a
  doorway panel; `--pick PX,PY` also says which wall's doorway a click there asks for.
- **Checked:** `candidates_go_through_the_doorways_and_by_the_way_asked_for` (the router's own
  door asked for is the same route; every doorway on medium3's west wall either has its door
  there and is made so, or says none fits in those words; vents only; both ends; the errors; the
  plain search unchanged); a window test (egui clicks: pick, the hover line, a wall, the other
  room's wall and again, "via vent", a row's way button, "every room", the same wall again, the
  side view's click, a join made through the doorway keeps it); the command line (made, saved,
  refused, a doorway off the wall); pd_edit's (the 3D click picks on the storey met, a wall
  spot the camera sees asks for its doorway, drawn; with the room tool a wall only selects).
  Pictures read: seed 40 with medium3 moved, W:26 to medium1 made and W:28 with E:30 refused on
  the plan; the side view along y = 30 with both doorways; vast's E:28 on 0 and 276 in 3D.
- **Rough:** nobody has looked at the window. The ticks are faint and lie under a selected
  box's grips; the doorways' words can sit on a corridor's key. A doorway is a cell: a wider
  door the router makes starts there. The egui tests found that the plan moves for a frame or
  two while the panel grows to its list, and that clicks a frame apart are a double click there
  (it frames the level again): a quick second click on a wall in the window may do that too.
- **Styles beyond corridor and vent:** what each would take of the router. "straight" (no turn)
  fits as an override, and is built: `via: "straight"` (`recipe::VIAS`' third) is the join's
  terms with a turn dearer than the whole budget and no limit on a run's length
  (`Terms::turn`, `straight: 0`), never a vent; "no straight corridor fits ..." where none does.
  It is a way button and a "way:" choice like the others (`a_straight_join_has_no_turn`: seed
  40's tower on 551, every one found has no turn in its centre line, the longest made as shown).
  An `Asked` via of "vent" still falls back as a vent; "corridor" and "straight" never do. "L" (one turn) needs a count of
  turns in the search's state (`route_with`'s heads are (x, y, z, dir, run)): a turn budget of
  its own, None as now. "ramp" (a ramp wherever the storeys differ, never a stair of stubs) is
  the router's ramps already; forcing one between rooms on the same storey needs a head that
  must climb and come down. "stairs" has no piece in the corridor kit: `stairs.rs` lays a room's
  ramps, not a corridor's. None of the three is built.

### Re-rolls side by side (roadmap item 5, 2026-11-23)

- **The edit:** `Edit::Reroll` gains `part` (`level::PARTS`: "catwalks", "ramps", "cover",
  "windows"; none: the whole inside, as ever). `Recipe::effective` puts the part re-rolls in
  `Options::rerolled` (as features go in `forced`; not saved), and `Options::reroll(room, part)`
  is the stream a part is on (the last edit for it). Unknown part: "reroll: a part is one of
  catwalks, ramps, cover, windows, not \"pillars\""; a pod is refused as the whole re-roll is.
- **Where each part draws, and how it is moved:** the room's attempt stream
  (`{seed}/{key}/{attempt}`) is shared by the layout draw (`build::layout`, a tower's ring or
  ends), `lay_pool`, `lay_decks` (a mezzanine's side, a basin's ring, bridges), `place_ramps`
  (the score's jitter) and `lay_cover` (crates, a notch or a cross wall). `Interior::part(name,
  lay)` lays a re-rolled part from `{seed}/{key}/{part}{n}/{attempt}` (under `reroll{m}/` if the
  whole room is re-rolled too) and first lays it on a throw-away copy from the room's stream, so
  the stream is left where the seed's level leaves it and **the parts after it draw the numbers
  they did** (they still fit themselves round what the part became: the decks are trimmed where
  a ramp lands, so a ramps re-roll moves deck edges too). The catwalks take the layout draw with
  them (`.../{part}{n}/layout/{attempt}`; drawn on the room's stream all the same) and a towering
  tower's storey shapes (`.../shape/{attempt}/{z}`). Windows (`windows::reroll`): a hall's own
  places are dealt out again among the slots they hold in the sorted list
  (`{seed}/{hall}/windows{n}`), and glass or rail is drawn from `.../windows{n}/{z}/{x},{y}` after
  the shared draw is made anyway; a place it takes can be a corridor piece's one window that
  another hall had (the only way a part re-roll reaches another room). With no part re-roll
  nothing of this runs: the answer keys pass unchanged.
- **What is drawn and what is not:** pillars (`colonnade`) and a lane's pillars stand by rule,
  so a cover re-roll of a room of "pillars", "lanes" or none with no shape changes nothing:
  `choices::Part::cannot` says so, as it does for a room with none of a part, and for a pod.
  `Choices::parts` (`Part { name, n, count, cannot }`); each part is a choice kind of its own
  (`choices::PART_KINDS`, "reroll catwalks", ...), so `set` keeps one a room and `clear` (and
  `--own ROOM@all`) takes it out.
- **Side by side** (`autolevel::variants`): `variants(recipe, level, room, part, from, count)` is
  the recipes of the next streams (the one it has skipped), each with the re-roll made as a
  choice; `made` makes their levels on threads (`side_by_side`, which `candidates::verdicts` now
  uses too); `part_of` is a part as text, to tell a re-roll that came out the same; `label`.
- **The strip** (`autolevel_edit::strip`): `Strip::start` makes five re-rolls' levels (as the
  editor's `Made`) on a thread; its tiles are re-rolls of the recipe with the room's re-roll of
  that kind taken out (`base`), so keeping one leaves the rest good and any other change closes
  it (`fits`). Each tile's picture is worked out once (`pic`: the plan's shapes, every storey,
  or the floor alone for cover, cut to the room's box and two cells round it). `was` is the room
  as it was when the strip was opened. `Doc::adopt` keeps a tile: its edit made as a choice and
  its level, already made, the one on show (nothing is made again).
  - **The generator's room panel:** a "re-rolls" row (inside, catwalks, ramps, cover, windows,
    each greyed with why a re-roll changes nothing), the strip under it: the first tile the room
    as it was ("now" or "before"), then five, the one it has now framed in amber, a re-roll that
    came out the same or makes no level dimmed with those words over it; a click keeps one
    (status "kept: colossal: catwalks 3", one step to undo); "more" (the next five) and "close".
    `Editor::vary`, `vary_more`, `keep_variant`, `strip_ui` (shared).
  - **`pd_edit`'s Layout screen:** under "Try another inside", "Try five": Inside, Catwalks,
    Ramps, Cover, Windows, and the same strip, tiles 88 by 72 laid in rows by hand.
- **Command line:** `autolevel_edit --reroll ROOM@PART@N` (0: the seed's own); `--variants
  ROOM[@PART] [--from N] [--keep N]` lists the five, numbered, and with `--shot FILE.png` writes
  the strip to `FILE.variants.png` (the room now, then each, captioned); `--own
  ROOM@reroll-PART`; `--choices` has a "part" line each. `pd_edit --shot-ui OUT --seed 40 --step
  layout --room colossal --vary catwalks` (opened once the level is made).
- **Checked:** `a_part_of_a_rooms_inside_is_rolled_again_and_the_rest_stays` (cover on vast made
  crates: its places move, decks and ramps don't, the same every time; ramps: what is laid before
  them stays; catwalks: colossal's change on some stream; no other room's inside moves for any;
  with the whole inside re-rolled too only the part moves; windows change on some stream, no
  inside moves; the file both ways; the errors; 0 is the same level); `variants` (the streams,
  the one it has skipped, each the level its recipe makes); the strip (five, kept one leaves it
  good, any other change stale, the PNG's size, a pod refused); a window test (egui clicks: open
  catwalks, five made, the third kept with its level adopted at once, the first tile has the room
  back, "more" is 6 to 10, "close", another change closes it, the steps to undo); the command line
  (made, kept, saved, `--from`, `--own`, the refusals). Pictures read: colossal's catwalks strip
  (3, 4 and 5 differ, 1 and 2 the same), vast's cover strip with crates, vast's ramps strip, the
  Layout screen with the strip open.
- **Rough:** nobody has looked at the window. Ramps re-rolls are hard to tell apart at this size
  (item 7 now lights what each changed on its tile). The first tile is missing if the strip is opened before
  the level is made. A room at the level's edge sits off-centre in its tile (the tile's spare
  width shows its neighbours). Pillars can't be re-rolled (they stand by rule): item 6 is where
  they are placed by hand.

### Single things by place (roadmap item 6, 2026-11-23)

- **The edits:** `Edit::Window { room, storey, at, as }`, `Edit::Piece { room, at, as | to, size }`,
  `Edit::Drop { room, at }` (the recipe table above). `recipe::put_of` makes each a `level::Put`
  (`Window`, `Stand`, `Shift`, `Drop`), and `Recipe::effective` hands them to the stages as
  `Options::puts` (as `forced` and `rerolled` go; not saved). With none, nothing runs: the answer
  keys pass unchanged.
- **Where each is put, after the generator's own** (so no draw moves; what follows fits itself
  round it):
  - **Windows** (`windows.rs`): the places are found as ever (`found`; `places(lv)` lists them
    for an editor: a corridor piece along a hall's wall a cell away, its 2 to 4 wall cells,
    `level` where the corridor is on the hall's floor); the level takes its own (the shared draw
    made as ever); then `put`: the window whose wall holds `at` on that storey is made glass or
    rail or taken out, else a place there is given one past `windows` and the one-a-piece,
    two-a-hall limits. A window is named by any cell of its wall; an editor names it by the
    first (`puts::click`).
  - **Pieces** (`furnish/cover.rs`): `lay_cover` is now `site` (the floor as cover sees it) and
    `lay_drawn` (the room's own shape, lane pillars, colonnade or crates, exactly as before), then
    `put_pieces`: a `Stand` takes out what overlaps it and stands where `stand_piece` allows it
    (on the floor, clear of the door aprons, ramps' feet, water and drops, on no other piece, a
    pillar's `reach` not 0, and `place`'s connectivity), else puts back what it took out where it
    was in `cover`; a `Shift` finds the piece over the cell (`piece_at`, never a shape's wall) and
    stands it at `to` the same way (its height kept for a crate, `reach` again for a pillar). A
    pillar is as tall as `reach` makes it; a crate 118. The ladders, rails and walks after it see
    it. A room under 9 cells or a pod: "too small to stand anything in".
  - **Drops** (`build.rs`): `put_drops` after `openings` (the grates are chosen first, so they
    stay): each takes the place of its room's drop (`drop_at`, the generator's terms; one drop a
    room) or takes it out; a cellar's hall keeps its hatch. A hand drop the lower room's furnishing
    blocks (`lands_clear`) or the upper can't be furnished round is left out with that reason.
  - Each left out is in `Level::unmet` (`Build::unmet`, `Interior::unmet`, `windows::windows`'s),
    with why.
- **What an editor asks** (`autolevel::puts`): `Tool` (glass, rail, no window, pillar, crate,
  take out, drop); `click(lv, tool, cell, storey)` is the edit a click makes (the room under the
  cell on the storey shown, `room_at`; a window by `window_at`: its wall, or the cell either side
  of it; a piece taken out by its low corner; a click on a drop takes it out); `drag` moves a piece
  by as many cells; `set` keeps the recipe short (a window or a room's drop replaces its earlier
  edit; a piece stood and then moved is one edit stood where it went, moved twice one move, moved
  back none, stood then taken out none); `clear` (a place's edits out); `listed` (each with
  whether it took).
- **`autolevel_edit`:** a third tool, **Things**, and under it a row of the seven; the line under
  the plan says what a click puts (or why nothing can be); a click puts it, a drag of a pillar or
  crate moves it ("let go: ..."). The plan frames each thing put in magenta (a line from where a
  moved piece was, a cross on one taken out), in red with "not put" where the level did not take
  it; a window tool dots every window place on the storey shown. The side view draws the cover
  (pillars to their height, crates), windows (glass a band, rail low and orange) and drops where
  it cuts them, frames the things put, and takes a click: the cell under it and the storey of its
  height (`sideview::place_at`). The panel lists "things put by place", each with an x (the
  generator's own again) and, in red, why one was not put. `Doc::put`, `clear_put`, `puts`;
  `Editor::click_thing`, `drag_thing`, `put`, `clear_put`.
- **`pd_edit`:** on the Layout screen, "Put things" beside "Join rooms" under "Change the plan",
  and under it the same row of seven and the list (`Editor::things_ui`, shared). In the 3D view,
  with it in hand a click puts what the tool puts where the ray meets the level
  (`boxgizmo::place_at`: the cell a hair past the surface, so a pillar's side is the pillar and a
  wall the cell behind its face, and the storey of the point met), and the line over the view says
  what a click there puts; the things put are framed on top, magenta or red
  (`boxgizmo::thing_box`, `Shown::marks`). `pd_edit --shot-generated OUT --things TOOL --put
  PX,PY` (the click, the level made again, the frames drawn); `--shot-ui OUT --seed 40 --step
  layout --things TOOL` (tall: `--size 1600x1500` shows "Change the plan").
- **Command line:** `--window ROOM@STOREY@X,Y@glass|rail|none`, `--piece ROOM@X,Y@pillar|crate|none`,
  `--nudge ROOM@X,Y@TX,TY`, `--drop ROOM@X,Y|none`, `--unput ROOM@X,Y`, `--things TOOL` (the tool,
  its marks on the shot) and `--click X,Y` (with `--storey`); each prints "put:" or "not put: ...:
  why".
- **Checked:** `tests/recipe.rs` (a window by any wall cell made rail, none, given where `windows`
  is 0, refused places and a rail on the floor with their words, the file both ways; vast's lane
  pillar taken out by either cell, moved where it fits and refused under a deck's edge, off the
  floor and where nothing stands, a crate and a pillar stood, one stood on another, a size, cells
  along the walls refused, no other room's inside moves; colossal's drop taken out, loft2's into
  vast at every spot either made with its walk or refused, refused where nothing is under it, the
  generator's kept); `puts` (clicks name things the same way, the recipe kept short, the list);
  two window tests (egui: the tool, the hover line, a crate clicked, a pillar taken out by its
  second cell, nothing there refused in red, a drag moves the other, a window made rail on 276,
  the list's x, undo; the side view's click names medium1's window on 276); the command line
  (put, not put, a click, saved and the same picture again, unput, the refusals); `pd_edit`'s
  (the 3D view: the hover line, a click on vast's floor is one `piece` edit, framed once made,
  the room tool's click only selects). The answer keys
  pass. Pictures read: seed 40's plan on the ground floor with a crate, a pillar moved and two
  refused; the side view along y = 30 with the crate, the rail and loft2's pillar; two crates
  clicked on vast's floor in 3D, framed; the Layout screen's "Put things" row
  (`out/autolevel/things/`, scratch).
- **Rough:** nobody has looked at the window. On the plan with every storey drawn a click finds
  the highest room over the cell (loft2 over vast): choose a storey first. A window named by a
  cell that is not its first (`--window` by hand) is a separate edit from the one a click makes.
  Only drops between rooms: a pit in a platform (`furnish::pits`) is not yet put by hand. A
  piece's size can't be changed but by `size` in the file. A drag shows no piece moving, only the
  line under the plan; in 3D there is no drag (a click only). The Layout screen's footer still
  gives the room tool's hints. Seed 40 with the default options has no window places at all (its
  corridors run along no hall a cell away): the window tools find nothing there.

### The cascade shown (roadmap item 7, 2026-11-23)

- **The diff** (`autolevel::cascade`): `diff(before, after)` is a `Cascade`, a list of `Change`
  (`owner`, `what`, `how`: New, Gone, Moved or Changed; `was` and `now`, the `Spot`s it is drawn
  at: a box of half cells, the storey whose plan shows it, its heights; `arrow` for a move;
  `words`). Things are matched by what names them for good, never by an index or a route key:
  - **rooms** by key: moved ("medium3 moved 1 east, 21 north", an arrow), resized, raised
    ("stands 276 to 512 (was 0 to 236)"), another kind, new, gone;
  - **corridors and vents** by their two rooms ("long-medium3"; the route keys are dealt out again
    on every remake): the same cells are no change; the rest paired by the cells they share,
    "corridor long-medium3 re-routed, -11 cells", "medium1-tiny4 is a vent now (was a corridor)",
    new, gone; the ghost is the cells it lost, the lit part the cells it gained;
  - **a room's inside, in its own frame** (`Frame`: a room moved whole is compared shifted by its
    move, storeys by their index, so what moved with it is no change): decks per storey
    ("vast's deck on 276 reshaped: +1, -1 cells"), ramps (a ramp gone and one new joining the same
    storeys paired as a move; the same corner, "laid another way"), cover (paired by kind,
    crate or pillar, within 8 cells, whatever the size: "a pillar moved from 41, 34 to 41, 32,
    1 x 1 now (was 2 x 1)"), pits, and in words its layout (only where a deck changed too: a
    re-roll can relabel colossal's ends as a ring with nothing built otherwise), shape, flood,
    angled walkways;
  - **windows** by hall, storey and wall ("medium1's window in its west wall on 276 is rail now
    (was glass)"), **drops** by their two rooms (a move with an arrow), **grates** by the room
    under them.
  - `Cascade::lines` is the list in words (a room's more than three of one kind counted in one
    line); `summary`, `rooms`, `of(room)`, `within(room, rect)` (a small picture's part).
- **Kept by the document** (`autolevel_edit::doc`): when a level arrives for a newer recipe than
  the one on show (an edit, an undo, a re-roll kept: `accept`, `adopt`), the one it replaces is
  kept (`Doc::before`) and the two diffed (`Doc::cascade`); the same recipe's level again keeps
  what was said. `look()` puts it on the plan's `View::cascade`.
- **Drawn:**
  - **the plan** (`planview::Pic::cascade`): lit green where a thing is now (a room's box framed,
    not filled), a dashed salmon ghost where it was, a dashed green arrow for a move, the change's
    number on a badge (`numbered`, as `lines` numbers them); with one storey shown, what is on it.
    The legend keys them; a `--shot` lists the lines under the legend (`cascade_rows`).
  - **the side view** (`SideView::cascade`): the same where the section cuts a spot, at its
    heights.
  - **the variants strip** (`strip`): each tile against the room as it was when the strip was
    opened (`Strip::was`, else the level on show), cut to the room and two cells round it
    (`strip::against`): what it changed lit on the tile, the lines on hover in the window, under
    the tile on the PNG and under its row in `--variants`' text (`Tile::changes`).
  - **the window's panel** (`Editor::cascade_ui`, shared with `pd_edit`): under the status line,
    "the last change changed N:" with a "show" switch (`show_cascade`: drawing on the plan and
    the side view; the list stays) and a three-line area scrolled for the rest, the same height
    whatever the change, so the controls under it never move once it is there; a click on a line
    selects its room ("vast selected: ...").
  - **`pd_edit`:** the same list on the Layout screen under "Your changes", and in the 3D view
    the cascade's boxes on top (`boxgizmo::cascade_boxes`, `Shown::cascade`: lit green, ghosts
    salmon).
- **Command line:** `autolevel_edit` says "changed (N):" and a line each ("  #1  ...", numbered as
  the picture numbers them) after every change it makes (choices, puts, removes, joins, a kept
  re-roll); for room changes, which come before any level, the recipe's own level is made first
  to compare with. `--no-cascade` says and draws none (the older tests pass it).
  `pd_edit --shot-generated ... --things TOOL --put PX,PY` says it and draws its boxes;
  `pd_edit --shot-ui OUT --seed 40 --step layout --nudge ROOM@X,Y@TX,TY` moves a piece once the
  level is made and shows the list.
- **Checked:** `tests/cascade.rs` (seed 40: a level against itself is nothing; a pillar moved is
  only vast's cover, one line, its arrow, and back again; medium3 moved re-routes its corridors,
  each named by its two rooms, and its own inside is no change; a window made rail is one line,
  none is gone); the document (`a_change_and_its_undo_each_say_what_they_changed`: the first level
  none, a put said, its undo the way back, the same level again keeps it, on the view); the
  command line (`the_cascade_is_said_after_each_change_and_drawn`: a put, a move, nothing, a
  choice; quiet with `--no-cascade`, another picture); the strip (a tile's changes are empty
  exactly when it came out the same); a window test (egui: a crate clicked, listed and drawn,
  "show" off draws less, a line's click selects vast, undo lists the crate gone). The answer keys
  pass (nothing in the generator changed). Pictures read (`out/autolevel/cascade/`, scratch):
  medium3 moved on the plan, all storeys and storey 0 (its ghost, the re-routed corridors, vast's
  ramp and pillar with arrows, the list under the legend); the side view through x = 41; vast's
  ramps strip (the moved ramp lit on each tile that moves it, "the same" dark); two crates in 3D;
  the Layout screen with the list under "Your changes".
- **Rough:** nobody has looked at the window. A seed change or a big move lists dozens of lines
  (the window shows three and scrolls). Ghosts of a re-routed corridor are only the cells it lost,
  so a corridor that moved one cell over reads as two thin strips. On vast's ramps strip, tiles 1,
  3, 4 and 5 say the same words and look the same: `variants::part_of` tells them apart by
  something the diff does not compare (the rails, or the order of things). The badges can sit on
  each other where changes are close. The 3D boxes are wire boxes only (no arrows, no numbers),
  and a put's lit box sits inside its magenta frame. Alternatives (blank levels' piece 3) don't
  carry their own list yet.

### Lock, copy, paste, mirror and presets (roadmap item 8: the design, 2026-11-23)

Three tools, each a recipe edit that a recipe without it never takes, so no seed's level changes.

**What a room's inside hangs on.** A room is furnished from its own streams
(`{seed}/{key}/dress`, `/{attempt}`, `/shape`, `/flooded`, `/sunken`, `/cellar`, `/pits`, ...),
laid to its doors. Measured on seed 40 (both `Options::python` and the defaults): with every
other room's whole inside re-rolled (`reroll` n, n = 1 to 4), vast, medium3, medium1 and tower
come through `cascade::diff` untouched for every n; colossal does not for three of the four: the
dead-end pass joins tower on 551 to colossal only while tower's own inside leaves it a dead end
(`Fix::Joined`), so a re-roll of tower takes colossal's door away, and its decks with it. A room's
doors are what a lock must pin, besides its streams. The pass's joins replayed as the recipe's
own `join` edits, through the doorways the route took (`door`, `to_door`, `on` the far storey,
`via` corridor or vent), give a level the diff can't tell from the seed's own (seed 40's three,
alone and together, both option sets).

**Lock.** `lock { room }`: the room is held as it is: its box, its streams, its choices and its
doors.
- `autolevel::lock::lock(recipe, level, room)` is the recipe locked: every corridor the dead-end
  pass made to or from the room becomes a `join` edit of the recipe's own (through the route's
  doorways), every cut the pass made to its decks a `cut` edit, and then the `lock`. So locking
  changes nothing (tested: the diff against the level before it is empty for every room of seed
  40). Refused for a pod (it is held with its hall) and a room that is not there.
- The dead-end pass with a lock (`Build::locked`): it makes no join to or from a locked room
  (`deadends::joins` naming one are passed over), cuts none of a locked room's decks, and leaves
  out no room with a corridor to one; such a dead end is left as it is. With no lock it is the
  pass it always was.
- Any edit after the lock that names the room is refused ("move: vast is locked: unlock it to
  change it"); the editors ask before they make it (`lock::refusal`). Unlocking takes the `lock`
  out; the joins it wrote stay (they are the level's corridors either way).
- What a lock cannot hold, and the cascade then says, naming the room: another room moved, sized
  or added onto a locked hall as its pod, a corridor re-routed through its wall by a change to
  its neighbours' boxes, a window or drop of a corridor or room beside it. None of these is
  refused; the editors mark a cascade line whose room is locked.

**Re-roll the rest.** `rest { n }`: every room that is not locked, not a pod, and has no whole
`reroll` of its own, is furnished from its `n`th other stream (`{seed}/{key}/reroll{n}/...`, the
one `reroll` n would give it: a room re-rolled by the rest is the room re-rolled alone). One a
recipe (the last wins); 0 is none. Applied at furnishing (`Options::rest`, filled by `effective`
as `rerolled` is). What it does not re-roll, and why: the lots and the corridors (a room's inside
is laid to its doors: a new floor plan round a locked room would bring it new doors, so nothing
of it could be kept; copy the room into another seed's level instead), windows and drops (drawn
level-wide). Tested on seed 40: each room locked, the rest re-rolled for n = 1 to 5, the locked
room absent from the diff (`Cascade::of`).

**Copy.** `autolevel::copy::Copy` (plain data, and a preset's body): what a room is, relative to
itself:
- `kind`; `size` (cells wide and deep); `storeys` (how many, counted on `storeys::LEVELS`: a paste
  stands as many storeys tall from the floor it is put on); the floor it was on (a paste's default);
- `draws`: the name its streams are drawn from ("40/vast"; the room's own `draws` if it was pasted
  itself), so a paste is dressed and furnished from the same numbers: its cover, shape cut, pillar
  and first choices come out the same; the rest is laid to its own doors;
- its choices: its `layout`, `cover`, `brightness`, `ceiling`, `reroll` (whole and parts) and
  `feature` edits;
- its puts (`window`, `piece`, `drop`), their cells relative to its low corner, a window's storey
  as a count of storeys from its floor;
- a hall's pods, each a `Copy` of its own placed relative to the hall;
- `mirror`: the axes it is flipped on.
`copy::copy(recipe, level, room)`; refused for a pod (copy its hall), a spire is copied whole.

**Paste.** `copy::paste(recipe, &Copy, at, floor) -> (Recipe, key)`: an `add` of a new key
(`boxes::new_key`) with the copy's kind, its box from `at` (the low corner), its floor and its
top that many storeys up, and `draws`; then each pod as an `add` of its own (with its draws), the
choices and puts renamed to it and moved by `at` (a window's storey from the new floor), and a
`mirror` for each axis. Refused as an `add` is: the overlap rule, the site, heights no room
stands at; the editors try it with `Recipe::rooms` (quick) as the pointer moves. `Edit::Add`
gains `draws` (left out of the file when not given: every recipe there was reads as it did), and
`Options::draws` (room -> name, filled by `effective`) is asked wherever a room's streams are
named (`Options::named(seed, key)`: "{seed}/{key}" unless drawn as another).

**Mirror.** `mirror { room, axis }` ("x": east for west; "y": north for south). What a room's
draws place by its own box is flipped about its middle: the shape's cut (an L's corner, a U's
side) and the sunken pit. Not its inside (laid to its doors, which are where the router put
them: a mirrored inside would not meet them). Mirroring a copy (`Copy::mirrored(axis)`) flips its
pods' boxes and its puts' cells and adds the axis; mirroring a room in place
(`copy::mirror(recipe, level, room, axis)`) is the same done to the recipe: its pods moved
(`move`), its puts' cells flipped, the `mirror` edit set (the same axis twice is none).

**Presets.** A copy saved: `{ "preset": 1, "name": "Pump hall", "from": "seed 40, vast", ...the
copy }` (`copy::Preset`, `preset::load`, `save`; unknown fields refused, as a recipe's). Where:
`pd_edit` keeps them in `crates/pd_import/levels/presets/<name>.json` (committed, beside the
recipes they come from; listed on the Layout screen); `autolevel_edit` reads and writes any path
(`--preset FILE`), its window a `presets/` folder beside the recipe it has open.

**Order of work:** (1) `lock`, `rest`, the pass's rule, in `autolevel` with tests on seed 40; (2)
`draws`, `copy`, `paste`, `mirror`, presets, with tests; (3) the editors: `autolevel_edit`'s plan
(a locked room hatched, a paste's ghost box following the pointer, red where refused) and panel
(lock, re-roll the rest, copy, paste, mirror, save as preset, the presets), `pd_edit`'s Layout
screen (the same in the room panel); (4) the command line (`--lock`, `--unlock`, `--rest N`,
`--copy ROOM`, `--paste X,Y@FLOOR`, `--mirror x|y`, `--save-preset FILE`, `--preset FILE`).

### As built (roadmap item 8, 2026-11-23)

- **The edits** (`recipe.rs`): `lock { room }`, `rest { n }`, `draws { room, from }`, `mirror {
  room, axis }`. `Recipe::effective` fills `Options::locked`, `rest`, `draws` and `mirrored` (not
  saved, as `forced` is not) and refuses any edit that names a room locked before it ("cover:
  vast is locked: unlock it to change it"). `draws` and `mirror` belong to the lots stage (the
  rooms' rolls), `lock` and `rest` to furnishing. With none of them nothing runs: the answer keys
  pass unchanged.
- **Where they reach the generator:** `Options::named(seed, key)` ("{seed}/{key}", or the
  room's `draws`) names the room's streams in `level.rs` (spiral, grow, shape, flooded, sunken,
  cellar, galleries) and in `Build::furnish` (dress, attempts, parts, pits); the planner's names
  and the bases keep "{seed}/{key}". `Build::furnish` gives a room not locked, not a pod and with
  no whole re-roll of its own the stream `o.rest`. The pass (`Build::join`) passes over a join
  naming a locked room; `shorten` leaves a dead end whose cut names one, or whose leaf has a
  corridor to one. `level::flipped` flips a shape's cut and a sunken pit for a mirrored room
  (before their fit is checked: a flipped cut that no longer fits takes another try).
- **`autolevel::lock`:** `lock` (the pass's `Fix::Joined` to or from the room as `join` edits
  through the route's doorways, `on` the far storey, `via` corridor or vent; its `Fix::Cut`s as
  `cut`s; then the `lock`), `unlock`, `rest` (one edit, in its place; 0 none), `locked`,
  `is_locked`, `rest_of`, `refusal` (an edit before it is made) and `guard(old, new)`: every
  editor change goes through it, and it refuses any change to the edits naming a room locked in
  `old`, its lock aside (so unlocking passes, and deleting a locked room, whose lock goes with
  it, does not).
- **`autolevel::copy`:** `Copy` (room, kind, size, storeys, floor, draws, choices, puts relative
  to the low corner, a window's storey a count up, `sizes` of pieces moved, pods, mirror axes),
  taken from the lots (a grown pod is grown again where it is pasted, from the same draws); a
  stood piece's size is filled in from the level. `paste` (keys from `boxes::new_key`, the
  copy's edits renamed and moved; checked with `Recipe::rooms`), `Copy::mirrored` (twice is
  itself), `mirror` in place (pods by `boxes::trial` moves, so twice gives the recipe back),
  `Preset`, `save_preset`, `load_preset`, `file_name`.
- **`autolevel_edit`:** every `Doc` change goes through `Doc::set` (the guard): `add`, `join`,
  `keep`, `cut`, `delete_edit`, `put`, `clear_put`, `clear_choice` now return a `Result`. New:
  `lock`, `unlock`, `locked`, `set_rest`, `rest`, `copy` (the clipboard), `set_clipboard`,
  `mirror_clipboard`, `try_paste`, `paste`, `mirror`. The plan: a locked room framed in gold with
  a padlock in its north-west corner (`View::locked`, a legend row); a paste on its way as
  dashed boxes for the room and its pods under the pointer, red with the refusal in the words
  panel (`Boxes::ghost`, `Ghost`). The generator's panel: a "Lock, copy, paste" fold in the Rooms
  section, open once anything is locked, re-rolled or copied (so the list of rooms keeps its
  place), and the whole panel now scrolls (with a room selected it ran past a 950-pixel window
  already: the room list's lower rows could not be reached). Its rows, shared with `pd_edit`:
  `Editor::lock_ui` (Lock, Copy, Mirror east-west, Mirror north-south, for the selected room),
  `rest_ui` ("the rest: the
  seed's own | another | back", the rooms locked), `clipboard_ui` (what is copied; Paste, which
  puts the room tool in hand and makes the next click on the plan a paste, the copy's middle on
  the cell, on the storey shown or its own floor; Flip east-west, north-south; Forget; a name and
  Save; the presets, a click copies one). Presets live in `Editor::presets_dir`: a `presets/`
  folder beside the recipe; `pd_edit` sets `crates/pd_import/levels/presets/`.
- **`pd_edit`'s Layout screen:** "Keep, copy, paste" under "Put things" (the rest and the
  clipboard), and the lock row under Reset room / Delete room.
- **Command line:** `--lock ROOM`, `--unlock ROOM`, `--rest N`, `--copy ROOM`, `--preset FILE`,
  `--flip x|y`, `--paste X,Y[@FLOOR]` (refused: drawn red where it would go, exit 1), `--mirror
  ROOM@x|y`, `--save-preset FILE[@NAME]`, in the order given after the things put, each saying
  what it did and (with the cascade) what changed.
- **Checked:** `tests/lock.rs` (every room of seed 40 locked: an empty diff, both option sets
  for the rooms the pass joins; colossal, vast, medium3 and tower locked and the rest re-rolled
  from 1 to 5: never in the cascade, while colossal unlocked loses tower's corridor on stream 1;
  a room the rest re-rolls is that room re-rolled alone; refusals and the guard; the file);
  `tests/copy.rs` (a room drawn as itself is the same level, as seed 41's only it changes; a copy
  pasted with its choices and crate, where it can't go, the height past the top; vast with its
  pods into an empty start; a copy mirrored and twice; a shaped room's cut flipped (an empty
  start's room: no seeded level of seed 1 to 39 has a shape, and seed 40's rooms can't take one)
  and vast's pods across, twice back; presets); a window test (egui: lock, the legend, a delete
  refused, the rest another with the locked room out of the cascade, unlock, copy, Paste and a
  click on the plan with the ghost's words and the line under the plan, Mirror north-south and
  undo, Save a preset, Forget, the preset's button); the command line (lock, rest, a refused
  cover, unlock, copy flip paste save-preset, preset paste, a refused paste drawn and exit 1,
  mirror, the errors). The answer keys pass. Pictures read (`out/autolevel/item8/`, scratch):
  colossal locked and the rest re-rolled (35 changes, none in it, the padlock), a refused paste
  of vast over itself, medium3 flipped and pasted at 12, 2, the Layout screen's rows.
- **Rough:** nobody has looked at the window. The padlock is 12 pixels: small on a big plan.
  Paste is on the plan only (not in the side view or `pd_edit`'s 3D view), and the 3D view does
  not mark locked rooms. A lock holds against edits naming the room, not against a change to a
  neighbour that re-routes a corridor into it or makes a room its pod (the cascade then names
  it); the dead-end pass's leaf rule leaves such a dead end as it is, which can leave a too-long
  dead end in the level. Mirroring flips the cut, the pit, the pods and the things put, not the
  inside (laid to its doors), so a mirrored room is mostly the same inside. A paste's things put
  by hand that name the generator's own pieces (taking one out, moving one) may find nothing
  there in the paste (its inside is laid to other doors): the level says "not put" with why.
  The room boxes cross out the generator's own bases as deleted (they have no lot): older than
  item 8, seen in these pictures with bases on.

### Sketching: the plan may break the rule for a while (2026-11-24)

The user's call (2026-11-24): every room change is allowed while editing, and checked as it goes
without blocking; drawing a room over another and then lifting it, or a spire drawn first and made
taller after, are a sketch put right in steps.

- **`place::Placed::problems`** lists every way the lots break the scatter's rule (each lot's
  first, in order, then each pair's), each a `Problem { rooms, words }`; `check` is its first, so
  every message and refusal there was is word for word as before. **`boxes::problems`** is a
  recipe's: those, else what `Recipe::rooms_on` still refuses (a pod with no way in, an edit
  naming a deleted room), with the rooms its words name.
- **`Doc::sketch`** (the window's room changes: `Editor::change`, the plan, the side view, the 3D
  gizmo, the room panel's buttons) keeps a change that breaks the rule; `Doc::change` (the
  command line's) still refuses it, so a scripted change that breaks a rule exits 1 as ever.
  What can't be made at all (a room that is not there, a box under 2 x 2) and a change to a
  locked room are refused either way. `Doc::problems` (per state of the recipe), `Doc::unready`.
- **While the plan has problems:** the recipe makes no level and the last one made stays on show;
  the plan draws the room boxes whatever tool is in hand, each problem's rooms filled red, the
  problems listed by them (`Boxes::problems`); the panel says "The plan has N problems: the level
  shown is the last one that worked" and lists them, a click selecting a room
  (`Editor::problems_ui`, shared with `pd_edit`'s Layout screen in place of "These choices make
  no level"). A room's inside, Put things, lock, copy, mirror and the rest's re-roll wait
  (`Doc::unready`: "the plan has 1 problem: put the rooms in red right first"), and `pd_edit`'s
  "Build the arena" is greyed. Undo steps through it all as through any change.
- **A new room's height:** `boxes::default_top` (the lowest that keeps the rule): two storeys for a
  spire, one for every other kind; a top chosen that is high enough is kept when the kind changes.
- **Checked:** `boxes` (every problem, each with its rooms, the first the trial's refusal); window
  tests (a drag onto spire1 kept as one edit with one problem naming both rooms, drawn and listed,
  lock refused while it stands, undone; a room drawn over medium1 kept, then pinned to 827 and
  good; a typed x landing on spire1; a recipe that starts broken draws its boxes and the
  problem with any tool; a spire's default height); `pd_edit`'s 3D gizmo (a top dragged into loft1
  and a move landing small4 in vast, each kept as a problem and undone). Picture: an empty start
  with a room inside another and a one-storey spire, both red and listed (`out/sketch/`).
- **Rough:** the problems' words are the generator's (cells and units). A room is listed once per
  problem, so one room in three overlaps shows three lines. A two-storey room drawn on 0 and
  pinned up keeps its `pin` beside its `add` (the scatter's top across the 512/551 gap is a unit
  off; older than sketching): the level is the same.

## Risks

- **A missed order dependence** shows up as a key mismatch deep in furnish. The cure is in
  "Where Python and Rust differ".
- **Float differences.** The layout uses floats only in costs (halves), sort keys and
  `uniform` draws that are rounded straight away. The polygon stage uses more, and may
  need the key compared with a tiny tolerance, or the Python changed to whole units.
- **The bake's look.** There is no exact key. Agree the tolerance and look at frames.
- **Two generators.** The Rust now does everything the Python did, and some things better (the
  dead-end measure, above; no deck under a ramp, in "What is left of Blender and the Python").
  Do not let both drift: new rules go into the Rust only, and the user decides when the Python
  is retired.
- **Two repos.** The keys are written by a script in the Blender repo into this one. A change
  here that moves a key must be seen as a diff under `tests/key/` and never be made by hand.
- **The bake follows Blender's arithmetic** (single-precision rays, a byte read back through
  `powf`). On another machine's maths library its test of the read-back fails; that is the
  signal, not a fault to tune away.
- **Simulants on these levels** route everywhere a player walks (all waypoints one part), but
  nobody has watched a match: six are thin on 82 x 63 m, and PD does not respawn into a hall an
  opponent is in.
