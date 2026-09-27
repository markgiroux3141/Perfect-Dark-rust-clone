# SPIKE: Perfect Dark's Combat Simulator menus

Branch `spike/pd-combat-sim-menu`. A standalone window that runs Perfect Dark's own menu system: the Perfect Menu → Combat Simulator → every multiplayer setup dialog, drawn the way PD draws them, into PD's 320×220 framebuffer.

```
cargo run --release --bin pd_combat_sim                   # the menus (window)
cargo run --release --bin pd_combat_sim -- --combat       # start in the Combat Simulator
cargo run --release --bin pd_combat_sim -- --fresh        # a new save: challenges and unlockables locked
cargo run --release --bin pd_combat_sim_snapshot -- <outdir> [--fresh] [--combat] <script...>   # headless PNGs
```

The snapshot script is a list of steps:

| Step | Meaning |
|---|---|
| `wN` | wait N frames |
| `up` `down` `left` `right` `a` `b` `z` `start` `l` `r` `cu` `cd` `cl` `cr` | tap a button |
| `p2start` `p3start` `p4start` | press START on controller 2 / 3 / 4 |
| `sxN` / `syN` | hold the stick at N (e.g. `sx60`, `sx0`) |
| `bs` | backspace (name keyboard) |
| `shot:name` | write `name.png` |

For example, the character select:

```
--combat w40 down down down a w50 right w30 down a w80 shot:char
```

## Keys (window)

| Key | N64 button |
|---|---|
| Arrows / WASD | D-pad |
| Enter | A |
| Esc | B |
| Space | START |
| Z | Z |
| Q / E | L / R |
| Backspace | delete, in the name keyboard |

- F1 opens the panel: save profile, restart, the frame rate (60/30/20 Hz), RGBA5551 output, and which controller the keyboard drives.
- F2–F4 press START on controllers 2–4, to join players.
- A USB N64 pad works too, through the engine's gamepad layer.

## The ROM

`D:\GoldenPerfectModding\Perfect Dark (U) (V1.1)` has the same MD5 (`e03b088b6ac9e0080440efed07c1e40f`) as the decomp's `pd.ntsc-final.z64`. Everything the spike needs is therefore already in the decomp's extracted assets: fonts, lang banks, textures, character files, `mpconfigs.bin`, `mpstringsE.bin` and sounds. Nothing is read from the ROM at runtime.

## What is ported

Everything is from the decomp (NTSC final), function by function under PD's names, with `file:line` citations.

| Area | PD source | Rust (`pd_menu/`) |
|---|---|---|
| Dialog engine: layers, siblings, rows/cols/blocks, focus, open/close/push/pop/replace, dialog states PREOPEN → POPULATED and the diagonal redraw, swipe, scroll, scissor, key repeat, multi-player splits, joining | `menu.c` | `menu.rs` |
| Menu items: list, dropdown + overlay, keyboard, separator, label, selectable, slider, carousel, checkbox, scrollable, marquee, player stats | `menuitem.c` | `menuitem.rs` |
| Every Combat Simulator dialog and handler: quick start, advanced setup, arenas, scenarios and their options, weapons and weapon sets, simulants, teams, handicaps, challenges, music, player setup/stats, name keyboard | `mplayer/setup.c`, scenario `.inc`s, `mainmenu.c` | `generated.rs` (tables), `handlers.rs` |
| MP state: configs, chrs/bots, presets, locks, challenges and unlocks | `mplayer.c`, `challenge.c` | `mp.rs` |
| Text: fonts, v2 renderer + v1 glow shadow, diagonal/menu/wave/horizontal blends, per-character holorays | `text.c` | `text.rs` |
| Menu graphics: borders, gradients, shimmer, sliders, chevrons, checkboxes, blurred backdrop, the two rotating cones, holoray planes | `menugfx.c` | `menugfx.rs` |
| 3D menu models: character select body + head (animated, zooming full body → face), head-only carousel, the hudpiece "eye" with its unfold animation, spinning rotor, scrolling liquid, and the holoray origin | `menu.c:1719` `menu_render_model`, `model.c`, `body.c` | `model.rs`, `pdmodel.rs` |
| Menu sounds | `menu_play_sound` | `app.rs` |

The rasteriser (`gfx.rs` for 2D, `pdmodel.rs` for models) is a CPU stand-in for the RSP and RDP:
- PD's ortho and holoray matrices;
- the top-left fill rule;
- the 3-point bilinear filter;
- the `XLU` blender;
- for models: RSP lighting, texgen, the literal two-cycle combiner, TRILERP mips and the texture-edge alpha compare.

## Assets

Two scripts regenerate everything. The outputs are committed.

```
python tools/pd-assets/pd_menu_gen.py      # generated.rs + fonts, lang_en.json, mpconfigs.bin, mpstringsE.bin
python tools/pd-assets/pd_menu_models.py   # models/: 137 bodies/heads/hudpiece (.pdm), 1040 textures, ANIM_01FC + ANIM_040D
```

- **Menu textures:** `native/assets/pd_menu/textures/` (menuray, envstar, etc.), via `pd_tex.py`.
- **Menu sounds:** `native/assets/audio/pd/menu_sfx/`, via `pd_sfx.py export`. This is separate from the gun pack.
- **Optional backdrop:** drop `native/assets/pd_menu/bg_source.png` to blur a real backdrop. Without it the blur is a dim gradient, which stands in for the Carrington Institute scene PD blurs.

## Not in the spike

- **Controller Pak / file manager:** these flows show a stub dialog, and there are no camera-made "perfect heads".
- **Solo, Co-op and Counter-op:** a stub dialog ("not in this spike").
- **Starting a match:** shows a summary of the chosen setup, then returns to the menus.
- **Music:** none plays.
- **Anti-aliasing:** coverage AA and the RDP's exact edge walker are not emulated.
- **Texture detail:** mip levels are box-filtered from level 0, with the LOD chosen per triangle.

## Measured facts

Each of these overturned a plausible assumption during the build:

- **Every chr body uses PD's elbow/knee helper matrices** (`MODELNODETYPE_0100`: half-rotation joints). The gun spike's model walker skips them, so the menus carry their own walker (`pdmodel.rs`) and reuse only the gun spike's animation code.
- **A head has no matrices of its own.** Its display lists load matrix 0 from the *body's* matrix segment, which is the headspot's parent joint.
- **The menu model context turns fog off:** `G_RM_PASS` in 1-pass, fog `0xffffff00` in 2-pass. The exporter's `fog_tint` flag (from the gun context) must be ignored here, or everything washes to white.
- **The dialog text fading out and back in every few seconds is PD** (`menu.c:4032`). A populated dialog holds its text for 2 s, then the redraw timer runs its diagonal wipe again.
- **The dialog model's screen offset goes through the full-screen camera, but it projects into the scissor viewport** (`cam0f0b4c3c` before `vi_set_fov_aspect_and_size`). Its offsets therefore shrink, as in PD.
- **The binaries locate `native/assets/pd_menu` relative to the executable,** with a fallback to the compile-time path. A build made in a scratch worktree still finds the repo's assets.
