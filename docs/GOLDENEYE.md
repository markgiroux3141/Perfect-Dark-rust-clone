# Porting GoldenEye levels

How a GoldenEye 007 level becomes a Combat Simulator arena, and everything we learned doing Facility (M16) that the next level should not have to rediscover. Read this before touching `tools/ge-extract` or adding a recipe.

## The idea

GoldenEye is Perfect Dark's ancestor in nearly every part PD's engine runs on: rooms and portals, floor tiles, pads, doors, the display-list dialect, the image format, the model format. So a GE level is **not** imported like Kokiri (made-up rooms, baked light, rewritten collision). It is converted **through PD's own exporters** (`tools/pd-assets`: `pd_tex` decodes the images, `pd_fpgun.Interp` runs the display lists, `pd_models.write_model` writes the one model format) straight into PD's stage formats. The run time sees four stage files like any arena's, plus the level's door models in `custom/models/`.

The user's rule (2026-09-29): **preprocess the level into PD's shape; keep the engine clean.** GE doors run on PD's door code. When GE and PD differ, fix the data in the extractor, citing both decomps. When PD's engine lacks a PD feature a GE level needs (the windowed door's portal, `floorcol` shading), port that PD function under PD's name. Don't add GE code paths to the game.

## Sources

| What | Where |
|---|---|
| GE decomp (n64decomp/007) | `reference/ge-decomp` (`git clone --depth 1 https://github.com/n64decomp/007 reference/ge-decomp`). It provides the level table (`src/game/bg.c:184`), the fog table (`src/game/bgfog.c`, **NTSC is the `#else` branch**), the image table (`assets/images.def`), the prop model table and headers (`assets/obseg/prop/`), and the setups as generated C (`assets/obseg/setup/`) |
| The ROM | `D:\GoldenPerfectModding\007 - GoldenEye (USA)\007 - GoldenEye (USA).n64`, a v64 (byte-swapped) dump. `ge_rom.z64` puts any dump in z64 order, and it must match `ge007.u.sha1` or the offsets are wrong |
| File offsets | `scripts/filelist.u.csv` (`offset,size,path,compressed,extract`) and `imagelist.u.csv` (`offset,size`, row N = image N; the last row is padding). No extraction step is needed: the extractor reads the ROM directly |

The other things in `D:\GoldenPerfectModding` (the GE Setup Editor OBJ exports, the `tempImgEd*.bmp` textures, GEEdit's `BGData`) aren't needed. They served only as a cross-check: our image decode matches the editor's BMPs pixel for pixel once its conventions are undone (its BMPs are upside down, its channels are expanded by shifting, and its CI8 widths are padded to 8).

## Running it

```
cargo run --release -p pd_import --bin pd_import -- facility   # ~1 min: the extractor (2 s) + waypoints + a check match
cargo test -p pd_import --release -- --ignored facility        # the import from the ROM; doors open; the level is one piece
cargo run --release -p pd_tools --bin pd_snapshot -- out stage facility --full --at x,y,z,theta   # a view (the F1 panel's coords)
cargo run --release -p pd_tools --bin pd_snapshot -- out lab facility    # the waypoint map: the fastest way to see a split level
target/release/pd_import --probe facility x0 z0 x1 z1                    # why the generator won't link two points
target/release/pd_import --explain facility x z                          # the floors, ground, fit and walls at a point
```

`cargo build --release` doesn't rebuild `pd_tools`, so after a shader change run `pd_snapshot` through `cargo run -p pd_tools`.

## A new level

1. Copy `crates/pd_import/levels/facility.json`. Set `level` to the `LEVELID_*` row in `bg.c`'s `levelinfotable`, `setup` to the solo setup whose doors you keep (`Usetup<code>Z`), and `markers` to the MP setup (`Ump_setup<code>Z`), whose spawns and pickups become placement hints. Also set a new `code` and `stagenum` (0x60–0x7f) and `marker_share`: 0.5 when the MP setup covers only part of the solo level, as Facility's does.
2. Run the import and read `custom/stages/<code>/report.txt`. It fails loudly on anything unported: clouds, a door scale, a node type a door model doesn't use.
3. Check the lab map (one connected graph?), the stage views, and one door of each model with `--full --at`. Then hand a playtest brief to the user.
4. Go through the checklist at the end of this file for what Facility didn't have.

## How each part maps

| GE | PD | Notes |
|---|---|---|
| BG rooms 1..`g_MaxNumRooms-1` (`bg.c:856, 891`; the last row with a primary DL only marks the end) | rooms one for one | Each room's point table, primary DL and secondary DL is its own 1172 stream. Pointers are file offsets + 0x0F000000 |
| Primary / secondary DL | the room's `opa` / `xlu` DL node | Every opaque layer is drawn first, then every translucent one |
| Portal table `{verts, u8 room1, u8 room2, u16}` | `bg.json` `portals` | Vertices are absolute. 96 of Facility's are quads and 13 have six points; ours take any count |
| Global visibility commands | dropped (`bgcmds: [[0,1,0]]`) | The same opcode set as PD's BGCMD, but no arena uses them. SUBST |
| `Vtx_t` (16 bytes, colour inline, room-relative) | our vertex, world cm | `GeDialect.vtx_size = 16`. `cidx` is the vertex index (a GE room's colour table is its vertices) |
| Units | cm | **world = BG units / `levelscale`** (`bg.c:199`; Facility 1.20648, every level different). BG, stan, portals and setup pads (incl. bound pad boxes) all share BG space. GE world units are cm (Bond's eye is 175, `bondview.c:1507`) |
| Image N (`imagelist.u.csv`) | `0x10000 \| N`, `custom/ge/tex/<nnnn>.png` | `pd_tex.decode` handles all 2698 images, since `rzip_inflate` accepts GE's header. An image's hit types (`images.def`: sound, then effect, `image.h:57`) are PD's `SURFACETYPE_*` in the same order |
| Fog row (`bgfog.c`, NTSC table) | `fogenvironment` | The columns are PD's, in order: near, far, opa%, xlu%, refdist, then NTSC's extra `mnvisrng` and `intensity`, then `dif_ght`/`far_alight` = fogmin/fogmax, then the sky colour. Dam's row is PD's Pelagic's to the digit |
| Stan tiles | `tiles.json` floors (`0x1b`), `floorcol = mid & 0xfff` | Floors only. See the lessons for the walls and the risers |
| Setup pads / bound pads | `pads.json` | The axes match PD's (bbox x along up×look, y along up, z along look). A door's `pad` indexes the bound pads; other objects use `pad ≥ 10000` for bound pads |
| Door record (64 words) | a `setup.json` `door` row | Field for field (`prop.c:925` `setupDoor` against PD `setup.c:944`); see the lessons |
| Prop model `P<name>Z` | `custom/models/ge_<name>`, MODEL `0x1000 + prop number` | The header is in the game, not the file; see the lessons |

## Hard-learned lessons

Each of these cost a debugging round on Facility.

### Compression and formats
1. **GE's rzip is `0x11 0x72` + raw DEFLATE, with no length field.** PD's is `0x11 0x73` + a u24 length. `zlib.decompressobj(-15)` and read to the end block (`decompress.c:53-66`).
2. **A GE model file has no header.** `ModelFileHeader` (skeleton, switch count, matrices, radius, texture count) lives in the game (`pobjdata.c`). Scrape it from `assets/obseg/prop/<name>/ModelFileHeader.inc.c` (`MODELFILEHEADER(name, root, skel, switches, numswitches, nummatrices, radius, numrecords, numtextures)`). The file starts with the switch pointers, then the texture table (12-byte entries, global image numbers), then the root node.
3. **GE's DL node (0x18, DLCOLLISION) has a different layout from PD's:** vertices at +0x08, count at +0x0C, model type (the render mode) at +0x18. GE's 0x04 node keeps its model type as an s8 at +0x12. The node types a door uses match PD's numbers (0x02 GROUP = POSITION, 0x0A BBOX, 0x12 SWITCH = TOGGLE, 0x18 DL). **Skeletons:** `standard_object` = `SKEL_BASIC`, and `door` = `SKEL_WINDOWEDDOOR`, whose switches 0–3 are PD's `MODELPART_WINDOWEDDOOR_0000..3`. PD's modeldef `scale` is the bounding radius, the same as GE's header's.
4. **The setup C is easy to parse by its generator comments.** Each record opens with `/* Type = X; index = N */`, so you need no per-type size table. Words are `_mkword(a,b) = a<<16|b` and `_mkshort(a,b) = a<<8|b`.

### Rendering
5. **`FORCE_BL` decides opacity, not the blend formula.** GE's opaque rooms draw `G_RM_PASS` then `(IN, A_IN, MEM, 1MA)` without `FORCE_BL` (`0x0C182078`). The RDP blends only partly covered (edge) pixels, so the surface is opaque. fast3d's rule (which `pd_fpgun` keeps for PD's assets) would call it alpha-blended; `ge_gbi.GeDialect.blend_state` applies the RDP's rule.
6. **Under `G_FOG` the RSP writes the fog into the shade alpha**, and GE turns `G_FOG` on for both room passes (`bg.c:633, 695`). Two consequences:
   - A combiner that passes shade alpha through wrote the fog to the frame's alpha, so snapshots and screenshots came out transparent (white in a viewer). **Fixed in the engine:** an opaque pipeline now writes alpha 1 (`override BLENDED` in `combiner.wgsl`), as the RDP stores coverage, not combiner alpha.
   - The translucent layer's combiner `(G_CC_TRILERP, G_CC_MODULATEIA2)` takes alpha from shade, so the railings and glass took the fog as their alpha: 0 near the eye, invisible. **PD's own fix applies:** in a fog stage PD rewrites room DLs with `g_GfxGroup01` (opaque) and `g_GfxGroup05` (translucent) (`bg_load_room`, `bg.c:2971`), swapping those combiners for ones reading ENV alpha (`G_CC_CUSTOM_06/08`). GE uses the very same command words (`FC26A004 1F1093FF` …), so `ge_bg.fog_rewrite` runs PD's groups. The render-mode half (`G_RM_PASS` to `G_RM_FOG_SHADE_A`) is `fog_shade` on every BG material.
7. **An object's fog starts from its floor's tint, not white.** PD merges `obj->shadecol` toward the sky (`obj_merge_colour_fracs`), and `shadecol` is the floor colour under the object reduced to its chroma (`prop_calculate_shade_colour`, `propobj.c:1623-1690`), so a grey or white floor gives black. Starting from white made distant doors look pale. GE's floors are mostly `0xfff`.
8. **GE's fog is heavy for objects:** with near 10, far 5000 and fog 990–1000, an object is already 75% fogged at 15 m. That is GE's row, faithfully applied.
9. **GE picks a different fog row by player count** (`fogLoadLevelEnvironment`: `LEVELID + 100 × players`, then a generic `100 × players` row). The extractor takes the one-player row. For a level whose MP rows differ, decide which one the arena should use.

### Collision
10. **Stan tiles are floors only.** GE implies a wall wherever a tile edge has no neighbour (`link >> 4 == 0`). `ge_stan` makes a wall at every unlinked edge, in two kinds:
    - **A full wall** (`0x1c`), when nothing is beyond the edge, or a floor at the same height, or only a higher one.
    - **A `WALL`-only rail** (nobody walks off it, as in GE, and sight and shots pass), when a lower floor is beyond **and** a ray 1.5 m above the edge's floor, 40 cm out, meets no opaque BG triangle. Without that ray test, a solid wall with a lower room behind it became see-through. Watch for it as a spawn "facing into the room" that is actually facing a wall.
11. **A made wall must stop a step below the floor above it**, not at the room's top: the lowest floor overhead minus 60 cm. Stairs run over landings, and a wall under a stair cut through the treads above and walled the stair shut. That was the single biggest cause of the level coming apart into pieces (419 of 924 waypoints reachable at first; 897 after).
12. **Keep GE's stair risers, flagged `GEOFLAG_STEP`.** GE models each riser as a vertical stan tile. PD kept them exactly so when it remade this level as Felicity (`assets/stages/mp11`: `0x201b`), and `cd_find_ground_finalise` handles them. Dropping them was a mistake. Their only unlinked edges are vertical (zero length in XZ) and make no walls.
13. **Stan `special`:** 1 is force-crouch (the vents; PD has only simulant crouch, `GEOFLAG_AIBOTCROUCH`, a SUBST) and 3 is a ladder. Facility has no ladders; a level with them needs the ladder checked in play.

### Doors
14. **The door record** (`bondtypes.h:2778`). Word 1 is `obj << 16 | pad`, where `obj` is an index into `PitemZ_entries` (the order of `propItemModelFileRecord.inc.c`); 2 flags; 3 flags2; 29 `maxdamage << 16`; 32 the linked door (relative, recompute it for the output order); 33–37 maxfrac, perimfrac, accel, decel, maxspeed; 38 `doorflags << 16 | doortype`; 39 keyflags; 40 autoclose frames; 41 the open-sound type; **48 and 49 the glass's xlu and opa distances, each a full s32** (`propobj.c:5830` reads `*(s32 *)(door + 0xC4)`, despite the struct's s16).
15. **Units:** GE's accel and decel are `/ 65536` (`prop.c:1120`) and PD's are `/ 65536000` (`setup.c:1057`), so write them × 1000. maxfrac, perimfrac and maxspeed are the same. The door types 0–9 and doorflags 1/2/4/8 are PD's.
16. **Three flag bits are renumbered:** GE TWOWAY `0x08000000` → PD `0x80000000`, GE KEEPOPEN `0x80000000` → PD `0x40000000`, and GE NO_PORTAL_CLOSE `0x40000000` (which is PD's KEEPOPEN bit) is dropped. flags2 0x1 and 0x8 are GE-only and dropped.
17. **Clear keys, one-way locks and the airlock** (flags2 `0x08000000 | 0x10000000 | 0x40000000`, keyflags). GE's own multiplayer setup does: every door's flags2 in `Ump_setuparkZ` is 0. An airlock door only opens while its partner is shut, which a simulant standing in the other one holds up for good ("the door won't open").
18. **GE's doors stand flush against their portals** (one face on the portal plane, some a few cm past). GE puts a door in the rooms on both sides by construction (`prop.c:1141`). PD finds a door's rooms from its box through the portals (`obj_detect_rooms`) and lets a player use only doors in rooms on screen. So a shut door couldn't be used from the side whose room it missed. `ge_setup.align_portals` moves each door's portal (found as `setup_get_portal_by_door_pad` finds it) the 5–11 cm onto the door's middle plane.
19. **Windowed (glass) doors keep their portal open while you can see through them** (`door_update_portal_if_windowed`, `propobj.c:7803`, now ported). Otherwise the room behind isn't drawn and the fog colour shows through the glass.
20. **Door sounds:** PD's `soundtype` table is GE's `DOOR_OPEN_SOUND_*` table with a new row inserted at 5 (GE 1–4 = PD 1–4, GE 5–17 = PD 6–18). PD kept GE's sound numbers but some of those numbers hold different samples, so the mapping (`ge_setup.GE_DOOR_SOUNDTYPE`) was made by comparing the two banks' samples. GE's sfx bank is `sfx.ctl`/`.tbl` in the ROM (`filelist.u.csv`), in the same n_audio format, so `pd_sfx.Bank` parses both. GE 4 (the heavy slide) is PD 17, because PD 4 adds a different loop, Runway's rolling door; GE 10 is PD 11. GE's loops 211 and 216 aren't in PD's bank.

### Placement
21. **An MP setup covers only part of a solo level.** `marker_share` caps how many spawns and weapons stand on the source's spots, and farthest-point sampling spreads the rest. The user will place them by hand in a future editor, so don't polish this.
22. **GE pad looks are axis-aligned defaults**, and a marker snapped to a waypoint can face a wall. A marker's facing is kept only with 3 m of view.
23. **A level may have parts GE's own geometry closes off.** On Facility these are the vent Bond starts in, and a door onto a 1 m ledge over a 4.8 m drop. Placement uses the largest strongly connected part of the waypoint graph, so these are harmless. Check each one with `--probe` before assuming it's a bug.

## Checklist for the next level

Things Facility didn't exercise, which the extractor refuses or leaves out:

- **Clouds and water** (`fog_tables` clouds/water columns): refused. Dam, Surface, Frigate and Runway have them. PD has both (`g_FogEnvironments` clouds and water).
- **Glass and tinted glass** (`Glass`, `TintedGlass` props): left out. Facility has 52 (its windows). PD has both kinds (`props::glass`), and their models are GE props like the doors.
- **A door scale** (a `DoorScale` record, `prop.c:980`): refused.
- **Lifts, ladders, eye / iris / fall-away doors**: not seen yet. PD's door types match GE's numbers, but only sliding, vertical and swinging are ported (`door.rs`).
- **Floor types:** GE has none, so every tile is `FLOORTYPE_DEFAULT` and all footsteps sound the same. They could come from the BG texture over each tile.
- **Door sounds** for the GE types Facility doesn't use: the table covers 0–18 by the same sample comparison, but only 1, 4 and 10 have been heard.
- **Levels sharing a BG** (Library, Basement and Stack all use `bg_ame`): the recipe names the level row, so the setup decides which half you get.
