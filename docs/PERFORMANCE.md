# Performance survey

Where this repo spends time, where it wastes time, and what could be replaced with a faster version without changing what the player sees. This is a read-only survey taken on 2026-10-01 at commit `bf3bc88`. Line numbers refer to that commit. The top three are done (see [Done](#done)); nothing else is.

## Done

The top three, on 2026-10-01. `cargo test -p pd_sim --release --lib probe_fingerprint -- --ignored --nocapture` (`FP_STAGES`, `FP_SECONDS`, `FP_GEN=1`) runs a seeded match per stage and prints the step time and a hash of every chr's state every frame (with `FP_GEN=1`, also `nav::gen`'s time and a hash of its graph). Use it for any change that claims bit-identical output.

| | Before | After | Output |
|---|---|---|---|
| Sim step, mean (1 player, 8 Hard simulants, 60 s) | ref 296-358 µs, mp11 300-336, facility 421, cifelicity 536 | ref 212, mp11 221, facility 305, cifelicity 386 | Fingerprints identical on all four |
| `nav::gen` (default params) | ref 1.9 s, mp11 3.7 s, kokiri 35 s, facility 60 s, cifelicity 160 s | ref 0.53 s, mp11 0.75 s, facility 8.2 s, cifelicity 16.3 s | Graph hashes identical |
| Objects drawn per view (`stage --full`) | all | facility 1-29 of 46, ref 0-3 of 5 | 40 snapshots byte-identical (stage ref and facility `--full`, 4-player split, htb, pickups, kokiri flow) |

1. **Collision grid**: `PolyGrid` and `GridFrame` in [collision.rs](../crates/pd_sim/src/stage/collision.rs), 2 m cells, for the walls, floors, sight, shot and any-blocker lists, plus per-room floor lists for `cd_find_y_in`. Ladders, crouch and duck tiles are few and stay plain lists. Both free fixes are in. The sim gain is smaller than estimated: the scans were about a third of the step, not most of it.
2. **Skipping tickless frames**: `AppConfig::render_on_tick_only` (engine). `perfect_dark` turns it on; pd_edit and pd_lab keep redrawing every frame. At the 20 Hz rate, the F1 panel also refreshes at 20 Hz.
3. **Object culling**: `obj_is_onscreen` in [pd_render/src/lib.rs](../crates/pd_render/src/lib.rs), which is PD's `obj_tick_player` test (`pos_is_onscreen` with the model's effective scale, or the draw distance for `OBJFLAG2_CANFILLVIEWPORT`). It does not cull by room, because `Obj::room` is the placement room: thrown and dropped objects have none, and lifts move away from theirs. A PD-style room list needs the sim to track each object's rooms as it moves. Every object's model still loads on the first frame, so culling adds no first-sight hitch.

## Recommendation

If I had to pick three, I'd take the **collision grid** (the sim and the importer both benefit), **skipping unchanged frames** (which also fixes the effect-speed bug), and **object culling**. All three are small changes. Each one either leaves the output bit-identical or brings it closer to PD, and the existing tests can check them.

