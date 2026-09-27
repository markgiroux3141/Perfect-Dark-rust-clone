# Handoff — Perfect Dark asset import

State as of 2026-08-16. Read [START_HERE.md](START_HERE.md) §2 first for the format
details; this file is the working context for continuing the import.

## Where things stand

Five Python tools in [tools/pd-assets/](tools/pd-assets/) decode and re-export PD assets:

| Tool | Does |
|---|---|
| `pd_model.py` | model `.bin` → OBJ; walks the node tree and executes display lists |
| `pd_anim.py` | animation `.bin` → per-bone rotation/translation per frame |
| `pd_pose.py` | model + animation → posed OBJ (assembles the skeleton, skins per vertex) |
| `pd_gltf.py` | **model → skinned GLB, animation → clip GLB** — the engine import |
| `pd_tex.py` | compressed global-pool textures → RGBA8 (PD's rzip/paletted codec) |
| `pd_preview.py` | renders any GLB to a PNG on the CPU, for verifying by eye |
| `pd_triage.py` | **posture curve** over a clip — screens 1,207 animations into death / injury / upright before anything is rendered |
| `pd_animmap.py` | **measures the GoldenEye ↔ Perfect Dark animation-id correspondence** (`--check` fails if a claimed match breaks) |

**Perfect Dark characters are the hunters in the `PD_LAB`.** Six bodies + a
**36-clip hunter set** (locomotion, three fire clips, 12 hit reactions, 17 deaths)
ship under `native/assets/enemies/pd/`, loaded through the *existing* GoldenEye
pipeline (`engine/src/skeletal/gltf_skin.rs`, `engine/src/skeletal/clip.rs`) with no
new Rust loader. All six wear **Perfect Dark's own textures**, and any of the 65
shared-rig bodies can be exported the same way.

`PD_LAB=1` now spawns Joanna Dark, Cassandra, Elvis and the rest as the hunters
themselves — they walk, aim, fire, take hits and die on Perfect Dark's own
animations. `World::spawn_family` picks the family; a wave is all-PD or all-GE,
never mixed. The `PdShowcase` lineup and the `PropCategory::PerfectDark` preview
props it replaced are both gone.

**Look at [tools/pd-assets/preview/](tools/pd-assets/preview/) first** — every claim
below that could be checked by eye is rendered there, with an index saying what each
image proves.

```powershell
cd native
$env:PD_LAB = 1
cargo run --release
```

Press `G`. 279 tests green, release built.

Re-export any time with:

```sh
python tools/pd-assets/pd_gltf.py batch tools/pd-assets/pd_roster.json native/assets/enemies/pd
```

## What the glTF export decides, and why

Four choices carry the conversion. Each is derived, and each is checkable.

1. **The rig is renamed onto GoldenEye's bone names.** The game addresses bones by
   name (`Bone_9` weapon hand, `Bone_3` head, `Bone_14/15` feet — see
   `game/src/combat/enemy_weapons.rs`), so PD's 15 joints are renamed onto the GE
   roles and every bone-driven system works on a PD body unmodified.

   Sidedness was **not** guessed from the ±X sign. `modeldef.parts` is a sorted
   `MODELPART_CHR_* → node` table, and it settles it identically on a51guard,
   dark_frock, maian_soldier and mrblonde: `RIGHTHAND` (3) → anim part 8 (the −X
   arm), `LEFTHAND` (5) → anim part 7. **−X is the character's right.** Confirmed on
   screen afterwards: `Bone_9` highlights on the same side as it does on GE's Karl.

2. **Identity inverse-bind matrices, vertices left in bone-local space.** PD stores
   vertices relative to their bone and transforms them by that bone's *world*
   matrix. glTF computes `jointMatrix = global(joint) × inverseBind(joint)`, so
   `inverseBind = identity` makes `jointMatrix` exactly PD's bone world matrix and
   the conversion becomes a pure re-encoding with no change of space. (The GE assets
   use the other convention; both are legal glTF and the loader assumes neither.)

3. **Blend matrices are now exact, not approximated.** A `POSITION` node's slots 1
   and 2 are midpoint frames — `lib/modelasm_c.c:367-430` builds them as the parent
   matrix composed with **half** the joint's rotation. Each becomes its own glTF
   joint carrying the half-rotation, instead of being folded onto the owning joint.
   Measured: the 120 seam vertices move up to 3.6 cm, and every other vertex moves
   0.0000 cm. `--no-blend-joints` reverts to the old behaviour for A/B.

   **All 15 blend joints are declared on every rig, used or not** — see bug 6. Which
   bones carry one varies per character, and the rig has to be body-independent for a
   single clip export to drive them all.

4. **Heads are attached.** Only 3 of 65 shared-rig characters (a51guard, dd_shock,
   elvis) have a head built in; the other 62 carry a `HEADSPOT`. `model_attach_head`
   just reparents the head model under that node with no transform of its own, so a
   head's vertices are already in head-joint space and its display lists bind matrix
   slot 0 — which is the head joint on every body. The exporter checks that rather
   than assuming it, and pairs body to head from `pd_roster.json` (PD mixes them
   freely, so it is an authoring choice, not a derivation).

Two more, smaller: bodies export in the engine's character units so `CHAR_SCALE`
lands them at life size with no per-body scale, and clips export at **30 fps**,
derived from `chr_action_go_to_position` (`chraction.c:2189`) setting a 60Hz-frame
ETA of `distance / (movedist_per_frame × mult)` with `mult = 0.5` for locomotion.

## Bugs found and fixed, in order

Each one produced output that looked plausible until inspected the right way. Worth
reading before adding a sixth.

1. **`G_VTX` reads segment 4, not 5.** The renderer rebinds the RSP segment table per
   node: seg 3 = matrices, seg 4 = that node's vertex array, seg 5 = model base, seg 6
   = colours. Reading seg 5 gave garbage for chrs and props while guns still looked
   fine. *Caught by:* a crate that wasn't a cube.
2. **No separate bind pose exists.** Early wrong assumption that nearly became a
   workstream. The node tree IS the skeleton; the animation supplies rotations; frame 0
   of any clip is a rest pose. *Caught by:* reading `model_update_position_node_mtx`.
3. **Skinning is per-vertex via `G_MTX`, not per mesh node.** A display list switches
   bone matrices mid-stream, so one `DL` node spans several bones. Matrix index =
   `(w1 & 0xffffff) / 64`. *Caught by:* looking at it on screen — limbs tore off at the
   joints while every numeric check (heights, joint positions, symmetry) passed.
4. **`G_TRI1` (0xBF) was ignored.** PD emits both its custom `G_TRI4` and stock
   single triangles; the guard has 112 of the latter against 127 of the former.
   Indices are stored ×10, one per byte of `w1`. *Caught by:* holes in the mesh, then
   an opcode histogram over the display lists.
5. **`modeldef.scale` is not units-per-metre.** The right figure is a derived **1000
   units/m** — model units are millimetres. `dark_frock` carries 1982, which renders
   Joanna at two-thirds height. Derivation in `pd_pose.py`'s `UNITS_PER_METRE` comment.
   *Caught by:* a character measuring 0.95 m.
6. **Blend matrices sit on different bones per character.** 51 of the 65 put them on
   elbows + knees, but `elvis` uniquely uses shoulders + hips, and six other layouts
   exist. Declaring only the blend joints a body happens to use makes the rig
   body-*dependent*, and clips bind by name — so `elvis`'s hip blends had no channel
   and kept their bind rotation (that geometry stays splayed, giving a flat fin at the
   hip), while the clip's unmatched `Blend_6` landed on an arbitrary joint through
   `clip.rs`'s node-index fallback. The channel count matched throughout. Fixed by
   declaring all 15 blend joints on every rig and in every clip, so names always
   resolve and the fallback never fires. *Caught by:* someone looking at the picture
   and saying the alien's legs were wrong
   ([19-elvis-fin-fixed.png](tools/pd-assets/preview/19-elvis-fin-fixed.png)).
   `world::tests::pd_bodies_load_skinned_and_animated` now pins the name set.
7. **Texture rows are padded to 8 bytes, and odd rows are word-swapped.** Read back
   linearly, an inline texture still decodes to something recognisable — the right
   colours, the right shapes, a guard who reads as a guard. The swizzle only shows up
   as a fine vertical comb, which is easy to mistake for N64 dithering until you look
   at a *face*. *Caught by:* zooming one texture far enough to see it was a face, then
   A/B-ing the swap ([13-swizzle-before-after.png](tools/pd-assets/preview/13-swizzle-before-after.png)).
   `tex_swizzle` confirmed it afterwards, term for term.

8. **`ANIMFIELD_08` is root motion, and its bits come first.** Part 0 carries a
   four-channel field — x, y, z and a facing angle — that
   `anim_get_rot_translate_scale` deliberately reports as *no translation*
   (anim.c:510) because a different function, `anim_get_pos_angle_as_int`, is what
   reads it. `pd_anim.py` skipped it as "present but unused", which cost two things
   at once: the clip lost its travel, **and** every rotation was read 28–42 bits
   early, because those bits sit ahead of the rotation bits in the same frame. The
   result is not noise — it is a perfectly coherent human body that happens to be
   lying down for the entire animation, which is why it survived being looked at.
   *Caught by:* exporting a death for the first time and asking the only question a
   contact sheet cannot answer — does the body end up on the ground? (`pd_triage.py`
   exists because of this.) *Why it hid for a whole session:* the four bit lengths
   are per-animation, and `ANIM_TWO_GUN_HOLD` — the idle, the one clip every earlier
   render used — is authored dead still, so all four are **0** and the misalignment
   on that single animation was exactly zero. Walk and run were 28 and 29 bits out
   the whole time and still read as a person walking.
   ([24-root-motion-fix.png](tools/pd-assets/preview/24-root-motion-fix.png))
9. **The GPU only had room for 16 joints.** A PD body declares 30
   (`Bone_1..15` + `Blend_1..15`); `renderer::MAX_JOINTS` and
   `shader_skinned.wgsl` were sized for GoldenEye's 15. The CPU wrote the first 16
   matrices and WGSL clamped every out-of-range index onto the last one, so every
   blend-weighted vertex was skinned by `Bone_16` and the body drew as a fan of
   stretched black triangles converging on a point. Nothing headless could see it:
   `skinning_matrices` returns all 30, finite and correct, and `pd_preview.py` skins
   on the CPU with no such cap — *the defect existed only on the GPU path*. The
   blackness was a second, independent bug: a caller with no blood to paint passed an
   empty slice, which never resized the 1-vertex placeholder colour buffer, and an
   indexed draw reads past a short vertex buffer as zeros with no validation error.
   *Caught by:* the user running the game and saying there was a weird black moving
   thing attached to the crosshair.

**Lesson for whoever continues:** the numeric checks in this codebase are necessary but
not sufficient — and bug 9 sharpens that, because *no* CPU-side check of any kind could
have found it. Run the thing. Bugs 3, 4, 6 and 7 all passed every structural assertion. Put it on
screen — and then keep looking, because bug 7 survived being looked at from four angles
(it needed zooming into a single texture), and bug 6 survived every render *and* a
0.0006 mm engine-vs-verifier agreement, because both sides were consistently wrong about
the same asset. It was caught by a human glancing at the contact sheet and saying the
alien's legs looked wrong.

`pd_preview.py` exists so that "put it on screen" costs nothing — it re-reads a `.glb`
off disk and re-implements the glTF skinning math from the spec, sharing no code with
the exporter, so a mistake would have to be made twice in opposite directions to hide.
It renders GE bodies too, which is how the PD facing (+Z) and handedness were checked
against a known-good asset rather than against an opinion:

```sh
python tools/pd-assets/pd_preview.py native/assets/enemies/pd/characters/pd_a51guard.glb out.png \
    --clip native/assets/enemies/pd/animations/pd-running.glb --frames 6
python tools/pd-assets/pd_preview.py native/assets/enemies/characters/russian-guard_karl.glb ge.png \
    --clip native/assets/enemies/animations/00-idle.glb --highlight Bone_9
```

One level up again: `cargo run --release --example pd_pose_dump -- <body> <clip> 8 out/`
writes the poses **the engine itself computes** (through `gltf_skin::load` /
`clip::load` / `Skeleton::skinning_matrices`), and `pd_preview.py --positions` draws
those. The exporter and the Python renderer could in principle be self-consistently
wrong about the same asset; they cannot also agree with an independent Rust
implementation. Measured over all four bodies × all four clips, they agree to
**0.0006 mm** — float32 rounding, nothing else.

`--frame-radius` pins the camera instead of auto-framing, which is how two assets are
compared at true relative size (that is how the PD-vs-GE height gap below was seen).

## Textures

Character textures are stored two ways, and the display lists name them two ways:

| | Where the pixels are | How a triangle names it |
|---|---|---|
| **Inline** (a51guard, dd_shock, elvis, testchr) | in the model file, already-decompressed `RGBA5551` | `G_SETTIMG`, by segmented address |
| **Pooled** (every other body, **all 76 heads**) | `textures/`, compressed | **`0xC0`**, PD's own opcode, by pool texture number |

Both decode now. `pd_tex.py` handles the pool: **2,886 of 3,503** textures, which
covers **65 bodies and 65 heads fully**, a further 3 bodies and 11 heads all but a
texture or two. The rest are on PD's non-zlib codec (below).

**Opcode `0xC0` was the thing standing in the way**, and it fails silently: a pooled
model's display lists contain no `G_SETTIMG` at all (the game rewrites `0xC0` into
one at load, `tex_load_from_display_list`), so every triangle reported "no texture"
and fell back to flat colour with nothing to suggest a bug. It is not a guess —
across the 144 character models that emit it, **all 5,474 operands** are a key in
that model's own `texconfigs` table.

