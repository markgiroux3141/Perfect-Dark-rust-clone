# Handoff: Felicity levels, the next round of variation

**Status (2026-11-17): all four items done**, one commit each on `autolevel-kits` (`b577a9c`
walkways, `dd35c14` second gallery and landings, `5f86f66` arches and the hall's bays, `e7f4e1c`
doorway waypoints). What each does, and what is left (doors held shut by simulants on both sides,
PD's own door logic; a decision for the user), is in `docs/AUTOLEVEL.md`, "Felicity". The brief
below is as it was written.

Written 2026-11-17 at the end of a long session; the next session starts cold. Branch
`autolevel-kits` (last commit `e97a8f0`, the sprawling family). Read `CLAUDE.md` (loaded),
then `docs/AUTOLEVEL.md`'s "Felicity" section (the whole of it: both families, room types,
stairs, circulation targets, the stall finding), then the code below.

## Where things are

- `crates/autolevel/src/felicity.rs`: the planner. `generate` picks a family once a seed
  (`SPRAWL` 0.7) and redraws until `circulation::TARGETS` are met. `sprawl()` grows corridors and
  big open rooms (`Kind::Open`, one to three storeys) from the hall, then a gallery with two
  stair rooms, then rooms; `block()` is the two-corridor layout. Both make a `Draft` (lots,
  wanted doorways, stair rooms, washrooms); `layout()` turns it, places doorways, fits stairs
  (`stairs::fit`), adds second ways in and room types; `level()` makes doors (their own random
  stream), the plan (pillars, the hall's platform), walks and views.
- `crates/autolevel/src/stairs.rs` (straight, L, U flights; their spans and rails),
  `circulation.rs` (the targets), `level.rs` (`Level`, `Doorway`).
- `crates/autolevel_build/src/felicity.rs`: the kit (materials, room `TYPES`, the face style
  over Complex's box carver `complex::plan_polys`).
- `crates/pd_import/src/autolevel.rs` (doors), `autonav.rs` (waypoint repair from the
  generator's walks), `crates/pd_import/src/waypoints.rs` and `pd_sim::nav` (the waypoint maker).
- Showcase arena: seed 10, code `felicity10`, stage 127 (`crates/pd_import/levels/felicity10.json`).

## The work (the user's list, in this order)

1. **Balconies and walkways inside the tall rooms.** Open rooms two or three storeys tall
   (and the 7.8 m hall) have nothing up high. Add a walkway or balcony along a wall (or across)
   at a storey line (283, 566), railed (the kit's `rail`; `Room::rails` and `Level::ramp_rails`
   as the stairs use them), reached by stairs in the room or from a doorway on an upper storey.
   A walkway is a solid filled back into the room's air (as the stairs' landings are), its top
   a floor; its underside and sides need dressing in the kit's style.
2. **More than one upstairs area, with stairs into the tall rooms.** Today one gallery over one
   corridor. Allow a second (or an upper level off a tall room's walkway), each reached two ways.
3. **Arched doorways and the hall's bays.** Felicity's hall is three bays behind arched walls.
   The carver makes axis-aligned boxes only (`crates/autolevel_build/src/csg.rs`): an arch needs
   a new piece (faces under a curved or stepped top in a doorway or a bay wall), dressed and
   tiled like the rest. Look at Felicity's own (`PD_PROBE_STAGE=mp11 PD_PROBE_OUT=tris.json cargo
   test -p pd_import --release probe_stage_triangles -- --ignored`) before choosing a shape.
4. **The simulant stalls.** `PD_PROBE_STAGE=felicity10 cargo test -p pd_import --release
   probe_stalls -- --ignored --nocapture` lists each 3-s stall: on seed 10 all six are at one
   doorway in `upper1`, where a waypoint stands 10 cm from the wall in the doorway's mouth and
   chrs coming along the wall catch the jamb. The fix belongs where waypoints are placed or
   repaired, and that is shared with every generated level and custom import: keep Complex's and
   the other arenas' results unchanged, or say what changed and why.

## Rules that must hold (the acceptance for every step)

- **Complex unchanged**: `cargo test --release -p autolevel -p autolevel_build -p
  autolevel_edit -p pd_import` passes (the answer keys under `tests/key/` never change), and
  `target/release/autolevel.exe build 40` is byte for byte what it was (build it to a scratch
  folder before you start; compare every file after).
- **Felicity**: `cargo test -p autolevel --release` passes (it includes
  `felicity_meets_the_circulation_targets` on seeds 1-60); every walk passes on seeds 1-40
  (`cargo build --release -p pd_import`, then `target/release/autolevel.exe build N --style
  felicity --out <dir> --no-bake`, last line "X of X walks as expected"). New features get walks
  (up every new stair, along every walkway) and tests.
- The two random streams (`<seed>/felicity` layout, `<seed>/felicity/doors`), the doorway
  contract (a box through a 40 wall on a grid line, a half each side, 106 or 212 wide for doors,
  212 tall; one doorway per pair of rooms), `Plan::check` passing.
- rustfmt on `autolevel`, `autolevel_build`, `autolevel_edit` only. `--release` always.
- **Never run `pd_edit` or the game without shot arguments** (they open the user's windows).
  Look with: `target/release/pd_edit.exe --shot <code> out.png --at X,Y,Z,yaw,pitch` (an
  imported arena; from above: Y 4000-6000, pitch -89; ceilings face down), `cargo run --release
  -p pd_tools --bin pd_snapshot -- <out> stage <code>`. `pd_edit --shot-generated --plan` draws
  Complex's pipeline, not Felicity's (the editor doesn't do Felicity yet).
- Importing: `target/release/autolevel.exe import <dir> --code felicity10 --name "Felicity 10"
  --stagenum 127` (reuse that code and number: importing on a taken stage number drops the
  other arena from the menu).
- Commit per step on `autolevel-kits` (the attribution line from the system reminder); update
  the "Felicity" section of `docs/AUTOLEVEL.md` as you go. Hand off playtests to the user with
  `cargo build --release` and a short brief of what changed and what to look at.
