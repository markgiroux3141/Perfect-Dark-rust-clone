# Handoff: the Felicity planner, version 2

A brief for one session, written 2026-11-17 by the session that built version 1. It is
self-contained: read it, then the files it names, then do the work. When you are done, report
back in the form under "What to hand back"; the user will carry it to the other session.

## The goal in one paragraph

The level generator makes Felicity-style arenas from a seed (`autolevel build 7 --style
felicity`). Version 1 works end to end (polygons, lighting, doors, stairs, walks, import into
the game) but its **layout** is wrong for play. In the user's words, after playing it: the level
is "a bit more of a maze than a level"; it should be "more like corridors and have rooms as
offshoots, not very complex webs of rooms nested deep. We can have some rooms like this." Your
job is to rewrite the layout part of the planner, `crates/autolevel/src/felicity.rs`, so that
its levels meet measurable circulation targets, while keeping everything around it working.
This is design work: how corridors, halls, stairs and rooms are arranged. Everything below
the layout (geometry, materials, stairs geometry, doors, import) is done and must keep working.

## Read first

1. `CLAUDE.md` (loaded for you): the repo's rules. The decomp rules don't apply to the
   generator crates (they are not PD ports), but these do: never drive the game or editor
   windows; `cargo` always with `--release`; rustfmt on `autolevel`, `autolevel_build`,
   `autolevel_edit` only (`cargo fmt -p autolevel`).
2. `docs/AUTOLEVEL.md`: "Decisions" (top), and the whole "Felicity" section near the end.
3. The code, in this order:
   - `crates/autolevel/src/felicity.rs`: version 1, the file you rewrite. Read all of it: the
     constants, `Lot`, `Gap`, `gap()`, how doorways become `Doorway`s and half boxes, the door
     stream, the plan, the walks, the views, the `Level` it returns, its tests.
   - `crates/autolevel/src/circulation.rs`: the measures and `TARGETS` you must meet.
   - `crates/autolevel/src/stairs.rs`: `fit()`, the three stair shapes and their spans.
   - `crates/autolevel/src/level.rs`: `Level`, `Doorway`, `DoorKind`, `Options` (`style`).
   - `crates/autolevel/src/plan.rs`: `Plan`, `Room`, `Plan::check` (what it refuses).
   - `crates/autolevel_build/src/felicity.rs`: the kit (read only): room types `TYPES`, which
     floor, walls and ceiling each gets, and how faces in doorways, steps and landings are
     dressed.
   - `crates/pd_import/src/autolevel.rs`, `doors()`: how a `Doorway` with a door becomes a
     door in the game (read only).

## The task

Rewrite the layout in `felicity.rs` (the "building", "doorways" and "plan" stages; keep or
adapt the door, walk and view stages) so that **circulation comes first**: a network of
corridors, halls and stair rooms that reach each other directly, with rooms hung off it.

What the user wants, and what Felicity has, to design towards (not a prescription; you choose):

- **Corridors** (kind `corridor`, 2 to 4 cells wide) as the main routes, joined to each other
  and to halls without passing through a room. Shapes to consider: a ring, a T or H, two
  parallel corridors joined at their ends, a corridor that bends. Loops through the circulation
  itself are good; dead-end corridors short.
- **Rooms as offshoots** of the corridors, on both sides, mostly one doorway from circulation;
  a few rooms off rooms (depth 2) are fine, nothing deeper. Some rooms with two doors (onto two
  corridors, or a corridor and a neighbour) make loops.
- **The hall** (kind `hub`, type `hall`; 7.8 m tall, its platform and pillars as in version 1)
  as a node of the circulation, reached from at least two places.
- **Stairs**: stair rooms (kind `stair`) placed where corridors meet or end, sized and oriented
  so that `stairs::fit` gives a shape that suits what they join (the straight flight, the L, the
  U: version 1 only ever fit them into fixed 8-cell rooms, so the U was rare). The upper storey
  should also be circulation with offshoots (an upper corridor or gallery with rooms), reached
  by two or more stairs so it is not a dead end.
