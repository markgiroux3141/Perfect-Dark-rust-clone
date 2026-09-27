# SPIKE step 2: PD bots on Complex, our waypoints vs PD's

Follows `SPIKE_PD_SIMULANTS.md`. Same branch (`spike/pd-simulant`) and binary (`pd_arena`).

## The question

Can simulants find each other in a real Perfect Dark multiplayer level when they route with **our** waypoint graph instead of PD's hand-authored one?

"Find each other" is measured below. It covers:
- every spawn can route to every other spawn;
- bots make first contact about as fast as they do on PD's own graph;
- nobody gets stuck;
- bots use the whole level, including stairs and upper floors.

The test is only fair if the graph is the one thing that changes. So PD's original graph is loaded too, and it runs through the same ported routing code as the baseline.

## What the decomp already gives us (checked)

Complex is stage `ref` (`STAGE_MP_COMPLEX`, `stagetable.c:22`). Everything needed ships as JSON in `reference/pd-decomp/src/assets/ntsc-final/`, so there's no ROM parsing:

| file | contents |
|---|---|
| `tiles/ref.json` | **Collision geometry**: 1,208 polygons (170 triangles, 1,037 quads, 1 pentagon) in **45 rooms**. Each carries `GEOFLAG`s: 350 floor tiles (`FLOOR1`/`FLOOR2`) and 858 wall tiles (`WALL`), plus `BLOCK_SIGHT` / `BLOCK_SHOOT`, 3 `aibotcrouch` and 2 ladders. Units are cm: x −5053…−643, y −276…748, z −1956…1943, i.e. **about 44 × 39 m on several floors** (0, ~2.7 m, ~5.3 m, and a −2.8 m pit). Stairs are ramp tiles at 15–35°. |
| `pads/ref.json` | 226 pads (pos, look, AI flags), **144 waypoints** (pad, neighbour list with two direction-disable flags, waygroup), **20 waygroups**, 101 cover points. This is PD's own nav graph. |
| `src/setups/mp_setupref.c` | 19 MP spawn pads (`intro[]`: `spawn(PAD_REF_001C … 002E)`), plus weapon and ammo pads. |
| `game/padhalllv.c` | The routing code: `waypoint_find_closest_to_pos` (room-based candidates plus sight checks) and `nav_find_route`, which routes waygroup-to-waygroup and then waypoint-to-waypoint with a seeded random tie-break. About 350 lines of C, and it can be ported verbatim. |

**Not available:** the textured display geometry. `bg_ref.seg` holds compressed display lists and we have no BG decoder. The spike draws a **greybox built from the collision tiles**, which is what bots actually "see" anyway. Textures are out of scope.

**Found while researching (already fixed on the branch):** `nav_find_route` counts its NULL terminator, so PD's `numwaypoints > 1` means *at least one* waypoint. The first spike build required two, which made close-range go-to orders fail.

## Design

### 1. A `Level` abstraction

`Sim.arena` becomes a trait object with two implementations: the current box room (kept for regression tests) and `TileLevel`. The trait covers what the bot code asks of the world:

- `los(a, b)`: blocked by `BLOCK_SIGHT` tiles (`chr_has_los_to_chr`).
- `raycast_shoot(o, dir)`: blocked by `BLOCK_SHOOT` tiles (`chr_shoot`).
- `ground_at(x, z, from_y)`: the highest floor tile under the cylinder, no more than a step above the current ground. PD probes from `manground + 69` (`chr_update_position`), so steps up to 69 cm are climbable.
- `push_pos(old, new, radius, y_range)`: the cylinder against wall tiles, taken as vertical-ish segments with a y-extent, sliding along them. This stands in for `chr_calculate_push_pos`.
- `room_at(pos)`: the room of the floor tile underfoot.
- `room_neighbours(room)`: inferred from rooms whose tiles share an edge. Portals aren't in the tiles JSON.
- the render mesh.

At ~1,200 polygons, brute force with a uniform xz grid is fast enough. Chr `pos.y` becomes real: ground height, with gravity when stepping off a ledge (a simple fall, not `projectile_update_fall`).

### 2. Rooms, now that there are 45 of them

