# Handoff — Perfect Dark weapons: **PARKED** (2026-08-18)

**Decision (user, 2026-08-18): the PD guns go on the backburner.** The remaining texture
problems plus PD's much more complex gun dynamics are not worth chasing right now. The game
ships with **GoldenEye guns for the player and for the hunters, and Perfect Dark bodies** —
which is already the default and needed no code change to arrange.

This document is the resume point. It is deliberately blunt about what is unfinished.

## What ships today

```powershell
.\native\target\release\build-and-hide.exe          # GE guns, PD bodies — the default
$env:ARSENAL = "pd"; .\native\...\build-and-hide.exe  # the parked PD arsenal, opt-in
```

* `ARSENAL` defaults to **GoldenEye** (`arsenal.rs`'s `from_env`), and the hunter roster
  follows it — `enemy_roster_for` only returns the PD picks under `ARSENAL=pd`, so hunters
  hold the tuned GoldenEye six. The resolved arsenal is logged at boot, so what you are
  looking at is never a guess.
* **PD bodies and the PD hunter AI are unaffected** — they are selected separately
  (`BODIES=`, and the AI promotion out of `PD_LAB`). A PD body holds a GoldenEye gun because
  the PD rig is exported onto GoldenEye's bone names, so `Bone_9`/`Bone_8` attach exactly as
  they do for a GoldenEye body. Nothing on the default path reads the PD weapon table.
* `ARSENAL=pd` still works and is not deprecated. Everything below stays reachable.

## What is DONE, and worth not redoing

The PD arsenal is playable, and this session's fidelity pass fixed most of what playtest
reported. `git log` has the detail; the short version:

* **33 guns** transcribed from the decomp with provenance, two firing functions each, both
  models per gun (first-person + the `chr*` a hunter holds), PD's explosion model.
* **Textures**: 1,063 palette-fallback triangles → 61. Every first-person gun and every
  muzzle flash is real, sourced from the Perfect Gold dump where our codec cannot decode.
* **The muzzle flash** is a soft flame (its four textures are a 2×2 tiling of one image).
* **Env mapping**: reflection maps are matcapped rather than painted flat; `PerMaterial`
  scope for PD so a single env texture does not wash out a whole gun.
* **`invaimsettings`**: the gun slides with the crosshair (3 / 8 / 15, global not per-weapon).
* **`gunviscmds`**, authored-untextured triangles, transparent-texel discard, and
  `every_shader_compiles` — see the previous section of `git log`.

## What is KNOWN-BROKEN or unfinished

Texture-side, in the order they are likely to be what you noticed on screen:

1. **61 triangles still render the debug palette**, all on five third-person `chr*` models —
   MagSec 4 (36 tris), Shotgun (15), DY357 (3), DY357-LX (1), and one more. Their pool
   textures (`0x934`, `0x96a`–`0x97d`) were never in the editor dump, which was a
   first-person export. **Fix: port `tex_inflate_non_zlib`** (193 lines,
   `game/texdecompress.c:699`, called from `:2263`) and the pipeline stops needing the dump
   at all.
2. **Partial alpha is drawn opaque.** The gun pass discards only fully transparent texels;
   anything in between (PD's `Transparent*` materials — decals, glass) renders solid. PD
   blends them. Needs either a cutout threshold or a second, blended pass.
3. **The matcap gain `1.6` (`shader_viewmodel.wgsl`) is a GoldenEye number**, tuned for gold
   and chrome guns with black bases. It has never been judged against PD's metals; the
   Falcon 2 may well be too bright.
4. **Texture `0x606` disagrees between sources** (4×16 texconfig vs 8×16 in the dump). UVs
   follow the shipped image, which is the defensible choice, but only one of the two can be
   what PD used.
5. **The specific defects the user saw on 2026-08-18 were not captured.** A screenshot of
   whichever gun looks wrong is worth more than any amount of re-derivation here — the last
   three "obvious" diagnoses on this track were all wrong.

Mechanics-side:

6. **Gun dynamics are the real gap, and they are structural.** PD's viewmodel is an
   *articulated* model driven by the `guncmd` bytecode: a slide that cycles, a magazine that
   leaves the gun, part visibility keyed to animation frames, three reload sounds at authored
   times, and `allowfeature(ATTACKAGAIN)` as the authored answer to "when can you fire
   again". Ours is one static mesh with a dip. **The bytecode is 12 opcodes and small; the
   articulated export is the gate.** The exports were built for it — part offsets are baked
   into the geometry rather than dropped, so articulation is a **re-export, not a re-rip**
   (`export_gun` already takes `only_parts`).
7. **Sounds are still GoldenEye's.** The real `funcdef_shoot.shootsound` ids are transcribed
   and unused. Needs a **VADPCM decoder** for `sfx.ctl`/`sfx.tbl`
   (`reference/pd-decomp/src/assets/ntsc-final/`), then `SFXMAP_*` → bank → sample → WAV.
   Self-contained; touches nothing else. This was the user's pick for "next" before the park.
8. **`invaimsettings.zoomfov` is unported** — per-weapon scope zoom (MagSec 4 25°, AR34 and
   K7 20°, Falcon 2 scope and the heavies 30°, against a 60° default). It is the only
   genuinely per-weapon field in that table.
9. **Placement is PD's authored `posx/posy/posz`, never eyeballed.** The open question the
   park suspends: hand-tune the 33 like the GoldenEye set, or keep PD's? The scale question
   is settled — 0.0007 is right (AR34 0.778 m vs GoldenEye's AR33 0.776 m).
10. **The Reaper's alternating flash** (`MUZZLEFLASH2/3`, same centroid, one gun only) needs
    a runtime that holds more than one flash mesh per weapon.

## How to resume

```
python tools/pd-assets/pd_weapons.py json tools/pd-assets/pd_weapons.json
python tools/pd-assets/pd_gltf.py guns tools/pd-assets/pd_weapons.json native/assets/weapons/pd
python tools/pd-assets/pd_preview.py <gun.glb> out.png --viewmodel    # see it without launching
cargo test --release
```

`pd_preview.py --viewmodel` transliterates `shader_viewmodel.wgsl` (matcap, `ENV_GAIN` = the
shader's 1.6, base-colour factor, transparent-texel discard), so a rendering decision can be
made from a PNG. It is necessary but not sufficient — it cannot see placement, scale in the
hand, or anything the engine does per frame.

**Suggested order if this is picked back up:** (7) sounds, because it is isolated and the ids
are already sitting there → (1) the codec port, which retires the dump dependency → (6)
articulated export + `guncmd`, which is the big one and unlocks reloads, part visibility and
fire timing together.

## Traps that keep firing on this track

1. **CPU-side green does not mean it works.** Every defect reported by playtest passed the
   whole suite. Hand off a playtest brief; never drive the game yourself.
2. **A test can fail because of the fix.** Teach it the new fact rather than reverting.
3. **Check the data arrived before theorising about the renderer.** Three separate symptoms
   ("white square flash", "dull blue-grey metal", "the Mauler looks off") were all missing or
   misused *data*. None was a renderer bug.
4. **Verify a claim against the assets before building on it.** Three claims in the previous
   handoff were wrong — no gun model carries a hand at all; `MUZZLEFLASH2/3` are the Reaper's
   only; `PD_VIEW_SCALE` was already correct — and each took one measurement to settle and
   would have cost a session to act on.

## Context

* `HANDOFF_PD_WEAPONS.md` — what the first pass shipped.
* `DESIGN_PD_WEAPON_MECHANICS.md` — §2 (no attach offset), §3 (two models per gun), §8 (`guncmd`).
* `tools/pd-assets/pd_gltf.py` — `editor_textures`, `read_bmp`, `resolve_texture`, `export_gun`.
* `combat/arsenal.rs` — the bridge, with the reasoning for every conversion.
* `tools/pd-assets/preview/28..30` — the textured arsenal, the flash, the env-map A/B.
* Memory: `pd-arsenal-decisions`, `pd-weapon-mechanics`, `pd-editor-dump-textures`.