### The pool format

`[flags byte][payload]`, flags = `hasloddata`<<7 | `iszlib`<<6 | `numlods`
(`tex_load`, `texdecompress.c:2141`). The `iszlib` payload is a bit stream:
`u8 format`, `u8 numcolours-1`, `u16 palette[]`, then per image `u8 width`,
`u8 height` and an rzip stream (`0x11 0x73`, `u24` length, raw DEFLATE — `lib/rzip.s:223`,
the same codec START_HERE §4 documents).

Every `iszlib` texture is **paletted** — 2,462 `RGBA16_CI4`, 417 `RGBA16_CI8`, 7
`IA16_CI8` — which is why `tex_align_indices` only ever handles the CI cases.

**Pool data inflates linear.** `tex_swizzle` and the 8-byte row padding are applied
*after* inflation, on the way to the RDP — the exact opposite of the inline layout,
which is stored already-swizzled and has to be undone. Confirmed on 265 textures:
the inflated size is exactly `ceil(width/indicesperbyte) * height`, never padded.

### Still to port

`tex_inflate_non_zlib` (`texdecompress.c:699-1930`) — a different codec entirely
(Huffman, RLE, lookup tables), covering **616** pool textures. It costs a few
patches on a few heads today, so it is now a polish item rather than a blocker.
`decode` raises `UnsupportedTexture` for those and the exporter drops the affected
triangles to the debug palette.