- **Room types** (`scheme`; the kit's `TYPES`): assign by where a room is and how big it is,
  not at random. Felicity's own layout, measured: offices (`office`, `marble`) are most of the
  building, along its hallway; the washrooms (`washroom`) are small rooms together; the
  industrial rooms (`industrial`, grey) are a larger room and a corridor; `hallside` rooms sit
  off the hall. Version 1's `room_type()` is a placeholder.

## Acceptance (all of these)

1. `felicity_meets_the_circulation_targets` (in `felicity.rs`'s tests; now `#[ignore]`) passes
   on seeds 1-60 with the `#[ignore]` removed. It prints each seed's measures:
   `cargo test -p autolevel --release felicity_meets_the_circulation_targets -- --ignored --nocapture`.
   Don't loosen `circulation::TARGETS`; if one target seems wrong for play, say so in your
   report instead.
2. The other Felicity tests pass (`cargo test -p autolevel --release`: the whole planner crate,
   about 20 s, the Felicity, stairs and circulation tests among them). You may rewrite them for the new layout, but keep what they hold: a seed makes
   the same level twice, its plan passes `Plan::check`, it is one piece, the door shares (about
   0.8 from circulation into a room, about 0.33 between rooms, none into small rooms), every
   stair shape is used, stairs rise a storey in 12 risers.
3. **Every walk passes on seeds 1-40**, by PD's own player in this process:
   `cargo build --release -p pd_import`, then for each seed
   `target/release/autolevel.exe build N --style felicity --out out/autolevel/fN --no-bake`
   (the last line says "X of X walks as expected"). Walks must cover every doorway, each room
   door to door, and each flight of stairs (version 1's walks show how).
4. **Complex is untouched**: `cargo test --release -p autolevel -p autolevel_build -p
   autolevel_edit -p pd_import` all pass (the answer keys under `tests/key/` must not change),
   and `autolevel build 40` is byte for byte what it was (build it before you start and compare).
5. One seed imported and looked at: `target/release/autolevel.exe import out/autolevel/f7
   --code felicity7 --name "Felicity 7" --stagenum 127` (that code and stage number are this
   level's; reuse them), and the import report shows every door closing its portal and every
   waypoint in one part.

## Rules for the code

- Change `crates/autolevel/src/felicity.rs` and its tests. You may add small pure helpers to
  `stairs.rs` or `circulation.rs` (with tests); don't change `TARGETS`, the kits, Complex's code
  or the importer. If you truly need something outside, say why in the report.
- Keep the two random streams: `<seed>/felicity` for the layout, `<seed>/felicity/doors` for
  doors (a change to how doors are chosen must never move a room).
- Keep the doorway contract (the kit, the importer and the door placement rely on it): a
  `Doorway` is the box through a wall 40 thick, centred on a grid line; `a` the low side, `b` the
  high; each room gets its half as an extra air box, so the rooms meet on the wall's middle
  plane (the portal, where the door stands); 106 wide (one cell, one leaf) or 212 (two leaves),
  212 tall; at least a cell of wall at each end of the shared wall. One doorway per pair of
  rooms (`Plan::check` refuses a link declared twice).
- Rooms are bare boxes on the 106 grid (walls 40 thick on the grid lines, `Lot::air`), storey
  283, rooms 259 tall, the hall 780. Plan units are cm at the importer's scale 1.
- The `Level` must still carry what the rest reads: `plan`, `doorways`, `walks`, `views`,
  `ramp_rails` (the stairs' sloped rails), `report` (version 1 fills the basic fields), the
  rooms as `Space`s for `net`.
- Comments as the repo writes them: a `//!` paragraph at the top of the file saying what it
  makes and why; plain sentences.

## How to look at a level (no window)

- Its plan as the game draws it, from above, after importing it:
  `target/release/pd_edit.exe --shot felicity7 out.png --at X,4500,Z,0,-89` (X, Z: the middle
  of the level in PD's coordinates, x = plan x, z = minus plan y). Ceilings face down, so you
  see every floor. **Never run `pd_edit` without `--shot` arguments**: it opens the user's window.
- One room's view, with no import: `target/release/pd_edit.exe --shot-generated out.png
  --recipe out/autolevel/fN/recipe.json --view <room key>` (the views are `Level::views`).
- From the spawn pads, after importing: `cargo run --release -p pd_tools --bin pd_snapshot --
  out stage felicity7`.
- `out/autolevel/fN/plan.svg` is the plan's boxes, written by every build.

## What to hand back

Commit on the current branch (`autolevel-kits`), message ending with the repo's attribution
line, then report:

1. The design in a few sentences: how circulation is laid out, how rooms, stairs and types are
   placed, what varies by seed.
2. The targets over seeds 1-60: for each measure its mean and worst seed (the test's output
   summarised), against version 1's baseline (all 60 missed; rooms right off circulation 64% on
   average; rooms up to 5 doorways deep).
3. Walks on seeds 1-40 (all must pass), the stair shapes' counts, the room types' counts.
4. Paths of two or three images you looked at, and what you saw.
5. What you didn't do, and anything you think the user should decide.

Update `docs/AUTOLEVEL.md`'s "Felicity" section to describe version 2 (replace what no longer
holds). Stop when the acceptance is met; polish beyond it is the next session's.