Rooms change behaviour, so they're in scope:
- **`bot_is_about_to_attack`:** the same-room / neighbour-room / last-seen tests stop being always true. Out of sight in a far room, a bot now faces its travel direction instead of its target.
- **Spawning:** passes 1 and 2 (pads more than 10 m from enemies and not in an enemy's room) start taking pads. In the one-room arena every pad was "very bad".
- **`waypoint_find_closest_to_pos`:** candidates come from the chr's room plus its neighbours.

### 3. Two graphs, one routing code (`PD_NAV=pd|ours`)

- **PD graph, the oracle.** Load the waypoints and waygroups from `pads/ref.json`. Port `nav_find_route`, the group step discovery, `waypoint_collect_local`, the neighbour choice with `nav_set_seed`/`CHRNAVSEED`, and `waypoint_find_closest_to_pos`, verbatim. The neighbour direction flags (`WPSEGFLAG_*`, the JSON's `flag4000`/`flag8000`) must be honoured.
- **Our graph, the replacement under test**, generated from the tiles:
  1. Sample floor tiles on an xz lattice (100 cm). Keep a sample if it has 185 cm headroom and is at least radius + margin from wall tiles.
  2. Link lattice neighbours whose ground height changes ≤ 69 cm per step along a swept cylinder. Ramps pass; drops are one-way.
  3. **Sparsify to PD-like density** with a greedy cover: choose nodes until every sample is within ~3 m, and in sight, of a node. Connect nodes whose corridor is walkable. The target is roughly PD's 144.
  4. **Emit PD's own format:** waypoints plus waygroups, with groups clustered per room or per connected patch. `nav_find_route` then runs unchanged on both graphs, so the graph is the only variable.

   This is a surface-sampling generator, and that fits here: the tiles are real collision surfaces, pre-classified as floor and wall. The Recast attempt failed on *our render mesh*, which is a different input (see the `recast-navmesh-crates` memory).

### 4. Viewer additions

- Level switch (`PD_LEVEL=arena|complex`).
- Greybox drawn with floors shaded by height and walls darker, an optional per-room tint, and a clip plane or "hide upper floors" toggle so lower floors stay readable.
- Overlays:
  - PD graph in yellow, ours in blue, drawn together;
  - the selected bot's current route;
  - spawn pads and cover points;
  - a position heatmap.

## Test plan

### Static checks (headless, no bots)

| id | check | pass |
|---|---|---|
| S1 | **Coverage:** every PD waypoint pad and every spawn pad has one of our nodes within 150 cm, on the same floor, in sight | all 144 + 19 |
| S2 | **Connectivity:** our graph has the same connected components as PD's | 1 component (or PD's count) |
| S3 | **Route agreement:** all 19 × 18 spawn-pair routes exist under both graphs; compare path length ours/PD | none missing; median ≤ 1.15, worst ≤ 1.5 |
| S4 | **Walkability:** a scripted walker traverses every edge of our graph with the real movement code | every edge arrives within 2× nominal time, no falls, no snags |

### Dynamic checks (bots, same seeds on both graphs, ≥ 10 seeds × 3 minutes)

| id | metric | pass |
|---|---|---|
| D1 | **First contact:** time from spawn until each bot first has a target in sight | ours within 20% of PD's median |
| D2 | **Engagement:** kills per minute, % of time with the target in sight, % of time in ADVANCE/GOTO | within 20% of PD |
| D3 | **Stuck:** 1-second re-path events per bot-minute; GoPos with no displacement for more than 3 s; ledge falls | ours ≤ PD on each |
| D4 | **Coverage:** share of the level's floor cells visited, per floor | every floor visited; ours ≥ 80% of PD's coverage |

Plus a playtest handoff: watch both graphs side by side on the same seed.

## Stages

1. **Load and look.** Tiles JSON → `TileLevel` → greybox, camera, level switch. User check: does it read as Complex?
2. **Walk it.** Ground, steps, ramps, falls and wall sliding, with a scripted walker driven to clicked points. Headless test: walk every PD waypoint edge.
3. **PD's baseline.** Rooms, the PD graph and the verbatim routing port; bots fight on Complex with their original graph. This is the reference behaviour, and a milestone to show on its own.
4. **Our generator.** Build the graph and run static checks S1–S4, iterating until they pass.
5. **A/B.** Dynamic checks D1–D4 in a headless harness (a `probe`-style ignored test that prints a report table), then the playtest.

## Risks and open questions

- **Corridor snagging.** Bots steer in straight lines between points; PD's `chr_nav_tick_main` has an "expensive" mode that steps around obstacles, including other chrs. In Complex's corridors that could matter. Port it only if D3 shows snags on *both* graphs.
- **Room adjacency from shared edges** may miss rooms joined through a doorway with no shared floor edge. Validate against PD's own waygroup neighbour list, which implies which rooms connect.
- **Odd tiles:** 18 floor-flagged vertical tiles (probably step risers). Complex carries **no** `RAMPWALL`, `STEP` or `SLOPE` tiles (`flag0080`/`flag2000`/`flag0100` are false on all 1,208), so ramps are just tilted floors. If another stage has `STEP`, treat it as climbable regardless of slope, as PD does.
- **No pickups, still:** bots get their loadout on spawn. On a real map PD bots would path to weapons first, which changes early movement. Accepted for this question; a later stage could port pickups using the weapon pads.

## Status

### Stage 1: load and look (done; visual check passed)

Run with `$env:PD_LEVEL = "complex"`, or pick the level in the panel. `PD_DECOMP_DIR` overrides where the decomp is read from (default `reference/pd-decomp`; nothing PD-derived is copied into the repo).

- `level_geom.rs`: the generic input (`LevelGeom`/`GeomPoly`). Each polygon has an outline, floor, wall, blocks sight, blocks shots, an optional room, and since stage 2 also ladder, crouch and duck. The generator will only ever see this type.
- `pd_tiles.rs`: the PD adapter. Tiles → `LevelGeom`; pads (with their `PADFLAG_AI*` bits), waypoints and waygroups (neighbours in PD's `id | WPSEGFLAG_*` encoding), cover, and the `intro[]` spawn pads.
- `greybox.rs`: draws any `LevelGeom`.

**Measured (corrections to the recon above):**
- Floor orientation must come from `|normal.y|`: 19 floor tiles are wound downwards.
- 41 wall tiles block neither sight nor shots (`flag0004` only).
- **Pad `y` is not ground.** PD places pads 52–121 cm above their floor, and some sit on a railing line or a walkway edge, just off their tile. The pit pads sit 118 cm above the *rim*, not the pit floor, so pad height is no guide there.
- Flat floors sit at −280, 0, 280 and 510/550 cm.
- `waygroup.waypoints` in waypoint index order is PD's real order: `tools/assetmgr/mkpads:40-46` builds it that way, and the decomp matches the ROM.

### Stage 2: walk it (done)

`tile_level.rs` ports the `lib/collision.c` primitives the chr movement code calls onto generic polygons:
- the wall volume tests (`cd_volume_collect_tilei`, `cd_test_volume_simple` / `_closestedge`);
- the swept centre-line test (`cd_is_cylpath_intersecting_tilei` under `cd_test_cylmove_*`);
- ground finding (`cd_find_ground_at_cyl_ctfril` + `cd_find_ground_finalise`);
- `cd_find_ladder` and `is_cyl_touching_tile_with_flags`;
- other chrs as `GEOTYPE_CYL` perimeters.

On top of those, `chraction.rs` now ports `chr_calculate_push_pos` (slide along the hit edge, else round its end, else refuse), the ground half of `chr_update_position` (69 cm step probe, low-pass step-up, gravity fall, ladder climb, duck/crouch heights), `chr_ascend`, `projectile_update_fall`, `chr_prop_can_move_to_pos_without_nav` and `chr_adjust_pos_for_spawn`.

**The arena is now the second adapter** (`Arena::geom`). Its boxes become polygons, so both levels run the same ported collision code, and the old push-out stand-in is no longer on the simulation path. This replaces the planned `Level` trait: one implementation is enough once the arena is geometry too.

Substitutions, marked at the call sites:
- Every polygon is a candidate, rejected by bounding box. PD only tests the tiles of the rooms a chr is in.
- "Outside every room" (`oobfail`) becomes "no floor within the cylinder below".
- Sight and shot rays use a standard segment/triangle test over each polygon's fan, standing in for `func0002f490`.
- `STEP`, `SLOPE`, `DIE`, `RAMPWALL` and lifts are not in the generic input. Complex has none of them.

**Check (`a_bot_walks_every_link_of_pds_complex_graph_but_three_known_ones`):** a bot walks every allowed direction of every link of PD's graph with the real movement code: 401 walks, including the ladder, the crawl space and 19 ledge drops. **398 arrive.** The 3 that don't are pinned:
- `0x01 → 0x03` and `0x88 → 0x8a` climb 276 cm straight out of the pit. PD's data marks them two-way, but the pit wall is sheer, so they only work downward and a PD bot would strand on them too. **PD's graph has 2 bad links.**
- `0x0f → 0x0e` starts at a walkway-edge pad; a spawn there lands on the floor below. A bot passing `0x0f` on a route is on the walkway.

**Measured:**
- Walls are surfaces: a cylinder deep inside a solid touches nothing, exactly as in PD.
- The crawl space needs `GEOFLAG_AIBOTCROUCH`: a 118 cm wall-flagged ceiling over room 0x12. A bot drops to 90 cm when within 22 cm of a crouch tile, before its 20 cm radius reaches the ceiling.
- **Going down a ramp, bots float up to ~50 cm above it** (8 of 401 walks, 25–50 cm). This is PD's code: any drop takes the gravity branch (`chr.c:891`), and a bot at walking speed outruns its fall on a long 35° ramp.
- The body still plays its standing rows when crouched (squat rows not ported), so heads clip the crawl-space ceiling.

### Stage 3: PD's baseline (done; visual check passed)

`pd_nav.rs` ports `padhalllv.c` function by function over a PD-format `NavGraph`. The same code will route on our generated graph.
- `nav_find_route`: group-level Dijkstra by hop count, then waypoint-level inside each group.
- `waypoint_find_closest_to_pos`: room candidates, a floors-only sight test, a zero-height swept line, and the fallbacks.
- The seeded tie-breaks. With a nav seed set, PD rotates a *copy* of the seed on every coin flip, so one routing call takes either every first match or every last one.

`chraction.rs` now ports:
- `chr_go_to_room_pos`: `MAX_CHRWAYPOINTS` = 6 slots and `CHRNAVSEED`.
- `chr_gopos_advance_waypoint`: re-routes to `act_gopos.target` once past slot 3.
- `chr_tick_gopos`:
  - `pos_is_arriving_at_pos`, i.e. ±150 cm vertically, from `prop->pos`;
  - the skip-ahead rules with `PADFLAG_AIWALKDIRECT`;
  - `PADFLAG_AICROUCH`/`AIDUCK` → `GOPOSFLAG_CROUCH`/`DUCK`.

`bot.rs` now has the full `bot_is_about_to_attack` (rooms, `chrrooms`, `numwaystepstotarget`) and the round-robin route to the target. `sim.rs` has all three passes of `player_choose_spawn_location`; on Complex, passes 1 and 2 finally take pads.

Rooms, substituted:
- A pad's room is the room of the floor under it.
- Two rooms neighbour when their polygon edges run along each other, collinear and overlapping, in 3D or seen from above. "Along" handles T-junctions; "from above" joins rooms stacked over open air.
- Checked against PD's rule that linked waypoints share a room or neighbour rooms (`padhalllv.c:67`): all links pass except 6, which go through a 1–3-tile doorway room. PD's portals may join those rooms directly; that can't be checked without the BSP.

The arena now routes with the same `nav_find_route`, over its grid turned into a one-group graph. That means hop count instead of distance.

**Headless baseline** (`probe_complex_match`, 4 NormalSims, 3 minutes, default seed):

| metric | value |
|---|---|
| kills | 33 (11 per minute) |
| target in sight | 32% of alive time |
| first contact | 4.6–10 s after spawn |
| floor share | ground 70%, first floor 29%, top walkway 1%, pit 1% |
| go-tos stuck for 3 s | 0 |

The top walkway is barely used, which is worth watching on the PD graph before blaming ours. `bots_fight_across_complex_on_pds_graph_without_stalling` pins kills, time upstairs and zero stalls.

### Stage 4: our generator (done: S1–S4 pass)

`navgen.rs` generates the graph from a `TileLevel` alone: no pads, no PD graph. It takes 0.7 s on Complex. `navcheck.rs` measures S1–S4 on any two graphs.

1. **Sample** floors on a 50 cm lattice. A sample is kept where:
   - a chr fits by PD's volume test (185 cm, or 90 cm on crouch floors, 3 cm margin);
   - PD's ground finder stands it on that floor, probing from both +69 and +120;
   - it's outside every ladder's 2.5-radius climb reach.
2. **Link** lattice neighbours with a kinematic probe that follows the movement code, and link ladder foot to top.
3. **Cover** greedily: the widest-clearance uncovered sample becomes a node, and covers what it can walk to in a straight line within 2 m.
4. **Link nodes** that can see and walk to each other within 3.6 m, in both directions. Drop any link that a two-link detour matches within 10%.
5. **Repair reachability:** wherever a lattice link a → b isn't matched by node reachability, a and b become nodes with forced links.
6. **Emit PD's format:**
   - one-way links carry `WPSEGFLAG_*`;
   - waygroups are compact, grown within 2 m over two-way links (so each is strongly connected);
   - `PADFLAG_AIWALKDIRECT` goes where PD's walls-only skip-ahead test would be fooled;
   - crouch nodes get `PADFLAG_AICROUCH`.

The probe mirrors, one rule at a time, every way the real walker failed:
- **Tall bots step higher than 69 cm.** PD probes the ground from `max(manground + 69, prop->pos.y)`, and a running bot's root is 87–120 cm, so the probe rejects any surface within 120 cm above.
- **A go-to chr near a ladder climbs it.**
- **A blocked fall waits.** A falling chr can't descend through a wall, so the drop starts a few cm later, but it may wait at most 30 cm, and a "hover" across a gap is refused.
- **Crouch zones are entered by sliding** along the low ceiling until the crouch trigger fires.
- **The probe keeps a 2 cm margin** from walls.

| check | ours | PD's graph | threshold |
|---|---|---|---|
| S1 pads uncovered (150 cm, same floor, in sight) | 3 of 163 | 9 of 163 | "all": PD's own graph fails it, so the test is "no worse than PD" |
| S2 components (weak / strong) | 1 / 1 | 1 / 1 | same as PD |
| S3 spawn-pair routes, ours ÷ PD length | none missing; median 0.93, worst 1.43 | — | median ≤ 1.15, worst ≤ 1.5 |
| S4 directed links walked | 1,202, 0 bad | — | all arrive |

Pinned in `our_generated_graph_passes_the_static_checks_against_pds`. Our graph has **~363 nodes, 2.5× PD's 144**. S1 is what drives the density: at a 3 m cover (276 nodes), S1 rises to 22 uncovered.

Harness lessons, which S4 needed before it measured the graph:
- Start our walks exactly on the node. PD's spawn adjustment moves starts 60 cm and down a floor.
- Apply the goal pad's crouch flag, as a real go-to does.
- Count a fall only past 100 cm on a two-way link, because steep ramps float up to ~75 cm.

### Stage 5: A/B (crawl-space blocker fixed; two coverage gaps left)

Set `PD_NAV=pd|ours`, or use the panel's "Route with" picker. `abtest.rs` plus `probe_ab` run 10 seeds × 3 minutes × 4 bots on each graph.

**With crouch zones in our graph (the default):**

| metric | PD | ours | ratio |
|---|---|---|---|
| D1 first contact, median | 5.6 s | 4.9 s | 0.87 |
| D2 kills/min | 10.7 | 7.5 | 0.70 |
| D2 target in sight | 36% | 63% | 1.73 |
| D2 in a go-to | 79% | 52% | 0.66 |
| D3 re-paths per bot-minute | 0.025 | 2.0 | ✗ |
| D4 coverage ground / first | 0.52 / 0.57 | 0.43 / 0.32 | ✗ first |

Kills per match: PD 15–36 (mostly 35); ours 3–40.

**Cause: Complex's crawl space (room 0x12, a 118 cm ceiling, 79 cm wide).** It's a spike limitation, not the graph:
- PD crouches bots there with the squat/duck rows `ANIM_0280`–`0287`, which the spike never exported.
- So a crouched bot keeps its standing-height root and gun, above the ceiling.
- Two bots meeting in the tunnel see each other (the eye uses the crouched height) but shoot the ceiling, forever, or deadlock head-on.

Our graph routes through the tunnel more often than PD's does. PD's 15-kill seed looks like the same trap.

**With crouch zones left out of our graph (`NAVGEN_NO_CROUCH=1`):**

| metric | PD | ours | ratio |
|---|---|---|---|
| D1 first contact | 5.6 s | 4.6 s | 0.83 ✓ |
| D2 kills/min | 10.7 | 13.3 | 1.25 (better, but outside ±20%) |
| D2 in sight / in a go-to | 36% / 79% | 34% / 78% | ✓ |
| D3 re-paths / stalls / ledge falls | 0.025 / 0 / 1.68 | 0 / 0 / 1.49 | ✓ |
| D4 ground / first | 0.52 / 0.57 | 0.47 / 0.46 | ✓ (≥ 0.8) |
| D4 pit / top | 0.17 / 0 | 0 / 0 | ✗ pit; neither graph reaches the top walkway |

Kills per match: ours 37–43, on every seed.

The pit difference is mostly PD's data. PD's two bad pit links make the pit look like a shortcut, so PD bots route through it. Ours marks it drop-in only, so bots only go in after a target.


**Fix (2026-09-26):**
- **Squat and duck animations ported.** The user supplied GoldenEye Setup Editor frames: their `#animation` comments name `Animation0284`–`0287`, and their frame-0 rotations match `pd_anim.py` to within 0.06°. So all eight clips `ANIM_0280`–`0287` were exported with `pd_gltf.py clip` into `bot_anims/`; the OBJs are kept in `bot_anims/setup_editor_frames/`.
- **Ported with them:**
  - the chooser's duck and squat rows and their `attackanimconfig`s;
  - `bot_guess_crouch_pos` (from the chr's height);
  - speed ×0.35 squatting and ×0.5 ducking;
  - spread ×0.5 squatting;
  - root heights regenerated (squat 57–68 cm).
- **Headless gun positions:** `gunpos.rs` computes where each drawn gun really is (skeleton + clips, CPU), which is what PD's `chr_get_gun_pos` reads for a drawn gun. The viewer uses the same code, and the A/B uses it by default (`AB_FALLBACK_GUNS=1` switches back to PD's off-screen fallback, root + 30 cm).

**A/B after the fix** (10 seeds × 3 min, crawl space included in our graph):

| metric | PD | ours | ratio | result |
|---|---|---|---|---|
| D1 first contact | 6.15 s | 5.22 s | 0.85 | ✓ |
| D2 kills/min | 11.7 | 12.1 | 1.03 | ✓ |
| D2 target in sight | 30% | 31% | 1.03 | ✓ |
| D2 in a go-to | 82% | 80% | 0.97 | ✓ |
| D3 re-paths per bot-minute | 0.042 | 0.050 | 1.20 | noise: 6 events vs 5 |
| D3 stalls / ledge falls | 0 / 1.53 | 0 / 1.43 | — | ✓ |
| D4 ground | 0.55 | 0.49 | 0.90 | ✓ |
| D4 first floor | 0.59 | 0.44 | 0.73 | ✗ per match; 0.89 summed over seeds. PD's own routes detour upstairs (see the diagnosis below) |
| D4 pit / top | 0.17 / 0 | 0 / 0 | — | ✗ pit (PD's is inflated by its 2 bad links) |

Kills per match: PD 33–39, ours 34–39.

### Stage 5 diagnosis: the two open D3/D4 rows (2026-09-26, later)

**First floor (D4 0.73): PD's graph sends bots upstairs on detours; ours doesn't.** The gap is real: over 30 seeds our bots spend 17.5% of their time on the first floor against PD's 24.2%, and ours is below PD on 29 of 30 seeds. They go up 10% less often and stay 24% shorter (4.1 s vs 5.2 s). But nothing is missing from our graph:
- A target on the first floor resolves to a first-floor waypoint on both graphs (261 of 262 cells).
- Routed between every pair of nodes, or between random floor cells, our routes run on the first floor as much as PD's do (ground → first: 28% of the route vs 25%).
- **Replaying the 5,627 go-to requests PD-graph bots made** (`probe_replay_gotos`) isolates route choice from the match. PD routes them 29.7% on the first floor, ours 24.5%. Where only PD's route climbs (313 requests, `probe_replay_divergent`), **our route is 0.69× the length of PD's** (median). Where only ours climbs (133), the lengths are about equal (0.95).
- Why PD climbs: PD's first floor is sparse. The east corridor is 14 m in 3 links (~6 m each), against ~4–5 m links on the ground floor. PD routes by fewest hops, so the walkway ring (pads 0x1d, 0x51–0x59) and the east corridor (0x3d, 0x3e) become shortcuts that are ~45% longer in metres.
- Summed over 10 seeds, ours visits 187 of the 262 first-floor cells and PD 209 (0.89). Per match it's 0.44 vs 0.59, because ours goes up only when a target is there.

**Tried and removed: express links.** These were straight links of 5–8 m, walkable both ways, added where they save ≥ 2 hops, so our long corridors would be as cheap as PD's. First-floor coverage didn't move (0.74 / 0.61 / 0.72 at 600 / 700 / 800 cm). From 800 cm S3 fails (worst 1.57; 1.89 at 1000 cm), because under hop costs two long links beat three short ones however far round they go. Two traps they hit, for anyone retrying: a long link into the crawl space walks at crouch speed after sliding along the ceiling, and a height limit on candidates (≤ 150 cm) excludes exactly the ramps PD makes cheap.

**Re-paths (D3 0.050 vs 0.042) are noise:** 6 events against 5 over 120 bot-minutes.
- `probe_repaths_on_ours` had reported 163 because it still fired from PD's off-screen fallback gun, which re-creates the crawl-space deadlock. It now uses the posed guns, like `probe_ab`.
- The one spot that repeats (seeds 2 and 6): a bot pressed against ramp wall 891 at (−2750, 214, 1613). `waypoint_find_closest_to_pos` tests a zero-height line at prop height, which clears the low wall, so the re-route starts at first-floor node 60, and the bot presses again. That's PD's function as written; PD's hand placement just never puts a pad there.
- 17 of 3,411 lattice samples have a closest waypoint that can't be walked to straight, mostly wall-hugging samples where the probe's 2 cm margin is stricter than the walker. The spot above isn't a sample at all, so this isn't repaired.

**Pit and top walkway: open (the user's call).**
- Top: both graphs reach it by the same ladder at (−700, −1890) and leave by dropping. It's a dead end, which fewest hops never routes through, and PD's bots spend 0.1% of their time there.
- Pit: both drop into it in 2 places and walk out once (ours by a ramp at z ≈ 1894). PD's extra visits come from its 2 bad two-way links.
- Either way, what brings PD bots into dead ends is pickups, which the spike replaces with a gun on spawn (step 1 substitution).

New probes (all `-- --ignored --nocapture`):
- `probe_coverage_map`: per-cell visit counts for both graphs plus both graphs as JSON (`COVERAGE_DUMP`, `AB_FIRST_SEED`), time per band, band moves, how first-floor visits end.
- `probe_replay_gotos` (`AB_PD`) and `probe_replay_divergent`: replay logged go-tos (`SimStats::goto_log`) on both graphs.
- `probe_walk_link`: one traced walk (`WALK_LINK="x,y,z;x,y,z"`).
- `probe_navgen` now also counts samples whose closest waypoint isn't walkable straight.

## Decision (made 2026-09-26)

**"Our replacement" = a waypoint graph auto-generated from level geometry, in PD's own
format (waypoints + waygroups),** so the same generator can later serve levels built in our
editor, by sampling the CSG floor surfaces instead of PD tiles. The generator should
therefore take a **generic input**: floor polygons, wall/blocker polygons and optional rooms.
PD tiles are just the first adapter. Do not build it around anything PD-specific that an
editor level couldn't supply.