Mip levels after 0 are ignored: PD regenerates them at load (`tex_shrink_*`).

**To unlock the other 62**, port the decode half of `game/texdecompress.c`
(~2,300 lines, but the `tex_shrink_*` mip generation is not needed):
`tex_inflate_zlib` — which is Rare's raw-deflate, already solved for `1173` in
START_HERE §4 — plus the `tex_inflate_non_zlib` path (`tex_inflate_huffman`,
`tex_inflate_rle`, `tex_build_lookup`, `tex_read_uncompressed`), then
`tex_channels_to_pixels` and `tex_swizzle`. The `struct tex` metadata is *not* in
the extracted `textures/*.bin` (they are payload only) — dimensions come from
`texconfig`, and the format from the texture table.
- **PD bodies are 1.73 m; GoldenEye bodies render 1.50 m** — `CHAR_SCALE` is
  `0.00104 × 0.8`, i.e. the GE bodies were deliberately shrunk 20%. **Resolved by
  keeping both.** `World::new` measures each body's standing height over its own idle
  and `World::body_capsule` / `body_hit_zones` scale the hit capsule and the
  head/torso/legs boundaries to it, calibrated so a 1.50 m body reproduces
  `ENEMY_RADIUS` / `ENEMY_HALF_HEIGHT` / `ZONE_HEAD_MIN` to the millimetre. Without
  it a PD hunter's head sat 23 cm above its own collider — unshootable, and the 4×
  headshot multiplier could never fire.