Before any of them, add a profiler (see [Measure first](#measure-first)).

## The short version

The faithful porting is not what costs time. The N64-era maths is either already cheap (the sim's angles and positions are native f32; no fixed-point is emulated) or is the point of the port (the audio synth, the CPU RDP behind the menu goldens). Most of the waste is in the scaffolding around the port:

- brute-force searches where PD itself used room lists;
- rendering more often than the sim ticks;
- buffers allocated, cleared and copied every frame.

## Measure first

Nothing measures frame time today. The only timing code is the engine's frame clock ([clock.rs](../crates/engine/src/clock.rs)), the fps label ([main.rs:499](../crates/pd_game/src/main.rs#L499)) and the importer's waypoint timer ([place.rs:169](../crates/pd_import/src/place.rs#L169)). There are no wgpu timestamp queries (every pass has `timestamp_writes: None`) and no tracy or puffin spans. Everything below on the render side is an estimate from reading the code.

Step zero: put tracy or puffin spans around the sim step, the menu render, the HUD for each view and the uploads, and add timestamp queries to the render passes.

## What is already cheap

The sim's timings come from a throwaway benchmark: `harness::world` with 4 idle players and 8 simulants on PD's nav graph, 3600 frames. The machine was noisy (a second run was about 2x slower, with OS spikes), so treat these as rough.

| Stage | Polygons | Mean step | p99 |
|---|---|---|---|
| Complex (`ref`) | 1208 | 268 µs | 513 µs |
| mp11 | 2329 | 369 µs | — |
| facility | 4397 | 604 µs | — |
| cifelicity | 7095 | 853 µs | 2.3 ms |

That is 2-5% of a 16.7 ms frame. The sim does not limit the frame rate today. It matters for long headless probes, for pd_lab and for bigger custom levels, because its cost grows linearly with polygon count (see item 1).

The following are fine too:

- **Audio.** `n64::naudio` calls the RSP audio ABI ops directly against a 4 KB DMEM rather than interpreting command lists. `pd_music --flow 120` renders 125 s of music in 1.4 s, about 1% of one core. A float mixer would change the sound (`clamp16`, `vmulf` rounding, the saturating envelope ramps) for no gain. Leave it.
- **Textures** are decoded once. The exporter resolves TLUTs, and `MipTex` and the model texture cache ([draw.rs:30](../crates/pd_core/src/model/draw.rs#L30)) hold expanded RGBA. Dithering happens only on the GPU.
- **Skinning** is on the GPU (a joint palette plus a matrix index per vertex). Chrs are posed once per tick in world space and shared by every view.
- **BG room visibility** is PD's own portal algorithm, and only on-screen rooms are drawn.
- **Stage loads** take about 20 ms for an arena and 0.1 s for Facility since commit `1602e36`.
- **Bot targeting** is PD's round-robin: one LOS test per simulant per frame.
- `WorldRes` fields are `Arc`, so the many `res.gset.clone()` calls are cheap. The SipHash `HashMap`s cost tens of nanoseconds a lookup, which is negligible.

## Top three

### 1. A spatial grid for collision

**What happens now.** Every collision query walks the whole list of walls, floors or sight polygons, rejecting each by bounding box. The header of [collision.rs:19-21](../crates/pd_sim/src/stage/collision.rs#L19-L21) marks this as a departure: PD tests only the tiles of the rooms a chr is in, so here the port does more work than the 1999 game. The scans:

- `volume_collect_wall` ([:493](../crates/pd_sim/src/stage/collision.rs#L493))
- `first_touching` ([:521](../crates/pd_sim/src/stage/collision.rs#L521))
- `cd_find_ladder` ([:541](../crates/pd_sim/src/stage/collision.rs#L541))
- `atob_cyl_obstacle` ([:789](../crates/pd_sim/src/stage/collision.rs#L789))
- `cd_test_volume_fromdir` ([:957](../crates/pd_sim/src/stage/collision.rs#L957))
- `cd_find_ground_at_cyl_ctfril` ([:1078](../crates/pd_sim/src/stage/collision.rs#L1078))
- `first_hit` ([:1319](../crates/pd_sim/src/stage/collision.rs#L1319))

Each chr runs about 3-6 wall scans and 1-2 floor scans a tick, and each simulant one LOS. By the benchmark these scans are most of the tick: about 0.2 of 0.27 ms on Complex and 0.7 of 0.85 ms on cifelicity. They are also most of the waypoint generator's time (item 1 of [Import time](#import-time)).

**Replacement.** A uniform XZ grid of about 2 m cells. Each cell lists, in ascending order, the indices of the polygons whose boxes overlap it, one list each for walls, floors and sight. A query gathers its cells, merges and dedupes them, and visits the candidates in ascending index order.

**Output.** Identical. Every query is already box-gated, so a polygon outside the gathered cells fails its box test anyway. Visiting in index order keeps the order-sensitive spots the same: the 20-collision cap in `cd_find_ground_at_cyl_ctfril`, the "first" in `first_touching` and `cd_find_ladder`, and strict-`<` tie-breaks. No RNG is involved.

A PD-style room list would be closer to PD, but it is a different candidate set, so its output would not be bit-identical to today's.

**Impact.** Expect 10-50x fewer polygon tests per query. Both run-time simulants and the importer benefit.

**Two free fixes in the same file:**

- `atob_cyl_obstacle` ([:791](../crates/pd_sim/src/stage/collision.rs#L791)) calls `ramp_wall_ok`, which loads `geom.polys[poly]` (a cache miss), before the cheap box reject. Swapping the order changes nothing else.
- `cd_find_y_in` ([:1240](../crates/pd_sim/src/stage/collision.rs#L1240)) clones the whole `floors` Vec (2-7k entries) on every `ycnp`/`ycfn` call. Falling projectiles call it up to 4 times a tick ([projectile.rs:710-739](../crates/pd_sim/src/props/projectile.rs#L710-L739)). Iterate the slice instead, and give the `Some(rooms)` path precomputed per-room floor lists.

**Check:** the seeded harness matches (`M10_SEED`, `M12_SEED`) bit for bit, plus `cargo test --workspace --release`.

### 2. Skip rendering when no tick ran

**What happens now.** The engine renders up to `max_fps: 240` ([app.rs:43](../crates/engine/src/app.rs#L43)) with Mailbox present, and `perfect_dark` keeps that default ([main.rs:685](../crates/pd_game/src/main.rs#L685)). The sim ticks at 60 Hz (or 30 or 20), and pd_game does not override `Game::frame` or interpolate. `Runner::frame` ([app.rs:123-149](../crates/engine/src/app.rs#L123-L149)) renders whether or not a tick ran, so on a fast display about 3 frames in 4 redraw an unchanged world.

**There is a bug in this too.** Some render-side state advances once per frame drawn rather than once per tick, so at high refresh it runs too fast:

- `slide_laser_liquid(..., lvupdate240)` ([lib.rs:488](../crates/pd_render/src/lib.rs#L488)) slides the laser's liquid about 4x too fast at 240 fps;
- `jitter_star`'s RNG ([lib.rs:490](../crates/pd_render/src/lib.rs#L490));
- `post.seed` ([post.rs:123](../crates/pd_render/src/post.rs#L123));
- the N64 video chain's `frame` counter.

**Replacement.** When `take_ticks()` returns 0, present the last image again or skip the frame. Alternatively, move the per-render state onto the tick.

**Output.** The same images at the same tick rate, and the effects above run at PD's speed. **Impact:** up to 4x less CPU and GPU render work on high-refresh displays.

**Check:** a playtest on a high-refresh monitor (the laser's liquid), plus the stage and match snapshots.

### 3. Cull objects by room

**What happens now.** `obj_draws` ([lib.rs:654](../crates/pd_render/src/lib.rs#L654)) draws every object in the stage, in every view. Chrs are filtered by `onanyscreen`; objects aren't. PD's `props_render` draws only the props in rooms that are on screen.

**Replacement.** Test `portals.is_onscreen(o.room)`, then a bounding-box frustum test.

**Output.** Closer to PD. Today an object in a room that isn't on screen can show through sky or scissor gaps. **Impact:** hundreds to thousands of draws and uniform writes saved per view on large stages.

**Check:** `pd_snapshot out stage ref --full` and `out flow --players 4 ...`, compared before and after.

## Other findings, by area

### Render (GPU)

| # | What | Where | Replacement | Output |
|---|---|---|---|---|
| R1 | Every model instance makes two `write_buffer` calls plus a Vec allocation (each `write_buffer` gets its own staging allocation in wgpu 24). Every view multiplies `w2e * m` per joint on the CPU into a fresh Vec | [n64/src/gpu/mod.rs:230](../crates/n64/src/gpu/mod.rs#L230), [lib.rs:419](../crates/pd_render/src/lib.rs#L419), [:710](../crates/pd_render/src/lib.rs#L710) | Upload every palette in world space once a frame into one storage buffer shared by all views; `w2e` goes in the frame uniform; select each instance by dynamic offset | Last-ulp float differences |
| R2 | `set_pipeline` and two `set_bind_group`s on every batch with no check for repeats; `Cmd` carries `model: String`, cloned per batch per frame and looked up in a `HashMap<String>`. BG `draw_one` is the same | [models.rs:233](../crates/pd_render/src/models.rs#L233), [:309-325](../crates/pd_render/src/models.rs#L309-L325), [bg.rs:542](../crates/pd_render/src/bg.rs#L542) | Skip repeated state; a `usize` model index. Later: one vertex/index buffer for static models, materials in a storage buffer, `multi_draw_indexed_indirect` | Identical (keep draw order) |
| R3 | The one combiner shader contains `discard` ([combiner.wgsl:280, 282](../crates/n64/src/gpu/combiner.wgsl#L280)), so every pipeline has it, opaque BG included. Many GPUs then turn off early depth testing, and overdraw is expensive with the 3-point filter (6 samples plus derivatives) | combiner.wgsl | An `ALPHA_TEST` override constant in `PipeKey`, like `BLENDED` | Identical |
| R4 | Split screen does one `queue.submit` per view because the views share uniform buffers (the BG's `FrameSlot`, fx, sky, post), then copies each view in with `copy_texture_to_texture`. The dyntex UV rewrite (bg.rs:463) and the `obj_draws` walk repeat per view | [lib.rs:259-306](../crates/pd_render/src/lib.rs#L259-L306) | A uniform region per view (a ring with dynamic offsets), one encoder and one submit, each view drawn into its sub-rect by viewport and scissor | Identical |
| R5 | Models load lazily mid-render: on first sight, a PNG decode, f32 box-filter mips (at least 6 levels, `want.max(6)` at [rdp.rs:154](../crates/n64/src/rdp.rs#L154)), conversion back to u8, upload. Likely a hitch on first appearance | [lib.rs:476](../crates/pd_render/src/lib.rs#L476) | Preload every model the setup can show at stage load. Mips could move to the exporter if it rounds exactly as now | Identical |
| R6 | Smaller per-frame work: the sky's `create_buffer_init` per view ([sky.rs:391](../crates/pd_render/src/sky.rs#L391)); a bind group per post pass ([post.rs:179](../crates/pd_render/src/post.rs#L179)); the full-frame copy for motion blur even when nothing blurs ([post.rs:203](../crates/pd_render/src/post.rs#L203)); 6-7 bind groups per frame in the N64 video chain ([video.rs:639](../crates/n64/src/gpu/video.rs#L639)), VI and divot passes that run with their flags off, `gauss()` recomputed per tap, a non-separable 25-tap glow; `node_visible` quadratic in nodes for instances without visibility data ([models.rs:104](../crates/pd_render/src/models.rs#L104)); `draw_order()` allocating and sorting twice per view; the x-ray's BG vertex colours rebuilt on the CPU every frame ([xray.rs:136](../crates/pd_render/src/xray.rs#L136)) | various | Cache bind groups and buffers; swap targets instead of copying; skip disabled passes; precompute at load | Identical |

### HUD and menus (CPU)

The CPU 2D path costs little per frame (320×220 is about 70k pixels). What it wastes is memory traffic and empty pixels.

| # | What | Where | Replacement | Output |
|---|---|---|---|---|
| H1 | Each frame allocates a fresh `hud_frame` (1.1 MB of f32x4) plus a z-buffer the HUD never uses. For each view it then clears `hud_scratch`, runs `crop()` (another Gfx and z-buffer) and `paste()`, and `premultiplied_rgba8()` collects into a Vec with no size hint. N64 video mode converts again ([main.rs:632](../crates/pd_game/src/main.rs#L632)). With 4 players that is about 10-15 MB of memset and copy a frame, roughly 1-2 ms | [lib.rs:263, 503-552](../crates/pd_render/src/lib.rs#L263), [hud.rs:790-947](../crates/pd_render/src/hud.rs#L790-L947) | Keep the buffers; clear only each view's rect; draw straight into `hud_frame` with the scissor set; convert into a preallocated buffer, or upload only the rect. Optionally upload f16/f32 and convert on the GPU | Identical |
| H2 | `menu.render()` runs every match frame and clears the 1.1 MB framebuffer and 280 KB z-buffer even when no menu is showing. When one is, the whole framebuffer is copied into `menu_layer` ([main.rs:352](../crates/pd_game/src/main.rs#L352)), and `push_layer` allocates a fresh 1.1 MB Vec | [session.rs:63](../crates/pd_game/src/session.rs#L63), [menu.rs:2504](../crates/pd_menu/src/menu.rs#L2504), [rdp.rs:294](../crates/n64/src/rdp.rs#L294) | Skip the render when nothing shows; swap buffers instead of copying; reuse one pooled layer | Identical |
| H3 | The rasteriser walks a triangle's whole bounding box, evaluating the full edge function at every pixel. The menu cone's 16 rays (600 px long, perspective-textured, 3-point filtered; [gfx.rs:455](../crates/pd_menu/src/gfx.rs#L455)) have mostly empty boxes, so they are probably the most expensive thing in the menus. The full-screen blur quad ([gfx.rs:331](../crates/pd_menu/src/gfx.rs#L331)) is about 70k bilinear samples | [rdp.rs:417-457](../crates/n64/src/rdp.rs#L417-L457) (2D), [:698-761](../crates/n64/src/rdp.rs#L698-L761) (3D) | Compute a conservative x-span per row from the edges and evaluate the same per-pixel formula only within it (±1 px). About 2-5x on the cone and holorays | Identical |
| H4 | Draw state is dispatched per pixel: the 2D path matches on `st.tex`, `st.filter`, `st.cc` and `st.blend`; `blend_px` ([:333-354](../crates/n64/src/rdp.rs#L333-L354)) repeats a scissor test the clamped box already did; the 3D path evaluates 8 mux closures per cycle per pixel ([:779-836](../crates/n64/src/rdp.rs#L779-L836)) | rdp.rs | Specialise per draw state (const generics or a few closures), resolve the 3D mux to input indices once per `DrawState`. About 1.3-2x on the inner loop | Identical if the arithmetic order is kept |
| H5 | Texel addressing uses `rem_euclid` (an integer divide) for each of 3 texels per sample, doubled for trilerp; texels are `[f32;4]` (16 B) | [rdp.rs:63-73, 100-104, 184](../crates/n64/src/rdp.rs#L63-L73) | Masks for power-of-two textures; store `[u8;4]` and convert on fetch (`u8 as f32 / 255.0` is exact) | Identical |
| H6 | A Vec per triangle in `raster()` and `raster_clipped`, and per batch in `draw_node`: 1-3k allocations a frame for a menu character | [rdp.rs:616, 644, 679](../crates/n64/src/rdp.rs#L616), [draw.rs:144](../crates/pd_core/src/model/draw.rs#L144) | Fixed arrays or a reused scratch buffer | Identical |
| H7 | Per-pixel font work: a CI4 nybble decode with a bounds check, a TLUT lookup, `blend_px`. Low priority: a dense menu is under 20k glyph pixels | [text.rs:104-111, 758-880](../crates/pd_core/src/text.rs#L758-L880) | Expand each glyph once into alpha bytes | Identical |
| H8 | `rgba8(n64)` builds a fresh Vec byte by byte for the menu upload every frame, about 0.1-0.2 ms | [main.rs:653](../crates/pd_game/src/main.rs#L653), [rdp.rs:485-499](../crates/n64/src/rdp.rs#L485-L499) | Reuse the buffer, or upload f32 and quantise in the present shader | Identical (a shader might differ at exact .5 ties) |

Moving the menus and HUD to the GPU is possible but not recommended. The WGSL combiner covers only the 3D model path and is not an exact match for the CPU rasteriser. A 2D GPU front end (fill and texture-rect paths, the layer, swap and composite sequence of [menu.rs:2577-2612](../crates/pd_menu/src/menu.rs#L2577-L2612), glyph quads) would break the exact goldens. It is a large port for a few milliseconds; items H1-H4 get most of that with no change to the output.

### Sim

| # | What | Where | Replacement | Output |
|---|---|---|---|---|
| S1 | Routing on custom levels is about 100x slower than on PD's stages: `nav_find_route` takes 1.4 µs on a PD stage, 49 µs on kokiri, 131 µs on cifelicity. Custom graphs have about one waygroup per waypoint (1707 groups for 1852 waypoints), and `waygroup_discover_one_step` rescans every group for each BFS layer. PD already throttles routing to one simulant a frame ([bot/mod.rs:798](../crates/pd_sim/src/bot/mod.rs#L798)) | [nav/mod.rs:347](../crates/pd_sim/src/nav/mod.rs#L347) | **A** (offline, no run-time change): have `nav::gen` build bigger waygroups. **B**: a frontier-queue BFS that assigns the same step numbers; tie-breaks stay in `*_choose_neighbour` over the same neighbour order | A changes the graph; B identical |
| S2 | `waypoint_find_closest_to_pos` (9-54 µs) runs up to 10 full floor LOS tests plus full wall cylmoves | nav | The collision grid (item 1) | Identical |
| S3 | Fresh Vecs per chr move and per player: `chr_perims_except` and `prop_floors` ([position.rs:73, 82](../crates/pd_sim/src/chr/position.rs#L73)), [world.rs:576](../crates/pd_sim/src/world.rs#L576); `chr_has_los_to_chr` builds `obj_geos` and `prop_floors` on every call ([aim.rs:176](../crates/pd_sim/src/chr/aim.rs#L176)) | chr | Reuse scratch buffers. Don't cache their contents across chrs: PD reads live positions, and chrs moved earlier in the tick must be seen | Identical |
| S4 | `def.nodes[i].kind.clone()` for every node of every pose, which heap-allocates for `Other(String)`, `Geo{verts}` and `StarGunfire{quads}`; `reaches()` walks the parent chain for every node | [pose.rs:214](../crates/pd_core/src/model/pose.rs#L214), [:182](../crates/pd_core/src/model/pose.rs#L182) | Match by reference; propagate visibility top-down | Identical |
| S5 | Animation decoding: `seek_part` walks the header from part 0 for every part (quadratic in parts), `remap_frame_for_load` walks the repeat list on every call, and each joint does a `HashMap<u16>` lookup ([pose.rs:288](../crates/pd_core/src/model/pose.rs#L288)) for up to 4 frames | [anim/bank.rs:171](../crates/pd_core/src/anim/bank.rs#L171) | A part-offset table per anim, built at load; a Vec index. Decoding the whole bank to f32 is not worth it (100+ MB) | Identical |
| S6 | `Props::get` is a linear find, called per id in `props_test_for_pickup` and `bot_check_pickups` ([botinv.rs:714](../crates/pd_sim/src/bot/botinv.rs#L714)). n is small (5-54 objects) | [props/mod.rs:447](../crates/pd_sim/src/props/mod.rs#L447) | An id-to-index map | Identical |

Don't prefilter pickups by distance (S6). `bot_test_prop_for_pickup` gives a throwables crate's weapon before the 1 m range check (`bot.c:584`), and `bot_find_pickup` draws `random()` per prop, newest first (`bot.c:877`). A spatial index over pickups would change RNG consumption and therefore the match.

### Import time

cifelicity's report reads "1852 waypoints from 28110 floor samples in 143.2 s". Nearly all of it is the waypoint generator, [nav/gen.rs](../crates/pd_sim/src/nav/gen.rs), on top of the full-scan collision.

| # | What | Where | Replacement | Output |
|---|---|---|---|---|
| I1 | Every lattice point tests every floor polygon with `xz_in_convex` | `sample()`, [gen.rs:283-334](../crates/pd_sim/src/nav/gen.rs#L283-L334) | Rasterise each floor's box onto the lattice; add heights to each cell in floor-index order ("first within 30 cm wins" depends on it) | Identical |
| I2 | No parallelism. Independent read-only probes against `&TileLevel`: the per-cell sampling, `lattice_links` (28k samples × 8 neighbours × a ~25-step `probe_walk` of ~6 queries each, [:355](../crates/pd_sim/src/nav/gen.rs#L355)), the node-pair probes in `node_links_by_walking` ([:528](../crates/pd_sim/src/nav/gen.rs#L528)), `skip_fooled` in `emit` ([:672](../crates/pd_sim/src/nav/gen.rs#L672)) | gen.rs | rayon, collecting results in index order. The links are sorted and deduplicated afterwards anyway ([:204](../crates/pd_sim/src/nav/gen.rs#L204), [:575](../crates/pd_sim/src/nav/gen.rs#L575)) | Identical |
| I3 | `node_links_by_walking` re-probes every node pair on every validation round (up to 12, [:241](../crates/pd_sim/src/nav/gen.rs#L241)) though only a few nodes are added each round | gen.rs | Memoise `probe_walk` and `los_autoflags` by sample pair | Identical |
| I4 | `dijkstra` allocates and scans a vector of every sample per call (1852 × 28k ≈ 52M entries) | [gen.rs:463](../crates/pd_sim/src/nav/gen.rs#L463) | Return only the visited list. Small gain | Identical |

Together with the collision grid, these could plausibly take cifelicity from 143 s to a few seconds with an identical graph, and the existing cache in `custom/cache/nav` keeps working.

### Loading, audio, build

- **bg.json is parsed four times per stage load:** `ModelDef::load_file`, [bg.rs:260](../crates/pd_render/src/bg.rs#L260), [bghit.rs:129](../crates/pd_sim/src/stage/bghit.rs#L129) and [rooms.rs:172](../crates/pd_sim/src/stage/rooms.rs#L172). Parsing once saves a few ms. A binary format or mmap gains little at these sizes (bg.bin 0.3-1 MB, tiles.json up to 1.25 MB).
- **Textures** (1793 PNGs, mostly tiny) are cached by file name. Decoding a stage's textures in parallel would save milliseconds.
- **First-play sound hitch:** [audio.rs:194-211](../crates/engine/src/audio.rs#L194-L211) decodes a WAV on the main thread the first time it plays. Preload `assets/sfx` (5.8 MB, 260 files) on a worker at boot.
- **Audio temporaries:** `dmem_move` ([abi.rs:205](../crates/n64/src/naudio/abi.rs#L205)) and `interleave` ([abi.rs:394](../crates/n64/src/naudio/abi.rs#L394)) allocate a Vec per call. Stack arrays keep the output bit-exact.
- **Build profile:** `release` has `lto = false` and `codegen-units = 16`, which is right for fast rebuilds. `release-dist` already does the slow, optimised build. No `target-cpu` is set.
- **Python exporters** are offline and nothing in them is egregious. `build_assets.py` could use `multiprocessing` if a full regeneration ever gets annoying.

## Constraints on any change

"Only the final output matters" still has two hard edges in this repo.

1. **The menu goldens are exact.** [golden.rs:35-45](../crates/pd_menu/tests/golden.rs#L35-L45) requires a pixel-exact match after RGBA5551 quantisation. Anything that reorders float operations (incremental edge stepping, FMA, a reassociated SIMD lerp, GPU interpolation) can flip pixels across a 5-bit boundary. Restructuring that keeps the same per-pixel formula is safe; anything else needs a ±1 tolerance or new goldens.
2. **The sim is deterministic.** A match is a pure function of its setup, seed and inputs, and the seeded harness matches pin it.
   - Keep float operation order in the collision maths (no FMA, no SIMD reassociation): positions feed distance comparisons that later steer branches that draw random numbers.
   - Keep iteration order wherever a loop stops early, caps (the 20-collision cap) or draws `random()`.

Everything in the top three and every row marked "Identical" keeps both. R1's last-ulp differences affect only the GPU path, which no golden pins exactly. S1's option A changes custom levels' waypoint graphs, which are regenerated offline anyway.

## Suggested order

1. Profiler spans and timestamp queries.
2. The top three: the collision grid with its two free fixes, skipping unchanged frames, object culling.
3. The waypoint generator: rayon (I2), memoisation (I3), rasterised sampling (I1).
4. Buffer reuse in the HUD and menus (H1, H2), and the per-tick allocations (S3, S4).
5. Preloading models and sounds (R5, the sound hitch).
6. Whatever the profiler then says. H3/H4, R1-R4 and S1 are the next candidates.