- **12 triangle edges exceed 25 cm** on the posed guard, all among the `G_TRI1` set.
  Probably legitimate (those triangles bridge bone groups at seams, so they span joints
  by design) but unverified. GE's Karl has 52 such edges, for scale.
- **616 pool textures are still on the non-zlib codec** (below) — a few flat patches
  on a few heads.

## The hunter clip set

A hunter needs the engine's fixed 36-slot layout — locomotion 0–3, one fire clip per
weapon class, 12 hit reactions, 17 deaths — and the combat code does arithmetic on
those indices (`CHAR_HIT_START + HIT_CLIPS.len()`). `World::pd_anim_template` fills
all 36 with Perfect Dark animations, chosen in `tools/pd-assets/pd_roster.json`.

**Perfect Dark and GoldenEye share one animation bank.** Rare carried the GoldenEye
character animations into PD at the *same numbers*, so the GE clip the game ships as
`2A-jogging.glb` is PD's `ANIM_002A`. That is measured, not assumed —
`pd_animmap.py` checks all 36 and 30 match frame-for-frame (143, 227, 245, 185
frames…), and the posed body traces the same posture curve turn for turn at the 1.15×
height ratio ([23-shared-animation-bank.png](tools/pd-assets/preview/23-shared-animation-bank.png)).
It also resolves the two places the banks disagree: PD's slot 0 is a null entry, so
idle sits at `0x01` (`ANIM_TWO_GUN_HOLD`), and GE ships `39-death-left-leg` for what
PD's own symbols call `ANIM_DEATH_STOMACH_LONG`.

Six GE deaths have no PD counterpart at their id — `ANIM_0016/0018/001B/001D/001E/001F`
are different animations that never leave their feet, confirmed by two independent
signals (frame count *and* `pd_triage.py`'s posture curve, which partition the same
six). Three of them are GoldenEye *mirrors*, which PD has no need of: `animtablerow`
carries a `flip` flag and the game mirrors at runtime. Those six slots are filled from
`g_DeathAnimsHuman*` (`game/chraction.c:228`) instead, so a PD hunter dies six ways a
GoldenEye guard cannot.

**Which clip is which was decided by looking, not by name.** `pd_triage.py` skins a
body across a clip and reports its standing height over time — tall throughout is
locomotion or a fire, a dip that recovers is an injury, tall-then-short-and-stays is a
death. That is exactly what a contact sheet cannot show, because `pd_preview.py`
frames every tile independently on purpose. All 36 shipped clips classify as their
role, including `21-death-stagger-back-to-wall` settling at 1.00 m rather than 0.35 —
correct, because it is PD's `g_AnimTableHumanSlumped`, a body slumped against a wall.

## After that

From [DESIGN_PD_WEAPON_MECHANICS.md](DESIGN_PD_WEAPON_MECHANICS.md) §10 — all the data
is located and verified, these are wiring jobs:

3. **Weapon attach.** `MODELPART_CHR_RIGHTHAND` (3) / `LEFTHAND` (5) resolve to real
   joints — now known to be anim parts 8 and 7, i.e. `Bone_9` and `Bone_8` on the
   exported rig. The gun's root transform is literally the hand matrix, no offset,
   which would delete the hand-tuned placement in `combat/enemy_weapons.rs`.

   **This turned out not to be a blocker for PD-bodied hunters, and the reason is
   worth knowing.** The expectation was that GoldenEye's hand-tuned bone-local
   offsets would put the gun somewhere wrong on a PD rig. They don't, because
   `spawn_wave` never assumed where the barrel ends up: it measures the real barrel
   direction in the chest frame from *that hunter's own* skeleton and aim pose
   (`EnemyArm::barrel_forward_in_chest`) and hands it to the chest-aim layer, which
   swings the hold until the barrel points at the player. Different rig, different
   measurement, same result on screen — see
   [25-pd-hunter-in-game.png](tools/pd-assets/preview/25-pd-hunter-in-game.png), and
   `world::tests::the_chest_aim_axis_is_measured_per_body`, which fails if the two
   families ever measure the same axis (i.e. if the calibration silently became a
   constant). PD's own rule still applies to PD's own gun models, which the hunters
   do not carry — they carry the GoldenEye arsenal.
4. **Muzzle flash + barrel origin** from the `CHRGUNFIRE` node on `props/chr*.bin`.
5. **`attackanimconfig`** (`game/chraction.c:912+`) — authored per-animation frame
   windows for shoot/aim/recoil, replacing the guessed `FIRE_TIMING`.
6. **Weapon stats** from `game/invitems.c`.
7. **Player viewmodel** — `guncmd` scripts + `files/guns/*`; `g_HeadsAndBodies[].handfilenum`
   gives the per-character first-person hands.

Also worth transcribing wholesale: `g_HeadsAndBodies` (`game/modeldata/robot.c:64`) —
per body it gives height in cm, scale, animscale and handfilenum.

## Related

The PD **simulant AI** is a separate, already-landed track — see
[DESIGN_PD_SIMULANT_AI.md](DESIGN_PD_SIMULANT_AI.md) §9 and the `pd-simulant-port`
memory. It runs behind the same `PD_LAB=1` flag and is independent of this asset work.
